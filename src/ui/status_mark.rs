// Presence marks drawn as Discord draws them. Every mark is the same few parts (an outline, a hole, the play
// triangle, the phone's home dot) set differently, so one status morphs into the next the way Discord's do,
// and the holes are real: whatever is under the mark shows through them. Sizes come from Discord's avatar
// table.
use gpui_kit::{
    Animation, AnimationExt, App, Bounds, Div, ElementId, Hsla, ParentElement, PathBuilder, Pixels, Point, Styled, Window,
    base::{Interpolate, Transition, transition},
    canvas, div, point, prelude::FluentBuilder as _, px,
};

use super::{DUR_PANEL, face::cut_out};
use crate::{
    discord::{Mark, Status},
    motion, palette,
};

// A presence mark, bare: a dot for online, a moon for idle, a bar for do not disturb, a ring for offline, a
// phone for online on mobile only, a play disc while streaming, a pill of three dots while typing.
// `dot` is the mark's size (its width, or its height for the phone and the pill); on a face it goes
// through `Badged::status`, which sizes it from Discord's table, rings it and morphs it.
// Keywords: presence, online, idle, dnd, offline, mobile, streaming, typing, dot
pub fn status_mark(status: impl Into<Mark>, dot: f32) -> Div {
    let shape = Shape::of(status.into(), dot, 0.);
    shape_el(shape, dot)
}

// The mark ringed on a round face: placed by `metrics` from the face's bottom-right corner, its ring cut out
// of the face in `surface`. With `morph`, a change of status animates from the last one, keyed by it.
pub(super) fn ringed_status(mark: Mark, metrics: StatusMetrics, surface: Hsla, morph: Option<ElementId>, window: &mut Window, cx: &mut App) -> Div {
    let StatusMetrics { dot, ring, inset } = metrics;
    let target = Shape::of(mark, dot, ring);
    let shape = match morph {
        Some(id) => transition((id, "presence-morph"), target, Transition::new(DUR_PANEL).easing(motion::ease_out()), window, cx),
        None => target,
    };
    let Rounded { x, y, w, h, .. } = shape.outline;
    let holder = div().absolute().right(px(inset - x - w - ring)).bottom(px(inset - y - h - ring)).p(px(ring)).rounded(px(shape.ring_radius));
    cut_out(holder, surface).child(shape_el(shape, dot))
}

// Discord's avatar sizes, each with its mark's size, the ring cut around the mark, and how far the mark sits
// in from the face's bottom-right corner.
const METRICS: [(f32, StatusMetrics); 13] = [
    (16., StatusMetrics { dot: 6., ring: 2., inset: 0. }),
    (20., StatusMetrics { dot: 6., ring: 2., inset: 0. }),
    (24., StatusMetrics { dot: 8., ring: 3., inset: 0. }),
    (32., StatusMetrics { dot: 10., ring: 3., inset: 0. }),
    (40., StatusMetrics { dot: 12., ring: 4., inset: 0. }),
    (44., StatusMetrics { dot: 12., ring: 4., inset: 0. }),
    (48., StatusMetrics { dot: 12., ring: 4., inset: 0. }),
    (56., StatusMetrics { dot: 14., ring: 4., inset: 2. }),
    (72., StatusMetrics { dot: 16., ring: 6., inset: 4. }),
    (80., StatusMetrics { dot: 16., ring: 6., inset: 4. }),
    (96., StatusMetrics { dot: 20., ring: 8., inset: 6. }),
    (120., StatusMetrics { dot: 24., ring: 8., inset: 8. }),
    (152., StatusMetrics { dot: 30., ring: 10., inset: 10. }),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct StatusMetrics {
    dot: f32,
    ring: f32,
    inset: f32,
}

// The metrics of the largest Discord size that fits a round face `size` across (the smallest below 16).
pub(super) fn status_metrics(size: f32) -> StatusMetrics {
    METRICS.iter().rev().find(|(face, _)| *face <= size).unwrap_or(&METRICS[0]).1
}

// A rounded rectangle in px.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rounded {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
}

impl Rounded {
    fn scaled((x, y, w, h, r): (f32, f32, f32, f32, f32), t: f32) -> Self {
        Rounded { x: x * t, y: y * t, w: w * t, h: h * t, r: r * t }
    }

