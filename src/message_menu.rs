// Discord's message context menu (navId "message"), item-for-item from its menu code
// (docs/discord-menus-user-message.md §5). Items for features Waffle doesn't have yet (translate,
// bookmarks, reminders, apps, report to mods) are left out. Report is hidden on your own messages and call records;
// GAP: Discord also hides it on system-user messages and type 67 (friend request accepted), which `Message` doesn't carry.
use gpui_kit::{Context, Pixels, Point};
use reqwest::Method;

use crate::{
    EmojiTarget, Waffle,
    discord::{Emoji, Message, SendState, perm},
    icons::Glyph,
    menu::{Entry, item, submenu},
};

// Discord's padding for the frequent-emoji rows when there's no history yet.
pub const QUICK_PAD: [&str; 12] = ["👍", "❤️", "😂", "💯", "😆", "💖", "🔥", "😭", "🙏", "👀", "😮", "🎉"];

impl Waffle {
    pub(crate) fn open_message_menu(&mut self, message: Message, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.open_live_menu(at, move |this| this.message_menu(&message), cx);
    }

    pub(crate) fn message_menu(&self, message: &Message) -> Vec<Entry> {
        if message.state == SendState::Failed {
            let (retry, delete) = (message.clone(), message.clone());
            return vec![
                item("Resend message", move |this, _, cx| this.retry(retry.clone(), cx)).icon(Glyph::Retry).entry(),
                self.action_item(MessageAction::Delete, &delete).entry(),
            ];
        }
        if message.state != SendState::Sent {
            return Vec::new();
        }
        let manage = self.can_in(&message.channel_id, perm::MANAGE_MESSAGES);
        let mut out = Vec::new();

        // G3: reactions.
        let react = |emoji: Emoji, m: &Message| {
            let m = m.clone();
            move |this: &mut Waffle, _: &mut gpui_kit::Window, cx: &mut Context<Waffle>| this.toggle_reaction(&m, emoji.clone(), cx)
        };
        let picker = message.clone();
        let open_picker = move |this: &mut Waffle, window: &mut gpui_kit::Window, cx: &mut Context<Waffle>| this.open_emoji_picker(EmojiTarget::Reaction(Box::new(picker.clone())), window, cx);
        let quick: Vec<_> = self.prefs.frequent_emojis(4, &QUICK_PAD).into_iter().map(|e| (e.clone(), std::rc::Rc::new(react(e, message)) as crate::menu::Act)).collect();
        out.push(Entry::Emojis { quick, more: std::rc::Rc::new(open_picker.clone()) });
        let mut more: Vec<Entry> = self.prefs.frequent_emojis(12, &QUICK_PAD).into_iter().map(|e| item(shortcode(&e), react(e.clone(), message)).emoji(e).entry()).collect();
        more.push(Entry::Separator);
        more.push(item("All reactions", open_picker).icon(Glyph::Search).entry());
        out.push(submenu("Add reaction", more).icon(Glyph::Emoji).entry());
        if let Some(first) = crate::reactions::rail_order(&message.reactions).first() {
            let (m, emoji) = (message.clone(), first.emoji.clone());
            out.push(item("View reactions", move |this, _, cx| this.open_reactions(&m, emoji.clone(), cx)).icon(Glyph::Users).entry());
        }
        out.push(Entry::Separator);

        // G4: primary.
        let facts = self.message_facts(message);
        let action = |a: MessageAction| a.allowed(message, &facts).then(|| self.action_item(a, message).entry());
        out.extend([MessageAction::Edit, MessageAction::Reply, MessageAction::Forward, MessageAction::Thread].into_iter().filter_map(action));
        out.push(Entry::Separator);

        // G5: secondary.
        out.extend([MessageAction::CopyText, MessageAction::Pin, MessageAction::MarkUnread, MessageAction::Save].into_iter().filter_map(action));
        let m = message.clone();
        out.push(item("Copy message link", move |this, _, cx| {
            let link = this.message_link(&m);
            this.copy_text(link, cx)
        }).icon(Glyph::Link).entry());
        if cfg!(target_os = "macos") && !message.content.is_empty() {
            let text = message.content.clone();
            out.push(item("Speak message", move |this, _, _| speak(&text, this.prefs.tts_rate)).icon(Glyph::Soundboard).entry());
        }
        out.push(Entry::Separator);

        // G6: destructive.
        if manage && !message.reactions.is_empty() {
            let per_emoji: Vec<Entry> = message
                .reactions
                .iter()
                .map(|r| {
                    let (m, emoji) = (message.clone(), r.emoji.clone());
                    item(shortcode(&r.emoji), move |this, _, cx| {
                        let path = format!("/channels/{}/messages/{}/reactions/{}", m.channel_id, m.id, emoji.path());
                        let emoji = emoji.clone();
                        this.reactions_optimistic(&m, path, move |r| r.emoji != emoji, cx);
                    })
                    .entry()
                })
                .collect();
            out.push(submenu("Remove reactions", per_emoji).danger().icon(Glyph::SmileyXEyes).entry());
            let m = message.clone();
            out.push(
                item("Remove all reactions", move |this, _, cx| {
                    let m = m.clone();
                    this.confirm("Remove all reactions", "Are you sure you want to remove all the reactions on this message?", "Remove all", move |this, _, cx| {
                        this.reactions_optimistic(&m, format!("/channels/{}/messages/{}/reactions", m.channel_id, m.id), |_| false, cx);
                    }, cx);
                })
                .danger()
                .icon(Glyph::SmileyXEyes)
                .entry(),
            );
        }
        out.extend(action(MessageAction::Delete));
        if !facts.mine && message.call.is_none() {
            let m = message.clone();
            out.push(item("Report message", move |this, _, cx| this.open_report(crate::report::ReportTarget::Message(Box::new(m.clone())), cx)).danger().icon(Glyph::Flag).entry());
        }
        out.push(Entry::Separator);

        // G9: developer.
        out.extend(action(MessageAction::CopyId));
        out
    }
}

