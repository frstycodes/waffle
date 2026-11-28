// shadcn/ui's Badge: a small label for what something is, a count that asks for you, or an unread dot.
use gpui_kit::{AnyElement, App, Div, FontWeight, Hsla, IntoElement, ParentElement, RenderOnce, SharedString, StyleRefinement, Styled, Window, component::Icon, div, prelude::FluentBuilder, px};

use super::{HAIRLINE, RADIUS_XS, Squircle};
use crate::{icons::Glyph, palette};

// A badge's colours: the accent solid, the quiet fill, what asks for you, a hairline, or the accent's tint.
// Keywords: badge variant, badge colour, tag style, shadcn
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BadgeVariant {
    // The accent, solid: NEW, a quality mark.
    #[default]
    Default,
    // The quiet fill: tags and counts that sit in running text.
    Secondary,
    // What asks for you: LIVE, a count, an unread dot.
    Destructive,
    // A hairline and no fill.
    Outline,
    // The accent's tint, uppercase 11px: what something is (App, Host, New). Waffle's own, beside shadcn's four.
    Accent,
}

// A small label: 20px, 11px semibold, squircled. `.variant` picks its colours, `.colors(fill, ink)` takes data's (a
// forum tag, a quality mark over video), `.icon` leads the text, and callers add children, size and placement.
// `Badge::count(n)` is a capsule that grows with the number and caps at 99+, `Badge::dot()` a bare unread mark and
// `Badge::live()` the LIVE label.
// GAP: the design tracks these 0.04–0.06em; gpui text has no letter spacing.
// Keywords: badge, tag, label, pill, count badge, unread dot, live, new, notification count, status label, shadcn
#[derive(IntoElement)]
pub struct Badge {
    base: Div,
    variant: BadgeVariant,
    colors: Option<(Hsla, Hsla)>,
    glyph: Option<Glyph>,
    text: Option<SharedString>,
    shape: Shape,
    children: Vec<AnyElement>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Label,
    Count,
    Dot,
}

impl Badge {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self { text: Some(text.into()), ..Self::empty() }
    }

    // A badge for its children (`.child(..)`) and `.colors`.
    pub fn empty() -> Self {
        Self { base: div(), variant: BadgeVariant::Default, colors: None, glyph: None, text: None, shape: Shape::Label, children: Vec::new() }
    }

    pub fn count(count: u32) -> Self {
        let text = if count > 99 { "99+".to_owned() } else { count.to_string() };
        Self { text: Some(text.into()), shape: Shape::Count, variant: BadgeVariant::Destructive, ..Self::empty() }
    }

    pub fn dot() -> Self {
        Self { shape: Shape::Dot, variant: BadgeVariant::Destructive, ..Self::empty() }
    }

    pub fn live() -> Self {
        Self::new("LIVE").variant(BadgeVariant::Destructive)
    }

    pub fn variant(mut self, variant: BadgeVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn colors(mut self, fill: Hsla, ink: Hsla) -> Self {
        self.colors = Some((fill, ink));
        self
    }

    pub fn icon(mut self, glyph: Glyph) -> Self {
        self.glyph = Some(glyph);
        self
    }
}

impl RenderOnce for Badge {
    fn render(mut self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let (fill, ink) = self.colors.unwrap_or_else(|| match self.variant {
            BadgeVariant::Default => (palette::accent(), palette::on_accent()),
            BadgeVariant::Secondary => (palette::selected(cx), palette::text(cx)),
            BadgeVariant::Destructive => (palette::alert(), gpui_kit::white()),
            BadgeVariant::Outline => (gpui_kit::transparent_black(), palette::text(cx)),
            BadgeVariant::Accent => (palette::accent_soft(cx), palette::accent_ink(cx)),
        });
        let caller = self.base.style().clone();
        let base = self.base.flex_none().bg(fill).text_color(ink).flex().items_center();
        match self.shape {
            Shape::Dot => base.when(caller.size.width.is_none(), |b| b.size(px(8.))).rounded_full(),
            Shape::Count => base.min_w(px(18.)).h(px(18.)).px(px(5.)).rounded_full().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).line_height(px(18.)).justify_center().children(self.text).children(self.children),
            Shape::Label => {
                let accent = self.variant == BadgeVariant::Accent;
                let (height, pad) = if accent { (18., 6.) } else { (20., 7.) };
                base.h(px(height))
                    .px(px(pad))
                    .squircle(RADIUS_XS)
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .when(accent, |b| b.gap(px(3.)))
                    .when(self.variant == BadgeVariant::Outline, |b| b.border(HAIRLINE).border_color(palette::line_strong(cx)))
                    .when_some(self.glyph, |b, glyph| b.child(Icon::new(glyph).size(px(11.))))
                    .children(self.text.map(|t| if accent { SharedString::from(t.to_uppercase()) } else { t }))
                    .children(self.children)
            }
        }
    }
}

impl Default for Badge {
    fn default() -> Self {
        Self::empty()
    }
}

impl Styled for Badge {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl ParentElement for Badge {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}
