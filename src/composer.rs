//! The composer: typing, drafts, replies, edits and mention suggestions.

use crate::*;

impl Waffle {
    pub(crate) fn on_composer(&mut self, _: &Entity<ComposerState>, event: &InputEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            InputEvent::Change => self.on_composer_change(window, cx),
            InputEvent::PressEnter { shift: false, .. } if self.on_command_enter(window, cx) => {}
            InputEvent::PressEnter { shift: false, .. } if self.autocomplete.is_some() => self.accept_suggestion(window, cx),
            InputEvent::PressEnter { shift: false, .. } => self.submit(self.hold_up.is_some(), window, cx),
            _ => {}
        }
    }
    // A slash command typed out gets the hint bar instead of mention suggestions, and sends no typing.
    pub(crate) fn on_composer_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).value();
        self.has_draft = !text.trim().is_empty() || !self.pending_files.is_empty();
        self.stash_draft(cx);
        self.update_command_picker(&text, cx);
        let command = self.on_command_text(window, cx);
        self.autocomplete = (!command && !self.command_picker.is_open()).then(|| self.suggest(&text)).flatten();
        self.hold_up = None;
        self.chip_silent(window, cx);
        if !command {
            self.send_typing();
        }
        self.redraw(part::Column::Nav, cx); self.redraw_main(cx);
    }
    pub(crate) fn send_typing(&mut self) {
        let (Some(client), Some(channel)) = (self.client.clone(), self.channel.as_ref()) else { return };
        if !self.has_draft || self.last_typing_sent.is_some_and(|t| t.elapsed() < TYPING_SEND_EVERY) {
            return;
        }
        self.last_typing_sent = Some(Instant::now());
        let id = channel.id.clone();
        discord::runtime().spawn(async move { client.typing(&id).await });
    }
    pub(crate) fn start_reply(&mut self, message: Message, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = None;
        self.reply_ping = true;
        self.replying = Some(message);
        self.composer.update(cx, |state, cx| state.focus(window, cx));
        self.redraw_main(cx);
    }
    pub(crate) fn start_edit(&mut self, message: Message, window: &mut Window, cx: &mut Context<Self>) {
        self.replying = None;
        let text = self.shown_text(&message.content);
        self.editing = Some(message);
        self.composer.update(cx, |state, cx| {
            state.set_value(text, window, cx);
            state.focus(window, cx);
        });
        self.has_draft = true;
        self.redraw_main(cx);
    }
    pub(crate) fn cancel_compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.autocomplete = None;
        if self.editing.take().is_some() {
            self.restore_draft(window, cx);
            return self.redraw_main(cx);
        }
        self.replying = None;
        self.redraw_main(cx);
    }
    // Keeps the open channel's composer as its draft; an edit in progress is not one.
    pub(crate) fn stash_draft(&mut self, cx: &mut Context<Self>) {
        let Some(channel_id) = self.channel.as_ref().map(|c| c.id.clone()) else { return };
        if self.editing.is_some() {
            return;
        }
        let (command, picks) = self.command_draft(cx);
        let draft = drafts::Draft::new(&self.composer.read(cx).content(), self.replying.clone()).with_command(command, picks);
        self.drafts.set(&channel_id, draft);
        self.save_drafts_soon(cx);
    }
    // Puts the open channel's draft back in the composer (or empties it), caret at the end.
    pub(crate) fn restore_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let draft = self.channel.as_ref().and_then(|c| self.drafts.get(&c.id)).cloned().unwrap_or_default();
        let content = draft.content();
        let end = content.text().len();
        self.has_draft = !content.text().trim().is_empty() || !self.pending_files.is_empty();
        self.composer.update(cx, |state, cx| {
            state.set_value(content, window, cx);
            state.set_selected_range(end..end, cx);
        });
        self.restore_command(draft.command, draft.picks, cx);
        self.replying = draft.reply;
    }
    // Typing rewrites the drafts file at most once per pause.
    pub(crate) fn save_drafts_soon(&mut self, cx: &mut Context<Self>) {
        self.drafts_save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DRAFTS_SAVE_AFTER).await;
            this.update(cx, |this, _| this.drafts.save()).ok();
        }));
    }
    /// Up arrow in an empty composer edits your last message, like Discord.
    pub(crate) fn edit_last(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let (Some(me), Some(channel)) = (&self.me, &self.channel) else { return false };
        let last = self.feeds.get(&channel.id).and_then(|f| f.messages.iter().rev().find(|m| m.author.id == me.id && m.state == SendState::Sent)).cloned();
        let Some(last) = last else { return false };
        self.start_edit(last, window, cx);
        true
    }
    pub(crate) fn suggest(&self, text: &str) -> Option<Autocomplete> {
        let last = text.rsplit(char::is_whitespace).next()?;
        if let Some(query) = composer_emoji::emoji_query(last) {
            let items: Vec<Suggestion> = self
                .emoji_suggestions(query)
                .into_iter()
                .map(|m| {
                    let shown = format!(":{}:", m.name);
                    let wire = match &m.emoji.id {
                        Some(id) => format!("<{}:{}:{id}>", if m.emoji.animated { "a" } else { "" }, m.emoji.name),
                        None => m.emoji.name.clone(),
                    };
                    Suggestion { label: shown, insert: wire.clone(), wire, lead: SuggestLead::Emoji(m.emoji), detail: m.server }
                })
                .collect();
            return (!items.is_empty()).then(|| Autocomplete { token: last.to_owned(), items, selected: 0 });
        }
        // A bare `@` or `#` already lists everyone or every channel, as Discord does.
        let token = Some(last).filter(|t| t.starts_with('@') || t.starts_with('#'))?;
        let query = token[1..].to_lowercase();
        let mut items: Vec<Suggestion> = Vec::new();
        if token.starts_with('#') {
            for c in self.guild().map(|g| g.channels.as_slice()).unwrap_or_default() {
                if c.kind == ChannelKind::Text && c.name.to_lowercase().contains(&query) {
                    items.push(Suggestion { label: c.name.clone(), insert: format!("#{}", c.name), wire: format!("<#{}>", c.id), lead: SuggestLead::Channel, detail: None });
                }
            }
        } else {
            for user in self.mention_candidates() {
                if user.name.to_lowercase().contains(&query) && !items.iter().any(|s| s.wire == format!("<@{}>", user.id)) {
                    items.push(Suggestion { label: user.name.clone(), insert: format!("@{}", user.name), wire: format!("<@{}>", user.id), lead: SuggestLead::Avatar(user.avatar), detail: None });
                }
            }
        }
        items.truncate(8);
        (!items.is_empty()).then(|| Autocomplete { token: token.to_owned(), items, selected: 0 })
    }
    pub(crate) fn accept_suggestion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(auto) = self.autocomplete.take() else { return };
        let Some(pick) = auto.items.get(auto.selected) else { return };
        // An emoji goes in as itself: the character, or a custom one's `:name:` sent as its `<:name:id>`.
        if let SuggestLead::Emoji(emoji) = &pick.lead {
            let shown = if emoji.id.is_some() { pick.label.clone() } else { pick.wire.clone() };
            if emoji.id.is_some() {
                self.mention_wire.insert(shown.clone(), pick.wire.clone());
            }
            let emoji = emoji.clone();
            self.update_prefs(cx, |p| p.record_emoji(&emoji));
            self.composer.update(cx, |state, cx| complete_text(state, &auto.token, &shown, window, cx));
            self.has_draft = true;
            return self.redraw_main(cx);
        }
        let chip = InlineToken::new(pick.wire.clone(), pick.insert.clone());
        self.composer.update(cx, |state, cx| complete_mention(state, &auto.token, chip, window, cx));
        self.redraw_main(cx);
    }
    // "@silent " typed at the start becomes a chip, as Discord styles the prefix; its wire form is the text itself.
    pub(crate) fn chip_silent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let content = self.composer.read(cx).content();
        if content.text().as_ref() != "@silent " || !content.tokens().is_empty() {
            return;
        }
        self.composer.update(cx, |state, cx| complete_mention(state, "@silent ", InlineToken::new("@silent", "@silent"), window, cx));
    }
    pub(crate) fn move_suggestion(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(auto) = &mut self.autocomplete else { return };
        auto.selected = (auto.selected as isize + delta).rem_euclid(auto.items.len() as isize) as usize;
        self.autocomplete_scroll.scroll_to_item(auto.selected);
        self.redraw_main(cx);
    }
    // "@Ferris" → "<@id>": longest first, so "@Ann Marie" wins over "@Ann".
    pub(crate) fn to_wire(&self, text: &str) -> String {
        let mut pairs: Vec<_> = self.mention_wire.iter().collect();
        pairs.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));
        pairs.into_iter().fold(text.to_owned(), |text, (shown, wire)| text.replace(shown.as_str(), wire))
    }
    pub(crate) fn shown_text(&self, text: &str) -> String {
        self.mention_wire.iter().fold(text.to_owned(), |text, (shown, wire)| text.replace(wire.as_str(), shown))
    }
    // Who `@` offers: in a DM or group DM only its own people, in a server everyone known there; you last.
    pub(crate) fn mention_candidates(&self) -> Vec<User> {
        let me = self.me.as_ref().map(|m| m.id.as_str());
        match self.channel.as_ref().filter(|_| self.guild_id.is_none()) {
            Some(dm) => mention_order(dm.recipients.iter().cloned().chain(dm.recipient.clone()).chain(self.me.clone()), me),
            None => {
                let members = self.guild_id.as_ref().and_then(|g| self.member_lists.get(g)).into_iter().flatten().filter_map(|i| match i {
                    ListItem::Member(m) => Some(m.user.clone()),
                    ListItem::Group { .. } => None,
                });
                let authors = self.channel.as_ref().and_then(|c| self.feeds.get(&c.id)).into_iter().flat_map(|f| f.messages.iter().rev().map(|m| m.author.clone()));
                mention_order(members.chain(authors), me)
            }
        }
    }
}
