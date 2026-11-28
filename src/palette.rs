use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use gpui_kit::{
    App, Hsla, Window,
    component::{ActiveTheme, Theme, ThemeMode, scroll::ScrollbarMode},
    rgb,
};

// Waffle's colours, ported from the design system (design/tokens.css, Graphite neutrals). The OKLCH
// tokens there are converted to sRGB here; keep the two in step.
//
// Honey is the one accent: primary actions, current selection and focus. It is light, so what sits on
// it is dark (ON_ACCENT, 7.8:1), never white. What asks for you (counts, LIVE, Do Not Disturb) is a true
// red, oklch(0.58 0.21 25), and danger is crimson, so Delete never looks like the primary action.
// GAP: idle and warning are a yellow close to honey; elevated-mode banners can read as the accent.
const ACCENT: u32 = 0xEFA831;
const ACCENT_HOVER: u32 = 0xE29A1D;
const ACCENT_PRESS: u32 = 0xD68D0A;
const ON_ACCENT: u32 = 0x2A1C00;
// The mark's honey. On light surfaces it deepens (LOGO_ON_LIGHT, 3.2:1) so the mark still holds.
const LOGO: u32 = 0xEFA831;
const LOGO_ON_LIGHT: u32 = 0xBF7D06;
const DANGER: u32 = 0xC21F3A;
const DANGER_HOVER: u32 = 0xA8172F;
const SUCCESS: u32 = 0x00753E;
const SUCCESS_HOVER: u32 = 0x00662F;
const ALERT: u32 = 0xDB2B33;
const DND: u32 = ALERT;
const OFFLINE: u32 = 0x83868C;
// Discord's presence marks (its GREEN_NEW_38, YELLOW_NEW_30, RED_NEW_46, NEUTRAL_34, Twitch purple).
const PRESENCE_ONLINE: u32 = 0x45A366;
const PRESENCE_IDLE: u32 = 0xFFC04E;
const PRESENCE_DND: u32 = 0xDA3E44;
const PRESENCE_OFFLINE: u32 = 0x84858D;
const PRESENCE_STREAMING: u32 = 0x9147FF;
// Graphite's cast, for the few neutrals computed from OKLCH rather than listed below.
const NEUTRAL_HUE: f32 = 260.;
const NEUTRAL_CHROMA: f32 = 0.004;

// One theme's colours by role.
struct Tokens {
    // Surfaces, deepest to highest: server rail, sidebar, page, raised (inputs, cards), overlay (menus).
    rail: u32,
    sidebar: u32,
    page: u32,
    raised: u32,
    group: u32,
    overlay: u32,
    strong: u32,
    text: u32,
    muted: u32,
    faint: u32,
    accent_ink: u32,
    danger_ink: u32,
    online: u32,
    online_ink: u32,
    idle: u32,
    info_ink: u32,
    warning_ink: u32,
    // The unfilled part of sliders, progress and switches; opaque so overlapping pieces never double up.
    track: u32,
    // Translucent fills over any surface: white in the dark theme, ink in the light one.
    ink_wash: u32,
    hover: f32,
    press: f32,
    selected: f32,
    line: f32,
    line_strong: f32,
    // The accent tint, a touch deeper on light so it still reads on white.
    accent_soft: (u32, f32),
    shade: u32,
    shimmer: (u32, f32),
    seg_track: (u32, f32),
    seg_thumb: (u32, f32),
}

const DARK: Tokens = Tokens {
    rail: 0x0A0B0D,
    sidebar: 0x111213,
    page: 0x17181A,
    raised: 0x202224,
    group: 0x202224,
    overlay: 0x26282A,
    strong: 0xF3F5F8,
    text: 0xD4D8DD,
    muted: 0xA0A5AC,
    faint: 0x8B8F97,
    accent_ink: 0xF6B84D,
    danger_ink: 0xFF7A8C,
    online: 0x3EC87D,
    online_ink: 0x59D38C,
    idle: 0xF2B036,
    info_ink: 0x65B4E9,
    warning_ink: 0xF3B94C,
    track: 0x33353A,
    ink_wash: 0xFFFFFF,
    hover: 0.05,
    press: 0.09,
    selected: 0.08,
    line: 0.08,
    line_strong: 0.14,
    accent_soft: (0xEFA831, 0.16),
    shade: 0x000000,
    shimmer: (0xFFFFFF, 0.06),
    seg_track: (0x000000, 0.3),
    seg_thumb: (0xFFFFFF, 0.15),
};

