// shadcn/ui's Kbd and KbdGroup: a key you could press, and a shortcut as keys side by side.
use gpui_kit::{
    AnyElement, App, Div, FontWeight, Hsla, IntoElement, ParentElement, RenderOnce, SharedString, StyleRefinement, Styled, Window,
    component::Icon, div, prelude::FluentBuilder, px,
};

use super::{Squircle, RADIUS_XS, shadow};
use crate::{icons::Glyph, palette};

// A keycap: the raised fill, a hairline ring and a deeper bottom edge, so it reads as a key you could press.
// `.translucent(ink)` is a cap on a coloured fill (a danger button): a wash of `ink` with an inset edge, lettered
// in `ink`. `Kbd::glyph` letters it with an icon (the arrow keys).
// Keywords: kbd, keycap, key, shortcut key, keyboard, hotkey, shadcn
#[derive(IntoElement)]
pub struct Kbd {
    base: Div,
    key: Option<SharedString>,
    glyph: Option<Glyph>,
    translucent: Option<Hsla>,
    children: Vec<AnyElement>,
}

impl Kbd {
    pub fn new(key: impl Into<SharedString>) -> Self {
        Self { base: div(), key: Some(key.into()), glyph: None, translucent: None, children: Vec::new() }
    }

    pub fn glyph(glyph: Glyph) -> Self {
        Self { base: div(), key: None, glyph: Some(glyph), translucent: None, children: Vec::new() }
    }

    pub fn translucent(mut self, ink: Hsla) -> Self {
        self.translucent = Some(ink);
        self
    }
}

impl RenderOnce for Kbd {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let cap = self
            .base
            .min_w(px(22.))
            .h(px(22.))
            .px(px(6.))
            .flex_none()
            .squircle(RADIUS_XS)
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .whitespace_nowrap()
            .flex()
            .items_center()
            .justify_center();
        let cap = match self.translucent {
            Some(ink) => cap.bg(ink.opacity(0.16)).shadow(vec![shadow(ink.opacity(0.24), 0., 0., 0., 1., true)]).text_color(ink),
            None => {
                let line = palette::line_strong(cx);
                cap.bg(palette::surface(palette::Level::Raised, cx))
                    .shadow(vec![shadow(line, 0., -1., 0., 0., true), shadow(line, 0., 0., 0., 1., false)])
                    .text_color(palette::strong(cx))
            }
        };
        cap.children(self.key).when_some(self.glyph, |c, glyph| c.child(Icon::new(glyph).size(px(12.)))).children(self.children)
    }
}

impl Styled for Kbd {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl ParentElement for Kbd {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

// A shortcut: its keys side by side.
// Keywords: kbd group, shortcut, key combo, hotkey, chord, shadcn
#[derive(IntoElement)]
pub struct KbdGroup {
    keys: Vec<SharedString>,
}

impl KbdGroup {
    pub fn new<I: Into<SharedString>>(keys: impl IntoIterator<Item = I>) -> Self {
        Self { keys: keys.into_iter().map(Into::into).collect() }
    }
}

impl RenderOnce for KbdGroup {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div().flex().items_center().gap(px(4.)).children(self.keys.into_iter().map(Kbd::new))
    }
}
