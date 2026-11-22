// Plays a downloaded MP4: H.264 decoded a few frames ahead on its own thread, AAC audio through rodio,
// and each picture shown when the audio reaches its time, so sound and picture stay together.
use std::{
    io::Cursor,
    ops::ControlFlow,
    sync::{Arc, OnceLock, mpsc},
    time::{Duration, Instant},
};

use gpui_kit::RenderImage;
use rodio::{Player, mixer::Mixer};

use crate::h264::{Track, bgra};

// A window's worth; wider videos are scaled down as they're decoded.
const MAX_WIDTH: u32 = 1920;
// Pictures decoded ahead of the clock: a quarter second at 30 fps rides out a slow frame.
const AHEAD: usize = 8;

pub struct Playback {
    bytes: Arc<[u8]>,
    duration: Duration,
    frames: mpsc::Receiver<Picture>,
    next: Option<Picture>,
    // Just sought: show the first picture that arrives, even while paused.
    sought: bool,
    audio: Option<Audio>,
    clock: Clock,
}

pub struct Picture {
    at: Duration,
    image: Arc<RenderImage>,
}

impl Playback {
    // Blocking (parses the clip and opens the audio device): call off the UI thread.
    pub fn open(bytes: Arc<[u8]>) -> Result<Self, String> {
        let bytes = crate::h264::modernize_audio_entries(&bytes).map(Arc::from).unwrap_or(bytes);
        let duration = Track::open(bytes.clone())?.duration();
        let audio = Audio::open(&bytes);
        Ok(Self { frames: decode_from(bytes.clone(), Duration::ZERO), bytes, duration, next: None, sought: true, audio, clock: Clock::default() })
    }

    pub fn duration(&self) -> Duration {
        self.duration
    }

    pub fn position(&self) -> Duration {
        self.clock.at(Instant::now()).min(self.duration)
    }

    pub fn is_playing(&self) -> bool {
        self.clock.is_running()
    }

    // Waiting for the picture a seek landed on.
    pub fn is_seeking(&self) -> bool {
        self.sought
    }

    pub fn is_ended(&self) -> bool {
        self.clock.at(Instant::now()) >= self.duration
    }

    pub fn play(&mut self) {
        if self.is_ended() {
            self.seek(Duration::ZERO);
        }
        self.clock.play(Instant::now());
        if let Some(audio) = &self.audio {
            audio.player.play();
        }
    }

    pub fn pause(&mut self) {
        self.clock.pause(Instant::now());
        if let Some(audio) = &self.audio {
            audio.player.pause();
        }
    }

    pub fn seek(&mut self, at: Duration) {
        let at = at.min(self.duration);
        self.frames = decode_from(self.bytes.clone(), at);
        self.next = None;
        self.sought = true;
        self.clock.set(at, Instant::now());
        let playing = self.is_playing();
        if let Some(audio) = &mut self.audio {
            audio.start_at(at, playing);
        }
    }

    pub fn set_volume(&self, volume: f32) {
        if let Some(audio) = &self.audio {
            audio.player.set_volume(volume);
        }
    }

    // The picture to show now, when it's changed: the latest one due, skipping any the clock has passed.
    pub fn poll(&mut self) -> Option<Arc<RenderImage>> {
        let now = Instant::now();
        if self.is_playing()
            && let Some(heard) = self.audio.as_ref().and_then(Audio::position)
        {
            self.clock.set(heard, now);
        }
        let mut drained = false;
        let incoming = std::iter::from_fn(|| match self.frames.try_recv() {
            Ok(picture) => Some(picture),
            Err(error) => {
                drained = error == mpsc::TryRecvError::Disconnected;
                None
            }
        });
        let pending = self.next.take().into_iter().chain(incoming).map(|p| (p.at, p.image));
        let at = self.clock.at(now);
        let due = if self.sought { first_then_due(pending, at) } else { latest_due(pending, at) };
        self.next = due.next.map(|(at, image)| Picture { at, image });
        if due.shown.is_some() || (drained && self.next.is_none()) {
            self.sought = false;
        }
        due.shown
    }
}

