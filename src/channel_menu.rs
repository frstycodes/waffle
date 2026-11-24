// Discord's channel, category, server, folder and server-dropdown menus, item-for-item from its
// menu code (docs/discord-menus-channel-server.md, read from Discord web 2026-11-14). Stages, events,
// apps and the full server-settings screens aren't built, so their items are left out. Favorites-mode
// menus live in favorites.rs.
use gpui_kit::{Context, Pixels, Point, SharedString, Window};
use reqwest::Method;
use serde_json::{Value, json};

mod server;
mod thread;

use crate::{
    Waffle,
    part::Column,
    discord::{Channel, ChannelKind, GuildSettings, channel_flag, guild_flag, parse_guild_settings, perm},
    icons::Glyph,
    menu::{Entry, HeadLead, header, item, submenu},
};

// Discord's mute lengths (seconds; -1 is "until I turn it back on").
pub(crate) const MUTE_FOR: [(&str, i64); 6] =
    [("For 15 minutes", 900), ("For 1 hour", 3600), ("For 3 hours", 10_800), ("For 8 hours", 28_800), ("For 24 hours", 86_400), ("Until I turn it back on", -1)];

// Discord's channel types for creating channels.
const TEXT: u64 = 0;
const VOICE: u64 = 2;
const CATEGORY: u64 = 4;

impl Waffle {
    // Our settings for a server, with "use the server's default" resolved to that default, as
    // Discord shows it when we've never changed anything there.
    pub(crate) fn settings_for(&self, guild_key: &str) -> GuildSettings {
        let default = self.guilds.iter().find(|g| g.id == guild_key).map_or(0, |g| g.default_notifications);
        match self.guild_settings.get(guild_key).cloned() {
            Some(mut settings) => {
                if settings.notifications == 3 {
                    settings.notifications = default;
                }
                settings
            }
            None => GuildSettings { notifications: default, ..Default::default() },
        }
    }

    // A permission in one server channel or thread, overwrites applied. Found by channel id because
    // messages from REST history carry no guild_id; the open channel covers threads not in `threads`.
    pub(crate) fn can_in(&self, channel_id: &str, bit: u64) -> bool {
        let Some(me) = self.me.as_ref() else { return false };
        let open = self.channel.as_ref().filter(|c| c.id == channel_id).zip(self.guild());
        let found = self.guilds.iter().find_map(|g| Some((g.channel(channel_id)?, g))).or(open);
        found.is_some_and(|(channel, guild)| guild.channel_permissions(&me.id, channel) & bit != 0)
    }

