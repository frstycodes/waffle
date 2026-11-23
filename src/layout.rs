// Waffle's frame (design/navigation.html): the rail, channel list, chat and side panel run to the window's
// top, each column's own header doubling as the title bar; the user area sits at the foot of the channel list.
use gpui_kit::{
    AnyElement, Context, Div, FontWeight, Hsla, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, NavigationDirection,
    Render, SharedString, Stateful, StyleRefinement, Window,
    component::{ActiveTheme, Icon},
    div, img, linear_color_stop, linear_gradient, prelude::*, px,
};

use crate::{
    Waffle, Drag, Edge, Status,
    discord,
    icons::Glyph,
    login, palette,
    part::Column,
    ui,
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
        crate::perf::probe("Waffle");
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
        // A message select whose menu closed sends its picks (Discord's send on close).
        self.settle_select(cx);
        // First, so every fill below knows whether it sits on the native material this frame.
        let sidebar_width = if self.settings_open() { self.settings_width } else { RAIL_WIDTH + self.channels_width };
        crate::material::sync_sidebar_material(window, sidebar_width, cx.theme().is_dark(), self.prefs.native_sidebar);
        let theme = cx.theme().clone();
        let root = ui::col()
            .id("waffle")
            .track_focus(&self.focus)
            .size_full()
            .when(!crate::material::active(), |d| d.bg(theme.title_bar))
            .text_color(theme.foreground)
            .text_size(ui::TEXT_MD)
            .on_key_down(cx.listener(Self::on_root_key))
            .on_action(cx.listener(|this, _: &crate::app_keys::AddServer, window, cx| this.add_server_shortcut(window, cx)))
            .on_action(cx.listener(Self::toggle_soundboard_key))
            .on_modifiers_changed(cx.listener(|this, e: &gpui_kit::ModifiersChangedEvent, _, cx| {
                if this.shift_held != e.modifiers.shift {
                    this.shift_held = e.modifiers.shift;
                    cx.notify(); // lint-allow(global-notify): shift swaps the actions on message rows (main), menus and popovers (overlays, which only follow the app)
                }
            }))
            .on_mouse_down(MouseButton::Navigate(NavigationDirection::Back), cx.listener(|this, _, window, cx| this.navigate(false, window, cx)))
            .on_mouse_down(MouseButton::Navigate(NavigationDirection::Forward), cx.listener(|this, _, window, cx| this.navigate(true, window, cx)))
            .when(self.drag.is_some(), |root| root.child(window_drag(cx, Self::on_drag_move, Self::end_drag)));
        let root = self.listen_shortcuts(root, cx);
        self.restore_last_channel(window, cx);
        if let Status::SignedOut(login) = &self.status {
            let login = login.clone();
            let back = self.prefs.account.as_ref().and_then(|id| self.prefs.accounts.iter().find(|a| &a.id == id)).cloned();
            return root
                .bg(theme.background)
                .child(login::render(&login, back, &self.updates.status, window, cx))
                .children(crate::window_controls::render(window, cx))
                .into_any_element();
        }
        if let Some(video) = crate::video::fullscreen(cx) {
            crate::gif::set_covered(true);
            return root.child(video).into_any_element();
        }
        self.sync_activity_frame(cx);
        if self.call_fullscreen && self.call_view
            && let Some(call) = self.render_call_with_chat(window, cx)
        {
            let (overlays, call_overlays) = (self.overlays(cx), self.render_call_overlays(window, cx));
            let (menu, dialog) = (self.render_menu(window, cx), self.render_dialog(window, cx).or_else(|| self.render_create_channel(window, cx)).or_else(|| self.render_create_server(window, cx)).or_else(|| self.render_invite(window, cx)).or_else(|| self.render_stage_topic(window, cx)).or_else(|| self.render_modify_attachment(window, cx)).or_else(|| self.render_notification_settings(window, cx)).or_else(|| self.render_new_post(window, cx)));
            let combos = self.render_keyboard_combos(window, cx);
            crate::gif::set_covered(!call_overlays.is_empty() || menu.is_some() || dialog.is_some() || combos.is_some());
            let layer = std::iter::once(overlays).chain(call_overlays).chain(self.render_toast(cx)).chain(menu).chain(dialog).chain(combos);
            return root.child(call).children(self.call_window(cx)).child(over_the_game(layer)).into_any_element();
        }
        // A settings view covers the whole window and its nav takes the sidebar's place, so the app under it
        // isn't drawn: over the native material its own sidebar would show through the settings nav.
        let body = ui::row().items_stretch().relative().flex_1().min_h_0();
        let body = if self.settings_open() {
            body
        } else {
            let nav = self.part(Column::Nav, StyleRefinement::default().w(px(sidebar_width)).h_full().flex_none(), cx);
            let main = self.part(Column::Main, StyleRefinement::default().flex_1().min_w_0().h_full(), cx);
            body.child(self.above_game(nav)).child(main)
        };
        // Anything drawn over the chat pauses its GIFs (the GIF layer paints above the whole app).
        let (overlays, call_overlays) = (self.overlays(cx), self.render_call_overlays(window, cx));
        let (menu, dialog) = (self.render_menu(window, cx), self.render_dialog(window, cx).or_else(|| self.render_create_channel(window, cx)).or_else(|| self.render_create_server(window, cx)).or_else(|| self.render_invite(window, cx)).or_else(|| self.render_stage_topic(window, cx)).or_else(|| self.render_modify_attachment(window, cx)).or_else(|| self.render_notification_settings(window, cx)).or_else(|| self.render_new_post(window, cx)));
        let (viewer, settings) = (self.viewer_overlay(window, cx), self.render_settings(window, cx));
        let channel_settings = self.render_channel_settings(window, cx);
        let server_settings = self.render_server_settings(window, cx);
        let combos = self.render_keyboard_combos(window, cx);
        crate::gif::set_covered(combos.is_some() || !call_overlays.is_empty() || menu.is_some() || dialog.is_some() || viewer.is_some() || settings.is_some() || channel_settings.is_some() || server_settings.is_some());
        // Over everything but popups: a DM call's window, or, away from a server call, the stream we watch.
        // An activity away from its call outranks the DM call's window (it carries the way back); a ring outranks both.
        let activity = (!self.call_view && self.ringing_in.is_none()).then(|| self.activity_pip(cx)).flatten();
        let pip = activity.or_else(|| self.call_window(cx)).or_else(|| (!self.call_view).then(|| self.stream_pip(cx)).flatten());
        let layer = std::iter::once(overlays)
            .chain(call_overlays)
            .chain(viewer)
            .chain(settings)
            .chain(channel_settings)
            .chain(server_settings)
            // Over the settings views too: their menus, confirms and toasts come from the same places.
            .chain(self.render_toast(cx))
            .chain(menu)
            .chain(dialog)
            .chain(combos);
        self.pip_drop_zone(root, cx)
            .child(body)
            .children(pip)
            .child(over_the_game(layer))
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
            Some(bar) => ui::col().size_full().child(bar).child(ui::row().items_stretch().flex_1().min_h_0().child(content)).into_any_element(),
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
                self.render_forum(forum, window, cx)
            }
            None => {
                let chat = self.render_chat(window, cx).into_any_element();
                self.chat_with_activity(chat, cx)
            }
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
        let reconnect = ui::button("voice-notice-reconnect", ui::Tone::Ghost, cx)
            .small()
            .h_6()
            .px_2p5()
            .border_1()
            .border_color(ink.opacity(0.6))
            .text_size(ui::TEXT_SM)
            .text_color(ink)
            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
            .hover(move |s| s.bg(ink.opacity(0.12)))
            .child("Reconnect")
            .on_click(cx.listener(|this, _, _, cx| this.reconnect_voice(cx)));
        let close = ui::icon_button("voice-notice-close", crate::icons::Glyph::X, "Dismiss", cx)
            .small()
            .text_color(ink)
            .hover(move |s| s.bg(ink.opacity(0.12)))
            .on_click(cx.listener(|this, _, _, cx| {
                this.voice_notice = None;
                this.redraw(Column::Main, cx);
            }));
        Some(
            ui::row()
                .flex_none()
                .h_9()
                .pl_4()
                .pr_2()
                .gap_3()
                .bg(fill)
                .text_color(ink)
                .text_size(ui::TEXT_SM)
                .child(ui::row().flex_1().justify_center().gap_3().child(ui::line(text)).child(reconnect))
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
        if keystroke.key == "escape" && self.dismiss_notification_settings(cx) {
            return cx.stop_propagation();
        }
        if keystroke.key == "escape" && self.viewer.is_none() && self.dismiss_overlay(cx) {
            return cx.stop_propagation();
        }
        if keystroke.key == "escape" && self.viewer.is_none() && self.mod_view_escape(cx) {
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
        // Esc closes the voice channel's chat once its messages are scrolled to the bottom, as Discord's.
        if keystroke.key == "escape" && self.call_view && self.call_chat_shown() && !self.scrolled_up {
            self.call_chat = false;
            self.redraw(Column::Main, cx);
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
        let nav = ui::col().relative().flex_none().h_full().w(px(self.settings_width)).child(ui::settings_nav_edge(cx)).child(nav).child(self.resize_handle(Edge::Settings, cx));
        // The page's top band drags the window too, as every other column's header does; it stops above
        // the close button, which sits below the caption buttons.
        let drag = div().absolute().top_0().left_0().right_0().h(px(TITLE_HEIGHT)).window_control_area(gpui_kit::WindowControlArea::Drag);
        let main = ui::row().items_stretch().relative().flex_1().h_full().child(main).child(drag);
        ui::settings_window(id, nav, main, cx)
    }

    fn end_drag(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else { return };
        if drag.edge == Edge::CallChat {
            self.prefs.save();
        }
        cx.notify(); // lint-allow(global-notify): the root's layout sizes every column, so a finished drag redraws them all
    }

    fn on_drag_move(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = &self.drag else { return };
        let dx = f32::from(event.position.x) - drag.start_x;
        match drag.edge {
            Edge::Channels => self.channels_width = (drag.start_width + dx).clamp(CHANNELS_RANGE.0, CHANNELS_RANGE.1),
            // The member list grows leftward.
            Edge::Panel => self.panel_width = (drag.start_width - dx).clamp(PANEL_RANGE.0, PANEL_RANGE.1),
            Edge::Settings => self.settings_width = (drag.start_width + dx).clamp(SETTINGS_RANGE.0, SETTINGS_RANGE.1),
            Edge::CallChat => self.prefs.call_chat_width = crate::call::chat_width(drag.start_width - dx, f32::from(window.viewport_size().width)),
        }
        cx.notify(); // lint-allow(global-notify): a dragged edge resizes columns the root's layout sizes, so all of them redraw
    }

    /// An 8px grab strip over a column's edge drawing a 2px line: accent at 60% on hover, full while
    /// dragging (.panel-grip). Dragging it resizes the column.
    pub(crate) fn resize_handle(&self, edge: Edge, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.drag.as_ref().is_some_and(|d| d.edge == edge);
        let grows_left = matches!(edge, Edge::Panel | Edge::CallChat);
        let accent = palette::accent();
        ui::row()
            .id(match edge {
                Edge::Channels => "resize-channels",
                Edge::Panel => "resize-panel",
                Edge::Settings => "resize-settings",
                Edge::CallChat => "resize-call-chat",
            })
            .absolute()
            .top_0()
            .bottom_0()
            .w_2()
            .when(!grows_left, |h| h.right_neg_1())
            .when(grows_left, |h| h.left_neg_1())
            .group("resize-grip")
            .cursor_col_resize()
            .justify_center()
            .child(div().w_0p5().h_full().rounded_xs().when(active, |l| l.bg(accent)).when(!active, |l| l.group_hover("resize-grip", |l| l.bg(accent.opacity(0.6)))))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    let start_width = match edge {
                        Edge::Channels => this.channels_width,
                        Edge::Panel => this.panel_width,
                        Edge::Settings => this.settings_width,
                        Edge::CallChat => this.call_chat_width(window),
                    };
                    this.drag = Some(Drag { edge, start_x: f32::from(event.position.x), start_width });
                    cx.stop_propagation();
                    cx.notify(); // lint-allow(global-notify): a dragged edge resizes columns the root's layout sizes, so all of them redraw
                }),
            )
    }

    /// The room a header running to the window's right edge leaves for Windows' caption buttons.
    pub(crate) fn caption_clearance(&self, clear: bool) -> Div {
        div().flex_none().when(clear, |d| d.w(px(crate::window_controls::WIDTH)))
    }

    // The rail and the channel list, with the user area floating across the foot of both.
    pub(crate) fn render_nav(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let user_panel = self.me.clone().map(|me| self.user_panel(me, window, cx));
        ui::col()
            .relative()
            .h_full()
            .flex_none()
            .bg(crate::material::sidebar_fill(cx))
            .child(ui::row().items_stretch().flex_1().min_h_0().child(self.render_rail_column(window, cx)).child(self.render_channels(window, cx)))
            .children(user_panel)
    }

    /// Room the columns leave at their foot so their last rows scroll clear of the user area.
    pub(crate) fn user_panel_clearance(&self) -> gpui_kit::Pixels {
        match self.me.is_some() {
            true => px(self.panel_stack_height() + 16.),
            false => gpui_kit::Pixels::ZERO,
        }
    }

    /// The user area's height with the update row and voice panel above it, for overlays that open just over it.
    pub fn panel_stack_height(&self) -> f32 {
        let update = if self.updates.staged().is_some() { UPDATE_HEIGHT } else { 0. };
        PANEL_HEIGHT + update + self.voice_panel_height()
    }

    // Tops the user area once an update has downloaded: which version, and the restart that installs it.
    // Left alone, it installs on the next launch. In a call, Restart asks before leaving it.
    pub(crate) fn update_row(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let staged = self.updates.staged()?;
        let restart = ui::button("restart-to-update", ui::Tone::Primary, cx)
            .small()
            .child("Restart")
            .tooltip(|window, cx| ui::tip("Installs the update and reopens Waffle", window, cx))
            .on_click(cx.listener(|this, _, _, cx| this.request_restart(this.updates.staged().cloned(), cx)));
        let row = ui::row()
            .h(px(UPDATE_HEIGHT))
            .flex_none()
            .pl_3()
            .pr_1p5()
            .gap_2()
            .relative()
            .child(ui::inset_rule(crate::call::PANEL_PAD, cx))
            .child(Icon::new(Glyph::Download).size(ui::ICON_MD).flex_none().text_color(palette::accent_ink(cx)))
            .child(ui::line(format!("Waffle {} is ready", staged.version)).flex_1().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)))
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
                    cx.notify(); // lint-allow(global-notify): the update row (nav) and the Updates settings page (an overlay, which only follows the app) both show the status
                });
                if shown.is_err() {
                    return;
                }
            }
        })
        .detach(); // lint-allow(detached-task): ends when the updater's status stream closes or the app is gone, holding a WeakEntity
    }
}

