use crate::ui;
use super::*;

// Discord's tile overlay thresholds: compact chips at this width and under, no foot at all at the second.
const COMPACT: f32 = 220.;
const NO_FOOT: f32 = 124.;
// A narrower tile's Watch button says only "Watch", and goes small under the second width.
const WATCH_SHORT: f32 = 175.;
const WATCH_SMALL: f32 = 195.;
// The avatar on a tile without a picture. It needs a tile 1.5x its height; a shorter one (a 124-207px strip tile,
// where Discord's 80 would overflow) takes the 40px avatar.
const TILE_AVATAR: f32 = 80.;
const TILE_AVATAR_FROM: f32 = 1.5 * TILE_AVATAR * 16. / 9.;
// The speaking ring: 3px of green with a 1px dark line inside it.
const RING: f32 = 3.;
// An unwatched stream's preview shows faintly behind its Watch button.
const UNWATCHED_PREVIEW: f32 = 0.15;
// Discord's overlay black, at rest and hovered.
const OVERLAY: f32 = 0.56;
const OVERLAY_HOVER: f32 = 0.64;

impl TileSize {
    fn width(self) -> f32 {
        match self {
            TileSize::Card(w) | TileSize::Focus { w, .. } => w,
            TileSize::Fill => f32::INFINITY,
        }
    }

    // A focused tile that fills the stage on an axis loses its corners, as Discord's.
    fn radius(self) -> f32 {
        match self {
            TileSize::Card(_) | TileSize::Focus { flush: false, .. } => RADIUS_LG,
            TileSize::Focus { flush: true, .. } | TileSize::Fill => 0.,
        }
    }

    fn shape(self, d: Stateful<Div>) -> Stateful<Div> {
        let radius = self.radius();
        match self {
            TileSize::Card(w) => d.w(px(w)).h(px(w * 9. / 16.)).flex_none().squircle(radius).overflow_hidden(),
            TileSize::Focus { w, h, .. } => d.w(px(w)).h(px(h)).flex_none().squircle(radius).overflow_hidden(),
            TileSize::Fill => d.size_full(),
        }
    }
}

impl Waffle {
    // A watched stream that ended, in its card's place: that it ended and why, and Close stream to clear
    // it. While it's up, the streamer going live again brings us back in as a viewer, as Discord's does.
    pub(super) fn ended_tile(&self, streamer: &str, size: TileSize, cx: &mut Context<Self>) -> AnyElement { // lint-allow(custom-component): ui::Empty::new lacks a compact stage tile with a close action
        let name = self.user_name(streamer);
        let line = match (self.participants().iter().any(|s| s.user_id == streamer), self.activity_name(streamer)) {
            (false, _) => format!("{name} left the call"),
            (true, Some(app)) => format!("{name} stopped sharing {app}"),
            (true, None) => format!("{name} stopped streaming"),
        };
        let close = streamer.to_owned();
        self.stage_notice(format!("ended-{streamer}"), size, ("Stream ended", line, "Close stream"), move |this, cx| this.close_ended_stream(&close, cx), cx)
    }

    // Discord's failed-stream screen: what went wrong, its error code, and Close, which stops watching.
    pub(super) fn failed_tile(&self, streamer: &str, code: u16, size: TileSize, cx: &mut Context<Self>) -> AnyElement { // lint-allow(custom-component): ui::Empty::new lacks a compact stage tile with a close action
        let close = streamer.to_owned();
        let line = format!("{} Error code: {code}", failure_line(code));
        self.stage_notice(format!("failed-{streamer}"), size, ("The stream couldn't start", line, "Close stream"), move |this, cx| {
            this.unwatch_stream(&close, cx);
            if this.focused_stream().as_deref() == Some(close.as_str()) {
                this.call_focus = None;
            }
        }, cx)
    }

