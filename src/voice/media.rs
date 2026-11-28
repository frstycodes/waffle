// Media over UDP: mic → processing → Opus → DAVE → AEAD → socket and the reverse into the mixer;
// H.264 video received and decoded into frames; our camera and Go Live sent as H.264. Runs on
// plain threads: cpal, Opus, OpenH264, capture callbacks and blocking UDP are all synchronous.

use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    io::ErrorKind,
    net::UdpSocket,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering},
        mpsc::{self as std_mpsc, Receiver, RecvTimeoutError, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

use ::rtp::{
    codecs::h264::H264Payloader,
    header::Header,
    packet::Packet,
    packetizer::Payloader,
};
use bytes::Bytes;
use davey::{Codec, DaveSession, MediaType, OPUS_SILENCE_PACKET};
use futures::channel::mpsc::UnboundedSender;
use interceptor::Attributes;
use opus::{Application, Channels, Encoder};
use rtcp::{
    payload_feedbacks::{full_intra_request::FullIntraRequest, picture_loss_indication::PictureLossIndication},
    receiver_report::ReceiverReport,
    reception_report::ReceptionReport,
    sender_report::SenderReport,
};
use serde_json::json;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use super::{
    GoLiveOptions, InputMode, MicLevel, QOS_TOS, Role, ShareSource, Shared, Ssrcs, VideoKind, VideoSource, VoiceEvent,
    audio::{AudioSettings, FRAME_LEN, FRAME_SAMPLES, Mixer, SAMPLE_RATE},
    feedback::{self, Feedback, RemoteVideo},
    issues,
    rtp::{self, Cipher},
    vad::{Gate, InputWatch, PreRoll},
    video::{self, FrameAssembler, VideoDecoder, VideoEncoder, VideoFrame},
};

const DAVE_MARKER: [u8; 2] = [0xfa, 0xfa];
const SILENCE_FRAMES: u8 = 5;
pub const BITRATE: i32 = 64_000;
const EXPECTED_LOSS_PERCENT: i32 = 5;
const QUIET_AFTER: Duration = Duration::from_millis(250);
const POLL: Duration = Duration::from_millis(100);
const CAMERA_POLL: Duration = Duration::from_millis(200);
const KEYFRAME_REQUEST_GAP: Duration = Duration::from_secs(1);
// The fewest keyframes we send while streaming, however many viewers ask.
const KEYFRAME_MIN_GAP: Duration = Duration::from_millis(500);
const RECV_BUFFER: usize = 4 << 20;
// Frames waiting for a picture's decoder, about 70 ms at 60 fps.
const DECODE_QUEUE: usize = 4;
// The access unit delimiter and filler NAL types. The H.264 payloader drops them, so they are taken
// out before DAVE encrypts the frame: a viewer must reassemble exactly the bytes that were sealed.
const UNSENT_NAL_TYPES: [u8; 2] = [9, 12];
// A silence frame this often while not talking keeps the UDP path (NAT mapping) open.
const KEEPALIVE: Duration = Duration::from_secs(5);
// Mic level events go out every this many 20 ms frames (25 per second).
const LEVEL_EVERY: u32 = 2;
pub const CAMERA_SIZE: (u32, u32) = (1280, 720);
pub const CAMERA_FPS: u32 = 30;
const CAMERA_QUIET_AFTER: Duration = Duration::from_secs(15);
// `GoLiveOptions::resolution == 0` (source size) is still capped here.
const SOURCE_SIZE_CAP: (u32, u32) = (3840, 2160);
const VIDEO_MTU: usize = 1200;
const VIDEO_CLOCK_HZ: u128 = 90_000;
const STREAM_AUDIO_BITRATE: i32 = 128_000;

// 16:9 bounds for a Go Live height.
pub fn go_live_size(resolution: u32) -> (u32, u32) {
    match resolution {
        0 => SOURCE_SIZE_CAP,
        height => ((height * 16 / 9) & !1, height),
    }
}

// Roughly 0.09 bits per pixel, which keeps screen text sharp in H.264 baseline.
pub fn video_bitrate(width: u32, height: u32, fps: u32) -> u32 {
    ((width as f64 * height as f64 * fps as f64 * 0.09) as u32).clamp(300_000, 8_000_000)
}

pub struct Link {
    pub socket: UdpSocket,
    pub ssrc: u32,
    // Our outgoing video SSRCs, from the voice ready payload.
    pub video_ssrc: u32,
    pub rtx_ssrc: u32,
    pub cipher: Cipher,
    pub dave: Arc<Mutex<DaveSession>>,
    pub e2ee: Arc<AtomicBool>,
    pub shared: Arc<Shared>,
    pub role: Role,
    pub my_id: String,
    pub events: UnboundedSender<VoiceEvent>,
    pub ws: UnboundedSender<WsMessage>,
    // One nonce counter for every packet sealed with this key (RTP and RTCP): a repeat breaks AES-GCM.
    pub nonce: AtomicU32,
    // Set when a viewer asks for a keyframe (or when sending starts).
    pub keyframe: AtomicBool,
    // The server's keyframe interval in ms (op 4 / op 14), 0 for none: a keyframe at least this often.
    pub keyframe_every: AtomicU64,
    // What the server last asked of our camera (op 15's want, 100 = full), which sets its size and bitrate.
    pub sink_want: AtomicU8,
    pub counters: Arc<Counters>,
    // From `connection_id`: tags this connection's tracks in the shared mixer.
    pub connection: u32,
}

static CONNECTIONS: AtomicU32 = AtomicU32::new(1);

pub fn connection_id() -> u32 {
    CONNECTIONS.fetch_add(1, Ordering::Relaxed)
}

impl Link {
    pub(super) fn next_nonce(&self) -> u32 {
        self.nonce.fetch_add(1, Ordering::Relaxed)
    }

    // Mixer tracks are shared by the voice connection and every stream watched, whose SSRCs may collide.
    fn mix_key(&self, ssrc: u32) -> u64 {
        (u64::from(self.connection) << 32) | u64::from(ssrc)
    }

    // The sender of our video feedback (PLI, NACK). Discord's client uses its own video SSRC
    // (`RemoteUser::CreateVideoReceiveStream` takes `LocalUser::VideoSsrc()`, read in
    // discord_voice.node 0.0.422); the audio SSRC stands in if the server gave us none.
    pub(super) fn feedback_ssrc(&self) -> u32 {
        if self.video_ssrc == 0 { self.ssrc } else { self.video_ssrc }
    }
}

// Loss of what we send, as the server's RTCP reports give it. Incoming loss isn't counted: gaps in
// other people's audio mix network loss with reordering and decryption failures, and it's our own
// audio reaching the call that matters.
pub struct Counters {
    // Percent, as f32 bits; NaN until the server has reported.
    outbound_loss: AtomicU32,
}

impl Default for Counters {
    fn default() -> Self {
        Self { outbound_loss: AtomicU32::new(f32::NAN.to_bits()) }
    }
}

impl Counters {
    // The server's latest report on our audio, if it has sent one.
    pub fn outbound_loss(&self) -> Option<f32> {
        Some(f32::from_bits(self.outbound_loss.load(Ordering::Relaxed))).filter(|loss| !loss.is_nan())
    }
}

pub type Users = Arc<Mutex<HashMap<u32, String>>>;

pub fn map_ssrc(users: &Users, ssrc: u32, user_id: &str) {
    if let Ok(mut users) = users.lock() {
        users.insert(ssrc, user_id.to_owned());
    }
}

pub struct Media {
    stop: Arc<AtomicBool>,
    link: Arc<Link>,
    users: Users,
    // Audio devices or the screen capture; dropping it stops them.
    _devices: Devices,
}

impl Drop for Media {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

// Binds a UDP socket to the voice server and asks it for our public address.
pub fn discover(ip: &str, port: u16, ssrc: u32) -> Result<(UdpSocket, (String, u16)), String> {
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    // A keyframe arrives as hundreds of packets at once, more than a default receive buffer holds
    // (Windows' is 64 KB, from memory; macOS's is 768 KB). The OS caps it at its own maximum.
    if let Err(e) = socket2::SockRef::from(&socket).set_recv_buffer_size(RECV_BUFFER) {
        eprintln!("voice: udp receive buffer: {e}");
    }
    socket.connect((ip, port)).map_err(|e| e.to_string())?;
    socket.set_read_timeout(Some(Duration::from_secs(2))).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 128];
    for _ in 0..3 {
        socket.send(&rtp::discovery_request(ssrc)).map_err(|e| e.to_string())?;
        let Ok(n) = socket.recv(&mut buf) else { continue };
        if let Some(address) = rtp::discovery_response(&buf[..n]) {
            return Ok((socket, address));
        }
    }
    Err("IP discovery got no answer".into())
}

impl Media {
    // `users` (SSRC → user id) is filled by the gateway, often before media starts.
    pub fn start(link: Link, users: Users) -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let rx_socket = link.socket.try_clone().map_err(|e| e.to_string())?;
        rx_socket.set_read_timeout(Some(POLL)).map_err(|e| e.to_string())?;
        let link = Arc::new(link);
        let feedback = Feedback::new(&link)?;

        let devices: Devices = match link.role {
            Role::Broadcast(source, options) => {
                let ScreenFeed { capture, frames, sound, covered } = start_capture(source, options).inspect_err(|_| link.shared.issue(&link.my_id, issues::SCREENSHARE_OS_ERROR, true))?;
                let (width, height) = go_live_size(options.resolution);
                let bitrate = video_bitrate(width, height, options.fps);
                let (l, f, s, fps) = (link.clone(), feedback.clone(), stop.clone(), options.fps);
                spawn("waffle-golive-send", move || video_send_loop(&l, &f, Feed { frames, covered }, fps, bitrate, &s))?;
                if let Some(sound) = sound {
                    let (l, s) = (link.clone(), stop.clone());
                    spawn("waffle-golive-audio", move || stream_audio_loop(&l, sound, &s))?;
                }
                let (l, f, s) = (link.clone(), feedback.clone(), stop.clone());
                spawn("waffle-golive-rtcp", move || rtcp_loop(&l, &f, &rx_socket, &s))?;
                capture
            }
            Role::Voice => {
                let shared = &link.shared;
                let tos = if shared.qos { QOS_TOS } else { 0 };
                if let Err(e) = socket2::SockRef::from(&link.socket).set_tos_v4(tos) {
                    eprintln!("voice: quality of service: {e}");
                }
                let (engine, pcm) = super::audio::start(shared.audio.clone(), shared.mixer.clone(), true)?;
                let (l, s) = (link.clone(), stop.clone());
                spawn("waffle-voice-send", move || send_loop(&l, pcm, &s))?;
                let (l, f, s) = (link.clone(), feedback.clone(), stop.clone());
                spawn("waffle-camera-control", move || camera_loop(&l, &f, &s))?;
                let (l, f, s, u) = (link.clone(), feedback, stop.clone(), users.clone());
                spawn("waffle-voice-recv", move || recv_loop(&l, &f, &rx_socket, &s, &u))?;
                Box::new(engine)
            }
            // Plays through the voice connection's mixer and speakers, so echo cancellation
            // sees everything that is played.
            Role::Watch => {
                let (l, f, s, u) = (link.clone(), feedback, stop.clone(), users.clone());
                spawn("waffle-watch-recv", move || recv_loop(&l, &f, &rx_socket, &s, &u))?;
                Box::new(())
            }
        };
        Ok(Self { stop, link, users, _devices: devices })
    }

    // Op 15 from the server: the want for our video SSRC, else for anyone.
    pub fn set_sink_wants(&self, d: &serde_json::Value) {
        let want = d[self.link.video_ssrc.to_string()].as_u64().or(d["any"].as_u64()).unwrap_or(100);
        self.link.sink_want.store(want.min(100) as u8, Ordering::Relaxed);
    }

    pub fn set_keyframe_interval(&self, ms: u64) {
        self.link.keyframe_every.store(ms, Ordering::Relaxed);
    }

    pub fn user_left(&self, user_id: &str) {
        let Ok(mut users) = self.users.lock() else { return };
        users.retain(|&ssrc, user| {
            let keep = user != user_id;
            if !keep {
                self.link.shared.mixer.remove(self.link.mix_key(ssrc));
            }
            keep
        });
    }
}

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) -> Result<(), String> {
    thread::Builder::new().name(name.into()).spawn(f).map(|_| ()).map_err(|e| e.to_string())
}

