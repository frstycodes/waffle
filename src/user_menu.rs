// Discord's user context menus, built item-for-item from its own menu code (docs/discord-menus-user-
// message.md, read from Discord web 2026-11-14). Each item carries Discord's show-condition; items
// for features Waffle doesn't have (activities, apps, stages, mod view, E2EE codes) are left out.
use gpui_kit::{Context, Pixels, Point, Window};
use reqwest::Method;
use serde_json::json;

use crate::{
    Waffle,
    discord::{ChannelKind, Relation, User, perm},
    icons::Glyph,
    menu::{Entry, HeadLead, header, item, slider, submenu},
};

// Where the menu was opened, which decides Discord's variant and its flags.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    // Server chat: author name or avatar, member list (`chat`).
    ServerChat,
    // Server voice row or call tile (`media`).
    ServerVoice,
    // A 1:1 DM: the DM list row or a DM author.
    Dm,
    // No channel: the Friends list.
    Generic,
}

// Discord's timeout lengths, in seconds.
const TIMEOUTS: [(&str, i64); 6] = [("60 seconds", 60), ("5 minutes", 300), ("10 minutes", 600), ("1 hour", 3600), ("1 day", 86_400), ("1 week", 604_800)];

impl Waffle {
    pub(crate) fn open_user_menu(&mut self, user: User, guild_id: Option<String>, surface: Surface, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.open_live_menu(at, move |this| this.user_menu(&user, guild_id.as_deref(), surface, at), cx);
    }

    // Discord's DM row menu: read state, the DM user's items, then Close DM and the channel's ID.
    pub(crate) fn open_dm_menu(&mut self, channel: crate::discord::Channel, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.open_live_menu(at, move |this| this.dm_menu(&channel, at), cx);
    }

    fn dm_menu(&self, channel: &crate::discord::Channel, at: Point<Pixels>) -> Vec<Entry> {
        let mut entries = Vec::new();
        let unread = self.unread(&channel.id);
        let id = channel.id.clone();
        let read = item("Mark as read", move |this, _, cx| this.mark_channel_read(&id, cx)).icon(Glyph::Checks);
        entries.push(if unread { read } else { read.disabled() }.entry());
        entries.push(Entry::Separator);
        if let Some(user) = channel.recipient.clone() {
            entries.extend(self.user_menu(&user, None, Surface::Dm, at).into_iter().filter(|e| !e.is_copy_id()));
        }
        let id = channel.id.clone();
        entries.push(item("Close DM", move |this, _, cx| this.close_dm(&id, cx)).icon(Glyph::X).entry());
        entries.push(Entry::Separator);
        if self.developer_mode {
            if let Some(user) = &channel.recipient {
                let uid = user.id.clone();
                entries.push(item("Copy user ID", move |this, _, cx| this.copy_text(uid.clone(), cx)).icon(Glyph::Id).entry());
            }
            let cid = channel.id.clone();
            entries.push(item("Copy channel ID", move |this, _, cx| this.copy_text(cid.clone(), cx)).icon(Glyph::Id).entry());
        }
        entries
    }

    // Closes a DM (Discord's DELETE on the channel only hides it from the list for us).
    pub(crate) fn close_dm(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        let id = channel_id.to_owned();
        self.dms.retain(|d| d.id != id);
        if self.channel.as_ref().is_some_and(|c| c.id == id) {
            self.open_home(cx);
        }
        self.rest(Method::DELETE, format!("/channels/{id}"), None, |_, _| {}, cx);
    }

    // An author's name or avatar in chat: the server menu in a server, the DM menu in a DM.
    pub(crate) fn open_author_menu(&mut self, user: User, at: Point<Pixels>, cx: &mut Context<Self>) {
        let surface = if self.guild_id.is_some() { Surface::ServerChat } else { Surface::Dm };
        let guild_id = self.guild_id.clone();
        self.open_user_menu(user, guild_id, surface, at, cx);
    }

    pub(crate) fn relation(&self, user_id: &str) -> Option<Relation> {
        self.relationships.iter().find(|r| r.user.id == user_id).map(|r| r.kind)
    }

