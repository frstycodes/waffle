// Everything around a voice call (design/voice.html): the voice panel over the user panel with its
// connection stages, the connection details popover, the call stage with its tiles, header and
// control bar, device menus and the participant menu.
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, Context, Div, ElementId, FontFeatures, FontWeight, Hsla, ImageSource, MouseButton, MouseDownEvent, Pixels, Point, SharedString, Stateful, Window,
    component::{ActiveTheme, Icon},
    div, prelude::*, px, relative,
};

mod entry;
mod fit;
mod grid;
mod previews;
mod layouts;
mod dock;
mod menus;
mod tiles;
mod header;
use dock::*;
pub(crate) use dock::VoicePopover;
pub(crate) use layouts::StageUi;

use crate::{
    call_issues::{failure_line, issue_chip},
    Waffle,
    discord::{self, User, VoiceState},
    icons::Glyph,
    layout::{HEADER_HEIGHT, RAIL_WIDTH},
    part::Column,
    mascot,
    menu::{Entry, item, submenu},
    palette::{self, stage},
    settings::Page,
    soundboard,
    ui::{self, RADIUS_LG, RADIUS_MD, RADIUS_SM, RADIUS_XS, Squircle},
    voice::{self, Stage},
};

// .voicebar: 10px padding, a 38px status block (a 20px line over an 18px one), 8px gap, 32px
// buttons, and its 1px hairline.
pub const VOICE_HEIGHT: f32 = 99.;
pub(crate) const PANEL_PAD: f32 = 10.;
// The error bar is the voicebar without its buttons.
const ERROR_HEIGHT: f32 = 59.;
const PREVIEW_AGE: std::time::Duration = std::time::Duration::from_secs(60);
// After a failed preview fetch, how long until the next try.
const PREVIEW_RETRY: std::time::Duration = std::time::Duration::from_secs(20);
// The stage (.call): the 48px header line (FOCUS_HEAD) over a 32px header row, then 16px padding and gaps
// around the tiles and 48px controls.
// Long enough that a hiccup in a streamer's capture doesn't read as stalled.
// The stage redraws once a second during a call, so the loader shows within a second of this.
const STALL_AFTER: Duration = Duration::from_secs(3);
pub(crate) const STAGE_PAD: f32 = 16.;
pub(crate) const CONTROLS_HEIGHT: f32 = 48.;
// A group's inset around its buttons.
pub(crate) const DOCK_PAD: f32 = 4.;
// .filmstrip under a focused activity.
const STRIP_TILE: f32 = 148.;
const STRIP_GAP: f32 = 8.;
// The filmstrip's fold button.
const STRIP_TOGGLE: f32 = 28.;
const STRIP_LIMIT: usize = 12;
// Names on strip cards and the focused stream show while the pointer is anywhere on the stage.
// A stage button's height (the Watch and Go Live buttons share it).
const STAGE_BUTTON_HEIGHT: f32 = 30.;

const STAGE_GROUP: &str = "call-stage";
// The stage's header band, the app's one header line.
pub(crate) const FOCUS_HEAD: f32 = 48.;
// Discord's idle timeout: the header, the bar and the name chips fade after the pointer rests this long.
const CONTROLS_IDLE: Duration = Duration::from_secs(2);
// The voice channel's chat beside the call: at least 450 wide, leaving the stage 550 (Discord's numbers).
const CHAT_SLIDE: Duration = Duration::from_millis(260);
pub(crate) const CHAT_MIN_WIDTH: f32 = 450.;
const CHAT_STAGE_ROOM: f32 = 550.;
// The connection popover's colour bands (its legend): under 80 ms, 80 to 120, over 120.
const PING_FAIR: u32 = 80;
const PING_BAD: u32 = 120;
// When the panel says "Poor connection": a bad average ping, or this much packet loss.
const LOSS_POOR_PCT: f32 = 5.;
const METER_BARS: usize = 28;

