// shadcn/ui's Popover: a floating surface for anything, ringed, shadowed, never behind the pointer.
#![allow(unused_imports)]
use gpui_kit::*;

use super::*;
use crate::{icons::Glyph, illustration::Illustration, palette};

// A bar that floats over a page's foot (unsaved changes, bulk actions): the overlay card at the settings
// column's width. It occludes, so the rows under it never take its hover or clicks.
// Keywords: popover, shadcn
pub struct Popover;

impl Popover {
    // A bar that floats over a page's foot (unsaved changes, bulk actions): the overlay card at the settings
    // column's width. It occludes, so the rows under it never take its hover or clicks.
    pub fn bar(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
        super::floating_bar(id, cx)
    }

    /// A surface floating over everything after it, placed by its parent's position and kept inside the
    /// window, under its title line. It occludes, so nothing under it lights up or takes its clicks.
    pub fn floating(id: impl Into<ElementId>, content: impl IntoElement) -> Deferred {
        super::floating(id, content)
    }

    /// `floating` with its `corner` pinned to the window point `at` (a click, a button's edge).
    pub fn floating_at(id: impl Into<ElementId>, at: Point<Pixels>, corner: Anchor, content: impl IntoElement) -> Deferred {
        super::floating_at(id, at, corner, content)
    }
}

// A floating surface (menu, popover, picker): rounded, ringed and shadowed, with its content inset so
// nothing reaches the rounded edge. A menu card is `surface(cx).w(px(248.))`.
// Keywords: popover, card, menu, panel, popup, picker, shadcn
pub struct PopoverContent;

impl PopoverContent {
    // A floating surface (menu, popover, picker): rounded, ringed and shadowed, with its content inset so
    // nothing reaches the rounded edge. A menu card is `surface(cx).w(px(248.))`.
    // Keywords: popover, card, menu, panel, popup, picker
    pub fn new(cx: &App) -> Div {
        super::surface(cx)
    }

    // A floating surface in a scope's own colours (the call stage's dark overlay and ring, a menu in its ink): the
    // kit's surface with its `fill` and the hairline `ring` the scope draws. Callers add placement and children.
    // Keywords: stage card, menu card, scoped surface, popover in stage colours, dark overlay card
    pub fn scoped(fill: Hsla, ring: Hsla, cx: &App) -> Div {
        super::surface_in(fill, ring, cx)
    }
}