    // Discord's canManageUser: the permission, and my highest role above theirs (or I own the server).
    fn can_manage(&self, guild_id: &str, target: &str, bit: u64) -> bool {
        let (Some(guild), Some(me)) = (self.guilds.iter().find(|g| g.id == guild_id), self.me.as_ref()) else { return false };
        if !guild.can(&me.id, bit) || guild.owner_id.as_deref() == Some(target) {
            return false;
        }
        let theirs = self.member_roles.get(guild_id).and_then(|r| r.get(target)).map(|roles| guild.roles.iter().filter(|r| roles.contains(&r.id)).map(|r| r.position).max().unwrap_or(0)).unwrap_or(0);
        guild.top_position(&me.id) > theirs
    }

    fn user_menu(&self, user: &User, guild_id: Option<&str>, surface: Surface, at: Point<Pixels>) -> Vec<Entry> {
        let Some(me) = self.me.clone() else { return Vec::new() };
        let target = user.id.clone();
        let is_me = target == me.id;
        let media = surface == Surface::ServerVoice;
        let in_server = matches!(surface, Surface::ServerChat | Surface::ServerVoice);
        let guild = guild_id.filter(|_| in_server).and_then(|id| self.guilds.iter().find(|g| g.id == id));
        let relation = self.relation(&target);
        let (friend, blocked) = (relation == Some(Relation::Friend), relation == Some(Relation::Blocked));
        let ignored = self.ignored.contains(&target);
        let name = user.name.clone();
        let mut out: Vec<Entry> = vec![self.user_header(user, guild)];

        // G2: profile and social.
        let profile_user = user.clone();
        out.push(item("Profile", move |this, _, cx| this.open_profile(profile_user.clone(), at, cx)).icon(Glyph::UserCircle).entry());
        if surface == Surface::ServerChat && self.channel.is_some() {
            let mention = user.clone();
            out.push(item("Mention", move |this, window, cx| this.mention_user(&mention, window, cx)).icon(Glyph::At).entry());
        }
        if !is_me && surface != Surface::Dm {
            let id = target.clone();
            out.push(item("Message", move |this, window, cx| this.message_user(id.clone(), window, cx)).icon(Glyph::ChatRound).entry());
        }
        let in_dm_call = self.call.as_ref().is_some_and(|c| c.channel.kind == ChannelKind::Dm && c.channel.recipient.as_ref().is_some_and(|r| r.id == target));
        if !is_me && !user.bot && !in_dm_call {
            let id = target.clone();
            let start = item("Call", move |this, window, cx| this.start_call(id.clone(), window, cx)).icon(Glyph::Phone);
            out.push(if blocked { start.disabled() } else { start }.entry());
        }
        if !is_me {
            let (id, who) = (target.clone(), name.clone());
            out.push(item("Add note", move |this, window, cx| this.edit_note(id.clone(), who.clone(), window, cx)).hint("Only you can see it").icon(Glyph::Note).entry());
        }
        if friend {
            let (id, who) = (target.clone(), name.clone());
            out.push(item("Add friend nickname", move |this, window, cx| this.edit_friend_nickname(id.clone(), who.clone(), window, cx)).icon(Glyph::Tag).entry());
        }
        out.push(Entry::Separator);

        // G3: volume.
        if media && !is_me {
            let volume = self.prefs.user_volumes.get(&target).copied().unwrap_or(1.);
            let id = target.clone();
            out.push(slider("User volume", format!("user-volume-{target}"), volume / 2., move |this, f, cx| this.set_user_volume(&id, f * 2., cx)));
            out.push(Entry::Separator);
        }

        // G5: media and social actions.
        if media && is_me {
            out.push(item("Mute", |this, _, cx| this.toggle_mute(cx)).checked(self.muted || self.deafened).icon(Glyph::MicOff).entry());
            out.push(item("Deafen", |this, _, cx| this.toggle_deafen(cx)).checked(self.deafened).icon(Glyph::HeadphonesOff).entry());
        }
        if media && !is_me {
            let id = target.clone();
            out.push(item("Mute", move |this, _, cx| this.toggle_local_mute(&id, cx)).checked(self.prefs.local_mutes.contains(&target)).icon(Glyph::MicOff).entry());
            let id = target.clone();
            out.push(
                item("Mute soundboard", move |this, _, cx| this.update_prefs(cx, |p| toggle(&mut p.soundboard_mutes, &id))).checked(self.prefs.soundboard_mutes.contains(&target)).icon(Glyph::SoundboardOff).entry(),
            );
            let id = target.clone();
            out.push(item("Hide video", move |this, _, cx| this.update_prefs(cx, |p| toggle(&mut p.hidden_video, &id))).checked(self.prefs.hidden_video.contains(&target)).icon(Glyph::VideoOff).entry());
        }
        if let Some(guild) = guild {
            let gid = guild.id.clone();
            if is_me {
                let current = self.members.get(&gid).and_then(|m| m.get(&me.id)).map(|u| u.name.clone()).unwrap_or_default();
                out.push(item("Edit per-server profile", move |this, window, cx| this.edit_nickname(gid.clone(), "@me".into(), current.clone(), window, cx)).icon(Glyph::UserGear).entry());
            } else if self.can_manage(&guild.id, &target, perm::MANAGE_NICKNAMES) {
                let (id, current) = (target.clone(), name.clone());
                out.push(item("Change nickname", move |this, window, cx| this.edit_nickname(gid.clone(), id.clone(), current.clone(), window, cx)).icon(Glyph::Signature).entry());
            }
        }
        if !is_me && !user.bot && !blocked && !ignored {
            let invites: Vec<Entry> = self
                .guilds
                .iter()
                .filter(|g| Some(g.id.as_str()) != guild_id && g.invite_channel(&me.id).is_some())
                .map(|g| {
                    let (gid, uid) = (g.id.clone(), target.clone());
                    item(g.name.clone(), move |this, window, cx| this.invite_to_server(gid.clone(), uid.clone(), window, cx)).server(g.icon.clone(), g.name.clone()).entry()
                })
                .collect();
            if !invites.is_empty() {
                out.push(submenu("Invite to server", invites).icon(Glyph::PaperPlane).entry());
            }
        }
        if !is_me && !user.bot {
            let id = target.clone();
            let who = name.clone();
            out.push(match relation {
                Some(Relation::Friend) => item("Remove friend", move |this, _, cx| {
                    let id = id.clone();
                    this.confirm(format!("Remove '{who}'"), format!("Are you sure you want to remove {who} from your friends?"), "Remove friend", move |this, _, cx| {
                        this.drop_relation(&id, cx);
                    }, cx);
                })
                .icon(Glyph::UserMinus)
                .entry(),
                Some(Relation::Incoming) => {
                    let friend = user.clone();
                    item("Accept friend request", move |this, _, cx| this.befriend(friend.clone(), cx)).icon(Glyph::GroupPlus).entry()
                }
                Some(Relation::Outgoing) => item("Friend request sent", |_, _, _| {}).icon(Glyph::GroupPlus).disabled().entry(),
                Some(Relation::Blocked) => item("Add friend", |_, _, _| {}).icon(Glyph::GroupPlus).disabled().entry(),
                None => {
                    let friend = user.clone();
                    item("Add friend", move |this, _, cx| this.befriend(friend.clone(), cx)).icon(Glyph::GroupPlus).entry()
                }
            });
        }
        if !is_me && !blocked {
            let (id, who) = (target.clone(), name.clone());
            out.push(match ignored {
                true => item("Unignore", move |this, _, cx| this.set_ignored(id.clone(), false, cx)).icon(Glyph::EyeSlash).entry(),
                false => item("Ignore", move |this, _, cx| {
                    let id = id.clone();
                    this.confirm(format!("Ignore {who}?"), "You won't see their messages or be notified by them, and they won't know.", "Ignore", move |this, _, cx| this.set_ignored(id.clone(), true, cx), cx);
                })
                .icon(Glyph::EyeSlash)
                .entry(),
            });
        }
        if !is_me {
            let (id, blockee) = (target.clone(), user.clone());
            out.push(match blocked {
                true => item("Unblock", move |this, _, cx| this.drop_relation(&id, cx)).icon(Glyph::Prohibit).entry(),
                false => item("Block", move |this, _, cx| this.ask_block(blockee.clone(), cx))
                .danger()
                .icon(Glyph::Prohibit)
                .entry(),
            });
        }
        out.push(Entry::Separator);

        // G6: roles and voice moves.
        let voice = guild.and_then(|g| g.voice_states.iter().find(|v| v.user_id == target).map(|v| (g, v.clone())));
        if let Some(guild) = guild {
            let theirs = self.member_roles.get(&guild.id).and_then(|r| r.get(&target)).cloned().unwrap_or_default();
            let manage = guild.can(&me.id, perm::MANAGE_ROLES);
            let top = guild.top_position(&me.id);
            let mut roles: Vec<_> = guild.roles.iter().filter(|r| r.id != guild.id && !r.managed).collect();
            roles.sort_by_key(|r| std::cmp::Reverse(r.position));
            let rows: Vec<Entry> = roles
                .into_iter()
                .filter(|r| theirs.contains(&r.id) || (manage && r.position < top))
                .map(|role| {
                    let has = theirs.contains(&role.id);
                    let dot = if role.color == 0 { None } else { Some(role.color) };
                    if !manage {
                        return item(role.name.clone(), |_, _, _| {}).dot(dot).entry();
                    }
                    let (gid, uid, rid) = (guild.id.clone(), target.clone(), role.id.clone());
                    let row = item(role.name.clone(), move |this, _, cx| {
                        let method = if has { Method::DELETE } else { Method::PUT };
                        let (g1, u1, r1, g2, u2, r2) = (gid.clone(), uid.clone(), rid.clone(), gid.clone(), uid.clone(), rid.clone());
                        this.rest_optimistic(
                            method,
                            format!("/guilds/{gid}/members/{uid}/roles/{rid}"),
                            None,
                            move |this| this.set_role(&g1, &u1, &r1, !has),
                            move |this| this.set_role(&g2, &u2, &r2, has),
                            cx,
                        );
                    })
                    .checked(has)
                    .dot(dot);
                    if role.position >= top { row.disabled().hint("Above your highest role") } else { row }.entry()
                })
                .collect();
            if !rows.is_empty() {
                let label = if rows.len() == 1 { "Role" } else { "Roles" };
                let mut held: Vec<_> = guild.roles.iter().filter(|r| theirs.contains(&r.id)).collect();
                held.sort_by_key(|r| std::cmp::Reverse(r.position));
                let names = held.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", ");
                let roles = submenu(label, rows).icon(Glyph::Shield);
                out.push(if names.is_empty() { roles } else { roles.hint(names) }.entry());
            }
        }
        if let Some((guild, state)) = &voice {
            let channels: Vec<Entry> = guild
                .channels
                .iter()
                .filter(|c| c.kind == ChannelKind::Voice && state.channel_id.as_ref() != Some(&c.id) && self.can_move_voice(&guild.id, &target, &c.id))
                .map(|c| {
                    let (gid, uid, channel) = (guild.id.clone(), target.clone(), c.clone());
                    item(c.name.clone(), move |this, _, cx| {
                        match is_me {
                            true => this.join_voice(channel.clone(), cx),
                            false => {
                                let drag = crate::drag::VoiceDrag { user_id: uid.clone(), guild_id: gid.clone(), name: Default::default(), avatar: String::new() };
                                this.drop_voice_user(&drag, channel.clone(), cx);
                            }
                        }
                    })
                    .icon(Glyph::VolumeHigh)
                    .entry()
                })
                .collect();
            if !channels.is_empty() {
                out.push(submenu("Move to", channels).icon(Glyph::MoveTo).entry());
            }
        }
        out.push(Entry::Separator);

