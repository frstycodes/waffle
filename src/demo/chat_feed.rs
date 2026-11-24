use super::*;

// design/embeds.html's examples: every field, bot logs, a video, a gallery, a poll result, the worst case.
pub(super) fn embed_messages() -> Vec<Message> {
    let bot = |id: &str, name: &str| {
        let mut u = user(id, name);
        u.bot = true;
        u
    };
    let post = |id: &str, author: User, ts: &str, embeds: Vec<Embed>| Message { id: id.into(), channel_id: "general".into(), guild_id: Some("1".into()), author, timestamp: ts.into(), embeds, ..Default::default() };
    let pic = |seed: &str, width: f32, height: f32| Media { url: avatar_url(seed, None), width, height, kind: MediaKind::Image, ..Default::default() };
    let field = |name: &str, value: &str, inline: bool| (name.to_owned(), value.to_owned(), inline);
    let now = chrono::Utc::now();
    let ago = |min: i64| (now - chrono::Duration::minutes(min)).to_rfc3339();
    let soon = |min: i64| (now + chrono::Duration::minutes(min)).timestamp();
    let kitchen = Embed {
        color: Some(0xE67E22),
        author: Some("Raidbot · Night owls".into()),
        author_url: Some("https://example.com/raid".into()),
        author_icon: Some(avatar_url("1", None)),
        title: Some("Raid tonight: Ashen Peaks, heroic".into()),
        url: Some("https://example.com/r/2291".into()),
        description: Some(format!("Sign-ups close <t:{}:R>. Bring **fire resist**, and read the [route notes](https://example.com/notes) first.\n> Ping <@&r1> if you can't make it.", soon(90))),
        fields: vec![field("Starts", &format!("<t:{}:t>", soon(150)), true), field("Signed up", "17 of 20", true), field("Lead", "<@8388608>", true), field("Roles still open", "1 tank, 2 healers. Reply with `/join` and your role.", false)],
        thumbnail: Some(pic("2", 128., 128.)),
        images: vec![pic("3", 1280., 640.)],
        footer: Some("Raid #2291".into()),
        footer_icon: Some(avatar_url("1", None)),
        timestamp: Some(ago(4)),
        ..Default::default()
    };
    let voice = |joined: bool| Embed {
        color: Some(if joined { 0x43B581 } else { 0xF04747 }),
        author: Some("_kiwi".into()),
        author_icon: Some(avatar_url("4194304", None)),
        description: Some(format!("<@4194304> **{} voice channel** <#lounge>", if joined { "joined" } else { "left" })),
        footer: Some("ID: 4194304".into()),
        timestamp: Some(ago(if joined { 3 } else { 0 })),
        ..Default::default()
    };
    let ban = Embed {
        color: Some(0xC21F3A),
        author: Some("Member banned".into()),
        author_icon: Some(avatar_url("5", None)),
        description: Some("**spam_account_92** was banned from Night owls.".into()),
        fields: vec![field("User", "<@12582912>", true), field("Moderator", "<@8388608>", true), field("Duration", "Permanent", true), field("Reason", "Posting scam links in <#general> after a warning.", false)],
        footer: Some("Case 418".into()),
        timestamp: Some(ago(30)),
        ..Default::default()
    };
    let stats = Embed {
        color: Some(0x3498DB),
        title: Some("Wren's season so far".into()),
        url: Some("https://example.com/u/wren".into()),
        thumbnail: Some(pic("4", 128., 128.)),
        fields: vec![field("Rank", "Diamond II", true), field("Win rate", "58%", true), field("Matches", "214", true), field("Best hero", "Tide", true), field("Last match", "Win on Ashen Peaks, 13 to 9", false)],
        footer: Some("stats.example".into()),
        ..Default::default()
    };
    let plain = Embed { description: Some(format!("Servers go down for maintenance <t:{}:F>. It should take about an hour.", soon(60 * 24))), ..Default::default() };
    let video = Embed {
        video: true,
        color: Some(0xFF0000),
        url: Some("https://example.com/watch?v=s7".into()),
        provider: Some("VideoSite".into()),
        author: Some("Rift Notes".into()),
        title: Some("Every change in Season 7, in 9 minutes".into()),
        images: vec![pic("5", 1280., 720.)],
        ..Default::default()
    };
    let gallery = Embed {
        color: Some(0x9B59B6),
        url: Some("https://example.com/peaks-set".into()),
        author: Some("Rosa on ArtSite".into()),
        author_icon: Some(avatar_url("6", None)),
        title: Some("Ashen Peaks, concept set".into()),
        description: Some("Day, dusk, night and the storm pass.".into()),
        images: ["1", "2", "3", "4"].map(|s| pic(s, 800., 600.)).to_vec(),
        footer: Some("ArtSite".into()),
        ..Default::default()
    };
    let poll = |winner: bool| Embed {
        poll: Some(crate::discord::PollResult {
            question: "Which map first tonight?".into(),
            winner: winner.then(|| ("Ashen Peaks".into(), Some(Emoji::unicode("🏔️")))),
            winner_votes: if winner { 68 } else { 0 },
            total_votes: if winner { 142 } else { 12 },
        }),
        ..Default::default()
    };
    let notes = "Patch notes for **Season 7**.\n\n# Ashen Peaks\nThe new map flips between day and night every four minutes. Sightlines change with the light, so routes that are safe at noon are deadly at dusk.\n\n## Heroes\n- **Ember**, a close-range brawler who heals on burn damage\n- **Tide**, a support who pushes and pulls with water walls\n\n## Ranked\n> The ladder now resets softly: you drop one division instead of two tiers.\n\nPlacement matches are down from ten to five, and duo queue opens at every rank.\n\n```\nqueue: ranked\nplacements: 5\n```\n-# Full notes on the site.";
    let worst = Embed {
        color: Some(0xFFFFFF),
        author: Some("Bartholomew Featherstonehaugh-Montgomery of the Very Long Display Name and Several Other Titles".into()),
        author_icon: Some(avatar_url("2", None)),
        title: Some("This title runs to the full two hundred and fifty-six characters that Discord allows, which is far longer than anyone should write, so it has to wrap cleanly across several lines without pushing the thumbnail away or overflowing the card at any width at all, ok".into()),
        url: Some("https://example.com".into()),
        description: Some(format!("{notes}\nAn unbroken link: https://clips.example/watch/7f3a9c2e4b1d8a6f0e5c3b2a1d9e8f7c6b5a4d3e2f1a0b9c8d7e6f5a4b3c2d1e0")),
        thumbnail: Some(pic("3", 128., 128.)),
        fields: ["Region", "Queue", "Ping", "Players", "Mode", "Map", "Season", "Patch", "A field name long enough to wrap onto a second line"]
            .iter()
            .zip(["EU West", "Ranked", "24 ms", "9,812", "Control", "Ashen Peaks", "7", "7.0.3", "and a value that runs on for a while too"])
            .map(|(n, v)| field(n, v, true))
            .collect(),
        footer: Some(format!("A footer long enough to wrap: {}", "server status, region, build and shard ".repeat(3).trim())),
        footer_icon: Some(avatar_url("4", None)),
        timestamp: Some(ago(60 * 26)),
        ..Default::default()
    };
    let black = Embed { color: Some(0), title: Some("Colour 0x000000".into()), description: Some("A pure black bar, as Discord draws it.".into()), ..Default::default() };
    let sensitive = Embed { color: Some(0x95A5A6), title: Some("Uncensored match highlights".into()), url: Some("https://example.com/h".into()), sensitive: true, images: vec![pic("5", 1280., 720.)], footer: Some("clips.example".into()), ..Default::default() };
    let mut poll_post = post("206", bot("16777216", "Wren"), "2026-11-20T18:30:00Z", vec![poll(true), poll(false)]);
    poll_post.reply_to = Some("201".into());
    vec![
        post("201", bot("33554432", "Raidbot"), "2026-11-20T12:00:00Z", vec![kitchen]),
        post("202", bot("50331648", "Logger"), "2026-11-20T12:49:00Z", vec![voice(true), voice(false)]),
        post("203", bot("67108864", "Dyno"), "2026-11-20T13:10:00Z", vec![ban]),
        post("204", bot("83886080", "Statsy"), "2026-11-20T15:30:00Z", vec![stats, plain]),
        post("205", user("8388608", "Wren"), "2026-11-20T18:12:00Z", vec![video, gallery]),
        poll_post,
        post("207", bot("100663296", "Stressbot"), "2026-11-20T21:00:00Z", vec![worst, black, sensitive]),
    ]
}

