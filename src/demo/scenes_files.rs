use super::*;

// The header search focused: its filters and made-up history, or a filter's values being picked. The field
// is filled once the window is up, as the mention scene does.
pub(super) fn demo_search(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    use crate::search::{Saved, SavedToken};
    let token = |start: usize, end: usize, label: &str| SavedToken { start, end, label: label.into() };
    let history = vec![
        Saved { text: "from:8388608 in:media has:video".into(), tokens: vec![token(0, 12, "Wren"), token(13, 21, "📷-media"), token(22, 31, "video")] },
        Saved { text: "boss fight from:12582912 tho".into(), tokens: vec![token(11, 24, "moss lantern")] },
        Saved { text: "patch notes before:2026-11-01".into(), tokens: vec![token(12, 29, "Nov 1, 2026")] },
        Saved { text: "the absolutely final tournament bracket v12 FINAL really final".into(), tokens: Vec::new() },
    ];
    waffle.prefs.search_history.insert(me_user().id, history);
    let content = match scene {
        "search-from" => Saved { text: "in:general from:".into(), tokens: vec![token(0, 10, "👋-general")] }.to_content(),
        "search-date" => "dodge before:".into(),
        _ => "".into(),
    };
    cx.spawn_in(window, async move |this, cx| {
        cx.background_executor().timer(std::time::Duration::from_millis(500)).await;
        this.update_in(cx, |this, window, cx| {
            this.search_input.update(cx, |state, cx| {
                state.set_value(content, window, cx);
                state.focus(window, cx);
            });
            cx.notify();
        })
        .ok();
    })
    .detach(); // lint-allow(detached-task): one-shot fixture step; ends after one timer and update, or when the view is gone
}

// A page of results for `from:Wren has:image`: one with a picture, one with a gallery, one from another
// channel; Filters open over them in "search-filters".
pub(super) fn demo_search_results(waffle: &mut Waffle, filters_open: bool, window: &mut Window, cx: &mut Context<Waffle>) {
    let mut hits: Vec<Message> = messages().into_iter().rev().take(3).collect();
    // Real shapes: the avatars are square, the banner is 600×240.
    let avatar = |id: &str| Media { url: avatar_url(id, None), width: 256., height: 256., kind: MediaKind::Image, ..Default::default() };
    let banner = Media { url: "demo/banner.png".into(), width: 600., height: 240., kind: MediaKind::Image, ..Default::default() };
    for (i, hit) in hits.iter_mut().enumerate() {
        hit.author = user("8388608", "Wren");
        hit.media = match i {
            0 => vec![banner.clone()],
            1 => vec![avatar("5"), avatar("2"), avatar("4")],
            _ => Vec::new(),
        };
    }
    if let Some(last) = hits.last_mut() {
        last.channel_id = "media".into();
        last.content = "the night sky from the ridge, more in the thread".into();
        last.media = vec![avatar("2")];
    }
    waffle.right_panel = crate::RightPanel::Search(Box::new(crate::search_results::Results::demo(hits, 880, 2, filters_open)));
    let content = crate::search::Saved { text: "from:8388608 has:image".into(), tokens: vec![crate::search::SavedToken { start: 0, end: 12, label: "Wren".into() }, crate::search::SavedToken { start: 13, end: 22, label: "image".into() }] }.to_content();
    waffle.search_input.update(cx, |state, cx| state.set_value(content, window, cx));
}

// Search in the DM with Wren: the filters without `in:`, or a page of results from the DM.
pub(super) fn demo_search_dm(waffle: &mut Waffle, results: bool, window: &mut Window, cx: &mut Context<Waffle>) {
    let Some(dm) = waffle.dms.iter().find(|d| d.id == "dm").cloned() else { return };
    waffle.guild_id = None;
    waffle.open_channel(dm, window, cx);
    if results {
        let picture = Media { url: "demo/banner.png".into(), width: 600., height: 240., kind: MediaKind::Image, ..Default::default() };
        let hit = |id: &str, author: User, content: &str, media: Vec<Media>| Message { id: id.into(), channel_id: "dm".into(), author, timestamp: "2026-11-12T21:40:00+00:00".into(), content: content.into(), media, ..Default::default() };
        let hits = vec![hit("501", user("8388608", "Wren"), "here's the photo from the ridge", vec![picture]), hit("502", me_user(), "photo dump later tonight?", Vec::new())];
        waffle.right_panel = crate::RightPanel::Search(Box::new(crate::search_results::Results::demo(hits, 2, 1, false)));
        let content = crate::search::Saved { text: "has:image photo".into(), tokens: vec![crate::search::SavedToken { start: 0, end: 9, label: "image".into() }] }.to_content();
        waffle.search_input.update(cx, |state, cx| state.set_value(content, window, cx));
        return;
    }
    demo_search(waffle, "search", window, cx);
}

