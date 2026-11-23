use super::*;

// The Friends page's Active Now: Wren and moss lantern in the hangout (moss on Go Live), rosa an hour into
// Celeste, mikey partway through a track. Empty: nobody in voice or doing anything.
pub(super) fn demo_active_now(waffle: &mut Waffle, empty: bool, cx: &mut Context<Waffle>) {
    waffle.restore = None;
    waffle.open_home(cx);
    if empty {
        waffle.guilds.iter_mut().for_each(|g| g.voice_states.clear());
        return;
    }
    let now = chrono::Utc::now().timestamp_millis();
    let celeste = crate::discord::RichPresence {
        kind: ActivityKind::Playing,
        name: "Celeste".into(),
        app_id: Some("celeste".into()),
        details: Some("Chapter 7".into()),
        state: Some("The Summit".into()),
        image: Some("brand/icons/riso.png".into()),
        started: Some(now - 72 * 60_000 - 4_000),
        ..Default::default()
    };
    let track = crate::discord::RichPresence {
        kind: ActivityKind::Listening,
        name: "Spotify".into(),
        details: Some("Midnight City".into()),
        state: Some("M83; Anthony Gonzalez".into()),
        image: Some("brand/icons/paper.png".into()),
        started: Some(now - 100_000),
        ends: Some(now + 143_000),
        party_id: Some("spotify:29360128".into()),
        ..Default::default()
    };
    for (id, name, rich) in [("25165824", "rosa", celeste), ("29360128", "mikey", track)] {
        let activity = Activity { kind: rich.kind, text: rich.name.clone(), rest: Vec::new() };
        waffle.presences.insert(id.into(), Presence { status: Presence_::Online, activity: Some(activity), rich: Some(rich), ..Default::default() });
        waffle.relationships.push(Relationship { user: user(id, name), kind: Relation::Friend, since: None });
    }
}

// Wren's popout with every section filled: theme, decoration, tag, badges, mutuals, an emoji bio, Member Since
// with the server, Friends Since, connections, games, and a joinable party with her voice channel peeking under it.
pub(super) fn demo_profile(waffle: &mut Waffle, cx: &mut Context<Waffle>) {
    let started = chrono::Utc::now().timestamp_millis() - 7 * 60_000 - 2_000;
    let rich = crate::discord::RichPresence {
        kind: ActivityKind::Playing,
        name: "PUBG: BATTLEGROUNDS".into(),
        app_id: Some("1402418717781921935".into()),
        details: Some("Normal, Erangel, 58/99".into()),
        state: Some("4 Squad".into()),
        image: Some("brand/icons/blueprint.png".into()),
        party: Some((4, 4)),
        started: Some(started),
        joinable: true,
        ..Default::default()
    };
    let playing = Activity { kind: ActivityKind::Playing, text: rich.name.clone(), rest: Vec::new() };
    waffle.presences.insert("8388608".into(), Presence { status: Presence_::Dnd, custom: None, activity: Some(playing), rich: Some(rich), ..Default::default() });
    if let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") {
        guild.voice_states.push(VoiceState { guild_id: Some("1".into()), channel_id: Some("lounge".into()), user_id: "8388608".into(), user: Some(user("8388608", "Wren")), muted: false, deafened: false, server_muted: false, server_deafened: false, streaming: false, camera: false, suppress: false, requested_at: None });
    }
    let mut zoe = user("8388608", "Wren");
    zoe.tag = Some(GuildTag { text: "wony".into(), badge: Some("brand/icons/sticker.png".into()) });
    zoe.decoration = Some("demo/decoration.png".into());
    waffle.open_profile(zoe, demo_at(520., 120.), cx);
    let badge = |name: &str, description: &str| crate::discord::Badge { description: description.into(), icon: format!("brand/icons/{name}.png") };
    let profile = crate::discord::Profile {
        username: "wren.lark".into(),
        bio: Some("LUCKY VICKY <a:aWonUwu:1328465377218269257>\n**gpui** fan, _probably_ in voice".into()),
        pronouns: Some("she/her".into()),
        banner: Some("demo/banner.png".into()),
        roles: vec!["r2".into()],
        theme: Some((0x9B1FA8, 0xE9A6C9)),
        badges: ["paper", "peek", "sticker", "riso", "blueprint"].iter().map(|n| badge(n, "Subscriber since 14 Nov 2024")).collect(),
        mutual_friends: vec![user("12582912", "moss lantern"), user("16777216", "toma reed"), user("20971520", "ferris")],
        mutual_friend_count: 5,
        mutual_server_count: 1,
        games: ["peek", "riso", "blueprint", "sticker", "paper"].iter().map(|n| format!("brand/icons/{n}.png")).collect(),
        joined: Some("2022-06-18T19:30:00+00:00".into()),
        connections: vec![
            Connection { kind: "spotify".into(), id: "31wrenlark".into(), name: "wren.lark".into(), verified: true },
            Connection { kind: "twitch".into(), id: "88".into(), name: "wrenplays_after_dark".into(), verified: false },
            Connection { kind: "xbox".into(), id: "77".into(), name: "WrenLark".into(), verified: false },
        ],
        ..Default::default()
    };
    if let Some(card) = waffle.profile_card.as_mut() {
        card.profile = Some(profile);
    }
}

