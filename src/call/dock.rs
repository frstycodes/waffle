use crate::ui;
use super::*;

// The connection popover's width, and the height of each of its stat rows and of the ping graph.
const INFO_WIDTH: f32 = 300.;
const INFO_ROW_HEIGHT: f32 = 26.;
const GRAPH_HEIGHT: f32 = 56.;

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
        match (&self.call, &self.voice_error) {
            (Some(_), _) => VOICE_HEIGHT + self.activity_panel_height(),
            (None, Some(_)) => ERROR_HEIGHT,
            (None, None) => 0.,
        }
    }

    pub(crate) fn dismiss_call_menus(&mut self) -> bool {
        std::mem::take(&mut self.connection_info) | self.device_menu.take().is_some() | self.activities.launcher.take().is_some()
    }

    // .voicebar: the connection's state and where you are (or why it needs you), the one button
    // that leaves, then camera, share and activities.
    pub(crate) fn voice_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let Some(call) = &self.call else { return self.voice_error_panel(cx) };
        let link = Link::of(call.stage, call.stats.as_ref());
        let tone = link.tone(cx);
        let connected = call.stage == Stage::Connected;
        let ping = call.stats.as_ref().map(|s| s.ping_ms);
        let place = match call.guild_id.as_ref().and_then(|id| self.guilds.iter().find(|g| &g.id == id)) {
            Some(guild) => format!("{} · {}", call.channel.name, guild.name),
            None => call.channel.name.clone(),
        };
        let channel = call.channel.clone();
        let second: AnyElement = match (link, call.stats.as_ref(), self.voice_warning()) {
            (Link::Poor, Some(stats), _) => why_line(format!("{} ms ping, {:.0}% packet loss", stats.median_ping_ms, loss(stats)), tone).into_any_element(),
            (Link::Lost, _, _) => why_line("Lost the connection, retrying", tone).into_any_element(),
            (_, _, Some(warning)) => why_line(warning, palette::warning_ink(cx))
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
        let bars: AnyElement = match link {
            Link::Connecting | Link::Lost => ui::bars("voice-bars", tone, 12.),
            Link::Connected => ui::bars_still(tone, 12.).into_any_element(),
            Link::Poor => ui::bars_weak(tone, 12.).into_any_element(),
        };
        let tip: SharedString = match (link, ping) {
            (Link::Connected | Link::Poor, Some(ms)) => format!("{ms} ms ping, {} connection. Click for details", quality_word(ms).to_lowercase()).into(),
            _ => "Click for connection details".into(),
        };
        let hover = palette::hover(cx);
        let status = div()
            .id("voice-status")
            .flex_1()
            .min_w_0()
            .mx_neg_1p5()
            .my_neg_1()
            .px_1p5()
            .py_1()
            .squircle(RADIUS_SM)
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .when(self.connection_info, |d| d.bg(hover))
            .child(ui::row().h_5().gap_1p5().text_size(ui::TEXT_SM).font_weight(FontWeight::MEDIUM).text_color(tone).child(bars).child(ui::line(link.label())))
            .child(second)
            .tooltip(move |window, cx| ui::tip(tip.clone(), window, cx))
            .on_click(cx.listener(|this, _, _, cx| {
                this.connection_info = !this.connection_info;
                cx.notify(); // lint-allow(global-notify): the info button highlight is in the nav dock and the connection popover is an overlay
            }));
        let leave_tip = if link == Link::Connecting { "Cancel joining" } else { "Disconnect" };
        let top = ui::row().gap_2().child(status).child(ui::icon_button("hang-up", Glyph::CallEnd, leave_tip, cx).on_click(cx.listener(|this, _, _, cx| this.leave_call_asking(cx))));
        let wait = if link == Link::Lost { "Available once you're reconnected" } else { "Available once you're connected" };
        let ready = |label: &'static str| if connected { label } else { wait };
        let sharing = call.sharing.is_some();
        let camera = panel_action("panel-camera", Glyph::Video, ready(if call.camera_on { "Turn off camera" } else { "Turn on camera" }), connected, false, cx)
            .when(call.camera_on, |b| b.bg(palette::strong(cx)).text_color(palette::surface(palette::Level::Page, cx)))
            .when(connected, |b| b.on_click(cx.listener(|this, _, _, cx| this.toggle_camera(cx))));
        let share = match sharing {
            true => panel_action("panel-share", Glyph::ShareScreen, "Stream options", true, true, cx)
                .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_stream_menu(e.position, cx))),
            false => panel_action("panel-share", Glyph::ShareScreen, ready("Share your screen"), connected, false, cx).when(connected, |b| b.on_click(cx.listener(|this, _, _, cx| this.toggle_screen_share(cx)))),
        };
        let activities = panel_action("panel-activities", Glyph::Activities, ready("Activities"), connected, false, cx)
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
            .h(px(VOICE_HEIGHT + self.activity_panel_height()))
            .flex_none()
            .p(px(PANEL_PAD))
            .gap_2()
            .rounded_t(px(RADIUS_MD)).corner_shape(ui::SQUIRCLE)
            .relative()
            .child(ui::inset_rule(PANEL_PAD, cx))
            .children(self.activity_panel_row(cx)).child(top)
            .child(ui::row().items_stretch().gap_1p5().child(camera).child(share).children(soundboard).child(activities));
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

    // .connpop: how the connection is doing, ping over the last few minutes, the numbers behind it,
    // and how the call is secured.
    pub(super) fn connection_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.connection_info {
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
            .children(stats.last_pings.iter().map(|&p| div().flex_1().min_w_0p5().h(relative((p as f32 / peak).clamp(0.06, 1.))).rounded_t_xs().rounded_b(ui::HAIRLINE).bg(ping_color(p, cx))));
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
        let (lock_ink, secured) = match stats.dave {
            true => (palette::positive_ink(cx), "Voice and video in this call are end-to-end encrypted. Only the people in it can hear them."),
            false => (muted, "Voice and video are encrypted on the way to Discord's servers, not end to end."),
        };
        let card = ui::col()
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
            .child(
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
                    .child(row("Ping", dash(format!("{} ms", stats.median_ping_ms))))
                    .child(row("Packet loss", dash(stats.outbound_loss_pct.map_or("—".into(), |loss| format!("{loss:.1}%")))))
                    .child(row("Server", dash(stats.endpoint.clone()))),
            )
            .child(
                ui::row()
                    .pt_3()
                    .border_t_1()
                    .border_color(palette::line(cx))
                    .items_start()
                    .gap_2()
                    .text_size(ui::TEXT_XS)
                    .text_color(muted)
                    .child(Icon::new(Glyph::HashtagLock).size(ui::ICON_MD).flex_none().text_color(lock_ink))
                    .child(div().flex_1().min_w_0().child(secured)),
            )
            .child(div().text_size(ui::TEXT_XS).text_color(faint).child("Updated every 2 seconds"))
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

// .voicebar .acts .btn: equal-width 32px buttons; disabled until connected, saying why.
// `accent`: the action that's on (our stream), in the accent fill.
pub(super) fn panel_action(id: &'static str, glyph: Glyph, tip: &'static str, enabled: bool, accent: bool, cx: &App) -> Stateful<Div> {
    let (fill, press, ink) = match accent {
        true => (palette::accent(), palette::accent_hover(), palette::on_accent()),
        false => (palette::selected(cx), palette::press(cx), palette::strong(cx)),
    };
    ui::row()
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