// design/explore-system-messages.html's notices: a folded run of four joins, a lone join with its wave
// button, bot joins, boosts, threads, a follow, stage lines, group adds and leaves, a rename, discovery, a
// poll result, AutoMod, thread starters (whole, deleted, not loaded) and a type Discord draws nothing for.
pub(super) fn system_messages() -> Vec<Message> {
    let at = |minute: u32| format!("2026-11-21T12:{minute:02}:00Z");
    // Real snowflakes from the times, so each join picks its line the way Discord does.
    let id = |minute: u32| {
        let ms = chrono::DateTime::parse_from_rfc3339(&at(minute)).map_or(0, |t| t.timestamp_millis() as u64);
        (((ms - 1_420_070_400_000) << 22) | u64::from(minute)).to_string()
    };
    let notice = |minute: u32, kind: u8, author: &User, content: &str| Message {
        id: id(minute),
        channel_id: "general".into(),
        guild_id: Some("1".into()),
        author: author.clone(),
        content: content.into(),
        timestamp: at(minute),
        kind,
        ..Default::default()
    };
    let bot = |id: &str, name: &str| User { bot: true, ..user(id, name) };
    let field = |name: &str, value: &str| (name.to_owned(), value.to_owned(), false);
    let (ana, jun, rosa, kai, mira, theo) = (user("20971520", "Ana"), user("25165824", "Jun"), user("29360128", "Rosa"), user("33554432", "Kai"), user("37748736", "Mira"), user("41943040", "Theo"));
    let me = me_user();
    let parent = Message { content: "ok who's doing the patch 7 tier list, I'll start: honey is S tier".into(), ..notice(0, 0, &jun, "") };

    let mut carl = notice(7, 7, &bot("46137344", "Carl-bot"), "");
    carl.actor = Some(theo.clone());
    let mut thread = notice(12, 18, &jun, "gpui list virtualisation");
    thread.reference_channel = Some("t1".into());
    let mut added = notice(20, 1, &ana, "");
    added.mentions = vec![rosa.clone()];
    let mut poll = notice(24, 46, &ana, "");
    poll.reply_to = Some(id(0));
    poll.embeds = vec![Embed {
        poll: Some(crate::discord::PollResult { question: "Best waffle topping for the Friday stream, honey or the questionable syrup?".into(), winner: Some(("Honey".into(), Some(Emoji::unicode("🍯")))), winner_votes: 7, total_votes: 12 }),
        ..Default::default()
    }];
    let mut automod = notice(26, 24, &rosa, "");
    automod.embeds = vec![Embed {
        description: Some("free nitro at discord-gift.ru hurry".into()),
        fields: vec![
            field("rule_name", "Block scam links"),
            field("channel_id", "general"),
            field("keyword", "discord-gift"),
            field("keyword_matched_content", "discord-gift"),
            field("flagged_message_id", &id(25)),
            field("timeout_duration", "600"),
        ],
        ..Default::default()
    }];
    let mut starter = notice(28, 21, &jun, "");
    starter.reply = Some(Box::new(parent.clone()));
    starter.reply_to = Some(parent.id.clone());
    let mut deleted = notice(29, 21, &jun, "");
    deleted.reference_deleted = true;
    vec![
        parent,
        notice(1, 7, &ana, ""),
        notice(2, 7, &jun, ""),
        notice(3, 7, &rosa, ""),
        notice(4, 7, &kai, ""),
        Message { content: "welcome all four of you!".into(), ..notice(5, 0, &me, "") },
        notice(6, 7, &mira, ""),
        carl,
        notice(8, 7, &bot("50331648", "Statsy"), ""),
        notice(9, 8, &rosa, ""),
        notice(10, 10, &kai, "2"),
        notice(11, 22, &theo, ""),
        thread,
        notice(13, 12, &theo, "Rust News #announcements"),
        notice(14, 27, &theo, "Friday Q&A"),
        notice(15, 29, &mira, ""),
        notice(16, 30, &kai, ""),
        notice(17, 31, &theo, "Friday Q&A: patch 7"),
        notice(18, 28, &theo, "Friday Q&A: patch 7"),
        notice(19, 4, &theo, "late-night-games"),
        added,
        notice(21, 2, &jun, ""),
        notice(22, 14, &theo, ""),
        notice(23, 15, &theo, ""),
        poll,
        automod,
        notice(27, 36, &theo, "2026-11-22T09:44:00Z"),
        starter,
        deleted,
        notice(30, 21, &jun, ""),
    ]
}