// The chat's width for a window this wide: what was asked for, within Discord's bounds and no wider than
// `fit::CHAT_MAX_WIDTH`. A window too narrow for both keeps the minimum.
pub(crate) fn chat_width(wanted: f32, window: f32) -> f32 {
    wanted.min(fit::CHAT_MAX_WIDTH).min(window - CHAT_STAGE_ROOM).max(CHAT_MIN_WIDTH)
}

#[derive(Clone)]
enum Tile {
    User(VoiceState),
    Stream(VoiceState),
    // A stream we watched that ended, kept in its place until closed (by its streamer's id).
    Ended(String),
    // An activity running in the call (by its app's id), or the one we're starting.
    Activity(String),
}

// How a tile is drawn: a 16:9 card of this width, the focused tile at its fitted size (flush when it fills the
// stage on an axis, so it loses its corners), or a focused activity filling the stage edge to edge.
#[derive(Clone, Copy)]
enum TileSize {
    Card(f32),
    Focus { w: f32, h: f32, flush: bool },
    Fill,
}

// The stage's fixed order: streams you watch or focus (and ended ones), other streams, cameras, then
// avatars. Within each group people keep the order they joined in (`people` is in join order), so
// nothing moves when someone speaks or joins. An ended stream whose streamer is live again has its
// stream back instead.
fn order_tiles(people: Vec<VoiceState>, ended: &[String], opened: impl Fn(&str) -> bool, camera: impl Fn(&VoiceState) -> bool) -> Vec<Tile> {
    let streams = people.iter().filter(|s| s.streaming).cloned().map(Tile::Stream);
    let ended = ended.iter().filter(|id| !people.iter().any(|s| s.streaming && &s.user_id == *id)).cloned().map(Tile::Ended);
    let mut tiles: Vec<Tile> = streams.chain(ended).chain(people.iter().cloned().map(Tile::User)).collect();
    tiles.sort_by_key(|tile| match tile {
        Tile::Stream(s) if opened(&s.user_id) => 0,
        Tile::Ended(_) | Tile::Activity(_) => 0,
        Tile::Stream(_) => 1,
        Tile::User(s) if camera(s) => 2,
        Tile::User(_) => 3,
    });
    tiles
}

