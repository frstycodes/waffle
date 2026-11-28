// Identity: Avatar, Presence mark, User avatar, Server tile, App tile, Avatar stack, List row.
use super::*;
use crate::discord::Mark;

// The rail's tile size, which the badge offsets are drawn for.
const TILE: f32 = 44.;
const MORPH_STEP: Duration = Duration::from_millis(1200);
const MORPH_MARKS: [(Mark, &str); 7] = [
    (Mark::Status(Status::Online), "Online"),
    (Mark::Status(Status::Idle), "Idle"),
    (Mark::Status(Status::Dnd), "Do not disturb"),
    (Mark::Status(Status::Offline), "Offline"),
    (Mark::Mobile, "Mobile"),
    (Mark::Streaming, "Streaming"),
    (Mark::Typing, "Typing"),
];

fn variant(title: &'static str, lede_text: &'static str, content: Div, cx: &App) -> Frame {
    (Kind::Variants, section(title, lede_text, cx).child(board(cx).child(content)).into_any_element())
}

pub(super) fn avatar_frames(cx: &App) -> Vec<Frame> {
    let sizes = row().gap_4().items_end().children([16., 20., 24., 32., 40., 56., 80., 120.].map(|size| ui::AvatarFallback::new("Mira", size))).child(note("16 20 24 32 40 56 80 120 · below 24 no initials", cx));
    let hues = row().gap_4().children(["Mira", "Theo", "Ana", "Jun", "Kai", "Lu", "Ivy", "Rosa"].map(|name| ui::AvatarFallback::new(name, ui::AVATAR_XL))).child(note("The hue is a stable hash of the name", cx));
    vec![variant("Avatar / Size", "A photo, or the name's initial on a gradient hued by the name.", ui::col().gap_6().child(sizes).child(hues), cx)]
}

pub(super) fn presence_frames(window: &mut Window, cx: &mut App) -> Vec<Frame> {
    let ring = palette::surface(Level::Sidebar, cx);
    let person = |name: &'static str, label: &'static str, status: Option<Mark>| {
        let avatar = match status {
            Some(status) => ui::badged(ui::AvatarFallback::new(name, 40.), 40., ui::Shape::Round, ring).status(Some(status)).into_any_element(),
            None => ui::AvatarFallback::new(name, 40.).shadow(ui::speaking_ring(ring)).into_any_element(),
        };
        row().child(avatar).child(div().text_color(palette::muted(cx)).child(label))
    };
    let all = row()
        .gap_6()
        .child(person("Mira", "Online", Some(Status::Online.into())))
        .child(person("Ana", "Idle", Some(Status::Idle.into())))
        .child(person("Theo", "Do not disturb", Some(Status::Dnd.into())))
        .child(person("Jun", "Offline", Some(Status::Offline.into())))
        .child(person("Kai", "Mobile", Some(Mark::Mobile)))
        .child(person("Lu", "Streaming", Some(Mark::Streaming)))
        .child(person("Ivy", "Typing", Some(Mark::Typing)))
        .child(person("Rosa", "Speaking", None));
    vec![
        variant("Presence mark / Status", "Status is never colour alone: online is a dot, idle a moon, do not disturb a bar, offline a ring, a phone for mobile, a play disc while streaming, a pill of dots while typing.", all, cx),
        variant("Presence mark / Morph", "One face stepping through every status every 1.2s, so the morph between marks can be watched.", morphing(window, cx), cx),
    ]
}

fn morphing(window: &mut Window, cx: &mut App) -> Div {
    let started = *window.use_keyed_state("ds-morph-start", cx, |_, _| Instant::now()).read(cx);
    let step = (started.elapsed().as_millis() / MORPH_STEP.as_millis()) as usize % MORPH_MARKS.len();
    // lint-allow(endless-animation): drawn only on the design-system page, which exists to show it moving.
    window.request_animation_frame();
    let (mark, label) = MORPH_MARKS[step];
    let face = |size: f32| ui::badged(ui::AvatarFallback::new("Mira", size), size, ui::Shape::Round, palette::surface(Level::Page, cx)).status(Some(mark)).morph(("ds-morph", size as usize));
    row().gap_6().child(face(32.)).child(face(80.)).child(face(120.)).child(note(label, cx))
}

pub(super) fn user_avatar_frames(cx: &App) -> Vec<Frame> {
    let page = palette::surface(Level::Page, cx);
    let voice = ui::MarkFill::Solid(palette::media_badge());
    let person = |name: &'static str| ui::AvatarFallback::new(name, ui::AVATAR_XL);
    let marks = row()
        .gap_6()
        .child(ui::badged(person("Mira"), ui::AVATAR_XL, ui::Shape::Round, page).status(Some(Mark::Mobile)))
        .child(ui::badged(person("Ana"), ui::AVATAR_XL, ui::Shape::Round, page).status(Some(Mark::Streaming)))
        .child(ui::badged(person("Theo"), ui::AVATAR_XL, ui::Shape::Round, page).status(Some(Status::Idle)).icon(ui::Spot::TopRight, Icon::new(Glyph::VolumeHigh), voice))
        .child(ui::badged(person("Jun"), ui::AVATAR_XL, ui::Shape::Round, page).mark(ui::Spot::TopLeft, ui::avatar_badge(ui::Badge::dot())).mark(ui::Spot::BottomLeft, ui::avatar_badge(ui::Badge::dot())).count(3))
        .child(ui::badged(person("Kai"), ui::AVATAR_XL, ui::Shape::Round, page).mark(ui::Spot::TopRight, ui::avatar_badge(ui::Badge::dot()).bare()))
        .child(note("Round: presence bottom-right, any mark in any corner; a bare mark skips the ring", cx));
    vec![variant("User avatar / Spot", "`ui::badged` puts marks on a face's corners; each is placed for the face's shape and ringed in the surface, so marks never collide with the edge.", ui::col().child(marks), cx)]
}

