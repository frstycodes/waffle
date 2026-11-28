// Icons page.
use super::*;

pub(super) fn icons(cx: &App) -> impl IntoElement {
    // The page's set, in its order, where the app has the glyph.
    let set = [
        Glyph::Hashtag,
        Glyph::VolumeHigh,
        Glyph::Mic,
        Glyph::MicOff,
        Glyph::Headphones,
        Glyph::ShareScreen,
        Glyph::Video,
        Glyph::Stream,
        Glyph::CallEnd,
        Glyph::Users,
        Glyph::Thread,
        Glyph::Pin,
        Glyph::Search,
        Glyph::Settings,
        Glyph::Emoji,
        Glyph::Gif,
        Glyph::EmojiAdd,
        Glyph::Plus,
        Glyph::Reply,
        Glyph::Trash,
        Glyph::Bell,
        Glyph::Game,
        Glyph::Crown,
    ];
    section("Icons", "Reicon's filled weight, like Discord; line-only glyphs (#, +, arrows) stay lines. The full set is design/icons.html.", cx)
        .child(board(cx).child(row().gap_4().text_color(palette::text(cx)).children(set.map(|g| Icon::new(g).size(px(MARK_SIZE))))))
}

pub(super) fn illustrations(cx: &App) -> impl IntoElement {
    let card = |el: Div| div().squircle(ui::RADIUS_LG).bg(palette::surface(Level::Page, cx)).child(el);
    section("Illustration / Art", "Polished stylised objects from Waffle's breakfast-and-café world; every piece reads on both surfaces.", cx).child(board(cx).child(row().gap_4().children(crate::illustration::Illustration::ALL.map(|art| card(div().child(gpui_kit::img(art.path()).w(px(ART_SIZE.0)).h(px(ART_SIZE.1))))))))
}
