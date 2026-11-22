// Checks the vendored gpui patch that shows and hides a tooltip by re-rendering only the view that owns it.
// Upstream refreshes the whole window, which makes every cached view re-render: in the app, each tooltip
// along the server rail rebuilt all three columns twice (hide, then show). Drop with the vendored gpui
// once upstream does the same.
use std::{cell::RefCell, rc::Rc, time::Duration};

use gpui_kit::prelude::*;
use gpui_kit::{AnyView, App, Context, Entity, MouseMoveEvent, Pixels, Point, StyleRefinement, TestAppContext, VisualTestContext, Window, div, point, px};

type Shown = Rc<RefCell<usize>>;

struct Tip;

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size(px(40.)).child("tip")
    }
}

// A column holding one element with a tooltip.
struct Tipped {
    renders: usize,
    shown: Shown,
}

impl Render for Tipped {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        let shown = self.shown.clone();
        div().size_full().child(div().id("tile").size(px(100.)).tooltip_show_delay(Duration::ZERO).tooltip(move |_: &mut Window, cx: &mut App| -> AnyView {
            *shown.borrow_mut() += 1;
            cx.new(|_| Tip).into()
        }))
    }
}

// The column beside it, which the tooltip has nothing to do with.
struct Sibling {
    renders: usize,
}

impl Render for Sibling {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        div().size_full()
    }
}

struct Columns {
    tipped: Entity<Tipped>,
    sibling: Entity<Sibling>,
}

impl Render for Columns {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let column = StyleRefinement::default().w(px(200.)).h_full();
        div()
            .size_full()
            .flex()
            .child(AnyView::from(self.tipped.clone()).cached(column.clone()))
            .child(AnyView::from(self.sibling.clone()).cached(column))
    }
}

#[gpui_kit::test]
fn a_tooltip_re_renders_only_its_own_view(cx: &mut TestAppContext) {
    let shown: Shown = Rc::default();
    let (columns, cx) = cx.add_window_view({
        let shown = shown.clone();
        move |_, cx| Columns { tipped: cx.new(|_| Tipped { renders: 0, shown }), sibling: cx.new(|_| Sibling { renders: 0 }) }
    });
    cx.run_until_parked();
    let (tipped, sibling) = columns.read_with(cx, |c, _| (c.tipped.clone(), c.sibling.clone()));
    let renders = |cx: &mut VisualTestContext| (tipped.read_with(cx, |t, _| t.renders), sibling.read_with(cx, |s, _| s.renders));
    assert_eq!(renders(cx), (1, 1));

    move_to(cx, point(px(50.), px(50.)));
    wait(cx, 0);
    assert_eq!(*shown.borrow(), 1, "the tooltip shows");
    assert_eq!(renders(cx).1, 1, "showing it leaves the other column replaying");

    // Its entrance animates over the next frames; the window repaints the tooltip, no view re-renders.
    let shown_at = renders(cx);
    for _ in 0..6 {
        frame(cx, 60);
    }
    assert_eq!(renders(cx), shown_at, "the entrance re-renders neither column");

    move_to(cx, point(px(300.), px(50.)));
    wait(cx, 0);
    assert_eq!(renders(cx).1, 1, "hiding it leaves the other column replaying");

    // The closed tooltip lingers a moment, still without re-rendering anything.
    let hidden_at = renders(cx);
    for _ in 0..4 {
        frame(cx, 40);
    }
    assert_eq!(renders(cx), hidden_at, "the linger re-renders neither column");

    move_to(cx, point(px(50.), px(50.)));
    wait(cx, 0);
    assert_eq!(*shown.borrow(), 2, "it shows again on the way back");
    assert_eq!(renders(cx).1, 1);
}

fn move_to(cx: &mut VisualTestContext, position: Point<Pixels>) {
    cx.simulate_event(MouseMoveEvent { position, pressed_button: None, modifiers: Default::default() });
}

// One platform frame `ms` after the last: what an animation asked for on the next frame runs, then the window draws.
fn frame(cx: &mut VisualTestContext, ms: u64) {
    cx.executor().advance_clock(Duration::from_millis(ms));
    cx.update(|window, cx| window.simulate_next_frame(cx));
    cx.run_until_parked();
}

fn wait(cx: &mut VisualTestContext, ms: u64) {
    cx.executor().advance_clock(Duration::from_millis(ms));
    cx.run_until_parked();
}
