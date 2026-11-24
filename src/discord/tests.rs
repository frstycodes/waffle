// lint-allow(file-lines): the gateway and parser unit tests, one function per payload shape.
use super::*;

#[test]
fn activity_payloads() {
    let update = json!({
        "application_id": "945737671223947305",
        "launch_id": "l1",
        "location": { "id": "gc-9-7", "kind": "gc", "channel_id": "7", "guild_id": "9" },
        "participants": [{ "user_id": "u1", "session_id": "s1", "nonce": null }],
    });
    let instance = parse_activity_instance(&update).expect("parses");
    assert_eq!((instance.instance_id.as_str(), instance.location_id.as_str(), instance.channel_id.as_str()), ("l1", "gc-9-7", "7"));
    assert_eq!(instance.participants, vec![ActivityParticipant { user_id: "u1".into(), session_id: "s1".into() }]);
    let composite = json!({ "application_id": "1", "launch_id": "l", "composite_instance_id": "c", "location": { "id": "pc-3", "channel_id": "3" } });
    let dm = parse_activity_instance(&composite).expect("parses");
    assert_eq!((dm.instance_id.as_str(), dm.guild_id.as_deref(), dm.participants.len()), ("c", None, 0));

    let shelf = json!({
        "activities": [{ "application_id": "b", "shelf_rank": 2 }, { "application_id": "a", "shelf_rank": 1 }, { "application_id": "gone", "shelf_rank": 0 }],
        "applications": [{ "id": "a", "name": "Alpha", "icon": "h" }, { "id": "b", "name": "Beta", "description": "golf" }],
    });
    let apps = parse_shelf(&shelf);
    assert_eq!(apps.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["Alpha", "Beta"]);
    assert!(apps[0].icon.as_deref().is_some_and(|u| u.contains("/app-icons/a/h.png")));

    // A /launch post as the API returns it (2026-11-23): the app under `application`, no `application_id`.
    let launch = json!({
        "id": "m1", "channel_id": "7", "type": 23, "content": "", "timestamp": "2026-11-22T21:15:24.986000+00:00",
        "author": { "id": "945737671223947305", "username": "Putt Party", "bot": true },
        "application": { "id": "945737671223947305", "name": "Putt Party" },
        "activity_instance": { "id": "i-1-gc-9-7" },
        "interaction_metadata": { "id": "1", "type": 2, "user": { "id": "u1", "username": "_frsty", "global_name": "frsty" } },
    });
    let posted = parse_message(&launch).and_then(|m| m.activity).expect("an activity post");
    assert_eq!((posted.instance_id.as_str(), posted.app_id.as_str(), posted.starter.as_deref()), ("i-1-gc-9-7", "945737671223947305", Some("frsty")));
    assert_eq!(posted.app_name.as_deref(), Some("Putt Party"));
    assert_eq!(oauth_code("https://x.discordsays.com/?code=abc&state=s").as_deref(), Some("abc"));
    assert_eq!(oauth_code("https://x.discordsays.com/?error=access_denied"), None);
}

#[test]
fn channel_info_carries_start_times_and_statuses() {
    let info = json!({ "guild_id": "g", "channels": [{ "id": "a", "status": "lofi <:waffle:42>", "voice_start_time": 1700 }, { "id": "b", "status": null }] });
    let Some(Event::Batch(events)) = dispatch("CHANNEL_INFO", &info) else { panic!("a batch") };
    let [Event::VoiceStartTimes { guild_id, times, all: true }, Event::VoiceStatuses { statuses, all: true, .. }] = events.as_slice() else { panic!("times then statuses") };
    assert_eq!(guild_id, "g");
    assert_eq!(times, &[("a".to_owned(), Some(1700)), ("b".to_owned(), None)]);
    assert_eq!(statuses, &[("a".to_owned(), "lofi <:waffle:42>".to_owned()), ("b".to_owned(), String::new())]);
    let update = json!({ "id": "a", "guild_id": "g", "status": "" });
    let Some(Event::VoiceStatuses { guild_id, statuses, all: false }) = dispatch("VOICE_CHANNEL_STATUS_UPDATE", &update) else { panic!("one status") };
    assert_eq!((guild_id.as_str(), statuses), ("g", vec![("a".to_owned(), String::new())]));
}

#[test]
fn display_name_styles_parse_with_member_override() {
    let user = serde_json::json!({ "id": "1", "global_name": "Yumiko", "display_name_styles": { "font_id": 12, "effect_id": 2, "colors": [2797222, 16762000, -1, 16777216] } });
    let style = parse_user(&user).name_style.expect("styled");
    assert_eq!(style, NameStyle { font: 12, effect: NameEffect::Gradient, colors: vec![0x2AAEA6, 0xFFC490] }, "out-of-range colours dropped");
    assert_eq!(parse_user(&serde_json::json!({ "id": "1", "display_name_styles": null })).name_style, None);
    let unknown = serde_json::json!({ "id": "1", "display_name_styles": { "effect_id": 1001 } });
    assert_eq!(parse_user(&unknown).name_style, Some(NameStyle { font: 11, effect: NameEffect::Solid, colors: vec![] }));
    let member = serde_json::json!({ "display_name_styles": { "font_id": 3, "effect_id": 8, "colors": [255] } });
    assert_eq!(parse_member_user(&member, &user, Some("g")).name_style.map(|s| s.effect), Some(NameEffect::Gummy), "the server's style wins");
}

#[test]
fn pin_notice_names_the_pinned_message() {
    let notice = json!({ "id": "9", "channel_id": "1", "type": 6, "author": { "id": "5", "username": "frsty" }, "message_reference": { "message_id": "3" } });
    let notice = parse_message(&notice).expect("parses");
    assert!(notice.pin_notice);
    assert_eq!(notice.reply_to.as_deref(), Some("3"));
    assert!(!parse_message(&json!({ "id": "3", "channel_id": "1", "type": 0, "author": { "id": "5" } })).expect("parses").pin_notice);
}

#[test]
fn send_options_ride_on_the_body_and_come_back_parsed() {
    let mut body = message_body("hi", "n", None, false, &[]);
    SendOptions { silent: true, tts: true }.apply(&mut body);
    assert_eq!((body["flags"].as_u64(), body["tts"].as_bool()), (Some(4096), Some(true)));
    let mut plain = message_body("hi", "n", None, false, &[]);
    SendOptions::default().apply(&mut plain);
    assert!(plain.get("flags").is_none() && plain.get("tts").is_none());
    let sent = parse_message(&json!({ "id": "1", "channel_id": "c", "author": { "id": "u" }, "flags": 4096, "tts": true })).expect("parses");
    assert_eq!(sent.send_options(), SendOptions { silent: true, tts: true });
}

fn upload(name: &str, size: u64) -> Upload {
    Upload { path: PathBuf::from(format!("/tmp/{name}")), name: name.into(), size, description: None, spoiler: false, sent: Default::default() }
}

#[test]
fn http_failures_retry_only_where_discord_allows() {
    let limited = json!({ "message": "You are being rate limited.", "retry_after": 0.5, "global": false });
    assert_eq!(retry_wait(429, &limited, None, false, 0), Some(Duration::from_millis(500)), "429 retries even a POST");
    assert_eq!(retry_wait(429, &limited, None, false, 2), None, "two retries at most");
    assert_eq!(retry_wait(429, &json!({ "retry_after": 60.0 }), None, true, 0), None, "too long to wait");
    assert_eq!(retry_wait(429, &Value::Null, Some(2.), true, 0), Some(Duration::from_secs(2)), "Retry-After header");
    assert_eq!(retry_wait(502, &Value::Null, None, true, 0), Some(Duration::from_secs(1)));
    assert_eq!(retry_wait(502, &Value::Null, None, false, 0), None, "a POST never repeats on 5xx");
    assert_eq!(retry_wait(502, &Value::Null, None, true, 1), None);
    assert_eq!(retry_wait(429, &json!({ "retry_after": 3.0 }), Some(2.), true, 0), Some(Duration::from_secs(2)), "the header wins, as in Discord's client");
    assert_eq!(retry_wait(429, &Value::Null, None, true, 0), Some(Duration::from_secs(5)), "5s when nothing says");
    for status in [400, 401, 403, 404, 405, 500, 503] {
        assert_eq!(retry_wait(status, &Value::Null, None, true, 0), None, "{status}");
    }
    assert_eq!(error_text(403, &json!({ "message": "Missing Permissions", "code": 50013 })), "Missing Permissions");
    assert_eq!(error_text(404, &Value::Null), "404 Not Found");
}