    // `text`: the heading, the line under it, and the button's label.
    pub(super) fn stage_notice(&self, key: String, size: TileSize, text: (&'static str, String, &'static str), close: impl Fn(&mut Self, &mut Context<Self>) + 'static, cx: &mut Context<Self>) -> AnyElement {
        let (heading, line, button) = text;
        let card = size.shape(ui::col().id(SharedString::from(key.clone()))).relative().bg(stage::ended()).items_center().justify_center().gap_1().p_3().text_center();
        if size.width() <= COMPACT {
            return card.child(Icon::new(Glyph::ScreenOff).size_6().text_color(stage::muted())).into_any_element();
        }
        card.child(div().text_size(ui::TEXT_MD).font_weight(FontWeight::MEDIUM).text_color(stage::strong()).child(heading))
            .child(div().text_size(ui::TEXT_XS).text_color(stage::muted()).child(line))
            .child(div().mt_1p5().child(stage_button(SharedString::from(format!("{key}-close")), button, cx).on_click(cx.listener(move |this, _, _, cx| { // lint-allow(raw-margin): optical gap under a tile's message before its action; the column is built by the caller
                cx.stop_propagation();
                close(this, cx);
            }))))
            .with_animation(SharedString::from(format!("{key}-in")), Animation::new(ui::DUR_PANEL), |d, t| d.opacity(t.min(1.)))
            .into_any_element()
    }

    pub(super) fn close_ended_stream(&mut self, streamer: &str, cx: &mut Context<Self>) {
        if let Some(call) = &mut self.call {
            call.ended.retain(|id| id != streamer);
        }
        if self.focused_stream().as_deref() == Some(streamer) {
            self.call_focus = None;
        }
        cx.notify(); // lint-allow(global-notify): dropping the focus swaps the stage layout (main) and the activity frame
    }

    // Over a stream not yet watched: Watch Stream switches to it alone and focuses it; shift-click adds it to the
    // streams already playing, as Discord's.
    fn watch_cta(&self, tile: &Tile, w: f32, cx: &mut Context<Self>) -> ui::Button {
        let card = tile.clone();
        let label = if w < WATCH_SHORT { "Watch" } else { "Watch Stream" };
        let button = ui::Button::new(SharedString::from(format!("watch-{}", tile_key(tile)))).scope(ui::ButtonScope::Stage).child(label);
        if w < WATCH_SMALL { button.size(ui::ButtonSize::Sm) } else { button }.on_click(cx.listener(move |this, e: &gpui_kit::ClickEvent, _, cx| {
            cx.stop_propagation();
            let Tile::Stream(state) = &card else { return };
            if e.modifiers().shift {
                return this.watch_stream(state.user_id.clone(), cx);
            }
            this.stop_watching(cx);
            this.call_focus = None;
            this.toggle_card(&card, cx);
        }))
    }

