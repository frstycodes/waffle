use crate::ui;
use super::*;

// The connection popover's width, and the height of each of its stat rows and of the ping graph.
const INFO_WIDTH: f32 = 300.;
const INFO_ROW_HEIGHT: f32 = 26.;
const GRAPH_HEIGHT: f32 = 56.;
// The noise suppression popout's width (Discord's 244).
const NOISE_WIDTH: f32 = 244.;
// The action row and the gap above it; a stage's audience has none.
const ACTIONS_HEIGHT: f32 = 40.;

impl Waffle {
    // In-call warnings: a mic that sends nothing, and Advanced suppression that had to fall back to
    // RNNoise because this computer can't keep up with it.
    pub(super) fn voice_warning(&self) -> Option<&'static str> {
        let call = self.call.as_ref()?;
        let silent = self.prefs.silence_warning && !self.muted && !self.deafened && call.session.input_detected() == Some(false);
        match (silent, call.session.noise_suppression_degraded()) {
            (true, _) => Some("No audio detected from your mic"),
            (false, true) => Some("Advanced noise suppression fell back to Standard"),
            _ => None,
        }
    }

    pub(crate) fn voice_panel_height(&self) -> f32 {
        let actions = if self.in_stage_audience() { 0. } else { ACTIONS_HEIGHT };
        match (&self.call, &self.voice_error) {
            (Some(_), _) => VOICE_HEIGHT - ACTIONS_HEIGHT + actions + self.activity_panel_height(),
            (None, Some(_)) => ERROR_HEIGHT,
            (None, None) => 0.,
        }
    }

    pub(crate) fn dismiss_call_menus(&mut self) -> bool {
        self.voice_popover.take().is_some() | self.device_menu.take().is_some() | self.activities.launcher.take().is_some()
    }

    // .voicebar: the connection's state and where you are (or why it needs you), the one button
    // that leaves, then camera, share and activities.
    // Discord's RTC panel (design/user-row.html): the ping mark and the connection's state word, which rolls to
    // "Voice Details" under the pointer, over the channel / server link; disconnect beside them, then camera,
    // share, soundboard and activities.
    pub(crate) fn voice_panel(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let Some(call) = &self.call else { return self.voice_error_panel(cx) };
        let connected = call.stage == Stage::Connected;
        let video = call.camera_on || call.sharing.is_some();
        let (word, state) = stage_word(call.stage, video);
        let tone = state.ink(cx);
        let ping = call.stats.as_ref().map(|s| s.ping_ms);
        let place = match call.guild_id.as_ref().and_then(|id| self.guilds.iter().find(|g| &g.id == id)) {
            Some(guild) => format!("{} / {}", call.channel.name, guild.name),
            None => call.channel.name.clone(),
        };
        let channel = call.channel.clone();
        // GAP: Discord's line is only ever the link; Waffle also says here when the mic sends nothing.
        let second: AnyElement = match self.voice_warning() {
            Some(warning) => why_line(warning, palette::warning_ink(cx))
                .id("voice-warning")
                .cursor_pointer()
                .hover(|s| s.underline())
                .on_click(cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.open_settings(Page::VoiceVideo, window, cx);
                }))
                .into_any_element(),
            _ => ui::line(place)
                .id("voice-place")
                .h(ui::LINE_DETAIL)
                .text_size(ui::TEXT_XS)
                .text_color(palette::faint(cx))
                .hover(|s| s.underline())
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    // A DM call lives in its DM's strip: go back to the chat, not a stage.
                    match channel.kind {
                        discord::ChannelKind::Dm => {
                            this.guild_id = None;
                            this.call_view = false;
                            this.open_channel(channel.clone(), window, cx);
                        }
                        _ => this.show_call(channel.clone(), window, cx),
                    }
                }))
                .into_any_element(),
        };
        // The ping mark shows only once connected, in its quality's colour; it says the ping under the pointer.
        let mark = ping.filter(|_| connected).map(|ms| {
            let bars = match ms {
                ms if ms > PING_BAD => ui::bars_weak(ping_color(ms, cx), 12.),
                ms => ui::bars_still(ping_color(ms, cx), 12.),
            };
            div().id("voice-ping").flex_none().child(bars).tooltip(move |window, cx| ui::tip(format!("{ms} ms"), window, cx))
        });
        let (hovered, on_hover) = ui::hover_state("voice-status-hover", window, cx);
        let details = if video { "Video Details" } else { "Voice Details" };
        let word = ui::hover_roll("voice-status-roll", ui::line(word), ui::line(details), hovered, ui::LINE_SM, window, cx).flex_1();
        let status = div() // lint-allow(custom-button): ui::button and ui::menu_row fill on hover; Discord's status is a bare link with no fill
            .id("voice-status")
            .flex_1()
            .min_w_0()
            .cursor_pointer()
            .on_hover(on_hover)
            .child(ui::row().h_5().gap_1().text_size(ui::TEXT_SM).font_weight(FontWeight::MEDIUM).text_color(tone).children(mark).child(word))
            .child(second)
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_voice_popover(VoicePopover::Connection);
                cx.notify(); // lint-allow(global-notify): the info button highlight is in the nav dock and the connection popover is an overlay
            }));
        let leave_tip = if crate::discord::STAGE_TYPE == call.channel.raw_type { "Disconnect Quietly" } else { "Disconnect" };
        // Noise suppression and Disconnect, as Discord's; noise suppression opens its popover.
        let noise_open = self.voice_popover == Some(VoicePopover::Noise);
        let noise = ui::icon_button("voice-noise", Glyph::Soundwave, "Noise Suppression", cx)
            .toggled(noise_open, cx)
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_voice_popover(VoicePopover::Noise);
                cx.notify(); // lint-allow(global-notify): the button lights in the nav dock and its popover is an overlay
            }));
        let top = ui::row().gap_1().child(status).child(noise).child(ui::icon_button("hang-up", Glyph::CallEnd, leave_tip, cx).on_click(cx.listener(|this, _, _, cx| this.leave_call_asking(cx))));
        let wait = if state == ConnState::Error { "Available once you're reconnected" } else { "Available once you're connected" };
        let ready = |label: &'static str| if connected { label } else { wait };
        let sharing = call.sharing.is_some();
        let camera = panel_action("panel-camera", Glyph::Video, ready(if call.camera_on { "Turn Off Camera" } else { "Turn On Camera" }), connected, call.camera_on, cx)
            .when(connected, |b| b.on_click(cx.listener(|this, _, _, cx| this.toggle_camera(cx))));
        let share = match sharing {
            true => panel_action("panel-share", Glyph::ShareScreen, "Stream options", true, true, cx)
                .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_stream_menu(e.position, cx))),
            false => panel_action("panel-share", Glyph::ShareScreen, ready("Share Your Screen"), connected, false, cx).when(connected, |b| b.on_click(cx.listener(|this, _, _, cx| this.toggle_screen_share(cx)))),
        };
        let activities = panel_action("panel-activities", Glyph::Activities, ready("Activities"), connected, self.activity_panel_height() > 0., cx)
            .when(connected, |b| b.on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, window, cx| this.open_launcher_from_panel(e.position, window, cx))));
        let access = self.soundboard_access();
        let soundboard = (access != soundboard::Access::Hidden).then(|| {
            let on = connected && access == soundboard::Access::Ready;
            panel_action("panel-soundboard", Glyph::Soundboard, if connected { access.tip() } else { wait }, on, false, cx)
                .relative()
                .when(self.soundboard_open(), |b| b.bg(palette::press(cx)))
                .child(self.soundboard_probe(soundboard::PANEL_CONTROL))
                .when(on, |b| b.on_click(cx.listener(|this, _, window, cx| this.open_soundboard(Some(soundboard::PANEL_CONTROL), window, cx))))
        });
        let voice = ui::col()
            .h(px(self.voice_panel_height()))
            .flex_none()
            .p(px(PANEL_PAD))
            .gap_2()
            .rounded_t(px(RADIUS_MD)).corner_shape(ui::SQUIRCLE)
            .relative()
            .child(ui::inset_rule(PANEL_PAD, cx))
            .children(self.activity_panel_row(cx)).child(top)
            // A stage's audience has no camera, share or activities, as Discord's.
            .when(!self.in_stage_audience(), |v| v.child(ui::row().items_stretch().gap_2().child(camera).child(share).children(soundboard).child(activities)));
        Some(voice.into_any_element())
    }

    // The voicebar's danger state once a call is gone: what happened, and a way to dismiss it.
    pub(super) fn voice_error_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let error = self.voice_error.clone()?;
        let tone = palette::danger_ink(cx);
        Some(
            ui::row()
                .h(px(ERROR_HEIGHT))
                .p(px(PANEL_PAD))
                .gap_2()
                .rounded_t(px(RADIUS_MD)).corner_shape(ui::SQUIRCLE)
                .relative()
                .child(ui::inset_rule(PANEL_PAD, cx))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(ui::row().h_5().text_size(ui::TEXT_SM).font_weight(FontWeight::MEDIUM).text_color(tone).child("Voice disconnected"))
                        .child(why_line(error, tone)),
                )
                .child(ui::icon_button("dismiss-voice", Glyph::X, "Dismiss", cx).on_click(cx.listener(|this, _, _, cx| {
                    this.voice_error = None;
                    this.redraw(Column::Nav, cx);
                })))
                .into_any_element(),
        )
    }

    fn toggle_voice_popover(&mut self, popover: VoicePopover) {
        self.voice_popover = (self.voice_popover != Some(popover)).then_some(popover);
    }

    // Discord's noise suppression popout over its button: the switch, what it does, and your mic's live level
    // (Discord's Mic Test).
    pub(super) fn noise_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.voice_popover != Some(VoicePopover::Noise) {
            return None;
        }
        let call = self.call.as_ref()?;
        let on = self.prefs.noise_suppression;
        let (strong, muted) = (palette::strong(cx), palette::muted(cx));
        // Discord's Mic Test: Test loops your processed mic back to you (the settings page's test); the
        // meter follows the test while it runs, the call's mic otherwise.
        let testing = self.mic_test.is_some();
        let level = ((if testing { self.mic_level } else { call.level.0 }) + 100.) / 100.;
        let test = ui::button("noise-mic-test", ui::Tone::Secondary, cx)
            .small()
            .flex_none()
            .child(if testing { "Stop" } else { "Test" })
            .on_click(cx.listener(|this, _, _, cx| this.toggle_mic_test(cx)));
        let card = ui::surface(cx)
            .w(px(NOISE_WIDTH))
            .p_4()
            .gap_3()
            .text_size(ui::TEXT_SM)
            .child(
                ui::row()
                    .id("noise-row")
                    .justify_between()
                    .gap_2()
                    .cursor_pointer()
                    .child(div().text_color(strong).font_weight(FontWeight::MEDIUM).child("Noise Suppression"))
                    .child(ui::switch_live("noise-switch", on, window, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_noise_suppression(cx))),
            )
            .child(div().text_color(muted).child("Make some noise, like clapping your hands, while speaking. Your friends will hear nothing but your beautiful voice."))
            .child(
                ui::col()
                    .gap_1p5()
                    .child(div().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(muted).child("Mic Test"))
                    .child(ui::row().gap_3().child(test).child(ui::meter(level, METER_BARS, cx).flex_1().min_w_0().h_3p5().gap_0p5())),
            );
        let sidebar = RAIL_WIDTH + self.channels_width;
        let at = gpui_kit::point(px(sidebar - 8.), window.viewport_size().height - px(self.panel_stack_height() + 16.));
        Some(call_backdrop("noise-backdrop", cx).child(ui::floating_at("noise-card", at, gpui_kit::Anchor::BottomRight, ui::rise(card.transform_origin(ui::corner(true, true)), "noise-card", Duration::ZERO))).into_any_element())
    }

    // .connpop: how the connection is doing, ping over the last few minutes, the numbers behind it,
    // and how the call is secured.
    pub(super) fn connection_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.voice_popover != Some(VoicePopover::Connection) {
            return None;
        }
        let call = self.call.as_ref()?;
        let stats = call.stats.clone().unwrap_or_default();
        let measured = call.stats.is_some();
        let (strong, muted, faint) = (palette::strong(cx), palette::muted(cx), palette::faint(cx));
        let link = Link::of(call.stage, call.stats.as_ref());
        let verdict = match (link, measured) {
            (Link::Connected | Link::Poor, true) => quality_word(stats.median_ping_ms),
            (Link::Lost, _) => "Reconnecting",
            _ => "Connecting",
        };
        let verdict_ink = match verdict {
            "Good" => palette::positive_ink(cx),
            "Connecting" => muted,
            _ => palette::warning_ink(cx),
        };
        let peak = stats.last_pings.iter().copied().max().unwrap_or(0).max(160) as f32;
        let graph = ui::row()
            .h(px(GRAPH_HEIGHT))
            .p_1p5()
            .squircle(RADIUS_SM)
            .bg(palette::surface(palette::Level::Page, cx))
            .items_end()
            .gap_0p5()
            .children(stats.last_pings.iter().map(|&p| div().flex_1().min_w_0p5().h(relative((p as f32 / peak).clamp(0.06, 1.))).rounded_t_xs().rounded_b(ui::HAIRLINE).bg(ping_color(p, cx)))); // lint-allow(custom-divider): ui::progress_bar lacks a ping graph; these are data-sized bars, not dividers
        let swatch = |color: Hsla, text: &'static str| ui::row().gap_1().child(div().size_2().rounded_xs().bg(color)).child(text);
        let legend = ui::row().items_stretch()
            .flex_wrap()
            .gap_3()
            .text_size(ui::TEXT_XS)
            .text_color(muted)
            .child(swatch(ping_color(0, cx), "Under 80 ms"))
            .child(swatch(ping_color(PING_FAIR, cx), "80–120 ms"))
            .child(swatch(ping_color(PING_BAD + 1, cx), "Over 120 ms"));
        let dash = |value: String| if measured { value } else { "—".into() };
        let row = |label: &'static str, value: String| {
            ui::row()
                .min_h(px(INFO_ROW_HEIGHT))
                .justify_between()
                .gap_3()
                .child(div().flex_none().text_color(muted).child(label))
                .child(ui::line(value).text_color(strong).font_weight(FontWeight::MEDIUM).font_features(tabular()))
        };
        // Discord's badge at the popout's foot: every call is secured, a DAVE call end to end.
        let secured = if stats.dave { "End-to-end encrypted" } else { "Secure Connection" };
        let positive = palette::positive_ink(cx);
        // The server by its short name, as Discord's ("rotterdam8412").
        let host = stats.endpoint.split('.').next().unwrap_or_default().to_owned();
        let card = ui::col() // lint-allow(custom-badge, custom-mark, custom-surface): ui::surface lacks the stage-scoped quality card (stage colours, not palette)
            .id("connection-card")
            .occlude()
            .absolute()
            .left_2()
            .bottom(px(self.panel_stack_height() + 16.))
            .w(px(INFO_WIDTH))
            .p_4()
            .squircle(RADIUS_LG)
            .bg(palette::surface(palette::Level::Overlay, cx))
            .shadow(ui::shadow_pop(cx))
            .gap_3()
            .text_size(ui::TEXT_SM)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(ui::row().items_baseline().justify_between().gap_2().child(div().text_color(strong).font_weight(FontWeight::MEDIUM).child("Connection")).child(div().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(verdict_ink).child(verdict)))
            .when_some(stage_help(call.stage), |card, help| card.child(div().text_color(muted).child(help)))
            .when(call.stage == Stage::Connected, |card| card.child(
                ui::col()
                    .gap_1()
                    .child(graph)
                    // GAP: the engine keeps its last 20 heartbeat pings, about 13.75s apart, so the graph spans roughly five minutes.
                    .child(ui::row().items_stretch().justify_between().text_size(ui::TEXT_XS).text_color(faint).child("5 min ago").child("Now"))
                    .child(legend),
            )
            .child(
                ui::col()
                    .gap_0p5()
                    // GAP: the engine keeps a median, not Discord's average.
                    .child(row("Median ping", dash(format!("{} ms", stats.median_ping_ms))))
                    .child(row("Last ping", dash(format!("{} ms", stats.ping_ms))))
                    .child(row("Outbound packet loss", dash(stats.outbound_loss_pct.map_or("—".into(), |loss| format!("{loss:.1}%")))))
                    .child(row("Server", dash(host))),
            )
            .child(div().text_size(ui::TEXT_XS).text_color(muted).child("You may notice delayed audio at 250 ms or higher. You may sound robotic if your packet loss rate is over 10%. If the problem persists, disconnect and try again.")))
            .child(
                ui::row()
                    .pt_3()
                    .border_t_1()
                    .border_color(palette::line(cx))
                    .gap_1p5()
                    .text_size(ui::TEXT_XS)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(positive)
                    .child(Icon::new(Glyph::HashtagLock).size(ui::ICON_SM).flex_none())
                    .child(secured),
            )