#[test]
fn command_index_waits_on_202_and_429() {
    assert_eq!(index_wait(202, &Value::Null, None), Some(Duration::from_secs(5)));
    assert_eq!(index_wait(429, &json!({ "retry_after": 1.5 }), None), Some(Duration::from_millis(1500)));
    assert_eq!(index_wait(200, &Value::Null, None), None);
    assert_eq!(index_wait(500, &Value::Null, None), None);
}

#[test]
fn interaction_refusals_read_as_discords_client_reads_them() {
    let stale = json!({ "code": 50035, "message": "Invalid Form Body",
        "errors": { "data": { "_errors": [{ "code": "INTERACTION_APPLICATION_COMMAND_INVALID_VERSION", "message": "This command is outdated, please try again in a few minutes" }] } } });
    let e = interact_error(400, &stale);
    assert!(e.stale_index);
    assert_eq!(e.message.as_deref(), Some("This command is outdated, please try again in a few minutes"));
    assert_eq!(e.code, None);
    let other = interact_error(403, &json!({ "code": 50001, "message": "Missing Access" }));
    assert_eq!((other.code, other.message.as_deref(), other.stale_index), (Some(50001), Some("Missing Access"), false));
    assert!(interact_error(400, &json!({ "code": 10005, "message": "Unknown Integration" })).stale_index);
    let limited = interact_error(429, &json!({ "message": "You are being rate limited.", "retry_after": 2.0 }));
    assert_eq!((limited.status, limited.retry_after), (Some(429), Some(2.0)));
    assert_eq!(interact_error(500, &Value::Null), InteractError { status: Some(500), ..Default::default() });
}

#[test]
fn every_gateway_close_code_has_its_documented_outcome() {
    for code in [1000, 1001, 1006, 4000, 4001, 4002, 4003, 4005, 4008] {
        assert_eq!(close_kind(code), Close::Resume, "{code}");
    }
    for code in [4007, 4009, 4015, 4016] {
        assert_eq!(close_kind(code), Close::Identify, "{code}");
    }
    for code in 4010..=4014 {
        assert_eq!(close_kind(code), Close::Fatal, "{code}");
    }
    assert_eq!(close_kind(4004), Close::AuthFailed);
    assert_eq!(gateway_close_text(4009, ""), "Session timed out (4009)");
    assert!(is_unauthorized(&error_text(401, &json!({ "message": "401: Unauthorized", "code": 0 }))));
    assert!(!is_unauthorized(&error_text(403, &json!({ "message": "Missing Access", "code": 50001 }))));
}

#[test]
fn pins_parse_both_the_new_and_the_deprecated_shape() {
    let message = |id: &str| json!({ "id": id, "channel_id": "c", "author": { "id": "u" } });
    let new = json!({ "items": [{ "pinned_at": "2026-01-01T00:00:00Z", "message": message("2") }, { "pinned_at": "x", "message": message("1") }], "has_more": false });
    assert_eq!(parse_pins(&new).iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["2", "1"]);
    assert_eq!(parse_pins(&json!([message("3")])).len(), 1);
}

#[test]
fn upload_limit_is_the_higher_of_nitro_and_boosts() {
    assert_eq!(upload_limit(0, 0), 10 << 20);
    assert_eq!(upload_limit(0, 1), 10 << 20, "boost level 1 adds nothing");
    assert_eq!(upload_limit(3, 0), 50 << 20, "Nitro Basic");
    assert_eq!(upload_limit(1, 3), 100 << 20, "the server's level 3 beats Classic");
    assert_eq!(upload_limit(2, 3), 500 << 20, "Nitro beats every server");
}

#[test]
fn asks_for_one_upload_url_per_file() {
    let body = attachments_request(&[upload("cat.png", 1200), upload("SPOILER_notes.txt", 3)]);
    assert_eq!(body, json!({ "files": [{ "id": "0", "filename": "cat.png", "file_size": 1200 }, { "id": "1", "filename": "SPOILER_notes.txt", "file_size": 3 }] }));
}

#[test]
fn upload_urls_come_back_in_request_order() {
    let body = json!({ "attachments": [
        { "id": 1, "upload_url": "https://up/b", "upload_filename": "u/b.txt" },
        { "id": 0, "upload_url": "https://up/a", "upload_filename": "u/a.png" },
    ] });
    assert_eq!(parse_upload_slots(&body, 2).unwrap(), vec![("https://up/a".into(), "u/a.png".into()), ("https://up/b".into(), "u/b.txt".into())]);
    assert!(parse_upload_slots(&body, 3).is_err(), "a missing slot is an error, never a message without the file");
}

#[test]
fn the_message_names_each_uploaded_file() {
    let sent = |filename: &str, description: Option<&str>, spoiler| SentAttachment { filename: filename.into(), uploaded_filename: format!("u/{filename}"), description: description.map(Into::into), spoiler };
    let body = message_body("look", "n1", Some("42"), false, &[sent("cat.png", None, false), sent("end.png", Some("The boss falls"), true), sent("blank.png", Some(""), false)]);
    assert_eq!(
        body["attachments"],
        json!([
            { "id": "0", "filename": "cat.png", "uploaded_filename": "u/cat.png" },
            { "id": "1", "filename": "end.png", "uploaded_filename": "u/end.png", "description": "The boss falls", "is_spoiler": true },
            { "id": "2", "filename": "blank.png", "uploaded_filename": "u/blank.png", "description": "" },
        ]),
        "description only when set, is_spoiler only when true, never a SPOILER_ name"
    );
    assert_eq!(body["message_reference"], json!({ "message_id": "42" }));
    assert_eq!(body["allowed_mentions"]["replied_user"], json!(false));
    assert!(message_body("hi", "n2", None, true, &[]).get("attachments").is_none(), "plain text carries no attachments key");
}

#[test]
fn modifying_a_sent_attachment_lists_every_one() {
    let ids = ["10".to_owned(), "11".to_owned(), "12".to_owned()];
    assert_eq!(
        attachments_patch(&ids, "11", "A pier at dusk", false),
        json!({ "attachments": [{ "id": "10" }, { "id": "11", "description": "A pier at dusk", "is_spoiler": false }, { "id": "12" }] })
    );
    assert_eq!(attachments_patch(&ids[..1], "10", "", true), json!({ "attachments": [{ "id": "10", "description": "", "is_spoiler": true }] }));
}

#[test]
fn reads_alt_text_and_spoiler_flags() {
    let m = parse_message(&json!({ "id": "1", "channel_id": "c", "author": { "id": "u" }, "attachments": [
        { "id": "a1", "url": "a.png", "width": 10, "height": 20, "content_type": "image/png", "description": "A cat", "flags": 8 },
        { "id": "a2", "url": "b.png", "width": 10, "height": 20, "content_type": "image/png", "description": "" },
        { "id": "a3", "url": "c.zip", "filename": "c.zip", "size": 5, "flags": 8 },
    ] }))
    .unwrap();
    assert_eq!((m.media[0].id.as_deref(), m.media[0].description.as_deref(), m.media[0].spoiler), (Some("a1"), Some("A cat"), true));
    assert_eq!((m.media[1].description.as_deref(), m.media[1].spoiler), (None, false));
    assert_eq!((m.files[0].id.as_str(), m.files[0].spoiler), ("a3", true));
}

#[test]
fn reads_the_activity_feed() {
    let body = json!({ "entries": [
        { "content": { "id": "1300000000000000000", "author_id": "7", "content_type": 1, "ended_at": "2026-11-17T10:00:00+00:00",
            "extra": { "type": "played_game_extra", "game_name": "EA SPORTS FC 26" }, "traits": [{ "type": 8, "streak_count_days": 6 }] } },
        { "content": { "id": "1300000000000000001", "author_id": "8", "content_type": 5,
            "extra": { "entries": [{ "media": { "title": "Bad Apple!!", "image_url": "https://i.scdn.co/image/x" } }] }, "traits": [{ "type": 3, "is_live": true }] } },
        { "content": { "id": "1", "author_id": "9", "content_type": 9, "extra": {} } }
    ]});
    let feed = parse_activity_feed(&body);
    assert_eq!(feed.len(), 2);
    assert_eq!((feed[0].title.as_str(), feed[0].streak_days, feed[0].kind), ("EA SPORTS FC 26", Some(6), ActivityKind::Playing));
    assert_eq!(feed[0].at, 1_794_909_600_000);
    assert_eq!((feed[1].kind, feed[1].live, feed[1].image.as_deref()), (ActivityKind::Listening, true, Some("https://i.scdn.co/image/x")));
    assert_eq!(feed[1].at, (1_300_000_000_000_000_001i64 >> 22) + 1_420_070_400_000);
}

