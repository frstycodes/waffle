// shadcn/ui's Avatar, AvatarGroup and AvatarFallback.
#![allow(unused_imports)]
use gpui_kit::*;

use super::*;
use crate::{icons::Glyph, illustration::Illustration, palette};

// A photo avatar.
// Keywords: avatar, shadcn
pub struct Avatar;

impl Avatar {
    // A photo avatar.
    pub fn new(url: impl Into<gpui_kit::SharedString>, size: f32) -> Img {
        super::avatar(url, size)
    }
}

// Overlapping avatars, each ringed in `ring` (the surface colour) so they read apart.
// Keywords: avatargroup, shadcn
pub struct AvatarGroup;

impl AvatarGroup {
    // Overlapping avatars, each ringed in `ring` (the surface colour) so they read apart.
    pub fn new(urls: impl IntoIterator<Item = String>, size: f32, ring: Hsla) -> Div {
        super::avatar_stack(urls, size, ring)
    }

    // Avatars of one `size` (photos, monograms, a "+3"), overlapped 6px and ringed in the surface's colour.
    // Placed absolutely in a box sized to the overlap, not as a flex row with -6px margins: taffy 0.13's flex
    // intrinsic sizing reads a negative margin as the item asking to shrink below its basis, so such a row
    // measures too narrow (four 24px avatars at -6px measure 0 wide, at -1px one avatar wide) and whatever
    // follows slides underneath.
    pub fn stacked(avatars: impl IntoIterator<Item = AnyElement>, size: f32, ring: Hsla) -> Div {
        super::stacked(avatars, size, ring)
    }

    // At most `cap` faces; past that the last slot becomes a "+n" chip in `more_fill`, so the stack stays a
    // glance, not a roster.
    pub fn capped(people: &[Entry], cap: usize, size: f32, ring: Hsla, more_fill: Hsla, more_ink: Hsla) -> Div {
        super::capped_stack(people, cap, size, ring, more_fill, more_ink)
    }

    // A capped stack whose "+n" isn't a dead end: hovering it lists everyone, under `title`.
    #[allow(clippy::too_many_arguments)]
    pub fn listed(id: impl Into<ElementId>, title: impl Into<SharedString>, people: Vec<Entry>, cap: usize, size: f32, ring: Hsla, more_fill: Hsla, more_ink: Hsla) -> Stateful<Div> {
        super::listed_stack(id, title, people, cap, size, ring, more_fill, more_ink)
    }
}

// An avatar without a photo: the name's initial on a gradient picked from the name. Below 24px it
// drops the initial.
// Keywords: avatarfallback, shadcn
pub struct AvatarFallback;

impl AvatarFallback {
    // An avatar without a photo: the name's initial on a gradient picked from the name. Below 24px it
    // drops the initial.
    pub fn new(name: &str, size: f32) -> Div {
        super::monogram(name, size)
    }
}