    fn lerp(&self, to: &Self, p: f32) -> Self {
        let l = |a: f32, b: f32| a + (b - a) * p;
        Rounded { x: l(self.x, to.x), y: l(self.y, to.y), w: l(self.w, to.w), h: l(self.h, to.h), r: l(self.r, to.r) }
    }
}

// One state of a mark, in px from the bottom-right corner of the round mark's box: Discord's animated status
// (module 935154's `v()`) read as the avatar's mask places it at rest.
#[derive(Clone, Debug, PartialEq)]
struct Shape {
    // The mark's size: its round outline's width.
    size: f32,
    outline: Rounded,
    ring_radius: f32,
    hole: Rounded,
    // The play triangle's scale, 0 to 1.
    play: f32,
    // The phone's home dot, its radius.
    home: f32,
    // The typing pill's dots, 0 to 1.
    typing: f32,
    color: Hsla,
}

impl Shape {
    fn of(mark: Mark, t: f32, ring: f32) -> Self {
        let round = (-1., -1., 1., 1., 0.5);
        // A hole drawn as a point at the mark's middle: nothing cut.
        let none = (-0.5, -0.5, 0., 0., 0.);
        let (outline, hole) = match mark {
            Mark::Status(Status::Online) => (round, none),
            Mark::Status(Status::Idle) => (round, (-1.125, -1.125, 0.75, 0.75, 0.375)),
            Mark::Status(Status::Dnd) => (round, (-0.875, -0.625, 0.75, 0.25, 0.125)),
            Mark::Status(Status::Offline) => (round, (-0.75, -0.75, 0.5, 0.5, 0.25)),
            Mark::Streaming => (round, (-0.5, -0.5, 0., 0., 0.25)),
            Mark::Mobile => ((-1., -1.5, 1., 1.5, 0.1875), (-0.875, -1.25, 0.75, 0.75, 0.)),
            Mark::Typing => ((-1.75, -1., 2.5, 1., 0.5), none),
        };
        // The ring follows the outline's corners; Discord cuts the phone's with corners a fifth of its height.
        let ring_radius = match mark {
            Mark::Mobile => (1.5 * t + 2. * ring) * 0.2,
            _ => t / 2. + ring,
        };
        Shape {
            size: t,
            outline: Rounded::scaled(outline, t),
            ring_radius,
            hole: Rounded::scaled(hole, t),
            play: if mark == Mark::Streaming { 1. } else { 0. },
            home: if mark == Mark::Mobile { 0.125 * t } else { 0. },
            typing: if mark == Mark::Typing { 1. } else { 0. },
            color: palette::presence(mark),
        }
    }
}

impl Interpolate for Shape {
    fn interpolate(&self, to: &Self, p: f32) -> Self {
        let l = |a: f32, b: f32| a + (b - a) * p;
        Shape {
            size: l(self.size, to.size),
            outline: self.outline.lerp(&to.outline, p),
            ring_radius: l(self.ring_radius, to.ring_radius),
            hole: self.hole.lerp(&to.hole, p),
            play: l(self.play, to.play),
            home: l(self.home, to.home),
            typing: l(self.typing, to.typing),
            color: mix_rgb(self.color, to.color, p),
        }
    }
}

// Colours blended in RGB: blending hue would swing idle's yellow through green and blue on its way to red.
fn mix_rgb(from: Hsla, to: Hsla, p: f32) -> Hsla {
    if from == to || p >= 1. {
        return to;
    }
    let (a, b) = (from.to_rgb(), to.to_rgb());
    let l = |x: f32, y: f32| x + (y - x) * p;
    gpui_kit::Rgba { r: l(a.r, b.r), g: l(a.g, b.g), b: l(a.b, b.b), a: l(a.a, b.a) }.into()
}

// The mark's element: the outline's box, the shape painted in it, the typing dots over it.
fn shape_el(shape: Shape, t: f32) -> Div {
    let Rounded { w, h, .. } = shape.outline;
    let typing = shape.typing;
    let painted = shape.clone();
    div()
        .relative()
        .flex_none()
        .w(px(w))
        .h(px(h))
        .child(canvas(|_, _, _| {}, move |bounds, _, window, _| paint(window, bounds, &painted)).absolute().size_full())
        .when(typing > 0.01, |d| d.child(typing_dots(t).opacity(typing)))
}

// Points per quarter circle when a rounded corner is flattened.
const CORNER_STEPS: usize = 8;

