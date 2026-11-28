// shadcn/ui's Textarea: several lines of text in the same frame.
#![allow(unused_imports)]
use gpui_kit::*;

use super::*;
use super::field::TextField;
use crate::{icons::Glyph, illustration::Illustration, palette};

// The same frame for a multi-line input: at least 88px, 10px top and bottom.
// Keywords: textarea, shadcn
pub struct Textarea;

impl Textarea {
    // The same frame for a multi-line input: at least 88px, 10px top and bottom.
    pub fn new(id: impl Into<ElementId>, input: impl IntoElement, focus: &FocusHandle) -> TextField {
        super::text_area(id, input, focus)
    }
}