pub(super) fn server_tile_frames(cx: &App) -> Vec<Frame> {
    let rail = palette::surface(Level::Rail, cx);
    let voice = ui::MarkFill::Solid(palette::media_badge());
    let live = ui::MarkFill::Solid(palette::status_in(Status::Online, cx));
    let tile = |name: &'static str| ui::server_tile(None, name, TILE, cx);
    let marks = row()
        .gap_6()
        .bg(rail)
        .p_4()
        .child(ui::badged(tile("Night Shift"), TILE, ui::Shape::Tile, rail).count(3))
        .child(ui::badged(tile("Lounge"), TILE, ui::Shape::Tile, rail).icon(ui::Spot::TopRight, Icon::new(Glyph::VolumeHigh), voice))
        .child(ui::badged(tile("Arcade"), TILE, ui::Shape::Tile, rail).icon(ui::Spot::TopRight, Icon::new(Glyph::ShareScreen), live).count(120))
        .child(ui::badged(tile("Forum"), TILE, ui::Shape::Tile, rail).icon(ui::Spot::BottomRight, Icon::new(Glyph::Hashtag), ui::MarkFill::Surface))
        .child(note("Count bottom-right, voice top-right, a channel's kind", cx));
    let sizes = row().gap_4().items_end().children([ui::AVATAR_SM, ui::AVATAR_MD, ui::AVATAR_LG, ui::AVATAR_XL, TILE].map(|size| ui::server_tile(None, "Night Shift", size, cx))).child(note("Initials on the selected fill when there is no icon", cx));
    vec![variant("Server tile / Size", "A server's icon as a squircle at any size, or its initials.", ui::col().child(sizes), cx), variant("Server tile / Marks", "On the rail, over its own surface.", ui::col().child(marks), cx)]
}

pub(super) fn app_tile_frames(cx: &App) -> Vec<Frame> {
    let tiles = row().gap_4().children([("Wumpus Quest", "a"), ("Song Quiz", "b"), ("Night Chess", "c"), ("Pixel Party", "d")].map(|(name, id)| ui::app_tile(None, name, id, ui::AVATAR_XL, cx))).child(note("Without an icon: the initial on a gradient hued by the app's id", cx));
    vec![variant("App tile / Icon", "An app keeps its colour from its id.", ui::col().child(tiles), cx)]
}

pub(super) fn avatar_stack_frames(cx: &App) -> Vec<Frame> {
    let ring = palette::surface(Level::Sidebar, cx);
    let faces = |names: &[&str]| names.iter().map(|n| ui::AvatarFallback::new(n, 24.).into_any_element()).collect::<Vec<_>>();
    let more = || ui::Badge::empty().colors(palette::surface(Level::Overlay, cx), palette::strong(cx)).size_6().rounded_full().px_0().justify_center().child("+3").into_any_element();
    let mut four = faces(&["Mira", "Theo", "Ana"]);
    four.push(more());
    let content = row()
        .gap_6()
        .child(ui::AvatarGroup::stacked(faces(&["Mira"]), 24., ring))
        .child(ui::AvatarGroup::stacked(faces(&["Mira", "Theo"]), 24., ring))
        .child(row().child(ui::AvatarGroup::stacked(four, 24., ring)).child(div().text_color(palette::muted(cx)).child("6 watching")));
    vec![variant("Avatar stack / Count", "Overlapping avatars of one size, each ringed in the surface's colour; past the cap the last slot becomes +n.", ui::col().child(content), cx)]
}

pub(super) fn list_row_frames(cx: &App) -> Vec<Frame> {
    let page = palette::surface(Level::Page, cx);
    let face = |name: &'static str| ui::AvatarFallback::new(name, ui::AVATAR_XL);
    let rows = ui::col()
        .w_80()
        .child(ui::Item::new("ds-row-1", "Mira", cx).lead(ui::badged(face("Mira"), ui::AVATAR_XL, ui::Shape::Round, page).status(Some(Status::Online))).subtitle("Playing Waffle Quest").height(ui::RowHeight::Tall))
        .child(ui::Item::new("ds-row-2", "Theo", cx).lead(face("Theo")).subtitle("Idle").trailing(ui::Button::new("ds-row-msg").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconXs).icon(Glyph::ChatRound).tooltip_text("Message")))
        .child(ui::Item::new("ds-row-3", "Picked", cx).height(ui::RowHeight::Dense).control(|c| c.toggled(true, cx)));
    let stamped = ui::col()
        .w_96()
        .child(ui::Item::new("ds-row-stamp", "Mira", cx).height(ui::RowHeight::Fit).top().lead(ui::AvatarFallback::new("Mira", ui::AVATAR_SM)).stamp("12:04").body(div().text_color(palette::text(cx)).child("Two lines of the message, wrapping under the name.")))
        .child(ui::Item::new("ds-row-ink", "Playing now", cx).height(ui::RowHeight::Dense).title_ink(palette::accent_ink(cx)));
    vec![variant("List row / Height", "Lead, name, a muted line, trailing actions; it fills on hover.", ui::col().gap_6().child(rows).child(stamped), cx)]
}
