// Discord's drags in the channel list: people between voice channels (you, or anyone with Move
// Members), and channels to reorder them or move them between categories (Manage Channels).
// Both apply at once and roll back if Discord refuses.
//
// What's carried follows design/flows-voice.html: a ghost of the row trails the pointer on a loose
// spring, tilting with horizontal speed and squashing with vertical speed, while a faint outline
// holds its old place. Let go, and a stiffer spring carries it into wherever its row now is.
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, Bounds, BoxShadow, Context, Div, Entity, FontWeight, IntoElement, Pixels, Point, Radians, Render, SharedString, SizeRefinement, SpringConfig,
    SpringState, WeakEntity, Window, anchored, canvas, component::Icon, deferred, div, point, prelude::*, px, relative,
};
use reqwest::Method;
use serde_json::json;

use crate::{
    Waffle,
    sortable::{Finished, Place},
    discord::{Channel, ChannelKind, perm},
    icons::Glyph,
    palette,
    sidebar::{Face, channel_glyph, render_face},
    ui::{self, Squircle},
};

// Someone being dragged out of a voice channel.
#[derive(Clone)]
pub struct VoiceDrag {
    pub user_id: String,
    pub guild_id: String,
    pub name: SharedString,
    pub avatar: String,
}

// A channel being dragged to a new place.
#[derive(Clone)]
pub struct ChannelDrag {
    pub channel_id: String,
    pub guild_id: String,
}

// A pinned channel, which refuses to move: picking it up asks to unpin it instead, and nothing
// accepts it as a drop.
#[derive(Clone)]
pub struct PinnedDrag;

// What a refused drag carries: nothing.
pub struct NoGhost;

impl Render for NoGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::Empty
    }
}

// CARRY

// The ghost's physics: position, tilt and lift, each a damped spring stepped once a frame.
pub struct Carry {
    x: SpringState,
    y: SpringState,
    tilt: SpringState,
    lift: SpringState,
    target: Point<Pixels>,
    // How far it swells while held, and the tilt it rests at (rail tiles hang at -4°, .srv.dragging).
    lifted: f32,
    lean: f32,
    landing: bool,
    stepped_at: Instant,
}

// Held: stiff and a touch under critical, so the ghost keeps up with the pointer (~100ms) with a slight
// settle; a looser spring read as the drag lagging. The tilt is softer.
const FOLLOW: (f32, f32) = (1400., 0.78);
const FOLLOW_TILT: (f32, f32) = (260., 0.55);
// Landing: stiffer and nearly critical, so it glides in (~250ms) without bouncing off the row.
const LAND: (f32, f32) = (900., 0.82);
const LAND_TILT: (f32, f32) = (900., 0.9);
// Degrees of tilt per px/s of horizontal speed, and its limit.
const TILT_PER_SPEED: f32 = 0.018;
const MAX_TILT: f32 = 14.;
// px/s of vertical speed per unit of squash, and its limit.
const SQUASH_SPEED: f32 = 9000.;
const MAX_SQUASH: f32 = 0.08;
// A long frame steps as one this long, so a hitch can't fling the ghost.
const MAX_STEP: f32 = 0.032;

fn spring((stiffness, ratio): (f32, f32)) -> SpringConfig {
    SpringConfig::new(stiffness, 2. * stiffness.sqrt() * ratio, 1.)
}

fn at_rest(value: f32) -> SpringState {
    SpringState { position: value, velocity: 0. }
}

impl Carry {
    pub fn new(origin: Point<Pixels>, lifted: f32, lean: f32) -> Self {
        Self {
            x: at_rest(origin.x.into()),
            y: at_rest(origin.y.into()),
            tilt: at_rest(0.),
            lift: at_rest(1.),
            target: origin,
            lifted,
            lean,
            landing: false,
            stepped_at: Instant::now(),
        }
    }

    pub fn follow(&mut self, target: Point<Pixels>) {
        self.target = target;
    }

    pub fn land(&mut self) {
        self.landing = true;
    }

    // Steps to now; true once it has landed and come to rest.
    pub fn step(&mut self) -> bool {
        let now = Instant::now();
        let dt = now.duration_since(self.stepped_at).as_secs_f32().min(MAX_STEP);
        self.stepped_at = now;
        self.advance(dt)
    }

