// The call stage's layouts, as Discord's (design/voice-stage.html): the grid, the focused tile over its strip,
// and the header and control bar floating over both on 160px gradients, fading away while the pointer rests.
use super::{
    grid::{GRID_GAP, STRIP_FROM_HEIGHT, STRIP_MAX, StripRank, grid_fit, strip_fit},
    *,
};
use crate::ui;

// The black that fades in under the header and the bar, from the stage's edge.
const CHROME_FADE: f32 = 160.;
// How far the header and the bar slide toward their edge as they fade.
const CHROME_DRIFT: f32 = 8.;
// The grid's foot below its last row, clear of the bar.
const GRID_FOOT: f32 = 56.;
// The focused tile keeps this much clear above and below it for the header, the strip and the bar.
const FOCUS_KEEP: f32 = 72.;
const STRIP_ROOM: f32 = 68.;
// The strip's foot sits this far above the stage's foot, and lifts the focused tile by this much.
const STRIP_BOTTOM: f32 = 76.;
const STRIP_LIFT: f32 = 60.;
// The members pill's offset under the focused tile, and its bottom without the strip.
const MEMBERS_BELOW: f32 = 2.;
const MEMBERS_BOTTOM: f32 = 40.;
// Your camera floating over the stage while the strip is hidden.
const SELF_PREVIEW: f32 = 160.;
const SELF_PREVIEW_BOTTOM: f32 = 50.;
// People who spoke this recently still rank above the quiet in the strip.
const SPOKE_RECENTLY: Duration = Duration::from_secs(10);

// What only the stage draws about the call.
#[derive(Default)]
pub struct StageUi {
    // Discord's "Hide Members": the strip under a focused tile folded away.
    pub(crate) strip_hidden: bool,
    // A voice channel we left while looking at it: its join screen shows until we look elsewhere.
    pub(crate) join: Option<discord::Channel>,
    // The "Start An Activity" tile dismissed for this session.
    pub(crate) entry_hidden: bool,
}

impl Waffle {
    // 1 while the header, the bar and the name chips show; 0 once the pointer has rested 2s or left the stage.
    pub(super) fn chrome_shown(&self, window: &mut Window, cx: &mut App) -> f32 {
        // The webview over a focused activity hides the pointer from the stage, so its chrome stays.
        let activity_focused = self.call_focus.as_deref().is_some_and(|k| k.starts_with("activity:"));
        let menus_open = self.device_menu.is_some() || self.menu.is_some();
        let away = !self.call_pointer_inside && !self.call_pointer_on_chrome;
        let rest = !activity_focused && !menus_open && (away || self.call_idle());
        gpui_kit::base::transition("call-chrome", if rest { 0. } else { 1. }, gpui_kit::base::Transition::new(ui::DUR_POP), window, cx)
    }

    // The header and the bar over the stage, each on its fade, sliding 8px toward its edge as it goes.
    pub(super) fn stage_chrome(&self, shown: f32, reaches_edge: bool, cx: &mut Context<Self>) -> AnyElement {
        let drift = CHROME_DRIFT * (1. - shown);
        let fade = |angle: f32| gpui_kit::linear_gradient(angle, gpui_kit::linear_color_stop(gpui_kit::black().opacity(0.8), 0.), gpui_kit::linear_color_stop(gpui_kit::transparent_black(), 1.));
        let head = keeps_chrome(div().id("call-head"), cx).occlude().absolute().top(px(-drift)).left_0().right_0().opacity(shown).child(self.stage_head(self.call_header(cx), reaches_edge));
        let bar = div().absolute().left_0().right_0().bottom(px(STAGE_PAD - drift)).px(px(STAGE_PAD)).opacity(shown).child(self.call_controls(cx));
        div()
            .absolute()
            .inset_0()
            .child(div().absolute().top_0().left_0().right_0().h(px(CHROME_FADE)).opacity(shown).bg(fade(180.)))
            .child(div().absolute().bottom_0().left_0().right_0().h(px(CHROME_FADE)).opacity(shown).bg(fade(0.)))
            .child(self.above_game(head))
            .child(self.above_game(bar))
            .into_any_element()
    }

    // The grid, fitted to the space it has this frame: sizing it from the last frame's measurement drew one stale
    // frame on every resize and Space change.
    pub(super) fn card_stage(&self, tiles: Vec<Tile>, shown: f32, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.weak_entity();
        ui::sized(move |size, _, cx| {
            this.update(cx, |waffle, cx| waffle.tile_grid(&tiles, f32::from(size.width), f32::from(size.height), shown, cx)).unwrap_or_else(|_| div().into_any_element())
        })
        .size_full()
        .into_any_element()
    }

