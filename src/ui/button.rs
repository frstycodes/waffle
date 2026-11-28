// shadcn/ui's Button: `Button::new(id).variant(ButtonVariant::Outline).size(ButtonSize::Sm)`. Looks live in
// `ButtonVariant::look`, the one place that says what each variant is, as cva's `variants` table does.
use gpui_kit::{
    prelude::FluentBuilder,
    SharedString, AnyElement, App, Div, ElementId, InteractiveElement, Interactivity, IntoElement, ParentElement, RenderOnce, Stateful, StatefulInteractiveElement, StyleRefinement, Styled, Window, div,
};

use super::{control::{self, Look}, theme};
use crate::{icons::Glyph, palette};

// What a button is for, as shadcn's cva variants: default, secondary, destructive, outline, ghost, link.
// Keywords: button variant, button style, primary secondary destructive, cva, shadcn
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ButtonVariant {
    // The accent: the action the surface is for.
    #[default]
    Default,
    // The quiet fill: every other action.
    Secondary,
    Destructive,
    // A hairline edge, no fill until hovered.
    Outline,
    // No fill until hovered: low-stakes actions in dense rows.
    Ghost,
    // Just the accent ink, underlined on hover.
    Link,
}

// A button's size: default 36, sm 30, lg 44, and the icon squares 24, 32, 36 and 40.
// Keywords: button size, icon button size, square button, sm lg icon, shadcn
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum ButtonSize {
    #[default]
    Default,
    Sm,
    Lg,
    // Squares for an icon alone: 24, 32, 36 (shadcn's `icon`) and 40.
    IconXs,
    IconSm,
    Icon,
    IconLg,
    // An exact square the four don't cover (the server rail's 44).
    Square(f32),
}

// Where the button sits. Page is the app's own surface; the rest keep their own ink over what is behind them.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum ButtonScope {
    #[default]
    Page,
    // Translucent black over video and images.
    Media,
    // Solid white over art and video: the one action there (Join Activity, Play).
    Art,
    // A wash of the stage's ink over the call stage, which keeps its own dark palette in both themes.
    Stage,
    // A confirming fill (Discord's Success component button).
    Success,
    // A profile's own colours (a themed card): fill at rest and under the pointer, ink at rest and under it.
    Themed { rest: gpui_kit::Hsla, hover: gpui_kit::Hsla, ink: gpui_kit::Hsla, ink_hover: gpui_kit::Hsla },
}

impl ButtonScope {
    // The scope's colours in place of the variant's; `None` on the page.
    pub(super) fn look(self) -> Option<Look> {
        let white = gpui_kit::white();
        if let Self::Themed { rest, hover, ink, ink_hover } = self {
            return Some(Look { rest, hover, press: hover, ink, ink_hover, edge: None });
        }
        let (rest, hover, press, ink) = match self {
            Self::Page | Self::Themed { .. } => return None,
            Self::Media => (gpui_kit::black().opacity(0.6), gpui_kit::black().opacity(0.75), gpui_kit::black().opacity(0.85), white),
            // Dark ink on white whatever the theme: the art behind it is what sets the tone.
            Self::Art => (white, super::mix(white, palette::stage::bg(), 0.12), super::mix(white, palette::stage::bg(), 0.2), palette::stage::bg()),
            Self::Success => (palette::success(), palette::success_hover(), palette::success_hover(), white),
            Self::Stage => {
                let ink = palette::stage::strong();
                (ink.opacity(0.1), ink.opacity(0.16), ink.opacity(0.22), ink)
            }
        };
        Some(Look { rest, hover, press, ink, ink_hover: ink, edge: None })
    }
}

