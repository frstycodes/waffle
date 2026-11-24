use super::*;

pub(super) fn user(id: &str, name: &str) -> User {
    // Display-name styles: fonts show everywhere, the effects' colours only outside servers.
    use crate::discord::{NameEffect, NameStyle};
    let name_style = match id {
        "8388608" => Some(NameStyle { font: 12, effect: NameEffect::Gradient, colors: vec![0x2AAEA6, 0xFFC490] }),
        "12582912" => Some(NameStyle { font: 3, effect: NameEffect::Gummy, colors: vec![] }),
        "16777216" => Some(NameStyle { font: 14, effect: NameEffect::Solid, colors: vec![0x47BAE7] }),
        _ => None,
    };
    User { id: id.into(), name: name.into(), avatar: avatar_url(id, None), name_style, ..Default::default() }
}

pub(super) fn channel(id: &str, name: &str, kind: ChannelKind, parent: Option<&str>) -> Channel {
    Channel {
        id: id.into(),
        name: name.into(),
        kind,
        parent_id: parent.map(Into::into),
        position: 0,
        topic: None,
        last_message_id: None,
        private: false,
        recipient: None,
        recipients: Vec::new(),
        icon: None,
        thread: None,
        raw_type: match kind {
            ChannelKind::Voice => 2,
            ChannelKind::Category => 4,
            ChannelKind::Dm => 1,
            ChannelKind::Thread => 11,
            ChannelKind::Forum => 15,
            ChannelKind::Text | ChannelKind::Other => 0,
        },
        overwrites: Vec::new(),
        props: Default::default(),
    }
}

pub(super) fn ready(me: User) -> Ready {
    let mut general = channel("general", "👋-general", ChannelKind::Text, Some("c0"));
    general.topic = Some("Este es un buen lugar para hablar de Rust, gpui y todo lo demás".into());
    let mut staff = channel("staff", "🔒-staff-notes", ChannelKind::Text, Some("c0"));
    staff.private = true;
    staff.last_message_id = Some("900".into());
    let mut media = channel("media", "📷-media", ChannelKind::Text, Some("c0"));
    media.last_message_id = Some("900".into());
    let roles = vec![
        Role { id: "r1".into(), name: "Council".into(), color: 0xE91E63, position: 3, permissions: 0, managed: false, icon: None },
        Role { id: "r2".into(), name: "Rustacean".into(), color: 0xE67E22, position: 2, permissions: 0, managed: false, icon: None },
        Role { id: "1".into(), name: "@everyone".into(), color: 0, position: 0, permissions: 0, managed: false, icon: None },
    ];
    let guild = |id: &str, name: &str, channels: Vec<Channel>, voice_states: Vec<VoiceState>| Guild {
        id: id.into(),
        name: name.into(),
        icon: None,
        channels,
        roles: roles.clone(),
        voice_states,
        stage_instances: Vec::new(),
        threads: Vec::new(),
        default_notifications: 1,
        owner_id: Some("4194304".into()),
        my_roles: Vec::new(),
        emojis: Vec::new(),
        stickers: Vec::new(),
        premium_tier: 1,
        afk_channel_id: None,
        ..Default::default()
    };
    let lounge_states = vec![
        VoiceState { guild_id: Some("1".into()), channel_id: Some("lounge".into()), user_id: "12582912".into(), user: Some(user("12582912", "moss lantern")), muted: false, deafened: false, server_muted: false, server_deafened: false, streaming: true, camera: false, suppress: false, requested_at: None },
        VoiceState { guild_id: Some("1".into()), channel_id: Some("lounge".into()), user_id: "16777216".into(), user: Some(user("16777216", "toma reed")), muted: true, deafened: false, server_muted: false, server_deafened: false, streaming: false, camera: false, suppress: false, requested_at: None },
        VoiceState { guild_id: Some("1".into()), channel_id: Some("lounge".into()), user_id: "8388608".into(), user: Some(user("8388608", "Wren")), muted: false, deafened: false, server_muted: false, server_deafened: false, streaming: false, camera: false, suppress: false, requested_at: None },
        VoiceState { guild_id: Some("1".into()), channel_id: Some("lounge".into()), user_id: "20971520".into(), user: Some(user("20971520", "ferris")), muted: true, deafened: true, server_muted: false, server_deafened: false, streaming: false, camera: false, suppress: false, requested_at: None },
        VoiceState { guild_id: Some("1".into()), channel_id: Some("lounge".into()), user_id: "4194304".into(), user: Some(user("4194304", "kiwi")), muted: false, deafened: false, server_muted: false, server_deafened: false, streaming: false, camera: false, suppress: false, requested_at: None },
    ];
    let channels = vec![
        channel("c0", "General", ChannelKind::Category, None),
        general,
        media,
        staff,
        channel("long", "a-channel-name-long-enough-to-truncate-in-the-sidebar", ChannelKind::Text, Some("c0")),
        channel("c1", "Voice", ChannelKind::Category, None),
        channel("lounge", "hangout", ChannelKind::Voice, Some("c1")),
        channel("hub", "study-hall", ChannelKind::Voice, Some("c1")),
    ];
    let mut dm = channel("dm", "Wren", ChannelKind::Dm, None);
    dm.recipient = Some(user("8388608", "Wren"));
    Ready {
        developer_mode: true,
        account_settings: crate::discord::AccountSettings { developer_mode: true, ..Default::default() },
        my_account: crate::discord::MyAccount { username: "kiwi.dev".into(), global_name: Some("kiwi".into()), banner: None, premium: false, premium_type: 0 },
        guild_settings: Vec::new(),
        ignored: Vec::new(),
        me,
        status: Presence_::Dnd,
        custom_status: None,
        session_id: String::new(),
        guilds: vec![
            {
                let mut g = guild("1", "Night Owls", channels, lounge_states);
                g.threads = vec![demo_thread()];
                g
            },
            guild("2", "gpui", vec![channel("g2", "general", ChannelKind::Text, None)], vec![]),
            guild("3", "Zed Industries", vec![], vec![]),
            guild("4", "Bevy", vec![], vec![]),
            guild("5", "Tokio", vec![], vec![]),
        ],
        folders: vec![
            Folder { id: String::new(), name: None, color: None, guild_ids: vec!["1".into()] },
            Folder { id: "9".into(), name: Some("Rust".into()), color: Some(0xCE422B), guild_ids: vec!["2".into(), "3".into(), "4".into(), "5".into()] },
        ],
        dms: vec![dm],
        read_states: vec![
            ("media".into(), ReadState { last_read: 100, mentions: 0 }),
            ("staff".into(), ReadState { last_read: 100, mentions: 2 }),
        ],
        presences: vec![("8388608".into(), Presence { status: Presence_::Online, custom: None, activity: None, ..Default::default() })],
        relationships: vec![
            Relationship { user: user("8388608", "Wren"), kind: Relation::Friend, since: Some("2023-08-09T18:20:00+00:00".into()) },
            Relationship { user: user("12582912", "moss lantern"), kind: Relation::Friend, since: Some("2024-02-14T09:00:00+00:00".into()) },
            Relationship { user: user("16777216", "toma reed"), kind: Relation::Incoming, since: None },
        ],
    }
}