// The top of a DM's history: a stranger with nothing said yet, an iconless group with nothing said, and
// the same group scrolled back to its first messages.
// Three requests (one with a name far too long for its row) and two spam, each with a profile for its
// username and mutual servers. `request-open` reads the newest in the chat, its accept bar over the composer.
pub(super) fn demo_requests(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    use crate::requests::{RequestsTab, snowflake_at};
    waffle.restore = None;
    waffle.guild_id = None;
    let fixtures = [
        ("29360128", "lunar.moth", "lunar.moth", 2, 120, false),
        ("33554432", "Gabe", "gabe_k", 1, 60 * 26, false),
        ("37748736", "Featherstonehaugh Bartholomew the Third of Many Names and Even More Titles", "bart.iii", 0, 60 * 72, false),
        ("41943040", "Free Nitro", "nitro.gift.2026", 0, 60, true),
        ("46137344", "crypto_dan", "crypto_dan", 1, 60 * 24 * 5, true),
    ];
    if scene != "requests-empty" {
        let now = chrono::Utc::now().timestamp_millis() as u64;
        for (id, name, username, mutual, minutes, spam) in fixtures {
            let person = user(id, name);
            let mut dm = channel(&format!("request-{id}"), name, ChannelKind::Dm, None);
            dm.raw_type = 1;
            dm.recipient = Some(person.clone());
            dm.recipients = vec![person];
            dm.last_message_id = Some(snowflake_at(now - minutes * 60_000).to_string());
            dm.props.message_request = !spam;
            dm.props.spam = spam;
            waffle.dm_requests.push(dm);
            let profile = crate::discord::Profile { username: username.into(), mutual_server_count: mutual, ..Default::default() };
            waffle.profiles.seed(&crate::profile::profile_key(id, None), profile);
        }
    }
    if scene != "request-open" {
        let tab = if scene == "requests-spam" { RequestsTab::Spam } else { RequestsTab::Requests };
        return waffle.open_requests(tab, cx);
    }
    let dm = waffle.dm_requests[0].clone();
    let ago = |mins: i64| (chrono::Utc::now() - chrono::Duration::minutes(mins)).to_rfc3339();
    let author = dm.recipient.clone().unwrap_or_default();
    let messages = vec![Message { id: "501".into(), channel_id: dm.id.clone(), author, timestamp: ago(120), content: "hey! saw your post in #art, do you take commissions?".into(), ..Default::default() }];
    waffle.feeds.insert(dm.id.clone(), Feed { messages: std::sync::Arc::new(messages), loaded: true, ..Default::default() });
    waffle.open_channel(dm, window, cx);
}