    // .tile: a person (camera, or their avatar on their colour) or someone's Go Live. Its foot carries the name
    // chip and, on hover, Options (and Mute on a watched stream); streams say LIVE. Speaking draws the green ring
    // above everything. Click focuses, double-click goes full screen on it.
    pub(super) fn tile(&self, tile: &Tile, size: TileSize, shown: f32, cx: &mut Context<Self>) -> AnyElement {
        let call = self.call.as_ref().expect("in a call");
        let (w, radius, big) = (size.width(), size.radius(), !matches!(size, TileSize::Card(_)));
        let state = match tile {
            Tile::User(state) | Tile::Stream(state) => state,
            Tile::Ended(streamer) => return self.ended_tile(streamer, size, cx),
            Tile::Activity(app_id) => return self.activity_tile(app_id, (!big).then_some(w), cx),
        };
        let user = state.user.clone().unwrap_or_else(|| User {
            id: state.user_id.clone(),
            name: self.user_name(&state.user_id),
            avatar: discord::avatar_url(&state.user_id, None),
            ..Default::default()
        });
        let me = self.me.as_ref().is_some_and(|m| m.id == state.user_id);
        let (muted, deafened) = if me { (self.muted || self.deafened, self.deafened) } else { (state.muted, state.deafened) };
        let local_muted = !me && self.prefs.local_mutes.contains(&state.user_id);
        let is_stream = matches!(tile, Tile::Stream(_));
        let watch_state = call.watched(&state.user_id).map(|w| w.state);
        if is_stream
            && let Some(crate::WatchState::Failed(code)) = watch_state
        {
            return self.failed_tile(&state.user_id, code, size, cx);
        }
        let waiting = call.issues.contains(&(format!("stream:{}", state.user_id), voice::issues::RECEIVER_READY_TIMEOUT));
        if is_stream && waiting {
            let streamer = state.user_id.clone();
            let line = format!("Connected, but no picture has come through. Error code: {}", voice::issues::RECEIVER_READY_TIMEOUT);
            return self.stage_notice(format!("no-video-{}", state.user_id), size, ("No video yet", line, "Try again"), move |this, cx| this.retry_watch(&streamer, cx), cx);
        }
        if is_stream && me && self.self_stream_hidden() {
            return self.stage_notice(format!("hidden-{}", state.user_id), size, ("Stream preview hidden", "You're still live; only your own preview is off.".into(), "Show preview"), |this, cx| this.toggle_self_stream_hidden(cx), cx);
        }
        let talking = !is_stream && self.speaking(&state.user_id);
        let sounding = !is_stream && self.soundboard_playing(&state.user_id);
        // Disable Video hides their camera here; GAP: the voice engine still receives it.
        let camera = (!is_stream && !self.prefs.hidden_video.contains(&state.user_id)).then(|| call.cameras.get(&state.user_id)).flatten();
        // A stream card plays the stream once it's watched (our own from the local capture), and shows
        // Discord's preview still until then. A watched stream that stopped delivering goes back to the loader
        // instead of freezing on its last frame; a paused one keeps it.
        let stalled = !me && call.watched(&state.user_id).is_some_and(|w| w.arrivals.back().is_some_and(|at| at.elapsed() > STALL_AFTER)) && !self.stream_paused(&state.user_id);
        let live_frame = match me {
            true => call.self_view.as_ref().map(|(frame, _)| frame.clone()),
            false => call.watched(&state.user_id).and_then(|w| w.frame.clone()).filter(|_| !stalled),
        };
        let watching = call.watched(&state.user_id).is_some();
        let watchable = is_stream && !watching && !me;
        let loading = is_stream && live_frame.is_none() && (me || watching);
        // Focused, the picture keeps its whole frame (bars on the axis that doesn't fit); a card is cropped to fill.
        let fit = if big { gpui_kit::ObjectFit::Contain } else { gpui_kit::ObjectFit::Cover };
        let face: AnyElement = match (is_stream, camera) {
            // Playing, the picture letterboxes on black; the gradient only holds the place before it.
            (true, _) => match big || live_frame.is_some() || loading {
                true => div().relative().size_full().bg(gpui_kit::black()),
                false => stream_fill(radius).size_full(),
            }
            .map(|f| match live_frame {
                Some(frame) => f.child(ui::media(ImageSource::Render(frame), ui::all(radius)).absolute().inset_0().size_full().object_fit(gpui_kit::ObjectFit::Contain)),
                // Discord's preview is a stale, soft still: once we're waiting on the real picture, say so.
                None if loading => f
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(mascot::loader(SharedString::from(format!("stream-loading-{}", state.user_id)), if big { 64. } else { 40. }, palette::logo(), cx))
                    // Discord's reconnecting overlay, with its error code.
                    .when(watch_state == Some(crate::WatchState::Reconnecting) && w > COMPACT, |f| {
                        f.child(div().text_size(ui::TEXT_XS).text_color(stage::muted()).child(format!("Reconnecting… Error code: {}", crate::STREAM_RECONNECTING)))
                    }),
                None => f.children(self.stream_preview(&state.user_id).map(|url| ui::media(url, ui::all(radius)).absolute().inset_0().size_full().object_fit(fit).when(watchable, |m| m.opacity(UNWATCHED_PREVIEW)))),
            })
            .into_any_element(),
            // Our own camera reads as a mirror, as every call app shows it; others see it unflipped.
            (false, Some(frame)) => ui::media(ImageSource::Render(frame.clone()), ui::all(radius)).size_full().object_fit(fit).when(me, mirrored).into_any_element(),
            (false, None) => ui::row()
                .size_full()
                .squircle(radius)
                .bg(self.tile_fill(&state.user_id))
                .justify_center()
                .child(ui::Avatar::new(user.avatar.clone(), if w < TILE_AVATAR_FROM { ui::AVATAR_XL } else { TILE_AVATAR }).when(local_muted, |a| a.opacity(0.5)))
                .into_any_element(),
        };
        let compact = w <= COMPACT;
        let label = match (is_stream, self.activity_name(&state.user_id)) {
            (true, Some(app)) => format!("{} · {app}", user.name),
            _ => user.name.clone(),
        };
        let mark = match () {
            _ if is_stream => None,
            _ if state.server_muted => Some(Glyph::MicOff),
            _ if deafened => Some(Glyph::HeadphonesOff),
            _ if muted => Some(Glyph::MicOff),
            _ => None,
        };
        let group = SharedString::from(format!("tile-{}-{is_stream}-{}", state.user_id, w.min(9999.) as u32));
        let chip = name_label(compact)
            .when(is_stream, |d| d.child(Icon::new(Glyph::Screen).size(ui::ICON_MD)))
            .when(!is_stream && self.stage_moderator(&state.user_id), |d| d.child(crate::stage_view::moderator_mark(&state.user_id)))
            .children(mark.map(|glyph| Icon::new(glyph).size(ui::ICON_MD).text_color(stage::muted_ink())))
            .when(local_muted, |d| d.child(Icon::new(Glyph::VolumeOff).size(ui::ICON_MD).text_color(stage::muted_ink())))
            .child(ui::line(label));
        let buttons = ui::row().flex_none().gap_2().opacity(0.).group_hover(group.clone(), |s| s.opacity(1.)).children((is_stream && watching && !me).then(|| self.stream_mute_button(&state.user_id, cx))).child(self.tile_options(&state.user_id, is_stream, cx));
        let foot = (w > NO_FOOT).then(|| {
            let foot = ui::row().absolute().items_end().justify_between().gap_1().opacity(shown).child(chip).child(buttons);
            if compact { foot.left_1().right_1().bottom_1() } else { foot.left_3().right_3().bottom_3() }
        });
        let issues = self.tile_issues(&state.user_id, is_stream, me);
        let issue_chip = (!issues.is_empty() && !compact).then(|| issue_chip(SharedString::from(format!("issues-{}-{is_stream}", state.user_id)), issues).absolute().top_2().left_2());
        let viewers = if is_stream && !compact { self.viewers(&state.user_id) } else { Vec::new() };
        let live = is_stream.then(|| ui::row().absolute().top_2().right_2().gap_1().when(!viewers.is_empty(), |d| d.child(viewer_chip(viewers, &state.user_id))).child(ui::Badge::live()));
        let (card, double, menu_user) = (tile.clone(), tile.clone(), state.user_id.clone());
        size.shape(div().id(group.clone()))
            .group(group.clone())
            .relative()
            .child(face)
            .children(issue_chip)
            .children(live)
            .when(watchable, |d| d.child(ui::row().absolute().inset_0().justify_center().child(self.watch_cta(tile, w, cx))))
            .children(foot)
            .children((!is_stream).then(|| self.soundboard_tile_effect(&state.user_id, effect_area(size), cx)).flatten())
            .cursor_pointer()
            .on_click(cx.listener(move |this, e: &gpui_kit::ClickEvent, window, cx| match e.click_count() {
                1 => this.toggle_card(&card, cx),
                _ => this.fullscreen_tile(&double, window, cx),
            }))
            .when(talking || sounding, |d| d.child(tile_ring(sounding, local_muted, radius)))
            .on_mouse_down(MouseButton::Right, cx.listener(move |this, e: &MouseDownEvent, _, cx| match is_stream {
                true => this.open_stream_tile_menu(menu_user.clone(), e.position, cx),
                false => this.voice_user_menu(&menu_user, e.position, cx),
            }))
            .into_any_element()
    }

