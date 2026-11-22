// Checks the vendored gpui patch that keeps one tooltip up at a time: showing a tooltip dismisses another
// still waiting out its hoverable hide delay. Without it, the window draws whichever request comes last in
// paint order, so moving up a column of hoverable tooltips (the server rail) showed each tile left behind
// for its full delay before the hovered one appeared. Drop with the vendored gpui once upstream does the same.
use std::{cell::RefCell, rc::Rc, time::Duration};

use gpui_kit::prelude::*;
use gpui_kit::{AnyView, App, Context, MouseMoveEvent, Pixels, Point, TestAppContext, VisualTestContext, Window, div, point, px};

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
            div().id(id).size(px(50.)).tooltip_show_delay(Duration::ZERO).hoverable_tooltip(move |_: &mut Window, cx: &mut App| -> AnyView {
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

fn move_to(cx: &mut VisualTestContext, position: Point<Pixels>) {
    cx.simulate_event(MouseMoveEvent { position, pressed_button: None, modifiers: Default::default() });
}

fn wait(cx: &mut VisualTestContext, ms: u64) {
    cx.executor().advance_clock(Duration::from_millis(ms));
    cx.run_until_parked();
}