const LIGHT: Tokens = Tokens {
    rail: 0xDEE0E3,
    sidebar: 0xECEEF0,
    page: 0xF8F9FB,
    raised: 0xFDFDFE,
    // Darker than the page, not lighter: a borderless group one step lighter than F8F9FB can't be seen.
    group: 0xF0F1F3,
    overlay: 0xFEFEFF,
    strong: 0x14161A,
    text: 0x2B2E32,
    muted: 0x55585E,
    faint: 0x5D6166,
    accent_ink: 0x8A5601,
    danger_ink: 0xB0182F,
    online: 0x00A159,
    online_ink: 0x00723B,
    idle: 0xDA950B,
    info_ink: 0x0068A7,
    warning_ink: 0x975800,
    track: 0xD1D4DA,
    ink_wash: 0x2B2E32,
    hover: 0.06,
    press: 0.1,
    selected: 0.09,
    line: 0.1,
    line_strong: 0.18,
    accent_soft: (0xEFA831, 0.14),
    shade: 0x2B2E32,
    shimmer: (0xFFFFFF, 0.6),
    seg_track: (0x2B2E32, 0.1),
    seg_thumb: (0xFFFFFF, 1.),
};

// High contrast (Discord's "bold colors and sharp contrast"): secondary text steps up a rung (muted takes
// the text colour, faint takes muted), hairlines take the strong line, and fills and strong lines deepen.
const HIGH_CONTRAST_FILL: f32 = 1.75;
const DARK_HC: Tokens = high_contrast(DARK);
const LIGHT_HC: Tokens = high_contrast(LIGHT);

const fn high_contrast(t: Tokens) -> Tokens {
    Tokens {
        muted: t.text,
        faint: t.muted,
        hover: t.hover * HIGH_CONTRAST_FILL,
        press: t.press * HIGH_CONTRAST_FILL,
        selected: t.selected * HIGH_CONTRAST_FILL,
        line: t.line_strong,
        line_strong: t.line_strong * 1.5,
        ..t
    }
}

fn tokens(cx: &App) -> &'static Tokens {
    match (cx.theme().is_dark(), HIGH_CONTRAST.load(Ordering::Relaxed)) {
        (true, false) => &DARK,
        (false, false) => &LIGHT,
        (true, true) => &DARK_HC,
        (false, true) => &LIGHT_HC,
    }
}

// Settings › Accessibility's look, read by every colour built here (atomics: `hex` runs without an App).
static SATURATION: AtomicU32 = AtomicU32::new(1f32.to_bits());
static DESATURATE_USER_COLORS: AtomicBool = AtomicBool::new(false);
static HIGH_CONTRAST: AtomicBool = AtomicBool::new(false);

// Stores the prefs' look; true when it changed, so the caller re-applies the theme (`apply`).
pub fn set_look(prefs: &crate::settings::Prefs) -> bool {
    let saturation = prefs.saturation.clamp(0., 1.).to_bits();
    let changed = [
        SATURATION.swap(saturation, Ordering::Relaxed) != saturation,
        DESATURATE_USER_COLORS.swap(prefs.desaturate_user_colors, Ordering::Relaxed) != prefs.desaturate_user_colors,
        HIGH_CONTRAST.swap(prefs.high_contrast, Ordering::Relaxed) != prefs.high_contrast,
    ];
    changed.contains(&true)
}

fn saturation() -> f32 {
    f32::from_bits(SATURATION.load(Ordering::Relaxed))
}

// Scales saturation alone, so hue and lightness hold.
fn desaturate(mut colour: Hsla, saturation: f32) -> Hsla {
    colour.s *= saturation;
    colour
}

// Every UI colour passes through here, so the saturation setting reaches all of them. Pictures (images,
// video, avatars, emoji) never do.
fn hex(value: u32) -> Hsla {
    desaturate(rgb(value).into(), saturation())
}

// A colour given as data (a role, an embed, a profile theme, a folder, a nameplate, a theme preview), exactly
// as given: the saturation setting doesn't reach it.
pub fn data_color(value: u32) -> Hsla {
    rgb(value).into()
}