    fn advance(&mut self, dt: f32) -> bool {
        let (travel, turn) = match self.landing {
            true => (spring(LAND), spring(LAND_TILT)),
            false => (spring(FOLLOW), spring(FOLLOW_TILT)),
        };
        self.x = travel.step(self.x, self.target.x.into(), dt);
        self.y = travel.step(self.y, self.target.y.into(), dt);
        let lean = match self.landing {
            true => 0.,
            false => self.lean + (self.x.velocity * TILT_PER_SPEED).clamp(-MAX_TILT, MAX_TILT),
        };
        self.tilt = turn.step(self.tilt, lean, dt);
        self.lift = travel.step(self.lift, if self.landing { 1. } else { self.lifted }, dt);
        self.landing && self.settled()
    }

    fn settled(&self) -> bool {
        let (dx, dy) = (self.x.position - f32::from(self.target.x), self.y.position - f32::from(self.target.y));
        dx.hypot(dy) < 1. && self.x.velocity.hypot(self.y.velocity) < 90. && self.tilt.position.abs() < 0.6 && (self.lift.position - 1.).abs() < 0.008
    }

    fn at(&self) -> Point<Pixels> {
        point(px(self.x.position), px(self.y.position))
    }

    // Squashed by vertical speed: narrower and taller while it drops or rises fast.
    fn scale(&self) -> (f32, f32) {
        let squash = (self.y.velocity.abs() / SQUASH_SPEED).min(MAX_SQUASH);
        (self.lift.position * (1. - squash), self.lift.position * (1. + squash))
    }
}

// HAND

// What the ghost looks like: a copy of the row it was lifted from.
#[derive(Clone)]
pub enum Look {
    Person { avatar: String, name: SharedString },
    Channel { channel: Box<Channel>, people: Vec<(String, SharedString)> },
    Tile(Face),
    // A settings row: a colour dot and a name (a role).
    Label { dot: gpui_kit::Hsla, text: SharedString },
    // A Favorites category header: its caret and name.
    Category(SharedString),
}

// One carried row, shared by the drag's ghost and the app, which lands it.
pub struct Held {
    // The element id of the row it came from, which is also the row it lands on.
    key: SharedString,
    look: Look,
    size: Point<Pixels>,
    // gpui draws a drag's view at the pointer less this offset, the row's own origin.
    offset: Point<Pixels>,
    carry: Carry,
    // Beside the ghost: what letting go here will do.
    tag: Option<SharedString>,
    // A sortable list's row stays in its column: the x it was picked up at.
    lock_x: Option<Pixels>,
}

type Hand = Rc<RefCell<Held>>;

// The carried row, the one landing, and where their rows are on screen.
#[derive(Default)]
pub struct DragMotion {
    hand: Option<Hand>,
    landing: Option<Landing>,
    spots: Rc<RefCell<HashMap<SharedString, Bounds<Pixels>>>>,
    // The voice channel a person is held over, and which of its rows the pointer is on.
    pub voice_target: Option<(String, SharedString)>,
}

struct Landing {
    hand: Hand,
    since: Instant,
    // Set once its row turns out to be gone (they left, or it went into a closed folder).
    puff: Option<Instant>,
}

// How the carried row's own place draws.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RowDrag {
    Rest,
    // .is-hole: a faint outline of where it was.
    Hole,
    // Under the landing ghost, until the two swap.
    Hidden,
}

// How long the landing waits for its row to paint before puffing the ghost away instead.
const FIND_ROW: Duration = Duration::from_millis(60);
const PUFF: Duration = Duration::from_millis(220);
// The ghost fades into the row's own look as it glides in.
const SETTLE_LOOK: Duration = Duration::from_millis(180);

impl DragMotion {
    pub fn row(&self, key: &str) -> RowDrag {
        let carries = |hand: &Hand| hand.borrow().key.as_ref() == key;
        match (&self.hand, &self.landing) {
            (Some(hand), _) if carries(hand) => RowDrag::Hole,
            (_, Some(landing)) if carries(&landing.hand) => RowDrag::Hidden,
            _ => RowDrag::Rest,
        }
    }

