// shadcn/ui's InputGroup: an input with parts around it (the search box, the number stepper).
#![allow(unused_imports)]
use gpui_kit::*;

use super::*;
use super::field::TextField;
use crate::{icons::Glyph, illustration::Illustration, palette};

// The one search box: the regular input's size, edge-free on a translucent fill over a light blur (content
// scrolling under a header stays faintly visible), the magnifier first, the accent edge and ring on focus.
// Keywords: inputgroup, shadcn
pub struct InputGroup;

impl InputGroup {
    // The one search box: the regular input's size, edge-free on a translucent fill over a light blur (content
    // scrolling under a header stays faintly visible), the magnifier first, the accent edge and ring on focus.
    pub fn search(id: impl Into<ElementId>, state: &Entity<gpui_kit::component::input::InputState>, cx: &App) -> TextField {
        super::search_field(id, state, cx)
    }

    // The same search box around an `Input` the caller has configured, e.g. with inline tokens.
    pub fn search_around(id: impl Into<ElementId>, input: gpui_kit::component::input::Input, focus: &FocusHandle) -> TextField {
        super::search_frame(id, input, focus)
    }

    // A button that opens a search somewhere else (the switcher), drawn as the search box with its label in the
    // placeholder ink: no caret, a hover fill and a pointer.
    pub fn search_button(id: impl Into<ElementId>, label: impl Into<SharedString>, cx: &App) -> Stateful<Div> {
        super::search_button(id, label, cx)
    }

    // A number nudged in steps: minus, the value, plus. Callers wire the two buttons' clicks.
    pub fn stepper(id: impl Into<ElementId>, value: impl Into<SharedString>, less: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static, more: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static, cx: &App) -> Div {
        super::stepper(id, value, less, more, cx)
    }
}

