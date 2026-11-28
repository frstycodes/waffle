// Colour page: surfaces, ink and accent, with live contrast.
use super::*;

pub(super) fn colour(cx: &App) -> impl IntoElement {
    let page = palette::surface(Level::Page, cx);
    let surface = |name: &'static str, token: &'static str, level: Level| {
        ui::col().min_h_0().flex_1().h(px(SWATCH_HEIGHT)).p_2p5().bg(palette::surface(level, cx)).justify_end().text_size(ui::TEXT_XS).text_color(palette::muted(cx)).child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(name)).child(token)
    };
    let ink = |name: &'static str, color: Hsla, use_: &'static str| {
        ui::row()
            .justify_between()
            .items_baseline()
            .gap_4()
            .px_3()
            .py_2()
            .squircle(ui::RADIUS_SM)
            .bg(page)
            .child(ui::row().items_stretch().gap_1().child(div().font_weight(FontWeight::MEDIUM).text_color(color).child(name)).child(div().text_color(palette::faint(cx)).child(format!("· {use_}"))))
            .child(pass(color, page, cx))
    };
    let swatch = |token: &'static str, name: &'static str, color: Hsla, white_text: bool| {
        div()
            .squircle(ui::RADIUS_MD)
            .border_1()
            .border_color(palette::line(cx))
            .bg(palette::surface(Level::Raised, cx))
            .child(edge_top(ui::row().h_16().p_2().bg(color).items_end().justify_end().when(white_text, |d| d.child(div().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(gpui_kit::white()).child(ratio(gpui_kit::white(), color))))))
            .child(div().px_2p5().py_2().text_size(ui::TEXT_XS).child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(token)).child(div().text_color(palette::faint(cx)).child(name)))
    };
    // shadcn's role pairs: the surface with "Aa" in its foreground, the token names and the measured contrast.
    let role = |name: &'static str, fill: Hsla, ink: Hsla, fill_token: &'static str, ink_token: &'static str| {
        ui::col()
            .gap_1()
            .p_3()
            .squircle(ui::RADIUS_MD)
            .border_1()
            .border_color(palette::line(cx))
            .bg(fill)
            .text_color(ink)
            .child(div().text_size(ui::TEXT_LG).font_weight(FontWeight::SEMIBOLD).child("Aa"))
            .child(div().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).child(name))
            .child(div().text_size(ui::TEXT_XXS).opacity(0.8).child(format!("{fill_token} / {ink_token}")))
            .child(div().text_size(ui::TEXT_XXS).opacity(0.8).child(ratio(ink, fill)))
    };
    let roles = div()
        .grid()
        .grid_cols(3)
        .gap_3()
        .child(role("Background", ui::theme::background(cx), ui::theme::foreground(cx), "background", "foreground"))
        .child(role("Card", ui::theme::card(cx), ui::theme::card_foreground(cx), "card", "card-foreground"))
        .child(role("Popover", ui::theme::popover(cx), ui::theme::popover_foreground(cx), "popover", "popover-foreground"))
        .child(role("Primary", ui::theme::primary(), ui::theme::primary_foreground(), "primary", "primary-foreground"))
        .child(role("Secondary", ui::theme::secondary(cx).blend(ui::theme::background(cx)), ui::theme::secondary_foreground(cx), "secondary", "secondary-foreground"))
        .child(role("Muted", ui::theme::muted(cx), ui::theme::muted_foreground(cx), "muted", "muted-foreground"))
        .child(role("Accent wash (shadcn accent)", ui::theme::accent_wash(cx).blend(ui::theme::background(cx)), ui::theme::accent_wash_foreground(cx), "accent-wash", "accent-wash-foreground"))
        .child(role("Destructive", ui::theme::destructive(), ui::theme::destructive_foreground(), "destructive", "destructive-foreground"))
        .child(role("Sidebar", ui::theme::sidebar::background(cx), ui::theme::sidebar::foreground(cx), "sidebar", "sidebar-foreground"));
    section("Colour", "Restrained: Graphite neutrals plus one honey. Depth in the dark theme comes from lighter surfaces, never shadows. Contrast figures are measured live against the page.", cx)
        .child(h3("Roles: a surface and the ink that sits on it", cx))
        .child(roles)
        .child(h3("Surfaces, deepest to highest", cx))
        .child(
            ui::row().items_stretch()
                .squircle(ui::RADIUS_LG)
                .border_1()
                .border_color(palette::line(cx))
                .child(surface("Rail", "--bg-rail", Level::Rail).rounded_l(px(ui::RADIUS_LG - 1.)).corner_shape(ui::SQUIRCLE))
                .child(surface("Sidebar", "--bg-sidebar", Level::Sidebar))
                .child(surface("Page", "--bg-page", Level::Page))
                .child(surface("Raised", "--bg-raised", Level::Raised))
                .child(surface("Overlay", "--bg-overlay", Level::Overlay).rounded_r(px(ui::RADIUS_LG - 1.)).corner_shape(ui::SQUIRCLE)),
        )
        .child(h3("Text", cx))
        .child(
            board(cx)
                .gap_1p5()
                .p_3()
                .child(ink("Strong", palette::strong(cx), "Names, titles, selected rows"))
                .child(ink("Default", palette::text(cx), "Message body"))
                .child(ink("Muted", palette::muted(cx), "Channel rows, descriptions"))
                .child(ink("Faint", palette::faint(cx), "Timestamps, placeholders"))
                .child(ink("Accent ink", palette::accent_ink(cx), "Links, mentions"))
                .child(ink("Danger ink", palette::danger_ink(cx), "Errors, destructive rows")),
        )
        .child(h3("Accent and status", cx))
        .child(
            div()
                .grid()
                .grid_cols(4)
                .gap_4()
                .child(swatch("--accent", "Accent · Primary buttons, counts, live", palette::accent(), true))
                .child(swatch("--accent-hover", "Accent hover", palette::accent_hover(), true))
                .child(swatch("--logo", "Logo · The mark only", palette::logo(), true))
                .child(swatch("--danger", "Danger · Leave, delete", palette::danger(), true))
                .child(swatch("--online", "Online · Dot, voice connected", palette::status_in(Status::Online, cx), false))
                .child(swatch("--idle", "Idle · Moon", palette::status_in(Status::Idle, cx), false))
                .child(swatch("--dnd", "Do not disturb · Bar", palette::status_in(Status::Dnd, cx), false))
                .child(swatch("--offline", "Offline · Ring", palette::status_in(Status::Offline, cx), false)),
        )
}

pub(super) fn edge_top(el: gpui_kit::Div) -> gpui_kit::Div {
    ui::edge(el, ui::top(ui::RADIUS_MD - 1.))
}

pub(super) fn h3(text: &'static str, cx: &App) -> gpui_kit::Div {
    div().pt_6().pb_3().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(text)
}

// "4.62:1 AA", the way the page labels each ink against what it sits on.
pub(super) fn pass(fg: Hsla, bg: Hsla, cx: &App) -> impl IntoElement {
    div().font_family(cx.theme().mono_font_family.clone()).text_size(ui::TEXT_XS).text_color(palette::faint(cx)).child(ratio(fg, bg))
}

pub(super) fn ratio(fg: Hsla, bg: Hsla) -> String {
    let r = palette::contrast(fg, bg);
    let grade = if r >= 4.5 { "AA" } else if r >= 3. { "AA large" } else { "fails" };
    format!("{r:.2}:1 {grade}")
}