// Made-up overwrites and settings on #general and the hangout voice channel, then the window on one page.
pub(super) fn demo_channel_settings(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    use crate::{channel_settings::Page, discord::Overwrite};
    const ATTACH: u64 = 1 << 15;
    const MENTION_EVERYONE: u64 = 1 << 17;
    let ow = |id: &str, allow: u64, deny: u64, member: bool| Overwrite { id: id.into(), allow, deny, member };
    let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") else { return };
    for c in guild.channels.iter_mut() {
        match c.id.as_str() {
            "c0" => c.overwrites = vec![ow("1", 0, ATTACH, false)],
            "general" => {
                c.topic = Some("Say hi, share what you're building. Spoilers go in ||bars||.".into());
                c.props.slowmode = 30;
                c.props.archive_after = 4320;
                c.overwrites = vec![ow("1", 0, ATTACH, false), ow("r2", ATTACH, MENTION_EVERYONE, false), ow("8388608", 1 << 13, 0, true)];
            }
            "lounge" => {
                c.raw_type = 2;
                c.props.bitrate = 64_000;
                c.props.user_limit = 12;
            }
            _ => {}
        }
    }
    let id = if scene.starts_with("channel-settings-voice") { "lounge" } else { "general" };
    let Some(channel) = waffle.guild().and_then(|g| g.channels.iter().find(|c| c.id == id).cloned()) else { return };
    waffle.open_channel_settings(channel, "1".into(), window, cx);
    let page = match scene {
        "channel-settings-perms" | "channel-settings-voice-perms" => Page::Permissions,
        "channel-settings-invites" => Page::Invites,
        _ => Page::Overview,
    };
    if let Some(view) = waffle.channel_settings.as_mut() {
        view.demo(page);
    }
    if scene == "channel-settings-search" {
        waffle.demo_channel_search("slow", window, cx);
    }
    if scene == "channel-settings-dirty" {
        waffle.demo_channel_edit(window, cx);
    }
}

pub(super) fn me_user() -> User {
    user("4194304", "kiwi")
}

pub(super) fn account(user: &User, status: Presence_) -> crate::accounts::Account {
    crate::accounts::Account { id: user.id.clone(), name: user.name.clone(), avatar: user.avatar.clone(), status }
}