// A card's identity across renders, for focus.
fn tile_key(tile: &Tile) -> String {
    match tile {
        Tile::User(s) => format!("user:{}", s.user_id),
        // Shares its stream's key, so a focused stream that ends stays the big card.
        Tile::Stream(VoiceState { user_id, .. }) | Tile::Ended(user_id) => format!("stream:{user_id}"),
        Tile::Activity(app_id) => crate::activities::activity_key(app_id),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceKind {
    Input,
    Output,
    Camera,
}

pub struct DeviceMenu {
    kind: DeviceKind,
    at: Point<Pixels>,
    // Opened from the call controls, so it takes the stage's dark tokens.
    on_stage: bool,
    inputs: Vec<voice::Device>,
    outputs: Vec<voice::Device>,
    cameras: Vec<voice::Device>,
}

impl DeviceMenu {
    /// Whether this is `kind`'s popover opened from the user area, not from the call stage's controls.
    pub fn opened_from_user_area(&self, kind: DeviceKind) -> bool {
        !self.on_stage && self.kind == kind
    }
}

// Where the connection stands, as the voice panel words and colours it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Link {
    Connecting,
    Connected,
    Poor,
    Lost,
}

impl Link {
    fn of(stage: Stage, stats: Option<&voice::Stats>) -> Self {
        match stage {
            Stage::Connected if stats.is_some_and(|s| s.median_ping_ms > PING_BAD || loss(s) >= LOSS_POOR_PCT) => Link::Poor,
            Stage::Connected => Link::Connected,
            Stage::NoRoute | Stage::Disconnected => Link::Lost,
            _ => Link::Connecting,
        }
    }
}

impl Waffle {

    // Opening a voice channel you're in shows its call, as clicking it in Discord does.
    pub(crate) fn show_call(&mut self, channel: discord::Channel, window: &mut Window, cx: &mut Context<Self>) {
        if self.call.as_ref().is_none_or(|c| c.channel.id != channel.id) {
            self.join_voice(channel.clone(), cx);
        }
        let guild_id = self.guilds.iter().find(|g| g.channels.iter().any(|c| c.id == channel.id)).map(|g| g.id.clone());
        if let Some(guild_id) = guild_id
            && self.guild_id.as_ref() != Some(&guild_id)
        {
            self.open_guild(Some(guild_id), window, cx);
        }
        self.call_view = true;
        cx.notify(); // lint-allow(global-notify): call_view swaps the main column and the nav selection
    }

    pub(crate) fn participants(&self) -> Vec<VoiceState> {
        let Some(call) = &self.call else { return Vec::new() };
        let all = match &call.guild_id {
            Some(guild_id) => self.guilds.iter().find(|g| &g.id == guild_id).map(|g| g.voice_states.as_slice()).unwrap_or_default(),
            None => self.dm_voice.as_slice(),
        };
        let states: Vec<VoiceState> = all
            .iter()
            .filter(|v| v.channel_id.as_ref() == Some(&call.channel.id))
            .cloned()
            .collect();
        if states.is_empty()
            && let Some(me) = &self.me
        {
            return vec![VoiceState { guild_id: None, channel_id: Some(call.channel.id.clone()), user_id: me.id.clone(), user: Some(me.clone()), muted: self.muted, deafened: self.deafened, server_muted: false, server_deafened: false, streaming: false, camera: false, suppress: false, requested_at: None }];
        }
        states
    }

    pub(crate) fn speaking(&self, user_id: &str) -> bool {
        let Some(call) = &self.call else { return false };
        match self.me.as_ref().is_some_and(|m| m.id == user_id) {
            true => call.level.1 && !self.muted && !self.deafened,
            false => call.speaking.contains(user_id),
        }
    }

    fn ordered_tiles(&self) -> Vec<Tile> {
        let Some(call) = &self.call else { return Vec::new() };
        let focused = self.focused_stream();
        let me = self.me.as_ref().map(|m| m.id.as_str());
        let activities = self.activity_tiles().into_iter().map(Tile::Activity);
        let people = order_tiles(
            self.participants(),
            &call.ended,
            |id| call.watched(id).is_some() || focused.as_deref() == Some(id),
            |s| s.camera || call.cameras.contains_key(&s.user_id) || (me == Some(s.user_id.as_str()) && call.camera_on),
        );
        activities.chain(people).collect()
    }

    // .call: the stage, dark in both themes. The grid or the focused tile fill it; the header and the control bar
    // float over them (`stage_chrome`).
    pub(crate) fn render_call(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.call.as_ref()?;
        if self.call_is_stage() {
            return Some(self.render_stage(window, cx));
        }
        let tiles = self.ordered_tiles();
        let shown = self.chrome_shown(window, cx);
        let focused = self.call_focus.as_ref().and_then(|key| tiles.iter().find(|t| tile_key(t) == *key)).cloned();
        let body = match focused {
            Some(big @ Tile::Activity(_)) => self.activity_stage(&big, &tiles, shown, cx),
            Some(big) => self.focus_view(big, tiles, shown, cx),
            None => self.card_stage(tiles, shown, cx),
        };
        Some(self.stage_root(cx).child(body).child(self.stage_chrome(shown, cx)).into_any_element())
    }

    // The stage's size in the window, beside the rail, the channel list and the call's chat.
    pub(crate) fn stage_size(&self, window: &mut Window, cx: &mut App) -> (f32, f32) {
        let viewport = window.viewport_size();
        let chat = if self.call_chat_floats(window) { 0. } else { self.call_chat_width(window) * self.call_chat_reveal(window, cx) };
        match self.call_fullscreen {
            true => (f32::from(viewport.width) - chat, f32::from(viewport.height)),
            false => (f32::from(viewport.width) - RAIL_WIDTH - self.channels_width - chat, f32::from(viewport.height) - 1.),
        }
    }

    // The stage's own surface, tracking the pointer for the chrome's autohide.
    pub(crate) fn stage_root(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("call-view")
            .group(STAGE_GROUP)
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(stage::bg())
            .text_color(stage::text())
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.call_pointer_inside = *hovered;
                if !*hovered {
                    this.call_pointer_at = Instant::now().checked_sub(CONTROLS_IDLE * 2).unwrap_or_else(Instant::now);
                }
                this.redraw(Column::Main, cx);
            }))
            .on_mouse_move(cx.listener(|this, _: &gpui_kit::MouseMoveEvent, _, cx| this.stage_pointer_moved(cx)))
    }

    fn stage_pointer_moved(&mut self, cx: &mut Context<Self>) {
        let was_idle = self.call_idle() || !self.call_pointer_inside;
        self.call_pointer_at = Instant::now();
        self.call_pointer_inside = true;
        if was_idle {
            self.redraw(Column::Main, cx);
        }
    }

    // Clicking a card makes it the big one; clicking it again goes back to the grid. A stream you
    // aren't watching starts playing as it comes forward, replacing the one you watch, as Discord's does;
    // once you watch two or more, it joins them instead. Ended streams not in focus go as focus moves.
    fn toggle_card(&mut self, tile: &Tile, cx: &mut Context<Self>) {
        let key = tile_key(tile);
        let me = self.me.as_ref().map(|m| m.id.clone());
        if let Some(call) = &mut self.call {
            call.ended.retain(|id| format!("stream:{id}") == key);
        }
        // Opening our own stream shows its preview again, as Discord's.
        if let Tile::Stream(state) = tile
            && me.as_deref() == Some(state.user_id.as_str())
            && self.self_stream_hidden()
        {
            self.toggle_self_stream_hidden(cx);
        }
        if self.call_focus.as_ref() == Some(&key) {
            self.call_focus = None;
            return cx.notify(); // lint-allow(global-notify): the stage focus swaps the stage layout (main) and the activity frame overlay
        }
        if let Tile::Stream(state) = tile
            && let Some(call) = &self.call
            && call.watched(&state.user_id).is_none()
            && me.as_deref() != Some(state.user_id.as_str())
        {
            let others = call.watching.iter().filter(|w| me.as_deref() != Some(w.streamer_id.as_str())).count();
            if others == 1 {
                let only = call.watching.iter().find(|w| me.as_deref() != Some(w.streamer_id.as_str())).map(|w| w.streamer_id.clone());
                if let Some(only) = only {
                    self.unwatch_stream(&only, cx);
                }
            }
            self.watch_stream(state.user_id.clone(), cx);
        }
        self.call_focus = Some(key);
        cx.notify(); // lint-allow(global-notify): the stage focus swaps the stage layout (main) and the activity frame overlay
    }

    // A stage's speakers as the call's cards, in Discord's columns for the width (1 under 424px, 2 under
    // 624, 3 under 824, else 4).
    pub(crate) fn speaker_grid(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let tiles: Vec<Tile> = self
            .ordered_tiles()
            .into_iter()
            .filter(|t| match t {
                Tile::User(s) | Tile::Stream(s) => s.on_stage() == discord::OnStage::Speaker,
                Tile::Ended(_) => true,
                Tile::Activity(_) => false,
            })
            .collect();
        let cols = match width {
            w if w < 424. => 1,
            w if w < 624. => 2,
            w if w < 824. => 3,
            _ => 4,
        }
        .min(tiles.len().max(1));
        let tile_w = ((width - grid::GRID_GAP * (cols as f32 - 1.)) / cols as f32).floor();
        let cards: Vec<AnyElement> = tiles.iter().map(|t| self.tile(t, TileSize::Card(tile_w), 1., cx)).collect();
        ui::row().w(px(width)).flex_wrap().justify_center().gap(px(grid::GRID_GAP)).children(cards).into_any_element()
    }

    // Focus This Person from a menu: the tile with `key` becomes the big one, or goes back to the grid.
    pub(crate) fn toggle_focus(&mut self, key: String, cx: &mut Context<Self>) {
        let tile = self.ordered_tiles().into_iter().find(|t| tile_key(t) == key);
        if let Some(tile) = tile {
            self.toggle_card(&tile, cx);
        }
    }

    // Full screen shows the call alone: no servers, channels or user panel.
    pub(crate) fn set_call_fullscreen(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.call_fullscreen = on;
        if window.is_fullscreen() != on {
            window.toggle_fullscreen();
        }
        cx.notify(); // lint-allow(global-notify): full screen hides the nav and the panel and shows the stage alone
    }

    // Still pointer for a while, and nothing open that belongs to the bar.
    fn call_idle(&self) -> bool {
        !self.call_pointer_on_chrome && self.call_pointer_at.elapsed() > CONTROLS_IDLE && self.device_menu.is_none() && self.menu.is_none()
    }

    // The focused card's stream, if a stream is the one shown big.
    fn focused_stream(&self) -> Option<String> {
        self.call_focus.as_deref()?.strip_prefix("stream:").map(str::to_owned)
    }

    fn stream_paused(&self, streamer_id: &str) -> bool {
        let suffix = format!(":{streamer_id}");
        self.stream_paused.iter().any(|key| key.ends_with(&suffix))
    }

    // Everyone watching a streamer, other than the streamer.
    pub(crate) fn viewers(&self, streamer_id: &str) -> Vec<Viewer> {
        let suffix = format!(":{streamer_id}");
        let ids = self.stream_viewers.iter().find(|(key, _)| key.ends_with(&suffix)).map(|(_, v)| v.as_slice()).unwrap_or_default();
        ids.iter().filter(|id| *id != streamer_id).map(|id| Viewer { name: self.user_name(id).into(), avatar: self.user_avatar(id) }).collect()
    }

    // What someone is playing or streaming, from their presence, for name tags.
    fn activity_name(&self, user_id: &str) -> Option<String> {
        let activity = self.presences.get(user_id)?.activity.as_ref()?;
        matches!(activity.kind, discord::ActivityKind::Playing | discord::ActivityKind::Streaming | discord::ActivityKind::Competing).then(|| activity.text.clone())
    }

    fn open_stream_volume(&mut self, streamer_id: String, at: Point<Pixels>, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.open_live_menu(
            at,
            move |this| {
                let volume = this.stream_volumes.get(&streamer_id).copied().unwrap_or(1.);
                let id = streamer_id.clone();
                vec![crate::menu::slider("Stream volume", format!("stream-volume-{streamer_id}"), volume / 2., move |this, f, cx| this.set_stream_volume(&id, f * 2., cx))]
            },
            cx,
        );
    }

    fn set_stream_volume(&mut self, streamer_id: &str, volume: f32, cx: &mut Context<Self>) {
        self.stream_volumes.insert(streamer_id.to_owned(), volume);
        if let Some(call) = &self.call {
            call.session.set_stream_volume(streamer_id, volume);
        }
        cx.notify(); // lint-allow(global-notify): the stream volume shows on the stage tile (main) and in its overlay menu
    }

    pub(crate) fn render_call_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::new();
        out.extend(self.connection_overlay(cx));
        out.extend(self.noise_overlay(window, cx));
        out.extend(self.device_overlay(window, cx));
        out.extend(self.render_launcher(cx));
        out.extend(self.render_activity_consent(window, cx));
        out.extend(self.render_activity_ask(window, cx));
        out
    }

}

