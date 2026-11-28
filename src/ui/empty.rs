// shadcn/ui's Empty: spot art, what is missing and room for one action.
#![allow(unused_imports)]
use gpui_kit::*;

use super::*;
use crate::{icons::Glyph, illustration::Illustration, palette};

// An empty or waiting state: spot art (the mascot, or an illustration where Discord leads with one), what's
// missing, and room for one action.
// Keywords: empty, shadcn
pub struct Empty;

impl Empty {
    // An empty or waiting state: spot art (the mascot, or an illustration where Discord leads with one), what's
    // missing, and room for one action.
    pub fn new(id: &'static str, art: impl Into<Art>, title: impl Into<SharedString>, body: impl Into<SharedString>, cx: &App) -> Div {
        super::empty_state(id, art, title, body, cx)
    }
}

