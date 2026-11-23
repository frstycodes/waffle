// Discord's Active Now column on the Friends page: friends grouped into parties by the voice channel or
// activity party they share, one card each. Behaviour and copy follow docs/discord-friends-active-now-re.md.
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{AnyElement, App, Context, Div, FontWeight, Hsla, SharedString, Window, component::Icon, div, prelude::*, px};

use crate::{
    Waffle,
    discord::{ActivityKind, Guild, Presence, Relation, RichPresence, User},
    home::FriendsTab,
    icons::Glyph,
    mascot::Mood,
    palette,
    sidebar::Nav,
    ui::{self, Squircle, Tone},
};
use crate::part::Column;

// Discord hides the column at this window width and below.
const BREAKPOINT: f32 = 1200.;
const WIDTH: f32 = 360.;

// Someone in a voice channel we can see, outside the server's AFK channel.
#[derive(Clone, Debug, PartialEq)]
pub struct Seat {
    pub user_id: String,
    pub guild_id: String,
    pub channel_id: String,
    pub streaming: bool,
}

#[derive(Clone, Debug)]
pub struct Party {
    pub key: String,
    pub members: Vec<User>,
    // (guild id, channel id) and everyone seated there, friends or not.
    pub voice: Option<(String, String)>,
    pub occupants: Vec<String>,
    // One per application, first come.
    pub activities: Vec<RichPresence>,
    // Members live on Go Live.
    pub streamers: Vec<String>,
}

impl Party {
    fn spotify_only(&self) -> bool {
        !self.activities.is_empty() && self.activities.iter().all(is_spotify)
    }
}

pub enum Subtitle {
    Text(String),
    // Unix milliseconds the shared activity started, shown as a running clock.
    Since(i64),
}

pub fn is_spotify(rich: &RichPresence) -> bool {
    rich.kind == ActivityKind::Listening && rich.name == "Spotify"
}

pub fn voice_seats(guilds: &[Guild]) -> Vec<Seat> {
    guilds
        .iter()
        .flat_map(|g| {
            g.voice_states.iter().filter_map(move |v| {
                let channel_id = v.channel_id.as_ref()?;
                let visible = g.channels.iter().any(|c| &c.id == channel_id);
                (visible && g.afk_channel_id.as_ref() != Some(channel_id)).then(|| Seat { user_id: v.user_id.clone(), guild_id: g.id.clone(), channel_id: channel_id.clone(), streaming: v.streaming })
            })
        })
        .collect()
}

// Groups friends by shared voice channel, else activity party, else alone; keeps parties with voice, an
// activity or a stream; drops any with a blocked member or occupant; sorts as Discord's NowPlayingViewStore.
// GAP: Discord also drops ignored users and voice that isn't discoverable; Waffle tracks neither.
pub fn parties(friends: &[User], blocked: &HashSet<&str>, presences: &HashMap<String, Presence>, seats: &[Seat]) -> Vec<Party> {
    let mut parties: Vec<Party> = Vec::new();
    for friend in friends.iter().filter(|f| !f.bot) {
        let seat = seats.iter().find(|s| s.user_id == friend.id);
        let rich = presences.get(&friend.id).and_then(|p| p.rich.as_ref());
        let key = match (seat, rich.and_then(|r| r.party_id.as_ref())) {
            (Some(seat), _) => format!("channel-{}", seat.channel_id),
            (None, Some(party)) => format!("party-{party}"),
            (None, None) => format!("user-{}", friend.id),
        };
        let at = match parties.iter().position(|p| p.key == key) {
            Some(at) => at,
            None => {
                let voice = seat.map(|s| (s.guild_id.clone(), s.channel_id.clone()));
                let occupants = seat.map(|s| seats.iter().filter(|o| o.guild_id == s.guild_id && o.channel_id == s.channel_id).map(|o| o.user_id.clone()).collect()).unwrap_or_default();
                parties.push(Party { key, members: Vec::new(), voice, occupants, activities: Vec::new(), streamers: Vec::new() });
                parties.len() - 1
            }
        };
        let party = &mut parties[at];
        party.members.push(friend.clone());
        if seat.is_some_and(|s| s.streaming) {
            party.streamers.push(friend.id.clone());
        }
        if let Some(rich) = rich.filter(|r| r.kind != ActivityKind::Custom) {
            let app = |r: &RichPresence| r.app_id.clone().unwrap_or_else(|| r.name.clone());
            if !party.activities.iter().any(|a| app(a) == app(rich)) {
                party.activities.push(rich.clone());
            }
        }
    }
    parties.retain(|p| {
        let shown = p.voice.is_some() || !p.activities.is_empty() || !p.streamers.is_empty();
        let blocked = p.members.iter().any(|m| blocked.contains(m.id.as_str())) || p.occupants.iter().any(|o| blocked.contains(o.as_str()));
        shown && !blocked
    });
    for party in &mut parties {
        party.members.sort_by_key(|m| m.name.to_lowercase());
    }
    parties.sort_by_key(|p| {
        (Reverse(p.members.len() > 1), Reverse(!p.streamers.is_empty()), Reverse(p.voice.is_some()), Reverse(!p.activities.is_empty()), p.spotify_only(), p.members[0].name.to_lowercase())
    });
    parties
}