// `load`: an account the size of a busy real one, for frame timing. 60 more servers of 100 channels, each
// channel with role overwrites, and none owned by us, so every permission check runs in full; 80 more
// channels in Night Owls; 300 members; 600 messages.
pub(super) const LOAD_SERVERS: usize = 60;
pub(super) const LOAD_CHANNELS: usize = 100;
pub(super) const LOAD_ROLES: usize = 25;

pub(super) fn load_guilds(waffle: &mut Waffle) {
    let view = crate::discord::VIEW_CHANNEL;
    let roles: Vec<Role> = (0..LOAD_ROLES).map(|r| Role { id: format!("lr{r}"), name: format!("role {r}"), color: 0x5865F2 + r as u32 * 0x0A0A, position: r as i64, permissions: if r == 0 { view } else { 0 }, managed: false, icon: None }).collect();
    let overwrites = |c: usize| (0..4).map(|o| crate::discord::Overwrite { id: format!("lr{}", (c + o) % LOAD_ROLES), allow: if o == 0 { view } else { 0 }, deny: 0, member: false }).collect::<Vec<_>>();
    let channels = |g: usize, count: usize| -> Vec<Channel> {
        (0..count)
            .map(|c| {
                let (kind, parent) = match c % 10 {
                    0 => (ChannelKind::Category, None),
                    9 => (ChannelKind::Voice, Some(format!("l{g}-{}", c - c % 10))),
                    _ => (ChannelKind::Text, Some(format!("l{g}-{}", c - c % 10))),
                };
                let mut channel = channel(&format!("l{g}-{c}"), &format!("channel-{c}-of-server-{g}"), kind, parent.as_deref());
                channel.position = c as i64;
                channel.overwrites = overwrites(c);
                channel.last_message_id = Some("900".into());
                channel
            })
            .collect()
    };
    for g in 0..LOAD_SERVERS {
        waffle.guilds.push(Guild {
            id: format!("l{g}"),
            name: format!("Server {g} with a longer name"),
            icon: None,
            channels: channels(g, LOAD_CHANNELS),
            roles: roles.clone(),
            voice_states: Vec::new(),
            stage_instances: Vec::new(),
            threads: Vec::new(),
            default_notifications: 1,
            owner_id: Some("1".into()),
            my_roles: (0..LOAD_ROLES).step_by(3).map(|r| format!("lr{r}")).collect(),
            emojis: Vec::new(),
            stickers: Vec::new(),
            premium_tier: 1,
            afk_channel_id: None,
            ..Default::default()
        });
    }
    if let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") {
        guild.channels.extend(channels(0, 80).into_iter().map(|mut c| {
            c.id = format!("owl-{}", c.id);
            c.parent_id = c.parent_id.map(|p| format!("owl-{p}"));
            c.overwrites.clear();
            c
        }));
    }
}

