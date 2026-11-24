// The profile card's activity: a stream, rich presence, or the voice channel they're in.
use gpui_kit::{FontWeight, Stateful, component::ActiveTheme};

use super::*;
use crate::{
    discord::{ActivityKind, RichPresence},
    ui::{RADIUS_MD, RADIUS_SM},
};

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
            .squircle(RADIUS_MD)
            .bg(ink.tile)
            .child(div().text_size(ui::TEXT_SM).font_weight(FontWeight::MEDIUM).text_color(ink.strong).child("Game Collection"))
            .child(ui::row().items_stretch().gap_1().children(icons).children(overflow)),
    )
}

pub(super) fn activity_heading(kind: ActivityKind) -> &'static str {
    match kind {
        ActivityKind::Streaming => "Streaming",
        ActivityKind::Listening => "Listening to",
        ActivityKind::Watching => "Watching",
        ActivityKind::Competing => "Competing in",
        ActivityKind::Playing | ActivityKind::Custom => "Playing",
    }
}

// GAP: the join request endpoint isn't wired, so the button shows the party is open but can't send yet.
pub(super) fn ask_to_join(ink: Ink) -> Stateful<Div> {
    // lint-allow(custom-badge): ui::button(...).disabled(true) takes a palette tone; the card wears the user's profile colours (Ink), which ui::button's palette tones can't take.
    ui::row()
        .id("profile-ask-to-join")
        .h_9()
        .justify_center()
        .gap_2()
        .squircle(RADIUS_SM)
        .bg(ink.tile)
        .text_color(ink.muted)
        .text_size(ui::TEXT_SM)
        .opacity(0.7)
        .child(Icon::new(Glyph::Game).size(ui::ICON_MD))
        .child("Ask to Join")
        .tooltip(|window, cx| ui::tip("Joining from Waffle isn't supported yet", window, cx))
}

impl Waffle {
    // Everything they're doing, in Discord's order: the stream, each activity, then voice when not streaming.
    pub(super) fn profile_activity_cards(&self, card: &ProfileCard, ink: Ink, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let place = self.guilds.iter().find_map(|g| {
            let state = g.voice_states.iter().find(|v| v.user_id == card.user.id)?;
            let channel = g.channels.iter().find(|c| Some(&c.id) == state.channel_id.as_ref())?;
            Some((g, channel, state.streaming))
        });
        let mut cards: Vec<AnyElement> = Vec::new();
        if let Some((guild, channel, true)) = place {
            cards.push(self.stream_activity(card, guild, channel, ink, cx));
        }
        if let Some(rich) = self.presences.get(&card.user.id).and_then(|p| p.rich.as_ref()) {
            cards.push(self.activity_card(card, rich, ink, cx).into_any_element());
        }
        if let Some((guild, channel, false)) = place {
            cards.push(self.voice_activity(card, guild, channel, ink, cx));
        }
        cards
    }

    // Discord's popout shows one activity card; more than one peeks from under it, and clicking the peek
    // lists them all in place.
    pub(super) fn profile_activity(&self, card: &ProfileCard, ink: Ink, cx: &mut Context<Self>) -> Option<AnyElement> {
        let mut cards = self.profile_activity_cards(card, ink, cx);
        if cards.len() <= 1 {
            return cards.pop();
        }
        if card.all_activity {
            let less = div()
                .id("profile-activity-less")
                .text_size(ui::TEXT_SM)
                .font_weight(FontWeight::MEDIUM)
                .text_color(ink.muted)
                .cursor_pointer()
                .hover(move |s| s.text_color(ink.strong))
                .child("Show less")
                .on_click(cx.listener(|this, _, _, cx| this.show_all_activity(false, cx)));
            return Some(ui::col().items_start().gap_2().children(cards.into_iter().map(|c| div().w_full().child(c))).child(less).into_any_element());
        }
        let more = cards.len() - 1;
        let first = cards.into_iter().next()?;
        Some(self.activity_more(first, more, ink, cx).into_any_element())
    }

    // The first card, then a button under it that unfolds the rest in place.
    pub(super) fn activity_more(&self, first: AnyElement, more: usize, ink: Ink, cx: &mut Context<Self>) -> Div {
        let label = if more == 1 { "Show 1 more activity".to_owned() } else { format!("Show {more} more activities") };
        let button = ui::button("profile-activity-view-more", ui::Tone::Ghost, cx)
            .scoped(ink.tile, ink.tile, ink.muted, ink.strong)
            .h_7()
            .squircle(RADIUS_SM)
            .child(label)
            .on_click(cx.listener(|this, _, _, cx| this.show_all_activity(true, cx)));
        ui::col().gap_1p5().child(first).child(button)
    }

