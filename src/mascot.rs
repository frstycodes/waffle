use std::{
    f32::consts::{PI, TAU},
    sync::LazyLock,
    time::Duration,
};

use gpui_kit::{Animation, AnimationExt, AnyElement, App, Bounds, ElementId, Hsla, PathBuilder, Pixels, Window, canvas, div, point, prelude::*, px};

use crate::palette;

// The mascot is the mark come alive (assets/brand/waffle-mark.svg, drawn in design/waffle.html): a tilted waffle
// with honey coating its bottom edge in three drips, moving with the mood.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    // Breathes gently.
    Idle,
    // Its pockets fill with honey in turn, like typing dots: waiting on something outside the app.
    Waiting,
    // Hops, the drips stretching behind it.
    Happy,
}

const SIZE: f32 = 64.;
// How far a happy waffle hops, in units.
const HOP: f32 = 6.;

// The mascot drawn at `scale` (1 is 64px wide), for tighter places such as the login card.
pub fn mascot_at(id: impl Into<ElementId>, mood: Mood, scale: f32, cx: &App) -> AnyElement {
    let (size, ink) = (SIZE * scale, palette::logo_ink(cx));
    if cx.reduce_motion() {
        return mark(size, ink, Pose::REST).into_any_element();
    }
    let period = match mood {
        Mood::Idle => 2.6,
        Mood::Waiting => 1.6,
        Mood::Happy => 1.2,
    };
    div()
        .size(px(size))
        .flex_none()
        // lint-allow(endless-animation): plays only while the mascot is drawn, and reduce-motion gets the still mark above.
        .with_animation(id, Animation::new(Duration::from_secs_f32(period)).repeat(), move |el, t| el.child(mark(size, ink, pose(mood, t))))
        .into_any_element()
}

// The loader: the mark as it ships, its drips forming and falling one after another, a drop about every second.
// `ink` suits the surface: `palette::logo_ink` on the theme's own, `palette::logo` on the always-dark stage.
pub fn loader(id: impl Into<ElementId>, size: f32, ink: Hsla, cx: &App) -> AnyElement {
    if cx.reduce_motion() {
        return mark(size, ink, Pose::REST).into_any_element();
    }
    div()
        .size(px(size))
        .flex_none()
        // lint-allow(endless-animation): plays only while a loader is drawn, and reduce-motion gets the still mark above.
        .with_animation(id, Animation::new(DRIP_CYCLE).repeat(), move |el, t| el.child(mark(size, ink, Pose { drips: loader_drips(t), ..Pose::REST })))
        .into_any_element()
}

// How the mark is posed in one frame.
#[derive(Clone, Copy, Debug)]
struct Pose {
    // The whole mark, about its centre.
    scale: f32,
    // Units the whole mark is lifted.
    lift: f32,
    // Honey swelling in each pocket (0..1), in POCKETS' order.
    pockets: [f32; 4],
    drips: [Drip; 3],
}

impl Pose {
    const REST: Pose = Pose { scale: 1., lift: 0., pockets: [0.; 4], drips: [Drip::hanging(DRIPS[0].len), Drip::hanging(DRIPS[1].len), Drip::hanging(DRIPS[2].len)] };
}

// One drip, in units below the lip: where its neck ends, where its bead's centre is, and the bead's size.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Drip {
    neck: f32,
    bead_y: f32,
    bead: f32,
}

impl Drip {
    const fn hanging(len: f32) -> Drip {
        Drip { neck: len, bead_y: len, bead: 1. }
    }
}

fn pose(mood: Mood, t: f32) -> Pose {
    match mood {
        Mood::Idle => Pose { scale: 1. - 0.03 * (0.5 - 0.5 * (t * TAU).cos()), ..Pose::REST },
        Mood::Waiting => Pose { pockets: [0., 1., 2., 3.].map(|i| ((t - i / 4.) * TAU).cos().max(0.)), ..Pose::REST },
        Mood::Happy => {
            // The hop takes the first part of the beat, then it rests. Honey lags: drips stretch on the way up and
            // squash on the way down.
            let u = (t / HOP_SHARE).min(1.);
            let stretch = 1. + 0.3 * (u * TAU).sin();
            Pose { lift: HOP * 4. * u * (1. - u), drips: DRIPS.map(|d| Drip::hanging(d.len * stretch)), ..Pose::REST }
        }
    }
}