// A stream's tile before its first frame: a deep ground under a teal-to-magenta glow, in both themes.
pub fn stream_ground() -> Hsla {
    rgb(0x1D1E39).into()
}

pub fn stream_glow() -> (Hsla, Hsla) {
    (rgb(0x0097A1).into(), rgb(0x9D2398).into())
}

// The design's media scrim, oklch(0.14 0.004 260): what dims the edges of images and video.
pub fn media_scrim() -> Hsla {
    rgb(0x08090B).into()
}

// Strong ink on a fill that's light whatever the theme (a light profile theme, a pale brand tile).
pub fn ink_on_light() -> Hsla {
    rgb(0x1A1A1E).into()
}

// Status green deepened to stay readable on a light profile card (the status green is ~2:1 there).
pub fn online_on_light() -> Hsla {
    rgb(0x1E7A46).into()
}

// The QR code's modules: dark on white in both themes, or phones can't read it: oklch(0.22 0.004 260).
pub fn qr_ink() -> Hsla {
    rgb(0x191B1C).into()
}

// A call's media badge on the rail while not connected: dark under white ink in both themes, as Discord draws it.
pub fn media_badge() -> Hsla {
    gpui_kit::hsla(0., 0., 0.08, 1.)
}

// Windows 11's caption red, not the app's danger tone: Close should look like every other window's.
pub fn window_close_hover() -> Hsla {
    rgb(0xC42B1C).into()
}

pub fn sync(window: &mut Window, cx: &mut App) {
    Theme::sync_system_appearance(Some(window), cx);
    let prefs = crate::settings::Prefs::load();
    set_look(&prefs);
    match prefs.theme {
        crate::settings::ThemePref::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
        crate::settings::ThemePref::Light => Theme::change(ThemeMode::Light, Some(window), cx),
        crate::settings::ThemePref::System => {}
    }
    #[cfg(debug_assertions)]
    if let Ok(mode) = std::env::var("WAFFLE_THEME") {
        let mode = if mode == "dark" { ThemeMode::Dark } else { ThemeMode::Light };
        Theme::change(mode, Some(window), cx);
    }
    apply(cx);
}

// Paints Waffle's tokens onto the current theme mode (call after `Theme::change`).
// Code colours: One Dark Pro's in the dark theme, One Light's in the light one, on Waffle's own surfaces.
// Each is that palette's hue at a lightness that holds 4.5:1 on the raised surface code blocks sit on
// (only One Dark's comment and most of One Light needed moving). Variables keep the text colour: every
// identifier is one, so One Dark's red would flood the block.
const SYNTAX_DARK: [(&str, u32); 12] = [
    ("keyword", 0xC678DD),
    ("function", 0x61AFEF),
    ("string", 0x98C379),
    ("number", 0xD19A66),
    ("type", 0xE5C07B),
    ("comment", 0x868B94),
    ("variable.special", 0xE06C75),
    ("property", 0xE06C75),
    ("tag", 0xE06C75),
    ("operator", 0x56B6C2),
    ("string.escape", 0x56B6C2),
    ("punctuation", 0xABB2BF),
];
const SYNTAX_LIGHT: [(&str, u32); 12] = [
    ("keyword", 0xA626A4),
    ("function", 0x1358EF),
    ("string", 0x397338),
    ("number", 0x9A4F00),
    ("type", 0x7A5F00),
    ("comment", 0x65666D),
    ("variable.special", 0xC32B1D),
    ("property", 0xC32B1D),
    ("tag", 0xC32B1D),
    ("operator", 0x016D9B),
    ("string.escape", 0x016D9B),
    ("punctuation", 0x383A42),
];

// Names that take the colour of a listed one, the way One Dark groups them.
const SYNTAX_ALIASES: [(&str, &str); 16] = [
    ("comment.doc", "comment"),
    ("preproc", "keyword"),
    ("tag.doctype", "keyword"),
    ("constructor", "type"),
    ("enum", "number"),
    ("constant", "number"),
    ("boolean", "number"),
    ("attribute", "number"),
    ("variant", "function"),
    ("link_text", "function"),
    ("link_uri", "operator"),
    ("string.regex", "string.escape"),
    ("string.special", "string.escape"),
    ("string.special.symbol", "string.escape"),
    ("punctuation.bracket", "punctuation"),
    ("punctuation.delimiter", "punctuation"),
];