// Chrome that occludes takes the pointer from the stage under it, whose hover and moves drive the
// autohide; it reports them itself so the chrome stays up while the pointer is on it.
fn keeps_chrome<E: StatefulInteractiveElement>(surface: E, cx: &mut Context<Waffle>) -> E {
    surface
        .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
            this.call_pointer_on_chrome = *hovered;
            match *hovered {
                true => this.stage_pointer_moved(cx),
                false => this.redraw(Column::Main, cx),
            }
        }))
        .on_mouse_move(cx.listener(|this, _: &gpui_kit::MouseMoveEvent, _, cx| this.stage_pointer_moved(cx)))
}

fn call_backdrop(id: &'static str, cx: &mut Context<Waffle>) -> Stateful<Div> {
    div().id(id).absolute().inset_0().occlude().on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _, _, cx| {
            this.dismiss_call_menus();
            cx.notify(); // lint-allow(global-notify): the call menus are overlays, which only follow the app
        }),
    )
}

// The few tokens a menu reads, from the stage or from the theme.
struct Inks {
    overlay: Hsla,
    strong: Hsla,
    text: Hsla,
    muted: Hsla,
    faint: Hsla,
    hover: Hsla,
    line: Hsla,
    line_strong: Hsla,
    // A meter's lit bars: on, hot, clipping.
    lit: [Hsla; 3],
}