    // Discord's grid: rows of the biggest 16:9 tiles, 8px apart and centred, then the entry tile; past 25 it scrolls.
    fn tile_grid(&self, tiles: &[Tile], width: f32, height: f32, shown: f32, cx: &mut Context<Self>) -> AnyElement {
        let entry = self.entry_kind();
        let g = grid_fit(tiles.len() + usize::from(entry.is_some()), width, height);
        let mut cards: Vec<AnyElement> = tiles.iter().map(|t| self.tile(t, TileSize::Card(g.w), shown, cx)).collect();
        cards.extend(entry.map(|alone| self.entry_tile(alone, g.w, cx)));
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut cards = cards.into_iter().peekable();
        while cards.peek().is_some() {
            rows.push(ui::row().flex_none().justify_center().gap(px(GRID_GAP)).children(cards.by_ref().take(g.cols)).into_any_element());
        }
        // lint-allow(unvirtualized-list): at most 25 rows of tiles; Discord's grid draws every tile too
        div().id("call-grid").size_full().when(g.scrolls, |d| d.overflow_y_scroll()).child(ui::col().pt(px(g.top)).pb(px(GRID_FOOT)).px(px(GRID_GAP)).gap(px(GRID_GAP)).children(rows)).into_any_element()
    }

    // The focused tile, sized to the space it has this frame, over the strip of everyone (itself marked).
    pub(super) fn focus_view(&self, big: Tile, tiles: Vec<Tile>, shown: f32, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.weak_entity();
        ui::sized(move |size, _, cx| {
            this.update(cx, |waffle, cx| waffle.focus_layout(&big, tiles, (f32::from(size.width), f32::from(size.height)), shown, cx)).unwrap_or_else(|_| div().into_any_element())
        })
        .size_full()
        .into_any_element()
    }

    fn focus_layout(&self, big: &Tile, tiles: Vec<Tile>, (width, height): (f32, f32), shown: f32, cx: &mut Context<Self>) -> AnyElement {
        let tall = height > STRIP_FROM_HEIGHT;
        let strip_on = tall && !self.stage_ui.strip_hidden && tiles.len() > 1;
        let keep = if strip_on { FOCUS_KEEP + STRIP_ROOM } else { 0. };
        let room = height - 2. * keep;
        let w = width.min(room * 16. / 9.);
        let h = (w * 9. / 16.).floor();
        let top = (height - h) / 2. - if strip_on { STRIP_LIFT } else { 0. };
        let flush = room * 16. / 9. >= width || room >= height;
        let key = tile_key(big);
        let focused = div().absolute().top(px(top)).left(px((width - w) / 2.)).child(self.tile(big, TileSize::Focus { w, h, flush }, shown, cx));
        let own = self.own_camera_tile(&tiles);
        let strip = strip_on.then(|| {
            let strip = strip_fit(width, tiles.clone(), |t| self.strip_rank(t), |t| own.as_ref().is_some_and(|o| tile_key(o) == tile_key(t)));
            let mut cards: Vec<AnyElement> = strip.tiles.iter().map(|t| self.strip_item(t, strip.w, tile_key(t) == key, shown, cx)).collect();
            cards.extend(strip.own.as_ref().map(|t| self.strip_item(t, STRIP_MAX, tile_key(t) == key, shown, cx)));
            ui::row().absolute().left_0().right_0().bottom(px(STRIP_BOTTOM)).px(px(GRID_GAP)).justify_center().items_end().gap(px(GRID_GAP)).children(cards)
        });
        let preview = (!strip_on).then_some(own).flatten().filter(|t| tile_key(t) != key).map(|t| {
            div().absolute().right(px(STAGE_PAD)).bottom(px(SELF_PREVIEW_BOTTOM)).shadow(ui::shadow_pop_on(stage::wash(0.08), cx)).child(self.tile(&t, TileSize::Card(SELF_PREVIEW), shown, cx))
        });
        let members = (tall && tiles.len() > 1).then(|| {
            let at = ui::row().absolute().left_0().right_0().justify_center().opacity(shown);
            let at = match strip_on {
                true => at.top(px(top + h + MEMBERS_BELOW)),
                false => at.bottom(px(MEMBERS_BOTTOM + (FOCUS_KEEP - (height - h) / 2.).max(0.))),
            };
            self.above_game(at.child(self.strip_toggle(strip_on, cx)))
        });
        div().relative().size_full().child(focused).children(strip).children(preview).children(members).into_any_element()
    }

