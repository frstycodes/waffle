// A ruled data table (design/explore-navigation.html #member-signals): one bordered box, a shaded header
// band, full-width dividers, 48px rows. Every row lays its cells out on the same `Col`s, so columns line
// up without a grid. Narrower than its columns' minimums, the content scrolls sideways inside the box while
// the border and corners stay put.
use gpui_kit::{AnyElement, App, Div, ElementId, FontWeight, InteractiveElement, ParentElement, SharedString, Stateful, StatefulInteractiveElement, Styled, div, px};

use super::{RADIUS_MD, Squircle, edge, top};
use crate::palette;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Col {
    Fixed(f32),
    // A share of the spare width by weight, never narrower than its minimum.
    Grow { weight: f32, min: f32 },
}

// The narrowest the table's content can be with every column at its minimum.
pub fn table_min_width(cols: &[Col]) -> f32 {
    let widths: f32 = cols.iter().map(|c| match c {
        Col::Fixed(w) => *w,
        Col::Grow { min, .. } => *min,
    }).sum();
    widths + GAP * cols.len().saturating_sub(1) as f32 + PAD * 2. + 2.
}

const GAP: f32 = 16.;
const PAD: f32 = 16.;
const HEAD: f32 = 40.;

// The box, its header band and the sideways scroller. The band is painted on the box, not the scrolling
// header row: gpui clips to rectangles, so a moving band would poke square corners past the rounded border.
pub fn table(id: impl Into<SharedString>, min_width: f32, head: Div, rows: impl IntoIterator<Item = AnyElement>, cx: &App) -> Stateful<Div> {
    let id = id.into();
    let band = edge(div().absolute().top_0().left_0().right_0().h(px(HEAD)), top(RADIUS_MD - 1.)).bg(palette::surface(palette::Level::Group, cx));
    let content = div().min_w(px(min_width)).flex().flex_col().child(head).children(rows);
    div()
        .id(ElementId::Name(id.clone()))
        .relative()
        .min_w_0()
        .squircle(RADIUS_MD)
        .border_1()
        .border_color(palette::line_strong(cx))
        .overflow_hidden()
        .child(band)
        .child(div().id(ElementId::Name(format!("{id}-scroll").into())).overflow_x_scroll().child(content))
}

// The header row on the band: 12px muted labels, a fixed height so the band under it lines up.
pub fn table_head(cols: &[Col], cells: impl IntoIterator<Item = AnyElement>, cx: &App) -> Div {
    cells_row(cols, cells).h(px(HEAD)).py_0().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(palette::muted(cx))
}

// A body row under a full-width divider.
pub fn table_row(id: impl Into<ElementId>, cols: &[Col], cells: impl IntoIterator<Item = AnyElement>, cx: &App) -> Stateful<Div> {
    cells_row(cols, cells).id(id.into()).min_h(px(48.)).border_t_1().border_color(palette::line(cx)).text_color(palette::text(cx))
}

fn cells_row(cols: &[Col], cells: impl IntoIterator<Item = AnyElement>) -> Div {
    div().flex().items_center().gap(px(GAP)).px(px(PAD)).py(px(6.)).children(cols.iter().zip(cells).map(|(col, cell)| cell_box(*col).child(cell)))
}

fn cell_box(col: Col) -> Div {
    let cell = div().min_w_0().flex().items_center();
    match col {
        Col::Fixed(width) => cell.w(px(width)).flex_none(),
        Col::Grow { weight, min } => cell.flex_basis(px(min)).min_w(px(min)).flex_shrink_0().flex_grow(weight),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_width_sums_minimums_gaps_padding_and_border() {
        let cols = [Col::Fixed(20.), Col::Grow { weight: 2., min: 220. }, Col::Grow { weight: 1., min: 120. }];
        assert_eq!(table_min_width(&cols), 20. + 220. + 120. + 2. * GAP + 2. * PAD + 2.);
        assert_eq!(table_min_width(&[]), 2. * PAD + 2.);
    }
}
