use super::*;

#[derive(Clone)]
pub struct Client {
    pub(super) http: reqwest::Client,
    pub(super) token: String,
    pub(super) commands: mpsc::UnboundedSender<Value>,
    // The voice engine's view of this gateway: it sends its own ops and sees VOICE_* / STREAM_* dispatches.
    pub voice: crate::voice::GatewayHandle,
}

// One runtime for the gateway, REST and QR sign-in; GPUI awaits its results on its own executor.
pub fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| Runtime::new().expect("tokio runtime"))
}

pub fn connect(token: String) -> (Client, mpsc::UnboundedReceiver<Event>) {
    let (commands, command_rx) = mpsc::unbounded();
    let (voice, voice_out) = crate::voice::GatewayHandle::new();
    let client = Client { http: reqwest::Client::new(), token: token.clone(), commands, voice: voice.clone() };
    let (tx, rx) = mpsc::unbounded();
    runtime().spawn(run_gateway(token, tx, Outbound { commands: command_rx, voice, voice_out }));
    (client, rx)
}

impl Client {
    pub(super) fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http.request(method, format!("{API}{path}")).header("Authorization", &self.token)
    }

    pub(super) async fn run(&self, req: reqwest::RequestBuilder) -> Result<Value, String> {
        runtime().spawn(send(req)).await.map_err(|e| e.to_string())?
    }

    /// Sends a raw gateway payload (lazy member list, member requests, voice state updates).
    pub fn gateway(&self, payload: Value) {
        let _ = self.commands.unbounded_send(payload);
    }

    // Discord's REQUEST_CHANNEL_INFO (op 43): voice channel statuses and start times, answered by CHANNEL_INFO.
    pub fn request_channel_info(&self, guild_id: &str) {
        self.gateway(json!({ "op": 43, "d": { "guild_id": guild_id, "fields": ["status", "voice_start_time"] } }));
    }

    // Discord's REQUEST_SOUNDBOARD_SOUNDS (op 31): each server's soundboard, answered by SOUNDBOARD_SOUNDS.
    pub fn request_soundboard_sounds(&self, guild_ids: &[String]) {
        self.gateway(json!({ "op": 31, "d": { "guild_ids": guild_ids } }));
    }

    pub async fn default_soundboard_sounds(&self) -> Result<Vec<SoundboardSound>, String> {
        let body = self.run(self.request(reqwest::Method::GET, "/soundboard-default-sounds")).await?;
        Ok(body.as_array().into_iter().flatten().filter_map(SoundboardSound::parse).collect())
    }

    // Plays `sound` to everyone in the voice channel. Dropping the future cancels the request, as
    // Discord's client does when you leave the channel before it went out.
    pub async fn send_soundboard_sound(&self, channel_id: &str, sound: &SoundboardSound) -> Result<(), String> {
        let req = self.request(reqwest::Method::POST, &format!("/channels/{channel_id}/send-soundboard-sound")).json(&soundboard_play_body(sound));
        let mut send = AbortOnDrop(runtime().spawn(send(req)));
        (&mut send.0).await.map_err(|e| e.to_string())?.map(|_| ())
    }

    // The sound file from the CDN: an mp3 or ogg, which its URL doesn't say.
    pub async fn soundboard_file(&self, sound_id: &str) -> Result<bytes::Bytes, String> {
        let get = self.http.get(soundboard_url(sound_id));
        let mut fetch = AbortOnDrop(runtime().spawn(async move {
            let res = get.send().await?.error_for_status()?;
            res.bytes().await
        }));
        (&mut fetch.0).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())
    }

    pub fn request_member_list(&self, guild_id: &str, channel_id: &str) {
        self.gateway(json!({
            "op": 14,
            "d": { "guild_id": guild_id, "typing": true, "activities": true, "threads": true, "channels": { channel_id: [[0, 99]] } }
        }));
    }

    /// Members per role across the server, by role id.
    pub async fn role_member_counts(&self, guild_id: &str) -> Result<HashMap<String, u32>, String> {
        let body = self.run(self.request(reqwest::Method::GET, &format!("/guilds/{guild_id}/roles/member-counts"))).await?;
        Ok(body.as_object().into_iter().flatten().filter_map(|(role, count)| Some((role.clone(), count.as_u64()? as u32))).collect())
    }

    /// The ids of a role's members, as many as the server sends.
    // Not verified: the cap on how many ids one response carries (the client sets none).
    pub async fn role_member_ids(&self, guild_id: &str, role_id: &str) -> Result<Vec<String>, String> {
        let body = self.run(self.request(reqwest::Method::GET, &format!("/guilds/{guild_id}/roles/{role_id}/member-ids"))).await?;
        Ok(strings(&body))
    }

    // Discord's REQUEST_FORUM_UNREADS (op 28): unread counts since each ack, answered by FORUM_UNREADS. Callers send at most 180.
    pub fn request_forum_unreads(&self, guild_id: &str, forum_id: &str, threads: &[(String, u64)]) {
        let threads: Vec<Value> = threads.iter().map(|(id, ack)| json!({ "thread_id": id, "ack_message_id": ack.to_string() })).collect();
        self.gateway(json!({ "op": 28, "d": { "guild_id": guild_id, "channel_id": forum_id, "threads": threads } }));
    }

    pub fn request_members(&self, guild_id: &str, user_ids: Vec<String>) {
        self.gateway(json!({ "op": 8, "d": { "guild_id": guild_id, "user_ids": user_ids, "presences": false } }));
    }

    // Members whose username or nickname starts with `query`; they arrive as GUILD_MEMBERS_CHUNK.
    // Not verified: a user account's op 8 is believed to accept `query` and `limit` as a bot's does.
    pub fn search_members(&self, guild_id: &str, query: &str, limit: u32) {
        self.gateway(json!({ "op": 8, "d": { "guild_id": guild_id, "query": query, "limit": limit, "presences": false } }));
    }

    /// Oldest first. `before` pages back through history; `around` loads the window a reply jumps to.
    pub async fn messages(&self, channel_id: &str, before: Option<&str>, around: Option<&str>) -> Result<Vec<Message>, String> {
        let mut path = format!("/channels/{channel_id}/messages?limit=50");
        if let Some(id) = before {
            path.push_str(&format!("&before={id}"));
        }
        if let Some(id) = around {
            path.push_str(&format!("&around={id}"));
        }
        let body = self.run(self.request(reqwest::Method::GET, &path)).await?;
        let mut messages: Vec<Message> = body.as_array().into_iter().flatten().filter_map(parse_message).collect();
        messages.reverse();
        Ok(messages)
    }

    /// `ping_author`: whether a reply mentions the person replied to (Discord's "@ ON / OFF").
    pub async fn send_message(&self, channel_id: &str, content: &str, nonce: &str, reply_to: Option<&str>, ping_author: bool, options: SendOptions) -> Result<Message, String> {
        let mut body = message_body(content, nonce, reply_to, ping_author, &[]);
        options.apply(&mut body);
        let req = self.request(reqwest::Method::POST, &format!("/channels/{channel_id}/messages")).json(&body);
        self.run(req).await.and_then(|v| parse_message(&v).ok_or_else(|| "Unexpected response".into()))
    }

    /// Forwards `original` into `channel_id`: a message that is only a type-1 reference to it. `guild_id` is
    /// the original's server, which history loaded over REST doesn't carry on the message.
    pub async fn forward_message(&self, channel_id: &str, original: &Message, guild_id: Option<&str>, nonce: &str) -> Result<Message, String> {
        let body = forward_body(original, guild_id, nonce);
        let req = self.request(reqwest::Method::POST, &format!("/channels/{channel_id}/messages")).json(&body);
        self.run(req).await.and_then(|v| parse_message(&v).ok_or_else(|| "Unexpected response".into()))
    }

    /// A message that is one sticker, as Discord's picker sends it: no text.
    pub async fn send_sticker(&self, channel_id: &str, sticker_id: &str, nonce: &str, reply_to: Option<&str>, ping_author: bool) -> Result<Message, String> {
        let mut body = message_body("", nonce, reply_to, ping_author, &[]);
        body["sticker_ids"] = json!([sticker_id]);
        let req = self.request(reqwest::Method::POST, &format!("/channels/{channel_id}/messages")).json(&body);
        self.run(req).await.and_then(|v| parse_message(&v).ok_or_else(|| "Unexpected response".into()))
    }

    /// Discord's "Wave to say hi!": a sticker sent as a reply to a join notice, pinging the joiner.
    pub async fn greet(&self, channel_id: &str, guild_id: &str, message_id: &str, sticker_id: &str) -> Result<Message, String> {
        let body = json!({ "sticker_ids": [sticker_id], "message_reference": { "guild_id": guild_id, "channel_id": channel_id, "message_id": message_id } });
        let req = self.request(reqwest::Method::POST, &format!("/channels/{channel_id}/greet")).json(&body);
        self.run(req).await.and_then(|v| parse_message(&v).ok_or_else(|| "Unexpected response".into()))
    }

    /// A message that is one poll; `layout_type` 1 is Discord's only layout.
    pub async fn send_poll(&self, channel_id: &str, poll: &PollDraft, nonce: &str) -> Result<Message, String> {
        let mut body = message_body("", nonce, None, false, &[]);
        let answers: Vec<Value> = poll
            .answers
            .iter()
            .map(|(text, emoji)| match emoji {
                // Only one field: the id for a server's emoji, the character for a Unicode one.
                Some(Emoji { id: Some(id), .. }) => json!({ "poll_media": { "text": text, "emoji": { "id": id } } }),
                Some(e) => json!({ "poll_media": { "text": text, "emoji": { "name": e.name } } }),
                None => json!({ "poll_media": { "text": text } }),
            })
            .collect();
        body["poll"] = json!({ "question": { "text": poll.question }, "answers": answers, "duration": poll.hours, "allow_multiselect": poll.multiselect, "layout_type": 1 });
        let req = self.request(reqwest::Method::POST, &format!("/channels/{channel_id}/messages")).json(&body);
        self.run(req).await.and_then(|v| parse_message(&v).ok_or_else(|| "Unexpected response".into()))
    }

    /// The server's index (a DM's comes from the channel) merged with your user-installed apps' index,
    /// as Discord's client merges them. Either alone still lists; both failing is the context's error.
    pub async fn command_index(&self, channel_id: &str, guild_id: Option<&str>) -> Result<crate::app_commands::CommandIndex, String> {
        use crate::app_commands::{CommandIndex, parse_index};
        let path = match guild_id {
            Some(g) => format!("/guilds/{g}/application-command-index"),
            None => format!("/channels/{channel_id}/application-command-index"),
        };
        let context = runtime().spawn(fetch_index(self.request(reqwest::Method::GET, &path)));
        let user = runtime().spawn(fetch_index(self.request(reqwest::Method::GET, "/users/@me/application-command-index")));
        let (context, user) = futures::join!(context, user);
        let (context, user) = (context.map_err(|e| e.to_string())?.map(|v| parse_index(&v)), user.map_err(|e| e.to_string())?.map(|v| parse_index(&v)));
        match (context, user) {
            (Ok(context), Ok(user)) => Ok(CommandIndex::merge(context, user)),
            (Ok(only), Err(_)) | (Err(_), Ok(only)) => Ok(only),
            (Err(error), Err(_)) => Err(error),
        }
    }

    /// Answers 204; what happens next arrives as INTERACTION_* dispatches matched by the body's nonce.
    /// No retries: a refusal is the interaction's failure. Component presses and autocomplete give up
    /// after 3 s, as Discord's client does; commands and modal submits wait.
    pub async fn interact(&self, body: Value) -> Result<(), InteractError> {
        let mut req = self.request(reqwest::Method::POST, "/interactions").json(&body);
        if matches!(body["type"].as_u64(), Some(3 | 4)) {
            req = req.timeout(Duration::from_secs(3));
        }
        runtime().spawn(send_interaction(req)).await.unwrap_or_else(|_| Err(InteractError::default()))
    }

    /// Uploads files for an app command's ATTACHMENT options or a form's file fields the way messages do (Cloud Uploads): a slot
    /// per file, a PUT of each, and the uploaded names to pass to `app_commands::attach_files`.
    pub async fn stage_files(&self, channel_id: &str, files: &[Upload]) -> Result<Vec<crate::app_commands::CommandAttachment>, String> {
        let uploaded = self.put_uploads(channel_id, files).await?;
        Ok(uploaded
            .into_iter()
            .map(|a| crate::app_commands::CommandAttachment { spoiler: a.spoiler || a.filename.starts_with("SPOILER_"), filename: a.filename, uploaded_filename: a.uploaded_filename })
            .collect())
    }

    // A slot per file, an abortable PUT of each, and each file as Create Message names it.
    pub(super) async fn put_uploads(&self, channel_id: &str, files: &[Upload]) -> Result<Vec<SentAttachment>, String> {
        let req = self.request(reqwest::Method::POST, &format!("/channels/{channel_id}/attachments")).json(&attachments_request(files));
        let slots = self.run(req).await?;
        let slots = parse_upload_slots(&slots, files.len())?;
        for (file, (url, _)) in files.iter().zip(&slots) {
            // Cancel drops this future; the guard stops the PUT with it.
            let mut put = AbortOnDrop(runtime().spawn(put_file(self.http.clone(), url.clone(), file.path.clone(), file.sent.clone())));
            (&mut put.0).await.map_err(|e| e.to_string())??;
        }
        Ok(files
            .iter()
            .zip(slots)
            .map(|(f, (_, uploaded_filename))| SentAttachment { filename: f.name.clone(), uploaded_filename, description: f.description.clone(), spoiler: f.spoiler })
            .collect())
    }

    /// The FRECENCY_AND_FAVORITES settings proto (type 2) as bytes: `GET /users/@me/settings-proto/2`
    /// answers `{settings: base64}`, as Discord's client reads it (`web#594061`).
    pub async fn frecency_settings(&self) -> Result<Vec<u8>, String> {
        use base64::Engine;
        let body = self.run(self.request(reqwest::Method::GET, "/users/@me/settings-proto/2")).await?;
        let settings = body["settings"].as_str().ok_or("No settings in the answer")?;
        base64::engine::general_purpose::STANDARD.decode(settings).map_err(|e| e.to_string())
    }

    /// Discord's own packs, for everyone.
    pub async fn sticker_packs(&self) -> Result<Vec<StickerPack>, String> {
        let body = self.run(self.request(reqwest::Method::GET, "/sticker-packs")).await?;
        Ok(body["sticker_packs"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| StickerPack { name: p["name"].as_str().unwrap_or_default().to_owned(), stickers: p["stickers"].as_array().into_iter().flatten().filter_map(Sticker::parse).collect() })
            .filter(|p| !p.stickers.is_empty())
            .collect())
    }

    /// One message carrying all the files, as Discord's own client sends it (docs.discord.food, Cloud
    /// Uploads): upload URLs for every file, a PUT of each to its URL, then the message naming them.
    /// Each file's `sent` counts its bytes as they go out.
    /// `reply`: the message replied to, and whether the reply pings its author.
    pub async fn upload(&self, channel_id: &str, files: &[Upload], content: &str, nonce: &str, reply: Option<(&str, bool)>, options: SendOptions) -> Result<Message, String> {
        let uploaded = self.put_uploads(channel_id, files).await?;
        let mut body = message_body(content, nonce, reply.map(|(id, _)| id), reply.is_some_and(|(_, ping)| ping), &uploaded);
        options.apply(&mut body);
        let req = self.request(reqwest::Method::POST, &format!("/channels/{channel_id}/messages")).json(&body);
        self.run(req).await.and_then(|v| parse_message(&v).ok_or_else(|| "Unexpected response".into()))
    }

    /// Recent messages that mention you, across servers (the title bar's Inbox).
    /// Recent mentions of you; `everyone` and `roles` include @everyone/@here and role mentions, `guild_id`
    /// keeps one server's.
    pub async fn mentions(&self, everyone: bool, roles: bool, guild_id: Option<&str>) -> Result<Vec<Message>, String> {
        let mut path = format!("/users/@me/mentions?limit=25&roles={roles}&everyone={everyone}");
        if let Some(guild) = guild_id {
            path.push_str(&format!("&guild_id={guild}"));
        }
        let body = self.run(self.request(reqwest::Method::GET, &path)).await?;
        Ok(body.as_array().into_iter().flatten().filter_map(parse_message).collect())
    }

    /// One of your settings-proto blobs (base64), by Discord's settings type.
    pub async fn settings_proto(&self, kind: u8) -> Result<String, String> {
        let body = self.run(self.request(reqwest::Method::GET, &format!("/users/@me/settings-proto/{kind}"))).await?;
        body["settings"].as_str().map(str::to_owned).ok_or_else(|| "Unexpected response".into())
    }

    /// Merges `settings` (base64, only the fields to change) into a settings-proto blob.
    pub async fn patch_settings_proto(&self, kind: u8, settings: String) -> Result<(), String> {
        let req = self.request(reqwest::Method::PATCH, &format!("/users/@me/settings-proto/{kind}")).json(&serde_json::json!({ "settings": settings }));
        self.run(req).await.map(drop)
    }

    /// Your saved-for-later messages, newest saved first (Discord's bookmarks, 25 a page).
    pub async fn saved_messages(&self) -> Result<Vec<Message>, String> {
        let body = self.run(self.request(reqwest::Method::GET, "/users/@me/saved-messages/bookmarks?limit=25")).await?;
        Ok(body["results"].as_array().into_iter().flatten().filter_map(|r| parse_message(&r["message"])).collect())
    }

    /// The ids of the messages you saved, as the client loads them on connect.
    pub async fn saved_message_ids(&self) -> Result<Vec<String>, String> {
        let body = self.run(self.request(reqwest::Method::GET, "/users/@me/saved-messages?bookmark_ids=true")).await?;
        Ok(bookmark_ids(&body["bookmark_ids"]))
    }

    /// Discord's GIF search; an empty query returns what's trending. Shapes from memory: Not verified.
    pub async fn gifs(&self, query: &str) -> Result<Vec<Gif>, String> {
        let mut url = reqwest::Url::parse(&format!("{API}/gifs/{}", if query.is_empty() { "trending-gifs" } else { "search" })).map_err(|e| e.to_string())?;
        url.query_pairs_mut().append_pair("provider", "tenor").append_pair("locale", "en-US").append_pair("media_format", "gif");
        if !query.is_empty() {
            url.query_pairs_mut().append_pair("q", query);
        }
        let body = self.run(self.http.get(url).header("Authorization", &self.token)).await?;
        Ok(body
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|g| {
                Some(Gif {
                    url: g["url"].as_str()?.to_owned(),
                    preview: g["gif_src"].as_str().or(g["src"].as_str())?.to_owned(),
                    width: g["width"].as_f64().unwrap_or(200.) as f32,
                    height: g["height"].as_f64().unwrap_or(150.) as f32,
                })
            })
            .collect())
    }

    /// The member list's Activity feed: everyone this account knows, not one server's; the panel keeps
    /// the entries whose author is in the open server.
    pub async fn activity_feed(&self) -> Result<Vec<FeedEntry>, String> {
        let body = self.run(self.request(reqwest::Method::GET, "/content-inventory/users/@me")).await?;
        Ok(parse_activity_feed(&body))
    }

    /// Newest pin first. Discord moved pins to `/messages/pins`, which wraps each as `{ pinned_at, message }`;
    /// the old `/pins` route is deprecated.
    // ponytail: first page only (50); page with `before` = the last `pinned_at` if a channel needs more.
    pub async fn pins(&self, channel_id: &str) -> Result<Vec<Message>, String> {
        let body = self.run(self.request(reqwest::Method::GET, &format!("/channels/{channel_id}/messages/pins?limit=50"))).await?;
        Ok(parse_pins(&body))
    }

    pub async fn edit_message(&self, channel_id: &str, id: &str, content: &str) -> Result<(), String> {
        let req = self.request(reqwest::Method::PATCH, &format!("/channels/{channel_id}/messages/{id}")).json(&json!({ "content": content }));
        self.run(req).await.map(|_| ())
    }

    // `PUT /channels/{id}/voice-status`, 204 on success; "" clears it. Discord's refusal comes back as its message.
    pub async fn set_voice_channel_status(&self, channel_id: &str, status: &str) -> Result<(), String> {
        let req = self.request(reqwest::Method::PUT, &format!("/channels/{channel_id}/voice-status")).json(&json!({ "status": status }));
        self.run(req).await.map(|_| ())
    }

    pub async fn delete_message(&self, channel_id: &str, id: &str) -> Result<(), String> {
        self.run(self.request(reqwest::Method::DELETE, &format!("/channels/{channel_id}/messages/{id}"))).await.map(|_| ())
    }

    pub async fn react(&self, channel_id: &str, message_id: &str, emoji: &Emoji, add: bool) -> Result<(), String> {
        let mut url = reqwest::Url::parse(&format!("{API}/channels/{channel_id}/messages/{message_id}/reactions")).map_err(|e| e.to_string())?;
        // path_segments_mut percent-encodes the emoji, which unicode reactions need.
        url.path_segments_mut().map_err(|_| "Bad URL")?.push(&emoji.key()).push("@me");
        let method = if add { reqwest::Method::PUT } else { reqwest::Method::DELETE };
        self.run(self.http.request(method, url).header("Authorization", &self.token)).await.map(|_| ())
    }

    /// One page of who reacted with `emoji`, in reaction order, starting after the user id `after`.
    pub async fn reactors(&self, channel_id: &str, message_id: &str, emoji: &Emoji, after: Option<&str>, limit: usize) -> Result<Vec<Reactor>, String> {
        let after = after.map(|id| format!("&after={id}")).unwrap_or_default();
        let path = format!("/channels/{channel_id}/messages/{message_id}/reactions/{}?limit={limit}{after}", emoji.path());
        let body = self.run(self.request(reqwest::Method::GET, &path)).await?;
        let reactor = |v: &Value| Reactor { user: parse_user(v), username: v["username"].as_str().unwrap_or_default().to_owned() };
        Ok(body.as_array().map(|users| users.iter().map(reactor).collect()).unwrap_or_default())
    }

    pub async fn typing(&self, channel_id: &str) {
        let _ = self.run(self.request(reqwest::Method::POST, &format!("/channels/{channel_id}/typing"))).await;
    }

    pub async fn ack(&self, channel_id: &str, message_id: &str) {
        let req = self.request(reqwest::Method::POST, &format!("/channels/{channel_id}/messages/{message_id}/ack")).json(&json!({ "token": null }));
        let _ = self.run(req).await;
    }

    /// Discord's Mark Unread: acks `ack_id`, just before the message marked, with the mentions after it.
    /// The gateway echoes it to every device as a manual MESSAGE_ACK.
    pub async fn ack_manual(&self, channel_id: &str, ack_id: u64, mention_count: u32) {
        let req = self.request(reqwest::Method::POST, &format!("/channels/{channel_id}/messages/{ack_id}/ack")).json(&json!({ "manual": true, "mention_count": mention_count }));
        let _ = self.run(req).await;
    }

    /// Clears one mention from Recent Mentions on every device; the channel's read state stays.
    pub async fn delete_mention(&self, message_id: &str) -> Result<(), String> {
        self.run(self.request(reqwest::Method::DELETE, &format!("/users/@me/mentions/{message_id}"))).await.map(|_| ())
    }

    pub async fn profile(&self, user_id: &str, guild_id: Option<&str>) -> Result<Profile, String> {
        let mut path = format!("/users/{user_id}/profile?with_mutual_guilds=true&with_mutual_friends=true");
        if let Some(guild) = guild_id {
            path.push_str(&format!("&guild_id={guild}"));
        }
        let body = self.run(self.request(reqwest::Method::GET, &path)).await?;
        let mut profile = parse_profile(user_id, &body);
        // No note answers 404, which reads as none.
        let note = self.run(self.request(reqwest::Method::GET, &format!("/users/@me/notes/{user_id}"))).await.ok();
        profile.note = note.and_then(|n| n["note"].as_str().map(str::to_owned)).filter(|n| !n.is_empty());
        Ok(profile)
    }

    /// A profile frame's layers, from the collectibles product its sku names.
    pub async fn profile_frame(&self, sku: &str) -> Result<Option<ProfileFrame>, String> {
        let body = self.run(self.request(reqwest::Method::GET, &format!("/collectibles-products/{sku}"))).await?;
        Ok(parse_profile_frame(sku, &body))
    }

    /// What they've been doing lately: their content outbox, for the full profile's Recent activity.
    pub async fn recent_activity(&self, user_id: &str) -> Result<Vec<RecentEntry>, String> {
        let body = self.run(self.request(reqwest::Method::GET, &format!("/content-inventory/users/{user_id}/outbox"))).await?;
        Ok(parse_outbox(&body))
    }

    // A profile effect's layers, from the collectibles product its sku names.
    pub async fn profile_effect(&self, sku: &str) -> Result<Vec<EffectLayer>, String> {
        let body = self.run(self.request(reqwest::Method::GET, &format!("/collectibles-products/{sku}"))).await?;
        Ok(parse_profile_effect(&body))
    }

    /// Opens (or finds) the DM channel with a user.
    pub async fn open_dm(&self, user_id: &str) -> Result<Channel, String> {
        self.open_private(&[user_id.to_owned()]).await
    }

    /// The DM with one recipient, or a new group DM with several (type 3 in the answer).
    pub async fn open_private(&self, recipients: &[String]) -> Result<Channel, String> {
        let req = self.request(reqwest::Method::POST, "/users/@me/channels").json(&json!({ "recipients": recipients }));
        let body = self.run(req).await?;
        parse_dm(&body, &Value::Null).ok_or_else(|| "Unexpected response".into())
    }

    /// Changes your status everywhere: the gateway presence now, the saved setting for next time.
    /// `custom` rides along so the presence update doesn't wipe your custom status.
    pub async fn set_status(&self, status: Status, custom: Option<&CustomStatus>, game: Option<&crate::games::Game>, activity: Option<Value>) {
        self.send_presence(status, custom, game, activity);
        let req = self.request(reqwest::Method::PATCH, "/users/@me/settings").json(&json!({ "status": status_wire(status) }));
        let _ = self.run(req).await;
    }

    // A custom status travels as a type-4 activity in the presence, the game you're playing as type 0, and an
    // activity you're in (Discord's embedded app) as its own type-0 entry, which leads.
    pub fn send_presence(&self, status: Status, custom: Option<&CustomStatus>, game: Option<&crate::games::Game>, activity: Option<Value>) {
        let activities: Vec<Value> = custom.map(CustomStatus::activity).into_iter().chain(activity).chain(game.map(crate::games::Game::activity)).collect();
        self.gateway(json!({ "op": 3, "d": { "status": status_wire(status), "since": 0, "activities": activities, "afk": false } }));
    }

    /// An application's icon hash; `/rpc` is the public subset of the application, readable by anyone.
    pub async fn app_icon(&self, app_id: &str) -> Result<Option<String>, String> {
        let body = self.run(self.request(reqwest::Method::GET, &format!("/applications/{app_id}/rpc"))).await?;
        Ok(body["icon"].as_str().map(str::to_owned))
    }

    /// Saves account settings: `patch` holds only the fields that changed.
    // Not verified: the legacy endpoint still accepting each field Settings sends (it accepts `status`).
    pub async fn set_account_settings(&self, patch: Value) -> Result<(), String> {
        let req = self.request(reqwest::Method::PATCH, "/users/@me/settings").json(&patch);
        self.run(req).await.map(|_| ())
    }

    /// Changes your account (PATCH /users/@me): display name, username (with `password`), avatar, banner.
    /// Images go as data URIs. Returns the updated user.
    pub async fn modify_me(&self, patch: Value) -> Result<Value, String> {
        let req = self.request(reqwest::Method::PATCH, "/users/@me").json(&patch);
        self.run(req).await
    }

    /// Your profile as its editor loads it, raw: `GET /users/{id}/profile`, with the server profile when `guild_id` is set.
    pub async fn my_profile(&self, user_id: &str, guild_id: Option<&str>) -> Result<Value, String> {
        let guild = guild_id.map(|g| format!("&guild_id={g}")).unwrap_or_default();
        self.run(self.request(reqwest::Method::GET, &format!("/users/{user_id}/profile?with_mutual_guilds=false{guild}"))).await
    }

    /// Saves your main profile (PATCH /users/@me/profile): `bio`, `pronouns`, `accent_color`. Returns the profile.
    pub async fn modify_my_profile(&self, patch: Value) -> Result<Value, String> {
        self.run(self.request(reqwest::Method::PATCH, "/users/@me/profile").json(&patch)).await
    }

    /// Saves your profile on one server (PATCH /guilds/{id}/profile/@me): `pronouns`, and for Nitro `bio` and `banner`.
    pub async fn modify_guild_profile(&self, guild_id: &str, patch: Value) -> Result<Value, String> {
        self.run(self.request(reqwest::Method::PATCH, &format!("/guilds/{guild_id}/profile/@me")).json(&patch)).await
    }

    /// Saves your member entry on one server (PATCH /guilds/{id}/members/@me): `nick`, and for Nitro the server avatar.
    pub async fn modify_my_member(&self, guild_id: &str, patch: Value) -> Result<Value, String> {
        self.run(self.request(reqwest::Method::PATCH, &format!("/guilds/{guild_id}/members/@me")).json(&patch)).await
    }

    /// Wears a server's tag next to your name, or none.
    pub async fn set_server_tag(&self, guild_id: Option<&str>) -> Result<Value, String> {
        let body = json!({ "identity_guild_id": guild_id, "identity_enabled": guild_id.is_some() });
        self.run(self.request(reqwest::Method::PUT, "/users/@me/clan").json(&body)).await
    }

    /// A server's public profile, which carries its tag and badge.
    // Not verified: `GET /guilds/{id}/profile` and its `tag` / `badge` fields.
    pub async fn guild_profile(&self, guild_id: &str) -> Result<Value, String> {
        self.run(self.request(reqwest::Method::GET, &format!("/guilds/{guild_id}/profile"))).await
    }

    /// Your last avatars: `{avatars: [{id, storage_hash, description}]}`.
    pub async fn recent_avatars(&self) -> Result<Value, String> {
        self.run(self.request(reqwest::Method::GET, "/users/@me/avatars")).await
    }

    pub async fn delete_recent_avatar(&self, avatar_id: &str) -> Result<(), String> {
        self.run(self.request(reqwest::Method::DELETE, &format!("/users/@me/avatars/{avatar_id}"))).await.map(|_| ())
    }

    /// Saves the rail's order and folders. Loose servers are folders without an id.
    // Not verified: the legacy settings endpoint still accepting `guild_folders` (it accepts `status`).
    pub async fn set_guild_folders(&self, folders: Vec<Folder>) {
        let wire: Vec<Value> = folders
            .iter()
            .map(|f| json!({ "id": f.id.parse::<u64>().ok(), "name": f.name, "color": f.color, "guild_ids": f.guild_ids }))
            .collect();
        let positions: Vec<&String> = folders.iter().flat_map(|f| &f.guild_ids).collect();
        let req = self.request(reqwest::Method::PATCH, "/users/@me/settings").json(&json!({ "guild_folders": wire, "guild_positions": positions }));
        let _ = self.run(req).await;
    }

    /// Any REST call the menus make: moderation, relationships, notes. `body` None sends no JSON.
    pub async fn call(&self, method: reqwest::Method, path: String, body: Option<Value>) -> Result<Value, String> {
        let req = self.request(method, &path);
        let req = match body {
            Some(body) => req.json(&body),
            None => req,
        };
        self.run(req).await
    }

    /// A call whose failure the caller reads itself: Discord's status and body as sent, with none of `call`'s
    /// retries (a publish's 429 carries the minutes to wait in `retry_after`).
    pub async fn call_raw(&self, method: reqwest::Method, path: String, body: Option<Value>) -> Result<(u16, Value), String> {
        let req = self.request(method, &path);
        let req = match body {
            Some(body) => req.json(&body),
            None => req,
        };
        let sent = async move {
            let res = req.send().await.map_err(|e| e.to_string())?;
            let status = res.status().as_u16();
            Ok((status, res.json().await.unwrap_or(Value::Null)))
        };
        runtime().spawn(sent).await.map_err(|e| e.to_string())?
    }

    /// The message a link points at, through the history endpoint (`around`, one message), as Discord's
    /// client does. None: it's gone, or the channel isn't one you can read (403, or no history access, which
    /// returns an empty list).
    pub async fn linked_message(&self, channel_id: &str, message_id: &str) -> Result<Option<Message>, String> {
        let req = self.request(reqwest::Method::GET, &format!("/channels/{channel_id}/messages?around={message_id}&limit=1"));
        let body = runtime().spawn(send_or_gone(req)).await.map_err(|e| e.to_string())??;
        Ok(body.and_then(|b| b.as_array()?.iter().filter_map(parse_message).find(|m| m.id == message_id)))
    }

    /// An invite's server card. None: the code is unknown or expired (404).
    pub async fn invite(&self, code: &str) -> Result<Option<Invite>, String> {
        let req = self.request(reqwest::Method::GET, &format!("/invites/{code}?with_counts=true"));
        let body = runtime().spawn(send_or_gone(req)).await.map_err(|e| e.to_string())??;
        Ok(body.as_ref().and_then(parse_invite))
    }

    /// Joins the invite's server (`POST /invites/{code}`) and returns its id; already a member succeeds too.
    /// Discord usually wants a captcha first (see `call_with_captcha`).
    pub async fn join_invite(&self, code: &str, captcha_url: mpsc::UnboundedSender<String>) -> Result<String, String> {
        let body = self.call_with_captcha(reqwest::Method::POST, format!("/invites/{code}"), Some(json!({})), captcha_url).await?;
        joined_guild(&body).ok_or_else(|| "That invite isn't to a server".into())
    }

    /// A call Discord may answer with a captcha (joining a server, sending a friend request): its page goes
    /// out on `captcha_url` for the browser, and the call is retried with the solved key, as the sign-in
    /// does (`auth::exchange_ticket`). Other failures read as `send` words them.
    pub async fn call_with_captcha(&self, method: reqwest::Method, path: String, body: Option<Value>, captcha_url: mpsc::UnboundedSender<String>) -> Result<Value, ApiError> {
        let client = self.clone();
        runtime().spawn(async move { client.captcha_loop(method, &path, body, captcha_url).await }).await.map_err(|e| ApiError::from(e.to_string()))?
    }

    pub(super) async fn captcha_loop(&self, method: reqwest::Method, path: &str, body: Option<Value>, captcha_url: mpsc::UnboundedSender<String>) -> Result<Value, ApiError> {
        let mut solved: Option<(crate::captcha::Challenge, String)> = None;
        for _ in 0..3 {
            let mut req = self.request(method.clone(), path);
            if let Some(body) = &body {
                req = req.json(body);
            }
            if let Some((challenge, answer)) = &solved {
                for (name, value) in challenge.headers(answer) {
                    req = req.header(name, value);
                }
            }
            let res = req.send().await.map_err(|e| e.to_string())?;
            let status = res.status();
            let body: Value = res.json().await.unwrap_or(Value::Null);
            if status.is_success() {
                return Ok(body);
            }
            let Some(challenge) = crate::captcha::Challenge::from_response(&body) else {
                eprintln!("http: {status} {path} (code {}): {body}", body["code"]);
                return Err(ApiError { status: status.as_u16(), code: body["code"].as_u64(), text: error_text(status.as_u16(), &body) });
            };
            let pending = crate::captcha::serve(&challenge).await?;
            let _ = captcha_url.unbounded_send(pending.url.clone());
            solved = Some((challenge, pending.solved().await?));
        }
        Err(ApiError::from("Discord kept asking for a captcha".to_owned()))
    }

    /// Marks several channels read at once (a server's "Mark As Read").
    // A form upload with one file, as sticker creation takes it.
    pub async fn call_form(&self, path: String, fields: Vec<(&'static str, String)>, file_name: String, bytes: Vec<u8>) -> Result<Value, String> {
        let form = fields.into_iter().fold(reqwest::multipart::Form::new(), |form, (key, value)| form.text(key, value));
        let form = form.part("file", reqwest::multipart::Part::bytes(bytes).file_name(file_name));
        self.run(self.request(reqwest::Method::POST, &path).multipart(form)).await
    }

    // A JSON body with files, as `payload_json` and `files[n]` parts; the body names each by its index.
    pub async fn call_files(&self, path: String, payload: Value, files: Vec<Upload>) -> Result<Value, String> {
        let req = self.request(reqwest::Method::POST, &path);
        let sent = async move {
            let mut form = reqwest::multipart::Form::new().text("payload_json", payload.to_string());
            for (i, file) in files.into_iter().enumerate() {
                let bytes = std::fs::read(&file.path).map_err(|e| format!("{}: {e}", file.name))?;
                form = form.part(format!("files[{i}]"), reqwest::multipart::Part::bytes(bytes).file_name(file.name));
            }
            send(req.multipart(form)).await
        };
        runtime().spawn(sent).await.map_err(|e| e.to_string())?
    }

    pub async fn ack_bulk(&self, reads: Vec<(String, u64)>) {
        let read_states: Vec<Value> = reads.into_iter().map(|(channel, message)| json!({ "channel_id": channel, "message_id": message.to_string(), "read_state_type": 0 })).collect();
        let req = self.request(reqwest::Method::POST, "/read-states/ack-bulk").json(&json!({ "read_states": read_states }));
        let _ = self.run(req).await;
    }

    /// `scope` is `guilds/{id}` or, for a DM, `channels/{id}`. `params` are Discord's search filters, the
    /// page and the sort (docs.discord.food, Search Guild Messages): 25 hits a page, `offset` in hits.
    pub async fn search(&self, scope: &str, content: &str, params: &[(String, String)]) -> Result<SearchPage, String> {
        let mut url = reqwest::Url::parse(&format!("{API}/{scope}/messages/search")).map_err(|e| e.to_string())?;
        {
            let mut pairs = url.query_pairs_mut();
            if !content.is_empty() {
                pairs.append_pair("content", content);
            }
            for (name, value) in params {
                pairs.append_pair(name, value);
            }
        }
        let body = self.run(self.http.get(url).header("Authorization", &self.token)).await?;
        // Each hit is a small array of messages around the match; the match is the one marked `hit`.
        let messages = body["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|around| around.as_array().and_then(|a| a.iter().find(|m| m["hit"].as_bool() == Some(true)).or_else(|| a.first())).and_then(parse_message))
            .collect();
        Ok(SearchPage { messages, total: body["total_results"].as_u64().unwrap_or(0) as u32 })
    }
}


// `bookmark_ids` from GET /users/@me/saved-messages. Not verified: each entry's shape; read as either a
// message id or a save with `message_id`.
pub(super) fn bookmark_ids(list: &Value) -> Vec<String> {
    list.as_array().into_iter().flatten().filter_map(|v| v.as_str().or_else(|| v["message_id"].as_str())).map(str::to_owned).collect()
}
