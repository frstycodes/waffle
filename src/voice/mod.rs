// Discord voice and video: main-gateway handshake, voice gateway v8, UDP transport, DAVE E2EE,
// Opus audio with Discord's voice processing (docs/discord-audio.md), H.264 cameras and Go Live (watching and, on macOS,
// streaming). Voice, watching a Go Live and the stream ops 19/20 were checked against Discord
// live; going live (main ops 18/22, voice op 12, soundshare speaking flag) and sending a camera
// are from memory of what official clients send — Not verified.
// `WAFFLE_VOICE_DEBUG=1` logs gateway traffic and undecodable packets to stderr.

mod attenuation;
mod audio;
mod camera;
#[cfg(target_os = "macos")]
mod capture;
#[cfg(target_os = "windows")]
#[path = "capture_windows.rs"]
mod capture;
mod dave;
pub mod issues;
mod dsp;
mod feedback;
mod taps;
#[cfg(test)]
mod echo_check;
#[cfg(test)]
mod chain_check;
#[cfg(test)]
mod fixtures;
mod jitter;
mod media;
mod rtp;
mod vad;
mod video;
#[cfg(target_os = "windows")]
mod video_mf;

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use futures::{SinkExt, StreamExt, channel::mpsc, channel::oneshot};
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::{self, Message as WsMessage};

pub use attenuation::{Attenuation, SUPPORTED as ATTENUATION_SUPPORTED};
pub use audio::{Device, input_devices, output_devices};
pub use camera::cameras;
pub use dsp::NoiseSuppression;
pub use video::VideoFrame;

const SERVER_TIMEOUT: Duration = Duration::from_secs(10);
// How long a call closed with 4022 waits for a VOICE_SERVER_UPDATE that moves it elsewhere.
const MOVE_TIMEOUT: Duration = Duration::from_secs(5);
// Every voice state in every guild passes through: a receiver more than this far behind misses
// dispatches, and a missed STREAM_DELETE leaves a stream we watch frozen on its last frame.
const DISPATCH_BACKLOG: usize = 1024;
// How long a stopped stream waits for Discord to confirm its delete before a replacement starts anyway.
const DELETE_TIMEOUT: Duration = Duration::from_secs(3);
const STATS_EVERY: Duration = Duration::from_secs(2);
const PINGS_KEPT: usize = 20;
// Stats ticks of outbound loss averaged: 15 × 2 s = 30 s.
const LOSS_WINDOW: usize = 15;
const DUCK_TICK: Duration = Duration::from_millis(20);
// A dropped voice gateway (a reset, a server restart) is resumed this many times in a row before the
// call ends, waiting RESUME_BACKOFF longer each time. The UDP media keeps flowing meanwhile.
const RESUME_TRIES: u32 = 5;
const RESUME_BACKOFF: Duration = Duration::from_secs(1);
// DSCP Expedited Forwarding in the IPv4 TOS byte: the "high packet priority" Quality of Service asks for.
const QOS_TOS: u32 = 0xb8;

pub enum VoiceEvent {
    Connected,
    Stage(Stage),
    Stats(Stats),
    // Our mic level after processing, about 25 times a second while in a call.
    InputLevel { db: f32, gate_open: bool },
    Speaking { user_id: String, speaking: bool },
    // A user in our channel turned Go Live or their camera on/off. Users already live when we
    // joined are in GUILD_CREATE `voice_states[].self_stream` / `self_video`, which the UI owns.
    VideoState { user_id: String, streaming: bool, camera: bool },
    // A new frame of a remote camera or Go Live, or our own camera or Go Live (our user id) for the
    // self-preview: take it with `VoiceSession::take_frame`. Frames not taken yet are replaced.
    StreamFrame { user_id: String, source: VideoSource },
    // One of Discord's AV error conditions (`issues`) started or cleared for `user_id` (ours for what
    // we send).
    Issue { user_id: String, code: u16, active: bool },
    // A watched Go Live's media connection is up (Discord's RTC_CONNECTED for the stream).
    StreamConnected { user_id: String },
    // `reason` is STREAM_DELETE's (`STREAM_DELETE_*` below), "Stopped" when we ended it, or the
    // transport error that dropped the connection.
    StreamEnded { user_id: String, reason: String },
    // Discord moved our session to this channel (someone with Move Members dragged us): the
    // session has ended without leaving, for the app to join it there.
    Moved(String),
    Disconnected(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VideoSource {
    Camera,
    GoLive,
}

// Connection progress. Discord's desktop voice runs over plain UDP, not WebRTC, so the ICE and
// DTLS stages stand for their UDP counterparts: IceChecking = IP discovery, DtlsConnecting =
// waiting for the encryption key after choosing a cipher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    AwaitingEndpoint,
    Authenticating,
    RtcConnecting,
    IceChecking,
    DtlsConnecting,
    Connected,
    NoRoute,
    Disconnected,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub ping_ms: u32,
    // The median of the kept pings: one slow heartbeat can't make the whole call read as poor.
    pub median_ping_ms: u32,
    pub last_pings: Vec<u32>,
    // How much of our audio the server says it lost, averaged over the last LOSS_WINDOW of reports;
    // None until it has reported.
    pub outbound_loss_pct: Option<f32>,
    // The largest jitter-buffer target among the speakers we hear; None before anyone has spoken.
    pub playout_delay_ms: Option<u32>,
    pub endpoint: String,
    pub server_ip: String,
    pub encryption: String,
    pub dave: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputMode {
    // `auto` lets a detector decide: Advanced Voice Activity's speech probability when `advanced`,
    // else WebRTC's VAD. Otherwise the mic opens above `threshold_db` (-100..0).
    VoiceActivity { auto: bool, threshold_db: f32, advanced: bool },
    // Transmits while the talk key is held, plus `release_delay_ms` after release.
    PushToTalk { release_delay_ms: u32 },
}

// Everything that shapes our audio, applied from the first packet. All of it can also be changed
// live on the session.
#[derive(Clone, Debug, PartialEq)]
pub struct VoiceSettings {
    // Device ids from `input_devices` / `output_devices`; None = system default.
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    // 0..=2.0
    pub input_volume: f32,
    pub output_volume: f32,
    pub input_mode: InputMode,
    pub echo_cancellation: bool,
    pub noise_suppression: NoiseSuppression,
    pub auto_gain: bool,
    pub attenuation: Attenuation,
    // Mark voice packets high priority (DSCP EF).
    pub qos: bool,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            input_device: None,
            output_device: None,
            input_volume: 1.0,
            output_volume: 1.0,
            input_mode: DEFAULT_INPUT_MODE,
            echo_cancellation: true,
            noise_suppression: NoiseSuppression::Advanced,
            auto_gain: true,
            attenuation: Attenuation::default(),
            qos: false,
        }
    }
}

// Discord's defaults: voice activity, automatic sensitivity with Advanced Voice Activity, -60 dB
// for when it's manual.
const DEFAULT_INPUT_MODE: InputMode = InputMode::VoiceActivity { auto: true, threshold_db: -60.0, advanced: true };

impl VoiceSettings {
    fn audio(&self) -> audio::AudioSettings {
        let processor = dsp::Processor::new(self.echo_cancellation, self.noise_suppression, self.auto_gain);
        audio::AudioSettings::new(self.input_device.clone(), self.output_device.clone(), self.input_volume, self.output_volume, processor)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoLiveOptions {
    // Stream height: 480, 720 or 1080; 0 = the source's own size.
    pub resolution: u32,
    // 15, 30 or 60.
    pub fps: u32,
    // Send the shared app's / system's sound.
    pub audio: bool,
}

impl Default for GoLiveOptions {
    fn default() -> Self {
        Self { resolution: 720, fps: 30, audio: true }
    }
}

// What a voice-gateway connection is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    // The channel itself: our mic and camera out, everyone's audio and cameras in.
    Voice,
    // Someone else's Go Live: their screen and its audio in.
    Watch,
    // Our Go Live: our screen (or one app / window) out.
    Broadcast(ShareSource, GoLiveOptions),
}

impl Role {
    fn video_source(self) -> VideoSource {
        match self {
            Role::Voice => VideoSource::Camera,
            Role::Watch | Role::Broadcast(..) => VideoSource::GoLive,
        }
    }

    fn broadcasts(self) -> bool {
        matches!(self, Role::Broadcast(..))
    }
}

// What a Go Live shares. Audio follows the source, as in Discord: the whole screen sends all
// system audio except Waffle's own; an app or window sends only that app's sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShareSource {
    // One display, by `ShareableScreen::id`.
    Screen(u32),
    // Every window of the app with this process id.
    #[allow(dead_code, reason = "examples/voice_join.rs drives it; the app doesn't yet")]
    App(i32),
    // One window, by `ShareableWindow::id`.
    Window(u32),
}

// A window that can be shared, for the UI's picker. `pid` doubles as `ShareSource::App`.
#[derive(Clone, Debug)]
pub struct ShareableWindow {
    pub id: u32,
    pub title: String,
    pub app_name: String,
    pub pid: i32,
}

