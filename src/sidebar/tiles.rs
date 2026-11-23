// What the rail's tiles are made of: faces, folder trips, badges and their sortable lists.
use super::*;

pub(super) const MINI: f32 = 16.;

pub(super) const MINI_GAP: f32 = 4.;

pub(super) const MINI_INSET: f32 = (TILE - 2. * MINI - MINI_GAP) / 2.;

// A folder opening is one trip: the servers travel from their mini tiles to their slots, the folder's
// pill splits into theirs and its count hands over to theirs on the way. Closing runs it backwards.
pub(super) const TRIP: Duration = Duration::from_millis(260);

#[derive(Clone)]
pub(crate) enum Face {
    Home,
    Favorites,
    // A DM on the rail: the person's avatar, round like every person, or a group's.
    Person(crate::discord::DmFace),
    Icon(String),
    Letters(String),
    Folder { tint: Hsla, minis: Vec<Mini> },
}

// A server as a folder shows it: its icon, or its first letter.
#[derive(Clone)]
pub(crate) struct Mini {
    icon: Option<String>,
    letters: String,
}

pub fn initials(name: &str) -> String {
    name.split_whitespace().filter_map(|w| w.chars().next()).take(3).collect()
}

// RAIL

// A folder's trip this frame: 0 closed, 1 open.
pub(super) struct FolderMotion {
    pub(super) trip: f32,
}

impl FolderMotion {
    pub(super) fn frame(key: &str, open: bool, window: &mut Window, cx: &mut App) -> Self {
        let id = SharedString::from(key.to_owned());
        let trip = transition((id, "trip"), if open { 1. } else { 0. }, Tween::new(TRIP).ease(ease_out()), window, cx);
        Self { trip }
    }
}

// A rail tile's state pill: full when selected, longer on hover, a dot when unread.
pub(super) fn pill_height(selected: bool, hovered: bool, unread: bool) -> f32 {
    match (selected, hovered, unread) {
        (true, _, _) => 36.,
        (_, true, _) => 20.,
        (_, _, true) => 8.,
        _ => 0.,
    }
}

// A folder server's marks, which ride its tile on the trip.
pub(super) struct Badge {
    pub(super) pill: f32,
    pub(super) mentions: u32,
}

// --ease-out: cubic-bezier(0.23, 1, 0.32, 1).
pub(super) fn ease_out() -> impl Fn(f32) -> f32 + 'static {
    gpui_kit::base::animation::cubic_bezier(0.23, 1., 0.32, 1.)
}

// Where an open folder's `i`th server sits, from the folder's top.
pub(super) fn slot_y(i: usize) -> f32 {
    (i + 1) as f32 * (TILE + rail_gap())
}

// A server on its way between its mini tile and its slot. Past the fourth, servers have no mini and
// grow out of the last one. Its pill starts as the folder's (`head_pill`, on the folder's row) and slides
// with it to its own height; its count grows with the tile, so both land together.
#[allow(clippy::too_many_arguments)]
pub(super) fn flying_tile(mini: &Mini, badge: &Badge, head_pill: f32, i: usize, total: usize, t: f32, rail: Hsla, cx: &App) -> AnyElement {
    let (x, y) = mini_slot(i.min(3), total.min(4));
    let from = (TILE_X + x, y, if i < 4 { MINI } else { 0. });
    let to = (TILE_X, slot_y(i), TILE);
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    let size = lerp(from.2, to.2);
    let face = match &mini.icon {
        Some(url) => img(url.clone()).size(px(size)).squircle(lerp(ui::RADIUS_XS, ui::RADIUS_LG)).into_any_element(),
        None => letters_tile(&mini.letters, size, lerp(ui::RADIUS_XS, ui::RADIUS_LG), cx).into_any_element(),
    };
    let tile = div().absolute().left(px(lerp(from.0, to.0))).top(px(lerp(from.1, to.1))).child(ui::badged(face, size, ui::Shape::Tile, rail).count_scaled(badge.mentions, t));
    let pill = (badge.pill > 0.).then(|| {
        let height = lerp(head_pill, badge.pill);
        let center = lerp(TILE / 2., slot_y(i) + TILE / 2.);
        div().absolute().left_0().top(px(center - height / 2.)).w_1().h(px(height)).rounded_r_sm().bg(palette::strong(cx))
    });
    div().absolute().inset_0().children(pill).child(tile).into_any_element()
}