pub(super) fn load_members() -> (Vec<(String, u32)>, Vec<ListItem>) {
    let groups = [("r1", Presence_::Online, 20), ("online", Presence_::Online, 150), ("offline", Presence_::Offline, 130)];
    let mut items = Vec::new();
    for (group, status, count) in groups {
        items.push(ListItem::Group { id: group.into(), count });
        for n in 0..count {
            let id = format!("{}", 30_000_000 + items.len());
            let activity = (n % 3 == 0).then(|| Activity { kind: ActivityKind::Playing, text: format!("Game number {n}"), rest: Vec::new() });
            items.push(ListItem::Member(Member { user: user(&id, &format!("{group} member {n}")), roles: vec!["r2".into()], presence: Presence { status, custom: None, activity, ..Default::default() }, boosting_since: (n % 7 == 0).then(|| "2025-03-14T10:00:00+00:00".into()) }));
        }
    }
    (groups.iter().map(|(g, _, c)| ((*g).into(), *c)).collect(), items)
}

// The fixture's messages over and over, with their own ids and times, so the list has a real length.
pub(super) fn load_messages() -> Vec<Message> {
    let base = messages();
    (0..600)
        .map(|n| {
            let mut message = base[n % base.len()].clone();
            message.id = format!("{}", 10_000 + n);
            message.timestamp = format!("2026-11-{:02}T{:02}:{:02}:00Z", 1 + n / 60, (n / 4) % 24, n % 60);
            message.reply = None;
            message.reply_to = None;
            message
        })
        .collect()
}

pub(super) fn members() -> Vec<ListItem> {
    let member = |id: &str, name: &str, roles: &[&str], status, activity: Option<(ActivityKind, &str)>, bot: bool| {
        let mut user = user(id, name);
        user.bot = bot;
        // A nameplate, with bundled art standing in for Discord's (the demo stays offline).
        if id == "4194304" {
            user.nameplate = Some(Box::new(crate::discord::Nameplate { art: "brand/icons/paper.png".into(), animated: None, tint: (0x2D6A4F, 0x95D5B2) }));
        }
        ListItem::Member(Member { user, roles: roles.iter().map(|r| r.to_string()).collect(), presence: Presence {
            status,
            custom: activity.filter(|(kind, _)| *kind == ActivityKind::Custom).map(|(_, text)| text.into()),
            activity: activity.filter(|(kind, _)| *kind != ActivityKind::Custom).map(|(kind, text)| Activity { kind, text: text.into(), rest: Vec::new() }),
            activities: Vec::new(),
            // Wren is only on their phone, so the panel shows the mobile mark.
            mobile_only: id == "8388608",
            streaming: activity.is_some_and(|(kind, _)| kind == ActivityKind::Streaming),
        }, boosting_since: (id == "4194304").then(|| "2025-03-14T10:00:00+00:00".into()) })
    };
    vec![
        ListItem::Group { id: "r1".into(), count: 2 },
        member("4194304", "kiwi", &["r1"], Presence_::Dnd, None, false),
        member("20971520", "Jockie Music", &["r1"], Presence_::Online, Some((ActivityKind::Custom, "m!help")), true),
        ListItem::Group { id: "online".into(), count: 3 },
        member("12582912", "moss lantern", &["r2"], Presence_::Online, Some((ActivityKind::Playing, "Apex Legends")), false),
        member("16777216", "toma reed", &[], Presence_::Idle, Some((ActivityKind::Streaming, "Speedrunning Celeste")), false),
        member("8388608", "Wren", &["r2"], Presence_::Online, None, false),
        ListItem::Group { id: "offline".into(), count: 1 },
        member("25165824", "a member whose display name runs long", &[], Presence_::Offline, None, false),
    ]
}