const HOP_SHARE: f32 = 0.55;

// Each drip runs one cycle, a third of a cycle behind the one before: it grows from nothing to a full drip, then its
// bead lets go and falls away while the neck snaps back up.
const DRIP_CYCLE: Duration = Duration::from_secs(3);
const DRIP_GROW: f32 = 0.8;
const DRIP_FULL: f32 = 12.5;
// How far a bead falls before it's gone, in units; short, so it shrinks away inside the mark's box.
const DRIP_FALL: f32 = 7.;
// Phases at t = 0, so the first frame is close to the mark: the long drip about to fall, the others behind it.
const DRIP_PHASES: [f32; 3] = [DRIP_GROW, DRIP_GROW - 1. / 3., DRIP_GROW - 2. / 3.];

fn loader_drips(t: f32) -> [Drip; 3] {
    DRIP_PHASES.map(|phase| drip_at((t + phase).fract()))
}

// One drip `p` of the way through its cycle.
fn drip_at(p: f32) -> Drip {
    if p < DRIP_GROW {
        let u = p / DRIP_GROW;
        return Drip { neck: DRIP_FULL * u * u, bead_y: DRIP_FULL * u * u, bead: u.sqrt() };
    }
    let w = (p - DRIP_GROW) / (1. - DRIP_GROW);
    Drip { neck: DRIP_FULL * (1. - w).powi(2), bead_y: DRIP_FULL + DRIP_FALL * w * w, bead: 1. - w }
}

// GEOMETRY

// In the units of design/waffle.html, a 100-unit page. The waffle and its lip are drawn in their own frame, then
// tilted about the page's centre; the drips hang plumb in page space.
const TILT: f32 = -10.;
// The square crop of the page that holds the mark, as waffle-mark.svg's viewBox: x, y, side.
const VIEW: (f32, f32, f32) = (6.96, 12.343, 88.);
const SQUARE: (f32, f32, f32, f32) = (22., 24., 78., 76.);
const SQUARE_RADIUS: f32 = 6.;
// The 2 × 2 pockets' top-left corners, clockwise from the top left (the order Waiting fills them in), and their size.
const POCKETS: [(f32, f32); 4] = [(29., 31.), (53.5, 31.), (53.5, 53.5), (29., 53.5)];
const POCKET: (f32, f32) = (17.5, 15.5);
// Honey across the bottom edge: its top is two cubics, it hangs 3 below the square, and it cuts a gap into the waffle.
const LIP_TOP: [[(f32, f32); 4]; 2] = [[(22., 68.3), (30.5, 64.6), (38.3, 70.4), (48., 67.5)], [(48., 67.5), (58., 64.5), (68., 66.), (78., 64.6)]];
const LIP_BOTTOM: f32 = 79.;
const LIP_GAP: f32 = 2.;
// The lip's highlight, a line cut out of it.
const GLINT: [(f32, f32); 4] = [(27., 72.5), (31., 70.5), (35., 71.2), (38.5, 70.8)];
const GLINT_WIDTH: f32 = 1.8;

// One drip as drawn in the mark, in page space: where it leaves the lip, how far down its bead's centre hangs,
// its neck's radius at the lip and at the bead, and the bead's half width and half height.
#[derive(Clone, Copy)]
struct DripShape {
    x: f32,
    lip: f32,
    len: f32,
    neck: (f32, f32),
    bead: (f32, f32),
}

const DRIPS: [DripShape; 3] = [
    DripShape { x: 46.2, lip: 80.13, len: 12.5, neck: (2.6, 1.7), bead: (3.1, 3.8) },
    DripShape { x: 56.8, lip: 78.25, len: 6.5, neck: (2.4, 1.8), bead: (2.8, 3.3) },
    DripShape { x: 68.7, lip: 76.15, len: 3.4, neck: (2.6, 2.4), bead: (2.8, 3.) },
];
// A honey drop at its fullest in a pocket, in units.
const POCKET_DROP: f32 = 4.5;

type Contour = Vec<(f32, f32)>;

