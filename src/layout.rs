// Waffle's frame (design/navigation.html): the rail, channel list, chat and side panel run to the window's
// top, each column's own header doubling as the title bar; the user area sits at the foot of the channel list.
use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, Context, Div, FontWeight, Hsla, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, NavigationDirection,
    Render, SharedString, Stateful, StyleRefinement, Window,
    component::{ActiveTheme, Icon, tooltip::Tooltip},
    div, img, linear_color_stop, linear_gradient, prelude::*, px,
};

use crate::{
    Waffle, Drag, Edge, Status,
    discord,
    icons::Glyph,
    login, palette,
    part::Column,
    sidebar::Nav,
    ui::{self, Squircle},
};

// Room at the window's top for the traffic lights and Windows' caption buttons, which overlays keep clear of.
// The caption buttons run the header line's full height.
pub const TITLE_HEIGHT: f32 = if cfg!(target_os = "windows") { HEADER_HEIGHT } else { 36. };
// Every column's header, which is also where the window drags from.
pub const HEADER_HEIGHT: f32 = 48.;
pub const RAIL_WIDTH: f32 = 68.;
pub const CHANNELS_WIDTH: f32 = 240.;
pub const PANEL_WIDTH: f32 = 264.;
// The user area: its 42px row (name 18 + status 16 beside a 32px avatar, in a 4px-padded button), inset 6px
// all round.
pub const PANEL_HEIGHT: f32 = 54.;
const UPDATE_HEIGHT: f32 = 42.;
const CHANNELS_RANGE: (f32, f32) = (200., 440.);
const PANEL_RANGE: (f32, f32) = (200., 380.);
const SETTINGS_RANGE: (f32, f32) = (200., 360.);

impl Render for Waffle {
    // Everything draws through the app's image cache, which frees images that are no longer drawn.
    // The shell (shell.rs) caches this view's drawing, so this runs only when the app changed; GIFs
    // placed while it runs are played by the GIF layer beside it.
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::gif::begin_layout();
        let images = crate::images::cache(cx);
        images.update(cx, |images, cx| images.next_frame(cx));
        gpui_kit::image_cache(images).size_full().child(self.render_root(window, cx)).into_any_element()
    }
}

