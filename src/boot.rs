//! Building the app state and the splash that waits on first data.

use crate::*;

// How far past the viewport the feed and the pins list lay out rows, so scrolling doesn't pop them in.
const FEED_OVERDRAW: f32 = 600.;
const PINS_OVERDRAW: f32 = 500.;

impl Waffle {
    pub(crate) fn new(window: &mut Window, splash: Option<gpui_kit::WindowHandle<splash::Splash>>, cx: &mut Context<Self>) -> Self {
        // lint-allow(detached-task): the appearance observer ends with the window
        cx.observe_window_appearance(window, |_, window, cx| palette::sync(window, cx)).detach();
        cx.observe_window_activation(window, |this: &mut Self, window, cx| {
            this.window_active = window.is_window_active();
            // Coming back to the newest message of the open channel reads it.
            this.ack_open(cx);
            // lint-allow(global-notify): window focus restyles carets, focus rings and selections in every column
            cx.notify();
        })
        // lint-allow(detached-task): the activation observer ends with the window
        .detach();
        #[allow(unused_mut)]
        let mut waffle = Self::build(window, splash, cx);
        layout::apply_zoom(window, waffle.prefs.zoom, cx);
        let main = restart::Main(cx.entity().downgrade());
        cx.set_global(main);
        waffle.watch_for_updates(cx);
        cx.on_focus_out(&waffle.feed_focus, window, |this, _, _, cx| {
            this.focused_message = None;
            this.redraw_main(cx);
        })
        // lint-allow(detached-task): the focus observer ends with the feed's focus handle
        .detach();
        #[cfg(debug_assertions)]
        if let Ok(scene) = std::env::var("WAFFLE_DEMO") {
            demo::load(&mut waffle, &scene, window, cx);
            return waffle;
        }
        waffle.start_hotkeys(cx);
        waffle.start_store(window, cx);
        match accounts::startup_token(&waffle.prefs) {
            Some(token) => waffle.connect(token, cx),
            None => waffle.sign_in(cx),
        }
        if waffle.splash.is_some() {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(SPLASH_LIMIT).await;
                this.update(cx, |this, cx| this.finish_splash(cx)).ok();
            })
            // lint-allow(detached-task): one SPLASH_LIMIT timer, then it ends
            .detach();
        }
        waffle
    }
    // Everything one signed-in session owns; switching accounts builds it afresh.
    pub(crate) fn build(window: &mut Window, splash: Option<gpui_kit::WindowHandle<splash::Splash>>, cx: &mut Context<Self>) -> Self {
        let composer = cx.new(|cx| {
            let mut state = ComposerState::new(window, cx).placeholder("Message").submit_on_enter(true);
            state.set_auto_grow(1, 10, cx);
            state
        });
        // lint-allow(detached-task): the subscription ends with the composer it watches
        cx.subscribe_in(&composer, window, Self::on_composer).detach();
        // lint-allow(detached-task): the observer ends with the composer it watches
        cx.observe(&composer, |this, _, cx| this.on_composer_selection(cx)).detach();
        let search_input = cx.new(|cx| SearchState::new(window, cx).placeholder("Search"));
        // lint-allow(detached-task): the subscription ends with the search input it watches
        cx.subscribe_in(&search_input, window, Self::on_search_input).detach();
        // Typing filters the picker and switcher; the views read the inputs' text on render.
        let emoji_search = cx.new(|cx| SearchState::new(window, cx).placeholder("Find the perfect emoji"));
        // lint-allow(detached-task): the subscription ends with the emoji search it watches
        cx.subscribe_in(&emoji_search, window, Self::on_emoji_search).detach();
        let soundboard = soundboard::Soundboard::new(window, cx);
        let feed_focus = cx.focus_handle();
        // Esc closing a focused popup (search, ⌘K) leaves nothing focused, and app keys only reach the root
        // through the focus path, so take focus back.
        // lint-allow(detached-task): the focus observer ends with the window
        cx.on_focus_lost(window, |this, window, cx| this.focus.focus(window, cx)).detach();
        let switcher_input = cx.new(|cx| SearchState::new(window, cx).placeholder("Where would you like to go?"));
        let gif_search = cx.new(|cx| SearchState::new(window, cx).placeholder("Search Tenor"));
        // lint-allow(detached-task): the subscription ends with the GIF search it watches
        cx.subscribe_in(&gif_search, window, Self::on_gif_search).detach();
        let sticker_search = cx.new(|cx| SearchState::new(window, cx).placeholder("Search stickers"));
        // lint-allow(detached-task): the subscription ends with the sticker search it watches
        cx.subscribe_in(&sticker_search, window, Self::on_sticker_search).detach();
        // lint-allow(detached-task): the subscription ends with the switcher input it watches
        cx.subscribe_in(&switcher_input, window, Self::on_switcher_input).detach();
        let settings_search = cx.new(|cx| SearchState::new(window, cx).placeholder("Search settings"));
        let dialog_input = cx.new(|cx| SearchState::new(window, cx));
        // Enter in the field submits its dialog; the input keeps the key from reaching the dialog itself.
        cx.subscribe_in(&dialog_input, window, |this: &mut Self, _, event: &InputEvent, window, cx| {
            if this.stage_topic.is_some() {
                return this.stage_topic_input(event, window, cx);
            }
            // Modify Attachment locks its spoiler box while the name starts with SPOILER_.
            if matches!(event, InputEvent::Change) && this.modify_attachment.is_some() {
                // lint-allow(global-notify): the Modify Attachment dialog is drawn by the root's overlays, outside the columns
                cx.notify();
            }
            if !matches!(event, InputEvent::PressEnter { .. }) {
                return;
            }
            if this.create_channel.is_some() {
                return this.advance_create_channel(window, cx);
            }
            if this.modify_attachment.is_some() {
                return this.save_modify_attachment(cx);
            }
            if this.dialog.as_ref().is_some_and(|d| d.input) {
                this.confirm_dialog(window, cx);
            }
        })
        // lint-allow(detached-task): the subscription ends with the dialog input it watches
        .detach();
        let alt_input = cx.new(|cx| TextareaState::new(window, cx).placeholder("Add a description").auto_grow(3, 8));
        // lint-allow(detached-task): the subscription ends with the description input it watches
        cx.subscribe_in(&alt_input, window, Self::on_alt_input).detach();
        let status_input = cx.new(|cx| SearchState::new(window, cx).placeholder("What's on your mind?"));
        // lint-allow(detached-task): the subscription ends with the status input it watches
        cx.subscribe_in(&status_input, window, Self::on_status_input).detach();
        let add_friend_input = cx.new(|cx| SearchState::new(window, cx).placeholder("username"));
        // lint-allow(detached-task): the subscription ends with the add-friend input it watches
        cx.subscribe_in(&add_friend_input, window, Self::on_add_friend_input).detach();
        let add_friend_note = cx.new(|cx| TextareaState::new(window, cx).placeholder("Say hi, or how you know each other"));
        // lint-allow(global-notify, detached-task): the add-friend dialog is drawn by the root's overlays, outside the columns; the observer ends with the note it watches
        cx.observe(&add_friend_note, |_, _, cx| cx.notify()).detach();
        cx.subscribe(&settings_search, |this: &mut Self, _, _: &InputEvent, cx| {
            // A new query starts at its best result.
            if let Some(view) = this.settings.as_mut() {
                view.reset_result();
            }
            // lint-allow(global-notify): the settings view covers the whole window, so a new query redraws every column
            cx.notify();
        })
        // lint-allow(detached-task): the subscription ends with the settings search it watches
        .detach();
        let mut waffle = Self {
            status: Status::Connecting,
            launch: launch::Launch::new(),
            client: None,
            token: None,
            _session: None,
            me: None,
            my_status: discord::Status::Online,
            custom_status: None,
            status_until: None,
            status_editor: None,
            poll_creator: None,
            poll_picks: HashMap::new(),
            poll_peek: HashSet::new(),
            open_join_runs: HashSet::new(),
            forward_dialog: None,
            interactions: HashMap::new(),
            command_picker: command_picker::CommandPicker::load(),
            component_state: Default::default(),
            app_modal: None,
            announcements: Default::default(),
            events: Default::default(),
            report_dialog: None,
            group_dialog: None,
            status_flyout: status_menu::Flyout::default(),
            status_input,
            clear_after: status_menu::ClearAfter::default(),
            guilds: Vec::new(),
            folders: Vec::new(),
            open_folders: HashSet::new(),
            rail_left_folder: false,
            rail_board: Default::default(),
            channel_board: Default::default(),
            role_board: Default::default(),
            favorite_board: Default::default(),
            drag_motion: Default::default(),
            members: HashMap::new(),
            dms: Vec::new(),
            dm_requests: Vec::new(),
            guild_id: None,
            channel: None,
            last_channel: HashMap::new(),
            feeds: HashMap::new(),
            list: ListState::new(0, ListAlignment::Bottom, px(FEED_OVERDRAW)),
            scroll_anchors: HashMap::new(),
            read_states: HashMap::new(),
            last_message: HashMap::new(),
            presences: HashMap::new(),
            member_roles: HashMap::new(),
            member_lists: HashMap::new(),
            member_groups: HashMap::new(),
            requested_members: HashSet::new(),
            typing: HashMap::new(),
            last_typing_sent: None,
            right_panel: RightPanel::Members,
            composer,
            search_input,
            search_pick: None,
            search_more: false,
            search_day: search_menu::today(),
            has_draft: false,
            drafts: drafts::Drafts::load(),
            drafts_save: None,
            send_error: None,
            replying: None,
            reply_ping: true,
            pending_files: Vec::new(),
            file_previews: HashMap::new(),
            uploads: HashMap::new(),
            upload_tasks: HashMap::new(),
            talk_key_down: false,
            prefs: settings::Prefs::load(),
            settings: None,
            channel_settings: None,
            server_settings: None,
            settings_search,
            hotkeys: None,
            keybind_errors: Vec::new(),
            mic_level: SILENCE_DB,
            mic_test: None,
            slider_bounds: Rc::default(),
            call_view: false,
            voice_popover: None,
            muted_cue: Default::default(),
            device_menu: None,
            menu: None,
            stream_previews: HashMap::new(),
            forums: HashMap::new(),
            developer_mode: false,
            account_settings: Default::default(),
            my_account: Default::default(),
            guild_settings: HashMap::new(),
            ignored: HashSet::new(),
            shown_folds: HashSet::new(),
            fold_layout: Default::default(),
            ring_on_connect: None,
            ringing_out: None,
            dm_calls: HashMap::new(),
            dm_voice: Vec::new(),
            ringing_in: None,
            dm_call_ended: None,
            camera_on_connect: false,
            hidden_voice_names: HashSet::new(),
            shift_held: false,
            stream_card_hover: None,
            stream_card_bounds: Rc::default(),
            stream_tunnel: ui::Tunnel::default(),
            stream_hold: None,
            activity_feed: Vec::new(),
            watch_on_connect: None,
            window_active: true,
            unread_scroll_pending: false,
            held_unread: None,
            dialog: None,
            keyboard_combos: false,
            create_channel: None,
            create_server: None,
            invite: None,
            notification_settings: None,
            stage_topic: None,
            modify_attachment: None,
            dialog_input,
            alt_input,
            toast: None,
            toast_action: None,
            add_friend_input,
            add_friend_note,
            add_friend_result: None,
            active_now_ticking: Rc::default(),
            auth_retried: false,
            share_picker: None,
            share_choice: None,
            share_choice_label: None,
            stream_viewers: HashMap::new(),
            stream_paused: HashSet::new(),
            stream_live_since: HashMap::new(),
            call_pointer_at: Instant::now(),
            call_pointer_inside: false,
            call_pointer_on_chrome: false,
            stage_ui: Default::default(),
            preview_generation: 0,
            splash,
            main_window: window.window_handle(),
            joining: None,
            stream_volumes: HashMap::new(),
            call_fullscreen: false,
            call_chat: false,
            call_previews: Default::default(),
            call_unread_at: None,
            voice_start: HashMap::new(),
            voice_status: HashMap::new(),
            voice_status_dialog: None,
            channel_info_requested: HashSet::new(),
            avatar_colors: HashMap::new(),
            pins: None,
            revealed: Rc::default(),
            text_files: Rc::default(),
            relationships: Vec::new(),
            friends_tab: home::FriendsTab::Online,
            inbox: None,
            saved: None,
            saved_ids: Default::default(),
            frecency_save_pending: false,
            inbox_open: false,
            inbox_tab: inbox::InboxTab::default(),
            pending_jump: None,
            popover_at: gpui_kit::Point::default(),
            inbox_page: false,
            requests_page: None,
            join: joining::Join::default(),
            guild_page: None,
            gif_picker: None,
            picker_tab: stickers::PickerTab::default(),
            sticker_packs: None,
            sticker_search,
            gif_search,
            restore: None,
            editing: None,
            mention_wire: HashMap::new(),
            autocomplete: None,
            hold_up: None,
            autocomplete_scroll: gpui_kit::ScrollHandle::new(),
            channels_scroll: gpui_kit::ScrollHandle::new(),
            rail_scroll: gpui_kit::ScrollHandle::new(),
            panel_scroll: gpui_kit::ScrollHandle::new(),
            flash: None,
            muted: false,
            deafened: false,
            call: None,
            activities: Default::default(),
            voice_error: None,
            voice_notice: None,
            last_call: None,
            gateway_session: String::new(),
            call_switched: false,
            viewer: None,
            profile_card: None,
            role_popout: Default::default(),
            status_menu: false,
            updates: updater::Updates::default(),
            hovered_row: None,
            hovered_message: None,
            hovered_tile: None,
            alt_popout: None,
            focused_message: None,
            feed_focus,
            call_focus: None,
            profiles: query::Query::new("profiles", Duration::from_secs(5 * 60)),
            profile_effects: HashMap::new(),
            link_previews: link_preview::previews(),
            reactors: HashMap::new(),
            reactions_dialog: None,
            games: Default::default(),
            composer_height: Rc::new(std::cell::Cell::new(86.)),
            guard: Default::default(),
            pins_button_x: Rc::default(),
            pins_list: ListState::new(0, ListAlignment::Top, px(PINS_OVERDRAW)),
            pip: Default::default(),
            emoji_picker: None,
            emoji_search,
            soundboard,
            switcher: None,
            switcher_input,
            unread_marker: None,
            scrolled_up: false,
            copied: None,
            focus: cx.focus_handle(),
            channels_width: layout::CHANNELS_WIDTH,
            settings_width: ui::SETTINGS_NAV_WIDTH,
            settings_nav_scroll: gpui_kit::ScrollHandle::new(),
            panel_width: layout::PANEL_WIDTH,
            drag: None,
            history: home::History::default(),
            folded: HashSet::new(),
            favorites: Default::default(),
            favorites_open: None,
            hovered: None,
            member_hover: None,
            motions: Motions::new(),
            rail_dms_shown: None,
            arrivals: Rc::default(),
            parts: part::Parts::default(),
        };
        let weak = cx.weak_entity();
        waffle.list.set_scroll_handler(move |event, _, cx| {
            let away = chat::away_from_present(event.visible_range.end, event.count);
            weak.update(cx, |this, cx| {
                if this.scrolled_up != away {
                    this.scrolled_up = away;
                    this.redraw_main(cx);
                }
                // Reaching the newest message is what reads the channel, as in Discord.
                if !away {
                    this.ack_open(cx);
                }
            })
            .ok();
        });
        waffle.apply_prefs();
        waffle
    }
    pub(crate) fn splash_stage(&mut self, step: usize, stage: &'static str, cx: &mut Context<Self>) {
        if let Some(splash) = self.splash {
            splash.update(cx, |splash, _, cx| splash.set(step, stage, cx)).ok();
        }
    }
    // Servers are in: say so, then open Waffle a beat later so the step registers.
    pub(crate) fn splash_ready(&mut self, cx: &mut Context<Self>) {
        if self.splash.is_none() {
            return;
        }
        self.splash_stage(1, "Loading your servers…", cx);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SPLASH_BEAT).await;
            this.update(cx, |this, cx| this.splash_stage(2, "Opening Waffle…", cx)).ok();
            cx.background_executor().timer(SPLASH_BEAT).await;
            this.update(cx, |this, cx| this.finish_splash(cx)).ok();
        })
        // lint-allow(detached-task): two SPLASH_BEAT timers, then it ends
        .detach();
    }
    // Closes the launch window and brings the main window forward, after this update finishes
    // (a window can't be shown from inside its own update).
    pub(crate) fn finish_splash(&mut self, cx: &mut Context<Self>) {
        let Some(splash) = self.splash.take() else { return };
        let main = self.main_window;
        cx.defer(move |cx| {
            splash.update(cx, |_, window, _| window.remove_window()).ok();
            main.update(cx, |_, window, _| window.activate_window()).ok();
        });
    }
}
