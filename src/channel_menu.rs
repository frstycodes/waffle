// Discord's channel, category, server, folder and server-dropdown menus, item-for-item from its
// menu code (docs/discord-menus-channel-server.md, read from Discord web 2026-11-14). Favorites,
// stages, events, apps and the full server-settings screens aren't built, so their items are left out.
use gpui_kit::{Context, Pixels, Point, SharedString, Window};
use reqwest::Method;
use serde_json::{Value, json};

use crate::{
    Waffle,
    discord::{Channel, ChannelKind, GuildSettings, parse_guild_settings, perm},
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
        cx.notify();
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
                cx.notify();
            })
            .ok();
        })
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
    fn mute_entry(&self, label: &str, unmute: &str, muted: bool, end: Option<&str>, apply: impl Fn(&mut Waffle, Value, &mut Context<Waffle>) + Clone + 'static) -> Entry {
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

        let (gid, cid) = (guild_id.to_owned(), channel.id.clone());
        out.push(item(if voice { "Invite to voice" } else { "Invite people" }, move |this, window, cx| this.show_invite(gid.clone(), cid.clone(), window, cx)).brand().icon(Glyph::GroupPlus).entry());
        let link = format!("https://discord.com/channels/{guild_id}/{}", channel.id);
        out.push(item("Copy link", move |this, _, cx| this.copy_text(link.clone(), cx)).icon(Glyph::Link).entry());
        out.push(Entry::Separator);

        if voice {
            let open = channel.clone();
            out.push(item("Open chat", move |this, window, cx| this.open_channel(open.clone(), window, cx)).icon(Glyph::ChatRound).entry());
            let id = channel.id.clone();
            out.push(item("Hide names", move |this, _, cx| {
                if !this.hidden_voice_names.remove(&id) {
                    this.hidden_voice_names.insert(id.clone());
                }
                cx.notify();
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

        let id = category.id.clone();
        out.push(item("Collapse category", move |this, _, cx| {
            if !this.folded.remove(&id) {
                this.folded.insert(id.clone());
            }
            cx.notify();
        })
        .checked(self.folded.contains(&category.id))
        .icon(Glyph::Collapse)
        .entry());
        let categories: Vec<String> = self.guilds.iter().find(|g| g.id == guild_id).map(|g| g.channels.iter().filter(|c| c.kind == ChannelKind::Category).map(|c| c.id.clone()).collect()).unwrap_or_default();
        if categories.iter().any(|c| !self.folded.contains(c)) {
            out.push(item("Collapse all categories", move |this, _, cx| {
                this.folded.extend(categories.iter().cloned());
                cx.notify();
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

    // Discord's invite modal, simplified: a week-long invite link to copy.
    pub(crate) fn show_invite(&mut self, guild_id: String, channel_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let name = self.guilds.iter().find(|g| g.id == guild_id).map(|g| g.name.clone()).unwrap_or_default();
        cx.spawn_in(window, async move |this, cx| {
            let invite = client.call(Method::POST, format!("/channels/{channel_id}/invites"), Some(json!({ "max_age": 604_800, "max_uses": 0 }))).await;
            this.update_in(cx, |this, window, cx| match invite.ok().and_then(|i| i["code"].as_str().map(|c| format!("https://discord.gg/{c}"))) {
                Some(link) => this.prompt(format!("Invite friends to {name}"), "Send this link to a friend. It expires in 7 days.", &link, "Copy", |this, link, _, cx| this.copy_text(link, cx), window, cx),
                None => this.show_toast("You don't have permission to create invites here", cx),
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn open_server_menu(&mut self, guild_id: String, at: Point<Pixels>, dropdown: bool, cx: &mut Context<Self>) {
        self.open_live_menu(at, move |this| this.server_menu(&guild_id, dropdown), cx);
    }

    // The server icon's menu, or (with `dropdown`) the server name dropdown, which orders the same
    // items Discord's way and drops Mark As Read and the submenus.
    fn server_menu(&self, guild_id: &str, dropdown: bool) -> Vec<Entry> {
        let Some(guild) = self.guilds.iter().find(|g| g.id == guild_id) else { return Vec::new() };
        let Some(me) = self.me.clone() else { return Vec::new() };
        let settings = self.settings_for(guild_id);
        let owner = guild.owner_id.as_deref() == Some(me.id.as_str());
        let manage_channels = guild.can(&me.id, perm::MANAGE_CHANNELS);
        let invite_channel = guild.invite_channel(&me.id).map(|c| c.id.clone());
        let subtitle = owner.then(|| SharedString::from("You own this server"));
        let mut out = vec![header(HeadLead::Server { icon: guild.icon.clone(), name: guild.name.clone() }, guild.name.clone(), subtitle)];

        if !dropdown {
            let gid = guild_id.to_owned();
            let unread = guild.channels.iter().any(|c| self.unread(&c.id) && !settings.channel_muted(&c.id));
            let read = item("Mark as read", move |this, _, cx| this.mark_guild_read(&gid, cx)).icon(Glyph::Checks);
            out.push(if unread { read } else { read.disabled() }.entry());
            out.push(Entry::Separator);
        }
        if let Some(channel) = invite_channel.clone() {
            let gid = guild_id.to_owned();
            out.push(item("Invite people", move |this, window, cx| this.show_invite(gid.clone(), channel.clone(), window, cx)).brand().icon(Glyph::GroupPlus).entry());
        }
        if self.can_open_server_settings(guild_id) {
            let gid = guild_id.to_owned();
            out.push(item("Server settings", move |this, window, cx| this.open_server_settings(gid.clone(), window, cx)).icon(Glyph::Settings).entry());
        }
        if dropdown && manage_channels {
            let gid = guild_id.to_owned();
            out.push(item("Create channel", move |this, window, cx| this.create_channel(gid.clone(), TEXT, None, window, cx)).icon(Glyph::Plus).entry());
            let gid = guild_id.to_owned();
            out.push(item("Create category", move |this, window, cx| this.create_channel(gid.clone(), CATEGORY, None, window, cx)).icon(Glyph::FolderPlus).entry());
        }
        out.push(Entry::Separator);

        let gid = guild_id.to_owned();
        out.push(self.mute_entry("Mute server", "Unmute server", settings.is_muted(), settings.mute_end.as_deref(), move |this, body, cx| this.patch_guild_settings(gid.clone(), body, cx)));
        let gid = guild_id.to_owned();
        out.push(item("Notification settings", move |this, window, cx| this.open_notification_settings(gid.clone(), window, cx)).hint(notification_label(settings.notifications)).icon(Glyph::Bell).entry());
        let gid = guild_id.to_owned();
        let hide = settings.hide_muted;
        out.push(item("Hide muted channels", move |this, _, cx| this.patch_guild_settings(gid.clone(), json!({ "hide_muted_channels": !hide }), cx)).checked(hide).icon(Glyph::EyeSlash).entry());
        if guild.has_feature(crate::joining::features::COMMUNITY) {
            use crate::joining::settings_flags::OPT_IN_CHANNELS_ON;
            let (gid, flags) = (guild_id.to_owned(), settings.flags);
            let show_all = flags & OPT_IN_CHANNELS_ON == 0;
            out.push(item("Show All Channels", move |this, _, cx| this.patch_guild_settings(gid.clone(), json!({ "flags": flags ^ OPT_IN_CHANNELS_ON }), cx)).checked(show_all).icon(Glyph::Eye).entry());
        }
        out.push(Entry::Separator);

        let gid = guild_id.to_owned();
        let current = self.members.get(guild_id).and_then(|m| m.get(&me.id)).map(|u| u.name.clone()).unwrap_or_else(|| me.name.clone());
        out.push(item("Edit per-server profile", move |this, window, cx| this.edit_own_nickname(gid.clone(), current.clone(), window, cx)).icon(Glyph::UserGear).entry());
        if !dropdown && manage_channels {
            let gid = guild_id.to_owned();
            out.push(Entry::Separator);
            out.push(item("Create channel", move |this, window, cx| this.create_channel(gid.clone(), TEXT, None, window, cx)).icon(Glyph::Plus).entry());
            let gid = guild_id.to_owned();
            out.push(item("Create category", move |this, window, cx| this.create_channel(gid.clone(), CATEGORY, None, window, cx)).icon(Glyph::FolderPlus).entry());
        }
        out.push(Entry::Separator);

        if !dropdown {
            out.push(self.move_server(guild_id));
            out.push(Entry::Separator);
        }
        if !owner {
            let (gid, name) = (guild_id.to_owned(), guild.name.clone());
            out.push(
                item("Leave server", move |this, _, cx| {
                    let gid = gid.clone();
                    this.confirm(format!("Leave '{name}'"), format!("Are you sure you want to leave {name}? You won't be able to rejoin this server unless you are re-invited."), "Leave server", move |this, _, cx| {
                        this.leave_guild_optimistic(&gid, cx);
                    }, cx);
                })
                .danger()
                .icon(Glyph::DoorExit)
                .entry(),
            );
            out.push(Entry::Separator);
        }
        if self.developer_mode {
            let id = guild_id.to_owned();
            out.push(item("Copy server ID", move |this, _, cx| this.copy_text(id.clone(), cx)).icon(Glyph::Id).entry());
        }
        out
    }

    fn edit_own_nickname(&mut self, guild_id: String, current: String, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt("Change nickname", "Everyone in this server sees this name.", &current, "Save", move |this, nick, _, cx| {
            let nick = (!nick.trim().is_empty()).then(|| nick.trim().to_owned());
            this.nickname_optimistic(&guild_id, "@me", nick, cx);
        }, window, cx);
    }

    // Discord's "Move to..." for a server: into a folder, out of one, or to either end of the rail.
    fn move_server(&self, guild_id: &str) -> Entry {
        let entries = self.rail_entries();
        let in_folder = self.folders.iter().any(|f| !f.id.is_empty() && f.id != "0" && f.guild_ids.iter().any(|g| g == guild_id));
        let folders: Vec<Entry> = self
            .folders
            .iter()
            .filter(|f| !f.id.is_empty() && f.id != "0")
            .map(|f| {
                let (gid, target) = (guild_id.to_owned(), format!("folder-{}", f.id));
                let name = f.name.clone().unwrap_or_else(|| "Folder".into());
                item(name, move |this, _, cx| this.drop_on_rail(&gid, &target, crate::sortable::Place::Into, cx)).icon(Glyph::Folder).entry()
            })
            .collect();
        let first = entries.first().map(|e| rail_key(e, &self.guilds));
        let last = entries.last().map(|e| rail_key(e, &self.guilds));
        let (gid, gid2, gid3) = (guild_id.to_owned(), guild_id.to_owned(), guild_id.to_owned());
        let last_loose = last.clone();
        let no_folder = item("No folder", move |this, _, cx| {
            if let Some(target) = &last_loose {
                this.drop_on_rail(&gid3, target, crate::sortable::Place::After, cx);
            }
        })
        .icon(Glyph::Minus);
        let mut folder_rows = vec![if in_folder { no_folder } else { no_folder.disabled() }.entry()];
        folder_rows.extend(folders);
        let first_item = item("First in list", move |this, _, cx| {
            if let Some(target) = &first {
                this.drop_on_rail(&gid, target, crate::sortable::Place::Before, cx);
            }
        })
        .icon(Glyph::ArrowLineUp);
        let last_item = item("Last in list", move |this, _, cx| {
            if let Some(target) = &last {
                this.drop_on_rail(&gid2, target, crate::sortable::Place::After, cx);
            }
        })
        .icon(Glyph::ArrowLineDown);
        submenu("Move server", vec![submenu("Folder", folder_rows).icon(Glyph::Folder).entry(), Entry::Separator, first_item.entry(), last_item.entry()]).icon(Glyph::MoveTo).entry()
    }

    pub(crate) fn open_folder_menu(&mut self, folder_id: String, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.open_live_menu(at, move |this| this.folder_menu(&folder_id), cx);
    }

    fn folder_menu(&self, folder_id: &str) -> Vec<Entry> {
        let folder_id = folder_id.to_owned();
        let Some(folder) = self.folders.iter().find(|f| f.id == folder_id).cloned() else { return Vec::new() };
        let mut entries = Vec::new();
        let unread = folder.guild_ids.iter().any(|g| self.guilds.iter().find(|x| &x.id == g).is_some_and(|x| x.channels.iter().any(|c| self.unread(&c.id))));
        let ids = folder.guild_ids.clone();
        let read = item("Mark folder as read", move |this, _, cx| {
            for id in &ids {
                this.mark_guild_read(id, cx);
            }
        })
        .icon(Glyph::Checks);
        entries.push(if unread { read } else { read.disabled() }.entry());
        entries.push(Entry::Separator);
        let rail = self.rail_entries();
        let (first, last) = (rail.first().map(|e| rail_key(e, &self.guilds)), rail.last().map(|e| rail_key(e, &self.guilds)));
        let key = format!("folder-{folder_id}");
        let (k1, k2) = (key.clone(), key.clone());
        entries.push(
            submenu(
                "Move to...",
                vec![
                    item("First in List", move |this, _, cx| {
                        if let Some(t) = &first {
                            this.drop_on_rail(&k1, t, crate::sortable::Place::Before, cx);
                        }
                    })
                    .icon(Glyph::ArrowLineUp)
                    .entry(),
                    item("Last in List", move |this, _, cx| {
                        if let Some(t) = &last {
                            this.drop_on_rail(&k2, t, crate::sortable::Place::After, cx);
                        }
                    })
                    .icon(Glyph::ArrowLineDown)
                    .entry(),
                ],
            )
            .icon(Glyph::MoveTo)
            .entry(),
        );
        entries.push(Entry::Separator);
        let (fid, name) = (folder_id.clone(), folder.name.clone().unwrap_or_default());
        entries.push(item("Folder settings", move |this, window, cx| this.rename_folder(fid.clone(), name.clone(), window, cx)).icon(Glyph::Settings).entry());
        if !self.open_folders.is_empty() {
            entries.push(item("Close all folders", |this, _, cx| {
                this.open_folders.clear();
                cx.notify();
            })
            .icon(Glyph::CollapseAll)
            .entry());
        }
        entries
    }

    // ponytail: Discord's Folder Settings also picks a colour; this renames only.
    fn rename_folder(&mut self, folder_id: String, current: String, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt("Folder settings", "Folder name", &current, "Done", move |this, name, _, cx| {
            if let Some(folder) = this.folders.iter_mut().find(|f| f.id == folder_id) {
                folder.name = (!name.trim().is_empty()).then(|| name.trim().to_owned());
            }
            let folders = this.folders.clone();
            if let Some(client) = this.client.clone() {
                crate::discord::runtime().spawn(async move { client.set_guild_folders(folders).await });
            }
            cx.notify();
        }, window, cx);
    }
}

impl Waffle {
    // Discord's thread menu (navId "thread-context"): read state, join, close, lock, mute and
    // notifications for the thread, delete, leave, and its ID.
    pub(crate) fn open_thread_menu(&mut self, thread: Channel, guild_id: String, at: Point<Pixels>, cx: &mut Context<Self>) {
        // The thread as it is now (joined, archived, locked change while the menu is open).
        self.open_live_menu(at, move |this| {
            let current = this.guilds.iter().flat_map(|g| g.threads.iter()).find(|t| t.id == thread.id).cloned().unwrap_or_else(|| thread.clone());
            this.thread_menu(&current, &guild_id)
        }, cx);
    }

    fn thread_menu(&self, thread: &Channel, guild_id: &str) -> Vec<Entry> {
        let (thread, guild_id) = (thread.clone(), guild_id.to_owned());
        let meta = thread.thread.clone().unwrap_or_default();
        let me = self.me.as_ref().map(|m| m.id.clone()).unwrap_or_default();
        // From the thread itself: forum posts opened from the list may not be in the server's `threads`.
        let manage = self.guilds.iter().find(|g| g.id == guild_id).is_some_and(|g| g.channel_permissions(&me, &thread) & perm::MANAGE_THREADS != 0);
        let owner = meta.owner_id.as_deref() == Some(me.as_str());
        let mut out = vec![self.channel_menu_header(&thread, &guild_id)];

        let id = thread.id.clone();
        let read = item("Mark as read", move |this, _, cx| this.mark_channel_read(&id, cx)).icon(Glyph::Checks);
        out.push(if self.unread(&thread.id) { read } else { read.disabled() }.entry());
        out.push(Entry::Separator);

        if !meta.joined {
            let id = thread.id.clone();
            out.push(item("Join thread", move |this, _, cx| {
                let joined = id.clone();
                this.thread_joined_optimistic(&joined, true, cx);
            })
            .icon(Glyph::SignIn)
            .entry());
        }
        if manage || (owner && !meta.locked) {
            let (id, archived) = (thread.id.clone(), meta.archived);
            out.push(item(if archived { "Open thread" } else { "Close thread" }, move |this, _, cx| {
                this.channel_optimistic(&id, Method::PATCH, json!({ "archived": !archived }), move |c| {
                    if let Some(meta) = c.thread.as_mut() {
                        meta.archived = !archived;
                    }
                }, cx)
            }).icon(Glyph::Archive).entry());
        }
        if manage {
            let (id, locked) = (thread.id.clone(), meta.locked);
            out.push(item(if locked { "Unlock thread" } else { "Lock thread" }, move |this, _, cx| {
                this.channel_optimistic(&id, Method::PATCH, json!({ "locked": !locked }), move |c| {
                    if let Some(meta) = c.thread.as_mut() {
                        meta.locked = !locked;
                    }
                }, cx)
            }).icon(Glyph::HashtagLock).entry());
        }
        let link = format!("https://discord.com/channels/{guild_id}/{}", thread.id);
        out.push(item("Copy link", move |this, _, cx| this.copy_text(link.clone(), cx)).icon(Glyph::Link).entry());
        out.push(Entry::Separator);

        match meta.joined {
            true => {
                let id = thread.id.clone();
                out.push(self.mute_entry("Mute thread", "Unmute thread", meta.muted, None, move |this, body, cx| {
                    let muted = body["muted"].as_bool().unwrap_or(false);
                    this.thread_member_optimistic(&id, body, move |meta| meta.muted = muted, cx);
                }));
                // The level is one of three flag bits; the member's other flag bits are kept.
                let rows: Vec<Entry> = [("All messages", 2u64), ("Only @mentions", 4), ("Nothing", 8)]
                    .into_iter()
                    .map(|(label, flag)| {
                        let (id, flags) = (thread.id.clone(), (meta.flags & !0b1110) | flag);
                        item(label, move |this, _, cx| this.thread_member_optimistic(&id, json!({ "flags": flags }), move |meta| meta.flags = flags, cx))
                            .radio(meta.flags & flag != 0)
                            .entry()
                    })
                    .collect();
                out.push(submenu("Notification settings", rows).icon(Glyph::Bell).entry());
            }
            false => {
                out.push(item("Mute thread", |_, _, _| {}).icon(Glyph::BellSlash).hint("Join the thread to mute it").disabled().entry());
                out.push(item("Notification settings", |_, _, _| {}).icon(Glyph::Bell).disabled().entry());
            }
        }
        out.push(Entry::Separator);

        if manage {
            out.push(self.delete_channel_entry(&thread, "Delete thread"));
            out.push(Entry::Separator);
        }
        if meta.joined {
            let id = thread.id.clone();
            out.push(item("Leave thread", move |this, _, cx| {
                let left = id.clone();
                this.thread_joined_optimistic(&left, false, cx);
            })
            .icon(Glyph::DoorExit)
            .entry());
            out.push(Entry::Separator);
        }
        if self.developer_mode {
            let id = thread.id.clone();
            out.push(item("Copy thread ID", move |this, _, cx| this.copy_text(id.clone(), cx)).icon(Glyph::Id).entry());
        }
        out
    }

    // The header's Threads button: the channel's active threads, each opening on click.
    pub(crate) fn open_threads_popout(&mut self, channel_id: String, at: Point<Pixels>, cx: &mut Context<Self>) {
        let threads: Vec<Channel> = self.guilds.iter().flat_map(|g| g.threads.iter()).filter(|t| t.parent_id.as_ref() == Some(&channel_id) && !t.thread.as_ref().is_some_and(|m| m.archived)).cloned().collect();
        let mut out: Vec<Entry> = threads
            .into_iter()
            .map(|t| {
                let count = t.thread.as_ref().map_or(0, |m| m.message_count);
                let open = t.clone();
                item(t.name.clone(), move |this, window, cx| this.open_channel(open.clone(), window, cx)).hint(format!("{count} {}", if count == 1 { "message" } else { "messages" })).icon(Glyph::Thread).entry()
            })
            .collect();
        if out.is_empty() {
            out.push(item("There are no active threads.", |_, _, _| {}).disabled().entry());
        }
        self.open_menu(at, out, cx);
    }

    // Discord's Create Thread from a message: name it, then open it.
    pub(crate) fn create_thread(&mut self, message: crate::discord::Message, window: &mut Window, cx: &mut Context<Self>) {
        let default: String = message.content.chars().take(40).collect();
        let default = if default.trim().is_empty() { "New thread".to_owned() } else { default };
        self.prompt("Create thread", "Thread name", &default, "Create", move |this, name, window, cx| {
            let Some(client) = this.client.clone() else { return };
            let (channel, id, guild_id) = (message.channel_id.clone(), message.id.clone(), message.guild_id.clone());
            let name = if name.trim().is_empty() { "New thread".to_owned() } else { name.trim().to_owned() };
            cx.spawn_in(window, async move |this, cx| {
                let created = client.call(Method::POST, format!("/channels/{channel}/messages/{id}/threads"), Some(json!({ "name": name, "auto_archive_duration": 4320 }))).await;
                this.update_in(cx, |this, window, cx| match created.ok().and_then(|t| crate::discord::parse_thread(&t, guild_id.as_deref().unwrap_or_default())) {
                    Some(mut thread) => {
                        if let Some(meta) = thread.thread.as_mut() {
                            meta.joined = true;
                        }
                        if let Some(guild_id) = guild_id.clone() {
                            this.on_event(crate::discord::Event::ChannelUpsert { guild_id, channel: thread.clone() }, cx);
                        }
                        this.open_channel(thread, window, cx);
                    }
                    None => this.show_toast("Couldn't create the thread", cx),
                })
                .ok();
            })
            .detach();
        }, window, cx);
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

#[cfg(test)]
mod tests {
    use super::*;

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