impl Waffle {
    fn render_root(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // Before the rows draw, so a row just let go of lands in its new place rather than as a hole.
        self.finish_sorts(cx);
        self.settle_carry(cx);
        // First, so every fill below knows whether it sits on the native material this frame.
        let sidebar_width = if self.settings_open() { self.settings_width } else { RAIL_WIDTH + self.channels_width };
        crate::material::sync_sidebar_material(window, sidebar_width, cx.theme().is_dark(), self.prefs.native_sidebar);
        let theme = cx.theme().clone();
        let root = div()
            .id("waffle")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .when(!crate::material::active(), |d| d.bg(theme.title_bar))
            .text_color(theme.foreground)
            .text_size(px(16.))
            .on_key_down(cx.listener(Self::on_root_key))
            .on_modifiers_changed(cx.listener(|this, e: &gpui_kit::ModifiersChangedEvent, _, cx| {
                if this.shift_held != e.modifiers.shift {
                    this.shift_held = e.modifiers.shift;
                    cx.notify();
                }
            }))
            .on_mouse_down(MouseButton::Navigate(NavigationDirection::Back), cx.listener(|this, _, window, cx| this.navigate(false, window, cx)))
            .on_mouse_down(MouseButton::Navigate(NavigationDirection::Forward), cx.listener(|this, _, window, cx| this.navigate(true, window, cx)))
            .on_mouse_move(cx.listener(Self::on_drag_move))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                if this.drag.take().is_some() {
                    cx.notify();
                }
            }));
        self.restore_last_channel(window, cx);
        if let Status::SignedOut(login) = &self.status {
            let login = login.clone();
            let back = self.prefs.account.as_ref().and_then(|id| self.prefs.accounts.iter().find(|a| &a.id == id)).cloned();
            return root
                .bg(theme.background)
                .child(login::render(&login, back, window, cx))
                .children(crate::window_controls::render(window, cx))
                .into_any_element();
        }
        if let Some(video) = crate::video::fullscreen(cx) {
            crate::gif::set_covered(true);
            return root.child(video).into_any_element();
        }
        if self.call_fullscreen && self.call_view
            && let Some(call) = self.render_call_with_chat(window, cx)
        {
            let (overlays, call_overlays) = (self.render_overlays(window, cx), self.render_call_overlays(window, cx));
            let (menu, dialog) = (self.render_menu(window, cx), self.render_dialog(window, cx).or_else(|| self.render_create_channel(window, cx)).or_else(|| self.render_stage_topic(window, cx)));
            crate::gif::set_covered(!overlays.is_empty() || !call_overlays.is_empty() || menu.is_some() || dialog.is_some());
            return root
                .child(call)
                .children(self.call_window(cx))
                .children(overlays)
                .children(call_overlays)
                .children(self.render_toast(cx))
                .children(menu)
                .children(dialog)
                .into_any_element();
        }
        // A settings view covers the whole window and its nav takes the sidebar's place, so the app under it
        // isn't drawn: over the native material its own sidebar would show through the settings nav.
        let body = div().relative().flex_1().min_h_0().flex();
        let body = if self.settings_open() {
            body
        } else {
            let nav = self.part(Column::Nav, StyleRefinement::default().w(px(sidebar_width)).h_full().flex_none(), cx);
            let main = self.part(Column::Main, StyleRefinement::default().flex_1().min_w_0().h_full(), cx);
            body.child(nav).child(main)
        };
        // Anything drawn over the chat pauses its GIFs (the GIF layer paints above the whole app).
        let (overlays, call_overlays) = (self.render_overlays(window, cx), self.render_call_overlays(window, cx));
        let (menu, dialog) = (self.render_menu(window, cx), self.render_dialog(window, cx).or_else(|| self.render_create_channel(window, cx)).or_else(|| self.render_stage_topic(window, cx)));
        let (viewer, settings) = (self.viewer_overlay(window, cx), self.render_settings(window, cx));
        let channel_settings = self.render_channel_settings(window, cx);
        let server_settings = self.render_server_settings(window, cx);
        crate::gif::set_covered(!overlays.is_empty() || !call_overlays.is_empty() || menu.is_some() || dialog.is_some() || viewer.is_some() || settings.is_some() || channel_settings.is_some() || server_settings.is_some());
        // Over everything but popups: a DM call's window, or, away from a server call, the stream we watch.
        let pip = self.call_window(cx).or_else(|| (!self.call_view).then(|| self.stream_pip(cx)).flatten());
        self.pip_drop_zone(root, cx)
            .child(body)
            .children(pip)
            .children(overlays)
            .children(call_overlays)
            .children(self.render_toast(cx))
            .children(menu)
            .children(dialog)
            .children(viewer)
            .children(settings)
            .children(channel_settings)
            .children(server_settings)
            .children(crate::window_controls::render(window, cx))
            .children(self.render_landing(window, cx))
            .into_any_element()
    }
}

impl Waffle {
    // The column beside the sidebar: the call, the inbox, home, a forum or the chat (a part, see part.rs).
    pub(crate) fn render_main(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        crate::gif::begin_inline_layout();
        let content = self.render_main_content(window, cx);
        match self.voice_notice_bar(cx) {
            Some(bar) => div().size_full().flex().flex_col().child(bar).child(div().flex_1().min_h_0().flex().child(content)).into_any_element(),
            None => content,
        }
    }