    // Options, bottom right on hover: the person's menu, or the stream's.
    fn tile_options(&self, user_id: &str, is_stream: bool, cx: &mut Context<Self>) -> ui::Button {
        let id = user_id.to_owned();
        overlay_button(SharedString::from(format!("tile-options-{user_id}-{is_stream}")), Glyph::More, "Options", cx).on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                match is_stream {
                    true => this.open_stream_tile_menu(id.clone(), e.position, cx),
                    false => this.voice_user_menu(&id, e.position, cx),
                }
            }),
        )
    }

    // Mute on a stream we watch: its sound off, and back to where it was.
    fn stream_mute_button(&self, streamer_id: &str, cx: &mut Context<Self>) -> ui::Button {
        let volume = self.stream_volumes.get(streamer_id).copied().unwrap_or(1.);
        let (glyph, tip) = if volume == 0. { (Glyph::VolumeOff, "Unmute") } else { (Glyph::VolumeHigh, "Mute") };
        let id = streamer_id.to_owned();
        overlay_button(SharedString::from(format!("tile-mute-{streamer_id}")), glyph, tip, cx).on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.set_stream_volume(&id, if volume == 0. { 1. } else { 0. }, cx);
        }))
    }

    // A double-click focuses the tile and takes the call full screen.
    fn fullscreen_tile(&mut self, tile: &Tile, window: &mut Window, cx: &mut Context<Self>) {
        if self.call_focus.as_deref() != Some(tile_key(tile).as_str()) {
            self.toggle_card(tile, cx);
        }
        self.set_call_fullscreen(true, window, cx);
    }

    // A person's tile ground: their avatar's colour, as Discord takes its dominant one; until it's fetched a dark
    // tint from their id, so the grid isn't one grey.
    pub(super) fn tile_fill(&self, user_id: &str) -> Hsla {
        if let Some(color) = self.avatar_colors.get(user_id).copied().flatten() {
            return color;
        }
        let hue = user_id.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32)) as f32 % 360.;
        palette::oklch(0.24, 0.02, hue)
    }
}