pub(super) fn demo_attachments(waffle: &mut Waffle, window: &mut Window, cx: &mut Context<Waffle>) {
    use std::sync::{Arc, atomic::AtomicU64};

    use crate::{SendState, UploadJob, discord::Upload};
    let dir = std::env::temp_dir().join("waffle-demo-attachments");
    let _ = std::fs::create_dir_all(&dir); // lint-allow(main-thread-io): fixture file written while the demo scene builds the window, before its first frame
    let picture = |name: &str, tint: u8| {
        let path = dir.join(name);
        let image = image::RgbaImage::from_fn(1600, 1000, |x, y| image::Rgba([tint.wrapping_add((x / 8) as u8), (60 + y / 6) as u8, (140 + (x + y) / 30) as u8, 255]));
        let _ = image.save(&path);
        path
    };
    let filler = |name: &str, size: usize| {
        let path = dir.join(name);
        let _ = std::fs::write(&path, vec![b'x'; size]); // lint-allow(main-thread-io): fixture file written while the demo scene builds the window, before its first frame
        path
    };
    let paths = vec![
        picture("dusk-over-the-harbour.png", 200),
        picture("plot-twist.png", 40),
        filler("first-flight.mp4", 3_400_000),
        filler("lantern-festival-flyer.pdf", 820_000),
        filler("save-files.zip", 6_100_000),
        filler("a-very-long-file-name-that-keeps-going-well-past-the-tile-edge-v12-final.txt", 2_048),
    ];
    waffle.add_files(paths.clone(), window, cx);
    if let Some(spoiler) = waffle.pending_files.get_mut(1) {
        spoiler.spoiler = true;
    }
    let job = |files: &[(&std::path::PathBuf, f64)], error: Option<&str>| UploadJob {
        channel_id: "general".into(),
        files: files
            .iter()
            .map(|(path, done)| {
                let size = std::fs::metadata(path).map_or(0, |m| m.len()); // lint-allow(main-thread-io): fixture file written while the demo scene builds the window, before its first frame
                let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                Upload { path: (*path).clone(), name, size, description: None, spoiler: false, sent: Arc::new(AtomicU64::new((size as f64 * done) as u64)) }
            })
            .collect(),
        content: String::new(),
        reply_to: None,
        ping: true,
        options: Default::default(),
        error: error.map(str::to_owned),
    };
    waffle.uploads.insert("demo-up-1".into(), job(&[(&paths[0], 1.0), (&paths[4], 0.42)], None));
    waffle.uploads.insert("demo-up-2".into(), job(&[(&paths[3], 0.0)], Some("Upload failed: 413 Payload Too Large")));
    let post = |id: &str, content: &str, state: SendState| Message {
        id: format!("pending-{id}"),
        channel_id: "general".into(),
        guild_id: Some("1".into()),
        author: me_user(),
        content: content.into(),
        timestamp: "2026-11-14T08:24:00Z".into(),
        nonce: Some(id.into()),
        state,
        ..Default::default()
    };
    // Through push_local, so the open feed's list grows with them.
    waffle.push_local(post("demo-up-1", "the harbour shots from saturday", SendState::Sending), cx);
    waffle.push_local(post("demo-up-2", "", SendState::Failed), cx);
}

// File name and length of each demo audio file; the voice messages say their length up front, but
// `voice-message-3.ogg` doesn't, to show the `--:--` it waits on.
pub(super) const DEMO_AUDIO: [(&str, f32); 6] = [
    ("voice-message.ogg", 12.4),
    ("voice-message-1.ogg", 96.),
    ("voice-message-2.ogg", 1.6),
    ("voice-message-3.ogg", 8.),
    ("late night demo – rough mix v3 (final final).mp3", 225.),
    ("ping.wav", 7.),
];

pub(super) fn demo_voice_messages(waffle: &mut Waffle) {
    use crate::discord::File;
    let _ = crate::discord::DEMO_MEDIA.set(demo_media);
    let file = |i: usize, voice: bool| {
        let (name, secs) = DEMO_AUDIO[i];
        let waveform = if voice && i != 3 { (0..64).map(|k| (128. + 110. * ((k as f32 / 3.) + i as f32).sin() * ((k * 7 + i) % 5) as f32 / 4.) as u8).collect() } else { Vec::new() };
        let length = (voice && i != 3).then_some(secs);
        File { name: name.into(), url: format!("https://cdn.discordapp.com/attachments/demo/{name}"), size: (secs * 16_000.) as u64, id: format!("demo-audio-{i}"), duration_secs: length, waveform, ..Default::default() }
    };
    crate::audio_player::demo_spot(&mut waffle.prefs, "demo-audio-1", 31.);
    let Some(feed) = waffle.feeds.get_mut("general") else { return };
    let mut messages = feed.messages.as_ref().clone();
    let people = [user("12582912", "moss lantern"), user("8388608", "Wren"), user("4194304", "juniper")];
    let post = |i: usize, voice: bool| Message {
        id: format!("audio-{i}"),
        channel_id: "general".into(),
        guild_id: Some("1".into()),
        author: people[i % people.len()].clone(),
        timestamp: "2026-11-14T08:20:00Z".into(),
        files: vec![file(i, voice)],
        flags: if voice { crate::discord::message_flag::IS_VOICE_MESSAGE } else { 0 },
        ..Default::default()
    };
    messages.extend((0..4).map(|i| post(i, true)));
    messages.extend((4..6).map(|i| post(i, false)));
    feed.messages = std::sync::Arc::new(messages);
    waffle.list.reset(waffle.feeds.get("general").map_or(0, |f| f.messages.len()));
}