    fn render_main_content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match self.call_view.then(|| self.render_call_with_chat(window, cx)).flatten() {
            Some(call) => div().flex_1().min_w_0().h_full().child(call).into_any_element(),
            None if self.inbox_page => self.render_inbox_page(window, cx),
            None if self.requests_page.is_some() && self.guild_id.is_none() && self.channel.is_none() => self.render_requests_page(window, cx),
            None if self.guild_page.is_some() => self.render_guild_page(window, cx),
            None if self.guild_id.is_none() && self.channel.is_none() => self.render_home(window, cx),
            None if self.channel.as_ref().is_some_and(|c| c.kind == discord::ChannelKind::Forum) => {
                let forum = self.channel.clone().expect("checked above");
                self.render_forum(forum, cx)
            }
            None => self.render_chat(window, cx).into_any_element(),
        }
    }

    // Discord's notice bar over the main column: a call we lost (in Discord's words when another client of
    // ours took it, or to Waffle's own restart), with a way to take it back here.
    fn voice_notice_bar(&self, cx: &mut Context<Self>) -> Option<Div> {
        let notice = self.voice_notice.as_ref()?;
        // Losing the call to another client is a fault; a restart's offer is just an offer.
        let (fill, ink, text) = match notice.why {
            crate::NoticeWhy::Elsewhere => (palette::danger(), gpui_kit::white(), "Your voice has been disconnected because you connected at another location.".to_owned()),
            crate::NoticeWhy::Restarted => (palette::accent(), palette::on_accent(), format!("You left {} when Waffle restarted.", notice.channel.name)),
        };
        let reconnect = div()
            .id("voice-notice-reconnect")
            .h(px(24.))
            .px(px(10.))
            .flex()
            .items_center()
            .squircle(crate::ui::RADIUS_XS)
            .border_1()
            .border_color(ink.opacity(0.6))
            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
            .cursor_pointer()
            .hover(move |s| s.bg(ink.opacity(0.12)))
            .child("Reconnect")
            .on_click(cx.listener(|this, _, _, cx| this.reconnect_voice(cx)));
        let close = div()
            .id("voice-notice-close")
            .size(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .squircle(crate::ui::RADIUS_XS)
            .cursor_pointer()
            .hover(move |s| s.bg(ink.opacity(0.12)))
            .child(gpui_kit::component::Icon::new(crate::icons::Glyph::X).size(px(16.)))
            .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Dismiss").build(window, cx))
            .on_click(cx.listener(|this, _, _, cx| {
                this.voice_notice = None;
                cx.notify();
            }));
        Some(
            div()
                .flex_none()
                .h(px(36.))
                .pl(px(16.))
                .pr(px(8.))
                .flex()
                .items_center()
                .gap(px(12.))
                .bg(fill)
                .text_color(ink)
                .text_size(px(14.))
                .child(div().flex_1().min_w_0().flex().items_center().justify_center().gap(px(12.)).child(div().truncate().child(text)).child(reconnect))
                .child(close)
                .child(self.caption_clearance(cfg!(target_os = "windows"))),
        )
    }

    fn on_root_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if self.record_keystroke(keystroke, cx) {
            return cx.stop_propagation();
        }
        if self.dialog_key().is_some_and(|key| crate::feed_keys::combo(keystroke) == key) {
            self.confirm_dialog(window, cx);
            return cx.stop_propagation();
        }
        if keystroke.key == "escape" && self.call_fullscreen {
            return self.set_call_fullscreen(false, window, cx);
        }
        if keystroke.key == "escape" && self.settings.is_some() {
            self.close_settings(cx);
            return cx.stop_propagation();
        }
        if keystroke.key == "escape" && self.server_settings.is_some() {
            self.close_server_settings(false, cx);
            return cx.stop_propagation();
        }
        if keystroke.key == "escape" && self.channel_settings.is_some() {
            self.close_channel_settings(false, cx);
            return cx.stop_propagation();
        }
        if keystroke.key == "k" && (keystroke.modifiers.platform || keystroke.modifiers.control) {
            self.open_switcher(window, cx);
            return cx.stop_propagation();
        }
        if keystroke.key == "escape" && self.viewer.is_none() && self.dismiss_overlay(cx) {
            return cx.stop_propagation();
        }
        // On a DM call's stage, Esc with nothing else to close goes back to the chat.
        if keystroke.key == "escape" && self.call_view && self.call.as_ref().is_some_and(|c| c.guild_id.is_none()) {
            self.back_to_chat(window, cx);
            return cx.stop_propagation();
        }
        if let Some(forward) = history_step(keystroke) {
            self.navigate(forward, window, cx);
            return cx.stop_propagation();
        }
        if keystroke.modifiers.alt && (keystroke.key == "up" || keystroke.key == "down") {
            self.step_channel(if keystroke.key == "up" { -1 } else { 1 }, window, cx);
            return cx.stop_propagation();
        }
        // Esc with nothing else to close marks the channel read and returns to the newest message, as Discord does.
        if keystroke.key == "escape" && self.viewer.is_none() {
            if let Some(id) = self.channel.as_ref().map(|c| c.id.clone()) {
                self.mark_channel_read(&id, cx);
                self.unread_marker = None;
                self.list.scroll_to_end();
            }
            return;
        }
        if self.on_viewer_key(&event.keystroke, cx) {
            cx.stop_propagation();
        }
    }

    pub(crate) fn settings_open(&self) -> bool {
        self.settings.is_some() || self.channel_settings.is_some() || self.server_settings.is_some()
    }

    // A settings view: its nav with a resize handle on the edge, then the page. It covers the window, so it
    // tracks a drag of that handle itself.
    pub(crate) fn settings_frame(&self, id: &'static str, nav: AnyElement, main: impl IntoElement, cx: &mut Context<Self>) -> gpui_kit::Stateful<Div> {
        let nav = div().relative().flex_none().h_full().w(px(self.settings_width)).flex().flex_col().child(ui::settings_nav_edge(cx)).child(nav).child(self.resize_handle(Edge::Settings, cx));
        // The page's top band drags the window too, as every other column's header does; it stops above
        // the close button, which sits below the caption buttons.
        let drag = div().absolute().top_0().left_0().right_0().h(px(TITLE_HEIGHT)).window_control_area(gpui_kit::WindowControlArea::Drag);
        let main = div().relative().flex_1().min_w_0().h_full().flex().child(main).child(drag);
        ui::settings_window(id, nav, main, cx).on_mouse_move(cx.listener(Self::on_drag_move)).on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                if this.drag.take().is_some() {
                    cx.notify();
                }
            }),
        )
    }

    fn on_drag_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = &self.drag else { return };
        let dx = f32::from(event.position.x) - drag.start_x;
        match drag.edge {
            Edge::Channels => self.channels_width = (drag.start_width + dx).clamp(CHANNELS_RANGE.0, CHANNELS_RANGE.1),
            // The member list grows leftward.
            Edge::Panel => self.panel_width = (drag.start_width - dx).clamp(PANEL_RANGE.0, PANEL_RANGE.1),
            Edge::Settings => self.settings_width = (drag.start_width + dx).clamp(SETTINGS_RANGE.0, SETTINGS_RANGE.1),
        }
        cx.notify();
    }

    /// An 8px grab strip over a column's edge drawing a 2px line: accent at 60% on hover, full while
    /// dragging (.panel-grip). Dragging it resizes the column.
    pub(crate) fn resize_handle(&self, edge: Edge, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.drag.as_ref().is_some_and(|d| d.edge == edge);
        let accent = palette::accent();
        div()
            .id(match edge {
                Edge::Channels => "resize-channels",
                Edge::Panel => "resize-panel",
                Edge::Settings => "resize-settings",
            })
            .absolute()
            .top_0()
            .bottom_0()
            .w(px(8.))
            .when(edge != Edge::Panel, |h| h.right(px(-4.)))
            .when(edge == Edge::Panel, |h| h.left(px(-4.)))
            .group("resize-grip")
            .cursor_col_resize()
            .child(div().ml(px(3.)).w(px(2.)).h_full().rounded(px(2.)).when(active, |l| l.bg(accent)).when(!active, |l| l.group_hover("resize-grip", |l| l.bg(accent.opacity(0.6)))))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    let start_width = match edge {
                        Edge::Channels => this.channels_width,
                        Edge::Panel => this.panel_width,
                        Edge::Settings => this.settings_width,
                    };
                    this.drag = Some(Drag { edge, start_x: f32::from(event.position.x), start_width });
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
    }

    /// The room a header running to the window's right edge leaves for Windows' caption buttons.
    pub(crate) fn caption_clearance(&self, clear: bool) -> Div {
        div().flex_none().when(clear, |d| d.w(px(crate::window_controls::WIDTH)))
    }

    // The user area (.me), at the foot of the channel list: who you are, how you look to others, and
    // mute, deafen and settings. Mute and deafen turn coral when off; right-click either for devices.
    // In a call, the voice panel sits on top of it inside the same card.
    pub(crate) fn user_panel(&mut self, me: discord::User, cx: &mut Context<Self>) -> AnyElement {
        let rail = palette::surface(palette::Level::Rail, cx);
        let (card, ring) = (crate::material::card(cx), crate::material::behind_sidebar(rail));
        let sharing = self.call.as_ref().is_some_and(|c| c.sharing.is_some());
        let server_muted = self.call.as_ref().and_then(|c| self.guilds.iter().find(|g| Some(&g.id) == c.guild_id.as_ref())).and_then(|g| g.voice_states.iter().find(|v| v.user_id == me.id)).is_some_and(|v| v.server_muted);
        let (muted, deafened) = (self.muted || self.deafened || server_muted, self.deafened);
        let has_plate = me.nameplate.is_some();
        // Over nameplate art, as Discord does, a muted or deafened control sits on a plate of black at 22%
        // so its red reads; controls that are on stay bare, and there is no box around the row.
        let plate = |on: bool| (has_plate && on).then(|| gpui_kit::black().opacity(0.22));
        let (faint, danger) = (palette::faint(cx), palette::danger_ink(cx));
        let status_line: AnyElement = match (server_muted, sharing) {
            (true, _) => div().truncate().text_color(danger).child("Server muted by a moderator").into_any_element(),
            (false, true) => div().flex().items_center().gap_1().child(Icon::new(Glyph::ShareScreen).size(px(14.)).text_color(palette::positive_ink(cx))).child(div().truncate().child("Sharing their screen")).into_any_element(),
            (false, false) => ui::truncating("me-status", self.status_line()).into_any_element(),
        };
        let mic = me_button("mic", if muted { Nav::MicOff } else { Nav::Mic }, muted, if server_muted { "A moderator muted you here. Only they can unmute you." } else if muted { "Unmute" } else { "Mute" }, cx)
            .when(!server_muted, |b| b.on_click(cx.listener(|this, _, _, cx| this.toggle_mute(cx))))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_device_menu(crate::call::DeviceKind::Input, e.position, cx)))
            .when_some(plate(muted), |b, fill| b.bg(fill));
        let headphones = me_button("deafen", if deafened { Nav::HeadphonesOff } else { Nav::Headphones }, deafened, if deafened { "Undeafen" } else { "Deafen" }, cx)
            .on_click(cx.listener(|this, _, _, cx| this.toggle_deafen(cx)))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_device_menu(crate::call::DeviceKind::Output, e.position, cx)))
            .when_some(plate(deafened), |b, fill| b.bg(fill));
        // A chevron beside each opens that device's popover (devices, volume, and noise suppression for the
        // mic), as Discord's does; right-clicking the button itself opens the same one.
        let menu_open = |kind: crate::call::DeviceKind| self.device_menu.as_ref().is_some_and(|m| m.opened_from_user_area(kind));
        let mic_options = me_chevron("mic-options", "Input options", menu_open(crate::call::DeviceKind::Input), cx)
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_device_menu(crate::call::DeviceKind::Input, e.position, cx)))
            .when_some(plate(muted), |b, fill| b.bg(fill));
        let output_options = me_chevron("output-options", "Output options", menu_open(crate::call::DeviceKind::Output), cx)
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_device_menu(crate::call::DeviceKind::Output, e.position, cx)))
            .when_some(plate(deafened), |b, fill| b.bg(fill));
        let settings = me_button("settings", Nav::Settings, false, "User settings", cx).on_click(cx.listener(|this, _, window, cx| this.open_settings(crate::settings::Page::Account, window, cx)));
        // The card's bottom corners, and its top ones too when nothing sits above; outside them, the sidebar.
        let stacked = self.panel_stack_height() > PANEL_HEIGHT;
        let top = if stacked { 0. } else { ui::RADIUS_MD };
        let corners = [top, top, ui::RADIUS_MD, ui::RADIUS_MD];
        let nameplate = me.nameplate.as_ref().map(|plate| nameplate(plate, palette::surface(palette::Level::Sidebar, cx), corners, cx.theme().is_dark()));
        let hover = palette::hover(cx);
        let who = div()
            .id("me")
            .flex_1()
            .min_w_0()
            .p_1()
            .squircle(ui::RADIUS_SM)
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .when(self.status_menu, |w| w.bg(hover))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_status_menu(cx)))
            .child(decorated_avatar(&me.avatar, me.decoration.as_deref(), 32., Some(self.my_status.into()), ring))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(ui::truncating("me-name", me.name.clone()).line_height(px(18.)).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)))
                    .child(div().line_height(px(16.)).text_size(px(12.)).text_color(faint).child(status_line)),
            );
        let voice = self.voice_panel(cx);
        div()
            .absolute()
            .left_2()
            .right_2()
            .bottom_2()
            .squircle(ui::RADIUS_MD)
            .overflow_hidden()
            .bg(card)
            .flex()
            .flex_col()
            .text_size(px(14.))
            .children(self.update_row(cx))
            .children(voice)
            .child(
                div()
                    .relative()
                    .h(px(PANEL_HEIGHT))
                    .flex_none()
                    .children(nameplate)
                    .child(
                        div()
                            .relative()
                            .size_full()
                            // 6px on every side, so a hover fill never meets the card's edge.
                            .p(px(6.))
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(who)
                            .child(div().relative().flex().items_center().gap_2().child(split_control(mic, mic_options)).child(split_control(headphones, output_options)).child(settings)),
                    ),
            )
            .into_any_element()
    }

    // The rail and the channel list, with the user area floating across the foot of both.
    pub(crate) fn render_nav(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let user_panel = self.me.clone().map(|me| self.user_panel(me, cx));
        div()
            .relative()
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(crate::material::sidebar_fill(cx))
            .child(div().flex_1().min_h_0().flex().child(self.render_rail_column(window, cx)).child(self.render_channels(window, cx)))
            .children(user_panel)
    }

    /// Room the columns leave at their foot so their last rows scroll clear of the user area.
    pub(crate) fn user_panel_clearance(&self) -> gpui_kit::Pixels {
        match self.me.is_some() {
            true => px(self.panel_stack_height() + 16.),
            false => px(0.),
        }
    }

    /// The user area's height with the update row and voice panel above it, for overlays that open just over it.
    pub fn panel_stack_height(&self) -> f32 {
        let update = if self.updates.staged().is_some() { UPDATE_HEIGHT } else { 0. };
        PANEL_HEIGHT + update + self.voice_panel_height()
    }

    // Tops the user area once an update has downloaded: which version, and the restart that installs it.
    // Left alone, it installs on the next launch. In a call, Restart asks before leaving it.
    fn update_row(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let staged = self.updates.staged()?;
        let restart = ui::button("restart-to-update", ui::Tone::Primary, cx)
            .small()
            .child("Restart")
            .tooltip(|window, cx| ui::tip("Installs the update and reopens Waffle", window, cx))
            .on_click(cx.listener(|this, _, _, cx| this.request_restart(this.updates.staged().cloned(), cx)));
        let row = div()
            .h(px(UPDATE_HEIGHT))
            .flex_none()
            .pl(px(12.))
            .pr(px(6.))
            .flex()
            .items_center()
            .gap_2()
            .relative()
            .child(ui::inset_rule(crate::call::PANEL_PAD, cx))
            .child(Icon::new(Glyph::Download).size(px(16.)).flex_none().text_color(palette::accent_ink(cx)))
            .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(format!("Waffle {} is ready", staged.version)))
            .child(restart);
        Some(row.into_any_element())
    }

    pub(crate) fn watch_for_updates(&mut self, cx: &mut Context<Self>) {
        if !crate::updater::enabled() {
            return;
        }
        let (updater, mut statuses) = crate::updater::watch(self.prefs.auto_download_updates);
        self.updates.updater = Some(updater);
        cx.spawn(async move |this, cx| {
            while let Some(status) = futures::StreamExt::next(&mut statuses).await {
                let shown = this.update(cx, |this, cx| {
                    this.updates.apply(status, std::time::SystemTime::now());
                    cx.notify();
                });
                if shown.is_err() {
                    return;
                }
            }
        })
        .detach();
    }
}

