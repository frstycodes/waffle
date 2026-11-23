use super::*;

// `nav-pins`: Night Shift, a Community server with opt-in on (Show all channels unticked): two pinned
// channels above the rest, a few opted-in channels, #art kept in view by a mention, #clips by an
// unread thread you joined, Gaming (two people in it) under Active Now, and #memes and Studio
// hidden. Home lists six DMs with two pinned ones first.
pub(super) fn demo_pins(waffle: &mut Waffle, window: &mut Window, cx: &mut Context<Waffle>) {
    use crate::discord::{ChannelSettings, GuildSettings, channel_flag, guild_flag};
    let Some(template) = waffle.guilds.first().cloned() else { return };
    let layout = [
        ("ns-welcome", "welcome", ChannelKind::Text, None),
        ("ns-info", "Information", ChannelKind::Category, None),
        ("ns-announcements", "announcements", ChannelKind::Text, Some("ns-info")),
        ("ns-rules", "rules", ChannelKind::Text, Some("ns-info")),
        ("ns-text", "Text", ChannelKind::Category, None),
        ("ns-general", "general", ChannelKind::Text, Some("ns-text")),
        ("ns-off-topic", "off-topic", ChannelKind::Text, Some("ns-text")),
        ("ns-memes", "memes", ChannelKind::Text, Some("ns-text")),
        ("ns-art", "art", ChannelKind::Text, Some("ns-text")),
        ("ns-clips", "clips", ChannelKind::Text, Some("ns-text")),
        ("ns-voice", "Voice", ChannelKind::Category, None),
        ("ns-lounge", "Lounge", ChannelKind::Voice, Some("ns-voice")),
        ("ns-gaming", "Gaming", ChannelKind::Voice, Some("ns-voice")),
        ("ns-studio", "Studio", ChannelKind::Voice, Some("ns-voice")),
    ];
    let channels: Vec<Channel> = layout
        .iter()
        .enumerate()
        .map(|(i, &(id, name, kind, parent))| Channel { position: i as i64, ..channel(id, name, kind, parent) })
        .collect();
    let general = channels.iter().find(|c| c.id == "ns-general").cloned();
    let mut highlights = channel("ns-clips-best", "best of the week", ChannelKind::Thread, Some("ns-clips"));
    highlights.thread = Some(crate::discord::ThreadMeta { joined: true, message_count: 8, ..Default::default() });
    let gaming = |id: &str, name: &str| VoiceState { guild_id: Some("6".into()), channel_id: Some("ns-gaming".into()), user_id: id.into(), user: Some(user(id, name)), ..Default::default() };
    let voice_states = vec![gaming("12582912", "moss lantern"), gaming("16777216", "toma reed")];
    waffle.guilds.push(Guild { id: "6".into(), name: "Night Shift".into(), channels, voice_states, threads: vec![highlights], features: vec!["COMMUNITY".into()], ..template });

    let pinned = channel_flag::FAVORITED | channel_flag::OPT_IN_ENABLED;
    let flags = [("ns-general", pinned), ("ns-announcements", channel_flag::FAVORITED), ("ns-info", channel_flag::OPT_IN_ENABLED), ("ns-welcome", channel_flag::OPT_IN_ENABLED), ("ns-off-topic", channel_flag::OPT_IN_ENABLED), ("ns-lounge", channel_flag::OPT_IN_ENABLED)];
    let channels = flags.iter().map(|&(id, flags)| (id.to_owned(), ChannelSettings { notifications: 3, flags, ..Default::default() })).collect();
    waffle.guild_settings.insert("6".into(), GuildSettings { flags: guild_flag::OPT_IN_CHANNELS_ON, channels, ..Default::default() });
    waffle.last_message.insert("ns-art".into(), 200);
    waffle.read_states.insert("ns-art".into(), ReadState { last_read: 100, mentions: 1 });
    waffle.last_message.insert("ns-clips-best".into(), 200);
    waffle.read_states.insert("ns-clips-best".into(), ReadState { last_read: 100, mentions: 0 });

    // Newest message first, as the list keeps them; "dm-mira" and the group are pinned.
    let Some(wren) = waffle.dms.first().cloned() else { return };
    let people = [("dm-toma", "toma reed", "16777216"), ("dm-moss", "moss lantern", "12582912"), ("dm-mira", "Mira", "20971520"), ("dm-ferris", "ferris", "25165824")];
    for (id, name, user_id) in people {
        waffle.dms.push(Channel { id: id.into(), name: name.into(), recipient: Some(user(user_id, name)), ..wren.clone() });
    }
    let mut group = channel("dm-squad", "Ranked squad", ChannelKind::Dm, None);
    group.recipients = vec![user("12582912", "moss lantern"), user("16777216", "toma reed"), user("8388608", "Wren")];
    waffle.dms.push(group);
    let dm_pins = ["dm-mira", "dm-squad"].map(|id| (id.to_owned(), ChannelSettings { notifications: 3, flags: channel_flag::FAVORITED, ..Default::default() }));
    waffle.guild_settings.insert("@me".into(), GuildSettings { channels: dm_pins.into_iter().collect(), ..Default::default() });

    waffle.restore = None;
    waffle.guild_id = Some("6".into());
    if let Some(general) = general {
        waffle.open_channel(general, window, cx);
    }
}

