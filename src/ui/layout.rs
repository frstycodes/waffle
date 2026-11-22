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

use std::ops::Range;

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

/// The rows of a scrolled column worth building: those a view from `view_top`, `view_height` tall, shows,
/// widened by `margin` either side. `heights` are every row's, laid `gap` apart from `top` down the scrolled
/// content. Returns that range and the heights of the spacers standing in for the rows before and after it,
/// so the content keeps its full height and a long list costs only what shows.
pub fn visible_rows(heights: &[f32], gap: f32, top: f32, view_top: f32, view_height: f32, margin: f32) -> (Range<usize>, f32, f32) {
    let (low, high) = (view_top - margin, view_top + view_height + margin);
    let (mut y, mut range) = (top, None::<Range<usize>>);
    for (i, height) in heights.iter().enumerate() {
        if y + height >= low && y <= high {
            range = Some(range.map_or(i..i + 1, |r| r.start..i + 1));
        }
        y += height + gap;
    }
    // Nothing in view: the view is above the rows, or past them.
    let range = range.unwrap_or(if top > high { 0..0 } else { heights.len()..heights.len() });
    let span = |rows: &[f32]| if rows.is_empty() { 0. } else { rows.iter().sum::<f32>() + gap * (rows.len() - 1) as f32 };
    (range.clone(), span(&heights[..range.start]), span(&heights[range.end..]))
}

#[cfg(test)]
mod tests {
    use super::visible_rows;

    #[test]
    fn builds_what_shows_and_keeps_the_rest_s_height() {
        // 100 rows 40 tall, 4 apart, from 100 down; a 400 tall view with 50 either side.
        let rows = [40.; 100];
        let (range, before, after) = visible_rows(&rows, 4., 100., 1000., 400., 50.);
        assert_eq!(range, 19..31);
        // Every row and gap is accounted for: spacers, built rows and the gaps between all of them.
        let built = range.len() as f32 * 40. + (range.len() - 1) as f32 * 4.;
        assert_eq!(before + 4. + built + 4. + after, 100. * 40. + 99. * 4.);
        assert_eq!(visible_rows(&rows, 4., 100., 0., 400., 50.).0, 0..8);
        assert_eq!(visible_rows(&rows, 4., 100., 99_999., 400., 50.), (100..100, 100. * 40. + 99. * 4., 0.));
        assert_eq!(visible_rows(&[], 4., 0., 0., 400., 50.), (0..0, 0., 0.));
        // Mixed heights (a label between rows).
        assert_eq!(visible_rows(&[28., 44., 44., 40., 44.], 2., 0., 80., 20., 0.).0, 2..3);
    }
}