fn send_loop(link: &Link, pcm: Receiver<Vec<f32>>, stop: &AtomicBool) {
    let mut encoder = match Encoder::new(SAMPLE_RATE, Channels::Stereo, Application::Voip) {
        Ok(encoder) => encoder,
        Err(e) => return eprintln!("voice: opus encoder: {e}"),
    };
    let _ = encoder.set_bitrate(opus::Bitrate::Bits(BITRATE));
    // Discord negotiates useinbandfec=1; Opus only adds the FEC copy when it expects loss.
    // TODO(voice): feed the loss from RTCP receiver reports instead of a fixed guess.
    let _ = encoder.set_inband_fec(true);
    let _ = encoder.set_packet_loss_perc(EXPECTED_LOSS_PERCENT);
    let mut out = RtpOut { sequence: rand::random(), timestamp: rand::random() };
    let mut pending: Vec<f32> = Vec::with_capacity(FRAME_LEN * 2);
    let mut frame = [0f32; FRAME_LEN];
    let mut opus_buf = [0u8; 4000];
    let (mut speaking, mut silence_left) = (false, 0u8);
    let mut gate = Gate::default();
    let mut pre_roll = PreRoll::<FRAME_LEN>::default();
    let mut watch = InputWatch::default();
    let mut last_sent = Instant::now();
    let mut frames = 0u32;

    while !stop.load(Ordering::Relaxed) {
        match pcm.recv_timeout(POLL) {
            Ok(chunk) => pending.extend(chunk),
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        }
        while pending.len() >= FRAME_LEN {
            frame.copy_from_slice(&pending[..FRAME_LEN]);
            pending.drain(..FRAME_LEN);
            let audio = &link.shared.audio;
            let mic = audio.process_mic(&mut frame);
            let detected = watch.update(mic.raw_db);
            link.shared.input_detected.store(detected.map_or(0, |d| if d { 1 } else { 2 }), Ordering::Relaxed);
            let held = link.shared.talk_key_held.load(Ordering::Relaxed);
            let open = gate.update(link.shared.input_mode(), audio.processor.gain(), &mic.blocks, held);
            frames = frames.wrapping_add(1);
            if frames.is_multiple_of(LEVEL_EVERY) {
                let _ = link.events.unbounded_send(VoiceEvent::InputLevel { db: mic.db, gate_open: open });
            }
            let wants = link.shared.may_transmit() && open;

            if wants != speaking {
                speaking = wants;
                link.shared.self_speaking.store(speaking, Ordering::Relaxed);
                silence_left = if speaking { 0 } else { SILENCE_FRAMES };
                // Discord's voice activity says its lead-in (5 frames); push to talk has none.
                let delay = if matches!(link.shared.input_mode(), InputMode::VoiceActivity { .. }) { 5 } else { 0 };
                let op = json!({ "op": 5, "d": { "speaking": u8::from(speaking), "delay": delay, "ssrc": link.ssrc } });
                let _ = link.ws.unbounded_send(WsMessage::Text(op.to_string()));
                let _ = link.events.unbounded_send(VoiceEvent::Speaking { user_id: link.my_id.clone(), speaking });
                if speaking {
                    send_pre_roll(link, &mut out, &mut encoder, &mut pre_roll, &mut opus_buf);
                }
            }
            let opus: &[u8] = if speaking {
                match encoder.encode_float(&frame, &mut opus_buf) {
                    Ok(n) => &opus_buf[..n],
                    Err(e) => {
                        eprintln!("voice: opus encode: {e}");
                        &OPUS_SILENCE_PACKET
                    }
                }
            } else if silence_left > 0 || last_sent.elapsed() >= KEEPALIVE {
                silence_left = silence_left.saturating_sub(1);
                pre_roll.clear();
                &OPUS_SILENCE_PACKET
            } else {
                // Only unsent, unmuted voice-activity audio may lead a later onset: never what came
                // before an unmute or a push-to-talk press.
                let voice_activity = matches!(link.shared.input_mode(), InputMode::VoiceActivity { .. });
                if voice_activity && link.shared.may_transmit() {
                    pre_roll.push(&frame);
                } else {
                    pre_roll.clear();
                }
                out.timestamp = out.timestamp.wrapping_add(FRAME_SAMPLES as u32);
                continue;
            };
            if let Some(packet) = seal(link, &mut out, opus) {
                let _ = link.socket.send(&packet);
                last_sent = Instant::now();
            }
        }
    }
}