// Mod View on Wren before anything answers: the skeleton.
pub(super) fn demo_mod_view_loading(waffle: &mut Waffle, cx: &mut Context<Waffle>) {
    mod_view_fixture(waffle);
    waffle.demo_mod_view_loading("1".into(), "8388608".into(), cx);
}

// Moderation rights for us in server 1, and Wren's two roles there.
pub(super) fn mod_view_fixture(waffle: &mut Waffle) {
    if let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") {
        for role in &mut guild.roles {
            role.permissions = match role.id.as_str() {
                "r1" => crate::discord::perm::MODERATE_MEMBERS | crate::discord::perm::MANAGE_MESSAGES | (1 << 17),
                "r2" => crate::discord::perm::SEND_MESSAGES,
                _ => role.permissions,
            };
        }
    }
    waffle.member_roles.entry("1".into()).or_default().insert("8388608".into(), vec!["r1".into(), "r2".into()]);
    let members = waffle.members.entry("1".into()).or_default();
    members.insert("8388608".into(), user("8388608", "Wren"));
    members.insert("12582912".into(), user("12582912", "moss lantern"));
}

// Three apps' commands, the shapes `app_commands::parse_index` produces.
pub(super) fn demo_commands() -> crate::app_commands::CommandIndex {
    use crate::app_commands::{App, AppCommand, Choice, CommandIndex, CommandOption, OptionKind};
    let app = |id: &str, name: &str| App { id: id.into(), name: name.into(), icon: None };
    let option = |name: &str, kind: OptionKind, required: bool, description: &str| CommandOption { kind, name: name.into(), description: description.into(), required, ..Default::default() };
    let command = |id: &str, app: &str, path: &str, description: &str, options: Vec<CommandOption>| AppCommand {
        id: id.into(),
        application_id: app.into(),
        version: "1".into(),
        path: path.split(' ').map(str::to_owned).collect(),
        description: description.into(),
        options,
        raw: serde_json::json!({ "id": id, "name": path.split(' ').next() }),
        ..Default::default()
    };
    let query = CommandOption { autocomplete: true, ..option("query", OptionKind::String, true, "A song name or link") };
    let volume = CommandOption { min_value: Some(0.), max_value: Some(100.), ..option("volume", OptionKind::Integer, false, "Loudness, 0 to 100") };
    let category = CommandOption {
        choices: ["Music", "Movies", "Games"].map(|c| Choice { name: c.into(), name_localized: None, value: serde_json::json!(c.to_lowercase()) }).to_vec(),
        ..option("category", OptionKind::String, true, "What the questions are about")
    };
    CommandIndex {
        apps: vec![app("w", "Wavebot"), app("p", "Pollster"), app("q", "Quizzy")],
        commands: vec![
            command("10", "w", "play", "Play a song or playlist in your voice channel", vec![query, volume, option("shuffle", OptionKind::Boolean, false, "Mix up the order"), option("loop", OptionKind::Boolean, false, "Start again when it ends")]),
            command("11", "w", "playlist", "Queue one of your saved playlists", vec![option("name", OptionKind::String, true, "The playlist's name")]),
            command("12", "w", "queue", "Songs coming up, and who placed them", Vec::new()),
            command("13", "w", "music queue clear", "Empty the queue", Vec::new()),
            command("20", "p", "place-vote", "Vote in an open Pollster poll", vec![option("poll", OptionKind::String, true, "Which poll"), option("choice", OptionKind::Integer, true, "Your answer's number")]),
            command("21", "p", "poll", "Start a poll in this channel", vec![option("question", OptionKind::String, true, "What to ask")]),
            command("22", "p", "poll-image", "Start a poll about a picture", vec![option("question", OptionKind::String, true, "What to ask"), option("image", OptionKind::Attachment, true, "The picture to vote on")]),
            command("30", "q", "quiz", "Start a quiz round for everyone here", vec![category, option("host", OptionKind::User, false, "Who reads the questions")]),
            command("31", "q", "leaderboard", "Who's answered the most", Vec::new()),
        ],
    }
}