#[test]
fn parses_both_ready_shapes() {
    let legacy = json!({ "id": "1", "name": "Rust", "icon": "abc", "channels": [
        { "id": "b", "type": 0, "name": "general", "position": 2 },
        { "id": "a", "type": 4, "name": "Text", "position": 1 },
    ]});
    let guild = parse_guild(&legacy).unwrap();
    assert_eq!(guild.name, "Rust");
    assert_eq!(guild.icon.as_deref(), Some("https://cdn.discordapp.com/icons/1/abc.png?size=96"));
    assert_eq!(guild.channels[0].kind, ChannelKind::Category);

    let deduped = json!({ "id": "2", "properties": { "name": "Zed", "icon": null }, "channels": [] });
    let guild = parse_guild(&deduped).unwrap();
    assert_eq!(guild.name, "Zed");
    assert!(guild.icon.is_none());

    let users = json!([{ "id": "9", "username": "ferris", "global_name": "Ferris", "avatar": "h" }]);
    let dm = parse_dm(&json!({ "id": "d", "type": 1, "recipient_ids": ["9"] }), &users).unwrap();
    assert_eq!(dm.name, "Ferris");
    assert_eq!(dm.recipient.unwrap().avatar, "https://cdn.discordapp.com/avatars/9/h.png?size=128");
}

#[test]
fn group_dm_faces() {
    let users = json!([]);
    let group = |extra: Value| {
        let mut c = json!({ "id": "g", "type": 3, "recipients": [{ "id": "1", "username": "a", "avatar": "x" }, { "id": "2", "username": "b", "avatar": "y" }, { "id": "3", "username": "c" }] });
        c.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        parse_dm(&c, &users).unwrap()
    };
    let plain = group(json!({}));
    assert_eq!(plain.recipients.len(), 3);
    assert_eq!(plain.face(), DmFace::Pair(avatar_url("1", Some("x")), avatar_url("2", Some("y"))));
    assert_eq!(group(json!({ "icon": "i" })).face(), DmFace::Icon("https://cdn.discordapp.com/channel-icons/g/i.png?size=128".into()));

    let left = parse_dm(&json!({ "id": "g", "type": 3, "name": "squad", "recipients": [{ "id": "1", "username": "a", "avatar": "x" }] }), &users).unwrap();
    assert!(left.recipient.is_none());
    assert_eq!(left.face(), DmFace::One(avatar_url("1", Some("x"))));
    let alone = parse_dm(&json!({ "id": "g", "type": 3, "name": "squad", "recipients": [] }), &users).unwrap();
    assert_eq!(alone.face(), DmFace::Letters("squad".into()));
}

#[test]
fn dm_events_and_request_flags() {
    let Some(Event::DmUpsert(dm)) = dispatch("CHANNEL_CREATE", &json!({ "id": "d", "type": 1, "is_message_request": true, "recipients": [{ "id": "1", "username": "a" }] })) else { panic!("a DM create is a DmUpsert") };
    assert!(dm.props.message_request && !dm.props.spam && dm.is_request());
    let Some(Event::DmUpsert(group)) = dispatch("CHANNEL_UPDATE", &json!({ "id": "g", "type": 3, "owner_id": "1", "is_spam": true, "recipients": [] })) else { panic!("a group update is a DmUpsert") };
    assert_eq!(group.props.owner_id.as_deref(), Some("1"));
    assert!(group.is_request());
    assert!(matches!(dispatch("CHANNEL_DELETE", &json!({ "id": "d", "type": 1 })), Some(Event::DmDelete(id)) if id == "d"));
    assert!(matches!(dispatch("CHANNEL_CREATE", &json!({ "id": "c", "type": 0, "guild_id": "s" })), Some(Event::ChannelUpsert { .. })));
    let Some(Event::RecipientChange { channel_id, user, added }) = dispatch("CHANNEL_RECIPIENT_REMOVE", &json!({ "channel_id": "g", "user": { "id": "2", "username": "b" } })) else { panic!("recipient remove") };
    assert_eq!((channel_id.as_str(), user.id.as_str(), added), ("g", "2", false));
}

#[test]
fn default_avatar_when_hash_missing() {
    assert_eq!(avatar_url("4194304", None), "https://cdn.discordapp.com/embed/avatars/1.png");
    assert_eq!(avatar_url("1", Some("0")), "https://cdn.discordapp.com/embed/avatars/0.png");
}

#[test]
fn sizes_images_through_the_media_proxy() {
    let signed = "https://cdn.discordapp.com/attachments/1/2/shot.png?ex=a&is=b&hm=c&";
    assert_eq!(
        sized_image(signed, 400., 225., (3840., 2160.)),
        "https://media.discordapp.net/attachments/1/2/shot.png?ex=a&is=b&hm=c&&width=800&height=450"
    );
    assert_eq!(sized_image("https://cdn.discordapp.com/attachments/1/2/a.png", 100., 100., (150., 150.)), "https://media.discordapp.net/attachments/1/2/a.png", "never upscaled");
    assert_eq!(sized_image("https://example.com/a.png", 10., 10., (999., 999.)), "https://example.com/a.png");
}

#[test]
fn thumbnails_cover_a_square_through_the_proxy() {
    let photo = Media { url: "https://cdn.discordapp.com/attachments/1/2/a.png?ex=a&is=b&hm=c&".into(), width: 4000., height: 3000., kind: MediaKind::Image, ..Default::default() };
    assert_eq!(thumb_url(&photo, 112.), "https://media.discordapp.net/attachments/1/2/a.png?ex=a&is=b&hm=c&&width=149&height=112&format=webp", "shorter edge fills the square");
    let tall = Media { width: 300., height: 1200., ..photo.clone() };
    assert!(thumb_url(&tall, 112.).ends_with("width=112&height=448&format=webp"));
    let small = Media { width: 64., height: 64., ..photo.clone() };
    assert_eq!(thumb_url(&small, 112.), "https://media.discordapp.net/attachments/1/2/a.png?ex=a&is=b&hm=c&&format=webp", "never upscaled");
    let clip = Media { url: "https://cdn.discordapp.com/attachments/1/2/v.mp4".into(), width: 1920., height: 1080., kind: MediaKind::Video { poster: "https://media.discordapp.net/attachments/1/2/v.mp4?format=jpeg".into() }, ..Default::default() };
    assert_eq!(thumb_url(&clip, 96.), "https://media.discordapp.net/attachments/1/2/v.mp4?format=jpeg&width=171&height=96", "video keeps its proxy still, sized");
    let elsewhere = Media { url: "https://example.com/a.png".into(), ..photo };
    assert_eq!(thumb_url(&elsewhere, 96.), "https://example.com/a.png");
}

#[test]
fn locks_channels_hidden_from_everyone() {
    let hidden = json!({ "id": "c", "type": 0, "permission_overwrites": [{ "id": "g", "type": 0, "deny": "1024", "allow": "0" }] });
    let open = json!({ "id": "c", "type": 0, "permission_overwrites": [{ "id": "g", "type": 0, "deny": "2048", "allow": "0" }] });
    assert!(parse_channel(&hidden, "g").unwrap().private);
    assert!(!parse_channel(&open, "g").unwrap().private);
}

#[test]
fn form_errors_name_the_field() {
    let body = json!({ "code": 50035, "message": "Invalid Form Body", "errors": { "name": { "_errors": [{ "code": "BASE_TYPE_BAD_LENGTH", "message": "Must be between 1 and 100 in length." }] } } });
    assert_eq!(api_error(&body).as_deref(), Some("Invalid Form Body: name: Must be between 1 and 100 in length."));
    assert_eq!(api_error(&json!({ "message": "Missing Permissions", "code": 50013 })).as_deref(), Some("Missing Permissions"));
    assert_eq!(api_error(&Value::Null), None);
}