#[derive(Clone, Debug)]
pub struct ShareableScreen {
    pub id: u32,
    // "Screen 1", "Screen 2", … in the system's display order (main display first).
    pub name: String,
    // Points, not pixels.
    #[allow(dead_code, reason = "examples/voice_join.rs drives it; the app doesn't yet")]
    pub width: u32,
    #[allow(dead_code, reason = "examples/voice_join.rs drives it; the app doesn't yet")]
    pub height: u32,
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const NO_CAPTURE: &str = "Screen sharing is only implemented on macOS and Windows";

// Visible, titled windows of other apps (macOS and Windows); Err elsewhere.
pub fn shareable_windows() -> Result<Vec<ShareableWindow>, String> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    return capture::shareable_windows();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    Err(NO_CAPTURE.into())
}

pub fn screens() -> Result<Vec<ShareableScreen>, String> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    return capture::screens();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    Err(NO_CAPTURE.into())
}

// One still of `source`, BGRA, scaled to fit `max_width` (and never upscaled). Takes tens of
// milliseconds, so call it off the UI thread.
pub fn thumbnail(source: ShareSource, max_width: u32) -> Result<VideoFrame, String> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    return capture::thumbnail(source, max_width);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (source, max_width);
        Err("No thumbnails on this platform yet".into())
    }
}

// The icon of the app with process id `pid`, as a `size` x `size` BGRA square (premultiplied alpha).
pub fn app_icon(pid: i32, size: u32) -> Option<VideoFrame> {
    #[cfg(target_os = "macos")]
    return capture::app_icon(pid, size);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (pid, size);
        None
    }
}

// The voice module's view of the main gateway. `src/discord.rs` writes every string from the
// receiver returned by `new` to its socket and feeds VOICE_* / STREAM_* dispatches into `dispatch`.
#[derive(Clone)]
pub struct GatewayHandle {
    outbound: mpsc::UnboundedSender<String>,
    dispatches: broadcast::Sender<(String, Value)>,
}

impl GatewayHandle {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<String>) {
        let (outbound, rx) = mpsc::unbounded();
        (Self { outbound, dispatches: broadcast::channel(DISPATCH_BACKLOG).0 }, rx)
    }

    pub fn dispatch(&self, kind: &str, d: &Value) {
        if kind.starts_with("VOICE_") || kind.starts_with("STREAM_") {
            if debug() {
                let brief = json!({ "user": d["user_id"], "session": d["session_id"], "channel": d["channel_id"], "stream": d["self_stream"], "key": d["stream_key"] });
                eprintln!("main {kind}: {brief}");
            }
            let _ = self.dispatches.send((kind.to_owned(), d.clone()));
        }
    }

    fn send(&self, op: u8, d: Value) {
        let _ = self.outbound.unbounded_send(json!({ "op": op, "d": d }).to_string());
    }

    // Our voice state; `channel_id` None leaves voice.
    fn send_voice_state(&self, guild_id: Option<&str>, channel_id: Option<&str>, muted: bool, deafened: bool, video: bool) {
        self.send(4, json!({ "guild_id": guild_id, "channel_id": channel_id, "self_mute": muted, "self_deaf": deafened, "self_video": video }));
    }

    // Leaves voice when no session can: one that was moved ends without leaving, for the next to carry on.
    pub fn leave_voice(&self, guild_id: Option<&str>, muted: bool, deafened: bool) {
        self.send_voice_state(guild_id, None, muted, deafened, false);
    }
}

pub struct VoiceSession {
    shared: Arc<Shared>,
    // Dropping this sender is the leave signal.
    _leave: oneshot::Sender<()>,
}

// Dropping it stops watching.
pub struct StreamWatch {
    stop: oneshot::Sender<()>,
}

impl StreamWatch {
    // The stream ended on its own. Already true when its `StreamEnded` arrives, which tells that
    // event apart from the end of a watch this one replaced.
    pub fn ended(&self) -> bool {
        self.stop.is_canceled()
    }
}

// Our Go Live; dropping it ends the stream.
pub struct GoLive {
    stop: oneshot::Sender<()>,
    // Fires once Discord has deleted this stream, so a replacement under the same key can start.
    gone: oneshot::Receiver<()>,
}

impl GoLive {
    // As `StreamWatch::ended`: a restarted stream's predecessor ends after it began.
    pub fn ended(&self) -> bool {
        self.stop.is_canceled()
    }
}

struct Shared {
    gateway: GatewayHandle,
    user_id: String,
    guild_id: Option<String>,
    channel_id: String,
    events: mpsc::UnboundedSender<VoiceEvent>,
    // Issues raised and not yet cleared, so each change is told once.
    issues: Mutex<std::collections::HashSet<(String, u16)>>,
    session_id: Mutex<Option<String>>,
    // A VOICE_SERVER_UPDATE mid-call: Discord moved the call to this server; the session reconnects there.
    moved_server: Mutex<Option<Server>>,
    muted: AtomicBool,
    deafened: AtomicBool,
    // A voice-state update is waiting to go out, and when the last one did (see `send_flags_soon`).
    flags_pending: AtomicBool,
    flags_sent: Mutex<Option<Instant>>,
    talk_key_held: AtomicBool,
    input_mode: Mutex<InputMode>,
    audio: Arc<audio::AudioSettings>,
    // Everything we play, from the voice connection and any stream we watch.
    mixer: Arc<audio::Mixer>,
    // Local per-user listening volume (0..=2) and mute, by user id.
    listening: Mutex<HashMap<String, (f32, bool)>>,
    // Per-streamer Go Live volume (0..=2), on top of their listening volume.
    stream_volume: Mutex<HashMap<String, f32>>,
    // The camera the user wants on (None = off); the voice connection follows it.
    camera: Mutex<Option<String>>,
    attenuation: Mutex<Attenuation>,
    qos: bool,
    // Who is speaking right now, for attenuation.
    self_speaking: AtomicBool,
    others_speaking: AtomicBool,
    // "No Audio Detected": 0 not known yet, 1 detected, 2 not detected.
    input_detected: AtomicU8,
    // Set when another channel's join replaces this session: it must not send the leave, which
    // can reach Discord after that join and disconnect us from the new channel.
    superseded: AtomicBool,
    // Another client of ours joined voice and took the call. Like `superseded`, the leave must not go
    // out: our account has one voice state, and clearing it could end the call on that other client.
    taken_elsewhere: AtomicBool,
    // The newest frame of each picture, until the UI takes it. Its event goes out only when the
    // slot was empty, so frames the UI hasn't drawn are replaced, never queued behind each other
    // (and ahead of every other voice event).
    frames: Mutex<HashMap<(String, VideoSource), Arc<VideoFrame>>>,
}

// Spamming mute or deafen sent an update per press, and Discord rate-limits the gateway (about 120
// messages a minute): updates were dropped, the shown state went stale, and rejoining could fail. Presses
// now coalesce into one update with the latest flags, at most one per `FLAGS_GAP`.
const FLAGS_GAP: Duration = Duration::from_millis(500);

fn send_flags_soon(shared: &Arc<Shared>) {
    if shared.flags_pending.swap(true, Ordering::AcqRel) {
        return;
    }
    let shared = shared.clone();
    crate::discord::runtime().spawn(async move {
        let since = shared.flags_sent.lock().ok().and_then(|sent| *sent).map(|at| at.elapsed());
        let wait = since.map_or(Duration::ZERO, |since| FLAGS_GAP.saturating_sub(since));
        tokio::time::sleep(wait.max(Duration::from_millis(60))).await;
        shared.flags_pending.store(false, Ordering::Release);
        if let Ok(mut sent) = shared.flags_sent.lock() {
            *sent = Some(Instant::now());
        }
        shared.send_voice_state(true);
    });
}

impl Shared {
    fn supersede(&self) {
        self.superseded.store(true, Ordering::Release);
    }

    fn send_voice_state(&self, in_channel: bool) {
        let camera_on = self.camera.lock().is_ok_and(|c| c.is_some());
        self.gateway.send_voice_state(
            self.guild_id.as_deref(),
            in_channel.then_some(self.channel_id.as_str()),
            self.muted.load(Ordering::Relaxed),
            self.deafened.load(Ordering::Relaxed),
            in_channel && camera_on,
        );
    }

    fn may_transmit(&self) -> bool {
        !self.muted.load(Ordering::Relaxed) && !self.deafened.load(Ordering::Relaxed)
    }

    fn input_mode(&self) -> InputMode {
        self.input_mode.lock().map(|m| *m).unwrap_or(DEFAULT_INPUT_MODE)
    }

    // The volume factor for other applications right now.
    fn attenuation_target(&self) -> f32 {
        let attenuation = self.attenuation.lock().map(|a| *a).unwrap_or_default();
        attenuation.target(self.self_speaking.load(Ordering::Relaxed), self.others_speaking.load(Ordering::Relaxed))
    }

    // The gain to play `user_id` at: their local volume, 0 when locally muted or deafened.
    fn listening_gain(&self, user_id: &str) -> f32 {
        if self.deafened.load(Ordering::Relaxed) {
            return 0.0;
        }
        let local = self.listening.lock().ok().and_then(|l| l.get(user_id).copied());
        match local {
            Some((_, true)) => 0.0,
            Some((volume, false)) => volume,
            None => 1.0,
        }
    }

    // A watched Go Live plays at the streamer's listening gain times its own stream volume.
    fn stream_gain(&self, user_id: &str) -> f32 {
        self.listening_gain(user_id) * self.stream_volume.lock().ok().and_then(|v| v.get(user_id).copied()).unwrap_or(1.0)
    }

