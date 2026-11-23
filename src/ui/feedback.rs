// Tracks and waits: the slider, progress (determinate and travelling), the mic meter, the ring and the
// skeleton shimmer. Done and remaining never touch: a gap sits between them, and the ends facing the
// gap get small corners while the outer ends stay round.
use std::{f32::consts::TAU, time::Duration};

use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, AppContext, Bounds, BoxShadow, Div, ElementId, Empty, FocusHandle, Hsla, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, ParentElement,
    PathBuilder, Pixels, Render, Stateful, StatefulInteractiveElement, Styled, Window,
    base::{Easing, Transition, transition},
    canvas, div, fill, point, prelude::FluentBuilder, px, size,
};

use super::{DUR_PANEL, DUR_PRESS, SQUIRCLE, Squircle, control::part, stretch};
use crate::{motion, palette};

// Progress: the done part, a 4px gap, then what's left.
// Keywords: progress bar, loading, upload, meter, track
pub fn progress(fraction: f32, cx: &App) -> Div {
    let f = fraction.clamp(0., 1.);
    split_track(f, 6., 3., 2., cx).gap(px(4.))
}

// Progress whose done part grows to a new value over the panel timing rather than jumping.
pub fn progress_live(id: impl Into<ElementId>, fraction: f32, window: &mut Window, cx: &mut App) -> Div {
    let f = transition(id.into(), fraction.clamp(0., 1.), Transition::new(DUR_PANEL).easing(motion::ease_out()), window, cx);
    progress(f, cx)
}

// Work that can't be counted: a done-coloured stretch travels the track, gapped from the rest on both
// sides, on the design's 1.6s move curve.
pub fn progress_indeterminate(id: impl Into<ElementId>, cx: &App) -> AnyElement {
    let (accent, rest) = (palette::accent(), palette::line_strong(cx));
    let frame = move |t: f32| {
        let (from, to) = travel(t);
        let piece = |grow: f32, color: Hsla| div().h(px(6.)).flex_basis(px(0.)).flex_grow(grow).rounded(px(2.)).corner_shape(SQUIRCLE).bg(color);
        div()
            .w_full()
            .h(px(6.))
            .flex()
            .gap(px(4.))
            .when(from > 0.001, |d| d.child(piece(from, rest).rounded_l(px(3.))))
            .when(to - from > 0.001, |d| d.child(piece(to - from, accent)))
            .when(to < 0.999, |d| d.child(piece(1. - to, rest).rounded_r(px(3.))))
    };
    frame(0.).with_animation(id, Animation::new(Duration::from_millis(1600)).repeat(), move |_, t| frame(t)).into_any_element()
}

// The design's `travel` keyframes: start and end of the moving stretch at `t` through one cycle; the
// curve applies to each keyframe segment as CSS does.
fn travel(t: f32) -> (f32, f32) {
    let ease = motion::ease_move();
    if t < 0.6 {
        let e = ease.sample(t / 0.6);
        return (0.45 * e, e);
    }
    let e = ease.sample((t - 0.6) / 0.4);
    (0.45 + 0.55 * e, 1.)
}

// Done and remaining as two flex pieces, ends facing the gap at `inner` radius.
fn split_track(f: f32, height: f32, outer: f32, inner: f32, cx: &App) -> Div {
    let piece = |grow: f32, color: Hsla| div().h(px(height)).flex_basis(px(0.)).flex_grow(grow).corner_shape(SQUIRCLE).bg(color);
    div()
        .w_full()
        .flex()
        .items_center()
        .when(f > 0., |d| d.child(piece(f, palette::accent()).rounded_l(px(outer)).rounded_r(px(if f < 1. { inner } else { outer }))))
        .when(f < 1., |d| d.child(piece(1. - f, palette::line_strong(cx)).rounded_r(px(outer)).rounded_l(px(if f > 0. { inner } else { outer }))))
}

#[derive(Clone, Copy)]
struct Thumb {
    // 0 at rest, 1 hovered: the thumb grows 8% taller.
    hover: f32,
    // 0 at rest, 1 held: the thumb narrows to two thirds, and its press transform replaces the hover's.
    press: f32,
    ring: Option<Hsla>,
}

/// The slider's resting look, for screens that run their own drag.
pub fn slider_track(value: f32, cx: &App) -> Div {
    thumb_track(value.clamp(0., 1.), Thumb { hover: 0., press: 0., ring: None }, cx)
}