#[test]
fn joined_invite_names_its_server() {
    assert_eq!(joined_guild(&json!({ "code": "x", "guild": { "id": "7" } })).as_deref(), Some("7"));
    assert_eq!(joined_guild(&json!({ "code": "x", "guild_id": "7" })).as_deref(), Some("7"));
    assert_eq!(joined_guild(&json!({ "code": "x", "channel": { "id": "9" } })), None);
}

#[test]
fn hides_channels_we_cannot_view() {
    let role = |id: &str, permissions: u64| Role { id: id.into(), name: id.into(), color: 0, position: 0, permissions, managed: false, icon: None };
    let channel = |id: &str, overwrites: Value| parse_channel(&json!({ "id": id, "type": 0, "permission_overwrites": overwrites }), "g").unwrap();
    let deny_everyone = json!({ "id": "g", "type": 0, "deny": "1024", "allow": "0" });
    let guild = Guild {
        id: "g".into(),
        name: "g".into(),
        icon: None,
        channels: vec![
            channel("open", json!([])),
            channel("staff", json!([deny_everyone, { "id": "mods", "type": 0, "allow": "1024", "deny": "0" }])),
            channel("mine", json!([deny_everyone, { "id": "me", "type": 1, "allow": "1024", "deny": "0" }])),
            channel("secret", json!([deny_everyone])),
            channel("kicked", json!([{ "id": "me", "type": 1, "allow": "0", "deny": "1024" }])),
        ],
        roles: vec![role("g", VIEW_CHANNEL), role("mods", 0), role("admins", perm::ADMINISTRATOR)],
        voice_states: Vec::new(),
        stage_instances: Vec::new(),
        emojis: Vec::new(),
        stickers: Vec::new(),
        owner_id: Some("owner".into()),
        premium_tier: 0,
        afk_channel_id: None,
        timed_out_until: None,
        system_channel_flags: 0,
        events: Vec::new(),
        public_updates_channel_id: None,
        vanity_url_code: None,
        threads: Vec::new(),
        my_roles: Vec::new(),
        default_notifications: Default::default(),
        ..Default::default()
    };
    fn visible(g: &Guild) -> Vec<&str> {
        g.channels.iter().filter(|c| g.can_view("me", c)).map(|c| c.id.as_str()).collect()
    }
    assert_eq!(visible(&guild), ["open", "mine"]);
    let modded = Guild { my_roles: vec!["mods".into()], ..guild.clone() };
    assert_eq!(visible(&modded), ["open", "staff", "mine"]);
    let admin = Guild { my_roles: vec!["admins".into()], ..guild.clone() };
    assert_eq!(visible(&admin).len(), 5);
    assert!(guild.can_view("owner", &guild.channels[3]));
}

#[test]
fn manage_messages_follows_owner_admin_roles_and_overwrites() {
    let role = |id: &str, permissions: u64| Role { id: id.into(), name: id.into(), color: 0, position: 0, permissions, managed: false, icon: None };
    let channel = |id: &str, overwrites: Value| parse_channel(&json!({ "id": id, "type": 0, "permission_overwrites": overwrites }), "g").unwrap();
    let thread = parse_channel(&json!({ "id": "t", "type": 11, "parent_id": "locked", "thread_metadata": {} }), "g").unwrap();
    let guild = Guild {
        id: "g".into(),
        name: "g".into(),
        icon: None,
        channels: vec![
            channel("open", json!([])),
            channel("locked", json!([{ "id": "mods", "type": 0, "allow": "0", "deny": "8192" }])),
            channel("mine", json!([{ "id": "mods", "type": 0, "allow": "0", "deny": "8192" }, { "id": "me", "type": 1, "allow": "8192", "deny": "0" }])),
            channel("everyone", json!([{ "id": "g", "type": 0, "allow": "8192", "deny": "0" }])),
        ],
        roles: vec![role("g", VIEW_CHANNEL), role("mods", perm::MANAGE_MESSAGES), role("admins", perm::ADMINISTRATOR)],
        voice_states: Vec::new(),
        stage_instances: Vec::new(),
        emojis: Vec::new(),
        stickers: Vec::new(),
        owner_id: Some("owner".into()),
        premium_tier: 0,
        afk_channel_id: None,
        timed_out_until: None,
        system_channel_flags: 0,
        events: Vec::new(),
        public_updates_channel_id: None,
        vanity_url_code: None,
        threads: vec![thread],
        my_roles: Vec::new(),
        default_notifications: Default::default(),
        ..Default::default()
    };
    let can = |g: &Guild, me: &str, id: &str| g.channel_permissions(me, g.channel(id).unwrap()) & perm::MANAGE_MESSAGES != 0;
    let all = ["open", "locked", "mine", "everyone", "t"];
    let allowed = |g: &Guild, me: &str| all.iter().copied().filter(|id| can(g, me, id)).collect::<Vec<_>>();

    assert_eq!(allowed(&guild, "me"), ["mine", "everyone"]);
    assert_eq!(allowed(&guild, "owner"), all);
    assert_eq!(allowed(&Guild { my_roles: vec!["admins".into()], ..guild.clone() }, "me"), all);
    // The role grants it server-wide; a role overwrite takes it back, a member overwrite beats that, and threads follow their parent.
    assert_eq!(allowed(&Guild { my_roles: vec!["mods".into()], ..guild.clone() }, "me"), ["open", "mine", "everyone"]);
}

#[test]
fn voice_moves_and_invites_follow_channel_permissions() {
    let role = |id: &str, permissions: u64| Role { id: id.into(), name: id.into(), color: 0, position: 0, permissions, managed: false, icon: None };
    let channel = |id: &str, kind: u8, limit: u32, overwrites: Value| parse_channel(&json!({ "id": id, "type": kind, "user_limit": limit, "permission_overwrites": overwrites }), "g").unwrap();
    let in_voice = |user: &str, channel: &str| VoiceState { user_id: user.into(), channel_id: Some(channel.into()), ..Default::default() };
    let deny = |bits: u64| json!([{ "id": "mods", "type": 0, "allow": "0", "deny": bits.to_string() }]);
    let guild = Guild {
        id: "g".into(),
        name: "g".into(),
        icon: None,
        channels: vec![
            channel("rules", 0, 0, deny(perm::CREATE_INSTANT_INVITE)),
            channel("chat", 0, 0, json!([])),
            channel("a", 2, 0, json!([])),
            channel("b", 2, 0, json!([])),
            channel("full", 2, 1, json!([])),
            channel("locked", 2, 0, deny(perm::MOVE_MEMBERS)),
            channel("closed", 2, 0, deny(perm::CONNECT)),
        ],
        roles: vec![role("g", VIEW_CHANNEL | perm::CONNECT | perm::CREATE_INSTANT_INVITE), role("mods", perm::MOVE_MEMBERS)],
        voice_states: vec![in_voice("them", "a"), in_voice("other", "full"), in_voice("held", "locked")],
        emojis: Vec::new(),
        stickers: Vec::new(),
        owner_id: Some("owner".into()),
        premium_tier: 0,
        afk_channel_id: None,
        timed_out_until: None,
        system_channel_flags: 0,
        events: Vec::new(),
        public_updates_channel_id: None,
        vanity_url_code: None,
        threads: Vec::new(),
        my_roles: vec!["mods".into()],
        default_notifications: Default::default(),
        ..Default::default()
    };
    let to = |g: &Guild, me: &str, who: &str| ["b", "full", "locked", "closed"].into_iter().filter(|id| g.can_move(me, who, g.channel(id).unwrap())).collect::<Vec<_>>();

    // Move members on both ends and Connect on the destination; full channels are out.
    assert_eq!(to(&guild, "me", "them"), ["b"]);
    // Not from a channel where Move members is denied.
    assert_eq!(to(&guild, "me", "held"), Vec::<&str>::new());
    // Without the role, nobody else moves.
    assert_eq!(to(&Guild { my_roles: Vec::new(), ..guild.clone() }, "me", "them"), Vec::<&str>::new());
    // Ourselves: Connect is enough.
    assert_eq!(to(&guild, "me", "me"), ["b", "locked"]);
    assert_eq!(to(&guild, "owner", "them"), ["b", "locked", "closed"]);

    assert_eq!(guild.invite_channel("me").map(|c| c.id.as_str()), Some("chat"));
    assert_eq!(guild.invite_channel("owner").map(|c| c.id.as_str()), Some("rules"));
    let no_invites = Guild { roles: vec![role("g", VIEW_CHANNEL)], my_roles: Vec::new(), ..guild.clone() };
    assert_eq!(no_invites.invite_channel("me").map(|c| c.id.as_str()), None);
}

