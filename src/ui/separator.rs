// shadcn/ui's Separator: a hairline, horizontal or vertical, optionally naming what follows it. Callers keep
// composing it (`.w_5()`, `.bg(..)`, `.absolute()`); a fill, width or height they set wins over its own.
use gpui_kit::{App, Div, FontWeight, Hsla, IntoElement, ParentElement, RenderOnce, SharedString, StyleRefinement, Styled, Window, div};

use super::{HAIRLINE, TEXT_XS};
use crate::palette;

// Which way a separator runs.
// Keywords: orientation, horizontal vertical, divider direction, shadcn
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Orientation {
    #[default]
    Horizontal,
    Vertical,
}

// A hairline between groups: horizontal runs as long as its parent gives it (`.w_full()`, `.flex_1()`, `.w_5()`);
// vertical is 20px tall with 2px clear either side, for toolbar sections and a header's controls. A label names what
// follows (a day's date); `.color` and `.ink` take a scope's own line and label colours (the call stage, a state).
// `.inset(n)` lays it absolutely along the foot of a relative parent, held `n` in from its edges.
// Keywords: separator, divider, rule, hairline, line, seam, vertical rule, toolbar separator, day divider, labelled, inset rule, shadcn
#[derive(IntoElement)]
pub struct Separator {
    base: Div,
    orientation: Orientation,
    label: Option<SharedString>,
    color: Option<Hsla>,
    ink: Option<Hsla>,
    inset: Option<f32>,
}

impl Separator {
    pub fn new() -> Self {
        Self { base: div(), orientation: Orientation::Horizontal, label: None, color: None, ink: None, inset: None }
    }

    pub fn orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = orientation;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    // The line's colour, where the scope has its own.
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    // The label's colour, where the scope has its own; it also sets the label's tighter, medium-weight type.
    pub fn ink(mut self, ink: Hsla) -> Self {
        self.ink = Some(ink);
        self
    }

    pub fn inset(mut self, inset: f32) -> Self {
        self.inset = Some(inset);
        self
    }
}

impl Default for Separator {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderOnce for Separator {
    fn render(mut self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let line = self.color.unwrap_or_else(|| palette::line(cx));
        let caller = self.base.style().clone();
        let hairline = || div().flex_1().h(HAIRLINE).bg(line);
        if let Some(label) = self.label {
            return match self.ink {
                Some(ink) => self.base.flex().items_center().w_full().gap_2p5().whitespace_nowrap().text_size(TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(ink).child(hairline()).child(label).child(hairline()),
                None => self.base.flex().items_center().w_full().gap_3().child(hairline()).child(div().text_size(TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(palette::faint(cx)).child(label)).child(hairline()),
            };
        }
        if let Some(inset) = self.inset {
            return self.base.absolute().bottom_0().left(gpui_kit::px(inset)).right(gpui_kit::px(inset)).h(HAIRLINE).bg(line);
        }
        match self.orientation {
            Orientation::Horizontal => self.base.h(HAIRLINE).bg(line),
            Orientation::Vertical => {
                let base = self.base.flex_none().w(HAIRLINE).h_5().mx_0p5();
                if caller.background.is_some() { base } else { base.bg(line) }
            }
        }
    }
}

impl Styled for Separator {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}
