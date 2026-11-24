// The pieces every profile surface shares: its colours, the name block, badges, bio, sections and dates.
use gpui_kit::{
    AnimationExt, AnyElement, App, ClickEvent, Context, Div, FontWeight, Hsla, MouseButton, SharedString, Stateful, StyledText, Window,
    component::{ActiveTheme, text::TextView},
    div, img, linear_color_stop, linear_gradient, prelude::*, px, relative,
};

use super::{BORDER, SKELETON_BADGES};
use crate::{
    Waffle,
    discord::{Badge, Profile, User},
    icons::Glyph,
    palette,
    ui::{self, RADIUS_LG, RADIUS_MD, RADIUS_XS, Squircle},
    user_menu::Surface,
};

// The guild tag chip's height, a badge's size.
const TAG_HEIGHT: f32 = 22.;
const BADGE_SIZE: f32 = 20.;
const MAX_BADGES: usize = 6;

// The card's colours. Themed: the user's two colours washed toward the theme's ground for the body,
// raw for the border, with ink picked by the body's luminance. Otherwise the plain overlay surface.
#[derive(Clone, Copy)]
pub(crate) struct Ink {
    pub(crate) top: Hsla,
    pub(crate) bottom: Hsla,
    pub(crate) border: Option<(Hsla, Hsla)>,
    pub(crate) strong: Hsla,
    pub(crate) text: Hsla,
    pub(crate) muted: Hsla,
    pub(crate) tile: Hsla,
}

impl Ink {
    pub(crate) fn new(theme: Option<(u32, u32)>, cx: &App) -> Self {
        let Some((from, to)) = theme else {
            let surface = palette::surface(palette::Level::Overlay, cx);
            return Ink { top: surface, bottom: surface, border: None, strong: palette::strong(cx), text: palette::text(cx), muted: palette::muted(cx), tile: palette::selected(cx) };
        };
        let (from, to): (Hsla, Hsla) = (palette::data_color(from), palette::data_color(to));
        let dark = cx.theme().is_dark();
        let ground: Hsla = if dark { gpui_kit::black() } else { gpui_kit::white() };
        let (top, bottom) = (ui::mix(from, ground, 0.55), ui::mix(to, ground, 0.55));
        let strong: Hsla = if luminance(ui::mix(top, bottom, 0.5)) > 0.4 { palette::ink_on_light() } else { gpui_kit::white() };
        Ink { top, bottom, border: Some((from, to)), strong, text: strong.opacity(0.92), muted: strong.opacity(0.72), tile: strong.opacity(0.1) }
    }

    // The body's gradient, top to bottom.
    pub(crate) fn wash(self) -> gpui_kit::Background {
        linear_gradient(180., linear_color_stop(self.top, 0.), linear_color_stop(self.bottom, 1.))
    }
}

pub(super) fn luminance(color: Hsla) -> f32 {
    let c = gpui_kit::Rgba::from(color);
    0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
}

// The card in the profile's colours: its gradient body inside the gradient border when the profile has a
// theme. Returns (outer, inner); content goes in the inner.
pub(crate) fn themed_card(ink: Ink, max_height: f32, cx: &App) -> (Div, Div) {
    let inner = ui::col().max_h(px(max_height - if ink.border.is_some() { 2. * BORDER } else { 0. })).squircle(inner_radius(ink)).bg(ink.wash());
    // lint-allow(custom-surface): ui::surface is a flat overlay fill; this card is the user's gradient inside a gradient border.
    let outer = div()
        .squircle(RADIUS_LG)
        .shadow(ui::shadow_pop(cx))
        .when_some(ink.border, |c, (from, to)| c.p(px(BORDER)).bg(linear_gradient(180., linear_color_stop(from, 0.), linear_color_stop(to, 1.))));
    (outer, inner)
}

pub(crate) fn inner_radius(ink: Ink) -> f32 {
    if ink.border.is_some() { RADIUS_LG - BORDER } else { RADIUS_LG }
}

