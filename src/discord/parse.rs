use super::*;

pub(super) fn parse_ready(d: &Value) -> Ready {
    let entries = d["read_state"]["entries"].as_array().or(d["read_state"].as_array());
    Ready {
        me: parse_user(&d["user"]),
        status: Status::parse(d["user_settings"]["status"].as_str().unwrap_or("online")),
        custom_status: CustomStatus::parse(&d["user_settings"]["custom_status"], chrono::Utc::now()),
        session_id: d["session_id"].as_str().unwrap_or_default().to_owned(),
        guilds: d["guilds"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .filter_map(|(i, g)| {
                let mut guild = parse_guild(g)?;
                let member = my_member(g, &d["merged_members"][i], d["user"]["id"].as_str().unwrap_or_default());
                guild.my_roles = strings(&member["roles"]);
                guild.timed_out_until = timeout_end(&member);
                guild.me = Membership::parse(&member);
                Some(guild)
            })
            .collect(),
        dms: d["private_channels"].as_array().into_iter().flatten().filter_map(|c| parse_dm(c, &d["users"])).collect(),
        read_states: entries
            .into_iter()
            .flatten()
            .filter_map(|r| {
                let id = r["id"].as_str()?.to_owned();
                Some((id, ReadState { last_read: snowflake(&r["last_message_id"]), mentions: r["mention_count"].as_u64().unwrap_or(0) as u32 }))
            })
            .collect(),
        developer_mode: d["user_settings"]["developer_mode"].as_bool().unwrap_or(false),
        my_account: MyAccount::parse(&d["user"]),
        account_settings: {
            let mut settings = AccountSettings::default();
            settings.apply(&d["user_settings"]);
            // Not verified: READY carries the settings proto as base64 under `user_settings_proto`.
            settings.apply_proto(d["user_settings_proto"].as_str().unwrap_or_default());
            settings
        },
        guild_settings: d["user_guild_settings"]["entries"].as_array().or(d["user_guild_settings"].as_array()).into_iter().flatten().map(parse_guild_settings).collect(),
        ignored: d["relationships"].as_array().into_iter().flatten().filter(|r| r["user_ignored"].as_bool() == Some(true)).filter_map(|r| r["id"].as_str().map(str::to_owned)).collect(),
        relationships: d["relationships"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| {
                let kind = match r["type"].as_u64()? {
                    1 => Relation::Friend,
                    2 => Relation::Blocked,
                    3 => Relation::Incoming,
                    4 => Relation::Outgoing,
                    _ => return None,
                };
                // Legacy READY embeds the user; newer shapes reference READY.users by id.
                let user = if r["user"].is_object() { parse_user(&r["user"]) } else { d["users"].as_array()?.iter().find(|u| u["id"] == r["id"]).map(parse_user)? };
                Some(Relationship { user, kind, since: r["since"].as_str().map(str::to_owned) })
            })
            .collect(),
        folders: d["user_settings"]["guild_folders"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| Folder {
                id: f["id"].as_u64().map(|n| n.to_string()).or(f["id"].as_str().map(str::to_owned)).unwrap_or_default(),
                name: f["name"].as_str().filter(|n| !n.is_empty()).map(str::to_owned),
                color: f["color"].as_u64().map(|c| c as u32),
                guild_ids: strings(&f["guild_ids"]),
            })
            .collect(),
        presences: d["presences"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| Some((p["user"]["id"].as_str().or(p["user_id"].as_str())?.to_owned(), parse_presence(p))))
            .collect(),
    }
}

pub(super) fn parse_guild(g: &Value) -> Option<Guild> {
    let id = g["id"].as_str()?;
    let props = if g["name"].is_string() { g } else { &g["properties"] };
    let mut channels: Vec<Channel> = g["channels"].as_array().into_iter().flatten().filter_map(|c| parse_channel(c, id)).collect();
    channels.sort_by_key(|c| c.position);
    Some(Guild {
        id: id.to_owned(),
        name: props["name"].as_str()?.to_owned(),
        icon: props["icon"].as_str().map(|hash| format!("{CDN}/icons/{id}/{hash}.png?size=96")),
        channels,
        roles: g["roles"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| {
                Some(Role {
                    id: r["id"].as_str()?.to_owned(),
                    name: r["name"].as_str().unwrap_or_default().to_owned(),
                    color: r["color"].as_u64().unwrap_or(0) as u32,
                    position: r["position"].as_i64().unwrap_or(0),
                    permissions: permission(&r["permissions"]),
                    managed: r["managed"].as_bool().unwrap_or(false),
                    icon: role_icon(r),
                })
            })
            .collect(),
        // Guild payloads leave out each voice state's guild_id.
        voice_states: g["voice_states"].as_array().into_iter().flatten().map(|v| VoiceState { guild_id: Some(id.to_owned()), ..parse_voice_state(v) }).collect(),
        stage_instances: g["stage_instances"].as_array().into_iter().flatten().filter_map(parse_stage_instance).collect(),
        emojis: g["emojis"].as_array().into_iter().flatten().map(parse_emoji).collect(),
        // Not verified: READY's user-account guild objects carrying `stickers` the way bot GUILD_CREATE does.
        stickers: g["stickers"].as_array().into_iter().flatten().filter_map(Sticker::parse).collect(),
        owner_id: props["owner_id"].as_str().or(g["owner_id"].as_str()).map(str::to_owned),
        default_notifications: props["default_message_notifications"].as_u64().or(g["default_message_notifications"].as_u64()).unwrap_or(0) as u8,
        threads: g["threads"].as_array().into_iter().flatten().filter_map(|t| parse_channel(t, id)).collect(),
        my_roles: Vec::new(),
        premium_tier: props["premium_tier"].as_u64().or(g["premium_tier"].as_u64()).unwrap_or(0) as u8,
        max_video_users: props["max_video_channel_users"].as_u64().or(g["max_video_channel_users"].as_u64()).unwrap_or(0) as u32,
        max_stage_video_users: props["max_stage_video_channel_users"].as_u64().or(g["max_stage_video_channel_users"].as_u64()).unwrap_or(0) as u32,
        afk_channel_id: props["afk_channel_id"].as_str().or(g["afk_channel_id"].as_str()).map(str::to_owned),
        timed_out_until: None,
        system_channel_flags: props["system_channel_flags"].as_u64().or(g["system_channel_flags"].as_u64()).unwrap_or(0),
        features: strings(if props["features"].is_array() { &props["features"] } else { &g["features"] }),
        splash: props["splash"].as_str().or(g["splash"].as_str()).map(|hash| splash_url(id, hash)),
        rules_channel_id: props["rules_channel_id"].as_str().or(g["rules_channel_id"].as_str()).map(str::to_owned),
        me: Membership::default(),
        events: g["guild_scheduled_events"].as_array().into_iter().flatten().filter_map(parse_scheduled_event).collect(),
        public_updates_channel_id: props["public_updates_channel_id"].as_str().or(g["public_updates_channel_id"].as_str()).map(str::to_owned),
        vanity_url_code: props["vanity_url_code"].as_str().or(g["vanity_url_code"].as_str()).map(str::to_owned),
    })
}

// READY lists our own member entry per server, either inside each guild or in `merged_members`.
pub(super) fn my_member(guild: &Value, merged: &Value, me: &str) -> Value {
    let mine = |list: &Value| list.as_array().into_iter().flatten().find(|m| m["user_id"].as_str().or(m["user"]["id"].as_str()) == Some(me)).cloned();
    mine(&guild["members"]).or_else(|| mine(merged)).unwrap_or(Value::Null)
}

pub(super) fn timeout_end(member: &Value) -> Option<chrono::DateTime<chrono::Utc>> {
    let end = member["communication_disabled_until"].as_str()?;
    Some(chrono::DateTime::parse_from_rfc3339(end).ok()?.with_timezone(&chrono::Utc))
}

pub fn parse_thread(c: &Value, guild_id: &str) -> Option<Channel> {
    parse_channel(c, guild_id)
}

pub(crate) fn parse_channel(c: &Value, guild_id: &str) -> Option<Channel> {
    let kind = match c["type"].as_u64()? {
        0 | 5 => ChannelKind::Text,
        2 | 13 => ChannelKind::Voice,
        4 => ChannelKind::Category,
        10..=12 => ChannelKind::Thread,
        15 | 16 => ChannelKind::Forum,
        _ => ChannelKind::Other,
    };
    let meta = &c["thread_metadata"];
    let thread = (kind == ChannelKind::Thread).then(|| ThreadMeta {
        archived: meta["archived"].as_bool().unwrap_or(false),
        locked: meta["locked"].as_bool().unwrap_or(false),
        joined: c["member"].is_object(),
        message_count: c["message_count"].as_u64().unwrap_or(0) as u32,
        owner_id: c["owner_id"].as_str().map(str::to_owned),
        muted: c["member"]["muted"].as_bool().unwrap_or(false),
        flags: c["member"]["flags"].as_u64().unwrap_or(0),
        applied_tags: strings(&c["applied_tags"]),
        pinned: c["flags"].as_u64().unwrap_or(0) & 2 != 0,
    });
    let overwrites: Vec<Overwrite> = c["permission_overwrites"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|o| Some(Overwrite { id: o["id"].as_str()?.to_owned(), allow: permission(&o["allow"]), deny: permission(&o["deny"]), member: o["type"].as_u64() == Some(1) }))
        .collect();
    let private = overwrites.iter().any(|o| o.id == guild_id && o.deny & VIEW_CHANNEL != 0);
    Some(Channel {
        id: c["id"].as_str()?.to_owned(),
        name: c["name"].as_str().unwrap_or_default().to_owned(),
        kind,
        parent_id: c["parent_id"].as_str().map(str::to_owned),
        position: c["position"].as_i64().unwrap_or(0),
        topic: c["topic"].as_str().filter(|t| !t.is_empty()).map(str::to_owned),
        last_message_id: c["last_message_id"].as_str().map(str::to_owned),
        private,
        recipient: None,
        recipients: Vec::new(),
        icon: None,
        thread,
        raw_type: c["type"].as_u64().unwrap_or(0) as u8,
        overwrites,
        props: Box::new(ChannelProps {
            nsfw: c["nsfw"].as_bool().unwrap_or(false),
            slowmode: c["rate_limit_per_user"].as_u64().unwrap_or(0) as u32,
            bitrate: c["bitrate"].as_u64().unwrap_or(0) as u32,
            user_limit: c["user_limit"].as_u64().unwrap_or(0) as u32,
            rtc_region: c["rtc_region"].as_str().map(str::to_owned),
            video_full: c["video_quality_mode"].as_u64() == Some(2),
            archive_after: c["default_auto_archive_duration"].as_u64().unwrap_or(0) as u32,
            tags: c["available_tags"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|t| {
                    Some(ForumTag {
                        id: t["id"].as_str()?.to_owned(),
                        name: t["name"].as_str().unwrap_or_default().to_owned(),
                        emoji: parse_forum_emoji(&t["emoji_id"], &t["emoji_name"]),
                        moderated: t["moderated"].as_bool().unwrap_or(false),
                    })
                })
                .collect(),
            require_tag: c["flags"].as_u64().unwrap_or(0) & 16 != 0,
            sort_by_creation: c["default_sort_order"].as_u64() == Some(1),
            gallery: c["type"].as_u64() == Some(16) || c["default_forum_layout"].as_u64() == Some(2),
            match_all: c["default_tag_setting"].as_str() == Some("match_all"),
            default_reaction: parse_forum_emoji(&c["default_reaction_emoji"]["emoji_id"], &c["default_reaction_emoji"]["emoji_name"]),
            template: c["template"].as_str().unwrap_or_default().to_owned(),
            ..Default::default()
        }),
    })
}

pub(super) fn parse_dm(c: &Value, users: &Value) -> Option<Channel> {
    let recipients: Vec<User> = match c["recipients"].as_array() {
        Some(list) => list.iter().map(parse_user).collect(),
        None => c["recipient_ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|rid| users.as_array()?.iter().find(|u| u["id"] == *rid).map(parse_user))
            .collect(),
    };
    let names: Vec<&str> = recipients.iter().map(|u| u.name.as_str()).collect();
    let name = c["name"].as_str().filter(|n| !n.is_empty()).map(str::to_owned).unwrap_or_else(|| names.join(", "));
    let id = c["id"].as_str()?.to_owned();
    let icon = c["icon"].as_str().map(|hash| format!("{CDN}/channel-icons/{id}/{hash}.png?size=128"));
    Some(Channel {
        id,
        name,
        kind: ChannelKind::Dm,
        parent_id: None,
        position: 0,
        topic: None,
        last_message_id: c["last_message_id"].as_str().map(str::to_owned),
        private: false,
        recipient: (recipients.len() == 1 && c["type"].as_u64() != Some(3)).then(|| recipients[0].clone()),
        recipients,
        icon,
        thread: None,
        raw_type: c["type"].as_u64().unwrap_or(1) as u8,
        overwrites: Vec::new(),
        props: Box::new(ChannelProps {
            owner_id: c["owner_id"].as_str().map(str::to_owned),
            message_request: c["is_message_request"].as_bool().unwrap_or(false),
            spam: c["is_spam"].as_bool().unwrap_or(false),
            ..Default::default()
        }),
    })
}

// `/messages/pins` lists `{ items: [{ pinned_at, message }] }`; the deprecated `/pins` a bare message array.
pub(super) fn parse_pins(body: &Value) -> Vec<Message> {
    match body["items"].as_array() {
        Some(items) => items.iter().filter_map(|pin| parse_message(&pin["message"])).collect(),
        None => body.as_array().into_iter().flatten().filter_map(parse_message).collect(),
    }
}

// An attachment's `flags` bit for a spoiler (Discord's IS_SPOILER).
pub(super) const ATTACHMENT_SPOILER: u64 = 8;

pub fn parse_message(m: &Value) -> Option<Message> {
    use base64::Engine;
    let mut media = Vec::new();
    let mut files = Vec::new();
    let mut media_files = Vec::new();
    for a in m["attachments"].as_array().into_iter().flatten() {
        let url = a["url"].as_str().unwrap_or_default().to_owned();
        let kind = a["content_type"].as_str().unwrap_or_default();
        let id = a["id"].as_str().unwrap_or_default().to_owned();
        let description = a["description"].as_str().filter(|d| !d.is_empty()).map(str::to_owned);
        let spoiler = a["flags"].as_u64().unwrap_or(0) & ATTACHMENT_SPOILER != 0;
        let file = File {
            name: a["filename"].as_str().unwrap_or("file").to_owned(),
            url: url.clone(),
            size: a["size"].as_u64().unwrap_or(0),
            id: id.clone(),
            spoiler,
            duration_secs: a["duration_secs"].as_f64().map(|d| d as f32),
            waveform: a["waveform"].as_str().and_then(|w| base64::engine::general_purpose::STANDARD.decode(w).ok()).unwrap_or_default(),
        };
        let (Some(w), Some(h)) = (a["width"].as_f64(), a["height"].as_f64()) else {
            files.push(file);
            continue;
        };
        media_files.push(file);
        let kind = match kind.starts_with("video/") {
            // Discord's media proxy returns a still frame for a video when asked for an image format.
            true => {
                let proxy = a["proxy_url"].as_str().unwrap_or(&url);
                MediaKind::Video { poster: format!("{proxy}{}format=jpeg", if proxy.contains('?') { "&" } else { "?" }) }
            }
            false => MediaKind::Image,
        };
        media.push(Media { url, width: w as f32, height: h as f32, kind, id: Some(id), description, spoiler });
    }
    let flags = m["flags"].as_u64().unwrap_or(0);
    let attachment_media = media.len();
    let embeds = if flags & message_flag::SUPPRESS_EMBEDS == 0 { parse_embeds(&m["embeds"], &mut media) } else { Vec::new() };
    let embed_media = media.len() - attachment_media;
    // Reference type 1 is a forward; only 0 (or none, on older messages) is a reply.
    // A crosspost's reference points at its source in the followed channel, not at a message it answers.
    let is_crosspost = m["flags"].as_u64().unwrap_or(0) & Message::IS_CROSSPOST != 0;
    let is_reply = !is_crosspost && matches!(m["message_reference"]["type"].as_u64(), None | Some(0));
    let reply = (is_reply && m["referenced_message"].is_object()).then(|| parse_message(&m["referenced_message"])).flatten().map(Box::new);
    // Discord sends the key with null when the original was deleted, and leaves it out when it wasn't loaded.
    let reply_deleted = is_reply && m["message_reference"].is_object() && m.get("referenced_message").is_some_and(serde_json::Value::is_null);
    Some(Message {
        id: m["id"].as_str()?.to_owned(),
        channel_id: m["channel_id"].as_str()?.to_owned(),
        guild_id: m["guild_id"].as_str().map(str::to_owned),
        author: parse_member_user(&m["member"], &m["author"], m["guild_id"].as_str()),
        roles: m["member"]["roles"].is_array().then(|| strings(&m["member"]["roles"])),
        content: m["content"].as_str().unwrap_or_default().to_owned(),
        timestamp: m["timestamp"].as_str().unwrap_or_default().to_owned(),
        edited: m["edited_timestamp"].is_string(),
        media,
        embeds,
        files,
        media_files,
        reactions: m["reactions"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|r| Reaction { emoji: parse_emoji(&r["emoji"]), count: r["count"].as_u64().unwrap_or(1) as u32, me: r["me"].as_bool().unwrap_or(false) })
            .collect(),
        reply_deleted,
        reply_to: m["message_reference"]["message_id"].as_str().filter(|_| is_reply).map(str::to_owned),
        reply,
        mentions: m["mentions"].as_array().into_iter().flatten().map(parse_user).collect(),
        mention_roles: strings(&m["mention_roles"]),
        mention_everyone: m["mention_everyone"].as_bool().unwrap_or(false),
        tts: m["tts"].as_bool().unwrap_or(false),
        nonce: m["nonce"].as_str().map(str::to_owned),
        state: SendState::Sent,
        pinned: m["pinned"].as_bool().unwrap_or(false),
        thread: m["thread"].is_object().then(|| parse_channel(&m["thread"], m["guild_id"].as_str().unwrap_or_default())).flatten(),
        call: (m["type"] == 3).then(|| CallRecord { participants: strings(&m["call"]["participants"]), ended: m["call"]["ended_timestamp"].as_str().map(str::to_owned) }),
        stickers: m["sticker_items"].as_array().or(m["stickers"].as_array()).into_iter().flatten().filter_map(Sticker::parse).collect(),
        poll: m["poll"].is_object().then(|| parse_poll(&m["poll"])),
        forward: parse_forward(m),
        pin_notice: m["type"] == 6,
        activity: parse_message_activity(m),
        kind: m["type"].as_u64().unwrap_or(0) as u8,
        flags,
        reference_channel: m["message_reference"]["channel_id"].as_str().map(str::to_owned),
        reference_deleted: m["message_reference"].is_object() && m.get("referenced_message").is_some_and(Value::is_null),
        // Not verified: the wire key. Discord's web client reads `message.actor`.
        actor: m["actor"].is_object().then(|| parse_user(&m["actor"])),
        embed_media,
        interaction: crate::app_commands::parse_interaction(m),
        components: crate::app_commands::parse_components(&m["components"]),
        application_id: m["application_id"].as_str().map(str::to_owned),
        interaction_error: None,
        crosspost_from: is_crosspost.then(|| crosspost_source(&m["message_reference"])).flatten(),
    })
}

pub(super) fn crosspost_source(reference: &Value) -> Option<CrosspostSource> {
    Some(CrosspostSource { guild_id: reference["guild_id"].as_str()?.to_owned(), channel_id: reference["channel_id"].as_str()?.to_owned() })
}

// Image and GIF embeds join `media` (what the gallery pages through); the rest come back as cards.
pub(super) fn parse_embeds(list: &Value, media: &mut Vec<Media>) -> Vec<Embed> {
    let mut embeds = Vec::new();
    for e in list.as_array().into_iter().flatten() {
        match e["type"].as_str().unwrap_or("rich") {
            "image" => media.extend(embed_media(&e["thumbnail"]).or_else(|| embed_media(&e["image"]))),
            // GIF pickers (Tenor, Giphy) send a looping mp4 with a still thumbnail.
            "gifv" => {
                let poster = embed_media(&e["thumbnail"]);
                let video = e["video"]["url"].as_str();
                if let (Some(poster), Some(video)) = (poster, video) {
                    media.push(Media { url: video.to_owned(), width: poster.width, height: poster.height, kind: MediaKind::Gif { poster: poster.url }, ..Default::default() });
                }
            }
            _ => {
                let embed = parse_embed(e);
                // Discord folds embeds that share a url into the first one's gallery.
                let first = embed.url.as_ref().and_then(|url| embeds.iter_mut().find(|f: &&mut Embed| f.url.as_ref() == Some(url)));
                match first {
                    Some(first) => first.images.extend(embed.images.into_iter().take(4usize.saturating_sub(first.images.len()))),
                    None => embeds.push(embed),
                }
            }
        }
    }
    embeds
}

// A forward's snapshot carries the original's content, media and time but no id, author or channel;
// those come from the reference.
pub(super) fn parse_forward(m: &Value) -> Option<Box<Message>> {
    let reference = &m["message_reference"];
    if reference["type"].as_u64() != Some(1) {
        return None;
    }
    let mut original = m["message_snapshots"][0]["message"].clone();
    if !original.is_object() {
        return None;
    }
    original["id"] = reference["message_id"].clone();
    original["channel_id"] = reference["channel_id"].clone();
    original["guild_id"] = reference["guild_id"].clone();
    parse_message(&original).map(Box::new)
}

/// Merges a partial MESSAGE_UPDATE (edits, late embeds) into a cached message.
pub fn apply_update(message: &mut Message, d: &Value) {
    if let Some(full) = d["author"].is_object().then(|| parse_message(d)).flatten() {
        let reactions = std::mem::take(&mut message.reactions);
        *message = Message { reactions, ..full };
        return;
    }
    if let Some(content) = d["content"].as_str() {
        message.content = content.to_owned();
        message.edited = true;
    }
    if let Some(flags) = d["flags"].as_u64() {
        message.flags = flags;
    }
    if d["embeds"].is_array() || message.embeds_suppressed() {
        message.media.truncate(message.media.len().saturating_sub(message.embed_media));
        let before = message.media.len();
        message.embeds = if message.embeds_suppressed() { Vec::new() } else { parse_embeds(&d["embeds"], &mut message.media) };
        message.embed_media = message.media.len() - before;
    }
}

// A member as that server shows them: their server nickname and server avatar win over the account's.
pub fn parse_member_user(member: &Value, user: &Value, guild_id: Option<&str>) -> User {
    let mut parsed = parse_user(user);
    if let Some(nick) = member["nick"].as_str().filter(|n| !n.is_empty()) {
        parsed.name = nick.to_owned();
    }
    if let (Some(hash), Some(guild_id)) = (member["avatar"].as_str().filter(|h| !h.is_empty()), guild_id) {
        parsed.avatar = format!("{CDN}/guilds/{guild_id}/users/{}/avatars/{hash}.png?size=128", parsed.id);
    }
    if let Some(style) = parse_name_style(&member["display_name_styles"]) {
        parsed.name_style = Some(style);
    }
    parsed.boosting_since = member["premium_since"].as_str().map(str::to_owned);
    parsed
}

pub(super) fn role_icon(role: &Value) -> Option<String> {
    let (id, hash) = (role["id"].as_str()?, role["icon"].as_str()?);
    Some(format!("{CDN}/role-icons/{id}/{hash}.png?size=32"))
}

pub fn parse_user(user: &Value) -> User {
    let id = user["id"].as_str().unwrap_or_default().to_owned();
    let name = user["global_name"].as_str().or(user["username"].as_str()).unwrap_or("Unknown").to_owned();
    let avatar = avatar_url(&id, user["avatar"].as_str());
    let guild = if user["primary_guild"].is_object() { &user["primary_guild"] } else { &user["clan"] };
    let tag = guild["tag"].as_str().filter(|_| guild["identity_enabled"].as_bool() != Some(false)).map(|text| GuildTag {
        text: text.to_owned(),
        badge: match (guild["identity_guild_id"].as_str(), guild["badge"].as_str()) {
            (Some(gid), Some(badge)) => Some(format!("{CDN}/guild-tag-badges/{gid}/{badge}.png?size=32")),
            _ => None,
        },
        guild_id: guild["identity_guild_id"].as_str().map(str::to_owned),
    });
    let plate = &user["collectibles"]["nameplate"];
    let nameplate = plate["asset"].as_str().map(|asset| Nameplate {
        art: format!("{CDN}/assets/collectibles/{asset}static.png"),
        animated: plate["sku_id"].as_str().map(|sku| format!("{CDN}/media/v1/collectibles-shop/{sku}/animated")),
        tint: palette_tint(plate["palette"].as_str().unwrap_or_default()),
    }).map(Box::new);
    let decoration = user["avatar_decoration_data"]["asset"].as_str().map(|asset| format!("{CDN}/avatar-decoration-presets/{asset}.png?size=96&passthrough=false"));
    let name_style = parse_name_style(&user["display_name_styles"]);
    let username = user["username"].as_str().unwrap_or_default().to_owned();
    User { id, name, avatar, bot: user["bot"].as_bool().unwrap_or(false), tag, nameplate, decoration, name_style, username, boosting_since: None }

}