fn highlight_theme(dark: bool) -> std::sync::Arc<gpui_kit::component::highlighter::HighlightTheme> {
    let colours = if dark { &SYNTAX_DARK } else { &SYNTAX_LIGHT };
    let colour = |name: &str| colours.iter().find(|(n, _)| *n == name).map(|(_, c)| format!("#{c:06X}"));
    let mut syntax = serde_json::Map::new();
    for (name, _) in colours {
        syntax.insert((*name).into(), serde_json::json!({ "color": colour(name), "font_style": (*name == "comment").then_some("italic") }));
    }
    for (name, like) in SYNTAX_ALIASES {
        syntax.insert(name.into(), serde_json::json!({ "color": colour(like), "font_style": (like == "comment").then_some("italic") }));
    }
    let json = serde_json::json!({ "name": if dark { "Waffle One Dark" } else { "Waffle One Light" }, "appearance": if dark { "dark" } else { "light" }, "style": { "syntax": syntax } });
    std::sync::Arc::new(serde_json::from_value(json).expect("a valid highlight theme"))
}

pub fn apply(cx: &mut App) {
    let t = tokens(cx);
    // `update`, not `global_mut`: gpui-component's own widgets paint from `theme.tokens`, which only
    // `update` reconciles with the colours set here.
    Theme::update(cx, |theme| {
        // The picked interface and code fonts: Figtree and Roboto Mono, bundled, by default.
        let prefs = crate::settings::Prefs::load();
        theme.font_family = prefs.ui_font.into();
        theme.mono_font_family = prefs.mono_font.into();
        theme.highlight_theme = highlight_theme(theme.is_dark());
        let text = hex(t.text);
        let line = hex(t.ink_wash).opacity(t.line);
        theme.background = hex(t.page);
        theme.title_bar = hex(t.rail);
        theme.sidebar = hex(t.sidebar);
        theme.popover = hex(t.overlay);
        theme.muted = hex(t.raised);
        theme.border = line;
        theme.sidebar_border = line;
        theme.input = hex(t.ink_wash).opacity(t.line_strong);
        theme.foreground = text;
        theme.sidebar_foreground = text;
        theme.popover_foreground = text;
        theme.caret = text;
        theme.muted_foreground = hex(t.muted);
        theme.sidebar_accent = hex(t.ink_wash).opacity(t.selected);
        theme.sidebar_accent_foreground = hex(t.strong);
        theme.list_hover = hex(t.ink_wash).opacity(t.hover);
        theme.primary = accent();
        theme.primary_hover = accent_hover();
        theme.primary_active = hex(ACCENT_PRESS);
        theme.primary_foreground = on_accent();
        theme.ring = hex(t.accent_ink);
        theme.selection = accent().opacity(0.3);
        theme.link = hex(t.accent_ink);
        theme.link_hover = theme.link;
        theme.danger = danger();
        theme.danger_hover = danger_hover();
        theme.danger_active = danger_hover();
        theme.danger_foreground = gpui_kit::white();
        theme.success = hex(SUCCESS);
        theme.warning = hex(t.idle);
        theme.scrollbar_thumb = hex(t.ink_wash).opacity(t.line_strong);
        theme.scrollbar = gpui_kit::transparent_black();
        // Revealed by scrolling or by the pointer over the bar's strip; init's system sync picks `Scrolling`
        // on macOS, which never shows the bar to a pointer that hasn't scrolled.
        theme.scrollbar_mode = ScrollbarMode::Hover;
    });
}

pub fn accent() -> Hsla {
    hex(ACCENT)
}

pub fn logo() -> Hsla {
    hex(LOGO)
}

// The mark on the current theme's surfaces: honey on dark, a deeper honey on light.
pub fn logo_ink(cx: &App) -> Hsla {
    if cx.theme().mode.is_dark() { logo() } else { logo_on_light() }
}

// The mark on a surface that's light whatever the theme (the sign-in QR card).
pub fn logo_on_light() -> Hsla {
    hex(LOGO_ON_LIGHT)
}

// Text and icons on an accent fill.
pub fn on_accent() -> Hsla {
    hex(ON_ACCENT)
}

pub fn accent_hover() -> Hsla {
    hex(ACCENT_HOVER)
}

pub fn accent_press() -> Hsla {
    hex(ACCENT_PRESS)
}