// Discord's round buttons over the banner: translucent black, as over any media.
pub(super) fn banner_button(id: &'static str, glyph: Glyph, label: &'static str, cx: &App) -> ui::Control {
    ui::icon_button(id, glyph, label, cx).tone(ui::Tone::OnMedia, cx).round()
}

pub(super) fn menu_surface(in_guild: bool) -> Surface {
    if in_guild { Surface::ServerChat } else { Surface::Dm }
}

pub(crate) type OnClick = std::rc::Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

// Display name, then "username • pronouns" with the guild tag chip and their badges trailing that line.
// `open` makes the display name and username links (the popout's open the full profile); `note` trails the
// display name (the popout's note button); `tag` makes the guild tag open its server card.
pub(crate) fn name_block(user: &User, profile: &Profile, ink: Ink, open: Option<OnClick>, note: Option<AnyElement>, tag_click: Option<OnClick>) -> Div {
    let runs = user.name_style.as_ref().map(|style| crate::chat::name_runs(&user.name, style, ui::mix(ink.top, ink.bottom, 0.5))).unwrap_or_default();
    let handle = [Some(profile.username.clone()).filter(|u| !u.is_empty()), profile.pronouns.clone()].into_iter().flatten().collect::<Vec<_>>().join(" • ");
    let tag = user.tag.as_ref().map(|tag| {
        // lint-allow(custom-badge): ui::badge is a 20px solid label in 11px type; the guild tag is 22px in the card's 14px type and ink.
        ui::row()
            .h(px(TAG_HEIGHT))
            .px_1p5()
            .flex_none()
            .gap_1()
            .squircle(RADIUS_XS)
            .bg(ink.tile)
            .text_color(ink.strong)
            .text_size(ui::TEXT_SM)
            .font_weight(FontWeight::MEDIUM)
            .children(tag.badge.clone().map(|badge| img(badge).size_3p5()))
            .child(tag.text.clone())
            .id("profile-guild-tag")
            .when_some(tag_click.clone(), |t, click| {
                let hover = ink.strong.opacity(0.16);
                t.cursor_pointer().hover(move |s| s.bg(hover)).on_click(move |e, w, cx| click(e, w, cx))
            })
    });
    // lint-allow(raw-truncate): ui::line takes plain text; this is the user's name set in their chosen name font and styled runs.
    let name = crate::name_fonts::styled(div().id("profile-name").min_w_0().truncate().text_size(ui::TEXT_LG).line_height(ui::LINE_LG).font_weight(FontWeight::SEMIBOLD), user.name_style.as_ref())
        .text_color(ink.strong)
        .child(StyledText::new(user.name.clone()).with_highlights(runs))
        .when_some(open.clone(), |n, open| n.cursor_pointer().on_click(move |e, w, cx| open(e, w, cx)));
    let handle = (!handle.is_empty()).then(|| {
        // lint-allow(custom-button): the username is a link, as Discord's; ui::button would give it a fill and padding.
        div().id("profile-username").min_w_0().child(ui::line(handle)).when_some(open, |h, open| h.cursor_pointer().on_click(move |e, w, cx| open(e, w, cx)))
    });
    let badges = badges(&profile.badges);
    ui::col()
        .gap_0p5()
        .child(ui::row().gap_1().child(name).children(note))
        .when(handle.is_some() || tag.is_some() || badges.is_some(), |b| b.child(ui::row().flex_wrap().gap_x_1p5().gap_y_0p5().text_size(ui::TEXT_SM).text_color(ink.text).children(handle).children(tag).children(badges)))
}