// A mono 16-bit WAV of a voice-like hum: a low tone whose loudness swells like syllables.
pub(super) fn demo_tone(secs: f32) -> Vec<u8> {
    const RATE: u32 = 22_050;
    let samples = (secs * RATE as f32) as u32;
    let mut out = Vec::with_capacity(44 + samples as usize * 2);
    let header: [&[u8]; 13] = [b"RIFF", &(36 + samples * 2).to_le_bytes(), b"WAVEfmt ", &16u32.to_le_bytes(), &1u16.to_le_bytes(), &1u16.to_le_bytes(), &RATE.to_le_bytes(), &(RATE * 2).to_le_bytes(), &2u16.to_le_bytes(), &16u16.to_le_bytes(), b"data", &(samples * 2).to_le_bytes(), &[]];
    header.iter().for_each(|part| out.extend_from_slice(part));
    for n in 0..samples {
        let t = n as f32 / RATE as f32;
        let swell = (t * 3.1).sin().abs() * 0.6 + 0.1;
        let tone = (t * 220. * std::f32::consts::TAU).sin() * 0.6 + (t * 330. * std::f32::consts::TAU).sin() * 0.3;
        out.extend_from_slice(&((tone * swell * 12_000.) as i16).to_le_bytes());
    }
    out
}