#[test]
fn sorts_attachments_into_images_and_files() {
    let m = parse_message(&json!({ "id": "1", "channel_id": "c", "author": { "id": "u" }, "attachments": [
        { "url": "a.png", "width": 10, "height": 20, "content_type": "image/png", "description": "a red door" },
        { "url": "b.zip", "filename": "b.zip", "size": 5 },
    ], "referenced_message": { "id": "0", "channel_id": "c", "author": { "id": "v", "username": "zoe" }, "content": "hi" },
       "message_reference": { "message_id": "0" } }))
    .unwrap();
    assert_eq!(m.media.len(), 1);
    assert_eq!(m.media[0].description.as_deref(), Some("a red door"));
    assert_eq!(m.files[0].name, "b.zip");
    assert_eq!(m.reply.unwrap().author.name, "zoe");
    assert_eq!(m.reply_to.as_deref(), Some("0"));
}

#[test]
fn folds_poll_counts_into_answers() {
    let m = parse_message(&json!({ "id": "1", "channel_id": "c", "author": { "id": "u" }, "poll": {
        "question": { "text": "Map?" }, "expiry": "2026-11-20T21:00:00Z", "allow_multiselect": true,
        "answers": [
            { "answer_id": 1, "poll_media": { "text": "Peaks", "emoji": { "name": "🏔️" } } },
            { "answer_id": 2, "poll_media": { "text": "Harbor" } },
        ],
        "results": { "is_finalized": false, "answer_counts": [{ "id": 1, "count": 3, "me_voted": true }] },
    } }))
    .unwrap();
    let poll = m.poll.unwrap();
    assert_eq!((poll.question.as_str(), poll.multiselect, poll.finalized), ("Map?", true, false));
    assert_eq!(poll.answers[0], PollAnswer { id: 1, text: "Peaks".into(), emoji: Some(Emoji::unicode("🏔️")), count: 3, me: true });
    assert_eq!((poll.answers[1].count, poll.answers[1].me, poll.answers[1].emoji.is_none()), (0, false, true));
}

#[test]
fn a_forward_carries_its_snapshot_and_is_no_reply() {
    let m = parse_message(&json!({ "id": "2", "channel_id": "dest", "author": { "id": "u" }, "content": "",
        "message_reference": { "type": 1, "message_id": "9", "channel_id": "src", "guild_id": "g" },
        "message_snapshots": [{ "message": { "content": "look", "timestamp": "2026-11-19T23:02:00+00:00",
            "attachments": [{ "url": "a.png", "width": 10, "height": 20, "content_type": "image/png" }] } }] }))
    .unwrap();
    assert_eq!(m.reply_to, None, "a type-1 reference is a forward, not a reply");
    let original = m.forward.expect("forward");
    assert_eq!((original.id.as_str(), original.channel_id.as_str(), original.guild_id.as_deref()), ("9", "src", Some("g")));
    assert_eq!(original.content, "look");
    assert_eq!(original.media.len(), 1);

    let body = forward_body(&original, Some("g"), "n");
    assert_eq!(body["message_reference"], json!({ "type": 1, "message_id": "9", "channel_id": "src", "guild_id": "g" }));
    assert!(body.get("content").is_none());
}

#[test]
fn reads_voice_messages() {
    let m = parse_message(&json!({ "id": "1", "channel_id": "c", "author": { "id": "u" }, "flags": 8192,
        "attachments": [{ "id": "9", "filename": "voice-message.ogg", "url": "https://cdn/v.ogg", "size": 9000, "duration_secs": 4.2, "waveform": "AP8=" }] }))
    .unwrap();
    assert!(m.is_voice());
    let file = &m.files[0];
    assert_eq!((file.id.as_str(), file.duration_secs, file.waveform.as_slice()), ("9", Some(4.2), [0u8, 255].as_slice()));
    assert!(file.is_audio());
    let plain = parse_message(&json!({ "id": "2", "channel_id": "c", "author": { "id": "u" },
        "attachments": [{ "filename": "notes.PDF", "url": "u", "size": 1, "waveform": "not base64!" }] }))
    .unwrap();
    assert!(!plain.is_voice() && !plain.files[0].is_audio() && plain.files[0].waveform.is_empty());
}

#[test]
fn sorts_embeds_by_kind() {
    let m = parse_message(&json!({ "id": "1", "channel_id": "c", "author": { "id": "u" },
        "attachments": [{ "url": "v.mp4", "proxy_url": "https://media/v.mp4", "width": 640, "height": 360, "content_type": "video/mp4" }],
        "embeds": [
            { "type": "gifv", "thumbnail": { "url": "t.png", "width": 200, "height": 100 }, "video": { "url": "g.mp4" } },
            { "type": "image", "thumbnail": { "url": "i.png", "width": 50, "height": 50 } },
            { "type": "video", "title": "Clip", "provider": { "name": "YouTube" }, "thumbnail": { "url": "y.jpg", "width": 1280, "height": 720 } },
        ] }))
    .unwrap();
    assert_eq!(m.media.len(), 3);
    assert_eq!(m.media[0].kind, MediaKind::Video { poster: "https://media/v.mp4?format=jpeg".into() });
    assert_eq!(m.media[1].url, "g.mp4");
    assert_eq!(m.media[1].kind, MediaKind::Gif { poster: "t.png".into() });
    assert_eq!(m.media_files.iter().map(|f| f.url.as_str()).collect::<Vec<_>>(), ["v.mp4"], "only uploads have files");
    assert_eq!(m.embeds.len(), 1);
    assert_eq!(m.embeds[0].provider.as_deref(), Some("YouTube"));
}

fn embedded() -> Value {
    json!({ "id": "1", "channel_id": "c", "author": { "id": "u" }, "content": "look",
        "attachments": [{ "url": "a.png", "width": 10, "height": 10, "content_type": "image/png" }],
        "embeds": [
            { "type": "image", "thumbnail": { "url": "i.png", "width": 50, "height": 50 } },
            { "type": "article", "url": "https://a", "title": "A" },
        ] })
}

#[test]
fn suppressed_embeds_parse_to_none_and_keep_attachments() {
    let mut m = embedded();
    m["flags"] = json!(4);
    let m = parse_message(&m).unwrap();
    assert!(m.embeds_suppressed());
    assert!(m.embeds.is_empty());
    assert_eq!(m.media.iter().map(|m| m.url.as_str()).collect::<Vec<_>>(), ["a.png"]);
}

#[test]
fn update_with_suppress_flag_clears_embeds_and_their_media() {
    let mut m = parse_message(&embedded()).unwrap();
    assert_eq!((m.embeds.len(), m.media.len(), m.embed_media), (1, 2, 1));
    apply_update(&mut m, &json!({ "id": "1", "channel_id": "c", "flags": 4 }));
    assert!(m.embeds.is_empty());
    assert_eq!(m.media.iter().map(|m| m.url.as_str()).collect::<Vec<_>>(), ["a.png"]);
    assert_eq!((m.flags, m.content.as_str(), m.edited), (4, "look", false));
}

#[test]
fn late_embeds_update_replaces_the_old_ones() {
    let mut m = parse_message(&embedded()).unwrap();
    apply_update(&mut m, &json!({ "id": "1", "channel_id": "c", "embeds": [{ "type": "article", "url": "https://b", "title": "B" }] }));
    assert_eq!(m.embeds.iter().map(|e| e.title.as_deref()).collect::<Vec<_>>(), [Some("B")]);
    assert_eq!(m.media.iter().map(|m| m.url.as_str()).collect::<Vec<_>>(), ["a.png"]);
}