// Their badges, bare, trailing the username row as Discord's; past six the rest wait behind "+N".
fn badges(badges: &[Badge]) -> Option<Div> {
    if badges.is_empty() {
        return None;
    }
    let shown = badges.iter().take(MAX_BADGES).enumerate().map(|(i, badge)| {
        let label = SharedString::from(badge.description.clone());
        let link = badge.link.clone();
        div()
            .id(("profile-badge", i))
            .size(px(BADGE_SIZE))
            .flex_none()
            .child(img(badge.icon.clone()).size_full())
            .tooltip(move |window, cx| ui::tip(label.clone(), window, cx))
            .when_some(link, |b, url| b.cursor_pointer().on_click(move |_, _, cx| cx.open_url(&url)))
    });
    let more = badges.len().saturating_sub(MAX_BADGES);
    let row = ui::row().id("profile-badges").flex_none().gap_0p5().children(shown).when(more > 0, |p| p.child(div().pl_0p5().text_size(ui::TEXT_XS).child(format!("+{more}"))));
    let row = match more > 0 {
        true => ui::overflow_list(row, "Badges", badges.iter().map(|b| ui::Entry::item(b.icon.clone(), b.description.clone())).collect()),
        false => row,
    };
    Some(ui::row().flex_none().child(row))
}

// "1 Mutual Server", "12 Mutual Friends".
pub(super) fn mutual_count(n: usize, noun: &str) -> String {
    if n == 1 { format!("1 {noun}") } else { format!("{n} {noun}s") }
}

// One half of the mutuals row: faces then its count, underlined on hover as Discord's link text.
pub(super) fn mutual_link(id: &'static str, faces: Option<Div>, label: String, ink: Ink) -> Stateful<Div> {
    let hover = ink.strong;
    ui::row().id(id).min_w_0().gap_1p5().cursor_pointer().text_color(ink.text).hover(move |s| s.text_color(hover).underline()).children(faces).child(ui::line(label))
}

// Discord markdown with custom emoji as images, as a message renders it; Discord gives the bio no heading.
pub(crate) fn bio_view(user_id: &str, bio: &str, ink: Ink) -> Div {
    let view = TextView::markdown(SharedString::from(format!("bio-{user_id}")), crate::chat::inline_emoji(&crate::chat::discord_breaks(bio))).selectable(true);
    div().text_size(ui::TEXT_SM).text_color(ink.strong).child(view)
}

// The bio cut to about three lines, as the popout and side panel show it; `true` when some is cut.
pub(super) fn clamped_bio(user_id: &str, bio: &str, ink: Ink) -> (Div, bool) {
    let cut = bio_overflows(bio);
    (bio_view(user_id, bio, ink).when(cut, |b| b.max_h(px(BIO_CLAMP)).overflow_hidden()), cut)
}

// Discord measures the rendered bio against 57.75px; gpui lays text out after this is built, so this guesses
// from the text: more than three lines, counting wraps at about 40 characters to the line.
// GAP: a bio of wide glyphs or many custom emoji can wrap sooner and lose its last line without the button.
pub(super) fn bio_overflows(bio: &str) -> bool {
    bio.lines().map(|line| line.chars().count().div_ceil(BIO_LINE_CHARS).max(1)).sum::<usize>() > 3
}

// Three lines of the 14px bio at its 20px leading, and the characters to a line in the 268px column.
const BIO_CLAMP: f32 = 58.;
const BIO_LINE_CHARS: usize = 40;

// The custom status in a thought bubble beside the avatar, two lines at most.
pub(super) fn status_bubble(text: String, expanded: bool, ink: Ink, cx: &App) -> Div {
    let fill = if ink.border.is_some() { ui::mix(ink.top, ink.strong, 0.08) } else { palette::surface(palette::Level::Raised, cx) };
    // A status past two lines takes the text's whole width: wrapped any narrower, gpui lays the clamped lines out
    // wider than the bubble and cuts their ends.
    let long = super::status_runs_long(&text);
    // lint-allow(raw-truncate): ui::line ends one line in an ellipsis; this clamps the status to two lines and ends the second.
    let content = div().min_w_0().max_w(px(BUBBLE_TEXT)).when(long, |c| c.w(px(BUBBLE_TEXT))).when(!expanded, |c| c.line_clamp(2).text_ellipsis()).text_color(ink.text).child(text);
    ui::row().items_start().child(ui::thought_bubble("profile-status-bubble", content, fill, palette::line(cx)).max_w(px(BUBBLE_MAX)))
}

