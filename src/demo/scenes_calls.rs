use super::*;

pub(super) fn call_scenes(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    match scene {
        "active-now" | "active-now-empty" => demo_active_now(waffle, scene == "active-now-empty", cx),
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
        // "presence-morph" walks juniper through every status, so each mark morphs into the next.
        "home-presence" | "presence-morph" => {
            let people = [("12582912", "moss lantern", Presence_::Idle, false), ("16777216", "toma reed", Presence_::Dnd, false), ("25165824", "ferris", Presence_::Offline, false), ("33554432", "pip", Presence_::Online, true), ("41943040", "juniper", Presence_::Online, false)];
            for (id, name, status, streaming) in people {
                let mut dm = waffle.dms[0].clone();
                dm.id = format!("dm-{id}");
                dm.name = name.into();
                dm.recipient = Some(user(id, name));
                waffle.dms.push(dm);
                waffle.presences.insert(id.into(), Presence { status, streaming, ..Default::default() });
            }
            // Every presence mark the DM list draws, typing included.
            let until = std::time::Instant::now() + std::time::Duration::from_secs(3600);
            waffle.typing.insert("dm-41943040".into(), vec![crate::Typer { user_id: "41943040".into(), name: "juniper".into(), until }]);
            if scene == "presence-morph" {
                demo_presence_morph(window, cx);
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
                        cx.notify(); // lint-allow(hot-notify): one step per 1.5 s, and each step changes a rail unread badge
                    });
                    if alive.is_err() {
                        return;
                    }
                }
            })
            .detach(); // lint-allow(detached-task): the loop returns once the view is gone and the update fails
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
            .detach(); // lint-allow(detached-task): the loop returns once the view is gone and the update fails
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
                list.set_offset(demo_at(0., -40.));
                window.refresh();
            });
        }
        // In hangout: both voice channels with a status (a custom emoji in study-hall's), the call header too.
        // `-set`: in hangout without one, so its row offers "Set a channel status".
        "voice-status" | "voice-status-set" => {
            demo_call(waffle);
            if scene == "voice-status" {
                let statuses = vec![("lounge".into(), "🎮 ranked grind, come through".into()), ("hub".into(), "finals week <:ferris:1> quiet please, cameras optional, snacks encouraged".into())];
                waffle.on_event(Event::VoiceStatuses { guild_id: "1".into(), statuses, all: true }, cx);
            }
        }
        // The status dialog empty, and `-long` with 480 characters typed: the counter showing 20 left.
        "voice-status-dialog" | "voice-status-dialog-long" => {
            demo_call(waffle);
            let Some(lounge) = waffle.guild().and_then(|g| g.channels.iter().find(|c| c.id == "lounge").cloned()) else { return };
            waffle.open_voice_status(&lounge, window, cx);
            if let Some(dialog) = waffle.voice_status_dialog.as_ref().filter(|_| scene == "voice-status-dialog-long") {
                let long = "lofi and bug fixing, drop in whenever ".repeat(13);
                dialog.input.update(cx, |state, cx| state.set_value(&long[..480], window, cx));
            }
        }
        "dm-empty" | "group-empty" | "group-start" | "group-mention" => demo_dm_start(waffle, scene, window, cx),
        "guard-slowmode" | "guard-slowmode-counting" | "guard-slowmode-queued" | "guard-immune" | "guard-no-permission" | "guard-timeout" | "guard-blocked-dm" => demo_guard(waffle, scene, window, cx),
        "requests" | "requests-spam" | "requests-empty" | "request-open" => demo_requests(waffle, scene, window, cx),
        "group-dm" | "group-picker" | "group-edit" | "group-leave" => demo_group(waffle, scene, window, cx),
        "stream-card" => {
            let key = "guild:1:lounge:12582912".to_owned();
            let blender = Activity { kind: ActivityKind::Playing, text: "Blender".into(), rest: Vec::new() };
            waffle.presences.insert("12582912".into(), Presence { status: Presence_::Online, custom: None, activity: Some(blender), ..Default::default() });
            let since = std::time::Instant::now().checked_sub(std::time::Duration::from_secs(24 * 60)).unwrap_or_else(std::time::Instant::now);
            waffle.stream_live_since.insert(key.clone(), since);
            waffle.on_event(Event::StreamUpdate { key, viewers: vec!["8388608".into(), "16777216".into(), "20971520".into(), "4194304".into()], paused: false }, cx);
            waffle.stream_card_hover = Some("stream-12582912".into());
        }
        "mod-view" => demo_mod_view(waffle, cx),
        "mod-view-loading" => demo_mod_view_loading(waffle, cx),
        "user-menu" => {
            demo_call(waffle);
            waffle.call_view = false;
            waffle.voice_user_menu("8388608", demo_at(300., 160.), cx);
        }
        "server-menu" => waffle.open_server_menu("1".into(), demo_at(60., 130.), false, cx),
        // Move server is entry 14 (the header and separators count); Folder is its first child.
        "server-menu-nested" => {
            waffle.open_server_menu("1".into(), demo_at(60., 130.), false, cx);
            waffle.open_submenus(vec![14, 0]);
        }
        s if ["invite", "onboarding", "rules", "roles", "guide"].iter().any(|p| s.starts_with(p)) => crate::joining::demo(waffle, scene, window, cx),
        "server-settings" | "server-settings-engagement" | "server-settings-roles" | "server-settings-role" | "server-settings-role-perms" | "server-settings-audit" | "server-settings-bans" | "server-settings-automod" | "server-settings-invites" | "server-settings-dirty" => crate::server_settings::demo::open(waffle, scene, window, cx),
        "server-settings-members" | "server-settings-members-bulk" | "server-settings-members-filter" => demo_server_settings_members(waffle, scene, window, cx),
        "channel-settings" | "channel-settings-search" | "channel-settings-perms" | "channel-settings-voice" | "channel-settings-voice-perms" | "channel-settings-invites" | "channel-settings-dirty" => demo_channel_settings(waffle, scene, window, cx),
        "nav-pins" => demo_pins(waffle, window, cx),
        "forum" | "forum-gallery" | "forum-search" | "forum-sort" | "forum-new-post" => crate::forum::demo::open(waffle, scene, window, cx),
        "channel-menu" => {
            let general = waffle.guild().and_then(|g| g.channels.iter().find(|c| c.id == "general").cloned());
            if let Some(channel) = general {
                waffle.open_channel_menu(channel, "1".into(), demo_at(200., 170.), cx);
            }
        }
        "message-menu" => {
            let message = waffle.feeds.get("general").and_then(|f| f.messages.iter().find(|m| m.author.id == "8388608").cloned());
            if let Some(message) = message {
                waffle.open_message_menu(message, demo_at(700., 260.), cx);
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
        "call-streams" | "call-streams-ended" | "call-streams-focus" | "call-streams-strip" | "call-streams-full" | "call-streams-failed" | "call-streams-reconnecting" | "call-streams-issues" | "call-streams-hidden" | "call-streams-paused" => {
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
                call.watching.push(crate::Watching::new("12582912".into(), Some(demo_frame(1440, 1080, 40)), None));
                call.self_view = Some((demo_frame(1280, 720, 200), std::time::Instant::now()));
                call.cameras.insert("20971520".into(), demo_frame(640, 480, 120));
                call.cameras.insert("8388608".into(), demo_frame(640, 360, 230));
                if scene == "call-streams-ended" {
                    call.ended = vec!["20971520".into(), "99".into()];
                }
                // The watched stream's states, and every issue a tile can carry: ours (4194304) and our camera
                // (shown here on Wren's tile key, as a viewer would see a 2012 on a camera that never arrived).
                use crate::voice::issues::*;
                match scene {
                    "call-streams-failed" => call.watching[0].state = crate::WatchState::Failed(crate::STREAM_FAILED_TO_START),
                    "call-streams-reconnecting" => {
                        call.watching[0].state = crate::WatchState::Reconnecting;
                        call.watching[0].frame = None;
                    }
                    "call-streams-issues" => {
                        call.watching[0].state = crate::WatchState::Active;
                        let now = std::time::Instant::now();
                        call.watching[0].arrivals = (0..60).map(|i| now - std::time::Duration::from_millis(i * 33)).rev().collect();
                        for code in [STREAM_SEND_NETWORK_QUALITY, SOUNDSHARE_FAILED] {
                            call.issues.insert(("4194304".into(), code));
                        }
                        call.issues.insert(("12582912".into(), STREAM_VIEW_HIGH_PACKET_LOSS));
                        call.issues.insert(("camera:8388608".into(), RECEIVER_READY_TIMEOUT));
                    }
                    _ => {}
                }
            }
            // Both sides of a paused Go Live: the stream we watch and our own.
            if scene == "call-streams-paused" {
                waffle.stream_paused.extend(["guild:1:lounge:12582912".to_owned(), "guild:1:lounge:4194304".to_owned()]);
            }
            if scene == "call-streams-hidden" {
                waffle.prefs.hidden_self_streams.push("lounge".into());
            }
            if !matches!(scene, "call-streams" | "call-streams-ended" | "call-streams-failed" | "call-streams-reconnecting" | "call-streams-issues" | "call-streams-hidden" | "call-streams-paused") {
                waffle.call_focus = Some("stream:12582912".into());
                // Holds the chrome up for the first half minute, as if the pointer had just moved.
                waffle.call_pointer_at = std::time::Instant::now() + std::time::Duration::from_secs(30);
            }
            waffle.stage_ui.strip_hidden = scene != "call-streams-strip";
            if scene == "call-streams-full" {
                waffle.set_call_fullscreen(true, window, cx);
            }
        }
        // Discord's camera preview before the camera turns on.
        "camera-preview" => {
            demo_call(waffle);
            waffle.preview_camera(cx);
        }
        // A game on the call's stage, for checking what draws over it and how it resizes: a WebGL scene that
        // answers every pointer move, as a game does.
        "call-activity" => {
            demo_call(waffle);
            waffle.demo_activity("https://threejs.org/examples/webgl_animation_keyframes.html", window, cx);
        }
        // Text chat in the voice channel we're in: the sidebar's rows, the stage's message previews (with a
        // mention, or unread only) and the chat open beside the call.
        "sidebar-voice-chat" | "call-previews" | "call-previews-unread" | "call-chat" => demo_voice_chat(waffle, scene, window, cx),
        "call" | "call-info" | "call-noise" | "call-devices" | "call-watch" | "call-focus" | "call-person" | "call-alone" | "call-join" | "call-muted" | "call-idle" | "pip" => {
            demo_call(waffle);
            // As if the pointer just moved on the stage, so its chrome shows for the first half minute.
            waffle.call_pointer_inside = scene != "call-idle";
            if scene != "call-idle" {
                waffle.call_pointer_at = std::time::Instant::now() + std::time::Duration::from_secs(30);
            }
            match scene {
                // Wren and Bao on camera, Wren focused over the strip.
                "call-person" => {
                    if let Some(call) = waffle.call.as_mut() {
                        call.cameras.insert("8388608".into(), demo_frame(640, 360, 230));
                        call.cameras.insert("20971520".into(), demo_frame(640, 480, 120));
                    }
                    waffle.call_focus = Some("user:8388608".into());
                }
                // Everyone else gone: the entry tile offers Invite to Voice and Choose Activity.
                "call-alone" => {
                    if let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") {
                        guild.voice_states.retain(|v| v.channel_id.as_deref() != Some("lounge") || v.user_id == "4194304");
                    }
                }
                // Left the call while looking at it: the channel's join screen.
                "call-join" => {
                    waffle.end_call(cx);
                }
                "call-muted" => waffle.muted = true,
                "call-info" => waffle.voice_popover = Some(crate::call::VoicePopover::Connection),
                "call-noise" => waffle.voice_popover = Some(crate::call::VoicePopover::Noise),
                // Watching moss lantern's Go Live: the stream over the filmstrip.
                "call-watch" | "call-focus" | "pip" => {
                    if let Some(call) = waffle.call.as_mut() {
                        // The PiP plays a 4:3 picture, to show it letterboxed inside its 16:9 frame.
                        let frame = (scene == "pip").then(|| demo_frame(1440, 1080, 40));
                        call.watching.push(crate::Watching::new("12582912".into(), frame, None));
                    }
                    // Reading #general meanwhile: the stream floats as a picture-in-picture.
                    waffle.call_view = scene != "pip";
                    // The stream clicked: it fills the stage over a strip of everyone else.
                    if scene == "call-focus" {
                        waffle.call_focus = Some("stream:12582912".into());
                    }
                }
                "call-devices" => waffle.open_device_menu(crate::call::DeviceKind::Input, demo_at(260., 760.), cx),
                _ => {}
            }
        }
        // The soundboard over the call: favorites, frequently used, Night Owls' sounds, gpui's locked without
        // Nitro, Discord's own, quack playing. "soundboard-effect": Wren's quack rising on her tile.
        "soundboard" | "soundboard-effect" => {
            demo_call(waffle);
            waffle.call_pointer_inside = true;
            crate::soundboard::demo(waffle, scene == "soundboard", window, cx);
        }
        _ if scene.starts_with("stage") => demo_stage(waffle, scene, window, cx),
        _ => {}
    }
}

