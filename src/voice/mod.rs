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
mod transport;
use transport::*;
#[allow(unused_imports)] // the app reads these; the voice example that includes this module does not
pub use transport::{ANOTHER_LOCATION, NEW_SESSION, STREAM_DELETE_ENDED, STREAM_DELETE_FULL, STREAM_DELETE_RATE_LIMITED, STREAM_DELETE_UNAUTHORIZED, STREAM_DELETE_UNAVAILABLE};
pub(crate) use transport::debug;
#[cfg(test)]
mod echo_check;
#[cfg(test)]
mod chain_check;
#[cfg(test)]
mod fixtures;
mod jitter;
mod reconnect;
mod wants;
pub use wants::VideoWants;
use reconnect::{Backoff, Reconnect, close_label, reconnect};
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
const HELLO_TIMEOUT: Duration = Duration::from_secs(20);
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
        // READY carries every guild; voice needs only that a fresh session began.
        if kind == "READY" {
            let _ = self.dispatches.send((kind.to_owned(), json!({ "session_id": d["session_id"] })));
            return;
        }
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
    // Server-muted, or a stage listener (suppressed): Discord's engine stops sending as for a self-mute.
    server_muted: AtomicBool,
    // What video we ask the SFU for (op 15), and a wake-up for the connection to send it again.
    video_wants: Mutex<VideoWants>,
    wants_changed: tokio::sync::Notify,
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

