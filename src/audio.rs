// Voice messages and audio attachments: the whole file decoded to PCM, played through the shared
// output at any speed with the pitch kept (WSOLA time-stretch).

use std::{
    f32::consts::TAU,
    io::Cursor,
    num::NonZero,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use rodio::{ChannelCount, Player, SampleRate, Source, mixer::Mixer};

const OPUS_RATE: u32 = 48_000;
// The mixer pulls in bursts; between them the position runs on the wall clock, never further than this.
const DRIFT: Duration = Duration::from_millis(50);
const MIN_RATE: f32 = 0.75;
const MAX_RATE: f32 = 2.0;

// The whole file as interleaved f32: a 20-minute 48 kHz mono voice message is about 230 MB. Keeping
// i16, or down-mixing stereo files to mono, is the way to shrink it if long files turn up.
#[derive(Clone)]
pub struct Clip(Arc<Pcm>);

struct Pcm {
    samples: Vec<f32>,
    channels: u16,
    rate: u32,
}

impl Clip {
    pub fn duration(&self) -> Duration {
        self.0.time(self.0.frames())
    }
}

impl Pcm {
    fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels)
    }

    fn time(&self, frame: usize) -> Duration {
        Duration::from_secs_f64(frame as f64 / f64::from(self.rate))
    }

    fn frame(&self, at: Duration) -> usize {
        ((at.as_secs_f64() * f64::from(self.rate)) as usize).min(self.frames())
    }

    fn mono(&self, frame: usize) -> f32 {
        let ch = usize::from(self.channels);
        self.samples[frame * ch..(frame + 1) * ch].iter().sum()
    }
}

// Blocking: decodes the whole file. Call off the UI thread.
pub fn decode(bytes: &[u8]) -> Result<Clip, String> {
    let packets = ogg_packets(bytes);
    let pcm = match packets.split_first() {
        Some((head, rest)) if head.starts_with(b"OpusHead") => decode_opus(head, rest.get(1..).unwrap_or_default())?,
        _ => decode_other(bytes)?,
    };
    if pcm.samples.is_empty() {
        return Err("The file has no audio".into());
    }
    Ok(Clip(Arc::new(pcm)))
}

// `packets` follow OpusHead and OpusTags.
fn decode_opus(head: &[u8], packets: &[Vec<u8>]) -> Result<Pcm, String> {
    let (Some(&channels), Some(pre_skip)) = (head.get(9), head.get(10..12)) else { return Err("Bad OpusHead".into()) };
    let layout = match channels {
        1 => opus::Channels::Mono,
        2 => opus::Channels::Stereo,
        n => return Err(format!("{n}-channel Opus isn't supported")),
    };
    let mut decoder = opus::Decoder::new(OPUS_RATE, layout).map_err(|e| e.to_string())?;
    let ch = usize::from(channels);
    // 120 ms, Opus's longest packet.
    let mut frame = vec![0.0; 5760 * ch];
    let mut samples = Vec::new();
    for packet in packets {
        let Ok(n) = decoder.decode_float(packet, &mut frame, false) else { continue };
        samples.extend_from_slice(&frame[..n * ch]);
    }
    let skip = usize::from(u16::from_le_bytes([pre_skip[0], pre_skip[1]])) * ch;
    samples.drain(..skip.min(samples.len()));
    Ok(Pcm { samples, channels: channels.into(), rate: OPUS_RATE })
}

fn decode_other(bytes: &[u8]) -> Result<Pcm, String> {
    let decoder = rodio::Decoder::try_from(Cursor::new(bytes.to_vec())).map_err(|e| e.to_string())?;
    let (channels, rate) = (decoder.channels().get(), decoder.sample_rate().get());
    Ok(Pcm { samples: decoder.collect(), channels, rate })
}