// Every 1.2s juniper's mark steps to the next status (typing, online, idle, dnd, offline, mobile, streaming).
fn demo_presence_morph(window: &mut Window, cx: &mut Context<Waffle>) {
    let steps = [(None, Presence_::Online, false, false), (None, Presence_::Idle, false, false), (None, Presence_::Dnd, false, false), (None, Presence_::Offline, false, false), (None, Presence_::Online, true, false), (None, Presence_::Online, false, true), (Some(()), Presence_::Online, false, false)];
    cx.spawn_in(window, async move |this, cx| loop {
        for (typing, status, mobile_only, streaming) in steps {
            cx.background_executor().timer(std::time::Duration::from_millis(1200)).await;
            let alive = this.update(cx, |this, cx| {
                let until = std::time::Instant::now() + std::time::Duration::from_secs(if typing.is_some() { 3600 } else { 0 });
                this.typing.insert("dm-41943040".into(), vec![crate::Typer { user_id: "41943040".into(), name: "juniper".into(), until }]);
                this.presences.insert("41943040".into(), Presence { status, mobile_only, streaming, ..Default::default() });
                this.redraw(crate::part::Column::Nav, cx);
            });
            if alive.is_err() {
                return;
            }
        }
    })
    .detach(); // lint-allow(detached-task): the loop returns once the view is gone and the update fails
}
