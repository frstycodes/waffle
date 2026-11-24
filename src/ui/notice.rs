// Inline notices and their small parts: a surface in a scope's own colours, a strip with its action at the row's end, a text-only toast, a
// numbered disc, and the hairline rules (labelled dividers, a bare rule in any ink).
use std::time::Duration;

use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, Div, ElementId, FontWeight, Hsla, InteractiveElement, IntoElement, ParentElement, SharedString, Styled, div, px,
};

use super::{HAIRLINE, LINE_SM, RADIUS_LG, RADIUS_MD, RADIUS_SM, Squircle, TEXT_SM, TEXT_XS, mix, row, shadow_pop};
use crate::{motion, palette};

// What an inline strip is filled with: the raised card with a hairline edge, the selected wash, or a tint of
// the accent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StripFill {
    Raised,
    Selected,
    Accent,
}

// An inline strip: a row on its own fill with its mark, message and action, the action at the row's end
// (`flex_1` on the message). Callers add the children and may set their own padding or `.squircle(..)`.
// Keywords: notice bar, banner, compact callout, action bar, follow bar, inline action, strip
pub fn strip(fill: StripFill, cx: &App) -> Div {
    let base = row().min_h_11().py_1p5().pl_3().pr_2p5().gap_2().squircle(RADIUS_MD).text_size(TEXT_SM).text_color(palette::strong(cx));
    match fill {
        StripFill::Raised => base.squircle(RADIUS_LG).bg(palette::surface(palette::Level::Raised, cx)).border_1().border_color(palette::line(cx)),
        StripFill::Selected => base.bg(palette::selected(cx)).text_color(palette::text(cx)),
        StripFill::Accent => base.squircle(RADIUS_SM).bg(mix(palette::surface(palette::Level::Page, cx), palette::accent(), 0.16)),
    }
}

// A toast of one line of text, with no lead glyph, rising in like `ui::toast`.
// Keywords: text toast, plain toast, snackbar, one line message
pub fn toast_text(id: impl Into<ElementId>, text: impl Into<SharedString>, cx: &App) -> AnyElement {
    row()
        .max_w_96()
        .min_h_10()
        .py_2p5()
        .pl_3p5()
        .pr_2p5()
        .squircle(RADIUS_LG)
        .bg(palette::surface(palette::Level::Overlay, cx))
        .shadow(shadow_pop(cx))
        .text_size(TEXT_SM)
        .line_height(LINE_SM)
        .font_weight(FontWeight::NORMAL)
        .text_color(palette::strong(cx))
        .child(text.into())
        .with_animation(id, Animation::new(Duration::from_millis(240)).with_easing(|t| motion::ease_out().sample(t)), |toast, t| toast.translate_y(px(8. * (1. - t))).scale(0.97 + 0.03 * t).opacity(t))
        .into_any_element()
}

// A numbered disc, 24px, in the accent tint: a step's or a rule's number.
// Keywords: step number, numbered mark, round badge, ordinal, counter disc
pub fn number_mark(n: usize, cx: &App) -> Div {
    row().size_6().flex_none().rounded_full().bg(palette::accent_soft(cx)).text_color(palette::accent_ink(cx)).text_size(TEXT_XS).font_weight(FontWeight::BOLD).justify_center().child(n.to_string())
}

// A hairline in `color`: give it a length (`flex_1`, `w_5`) or `absolute` placement. A vertical rule is
// `ui::seam`.
// Keywords: rule, hairline, line, separator, connector, stub
pub fn rule(color: Hsla) -> Div {
    div().h(HAIRLINE).bg(color)
}

// A divider whose label and line colour come from data (a day's date, a state's ink): rules either side of
// the label, inside the parent's gutters.
// Keywords: day divider, date separator, labelled rule, section line
pub fn divider_with(label: impl Into<SharedString>, ink: Hsla, line: Hsla) -> Div {
    row().w_full().gap_2p5().whitespace_nowrap().text_size(TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(ink).child(rule(line).flex_1()).child(label.into()).child(rule(line).flex_1())
}

// A floating surface in a scope's own colours (the call stage's dark overlay and ring, a menu in its ink): the
// kit's surface with its `fill` and the hairline `ring` the scope draws. Callers add placement and children.
// Keywords: stage card, menu card, scoped surface, popover in stage colours, dark overlay card
pub fn surface_in(fill: Hsla, ring: Hsla, cx: &App) -> Div {
    super::col().squircle(RADIUS_LG).bg(fill).shadow(super::shadow_pop_on(ring, cx)).p(px(super::SURFACE_INSET)).gap(px(1.))
}

// A card that only holds things (a quoted embed, a preview): the raised fill, a hairline edge, rounded large,
// content in a column. It takes no pointer; a clickable one is `ui::card`.
// Keywords: static card, quote card, embed card, panel, raised box, preview box
pub fn panel(cx: &App) -> Div {
    super::col().squircle(RADIUS_LG).bg(palette::surface(palette::Level::Raised, cx)).border_1().border_color(palette::line(cx))
}

// A thought bubble: a pill of `content` in `fill` with a hairline `edge`, and two dots trailing up-left from it
// towards whoever's thinking it (a custom status beside an avatar). The pill paints over the larger dot, so
// only its top shows, like the bubble's tail.
// Keywords: thought bubble, status bubble, speech bubble, custom status, callout bubble, tail
pub fn thought_bubble(id: impl Into<ElementId>, content: impl IntoElement, fill: Hsla, edge: Hsla) -> gpui_kit::Stateful<Div> {
    let dot = |size: f32| div().absolute().size(px(size)).rounded_full().bg(fill).border_1().border_color(edge);
    div()
        .id(id)
        .relative()
        .min_w_0()
        .child(dot(BUBBLE_DOT_SMALL).left_neg_2().top_neg_3p5())
        .child(dot(BUBBLE_DOT_LARGE).left_1().top_neg_2())
        .child(row().relative().min_h_9().px_3().py_2().rounded(px(BUBBLE_RADIUS)).bg(fill).border_1().border_color(edge).text_size(TEXT_SM).line_height(super::LINE_DETAIL).child(content))
}

// The bubble's pill corner and its two trailing dots.
const BUBBLE_RADIUS: f32 = 18.;
const BUBBLE_DOT_SMALL: f32 = 8.;
const BUBBLE_DOT_LARGE: f32 = 16.;