pub fn title(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [a] => (*a).to_owned(),
        [a, b] => format!("{a} and {b}"),
        [a, b, rest @ ..] if rest.len() == 1 => format!("{a}, {b}, and 1 other"),
        [a, b, rest @ ..] => format!("{a}, {b}, and {} others", rest.len()),
    }
}

pub fn subtitle(party: &Party) -> Option<Subtitle> {
    let text = |s: String| Some(Subtitle::Text(s));
    match party.activities.as_slice() {
        [] if party.voice.is_some() => text("In a Voice Channel".into()),
        [] => None,
        [one] => match one.kind {
            ActivityKind::Playing | ActivityKind::Competing => Some(one.started.map_or_else(|| Subtitle::Text(format!("Playing {}", one.name)), Subtitle::Since)),
            ActivityKind::Streaming => text("Streaming on Twitch".into()),
            ActivityKind::Listening => text(format!("Listening to {}", one.name)),
            ActivityKind::Watching => text(format!("Watching {}", one.name)),
            ActivityKind::Custom => text("In a Voice Channel".into()),
        },
        _ => text("Playing Multiple Games".into()),
    }
}

// How far into a track `now` is, 0 to 1.
fn progress(started: i64, ends: i64, now: i64) -> f32 {
    if ends <= started {
        return 0.;
    }
    ((now - started) as f32 / (ends - started) as f32).clamp(0., 1.)
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn clock_since(start: i64) -> String {
    crate::call::clock(((now_ms() - start).max(0) / 1000) as u64)
}

impl Waffle {
    fn active_parties(&self) -> Vec<Party> {
        let friends: Vec<User> = self.relationships.iter().filter(|r| r.kind == Relation::Friend).map(|r| r.user.clone()).collect();
        let blocked: HashSet<&str> = self.relationships.iter().filter(|r| r.kind == Relation::Blocked).map(|r| r.user.id.as_str()).collect();
        parties(&friends, &blocked, &self.presences, &voice_seats(&self.guilds))
    }

    // The column right of the friends list: every tab but Add Friend, in windows wider than Discord's breakpoint.
    pub(crate) fn active_now_column(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.friends_tab == FriendsTab::AddFriend || window.viewport_size().width <= px(BREAKPOINT) {
            return None;
        }
        let parties = self.active_parties();
        if parties.iter().any(|p| p.activities.iter().any(|a| a.started.is_some())) {
            self.tick_active_now(cx);
        }
        let heading = div().text_size(ui::TEXT_LG).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child("Active Now");
        let body = match parties.is_empty() {
            true => quiet_state(cx).into_any_element(),
            false => ui::col().gap_3().children(parties.iter().map(|p| self.party_card(p, cx))).into_any_element(),
        };
        let column = div()
            .id("active-now")
            .w(px(WIDTH))
            .flex_none()
            .h_full()
            .border_l_1()
            .border_color(palette::line(cx))
            .overflow_y_scrollbar()
            .child(ui::col().p_4().gap_3().child(heading).child(body));
        Some(column.into_any_element())
    }

    // Repaints once a second for the play clocks and track progress, while the column is up.
    fn tick_active_now(&self, cx: &mut Context<Self>) {
        if self.active_now_ticking.replace(true) {
            return;
        }
        let ticking = self.active_now_ticking.clone();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let showing = this
                    .update(cx, |this, cx| {
                        let showing = this.guild_id.is_none() && this.channel.is_none() && !this.inbox_page && this.friends_tab != FriendsTab::AddFriend;
                        if showing {
                            this.redraw(Column::Main, cx);
                        }
                        showing
                    })
                    .unwrap_or(false);
                if !showing {
                    break;
                }
            }
            ticking.set(false);
        })
        .detach();
    }

    fn party_card(&self, party: &Party, cx: &mut Context<Self>) -> AnyElement {
        let raised = palette::surface(palette::Level::Raised, cx);
        let names: Vec<&str> = party.members.iter().map(|m| m.name.as_str()).collect();
        let people: Vec<ui::Entry> = party.members.iter().map(|m| ui::Entry::person(m.avatar.clone(), m.name.clone())).collect();
        let faces = ui::listed_stack(SharedString::from(format!("active-now-people-{}", party.key)), "People", people, 3, 32., raised, palette::surface(palette::Level::Group, cx), palette::muted(cx));
        let sub = subtitle(party).map(|s| match s {
            Subtitle::Text(text) => text,
            Subtitle::Since(start) => clock_since(start),
        });
        let who = ui::col()
            .flex_1()
            .child(ui::line(title(&names)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)))
            .children(sub.map(|s| ui::line(s).text_size(ui::TEXT_DETAIL).text_color(palette::muted(cx))));
        let head = ui::row().gap_2p5().child(faces).child(who).children(head_mark(party, cx));
        let first = party.members[0].clone();
        let hover = palette::line_strong(cx);
        ui::col()
            .id(SharedString::from(format!("active-now-{}", party.key)))
            .p_3()
            .gap_2p5()
            .squircle(ui::RADIUS_LG)
            .bg(raised)
            .border_1()
            .border_color(palette::line(cx))
            .cursor_pointer()
            .hover(move |s| s.border_color(hover))
            .child(head)
            .children(self.voice_section(party, cx))
            .children(party.activities.iter().map(|a| activity_section(a, cx)))
            .on_click(cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| this.open_profile(first.clone(), event.position(), cx)))
            .into_any_element()
    }

    fn voice_section(&self, party: &Party, cx: &mut Context<Self>) -> Option<Div> {
        let (guild_id, channel_id) = party.voice.clone()?;
        let guild = self.guilds.iter().find(|g| g.id == guild_id)?;
        let channel = guild.channels.iter().find(|c| c.id == channel_id)?.clone();
        let group = palette::surface(palette::Level::Group, cx);
        let occupants: Vec<ui::Entry> = party.occupants.iter().map(|id| ui::Entry::person(self.user_avatar(id), self.user_name(id))).collect();
        let channel_name = channel.name.clone();
        let joined = self.call.as_ref().is_some_and(|c| c.channel.id == channel.id);
        let join = ui::button(SharedString::from(format!("active-now-join-{channel_id}")), Tone::Secondary, cx)
            .small()
            .disabled(joined)
            .child(if joined { "Joined" } else { "Join" })
            .on_click(cx.listener(move |this, _, _, cx| this.join_voice_in(Some(guild_id.clone()), channel.clone(), cx)));
        let meta = ui::col()
            .flex_1()
            .child(
                ui::row()
                    .gap_1p5()
                    .child(ui::line(channel_name).text_size(ui::TEXT_SM).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)))
                    .when(!party.streamers.is_empty(), |d| d.child(ui::live_badge())),
            )
            .child(ui::line(guild.name.clone()).text_size(ui::TEXT_XS).text_color(palette::muted(cx)));
        Some(
            inner(cx)
                .child(div().flex_none().text_color(palette::muted(cx)).child(Icon::new(Glyph::VolumeHigh).size(ui::ICON_XL)))
                .child(meta)
                .child(ui::listed_stack(SharedString::from(format!("active-now-voice-{channel_id}")), "In voice", occupants, 3, 20., group, palette::surface(palette::Level::Raised, cx), palette::muted(cx)))
                // The button keeps its press, so the card's own click (the profile) never fires with it.
                .child(div().flex_none().on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| cx.stop_propagation()).child(join)),
        )
    }
}

