// The design-system page's foundations: the type scale and the spacing steps, drawn from the tokens.
use super::*;

pub(super) fn type_scale(cx: &App) -> impl IntoElement {
    let spec = |meta: String, first: bool, text: gpui_kit::Div| {
        div()
            .flex()
            .items_baseline()
            .gap_4()
            .py_3()
            .when(!first, |d| d.border_t_1().border_color(palette::line(cx)))
            .child(div().w(px(META_WIDTH)).flex_none().font_family(cx.theme().mono_font_family.clone()).text_size(ui::TEXT_XS).text_color(palette::faint(cx)).child(meta))
            .child(text)
    };
    let (strong, text, faint) = (palette::strong(cx), palette::text(cx), palette::faint(cx));
    let sample = |size: Pixels, leading: f32, weight: FontWeight, ink: Hsla, words: &'static str| div().text_size(size).line_height(relative(leading)).font_weight(weight).text_color(ink).child(words);
    let rows = [
        ("TEXT_XL", ui::TEXT_XL, ui::LEADING_TIGHT, FontWeight::SEMIBOLD, strong, "Welcome to Night Shift"),
        ("TEXT_LG", ui::TEXT_LG, ui::LEADING_TIGHT, FontWeight::SEMIBOLD, strong, "Voice and video"),
        ("TEXT_MD", ui::TEXT_MD, ui::LEADING_BODY, FontWeight::NORMAL, text, "anyone up for a few rounds after dinner? bringing snacks this time, promise"),
        ("TEXT_SM", ui::TEXT_SM, ui::LEADING_BODY, FontWeight::NORMAL, text, "general · looking-for-group · Lounge"),
        ("TEXT_XS", ui::TEXT_XS, ui::LEADING_BODY, FontWeight::MEDIUM, faint, "Today at 9:41 PM · Online · 3"),
        ("TEXT_XXS", ui::TEXT_XXS, ui::LEADING_TIGHT, FontWeight::SEMIBOLD, faint, "NEW · HOST · APP"),
    ];
    section("Type", "One family for the interface, a mono for code. Sizes are `ui::TEXT_*`, line heights `ui::LEADING_*`.", cx).child(
        board(cx).gap_0().children(rows.into_iter().enumerate().map(|(i, (name, size, leading, weight, ink, words))| {
            spec(format!("{name} · {} · {leading}", f32::from(size)), i == 0, sample(size, leading, weight, ink, words))
        })),
    )
}

// Drawn from the tokens themselves, so this page can't drift from what the app uses.
pub(super) fn space(cx: &App) -> impl IntoElement {
    let bar = |(name, n): (&'static str, f32)| {
        div()
            .flex()
            .items_center()
            .gap_3()
            .text_size(ui::TEXT_XS)
            .text_color(palette::faint(cx))
            .font_family(cx.theme().mono_font_family.clone())
            .child(div().w(px(STEP_LABEL)).child(format!("{name} · {n}")))
            .child(div().w(px(n * 3.)).h_3().squircle(3.).bg(palette::accent().opacity(0.8)))
    };
    let tile = |name: &'static str, radius: f32, use_: &'static str| {
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap_1p5()
            .text_size(ui::TEXT_XS)
            .child(div().size(px(RADIUS_SAMPLE)).squircle(radius).bg(palette::surface(Level::Raised, cx)).border(px(SAMPLE_EDGE)).border_color(palette::line_strong(cx)))
            .child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).font_family(cx.theme().mono_font_family.clone()).child(format!("{name} · {radius}")))
            .child(div().text_color(palette::faint(cx)).child(use_))
    };
    let sizes = |label: &'static str, marks: Vec<AnyElement>| stack().child(note(label, cx)).child(row().gap_4().items_end().children(marks));
    let icon_sizes = [ui::ICON_XS, ui::ICON_SM, ui::ICON_MD, ui::ICON_LG, ui::ICON_XL].map(|size| Icon::new(Glyph::Pin).size(size).into_any_element()).into();
    let avatar_sizes = [ui::AVATAR_XS, ui::AVATAR_SM, ui::AVATAR_MD, ui::AVATAR_LG, ui::AVATAR_XL].map(|size| ui::monogram("Mira", size).into_any_element()).into();
    section("Space and shape", "Spacing is gpui's own scale (`gap_1` 4px … `p_12` 48px; the rem is fixed at 16px). Corners are `ui::RADIUS_*` through `squircle`; nested corners stay concentric.", cx).child(
        div()
            .grid()
            .grid_cols(3)
            .gap_4()
            .child(board(cx).gap_1p5().children([("0p5", 2.), ("1", 4.), ("1p5", 6.), ("2", 8.), ("2p5", 10.), ("3", 12.), ("3p5", 14.), ("4", 16.), ("6", 24.), ("8", 32.), ("12", 48.)].map(bar)))
            .child(
                board(cx).child(
                    row()
                        .gap_4()
                        .items_start()
                        .child(tile("RADIUS_XS", ui::RADIUS_XS, "Tags, tooltips"))
                        .child(tile("RADIUS_SM", ui::RADIUS_SM, "Rows, menu items"))
                        .child(tile("RADIUS_MD", ui::RADIUS_MD, "Buttons, inputs"))
                        .child(tile("RADIUS_LG", ui::RADIUS_LG, "Cards, tiles"))
                        .child(tile("RADIUS_XL", ui::RADIUS_XL, "Dialogs")),
                ),
            )
            .child(board(cx).child(sizes("ICON_XS … ICON_XL", icon_sizes)).child(sizes("AVATAR_XS … AVATAR_XL", avatar_sizes))),
    )
}

// The specimen's columns: a type style's metadata, a spacing step's label, and a radius sample with its edge.
const META_WIDTH: f32 = 140.;
const STEP_LABEL: f32 = 70.;
const RADIUS_SAMPLE: f32 = 76.;
const SAMPLE_EDGE: f32 = 1.5;