// Types `text` into the composer half a second in, the caret at `caret`, then runs `then`.
pub(super) fn demo_command_text(window: &mut Window, cx: &mut Context<Waffle>, text: &'static str, caret: usize, then: impl FnOnce(&mut Waffle, &mut Window, &mut Context<Waffle>) + 'static) {
    cx.spawn_in(window, async move |this, cx| {
        cx.background_executor().timer(std::time::Duration::from_millis(500)).await;
        this.update_in(cx, |this, window, cx| {
            this.composer.update(cx, |state, cx| {
                state.set_value(text, window, cx);
                state.set_selected_range(caret..caret, cx);
                state.focus(window, cx);
            });
            this.on_composer_change(window, cx);
            then(this, window, cx);
        })
        .ok();
    })
    .detach(); // lint-allow(detached-task): one-shot fixture step; ends after one timer and update, or when the view is gone
}

pub(super) fn app_user(id: &str, name: &str) -> User {
    User { bot: true, ..user(id, name) }
}

pub(super) fn app_post(id: &str, author: &User, minutes_ago: i64, content: &str) -> Message {
    Message {
        id: id.into(),
        channel_id: "general".into(),
        guild_id: Some("1".into()),
        author: author.clone(),
        content: content.into(),
        timestamp: (chrono::Utc::now() - chrono::Duration::minutes(minutes_ago)).to_rfc3339(),
        application_id: Some(author.id.clone()),
        ..Default::default()
    }
}

pub(super) fn used(by: &User, command: &str) -> Option<crate::app_commands::InteractionMeta> {
    Some(crate::app_commands::InteractionMeta { id: format!("demo-{command}"), user: by.clone(), command: Some(command.into()), ephemerality_reason: None })
}

pub(super) fn append(waffle: &mut Waffle, posts: Vec<Message>) {
    let Some(feed) = waffle.feeds.get_mut("general") else { return };
    let mut messages = feed.messages.as_ref().clone();
    messages.extend(posts);
    feed.messages = std::sync::Arc::new(messages);
    waffle.list.reset(waffle.feeds.get("general").map_or(0, |f| f.messages.len()));
}

// An interaction as `Waffle::interact` leaves it: in flight, or failed when `failed`.
pub(super) fn pending(app: &User, kind: crate::interactions::InteractionKind, failed: bool) -> crate::interactions::PendingInteraction {
    let app = crate::app_commands::App { id: app.id.clone(), name: app.name.clone(), icon: Some(app.avatar.clone()) };
    crate::interactions::PendingInteraction { channel_id: "general".into(), app, id: None, kind, failure: failed.then(|| crate::interactions::INTERACTION_FAILED.to_owned()) }
}

pub(super) fn demo_app_replies(waffle: &mut Waffle) {
    use crate::{app_commands::{EPHEMERAL, LOADING}, discord::SendState, interactions::InteractionKind};
    let (wavebot, quizzy, mira, me) = (app_user("900001", "Wavebot"), app_user("900002", "Quizzy"), user("31000020", "Mira"), me_user());
    let thinking = Message { id: "pending-demo-think".into(), nonce: Some("demo-think".into()), state: SendState::Sending, flags: LOADING, interaction: used(&mira, "play"), ..app_post("", &wavebot, 4, "") };
    let reply = Message { interaction: used(&mira, "queue"), ..app_post("9101", &wavebot, 3, "Now playing **Midnight Arcade** by Neon Harbor in **Lounge**. 3 songs in the queue.") };
    let only_you = Message { interaction: used(&me, "play"), flags: EPHEMERAL, ..app_post("9102", &wavebot, 2, "Join a voice channel first, then try /play again.") };
    let mut limited = used(&me, "skip");
    if let Some(meta) = limited.as_mut() {
        meta.ephemerality_reason = Some(4);
    }
    let slowed = Message { interaction: limited, flags: EPHEMERAL, ..app_post("9103", &wavebot, 2, "Skipped **Midnight Arcade**.") };
    let failed = Message { id: "pending-demo-fail".into(), nonce: Some("demo-fail".into()), state: SendState::Failed, interaction: used(&me, "trivia"), ..app_post("", &quizzy, 1, "") };
    waffle.interactions.insert("demo-think".into(), pending(&wavebot, InteractionKind::Command, false));
    waffle.interactions.insert("demo-fail".into(), pending(&quizzy, InteractionKind::Command, true));
    append(waffle, vec![thinking, reply, only_you, slowed, failed]);
}

