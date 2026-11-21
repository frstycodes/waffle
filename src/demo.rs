// Worst-case fixture data for checking each screen without a Discord account (debug builds only):
// WAFFLE_DEMO=chat | busy | message-focus | message-focus-menu | message-focus-delete | message-menu-theirs | user-menu-voice | user-menu-voice-denied | mention | mention-tab | mention-enter | reactors | reactions-dialog | text-files | text-expanded | text-viewer | link-previews | activity | activity-all | rail-dms | rail-dms-live | gallery | viewer | viewer-video | pins | header-search | search | search-from | search-date | search-results | search-filters | search-dm | search-dm-results | profile | profile-plain | emoji | switcher | settings | voice-settings | keybinds | appearance | appearance-scrolled | notifications | content-social | chat-settings | settings-search | status | status-custom | status-durations | status-editor | stickers | stickers-chat | accounts | unread | emoji-suggest | emoji-accept | notify-bell | jump | inbox | inbox-unreads | home | add-friend | create-channel | advanced | zoom | home-presence | folder | stream-card | call-streams | call-streams-ended | call-streams-focus | call-streams-strip | call-streams-full | voice-elsewhere | call-incoming | call-banner | call-joinable | call-dm | call-dm-camera | sidebar-resize | sidebar-voice | sidebar-voice-error | sidebar-voice-solid | sidebar-voice-fullscreen | dm-empty | group-empty | group-start | login | scanned | failed | empty | update-ready | updates | updates-downloading | updates-ready | channel-settings | channel-settings-search | channel-settings-perms | channel-settings-voice | channel-settings-voice-perms | channel-settings-invites | channel-settings-dirty | load | server-settings | server-settings-engagement | server-settings-roles | server-settings-role | server-settings-role-perms | server-settings-audit | server-settings-bans | server-settings-automod | server-settings-invites | server-settings-dirty | attachments | attachments-drop | forwarded | forward-dialog | poll | poll-creator | drafts | friends-empty | friends-all-empty | friends-pending-empty | friends-blocked-empty | pins-empty | search-empty | inbox-empty | inbox-unreads-empty | forum-empty | load-failed | updating (the launch splash alone).
use gpui_kit::{
    Context, Window,
    component::input::{InlineToken, InputContent},
};

use crate::{
    Waffle, Feed, Status,
    drafts::Draft,
    discord::{
        Activity, ActivityKind, Channel, ChannelKind, Embed, Emoji, Event, Folder, Guild, GuildTag, ListItem, ListOp, Media, MediaKind, Member, Message, Presence, Reaction, Reactor, ReadState, Ready, Relation, Relationship, Role,
        Status as Presence_, User, VoiceState, avatar_url,
    },
    login::Login,
};

// The launch splash mid-download, without the main window.
pub fn updating_splash(cx: &mut gpui_kit::App) {
    crate::splash::open("Installing 0.5.0…", cx);
}

