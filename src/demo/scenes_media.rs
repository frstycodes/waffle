use super::*;

pub(super) fn demo_link_previews(waffle: &mut Waffle) {
    use crate::{discord::Invite, link_preview::Preview};
    let mut announcements = channel("1100", "📌-announcements", ChannelKind::Text, Some("c0"));
    announcements.position = 0;
    if let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") {
        guild.channels.push(announcements);
    }
    let quoted = Message {
        id: "1101".into(),
        channel_id: "1100".into(),
        guild_id: Some("1".into()),
        author: user("8388608", "Wren"),
        timestamp: (chrono::Utc::now() - chrono::Duration::hours(3)).to_rfc3339(),
        content: "Season 7 notes are up: placements drop to **5 matches**, duo queue opens at every rank, and Foundry is back with the smaller mid. Ping <@4194304> if your rank looks off after the reset, we're collecting reports in one thread so nothing gets lost.".into(),
        media: vec![Media { url: "brand/icons/sticker.png".into(), width: 512., height: 512., kind: MediaKind::Image, ..Default::default() }],
        ..Default::default()
    };
    {
        let mut previews = waffle.link_previews.borrow_mut();
        previews.seed("m-1100-1101", Preview::Message(Box::new(quoted)));
        previews.seed("m-1100-1199", Preview::Unavailable);
        previews.seed("i-night-owls", Preview::Invite(Invite { guild_id: "1".into(), name: "Night Owls".into(), icon: None, online: Some(42), members: Some(318), ..Default::default() }));
        let tea = Invite { guild_id: "77".into(), name: "Late Night Tea House, a server with a name long enough to truncate".into(), icon: None, online: Some(1_204), members: Some(8_311), ..Default::default() };
        previews.seed("i-tea-house", Preview::Invite(tea));
        previews.seed("i-expired-code", Preview::Unavailable);
    }
    let Some(feed) = waffle.feeds.get_mut("general") else { return };
    let mut messages = feed.messages.as_ref().clone();
    let before = messages.len();
    let ago = |mins: i64| (chrono::Utc::now() - chrono::Duration::minutes(mins)).to_rfc3339();
    let post = |id: &str, author: User, mins: i64, content: &str| Message { id: id.into(), channel_id: "general".into(), guild_id: Some("1".into()), author, timestamp: ago(mins), content: content.into(), ..Default::default() };
    messages.extend([
        post("121", user("16777216", "toma reed"), 12, "still fetching https://discord.gg/still-loading https://ptb.discord.com/channels/1/1100/1150"),
        post("122", user("16777216", "toma reed"), 11, "this one's dead https://discord.gg/expired-code and I can't open https://canary.discord.com/channels/1/1100/1199"),
        post("123", user("8388608", "Wren"), 9, "come hang after ranked https://discord.gg/tea-house and bring people here too https://discord.gg/night-owls"),
        post("124", user("12582912", "moss lantern"), 6, "older notes are pinned in https://discord.com/channels/1/1100 if you missed them"),
        post("125", me_user(), 2, "did everyone see this? https://discord.com/channels/1/1100/1101"),
    ]);
    feed.messages = std::sync::Arc::new(messages);
    // Appended like arriving messages, so the feed stays pinned to the newest.
    waffle.list.splice(before..before, feed.messages.len() - before);
}

pub(super) const PATCH_NOTES: &str = "Patch notes, season 7

Ranked
- Placement matches drop from 10 to 5; your old rank seeds the first one.
- Leaving a match early now costs twice the rank it used to, and the second leave in a day locks the queue for 30 minutes.
- Duo queue opens at every rank. Trios stay closed above Diamond.
- A very long line with no spaces to check wrapping: https://example.com/a/really/long/path/that/keeps/going/and/going/and/going/without/any/break/at/all/until/the/end

Maps
- Harbor: the long sightline from B main to the crane is blocked by new containers.
- Foundry returns to the pool with the smaller mid it had in season 5.
- Every map gets a second spawn barrier, so the first ten seconds can't be peeked.