pub(super) fn button(style: crate::app_commands::ButtonStyle, label: &str, custom_id: &str) -> crate::app_commands::Component {
    let url = (style == crate::app_commands::ButtonStyle::Link).then(|| "https://example.com/rules".to_owned());
    let custom_id = url.is_none().then(|| custom_id.to_owned());
    crate::app_commands::Component::Button(crate::app_commands::Button { style, label: Some(label.into()), emoji: None, custom_id, url, disabled: false, sku_id: None })
}

pub(super) fn demo_components(waffle: &mut Waffle, users_open: bool, window: &mut Window, cx: &mut Context<Waffle>) {
    use crate::{
        app_commands::{Button, ButtonStyle::*, Component, Select, SelectKind, SelectOption},
        interactions::InteractionKind,
    };
    let (pollster, quizzy) = (app_user("900003", "Pollster"), app_user("900002", "Quizzy"));
    let results = Component::Button(Button { style: Secondary, label: Some("Results (after vote)".into()), emoji: None, custom_id: Some("results".into()), url: None, disabled: true, sku_id: None });
    let premium = |sku: &str| Component::Button(Button { style: Premium, label: None, emoji: None, custom_id: None, url: None, disabled: false, sku_id: Some(sku.into()) });
    waffle.demo_sku("demo-sku-maps", crate::components::Sku::Ready("Map pack - $4.99".into()));
    waffle.demo_sku("demo-sku-gone", crate::components::Sku::Unavailable);
    let vote = Message {
        components: vec![
            Component::Row(vec![button(Primary, "Vote Ashen Peaks", "ashen"), button(Secondary, "Drowned Harbor", "harbor"), button(Success, "Ready", "ready"), button(Danger, "Leave lobby", "leave"), button(Link, "Rules", "")]),
            Component::Row(vec![results, button(Secondary, "Checking", "check"), premium("demo-sku-maps"), premium("demo-sku-gone")]),
            Component::Row(vec![button(Secondary, "Shuffle teams", "shuffle")]),
        ],
        ..app_post("9201", &pollster, 3, "Map vote for tonight's scrim is open.")
    };
    let option = |emoji: &str, label: &str, value: &str, description: Option<&str>| SelectOption { label: label.into(), value: value.into(), description: description.map(Into::into), emoji: Some(emoji.into()), default: false };
    let categories = Select {
        kind: SelectKind::String,
        custom_id: "categories".into(),
        placeholder: Some("Choose up to 3 categories".into()),
        options: vec![option("🎮", "Games", "games", Some("Consoles, PC, retro")), option("🎬", "Movies", "movies", Some("Since 1990")), option("🎵", "Music", "music", None), option("🌍", "Geography", "geography", None)],
        min: 1,
        max: 3,
        disabled: false,
        required: false,
        default_values: Vec::new(),
        channel_types: Vec::new(),
    };
    let quiz = Message { components: vec![Component::Row(vec![Component::Select(categories)])], ..app_post("9202", &quizzy, 1, "Pick the categories for this round.") };
    // Wren is the default host; the member cache knows Wren, so the field reads the name.
    let wren = user("8388608", "Wren");
    waffle.members.entry("1".into()).or_default().insert(wren.id.clone(), wren.clone());
    let default_host = vec![crate::app_commands::DefaultValue { id: wren.id.clone(), kind: "user".into() }];
    let host = Select { kind: SelectKind::User, custom_id: "host".into(), placeholder: Some("Who reads the questions?".into()), options: Vec::new(), min: 1, max: 1, disabled: false, required: false, default_values: default_host, channel_types: Vec::new() };
    let room = Select { kind: SelectKind::Channel, custom_id: "room".into(), placeholder: Some("Which voice channel?".into()), options: Vec::new(), min: 1, max: 1, disabled: false, required: false, default_values: Vec::new(), channel_types: vec![2] };
    let hosts = Message { components: vec![Component::Row(vec![Component::Select(host)]), Component::Row(vec![Component::Select(room)])], ..app_post("9203", &quizzy, 0, "Pick tonight's host and room.") };
    let call = |custom_id: &str| InteractionKind::Component { message_id: "9201".into(), custom_id: custom_id.into() };
    waffle.interactions.insert("demo-check".into(), pending(&pollster, call("check"), false));
    waffle.interactions.insert("demo-shuffle".into(), pending(&pollster, call("shuffle"), true));
    append(waffle, vec![vote, quiz.clone(), hosts.clone()]);
    let (open, custom_id, picks): (Message, &str, &[&str]) = if users_open { (hosts, "host", &[]) } else { (quiz, "categories", &["games", "music"]) };
    // Once the field has painted, so the menu opens under it.
    cx.spawn_in(window, async move |this, cx| {
        cx.background_executor().timer(std::time::Duration::from_millis(600)).await;
        this.update_in(cx, |this, window, cx| this.demo_open_select(open, custom_id, picks, window, cx)).ok();
    })
    .detach(); // lint-allow(detached-task): one-shot fixture step; ends after one timer and update, or when the view is gone
}