pub fn load(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    // Fixture drafts stay in memory, away from the real drafts file.
    waffle.drafts = crate::drafts::Drafts::default();
    let me = me_user();
    match scene {
        "login" => return waffle.status = Status::SignedOut(Login::qr("https://discord.com/ra/demo-fingerprint-0123456789abcdef")),
        "scanned" => return waffle.status = Status::SignedOut(Login::Scanned(me)),
        "failed" => return waffle.status = Status::SignedOut(Login::Failed("Invalid ticket".into())),
        _ => {}
    }
    let load = scene == "load";
    waffle.on_event(Event::Ready(Box::new(ready(me))), cx);
    if load {
        load_guilds(waffle);
    }
    let (groups, items) = if load { load_members() } else { (vec![("r1".into(), 2), ("online".into(), 3), ("offline".into(), 1)], members()) };
    waffle.on_event(Event::MemberList { guild_id: "1".into(), groups, ops: vec![ListOp::Sync { start: 0, items }] }, cx);
    waffle.guild_id = Some("1".into());
    if scene == "empty" {
        return;
    }
    let messages = match scene {
        _ if load => load_messages(),
        "embeds" => embed_messages(),
        _ => messages(),
    };
    waffle.feeds.insert("general".into(), Feed { messages: std::sync::Arc::new(messages), loaded: true, ..Default::default() });
    let general = waffle.guild().and_then(|g| g.channels.iter().find(|c| c.id == "general").cloned());
    if let Some(channel) = general {
        waffle.open_channel(channel, window, cx);
    }
    waffle.on_event(Event::Typing { channel_id: "general".into(), user_id: "8388608".into(), name: Some("Wren".into()) }, cx);
    match scene {
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
                ListItem::Member(Member { user, roles: Vec::new(), presence: Presence { status: Presence_::Offline, custom: None, activity: None, rich: None, ..Default::default() } })
            }));
            waffle.on_event(Event::MemberList { guild_id: "1".into(), groups: vec![("r1".into(), 2), ("online".into(), 3), ("offline".into(), 81)], ops: vec![ListOp::Sync { start: 0, items }] }, cx);
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
                .detach();
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
            .detach(),
        "profile" => demo_profile(waffle, cx),
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
        // The nav scrolled up under its header strip, where the traffic lights sit.
        "appearance-scrolled" => {
            waffle.open_settings(crate::settings::Page::Appearance, window, cx);
            let nav = waffle.settings_nav_scroll.clone();
            window.on_next_frame(move |window, _| {
                nav.set_offset(gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(-90.)));
                window.refresh();
            });
        }
        "notifications" => waffle.open_settings(crate::settings::Page::Notifications, window, cx),
        "content-social" => waffle.open_settings(crate::settings::Page::ContentSocial, window, cx),
        "chat-settings" => waffle.open_settings(crate::settings::Page::Chat, window, cx),
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
            .detach();
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
            .detach();
        }
        // The channel header's bell with its menu open.
        "notify-bell" => {
            if let Some(channel) = waffle.channel.clone() {
                waffle.open_notify_menu(channel, "1".into(), gpui_kit::point(gpui_kit::px(1180.), gpui_kit::px(40.)), cx);
            }
        }
        // A jump landing: the message centred, filled and squiggled.
        "jump" => waffle.jump_to("103".into(), cx),
        // The rail's Inbox on each tab: mentions under their filter chips, and the unread channels with the
        // newest cached messages of one.
        "inbox" | "inbox-unreads" => {
            let all = self::messages();
            waffle.inbox = Some(Some(Ok(all.iter().filter(|m| !m.mentions.is_empty() || m.content.contains("@")).cloned().collect())));
            let staff: Vec<Message> = all.iter().rev().take(2).map(|m| Message { channel_id: "staff".into(), id: format!("9{}", m.id), ..m.clone() }).collect();
            waffle.feeds.insert("staff".into(), Feed { messages: std::sync::Arc::new(staff), loaded: true, ..Default::default() });
            waffle.inbox_tab = if scene == "inbox" { crate::inbox::InboxTab::Mentions } else { crate::inbox::InboxTab::Unreads };
            waffle.inbox_open = true;
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
            .detach()
        }
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
                    waffle.open_message_menu(message, gpui_kit::point(gpui_kit::px(560.), gpui_kit::px(120.)), cx);
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
            waffle.open_user_menu(user("8388608", "Wren"), Some("1".into()), crate::user_menu::Surface::ServerVoice, gpui_kit::point(gpui_kit::px(300.), gpui_kit::px(300.)), cx);
        }
        // As the server's owner, someone else's message as REST history loads it (no guild_id): Delete, Pin
        // and the reaction removals are on offer.
        "message-menu-theirs" => {
            let theirs = waffle.feeds.get("general").and_then(|f| f.messages.iter().find(|m| m.id == "102").cloned());
            if let Some(message) = theirs {
                waffle.open_message_menu(Message { guild_id: None, ..message }, gpui_kit::point(gpui_kit::px(560.), gpui_kit::px(120.)), cx);
            }
        }
        "search" | "search-from" | "search-date" => demo_search(waffle, scene, window, cx),
        "search-results" | "search-filters" => demo_search_results(waffle, scene == "search-filters", window, cx),
        "search-dm" | "search-dm-results" => demo_search_dm(waffle, scene == "search-dm-results", window, cx),
        // A long message.txt (Discord's file for a message past 2,000 characters), a short code file, and
        // a .log that isn't text, which stays a file card. The fixtures stand in for the CDN fetch.
        "text-files" | "text-expanded" | "text-viewer" => demo_text_files(waffle, scene),
        // Made-up files of each kind waiting in the composer (a picture, a spoilered one, a video, a PDF,
        // an archive, a long name), one upload part way and one Discord refused. `-drop` holds the drop
        // zone up as if files were dragged over.
        "attachments" | "attachments-drop" => demo_attachments(waffle, window, cx),
        // Discord's own links: a message card, a channel chip, invites to a joined server, another server
        // and an expired code, a message you can't see (no card), and two still loading.
        "link-previews" => demo_link_previews(waffle),
        "forwarded" => demo_forwarded(waffle),
        // Report Message on someone else's message, through a sample menu (Discord's real one is server-sent).
        // `-submit` stops at Submit with Also block ticked; `-sent` is the thank-you node; `-user` reports a person.
        // Signed out, nothing is sent.
        "report" | "report-submit" | "report-sent" | "report-user" => demo_report(waffle, scene, cx),
        // The dialog with #📷-media picked and a note; gpui's channel reads only, so it shows locked.
        "forward-dialog" => {
            if let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "2") {
                guild.owner_id = Some("1".into());
                guild.roles = vec![Role { id: "2".into(), name: "@everyone".into(), color: 0, position: 0, permissions: crate::discord::VIEW_CHANNEL, managed: false }];
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
                        .map(|(id, width, height)| Media { url: avatar_url(id, None), width, height, kind: MediaKind::Image })
                        .to_vec();
                }
                feed.messages = std::sync::Arc::new(messages);
            }
        }
        // The illustrated empty states (docs/illustrations.md), one scene each.
        "friends-empty" | "friends-all-empty" | "friends-pending-empty" | "friends-blocked-empty" => {
            waffle.relationships.clear();
            waffle.restore = None;
            waffle.open_home(cx);
            waffle.friends_tab = match scene {
                "friends-all-empty" => crate::home::FriendsTab::All,
                "friends-pending-empty" => crate::home::FriendsTab::Pending,
                "friends-blocked-empty" => crate::home::FriendsTab::Blocked,
                _ => crate::home::FriendsTab::Online,
            };
        }
        "pins-empty" => waffle.pins = Some(Some(Ok(Vec::new()))),
        "search-empty" => waffle.right_panel = crate::RightPanel::Search(Box::new(crate::search_results::Results::demo(Vec::new(), 0, 0, false))),
        "inbox-empty" | "inbox-unreads-empty" => {
            waffle.inbox = Some(Some(Ok(Vec::new())));
            waffle.inbox_tab = if scene == "inbox-empty" { crate::inbox::InboxTab::Mentions } else { crate::inbox::InboxTab::Unreads };
            waffle.inbox_open = true;
        }
        "forum-empty" => {
            let forum = channel("forum", "ideas", ChannelKind::Forum, Some("c0"));
            waffle.forums.insert(forum.id.clone(), crate::forum::ForumState::default());
            waffle.channel = Some(forum);
        }
        "load-failed" => {
            waffle.feeds.insert("general".into(), Feed { error: Some("Discord didn't answer in time. Check your connection and try again.".into()), ..Default::default() });
        }
        "home" | "add-friend" => {
            waffle.restore = None;
            waffle.open_home(cx);
            if scene == "add-friend" {
                waffle.friends_tab = crate::home::FriendsTab::AddFriend;
            }
        }
        // One DM per presence, so each status mark shows its cutout on the sidebar.
        "home-presence" => {
            for (id, name, status) in [("12582912", "moss lantern", Presence_::Idle), ("16777216", "toma reed", Presence_::Dnd), ("25165824", "ferris", Presence_::Offline)] {
                let mut dm = waffle.dms[0].clone();
                dm.id = format!("dm-{id}");
                dm.name = name.into();
                dm.recipient = Some(user(id, name));
                waffle.dms.push(dm);
                waffle.presences.insert(id.into(), Presence { status, ..Default::default() });
            }
            waffle.restore = None;
            waffle.open_home(cx);
        }
        "activity" | "activity-all" => {
            let ago = |hours: i64| chrono::Utc::now().timestamp_millis() - hours * 3_600_000;
            let entry = |author: &str, kind, title: &str, image: Option<&str>, live, at, streak_days| crate::discord::FeedEntry { author_id: author.into(), kind, title: title.into(), image: image.map(Into::into), app_id: None, live, at, streak_days };
            let game = |author: &str, (name, app_id): (&str, &str), at, streak_days| crate::discord::FeedEntry { app_id: Some(app_id.into()), ..entry(author, ActivityKind::Playing, name, None, false, at, streak_days) };
            demo_games(waffle);
            waffle.activity_feed = vec![
                game("12582912", HADES, ago(26), Some(6)),
                entry("16777216", ActivityKind::Listening, "Too Sweet", Some("demo/banner.png"), true, ago(0), None),
                // No application: the card keeps its placeholder art.
                entry("8388608", ActivityKind::Playing, "The Legend of Heroes: Trails through Daybreak II", None, false, ago(30), None),
                game("25165824", FACTORIO, ago(2), None),
                entry("4194304", ActivityKind::Watching, "Frieren: Beyond Journey's End", Some("demo/banner.png"), false, ago(3), None),
            ];
            if scene == "activity-all" {
                waffle.right_panel = crate::RightPanel::Activity;
            }
        }
        // Unread DMs stack under Home on the rail: Wren with one message, a group-less second with 12.
        "rail-dms" => {
            let mut ani = waffle.dms[0].clone();
            ani.id = "dm2".into();
            ani.recipient = Some(user("16777216", "toma reed"));
            waffle.dms.push(ani);
            for (id, mentions) in [("dm", 1), ("dm2", 12)] {
                waffle.last_message.insert(id.into(), 200);
                waffle.read_states.insert(id.into(), ReadState { last_read: 100, mentions });
            }
        }
        "folder" => {
            waffle.open_folders.insert("9".into());
        }
        "rail-dms-live" => {
            let mut ani = waffle.dms[0].clone();
            ani.id = "dm2".into();
            ani.recipient = Some(user("16777216", "toma reed"));
            waffle.dms.push(ani);
            cx.spawn_in(window, async move |this, cx| loop {
                for (id, last_read, mentions) in [("dm", 100, 1), ("dm2", 100, 3), ("dm", 200, 0), ("dm2", 200, 0)] {
                    cx.background_executor().timer(std::time::Duration::from_millis(1500)).await;
                    let alive = this.update(cx, |this, cx| {
                        this.last_message.insert(id.into(), 200);
                        this.read_states.insert(id.into(), ReadState { last_read, mentions });
                        cx.notify();
                    });
                    if alive.is_err() {
                        return;
                    }
                }
            })
            .detach();
        }
        // Wren calls: the strip rings in her DM; "call-banner" takes the call while #general is open.
        // "call-dm": we're in Wren's call, its window floating over the DM; "call-dm-camera" with her camera on.
        "call-incoming" | "call-banner" | "call-joinable" | "call-dm" | "call-dm-camera" => demo_dm_call(waffle, scene, window, cx),
        // The foot of the sidebar stacked full (an update over the voice panel, then its disconnected
        // state), with the channel list scrolled so rows run under the header's blur.
        // The channel list resized back and forth, as if its edge were being dragged: the traffic lights
        // must hold still.
        "sidebar-resize" => {
            cx.spawn(async move |this, cx| {
                for step in 0.. {
                    cx.background_executor().timer(std::time::Duration::from_millis(16)).await;
                    let width = 240. + 80. * ((step as f32) / 30.).sin().abs();
                    if this.update(cx, |this, cx| {
                        this.channels_width = width;
                        cx.notify();
                    })
                    .is_err()
                    {
                        return;
                    }
                }
            })
            .detach();
        }
        // Another client of ours took the call: the notice bar over the chat, with Reconnect.
        "voice-elsewhere" => {
            let channel = waffle.guilds.iter().flat_map(|g| &g.channels).find(|c| c.kind == crate::discord::ChannelKind::Voice).cloned();
            waffle.voice_notice = channel.map(|channel| crate::VoiceNotice { guild_id: Some("1".into()), channel, at: std::time::Instant::now(), why: crate::NoticeWhy::Elsewhere });
        }
        "sidebar-voice" | "sidebar-voice-error" | "sidebar-voice-solid" | "sidebar-voice-fullscreen" => {
            waffle.prefs.native_sidebar = scene != "sidebar-voice-solid";
            waffle.updates.status = crate::updater::Status::Ready(crate::updater::Staged::demo("0.8.0"));
            match scene {
                "sidebar-voice" | "sidebar-voice-solid" | "sidebar-voice-fullscreen" => demo_call(waffle),
                _ => waffle.voice_error = Some("Moved to a voice channel Waffle can't show".into()),
            }
            demo_games(waffle);
            if let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") {
                guild.channels.extend((1..=16).map(|n| channel(&format!("filler-{n}"), &format!("filler-{n}"), ChannelKind::Text, Some("c0"))));
            }
            // After the first layout: before it there is nothing to scroll, and the offset clamps to 0.
            if scene == "sidebar-voice-fullscreen" {
                window.toggle_fullscreen();
            }
            let list = waffle.channels_scroll.clone();
            window.on_next_frame(move |window, _| {
                list.set_offset(gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(-40.)));
                window.refresh();
            });
        }
        "dm-empty" | "group-empty" | "group-start" | "group-mention" => demo_dm_start(waffle, scene, window, cx),
        "stream-card" => {
            let key = "guild:1:lounge:12582912".to_owned();
            let blender = Activity { kind: ActivityKind::Playing, text: "Blender".into(), more: 0 };
            waffle.presences.insert("12582912".into(), Presence { status: Presence_::Online, custom: None, activity: Some(blender), rich: None, ..Default::default() });
            let since = std::time::Instant::now().checked_sub(std::time::Duration::from_secs(24 * 60)).unwrap_or_else(std::time::Instant::now);
            waffle.stream_live_since.insert(key.clone(), since);
            waffle.on_event(Event::StreamUpdate { key, viewers: vec!["8388608".into(), "16777216".into(), "20971520".into(), "4194304".into()], paused: false }, cx);
            waffle.stream_card_hover = Some("stream-12582912".into());
        }
        "user-menu" => {
            demo_call(waffle);
            waffle.call_view = false;
            waffle.voice_user_menu("8388608", gpui_kit::point(gpui_kit::px(300.), gpui_kit::px(160.)), cx);
        }
        "server-menu" => waffle.open_server_menu("1".into(), gpui_kit::point(gpui_kit::px(60.), gpui_kit::px(130.)), false, cx),
        // Move server is entry 14 (the header and separators count); Folder is its first child.
        "server-menu-nested" => {
            waffle.open_server_menu("1".into(), gpui_kit::point(gpui_kit::px(60.), gpui_kit::px(130.)), false, cx);
            waffle.open_submenus(vec![14, 0]);
        }
        "server-settings" | "server-settings-engagement" | "server-settings-roles" | "server-settings-role" | "server-settings-role-perms" | "server-settings-audit" | "server-settings-bans" | "server-settings-automod" | "server-settings-invites" | "server-settings-dirty" => crate::server_settings::demo::open(waffle, scene, window, cx),
        "channel-settings" | "channel-settings-search" | "channel-settings-perms" | "channel-settings-voice" | "channel-settings-voice-perms" | "channel-settings-invites" | "channel-settings-dirty" => demo_channel_settings(waffle, scene, window, cx),
        "channel-menu" => {
            let general = waffle.guild().and_then(|g| g.channels.iter().find(|c| c.id == "general").cloned());
            if let Some(channel) = general {
                waffle.open_channel_menu(channel, "1".into(), gpui_kit::point(gpui_kit::px(200.), gpui_kit::px(170.)), cx);
            }
        }
        "message-menu" => {
            let message = waffle.feeds.get("general").and_then(|f| f.messages.iter().find(|m| m.author.id == "8388608").cloned());
            if let Some(message) = message {
                waffle.open_message_menu(message, gpui_kit::point(gpui_kit::px(700.), gpui_kit::px(260.)), cx);
            }
        }
        // Fixture sources rather than the real listing, which would ask for Screen Recording access.
        // share-window remembers a closed Figma window, so a sibling is preselected.
        "share" | "share-window" => {
            demo_call(waffle);
            if scene == "share-window" {
                waffle.prefs.last_share = Some(crate::go_live::LastShare::Window { id: 99, pid: 501, app_name: "Figma".into() });
            }
            let window = |id, pid, app: &str, title: &str| crate::voice::ShareableWindow { id, title: title.into(), app_name: app.into(), pid };
            let windows = vec![
                window(1, 500, "Safari", "Waffle: a Discord client in Rust with gpui, the full and very long window title of a pull request"),
                window(2, 501, "Figma", "Go Live picker"),
                window(3, 502, "Terminal", "zsh"),
                window(4, 503, "Notes", "N"),
            ];
            let screen = |id, name: &str| crate::voice::ShareableScreen { id, name: name.into(), width: 1512, height: 982 };
            waffle.empty_share_picker(cx);
            waffle.list_shareables(Ok(windows), Ok(vec![screen(1, "Screen 1"), screen(2, "Screen 2")]), cx);
        }
        // Three streams (one watched at 4:3, so focus letterboxes it; one unopened; our own) and two
        // cameras: the grid's order, chips and Watch, focus and full screen. Ended adds two ended
        // streams' cards: one streamer still in the call, one who left.
        "call-streams" | "call-streams-ended" | "call-streams-focus" | "call-streams-strip" | "call-streams-full" => {
            demo_call(waffle);
            waffle.call_pointer_inside = true;
            if let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") {
                for state in guild.voice_states.iter_mut() {
                    match state.user_id.as_str() {
                        "16777216" | "4194304" => state.streaming = true,
                        "20971520" | "8388608" => state.camera = true,
                        _ => {}
                    }
                }
            }
            if let Some(call) = waffle.call.as_mut() {
                call.watching.push(crate::Watching { streamer_id: "12582912".into(), frame: Some(demo_frame(1440, 1080, 40)), arrivals: Default::default(), watch: None, rejoins: 0 });
                call.self_view = Some((demo_frame(1280, 720, 200), std::time::Instant::now()));
                call.cameras.insert("20971520".into(), demo_frame(640, 480, 120));
                call.cameras.insert("8388608".into(), demo_frame(640, 360, 230));
                if scene == "call-streams-ended" {
                    call.ended = vec!["20971520".into(), "99".into()];
                }
            }
            if !matches!(scene, "call-streams" | "call-streams-ended") {
                waffle.call_focus = Some("stream:12582912".into());
                // Holds the chrome up for the first half minute, as if the pointer had just moved.
                waffle.call_pointer_at = std::time::Instant::now() + std::time::Duration::from_secs(30);
            }
            waffle.call_strip_hidden = scene != "call-streams-strip";
            if scene == "call-streams-full" {
                waffle.set_call_fullscreen(true, window, cx);
            }
        }
        "call" | "call-info" | "call-devices" | "call-watch" | "call-focus" | "pip" => {
            demo_call(waffle);
            // As if the pointer rests on the stage, so its chrome shows.
            waffle.call_pointer_inside = true;
            match scene {
                "call-info" => waffle.connection_info = true,
                // Watching moss lantern's Go Live: the stream over the filmstrip.
                "call-watch" | "call-focus" | "pip" => {
                    if let Some(call) = waffle.call.as_mut() {
                        // The PiP plays a 4:3 picture, to show it letterboxed inside its 16:9 frame.
                        let frame = (scene == "pip").then(|| demo_frame(1440, 1080, 40));
                        call.watching.push(crate::Watching { streamer_id: "12582912".into(), frame, arrivals: Default::default(), watch: None, rejoins: 0 });
                    }
                    // Reading #general meanwhile: the stream floats as a picture-in-picture.
                    waffle.call_view = scene != "pip";
                    // The stream clicked: it fills the stage over a strip of everyone else.
                    if scene == "call-focus" {
                        waffle.call_focus = Some("stream:12582912".into());
                    }
                }
                "call-devices" => waffle.open_device_menu(crate::call::DeviceKind::Input, gpui_kit::point(gpui_kit::px(260.), gpui_kit::px(760.)), cx),
                _ => {}
            }
        }
        _ => {}
    }
}