/// A collectible nameplate behind the user panel, as Discord draws it there: the palette colour as a
/// left-to-right tint (10% → 40%) on `surface` laid solid, as Discord's sits on the panel's own base (over
/// the glass it read washed out), and the art at 40%, `height: 100%; width: auto` pinned to the right edge,
/// so the art's own transparent edge is the fade. `corners` are the radii of the shape it fills (top-left,
/// top-right, bottom-right, bottom-left); every layer rounds itself, since over the glass nothing behind
/// can paint the corners off.
pub fn nameplate(plate: &discord::Nameplate, surface: Hsla, corners: [f32; 4], dark: bool) -> AnyElement {
    let tint = nameplate_tint(plate, dark);
    let [tl, tr, br, bl] = corners.map(px);
    let shape = gpui_kit::Corners { top_left: tl, top_right: tr, bottom_right: br, bottom_left: bl };
    let wash = ui::edge(div().absolute().inset_0(), shape).bg(linear_gradient(90., linear_color_stop(tint.opacity(0.1), 0.), linear_color_stop(tint.opacity(0.4), 1.)));
    ui::edge(div(), shape).absolute().inset_0().overflow_hidden().bg(surface).child(wash).child(nameplate_art(plate, PANEL_HEIGHT, tr, br, false).opacity(0.4)).into_any_element()
}

