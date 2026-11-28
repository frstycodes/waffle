// How a dropped voice or Go Live connection comes back, as Discord's client does it. The socket resumes
// (op 7) a few times; past that, or when the close says the session is gone, the connection identifies
// afresh after a growing wait, forever. No close code ends a call: only the main gateway (our voice
// state cleared or moved, the stream deleted) or the user does.

use std::time::Duration;

// Discord's socket gives up resuming after this many failures in a row.
const RESUME_TRIES: u32 = 4;
// A session whose last heartbeat ack is older than this is past resuming.
const RESUME_WITHIN: Duration = Duration::from_secs(60);

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Reconnect {
    Resume,
    Identify,
}

// `close` is the close frame's code (None for a reset, an error or a missed heartbeat ack), `resumes` the
// resumes tried since the last success, `described` whether the server sent the session description
// (op 4), which is what makes a session resumable.
pub(super) fn reconnect(close: Option<u16>, resumes: u32, described: bool, since_ack: Duration) -> Reconnect {
    // Authentication failed, session invalid, server not found, server crashed: Discord identifies again.
    let session_gone = matches!(close, Some(4004 | 4006 | 4011 | 4015));
    match session_gone || resumes >= RESUME_TRIES || !described || since_ack > RESUME_WITHIN {
        true => Reconnect::Identify,
        false => Reconnect::Resume,
    }
}

// Discord's backoff: each failure waits up to three times the last wait (at random), capped at `max`.
pub(super) struct Backoff {
    min: Duration,
    max: Duration,
    current: Duration,
}

impl Backoff {
    // A resuming socket's waits.
    pub(super) fn socket() -> Self {
        Self::new(Duration::from_secs(1), Duration::from_secs(5))
    }

    // A connection identifying again.
    pub(super) fn connection() -> Self {
        Self::new(Duration::from_secs(1), Duration::from_secs(10))
    }

    fn new(min: Duration, max: Duration) -> Self {
        Self { min, max, current: min }
    }

    pub(super) fn fail(&mut self) -> Duration {
        self.current = (self.current + self.current.mul_f64(2.0 * rand::random::<f64>())).min(self.max);
        self.current
    }

    pub(super) fn succeed(&mut self) {
        self.current = self.min;
    }
}

// Voice close codes (docs.discord.com, Opcodes and Status Codes; 4013 from docs.discord.food), as the
// reason a reconnect logs.
pub(super) fn close_label(code: u16) -> Option<&'static str> {
    Some(match code {
        4001 => "Unknown opcode",
        4002 => "Failed to decode payload",
        4003 => "Not authenticated",
        4004 => "Authentication failed",
        4005 => "Already authenticated",
        4006 => "Session no longer valid",
        4009 => "Session timed out",
        4011 => "Voice server not found",
        4012 => "Unknown protocol",
        4013 => "WebRTC crashed",
        4014 => "Disconnected by Discord",
        4015 => "Voice server crashed",
        4016 => "Unknown encryption mode",
        4017 => "End-to-end encryption required",
        4020 => "Bad request",
        4021 => "Rate limited by Discord",
        4022 => "The call ended",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRESH: Duration = Duration::from_secs(5);

    #[test]
    fn no_close_code_ends_the_call() {
        for code in [None, Some(1000), Some(1006), Some(4009), Some(4013), Some(4014), Some(4021), Some(4022)] {
            assert_eq!(reconnect(code, 0, true, FRESH), Reconnect::Resume, "{code:?} resumes first");
        }
        for code in [4004, 4006, 4011, 4015] {
            assert_eq!(reconnect(Some(code), 0, true, FRESH), Reconnect::Identify, "{code} identifies again");
        }
    }

    #[test]
    fn resume_gives_way_to_identify() {
        assert_eq!(reconnect(None, 3, true, FRESH), Reconnect::Resume);
        assert_eq!(reconnect(None, 4, true, FRESH), Reconnect::Identify, "four failed resumes");
        assert_eq!(reconnect(None, 0, false, FRESH), Reconnect::Identify, "no session described yet");
        assert_eq!(reconnect(None, 0, true, Duration::from_secs(61)), Reconnect::Identify, "no ack for a minute");
    }

    #[test]
    fn backoff_grows_to_its_cap_and_resets() {
        let mut backoff = Backoff::connection();
        let waits: Vec<Duration> = (0..40).map(|_| backoff.fail()).collect();
        assert!(waits.iter().all(|w| (Duration::from_secs(1)..=Duration::from_secs(10)).contains(w)));
        assert!(waits.windows(2).all(|w| w[0] <= w[1]), "never shrinks while failing");
        backoff.succeed();
        assert!(backoff.fail() <= Duration::from_secs(3));
    }
}