pub(super) fn demo_components_v2(waffle: &mut Waffle) {
    use crate::app_commands::{ButtonStyle::*, COMPONENTS_V2, Component, GalleryItem};
    let wavebot = app_user("900001", "Wavebot");
    let text = |t: &str| Component::Text(t.into());
    let now_playing = Component::Container {
        accent: Some(0x5865F2),
        spoiler: false,
        children: vec![
            Component::Section {
                children: vec![text("### Midnight Arcade\nNeon Harbor · 3:41 · requested by Mira\n-# Up next: Glass Desert, Ferris Wheel at Dawn")],
                accessory: Some(Box::new(Component::Thumbnail { url: avatar_url("3", None), description: None, spoiler: false, width: Some(640), height: Some(360) })),
            },
            Component::Separator { divider: true, large: false },
            Component::Row(vec![button(Secondary, "Pause", "pause"), button(Secondary, "Skip", "skip"), button(Secondary, "Shuffle", "shuffle"), button(Danger, "Stop", "stop")]),
        ],
    };
    let item = |id: &str, spoiler: bool, (w, h): (u32, u32)| GalleryItem { url: avatar_url(id, None), description: None, spoiler, width: Some(w), height: Some(h) };
    let album = Component::Container {
        accent: None,
        spoiler: false,
        children: vec![
            Component::Section { children: vec![text("**Screenshots from tonight**\n-# 3 images · uploaded by Theo")], accessory: Some(Box::new(button(Secondary, "Open album", "album"))) },
            Component::Gallery(vec![item("3", false, (1200, 800)), item("8388608", false, (800, 1200)), item("12582912", true, (1000, 1000))]),
            Component::File { url: "https://example.com/setlist.txt".into(), name: Some("setlist.txt".into()), size: Some(18_432), spoiler: false },
            Component::File { url: "https://example.com/scores.csv".into(), name: Some("final-scores.csv".into()), size: Some(2_310_144), spoiler: true },
        ],
    };
    let reveal = Component::Container {
        accent: None,
        spoiler: false,
        children: vec![Component::Section {
            children: vec![text("**Mystery guest**\nRevealed at the end of round three.")],
            accessory: Some(Box::new(Component::Thumbnail { url: avatar_url("16777216", None), description: None, spoiler: true, width: Some(300), height: Some(400) })),
        }],
    };
    let ending = Component::Container {
        accent: None,
        spoiler: true,
        children: vec![text("## Tonight's winner\nTheo took the crown with 42 points."), Component::Row(vec![button(Secondary, "Rematch", "rematch")])],
    };
    let cover = Component::Container { accent: None, spoiler: false, children: vec![text("**Tonight's poster**"), Component::Gallery(vec![item("4194304", false, (1600, 900))])] };
    let post = Message { flags: COMPONENTS_V2, components: vec![now_playing, album, reveal, cover, ending], ..app_post("9301", &wavebot, 1, "") };

    append(waffle, vec![post]);
}

