// shadcn/ui's Item: a person or thing in a list, a settings row, and the tile that leads it.
#![allow(unused_imports)]
use gpui_kit::*;

use super::*;
use super::kit::ListRow;
use crate::{icons::Glyph, illustration::Illustration, palette};

// A list row for a person or thing: lead, name, muted line, trailing actions; see `ListRow`.
// Keywords: person row, member row, friend row, user row, list item, avatar name subtitle, result row, message row, timestamp, request row, activity row, shadcn
pub struct Item;

impl Item {
    // A list row for a person or thing: lead, name, muted line, trailing actions; see `ListRow`.
    // Keywords: person row, member row, friend row, user row, list item, avatar name subtitle, result row, message row, timestamp, request row, activity row
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>, cx: &App) -> ListRow {
        super::list_row(id, title, cx)
    }

    // A row in a settings card: title and description first, the control last. `line` draws the inset hairline
    // above it (every row but the first), hidden while the row itself is hovered.
    // GAP: CSS also hides the hairline under a hovered row (the next row's); gpui can't style a sibling.
    pub fn setting(id: impl Into<SharedString>, line: bool, cx: &App) -> Stateful<Div> {
        super::settings_row(id, line, cx)
    }
}

// A squircle holding an icon, for row leads and settings.
// Keywords: itemmedia, shadcn
pub struct ItemMedia;

impl ItemMedia {
    // A squircle holding an icon, for row leads and settings.
    pub fn icon(glyph: Glyph, fill: Hsla, ink: Hsla) -> Div {
        super::icon_tile(glyph, fill, ink)
    }
}

