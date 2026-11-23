//! Rubber-band overscroll for scroll containers, after AppKit's `NSScrollView`.
//!
//! The container's real scroll offset stays clamped to its content; [`Overscroll`] holds a
//! separate, purely visual offset that the container adds when it positions its children.
//!
//! The curves are AppKit's private `_NSHyperbolicElasticDeltaForReboundDelta` and
//! `_NSElasticDeltaForTimeDelta`, which WebKit's `ScrollingEffectsController.mm` and
//! `ScrollAnimationRubberBand.mm` call; their constants were read back from AppKit itself.

use crate::{App, Pixels, Point, ScrollEvent, Size, TouchPhase, point, px};
use std::time::Instant;

/// AppKit's `NSScrollingBehaviorHyperbolicCoefficientTrackpad`.
const TRACKPAD_COEFFICIENT: f32 = 0.075;
const REBOUND_STIFFNESS: f32 = 20.;
const REBOUND_AMPLITUDE: f32 = 0.31;
const REBOUND_PERIOD: f32 = 1.6;
/// A gap this long between scroll events means the content had stopped: no carried velocity.
const VELOCITY_ZEROING_TIMEOUT: f32 = 0.1;
/// WebKit coalesces wheel events per frame before measuring velocity; gpui delivers them one by
/// one, and two landing microseconds apart would read as a near-infinite speed.
const MIN_EVENT_INTERVAL: f32 = 1. / 240.;
/// The rebound may head away from the edge before it turns, so it can't end this early.
const MIN_REBOUND_SECS: f32 = 0.024;
const REST_DISTANCE: f32 = 0.5;

/// AppKit's trackpad stretch: how far content past an edge is shown for `force`, the scroll
/// distance pushed past it, along an axis whose viewport is `d` long. Odd in `force`, and its
/// magnitude never reaches `d`.
pub fn rubber_band(force: f32, d: f32) -> f32 {
    if d <= 0. {
        return 0.;
    }
    (1. - 1. / (force.abs() * TRACKPAD_COEFFICIENT / d + 1.)) * d * force.signum()
}

/// The force that [`rubber_band`] maps to `shown`, so a gesture can resume a stretch that is
/// already on screen.
pub fn rubber_band_inverse(shown: f32, d: f32) -> f32 {
    if d <= 0. {
        return 0.;
    }
    let f = shown.abs().min(d * 0.999);
    f * d / ((d - f) * TRACKPAD_COEFFICIENT) * shown.signum()
}

/// AppKit's rebound: the stretch `t` seconds after letting go of `initial` while moving at
/// `velocity` px/s, positive away from the edge. It carries on outward for a moment before
/// decaying to the edge.
pub fn rebound_offset(initial: f32, velocity: f32, t: f32) -> f32 {
    (initial + velocity * t * REBOUND_AMPLITUDE) * (-t * REBOUND_STIFFNESS / REBOUND_PERIOD).exp()
}

/// Whether this scroll event may stretch past an edge. Wheel notches clamp. Trackpads stretch on
/// macOS and Windows alike: Windows' Direct Manipulation ends the touch when its inertia starts
/// (direct_manipulation.rs), giving the same phases as macOS.
pub(crate) fn rubber_bands(event: &ScrollEvent, cx: &App) -> bool {
    event.delta.precise() && !cx.reduce_motion()
}

/// Whether momentum may stretch past an edge and rebound: the same settings as [`rubber_bands`],
/// for motion stepped per frame rather than fed by an event.
pub(crate) fn coasts_into_edges(cx: &App) -> bool {
    !cx.reduce_motion()
}

#[derive(Clone, Copy, Debug)]
struct Rebound {
    initial: f32,
    velocity: f32,
    start: Instant,
}

#[derive(Clone, Copy, Debug, Default)]
struct Axis {
    shown: f32,
    rebound: Option<Rebound>,
    momentum_spent: bool,
}

impl Axis {
    fn grab(&mut self) {
        self.rebound = None;
        self.momentum_spent = false;
    }

    fn release(&mut self, velocity: f32, now: Instant) {
        if self.shown == 0. {
            return;
        }
        self.momentum_spent = true;
        self.rebound = Some(Rebound {
            initial: self.shown,
            velocity,
            start: now,
        });
    }

    /// `position` runs from `-max` to 0 and `delta` is added to it. Returns the part of `delta`
    /// the real offset takes. Pushing further out goes through [`rubber_band`]; pulling back
    /// takes the stretch in one to one, as AppKit does.
    fn drag(&mut self, position: f32, delta: f32, max: f32, d: f32) -> f32 {
        if max <= 0. || d <= 0. {
            return delta;
        }
        if self.shown * delta < 0. {
            let unwound = self.shown + delta;
            if unwound * self.shown > 0. {
                self.shown = unwound;
                return 0.;
            }
            self.shown = 0.;
            return (position + unwound).clamp(-max, 0.) - position;
        }
        let next = (position + delta).clamp(-max, 0.);
        let past_edge = position + delta - next;
        if past_edge != 0. {
            self.shown = rubber_band(rubber_band_inverse(self.shown, d) + past_edge, d);
        }
        next - position
    }