// At a voice onset, sends the frames that led up to it (Discord's leading blocks) with their own
// timestamps, just behind the current frame. The pre-roll only ever holds the unsent frames right
// before this one, so timestamps never repeat or go backwards.
fn send_pre_roll(link: &Link, out: &mut RtpOut, encoder: &mut Encoder, pre_roll: &mut PreRoll<FRAME_LEN>, opus_buf: &mut [u8]) {
    let behind = pre_roll.len() as u32 * FRAME_SAMPLES as u32;
    let current = out.timestamp;
    out.timestamp = current.wrapping_sub(behind);
    for frame in pre_roll.drain() {
        if let Ok(n) = encoder.encode_float(frame, opus_buf)
            && let Some(packet) = seal(link, out, &opus_buf[..n])
        {
            let _ = link.socket.send(&packet);
        }
    }
    out.timestamp = current;
}

// The mic test: the same mic chain and voice-activity gate as a call, played straight back.
pub fn mic_test_loop(
    audio: &AudioSettings,
    mixer: &Mixer,
    pcm: &Receiver<Vec<f32>>,
    mode: InputMode,
    levels: &UnboundedSender<MicLevel>,
    stop: &AtomicBool,
) {
    let mut pending: Vec<f32> = Vec::with_capacity(FRAME_LEN * 2);
    let mut frame = [0f32; FRAME_LEN];
    let mut gate = Gate::default();
    let mut watch = InputWatch::default();
    let mut frames = 0u32;
    while !stop.load(Ordering::Relaxed) {
        match pcm.recv_timeout(POLL) {
            Ok(chunk) => pending.extend(chunk),
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        }
        while pending.len() >= FRAME_LEN {
            frame.copy_from_slice(&pending[..FRAME_LEN]);
            pending.drain(..FRAME_LEN);
            let mic = audio.process_mic(&mut frame);
            let input_detected = watch.update(mic.raw_db);
            let speaking = gate.update(mode, audio.processor.gain(), &mic.blocks, false);
            // Only what a call would send: silence while the voice gate is closed.
            mixer.push(0, &frame, if speaking { 1.0 } else { 0.0 });
            frames = frames.wrapping_add(1);
            if frames.is_multiple_of(LEVEL_EVERY) && levels.unbounded_send(MicLevel { db: mic.db, speaking, input_detected }).is_err() {
                return;
            }
        }
    }
}