// Remove embeds and YouTube (design/explore-system-messages.html): your link with an article card (✕ on
// hover), a bot's embed-only post (no ✕), a message whose embeds were removed (flag 4, so none show),
// then a YouTube video and a Short, each with Play and Open Link.
pub(super) fn embed_suppress_messages() -> Vec<Message> {
    let post = |id: &str, author: User, ts: &str, content: &str, embeds: Vec<Embed>| Message {
        id: id.into(),
        channel_id: "general".into(),
        guild_id: Some("1".into()),
        author,
        timestamp: ts.into(),
        content: content.into(),
        embeds,
        ..Default::default()
    };
    let pic = |seed: &str, width: f32, height: f32| Media { url: avatar_url(seed, None), width, height, kind: MediaKind::Image, ..Default::default() };
    let article = Embed {
        url: Some("https://blog.rust-lang.org/2026/11/20/Rust-1.95.0.html".into()),
        provider: Some("Rust Blog".into()),
        title: Some("Announcing Rust 1.95.0".into()),
        description: Some("The Rust team is happy to announce a new version of Rust, 1.95.0. Rust is a programming language empowering everyone to build reliable and efficient software.".into()),
        ..Default::default()
    };
    let mut bot = user("33554432", "Raidbot");
    bot.bot = true;
    let raid = Embed { rich: true, color: Some(0xE67E22), title: Some("Raid tonight: Ashen Peaks".into()), description: Some("Sign-ups are open.".into()), ..Default::default() };
    let youtube = |id: &str, title: &str, still: Media| Embed {
        video: true,
        color: Some(0xFF0000),
        url: Some(format!("https://www.youtube.com/watch?v={id}")),
        provider: Some("YouTube".into()),
        provider_url: Some("https://www.youtube.com".into()),
        title: Some(title.into()),
        images: vec![still],
        ..Default::default()
    };
    let wren = user("8388608", "Wren");
    let suppressed = Message { flags: crate::discord::message_flag::SUPPRESS_EMBEDS, ..post("303", wren.clone(), "2026-11-20T22:05:00Z", "no preview on this one https://blog.rust-lang.org/", vec![article.clone()]) };
    vec![
        post("301", me_user(), "2026-11-20T22:02:00Z", "this is the one I meant https://blog.rust-lang.org/2026/11/20/Rust-1.95.0.html", vec![article]),
        post("302", bot, "2026-11-20T22:03:00Z", "", vec![raid]),
        suppressed,
        post("304", wren.clone(), "2026-11-20T22:10:00Z", "https://youtu.be/dQw4w9WgXcQ", vec![youtube("dQw4w9WgXcQ", "How waffle irons are made", pic("5", 1280., 720.))]),
        post("305", wren, "2026-11-20T22:11:00Z", "https://youtube.com/shorts/abc123", vec![youtube("abc123", "honey pour asmr #shorts", pic("6", 720., 1280.))]),
    ]
}

