// Discord's AV error codes and the checks that raise them, from its web client's error table and
// AVErrorManager (2026-11-21). The media threads run these checks and raise issues through
// `Shared::issue`; the app gets `VoiceEvent::Issue` when one starts or clears, and decides what to show.
use std::time::{Duration, Instant};

pub const SOUNDSHARE_FAILED: u16 = 1001;
pub const STREAM_VIEW_HIGH_PACKET_LOSS: u16 = 2003;
pub const STREAM_SEND_HIGH_PACKET_LOSS: u16 = 2004;
pub const STREAM_SEND_LOW_FPS: u16 = 2005;
pub const STREAM_SEND_NETWORK_QUALITY: u16 = 2006;
pub const SENDER_READY_TIMEOUT: u16 = 2011;
pub const CAMERA_SEND_LOW_FPS: u16 = 2013;
pub const SCREENSHARE_OS_ERROR: u16 = 3004;

// A camera or stream that hasn't delivered its first frame by now is flagged (2011, 2012).
// Not verified: Discord's own timeout isn't in its downloaded bundle.
pub const READY_TIMEOUT: Duration = Duration::from_secs(10);
pub const RECEIVER_READY_TIMEOUT: u16 = 2012;

// What a connection's end clears: a watch's, and our stream's (the send loop clears its own).
pub const WATCH_ISSUES: [u16; 1] = [STREAM_VIEW_HIGH_PACKET_LOSS];
pub const BROADCAST_ISSUES: [u16; 4] = [SOUNDSHARE_FAILED, STREAM_SEND_HIGH_PACKET_LOSS, STREAM_SEND_NETWORK_QUALITY, SCREENSHARE_OS_ERROR];

// Discord flags packet loss over 10%.
pub const LOSS_LINE: f32 = 10.;
// Discord needs 15 samples before judging; a window this long gives that at any usable rate.
const WINDOW: Duration = Duration::from_secs(5);
const MIN_PACKETS: u64 = 15;

// Discord's low-frame-rate line for a stream's own rate: 3 / 8 / 15 / 30 fps for up to 5 / 15 / 30 /
// 60 fps. A camera's line is a flat 10.
pub fn low_fps_line(fps: u32) -> f32 {
    match fps {
        ..=5 => 3.,
        6..=15 => 8.,
        16..=30 => 15.,
        _ => 30.,
    }
}

// Received RTP against the sequence range it spans, per window: the share lost, once a window has
// enough packets to judge.
#[derive(Default)]
pub struct LossMeter {
    since: Option<Instant>,
    last: Option<u16>,
    span: u64,
    received: u64,
}

impl LossMeter {
    // A percentage at the end of each window; None in between.
    pub fn packet(&mut self, sequence: u16, now: Instant) -> Option<f32> {
        let since = *self.since.get_or_insert(now);
        match self.last {
            // Forward (allowing for wrap): the gap counts as expected packets. Late or repeated ones only count as received.
            Some(last) if sequence.wrapping_sub(last) < 0x8000 && sequence != last => {
                self.span += u64::from(sequence.wrapping_sub(last));
                self.last = Some(sequence);
            }
            Some(_) => {}
            None => {
                self.span = 1;
                self.last = Some(sequence);
            }
        }
        self.received += 1;
        if now.duration_since(since) < WINDOW {
            return None;
        }
        let (span, received) = (self.span, self.received.min(self.span));
        *self = Self { last: self.last, ..Default::default() };
        (span >= MIN_PACKETS).then(|| (span - received) as f32 / span as f32 * 100.)
    }
}

// Frames per second over each window, once a window ends.
pub struct RateMeter {
    since: Instant,
    frames: u32,
}

impl RateMeter {
    pub fn new(now: Instant) -> Self {
        Self { since: now, frames: 0 }
    }

    pub fn frame(&mut self) {
        self.frames += 1;
    }

    pub fn rate(&mut self, now: Instant) -> Option<f32> {
        let elapsed = now.duration_since(self.since);
        if elapsed < WINDOW {
            return None;
        }
        let fps = self.frames as f32 / elapsed.as_secs_f32();
        *self = Self::new(now);
        Some(fps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loss_counts_gaps_across_a_window() {
        let start = Instant::now();
        let mut meter = LossMeter::default();
        // 100 sequences, every 5th missing, then the window closes: 20% lost.
        let mut result = None;
        for i in 0..100u16 {
            if i % 5 == 4 {
                continue;
            }
            result = meter.packet(65_500u16.wrapping_add(i), start);
        }
        assert_eq!(result, None, "the window isn't over");
        result = meter.packet(65_500u16.wrapping_add(100), start + WINDOW);
        let loss = result.expect("window over");
        assert!((loss - 19.8).abs() < 0.5, "{loss}");
    }

    #[test]
    fn too_few_packets_say_nothing() {
        let start = Instant::now();
        let mut meter = LossMeter::default();
        meter.packet(1, start);
        assert_eq!(meter.packet(9, start + WINDOW), None);
    }

    #[test]
    fn rate_per_window_and_discord_lines() {
        let start = Instant::now();
        let mut meter = RateMeter::new(start);
        (0..50).for_each(|_| meter.frame());
        assert_eq!(meter.rate(start + Duration::from_secs(1)), None);
        assert_eq!(meter.rate(start + WINDOW), Some(10.));
        assert_eq!((low_fps_line(5), low_fps_line(15), low_fps_line(30), low_fps_line(60)), (3., 8., 15., 30.));
    }
}
