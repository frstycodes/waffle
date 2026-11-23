// Thread menus and the threads popout.
use super::*;

impl Waffle {
    // Discord's thread menu (navId "thread-context"): read state, join, close, lock, mute and
    // notifications for the thread, delete, leave, and its ID.
    pub(crate) fn open_thread_menu(&mut self, thread: Channel, guild_id: String, at: Point<Pixels>, cx: &mut Context<Self>) {
        // The thread as it is now (joined, archived, locked change while the menu is open).
        self.open_live_menu(at, move |this| {
            let current = this.guilds.iter().flat_map(|g| g.threads.iter()).find(|t| t.id == thread.id).cloned().or_else(|| this.forum_post(&thread.id)).unwrap_or_else(|| thread.clone());
            this.thread_menu(&current, &guild_id)
        }, cx);
    }

    pub(super) fn thread_menu(&self, thread: &Channel, guild_id: &str) -> Vec<Entry> {
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
        out.extend(self.edit_tags_entry(&thread, manage, owner));
        out.push(Entry::Separator);
        out.extend(self.favorite_add_entry(&thread));
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
        }
        out.extend(self.favorite_remove_entry(&thread));
        out.push(Entry::Separator);
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
            // lint-allow(detached-task): ends when Discord answers the request, and holds only a WeakEntity.
            .detach();
        }, window, cx);
    }
}