fn thumb_track(f: f32, thumb: Thumb, cx: &App) -> Div {
    let piece = |grow: f32, color: Hsla| div().h(px(8.)).flex_basis(px(0.)).flex_grow(grow).corner_shape(SQUIRCLE).bg(color);
    let ring = |color: Hsla, spread: f32| BoxShadow { color, offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(spread), inset: false };
    let bar = stretch(div().w(px(6.)).h(px(24.)), 1. - 0.34 * thumb.press, 1. + 0.08 * thumb.hover * (1. - thumb.press))
        .squircle(3.)
        .bg(palette::accent())
        .when_some(thumb.ring, |b, accent_ink| b.shadow(vec![ring(accent_ink, 4.), ring(palette::surface(palette::Level::Page, cx), 2.)]));
    div()
        .w_full()
        .h(px(24.))
        .flex()
        .items_center()
        .gap(px(4.))
        .when(f > 0., |d| d.child(piece(f, palette::accent()).rounded_l(px(4.)).rounded_r(px(2.5))))
        .child(div().w(px(6.)).h(px(24.)).flex_none().flex().items_center().justify_center().child(bar))
        .when(f < 1., |d| d.child(piece(1. - f, palette::line_strong(cx)).rounded_r(px(4.)).rounded_l(px(2.5))))
}

