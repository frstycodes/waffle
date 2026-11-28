// The profile's activity cards, as Discord draws them: a Go Live stream, one card per activity, then the voice
// channel they're in. The popout and the DM side panel show the first card with the rest peeking under it; the
// full profile lists them all, larger.
use gpui_kit::{FontWeight, Hsla};

use super::activity_voice::{hairline, voice_heading};
use super::*;
use crate::discord::{Channel, Guild};

// An activity's art and the badge on its corner: Discord's 60/24 in the popout and side panel, 100/32 in the
// full profile, the badge hanging 4px past the art's corner.
const ART: f32 = 60.;
const ART_WIDE: f32 = 100.;
const BADGE: f32 = 24.;
const BADGE_WIDE: f32 = 32.;
const BADGE_HANG: f32 = -4.;
// The gap ringing the badge where it covers the art, standing in for Discord's mask hole.
const BADGE_GAP: f32 = 2.;

// The popout and side panel's compact card, or the full profile's wide one.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Fit {
    Compact,
    Wide,
}

impl Fit {
    pub(super) fn art(self) -> f32 {
        if self == Fit::Wide { ART_WIDE } else { ART }
    }

    // Art and previews inside the card: one squircle step under the card's own.
    pub(super) fn inner_radius(self) -> f32 {
        if self == Fit::Wide { ui::RADIUS_MD } else { ui::RADIUS_SM }
    }

    fn badge(self) -> f32 {
        if self == Fit::Wide { BADGE_WIDE } else { BADGE }
    }
}

pub(super) fn game_collection(games: &[String], ink: Ink) -> Option<Div> {
    if games.is_empty() {
        return None;
    }
    let more = games.len().saturating_sub(MAX_GAMES);
    let tile = |el: Div| el.size_8().flex_none().squircle(RADIUS_XS);
    let icons = games.iter().take(MAX_GAMES).map(|url| ui::media(url.clone(), ui::all(RADIUS_XS)).size_8().flex_none().object_fit(gpui_kit::ObjectFit::Cover));
    let overflow = (more > 0).then(|| {
        tile(div()).bg(gpui_kit::black().opacity(0.6)).flex().items_center().justify_center().text_size(ui::TEXT_SM).text_color(gpui_kit::white()).child(format!("+{more}"))
    });
    Some(
        ui::row()
            .px_3()
            .py_2p5()
            .justify_between()
            .gap_2()
            .squircle(ui::RADIUS_MD)
            .bg(ink.tile)
            .child(div().text_size(ui::TEXT_SM).font_weight(FontWeight::MEDIUM).text_color(ink.strong).child("Game Collection"))
            .child(ui::row().items_stretch().gap_1().children(icons).children(overflow)),
    )
}

// The card's fill: compact cards are 8px in from their edge, wide ones 12, at the app's control and surface
// rounding rather than Discord's 8 and 12.
pub(super) fn shell(fit: Fit, ink: Ink) -> Div {
    let card = ui::col().w_full().gap_2().bg(ink.tile);
    match fit {
        Fit::Compact => card.p_2().squircle(ui::RADIUS_MD),
        Fit::Wide => card.p_3().squircle(ui::RADIUS_LG),
    }
}

// The header: what they're doing in small strong type, the brand's mark, any tag (LIVE), and the card's menu.
pub(super) fn head(text: String, mark: Option<&'static str>, tag: Option<AnyElement>, menu: Option<AnyElement>, ink: Ink) -> Div {
    let mark = mark.and_then(crate::icons::brand_mark).map(|path| gpui_kit::svg().path(path).size(ui::ICON_XS).flex_none().text_color(ink.muted));
    let title = ui::row().flex_1().gap_1().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(ink.strong).child(ui::line(text)).children(mark).children(tag);
    ui::row().gap_1().justify_between().child(title).children(menu)
}

