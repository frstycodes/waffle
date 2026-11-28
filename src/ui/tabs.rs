// Option strips with one sliding indicator: underline tabs, pill tabs and the segmented control. Like
// the design (site.js measures the chosen option and slides one element there), each strip measures
// its options after layout and moves a single indicator to the chosen one, so a change slides across
// rather than one mark collapsing while another grows.
use std::{rc::Rc, time::Duration};

use gpui_kit::{
    App, Bounds, Div, ElementId, Entity, ParentElement, Pixels, SharedString, StatefulInteractiveElement, Styled, Window,
    base::{Easing, Transition, transition},
    div, prelude::FluentBuilder, px,
};

use super::{RADIUS_MD, RADIUS_SM, Squircle, control::{TabKind, part, tab_option}, Badge, seg_lift};
use crate::{icons::Glyph, motion, palette};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    Underline,
    Pills,
    Segmented,
}

// One option in a strip: its label, with an optional lead glyph and trailing count.
#[derive(Clone)]
pub struct Tab {
    label: SharedString,
    glyph: Option<Glyph>,
    count: Option<u32>,
    // The strip's call to action (Add friend): accent ink on an accent tint, a deeper tint when chosen.
    accent: bool,
}

impl Tab {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self { label: label.into(), glyph: None, count: None, accent: false }
    }

    pub fn accent(mut self) -> Self {
        self.accent = true;
        self
    }

    pub fn glyph(mut self, glyph: Glyph) -> Self {
        self.glyph = Some(glyph);
        self
    }

    pub fn count(mut self, count: Option<u32>) -> Self {
        self.count = count;
        self
    }
}


fn underline(id: impl Into<ElementId>, items: &[Tab], selected: usize, on_select: impl Fn(usize, &mut Window, &mut App) + 'static, window: &mut Window, cx: &mut App) -> Div {
    strip(id.into(), Style::Underline, items, selected, Rc::new(on_select), window, cx)
}

// Pill tabs filter a list inside one view.
fn pills(id: impl Into<ElementId>, items: &[Tab], selected: usize, on_select: impl Fn(usize, &mut Window, &mut App) + 'static, window: &mut Window, cx: &mut App) -> Div {
    strip(id.into(), Style::Pills, items, selected, Rc::new(on_select), window, cx)
}

// A recessed track with one raised thumb that springs between options sized to their labels.
fn segmented(id: impl Into<ElementId>, items: &[Tab], selected: usize, on_select: impl Fn(usize, &mut Window, &mut App) + 'static, window: &mut Window, cx: &mut App) -> Div {
    // A row wrapper takes whatever width the parent stretches it to, so the track itself hugs its options.
    div().flex().child(strip(id.into(), Style::Segmented, items, selected, Rc::new(on_select), window, cx))
}

type OnSelect = Rc<dyn Fn(usize, &mut Window, &mut App)>;

// The options' bounds from the last layout, relative to the strip; the first entry is the strip itself.
#[derive(Default)]
struct Measured(Vec<Bounds<Pixels>>);

fn strip(id: ElementId, style: Style, items: &[Tab], selected: usize, on_select: OnSelect, window: &mut Window, cx: &mut App) -> Div {
    let measured = window.use_keyed_state(part(&id, "measured"), cx, |_, _| Measured::default());
    let accent_chosen = items.get(selected).is_some_and(|t| t.accent);
    let indicator = slide(&id, style, &measured, selected, accent_chosen, window, cx);
    let (accent_tint, accent_ink) = (palette::accent_soft(cx), palette::accent_ink(cx));
    let kind = match style {
        Style::Underline => TabKind::Underline,
        Style::Pills => TabKind::Pill,
        Style::Segmented => TabKind::Segment,
    };
    let options = items.iter().enumerate().map(|(i, tab)| {
        let on_select = on_select.clone();
        tab_option(part_n(&id, i), kind, i == selected, tab.glyph, cx)
            .child(tab.label.clone())
            .when_some(tab.count, |o, n| o.child(Badge::count(n)))
            .when(tab.accent, |o| o.text_color(accent_ink).when(i != selected, |o| o.squircle(RADIUS_SM).bg(accent_tint)))
            .on_click(move |_, window, cx| on_select(i, window, cx))
    });
    let frame = match style {
        Style::Underline => div().flex().gap(px(16.)).border_b_1().border_color(palette::line(cx)),
        Style::Pills => div().flex().flex_wrap().gap(px(4.)),
        Style::Segmented => div().flex().p(px(3.)).gap(px(2.)).squircle(RADIUS_MD).bg(palette::seg_track(cx)),
    };
    frame
        .relative()
        // The measuring layer comes first: its bounds are the strip's, the options' follow.
        .child(div().absolute().top_0().left_0().size_full().children(indicator))
        .children(options)
        .on_children_prepainted(move |bounds, _, cx| {
            measured.update(cx, |m, cx| {
                if m.0 != bounds {
                    m.0 = bounds;
                    cx.notify();
                }
            })
        })
}

