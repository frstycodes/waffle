// The server dropdown and its folder menus.
use super::*;

impl Waffle {
    pub(crate) fn open_server_menu(&mut self, guild_id: String, at: Point<Pixels>, dropdown: bool, cx: &mut Context<Self>) {
        self.open_live_menu(at, move |this| this.server_menu(&guild_id, dropdown), cx);
    }

    // The server icon's menu, or (with `dropdown`) the server name dropdown, which orders the same
    // items Discord's way and drops Mark As Read and the submenus.
    pub(super) fn server_menu(&self, guild_id: &str, dropdown: bool) -> Vec<Entry> {
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
        out.extend(self.create_event_entry(guild_id).filter(|_| dropdown));
        out.push(Entry::Separator);

        let gid = guild_id.to_owned();
        out.push(self.mute_entry("Mute server", "Unmute server", settings.is_muted(), settings.mute_end.as_deref(), move |this, body, cx| this.patch_guild_settings(gid.clone(), body, cx)));
        out.push(self.server_notify_entry(guild_id, &settings, dropdown));
        let gid = guild_id.to_owned();
        let hide = settings.hide_muted;
        out.push(item("Hide muted channels", move |this, _, cx| this.patch_guild_settings(gid.clone(), json!({ "hide_muted_channels": !hide }), cx)).checked(hide).icon(Glyph::EyeSlash).entry());
        if guild.has_feature(crate::joining::features::COMMUNITY) {
            let gid = guild_id.to_owned();
            out.push(item("Show all channels", move |this, _, cx| this.toggle_show_all_channels(&gid, cx)).checked(!settings.opt_in()).icon(Glyph::Eye).entry());
        }
        out.extend(dropdown.then(|| self.privacy_entry(guild_id)));
        out.push(Entry::Separator);
        out.extend((!dropdown).then(|| self.privacy_entry(guild_id)));

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
        out.extend(self.create_event_entry(guild_id).filter(|_| !dropdown));
        out.push(Entry::Separator);

        if !dropdown {
            out.push(self.move_server(guild_id));
            out.push(Entry::Separator);
        }
        out.extend(self.safety_entries(guild, dropdown));
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

    // The server icon's Notification Settings is Discord's submenu: the level, the suppress switches, then
    // mobile push. The name dropdown's opens the dialog.
    fn server_notify_entry(&self, guild_id: &str, settings: &GuildSettings, dropdown: bool) -> Entry {
        let gid = guild_id.to_owned();
        if dropdown {
            return item("Notification settings", move |this, window, cx| this.open_notification_settings(gid.clone(), window, cx)).hint(notification_label(settings.notifications)).icon(Glyph::Bell).entry();
        }
        let mut rows = self.notification_radios(settings.notifications, None, move |this, level, cx| this.patch_guild_settings(gid.clone(), json!({ "message_notifications": level }), cx));
        rows.push(Entry::Separator);
        let highlights_off = settings.notify_highlights == crate::notification_settings::HIGHLIGHTS_DISABLED;
        let highlights = if highlights_off { crate::notification_settings::HIGHLIGHTS_ENABLED } else { crate::notification_settings::HIGHLIGHTS_DISABLED };
        let switches = [
            ("Suppress @everyone and @here", settings.suppress_everyone, json!({ "suppress_everyone": !settings.suppress_everyone })),
            ("Suppress All Role @mentions", settings.suppress_roles, json!({ "suppress_roles": !settings.suppress_roles })),
            ("Suppress Highlights", highlights_off, json!({ "notify_highlights": highlights })),
            ("Mute New Events", settings.mute_scheduled_events, json!({ "mute_scheduled_events": !settings.mute_scheduled_events })),
        ];
        for (label, on, body) in switches {
            let gid = guild_id.to_owned();
            rows.push(item(label, move |this, _, cx| this.patch_guild_settings(gid.clone(), body.clone(), cx)).checked(on).entry());
        }
        rows.push(Entry::Separator);
        let (gid, push) = (guild_id.to_owned(), settings.mobile_push);
        rows.push(item("Mobile Push Notifications", move |this, _, cx| this.patch_guild_settings(gid.clone(), json!({ "mobile_push": !push }), cx)).checked(push).entry());
        submenu("Notification settings", rows).hint(notification_label(settings.notifications)).icon(Glyph::Bell).entry()
    }

    // Discord's Privacy Settings for one server. GAP: Discord opens a dialog that also has Message Requests
    // and Activity Joining; Waffle shows the two settings it keeps as a submenu.
    fn privacy_entry(&self, guild_id: &str) -> Entry {
        let account = &self.account_settings;
        let dms = !account.restricted_guilds.iter().any(|g| g == guild_id);
        let activity = !account.activity_restricted_guilds.iter().any(|g| g == guild_id);
        let gid = guild_id.to_owned();
        let direct = item("Direct Messages", move |this, _, cx| {
            let before = this.account_settings.clone();
            let list = &mut this.account_settings.restricted_guilds;
            if !list.iter().any(|g| g == &gid) {
                list.push(gid.clone());
            } else {
                list.retain(|g| g != &gid);
            }
            let patch = json!({ "restricted_guilds": this.account_settings.restricted_guilds });
            this.save_account_settings(before, patch, cx);
        })
        .hint("Allow DMs from other members in this server")
        .checked(dms)
        .entry();
        let gid = guild_id.to_owned();
        let status = item("Activity Status", move |this, _, cx| this.toggle_guild_activity(&gid, cx)).checked(activity).entry();
        submenu("Privacy settings", vec![direct, status]).icon(Glyph::Shield).entry()
    }

    // A Community server's Security Actions, and in the name dropdown Report Raid, for its moderators.
    // Not verified: Discord's exact permission for Security Actions; this takes the same set as Report Raid.
    // GAP: Security Actions opens the Invites page, where Pause invites lives, not Discord's dialog; Report Raid
    // shows whether or not a raid is already reported, since Waffle doesn't track incidents, and its confirm
    // copy is Waffle's own (Discord's dialog was not read).
    fn safety_entries(&self, guild: &crate::discord::Guild, dropdown: bool) -> Vec<Entry> {
        use crate::server_settings::{access::Page, perms::MANAGE_GUILD};
        let me = self.my_id();
        let moderator = [perm::BAN_MEMBERS, perm::KICK_MEMBERS, perm::MODERATE_MEMBERS, MANAGE_GUILD].iter().any(|&bit| guild.can(me, bit));
        if !guild.has_feature(crate::joining::features::COMMUNITY) || !moderator {
            return Vec::new();
        }
        let gid = guild.id.clone();
        let mut out = vec![item("Security Actions", move |this, window, cx| {
            this.open_server_settings(gid.clone(), window, cx);
            this.open_server_page(Page::Invites, cx);
        })
        .danger()
        .icon(Glyph::Shield)
        .entry()];
        if dropdown {
            let gid = guild.id.clone();
            out.push(item("Report Raid", move |this, _, cx| {
                let gid = gid.clone();
                this.confirm("Report Raid", "Let Discord know this server is being raided.", "Report Raid", move |this, _, cx| {
                    this.rest(Method::POST, format!("/guilds/{gid}/auto-moderation/report-raid"), Some(json!({})), |this, cx| this.show_toast("Raid reported", cx), cx);
                }, cx);
            })
            .danger()
            .icon(Glyph::Shield)
            .entry());
        }
        out
    }

    // Discord's PinChannel: not for threads or categories (callers never pass those).
    pub(super) fn pin_channel_entry(&self, channel: &Channel, guild_id: &str) -> Entry {
        let settings = self.settings_for(guild_id);
        let pinned = settings.pinned(&channel.id);
        let (gid, cid) = (guild_id.to_owned(), channel.id.clone());
        let flags = pin_flags(&settings, channel, !pinned);
        let row = item(if pinned { "Unpin channel" } else { "Pin channel to top" }, move |this, _, cx| this.set_channel_flags(gid.clone(), &cid, flags, cx));
        row.icon(if pinned { Glyph::PinSlash } else { Glyph::Pin }).entry()
    }

    // Discord's ChannelListOptIn: only while a Community server has opt-in on. Under an opted-in
    // category a channel offers to unfollow the category instead.
    pub(super) fn channel_list_entry(&self, channel: &Channel, guild_id: &str) -> Option<Entry> {
        let community = self.guilds.iter().find(|g| g.id == guild_id).is_some_and(|g| g.has_feature(crate::joining::features::COMMUNITY));
        let settings = self.settings_for(guild_id);
        if !community || !settings.opt_in() {
            return None;
        }
        let parent = channel.parent_id.clone().filter(|p| settings.opted_in(p));
        let (target, label, on) = match parent {
            Some(parent) => (parent, "Unfollow channel category", false),
            None if settings.opted_in(&channel.id) => (channel.id.clone(), "Remove from channel list", false),
            None => (channel.id.clone(), "Add to channel list", true),
        };
        let flags = opt_in_flags(settings.channel_flags(&target), on);
        let gid = guild_id.to_owned();
        let row = item(label, move |this, _, cx| this.set_channel_flags(gid.clone(), &target, flags, cx));
        Some(row.icon(if on { Glyph::Eye } else { Glyph::EyeSlash }).entry())
    }

    // One channel override's flags, for a server or (under "@me") a DM.
    pub(crate) fn set_channel_flags(&mut self, guild_key: String, channel_id: &str, flags: u64, cx: &mut Context<Self>) {
        let mut overrides = serde_json::Map::new();
        overrides.insert(channel_id.to_owned(), json!({ "flags": flags }));
        self.patch_guild_settings(guild_key, json!({ "channel_overrides": overrides }), cx);
    }

    pub(super) fn toggle_show_all_channels(&mut self, guild_id: &str, cx: &mut Context<Self>) {
        let Some(guild) = self.guilds.iter().find(|g| g.id == guild_id) else { return };
        let body = show_all_channels_body(&self.settings_for(guild_id), &guild.channels);
        self.patch_guild_settings(guild_id.to_owned(), body, cx);
    }

    pub(super) fn edit_own_nickname(&mut self, guild_id: String, current: String, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt("Change nickname", "Everyone in this server sees this name.", &current, "Save", move |this, nick, _, cx| {
            let nick = (!nick.trim().is_empty()).then(|| nick.trim().to_owned());
            this.nickname_optimistic(&guild_id, "@me", nick, cx);
        }, window, cx);
    }

    // Discord's "Move to..." for a server: into a folder, out of one, or to either end of the rail.
    pub(super) fn move_server(&self, guild_id: &str) -> Entry {
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

    pub(super) fn folder_menu(&self, folder_id: &str) -> Vec<Entry> {
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
                this.redraw(Column::Nav, cx);
            })
            .icon(Glyph::CollapseAll)
            .entry());
        }
        entries
    }

    // ponytail: Discord's Folder Settings also picks a colour; this renames only.
    pub(super) fn rename_folder(&mut self, folder_id: String, current: String, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt("Folder settings", "Folder name", &current, "Done", move |this, name, _, cx| {
            if let Some(folder) = this.folders.iter_mut().find(|f| f.id == folder_id) {
                folder.name = (!name.trim().is_empty()).then(|| name.trim().to_owned());
            }
            let folders = this.folders.clone();
            if let Some(client) = this.client.clone() {
                crate::discord::runtime().spawn(async move { client.set_guild_folders(folders).await });
            }
            this.redraw(Column::Nav, cx);
        }, window, cx);
    }
}
