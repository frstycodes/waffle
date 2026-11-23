// Checks that hover holds, as browsers do, while the wheel scrolls under a still pointer (gpui-fast's hover
// freeze), and that a click under it still lands.
use std::time::Duration;

use gpui_kit::prelude::*;
use gpui_kit::{
    Context, Entity, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollDelta, ScrollEvent,
    TestAppContext, TouchPhase, VisualTestContext, Window, div, point, px,
};

struct Row {
    hovered: bool,
}

impl Render for Row {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(div().id("row").size(px(200.)).on_hover(cx.listener(|this, hovered: &bool, _, cx| {
            this.hovered = *hovered;
            cx.notify();
        })))
    }
}

#[gpui_kit::test]
fn wheel_keeps_hover_under_a_still_pointer(cx: &mut TestAppContext) {
    let (row, cx) = cx.add_window_view(|_, _| Row { hovered: false });

    move_to(cx, point(px(50.), px(50.)));
    assert!(hovered(&row, cx), "pointer over the row");

    wheel(cx, TouchPhase::Moved);
    assert!(hovered(&row, cx), "one wheel step keeps hover");
    move_to(cx, point(px(50.), px(50.)));
    assert!(hovered(&row, cx), "a move that goes nowhere is the platform echoing the scroll");
    wait(cx);
    assert!(hovered(&row, cx), "still hovered once wheel events stop");

    wheel(cx, TouchPhase::Started);
    wait(cx);
    assert!(hovered(&row, cx), "a finger resting on the trackpad keeps hover");
    wheel(cx, TouchPhase::Ended);
    wait(cx);
    assert!(hovered(&row, cx));

    wheel(cx, TouchPhase::Moved);
    move_to(cx, point(px(300.), px(300.)));
    assert!(!hovered(&row, cx), "moving off the row ends hover at once");
}

const ROW_HEIGHT: f32 = 50.;

struct Rows {
    hovered: Vec<bool>,
}

impl Render for Rows {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().id("scroller").size(px(200.)).overflow_y_scroll().children((0..self.hovered.len()).map(|ix| {
            div().id(ix).h(px(ROW_HEIGHT)).w_full().on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                this.hovered[ix] = *hovered;
                cx.notify();
            }))
        }))
    }
}

// As in browsers: content scrolling under a still pointer doesn't move hover until the scroll ends.
#[gpui_kit::test]
fn hover_stays_on_the_row_it_was_on_until_the_scroll_ends(cx: &mut TestAppContext) {
    let (rows, cx) = cx.add_window_view(|_, _| Rows { hovered: vec![false; 10] });
    let hovered_rows = |cx: &mut VisualTestContext| -> Vec<usize> {
        rows.read_with(cx, |rows, _| (0..rows.hovered.len()).filter(|ix| rows.hovered[*ix]).collect())
    };

    move_to(cx, point(px(50.), px(25.)));
    assert_eq!(hovered_rows(cx), [0]);

    wheel_by(cx, px(-ROW_HEIGHT - 10.));
    assert_eq!(hovered_rows(cx), [0], "row 0 holds hover while row 1 passes under the pointer");

    wait(cx);
    assert_eq!(hovered_rows(cx), [1], "hover moves to the row under the pointer once the scroll ends");
}

struct Button {
    clicks: usize,
}

impl Render for Button {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(div().id("button").size(px(200.)).on_click(cx.listener(|this, _, _, cx| {
            this.clicks += 1;
            cx.notify();
        })))
    }
}

// A wheel or trackpad-inertia event between press and release must not drop the click.
#[gpui_kit::test]
fn a_wheel_event_between_press_and_release_keeps_the_click(cx: &mut TestAppContext) {
    let (button, cx) = cx.add_window_view(|_, _| Button { clicks: 0 });
    let at = point(px(50.), px(50.));
    move_to(cx, at);
    cx.simulate_event(MouseDownEvent { position: at, button: MouseButton::Left, modifiers: Default::default(), click_count: 1, first_mouse: false });
    wheel(cx, TouchPhase::Moved);
    cx.simulate_event(MouseUpEvent { position: at, button: MouseButton::Left, modifiers: Default::default(), click_count: 1 });
    cx.run_until_parked();
    assert_eq!(button.read_with(cx, |b, _| b.clicks), 1);
}

// gpui-fast holds hover for 100ms of wall-clock time after the last wheel event, then redraws on a timer:
// let that time pass for real, then fire the timer.
fn wait(cx: &mut VisualTestContext) {
    std::thread::sleep(Duration::from_millis(120));
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
}

fn hovered(row: &Entity<Row>, cx: &mut VisualTestContext) -> bool {
    row.read_with(cx, |row, _| row.hovered)
}

fn move_to(cx: &mut VisualTestContext, position: Point<Pixels>) {
    cx.simulate_event(MouseMoveEvent { position, pressed_button: None, modifiers: Default::default() });
    cx.run_until_parked();
}

fn wheel(cx: &mut VisualTestContext, touch_phase: TouchPhase) {
    scroll(cx, px(-20.), touch_phase);
}

fn wheel_by(cx: &mut VisualTestContext, dy: Pixels) {
    scroll(cx, dy, TouchPhase::Moved);
}

fn scroll(cx: &mut VisualTestContext, dy: Pixels, touch_phase: TouchPhase) {
    cx.simulate_event(ScrollEvent {
        position: point(px(50.), px(25.)),
        delta: ScrollDelta::Pixels(point(px(0.), dy)),
        modifiers: Default::default(),
        touch_phase,
        source: Default::default(),
    });
    cx.run_until_parked();
}