#[test]
fn inline_embeds_are_bare_media_without_a_title_or_author() {
    let pic = || json!({ "url": "p", "width": 100, "height": 50 });
    let inline = |e: Value| parse_embed(&e).inline();
    assert!(inline(json!({ "type": "video", "thumbnail": pic(), "video": { "url": "v.mp4" } })));
    assert!(inline(json!({ "type": "article", "image": pic() })));
    assert!(!inline(json!({ "type": "video", "title": "Clip", "thumbnail": pic(), "video": { "url": "v" } })), "titled: a card");
    assert!(!inline(json!({ "type": "rich", "image": pic() })), "a bot's embed is always a card");
    assert!(!inline(json!({ "image": pic() })), "no type means rich");
    assert!(!inline(json!({ "type": "article", "author": { "name": "x" }, "image": pic() })));
    assert!(!inline(json!({ "type": "link", "description": "text only" })), "no media");
}

#[test]
fn embeds_split_thumbnail_from_image_and_fold_shared_urls() {
    let pic = |url: &str| json!({ "url": url, "width": 100, "height": 50 });
    let m = parse_message(&json!({ "id": "1", "channel_id": "c", "author": { "id": "u" }, "embeds": [
        { "url": "https://a", "title": "Set", "thumbnail": pic("t"), "image": pic("1"), "flags": 16 },
        { "url": "https://a", "image": pic("2") },
        { "url": "https://a", "image": pic("3") },
        { "url": "https://a", "image": pic("4") },
        { "url": "https://a", "image": pic("5") },
        { "type": "video", "url": "https://v", "thumbnail": pic("still") },
        { "type": "poll_result", "fields": [
            { "name": "poll_question_text", "value": "Map?" }, { "name": "victor_answer_votes", "value": "3" },
            { "name": "total_votes", "value": "5" }, { "name": "victor_answer_text", "value": "Peaks" }, { "name": "victor_answer_emoji_name", "value": "🏔️" } ] },
    ] }))
    .unwrap();
    assert_eq!(m.embeds.len(), 3);
    let set = &m.embeds[0];
    assert_eq!(set.thumbnail.as_ref().map(|t| t.url.as_str()), Some("t"));
    assert_eq!(set.images.iter().map(|i| i.url.as_str()).collect::<Vec<_>>(), ["1", "2", "3", "4"]);
    assert!(set.sensitive && !set.video);
    let video = &m.embeds[1];
    assert!(video.video && video.thumbnail.is_none());
    assert_eq!(video.images[0].url, "still");
    let poll = m.embeds[2].poll.as_ref().unwrap();
    assert_eq!((poll.question.as_str(), poll.winner_votes, poll.total_votes), ("Map?", 3, 5));
    assert_eq!(poll.winner, Some(("Peaks".into(), Some(Emoji::unicode("🏔️")))));
}

#[test]
fn stickers_parse_from_messages_and_pick_their_format() {
    let m = parse_message(&json!({ "id": "1", "channel_id": "2", "author": { "id": "3" }, "sticker_items": [
        { "id": "10", "name": "wave", "format_type": 1 },
        { "id": "11", "name": "dance", "format_type": 4 },
        { "id": "12", "name": "lottie", "format_type": 3 },
        { "id": "13", "name": "bounce", "format_type": 2 },
    ] }))
    .expect("message");
    let urls: Vec<_> = m.stickers.iter().map(|s| s.url(160)).collect();
    assert_eq!(urls[0].as_deref(), Some("https://media.discordapp.net/stickers/10.png?size=160"));
    assert_eq!(urls[1].as_deref(), Some("https://media.discordapp.net/stickers/11.gif?size=160"));
    assert_eq!(urls[2], None);
    assert_eq!(urls[3].as_deref(), Some("https://media.discordapp.net/stickers/13.webp?size=160&animated=true"));
    assert!(!m.stickers[0].animated() && m.stickers[1].animated());
    let tagged = Sticker { name: "Wumpus wave".into(), tags: "👋, hello".into(), ..Sticker::default() };
    assert!(tagged.matches("wave") && tagged.matches("HELLO") && !tagged.matches("bye"));
}

#[test]
fn guild_settings_read_pin_and_opt_in_flags() {
    let (key, settings) = parse_guild_settings(&json!({
        "guild_id": "g",
        "flags": guild_flag::OPT_IN_CHANNELS_ON,
        "channel_overrides": [
            { "channel_id": "pinned", "flags": channel_flag::FAVORITED | channel_flag::OPT_IN_ENABLED },
            { "channel_id": "plain", "muted": true },
        ],
    }));
    assert_eq!(key, "g");
    assert!(settings.opt_in());
    assert!(settings.pinned("pinned") && settings.opted_in("pinned"));
    assert!(!settings.pinned("plain") && !settings.opted_in("plain") && !settings.pinned("missing"));
    let (key, dms) = parse_guild_settings(&json!({ "guild_id": null, "channel_overrides": [{ "channel_id": "dm", "flags": 2048 }] }));
    assert_eq!(key, "@me");
    assert!(dms.pinned("dm") && !dms.opt_in());
}

#[test]
fn custom_status_reads_settings_and_drops_expired() {
    let now = chrono::Utc::now();
    let soon = (now + chrono::Duration::hours(1)).to_rfc3339();
    let past = (now - chrono::Duration::hours(1)).to_rfc3339();
    let s = CustomStatus::parse(&json!({ "text": "brb", "emoji_name": "🍡", "expires_at": soon }), now).expect("live status");
    assert_eq!((s.text.as_str(), s.emoji.as_ref().map(|e| e.name.as_str())), ("brb", Some("🍡")));
    assert!(CustomStatus::parse(&json!({ "text": "old", "expires_at": past }), now).is_none());
    assert!(CustomStatus::parse(&json!({ "text": "", "emoji_name": null }), now).is_none());
    assert!(CustomStatus::parse(&Value::Null, now).is_none());
    let emoji_only = CustomStatus::parse(&json!({ "emoji_name": "x", "emoji_id": "1" }), now).expect("emoji alone counts");
    assert_eq!(emoji_only.setting()["text"], Value::Null);
    assert_eq!(emoji_only.activity()["emoji"]["id"], "1");
}

#[test]
fn animated_swaps_only_moving_art() {
    assert_eq!(animated(&avatar_url("1", Some("a_abc"))), format!("{CDN}/avatars/1/a_abc.gif?size=128"));
    assert_eq!(animated(&avatar_url("1", Some("abc"))), avatar_url("1", Some("abc")), "a still avatar stays a PNG");
    assert_eq!(animated(&format!("{CDN}/avatar-decoration-presets/x.png?size=96&passthrough=false")), format!("{CDN}/avatar-decoration-presets/x.png?size=96&passthrough=true"));
}

#[test]
fn presence_keeps_custom_status_apart() {
    let p = parse_presence(&json!({ "status": "dnd", "activities": [{ "type": 0, "name": "Apex" }, { "type": 4, "state": "m!help" }] }));
    assert_eq!(p.status, Status::Dnd);
    assert_eq!(p.custom.as_deref(), Some("m!help"));
    assert_eq!(p.activity.unwrap().text, "Apex");
    let p = parse_presence(&json!({ "status": "idle", "activities": [{ "type": 2, "name": "Spotify", "state": "Hozier" }, { "type": 0, "name": "Apex" }] }));
    assert_eq!(p.activity, Some(Activity { kind: ActivityKind::Listening, text: "Hozier".into(), rest: vec![(ActivityKind::Playing, "Apex".into())] }));
}

#[test]
fn presence_mark_follows_device_and_stream() {
    let mark = |v: Value| parse_presence(&v).mark();
    assert_eq!(mark(json!({ "status": "online", "client_status": { "mobile": "online" } })), Mark::Mobile);
    assert_eq!(mark(json!({ "status": "online", "client_status": { "mobile": "online", "desktop": "idle" } })), Mark::Status(Status::Online));
    assert_eq!(mark(json!({ "status": "idle", "client_status": { "mobile": "idle" } })), Mark::Status(Status::Idle));
    assert_eq!(mark(json!({ "status": "online" })), Mark::Status(Status::Online));
    let stream = json!([{ "type": 0, "name": "Apex" }, { "type": 1, "name": "Twitch", "url": "https://twitch.tv/x" }]);
    assert_eq!(mark(json!({ "status": "dnd", "client_status": { "mobile": "dnd" }, "activities": stream })), Mark::Streaming);
    assert_eq!(mark(json!({ "status": "offline", "activities": stream })), Mark::Status(Status::Offline));
}