impl ButtonVariant {
    pub(super) fn look(self, cx: &App) -> Look {
        let clear = gpui_kit::transparent_black();
        let flat = |rest, hover, press, ink| Look { rest, hover, press, ink, ink_hover: ink, edge: None };
        match self {
            Self::Default => flat(theme::primary(), palette::accent_hover(), palette::accent_press(), theme::primary_foreground()),
            Self::Secondary => flat(theme::secondary(cx), palette::press(cx), palette::press(cx), theme::secondary_foreground(cx)),
            Self::Destructive => flat(theme::destructive(), palette::danger_hover(), palette::danger_hover(), theme::destructive_foreground()),
            Self::Outline => Look { edge: Some((theme::input(cx), theme::input(cx))), ..flat(clear, theme::accent_wash(cx), palette::press(cx), theme::foreground(cx)) },
            Self::Ghost => flat(clear, theme::accent_wash(cx), palette::press(cx), theme::foreground(cx)),
            Self::Link => flat(clear, clear, clear, palette::accent_ink(cx)),
        }
    }
}

// A text button for a verb and its object, in shadcn's variants and sizes; callers add the label and `.on_click`.
// Keywords: button, cta, action, primary, secondary, destructive, outline, ghost, link, cva, shadcn
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    base: Stateful<Div>,
    children: Vec<AnyElement>,
    variant: ButtonVariant,
    size: ButtonSize,
    scope: ButtonScope,
    disabled: bool,
    toggled: bool,
    busy: bool,
    danger: bool,
    highlighted: bool,
    glyph: Option<Glyph>,
    tooltip: Option<SharedString>,
    off: bool,
    succeeded: bool,
    round: bool,
    bordered: bool,
    loading: Option<SharedString>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>) -> Self {
        let id = id.into();
        Self { base: div().id(id.clone()), id, children: Vec::new(), variant: ButtonVariant::default(), size: ButtonSize::default(), scope: ButtonScope::default(), disabled: false, toggled: false, busy: false, danger: false, highlighted: false, glyph: None, tooltip: None, off: false, succeeded: false, round: false, bordered: false, loading: None }
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    // The destructive row's look on a quiet button: danger ink, and on hover the danger fill under white ink.
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    // Shows the hover state without the pointer: a row of states side by side, the keyboard's row.
    pub fn highlighted(mut self, on: bool) -> Self {
        self.highlighted = on;
        self
    }

    // The glyph of an icon button, sized for the button.
    pub fn icon(mut self, glyph: Glyph) -> Self {
        self.glyph = Some(glyph);
        self
    }

    // Names an icon button; every one has it.
    pub fn tooltip_text(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    // Something switched off (a muted mic): danger ink.
    pub fn off(mut self, off: bool) -> Self {
        self.off = off;
        self
    }

    // The action just worked (Copied): the positive ink until the caller clears it.
    pub fn succeeded(mut self, on: bool) -> Self {
        self.succeeded = on;
        self
    }

    pub fn round(mut self) -> Self {
        self.round = true;
        self
    }

    // The hairline edge a button takes on a surface the same tone as its fill.
    pub fn bordered(mut self) -> Self {
        self.bordered = true;
        self
    }

    pub fn scope(mut self, scope: ButtonScope) -> Self {
        self.scope = scope;
        self
    }

    // A toggle that's on: the accent tint.
    pub fn toggled(mut self, on: bool) -> Self {
        self.toggled = on;
        self
    }

    // Busy without changing size: the content stays in place, invisible, under the mini wave.
    pub fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }

    // Busy: the mini wave and `label` in place of the content, until the caller clears it.
    pub fn loading(mut self, label: Option<impl Into<SharedString>>) -> Self {
        self.loading = label.map(Into::into);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let control = control::button_control(self.id, self.base, self.variant, self.scope, self.size, self.disabled, self.glyph, self.tooltip, cx)
            .toggled(self.toggled, cx)
            .highlighted(self.highlighted)
            .off(self.off, cx)
            .succeeded(self.succeeded, cx)
            .when(self.round, |c| c.round())
            .when(self.bordered, |c| c.bordered(cx))
            .busy(self.busy)
            .when(self.danger, |c| c.danger(cx))
            .loading(self.loading);
        gpui_kit::ParentElement::children(control, self.children).render(window, cx)
    }
}

impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Button {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Button {}

impl ParentElement for Button {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}
