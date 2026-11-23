use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use rodio::Source;

// Waffle's sound cues (assets/sounds, made by tools/sound_cues.py), played where Discord plays its own.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub enum Cue {
    Mute,
    Unmute,
    Deafen,
    Undeafen,
    UserJoin,
    UserLeave,
    UserMoved,
    Disconnect,
    StreamStarted,
    StreamEnded,
    StreamUserJoined,
    StreamUserLeft,
    Message,
    Mention,
    PttStart,
    PttStop,
    CallCalling,
    CallRinging,
    ActivityLaunch,
    ActivityEnd,
    ActivityUserJoin,
    ActivityUserLeft,
}

impl Cue {
    fn bytes(self) -> &'static [u8] {
        match self {
            Cue::Mute => include_bytes!("../assets/sounds/mute.mp3"),
            Cue::Unmute => include_bytes!("../assets/sounds/unmute.mp3"),
            Cue::Deafen => include_bytes!("../assets/sounds/deafen.mp3"),
            Cue::Undeafen => include_bytes!("../assets/sounds/undeafen.mp3"),
            Cue::UserJoin => include_bytes!("../assets/sounds/user_join.mp3"),
            Cue::UserLeave => include_bytes!("../assets/sounds/user_leave.mp3"),
            Cue::UserMoved => include_bytes!("../assets/sounds/user_moved.mp3"),
            Cue::Disconnect => include_bytes!("../assets/sounds/disconnect.mp3"),
            Cue::StreamStarted => include_bytes!("../assets/sounds/stream_started.mp3"),
            Cue::StreamEnded => include_bytes!("../assets/sounds/stream_ended.mp3"),
            Cue::StreamUserJoined => include_bytes!("../assets/sounds/stream_user_joined.mp3"),
            Cue::StreamUserLeft => include_bytes!("../assets/sounds/stream_user_left.mp3"),
            Cue::Message => include_bytes!("../assets/sounds/message.mp3"),
            Cue::Mention => include_bytes!("../assets/sounds/mention.mp3"),
            Cue::PttStart => include_bytes!("../assets/sounds/ptt_start.mp3"),
            Cue::PttStop => include_bytes!("../assets/sounds/ptt_stop.mp3"),
            Cue::CallCalling => include_bytes!("../assets/sounds/call_calling.mp3"),
            Cue::CallRinging => include_bytes!("../assets/sounds/call_ringing.mp3"),
            Cue::ActivityLaunch => include_bytes!("../assets/sounds/activity_launch.mp3"),
            Cue::ActivityEnd => include_bytes!("../assets/sounds/activity_end.mp3"),
            Cue::ActivityUserJoin => include_bytes!("../assets/sounds/activity_user_join.mp3"),
            Cue::ActivityUserLeft => include_bytes!("../assets/sounds/activity_user_left.mp3"),
        }
    }
}

static SILENCED: std::sync::Mutex<Vec<Cue>> = std::sync::Mutex::new(Vec::new());

// The cues turned off in Settings; `play` skips them, `preview` doesn't.
pub fn silence(cues: &[Cue]) {
    if let Ok(mut silenced) = SILENCED.lock() {
        *silenced = cues.to_vec();
    }
}

fn silenced(cue: Cue) -> bool {
    SILENCED.lock().is_ok_and(|s| s.contains(&cue))
}

pub fn play(cue: Cue) {
    if silenced(cue) {
        return;
    }
    preview(cue);
}

pub fn preview(cue: Cue) {
    send(cue, None);
}

// The ringtone playing now, if any; setting its flag ends the loop.
static RINGING: std::sync::Mutex<Option<Arc<AtomicBool>>> = std::sync::Mutex::new(None);

// Loops a ringtone until `stop_ring`, replacing any ringing already.
pub fn ring(cue: Cue) {
    stop_ring();
    if silenced(cue) {
        return;
    }
    let stop = Arc::new(AtomicBool::new(false));
    if let Ok(mut ringing) = RINGING.lock() {
        *ringing = Some(stop.clone());
    }
    send(cue, Some(stop));
}

pub fn stop_ring() {
    if let Some(stop) = RINGING.lock().ok().and_then(|mut ringing| ringing.take()) {
        stop.store(true, Ordering::Relaxed);
    }
}

// A soundboard sound as fetched (mp3 or ogg) at a gain of 0..=1; playing a key again restarts it.
pub fn play_clip(key: String, bytes: bytes::Bytes, volume: f32) {
    request(Request::Clip { key, bytes, volume });
}