// The waffle (its upper pockets as holes) and the lip (its glint as a hole), tilted into page space.
static WAFFLE: LazyLock<Vec<Contour>> = LazyLock::new(|| tilt(waffle()));
static LIP: LazyLock<Vec<Contour>> = LazyLock::new(|| tilt(lip()));
// Where a drop sits in each pocket, in page space: the middle of the pocket, or of what the lip's gap leaves of it.
static POCKET_CENTRES: LazyLock<[(f32, f32); 4]> = LazyLock::new(|| {
    let cut = cut_line();
    POCKETS.map(|(x, y)| {
        let cx = x + POCKET.0 / 2.;
        tilt_point((cx, (y + (y + POCKET.1).min(y_at(&cut, cx))) / 2.))
    })
});

fn mark(size: f32, ink: Hsla, pose: Pose) -> impl IntoElement {
    canvas(|_, _, _| {}, move |bounds, _, window, _| paint(window, bounds, ink, pose)).size(px(size)).flex_none()
}

fn paint(window: &mut Window, bounds: Bounds<Pixels>, ink: Hsla, pose: Pose) {
    let unit = f32::from(bounds.size.width) / VIEW.2;
    let place = |p| {
        let (x, y) = posed(p, pose);
        bounds.origin + point(px((x - VIEW.0) * unit), px((y - VIEW.1) * unit))
    };
    // One path per part: lyon fills each even-odd, which cuts its holes.
    let mut fill = |contours: &[Contour]| {
        let mut path = PathBuilder::fill();
        for contour in contours {
            let mut points = contour.iter().map(|&p| place(p));
            let Some(first) = points.next() else { continue };
            path.move_to(first);
            points.for_each(|p| path.line_to(p));
            path.close();
        }
        if let Ok(path) = path.build() {
            window.paint_path(path, ink);
        }
    };
    fill(&WAFFLE);
    fill(&LIP);
    // A drip's neck and bead overlap, so each gets its own path, or even-odd would cut the overlap out.
    for contour in DRIPS.iter().zip(pose.drips).flat_map(|(shape, drip)| drip_contours(shape, drip)) {
        fill(&[contour]);
    }
    for (&centre, amount) in POCKET_CENTRES.iter().zip(pose.pockets) {
        if amount > 0.01 {
            fill(&[ellipse(centre, (POCKET_DROP * amount, POCKET_DROP * amount))]);
        }
    }
}

// A page point moved by the pose's scale (about the mark's centre) and lift.
fn posed((x, y): (f32, f32), pose: Pose) -> (f32, f32) {
    let (cx, cy) = (VIEW.0 + VIEW.2 / 2., VIEW.1 + VIEW.2 / 2.);
    (cx + (x - cx) * pose.scale, cy + (y - cy) * pose.scale - pose.lift)
}

// The waffle above the lip's gap: its outline runs up the left side, over the top and down the right, then back
// along the gap, rising into each lower pocket (the gap opens them). The upper pockets are holes.
fn waffle() -> Vec<Contour> {
    let (x0, y0, x1, _) = SQUARE;
    let r = SQUARE_RADIUS * CORNER_SCALE;
    let pr = POCKET.0 * 0.22 * CORNER_SCALE;
    let cut = cut_line();
    let mut outline = vec![(x0, y_at(&cut, x0))];
    outline.extend(corner((x0 + r, y0 + r), r, (-1., -1.), false));
    outline.extend(corner((x1 - r, y0 + r), r, (1., -1.), true));
    let mut from = x1;
    for &(left, top) in [POCKETS[2], POCKETS[3]].iter() {
        let right = left + POCKET.0;
        outline.extend(along(&cut, from, right));
        outline.extend(corner((right - pr, top + pr), pr, (1., -1.), false));
        outline.extend(corner((left + pr, top + pr), pr, (-1., -1.), true));
        from = left;
    }
    outline.extend(along(&cut, from, x0));
    let holes = POCKETS[..2].iter().map(|&(x, y)| squircle_rect(x, y, x + POCKET.0, y + POCKET.1, POCKET.0 * 0.22));
    std::iter::once(outline).chain(holes).collect()
}

fn lip() -> Vec<Contour> {
    let (x0, _, x1, _) = SQUARE;
    let r = SQUARE_RADIUS * CORNER_SCALE;
    let mut outline = lip_top();
    outline.extend(corner((x1 - r, LIP_BOTTOM - r), r, (1., 1.), false));
    outline.extend(corner((x0 + r, LIP_BOTTOM - r), r, (-1., 1.), true));
    // Flat-capped: the caps are under a pixel at any size the mascot is drawn.
    let glint = cubic(GLINT, 12);
    let mut glint_outline = offset(&glint, GLINT_WIDTH / 2.);
    glint_outline.extend(offset(&glint, -GLINT_WIDTH / 2.).into_iter().rev());
    vec![outline, glint_outline]
}