// The mark beside the title: Spotify's notes, the game's icon, or a pad for several games.
fn head_mark(party: &Party, cx: &App) -> Option<AnyElement> {
    let muted = palette::muted(cx);
    let glyph = |icon: Icon, ink: Hsla| div().flex_none().text_color(ink).child(icon.size(ui::ICON_XL)).into_any_element();
    match party.activities.as_slice() {
        [] => None,
        [one] if is_spotify(one) => Some(glyph(Nav::MusicFill.icon(), palette::positive_ink(cx))),
        [one] => Some(match &one.image {
            Some(url) => ui::media(url.clone(), ui::all(ui::RADIUS_XS)).size_6().flex_none().object_fit(gpui_kit::ObjectFit::Cover).into_any_element(),
            None => glyph(crate::sidebar::activity_glyph(one.kind).unwrap_or(Nav::GameFill.icon()), muted),
        }),
        _ => Some(glyph(Nav::GameFill.icon(), muted)),
    }
}

fn inner(cx: &App) -> Div {
    ui::row().p_2p5().gap_2p5().squircle(ui::RADIUS_MD).bg(palette::surface(palette::Level::Group, cx))
}

fn art(image: Option<&String>, fallback: Icon, cx: &App) -> AnyElement {
    match image {
        Some(url) => ui::media(url.clone(), ui::all(ui::RADIUS_SM)).size_12().flex_none().object_fit(gpui_kit::ObjectFit::Cover).into_any_element(),
        None => ui::row().size_12().flex_none().squircle(ui::RADIUS_SM).bg(palette::surface(palette::Level::Raised, cx)).text_color(palette::muted(cx)).justify_center().child(fallback.size_6()).into_any_element(),
    }
}