// What can be done to one message, from its menu or, while it has keyboard focus, by key. The menu and
// the keys both read `allowed`, so a key never does what the menu wouldn't offer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MessageAction {
    Edit,
    Reply,
    Forward,
    Thread,
    React,
    CopyText,
    Pin,
    MarkUnread,
    Save,
    Delete,
    CopyId,
}

// Who you are to a message and what its channel lets you do.
#[derive(Clone, Copy, Default)]
pub(crate) struct Facts {
    pub mine: bool,
    pub manage: bool,
    pub pin: bool,
    pub in_dm: bool,
    pub threads: bool,
    pub developer: bool,
    // In your saved-for-later messages.
    pub saved: bool,
}

impl MessageAction {
    pub const ALL: [Self; 11] = [Self::Edit, Self::Reply, Self::Forward, Self::Thread, Self::React, Self::CopyText, Self::Pin, Self::MarkUnread, Self::Save, Self::Delete, Self::CopyId];

    pub fn allowed(self, message: &Message, facts: &Facts) -> bool {
        match message.state {
            SendState::Sending => return false,
            SendState::Failed => return self == Self::Delete,
            SendState::Sent => {}
        }
        match self {
            Self::Edit => facts.mine,
            Self::Reply | Self::React | Self::MarkUnread | Self::Save => true,
            Self::Forward => message.call.is_none(),
            Self::Thread => facts.threads && message.thread.is_none(),
            Self::CopyText => !message.content.is_empty(),
            Self::Pin => facts.in_dm || facts.pin,
            Self::Delete => facts.mine || facts.manage,
            Self::CopyId => facts.developer,
        }
    }

    // Keys in Waffle's saved-keybind form, with `secondary` for ⌘ on macOS and Ctrl elsewhere; the first
    // is the one the menu shows. Discord's own (support article 1500000056121): e, r, +, Backspace, p,
    // ⌘/Ctrl+C, Alt+Enter. The rest aren't in it: d (Waffle's own delete, and d again confirms it), f, a, t, c, s,
    // Delete, ⇧ to skip the delete confirm, ⇧⌘C.
    pub const fn keys(self) -> &'static [&'static str] {
        match self {
            Self::Edit => &["e"],
            Self::Reply => &["r"],
            Self::Forward => &["f"],
            Self::Thread => &["t"],
            Self::React => &["plus", "a"],
            Self::CopyText => &["secondary+c", "c"],
            Self::Pin => &["p"],
            Self::MarkUnread => &["alt+enter"],
            Self::Save => &["s"],
            Self::Delete => &["d", "backspace", "delete", "shift+d", "shift+backspace", "shift+delete"],
            Self::CopyId => &["shift+secondary+c"],
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Edit => "Edit message",
            Self::Reply => "Reply",
            Self::Forward => "Forward",
            Self::Thread => "Create thread",
            Self::React => "Add reaction",
            Self::CopyText => "Copy text",
            Self::Pin => "Pin message",
            Self::MarkUnread => "Mark unread",
            Self::Save => "Save message",
            Self::Delete => "Delete message",
            Self::CopyId => "Copy message ID",
        }
    }

    pub fn for_combo(combo: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.keys().contains(&combo))
    }
}