pub(super) fn messages() -> Vec<Message> {
    let message = |id: &str, author: User, roles: &[&str], ts: &str, content: &str| Message {
        id: id.into(),
        channel_id: "general".into(),
        guild_id: Some("1".into()),
        author,
        roles: Some(roles.iter().map(|r| r.to_string()).collect()),
        content: content.into(),
        timestamp: ts.into(),
        ..Default::default()
    };
    let me = user("4194304", "kiwi");
    let mut abyss = user("12582912", "moss lantern");
    abyss.tag = Some(GuildTag { text: "FNC".into(), badge: None, guild_id: None });
    let zoe = user("8388608", "Wren");
    let mut first = message("101", abyss.clone(), &["r2"], "2026-11-13T17:18:00Z", "Morning! Anyone tried the new **gpui** list? It virtualises rows, so long channels stay cheap.");
    first.reactions = vec![Reaction { emoji: Emoji::unicode("🔥"), count: 3, me: true }, Reaction { emoji: Emoji::unicode("👀"), count: 1, me: false }];
    let mut image = message("102", zoe.clone(), &["r2"], "2026-11-13T17:20:00Z", "look at this");
    image.media = vec![
        Media { url: avatar_url("3", None), width: 256., height: 256., kind: MediaKind::Image, ..Default::default() },
        Media { url: "https://example.com/clip.mp4".into(), width: 256., height: 256., kind: MediaKind::Video { poster: avatar_url("4", None) }, ..Default::default() },
    ];
    image.embeds = vec![Embed {
        url: Some("https://www.rust-lang.org".into()),
        color: Some(0xCE422B),
        provider: Some("rust-lang.org".into()),
        title: Some("Rust Programming Language".into()),
        description: Some("A language empowering everyone to build reliable and efficient software.".into()),
        thumbnail: Some(Media { url: avatar_url("1", None), width: 80., height: 80., kind: MediaKind::Image, ..Default::default() }),
        ..Default::default()
    }, Embed {
        color: Some(0x43B581),
        author: Some("_kiwi".into()),
        author_icon: Some(avatar_url("4194304", None)),
        description: Some("**<@8388608> joined voice channel <#lounge>**".into()),
        footer: Some("ID: 8388608".into()),
        timestamp: Some("2026-11-14T08:12:00Z".into()),
        ..Default::default()
    }];
    first_thread_chip(&mut image);
    image.pinned = true;
    let mut pin_notice = message("107", me.clone(), &["r1"], "2026-11-14T08:21:00Z", "");
    (pin_notice.pin_notice, pin_notice.reply_to) = (true, Some("102".into()));
    let mut reply = message("103", me.clone(), &["r1"], "2026-11-14T08:15:00Z", "discord on rust <@8388608>, check <#media>");
    reply.reply_to = Some("101".into());
    reply.reply = Some(Box::new(first.clone()));
    reply.mentions = vec![zoe.clone()];
    reply.edited = true;
    let mut mention = message("104", zoe.clone(), &["r2"], "2026-11-14T08:17:00Z", "<@4194304> it already feels faster than the real one");
    mention.mentions = vec![me.clone()];
    vec![
        first,
        image,
        reply,
        mention,
        message("105", abyss.clone(), &["r2"], "2026-11-14T08:18:00Z", "```rust\nfn main() {\n    println!(\"hi\");\n}\n```"),
        // Discord keeps each line and its indent.
        Message { pinned: true, ..message("106", abyss, &["r2"], "2026-11-14T08:20:00Z", "the build so far:\n    Brightline 8-core CPU\n    Coldfront 240 mm cooler\n    32 GB memory\n<@4194304>") },
        pin_notice,
    ]
}

