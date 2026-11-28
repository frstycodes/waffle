// shadcn/ui's DropdownMenu: a panel of rows under its trigger, and the row.
#![allow(unused_imports)]
use gpui_kit::*;

use super::*;
use crate::{icons::Glyph, illustration::Illustration, palette};

// A floating panel under its trigger (`top` is the trigger's height), dismissed by a press anywhere else:
// a select's list, or a picker with its own search. `panel` brings its own id and `occlude`.
// Keywords: dropdownmenu, shadcn
pub struct DropdownMenu;

impl DropdownMenu {
    // A floating panel under its trigger (`top` is the trigger's height), dismissed by a press anywhere else:
    // a select's list, or a picker with its own search. `panel` brings its own id and `occlude`.
    pub fn panel(id: impl Into<SharedString>, panel: impl IntoElement, top: f32, on_dismiss: impl Fn(&mut Window, &mut App) + 'static) -> AnyElement {
        super::dropdown(id, panel, top, on_dismiss)
    }

    // A menu surface of `rows` rising from under its trigger, which sits `top` px tall above it in the same
    // relative parent; `align` picks the trigger edge it lines up with.
    pub fn menu(id: impl Into<SharedString>, top: f32, align: Align, rows: Vec<AnyElement>, on_dismiss: impl Fn(&mut Window, &mut App) + 'static, cx: &App) -> AnyElement {
        super::menu_dropdown(id, top, align, rows, on_dismiss, cx)
    }

}

// A row inside a floating surface (menu, popover, picker): no fill until hovered, then the hover fill and
// the strong ink fade in. Callers add the lead glyph, the label (`flex_1`) and any trailing mark;
// `.danger(cx)` makes it the destructive row.
// Keywords: list row, item, option, hover row, shadcn
pub struct DropdownMenuItem;

impl DropdownMenuItem {
    // A row inside a floating surface (menu, popover, picker): no fill until hovered, then the hover fill and
    // the strong ink fade in. Callers add the lead glyph, the label (`flex_1`) and any trailing mark;
    // `.danger(cx)` makes it the destructive row.
    // Keywords: list row, item, option, hover row
    pub fn new(id: impl Into<ElementId>, cx: &App) -> Control {
        super::menu_row(id, cx)
    }
}