// A game's rich presence, or a Spotify track with its progress.
fn activity_section(rich: &RichPresence, cx: &App) -> Div {
    let line = |text: String| ui::line(text).text_size(ui::TEXT_XS).text_color(palette::muted(cx));
    let name = |text: String| ui::line(text).text_size(ui::TEXT_SM).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx));
    let meta = ui::col().flex_1().gap_0p5();
    if is_spotify(rich) {
        let artists = rich.state.as_ref().map(|s| format!("by {}", s.replace("; ", ", ")));
        let track = rich.started.zip(rich.ends).map(|(start, end)| {
            let done = progress(start, end, now_ms());
            div().mt_1p5().h_1().w_full().rounded_xs().bg(palette::line_strong(cx)).overflow_hidden().child(div().h_full().w(gpui_kit::relative(done)).bg(palette::muted(cx)))
        });
        let meta = meta.child(name(rich.details.clone().unwrap_or_else(|| rich.name.clone()))).children(artists.map(line)).children(track);
        return inner(cx).child(art(rich.image.as_ref(), Nav::MusicFill.icon(), cx)).child(meta);
    }
    let state = rich.state.clone().map(|state| match rich.party {
        Some((current, max)) => format!("{state} ({current} of {max})"),
        None => state,
    });
    let meta = meta.child(name(rich.name.clone())).children(rich.details.clone().map(line)).children(state.map(line));
    inner(cx).child(art(rich.image.as_ref(), Icon::new(Glyph::Game), cx)).child(meta)
}

