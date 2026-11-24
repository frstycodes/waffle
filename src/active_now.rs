// Discord's Active Now column on the Friends page: friends grouped into parties by the voice channel or
// activity party they share, one card each. Behaviour and copy follow docs/discord-friends-active-now-re.md.
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{AnyElement, App, Context, Div, FontWeight, Window, div, prelude::*, px};

use crate::part::Column;
use crate::{
    Waffle,
    discord::{ActivityKind, Guild, Presence, Relation, RichPresence, User},
    home::FriendsTab,
    palette,
    profile::activity_text::is_spotify,
    ui,
};

mod sections;
pub(crate) use sections::watch_label;


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
    pub streamers: Vec<String>, // lint-allow(unbounded-growth): one per member live on Go Live in a party, rebuilt on every render and never kept
}

impl Party {
    fn spotify_only(&self) -> bool {
        !self.activities.is_empty() && self.activities.iter().all(is_spotify)
    }
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
        let rich = presences.get(&friend.id).and_then(Presence::rich);
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
        for rich in presences.get(&friend.id).map(|p| p.activities.as_slice()).unwrap_or_default().iter().filter(|r| r.kind != ActivityKind::Custom) {
            if !party.activities.iter().any(|a| app_key(a) == app_key(rich)) {
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

// One entry per application, as Discord de-duplicates them.
pub fn app_key(rich: &RichPresence) -> String {
    rich.app_id.clone().unwrap_or_else(|| rich.name.clone())
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

// The line under the names: what the one person is doing, else what the group shares, else Discord's catch-alls.
pub fn subtitle(party: &Party, now: i64) -> Option<String> {
    if let ([_], Some(first)) = (party.members.as_slice(), party.activities.first()) {
        return Some(match first.kind {
            ActivityKind::Playing | ActivityKind::Competing => since(first, now),
            ActivityKind::Streaming => "Streaming on Twitch".into(),
            ActivityKind::Listening => format!("Listening to {}", first.name),
            ActivityKind::Watching => format!("Watching {}", first.name),
            ActivityKind::Custom => "In a Voice Channel".into(),
        });
    }
    let games: Vec<&RichPresence> = party.activities.iter().filter(|a| !is_spotify(a)).collect();
    match games.as_slice() {
        [] if party.voice.is_some() => Some("In a Voice Channel".into()),
        [] if !party.activities.is_empty() => Some("Listening to Spotify".into()),
        [] => None,
        [game] => Some(since(game, now)),
        _ => Some("Playing Multiple Games".into()),
    }
}

// "{game} – 12m": how long they've played, in the largest whole unit; "just now" under a minute.
pub fn since(rich: &RichPresence, now: i64) -> String {
    let Some(start) = rich.started else { return rich.name.clone() };
    let minutes = (now - start).max(0) / 60_000;
    match minutes {
        0 => format!("{} - just now", rich.name),
        m if m < 60 => format!("{} \u{2013} {m}m", rich.name),
        m if m < 24 * 60 => format!("{} \u{2013} {}h", rich.name, m / 60),
        m => format!("{} \u{2013} {}d", rich.name, m / (24 * 60)),
    }
}

pub(crate) fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
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
        let heading = div().pt_2().pb_0p5().text_size(ui::TEXT_LG).line_height(ui::LINE_LG).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child("Active Now");
        let body = match parties.is_empty() {
            true => quiet_state(cx).into_any_element(),
            false => ui::col().gap_3().children(parties.iter().map(|p| self.party_card(p, cx))).into_any_element(),
        };
        let column = div()
            .id("active-now")
            .w(px(WIDTH))
            .flex_none()
            .h_full()
            .overflow_y_scrollbar()
            .child(ui::col().pt_1p5().px_4().pb_4().gap_3().child(heading).child(body));
        Some(column.into_any_element())
    }

    // Repaints once a second for the play clocks, while the column is up.
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
        .detach(); // lint-allow(detached-task): ends itself when the panel stops showing, holding a WeakEntity
    }
}

// Discord's empty card: no art here, only the line and what will show up.
fn quiet_state(cx: &App) -> Div {
    ui::panel(cx)
        .p_4()
        .gap_1()
        .items_center()
        .text_center()
        .child(div().text_size(ui::TEXT_MD).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child("It's quiet for now..."))
        .child(div().text_size(ui::TEXT_SM).text_color(palette::muted(cx)).child("When a friend starts an activity\u{2014}like playing a game or hanging out on voice\u{2014}we\u{2019}ll show it here!"))
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
        Presence { activities: vec![rich], ..Default::default() }
    }

    fn spotify() -> Presence {
        let rich = RichPresence { kind: ActivityKind::Listening, name: "Spotify".into(), party_id: Some("spotify:x".into()), ..Default::default() };
        Presence { activities: vec![rich], ..Default::default() }
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
        let custom = Presence { activities: vec![RichPresence { kind: ActivityKind::Custom, name: "brb".into(), ..Default::default() }], ..Default::default() };
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
        let now = 10 * 86_400_000;
        let base = Party { key: "k".into(), members: vec![friend("1", "a")], voice: None, occupants: Vec::new(), activities: Vec::new(), streamers: Vec::new() };
        let with = |kind, name: &str| Party { activities: vec![RichPresence { kind, name: name.into(), ..Default::default() }], ..base.clone() };
        let pair = |party: Party| Party { members: vec![friend("1", "a"), friend("2", "b")], ..party };
        assert_eq!(subtitle(&Party { voice: Some(("g".into(), "c".into())), ..base.clone() }, now).as_deref(), Some("In a Voice Channel"));
        assert_eq!(subtitle(&with(ActivityKind::Listening, "Spotify"), now).as_deref(), Some("Listening to Spotify"));
        assert_eq!(subtitle(&with(ActivityKind::Streaming, "Twitch"), now).as_deref(), Some("Streaming on Twitch"));
        assert_eq!(subtitle(&with(ActivityKind::Watching, "YouTube"), now).as_deref(), Some("Watching YouTube"));
        let hades = |mins: i64| Party { activities: vec![RichPresence { name: "Hades".into(), started: Some(now - mins * 60_000), ..Default::default() }], ..base.clone() };
        assert_eq!(subtitle(&hades(0), now).as_deref(), Some("Hades - just now"));
        assert_eq!(subtitle(&hades(12), now).as_deref(), Some("Hades \u{2013} 12m"));
        assert_eq!(subtitle(&hades(150), now).as_deref(), Some("Hades \u{2013} 2h"));
        assert_eq!(subtitle(&hades(3 * 24 * 60), now).as_deref(), Some("Hades \u{2013} 3d"));
        assert_eq!(subtitle(&pair(with(ActivityKind::Listening, "Spotify")), now).as_deref(), Some("Listening to Spotify"));
        let two = pair(Party { activities: vec![RichPresence { name: "x".into(), ..Default::default() }, RichPresence { name: "y".into(), ..Default::default() }], ..base.clone() });
        assert_eq!(subtitle(&two, now).as_deref(), Some("Playing Multiple Games"));
        assert_eq!(subtitle(&pair(base), now), None);
    }
}