// The accent as a tint: mention rows, your own reactions, selected chips, the focus ring.
pub fn accent_soft(cx: &App) -> Hsla {
    let t = tokens(cx);
    hex(t.accent_soft.0).opacity(t.accent_soft.1)
}

// The tint behind a callout of each tone (design/css/primitives.css, `.callout--*`). Same in both themes.
pub fn info_tint() -> Hsla {
    hex(0x3D98D1).opacity(0.12)
}

pub fn warning_tint() -> Hsla {
    hex(0xF2B036).opacity(0.13)
}

pub fn danger_tint() -> Hsla {
    hex(0xDE3E2D).opacity(0.12)
}

pub fn success_tint() -> Hsla {
    hex(0x3BB974).opacity(0.12)
}

// What drop shadows are made of, at full strength: black on dark, Graphite's near-black on light.
pub fn shade(cx: &App) -> Hsla {
    hex(tokens(cx).shade)
}

// What dims the app behind a dialog (tokens.css --scrim): a deep Graphite veil on dark, a lighter one on light.
pub fn scrim(cx: &App) -> Hsla {
    if cx.theme().is_dark() {
        return oklch(0.08, NEUTRAL_CHROMA, NEUTRAL_HUE).opacity(0.7);
    }
    oklch(0.2, NEUTRAL_CHROMA, NEUTRAL_HUE).opacity(0.4)
}

// The skeleton's sweep.
pub fn shimmer(cx: &App) -> Hsla {
    let (c, a) = tokens(cx).shimmer;
    hex(c).opacity(a)
}

// The call stage is dark in both themes (design/css/voice.css `.call`), so whatever sits on it reads
// Graphite's dark tokens instead of the current theme's.
pub mod stage {
    use super::{DARK, DND, Hsla, LIGHT, hex};

    pub fn bg() -> Hsla {
        hex(DARK.rail)
    }

    pub fn overlay() -> Hsla {
        hex(DARK.overlay)
    }

    pub fn ended() -> Hsla {
        hex(DARK.sidebar)
    }

    // The control bar's groups: opaque, a step up from the stage, so they hold over any picture.
    pub fn dock() -> Hsla {
        hex(DARK.page)
    }

    pub fn strong() -> Hsla {
        hex(DARK.strong)
    }

    pub fn text() -> Hsla {
        hex(DARK.text)
    }

    pub fn muted() -> Hsla {
        hex(DARK.muted)
    }

    pub fn faint() -> Hsla {
        hex(DARK.faint)
    }

    pub fn online() -> Hsla {
        hex(DARK.online)
    }

    pub fn idle() -> Hsla {
        hex(DARK.idle)
    }

    // Translucent white: hover .05, press .09, selected .08, line .08, strong line .14.
    pub fn wash(alpha: f32) -> Hsla {
        hex(DARK.ink_wash).opacity(alpha)
    }

    // Off states (muted, camera off) are soft coral, apart from the red that leaves.
    pub fn off(hover: bool) -> Hsla {
        hex(DND).opacity(if hover { 0.3 } else { 0.2 })
    }

    pub fn off_ink() -> Hsla {
        hex(0xFF9079)
    }

    // A muted mic on the call's bar and tiles, as Discord's: the light red ink on a 12% wash of red (16% hovered,
    // 24% pressed).
    pub fn muted_fill(alpha: f32) -> Hsla {
        hex(DND).opacity(alpha)
    }

    pub fn muted_ink() -> Hsla {
        hex(DARK.danger_ink)
    }

    // On states (camera on) are a light fill with dark ink.
    pub fn on(hover: bool) -> Hsla {
        hex(if hover { DARK.text } else { DARK.strong })
    }

    pub fn on_ink() -> Hsla {
        hex(LIGHT.strong)
    }
}

// An OKLCH colour (hue in degrees), for colours the design computes: avatar gradients, role dots, swatches.
pub fn oklch(l: f32, c: f32, hue: f32) -> Hsla {
    hex(from_oklch(l, c, hue.to_radians()))
}

// An OKLCH colour standing in for a picture (an avatar's monogram), which keeps its colour at any saturation.
pub fn picture_oklch(l: f32, c: f32, hue: f32) -> Hsla {
    rgb(from_oklch(l, c, hue.to_radians())).into()
}

pub fn danger() -> Hsla {
    hex(DANGER)
}