    pub(super) fn show_all_activity(&mut self, all: bool, cx: &mut Context<Self>) {
        if let Some(card) = &mut self.profile_card {
            card.all_activity = all;
        }
        self.redraw_overlays(cx);
    }

    // Discord's rich presence card: heading and menu, the large image, then name, details, state with
    // the party size, the elapsed time, and Ask to Join when the party is open.

    // .profile-stream: "Streaming {game}" with LIVE, the stream's preview (a click watches it, joining the
    // call first if need be), where it is, and Join Voice.
    pub(super) fn stream_activity(&self, card: &ProfileCard, guild: &crate::discord::Guild, channel: &crate::discord::Channel, ink: Ink, cx: &mut Context<Self>) -> AnyElement {
        let user_id = card.user.id.clone();
        let is_me = self.me.as_ref().is_some_and(|me| me.id == user_id);
        let game = self.presences.get(&user_id).and_then(|p| p.activity.as_ref()).filter(|a| a.kind == ActivityKind::Playing).map(|a| a.text.clone());
        let heading = game.map_or_else(|| "Streaming".to_owned(), |game| format!("Streaming {game}"));
        let connected = self.call.as_ref().is_some_and(|c| c.channel.id == channel.id && c.connected);
        let watch_channel = channel.clone();
        let preview = div()
            .id("profile-stream-preview")
            .relative()
            .w_full()
            .aspect_ratio(16. / 9.)
            .child(crate::call::stream_fill(RADIUS_SM).absolute().inset_0())
            .children(self.stream_preview(&user_id).map(|url| ui::media(url, ui::all(RADIUS_SM)).absolute().size_full().object_fit(gpui_kit::ObjectFit::Cover)))
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
        let head = ui::row()
            .gap_2()
            .child(ui::line(heading).text_size(ui::TEXT_SM).font_weight(FontWeight::MEDIUM).text_color(ink.strong))
            .child(ui::live_badge());
        ui::col()
            .p_3()
            .gap_2p5()
            .squircle(RADIUS_MD)
            .bg(ink.tile)
            .child(head)
            .child(preview)
            .child(self.voice_place_row(guild, channel, ink))
            .children((!is_me && !connected).then(|| self.join_voice_button(guild, channel, cx)))
            .into_any_element()
    }

    // .profile-voice: "In voice", who's there, where, and Join Voice.
    pub(super) fn voice_activity(&self, card: &ProfileCard, guild: &crate::discord::Guild, channel: &crate::discord::Channel, ink: Ink, cx: &mut Context<Self>) -> AnyElement {
        let is_me = self.me.as_ref().is_some_and(|me| me.id == card.user.id);
        let connected = self.call.as_ref().is_some_and(|c| c.channel.id == channel.id && c.connected);
        ui::col()
            .p_3()
            .gap_2p5()
            .squircle(RADIUS_MD)
            .bg(ink.tile)
            .child(div().text_size(ui::TEXT_SM).font_weight(FontWeight::MEDIUM).text_color(ink.strong).child("In voice"))
            .child(self.voice_place_row(guild, channel, ink))
            .children((!is_me && !connected).then(|| self.join_voice_button(guild, channel, cx)))
            .into_any_element()
    }

    // The server › channel breadcrumb, and who's in it.
    pub(super) fn voice_place_row(&self, guild: &crate::discord::Guild, channel: &crate::discord::Channel, ink: Ink) -> Div {
        let people: Vec<ui::Entry> = guild
            .voice_states
            .iter()
            .filter(|v| v.channel_id.as_ref() == Some(&channel.id))
            .map(|v| match &v.user {
                Some(u) => ui::Entry::person(u.avatar.clone(), u.name.clone()),
                None => ui::Entry::person(crate::discord::avatar_url(&v.user_id, None), v.user_id.clone()),
            })
            .collect();
        let server = match &guild.icon {
            Some(icon) => ui::media(icon.clone(), ui::all(RADIUS_XS)).size_5().flex_none().into_any_element(),
            None => div().size_5().flex_none().squircle(RADIUS_XS).bg(ink.top).into_any_element(),
        };
        ui::row()
            .gap_1p5()
            .text_size(ui::TEXT_SM)
            .text_color(ink.text)
            .child(server)
            .child(Icon::new(Glyph::ChevronRight).size(ui::ICON_SM).flex_none().text_color(ink.muted))
            .child(crate::sidebar::channel_glyph(channel).size_4().flex_none().text_color(ink.muted))
            .child(ui::line(channel.name.clone()).flex_1())
            .child(ui::listed_stack(SharedString::from(format!("profile-voice-{}", channel.id)), "In voice", people, 3, 20., ink.tile, ink.top, ink.strong))
    }

