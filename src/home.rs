// The Direct Messages home: Discord's Friends page, the Friends row atop the DM list, the Inbox and the
// GIF picker actions, and reopening the last channel on launch.
use std::time::Duration;

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    AnyElement, App, Context, Entity, Focusable, FontWeight, Hsla, SharedString, Window, WindowControlArea,
    component::{
        Icon,
        input::{Input, InputEvent, Textarea},
    },
    div, prelude::*, px,
};
use reqwest::Method;
use serde_json::json;

use crate::{
    Waffle, SearchState,
    part::Column,
    discord::{ApiError, Channel, Relation, Relationship, Status},
    icons::Glyph,
    illustration::Illustration,
    layout::avatar_with_status,
    mascot::Mood,
    palette,
    sidebar::{Nav, RowState, channel_row},
    ui::{self, Squircle, Tone},
};

pub(crate) const MASCOT: &str = "add-friend-mascot-hit";

// The Inbox page's column and the Add Friend page's column.
const INBOX_WIDTH: f32 = 760.;
const ADD_FRIEND_WIDTH: f32 = 560.;

// Discord's cap on a friend request's note.
const NOTE_MAX: usize = 120;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FriendsTab {
    Online,
    All,
    Pending,
    Blocked,
    AddFriend,
}

// A place Back and Forward return to: a channel, or with no channel, Friends (or an empty server).
#[derive(Clone)]
pub(crate) struct Stop {
    pub guild_id: Option<String>,
    pub channel: Option<Channel>,
}

// Browser-style history: opening somewhere new drops the forward entries.
pub(crate) struct History<T> {
    back: Vec<T>,
    forward: Vec<T>,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self { back: Vec::new(), forward: Vec::new() }
    }
}

impl<T> History<T> {
    pub(crate) fn visit(&mut self, from: T) {
        self.back.push(from);
        self.forward.clear();
    }

    // The stop one step back or forward; `current` goes on the other stack to return to.
    pub(crate) fn step(&mut self, forward: bool, current: T) -> Option<T> {
        let (from, to) = if forward { (&mut self.forward, &mut self.back) } else { (&mut self.back, &mut self.forward) };
        let next = from.pop()?;
        to.push(current);
        Some(next)
    }
}

impl History<Stop> {
    // The server most recently left behind, for the shortcut that toggles back to it.
    pub(crate) fn last_guild(&self) -> Option<String> {
        self.back.iter().rev().find_map(|stop| stop.guild_id.clone())
    }
}

// One file per account ("last-channel-<id>"); the shared file from before accounts is read as a fallback.
fn last_channel_file(user_id: Option<&str>) -> Option<std::path::PathBuf> {
    let name = match user_id {
        Some(id) if !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) => format!("last-channel-{id}"),
        Some(_) => return None,
        None => "last-channel".into(),
    };
    Some(crate::data_dir()?.join(name))
}

/// "guild_id channel_id" ("@me" for DMs), so a restart lands where you left off, as Discord does.
pub fn load_last_channel(user_id: &str) -> Option<(Option<String>, String)> {
    let text = [Some(user_id), None].into_iter().find_map(|id| std::fs::read_to_string(last_channel_file(id)?).ok())?;
    let (guild, channel) = text.trim().split_once(' ')?;
    Some(((guild != "@me").then(|| guild.to_owned()), channel.to_owned()))
}

// Written on the IO thread, whole or not at all.
pub fn save_last_channel(user_id: &str, guild_id: Option<&str>, channel_id: &str) {
    let Some(path) = last_channel_file(Some(user_id)) else { return };
    let text = format!("{} {channel_id}", guild_id.unwrap_or("@me"));
    crate::store::io(move || drop(crate::store::write_atomic(&path, text.as_bytes())));
}

// Logging out forgets where the account was.
pub fn forget_last_channel(user_id: &str) {
    let Some(path) = last_channel_file(Some(user_id)) else { return };
    crate::store::io(move || drop(std::fs::remove_file(path)));
}