impl Inks {
    fn of(on_stage: bool, cx: &App) -> Self {
        match on_stage {
            true => Inks { overlay: stage::overlay(), strong: stage::strong(), text: stage::text(), muted: stage::muted(), faint: stage::faint(), hover: stage::wash(0.05), line: stage::wash(0.08), line_strong: stage::wash(0.14), lit: [discord::Status::Online, discord::Status::Idle, discord::Status::Dnd].map(palette::status) },
            false => Inks {
                overlay: palette::surface(palette::Level::Overlay, cx),
                strong: palette::strong(cx),
                text: palette::text(cx),
                muted: palette::muted(cx),
                faint: palette::faint(cx),
                hover: palette::hover(cx),
                line: palette::line(cx),
                line_strong: palette::line_strong(cx),
                lit: [discord::Status::Online, discord::Status::Idle, discord::Status::Dnd].map(|s| palette::status_in(s, cx)),
            },
        }
    }
}

// .iconbtn on the stage: muted until hovered; `pressed` holds the selected fill.
pub(crate) fn stage_icon_button(id: impl Into<ElementId>, glyph: Glyph, tip: &'static str, pressed: bool, cx: &App) -> ui::Control {
    let clear = gpui_kit::transparent_black();
    let (rest, hover, ink) = if pressed { (stage::wash(0.08), stage::wash(0.09), stage::strong()) } else { (clear, stage::wash(0.05), stage::muted()) };
    ui::icon_button(id, glyph, tip, cx).scoped(rest, hover, ink, stage::strong())
}