fn paint(window: &mut Window, bounds: Bounds<Pixels>, shape: &Shape) {
    let origin = (shape.outline.x, shape.outline.y);
    let outline = polygon(shape.outline);
    // lyon fills even-odd: every hole drawn inside the outline is cut out of it. The hole is clipped to the
    // outline first, as Discord's mask clips it: the idle moon's bite reaches past the disc.
    let hole = clip(&polygon(shape.hole), &outline);
    let t = shape.size;
    let play_centre = (-0.4375 * t, -0.5 * t);
    let triangle: Vec<(f32, f32)> = [(-0.216_506, -0.25), (0.216_506, 0.), (-0.216_506, 0.25)].iter().map(|(x, y)| (play_centre.0 + x * t * shape.play, play_centre.1 + y * t * shape.play)).collect();
    let home = polygon(Rounded { x: -0.5 * t - shape.home, y: -0.25 * t - shape.home, w: 2. * shape.home, h: 2. * shape.home, r: shape.home });
    let at = |(x, y): (f32, f32)| bounds.origin + point(px(x - origin.0), px(y - origin.1));
    let mut path = PathBuilder::fill();
    let mut add = |points: &[(f32, f32)]| {
        if area(points) > 0.01 {
            path.add_polygon(&points.iter().map(|p| at(*p)).collect::<Vec<Point<Pixels>>>(), true);
        }
    };
    add(&outline);
    add(&hole);
    add(&triangle);
    add(&home);
    if let Ok(path) = path.build() {
        window.paint_path(path, shape.color);
    }
}

// A rounded rectangle as a convex polygon, clockwise from its top edge.
fn polygon(b: Rounded) -> Vec<(f32, f32)> {
    let r = b.r.min(b.w / 2.).min(b.h / 2.).max(0.);
    let arcs = [(b.x + b.w - r, b.y + r, -90.), (b.x + b.w - r, b.y + b.h - r, 0.), (b.x + r, b.y + b.h - r, 90.), (b.x + r, b.y + r, 180.)];
    let corners = arcs
        .iter()
        .flat_map(|&(cx, cy, from)| {
            (0..=CORNER_STEPS).map(move |i| {
                let a = (from + 90. * i as f32 / CORNER_STEPS as f32).to_radians();
                (cx + r * a.cos(), cy + r * a.sin())
            })
        })
        .collect::<Vec<_>>();
    // Where a side has no straight run, one corner ends where the next starts (the last where the first
    // began): keep one point, as a zero-length edge would give the clip a direction picked by rounding error.
    let apart = |p: (f32, f32), q: (f32, f32)| (p.0 - q.0).hypot(p.1 - q.1) > 1e-3;
    let mut points: Vec<(f32, f32)> = Vec::with_capacity(corners.len());
    for p in corners {
        if points.last().is_none_or(|q| apart(p, *q)) {
            points.push(p);
        }
    }
    while points.len() > 1 && !apart(points[0], points[points.len() - 1]) {
        points.pop();
    }
    points
}

// Sutherland–Hodgman: `subject` cut down to what lies inside the convex, clockwise `clip`.
fn clip(subject: &[(f32, f32)], clip: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let inside = |p: (f32, f32), a: (f32, f32), b: (f32, f32)| (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0) >= 0.;
    let cross = |p: (f32, f32), q: (f32, f32), a: (f32, f32), b: (f32, f32)| {
        let (r, s) = ((q.0 - p.0, q.1 - p.1), (b.0 - a.0, b.1 - a.1));
        let denom = r.0 * s.1 - r.1 * s.0;
        if denom.abs() < f32::EPSILON {
            return p;
        }
        let u = ((a.0 - p.0) * s.1 - (a.1 - p.1) * s.0) / denom;
        (p.0 + u * r.0, p.1 + u * r.1)
    };
    let mut out = subject.to_vec();
    for i in 0..clip.len() {
        let (a, b) = (clip[i], clip[(i + 1) % clip.len()]);
        let input = std::mem::take(&mut out);
        for j in 0..input.len() {
            let (p, q) = (input[j], input[(j + 1) % input.len()]);
            match (inside(p, a, b), inside(q, a, b)) {
                (true, true) => out.push(q),
                (true, false) => out.push(cross(p, q, a, b)),
                (false, true) => out.extend([cross(p, q, a, b), q]),
                (false, false) => {}
            }
        }
    }
    out
}