pub fn stop_clips() {
    request(Request::StopClips);
}

// How long a clip plays: from its header when it says, else by decoding it.
pub fn clip_length(bytes: &bytes::Bytes) -> Option<Duration> {
    let source = clip_source(bytes.clone())?;
    if let Some(length) = source.total_duration() {
        return Some(length);
    }
    let per_second = source.sample_rate().get() as f32 * f32::from(source.channels().get());
    Some(Duration::from_secs_f32(source.count() as f32 / per_second))
}

// rodio reads mp3 and Ogg Vorbis but not Ogg Opus, which Discord keeps some uploaded sounds as; libopus,
// linked for voice, decodes those.
fn clip_source(bytes: bytes::Bytes) -> Option<Box<dyn Source + Send>> {
    if let Some(samples) = ogg_opus(&bytes) {
        let (channels, rate) = (std::num::NonZero::new(2)?, std::num::NonZero::new(OPUS_RATE)?);
        return Some(Box::new(rodio::buffer::SamplesBuffer::new(channels, rate, samples)));
    }
    let decoder = rodio::Decoder::try_from(std::io::Cursor::new(bytes)).ok()?;
    Some(Box::new(decoder))
}

const OPUS_RATE: u32 = 48_000;

// An Ogg Opus file as interleaved stereo at 48kHz, its pre-skip dropped; None for anything else.
fn ogg_opus(bytes: &[u8]) -> Option<Vec<f32>> {
    let packets = ogg_packets(bytes);
    let head = packets.first().filter(|p| p.starts_with(b"OpusHead"))?;
    let pre_skip = usize::from(u16::from_le_bytes([*head.get(10)?, *head.get(11)?]));
    let mut decoder = opus::Decoder::new(OPUS_RATE, opus::Channels::Stereo).ok()?;
    // 120ms, Opus's longest packet, in stereo.
    let mut frame = vec![0f32; 5760 * 2];
    let mut out = Vec::new();
    for packet in packets.iter().skip(2) {
        let Ok(n) = decoder.decode_float(packet, &mut frame, false) else { continue };
        out.extend_from_slice(&frame[..n * 2]);
    }
    out.drain(..(pre_skip * 2).min(out.len()));
    Some(out)
}

// The packets of an Ogg stream's pages, in order (one logical stream assumed, as a sound file has).
fn ogg_packets(mut bytes: &[u8]) -> Vec<Vec<u8>> {
    let (mut packets, mut packet) = (Vec::new(), Vec::new());
    while bytes.starts_with(b"OggS") && bytes.len() >= 27 {
        let segments = usize::from(bytes[26]);
        let Some(table) = bytes.get(27..27 + segments) else { break };
        let mut body = &bytes[27 + segments..];
        for &lace in table {
            let Some(chunk) = body.get(..usize::from(lace)) else { return packets };
            packet.extend_from_slice(chunk);
            body = &body[chunk.len()..];
            // A lacing value under 255 ends the packet; 255 continues it into the next segment.
            if lace < 255 {
                packets.push(std::mem::take(&mut packet));
            }
        }
        bytes = body;
    }
    packets
}

fn send(cue: Cue, looping: Option<Arc<AtomicBool>>) {
    request(Request::Cue(cue, looping));
}

enum Request {
    // A cue sent with a stop flag loops until the flag is set.
    Cue(Cue, Option<Arc<AtomicBool>>),
    Clip { key: String, bytes: bytes::Bytes, volume: f32 },
    StopClips,
}

