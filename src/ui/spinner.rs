// shadcn/ui's Spinner: the mini wave, for speaking, typing, loading and live.
#![allow(unused_imports)]
use gpui_kit::*;

use super::*;
use crate::{icons::Glyph, illustration::Illustration, palette};

// The mini wave: three bars, the mark for speaking, typing, loading and live.
// Keywords: spinner, shadcn
pub struct Spinner;

impl Spinner {
    // The mini wave: three bars, the mark for speaking, typing, loading and live.
    pub fn new(id: impl Into<ElementId>, color: Hsla, height: f32) -> AnyElement {
        super::bars(id, color, height)
    }

    // The mini wave at rest (`.bars.still`): connected, in a call.
    pub fn still(color: Hsla, height: f32) -> Div {
        super::bars_still(color, height)
    }

    // The resting wave with all but its first bar dimmed (`.bars.weak`): connected, on a poor link.
    pub fn weak(color: Hsla, height: f32) -> Div {
        super::bars_weak(color, height)
    }
}