// The usual feed, then a busy run: several authors, continuations, links and a described picture.
pub(super) fn display_messages() -> Vec<Message> {
    let message = |id: &str, author: User, role: &str, ts: &str, content: &str| Message {
        id: id.into(),
        channel_id: "general".into(),
        guild_id: Some("1".into()),
        author,
        roles: Some(vec![role.into()]),
        content: content.into(),
        timestamp: ts.into(),
        ..Default::default()
    };
    let (kiwi, wren, moss) = (user("4194304", "kiwi"), user("8388608", "Wren"), user("12582912", "moss lantern"));
    let mut photo = message("208", wren.clone(), "r2", "2026-11-14T08:31:00Z", "the view from the pass");
    photo.media = vec![Media { url: avatar_url("3", None), width: 320., height: 200., kind: MediaKind::Image, description: Some("A snowy ridge at sunrise, two tents in the foreground".into()), ..Default::default() }];
    let mut reply = message("209", kiwi.clone(), "r1", "2026-11-14T08:33:00Z", "worth the climb");
    reply.reply_to = Some("208".into());
    reply.reply = Some(Box::new(photo.clone()));
    let mut feed = messages();
    feed.extend([
        message("201", kiwi.clone(), "r1", "2026-11-14T08:24:00Z", "release notes are up: https://example.com/notes/0.14"),
        message("202", kiwi.clone(), "r1", "2026-11-14T08:24:30Z", "and the [migration guide](https://example.com/migrate) too"),
        message("203", kiwi, "r1", "2026-11-14T08:25:00Z", "ping me if anything breaks"),
        message("204", moss.clone(), "r2", "2026-11-14T08:27:00Z", "on it"),
        message("205", moss, "r2", "2026-11-14T08:27:20Z", "the `cargo update` step needed `--precise`, otherwise smooth"),
        message("206", wren.clone(), "r2", "2026-11-14T08:29:00Z", "a long one, so the line has to wrap: the new list virtualises rows, keeps their heights cached between frames, and only measures what scrolls into view, which is why the big channels feel light now"),
        message("207", wren, "r2", "2026-11-14T08:30:00Z", "also hiked this weekend"),
        photo,
        reply,
    ]);
    feed
}


