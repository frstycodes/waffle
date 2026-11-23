// The player's controls drawn over the video: the centre button, the bar, the scrubber, the corners and the volume.
use crate::ui;

use super::*;

impl VideoPlayer {
    // The round play button over the poster, and over the last frame once the video ends.
    pub(super) fn center_button(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement { // lint-allow(custom-component): ui::round_button lacks a video-tone centre play button that fades with the chrome
        let loading = matches!(self.stage, Stage::Loading);
        let size = self.press_frame(Control::Center, CENTER, window, cx);
        let lift = size.target().y_px();
        let side = size.target().width_px().unwrap_or(CENTER);
        let face = ui::row()
            .relative()
            .top(px(lift))
            .size(px(side))
            .rounded_full()
            .bg(gpui_kit::black().opacity(0.6))
            .justify_center()
            .text_color(gpui_kit::white())
            .child(match loading {
                true => loading_dots(),
                false => Icon::new(Glyph::Play).size(px(side / 2.)).into_any_element(),
            });
        let ended = matches!(&self.stage, Stage::Ready(p) if !p.is_playing() && p.is_ended());
        self.control(Control::Center, if ended { "Play Again" } else { "Play" }, ui::row().size(px(CENTER)).justify_center().child(face), |this, _, cx| this.toggle(cx), cx)
    }

    // Discord's bar on a gradient: play/pause, then time and scrubber, then volume and fullscreen.
    pub(super) fn bar(&mut self, alpha: f32, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let (position, duration) = match &self.stage {
            Stage::Ready(p) => (p.position(), p.duration()),
            _ => (Duration::ZERO, Duration::ZERO),
        };
        let position = self.scrub.as_ref().map_or(position, |s| s.at);
        let playing = self.is_playing();
        let play = self.icon_button(Control::Play, if playing { Glyph::Pause } else { Glyph::Play }, if playing { "Pause" } else { "Play" }, |this, _, cx| this.toggle(cx), window, cx);
        let scrubber = self.scrubber(if duration.is_zero() { 0. } else { position.as_secs_f32() / duration.as_secs_f32() }, window, cx);
        let tabular = FontFeatures(Arc::new(vec![("tnum".into(), 1)]));
        let time = ui::row().items_stretch()
            .flex_none()
            .gap_1()
            .text_size(ui::TEXT_XS)
            .font_features(tabular)
            .text_color(gpui_kit::white())
            .child(crate::playback::time_label(position))
            .child(div().text_color(gpui_kit::white().opacity(0.6)).child("/"))
            .child(crate::playback::time_label(duration));
        let shade = |top: f32, bottom: f32| linear_gradient(180., linear_color_stop(gpui_kit::black().opacity(top), 0.), linear_color_stop(gpui_kit::black().opacity(bottom), 1.));
        div()
            .id("video-bar")
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .opacity(alpha)
            .when(!self.fullscreen, |d| d.rounded_b(px(RADIUS)))
            // CSS's three-stop linear-gradient(180deg, transparent, rgba(0,0,0,.68) 45%, rgba(0,0,0,.92)), as two halves.
            .child(div().absolute().top_0().left_0().right_0().h(relative(0.45)).bg(shade(0., 0.68)))
            .child(div().absolute().top(relative(0.45)).bottom_0().left_0().right_0().bg(shade(0.68, 0.92)).when(!self.fullscreen, |d| d.rounded_b(px(RADIUS))))
            .child(
                ui::row()
                    .relative()
                    .pt_7()
                    .pb_2p5()
                    .px_3()
                    .gap_4()
                    .child(play)
                    // The time sits above the scrubber so even a narrow video shows it whole; volume
                    // and full screen live in the top corners (see `corners`).
                    .child(ui::col().flex_1().gap_0p5().child(time).child(scrubber)),
            )
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .into_any_element()
    }

    // Hover lifts a control 1px; pressing shrinks it to 94%, springing back with a slight overshoot.
    pub(super) fn press_frame(&mut self, control: Control, side: f32, window: &mut Window, cx: &App) -> gpui_motion_kit::core::Frame {
        let (hot, pressed) = (self.hot == Some(control), self.pressed == Some(control));
        let target = Target::default().y(if hot && !pressed { -1. } else { 0. }).width(if pressed { side * 0.94 } else { side });
        *self.motions.frame_one(control, Motion::new(target).transition(pop()), window, cx)
    }

    pub(super) fn icon_button( // lint-allow(custom-component): ui::icon_button lacks the OnMedia tone with a toggled state
        &mut self,
        control: Control,
        glyph: Glyph,
        label: &'static str,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let frame = self.press_frame(control, SLOT, window, cx);
        let scale = frame.target().width_px().unwrap_or(SLOT) / SLOT;
        let face = div()
            .relative()
            .top(px(frame.target().y_px()))
            .text_color(gpui_kit::white())
            .opacity(if self.hot == Some(control) { 1. } else { 0.86 })
            .child(Icon::new(glyph).size(px(ICON * scale)));
        let slot = ui::row().size(px(SLOT)).flex_none().justify_center().child(face);
        self.control(control, label, slot, action, cx)
    }

    // Wires a control's hover, press and click; the click stays off the video under it.
    pub(super) fn control(&mut self, control: Control, label: &'static str, body: gpui_kit::Div, action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static, cx: &mut Context<Self>) -> AnyElement {
        body.id(SharedString::from(format!("video-{control:?}")))
            .cursor_pointer()
            .tooltip(move |window, cx| ui::tip(label, window, cx))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                match *hovered {
                    true => this.hot = Some(control),
                    false if this.hot == Some(control) => this.hot = None,
                    false => {}
                }
                cx.notify();
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                this.pressed = Some(control);
                cx.stop_propagation();
                cx.notify();
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                this.pressed = None;
                cx.notify();
            }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                if this.pressed == Some(control) {
                    this.pressed = None;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                window.focus(&this.focus, cx);
                action(this, window, cx);
            }))
            .into_any_element()
    }

    // A 4px track with a accent fill; the knob springs in while the pointer is on it or dragging.
    pub(super) fn scrubber(&mut self, fraction_played: f32, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let active = self.hot == Some(Control::Scrubber) || self.scrub.is_some();
        let knob = self.motions.frame_one(Control::Knob, Motion::new(Target::default().width(if active { 12. } else { 0. })).transition(pop()), window, cx);
        let knob_side = knob.target().width_px().unwrap_or(0.).max(0.);
        let played = relative(fraction_played.clamp(0., 1.));
        let record = self.scrub_bounds.clone();
        div()
            .id("video-scrubber")
            .relative()
            .flex_1()
            .min_w_10()
            .h_4()
            .cursor_pointer()
            .child(canvas(move |bounds, _, _| record.set(bounds), |_, _, _, _| {}).absolute().size_full())
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top_1p5()
                    .h_1()
                    .rounded_xs()
                    .bg(gpui_kit::white().opacity(0.24))
                    .child(div().h_full().w(played).rounded_xs().bg(palette::accent())),
            )
            .child(div().absolute().top(px(KNOB_TOP - knob_side / 2.)).left(played).ml(px(-knob_side / 2.)).size(px(knob_side)).rounded_full().bg(gpui_kit::white())) // lint-allow(custom-mark, raw-margin): ui::slider_live lacks a scrubber with a knob sized by the hover; the margin centres the knob on its point
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                match *hovered {
                    true => this.hot = Some(Control::Scrubber),
                    false if this.hot == Some(Control::Scrubber) => this.hot = None,
                    false => {}
                }
                cx.notify();
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &gpui_kit::MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.begin_scrub(e.position.x, cx);
            }))
            .on_drag(ScrubDrag, |drag, _, _, cx| cx.new(|_| drag.clone()))
            .on_drag_move::<ScrubDrag>(cx.listener(|this, e: &gpui_kit::DragMoveEvent<ScrubDrag>, _, cx| this.scrub_to(e.event.position.x, e.bounds, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| this.end_scrub(cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| this.end_scrub(cx)))
            .into_any_element()
    }

    // The speaker mutes on click; hovering it raises Discord's vertical volume slider above it.
    // Shown while the pointer is over the video (in full screen, until it rests), or while one of
    // them is in use.
    pub(super) fn corners_shown(&self) -> bool {
        let busy = self.adjusting_volume || matches!(self.hot, Some(Control::Volume | Control::VolumePanel | Control::Fullscreen));
        busy || (self.hovered && (!self.fullscreen || self.pointer_at.elapsed() < IDLE))
    }

    // Full screen in the top-left corner and volume in the top-right, each on a dark pill so they
    // read over any picture.
    pub(super) fn corners(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let full = self.icon_button(
            Control::Fullscreen,
            if self.fullscreen { Glyph::FullscreenExit } else { Glyph::Fullscreen },
            if self.fullscreen { "Exit Full Screen" } else { "Full Screen" },
            |this, window, cx| this.set_fullscreen(!this.fullscreen, window, cx),
            window,
            cx,
        );
        let volume = self.volume(window, cx);
        // A click on a pill's edge stays off the video under it.
        let pill = || {
            div().absolute().top_2().p_1().rounded(px(RADIUS)).bg(gpui_kit::black().opacity(0.56)).on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        };
        div()
            .absolute()
            .inset_0()
            .child(pill().left_2().child(full))
            .child(pill().right_2().child(volume))
            .into_any_element()
    }

    pub(super) fn volume(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let glyph = if self.level.heard() == 0. { Glyph::VolumeOff } else { Glyph::VolumeHigh };
        let label = if self.level.muted { "Unmute" } else { "Mute" };
        let button = self.icon_button(Control::Volume, glyph, label, |this, _, cx| this.set_level(Level { muted: !this.level.muted, ..this.level }, cx), window, cx);
        let open = matches!(self.hot, Some(Control::Volume | Control::VolumePanel)) || self.adjusting_volume;
        let hidden = Target::default().opacity(0.).y(-6.);
        let panel = self.motions.frame_one(Control::VolumePanel, Motion::new(Target::default()).initial(hidden).exit(hidden).present(open).transition(pop()), window, cx);
        let level = relative(self.level.heard());
        let record = self.volume_bounds.clone();
        let slider = ui::row().items_stretch()
            .id("video-volume-panel")
            .w_8()
            .h_24()
            .py_3()
            .rounded(px(RADIUS))
            .bg(gpui_kit::black().opacity(0.8))
            .justify_center()
            .cursor_pointer()
            .child(
                div()
                    .relative()
                    .w_1()
                    .h_full()
                    .rounded_xs()
                    .bg(gpui_kit::white().opacity(0.24))
                    .child(canvas(move |bounds, _, _| record.set(bounds), |_, _, _, _| {}).absolute().size_full())
                    .child(div().absolute().bottom_0().left_0().right_0().h(level).rounded_xs().bg(palette::accent()))
                    .child(div().absolute().bottom(level).left_neg_1().mb_neg_1p5().size_3().rounded_full().bg(gpui_kit::white())), // lint-allow(custom-mark, raw-margin): ui::slider_live lacks a vertical volume slider; the margin centres the knob on its level
            )
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &gpui_kit::MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.adjusting_volume = true;
                this.volume_to(e.position.y, this.volume_bounds.get(), cx);
            }))
            .on_drag(VolumeDrag, |drag, _, _, cx| cx.new(|_| drag.clone()))
            .on_drag_move::<VolumeDrag>(cx.listener(|this, e: &gpui_kit::DragMoveEvent<VolumeDrag>, _, cx| {
                if this.adjusting_volume {
                    this.volume_to(e.event.position.y, this.volume_bounds.get(), cx);
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                this.adjusting_volume = false;
                cx.notify();
            }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| {
                if this.adjusting_volume {
                    this.adjusting_volume = false;
                    cx.notify();
                }
            }))
            .on_click(|_, _, cx| cx.stop_propagation());
        // The speaker is in the top corner, so the panel opens below it. The gap between them belongs to
        // the panel, so moving down to it never closes it.
        let reach = div()
            .id("video-volume-reach")
            .absolute()
            .top(px(SLOT))
            .left(px((SLOT - 32.) / 2.))
            .pt_2()
            .child(panel.render(|f| f.apply(slider)))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                match *hovered {
                    true => this.hot = Some(Control::VolumePanel),
                    false if this.hot == Some(Control::VolumePanel) => this.hot = None,
                    false => {}
                }
                cx.notify();
            }));
        div().relative().flex_none().child(button).when(panel.is_mounted(), |d| d.child(reach)).into_any_element()
    }
}