#[test]
fn parses_a_live_profile_effect() {
    // The shape of a real `/collectibles-products/{sku}` answer for an effect (2026-11-24).
    let body = json!({ "sku_id": "1", "items": [{ "type": 1, "sku_id": "1", "animationType": 2, "effects": [
        { "src": "https://cdn/a", "loop": false, "height": 880, "width": 450, "duration": 7833, "start": 0, "loopDelay": 0, "position": { "x": 0, "y": 0 }, "zIndex": 100, "randomizedSources": [] },
        { "src": "https://cdn/b", "loop": true, "height": 880, "width": 450, "duration": 4250, "start": 9000, "loopDelay": 3000, "position": { "x": 0, "y": 0 }, "zIndex": 101, "randomizedSources": [] },
    ] }] });
    let layers = parse_profile_effect(&body);
    assert_eq!(layers.len(), 2, "{layers:?}");
    assert_eq!((layers[1].start, layers[1].duration, layers[1].looping, layers[1].loop_delay), (9000., 4250., true, 3000.));
}

#[test]
fn parses_profile_frame() {
    let body = json!({ "items": [{ "type": 3, "inner_width": 1200, "overflow_top": 120, "overflow_bottom": 80, "overflow_horizontal": 24, "layers": [
        { "id": "top", "type": "staple", "order": "front", "anchor": "top" },
        { "id": "side", "type": "rail", "order": "back", "anchor": "center" },
        { "id": "odd", "type": "sparkle" },
    ] }] });
    let frame = parse_profile_frame("sku1", &body).expect("a frame");
    assert_eq!((frame.inner_width, frame.overflow_top, frame.overflow_horizontal), (1200., 120., 24.));
    assert_eq!(frame.layers.len(), 2);
    assert_eq!((frame.layers[0].kind, frame.layers[0].front, frame.layers[0].anchor), (FrameKind::Staple, true, FrameAnchor::Top));
    assert_eq!(frame.layers[1].src, "https://cdn.discordapp.com/media/v1/collectibles-shop/sku1/side/static");
    assert!(parse_profile_frame("x", &json!({ "items": [] })).is_none());
    let worn = parse_profile("9", &json!({ "user_profile": { "collectibles": [{ "sku_id": "fr", "type": 3 }] } }));
    assert_eq!(worn.frame_sku.as_deref(), Some("fr"));
    // A server profile overrides the main one where it sets a field.
    let server = parse_profile("9", &json!({
        "user": { "id": "9", "banner": "main" },
        "user_profile": { "bio": "main bio", "pronouns": "she/her", "theme_colors": [1, 2] },
        "guild_member_profile": { "guild_id": "g", "banner": "a_srv", "bio": "server bio", "pronouns": "", "theme_colors": [3, 4] },
        "badges": [{ "description": "Nitro", "icon": "n" }],
        "guild_badges": [{ "description": "Booster", "icon": "b" }],
    }));
    assert_eq!(server.bio.as_deref(), Some("server bio"));
    assert_eq!(server.pronouns.as_deref(), Some("she/her"));
    assert_eq!(server.theme, Some((3, 4)));
    assert_eq!(server.banner.as_deref(), Some("https://cdn.discordapp.com/guilds/g/users/9/banners/a_srv.gif?size=600"));
    assert_eq!(server.badges.len(), 2);
}

#[test]
fn parses_outbox() {
    let body = json!({ "entries": [
        { "id": "1", "content_type": 1, "extra": { "game_name": "Celeste", "application_id": "9" }, "traits": [{ "type": 1, "first_time": true }, { "type": 2, "duration_seconds": 9000 }, { "type": 8, "streak_count_days": 4 }] },
        { "id": "2", "content_type": 2, "extra": { "media_title": "Frieren", "media_subtitle": "S1E3" }, "traits": [] },
        { "id": "3", "content_type": 5, "extra": { "entries": [{ "media": { "title": "Aqua Regia", "artists": [{ "name": "Sleep Token" }] } }] } },
        { "id": "4", "content_type": 7, "extra": {} },
        { "content_type": 1, "extra": { "game_name": "no id" } },
    ] });
    let rows = parse_outbox(&body);
    assert_eq!(rows.len(), 3);
    assert_eq!((rows[0].kind, rows[0].title.as_str(), rows[0].first_time, rows[0].marathon_hours, rows[0].streak_days), (RecentKind::Played, "Celeste", true, Some(3), Some(4)));
    assert_eq!((rows[1].kind, rows[1].subtitle.as_deref()), (RecentKind::Watched, Some("S1E3")));
    assert_eq!((rows[2].kind, rows[2].title.as_str(), rows[2].subtitle.as_deref()), (RecentKind::Listened, "Aqua Regia", Some("Sleep Token")));
}

#[test]
fn parses_profile() {
    let body = json!({
        "user": { "id": "9", "username": "aayurene", "global_name": "Aayurene", "banner": "a_b", "avatar_decoration_data": { "asset": "deco" } },
        "user_profile": { "bio": "LUCKY VICKY <a:aWonUwu:1328465377218269257>", "pronouns": "WON/YOUNG", "theme_colors": [4980812, 11558570] },
        "badges": [{ "id": "premium", "description": "Subscriber since", "icon": "2ba85e8026a8614b640c2837bcdfe21b", "link": "https://discord.com/settings/premium" }, { "id": "broken" }],
        "mutual_friends": [{ "id": "1", "username": "a" }, { "id": "2", "username": "b" }],
        "mutual_friend_count": 5,
        "mutual_guilds": [{ "id": "g", "nick": null }, { "id": "h", "nick": "wrenny" }, { "nick": "no id" }],
        "guild_member": { "roles": ["r"], "joined_at": "2022-06-18T10:00:00+00:00" },
        "connected_accounts": [{ "type": "spotify", "id": "s1", "name": "wren", "verified": true }, { "type": "steam", "id": "7656" }, { "name": "no type" }],
        "guild_member_profile": { "collectibles": [{ "sku_id": "old", "type": 1, "expires_at": "2001-01-01T00:00:00+00:00" }] },
    });
    let p = parse_profile("9", &body);
    assert_eq!(p.username, "aayurene");
    assert_eq!(p.pronouns.as_deref(), Some("WON/YOUNG"));
    assert_eq!(p.banner.as_deref(), Some("https://cdn.discordapp.com/banners/9/a_b.gif?size=600"));
    assert_eq!(p.theme, Some((0x4C004C, 0xB05EAA)));
    assert_eq!(p.badges.len(), 1);
    assert_eq!(p.badges[0].icon, "https://cdn.discordapp.com/badge-icons/2ba85e8026a8614b640c2837bcdfe21b.png");
    assert_eq!(p.badges[0].link.as_deref(), Some("https://discord.com/settings/premium"));
    assert_eq!((p.mutual_friends.len(), p.mutual_friend_count, p.mutual_server_count), (2, 5, 2));
    assert_eq!(p.mutual_guilds, [MutualGuild { id: "g".into(), nick: None }, MutualGuild { id: "h".into(), nick: Some("wrenny".into()) }]);
    assert!(p.decoration.unwrap().contains("/avatar-decoration-presets/deco.png"));
    assert_eq!(p.roles, ["r"]);
    assert_eq!(p.joined.as_deref(), Some("2022-06-18T10:00:00+00:00"));
    assert_eq!(p.connections.iter().map(|c| (c.kind.as_str(), c.name.as_str(), c.verified)).collect::<Vec<_>>(), [("spotify", "wren", true), ("steam", "", false)]);
    assert_eq!(p.effect_sku, None);
    let worn = parse_profile("9", &json!({ "user_profile": { "collectibles": [{ "sku_id": "frame", "type": 2 }, { "sku_id": "fx", "type": 1, "expires_at": null }] } }));
    assert_eq!(worn.effect_sku.as_deref(), Some("fx"));
    let bare = parse_profile("9", &json!({ "user": { "id": "9" }, "user_profile": { "theme_colors": [1] } }));
    assert_eq!((bare.theme, bare.mutual_friend_count, bare.badges.len()), (None, 0, 0));
}

