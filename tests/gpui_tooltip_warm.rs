// Checks the vendored gpui patch that lets a tooltip follow the pointer from one element to the next: once
// one shows, its neighbour's shows at once instead of after the hover delay. Tooltips show at once by
// default (DEFAULT_TOOLTIP_SHOW_DELAY); these buttons ask for a delay, as an element still can. Drop with the vendored gpui
// once upstream ships the same behaviour.
use std::{cell::RefCell, rc::Rc, time::Duration};

use gpui_kit::prelude::*;
use gpui_kit::{AnyView, App, Context, MouseMoveEvent, Pixels, Point, TestAppContext, VisualTestContext, Window, div, point, px};

type Shown = Rc<RefCell<Vec<&'static str>>>;

struct Buttons {
    shown: Shown,
}

struct Tip;

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size(px(40.)).child("tip")
    }
}

impl Render for Buttons {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let button = |id: &'static str, shown: Shown| {
            div().id(id).size(px(100.)).tooltip_show_delay(Duration::from_millis(500)).tooltip(move |_: &mut Window, cx: &mut App| -> AnyView {
                shown.borrow_mut().push(id);
                cx.new(|_| Tip).into()
            })
        };
        div().size_full().flex().child(button("a", self.shown.clone())).child(button("b", self.shown.clone()))
    }
}

#[gpui_kit::test]
fn a_neighbour_shows_at_once_while_warm_and_after_the_delay_when_cold(cx: &mut TestAppContext) {
    let shown: Shown = Rc::default();
    let (_, cx) = cx.add_window_view({
        let shown = shown.clone();
        move |_, _| Buttons { shown }
    });

    move_to(cx, point(px(50.), px(50.)));
    wait(cx, 100);
    assert!(shown.borrow().is_empty(), "the first tooltip waits for the hover delay");
    wait(cx, 500);
    assert_eq!(*shown.borrow(), ["a"]);

    move_to(cx, point(px(150.), px(50.)));
    wait(cx, 0);
    assert_eq!(*shown.borrow(), ["a", "b"], "the neighbour shows without the delay");

    move_to(cx, point(px(150.), px(300.)));
    wait(cx, 600);
    move_to(cx, point(px(50.), px(50.)));
    wait(cx, 100);
    assert_eq!(*shown.borrow(), ["a", "b"], "cold again once nothing has shown for a while");
}

fn move_to(cx: &mut VisualTestContext, position: Point<Pixels>) {
    cx.simulate_event(MouseMoveEvent { position, pressed_button: None, modifiers: Default::default() });
}

fn wait(cx: &mut VisualTestContext, ms: u64) {
    cx.executor().advance_clock(Duration::from_millis(ms));
    cx.run_until_parked();
}