fn quiet_state(cx: &App) -> Div {
    ui::empty_state("active-now-empty", Mood::Idle, "It's quiet for now...", "When a friend starts an activity, like playing a game or hanging out on voice, we'll show it here!", cx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn friend(id: &str, name: &str) -> User {
        User { id: id.into(), name: name.into(), ..Default::default() }
    }

    fn seat(user: &str, channel: &str) -> Seat {
        Seat { user_id: user.into(), guild_id: "g".into(), channel_id: channel.into(), streaming: false }
    }

    fn playing(name: &str, party: Option<&str>) -> Presence {
        let rich = RichPresence { kind: ActivityKind::Playing, name: name.into(), app_id: Some(format!("app-{name}")), party_id: party.map(Into::into), started: Some(1), ..Default::default() };
        Presence { rich: Some(rich), ..Default::default() }
    }

    fn spotify() -> Presence {
        let rich = RichPresence { kind: ActivityKind::Listening, name: "Spotify".into(), party_id: Some("spotify:x".into()), ..Default::default() };
        Presence { rich: Some(rich), ..Default::default() }
    }

    fn keys(parties: &[Party]) -> Vec<&str> {
        parties.iter().map(|p| p.key.as_str()).collect()
    }

    #[test]
    fn groups_by_voice_then_party_then_user() {
        let friends = [friend("1", "ana"), friend("2", "bo"), friend("3", "cy"), friend("4", "di"), friend("5", "ed")];
        let presences = HashMap::from([("3".into(), playing("Celeste", Some("p"))), ("4".into(), playing("Celeste", Some("p"))), ("5".into(), playing("Hades", None))]);
        let seats = [seat("1", "lounge"), seat("2", "lounge")];
        let parties = parties(&friends, &HashSet::new(), &presences, &seats);
        assert_eq!(keys(&parties), ["channel-lounge", "party-p", "user-5"]);
        assert_eq!(parties[1].activities.len(), 1, "one entry per application");
    }

    #[test]
    fn keeps_only_parties_with_something_to_show() {
        let friends = [friend("1", "ana"), friend("2", "bo"), User { bot: true, ..friend("3", "bot") }];
        let custom = Presence { rich: Some(RichPresence { kind: ActivityKind::Custom, name: "brb".into(), ..Default::default() }), ..Default::default() };
        let presences = HashMap::from([("1".into(), custom), ("3".into(), playing("Hades", None))]);
        assert!(parties(&friends, &HashSet::new(), &presences, &[]).is_empty());
    }

    #[test]
    fn drops_parties_with_a_blocked_occupant() {
        let friends = [friend("1", "ana")];
        let seats = [seat("1", "lounge"), seat("9", "lounge")];
        assert!(parties(&friends, &HashSet::from(["9"]), &HashMap::new(), &seats).is_empty());
        assert_eq!(parties(&friends, &HashSet::new(), &HashMap::new(), &seats)[0].occupants, ["1", "9"]);
    }

    #[test]
    fn sorts_groups_streams_voice_activities_then_spotify_last() {
        let friends = [friend("1", "abe"), friend("2", "amy"), friend("3", "bea"), friend("4", "cat"), friend("5", "dan"), friend("6", "eve")];
        let presences = HashMap::from([("1".into(), spotify()), ("2".into(), playing("Hades", None)), ("3".into(), playing("Celeste", None))]);
        let seats = [Seat { streaming: true, ..seat("4", "stage") }, seat("5", "lounge"), seat("6", "lounge")];
        let parties = parties(&friends, &HashSet::new(), &presences, &seats);
        assert_eq!(keys(&parties), ["channel-lounge", "channel-stage", "user-2", "user-3", "party-spotify:x"]);
    }

    #[test]
    fn titles_name_two_then_count_the_rest() {
        assert_eq!(title(&["a"]), "a");
        assert_eq!(title(&["a", "b"]), "a and b");
        assert_eq!(title(&["a", "b", "c"]), "a, b, and 1 other");
        assert_eq!(title(&["a", "b", "c", "d"]), "a, b, and 2 others");
    }

    #[test]
    fn subtitles_follow_the_activity() {
        let text = |party: &Party| match subtitle(party) {
            Some(Subtitle::Text(t)) => t,
            Some(Subtitle::Since(at)) => format!("since {at}"),
            None => String::new(),
        };
        let base = Party { key: "k".into(), members: vec![friend("1", "a")], voice: None, occupants: Vec::new(), activities: Vec::new(), streamers: Vec::new() };
        let with = |kind, name: &str| Party { activities: vec![RichPresence { kind, name: name.into(), ..Default::default() }], ..base.clone() };
        assert_eq!(text(&Party { voice: Some(("g".into(), "c".into())), ..base.clone() }), "In a Voice Channel");
        assert_eq!(text(&with(ActivityKind::Listening, "Spotify")), "Listening to Spotify");
        assert_eq!(text(&with(ActivityKind::Streaming, "Twitch")), "Streaming on Twitch");
        assert_eq!(text(&with(ActivityKind::Watching, "YouTube")), "Watching YouTube");
        assert_eq!(text(&Party { activities: vec![RichPresence { started: Some(5), ..Default::default() }], ..base.clone() }), "since 5");
        let two = Party { activities: vec![RichPresence::default(), RichPresence::default()], ..base };
        assert_eq!(text(&two), "Playing Multiple Games");
    }

    #[test]
    fn progress_clamps_to_the_track() {
        assert_eq!(progress(0, 100, 50), 0.5);
        assert_eq!(progress(0, 100, 150), 1.);
        assert_eq!(progress(100, 100, 150), 0.);
    }
}