pub(super) fn demo_dm_start(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    let ferris = user("20971520", "ferris");
    let mut stranger = channel("dm-new", "ferris", ChannelKind::Dm, None);
    stranger.recipient = Some(ferris.clone());
    stranger.recipients = vec![ferris.clone()];
    let profile = crate::discord::Profile { username: "ferris.rs".into(), mutual_server_count: 2, ..Default::default() };
    waffle.profiles.seed(&crate::profile::profile_key(&ferris.id, None), profile);
    let mut group = channel("group", "Ranked squad", ChannelKind::Dm, None);
    group.raw_type = 3;
    group.recipients = vec![user("8388608", "Wren"), user("16777216", "toma reed"), ferris];
    waffle.dms.extend([group.clone(), stranger.clone()]);
    let ago = |mins: i64| (chrono::Utc::now() - chrono::Duration::minutes(mins)).to_rfc3339();
    let messages = match scene {
        "group-start" => vec![
            Message { id: "401".into(), channel_id: "group".into(), author: group.recipients[0].clone(), timestamp: ago(60), content: "made a group for ranked tonight".into(), ..Default::default() },
            Message { id: "402".into(), channel_id: "group".into(), author: group.recipients[1].clone(), timestamp: ago(58), content: "in, after dinner".into(), ..Default::default() },
            Message { id: "403".into(), channel_id: "group".into(), author: me_user(), timestamp: ago(2), content: "on in 5".into(), ..Default::default() },
        ],
        _ => Vec::new(),
    };
    let open = if scene == "dm-empty" { stranger } else { group };
    waffle.feeds.insert(open.id.clone(), Feed { messages: std::sync::Arc::new(messages), loaded: true, ..Default::default() });
    // Unread, so the group also stacks under Home on the rail.
    waffle.last_message.insert("group".into(), 200);
    waffle.read_states.insert("group".into(), ReadState { last_read: 100, mentions: 2 });
    waffle.restore = None;
    waffle.guild_id = None;
    waffle.open_channel(open, window, cx);
    // A bare `@` in a group DM: only its people, you last.
    if scene == "group-mention" {
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(500)).await;
            this.update_in(cx, |this, window, cx| {
                this.composer.update(cx, |state, cx| state.set_value("@", window, cx));
                this.autocomplete = this.suggest("@");
                cx.notify();
            })
            .ok();
        })
        .detach(); // lint-allow(detached-task): one-shot fixture step; ends after one timer and update, or when the view is gone
    }
}

// A five-member group you own, its member list on; the picker (two friends picked), Edit Group and Leave
// Group over it. The friends list runs long so the picker scrolls, one name long enough to truncate.
pub(super) fn demo_group(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    let rosa = user("12582912", "moss lantern");
    let members = [user("8388608", "Wren"), rosa.clone(), user("20971520", "ferris"), user("29360128", "Bartholomew Featherstonehaugh-Wright")];
    let friends = members.iter().cloned().chain([user("33554432", "saik"), user("37748736", "Laryke"), user("41943040", "h y p r"), user("46137344", "pouyas"), user("50331648", "valse")]);
    waffle.relationships.retain(|r| r.kind != Relation::Friend);
    waffle.relationships.extend(friends.map(|user| Relationship { user, kind: Relation::Friend, since: None }));
    let mut group = channel("road-trip", "road trip", ChannelKind::Dm, None);
    group.raw_type = 3;
    group.recipients = members.to_vec();
    group.props.owner_id = Some(me_user().id);
    waffle.dms.insert(0, group.clone());
    let ago = |mins: i64| (chrono::Utc::now() - chrono::Duration::minutes(mins)).to_rfc3339();
    let messages = vec![
        Message { id: "501".into(), channel_id: group.id.clone(), author: rosa, timestamp: ago(40), content: "who's driving the first leg".into(), ..Default::default() },
        Message { id: "502".into(), channel_id: group.id.clone(), author: me_user(), timestamp: ago(3), content: "me, then Wren after lunch".into(), ..Default::default() },
    ];
    waffle.feeds.insert(group.id.clone(), Feed { messages: std::sync::Arc::new(messages), loaded: true, ..Default::default() });
    waffle.restore = None;
    waffle.guild_id = None;
    waffle.open_channel(group.clone(), window, cx);
    waffle.right_panel = crate::RightPanel::Members;
    match scene {
        "group-picker" => {
            waffle.open_home(cx);
            waffle.open_group_picker(None, window, cx);
            for pick in [user("8388608", "Wren"), user("33554432", "saik")] {
                waffle.toggle_group_pick(pick, window, cx);
            }
        }
        "group-edit" => waffle.open_edit_group(group.id, window, cx),
        "group-leave" => waffle.ask_leave_group(group.id, cx),
        _ => {}
    }
}

// Real Discord applications (id, icon hash from /applications/detectable), so their icons load from the CDN.
pub(super) const HADES: (&str, &str) = ("Hades II", "1237101681695522867");
pub(super) const FACTORIO: (&str, &str) = ("Factorio", "358422126602223616");
pub(super) const APEX: (&str, &str) = ("Apex Legends", "542075586886107149");

