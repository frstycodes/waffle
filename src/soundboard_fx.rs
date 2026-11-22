// The emoji effects a soundboard sound or voice effect plays over a call tile. Discord plays Lottie
// files here (one Basic, 21 Premium in four families: docs/discord-soundboard-re.md); these are Waffle's
// own keyframes after the same moves, with springier landings: a jelly launch, a hopping parade, rows
// that glide across, a grid that pops in a wave, and a rain. Positions are fractions of the tile
// (x from the left, y from the top, of the emoji's centre); size is a fraction of the tile's height.

use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Family {
    Launch,
    Parade,
    Glide,
    Pop,
    Rain,
}

impl Family {
    pub const ALL: [Family; 5] = [Family::Launch, Family::Parade, Family::Glide, Family::Pop, Family::Rain];

    // Discord's animation_type 0 is Premium, whose id picks one of 21 variants: 0-4 hop across, 5-9
    // glide in rows, 10-15 pop in a grid, 16-20 rain. Type 1, Basic, is Discord's one launch; Waffle
    // varies it across every family by its id instead, so most plays don't all look alike.
    pub fn of(animation_type: Option<u8>, animation_id: Option<u64>) -> Self {
        match (animation_type, animation_id) {
            (Some(0), Some(id)) => match id % 21 {
                0..=4 => Family::Parade,
                5..=9 => Family::Glide,
                10..=15 => Family::Pop,
                _ => Family::Rain,
            },
            (_, Some(id)) => Self::ALL[(id % 5) as usize],
            (_, None) => Family::Launch,
        }
    }

    // Our own play, which Discord doesn't echo back with its animation: any family at random.
    pub fn random() -> Self {
        Self::ALL[rand::random::<usize>() % Self::ALL.len()]
    }

    pub fn duration(self) -> Duration {
        Duration::from_millis(match self {
            Family::Launch => 1350,
            Family::Parade => 2300,
            Family::Glide => 1850,
            Family::Pop => 1850,
            Family::Rain => 1600,
        })
    }