// The composer's guard, one state per scene. Wren owns Night Owls here, so our permissions are @everyone's
// (and Council's, for the immune scene).
pub(super) const VEX: &str = "33554432";
pub(super) const DUNE: &str = "41943040";

// One person's run between others', for blocked-messages: Wren, Vex three times, moss lantern, Dune once, Wren.
pub(super) fn blocked_messages() -> Vec<Message> {
    let message = |id: &str, author: User, ts: &str, content: &str| Message {
        id: id.into(),
        channel_id: "general".into(),
        guild_id: Some("1".into()),
        author,
        content: content.into(),
        timestamp: ts.into(),
        ..Default::default()
    };
    let (wren, moss, vex, dune) = (user("8388608", "Wren"), user("12582912", "moss lantern"), user(VEX, "Vex"), user(DUNE, "Dune"));
    let taunt = message("304", vex.clone(), "2026-11-14T21:42:00Z", "@kiwi answer me");
    let mut reply = message("307", wren.clone(), "2026-11-14T21:46:00Z", "ok starting at 10 sharp");
    reply.reply_to = Some(taunt.id.clone());
    reply.reply = Some(Box::new(taunt.clone()));
    vec![
        message("301", wren, "2026-11-14T21:40:00Z", "who's still in the lobby"),
        message("302", vex.clone(), "2026-11-14T21:41:00Z", "lol you're all trash"),
        message("303", vex, "2026-11-14T21:41:30Z", "come 1v1 me"),
        taunt,
        message("305", moss, "2026-11-14T21:43:00Z", "me, give me 2 minutes"),
        message("306", dune, "2026-11-14T21:44:00Z", "did anyone see my last clip"),
        reply,
    ]
}
