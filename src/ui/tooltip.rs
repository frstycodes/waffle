// shadcn/ui's Tooltip: a one-line label that follows the pointer to a control. `.tooltip(ui::Tooltip::text("Pin"))`.
use gpui_kit::{AnyView, App, AppContext, FontWeight, IntoElement, ParentElement, Render, SharedString, Styled, Window, div, point, px, relative};

use super::{RADIUS_SM, SQUIRCLE};
use crate::palette;

// The design's tooltip: inverted ink, 12px semibold, entering with the window's plain fade (the same as every floating
// surface). gpui places it just below and right of the pointer.
// Keywords: tooltip, hint, label on hover, title, shadcn
pub struct Tooltip {
    text: SharedString,
}

impl Tooltip {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self { text: text.into() }
    }

    // The tooltip as a view, for `.tooltip(move |window, cx| ui::Tooltip::new(text.clone()).build(window, cx))`.
    pub fn build(self, _: &mut Window, cx: &mut App) -> AnyView {
        cx.new(|_| self).into()
    }

    // `.tooltip(ui::Tooltip::text("Pin"))`: the builder gpui's `.tooltip` takes, for a label that doesn't change.
    pub fn text(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
        let text: SharedString = text.into();
        move |window, cx| Tooltip::new(text.clone()).build(window, cx)
    }
}

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, cx: &mut gpui_kit::Context<Self>) -> impl IntoElement {
        crate::perf::probe("Tooltip");
        div()
            .max_w(px(240.))
            .px(px(10.))
            .py(px(6.))
            .rounded(px(RADIUS_SM))
            .corner_shape(SQUIRCLE)
            .transform_origin(point(relative(0.), relative(0.)))
            .bg(palette::strong(cx))
            .text_color(palette::surface(palette::Level::Page, cx))
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .line_height(relative(1.35))
            .child(self.text.clone())
    }
}
