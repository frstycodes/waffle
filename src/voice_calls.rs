//! Voice calls, streams and screen sharing.

use crate::*;

// Discord's video cut-offs: a camera off screen this long (longer on a stage; the three seen last kept), and
// every camera once the window has been hidden this long.
const OFFSCREEN_CUT: Duration = Duration::from_secs(30);
const STAGE_OFFSCREEN_CUT: Duration = Duration::from_secs(120);
const KEEP_OFFSCREEN: usize = 3;
const HIDDEN_CUT: Duration = Duration::from_secs(30);

// Discord's go-live notify goes out only in servers of this many members (StreamNotificationsGuildMaxSize).
const GO_LIVE_NOTIFY_FROM: u32 = 31;
const GO_LIVE_NOTIFY_UP_TO: u32 = 50;

impl Waffle {
    pub(crate) fn set_talk_key(&mut self, down: bool) {
        if self.talk_key_down == down {
            return;
        }
        self.talk_key_down = down;
        if let Some(call) = &self.call {
            call.session.set_talk_key(down);
            // Discord's push-to-talk cues stay quiet while you're muted.
            if !self.muted {
                crate::sounds::play(if down { crate::sounds::Cue::PttStart } else { crate::sounds::Cue::PttStop });
            }
        }
    }
    // The call Waffle's own restart left (restart.rs), offered back under the notice bar. Once only: a
    // channel gone or hidden is dropped.
    pub(crate) fn rejoin_after_restart(&mut self, cx: &mut Context<Self>) {
        let Some((guild_id, channel_id)) = self.prefs.rejoin_voice.take() else { return };
        self.prefs.save();
        let channel = self.guilds.iter().find(|g| g.id == guild_id).and_then(|g| g.channels.iter().find(|c| c.id == channel_id)).cloned();
        if let Some(channel) = channel {
            self.voice_notice = Some(VoiceNotice { guild_id: Some(guild_id), channel, at: Instant::now(), why: NoticeWhy::Restarted });
            self.redraw_main(cx);
        }
    }
    pub(crate) fn join_voice(&mut self, channel: Channel, cx: &mut Context<Self>) {
        let guild_id = self.guild_id.clone();
        self.switch_call_asking(channel.clone(), cx, move |this, cx| this.join_voice_in(guild_id.clone(), channel.clone(), cx));
    }
    // The call's server, not the one on screen: a move can come while another server is open.
    pub(crate) fn follow_voice_move(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        let guild_id = self.call.as_ref().and_then(|c| c.guild_id.clone());
        let channel = self.guilds.iter().find(|g| Some(&g.id) == guild_id.as_ref()).and_then(|g| g.channels.iter().find(|c| c.id == channel_id)).cloned();
        match channel {
            Some(channel) => self.join_voice_in(guild_id, channel, cx),
            // We can't show that channel, so we don't stay in it: Discord still has us there.
            None => {
                if let Some(client) = &self.client {
                    client.voice.leave_voice(guild_id.as_deref(), self.muted, self.deafened);
                }
                self.leave_voice(cx);
                self.voice_error = Some("Moved to a voice channel Waffle can't show".into());
            }
        }
    }
    pub(crate) fn join_voice_in(&mut self, guild_id: Option<String>, channel: Channel, cx: &mut Context<Self>) {
        let (Some(client), Some(me)) = (self.client.clone(), self.me.clone()) else { return };
        if self.call.as_ref().is_some_and(|c| c.channel.id == channel.id) {
            return;
        }
        let switched = guild_id.is_some() && self.call.as_ref().is_some_and(|c| c.guild_id == guild_id);
        if let Some(call) = &self.call {
            call.session.supersede();
        }
        match switched {
            true => _ = self.end_call(cx),
            false => self.leave_voice(cx),
        }
        self.call_switched = switched;
        self.voice_error = None;
        self.voice_notice = None;
        // Its playback is outside the call's echo cancellation (see `close_settings`). Before the
        // join, so the call starts with the deafen state the test held.
        self.stop_mic_test();
        let (session, mut events) = voice::join(me.id, guild_id.clone(), channel.id.clone(), client.voice.clone(), self.prefs.voice_settings());
        for (user_id, volume) in &self.prefs.user_volumes {
            session.set_user_volume(user_id, *volume);
        }
        for user_id in &self.prefs.local_mutes {
            session.set_user_muted(user_id, true);
        }
        if self.muted || self.deafened {
            session.set_flags(true, self.deafened);
        }
        let task = cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this.update(cx, |this, cx| this.on_voice(event, cx)).is_err() {
                    break;
                }
            }
        });
        // Ticks the "Voice Connected" timer once a second while in the call.
        cx.spawn(async move |this, cx| {
            // The clock shows whole seconds, so a tick that lands in the same second draws nothing.
            let mut shown = None;
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let in_call = this.update(cx, |this, cx| {
                    let secs = this.call.as_ref().map(|call| call.joined.elapsed().as_secs());
                    if secs.is_some() && shown.replace(secs) != Some(secs) {
                        this.redraw_main(cx);
                    }
                    this.call.is_some()
                });
                if !matches!(in_call, Ok(true)) {
                    break;
                }
            }
        })
        // lint-allow(detached-task): the clock loop stops itself once the call is gone
        .detach();
        // Loaded now, so the chat button can count what arrives before the chat is first opened.
        if guild_id.is_some() {
            self.prefetch(&channel.id, cx);
        }
        self.call = Some(Call { session, guild_id, channel, connected: false, stage: voice::Stage::AwaitingEndpoint, stats: None, level: (SILENCE_DB, false), speaking: HashSet::new(), last_spoke: HashMap::new(), cameras: HashMap::new(), camera_on: false, watching: Vec::new(), focus: None, ended: Vec::new(), sharing: None, share_label: None, share_source: None, live_since: None, self_view: None, issues: HashSet::new(), joined: Instant::now(), _events: task });
        // lint-allow(global-notify): joining changes the dock, the rail and the participant list as well as the main column
        cx.notify();
    }
    // Another client of ours joined voice in `guild_id` (None for a DM call). The voice server can drop
    // this one (4006) before that client's voice state reaches us, so a call in the same server that
    // ended moments ago is explained after the fact. One in another server is a call of its own.
    pub(crate) fn taken_elsewhere(&mut self, guild_id: Option<&str>) {
        if self.call.is_some() {
            return;
        }
        let Some(last) = self.last_call.take_if(|c| c.at.elapsed() < TAKEN_ELSEWHERE_WINDOW && c.guild_id.as_deref() == guild_id) else { return };
        self.voice_error = None;
        self.voice_notice = Some(last);
    }
    pub(crate) fn reconnect_voice(&mut self, cx: &mut Context<Self>) {
        let Some(notice) = self.voice_notice.take() else { return };
        self.join_voice_in(notice.guild_id, notice.channel, cx);
    }
    pub(crate) fn ring_out(&mut self, channel: String, cx: &mut Context<Self>) {
        sounds::ring(sounds::Cue::CallCalling);
        self.ringing_out = Some(channel.clone());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(60)).await;
            this.update(cx, |this, _| {
                if this.ringing_out.as_ref() == Some(&channel) {
                    this.stop_ringing_out();
                }
            })
            .ok();
        })
        // lint-allow(detached-task): one 60 second timer, then it ends
        .detach();
    }
    pub(crate) fn stop_ringing_out(&mut self) {
        if self.ringing_out.take().is_some() {
            sounds::stop_ring();
        }
    }
    pub(crate) fn leave_voice(&mut self, cx: &mut Context<Self>) {
        let stage = self.call.as_ref().is_some_and(|c| c.channel.raw_type == 13);
        if self.end_call(cx) && !stage {
            sounds::play(sounds::Cue::Disconnect);
        }
    }
    // Ends the call without a sound; whether there was one.
    pub(crate) fn end_call(&mut self, cx: &mut Context<Self>) -> bool {
        self.call_fullscreen = false;
        self.camera_on_connect = false;
        self.stop_ringing_out();
        let Some(call) = self.call.take() else { return false };
        // Left while looking at it: the channel shows its join screen, as Discord's.
        if self.call_view && call.guild_id.is_some() && call.channel.raw_type != 13 {
            self.stage_ui.join = Some(call.channel.clone());
        }
        self.end_activities(cx);
        self.soundboard.leave_call();
        self.call_focus = None;
        self.call_chat = false;
        self.call_previews = Default::default();
        self.call_unread_at = None;
        if call.guild_id.is_none() {
            let name = self.dms.iter().find(|d| d.id == call.channel.id).map_or_else(|| call.channel.name.clone(), |d| d.name.clone());
            self.show_call_ended(name, call.joined.elapsed().as_secs(), cx);
        }
        release_frames(call, cx);
        // lint-allow(global-notify): ending the call changes the dock, the rail and the participant list as well as the main column
        cx.notify();
        true
    }
    pub(crate) fn on_voice(&mut self, event: voice::VoiceEvent, cx: &mut Context<Self>) {
        // The mic level arrives ~25 times a second in a call, so focus and Disable Video reach the SFU
        // within a frame or two without hooking every place that changes them.
        if matches!(event, voice::VoiceEvent::InputLevel { .. }) {
            self.sync_video_wants();
        }
        // A stream picked from the hover card before we'd joined: watch it once connected.
        let mut start_watch: Option<String> = None;
        // Deafened, Discord hears no one and so shows no one speaking; our own ring stays.
        let event = match event {
            voice::VoiceEvent::Speaking { user_id, speaking: true } if self.deafened && self.me.as_ref().is_none_or(|m| m.id != user_id) => voice::VoiceEvent::Speaking { user_id, speaking: false },
            other => other,
        };
        if let voice::VoiceEvent::Speaking { user_id, speaking } = &event {
            self.activity_speaking(user_id, *speaking, cx);
        }
        let mut lapsed: Option<String> = None;
        let mut ready_checks: Vec<String> = Vec::new();
        let Some(call) = &mut self.call else { return };
        match event {
            voice::VoiceEvent::Connected => {
                if self.ring_on_connect.as_ref() == Some(&call.channel.id) {
                    let channel = self.ring_on_connect.take().unwrap_or_default();
                    self.ring_dm(channel, cx);
                }
                start_watch = self.watch_on_connect.take();
                let me = self.me.as_ref().map(|m| m.id.clone());
                for live in self.participants().into_iter().filter(|p| p.streaming && Some(&p.user_id) != me.as_ref()) {
                    self.auto_watch(live.user_id, cx);
                }
                self.activity_on_connect(cx);
                let Some(call) = &mut self.call else { return };
                call.connected = true;
                call.stage = voice::Stage::Connected;
                // Stages make no join or leave sounds (docs/discord-stage-re.md).
                if call.channel.raw_type != 13 {
                    sounds::play(sounds::connected_cue(self.call_switched));
                }
                self.fetch_soundboard(cx);
                if std::mem::take(&mut self.camera_on_connect) {
                    self.toggle_camera(cx);
                }
            }
            voice::VoiceEvent::Stage(stage) => {
                // A connected call dropping plays Discord's disconnect cue; reconnecting plays the join cue.
                if call.connected && matches!(stage, voice::Stage::Disconnected | voice::Stage::NoRoute) {
                    call.connected = false;
                    if call.channel.raw_type != STAGE_CHANNEL {
                        sounds::play(sounds::Cue::Disconnect);
                    }
                }
                call.stage = stage;
            }
            voice::VoiceEvent::Stats(stats) => call.stats = Some(stats),
            // ~25 a second: only the gate is drawn in the call, the level only on the settings meter.
            voice::VoiceEvent::InputLevel { db, gate_open } => {
                let gate_changed = call.level.1 != gate_open;
                call.level = (db, gate_open);
                if self.mic_test.is_none() {
                    self.mic_level = db;
                }
                if gate_changed && gate_open {
                    self.heard_while_muted(cx);
                }
                if !gate_changed && !self.settings_open() {
                    return;
                }
            }
            voice::VoiceEvent::Speaking { user_id, speaking: true } => {
                call.speaking.insert(user_id);
            }
            voice::VoiceEvent::Speaking { user_id, speaking: false } => {
                call.speaking.remove(&user_id);
                call.last_spoke.insert(user_id, Instant::now());
            }
            voice::VoiceEvent::VideoState { user_id, streaming, camera } => {
                let guild_id = call.guild_id.clone();
                let state = self.guilds.iter_mut().filter(|g| Some(&g.id) == guild_id.as_ref()).flat_map(|g| g.voice_states.iter_mut()).find(|v| v.user_id == user_id);
                if let Some(state) = state {
                    state.streaming = streaming;
                    state.camera = camera;
                }
                // Live again while their ended card is still up: rejoin, as Discord does. Close stream clears it.
                if streaming && call.ended.contains(&user_id) {
                    start_watch = Some(user_id.clone());
                }
                if !streaming && call.watched(&user_id).is_some_and(|w| !w.ended()) {
                    lapsed = Some(user_id.clone());
                }
                // A camera tile is drawn while it has a frame: without this, a camera turned off froze on its last one.
                if camera && !call.cameras.contains_key(&user_id) {
                    ready_checks.push(format!("camera:{user_id}"));
                }
                if !camera {
                    call.issues.remove(&(format!("camera:{user_id}"), voice::issues::RECEIVER_READY_TIMEOUT));
                }
                if !camera && let Some(frame) = call.cameras.remove(&user_id) {
                    cx.drop_image(frame, None);
                }
            }
            voice::VoiceEvent::StreamFrame { user_id, source: voice::VideoSource::GoLive } => {
                // Taken whatever happens next: an untaken frame holds back the next one's event.
                let Some(frame) = call.session.take_frame(&user_id, voice::VideoSource::GoLive) else { return };
                let mine = self.me.as_ref().is_some_and(|m| m.id == user_id);
                let watching_self = call.watched(&user_id).is_some();
                if mine && self.prefs.hidden_self_streams.contains(&call.channel.id) {
                    if let Some((old, _)) = call.self_view.take() {
                        cx.drop_image(old, None);
                    }
                    return;
                }
                // The self-view runs at 15 fps: a full-rate copy of every frame isn't worth it for a thumbnail.
                if mine && !watching_self {
                    if call.self_view.as_ref().is_some_and(|(_, at)| at.elapsed() < SELF_VIEW_INTERVAL) {
                        return;
                    }
                    let Some(image) = render_image(frame) else { return };
                    if let Some((old, _)) = call.self_view.replace((image, Instant::now())) {
                        cx.drop_image(old, None);
                    }
                    // lint-allow(global-notify): the self-view frame shows in the call view and in the stream PiP, which is drawn outside the columns
                    return cx.notify();
                }
                call.issues.remove(&(format!("stream:{user_id}"), voice::issues::RECEIVER_READY_TIMEOUT));
                let Some(watching) = call.watching.iter_mut().find(|w| w.streamer_id == user_id) else { return };
                let Some(image) = render_image(frame) else { return };
                watching.rejoins = 0;
                watching.state = WatchState::Active;
                watching.arrivals.push_back(Instant::now());
                while watching.arrivals.len() > 120 {
                    watching.arrivals.pop_front();
                }
                if let Some(old) = watching.frame.replace(image) {
                    // Each frame is a new GPU texture; the old one must be freed or the atlas grows forever.
                    cx.drop_image(old, None);
                }
            }
            voice::VoiceEvent::StreamFrame { user_id, source: voice::VideoSource::Camera } => {
                let Some(frame) = call.session.take_frame(&user_id, voice::VideoSource::Camera) else { return };
                call.issues.remove(&(format!("camera:{user_id}"), voice::issues::RECEIVER_READY_TIMEOUT));
                let Some(image) = render_image(frame) else { return };
                if let Some(old) = call.cameras.insert(user_id, image) {
                    cx.drop_image(old, None);
                }
            }
            voice::VoiceEvent::Issue { user_id, code, active } => {
                match active {
                    true => call.issues.insert((user_id, code)),
                    false => call.issues.remove(&(user_id, code)),
                };
            }
            voice::VoiceEvent::StreamCreated { user_id } if self.me.as_ref().is_some_and(|m| m.id == user_id) => return self.notify_go_live(cx),
            voice::VoiceEvent::StreamCreated { user_id } => {
                if let Some(watching) = call.watching.iter_mut().find(|w| w.streamer_id == user_id && matches!(w.state, WatchState::Connecting(_))) {
                    watching.state = WatchState::Connecting(None);
                }
            }
            voice::VoiceEvent::StreamConnected { user_id } => {
                if let Some(watching) = call.watching.iter_mut().find(|w| w.streamer_id == user_id)
                    && matches!(watching.state, WatchState::Connecting(_) | WatchState::Reconnecting)
                {
                    watching.state = WatchState::Active;
                }
                ready_checks.push(format!("stream:{user_id}"));
            }
            voice::VoiceEvent::StreamEnded { user_id, reason } => {
                if reason != "Stopped" && !is_delete_reason(&reason) {
                    eprintln!("voice: stream ended: {reason}");
                }
                // A stream that was replaced or watched again ends after its successor started; only the
                // one whose own handle has ended is over.
                if self.me.as_ref().is_some_and(|m| m.id == user_id) {
                    let capture_failed = call.issues.contains(&(user_id.clone(), voice::issues::SCREENSHARE_OS_ERROR));
                    if call.sharing.as_ref().is_some_and(voice::GoLive::ended) {
                        self.end_share(cx);
                        if capture_failed {
                            self.voice_error = Some(format!("Your system stopped the screen capture. Error code: {}", voice::issues::SCREENSHARE_OS_ERROR).into());
                        }
                    }
                    return;
                }
                if !call.watched(&user_id).is_some_and(Watching::ended) {
                    return;
                }
                return self.on_watch_ended(user_id, reason, cx);
            }
            voice::VoiceEvent::Moved(channel_id) => return self.follow_voice_move(&channel_id, cx),
            voice::VoiceEvent::Disconnected(reason) => {
                let stage = self.call.as_ref().is_some_and(|c| c.channel.raw_type == 13);
                if let Some(call) = self.call.take() {
                    self.last_call = Some(VoiceNotice { guild_id: call.guild_id.clone(), channel: call.channel.clone(), at: Instant::now(), why: NoticeWhy::Elsewhere });
                    release_frames(call, cx);
                }
                self.call_focus = None;
                self.stop_ringing_out();
                if !stage {
                    sounds::play(sounds::Cue::Disconnect);
                }
                match reason.as_str() {
                    "Left" => {}
                    voice::ANOTHER_LOCATION => {
                        let guild_id = self.last_call.as_ref().and_then(|c| c.guild_id.clone());
                        self.taken_elsewhere(guild_id.as_deref());
                    }
                    _ => self.voice_error = Some(reason.into()),
                }
            }
        }
        if let Some(streamer) = start_watch {
            self.watch_stream(streamer, cx);
        }
        if let Some(streamer) = lapsed {
            self.on_live_lapse(streamer, cx);
        }
        for key in ready_checks {
            self.check_ready(key, cx);
        }
        // lint-allow(global-notify): a voice event changes the call tiles, the sidebar's participants and the dock, and the PiP outside the columns
        cx.notify();
    }
    // Adds `streamer_id`'s Go Live to the streams we watch and focuses it; Discord lets you watch several.
    // The call's voice channel as the server now has it, for its region.
    fn call_channel_region(&self) -> Option<String> {
        let call = self.call.as_ref()?;
        let guild = self.guilds.iter().find(|g| Some(&g.id) == call.guild_id.as_ref())?;
        guild.channels.iter().find(|c| c.id == call.channel.id)?.props.rtc_region.clone()
    }
    // Our Go Live's key, as Discord builds it.
    pub(crate) fn own_stream_key(&self) -> Option<String> {
        let (call, me) = (self.call.as_ref()?, self.me.as_ref()?);
        Some(match &call.guild_id {
            Some(guild) => format!("guild:{guild}:{}:{}", call.channel.id, me.id),
            None => format!("call:{}:{}", call.channel.id, me.id),
        })
    }
    // Discord tells friends we went live in a server of 31 to 50 members, when the setting is on.
    fn notify_go_live(&mut self, cx: &mut Context<Self>) {
        let guild = self.call.as_ref().and_then(|c| c.guild_id.as_ref()).and_then(|id| self.guilds.iter().find(|g| &g.id == id));
        let mid_size = guild.is_some_and(|g| (GO_LIVE_NOTIFY_FROM..=GO_LIVE_NOTIFY_UP_TO).contains(&g.member_count));
        let (Some(client), Some(key)) = (self.client.clone(), self.own_stream_key()) else { return };
        if !mid_size || !self.account_settings.notify_friends_on_go_live {
            return;
        }
        cx.background_spawn(async move {
            if let Err(e) = client.call(crate::discord::Method::POST, format!("/streams/{key}/notify"), None).await {
                eprintln!("go live notify {key}: {e}");
            }
        })
        // lint-allow(detached-task): a one-shot request that ends when it returns
        .detach();
    }
    // The voice channel's region changed while we're live: Discord moves the stream there too.
    pub(crate) fn follow_stream_region(&mut self, cx: &mut Context<Self>) {
        let region = self.call_channel_region();
        let live = self.call.as_ref().is_some_and(|c| c.sharing.is_some());
        let (Some(client), Some(key), true) = (self.client.clone(), self.own_stream_key(), live) else { return };
        if let Some(call) = &self.call {
            call.session.set_stream_region(region.clone());
        }
        cx.background_spawn(async move {
            if let Err(e) = client.call(crate::discord::Method::PATCH, format!("/streams/{key}"), Some(serde_json::json!({ "region": region }))).await {
                eprintln!("stream region {key}: {e}");
            }
        })
        // lint-allow(detached-task): a one-shot request that ends when it returns
        .detach();
    }
    // In a DM call or on a stage Discord watches every stream as it starts, without taking the focus.
    pub(crate) fn auto_watch(&mut self, streamer_id: String, cx: &mut Context<Self>) {
        let Some(call) = &self.call else { return };
        if call.guild_id.is_some() && call.channel.raw_type != STAGE_CHANNEL || call.watched(&streamer_id).is_some() {
            return;
        }
        let focus = call.focus.clone();
        self.watch_stream(streamer_id, cx);
        if let Some(call) = &mut self.call {
            call.focus = focus;
        }
    }
    // Discord's media sink wants: other people's cameras that are on and not disabled set the quality,
    // the focused one gets full quality, a disabled one none.
    // Discord also stops a camera that has been off screen a while (the three seen last excepted), and every
    // camera once the window has been hidden a while.
    fn sync_video_wants(&mut self) {
        let Some(call) = &self.call else { return };
        let me = self.me.as_ref().map(|m| m.id.clone());
        let on: Vec<String> = self.participants().into_iter().filter(|p| p.camera && Some(&p.user_id) != me.as_ref() && !self.prefs.hidden_video.contains(&p.user_id)).map(|p| p.user_id).collect();
        let focused = self.call_focus.as_deref().and_then(|key| key.strip_prefix("user:")).map(str::to_owned);
        // The call view shows every camera; elsewhere only the PiP's, which is the focused one or the first.
        let on_screen = |id: &String| self.call_view || focused.as_ref().or(on.first()) == Some(id);
        let now = Instant::now();
        self.camera_seen.retain(|id, _| on.contains(id));
        for id in on.iter().filter(|id| on_screen(id)) {
            self.camera_seen.insert(id.clone(), now);
        }
        let cut_off = if call.channel.raw_type == STAGE_CHANNEL { STAGE_OFFSCREEN_CUT } else { OFFSCREEN_CUT };
        let mut off: Vec<(&String, Instant)> = on.iter().filter(|id| !on_screen(id)).map(|id| (id, *self.camera_seen.entry(id.clone()).or_insert(now))).collect();
        off.sort_by_key(|(_, seen)| std::cmp::Reverse(*seen));
        let hidden = self.window_hidden_since.is_some_and(|since| since.elapsed() >= HIDDEN_CUT);
        let mut disabled = self.prefs.hidden_video.clone();
        disabled.extend(off.into_iter().skip(KEEP_OFFSCREEN).filter(|(_, seen)| seen.elapsed() >= cut_off).map(|(id, _)| id.clone()));
        if hidden {
            disabled.extend(on.iter().cloned());
        }
        call.session.set_video_wants(voice::VideoWants { cameras: on.len(), focused, disabled });
    }
    pub(crate) fn watch_stream(&mut self, streamer_id: String, cx: &mut Context<Self>) {
        let mine = self.me.as_ref().is_some_and(|m| m.id == streamer_id);
        let Some(call) = &mut self.call else { return };
        call.ended.retain(|id| *id != streamer_id);
        call.focus = Some(streamer_id.clone());
        if call.watched(&streamer_id).is_some() {
            // lint-allow(global-notify): focusing a stream changes the call tiles, the sidebar's participants and the PiP outside the columns
            return cx.notify();
        }
        let watch = match mine {
            true if call.sharing.is_some() => None,
            // lint-allow(global-notify): watching a stream changes the call tiles, the sidebar's participants and the PiP outside the columns
            true => return cx.notify(),
            false => match call.session.watch_stream(&streamer_id) {
                Ok(watch) => Some(watch),
                Err(error) => {
                    self.voice_error = Some(error.into());
                    // lint-allow(global-notify): the watch error shows in the dock and on the call tiles, outside the main column
                    return cx.notify();
                }
            },
        };
        let timeout = if call.channel.raw_type == STAGE_CHANNEL { STAGE_WATCH_TIMEOUT } else { WATCH_TIMEOUT };
        call.watching.push(Watching::new(streamer_id.clone(), None, watch));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(timeout).await;
            this.update(cx, |this, cx| this.watch_timed_out(&streamer_id, timeout, cx)).ok();
        })
        // lint-allow(detached-task): one watch-timeout timer, then it ends
        .detach();
        // lint-allow(global-notify): watching a stream changes the call tiles, the sidebar's participants and the PiP outside the columns
        cx.notify();
    }
    pub(crate) fn watch_timed_out(&mut self, streamer_id: &str, timeout: Duration, cx: &mut Context<Self>) {
        let Some(watching) = self.call.as_mut().and_then(|c| c.watching.iter_mut().find(|w| w.streamer_id == streamer_id)) else { return };
        if matches!(watching.state, WatchState::Connecting(Some(since)) if since.elapsed() >= timeout) {
            watching.state = WatchState::Failed(STREAM_FAILED_TO_START);
            // lint-allow(global-notify): a failed watch shows on the call tile and in the PiP outside the columns
            cx.notify();
        }
    }
    // A watched stream's connection ended, handled as Discord handles STREAM_DELETE: unavailable or a
    // dropped connection reconnects, unauthorized fails, full closes with a notice, anything else ends.
    pub(crate) fn on_watch_ended(&mut self, streamer_id: String, reason: String, cx: &mut Context<Self>) {
        let Some(watching) = self.call.as_mut().and_then(|c| c.watching.iter_mut().find(|w| w.streamer_id == streamer_id)) else { return };
        match reason.as_str() {
            voice::STREAM_DELETE_FULL => {
                self.unwatch_stream(&streamer_id, cx);
                self.alert("Stream is Full", "Sorry, this stream has the max number of viewers.", cx);
            }
            voice::STREAM_DELETE_RATE_LIMITED => {
                let guild_id = self.call.as_ref().and_then(|c| c.guild_id.clone());
                let server = self.guilds.iter().find(|g| Some(&g.id) == guild_id.as_ref()).map_or_else(|| "This server".to_owned(), |g| g.name.clone());
                self.end_watch(streamer_id, cx);
                let body = format!("{server} has temporarily lost access to some Discord features because it broke Discord's rules for servers. Please see the Help Center for more information.");
                self.alert("This server's features are limited", body, cx);
            }
            voice::STREAM_DELETE_UNAUTHORIZED => fail_watch(watching, cx),
            // Already failed: the delete that follows a failure keeps it failed, as Discord's.
            _ if matches!(watching.state, WatchState::Failed(_)) => {}
            voice::STREAM_DELETE_UNAVAILABLE => self.reconnect_watch(streamer_id, cx),
            _ if is_delete_reason(&reason) => self.end_watch(streamer_id, cx),
            // Never connected: the stream server didn't answer or refused us.
            _ if matches!(watching.state, WatchState::Connecting(_)) => fail_watch(watching, cx),
            _ => self.reconnect_watch(streamer_id, cx),
        }
    }
    pub(crate) fn reconnect_watch(&mut self, streamer_id: String, cx: &mut Context<Self>) {
        let Some(watching) = self.call.as_mut().and_then(|c| c.watching.iter_mut().find(|w| w.streamer_id == streamer_id)) else { return };
        // Waits longer each time, up to six REJOIN_AFTERs, for as long as the streamer is live.
        let wait = REJOIN_AFTER * u32::from(watching.rejoins.min(5) + 1);
        watching.rejoins = watching.rejoins.saturating_add(1);
        watching.state = WatchState::Reconnecting;
        // The tile shows the reconnecting state rather than a frozen picture.
        if let Some(frame) = watching.frame.take() {
            cx.drop_image(frame, None);
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            this.update(cx, |this, cx| this.rewatch(streamer_id, cx)).ok();
        })
        // lint-allow(detached-task): one rejoin timer, then it ends
        .detach();
        // lint-allow(global-notify): a reconnecting stream shows on the call tile and in the PiP outside the columns
        cx.notify();
    }
    pub(crate) fn rewatch(&mut self, streamer_id: String, cx: &mut Context<Self>) {
        let live = self.participants().iter().any(|s| s.user_id == streamer_id && s.streaming);
        let Some(call) = &mut self.call else { return };
        // Stopped or watched again by hand meanwhile.
        let Some(watching) = call.watching.iter_mut().find(|w| w.streamer_id == streamer_id && w.state == WatchState::Reconnecting) else { return };
        if !live {
            return self.end_watch(streamer_id, cx);
        }
        match call.session.watch_stream(&streamer_id) {
            Ok(watch) => watching.watch = Some(watch),
            // Voice is reconnecting: try again once it's back.
            Err(_) => return self.reconnect_watch(streamer_id, cx),
        }
        // lint-allow(global-notify): a rewatched stream shows on the call tile and in the PiP outside the columns
        cx.notify();
    }
    // A stream connected, or a camera turned on, with no picture READY_TIMEOUT later: Discord's 2012
    // (`key` is "stream:{id}" or "camera:{id}"). The first frame clears it.
    pub(crate) fn check_ready(&mut self, key: String, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(voice::issues::READY_TIMEOUT).await;
            this.update(cx, |this, cx| {
                let Some(call) = &mut this.call else { return };
                let pictured = match key.split_once(':') {
                    Some(("stream", id)) => call.watched(id).is_none_or(|w| !w.arrivals.is_empty() || w.state != WatchState::Active),
                    Some((_, id)) => call.cameras.contains_key(id),
                    None => true,
                };
                if !pictured {
                    call.issues.insert((key, voice::issues::RECEIVER_READY_TIMEOUT));
                    // lint-allow(global-notify): a missing-picture issue shows on the call tile, the dock and the PiP outside the columns
                    cx.notify();
                }
            })
            .ok();
        })
        // lint-allow(detached-task): one READY_TIMEOUT timer, then it ends
        .detach();
    }
    // The streamer's voice state says they stopped while we still watch: closed after LIVE_LAPSE
    // unless they're live again, as Discord does.
    pub(crate) fn on_live_lapse(&mut self, streamer_id: String, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(LIVE_LAPSE).await;
            this.update(cx, |this, cx| {
                let live = this.participants().iter().any(|s| s.user_id == streamer_id && s.streaming);
                let ended = this.call.as_ref().is_some_and(|c| c.ended.contains(&streamer_id));
                if !live && !ended && this.call.as_ref().is_some_and(|c| c.watched(&streamer_id).is_some()) {
                    this.unwatch_stream(&streamer_id, cx);
                }
            })
            .ok();
        })
        // lint-allow(detached-task): one LIVE_LAPSE timer, then it ends
        .detach();
    }
    pub(crate) fn end_watch(&mut self, streamer_id: String, cx: &mut Context<Self>) {
        let focused = self.call_focus.as_deref() == Some(format!("stream:{streamer_id}").as_str());
        let Some(call) = &mut self.call else { return };
        if let Some(frame) = call.unwatch(&streamer_id).and_then(|w| w.frame) {
            cx.drop_image(frame, None);
        }
        sounds::play(sounds::Cue::StreamEnded);
        // In focus, its card stays, saying who stopped, until closed; elsewhere it just goes, as Discord's.
        if focused && !call.ended.contains(&streamer_id) {
            call.ended.push(streamer_id);
        }
        // lint-allow(global-notify): an ended stream leaves the call tiles, the sidebar's participants and the PiP outside the columns
        cx.notify();
    }
    pub(crate) fn unwatch_stream(&mut self, streamer_id: &str, cx: &mut Context<Self>) {
        if let Some(frame) = self.call.as_mut().and_then(|c| c.unwatch(streamer_id)).and_then(|w| w.frame) {
            cx.drop_image(frame, None);
        }
        // lint-allow(global-notify): an unwatched stream leaves the call tiles, the sidebar's participants and the PiP outside the columns
        cx.notify();
    }
    // Whether `key` is our own Go Live or the one we're watching: the stream whose viewers Discord announces.
    pub(crate) fn in_stream(&self, key: &str) -> bool {
        let Some(call) = &self.call else { return false };
        let mine = call.sharing.is_some() && self.me.as_ref().is_some_and(|m| key.ends_with(&format!(":{}", m.id)));
        // Discord announces viewers only for the single stream you're in.
        let watched = matches!(call.watching.as_slice(), [only] if key.ends_with(&format!(":{}", only.streamer_id)));
        mine || watched
    }
    pub(crate) fn stop_watching(&mut self, cx: &mut Context<Self>) {
        let Some(call) = &mut self.call else { return };
        call.focus = None;
        if self.call_focus.as_deref().is_some_and(|key| key.starts_with("stream:")) {
            self.call_focus = None;
        }
        for frame in call.watching.drain(..).filter_map(|w| w.frame) {
            cx.drop_image(frame, None);
        }
        // lint-allow(global-notify): dropping every watched stream changes the call tiles, the sidebar's participants and the PiP outside the columns
        cx.notify();
    }
    /// Stops sharing, or opens Discord's picker of what to share: the whole screen, an app or a window.
    pub(crate) fn toggle_screen_share(&mut self, cx: &mut Context<Self>) {
        let Some(call) = &self.call else { return };
        if call.sharing.is_some() {
            return self.end_share(cx);
        }
        self.open_share_picker(cx);
    }
    // Stops our Go Live, or clears it away once it ended on its own.
    pub(crate) fn end_share(&mut self, cx: &mut Context<Self>) {
        let Some(call) = &mut self.call else { return };
        call.sharing = None;
        call.share_source = None;
        call.live_since = None;
        if let Some((frame, _)) = call.self_view.take() {
            cx.drop_image(frame, None);
        }
        sounds::play(sounds::Cue::StreamEnded);
        if let Some(me) = self.me.as_ref().map(|m| m.id.clone()) {
            self.unwatch_stream(&me, cx);
        }
        // lint-allow(global-notify): ending our Go Live changes the call tiles, the dock, the rail and the PiP outside the columns
        cx.notify();
    }
    pub(crate) fn start_share(&mut self, source: voice::ShareSource, cx: &mut Context<Self>) {
        self.close_share_picker(cx);
        let label = self.share_choice_label.take();
        if self.go_live(source, cx) {
            if let Some(call) = &mut self.call {
                call.share_label = label;
                call.live_since = Some(Instant::now());
            }
            sounds::play(sounds::Cue::StreamStarted);
        }
        // lint-allow(global-notify): starting our Go Live changes the call tiles, the dock, the rail and the PiP outside the columns
        cx.notify();
    }
    // Starts (or restarts) our Go Live of `source` with the current quality prefs.
    pub(crate) fn go_live(&mut self, source: voice::ShareSource, cx: &mut Context<Self>) -> bool {
        let region = self.call_channel_region();
        let Some(call) = &mut self.call else { return false };
        call.session.set_stream_region(region);
        let options = voice::GoLiveOptions { resolution: self.prefs.stream_resolution, fps: self.prefs.stream_fps, audio: self.prefs.stream_audio };
        match call.session.go_live_with_options(source, options, call.sharing.take()) {
            Ok(live) => {
                call.sharing = Some(live);
                call.share_source = Some(source);
                self.upload_stream_previews(source, cx);
                true
            }
            Err(error) => {
                self.voice_error = Some(error.into());
                false
            }
        }
    }
    // Quality and sound changes take effect by restarting the same stream; viewers rejoin it.
    pub(crate) fn restart_share(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut() else { return };
        let Some(source) = call.share_source.filter(|_| call.sharing.is_some()) else { return };
        self.go_live(source, cx);
        // lint-allow(global-notify): restarting our Go Live changes the call tiles, the dock and the PiP outside the columns
        cx.notify();
    }
    // Mute and deafen apply to the call as well as the buttons; deafening also mutes, as in Discord.
    pub(crate) fn sync_voice_flags(&self) {
        let Some(call) = &self.call else { return };
        call.session.set_flags(self.muted || self.deafened, self.deafened);
    }
    // `key` names what was copied (the code itself, a file's URL), so its button shows Copied for a moment.
    pub(crate) fn copy_code(&mut self, code: String, key: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(code));
        self.copied = Some(key.clone());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(1500)).await;
            this.update(cx, |this, cx| {
                if this.copied.as_ref() == Some(&key) {
                    this.copied = None;
                }
                // lint-allow(global-notify): the Copied label shows on whichever surface holds the button, in any column or the overlays
                cx.notify();
            })
            .ok();
        })
        // lint-allow(detached-task): one 1.5 second timer, then it ends
        .detach();
        // lint-allow(global-notify): the Copied label shows on whichever surface holds the button, in any column or the overlays
        cx.notify();
    }
}