// Night Owls as a NEWS server with #announcements: a crosspost from Bevy and one whose source was deleted, our
// published message and a fresh one under its Publish bump, the toolbar up. `-publish` opens the confirm (12
// following servers, 3 of 10 used), `-limit` the rate-limit alert, `-follow` the Follow dialog, `-member` the
// blocker a member who can't post sees, `-settings` the channel's Announcement Channel switch, `-followed` the
// Integrations page's Channels Followed, and `-source` the source-server card Bevy's crosspost opens.
pub(super) fn demo_announcements(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    let mut news = channel("news", "announcements", ChannelKind::Text, Some("c0"));
    (news.raw_type, news.topic) = (5, Some("Patch notes and server news".into()));
    let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") else { return };
    guild.features.push("NEWS".into());
    guild.channels.insert(1, news.clone());
    if scene == "announcements-member" {
        guild.owner_id = Some("8388608".into());
        for everyone in guild.roles.iter_mut().filter(|r| r.id == "1") {
            everyone.permissions = crate::discord::VIEW_CHANNEL | crate::discord::perm::READ_MESSAGE_HISTORY;
        }
    }
    let me = user("4194304", "kiwi");
    let post = |id: &str, author: User, days: i64, content: &str, flags: u64| Message {
        id: id.into(),
        channel_id: "news".into(),
        guild_id: Some("1".into()),
        author,
        content: content.into(),
        timestamp: (chrono::Utc::now() - chrono::Duration::days(days)).to_rfc3339(),
        flags,
        crosspost_from: (flags & Message::IS_CROSSPOST != 0).then(|| crate::discord::CrosspostSource { guild_id: "bevy".into(), channel_id: "bevy-news".into() }),
        ..Default::default()
    };
    let bevy =User { id: "77".into(), name: "Bevy #announcements".into(), avatar: avatar_url("5", None), bot: true, ..Default::default() };
    let fresh = post("954", me.clone(), 0, "Patch 7.1 is live! Healer ult range now shows on the minimap. Full notes in #patch-notes.", 0);
    let messages = vec![
        post("951", bevy.clone(), 3, "Bevy 0.17 is out: required components everywhere and a new picking backend.", Message::IS_CROSSPOST),
        post("952", bevy, 2, "", Message::IS_CROSSPOST | Message::SOURCE_MESSAGE_DELETED),
        post("953", me, 1, "Ranked season 7 starts Friday.", Message::CROSSPOSTED),
        fresh.clone(),
    ];
    waffle.feeds.insert("news".into(), Feed { messages: std::sync::Arc::new(messages), loaded: true, ..Default::default() });
    waffle.open_channel(news.clone(), window, cx);
    waffle.offer_publish_bump(&fresh);
    waffle.demo_announcement_stats("news", 3);
    match scene {
        "announcements-publish" => waffle.open_publish(fresh, cx),
        "announcements-limit" => waffle.demo_publish_limit(1415.),
        "announcements-follow" => waffle.open_follow(news, cx),
        "announcements-settings" => waffle.open_channel_settings(news, "1".into(), window, cx),
        "announcements-member" => {}
        "announcements-followed" => demo_followed_channels(waffle, window, cx),
        "announcements-source" => {
            let crosspost = waffle.feeds.get("news").and_then(|f| f.messages.iter().find(|m| m.id == "951").cloned());
            if let Some(crosspost) = crosspost {
                waffle.demo_source_card(&crosspost, demo_at(380., 170.), cx);
            }
        }
        _ => waffle.hovered_message = Some("954".into()),
    }
}

// Night Owls' Integrations page with two followed channels: Bevy's news into #general and Discord's
// community updates into #staff.
pub(super) fn demo_followed_channels(waffle: &mut Waffle, window: &mut Window, cx: &mut Context<Waffle>) {
    crate::server_settings::demo::open(waffle, "server-settings", window, cx);
    if let Some(view) = waffle.server_settings.as_mut() {
        view.page = crate::server_settings::access::Page::Integrations;
    }
    let follow = |id: &str, name: &str, into: &str, guild: &str, channel: &str| {
        serde_json::json!({ "id": id, "type": 2, "name": name, "channel_id": into, "source_guild": { "id": guild, "name": name.rsplit_once(' ').map_or(name, |(g, _)| g), "icon": null }, "source_channel": { "id": format!("{guild}-news"), "name": channel } })
    };
    let hooks = serde_json::json!([follow("f1", "Bevy #announcements", "general", "bevy", "announcements"), follow("f2", "Discord Updates #community-updates", "staff", "667560445975986187", "community-updates")]);
    waffle.demo_followed_channels("1", hooks);
}