// Wren's popout with every section filled: theme, decoration, tag, badges, mutuals, an emoji bio, games and a joinable party.
fn demo_profile(waffle: &mut Waffle, cx: &mut Context<Waffle>) {
    let started = chrono::Utc::now().timestamp_millis() - 7 * 60_000 - 2_000;
    let rich = crate::discord::RichPresence {
        kind: ActivityKind::Playing,
        name: "PUBG: BATTLEGROUNDS".into(),
        app_id: Some("1402418717781921935".into()),
        details: Some("Normal, Erangel, 58/99".into()),
        state: Some("4 Squad".into()),
        image: Some("brand/icons/blueprint.png".into()),
        party: Some((4, 4)),
        started: Some(started),
        joinable: true,
    };
    let playing = Activity { kind: ActivityKind::Playing, text: rich.name.clone(), more: 0 };
    waffle.presences.insert("8388608".into(), Presence { status: Presence_::Dnd, custom: None, activity: Some(playing), rich: Some(rich), ..Default::default() });
    let mut zoe = user("8388608", "Wren");
    zoe.tag = Some(GuildTag { text: "wony".into(), badge: Some("brand/icons/sticker.png".into()) });
    zoe.decoration = Some("demo/decoration.png".into());
    waffle.open_profile(zoe, gpui_kit::point(gpui_kit::px(520.), gpui_kit::px(120.)), cx);
    let badge = |name: &str, description: &str| crate::discord::Badge { description: description.into(), icon: format!("brand/icons/{name}.png") };
    let profile = crate::discord::Profile {
        username: "wren.lark".into(),
        bio: Some("LUCKY VICKY <a:aWonUwu:1328465377218269257>\n**gpui** fan, _probably_ in voice".into()),
        pronouns: Some("she/her".into()),
        banner: Some("demo/banner.png".into()),
        roles: vec!["r2".into()],
        theme: Some((0x9B1FA8, 0xE9A6C9)),
        badges: ["paper", "peek", "sticker", "riso", "blueprint"].iter().map(|n| badge(n, "Subscriber since 14 Nov 2024")).collect(),
        mutual_friends: vec![user("12582912", "moss lantern"), user("16777216", "toma reed"), user("20971520", "ferris")],
        mutual_friend_count: 5,
        mutual_server_count: 1,
        games: ["peek", "riso", "blueprint", "sticker", "paper"].iter().map(|n| format!("brand/icons/{n}.png")).collect(),
        ..Default::default()
    };
    if let Some(card) = waffle.profile_card.as_mut() {
        card.profile = Some(profile);
    }
}