    fn server_id(&self) -> &str {
        self.guild_id.as_deref().unwrap_or(&self.channel_id)
    }

    fn emit(&self, event: VoiceEvent) {
        let _ = self.events.unbounded_send(event);
    }

    fn issue(&self, user_id: &str, code: u16, active: bool) {
        let Ok(mut held) = self.issues.lock() else { return };
        let key = (user_id.to_owned(), code);
        let changed = if active { held.insert(key) } else { held.remove(&key) };
        if changed {
            self.emit(VoiceEvent::Issue { user_id: user_id.to_owned(), code, active });
        }
    }

    fn publish_frame(&self, user_id: &str, source: VideoSource, frame: Arc<VideoFrame>) {
        let Ok(mut frames) = self.frames.lock() else { return };
        if frames.insert((user_id.to_owned(), source), frame).is_none() {
            self.emit(VoiceEvent::StreamFrame { user_id: user_id.to_owned(), source });
        }
    }
}

// `guild_id` is None for DM / group calls. Safe to call from any thread; the work runs on
// `crate::discord::runtime()`.
pub fn join(
    user_id: String,
    guild_id: Option<String>,
    channel_id: String,
    gateway: GatewayHandle,
    settings: VoiceSettings,
) -> (VoiceSession, mpsc::UnboundedReceiver<VoiceEvent>) {
    let (events, rx) = mpsc::unbounded();
    let shared = Arc::new(Shared {
        gateway,
        user_id,
        guild_id,
        channel_id,
        events,
        issues: Mutex::default(),
        session_id: Mutex::new(None),
        moved_server: Mutex::new(None),
        muted: AtomicBool::new(false),
        deafened: AtomicBool::new(false),
        flags_pending: AtomicBool::new(false),
        flags_sent: Mutex::new(None),
        talk_key_held: AtomicBool::new(false),
        input_mode: Mutex::new(settings.input_mode),
        audio: Arc::new(settings.audio()),
        mixer: Arc::default(),
        listening: Mutex::new(HashMap::new()),
        stream_volume: Mutex::new(HashMap::new()),
        camera: Mutex::new(None),
        attenuation: Mutex::new(settings.attenuation),
        qos: settings.qos,
        self_speaking: AtomicBool::new(false),
        others_speaking: AtomicBool::new(false),
        input_detected: AtomicU8::new(0),
        superseded: AtomicBool::new(false),
        taken_elsewhere: AtomicBool::new(false),
        frames: Mutex::default(),
    });
    if ATTENUATION_SUPPORTED {
        let weak = Arc::downgrade(&shared);
        let _ = thread::Builder::new().name("waffle-attenuation".into()).spawn(move || duck_other_apps(&weak));
    }
    let (leave_tx, leave_rx) = oneshot::channel();
    crate::discord::runtime().spawn(run(shared.clone(), leave_rx));
    (VoiceSession { shared, _leave: leave_tx }, rx)
}

impl VoiceSession {
    // Mute and deafen go to Discord together, as one voice-state update carrying the latest of both.
    pub fn set_flags(&self, muted: bool, deafened: bool) {
        self.shared.muted.store(muted, Ordering::Relaxed);
        self.shared.deafened.store(deafened, Ordering::Relaxed);
        send_flags_soon(&self.shared);
    }

    // Device ids from `input_devices` / `output_devices`; None = system default. Switches live.
    pub fn set_input_device(&self, id: Option<String>) {
        if let Ok(mut device) = self.shared.audio.input_device.lock() {
            *device = id;
        }
    }

    pub fn set_output_device(&self, id: Option<String>) {
        if let Ok(mut device) = self.shared.audio.output_device.lock() {
            *device = id;
        }
    }

    // 0..=2.0
    pub fn set_input_volume(&self, volume: f32) {
        self.shared.audio.set_input_volume(volume);
    }

    pub fn set_output_volume(&self, volume: f32) {
        self.shared.audio.set_output_volume(volume);
    }

    // How loud `user_id` plays for us, 0..=2.0. Local only.
    // The volume of `user_id`'s Go Live while watching it (0..=2), apart from their voice.
    pub fn set_stream_volume(&self, user_id: &str, volume: f32) {
        if let Ok(mut volumes) = self.shared.stream_volume.lock() {
            volumes.insert(user_id.to_owned(), volume.clamp(0.0, audio::MAX_VOLUME));
        }
    }

    pub fn set_user_volume(&self, user_id: &str, volume: f32) {
        if let Ok(mut listening) = self.shared.listening.lock() {
            listening.entry(user_id.to_owned()).or_insert((1.0, false)).0 = volume.clamp(0.0, audio::MAX_VOLUME);
        }
    }

    // Stop hearing `user_id`. Local only.
    pub fn set_user_muted(&self, user_id: &str, muted: bool) {
        if let Ok(mut listening) = self.shared.listening.lock() {
            listening.entry(user_id.to_owned()).or_insert((1.0, false)).1 = muted;
        }
    }

    pub fn set_input_mode(&self, mode: InputMode) {
        if let Ok(mut current) = self.shared.input_mode.lock() {
            *current = mode;
        }
    }

    // The push-to-talk key; only matters in `InputMode::PushToTalk`.
    pub fn set_talk_key(&self, held: bool) {
        self.shared.talk_key_held.store(held, Ordering::Relaxed);
    }

    pub fn set_echo_cancellation(&self, on: bool) {
        self.shared.audio.processor.set(Some(on), None, None);
    }

    pub fn set_noise_suppression(&self, mode: NoiseSuppression) {
        self.shared.audio.processor.set(None, Some(mode), None);
    }

    pub fn set_auto_gain(&self, on: bool) {
        self.shared.audio.processor.set(None, None, Some(on));
    }

    pub fn set_attenuation(&self, attenuation: Attenuation) {
        if let Ok(mut current) = self.shared.attenuation.lock() {
            *current = attenuation;
        }
    }

    // Discord's "No Audio Detected" state for this call: None until known.
    pub fn input_detected(&self) -> Option<bool> {
        detected_from(self.shared.input_detected.load(Ordering::Relaxed))
    }

    // True once the Advanced suppressor fell back to RNNoise because this CPU can't run it in real time.
    pub fn noise_suppression_degraded(&self) -> bool {
        self.shared.audio.processor.degraded()
    }

    // Camera id from `cameras()`; None turns the camera off. Our own frames come back as
    // `StreamFrame { source: Camera }` with our user id.
    pub fn set_camera(&self, id: Option<String>) -> Result<(), String> {
        if let Some(id) = &id
            && !cameras().iter().any(|c| &c.id == id)
        {
            return Err("That camera is not connected".into());
        }
        if let Ok(mut camera) = self.shared.camera.lock() {
            *camera = id;
        }
        self.shared.send_voice_state(true);
        Ok(())
    }

    // Watch `streamer_id`'s Go Live. Frames arrive as `StreamFrame { source: GoLive, .. }` on this
    // session's event stream, and `StreamEnded` when it stops. Needs `Connected` first.
    pub fn watch_stream(&self, streamer_id: &str) -> Result<StreamWatch, String> {
        let (stop, _) = self.start_stream(streamer_id, Role::Watch, None)?;
        Ok(StreamWatch { stop })
    }

    // The frame a `StreamFrame` announced; None if it was already taken.
    pub fn take_frame(&self, user_id: &str, source: VideoSource) -> Option<Arc<VideoFrame>> {
        self.shared.frames.lock().ok()?.remove(&(user_id.to_owned(), source))
    }

    // Share the main display (macOS) at 720p30 with sound. Our own `StreamEnded` reports when it stops.
    #[allow(dead_code, reason = "examples/voice_join.rs drives it; the app doesn't yet")]
    pub fn go_live(&self) -> Result<GoLive, String> {
        let main = screens()?.first().map(|s| s.id).ok_or("No display to share")?;
        self.go_live_with_options(ShareSource::Screen(main), GoLiveOptions::default(), None)
    }

    // Share one app or window, with only its audio; see `shareable_windows` for the choices.
    #[allow(dead_code, reason = "examples/voice_join.rs drives it; the app doesn't yet")]
    pub fn go_live_with(&self, source: ShareSource) -> Result<GoLive, String> {
        self.go_live_with_options(source, GoLiveOptions::default(), None)
    }

    // Our streams share one key, so a stream that `replaces` another (new quality, another window) waits
    // for Discord to delete the old one; otherwise that delete ends the new stream too.
    pub fn go_live_with_options(&self, source: ShareSource, options: GoLiveOptions, replaces: Option<GoLive>) -> Result<GoLive, String> {
        let after = replaces.map(|old| old.gone);
        let (stop, gone) = self.start_stream(&self.shared.user_id, Role::Broadcast(source, options), after)?;
        Ok(GoLive { stop, gone })
    }