#[test]
fn relationships_carry_since() {
    let since = "2023-08-09T12:00:00+00:00";
    let Some(Event::RelationshipChange { relationship: Some(r), .. }) = dispatch("RELATIONSHIP_ADD", &json!({ "id": "5", "type": 1, "since": since })) else { panic!("no relationship") };
    assert_eq!((r.kind, r.since.as_deref()), (Relation::Friend, Some(since)));
    let ready = parse_ready(&json!({ "user": { "id": "1" }, "relationships": [{ "id": "5", "type": 1, "since": since, "user": { "id": "5" } }] }));
    assert_eq!(ready.relationships[0].since.as_deref(), Some(since));
}

#[test]
fn parses_profile_effect() {
    let body = json!({ "items": [{ "type": 1, "effects": [
        { "src": "https://x/top.png", "width": 450, "height": 880, "position": { "x": 0, "y": 0 }, "zIndex": 2 },
        { "src": "https://x/base.png", "randomizedSources": [{ "src": "https://x/a.png" }], "width": 450, "height": 200, "position": { "x": 0, "y": 40 }, "zIndex": 1 },
        { "src": "https://x/empty.png" },
    ]}]});
    let layers = parse_profile_effect(&body);
    assert_eq!(layers.iter().map(|l| l.src.as_str()).collect::<Vec<_>>(), ["https://x/a.png", "https://x/top.png"]);
    assert_eq!((layers[0].y, layers[0].height), (40., 200.));
    assert!(parse_profile_effect(&json!({ "items": [] })).is_empty());
}

#[test]
fn parses_rich_presence() {
    let p = parse_presence(&json!({ "status": "dnd", "activities": [
        { "type": 4, "state": "busy" },
        { "type": 0, "name": "PUBG: BATTLEGROUNDS", "application_id": "530196305138417685", "details": "Normal, Erangel, 58/99", "state": "4 Squad",
          "party": { "id": "p", "size": [4, 4] }, "timestamps": { "start": 1700000000000i64 }, "flags": 2, "assets": { "large_image": "123" } },
    ]}));
    let rich = p.rich.unwrap();
    assert_eq!(rich.name, "PUBG: BATTLEGROUNDS");
    assert_eq!(rich.party, Some((4, 4)));
    assert_eq!(rich.started, Some(1700000000000));
    assert!(rich.joinable);
    assert_eq!(rich.image.as_deref(), Some("https://cdn.discordapp.com/app-assets/530196305138417685/123.png"));
    let proxied = parse_rich(&json!({ "name": "x", "assets": { "large_image": "mp:external/abc/https/x.png" } })).unwrap();
    assert_eq!(proxied.image.as_deref(), Some("https://media.discordapp.net/external/abc/https/x.png"));
    assert!(!proxied.joinable);
}

#[test]
fn parses_member_list_ops() {
    let op = parse_list_op(&json!({ "op": "SYNC", "range": [0, 99], "items": [
        { "group": { "id": "online", "count": 2 } },
        { "member": { "user": { "id": "1", "username": "a" }, "nick": "Nick", "avatar": "h", "roles": ["r"], "presence": { "status": "online" } } },
    ]}), Some("g"))
    .unwrap();
    let ListOp::Sync { items, .. } = op else { panic!("sync") };
    assert!(matches!(&items[1], ListItem::Member(m) if m.roles == ["r"] && m.user.name == "Nick" && m.user.avatar.contains("/guilds/g/users/1/avatars/h.png")));
}

#[test]
fn invites_read_their_server_and_counts() {
    let body = json!({ "code": "waffle", "guild": { "id": "9", "name": "Dumpling Hall", "icon": "ff" }, "approximate_member_count": 8311, "approximate_presence_count": 1204 });
    let invite = parse_invite(&body).unwrap();
    assert_eq!(invite, Invite { guild_id: "9".into(), name: "Dumpling Hall".into(), icon: Some("https://cdn.discordapp.com/icons/9/ff.png?size=96".into()), online: Some(1204), members: Some(8311), ..Default::default() });
    assert_eq!(parse_invite(&json!({ "code": "gdm", "channel": { "id": "5", "type": 3 } })), None, "a group DM invite has no server card");
}

#[test]
fn voice_effect_reads_the_sound_and_its_emoji() {
    let d = json!({
        "channel_id": "10", "guild_id": "1", "user_id": "7", "sound_id": "33", "sound_volume": 0.5,
        "emoji": { "id": null, "name": "🦆" }, "animation_type": 1, "animation_id": 4, "name": "quack", "source_guild_id": "2"
    });
    let Some(Event::VoiceEffect(effect)) = dispatch("VOICE_CHANNEL_EFFECT_SEND", &d) else { panic!("not an effect") };
    assert_eq!(effect, VoiceEffect {
        channel_id: "10".into(),
        user_id: "7".into(),
        sound_id: Some("33".into()),
        sound_volume: 0.5,
        emoji: Some(Emoji::unicode("🦆")),
        animation_type: Some(1),
        animation_id: Some(4),
        name: Some("quack".into()),
        source_guild_id: Some("2".into()),
    });
    let bare = parse_voice_effect(&json!({ "channel_id": "10", "user_id": "7", "emoji": { "id": "55", "name": "pog" } })).unwrap();
    assert_eq!((bare.sound_id, bare.sound_volume, bare.emoji), (None, 1., Some(Emoji { id: Some("55".into()), name: "pog".into(), animated: false })), "an emoji alone, at full volume");
}

#[test]
fn soundboard_sounds_reply_and_updates() {
    let d = json!({ "guild_id": "1", "soundboard_sounds": [
        { "sound_id": "33", "name": "quack", "volume": 0.8, "emoji_id": null, "emoji_name": "🦆", "guild_id": "1", "available": true },
        { "name": "no id" }
    ] });
    let Some(Event::SoundboardSounds { guild_id, sounds, all }) = dispatch("SOUNDBOARD_SOUNDS", &d) else { panic!("not sounds") };
    assert_eq!((guild_id.as_str(), all), ("1", true));
    assert_eq!(sounds, vec![SoundboardSound { id: "33".into(), name: "quack".into(), guild_id: "1".into(), volume: 0.8, emoji: Some(Emoji::unicode("🦆")), available: true }]);
    let Some(Event::SoundboardSounds { all: false, sounds, .. }) = dispatch("GUILD_SOUNDBOARD_SOUND_UPDATE", &json!({ "sound_id": "33", "name": "honk", "guild_id": "1" })) else { panic!("not an update") };
    assert_eq!(sounds[0].name, "honk");
    assert!(matches!(dispatch("GUILD_SOUNDBOARD_SOUND_DELETE", &json!({ "sound_id": "33", "guild_id": "1" })), Some(Event::SoundboardSoundDelete { sound_id, .. }) if sound_id == "33"));
}

#[test]
fn default_sounds_are_server_zero_and_play_without_a_source() {
    let sound = SoundboardSound::parse(&json!({ "sound_id": 1, "name": "quack", "volume": 1.0, "emoji_name": "🦆" })).unwrap();
    assert_eq!((sound.id.as_str(), sound.is_default()), ("1", true), "a numeric id reads as a string");
    assert_eq!(soundboard_play_body(&sound), json!({ "sound_id": "1", "emoji_id": null, "emoji_name": "🦆" }));
    let custom = SoundboardSound { guild_id: "9".into(), emoji: Some(Emoji { id: Some("55".into()), name: String::new(), animated: false }), ..sound };
    assert_eq!(soundboard_play_body(&custom), json!({ "sound_id": "1", "emoji_id": "55", "emoji_name": null, "source_guild_id": "9" }));
    assert_eq!(custom.url(), "https://cdn.discordapp.com/soundboard-sounds/1");
}

#[test]
fn a_manual_ack_carries_its_mention_count() {
    let manual = dispatch("MESSAGE_ACK", &json!({ "channel_id": "7", "message_id": "42", "manual": true, "mention_count": 3, "version": 9 }));
    assert!(matches!(manual, Some(Event::Ack { message_id: 42, manual: Some(3), .. })));
    let read = dispatch("MESSAGE_ACK", &json!({ "channel_id": "7", "message_id": "42", "version": 9 }));
    assert!(matches!(read, Some(Event::Ack { manual: None, .. })));
}
#[cfg(test)]
mod saved_tests {
    #[test]
    fn bookmark_ids_read_ids_or_saves() {
        let list = serde_json::json!(["1", { "channel_id": "9", "message_id": "2" }, 3]);
        assert_eq!(super::bookmark_ids(&list), ["1", "2"]);
    }
}