pub fn nameplate_tint(plate: &discord::Nameplate, dark: bool) -> Hsla {
    gpui_kit::rgb(if dark { plate.tint.0 } else { plate.tint.1 }).into()
}

/// Discord's `height: 100%; width: auto` art, pinned to the right edge and rounding its own right corners.
/// gpui sizes an image's width from its aspect only when the height is absolute, so the row's height
/// stands in for 100%.
// `animate` swaps in the moving art while it loads and plays, the still standing in until then.
pub fn nameplate_art(plate: &discord::Nameplate, height: f32, top_right: gpui_kit::Pixels, bottom_right: gpui_kit::Pixels, animate: bool) -> gpui_kit::Img {
    let corners = gpui_kit::Corners { top_left: px(0.), top_right, bottom_right, bottom_left: px(0.) };
    let still = plate.art.clone();
    let Some(animated) = plate.animated.clone().filter(|_| animate) else {
        return ui::edge(img(still).absolute().top_0().right_0().h(px(height)), corners);
    };
    let fallback = still.clone();
    let moving = img(animated)
        .with_loading(move || img(still.clone()).h(px(height)).into_any_element())
        .with_fallback(move || img(fallback.clone()).h(px(height)).into_any_element());
    ui::edge(moving.absolute().top_0().right_0().h(px(height)), corners)
}