    // Saves part of our notification settings for a server ("@me" for DMs). The change shows at
    // once; Discord's answer then replaces it, or a refusal puts the old settings back.
    pub(crate) fn patch_guild_settings(&mut self, guild_key: String, body: Value, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let before = self.guild_settings.get(&guild_key).cloned();
        let mut optimistic = self.settings_for(&guild_key);
        merge_settings(&mut optimistic, &body);
        self.guild_settings.insert(guild_key.clone(), optimistic);
        cx.notify(); // lint-allow(global-notify): mute and notification settings show in the rail, the channel list and the chat
        cx.spawn(async move |this, cx| {
            let result = client.call(Method::PATCH, format!("/users/@me/guilds/{guild_key}/settings"), Some(body)).await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(updated) => {
                        let (key, settings) = parse_guild_settings(&updated);
                        this.guild_settings.insert(key, settings);
                    }
                    Err(error) => {
                        match before {
                            Some(settings) => this.guild_settings.insert(guild_key.clone(), settings),
                            None => this.guild_settings.remove(&guild_key),
                        };
                        this.show_toast(error, cx);
                    }
                }
                cx.notify(); // lint-allow(global-notify): mute and notification settings show in the rail, the channel list and the chat
            })
            .ok();
        })
        // lint-allow(detached-task): ends when Discord answers the request, and holds only a WeakEntity.
        .detach();
    }

    // Saves part of one channel's or category's override in a server.
    pub(crate) fn patch_channel_override(&mut self, guild_id: String, channel_id: &str, body: Value, cx: &mut Context<Self>) {
        let mut overrides = serde_json::Map::new();
        overrides.insert(channel_id.to_owned(), body);
        self.patch_guild_settings(guild_id, json!({ "channel_overrides": overrides }), cx);
    }

    // A channel or thread edit (name, category, archived, locked, …) shown at once; the old copy
    // comes back if Discord refuses.
    pub(crate) fn channel_optimistic(&mut self, channel_id: &str, method: Method, body: Value, change: impl Fn(&mut Channel) + 'static, cx: &mut Context<Self>) {
        let before = self.guilds.iter().flat_map(|g| g.channels.iter().chain(g.threads.iter())).find(|c| c.id == channel_id).cloned();
        let (a, b) = (channel_id.to_owned(), channel_id.to_owned());
        self.rest_optimistic(
            method,
            format!("/channels/{channel_id}"),
            Some(body),
            move |this| {
                if let Some(channel) = this.guilds.iter_mut().flat_map(|g| g.channels.iter_mut().chain(g.threads.iter_mut())).find(|c| c.id == a) {
                    change(channel);
                }
                for guild in this.guilds.iter_mut() {
                    guild.channels.sort_by_key(|c| c.position);
                }
            },
            move |this| {
                if let (Some(before), Some(channel)) = (before, this.guilds.iter_mut().flat_map(|g| g.channels.iter_mut().chain(g.threads.iter_mut())).find(|c| c.id == b)) {
                    *channel = before;
                }
            },
            cx,
        );
    }

    fn thread_joined_optimistic(&mut self, thread_id: &str, joined: bool, cx: &mut Context<Self>) {
        let (a, b) = (thread_id.to_owned(), thread_id.to_owned());
        self.rest_optimistic(
            if joined { Method::PUT } else { Method::DELETE },
            format!("/channels/{thread_id}/thread-members/@me"),
            None,
            move |this| set_joined(this, &a, joined),
            move |this| set_joined(this, &b, !joined),
            cx,
        );
    }

    // Our thread-member settings (mute, notification flags), shown at once.
    fn thread_member_optimistic(&mut self, thread_id: &str, body: Value, change: impl Fn(&mut crate::discord::ThreadMeta) + 'static, cx: &mut Context<Self>) {
        let before = self.guilds.iter().flat_map(|g| g.threads.iter()).find(|t| t.id == thread_id).and_then(|t| t.thread.clone());
        let (a, b) = (thread_id.to_owned(), thread_id.to_owned());
        self.rest_optimistic(
            Method::PATCH,
            format!("/channels/{thread_id}/thread-members/@me/settings"),
            Some(body),
            move |this| {
                if let Some(meta) = this.guilds.iter_mut().flat_map(|g| g.threads.iter_mut()).find(|t| t.id == a).and_then(|t| t.thread.as_mut()) {
                    change(meta);
                }
            },
            move |this| {
                if let (Some(before), Some(meta)) = (before, this.guilds.iter_mut().flat_map(|g| g.threads.iter_mut()).find(|t| t.id == b).and_then(|t| t.thread.as_mut())) {
                    *meta = before;
                }
            },
            cx,
        );
    }

    fn delete_channel_optimistic(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        let found = self.guilds.iter().enumerate().find_map(|(gi, g)| {
            g.channels.iter().position(|c| c.id == channel_id).map(|i| (gi, false, i, g.channels[i].clone())).or_else(|| g.threads.iter().position(|c| c.id == channel_id).map(|i| (gi, true, i, g.threads[i].clone())))
        });
        let Some((guild_index, thread, index, channel)) = found else { return };
        let id = channel_id.to_owned();
        self.rest_optimistic(
            Method::DELETE,
            format!("/channels/{channel_id}"),
            None,
            move |this| {
                for guild in this.guilds.iter_mut() {
                    guild.channels.retain(|c| c.id != id);
                    guild.threads.retain(|c| c.id != id);
                }
            },
            move |this| {
                if let Some(guild) = this.guilds.get_mut(guild_index) {
                    let list = if thread { &mut guild.threads } else { &mut guild.channels };
                    list.insert(index.min(list.len()), channel);
                }
            },
            cx,
        );
    }

    fn leave_guild_optimistic(&mut self, guild_id: &str, cx: &mut Context<Self>) {
        let Some(index) = self.guilds.iter().position(|g| g.id == guild_id) else { return };
        let guild = self.guilds[index].clone();
        let id = guild_id.to_owned();
        self.rest_optimistic(
            Method::DELETE,
            format!("/users/@me/guilds/{guild_id}"),
            Some(json!({ "lurking": false })),
            move |this| {
                this.guilds.retain(|g| g.id != id);
                if this.guild_id.as_deref() == Some(id.as_str()) {
                    this.guild_id = None;
                    this.channel = None;
                }
            },
            move |this| {
                let at = index.min(this.guilds.len());
                this.guilds.insert(at, guild);
            },
            cx,
        );
    }

    // Mute and its six lengths, or Unmute with when it ends: the same shape for channels and servers.
    pub(crate) fn mute_entry(&self, label: &str, unmute: &str, muted: bool, end: Option<&str>, apply: impl Fn(&mut Waffle, Value, &mut Context<Waffle>) + Clone + 'static) -> Entry {
        if muted {
            let hint = end.and_then(|e| chrono::DateTime::parse_from_rfc3339(e).ok()).map(|e| format!("Muted until {}", until(e.with_timezone(&chrono::Local), chrono::Local::now())));
            let apply = apply.clone();
            let row = item(unmute.to_owned(), move |this, _, cx| apply(this, json!({ "muted": false }), cx)).icon(Glyph::BellRinging);
            return match hint {
                Some(hint) => row.hint(hint),
                None => row,
            }
            .entry();
        }
        let lengths: Vec<Entry> = MUTE_FOR
            .iter()
            .map(|&(text, seconds)| {
                let apply = apply.clone();
                let row = item(text, move |this, _, cx| apply(this, mute_body(seconds), cx));
                // Each length says when it ends; the menu redraws, so the times stay current.
                let now = chrono::Local::now();
                if seconds > 0 { row.hint(format!("Until {}", until(now + chrono::Duration::seconds(seconds), now))) } else { row }.entry()
            })
            .collect();
        submenu(label.to_owned(), lengths).icon(Glyph::BellSlash).entry()
    }

    fn notification_radios(&self, current: u8, inherited: Option<(&'static str, u8)>, apply: impl Fn(&mut Waffle, u8, &mut Context<Waffle>) + Clone + 'static) -> Vec<Entry> {
        let mut options: Vec<(&'static str, Option<&'static str>, u8)> = Vec::new();
        if let Some((label, value)) = inherited {
            options.push((label, Some(notification_label(value)), 3));
        }
        options.extend([("All messages", None, 0), ("Only @mentions", None, 1), ("Nothing", None, 2)]);
        options
            .into_iter()
            .map(|(label, hint, value)| {
                let apply = apply.clone();
                let row = item(label, move |this, _, cx| apply(this, value, cx)).radio(current == value);
                match hint {
                    Some(hint) => row.hint(hint),
                    None => row,
                }
                .entry()
            })
            .collect()
    }

    pub(crate) fn open_channel_menu(&mut self, channel: Channel, guild_id: String, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.open_live_menu(at, move |this| match channel.kind {
            ChannelKind::Category => this.category_menu(&channel, &guild_id),
            _ => this.channel_menu(&channel, &guild_id),
        }, cx);
    }

    /// Mute and notification level for one channel: in its right-click menu and behind the header's bell.
    pub(crate) fn channel_notify_entries(&self, channel: &Channel, guild_id: &str) -> Vec<Entry> {
        let settings = self.settings_for(guild_id);
        let overrides = settings.channels.get(&channel.id).cloned().unwrap_or_default();
        let (gid, cid) = (guild_id.to_owned(), channel.id.clone());
        let mut out = vec![self.mute_entry("Mute channel", "Unmute channel", settings.channel_muted(&channel.id), overrides.mute_end.as_deref(), move |this, mut body, cx| {
            body["channel_id"] = json!(cid);
            this.patch_channel_override(gid.clone(), &cid, body, cx);
        })];
        // Discord has no notification levels for voice channels.
        if channel.kind != ChannelKind::Voice {
            let inherited = if channel.parent_id.is_some() { "Use category default" } else { "Use server default" };
            let (gid, cid) = (guild_id.to_owned(), channel.id.clone());
            let radios = self.notification_radios(overrides.notifications, Some((inherited, settings.notifications)), move |this, level, cx| {
                this.patch_channel_override(gid.clone(), &cid, json!({ "message_notifications": level }), cx);
            });
            out.push(submenu("Notification settings", radios).hint(notification_label(effective(overrides.notifications, settings.notifications))).icon(Glyph::Bell).entry());
        }
        out
    }

    pub(crate) fn open_notify_menu(&mut self, channel: Channel, guild_id: String, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.open_live_menu(at, move |this| this.channel_notify_entries(&channel, &guild_id), cx);
    }

    fn channel_menu(&self, channel: &Channel, guild_id: &str) -> Vec<Entry> {
        let voice = channel.kind == ChannelKind::Voice;
        let manage = self.can_in(&channel.id, perm::MANAGE_CHANNELS);
        let mut out = vec![self.channel_menu_header(channel, guild_id)];

        let id = channel.id.clone();
        let read = item("Mark as read", move |this, _, cx| this.mark_channel_read(&id, cx)).icon(Glyph::Checks);
        out.push(if self.unread(&channel.id) { read } else { read.disabled() }.entry());
        out.push(Entry::Separator);
        out.extend(self.favorite_add_entry(channel));
        out.push(Entry::Separator);

        let (gid, cid) = (guild_id.to_owned(), channel.id.clone());
        out.push(item(if voice { "Invite to voice" } else { "Invite people" }, move |this, window, cx| this.show_invite(gid.clone(), cid.clone(), window, cx)).brand().icon(Glyph::GroupPlus).entry());
        out.push(self.pin_channel_entry(channel, guild_id));
        out.extend(self.channel_list_entry(channel, guild_id));
        let link = format!("https://discord.com/channels/{guild_id}/{}", channel.id);
        out.push(item("Copy link", move |this, _, cx| this.copy_text(link.clone(), cx)).icon(Glyph::Link).entry());
        out.push(Entry::Separator);

        if voice {
            let open = channel.clone();
            out.push(item("Open chat", move |this, window, cx| this.open_channel(open.clone(), window, cx)).icon(Glyph::ChatRound).entry());
            out.extend(self.voice_status_entry(channel));
            let id = channel.id.clone();
            out.push(item("Hide names", move |this, _, cx| {
                if !this.hidden_voice_names.remove(&id) {
                    this.hidden_voice_names.insert(id.clone());
                }
                this.redraw(Column::Nav, cx);
            })
            .checked(self.hidden_voice_names.contains(&channel.id))
            .icon(Glyph::EyeSlash)
            .entry());
            out.push(Entry::Separator);
        }

        out.extend(self.channel_notify_entries(channel, guild_id));
        out.push(Entry::Separator);

        if !self.channel_setting_pages(guild_id, channel).is_empty() {
            let (gid, ch) = (guild_id.to_owned(), channel.clone());
            out.push(item("Edit channel", move |this, window, cx| this.open_channel_settings(ch.clone(), gid.clone(), window, cx)).icon(Glyph::Pen).entry());
        }
        if manage {
            out.push(self.move_to_category(channel, guild_id));
            out.push(Entry::Separator);
            let gid = guild_id.to_owned();
            let (gid2, ch2) = (gid.clone(), channel.clone());
            out.push(item("Duplicate channel", move |this, _, cx| {
                let body = json!({ "name": ch2.name, "type": ch2.raw_type, "parent_id": ch2.parent_id, "topic": ch2.topic });
                this.rest(Method::POST, format!("/guilds/{gid2}/channels"), Some(body), |_, _| {}, cx);
            })
            .icon(Glyph::CopySimple)
            .entry());
            let (gid3, parent, kind) = (gid.clone(), channel.parent_id.clone(), if voice { VOICE } else { TEXT });
            out.push(item(if voice { "Create voice channel" } else { "Create text channel" }, move |this, window, cx| this.create_channel(gid3.clone(), kind, parent.clone(), window, cx)).icon(Glyph::Plus).entry());
            out.push(self.delete_channel_entry(channel, "Delete channel"));
            out.push(Entry::Separator);
        }
        out.extend(self.favorite_remove_entry(channel));
        out.push(Entry::Separator);

        if self.developer_mode {
            let id = channel.id.clone();
            out.push(item("Copy channel ID", move |this, _, cx| this.copy_text(id.clone(), cx)).icon(Glyph::Id).entry());
        }
        out
    }

    fn category_menu(&self, category: &Channel, guild_id: &str) -> Vec<Entry> {
        let settings = self.settings_for(guild_id);
        let overrides = settings.channels.get(&category.id).cloned().unwrap_or_default();
        let children: Vec<String> = self.guilds.iter().find(|g| g.id == guild_id).map(|g| g.channels.iter().filter(|c| c.parent_id.as_ref() == Some(&category.id)).map(|c| c.id.clone()).collect()).unwrap_or_default();
        let mut out = vec![self.channel_menu_header(category, guild_id)];

        let unread = children.iter().any(|c| self.unread(c));
        let ids = children.clone();
        let read = item("Mark as read", move |this, _, cx| {
            for id in &ids {
                this.mark_channel_read(id, cx);
            }
        })
        .icon(Glyph::Checks);
        out.push(if unread { read } else { read.disabled() }.entry());
        out.push(Entry::Separator);

        out.extend(self.channel_list_entry(category, guild_id));
        let id = category.id.clone();
        out.push(item("Collapse category", move |this, _, cx| {
            if !this.folded.remove(&id) {
                this.folded.insert(id.clone());
            }
            this.redraw(Column::Nav, cx);
        })
        .checked(self.folded.contains(&category.id))
        .icon(Glyph::Collapse)
        .entry());
        let categories: Vec<String> = self.guilds.iter().find(|g| g.id == guild_id).map(|g| g.channels.iter().filter(|c| c.kind == ChannelKind::Category).map(|c| c.id.clone()).collect()).unwrap_or_default();
        if categories.iter().any(|c| !self.folded.contains(c)) {
            out.push(item("Collapse all categories", move |this, _, cx| {
                this.folded.extend(categories.iter().cloned());
                this.redraw(Column::Nav, cx);
            })
            .icon(Glyph::CollapseAll)
            .entry());
        }
        out.push(Entry::Separator);

        let (gid, cid) = (guild_id.to_owned(), category.id.clone());
        out.push(self.mute_entry("Mute category", "Unmute category", settings.channel_muted(&category.id), overrides.mute_end.as_deref(), move |this, body, cx| {
            this.patch_channel_override(gid.clone(), &cid, body, cx);
        }));
        let (gid, cid) = (guild_id.to_owned(), category.id.clone());
        let radios = self.notification_radios(overrides.notifications, Some(("Use server default", settings.notifications)), move |this, level, cx| {
            this.patch_channel_override(gid.clone(), &cid, json!({ "message_notifications": level }), cx);
        });
        out.push(submenu("Notification settings", radios).hint(notification_label(effective(overrides.notifications, settings.notifications))).icon(Glyph::Bell).entry());
        out.push(Entry::Separator);

        if !self.channel_setting_pages(guild_id, category).is_empty() {
            let (gid, ch) = (guild_id.to_owned(), category.clone());
            out.push(item("Edit category", move |this, window, cx| this.open_channel_settings(ch.clone(), gid.clone(), window, cx)).icon(Glyph::Pen).entry());
        }
        if self.can_in(&category.id, perm::MANAGE_CHANNELS) {
            out.push(self.delete_channel_entry(category, "Delete category"));
            out.push(Entry::Separator);
        }
        if self.developer_mode {
            let id = category.id.clone();
            out.push(item("Copy category ID", move |this, _, cx| this.copy_text(id.clone(), cx)).icon(Glyph::Id).entry());
        }
        out
    }

    fn move_to_category(&self, channel: &Channel, guild_id: &str) -> Entry {
        let categories: Vec<Entry> = self
            .guilds
            .iter()
            .find(|g| g.id == guild_id)
            .map(|g| {
                g.channels
                    .iter()
                    .filter(|c| c.kind == ChannelKind::Category)
                    .map(|c| {
                        let (id, parent) = (channel.id.clone(), c.id.clone());
                        let row = item(c.name.clone(), move |this, _, cx| {
                            let next = Some(parent.clone());
                            this.channel_optimistic(&id, Method::PATCH, json!({ "parent_id": parent }), move |c| c.parent_id = next.clone(), cx)
                        });
                        let row = row.icon(Glyph::Folder);
                        if channel.parent_id.as_ref() == Some(&c.id) { row.disabled() } else { row }.entry()
                    })
                    .collect()
            })
            .unwrap_or_default();
        let id = channel.id.clone();
        let none = item("No category", move |this, _, cx| this.channel_optimistic(&id, Method::PATCH, json!({ "parent_id": null }), |c| c.parent_id = None, cx)).icon(Glyph::Minus);
        let mut rows = vec![if channel.parent_id.is_none() { none.disabled() } else { none }.entry(), Entry::Separator];
        rows.extend(categories);
        submenu("Move to", vec![submenu("Category", rows).icon(Glyph::Folder).entry()]).icon(Glyph::MoveTo).entry()
    }

    // The channel the menu is about: its kind's glyph, its name, and where it sits.
    fn channel_menu_header(&self, channel: &Channel, guild_id: &str) -> Entry {
        let guild = self.guilds.iter().find(|g| g.id == guild_id);
        let category = guild.and_then(|g| g.channels.iter().find(|c| Some(&c.id) == channel.parent_id.as_ref()));
        let place = guild.map(|g| match category {
            Some(category) => format!("{} · {}", g.name, category.name),
            None => g.name.clone(),
        });
        let glyph = match channel.kind {
            ChannelKind::Voice => Glyph::VolumeHigh,
            ChannelKind::Category => Glyph::Folder,
            ChannelKind::Thread => Glyph::Thread,
            _ if channel.private => Glyph::HashtagLock,
            _ => Glyph::Hashtag,
        };
        header(HeadLead::Tile(glyph), channel.name.clone(), place.map(SharedString::from))
    }

    fn delete_channel_entry(&self, channel: &Channel, label: &'static str) -> Entry {
        let (id, name) = (channel.id.clone(), channel.name.clone());
        item(label, move |this, _, cx| {
            let id = id.clone();
            this.confirm_delete(label, format!("Are you sure you want to delete #{name}? This cannot be undone."), label, move |this, _, cx| {
                this.delete_channel_optimistic(&id, cx);
            }, cx);
        })
        .danger()
        .icon(Glyph::Trash)
        .entry()
    }

    fn create_channel(&mut self, guild_id: String, kind: u64, parent: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if kind == CATEGORY {
            return self.prompt("Create category", "Category name", "", "Create", move |this, name, _, cx| this.post_channel(&guild_id, &name, CATEGORY, None, cx), window, cx);
        }
        self.open_create_channel(guild_id, kind, parent, window, cx);
    }

    fn post_channel(&mut self, guild_id: &str, name: &str, kind: u64, parent: Option<String>, cx: &mut Context<Self>) {
        if name.trim().is_empty() {
            return;
        }
        let body = json!({ "name": name.trim(), "type": kind, "parent_id": parent });
        self.rest(Method::POST, format!("/guilds/{guild_id}/channels"), Some(body), |_, _| {}, cx);
    }

    pub(crate) fn show_invite(&mut self, guild_id: String, channel_id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.open_invite_dialog(guild_id, channel_id, window, cx);
    }

}


