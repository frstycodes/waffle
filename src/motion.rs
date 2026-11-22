use std::time::Duration;

use gpui_kit::base::{Easing as Curve, LinearStop};
use gpui_motion_kit::core::{Bezier, Easing, Spring, Transition};

// Critically damped and quick (Apple's damping 1.0, ~220ms): settles without wobble and retargets from
// its live value, so it can be interrupted mid-flight.
pub fn spring() -> Transition {
    Transition::Spring(Spring::visual(Duration::from_millis(220), 0.).expect("valid spring"))
}

// SwiftUI's `.snappy` bounce (0.15) on `spring`'s 220ms: its own 0.5s read as slow.
pub fn snappy() -> Transition {
    Transition::Spring(Spring::visual(Duration::from_millis(220), 0.15).expect("valid spring"))
}

// Folds are one block, height and fade together, on the house enter curve.
pub fn fold() -> Transition {
    Transition::Tween(Duration::from_millis(140), Easing::Bezier(Bezier::new(0.23, 1., 0.32, 1.).expect("monotonic bezier")))
}

// Ease-out-back: a one-shot pop for gpui's built-in animations, overshooting ~10% before settling.
pub fn back_out(t: f32) -> f32 {
    let (c1, c3) = (1.70158, 2.70158);
    1. + c3 * (t - 1.).powi(3) + c1 * (t - 1.).powi(2)
}

// A refusal's horizontal shake, in px at progress `t` of `SHAKE`: three swings that die out by the end.
pub const SHAKE: Duration = Duration::from_millis(260);

pub fn shake(t: f32) -> f32 {
    (t * std::f32::consts::PI * 6.).sin() * (1. - t) * 5.
}

// The design's curves (design/tokens.css), for `gpui_kit::base::transition`: enter and exit, on-screen
// movement, and the touch spring.
pub fn ease_out() -> Curve {
    Curve::cubic_bezier(0.23, 1., 0.32, 1.).expect("valid bezier")
}

pub fn ease_move() -> Curve {
    Curve::cubic_bezier(0.77, 0., 0.175, 1.).expect("valid bezier")
}

// `--ease-spring` stop for stop: ~0.3 bounce that settles inside the duration it runs over.
pub fn ease_spring() -> Curve {
    thread_local! {
        static SPRING: Curve = Curve::linear_stops([
            LinearStop::new(0.),
            LinearStop::at(0.42, 0.12),
            LinearStop::at(0.82, 0.25),
            LinearStop::at(1.06, 0.40),
            LinearStop::at(1.1, 0.48),
            LinearStop::at(1.04, 0.62),
            LinearStop::at(0.99, 0.78),
            LinearStop::new(1.),
        ])
        .expect("ordered stops");
    }
    SPRING.with(Clone::clone)
}

#[cfg(test)]
mod tests {
    #[test]
    fn back_out_overshoots_then_lands() {
        assert_eq!(super::back_out(0.), 0.);
        assert!((super::back_out(1.) - 1.).abs() < 1e-6);
        let peak = (1..10).map(|i| super::back_out(i as f32 / 10.)).fold(0., f32::max);
        assert!(peak > 1.05 && peak < 1.15, "peak {peak}");
    }

    #[test]
    fn touch_spring_peaks_at_the_design_stop_then_lands() {
        let curve = super::ease_spring();
        assert!((curve.sample(0.48) - 1.1).abs() < 1e-4);
        assert!((curve.sample(1.) - 1.).abs() < 1e-6);
    }
}