// Discord's bubble caps at 181px, its text at 155px.
const BUBBLE_MAX: f32 = 181.;
const BUBBLE_TEXT: f32 = 155.;

// A profile differs per server (nickname, roles, server avatar), so the server is part of its key.
pub(crate) fn profile_key(user_id: &str, guild_id: Option<&str>) -> String {
    format!("{user_id}-{}", guild_id.unwrap_or("dm"))
}

pub(super) fn profile_skeleton(cx: &App) -> Div {
    let line = |id: &'static str, width: f32| ui::skeleton(id, 4., cx).h_3().w(relative(width));
    ui::col()
        .gap_2p5()
        .child(ui::skeleton("profile-skel-badges", 8., cx).h_5().w(px(SKELETON_BADGES)))
        .child(line("profile-skel-bio-1", 0.9))
        .child(line("profile-skel-bio-2", 0.55))
}

// The full avatar over the window at Discord's size: 70% of the window's short side, at most 1024.
// A click outside it or Esc closes it, back to the profile.
pub(super) fn avatar_viewer(user: &User, window: &Window, cx: &mut Context<Waffle>) -> AnyElement {
    let viewport = window.viewport_size();
    let side = (0.7 * f32::from(viewport.width).min(f32::from(viewport.height))).round().min(1024.);
    let picture = crate::chat::animated_image(super::media::full_avatar(&user.avatar)).size(px(side)).squircle(RADIUS_MD).object_fit(gpui_kit::ObjectFit::Cover);
    ui::row()
        .id("profile-avatar-full")
        .occlude()
        .absolute()
        .inset_0()
        .bg(gpui_kit::black().opacity(0.88))
        .justify_center()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.dismiss_overlay(cx);
            }),
        )
        .child(div().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).child(picture))
        .with_animation("profile-avatar-full", gpui_kit::Animation::new(ui::DUR_PANEL), |el, t| el.opacity(t))
        .into_any_element()
}

pub(super) fn section(title: &'static str, content: impl IntoElement, ink: Ink) -> Div {
    ui::col().gap_1p5().child(div().text_size(ui::TEXT_XS).font_weight(FontWeight::SEMIBOLD).text_color(ink.text).child(title)).child(content)
}

pub(super) fn since_date(id: &'static str, mark: impl IntoElement, tip: SharedString, at: &chrono::DateTime<chrono::Utc>, ink: Ink) -> Stateful<Div> {
    ui::row()
        .id(id)
        .gap_1p5()
        .text_size(ui::TEXT_SM)
        .text_color(ink.text)
        .child(mark)
        .child(short_date(at))
        .tooltip(move |window, cx| ui::tip(tip.clone(), window, cx))
}

// When a snowflake was minted: its top 42 bits are milliseconds since Discord's epoch (2015).
pub(super) fn snowflake_time(id: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    const DISCORD_EPOCH: i64 = 1_420_070_400_000;
    chrono::DateTime::from_timestamp_millis((id.parse::<u64>().ok()? >> 22) as i64 + DISCORD_EPOCH)
}

pub(super) fn parse_time(iso: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(iso).ok().map(|t| t.to_utc())
}

// Discord's `toLocaleDateString(…, {month: "short", day: "numeric", year: "numeric"})` in English, on the
// local calendar: "Mar 4, 2021".
pub(super) fn short_date(at: &chrono::DateTime<chrono::Utc>) -> String {
    at.with_timezone(&chrono::Local).format("%b %-d, %Y").to_string()
}