// Applies a settings PATCH body to our local copy, field by field, as Discord will.
fn merge_settings(settings: &mut GuildSettings, body: &Value) {
    let end = |v: &Value| v["mute_config"]["end_time"].as_str().map(str::to_owned);
    if let Some(muted) = body["muted"].as_bool() {
        settings.muted = muted;
        settings.mute_end = end(body);
        settings.mute_window = body["mute_config"]["selected_time_window"].as_i64();
    }
    if let Some(level) = body["message_notifications"].as_u64() {
        settings.notifications = level as u8;
    }
    if let Some(on) = body["suppress_everyone"].as_bool() {
        settings.suppress_everyone = on;
    }
    if let Some(on) = body["suppress_roles"].as_bool() {
        settings.suppress_roles = on;
    }
    if let Some(value) = body["notify_highlights"].as_u64() {
        settings.notify_highlights = value as u8;
    }
    if let Some(on) = body["mute_scheduled_events"].as_bool() {
        settings.mute_scheduled_events = on;
    }
    if let Some(on) = body["mobile_push"].as_bool() {
        settings.mobile_push = on;
    }
    if let Some(on) = body["hide_muted_channels"].as_bool() {
        settings.hide_muted = on;
    }
    if let Some(flags) = body["flags"].as_u64() {
        settings.flags = flags;
    }
    for (id, change) in body["channel_overrides"].as_object().into_iter().flatten() {
        let channel = settings.channels.entry(id.clone()).or_insert_with(|| crate::discord::ChannelSettings { notifications: 3, ..Default::default() });
        if let Some(muted) = change["muted"].as_bool() {
            channel.muted = muted;
            channel.mute_end = end(change);
        }
        if let Some(level) = change["message_notifications"].as_u64() {
            channel.notifications = level as u8;
        }
        if let Some(flags) = change["flags"].as_u64() {
            channel.flags = flags;
        }
    }
}