// The large image with the small one on its corner, the small one taking the large's place when it's alone,
// and Discord's unknown-app square when there's neither.
pub(super) fn art(large: Option<String>, small: Option<String>, fit: Fit, ink: Ink) -> Div {
    let (size, badge) = (fit.art(), fit.badge());
    let (large, small) = match large {
        Some(large) => (Some(large), small),
        None => (small, None),
    };
    let radius = fit.inner_radius();
    let big = match large {
        Some(url) => ui::media(url, ui::all(radius)).size_full().object_fit(gpui_kit::ObjectFit::Cover).into_any_element(),
        None => unknown_app(radius, ink).into_any_element(),
    };
    let corner = small.map(|url| {
        div()
            .absolute()
            .bottom(px(BADGE_HANG))
            .right(px(BADGE_HANG))
            .size(px(badge))
            .child(ui::gap(solid_tile(ink), BADGE_GAP))
            .child(ui::Avatar::new(url, badge))
    });
    div().relative().flex_none().size(px(size)).child(big).children(corner)
}

// Discord's stand-in for an app with no art: a solid square in the card's strongest ink with a question mark.
fn unknown_app(radius: f32, ink: Ink) -> Div {
    // lint-allow(custom-badge): ui::Badge is a small label; this is the art slot itself, Discord's unknown-app square.
    ui::row().size_full().justify_center().squircle(radius).bg(ink.strong).text_color(solid_tile(ink)).font_weight(FontWeight::BOLD).text_size(ui::TEXT_LG).child("?")
}

// The card's fill made opaque over the card's own top colour, for rings that must read as gaps.
pub(super) fn solid_tile(ink: Ink) -> Hsla {
    ui::mix(ink.top, ink.tile.opacity(1.), ink.tile.a)
}

// A button on a card, in the card's own colours: Discord's small secondary.
pub(super) fn card_button(id: impl Into<gpui_kit::ElementId>, ink: Ink, _cx: &App) -> ui::Button {
    ui::Button::new(id).variant(ui::ButtonVariant::Secondary).size(ui::ButtonSize::Sm).scope(ui::ButtonScope::Themed { rest: ink.strong.opacity(0.1), hover: ink.strong.opacity(0.16), ink: ink.strong, ink_hover: ink.strong })
}

impl Waffle {
    // Everything they're doing, in Discord's order: the stream, each activity, then voice when not streaming.
    pub(super) fn profile_activity_cards(&self, card: &ProfileCard, fit: Fit, ink: Ink, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let place = self.guilds.iter().find_map(|g| {
            let state = g.voice_states.iter().find(|v| v.user_id == card.user.id)?;
            let channel = g.channels.iter().find(|c| Some(&c.id) == state.channel_id.as_ref())?;
            Some((g, channel, state.streaming))
        });
        let mut cards: Vec<AnyElement> = Vec::new();
        if let Some((guild, channel, true)) = place {
            cards.push(self.stream_activity(card, guild, channel, fit, ink, cx));
        }
        let activities = self.presences.get(&card.user.id).map(|p| p.activities.clone()).unwrap_or_default();
        for (at, rich) in activities.iter().enumerate() {
            cards.push(self.activity_card(card, at, rich, fit, ink, cx).into_any_element());
        }
        if let Some((guild, channel, false)) = place {
            cards.push(self.voice_activity(card, guild, channel, fit, ink, cx));
        }
        cards
    }

