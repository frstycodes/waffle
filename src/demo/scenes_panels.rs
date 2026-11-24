use super::*;

pub(super) fn panel_scenes(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    match scene {
        // Menus from the context-menu audit: the server icon's Notification settings submenu, an activity
        // card's ⋯, a Friends row's ⋯, a picture's message menu and the message box's.
        "server-menu-notify" => {
            waffle.open_server_menu("1".into(), demo_at(60., 130.), false, cx);
            waffle.open_submenu_named("Notification settings");
        }
        "activity-menu" => {
            let rich = crate::discord::RichPresence { kind: crate::discord::ActivityKind::Playing, name: "Minecraft".into(), ..Default::default() };
            waffle.open_activity_menu(user("8388608", "Wren"), rich, demo_at(640., 260.), cx);
        }
        "friend-row-menu" => waffle.open_friend_row_menu(user("8388608", "Wren"), demo_at(900., 200.), cx),
        "image-menu" => {
            let Some(message) = waffle.feeds.get("general").and_then(|f| f.messages.iter().find(|m| m.id == "102").cloned()) else { return };
            let target = Some(crate::message_menu::MenuTarget::Image(avatar_url("3", None)));
            waffle.open_message_menu_on(message, target, demo_at(700., 120.), cx);
        }
        "text-box-menu" => waffle.open_text_box_menu(demo_at(600., 700.), cx),
        // The channel header's bell with its menu open.
        "notify-bell" => {
            if let Some(channel) = waffle.channel.clone() {
                waffle.open_notify_menu(channel, "1".into(), demo_at(1180., 40.), cx);
            }
        }
        // The server's Notification Settings: a category on Nothing, a muted channel, @everyone suppressed;
        // `-muted` adds an eight-hour server mute, which locks the levels, highlights and push.
        "notification-settings" | "notification-settings-muted" => {
            use crate::discord::{ChannelSettings, GuildSettings};
            let mut settings = GuildSettings { notifications: 1, suppress_everyone: true, ..Default::default() };
            settings.channels.insert("c1".into(), ChannelSettings { notifications: 2, muted: false, mute_end: None, flags: 0 });
            settings.channels.insert("media".into(), ChannelSettings { notifications: 3, muted: true, mute_end: None, flags: 0 });
            settings.channels.insert("long".into(), ChannelSettings { notifications: 0, muted: false, mute_end: None, flags: 0 });
            if scene == "notification-settings-muted" {
                settings.muted = true;
                settings.mute_window = Some(28_800);
                settings.mute_end = Some((chrono::Utc::now() + chrono::Duration::hours(8)).to_rfc3339());
            }
            waffle.guild_settings.insert("1".into(), settings);
            waffle.open_notification_settings("1".into(), window, cx);
        }
        // A jump landing: the message centred, filled and squiggled.
        "jump" => waffle.jump_to("103".into(), cx),
        // The rail's Inbox on each tab: mentions under their filter chips, and the unread channels with the
        // newest cached messages of one.
        "inbox-saved" => {
            waffle.saved = Some(Some(Ok(self::messages().into_iter().rev().take(3).collect())));
            waffle.inbox_tab = crate::inbox::InboxTab::Saved;
            waffle.inbox_open = true;
        }
        "games" => waffle.open_settings(crate::settings::Page::ActivityPrivacy, window, cx),
        "profile-editor" => waffle.demo_profile_editor(crate::settings::ProfileTab::Main, window, cx),
        "profile-editor-server" => waffle.demo_profile_editor(crate::settings::ProfileTab::Server, window, cx),
        // Where I share activity: one server restricted, one name long enough to truncate.
        "activity-privacy" => {
            if let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "4") {
                guild.name = "Garden of the Very Long Server Name That Never Ends".into();
            }
            waffle.account_settings.activity_restricted_guilds = vec!["3".into()];
            waffle.account_settings.default_guilds_activity_restricted = 1;
            waffle.open_settings(crate::settings::Page::ActivityPrivacy, window, cx);
        }
        "inbox" | "inbox-unreads" => {
            let all = self::messages();
            waffle.inbox = Some(Some(Ok(all.iter().filter(|m| !m.mentions.is_empty() || m.content.contains("@")).cloned().collect())));
            let staff: Vec<Message> = all.iter().rev().take(2).map(|m| Message { channel_id: "staff".into(), id: format!("9{}", m.id), ..m.clone() }).collect();
            waffle.feeds.insert("staff".into(), Feed { messages: std::sync::Arc::new(staff), loaded: true, ..Default::default() });
            waffle.inbox_tab = if scene == "inbox" { crate::inbox::InboxTab::Mentions } else { crate::inbox::InboxTab::Unreads };
            waffle.inbox_open = true;
        }
        // The Inbox on Mentions just after the first mention was marked read: it's gone, and the next card
        // shows as hovered, Jump and Mark As Read up (overlays.rs reads the scene name for that).
        "inbox-mark-read" => {
            let mentions: Vec<Message> = self::messages().into_iter().filter(|m| !m.mentions.is_empty() || m.content.contains("@")).collect();
            let first = mentions.first().map(|m| m.id.clone());
            waffle.inbox = Some(Some(Ok(mentions)));
            waffle.inbox_tab = crate::inbox::InboxTab::Mentions;
            waffle.inbox_open = true;
            if let Some(id) = first {
                waffle.mark_mention_read(id, cx);
            }
        }
        // Read to the end, then Mark Unread on Zoe's mention: the NEW line moves above it and the channel
        // counts that mention again.
        "mark-unread" => {
            let last = waffle.feeds.get("general").and_then(|f| f.messages.iter().map(|m| crate::order(&m.id)).max()).unwrap_or(0);
            waffle.last_message.insert("general".into(), last);
            waffle.read_states.insert("general".into(), ReadState { last_read: last, mentions: 0 });
            let marked = waffle.feeds.get("general").and_then(|f| f.messages.iter().find(|m| m.id == "104").cloned());
            if let Some(message) = marked {
                waffle.mark_unread(&message, cx);
            }
        }
        // A bare `@` in the composer: every member, in the suggestion popup.
        // Typed once the window is up: the composer's events aren't wired while the app is being built.
        // "-tab" / "-enter": the first one accepted through that key's own action, then an x typed at the caret.
        "mention" | "mention-tab" | "mention-enter" => {
            let accept: Option<Box<dyn gpui_kit::Action>> = match scene {
                "mention-tab" => Some(Box::new(gpui_kit::component::input::IndentInline)),
                "mention-enter" => Some(Box::new(gpui_kit::component::input::Enter { secondary: false, shift: false })),
                _ => None,
            };
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(500)).await;
                this.update_in(cx, |this, window, cx| {
                    this.composer.update(cx, |state, cx| {
                        state.set_value("@", window, cx);
                        state.focus(window, cx);
                    });
                    this.autocomplete = this.suggest("@");
                    cx.notify();
                })
                .ok();
                let Some(accept) = accept else { return };
                cx.background_executor().timer(std::time::Duration::from_millis(700)).await;
                this.update_in(cx, |_, window, cx| window.dispatch_action(accept, cx)).ok();
                cx.background_executor().timer(std::time::Duration::from_millis(300)).await;
                this.update_in(cx, |this, window, cx| this.composer.update(cx, |state, cx| state.insert("x", window, cx))).ok();
            })
            .detach() // lint-allow(detached-task): one-shot fixture step; ends after one timer and update, or when the view is gone
        }
        "send-options" | "tts" => demo_send_options(waffle, scene, window, cx),
        "hover" => waffle.hovered_message = Some("105".into()),
        // Keyboard focus on the code message; "-menu" also opens your own message's menu, with its key caps.
        // "-delete": D on your own message, its confirm up with the D cap.
        "message-focus" | "message-focus-menu" | "message-focus-delete" => {
            waffle.focus_message("105".into(), window, cx);
            let mine = waffle.feeds.get("general").and_then(|f| f.messages.iter().find(|m| m.id == "103").cloned());
            if let Some(message) = mine.as_ref().filter(|_| scene == "message-focus-delete") {
                waffle.run_message_action(crate::message_menu::MessageAction::Delete, message, false, window, cx);
            }
            if scene == "message-focus-menu" {
                waffle.developer_mode = true;
                if let Some(message) = mine {
                    waffle.open_message_menu(message, demo_at(560., 120.), cx);
                }
            }
        }
        // Wren's voice menu. As owner, Move to lists study-hall; "-denied" makes us a Council moderator whose
        // role study-hall denies Move members, so Move to is gone while mute, deafen and disconnect stay.
        "user-menu-voice" | "user-menu-voice-denied" => {
            use crate::discord::{Overwrite, VIEW_CHANNEL, perm};
            if scene == "user-menu-voice-denied"
                && let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1")
            {
                guild.owner_id = Some("8388608".into());
                guild.my_roles = vec!["r1".into()];
                for role in guild.roles.iter_mut().filter(|r| r.id == "r1") {
                    role.permissions = VIEW_CHANNEL | perm::CONNECT | perm::MOVE_MEMBERS | perm::MUTE_MEMBERS | perm::DEAFEN_MEMBERS;
                }
                for hub in guild.channels.iter_mut().filter(|c| c.id == "hub") {
                    hub.overwrites.push(Overwrite { id: "r1".into(), allow: 0, deny: perm::MOVE_MEMBERS, member: false });
                }
            }
            waffle.open_user_menu(user("8388608", "Wren"), Some("1".into()), crate::user_menu::Surface::ServerVoice, demo_at(300., 300.), cx);
        }
        // As the server's owner, someone else's message as REST history loads it (no guild_id): Delete, Pin
        // and the reaction removals are on offer.
        "message-menu-theirs" => {
            let theirs = waffle.feeds.get("general").and_then(|f| f.messages.iter().find(|m| m.id == "102").cloned());
            if let Some(message) = theirs {
                waffle.open_message_menu(Message { guild_id: None, ..message }, demo_at(560., 120.), cx);
            }
        }
        "search" | "search-from" | "search-date" => demo_search(waffle, scene, window, cx),
        "search-results" | "search-filters" => demo_search_results(waffle, scene == "search-filters", window, cx),
        "search-dm" | "search-dm-results" => demo_search_dm(waffle, scene == "search-dm-results", window, cx),
        // A long message.txt (Discord's file for a message past 2,000 characters), a short code file, and
        // a .log that isn't text, which stays a file card. The fixtures stand in for the CDN fetch.
        "text-files" | "text-expanded" | "text-viewer" => demo_text_files(waffle, scene),
        // Voice messages (fresh, one left part-way, a short one, one missing its waveform and length) and two
        // audio files, all playing made-up tones.
        "voice-messages" => demo_voice_messages(waffle),
        // Made-up files of each kind waiting in the composer (a picture, a spoilered one, a video, a PDF,
        // an archive, a long name), one upload part way and one Discord refused. `-drop` holds the drop
        // zone up as if files were dragged over.
        "attachments" | "attachments-drop" => demo_attachments(waffle, window, cx),
        // Alt text and spoilers (gap 1.18): three files waiting (a picture, a spoilered one with a
        // description, a PDF), your own two described pictures and someone's single described one, with
        // Show image descriptions on.
        "uploads" => demo_uploads(waffle, window, cx),
        // Discord's own links: a message card, a channel chip, invites to a joined server, another server
        // and an expired code, a message you can't see (no card), and two still loading.
        "link-previews" => demo_link_previews(waffle),
        "forwarded" => demo_forwarded(waffle),
        "favorites" => demo_favorites(waffle, window, cx),
        // An app thinking, its reply, an ephemeral reply, one with Discord's reason line, and a command it
        // never answered.
        "app-replies" => demo_app_replies(waffle),
        // Every button style (one disabled, one spinning at its own width, one failed, a premium one priced
        // and one unavailable), a user select starting at its default, a voice-only channel select, and a
        // multi-select open with two picks and no Done (`-users`: the user select open, its search on top).
        "components" | "components-users" => demo_components(waffle, scene == "components-users", window, cx),
        // An accented container with a section (its `-#` subtext, reduced text), a wide thumbnail kept at its
        // shape, separator and buttons; a gallery container at its items' own sizes with a spoilered item, a
        // file with its size and a spoilered one; a section with a spoilered thumbnail; a lone gallery item
        // spanning its container; a spoiler container.
        "components-v2" => demo_components_v2(waffle),
        scene if scene.starts_with("announcements") => demo_announcements(waffle, scene, window, cx),
        // An upload, a link image, a GIF, an animated emoji, a sticker and spoilers, with uploads kept out of
        // line, GIFs still until hovered, stickers moving on hover and spoilers always shown.
        "chat-media-settings" => demo_media_settings(waffle),
        // Your message held hovered, so its article card shows the ✕ without the pointer on it.
        "embed-suppress" => waffle.hovered_message = Some("301".into()),
        // Report Message on someone else's message, through a sample menu (Discord's real one is server-sent).
        // `-submit` stops at Submit with Also block ticked; `-sent` is the thank-you node; `-user` reports a person.
        // Signed out, nothing is sent.
        "report" | "report-submit" | "report-sent" | "report-user" => demo_report(waffle, scene, cx),
        // The dialog with #📷-media picked and a note; gpui's channel reads only, so it shows locked.
        "forward-dialog" => {
            if let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "2") {
                guild.owner_id = Some("1".into());
                guild.roles = vec![Role { id: "2".into(), name: "@everyone".into(), color: 0, position: 0, permissions: crate::discord::VIEW_CHANNEL, managed: false, icon: None }];
            }
            let Some(message) = waffle.feeds.get("general").and_then(|f| f.messages.iter().find(|m| m.id == "102").cloned()) else { return };
            waffle.open_forward(message, window, cx);
            waffle.toggle_forward_place("media".into(), "#📷-media".into(), cx);
            if let Some(dialog) = &waffle.forward_dialog {
                dialog.note.update(cx, |state, cx| state.set_value("you two need to see this", window, cx));
            }
        }
        // Four pictures of mixed shapes on one message: the 2x2 gallery.
        "gallery-grid" => {
            if let Some(feed) = waffle.feeds.get_mut("general") {
                let mut messages = feed.messages.as_ref().clone();
                if let Some(last) = messages.iter_mut().find(|m| m.id == "102") {
                    last.media = [("3", 1200., 600.), ("5", 600., 900.), ("2", 800., 800.), ("4", 700., 1100.)]
                        .map(|(id, width, height)| Media { url: avatar_url(id, None), width, height, kind: MediaKind::Image, ..Default::default() })
                        .to_vec();
                }
                feed.messages = std::sync::Arc::new(messages);
            }
        }
        _ => {}
    }
}
