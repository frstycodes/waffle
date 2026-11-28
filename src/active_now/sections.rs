// One Active Now card as Discord draws it: a tinted header with who and what, then a section per voice channel,
// per stream and per activity, each under a hairline. A click opens the card's menu.
use gpui_kit::{AnyElement, App, Context, Div, FontWeight, Hsla, SharedString, Stateful, component::Icon, div, prelude::*, px};

use super::{Party, app_key, now_ms, subtitle, title};
use crate::{
    Waffle,
    discord::{ActivityKind, RichPresence, User},
    icons::Glyph,
    menu::{Entry, item, label},
    palette,
    profile::activity_text::is_spotify,
    sidebar::Nav,
    ui::{self, Squircle},
};

// A section's art: an album or a game's image, at Discord's 32px, in the app's small squircle.
const ART: f32 = ui::AVATAR_LG;
// The faces at a section's end: two, a third only when it's the last.
const FACES: usize = 3;
// The badge on a voice section's server: the speaker on Discord's darker disc.
const SPEAKER: f32 = ui::AVATAR_XS;

impl Waffle {
    pub(super) fn party_card(&self, party: &Party, cx: &mut Context<Self>) -> AnyElement {
        let raised = palette::surface(palette::Level::Raised, cx);
        let lead = &party.members[0];
        let names: Vec<&str> = party.members.iter().map(|m| m.name.as_str()).collect();
        let face = ui::badged(ui::Avatar::new(lead.avatar.clone(), ui::AVATAR_LG), ui::AVATAR_LG, ui::Shape::Round, header_fill(cx)).status(self.presences.get(&lead.id).map(|p| p.mark()));
        let who = ui::col()
            .flex_1()
            .child(ui::line(title(&names)).text_size(ui::TEXT_MD).line_height(ui::LINE_SM).font_weight(FontWeight::SEMIBOLD).text_color(palette::text(cx)))
            .children(subtitle(party, now_ms()).map(|s| ui::line(s).text_size(ui::TEXT_SM).line_height(ui::LINE_DETAIL).text_color(palette::muted(cx))));
        let head = ui::edge(ui::row().p_4().gap_3().bg(header_fill(cx)), ui::top(ui::RADIUS_LG)).child(face).child(who).children(self.head_mark(party, cx));
        let mut sections: Vec<AnyElement> = self.party_voice(party, cx).into_iter().collect();
        for id in &party.streamers {
            sections.extend(self.party_stream(party, id, cx));
        }
        for rich in &party.activities {
            sections.push(self.party_activity(party, rich, cx));
        }
        let key = party.key.clone();
        ui::Card::interactive(SharedString::from(format!("active-now-{key}")), cx)
            .scoped(raised, palette::surface(palette::Level::Overlay, cx), palette::text(cx), palette::text(cx))
            .bordered(cx)
            .stacked()
            .p_0()
            .gap_0()
            .child(head)
            .children(sections.into_iter().map(|s| div().border_t_1().border_color(palette::line(cx)).p_4().child(s)))
            .on_click(cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| {
                let entries = this.active_parties().into_iter().find(|p| p.key == key).map(|p| this.party_menu(&p)).unwrap_or_default();
                this.open_menu(event.position(), entries, cx);
            }))
            .into_any_element()
    }

    // The mark at the header's end: the game's icon, Spotify's, or a controller for several games.
    fn head_mark(&self, party: &Party, cx: &mut Context<Self>) -> Option<AnyElement> {
        let games: Vec<&RichPresence> = party.activities.iter().filter(|a| !is_spotify(a)).collect();
        match (games.as_slice(), party.activities.first()) {
            ([], Some(first)) if is_spotify(first) => Some(brand("spotify", palette::positive_ink(cx)).into_any_element()),
            ([game], _) => Some(self.game_art(game, ui::AVATAR_MD, cx)),
            ([], _) => None,
            _ => Some(ui::row().size_6().flex_none().justify_center().rounded_full().bg(palette::hover(cx)).text_color(palette::muted(cx)).child(Nav::GameFill.icon().size(ui::ICON_LG)).into_any_element()),
        }
    }

    // The server with the speaker on its corner, the server and channel names, who's in it, and Join.
    fn party_voice(&self, party: &Party, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (guild_id, channel_id) = party.voice.clone()?;
        let guild = self.guilds.iter().find(|g| g.id == guild_id)?;
        let channel = guild.channels.iter().find(|c| c.id == channel_id)?.clone();
        let tile = ui::badged(ui::server_tile(guild.icon.as_deref(), &guild.name, ui::AVATAR_LG, cx), ui::AVATAR_LG, ui::Shape::Round, palette::surface(palette::Level::Raised, cx))
            .icon(ui::Spot::BottomRight, Icon::new(Glyph::VolumeHigh).size(px(SPEAKER)), ui::MarkFill::Surface);
        let occupants: Vec<ui::Entry> = party.occupants.iter().map(|id| ui::Entry::person(self.user_avatar(id), self.user_name(id))).collect();
        let joined = self.call.as_ref().is_some_and(|c| c.channel.id == channel.id);
        // Waffle's own: Discord joins from the menu or the channel's name; a button says it plainly.
        let join = ui::Button::new(SharedString::from(format!("active-now-join-{channel_id}"))).variant(ui::ButtonVariant::Secondary)
            .size(ui::ButtonSize::Sm)
            .disabled(joined)
            .child(if joined { "Joined" } else { "Join" })
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.join_voice_in(Some(guild_id.clone()), channel.clone(), cx);
            }));
        let text = two_lines(guild.name.clone(), Some(party_channel_name(self, party)), cx);
        Some(ui::row().gap_3().child(tile).child(text).child(faces(&format!("active-now-voice-{channel_id}"), occupants, cx)).child(join).into_any_element())
    }

    // Someone on Go Live: who and what, LIVE, then the stream's preview.
    fn party_stream(&self, party: &Party, user_id: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        let user = party.members.iter().find(|m| m.id == user_id)?;
        let doing = self.presences.get(user_id).and_then(|p| p.activity.as_ref()).map(|a| a.text.clone()).unwrap_or_else(|| "Sharing their screen".into());
        let row = ui::row().gap_3().child(ui::Avatar::new(user.avatar.clone(), ui::AVATAR_LG)).child(two_lines(user.name.clone(), Some(doing), cx)).child(ui::Badge::live());
        let group = SharedString::from(format!("active-now-stream-{user_id}"));
        let preview = div()
            .relative()
            .group(group.clone())
            .w_full()
            .aspect_ratio(16. / 9.)
            .child(crate::call::stream_fill(ui::RADIUS_MD).absolute().inset_0())
            .children(self.stream_preview(user_id).map(|url| ui::media(url, ui::all(ui::RADIUS_MD)).absolute().size_full().object_fit(gpui_kit::ObjectFit::Cover)))
            .child(ui::row().absolute().inset_0().justify_center().child(watch_label(group, "Watch Stream")));
        Some(ui::col().gap_3().child(row).child(preview).into_any_element())
    }

    // A game (its art, details, state and how long), several friends in one game, or a Spotify track.
    fn party_activity(&self, party: &Party, rich: &RichPresence, cx: &mut Context<Self>) -> AnyElement {
        let holders: Vec<&User> = party.members.iter().filter(|m| self.presences.get(&m.id).is_some_and(|p| p.activities.iter().any(|a| app_key(a) == app_key(rich)))).collect();
        let people: Vec<ui::Entry> = holders.iter().map(|m| ui::Entry::person(m.avatar.clone(), m.name.clone())).collect();
        let faces = (holders.len() > 1).then(|| faces(&format!("active-now-{}-{}", party.key, app_key(rich)), people, cx));
        if is_spotify(rich) {
            let artists = rich.state.as_ref().map(|s| s.replace("; ", ", "));
            let album = match &rich.image {
                Some(url) => ui::media(url.clone(), ui::all(ui::RADIUS_XS)).size(px(ART)).flex_none().object_fit(gpui_kit::ObjectFit::Cover).into_any_element(),
                None => brand("spotify", palette::positive_ink(cx)).size(px(ART)).into_any_element(),
            };
            return ui::row().items_start().gap_3().child(album).child(two_lines(rich.details.clone().unwrap_or_else(|| rich.name.clone()), artists, cx)).children(faces).into_any_element();
        }
        if rich.kind == ActivityKind::Streaming {
            let mark = brand(if rich.name == "YouTube" { "youtube" } else { "twitch" }, palette::muted(cx)).size(px(ART));
            return ui::row().gap_3().child(mark).child(two_lines(rich.name.clone(), rich.details.clone(), cx)).children(faces).into_any_element();
        }
        if holders.len() > 1 {
            let count = format!("{} People", holders.len());
            return ui::row().gap_3().child(self.game_art(rich, ART, cx)).child(two_lines(rich.name.clone(), Some(count), cx)).children(faces).into_any_element();
        }
        let elapsed = rich.started.map(|start| format!("{} elapsed", crate::call::clock(((now_ms() - start).max(0) / 1000) as u64)));
        let lines = ui::col()
            .flex_1()
            .child(ui::line(rich.details.clone().unwrap_or_else(|| rich.name.clone())).text_size(ui::TEXT_SM).font_weight(FontWeight::SEMIBOLD).text_color(palette::text(cx)))
            .children([rich.state.clone(), elapsed].into_iter().flatten().map(|t| ui::line(t).text_size(ui::TEXT_XS).text_color(palette::muted(cx))));
        ui::row().items_start().gap_3().child(self.game_art(rich, ART, cx)).child(lines).into_any_element()
    }

    // The activity's image, else its app's icon; nothing to show draws Discord's grey controller tile.
    fn game_art(&self, rich: &RichPresence, size: f32, cx: &mut Context<Self>) -> AnyElement {
        let image = rich.image.clone().or_else(|| rich.app_id.as_deref().and_then(|id| self.app_icon(id, cx)));
        match image {
            Some(url) => ui::media(url, ui::all(ui::RADIUS_XS)).size(px(size)).flex_none().object_fit(gpui_kit::ObjectFit::Cover).into_any_element(),
            None => ui::row().size(px(size)).flex_none().justify_center().squircle(ui::RADIUS_XS).bg(palette::hover(cx)).text_color(palette::muted(cx)).child(Nav::GameFill.icon().size(px(size * 0.6))).into_any_element(),
        }
    }

    // Discord's card menu: Message for one person, the voice channel's and stream's ways in, Spotify's, then who.
    fn party_menu(&self, party: &Party) -> Vec<Entry> {
        let mut entries = Vec::new();
        if let [one] = party.members.as_slice() {
            let id = one.id.clone();
            entries.push(item("Message", move |this, window, cx| this.message_user(id.clone(), window, cx)).icon(Glyph::ChatRound).entry());
        }
        let channel = party.voice.as_ref().and_then(|(g, c)| Some((g.clone(), self.guilds.iter().find(|x| &x.id == g)?.channels.iter().find(|x| &x.id == c)?.clone())));
        if let Some((guild_id, channel)) = channel {
            entries.push(Entry::Separator);
            let (join_guild, join_channel) = (guild_id.clone(), channel.clone());
            entries.push(item("Join Channel", move |this, _, cx| this.join_voice_in(Some(join_guild.clone()), join_channel.clone(), cx)).icon(Glyph::VolumeHigh).entry());
            entries.push(item("Go to Server", move |this, window, cx| this.open_guild(Some(guild_id.clone()), window, cx)).entry());
            for id in &party.streamers {
                let (watch, watch_channel) = (id.clone(), channel.clone());
                entries.push(item(format!("Watch {}'s Stream", self.user_name(id)), move |this, window, cx| {
                    this.watch_on_connect = Some(watch.clone());
                    this.show_call(watch_channel.clone(), window, cx);
                }).icon(Glyph::Stream).entry());
            }
        }
        if let Some(track) = party.activities.iter().find(|a| is_spotify(a)).and_then(|a| a.sync_id.clone()) {
            entries.push(Entry::Separator);
            entries.push(item("Play on Spotify", move |_, _, cx| cx.open_url(&format!("https://open.spotify.com/track/{track}"))).icon(Glyph::Play).entry());
            // GAP: listening along needs Spotify's player connection, which Waffle doesn't make yet.
            entries.push(item("Listen Along", |_, _, _| {}).icon(Glyph::Headphones).disabled().entry());
        }
        entries.push(Entry::Separator);
        entries.push(label(if party.members.len() == 1 { "1 Person".to_owned() } else { format!("{} People", party.members.len()) }));
        for member in &party.members {
            let user = member.clone();
            entries.push(item(member.name.clone(), move |this, window, cx| this.open_profile(user.clone(), window.mouse_position(), cx)).icon(Glyph::User).entry());
        }
        entries
    }
}

