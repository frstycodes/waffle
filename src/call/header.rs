use crate::ui;
use super::*;

impl Waffle {
    // The call view, with the voice channel's chat beside it when open (the stage shrinks; the
    // chat never covers the stream).
    // The panel slides in from the window's right edge and back out the same way, the stage narrowing
    // with it: its width rides one critically damped spring, so a click mid-slide reverses from where
    // it is. The panel keeps its full width inside a clip that grows from the right.
    pub(crate) fn render_call_with_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let Some(call) = self.call.as_ref() else { return self.join_screen(cx) };
        let call_channel = call.channel.clone();
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
                    .child(ui::Button::new("call-chat-close").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).icon(Glyph::X).tooltip_text("Close Chat").occlude().on_click(cx.listener(|this, _, _, cx| {
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
    // stage runs to the window's right edge (`stage_reaches_edge`).
    pub(crate) fn stage_head(&self, header: impl IntoElement, reaches_edge: bool) -> Div {
        ui::row()
            .h(px(FOCUS_HEAD))
            .flex_none()
            .pl(px(STAGE_PAD))
            .pr_2()
            // Drag area: every control inside it occludes it, or Windows' caption takes its clicks.
            .window_control_area(gpui_kit::WindowControlArea::Drag)
            .child(div().flex_1().min_w_0().child(header))
            .child(self.caption_clearance(reaches_edge))
    }

    // Whether the stage reaches the window's right edge: no chat docked beside it, or one still sliding in (its
    // header's room for the caption buttons isn't there yet), or one floating over it.
    pub(crate) fn stage_reaches_edge(&self, window: &mut Window, cx: &mut App) -> bool {
        !self.call_chat_shown() || self.call_chat_floats(window) || self.call_chat_reveal(window, cx) < 1.
    }

    pub(crate) fn call_chat_shown(&self) -> bool {
        self.call_chat && self.call.as_ref().is_some_and(|call| self.channel.as_ref().is_some_and(|c| c.id == call.channel.id))
    }

    // .callhead, as Discord's: the room and its status; a focused stream adds whose screen it is on the left and
    // its quality and LIVE on the right; the participant count shows while the strip is folded away, and Show Chat
    // only while the chat is closed.
    pub(super) fn call_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let call = self.call.as_ref().expect("in a call");
        let me = self.me.as_ref().map(|m| m.id.as_str());
        let streamer = self.focused_stream();
        let connecting = call.stage != Stage::Connected;
        let screen = streamer.as_ref().map(|id| {
            let name = self.user_name(id);
            ui::row().gap_2().min_w_0().child(ui::Separator::new().orientation(ui::Orientation::Vertical).bg(stage::wash(0.24))).child(ui::Avatar::new(self.user_avatar(id), ui::AVATAR_MD)).child(ui::line(format!("{name}’s Screen")).font_weight(FontWeight::MEDIUM))
        });
        let quality = streamer.as_ref().and_then(|id| match me == Some(id.as_str()) {
            true => Some(quality_label(self.prefs.stream_resolution, self.prefs.stream_fps)),
            // GAP: the voice engine doesn't report a remote stream's frame rate, so only its height shows.
            false => call.watched(id)?.frame.as_ref().map(|f| format!("{}P", f.size(0).height.0)),
        });
        let live = streamer.is_some().then(|| ui::row().gap_1().children(quality.map(|q| ui::Badge::empty().colors(gpui_kit::black().opacity(0.56), gpui_kit::white()).font_features(tabular()).child(q.to_uppercase()))).child(ui::Badge::live()));
        let count = self.participants().len();
        let people = (self.call_focus.is_some() && self.stage_ui.strip_hidden).then(|| {
            ui::row().id("call-people").flex_none().gap_1().text_color(stage::muted()).child(Icon::new(Glyph::Users).size(ui::ICON_XL)).child(count.to_string()).tooltip(ui::Tooltip::text("Participants"))
        });
        // A DM call's stage is opened from its window; this is the way back to the chat (Esc too).
        let back = call.guild_id.is_none().then(|| keeps_chrome(stage_button("back-to-chat", "Back to chat", cx), cx).occlude().on_click(cx.listener(|this, _, window, cx| this.back_to_chat(window, cx))));
        let chat = (call.guild_id.is_some() && !self.call_chat_shown()).then(|| self.chat_toggle(cx));
        ui::row()
            .h(px(FOCUS_HEAD))
            .gap_2()
            .text_color(stage::strong())
            .child(
                ui::row()
                    .flex_1()
                    .gap_2()
                    .child(Icon::new(Glyph::VolumeHigh).size(ui::ICON_XL).text_color(stage::muted()))
                    .child(ui::line(self.activity_title(&call.channel.name, cx)).text_size(ui::TEXT_MD).font_weight(FontWeight::SEMIBOLD))
                    .when(connecting, |d| d.child(div().flex_none().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(stage::muted()).child("Connecting")))
                    .children(self.call_status_line(&call.channel, cx))
                    .children(screen),
            )
            .child(ui::row().flex_none().gap_4().children(live).children(people).children(self.activity_invite_button(cx)).children(back).children(chat))
            .into_any_element()
    }

    // The voice channel's chat: "Show Chat" / "Hide Chat", highlighted while mentions wait or for a
    // while after new messages, with the count on its corner (mentions, else unread).
    pub(crate) fn chat_toggle(&self, cx: &mut Context<Self>) -> ui::Button {
        let open = self.call_chat_shown();
        keeps_chrome(stage_icon_button("call-chat", Glyph::ChatRound, if open { "Hide Chat" } else { "Show Chat" }, open || self.call_chat_highlighted(), cx), cx)
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
            (0, unread) => ui::Badge::count(unread),
            (mentions, _) => ui::Badge::count(mentions),
        };
        Some(div().absolute().right_neg_1().bottom_neg_1().rounded_full().border_2().border_color(stage::bg()).child(badge)) // lint-allow(custom-mark): ui::badged lacks a stage-ringed corner badge on a control
    }

    // The chevron above the filmstrip that folds it away and back, as Discord's.
    pub(super) fn strip_toggle(&self, shown: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> { // lint-allow(custom-component): ui::Button lacks a stage tone with a corner badge; this wraps it for the call header
        let (glyph, tip) = if shown { (Glyph::ChevronDown, "Hide Members") } else { (Glyph::ChevronUp, "Show Members") };
        ui::row() // lint-allow(custom-button): ui::round_button lacks a stage toggle with a corner badge
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
            .tooltip(move |window, cx| ui::Tooltip::new(tip).build(window, cx))
            .on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.stage_ui.strip_hidden = !this.stage_ui.strip_hidden;
                this.redraw(Column::Main, cx);
            }))
    }
}
