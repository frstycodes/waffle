// shadcn/ui's Card: a grouped card, or a clickable one.
#![allow(unused_imports)]
use gpui_kit::*;

use super::*;
use crate::{icons::Glyph, illustration::Illustration, palette};

// Rows on the raised surface, one step from the page with no border, inset so their fills clear its edge.
// Keywords: clickable card, choice card, option card, tile button, template, panel, preview card, shadcn
pub struct Card;

impl Card {
    // Rows on the raised surface, one step from the page with no border, inset so their fills clear its edge.
    pub fn new(cx: &App) -> Div {
        super::settings_card(cx)
    }

    // A clickable card (a template, a post, a launcher entry, a theme): a quiet fill on the page that deepens on
    // hover, rounded large; `.toggled` marks the chosen one and `.bordered` adds the hairline edge.
    // Keywords: clickable card, choice card, option card, tile button, template, panel, preview card
    pub fn interactive(id: impl Into<ElementId>, cx: &App) -> Control {
        super::card(id, cx)
    }
}

