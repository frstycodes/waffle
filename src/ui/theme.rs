// shadcn/ui's colour variables over Waffle's palette (design/tokens.css has the same aliases). Honey is
// `primary`. Surface levels: Page → background, Raised → card, Overlay → popover, Sidebar → sidebar; Rail has
// no shadcn name and stays `palette::Level::Rail`. shadcn's `accent` is the hover fill, which collides with
// Waffle's honey `palette::accent`, so it is `accent_wash` here.
use gpui_kit::{App, Hsla};

use super::RADIUS_MD;
use crate::palette::{self, Level};

pub const RADIUS: f32 = RADIUS_MD;

// shadcn's `--background`: the page.
// Keywords: background, shadcn, theme token, colour role, css variable
pub fn background(cx: &App) -> Hsla {
    palette::surface(Level::Page, cx)
}

// shadcn's `--foreground`: the ink on the page.
// Keywords: foreground, shadcn, theme token, colour role, css variable
pub fn foreground(cx: &App) -> Hsla {
    palette::text(cx)
}

// shadcn's `--card`: a raised card.
// Keywords: card, shadcn, theme token, colour role, css variable
pub fn card(cx: &App) -> Hsla {
    palette::surface(Level::Raised, cx)
}

// shadcn's `--card-foreground`: the ink on a card.
// Keywords: card foreground, shadcn, theme token, colour role, css variable
pub fn card_foreground(cx: &App) -> Hsla {
    palette::text(cx)
}

// shadcn's `--popover`: a floating layer.
// Keywords: popover, shadcn, theme token, colour role, css variable
pub fn popover(cx: &App) -> Hsla {
    palette::surface(Level::Overlay, cx)
}

// shadcn's `--popover-foreground`: the ink on a floating layer.
// Keywords: popover foreground, shadcn, theme token, colour role, css variable
pub fn popover_foreground(cx: &App) -> Hsla {
    palette::text(cx)
}

// shadcn's `--primary`: the honey action fill.
// Keywords: primary, shadcn, theme token, colour role, css variable
pub fn primary() -> Hsla {
    palette::accent()
}

// shadcn's `--primary-foreground`: the ink on the honey fill.
// Keywords: primary foreground, shadcn, theme token, colour role, css variable
pub fn primary_foreground() -> Hsla {
    palette::on_accent()
}

// shadcn's `--secondary`: the quiet fill.
// Keywords: secondary, shadcn, theme token, colour role, css variable
pub fn secondary(cx: &App) -> Hsla {
    palette::selected(cx)
}

// shadcn's `--secondary-foreground`: the ink on the quiet fill.
// Keywords: secondary foreground, shadcn, theme token, colour role, css variable
pub fn secondary_foreground(cx: &App) -> Hsla {
    palette::strong(cx)
}

// shadcn's `--muted`: a soft band, a settings group.
// Keywords: muted, shadcn, theme token, colour role, css variable
pub fn muted(cx: &App) -> Hsla {
    palette::surface(Level::Group, cx)
}

// shadcn's `--muted-foreground`: secondary and descriptive text.
// Keywords: muted foreground, shadcn, theme token, colour role, css variable
pub fn muted_foreground(cx: &App) -> Hsla {
    palette::muted(cx)
}

// shadcn's `--accent-wash`: the hover wash (shadcn calls it accent; honey is primary here).
// Keywords: accent wash, shadcn, theme token, colour role, css variable
pub fn accent_wash(cx: &App) -> Hsla {
    palette::hover(cx)
}

// shadcn's `--accent-wash-foreground`: the ink on the hover wash.
// Keywords: accent wash foreground, shadcn, theme token, colour role, css variable
pub fn accent_wash_foreground(cx: &App) -> Hsla {
    palette::strong(cx)
}

// shadcn's `--destructive`: the destructive fill.
// Keywords: destructive, shadcn, theme token, colour role, css variable
pub fn destructive() -> Hsla {
    palette::danger()
}

// shadcn's `--destructive-foreground`: the ink on the destructive fill.
// Keywords: destructive foreground, shadcn, theme token, colour role, css variable
pub fn destructive_foreground() -> Hsla {
    gpui_kit::white()
}

// shadcn's `--border`: a hairline.
// Keywords: border, shadcn, theme token, colour role, css variable
pub fn border(cx: &App) -> Hsla {
    palette::line(cx)
}

// shadcn's `--input`: a field's edge.
// Keywords: input, shadcn, theme token, colour role, css variable
pub fn input(cx: &App) -> Hsla {
    palette::line_strong(cx)
}

// shadcn's `--ring`: the focus ring.
// Keywords: ring, shadcn, theme token, colour role, css variable
pub fn ring() -> Hsla {
    palette::accent()
}

pub mod sidebar {
    use gpui_kit::{App, Hsla};

    use crate::palette::{self, Level};

    // shadcn's `--background`: the page.

    // Keywords: background, shadcn, theme token, colour role, css variable

    pub fn background(cx: &App) -> Hsla {
        palette::surface(Level::Sidebar, cx)
    }

    // shadcn's `--foreground`: the ink on the page.

    // Keywords: foreground, shadcn, theme token, colour role, css variable

    pub fn foreground(cx: &App) -> Hsla {
        palette::text(cx)
    }

    // shadcn's `--primary`: the honey action fill.

    // Keywords: primary, shadcn, theme token, colour role, css variable

    pub fn primary() -> Hsla {
        palette::accent()
    }

    // shadcn's `--primary-foreground`: the ink on the honey fill.

    // Keywords: primary foreground, shadcn, theme token, colour role, css variable

    pub fn primary_foreground() -> Hsla {
        palette::on_accent()
    }

    pub fn accent(cx: &App) -> Hsla {
        palette::hover(cx)
    }

    pub fn accent_foreground(cx: &App) -> Hsla {
        palette::strong(cx)
    }

    // shadcn's `--border`: a hairline.

    // Keywords: border, shadcn, theme token, colour role, css variable

    pub fn border(cx: &App) -> Hsla {
        palette::line(cx)
    }

    // shadcn's `--ring`: the focus ring.

    // Keywords: ring, shadcn, theme token, colour role, css variable

    pub fn ring() -> Hsla {
        palette::accent()
    }
}