fn lip_top() -> Contour {
    let mut points = cubic(LIP_TOP[0], 16);
    points.extend(cubic(LIP_TOP[1], 16).into_iter().skip(1));
    points
}

// Where the waffle stops above the lip: the lip's top raised by the gap.
fn cut_line() -> Contour {
    offset(&lip_top(), LIP_GAP)
}

// A drip's neck, tapering from inside the lip down to the bead, and the bead; in page space.
fn drip_contours(shape: &DripShape, drip: Drip) -> Vec<Contour> {
    let mut out = Vec::new();
    let (top, bottom) = (shape.lip - 2., shape.lip + drip.neck - shape.bead.1 * drip.bead * 0.5);
    let (n0, n1) = shape.neck;
    if bottom > top {
        let side = |sign: f32| -> Contour {
            (0..=8)
                .map(|i| {
                    let t = i as f32 / 8.;
                    (shape.x + sign * (n0 + (n1 - n0) * t * t * (3. - 2. * t)), top + (bottom - top) * t)
                })
                .collect()
        };
        // Down the left, round the end, up the right.
        let mut neck = side(-1.);
        neck.extend((1..8).map(|i| {
            let a = PI - i as f32 / 8. * PI;
            (shape.x + n1 * a.cos(), bottom + n1 * a.sin())
        }));
        neck.extend(side(1.).into_iter().rev());
        out.push(neck);
    }
    if drip.bead > 0.01 {
        out.push(ellipse((shape.x, shape.lip + drip.bead_y), (shape.bead.0 * drip.bead, shape.bead.1 * drip.bead)));
    }
    out
}

// Corners are superellipses in Waffle's own shape: exponent 2^1.4, radii 1.4x a circular corner's.
const CORNER_SCALE: f32 = 1.4;

// One superellipse corner centred on `c`, bulging toward `dir`, from its horizontal edge's end to its vertical
// edge's end (or reversed).
fn corner((cx, cy): (f32, f32), r: f32, (sx, sy): (f32, f32), reverse: bool) -> Contour {
    let k = 2. / 2f32.powf(1.4);
    (0..=24)
        .map(|i| {
            let t = (if reverse { 24 - i } else { i }) as f32 / 24. * PI / 2.;
            // f32 puts cos(π/2) a hair below zero, and powf of a negative is NaN.
            (cx + sx * r * t.cos().max(0.).powf(k), cy + sy * r * t.sin().max(0.).powf(k))
        })
        .collect()
}

// A rectangle with superellipse corners; `r` is the circular radius it replaces.
fn squircle_rect(x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> Contour {
    let r = (r * CORNER_SCALE).min((x1 - x0) / 2.).min((y1 - y0) / 2.);
    [((x1 - r, y1 - r), (1., 1.), false), ((x0 + r, y1 - r), (-1., 1.), true), ((x0 + r, y0 + r), (-1., -1.), false), ((x1 - r, y0 + r), (1., -1.), true)]
        .into_iter()
        .flat_map(|(c, dir, reverse)| corner(c, r, dir, reverse))
        .collect()
}

fn cubic([p0, p1, p2, p3]: [(f32, f32); 4], steps: usize) -> Contour {
    (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let [a, b, c, d] = [(1. - t).powi(3), 3. * t * (1. - t).powi(2), 3. * t * t * (1. - t), t.powi(3)];
            (a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0, a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1)
        })
        .collect()
}

// A rightward polyline moved `d` units to its left (up the page).
fn offset(line: &[(f32, f32)], d: f32) -> Contour {
    (0..line.len())
        .map(|i| {
            let (a, b) = (line[i.saturating_sub(1)], line[(i + 1).min(line.len() - 1)]);
            let (tx, ty) = (b.0 - a.0, b.1 - a.1);
            let len = tx.hypot(ty);
            (line[i].0 + ty / len * d, line[i].1 - tx / len * d)
        })
        .collect()
}

