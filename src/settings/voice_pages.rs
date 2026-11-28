use super::*;

// The Test mic button's least width, a keybind recorder's preferred width and left inset, the reset prompt's least
// width, and a sound cue row's least height.
const MIC_TEST_WIDTH: f32 = 112.;
const RECORDER_BASIS: f32 = 300.;
const RECORDER_INSET: f32 = 11.;
const RESET_PROMPT_WIDTH: f32 = 200.;
const CUE_ROW_HEIGHT: f32 = 52.;

impl Waffle {
    // While a keybind is recording, held keys record it instead of firing anything but releases.
    pub(super) fn on_keys_held(&mut self, held: Vec<&'static str>, cx: &mut Context<Self>) {
        let recording = self.settings.as_ref().and_then(|s| s.recording);
        let Some(hotkeys) = &mut self.hotkeys else { return };
        let was_listening = hotkeys.listening;
        let mut changes = hotkeys.update(held);
        let recorded = hotkeys.record(recording.is_some());
        if recording.is_some() {
            changes.retain(|(_, pressed)| !pressed);
        }
        if let (Some(index), Some(keys)) = (recording, recorded) {
            self.finish_recording(index, keys, cx);
        }
        for (action, pressed) in changes {
            self.on_hotkey(action, pressed, cx);
        }
        if !was_listening {
            // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
            cx.notify();
        }
    }

    // Escape alone cancels, as in Discord.
    pub(super) fn finish_recording(&mut self, index: usize, keys: String, cx: &mut Context<Self>) {
        if let Some(settings) = &mut self.settings {
            settings.recording = None;
        }
        if keys == "escape" {
            // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
            return cx.notify();
        }
        self.update_prefs(cx, |prefs| {
            if let Some(bind) = prefs.keybinds.get_mut(index) {
                bind.keys = Some(keys);
            }
        });
    }

    pub(super) fn on_hotkey(&mut self, action: Action, pressed: bool, cx: &mut Context<Self>) {
        if !pressed && !action.held() {
            return;
        }
        match action {
            Action::PushToTalk if self.prefs.push_to_talk => self.set_talk_key(pressed),
            Action::PushToTalk => {}
            Action::PushToMute => {
                if let Some(call) = &self.call {
                    call.session.set_flags(pressed || self.muted || self.deafened, self.deafened);
                }
            }
            Action::ToggleMute => self.toggle_mute(cx),
            Action::ToggleDeafen => self.toggle_deafen(cx),
            Action::ToggleVoiceMode => self.update_prefs(cx, |p| p.push_to_talk = !p.push_to_talk),
            Action::ToggleScreenShare => self.toggle_share_keybind(cx),
            Action::Disconnect => self.leave_voice(cx),
            Action::ToggleVoiceChannelChat => self.toggle_call_chat_keybind(cx),
            Action::Unassigned => {}
        }
        // lint-allow(global-notify): a hotkey can toggle mute, deafen, sharing or the call chat, which the nav's voice bar, the call stage and the open page each draw
        cx.notify();
    }

