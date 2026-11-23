// Checks the vendored gpui patch's rubber-band overscroll from outside the crate: gpui-pre's own lib tests
// don't build standalone. Drop with the vendored gpui once upstream ships the feature.
use std::time::{Duration, Instant};

use gpui_kit::{Overscroll, Point, Size, TouchPhase, point, px, rebound_offset, rubber_band, rubber_band_inverse};

const D: f32 = 800.;

fn assert_close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 1e-3 * expected.abs().max(1.), "{actual} != {expected}");
}

// Expected values are AppKit's own `_NSHyperbolicElasticDeltaForReboundDelta(x, 0.075, 800)`, the trackpad
// coefficient, called on macOS 27.
#[test]
fn rubber_band_matches_appkit_trackpad_stretch() {
    assert_eq!(rubber_band(0., D), 0.);
    assert_close(rubber_band(10., D), 0.749_297_5);
    assert_close(rubber_band(100., D), 7.430_341);
    assert_close(rubber_band(400., D), 28.915_663);
    assert_close(rubber_band(1000., D), 68.571_43);
    assert_close(rubber_band(10_000., D), 387.096_77);
    assert_close(rubber_band(-100., D), -7.430_341);
}

#[test]
fn rubber_band_rises_stiffens_and_stays_below_the_viewport() {
    let mut previous = 0.;
    let mut previous_step = f32::INFINITY;
    for x in (1..2_000).map(|i| i as f32 * 10.) {
        let shown = rubber_band(x, D);
        assert!(shown > previous, "not increasing at {x}");
        assert!(shown - previous <= previous_step + 1e-4, "not stiffening at {x}");
        assert!(shown < D, "reached the viewport at {x}");
        previous_step = shown - previous;
        previous = shown;
    }
}

// AppKit's `_NSHyperbolicReboundDeltaForElasticDelta(7.430_341, 0.075, 800)` returns 100.
#[test]
fn rubber_band_inverse_matches_appkit_and_round_trips() {
    assert_close(rubber_band_inverse(7.430_341, D), 100.);
    for x in [-600., -40., 0., 3., 75., 900.] {
        let back = rubber_band_inverse(rubber_band(x, D), D);
        assert!((back - x).abs() < 0.01 * f32::max(x.abs(), 1.), "{x} came back as {back}");
    }
}

// Expected values are AppKit's `_NSElasticDeltaForTimeDelta(initial, -velocity, t)` on macOS 27: AppKit
// counts velocity towards the content, this counts it away from the edge.
#[test]
fn rebound_matches_appkit_elastic_delta_for_time_delta() {
    assert_close(rebound_offset(100., 0., 0.05), 53.526_142);
    assert_close(rebound_offset(100., 0., 0.1), 28.650_48);
    assert_close(rebound_offset(30., 0., 0.2), 2.462_55);
    assert_close(rebound_offset(0., 1000., 0.1), 8.881_649);
    assert_close(rebound_offset(0., 2000., 0.08), 18.246_82);
    assert_close(rebound_offset(10., 500., 0.1), 7.305_872);
    assert_close(rebound_offset(10., -500., 0.03), 3.676_998);
}

#[test]
fn rebound_from_rest_decays_to_the_edge_without_crossing_it() {
    for start in [-300., -12., 4., 150., 700.] {
        let mut previous = f32::abs(start);
        for frame in 1..=60 {
            let shown = rebound_offset(start, 0., frame as f32 / 120.);
            assert!(shown * start > 0., "crossed the edge from {start}");
            assert!(shown.abs() < previous, "{start} grew at frame {frame}");
            previous = shown.abs();
        }
    }
}

fn scroller(max: f32) -> Overscroll {
    let mut overscroll = Overscroll::default();
    overscroll.set_extent(point(px(0.), px(max)), Size { width: px(400.), height: px(D) });
    overscroll
}

fn settle(overscroll: &mut Overscroll, from: Instant) -> Duration {
    let mut now = from;
    while overscroll.tick(now) {
        now += Duration::from_millis(8);
        assert!(now - from < Duration::from_secs(2), "never settled");
    }
    now - from
}

const FRAME: Duration = Duration::from_millis(16);

