// Three joined segments, deny / inherit / allow (design/css/server-settings.css `.ss-tri`): 36x30 each, the
// chosen one tinted by meaning and keeping its icon, so the state never rests on colour alone.
use gpui_kit::{App, Div, ElementId, Hsla, InteractiveElement, ParentElement, StatefulInteractiveElement, Styled, Window, component::Icon, div, prelude::FluentBuilder, px};

use super::{RADIUS_SM, Squircle, control::part, pressable, Tooltip};
use crate::{icons::Glyph, palette};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tri {
    Deny,
    Inherit,
    Allow,
}

pub fn tri_state(id: impl Into<ElementId>, value: Tri, on_pick: impl Fn(Tri, &mut Window, &mut App) + Clone + 'static, cx: &App) -> Div {
    let id = id.into();
    let line = palette::line_strong(cx);
    let segment = |tri: Tri, cx: &App| {
        let (glyph, label, tone) = match tri {
            Tri::Deny => (Glyph::XCircle, "Deny", (palette::danger_tint(), palette::danger_ink(cx))),
            Tri::Inherit => (Glyph::Minus, "Inherit", (palette::selected(cx), palette::strong(cx))),
            Tri::Allow => (Glyph::CheckCircle, "Allow", (palette::success_tint(), palette::positive_ink(cx))),
        };
        let on = tri == value;
        let (hover, strong, faint) = (palette::hover(cx), palette::strong(cx), palette::faint(cx));
        let (fill, ink): (Hsla, Hsla) = if on { tone } else { (gpui_kit::transparent_black(), faint) };
        let on_pick = on_pick.clone();
        pressable(div().id(part(&id, label)), 0.94)
            .w(px(36.))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(fill)
            .text_color(ink)
            .when(!on, |d| d.hover(move |s| s.bg(hover).text_color(strong)))
            .cursor_pointer()
            .tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
            .child(Icon::new(glyph).size(px(16.)))
            .on_click(move |_, window, cx| on_pick(tri, window, cx))
    };
    div()
        .h(px(30.))
        .flex_none()
        .flex()
        .squircle(RADIUS_SM)
        .border_1()
        .border_color(line)
        .overflow_hidden()
        .child(segment(Tri::Deny, cx).rounded_l(px(RADIUS_SM - 1.)))
        .child(div().w(px(1.)).h_full().bg(line))
        .child(segment(Tri::Inherit, cx))
        .child(div().w(px(1.)).h_full().bg(line))
        .child(segment(Tri::Allow, cx).rounded_r(px(RADIUS_SM - 1.)))
}