pub(super) fn demo_uploads(waffle: &mut Waffle, window: &mut Window, cx: &mut Context<Waffle>) {
    let _ = crate::discord::DEMO_MEDIA.set(demo_media);
    // Set without saving: the fixture never writes the real settings file.
    waffle.prefs.show_image_descriptions = true;
    let dir = std::env::temp_dir().join("waffle-demo-uploads");
    let _ = std::fs::create_dir_all(&dir); // lint-allow(main-thread-io): fixture file written while the demo scene builds the window, before its first frame
    let picture = |name: &str, n: usize| {
        let path = dir.join(name);
        let _ = std::fs::write(&path, demo_picture(n, 1200, 800)); // lint-allow(main-thread-io): fixture file written while the demo scene builds the window, before its first frame
        path
    };
    let pdf = dir.join("lantern-festival-flyer.pdf");
    let _ = std::fs::write(&pdf, vec![b'x'; 120_000]); // lint-allow(main-thread-io): fixture file written while the demo scene builds the window, before its first frame
    waffle.add_files(vec![picture("pier.png", 2), picture("the-ending.png", 6), pdf], window, cx);
    if let Some(ending) = waffle.pending_files.get_mut(1) {
        ending.spoiler = true;
        ending.description = Some("The final boss, mid-fall.".into());
    }
    let Some(feed) = waffle.feeds.get_mut("general") else { return };
    let mut messages = feed.messages.as_ref().clone();
    let described = |n: usize, id: &str, alt: &str| Media { id: Some(id.into()), description: Some(alt.into()), ..demo_item(n) };
    let post = |id: &str, author: User, content: &str, media: Vec<Media>| Message {
        id: id.into(),
        channel_id: "general".into(),
        guild_id: Some("1".into()),
        author,
        content: content.into(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        media,
        ..Default::default()
    };
    messages.push(post(
        "120",
        me_user(),
        "pier at dusk",
        vec![described(0, "a120", "A wooden pier running into calm water at dusk, the sun low on the left."), described(1, "a121", "A rowing boat tied up under the pier.")],
    ));
    messages.push(post("121", user("8388608", "Wren"), "", vec![described(2, "a122", "A green hillside under a flat grey sky, a single bench at the top.")]));
    feed.messages = std::sync::Arc::new(messages);
}

pub(super) fn demo_text_files(waffle: &mut Waffle, scene: &str) {
    use crate::{discord::File, text_file::TextFile};
    let notes: String = PATCH_NOTES.repeat(3);
    let hello = "fn main() {\n    let name = std::env::args().nth(1).unwrap_or(\"world\".into());\n    println!(\"hello, {name}\");\n}\n";
    let file = |name: &str, size: usize| File { name: name.into(), url: format!("https://cdn.discordapp.com/attachments/demo/{name}"), size: size as u64, ..Default::default() };
    let (long, short, binary) = (file("message.txt", notes.len()), file("hello.rs", hello.len()), file("crash-dump.log", 48_213));
    {
        let mut texts = waffle.text_files.borrow_mut();
        texts.files.insert(long.url.clone(), TextFile::Ready { text: notes.into(), truncated: false });
        texts.files.insert(short.url.clone(), TextFile::Ready { text: hello.into(), truncated: false });
        texts.files.insert(binary.url.clone(), TextFile::Unreadable);
        match scene {
            "text-viewer" => texts.viewer = Some(long.clone()),
            "text-expanded" => drop(texts.expanded.insert(long.url.clone())),
            _ => {}
        }
    }
    let Some(feed) = waffle.feeds.get_mut("general") else { return };
    let mut messages = feed.messages.as_ref().clone();
    let author = user("12582912", "moss lantern");
    let post = |id: &str, files: Vec<File>| Message { id: id.into(), channel_id: "general".into(), guild_id: Some("1".into()), author: author.clone(), timestamp: "2026-11-14T08:20:00Z".into(), files, ..Default::default() };
    messages.push(post("106", vec![long]));
    messages.push(post("107", vec![short, binary]));
    feed.messages = std::sync::Arc::new(messages);
}

// Wren forwards a clip from #📷-media into #👋-general, with a note under it.
pub(super) fn demo_report(waffle: &mut Waffle, scene: &str, cx: &mut Context<Waffle>) {
    use crate::report::{ReportMenu, ReportTarget};
    let Some(message) = waffle.feeds.get("general").and_then(|f| f.messages.iter().find(|m| m.id == "102").cloned()) else { return };
    let target = match scene {
        "report-user" => ReportTarget::User { user: Box::new(message.author.clone()), guild_id: Some("1".into()) },
        _ => ReportTarget::Message(Box::new(message)),
    };
    waffle.open_report(target, cx);
    waffle.report_menu_loaded(ReportMenu::parse(&sample_report_menu()), cx);
    if scene == "report" || scene == "report-user" {
        return;
    }
    waffle.report_step("2".into(), Some("Spam".into()), cx);
    waffle.report_step("10".into(), Some("Unsolicited ads or links".into()), cx);
    if let Some(dialog) = waffle.report_dialog.as_mut() {
        dialog.also_block = true;
    }
    if scene == "report-sent" {
        waffle.report_sent(true, cx);
    }
}

// Discord's shape (docs/discord-report-re.md §7) with made-up reasons and copy.
pub(super) fn sample_report_menu() -> serde_json::Value {
    let preview = serde_json::json!({ "type": "message_preview", "name": "message_preview", "data": null });
    serde_json::json!({
        "version": "demo", "language": "en", "root_node_id": 1, "success_node_id": 90, "fail_node_id": 91,
        "nodes": {
            "1": { "id": 1, "header": "What's wrong with this message?", "subheader": "Pick the closest match. Reports are anonymous.", "elements": [preview, { "type": "user_preview", "name": "user_preview" }],
                   "children": [["Spam", 2], ["Abuse or harassment", 3], ["Harmful content", 4], ["Something else", 5]] },
            "2": { "id": 2, "header": "What kind of spam?", "elements": [], "children": [["Unsolicited ads or links", 10], ["Scam or phishing", 10], ["Flooding the chat", 10]] },
            "3": { "id": 3, "header": "Who is it aimed at?", "elements": [], "children": [["Me", 10], ["Someone else", 10], ["A group of people", 10]] },
            "4": { "id": 4, "header": "What does it show?", "elements": [], "children": [["Violence or gore", 10], ["Sexual content", 10], ["Self-harm", 11]] },
            "5": { "id": 5, "header": "Tell us more", "elements": [
                     { "type": "checkbox", "name": "details", "should_submit_data": true, "data": [["imp", "Impersonation", "Pretending to be someone else", false], ["pii", "Private information", "", false]] }
                   ], "button": { "type": "next", "target": 10 } },
            "10": { "id": 10, "header": "Submit your report", "subheader": "We'll look at this message and what it's in reply to.", "elements": [preview, { "type": "breadcrumbs", "name": "breadcrumbs" }], "button": { "type": "submit" } },
            "11": { "id": 11, "header": "You're not alone", "info": "If someone is in danger, contact local emergency services. Crisis lines are listed at [discord.com/safety](https://discord.com/safety).",
                    "elements": [{ "type": "external_link", "name": "help", "data": { "url": "https://discord.com/safety", "link_text": "Safety Center", "link_description": "Resources and crisis lines" } }],
                    "button": { "type": "next", "target": 10 } },
            "90": { "id": 90, "header": "Thanks for letting us know", "subheader": "We'll review it against the Community Guidelines.", "button": { "type": "done" },
                    "elements": [{ "type": "success", "name": "success" }, { "type": "breadcrumbs", "name": "breadcrumbs" }, { "type": "block_users", "name": "block" }, { "type": "ignore_users", "name": "ignore" }, { "type": "delete_message", "name": "delete" }] },
            "91": { "id": 91, "header": "Something went wrong", "elements": [], "button": { "type": "done" } }
        }
    })
}

pub(super) fn demo_forwarded(waffle: &mut Waffle) {
    let Some(feed) = waffle.feeds.get_mut("general") else { return };
    let wren = user("8388608", "Wren");
    let original = Message {
        id: "880".into(),
        channel_id: "media".into(),
        guild_id: Some("1".into()),
        content: "caught this on stream last night".into(),
        timestamp: (chrono::Utc::now() - chrono::Duration::days(1)).to_rfc3339(),
        media: vec![Media { url: avatar_url("3", None), width: 320., height: 200., kind: MediaKind::Image, ..Default::default() }],
        ..Default::default()
    };
    let post = |id: &str, content: &str, forward: Option<Box<Message>>| Message {
        id: id.into(),
        channel_id: "general".into(),
        guild_id: Some("1".into()),
        author: wren.clone(),
        content: content.into(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        forward,
        ..Default::default()
    };
    let mut messages = feed.messages.as_ref().clone();
    messages.push(post("901", "", Some(Box::new(original))));
    messages.push(post("902", "you two need to see this", None));
    feed.messages = std::sync::Arc::new(messages);
    waffle.list.reset(waffle.feeds.get("general").map_or(0, |f| f.messages.len()));
}

pub(super) fn demo_media_settings(waffle: &mut Waffle) {
    use crate::discord::{File, Sticker, StickerFormat};
    waffle.account_settings.inline_attachment_media = false;
    waffle.account_settings.gif_auto_play = false;
    waffle.account_settings.animate_stickers = 1;
    waffle.prefs.spoilers = crate::settings::Spoilers::Always;
    let Some(feed) = waffle.feeds.get_mut("general") else { return };
    let wren = user("8388608", "Wren");
    let post = |id: &str, content: &str| Message { id: id.into(), channel_id: "general".into(), guild_id: Some("1".into()), author: wren.clone(), content: content.into(), timestamp: chrono::Utc::now().to_rfc3339(), ..Default::default() };
    let upload = |name: &str, size: u64| File { name: name.into(), url: format!("https://cdn.discordapp.com/attachments/demo/{name}"), size, ..Default::default() };
    let mut messages = feed.messages.as_ref().clone();
    messages.push(Message {
        media: vec![Media { url: avatar_url("3", None), width: 640., height: 400., kind: MediaKind::Image, ..Default::default() }, Media { url: avatar_url("5", None), width: 640., height: 400., kind: MediaKind::Image, ..Default::default() }],
        media_files: vec![upload("sunset.png", 482_113), upload("SPOILER_ending.png", 210_554)],
        ..post("951", "two from last night, second one spoils the ending")
    });
    messages.push(Message { media: vec![Media { url: avatar_url("4", None), width: 320., height: 320., kind: MediaKind::Image, ..Default::default() }], ..post("952", "https://example.com/cat.png") });
    messages.push(Message { media: vec![Media { url: "https://example.com/dance.mp4".into(), width: 320., height: 240., kind: MediaKind::Gif { poster: avatar_url("2", None) }, ..Default::default() }], ..post("953", "https://tenor.com/view/dance-gif-1") });
    messages.push(post("954", "<a:dance:1328465377218269257> the boss dies in phase ||two, not three||"));
    messages.push(Message { stickers: vec![Sticker { id: "781291131828699156".into(), name: "Wave".into(), format: StickerFormat::Apng, tags: String::new() }], ..post("955", "") });
    feed.messages = std::sync::Arc::new(messages);
    waffle.list.reset(waffle.feeds.get("general").map_or(0, |f| f.messages.len()));
}
