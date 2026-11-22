// The design's scales (design/tokens.css) in Rust; keep the two in step. Colours live in `palette`, curves
// and springs in `motion`.
//
// Spacing has no constants here: the window's rem is 16px and nothing changes it, so gpui's own scale is the
// design's. `gap_0p5` 2px, `gap_1` 4px, `p_1p5` 6px, `gap_2` 8px, `px_2p5` 10px, `p_3` 12px, `gap_3p5` 14px,
// `p_4` 16px, `gap_6` 24px, `p_8` 32px, `p_12` 48px. The same steps exist for every padding, margin, gap
// and inset method (`px_3`, `mt_1p5`, `top_2`, …).
use std::time::Duration;

use gpui_kit::{CornerShape, Pixels, px};

// Type sizes. Xxs is the uppercase tag and badge size; the rest are tokens.css `--text-*`.
pub const TEXT_XXS: Pixels = px(11.);
pub const TEXT_XS: Pixels = px(12.);
pub const TEXT_SM: Pixels = px(14.);
pub const TEXT_MD: Pixels = px(16.);
pub const TEXT_LG: Pixels = px(20.);
pub const TEXT_XL: Pixels = px(28.);

// Line heights, for `line_height(relative(..))`: titles, then running text.
pub const LEADING_TIGHT: f32 = 1.2;
pub const LEADING_BODY: f32 = 1.45;

// Radius tiers: tags and tooltips, rows and menu items, controls, cards and tiles, dialogs. Sized for the
// squircle, whose curve hugs the corner tighter than a circular arc of the same radius. Applied with
// `Squircle::squircle`; people, status marks and pills stay `rounded_full`.
pub const RADIUS_XS: f32 = 8.;
pub const RADIUS_SM: f32 = 11.;
pub const RADIUS_MD: f32 = 14.;
pub const RADIUS_LG: f32 = 20.;
pub const RADIUS_XL: f32 = 28.;
// CSS `corner-shape: superellipse(1.4)`.
pub const SQUIRCLE: CornerShape = CornerShape::Superellipse(1.4);

// Glyph sizes, for `Icon::size`.
pub const ICON_XS: Pixels = px(12.);
pub const ICON_SM: Pixels = px(14.);
pub const ICON_MD: Pixels = px(16.);
pub const ICON_LG: Pixels = px(18.);
pub const ICON_XL: Pixels = px(20.);

// People, for `avatar` and the stacks built on it.
pub const AVATAR_XS: f32 = 16.;
pub const AVATAR_SM: f32 = 20.;
pub const AVATAR_MD: f32 = 24.;
pub const AVATAR_LG: f32 = 32.;
pub const AVATAR_XL: f32 = 40.;

// Rules, seams and rings.
pub const HAIRLINE: Pixels = px(1.);
// Padding a surface keeps between its rounded edge and its rows.
pub const SURFACE_INSET: f32 = 6.;
// The least a floating surface keeps from the window's edge.
pub const WINDOW_MARGIN: Pixels = px(8.);

// Motion durations (tokens.css `--dur-*`): press, hover, pop, panel.
pub const DUR_PRESS: Duration = Duration::from_millis(120);
pub const DUR_HOVER: Duration = Duration::from_millis(150);
pub const DUR_POP: Duration = Duration::from_millis(200);
pub const DUR_PANEL: Duration = Duration::from_millis(260);
