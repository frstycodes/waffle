//! Scroll motion timed to the display rather than to input events, on every platform.
//!
//! macOS delivers momentum ("coasting") scroll events out of phase with the display: every so often a
//! tick is skipped and the next event carries twice the distance, so applying events as they arrive
//! moves the content 0 px one frame and double the next. WebKit's `MomentumEventDispatcher` swallows
//! the system's momentum and generates its own from the finger's last speed, sampled at each display
//! refresh; this does the same. Windows' Direct Manipulation inertia is treated as the system's
//! momentum too, so both platforms coast alike. Notched mouse wheels, which jump a few lines per
//! click, ease to their target instead, as Chromium's do.
//!
//! A scroll container feeds each wheel event to [`ScrollMotion::input`], applies what it returns at
//! once (the finger's own movement), and once per frame applies what [`ScrollMotion::step`] returns.

use crate::{Pixels, Point, TouchPhase, point, px};
use std::{collections::VecDeque, time::Instant};

/// Motion ends once less than this is left to travel.
const REST_DISTANCE: f32 = 0.5;
const SAMPLE_LIMIT: usize = 9;
/// How far one wheel line scrolls, from the scroll feel.
pub(crate) fn wheel_line() -> crate::Pixels {
    crate::Pixels(crate::scroll_feel().wheel_line)
}

#[derive(Clone, Copy, Debug)]
struct Momentum {
    velocity: Point<f32>,
    start: Instant,
    travelled: Point<f32>,
}

impl Momentum {
    /// Distance travelled `t` seconds in: `v·τ·(1 − e^(−t/τ))`.
    fn travelled_at(&self, now: Instant) -> Point<f32> {
        let t = now.saturating_duration_since(self.start).as_secs_f32();
        let covered = crate::scroll_feel().momentum_time_constant * (1. - (-t / crate::scroll_feel().momentum_time_constant).exp());
        point(self.velocity.x * covered, self.velocity.y * covered)
    }

    fn remaining(&self) -> f32 {
        let total = point(self.velocity.x * crate::scroll_feel().momentum_time_constant, self.velocity.y * crate::scroll_feel().momentum_time_constant);
        (total.x - self.travelled.x).abs().max((total.y - self.travelled.y).abs())
    }
}

/// One scroll container's momentum and wheel easing.
#[derive(Debug, Default)]
pub struct ScrollMotion {
    touching: bool,
    samples: VecDeque<(Instant, Point<f32>)>,
    momentum: Option<Momentum>,
    /// When the system's momentum events, ignored in favour of [`Momentum`], were last seen.
    system_momentum: Option<Instant>,
    wheel_remaining: Point<f32>,
    last_step: Option<Instant>,
}

/// What a wheel event is, as far as motion goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollInput {
    /// A trackpad's pixel deltas (or any device reporting them).
    Precise,
    /// A notched wheel's lines, already turned into pixels.
    Notched,
}

impl ScrollMotion {
    /// Feeds one wheel event's `delta` (pixels), returning the part to apply now. Finger movement
    /// applies at once; momentum and notches come out of [`Self::step`] instead.
    pub fn input(&mut self, delta: Point<Pixels>, input: ScrollInput, phase: TouchPhase, eased: bool, now: Instant) -> Point<Pixels> {
        let delta = point(delta.x.0, delta.y.0);
        if input == ScrollInput::Notched {
            self.momentum = None;
            if !eased {
                return point(px(delta.x), px(delta.y));
            }
            self.wheel_remaining = point(self.wheel_remaining.x + delta.x, self.wheel_remaining.y + delta.y);
            return Point::default();
        }
        match phase {
            TouchPhase::Started => {
                self.touching = true;
                self.momentum = None;
                self.system_momentum = None;
                self.wheel_remaining = Point::default();
                self.samples.clear();
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                self.touching = false;
                self.system_momentum = Some(now);
                let velocity = self.release_velocity(now);
                self.samples.clear();
                if velocity.x.abs().max(velocity.y.abs()) >= crate::scroll_feel().min_fling_speed && phase == TouchPhase::Ended {
                    self.momentum = Some(Momentum { velocity, start: now, travelled: Point::default() });
                    self.last_step = None;
                }
                return point(px(delta.x), px(delta.y));
            }
            TouchPhase::Moved if !self.touching => {
                // The system's momentum, swallowed while it keeps coming after a lift.
                if self.system_momentum.is_some_and(|seen| now.saturating_duration_since(seen).as_secs_f32() < crate::scroll_feel().system_momentum_gap) {
                    self.system_momentum = Some(now);
                    return Point::default();
                }
                self.system_momentum = None;
                return point(px(delta.x), px(delta.y));
            }
            TouchPhase::Moved => {}
        }
        if self.touching {
            self.samples.push_back((now, delta));
            while self.samples.len() > SAMPLE_LIMIT {
                self.samples.pop_front();
            }
        }
        point(px(delta.x), px(delta.y))
    }