// .tile's name chip: Discord's overlay black, its state icon first; 12px in a compact tile.
fn name_label(compact: bool) -> Div {
    let chip = ui::row().min_w_0().gap_1().squircle(RADIUS_XS).bg(gpui_kit::black().opacity(OVERLAY)).text_color(gpui_kit::white());
    match compact {
        true => chip.px_1p5().py_0p5().text_size(ui::TEXT_XS),
        false => chip.px_2().py_1p5().text_size(ui::TEXT_SM),
    }
}

// The 32px buttons over a tile's foot, on the overlay black.
fn overlay_button(id: SharedString, glyph: Glyph, tip: &'static str, _cx: &App) -> ui::Button {
    let black = gpui_kit::black();
    ui::Button::new(id).variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).scope(ui::ButtonScope::Themed { rest: black.opacity(OVERLAY), hover: black.opacity(OVERLAY_HOVER), ink: gpui_kit::white(), ink_hover: gpui_kit::white() }).icon(glyph).tooltip_text(tip)
}

// The tile a soundboard effect plays across.
// GAP: a focused tile filling an activity's stage has no known size here; 960 wide stands in.
pub(super) fn effect_area(size: TileSize) -> (f32, f32) {
    match size {
        TileSize::Card(w) => (w, w * 9. / 16.),
        TileSize::Focus { w, h, .. } => (w, h),
        TileSize::Fill => (960., 540.),
    }
}

// Speaking, inset as Discord's: 3px of green with a 1px dark line inside it (neutral when they're muted for you).
// A soundboard sound gets honey instead, the colour the picker marks a playing sound with, so it never reads as
// someone talking; it outranks the green while it plays.
pub(super) fn tile_ring(sounding: bool, local_muted: bool, radius: f32) -> Div {
    // The stage is always dark, so the dark theme's green.
    let color = match (sounding, local_muted) {
        (true, _) => palette::accent(),
        (false, true) => stage::wash(0.42),
        (false, false) => palette::status(crate::discord::Status::Online),
    };
    let ring = div().absolute().inset_0().squircle(radius); // lint-allow(custom-mark): ui::speaking_ring lacks a double ring sized by the tile's computed radius
    ring.border(px(RING)).border_color(color).child(div().absolute().inset_0().squircle((radius - RING).max(0.)).border_1().border_color(gpui_kit::black())) // lint-allow(custom-mark): ui::speaking_ring lacks a double ring sized by the tile's computed radius
}
