// Pressable controls: buttons, icon buttons, round media buttons, chips, colour swatches, choice and menu
// rows. Each is a RenderOnce that fades its fill, ink and edge over DUR_HOVER and eases its press scale over
// DUR_PRESS, the way the design's CSS transitions do (gpui's own `.hover` and `.active` styles swap in one
// frame), and keeps the caller's builder chain: `.child`, `.on_click`, `.tooltip`, layout. A fill, ink or
// corner the caller sets wins over the control's own.
use std::sync::Arc;

use gpui_kit::{
    AnyElement, AnyView, App, AppContext, BoxShadow, Div, ElementId, FontWeight, Hsla, InteractiveElement, Interactivity, IntoElement, MouseButton, ParentElement, Render, RenderOnce, SharedString,
    Stateful, StatefulInteractiveElement, StyleRefinement, Styled, Window,
    base::{Easing, Transition, transition},
    component::Icon,
    div, point, prelude::FluentBuilder, px, relative,
};

use super::{DUR_HOVER, DUR_PRESS, RADIUS_MD, RADIUS_SM, RADIUS_XS, SQUIRCLE, Tone, mix};
use crate::{icons::Glyph, motion, palette};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Button,
    Icon,
    Round(u8),
    Chip,
    Swatch,
    Tab(TabKind),
    Field,
    // Full-width rows (choices, menu items) and bare sides (the stepper's): they fill but never scale.
    Row,
}

// The options of a strip (`tabs.rs`): underline tabs, pill tabs, segmented options.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum TabKind {
    Underline,
    Pill,
    Segment,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Size {
    Small,
    #[default]
    Medium,
    Large,
}

// A control's colours at rest, hovered and pressed; each change fades over the hover timing.
#[derive(Clone, Copy)]
struct Look {
    rest: Hsla,
    hover: Hsla,
    press: Hsla,
    ink: Hsla,
    ink_hover: Hsla,
    edge: Option<(Hsla, Hsla)>,
}

#[derive(IntoElement)]
pub struct Control {
    id: ElementId,
    base: Stateful<Div>,
    children: Vec<AnyElement>,
    kind: Kind,
    size: Size,
    look: Look,
    glyph: Option<Glyph>,
    disabled: bool,
    highlighted: bool,
    loading: Option<SharedString>,
    // In flight with its size kept: the content hidden under the mini wave (Discord's loading button).
    busy: bool,
    ring: Option<Hsla>,
}

// A text button, 36px tall (`.large()` 44, `.small()` 30). Callers add the label and `.on_click`.
pub fn button(id: impl Into<ElementId>, tone: Tone, cx: &App) -> Control {
    let control = Control::new(id, Kind::Button, tone_look(tone, cx));
    // A link is just its ink: no box, no padding, underlined on hover.
    if tone != Tone::Link {
        return control;
    }
    control.base_style(|b| b.h_auto().px_0().rounded(px(RADIUS_XS)).hover(|s| s.underline()))
}

// A square icon-only button that fills on hover, with its tooltip: 32px (`.small()` 24, `.large()` 40).
pub fn icon_button(id: impl Into<ElementId>, glyph: Glyph, tooltip: impl Into<SharedString>, cx: &App) -> Control {
    let look = Look {
        rest: gpui_kit::transparent_black(),
        hover: palette::hover(cx),
        press: palette::press(cx),
        ink: palette::muted(cx),
        ink_hover: palette::strong(cx),
        edge: None,
    };
    let tooltip: SharedString = tooltip.into();
    let mut control = Control::new(id, Kind::Icon, look);
    control.glyph = Some(glyph);
    control.tooltip(move |window: &mut Window, cx: &mut App| super::tip(tooltip.clone(), window, cx))
}

// A round icon button over video or the call stage (`size` 28 or 40).
pub fn round_button(id: impl Into<ElementId>, glyph: Glyph, tooltip: impl Into<SharedString>, size: f32) -> Control {
    let white = gpui_kit::white();
    let look = Look { rest: white.opacity(0.12), hover: white.opacity(0.2), press: white.opacity(0.28), ink: white, ink_hover: white, edge: None };
    let tooltip: SharedString = tooltip.into();
    let mut control = Control::new(id, Kind::Round(size as u8), look);
    control.glyph = Some(glyph);
    control.tooltip(move |window: &mut Window, cx: &mut App| super::tip(tooltip.clone(), window, cx))
}