// The packets of the file's first logical stream. Page CRCs aren't checked: the bytes came over TLS.
fn ogg_packets(bytes: &[u8]) -> Vec<Vec<u8>> {
    let (mut packets, mut packet, mut serial) = (Vec::new(), Vec::new(), None);
    let mut at = 0;
    while let Some(header) = bytes.get(at..at + 27).filter(|h| h.starts_with(b"OggS")) {
        let Some(lacing) = bytes.get(at + 27..at + 27 + usize::from(header[26])) else { break };
        let page_serial = &header[14..18];
        let ours = *serial.get_or_insert(page_serial) == page_serial;
        at += 27 + lacing.len();
        for &len in lacing {
            let Some(segment) = bytes.get(at..at + usize::from(len)) else { return packets };
            at += usize::from(len);
            if !ours {
                continue;
            }
            packet.extend_from_slice(segment);
            if len < 255 {
                packets.push(std::mem::take(&mut packet));
            }
        }
    }
    packets
}

pub struct AudioPlayback {
    clip: Clip,
    mixer: Mixer,
    player: Player,
    progress: Arc<Progress>,
    rate: f32,
    // Where it was paused; the source's read position runs a little ahead of what was heard.
    paused_at: Option<Duration>,
}

impl AudioPlayback {
    // Blocking only for opening the output device; starts paused at 0.
    pub fn open(clip: Clip) -> Result<Self, String> {
        let mixer = crate::playback::mixer().ok_or("No audio output device")?;
        let (player, progress) = start(&clip, &mixer, Duration::ZERO, 1.0, 1.0, false);
        Ok(Self { clip, mixer, player, progress, rate: 1.0, paused_at: Some(Duration::ZERO) })
    }

    pub fn duration(&self) -> Duration {
        self.clip.duration()
    }

    // Smooth: the source's read position, run forward on the wall clock between the mixer's pulls.
    pub fn position(&self) -> Duration {
        if let Some(at) = self.paused_at {
            return at;
        }
        if self.is_ended() {
            return self.duration();
        }
        let (frame, stamp) = self.progress.read();
        let since = stamp.elapsed().min(DRIFT).mul_f32(self.rate);
        (self.clip.0.time(frame) + since).min(self.duration())
    }

    pub fn is_playing(&self) -> bool {
        self.paused_at.is_none() && !self.is_ended()
    }

    pub fn is_ended(&self) -> bool {
        self.player.empty()
    }

    pub fn play(&mut self) {
        if self.is_ended() {
            return self.start_at(Duration::ZERO, true);
        }
        let Some(at) = self.paused_at.take() else { return };
        self.progress.set(self.clip.0.frame(at));
        self.player.play();
    }

    pub fn pause(&mut self) {
        if !self.is_playing() {
            return;
        }
        self.paused_at = Some(self.position());
        self.player.pause();
    }

    pub fn seek(&mut self, to: Duration) {
        let playing = self.is_playing();
        self.start_at(to.min(self.duration()), playing);
    }

    pub fn set_volume(&mut self, amp: f32) {
        self.player.set_volume(amp.clamp(0.0, 1.0));
    }

    pub fn set_rate(&mut self, rate: f32) {
        let rate = rate.clamp(MIN_RATE, MAX_RATE);
        if rate == self.rate {
            return;
        }
        let (at, playing, ended) = (self.position(), self.is_playing(), self.is_ended());
        self.rate = rate;
        if !ended {
            self.start_at(at, playing);
        }
    }

    fn start_at(&mut self, at: Duration, playing: bool) {
        (self.player, self.progress) = start(&self.clip, &self.mixer, at, self.rate, self.player.volume(), playing);
        self.paused_at = (!playing).then_some(at);
    }
}

fn start(clip: &Clip, mixer: &Mixer, at: Duration, rate: f32, volume: f32, playing: bool) -> (Player, Arc<Progress>) {
    let progress = Arc::new(Progress::new(clip.0.frame(at)));
    let player = Player::connect_new(mixer);
    player.set_volume(volume);
    if !playing {
        player.pause();
    }
    player.append(Stretch::new(clip.0.clone(), clip.0.frame(at), rate, progress.clone()));
    (player, progress)
}

// The input frame the source has read up to, and when it got there.
struct Progress {
    frame: AtomicU64,
    stamp: AtomicU64,
    epoch: Instant,
}

impl Progress {
    fn new(frame: usize) -> Self {
        Self { frame: AtomicU64::new(frame as u64), stamp: AtomicU64::new(0), epoch: Instant::now() }
    }