pub fn danger_hover() -> Hsla {
    hex(DANGER_HOVER)
}

// The positive fill (an app's Success button), under white ink.
pub fn success() -> Hsla {
    hex(SUCCESS)
}

pub fn success_hover() -> Hsla {
    hex(SUCCESS_HOVER)
}

// Counts, unread marks and LIVE: what asks for you.
pub fn alert() -> Hsla {
    hex(ALERT)
}

// The surface ramp, deepest first.
// WCAG relative luminance of an opaque colour, 0 to 1.
pub fn luminance(c: Hsla) -> f32 {
    let c = gpui_kit::Rgba::from(c);
    let lin = |v: f32| if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

// WCAG contrast ratio between two opaque colours, 1 to 21.
pub fn contrast(a: Hsla, b: Hsla) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Rail,
    Sidebar,
    Page,
    Raised,
    // A settings group: borderless rows one step from the page (lighter in dark, darker in light).
    Group,
    Overlay,
}

pub fn surface(level: Level, cx: &App) -> Hsla {
    let t = tokens(cx);
    hex(match level {
        Level::Rail => t.rail,
        Level::Sidebar => t.sidebar,
        Level::Page => t.page,
        Level::Raised => t.raised,
        Level::Group => t.group,
        Level::Overlay => t.overlay,
    })
}

// Translucent fills that work over any surface.
pub fn hover(cx: &App) -> Hsla {
    let t = tokens(cx);
    hex(t.ink_wash).opacity(t.hover)
}

pub fn press(cx: &App) -> Hsla {
    let t = tokens(cx);
    hex(t.ink_wash).opacity(t.press)
}

pub fn selected(cx: &App) -> Hsla {
    let t = tokens(cx);
    hex(t.ink_wash).opacity(t.selected)
}

pub fn line(cx: &App) -> Hsla {
    let t = tokens(cx);
    hex(t.ink_wash).opacity(t.line)
}

pub fn line_strong(cx: &App) -> Hsla {
    let t = tokens(cx);
    hex(t.ink_wash).opacity(t.line_strong)
}

pub fn track(cx: &App) -> Hsla {
    hex(tokens(cx).track)
}

pub fn seg_track(cx: &App) -> Hsla {
    let (c, a) = tokens(cx).seg_track;
    hex(c).opacity(a)
}

pub fn seg_thumb(cx: &App) -> Hsla {
    let (c, a) = tokens(cx).seg_thumb;
    hex(c).opacity(a)
}

// Text, strongest to faintest. Faint still clears 4.5:1 on every surface.
pub fn strong(cx: &App) -> Hsla {
    hex(tokens(cx).strong)
}

pub fn text(cx: &App) -> Hsla {
    hex(tokens(cx).text)
}

pub fn muted(cx: &App) -> Hsla {
    hex(tokens(cx).muted)
}

pub fn faint(cx: &App) -> Hsla {
    hex(tokens(cx).faint)
}

pub fn accent_ink(cx: &App) -> Hsla {
    hex(tokens(cx).accent_ink)
}

pub fn positive_ink(cx: &App) -> Hsla {
    hex(tokens(cx).online_ink)
}

// Discord's server-boost pink, for the gem beside a booster's name.
pub fn boost_ink(cx: &App) -> Hsla {
    hex(if cx.theme().is_dark() { 0xF07CF5 } else { 0xB21FC4 })
}

// Green on a tooltip, whose surface is the other theme's (`ui::tip` inverts the page).
pub fn positive_ink_inverted(cx: &App) -> Hsla {
    hex(if cx.theme().is_dark() { LIGHT.online_ink } else { DARK.online_ink })
}

pub fn danger_ink(cx: &App) -> Hsla {
    hex(tokens(cx).danger_ink)
}

pub fn info_ink(cx: &App) -> Hsla {
    hex(tokens(cx).info_ink)
}

pub fn warning_ink(cx: &App) -> Hsla {
    hex(tokens(cx).warning_ink)
}

// Presence colours. Status is never colour alone: `ui::status` gives each its shape.
pub fn status_in(status: crate::discord::Status, cx: &App) -> Hsla {
    use crate::discord::Status;
    let t = tokens(cx);
    hex(match status {
        Status::Online => t.online,
        Status::Idle => t.idle,
        Status::Dnd => DND,
        Status::Offline => OFFLINE,
    })
}