/// A collectible nameplate behind the user panel, as Discord draws it there: the palette colour as a
/// left-to-right tint (10% → 40%) on `surface` laid solid, as Discord's sits on the panel's own base (over
/// the glass it read washed out), and the art at 40%, `height: 100%; width: auto` pinned to the right edge,
/// so the art's own transparent edge is the fade. `corners` are the radii of the shape it fills (top-left,
/// top-right, bottom-right, bottom-left); every layer rounds itself, since over the glass nothing behind
/// can paint the corners off.
pub fn nameplate(plate: &discord::Nameplate, surface: Hsla, corners: [f32; 4], dark: bool, animate: bool) -> AnyElement {
    let tint = nameplate_tint(plate, dark);
    let [tl, tr, br, bl] = corners.map(px);
    let shape = gpui_kit::Corners { top_left: tl, top_right: tr, bottom_right: br, bottom_left: bl };
    let wash = ui::edge(div().absolute().inset_0(), shape).bg(linear_gradient(90., linear_color_stop(tint.opacity(0.1), 0.), linear_color_stop(tint.opacity(0.4), 1.)));
    ui::edge(div(), shape).absolute().inset_0().overflow_hidden().bg(surface).child(wash).child(nameplate_art(plate, PANEL_HEIGHT, tr, br, animate).opacity(0.4)).into_any_element()
}

