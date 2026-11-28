// shadcn/ui's Select: a field that opens a menu of choices.
#![allow(unused_imports)]
use gpui_kit::*;

use super::*;
use crate::{icons::Glyph, illustration::Illustration, palette};

// A dropdown's trigger: the chosen value, a caret that turns over while its list is open, the accent edge.
// Keywords: select, shadcn
pub struct Select;

impl Select {
    // A dropdown's trigger: the chosen value, a caret that turns over while its list is open, the accent edge.
    pub fn new(id: impl Into<ElementId>, glyph: Option<Glyph>, value: impl Into<SharedString>, open: bool, cx: &App) -> Control {
        super::select(id, glyph, value, open, cx)
    }

    // A select's open list, rising from under its trigger (`top` is the trigger's height); the chosen option
    // carries a check (`dropdown`).
    pub fn options(
    id: impl Into<SharedString>,
    options: &[(String, String)],
    chosen: &str,
    top: f32,
    on_choose: impl Fn(String, &mut Window, &mut App) + Clone + 'static,
    on_dismiss: impl Fn(&mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
        super::select_options(id, options, chosen, top, on_choose, on_dismiss, cx)
    }

    // Options any number of which can be on, each with its checkbox; choosing one toggles it and leaves the
    // list open.
    pub fn check_options(
    id: impl Into<SharedString>,
    options: &[(String, String, bool)],
    top: f32,
    on_toggle: impl Fn(String, &mut Window, &mut App) + Clone + 'static,
    on_dismiss: impl Fn(&mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
        super::check_options(id, options, top, on_toggle, on_dismiss, cx)
    }
}

