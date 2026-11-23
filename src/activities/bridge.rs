// What an activity's SDK asks the client, answered: who's here, which channel, sign-in through Discord (behind
// Waffle's consent dialog), and the asks that need your say first (links, sharing, buying).
use super::*;

pub(super) const CONSENT_WIDTH: f32 = 400.;
pub(super) const SHARE_PREVIEW: f32 = 200.;
// SDK RPC error codes: an unknown command, and a sign-in the user turned down.
pub(super) const RPC_UNKNOWN: u32 = 1000;
pub(super) const RPC_OAUTH_DENIED: u32 = 5000;

// What an app asked Waffle to do for it, waiting on you. Each is Waffle's own dialog, never the app's.
pub(super) enum Ask {
    // Open a link in your browser; `trust` ticks "open this site's links without asking".
    Link { url: String, domain: String, trust: bool },
    // Share a picture the app made into the call's chat.
    Share { url: String },
    // Buy something: Waffle can't take payments, so it hands off to Discord in the browser.
    Purchase,
}

pub(super) struct PendingAsk {
    ask: Ask,
    nonce: Value,
    cmd: String,
}

// An activity asking to sign you in, waiting on your answer.
pub(super) struct Consent {
    request: OAuthRequest,
    nonce: Value,
}

// A user as the SDK expects one. GAP: we keep display names, not usernames, so both carry the display name.
pub(super) fn sdk_voice_state(state: &discord::VoiceState, user: &User) -> Value {
    json!({
        "nick": user.name,
        "mute": state.server_muted,
        "voice_state": { "mute": state.server_muted, "deaf": state.server_deafened, "self_mute": state.muted, "self_deaf": state.deafened, "suppress": false },
        "user": sdk_user(user),
    })
}

pub(super) fn sdk_user(user: &User) -> Value {
    json!({
        "id": user.id,
        "username": user.name,
        "global_name": user.name,
        "nickname": user.name,
        "discriminator": "0",
        "avatar": avatar_hash(&user.avatar),
        "avatar_decoration_data": null,
        "bot": user.bot,
        "flags": 0,
        "premium_type": 0,
    })
}

// The hash in a CDN avatar URL (…/avatars/<id>/<hash>.png); None for a default avatar.
pub(super) fn avatar_hash(url: &str) -> Option<&str> {
    let (_, rest) = url.split_once("/avatars/")?;
    let file = rest.split('/').nth(1)?;
    Some(file.split(['.', '?']).next().unwrap_or(file))
}

pub(super) fn scope_text(scope: &str) -> String {
    match scope {
        "identify" => "Know your username, avatar and banner".into(),
        "guilds" => "Know which servers you're in".into(),
        "guilds.members.read" => "Read your profile in this server".into(),
        "rpc.voice.read" => "Know who's in your voice channel".into(),
        "rpc.activities.write" => "Show what you're doing in it on your profile".into(),
        "applications.commands" => "Add commands to use in your chats".into(),
        "email" => "Know your email address".into(),
        "relationships.read" => "Know who your friends are".into(),
        other => format!("Use {other}"),
    }
}