pub fn nameplate_tint(plate: &discord::Nameplate, dark: bool) -> Hsla {
    palette::data_color(if dark { plate.tint.0 } else { plate.tint.1 })
}

/// Discord's `height: 100%; width: auto` art, pinned to the right edge and rounding its own right corners.
/// gpui sizes an image's width from its aspect only when the height is absolute, so the row's height
/// stands in for 100%.
// `animate` swaps in the moving art while it loads and plays, the still standing in until then.
// It carries an id either way, since gpui animates only an image with one; the still has one frame.
pub fn nameplate_art(plate: &discord::Nameplate, height: f32, top_right: gpui_kit::Pixels, bottom_right: gpui_kit::Pixels, animate: bool) -> Stateful<gpui_kit::Img> {
    let corners = gpui_kit::Corners { top_left: gpui_kit::Pixels::ZERO, top_right, bottom_right, bottom_left: gpui_kit::Pixels::ZERO };
    let still = plate.art.clone();
    let Some(animated) = plate.animated.clone().filter(|_| animate) else {
        return ui::edge(img(still).absolute().top_0().right_0().h(px(height)), corners).id("nameplate-art");
    };
    let fallback = still.clone();
    let moving = img(animated)
        .loading_at_once()
        .with_loading(move || img(still.clone()).h(px(height)).into_any_element())
        .with_fallback(move || img(fallback.clone()).h(px(height)).into_any_element());
    ui::edge(moving.absolute().top_0().right_0().h(px(height)), corners).id("nameplate-art")
}