// One audio thread owns the output device for the life of the app; every sound is queued to it.
fn request(request: Request) {
    static PLAYER: std::sync::OnceLock<std::sync::mpsc::Sender<Request>> = std::sync::OnceLock::new();
    let player = PLAYER.get_or_init(|| {
        let (send, receive) = std::sync::mpsc::channel::<Request>();
        std::thread::spawn(move || {
            let Ok(mut sink) = rodio::DeviceSinkBuilder::open_default_sink() else { return };
            sink.log_on_drop(false);
            let mut clips: std::collections::HashMap<String, Arc<AtomicBool>> = std::collections::HashMap::new();
            for request in receive {
                match request {
                    Request::Cue(cue, looping) => {
                        let Ok(sound) = rodio::Decoder::try_from(std::io::Cursor::new(cue.bytes())) else { continue };
                        match looping {
                            None => sink.mixer().add(sound),
                            Some(stop) => sink.mixer().add(sound.buffered().repeat_infinite().stoppable().periodic_access(Duration::from_millis(50), stopper(stop))),
                        }
                    }
                    Request::Clip { key, bytes, volume } => {
                        let bytes_len = bytes.len();
                        let Some(sound) = clip_source(bytes) else {
                            eprintln!("soundboard: can't decode a {}-byte clip", bytes_len);
                            continue;
                        };
                        let stop = Arc::new(AtomicBool::new(false));
                        if let Some(old) = clips.insert(key, stop.clone()) {
                            old.store(true, Ordering::Relaxed);
                        }
                        sink.mixer().add(sound.amplify(volume).stoppable().periodic_access(Duration::from_millis(20), stopper(stop)));
                    }
                    Request::StopClips => {
                        for (_, stop) in clips.drain() {
                            stop.store(true, Ordering::Relaxed);
                        }
                    }
                }
            }
        });
        send
    });
    let _ = player.send(request);
}

fn stopper<S: Source>(stop: Arc<AtomicBool>) -> impl FnMut(&mut rodio::source::Stoppable<S>) {
    move |s| {
        if stop.load(Ordering::Relaxed) {
            s.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    // ffmpeg -f lavfi -i sine=d=0.2 -ac 1 -c:a libopus: a mono Ogg Opus file, as Discord serves some sounds.
    const OPUS: &[u8] = include_bytes!("../assets/sounds/test_opus.ogg");

    #[test]
    fn ogg_opus_decodes_to_stereo_at_its_length() {
        let samples = super::ogg_opus(OPUS).expect("decodes");
        let secs = samples.len() as f32 / 2. / super::OPUS_RATE as f32;
        assert!((secs - 0.2).abs() < 0.03, "{secs}s");
        assert!(samples.iter().any(|s| s.abs() > 0.1), "silent");
        let length = super::clip_length(&bytes::Bytes::from_static(OPUS)).expect("measured");
        assert!((length.as_secs_f32() - 0.2).abs() < 0.03);
    }

    #[test]
    fn non_opus_isnt_taken_for_opus() {
        assert!(super::ogg_opus(include_bytes!("../assets/sounds/mute.mp3")).is_none());
    }
}

// The cue for someone's voice state changing, seen from our call. Others arriving or going sound as
// a join or a leave, from or to another channel too. Our own come from the call: a join when it
// connects, or a move when it switched channels in the same server (see `connected_cue`).
pub(crate) fn member_cue(mine: bool, was_here: bool, is_here: bool) -> Option<Cue> {
    match (mine, was_here, is_here) {
        (true, ..) => None,
        (false, false, true) => Some(Cue::UserJoin),
        (false, true, false) => Some(Cue::UserLeave),
        _ => None,
    }
}

pub(crate) fn connected_cue(switched: bool) -> Cue {
    if switched { Cue::UserMoved } else { Cue::UserJoin }
}

// Discord's cue when the viewer count of the stream we're in changes (it stays quiet past 25).
pub(crate) fn viewer_cue(before: usize, after: usize) -> Option<Cue> {
    match () {
        _ if before > 25 => None,
        _ if after > before => Some(Cue::StreamUserJoined),
        _ if after < before => Some(Cue::StreamUserLeft),
        _ => None,
    }
}

#[cfg(test)]
mod viewer_tests {
    use super::*;

    #[test]
    fn viewer_cues() {
        assert_eq!(viewer_cue(0, 1), Some(Cue::StreamUserJoined));
        assert_eq!(viewer_cue(2, 1), Some(Cue::StreamUserLeft));
        assert_eq!(viewer_cue(1, 1), None);
        assert_eq!(viewer_cue(30, 31), None);
    }

    // A channel hop is one move for us, and a join or leave for whoever we're in the call with.
    #[test]
    fn voice_state_cues() {
        assert_eq!(member_cue(false, false, true), Some(Cue::UserJoin), "someone hops in from another channel");
        assert_eq!(member_cue(false, true, false), Some(Cue::UserLeave), "someone hops out to another channel");
        assert_eq!(member_cue(false, true, true), None);
        assert_eq!(member_cue(true, true, false), None, "our own hop sounds once, from the call");
        assert_eq!(connected_cue(true), Cue::UserMoved);
        assert_eq!(connected_cue(false), Cue::UserJoin);
    }
}