impl Waffle {
    /// Called on render: once the gateway has delivered servers and DMs, reopen the saved channel.
    pub(crate) fn restore_last_channel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.guilds.is_empty() && self.dms.is_empty() {
            return;
        }
        let Some((guild_id, channel_id)) = self.restore.take() else { return };
        let channel = match &guild_id {
            Some(id) => self.guilds.iter().find(|g| &g.id == id).and_then(|g| g.channels.iter().find(|c| c.id == channel_id)).cloned(),
            None => self.dms.iter().find(|d| d.id == channel_id).cloned(),
        };
        if let Some(channel) = channel {
            self.guild_id = guild_id;
            self.open_channel(channel, window, cx);
            // Launch lands here; there's nothing to go back to.
            self.history = History::default();
        }
    }

    /// Friends, which is a stop in Back/Forward like Discord's /channels/@me route.
    pub(crate) fn open_home(&mut self, cx: &mut Context<Self>) {
        if self.guild_id.is_some() || self.channel.is_some() {
            self.history.visit(self.here());
        }
        self.show_no_channel(None, cx);
    }

    // Friends (no server), or a server with no channel open.
    pub(crate) fn show_no_channel(&mut self, guild_id: Option<String>, cx: &mut Context<Self>) {
        self.inbox_page = false;
        self.requests_page = None;
        self.guild_id = guild_id;
        self.channel = None;
        self.call_view = false;
        cx.notify(); // lint-allow(global-notify): opening Friends or a server home changes the channel list (nav), the content (main) and the member list (panel)
    }

    pub(crate) fn here(&self) -> Stop {
        Stop { guild_id: self.guild_id.clone(), channel: self.channel.clone() }
    }

    // A DM opened or changed: it lands in the DM list or in Message Requests by its request state, so an
    // accepted request moves over. An update without recipients keeps the ones we know.
    pub(crate) fn upsert_dm(&mut self, mut dm: Channel, cx: &mut Context<Self>) {
        let known = self.dms.iter().chain(&self.dm_requests).find(|d| d.id == dm.id);
        if dm.recipients.is_empty()
            && let Some(known) = known
        {
            dm.recipients = known.recipients.clone();
            dm.recipient = known.recipient.clone();
        }
        let at = self.dms.iter().position(|d| d.id == dm.id);
        self.dms.retain(|d| d.id != dm.id);
        self.dm_requests.retain(|d| d.id != dm.id);
        if let Some(open) = self.channel.as_mut().filter(|c| c.id == dm.id) {
            *open = dm.clone();
        }
        match (dm.is_request(), at) {
            (true, _) => self.dm_requests.insert(0, dm),
            (false, Some(at)) => self.dms.insert(at, dm),
            (false, None) => self.dms.insert(0, dm),
        }
        cx.notify(); // lint-allow(global-notify): a DM moving between lists changes the DM list (nav) and the open DM's header (main)
    }

    pub(crate) fn open_inbox(&mut self, cx: &mut Context<Self>) {
        self.inbox_open = true;
        self.fetch_mentions(cx);
    }

    /// The Inbox page: every recent mention, in the content column.
    pub(crate) fn open_inbox_page(&mut self, cx: &mut Context<Self>) {
        self.inbox_open = false;
        self.inbox_page = true;
        self.requests_page = None;
        if self.inbox.is_none() {
            self.fetch_mentions(cx);
        }
        cx.notify(); // lint-allow(global-notify): the inbox page replaces the content (main) and lights the inbox button (nav)
    }

    pub(crate) fn fetch_saved(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        self.saved = Some(None);
        cx.notify(); // lint-allow(global-notify): the saved list shows in the inbox popover, an overlay that only follows the app
        cx.spawn(async move |this, cx| {
            let result = client.saved_messages().await;
            this.update(cx, |this, cx| {
                if let Ok(list) = &result {
                    std::rc::Rc::make_mut(&mut this.saved_ids).extend(list.iter().map(|m| m.id.clone()));
                }
                this.saved = Some(Some(result.map_err(Into::into)));
                cx.notify(); // lint-allow(global-notify): the saved list shows in the inbox popover, an overlay that only follows the app
            })
            .ok();
        })
        .detach(); // lint-allow(detached-task): a one-shot fetch that ends when the request returns, holding a WeakEntity
    }

    // Your reaction frecency from Discord, merged into this device's.
    pub(crate) fn load_reaction_frecency(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            let Ok(settings) = client.settings_proto(crate::frecency_sync::SETTINGS_TYPE).await else { return };
            let Some(uses) = crate::frecency_sync::decode(&settings) else { return };
            this.update(cx, |this, cx| {
                let remote: Vec<_> = uses
                    .into_iter()
                    .filter_map(|u| {
                        let emoji = crate::frecency_sync::emoji(&u.key, |id| this.guilds.iter().flat_map(|g| g.emojis.iter()).find(|e| e.id.as_deref() == Some(id)).cloned())?;
                        Some((emoji, u.total, u.recent.iter().map(|ms| (*ms / 1000) as i64).collect::<Vec<_>>()))
                    })
                    .collect();
                this.update_prefs(cx, |p| p.merge_emoji_uses(remote));
            })
            .ok();
        })
        .detach(); // lint-allow(detached-task): a one-shot fetch that ends when the request returns, holding a WeakEntity
    }

    // Sends this device's reaction frecency to Discord a moment after the last reaction.
    pub(crate) fn save_reaction_frecency(&mut self, cx: &mut Context<Self>) {
        const DELAY: std::time::Duration = std::time::Duration::from_secs(5);
        if std::mem::replace(&mut self.frecency_save_pending, true) {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DELAY).await;
            let Ok(Some((client, settings))) = this.update(cx, |this, _| {
                this.frecency_save_pending = false;
                let uses: Vec<_> = this
                    .prefs
                    .emoji_uses
                    .iter()
                    .filter_map(|(emoji, total)| {
                        let times = this.prefs.emoji_times.iter().find(|(e, _)| e == emoji).map(|(_, t)| t.iter().map(|s| *s as u64 * 1000).collect()).unwrap_or_default();
                        Some(crate::frecency_sync::Use { key: crate::frecency_sync::key(emoji)?, total: *total, recent: times })
                    })
                    .collect();
                Some((this.client.clone()?, crate::frecency_sync::encode(&uses)))
            }) else {
                return;
            };
            if let Err(e) = client.patch_settings_proto(crate::frecency_sync::SETTINGS_TYPE, settings).await {
                eprintln!("frecency: save failed: {e}");
            }
        })
        .detach(); // lint-allow(detached-task): a one-shot sync that ends when the request returns, holding a WeakEntity
    }

    pub(crate) fn fetch_saved_ids(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            let Ok(ids) = client.saved_message_ids().await else { return };
            this.update(cx, |this, cx| {
                this.saved_ids = std::rc::Rc::new(ids.into_iter().collect());
                cx.notify(); // lint-allow(global-notify): saved marks show in message menus (overlays, which only follow the app) and message rows
            })
            .ok();
        })
        .detach(); // lint-allow(detached-task): a one-shot fetch that ends when the request returns, holding a WeakEntity
    }

    pub(crate) fn fetch_mentions(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        self.inbox = Some(None);
        cx.notify(); // lint-allow(global-notify): the mentions list shows in the inbox popover, an overlay that only follows the app
        let filters = self.prefs.mention_filters;
        let guild = self.guild_id.clone().filter(|_| filters.this_server);
        cx.spawn(async move |this, cx| {
            let result = client.mentions(filters.everyone, filters.roles, guild.as_deref()).await;
            this.update(cx, |this, cx| {
                this.inbox = Some(Some(result.map_err(Into::into)));
                cx.notify(); // lint-allow(global-notify): the mentions list shows in the inbox popover, an overlay that only follows the app
            })
            .ok();
        })
        .detach(); // lint-allow(detached-task): a one-shot fetch that ends when the request returns, holding a WeakEntity
    }

    pub(crate) fn open_gifs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.gif_picker = Some(None);
        self.picker_tab = crate::stickers::PickerTab::Gifs;
        self.gif_search.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.focus(window, cx);
        });
        self.search_gifs(String::new(), cx);
    }

    pub(crate) fn on_gif_search(&mut self, state: &Entity<SearchState>, event: &InputEvent, _: &mut Window, cx: &mut Context<Self>) {
        let InputEvent::Change = event else { return };
        let query = state.read(cx).value().to_string();
        // Waits for a pause in typing, then searches if the text is still the same.
        let input = state.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(300)).await;
            this.update(cx, |this, cx| {
                if input.read(cx).value() == query.as_str() {
                    this.search_gifs(query, cx);
                }
            })
            .ok();
        })
        .detach(); // lint-allow(detached-task): a one-shot debounce that ends after its timer fires, holding a WeakEntity
    }

    fn search_gifs(&mut self, query: String, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            let result = client.gifs(query.trim()).await;
            this.update(cx, |this, cx| {
                if this.gif_picker.is_some() {
                    this.gif_picker = Some(Some(result.map_err(Into::into)));
                }
                cx.notify(); // lint-allow(global-notify): the GIF picker is a popover, an overlay that only follows the app
            })
            .ok();
        })
        .detach(); // lint-allow(detached-task): a one-shot search that ends when the request returns, holding a WeakEntity
    }

    /// Sends the GIF the way Discord does: its Tenor link, which Discord embeds.
    pub(crate) fn send_gif(&mut self, url: String, window: &mut Window, cx: &mut Context<Self>) {
        self.gif_picker = None;
        self.composer.update(cx, |state, cx| state.set_value(url, window, cx));
        self.submit(false, window, cx);
    }

    /// The Friends row at the top of the DM list (.ch.navrow).
    pub(crate) fn friends_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.guild_id.is_none() && self.channel.is_none() && self.requests_page.is_none();
        let pending = self.relationships.iter().filter(|r| r.kind == Relation::Incoming).count() as u32;
        let state = RowState { selected, unread: false, muted: false, strong: false };
        channel_row("friends-row".into(), "friends-row".into(), state, cx)
            .h_10()
            .gap_3()
            .child(Icon::new(Glyph::Users).size(ui::ICON_XL).flex_none())
            .child(ui::line("Friends").flex_1())
            .when(pending > 0, |r| r.child(ui::count_badge(pending)))
            .on_click(cx.listener(|this, _, _, cx| this.open_home(cx)))
            .into_any_element()
    }

    /// The Friends page: a header with the filter tabs, then the people in that tab.
    // The Inbox page: every recent mention, under a header like a channel's.
    pub(crate) fn render_inbox_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let page = palette::surface(palette::Level::Page, cx);
        let header = ui::row()
            .occlude()
            .h(px(crate::layout::HEADER_HEIGHT))
            .pl_4()
            .pr_3()
            .gap_2()
            .window_control_area(WindowControlArea::Drag)
            .text_size(ui::TEXT_MD)
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette::strong(cx))
            .child(Icon::new(Glyph::Inbox).size(ui::ICON_XL).text_color(palette::faint(cx)))
            .child("Inbox")
            .child(ui::row().flex_1().justify_end().child(self.caption_clearance(true)));
        ui::col().min_h_0()
            .relative()
            .flex_1()
            .h_full()
            .bg(page)
            .text_color(palette::text(cx))
            .text_size(ui::TEXT_SM)
            // The main view's header: a hard edge, never a hairline.
            .child(ui::hard_edge(|| false, cx).child(header))
            .children(self.voice_notice_bar(cx))
            .child(
                div().id("inbox-page").flex_1().min_h_0().overflow_y_scrollbar().pt_4().px_4().pb_6().child(
                    ui::row().w_full().justify_center().child(ui::col().w_full().max_w(px(INBOX_WIDTH)).gap_3().child(self.inbox_head(false, window, cx)).child(self.inbox_body(None, cx))),
                ),
            )
            .into_any_element()
    }

    pub(crate) fn render_home(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let tab = self.friends_tab;
        let status_of = |id: &str| self.presences.get(id).map(|p| p.status).unwrap_or_default();
        let people: Vec<Relationship> = self
            .relationships
            .iter()
            .filter(|r| match tab {
                FriendsTab::Online => r.kind == Relation::Friend && status_of(&r.user.id) != Status::Offline,
                FriendsTab::All => r.kind == Relation::Friend,
                FriendsTab::Pending => matches!(r.kind, Relation::Incoming | Relation::Outgoing),
                FriendsTab::Blocked => r.kind == Relation::Blocked,
                FriendsTab::AddFriend => false,
            })
            .cloned()
            .collect();
        let pending = self.relationships.iter().filter(|r| r.kind == Relation::Incoming).count() as u32;
        const TABS: [FriendsTab; 5] = [FriendsTab::Online, FriendsTab::All, FriendsTab::Pending, FriendsTab::Blocked, FriendsTab::AddFriend];
        let items = [ui::Tab::new("Online"), ui::Tab::new("All"), ui::Tab::new("Pending").count((pending > 0).then_some(pending)), ui::Tab::new("Blocked"), ui::Tab::new("Add friend").accent()];
        let selected = TABS.iter().position(|t| *t == tab).unwrap_or_default();
        let this = cx.entity().downgrade();
        let tabs = ui::pill_tabs("friends-tabs", &items, selected, move |i, _, cx| {
            this.update(cx, |this, cx| {
                this.friends_tab = TABS[i];
                this.add_friend_result = None;
                this.redraw(Column::Main, cx);
                this.redraw(Column::Panel, cx);
            })
            .ok();
        }, window, cx);
        let new_group = ui::icon_button("new-group-dm", Glyph::ThreadNew, "New Group DM", cx).occlude().on_click(cx.listener(|this, _, window, cx| this.open_group_picker(None, window, cx)));
        let header = ui::row()
            .occlude()
            .h(px(crate::layout::HEADER_HEIGHT))
            .pl_4()
            .pr_3()
            .gap_3()
            .window_control_area(WindowControlArea::Drag)
            .child(
                ui::row()
                    .gap_2()
                    .pr_3()
                    .border_r_1()
                    .border_color(palette::line_strong(cx))
                    .whitespace_nowrap()
                    .text_size(ui::TEXT_MD)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette::strong(cx))
                    .child(Icon::new(Glyph::Users).size(ui::ICON_XL).text_color(palette::faint(cx)))
                    .child("Friends"),
            )
            .child(div().min_w_0().overflow_hidden().occlude().child(tabs))
            .child(ui::row().flex_1().justify_end().child(new_group).child(self.caption_clearance(true)));
        let heading = match tab {
            FriendsTab::Online => "Online",
            FriendsTab::All => "All friends",
            FriendsTab::Pending => "Pending",
            FriendsTab::Blocked => "Blocked",
            FriendsTab::AddFriend => "Add friend",
        };
        let body = match (people.is_empty(), tab) {
            (_, FriendsTab::AddFriend) => self.add_friend_page(cx),
            (true, FriendsTab::Online) => ui::empty_state("friends-empty", Illustration::FriendsOnline, "No one's around to play with Waffle.", "Your friends are offline. Say hi anyway, they'll see it when they're back.", cx)
                .child(div().pt_2().child(ui::button("show-all-friends", Tone::Secondary, cx).child("Show all friends").on_click(cx.listener(|this, _, _, cx| {
                    this.friends_tab = FriendsTab::All;
                    this.redraw(Column::Main, cx);
                    this.redraw(Column::Panel, cx);
                }))))
                .into_any_element(),
            (true, FriendsTab::All) => ui::empty_state("friends-empty", Illustration::FriendsAll, "Waffle is waiting on friends. You don't have to though!", "Add someone by their username to start chatting.", cx)
                .child(div().pt_2().child(ui::button("empty-add-friend", Tone::Primary, cx).child("Add friend").on_click(cx.listener(|this, _, _, cx| {
                    this.friends_tab = FriendsTab::AddFriend;
                    this.redraw(Column::Main, cx);
                    this.redraw(Column::Panel, cx);
                }))))
                .into_any_element(),
            (true, FriendsTab::Pending) => ui::empty_state("friends-empty", Illustration::FriendsPending, "There are no pending friend requests. Here's Waffle for now.", "Requests you send and receive show up here.", cx).into_any_element(),
            (true, FriendsTab::Blocked) => ui::empty_state("friends-empty", Illustration::FriendsBlocked, "You can't unblock Waffle.", "People you block show up here.", cx).into_any_element(),
            (false, _) => {
                let label = div().pt_2().px_2().pb_1().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(palette::faint(cx)).child(format!("{heading} · {}", people.len()));
                let rows: Vec<AnyElement> = people.iter().map(|r| self.friend_row(r, cx)).collect();
                ui::col().gap_0p5().child(label).children(rows).into_any_element()
            }
        };
        // The list stops under the header, a hard edge (the main view's never takes a hairline).
        let page = palette::surface(palette::Level::Page, cx);
        ui::col().min_h_0()
            .relative()
            .flex_1()
            .h_full()
            .bg(page)
            .text_color(palette::text(cx))
            .text_size(ui::TEXT_SM)
            .child(ui::hard_edge(|| false, cx).child(header))
            .children(self.voice_notice_bar(cx))
            .child(
                ui::row().items_stretch()
                    .flex_1()
                    .min_h_0()
                    .child(div().id("friends-list").flex_1().min_w_0().h_full().overflow_y_scrollbar().pt_4().px_4().pb_6().child(body))
                    .children(self.active_now_column(window, cx)),
            )
            .into_any_element()
    }

    // Add a friend, as one centred column rather than Discord's full-width form: the mascot (hopping once a
    // request lands) over the heading, one card holding the username and an optional note, the requests
    // you've sent right under it so you see yours arrive, and public servers as the other way in.
    fn add_friend_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let (strong, muted) = (palette::strong(cx), palette::muted(cx));
        let empty = self.add_friend_input.read(cx).value().trim().is_empty();
        let note_len = self.add_friend_note.read(cx).value().chars().count();
        let poked = self.hovered.as_deref() == Some(MASCOT);
        let mood = if poked || matches!(self.add_friend_result, Some(Ok(_))) { Mood::Happy } else { Mood::Idle };
        // It hops under the pointer, squishes on press, and a click drops you into the username field.
        let mascot = ui::pressable(div().id(MASCOT), 0.9)
            .cursor_pointer()
            .child(crate::mascot::mascot_at("add-friend-mascot", mood, 1., cx))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| this.set_hovered(MASCOT, *hovered, cx)))
            .on_click(cx.listener(|this, _, window, cx| this.add_friend_input.update(cx, |input, cx| input.focus(window, cx))));
        let send = ui::button("send-friend-request", Tone::Primary, cx)
            .flex_none()
            .child("Send request")
            .when(empty, |b| b.opacity(0.5))
            .on_click(cx.listener(|this, _, _, cx| this.send_friend_request(cx)));
        let username_focus = self.add_friend_input.read(cx).focus_handle(cx);
        let note_focus = self.add_friend_note.read(cx).focus_handle(cx);
        let username = ui::row()
            .gap_2()
            .child(div().flex_1().min_w_0().child(ui::text_field("add-friend-name", Input::new(&self.add_friend_input).appearance(false), &username_focus).leading(Icon::new(Glyph::At).size(ui::ICON_MD))))
            .child(send);
        let note = ui::text_area("add-friend-note", Textarea::new(&self.add_friend_note).appearance(false).px_0().text_size(ui::TEXT_SM), &note_focus).error(note_len > NOTE_MAX);
        let hint = ui::field_hint("Optional. If they accept, it opens your DM.", Some(format!("{note_len} / {NOTE_MAX}").into()), cx);
        let result = self.add_friend_result.clone().map(|result| {
            let (ink, text) = match result {
                Ok(text) => (palette::positive_ink(cx), text),
                Err(text) => (palette::danger_ink(cx), text),
            };
            div().text_size(ui::TEXT_SM).text_color(ink).child(text)
        });
        let sent: Vec<AnyElement> = self.relationships.iter().filter(|r| r.kind == Relation::Outgoing).map(|r| self.friend_row(r, cx)).collect();
        let sent = (!sent.is_empty()).then(|| {
            let label = div().px_2().pb_1().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(palette::faint(cx)).child(format!("Sent requests · {}", sent.len()));
            ui::col().gap_0p5().child(label).children(sent)
        });
        let explore = ui::list_row("add-friend-explore", "Explore public servers", cx)
            .height(ui::RowHeight::Tall)
            .lead(ui::row().size_9().flex_none().squircle(ui::RADIUS_MD).bg(palette::positive_ink(cx)).text_color(gpui_kit::white()).justify_center().child(Nav::Compass.icon().size_5()))
            .subtitle("No username handy? Meet people around gaming, music, anime and more.")
            .trailing(div().text_color(palette::faint(cx)).child(Icon::new(Glyph::ChevronRight).size(ui::ICON_MD)))
            .on_click(|_, _, cx| cx.open_url("https://discord.com/servers"));
        let column = ui::col()
            .w_full()
            .max_w(px(ADD_FRIEND_WIDTH))
            .gap_6()
            .child(
                ui::col()
                    .items_center()
                    .gap_2()
                    .child(mascot)
                    .child(div().text_size(ui::TEXT_LG).font_weight(FontWeight::SEMIBOLD).text_color(strong).child("Add a friend"))
                    .child(div().text_color(muted).child("Send a request with their Discord username.")),
            )
            .child(ui::col().gap_2().child(ui::settings_card(cx).p_4().gap_3().child(username).child(note).child(hint)).children(result))
            .children(sent)
            .child(explore);
        ui::row().items_stretch().w_full().justify_center().pt_6().child(column).into_any_element()
    }

    pub(crate) fn on_add_friend_input(&mut self, _: &Entity<SearchState>, event: &InputEvent, _: &mut Window, cx: &mut Context<Self>) {
        match event {
            InputEvent::PressEnter { .. } => self.send_friend_request(cx),
            InputEvent::Change => {
                self.add_friend_result = None;
                self.redraw(Column::Main, cx);
            }
            _ => {}
        }
    }

    fn send_friend_request(&mut self, cx: &mut Context<Self>) {
        let username = self.add_friend_input.read(cx).value().trim().trim_start_matches('@').to_lowercase();
        if username.is_empty() {
            return;
        }
        // Discord caps a friend request's note at 120 characters (docs.discord.food, Relationships).
        let note: String = self.add_friend_note.read(cx).value().trim().chars().take(120).collect();
        let Some(client) = self.client.clone() else { return };
        // Discord may want a captcha for a friend request: its page opens in the browser, as for an invite.
        let (captcha_url, mut captcha_urls) = futures::channel::mpsc::unbounded::<String>();
        cx.spawn(async move |_, cx| {
            while let Some(url) = futures::StreamExt::next(&mut captcha_urls).await {
                cx.update(|cx: &mut App| cx.open_url(&url));
            }
        })
        .detach(); // lint-allow(detached-task): ends when the captcha channel closes with the request, holding no entity
        cx.spawn(async move |this, cx| {
            let mut body = json!({ "username": username, "discriminator": null });
            if !note.is_empty() {
                body["note"] = json!(note);
            }
            let result = client.call_with_captcha(Method::POST, "/users/@me/relationships".into(), Some(body), captcha_url).await;
            this.update(cx, |this, cx| {
                this.add_friend_result = Some(match result {
                    Ok(_) => Ok(format!("Success! Your friend request to {username} was sent.").into()),
                    Err(error) => Err(add_friend_error(&error, &username).into()),
                });
                this.redraw(Column::Main, cx);
            })
            .ok();
        })
        .detach(); // lint-allow(detached-task): a one-shot request that ends when it returns, holding a WeakEntity
    }

    // A person in the list (.friend): who they are, what they're up to, and the actions that fit.
    fn friend_row(&self, relation: &Relationship, cx: &mut Context<Self>) -> AnyElement {
        let user = &relation.user;
        let presence = self.presences.get(&user.id).cloned().unwrap_or_default();
        let subtitle: String = match relation.kind {
            Relation::Incoming => "Wants to be friends".into(),
            Relation::Outgoing => "You sent a request".into(),
            Relation::Blocked => "Blocked".into(),
            Relation::Friend => presence.custom.clone().or_else(|| presence.activity.as_ref().map(|a| a.text.clone())).unwrap_or_else(|| if presence.status == Status::Offline { "Offline".into() } else { presence.status.label().into() }),
        };
        let page = palette::surface(palette::Level::Page, cx);
        let status = (relation.kind == Relation::Friend).then_some(presence.mark());
        let id = user.id.clone();
        let acts: Vec<ui::Control> = match relation.kind {
            Relation::Friend => vec![
                friend_action(("message", &id), Glyph::ChatRound, "Message", None, cx).on_click(cx.listener({
                    let id = id.clone();
                    move |this, _, window, cx| this.message_user(id.clone(), window, cx)
                })),
                friend_action(("more", &id), Glyph::MoreVertical, "More", None, cx).on_mouse_down(gpui_kit::MouseButton::Left, {
                    let user = user.clone();
                    cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_user_menu(user.clone(), None, crate::user_menu::Surface::Generic, e.position, cx))
                }),
            ],
            Relation::Incoming => vec![
                friend_action(("accept", &id), Glyph::Checkmark, "Accept", Some(palette::positive_ink(cx)), cx).on_click(cx.listener({
                    let user = user.clone();
                    move |this, _, _, cx| this.relation_optimistic(&user.id, Some((user.clone(), Relation::Friend)), Method::PUT, Some(json!({})), cx)
                })),
                friend_action(("ignore", &id), Glyph::X, "Ignore", Some(palette::danger_ink(cx)), cx).on_click(cx.listener({
                    let id = id.clone();
                    move |this, _, _, cx| this.relation_optimistic(&id, None, Method::DELETE, None, cx)
                })),
            ],
            Relation::Outgoing => vec![friend_action(("cancel", &id), Glyph::X, "Cancel request", Some(palette::danger_ink(cx)), cx).on_click(cx.listener({
                let id = id.clone();
                move |this, _, _, cx| this.relation_optimistic(&id, None, Method::DELETE, None, cx)
            }))],
            Relation::Blocked => vec![friend_action(("unblock", &id), Glyph::X, "Unblock", None, cx).on_click(cx.listener({
                let id = id.clone();
                move |this, _, _, cx| this.relation_optimistic(&id, None, Method::DELETE, None, cx)
            }))],
        };
        ui::list_row(SharedString::from(format!("friend-{}", user.id)), user.name.clone(), cx)
            .height(ui::RowHeight::Tall)
            .lead(avatar_with_status(&user.avatar, 32., status, page))
            .subtitle(ui::line(subtitle))
            // The buttons keep their press to themselves, so the row's own click (open the DM) never fires with them.
            .trailing(ui::row().items_stretch().flex_none().gap_2().on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| cx.stop_propagation()).children(acts))
            .when(relation.kind == Relation::Friend, |row| {
                let id = user.id.clone();
                row.on_click(cx.listener(move |this, _, window, cx| this.message_user(id.clone(), window, cx)))
            })
            .on_mouse_down(gpui_kit::MouseButton::Right, {
                let user = user.clone();
                cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_user_menu(user.clone(), None, crate::user_menu::Surface::Generic, e.position, cx))
            })
            .into_any_element()
    }
}