// A filter or toggle chip; pressed takes the accent tint and a half-accent edge.
pub fn chip(id: impl Into<ElementId>, glyph: Option<Glyph>, label: impl Into<SharedString>, on: bool, cx: &App) -> Control {
    chip_look(id, on, cx).when_some(glyph, |c, glyph| c.child(Icon::new(glyph).size(px(16.)))).child(label.into())
}

fn chip_look(id: impl Into<ElementId>, on: bool, cx: &App) -> Control {
    let look = if on {
        let soft = palette::accent_soft(cx);
        let edge = palette::accent().opacity(0.5);
        Look { rest: soft, hover: soft, press: soft, ink: palette::accent_ink(cx), ink_hover: palette::accent_ink(cx), edge: Some((edge, edge)) }
    } else {
        let edge = palette::line_strong(cx);
        Look { rest: gpui_kit::transparent_black(), hover: palette::hover(cx), press: palette::press(cx), ink: palette::text(cx), ink_hover: palette::text(cx), edge: Some((edge, edge)) }
    };
    Control::new(id, Kind::Chip, look)
}

// A colour choice (role colours): a filled square, ringed and ticked when chosen. `ring` is the surface behind it.
pub fn swatch(id: impl Into<ElementId>, color: Hsla, on: bool, ring: Hsla) -> Control {
    let look = Look { rest: color, hover: color, press: color, ink: gpui_kit::white(), ink_hover: gpui_kit::white(), edge: None };
    let mut control = Control::new(id, Kind::Swatch, look);
    control.ring = on.then_some(ring);
    control.when(on, |c| c.child(Icon::new(Glyph::Checkmark).size(px(16.))))
}

// A row inside a floating surface (menu, popover, picker): no fill until hovered, then the hover fill and
// the strong ink fade in. Callers add the lead glyph, the label (`flex_1`) and any trailing mark;
// `.danger(cx)` makes it the destructive row.
pub fn menu_row(id: impl Into<ElementId>, cx: &App) -> Control {
    let hover = palette::hover(cx);
    let look = Look { rest: gpui_kit::transparent_black(), hover, press: hover, ink: palette::text(cx), ink_hover: palette::strong(cx), edge: None };
    Control::new(id, Kind::Row, look).base_style(|b| b.w_full().h_auto().min_h(px(34.)).px(px(10.)).py(px(6.)).gap(px(10.)).justify_start().text_size(px(14.)).font_weight(FontWeight::NORMAL))
}

// A bare control without glyph or tooltip, for callers that draw their own content (stepper sides).
pub(super) fn icon_control(id: impl Into<ElementId>, cx: &App) -> Control {
    let look = Look { rest: gpui_kit::transparent_black(), hover: palette::hover(cx), press: palette::press(cx), ink: palette::muted(cx), ink_hover: palette::strong(cx), edge: None };
    Control::new(id, Kind::Row, look)
}

// A field-shaped trigger (dropdowns): the page fill, an edge that firms on hover and takes the accent while open.
pub(super) fn field_trigger(id: impl Into<ElementId>, open: bool, cx: &App) -> Control {
    let page = palette::surface(palette::Level::Page, cx);
    let edge = if open { (palette::accent(), palette::accent()) } else { (palette::line_strong(cx), palette::faint(cx).opacity(0.6)) };
    let ink = palette::strong(cx);
    Control::new(id, Kind::Field, Look { rest: page, hover: page, press: page, ink, ink_hover: ink, edge: Some(edge) }).w_full().justify_start()
}

// A full-width row around a choice and its text (`ui::toggle_row`, `ui::pick_row`): no fill until hovered.
pub(super) fn choice(id: impl Into<ElementId>, cx: &App) -> Control {
    let clear = gpui_kit::transparent_black();
    let ink = palette::text(cx);
    Control::new(id, Kind::Row, Look { rest: clear, hover: palette::hover(cx), press: palette::press(cx), ink, ink_hover: ink, edge: None })
        .base_style(|b| b.w_full().h_auto().px(px(12.)).py(px(10.)).gap(px(12.)).rounded(px(RADIUS_MD)).justify_start().text_size(px(14.)))
}