        // G8: moderation.
        if let Some((guild, state)) = &voice {
            let gid = guild.id.clone();
            // Voice moderation is per channel: overwrites on the channel they're in apply.
            let can = |bit| state.channel_id.as_ref().is_some_and(|c| self.can_in(c, bit));
            if can(perm::MUTE_MEMBERS) {
                let (gid, uid, on) = (gid.clone(), target.clone(), state.server_muted);
                out.push(item("Server mute", move |this, _, cx| this.server_voice_optimistic(&gid, &uid, json!({ "mute": !on }), |v| v.server_muted = !v.server_muted, cx)).checked(on).hint(format!("Mutes {name} for everyone")).icon(Glyph::MicOff).entry());
            }
            if can(perm::DEAFEN_MEMBERS) {
                let (gid, uid, on) = (gid.clone(), target.clone(), state.server_deafened);
                out.push(item("Server deafen", move |this, _, cx| this.server_voice_optimistic(&gid, &uid, json!({ "deaf": !on }), |v| v.server_deafened = !v.server_deafened, cx)).checked(on).icon(Glyph::HeadphonesOff).entry());
            }
            if !is_me && can(perm::MOVE_MEMBERS) {
                let (gid, uid) = (gid.clone(), target.clone());
                out.push(item("Disconnect", move |this, _, cx| this.server_voice_optimistic(&gid, &uid, json!({ "channel_id": null }), |v| v.channel_id = None, cx)).danger().icon(Glyph::PhoneDisconnect).entry());
            }
        }
        if let Some(guild) = guild.filter(|_| !is_me) {
            let theirs = self.member_roles.get(&guild.id).and_then(|r| r.get(&target)).cloned().unwrap_or_default();
            let admin = guild.roles.iter().any(|r| theirs.contains(&r.id) && r.permissions & perm::ADMINISTRATOR != 0);
            if !admin && self.can_manage(&guild.id, &target, perm::MODERATE_MEMBERS) {
                let lengths: Vec<Entry> = TIMEOUTS
                    .iter()
                    .map(|&(label, seconds)| {
                        let (gid, uid) = (guild.id.clone(), target.clone());
                        item(label, move |this, _, cx| {
                            let until = (chrono::Utc::now() + chrono::Duration::seconds(seconds)).to_rfc3339();
                            this.rest(Method::PATCH, format!("/guilds/{gid}/members/{uid}"), Some(json!({ "communication_disabled_until": until })), |_, _| {}, cx);
                        })
                        .entry()
                    })
                    .collect();
                out.push(submenu(format!("Timeout {name}"), lengths).danger().icon(Glyph::Timer).entry());
            }
            if self.can_manage(&guild.id, &target, perm::KICK_MEMBERS) {
                let (gid, uid, who) = (guild.id.clone(), target.clone(), name.clone());
                out.push(
                    item(format!("Kick {name}"), move |this, _, cx| {
                        let (gid, uid) = (gid.clone(), uid.clone());
                        this.confirm(format!("Kick '{who}' from Server"), format!("Are you sure you want to kick {who} from the server? They will be able to rejoin again with a new invite."), "Kick", move |this, _, cx| {
                            this.rest(Method::DELETE, format!("/guilds/{gid}/members/{uid}"), None, |_, _| {}, cx);
                        }, cx);
                    })
                    .danger()
                    .icon(Glyph::DoorOpen)
                    .entry(),
                );
            }
            if !user.bot && self.can_manage(&guild.id, &target, perm::BAN_MEMBERS) {
                let (gid, uid, who) = (guild.id.clone(), target.clone(), name.clone());
                out.push(
                    item(format!("Ban {name}"), move |this, _, cx| {
                        let (gid, uid) = (gid.clone(), uid.clone());
                        this.confirm(format!("Ban '{who}'"), format!("Are you sure you want to ban {who}? They won't be able to rejoin unless unbanned."), "Ban", move |this, _, cx| {
                            this.rest(Method::PUT, format!("/guilds/{gid}/bans/{uid}"), Some(json!({ "delete_message_seconds": 0 })), |_, _| {}, cx);
                        }, cx);
                    })
                    .danger()
                    .icon(Glyph::Gavel)
                    .entry(),
                );
            }
        }
        out.push(Entry::Separator);