    fn set(&self, frame: usize) {
        self.frame.store(frame as u64, Ordering::Relaxed);
        self.stamp.store(self.epoch.elapsed().as_nanos() as u64, Ordering::Relaxed);
    }

    fn read(&self) -> (usize, Instant) {
        let stamp = self.epoch + Duration::from_nanos(self.stamp.load(Ordering::Relaxed));
        (self.frame.load(Ordering::Relaxed) as usize, stamp)
    }
}

// WSOLA: Hann-windowed 30 ms segments overlap-added every 15 ms of output, each read from where the
// rate puts it, moved up to 10 ms either way to line up with the waveform the last segment left off on.
// Rate 1 copies the input through.
struct Stretch {
    pcm: Arc<Pcm>,
    rate: f32,
    progress: Arc<Progress>,
    window: Vec<f32>,
    search: usize,
    // The input frame the next segment nominally starts at.
    analysis: f64,
    previous: Option<usize>,
    // The last segment's windowed second half, waiting for the next one's first half.
    tail: Vec<f32>,
    out: Vec<f32>,
    cursor: usize,
}

impl Stretch {
    fn new(pcm: Arc<Pcm>, from: usize, rate: f32, progress: Arc<Progress>) -> Self {
        let len = (pcm.rate as usize * 30 / 1000).max(2) & !1;
        let window = (0..len).map(|i| 0.5 - 0.5 * (TAU * i as f32 / len as f32).cos()).collect();
        let tail = vec![0.0; len / 2 * usize::from(pcm.channels)];
        let search = pcm.rate as usize / 100;
        Self { pcm, rate, progress, window, search, analysis: from as f64, previous: None, tail, out: Vec::new(), cursor: 0 }
    }

    fn hop(&self) -> usize {
        self.window.len() / 2
    }

    fn refill(&mut self) -> bool {
        let at = self.analysis.round() as usize;
        self.progress.set(at.min(self.pcm.frames()));
        self.out.clear();
        self.cursor = 0;
        if self.rate == 1.0 { self.copy(at) } else { self.overlap_add(at) }
    }

    fn copy(&mut self, at: usize) -> bool {
        let ch = usize::from(self.pcm.channels);
        let end = ((at + self.hop()) * ch).min(self.pcm.samples.len());
        let Some(chunk) = self.pcm.samples.get(at * ch..end).filter(|c| !c.is_empty()) else { return false };
        self.out.extend_from_slice(chunk);
        self.analysis += self.hop() as f64;
        true
    }

    fn overlap_add(&mut self, nominal: usize) -> bool {
        let Some(start) = self.segment_start(nominal) else { return false };
        let (ch, half) = (usize::from(self.pcm.channels), self.tail.len());
        let segment = &self.pcm.samples[start * ch..start * ch + 2 * half];
        let windowed = |i: usize| segment[i] * self.window[i / ch];
        self.out.extend((0..half).map(|i| self.tail[i] + windowed(i)));
        let next: Vec<f32> = (half..2 * half).map(windowed).collect();
        self.tail = next;
        self.previous = Some(start);
        self.analysis += self.hop() as f64 * f64::from(self.rate);
        true
    }

    // The start near `nominal` whose opening best matches the input that followed the last segment.
    fn segment_start(&self, nominal: usize) -> Option<usize> {
        let last = self.pcm.frames().checked_sub(self.window.len())?;
        let Some(previous) = self.previous else { return (nominal <= last).then_some(nominal) };
        let natural = previous + self.hop();
        let (lo, hi) = (nominal.saturating_sub(self.search), (nominal + self.search).min(last));
        (lo..=hi).map(|s| (s, self.similarity(s, natural))).max_by(|a, b| a.1.total_cmp(&b.1)).map(|(s, _)| s)
    }

    // Every other frame is plenty at speech and music frequencies, and halves the search.
    fn similarity(&self, a: usize, b: usize) -> f32 {
        (0..self.hop()).step_by(2).map(|i| self.pcm.mono(a + i) * self.pcm.mono(b + i)).sum()
    }
}

impl Iterator for Stretch {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.cursor == self.out.len() && !self.refill() {
            self.progress.set(self.pcm.frames());
            return None;
        }
        self.cursor += 1;
        Some(self.out[self.cursor - 1])
    }
}