struct Due<T> {
    shown: Option<T>,
    next: Option<(Duration, T)>,
}

// Pictures arrive in display order: show the last one whose time has come, hold the first one still ahead.
fn latest_due<T>(pending: impl Iterator<Item = (Duration, T)>, now: Duration) -> Due<T> {
    let mut shown = None;
    for (at, picture) in pending {
        if at > now {
            return Due { shown, next: Some((at, picture)) };
        }
        shown = Some(picture);
    }
    Due { shown, next: None }
}

// After a seek the first picture is shown whatever its time, so a paused scrub still shows where it landed.
fn first_then_due<T>(mut pending: impl Iterator<Item = (Duration, T)>, now: Duration) -> Due<T> {
    let Some((_, first)) = pending.next() else { return Due { shown: None, next: None } };
    let rest = latest_due(pending, now);
    Due { shown: rest.shown.or(Some(first)), next: rest.next }
}

// Decodes from the keyframe before `at`, sending pictures from `at` on. The thread ends when the
// receiver is dropped (a seek replaces it) or the clip ends.
fn decode_from(bytes: Arc<[u8]>, at: Duration) -> mpsc::Receiver<Picture> {
    let (send, receive) = mpsc::sync_channel(AHEAD);
    let decode = move || {
        let Ok(mut track) = Track::open(bytes) else { return };
        let from = track.keyframe_before(at);
        let _ = track.decode(from, |time, yuv| {
            if time < at {
                return ControlFlow::Continue(());
            }
            let Some(picture) = bgra(yuv, MAX_WIDTH) else { return ControlFlow::Continue(()) };
            let image = Arc::new(RenderImage::new([image::Frame::new(picture)]));
            match send.send(Picture { at: time, image }) {
                Ok(()) => ControlFlow::Continue(()),
                Err(_) => ControlFlow::Break(()),
            }
        });
    };
    let _ = std::thread::Builder::new().name("video-decode".into()).spawn(decode);
    receive
}

// The media timeline, running or paused; set back to the audio's position while sound plays.
#[derive(Clone, Copy, Debug, Default)]
struct Clock {
    base: Duration,
    since: Option<Instant>,
}

impl Clock {
    fn at(&self, now: Instant) -> Duration {
        self.base + self.since.map_or(Duration::ZERO, |since| now.saturating_duration_since(since))
    }

    fn is_running(&self) -> bool {
        self.since.is_some()
    }

    fn play(&mut self, now: Instant) {
        self.since.get_or_insert(now);
    }

    fn pause(&mut self, now: Instant) {
        self.base = self.at(now);
        self.since = None;
    }

    fn set(&mut self, at: Duration, now: Instant) {
        self.base = at;
        if self.since.is_some() {
            self.since = Some(now);
        }
    }
}

// The clip's AAC track, rewrapped as ADTS (which rodio's AAC decoder reads on its own) so the video
// track never reaches the audio decoder. Seeking starts a fresh player at the frame before the target.
struct Audio {
    adts: Arc<[u8]>,
    // Each ADTS frame's media time and byte offset.
    frames: Vec<(Duration, usize)>,
    // Encoder priming the edit list skips: the sound at media time zero starts this far in.
    priming: Duration,
    mixer: Mixer,
    player: Player,
    start: Duration,
}

impl Audio {
    fn open(bytes: &[u8]) -> Option<Self> {
        let Adts { stream, frames, priming } = adts(bytes)?;
        let mixer = mixer()?;
        let mut audio = Self { adts: stream.into(), frames, priming, player: Player::connect_new(&mixer), mixer, start: Duration::ZERO };
        audio.start_at(Duration::ZERO, false);
        Some(audio)
    }

    fn start_at(&mut self, at: Duration, playing: bool) {
        let i = self.frames.partition_point(|(t, _)| *t <= at + self.priming).saturating_sub(1);
        let Some(&(start, offset)) = self.frames.get(i) else { return };
        let player = Player::connect_new(&self.mixer);
        player.set_volume(self.player.volume());
        if !playing {
            player.pause();
        }
        if let Ok(source) = rodio::Decoder::try_from(Cursor::new(Tail(self.adts.clone(), offset))) {
            player.append(source);
        }
        self.player = player;
        self.start = start;
    }