    // A strip tile; the focused one is dimmed under the glyph of what it is.
    fn strip_item(&self, tile: &Tile, w: f32, selected: bool, shown: f32, cx: &mut Context<Self>) -> AnyElement {
        let glyph = match tile {
            Tile::Stream(_) | Tile::Ended(_) => Glyph::Screen,
            Tile::User(s) if self.has_camera(s) => Glyph::VideoFill,
            Tile::User(_) => Glyph::Phone,
            Tile::Activity(_) => Glyph::ActivitiesFill,
        };
        let card = self.tile(tile, TileSize::Card(w), shown, cx);
        let mark = selected.then(|| ui::row().absolute().inset_0().squircle(RADIUS_LG).justify_center().bg(gpui_kit::black().opacity(0.6)).child(Icon::new(glyph).size(ui::ICON_XL).text_color(gpui_kit::white()))); // lint-allow(custom-mark): ui::media_label lacks a full-tile scrim over the focused tile's strip copy
        div().id(SharedString::from(format!("strip-{}", tile_key(tile)))).relative().flex_none().child(card).children(mark).into_any_element()
    }

    fn strip_rank(&self, tile: &Tile) -> StripRank {
        let call = self.call.as_ref().expect("in a call");
        match tile {
            Tile::Activity(_) => StripRank::Activity,
            Tile::Stream(_) | Tile::Ended(_) => StripRank::Stream,
            Tile::User(s) if self.speaking(&s.user_id) => StripRank::Speaking,
            Tile::User(s) if call.last_spoke.get(&s.user_id).is_some_and(|at| at.elapsed() < SPOKE_RECENTLY) => StripRank::SpokeRecently,
            Tile::User(s) if self.has_camera(s) => StripRank::Camera,
            Tile::User(_) => StripRank::Rest,
        }
    }

    pub(super) fn has_camera(&self, state: &VoiceState) -> bool {
        let call = self.call.as_ref().expect("in a call");
        let me = self.me.as_ref().is_some_and(|m| m.id == state.user_id);
        state.camera || call.cameras.contains_key(&state.user_id) || (me && call.camera_on)
    }

    // Your own tile, while your camera is on.
    fn own_camera_tile(&self, tiles: &[Tile]) -> Option<Tile> {
        let me = self.me.as_ref()?;
        tiles.iter().find(|t| matches!(t, Tile::User(s) if s.user_id == me.id && self.has_camera(s))).cloned()
    }

    // A focused activity: its game fills the stage edge to edge, kept clear of the pill and the bar that draw over
    // its webview (`above_game`); everyone else waits behind the pill or sits in a strip under it.
    pub(super) fn activity_stage(&self, big: &Tile, tiles: &[Tile], shown: f32, cx: &mut Context<Self>) -> AnyElement {
        let key = tile_key(big);
        let others: Vec<&Tile> = tiles.iter().filter(|t| tile_key(t) != key).take(STRIP_LIMIT).collect();
        let strip = !self.stage_ui.strip_hidden && !others.is_empty();
        let band = CONTROLS_HEIGHT + STAGE_PAD * 2.;
        let toggle_bottom = if strip { STRIP_GAP } else { band + STRIP_GAP };
        let foot = crate::activities::TIP_ROOM + if others.is_empty() { band } else { toggle_bottom + STRIP_TOGGLE + STRIP_GAP };
        let media = div()
            .relative()
            .flex_1()
            .min_h_0()
            .bg(gpui_kit::black())
            .pt(px(FOCUS_HEAD))
            .pb(px(foot))
            .child(self.tile(big, TileSize::Fill, shown, cx))
            .when(!others.is_empty(), |d| d.child(self.above_game(ui::row().items_stretch().absolute().left_0().right_0().bottom(px(toggle_bottom)).justify_center().opacity(shown).child(self.strip_toggle(strip, cx)))));
        ui::col()
            .size_full()
            .child(media)
            .when(strip, |d| {
                let cards: Vec<AnyElement> = others.into_iter().map(|t| self.tile(t, TileSize::Card(STRIP_TILE), shown, cx)).collect();
                // Centred while it fits; wider than the stage, it starts at the left edge and scrolls.
                d.child(self.above_game(ui::row().items_stretch().id("filmstrip").flex_none().overflow_x_scroll().pt(px(STRIP_GAP)).px(px(STAGE_PAD)).child(ui::row().items_stretch().mx_auto().flex_none().gap(px(STRIP_GAP)).children(cards)))) // lint-allow(raw-margin): auto margins centre the strip while it fits and start it at the left edge when wider, which justify_center can't
                    .child(div().h(px(band)).flex_none())
            })
            .into_any_element()
    }
}