// Night Owls' events: a stage that's live, a voice hangout starting within the hour (yours, marked
// Interested), a weekly stream somewhere else with a cover and next week's stream canceled, and a canceled
// one. "-create" opens the wizard at its second step instead of the list; "-occurrence" opens the weekly
// stream's details, where its occurrences and their menus are.
pub(super) fn demo_events(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    use chrono::{Datelike, Duration, Utc};

    use crate::discord::ScheduledEvent;
    let at = |minutes: i64| (Utc::now() + Duration::minutes(minutes)).to_rfc3339();
    let weekly_start = Utc::now() + Duration::days(3);
    let next_week = weekly_start + Duration::weeks(1);
    let event = |id: &str, name: &str, start: String, status: u8, entity_type: u8, creator: User| ScheduledEvent {
        id: id.into(),
        guild_id: "1".into(),
        creator_id: Some(creator.id.clone()),
        creator: Some(creator),
        name: name.into(),
        start,
        status,
        entity_type,
        ..Default::default()
    };
    let live = ScheduledEvent {
        channel_id: Some("stage".into()),
        description: Some("Round two of the autumn bracket, called live from the stage. Raise a hand to ask the players anything between matches.".into()),
        interested: 1112,
        ..event("ev-live", "Community cup: round two of the autumn bracket, with commentary from the whole Night Owls crew", at(-20), ScheduledEvent::ACTIVE, ScheduledEvent::STAGE, user("8388608", "Wren"))
    };
    let soon = ScheduledEvent {
        channel_id: Some("lounge".into()),
        description: Some("We're watching the director's cut of The Long Night. Bring snacks, keep spoilers in the thread, and stay for the vote on next week's pick, which always runs longer than the film.".into()),
        interested: 24,
        me_interested: true,
        ..event("ev-soon", "Movie night: The Long Night", at(12), ScheduledEvent::SCHEDULED, ScheduledEvent::VOICE, me_user())
    };
    let weekly = ScheduledEvent {
        end: Some((weekly_start + Duration::hours(4)).to_rfc3339()),
        location: Some("https://twitch.tv/nightowls-community-cup-autumn-bracket-finals".into()),
        description: Some("Every week on the channel: the bracket, the highlights and the long arguments about map picks.".into()),
        interested: 318,
        image: Some("demo/banner.png".into()),
        recurrence: Some(serde_json::json!({ "start": weekly_start.to_rfc3339(), "frequency": 2, "interval": 1, "by_weekday": [weekly_start.weekday().num_days_from_monday()] })),
        exceptions: vec![crate::discord::EventException { id: crate::events::recurrence_id(next_week), start: Some(next_week.to_rfc3339()), end: None, canceled: true }],
        ..event("ev-weekly", "Night Owls weekly stream", weekly_start.to_rfc3339(), ScheduledEvent::SCHEDULED, ScheduledEvent::EXTERNAL, user("16777216", "toma reed"))
    };
    let canceled = ScheduledEvent { channel_id: Some("hub".into()), interested: 3, ..event("ev-canceled", "Snack rota planning", at(60 * 26), ScheduledEvent::CANCELED, ScheduledEvent::VOICE, user("20971520", "ferris")) };
    if let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") {
        let mut stage = channel("stage", "Friday stage", ChannelKind::Voice, Some("c1"));
        stage.raw_type = 13;
        guild.channels.push(stage);
        guild.features.push("COMMUNITY".into());
        guild.events = vec![live, soon, weekly, canceled];
    }
    match scene {
        "events-create" => {}
        "events-occurrence" => {
            waffle.open_events("1".into(), cx);
            return waffle.open_event_details("1".into(), "ev-weekly".into(), cx);
        }
        _ => return waffle.open_events("1".into(), cx),
    }
    waffle.open_event_wizard("1".into(), None, window, cx);
    waffle.fill_event_wizard("Community cup: round two", ScheduledEvent::EXTERNAL, "twitch.tv/nightowls", window, cx);
}
