// shadcn/ui's RadioGroupItem: pick one of a set. Off is a thin ring; on, the ring thickens to an accent band around
// the surface's colour. With an id the band thickens on the spring (200ms) when chosen.
use gpui_kit::{App, Div, ElementId, Hsla, IntoElement, RenderOnce, StyleRefinement, Styled, Window, base::{Transition, transition}, div, px};

use super::{DUR_POP, control, mix};
use crate::{motion, palette};

// One radio of a set: a thin ring that thickens to an accent band when chosen; `RadioGroupItem::mark` is the bare mark.
// Keywords: radio, radio group item, pick one, option, single choice, shadcn
#[derive(IntoElement)]
pub struct RadioGroupItem {
    base: Div,
    id: Option<ElementId>,
    checked: bool,
    size: f32,
    line: Option<Hsla>,
}

impl RadioGroupItem {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self { base: div(), id: Some(id.into()), checked: false, size: 20., line: None }
    }

    // The bare mark a row draws for a choice it owns: no animation, no id.
    pub fn mark(checked: bool) -> Self {
        Self { base: div(), id: None, checked, size: 20., line: None }
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    // A radio at `size` (menus use 18) with its resting ring in `line`, for scopes with their own line colour.
    pub fn scoped(mut self, size: f32, line: Hsla) -> Self {
        self.size = size;
        self.line = Some(line);
        self
    }
}

impl RenderOnce for RadioGroupItem {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let line = self.line.unwrap_or_else(|| palette::line_strong(cx));
        let band = match self.id {
            Some(id) => transition(control::part(&id, "band"), if self.checked { 1. } else { 0. }, Transition::new(DUR_POP).easing(motion::ease_spring()), window, cx),
            None => if self.checked { 1. } else { 0. },
        };
        radio_ring(self.base, band, self.size, line)
    }
}

impl Styled for RadioGroupItem {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

// The chosen band leaves an 8px hole at any size: 6px at 20, 5px at 18, as the design's two radios.
fn radio_ring(base: Div, band: f32, size: f32, line: Hsla) -> Div {
    let on_width = (size - 8.) / 2.;
    let color = mix(line, palette::accent(), band.clamp(0., 1.));
    base.size(px(size)).flex_none().rounded_full().border(px((1.5 + (on_width - 1.5) * band).max(1.))).border_color(color)
}