fn party_channel_name(this: &Waffle, party: &Party) -> String {
    let (guild_id, channel_id) = party.voice.clone().unwrap_or_default();
    this.guilds.iter().find(|g| g.id == guild_id).and_then(|g| g.channels.iter().find(|c| c.id == channel_id)).map(|c| c.name.clone()).unwrap_or_default()
}

// Discord's tinted header: a faint wash over the card.
fn header_fill(cx: &App) -> Hsla {
    palette::hover(cx)
}

// A section's bold line and its muted one, each cut to one line.
fn two_lines(first: String, second: Option<String>, cx: &App) -> Div {
    ui::col()
        .flex_1()
        .child(ui::line(first).text_size(ui::TEXT_SM).font_weight(FontWeight::SEMIBOLD).text_color(palette::text(cx)))
        .children(second.map(|s| ui::line(s).text_size(ui::TEXT_XS).text_color(palette::muted(cx))))
}

fn faces(id: &str, people: Vec<ui::Entry>, cx: &App) -> Stateful<Div> {
    let raised = palette::surface(palette::Level::Raised, cx);
    ui::AvatarGroup::listed(SharedString::from(id.to_owned()), "People", people, FACES, ui::AVATAR_MD, raised, palette::surface(palette::Level::Page, cx), palette::text(cx))
}

fn brand(kind: &str, ink: Hsla) -> Div {
    let path = crate::icons::brand_mark(kind).unwrap_or_default();
    ui::row().size_6().flex_none().justify_center().text_color(ink).child(gpui_kit::svg().path(path).size_full().text_color(ink))
}

// The label a stream's preview shows under the pointer.
pub(crate) fn watch_label(group: impl Into<SharedString>, label: &'static str) -> Div {
    ui::media_label().h_auto().px_4().py_1p5().text_size(ui::TEXT_SM).font_weight(FontWeight::MEDIUM).opacity(0.).group_hover(group.into(), |s| s.opacity(1.)).child(label)
}