// Games in hangout: you (kiwi) detected playing Hades II, moss lantern and Wren by their presences.
pub(super) fn demo_games(waffle: &mut Waffle) {
    for ((_, app_id), icon) in [(HADES, "e8aaa81a90f8c36e88014722906884b6"), (FACTORIO, "7624a1d91d9828ed2f2dc3510bbe09e6"), (APEX, "91fac0600c5b6527c1aea95a93c6a8e0")] {
        waffle.games.seed_icon(app_id, &crate::discord::app_icon_url(app_id, icon));
    }
    waffle.games.mine = Some(crate::games::Game { pid: 501, app_id: HADES.1.into(), name: HADES.0.into(), icon: Some("e8aaa81a90f8c36e88014722906884b6".into()), started: chrono::Utc::now().timestamp_millis() });
    for (id, (name, app_id)) in [("12582912", APEX), ("8388608", FACTORIO)] {
        let started = Some(chrono::Utc::now().timestamp_millis() - 47 * 60_000);
        let rich = crate::discord::RichPresence { kind: ActivityKind::Playing, name: name.into(), app_id: Some(app_id.into()), started, ..Default::default() };
        let activity = Activity { kind: ActivityKind::Playing, text: name.into(), rest: Vec::new() };
        waffle.presences.insert(id.into(), Presence { status: Presence_::Online, custom: None, activity: Some(activity), rich: Some(rich), ..Default::default() });
    }
}

// A connected call in hangout with made-up stats; the session itself never reaches a server.
pub(super) fn demo_call(waffle: &mut Waffle) {
    demo_call_in(waffle, "lounge");
}

pub(super) fn demo_call_in(waffle: &mut Waffle, channel_id: &str) {
    let Some(lounge) = waffle.guild().and_then(|g| g.channels.iter().find(|c| c.id == channel_id).cloned()) else { return };
    let (session, _) = crate::voice::join("4194304".into(), Some("1".into()), channel_id.into(), crate::voice::GatewayHandle::new().0, Default::default());
    let stats = crate::voice::Stats {
        ping_ms: 42,
        median_ping_ms: 47,
        last_pings: vec![44, 51, 47, 40, 62, 45, 43, 48, 120, 52, 46, 44, 41, 47, 50, 42],
        outbound_loss_pct: Some(0.4),
        playout_delay_ms: Some(60),
        endpoint: "c-bom06-1a2b3c4d.discord.media".into(),
        server_ip: "162.159.0.1".into(),
        encryption: "aead_aes256_gcm_rtpsize".into(),
        dave: true,
    };
    waffle.call = Some(crate::Call {
        session,
        guild_id: Some("1".into()),
        channel: lounge,
        connected: true,
        stage: crate::voice::Stage::Connected,
        stats: Some(stats),
        level: (-30., true),
        speaking: ["8388608".to_owned()].into_iter().collect(),
        last_spoke: Default::default(),
        cameras: Default::default(),
        camera_on: false,
        watching: Vec::new(),
        focus: None,
        ended: Vec::new(),
        sharing: None,
        share_source: None, live_since: None, self_view: None, issues: Default::default(), share_label: None,
        joined: std::time::Instant::now().checked_sub(std::time::Duration::from_secs(5047)).unwrap_or_else(std::time::Instant::now),
        _events: gpui_kit::Task::ready(()),
    });
    waffle.call_view = true;
    waffle.restore = None;
}

// In hangout: two unread messages there (a mention of us in the previews scenes), and two mentions
// waiting in study-hall, whose other unread stays hidden since we aren't in it.
pub(super) fn demo_voice_chat(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    demo_call(waffle);
    waffle.call_pointer_inside = true;
    let said = |id: &str, author: User, content: &str| Message {
        id: id.into(),
        channel_id: "lounge".into(),
        guild_id: Some("1".into()),
        author,
        content: content.into(),
        timestamp: "2026-11-22T21:41:00Z".into(),
        ..Default::default()
    };
    let (wren, moss, toma) = (user("8388608", "Wren"), user("12582912", "moss lantern"), user("16777216", "toma reed"));
    waffle.feeds.insert("lounge".into(), Feed { messages: std::sync::Arc::new(vec![said("500", toma, "who's carrying tonight")]), loaded: true, ..Default::default() });
    waffle.read_states.insert("lounge".into(), ReadState { last_read: 500, mentions: 0 });
    waffle.read_states.insert("hub".into(), ReadState { last_read: 100, mentions: 2 });
    waffle.last_message.insert("hub".into(), 900);
    let mut arrivals = vec![
        said("501", wren.clone(), "anyone up for ranked after this?"),
        said("502", moss, "give me ten minutes, I'm finishing a render that has been stuck at 97% for the better part of an hour and I refuse to cancel it now"),
    ];
    if matches!(scene, "call-previews" | "call-chat") {
        let mut mention = said("503", wren, "<@4194304> get in here, we need a fourth");
        mention.mentions = vec![me_user()];
        arrivals.push(mention);
    }
    for message in arrivals {
        waffle.on_event(Event::MessageCreate(Box::new(message)), cx);
    }
    // Held as if hovered, so the cards outlast their 10 s.
    waffle.call_previews.freeze(true, std::time::Instant::now());
    match scene {
        "sidebar-voice-chat" => waffle.call_view = false,
        "call-chat" => {
            if let Some(lounge) = waffle.call.as_ref().map(|c| c.channel.clone()) {
                waffle.open_call_chat(lounge, window, cx);
            }
        }
        _ => {}
    }
}