// One option in a strip: muted ink that strengthens on hover; the strip draws the selection itself.
pub(super) fn tab_option(id: impl Into<ElementId>, kind: TabKind, on: bool, glyph: Option<Glyph>, cx: &App) -> Control {
    let clear = gpui_kit::transparent_black();
    let (muted, strong) = (palette::muted(cx), palette::strong(cx));
    let hover = if kind == TabKind::Pill && !on { palette::hover(cx) } else { clear };
    let ink = if on { strong } else { muted };
    let mut control = Control::new(id, Kind::Tab(kind), Look { rest: clear, hover, press: hover, ink, ink_hover: strong, edge: None });
    control.glyph = glyph;
    control
}

impl Control {
    fn new(id: impl Into<ElementId>, kind: Kind, look: Look) -> Self {
        let id = id.into();
        Self { base: div().id(id.clone()), id, children: Vec::new(), kind, size: Size::Medium, look, glyph: None, disabled: false, highlighted: false, loading: None, busy: false, ring: None }
    }

    fn base_style(mut self, f: impl FnOnce(Stateful<Div>) -> Stateful<Div>) -> Self {
        self.base = f(self.base);
        self
    }

    pub fn small(mut self) -> Self {
        self.size = Size::Small;
        self
    }

    pub fn large(mut self) -> Self {
        self.size = Size::Large;
        self
    }

    // A toggle that's on: the accent tint for buttons, the selected fill for icon buttons.
    pub fn toggled(mut self, on: bool, cx: &App) -> Self {
        if !on {
            return self;
        }
        self.look = match self.kind {
            Kind::Icon => {
                let (fill, ink) = (palette::selected(cx), palette::strong(cx));
                Look { rest: fill, hover: fill, press: palette::press(cx), ink, ink_hover: ink, edge: None }
            }
            // The chosen radio row or option: a fill a step above hover, deeper again under the pointer.
            Kind::Row => {
                let strong = palette::strong(cx);
                let hover = strong.opacity(0.13);
                Look { rest: palette::press(cx), hover, press: hover, ink: strong, ink_hover: strong, edge: None }
            }
            _ => {
                let (soft, ink) = (palette::accent_soft(cx), palette::accent_ink(cx));
                Look { rest: soft, hover: soft, press: soft, ink, ink_hover: ink, edge: self.look.edge }
            }
        };
        self
    }

    // An icon button for something switched off (a muted mic): coral ink.
    pub fn off(mut self, off: bool, cx: &App) -> Self {
        if off {
            self.look.ink = palette::danger_ink(cx);
            self.look.ink_hover = palette::danger_ink(cx);
        }
        self
    }

    // The destructive row: danger ink, and on hover the danger fill under white ink.
    pub fn danger(mut self, cx: &App) -> Self {
        let fill = palette::danger();
        self.look = Look { hover: fill, press: fill, ink: palette::danger_ink(cx), ink_hover: gpui_kit::white(), ..self.look };
        self
    }

    // The menu's one accent row (Invite people): accent ink, and the accent tint on hover.
    pub fn accent(mut self, cx: &App) -> Self {
        let ink = palette::accent_ink(cx);
        self.look = Look { hover: palette::accent_soft(cx), press: palette::accent_soft(cx), ink, ink_hover: ink, ..self.look };
        self
    }

    // Shows the hover state without the pointer: the row a submenu flies out of, the keyboard's row.
    pub fn highlighted(mut self, on: bool) -> Self {
        self.highlighted = on;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    // Busy: the mini wave and `label` in place of the content, until the caller clears it.
    pub fn loading(mut self, label: Option<impl Into<SharedString>>) -> Self {
        self.loading = label.map(Into::into);
        self
    }

    // Busy without changing size: the content stays in place, invisible, under the mini wave.
    pub fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }

    fn dimensions(&self) -> (f32, f32, f32) {
        // (height, radius, icon size); buttons size their width from padding.
        match (self.kind, self.size) {
            (Kind::Button, Size::Small) => (30., RADIUS_SM, 18.),
            (Kind::Button, Size::Medium) => (36., RADIUS_MD, 18.),
            (Kind::Button, Size::Large) => (44., RADIUS_MD, 18.),
            (Kind::Icon, Size::Small) => (24., RADIUS_XS, 16.),
            (Kind::Icon, Size::Medium) => (32., RADIUS_SM, 20.),
            (Kind::Icon, Size::Large) => (40., RADIUS_MD, 22.),
            (Kind::Round(size), _) => (size as f32, size as f32 / 2., size as f32 / 2.),
            (Kind::Chip, _) => (30., RADIUS_SM, 16.),
            (Kind::Swatch, _) => (32., RADIUS_SM, 16.),
            (Kind::Field, _) => (40., RADIUS_MD, 18.),
            (Kind::Tab(TabKind::Underline), _) => (40., 0., 16.),
            (Kind::Tab(TabKind::Pill), _) => (30., RADIUS_SM, 16.),
            (Kind::Tab(TabKind::Segment), _) => (28., RADIUS_MD - 3., 16.),
            (Kind::Row, _) => (34., RADIUS_SM, 18.),
        }
    }

    // Horizontal padding, text size and weight by kind; `None` where the kind sets none.
    fn type_and_padding(&self) -> Option<(f32, f32, FontWeight)> {
        match (self.kind, self.size) {
            (Kind::Button, Size::Small) => Some((10., 14., FontWeight::MEDIUM)),
            (Kind::Button, Size::Medium) => Some((14., 14., FontWeight::MEDIUM)),
            (Kind::Button, Size::Large) => Some((20., 16., FontWeight::MEDIUM)),
            (Kind::Chip, _) => Some((12., 14., FontWeight::NORMAL)),
            (Kind::Field, _) => Some((12., 14., FontWeight::NORMAL)),
            (Kind::Tab(TabKind::Underline), _) => Some((0., 14., FontWeight::MEDIUM)),
            (Kind::Tab(_), _) => Some((12., 14., FontWeight::MEDIUM)),
            _ => None,
        }
    }

    // How far the design's `:active` scales the control, on which curve; `None` for kinds that hold still.
    fn press(&self) -> Option<(f32, Easing)> {
        match self.kind {
            Kind::Button | Kind::Chip => Some((0.97, motion::ease_out())),
            Kind::Icon | Kind::Round(_) => Some((0.92, motion::ease_out())),
            Kind::Swatch => Some((0.9, motion::ease_spring())),
            Kind::Tab(TabKind::Segment) => Some((0.96, motion::ease_out())),
            Kind::Tab(_) | Kind::Field | Kind::Row => None,
        }
    }
}

// Whether the pointer is over a control and holding it down, kept per control so its fill and scale ease
// rather than swap.
#[derive(Default)]
struct Pointer {
    hovered: bool,
    pressed: bool,
}

// Whether the control `id` is held down, for parts that answer the whole control's press (a toggle row's
// switch swells while the row is held).
pub(super) fn pressed(id: &ElementId, window: &mut Window, cx: &mut App) -> bool {
    window.use_keyed_state(part(id, "pointer"), cx, |_, _| Pointer::default()).read(cx).pressed
}