;
        // It opens above the user area, so it rises from its bottom-left corner.
        Some(call_backdrop("connection-backdrop", cx).child(ui::rise(card.transform_origin(ui::corner(false, true)), "connection-card", Duration::ZERO)).into_any_element())
    }
}

pub(super) fn loss(stats: &voice::Stats) -> f32 {
    stats.outbound_loss_pct.unwrap_or(0.)
}

// A ping's band in the connection graph.
pub(super) fn ping_color(ping: u32, cx: &App) -> Hsla {
    match ping {
        p if p < PING_FAIR => palette::status_in(discord::Status::Online, cx),
        p if p <= PING_BAD => palette::status_in(discord::Status::Idle, cx),
        _ => palette::status_in(discord::Status::Dnd, cx),
    }
}

pub(super) fn quality_word(ping: u32) -> &'static str {
    match ping {
        p if p < PING_FAIR => "Good",
        p if p <= PING_BAD => "Fair",
        _ => "Poor",
    }
}

// .voicebar .why: the second line, in the status's tone, saying what needs you.
pub(super) fn why_line(text: impl Into<SharedString>, tone: Hsla) -> Div {
    ui::line(text.into()).h(ui::LINE_DETAIL).text_size(ui::TEXT_XS).text_color(tone)
}

// .voicebar .acts .btn: equal-width 32px buttons; disabled until connected, saying why. `on` (camera on, live,
// in an activity) takes Discord's green: a 12% tint, 24% under the pointer, green ink.
pub(super) fn panel_action(id: &'static str, glyph: Glyph, tip: &'static str, enabled: bool, on: bool, cx: &App) -> Stateful<Div> {
    let green = palette::success();
    let (fill, press, ink) = match on {
        true => (green.opacity(0.12), green.opacity(0.24), palette::positive_ink(cx)),
        false => (palette::selected(cx), palette::press(cx), palette::strong(cx)),
    };
    ui::row() // lint-allow(custom-button): ui::round_button lacks a labelled stage control with a split chevron
        .id(id)
        .flex_1()
        .h_8()
        .squircle(RADIUS_SM)
        .justify_center()
        .bg(fill)
        .text_color(ink)
        .child(Icon::new(glyph).size(ui::ICON_LG))
        .when(enabled, |b| b.cursor_pointer().hover(move |s| s.bg(press)))
        .when(!enabled, |b| b.opacity(0.45).cursor(gpui_kit::CursorStyle::OperationNotAllowed))
        .tooltip(move |window, cx| ui::tip(tip, window, cx))
}