// The top of a DM's history: a stranger with nothing said yet, an iconless group with nothing said, and
// the same group scrolled back to its first messages.
fn demo_dm_start(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    let ferris = user("20971520", "ferris");
    let mut stranger = channel("dm-new", "ferris", ChannelKind::Dm, None);
    stranger.recipient = Some(ferris.clone());
    stranger.recipients = vec![ferris.clone()];
    let profile = crate::discord::Profile { username: "ferris.rs".into(), mutual_server_count: 2, ..Default::default() };
    waffle.profiles.seed(&crate::profile::profile_key(&ferris.id, None), profile);
    let mut group = channel("group", "Ranked squad", ChannelKind::Dm, None);
    group.raw_type = 3;
    group.recipients = vec![user("8388608", "Wren"), user("16777216", "toma reed"), ferris];
    waffle.dms.extend([group.clone(), stranger.clone()]);
    let ago = |mins: i64| (chrono::Utc::now() - chrono::Duration::minutes(mins)).to_rfc3339();
    let messages = match scene {
        "group-start" => vec![
            Message { id: "401".into(), channel_id: "group".into(), author: group.recipients[0].clone(), timestamp: ago(60), content: "made a group for ranked tonight".into(), ..Default::default() },
            Message { id: "402".into(), channel_id: "group".into(), author: group.recipients[1].clone(), timestamp: ago(58), content: "in, after dinner".into(), ..Default::default() },
            Message { id: "403".into(), channel_id: "group".into(), author: me_user(), timestamp: ago(2), content: "on in 5".into(), ..Default::default() },
        ],
        _ => Vec::new(),
    };
    let open = if scene == "dm-empty" { stranger } else { group };
    waffle.feeds.insert(open.id.clone(), Feed { messages: std::sync::Arc::new(messages), loaded: true, ..Default::default() });
    // Unread, so the group also stacks under Home on the rail.
    waffle.last_message.insert("group".into(), 200);
    waffle.read_states.insert("group".into(), ReadState { last_read: 100, mentions: 2 });
    waffle.restore = None;
    waffle.guild_id = None;
    waffle.open_channel(open, window, cx);
    // A bare `@` in a group DM: only its people, you last.
    if scene == "group-mention" {
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(500)).await;
            this.update_in(cx, |this, window, cx| {
                this.composer.update(cx, |state, cx| state.set_value("@", window, cx));
                this.autocomplete = this.suggest("@");
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

// Real Discord applications (id, icon hash from /applications/detectable), so their icons load from the CDN.
const HADES: (&str, &str) = ("Hades II", "1237101681695522867");
const FACTORIO: (&str, &str) = ("Factorio", "358422126602223616");
const APEX: (&str, &str) = ("Apex Legends", "542075586886107149");

// Games in hangout: you (kiwi) detected playing Hades II, moss lantern and Wren by their presences.
fn demo_games(waffle: &mut Waffle) {
    for ((_, app_id), icon) in [(HADES, "e8aaa81a90f8c36e88014722906884b6"), (FACTORIO, "7624a1d91d9828ed2f2dc3510bbe09e6"), (APEX, "91fac0600c5b6527c1aea95a93c6a8e0")] {
        waffle.games.seed_icon(app_id, &crate::discord::app_icon_url(app_id, icon));
    }
    waffle.games.mine = Some(crate::games::Game { pid: 501, app_id: HADES.1.into(), name: HADES.0.into(), icon: Some("e8aaa81a90f8c36e88014722906884b6".into()), started: chrono::Utc::now().timestamp_millis() });
    for (id, (name, app_id)) in [("12582912", APEX), ("8388608", FACTORIO)] {
        let started = Some(chrono::Utc::now().timestamp_millis() - 47 * 60_000);
        let rich = crate::discord::RichPresence { kind: ActivityKind::Playing, name: name.into(), app_id: Some(app_id.into()), started, ..Default::default() };
        let activity = Activity { kind: ActivityKind::Playing, text: name.into(), more: 0 };
        waffle.presences.insert(id.into(), Presence { status: Presence_::Online, custom: None, activity: Some(activity), rich: Some(rich), ..Default::default() });
    }
}

// A connected call in hangout with made-up stats; the session itself never reaches a server.
fn demo_call(waffle: &mut Waffle) {
    let Some(lounge) = waffle.guild().and_then(|g| g.channels.iter().find(|c| c.id == "lounge").cloned()) else { return };
    let (session, _) = crate::voice::join("4194304".into(), Some("1".into()), "lounge".into(), crate::voice::GatewayHandle::new().0, Default::default());
    let stats = crate::voice::Stats {
        ping_ms: 42,
        median_ping_ms: 47,
        last_pings: vec![44, 51, 47, 40, 62, 45, 43, 48, 120, 52, 46, 44, 41, 47, 50, 42],
        outbound_loss_pct: Some(0.4),
        playout_delay_ms: Some(60),
        endpoint: "c-bom06-1a2b3c4d.discord.media".into(),
        server_ip: "162.159.0.1".into(),
        encryption: "aead_aes256_gcm_rtpsize".into(),
        dave: true,
    };
    waffle.call = Some(crate::Call {
        session,
        guild_id: Some("1".into()),
        channel: lounge,
        connected: true,
        stage: crate::voice::Stage::Connected,
        stats: Some(stats),
        level: (-30., true),
        speaking: ["8388608".to_owned()].into_iter().collect(),
        last_spoke: Default::default(),
        cameras: Default::default(),
        camera_on: false,
        watching: Vec::new(),
        focus: None,
        ended: Vec::new(),
        sharing: None,
        share_source: None, live_since: None, self_view: None, share_label: None,
        joined: std::time::Instant::now().checked_sub(std::time::Duration::from_secs(5047)).unwrap_or_else(std::time::Instant::now),
        _events: gpui_kit::Task::ready(()),
    });
    waffle.call_view = true;
    waffle.restore = None;
}

fn demo_thread() -> Channel {
    let mut thread = channel("t1", "gpui list virtualisation", ChannelKind::Thread, Some("general"));
    thread.thread = Some(crate::discord::ThreadMeta { joined: true, message_count: 12, ..Default::default() });
    thread
}

fn first_thread_chip(message: &mut Message) {
    message.thread = Some(demo_thread());
}

fn demo_dm_call(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    let zoe = user("8388608", "Wren");
    let ago = |mins: i64| (chrono::Utc::now() - chrono::Duration::minutes(mins)).to_rfc3339();
    let record = |id: &str, author: &User, start: i64, end: Option<i64>, joined: &[&str]| Message {
        id: id.into(),
        channel_id: "dm".into(),
        author: author.clone(),
        timestamp: ago(start),
        call: Some(crate::discord::CallRecord { participants: joined.iter().map(|p| p.to_string()).collect(), ended: end.map(ago) }),
        ..Default::default()
    };
    let text = |id: &str, author: &User, at: i64, content: &str| Message { id: id.into(), channel_id: "dm".into(), author: author.clone(), timestamp: ago(at), content: content.into(), ..Default::default() };
    let me = me_user();
    let messages = vec![
        record("301", &me, 180, Some(168), &["4194304", "8388608"]),
        record("302", &zoe, 90, Some(89), &["8388608"]),
        text("303", &zoe, 89, "call me when you're back"),
        text("304", &me, 3, "back now"),
    ];
    waffle.feeds.insert("dm".into(), Feed { messages: std::sync::Arc::new(messages), loaded: true, ..Default::default() });
    let state = VoiceState { guild_id: None, channel_id: Some("dm".into()), user_id: zoe.id.clone(), user: Some(zoe), muted: false, deafened: false, server_muted: false, server_deafened: false, streaming: false, camera: false };
    waffle.on_event(Event::VoiceState(state, None), cx);
    let ringing = if matches!(scene, "call-joinable" | "call-dm" | "call-dm-camera") { vec![] } else { vec![me.id.clone()] };
    waffle.on_event(Event::Call { channel_id: "dm".into(), ringing }, cx);
    if scene.starts_with("call-dm") {
        let dm = waffle.dms[0].clone();
        let (session, _) = crate::voice::join(me.id.clone(), None, dm.id.clone(), crate::voice::GatewayHandle::new().0, Default::default());
        let cameras = match scene {
            "call-dm-camera" => [("8388608".to_owned(), demo_frame(480, 640, 90))].into_iter().collect(),
            _ => Default::default(),
        };
        waffle.call = Some(crate::Call {
            session,
            guild_id: None,
            channel: dm,
            connected: true,
            stage: crate::voice::Stage::Connected,
            stats: None,
            level: (-30., false),
            speaking: ["8388608".to_owned()].into_iter().collect(),
            last_spoke: Default::default(),
            cameras,
            camera_on: false,
            watching: Vec::new(),
            focus: None,
            ended: Vec::new(),
            sharing: None,
            share_source: None, live_since: None, self_view: None, share_label: None,
            joined: std::time::Instant::now().checked_sub(std::time::Duration::from_secs(724)).unwrap_or_else(std::time::Instant::now),
            _events: gpui_kit::Task::ready(()),
        });
    }
    if scene != "call-banner" {
        let dm = waffle.dms[0].clone();
        waffle.restore = None;
        waffle.guild_id = None;
        waffle.open_channel(dm, window, cx);
    }
}

fn user(id: &str, name: &str) -> User {
    // Display-name styles: fonts show everywhere, the effects' colours only outside servers.
    use crate::discord::{NameEffect, NameStyle};
    let name_style = match id {
        "8388608" => Some(NameStyle { font: 12, effect: NameEffect::Gradient, colors: vec![0x2AAEA6, 0xFFC490] }),
        "12582912" => Some(NameStyle { font: 3, effect: NameEffect::Gummy, colors: vec![] }),
        "16777216" => Some(NameStyle { font: 14, effect: NameEffect::Solid, colors: vec![0x47BAE7] }),
        _ => None,
    };
    User { id: id.into(), name: name.into(), avatar: avatar_url(id, None), name_style, ..Default::default() }
}

fn channel(id: &str, name: &str, kind: ChannelKind, parent: Option<&str>) -> Channel {
    Channel {
        id: id.into(),
        name: name.into(),
        kind,
        parent_id: parent.map(Into::into),
        position: 0,
        topic: None,
        last_message_id: None,
        private: false,
        recipient: None,
        recipients: Vec::new(),
        icon: None,
        thread: None,
        raw_type: 0,
        overwrites: Vec::new(),
        props: Default::default(),
    }
}

fn ready(me: User) -> Ready {
    let mut general = channel("general", "👋-general", ChannelKind::Text, Some("c0"));
    general.topic = Some("Este es un buen lugar para hablar de Rust, gpui y todo lo demás".into());
    let mut staff = channel("staff", "🔒-staff-notes", ChannelKind::Text, Some("c0"));
    staff.private = true;
    staff.last_message_id = Some("900".into());
    let mut media = channel("media", "📷-media", ChannelKind::Text, Some("c0"));
    media.last_message_id = Some("900".into());
    let roles = vec![
        Role { id: "r1".into(), name: "Council".into(), color: 0xE91E63, position: 3, permissions: 0, managed: false },
        Role { id: "r2".into(), name: "Rustacean".into(), color: 0xE67E22, position: 2, permissions: 0, managed: false },
        Role { id: "1".into(), name: "@everyone".into(), color: 0, position: 0, permissions: 0, managed: false },
    ];
    let guild = |id: &str, name: &str, channels: Vec<Channel>, voice_states: Vec<VoiceState>| Guild {
        id: id.into(),
        name: name.into(),
        icon: None,
        channels,
        roles: roles.clone(),
        voice_states,
        threads: Vec::new(),
        default_notifications: 1,
        owner_id: Some("4194304".into()),
        my_roles: Vec::new(),
        emojis: Vec::new(),
        stickers: Vec::new(),
        premium_tier: 1,
        afk_channel_id: None,
    };
    let lounge_states = vec![
        VoiceState { guild_id: Some("1".into()), channel_id: Some("lounge".into()), user_id: "12582912".into(), user: Some(user("12582912", "moss lantern")), muted: false, deafened: false, server_muted: false, server_deafened: false, streaming: true, camera: false },
        VoiceState { guild_id: Some("1".into()), channel_id: Some("lounge".into()), user_id: "16777216".into(), user: Some(user("16777216", "toma reed")), muted: true, deafened: false, server_muted: false, server_deafened: false, streaming: false, camera: false },
        VoiceState { guild_id: Some("1".into()), channel_id: Some("lounge".into()), user_id: "8388608".into(), user: Some(user("8388608", "Wren")), muted: false, deafened: false, server_muted: false, server_deafened: false, streaming: false, camera: false },
        VoiceState { guild_id: Some("1".into()), channel_id: Some("lounge".into()), user_id: "20971520".into(), user: Some(user("20971520", "ferris")), muted: true, deafened: true, server_muted: false, server_deafened: false, streaming: false, camera: false },
        VoiceState { guild_id: Some("1".into()), channel_id: Some("lounge".into()), user_id: "4194304".into(), user: Some(user("4194304", "kiwi")), muted: false, deafened: false, server_muted: false, server_deafened: false, streaming: false, camera: false },
    ];
    let channels = vec![
        channel("c0", "General", ChannelKind::Category, None),
        general,
        media,
        staff,
        channel("long", "a-channel-name-long-enough-to-truncate-in-the-sidebar", ChannelKind::Text, Some("c0")),
        channel("c1", "Voice", ChannelKind::Category, None),
        channel("lounge", "hangout", ChannelKind::Voice, Some("c1")),
        channel("hub", "study-hall", ChannelKind::Voice, Some("c1")),
    ];
    let mut dm = channel("dm", "Wren", ChannelKind::Dm, None);
    dm.recipient = Some(user("8388608", "Wren"));
    Ready {
        developer_mode: true,
        account_settings: crate::discord::AccountSettings { developer_mode: true, ..Default::default() },
        my_account: crate::discord::MyAccount { username: "kiwi.dev".into(), global_name: Some("kiwi".into()), banner: None, premium: false, premium_type: 0 },
        guild_settings: Vec::new(),
        ignored: Vec::new(),
        me,
        status: Presence_::Dnd,
        custom_status: None,
        session_id: String::new(),
        guilds: vec![
            {
                let mut g = guild("1", "Night Owls", channels, lounge_states);
                g.threads = vec![demo_thread()];
                g
            },
            guild("2", "gpui", vec![channel("g2", "general", ChannelKind::Text, None)], vec![]),
            guild("3", "Zed Industries", vec![], vec![]),
            guild("4", "Bevy", vec![], vec![]),
            guild("5", "Tokio", vec![], vec![]),
        ],
        folders: vec![
            Folder { id: String::new(), name: None, color: None, guild_ids: vec!["1".into()] },
            Folder { id: "9".into(), name: Some("Rust".into()), color: Some(0xCE422B), guild_ids: vec!["2".into(), "3".into(), "4".into(), "5".into()] },
        ],
        dms: vec![dm],
        read_states: vec![
            ("media".into(), ReadState { last_read: 100, mentions: 0 }),
            ("staff".into(), ReadState { last_read: 100, mentions: 2 }),
        ],
        presences: vec![("8388608".into(), Presence { status: Presence_::Online, custom: None, activity: None, rich: None, ..Default::default() })],
        relationships: vec![
            Relationship { user: user("8388608", "Wren"), kind: Relation::Friend },
            Relationship { user: user("12582912", "moss lantern"), kind: Relation::Friend },
            Relationship { user: user("16777216", "toma reed"), kind: Relation::Incoming },
        ],
    }
}

// `load`: an account the size of a busy real one, for frame timing. 60 more servers of 100 channels, each
// channel with role overwrites, and none owned by us, so every permission check runs in full; 80 more
// channels in Night Owls; 300 members; 600 messages.
const LOAD_SERVERS: usize = 60;
const LOAD_CHANNELS: usize = 100;
const LOAD_ROLES: usize = 25;

fn load_guilds(waffle: &mut Waffle) {
    let view = crate::discord::VIEW_CHANNEL;
    let roles: Vec<Role> = (0..LOAD_ROLES).map(|r| Role { id: format!("lr{r}"), name: format!("role {r}"), color: 0x5865F2 + r as u32 * 0x0A0A, position: r as i64, permissions: if r == 0 { view } else { 0 }, managed: false }).collect();
    let overwrites = |c: usize| (0..4).map(|o| crate::discord::Overwrite { id: format!("lr{}", (c + o) % LOAD_ROLES), allow: if o == 0 { view } else { 0 }, deny: 0, member: false }).collect::<Vec<_>>();
    let channels = |g: usize, count: usize| -> Vec<Channel> {
        (0..count)
            .map(|c| {
                let (kind, parent) = match c % 10 {
                    0 => (ChannelKind::Category, None),
                    9 => (ChannelKind::Voice, Some(format!("l{g}-{}", c - c % 10))),
                    _ => (ChannelKind::Text, Some(format!("l{g}-{}", c - c % 10))),
                };
                let mut channel = channel(&format!("l{g}-{c}"), &format!("channel-{c}-of-server-{g}"), kind, parent.as_deref());
                channel.position = c as i64;
                channel.overwrites = overwrites(c);
                channel.last_message_id = Some("900".into());
                channel
            })
            .collect()
    };
    for g in 0..LOAD_SERVERS {
        waffle.guilds.push(Guild {
            id: format!("l{g}"),
            name: format!("Server {g} with a longer name"),
            icon: None,
            channels: channels(g, LOAD_CHANNELS),
            roles: roles.clone(),
            voice_states: Vec::new(),
            threads: Vec::new(),
            default_notifications: 1,
            owner_id: Some("1".into()),
            my_roles: (0..LOAD_ROLES).step_by(3).map(|r| format!("lr{r}")).collect(),
            emojis: Vec::new(),
            stickers: Vec::new(),
            premium_tier: 1,
            afk_channel_id: None,
        });
    }
    if let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") {
        guild.channels.extend(channels(0, 80).into_iter().map(|mut c| {
            c.id = format!("owl-{}", c.id);
            c.parent_id = c.parent_id.map(|p| format!("owl-{p}"));
            c.overwrites.clear();
            c
        }));
    }
}

fn load_members() -> (Vec<(String, u32)>, Vec<ListItem>) {
    let groups = [("r1", Presence_::Online, 20), ("online", Presence_::Online, 150), ("offline", Presence_::Offline, 130)];
    let mut items = Vec::new();
    for (group, status, count) in groups {
        items.push(ListItem::Group { id: group.into(), count });
        for n in 0..count {
            let id = format!("{}", 30_000_000 + items.len());
            let activity = (n % 3 == 0).then(|| Activity { kind: ActivityKind::Playing, text: format!("Game number {n}"), more: 0 });
            items.push(ListItem::Member(Member { user: user(&id, &format!("{group} member {n}")), roles: vec!["r2".into()], presence: Presence { status, custom: None, activity, rich: None, ..Default::default() } }));
        }
    }
    (groups.iter().map(|(g, _, c)| ((*g).into(), *c)).collect(), items)
}

// The fixture's messages over and over, with their own ids and times, so the list has a real length.
fn load_messages() -> Vec<Message> {
    let base = messages();
    (0..600)
        .map(|n| {
            let mut message = base[n % base.len()].clone();
            message.id = format!("{}", 10_000 + n);
            message.timestamp = format!("2026-11-{:02}T{:02}:{:02}:00Z", 1 + n / 60, (n / 4) % 24, n % 60);
            message.reply = None;
            message.reply_to = None;
            message
        })
        .collect()
}

fn members() -> Vec<ListItem> {
    let member = |id: &str, name: &str, roles: &[&str], status, activity: Option<(ActivityKind, &str)>, bot: bool| {
        let mut user = user(id, name);
        user.bot = bot;
        // A nameplate, with bundled art standing in for Discord's (the demo stays offline).
        if id == "4194304" {
            user.nameplate = Some(crate::discord::Nameplate { art: "brand/icons/paper.png".into(), tint: (0x2D6A4F, 0x95D5B2) });
        }
        ListItem::Member(Member { user, roles: roles.iter().map(|r| r.to_string()).collect(), presence: Presence {
            status,
            custom: activity.filter(|(kind, _)| *kind == ActivityKind::Custom).map(|(_, text)| text.into()),
            activity: activity.filter(|(kind, _)| *kind != ActivityKind::Custom).map(|(kind, text)| Activity { kind, text: text.into(), more: 0 }),
            rich: None,
            // Wren is only on their phone, so the panel shows the mobile mark.
            mobile_only: id == "8388608",
            streaming: activity.is_some_and(|(kind, _)| kind == ActivityKind::Streaming),
        } })
    };
    vec![
        ListItem::Group { id: "r1".into(), count: 2 },
        member("4194304", "kiwi", &["r1"], Presence_::Dnd, None, false),
        member("20971520", "Jockie Music", &["r1"], Presence_::Online, Some((ActivityKind::Custom, "m!help")), true),
        ListItem::Group { id: "online".into(), count: 3 },
        member("12582912", "moss lantern", &["r2"], Presence_::Online, Some((ActivityKind::Playing, "Apex Legends")), false),
        member("16777216", "toma reed", &[], Presence_::Idle, Some((ActivityKind::Streaming, "Speedrunning Celeste")), false),
        member("8388608", "Wren", &["r2"], Presence_::Online, None, false),
        ListItem::Group { id: "offline".into(), count: 1 },
        member("25165824", "a member whose display name runs long", &[], Presence_::Offline, None, false),
    ]
}

// design/embeds.html's examples: every field, bot logs, a video, a gallery, a poll result, the worst case.
fn embed_messages() -> Vec<Message> {
    let bot = |id: &str, name: &str| {
        let mut u = user(id, name);
        u.bot = true;
        u
    };
    let post = |id: &str, author: User, ts: &str, embeds: Vec<Embed>| Message { id: id.into(), channel_id: "general".into(), guild_id: Some("1".into()), author, timestamp: ts.into(), embeds, ..Default::default() };
    let pic = |seed: &str, width: f32, height: f32| Media { url: avatar_url(seed, None), width, height, kind: MediaKind::Image };
    let field = |name: &str, value: &str, inline: bool| (name.to_owned(), value.to_owned(), inline);
    let now = chrono::Utc::now();
    let ago = |min: i64| (now - chrono::Duration::minutes(min)).to_rfc3339();
    let soon = |min: i64| (now + chrono::Duration::minutes(min)).timestamp();
    let kitchen = Embed {
        color: Some(0xE67E22),
        author: Some("Raidbot · Night owls".into()),
        author_url: Some("https://example.com/raid".into()),
        author_icon: Some(avatar_url("1", None)),
        title: Some("Raid tonight: Ashen Peaks, heroic".into()),
        url: Some("https://example.com/r/2291".into()),
        description: Some(format!("Sign-ups close <t:{}:R>. Bring **fire resist**, and read the [route notes](https://example.com/notes) first.\n> Ping <@&r1> if you can't make it.", soon(90))),
        fields: vec![field("Starts", &format!("<t:{}:t>", soon(150)), true), field("Signed up", "17 of 20", true), field("Lead", "<@8388608>", true), field("Roles still open", "1 tank, 2 healers. Reply with `/join` and your role.", false)],
        thumbnail: Some(pic("2", 128., 128.)),
        images: vec![pic("3", 1280., 640.)],
        footer: Some("Raid #2291".into()),
        footer_icon: Some(avatar_url("1", None)),
        timestamp: Some(ago(4)),
        ..Default::default()
    };
    let voice = |joined: bool| Embed {
        color: Some(if joined { 0x43B581 } else { 0xF04747 }),
        author: Some("_kiwi".into()),
        author_icon: Some(avatar_url("4194304", None)),
        description: Some(format!("<@4194304> **{} voice channel** <#lounge>", if joined { "joined" } else { "left" })),
        footer: Some("ID: 4194304".into()),
        timestamp: Some(ago(if joined { 3 } else { 0 })),
        ..Default::default()
    };
    let ban = Embed {
        color: Some(0xC21F3A),
        author: Some("Member banned".into()),
        author_icon: Some(avatar_url("5", None)),
        description: Some("**spam_account_92** was banned from Night owls.".into()),
        fields: vec![field("User", "<@12582912>", true), field("Moderator", "<@8388608>", true), field("Duration", "Permanent", true), field("Reason", "Posting scam links in <#general> after a warning.", false)],
        footer: Some("Case 418".into()),
        timestamp: Some(ago(30)),
        ..Default::default()
    };
    let stats = Embed {
        color: Some(0x3498DB),
        title: Some("Wren's season so far".into()),
        url: Some("https://example.com/u/wren".into()),
        thumbnail: Some(pic("4", 128., 128.)),
        fields: vec![field("Rank", "Diamond II", true), field("Win rate", "58%", true), field("Matches", "214", true), field("Best hero", "Tide", true), field("Last match", "Win on Ashen Peaks, 13 to 9", false)],
        footer: Some("stats.example".into()),
        ..Default::default()
    };
    let plain = Embed { description: Some(format!("Servers go down for maintenance <t:{}:F>. It should take about an hour.", soon(60 * 24))), ..Default::default() };
    let video = Embed {
        video: true,
        color: Some(0xFF0000),
        url: Some("https://example.com/watch?v=s7".into()),
        provider: Some("VideoSite".into()),
        author: Some("Rift Notes".into()),
        title: Some("Every change in Season 7, in 9 minutes".into()),
        images: vec![pic("5", 1280., 720.)],
        ..Default::default()
    };
    let gallery = Embed {
        color: Some(0x9B59B6),
        url: Some("https://example.com/peaks-set".into()),
        author: Some("Rosa on ArtSite".into()),
        author_icon: Some(avatar_url("6", None)),
        title: Some("Ashen Peaks, concept set".into()),
        description: Some("Day, dusk, night and the storm pass.".into()),
        images: ["1", "2", "3", "4"].map(|s| pic(s, 800., 600.)).to_vec(),
        footer: Some("ArtSite".into()),
        ..Default::default()
    };
    let poll = |winner: bool| Embed {
        poll: Some(crate::discord::PollResult {
            question: "Which map first tonight?".into(),
            winner: winner.then(|| ("Ashen Peaks".into(), Some(Emoji::unicode("🏔️")))),
            winner_votes: if winner { 68 } else { 0 },
            total_votes: if winner { 142 } else { 12 },
        }),
        ..Default::default()
    };
    let notes = "Patch notes for **Season 7**.\n\n# Ashen Peaks\nThe new map flips between day and night every four minutes. Sightlines change with the light, so routes that are safe at noon are deadly at dusk.\n\n## Heroes\n- **Ember**, a close-range brawler who heals on burn damage\n- **Tide**, a support who pushes and pulls with water walls\n\n## Ranked\n> The ladder now resets softly: you drop one division instead of two tiers.\n\nPlacement matches are down from ten to five, and duo queue opens at every rank.\n\n```\nqueue: ranked\nplacements: 5\n```\n-# Full notes on the site.";
    let worst = Embed {
        color: Some(0xFFFFFF),
        author: Some("Bartholomew Featherstonehaugh-Montgomery of the Very Long Display Name and Several Other Titles".into()),
        author_icon: Some(avatar_url("2", None)),
        title: Some("This title runs to the full two hundred and fifty-six characters that Discord allows, which is far longer than anyone should write, so it has to wrap cleanly across several lines without pushing the thumbnail away or overflowing the card at any width at all, ok".into()),
        url: Some("https://example.com".into()),
        description: Some(format!("{notes}\nAn unbroken link: https://clips.example/watch/7f3a9c2e4b1d8a6f0e5c3b2a1d9e8f7c6b5a4d3e2f1a0b9c8d7e6f5a4b3c2d1e0")),
        thumbnail: Some(pic("3", 128., 128.)),
        fields: ["Region", "Queue", "Ping", "Players", "Mode", "Map", "Season", "Patch", "A field name long enough to wrap onto a second line"]
            .iter()
            .zip(["EU West", "Ranked", "24 ms", "9,812", "Control", "Ashen Peaks", "7", "7.0.3", "and a value that runs on for a while too"])
            .map(|(n, v)| field(n, v, true))
            .collect(),
        footer: Some(format!("A footer long enough to wrap: {}", "server status, region, build and shard ".repeat(3).trim())),
        footer_icon: Some(avatar_url("4", None)),
        timestamp: Some(ago(60 * 26)),
        ..Default::default()
    };
    let black = Embed { color: Some(0), title: Some("Colour 0x000000".into()), description: Some("A pure black bar, as Discord draws it.".into()), ..Default::default() };
    let sensitive = Embed { color: Some(0x95A5A6), title: Some("Uncensored match highlights".into()), url: Some("https://example.com/h".into()), sensitive: true, images: vec![pic("5", 1280., 720.)], footer: Some("clips.example".into()), ..Default::default() };
    let mut poll_post = post("206", bot("16777216", "Wren"), "2026-11-20T18:30:00Z", vec![poll(true), poll(false)]);
    poll_post.reply_to = Some("201".into());
    vec![
        post("201", bot("33554432", "Raidbot"), "2026-11-20T12:00:00Z", vec![kitchen]),
        post("202", bot("50331648", "Logger"), "2026-11-20T12:49:00Z", vec![voice(true), voice(false)]),
        post("203", bot("67108864", "Dyno"), "2026-11-20T13:10:00Z", vec![ban]),
        post("204", bot("83886080", "Statsy"), "2026-11-20T15:30:00Z", vec![stats, plain]),
        post("205", user("8388608", "Wren"), "2026-11-20T18:12:00Z", vec![video, gallery]),
        poll_post,
        post("207", bot("100663296", "Stressbot"), "2026-11-20T21:00:00Z", vec![worst, black, sensitive]),
    ]
}

fn messages() -> Vec<Message> {
    let message = |id: &str, author: User, roles: &[&str], ts: &str, content: &str| Message {
        id: id.into(),
        channel_id: "general".into(),
        guild_id: Some("1".into()),
        author,
        roles: Some(roles.iter().map(|r| r.to_string()).collect()),
        content: content.into(),
        timestamp: ts.into(),
        ..Default::default()
    };
    let me = user("4194304", "kiwi");
    let mut abyss = user("12582912", "moss lantern");
    abyss.tag = Some(GuildTag { text: "FNC".into(), badge: None });
    let zoe = user("8388608", "Wren");
    let mut first = message("101", abyss.clone(), &["r2"], "2026-11-13T17:18:00Z", "Morning! Anyone tried the new **gpui** list? It virtualises rows, so long channels stay cheap.");
    first.reactions = vec![Reaction { emoji: Emoji::unicode("🔥"), count: 3, me: true }, Reaction { emoji: Emoji::unicode("👀"), count: 1, me: false }];
    let mut image = message("102", zoe.clone(), &["r2"], "2026-11-13T17:20:00Z", "look at this");
    image.media = vec![
        Media { url: avatar_url("3", None), width: 256., height: 256., kind: MediaKind::Image },
        Media { url: "https://example.com/clip.mp4".into(), width: 256., height: 256., kind: MediaKind::Video { poster: avatar_url("4", None) } },
    ];
    image.embeds = vec![Embed {
        url: Some("https://www.rust-lang.org".into()),
        color: Some(0xCE422B),
        provider: Some("rust-lang.org".into()),
        title: Some("Rust Programming Language".into()),
        description: Some("A language empowering everyone to build reliable and efficient software.".into()),
        thumbnail: Some(Media { url: avatar_url("1", None), width: 80., height: 80., kind: MediaKind::Image }),
        ..Default::default()
    }, Embed {
        color: Some(0x43B581),
        author: Some("_kiwi".into()),
        author_icon: Some(avatar_url("4194304", None)),
        description: Some("**<@8388608> joined voice channel <#lounge>**".into()),
        footer: Some("ID: 8388608".into()),
        timestamp: Some("2026-11-14T08:12:00Z".into()),
        ..Default::default()
    }];
    first_thread_chip(&mut image);
    image.pinned = true;
    let mut pin_notice = message("107", me.clone(), &["r1"], "2026-11-14T08:21:00Z", "");
    (pin_notice.pin_notice, pin_notice.reply_to) = (true, Some("102".into()));
    let mut reply = message("103", me.clone(), &["r1"], "2026-11-14T08:15:00Z", "discord on rust <@8388608>, check <#media>");
    reply.reply_to = Some("101".into());
    reply.reply = Some(Box::new(first.clone()));
    reply.mentions = vec![zoe.clone()];
    reply.edited = true;
    let mut mention = message("104", zoe.clone(), &["r2"], "2026-11-14T08:17:00Z", "<@4194304> it already feels faster than the real one");
    mention.mentions = vec![me.clone()];
    vec![
        first,
        image,
        reply,
        mention,
        message("105", abyss.clone(), &["r2"], "2026-11-14T08:18:00Z", "```rust\nfn main() {\n    println!(\"hi\");\n}\n```"),
        // Discord keeps each line and its indent.
        Message { pinned: true, ..message("106", abyss, &["r2"], "2026-11-14T08:20:00Z", "the build so far:\n    Brightline 8-core CPU\n    Coldfront 240 mm cooler\n    32 GB memory\n<@4194304>") },
        pin_notice,
    ]
}

// The header search focused: its filters and made-up history, or a filter's values being picked. The field
// is filled once the window is up, as the mention scene does.
fn demo_search(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    use crate::search::{Saved, SavedToken};
    let token = |start: usize, end: usize, label: &str| SavedToken { start, end, label: label.into() };
    let history = vec![
        Saved { text: "from:8388608 in:media has:video".into(), tokens: vec![token(0, 12, "Wren"), token(13, 21, "📷-media"), token(22, 31, "video")] },
        Saved { text: "boss fight from:12582912 tho".into(), tokens: vec![token(11, 24, "moss lantern")] },
        Saved { text: "patch notes before:2026-11-01".into(), tokens: vec![token(12, 29, "Nov 1, 2026")] },
        Saved { text: "the absolutely final tournament bracket v12 FINAL really final".into(), tokens: Vec::new() },
    ];
    waffle.prefs.search_history.insert(me_user().id, history);
    let content = match scene {
        "search-from" => Saved { text: "in:general from:".into(), tokens: vec![token(0, 10, "👋-general")] }.to_content(),
        "search-date" => "dodge before:".into(),
        _ => "".into(),
    };
    cx.spawn_in(window, async move |this, cx| {
        cx.background_executor().timer(std::time::Duration::from_millis(500)).await;
        this.update_in(cx, |this, window, cx| {
            this.search_input.update(cx, |state, cx| {
                state.set_value(content, window, cx);
                state.focus(window, cx);
            });
            cx.notify();
        })
        .ok();
    })
    .detach();
}

// A page of results for `from:Wren has:image`: one with a picture, one with a gallery, one from another
// channel; Filters open over them in "search-filters".
fn demo_search_results(waffle: &mut Waffle, filters_open: bool, window: &mut Window, cx: &mut Context<Waffle>) {
    let mut hits: Vec<Message> = messages().into_iter().rev().take(3).collect();
    // Real shapes: the avatars are square, the banner is 600×240.
    let avatar = |id: &str| Media { url: avatar_url(id, None), width: 256., height: 256., kind: MediaKind::Image };
    let banner = Media { url: "demo/banner.png".into(), width: 600., height: 240., kind: MediaKind::Image };
    for (i, hit) in hits.iter_mut().enumerate() {
        hit.author = user("8388608", "Wren");
        hit.media = match i {
            0 => vec![banner.clone()],
            1 => vec![avatar("5"), avatar("2"), avatar("4")],
            _ => Vec::new(),
        };
    }
    if let Some(last) = hits.last_mut() {
        last.channel_id = "media".into();
        last.content = "the night sky from the ridge, more in the thread".into();
        last.media = vec![avatar("2")];
    }
    waffle.right_panel = crate::RightPanel::Search(Box::new(crate::search_results::Results::demo(hits, 880, 2, filters_open)));
    let content = crate::search::Saved { text: "from:8388608 has:image".into(), tokens: vec![crate::search::SavedToken { start: 0, end: 12, label: "Wren".into() }, crate::search::SavedToken { start: 13, end: 22, label: "image".into() }] }.to_content();
    waffle.search_input.update(cx, |state, cx| state.set_value(content, window, cx));
}

// Search in the DM with Wren: the filters without `in:`, or a page of results from the DM.
fn demo_search_dm(waffle: &mut Waffle, results: bool, window: &mut Window, cx: &mut Context<Waffle>) {
    let Some(dm) = waffle.dms.iter().find(|d| d.id == "dm").cloned() else { return };
    waffle.guild_id = None;
    waffle.open_channel(dm, window, cx);
    if results {
        let picture = Media { url: "demo/banner.png".into(), width: 600., height: 240., kind: MediaKind::Image };
        let hit = |id: &str, author: User, content: &str, media: Vec<Media>| Message { id: id.into(), channel_id: "dm".into(), author, timestamp: "2026-11-12T21:40:00+00:00".into(), content: content.into(), media, ..Default::default() };
        let hits = vec![hit("501", user("8388608", "Wren"), "here's the photo from the ridge", vec![picture]), hit("502", me_user(), "photo dump later tonight?", Vec::new())];
        waffle.right_panel = crate::RightPanel::Search(Box::new(crate::search_results::Results::demo(hits, 2, 1, false)));
        let content = crate::search::Saved { text: "has:image photo".into(), tokens: vec![crate::search::SavedToken { start: 0, end: 9, label: "image".into() }] }.to_content();
        waffle.search_input.update(cx, |state, cx| state.set_value(content, window, cx));
        return;
    }
    demo_search(waffle, "search", window, cx);
}

// Made-up overwrites and settings on #general and the hangout voice channel, then the window on one page.
fn demo_channel_settings(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    use crate::{channel_settings::Page, discord::Overwrite};
    const ATTACH: u64 = 1 << 15;
    const MENTION_EVERYONE: u64 = 1 << 17;
    let ow = |id: &str, allow: u64, deny: u64, member: bool| Overwrite { id: id.into(), allow, deny, member };
    let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") else { return };
    for c in guild.channels.iter_mut() {
        match c.id.as_str() {
            "c0" => c.overwrites = vec![ow("1", 0, ATTACH, false)],
            "general" => {
                c.topic = Some("Say hi, share what you're building. Spoilers go in ||bars||.".into());
                c.props.slowmode = 30;
                c.props.archive_after = 4320;
                c.overwrites = vec![ow("1", 0, ATTACH, false), ow("r2", ATTACH, MENTION_EVERYONE, false), ow("8388608", 1 << 13, 0, true)];
            }
            "lounge" => {
                c.raw_type = 2;
                c.props.bitrate = 64_000;
                c.props.user_limit = 12;
            }
            _ => {}
        }
    }
    let id = if scene.starts_with("channel-settings-voice") { "lounge" } else { "general" };
    let Some(channel) = waffle.guild().and_then(|g| g.channels.iter().find(|c| c.id == id).cloned()) else { return };
    waffle.open_channel_settings(channel, "1".into(), window, cx);
    let page = match scene {
        "channel-settings-perms" | "channel-settings-voice-perms" => Page::Permissions,
        "channel-settings-invites" => Page::Invites,
        _ => Page::Overview,
    };
    if let Some(view) = waffle.channel_settings.as_mut() {
        view.demo(page);
    }
    if scene == "channel-settings-search" {
        waffle.demo_channel_search("slow", window, cx);
    }
    if scene == "channel-settings-dirty" {
        waffle.demo_channel_edit(window, cx);
    }
}

fn me_user() -> User {
    user("4194304", "kiwi")
}

fn account(user: &User, status: Presence_) -> crate::accounts::Account {
    crate::accounts::Account { id: user.id.clone(), name: user.name.clone(), avatar: user.avatar.clone(), status }
}

fn demo_attachments(waffle: &mut Waffle, window: &mut Window, cx: &mut Context<Waffle>) {
    use std::sync::{Arc, atomic::AtomicU64};

    use crate::{SendState, UploadJob, discord::Upload};
    let dir = std::env::temp_dir().join("waffle-demo-attachments");
    let _ = std::fs::create_dir_all(&dir);
    let picture = |name: &str, tint: u8| {
        let path = dir.join(name);
        let image = image::RgbaImage::from_fn(1600, 1000, |x, y| image::Rgba([tint.wrapping_add((x / 8) as u8), (60 + y / 6) as u8, (140 + (x + y) / 30) as u8, 255]));
        let _ = image.save(&path);
        path
    };
    let filler = |name: &str, size: usize| {
        let path = dir.join(name);
        let _ = std::fs::write(&path, vec![b'x'; size]);
        path
    };
    let paths = vec![
        picture("dusk-over-the-harbour.png", 200),
        picture("plot-twist.png", 40),
        filler("first-flight.mp4", 3_400_000),
        filler("lantern-festival-flyer.pdf", 820_000),
        filler("save-files.zip", 6_100_000),
        filler("a-very-long-file-name-that-keeps-going-well-past-the-tile-edge-v12-final.txt", 2_048),
    ];
    waffle.add_files(paths.clone(), window, cx);
    if let Some(spoiler) = waffle.pending_files.get_mut(1) {
        spoiler.spoiler = true;
    }
    let job = |files: &[(&std::path::PathBuf, f64)], error: Option<&str>| UploadJob {
        channel_id: "general".into(),
        files: files
            .iter()
            .map(|(path, done)| {
                let size = std::fs::metadata(path).map_or(0, |m| m.len());
                let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                Upload { path: (*path).clone(), name, size, sent: Arc::new(AtomicU64::new((size as f64 * done) as u64)) }
            })
            .collect(),
        content: String::new(),
        reply_to: None,
        ping: true,
        error: error.map(str::to_owned),
    };
    waffle.uploads.insert("demo-up-1".into(), job(&[(&paths[0], 1.0), (&paths[4], 0.42)], None));
    waffle.uploads.insert("demo-up-2".into(), job(&[(&paths[3], 0.0)], Some("Upload failed: 413 Payload Too Large")));
    let post = |id: &str, content: &str, state: SendState| Message {
        id: format!("pending-{id}"),
        channel_id: "general".into(),
        guild_id: Some("1".into()),
        author: me_user(),
        content: content.into(),
        timestamp: "2026-11-14T08:24:00Z".into(),
        nonce: Some(id.into()),
        state,
        ..Default::default()
    };
    // Through push_local, so the open feed's list grows with them.
    waffle.push_local(post("demo-up-1", "the harbour shots from saturday", SendState::Sending), cx);
    waffle.push_local(post("demo-up-2", "", SendState::Failed), cx);
}

fn demo_text_files(waffle: &mut Waffle, scene: &str) {
    use crate::{discord::File, text_file::TextFile};
    let notes: String = PATCH_NOTES.repeat(3);
    let hello = "fn main() {\n    let name = std::env::args().nth(1).unwrap_or(\"world\".into());\n    println!(\"hello, {name}\");\n}\n";
    let file = |name: &str, size: usize| File { name: name.into(), url: format!("https://cdn.discordapp.com/attachments/demo/{name}"), size: size as u64 };
    let (long, short, binary) = (file("message.txt", notes.len()), file("hello.rs", hello.len()), file("crash-dump.log", 48_213));
    {
        let mut texts = waffle.text_files.borrow_mut();
        texts.files.insert(long.url.clone(), TextFile::Ready { text: notes.into(), truncated: false });
        texts.files.insert(short.url.clone(), TextFile::Ready { text: hello.into(), truncated: false });
        texts.files.insert(binary.url.clone(), TextFile::Unreadable);
        match scene {
            "text-viewer" => texts.viewer = Some(long.clone()),
            "text-expanded" => drop(texts.expanded.insert(long.url.clone())),
            _ => {}
        }
    }
    let Some(feed) = waffle.feeds.get_mut("general") else { return };
    let mut messages = feed.messages.as_ref().clone();
    let author = user("12582912", "moss lantern");
    let post = |id: &str, files: Vec<File>| Message { id: id.into(), channel_id: "general".into(), guild_id: Some("1".into()), author: author.clone(), timestamp: "2026-11-14T08:20:00Z".into(), files, ..Default::default() };
    messages.push(post("106", vec![long]));
    messages.push(post("107", vec![short, binary]));
    feed.messages = std::sync::Arc::new(messages);
}

// Wren forwards a clip from #📷-media into #👋-general, with a note under it.
fn demo_report(waffle: &mut Waffle, scene: &str, cx: &mut Context<Waffle>) {
    use crate::report::{ReportMenu, ReportTarget};
    let Some(message) = waffle.feeds.get("general").and_then(|f| f.messages.iter().find(|m| m.id == "102").cloned()) else { return };
    let target = match scene {
        "report-user" => ReportTarget::User { user: Box::new(message.author.clone()), guild_id: Some("1".into()) },
        _ => ReportTarget::Message(Box::new(message)),
    };
    waffle.open_report(target, cx);
    waffle.report_menu_loaded(ReportMenu::parse(&sample_report_menu()), cx);
    if scene == "report" || scene == "report-user" {
        return;
    }
    waffle.report_step("2".into(), Some("Spam".into()), cx);
    waffle.report_step("10".into(), Some("Unsolicited ads or links".into()), cx);
    if let Some(dialog) = waffle.report_dialog.as_mut() {
        dialog.also_block = true;
    }
    if scene == "report-sent" {
        waffle.report_sent(true, cx);
    }
}

// Discord's shape (docs/discord-report-re.md §7) with made-up reasons and copy.
fn sample_report_menu() -> serde_json::Value {
    let preview = serde_json::json!({ "type": "message_preview", "name": "message_preview", "data": null });
    serde_json::json!({
        "version": "demo", "language": "en", "root_node_id": 1, "success_node_id": 90, "fail_node_id": 91,
        "nodes": {
            "1": { "id": 1, "header": "What's wrong with this message?", "subheader": "Pick the closest match. Reports are anonymous.", "elements": [preview, { "type": "user_preview", "name": "user_preview" }],
                   "children": [["Spam", 2], ["Abuse or harassment", 3], ["Harmful content", 4], ["Something else", 5]] },
            "2": { "id": 2, "header": "What kind of spam?", "elements": [], "children": [["Unsolicited ads or links", 10], ["Scam or phishing", 10], ["Flooding the chat", 10]] },
            "3": { "id": 3, "header": "Who is it aimed at?", "elements": [], "children": [["Me", 10], ["Someone else", 10], ["A group of people", 10]] },
            "4": { "id": 4, "header": "What does it show?", "elements": [], "children": [["Violence or gore", 10], ["Sexual content", 10], ["Self-harm", 11]] },
            "5": { "id": 5, "header": "Tell us more", "elements": [
                     { "type": "checkbox", "name": "details", "should_submit_data": true, "data": [["imp", "Impersonation", "Pretending to be someone else", false], ["pii", "Private information", "", false]] }
                   ], "button": { "type": "next", "target": 10 } },
            "10": { "id": 10, "header": "Submit your report", "subheader": "We'll look at this message and what it's in reply to.", "elements": [preview, { "type": "breadcrumbs", "name": "breadcrumbs" }], "button": { "type": "submit" } },
            "11": { "id": 11, "header": "You're not alone", "info": "If someone is in danger, contact local emergency services. Crisis lines are listed at [discord.com/safety](https://discord.com/safety).",
                    "elements": [{ "type": "external_link", "name": "help", "data": { "url": "https://discord.com/safety", "link_text": "Safety Center", "link_description": "Resources and crisis lines" } }],
                    "button": { "type": "next", "target": 10 } },
            "90": { "id": 90, "header": "Thanks for letting us know", "subheader": "We'll review it against the Community Guidelines.", "button": { "type": "done" },
                    "elements": [{ "type": "success", "name": "success" }, { "type": "breadcrumbs", "name": "breadcrumbs" }, { "type": "block_users", "name": "block" }, { "type": "ignore_users", "name": "ignore" }, { "type": "delete_message", "name": "delete" }] },
            "91": { "id": 91, "header": "Something went wrong", "elements": [], "button": { "type": "done" } }
        }
    })
}

fn demo_forwarded(waffle: &mut Waffle) {
    let Some(feed) = waffle.feeds.get_mut("general") else { return };
    let wren = user("8388608", "Wren");
    let original = Message {
        id: "880".into(),
        channel_id: "media".into(),
        guild_id: Some("1".into()),
        content: "caught this on stream last night".into(),
        timestamp: (chrono::Utc::now() - chrono::Duration::days(1)).to_rfc3339(),
        media: vec![Media { url: avatar_url("3", None), width: 320., height: 200., kind: MediaKind::Image }],
        ..Default::default()
    };
    let post = |id: &str, content: &str, forward: Option<Box<Message>>| Message {
        id: id.into(),
        channel_id: "general".into(),
        guild_id: Some("1".into()),
        author: wren.clone(),
        content: content.into(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        forward,
        ..Default::default()
    };
    let mut messages = feed.messages.as_ref().clone();
    messages.push(post("901", "", Some(Box::new(original))));
    messages.push(post("902", "you two need to see this", None));
    feed.messages = std::sync::Arc::new(messages);
    waffle.list.reset(waffle.feeds.get("general").map_or(0, |f| f.messages.len()));
}

fn demo_link_previews(waffle: &mut Waffle) {
    use crate::{discord::Invite, link_preview::Preview};
    let mut announcements = channel("1100", "📌-announcements", ChannelKind::Text, Some("c0"));
    announcements.position = 0;
    if let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") {
        guild.channels.push(announcements);
    }
    let quoted = Message {
        id: "1101".into(),
        channel_id: "1100".into(),
        guild_id: Some("1".into()),
        author: user("8388608", "Wren"),
        timestamp: (chrono::Utc::now() - chrono::Duration::hours(3)).to_rfc3339(),
        content: "Season 7 notes are up: placements drop to **5 matches**, duo queue opens at every rank, and Foundry is back with the smaller mid. Ping <@4194304> if your rank looks off after the reset, we're collecting reports in one thread so nothing gets lost.".into(),
        media: vec![Media { url: "brand/icons/sticker.png".into(), width: 512., height: 512., kind: MediaKind::Image }],
        ..Default::default()
    };
    {
        let mut previews = waffle.link_previews.borrow_mut();
        previews.seed("m-1100-1101", Preview::Message(Box::new(quoted)));
        previews.seed("m-1100-1199", Preview::Unavailable);
        previews.seed("i-night-owls", Preview::Invite(Invite { guild_id: "1".into(), name: "Night Owls".into(), icon: None, online: Some(42), members: Some(318) }));
        let tea = Invite { guild_id: "77".into(), name: "Late Night Tea House, a server with a name long enough to truncate".into(), icon: None, online: Some(1_204), members: Some(8_311) };
        previews.seed("i-tea-house", Preview::Invite(tea));
        previews.seed("i-expired-code", Preview::Unavailable);
    }
    let Some(feed) = waffle.feeds.get_mut("general") else { return };
    let mut messages = feed.messages.as_ref().clone();
    let before = messages.len();
    let ago = |mins: i64| (chrono::Utc::now() - chrono::Duration::minutes(mins)).to_rfc3339();
    let post = |id: &str, author: User, mins: i64, content: &str| Message { id: id.into(), channel_id: "general".into(), guild_id: Some("1".into()), author, timestamp: ago(mins), content: content.into(), ..Default::default() };
    messages.extend([
        post("121", user("16777216", "toma reed"), 12, "still fetching https://discord.gg/still-loading https://ptb.discord.com/channels/1/1100/1150"),
        post("122", user("16777216", "toma reed"), 11, "this one's dead https://discord.gg/expired-code and I can't open https://canary.discord.com/channels/1/1100/1199"),
        post("123", user("8388608", "Wren"), 9, "come hang after ranked https://discord.gg/tea-house and bring people here too https://discord.gg/night-owls"),
        post("124", user("12582912", "moss lantern"), 6, "older notes are pinned in https://discord.com/channels/1/1100 if you missed them"),
        post("125", me_user(), 2, "did everyone see this? https://discord.com/channels/1/1100/1101"),
    ]);
    feed.messages = std::sync::Arc::new(messages);
    // Appended like arriving messages, so the feed stays pinned to the newest.
    waffle.list.splice(before..before, feed.messages.len() - before);
}

const PATCH_NOTES: &str = "Patch notes, season 7

Ranked
- Placement matches drop from 10 to 5; your old rank seeds the first one.
- Leaving a match early now costs twice the rank it used to, and the second leave in a day locks the queue for 30 minutes.
- Duo queue opens at every rank. Trios stay closed above Diamond.
- A very long line with no spaces to check wrapping: https://example.com/a/really/long/path/that/keeps/going/and/going/and/going/without/any/break/at/all/until/the/end

Maps
- Harbor: the long sightline from B main to the crane is blocked by new containers.
- Foundry returns to the pool with the smaller mid it had in season 5.
- Every map gets a second spawn barrier, so the first ten seconds can't be peeked.

Bug fixes
- The scoreboard no longer swaps teams' colours after half time.
- Crouch-peeking a ledge no longer shows your head through the wall.
- Reconnecting mid-round puts you back on your team instead of spectating.

";

// A made-up picture: a diagonal wash with a few lighter "windows", so tiles read as real video. BGRA
// bytes, as gpui's render images expect.
fn demo_frame(width: u32, height: u32, tint: u8) -> std::sync::Arc<gpui_kit::RenderImage> {
    let panel = |x: u32, y: u32| (x * 8 / width + y * 6 / height).is_multiple_of(5);
    let buffer = image::RgbaImage::from_fn(width, height, |x, y| {
        let t = ((x + y) * 160 / (width + height)) as u8;
        let lift = if panel(x, y) { 70 } else { 0 };
        image::Rgba([tint.wrapping_add(t / 2).saturating_add(lift), (40 + t / 3).saturating_add(lift), (90 + t / 2).saturating_add(lift), 255])
    });
    std::sync::Arc::new(gpui_kit::RenderImage::new([image::Frame::new(buffer)]))
}

// The viewer scene's items: (width, height, is a clip), every fourth a clip.
const DEMO_SIZES: [(f32, f32); 20] = [
    (1600., 1067.), (1080., 1350.), (2048., 1152.), (1280., 720.), (1200., 1200.), (900., 1600.), (2400., 1600.), (1280., 720.), (1500., 1000.), (1920., 1080.),
    (1080., 1080.), (1280., 720.), (1600., 900.), (800., 1200.), (2000., 1333.), (1280., 720.), (1350., 1080.), (1024., 768.), (1440., 1800.), (1280., 720.),
];

fn demo_item(n: usize) -> Media {
    let (width, height) = DEMO_SIZES[n];
    if n % 4 == 3 {
        let url = format!("https://cdn.discordapp.com/attachments/demo/{n}/clip-{n}.mp4");
        return Media { kind: MediaKind::Video { poster: format!("https://media.discordapp.net/attachments/demo/{n}/clip-{n}.mp4?format=jpeg") }, url, width, height };
    }
    Media { url: format!("https://cdn.discordapp.com/attachments/demo/{n}/photo-{n}.png"), width, height, kind: MediaKind::Image }
}

// Serves the viewer scene's media without the network: a clip from testdata, and for pictures and
// posters a made-up gradient at whatever size the URL asks the proxy for.
fn demo_media(url: &str) -> Option<bytes::Bytes> {
    let rest = url.split("/attachments/demo/").nth(1)?;
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    if path.ends_with(".mp4") && !query.contains("format=") {
        return std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/video-aac.mp4")).ok().map(Into::into);
    }
    let n: usize = path.split('/').next()?.parse().ok()?;
    let (w, h) = DEMO_SIZES.get(n).copied()?;
    let param = |key: &str| query.split('&').find_map(|kv| kv.strip_prefix(key)?.strip_prefix('=')?.parse::<u32>().ok());
    let (w, h) = (param("width").unwrap_or(w as u32), param("height").unwrap_or(h as u32));
    let bytes = demo_picture(n, w, h);
    eprintln!("[demo media] {}x{} {} bytes {url}", w, h, bytes.len());
    Some(bytes.into())
}

// A soft two-hue gradient with a sun, distinct per item.
fn demo_picture(n: usize, w: u32, h: u32) -> Vec<u8> {
    let hue = (n as f32 * 47.) % 360.;
    let (a, b) = (hsl(hue, 0.55, 0.62), hsl((hue + 40.) % 360., 0.5, 0.32));
    let (cx, cy, r) = (w as f32 * (0.25 + 0.05 * (n % 10) as f32), h as f32 * 0.4, w.min(h) as f32 * 0.16);
    let picture = image::RgbImage::from_fn(w, h, |x, y| {
        let t = (x as f32 / w as f32 * 0.4 + y as f32 / h as f32 * 0.6).min(1.);
        let sun = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() < r;
        let mix = |i: usize| if sun { 245 } else { (a[i] as f32 * (1. - t) + b[i] as f32 * t) as u8 };
        image::Rgb([mix(0), mix(1), mix(2)])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    picture.write_to(&mut out, image::ImageFormat::Png).ok();
    out.into_inner()
}

fn hsl(h: f32, s: f32, l: f32) -> [u8; 3] {
    let c = (1. - (2. * l - 1.).abs()) * s;
    let x = c * (1. - ((h / 60.) % 2. - 1.).abs());
    let (r, g, b) = match (h / 60.) as u32 {
        0 => (c, x, 0.),
        1 => (x, c, 0.),
        2 => (0., c, x),
        3 => (0., x, c),
        4 => (x, 0., c),
        _ => (c, 0., x),
    };
    let m = l - c / 2.;
    [((r + m) * 255.) as u8, ((g + m) * 255.) as u8, ((b + m) * 255.) as u8]
}

fn demo_custom_status() -> crate::discord::CustomStatus {
    crate::discord::CustomStatus { text: "Reviewing the parity spec, back at 5".into(), emoji: Some(crate::discord::Emoji::unicode("🍡")), expires_at: None }
}

// design/messages.html's two boards, the second with your vote in and its longest answer wrapping.
fn demo_polls(waffle: &mut Waffle) {
    use crate::discord::{Poll, PollAnswer};
    let answer = |id: u32, emoji: &str, text: &str, count: u64, me: bool| PollAnswer { id, text: text.into(), emoji: Some(Emoji::unicode(emoji)), count, me };
    let ends = (chrono::Utc::now() + chrono::Duration::minutes(3 * 60 + 10)).to_rfc3339();
    let poll = |mine: bool| Poll {
        question: "Which map first tonight?".into(),
        answers: vec![
            answer(1, "🏔️", "Ashen Peaks", 67 + u64::from(mine), mine),
            answer(2, "🌊", "Drowned Harbor", 50, false),
            answer(3, "🌵", "Glass Desert, the one with the sandstorms that last forever", 24, false),
        ],
        expiry: Some(ends.clone()),
        multiselect: false,
        finalized: false,
    };
    let post = |id: &str, author: User, poll: Poll| Message { id: id.into(), channel_id: "general".into(), guild_id: Some("1".into()), author, timestamp: chrono::Utc::now().to_rfc3339(), poll: Some(poll), ..Default::default() };
    if let Some(feed) = waffle.feeds.get_mut("general") {
        std::sync::Arc::make_mut(&mut feed.messages).extend([post("195", user("8388608", "Jun"), poll(false)), post("196", user("12582912", "moss lantern"), poll(true))]);
    }
    waffle.list.reset(waffle.feeds.get("general").map_or(0, |f| f.messages.len()));
}

// Real ids from Discord's own packs (animated APNG), so the pictures load; Lottie ones stand in for the
// format Waffle can't play.
fn demo_stickers(waffle: &mut Waffle) {
    use crate::discord::{Sticker, StickerFormat, StickerPack};
    let sticker = |id: &str, name: &str, format| Sticker { id: id.into(), name: name.into(), format, tags: String::new() };
    let choco = vec![
        sticker("781291131828699156", "Wave", StickerFormat::Apng),
        sticker("781291442961383434", "Sing", StickerFormat::Apng),
        sticker("781291606493495306", "On My Way", StickerFormat::Apng),
        sticker("781321379546398740", "Eating", StickerFormat::Apng),
        sticker("781321874650562560", "Roll", StickerFormat::Apng),
    ];
    let hammy = vec![sticker("809207198856904764", "Cheer", StickerFormat::Apng), sticker("809207919115239525", "Laugh", StickerFormat::Apng), sticker("809208197235343410", "Sleep", StickerFormat::Apng)];
    if let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") {
        guild.stickers = vec![sticker("818597707132043285", "Love", StickerFormat::Apng), sticker("818597355619483688", "OMG", StickerFormat::Apng), sticker("818597454483161098", "Money", StickerFormat::Apng)];
    }
    waffle.sticker_packs = Some(Ok(vec![StickerPack { name: "Cheerful Choco".into(), stickers: choco.clone() }, StickerPack { name: "Hammy Ham".into(), stickers: hammy }]));
    let mut wave = Message { id: "190".into(), channel_id: "general".into(), guild_id: Some("1".into()), author: user("8388608", "Wren"), timestamp: "2026-11-14T14:10:00Z".into(), ..Default::default() };
    wave.stickers = vec![choco[0].clone()];
    let lottie = Message { id: "191".into(), author: user("12582912", "moss lantern"), stickers: vec![sticker("749044136589393960", "Smile", StickerFormat::Lottie)], timestamp: "2026-11-14T14:11:00Z".into(), ..wave.clone() };
    if let Some(feed) = waffle.feeds.get_mut("general") {
        std::sync::Arc::make_mut(&mut feed.messages).extend([wave, lottie]);
    }
    waffle.list.reset(waffle.feeds.get("general").map_or(0, |f| f.messages.len()));
}

// Puts `reactions` on the newest message in #general, where the feed opens; returns its id.
fn set_reactions(waffle: &mut Waffle, reactions: Vec<Reaction>) -> String {
    let id = newest(waffle);
    if let Some(feed) = waffle.feeds.get_mut("general") {
        let mut messages = feed.messages.as_ref().clone();
        if let Some(last) = messages.last_mut() {
            last.reactions = reactions;
        }
        feed.messages = std::sync::Arc::new(messages);
    }
    id
}

fn newest(waffle: &Waffle) -> String {
    waffle.feeds.get("general").and_then(|f| f.messages.last()).map(|m| m.id.clone()).unwrap_or_default()
}

// People who reacted, as the reactions route returns them: account names, which members the server knows
// show as their nicknames, then strangers with long and short names.
fn reactors(count: usize) -> Vec<Reactor> {
    let people = [
        ("12582912", "mosslantern", "mosslantern"),
        ("8388608", "wren.wav", "wren.wav"),
        ("16777216", "toma", "toma.reed"),
        ("25165824", "longname", "a.member.whose.display.name.runs.long"),
        ("31000001", "Dhalon", "dhalon"),
        ("31000002", "saik", "saik03"),
        ("31000003", "Digitally_Devious", "digitally_devious"),
        ("31000004", "Laryke", "laryke"),
        ("31000005", "h y p r", ".thehypr"),
        ("31000006", "pouyas", "pouyas"),
        ("31000007", "Bartholomew Featherstonehaugh-Wright", "bart.fw"),
        ("31000008", "J", "yqsod"),
        ("31000009", "valse", "valsesn"),
        ("31000010", "mikey", "mickeeeyyyyyy"),
    ];
    people.iter().take(count).map(|&(id, name, username)| Reactor { user: user(id, name), username: username.into() }).collect()
}
