// A ruled data table (design/explore-navigation.html #member-signals): one bordered box, a shaded header
// band, full-width dividers, ~40px rows. Every row lays its cells out on the same `Col`s, so columns line
// up without a grid.
use gpui_kit::{AnyElement, App, Div, ElementId, FontWeight, InteractiveElement, ParentElement, Stateful, Styled, div, px};

use super::{RADIUS_MD, Squircle, edge, top};
use crate::palette;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Col {
    Fixed(f32),
    // A share of the spare width by weight, never narrower than its minimum.
    Grow { weight: f32, min: f32 },
}

pub fn table(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    div().id(id.into()).min_w_0().flex().flex_col().squircle(RADIUS_MD).border_1().border_color(palette::line_strong(cx))
}

// The header band: 12px muted labels on the group tone, its top corners following the box's.
pub fn table_head(cols: &[Col], cells: impl IntoIterator<Item = AnyElement>, cx: &App) -> Div {
    edge(cells_row(cols, cells), top(RADIUS_MD - 1.)).min_h(px(32.)).bg(palette::surface(palette::Level::Group, cx)).text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(palette::muted(cx))
}

// A body row under a full-width divider.
pub fn table_row(id: impl Into<ElementId>, cols: &[Col], cells: impl IntoIterator<Item = AnyElement>, cx: &App) -> Stateful<Div> {
    cells_row(cols, cells).id(id.into()).min_h(px(40.)).border_t_1().border_color(palette::line(cx)).text_color(palette::text(cx))
}

fn cells_row(cols: &[Col], cells: impl IntoIterator<Item = AnyElement>) -> Div {
    div().flex().items_center().gap(px(12.)).px(px(14.)).py(px(4.)).children(cols.iter().zip(cells).map(|(col, cell)| cell_box(*col).child(cell)))
}

fn cell_box(col: Col) -> Div {
    let cell = div().min_w_0().flex().items_center();
    match col {
        Col::Fixed(width) => cell.w(px(width)).flex_none(),
        Col::Grow { weight, min } => cell.flex_basis(px(min)).min_w(px(min)).flex_shrink_0().flex_grow(weight),
    }
}