impl RenderOnce for Control {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let pointer = window.use_keyed_state(part(&self.id, "pointer"), cx, |_, _| Pointer::default());
        let (hovered, pressed) = {
            let p = pointer.read(cx);
            ((p.hovered || self.highlighted) && !self.disabled, p.pressed && !self.disabled)
        };
        // A disabled row goes faint and still; other kinds fade to 0.45 below.
        let faint_row = self.disabled && self.kind == Kind::Row;
        let t = hover_fade(part(&self.id, "hover"), hovered, window, cx);
        let scale = self.press().map(|(to, curve)| press_scale(&self.id, pressed, to, curve, window, cx));
        let (height, radius, icon) = self.dimensions();
        let type_and_padding = self.type_and_padding();
        let look = if faint_row { faint_look(cx) } else { self.look };
        let held = hover_fade(part(&self.id, "held"), pressed, window, cx);
        let (fill, ink) = (mix(mix(look.rest, look.hover, t), look.press, held), mix(look.ink, look.ink_hover, t));
        let mut base = self.base;
        // Whatever the caller set on the builder chain wins over the kind's defaults.
        let caller = base.style().clone();
        let square = matches!(self.kind, Kind::Icon | Kind::Round(_) | Kind::Swatch);
        let round = matches!(self.kind, Kind::Round(_));
        let gap = if matches!(self.kind, Kind::Chip | Kind::Tab(_)) { 6. } else { 8. };
        base = base
            .when(caller.position.is_none(), |b| b.relative())
            .when(caller.flex_grow.is_none() && caller.flex_shrink.is_none(), |b| b.flex_none())
            .flex()
            .when(caller.align_items.is_none(), |b| b.items_center())
            .when(caller.justify_content.is_none(), |b| b.justify_center())
            .when(type_and_padding.is_some(), |b| b.whitespace_nowrap())
            .when(caller.size.height.is_none(), |b| b.h(px(height)))
            .when(square && caller.size.width.is_none(), |b| b.w(px(height)))
            .when(caller.corner_radii.top_left.is_none(), |b| b.rounded(px(radius)))
            .when(caller.corner_shape.is_none() && !round, |b| b.corner_shape(SQUIRCLE))
            .when_some(scale.filter(|s| *s < 1.), |b, s| b.scale(s))
            .when(caller.gap.width.is_none(), |b| b.gap(px(gap)))
            .when_some(type_and_padding, |b, (pad, size, weight)| {
                b.when(caller.padding.left.is_none(), |b| b.px(px(pad)))
                    .when(caller.text.font_size.is_none(), |b| b.text_size(px(size)))
                    .when(caller.text.font_weight.is_none(), |b| b.font_weight(weight))
            })
            .when(caller.background.is_none(), |b| b.bg(fill))
            .when(caller.text.color.is_none(), |b| b.text_color(ink))
            .when_some(look.edge, |b, (rest, hover)| b.border_1().border_color(mix(rest, hover, t)))
            .when_some(self.ring, |b, ring| b.shadow(halo(ring, look.rest)))
            .when(!self.disabled, |b| b.cursor_pointer())
            .when(self.disabled && !faint_row, |b| b.opacity(0.45));
        let base = base.child(hit_area(&self.id, &pointer, self.disabled, window)).when_some(self.glyph, |b, glyph| b.child(Icon::new(glyph).size(px(icon))));
        let bars = super::bars(part(&self.id, "busy"), ink, 12.);
        match (self.loading, self.busy) {
            (Some(label), _) => base.child(bars).child(label),
            (None, true) => base
                .child(div().flex().items_center().gap(px(gap)).invisible().children(self.children))
                .child(div().absolute().inset_0().flex().items_center().justify_center().child(bars)),
            (None, false) => base.children(self.children),
        }
    }
}

// How far a held element has shrunk toward `to`, eased over the press timing on `curve`.
fn press_scale(id: &ElementId, pressed: bool, to: f32, curve: Easing, window: &mut Window, cx: &mut App) -> f32 {
    let held = transition(part(id, "press"), if pressed { 1. } else { 0. }, Transition::new(DUR_PRESS).easing(curve), window, cx);
    1. - (1. - to) * held
}

// Any element with an id that shrinks to `to` while held, eased like a Control's press (gpui's `.active`
// style swaps in one frame). Keeps the caller's builder chain: `.child`, `.on_click`, `.hover`, layout.
pub fn pressable(base: Stateful<Div>, to: f32) -> Pressable {
    Pressable { base, to }
}

#[derive(IntoElement)]
pub struct Pressable {
    base: Stateful<Div>,
    to: f32,
}

impl RenderOnce for Pressable {
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(id) = self.base.interactivity().element_id.clone() else { return self.base };
        let pointer = window.use_keyed_state(part(&id, "pointer"), cx, |_, _| Pointer::default());
        let pressed = pointer.read(cx).pressed;
        let scale = press_scale(&id, pressed, self.to, motion::ease_out(), window, cx);
        let positioned = self.base.style().position.is_some();
        self.base.when(!positioned, |b| b.relative()).when(scale < 1., |b| b.scale(scale)).child(hit_area(&id, &pointer, false, window))
    }
}

