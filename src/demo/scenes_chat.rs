use super::*;

pub(super) fn chat_scenes(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    match scene {
        // The run of four joins opened to each join's own line.
        "system-messages-open" => {
            let first = waffle.feeds.get("general").and_then(|f| f.messages.iter().find(|m| m.kind == 7).map(|m| m.id.clone()));
            waffle.open_join_runs.extend(first);
        }
        // An account big enough that the rail, the channel list and the member list all scroll.
        "busy" => {
            let servers: Vec<Guild> = (1..=24u8).map(|n| {
                let mut guild = waffle.guilds[0].clone();
                guild.id = format!("busy-{n}");
                guild.name = format!("Server {}", char::from(b'A' + n - 1));
                guild
            }).collect();
            waffle.guilds.extend(servers);
            if let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") {
                guild.channels.extend((1..=30).map(|n| channel(&format!("busy-{n}"), &format!("channel-{n}"), ChannelKind::Text, Some("c0"))));
            }
            let mut items = members();
            items.extend((0..80).map(|n| {
                let user = user(&(30_000_000 + n).to_string(), &format!("member {n}"));
                ListItem::Member(Member { user, roles: Vec::new(), presence: Presence { status: Presence_::Offline, custom: None, activity: None, ..Default::default() }, boosting_since: None })
            }));
            waffle.on_event(Event::MemberList { guild_id: "1".into(), groups: vec![("r1".into(), 2), ("online".into(), 3), ("offline".into(), 81)], ops: vec![ListOp::Sync { start: 0, items }] }, cx);
        }
        // Compact display with its small avatars, and the send button.
        "chat-compact" => {
            waffle.account_settings.message_display_compact = true;
            waffle.prefs.compact_avatars = true;
            waffle.prefs.send_button = true;
        }
        // The largest text, no space between groups, role dots, underlined links and image descriptions.
        "chat-large-text" => {
            waffle.prefs.chat_font_size = 24;
            waffle.prefs.group_spacing = Some(0);
            waffle.prefs.role_style = crate::settings::RoleStyle::NextToNames;
            waffle.prefs.underline_links = true;
            waffle.prefs.show_image_descriptions = true;
        }
        // One draft waiting in #staff-notes (its pencil in the sidebar), another with a chip in the open composer.
        "drafts" => {
            waffle.drafts.set("staff", Draft::new(&InputContent::new("remember to pin the release notes"), None));
            let open = InputContent::new("@Wren can you check the build ").with_token(0..5, InlineToken::new("<@8388608>", "@Wren"));
            if let Ok(open) = open {
                waffle.drafts.set("general", Draft::new(&open, None));
            }
            waffle.restore_draft(window, cx);
        }
        // Vex is blocked and Dune ignored: Vex's three messages fold into one line, Dune's one into another,
        // and Wren's reply to Vex doesn't quote it. "-shown" opens Vex's run.
        "blocked-messages" | "blocked-messages-shown" => {
            waffle.relationships.push(Relationship { user: user(VEX, "Vex"), kind: Relation::Blocked, since: None });
            waffle.ignored.insert(DUNE.into());
            if scene == "blocked-messages-shown" {
                waffle.shown_folds.insert("302".into());
            }
        }
        "gallery" => waffle.open_viewer(&avatar_url("3", None), window, cx),
        // Twenty made-up pictures and clips on one message, the viewer open at the tenth.
        "viewer" | "viewer-video" => {
            let _ = crate::discord::DEMO_MEDIA.set(demo_media);
            if let Some(feed) = waffle.feeds.get_mut("general") {
                let mut messages = feed.messages.as_ref().clone();
                if let Some(message) = messages.iter_mut().find(|m| m.id == "102") {
                    message.media = (0..DEMO_SIZES.len()).map(demo_item).collect();
                }
                feed.messages = std::sync::Arc::new(messages);
            }
            let open = demo_item(if scene == "viewer" { 9 } else { 19 }).url;
            waffle.open_viewer(&open, window, cx);
            // The last item is a clip: Space plays it, through the viewer's own keys.
            if scene == "viewer-video" {
                cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor().timer(std::time::Duration::from_millis(800)).await;
                    this.update(cx, |this, cx| this.on_viewer_key(&gpui_kit::Keystroke::parse("space").expect("valid key"), cx)).ok();
                })
                .detach(); // lint-allow(detached-task): one-shot fixture step; ends after one timer and update, or when the view is gone
            }
        }
        // Who reacted, as a fetch would leave it: 🔥 names three (account names, shown as their nicknames)
        // and links the 589 others to the dialog; 👀's fetch failed, so it falls back to the count. Hover a pill
        // on the newest message.
        "reactors" => {
            let id = set_reactions(waffle, vec![Reaction { emoji: Emoji::unicode("🔥"), count: 592, me: true }, Reaction { emoji: Emoji::unicode("👀"), count: 1, me: false }]);
            waffle.reactors.insert((id.clone(), Emoji::unicode("🔥").path()), crate::reactions::Reactors::loaded(reactors(4)));
            waffle.reactors.insert((id, Emoji::unicode("👀").path()), crate::reactions::Reactors::failed());
        }
        // The Reactions dialog on the newest message's most used reaction (yours), every reaction's people loaded.
        "reactions-dialog" => {
            let counts = [("💀", 592, true), ("😭", 132, false), ("✅", 119, false), ("❌", 115, false), ("🐈", 67, false), ("🍯", 61, true), ("7️⃣", 61, false)];
            let reactions: Vec<Reaction> = counts.iter().map(|&(e, count, me)| Reaction { emoji: Emoji::unicode(e), count, me }).collect();
            for (i, r) in reactions.iter().enumerate() {
                let mut people = reactors(14 - i);
                if r.me {
                    people.insert(3, Reactor { user: me_user(), username: "kiwi.dev".into() });
                }
                waffle.reactors.insert((newest(waffle), r.emoji.path()), crate::reactions::Reactors::loaded(people));
            }
            let id = set_reactions(waffle, reactions);
            if let Some(message) = waffle.feed_message("general", &id) {
                waffle.open_reactions(&message, Emoji::unicode("💀"), cx);
            }
        }
        // "-unpin": the pins up, then the first one's unpin confirm over them.
        "pins" | "pins-unpin" => {
            let pinned: Vec<Message> = waffle.feeds.get("general").map(|f| f.messages.iter().rev().filter(|m| m.pinned).cloned().collect()).unwrap_or_default();
            if let Some(first) = pinned.first().filter(|_| scene == "pins-unpin") {
                waffle.run_message_action(crate::message_menu::MessageAction::Pin, first, false, window, cx);
            }
            waffle.pins = Some(Some(Ok(pinned)));
        }
        // Typed once the window is up, as `mention` does: the field isn't live while the app is being built.
        "header-search" => cx
            .spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(500)).await;
                this.update_in(cx, |this, window, cx| {
                    this.search_input.update(cx, |s, cx| s.set_value("gpui", window, cx));
                    cx.notify();
                })
                .ok();
            })
            .detach(), // lint-allow(detached-task): one-shot fixture step; ends after one timer and update, or when the view is gone
        "profile" => demo_profile(waffle, cx),
        s if PROFILE_SCENES.contains(&s) => demo_profile_scene(waffle, s, window, cx),
        s if ACTIVITY_SCENES.contains(&s) => demo_activity_scene(waffle, s, window, cx),
        "role-popout" => demo_role_popout(waffle, cx),
        // Someone you share nothing with, opened from a DM: the Discord date alone, no Friends Since, no connections.
        "profile-stranger" => {
            waffle.guild_id = None;
            let ferris = user("20971520", "ferris");
            waffle.open_profile(ferris, demo_at(520., 120.), cx);
            if let Some(card) = waffle.profile_card.as_mut() {
                card.profile = Some(crate::discord::Profile { username: "ferris.rs".into(), ..Default::default() });
            }
        }
        // No theme, banner, badges or games: the plain surface, and the Add role row in view.
        "profile-plain" => {
            demo_profile(waffle, cx);
            if let Some(profile) = waffle.profile_card.as_mut().and_then(|c| c.profile.as_mut()) {
                profile.theme = None;
                profile.banner = None;
                profile.badges.clear();
                profile.games.clear();
            }
        }
        "emoji" => waffle.open_emoji_picker(crate::EmojiTarget::Composer, window, cx),
        "switcher" => waffle.open_switcher(window, cx),
        "keyboard-combos" => waffle.set_keyboard_combos(true, cx),
        "status" => waffle.status_menu = true,
        "status-custom" => {
            waffle.custom_status = Some(demo_custom_status());
            waffle.status_menu = true;
        }
        // The status menu with Do not disturb's durations open beside it.
        "status-durations" => {
            waffle.status_menu = true;
            waffle.status_flyout = crate::status_menu::Flyout::opened(crate::discord::Status::Dnd);
        }
        // A poll you haven't voted in (one answer picked, not yet sent) and one you have.
        "poll" => {
            demo_polls(waffle);
            waffle.poll_picks.insert("195".into(), vec![1]);
        }
        // Quizzy's "Suggest a question" form: text fields, a user select starting at its default, a text-channel
        // select, and a file field; then the same after a Submit with Answer empty that Quizzy refused.
        "app-modal" | "app-modal-error" => {
            waffle.open_app_modal(quizzy_modal(), cx);
            if scene == "app-modal-error" {
                waffle.refuse_app_modal(window, cx);
            }
        }
        "poll-creator" => {
            waffle.open_poll_creator(window, cx);
            waffle.fill_poll_creator("Which map first tonight?", &[("Ashen Peaks", Some("🏔️")), ("Drowned Harbor", Some("🌊")), ("", None)], window, cx);
        }
        // Sticker messages (an animated one, a Lottie one that can't play) and the picker on its Stickers tab.
        "stickers" | "stickers-chat" => {
            demo_stickers(waffle);
            if scene == "stickers" {
                waffle.open_stickers(window, cx);
            }
        }
        "status-editor" => {
            waffle.custom_status = Some(demo_custom_status());
            waffle.open_status_editor(window, cx);
        }
        "update-ready" => waffle.updates.status = crate::updater::Status::Ready(crate::updater::Staged::demo("0.8.0")),
        // The Updates page in each state the updater reports.
        "updates" | "updates-downloading" | "updates-ready" => {
            use crate::updater::{Staged, Status};
            waffle.updates.status = match scene {
                "updates" => Status::UpToDate,
                "updates-downloading" => Status::Downloading { version: "0.8.0".into(), done: 13 << 20, total: Some(31 << 20) },
                _ => Status::Ready(Staged::demo("0.8.0")),
            };
            waffle.updates.checked_at = Some(std::time::SystemTime::now() - std::time::Duration::from_secs(12 * 60));
            waffle.open_settings(crate::settings::Page::Updates, window, cx);
        }
        "accounts" => {
            waffle.prefs.accounts = vec![account(&me_user(), Presence_::Online), account(&user("8388608", "Bartholomew Featherstonehaugh-Wright"), Presence_::Dnd), account(&user("16777216", "juno.alt"), Presence_::Idle)];
            waffle.status_menu = true;
        }
        "settings" => waffle.open_settings(crate::settings::Page::Account, window, cx),
        "create-channel" => waffle.open_create_channel("1".into(), 0, Some("c0".into()), window, cx),
        "create-channel-private" | "create-channel-community" | "create-channel-stage" => crate::create_channel::demo(waffle, scene, window, cx),
        "create-server" | "create-server-template" | "create-server-customize" | "create-server-join" | "create-server-limit" => crate::create_server::demo(waffle, scene, window, cx),
        "invite" | "invite-sent" | "invite-settings" | "invite-vanity" | "invite-paused" => crate::invite::demo(waffle, scene, window, cx),
        "events" | "events-create" | "events-occurrence" => demo_events(waffle, scene, window, cx),
        "zoom" => crate::layout::apply_zoom(window, 1.5, cx),
        "advanced" => {
            waffle.developer_mode = true;
            waffle.open_settings(crate::settings::Page::Advanced, window, cx);
        }
        "voice-settings" => waffle.open_settings(crate::settings::Page::VoiceVideo, window, cx),
        "keybinds" => waffle.open_settings(crate::settings::Page::Keybinds, window, cx),
        "keybinds-select" => {
            waffle.open_settings(crate::settings::Page::Keybinds, window, cx);
            if let Some(view) = waffle.settings.as_mut() {
                view.open_first_select();
            }
        }
        "appearance" => waffle.open_settings(crate::settings::Page::Appearance, window, cx),
        "settings-accessibility" => waffle.open_settings(crate::settings::Page::Accessibility, window, cx),
        // The nav scrolled up under its header strip, where the traffic lights sit.
        "appearance-scrolled" => {
            waffle.open_settings(crate::settings::Page::Appearance, window, cx);
            let nav = waffle.settings_nav_scroll.clone();
            window.on_next_frame(move |window, _| {
                nav.set_offset(demo_at(0., -90.));
                window.refresh();
            });
        }
        "notifications" => waffle.open_settings(crate::settings::Page::Notifications, window, cx),
        "content-social" => waffle.open_settings(crate::settings::Page::ContentSocial, window, cx),
        "chat-settings" => waffle.open_settings(crate::settings::Page::Chat, window, cx),
        // The whole look turned: muted colour, high contrast, compact lists and switch indicators. In memory
        // only; changing any setting in the scene saves them to the real prefs file.
        "settings-contrast" => {
            let prefs = &mut waffle.prefs;
            (prefs.saturation, prefs.high_contrast, prefs.ui_density, prefs.switch_icons) = (0.3, true, crate::settings::Density::Compact, true);
            waffle.apply_prefs();
            crate::palette::apply(cx);
            waffle.open_settings(crate::settings::Page::Accessibility, window, cx);
        }
        // Typed once the window is up, as `mention` does.
        "settings-search" => {
            waffle.open_settings(crate::settings::Page::Account, window, cx);
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(500)).await;
                this.update_in(cx, |this, window, cx| {
                    this.settings_search.update(cx, |s, cx| s.set_value("mic", window, cx));
                    cx.notify();
                })
                .ok();
            })
            .detach(); // lint-allow(detached-task): one-shot fixture step; ends after one timer and update, or when the view is gone
        }
        // Opened with news since message 103: the NEW line above the first, and the bar over the feed.
        "unread" => {
            waffle.unread_marker = Some(103);
            waffle.last_message.insert("general".into(), 999);
            waffle.read_states.insert("general".into(), ReadState { last_read: 103, mentions: 0 });
        }
        // `:` emoji suggestions for a half-typed code.
        // `emoji-accept` then picks the first, which replaces the typed code.
        "emoji-suggest" | "emoji-accept" => {
            let accept = scene == "emoji-accept";
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(500)).await;
                this.update_in(cx, |this, window, cx| {
                    this.composer.update(cx, |state, cx| {
                        state.set_value("gg :smil", window, cx);
                        state.focus(window, cx);
                    });
                    this.autocomplete = this.suggest("gg :smil");
                    if accept {
                        this.accept_suggestion(window, cx);
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach(); // lint-allow(detached-task): one-shot fixture step; ends after one timer and update, or when the view is gone
        }
        // The `/` browser: Recent (the top commands, also in their app's section) and the app rail; or,
        // with `/pl` typed, one flat list by match.
        "command-picker" | "command-picker-search" => {
            waffle.command_picker.set_index("1", demo_commands());
            let now = crate::command_picker::now_ms();
            for (command, runs) in [(0, 3), (5, 2), (6, 1)] {
                for _ in 0..runs {
                    waffle.command_picker.recent.record(&demo_commands().commands[command], Some("1"), now);
                }
            }
            let text = if scene == "command-picker" { "/" } else { "/pl" };
            demo_command_text(window, cx, text, text.len(), |_, _, _| {});
        }
        // `/play` typed out, the caret in its query: Wavebot's suggestions, the skeletons while they load,
        // the failure, or no match.
        "command-options" | "command-options-loading" | "command-options-failed" | "command-options-empty" => {
            waffle.command_picker.set_index("1", demo_commands());
            let scene = scene.to_owned();
            demo_command_text(window, cx, "/play query:midnight ar volume:80", 23, move |this, _, cx| {
                let songs = ["Midnight Arcade", "Midnight Arcade (Slowed)", "Arcade Midnight Run"];
                let answer = match scene.as_str() {
                    "command-options" => Some(songs.map(|name| crate::app_commands::Choice { name: name.into(), name_localized: None, value: serde_json::json!(name) }).to_vec()),
                    "command-options-empty" => Some(Vec::new()),
                    "command-options-failed" => None,
                    _ => return,
                };
                this.suggest_for_option(answer, cx);
            });
        }
        // Tab past `/play`'s last box, then `vo` typed outside it: the option-name list narrowed to
        // `volume`, the query's value still `Midnight Arcade`.
        "command-options-tab" => {
            waffle.command_picker.set_index("1", demo_commands());
            demo_command_text(window, cx, "/play query:Midnight Arcade", 27, |this, window, cx| {
                this.on_command_tab(false, window, cx);
                this.composer.update(cx, |state, cx| state.insert("vo", window, cx));
            });
        }
        // Enter on a command with an empty required box and two bad values: every bad box red, the
        // caret in the first, its error in the header.
        "command-options-errors" => {
            waffle.command_picker.set_index("1", demo_commands());
            demo_command_text(window, cx, "/play query: volume:250 shuffle:maybe", 37, |this, window, cx| {
                this.on_command_enter(window, cx);
            });
        }
        _ => {}
    }
}