#[test]
fn dragging_past_the_top_stretches_then_rebounds_on_release() {
    let mut overscroll = scroller(1000.);
    let t = Instant::now();
    let at_top = Point::default();
    overscroll.scroll(at_top, Point::default(), TouchPhase::Started, t);
    for frame in 1..=4 {
        let applied = overscroll.scroll(at_top, point(px(0.), px(100.)), TouchPhase::Moved, t + FRAME * frame);
        assert_eq!(applied.y, px(0.));
    }
    assert_close(overscroll.offset().y.into(), rubber_band(400., D));
    let lifted = t + FRAME * 4 + Duration::from_millis(120);
    assert!(!overscroll.tick(lifted), "rebounded while the finger was down");

    overscroll.scroll(at_top, Point::default(), TouchPhase::Ended, lifted);
    assert!(overscroll.tick(lifted + Duration::from_millis(100)));
    assert_close(overscroll.offset().y.into(), rebound_offset(rubber_band(400., D), 0., 0.1));
    let took = settle(&mut overscroll, lifted);
    assert_eq!(overscroll.offset().y, px(0.));
    assert!(took <= Duration::from_millis(400), "took {took:?}");
}

#[test]
fn letting_go_mid_pull_carries_the_stretch_further_first() {
    let mut overscroll = scroller(1000.);
    let t = Instant::now();
    let at_top = Point::default();
    overscroll.scroll(at_top, Point::default(), TouchPhase::Started, t);
    overscroll.scroll(at_top, point(px(0.), px(200.)), TouchPhase::Moved, t + FRAME);
    overscroll.scroll(at_top, point(px(0.), px(32.)), TouchPhase::Moved, t + FRAME * 2);
    let released = overscroll.offset().y;
    overscroll.scroll(at_top, Point::default(), TouchPhase::Ended, t + FRAME * 2);
    overscroll.tick(t + FRAME * 2 + Duration::from_millis(40));
    let expected = rebound_offset(released.into(), 32. / FRAME.as_secs_f32(), 0.04);
    assert_close(overscroll.offset().y.into(), expected);
    assert!(overscroll.offset().y > released);
}

#[test]
fn dragging_back_takes_the_stretch_in_one_to_one_before_scrolling() {
    let mut overscroll = scroller(1000.);
    let t = Instant::now();
    let at_top = Point::default();
    overscroll.scroll(at_top, Point::default(), TouchPhase::Started, t);
    overscroll.scroll(at_top, point(px(0.), px(300.)), TouchPhase::Moved, t + FRAME);
    let stretched: f32 = overscroll.offset().y.into();
    let applied = overscroll.scroll(at_top, point(px(0.), px(-5.)), TouchPhase::Moved, t + FRAME * 2);
    assert_eq!(applied.y, px(0.));
    assert_close(overscroll.offset().y.into(), stretched - 5.);
    let applied = overscroll.scroll(at_top, point(px(0.), px(-100.)), TouchPhase::Moved, t + FRAME * 3);
    assert_eq!(overscroll.offset().y, px(0.));
    assert_close(applied.y.into(), stretched - 105.);
}

#[test]
fn momentum_into_an_edge_bounces_at_its_speed_once() {
    let mut overscroll = scroller(1000.);
    let t = Instant::now();
    let near_bottom = point(px(0.), px(-990.));
    overscroll.scroll(near_bottom, Point::default(), TouchPhase::Started, t);
    overscroll.scroll(near_bottom, point(px(0.), px(-2.)), TouchPhase::Moved, t + FRAME);
    overscroll.scroll(near_bottom, Point::default(), TouchPhase::Ended, t + FRAME);
    let hit = t + FRAME * 2;
    let applied = overscroll.scroll(near_bottom, point(px(0.), px(-40.)), TouchPhase::Moved, hit);
    assert_eq!(applied.y, px(-10.));
    let initial = rubber_band(-30., D);
    assert_close(overscroll.offset().y.into(), initial);

    overscroll.tick(hit + Duration::from_millis(50));
    assert_close(overscroll.offset().y.into(), rebound_offset(initial, -40. / FRAME.as_secs_f32(), 0.05));
    settle(&mut overscroll, hit + Duration::from_millis(50));
    assert_eq!(overscroll.offset().y, px(0.));

    let at_bottom = point(px(0.), px(-1000.));
    let later = hit + Duration::from_secs(1);
    let applied = overscroll.scroll(at_bottom, point(px(0.), px(-5.)), TouchPhase::Moved, later);
    assert_eq!(applied.y, px(0.));
    assert!(!overscroll.tick(later + FRAME), "the same fling bounced twice");
}