/// An avatar with Discord's status shape cut into its bottom-right corner: a dot for online, a moon for
/// idle, a bar for do-not-disturb, a ring for offline, a phone for online on mobile only, a play disc
/// while streaming. Cut-outs are painted in the surface colour.
pub fn avatar_with_status(url: &str, size: f32, status: Option<discord::Mark>, surface: Hsla) -> AnyElement {
    decorated_avatar(url, None, size, status, surface)
}

/// The same, with the user's avatar decoration framing it (drawn at 1.2× the avatar, as Discord does).
/// An animated avatar or decoration holds its first frame; `playing_avatar` moves them.
pub fn decorated_avatar(url: &str, decoration: Option<&str>, size: f32, status: Option<discord::Mark>, surface: Hsla) -> AnyElement {
    ui::badged(avatar_frame(url, decoration, size, None), size, ui::Shape::Round, surface).status(status).into_any_element()
}

/// `decorated_avatar` with its avatar and decoration swapped for their animated versions, playing, keyed
/// by `key` (gpui animates only an image with an id). `url` and `decoration` are the stills, which stay
/// up until the animated ones have loaded, so starting to play never flashes.
pub fn playing_avatar(key: &str, url: &str, decoration: Option<&str>, size: f32, status: Option<discord::Mark>, surface: Hsla) -> AnyElement {
    ui::badged(avatar_frame(url, decoration, size, Some(key)), size, ui::Shape::Round, surface).status(status).into_any_element()
}