Bug fixes
- The scoreboard no longer swaps teams' colours after half time.
- Crouch-peeking a ledge no longer shows your head through the wall.
- Reconnecting mid-round puts you back on your team instead of spectating.

";

// A made-up picture: a diagonal wash with a few lighter "windows", so tiles read as real video. BGRA
// bytes, as gpui's render images expect.
pub(super) fn demo_frame(width: u32, height: u32, tint: u8) -> std::sync::Arc<gpui_kit::RenderImage> {
    let panel = |x: u32, y: u32| (x * 8 / width + y * 6 / height).is_multiple_of(5);
    let buffer = image::RgbaImage::from_fn(width, height, |x, y| {
        let t = ((x + y) * 160 / (width + height)) as u8;
        let lift = if panel(x, y) { 70 } else { 0 };
        image::Rgba([tint.wrapping_add(t / 2).saturating_add(lift), (40 + t / 3).saturating_add(lift), (90 + t / 2).saturating_add(lift), 255])
    });
    std::sync::Arc::new(gpui_kit::RenderImage::new([image::Frame::new(buffer)]))
}

// The viewer scene's items: (width, height, is a clip), every fourth a clip.
pub(super) const DEMO_SIZES: [(f32, f32); 20] = [
    (1600., 1067.), (1080., 1350.), (2048., 1152.), (1280., 720.), (1200., 1200.), (900., 1600.), (2400., 1600.), (1280., 720.), (1500., 1000.), (1920., 1080.),
    (1080., 1080.), (1280., 720.), (1600., 900.), (800., 1200.), (2000., 1333.), (1280., 720.), (1350., 1080.), (1024., 768.), (1440., 1800.), (1280., 720.),
];

pub(super) fn demo_item(n: usize) -> Media {
    let (width, height) = DEMO_SIZES[n];
    if n % 4 == 3 {
        let url = format!("https://cdn.discordapp.com/attachments/demo/{n}/clip-{n}.mp4");
        return Media { kind: MediaKind::Video { poster: format!("https://media.discordapp.net/attachments/demo/{n}/clip-{n}.mp4?format=jpeg") }, url, width, height, ..Default::default() };
    }
    Media { url: format!("https://cdn.discordapp.com/attachments/demo/{n}/photo-{n}.png"), width, height, kind: MediaKind::Image, ..Default::default() }
}

// Serves the viewer scene's media without the network: a clip from testdata, and for pictures and
// posters a made-up gradient at whatever size the URL asks the proxy for.
pub(super) fn demo_media(url: &str) -> Option<bytes::Bytes> {
    let rest = url.split("/attachments/demo/").nth(1)?;
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    if let Some(&(_, secs)) = DEMO_AUDIO.iter().find(|(name, _)| *name == path) {
        return Some(demo_tone(secs).into());
    }
    if path.ends_with(".mp4") && !query.contains("format=") {
        return std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/video-aac.mp4")).ok().map(Into::into);
    }
    let n: usize = path.split('/').next()?.parse().ok()?;
    let (w, h) = DEMO_SIZES.get(n).copied()?;
    let param = |key: &str| query.split('&').find_map(|kv| kv.strip_prefix(key)?.strip_prefix('=')?.parse::<u32>().ok());
    let (w, h) = (param("width").unwrap_or(w as u32), param("height").unwrap_or(h as u32));
    let bytes = demo_picture(n, w, h);
    eprintln!("[demo media] {}x{} {} bytes {url}", w, h, bytes.len());
    Some(bytes.into())
}

