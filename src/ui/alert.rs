// shadcn/ui's Alert: an inline notice inside the view it is about.
#![allow(unused_imports)]
use gpui_kit::*;

use super::*;
use crate::{icons::Glyph, illustration::Illustration, palette};

// An inline notice inside the view it's about: a tinted surface, a leading icon, a title and a line.
// An action button, if any, goes last and centres itself.
// Keywords: notice bar, banner, compact callout, action bar, follow bar, inline action, strip, shadcn
pub struct Alert;

impl Alert {
    // An inline notice inside the view it's about: a tinted surface, a leading icon, a title and a line.
    // An action button, if any, goes last and centres itself.
    pub fn new(kind: Notice, glyph: Glyph, title: impl Into<SharedString>, body: impl Into<SharedString>, cx: &App) -> Div {
        super::callout(kind, glyph, title, body, cx)
    }

    // A callout whose actions sit under its text (`actions` is laid out by the caller), for notices that
    // offer more than one way on.
    pub fn with_actions(kind: Notice, glyph: Glyph, title: impl Into<SharedString>, body: impl Into<SharedString>, actions: impl IntoElement, cx: &App) -> Div {
        super::callout_with(kind, glyph, title, body, actions, cx)
    }

    // An inline strip: a row on its own fill with its mark, message and action, the action at the row's end
    // (`flex_1` on the message). Callers add the children and may set their own padding or `.squircle(..)`.
    // Keywords: notice bar, banner, compact callout, action bar, follow bar, inline action, strip
    pub fn strip(fill: StripFill, cx: &App) -> Div {
        super::strip(fill, cx)
    }
}