// .btn.btn--sm on the stage: the quiet fill.
pub(crate) fn stage_button(id: impl Into<ElementId>, label: &'static str, cx: &App) -> ui::Control {
    ui::button(id, ui::Tone::Stage, cx).h(px(STAGE_BUTTON_HEIGHT)).child(label)
}

// .preview .watch: the light button over a dimmed stream.
// lint-allow(custom-component): ui::button with Tone::Overlay needs an id and a cx, and its callers in sidebar.rs and call/tiles.rs add their own
pub(crate) fn watch_button(label: &'static str) -> Div {
    watch_button_look().px_2p5().gap_2().child(Icon::new(Glyph::Play).size(ui::ICON_MD)).child(label)
}

fn watch_button_look() -> Div {
    ui::row()
        .h(px(STAGE_BUTTON_HEIGHT))
        .squircle(RADIUS_SM)
        .bg(stage::strong())
        .hover(|s| s.bg(gpui_kit::white()))
        .text_color(stage::on_ink())
        .text_size(ui::TEXT_SM)
        .font_weight(FontWeight::MEDIUM)
}

// A call control's state: .cbtn at rest, .off (a muted mic: red ink on a red wash, its caret too), .on (light),
// .active (accent), the open caret ([aria-expanded]). The red .leave is its own button.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Control {
    Rest,
    Off,
    On,
    Active,
    Open,
}