// A volume slider's value (0..=2) as the gain it plays at, on Discord's loudness curve (see `audio`).
pub fn volume_gain(volume: f32) -> f32 {
    audio::volume_gain(volume)
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
        ![&self.muted, &self.deafened, &self.server_muted].iter().any(|flag| flag.load(Ordering::Relaxed))
    }

    fn input_mode(&self) -> InputMode {
        self.input_mode.lock().map(|m| *m).unwrap_or(DEFAULT_INPUT_MODE)
    }

    // The volume factor for other applications right now.
    fn attenuation_target(&self) -> f32 {
        let attenuation = self.attenuation.lock().map(|a| *a).unwrap_or_default();
        attenuation.target(self.self_speaking.load(Ordering::Relaxed), self.others_speaking.load(Ordering::Relaxed))
    }

    // The gain to play `user_id`'s voice at: their local volume, 0 when locally muted or deafened.
    fn listening_gain(&self, user_id: &str) -> f32 {
        if self.deafened.load(Ordering::Relaxed) {
            return 0.0;
        }
        self.local_gain(user_id)
    }

    // `user_id`'s local volume, 0 when locally muted; deafen isn't applied here.
    fn local_gain(&self, user_id: &str) -> f32 {
        let local = self.listening.lock().ok().and_then(|l| l.get(user_id).copied());
        match local {
            Some((_, true)) => 0.0,
            Some((volume, false)) => volume,
            None => 1.0,
        }
    }

    // A watched Go Live plays at the streamer's local volume times its own stream volume. Deafen
    // silences the call's voices, not a stream the user chose to watch.
    fn stream_gain(&self, user_id: &str) -> f32 {
        self.local_gain(user_id) * self.stream_volume.lock().ok().and_then(|v| v.get(user_id).copied()).unwrap_or(1.0)
    }

    fn server_id(&self) -> &str {
        self.guild_id.as_deref().unwrap_or(&self.channel_id)
    }

    fn stream_key(&self, streamer_id: &str) -> String {
        match &self.guild_id {
            Some(guild) => format!("guild:{guild}:{}:{streamer_id}", self.channel_id),
            None => format!("call:{}:{streamer_id}", self.channel_id),
        }
    }

    // Our Go Live paused (its app out of sight) or resumed: viewers see "Stream Paused" meanwhile.
    fn set_stream_paused(&self, paused: bool) {
        self.gateway.send(22, json!({ "stream_key": self.stream_key(&self.user_id), "paused": paused }));
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
        server_muted: AtomicBool::new(false),
        video_wants: Mutex::default(),
        wants_changed: tokio::sync::Notify::new(),
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

    // Asks the SFU for this video from now on; only a change goes out.
    pub fn set_video_wants(&self, wants: VideoWants) {
        let Ok(mut current) = self.shared.video_wants.lock() else { return };
        if *current != wants {
            *current = wants;
            self.shared.wants_changed.notify_one();
        }
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
            volumes.insert(user_id.to_owned(), volume_gain(volume));
        }
    }

    pub fn set_user_volume(&self, user_id: &str, volume: f32) {
        if let Ok(mut listening) = self.shared.listening.lock() {
            listening.entry(user_id.to_owned()).or_insert((1.0, false)).0 = volume_gain(volume);
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
        let key = self.shared.stream_key(streamer_id);
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
        assert!(matches!(ours_left(Some("me"), &state("phone", Some("c2"))), Left::Not), "another client in voice elsewhere isn't us");
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
            server_muted: AtomicBool::new(false),
            video_wants: Mutex::default(),
            wants_changed: tokio::sync::Notify::new(),
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
    fn close_reason_names_the_code() {
        use tungstenite::protocol::{CloseFrame, frame::coding::CloseCode};
        let close = |code: u16, reason: &str| CloseFrame { code: CloseCode::from(code), reason: reason.to_owned().into() };
        assert_eq!(close_reason(Some(&close(4014, ""))), "Disconnected by Discord (4014)");
        assert_eq!(close_reason(Some(&close(4999, "Odd"))), "Odd (4999)");
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
    fn a_lost_call_follows_the_move_while_it_waits() {
        let (shared, _rx) = shared();
        let mut dispatches = shared.gateway.dispatches.subscribe();
        let runtime = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        let wait = |dispatches: &mut broadcast::Receiver<(String, Value)>| runtime.block_on(wait_out(Duration::from_millis(20), dispatches, |kind, d| voice_dispatch(&shared, kind, d)));
        assert_eq!(wait(&mut dispatches), None, "nothing happened: try again");
        shared.gateway.dispatch("VOICE_SERVER_UPDATE", &json!({ "guild_id": "g", "token": "t", "endpoint": "b.discord.media" }));
        assert!(wait(&mut dispatches).is_some());
        assert_eq!(take_moved(&shared).map(|s| s.endpoint).as_deref(), Some("b.discord.media"));
    }

    #[test]
    fn a_fresh_gateway_session_rejoins_voice_and_ends_streams() {
        let (shared, _rx) = shared();
        *shared.session_id.lock().unwrap() = Some("s".into());
        assert_eq!(voice_dispatch(&shared, "READY", &json!({ "session_id": "s2" })).as_deref(), Some(NEW_SESSION));
        assert!(shared.session_id.lock().unwrap().is_none(), "the voice state goes out again under the new session");
        let moved = Mutex::new(None);
        assert_eq!(stream_dispatch("k", "READY", &json!({}), &moved).as_deref(), Some(NEW_SESSION));
        assert_eq!(stream_dispatch("k", "STREAM_SERVER_UPDATE", &json!({ "stream_key": "other", "token": "t", "endpoint": "e" }), &moved), None);
        assert!(stream_dispatch("k", "STREAM_SERVER_UPDATE", &json!({ "stream_key": "k", "token": "t", "endpoint": "e" }), &moved).is_some());
        assert_eq!(moved.lock().unwrap().take().map(|s| s.endpoint).as_deref(), Some("e"), "a new stream server reconnects there");
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
        assert!(voice_dispatch(&shared, "VOICE_STATE_UPDATE", &json!({ "user_id": "me", "session_id": "other", "guild_id": "g2", "channel_id": "x" })).is_none(), "another client in another server keeps its own call");
        assert!(!shared.taken_elsewhere.load(Ordering::Acquire));
        assert_eq!(voice_dispatch(&shared, "VOICE_STATE_UPDATE", &json!({ "user_id": "me", "session_id": "other", "guild_id": "g", "channel_id": "d" })).as_deref(), Some(ANOTHER_LOCATION));
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
        assert_eq!(shared.stream_gain("v"), 1.0, "a watched stream keeps playing while deafened");
        assert_eq!(shared.stream_gain("u"), 0.0, "a locally muted streamer's stream stays muted");
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