    /// Momentum carries the content to the edge; what is left over stretches past it and
    /// rebounds at the momentum's speed. The rest of that fling is ignored.
    fn coast(
        &mut self,
        position: f32,
        delta: f32,
        max: f32,
        d: f32,
        velocity: f32,
        now: Instant,
    ) -> f32 {
        if self.momentum_spent || self.rebound.is_some() {
            return 0.;
        }
        if max <= 0. || d <= 0. {
            return delta;
        }
        let next = (position + delta).clamp(-max, 0.);
        let past_edge = position + delta - next;
        if past_edge != 0. {
            self.shown = rubber_band(past_edge, d);
            self.release(velocity, now);
        }
        next - position
    }

    fn step(&mut self, now: Instant) {
        let Some(rebound) = self.rebound else {
            return;
        };
        let t = now.saturating_duration_since(rebound.start).as_secs_f32();
        let shown = rebound_offset(rebound.initial, rebound.velocity, t);
        let crossed_edge = shown * rebound.initial < 0.;
        if crossed_edge || (shown.abs() < REST_DISTANCE && t > MIN_REBOUND_SECS) {
            self.shown = 0.;
            self.rebound = None;
        } else {
            self.shown = shown;
        }
    }
}

/// One scroll container's stretch past its edges.
///
/// macOS sends the finger's scroll events between `Started` and `Ended`, then momentum as
/// phaseless `Moved` events, so a `Moved` outside a touch is momentum.
#[derive(Debug, Default)]
pub struct Overscroll {
    x: Axis,
    y: Axis,
    touching: bool,
    max: Point<Pixels>,
    viewport: Size<Pixels>,
    velocity: Point<f32>,
    last_event: Option<Instant>,
}

impl Overscroll {
    /// Records the scroll range and viewport, both measured at prepaint.
    pub fn set_extent(&mut self, max: Point<Pixels>, viewport: Size<Pixels>) {
        self.max = max;
        self.viewport = viewport;
    }

    /// The visual offset to add to the real scroll offset.
    pub fn offset(&self) -> Point<Pixels> {
        point(px(self.x.shown), px(self.y.shown))
    }

    /// Whether the stretch is rebounding, so frames must keep coming.
    pub fn is_rebounding(&self) -> bool {
        self.x.rebound.is_some() || self.y.rebound.is_some()
    }

    /// Feeds one scroll event at the real offset `position` (`-max..=0` per axis). Returns the
    /// part of `delta` to add to the real offset.
    pub fn scroll(
        &mut self,
        position: Point<Pixels>,
        delta: Point<Pixels>,
        phase: TouchPhase,
        now: Instant,
    ) -> Point<Pixels> {
        match phase {
            TouchPhase::Started => {
                self.touching = true;
                self.last_event = None;
                self.x.grab();
                self.y.grab();
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                self.touching = false;
                let velocity = self.carried_velocity(now);
                self.x.release(velocity.x, now);
                self.y.release(velocity.y, now);
                return Point::default();
            }
            TouchPhase::Moved => {}
        }
        self.track_velocity(delta, now);
        let (max, d) = (self.max, self.viewport);
        if self.touching {
            return point(
                px(self.x.drag(position.x.0, delta.x.0, max.x.0, d.width.0)),
                px(self.y.drag(position.y.0, delta.y.0, max.y.0, d.height.0)),
            );
        }
        let velocity = self.dominant_velocity();
        point(
            px(self
                .x
                .coast(position.x.0, delta.x.0, max.x.0, d.width.0, velocity.x, now)),
            px(self.y.coast(
                position.y.0,
                delta.y.0,
                max.y.0,
                d.height.0,
                velocity.y,
                now,
            )),
        )
    }

    fn track_velocity(&mut self, delta: Point<Pixels>, now: Instant) {
        let interval = self
            .last_event
            .map(|last| now.saturating_duration_since(last).as_secs_f32());
        self.velocity = match interval {
            Some(dt) if dt < VELOCITY_ZEROING_TIMEOUT => {
                let dt = dt.max(MIN_EVENT_INTERVAL);
                point(delta.x.0 / dt, delta.y.0 / dt)
            }
            _ => Point::default(),
        };
        self.last_event = Some(now);
    }

    fn carried_velocity(&self, now: Instant) -> Point<f32> {
        let stale = self.last_event.is_none_or(|last| {
            now.saturating_duration_since(last).as_secs_f32() >= VELOCITY_ZEROING_TIMEOUT
        });
        if stale {
            Point::default()
        } else {
            self.dominant_velocity()
        }
    }

    /// Like AppKit, a rebound keeps only the vertical velocity unless horizontal dominates.
    fn dominant_velocity(&self) -> Point<f32> {
        if self.velocity.y.abs() >= self.velocity.x.abs() {
            point(0., self.velocity.y)
        } else {
            self.velocity
        }
    }

    /// Advances the rebound to `now`. Returns whether another frame is needed.
    pub fn tick(&mut self, now: Instant) -> bool {
        self.x.step(now);
        self.y.step(now);
        self.is_rebounding()
    }
}
