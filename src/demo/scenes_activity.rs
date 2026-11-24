// Wren's activity cards, one kind per scene in the popout, all of them in the full profile and the DM panel,
// and the same people on the Friends page's Active Now.
use super::*;
use crate::discord::RichPresence;
use crate::profile::FullTab;

pub(super) const ACTIVITY_SCENES: &[&str] = &[
    "activity-game",
    "activity-spotify",
    "activity-watching",
    "activity-twitch",
    "activity-golive",
    "activity-voice",
    "activity-custom",
    "activity-bare",
    "activity-worst",
    "activity-stack",
    "activity-full",
    "activity-full-worst",
    "activity-dm",
    "active-now-stack",
    "active-now-worst",
];

const WREN: &str = "8388608";

pub(super) fn demo_activity_scene(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    if scene.starts_with("active-now") {
        return demo_active_now_stack(waffle, scene == "active-now-worst", cx);
    }
    let kind = scene.strip_prefix("activity-").unwrap_or(scene);
    let (activities, voice): (Vec<RichPresence>, Option<bool>) = match kind {
        "game" => (vec![game()], None),
        "spotify" => (vec![spotify()], None),
        "watching" => (vec![watching()], None),
        "twitch" => (vec![twitch()], None),
        "golive" => (vec![game()], Some(true)),
        "voice" => (Vec::new(), Some(false)),
        "custom" => (vec![custom()], None),
        "bare" => (vec![bare()], None),
        "worst" | "full-worst" => (vec![worst()], Some(true)),
        _ => (vec![game(), spotify(), watching(), twitch(), custom(), bare()], Some(true)),
    };
    match kind {
        "dm" => demo_profile_dm(waffle, window, cx),
        _ => demo_profile(waffle, cx),
    }
    seat_wren(waffle, voice);
    waffle.presences.entry(WREN.into()).or_default().activities = activities;
    if let Some(card) = waffle.profile_card.as_mut() {
        card.full = kind.starts_with("full").then_some(FullTab::Activity);
        card.recent = Some(Vec::new());
    }
}

// Wren in the lounge with three others, streaming or not, or out of voice.
fn seat_wren(waffle: &mut Waffle, voice: Option<bool>) {
    let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") else { return };
    guild.voice_states.retain(|v| v.user_id != WREN);
    let Some(streaming) = voice else { return };
    let seat = |id: &str, name: &str, streaming| VoiceState { guild_id: Some("1".into()), channel_id: Some("lounge".into()), user_id: id.into(), user: Some(user(id, name)), muted: false, deafened: false, server_muted: false, server_deafened: false, streaming, camera: false, suppress: false, requested_at: None };
    guild.voice_states.retain(|v| v.channel_id.as_deref() != Some("lounge"));
    guild.voice_states.extend([seat(WREN, "Wren", streaming), seat("12582912", "moss lantern", false), seat("16777216", "toma reed", false), seat("20971520", "ferris", false)]);
}

fn ago(ms: i64) -> i64 {
    chrono::Utc::now().timestamp_millis() - ms
}

fn game() -> RichPresence {
    RichPresence {
        kind: ActivityKind::Playing,
        name: "PUBG: BATTLEGROUNDS".into(),
        details: Some("Normal, Erangel, 58/99".into()),
        state: Some("4 Squad".into()),
        image: Some("brand/icons/blueprint.png".into()),
        small_image: Some("brand/icons/sticker.png".into()),
        small_text: Some("Gold III".into()),
        party: Some((3, 4)),
        party_id: Some("pubg-party".into()),
        started: Some(ago(7 * 60_000 + 2_000)),
        joinable: true,
        ..Default::default()
    }
}

fn spotify() -> RichPresence {
    RichPresence {
        kind: ActivityKind::Listening,
        name: "Spotify".into(),
        details: Some("Midnight City".into()),
        state: Some("M83; Anthony Gonzalez".into()),
        large_text: Some("Hurry Up, We're Dreaming".into()),
        image: Some("brand/icons/paper.png".into()),
        started: Some(ago(42_000)),
        ends: Some(ago(42_000) + 243_000),
        sync_id: Some("6GyFP1nfCDB8lbD2bG0Hq9".into()),
        party_id: Some(format!("spotify:{WREN}")),
        ..Default::default()
    }
}

