// shadcn/ui's HoverCard: a list that opens on hover and stays while the pointer is on it.
#![allow(unused_imports)]
use gpui_kit::*;

use super::*;
use crate::{icons::Glyph, illustration::Illustration, palette};

// Shows the full list when `el` is hovered; the card itself stays up while the pointer is on it.
// Keywords: hovercard, shadcn
pub struct HoverCard;

impl HoverCard {
    // Shows the full list when `el` is hovered; the card itself stays up while the pointer is on it.
    pub fn new<E: StatefulInteractiveElement>(el: E, title: impl Into<SharedString>, entries: Vec<Entry>) -> E {
        super::overflow_list(el, title, entries)
    }
}