// The rightward polyline's height at `x`, held flat past its ends.
fn y_at(line: &[(f32, f32)], x: f32) -> f32 {
    let i = line.partition_point(|p| p.0 < x).clamp(1, line.len() - 1);
    let (a, b) = (line[i - 1], line[i]);
    let t = ((x - a.0) / (b.0 - a.0)).clamp(0., 1.);
    a.1 + (b.1 - a.1) * t
}

// The rightward polyline traced leftward from `from` to `to`.
fn along(line: &[(f32, f32)], from: f32, to: f32) -> Contour {
    let mut out = vec![(from, y_at(line, from))];
    out.extend(line.iter().rev().filter(|p| p.0 < from && p.0 > to));
    out.push((to, y_at(line, to)));
    out
}

fn ellipse((cx, cy): (f32, f32), (rx, ry): (f32, f32)) -> Contour {
    (0..24).map(|i| (i as f32 / 24. * TAU).sin_cos()).map(|(s, c)| (cx + rx * c, cy + ry * s)).collect()
}

fn tilt(contours: Vec<Contour>) -> Vec<Contour> {
    contours.into_iter().map(|c| c.into_iter().map(tilt_point).collect()).collect()
}

// SVG's rotate(TILT 50 50).
fn tilt_point((x, y): (f32, f32)) -> (f32, f32) {
    let (s, c) = TILT.to_radians().sin_cos();
    (50. + (x - 50.) * c - (y - 50.) * s, 50. + (x - 50.) * s + (y - 50.) * c)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every point the pose paints, in page units.
    fn painted(pose: Pose) -> Vec<(f32, f32)> {
        let drips = DRIPS.iter().zip(pose.drips).flat_map(|(s, d)| drip_contours(s, d));
        let drops = POCKET_CENTRES.iter().zip(pose.pockets).map(|(&c, a)| ellipse(c, (POCKET_DROP * a, POCKET_DROP * a)));
        WAFFLE.iter().chain(LIP.iter()).cloned().chain(drips).chain(drops).flatten().map(|p| posed(p, pose)).collect()
    }

    fn inside_view(points: &[(f32, f32)]) -> bool {
        points.iter().all(|&(x, y)| x >= VIEW.0 && x <= VIEW.0 + VIEW.2 && y >= VIEW.1 && y <= VIEW.1 + VIEW.2)
    }

    #[test]
    fn every_mood_and_the_loader_stay_inside_the_marks_box() {
        for step in 0..100 {
            let t = step as f32 / 100.;
            for mood in [Mood::Idle, Mood::Waiting, Mood::Happy] {
                let pose = pose(mood, t);
                assert!(pose.scale <= 1. && (0. ..=HOP).contains(&pose.lift) && pose.pockets.iter().all(|a| (0. ..=1.).contains(a)));
                assert!(inside_view(&painted(pose)), "{t}");
            }
            assert!(inside_view(&painted(Pose { drips: loader_drips(t), ..Pose::REST })), "loader at {t}");
        }
    }

    #[test]
    fn the_waffle_keeps_its_upper_pockets_and_clears_the_lip() {
        // The outline plus the two upper pockets; the lower two open into the gap above the lip.
        let waffle = waffle();
        assert_eq!(waffle.len(), 3);
        let lip = lip_top();
        assert!(waffle[0].iter().all(|&(x, y)| y <= y_at(&lip, x) - LIP_GAP * 0.9));
    }

    #[test]
    fn waiting_fills_each_pocket_in_turn() {
        let fill = |i: usize, step: usize| pose(Mood::Waiting, step as f32 / 400.).pockets[i];
        let peaks: Vec<usize> = (0..4).map(|i| (0..400).max_by(|&a, &b| fill(i, a).total_cmp(&fill(i, b))).unwrap()).collect();
        assert!(peaks.windows(2).all(|w| w[0] < w[1]), "{peaks:?}");
    }

    #[test]
    fn a_drip_falls_away_and_starts_again_from_nothing() {
        let (start, end) = (drip_at(0.), drip_at(1. - 1e-6));
        assert!(start.bead == 0. && start.neck == 0. && end.bead < 1e-4 && end.neck < 1e-4);
        // Full length just as it lets go, so the hand-off doesn't jump.
        let (before, after) = (drip_at(DRIP_GROW - 1e-6), drip_at(DRIP_GROW));
        assert!((before.bead_y - after.bead_y).abs() < 1e-3 && (before.bead - after.bead).abs() < 1e-3 && (before.neck - after.neck).abs() < 1e-3);
    }
}
