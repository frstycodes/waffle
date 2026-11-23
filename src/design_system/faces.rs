// The design-system page's badged faces: one face component with four corner slots, each mark placed for
// the face's shape and cut out of it in the surface's colour.
use super::*;
use crate::discord::Mark;

pub(super) fn badged_faces(cx: &App) -> impl IntoElement {
    let page = palette::surface(Level::Page, cx);
    let rail = palette::surface(Level::Rail, cx);
    let voice = ui::MarkFill::Solid(palette::media_badge());
    let live = ui::MarkFill::Solid(palette::status_in(Status::Online, cx));
    let tile = |name: &'static str| ui::server_tile(None, name, TILE, cx);
    let person = |name: &'static str| ui::monogram(name, ui::AVATAR_XL);
    section("Badged faces", "`ui::badged` puts marks on a face's corners: presence, a count, a voice or stream mark, a channel's kind. Each is placed for the face's shape and ringed in the surface, so marks never touch the face or each other.", cx).child(
        board(cx)
            .gap_6()
            .child(
                row()
                    .gap_6()
                    .bg(rail)
                    .p_4()
                    .child(ui::badged(tile("Night Shift"), TILE, ui::Shape::Tile, rail).count(3))
                    .child(ui::badged(tile("Lounge"), TILE, ui::Shape::Tile, rail).icon(ui::Spot::TopRight, Icon::new(Glyph::VolumeHigh), voice))
                    .child(ui::badged(tile("Arcade"), TILE, ui::Shape::Tile, rail).icon(ui::Spot::TopRight, Icon::new(Glyph::ShareScreen), live).count(120))
                    .child(ui::badged(tile("Forum"), TILE, ui::Shape::Tile, rail).icon(ui::Spot::BottomRight, Icon::new(Glyph::Hashtag), ui::MarkFill::Surface))
                    .child(note("Tile: count bottom-right, voice top-right, a channel's kind", cx)),
            )
            .child(
                row()
                    .gap_6()
                    .child(ui::badged(person("Mira"), ui::AVATAR_XL, ui::Shape::Round, page).status(Some(Mark::Mobile)))
                    .child(ui::badged(person("Ana"), ui::AVATAR_XL, ui::Shape::Round, page).status(Some(Mark::Streaming)))
                    .child(ui::badged(person("Theo"), ui::AVATAR_XL, ui::Shape::Round, page).status(Some(Status::Idle)).icon(ui::Spot::TopRight, Icon::new(Glyph::VolumeHigh), voice))
                    .child(ui::badged(person("Jun"), ui::AVATAR_XL, ui::Shape::Round, page).mark(ui::Spot::TopLeft, ui::badge_of(ui::unread_dot())).mark(ui::Spot::BottomLeft, ui::badge_of(ui::unread_dot())).count(4))
                    .child(note("Round: presence bottom-right, any mark in any corner", cx)),
            ),
    )
}

// The rail's tile size, which the badge offsets are drawn for.
const TILE: f32 = 44.;

// Avatars at every size, and presence in each of its shapes.
pub(super) fn avatars(cx: &App) -> impl IntoElement {
    let ring = palette::surface(Level::Sidebar, cx);
    let page = palette::surface(Level::Page, cx);
    let sized = |size: f32| {
        let mark = ui::monogram("Mira", size);
        if size < 32. {
            return div().child(mark);
        }
        div().child(ui::badged(mark, size, ui::Shape::Round, page).status(Some(Status::Online)))
    };
    let person = |name: &'static str, label: &'static str, status: Option<Status>| {
        let avatar = match status {
            Some(status) => ui::badged(ui::monogram(name, 40.), 40., ui::Shape::Round, ring).status(Some(status)).into_any_element(),
            None => ui::monogram(name, 40.).shadow(ui::speaking_ring(ring)).into_any_element(),
        };
        row().child(avatar).child(div().text_color(palette::muted(cx)).child(label))
    };
    let more = ui::badge(palette::surface(Level::Overlay, cx), palette::strong(cx)).size_6().rounded_full().px_0().justify_center().child("+3");
    section("Avatars and status", "Status is never colour alone: online is a dot, idle a moon, do not disturb a bar, offline a ring. Speaking is the accent ring plus the bars.", cx).child(
        board(cx)
            .gap_6()
            .child(row().gap_4().items_end().children([16., 20., 24., 32., 40., 56., 80.].map(sized)).child(note("16 20 24 32 40 56 80 · below 24 no initials, no status", cx)))
            .child(
                row()
                    .gap_6()
                    .child(person("Mira", "Online", Some(Status::Online)))
                    .child(person("Ana", "Idle", Some(Status::Idle)))
                    .child(person("Theo", "Do not disturb", Some(Status::Dnd)))
                    .child(person("Jun", "Offline", Some(Status::Offline)))
                    .child(person("Rosa", "Speaking", None)),
            )
            .child(
                row()
                    .gap_3()
                    .child(ui::stacked([ui::monogram("Mira", 24.).into_any_element(), ui::monogram("Theo", 24.).into_any_element(), ui::monogram("Ana", 24.).into_any_element(), more.into_any_element()], 24., ring))
                    .child(div().text_color(palette::muted(cx)).child("6 watching")),
            ),
    )
}
