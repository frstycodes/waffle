use crate::ui;
use super::*;

// The device menu's width and the height of a choice row in it.
const MENU_WIDTH: f32 = 300.;
const CHOICE_ROW_HEIGHT: f32 = 34.;

impl Waffle {
    // .controls: mic and camera as split buttons with their device menus (a right-click opens them too); muted, the
    // mic and its caret take the red. `carets` is false in a narrow window.
    pub(crate) fn mic_control(&self, carets: bool, cx: &mut Context<Self>) -> ui::ButtonGroup {
        let muted = self.muted || self.deafened;
        let state = if muted { Control::Off } else { Control::Rest };
        let tip = match (self.deafened, muted) {
            (true, _) => "Undeafen",
            (false, true) => "Unmute",
            (false, false) => "Mute",
        };
        let open = |this: &mut Self, e: &MouseDownEvent, cx: &mut Context<Self>| this.open_stage_device_menu(DeviceKind::Input, e.position, cx);
        split_control(
            control("call-mic", if muted { Glyph::MicOffFill } else { Glyph::MicFill }, tip, state, true)
                .on_click(cx.listener(|this, _, _, cx| if this.deafened { this.toggle_deafen(cx) } else { this.toggle_mute(cx) }))
                .on_mouse_down(MouseButton::Right, cx.listener(move |this, e, _, cx| open(this, e, cx))),
            carets.then(|| {
                control("call-mic-devices", Glyph::ChevronUpBold, "More Microphone Options", if self.stage_menu_open(DeviceKind::Input) { Control::Open } else { state }, true)
                    .on_mouse_down(MouseButton::Left, cx.listener(move |this, e, _, cx| open(this, e, cx)))
            }),
        )
    }

    pub(crate) fn camera_control(&self, carets: bool, cx: &mut Context<Self>) -> ui::ButtonGroup {
        let call = self.call.as_ref().expect("in a call");
        let connected = call.stage == Stage::Connected;
        let camera_state = if call.camera_on { Control::On } else { Control::Rest };
        let open = |this: &mut Self, e: &MouseDownEvent, cx: &mut Context<Self>| this.open_stage_device_menu(DeviceKind::Camera, e.position, cx);
        split_control(
            control("call-camera", if call.camera_on { Glyph::VideoFill } else { Glyph::VideoOffFill }, if call.camera_on { "Turn Off Camera" } else { "Turn On Camera" }, camera_state, connected)
                .when(connected, |b| b.on_click(cx.listener(|this, _, _, cx| this.toggle_camera(cx))))
                .on_mouse_down(MouseButton::Right, cx.listener(move |this, e, _, cx| open(this, e, cx))),
            carets.then(|| {
                control("call-camera-devices", Glyph::ChevronUpBold, "More Options", if self.stage_menu_open(DeviceKind::Camera) { Control::Open } else { Control::Rest }, true)
                    .on_mouse_down(MouseButton::Left, cx.listener(move |this, e, _, cx| open(this, e, cx)))
            }),
        )
    }