    pub fn set_tag(&self, tag: Option<SharedString>) {
        if let Some(hand) = &self.hand {
            hand.borrow_mut().tag = tag;
        }
    }

    // Measures the carried row's place as it paints, so the landing knows where to go.
    pub fn spot(&self, key: &str) -> Option<AnyElement> {
        if self.row(key) == RowDrag::Rest {
            return None;
        }
        let (spots, key) = (self.spots.clone(), SharedString::from(key.to_owned()));
        Some(canvas(move |bounds, _, _| spots.borrow_mut().insert(key, bounds), |_, _, _, _| {}).absolute().inset_0().into_any_element())
    }
}

// .is-hole: the place a carried row left, a faint 1.5px outline at 32%; the caller leaves it empty.
pub fn hole<E: Styled>(place: E, cx: &App) -> E {
    place.opacity(0.32).border(px(1.5)).border_color(palette::faint(cx).opacity(0.55))
}

// Starts a carry as gpui starts the drag: the ghost is the drag's view, and the app keeps a hand on
// it to land it once let go. `size` is the row's; `lifted` and `lean` as for `Carry`.
pub fn pick_up<T: 'static>(
    this: WeakEntity<Waffle>,
    key: SharedString,
    look: Look,
    size: Point<Pixels>,
    lifted: f32,
    lean: f32,
    axis_locked: bool,
) -> impl Fn(&T, Point<Pixels>, &mut Window, &mut App) -> Entity<Ghost> + 'static {
    move |_, offset, window, cx| {
        let origin = window.mouse_position() - offset;
        let carry = Carry::new(origin, lifted, lean);
        let hand = Rc::new(RefCell::new(Held { key: key.clone(), look: look.clone(), size, offset, carry, tag: None, lock_x: axis_locked.then_some(origin.x) }));
        this.update(cx, |this, cx| {
            this.drag_motion.hand = Some(hand.clone());
            this.drag_motion.landing = None;
            cx.notify();
        })
        .ok();
        cx.new(|_| Ghost { hand })
    }
}

// The ghost while held, which gpui draws at the pointer; it trails there on its spring.
pub struct Ghost {
    hand: Hand,
}

impl Render for Ghost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::perf::probe("Ghost");
        let mut held = self.hand.borrow_mut();
        let anchor = window.mouse_position() - held.offset;
        let target = held.lock_x.map_or(anchor, |x| point(x, anchor.y));
        held.carry.follow(target);
        held.carry.step();
        window.request_animation_frame();
        ghost(&held, anchor, 0., cx)
    }
}

// The ghost at its pose. `origin` is where layout put it; `settle` how far it has faded into the
// row's own look (0 lifted, 1 the row).
fn ghost(held: &Held, origin: Point<Pixels>, settle: f32, cx: &App) -> Div {
    let lifted = 1. - settle;
    let body = match &held.look {
        Look::Tile(face) => div()
            .pl(px(crate::sidebar::TILE_X))
            .child(div().squircle(ui::RADIUS_LG).shadow(ui::shadow_pop_on(palette::line_strong(cx).opacity(lifted), cx)).child(render_face(face.clone(), crate::sidebar::TILE, ui::RADIUS_LG, false, 0., cx))),
        Look::Person { avatar, name } => row_card(ui::RADIUS_SM, lifted, cx).child(person(avatar, name)),
        Look::Channel { channel, people } => row_card(ui::RADIUS_MD, lifted, cx)
            .flex_col()
            .gap_px()
            .child(div().h_8().px_2().flex().items_center().gap_2().child(channel_glyph(channel).size(ui::ICON_LG).flex_none().text_color(palette::faint(cx))).child(div().flex_1().min_w_0().truncate().child(channel.name.clone())))
            .children(people.iter().map(|(avatar, name)| person(avatar, name).text_color(palette::muted(cx)))),
        Look::Label { dot, text } => row_card(ui::RADIUS_MD, lifted, cx)
            .h(held.size.y)
            .px_4()
            .items_center()
            .gap_3()
            .text_size(ui::TEXT_SM)
            .child(div().size_3().flex_none().rounded_full().bg(*dot))
            .child(div().flex_1().min_w_0().truncate().child(text.clone())),
        Look::Category(name) => row_card(ui::RADIUS_SM, lifted, cx)
            .h(held.size.y)
            .px_1p5()
            .items_center()
            .gap_1()
            .text_size(ui::TEXT_XS)
            .font_weight(FontWeight::MEDIUM)
            .child(crate::sidebar::Nav::Caret.icon().size(px(11.)))
            .child(div().flex_1().min_w_0().truncate().child(name.clone())),
    };
    let (sx, sy) = held.carry.scale();
    let grab = point(relative(f32::from(held.offset.x) / f32::from(held.size.x)), relative(f32::from(held.offset.y) / f32::from(held.size.y)));
    let mut ghost = div()
        .relative()
        .w(held.size.x)
        .font_weight(FontWeight::NORMAL)
        .text_size(ui::TEXT_MD)
        .child(body)
        .children(held.tag.clone().filter(|_| settle == 0.).map(|tag| tag_chip(tag, cx)))
        .translate(held.carry.at() - origin)
        .rotate(Radians(held.carry.tilt.position.to_radians()))
        .transform_origin(grab);
    ghost.style().scale = SizeRefinement { width: Some(sx), height: Some(sy) };
    ghost
}

