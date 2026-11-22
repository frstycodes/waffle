// Layout that can't overflow or leak the pointer.
//
// Overflow: a flex child's minimum size is its content's (CSS `min-width: auto`), so a long name in a row
// pushes the row past its parent instead of shrinking. `row` and `col` take `min_w_0`/`min_h_0` so they
// shrink inside whatever holds them, and `line` truncates its text with an ellipsis.
//
// The pointer: gpui hit-tests front to back until something occludes, so a surface drawn over others
// passes hover and clicks to what's behind it unless it blocks them (CLAUDE.md, "nothing floats without
// occluding"). `floating` and `floating_at` occlude for you.
use gpui_kit::{Anchor, AnyElement, App, AvailableSpace, Canvas, Deferred, Div, ElementId, InteractiveElement, IntoElement, ParentElement, Pixels, Point, SharedString, Size, Styled, Window, anchored, canvas, deferred, div};

use super::WINDOW_MARGIN;

/// A horizontal flex row, children centred on the cross axis, able to shrink below its content.
pub fn row() -> Div {
    div().flex().flex_row().items_center().min_w_0()
}

/// A vertical flex column, able to shrink below its content on both axes.
pub fn col() -> Div {
    div().flex().flex_col().min_w_0().min_h_0()
}

/// One line of text that ends in an ellipsis when it doesn't fit. `ui::truncating` also shows the cut
/// text in a tooltip.
pub fn line(text: impl Into<SharedString>) -> Div {
    div().min_w_0().truncate().child(text.into())
}

/// A surface floating over everything after it, placed by its parent's position and kept inside the
/// window. It occludes, so nothing under it lights up or takes its clicks.
pub fn floating(id: impl Into<ElementId>, content: impl IntoElement) -> Deferred {
    deferred(anchored().snap_to_window_with_margin(WINDOW_MARGIN).child(div().id(id).occlude().child(content)))
}

/// `floating` with its `corner` pinned to the window point `at` (a click, a button's edge).
pub fn floating_at(id: impl Into<ElementId>, at: Point<Pixels>, corner: Anchor, content: impl IntoElement) -> Deferred {
    deferred(anchored().position(at).anchor(corner).snap_to_window_with_margin(WINDOW_MARGIN).child(div().id(id).occlude().child(content)))
}

/// Content built for the size its slot has this frame, for layouts that pick their shape from the space
/// (a grid choosing its columns), without the one-frame lag of measuring and re-rendering.
pub fn sized(build: impl FnOnce(Size<Pixels>, &mut Window, &mut App) -> AnyElement + 'static) -> Canvas<AnyElement> {
    canvas(
        move |bounds, window, cx| {
            let content = build(bounds.size, window, cx);
            let mut el = div().w(bounds.size.width).h(bounds.size.height).child(content).into_any_element();
            el.prepaint_as_root(bounds.origin, bounds.size.map(AvailableSpace::Definite), window, cx);
            el
        },
        |_, mut el, window, cx| el.paint(window, cx),
    )
    .size_full()
}