    /// The finger's speed (px/s) over its last moves, zero if it rested before lifting.
    fn release_velocity(&self, now: Instant) -> Point<f32> {
        let recent: Vec<_> = self.samples.iter().filter(|(at, _)| now.saturating_duration_since(*at).as_secs_f32() <= crate::scroll_feel().velocity_window).collect();
        let (Some(first), Some(last)) = (recent.first(), recent.last()) else { return Point::default() };
        if now.saturating_duration_since(last.0).as_secs_f32() > crate::scroll_feel().rest_before_lift || recent.len() < 2 {
            return Point::default();
        }
        // The distance after the first sample over the time it took: the first only marks the start.
        let span = last.0.saturating_duration_since(first.0).as_secs_f32().max(1. / 240.);
        let moved = recent[1..].iter().fold(Point::default(), |sum: Point<f32>, (_, d)| point(sum.x + d.x, sum.y + d.y));
        point(moved.x / span, moved.y / span)
    }

    /// Advances to the frame being drawn at `now`, returning the distance to move this frame and
    /// whether another frame is needed.
    pub fn step(&mut self, now: Instant) -> (Point<Pixels>, bool) {
        let dt = self.last_step.map_or(0., |last| now.saturating_duration_since(last).as_secs_f32());
        self.last_step = Some(now);
        let mut delta = Point::<f32>::default();
        if let Some(momentum) = &mut self.momentum {
            let travelled = momentum.travelled_at(now);
            delta = point(travelled.x - momentum.travelled.x, travelled.y - momentum.travelled.y);
            momentum.travelled = travelled;
            if momentum.remaining() < REST_DISTANCE {
                self.momentum = None;
            }
        }
        if self.wheel_remaining != Point::default() {
            let taken = 1. - (-dt / crate::scroll_feel().wheel_time_constant).exp();
            let mut part = point(self.wheel_remaining.x * taken, self.wheel_remaining.y * taken);
            let left = point(self.wheel_remaining.x - part.x, self.wheel_remaining.y - part.y);
            if left.x.abs().max(left.y.abs()) < REST_DISTANCE {
                part = self.wheel_remaining;
            }
            self.wheel_remaining = point(self.wheel_remaining.x - part.x, self.wheel_remaining.y - part.y);
            delta = point(delta.x + part.x, delta.y + part.y);
        }
        (point(px(delta.x), px(delta.y)), self.is_moving())
    }

    /// Whether momentum or a notch's ease is still under way.
    pub fn is_moving(&self) -> bool {
        self.momentum.is_some() || self.wheel_remaining != Point::default()
    }

    /// Whether momentum (as opposed to a finger or a notch) is moving the content.
    pub fn is_coasting(&self) -> bool {
        self.momentum.is_some()
    }

    /// Stops any motion: the content hit an edge, or something moved it on purpose.
    pub fn stop(&mut self) {
        self.momentum = None;
        self.wheel_remaining = Point::default();
    }
}