        // G9: developer.
        if self.developer_mode {
            let id = target.clone();
            out.push(item("Copy user ID", move |this, _, cx| this.copy_text(id.clone(), cx)).icon(Glyph::Id).entry());
        }
        out
    }

    // Who the menu is about: their avatar and presence, their name, and in a call where they are.
    fn user_header(&self, user: &User, guild: Option<&crate::discord::Guild>) -> Entry {
        let status = self.presences.get(&user.id).map(|p| p.mark()).or(self.me.as_ref().filter(|me| me.id == user.id).map(|_| self.my_status.into()));
        let voice = guild.and_then(|g| g.voice_states.iter().find(|v| v.user_id == user.id).and_then(|v| g.channels.iter().find(|c| v.channel_id.as_ref() == Some(&c.id))));
        let speaking = self.call.as_ref().is_some_and(|c| c.speaking.contains(&user.id));
        let subtitle = voice.map(|c| if speaking { format!("In {} · speaking", c.name) } else { format!("In {}", c.name) });
        header(HeadLead::Avatar { url: user.avatar.clone(), status }, user.name.clone(), subtitle.map(Into::into))
    }

    // A friend request, or accepting theirs: Discord takes the same PUT for both.
    pub(crate) fn befriend(&mut self, user: User, cx: &mut Context<Self>) {
        let next = if self.relation(&user.id) == Some(Relation::Incoming) { Relation::Friend } else { Relation::Outgoing };
        let id = user.id.clone();
        self.relation_optimistic(&id, Some((user, next)), Method::PUT, Some(json!({})), cx);
    }

    // Unfriends, unblocks, or cancels or ignores a request.
    pub(crate) fn drop_relation(&mut self, user_id: &str, cx: &mut Context<Self>) {
        self.relation_optimistic(user_id, None, Method::DELETE, None, cx);
    }

    pub(crate) fn ask_block(&mut self, user: User, cx: &mut Context<Self>) {
        let title = format!("Block {}?", user.name);
        self.confirm(title, "They won't be able to message you, and their messages will be hidden.", "Block", move |this, _, cx| {
            let id = user.id.clone();
            this.relation_optimistic(&id, Some((user.clone(), Relation::Blocked)), Method::PUT, Some(json!({ "type": 2 })), cx);
        }, cx);
    }

    // Friends, blocks and requests change at once; Discord's refusal restores the old relationship.
    pub(crate) fn relation_optimistic(&mut self, user_id: &str, next: Option<(User, Relation)>, method: Method, body: Option<serde_json::Value>, cx: &mut Context<Self>) {
        let before = self.relationships.iter().find(|r| r.user.id == user_id).cloned();
        let (a, b) = (user_id.to_owned(), user_id.to_owned());
        self.rest_optimistic(
            method,
            format!("/users/@me/relationships/{user_id}"),
            body,
            move |this| {
                this.relationships.retain(|r| r.user.id != a);
                if let Some((user, kind)) = next {
                    this.relationships.push(crate::discord::Relationship { user, kind, since: None });
                }
            },
            move |this| {
                this.relationships.retain(|r| r.user.id != b);
                this.relationships.extend(before);
            },
            cx,
        );
    }

    pub(crate) fn set_role(&mut self, guild_id: &str, user_id: &str, role_id: &str, has: bool) {
        let roles = self.member_roles.entry(guild_id.to_owned()).or_default().entry(user_id.to_owned()).or_default();
        roles.retain(|r| r != role_id);
        if has {
            roles.push(role_id.to_owned());
        }
        if self.me.as_ref().is_some_and(|m| m.id == user_id) {
            let roles = roles.clone();
            if let Some(guild) = self.guilds.iter_mut().find(|g| g.id == guild_id) {
                guild.my_roles = roles;
            }
        }
    }

    // A moderator's change to someone's voice state (server mute, deafen, disconnect), shown at once.
    fn server_voice_optimistic(&mut self, guild_id: &str, user_id: &str, body: serde_json::Value, change: impl Fn(&mut crate::discord::VoiceState) + 'static, cx: &mut Context<Self>) {
        let before = self.guilds.iter().find(|g| g.id == guild_id).and_then(|g| g.voice_states.iter().find(|v| v.user_id == user_id)).cloned();
        let (g1, u1, g2, u2) = (guild_id.to_owned(), user_id.to_owned(), guild_id.to_owned(), user_id.to_owned());
        self.rest_optimistic(
            Method::PATCH,
            format!("/guilds/{guild_id}/members/{user_id}"),
            Some(body),
            move |this| {
                if let Some(state) = this.guilds.iter_mut().find(|g| g.id == g1).and_then(|g| g.voice_states.iter_mut().find(|v| v.user_id == u1)) {
                    change(state);
                }
            },
            move |this| {
                if let (Some(before), Some(state)) = (before, this.guilds.iter_mut().find(|g| g.id == g2).and_then(|g| g.voice_states.iter_mut().find(|v| v.user_id == u2))) {
                    *state = before;
                }
            },
            cx,
        );
    }

    // A nickname shows at once (member list, messages); `user_id` "@me" means us.
    pub(crate) fn nickname_optimistic(&mut self, guild_id: &str, user_id: &str, nick: Option<String>, cx: &mut Context<Self>) {
        let real_id = if user_id == "@me" { self.me.as_ref().map(|m| m.id.clone()).unwrap_or_default() } else { user_id.to_owned() };
        let account = self.relationships.iter().map(|r| &r.user).chain(self.me.iter()).find(|u| u.id == real_id).map(|u| u.name.clone());
        let before = self.members.get(guild_id).and_then(|m| m.get(&real_id)).cloned();
        let shown = nick.clone().or(account).or_else(|| before.as_ref().map(|u| u.name.clone())).unwrap_or_default();
        let (g1, u1, g2) = (guild_id.to_owned(), real_id.clone(), guild_id.to_owned());
        self.rest_optimistic(
            Method::PATCH,
            format!("/guilds/{guild_id}/members/{user_id}"),
            Some(json!({ "nick": nick })),
            move |this| {
                if let Some(member) = this.members.get_mut(&g1).and_then(|m| m.get_mut(&u1)) {
                    member.name = shown;
                }
                this.apply_members(&g1);
            },
            move |this| {
                if let Some(before) = before {
                    this.members.entry(g2.clone()).or_default().insert(before.id.clone(), before);
                }
                this.apply_members(&g2);
            },
            cx,
        );
    }

    fn edit_note(&mut self, user_id: String, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt(format!("Note for {name}"), "Only visible to you.", "", "Save", move |this, note, _, cx| {
            this.rest(Method::PUT, format!("/users/@me/notes/{user_id}"), Some(json!({ "note": note })), |_, _| {}, cx);
        }, window, cx);
    }

    fn edit_friend_nickname(&mut self, user_id: String, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt("Add friend nickname", format!("A name only you see for {name}."), "", "Save", move |this, nick, _, cx| {
            let nickname = (!nick.trim().is_empty()).then(|| nick.trim().to_owned());
            this.rest(Method::PATCH, format!("/users/@me/relationships/{user_id}"), Some(json!({ "nickname": nickname })), |_, _| {}, cx);
        }, window, cx);
    }

    // `user_id` is "@me" for our own nickname, as Discord's API spells it.
    fn edit_nickname(&mut self, guild_id: String, user_id: String, current: String, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt("Change nickname", "Everyone in this server sees this name.", &current, "Save", move |this, nick, _, cx| {
            let nick = (!nick.trim().is_empty()).then(|| nick.trim().to_owned());
            this.nickname_optimistic(&guild_id, &user_id, nick, cx);
        }, window, cx);
    }

    fn set_ignored(&mut self, user_id: String, ignore: bool, cx: &mut Context<Self>) {
        let method = if ignore { Method::PUT } else { Method::DELETE };
        let (a, b) = (user_id.clone(), user_id.clone());
        let flip = |this: &mut Waffle, id: String, on: bool| {
            match on {
                true => this.ignored.insert(id),
                false => this.ignored.remove(&id),
            };
        };
        self.rest_optimistic(method, format!("/users/@me/relationships/{user_id}/ignore"), None, move |this| flip(this, a, ignore), move |this| flip(this, b, !ignore), cx);
    }

    // Opens (or creates) our DM with `user_id` and joins its call; it rings once connected.
    pub(crate) fn start_call(&mut self, user_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        cx.spawn_in(window, async move |this, cx| {
            let Ok(dm) = client.open_dm(&user_id).await else { return };
            this.update_in(cx, |this, window, cx| {
                if !this.dms.iter().any(|d| d.id == dm.id) {
                    this.dms.insert(0, dm.clone());
                }
                this.guild_id = None;
                this.open_channel(dm.clone(), window, cx);
                this.ring_on_connect = Some(dm.id.clone());
                this.join_voice(dm, cx);
            })
            .ok();
        })
        .detach();
    }

    // Discord's "Invite to Server": a single-use invite to that server, sent to their DMs.
    fn invite_to_server(&mut self, guild_id: String, user_id: String, _: &mut Window, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let Some(me) = self.me.as_ref() else { return };
        let Some(channel) = self.guilds.iter().find(|g| g.id == guild_id).and_then(|g| g.invite_channel(&me.id)).map(|c| c.id.clone()) else { return };
        cx.spawn(async move |this, cx| {
            let result = async {
                let invite = client.call(Method::POST, format!("/channels/{channel}/invites"), Some(json!({ "max_uses": 1, "unique": true }))).await?;
                let code = invite["code"].as_str().ok_or("Discord didn't return an invite")?.to_owned();
                let dm = client.open_dm(&user_id).await?;
                client.call(Method::POST, format!("/channels/{}/messages", dm.id), Some(json!({ "content": format!("https://discord.gg/{code}") }))).await?;
                Ok::<_, String>(())
            }
            .await;
            this.update(cx, |this, cx| match result {
                Ok(()) => this.show_toast("Invite sent", cx),
                Err(error) => this.show_toast(error, cx),
            })
            .ok();
        })
        .detach();
    }
}

fn toggle(list: &mut Vec<String>, id: &str) {
    match list.iter().position(|x| x == id) {
        Some(i) => {
            list.remove(i);
        }
        None => list.push(id.to_owned()),
    }
}