fn avatar_frame(url: &str, decoration: Option<&str>, size: f32, key: Option<&str>) -> Div {
    let picture = |still: &str, part: &str, look: fn(gpui_kit::Img) -> gpui_kit::Img| match key {
        Some(key) => playing_image(still, SharedString::from(format!("{key}-{part}")), look).into_any_element(),
        None => look(img(still.to_owned())).into_any_element(),
    };
    let frame = size * 1.2;
    div()
        .relative()
        .size(px(size))
        .flex_none()
        .child(picture(url, "avatar", |i| i.size_full().rounded_full()))
        .children(decoration.map(|d| div().absolute().size(px(frame)).left(px((size - frame) / 2.)).top(px((size - frame) / 2.)).child(picture(d, "decoration", |i| i.size_full()))))
}

// The animated version of `still`, playing, with `still` standing in from the first frame until it has
// loaded (and if it fails). `look` styles both alike.
fn playing_image(still: &str, key: SharedString, look: fn(gpui_kit::Img) -> gpui_kit::Img) -> Stateful<gpui_kit::Img> {
    let animated = discord::animated(still);
    if animated == still {
        return look(img(animated)).id(key);
    }
    let (loading, fallback) = (still.to_owned(), still.to_owned());
    look(img(animated))
        .loading_at_once()
        .with_loading(move || look(img(loading.clone())).into_any_element())
        .with_fallback(move || look(img(fallback.clone())).into_any_element())
        .id(key)
}