fn watching() -> RichPresence {
    RichPresence {
        kind: ActivityKind::Watching,
        name: "Crunchyroll".into(),
        details: Some("Frieren: Beyond Journey's End".into()),
        state: Some("The Land Where Souls Rest".into()),
        large_text: Some("Season 2, Episode 5".into()),
        image: Some("demo/banner.png".into()),
        started: Some(ago(12 * 60_000)),
        buttons: vec!["Watch".into()],
        button_urls: vec!["https://www.crunchyroll.com".into()],
        ..Default::default()
    }
}

fn twitch() -> RichPresence {
    RichPresence { kind: ActivityKind::Streaming, name: "Twitch".into(), details: Some("any% honey run, day 12".into()), state: Some("Celeste".into()), url: Some("https://twitch.tv/wrenplays".into()), ..Default::default() }
}

fn custom() -> RichPresence {
    RichPresence {
        kind: ActivityKind::Playing,
        name: "Waffle Iron Simulator".into(),
        details: Some("Honeycomb Hills, level 12".into()),
        state: Some("Flipping for the high score".into()),
        image: Some("brand/icons/riso.png".into()),
        small_image: Some("brand/icons/peek.png".into()),
        started: Some(ago(65 * 60_000 + 9_000)),
        buttons: vec!["Play Now".into(), "Leaderboard".into()],
        button_urls: vec!["https://example.com/play".into()],
        ..Default::default()
    }
}

// An app with no art and no presence beyond its name.
fn bare() -> RichPresence {
    RichPresence { kind: ActivityKind::Playing, name: "Factorio".into(), started: Some(ago(3 * 3_600_000 + 61_000)), ..Default::default() }
}

fn worst() -> RichPresence {
    RichPresence {
        kind: ActivityKind::Playing,
        name: "The Legend of Heroes: Trails through Daybreak II Ultimate Deluxe Edition".into(),
        details: Some("Chapter 4: The Long Road Through the Honeycomb Marshes, Part Two".into()),
        state: Some("Searching for a party that will take a support main".into()),
        image: None,
        small_image: Some("brand/icons/peek.png".into()),
        party: Some((99, 100)),
        party_id: Some("worst".into()),
        started: Some(ago(27 * 3_600_000 + 3_000)),
        joinable: true,
        buttons: vec!["Join the Honeycomb Marshes Discord Community Server".into(), "View My Absolutely Enormous Profile".into()],
        ..Default::default()
    }
}

// Active Now with a voice group, a party of two in one game, one listener and one player, plus the worst case.
fn demo_active_now_stack(waffle: &mut Waffle, worst_case: bool, cx: &mut Context<Waffle>) {
    demo_active_now(waffle, false, cx);
    let party = || RichPresence { party_id: Some("pubg-party".into()), ..game() };
    for (id, name) in [("33554432", "pip"), ("37748736", "juniper")] {
        let activity = Activity { kind: ActivityKind::Playing, text: game().name, rest: Vec::new() };
        waffle.presences.insert(id.into(), Presence { status: Presence_::Online, activity: Some(activity), activities: vec![party()], ..Default::default() });
        waffle.relationships.push(Relationship { user: user(id, name), kind: Relation::Friend, since: None });
    }
    if !worst_case {
        return;
    }
    let long = "Featherstonehaugh Bartholomew the Third of Many Names";
    for n in 0..6 {
        let id = format!("{}", 50_000_000 + n);
        waffle.presences.insert(id.clone(), Presence { status: Presence_::Online, activities: vec![RichPresence { party_id: Some("pubg-party".into()), ..worst() }], ..Default::default() });
        waffle.relationships.push(Relationship { user: user(&id, &format!("{long} {n}")), kind: Relation::Friend, since: None });
    }
    waffle.presences.insert("29360128".into(), Presence { status: Presence_::Online, activities: vec![RichPresence { details: Some(worst().details.unwrap_or_default()), state: Some("An Artist With A Very Long Name; Another One; And A Third".into()), ..spotify() }], ..Default::default() });
}
