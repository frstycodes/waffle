//! Settings for how the framework's own motion behaves: tooltips and scrolling. The framework ships neutral
//! defaults (the upstream tooltip delay, no entrance, the default coasting feel); an app that wants its own
//! calls [`set_tooltip_feel`] and [`set_scroll_feel`] once at startup. Style decisions belong to the app, never
//! to constants in here.

use crate::Pixels;
use std::{sync::RwLock, time::Duration};

/// How a tooltip looks `t` (0 to 1) of the way through its entrance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TooltipEnter {
    /// 0 to 1.
    pub opacity: f32,
    /// 1 is full size.
    pub scale: f32,
    /// Radians, about the point on the tooltip that sits over its element.
    pub rotation: f32,
}

impl TooltipEnter {
    /// Fully shown: nothing left to animate.
    pub const SETTLED: Self = Self { opacity: 1., scale: 1., rotation: 0. };
}

/// When tooltips show and hide, and how they enter.
#[derive(Clone, Copy, Debug)]
pub struct TooltipFeel {
    /// How long an element is hovered before its tooltip shows.
    pub show_delay: Duration,
    /// How long a hoverable tooltip waits for the pointer to reach it before hiding.
    pub hide_delay: Duration,
    /// The space between an element and its tooltip, on the side the tooltip sits.
    pub gap: Pixels,
    /// How long the entrance runs; zero shows the tooltip at once.
    pub entrance: Duration,
    /// A tooltip shown within this long of the last closing carries on from it: no delay, no new entrance.
    pub warm: Duration,
    /// A closed tooltip stays drawn this long, so crossing the gap between two elements doesn't blank it.
    pub linger: Duration,
    /// The entrance's look at `t`, 0 to 1.
    pub enter: fn(f32) -> TooltipEnter,
}

impl TooltipFeel {
    const UPSTREAM: Self = Self {
        show_delay: Duration::from_millis(500),
        hide_delay: Duration::from_millis(500),
        gap: Pixels(0.),
        entrance: Duration::ZERO,
        warm: Duration::ZERO,
        linger: Duration::ZERO,
        enter: |_| TooltipEnter::SETTLED,
    };
}

impl Default for TooltipFeel {
    fn default() -> Self {
        Self::UPSTREAM
    }
}

/// Momentum, wheel easing and the rubber band at the ends of a scroll container.
#[derive(Clone, Copy, Debug)]
pub struct ScrollFeel {
    /// How long momentum takes to slow to 1/e of its speed.
    pub momentum_time_constant: f32,
    /// Below this release speed (px/s) a lift is a stop, not a fling.
    pub min_fling_speed: f32,
    /// The finger's speed at release is measured over its last moves within this window (s)...
    pub velocity_window: f32,
    /// ...and is zero if it rested this long (s) before lifting.
    pub rest_before_lift: f32,
    /// After a lift the system's own momentum events keep coming; one gap longer than this (s) ends them.
    pub system_momentum_gap: f32,
    /// A notch's ease: the remaining distance shrinks to 1/e every this long (s).
    pub wheel_time_constant: f32,
    /// How far one wheel line scrolls (px).
    pub wheel_line: f32,
    /// How hard the rubber band pulls back against a trackpad drag past an edge.
    pub trackpad_coefficient: f32,
    /// The rebound spring's stiffness.
    pub rebound_stiffness: f32,
    /// The rebound spring's amplitude.
    pub rebound_amplitude: f32,
    /// The rebound spring's period.
    pub rebound_period: f32,
    /// A drag that rested this long (s) has no velocity to hand to the rebound.
    pub velocity_zeroing_timeout: f32,
}

impl Default for ScrollFeel {
    fn default() -> Self {
        Self {
            momentum_time_constant: 0.325,
            min_fling_speed: 60.,
            velocity_window: 0.1,
            rest_before_lift: 0.05,
            system_momentum_gap: 0.15,
            wheel_time_constant: 0.035,
            wheel_line: 100. / 3.,
            trackpad_coefficient: 0.075,
            rebound_stiffness: 20.,
            rebound_amplitude: 0.31,
            rebound_period: 1.6,
            velocity_zeroing_timeout: 0.1,
        }
    }
}

static TOOLTIP: RwLock<TooltipFeel> = RwLock::new(TooltipFeel::UPSTREAM);
static SCROLL: RwLock<Option<ScrollFeel>> = RwLock::new(None);

/// Sets how tooltips time and enter, for every window.
pub fn set_tooltip_feel(feel: TooltipFeel) {
    *TOOLTIP.write().unwrap_or_else(|e| e.into_inner()) = feel;
}

/// How tooltips time and enter.
pub fn tooltip_feel() -> TooltipFeel {
    *TOOLTIP.read().unwrap_or_else(|e| e.into_inner())
}

/// Sets how scroll containers coast, ease and rebound, for every window.
pub fn set_scroll_feel(feel: ScrollFeel) {
    *SCROLL.write().unwrap_or_else(|e| e.into_inner()) = Some(feel);
}

/// How scroll containers coast, ease and rebound.
pub fn scroll_feel() -> ScrollFeel {
    SCROLL.read().unwrap_or_else(|e| e.into_inner()).unwrap_or_default()
}
