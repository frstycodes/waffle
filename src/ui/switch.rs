// shadcn/ui's Switch: a wide pill with a capsule knob, honey when on (iOS-style).
use std::{sync::atomic::{AtomicBool, Ordering}, time::Duration};

use gpui_kit::{
    AnyElement, App, BoxShadow, Div, ElementId, InteractiveElement, Interactivity, IntoElement, MouseButton, ParentElement, RenderOnce, Stateful, StatefulInteractiveElement, StyleRefinement, Styled, Window,
    base::{Transition, transition},
    component::Icon, div, prelude::FluentBuilder, px,
};

use super::{DUR_POP, control, hover_fade, mix, shadow, stretch};
use crate::{icons::Glyph, motion, palette};

// Settings › Accessibility's switch indicators, read while laying out.
static SWITCH_ICONS: AtomicBool = AtomicBool::new(false);

pub fn set_switch_icons(on: bool) {
    SWITCH_ICONS.store(on, Ordering::Relaxed);
}

// A setting that applies at once. The knob springs across (300ms on the touch spring) while the track's fill
// fades to the accent, and while pressed it swells into glass. The caller toggles on `.on_click`.
// Keywords: switch, toggle switch, on off, setting, boolean, shadcn
#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    base: Stateful<Div>,
    checked: bool,
    children: Vec<AnyElement>,
}

impl Switch {
    pub fn new(id: impl Into<ElementId>) -> Self {
        let id = id.into();
        Self { base: div().id(id.clone()), id, checked: false, children: Vec::new() }
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let id = self.id;
        let held = window.use_keyed_state(control::part(&id, "held"), cx, |_, _| false);
        let pressed = *held.read(cx);
        let (down, up, up_out) = (hold(&held, true, window), hold(&held, false, window), hold(&held, false, window));
        track(self.base, &id, self.checked, pressed, window, cx)
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, down)
            .on_mouse_up(MouseButton::Left, up)
            .on_mouse_up_out(MouseButton::Left, up_out)
            .children(self.children)
    }
}

impl Styled for Switch {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Switch {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Switch {}

impl ParentElement for Switch {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

// A mouse listener that records whether the switch is held down.
pub(super) fn hold<E: 'static>(held: &gpui_kit::Entity<bool>, value: bool, window: &Window) -> impl Fn(&E, &mut Window, &mut App) + 'static {
    window.listener_for(held, move |h: &mut bool, _: &E, _, cx| {
        *h = value;
        cx.notify();
    })
}

// The switch's track and knob, `pressed` coming from whoever holds it (the switch, or its toggle row).
pub(super) fn track<E: Styled + ParentElement>(base: E, id: &ElementId, on: bool, pressed: bool, window: &mut Window, cx: &mut App) -> E {
    let spring = || Transition::new(Duration::from_millis(300)).easing(motion::ease_spring());
    let travel = transition(control::part(id, "travel"), if on { 1. } else { 0. }, spring(), window, cx);
    let swell = transition(control::part(id, "swell"), if pressed { 1. } else { 0. }, spring(), window, cx);
    let fill = transition(control::part(id, "fill"), if on { 1. } else { 0. }, Transition::new(DUR_POP).easing(motion::ease_out()), window, cx);
    let glass = hover_fade(control::part(id, "glass"), pressed, window, cx);
    let knob = stretch(div().absolute().top(px(2.)).left(px(2.)).w(px(22.)).h(px(20.)).translate_x(px(14. * travel)), 1. + 0.18 * swell, 1. + 0.25 * swell)
        .rounded_full()
        .bg(mix(gpui_kit::white(), gpui_kit::white().opacity(0.42), glass))
        .shadow(knob_glass(glass))
        // The backdrop blur keeps to the knob's unscaled box: gpui's blur doesn't follow transforms.
        .when(glass > 0., |k| k.backdrop_blur(px(2. * glass)).backdrop_saturate(1. + 0.8 * glass))
        .when(SWITCH_ICONS.load(Ordering::Relaxed), |k| k.flex().items_center().justify_center().child(switch_indicator(fill > 0.5)));
    base.w(px(40.)).h(px(24.)).flex_none().relative().rounded_full().bg(mix(palette::track(cx), palette::accent(), fill.clamp(0., 1.))).child(knob)
}

// "Show on/off indicators": a tick or a cross in the knob, which is white in both themes.
fn switch_indicator(on: bool) -> Icon {
    let (glyph, alpha) = if on { (Glyph::Checkmark, 1.) } else { (Glyph::X, 0.6) };
    Icon::new(glyph).size(px(12.)).text_color(palette::on_accent().opacity(alpha))
}

// The knob's shadows, from resting (`t` 0) to pressed glass (`t` 1): a lit inner rim and a deeper drop.
fn knob_glass(t: f32) -> Vec<BoxShadow> {
    if t <= 0. {
        return knob_shadow();
    }
    let (white, black) = (gpui_kit::white(), gpui_kit::black());
    vec![
        shadow(white.opacity(0.7 * t), 0., 0., 0., t, true),
        shadow(white.opacity(0.9 * t), 0., t, t, 0., true),
        shadow(black.opacity(0.22 + 0.03 * t), 0., 2. + 2. * t, 6. + 6. * t, 0., false),
    ]
}

fn knob_shadow() -> Vec<BoxShadow> {
    let black = gpui_kit::black();
    vec![shadow(black.opacity(0.22), 0., 2., 6., 0., false), shadow(black.opacity(0.06), 0., 0., 0., 0.5, false)]
}

