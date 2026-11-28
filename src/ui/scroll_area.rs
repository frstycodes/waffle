// shadcn/ui's ScrollArea: a list that scrolls, with the kit's scrollbar over it.
#![allow(unused_imports)]
use gpui_kit::*;

use super::*;
use crate::{icons::Glyph, illustration::Illustration, palette};

/// A vertical scroll area moving `list` on `scroll`, with the kit's scrollbar over it. Not `overflow_y_scrollbar`: that
/// tracks a handle of its own, which nothing outside can read.
// Keywords: scrollarea, shadcn
pub struct ScrollArea;

impl ScrollArea {
    /// A vertical scroll area moving `list` on `scroll`, with the kit's scrollbar over it. Not `overflow_y_scrollbar`: that
    /// tracks a handle of its own, which nothing outside can read.
    pub fn new(list: Stateful<Div>, scroll: &gpui_kit::ScrollHandle) -> Div {
        super::scroll_area(list, scroll)
    }

    /// A list that scrolls inside a fixed box, its top and bottom edges fading into `surface` wherever there is
    /// more to scroll to: a short box of rows doesn't look scrollable on its own, and the fade says it is.
    pub fn faded(list: Stateful<Div>, scroll: &gpui_kit::ScrollHandle, fill: Hsla) -> Div {
        super::scroll_fade(list, scroll, fill)
    }
}