// #town-hall, a stage: four speakers (Wren moderates), fourteen listeners with three hands up and one
// invite pending. `stage` has us moderating from the speakers; `stage-audience` puts our hand up, 2nd in
// line, without moderating; `stage-invited` has us invited. The waiting and start scenes have no stage
// running yet; the dialog scenes open the stage's dialogs over it.
pub(super) fn demo_stage(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    use crate::discord::{Overwrite, StageInstance, perm};
    let live = !matches!(scene, "stage-waiting" | "stage-start" | "stage-start-dialog");
    let moderator = !matches!(scene, "stage-audience" | "stage-invited" | "stage-waiting");
    let ago = |secs: i64| (chrono::Utc::now() - chrono::Duration::seconds(secs)).to_rfc3339();
    let mine = match scene {
        "stage-audience" => (true, Some(ago(95))),
        "stage-invited" => (false, Some(ago(30))),
        _ if live && moderator => (false, None),
        _ => (true, None),
    };
    let state = |id: &str, name: &str, suppress: bool, requested_at: Option<String>| VoiceState {
        guild_id: Some("1".into()),
        channel_id: Some("town-hall".into()),
        user_id: id.into(),
        user: Some(user(id, name)),
        suppress,
        requested_at,
        ..Default::default()
    };
    let mut states = vec![
        state("8388608", "Wren", false, None),
        state("12582912", "moss lantern", false, None),
        VoiceState { muted: true, ..state("16777216", "toma reed", false, None) },
        state("4194304", "kiwi", mine.0, mine.1),
    ];
    if mine.0 {
        states.push(state("20971520", "ferris", false, None));
    }
    let listeners = ["Jun", "Rosa", "Lea", "Omar", "Pia", "Kai", "Noor", "Eli", "Bartholomew Featherstonehaugh-Wright", "Ana", "Theo", "Mira", "Sol", "Ivy"];
    states.extend(listeners.iter().enumerate().map(|(n, name)| {
        let id = (31_000_000 + n).to_string();
        match *name {
            "Jun" => state(&id, name, true, Some(ago(140))),
            "Lea" => state(&id, name, true, Some(ago(40))),
            "Kai" => state(&id, name, true, Some(ago(3))),
            "Noor" => state(&id, name, false, Some(ago(20))),
            _ => state(&id, name, true, None),
        }
    }));
    let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") else { return };
    let mut hall = channel("town-hall", "town-hall", ChannelKind::Voice, Some("c1"));
    hall.raw_type = crate::stage::STAGE;
    hall.overwrites = vec![Overwrite { id: "1".into(), allow: perm::REQUEST_TO_SPEAK, deny: 0, member: false }];
    guild.channels.push(hall);
    guild.voice_states.extend(states);
    // Council moderates every stage; Wren holds it.
    if let Some(council) = guild.roles.iter_mut().find(|r| r.id == "r1") {
        council.permissions |= perm::STAGE_MODERATOR;
    }
    if !moderator {
        guild.owner_id = Some("8388608".into());
        if let Some(everyone) = guild.roles.iter_mut().find(|r| r.id == "1") {
            everyone.permissions = crate::discord::VIEW_CHANNEL | perm::CONNECT | perm::SEND_MESSAGES;
        }
    }
    if live {
        guild.stage_instances.push(StageInstance { channel_id: "town-hall".into(), topic: "Season 7 patch review: every change, ranked".into() });
    }
    waffle.member_roles.entry("1".into()).or_default().insert("8388608".into(), vec!["r1".into()]);
    demo_call_in(waffle, "town-hall");
    waffle.call_pointer_inside = true;
    match scene {
        "stage-requests" => cx.set_global(crate::stage_view::StageRequests(Some("town-hall".into()))),
        "stage-start-dialog" | "stage-edit" => waffle.open_stage_topic("1".into(), "town-hall".into(), window, cx),
        "stage-end" => waffle.open_end_stage("town-hall".into(), cx),
        "stage-full" => waffle.open_stage_full(cx),
        _ => {}
    }
}