// .vd-ghost: the overlay surface under --lift, with the row's own layout inside.
fn row_card(radius: f32, lifted: f32, cx: &App) -> Div {
    div().w_full().flex().squircle(radius).bg(palette::surface(palette::Level::Overlay, cx).opacity(lifted)).shadow(lift_shadow(lifted, cx)).text_color(palette::strong(cx))
}

fn person(avatar: &str, name: &SharedString) -> Div {
    div().h(px(30.)).w_full().pl_8().pr_2().flex().items_center().gap_2().child(ui::avatar(avatar.to_owned(), 22.)).child(div().flex_1().min_w_0().truncate().child(name.clone()))
}

// --lift: 0 10px 28px black at 32% over a 1px line_strong ring, both fading as it lands.
fn lift_shadow(lifted: f32, cx: &App) -> Vec<BoxShadow> {
    vec![
        BoxShadow { color: gpui_kit::black().opacity(0.32 * lifted), offset: point(px(0.), px(10.)), blur_radius: px(28.), spread_radius: px(0.), inset: false },
        BoxShadow { color: palette::line_strong(cx).opacity(lifted), offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(1.), inset: false },
    ]
}

// .vd-tag: a dark chip 10px off the ghost's right edge, easing in from the left each time it changes.
fn tag_chip(tag: SharedString, cx: &App) -> AnyElement {
    div()
        .absolute()
        .left(relative(1.))
        .top(relative(0.5))
        .mt(px(-11.))
        .ml_2p5()
        .h(px(22.))
        .px_2()
        .squircle(ui::RADIUS_XS)
        .flex()
        .items_center()
        .gap_1()
        .whitespace_nowrap()
        .text_size(ui::TEXT_XS)
        .font_weight(FontWeight::MEDIUM)
        .bg(palette::strong(cx))
        .text_color(palette::surface(palette::Level::Page, cx))
        .child(Icon::new(Glyph::ArrowRight).size(ui::ICON_SM))
        .child(tag.clone())
        .with_animation(tag, Animation::new(ui::DUR_POP).with_easing(gpui_kit::ease_out_quint()), |chip, t| chip.opacity(t).translate_x(px(-4. * (1. - t))).scale(0.95 + 0.05 * t))
        .into_any_element()
}

impl Waffle {
    // Once gpui has let go of a drag (dropped on a target or on nothing), what was carried lands
    // on its row: in its new place if the drop moved it, back home if not.
    pub(crate) fn settle_carry(&mut self, cx: &App) {
        if cx.has_active_drag() {
            return;
        }
        let Some(hand) = self.drag_motion.hand.take() else { return };
        {
            let mut held = hand.borrow_mut();
            held.carry.land();
            held.tag = None;
            self.drag_motion.spots.borrow_mut().remove(&held.key);
        }
        self.drag_motion.voice_target = None;
        self.drag_motion.landing = Some(Landing { hand, since: Instant::now(), puff: None });
    }