// .cbtn: 40x40 inside its group, a squircle; icons 20px (the chevron 12px on a 20px strip). Pressed, it sinks to 92%.
// The group is opaque, so a button at rest has no fill of its own.
pub(crate) fn control(id: &'static str, glyph: Glyph, tip: &'static str, state: Control, enabled: bool) -> ui::Pressable {
    let (fill, hover, ink) = match state {
        Control::Rest => (gpui_kit::transparent_black(), stage::wash(0.08), gpui_kit::white()),
        Control::Off => (stage::muted_fill(0.12), stage::muted_fill(0.16), stage::muted_ink()),
        Control::On => (stage::on(false), stage::on(true), stage::on_ink()),
        Control::Active => (palette::accent(), palette::accent_hover(), palette::on_accent()),
        Control::Open => (stage::wash(0.12), stage::wash(0.12), gpui_kit::white()),
    };
    let chevron = matches!(glyph, Glyph::ChevronUpBold);
    let side = CONTROLS_HEIGHT - DOCK_PAD * 2.;
    ui::pressable(div().id(id), if enabled { 0.92 } else { 1. })
        .w(px(if chevron { 20. } else { side }))
        .h(px(side))
        .flex_none()
        .squircle(10.)
        .flex()
        .items_center()
        .justify_center()
        .bg(fill)
        .text_color(ink)
        .child(Icon::new(glyph).size(px(if chevron { 12. } else { 20. })))
        .when(enabled, |b| b.cursor_pointer().hover(move |s| s.bg(hover).text_color(ink)))
        .when(!enabled, |b| b.opacity(0.4).cursor(gpui_kit::CursorStyle::OperationNotAllowed))
        .tooltip(move |window, cx| ui::tip(tip, window, cx))
}

fn mirrored<E: Styled>(mut el: E) -> E {
    el.style().scale.width = Some(-1.);
    el
}

// .csplit: the main button with its caret a hairline beside it; in a narrow window the caret goes (right-click
// still opens the menu).
fn split_control(main: impl IntoElement, caret: Option<impl IntoElement>) -> Div {
    ui::row().items_stretch().gap(ui::HAIRLINE).child(main).children(caret)
}

// .cgroup: related controls on one opaque plate with a hairline, so they read over any picture.
pub(crate) fn control_group() -> Div {
    ui::row()
        .h(px(CONTROLS_HEIGHT))
        .gap_2()
        .p(px(DOCK_PAD))
        .squircle(10. + DOCK_PAD)
        .bg(stage::dock())
        .border_1()
        .border_color(stage::wash(0.08))
}

// .tile .viewers: who's watching, top right; a few faces and a "+n", the full list on hover.
const VIEWER_FACES: usize = 4;

#[derive(Clone)]
pub(crate) struct Viewer {
    name: SharedString,
    avatar: String,
}

pub(crate) fn viewer_chip(viewers: Vec<Viewer>, streamer_id: &str) -> Stateful<Div> {
    let people: Vec<ui::Entry> = viewers.iter().map(|v| ui::Entry::person(v.avatar.clone(), v.name.clone())).collect();
    // lint-allow(custom-mark): ui::badged marks a face's corner, and this chip pins over a stream tile's corner
    let chip = ui::row()
        .id(SharedString::from(format!("viewers-{streamer_id}")))
        .h_5()
        .pl_2()
        .pr_1()
        .rounded_full()
        .gap_1p5()
        .bg(stage::dock())
        .text_color(stage::muted())
        .child(Icon::new(Glyph::EyeBold).size(ui::ICON_SM))
        .child(ui::capped_stack(&people, VIEWER_FACES, ui::AVATAR_SM - 4., stage::dock(), stage::overlay(), stage::strong()));
    ui::overflow_list(chip, "Watching", people)
}

// .tile.stream: a stream before its first preview arrives.
pub(crate) fn stream_fill(radius: f32) -> Div {
    let glow = gpui_kit::linear_gradient(135., gpui_kit::linear_color_stop(palette::stream_glow().0.opacity(0.7), 0.), gpui_kit::linear_color_stop(palette::stream_glow().1.opacity(0.7), 1.));
    // lint-allow(custom-mark): ui::media places an image against a container's edge, and this is a gradient laid over a tile's ground
    div().relative().squircle(radius).bg(palette::stream_ground()).child(div().absolute().inset_0().squircle(radius).bg(glow))
}

#[derive(Clone)]
struct MenuSliderDrag(SharedString);