    pub(super) fn join_voice_button(&self, guild: &crate::discord::Guild, channel: &crate::discord::Channel, cx: &mut Context<Self>) -> AnyElement {
        let (guild_id, channel) = (guild.id.clone(), channel.clone());
        ui::button("profile-join-voice", ui::Tone::Secondary, cx)
            .w_full()
            .justify_center()
            .child("Join Voice")
            .on_click(cx.listener(move |this, _, _, cx| {
                this.profile_card = None;
                this.join_voice_in(Some(guild_id.clone()), channel.clone(), cx);
                cx.notify(); // lint-allow(global-notify): joining a call changes the rail, the channel list and the call stage
            }))
            .into_any_element()
    }

    pub(super) fn activity_card(&self, card: &ProfileCard, rich: &RichPresence, ink: Ink, cx: &mut Context<Self>) -> Div {
        let (user, menu_rich) = (card.user.clone(), rich.clone());
        let more = (!self.activity_menu(&card.user, rich).is_empty()).then(|| {
            ui::icon_button("profile-activity-more", Glyph::More, "More", cx)
                .scoped(gpui_kit::transparent_black(), ink.tile, ink.muted, ink.strong)
                .small()
                .on_click(cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| this.open_activity_menu(user.clone(), menu_rich.clone(), event.position(), cx)))
        });
        let art = match &rich.image {
            Some(url) => ui::media(url.clone(), ui::all(RADIUS_SM)).size(px(ACTIVITY_ART)).flex_none().object_fit(gpui_kit::ObjectFit::Cover).into_any_element(),
            None => ui::row().size(px(ACTIVITY_ART)).flex_none().squircle(RADIUS_SM).bg(ink.tile).justify_center().text_color(ink.muted).child(Icon::new(Glyph::Game).size_8()).into_any_element(),
        };
        let line = |text: String| ui::line(text).text_color(ink.text);
        let state = rich.state.clone().map(|state| match rich.party {
            Some((current, max)) => format!("{state} ({current} of {max})"),
            None => state,
        });
        let elapsed = rich.started.map(|start| crate::call::clock(((chrono::Utc::now().timestamp_millis() - start).max(0) / 1000) as u64));
        let mono = cx.theme().mono_font_family.clone();
        let facts = ui::col()
            .gap_0p5()
            .text_size(ui::TEXT_SM)
            .child(ui::line(rich.name.clone()).text_size(ui::TEXT_SM).font_weight(FontWeight::SEMIBOLD).text_color(ink.strong))
            .children(rich.details.clone().map(line))
            .children(state.map(|s| ui::row().gap_1().text_color(ink.text).when(rich.party.is_some(), |d| d.child(Icon::new(Glyph::Users).size(ui::ICON_SM).flex_none())).child(ui::line(s))))
            .children(elapsed.map(|time| {
                // Status green is ~2:1 on a light card; a deeper green keeps it readable there.
                let green = if luminance(ink.strong) < 0.5 { palette::online_on_light() } else { palette::status(crate::discord::Status::Online) };
                ui::row().gap_1().text_color(green).child(Icon::new(Glyph::Game).size(ui::ICON_SM)).child(div().font_family(mono).child(time))
            }));
        ui::col()
            .p_3()
            .gap_2p5()
            .squircle(RADIUS_MD)
            .bg(ink.tile)
            .group(ACTIVITY_GROUP)
            .child(
                ui::row()
                    .justify_between()
                    .child(div().text_size(ui::TEXT_SM).font_weight(FontWeight::MEDIUM).text_color(ink.strong).child(activity_heading(rich.kind)))
                    .child(ui::row().gap_1().children(self.interaction_toolbar(card, Source::Activity(rich.name.clone()), ACTIVITY_GROUP, ink, cx)).children(more)),
            )
            .child(ui::row().gap_3().child(art).child(facts))
            .when(rich.joinable, |c| c.child(ask_to_join(ink))).children(self.activity_join_button(&card.user.id, rich, cx))
    }
}