    // The landing ghost, over everything, until it comes to rest on its row.
    pub(crate) fn render_landing(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.settle_carry(cx);
        let landing = self.drag_motion.landing.as_mut()?;
        let hand = landing.hand.clone();
        let mut held = hand.borrow_mut();
        let spot = self.drag_motion.spots.borrow().get(&held.key).copied();
        match spot {
            Some(bounds) => held.carry.follow(bounds.origin),
            None if landing.puff.is_none() && landing.since.elapsed() > FIND_ROW => landing.puff = Some(Instant::now()),
            None => {}
        }
        let puffed = landing.puff.map(|at| (at.elapsed().as_secs_f32() / PUFF.as_secs_f32()).min(1.));
        let settle = (landing.since.elapsed().as_secs_f32() / SETTLE_LOOK.as_secs_f32()).min(1.);
        if held.carry.step() || puffed == Some(1.) {
            drop(held);
            self.drag_motion.landing = None;
            cx.notify();
            return None;
        }
        window.request_animation_frame();
        let mut ghost = ghost(&held, Point::default(), settle, cx);
        if let Some(t) = puffed {
            let ease_in = t * t * t;
            ghost = ghost.opacity(1. - ease_in).scale(1. - 0.6 * ease_in);
        }
        // lint-allow(raw-floating): a drag ghost must not occlude the drop targets under it.
        Some(deferred(anchored().position(Point::default()).child(ghost)).with_priority(2).into_any_element())
    }
}

impl Waffle {
    pub(crate) fn can_move_voice(&self, guild_id: &str, user_id: &str, to: &str) -> bool {
        let (Some(me), Some(guild)) = (self.me.as_ref(), self.guilds.iter().find(|g| g.id == guild_id)) else { return false };
        guild.channel(to).is_some_and(|to| guild.can_move(&me.id, user_id, to))
    }

    pub(crate) fn can_arrange_channels(&self, guild_id: &str) -> bool {
        let (Some(me), Some(guild)) = (self.me.as_ref(), self.guilds.iter().find(|g| g.id == guild_id)) else { return false };
        guild.can(&me.id, perm::MANAGE_CHANNELS)
    }

    // Drop someone on a voice channel: we join it ourselves, or move them there.
    pub(crate) fn drop_voice_user(&mut self, drag: &VoiceDrag, channel: Channel, cx: &mut Context<Self>) {
        if !self.can_move_voice(&drag.guild_id, &drag.user_id, &channel.id) {
            return;
        }
        let previous = self.voice_channel_of(&drag.guild_id, &drag.user_id);
        let Some(target) = voice_move_target(previous.as_deref(), &channel.id).map(str::to_owned) else { return };
        let me = self.me.as_ref().is_some_and(|m| m.id == drag.user_id);
        if me {
            return self.join_voice(channel, cx);
        }
        let (guild_id, user_id) = (drag.guild_id.clone(), drag.user_id.clone());
        let (g1, u1, g2, u2) = (guild_id.clone(), user_id.clone(), guild_id.clone(), user_id.clone());
        self.rest_optimistic(
            Method::PATCH,
            format!("/guilds/{guild_id}/members/{user_id}"),
            Some(json!({ "channel_id": target })),
            move |this| this.set_voice_channel(&g1, &u1, Some(target)),
            move |this| this.set_voice_channel(&g2, &u2, previous),
            cx,
        );
    }

    fn voice_channel_of(&self, guild_id: &str, user_id: &str) -> Option<String> {
        self.guilds.iter().find(|g| g.id == guild_id)?.voice_states.iter().find(|v| v.user_id == user_id)?.channel_id.clone()
    }

    fn set_voice_channel(&mut self, guild_id: &str, user_id: &str, channel: Option<String>) {
        if let Some(state) = self.guilds.iter_mut().find(|g| g.id == guild_id).and_then(|g| g.voice_states.iter_mut().find(|v| v.user_id == user_id)) {
            state.channel_id = channel;
        }
    }