// A soft two-hue gradient with a sun, distinct per item.
pub(super) fn demo_picture(n: usize, w: u32, h: u32) -> Vec<u8> {
    let hue = (n as f32 * 47.) % 360.;
    let (a, b) = (hsl(hue, 0.55, 0.62), hsl((hue + 40.) % 360., 0.5, 0.32));
    let (cx, cy, r) = (w as f32 * (0.25 + 0.05 * (n % 10) as f32), h as f32 * 0.4, w.min(h) as f32 * 0.16);
    let picture = image::RgbImage::from_fn(w, h, |x, y| {
        let t = (x as f32 / w as f32 * 0.4 + y as f32 / h as f32 * 0.6).min(1.);
        let sun = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() < r;
        let mix = |i: usize| if sun { 245 } else { (a[i] as f32 * (1. - t) + b[i] as f32 * t) as u8 };
        image::Rgb([mix(0), mix(1), mix(2)])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    picture.write_to(&mut out, image::ImageFormat::Png).ok();
    out.into_inner()
}

pub(super) fn hsl(h: f32, s: f32, l: f32) -> [u8; 3] {
    let c = (1. - (2. * l - 1.).abs()) * s;
    let x = c * (1. - ((h / 60.) % 2. - 1.).abs());
    let (r, g, b) = match (h / 60.) as u32 {
        0 => (c, x, 0.),
        1 => (x, c, 0.),
        2 => (0., c, x),
        3 => (0., x, c),
        4 => (x, 0., c),
        _ => (c, 0., x),
    };
    let m = l - c / 2.;
    [((r + m) * 255.) as u8, ((g + m) * 255.) as u8, ((b + m) * 255.) as u8]
}

pub(super) fn demo_custom_status() -> crate::discord::CustomStatus {
    crate::discord::CustomStatus { text: "Reviewing the parity spec, back at 5".into(), emoji: Some(crate::discord::Emoji::unicode("🍡")), expires_at: None }
}

// design/messages.html's two boards, the second with your vote in and its longest answer wrapping.
pub(super) fn quizzy_modal() -> crate::app_commands::Modal {
    use crate::app_commands::{App, DefaultValue, FileUpload, Modal, ModalComponent, Select, SelectKind, TextInput};
    let input = |custom_id: &str, label: &str, paragraph: bool, placeholder: Option<&str>, value: Option<&str>, required: bool, max: Option<u32>| {
        let input = TextInput { custom_id: custom_id.into(), paragraph, placeholder: placeholder.map(Into::into), value: value.map(Into::into), required, min: None, max, ..Default::default() };
        ModalComponent::Label { label: label.into(), description: None, component: Box::new(ModalComponent::TextInput(input)) }
    };
    Modal {
        id: "demo-modal".into(),
        nonce: None,
        channel_id: "general".into(),
        application: App { id: "quizzy".into(), name: "Quizzy".into(), icon: None },
        title: "Suggest a question".into(),
        custom_id: "suggest".into(),
        components: vec![
            input("question", "Question", false, None, Some("Which game shipped first: Doom or Myst?"), true, Some(200)),
            input("answer", "Answer", false, Some("The right answer"), None, true, None),
            input("why", "Why it's fun (optional)", true, Some("A line on why people will like it"), None, false, Some(400)),
            labelled("Credit", Some("Who thought of it"), ModalComponent::Select(Select { default_values: vec![DefaultValue { id: "4194304".into(), kind: "user".into() }], ..entity_select(SelectKind::User, "credit", 1) })),
            labelled("Post it in", None, ModalComponent::Select(Select { channel_types: vec![0], placeholder: Some("A text channel".into()), ..entity_select(SelectKind::Channel, "where", 1) })),
            labelled("Picture", Some("Shown with the question"), ModalComponent::FileUpload(FileUpload { custom_id: "picture".into(), required: false, min: None, max: Some(2) })),
        ],
    }
}

pub(super) fn labelled(label: &str, description: Option<&str>, component: crate::app_commands::ModalComponent) -> crate::app_commands::ModalComponent {
    crate::app_commands::ModalComponent::Label { label: label.into(), description: description.map(Into::into), component: Box::new(component) }
}

pub(super) fn entity_select(kind: crate::app_commands::SelectKind, custom_id: &str, max: u32) -> crate::app_commands::Select {
    crate::app_commands::Select { kind, custom_id: custom_id.into(), placeholder: None, options: Vec::new(), min: 1, max, disabled: false, required: false, default_values: Vec::new(), channel_types: Vec::new() }
}

pub(super) fn demo_polls(waffle: &mut Waffle) {
    use crate::discord::{Poll, PollAnswer};
    let answer = |id: u32, emoji: &str, text: &str, count: u64, me: bool| PollAnswer { id, text: text.into(), emoji: Some(Emoji::unicode(emoji)), count, me };
    let ends = (chrono::Utc::now() + chrono::Duration::minutes(3 * 60 + 10)).to_rfc3339();
    let poll = |mine: bool| Poll {
        question: "Which map first tonight?".into(),
        answers: vec![
            answer(1, "🏔️", "Ashen Peaks", 67 + u64::from(mine), mine),
            answer(2, "🌊", "Drowned Harbor", 50, false),
            answer(3, "🌵", "Glass Desert, the one with the sandstorms that last forever", 24, false),
        ],
        expiry: Some(ends.clone()),
        multiselect: false,
        finalized: false,
    };
    let post = |id: &str, author: User, poll: Poll| Message { id: id.into(), channel_id: "general".into(), guild_id: Some("1".into()), author, timestamp: chrono::Utc::now().to_rfc3339(), poll: Some(poll), ..Default::default() };
    if let Some(feed) = waffle.feeds.get_mut("general") {
        std::sync::Arc::make_mut(&mut feed.messages).extend([post("195", user("8388608", "Jun"), poll(false)), post("196", user("12582912", "moss lantern"), poll(true))]);
    }
    waffle.list.reset(waffle.feeds.get("general").map_or(0, |f| f.messages.len()));
}

// Real ids from Discord's own packs (animated APNG), so the pictures load; Lottie ones stand in for the
// format Waffle can't play.
pub(super) fn demo_stickers(waffle: &mut Waffle) {
    use crate::discord::{Sticker, StickerFormat, StickerPack};
    let sticker = |id: &str, name: &str, format| Sticker { id: id.into(), name: name.into(), format, tags: String::new() };
    let choco = vec![
        sticker("781291131828699156", "Wave", StickerFormat::Apng),
        sticker("781291442961383434", "Sing", StickerFormat::Apng),
        sticker("781291606493495306", "On My Way", StickerFormat::Apng),
        sticker("781321379546398740", "Eating", StickerFormat::Apng),
        sticker("781321874650562560", "Roll", StickerFormat::Apng),
    ];
    let hammy = vec![sticker("809207198856904764", "Cheer", StickerFormat::Apng), sticker("809207919115239525", "Laugh", StickerFormat::Apng), sticker("809208197235343410", "Sleep", StickerFormat::Apng)];
    if let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") {
        guild.stickers = vec![sticker("818597707132043285", "Love", StickerFormat::Apng), sticker("818597355619483688", "OMG", StickerFormat::Apng), sticker("818597454483161098", "Money", StickerFormat::Apng)];
    }
    waffle.sticker_packs = Some(Ok(vec![StickerPack { name: "Cheerful Choco".into(), stickers: choco.clone() }, StickerPack { name: "Hammy Ham".into(), stickers: hammy }]));
    let mut wave = Message { id: "190".into(), channel_id: "general".into(), guild_id: Some("1".into()), author: user("8388608", "Wren"), timestamp: "2026-11-14T14:10:00Z".into(), ..Default::default() };
    wave.stickers = vec![choco[0].clone()];
    let lottie = Message { id: "191".into(), author: user("12582912", "moss lantern"), stickers: vec![sticker("749044136589393960", "Smile", StickerFormat::Lottie)], timestamp: "2026-11-14T14:11:00Z".into(), ..wave.clone() };
    if let Some(feed) = waffle.feeds.get_mut("general") {
        std::sync::Arc::make_mut(&mut feed.messages).extend([wave, lottie]);
    }
    waffle.list.reset(waffle.feeds.get("general").map_or(0, |f| f.messages.len()));
}

// "send-options": a 1,270-member server, the newest message sent @silent, and Hold Up! over a composer
// holding "@everyone brackets are up". "tts": the composer holding "/tts", the command picker up with its Built-In /tts.
pub(super) fn demo_send_options(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    let groups = vec![("r1".into(), 20), ("online".into(), 150), ("offline".into(), 1100)];
    waffle.on_event(Event::MemberList { guild_id: "1".into(), groups, ops: Vec::new() }, cx);
    if let Some(feed) = waffle.feeds.get_mut("general") {
        let mut messages = feed.messages.as_ref().clone();
        if let Some(last) = messages.last_mut() {
            last.flags = crate::discord::SUPPRESS_NOTIFICATIONS;
        }
        feed.messages = std::sync::Arc::new(messages);
    }
    let typed = if scene == "tts" { "/tts" } else { "@everyone brackets are up" };
    // Typed once the window is up, as `mention` does; the popup follows, since typing closes it.
    cx.spawn_in(window, async move |this, cx| {
        cx.background_executor().timer(std::time::Duration::from_millis(500)).await;
        this.update_in(cx, |this, window, cx| {
            this.composer.update(cx, |state, cx| {
                state.set_value(typed, window, cx);
                state.focus(window, cx);
            })
        })
        .ok();
        cx.background_executor().timer(std::time::Duration::from_millis(100)).await;
        this.update(cx, |this, cx| {
            // "/tts" opened the command picker as it was typed.
            if !typed.starts_with('/') {
                this.hold_up = this.hold_up_for(typed);
            }
            cx.notify();
        })
        .ok();
    })
    .detach(); // lint-allow(detached-task): one-shot fixture step; ends after one timer and update, or when the view is gone
}

// Puts `reactions` on the newest message in #general, where the feed opens; returns its id.
pub(super) fn set_reactions(waffle: &mut Waffle, reactions: Vec<Reaction>) -> String {
    let id = newest(waffle);
    if let Some(feed) = waffle.feeds.get_mut("general") {
        let mut messages = feed.messages.as_ref().clone();
        if let Some(last) = messages.last_mut() {
            last.reactions = reactions;
        }
        feed.messages = std::sync::Arc::new(messages);
    }
    id
}

pub(super) fn newest(waffle: &Waffle) -> String {
    waffle.feeds.get("general").and_then(|f| f.messages.last()).map(|m| m.id.clone()).unwrap_or_default()
}

// People who reacted, as the reactions route returns them: account names, which members the server knows
// show as their nicknames, then strangers with long and short names.
pub(super) fn reactors(count: usize) -> Vec<Reactor> {
    let people = [
        ("12582912", "mosslantern", "mosslantern"),
        ("8388608", "wren.wav", "wren.wav"),
        ("16777216", "toma", "toma.reed"),
        ("25165824", "longname", "a.member.whose.display.name.runs.long"),
        ("31000001", "Dhalon", "dhalon"),
        ("31000002", "saik", "saik03"),
        ("31000003", "Digitally_Devious", "digitally_devious"),
        ("31000004", "Laryke", "laryke"),
        ("31000005", "h y p r", ".thehypr"),
        ("31000006", "pouyas", "pouyas"),
        ("31000007", "Bartholomew Featherstonehaugh-Wright", "bart.fw"),
        ("31000008", "J", "yqsod"),
        ("31000009", "valse", "valsesn"),
        ("31000010", "mikey", "mickeeeyyyyyy"),
    ];
    people.iter().take(count).map(|&(id, name, username)| Reactor { user: user(id, name), username: username.into() }).collect()
}

// Mod View on Wren: timed out for 42 minutes, flagged for unusual DMs three days ago, two roles, joined
// through an invite, with a short action log.
pub(super) fn demo_mod_view(waffle: &mut Waffle, cx: &mut Context<Waffle>) {
    mod_view_fixture(waffle);
    let audit = serde_json::json!({
        "audit_log_entries": [
            { "id": "1441000000000000000", "action_type": 24, "user_id": "12582912", "target_id": "8388608", "reason": "spam in #media", "changes": [{ "key": "communication_disabled_until", "new_value": "2026-11-22T21:30:00+00:00" }] },
            { "id": "1430000000000000000", "action_type": 25, "user_id": "4194304", "target_id": "8388608", "changes": [{ "key": "$add", "new_value": [{ "id": "r2", "name": "Rustacean" }] }] },
            { "id": "1420000000000000000", "action_type": 24, "user_id": "8388608", "target_id": "8388608", "changes": [{ "key": "nick", "old_value": "wren", "new_value": "Wren" }] },
        ],
        "users": [{ "id": "12582912", "username": "moss", "global_name": "moss lantern" }, { "id": "4194304", "username": "kiwi.dev", "global_name": "kiwi" }, { "id": "8388608", "username": "wren.wav", "global_name": "Wren" }],
    });
    waffle.demo_mod_view("1".into(), "8388608".into(), audit, cx);
}

// Wren pings @Rustacean and its card is open: five of its fourteen members loaded (kiwi owns the server,
// toma and moss boost it), the rest counted under "+9 more".
pub(super) fn demo_role_popout(waffle: &mut Waffle, cx: &mut Context<Waffle>) {
    let member = |id: &str, name: &str, roles: &[&str], boosting: bool| {
        let mut user = user(id, name);
        user.boosting_since = boosting.then(|| "2025-03-14T10:00:00+00:00".into());
        (user, roles.iter().map(|r| r.to_string()).collect())
    };
    let members = vec![
        member("4194304", "kiwi", &["r1", "r2"], false),
        member("16777216", "toma reed", &["r2"], true),
        member("12582912", "moss lantern", &["r2"], true),
        member("8388608", "Wren", &["r2"], false),
        member("31000001", "Dhalon", &["r2"], false),
    ];
    waffle.on_event(Event::MembersChunk { guild_id: "1".into(), members, facts: Vec::new() }, cx);
    waffle.presences.insert("31000001".into(), Presence { status: Presence_::Online, ..Default::default() });
    if let Some(feed) = waffle.feeds.get_mut("general") {
        let mut messages = feed.messages.as_ref().clone();
        let at = chrono::Utc::now().to_rfc3339();
        let author = user("8388608", "Wren");
        messages.push(Message { id: "990".into(), channel_id: "general".into(), guild_id: Some("1".into()), author, roles: Some(vec!["r2".into()]), content: "<@&r2> can someone check the build".into(), timestamp: at, ..Default::default() });
        feed.messages = std::sync::Arc::new(messages);
    }
    waffle.role_popout.seed_counts("1", [("r2".to_owned(), 14)].into());
    waffle.open_role_popout(Some("r2".into()), demo_at(470., 300.), cx);
}

// Favorites open: #ranked-lfg loose, then Games holding #patch-notes (nicknamed) and #show-and-tell (3 mentions),
// from two servers, the first one open.
pub(super) fn demo_favorites(waffle: &mut Waffle, window: &mut Window, cx: &mut Context<Waffle>) {
    let server = |waffle: &Waffle, id: &str, name: &str, channels: Vec<Channel>| {
        let mut guild = waffle.guilds[1].clone();
        guild.id = id.into();
        guild.name = name.into();
        guild.channels = channels;
        guild
    };
    let night_shift = server(waffle, "6", "Night Shift", vec![channel("6001", "ranked-lfg", ChannelKind::Text, None), channel("6002", "patch-notes", ChannelKind::Text, None)]);
    let pixel_club = server(waffle, "7", "Pixel Club", vec![channel("7001", "show-and-tell", ChannelKind::Text, None)]);
    waffle.guilds.extend([night_shift, pixel_club]);
    waffle.read_states.insert("7001".into(), ReadState { last_read: 100, mentions: 3 });
    waffle.last_message.insert("7001".into(), 900);
    let games = waffle.favorites.add_category("Games", 9000);
    waffle.favorites.add_channel(6001, 0, 0);
    waffle.favorites.add_channel(6002, 0, games);
    waffle.favorites.add_channel(7001, 0, games);
    if let Some(entry) = waffle.favorites.channels.get_mut(&6002) {
        entry.nickname = "patch notes".into();
    }
    waffle.open_favorites(window, cx);
}

// Server settings on the Members table: six members covering every signal and join method shape;
// `-filter` narrows it to the past 4 weeks with the date menu open, `-bulk` picks two rows for the bulk bar.
pub(super) fn demo_server_settings_members(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    crate::server_settings::demo::open(waffle, scene, window, cx);
}