// A mute for `seconds` (-1 until turned back on), as a settings PATCH body.
pub(crate) fn mute_body(seconds: i64) -> Value {
    let end = (seconds > 0).then(|| (chrono::Utc::now() + chrono::Duration::seconds(seconds)).to_rfc3339());
    json!({ "muted": true, "mute_config": { "selected_time_window": seconds, "end_time": end } })
}

fn set_joined(this: &mut Waffle, thread_id: &str, joined: bool) {
    for meta in this.guilds.iter_mut().flat_map(|g| g.threads.iter_mut()).filter(|t| t.id == thread_id).filter_map(|t| t.thread.as_mut()) {
        meta.joined = joined;
    }
}

pub(crate) fn rail_key(entry: &crate::RailEntry, guilds: &[crate::discord::Guild]) -> String {
    match entry {
        crate::RailEntry::Guild(i) => guilds[*i].id.clone(),
        crate::RailEntry::Folder(f, _) => format!("folder-{}", f.id),
    }
}

// When a mute ends, as a menu says it: "8:56 PM", or "tomorrow at 4:41 AM", or a date further out.
pub(crate) fn until(end: chrono::DateTime<chrono::Local>, now: chrono::DateTime<chrono::Local>) -> String {
    let time = end.format("%-I:%M %p");
    match (end.date_naive() - now.date_naive()).num_days() {
        0 => time.to_string(),
        1 => format!("tomorrow at {time}"),
        _ => format!("{} at {time}", end.format("%b %-d")),
    }
}