// The indicator at the chosen option, sliding there over the strip's timing once it has been measured.
fn slide(id: &ElementId, style: Style, measured: &Entity<Measured>, selected: usize, accent: bool, window: &mut Window, cx: &mut App) -> Option<Div> {
    let target = {
        let bounds = &measured.read(cx).0;
        let (strip, option) = (bounds.first()?, bounds.get(selected + 1)?);
        option.origin - strip.origin
    };
    let size = measured.read(cx).0[selected + 1].size;
    let (duration, easing): (Duration, Easing) = match style {
        Style::Segmented => (Duration::from_millis(280), motion::ease_spring()),
        _ => (Duration::from_millis(220), motion::ease_move()),
    };
    let channel = |name: &'static str, value: f32, window: &mut Window, cx: &mut App| transition(part(id, name), value, Transition::new(duration).easing(easing.clone()), window, cx);
    let x = channel("x", target.x.into(), window, cx);
    let w = channel("w", size.width.into(), window, cx);
    let y = channel("y", target.y.into(), window, cx);
    let h = channel("h", size.height.into(), window, cx);
    let base = div().absolute().left(px(x)).w(px(w));
    Some(match style {
        Style::Underline => base.bottom_0().h(px(2.)).squircle(2.).bg(palette::accent()),
        Style::Pills => base.top(px(y)).h(px(h)).squircle(RADIUS_SM).bg(if accent { palette::accent().opacity(0.3) } else { palette::selected(cx) }),
        Style::Segmented => base.top(px(y)).h(px(h)).squircle(RADIUS_MD - 3.).bg(palette::seg_thumb(cx)).shadow(seg_lift(cx)),
    })
}

fn part_n(id: &ElementId, i: usize) -> ElementId {
    ElementId::NamedChild(std::sync::Arc::new(id.clone()), SharedString::from(format!("option-{i}")))
}

// How tabs look: the recessed segmented track, or the underline strip.
// Keywords: tabs variant, segmented, underline, line tabs, shadcn
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TabsVariant {
    // A recessed track with one raised thumb that springs between options sized to their labels.
    #[default]
    Default,
    // Underline tabs split a page into views.
    Line,
}

type OnValueChange = Box<dyn Fn(usize, &mut Window, &mut App) + 'static>;

// shadcn/ui's Tabs: `Tabs::new(id).tabs(&[Tab::new("Overview")]).value(0).on_value_change(|i, window, cx| ..)`. The default
// variant is the segmented track, `Line` the underline strip.
// Keywords: tabs, segmented control, underline tabs, view switch, tab strip, segment, shadcn
#[derive(gpui_kit::IntoElement)]
pub struct Tabs {
    base: Div,
    id: ElementId,
    variant: TabsVariant,
    items: Vec<Tab>,
    value: usize,
    on_value_change: Option<OnValueChange>,
}

impl Tabs {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self { base: div(), id: id.into(), variant: TabsVariant::default(), items: Vec::new(), value: 0, on_value_change: None }
    }

    pub fn variant(mut self, variant: TabsVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn tabs(mut self, items: &[Tab]) -> Self {
        self.items = items.to_vec();
        self
    }

    // Tabs of plain labels.
    pub fn labels(mut self, labels: &[&'static str]) -> Self {
        self.items = labels.iter().map(|l| Tab::new(*l)).collect();
        self
    }

    pub fn value(mut self, value: usize) -> Self {
        self.value = value;
        self
    }

    pub fn on_value_change(mut self, change: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_value_change = Some(Box::new(change));
        self
    }
}

impl gpui_kit::RenderOnce for Tabs {
    fn render(self, window: &mut Window, cx: &mut App) -> impl gpui_kit::IntoElement {
        let change = self.on_value_change.unwrap_or_else(|| Box::new(|_, _, _| {}));
        let strip = match self.variant {
            TabsVariant::Line => underline(self.id, &self.items, self.value, change, window, cx),
            TabsVariant::Default => segmented(self.id, &self.items, self.value, change, window, cx),
        };
        self.base.child(strip)
    }
}

// shadcn/ui's ToggleGroup, single choice: pill options that filter a list inside one view.
// Keywords: toggle group, pill tabs, filter pills, filter strip, chips row, single choice, shadcn
#[derive(gpui_kit::IntoElement)]
pub struct ToggleGroup {
    base: Div,
    id: ElementId,
    items: Vec<Tab>,
    value: usize,
    on_value_change: Option<OnValueChange>,
}

impl ToggleGroup {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self { base: div(), id: id.into(), items: Vec::new(), value: 0, on_value_change: None }
    }

    pub fn items(mut self, items: &[Tab]) -> Self {
        self.items = items.to_vec();
        self
    }

    pub fn value(mut self, value: usize) -> Self {
        self.value = value;
        self
    }

    pub fn on_value_change(mut self, change: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_value_change = Some(Box::new(change));
        self
    }
}

impl gpui_kit::RenderOnce for ToggleGroup {
    fn render(self, window: &mut Window, cx: &mut App) -> impl gpui_kit::IntoElement {
        let change = self.on_value_change.unwrap_or_else(|| Box::new(|_, _, _| {}));
        self.base.child(pills(self.id, &self.items, self.value, change, window, cx))
    }
}

impl Styled for Tabs {
    fn style(&mut self) -> &mut gpui_kit::StyleRefinement {
        self.base.style()
    }
}

impl Styled for ToggleGroup {
    fn style(&mut self) -> &mut gpui_kit::StyleRefinement {
        self.base.style()
    }
}