    // Where the sound is, on the video's timeline; None once it's run out.
    fn position(&self) -> Option<Duration> {
        (!self.player.empty()).then(|| (self.start + self.player.get_pos()).saturating_sub(self.priming))
    }
}

struct Tail(Arc<[u8]>, usize);

impl AsRef<[u8]> for Tail {
    fn as_ref(&self) -> &[u8] {
        &self.0[self.1..]
    }
}

// One output stream for every video, opened on first use. Its device handle has to stay on the thread
// that opened it on some platforms, so that thread keeps it for the life of the app.
pub(crate) fn mixer() -> Option<Mixer> {
    static MIXER: OnceLock<Option<Mixer>> = OnceLock::new();
    MIXER
        .get_or_init(|| {
            let (send, receive) = mpsc::channel();
            std::thread::Builder::new()
                .name("video-audio".into())
                .spawn(move || {
                    let Ok(mut sink) = rodio::DeviceSinkBuilder::open_default_sink() else { return drop(send.send(None)) };
                    sink.log_on_drop(false);
                    let _ = send.send(Some(sink.mixer().clone()));
                    loop {
                        std::thread::park();
                    }
                })
                .ok()?;
            receive.recv().ok().flatten()
        })
        .clone()
}

// The AAC track as an ADTS stream, with each frame's time and offset, and the priming to skip.
struct Adts {
    stream: Vec<u8>,
    frames: Vec<(Duration, usize)>,
    priming: Duration,
}

fn adts(bytes: &[u8]) -> Option<Adts> {
    let mut reader = mp4::Mp4Reader::read_header(Cursor::new(bytes), bytes.len() as u64).ok()?;
    let track = reader.tracks().values().find(|t| matches!(t.media_type(), Ok(mp4::MediaType::AAC)))?;
    // ADTS has room for the four original AAC profiles; HE-AAC's core is AAC-LC.
    let profile = Some(track.audio_profile().ok()? as u8).filter(|p| (1..=4).contains(p)).unwrap_or(2);
    let (rate, channels) = (track.sample_freq_index().ok()? as u8, track.channel_config().ok()? as u8);
    let (id, count, timescale) = (track.track_id(), track.sample_count(), u64::from(track.timescale().max(1)));
    let priming = track.trak.edts.iter().filter_map(|e| e.elst.as_ref()).flat_map(|e| e.entries.first()).map(|e| e.media_time).find(|t| *t < timescale * 10).unwrap_or(0);
    let time = |ticks: u64| Duration::from_micros(ticks * 1_000_000 / timescale);
    let (mut out, mut frames) = (Vec::new(), Vec::new());
    for sample_id in 1..=count {
        let Ok(Some(sample)) = reader.read_sample(id, sample_id) else { continue };
        frames.push((time(sample.start_time), out.len()));
        out.extend_from_slice(&adts_header(profile, rate, channels, sample.bytes.len()));
        out.extend_from_slice(&sample.bytes);
    }
    (!frames.is_empty()).then(|| Adts { stream: out, frames, priming: time(priming) })
}

// ADTS's 7-byte header without CRC: MPEG-4, one raw block, buffer fullness "variable" (0x7FF).
fn adts_header(profile: u8, rate_index: u8, channels: u8, payload: usize) -> [u8; 7] {
    let len = payload + 7;
    [
        0xFF,
        0xF1,
        ((profile - 1) << 6) | ((rate_index & 0xF) << 2) | ((channels >> 2) & 1),
        ((channels & 3) << 6) | ((len >> 11) & 3) as u8,
        (len >> 3) as u8,
        ((len & 7) << 5) as u8 | 0x1F,
        0xFC,
    ]
}

