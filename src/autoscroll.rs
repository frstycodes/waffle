// A scripted scroll for profiling without a hand on the trackpad: with WAFFLE_AUTOSCROLL set, the open chat's feed
// (`feed`, the default) or the sidebar's list (`nav`) scrolls up and down by itself for a while once the app has
// loaded. Pair it with WAFFLE_PERF=1 and a sampler. Off (the default), nothing is started.
use std::time::Duration;

use gpui_kit::{Context, Entity, px};

use crate::{Waffle, part::Column};

// Time for sign-in and the first channel to load before the script starts.
const WARM_UP: Duration = Duration::from_secs(10);
const RUN: Duration = Duration::from_secs(40);
// Faster than any display, so the frame rate measured is the app's, not the script's.
const TICK: Duration = Duration::from_millis(1);
// How fast the script scrolls (a quick fling), and how long one direction lasts before turning back.
const SPEED: f32 = 6000.;
const LEG: Duration = Duration::from_millis(1500);

pub fn start<V: 'static>(app: &Entity<Waffle>, cx: &mut Context<V>) {
    let Some(target) = std::env::var("WAFFLE_AUTOSCROLL").ok() else { return };
    let nav = target == "nav";
    let app = app.downgrade();
    cx.spawn(async move |_, cx| {
        cx.background_executor().timer(WARM_UP).await;
        let started = std::time::Instant::now();
        let mut last = started;
        while started.elapsed() < RUN {
            cx.background_executor().timer(TICK).await;
            let now = std::time::Instant::now();
            let distance = SPEED * now.duration_since(last).as_secs_f32();
            last = now;
            let down = (started.elapsed().as_millis() / LEG.as_millis()).is_multiple_of(2);
            let step = if down { distance } else { -distance };
            let alive = app.update(cx, |this, cx| {
                if nav {
                    let at = this.channels_scroll.offset();
                    // lint-allow(raw-px): the step is computed from the time since the last tick.
                    this.channels_scroll.set_offset(gpui_kit::point(at.x, (at.y - px(step)).min(gpui_kit::Pixels::ZERO)));
                    this.redraw(Column::Nav, cx);
                } else {
                    this.list.scroll_by(px(step));
                    this.redraw(Column::Main, cx);
                }
            });
            if alive.is_err() {
                break;
            }
        }
        eprintln!("[perf] autoscroll finished");
    })
    // lint-allow(detached-task): ends itself after RUN, or as soon as the app is gone.
    .detach();
}