struct RtpOut {
    sequence: u16,
    timestamp: u32,
}

// With E2EE in force a frame that can't be DAVE-encrypted yet is dropped, never sent in the clear.
fn seal(link: &Link, out: &mut RtpOut, opus: &[u8]) -> Option<Vec<u8>> {
    let payload = if link.e2ee.load(Ordering::Relaxed) {
        let mut dave = link.dave.lock().ok()?;
        dave.encrypt_opus(opus).ok()?.into_owned()
    } else {
        opus.to_vec()
    };
    let header = rtp::header(rtp::OPUS_PAYLOAD_TYPE, false, out.sequence, out.timestamp, link.ssrc);
    let packet = link.cipher.seal(&header, &payload, link.next_nonce())?;
    out.sequence = out.sequence.wrapping_add(1);
    out.timestamp = out.timestamp.wrapping_add(FRAME_SAMPLES as u32);
    Some(packet)
}

// Follows `Shared::camera`: starts and stops the camera, its encoder and the op 12 announcement.
fn camera_loop(link: &Arc<Link>, feedback: &Arc<Feedback>, stop: &Arc<AtomicBool>) {
    let mut current: Option<String> = None;
    while !stop.load(Ordering::Relaxed) {
        let want = link.shared.camera.lock().ok().and_then(|c| c.clone());
        if current.as_ref() != want.as_ref() {
            let was_on = current.take().is_some();
            if want.is_none() {
                super::held_camera::release(&link.shared);
            }
            match want.map(|id| start_camera(link, feedback, &id, stop).map(|()| id)) {
                Some(Ok(started)) => current = Some(started),
                Some(Err(e)) => {
                    eprintln!("voice: camera: {e}");
                    if let Ok(mut camera) = link.shared.camera.lock() {
                        *camera = None;
                    }
                    link.shared.send_voice_state(true);
                }
                None => {}
            }
            if current.is_some() || was_on {
                let ssrcs = Ssrcs { audio: link.ssrc, video: link.video_ssrc, rtx: link.rtx_ssrc };
                let op = super::announce_video(&ssrcs, VideoKind::Camera, current.is_some());
                let _ = link.ws.unbounded_send(WsMessage::Text(op.to_string()));
            }
        }
        thread::sleep(CAMERA_POLL);
    }
}

// Frames go to the encoder (dropped if it lags) and back to the UI as our self-preview.
fn start_camera(link: &Arc<Link>, feedback: &Arc<Feedback>, id: &str, stop: &Arc<AtomicBool>) -> Result<(), String> {
    let (tx, frames) = std_mpsc::sync_channel::<Arc<VideoFrame>>(1);
    super::held_camera::attach(&link.shared, id, CAMERA_SIZE, tx)?;
    link.keyframe.store(true, Ordering::Relaxed);
    let (l, f, s) = (link.clone(), feedback.clone(), stop.clone());
    let bitrate = video_bitrate(CAMERA_SIZE.0, CAMERA_SIZE.1, CAMERA_FPS);
    // The loop ends with the connection, or when the held camera's sender moves on (another camera, or off).
    spawn("waffle-camera-send", move || video_send_loop(&l, &f, Feed { frames, covered: Arc::default() }, CAMERA_FPS, bitrate, &s))?;
    Ok(())
}