// What a "use the default" level resolves to.
pub(crate) fn effective(level: u8, inherited: u8) -> u8 {
    if level == 3 { inherited } else { level }
}

fn notification_label(level: u8) -> &'static str {
    match level {
        0 => "All messages",
        1 => "Only @mentions",
        2 => "Nothing",
        _ => "Use default",
    }
}

// A channel's flags once pinned or unpinned. Pinning a channel outside our list while the server
// has opt-in on also adds it to the list, as Discord's toggle does.
fn pin_flags(settings: &GuildSettings, channel: &Channel, pin: bool) -> u64 {
    let flags = settings.channel_flags(&channel.id);
    if !pin {
        return flags & !channel_flag::FAVORITED;
    }
    let listed = settings.opted_in(&channel.id) || channel.parent_id.as_ref().is_some_and(|p| settings.opted_in(p));
    match settings.opt_in() && !listed {
        true => flags | channel_flag::FAVORITED | channel_flag::OPT_IN_ENABLED,
        false => flags | channel_flag::FAVORITED,
    }
}

// Removing a channel from the list also unpins it, as Discord's opt-out does.
fn opt_in_flags(flags: u64, on: bool) -> u64 {
    match on {
        true => flags | channel_flag::OPT_IN_ENABLED,
        false => flags & !(channel_flag::OPT_IN_ENABLED | channel_flag::FAVORITED),
    }
}