    fn start_stream(&self, streamer_id: &str, role: Role, after: Option<oneshot::Receiver<()>>) -> Result<(oneshot::Sender<()>, oneshot::Receiver<()>), String> {
        let session_id = self.shared.session_id.lock().ok().and_then(|id| id.clone()).ok_or("Not connected to voice yet")?;
        let key = match &self.shared.guild_id {
            Some(guild) => format!("guild:{guild}:{}:{streamer_id}", self.shared.channel_id),
            None => format!("call:{}:{streamer_id}", self.shared.channel_id),
        };
        let (stop_tx, stop_rx) = oneshot::channel();
        let (gone_tx, gone_rx) = oneshot::channel();
        let ends = StreamEnds { stop: stop_rx, after, gone: gone_tx };
        crate::discord::runtime().spawn(stream(self.shared.clone(), role, streamer_id.to_owned(), key, session_id, ends));
        Ok((stop_tx, gone_rx))
    }

    #[allow(dead_code, reason = "examples/voice_join.rs drives it; the app doesn't yet")]
    pub fn leave(self) {}

    // Once dropped, this session ends without telling Discord we left: the next join's voice state moves us.
    pub fn supersede(&self) {
        self.shared.supersede();
    }
}

// Lowers other applications while someone speaks, until the session is gone.
fn duck_other_apps(shared: &Weak<Shared>) {
    let mut ducking = attenuation::Ducking::new();
    let mut ramp = attenuation::Ramp::default();
    let mut applied = 1.0;
    while let Some(target) = shared.upgrade().map(|s| s.attenuation_target()) {
        let factor = ramp.step(target, DUCK_TICK);
        if factor != applied {
            ducking.apply(factor);
            applied = factor;
        }
        thread::sleep(DUCK_TICK);
    }
}

fn detected_from(state: u8) -> Option<bool> {
    match state {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

// What the mic test reports about 25 times a second.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MicLevel {
    // dBFS after processing.
    pub db: f32,
    // Whether this would transmit (the voice-activity gate is open).
    pub speaking: bool,
    // Discord's "isn't detecting any input from your mic": None until known.
    pub input_detected: Option<bool>,
}

// Discord's "Let's Check": plays the processed mic back and reports its level. Drop to stop.
pub struct MicTest {
    stop: Arc<AtomicBool>,
    _engine: audio::Engine,
}

impl Drop for MicTest {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

// Works outside a call. `device` overrides `settings.input_device`.
pub fn mic_test(device: Option<String>, settings: &VoiceSettings) -> Result<(MicTest, mpsc::UnboundedReceiver<MicLevel>), String> {
    // Echo cancellation off: the only thing playing is our own voice looped back, and as an echo
    // reference it makes the canceller cut the live voice. With nobody on the far end there is no
    // echo to remove.
    let settings = VoiceSettings { input_device: device.or_else(|| settings.input_device.clone()), echo_cancellation: false, ..settings.clone() };
    let input_mode = settings.input_mode;
    let audio = Arc::new(settings.audio());
    let mixer = Arc::new(audio::Mixer::default());
    let (engine, pcm) = audio::start(audio.clone(), mixer.clone(), true)?;
    let (levels_tx, levels) = mpsc::unbounded();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    thread::Builder::new()
        .name("waffle-mic-test".into())
        .spawn(move || media::mic_test_loop(&audio, &mixer, &pcm, input_mode, &levels_tx, &flag))
        .map_err(|e| e.to_string())?;
    Ok((MicTest { stop, _engine: engine }, levels))
}

async fn run(shared: Arc<Shared>, leave: oneshot::Receiver<()>) {
    let mut dispatches = shared.gateway.dispatches.subscribe();
    shared.send_voice_state(true);
    shared.emit(VoiceEvent::Stage(Stage::AwaitingEndpoint));
    let reason = tokio::select! {
        result = join_voice(&shared, &mut dispatches) => result.err().unwrap_or_else(|| "Voice connection closed".to_owned()),
        _ = leave => "Left".to_owned(),
    };
    // A superseded session's call already lives on in the next one: a leave or a Disconnected
    // from it would end that call.
    if shared.superseded.load(Ordering::Acquire) {
        return;
    }
    // 4006 is how the voice server drops us when another client of ours joins; its voice state can
    // reach us after the close, so a 4006 doesn't send the leave either.
    let elsewhere = shared.taken_elsewhere.load(Ordering::Acquire);
    if !elsewhere && !reason.ends_with("(4006)") {
        shared.send_voice_state(false);
    }
    let reason = if elsewhere { ANOTHER_LOCATION.to_owned() } else { reason };
    shared.emit(VoiceEvent::Stage(Stage::Disconnected));
    shared.emit(VoiceEvent::Disconnected(reason));
}

async fn join_voice(shared: &Arc<Shared>, dispatches: &mut broadcast::Receiver<(String, Value)>) -> Result<(), String> {
    let (session_id, server) = tokio::time::timeout(SERVER_TIMEOUT, await_voice_server(shared, dispatches))
        .await
        .map_err(|_| "Voice server did not respond".to_owned())??;
    if let Ok(mut id) = shared.session_id.lock() {
        *id = Some(session_id.clone());
    }
    let mut target = Target {
        server_id: shared.server_id().to_owned(),
        dave_channel: shared.channel_id.clone(),
        session_id,
        server,
        role: Role::Voice,
        streamer: String::new(),
    };
    // Discord reallocates a call's voice server with a fresh VOICE_SERVER_UPDATE (docs: Voice, "Voice
    // Server Update"): the old connection is dropped and the same session identifies on the new endpoint.
    loop {
        let result = serve(shared, target.clone(), dispatches, |kind, d| voice_dispatch(shared, kind, d)).await;
        let mut moved = take_moved(shared);
        // The old server closes with 4022 when the call moves, possibly before the update naming the new
        // one arrives (Discord sends a null-endpoint update first). Not verified live: the order isn't documented.
        if moved.is_none() && result.as_ref().is_err_and(|reason| call_terminated(reason)) {
            moved = tokio::time::timeout(MOVE_TIMEOUT, await_move(shared, dispatches)).await.ok().flatten();
        }
        let Some(server) = moved else { return result };
        shared.emit(VoiceEvent::Stage(Stage::AwaitingEndpoint));
        target.server = server;
    }
}

fn take_moved(shared: &Shared) -> Option<Server> {
    shared.moved_server.lock().ok().and_then(|mut s| s.take())
}

// Follows dispatches as `serve` does until one ends the call; Some when that was a server move.
async fn await_move(shared: &Shared, dispatches: &mut broadcast::Receiver<(String, Value)>) -> Option<Server> {
    loop {
        let (kind, d) = next_dispatch(dispatches).await?;
        if voice_dispatch(shared, &kind, &d).is_some() {
            return take_moved(shared);
        }
    }
}

// Follows our channel's voice states while connected; ends the connection if we were moved out.
fn voice_dispatch(shared: &Shared, kind: &str, d: &Value) -> Option<String> {
    if kind == "VOICE_SERVER_UPDATE" {
        let for_us = d["guild_id"].as_str().or(d["channel_id"].as_str()) == Some(shared.server_id());
        // A null endpoint means another update follows with the new server; wait for that one.
        let server = parse_server(d).filter(|_| for_us)?;
        if let Ok(mut moved) = shared.moved_server.lock() {
            *moved = Some(server);
        }
        return Some("Voice server moved".into());
    }
    if kind != "VOICE_STATE_UPDATE" {
        return None;
    }
    let user_id = d["user_id"].as_str()?;
    let in_channel = d["channel_id"].as_str() == Some(&shared.channel_id);
    if user_id == shared.user_id && taken_elsewhere(shared.session_id.lock().ok().and_then(|id| id.clone()).as_deref(), d) {
        shared.taken_elsewhere.store(true, Ordering::Release);
        return Some(ANOTHER_LOCATION.into());
    }
    if user_id == shared.user_id && !in_channel {
        let session = shared.session_id.lock().ok().and_then(|id| id.clone());
        match ours_left(session.as_deref(), d) {
            Left::Not => return None,
            Left::Removed => return Some("Removed from the voice channel".into()),
            Left::MovedTo(channel) => {
                shared.supersede();
                shared.emit(VoiceEvent::Moved(channel.to_owned()));
                return Some("Moved".into());
            }
        }
    }
    let streaming = in_channel && d["self_stream"].as_bool().unwrap_or(false);
    let camera = in_channel && d["self_video"].as_bool().unwrap_or(false);
    shared.emit(VoiceEvent::VideoState { user_id: user_id.to_owned(), streaming, camera });
    None
}

// Why a call ended when another client of ours joined voice, any channel, and took it from this one.
pub const ANOTHER_LOCATION: &str = "Connected at another location";

// Our account in voice under another session: one of our other clients joined.
fn taken_elsewhere(our_session: Option<&str>, d: &Value) -> bool {
    let theirs = d["session_id"].as_str();
    our_session.zip(theirs).is_some_and(|(ours, theirs)| ours != theirs) && d["channel_id"].is_string()
}

enum Left<'a> {
    Not,
    Removed,
    MovedTo(&'a str),
}

// What a voice state of ours outside our channel means. Our own session in another channel is a move,
// and with no channel a disconnect. Another session of ours joining voice (another client) takes the
// call from us; another session leaving voice, or a stale one being cleaned up, says nothing about ours.
// With either session unknown, it's taken as a removal.
fn ours_left<'a>(our_session: Option<&str>, d: &'a Value) -> Left<'a> {
    let (theirs, channel) = (d["session_id"].as_str(), d["channel_id"].as_str());
    match (our_session.zip(theirs).map(|(a, b)| a == b), channel) {
        (Some(true), Some(channel)) => Left::MovedTo(channel),
        (Some(false), None) => Left::Not,
        _ => Left::Removed,
    }
}

