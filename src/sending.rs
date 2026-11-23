//! Sending messages and files, edits and reactions.

use crate::*;

impl Waffle {
    /// Enter in the composer: saves the edit in progress, otherwise sends (optimistically). A mass ping
    /// first opens Hold Up!, unless it's `confirmed` there.
    pub(crate) fn submit(&mut self, confirmed: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.held_unread = None;
        let (Some(client), Some(channel), Some(me)) = (self.client.clone(), self.channel.clone(), self.me.clone()) else { return };
        let typed = wire_text(&self.composer.read(cx).content());
        let typed = match self.account_settings.convert_emoticons {
            true => emoticons::convert(&typed),
            false => typed,
        };
        let typed = typed.trim();
        if typed.is_empty() && self.pending_files.is_empty() {
            return;
        }
        if self.composer_guard().is_some() || (self.editing.is_none() && self.hold_for_slowmode(&channel.id, cx)) {
            return;
        }
        // An edit keeps its text as typed: Discord reads @silent and /tts only on a new message.
        let (content, options) = match self.editing {
            Some(_) => (typed.to_owned(), discord::SendOptions::default()),
            None => send_options::parse(typed, self.tts_allowed()),
        };
        let content = self.to_wire(&content);
        if !confirmed && self.editing.is_none() {
            self.hold_up = self.hold_up_for(&content);
            if self.hold_up.is_some() {
                return self.redraw_main(cx);
            }
        }
        self.hold_up = None;
        self.composer.update(cx, |state, cx| state.set_value("", window, cx));
        self.has_draft = false;
        self.send_error = None;
        self.last_typing_sent = None;
        if let Some(editing) = self.editing.take() {
            self.restore_draft(window, cx);
            return self.save_edit(editing, content, cx);
        }
        self.drafts.clear(&channel.id);
        self.save_drafts_soon(cx);
        // Over Discord's limit, the text goes as a message.txt instead of being refused.
        if content.chars().count() > discord::message_limit(self.my_account.premium_type) {
            match attachments::save_message_text(&content).ok().and_then(attachments::PendingFile::new) {
                Some(file) => {
                    self.pending_files.push(file);
                    return self.send_files(String::new(), options, cx);
                }
                None => {
                    self.send_error = Some("Couldn't turn the long message into a file".into());
                    return self.redraw_main(cx);
                }
            }
        }
        if !self.pending_files.is_empty() {
            return self.send_files(content, options, cx);
        }
        let nonce = nonce();
        let reply = self.replying.take();
        let ping = self.reply_ping;
        let pending = Message {
            id: format!("pending-{nonce}"),
            channel_id: channel.id.clone(),
            guild_id: self.guild_id.clone(),
            author: me,
            content: content.clone(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            reply_to: reply.as_ref().map(|r| r.id.clone()),
            reply: reply.clone().map(Box::new),
            nonce: Some(nonce.clone()),
            state: SendState::Sending,
            flags: options.flags(),
            tts: options.tts,
            ..Default::default()
        };
        self.push_local(pending, cx);
        let live = self.when_live();
        cx.spawn(async move |this, cx| {
            live.await;
            let result = client.send_message(&channel.id, &content, &nonce, reply.as_ref().map(|r| r.id.as_str()), ping, options).await;
            this.update(cx, |this, cx| this.settle(&channel.id, &nonce, result, cx)).ok();
        })
        // lint-allow(detached-task): one send request, then it ends
        .detach();
    }
    pub(crate) fn push_local(&mut self, message: Message, cx: &mut Context<Self>) {
        let is_open = self.channel.as_ref().is_some_and(|c| c.id == message.channel_id);
        let feed = self.feeds.entry(message.channel_id.clone()).or_default();
        let at = feed.messages.len();
        self.arrivals.borrow_mut().fresh.insert(message.id.clone());
        Arc::make_mut(&mut feed.messages).push(message);
        if is_open {
            self.list.splice(at..at, 1);
            self.list.scroll_to_end();
        }
        // lint-allow(global-notify): a new local message shows in the feed and can reorder the sidebar's DM list
        cx.notify();
    }
    // The REST reply and the gateway echo race; whichever lands first replaces the pending copy.
    pub(crate) fn settle(&mut self, channel_id: &str, nonce: &str, result: Result<Message, String>, cx: &mut Context<Self>) {
        // Detached, not dropped: settle runs inside that task.
        if let Some(task) = self.upload_tasks.remove(nonce) {
            // lint-allow(detached-task): settle runs inside that task, so it ends itself
            task.detach();
        }
        // Before the pending lookup: when the gateway echo landed first there is no pending copy left,
        // and the job (with its upload cards) would stay forever.
        if result.is_ok() {
            self.forget_upload(nonce);
            self.slowmode_sent(channel_id, cx);
        }
        let pending = self.feeds.get_mut(channel_id).and_then(|f| Arc::make_mut(&mut f.messages).iter_mut().find(|m| m.nonce.as_deref() == Some(nonce) && m.state != SendState::Sent));
        let Some(pending) = pending else { return };
        match result {
            Ok(sent) => *pending = sent,
            Err(error) => {
                pending.state = SendState::Failed;
                match self.uploads.get_mut(nonce) {
                    Some(job) => job.error = Some(error),
                    None => self.send_error = Some(format!("Couldn't send: {error}").into()),
                }
            }
        }
        self.redraw_main(cx);
    }
    pub(crate) fn retry(&mut self, message: Message, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let Some(nonce) = message.nonce.clone() else { return };
        if let Some(m) = self.message_mut(&message.channel_id, &message.id) {
            m.state = SendState::Sending;
        }
        self.send_error = None;
        if self.uploads.contains_key(&nonce) {
            return self.start_upload(nonce, cx);
        }
        cx.spawn(async move |this, cx| {
            let result = match (message.stickers.first(), poll::draft_of(&message)) {
                (Some(sticker), _) => client.send_sticker(&message.channel_id, &sticker.id, &nonce, message.reply_to.as_deref(), true).await,
                (None, Some(draft)) => client.send_poll(&message.channel_id, &draft, &nonce).await,
                (None, None) => client.send_message(&message.channel_id, &message.content, &nonce, message.reply_to.as_deref(), true, message.send_options()).await,
            };
            this.update(cx, |this, cx| this.settle(&message.channel_id, &nonce, result, cx)).ok();
        })
        // lint-allow(detached-task): one resend request, then it ends
        .detach();
        self.redraw_main(cx);
    }
    pub(crate) fn edit(&mut self, original: Message, content: String, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        if let Some(m) = self.message_mut(&original.channel_id, &original.id) {
            m.content = content.clone();
            m.edited = true;
        }
        cx.spawn(async move |this, cx| {
            if client.edit_message(&original.channel_id, &original.id, &content).await.is_ok() {
                return;
            }
            this.update(cx, |this, cx| {
                if let Some(m) = this.message_mut(&original.channel_id, &original.id) {
                    *m = original;
                }
                this.send_error = Some("Couldn't save your edit".into());
                this.redraw_main(cx);
            })
            .ok();
        })
        // lint-allow(detached-task): one edit request, then it ends
        .detach();
        self.redraw_main(cx);
    }
    pub(crate) fn delete(&mut self, message: Message, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        self.remove_message(&message.channel_id, &message.id);
        self.redraw_main(cx);
        if message.state != SendState::Sent {
            // Unsent: also how an upload is cancelled mid-flight.
            if let Some(nonce) = message.nonce.as_deref() {
                self.upload_tasks.remove(nonce);
                self.forget_upload(nonce);
            }
            return;
        }
        cx.spawn(async move |this, cx| {
            if client.delete_message(&message.channel_id, &message.id).await.is_ok() {
                return;
            }
            this.update(cx, |this, cx| {
                this.send_error = Some("Couldn't delete that message".into());
                this.fetch_latest(&message.channel_id, cx);
            })
            .ok();
        })
        // lint-allow(detached-task): one delete request, then it ends
        .detach();
    }
    pub(crate) fn toggle_reaction(&mut self, message: &Message, emoji: Emoji, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let add = !message.reactions.iter().any(|r| r.emoji == emoji && r.me);
        if add {
            self.update_prefs(cx, |p| p.record_emoji(&emoji));
            self.save_reaction_frecency(cx);
        }
        let (channel_id, message_id) = (message.channel_id.clone(), message.id.clone());
        let me = self.my_id().to_owned();
        self.reactors_changed(&message_id, &emoji, &me, add);
        if let Some(m) = self.message_mut(&channel_id, &message_id) {
            adjust_reaction(m, &emoji, add, true, false);
        }
        cx.spawn(async move |this, cx| {
            if client.react(&channel_id, &message_id, &emoji, add).await.is_ok() {
                return;
            }
            this.update(cx, |this, cx| {
                this.reactors_changed(&message_id, &emoji, &me, !add);
                if let Some(m) = this.message_mut(&channel_id, &message_id) {
                    adjust_reaction(m, &emoji, !add, true, false);
                }
                this.redraw_main(cx);
            })
            .ok();
        })
        // lint-allow(detached-task): one reaction request, then it ends
        .detach();
        self.redraw_main(cx);
    }
    /// The + button: pick files to attach; they wait in the composer until sent, as in Discord.
    pub(crate) fn upload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions { files: true, directories: false, multiple: true, prompt: None });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            this.update_in(cx, |this, window, cx| this.add_files(paths, window, cx)).ok();
        })
        // lint-allow(detached-task): one file prompt, then it ends
        .detach();
    }
    /// Picked, dropped or pasted files join the composer within Discord's caps, which say what they left out.
    pub(crate) fn add_files(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let files = paths.into_iter().filter_map(attachments::PendingFile::new).collect();
        let limit = discord::upload_limit(self.my_account.premium_type, self.guild().map_or(0, |g| g.premium_tier));
        self.send_error = attachments::admit(&mut self.pending_files, files, limit).map(Into::into);
        self.has_draft |= !self.pending_files.is_empty();
        self.load_previews(cx);
        self.composer.update(cx, |state, cx| state.focus(window, cx));
        self.redraw_main(cx);
    }
    // Tile pictures, drawn by the OS off the UI thread: images and videos as thumbnails where it can,
    // everything else (and any it can't) as the icon its file manager shows.
    pub(crate) fn load_previews(&mut self, cx: &mut Context<Self>) {
        const SIDE: u32 = 96;
        for file in &self.pending_files {
            if self.file_previews.contains_key(&file.path) {
                continue;
            }
            self.file_previews.insert(file.path.clone(), None);
            let (path, kind) = (file.path.clone(), file.kind());
            let picture = cx.background_spawn(async move {
                let thumb = (kind != attachments::FileKind::Other).then(|| os_image::thumbnail(&path, SIDE)).flatten();
                thumb.or_else(|| os_image::file_icon(&path, SIDE))
            });
            let path = file.path.clone();
            cx.spawn(async move |this, cx| {
                let picture = picture.await;
                this.update(cx, |this, cx| {
                    this.file_previews.insert(path, picture);
                    this.redraw_main(cx);
                })
                .ok();
            })
            // lint-allow(detached-task): one thumbnail job, then it ends
            .detach();
        }
    }
    /// Cmd/Ctrl+V: files copied in Finder or Explorer, a picture (a screenshot) or text over 4000
    /// characters (as message.txt) attach; other text is left to the composer's own paste. True when it
    /// attached something.
    pub(crate) fn paste_files(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(item) = cx.read_from_clipboard() else { return false };
        let paths = match attachments::classify(&item) {
            attachments::Pasted::Text => return false,
            attachments::Pasted::Files(paths) => paths,
            attachments::Pasted::LongText(text) => match attachments::save_message_text(&text) {
                Ok(path) => vec![path],
                Err(error) => {
                    self.send_error = Some(format!("Couldn't attach that text: {error}").into());
                    self.redraw_main(cx);
                    return true;
                }
            },
            attachments::Pasted::Image(image) => match attachments::save_pasted_image(&image) {
                Ok(path) => vec![path],
                Err(error) => {
                    self.send_error = Some(format!("Couldn't paste that image: {error}").into());
                    self.redraw_main(cx);
                    return true;
                }
            },
        };
        self.add_files(paths, window, cx);
        true
    }
    pub(crate) fn send_files(&mut self, content: String, options: discord::SendOptions, cx: &mut Context<Self>) {
        let (Some(channel), Some(me)) = (self.channel.clone(), self.me.clone()) else { return };
        // Previews stay until the upload settles: the feed's upload cards show them too.
        let files: Vec<discord::Upload> = std::mem::take(&mut self.pending_files).iter().map(attachments::PendingFile::upload).collect();
        let nonce = nonce();
        let reply = self.replying.take();
        let pending = Message {
            id: format!("pending-{nonce}"),
            channel_id: channel.id.clone(),
            guild_id: self.guild_id.clone(),
            author: me,
            content: content.clone(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            reply_to: reply.as_ref().map(|r| r.id.clone()),
            reply: reply.clone().map(Box::new),
            nonce: Some(nonce.clone()),
            state: SendState::Sending,
            flags: options.flags(),
            tts: options.tts,
            ..Default::default()
        };
        let job = UploadJob { channel_id: channel.id.clone(), files, content, reply_to: reply.map(|r| r.id), ping: self.reply_ping, options, error: None };
        self.uploads.insert(nonce.clone(), job);
        self.push_local(pending, cx);
        self.start_upload(nonce, cx);
    }
    // Runs (or re-runs, for Resend) the upload under `nonce`, repainting its progress as bytes go out.
    pub(crate) fn start_upload(&mut self, nonce: String, cx: &mut Context<Self>) {
        let (Some(client), Some(job)) = (self.client.clone(), self.uploads.get_mut(&nonce)) else { return };
        job.error = None;
        let job = job.clone();
        let ticking = nonce.clone();
        cx.spawn(async move |this, cx| {
            // The upload cards in the feed draw the bytes sent so far: a tick that moved none draws nothing.
            let mut shown = None;
            loop {
                cx.background_executor().timer(Duration::from_millis(100)).await;
                let sending = this.update(cx, |this, cx| {
                    let Some(job) = this.uploads.get(&ticking).filter(|job| job.error.is_none()) else { return false };
                    let sent: u64 = job.files.iter().map(|file| file.sent.load(std::sync::atomic::Ordering::Relaxed)).sum();
                    if shown.replace(sent) != Some(sent) {
                        this.redraw_main(cx);
                    }
                    true
                });
                if !sending.unwrap_or(false) {
                    break;
                }
            }
        })
        // lint-allow(detached-task): the progress loop stops itself once the upload settles or fails
        .detach();
        let task = cx.spawn({
            let nonce = nonce.clone();
            async move |this, cx| {
                let result = client.upload(&job.channel_id, &job.files, &job.content, &nonce, job.reply_to.as_deref().map(|id| (id, job.ping)), job.options).await;
                this.update(cx, |this, cx| this.settle(&job.channel_id, &nonce, result, cx)).ok();
            }
        });
        self.upload_tasks.insert(nonce, task);
        self.redraw_main(cx);
    }
    pub(crate) fn forget_upload(&mut self, nonce: &str) {
        for file in self.uploads.remove(nonce).into_iter().flat_map(|job| job.files) {
            self.file_previews.remove(&file.path);
        }
    }
    pub(crate) fn remove_file(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.pending_files.len() {
            let file = self.pending_files.remove(index);
            self.file_previews.remove(&file.path);
        }
        self.send_error = None;
        self.has_draft = !self.pending_files.is_empty() || !self.composer.read(cx).value().trim().is_empty();
        self.redraw_main(cx);
    }
    pub(crate) fn toggle_spoiler(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(file) = self.pending_files.get_mut(index) {
            file.spoiler = !file.spoiler;
        }
        self.redraw_main(cx);
    }
}