impl Waffle {
    pub(super) fn answer_activity(&mut self, frame: Entity<ActivityFrame>, nonce: Value, cmd: &str, args: &Value, cx: &mut Context<Self>) {
        let reply = |data: Value| Ok::<Value, (u32, String)>(data);
        let answer = match cmd {
            "AUTHORIZE" => return self.authorize_activity(frame, nonce, args, cx),
            "AUTHENTICATE" => return self.authenticate_activity(frame, nonce, args, cx),
            "GET_ACTIVITY_INSTANCE_CONNECTED_PARTICIPANTS" | "GET_INSTANCE_CONNECTED_PARTICIPANTS" => {
                let app_id = frame.read(cx).app_id.clone();
                let instance = self.game_instance(&app_id);
                reply(json!({ "participants": instance.map(|i| self.sdk_participants(&i)).unwrap_or_default() }))
            }
            "GET_CHANNEL" => self.sdk_channel().ok_or((RPC_UNKNOWN, "Not in a call".to_owned())),
            "GET_CHANNEL_PERMISSIONS" => reply(json!({ "permissions": self.call_permissions().to_string() })),
            "USER_SETTINGS_GET_LOCALE" => reply(json!({ "locale": "en-US" })),
            "GET_PLATFORM_BEHAVIORS" => reply(json!({ "iosKeyboardResizesView": false })),
            "SET_CONFIG" => reply(json!({ "use_interactive_pip": args["use_interactive_pip"].as_bool().unwrap_or(false) })),
            "SET_ACTIVITY" => {
                self.set_activity_presence(&args["activity"]);
                reply(args["activity"].clone())
            }
            "GET_SKUS" => reply(json!({ "skus": [] })),
            "GET_ENTITLEMENTS" => reply(json!({ "entitlements": [] })),
            "OPEN_EXTERNAL_LINK" => match args["url"].as_str().and_then(|u| reqwest::Url::parse(u).ok()).filter(|u| matches!(u.scheme(), "https" | "http")) {
                Some(url) => {
                    let domain = url.host_str().unwrap_or_default().to_owned();
                    if self.prefs.trusted_link_domains.contains(&domain) {
                        cx.open_url(url.as_str());
                        reply(json!({ "opened": true }))
                    } else {
                        return self.ask_activity(Ask::Link { url: url.to_string(), domain, trust: false }, nonce, cmd, cx);
                    }
                }
                None => Err((RPC_UNKNOWN, "Only web links open".to_owned())),
            },
            "OPEN_SHARE_MOMENT_DIALOG" => match args["mediaUrl"].as_str().filter(|u| u.starts_with("https://")) {
                Some(url) => return self.ask_activity(Ask::Share { url: url.to_owned() }, nonce, cmd, cx),
                None => Err((RPC_UNKNOWN, "Nothing to share".to_owned())),
            },
            "START_PURCHASE" => return self.ask_activity(Ask::Purchase, nonce, cmd, cx),
            "OPEN_INVITE_DIALOG" => match self.game_place().and_then(|p| Some((p.guild_id.clone()?, p.channel.id.clone()))) {
                Some((guild, channel)) => {
                    let (window, this) = (self.main_window, cx.weak_entity());
                    cx.defer(move |cx| _ = window.update(cx, |_, window, cx| _ = this.update(cx, |this, cx| this.show_invite(guild, channel, window, cx))));
                    reply(Value::Null)
                }
                None => Err((RPC_UNKNOWN, "Invites open from server voice channels".to_owned())),
            },
            "GET_RELATIONSHIPS" => reply(json!({ "relationships": self.sdk_relationships() })),
            "INITIATE_IMAGE_UPLOAD" => return self.upload_for_activity(frame, nonce, cx),
            "SET_ORIENTATION_LOCK_STATE" | "ENCOURAGE_HW_ACCELERATION" | "CAPTURE_LOG" | "SEND_ANALYTICS_EVENT" => reply(Value::Null),
            _ => Err((RPC_UNKNOWN, format!("Waffle doesn't support {cmd} yet"))),
        };
        frame.read(cx).reply(nonce, cmd, answer);
    }