// Audio goes into the speaker's jitter buffer in the mixer, which decodes it as the device plays.
// Video is reassembled here and decoded on a thread per picture (`VideoIn`).
fn recv_loop(link: &Arc<Link>, feedback: &Arc<Feedback>, socket: &UdpSocket, stop: &AtomicBool, users: &Mutex<HashMap<u32, String>>) {
    let mut videos: HashMap<u32, VideoIn> = HashMap::new();
    let mut tracks: HashSet<u64> = HashSet::new();
    let mut heard: HashMap<String, Instant> = HashMap::new();
    let mut buf = [0u8; 2048];
    let speaking_events = link.role == Role::Voice;

    while !stop.load(Ordering::Relaxed) {
        match socket.recv(&mut buf) {
            Ok(n) if rtp::is_rtcp(&buf[..n]) => handle_rtcp(link, feedback, &buf[..n]),
            Ok(n) => {
                let Some((user, packet)) = open(link, users, &buf[..n]) else { continue };
                let ssrc = packet.header.ssrc;
                match packet.header.payload_type {
                    rtp::OPUS_PAYLOAD_TYPE => {
                        let Some(opus) = dave_open(link, &user, MediaType::AUDIO, packet.payload.to_vec()) else { continue };
                        // Closing silence frames still go to the buffer: they mark a talkspurt's end.
                        if opus != OPUS_SILENCE_PACKET && heard.insert(user.clone(), Instant::now()).is_none() && speaking_events {
                            let _ = link.events.unbounded_send(VoiceEvent::Speaking { user_id: user.clone(), speaking: true });
                        }
                        let gain = match link.role {
                            Role::Watch => link.shared.stream_gain(&user),
                            _ => link.shared.listening_gain(&user),
                        };
                        let key = link.mix_key(ssrc);
                        tracks.insert(key);
                        link.shared.mixer.receive(key, packet.header.sequence_number, packet.header.timestamp, opus, gain);
                    }
                    rtp::H264_PAYLOAD_TYPE => {
                        let video = match videos.entry(ssrc) {
                            Entry::Occupied(e) => e.into_mut(),
                            Entry::Vacant(e) => match VideoIn::start(link, feedback, user, ssrc) {
                                Ok(video) => e.insert(video),
                                Err(err) => {
                                    eprintln!("voice: h264 decoder: {err}");
                                    continue;
                                }
                            },
                        };
                        video.receive(packet);
                    }
                    // Each user sends one picture per connection, so the user names the stream a
                    // retransmission belongs to.
                    rtp::H264_RTX_PAYLOAD_TYPE => {
                        if let Some(video) = videos.values_mut().find(|v| v.user == user)
                            && let Some(original) = feedback::unwrap_rtx(&packet, video.ssrc)
                        {
                            video.receive(original);
                        }
                    }
                    _ => {}
                }
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            // A connected UDP socket reports ICMP errors (port unreachable) on the next receive. The
            // link can recover, and ending the call is the gateway's call, not this thread's.
            Err(e) => socket_error(link, &e),
        }
        heard.retain(|user, at| {
            let talking = at.elapsed() < QUIET_AFTER;
            if !talking && speaking_events {
                let _ = link.events.unbounded_send(VoiceEvent::Speaking { user_id: user.clone(), speaking: false });
            }
            talking
        });
        if speaking_events {
            link.shared.others_speaking.store(!heard.is_empty(), Ordering::Relaxed);
        }
    }
    // Left in the mixer, a track would be decoded forever, and watching the same stream again
    // would land in its stale jitter buffer.
    for key in tracks {
        link.shared.mixer.remove(key);
    }
}

fn socket_error(link: &Link, e: &std::io::Error) {
    if super::debug() {
        eprintln!("[{:?}] udp receive: {e}", link.role);
    }
    thread::sleep(POLL);
}

// Keyframe requests from viewers, NACKs (the responder resends what they name) and the server's
// loss reports about what we send.
fn handle_rtcp(link: &Link, feedback: &Feedback, packet: &[u8]) {
    let Some(rtcp) = link.cipher.open_rtcp(packet) else { return };
    for packet in feedback.rtcp_in(rtcp.into()) {
        let packet = packet.as_any();
        if packet.is::<PictureLossIndication>() || packet.is::<FullIntraRequest>() {
            link.keyframe.store(true, Ordering::Relaxed);
            if super::debug() {
                eprintln!("[{:?}] viewer asked for a keyframe", link.role);
            }
        }
        let reports = match (packet.downcast_ref::<ReceiverReport>(), packet.downcast_ref::<SenderReport>()) {
            (Some(receiver), _) => &receiver.reports,
            (_, Some(sender)) => &sender.reports,
            _ => continue,
        };
        // What the server says we lose of our stream's video (2004 past 10%).
        if matches!(link.role, Role::Broadcast(..))
            && let Some(loss) = reported_loss(reports, link.video_ssrc)
        {
            link.shared.issue(&link.my_id, issues::STREAM_SEND_HIGH_PACKET_LOSS, loss > issues::LOSS_LINE);
        }
        if let Some(loss) = reported_loss(reports, link.ssrc) {
            if super::debug() {
                eprintln!("[{:?}] server reports {loss:.1}% loss of our audio", link.role);
            }
            link.counters.outbound_loss.store(loss.to_bits(), Ordering::Relaxed);
        }
    }
}

// RTCP's fraction-lost byte for `ssrc`, as a percentage.
fn reported_loss(reports: &[ReceptionReport], ssrc: u32) -> Option<f32> {
    reports.iter().find(|r| r.ssrc == ssrc).map(|r| f32::from(r.fraction_lost) / 256.0 * 100.0)
}

// One remote picture. Packets are reassembled on the receive thread and frames decoded on their
// own, so a slow decode never holds up the socket (or everyone's audio, on the voice connection).
struct VideoIn {
    user: String,
    ssrc: u32,
    nacks: RemoteVideo,
    frames: FrameAssembler,
    units: SyncSender<Vec<u8>>,
    // A frame was lost before reaching the decoder, which then waits for a keyframe.
    lost: Arc<AtomicBool>,
    // A watched stream's loss after retransmissions, for 2003; None for cameras, which Discord doesn't check.
    loss: Option<(issues::LossMeter, Arc<Shared>)>,
}

impl VideoIn {
    fn start(link: &Arc<Link>, feedback: &Arc<Feedback>, user: String, ssrc: u32) -> Result<Self, String> {
        let decoder = VideoDecoder::new()?;
        let (units, queued) = std_mpsc::sync_channel(DECODE_QUEUE);
        let lost = Arc::new(AtomicBool::new(false));
        let (l, u, flag) = (link.clone(), user.clone(), lost.clone());
        spawn("waffle-video-decode", move || decode_loop(&l, &u, ssrc, decoder, &queued, &flag))?;
        let loss = (link.role == Role::Watch).then(|| (issues::LossMeter::default(), link.shared.clone()));
        Ok(Self { user, ssrc, nacks: feedback.receive_video(ssrc), frames: FrameAssembler::default(), units, lost, loss })
    }

    fn receive(&mut self, packet: Packet) {
        let Some(packet) = self.nacks.arrived(packet) else { return };
        let now = Instant::now();
        if let Some((meter, shared)) = &mut self.loss
            && let Some(loss) = meter.packet(packet.header.sequence_number, now)
        {
            shared.issue(&self.user, issues::STREAM_VIEW_HIGH_PACKET_LOSS, loss > issues::LOSS_LINE);
        }
        self.frames.push(packet, now);
        while let Some(frame) = self.frames.pop(now) {
            // A full queue means the decoder is behind: dropping a frame and catching up on the next
            // keyframe beats falling further behind.
            let queued = frame.is_ok_and(|unit| self.units.try_send(unit).is_ok());
            if !queued {
                self.lost.store(true, Ordering::Relaxed);
            }
        }
    }
}

// Ends when the receive loop drops its `VideoIn`. DAVE encrypts whole encoded frames, so
// decryption happens after reassembly.
fn decode_loop(link: &Link, user: &str, ssrc: u32, mut decoder: VideoDecoder, units: &Receiver<Vec<u8>>, lost: &AtomicBool) {
    let source = link.role.video_source();
    let mut last_request: Option<Instant> = None;
    loop {
        let unit = match units.recv_timeout(POLL) {
            Ok(unit) => Some(unit),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if lost.swap(false, Ordering::Relaxed) {
            decoder.frame_lost();
        }
        if let Some(unit) = unit {
            match dave_open(link, user, MediaType::VIDEO, unit) {
                Some(unit) => {
                    if let Some(frame) = decoder.decode(&unit) {
                        link.shared.publish_frame(user, source, Arc::new(frame));
                    }
                }
                None => decoder.frame_lost(),
            }
        }
        if decoder.awaiting_keyframe() && last_request.is_none_or(|at| at.elapsed() >= KEYFRAME_REQUEST_GAP) {
            last_request = Some(Instant::now());
            feedback::send_rtcp(link, &PictureLossIndication { sender_ssrc: link.feedback_ssrc(), media_ssrc: ssrc });
            if super::debug() {
                eprintln!("[{:?}] video: requested keyframe for ssrc {ssrc}", link.role);
            }
        }
    }
}

type Devices = Box<dyn std::any::Any + Send>;

struct ScreenFeed {
    capture: Devices,
    frames: Receiver<Arc<VideoFrame>>,
    sound: Option<Receiver<Vec<f32>>>,
    covered: Arc<AtomicBool>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn start_capture(source: ShareSource, options: GoLiveOptions) -> Result<ScreenFeed, String> {
    let (width, height) = go_live_size(options.resolution);
    let captured = super::capture::start(source, options.fps, width, height, options.audio)?;
    let sound = options.audio.then_some(captured.audio);
    Ok(ScreenFeed { capture: Box::new(captured.capture), frames: captured.frames, sound, covered: captured.covered })
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn start_capture(_source: ShareSource, _options: GoLiveOptions) -> Result<ScreenFeed, String> {
    Err("Screen sharing is only implemented on macOS and Windows".into())
}

// The stream's sound: always on (not gated by mic mute or voice detection), music-tuned Opus.
fn stream_audio_loop(link: &Link, sound: Receiver<Vec<f32>>, stop: &AtomicBool) {
    let mut encoder = match Encoder::new(SAMPLE_RATE, Channels::Stereo, Application::Audio) {
        Ok(encoder) => encoder,
        Err(e) => {
            link.shared.issue(&link.my_id, issues::SOUNDSHARE_FAILED, true);
            return eprintln!("voice: opus encoder: {e}");
        }
    };
    let _ = encoder.set_bitrate(opus::Bitrate::Bits(STREAM_AUDIO_BITRATE));
    // Not verified: speaking flag 2 (soundshare) marks this SSRC as stream audio.
    let op = json!({ "op": 5, "d": { "speaking": 2, "delay": 0, "ssrc": link.ssrc } });
    let _ = link.ws.unbounded_send(WsMessage::Text(op.to_string()));
    let mut out = RtpOut { sequence: rand::random(), timestamp: rand::random() };
    let mut pending: Vec<f32> = Vec::with_capacity(FRAME_LEN * 2);
    let mut opus_buf = [0u8; 4000];
    while !stop.load(Ordering::Relaxed) {
        match sound.recv_timeout(POLL) {
            Ok(chunk) => pending.extend(chunk),
            Err(RecvTimeoutError::Timeout) => continue,
            // The capture's sound ended while the stream goes on (Discord's 1001).
            Err(RecvTimeoutError::Disconnected) => {
                if !stop.load(Ordering::Relaxed) {
                    link.shared.issue(&link.my_id, issues::SOUNDSHARE_FAILED, true);
                }
                return;
            }
        }
        while pending.len() >= FRAME_LEN {
            let frame: Vec<f32> = pending.drain(..FRAME_LEN).collect();
            let Ok(n) = encoder.encode_float(&frame, &mut opus_buf) else { continue };
            if let Some(packet) = seal(link, &mut out, &opus_buf[..n]) {
                let _ = link.socket.send(&packet);
            }
        }
    }
}

// Encodes frames as they come (camera or screen) and sends them on our video SSRC, through the
// responder so viewers' NACKs can be answered. Ends when `stop` is set or the frame source is dropped.
// A video source's frames, and whether what it shows is out of sight (the stream pauses meanwhile).
struct Feed {
    frames: Receiver<Arc<VideoFrame>>,
    covered: Arc<AtomicBool>,
}

fn video_send_loop(link: &Arc<Link>, feedback: &Feedback, feed: Feed, fps: u32, bitrate: u32, stop: &AtomicBool) {
    let Feed { frames, covered } = feed;
    let mut encoder = match VideoEncoder::new(fps, bitrate) {
        Ok(encoder) => encoder,
        Err(e) => return eprintln!("voice: h264 encoder: {e}"),
    };
    let writer = feedback.send_video(link);
    let mut payloader = H264Payloader::default();
    let started = Instant::now();
    let mut sequence: u16 = rand::random();
    // Discord's viewers flag a stream under half its frame rate (error 2002, "stream-view-low-fps"),
    // so a still screen repeats its last frame at the full rate, as Discord's own capturer does; an
    // unchanged frame encodes to a few bytes.
    let mut interval = Duration::from_secs_f64(1.0 / f64::from(fps.max(1)));
    let mut spoke = Instant::now();
    let mut rung = super::ladder::rung(100);
    let mut last: Option<Arc<VideoFrame>> = None;
    let mut due = Instant::now() + interval;
    // Discord's checks on what we send: frames encoded per second against its line (2005 for a stream,
    // 2013 under 10 for a camera), and a source that never delivers a first frame (2011).
    let streaming = matches!(link.role, Role::Broadcast(..));
    let (low_code, low_line) = if streaming { (issues::STREAM_SEND_LOW_FPS, issues::low_fps_line(fps)) } else { (issues::CAMERA_SEND_LOW_FPS, 10.) };
    let mut rate = issues::RateMeter::new(Instant::now());
    // A camera's issues are told under "camera:{id}", apart from the stream's.
    let who = if streaming { link.my_id.clone() } else { format!("camera:{}", link.my_id) };
    let raise = |code, on| link.shared.issue(&who, code, on);
    // Sharing an app or a window, Discord pauses the stream while it can't be seen, and when its
    // capture ends; a whole screen's capture ending ends the stream.
    let shares_app = matches!(link.role, Role::Broadcast(ShareSource::App(_) | ShareSource::Window(_), _));
    let paused = std::cell::Cell::new(false);
    let pause = |on: bool| {
        if paused.replace(on) != on {
            link.shared.set_stream_paused(on);
            // Viewers need a keyframe to pick the picture back up.
            link.keyframe.store(!on, Ordering::Relaxed);
        }
    };
    let mut last_keyframe: Option<Instant> = None;
    let mut pacer = super::pacer::Pacer::new(bitrate);
    while !stop.load(Ordering::Relaxed) {
        // Every viewer's request forces one, and each is the largest frame there is: answering them
        // all would flood the link and lose more packets, which brings more requests.
        let every = link.keyframe_every.load(Ordering::Relaxed);
        if every > 0 && last_keyframe.is_some_and(|at| at.elapsed() >= Duration::from_millis(every)) {
            link.keyframe.store(true, Ordering::Relaxed);
        }
        let keyframe_due = link.keyframe.load(Ordering::Relaxed) && last_keyframe.is_none_or(|at| at.elapsed() >= KEYFRAME_MIN_GAP);
        // ScreenCaptureKit only delivers frames when the screen changes; repeating the last one
        // keeps the stream alive and lets keyframe requests be answered on a still screen.
        let frame = match frames.recv_timeout(due.saturating_duration_since(Instant::now())) {
            // Our own Go Live comes back to the UI as its self-view, as the camera's does.
            Ok(frame) if matches!(link.role, Role::Broadcast(..)) => {
                pause(false);
                link.shared.publish_frame(&link.my_id, VideoSource::GoLive, frame.clone());
                frame
            }
            Ok(frame) => frame,
            Err(RecvTimeoutError::Timeout) if shares_app && covered.load(Ordering::Relaxed) => {
                pause(true);
                due = Instant::now() + interval;
                continue;
            }
            Err(RecvTimeoutError::Timeout) => match &last {
                Some(frame) => frame.clone(),
                None => {
                    raise(issues::SENDER_READY_TIMEOUT, started.elapsed() >= issues::READY_TIMEOUT);
                    due = Instant::now() + interval;
                    continue;
                }
            },
            Err(RecvTimeoutError::Disconnected) if shares_app => {
                pause(true);
                while !stop.load(Ordering::Relaxed) {
                    thread::sleep(POLL);
                }
                return clear_send_issues(link, &who, low_code);
            }
            // The capture ended under a stream that goes on: the OS stopped it (Discord's 3004).
            Err(RecvTimeoutError::Disconnected) => {
                if streaming && !stop.load(Ordering::Relaxed) {
                    raise(issues::SCREENSHARE_OS_ERROR, true);
                }
                return clear_send_issues(link, &who, low_code);
            }
        };
        if last.is_none() {
            raise(issues::SENDER_READY_TIMEOUT, false);
        }
        if keyframe_due {
            link.keyframe.store(false, Ordering::Relaxed);
            last_keyframe = Some(Instant::now());
            encoder.force_keyframe();
        }
        // A camera sends what the server asks (its ladder rung's size and bitrate; nothing at want 0), and
        // drops to the rung's quiet rate after 15 s without speaking, or at once when muted, as Discord's.
        let frame = match streaming {
            true => frame,
            false => {
                let want = link.sink_want.load(Ordering::Relaxed);
                if want != rung.want && let Ok(rebuilt) = VideoEncoder::new(fps, super::ladder::rung(want).bitrate()) {
                    (rung, encoder) = (super::ladder::rung(want), rebuilt);
                    pacer = super::pacer::Pacer::new(rung.bitrate());
                }
                if link.shared.self_speaking.load(Ordering::Relaxed) {
                    spoke = Instant::now();
                }
                let quiet = link.shared.muted.load(Ordering::Relaxed) || spoke.elapsed() >= CAMERA_QUIET_AFTER;
                interval = Duration::from_secs_f64(1.0 / f64::from(if quiet { rung.quiet_fps.min(fps) } else { fps }.max(1)));
                if want == 0 {
                    due = Instant::now() + interval;
                    continue;
                }
                if frame.width > rung.width { Arc::new(frame.fit(rung.width, rung.height)) } else { frame }
            }
        };
        // Behind (a slow encode): the next frame is due now rather than in a burst catching up.
        due = (due + interval).max(Instant::now());
        let unit = encoder.encode(&frame);
        last = Some(frame);
        let Some(unit) = unit else { continue };
        rate.frame();
        if let Some(sent) = rate.rate(Instant::now()) {
            raise(low_code, sent < low_line);
        }
        let Some(sealed) = dave_seal_video(link, &video::without_nal_types(&unit, &UNSENT_NAL_TYPES)) else { continue };
        let timestamp = (started.elapsed().as_micros() * VIDEO_CLOCK_HZ / 1_000_000) as u32;
        let Ok(payloads) = payloader.payload(VIDEO_MTU, &Bytes::from(sealed)) else { continue };
        let last_index = payloads.len().saturating_sub(1);
        for (i, payload) in payloads.into_iter().enumerate() {
            let header = Header { version: 2, marker: i == last_index, payload_type: rtp::H264_PAYLOAD_TYPE, sequence_number: sequence, timestamp, ssrc: link.video_ssrc, ..Default::default() };
            sequence = sequence.wrapping_add(1);
            pacer.wait(rtp::HEADER_LEN + payload.len());
            feedback::run(writer.write(&Packet { header, payload }, &Attributes::new())).ok();
        }
    }
    clear_send_issues(link, &who, low_code);
}

// What a send loop raised goes with it.
fn clear_send_issues(link: &Link, who: &str, low_code: u16) {
    for code in [low_code, issues::SENDER_READY_TIMEOUT] {
        link.shared.issue(who, code, false);
    }
}

// Same rule as audio: under E2EE a frame that can't be DAVE-encrypted yet is dropped.
fn dave_seal_video(link: &Link, unit: &[u8]) -> Option<Vec<u8>> {
    if !link.e2ee.load(Ordering::Relaxed) {
        return Some(unit.to_vec());
    }
    let mut dave = link.dave.lock().ok()?;
    let sealed = dave.encrypt(MediaType::VIDEO, Codec::H264, unit);
    if let Err(e) = &sealed
        && super::debug()
    {
        eprintln!("[{:?}] dave encrypt: {e:?}", link.role);
    }
    Some(sealed.ok()?.into_owned())
}

// The broadcast socket only carries RTCP back to us: keyframe requests, NACKs and loss reports.
fn rtcp_loop(link: &Link, feedback: &Feedback, socket: &UdpSocket, stop: &AtomicBool) {
    let mut buf = [0u8; 1500];
    while !stop.load(Ordering::Relaxed) {
        match socket.recv(&mut buf) {
            Ok(n) if rtp::is_rtcp(&buf[..n]) => handle_rtcp(link, feedback, &buf[..n]),
            Ok(_) => {}
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(e) => socket_error(link, &e),
        }
    }
}

fn open(link: &Link, users: &Mutex<HashMap<u32, String>>, packet: &[u8]) -> Option<(String, Packet)> {
    let Some(received) = link.cipher.open(packet) else {
        if super::debug() {
            eprintln!("[{:?}] udp: {} bytes failed transport decryption", link.role, packet.len());
        }
        return None;
    };
    let user = users.lock().ok()?.get(&received.header.ssrc).cloned();
    if user.is_none() && super::debug() {
        eprintln!("[{:?}] udp: unmapped ssrc {} pt {}", link.role, received.header.ssrc, received.header.payload_type);
    }
    Some((user?, received))
}

// With E2EE in force only DAVE frames (and Opus silence) are accepted.
fn dave_open(link: &Link, user: &str, media: MediaType, data: Vec<u8>) -> Option<Vec<u8>> {
    if data.ends_with(&DAVE_MARKER) {
        let id = user.parse().ok()?;
        let result = link.dave.lock().ok()?.decrypt(id, media, &data);
        if let Err(e) = &result
            && super::debug()
        {
            eprintln!("[{:?}] dave decrypt for {user}: {e:?}", link.role);
        }
        return result.ok();
    }
    let allowed = !link.e2ee.load(Ordering::Relaxed) || data == OPUS_SILENCE_PACKET;
    allowed.then_some(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use opus::Decoder;

    #[test]
    fn outbound_loss_is_unknown_until_reported() {
        let counters = Counters::default();
        assert_eq!(counters.outbound_loss(), None);
        counters.outbound_loss.store(2.5f32.to_bits(), Ordering::Relaxed);
        assert_eq!(counters.outbound_loss(), Some(2.5));
    }

    #[test]
    fn go_live_sizes_and_bitrates() {
        assert_eq!(go_live_size(720), (1280, 720));
        assert_eq!(go_live_size(1080), (1920, 1080));
        assert_eq!(go_live_size(480), (852, 480));
        assert_eq!(go_live_size(0), SOURCE_SIZE_CAP);
        assert_eq!(video_bitrate(1280, 720, 30), 2_488_320);
        assert_eq!(video_bitrate(16, 16, 1), 300_000);
    }

    #[test]
    fn opus_round_trip_at_discord_frame_size() {
        let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Stereo, Application::Voip).unwrap();
        let mut decoder = Decoder::new(SAMPLE_RATE, Channels::Stereo).unwrap();
        let tone: Vec<f32> = (0..FRAME_LEN).map(|i| ((i / 2) as f32 * 0.05).sin() * 0.5).collect();
        let mut packet = [0u8; 4000];
        let n = encoder.encode_float(&tone, &mut packet).unwrap();
        let mut pcm = vec![0f32; FRAME_LEN];
        assert_eq!(decoder.decode_float(&packet[..n], &mut pcm, false).unwrap(), FRAME_SAMPLES);
    }
}