// A presence mark's colour, Discord's own and the same in both themes.
pub fn presence(mark: crate::discord::Mark) -> Hsla {
    use crate::discord::{Mark, Status};
    hex(match mark {
        Mark::Status(Status::Online) | Mark::Mobile | Mark::Typing => PRESENCE_ONLINE,
        Mark::Status(Status::Idle) => PRESENCE_IDLE,
        Mark::Status(Status::Dnd) => PRESENCE_DND,
        Mark::Status(Status::Offline) => PRESENCE_OFFLINE,
        Mark::Streaming => PRESENCE_STREAMING,
    })
}

// Presence colours for callers without a theme at hand (the dark values).
pub fn status(status: crate::discord::Status) -> Hsla {
    use crate::discord::Status;
    hex(match status {
        Status::Online => DARK.online,
        Status::Idle => DARK.idle,
        Status::Dnd => DND,
        Status::Offline => OFFLINE,
    })
}

// A role's colour as a name colour: the role's own hex, as Discord shows it. Only a role too dark for the
// dark theme (or too light for the light one) moves, in lightness alone, to the floor that still reads.
// Deviates from design/css/navigation.css's .role-ink, whose clamp turned every role pastel.
// Saturation reaches it only with "Apply saturation setting to custom colors" on.
pub fn role_ink(color: u32, dark: bool) -> Hsla {
    let (l, c, h) = to_oklch(color);
    let readable = if dark { l.max(0.62) } else { l.min(0.55) };
    let ink = rgb(if readable == l { color } else { from_oklch(readable, c, h) }).into();
    if !DESATURATE_USER_COLORS.load(Ordering::Relaxed) {
        return ink;
    }
    desaturate(ink, saturation())
}