/// Where the traffic lights sit at 100%: centred in the 36px title strip, leaving the first 72px to them.
pub const TRAFFIC_LIGHTS: gpui_kit::Point<gpui_kit::Pixels> = gpui_kit::point(px(TRAFFIC_LIGHTS_X), px(TRAFFIC_LIGHTS_Y));
const TRAFFIC_LIGHTS_X: f32 = 12.;
const TRAFFIC_LIGHTS_Y: f32 = 18.;

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

// What draws over the app (dialogs, settings, the viewer, menus, toasts), as one deferred layer after it: an
// activity's webview sits between the app and its deferred draws (activity_frame.rs), so these show over the game.
fn over_the_game(layer: impl Iterator<Item = AnyElement>) -> gpui_kit::Deferred {
    // lint-allow(raw-floating): a transparent full-window layer must not occlude; each overlay in it occludes itself
    gpui_kit::deferred(div().absolute().inset_0().children(layer))
}

// A drag's moves and its release, heard by the window wherever the pointer is: element listeners hear only while
// their element is hovered, and over a native view (an activity's webview) nothing of the app's is, so a drag
// stalled whenever the pointer crossed onto the game. Rendered only while the drag is on.
pub(crate) fn window_drag(
    cx: &mut Context<Waffle>,
    on_move: impl Fn(&mut Waffle, &MouseMoveEvent, &mut Window, &mut Context<Waffle>) + 'static,
    on_up: impl Fn(&mut Waffle, &mut Context<Waffle>) + 'static,
) -> impl IntoElement {
    let this = cx.weak_entity();
    gpui_kit::canvas(
        |_, _, _| {},
        move |_, _, window, _| {
            let moved = this.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if phase == gpui_kit::DispatchPhase::Bubble {
                    _ = moved.update(cx, |this, cx| on_move(this, event, window, cx));
                }
            });
            window.on_mouse_event(move |event: &gpui_kit::MouseUpEvent, phase, _, cx| {
                if phase == gpui_kit::DispatchPhase::Bubble && event.button == MouseButton::Left {
                    _ = this.update(cx, |this, cx| on_up(this, cx));
                }
            });
        },
    )
    .absolute()
    .size_0()
}
