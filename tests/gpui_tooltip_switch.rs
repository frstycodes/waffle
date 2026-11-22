// Checks the vendored gpui patch that keeps one tooltip up at a time: showing a tooltip dismisses another
// still waiting out its hoverable hide delay. Without it, the window draws whichever request comes last in
// paint order, so moving up a column of hoverable tooltips (the server rail) showed each tile left behind
// for its full delay before the hovered one appeared. Drop with the vendored gpui once upstream does the same.
use std::{cell::RefCell, rc::Rc, time::Duration};

use gpui_kit::prelude::*;
use gpui_kit::{AnyView, App, Context, MouseMoveEvent, Pixels, Point, TestAppContext, TooltipPlacement, VisualTestContext, Window, div, point, px};

type Drawn = Rc<RefCell<Vec<&'static str>>>;

struct Tip {
    id: &'static str,
    drawn: Drawn,
}

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.drawn.borrow_mut().push(self.id);
        div().size(px(40.)).child(self.id)
    }
}

// Tiles stacked top to bottom, each with a hoverable tooltip, as on the rail.
struct Rail {
    drawn: Drawn,
}

impl Render for Rail {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let tile = |id: &'static str, drawn: Drawn| {
            div().id(id).size(px(50.)).tooltip_show_delay(Duration::ZERO).tooltip_placement(TooltipPlacement::Side).hoverable_tooltip(move |_: &mut Window, cx: &mut App| -> AnyView {
                let drawn = drawn.clone();
                cx.new(|_| Tip { id, drawn }).into()
            })
        };
        div().flex().flex_col().children(["a", "b", "c"].map(|id| tile(id, self.drawn.clone())))
    }
}

#[gpui_kit::test]
fn moving_up_shows_the_hovered_tooltip_at_once(cx: &mut TestAppContext) {
    let drawn: Drawn = Rc::default();
    let (_, cx) = cx.add_window_view({
        let drawn = drawn.clone();
        move |_, _| Rail { drawn }
    });
    let showing = |drawn: &Drawn| *drawn.borrow().last().expect("a tooltip is drawn");

    move_to(cx, point(px(25.), px(125.)));
    wait(cx, 0);
    assert_eq!(showing(&drawn), "c");

    // Up the column: each tile's tooltip replaces the last at once, not after the one below has hidden.
    move_to(cx, point(px(25.), px(75.)));
    wait(cx, 0);
    assert_eq!(showing(&drawn), "b", "the tile moved onto shows, not the one left below it");
    move_to(cx, point(px(25.), px(25.)));
    wait(cx, 0);
    assert_eq!(showing(&drawn), "a");

    // And down again.
    move_to(cx, point(px(25.), px(125.)));
    wait(cx, 0);
    assert_eq!(showing(&drawn), "c");
}

// No delay set: a tooltip shows the moment its element is hovered, the first one included.
struct Plain {
    drawn: Drawn,
}

impl Render for Plain {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let drawn = self.drawn.clone();
        div().id("plain").size(px(50.)).tooltip(move |_: &mut Window, cx: &mut App| -> AnyView {
            let drawn = drawn.clone();
            cx.new(|_| Tip { id: "plain", drawn }).into()
        })
    }
}

#[gpui_kit::test]
fn a_tooltip_shows_at_once_by_default(cx: &mut TestAppContext) {
    let drawn: Drawn = Rc::default();
    let (_, cx) = cx.add_window_view({
        let drawn = drawn.clone();
        move |_, _| Plain { drawn }
    });
    move_to(cx, point(px(25.), px(25.)));
    wait(cx, 0);
    assert_eq!(drawn.borrow().last().copied(), Some("plain"), "shown with no time passed");
}

// The hoverable delay holds a tooltip only while the pointer is on its way into it.
#[gpui_kit::test]
fn a_hoverable_tooltip_closes_at_once_unless_the_pointer_heads_into_it(cx: &mut TestAppContext) {
    let drawn: Drawn = Rc::default();
    let (_, cx) = cx.add_window_view({
        let drawn = drawn.clone();
        move |_, _| Rail { drawn }
    });
    let drawn_now = |drawn: &Drawn, cx: &mut VisualTestContext| {
        drawn.borrow_mut().clear();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        drawn.borrow().last().copied()
    };

    // Down off the column, away from its tooltip (which opens to the right): it closes now, not in 500ms.
    move_to(cx, point(px(25.), px(25.)));
    wait(cx, 0);
    assert_eq!(drawn_now(&drawn, cx), Some("a"));
    move_to(cx, point(px(25.), px(300.)));
    wait(cx, 200);
    assert_eq!(drawn_now(&drawn, cx), None, "gone well inside the hover delay");

    // Across the gap into it: it stays while the pointer is on the tooltip.
    move_to(cx, point(px(25.), px(25.)));
    wait(cx, 400);
    move_to(cx, point(px(51.), px(25.)));
    wait(cx, 0);
    move_to(cx, point(px(70.), px(25.)));
    wait(cx, 600);
    assert_eq!(drawn_now(&drawn, cx), Some("a"), "the tooltip stays up while the pointer is on it");
}

fn move_to(cx: &mut VisualTestContext, position: Point<Pixels>) {
    cx.simulate_event(MouseMoveEvent { position, pressed_button: None, modifiers: Default::default() });
}

fn wait(cx: &mut VisualTestContext, ms: u64) {
    cx.executor().advance_clock(Duration::from_millis(ms));
    cx.run_until_parked();
}