// The PATCH for ticking or unticking Show all channels. Unticking it (opt-in on) with nothing in the
// list yet also adds every text and voice channel, so nothing vanishes at once.
fn show_all_channels_body(settings: &GuildSettings, channels: &[Channel]) -> Value {
    if settings.opt_in() {
        // Not verified: that Discord sets OPT_IN_CHANNELS_OFF when ticking it back, rather than only clearing ON.
        return json!({ "flags": (settings.flags & !guild_flag::OPT_IN_CHANNELS_ON) | guild_flag::OPT_IN_CHANNELS_OFF });
    }
    let mut body = json!({ "flags": (settings.flags & !guild_flag::OPT_IN_CHANNELS_OFF) | guild_flag::OPT_IN_CHANNELS_ON });
    if settings.channels.values().any(|c| c.flags & channel_flag::OPT_IN_ENABLED != 0) {
        return body;
    }
    // Not verified: Discord does this first opt-in through onboardExistingMember (POST
    // /guilds/{id}/onboarding-responses); setting OPT_IN_ENABLED on each channel is assumed equivalent.
    let overrides: serde_json::Map<String, Value> = channels
        .iter()
        .filter(|c| matches!(c.kind, ChannelKind::Text | ChannelKind::Voice))
        .map(|c| (c.id.clone(), json!({ "flags": settings.channel_flags(&c.id) | channel_flag::OPT_IN_ENABLED })))
        .collect();
    body["channel_overrides"] = Value::Object(overrides);
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(id: &str, kind: u8, parent: Option<&str>) -> Channel {
        crate::discord::parse_channel(&json!({ "id": id, "type": kind, "parent_id": parent }), "g").unwrap()
    }

    fn with_flags(guild: u64, channels: &[(&str, u64)]) -> GuildSettings {
        let mut settings = GuildSettings { flags: guild, ..Default::default() };
        for &(id, flags) in channels {
            settings.channels.insert(id.into(), crate::discord::ChannelSettings { flags, ..Default::default() });
        }
        settings
    }

    #[test]
    fn pinning_adds_to_the_list_only_under_opt_in() {
        let text = channel("t", 0, Some("cat"));
        assert_eq!(pin_flags(&with_flags(0, &[]), &text, true), channel_flag::FAVORITED);
        assert_eq!(pin_flags(&with_flags(guild_flag::OPT_IN_CHANNELS_ON, &[]), &text, true), channel_flag::FAVORITED | channel_flag::OPT_IN_ENABLED);
        let under_listed_category = with_flags(guild_flag::OPT_IN_CHANNELS_ON, &[("cat", channel_flag::OPT_IN_ENABLED)]);
        assert_eq!(pin_flags(&under_listed_category, &text, true), channel_flag::FAVORITED);
        let pinned = with_flags(guild_flag::OPT_IN_CHANNELS_ON, &[("t", channel_flag::FAVORITED | channel_flag::OPT_IN_ENABLED | 1024)]);
        assert_eq!(pin_flags(&pinned, &text, false), channel_flag::OPT_IN_ENABLED | 1024);
        assert_eq!(opt_in_flags(channel_flag::FAVORITED | channel_flag::OPT_IN_ENABLED | 1024, false), 1024);
    }

    #[test]
    fn first_opt_in_lists_every_text_and_voice_channel() {
        let channels = [channel("t", 0, None), channel("v", 2, None), channel("c", 4, None)];
        let body = show_all_channels_body(&with_flags(0, &[("t", channel_flag::FAVORITED)]), &channels);
        assert_eq!(body["flags"], guild_flag::OPT_IN_CHANNELS_ON);
        assert_eq!(body["channel_overrides"]["t"]["flags"], channel_flag::FAVORITED | channel_flag::OPT_IN_ENABLED);
        assert_eq!(body["channel_overrides"]["v"]["flags"], channel_flag::OPT_IN_ENABLED);
        assert!(body["channel_overrides"].get("c").is_none());
        let again = show_all_channels_body(&with_flags(guild_flag::OPT_IN_CHANNELS_OFF, &[("t", channel_flag::OPT_IN_ENABLED)]), &channels);
        assert_eq!(again, json!({ "flags": guild_flag::OPT_IN_CHANNELS_ON }));
        let back = show_all_channels_body(&with_flags(guild_flag::OPT_IN_CHANNELS_ON, &[]), &channels);
        assert_eq!(back, json!({ "flags": guild_flag::OPT_IN_CHANNELS_OFF }));
    }

    #[test]
    fn flag_patches_apply_locally() {
        let mut settings = GuildSettings::default();
        merge_settings(&mut settings, &json!({ "flags": guild_flag::OPT_IN_CHANNELS_ON, "channel_overrides": { "9": { "flags": channel_flag::FAVORITED } } }));
        assert!(settings.opt_in() && settings.pinned("9"));
    }

    #[test]
    fn settings_patches_apply_locally() {
        let mut settings = GuildSettings::default();
        merge_settings(&mut settings, &json!({ "muted": true, "mute_config": { "end_time": "2030-01-01T00:00:00Z" }, "suppress_everyone": true }));
        assert!(settings.muted && settings.suppress_everyone);
        assert_eq!(settings.mute_end.as_deref(), Some("2030-01-01T00:00:00Z"));
        merge_settings(&mut settings, &json!({ "channel_overrides": { "9": { "message_notifications": 2 } } }));
        assert_eq!(settings.channels["9"].notifications, 2);
        assert!(!settings.channels["9"].muted);
    }

    #[test]
    fn notification_dialog_fields_apply_locally() {
        let mut settings = GuildSettings::default();
        assert!(settings.mobile_push);
        merge_settings(&mut settings, &json!({ "notify_highlights": 1, "mute_scheduled_events": true, "mobile_push": false }));
        assert_eq!(settings.notify_highlights, 1);
        assert!(settings.mute_scheduled_events && !settings.mobile_push);
        merge_settings(&mut settings, &mute_body(3600));
        assert_eq!(settings.mute_window, Some(3600));
        assert!(settings.muted && settings.mute_end.is_some());
        merge_settings(&mut settings, &json!({ "muted": false }));
        assert_eq!((settings.muted, settings.mute_window, settings.mute_end.as_deref()), (false, None, None));
    }
}
