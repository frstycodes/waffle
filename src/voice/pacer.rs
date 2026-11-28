// Spreading a video frame's packets over time.

use std::{
    thread,
    time::{Duration, Instant},
};

// libwebrtc's pacing factor (Not verified that it's still 2.5); bursts under PACING_SLACK go out unpaced.
const PACING_FACTOR: f64 = 2.5;
const PACING_SLACK: Duration = Duration::from_millis(5);

// Spreads a frame's packets at PACING_FACTOR times the target bitrate instead of sending them in
// one burst, which overflows send buffers and router queues on every keyframe.
pub(super) struct Pacer {
    bits_per_second: f64,
    next: Instant,
}

impl Pacer {
    pub(super) fn new(bitrate: u32) -> Self {
        Self { bits_per_second: f64::from(bitrate) * PACING_FACTOR, next: Instant::now() }
    }

    pub(super) fn wait(&mut self, bytes: usize) {
        let now = Instant::now();
        if self.next > now + PACING_SLACK {
            thread::sleep(self.next - now);
        }
        self.next = self.next.max(now) + Duration::from_secs_f64(bytes as f64 * 8.0 / self.bits_per_second);
    }
}