fn to_oklch(color: u32) -> (f32, f32, f32) {
    let lin = |shift: u32| {
        let c = ((color >> shift) & 0xFF) as f32 / 255.;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let (r, g, b) = (lin(16), lin(8), lin(0));
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    let (lab_l, a, bb) = (0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s, 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s, 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s);
    (lab_l, a.hypot(bb), bb.atan2(a))
}

fn from_oklch(l: f32, c: f32, h: f32) -> u32 {
    let (a, b) = (c * h.cos(), c * h.sin());
    let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    let channel = |v: f32| {
        let v = v.clamp(0., 1.);
        let srgb = if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1. / 2.4) - 0.055 };
        (srgb * 255.).round() as u32
    };
    let r = channel(4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_);
    let g = channel(-1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_);
    let b = channel(-0.0041960863 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_);
    (r << 16) | (g << 8) | b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(value: u32) -> f32 {
        let channel = |shift: u32| {
            let c = ((value >> shift) & 0xFF) as f32 / 255.;
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }

    fn contrast(a: u32, b: u32) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    // `top` at `alpha` composited over `bottom`.
    fn over(top: u32, alpha: f32, bottom: u32) -> u32 {
        [16, 8, 0].iter().fold(0, |acc, shift| {
            let mix = ((top >> shift) & 0xFF) as f32 * alpha + ((bottom >> shift) & 0xFF) as f32 * (1. - alpha);
            acc | ((mix.round() as u32) << shift)
        })
    }

    #[test]
    fn text_meets_aa_on_every_surface() {
        for t in [&DARK, &LIGHT, &DARK_HC, &LIGHT_HC] {
            let selected_row = over(t.ink_wash, t.selected, t.sidebar);
            let surfaces = [t.rail, t.sidebar, t.page, t.raised, t.overlay, selected_row];
            for ink in [t.text, t.strong, t.muted, t.faint] {
                for surface in surfaces {
                    let ratio = contrast(ink, surface);
                    assert!(ratio >= 4.5, "{ink:06X} on {surface:06X} is {ratio:.2}:1");
                }
            }
            for ink in [t.accent_ink, t.online_ink, t.danger_ink, t.info_ink, t.warning_ink] {
                for surface in [t.sidebar, t.page, t.raised] {
                    let ratio = contrast(ink, surface);
                    assert!(ratio >= 4.5, "{ink:06X} on {surface:06X} is {ratio:.2}:1");
                }
            }
        }
        for fill in [ALERT, DANGER, SUCCESS] {
            assert!(contrast(0xFFFFFF, fill) >= 4.5, "white on {fill:06X}");
        }
        for fill in [ACCENT, ACCENT_HOVER, ACCENT_PRESS] {
            assert!(contrast(ON_ACCENT, fill) >= 4.5, "{ON_ACCENT:06X} on {fill:06X}");
        }
        assert!(contrast(LOGO_ON_LIGHT, LIGHT.page) >= 3., "the mark on a light page");
    }

    // Graphite is the only neutral: a near-grey with a warm cast (the old ink-and-bone or plum ramps)
    // anywhere in src/ fails here. The waffle's stick colours are the mark's own material, so they stay.
    #[test]
    fn no_warm_neutrals_in_src() {
        const MARK_STICKS: [u32; 3] = [0x8C7A6A, 0xC9A27A, 0xC79A6B];
        let mut warm = Vec::new();
        for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/src")).unwrap() {
            let path = entry.unwrap().path();
            let Ok(source) = std::fs::read_to_string(&path) else { continue };
            for (i, line) in source.lines().enumerate() {
                for hex in line.split("0x").skip(1).filter_map(|rest| rest.get(..6)) {
                    let Ok(value) = u32::from_str_radix(hex, 16) else { continue };
                    let (_, chroma, hue) = to_oklch(value);
                    let hue = hue.to_degrees().rem_euclid(360.);
                    let is_warm = (0.005..=0.045).contains(&chroma) && !(110. ..290.).contains(&hue);
                    if is_warm && !MARK_STICKS.contains(&value) {
                        warm.push(format!("{}:{}: 0x{hex}", path.display(), i + 1));
                    }
                }
            }
        }
        assert!(warm.is_empty(), "warm neutrals left in src/:\n{}", warm.join("\n"));
    }

    #[test]
    fn code_colours_meet_aa_where_code_sits() {
        for (t, colours) in [(&DARK, &SYNTAX_DARK), (&LIGHT, &SYNTAX_LIGHT)] {
            for (name, ink) in colours {
                for surface in [t.raised, t.page] {
                    let ratio = contrast(*ink, surface);
                    assert!(ratio >= 4.5, "{name} {ink:06X} on {surface:06X} is {ratio:.2}:1");
                }
            }
        }
        for dark in [true, false] {
            let theme = super::highlight_theme(dark);
            for name in ["keyword", "comment", "comment.doc", "string.special", "punctuation.bracket"] {
                assert!(theme.style.syntax.style(name).and_then(|s| s.color).is_some(), "{name} has a colour");
            }
        }
    }

    #[test]
    fn desaturating_keeps_hue_and_lightness() {
        let honey: Hsla = rgb(ACCENT).into();
        let grey = desaturate(honey, 0.);
        assert_eq!((grey.h, grey.l, grey.s), (honey.h, honey.l, 0.));
        let soft = desaturate(honey, 0.3);
        assert!((soft.s - honey.s * 0.3).abs() < 1e-6 && soft.h == honey.h && soft.l == honey.l);
        assert_eq!(desaturate(honey, 1.), honey);
    }

    #[test]
    fn high_contrast_steps_secondary_text_up() {
        for (t, hc) in [(&DARK, &DARK_HC), (&LIGHT, &LIGHT_HC)] {
            assert!(contrast(hc.muted, t.sidebar) > contrast(t.muted, t.sidebar));
            assert!(contrast(hc.faint, t.sidebar) > contrast(t.faint, t.sidebar));
            assert!(hc.line == t.line_strong && hc.selected > t.selected);
        }
    }

    #[test]
    fn role_colours_stay_readable_in_both_themes() {
        for role in [0xFFFF00, 0x0000FF, 0xE67E22, 0x1ABC9C, 0xFFFFFF, 0x000001] {
            let ink = |dark| {
                let (l, c, h) = to_oklch(role);
                let l = if dark { l.clamp(0.72, 0.92) } else { l.clamp(0.30, 0.45) };
                from_oklch(l, c.min(0.16), h)
            };
            for surface in [DARK.page, DARK.sidebar] {
                assert!(contrast(ink(true), surface) >= 4.5, "role {role:06X} on dark {surface:06X}");
            }
            for surface in [LIGHT.page, LIGHT.sidebar] {
                assert!(contrast(ink(false), surface) >= 4.5, "role {role:06X} on light {surface:06X}");
            }
        }
    }
}
