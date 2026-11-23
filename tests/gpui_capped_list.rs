// A popover that grows with its list up to a cap, then scrolls (Pinned Messages): a `list` that infers its height
// from its items, with the cap on it. A list lays out only the rows in view; a div of every row made each scroll
// step lay all of them out again, at ~19ms a frame for 45 pins.
use std::{cell::Cell, rc::Rc};

use gpui_kit::prelude::*;
use gpui_kit::{
    Context, ListAlignment, ListSizingBehavior, ListState, ScrollDelta, ScrollEvent, TestAppContext, TouchPhase, VisualTestContext,
    Window, canvas, div, list, point, px,
};

const HEADER: f32 = 44.;
const CAP: f32 = 500.;
const ROW: f32 = 80.;

struct Card {
    rows: ListState,
    height: Rc<Cell<f32>>,
}

impl Render for Card {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let height = self.height.clone();
        let probe = canvas(move |bounds, _, _| height.set(f32::from(bounds.size.height)), |_, _, _, _| {}).absolute().size_full();
        let rows = list(self.rows.clone(), |_, _, _| div().h(px(ROW)).into_any_element()).with_sizing_behavior(ListSizingBehavior::Infer).max_h(px(CAP));
        div().size_full().child(div().relative().w(px(420.)).flex().flex_col().child(div().flex_none().h(px(HEADER))).child(rows).child(probe))
    }
}

fn open(count: usize, cx: &mut TestAppContext) -> (&mut VisualTestContext, ListState, Rc<Cell<f32>>) {
    let (rows, height) = (ListState::new(count, ListAlignment::Top, px(CAP)), Rc::new(Cell::new(-1.)));
    let (state, probe) = (rows.clone(), height.clone());
    let (_, cx) = cx.add_window_view(move |_, _| Card { rows: state, height: probe });
    settle(cx);
    (cx, rows, height)
}

// Two passes: an inferred list learns its rows' heights in the first.
fn settle(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn short_list_sizes_the_card(cx: &mut TestAppContext) {
    let (_, _, height) = open(1, cx);
    assert_eq!(height.get(), HEADER + ROW, "header and the one row");
}

#[gpui_kit::test]
fn long_list_caps_and_scrolls(cx: &mut TestAppContext) {
    let (cx, rows, height) = open(10, cx);
    assert_eq!(height.get(), HEADER + CAP, "capped");
    let delta = ScrollDelta::Pixels(point(px(0.), px(-200.)));
    cx.simulate_event(ScrollEvent { position: point(px(100.), px(HEADER + 100.)), delta, modifiers: Default::default(), touch_phase: TouchPhase::Moved, source: Default::default() });
    settle(cx);
    let top = rows.logical_scroll_top();
    assert_eq!((top.item_ix, top.offset_in_item), (2, px(40.)), "the wheel scrolls 200px");
}