    pub fn copies(self) -> usize {
        match self {
            Family::Launch => 1,
            Family::Parade => 3,
            Family::Glide => 8,
            Family::Pop => 21,
            Family::Rain => 7,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Particle {
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub rotate: f32,
    pub opacity: f32,
}

impl Particle {
    fn at(x: f32, y: f32, size: f32) -> Self {
        Particle { x, y, size, scale_x: 1., scale_y: 1., rotate: 0., opacity: 1. }
    }
}

// Copy `i` of `family` at `secs` into the effect, or None while it's off stage.
pub fn particle(family: Family, i: usize, secs: f32, seed: u64) -> Option<Particle> {
    match family {
        Family::Launch => launch(secs),
        Family::Parade => parade(i, secs),
        Family::Glide => glide(i, secs),
        Family::Pop => pop(i, secs, seed),
        Family::Rain => rain(i, secs, seed),
    }
}

// One emoji shoots up from under the tile, overshoots and wobbles to rest in the middle, holds,
// crouches, and flings off the top. Squash and stretch carry the bounce.
fn launch(t: f32) -> Option<Particle> {
    let y = keys(t, &[(0., 1.25), (0.23, 0.4), (0.43, 0.55), (0.62, 0.47), (0.8, 0.5), (1.0, 0.5), (1.12, 0.6), (1.35, -0.25)]);
    let stretch = keys(t, &[(0., 1.25), (0.2, 1.2), (0.32, 0.82), (0.42, 1.1), (0.52, 0.93), (0.64, 1.04), (0.75, 1.), (1.0, 1.), (1.1, 0.8), (1.18, 1.3), (1.35, 1.35)]);
    let mut p = Particle::at(0.5, y, 0.34);
    (p.scale_x, p.scale_y) = squash(stretch);
    Some(p)
}

// Three emojis hop in from the left along the bottom, each hop lower than the last, squashing as they
// land and tipping into the next jump, and leave on the right.
fn parade(i: usize, t: f32) -> Option<Particle> {
    let t = t - [0., 0.28, 0.5][i.min(2)];
    if !(0. ..=1.75).contains(&t) {
        return None;
    }
    const HOPS: [(f32, f32); 5] = [(0.38, 0.62), (0.32, 0.42), (0.28, 0.3), (0.24, 0.2), (0.22, 0.12)];
    let size = 0.26;
    let floor = 1. - size / 2. - 0.02;
    let (mut start, mut from_x) = (0., -0.15);
    for (n, (length, height)) in HOPS.iter().enumerate() {
        let to_x = from_x + 0.26;
        if t <= start + length {
            let u = (t - start) / length;
            let mut p = Particle::at(lerp(from_x, to_x, u), floor - height * 4. * u * (1. - u), size);
            // Stretched in the air, squashed for the first ~12% after touching down, tilted with the arc.
            let landing = if u < 0.12 && n > 0 { 1. - u / 0.12 } else { 0. };
            let air = (4. * u * (1. - u)).min(1.);
            (p.scale_x, p.scale_y) = squash(1. + 0.18 * air - 0.3 * landing);
            p.rotate = 24. * (u * 2. - 1.) * if n % 2 == 0 { 1. } else { -1. };
            return Some(p);
        }
        start += length;
        from_x = to_x;
    }
    None
}

// Four rows glide in from the left, staggered bottom to top, and back out to the right; then four
// glide back from the right. Each eases to a breath of a pause mid-tile.
fn glide(i: usize, t: f32) -> Option<Particle> {
    let (wave, row) = (i / 4, i % 4);
    let t = t - wave as f32 * 0.8 - row as f32 * 0.06;
    if !(0. ..=0.85).contains(&t) {
        return None;
    }
    let u = t / 0.85;
    // Fast in, a slow stretch through the middle, fast out.
    let eased = 0.5 + 0.5 * ((u * 2. - 1.).powi(3) * 0.75 + (u * 2. - 1.) * 0.25);
    let x = lerp(-0.2, 1.2, eased);
    let x = if wave == 1 { 1. - x } else { x };
    let mut p = Particle::at(x, 0.86 - row as f32 * 0.21, 0.19);
    let speed = 1. - (u * 2. - 1.).powi(2);
    p.rotate = (if wave == 1 { -8. } else { 8. }) * (1. - speed);
    Some(p)
}

// A 7 x 3 grid pops in and out in a wave that sweeps from the bottom left, each emoji overshooting
// past full size and wobbling once before it shrinks away.
fn pop(i: usize, t: f32, seed: u64) -> Option<Particle> {
    let (col, row) = (i % 7, i / 7);
    let lead = (col + (2 - row)) as f32 * 0.07 + jitter(seed, i) * 0.06;
    let t = t - lead;
    if !(0. ..=0.95).contains(&t) {
        return None;
    }
    let scale = keys(t, &[(0., 0.), (0.18, 1.18), (0.3, 0.92), (0.42, 1.03), (0.52, 1.), (0.72, 1.), (0.95, 0.)]);
    let mut p = Particle::at(0.08 + col as f32 * 0.14, 0.2 + row as f32 * 0.3, 0.24);
    (p.scale_x, p.scale_y) = (scale, scale);
    p.rotate = (jitter(seed, i + 40) - 0.5) * 30. * scale;
    Some(p)
}

// Emojis of a few sizes fall from above the tile, staggered, picking up speed and turning as they go.
fn rain(i: usize, t: f32, seed: u64) -> Option<Particle> {
    let lead = [0., 0.12, 0.3, 0.45, 0.55, 0.2, 0.38][i % 7];
    let t = t - lead;
    let length = 0.75 + jitter(seed, i) * 0.25;
    if !(0. ..=length).contains(&t) {
        return None;
    }
    let u = t / length;
    let size = 0.22 + jitter(seed, i + 20) * 0.14;
    let mut p = Particle::at(0.08 + (i as f32 * 0.137 + jitter(seed, i + 10) * 0.08) % 0.86, lerp(-0.3, 1.3, u * u * 0.4 + u * 0.6), size);
    p.rotate = (jitter(seed, i + 30) - 0.5) * 120. * u;
    Some(p)
}

// Area-preserving squash: `stretch` over 1 is taller and thinner.
fn squash(stretch: f32) -> (f32, f32) {
    (1. / stretch.max(0.05).sqrt(), stretch.max(0.05).sqrt())
}

// Piecewise keyframes (seconds, value), each segment eased in and out.
fn keys(t: f32, frames: &[(f32, f32)]) -> f32 {
    let Some(next) = frames.iter().position(|(at, _)| *at >= t) else { return frames.last().map_or(0., |f| f.1) };
    if next == 0 {
        return frames[0].1;
    }
    let ((a, from), (b, to)) = (frames[next - 1], frames[next]);
    let u = ((t - a) / (b - a)).clamp(0., 1.);
    lerp(from, to, u * u * (3. - 2. * u))
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

// 0..1, stable for an effect and copy, so a repaint never reshuffles the scene.
fn jitter(seed: u64, i: usize) -> f32 {
    let mut h = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (i as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 31;
    (h % 1000) as f32 / 1000.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premium_ids_pick_families_and_basic_ids_vary() {
        assert_eq!(Family::of(Some(1), Some(0)), Family::Launch);
        assert_eq!(Family::of(Some(1), Some(3)), Family::Pop);
        assert_eq!(Family::of(Some(1), Some(9)), Family::Rain);
        assert_eq!(Family::of(None, None), Family::Launch);
        assert_eq!(Family::of(Some(0), Some(2)), Family::Parade);
        assert_eq!(Family::of(Some(0), Some(7)), Family::Glide);
        assert_eq!(Family::of(Some(0), Some(12)), Family::Pop);
        assert_eq!(Family::of(Some(0), Some(20)), Family::Rain);
    }

    #[test]
    fn every_effect_starts_and_ends_off_stage_and_shows_in_between() {
        for family in Family::ALL {
            let end = family.duration().as_secs_f32();
            let visible = |secs: f32| (0..family.copies()).filter_map(|i| particle(family, i, secs, 7)).filter(|p| p.opacity > 0. && p.scale_y > 0.05 && (-0.1..1.1).contains(&p.y)).count();
            assert!(visible(end * 0.4) > 0, "{family:?} shows nothing mid-way");
            assert_eq!(visible(end + 0.01), 0, "{family:?} still on stage after it ends");
        }
    }

    #[test]
    fn squash_keeps_the_area() {
        let (x, y) = squash(1.44);
        assert!((x * y - 1.).abs() < 1e-5 && y > x);
    }
}