pub(super) fn demo_thread() -> Channel {
    let mut thread = channel("t1", "gpui list virtualisation", ChannelKind::Thread, Some("general"));
    thread.thread = Some(crate::discord::ThreadMeta { joined: true, message_count: 12, ..Default::default() });
    thread
}

pub(super) fn first_thread_chip(message: &mut Message) {
    message.thread = Some(demo_thread());
}

pub(super) fn demo_dm_call(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    let zoe = user("8388608", "Wren");
    let ago = |mins: i64| (chrono::Utc::now() - chrono::Duration::minutes(mins)).to_rfc3339();
    let record = |id: &str, author: &User, start: i64, end: Option<i64>, joined: &[&str]| Message {
        id: id.into(),
        channel_id: "dm".into(),
        author: author.clone(),
        timestamp: ago(start),
        call: Some(crate::discord::CallRecord { participants: joined.iter().map(|p| p.to_string()).collect(), ended: end.map(ago) }),
        ..Default::default()
    };
    let text = |id: &str, author: &User, at: i64, content: &str| Message { id: id.into(), channel_id: "dm".into(), author: author.clone(), timestamp: ago(at), content: content.into(), ..Default::default() };
    let me = me_user();
    let messages = vec![
        record("301", &me, 180, Some(168), &["4194304", "8388608"]),
        record("302", &zoe, 90, Some(89), &["8388608"]),
        // A two-line message and a one-line one: each lays out at the feed's width, never one glyph per line.
        text("303", &zoe, 89, "call me when you're back\nnot before 9 though"),
        text("304", &me, 3, "ardia browser use garnu"),
    ];
    waffle.feeds.insert("dm".into(), Feed { messages: std::sync::Arc::new(messages), loaded: true, ..Default::default() });
    let state = VoiceState { guild_id: None, channel_id: Some("dm".into()), user_id: zoe.id.clone(), user: Some(zoe), muted: false, deafened: false, server_muted: false, server_deafened: false, streaming: false, camera: false, suppress: false, requested_at: None };
    waffle.on_event(Event::VoiceState(state, None), cx);
    let ringing = if matches!(scene, "call-joinable" | "call-dm" | "call-dm-camera") { vec![] } else { vec![me.id.clone()] };
    waffle.on_event(Event::Call { channel_id: "dm".into(), ringing }, cx);
    if scene.starts_with("call-dm") {
        let dm = waffle.dms[0].clone();
        let (session, _) = crate::voice::join(me.id.clone(), None, dm.id.clone(), crate::voice::GatewayHandle::new().0, Default::default());
        let cameras = match scene {
            "call-dm-camera" => [("8388608".to_owned(), demo_frame(480, 640, 90))].into_iter().collect(),
            _ => Default::default(),
        };
        waffle.call = Some(crate::Call {
            session,
            guild_id: None,
            channel: dm,
            connected: true,
            stage: crate::voice::Stage::Connected,
            stats: None,
            level: (-30., false),
            speaking: ["8388608".to_owned()].into_iter().collect(),
            last_spoke: Default::default(),
            cameras,
            camera_on: false,
            watching: Vec::new(),
            focus: None,
            ended: Vec::new(),
            sharing: None,
            share_source: None, live_since: None, self_view: None, issues: Default::default(), share_label: None,
            joined: std::time::Instant::now().checked_sub(std::time::Duration::from_secs(724)).unwrap_or_else(std::time::Instant::now),
            _events: gpui_kit::Task::ready(()),
        });
    }
    if scene != "call-banner" {
        let dm = waffle.dms[0].clone();
        waffle.restore = None;
        waffle.guild_id = None;
        waffle.open_channel(dm, window, cx);
    }
}