pub(crate) fn server_face(guild: &Guild) -> Face {
    match &guild.icon {
        Some(url) => Face::Icon(url.clone()),
        None => Face::Letters(initials(&guild.name)),
    }
}

pub(super) fn mini(guild: &Guild) -> Mini {
    Mini { icon: guild.icon.clone(), letters: initials(&guild.name) }
}

// A tile's face at `size`. `open` trades a folder's minis for the folder glyph.
pub(crate) fn render_face(face: Face, size: f32, radius: f32, selected: bool, open: f32, cx: &App) -> AnyElement {
    let tile = || ui::row().size(px(size)).flex_none().squircle(radius).justify_center();
    match face {
        Face::Icon(url) => img(url).size(px(size)).flex_none().squircle(radius).into_any_element(),
        Face::Person(face) => ui::dm_face(&face, size, crate::material::sidebar_fill(cx)),
        Face::Letters(label) => letters_tile(&label, size, radius, cx).into_any_element(),
        Face::Home => tile()
            .bg(if selected { palette::accent() } else { crate::material::lifted(palette::surface(palette::Level::Raised, cx), cx) })
            .child(svg().path(crate::icons::MARK).size(px(size / 2.)).text_color(if selected { palette::on_accent() } else { palette::logo_ink(cx) }))
            .into_any_element(),
        Face::Favorites => tile()
            .bg(if selected { palette::accent() } else { crate::material::lifted(palette::surface(palette::Level::Raised, cx), cx) })
            .text_color(if selected { palette::on_accent() } else { palette::warning_ink(cx) })
            .child(Icon::new(Glyph::Star).size(px(size / 2.)))
            .into_any_element(),
        Face::Folder { tint, minis } => tile()
            .relative()
            .bg(tint.opacity(0.24 * (1. - open)))
            .child(ui::row().absolute().inset_0().justify_center().opacity(open).text_color(tint).child(Icon::new(Glyph::Folder).size(px(FOLDER_GLYPH))))
            .when(open <= 0., |t| t.child(minis_grid(minis, cx)))
            .into_any_element(),
    }
}

// A closed folder's first four servers, laid out for how many there are (no empty slots).
pub(super) fn minis_grid(minis: Vec<Mini>, cx: &App) -> Div {
    let shown = minis.len().min(4);
    let cells = minis.into_iter().take(4).enumerate().map(|(i, mini)| {
        let (x, y) = mini_slot(i, shown);
        let face = match mini {
            Mini { icon: Some(url), .. } => img(url).size(px(MINI)).squircle(ui::RADIUS_XS).into_any_element(),
            Mini { letters, .. } => letters_tile(&letters, MINI, ui::RADIUS_XS, cx).into_any_element(),
        };
        div().absolute().left(px(x)).top(px(y)).child(face)
    });
    div().absolute().inset_0().children(cells)
}

// Where mini `i` of `shown` (1..=4) sits inside the folder tile: one centred, two side by side,
// three as two over one, four as a 2x2 grid.
pub(super) fn mini_slot(i: usize, shown: usize) -> (f32, f32) {
    let (centre, step) = ((TILE - MINI) / 2., MINI + MINI_GAP);
    match (shown, i) {
        (0 | 1, _) => (centre, centre),
        (2, i) => (MINI_INSET + i as f32 * step, centre),
        (3, 2) => (centre, MINI_INSET + step),
        (_, i) => (MINI_INSET + (i % 2) as f32 * step, MINI_INSET + (i / 2) as f32 * step),
    }
}

// A server with no icon: its initials on the sidebar tone. Minis keep only the first letter.
// lint-allow(custom-component): ui::server_tile sets initials on the selected fill; the rail's letters sit on the lifted raised tone.
pub(super) fn letters_tile(label: &str, size: f32, radius: f32, cx: &App) -> Div {
    let label: String = if size < 24. { label.chars().take(1).collect() } else { label.to_owned() };
    // lint-allow(custom-badge): ui::badge is a label; this is a server's letter tile, scaled with the rail tile.
    ui::row()
        .size(px(size))
        .flex_none()
        .squircle(radius)
        .bg(crate::material::lifted(palette::surface(palette::Level::Raised, cx), cx))
        .text_color(palette::strong(cx))
        // Minis keep a readable letter rather than scaling the tile's 14px down to nothing.
        .text_size(px((14. * size / TILE).max(8.)))
        .font_weight(FontWeight::SEMIBOLD)
        .justify_center()
        .child(label)
}