// A draggable slider for a 0..1 value: press anywhere on the track to jump there, drag to follow,
// arrows step 1% (shift 10%) once it has focus. `on_change` gets the new value; the caller stores it.
pub fn slider_live(id: impl Into<ElementId>, value: f32, on_change: impl Fn(f32, &mut Window, &mut App) + 'static, window: &mut Window, cx: &mut App) -> Stateful<Div> {
    let id: ElementId = id.into();
    let state = window.use_keyed_state(part(&id, "slider"), cx, |_, cx| SliderState { focus: cx.focus_handle(), hovered: false, held: false, keyed: false, bounds: None });
    let (focus, hovered, held, keyed) = {
        let s = state.read(cx);
        (s.focus.clone(), s.hovered, s.held, s.keyed)
    };
    let press_curve = || Transition::new(DUR_PRESS).easing(motion::ease_out());
    let hover = transition(part(&id, "thumb-hover"), if hovered { 1. } else { 0. }, press_curve(), window, cx);
    let press = transition(part(&id, "thumb-press"), if held { 1. } else { 0. }, press_curve(), window, cx);
    let ring = (keyed && focus.is_focused(window)).then(|| palette::accent_ink(cx));
    let value = value.clamp(0., 1.);
    let on_change = std::rc::Rc::new(on_change);
    let drag_id = id.clone();
    div()
        .id(id.clone())
        .w_full()
        .cursor_pointer()
        .track_focus(&focus)
        .child(thumb_track(value, Thumb { hover, press, ring }, cx))
        .on_hover(window.listener_for(&state, |s, hovered: &bool, _, cx| {
            s.hovered = *hovered;
            cx.notify();
        }))
        .on_mouse_down(MouseButton::Left, {
            let (state, on_change) = (state.clone(), on_change.clone());
            move |e: &MouseDownEvent, window, cx| {
                window.focus(&state.read(cx).focus.clone(), cx);
                state.update(cx, |s, cx| {
                    s.held = true;
                    s.keyed = false;
                    cx.notify();
                });
                if let Some(bounds) = state.read(cx).bounds {
                    on_change(at(e.position.x, bounds), window, cx);
                }
            }
        })
        .on_mouse_up(MouseButton::Left, window.listener_for(&state, |s, _, _, cx| s.release(cx)))
        .on_mouse_up_out(MouseButton::Left, window.listener_for(&state, |s, _, _, cx| s.release(cx)))
        .on_drag(SliderDrag(drag_id.clone()), |_, _, _, cx| cx.new(|_| Nothing))
        .on_drag_move({
            let on_change = on_change.clone();
            move |e: &gpui_kit::DragMoveEvent<SliderDrag>, window, cx| {
                if e.drag(cx).0 == drag_id {
                    on_change(at(e.event.position.x, e.bounds), window, cx);
                }
            }
        })
        .on_key_down({
            let state = state.clone();
            move |e: &KeyDownEvent, window, cx| {
                let step = if e.keystroke.modifiers.shift { 0.1 } else { 0.01 };
                let delta = match e.keystroke.key.as_str() {
                    "left" | "down" => -step,
                    "right" | "up" => step,
                    "home" => -1.,
                    "end" => 1.,
                    _ => return,
                };
                cx.stop_propagation();
                state.update(cx, |s, cx| {
                    s.keyed = true;
                    cx.notify();
                });
                on_change((value + delta).clamp(0., 1.), window, cx);
            }
        })
        .child(canvas(
            {
                let state = state.clone();
                move |bounds, _, cx| state.update(cx, |s, _| s.bounds = Some(bounds))
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full())
}

// The value under the pointer: the thumb's centre follows it, so the ends sit half a thumb in.
fn at(x: Pixels, bounds: Bounds<Pixels>) -> f32 {
    let span = f32::from(bounds.size.width) - 6.;
    ((f32::from(x - bounds.left()) - 3.) / span.max(1.)).clamp(0., 1.)
}

struct SliderState {
    focus: FocusHandle,
    hovered: bool,
    held: bool,
    // Focus came from the keyboard, so the thumb shows its focus ring.
    keyed: bool,
    bounds: Option<Bounds<Pixels>>,
}

impl SliderState {
    fn release(&mut self, cx: &mut gpui_kit::Context<Self>) {
        if self.held {
            self.held = false;
            cx.notify();
        }
    }
}

#[derive(Clone)]
pub struct SliderDrag(ElementId);

// Dragging a slider shows no preview; the thumb is the feedback.
struct Nothing;

impl Render for Nothing {
    fn render(&mut self, _: &mut Window, _: &mut gpui_kit::Context<Self>) -> impl IntoElement {
        Empty
    }
}

// The mic meter: `bars` equal bars that light up to `level` (0..1), turning amber then red near the top.
pub fn meter(level: f32, bars: usize, cx: &App) -> Div {
    let lit = [crate::discord::Status::Online, crate::discord::Status::Idle, crate::discord::Status::Dnd].map(|s| palette::status_in(s, cx));
    meter_with(level, bars, lit, palette::line_strong(cx))
}

// The meter in a scope with its own colours (the call stage): `lit` is on, hot and clipping.
pub fn meter_with(level: f32, bars: usize, [online, idle, dnd]: [Hsla; 3], rest: Hsla) -> Div {
    div().h(px(20.)).flex().items_center().gap(px(3.)).children((0..bars).map(move |i| {
        let at = i as f32 / bars as f32;
        let color = match at < level {
            false => rest,
            true if i + 2 >= bars => dnd,
            true if i * 4 >= bars * 3 => idle,
            true => online,
        };
        div().flex_1().min_w(px(3.)).h_full().rounded_full().bg(color)
    }))
}

// A ring of two round-capped arcs, done and left, with a gap at each meeting point: 40px, 4px stroke.
pub fn ring(fraction: f32, cx: &App) -> impl IntoElement {
    let (accent, rest) = (palette::accent(), palette::line_strong(cx));
    let v = fraction.clamp(0., 1.);
    canvas(|_, _, _| {}, move |bounds, _, window, _| {
        // In percent of the circumference, as the design's pathLength 100: each gap is 6.5.
        let gap = 6.5;
        let centre = bounds.center();
        let mut arc = |from: f32, len: f32, color: Hsla| {
            if len <= 0. {
                return;
            }
            paint_arc(window, centre, 18., from, len, color);
        };
        arc(gap / 2., v * 100. - gap, accent);
        arc(v * 100. + gap / 2., (1. - v) * 100. - gap, rest);
    })
    .size(px(40.))
    .flex_none()
}

// One 4px arc from `from` percent round the circle (0 at the top, clockwise) for `len` percent, round-capped.
// Filled as one outline, caps included, so a translucent colour never doubles where a cap meets the stroke.
fn paint_arc(window: &mut Window, centre: gpui_kit::Point<Pixels>, radius: f32, from: f32, len: f32, color: Hsla) {
    const HALF: f32 = 2.;
    let angle = |pct: f32| pct / 100. * TAU - TAU / 4.;
    let at = |a: f32, r: f32| centre + point(px(r * a.cos()), px(r * a.sin()));
    let (start, end) = (angle(from), angle(from + len));
    // Short segments keep the curve smooth at 40px.
    let steps = (len.ceil() as usize * 2).max(2);
    let edge = |r: f32, forward: bool| {
        (0..=steps).map(move |i| {
            let t = i as f32 / steps as f32;
            at(start + (end - start) * if forward { t } else { 1. - t }, r)
        })
    };
    // Half a turn about the cap's centre, outer edge to inner, bulging forward (`dir` 1) or back (-1).
    let cap = |a: f32, dir: f32| {
        (1..12).map(move |i| {
            let th = i as f32 / 12. * TAU / 2.;
            let (cos, sin) = (th.cos(), dir * th.sin());
            at(a, radius) + point(px(HALF * (cos * a.cos() - sin * a.sin())), px(HALF * (cos * a.sin() + sin * a.cos())))
        })
    };
    let start_cap: Vec<_> = cap(start, -1.).collect();
    let mut outline = edge(radius + HALF, true).chain(cap(end, 1.)).chain(edge(radius - HALF, false)).chain(start_cap.into_iter().rev());
    let mut path = PathBuilder::fill();
    let Some(first) = outline.next() else { return };
    path.move_to(first);
    for p in outline {
        path.line_to(p);
    }
    path.close();
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

// A loading placeholder: the selected fill with a calm shimmer that sweeps across, then rests. Callers
// size it; `radius` is its squircle corner. The sweep is painted in thin columns inset along a circular
// arc of the same radius, which sits inside the squircle, since gpui clips children to rectangles only.
pub fn skeleton(id: impl Into<ElementId>, radius: f32, cx: &App) -> Div {
    sweeping(id.into(), radius, cx).corner_shape(SQUIRCLE)
}

// A round placeholder for an avatar of `size`.
pub fn skeleton_circle(id: impl Into<ElementId>, size: f32, cx: &App) -> Div {
    sweeping(id.into(), size / 2., cx).size(px(size)).flex_none()
}

fn sweeping(id: ElementId, radius: f32, cx: &App) -> Div {
    let shimmer = palette::shimmer(cx);
    div().relative().rounded(px(radius)).bg(palette::selected(cx)).child(
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .with_animation(id, Animation::new(Duration::from_millis(2300)).repeat(), move |layer, t| {
                layer.child(canvas(|_, _, _| {}, move |bounds, _, window, _| paint_sweep(window, bounds, radius, shimmer, sweep(t))).size_full())
            }),
    )
}

// Where the band's left edge sits, as a fraction of the width: -1 to 1 over the first 48%, then still.
fn sweep(t: f32) -> f32 {
    thread_local! {
        static EASE: Easing = Easing::cubic_bezier(0.42, 0., 0.58, 1.).expect("ease-in-out");
    }
    -1. + 2. * EASE.with(|e| e.sample((t / 0.48).min(1.)))
}

fn paint_sweep(window: &mut Window, bounds: Bounds<Pixels>, radius: f32, shimmer: Hsla, offset: f32) {
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let r = radius.min(w / 2.).min(h / 2.);
    const COLUMN: f32 = 2.;
    let mut x = 0.;
    while x < w {
        let u = (x + COLUMN / 2.) / w - offset;
        if (0. ..1.).contains(&u) {
            let strength = 1. - (2. * u - 1.).abs();
            let edge = (x + COLUMN / 2.).min(w - x - COLUMN / 2.);
            let inset = if edge < r { r - (r * r - (r - edge).powi(2)).max(0.).sqrt() } else { 0. };
            let column = Bounds::new(bounds.origin + point(px(x), px(inset)), size(px(COLUMN.min(w - x)), px(h - 2. * inset)));
            window.paint_quad(fill(column, shimmer.opacity(strength)));
        }
        x += COLUMN;
    }
}

// A marker squiggle under where a jump landed. It waits for the view to settle, draws itself left to right
// like a hand with a marker, holds, then fades. `seed`
// varies the hand so no two look alike; callers size and place it (8px tall reads well).
pub fn squiggle(id: impl Into<ElementId>, seed: u64, color: Hsla, duration: Duration) -> impl IntoElement {
    const WAIT: f32 = 0.35;
    const DRAW: f32 = 0.55;
    const FADE: f32 = 0.5;
    let total = duration.as_secs_f32();
    div().size_full().with_animation(id.into(), Animation::new(duration), move |d, t| {
        let now = t * total;
        let drawn = ((now - WAIT) / DRAW).clamp(0., 1.);
        // Ease in and out, as a stroke speeds up and settles.
        let drawn = drawn * drawn * (3. - 2. * drawn);
        let alpha = ((total - now) / FADE).clamp(0., 1.);
        d.child(canvas(|_, _, _| (), move |bounds: Bounds<Pixels>, _, window, _| paint_squiggle(window, bounds, seed, drawn, color.opacity(color.a * alpha))).size_full())
    })
}

// The marker line up to `drawn` of its length, as a hand makes it: every half-wave its own width and height,
// a wandering baseline, a faint tremor, pressure in the width, and a flick at the pen-up. Round ends.
fn paint_squiggle(window: &mut Window, bounds: Bounds<Pixels>, seed: u64, drawn: f32, color: Hsla) {
    const MAX_LENGTH: f32 = 360.;
    if drawn <= 0. {
        return;
    }
    let length = f32::from(bounds.size.width).min(MAX_LENGTH);
    let mid = f32::from(bounds.size.height) / 2.;
    // A small seeded generator, so the same message gets the same scribble every frame.
    let mut state = seed ^ 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / (1u64 << 24) as f32
    };
    // Half-waves: where each ends and how high it swings (alternating up and down).
    let mut waves: Vec<(f32, f32)> = Vec::new();
    let mut x = 0.;
    let mut up = true;
    while x < length {
        x += 5. + next() * 5.;
        let height = (0.6 + next() * 2.2) * if up { -1. } else { 1. };
        waves.push((x.min(length), height));
        up = !up;
    }
    let (p1, p2, rise) = (next() * TAU, next() * TAU, -0.8 - next() * 1.2);
    let centre = |x: f32| {
        let i = waves.partition_point(|(end, _)| *end < x).min(waves.len() - 1);
        let start = if i == 0 { 0. } else { waves[i - 1].0 };
        let (end, height) = waves[i];
        let local = ((x - start) / (end - start).max(0.001)).clamp(0., 1.);
        let u = x / length;
        let swing = height * (local * std::f32::consts::PI).sin();
        let baseline = mid + rise * u + 0.9 * (u * 2.1 + p1).sin();
        let tremor = 0.25 * (x * 1.7 + p2).sin() * (x * 0.53).cos();
        // The pen-up flicks the last few pixels upward.
        let flick = -2.5 * ((u - 0.94) / 0.06).clamp(0., 1.).powi(2);
        point(px(x), px(baseline + swing + tremor + flick))
    };
    let half = |x: f32| (2.0 + 0.8 * (0.5 + 0.5 * (x / length * 5.3 + p1).sin())) / 2.;
    let steps = ((length * drawn) / 1.2).ceil().max(2.) as usize;
    let xs: Vec<f32> = (0..=steps).map(|i| length * drawn * i as f32 / steps as f32).collect();
    let normal = |x: f32| {
        let (a, b) = (centre(x - 0.4), centre(x + 0.4));
        let (dx, dy) = (f32::from(b.x - a.x), f32::from(b.y - a.y));
        let len = (dx * dx + dy * dy).sqrt().max(0.001);
        (-dy / len, dx / len)
    };
    let edge = |x: f32, side: f32| {
        let (c, (nx, ny), h) = (centre(x), normal(x), half(x) * side);
        bounds.origin + c + point(px(nx * h), px(ny * h))
    };
    let mut path = PathBuilder::fill();
    path.move_to(edge(xs[0], 1.));
    for &x in &xs[1..] {
        path.line_to(edge(x, 1.));
    }
    for &x in xs.iter().rev() {
        path.line_to(edge(x, -1.));
    }
    path.close();
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
    for x in [xs[0], xs[xs.len() - 1]] {
        let (c, r) = (bounds.origin + centre(x), half(x));
        window.paint_quad(fill(Bounds::new(c - point(px(r), px(r)), size(px(r * 2.), px(r * 2.))), color).corner_radii(px(r)));
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Bounds, point, px, size};

    #[test]
    fn slider_value_follows_the_thumb_centre_and_clamps() {
        let track = Bounds::new(point(px(100.), px(0.)), size(px(206.), px(24.)));
        assert_eq!(super::at(px(103.), track), 0.);
        assert_eq!(super::at(px(203.), track), 0.5);
        assert_eq!(super::at(px(303.), track), 1.);
        assert_eq!(super::at(px(0.), track), 0.);
        assert_eq!(super::at(px(900.), track), 1.);
    }

    #[test]
    fn travel_hits_the_design_keyframes() {
        let close = |(a, b): (f32, f32), (x, y): (f32, f32)| (a - x).abs() < 1e-3 && (b - y).abs() < 1e-3;
        assert!(close(super::travel(0.), (0., 0.)));
        assert!(close(super::travel(0.6), (0.45, 1.)));
        assert!(close(super::travel(0.9999), (1., 1.)));
    }

    #[test]
    fn sweep_crosses_then_rests() {
        assert!((super::sweep(0.) + 1.).abs() < 1e-4);
        assert!((super::sweep(0.48) - 1.).abs() < 1e-4);
        assert!((super::sweep(0.9) - 1.).abs() < 1e-4);
    }
}
