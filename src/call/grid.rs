// Discord's call layout math (design/voice-stage.html): the grid's tile size, the focus strip's tiles and the
// control bar's widths.

// Tiles sit 8px apart, and the grid keeps 64px clear at the top and foot for the header and the bar over it.
pub(super) const GRID_GAP: f32 = 8.;
pub(super) const GRID_KEEP: f32 = 64.;
const RATIO: f32 = 16. / 9.;
// Tiles stop shrinking at the size where this many fit; past it the grid scrolls.
const GRID_MOST: i32 = 25;
// The focus strip: tiles from 124 up to a 112px-tall picture plus its gap, at most 12.
pub(super) const STRIP_MIN: f32 = 124.;
pub(super) const STRIP_MAX: f32 = RATIO * 112. + GRID_GAP;
const STRIP_MOST: usize = 12;
// The strip shows only while the stage is taller than this.
pub(super) const STRIP_FROM_HEIGHT: f32 = 384.;
// The bar drops Invite at this stage width and below, then its whole second section; the carets go in a window this narrow.
const INVITE_UNTIL: f32 = 650.;
const SECOND_UNTIL: f32 = 550.;
pub(super) const CARETS_UNTIL: f32 = 456.;

#[derive(Debug, PartialEq)]
pub(super) struct Grid {
    pub w: f32,
    pub cols: usize,
    // Space above the first row: the 64 kept for the header, plus what centres the rows that don't fill the height.
    pub top: f32,
    pub scrolls: bool,
}

// The biggest 16:9 tile that fits `count` tiles in `width` x `height`, never smaller than the size where 25 fit.
pub(super) fn grid_fit(count: usize, width: f32, height: f32) -> Grid {
    let count = count.max(1) as i32;
    let (l, a) = ((width - 2. * GRID_GAP).floor() as i32, height - 2. * GRID_KEEP);
    let cols = |w: i32| ((l - w) as f32 / (GRID_GAP + w as f32)).floor().max(0.) as i32 + 1;
    let rows = |w: i32, round: fn(f32) -> f32| round((a - w as f32 / RATIO) / (GRID_GAP + w as f32 / RATIO)) as i32 + 1;
    let floor = l / GRID_MOST;
    let size = match count > GRID_MOST {
        true => lower_bound(floor, l, |w| cols(w) * rows(w, f32::ceil) > GRID_MOST),
        false => lower_bound(floor, l, |w| cols(w) * rows(w, f32::floor) >= count) - 1,
    };
    let cols = cols(size).max(1);
    let rows = ((count + cols - 1) / cols) as f32;
    let row_height = (size as f32 / RATIO).floor() + GRID_GAP;
    Grid { w: size.max(1) as f32, cols: cols as usize, top: GRID_KEEP + (a - row_height * rows).max(0.) / 2., scrolls: row_height * rows > a + GRID_GAP }
}

// The first value in `lo..hi` past the run where `fits` holds (Discord's binary search; `fits` holds up to a point).
fn lower_bound(mut lo: i32, hi: i32, fits: impl Fn(i32) -> bool) -> i32 {
    let mut span = hi - lo;
    while span > 0 {
        let half = span / 2;
        if fits(lo + half) {
            lo += half + 1;
            span -= half + 1;
        } else {
            span = half;
        }
    }
    lo
}

// Where a tile goes in the focus strip: activities, streams, people speaking, people who spoke in the last 10s,
// cameras, then everyone else.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(super) enum StripRank {
    Activity,
    Stream,
    Speaking,
    SpokeRecently,
    Camera,
    Rest,
}

pub(super) struct Strip<T> {
    pub tiles: Vec<T>,
    pub w: f32,
    // Your own camera, pinned at the end at the largest size.
    pub own: Option<T>,
}

// The strip for `width`: as many tiles as fit at 124px or more (at most 12), ranked, your camera held back for the end.
pub(super) fn strip_fit<T>(width: f32, mut tiles: Vec<T>, rank: impl Fn(&T) -> StripRank, own_camera: impl Fn(&T) -> bool) -> Strip<T> {
    let total = tiles.len();
    tiles.sort_by_key(&rank);
    let own = tiles.iter().position(&own_camera).map(|i| tiles.remove(i));
    let room = if own.is_some() { width - STRIP_MAX - GRID_GAP } else { width };
    let fit = (((room - GRID_GAP) / (STRIP_MIN + GRID_GAP)).floor().max(0.) as usize).min(STRIP_MOST).min(total);
    let w = ((room - GRID_GAP) / fit.max(1) as f32 - GRID_GAP).min(STRIP_MAX);
    tiles.truncate(fit);
    Strip { tiles, w, own }
}

// How much of the control bar fits a stage this wide.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Tray {
    Full,
    NoInvite,
    // Mic, camera and Disconnect only.
    Minimal,
}

impl Tray {
    pub(super) fn for_width(width: f32) -> Self {
        match width {
            w if w <= SECOND_UNTIL => Tray::Minimal,
            w if w <= INVITE_UNTIL => Tray::NoInvite,
            _ => Tray::Full,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_fits_until_25_then_scrolls() {
        for n in 1..=25 {
            for (width, height) in [(1600., 700.), (500., 1300.), (980., 560.), (900., 520.)] {
                let g = grid_fit(n, width, height);
                let rows = n.div_ceil(g.cols) as f32;
                assert!(g.w * g.cols as f32 + GRID_GAP * (g.cols as f32 + 1.) <= width + 0.5, "{n} tiles overflow {width} wide");
                assert!(!g.scrolls, "{n} tiles at {width}x{height} scroll");
                assert!((g.w / RATIO).floor() * rows + GRID_GAP * (rows - 1.) <= height - 2. * GRID_KEEP + 0.5, "{n} tiles overflow {height} tall");
            }
        }
        let crowded = grid_fit(30, 980., 560.);
        assert!(crowded.scrolls);
        assert!(crowded.w >= ((980_f32 - 16.) / 25.).floor());
        assert_eq!(grid_fit(1, 980., 560.).cols, 1);
        assert_eq!(grid_fit(4, 1280., 720.).cols, 2);
    }

    #[test]
    fn the_strip_ranks_and_pins_your_camera_last() {
        let tiles = vec![("rest", StripRank::Rest), ("me", StripRank::Camera), ("cam", StripRank::Camera), ("talking", StripRank::Speaking), ("live", StripRank::Stream)];
        let strip = strip_fit(1200., tiles, |t| t.1, |t| t.0 == "me");
        assert_eq!(strip.tiles.iter().map(|t| t.0).collect::<Vec<_>>(), ["live", "talking", "cam", "rest"]);
        assert_eq!(strip.own.map(|t| t.0), Some("me"));
        assert!(strip.w >= STRIP_MIN && strip.w <= STRIP_MAX);
        let many = strip_fit(4000., (0..20).map(|i| (i, StripRank::Rest)).collect(), |t| t.1, |_| false);
        assert_eq!(many.tiles.len(), 12);
        let narrow = strip_fit(300., (0..5).map(|i| (i, StripRank::Rest)).collect(), |t| t.1, |_| false);
        assert_eq!(narrow.tiles.len(), 2);
    }

    #[test]
    fn the_bar_drops_invite_then_its_second_section() {
        assert_eq!(Tray::for_width(651.), Tray::Full);
        assert_eq!(Tray::for_width(650.), Tray::NoInvite);
        assert_eq!(Tray::for_width(551.), Tray::NoInvite);
        assert_eq!(Tray::for_width(550.), Tray::Minimal);
    }
}
