// shadcn/ui's Input: a single line of text in a framed field.
#![allow(unused_imports)]
use gpui_kit::*;

use super::*;
use super::field::TextField;
use crate::{icons::Glyph, illustration::Illustration, palette};

// The frame around a text input with its focus fade; focus takes the accent edge and ring. `input` is
// the editor (gpui-component's `Input` with `.appearance(false)`), `focus` its handle.
// Keywords: input, shadcn
pub struct Input;

impl Input {
    // The frame around a text input with its focus fade; focus takes the accent edge and ring. `input` is
    // the editor (gpui-component's `Input` with `.appearance(false)`), `focus` its handle.
    pub fn new(id: impl Into<ElementId>, input: impl IntoElement, focus: &FocusHandle) -> TextField {
        super::text_field(id, input, focus)
    }

    // The static frame of a text input (`.input`): 40px, the page fill, a strong hairline, 16px strong text.
    // For a field that needs the focus fade, use `text_field`.
    pub fn frame(cx: &App) -> Div {
        super::field_frame(cx)
    }
}

