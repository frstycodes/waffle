use crate::ui;
use super::*;

impl Waffle {
    // The call view, with the voice channel's chat beside it when open (the stage shrinks; the
    // chat never covers the stream).
    // The panel slides in from the window's right edge and back out the same way, the stage narrowing
    // with it: its width rides one critically damped spring, so a click mid-slide reverses from where
    // it is. The panel keeps its full width inside a clip that grows from the right.
    pub(crate) fn render_call_with_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let call_channel = self.call.as_ref()?.channel.clone();
        let reveal = self.call_chat_reveal(window, cx);
        // Leaving shows another channel's messages, so only the call's own chat slides out.
        let on_call_channel = self.channel.as_ref().is_some_and(|c| c.id == call_channel.id);
        let floats = self.call_chat_floats(window);
        let drawer = (reveal > 0.001 && on_call_channel).then(|| {
            let width = self.call_chat_width(window);
            let panel = self.call_chat_drawer(&call_channel, window, cx);
            if floats {
                return self.floating_chat(panel, width, reveal, window, cx);
            }
            self.above_game(div().w(px(width * reveal)).flex_none().h_full().overflow_hidden().child(div().w(px(width)).h_full().child(panel)))
        });
        let (floating, docked) = if floats { (drawer, None) } else { (None, drawer) };
        let call = self.render_call(window, cx)?;
        let previews = self.render_call_previews(cx).map(|p| self.above_game(p));
        Some(ui::row().items_stretch().size_full().child(div().relative().flex_1().min_w_0().h_full().child(call).children(previews).children(floating)).children(docked).into_any_element())
    }

    // 0 closed to 1 open. A critically damped spring keeps its velocity when re-targeted, so reversing
    // mid-slide never snaps; reduced motion lands at once. 260ms: Apple's drawer response (~0.3s) at the
    // app's quicker pace.
    pub(super) fn call_chat_reveal(&self, window: &mut Window, cx: &mut App) -> f32 {
        let target = if self.call_chat_shown() { 1. } else { 0. };
        gpui_kit::base::spring("call-chat-reveal", target, gpui_kit::base::Spring::new(CHAT_SLIDE), window, cx)
    }

    pub(super) fn call_chat_drawer(&mut self, channel: &discord::Channel, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let messages = self.render_messages(0., cx).into_any_element();
        let composer = self.render_composer(window, cx).into_any_element();
        ui::col()
            .relative()
            .size_full()
            .bg(theme.background)
            .border_l_1()
            .border_color(theme.sidebar_border)
            .child(
                ui::row()
                    .h(px(HEADER_HEIGHT))
                    .flex_none()
                    .pl_4()
                    .pr_2()
                    .gap_2()
                    .window_control_area(gpui_kit::WindowControlArea::Drag)
                    .border_b_1()
                    .border_color(theme.sidebar_border)
                    .child(Icon::new(Glyph::VolumeHigh).size(ui::ICON_XL).text_color(theme.muted_foreground))
                    .child(ui::line(channel.name.clone()).flex_1().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)))
                    .child(ui::icon_button("call-chat-close", Glyph::X, "Close Chat", cx).occlude().on_click(cx.listener(|this, _, _, cx| {
                        this.call_chat = false;
                        cx.notify(); // lint-allow(global-notify): closing the call chat reshapes the stage (main) and the columns beside it
                    })))
                    .child(self.caption_clearance(true)),
            )
            .child(div().relative().flex_1().min_h_0().child(messages))
            .child(composer)
            .child(self.resize_handle(crate::Edge::CallChat, cx))
            .into_any_element()
    }

    // Shows or hides the voice channel's text chat beside the call, as Discord's chat button does.
    pub(crate) fn toggle_call_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.call_chat_shown() {
            self.call_chat = false;
            return cx.notify(); // lint-allow(global-notify): closing the call chat reshapes the stage (main) and the columns beside it
        }
        let Some(channel) = self.call.as_ref().map(|c| c.channel.clone()) else { return };
        self.open_call_chat(channel, window, cx);
    }

    pub(crate) fn call_chat_width(&self, window: &Window) -> f32 {
        chat_width(self.prefs.call_chat_width, f32::from(window.viewport_size().width))
    }

    // The header line over the stage: it drags the window, and clears Windows' caption buttons while the
    // stage runs to the window's right edge.
    pub(crate) fn stage_head(&self, header: impl IntoElement) -> Div {
        ui::row()
            .h(px(FOCUS_HEAD))
            .flex_none()
            .pl(px(STAGE_PAD))
            .pr_2()
            // Drag area: every control inside it occludes it, or Windows' caption takes its clicks.
            .window_control_area(gpui_kit::WindowControlArea::Drag)
            .child(div().flex_1().min_w_0().child(header))
            .child(self.caption_clearance(!self.call_chat_shown()))
    }

    pub(crate) fn call_chat_shown(&self) -> bool {
        self.call_chat && self.call.as_ref().is_some_and(|call| self.channel.as_ref().is_some_and(|c| c.id == call.channel.id))
    }

    // .callhead: the room (or whose screen), how long the call has run, who's in it, then the view
    // controls. While watching one stream: its quality, how smoothly it plays and a way to stop.
    pub(super) fn call_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let call = self.call.as_ref().expect("in a call");
        let people = self.participants();
        let watching = call.focused();
        let several = call.watching.len() > 1;
        // A focused stream names the room, then whose it is and what they're playing, as Discord's header.
        let title = match self.focused_stream() {
            Some(id) => match self.activity_name(&id) {
                Some(app) => format!("{} • {} playing {app}", call.channel.name, self.user_name(&id)),
                None => format!("{} • {}'s screen", call.channel.name, self.user_name(&id)),
            },
            None => self.activity_title(&call.channel.name, cx),
        };
        let glyph = Glyph::VolumeHigh;
        let timer = ui::row()
            .h(ui::LINE_LEAD)
            .px_2()
            .flex_none()
            .squircle(RADIUS_XS)
            .gap_1p5()
            .bg(stage::wash(0.08))
            .text_color(stage::muted())
            .text_size(ui::TEXT_XS)
            .font_weight(FontWeight::MEDIUM)
            .font_features(tabular())
            .child(div().size_1p5().rounded_full().bg(stage::online()))
            .child(clock(call.joined.elapsed().as_secs()));
        let faces: Vec<String> = people.iter().take(3).map(|p| p.user.as_ref().map(|u| u.avatar.clone()).unwrap_or_else(|| self.user_avatar(&p.user_id))).collect();
        let count = people.len();
        let who = ui::row()
            .id("call-people")
            .h_8()
            .pl_1()
            .pr_2()
            .flex_none()
            .squircle(RADIUS_SM)
            .gap_1p5()
            .text_color(stage::muted())
            .text_size(ui::TEXT_XS)
            .font_weight(FontWeight::MEDIUM)
            .child(ui::avatar_stack(faces, 22., stage::bg()))
            .child(count.to_string())
            .tooltip(move |window, cx| ui::tip(format!("{count} in {}", if count == 1 { "the call alone" } else { "the call" }), window, cx));
        let health = watching.filter(|w| !several && !w.arrivals.is_empty()).map(|w| {
            let (health, fps) = stream_health(&w.arrivals, Instant::now());
            let lossy = call.issues.contains(&(w.streamer_id.clone(), voice::issues::STREAM_VIEW_HIGH_PACKET_LOSS));
            let (label, color) = match health {
                // No frames because the streamer paused isn't a fault.
                Health::Stalled if self.stream_paused(&w.streamer_id) => ("Paused", stage::muted()),
                Health::Stalled => ("Stalled", stage::dnd()),
                _ if lossy => ("Packet loss", stage::idle()),
                Health::Smooth => ("Smooth", stage::online()),
                Health::Choppy => ("Choppy", stage::idle()),
            };
            // GAP: the frame rate isn't reported by the voice engine, so only the resolution shows.
            let quality = w.frame.as_ref().map(|f| format!("{}p · ", f.size(0).height.0)).unwrap_or_default();
            ui::row()
                .id("stream-health")
                .flex_none()
                .gap_1p5()
                .text_size(ui::TEXT_XS)
                .text_color(stage::muted())
                .child(div().size_1p5().rounded_full().bg(color))
                .child(format!("{quality}{label}"))
                .tooltip(move |window, cx| {
                    // Discord's viewer warnings for the same conditions, with their codes.
                    let mut tip = format!("{fps:.0} frames per second.");
                    if health == Health::Choppy {
                        tip.push_str(&format!(" {} Error code: {}", issue_line(crate::STREAM_VIEW_LOW_FPS), crate::STREAM_VIEW_LOW_FPS));
                    }
                    if lossy {
                        tip.push_str(&format!(" {} Error code: {}", issue_line(voice::issues::STREAM_VIEW_HIGH_PACKET_LOSS), voice::issues::STREAM_VIEW_HIGH_PACKET_LOSS));
                    }
                    ui::tip(tip, window, cx)
                })
        });
        let stop = watching.map(|_| keeps_chrome(stage_button("stop-watching", if several { "Stop all" } else { "Stop watching" }), cx).occlude().on_click(cx.listener(|this, _, _, cx| this.stop_watching(cx))));
        // A DM call's stage is opened from its window; this is the way back to the chat (Esc too).
        let back = call.guild_id.is_none().then(|| keeps_chrome(stage_button("back-to-chat", "Back to chat"), cx).occlude().on_click(cx.listener(|this, _, window, cx| this.back_to_chat(window, cx))));
        ui::row()
            .h(px(HEAD_HEIGHT))
            .gap_2()
            .text_color(stage::strong())
            .child(Icon::new(glyph).size(ui::ICON_XL).text_color(stage::muted()))
            .child(ui::col().child(ui::line(title).text_size(ui::TEXT_MD).line_height(ui::LINE_DETAIL).font_weight(FontWeight::MEDIUM)).children(self.call_status_line(&call.channel, cx)))
            .child(timer)
            .children(health)
            .child(div().flex_1()).children(self.activity_invite_button(cx))
            .children(back)
            .children(stop)
            .child(who)
            .child(div().w(ui::HAIRLINE).h_5().mx_0p5().flex_none().bg(stage::wash(0.14)))
            .child(self.chat_toggle(cx))
            .into_any_element()
    }

    // The voice channel's chat: "Show Chat" / "Hide Chat", highlighted while mentions wait or for a
    // while after new messages, with the count on its corner (mentions, else unread).
    pub(crate) fn chat_toggle(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let open = self.call_chat_shown();
        keeps_chrome(stage_icon_button("call-chat", Glyph::ChatRound, if open { "Hide Chat" } else { "Show Chat" }, open || self.call_chat_highlighted()), cx)
            .relative()
            .children(self.call_chat_badge())
            .occlude()
            .on_click(cx.listener(|this, _, window, cx| this.toggle_call_chat(window, cx)))
    }

    // Only for the voice channel we're in.
    pub(super) fn call_chat_badge(&self) -> Option<Div> {
        let (mentions, unread) = self.call_chat_counts()?;
        let badge = match (mentions, unread) {
            (0, 0) => return None,
            (0, unread) => ui::count_badge(unread),
            (mentions, _) => ui::count_badge(mentions),
        };
        Some(div().absolute().right_neg_1().bottom_neg_1().rounded_full().border_2().border_color(stage::bg()).child(badge))
    }

    // The chevron above the filmstrip that folds it away and back, as Discord's.
    pub(super) fn strip_toggle(&self, shown: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let (glyph, tip) = if shown { (Glyph::ChevronDown, "Hide everyone") } else { (Glyph::ChevronUp, "Show everyone") };
        ui::row()
            .id("strip-toggle")
            .h(px(STRIP_TOGGLE))
            .px_2p5()
            .gap_1p5()
            .rounded_full()
            .bg(gpui_kit::black().opacity(0.6))
            .text_color(gpui_kit::white())
            .cursor_pointer()
            .hover(|s| s.bg(gpui_kit::black().opacity(0.75)))
            .child(Icon::new(glyph).size(ui::ICON_SM))
            .child(Icon::new(Glyph::Users).size(ui::ICON_MD))
            .tooltip(move |window, cx| ui::tip(tip, window, cx))
            .on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.call_strip_hidden = !this.call_strip_hidden;
                this.redraw(Column::Main, cx);
            }))
    }
}