/// An avatar with Discord's status shape cut into its bottom-right corner: a dot for online, a moon for
/// idle, a bar for do-not-disturb, a ring for offline, a phone for online on mobile only, a play disc
/// while streaming. Cut-outs are painted in the surface colour.
pub fn avatar_with_status(url: &str, size: f32, status: Option<discord::Mark>, surface: Hsla) -> AnyElement {
    decorated_avatar(url, None, size, status, surface)
}

/// The same, with the user's avatar decoration framing it (drawn at 1.2× the avatar, as Discord does).
pub fn decorated_avatar(url: &str, decoration: Option<&str>, size: f32, status: Option<discord::Mark>, surface: Hsla) -> AnyElement {
    // .status: 30% of the avatar (9..16px), ringed 2px in the surface, its corner just past the avatar's.
    let dot = (size * 0.3).clamp(9., 16.).round();
    let inset = size * 0.04 - 2. - STATUS_RING;
    let frame = size * 1.2;
    div()
        .relative()
        .size(px(size))
        .flex_none()
        .child(img(url.to_owned()).size_full().rounded_full())
        .children(decoration.map(|d| img(d.to_owned()).absolute().size(px(frame)).left(px((size - frame) / 2.)).top(px((size - frame) / 2.))))
        .children(status.map(|status| {
            // The phone stands taller than the dot, growing upward from the same corner.
            let tall = if status == discord::Mark::Mobile { dot * PHONE_ASPECT } else { dot };
            ui::cut_out(div().absolute().right(px(inset)).bottom(px(inset)).w(px(dot + 2. * STATUS_RING)).h(px(tall + 2. * STATUS_RING)).rounded(px(if tall > dot { dot * 0.28 + STATUS_RING } else { dot })), surface)
                .flex()
                .items_center()
                .justify_center()
                .child(status_mark(status, dot, surface))
        }))
        .into_any_element()
}