    // Share Your Screen, or Stop Streaming while live; right-click holds the stream's options, as Discord's.
    pub(crate) fn share_control(&self, cx: &mut Context<Self>) -> ui::Pressable {
        let call = self.call.as_ref().expect("in a call");
        let connected = call.stage == Stage::Connected;
        let sharing = call.sharing.is_some();
        control("call-share", Glyph::ShareScreenFill, if sharing { "Stop Streaming" } else { "Share Your Screen" }, if sharing { Control::On } else { Control::Rest }, connected)
            .when(connected, |b| b.on_click(cx.listener(|this, _, _, cx| this.toggle_screen_share(cx))))
            .when(sharing, |b| b.on_mouse_down(MouseButton::Right, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_stream_menu(e.position, cx))))
    }

    // A stream tile's Options (and its right-click): watch or stop, its volume, focus, then the streamer.
    pub(super) fn open_stream_tile_menu(&mut self, streamer_id: String, at: Point<Pixels>, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.open_live_menu(
            at,
            move |this| {
                let Some(call) = &this.call else { return Vec::new() };
                let me = this.me.as_ref().is_some_and(|m| m.id == streamer_id);
                let watching = call.watched(&streamer_id).is_some();
                let mut entries = Vec::new();
                let id = streamer_id.clone();
                entries.push(match (me, watching) {
                    (true, _) => item("Stop Streaming", |this, _, cx| this.toggle_screen_share(cx)).entry(),
                    (false, true) => item("Stop Watching", move |this, _, cx| this.unwatch_stream(&id, cx)).entry(),
                    (false, false) => item("Watch Stream", move |this, _, cx| this.watch_stream(id.clone(), cx)).entry(),
                });
                if me {
                    let hidden = this.self_stream_hidden();
                    entries.push(item(if hidden { "Show Preview" } else { "Hide Preview" }, |this, _, cx| this.toggle_self_stream_hidden(cx)).icon(Glyph::EyeSlash).entry());
                }
                entries.push(Entry::Separator);
                if watching && !me {
                    let (id, volume) = (streamer_id.clone(), this.stream_volumes.get(&streamer_id).copied().unwrap_or(1.));
                    entries.push(crate::menu::slider("Stream Volume", format!("stream-volume-{streamer_id}"), volume / 2., move |this, f, cx| this.set_stream_volume(&id, f * 2., cx)));
                }
                let key = format!("stream:{streamer_id}");
                let focused = this.call_focus.as_deref() == Some(key.as_str());
                entries.push(item(if focused { "Unfocus This Person" } else { "Focus This Person" }, move |this, _, cx| this.toggle_focus(key.clone(), cx)).entry());
                entries.push(Entry::Separator);
                let id = streamer_id.clone();
                entries.push(item("Profile", move |this, _, cx| this.open_voice_full_profile(&id, cx)).icon(Glyph::UserCircle).entry());
                if !me {
                    let id = streamer_id.clone();
                    entries.push(item("Message", move |this, window, cx| this.message_user(id.clone(), window, cx)).icon(Glyph::ChatRound).entry());
                }
                entries
            },
            cx,
        );
    }

    pub(super) fn stage_menu_open(&self, kind: DeviceKind) -> bool {
        self.device_menu.as_ref().is_some_and(|m| m.on_stage && m.kind == kind)
    }

    pub(crate) fn open_device_menu(&mut self, kind: DeviceKind, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.show_device_menu(kind, at, false, cx);
    }

    pub(super) fn open_stage_device_menu(&mut self, kind: DeviceKind, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.show_device_menu(kind, at, true, cx);
    }

    pub(super) fn show_device_menu(&mut self, kind: DeviceKind, at: Point<Pixels>, on_stage: bool, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let (inputs, outputs, cameras) = match kind {
            DeviceKind::Input => (voice::input_devices(), voice::output_devices(), Vec::new()),
            DeviceKind::Output => (Vec::new(), voice::output_devices(), Vec::new()),
            DeviceKind::Camera => (Vec::new(), Vec::new(), voice::cameras()),
        };
        self.device_menu = Some(DeviceMenu { kind, at, on_stage, inputs, outputs, cameras });
        cx.notify(); // lint-allow(global-notify): the device menu is an overlay, which only follows the app
    }

    pub(super) fn choose_device(&mut self, kind: DeviceKind, id: Option<String>, cx: &mut Context<Self>) {
        self.update_prefs(cx, |p| match kind {
            DeviceKind::Input => p.input_device = id,
            DeviceKind::Output => p.output_device = id,
            DeviceKind::Camera => p.camera = id,
        });
        if kind == DeviceKind::Camera
            && let Some(call) = &self.call
            && call.camera_on
            && let Err(error) = call.session.set_camera(self.prefs.camera.clone())
        {
            self.voice_error = Some(error.into());
        }
    }

    // .devmenu, opened above its chevron: the devices to pick from, volume (with the live level for
    // the mic), noise suppression, and the way to the full settings. Over the stage it stays dark.
    pub(super) fn device_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.device_menu.as_ref()?;
        let ink = Inks::of(menu.on_stage, cx);
        let kind = menu.kind;
        let mut body: Vec<AnyElement> = Vec::new();
        let group = |body: &mut Vec<AnyElement>, title: &'static str, which: DeviceKind, devices: &[voice::Device], chosen: Option<String>, cx: &mut Context<Self>| {
            body.push(menu_label(title, &ink).into_any_element());
            let mut options: Vec<(Option<String>, String)> = Vec::new();
            if which != DeviceKind::Camera {
                options.push((None, "Default".into()));
            }
            options.extend(devices.iter().map(|d| (Some(d.id.clone()), d.name.clone())));
            body.extend(options.into_iter().enumerate().map(|(i, (id, name))| {
                let on = id == chosen;
                choice_row(SharedString::from(format!("device-{title}-{i}")), &ink, cx)
                    .child(ui::RadioGroupItem::mark(on).scoped(18., ink.line_strong))
                    .child(ui::line(name).flex_1().font_weight(FontWeight::NORMAL).text_color(ink.strong))
                    .on_click(cx.listener(move |this, _, _, cx| this.choose_device(which, id.clone(), cx)))
                    .into_any_element()
            }));
        };
        match kind {
            DeviceKind::Input => {
                group(&mut body, "Input device", DeviceKind::Input, &menu.inputs, self.prefs.input_device.clone(), cx);
                group(&mut body, "Output device", DeviceKind::Output, &menu.outputs, self.prefs.output_device.clone(), cx);
            }
            DeviceKind::Output => group(&mut body, "Output device", DeviceKind::Output, &menu.outputs, self.prefs.output_device.clone(), cx),
            DeviceKind::Camera => group(&mut body, "Camera", DeviceKind::Camera, &menu.cameras, self.prefs.camera.clone(), cx),
        }
        body.push(menu_rule(&ink).into_any_element());
        if kind != DeviceKind::Camera {
            let (label, value, key) = match kind {
                DeviceKind::Output => ("Output volume", self.prefs.output_volume, "menu-output-volume"),
                _ => ("Input volume", self.prefs.input_volume, "menu-input-volume"),
            };
            let level = (kind == DeviceKind::Input).then(|| self.call.as_ref().map(|c| c.level.0)).flatten();
            body.push(
                self.call_menu_slider(SharedString::from(key), label, value / 2., &ink, move |this, f, cx| {
                    this.update_prefs(cx, |p| match kind {
                        DeviceKind::Output => p.output_volume = f.clamp(0., 1.) * 2.,
                        _ => p.input_volume = f.clamp(0., 1.) * 2.,
                    })
                }, cx)
                .children(level.map(|db| ui::meter_with((db + 100.) / 100., METER_BARS, ink.lit, ink.line_strong).h_3p5().gap_0p5()))
                .into_any_element(),
            );
        }
        if kind == DeviceKind::Input {
            let noise = self.prefs.noise_suppression;
            body.push(
                choice_row("device-noise", &ink, cx)
                    .child(div().flex_1().min_w_0().child(div().font_weight(FontWeight::NORMAL).text_color(ink.strong).child("Noise suppression")).child(div().text_size(ui::TEXT_XS).text_color(ink.muted).child("Filters fans and keys on this device")))
                    .child(ui::Switch::new("device-noise-switch").checked(noise))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_noise_suppression(cx)))
                    .into_any_element(),
            );
            let deafened = self.deafened;
            body.push(
                choice_row("device-deafen", &ink, cx)
                    .child(div().flex_1().min_w_0().font_weight(FontWeight::NORMAL).text_color(ink.strong).child("Deafen"))
                    .child(ui::Switch::new("device-deafen-switch").checked(deafened))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_deafen(cx)))
                    .into_any_element(),
            );
            body.push(menu_rule(&ink).into_any_element());
        }
        let settings_label = if kind == DeviceKind::Camera { "Video settings" } else { "Voice settings" };
        body.push(
            choice_row("device-settings", &ink, cx)
                .child(Icon::new(Glyph::Settings).size(ui::ICON_LG).text_color(ink.muted))
                .child(div().flex_1().font_weight(FontWeight::NORMAL).text_color(ink.text).child(settings_label))
                .child(Icon::new(Glyph::ChevronRight).size(ui::ICON_SM).text_color(ink.faint))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.device_menu = None;
                    this.open_settings(Page::VoiceVideo, window, cx);
                }))
                .into_any_element(),
        );
        let viewport = window.viewport_size();
        let card = ui::PopoverContent::scoped(ink.overlay, ink.line_strong, cx)
            .id("device-menu")
            .occlude()
            .absolute()
            .left(px((f32::from(menu.at.x) - 150.).clamp(8., f32::from(viewport.width) - 308.)))
            .bottom(px((f32::from(viewport.height) - f32::from(menu.at.y) + 12.).max(8.)))
            .w(px(MENU_WIDTH))
            .text_size(ui::TEXT_SM)
            .text_color(ink.text)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(body);
        // It opens above its chevron, so it rises from its bottom edge's centre.
        let card = ui::rise(card.transform_origin(gpui_kit::point(gpui_kit::relative(0.5), gpui_kit::relative(1.))), "device-menu", Duration::ZERO);
        Some(call_backdrop("device-backdrop", cx).child(card).into_any_element())
    }

    // .mslider: the label and its percentage over the slider; click or drag anywhere on it.
    pub(super) fn call_menu_slider(&self, key: SharedString, label: &'static str, fraction: f32, ink: &Inks, on_set: impl Fn(&mut Waffle, f32, &mut Context<Waffle>) + Clone + 'static, cx: &mut Context<Self>) -> Div {
        let bounds = self.slider_bounds.clone();
        let record = bounds.clone();
        let (down_key, move_key, record_key) = (key.clone(), key.clone(), key.clone());
        let set = move |this: &mut Waffle, key: &SharedString, x: Pixels, cx: &mut Context<Waffle>| {
            let Some(b) = bounds.borrow().get(key).copied() else { return };
            on_set(this, ((x - b.left()) / b.size.width).clamp(0., 1.), cx);
        };
        let set_drag = set.clone();
        let slider = div()
            .id(SharedString::from(format!("menu-slider-{key}")))
            .relative()
            .w_full()
            .cursor_pointer()
            .child(gpui_kit::canvas(move |b, _, _| { record.borrow_mut().insert(record_key.clone(), b); }, |_, _, _, _| {}).absolute().size_full())
            .child(ui::Slider::track(fraction))
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, e: &MouseDownEvent, _, cx| set(this, &down_key, e.position.x, cx)))
            .on_drag(MenuSliderDrag(key.clone()), |drag, _, _, cx| cx.new(|_| drag.clone()))
            .on_drag_move::<MenuSliderDrag>(cx.listener(move |this, e: &gpui_kit::DragMoveEvent<MenuSliderDrag>, _, cx| {
                if e.drag(cx).0 == move_key {
                    set_drag(this, &move_key, e.event.position.x, cx);
                }
            }));
        ui::col()
            .px_2p5()
            .pt_1p5()
            .pb_2p5()
            .gap_2()
            .child(ui::row().items_stretch().justify_between().font_weight(FontWeight::NORMAL).text_color(ink.text).child(label).child(div().text_color(ink.muted).font_features(tabular()).child(format!("{:.0}%", fraction * 100.))))
            .child(slider)
    }

    // Right-click on anyone in voice opens Discord's server user menu with its voice items.
    pub(crate) fn voice_user_menu(&mut self, user_id: &str, at: Point<Pixels>, cx: &mut Context<Self>) {
        let (guild_id, user) = self.voice_user(user_id);
        // Anyone in voice, server or DM call, gets Discord's voice items (volume, local mute).
        let surface = crate::user_menu::Surface::ServerVoice;
        self.open_user_menu(user, guild_id, surface, at, cx);
    }

    // Clicking someone in voice opens their profile, as the member list does.
    pub(crate) fn open_voice_profile(&mut self, user_id: &str, at: Point<Pixels>, cx: &mut Context<Self>) {
        let (_, user) = self.voice_user(user_id);
        self.open_profile(user, at, cx);
    }

    pub(crate) fn open_voice_full_profile(&mut self, user_id: &str, cx: &mut Context<Self>) {
        let (_, user) = self.voice_user(user_id);
        self.open_full_profile(user, crate::profile::FullTab::Activity, cx);
    }

    // Who someone in voice is, and the server they're in voice in (else the call's).
    pub(super) fn voice_user(&self, user_id: &str) -> (Option<String>, User) {
        let found = self.guilds.iter().find_map(|g| g.voice_states.iter().find(|v| v.user_id == user_id).map(|v| (g.id.clone(), v.user.clone())));
        let guild_id = found.as_ref().map(|(g, _)| g.clone()).or_else(|| self.call.as_ref().and_then(|c| c.guild_id.clone()));
        let user = found.and_then(|(_, u)| u).unwrap_or_else(|| User {
            id: user_id.to_owned(),
            name: self.user_name(user_id),
            avatar: discord::avatar_url(user_id, None),
            ..Default::default()
        });
        (guild_id, user)
    }

    pub(crate) fn set_user_volume(&mut self, user_id: &str, volume: f32, cx: &mut Context<Self>) {
        if let Some(call) = &self.call {
            call.session.set_user_volume(user_id, volume);
        }
        let user_id = user_id.to_owned();
        self.update_prefs(cx, |p| {
            p.user_volumes.insert(user_id, volume);
        });
    }

    pub(crate) fn self_stream_hidden(&self) -> bool {
        self.call.as_ref().is_some_and(|c| self.prefs.hidden_self_streams.contains(&c.channel.id))
    }

    pub(crate) fn toggle_self_stream_hidden(&mut self, cx: &mut Context<Self>) {
        let Some(channel) = self.call.as_ref().map(|c| c.channel.id.clone()) else { return };
        let hide = !self.self_stream_hidden();
        self.update_prefs(cx, |p| match hide {
            true => p.hidden_self_streams.push(channel),
            false => p.hidden_self_streams.retain(|c| *c != channel),
        });
        cx.notify(); // lint-allow(global-notify): hiding your own stream changes the stage tiles (main) and the nav voice rows
    }

    // The issues a tile carries as a warning chip, as Discord gates them: what we send (2004 and 2005
    // only while someone watches, 2013 only with someone to see it), and a camera that never arrived.
    pub(super) fn tile_issues(&self, user_id: &str, is_stream: bool, me: bool) -> Vec<u16> {
        let Some(call) = &self.call else { return Vec::new() };
        let held = |who: &str, code| call.issues.contains(&(who.to_owned(), code));
        let camera = format!("camera:{user_id}");
        use voice::issues::*;
        match (is_stream, me) {
            (true, true) => {
                let watched = !self.viewers(user_id).is_empty();
                [SOUNDSHARE_FAILED, STREAM_SEND_HIGH_PACKET_LOSS, STREAM_SEND_LOW_FPS, STREAM_SEND_NETWORK_QUALITY, SENDER_READY_TIMEOUT]
                    .into_iter()
                    .filter(|&code| held(user_id, code) && (watched || !matches!(code, STREAM_SEND_HIGH_PACKET_LOSS | STREAM_SEND_LOW_FPS)))
                    .collect()
            }
            (false, true) => {
                let seen = self.participants().len() > 1;
                [SENDER_READY_TIMEOUT, CAMERA_SEND_LOW_FPS].into_iter().filter(|&code| held(&camera, code) && (seen || code != CAMERA_SEND_LOW_FPS)).collect()
            }
            (false, false) => [RECEIVER_READY_TIMEOUT].into_iter().filter(|&code| held(&camera, code)).collect(),
            (true, false) => Vec::new(),
        }
    }

    // Discord's ready-timeout Retry: watch the stream again from scratch.
    pub(super) fn retry_watch(&mut self, streamer_id: &str, cx: &mut Context<Self>) {
        self.unwatch_stream(streamer_id, cx);
        self.watch_stream(streamer_id.to_owned(), cx);
    }

    pub(crate) fn toggle_local_mute(&mut self, user_id: &str, cx: &mut Context<Self>) {
        let muted = !self.prefs.local_mutes.iter().any(|u| u == user_id);
        if let Some(call) = &self.call {
            call.session.set_user_muted(user_id, muted);
        }
        let user_id = user_id.to_owned();
        self.update_prefs(cx, |p| match muted {
            true => p.local_mutes.push(user_id),
            false => p.local_mutes.retain(|u| *u != user_id),
        });
    }

    // The stream menu, opened from the share button while we're live.
    pub(crate) fn open_stream_menu(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.open_live_menu(at, |this| this.stream_menu(), cx);
    }

    pub(super) fn stream_menu(&self) -> Vec<Entry> {
        let (res, fps, audio) = (self.prefs.stream_resolution, self.prefs.stream_fps, self.prefs.stream_audio);
        let mut quality: Vec<Entry> = [(720, "720p"), (1080, "1080p"), (1440, "1440p"), (0, "Source")]
            .into_iter()
            .map(|(value, label)| item(label, move |this, _, cx| this.set_stream_quality(Some(value), None, cx)).checked(res == value).entry())
            .collect();
        quality.push(Entry::Separator);
        quality.extend([15, 30, 60].into_iter().map(|value| item(format!("{value} FPS"), move |this, _, cx| this.set_stream_quality(None, Some(value), cx)).checked(fps == value).entry()));
        vec![
            item("Change Stream", |this, _, cx| this.open_share_picker(cx)).hint("Pick another window or screen").icon(Glyph::ShareScreen).entry(),
            submenu("Stream Quality", quality).hint(quality_label(res, fps)).entry(),
            item("Share Stream Audio", |this, _, cx| {
                this.update_prefs(cx, |p| p.stream_audio = !p.stream_audio);
                this.restart_share(cx);
            })
            .checked(audio)
            .entry(),
            Entry::Separator,
            item("Stop Streaming", |this, _, cx| this.toggle_screen_share(cx)).icon(Glyph::ScreenOff).danger().entry(),
        ]
    }

    pub(super) fn set_stream_quality(&mut self, resolution: Option<u32>, fps: Option<u32>, cx: &mut Context<Self>) {
        self.update_prefs(cx, |p| {
            p.stream_resolution = resolution.unwrap_or(p.stream_resolution);
            p.stream_fps = fps.unwrap_or(p.stream_fps);
        });
        self.restart_share(cx);
    }

    pub(crate) fn toggle_noise_suppression(&mut self, cx: &mut Context<Self>) {
        self.update_prefs(cx, |p| {
            p.noise_suppression = !p.noise_suppression;
            p.input_profile = crate::settings::InputProfile::Custom;
        });
    }

    // Discord's camera limit: with more people in the call's channel than the server allows video for,
    // a camera that's off can't come on. The limit, when it's reached.
    pub(crate) fn camera_limit(&self) -> Option<u32> {
        let call = self.call.as_ref()?;
        let guild = self.guilds.iter().find(|g| Some(&g.id) == call.guild_id.as_ref())?;
        let here = guild.voice_states.iter().filter(|v| v.channel_id.as_ref() == Some(&call.channel.id)).count() as u32;
        match call.channel.raw_type == crate::discord::STAGE_TYPE {
            true => (here > guild.max_stage_video_users).then_some(guild.max_stage_video_users),
            false => (guild.max_video_users > 0 && here > guild.max_video_users).then_some(guild.max_video_users),
        }
    }

    pub(crate) fn toggle_camera(&mut self, cx: &mut Context<Self>) {
        let limited = self.camera_limit().is_some();
        let Some(call) = &mut self.call else { return };
        let on = !call.camera_on;
        if on && limited {
            return;
        }
        let camera = on.then(|| self.prefs.camera.clone().or_else(|| voice::cameras().into_iter().next().map(|c| c.id))).flatten();
        if on && camera.is_none() {
            self.voice_error = Some("No camera found".into());
            return self.redraw(Column::Nav, cx);
        }
        match call.session.set_camera(camera) {
            Ok(()) => call.camera_on = on,
            Err(error) => self.voice_error = Some(error.into()),
        }
        if !on && let Some(me) = self.me.as_ref().map(|m| m.id.clone()) && let Some(frame) = call.cameras.remove(&me) {
            cx.drop_image(frame, None);
        }
        cx.notify(); // lint-allow(global-notify): the camera state shows in the nav dock, the stage tiles (main) and the PiP overlay
    }
}

// .mlabel
pub(super) fn menu_label(text: &'static str, ink: &Inks) -> Div {
    div().pt_1p5().px_2p5().pb_0p5().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(ink.muted).child(text)
}

// .devmenu hr
pub(super) fn menu_rule(ink: &Inks) -> Div {
    div().px_1().py_1().flex_none().child(ui::Separator::new().color(ink.line))
}

// .choice.vrow: a 34px menu row holding a radio or a switch.
pub(super) fn choice_row(id: impl Into<ElementId>, ink: &Inks, cx: &App) -> ui::Control {
    let clear = gpui_kit::transparent_black();
    ui::DropdownMenuItem::new(id, cx).scoped(clear, ink.hover, ink.text, ink.text).min_h(px(CHOICE_ROW_HEIGHT)).px_2p5().py_1p5().squircle(RADIUS_SM).gap_2p5()
}