// How Discord writes a video's times: m:ss, with hours only when there are some.
pub fn time_label(at: Duration) -> String {
    let s = at.as_secs();
    match s / 3600 {
        0 => format!("{}:{:02}", s / 60, s % 60),
        h => format!("{h}:{:02}:{:02}", s / 60 % 60, s % 60),
    }
}

#[cfg(test)]
mod tests {
    use rodio::Source;

    use super::*;

    const CLIP: &[u8] = include_bytes!("../testdata/video-aac.mp4");

    #[test]
    fn clock_runs_pauses_and_jumps() {
        let t0 = Instant::now();
        let at = |ms| t0 + Duration::from_millis(ms);
        let mut clock = Clock::default();
        assert_eq!(clock.at(at(500)), Duration::ZERO);
        clock.play(at(100));
        assert_eq!(clock.at(at(600)), Duration::from_millis(500));
        clock.pause(at(600));
        assert_eq!(clock.at(at(5000)), Duration::from_millis(500));
        clock.set(Duration::from_secs(2), at(5000));
        assert_eq!(clock.at(at(9000)), Duration::from_secs(2));
        clock.play(at(9000));
        clock.set(Duration::from_secs(3), at(9100));
        assert_eq!(clock.at(at(9350)), Duration::from_millis(3250));
    }

    #[test]
    fn shows_the_latest_due_picture_and_holds_the_next() {
        let ms = Duration::from_millis;
        let pending = || [(ms(0), 'a'), (ms(33), 'b'), (ms(66), 'c'), (ms(100), 'd')].into_iter();
        let due = latest_due(pending(), ms(70));
        assert_eq!((due.shown, due.next), (Some('c'), Some((ms(100), 'd'))));
        let due = latest_due(pending().skip(1), ms(10));
        assert_eq!((due.shown, due.next), (None, Some((ms(33), 'b'))));
        // A paused seek to 50ms lands between pictures: the first one after it shows anyway.
        let due = first_then_due(pending().skip(2), ms(50));
        assert_eq!((due.shown, due.next), (Some('c'), Some((ms(100), 'd'))));
    }

    // Seeking to 1.45s decodes from the 1s keyframe but only sends what's at or after the target.
    #[test]
    fn decode_thread_starts_at_the_seek_target() {
        let times: Vec<Duration> = decode_from(CLIP.into(), Duration::from_millis(1450)).iter().map(|p| p.at).collect();
        assert_eq!(times.first(), Some(&Duration::from_millis(1500)));
        assert_eq!(times.len(), 5);
    }

    #[test]
    fn labels_times_like_discord() {
        assert_eq!(time_label(Duration::from_millis(4_900)), "0:04");
        assert_eq!(time_label(Duration::from_secs(75)), "1:15");
        assert_eq!(time_label(Duration::from_secs(3_725)), "1:02:05");
    }

    #[test]
    fn adts_header_bits() {
        // AAC-LC, 44.1 kHz, stereo, 100-byte frame.
        assert_eq!(adts_header(2, 4, 2, 100), [0xFF, 0xF1, 0x50, 0x80, 0x0D, 0x7F, 0xFC]);
    }

    // The test clip's audio is two seconds of 44.1 kHz mono AAC: rewrapped, it decodes to all of it,
    // and a stream started from a later frame decodes the rest.
    #[test]
    fn aac_track_decodes_through_adts() {
        let Adts { stream, frames, priming } = adts(CLIP).expect("aac track");
        assert_eq!(frames.len(), 88);
        assert_eq!(priming, Duration::from_micros(1024 * 1_000_000 / 44_100));
        let decoder = rodio::Decoder::try_from(Cursor::new(stream.clone())).expect("decodes");
        assert_eq!((decoder.channels().get(), decoder.sample_rate().get()), (1, 44_100));
        let samples = decoder.count();
        assert!((88_000..=90_200).contains(&samples), "{samples}");
        let tail = rodio::Decoder::try_from(Cursor::new(Tail(stream.into(), frames[44].1))).expect("decodes").count();
        assert!((44_000..=45_100).contains(&tail), "{tail}");
    }
}