const STATUS_RING: f32 = 2.;

const PHONE_ASPECT: f32 = 1.5;

pub fn status_mark(status: discord::Mark, dot: f32, surface: Hsla) -> AnyElement {
    use discord::{Mark, Status};
    let color = match status {
        Mark::Status(status) => palette::status(status),
        Mark::Mobile => palette::status(Status::Online),
        Mark::Streaming => palette::streaming(),
    };
    let mark = div().relative().size(px(dot)).rounded_full().bg(color).overflow_hidden();
    let cut = |w: f32, h: f32| ui::cut_out(div().absolute().w(px(w)).h(px(h)), surface);
    match status {
        Mark::Status(Status::Online) => mark,
        Mark::Status(Status::Idle) => mark.child(cut(dot * 0.62, dot * 0.62).rounded_full().left(px(-dot * 0.12)).top(px(-dot * 0.12))),
        Mark::Status(Status::Dnd) => mark.flex().items_center().justify_center().child(cut(dot * 0.62, (dot * 0.24).max(2.)).relative().rounded(px(dot))),
        Mark::Status(Status::Offline) => mark.flex().items_center().justify_center().child(cut(dot * 0.46, dot * 0.46).relative().rounded_full()),
        // A handset: a screen cut out of the body, a chin below it for the home bar.
        Mark::Mobile => {
            let side = (dot * 0.18).max(1.5);
            mark.h(px(dot * PHONE_ASPECT)).rounded(px(dot * 0.28)).child(cut(dot - 2. * side, dot * PHONE_ASPECT - side - dot * 0.42).left(px(side)).top(px(side)).rounded(px(dot * 0.08)))
        }
        Mark::Streaming => mark.flex().items_center().justify_center().text_color(gpui_kit::white()).child(Icon::new(Glyph::Play).size(px((dot * 0.62).round()))),
    }
    .into_any_element()
}

/// Where the traffic lights sit at 100%: centred in the 36px title strip, leaving the first 72px to them.
pub const TRAFFIC_LIGHTS: gpui_kit::Point<gpui_kit::Pixels> = gpui_kit::point(px(12.), px(18.));

