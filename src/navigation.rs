//! Moving between servers and channels, loading and paging messages.

use crate::*;

impl Waffle {
    // Redraws what draws the feed, the composer and the call view: the main column, or the whole window
    // while a fullscreen call draws them in its own layout outside the columns.
    pub(crate) fn redraw_main(&mut self, cx: &mut Context<Self>) {
        if self.call_fullscreen && self.call_view {
            cx.notify(); // lint-allow(global-notify): the fullscreen call lays out its chat and tiles outside the columns
        } else {
            self.redraw(part::Column::Main, cx);
        }
    }
    pub(crate) fn open_guild(&mut self, guild_id: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.guild_id = guild_id.clone();
        self.favorites_open = None;
        self.guild_page = None;
        self.refresh_stream_previews(cx);
        self.request_channel_info();
        let remembered = self.last_channel.get(&guild_id).cloned();
        // In sidebar order, and only channels we can open (Discord also lists hidden ones).
        let channels: Vec<Channel> = match &guild_id {
            Some(_) => self.guild().map(|g| self.sidebar_sections(g).into_iter().flat_map(|(_, c)| c).collect()).unwrap_or_default(),
            None => self.dm_order(),
        };
        let target = remembered
            .and_then(|id| channels.iter().find(|c| c.id == id).cloned())
            .or_else(|| channels.into_iter().find(|c| matches!(c.kind, ChannelKind::Text | ChannelKind::Dm)));
        match target {
            Some(channel) => self.open_channel(channel, window, cx),
            None => {
                self.channel = None;
                // lint-allow(global-notify): an empty server changes the channel list, the feed and the panel together
                cx.notify();
            }
        }
    }
    pub(crate) fn open_channel(&mut self, channel: Channel, window: &mut Window, cx: &mut Context<Self>) {
        self.inbox_page = false;
        self.requests_page = None;
        self.held_unread = None;
        self.guild_page = None;
        self.note_channel_viewed(&channel, cx);
        // Results belong to the server or DM they searched: another place closes them.
        let scope = match &self.guild_id {
            Some(guild) => format!("guilds/{guild}"),
            None => format!("channels/{}", channel.id),
        };
        if matches!(&self.right_panel, RightPanel::Search(results) if !results.searched(&scope)) {
            self.right_panel = RightPanel::Members;
        }
        if matches!(&self.right_panel, RightPanel::ModView(view) if Some(view.guild_id()) != self.guild_id.as_deref()) {
            self.right_panel = RightPanel::Members;
        }
        if self.channel.as_ref().is_none_or(|c| c.id != channel.id) {
            self.history.visit(self.here());
        }
        self.show_channel(channel, window, cx);
    }
    /// Back/forward, like a browser's history of channels and Friends.
    pub(crate) fn navigate(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(stop) = self.history.step(forward, self.here()) else { return };
        match stop.channel {
            Some(channel) => {
                self.guild_id = stop.guild_id;
                self.show_channel(channel, window, cx);
            }
            None => self.show_no_channel(stop.guild_id, cx),
        }
    }
    pub(crate) fn show_channel(&mut self, channel: Channel, window: &mut Window, cx: &mut Context<Self>) {
        self.call_view = false;
        self.announcement_channel_opened(&channel, cx);
        let placeholder = format!("Message {}", channel_mention(&channel));
        self.composer.update(cx, |state, cx| state.set_placeholder(placeholder, window, cx));
        self.last_channel.insert(self.guild_id.clone(), channel.id.clone());
        home::save_last_channel(self.my_id(), self.guild_id.as_deref(), &channel.id);
        let id = channel.id.clone();
        if let (Some(client), Some(guild_id)) = (&self.client, &self.guild_id) {
            client.request_member_list(guild_id, &id);
        }
        // The channel we're leaving keeps what arrived while it was open.
        if let Some(previous) = self.channel.as_ref().filter(|c| c.id != id).and_then(|c| self.feeds.get(&c.id).map(|f| (c.id.clone(), f)))
            && previous.1.loaded
        {
            cache::save(&mut self.launch.messages, &previous.0, &previous.1.messages);
        }
        self.warm_feed(&id);
        self.load_dm_profile(&channel, cx);
        // Before the reset: the draft keeps the command typed into it.
        self.stash_draft(cx);
        self.command_picker.reset();
        self.clear_failed_interactions();
        if let Some(previous) = self.channel.as_ref().map(|c| c.id.clone()).filter(|p| *p != id) {
            match self.scroll_anchor() {
                Some(anchor) => self.scroll_anchors.insert(previous, anchor),
                None => self.scroll_anchors.remove(&previous),
            };
        }
        let is_forum = channel.kind == ChannelKind::Forum;
        self.channel = Some(channel);
        self.editing = None;
        self.restore_draft(window, cx);
        // Opening a channel or DM puts the cursor in its composer, as Discord's.
        if !is_forum {
            self.composer.update(cx, |state, cx| state.focus(window, cx));
        }
        self.send_error = None;
        self.autocomplete = None;
        self.scrolled_up = false;
        // Cached channels show at once; every open also refreshes in the background.
        // Messages after this sit under Discord's red NEW line until the channel is left.
        self.unread_marker = self.unread(&id).then(|| self.read_states.get(&id).map(|r| r.last_read)).flatten();
        if self.channel.as_ref().is_some_and(|c| c.kind == ChannelKind::Forum) {
            if !self.forums.contains_key(&id) {
                self.load_forum(id, cx);
            }
            // lint-allow(global-notify): opening a forum changes the channel list, the feed and the panel together
            return cx.notify();
        }
        let count = self.feeds.get(&id).map_or(0, |f| f.messages.len());
        self.list.reset(count);
        // With news, the channel opens on its first unread, under the NEW line, and stays unread until you
        // scroll down to the newest message (Discord's auto-read needs the bottom); otherwise it's read now.
        // Without, it opens where it was left.
        self.unread_scroll_pending = self.unread_marker.is_some();
        self.scroll_to_first_unread();
        if self.unread_marker.is_none()
            && let Some(anchor) = self.scroll_anchors.get(&id).cloned()
        {
            self.scroll_to_anchor(anchor);
        }
        self.fetch_latest(&id, cx);
        if self.unread_marker.is_none() {
            self.ack_open(cx);
        }
        // lint-allow(global-notify): opening a channel changes the channel list, the feed, the composer and the panel together
        cx.notify();
    }
    // The message at the top of the open feed and how far into it, while scrolled up from the newest.
    pub(crate) fn scroll_anchor(&self) -> Option<(String, gpui_kit::Pixels)> {
        if !self.scrolled_up {
            return None;
        }
        let top = self.list.logical_scroll_top();
        let feed = self.feeds.get(&self.channel.as_ref()?.id)?;
        Some((feed.messages.get(top.item_ix)?.id.clone(), top.offset_in_item))
    }
    // Back to an anchor, if its message is still in the feed.
    pub(crate) fn scroll_to_anchor(&mut self, (message_id, offset): (String, gpui_kit::Pixels)) {
        let Some(feed) = self.channel.as_ref().and_then(|c| self.feeds.get(&c.id)) else { return };
        let Some(item_ix) = feed.messages.iter().position(|m| m.id == message_id) else { return };
        self.list.scroll_to(ListOffset { item_ix, offset_in_item: offset });
        self.scrolled_up = true;
    }
    // Puts the first unread message near the top, once it's loaded.
    pub(crate) fn scroll_to_first_unread(&mut self) {
        let (Some(read), Some(channel)) = (self.unread_marker, self.channel.as_ref()) else { return };
        if !self.unread_scroll_pending {
            return;
        }
        let Some(feed) = self.feeds.get(&channel.id) else { return };
        let Some(first) = feed.messages.iter().position(|m| order(&m.id) > read) else { return };
        // Its row opens with the NEW line, at the feed's top, just under the unread bar.
        self.list.scroll_to(ListOffset { item_ix: first, offset_in_item: gpui_kit::Pixels::ZERO });
        self.unread_scroll_pending = false;
    }
    /// Hovering a channel warms its cache so the click shows messages instantly.
    pub(crate) fn prefetch(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        if self.feeds.get(channel_id).is_some_and(|f| f.loaded || f.fetching) {
            return;
        }
        self.fetch_latest(channel_id, cx);
    }
    pub(crate) fn fetch_latest(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let feed = self.feeds.entry(channel_id.to_owned()).or_default();
        if feed.fetching {
            return;
        }
        feed.fetching = true;
        let id = channel_id.to_owned();
        cx.spawn(async move |this, cx| {
            let result = client.messages(&id, None, None).await;
            this.update(cx, |this, cx| this.merge_latest(&id, result, cx)).ok();
        })
        // lint-allow(detached-task): one request for the latest messages, then it ends
        .detach();
    }
    pub(crate) fn merge_latest(&mut self, channel_id: &str, result: Result<Vec<Message>, String>, cx: &mut Context<Self>) {
        let is_open = self.channel.as_ref().is_some_and(|c| c.id == channel_id);
        // Read before the feed changes: the reset below would otherwise drop a reader scrolled up back to the bottom.
        let anchor = is_open.then(|| self.scroll_anchor()).flatten();
        let feed = self.feeds.entry(channel_id.to_owned()).or_default();
        feed.fetching = false;
        let fetched = match result {
            Ok(fetched) => fetched,
            Err(error) => {
                feed.error = Some(error.into());
                return self.redraw_main(cx);
            }
        };
        let newest = fetched.last().map(|m| order(&m.id)).unwrap_or(0);
        let kept: Vec<Message> = feed.messages.iter().filter(|m| m.state != SendState::Sent || order(&m.id) > newest).cloned().collect();
        let unchanged = kept.is_empty() && feed.loaded && feed.messages.len() == fetched.len();
        let reached_start = feed.has_more && fetched.len() < 50;
        feed.has_more = fetched.len() == 50;
        feed.messages = Arc::new(fetched.into_iter().chain(kept).collect());
        feed.loaded = true;
        feed.error = None;
        cache::save(&mut self.launch.messages, channel_id, &feed.messages);
        let count = feed.messages.len();
        let authors: Vec<String> = feed.messages.iter().map(|m| m.author.id.clone()).collect();
        if is_open && !unchanged {
            self.list.reset(count);
            if let Some(anchor) = anchor {
                self.scroll_to_anchor(anchor);
            }
            self.scroll_to_first_unread();
            // Tried once the messages are in: a first unread that never loads mustn't hold off reading.
            self.unread_scroll_pending = false;
        } else if is_open && reached_start {
            // The first row now carries the DM's start header.
            self.list.remeasure_items(0..1.min(count));
        }
        self.request_missing_roles(authors);
        if self.pending_jump.as_ref().is_some_and(|(channel, _)| channel == channel_id)
            && let Some((_, message_id)) = self.pending_jump.take()
        {
            self.jump_to(message_id, cx);
        }
        self.redraw_main(cx);
    }
    pub(crate) fn load_older(&mut self, cx: &mut Context<Self>) {
        let (Some(client), Some(channel)) = (self.client.clone(), self.channel.clone()) else { return };
        let Some(feed) = self.feeds.get_mut(&channel.id) else { return };
        if !feed.has_more || feed.loading_older || !feed.loaded {
            return;
        }
        let Some(oldest) = feed.messages.first().map(|m| m.id.clone()) else { return };
        feed.loading_older = true;
        cx.spawn(async move |this, cx| {
            let result = client.messages(&channel.id, Some(&oldest), None).await;
            this.update(cx, |this, cx| {
                let is_open = this.channel.as_ref().is_some_and(|c| c.id == channel.id);
                let Some(feed) = this.feeds.get_mut(&channel.id) else { return };
                feed.loading_older = false;
                let Ok(older) = result else { return };
                feed.has_more = older.len() == 50;
                let added = older.len();
                let authors: Vec<String> = older.iter().map(|m| m.author.id.clone()).collect();
                Arc::make_mut(&mut feed.messages).splice(0..0, older);
                if is_open && added == 0 && !feed.has_more {
                    this.list.remeasure_items(0..1.min(feed.messages.len()));
                }
                if is_open && added > 0 {
                    // Keep the same message under the reader's eye while history grows above it.
                    let top = this.list.logical_scroll_top();
                    this.list.splice(0..0, added);
                    this.list.scroll_to(ListOffset { item_ix: top.item_ix + added, offset_in_item: top.offset_in_item });
                }
                if added > 0 {
                    this.refresh_viewer(cx);
                }
                this.request_missing_roles(authors);
                this.redraw_main(cx);
            })
            .ok();
        })
        // lint-allow(detached-task): one request for older messages, then it ends
        .detach();
    }
    // History from REST carries no member roles; ask the gateway for the authors we can't colour yet.
    // History from REST names authors by their account; once their member entries arrive, show
    // them as the server does (nickname and server avatar).
    pub(crate) fn apply_members(&mut self, guild_id: &str) {
        let Some(known) = self.members.get(guild_id) else { return };
        let stale = |m: &Message| m.guild_id.as_deref() == Some(guild_id) && known.get(&m.author.id).is_some_and(|k| k.name != m.author.name || k.avatar != m.author.avatar);
        // Member lists update often: only a feed that actually changes is copied for writing.
        for feed in self.feeds.values_mut().filter(|f| f.messages.iter().any(stale)) {
            for message in Arc::make_mut(&mut feed.messages).iter_mut().filter(|m| stale(m)) {
                let member = &known[&message.author.id];
                message.author.name.clone_from(&member.name);
                message.author.avatar.clone_from(&member.avatar);
            }
        }
    }
    pub(crate) fn request_missing_roles(&mut self, authors: Vec<String>) {
        if let Some(guild_id) = self.guild_id.clone() {
            self.apply_members(&guild_id);
        }
        let (Some(client), Some(guild_id)) = (&self.client, &self.guild_id) else { return };
        let known = self.member_roles.get(guild_id);
        let mut missing: Vec<String> = Vec::new();
        for id in authors {
            if known.is_some_and(|k| k.contains_key(&id)) || !self.requested_members.insert((guild_id.clone(), id.clone())) {
                continue;
            }
            missing.push(id);
        }
        for chunk in missing.chunks(100) {
            client.request_members(guild_id, chunk.to_vec());
        }
    }
    /// Scrolls to the replied-to message and flashes it; loads the history around it first if needed.
    pub(crate) fn jump_to(&mut self, message_id: String, cx: &mut Context<Self>) {
        let Some(channel) = self.channel.clone() else { return };
        // A channel just opened is still fetching its newest page, which lands after this and resets the
        // list to the bottom: wait for it (`merge_latest` runs the jump).
        if self.feeds.get(&channel.id).is_some_and(|f| f.fetching) {
            self.pending_jump = Some((channel.id.clone(), message_id));
            return;
        }
        let position = self.feeds.get(&channel.id).and_then(|f| f.messages.iter().position(|m| m.id == message_id));
        if let Some(ix) = position {
            self.list.scroll_to(ListOffset { item_ix: ix, offset_in_item: gpui_kit::Pixels::ZERO });
            self.flash = Some(message_id.clone());
            // The row's height is known only once it's laid out: then pull it down to the middle of the view.
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(32)).await;
                this.update(cx, |this, cx| {
                    let (Some(row), view) = (this.list.bounds_for_item(ix), this.list.viewport_bounds()) else { return };
                    let spare = view.size.height - row.size.height;
                    if spare > gpui_kit::Pixels::ZERO {
                        this.list.scroll_by(row.top() - view.top() - spare / 2.);
                        this.redraw_main(cx);
                    }
                })
                .ok();
            })
            // lint-allow(detached-task): one 32ms wait, then it ends
            .detach();
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(chat::JUMP_FLASH).await;
                this.update(cx, |this, cx| {
                    if this.flash.as_ref() == Some(&message_id) {
                        this.flash = None;
                    }
                    this.redraw_main(cx);
                })
                .ok();
            })
            // lint-allow(detached-task): one JUMP_FLASH timer, then it ends
            .detach();
            return self.redraw_main(cx);
        }
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            let Ok(around) = client.messages(&channel.id, None, Some(&message_id)).await else { return };
            this.update(cx, |this, cx| {
                let feed = this.feeds.entry(channel.id.clone()).or_default();
                feed.messages = Arc::new(around);
                feed.has_more = true;
                this.list.reset(feed.messages.len());
                this.jump_to(message_id, cx);
            })
            .ok();
        })
        // lint-allow(detached-task): one request for the messages around the target, then it ends
        .detach();
    }
    // Reads the open channel up to its newest message, only while that message is in view and the window
    // has focus (Discord's auto-read).
    pub(crate) fn ack_open(&mut self, cx: &mut Context<Self>) {
        if self.scrolled_up || !self.window_active || self.unread_scroll_pending {
            return;
        }
        let (Some(client), Some(channel)) = (self.client.clone(), self.channel.as_ref()) else { return };
        if self.held_unread.as_ref() == Some(&channel.id) {
            return;
        }
        let Some(&last) = self.last_message.get(&channel.id) else { return };
        let read = self.read_states.entry(channel.id.clone()).or_default();
        if read.last_read >= last && read.mentions == 0 {
            return;
        }
        read.last_read = last;
        read.mentions = 0;
        let id = channel.id.clone();
        discord::runtime().spawn(async move { client.ack(&id, &last.to_string()).await });
        let _ = cx;
    }
    pub(crate) fn open_pins(&mut self, cx: &mut Context<Self>) {
        let (Some(client), Some(channel)) = (self.client.clone(), self.channel.clone()) else { return };
        self.pins = Some(None);
        self.pins_list.reset(0);
        // lint-allow(global-notify): the pins popover is drawn by the overlays, which render with the app
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = client.pins(&channel.id).await;
            this.update(cx, |this, cx| {
                if this.pins.is_some() {
                    this.pins = Some(Some(result.map_err(Into::into)));
                }
                // lint-allow(global-notify): the pins popover is drawn by the overlays, which render with the app
                cx.notify();
            })
            .ok();
        })
        // lint-allow(detached-task): one request for the pins, then it ends
        .detach();
    }
    /// Alt+↑/↓: the previous or next text channel in the sidebar's order, as in Discord.
    pub(crate) fn step_channel(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let order: Vec<Channel> = match self.guild() {
            Some(guild) => self.sidebar_sections(guild).into_iter().flat_map(|(_, c)| c).filter(|c| c.kind == ChannelKind::Text).collect(),
            None => self.dm_order(),
        };
        let current = self.channel.as_ref().and_then(|c| order.iter().position(|o| o.id == c.id)).unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, order.len() as isize - 1);
        if let Some(channel) = order.get(next as usize).cloned() {
            self.open_channel(channel, window, cx);
        }
    }
}