    // The popout and side panel show the first card and a plain "Show 2 more" under it, which lists the rest in
    // place. Discord tucks the next card's edge under the first instead; a label says what it does.
    pub(super) fn profile_activity(&self, card: &ProfileCard, ink: Ink, cx: &mut Context<Self>) -> Option<AnyElement> {
        let mut cards = self.profile_activity_cards(card, Fit::Compact, ink, cx);
        if cards.len() <= 1 {
            return cards.pop();
        }
        let more = cards.len() - 1;
        let all = card.all_activity;
        let label = if all { "Show less".to_owned() } else { format!("Show {more} more") };
        let toggle = ui::Button::new("profile-activity-more-cards").variant(ui::ButtonVariant::Ghost)
            .scope(ui::ButtonScope::Themed { rest: gpui_kit::transparent_black(), hover: ink.strong.opacity(0.1), ink: ink.muted, ink_hover: ink.strong })
            .size(ui::ButtonSize::Sm)
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| this.show_all_activity(!all, cx)));
        let shown = if all { cards.len() } else { 1 };
        Some(ui::col().gap_2().children(cards.into_iter().take(shown)).child(div().child(toggle)).into_any_element())
    }

    fn show_all_activity(&mut self, all: bool, cx: &mut Context<Self>) {
        let Some(card) = &mut self.profile_card else { return };
        card.all_activity = all;
        match card.panel {
            true => self.redraw(crate::part::Column::Panel, cx),
            false => self.redraw_overlays(cx),
        }
    }

    // .profile-stream: "Streaming {game}" with LIVE, the stream's preview (a click watches it, joining the
    // call first if need be), where it is, and Join Voice.
    fn stream_activity(&self, card: &ProfileCard, guild: &Guild, channel: &Channel, fit: Fit, ink: Ink, cx: &mut Context<Self>) -> AnyElement {
        let user_id = card.user.id.clone();
        let is_me = self.me.as_ref().is_some_and(|me| me.id == user_id);
        let game = self.presences.get(&user_id).and_then(|p| p.activity.as_ref()).filter(|a| a.kind == crate::discord::ActivityKind::Playing).map(|a| a.text.clone());
        let heading = game.map_or_else(|| "Streaming".to_owned(), |game| format!("Streaming {game}"));
        let connected = self.call.as_ref().is_some_and(|c| c.channel.id == channel.id && c.connected);
        let watch_channel = channel.clone();
        let label = match (is_me, connected) {
            (true, _) => "You\u{2019}re Streaming!",
            (false, true) => "Watch Stream",
            (false, false) => "Join Voice Channel",
        };
        let preview = div()
            .id("profile-stream-preview")
            .relative()
            .group("profile-stream-preview")
            .flex_none()
            .map(|d| match fit {
                Fit::Compact => d.w_full().aspect_ratio(16. / 9.),
                Fit::Wide => d.size(px(ART_WIDE)),
            })
            .child(crate::call::stream_fill(fit.inner_radius()).absolute().inset_0())
            .children(self.stream_preview(&user_id).map(|url| ui::media(url, ui::all(fit.inner_radius())).absolute().size_full().object_fit(gpui_kit::ObjectFit::Cover)))
            .when(fit == Fit::Compact, |d| d.child(ui::row().absolute().inset_0().justify_center().child(crate::active_now::watch_label("profile-stream-preview", label))))
            .when(!is_me, |d| {
                d.cursor_pointer().on_click(cx.listener(move |this, _, window, cx| {
                    this.profile_card = None;
                    match connected {
                        true => this.watch_stream(user_id.clone(), cx),
                        false => this.watch_on_connect = Some(user_id.clone()),
                    }
                    this.show_call(watch_channel.clone(), window, cx);
                }))
            });
        let head = head(heading, None, Some(ui::Badge::live().into_any_element()), None, ink);
        let join = (!is_me).then(|| self.voice_way_in(guild, channel, fit, ink, cx));
        match fit {
            Fit::Compact => shell(fit, ink).child(head).child(preview).child(hairline(ink)).child(self.voice_place_row(guild, channel, ink, cx)).children(join).into_any_element(),
            Fit::Wide => {
                let details = ui::col().flex_1().gap_1().child(voice_heading(guild, channel, ink)).child(self.voice_faces(guild, channel, ink)).children(join);
                shell(fit, ink).child(head).child(ui::row().items_start().gap_3().child(preview).child(details)).into_any_element()
            }
        }
    }

    // .profile-voice: "In voice", the channel's tile with who's there, where it is, and Join Voice.
    fn voice_activity(&self, card: &ProfileCard, guild: &Guild, channel: &Channel, fit: Fit, ink: Ink, cx: &mut Context<Self>) -> AnyElement {
        let is_me = self.me.as_ref().is_some_and(|me| me.id == card.user.id);
        let join = (!is_me).then(|| self.voice_way_in(guild, channel, fit, ink, cx));
        let details = ui::col().flex_1().min_w_0().gap_1().justify_center().child(voice_heading(guild, channel, ink));
        let (details, foot) = match fit {
            Fit::Wide => (details.children(join), None),
            Fit::Compact => (details, join),
        };
        let body = ui::row().map(|r| if fit == Fit::Wide { r.gap_3() } else { r.gap_2() }).child(self.voice_tile(guild, channel, fit, ink)).child(details);
        shell(fit, ink).child(head("In voice".into(), None, None, None, ink)).child(body).children(foot).into_any_element()
    }
}
