// shadcn/ui's Sonner: what just happened, rising in and leaving by itself.
#![allow(unused_imports)]
use gpui_kit::*;

use super::*;
use crate::{icons::Glyph, illustration::Illustration, palette};

// A toast: one line (and an optional detail) on the floating surface, rising in over 240ms from 8px lower
// and 0.97. GAP: the design also un-blurs it from 4px (`filter: blur`); gpui has no element blur.
// Keywords: text toast, plain toast, snackbar, one line message, shadcn
pub struct Sonner;

impl Sonner {
    // A toast: one line (and an optional detail) on the floating surface, rising in over 240ms from 8px lower
    // and 0.97. GAP: the design also un-blurs it from 4px (`filter: blur`); gpui has no element blur.
    pub fn toast(id: impl Into<ElementId>, glyph: Glyph, ink: Hsla, title: impl Into<SharedString>, detail: Option<SharedString>, cx: &App) -> AnyElement {
        super::toast(id, glyph, ink, title, detail, cx)
    }

    // A toast with its one action (Undo) at the end.
    pub fn toast_with(id: impl Into<ElementId>, glyph: Glyph, ink: Hsla, title: impl Into<SharedString>, detail: Option<SharedString>, action: Option<AnyElement>, cx: &App) -> AnyElement {
        super::toast_with(id, glyph, ink, title, detail, action, cx)
    }

    // A toast of one line of text, with no lead glyph, rising in like `ui::toast`.
    // Keywords: text toast, plain toast, snackbar, one line message
    pub fn text(id: impl Into<ElementId>, text: impl Into<SharedString>, cx: &App) -> AnyElement {
        super::toast_text(id, text, cx)
    }
}