impl Source for Stretch {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        NonZero::new(self.pcm.channels).unwrap_or(NonZero::<u16>::MIN)
    }

    fn sample_rate(&self) -> SampleRate {
        NonZero::new(self.pcm.rate).unwrap_or(NonZero::<u32>::MIN)
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn sine(secs: f32, hz: f32) -> Vec<f32> {
        (0..(secs * RATE as f32) as usize).map(|i| (TAU * hz * i as f32 / RATE as f32).sin() * 0.5).collect()
    }

    fn stretched(rate: f32) -> Vec<f32> {
        let pcm = Arc::new(Pcm { samples: sine(1.0, 440.0), channels: 1, rate: RATE });
        Stretch::new(pcm, 0, rate, Arc::new(Progress::new(0))).collect()
    }

    // Rising zero crossings per second, away from the edges' fades.
    fn frequency(samples: &[f32]) -> f32 {
        let middle = &samples[2_000..samples.len() - 2_000];
        let rising = middle.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        rising as f32 * RATE as f32 / middle.len() as f32
    }

    #[test]
    fn time_stretch_keeps_pitch() {
        for (rate, expected) in [(2.0, 0.5), (0.75, 1.0 / 0.75)] {
            let out = stretched(rate);
            let secs = out.len() as f32 / RATE as f32;
            assert!((secs - expected).abs() <= expected * 0.05, "rate {rate}: {secs}s");
            let hz = frequency(&out);
            assert!((hz - 440.0).abs() <= 440.0 * 0.03, "rate {rate}: {hz} Hz");
        }
    }

    #[test]
    fn rate_one_passes_samples_through() {
        assert_eq!(stretched(1.0), sine(1.0, 440.0));
    }

    // Pages hold at most `per_page` lacing values, so long packets continue across pages. CRCs are left
    // zero: the reader doesn't check them.
    fn ogg(packets: &[Vec<u8>], per_page: usize) -> Vec<u8> {
        let lacing: Vec<u8> = packets.iter().flat_map(|p| std::iter::repeat_n(255, p.len() / 255).chain([(p.len() % 255) as u8])).collect();
        let mut body = packets.concat().into_iter();
        let mut out = Vec::new();
        for (seq, page) in lacing.chunks(per_page).enumerate() {
            out.extend_from_slice(b"OggS\0\0");
            out.extend_from_slice(&[0; 8]);
            out.extend_from_slice(&7u32.to_le_bytes());
            out.extend_from_slice(&(seq as u32).to_le_bytes());
            out.extend_from_slice(&[0; 4]);
            out.push(page.len() as u8);
            out.extend_from_slice(page);
            out.extend(body.by_ref().take(page.iter().map(|&l| usize::from(l)).sum()));
        }
        out
    }

    fn opus_head(pre_skip: u16) -> Vec<u8> {
        [&b"OpusHead\x01\x01"[..], &pre_skip.to_le_bytes(), &RATE.to_le_bytes(), &[0, 0, 0]].concat()
    }

    #[test]
    fn ogg_reader_returns_packets_across_lacing_and_pages() {
        let packets = vec![opus_head(312), b"OpusTags".to_vec(), (0..600).map(|i| i as u8).collect(), vec![9; 40]];
        assert_eq!(ogg_packets(&ogg(&packets, 2)), packets);
        assert_eq!(ogg_packets(&ogg(&packets, 255)), packets);
    }

    #[test]
    fn opus_round_trip() {
        let mut encoder = opus::Encoder::new(RATE, opus::Channels::Mono, opus::Application::Audio).unwrap();
        let mut packets = vec![opus_head(312), b"OpusTags".to_vec()];
        for frame in sine(0.5, 440.0).chunks(960) {
            let mut packet = vec![0; 4000];
            let n = encoder.encode_float(frame, &mut packet).unwrap();
            packet.truncate(n);
            packets.push(packet);
        }
        let clip = decode(&ogg(&packets, 255)).unwrap();
        let secs = clip.duration().as_secs_f32();
        assert!((secs - 0.5).abs() <= 0.025, "{secs}s");
    }
}
