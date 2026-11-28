// shadcn/ui's Checkbox: pick many, a parent showing mixed. Static (no id) it is the plain mark; with an id it fades its
// fill and pops its tick in on the spring (200ms), as the design's does.
use gpui_kit::{App, Div, ElementId, Hsla, IntoElement, ParentElement, RenderOnce, StyleRefinement, Styled, Window, base::{Transition, transition}, component::Icon, div, prelude::FluentBuilder, px};

use super::{DUR_POP, Squircle, control, hover_fade, mix};
use crate::{icons::Glyph, motion, palette};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Check {
    Off,
    On,
    // Some of a group chosen: a minus bar in place of the tick.
    Mixed,
}

// A checkbox: off, on or mixed. With an id it eases its fill and tick; `Checkbox::mark` is the bare mark a row draws.
// Keywords: checkbox, check, tick, multi select, mixed, indeterminate, pick many, shadcn
#[derive(IntoElement)]
pub struct Checkbox {
    base: Div,
    id: Option<ElementId>,
    checked: Check,
}

impl Checkbox {
    // A live checkbox: its fill and tick ease between states.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self { base: div(), id: Some(id.into()), checked: Check::Off }
    }

    // The bare mark a row or menu draws for a state it owns: no animation, no id.
    pub fn mark(checked: Check) -> Self {
        Self { base: div(), id: None, checked }
    }

    pub fn checked(mut self, checked: Check) -> Self {
        self.checked = checked;
        self
    }
}

impl RenderOnce for Checkbox {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (state, accent) = (self.checked, palette::accent());
        let Some(id) = self.id else {
            let filled = state != Check::Off;
            return check_box(self.base, state, if filled { accent } else { palette::line_strong(cx) }, if filled { accent } else { gpui_kit::transparent_black() })
                .when(state == Check::On, |d| d.child(Icon::new(Glyph::Checkmark).size(px(13.))));
        };
        let filled = hover_fade(control::part(&id, "fill"), state != Check::Off, window, cx);
        let tick = transition(control::part(&id, "tick"), if state == Check::On { 1. } else { 0. }, Transition::new(DUR_POP).easing(motion::ease_spring()), window, cx);
        check_box(self.base, state, mix(palette::line_strong(cx), accent, filled), accent.opacity(filled))
            .when(state != Check::Mixed && tick > 0.01, |d| d.child(Icon::new(Glyph::Checkmark).size(px(13.)).scale(0.5 + 0.5 * tick).text_color(palette::on_accent().opacity(tick.min(1.)))))
    }
}

impl Styled for Checkbox {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

fn check_box(base: Div, state: Check, edge: Hsla, fill: Hsla) -> Div {
    base
        .size(px(20.))
        .flex_none()
        .squircle(6.)
        .border(px(1.5))
        .border_color(edge)
        .bg(fill)
        .text_color(palette::on_accent())
        .flex()
        .items_center()
        .justify_center()
        .when(state == Check::Mixed, |d| d.child(div().w(px(10.)).h(px(2.)).squircle(1.).bg(palette::on_accent())))
}

