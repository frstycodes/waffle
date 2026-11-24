// Dialogs: a card over the scrim that pops from its centre and, whenever its content changes (a tab
// switch, a list filling in), springs to its new height instead of snapping.
use std::time::Duration;

use gpui_kit::{
    AnimationElement, AnimationExt, AnyElement, App, Bounds, Div, ElementId, InteractiveElement, IntoElement, MouseButton, ParentElement, ScrollHandle, Stateful, StatefulInteractiveElement, Styled, Window,
    base::{Spring, spring},
    canvas, div, fill, linear_color_stop, linear_gradient, prelude::FluentBuilder, px, size,
};

use super::{DUR_PANEL, DUR_POP, RADIUS_XL, Squircle, control::part, shadow_pop};
use crate::{motion, palette};

// Fast and critically damped: a resize never overshoots, and a change mid-flight retargets from the live height.
const RESIZE: Spring = Spring::new(Duration::from_millis(150)).with_epsilon(0.5);

// Share of the window a dialog may fill before its own scroller takes over.
const MAX_HEIGHT: f32 = 0.92;

/// A dialog's card around `body`, `width` wide. The card owns the height: it measures `body` each
/// layout and springs to it, clipping while it travels. Reduced motion lands on the new height at once.
/// `body` is laid out at its natural height (capped to the window), so a scroller inside it must carry
/// its own `max_h` or `flex_1().min_h_0()`.
pub fn dialog(id: impl Into<ElementId>, width: f32, body: Div, window: &mut Window, cx: &mut App) -> AnimationElement<Stateful<Div>> {
    let id = id.into();
    let measured = window.use_keyed_state(part(&id, "measured"), cx, |_, _| None::<f32>);
    let height = measured.read(cx).map(|target| spring(part(&id, "height"), target, RESIZE, window, cx));
    let cap = f32::from(window.viewport_size().height) * MAX_HEIGHT;
    div()
        .w(px(width))
        .when_some(height, |d, h| d.h(px(h)))
        .overflow_hidden()
        .flex()
        .flex_col()
        .squircle(RADIUS_XL)
        .bg(palette::surface(palette::Level::Raised, cx))
        .shadow(shadow_pop(cx))
        .text_size(px(14.))
        .text_color(palette::text(cx))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(body.flex_none().max_h(px(cap)))
        .on_children_prepainted(move |bounds, _, cx| {
            let Some(body) = bounds.first() else { return };
            let next = Some(f32::from(body.size.height));
            measured.update(cx, |h, cx| {
                if *h != next {
                    *h = next;
                    cx.notify();
                }
            })
        })
        .id(id.clone())
        // Pops from the centre: scale 0.95 with its fade (CSS `pop`).
        .with_animation(part(&id, "pop"), gpui_kit::Animation::new(DUR_PANEL), |card, t| {
            let t = motion::ease_out().sample(t);
            card.opacity(t).scale(0.95 + 0.05 * t)
        })
}

/// A dialog of a fixed size whose content may draw past its edge (a profile's frame over the full profile): the
/// same card, shadow and pop as `dialog`, without the height spring, which needs the card to clip.
// Keywords: unclipped dialog, fixed size dialog, overflowing dialog, frame dialog, modal without clipping
pub fn dialog_fixed(id: impl Into<ElementId>, width: f32, height: f32, body: impl IntoElement, cx: &App) -> AnimationElement<Stateful<Div>> {
    let id = id.into();
    div()
        .w(px(width))
        .h(px(height))
        .flex()
        .flex_col()
        .squircle(RADIUS_XL)
        .bg(palette::surface(palette::Level::Raised, cx))
        .shadow(shadow_pop(cx))
        .text_size(px(14.))
        .text_color(palette::text(cx))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(body)
        .id(id.clone())
        .with_animation(part(&id, "pop"), gpui_kit::Animation::new(DUR_PANEL), |card, t| {
            let t = motion::ease_out().sample(t);
            card.opacity(t).scale(0.95 + 0.05 * t)
        })
}

// How far a scrolling dialog's edge fades into the card, and how far its fields reach past the body's
// inset so their focus rings aren't cut.
const FADE: f32 = 32.;
const RING_ROOM: f32 = 4.;

/// The part of a dialog between its fixed header and footer that scrolls when the window is too short,
/// fading out at whichever edge has more beyond it. Children stack 16px apart, as in `dialog_body`.
pub fn dialog_scroll(id: impl Into<ElementId>, children: impl IntoIterator<Item = AnyElement>, window: &mut Window, cx: &mut App) -> Div {
    let id = id.into();
    let handle = window.use_keyed_state(part(&id, "scroll"), cx, |_, _| ScrollHandle::new()).read(cx).clone();
    let card = palette::surface(palette::Level::Raised, cx);
    let edges = handle.clone();
    let fades = canvas(|_, _, _| {}, move |bounds, _, window, _| {
        let (offset, max) = (-edges.offset().y, edges.max_offset().y);
        let fade = px(FADE).min(bounds.size.height / 2.);
        if offset > px(0.5) {
            window.paint_quad(fill(Bounds::new(bounds.origin, size(bounds.size.width, fade)), linear_gradient(180., linear_color_stop(card, 0.), linear_color_stop(card.opacity(0.), 1.))));
        }
        if offset < max - px(0.5) {
            let top = bounds.bottom() - fade;
            window.paint_quad(fill(Bounds::new(gpui_kit::point(bounds.left(), top), size(bounds.size.width, fade)), linear_gradient(180., linear_color_stop(card.opacity(0.), 0.), linear_color_stop(card, 1.))));
        }
    })
    .absolute()
    .inset_0();
    let list = div()
        .id(id)
        .flex_1()
        .min_h_0()
        .mx(px(-RING_ROOM))
        .px(px(RING_ROOM))
        .overflow_y_scroll()
        .track_scroll(&handle)
        .child(div().w_full().py(px(RING_ROOM)).flex().flex_col().gap(px(16.)).children(children));
    div().relative().flex_1().min_h_0().flex().flex_col().my(px(-RING_ROOM)).child(list).child(fades)
}

/// A dialog's usual body: 24px in, sections 16px apart.
pub fn dialog_body() -> Div {
    div().p(px(24.)).flex().flex_col().gap(px(16.))
}

/// Dims the window behind `dialog` and centres it. `backdrop` is the caller's full-window catcher,
/// which decides what a click outside does.
pub fn modal(backdrop: Stateful<Div>, dialog: impl IntoElement, cx: &App) -> AnyElement {
    let scrim = palette::scrim(cx);
    backdrop
        .flex()
        .items_center()
        .justify_center()
        .child(dialog)
        .with_animation("scrim-in", gpui_kit::Animation::new(DUR_POP), move |backdrop, t| backdrop.bg(scrim.opacity(scrim.a * t)))
        .into_any_element()
}
