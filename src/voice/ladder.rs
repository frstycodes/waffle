// Discord's video ladder: the 16:9 sizes that fit a 1280x720 pixel budget shared by N cameras, one rung per
// size, each with a "want" value (100 for the full budget, 10 less per smaller rung). Op 15 speaks in these
// wants: what we ask of others (`level`) and what the server asks of our camera (`rung`).

use std::sync::OnceLock;

const BUDGET: (u32, u32) = (1280, 720);
// The camera's bitrate range; each rung gets its share of the budget's.
pub(super) const BITRATE_MAX: u32 = 2_500_000;
const BITRATE_MIN: u32 = 150_000;
const MAX_CAMERAS: u32 = 25;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rung {
    pub(super) want: u8,
    pub(super) width: u32,
    pub(super) height: u32,
    // Share of the full budget's pixels, which scales the bitrate.
    pub(super) portion: f64,
    // The frame rate a silent or muted camera drops to.
    pub(super) quiet_fps: u32,
}

impl Rung {
    pub(super) fn bitrate(&self) -> u32 {
        ((f64::from(BITRATE_MAX) * self.portion) as u32).max(BITRATE_MIN)
    }
}

// Lowest want first.
fn ladder() -> &'static [Rung] {
    static LADDER: OnceLock<Vec<Rung>> = OnceLock::new();
    LADDER.get_or_init(|| {
        let budget = BUDGET.0 * BUDGET.1;
        let aligned = |v: u32| matches!(v % 16, 0 | 4 | 8 | 10);
        // Sizes whose width (16/9 of the height) is whole and both sides land on Discord's alignments.
        let sizes: Vec<(u32, u32)> = (1..4096u32).filter(|h| (16 * h) % 9 == 0).map(|h| (16 * h / 9, h)).filter(|&(w, h)| aligned(w) && aligned(h)).collect();
        let (mut rungs, mut want, mut last_width) = (Vec::new(), 100u8, 0);
        for cameras in 1..=MAX_CAMERAS {
            let Some(&(width, height)) = sizes.iter().take_while(|(w, h)| w * h * cameras <= budget).last() else { continue };
            if width != last_width {
                let portion = f64::from(width * height) / f64::from(budget);
                rungs.push(Rung { want, width, height, portion, quiet_fps: if want <= 20 { 12 } else { 20 } });
                want = want.saturating_sub(10);
                last_width = width;
            }
        }
        rungs.reverse();
        rungs
    })
}

// The rung for a want the server asked of us: the highest whose want it meets, else the lowest.
pub(super) fn rung(want: u8) -> Rung {
    let ladder = ladder();
    ladder.iter().take_while(|r| want >= r.want).last().unwrap_or(&ladder[0]).to_owned()
}

// The want to ask of every camera when `cameras` are on: the largest rung that many fit in the budget.
pub(super) fn level(cameras: usize) -> u8 {
    let ladder = ladder();
    let budget = u64::from(BUDGET.0 * BUDGET.1);
    let cameras = cameras.max(1) as u64;
    ladder.iter().take_while(|r| u64::from(r.width * r.height) * cameras <= budget).last().unwrap_or(&ladder[0]).want
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ladder_matches_discords() {
        let top = rung(100);
        assert_eq!((top.width, top.height, top.bitrate()), (1280, 720, 2_500_000));
        assert_eq!((rung(90).width, rung(90).height), (896, 504));
        assert_eq!((rung(70).width, rung(70).height), (640, 360));
        assert_eq!(rung(20).quiet_fps, 12);
        assert_eq!(rung(30).quiet_fps, 20);
        assert_eq!(rung(0), ladder()[0], "below every rung takes the lowest");
        assert!(rung(20).bitrate() >= BITRATE_MIN);
    }

    #[test]
    fn quality_falls_as_cameras_come_on() {
        let levels: Vec<u8> = [0, 1, 2, 4, 5, 7, 10, 12, 17, 40].into_iter().map(level).collect();
        assert_eq!(levels, [100, 100, 90, 70, 60, 50, 40, 30, 20, 20]);
    }
}
