//! Signing in and handling gateway events.

use crate::*;

impl Waffle {
    pub(crate) fn connect(&mut self, token: String, cx: &mut Context<Self>) {
        self.splash_stage(0, "Signing in…", cx);
        let (client, mut events) = discord::connect(token.clone());
        self.token = Some(token);
        self.client = Some(client);
        self.watch_games(cx);
        self.status = Status::Connecting;
        self._session = Some(cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this.update(cx, |this, cx| this.on_event(event, cx)).is_err() {
                    break;
                }
            }
        }));
        // lint-allow(global-notify): connecting changes the status every column and the splash draw
        cx.notify();
    }
    // The gateway closed with 4004. Only a token the REST API also refuses is forgotten (that sends you to
    // sign in); anything else, a network error included, reconnects with the same token.
    pub(crate) fn verify_rejected_token(&mut self, cx: &mut Context<Self>) {
        let (Some(client), Some(token)) = (self.client.clone(), self.token.clone()) else { return self.sign_in(cx) };
        // A second 4004 before any READY: the token reads fine over REST but the gateway still refuses it.
        if std::mem::replace(&mut self.auth_retried, true) {
            self.forget_rejected_account();
            return self.sign_in(cx);
        }
        self.status = Status::Reconnecting("Checking your sign-in…".into());
        cx.spawn(async move |this, cx| {
            let refused = client.call(reqwest::Method::GET, "/users/@me".into(), None).await.err().is_some_and(|e| crate::discord::is_unauthorized(&e));
            this.update(cx, |this, cx| match refused {
                true => {
                    eprintln!("gateway: token refused (4004 and REST 401), signing out");
                    this.forget_rejected_account();
                    this.sign_in(cx);
                }
                false => {
                    eprintln!("gateway: 4004 but the token still works, reconnecting");
                    this.connect(token, cx);
                }
            })
            .ok();
        })
        // lint-allow(detached-task): one REST check of the token, then it ends
        .detach();
    }
    pub(crate) fn sign_in(&mut self, cx: &mut Context<Self>) {
        // Signing in happens in the main window, so the splash steps aside.
        self.finish_splash(cx);
        self.client = None;
        self.token = None;
        self.me = None;
        self.channel = None;
        self.guilds.clear();
        self.dms.clear();
        self.dm_requests.clear();
        self.feeds.clear();
        self.settings = None;
        self.status = Status::SignedOut(Login::Preparing);
        let mut events = auth::sign_in();
        // Replacing the task drops the gateway's receiver, which stops the old session.
        self._session = Some(cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this.update(cx, |this, cx| this.on_auth(event, cx)).is_err() {
                    break;
                }
            }
        }));
        // lint-allow(global-notify): starting a sign-in swaps the whole window to the login screen
        cx.notify();
    }
    pub(crate) fn on_auth(&mut self, event: auth::AuthEvent, cx: &mut Context<Self>) {
        let login = match event {
            auth::AuthEvent::SignedIn(token) => return self.connect(token, cx),
            auth::AuthEvent::Qr(url) => Login::qr(&url),
            auth::AuthEvent::Scanned(user) => Login::Scanned(user),
            auth::AuthEvent::Captcha(url) => {
                cx.open_url(&url);
                Login::Captcha(url.into())
            }
            auth::AuthEvent::Failed(error) => {
                // A failed sign-in may be ours to fix, and a build that can't sign in can't be updated from inside:
                // look for a newer Waffle now and fetch it straight away, whatever the automatic-download setting.
                // Download checks first when nothing is known; one already on its way or ready isn't fetched again.
                use crate::updater::{Command, Status as Update};
                if let Some(updater) = &self.updates.updater
                    && !matches!(self.updates.status, Update::Checking | Update::Downloading { .. } | Update::Ready(_))
                {
                    updater.send(Command::Download);
                }
                Login::Failed(error.into())
            }
        };
        self.status = Status::SignedOut(login);
        // lint-allow(global-notify): a sign-in step changes the login screen, which replaces every column
        cx.notify();
    }
    pub fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::SettingsProto { proto, partial } => self.apply_settings_proto(&proto, partial, cx),
            Event::UserSettings(changed) => {
                self.account_settings.apply(&changed);
                self.developer_mode = self.account_settings.developer_mode;
                // lint-allow(global-notify): account settings (developer mode and the rest) change menus and rows in every column
                cx.notify();
            }
            Event::Ready(ready) => {
                let ready = *ready;
                self.channel_info_requested.clear();
                self.command_picker.mark_indexes_stale();
                self.auth_retried = false;
                // Demo scenes set their own place; the last channel a real session saved would override it.
                if self.me.is_none() && std::env::var_os("WAFFLE_DEMO").is_none() {
                    self.restore = home::load_last_channel(&ready.me.id);
                }
                self.my_status = ready.status;
                self.remember_account(&ready.me);
                self.gateway_session = ready.session_id.clone();
                self.fetch_saved_ids(cx);
                self.load_reaction_frecency(cx);
                self.me = Some(ready.me);
                self.custom_status = ready.custom_status;
                self.expire_custom_status_later(cx);
                // The identify went out with no activities; a game already running goes back in.
                if self.games.mine.is_some() {
                    self.publish_presence();
                }
                for channel in ready.guilds.iter().flat_map(|g| &g.channels).chain(&ready.dms) {
                    if let Some(id) = channel.last_message_id.as_deref().and_then(|id| id.parse().ok()) {
                        self.last_message.insert(channel.id.clone(), id);
                    }
                }
                if cfg!(debug_assertions) && std::env::var_os("WAFFLE_STREAM_DEBUG").is_some() {
                    for guild in &ready.guilds {
                        let (voice, live) = (guild.voice_states.len(), guild.voice_states.iter().filter(|v| v.streaming).count());
                        if voice > 0 {
                            eprintln!("ready guild {}: {voice} in voice, {live} streaming", guild.id);
                        }
                    }
                }
                // Streams already live when we connect: their start is unknown, so count from now.
                for guild in &ready.guilds {
                    for state in guild.voice_states.iter().filter(|v| v.streaming) {
                        if let Some(channel) = &state.channel_id {
                            self.stream_live_since.entry(format!("guild:{}:{channel}:{}", guild.id, state.user_id)).or_insert_with(Instant::now);
                        }
                    }
                }
                self.guilds = ready.guilds;
                self.folders = ready.folders;
                (self.dm_requests, self.dms) = ready.dms.into_iter().partition(Channel::is_request);
                self.dms.sort_by_key(|c| std::cmp::Reverse(self.last_message.get(&c.id).copied().unwrap_or(0)));
                self.read_states = ready.read_states.into_iter().collect();
                self.relationships = ready.relationships;
                self.developer_mode = ready.developer_mode;
                self.account_settings = ready.account_settings;
                self.my_account = ready.my_account;
                self.guild_settings = ready.guild_settings.into_iter().collect();
                self.ignored = ready.ignored.into_iter().collect();
                self.presences = ready.presences.into_iter().collect();
                self.status = Status::Online;
                self.load_activity_feed(cx);
                self.splash_ready(cx);
                self.rejoin_after_restart(cx);
                self.load_favorites(cx);
                self.reconcile_ready(cx);
            }
            Event::Batch(events) => {
                for event in events {
                    self.on_event(event, cx);
                }
            }
            Event::ChannelUpsert { guild_id, channel } => {
                let Some(guild) = self.guilds.iter_mut().find(|g| g.id == guild_id) else { return };
                let list = if channel.kind == ChannelKind::Thread { &mut guild.threads } else { &mut guild.channels };
                match list.iter_mut().find(|c| c.id == channel.id) {
                    Some(existing) => {
                        // THREAD_UPDATE carries no membership: keep whether we'd joined.
                        let joined = existing.thread.as_ref().is_some_and(|t| t.joined);
                        *existing = channel;
                        if let Some(thread) = existing.thread.as_mut() {
                            thread.joined |= joined;
                        }
                    }
                    None => list.push(channel),
                }
                guild.channels.sort_by_key(|c| c.position);
            }
            Event::ScheduledEvent(event) => {
                let Some(guild) = self.guilds.iter_mut().find(|g| g.id == event.guild_id) else { return };
                match guild.events.iter_mut().find(|e| e.id == event.id) {
                    // Updates carry neither the counts nor our own Interested: keep what we know.
                    Some(existing) => *existing = discord::ScheduledEvent { interested: existing.interested, me_interested: existing.me_interested, creator: event.creator.clone().or(existing.creator.take()), ..event },
                    None => guild.events.push(event),
                }
            }
            Event::ScheduledEventDelete { guild_id, id } => {
                if let Some(guild) = self.guilds.iter_mut().find(|g| g.id == guild_id) {
                    guild.events.retain(|e| e.id != id);
                }
            }
            Event::ScheduledEventException { guild_id, event_id, id, exception } => {
                let event = self.guilds.iter_mut().filter(|g| g.id == guild_id).flat_map(|g| g.events.iter_mut()).find(|e| e.id == event_id);
                if let Some(event) = event {
                    event.exceptions.retain(|x| x.id != id);
                    event.exceptions.extend(exception);
                }
            }
            Event::ScheduledEventUser { guild_id, event_id, user_id, added } => {
                let mine = self.me.as_ref().is_some_and(|me| me.id == user_id);
                // Our own Interested echoes back after the optimistic toggle already counted it.
                let echo = |e: &discord::ScheduledEvent| mine && e.me_interested == added;
                if let Some(event) = self.guilds.iter_mut().filter(|g| g.id == guild_id).flat_map(|g| g.events.iter_mut()).find(|e| e.id == event_id && !echo(e)) {
                    event.interested = if added { event.interested + 1 } else { event.interested.saturating_sub(1) };
                    if mine {
                        event.me_interested = added;
                    }
                }
            }
            Event::ForumUnreads { forum_id, counts } => self.on_forum_unreads(&forum_id, counts),
            Event::ForumPostCreated { forum_id, post_id, owner_id } => self.on_forum_post_created(&forum_id, post_id, &owner_id),
            Event::ChannelDelete { guild_id, id } => {
                if let Some(guild) = self.guilds.iter_mut().find(|g| g.id == guild_id) {
                    guild.channels.retain(|c| c.id != id);
                    guild.threads.retain(|c| c.id != id);
                }
                if self.channel.as_ref().is_some_and(|c| c.id == id) {
                    self.channel = None;
                }
            }
            Event::MemberUpdate { guild_id, user, roles, timed_out_until, membership } => {
                self.profile_changed(&user.id);
                if self.me.as_ref().is_some_and(|me| me.id == user.id)
                    && let Some(guild) = self.guilds.iter_mut().find(|g| g.id == guild_id)
                {
                    guild.my_roles = roles.clone();
                    guild.timed_out_until = timed_out_until;
                    guild.me = membership;
                }
                self.member_roles.entry(guild_id.clone()).or_default().insert(user.id.clone(), roles);
                self.members.entry(guild_id.clone()).or_default().insert(user.id.clone(), user);
                self.apply_members(&guild_id);
            }
            Event::RelationshipChange { user_id, relationship } => {
                // An update may arrive without the user object; keep the one we have.
                let known = self.relationships.iter().find(|r| r.user.id == user_id).map(|r| r.user.clone());
                self.relationships.retain(|r| r.user.id != user_id);
                if let Some(mut relationship) = relationship {
                    if relationship.user.name.is_empty()
                        && let Some(user) = known
                    {
                        relationship.user = user;
                    }
                    self.relationships.push(relationship);
                }
            }
            Event::DmUpsert(dm) => self.upsert_dm(dm, cx),
            Event::DmDelete(id) => {
                self.dms.retain(|d| d.id != id);
                self.dm_requests.retain(|d| d.id != id);
                if self.channel.as_ref().is_some_and(|c| c.id == id) {
                    self.open_home(cx);
                }
            }
            Event::RecipientChange { channel_id, user, added } => {
                if !added && self.me.as_ref().is_some_and(|me| me.id == user.id) {
                    return self.on_event(Event::DmDelete(channel_id), cx);
                }
                let open = self.channel.as_mut().filter(|c| c.id == channel_id);
                for dm in self.dms.iter_mut().filter(|d| d.id == channel_id).chain(open) {
                    dm.recipients.retain(|r| r.id != user.id);
                    if added {
                        dm.recipients.push(user.clone());
                    }
                }
            }
            Event::ThreadJoined { id, joined } => {
                for thread in self.guilds.iter_mut().flat_map(|g| g.threads.iter_mut()).filter(|t| t.id == id) {
                    if let Some(meta) = thread.thread.as_mut() {
                        meta.joined = joined;
                    }
                }
            }
            Event::GuildSettings((key, settings)) => {
                self.guild_settings.insert(key, settings);
            }
            Event::GuildCreate(mut guild) => {
                let joined = self.joining.take_if(|id| *id == guild.id);
                // GUILD_CREATE carries no member entry for us: keep the roles we know (READY, or
                // GUILD_MEMBER_UPDATE), or take them from the member cache, so permissions hold.
                let me = self.me.as_ref().map(|m| m.id.clone()).unwrap_or_default();
                let known_roles = self.member_roles.get(&guild.id).and_then(|r| r.get(&me)).cloned();
                match self.guilds.iter_mut().find(|g| g.id == guild.id) {
                    Some(existing) => {
                        if guild.my_roles.is_empty() {
                            guild.my_roles = std::mem::take(&mut existing.my_roles);
                        }
                        guild.timed_out_until = guild.timed_out_until.or(existing.timed_out_until);
                        // Same for our membership state: GUILD_CREATE doesn't carry it.
                        guild.me = std::mem::take(&mut existing.me);
                        for thread in existing.threads.drain(..) {
                            if !guild.threads.iter().any(|t| t.id == thread.id) {
                                guild.threads.push(thread);
                            }
                        }
                        *existing = guild;
                    }
                    None => {
                        if guild.my_roles.is_empty() {
                            guild.my_roles = known_roles.unwrap_or_default();
                        }
                        self.guilds.push(guild);
                    }
                }
                if let Some(id) = joined {
                    self.open_joined_guild(id, cx);
                }
            }
            Event::MessageCreate(message) => {
                self.watch_deferred(&message.id, message.flags, cx);
                self.offer_publish_bump(&message);
                self.on_message(*message, cx);
            }
            Event::MessageUpdate(patch) => {
                let (channel_id, id) = (patch["channel_id"].as_str().unwrap_or_default(), patch["id"].as_str().unwrap_or_default());
                if let Some(m) = self.message_mut(channel_id, id) {
                    discord::apply_update(m, &patch);
                }
                if let Some(flags) = self.message_mut(channel_id, id).map(|m| m.flags) {
                    self.watch_deferred(id, flags, cx);
                }
            }
            Event::MessageDelete { channel_id, id } => self.remove_message(&channel_id, &id),
            Event::Reaction { channel_id, message_id, user_id, emoji, added } => {
                let mine = self.me.as_ref().is_some_and(|me| me.id == user_id);
                self.reactors_changed(&message_id, &emoji, &user_id, added);
                if let Some(m) = self.message_mut(&channel_id, &message_id) {
                    adjust_reaction(m, &emoji, added, mine, true);
                }
            }
            Event::PollVote { channel_id, message_id, user_id, answer_id, added } => {
                let mine = self.me.as_ref().is_some_and(|me| me.id == user_id);
                if let Some(poll) = self.message_mut(&channel_id, &message_id).and_then(|m| m.poll.as_mut()) {
                    poll::apply_vote(poll, answer_id, added, mine);
                }
            }
            Event::Typing { channel_id, user_id, name } => self.on_typing(channel_id, user_id, name, cx),
            Event::Presence { user_id, presence } => {
                self.presences.insert(user_id, presence);
            }
            Event::MemberList { guild_id, groups, ops } => {
                if !groups.is_empty() {
                    self.member_groups.insert(guild_id.clone(), groups.into_iter().collect());
                }
                let list = self.member_lists.entry(guild_id.clone()).or_default();
                for op in ops {
                    apply_list_op(list, op);
                }
                let roles = self.member_roles.entry(guild_id.clone()).or_default();
                let known = self.members.entry(guild_id.clone()).or_default();
                for item in list.iter() {
                    if let ListItem::Member(m) = item {
                        known.insert(m.user.id.clone(), m.user.clone());
                        roles.insert(m.user.id.clone(), m.roles.clone());
                        self.presences.insert(m.user.id.clone(), m.presence.clone());
                    }
                }
                self.apply_members(&guild_id);
            }
            Event::MembersChunk { guild_id, members, facts } => {
                let roles = self.member_roles.entry(guild_id.clone()).or_default();
                let known = self.members.entry(guild_id.clone()).or_default();
                for (user, member_roles) in members {
                    roles.insert(user.id.clone(), member_roles);
                    known.insert(user.id.clone(), user);
                }
                self.apply_members(&guild_id);
                self.mod_view_members(&guild_id, facts);
            }
            Event::Ack { channel_id, message_id, manual: Some(mentions) } => {
                if self.channel.as_ref().is_some_and(|c| c.id == channel_id) {
                    self.unread_marker = Some(message_id);
                }
                self.read_states.insert(channel_id, ReadState { last_read: message_id, mentions });
            }
            Event::Ack { channel_id, message_id, manual: None } => {
                let read = self.read_states.entry(channel_id).or_default();
                read.last_read = read.last_read.max(message_id);
                read.mentions = 0;
            }
            Event::VoiceState(state, session) => {
                self.activity_voice_state(&state, cx);
                let ours = self.call.as_ref().map(|c| c.channel.id.clone());
                let me = self.me.as_ref().map(|m| m.id.clone());
                if Some(&state.user_id) == me.as_ref() && state.channel_id.is_some() && session.is_some_and(|s| s != self.gateway_session) {
                    self.taken_elsewhere(state.guild_id.as_deref());
                }
                // Someone answering our ring joins the DM call: the ringtone stops.
                if state.channel_id.is_some() && state.channel_id == self.ringing_out && Some(&state.user_id) != me.as_ref() {
                    self.stop_ringing_out();
                }
                let mine = Some(&state.user_id) == me.as_ref();
                // Answered on another device: this one stops ringing too.
                if mine && state.channel_id.is_some() && state.channel_id == self.ringing_in {
                    self.stop_ringing_in();
                }
                let states = match &state.guild_id {
                    Some(id) => match self.guilds.iter_mut().find(|g| &g.id == id) {
                        Some(guild) => &mut guild.voice_states,
                        None => return,
                    },
                    None => &mut self.dm_voice,
                };
                let was_here = states.iter().any(|v| v.user_id == state.user_id && v.channel_id == ours);
                let is_here = ours.is_some() && state.channel_id == ours;
                let was_live = states.iter().any(|v| v.user_id == state.user_id && v.channel_id == ours && v.streaming);
                let is_live = is_here && state.streaming;
                let silent = self.call.as_ref().is_some_and(|c| c.channel.raw_type == 13);
                if let Some(cue) = sounds::member_cue(mine, was_here, is_here).filter(|_| !silent) {
                    sounds::play(cue);
                } else if !mine && !was_live && is_live {
                    sounds::play(sounds::Cue::StreamStarted);
                } else if was_live && !is_live && self.call.as_ref().is_some_and(|c| c.watched(&state.user_id).is_some()) {
                    sounds::play(sounds::Cue::StreamEnded);
                }
                let live_key = state.channel_id.as_ref().filter(|_| state.streaming).map(|c| match &state.guild_id {
                    Some(guild) => format!("guild:{guild}:{c}:{}", state.user_id),
                    None => format!("call:{c}:{}", state.user_id),
                });
                let suffix = format!(":{}", state.user_id);
                self.stream_live_since.retain(|key, _| !key.ends_with(&suffix) || Some(key) == live_key.as_ref());
                self.stream_paused.retain(|key| !key.ends_with(&suffix) || Some(key) == live_key.as_ref());
                if let Some(key) = live_key {
                    self.stream_live_since.entry(key).or_insert_with(Instant::now);
                }
                upsert_voice_state(states, state);
            }
            Event::Call { channel_id, ringing } => self.on_dm_call(channel_id, ringing, cx),
            Event::CallEnded { channel_id } => self.on_dm_call_ended(&channel_id),
            Event::VoiceServer(_) => {}
            Event::ActivityInstance(instance) => self.on_activity_instance(instance, cx),
            Event::VoiceEffect(effect) => self.on_voice_effect(effect, cx),
            Event::SoundboardSounds { guild_id, sounds, all } => self.soundboard.store_sounds(guild_id, sounds, all),
            Event::SoundboardSoundDelete { guild_id, sound_id } => self.soundboard.remove_sound(&guild_id, &sound_id),
            Event::StageInstance { guild_id, channel_id, instance } => {
                if let Some(guild) = self.guilds.iter_mut().find(|g| g.id == guild_id) {
                    guild.stage_instances.retain(|s| s.channel_id != channel_id);
                    guild.stage_instances.extend(instance);
                }
            }
            Event::VoiceStartTimes { guild_id, times, all } => {
                if all
                    && let Some(guild) = self.guilds.iter().find(|g| g.id == guild_id)
                {
                    for channel in &guild.channels {
                        self.voice_start.remove(&channel.id);
                    }
                }
                for (channel, start) in times {
                    match start {
                        Some(start) => self.voice_start.insert(channel, start),
                        None => self.voice_start.remove(&channel),
                    };
                }
            }
            Event::VoiceStatuses { guild_id, statuses, all } => self.on_voice_statuses(&guild_id, statuses, all, cx),
            Event::StreamUpdate { key, viewers, paused } => {
                match paused {
                    true => self.stream_paused.insert(key.clone()),
                    false => self.stream_paused.remove(&key),
                };
                let before = self.stream_viewers.get(&key).map_or(0, Vec::len);
                if self.in_stream(&key)
                    && let Some(cue) = sounds::viewer_cue(before, viewers.len())
                {
                    sounds::play(cue);
                }
                self.stream_viewers.insert(key, viewers);
            }
            Event::Disconnected(reason) => {
                self.splash_stage(0, "Reconnecting…", cx);
                self.status = Status::Reconnecting(reason.into());
            }
            Event::Resumed => {
                self.status = Status::Online;
                self.command_picker.mark_indexes_stale();
            }
            Event::AuthFailed => return self.verify_rejected_token(cx),
            Event::InteractionCreate { nonce, id } => self.on_interaction_create(nonce, id, cx),
            Event::InteractionSuccess { nonce } => self.on_interaction_success(nonce, cx),
            Event::InteractionFailure { nonce, reason_code } => {
                self.autocomplete_failed(&nonce, cx);
                self.on_interaction_failed(&nonce, reason_code.unwrap_or(0), cx);
                self.interaction_failed(&nonce, reason_code, cx);
            }
            Event::ModalCreate(modal) => self.open_app_modal(modal, cx),
            Event::Autocomplete { nonce, choices } => self.on_autocomplete(nonce, choices, cx),
        }
        self.resolve_voice_users();
        self.refresh_stream_previews(cx);
        self.refresh_avatar_colors(cx);
        self.update_badge();
        // lint-allow(global-notify): a gateway event can change the servers, channels, feed, members and calls that every column draws
        cx.notify();
    }
    // READY's voice states carry only user ids: fill them from members we know, and ask the
    // gateway once for the rest so every voice row and tile shows the real name and avatar.
    pub(crate) fn resolve_voice_users(&mut self) {
        let known: Vec<User> = self.known_users().into_iter().chain(self.me.clone()).collect();
        for state in self.dm_voice.iter_mut().filter(|v| v.user.is_none()) {
            state.user = known.iter().find(|u| u.id == state.user_id).cloned();
        }
        let mut missing: Vec<(String, String)> = Vec::new();
        for guild in &mut self.guilds {
            let known = self.members.get(&guild.id);
            for state in guild.voice_states.iter_mut().filter(|v| v.user.is_none()) {
                match known.and_then(|k| k.get(&state.user_id)) {
                    Some(user) => state.user = Some(user.clone()),
                    None if self.requested_members.insert((guild.id.clone(), state.user_id.clone())) => missing.push((guild.id.clone(), state.user_id.clone())),
                    None => {}
                }
            }
        }
        let Some(client) = &self.client else { return };
        let mut by_guild: HashMap<String, Vec<String>> = HashMap::new();
        for (guild_id, user_id) in missing {
            by_guild.entry(guild_id).or_default().push(user_id);
        }
        for (guild_id, ids) in by_guild {
            for chunk in ids.chunks(100) {
                client.request_members(&guild_id, chunk.to_vec());
            }
        }
    }
    pub(crate) fn on_message(&mut self, message: Message, cx: &mut Context<Self>) {
        self.note_message_sent(&message, cx);
        let channel_id = message.channel_id.clone();
        let mine = self.me.as_ref().is_some_and(|me| me.id == message.author.id);
        let is_open = self.channel.as_ref().is_some_and(|c| c.id == channel_id);
        if let Ok(id) = message.id.parse() {
            self.last_message.insert(channel_id.clone(), id);
            // Sending reads the channel up to our message, as Discord's server does; without it our own
            // message would mark the DM unread and raise its tile in the rail.
            if mine {
                self.read_states.entry(channel_id.clone()).or_default().last_read = id;
                // Posting reads the channel, and the NEW line goes, as in Discord.
                if is_open {
                    self.unread_marker = None;
                }
            }
        }
        if let (Some(guild_id), Some(roles)) = (&message.guild_id, &message.roles) {
            self.member_roles.entry(guild_id.clone()).or_default().insert(message.author.id.clone(), roles.clone());
        }
        if let Some(typers) = self.typing.get_mut(&channel_id) {
            typers.retain(|t| t.user_id != message.author.id);
        }
        let own_send = self.feeds.get(&channel_id).is_some_and(|f| f.messages.iter().any(|m| m.state != SendState::Sent && m.nonce.is_some() && m.nonce == message.nonce));
        self.note_call_message(&message, own_send, cx);
        let is_dm = self.dms.iter().any(|d| d.id == channel_id);
        let hidden = self.hidden_author(&message.author.id).is_some();
        if !mine && !is_open && !hidden && (self.mentions_me(&message) || is_dm) {
            self.read_states.entry(channel_id.clone()).or_default().mentions += 1;
        }
        self.speak_tts(&message, is_open, mine);
        let kind = self.notify_kind(&message, is_open && self.window_active);
        if kind != notify::Notify::None {
            sounds::play(if is_dm { sounds::Cue::Message } else { sounds::Cue::Mention });
            if !self.window_active || !is_open {
                self.notify(&message);
            }
        }
        if let Some(pos) = self.dms.iter().position(|d| d.id == channel_id) {
            let dm = self.dms.remove(pos);
            self.dms.insert(0, dm);
        }
        // An app's reply to our command takes its "is thinking…" stand-in's place.
        let nonce = self.interaction_reply_nonce(&message).or_else(|| message.nonce.clone());
        let Some(feed) = self.feeds.get_mut(&channel_id) else { return };
        // Our own optimistic copy, sent with this nonce: swap in the real one in place.
        if let Some(pending) = Arc::make_mut(&mut feed.messages).iter_mut().find(|m| m.state != SendState::Sent && m.nonce.is_some() && m.nonce == nonce) {
            *pending = message;
            return;
        }
        if feed.messages.iter().any(|m| m.id == message.id) {
            return;
        }
        let at = feed.messages.len();
        Arc::make_mut(&mut feed.messages).push(message.clone());
        if is_open {
            self.arrivals.borrow_mut().fresh.insert(message.id);
            self.list.splice(at..at, 1);
            self.ack_open(cx);
        }
    }
    pub(crate) fn on_typing(&mut self, channel_id: String, user_id: String, name: Option<String>, cx: &mut Context<Self>) {
        if self.me.as_ref().is_some_and(|me| me.id == user_id) {
            return;
        }
        let name = name.unwrap_or_else(|| self.user_name(&user_id));
        let typers = self.typing.entry(channel_id).or_default();
        typers.retain(|t| t.user_id != user_id);
        typers.push(Typer { user_id, name, until: Instant::now() + TYPING_TTL });
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TYPING_TTL).await;
            // lint-allow(global-notify): the typing line shows in the main column, the call chat and the sidebar's DM rows
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        // lint-allow(detached-task): one TYPING_TTL timer, then it ends
        .detach();
    }
}