#[test]
fn content_that_fits_does_not_stretch() {
    let mut overscroll = scroller(0.);
    let t = Instant::now();
    overscroll.scroll(Point::default(), Point::default(), TouchPhase::Started, t);
    let applied = overscroll.scroll(Point::default(), point(px(0.), px(80.)), TouchPhase::Moved, t);
    assert_eq!(applied.y, px(80.));
    assert_eq!(overscroll.offset(), Point::default());
}


// Rubber-banding only runs on macOS, where scroll phases tell the finger from momentum.
#[cfg(target_os = "macos")]
mod headless {
    use gpui_kit::prelude::*;
    use gpui_kit::{
        Context, ListAlignment, ListState, Pixels, ScrollDelta, ScrollEvent, TestAppContext, TouchPhase,
        VisualTestContext, Window, div, list, point, px,
    };

    struct Feed;

    impl Render for Feed {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                div()
                    .id("feed")
                    .h(px(200.))
                    .w(px(200.))
                    .overflow_y_scroll()
                    .child(div().debug_selector(|| "content".into()).h(px(1000.)).w_full()),
            )
        }
    }

    fn wheel(phase: TouchPhase, dy: f32) -> ScrollEvent {
        ScrollEvent {
            position: point(px(100.), px(100.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
            modifiers: Default::default(),
            touch_phase: phase,
            source: Default::default(),
        }
    }

    fn content_top(cx: &mut VisualTestContext) -> Pixels {
        cx.debug_bounds("content").expect("content painted").top()
    }

    #[gpui_kit::test]
    fn a_scroll_div_stretches_past_its_top_and_rebounds(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| Feed);
        let rest = content_top(cx);
        cx.simulate_event(wheel(TouchPhase::Started, 0.));
        cx.simulate_event(wheel(TouchPhase::Moved, 80.));
        cx.run_until_parked();
        let stretched = content_top(cx);
        assert!(stretched > rest, "content at {stretched:?}, rest {rest:?}");
        rest_finger();
        cx.simulate_event(wheel(TouchPhase::Ended, 0.));
        let mut previous = stretched;
        for _ in 0..50 {
            next_frame(cx);
            let top = content_top(cx);
            assert!(top <= previous && top >= rest, "{top:?} after {previous:?}");
            previous = top;
        }
        assert_eq!(content_top(cx), rest);
    }

    #[gpui_kit::test]
    fn a_bottom_aligned_list_stretches_past_its_end_and_rebounds(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| Messages(ListState::new(40, ListAlignment::Bottom, px(100.))));
        let rest = last_row_bottom(cx);
        cx.simulate_event(wheel(TouchPhase::Started, 0.));
        cx.simulate_event(wheel(TouchPhase::Moved, -80.));
        cx.run_until_parked();
        let stretched = last_row_bottom(cx);
        assert!(stretched < rest, "last row at {stretched:?}, rest {rest:?}");
        rest_finger();
        cx.simulate_event(wheel(TouchPhase::Ended, 0.));
        for _ in 0..50 {
            next_frame(cx);
        }
        assert_eq!(last_row_bottom(cx), rest);
    }

    struct Messages(ListState);

    impl Render for Messages {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                div().h(px(200.)).w(px(200.)).child(
                    list(self.0.clone(), |ix, _, _| {
                        let row = div().h(px(30.)).w_full();
                        if ix == 39 { row.debug_selector(|| "last".into()).into_any_element() } else { row.into_any_element() }
                    })
                    .size_full(),
                ),
            )
        }
    }

    // A finger held still past AppKit's velocity timeout lets go without carrying the stretch further.
    fn rest_finger() {
        std::thread::sleep(std::time::Duration::from_millis(110));
    }

    fn last_row_bottom(cx: &mut VisualTestContext) -> Pixels {
        cx.debug_bounds("last").expect("last row painted").bottom()
    }

    // The test platform has no display link: wait out a frame (the spring runs on wall-clock time), then
    // redraw as the display link would.
    fn next_frame(cx: &mut VisualTestContext) {
        std::thread::sleep(std::time::Duration::from_millis(10));
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }
}