    // Sortable drags let go of (sortable.rs): each board applies the move its list was showing, before
    // the lists render, so what was carried lands in its new place.
    pub(crate) fn finish_sorts(&mut self, cx: &mut Context<Self>) {
        if let Some(done) = self.rail_board.finish(cx) {
            self.finish_rail_sort(done, cx);
        }
        if let Some(done) = self.channel_board.finish(cx) {
            self.finish_channel_sort(done, cx);
        }
        if let Some(done) = self.role_board.finish(cx) {
            self.finish_role_sort(done, cx);
        }
        if let Some(done) = self.favorite_board.finish(cx) {
            self.finish_favorite_sort(done, cx);
        }
    }

    // A channel let go of over a category: beside a channel, or first in it from its header.
    fn finish_channel_sort(&mut self, done: Finished, cx: &mut Context<Self>) {
        let (Some(hint), Some(guild_id)) = (done.hint, self.guild_id.clone()) else { return };
        let Some(guild) = self.guilds.iter().find(|g| g.id == guild_id) else { return };
        let target = match &hint.target {
            Some(id) => guild.channels.iter().find(|c| c.id == *id),
            None => guild.channels.iter().find(|c| crate::sidebar::channel_list(Some(c)) == hint.list),
        };
        let Some(target) = target.cloned() else { return };
        self.drop_channel(&ChannelDrag { channel_id: done.dragged, guild_id }, &target, hint.place, cx);
    }

    // Drop a channel above or below another, or into a category: renumber that category's
    // channels and send Discord the new order in one request.
    pub(crate) fn drop_channel(&mut self, drag: &ChannelDrag, target: &Channel, place: Place, cx: &mut Context<Self>) {
        if drag.channel_id == target.id {
            return;
        }
        let Some(guild) = self.guilds.iter().find(|g| g.id == drag.guild_id) else { return };
        let Some(moved) = guild.channels.iter().find(|c| c.id == drag.channel_id).cloned() else { return };
        // Categories only move among categories; other channels land in the target's category.
        let parent = match (moved.kind, place) {
            (ChannelKind::Category, _) => None,
            (_, Place::Into) => Some(target.id.clone()),
            _ => target.parent_id.clone(),
        };
        let siblings: Vec<(String, i64)> = guild
            .channels
            .iter()
            .filter(|c| (c.kind == ChannelKind::Category) == (moved.kind == ChannelKind::Category) && (moved.kind == ChannelKind::Category || c.parent_id == parent))
            .map(|c| (c.id.clone(), c.position))
            .collect();
        let anchor = if place == Place::Into { None } else { Some((target.id.as_str(), place == Place::After)) };
        let order = reorder(siblings, &moved.id, anchor);
        let before: Vec<(String, i64, Option<String>)> = guild.channels.iter().map(|c| (c.id.clone(), c.position, c.parent_id.clone())).collect();
        let body: Vec<serde_json::Value> = order
            .iter()
            .map(|(id, position)| match id == &moved.id {
                true => json!({ "id": id, "position": position, "parent_id": parent, "lock_permissions": false }),
                false => json!({ "id": id, "position": position }),
            })
            .collect();
        let guild_id = drag.guild_id.clone();
        let (g1, g2, moved_id) = (guild_id.clone(), guild_id.clone(), moved.id.clone());
        self.rest_optimistic(
            Method::PATCH,
            format!("/guilds/{guild_id}/channels"),
            Some(json!(body)),
            move |this| {
                let Some(guild) = this.guilds.iter_mut().find(|g| g.id == g1) else { return };
                for channel in guild.channels.iter_mut() {
                    if let Some((_, position)) = order.iter().find(|(id, _)| *id == channel.id) {
                        channel.position = *position;
                    }
                    if channel.id == moved_id {
                        channel.parent_id = parent.clone();
                    }
                }
                guild.channels.sort_by_key(|c| c.position);
            },
            move |this| {
                let Some(guild) = this.guilds.iter_mut().find(|g| g.id == g2) else { return };
                for channel in guild.channels.iter_mut() {
                    if let Some((_, position, parent)) = before.iter().find(|(id, _, _)| *id == channel.id) {
                        channel.position = *position;
                        channel.parent_id = parent.clone();
                    }
                }
                guild.channels.sort_by_key(|c| c.position);
            },
            cx,
        );
    }
}