// The polygon's area (shoelace), unsigned.
fn area(points: &[(f32, f32)]) -> f32 {
    (0..points.len()).map(|i| {
        let (p, q) = (points[i], points[(i + 1) % points.len()]);
        p.0 * q.1 - q.0 * p.1
    }).sum::<f32>().abs() / 2.
}

// Discord's typing dots: three white dots a quarter of the mark across, centred in the pill, each pulsing
// from 30% to full a quarter-cycle after the one before.
fn typing_dots(dot: f32) -> Div {
    let size = dot / 2.;
    div().absolute().inset_0().flex().items_center().justify_center().gap(px(dot * 0.125)).children((0..3_usize).map(move |i| {
        let start = i as f32 * 0.25;
        // lint-allow(endless-animation): the typing mark shows only while someone is typing, which Discord ends after 10 seconds.
        div().size(px(size)).rounded_full().bg(gpui_kit::white()).with_animation(("typing-dot", i), Animation::new(TYPING_CYCLE).repeat(), move |d, t| d.opacity(pulse(t - start)))
    }))
}

const TYPING_CYCLE: std::time::Duration = std::time::Duration::from_millis(1400);

// A dot's opacity `t` through the cycle: dim for the first 40%, rising to full by 80%, then held.
fn pulse(t: f32) -> f32 {
    let t = t.rem_euclid(1.);
    match t {
        t if t < 0.4 => 0.3,
        t if t < 0.8 => 0.3 + 0.7 * (t - 0.4) / 0.4,
        _ => 1.,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_follow_discords_table() {
        assert_eq!(status_metrics(32.), StatusMetrics { dot: 10., ring: 3., inset: 0. });
        assert_eq!(status_metrics(80.), StatusMetrics { dot: 16., ring: 6., inset: 4. });
        // Between sizes: the largest that fits. Below the table: its smallest.
        assert_eq!(status_metrics(36.), status_metrics(32.));
        assert_eq!(status_metrics(12.), status_metrics(16.));
        assert_eq!(status_metrics(400.), status_metrics(152.));
    }

    #[test]
    fn idle_bite_is_clipped_to_the_disc() {
        let shape = Shape::of(Mark::Status(Status::Idle), 10., 3.);
        let disc = polygon(shape.outline);
        let bite = clip(&polygon(shape.hole), &disc);
        let inside = |p: &(f32, f32)| (p.0 + 5.).hypot(p.1 + 5.) <= 5.01;
        assert!(!bite.is_empty() && bite.iter().all(inside));
        // Less than the whole bite survives: part of it lay past the disc.
        assert!(area(&bite) < area(&polygon(shape.hole)) - 1.);
    }

    #[test]
    fn a_disc_has_no_repeated_points() {
        let disc = polygon(Rounded { x: 0., y: 0., w: 24., h: 24., r: 12. });
        let closed: Vec<_> = disc.iter().chain(disc.first()).collect();
        assert!(closed.windows(2).all(|w| (w[0].0 - w[1].0).hypot(w[0].1 - w[1].1) > 1e-3));
    }

    #[test]
    fn holes_at_rest_lie_inside_their_outline() {
        for (mark, t) in [Mark::Status(Status::Dnd), Mark::Status(Status::Offline), Mark::Mobile].into_iter().flat_map(|m| [10., 16., 24., 30.].map(|t| (m, t))) {
            let shape = Shape::of(mark, t, 3.);
            let hole = polygon(shape.hole);
            let clipped = clip(&hole, &polygon(shape.outline));
            assert!((area(&hole) - area(&clipped)).abs() < 0.01, "{mark:?} at {t}");
        }
    }

    #[test]
    fn morph_halfway_is_between_both_ends() {
        let (from, to) = (Shape::of(Mark::Status(Status::Online), 10., 3.), Shape::of(Mark::Typing, 10., 3.));
        let mid = from.interpolate(&to, 0.5);
        assert_eq!(mid.outline.w, 17.5);
        assert_eq!(mid.typing, 0.5);
        assert_eq!(from.interpolate(&to, 1.), to);
    }

    #[test]
    fn typing_pulse_wraps() {
        assert_eq!(pulse(0.1), 0.3);
        assert_eq!(pulse(0.9), 1.);
        assert_eq!(pulse(-0.15), pulse(0.85));
    }
}
