// Checks the vendored gpui patch that times scroll momentum and wheel easing to the display (gpui-pre's own
// lib tests don't build standalone). Drop with the vendored gpui once upstream ships the same behaviour.
use std::time::{Duration, Instant};

use gpui_kit::{Point, ScrollInput, ScrollMotion, TouchPhase, point, px};

// macOS's momentum decays with this time constant (see scroll_motion.rs).
const MOMENTUM_TIME_CONSTANT: f32 = 0.325;

const FRAME: Duration = Duration::from_micros(8_333);

fn fling(motion: &mut ScrollMotion, start: Instant, per_frame: f32, frames: u32) -> Instant {
    motion.input(Point::default(), ScrollInput::Precise, TouchPhase::Started, true, start);
    for frame in 1..=frames {
        motion.input(point(px(0.), px(per_frame)), ScrollInput::Precise, TouchPhase::Moved, true, start + FRAME * frame);
    }
    let lift = start + FRAME * frames;
    motion.input(Point::default(), ScrollInput::Precise, TouchPhase::Ended, true, lift);
    lift
}

#[test]
fn momentum_moves_evenly_per_frame_whatever_the_system_sends() {
    let mut motion = ScrollMotion::default();
    let lift = fling(&mut motion, Instant::now(), -60., 10);
    // The system's momentum, skipping ticks and doubling up, is swallowed.
    for (ms, dy) in [(9, -146.), (18, -72.), (44, -130.)] {
        assert_eq!(motion.input(point(px(0.), px(dy)), ScrollInput::Precise, TouchPhase::Moved, true, lift + Duration::from_millis(ms)), Point::default());
    }
    let mut previous = f32::INFINITY;
    motion.step(lift);
    for frame in 1..60 {
        let (delta, _) = motion.step(lift + FRAME * frame);
        let moved = -f32::from(delta.y);
        assert!(moved > 0. && moved < previous, "frame {frame}: {moved} after {previous}");
        previous = moved;
    }
}

#[test]
fn momentum_carries_speed_times_the_time_constant() {
    let mut motion = ScrollMotion::default();
    let lift = fling(&mut motion, Instant::now(), -20., 10);
    let mut total = 0f32;
    motion.step(lift);
    for frame in 1..600 {
        total += f32::from(motion.step(lift + FRAME * frame).0.y);
    }
    let speed = 20. / FRAME.as_secs_f32();
    assert!((total + speed * MOMENTUM_TIME_CONSTANT).abs() < 1., "travelled {total}");
    assert!(!motion.is_moving());
}

#[test]
fn resting_before_lifting_does_not_fling_and_a_touch_stops_momentum() {
    let mut motion = ScrollMotion::default();
    let start = Instant::now();
    let lift = fling(&mut motion, start, -20., 10);
    assert!(motion.is_coasting());
    motion.input(Point::default(), ScrollInput::Precise, TouchPhase::Started, true, lift + FRAME);
    assert!(!motion.is_moving());

    let mut motion = ScrollMotion::default();
    motion.input(Point::default(), ScrollInput::Precise, TouchPhase::Started, true, start);
    motion.input(point(px(0.), px(-20.)), ScrollInput::Precise, TouchPhase::Moved, true, start + FRAME);
    motion.input(point(px(0.), px(-20.)), ScrollInput::Precise, TouchPhase::Moved, true, start + FRAME * 2);
    motion.input(Point::default(), ScrollInput::Precise, TouchPhase::Ended, true, start + FRAME * 2 + Duration::from_millis(80));
    assert!(!motion.is_moving());
}

#[test]
fn a_notch_eases_to_its_target_and_the_next_retargets() {
    let mut motion = ScrollMotion::default();
    let start = Instant::now();
    assert_eq!(motion.input(point(px(0.), px(-60.)), ScrollInput::Notched, TouchPhase::Moved, true, start), Point::default());
    motion.step(start);
    let first: f32 = motion.step(start + FRAME).0.y.into();
    assert!(first < 0. && first > -60.);
    motion.input(point(px(0.), px(-60.)), ScrollInput::Notched, TouchPhase::Moved, true, start + FRAME);
    let mut total = first;
    for frame in 2..120 {
        total += f32::from(motion.step(start + FRAME * frame).0.y);
    }
    assert!((total + 120.).abs() < 1e-3, "travelled {total}");
    assert!(!motion.is_moving());
    // With reduced motion a notch jumps.
    assert_eq!(motion.input(point(px(0.), px(-60.)), ScrollInput::Notched, TouchPhase::Moved, false, start), point(px(0.), px(-60.)));
}

#[test]
fn precise_scrolling_without_phases_still_applies() {
    let mut motion = ScrollMotion::default();
    let delta = point(px(0.), px(-12.));
    assert_eq!(motion.input(delta, ScrollInput::Precise, TouchPhase::Moved, true, Instant::now()), delta);
}