#[derive(Clone)]
struct Server {
    token: String,
    endpoint: String,
}

#[derive(Clone)]
struct Target {
    server_id: String,
    // DAVE's MLS group id: the voice channel, or a Go Live's `rtc_channel_id`.
    dave_channel: String,
    session_id: String,
    server: Server,
    role: Role,
    // Whose Go Live a Watch connection carries; empty otherwise.
    streamer: String,
}

// STREAM_DELETE's reasons (Discord's client, 2026-11-21), plus `unavailable: true` as its own.
pub const STREAM_DELETE_UNAVAILABLE: &str = "unavailable";
pub const STREAM_DELETE_FULL: &str = "stream_full";
pub const STREAM_DELETE_UNAUTHORIZED: &str = "unauthorized";
pub const STREAM_DELETE_RATE_LIMITED: &str = "safety_guild_rate_limited";
pub const STREAM_DELETE_ENDED: &str = "stream_ended";

async fn await_voice_server(shared: &Shared, dispatches: &mut broadcast::Receiver<(String, Value)>) -> Result<(String, Server), String> {
    let (mut session_id, mut server) = (None, None);
    loop {
        let (kind, d) = next_dispatch(dispatches).await.ok_or("Gateway closed")?;
        let for_us = d["guild_id"].as_str().or(d["channel_id"].as_str()) == Some(shared.server_id());
        match kind.as_str() {
            "VOICE_STATE_UPDATE" if d["user_id"].as_str() == Some(&shared.user_id) && for_us => {
                session_id = d["session_id"].as_str().map(str::to_owned);
            }
            "VOICE_SERVER_UPDATE" if for_us => server = parse_server(&d).or(server),
            _ => {}
        }
        if let (Some(id), Some(server)) = (&session_id, &server) {
            return Ok((id.clone(), server.clone()));
        }
    }
}

// A null endpoint means the server is being reallocated; another update follows.
fn parse_server(d: &Value) -> Option<Server> {
    Some(Server { token: d["token"].as_str()?.to_owned(), endpoint: d["endpoint"].as_str()?.to_owned() })
}

// Main-gateway stream ops: 18 create (go live), 19 delete (stop streaming or watching),
// 20 watch, 22 set paused.
// How a stream task starts and stops: `after` holds it until the stream it replaces is gone, `stop` ends
// it, and `gone` tells whatever replaces it that Discord has deleted it.
struct StreamEnds {
    stop: oneshot::Receiver<()>,
    after: Option<oneshot::Receiver<()>>,
    gone: oneshot::Sender<()>,
}

async fn stream(shared: Arc<Shared>, role: Role, streamer: String, key: String, session_id: String, ends: StreamEnds) {
    let StreamEnds { stop, after, gone } = ends;
    if let Some(after) = after {
        let _ = after.await;
    }
    // Subscribed only now, so the replaced stream's STREAM_DELETE isn't taken for ours.
    let mut dispatches = shared.gateway.dispatches.subscribe();
    if role.broadcasts() {
        let kind = if shared.guild_id.is_some() { "guild" } else { "call" };
        let create = json!({ "type": kind, "guild_id": shared.guild_id, "channel_id": shared.channel_id, "preferred_region": null });
        shared.gateway.send(18, create);
    } else {
        shared.gateway.send(20, json!({ "stream_key": key }));
    }
    let (reason, stopped) = tokio::select! {
        result = connect_stream(&shared, role, &streamer, &key, session_id, &mut dispatches) => (result.err().unwrap_or_else(|| "Stream connection closed".to_owned()), false),
        _ = stop => ("Stopped".to_owned(), true),
    };
    shared.gateway.send(19, json!({ "stream_key": key }));
    // A stream that ended on its own was already deleted; one we stopped is gone once Discord says so.
    if stopped && role.broadcasts() {
        let _ = tokio::time::timeout(DELETE_TIMEOUT, deleted(&key, &mut dispatches)).await;
    }
    let _ = gone.send(());
    shared.emit(VoiceEvent::StreamEnded { user_id: streamer.clone(), reason });
    // After the end, so the app still sees what ended it.
    let ended: &[u16] = if role.broadcasts() { &issues::BROADCAST_ISSUES } else { &issues::WATCH_ISSUES };
    for code in ended {
        shared.issue(&streamer, *code, false);
    }
}

// The next VOICE_* / STREAM_* dispatch; None once the gateway handle is gone. A receiver that fell
// behind skips what it missed instead of failing the join or stream waiting on it.
async fn next_dispatch(dispatches: &mut broadcast::Receiver<(String, Value)>) -> Option<(String, Value)> {
    loop {
        match dispatches.recv().await {
            Ok(dispatch) => return Some(dispatch),
            Err(broadcast::error::RecvError::Lagged(missed)) => eprintln!("voice: missed {missed} gateway dispatches"),
            Err(broadcast::error::RecvError::Closed) => return None,
        }
    }
}

async fn deleted(key: &str, dispatches: &mut broadcast::Receiver<(String, Value)>) {
    while let Some((kind, d)) = next_dispatch(dispatches).await {
        if kind == "STREAM_DELETE" && d["stream_key"].as_str() == Some(key) {
            return;
        }
    }
}

async fn connect_stream(
    shared: &Arc<Shared>,
    role: Role,
    streamer: &str,
    key: &str,
    session_id: String,
    dispatches: &mut broadcast::Receiver<(String, Value)>,
) -> Result<(), String> {
    let (rtc, server) = tokio::time::timeout(SERVER_TIMEOUT, await_stream_server(key, dispatches))
        .await
        .map_err(|_| "Stream server did not respond".to_owned())??;
    if role.broadcasts() {
        shared.gateway.send(22, json!({ "stream_key": key, "paused": false }));
    }
    let target = Target { server_id: rtc.server_id, dave_channel: rtc.channel_id, session_id, server, role, streamer: streamer.to_owned() };
    serve(shared, target, dispatches, |kind, d| (kind == "STREAM_DELETE" && d["stream_key"].as_str() == Some(key)).then(|| delete_reason(d)))
    .await
}

fn delete_reason(d: &Value) -> String {
    match d["unavailable"].as_bool() {
        Some(true) => STREAM_DELETE_UNAVAILABLE.to_owned(),
        _ => d["reason"].as_str().unwrap_or(STREAM_DELETE_ENDED).to_owned(),
    }
}

#[derive(Clone)]
struct RtcIds {
    server_id: String,
    channel_id: String,
}

async fn await_stream_server(key: &str, dispatches: &mut broadcast::Receiver<(String, Value)>) -> Result<(RtcIds, Server), String> {
    let (mut rtc, mut server) = (None, None);
    loop {
        let (kind, d) = next_dispatch(dispatches).await.ok_or("Gateway closed")?;
        if d["stream_key"].as_str() != Some(key) {
            continue;
        }
        match kind.as_str() {
            "STREAM_CREATE" => {
                if let (Some(server_id), Some(channel_id)) = (d["rtc_server_id"].as_str(), d["rtc_channel_id"].as_str()) {
                    rtc = Some(RtcIds { server_id: server_id.to_owned(), channel_id: channel_id.to_owned() });
                }
            }
            "STREAM_SERVER_UPDATE" => server = parse_server(&d).or(server),
            "STREAM_DELETE" => return Err(d["reason"].as_str().unwrap_or("Stream unavailable").to_owned()),
            _ => {}
        }
        if let (Some(rtc), Some(server)) = (&rtc, &server) {
            return Ok((rtc.clone(), server.clone()));
        }
    }
}

// Round-trip times from heartbeat acks.
#[derive(Default)]
struct Pings(VecDeque<u32>);

impl Pings {
    fn record(&mut self, sent_ms: i64) {
        let rtt = (chrono::Utc::now().timestamp_millis() - sent_ms).clamp(0, u32::MAX as i64) as u32;
        self.0.push_back(rtt);
        if self.0.len() > PINGS_KEPT {
            self.0.pop_front();
        }
    }

    fn stats(&self) -> (u32, u32, Vec<u32>) {
        let last = self.0.back().copied().unwrap_or(0);
        (last, median(self.0.iter().copied()), self.0.iter().copied().collect())
    }
}

fn median(values: impl Iterator<Item = u32>) -> u32 {
    let mut sorted: Vec<u32> = values.collect();
    sorted.sort_unstable();
    match sorted.len() {
        0 => 0,
        n if n % 2 == 1 => sorted[n / 2],
        n => (sorted[n / 2 - 1] + sorted[n / 2]) / 2,
    }
}

// The server's outbound-loss reports over the last LOSS_WINDOW stats ticks, averaged.
#[derive(Default)]
struct LossWindow(VecDeque<f32>);

impl LossWindow {
    fn push(&mut self, report: Option<f32>) -> Option<f32> {
        if let Some(loss) = report {
            self.0.push_back(loss);
            if self.0.len() > LOSS_WINDOW {
                self.0.pop_front();
            }
        }
        (!self.0.is_empty()).then(|| self.0.iter().sum::<f32>() / self.0.len() as f32)
    }
}