// Discord's inline copy per relationship error code. No answer at all, or a signed-out session, keeps its
// own words rather than blaming the username.
fn add_friend_error(error: &ApiError, username: &str) -> String {
    match (error.status, error.code) {
        (0 | 401, _) => error.text.clone(),
        (429, _) => "You're sending requests too quickly. Wait a moment and try again.".into(),
        (_, Some(80000)) => format!("{username} is not accepting friend requests."),
        (_, Some(80007)) => "You're already friends with that user!".into(),
        (_, Some(30002)) => "You've hit the 1,000 friend limit. Remove a friend to add someone new.".into(),
        (_, Some(30078)) => "You've maxed out your pending outgoing friend requests.".into(),
        (_, Some(40068 | 340007)) => "Your account has limited access".into(),
        _ => "Hm, didn't work. Double check that the username is correct.".into(),
    }
}

// A friend row's action (.friend .acts .iconbtn): a raised 36px tile with a hairline; `tone` is the
// ink it hovers to (accept green, ignore coral).
fn friend_action(id: (&'static str, &str), glyph: Glyph, tooltip: &'static str, tone: Option<Hsla>, cx: &App) -> ui::Control {
    let ink = tone.unwrap_or(palette::strong(cx));
    ui::icon_button(SharedString::from(format!("{}-{}", id.0, id.1)), glyph, tooltip, cx).sized(36.).tone(Tone::Secondary, cx).bordered(cx).hover(move |s| s.text_color(ink))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error(status: u16, code: Option<u64>) -> ApiError {
        ApiError { status, code, text: "raw".into() }
    }

    #[test]
    fn add_friend_errors_use_discords_copy() {
        assert_eq!(add_friend_error(&error(400, Some(80000)), "rosa"), "rosa is not accepting friend requests.");
        assert_eq!(add_friend_error(&error(400, Some(80007)), "rosa"), "You're already friends with that user!");
        assert_eq!(add_friend_error(&error(400, Some(30002)), "rosa"), "You've hit the 1,000 friend limit. Remove a friend to add someone new.");
        assert_eq!(add_friend_error(&error(429, None), "rosa"), "You're sending requests too quickly. Wait a moment and try again.");
        assert_eq!(add_friend_error(&error(400, Some(80004)), "rosa"), "Hm, didn't work. Double check that the username is correct.");
        assert_eq!(add_friend_error(&error(404, None), "rosa"), "Hm, didn't work. Double check that the username is correct.");
        assert_eq!(add_friend_error(&error(0, None), "rosa"), "raw");
    }

    #[test]
    fn back_and_forward_walk_visits_and_a_visit_drops_forward() {
        let mut history = History::default();
        history.visit("friends");
        history.visit("general");
        assert_eq!(history.step(false, "dm"), Some("general"));
        assert_eq!(history.step(false, "general"), Some("friends"));
        assert_eq!(history.step(false, "friends"), None);
        assert_eq!(history.step(true, "friends"), Some("general"));
        history.visit("general");
        assert_eq!(history.step(true, "rules"), None);
        assert_eq!(history.step(false, "rules"), Some("general"));
    }
}