impl gpui_kit::Render for MenuSliderDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

pub(crate) fn tabular() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".into(), 1)]))
}

#[cfg(test)]
mod tests {
    use super::{previews::*, *};

    #[test]
    fn avatar_colour_and_url() {
        assert_eq!(average_color(&[255, 0, 0, 255, 0, 0, 255, 255, 9, 9, 9, 0]), Some((127, 0, 127)));
        assert_eq!(average_color(&[1, 2, 3, 0]), None);
        assert_eq!(small_png("https://cdn.discordapp.com/avatars/1/a_b.gif?size=128"), "https://cdn.discordapp.com/avatars/1/a_b.png?size=32");
        assert_eq!(small_png("https://cdn.discordapp.com/embed/avatars/3.png"), "https://cdn.discordapp.com/embed/avatars/3.png?size=32");
        let frame = voice::VideoFrame { width: 2, height: 1, bgra: vec![0, 0, 255, 255, 255, 0, 0, 255] };
        assert!(jpeg_data_url(&frame).is_some_and(|url| url.starts_with("data:image/jpeg;base64,/9j/")));
    }
}

// "1080p 60" or "Source 30", how Waffle names a stream's quality.
fn quality_label(resolution: u32, fps: u32) -> String {
    match resolution {
        0 => format!("Source {fps}"),
        r => format!("{r}p {fps}"),
    }
}

// A running time: "4:05", or "1:02:09" past an hour.
pub(crate) fn clock(secs: u64) -> String {
    match secs / 3600 {
        0 => format!("{}:{:02}", secs / 60, secs % 60),
        h => format!("{h}:{:02}:{:02}", secs / 60 % 60, secs % 60),
    }
}

#[cfg(test)]
mod pip_tests {
    use super::*;

    #[test]
    fn clocks_and_quality() {
        assert_eq!(clock(245), "4:05");
        assert_eq!(clock(3729), "1:02:09");
        assert_eq!(quality_label(1080, 60), "1080p 60");
        assert_eq!(quality_label(0, 30), "Source 30");
    }
}

#[cfg(test)]
mod order_tests {
    use super::*;

    fn person(id: &str, streaming: bool, camera: bool) -> VoiceState {
        VoiceState { guild_id: None, channel_id: Some("c".into()), user_id: id.into(), user: None, muted: false, deafened: false, server_muted: false, server_deafened: false, streaming, camera, suppress: false, requested_at: None }
    }

    #[test]
    fn streams_first_then_cameras_then_avatars_in_join_order() {
        // Joined in this order: a (avatar), b (streams), c (camera), d (streams, watched), e (avatar).
        let people = vec![person("a", false, false), person("b", true, false), person("c", false, true), person("d", true, false), person("e", false, false)];
        let keys: Vec<String> = order_tiles(people.clone(), &[], |id| id == "d", |s| s.camera).iter().map(tile_key).collect();
        assert_eq!(keys, ["stream:d", "stream:b", "user:c", "user:a", "user:b", "user:d", "user:e"]);
        // Someone speaking or a mute changes nothing; a new joiner lands at the end of their group.
        let mut joined = people;
        joined.push(person("f", true, false));
        let keys: Vec<String> = order_tiles(joined.clone(), &[], |id| id == "d", |s| s.camera).iter().map(tile_key).collect();
        assert_eq!(keys, ["stream:d", "stream:b", "stream:f", "user:c", "user:a", "user:b", "user:d", "user:e", "user:f"]);
        // An ended stream keeps a card up front until its streamer is live again; they may have left.
        let keys: Vec<String> = order_tiles(joined.clone(), &["gone".into()], |_| false, |s| s.camera).iter().map(tile_key).collect();
        assert_eq!(&keys[..2], ["stream:gone", "stream:b"]);
        let keys: Vec<String> = order_tiles(joined, &["b".into()], |_| false, |s| s.camera).iter().map(tile_key).collect();
        assert_eq!(keys.iter().filter(|k| *k == "stream:b").count(), 1, "live again: the stream, not the ended card");
    }
}