// One voice-gateway connection: the channel's own (audio + cameras) or a Go Live stream's.
async fn serve(
    shared: &Arc<Shared>,
    target: Target,
    dispatches: &mut broadcast::Receiver<(String, Value)>,
    on_dispatch: impl Fn(&str, &Value) -> Option<String>,
) -> Result<(), String> {
    let is_voice = target.role == Role::Voice;
    let stage = |stage| {
        if is_voice {
            shared.emit(VoiceEvent::Stage(stage));
        }
    };
    let host = target.server.endpoint.trim_start_matches("wss://").trim_end_matches('/').to_owned();
    let url = format!("wss://{host}/?v=8");
    let (ws, _) = tokio_tungstenite::connect_async(&url).await.map_err(|e| e.to_string())?;
    let (mut sink, mut stream) = ws.split();
    let (out, mut out_rx) = mpsc::unbounded::<WsMessage>();

    let my_id: u64 = shared.user_id.parse().map_err(|_| "Invalid user id".to_owned())?;
    let channel: u64 = target.dave_channel.parse().map_err(|_| "Invalid channel id".to_owned())?;
    let mut dave = dave::Dave::new(my_id, channel, out.clone())?;
    let mut media: Option<media::Media> = None;
    let users = media::Users::default();
    let counters = Arc::new(media::Counters::default());
    let mut ready: Option<(Ssrcs, std::net::UdpSocket)> = None;
    let mut seq_ack: i64 = -1;
    let mut heartbeat = tokio::time::interval(Duration::from_secs(3600));
    heartbeat.tick().await;
    let mut stats_tick = tokio::time::interval(STATS_EVERY);
    let mut pings = Pings::default();
    let mut loss = LossWindow::default();
    let mut stats = Stats { endpoint: host.clone(), ..Stats::default() };
    let mut resumes = 0;

    let identify = json!({ "op": 0, "d": {
        "server_id": target.server_id,
        "user_id": shared.user_id,
        "session_id": target.session_id,
        "token": target.server.token,
        "video": true,
        "streams": [{ "type": if target.role.broadcasts() { "screen" } else { "video" }, "rid": "100", "quality": 100 }],
        "max_dave_protocol_version": davey::DAVE_PROTOCOL_VERSION,
    }});
    sink.send(WsMessage::Text(identify.to_string())).await.map_err(|e| e.to_string())?;
    stage(Stage::Authenticating);

    loop {
        let polled: Result<Option<WsMessage>, String> = tokio::select! {
            _ = heartbeat.tick() => {
                let nonce = chrono::Utc::now().timestamp_millis();
                let beat = json!({ "op": 3, "d": { "t": nonce, "seq_ack": seq_ack } });
                sink.send(WsMessage::Text(beat.to_string())).await.map(|()| None).map_err(|e| e.to_string())
            }
            _ = stats_tick.tick(), if is_voice && media.is_some() => {
                (stats.ping_ms, stats.median_ping_ms, stats.last_pings) = pings.stats();
                stats.outbound_loss_pct = loss.push(counters.outbound_loss());
                stats.dave = dave.e2ee().load(Ordering::Relaxed);
                let playout = shared.mixer.playout_stats();
                stats.playout_delay_ms = playout.iter().map(|(_, p)| p.target_ms).max();
                if debug() {
                    for (key, p) in &playout {
                        eprintln!(
                            "[playout {key:x}] target {} ms, buffered {} ms, underruns {}, concealed {}, fec {}, late {}, dropped {}",
                            p.target_ms, p.buffered_ms, p.underruns, p.concealed, p.fec, p.late, p.dropped
                        );
                    }
                }
                shared.emit(VoiceEvent::Stats(stats.clone()));
                Ok(None)
            }
            Some(msg) = out_rx.next() => sink.send(msg).await.map(|()| None).map_err(|e| e.to_string()),
            Some((kind, d)) = next_dispatch(dispatches) => match on_dispatch(&kind, &d) {
                Some(reason) => return Err(reason),
                None => Ok(None),
            },
            frame = stream.next() => match frame {
                None => Err("Voice connection lost".to_owned()),
                Some(Err(e)) => Err(e.to_string()),
                Some(Ok(WsMessage::Close(close))) if !resumable(close.as_ref()) => return Err(close_reason(close.as_ref())),
                Some(Ok(WsMessage::Close(close))) => Err(close_reason(close.as_ref())),
                Some(Ok(frame)) => Ok(Some(frame)),
            },
        };
        let frame = match polled {
            Ok(Some(frame)) => frame,
            Ok(None) => continue,
            // Before media is up there is no session worth resuming: the join fails as it always did.
            Err(lost) if media.is_none() => return Err(lost),
            Err(lost) => {
                stage(Stage::Authenticating);
                let resume = WsMessage::Text(resume_payload(&target, seq_ack).to_string());
                (sink, stream) = loop {
                    if resumes >= RESUME_TRIES {
                        return Err(lost);
                    }
                    resumes += 1;
                    tokio::time::sleep(RESUME_BACKOFF * resumes).await;
                    let Ok((mut ws, _)) = tokio_tungstenite::connect_async(&url).await else { continue };
                    if ws.send(resume.clone()).await.is_ok() {
                        break ws.split();
                    }
                };
                continue;
            }
        };

        let payload = match frame {
            WsMessage::Binary(bytes) => {
                let Some((seq, op, body)) = dave::parse_binary(&bytes) else { continue };
                if debug() {
                    eprintln!("[{:?}] voice binary op {op} ({} bytes)", target.role, body.len());
                }
                seq_ack = seq as i64;
                dave.handle_binary(op, body);
                continue;
            }
            WsMessage::Text(text) => serde_json::from_str::<Value>(&text).map_err(|e| e.to_string())?,
            _ => continue,
        };
        if let Some(seq) = payload["seq"].as_i64() {
            seq_ack = seq;
        }
        let d = &payload["d"];
        if debug() && payload["op"] != 6 {
            eprintln!("[{:?}] voice op {}: {:.300}", target.role, payload["op"], d.to_string());
        }
        match payload["op"].as_u64() {
            Some(8) => {
                let ms = d["heartbeat_interval"].as_f64().unwrap_or(13_750.0) as u64;
                heartbeat = tokio::time::interval(Duration::from_millis(ms));
                heartbeat.tick().await;
            }
            Some(9) => {
                resumes = 0;
                if media.is_some() {
                    stage(Stage::Connected);
                }
            }
            Some(6) => {
                if let Some(sent) = d["t"].as_i64() {
                    pings.record(sent);
                    // Discord rates a connection bad past 500 ms or 10% loss of what it sends (2006).
                    if matches!(target.role, Role::Broadcast(..)) {
                        let ping = pings.0.back().copied().unwrap_or(0);
                        let bad = ping > 500 || counters.outbound_loss().is_some_and(|loss| loss > issues::LOSS_LINE);
                        shared.issue(&shared.user_id, issues::STREAM_SEND_NETWORK_QUALITY, bad);
                    }
                    if debug() {
                        eprintln!("[{:?}] heartbeat ack after {} ms", target.role, pings.0.back().copied().unwrap_or(0));
                    }
                }
            }
            Some(2) => {
                stage(Stage::RtcConnecting);
                let ssrc = d["ssrc"].as_u64().ok_or("Voice ready without ssrc")? as u32;
                let ip = d["ip"].as_str().ok_or("Voice ready without ip")?.to_owned();
                let port = d["port"].as_u64().ok_or("Voice ready without port")? as u16;
                stats.server_ip = format!("{ip}:{port}");
                let modes: Vec<&str> = d["modes"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                let mode = rtp::MODES.into_iter().find(|m| modes.contains(m)).ok_or("No supported encryption mode")?;
                stage(Stage::IceChecking);
                let discovered = tokio::task::spawn_blocking(move || media::discover(&ip, port, ssrc)).await.map_err(|e| e.to_string())?;
                let (socket, (my_ip, my_port)) = discovered.inspect_err(|_| stage(Stage::NoRoute))?;
                let video = &d["streams"][0];
                let ssrcs = Ssrcs {
                    audio: ssrc,
                    video: video["ssrc"].as_u64().unwrap_or(0) as u32,
                    rtx: video["rtx_ssrc"].as_u64().unwrap_or(0) as u32,
                };
                ready = Some((ssrcs, socket));
                sink.send(WsMessage::Text(select_protocol(&my_ip, my_port, mode).to_string())).await.map_err(|e| e.to_string())?;
                stage(Stage::DtlsConnecting);
            }
            Some(4) => {
                let (ssrcs, socket) = ready.take().ok_or("Session description before ready")?;
                let key: Vec<u8> = d["secret_key"].as_array().into_iter().flatten().filter_map(|b| b.as_u64().map(|b| b as u8)).collect();
                let mode = d["mode"].as_str().unwrap_or_default();
                stats.encryption = mode.to_owned();
                let cipher = rtp::Cipher::new(mode, &key).ok_or("Server picked an unsupported mode")?;
                dave.set_protocol_version(d["dave_protocol_version"].as_u64().unwrap_or(0) as u16);
                if let Role::Broadcast(_, options) = target.role {
                    sink.send(WsMessage::Text(announce_video(&ssrcs, VideoKind::Screen(options), true).to_string())).await.map_err(|e| e.to_string())?;
                }
                media = Some(media::Media::start(
                    media::Link {
                        socket,
                        ssrc: ssrcs.audio,
                        video_ssrc: ssrcs.video,
                        rtx_ssrc: ssrcs.rtx,
                        cipher,
                        dave: dave.session(),
                        e2ee: dave.e2ee(),
                        shared: shared.clone(),
                        role: target.role,
                        my_id: shared.user_id.clone(),
                        events: shared.events.clone(),
                        ws: out.clone(),
                        nonce: Default::default(),
                        keyframe: AtomicBool::new(true),
                        counters: counters.clone(),
                        connection: media::connection_id(),
                    },
                    users.clone(),
                )?);
                // Not verified: op 15 (media sink wants) asks the SFU to forward video at full quality.
                sink.send(WsMessage::Text(json!({ "op": 15, "d": { "any": 100 } }).to_string())).await.map_err(|e| e.to_string())?;
                if is_voice {
                    stage(Stage::Connected);
                    shared.emit(VoiceEvent::Connected);
                } else if target.role == Role::Watch {
                    shared.emit(VoiceEvent::StreamConnected { user_id: target.streamer.clone() });
                }
            }
            Some(5) => {
                if let (Some(user), Some(ssrc)) = (d["user_id"].as_str(), d["ssrc"].as_u64()) {
                    media::map_ssrc(&users, ssrc as u32, user);
                }
            }
            Some(12) => {
                if let Some(user) = d["user_id"].as_str() {
                    video_ssrcs(d).for_each(|ssrc| media::map_ssrc(&users, ssrc, user));
                }
            }
            Some(11) => dave.users_joined(d["user_ids"].as_array().into_iter().flatten().filter_map(Value::as_str)),
            Some(13) => {
                if let Some(user) = d["user_id"].as_str() {
                    dave.user_left(user);
                    if let Some(media) = &media {
                        media.user_left(user);
                    }
                }
            }
            Some(op @ (21 | 22 | 24)) => dave.handle_json(op, d),
            _ => {}
        }
    }
}

fn resume_payload(target: &Target, seq_ack: i64) -> Value {
    json!({ "op": 7, "d": {
        "server_id": target.server_id,
        "session_id": target.session_id,
        "token": target.server.token,
        "seq_ack": seq_ack,
    }})
}

// Discord's voice close codes (docs.discord.com, Opcodes and Status Codes; 4013 from docs.discord.food):
// whether a resume is worth trying, and the reason the voice panel shows. Docs: don't reconnect after
// 4014, 4021, 4022; try resuming after 4013 and 4015. The rest describe a session or request a resume
// can't repair, so they end the call too (the user can rejoin). Unlisted codes and a reset without a
// close frame resume.
fn voice_close(code: u16) -> (bool, Option<&'static str>) {
    match code {
        4001 => (false, Some("Unknown opcode")),
        4002 => (false, Some("Failed to decode payload")),
        4003 => (false, Some("Not authenticated")),
        4004 => (false, Some("Authentication failed")),
        4005 => (false, Some("Already authenticated")),
        4006 => (false, Some("Session no longer valid")),
        4009 => (false, Some("Session timed out")),
        4011 => (false, Some("Voice server not found")),
        4012 => (false, Some("Unknown protocol")),
        4013 => (true, Some("WebRTC crashed")),
        4014 => (false, Some("Disconnected by Discord")),
        4015 => (true, Some("Voice server crashed")),
        4016 => (false, Some("Unknown encryption mode")),
        4017 => (false, Some("End-to-end encryption required")),
        4020 => (false, Some("Bad request")),
        4021 => (false, Some("Rate limited by Discord")),
        4022 => (false, Some("The call ended")),
        _ => (true, None),
    }
}

fn call_terminated(reason: &str) -> bool {
    reason.ends_with("(4022)")
}

fn resumable(close: Option<&tungstenite::protocol::CloseFrame>) -> bool {
    close.is_none_or(|c| voice_close(u16::from(c.code)).0)
}

fn close_reason(close: Option<&tungstenite::protocol::CloseFrame>) -> String {
    match close {
        None => "Voice connection lost".to_owned(),
        Some(c) => {
            let code = u16::from(c.code);
            let reason = voice_close(code).1.map(str::to_owned).unwrap_or_else(|| if c.reason.is_empty() { "Voice connection lost".to_owned() } else { c.reason.to_string() });
            format!("{reason} ({code})")
        }
    }
}

// WAFFLE_VOICE_DEBUG=1 logs voice-gateway traffic and unroutable UDP packets to stderr.
pub(crate) fn debug() -> bool {
    static DEBUG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DEBUG.get_or_init(|| std::env::var_os("WAFFLE_VOICE_DEBUG").is_some())
}

struct Ssrcs {
    audio: u32,
    video: u32,
    rtx: u32,
}

// What our outgoing video is, for op 12.
#[derive(Clone, Copy)]
enum VideoKind {
    Camera,
    Screen(GoLiveOptions),
}

// Op 12 tells the SFU and viewers which SSRC carries our video and its limits. Shape follows
// what official clients send — Not verified.
fn announce_video(ssrcs: &Ssrcs, kind: VideoKind, active: bool) -> Value {
    let (kind, (width, height), fps) = match kind {
        VideoKind::Camera => ("video", media::CAMERA_SIZE, media::CAMERA_FPS),
        VideoKind::Screen(options) => ("screen", media::go_live_size(options.resolution), options.fps),
    };
    json!({ "op": 12, "d": {
        "audio_ssrc": ssrcs.audio,
        "video_ssrc": if active { ssrcs.video } else { 0 },
        "rtx_ssrc": if active { ssrcs.rtx } else { 0 },
        "streams": [{
            "type": kind, "rid": "100", "ssrc": ssrcs.video, "rtx_ssrc": ssrcs.rtx, "active": active, "quality": 100,
            "max_bitrate": media::video_bitrate(width, height, fps), "max_framerate": fps,
            "max_resolution": { "type": "fixed", "width": width, "height": height },
        }],
    }})
}

// We decode only H.264, so it is the only video codec offered.
fn select_protocol(address: &str, port: u16, mode: &str) -> Value {
    json!({ "op": 1, "d": {
        "protocol": "udp",
        "data": { "address": address, "port": port, "mode": mode },
        "codecs": [
            { "name": "opus", "type": "audio", "priority": 1000, "payload_type": rtp::OPUS_PAYLOAD_TYPE },
            { "name": "H264", "type": "video", "priority": 1000, "payload_type": rtp::H264_PAYLOAD_TYPE,
              "rtx_payload_type": rtp::H264_RTX_PAYLOAD_TYPE, "encode": true, "decode": true },
        ],
    }})
}

// Op 12 names a user's audio, video, retransmission and per-stream SSRCs; 0 means "none".
fn video_ssrcs(d: &Value) -> impl Iterator<Item = u32> + '_ {
    let streams = d["streams"].as_array().into_iter().flatten().flat_map(|s| [&s["ssrc"], &s["rtx_ssrc"]]);
    [&d["audio_ssrc"], &d["video_ssrc"], &d["rtx_ssrc"]]
        .into_iter()
        .chain(streams)
        .filter_map(Value::as_u64)
        .filter(|&ssrc| ssrc != 0)
        .map(|ssrc| ssrc as u32)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_stopped_stream_is_gone_on_its_own_delete_only() {
        use super::{Value, broadcast, deleted};
        use serde_json::json;
        let runtime = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        let (tx, mut rx) = broadcast::channel::<(String, Value)>(8);
        let waits = |rx: &mut broadcast::Receiver<(String, Value)>| runtime.block_on(async { tokio::time::timeout(std::time::Duration::from_millis(50), deleted("k", rx)).await }).is_ok();
        tx.send(("STREAM_DELETE".into(), json!({ "stream_key": "other" }))).unwrap();
        tx.send(("STREAM_CREATE".into(), json!({ "stream_key": "k" }))).unwrap();
        assert!(!waits(&mut rx), "another stream's delete or our create isn't the delete");
        tx.send(("STREAM_DELETE".into(), json!({ "stream_key": "k" }))).unwrap();
        assert!(waits(&mut rx));
    }

    use super::*;

    #[test]
    fn only_our_session_or_another_joining_ends_the_call() {
        use serde_json::json;
        let state = |session: &str, channel: Option<&str>| json!({ "session_id": session, "channel_id": channel });
        assert!(matches!(ours_left(Some("me"), &state("me", Some("c2"))), Left::MovedTo("c2")));
        assert!(matches!(ours_left(Some("me"), &state("me", None)), Left::Removed));
        assert!(matches!(ours_left(Some("me"), &state("phone", Some("c2"))), Left::Removed), "another client joined voice");
        assert!(matches!(ours_left(Some("me"), &state("phone", None)), Left::Not), "another client leaving isn't us");
    }

    #[test]
    fn median_ping_and_loss_window() {
        assert_eq!(median([40, 300, 45, 50].into_iter()), 47, "two middles average; one spike doesn't move it");
        assert_eq!(median([40, 300, 45].into_iter()), 45);
        assert_eq!(median(std::iter::empty()), 0);
        let mut window = LossWindow::default();
        assert_eq!(window.push(None), None, "unknown until the server reports");
        assert_eq!(window.push(Some(4.0)), Some(4.0));
        assert_eq!(window.push(None), Some(4.0), "a tick without a new report keeps the window");
        for _ in 0..LOSS_WINDOW {
            window.push(Some(0.0));
        }
        assert_eq!(window.push(Some(0.0)), Some(0.0), "old reports fall out after the window");
    }

    #[test]
    fn an_issue_is_told_once_when_it_starts_and_once_when_it_clears() {
        let (shared, mut rx) = shared();
        shared.issue("me", issues::STREAM_SEND_LOW_FPS, true);
        shared.issue("me", issues::STREAM_SEND_LOW_FPS, true);
        shared.issue("me", issues::STREAM_SEND_LOW_FPS, false);
        shared.issue("me", issues::STREAM_SEND_LOW_FPS, false);
        let told: Vec<bool> = std::iter::from_fn(|| rx.try_recv().ok())
            .filter_map(|e| match e {
                VoiceEvent::Issue { active, .. } => Some(active),
                _ => None,
            })
            .collect();
        assert_eq!(told, [true, false]);
    }

    fn shared() -> (Shared, mpsc::UnboundedReceiver<VoiceEvent>) {
        let (gateway, _out) = GatewayHandle::new();
        let (events, rx) = mpsc::unbounded();
        let shared = Shared {
            gateway,
            user_id: "me".into(),
            guild_id: Some("g".into()),
            channel_id: "c".into(),
            events,
            issues: Mutex::default(),
            session_id: Mutex::new(None),
            moved_server: Mutex::new(None),
            muted: AtomicBool::new(false),
            deafened: AtomicBool::new(false),
            flags_pending: AtomicBool::new(false),
            flags_sent: Mutex::new(None),
            talk_key_held: AtomicBool::new(false),
            input_mode: Mutex::new(VoiceSettings::default().input_mode),
            audio: Arc::new(VoiceSettings::default().audio()),
            mixer: Arc::default(),
            listening: Mutex::new(HashMap::new()),
            stream_volume: Mutex::new(HashMap::new()),
            camera: Mutex::new(None),
            attenuation: Mutex::new(Attenuation { strength: 40, ..Attenuation::default() }),
            qos: false,
            self_speaking: AtomicBool::new(false),
            others_speaking: AtomicBool::new(false),
            input_detected: AtomicU8::new(0),
            superseded: AtomicBool::new(false),
            taken_elsewhere: AtomicBool::new(false),
            frames: Mutex::default(),
        };
        (shared, rx)
    }

    #[test]
    fn only_final_close_codes_skip_resume() {
        use tungstenite::protocol::{CloseFrame, frame::coding::CloseCode};
        let close = |code: u16| CloseFrame { code: CloseCode::from(code), reason: "".into() };
        assert!(resumable(None), "a reset without a close frame resumes");
        for code in [1000, 1001, 1006, 4013, 4015] {
            assert!(resumable(Some(&close(code))), "{code} resumes");
        }
        for code in [4001, 4002, 4003, 4004, 4005, 4006, 4009, 4011, 4012, 4014, 4016, 4017, 4020, 4021, 4022] {
            assert!(!resumable(Some(&close(code))), "{code} ends the call");
        }
        assert_eq!(close_reason(Some(&close(4014))), "Disconnected by Discord (4014)");
        assert_eq!(close_reason(Some(&close(4022))), "The call ended (4022)");
        assert!(call_terminated(&close_reason(Some(&close(4022)))));
        assert!(!call_terminated(&close_reason(Some(&close(4014)))));
        assert_eq!(close_reason(Some(&CloseFrame { code: CloseCode::from(4999), reason: "Odd".into() })), "Odd (4999)");
        assert_eq!(close_reason(None), "Voice connection lost");
    }

    #[test]
    fn op12_ssrcs_skip_zero() {
        let d = json!({ "user_id": "1", "audio_ssrc": 5, "video_ssrc": 0, "streams": [{ "ssrc": 6, "rtx_ssrc": 7 }, { "ssrc": 0 }] });
        assert_eq!(video_ssrcs(&d).collect::<Vec<_>>(), [5, 6, 7]);
    }

    #[test]
    fn voice_state_tracks_video_and_removal() {
        let (shared, mut rx) = shared();
        assert_eq!(voice_dispatch(&shared, "VOICE_STATE_UPDATE", &json!({ "user_id": "u", "channel_id": "c", "self_stream": true })), None);
        let Ok(VoiceEvent::VideoState { user_id, streaming: true, camera: false }) = rx.try_recv() else { panic!() };
        assert_eq!(user_id, "u");
        assert!(voice_dispatch(&shared, "VOICE_STATE_UPDATE", &json!({ "user_id": "me", "channel_id": null })).is_some());
    }

    #[test]
    fn server_update_mid_call_moves_only_ours_with_an_endpoint() {
        let (shared, _rx) = shared();
        let update = |guild: &str, endpoint: Value| json!({ "guild_id": guild, "token": "t", "endpoint": endpoint });
        assert_eq!(voice_dispatch(&shared, "VOICE_SERVER_UPDATE", &update("g", Value::Null)), None, "null endpoint waits");
        assert_eq!(voice_dispatch(&shared, "VOICE_SERVER_UPDATE", &update("other", json!("b.discord.media"))), None);
        assert!(take_moved(&shared).is_none());
        assert!(voice_dispatch(&shared, "VOICE_SERVER_UPDATE", &update("g", json!("b.discord.media"))).is_some());
        assert_eq!(take_moved(&shared).map(|s| s.endpoint).as_deref(), Some("b.discord.media"));
    }

    #[test]
    fn closed_call_waits_for_the_move_in_the_backlog() {
        let (shared, _rx) = shared();
        let mut dispatches = shared.gateway.dispatches.subscribe();
        shared.gateway.dispatch("VOICE_SERVER_UPDATE", &json!({ "guild_id": "g", "token": "t", "endpoint": "b.discord.media" }));
        let runtime = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        let moved = runtime.block_on(await_move(&shared, &mut dispatches));
        assert_eq!(moved.map(|s| s.endpoint).as_deref(), Some("b.discord.media"));
    }

    #[test]
    fn leave_outside_a_session_clears_our_channel() {
        let (gateway, mut out) = GatewayHandle::new();
        gateway.leave_voice(Some("g"), true, false);
        let sent: Value = serde_json::from_str(&out.try_recv().unwrap()).unwrap();
        assert_eq!(sent, json!({ "op": 4, "d": { "guild_id": "g", "channel_id": null, "self_mute": true, "self_deaf": false, "self_video": false } }));
    }

    #[test]
    fn moved_session_follows_without_leaving() {
        let (shared, mut rx) = shared();
        *shared.session_id.lock().unwrap() = Some("s".into());
        assert_eq!(voice_dispatch(&shared, "VOICE_STATE_UPDATE", &json!({ "user_id": "me", "session_id": "other", "channel_id": "d" })).as_deref(), Some(ANOTHER_LOCATION));
        assert!(!shared.superseded.load(Ordering::Acquire), "another client's join is not a move");
        assert!(shared.taken_elsewhere.load(Ordering::Acquire), "another client's join must not send the leave");
        shared.taken_elsewhere.store(false, Ordering::Release);
        assert!(voice_dispatch(&shared, "VOICE_STATE_UPDATE", &json!({ "user_id": "me", "session_id": "other", "channel_id": null })).is_none(), "another client leaving says nothing about ours");
        assert!(voice_dispatch(&shared, "VOICE_STATE_UPDATE", &json!({ "user_id": "me", "session_id": "s", "channel_id": "d" })).is_some());
        assert!(shared.superseded.load(Ordering::Acquire), "a move must not send the leave");
        let Ok(VoiceEvent::Moved(channel)) = rx.try_recv() else { panic!() };
        assert_eq!(channel, "d");
    }

    #[test]
    fn listening_gain_follows_volume_mute_and_deafen() {
        let (shared, _rx) = shared();
        assert_eq!(shared.listening_gain("u"), 1.0);
        shared.listening.lock().unwrap().insert("u".into(), (1.5, false));
        assert_eq!(shared.listening_gain("u"), 1.5);
        shared.listening.lock().unwrap().insert("u".into(), (1.5, true));
        assert_eq!(shared.listening_gain("u"), 0.0, "locally muted");
        shared.deafened.store(true, Ordering::Relaxed);
        assert_eq!(shared.listening_gain("v"), 0.0, "deafened hears no one");
    }

    #[test]
    fn attenuation_follows_who_is_speaking() {
        let (shared, _rx) = shared();
        assert_eq!(shared.attenuation_target(), 1.0);
        shared.others_speaking.store(true, Ordering::Relaxed);
        assert!((shared.attenuation_target() - 0.6).abs() < 1e-6, "40% while others speak");
        shared.others_speaking.store(false, Ordering::Relaxed);
        shared.self_speaking.store(true, Ordering::Relaxed);
        assert_eq!(shared.attenuation_target(), 1.0, "not while only I speak, by default");
        assert_eq!(detected_from(shared.input_detected.load(Ordering::Relaxed)), None);
    }

    #[test]
    fn pings_median_and_cap() {
        let mut pings = Pings::default();
        let now = chrono::Utc::now().timestamp_millis();
        (0..25).for_each(|_| pings.record(now - 40));
        let (last, avg, all) = pings.stats();
        assert!((40..60).contains(&last) && (40..60).contains(&avg), "{last} {avg}");
        assert_eq!(all.len(), PINGS_KEPT);
    }
}
