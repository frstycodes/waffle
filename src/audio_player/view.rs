// The player's controls: its pill, the waveform, the speed and the volume.
use crate::ui;

use super::*;

impl AudioPlayer {
    pub(super) fn voice_pill(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui_kit::Stateful<gpui_kit::Div> {
        let playing = self.is_playing();
        let duration = self.duration();
        let left = duration.map(|d| if self.played { d.saturating_sub(self.position()) } else { d });
        let time = left.map_or("--:--".into(), |d| crate::playback::time_label(Duration::from_secs(d.as_secs_f32().ceil() as u64)));
        let rate = self.rate(cx);
        let rate_label = format!("{}X", rate.to_string().trim_start_matches('0'));
        let needs_length = duration.is_none() && matches!(self.stage, Stage::Idle);
        ui::row()
            .id(SharedString::from(format!("voice-{}", self.key)))
            .mt_1p5() // lint-allow(raw-margin): optical offset from the message text above; the parent is the chat row, outside this file
            .self_start()
            .max_w_full()
            .gap_3()
            .py_2()
            .pl_2()
            .pr_3()
            .squircle(RADIUS_XL)
            .border_1()
            .when(playing, |d| d.bg(palette::accent_soft(cx)).border_color(gpui_kit::transparent_black()))
            .when(!playing, |d| d.bg(palette::surface(Level::Sidebar, cx)).border_color(palette::line(cx)))
            // Discord asks for the file's length on first hover when the message didn't carry it.
            .when(needs_length, |d| d.on_hover(cx.listener(|this, hovered: &bool, _, cx| if *hovered && matches!(this.stage, Stage::Idle) { this.load(false, cx) })))
            .child(self.play_button(36., if playing { "Pause voice message" } else { "Play voice message" }, cx))
            .child(self.waveform(duration, cx))
            .child(div().flex_none().min_w_8().text_right().text_size(ui::TEXT_SM).font_features(tabular()).text_color(palette::muted(cx)).child(time))
            .child(
                ui::pressable(
                    ui::row() // lint-allow(custom-button): ui::pressable lacks a bordered pill with a playing tint; this is the voice message's whole pill
                        .id(SharedString::from(format!("voice-rate-{}", self.key)))
                        .h_6()
                        .min_w_9()
                        .px_1p5()
                        .flex_none()
                        .justify_center()
                        .squircle(RADIUS_XS)
                        .cursor_pointer()
                        .bg(palette::hover(cx))
                        .hover(|s| s.bg(palette::press(cx)).text_color(palette::strong(cx)))
                        .text_size(ui::TEXT_XS)
                        .font_weight(FontWeight::SEMIBOLD)
                        .font_features(tabular())
                        .text_color(palette::muted(cx))
                        .child(rate_label)
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.next_rate(cx);
                        })),
                    0.94,
                ),
            )
            .child(self.volume(window, cx))
    }

    pub(super) fn file_card(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui_kit::Stateful<gpui_kit::Div> {
        let playing = self.is_playing();
        let duration = self.duration();
        let ended = matches!(&self.stage, Stage::Ready(p) if p.is_ended());
        let total = duration.map_or("--:--".into(), crate::playback::time_label);
        let now = crate::playback::time_label(self.position());
        let label = if playing { "Pause" } else if ended { "Play again" } else { "Play" };
        let fraction = duration.filter(|d| !d.is_zero()).map_or(0., |d| self.position().as_secs_f32() / d.as_secs_f32());
        let open = self.file.url.clone();
        ui::col()
            .id(SharedString::from(format!("audio-{}", self.key)))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| this.key_down(e, cx)))
            .mt_2() // lint-allow(raw-margin): optical offset from the message text above; the parent is the chat row, outside this file
            .w_full()
            .max_w(px(PLAYER_MAX))
            .p_3()
            .gap_2p5()
            .squircle(RADIUS_LG)
            .bg(palette::surface(Level::Sidebar, cx))
            .border_1()
            .border_color(palette::line(cx))
            .child(
                ui::row()
                    .gap_3()
                    .child(ui::row().size_10().flex_none().squircle(RADIUS_XS).bg(palette::surface(Level::Raised, cx)).text_color(palette::muted(cx)).justify_center().child(Icon::new(Glyph::File).size(ui::ICON_XL)))
                    .child(
                        ui::col()
                            .flex_1()
                            .child(
                                ui::line(self.file.name.clone())
                                    .id(SharedString::from(format!("audio-name-{}", self.key)))
                                    .cursor_pointer()
                                    .text_size(ui::TEXT_SM)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(palette::strong(cx))
                                    .hover(|s| s.underline())
                                    .on_click(move |_, _, cx| {
                                        cx.stop_propagation();
                                        cx.open_url(&open)
                                    }),
                            )
                            .child(div().text_size(ui::TEXT_XS).text_color(palette::faint(cx)).child(crate::chat::format_size(self.file.size))),
                    ),
            )
            .child(
                ui::row()
                    .gap_2()
                    .child(self.play_button(32., label, cx))
                    .child(div().flex_none().text_size(ui::TEXT_XS).font_features(tabular()).text_color(palette::muted(cx)).child(format!("{now:0>width$} / {total}", width = total.len())))
                    .child(div().flex_1().min_w_0().child(ui::slider_live(SharedString::from(format!("audio-seek-{}", self.key)), fraction, {
                        let this = cx.entity().downgrade();
                        move |f, _, cx| {
                            this.update(cx, |this, cx| this.scrub(f, cx)).ok();
                        }
                    }, window, cx)))
                    .child(self.volume(window, cx)),
            )
    }

    pub(super) fn play_button(&self, side: f32, label: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
        let ended = matches!(&self.stage, Stage::Ready(p) if p.is_ended());
        let glyph = if self.is_playing() { Glyph::Pause } else if ended && !self.voice { Glyph::ArrowCounterClockwise } else { Glyph::Play };
        let loading = matches!(self.stage, Stage::Loading { play: true });
        ui::pressable(
            ui::row() // lint-allow(custom-button): ui::round_button lacks an accent-filled play button that swaps its glyph
                .id(SharedString::from(format!("audio-play-{}", self.key)))
                .size(px(side))
                .flex_none()
                .rounded_full()
                .bg(palette::accent())
                .hover(|s| s.bg(palette::accent_hover()))
                .text_color(palette::on_accent())
                .justify_center()
                .cursor_pointer()
                .when(loading, |d| d.opacity(0.7))
                .tooltip(move |window, cx| ui::tip(label, window, cx))
                .child(Icon::new(glyph).size(px(side * 0.45)))
                .on_click(cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.toggle(cx);
                })),
            0.94,
        )
    }

    pub(super) fn waveform(&self, duration: Option<Duration>, cx: &mut Context<Self>) -> impl IntoElement {
        let width = wave_width(duration.map_or(1., |d| d.as_secs_f32()));
        let levels = resample(&self.file.waveform, ((width + 4.) / PITCH).floor() as usize);
        let fraction = duration.filter(|d| !d.is_zero()).map_or(0., |d| (self.position().as_secs_f32() / d.as_secs_f32()).clamp(0., 1.));
        let (rest, heard) = match (self.played, self.is_playing()) {
            (false, _) => (palette::muted(cx), palette::muted(cx)),
            (true, true) => (palette::faint(cx), palette::accent_ink(cx)),
            (true, false) => (palette::faint(cx), palette::strong(cx)),
        };
        let played = if self.played { fraction } else { 0. };
        let bounds: Rc<Cell<Bounds<Pixels>>> = Rc::default();
        let at = |x: Pixels, bounds: Bounds<Pixels>| ((x - bounds.left()) / bounds.size.width).clamp(0., 1.);
        div()
            .id(SharedString::from(format!("voice-wave-{}", self.key)))
            .w(px(width))
            .h(px(WAVE_HEIGHT))
            .flex_none()
            .cursor_pointer()
            .child(canvas(
                {
                    let bounds = bounds.clone();
                    move |b, _, _| bounds.set(b)
                },
                move |b, _, window, _| paint_waveform(b, &levels, played, rest, heard, window),
            ).size_full())
            .on_mouse_down(MouseButton::Left, {
                let bounds = bounds.clone();
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.begin_scrub(at(e.position.x, bounds.get()), cx);
                })
            })
            .on_drag(WaveDrag, |_, _, _, cx| cx.new(|_| WaveDrag))
            .on_drag_move(cx.listener(move |this, e: &gpui_kit::DragMoveEvent<WaveDrag>, _, cx| this.scrub(at(e.event.position.x, e.bounds), cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| this.end_scrub(cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| this.end_scrub(cx)))
    }

    // The speaker mutes; hovering it shows the slider above, which stays while the pointer is on either.
    pub(super) fn volume(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (volume, muted) = crate::video::volume(cx);
        let silent = muted || volume == 0.;
        let position = if muted { 0. } else { volume.powf(1. / CURVE) };
        let slider = ui::slider_live(SharedString::from(format!("audio-volume-{}", self.key)), position, {
            let this = cx.entity().downgrade();
            move |f, _, cx| {
                this.update(cx, |this, cx| this.set_volume(f, cx)).ok();
            }
        }, window, cx);
        let card = div() // lint-allow(custom-surface): ui::surface lacks an in-flow card that holds itself open on hover
            .id(SharedString::from(format!("audio-volume-card-{}", self.key)))
            .occlude()
            .w(px(VOLUME_WIDTH))
            .px_3()
            .py_1()
            .squircle(RADIUS_SM)
            .bg(palette::surface(Level::Overlay, cx))
            .shadow(ui::shadow_pop(cx))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| this.hold_volume(1, *hovered, cx)))
            // A drag takes hover from the card, so holding the slider keeps it open on its own.
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.hold_volume(2, true, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| this.hold_volume(2, false, cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| this.hold_volume(2, false, cx)))
            .child(slider);
        div()
            .relative()
            .flex_none()
            .child(
                ui::pressable(
                    ui::row() // lint-allow(custom-button): ui::icon_button lacks a hover that also holds the volume card open
                        .id(SharedString::from(format!("audio-mute-{}", self.key)))
                        .size_7()
                        .squircle(RADIUS_XS)
                        .justify_center()
                        .cursor_pointer()
                        .text_color(palette::muted(cx))
                        .hover(|s| s.bg(palette::hover(cx)).text_color(palette::strong(cx)))
                        .child(Icon::new(if silent { Glyph::VolumeOff } else { Glyph::VolumeHigh }).size(ui::ICON_XL))
                        .on_hover(cx.listener(|this, hovered: &bool, _, cx| this.hold_volume(0, *hovered, cx)))
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.toggle_mute(cx);
                        })),
                    0.94,
                ),
            )
            .when(self.volume_open, |d| {
                // lint-allow(raw-floating): rises from its bottom-left at the parent's position, which floating/floating_at can't express; the card occludes itself.
                d.child(div().absolute().top_neg_2().left(px(VOLUME_LEFT)).child(gpui_kit::deferred(gpui_kit::anchored().anchor(Anchor::BottomLeft).snap_to_window_with_margin(ui::WINDOW_MARGIN).child(card)).with_priority(1)))
            })
    }
}