    // Discord's RPC asks first; an app already granted these scopes that asked for no prompt gets its code
    // without one, anything else waits on the consent dialog.
    pub(super) fn authorize_activity(&mut self, frame: Entity<ActivityFrame>, nonce: Value, args: &Value, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let scopes: Vec<String> = match &args["scope"] {
            Value::String(s) => s.split_whitespace().map(str::to_owned).collect(),
            other => other.as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_owned)).collect(),
        };
        let client_id = args["client_id"].as_str().unwrap_or(&frame.read(cx).app_id).to_owned();
        let state = args["state"].as_str().unwrap_or_default().to_owned();
        let silent = args["prompt"].as_str() == Some("none");
        cx.spawn(async move |this, cx| {
            let request = client.oauth_request(&client_id, scopes, state).await;
            let request = match request {
                Ok(request) if request.authorized && silent => {
                    let code = client.oauth_authorize(&request).await;
                    _ = this.update(cx, |_, cx| frame.read(cx).reply(nonce, "AUTHORIZE", code.map(|code| json!({ "code": code })).map_err(|e| (RPC_OAUTH_DENIED, e))));
                    return;
                }
                Ok(request) => request,
                Err(error) => {
                    _ = this.update(cx, |_, cx| frame.read(cx).reply(nonce, "AUTHORIZE", Err((RPC_OAUTH_DENIED, error))));
                    return;
                }
            };
            _ = this.update(cx, |this, cx| {
                this.activities.consent = Some(Consent { request, nonce });
                cx.notify(); // lint-allow(global-notify): the consent dialog is drawn by the root, over the columns
            });
        })
        .detach();
    }

    pub(super) fn ask_activity(&mut self, ask: Ask, nonce: Value, cmd: &str, cx: &mut Context<Self>) {
        // One at a time: an app asking again while you decide gets told no.
        if let Some(old) = self.activities.ask.take()
            && let Some(frame) = &self.activities.frame
        {
            frame.read(cx).reply(old.nonce, &old.cmd, Err((RPC_UNKNOWN, "Replaced by a newer request".into())));
        }
        self.activities.ask = Some(PendingAsk { ask, nonce, cmd: cmd.to_owned() });
        cx.notify(); // lint-allow(global-notify): the ask dialog is drawn by the root, over the columns
    }

    pub(super) fn answer_ask(&mut self, yes: bool, cx: &mut Context<Self>) {
        let Some(PendingAsk { ask, nonce, cmd }) = self.activities.ask.take() else { return };
        cx.notify(); // lint-allow(global-notify): the ask dialog is drawn by the root, over the columns
        let Some(frame) = self.activities.frame.clone() else { return };
        let answer = match (ask, yes) {
            (Ask::Link { .. }, false) => Ok(json!({ "opened": false })),
            (_, false) => Err((RPC_UNKNOWN, "You said no".to_owned())),
            (Ask::Link { url, domain, trust }, true) => {
                if trust && !self.prefs.trusted_link_domains.contains(&domain) {
                    self.prefs.trusted_link_domains.push(domain);
                    self.prefs.save();
                }
                cx.open_url(&url);
                Ok(json!({ "opened": true }))
            }
            (Ask::Share { url }, true) => {
                self.share_to_call_chat(url, cx);
                Ok(Value::Null)
            }
            // GAP: Discord's store page for an app's SKU isn't documented; this opens the app's own page in the
            // App Directory, where its store is. The purchase lands back here as an entitlement once paid.
            (Ask::Purchase, true) => {
                cx.open_url(&format!("https://discord.com/application-directory/{}/store", frame.read(cx).app_id));
                Err((RPC_UNKNOWN, "The purchase continues in the browser".to_owned()))
            }
        };
        frame.read(cx).reply(nonce, &cmd, answer);
    }

    pub(super) fn share_to_call_chat(&mut self, url: String, cx: &mut Context<Self>) {
        let (Some(client), Some(place)) = (self.client.clone(), self.game_place()) else { return };
        let channel_id = place.channel.id.clone();
        let nonce = crate::nonce();
        cx.spawn(async move |this, cx| {
            if let Err(error) = client.send_message(&channel_id, &url, &nonce, None, false, Default::default()).await {
                _ = this.update(cx, |this, cx| this.show_toast(format!("Couldn't share it: {error}"), cx));
            }
        })
        .detach();
    }

    pub(super) fn toggle_link_trust(&mut self, cx: &mut Context<Self>) {
        if let Some(PendingAsk { ask: Ask::Link { trust, .. }, .. }) = &mut self.activities.ask {
            *trust = !*trust;
        }
        cx.notify(); // lint-allow(global-notify): the ask dialog is drawn by the root, over the columns
    }

    // An app's picture to share: you pick an image, it goes up as the app's attachment, the app gets its URL.
    pub(super) fn upload_for_activity(&mut self, frame: Entity<ActivityFrame>, nonce: Value, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let app_id = frame.read(cx).app_id.clone();
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        cx.spawn(async move |this, cx| {
            let picked = paths.await.ok().and_then(Result::ok).flatten().and_then(|p| p.into_iter().next());
            let result = match picked {
                None => Err((RPC_UNKNOWN, "No image picked".to_owned())),
                Some(path) => {
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "image.png".into());
                    match std::fs::read(&path) {
                        Err(e) => Err((RPC_UNKNOWN, e.to_string())),
                        Ok(bytes) => client
                            .call_form(format!("/applications/{app_id}/attachment"), Vec::new(), name, bytes)
                            .await
                            .and_then(|res| res["attachment"]["url"].as_str().map(|url| json!({ "image_url": url })).ok_or_else(|| "No URL came back".into()))
                            .map_err(|e| (RPC_UNKNOWN, e)),
                    }
                }
            };
            _ = this.update(cx, |_, cx| frame.read(cx).reply(nonce, "INITIATE_IMAGE_UPLOAD", result));
        })
        .detach();
    }

    // Friends, for apps granted relationships.read. Discord's own RPC answers the same list.
    pub(super) fn sdk_relationships(&self) -> Vec<Value> {
        self.relationships
            .iter()
            .map(|r| {
                let kind = match r.kind {
                    discord::Relation::Friend => 1,
                    discord::Relation::Blocked => 2,
                    discord::Relation::Incoming => 3,
                    discord::Relation::Outgoing => 4,
                };
                json!({ "type": kind, "user": sdk_user(&r.user) })
            })
            .collect()
    }

    // The app hears who's talking in its channel, as on Discord.
    pub(crate) fn activity_speaking(&self, user_id: &str, speaking: bool, cx: &mut Context<Self>) {
        let (Some(frame), Some(call)) = (&self.activities.frame, &self.call) else { return };
        let evt = if speaking { "SPEAKING_START" } else { "SPEAKING_STOP" };
        frame.read(cx).dispatch(evt, json!({ "channel_id": call.channel.id, "user_id": user_id }));
    }

    // Someone in the app's channel muted, deafened, joined or left.
    pub(crate) fn activity_voice_state(&self, state: &discord::VoiceState, cx: &mut Context<Self>) {
        let (Some(frame), Some(call)) = (&self.activities.frame, &self.call) else { return };
        if state.channel_id.as_ref() != Some(&call.channel.id) {
            return;
        }
        frame.read(cx).dispatch("VOICE_STATE_UPDATE", sdk_voice_state(state, &self.player(&state.user_id)));
    }

    // Waffle's dialog for what the app asked: who asks, exactly what happens, and the way out first.
    pub(crate) fn render_activity_ask(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pending = self.activities.ask.as_ref()?;
        let app = self.activities.frame.as_ref().map(|f| self.app_name(&f.read(cx).app_id)).unwrap_or_else(|| "The activity".into());
        let (title, body, no, yes): (String, AnyElement, &str, &str) = match &pending.ask {
            Ask::Link { url, domain, trust } => (
                format!("{app} wants to open a link"),
                ui::col()
                    .gap_3()
                    .child(div().p_3().squircle(RADIUS_MD).bg(palette::surface(palette::Level::Page, cx)).text_size(TEXT_SM).text_color(palette::strong(cx)).child(url.clone()))
                    .child(ui::pick_row("ask-trust", ui::check(if *trust { ui::Check::On } else { ui::Check::Off }, cx), format!("Open {domain} links without asking"), None, cx).on_click(cx.listener(|this, _, _, cx| this.toggle_link_trust(cx))))
                    .into_any_element(),
                "Go back",
                "Open link",
            ),
            Ask::Share { url } => (
                "Share this moment?".to_owned(),
                ui::col()
                    .gap_3()
                    .child(gpui_kit::img(url.clone()).w_full().h(px(SHARE_PREVIEW)).squircle(RADIUS_MD).object_fit(gpui_kit::ObjectFit::Contain).bg(palette::surface(palette::Level::Page, cx)))
                    .child(div().text_size(TEXT_SM).text_color(palette::muted(cx)).child(format!("{app} made this. It goes to {}'s chat.", self.game_place().as_ref().map(|c| c.channel.name.clone()).unwrap_or_default())))
                    .into_any_element(),
                "Cancel",
                "Send",
            ),
            Ask::Purchase => (
                format!("Buy something in {app}?"),
                ui::callout(ui::Notice::Info, Glyph::Info, "Waffle can't take payments", "Finish in your browser on discord.com; it shows up here once it's paid.", cx).into_any_element(),
                "Not now",
                "Continue in browser",
            ),
        };
        let body = ui::dialog_body()
            .child(div().text_size(TEXT_LG).font_weight(gpui_kit::FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(title))
            .child(body)
            .child(
                ui::row()
                    .justify_end()
                    .gap_2()
                    .child(ui::button("ask-no", Tone::Secondary, cx).child(no).on_click(cx.listener(|this, _, _, cx| this.answer_ask(false, cx))))
                    .child(ui::button("ask-yes", Tone::Primary, cx).child(yes).on_click(cx.listener(|this, _, _, cx| this.answer_ask(true, cx)))),
            );
        let backdrop = div().id("ask-backdrop").absolute().inset_0().occlude();
        Some(ui::modal(backdrop, ui::dialog("activity-ask", CONSENT_WIDTH, body, window, cx), cx))
    }

    pub(super) fn answer_consent(&mut self, allow: bool, cx: &mut Context<Self>) {
        let (Some(consent), Some(frame), Some(client)) = (self.activities.consent.take(), self.activities.frame.clone(), self.client.clone()) else { return };
        cx.notify(); // lint-allow(global-notify): the consent dialog is drawn by the root, over the columns
        if !allow {
            return frame.read(cx).reply(consent.nonce, "AUTHORIZE", Err((RPC_OAUTH_DENIED, "You declined".into())));
        }
        cx.spawn(async move |this, cx| {
            let code = client.oauth_authorize(&consent.request).await;
            _ = this.update(cx, |_, cx| frame.read(cx).reply(consent.nonce, "AUTHORIZE", code.map(|code| json!({ "code": code })).map_err(|e| (RPC_OAUTH_DENIED, e))));
        })
        .detach();
    }

    pub(super) fn authenticate_activity(&mut self, frame: Entity<ActivityFrame>, nonce: Value, args: &Value, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let token = args["access_token"].as_str().unwrap_or_default().to_owned();
        cx.spawn(async move |this, cx| {
            let me = client.oauth_me(&token).await.map(|mut me| {
                me["access_token"] = Value::String(token);
                me
            });
            _ = this.update(cx, |_, cx| frame.read(cx).reply(nonce, "AUTHENTICATE", me.map_err(|e| (RPC_OAUTH_DENIED, e))));
        })
        .detach();
    }

    pub(super) fn sdk_participants(&self, instance: &ActivityInstance) -> Vec<Value> {
        instance.participants.iter().map(|p| sdk_user(&self.player(&p.user_id))).collect()
    }

    // The game's channel as the SDK reads it: a voice channel with who's in it, or the chat it runs over.
    pub(super) fn sdk_channel(&self) -> Option<Value> {
        let place = self.game_place()?;
        let voice_states: Vec<Value> = match place.voice {
            true => self.participants().into_iter().map(|s| sdk_voice_state(&s, &s.user.clone().unwrap_or_else(|| self.player(&s.user_id)))).collect(),
            false => Vec::new(),
        };
        // Discord's own channel type, as the payload gave it.
        let kind = place.channel.raw_type;
        Some(json!({ "id": place.channel.id, "type": kind, "guild_id": place.guild_id, "name": place.channel.name, "voice_states": voice_states, "messages": [] }))
    }

    // Discord's consent screen, in Waffle's words: who asks, and what they'd get.
    pub(crate) fn render_activity_consent(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let consent = self.activities.consent.as_ref()?;
        let request = &consent.request;
        let scopes = request.scopes.iter().map(|s| {
            ui::row().gap_2().child(Icon::new(Glyph::Checkmark).size(ICON_MD).text_color(palette::positive_ink(cx))).child(div().flex_1().min_w_0().child(scope_text(s)))
        });
        let body = ui::dialog_body()
            .child(div().text_size(TEXT_LG).font_weight(gpui_kit::FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(format!("Sign in to {}?", request.app_name)))
            .child(div().text_size(TEXT_SM).text_color(palette::muted(cx)).child("It's asking to use your Discord account. It will be able to:"))
            .child(ui::col().gap_2().text_size(TEXT_SM).text_color(palette::text(cx)).children(scopes))
            .child(
                ui::row()
                    .justify_end()
                    .gap_2()
                    .child(ui::button("consent-cancel", Tone::Secondary, cx).child("Cancel").on_click(cx.listener(|this, _, _, cx| this.answer_consent(false, cx))))
                    .child(ui::button("consent-allow", Tone::Primary, cx).child("Authorize").on_click(cx.listener(|this, _, _, cx| this.answer_consent(true, cx)))),
            );
        let backdrop = div().id("consent-backdrop").absolute().inset_0().occlude();
        Some(ui::modal(backdrop, ui::dialog("activity-consent", CONSENT_WIDTH, body, window, cx), cx))
    }
    // The app's own lines (Hole 3 of 9, a party size) join the entry; its name, app and start stay ours.
    pub(super) fn set_activity_presence(&mut self, activity: &Value) {
        let Some(Value::Object(ours)) = &mut self.activities.presence else { return };
        for key in ["details", "state", "assets", "party", "timestamps"] {
            match &activity[key] {
                Value::Null if key == "timestamps" => {}
                Value::Null => _ = ours.remove(key),
                value => _ = ours.insert(key.to_owned(), value.clone()),
            }
        }
        self.publish_presence();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatar_hash_from_cdn_urls() {
        assert_eq!(avatar_hash("https://cdn.discordapp.com/avatars/1/abc123.png?size=64"), Some("abc123"));
        assert_eq!(avatar_hash("https://cdn.discordapp.com/avatars/1/a_ff.gif"), Some("a_ff"));
        assert_eq!(avatar_hash("https://cdn.discordapp.com/embed/avatars/3.png"), None);
    }
}
