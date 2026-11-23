// The theme picker's previews: a tiny Waffle window drawn in each theme's colours, and Sync with OS's split one.
use gpui_kit::{AnyElement, Div, IntoElement, ParentElement, Styled, div, prelude::*, px, relative};

use crate::{
    palette,
    ui::{self, Squircle},
};

pub(super) struct Mini {
    rail: u32,
    side: u32,
    page: u32,
    ink: u32,
    faint: u32,
}

pub(super) const MINI_DARK: Mini = Mini { rail: 0x0A0B0D, side: 0x111213, page: 0x17181A, ink: 0xD6D7DA, faint: 0x4C4D4F };
pub(super) const MINI_LIGHT: Mini = Mini { rail: 0xDEE0E3, side: 0xECEEF0, page: 0xF8F9FB, ink: 0x2D2E30, faint: 0xBCBEC0 };

pub(super) fn mini_window(m: &Mini) -> Div {
    let bar = |color: u32, width: f32| div().h_1().w(relative(width)).rounded_full().bg(palette::data_color(color));
    // The rail runs along the window's rounded left edge, so it takes those corners: gpui clips only to
    // rectangles, and a square rail poked out past them.
    let rail = ui::edge(div(), ui::left(ui::RADIUS_MD))
        .w(relative(0.12))
        .h_full()
        .bg(palette::data_color(m.rail))
        .pt_1p5()
        .flex()
        .flex_col()
        .items_center()
        .gap_1()
        .child(div().w(relative(0.6)).h_3p5().squircle(4.).bg(palette::accent()))
        .child(div().w(relative(0.6)).h_3p5().squircle(4.).bg(palette::data_color(m.faint)));
    let side = ui::col().w(relative(0.28)).h_full().bg(palette::data_color(m.side)).py_2().px_1p5().gap(px(MINI_GAP)).child(bar(m.faint, 1.)).child(bar(m.ink, 0.8)).child(bar(m.faint, 0.6)).child(bar(m.faint, 1.));
    let content = ui::col().min_h_0()
        .flex_1()
        .h_full()
        .py_2p5()
        .px_2()
        .justify_end()
        .gap(px(MINI_GAP))
        .child(bar(m.ink, 0.4))
        .child(bar(m.faint, 0.85))
        .child(bar(m.ink, 0.4))
        .child(bar(m.faint, 0.85))
        .child(div().pt_1().w_full().child(div().h_3().w_full().squircle(4.).bg(palette::data_color(m.faint))));
    ui::row().items_stretch().w_full().h(px(MINI_HEIGHT)).squircle(ui::RADIUS_MD).bg(palette::data_color(m.page)).child(rail).child(side).child(content)
}

const MINI_HEIGHT: f32 = 130.;
// The gap between the preview's lines of "text".
const MINI_GAP: f32 = 5.;

// Sync with OS: the light window on the left half, the dark one on the right.
// GAP: the design splits the two along the diagonal (clip-path). gpui clips only to rectangles, and a
// clip inside a rotated element is its axis-aligned box, so rotation can't cut the diagonal either.
pub(super) fn mini_sync() -> AnyElement {
    let half = |left: bool, m: &Mini| {
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .w(relative(0.5))
            .when(left, |d| d.left_0())
            .when(!left, |d| d.right_0())
            .overflow_hidden()
            .child(div().absolute().top_0().w(relative(2.)).when(left, |d| d.left_0()).when(!left, |d| d.right_0()).child(mini_window(m)))
    };
    div().relative().w_full().h(px(MINI_HEIGHT)).child(half(true, &MINI_LIGHT)).child(half(false, &MINI_DARK)).into_any_element()
}
