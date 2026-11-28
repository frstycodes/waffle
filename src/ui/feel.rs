// How Waffle's tooltips, scrolling and links behave: the values the framework (vendor/) used to hard-code, set once
// at startup. Style and feel decisions live here, never in the framework.
use std::time::Duration;

use gpui_kit::{Pixels, ScrollFeel, TooltipEnter, TooltipFeel, px, set_scroll_feel, set_tooltip_feel};

// A tooltip shows 50ms after its element is hovered: long enough that a pointer passing over doesn't flash one, short
// enough to read as instant. A neighbour of one already up shows at once, and a closed one holds for a moment so
// crossing the gap between two elements doesn't blank it. It enters as every floating surface does (`ui::rise`): a fade
// with a slight grow from 0.97, no overshoot, no tilt.
const TOOLTIP_SHOW_DELAY: Duration = Duration::from_millis(50);
const TOOLTIP_HIDE_DELAY: Duration = Duration::from_millis(500);
// The space between an element and its tooltip or hover card: the framework places both this far from it.
const TOOLTIP_GAP: Pixels = px(8.);
const TOOLTIP_ENTRANCE: Duration = Duration::from_millis(150);
const TOOLTIP_WARM: Duration = Duration::from_millis(300);
const TOOLTIP_LINGER: Duration = Duration::from_millis(120);

// Installs Waffle's feel into the framework. Call once, before the first window opens.
// Keywords: feel, tooltip delay, tooltip entrance, scroll momentum, wheel, rubber band, overscroll, link underline
pub fn install() {
    set_tooltip_feel(TooltipFeel {
        show_delay: TOOLTIP_SHOW_DELAY,
        hide_delay: TOOLTIP_HIDE_DELAY,
        gap: TOOLTIP_GAP,
        entrance: TOOLTIP_ENTRANCE,
        warm: TOOLTIP_WARM,
        linger: TOOLTIP_LINGER,
        enter: |t| {
            let eased = 1. - (1. - t).powi(3);
            TooltipEnter { opacity: eased, scale: 0.97 + 0.03 * eased, ..TooltipEnter::SETTLED }
        },
    });
    // Scrolling: macOS-like momentum fitted to its own events (an 8.3ms-tick log of a fling halved its per-tick distance
    // over 0.23s), a notch easing in ~150ms and travelling 100px as Chromium (and Discord) scroll, and AppKit's rubber
    // band at the ends (its private elastic-delta curves, constants read back from AppKit).
    set_scroll_feel(ScrollFeel {
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
    });
    // Links are told apart by their colour alone, never underlined.
    gpui_kit::base::text::set_link_underline(false);
}