// The new order of a list of (id, position) after moving `moved` next to `anchor` (id, after?),
// or to the front when there's no anchor; positions are renumbered from 0.
pub fn reorder(mut siblings: Vec<(String, i64)>, moved: &str, anchor: Option<(&str, bool)>) -> Vec<(String, i64)> {
    siblings.sort_by_key(|(_, position)| *position);
    let mut ids: Vec<String> = siblings.into_iter().map(|(id, _)| id).filter(|id| id != moved).collect();
    let at = match anchor {
        Some((target, after)) => ids.iter().position(|id| id == target).map_or(ids.len(), |i| if after { i + 1 } else { i }),
        None => 0,
    };
    ids.insert(at.min(ids.len()), moved.to_owned());
    ids.into_iter().enumerate().map(|(i, id)| (id, i as i64)).collect()
}

// Where dropping someone in `current` on `target` moves them: nowhere when it's their own channel,
// or when they've already left voice (a PATCH can't move them, and `null` would mean disconnect).
fn voice_move_target<'a>(current: Option<&str>, target: &'a str) -> Option<&'a str> {
    current.filter(|current| *current != target).map(|_| target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(order: &[(String, i64)]) -> Vec<&str> {
        order.iter().map(|(id, _)| id.as_str()).collect()
    }

    #[test]
    fn reorders_and_renumbers() {
        let list = || vec![("a".to_owned(), 5), ("b".to_owned(), 1), ("c".to_owned(), 3)];
        assert_eq!(ids(&reorder(list(), "b", Some(("a", true)))), ["c", "a", "b"]);
        assert_eq!(ids(&reorder(list(), "a", Some(("b", false)))), ["a", "b", "c"]);
        assert_eq!(ids(&reorder(list(), "x", None)), ["x", "b", "c", "a"]);
        assert_eq!(reorder(list(), "c", Some(("b", false)))[0], ("c".to_owned(), 0));
    }

    // Carried right fast it tilts clockwise toward 14° (its underdamped spring may pass it a little,
    // as the design's does) and swells; let go, it glides onto its row in about 250ms without a jump.
    // Held, the ghost keeps up: 100px behind the pointer, it's there within 150ms, never more than a few px past.
    #[test]
    fn ghost_keeps_up_with_the_pointer() {
        let mut carry = Carry::new(point(px(0.), px(0.)), 1.04, 0.);
        carry.follow(point(px(0.), px(100.)));
        let mut peak: f32 = 0.;
        for _ in 0..9 {
            carry.advance(1. / 60.);
            peak = peak.max(carry.y.position);
        }
        assert!((carry.y.position - 100.).abs() < 3., "at 150ms it's {}", carry.y.position);
        assert!(peak < 106., "overshot to {peak}");
    }

    #[test]
    fn carries_then_lands() {
        let frame = 1. / 120.;
        let mut carry = Carry::new(point(px(0.), px(0.)), 1.05, 0.);
        for i in 1..=24 {
            carry.follow(point(px(i as f32 * 2000. * frame), px(0.)));
            carry.advance(frame);
            assert!(carry.tilt.position.abs() <= MAX_TILT * 1.2, "tilt {}", carry.tilt.position);
        }
        assert!(carry.tilt.position > 4., "tilt {}", carry.tilt.position);
        assert!(carry.lift.position > 1.02);
        carry.follow(point(px(40.), px(120.)));
        carry.land();
        let (mut last, mut frames) = (carry.at(), 0);
        while !carry.advance(frame) {
            let at = carry.at();
            assert!((at - last).magnitude() < 40., "jumped {:?}", at - last);
            last = at;
            frames += 1;
            assert!(frames < 60, "still landing after 500ms");
        }
        assert!(frames as f32 * frame > 0.15, "landed in {}ms", frames as f32 * frame * 1000.);
    }

    #[test]
    fn voice_drop_moves_only_to_another_channel() {
        assert_eq!(voice_move_target(Some("a"), "b"), Some("b"));
        assert_eq!(voice_move_target(Some("a"), "a"), None);
        assert_eq!(voice_move_target(None, "b"), None);
    }
}