impl Styled for Pressable {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Pressable {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Pressable {}

impl ParentElement for Pressable {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.base.extend(elements);
    }
}

// The transparent layer that tracks the pointer, so the caller keeps `on_hover` and the mouse handlers
// on the control itself. Disabled, it swallows the pointer so the control's own click never fires.
fn hit_area(id: &ElementId, pointer: &gpui_kit::Entity<Pointer>, disabled: bool, window: &mut Window) -> Stateful<Div> {
    div()
        .id(part(id, "hit"))
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .on_hover(window.listener_for(pointer, |p, hovered: &bool, _, cx| {
            p.hovered = *hovered;
            cx.notify();
        }))
        .on_mouse_down(MouseButton::Left, hold(pointer, true, window))
        .on_mouse_up(MouseButton::Left, hold(pointer, false, window))
        .on_mouse_up_out(MouseButton::Left, hold(pointer, false, window))
        .when(disabled, |d| d.occlude().cursor_not_allowed())
}

// A mouse listener that records whether the control is held down.
fn hold<E: 'static>(pointer: &gpui_kit::Entity<Pointer>, held: bool, window: &Window) -> impl Fn(&E, &mut Window, &mut App) + 'static {
    window.listener_for(pointer, move |p: &mut Pointer, _: &E, _, cx| {
        if p.pressed != held {
            p.pressed = held;
            cx.notify();
        }
    })
}

fn faint_look(cx: &App) -> Look {
    let (clear, faint) = (gpui_kit::transparent_black(), palette::faint(cx));
    Look { rest: clear, hover: clear, press: clear, ink: faint, ink_hover: faint, edge: None }
}

// A 2px gap in the surface's colour, then a 2px ring in the control's: the chosen swatch.
fn halo(surface: Hsla, color: Hsla) -> Vec<BoxShadow> {
    let ring = |spread: f32, color: Hsla| BoxShadow { color, offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(spread), inset: false };
    vec![ring(4., color), ring(2., surface)]
}

// How far `id`'s hover has faded in, 0 to 1, on the design's hover timing (150ms ease-out).
pub fn hover_fade(id: impl Into<gpui_kit::base::TransitionId>, hovered: bool, window: &mut Window, cx: &mut App) -> f32 {
    transition(id, if hovered { 1. } else { 0. }, Transition::new(DUR_HOVER).easing(motion::ease_out()), window, cx)
}

pub(super) fn part(id: &ElementId, name: &'static str) -> ElementId {
    ElementId::NamedChild(Arc::new(id.clone()), name.into())
}

fn tone_look(tone: Tone, cx: &App) -> Look {
    let white = gpui_kit::white();
    let clear = gpui_kit::transparent_black();
    let (rest, hover, press, ink) = match tone {
        Tone::Primary => (palette::accent(), palette::accent_hover(), palette::accent_press(), palette::on_accent()),
        Tone::Secondary => (palette::selected(cx), palette::press(cx), palette::press(cx), palette::strong(cx)),
        Tone::Ghost => (clear, palette::hover(cx), palette::press(cx), palette::text(cx)),
        Tone::Danger => (palette::danger(), palette::danger_hover(), palette::danger_hover(), white),
        Tone::Positive => (palette::success(), palette::success_hover(), palette::success_hover(), white),
        Tone::OnMedia => (gpui_kit::black().opacity(0.6), gpui_kit::black().opacity(0.75), gpui_kit::black().opacity(0.85), white),
        Tone::Link => (clear, clear, clear, palette::accent_ink(cx)),
    };
    Look { rest, hover, press, ink, ink_hover: ink, edge: None }
}

impl Styled for Control {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Control {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Control {}

impl ParentElement for Control {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

// The design's tooltip: inverted ink, 12px semibold, popping in from 0.95 with its fade over the hover
// timing. gpui places it just below and right of the pointer, so it grows from its top-left corner.
pub struct Tip {
    text: SharedString,
}

pub fn tip(text: impl Into<SharedString>, _: &mut Window, cx: &mut App) -> AnyView {
    let text = text.into();
    cx.new(|_| Tip { text }).into()
}

impl Render for Tip {
    fn render(&mut self, _: &mut Window, cx: &mut gpui_kit::Context<Self>) -> impl IntoElement {
        use gpui_kit::{Animation, AnimationExt};
        div()
            .max_w(px(240.))
            .px(px(10.))
            .py(px(6.))
            .rounded(px(RADIUS_SM))
            .corner_shape(SQUIRCLE)
            .transform_origin(point(relative(0.), relative(0.)))
            .bg(palette::strong(cx))
            .text_color(palette::surface(palette::Level::Page, cx))
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .line_height(relative(1.35))
            .child(self.text.clone())
            .with_animation("tip-in", Animation::new(DUR_HOVER).with_easing(|t| motion::ease_out().sample(t)), |tip, t| tip.opacity(t).scale(0.95 + 0.05 * t))
    }
}