// A connection state's kind, which sets its word's ink: Discord's positive, warning and critical.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ConnState {
    Connected,
    Connecting,
    Error,
}

impl ConnState {
    fn ink(self, cx: &App) -> Hsla {
        match self {
            ConnState::Connected => palette::positive_ink(cx),
            ConnState::Connecting => palette::warning_ink(cx),
            ConnState::Error => palette::danger_ink(cx),
        }
    }
}

// Discord's word for each step of joining (RTCConnectionStatus): "Video Connected" once a camera or stream is on.
pub(super) fn stage_word(stage: Stage, video: bool) -> (&'static str, ConnState) {
    match stage {
        Stage::AwaitingEndpoint => ("Waiting for Voice Server", ConnState::Connecting),
        Stage::Authenticating => ("Authenticating", ConnState::Connecting),
        Stage::RtcConnecting => ("RTC Connecting", ConnState::Connecting),
        Stage::IceChecking => ("Checking Route", ConnState::Connecting),
        Stage::DtlsConnecting => ("DTLS Connecting", ConnState::Connecting),
        Stage::Connected if video => ("Video Connected", ConnState::Connected),
        Stage::Connected => ("Voice Connected", ConnState::Connected),
        Stage::NoRoute => ("No Route", ConnState::Error),
        Stage::Disconnected => ("RTC Disconnected", ConnState::Error),
    }
}

// The voice panel's popovers: the connection's details, and noise suppression.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum VoicePopover {
    Connection,
    Noise,
}

// What Discord's connection popout says while a call isn't up yet, step by step; nothing once connected.
fn stage_help(stage: Stage) -> Option<&'static str> {
    Some(match stage {
        Stage::AwaitingEndpoint => "We're waiting to connect for ya. If your connection gets stuck here, we may be experiencing a region outage.",
        Stage::Authenticating => "Discord has connected to your real-time communication server and is securing the connection.",
        Stage::RtcConnecting => "Locked and loaded! Discord has established a secure connection to your real-time communication server and is attempting to send data.",
        Stage::IceChecking | Stage::DtlsConnecting => "Shields Up! Discord has established a secure connection to your real-time communication server and is attempting to send data.",
        Stage::NoRoute => "Hm, Discord can't establish a connection. We'll try again in a moment.",
        Stage::Disconnected => "Discord's connection has been interrupted. Sorry about that, we'll attempt to re-establish the connection in a moment.",
        Stage::Connected => return None,
    })
}