// The hairline between home, the servers and the actions.
// Add and Discover: a quiet tile that lifts to the raised tone on hover.
pub(super) fn rail_action(id: &'static str, icon: Icon, tooltip: &'static str, cx: &App) -> Stateful<Div> {
    let (raised, strong) = (crate::material::lifted(palette::surface(palette::Level::Raised, cx), cx), palette::strong(cx));
    // lint-allow(custom-button): ui::icon_button(...).sized(44) takes a Glyph and fills on hover; the rail's actions use Nav icons on the lifted raised tone.
    ui::row()
        .id(id)
        .size(px(TILE))
        .flex_none()
        .squircle(ui::RADIUS_LG)
        .justify_center()
        .cursor_pointer()
        .bg(raised)
        .text_color(palette::muted(cx))
        .hover(move |s| s.bg(raised).text_color(strong))
        .child(icon.size(ui::ICON_XL))
        .tooltip(move |window, cx| ui::tip(tooltip, window, cx))
        .tooltip_placement(TooltipPlacement::Side)
}

// What a server's icon badge shows, as Discord's GuildMediaStateStore decides it; the highest wins.
// GAP: Discord's stage, event and activity badges outrank these; we don't track those yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum MediaKind {
    Voice,
    Video,
    Screen,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Media {
    pub(super) kind: MediaKind,
    // You're in this voice: Discord's connected (green) style.
    pub(super) connected: bool,
}

// The badge for one server's voice states. `connected` is the channel you're in there, if any.
pub(super) fn media_state(states: &[crate::discord::VoiceState], me: &str, connected: Option<&str>, muted_server: bool, counts: impl Fn(&crate::discord::VoiceState) -> bool) -> Option<Media> {
    let pick = |screen: bool, video: bool, audio: bool| match (screen, video, audio) {
        (true, ..) => Some(MediaKind::Screen),
        (_, true, _) => Some(MediaKind::Video),
        (.., true) => Some(MediaKind::Voice),
        _ => None,
    };
    if let Some(channel) = connected {
        // Yours: your own stream, anyone's camera in your channel, or just the call.
        let screen = states.iter().any(|s| s.user_id == me && s.streaming);
        let video = states.iter().any(|s| s.channel_id.as_deref() == Some(channel) && s.camera);
        return pick(screen, video, true).map(|kind| Media { kind, connected: true });
    }
    if muted_server {
        return None;
    }
    let live: Vec<_> = states.iter().filter(|s| counts(s)).collect();
    pick(live.iter().any(|s| s.streaming), live.iter().any(|s| s.camera), !live.is_empty()).map(|kind| Media { kind, connected: false })
}

// The mark on a tile's top-right corner for its voice: dark with white ink in both themes, as Discord draws
// it; green while you're connected.
pub(super) fn media_mark(media: Media, cx: &App) -> (Icon, ui::MarkFill) {
    let icon = match media.kind {
        MediaKind::Voice => Icon::new(Glyph::VolumeHigh),
        MediaKind::Video => Nav::CameraFill.icon(),
        MediaKind::Screen => Icon::new(Glyph::ShareScreen),
    };
    let fill = if media.connected { palette::status_in(Status_::Online, cx) } else { palette::media_badge() };
    (icon, ui::MarkFill::Solid(fill))
}

// Going onto a tile (making or joining a folder): a ring around it. Reordering shows as the rail making room.
pub(super) fn combine_ring_mark() -> Div {
    // lint-allow(custom-mark): ui::badged marks a face's corner; this ring surrounds the whole tile.
    div().absolute().left(px(TILE_X - 4.)).top_neg_1().size(px(TILE + 8.)).squircle(ui::RADIUS_LG + 4.).border_2().border_color(palette::accent())
}

// The rail's own sortable list, and a folder's.
pub(super) const RAIL_LIST: &str = "rail";

pub(super) fn folder_list(id: &str) -> SharedString {
    format!("folder-list-{id}").into()
}

pub(super) fn rail_board(waffle: &mut Waffle) -> &mut Board {
    &mut waffle.rail_board
}

// `wanted` in its order, with each item of `before` that left put back right after the one it followed.
pub(super) fn keep_leaving<T>(mut wanted: Vec<T>, before: Vec<T>, same: impl Fn(&T, &T) -> bool) -> Vec<T> {
    let mut at = 0;
    for old in before {
        match wanted.iter().position(|w| same(w, &old)) {
            Some(i) => at = i + 1,
            None => {
                wanted.insert(at, old);
                at += 1;
            }
        }
    }
    wanted
}