    pub(crate) fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        // Unmuting while deafened undeafens too, as in Discord.
        let muted = !(self.muted || self.deafened);
        self.muted = muted;
        if !muted {
            self.deafened = false;
        }
        self.keep_mic_test_choice();
        sounds::play(if muted { Cue::Mute } else { Cue::Unmute });
        self.sync_voice_flags();
        // lint-allow(global-notify): mute shows on the nav's voice bar, the call stage and the settings window at once
        cx.notify();
    }

    pub(crate) fn toggle_deafen(&mut self, cx: &mut Context<Self>) {
        self.deafened = !self.deafened;
        self.keep_mic_test_choice();
        sounds::play(if self.deafened { Cue::Deafen } else { Cue::Undeafen });
        self.sync_voice_flags();
        // Deafening stops every soundboard sound playing, as Discord's does.
        if self.deafened {
            self.soundboard.stop_playing();
            self.soundboard.close_picker();
            // No one is heard while deafened, so no one shows as speaking.
            let me = self.me.as_ref().map(|m| m.id.clone());
            if let Some(call) = &mut self.call {
                call.speaking.retain(|id| Some(id) == me.as_ref());
            }
        }
        // lint-allow(global-notify): deafen shows on the nav's voice bar, the call stage and the settings window at once
        cx.notify();
    }

    // Discord's Mic Test: our processed mic, looped back to the speaker, metered. Deafened while it
    // runs, so a call's voices don't play over it.
    pub(crate) fn toggle_mic_test(&mut self, cx: &mut Context<Self>) {
        if self.mic_test.is_some() {
            self.stop_mic_test();
            // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
            return cx.notify();
        }
        match voice::mic_test(None, &self.prefs.voice_settings()) {
            Ok((test, mut levels)) => {
                let task = cx.spawn(async move |this, cx| {
                    while let Some(level) = futures::StreamExt::next(&mut levels).await {
                        if this.update(cx, |this, cx| {
                            this.mic_level = level.db;
                            if let Some(view) = &mut this.settings {
                                (view.mic_speaking, view.mic_input) = (level.speaking, level.input_detected);
                            }
                            // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
                            cx.notify();
                        })
                        .is_err()
                        {
                            return;
                        }
                    }
                    // The test ended by itself (its device failed): give deafen back.
                    let _ = this.update(cx, |this, cx| {
                        this.stop_mic_test();
                        // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
                        cx.notify();
                    });
                });
                let held = HeldDeafen::hold(&mut self.deafened);
                self.sync_voice_flags();
                self.mic_test = Some(MicTestRun { _test: test, _task: task, held });
            }
            Err(error) => self.voice_error = Some(error.into()),
        }
        // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
        cx.notify();
    }

    // Ends the mic test, if one runs, and restores deafen as the user last chose it.
    pub(crate) fn stop_mic_test(&mut self) {
        let Some(run) = self.mic_test.take() else { return };
        run.held.release(&mut self.deafened);
        self.mic_level = crate::SILENCE_DB;
        self.sync_voice_flags();
    }

    // A mute or deafen toggle during the mic test is what its end gives back.
    pub(super) fn keep_mic_test_choice(&mut self) {
        if let Some(run) = &mut self.mic_test {
            run.held.chosen(self.deafened);
        }
    }

    // While a keybind is recording the key hook records it (on_keys_held); keystrokes here only
    // cancel on Escape and are kept from the rest of the app.
    pub(crate) fn record_keystroke(&mut self, keystroke: &gpui_kit::Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(settings) = &mut self.settings else { return false };
        if settings.recording.is_none() {
            return false;
        }
        if keystroke.key == "escape" && !keystroke.modifiers.modified() {
            settings.recording = None;
            // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
            cx.notify();
        }
        true
    }

    pub(super) fn voice_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Section> {
        let prefs = self.prefs.clone();
        let view = self.settings.as_ref().expect("settings are open");
        let (inputs, outputs, cameras) = (view.devices.inputs.clone(), view.devices.outputs.clone(), view.devices.cameras.clone());
        let advanced = view.advanced;
        let (custom, studio) = (prefs.input_profile == InputProfile::Custom, prefs.input_profile == InputProfile::Studio);
        let mut out = vec![Section::with_anchor("Voice", self.devices_group(&inputs, &outputs, cx))];

        let mut lines = Hairlines::default();
        let profiles = [
            ("profile-isolation", "Voice isolation", "Just your beautiful voice: Waffle cuts through the noise around you", InputProfile::VoiceIsolation),
            ("profile-studio", "Studio", "Pure audio: an open mic with no processing", InputProfile::Studio),
            ("profile-custom", "Custom", "Advanced mode: every button and dial, set your way", InputProfile::Custom),
        ];
        let profile_rows: Vec<Stateful<Div>> = profiles
            .into_iter()
            .map(|(id, title, detail, value)| {
                radio_row(id, lines.next(), title, detail, prefs.input_profile == value, cx).on_click(cx.listener(move |this, _, _, cx| this.update_prefs(cx, |p| p.set_profile(value))))
            })
            .collect();
        out.push(Section::keyed(
            "Input profile",
            ui::settings_group("Input profile", cx)
                .child(ui::Card::new(cx).children(profile_rows))
                .when(!custom, |g| g.child(ui::settings_caption("This profile sets noise suppression, echo cancellation and sensitivity for you. Pick Custom to change them.", cx))),
        ));
        if !studio {
            out.push(Section::keyed("Input mode", ui::settings_group("Input mode", cx).child(self.input_mode_card(custom, window, cx))));
        }
        if custom {
            let mut lines = Hairlines::default();
            let noise = self.noise_row(lines.next(), window, cx);
            let echo = self.switch_row(SwitchRow { id: "set-echo", title: "Echo cancellation", detail: Some("Stops others hearing themselves through your speakers"), on: prefs.echo_cancellation, enabled: true, field: |p| &mut p.echo_cancellation }, lines.next(), window, cx);
            out.push(Section::keyed("Voice processing", ui::settings_group("Voice processing", cx).child(ui::Card::new(cx).child(noise).child(echo))));
        }
        let expander = expander_row("voice-advanced", false, if advanced { "Hide advanced voice settings" } else { "Show advanced voice settings" }, &advanced_summary(custom), advanced, window, cx).on_click(cx.listener(|this, _, _, cx| {
            if let Some(s) = &mut this.settings {
                s.advanced = !s.advanced;
            }
            // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
            cx.notify();
        }));
        let advanced_rows = advanced.then(|| reveal("voice-advanced-body", self.advanced_voice_rows(custom, window, cx)));
        out.push(Section::new(ui::Card::new(cx).child(expander).children(advanced_rows)));

        let camera = ui::col()
            .gap_1p5()
            .child(field_label("Camera", cx))
            .child(self.device_picker(Select::Camera, Glyph::Video, &cameras, prefs.camera.as_deref(), 40., cx));
        // GAP: the design's camera preview tile (Test video, mirrored self view) isn't on this page; the
        // call's camera preview dialog (camera_preview.rs) shows one.
        let always_preview = self.switch_row(SwitchRow { id: "set-always-preview-video", title: "Always preview video", detail: Some("Shows your camera before it turns on in a call"), on: prefs.always_preview_video, enabled: true, field: |p| &mut p.always_preview_video }, false, window, cx);
        out.push(Section::with_anchor("Camera", ui::settings_group("Camera", cx).child(card_pad(cx).child(camera)).child(ui::Card::new(cx).child(always_preview))));
        out.push(Section::with_anchor("Sounds", self.sounds_group(window, cx)));
        out.push(Section::with_anchor("Advanced", ui::settings_group("Advanced", cx).child(ui::Card::new(cx).child(self.reset_row(cx)))));
        out
    }

    pub(super) fn devices_group(&mut self, inputs: &[(String, String)], outputs: &[(String, String)], cx: &mut Context<Self>) -> Div {
        let prefs = self.prefs.clone();
        let view = self.settings.as_ref().expect("settings are open");
        let testing = self.mic_test.is_some();
        // The mic test's "isn't detecting any input", or in a call the No audio detected warning.
        let in_call_silent = prefs.silence_warning && self.call.as_ref().is_some_and(|c| c.session.input_detected() == Some(false));
        let no_input = if testing { view.mic_input == Some(false) } else { in_call_silent };
        let column = |label: &'static str, field: AnyElement, cx: &App| ui::col().min_h_0().flex_1().gap_1p5().child(field_label(label, cx)).child(field);
        let percent = |v: f32| format!("{}%", (v * 100.).round() as i32);
        let devices = ui::row().items_stretch()
            .gap_6()
            .child(column("Microphone", self.device_picker(Select::Microphone, Glyph::Mic, inputs, prefs.input_device.as_deref(), 40., cx), cx))
            .child(column("Speaker", self.device_picker(Select::Speaker, Glyph::Headphones, outputs, prefs.output_device.as_deref(), 40., cx), cx));
        let volumes = ui::row().items_stretch()
            .gap_6()
            .child(div().flex_1().min_w_0().child(self.volume("Microphone volume", &percent(prefs.input_volume), Slider::InputVolume, cx)))
            .child(div().flex_1().min_w_0().child(self.volume("Speaker volume", &percent(prefs.output_volume), Slider::OutputVolume, cx)));
        // Discord's own name for it; it sits under the speaker it plays through.
        let soundboard = format!("{}%", crate::soundboard::perceptual(prefs.soundboard_volume).round() as i32);
        let soundboard = ui::row().items_stretch().gap_6().child(div().flex_1()).child(div().flex_1().min_w_0().child(self.volume("Soundboard Volume", &soundboard, Slider::SoundboardVolume, cx)));
        let test_button = ui::Button::new("mic-test")
            .min_w(px(MIC_TEST_WIDTH))
            .child(Icon::new(if testing { Glyph::Stop } else { Glyph::Mic }).size(ui::ICON_MD))
            .child(if testing { "Stop testing" } else { "Test mic" })
            .on_click(cx.listener(|this, _, _, cx| this.toggle_mic_test(cx)));
        let mic_test = ui::row().gap_4().child(test_button).child(mic_wave(self.mic_level, testing, cx));
        let speaker = outputs.iter().find(|(id, _)| Some(id.as_str()) == prefs.output_device.as_deref()).map_or("your speaker", |(_, name)| name.as_str());
        let mic = inputs.iter().find(|(id, _)| Some(id.as_str()) == prefs.input_device.as_deref()).map_or("your microphone", |(_, name)| name.as_str());
        let notice = no_input.then(|| self.no_input_notice(mic, cx));
        ui::settings_group("Devices", cx).child(
            card_pad(cx)
                .child(ui::col().gap_4().child(devices).child(volumes).child(soundboard))
                .child(mic_test)
                .children(notice)
                .child(ui::settings_caption(format!("Talk normally. You'll hear yourself back through {speaker}."), cx).px_0()),
        )
    }

    // Names the device and every likely fix, with the fixes one click away.
    pub(super) fn no_input_notice(&self, mic: &str, cx: &mut Context<Self>) -> Div {
        let actions = ui::row().items_stretch()
            .flex_wrap()
            .gap_2()
            .child(ui::Button::new("nomic-pick").variant(ui::ButtonVariant::Secondary).size(ui::ButtonSize::Sm).child("Choose another input").on_click(cx.listener(|this, _, _, cx| {
                if let Some(s) = &mut this.settings {
                    s.select = Some(Select::Microphone);
                }
                // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
                cx.notify();
            })))
            .when(cfg!(target_os = "macos"), |d| {
                d.child(ui::Button::new("nomic-system").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::Sm).child("Open System Settings").on_click(|_, _, cx| cx.open_url(MAC_MIC_PRIVACY)))
            });
        let actions = div().pt_2().child(actions);
        ui::Alert::with_actions(
            ui::Notice::Warning,
            Glyph::MicOff,
            "No audio detected from your mic",
            format!("Waffle hears nothing from {mic}. Turn up Microphone volume above, pick another input, or let Waffle use the microphone in System Settings."),
            actions,
            cx,
        )
    }

    pub(super) fn volume(&self, label: &'static str, value: &str, slider: Slider, cx: &mut Context<Self>) -> Div {
        ui::col()
            .gap_2()
            .child(
                ui::row().items_stretch()
                    .justify_between()
                    .text_size(ui::TEXT_XS)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette::muted(cx))
                    .child(label)
                    .child(div().text_color(palette::strong(cx)).child(value.to_owned())),
            )
            .child(self.level_range(slider, None, cx))
    }

    // Voice activity or push to talk; push to talk shows its shortcut and release delay, voice
    // activity under Custom its sensitivity.
    pub(super) fn input_mode_card(&self, custom: bool, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let prefs = &self.prefs;
        let ptt = prefs.push_to_talk;
        let mut lines = Hairlines::default();
        let mode = |id: &'static str, title: &'static str, detail: &'static str, on_ptt: bool, line: bool, cx: &mut Context<Self>| {
            radio_row(id, line, title, detail, ptt == on_ptt, cx).on_click(cx.listener(move |this, _, _, cx| this.update_prefs(cx, |p| p.push_to_talk = on_ptt)))
        };
        let mut card = ui::Card::new(cx).child(mode("mode-vad", "Voice activity", "Transmits when you speak above the sensitivity", false, lines.next(), cx)).child(mode(
            "mode-ptt",
            "Push to talk",
            "Transmits while you hold a key",
            true,
            lines.next(),
            cx,
        ));
        if ptt {
            let ptt_index = prefs.keybinds.iter().position(|k| k.action == Action::PushToTalk);
            card = card
                .child(ui::Item::setting("ptt-shortcut", lines.next(), cx).child(ui::settings_text("Shortcut", Some("Also listed under Keybinds"), cx)).children(ptt_index.map(|i| self.keybind_recorder(i, cx).flex_basis(px(RECORDER_BASIS)).flex_shrink(1.))))
                .child(
                    stack_row("ptt-delay", lines.next(), cx)
                        .child(numbered("Push to talk release delay", "Keeps transmitting this long after you let go, so word endings aren't cut", &format!("{} ms", prefs.ptt_release_ms), cx))
                        .child(self.level_range(Slider::ReleaseDelay, None, cx)),
                );
        } else if custom {
            let auto = prefs.sensitivity_auto;
            card = card
                .child(self.switch_row(
                    SwitchRow { id: "set-auto-sensitivity", title: "Set sensitivity automatically", detail: Some("Waffle adjusts to the noise around you. When the bar fills, you're transmitting."), on: auto, enabled: true, field: |p| &mut p.sensitivity_auto },
                    lines.next(),
                    window,
                    cx,
                ))
                .child(
                    stack_row("input-sensitivity", lines.next(), cx)
                        .child(ui::settings_text("Input sensitivity", Some(if auto { "Your mic transmits when Waffle hears your voice" } else { "Your mic transmits when the level passes the handle" }), cx))
                        .child(if auto { sensitivity_track(None, self.mic_level, cx).into_any_element() } else { self.level_range(Slider::Sensitivity, Some(self.mic_level), cx) })
                        .when(!auto, |d| d.child(scale(&["\u{2212}100 dB", "0 dB"], cx))),
                );
        }
        card
    }

    pub(super) fn sounds_group(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let more = self.settings.as_ref().is_some_and(|s| s.more_sounds);
        let mut lines = Hairlines::default();
        let first: Vec<AnyElement> = VOICE_CUES.iter().map(|&(id, label, cue)| self.cue_row(id, label, cue, lines.next(), window, cx)).collect();
        let expander = expander_row(
            "more-sounds",
            lines.next(),
            &if more { format!("Hide {} sounds", MORE_CUES.len()) } else { format!("Show {} more sounds", MORE_CUES.len()) },
            "Voice disconnected, PTT activate, User join, and more",
            more,
            window,
            cx,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            if let Some(s) = &mut this.settings {
                s.more_sounds = !s.more_sounds;
            }
            // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
            cx.notify();
        }));
        let mut body = Hairlines(true);
        let rest = more.then(|| reveal("more-sounds-body", ui::col().children(MORE_CUES.iter().map(|&(id, label, cue)| self.cue_row(id, label, cue, body.next(), window, cx)))));
        ui::settings_group("Sounds", cx).child(ui::Card::new(cx).children(first).child(expander).children(rest)).child(ui::settings_caption("Message and mention sounds are under Notifications.", cx))
    }

    // Reset asks in place, naming what happens; once done it offers Undo for a few seconds.
    pub(super) fn reset_row(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let view = self.settings.as_ref().expect("settings are open");
        let (asking, done) = (view.asking == Some(Ask::Reset), view.reset_undo.is_some());
        let trigger = (!asking && !done).then(|| {
            ui::Button::new("reset-voice").variant(ui::ButtonVariant::Secondary).size(ui::ButtonSize::Sm).text_color(palette::danger_ink(cx)).child("Reset voice settings").on_click(cx.listener(|this, _, _, cx| this.set_ask(Some(Ask::Reset), cx)))
        });
        let ask = asking.then(|| {
            reveal(
                "reset-ask",
                ui::row()
                    .w_full()
                    .py_2p5()
                    .px_3()
                    .squircle(ui::RADIUS_MD)
                    .bg(palette::danger().opacity(0.1))
                    .flex_wrap()
                    .gap_2()
                    .child(div().flex_1().min_w(px(RESET_PROMPT_WIDTH)).font_weight(FontWeight::NORMAL).text_color(palette::strong(cx)).child("Reset every voice and video setting to its default?"))
                    .child(
                        ui::row().items_stretch()
                            .gap_2()
                            .child(ui::Button::new("reset-keep").variant(ui::ButtonVariant::Secondary).size(ui::ButtonSize::Sm).child("Keep my settings").on_click(cx.listener(|this, _, _, cx| this.set_ask(None, cx))))
                            .child(ui::Button::new("reset-yes").variant(ui::ButtonVariant::Destructive).size(ui::ButtonSize::Sm).child("Reset voice settings").on_click(cx.listener(|this, _, _, cx| this.reset_voice(cx)))),
                    ),
            )
        });
        let done = done.then(|| {
            ui::row()
                .gap_1p5()
                .font_weight(FontWeight::NORMAL)
                .text_color(palette::positive_ink(cx))
                .child(Icon::new(Glyph::CheckCircle).size(ui::ICON_MD))
                .child("Voice and video settings reset")
                .child(div().pl_1().child(ui::Button::new("reset-undo").variant(ui::ButtonVariant::Link).child("Undo").on_click(cx.listener(|this, _, _, cx| this.undo_reset(cx)))))
        });
        ui::Item::setting("reset", false, cx)
            .flex_wrap()
            .child(ui::settings_text("Reset voice and video settings", Some("Puts devices, levels, processing and sounds back to their defaults. Your keybinds and theme stay."), cx))
            .children(trigger)
            .children(done)
            .children(ask)
    }

    pub(super) fn reset_voice(&mut self, cx: &mut Context<Self>) {
        let before = self.prefs.clone();
        self.update_prefs(cx, |p| {
            let keep = (p.theme, p.keybinds.clone(), p.app_icon);
            *p = Prefs::default();
            (p.theme, p.keybinds, p.app_icon) = keep;
        });
        if let Some(view) = &mut self.settings {
            (view.asking, view.reset_undo) = (None, Some(before));
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(UNDO_FOR).await;
            this.update(cx, |this, cx| {
                if let Some(view) = &mut this.settings {
                    view.reset_undo = None;
                }
                // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
                cx.notify();
            })
            .ok();
        })
        // lint-allow(detached-task): a one-shot task that ends when its request answers, and it updates through a WeakEntity so a closed view drops the result
        .detach();
    }

    pub(super) fn undo_reset(&mut self, cx: &mut Context<Self>) {
        let Some(before) = self.settings.as_mut().and_then(|v| v.reset_undo.take()) else { return };
        self.update_prefs(cx, |p| *p = before);
    }

    pub(super) fn keybinds_page(&self, cx: &mut Context<Self>) -> Vec<Section> {
        let header = ui::row()
            .items_start()
            .gap_4()
            .child(ui::col().min_h_0().flex_1().gap_1().child(ui::settings_title("Keybinds", cx)).child(div().text_color(palette::muted(cx)).child("Shortcuts that work everywhere, even while another app is in front.")))
            .child(ui::Button::new("add-keybind").child(Icon::new(Glyph::Plus).size(ui::ICON_MD)).child("Add a keybind").on_click(cx.listener(|this, _, _, cx| {
                this.update_prefs(cx, |p| p.keybinds.push(Keybind { action: Action::Unassigned, keys: None }));
                let index = this.prefs.keybinds.len() - 1;
                if let Some(s) = &mut this.settings {
                    s.recording = Some(index);
                }
            })));
        let mut lines = Hairlines::default();
        let rows: Vec<AnyElement> = self.prefs.keybinds.iter().enumerate().map(|(index, keybind)| self.keybind_row(index, keybind, lines.next(), cx)).collect();
        let yours = ui::settings_group("Your keybinds", cx).children(self.input_monitoring_notice(cx)).when(!rows.is_empty(), |g| g.child(ui::Card::new(cx).children(rows)));
        vec![Section::new(header), Section::keyed("Your keybinds", yours), Section::keyed("Default keybinds", self.default_keybinds(cx))]
    }

    // macOS keeps keys from Waffle until it's allowed Input Monitoring; the hook starts once it is.
    pub(super) fn input_monitoring_notice(&self, cx: &mut Context<Self>) -> Option<Div> {
        if !self.keys_unheard() {
            return None;
        }
        let open = ui::Button::new("keys-system").variant(ui::ButtonVariant::Secondary).size(ui::ButtonSize::Sm).child("Open System Settings").on_click(|_, _, cx| cx.open_url(MAC_INPUT_PRIVACY));
        Some(ui::Alert::with_actions(
            ui::Notice::Warning,
            Glyph::Keyboard,
            "Keybinds need Input Monitoring",
            "Turn on Waffle in System Settings, under Privacy & Security, Input Monitoring. Already on? An update can leave it stale: remove Waffle with \u{2212} and add it again. Keybinds start working as soon as you do.",
            div().pt_2().child(ui::row().items_stretch().child(open)),
            cx,
        ))
    }

    pub(super) fn keys_unheard(&self) -> bool {
        cfg!(target_os = "macos") && self.hotkeys.as_ref().is_some_and(|h| !h.listening)
    }

    // Action picker, recorder, and a remove button that's always there.
    pub(super) fn keybind_row(&self, index: usize, keybind: &Keybind, line: bool, cx: &mut Context<Self>) -> AnyElement {
        let unknown = keybind.keys.as_ref().is_some_and(|k| self.keybind_errors.contains(k));
        let actions: Vec<(String, String)> = Action::ALL.iter().map(|a| (format!("{a:?}"), sentence(a.label()))).collect();
        let action = self.select_field(Select::Action(index), None, &actions, &format!("{:?}", keybind.action), sentence(keybind.action.label()).into(), 36., cx);
        let (danger, hover, press) = (palette::danger_ink(cx), palette::hover(cx), palette::press(cx));
        // The design's .iconbtn.rm: ui::icon_button, but hovering to the danger ink.
        // lint-allow(custom-button): ui::Button has no hover to the danger ink without the danger fill, which this quiet remove must not take
        let remove = ui::row()
            .id(SharedString::from(format!("keybind-delete-{index}")))
            .size_8()
            .flex_none()
            .squircle(ui::RADIUS_SM)
            .justify_center()
            .cursor_pointer()
            .text_color(palette::muted(cx))
            .hover(move |s| s.bg(hover).text_color(danger))
            .active(move |s| s.bg(press))
            .child(Icon::new(Glyph::Trash).size(ui::ICON_XL))
            .tooltip(ui::Tooltip::text("Remove keybind"))
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(s) = &mut this.settings {
                    s.recording = None;
                }
                this.update_prefs(cx, |p| {
                    if index < p.keybinds.len() {
                        p.keybinds.remove(index);
                    }
                });
            }));
        ui::Item::setting(SharedString::from(format!("keybind-{index}")), line, cx)
            .flex_wrap()
            .gap_3()
            .child(div().flex_grow(1.).flex_basis(Pixels::ZERO).min_w_0().child(action))
            .child(self.keybind_recorder(index, cx).flex_grow(1.25).flex_basis(Pixels::ZERO))
            .child(remove)
            .children(unknown.then(|| error_line("Waffle can't listen for this shortcut. Record it again.", cx).w_full()))
            .into_any_element()
    }

    // The binding as keycaps, and a button that starts, stops or edits recording. Recording rings the
    // field in the accent, pulsing.
    pub(super) fn keybind_recorder(&self, index: usize, cx: &mut Context<Self>) -> Div {
        let recording = self.settings.as_ref().and_then(|s| s.recording) == Some(index);
        let keys = self.prefs.keybinds.get(index).and_then(|k| k.keys.clone());
        let unknown = keys.as_ref().is_some_and(|k| self.keybind_errors.contains(k));
        let value = match (&keys, recording) {
            (_, true) => ui::row()
                .gap_1p5()
                .text_color(palette::muted(cx))
                .child(div().text_color(palette::accent_ink(cx)).child(Icon::new(Glyph::Record).size(ui::ICON_SM)))
                .child(if self.keys_unheard() { "Allow Input Monitoring first" } else { "Press keys\u{2026}" })
                .into_any_element(),
            (Some(keys), false) => key_caps(keys).into_any_element(),
            (None, false) => div().text_color(palette::muted(cx)).font_weight(FontWeight::NORMAL).child("No keybind set").into_any_element(),
        };
        let border = match (recording, unknown) {
            (true, _) => palette::accent(),
            (false, true) => palette::danger_ink(cx),
            (false, false) => palette::line_strong(cx),
        };
        let label = if recording {
            "Stop recording"
        } else if keys.is_some() {
            "Edit keybind"
        } else {
            "Record keybind"
        };
        ui::row()
            .relative()
            .h_10()
            .pl(px(RECORDER_INSET))
            .pr_1()
            .squircle(ui::RADIUS_MD)
            .bg(palette::surface(Level::Page, cx))
            .border_1()
            .border_color(border)
            .gap_2()
            .children(recording.then(|| pulse_ring(SharedString::from(format!("record-ring-{index}")), cx)))
            .child(div().flex_1().min_w_0().p_px().overflow_hidden().whitespace_nowrap().font_weight(FontWeight::NORMAL).text_color(palette::strong(cx)).child(value))
            .child(ui::Button::new(SharedString::from(format!("record-{index}"))).variant(ui::ButtonVariant::Secondary).size(ui::ButtonSize::Sm).child(label).on_click(cx.listener(move |this, _, window, cx| {
                if let Some(s) = &mut this.settings {
                    s.recording = if s.recording == Some(index) { None } else { Some(index) };
                }
                this.focus.focus(window, cx);
                // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
                cx.notify();
            })))
    }

    // Discord's "Show advanced voice settings" rows, in its order; the first two only under Custom.
    pub(super) fn advanced_voice_rows(&self, custom: bool, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let prefs = &self.prefs;
        let vad_enabled = !prefs.push_to_talk && prefs.sensitivity_auto;
        let mut lines = Hairlines(true);
        let mut rows: Vec<AnyElement> = Vec::new();
        if custom {
            rows.push(
                self.switch_row(SwitchRow { id: "set-agc", title: "Automatic gain control", detail: Some("Adjusts your mic volume to keep you clear and steady"), on: prefs.auto_gain, enabled: true, field: |p| &mut p.auto_gain }, lines.next(), window, cx)
                    .into_any_element(),
            );
            let detail = if vad_enabled { "Turn this off if automatic sensitivity misses your voice" } else { WHY_VAD };
            rows.push(
                self.switch_row(
                    SwitchRow { id: "set-advanced-vad", title: "Advanced voice activity", detail: Some(detail), on: prefs.advanced_voice_activity, enabled: vad_enabled, field: |p| &mut p.advanced_voice_activity },
                    lines.next(),
                    window,
                    cx,
                )
                .into_any_element(),
            );
        }
        rows.push(
            self.switch_row(
                SwitchRow { id: "set-silence-warning", title: "No audio detected warning", detail: Some("Warns you in a call when Waffle hears nothing from your mic"), on: prefs.silence_warning, enabled: true, field: |p| &mut p.silence_warning },
                lines.next(),
                window,
                cx,
            )
            .into_any_element(),
        );
        if voice::ATTENUATION_SUPPORTED {
            rows.push(
                stack_row("attenuation", lines.next(), cx)
                    .child(ui::settings_text("Global attenuation", Some("Lowers other apps by this much while someone speaks. 0% turns it off."), cx))
                    .child(self.volume("Attenuation strength", &format!("{}%", prefs.attenuation), Slider::Attenuation, cx))
                    .into_any_element(),
            );
            rows.push(
                self.switch_row(SwitchRow { id: "set-attenuate-self", title: "When I speak", detail: Some("Lower other apps while you talk"), on: prefs.attenuate_self, enabled: true, field: |p| &mut p.attenuate_self }, lines.next(), window, cx)
                    .into_any_element(),
            );
            rows.push(
                self.switch_row(
                    SwitchRow { id: "set-attenuate-others", title: "When others speak", detail: Some("Lower other apps while anyone else talks"), on: prefs.attenuate_others, enabled: true, field: |p| &mut p.attenuate_others },
                    lines.next(),
                    window,
                    cx,
                )
                .into_any_element(),
            );
        }
        rows.push(
            self.switch_row(
                SwitchRow {
                    id: "set-qos",
                    title: "Quality of service",
                    detail: Some("Asks your router to send Waffle's voice first. Turn it off if calls drop; some routers handle it badly."),
                    on: prefs.qos,
                    enabled: true,
                    field: |p| &mut p.qos,
                },
                lines.next(),
                window,
                cx,
            )
            .into_any_element(),
        );
        ui::col().children(rows)
    }

    // Three options fit in a row, so noise suppression is a segmented control. Krisp is licensed to
    // Discord, so Advanced is DeepFilterNet, an open neural suppressor, in its place.
    pub(super) fn noise_row(&self, line: bool, _window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let selected = match (self.prefs.noise_suppression, self.prefs.noise_mode) {
            (false, _) => 0,
            (true, NoiseMode::Standard) => 1,
            (true, NoiseMode::Advanced) => 2,
        };
        let degraded = selected == 2 && self.call.as_ref().is_some_and(|c| c.session.noise_suppression_degraded());
        let detail = if degraded {
            ui::row()
                .gap_1()
                .text_size(ui::TEXT_XS)
                .font_weight(FontWeight::NORMAL)
                .text_color(palette::warning_ink(cx))
                .child(Icon::new(Glyph::WarningBold).size(ui::ICON_SM))
                .child("Your computer couldn't run DeepFilterNet in real time, so this call uses Standard.")
        } else {
            div().text_size(ui::TEXT_XS).text_color(palette::muted(cx)).child("Reduces background noise from your mic. Advanced is powered by DeepFilterNet.")
        };
        let this = cx.entity().downgrade();
        let control = ui::Tabs::new("set-noise").labels(&["None", "Standard", "Advanced"]).value(selected).on_value_change(move |i, _, cx| {
                this.update(cx, |this, cx| {
                    this.update_prefs(cx, |p| match i {
                        0 => p.noise_suppression = false,
                        1 => (p.noise_suppression, p.noise_mode) = (true, NoiseMode::Standard),
                        _ => (p.noise_suppression, p.noise_mode) = (true, NoiseMode::Advanced),
                    })
                })
                .ok();
            });
        ui::Item::setting("noise", line, cx).child(ui::settings_text("Noise suppression", None::<&str>, cx).child(detail)).child(control.flex_none())
    }

    // A sound cue: its preview button beside its switch.
    pub(super) fn cue_row(&self, id: &'static str, title: &'static str, cue: Cue, line: bool, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let on = !self.prefs.silenced_cues.contains(&cue);
        let play = ui::Button::new(SharedString::from(format!("preview-{id}"))).variant(ui::ButtonVariant::Secondary).size(ui::ButtonSize::Icon).icon(Glyph::Play).tooltip_text(format!("Preview {title}"))
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                sounds::preview(cue);
            });
        let switch = ui::Switch::new(id).checked(on).on_click(cx.listener(move |this, _, _, cx| {
            this.update_prefs(cx, |p| match p.silenced_cues.iter().position(|c| *c == cue) {
                Some(i) => {
                    p.silenced_cues.remove(i);
                }
                None => p.silenced_cues.push(cue),
            })
        }));
        // GAP: the play button turns into the mini wave while its sound plays; sounds.rs doesn't report
        // playback yet.
        ui::Item::setting(SharedString::from(format!("cue-{id}")), line, cx)
            .min_h(px(CUE_ROW_HEIGHT))
            .child(ui::settings_text(title, None::<&str>, cx))
            .child(ui::row().gap_2().flex_none().child(play).child(switch))
            .into_any_element()
    }

    // A switch row toggles from anywhere on it. A disabled one dims its switch and says why.
    pub(super) fn switch_row(&self, spec: SwitchRow, line: bool, _window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let SwitchRow { id, title, detail, on, enabled, field } = spec;
        let text = if enabled {
            ui::settings_text(title, detail, cx)
        } else {
            ui::col().min_h_0()
                .flex_1()
                .gap_0p5()
                .child(ui::line(title).font_weight(FontWeight::NORMAL).text_color(palette::muted(cx)))
                .child(ui::row().gap_1().text_size(ui::TEXT_XS).text_color(palette::muted(cx)).child(Icon::new(Glyph::Info).size(ui::ICON_SM)).children(detail))
        };
        let switch = ui::Switch::new(id).checked(on);
        let row = ui::Item::setting(SharedString::from(format!("row-{id}")), line, cx).child(text);
        if !enabled {
            return row.child(div().opacity(0.45).child(switch));
        }
        ui::settings_choice(row, cx).child(switch).on_click(cx.listener(move |this, _, _, cx| {
            this.update_prefs(cx, |p| {
                let value = field(p);
                *value = !*value;
            })
        }))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn device_picker(&self, select: Select, glyph: Glyph, devices: &[(String, String)], chosen: Option<&str>, height: f32, cx: &mut Context<Self>) -> AnyElement {
        let mut options = vec![(String::new(), "Default".to_owned())];
        options.extend(devices.iter().cloned());
        let value = chosen.and_then(|id| devices.iter().find(|(d, _)| d == id)).map_or("Default".to_owned(), |(_, name)| name.clone());
        self.select_field(select, Some(glyph), &options, chosen.unwrap_or(""), value.into(), height, cx)
    }

    // A dropdown trigger whose list rises from it; the chosen option carries a check.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn select_field(&self, select: Select, glyph: Option<Glyph>, options: &[(String, String)], chosen: &str, value: SharedString, height: f32, cx: &mut Context<Self>) -> AnyElement {
        let open = self.settings.as_ref().and_then(|s| s.select) == Some(select);
        let field = ui::Select::new(SharedString::from(format!("select-{select:?}")), glyph, value, open, cx).h(px(height)).on_click(cx.listener(move |this, _, _, cx| {
            if let Some(s) = &mut this.settings {
                s.select = if s.select == Some(select) { None } else { Some(select) };
            }
            // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
            cx.notify();
        }));
        let list = open.then(|| {
            let (this, close) = (cx.entity(), cx.entity());
            let dismiss = move |_: &mut Window, cx: &mut App| {
                close.update(cx, |this, cx| {
                    if let Some(s) = &mut this.settings {
                        s.select = None;
                    }
                    // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
                    cx.notify();
                })
            };
            ui::Select::options(SharedString::from(format!("options-{select:?}")), options, chosen, height, move |id, window, cx| this.update(cx, |this, cx| this.choose(select, &id, window, cx)), dismiss, cx)
        });
        div().relative().w_full().child(field).children(list).into_any_element()
    }

    pub(super) fn choose(&mut self, select: Select, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let device = (!id.is_empty()).then(|| id.to_owned());
        if let Some(s) = &mut self.settings {
            s.select = None;
        }
        if select == Select::JoinActivity {
            return self.set_join_activity_default(id.parse().unwrap_or(0), cx);
        }
        if matches!(select, Select::UiFont | Select::MonoFont) {
            self.update_prefs(cx, |p| match select {
                Select::UiFont => p.ui_font = id.to_owned(),
                _ => p.mono_font = id.to_owned(),
            });
            palette::sync(window, cx);
            return window.refresh();
        }
        self.update_prefs(cx, |p| match select {
            Select::Microphone => p.input_device = device,
            Select::Speaker => p.output_device = device,
            Select::Camera => p.camera = device,
            Select::Action(index) => {
                let action = Action::ALL.into_iter().find(|a| format!("{a:?}") == id).unwrap_or(Action::Unassigned);
                if let Some(bind) = p.keybinds.get_mut(index) {
                    bind.action = action;
                }
            }
            Select::UiFont | Select::MonoFont | Select::JoinActivity => {}
        });
    }

    pub(super) fn level_range(&self, slider: Slider, level: Option<f32>, cx: &mut Context<Self>) -> AnyElement {
        let fill = match slider {
            Slider::Sensitivity => Fill::Threshold(level),
            _ => Fill::Accent,
        };
        self.range(format!("{slider:?}").into(), slider.fraction(&self.prefs), fill, move |this, f, cx| this.update_prefs(cx, |p| slider.set(p, f)), cx)
    }

    // Waffle's slider: the done part, a gap, a tall narrow thumb, a gap, the rest; click or drag anywhere
    // on it. The sensitivity slider draws the live level under its handle instead.
    pub(crate) fn range(
        &self,
        key: SharedString,
        fraction: f32,
        fill: Fill,
        on_set: impl Fn(&mut Waffle, f32, &mut Context<Waffle>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bounds = self.slider_bounds.clone();
        let record = bounds.clone();
        let track = match fill {
            Fill::Accent => ui::Slider::track(fraction).into_any_element(),
            Fill::Threshold(level) => sensitivity_track(Some(fraction), level.unwrap_or(crate::SILENCE_DB), cx).into_any_element(),
        };
        let (down_key, drag_key, move_key, record_key) = (key.clone(), key.clone(), key.clone(), key.clone());
        let set = move |this: &mut Waffle, key: &SharedString, x: Pixels, cx: &mut Context<Waffle>| {
            let Some(b) = bounds.borrow().get(key).copied() else { return };
            on_set(this, (x - b.left()) / b.size.width, cx);
        };
        let set_drag = set.clone();
        div()
            .id(SharedString::from(format!("slider-{key}")))
            .relative()
            .h_6()
            .w_full()
            .cursor_pointer()
            .child(canvas(move |b, _, _| { record.borrow_mut().insert(record_key.clone(), b); }, |_, _, _, _| {}).absolute().size_full())
            .child(track)
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| set(this, &down_key, e.position.x, cx)))
            .on_drag(SliderDrag(drag_key), |drag, _, _, cx| cx.new(|_| drag.clone()))
            .on_drag_move::<SliderDrag>(cx.listener(move |this, e: &gpui_kit::DragMoveEvent<SliderDrag>, _, cx| {
                if e.drag(cx).0 == move_key {
                    set_drag(this, &move_key, e.event.position.x, cx);
                }
            }))
            .into_any_element()
    }
}