impl Waffle {
    pub(crate) fn message_facts(&self, message: &Message) -> Facts {
        let in_text = self.guilds.iter().flat_map(|g| g.channels.iter()).any(|c| c.id == message.channel_id && c.kind == crate::discord::ChannelKind::Text);
        let can = |bit| self.can_in(&message.channel_id, bit);
        Facts {
            mine: self.me.as_ref().is_some_and(|me| me.id == message.author.id),
            manage: can(perm::MANAGE_MESSAGES),
            pin: can(perm::PIN_MESSAGES),
            in_dm: self.dms.iter().any(|c| c.id == message.channel_id),
            threads: in_text && can(perm::CREATE_PUBLIC_THREADS),
            developer: self.developer_mode,
            saved: self.saved_ids.contains(&message.id),
        }
    }

    fn action_item(&self, action: MessageAction, message: &Message) -> crate::menu::Item {
        let glyph = match action {
            MessageAction::Edit => Glyph::Pen,
            MessageAction::Reply => Glyph::Reply,
            MessageAction::Forward => Glyph::Forward,
            MessageAction::Thread => Glyph::Thread,
            MessageAction::React => Glyph::Emoji,
            MessageAction::CopyText => Glyph::Copy,
            MessageAction::Pin if message.pinned => Glyph::PinSlash,
            MessageAction::Pin => Glyph::Pin,
            MessageAction::MarkUnread => Glyph::Envelope,
            MessageAction::Save => Glyph::Bookmark,
            MessageAction::Delete => Glyph::Trash,
            MessageAction::CopyId => Glyph::Id,
        };
        let label = match action {
            MessageAction::Pin if message.pinned => "Unpin message",
            MessageAction::Save if self.saved_ids.contains(&message.id) => "Remove from saved",
            _ => action.label(),
        };
        let m = message.clone();
        let row = item(label, move |this, window, cx| this.run_message_action(action, &m, false, window, cx)).icon(glyph).shortcut(action.keys()[0]);
        if action == MessageAction::Delete { row.danger() } else { row }
    }

    // `quick` skips Delete's confirmation, as Shift does in Discord.
    pub(crate) fn run_message_action(&mut self, action: MessageAction, message: &Message, quick: bool, window: &mut gpui_kit::Window, cx: &mut Context<Self>) {
        let m = message.clone();
        match action {
            MessageAction::Edit => self.start_edit(m, window, cx),
            MessageAction::Reply => self.start_reply(m, window, cx),
            MessageAction::Forward => self.open_forward(m, window, cx),
            MessageAction::Thread => self.create_thread(m, window, cx),
            MessageAction::React => self.open_emoji_picker(EmojiTarget::Reaction(Box::new(m)), window, cx),
            MessageAction::CopyText => self.copy_text(m.content, cx),
            // Unpinning asks first, as Discord does; Shift skips it, from a key or a click.
            MessageAction::Pin if m.pinned && !quick && !window.modifiers().shift => {
                self.confirm("Unpin Message", "You sure you want to remove this pinned message?", "Remove it please!", move |this, _, cx| this.toggle_pin(&m, cx), cx);
                if let Some(dialog) = &mut self.dialog {
                    dialog.danger = true;
                }
                self.set_dialog_preview(message.clone(), "You can hold down shift when clicking unpin message to bypass this confirmation entirely.");
            }
            MessageAction::Pin => self.toggle_pin(&m, cx),
            MessageAction::MarkUnread => self.mark_unread(&m, cx),
            MessageAction::Save => self.toggle_saved(&m, cx),
            MessageAction::Delete if quick || m.state == SendState::Failed => self.delete(m, cx),
            MessageAction::Delete => {
                self.confirm_delete("Delete message", "Are you sure you want to delete this message?", "Delete", move |this, _, cx| this.delete(m.clone(), cx), cx);
                self.set_dialog_key(MessageAction::Delete.keys()[0]);
            }
            MessageAction::CopyId => self.copy_text(m.id, cx),
        }
    }