/// The traffic lights at `zoom`. They are AppKit's and never scale, so their offset follows the zoomed
/// header instead: down by half the header's growth so they stay centred on it, across by half the
/// rail's so they stay centred in its slot; never closer than 8pt to the window's edges.
pub fn traffic_lights(zoom: f32) -> gpui_kit::Point<gpui_kit::Pixels> {
    let grow = zoom - 1.;
    gpui_kit::point(px((12. + crate::layout::RAIL_WIDTH / 2. * grow).max(8.)), px((18. + HEADER_HEIGHT / 2. * grow).max(8.)))
}

// AppKit's three traffic lights and the gaps between them, in points; they keep this size at any zoom.
const LIGHTS_WIDTH: f32 = 54.;

/// How far the traffic lights reach past the rail into the channel list's header, in layout px: none at
/// 100% and above, more as zoom shrinks the rail they sit over.
pub fn traffic_lights_overhang(window: &Window) -> f32 {
    if !traffic_lights_shown(window) {
        return 0.;
    }
    let zoom = window.zoom();
    let right = (f32::from(traffic_lights(zoom).x) + LIGHTS_WIDTH) / zoom + 2.;
    (right - RAIL_WIDTH).max(0.)
}

/// Zooms the window and, on macOS, moves the traffic lights with it.
pub fn apply_zoom(window: &mut Window, zoom: f32, cx: &mut gpui_kit::App) {
    window.set_zoom(zoom, cx);
    #[cfg(target_os = "macos")]
    window.set_traffic_light_position(traffic_lights(zoom));
}

/// Whether macOS draws its traffic lights in the window: windowed, not full screen. They take the rail's slot
/// in the header row; everywhere else that slot holds the Home tile.
pub fn traffic_lights_shown(window: &Window) -> bool {
    cfg!(target_os = "macos") && !window.is_fullscreen()
}

// Back or forward through history: ⌘[ / ⌘] on macOS, Alt+←/→ elsewhere, like a browser.
fn history_step(keystroke: &gpui_kit::Keystroke) -> Option<bool> {
    let m = &keystroke.modifiers;
    let (back, forward) = match cfg!(target_os = "macos") {
        true if m.platform && !m.shift && !m.alt && !m.control => ("[", "]"),
        false if m.alt && !m.shift && !m.control && !m.platform => ("left", "right"),
        _ => return None,
    };
    match keystroke.key.as_str() {
        k if k == back => Some(false),
        k if k == forward => Some(true),
        _ => None,
    }
}

// A user area control (.me .iconbtn): 30px; `on` (muted, deafened) turns it coral. The glyph pops in on
// every flip, keyed by state.
// A control and its chevron as one split pill: round on the outside, square where they meet, 1px apart.
fn split_control(button: Stateful<Div>, chevron: Stateful<Div>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(1.))
        .child(button.rounded_tr(px(0.)).rounded_br(px(0.)))
        .child(chevron.rounded_tl(px(0.)).rounded_bl(px(0.)))
}

// The narrow chevron beside a user-area control that opens its device popover; it reads as part of the
// control, so it is quieter and half as wide.
fn me_chevron(id: &'static str, label: &'static str, open: bool, cx: &App) -> Stateful<Div> {
    let (hover, strong, selected) = (palette::hover(cx), palette::strong(cx), palette::selected(cx));
    div()
        .id(id)
        .w(px(16.))
        .h(px(30.))
        .flex_none()
        .squircle(ui::RADIUS_XS)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(if open { strong } else { palette::faint(cx) })
        .when(open, |b| b.bg(selected))
        .hover(move |s| s.bg(hover).text_color(strong))
        .child(Icon::new(Glyph::ChevronDown).size(px(12.)))
        .tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
}

fn me_button(id: &'static str, glyph: Nav, on: bool, label: &'static str, cx: &App) -> Stateful<Div> {
    let (hover, strong) = (palette::hover(cx), palette::strong(cx));
    let icon = glyph.icon().size(px(20.)).with_animation(SharedString::from(format!("{id}-{on}")), Animation::new(ui::DUR_POP).with_easing(crate::motion::back_out), |icon, t| icon.size(px(20. * (0.7 + 0.3 * t))));
    div()
        .id(id)
        .size(px(30.))
        .flex_none()
        .squircle(ui::RADIUS_SM)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(if on { palette::danger_ink(cx) } else { strong })
        .hover(move |s| s.bg(hover))
        .child(icon)
        .tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
}