    // Discord's Save for later: PUT saves, DELETE removes, shown at once and undone if refused.
    fn toggle_saved(&mut self, m: &Message, cx: &mut Context<Self>) {
        let saved = !self.saved_ids.contains(&m.id);
        let method = if saved { Method::PUT } else { Method::DELETE };
        let path = format!("/users/@me/saved-messages/{}/{}", m.channel_id, m.id);
        // Not verified: Discord's own client sends `source`; the endpoint is read from its bundle.
        let body = saved.then(|| serde_json::json!({ "due_at": null }));
        let (id1, id2, message) = (m.id.clone(), m.id.clone(), m.clone());
        self.rest_optimistic(
            method,
            path,
            body,
            move |this| {
                match saved {
                    true => {
                        std::rc::Rc::make_mut(&mut this.saved_ids).insert(id1.clone());
                        if let Some(Some(Ok(list))) = &mut this.saved {
                            list.insert(0, message.clone());
                        }
                    }
                    false => {
                        std::rc::Rc::make_mut(&mut this.saved_ids).remove(&id1);
                        if let Some(Some(Ok(list))) = &mut this.saved {
                            list.retain(|s| s.id != id1);
                        }
                    }
                }
            },
            move |this| {
                match saved {
                    true => std::rc::Rc::make_mut(&mut this.saved_ids).remove(&id2),
                    false => std::rc::Rc::make_mut(&mut this.saved_ids).insert(id2.clone()),
                };
            },
            cx,
        );
    }

    fn toggle_pin(&mut self, m: &Message, cx: &mut Context<Self>) {
        let method = if m.pinned { Method::DELETE } else { Method::PUT };
        let path = format!("/channels/{}/messages/pins/{}", m.channel_id, m.id);
        let (c1, i1, c2, i2, pinned) = (m.channel_id.clone(), m.id.clone(), m.channel_id.clone(), m.id.clone(), method == Method::PUT);
        self.rest_optimistic(
            method,
            path,
            None,
            move |this| {
                if let Some(m) = this.message_mut(&c1, &i1) {
                    m.pinned = pinned;
                }
                // ponytail: an unpin leaves the open pins list; a rollback doesn't put it back until it reopens.
                if let Some(Some(Ok(pins))) = &mut this.pins
                    && !pinned
                {
                    pins.retain(|p| p.id != i1);
                }
            },
            move |this| {
                if let Some(m) = this.message_mut(&c2, &i2) {
                    m.pinned = !pinned;
                }
            },
            cx,
        );
    }
}

impl Waffle {
    // Removing reactions shows at once (`keep` decides which stay); they come back if refused.
    fn reactions_optimistic(&mut self, message: &Message, path: String, keep: impl Fn(&crate::discord::Reaction) -> bool + 'static, cx: &mut Context<Self>) {
        let before = self.message_mut(&message.channel_id, &message.id).map(|m| m.reactions.clone()).unwrap_or_default();
        self.reactors.retain(|(id, _), _| *id != message.id);
        let (c1, i1, c2, i2) = (message.channel_id.clone(), message.id.clone(), message.channel_id.clone(), message.id.clone());
        self.rest_optimistic(
            Method::DELETE,
            path,
            None,
            move |this| {
                if let Some(m) = this.message_mut(&c1, &i1) {
                    m.reactions.retain(|r| keep(r));
                }
            },
            move |this| {
                if let Some(m) = this.message_mut(&c2, &i2) {
                    m.reactions = before;
                }
            },
            cx,
        );
    }
}

// How a reaction reads in a menu row: `:fire:` for Unicode, `:name:` for a server's own.
pub(crate) fn shortcode(emoji: &Emoji) -> String {
    let name = emojis::get(&emoji.name).and_then(|e| e.shortcode()).unwrap_or(&emoji.name);
    format!(":{name}:")
}

// Discord's "Speak message", through the system voice. `rate` is Discord's text-to-speech rate, 1 normal.
pub(crate) fn speak(text: &str, rate: f32) {
    let text = text.to_owned();
    // `say` reads 175 words a minute as normal. It takes 0 as its default speed, so the slowest end of
    // Discord's range speaks at 1, its slowest.
    let wpm = ((175. * rate).round() as u32).max(1);
    // Through stdin, so text starting with "-" can't be read as `say` options.
    std::thread::spawn(move || {
        use std::io::Write;
        let Ok(mut child) = std::process::Command::new("say").arg("-r").arg(wpm.to_string()).stdin(std::process::Stdio::piped()).spawn() else { return };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
    });
}
