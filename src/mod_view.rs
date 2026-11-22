// Mod View: one member at a glance for moderators, in the right panel (docs/discord-moderation-re.md §2,
// design/explore-navigation.html#mod-view). Signals, server activity, mod permissions, roles and account
// facts, with the action log and every granted permission as sub-pages.
use std::time::Duration;

use chrono::{DateTime, Utc};
use gpui_kit::{AnyElement, App, ClickEvent, Context, Div, FontWeight, SharedString, Window, component::Icon, div, prelude::*, px, rgb};
use reqwest::Method;
use serde_json::{Value, json};

use crate::{
    RightPanel, Waffle,
    discord::{Guild, MemberFacts, Role, Status, User, perm},
    icons::Glyph,
    layout::decorated_avatar,
    palette,
    panel::{group_label, wait_line},
    server_settings::{
        audit,
        perms::{self, CREATE_GUILD_EXPRESSIONS, MANAGE_GUILD, MANAGE_GUILD_EXPRESSIONS, MANAGE_WEBHOOKS, Permission},
        view::{sentence_spans, snowflake_time},
    },
    ui::{self, Squircle},
};

// Wider than the member list: sections carry a label and a value side by side.
pub(crate) const PANEL_WIDTH: f32 = 340.;

// Discord's Gate-7: any one of these at server level opens Mod View.
const GATE: u64 = perm::ADMINISTRATOR | MANAGE_GUILD | perm::BAN_MEMBERS | perm::KICK_MEMBERS | perm::MODERATE_MEMBERS | perm::MANAGE_ROLES | perm::MANAGE_NICKNAMES;
// Permissions Discord strips under required 2FA; they get the shield.
const ELEVATED: u64 = perm::KICK_MEMBERS | perm::BAN_MEMBERS | perm::ADMINISTRATOR | perm::MANAGE_CHANNELS | MANAGE_GUILD | perm::MANAGE_ROLES | perm::MANAGE_MESSAGES | perm::MANAGE_THREADS | perm::MODERATE_MEMBERS;
const MENTION_EVERYONE: u64 = 1 << 17;
const MANAGE_EVENTS: u64 = 1 << 33;
const CREATE_EVENTS: u64 = 1 << 44;
// GAP: Discord also adds its community-moderator set (module 124759), which the spec doesn't enumerate.
const MODERATOR: u64 = ELEVATED | MANAGE_GUILD_EXPRESSIONS | CREATE_GUILD_EXPRESSIONS | MANAGE_EVENTS | CREATE_EVENTS | MENTION_EVERYONE | MANAGE_WEBHOOKS;

// Not verified: member flag bits, from memory of discord-api-docs (guild member flags).
const COMPLETED_ONBOARDING: u64 = 1 << 1;
const BYPASSES_VERIFICATION: u64 = 1 << 2;

const MEDIA_EXTENSIONS: [&str; 12] = ["png", "jpg", "webp", "mov", "mp4", "qt", "gif", "mp3", "wav", "flac", "ogg", "opus"];
// Not verified: the guild feature behind Discord's rules gate.
const RULES_GATE: &str = "MEMBER_VERIFICATION_GATE_ENABLED";
// The member request goes over the gateway; nothing answers for someone who has left, so stop waiting.
const MEMBER_WAIT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    Info,
    Permissions,
    AuditLog,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Messages,
    Links,
    Media,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Counts {
    messages: u64,
    links: u64,
    media: u64,
}

#[derive(Clone, Debug, PartialEq)]
struct JoinSource {
    kind: u64,
    code: Option<String>,
    inviter_id: Option<String>,
    integration_type: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum Signal {
    UnusualDm(DateTime<Utc>),
    TimedOut(DateTime<Utc>),
}

pub(crate) struct ModView {
    guild_id: String,
    user_id: String,
    page: Page,
    // Bumped by Refresh: answers to an earlier load are dropped.
    load: u32,
    member: Option<MemberFacts>,
    // The member request has had its chance: what's missing now shows as missing, not loading.
    settled: bool,
    username: Option<String>,
    join: Option<JoinSource>,
    rules_gate: bool,
    counts: Option<Result<Counts, String>>,
    audit: Option<Result<(Vec<audit::Entry>, audit::Directory), String>>,
}

impl ModView {
    fn new(guild_id: String, user_id: String) -> Self {
        Self { guild_id, user_id, page: Page::Info, load: 0, member: None, settled: false, username: None, join: None, rules_gate: false, counts: None, audit: None }
    }

    pub(crate) fn guild_id(&self) -> &str {
        &self.guild_id
    }

    fn loading(&self) -> bool {
        self.member.is_none() && !self.settled
    }
}

impl Waffle {
    // Opens Mod View on a member of `guild_id` in the right panel, when we may moderate there.
    pub(crate) fn open_mod_view(&mut self, guild_id: String, user_id: String, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_mod_view(&guild_id) {
            return;
        }
        self.profile_card = None;
        self.right_panel = RightPanel::ModView(Box::new(ModView::new(guild_id, user_id)));
        self.load_mod_view(cx);
        self.tick_mod_view(cx);
    }

    pub(crate) fn can_mod_view(&self, guild_id: &str) -> bool {
        let (Some(me), Some(guild)) = (self.me.as_ref(), self.guilds.iter().find(|g| g.id == guild_id)) else { return false };
        can_open(guild.permissions(&me.id))
    }

    // Discord closes the panel the moment its gate turns false (roles changed under us).
    pub(crate) fn close_mod_view_if_barred(&mut self) {
        if matches!(&self.right_panel, RightPanel::ModView(view) if !self.can_mod_view(&view.guild_id)) {
            self.right_panel = RightPanel::Members;
        }
    }

    // Esc: a sub-page steps back to the main page, the main page closes Mod View.
    pub(crate) fn mod_view_escape(&mut self, cx: &mut Context<Self>) -> bool {
        let RightPanel::ModView(view) = &mut self.right_panel else { return false };
        match view.page {
            Page::Info => self.right_panel = RightPanel::Members,
            _ => view.page = Page::Info,
        }
        cx.notify();
        true
    }

    fn mod_view(&mut self) -> Option<&mut ModView> {
        match &mut self.right_panel {
            RightPanel::ModView(view) => Some(view),
            _ => None,
        }
    }

    // The view still on this member and this load, for an answer that just landed.
    fn mod_view_at(&mut self, guild_id: &str, user_id: &str, load: u32) -> Option<&mut ModView> {
        self.mod_view().filter(|v| v.guild_id == guild_id && v.user_id == user_id && v.load == load)
    }

    // A members chunk: the facts for the member Mod View is waiting on.
    pub(crate) fn mod_view_members(&mut self, guild_id: &str, facts: Vec<MemberFacts>) {
        let Some(view) = self.mod_view().filter(|v| v.guild_id == guild_id) else { return };
        if let Some(found) = facts.into_iter().find(|f| f.user_id == view.user_id) {
            view.member = Some(found);
        }
    }

    pub(crate) fn mod_view_timed_out(&mut self, guild_id: &str, user_id: &str, until: Option<DateTime<Utc>>, cx: &mut Context<Self>) {
        let Some(member) = self.mod_view().filter(|v| v.guild_id == guild_id && v.user_id == user_id).and_then(|v| v.member.as_mut()) else { return };
        member.timed_out_until = until;
        cx.notify();
    }

    // Everything at once: the member over the gateway, and over REST the join source, the profile (for
    // the username), the server's features, the message counts and the action log.
    fn load_mod_view(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.mod_view() else { return };
        let (g, u, load) = (view.guild_id.clone(), view.user_id.clone(), view.load);
        let Some(client) = self.client.clone() else { return };
        client.request_members(&g, vec![u.clone()]);

        let tabs = |extra: Value| {
            let mut tab = json!({ "author_id": [u], "limit": 1, "cursor": null });
            if let (Some(tab), Some(extra)) = (tab.as_object_mut(), extra.as_object()) {
                tab.extend(extra.clone());
            }
            tab
        };
        // Not verified: `author_id` as an array here, as the history search sends it.
        let counts = json!({
            "tabs": { "messages": tabs(json!({})), "links": tabs(json!({ "has": ["link"] })), "media": tabs(json!({ "attachment_extension": MEDIA_EXTENSIONS })) },
            "track_exact_total_hits": true,
            "include_nsfw": true,
        });
        let calls: [(Method, String, Option<Value>, Apply); 5] = [
            (Method::POST, format!("/guilds/{g}/members/supplemental"), Some(json!({ "user_ids": [u] })), apply_join),
            (Method::GET, format!("/users/{u}/profile?guild_id={g}"), None, apply_profile),
            (Method::GET, format!("/guilds/{g}"), None, apply_guild),
            (Method::POST, format!("/guilds/{g}/messages/search/tabs"), Some(counts), apply_counts),
            (Method::GET, format!("/guilds/{g}/audit-logs?limit=50&target_id={u}"), None, apply_audit),
        ];
        for (method, path, body, apply) in calls {
            let (client, g, u) = (client.clone(), g.clone(), u.clone());
            let join = path.ends_with("/supplemental");
            cx.spawn(async move |this, cx| {
                let result = client.call(method, path, body).await;
                this.update(cx, |this, cx| {
                    let guild = this.guilds.iter().find(|x| x.id == g).cloned();
                    let Some(view) = this.mod_view_at(&g, &u, load) else { return };
                    apply(view, result, guild.as_ref());
                    // The inviter's name comes the way the member's did.
                    let inviter = view.join.as_ref().filter(|_| join).and_then(|j| j.inviter_id.clone());
                    if let Some(inviter) = inviter.filter(|i| this.members.get(&g).is_none_or(|m| !m.contains_key(i))) {
                        client.request_members(&g, vec![inviter]);
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        let (g2, u2) = (g, u);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(MEMBER_WAIT).await;
            this.update(cx, |this, cx| {
                if let Some(view) = this.mod_view_at(&g2, &u2, load) {
                    view.settled = true;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn refresh_mod_view(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.mod_view() else { return };
        let fresh = ModView { page: view.page, load: view.load + 1, ..ModView::new(view.guild_id.clone(), view.user_id.clone()) };
        *view = fresh;
        self.load_mod_view(cx);
        cx.notify();
    }

    // The timeout counts down each second while Mod View stays open on this member.
    fn tick_mod_view(&self, cx: &mut Context<Self>) {
        let RightPanel::ModView(view) = &self.right_panel else { return };
        let (g, u) = (view.guild_id.clone(), view.user_id.clone());
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let open = this
                    .update(cx, |this, cx| {
                        let Some(view) = this.mod_view().filter(|v| v.guild_id == g && v.user_id == u) else { return false };
                        if view.member.as_ref().and_then(|m| m.timed_out_until).is_some_and(|t| t > Utc::now()) {
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !open {
                    break;
                }
            }
        })
        .detach();
    }

    fn set_mod_view_page(&mut self, page: Page, cx: &mut Context<Self>) {
        if let Some(view) = self.mod_view() {
            view.page = page;
        }
        self.panel_scroll.set_offset(gpui_kit::point(px(0.), px(0.)));
        cx.notify();
    }

    fn mod_view_user(&self, view: &ModView) -> Option<User> {
        self.members.get(&view.guild_id).and_then(|m| m.get(&view.user_id)).cloned().or_else(|| self.known_users().into_iter().find(|u| u.id == view.user_id))
    }

    // The panel's header row: the title with Refresh on the main page, Back on a sub-page; Close on both.
    pub(crate) fn mod_view_head(&self, view: &ModView, cx: &mut Context<Self>) -> Div {
        let title = match view.page {
            Page::Info => "Mod View",
            Page::Permissions => "All granted permissions",
            Page::AuditLog => "Moderator action log",
        };
        let back = (view.page != Page::Info).then(|| ui::icon_button("mod-view-back", Glyph::ChevronLeft, "Back", cx).occlude().on_click(cx.listener(|this, _, _, cx| this.set_mod_view_page(Page::Info, cx))));
        let refresh = (view.page == Page::Info).then(|| ui::icon_button("mod-view-refresh", Glyph::Retry, "Refresh", cx).occlude().on_click(cx.listener(|this, _, _, cx| this.refresh_mod_view(cx))));
        let close = ui::icon_button("mod-view-close", Glyph::X, "Close", cx).occlude().on_click(cx.listener(|this, _, _, cx| {
            this.right_panel = RightPanel::Members;
            cx.notify();
        }));
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap_1()
            .children(back)
            .child(div().flex_1().min_w_0().truncate().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).child(title))
            .children(refresh)
            .child(close)
    }

    pub(crate) fn mod_view_body(&self, view: &ModView, cx: &mut Context<Self>) -> AnyElement {
        if view.loading() {
            return wait_line("mod-view-wait", "Loading member…", cx).into_any_element();
        }
        let Some(guild) = self.guilds.iter().find(|g| g.id == view.guild_id) else { return div().into_any_element() };
        let roles = self.member_roles.get(&view.guild_id).and_then(|r| r.get(&view.user_id)).cloned().unwrap_or_default();
        match view.page {
            Page::Info => self.mod_view_info(view, guild, &roles, cx),
            Page::Permissions => permissions_page(view, guild, &roles, cx),
            Page::AuditLog => audit_page(view, cx),
        }
    }

    fn mod_view_info(&self, view: &ModView, guild: &Guild, roles: &[String], cx: &mut Context<Self>) -> AnyElement {
        let now = Utc::now();
        let user = self.mod_view_user(view).unwrap_or_else(|| User { id: view.user_id.clone(), name: view.user_id.clone(), ..Default::default() });
        let signals = signals(view.member.as_ref(), now);
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(div().flex().flex_col().gap(px(12.)).child(self.mod_view_who(view, &user, cx)).child(self.mod_view_actions(view, &user, cx)))
            .when(!signals.is_empty(), |d| d.child(section("Signals", cx).children(signals.iter().map(|s| signal_row(s, now, cx)))))
            .child(self.mod_view_activity(view, cx))
            .child(self.mod_view_permissions(view, guild, roles, cx))
            .child(self.mod_view_roles(view, guild, roles, cx))
            .child(self.mod_view_account(view, &user, cx))
            .into_any_element()
    }

    fn mod_view_who(&self, view: &ModView, user: &User, cx: &App) -> Div {
        let status = self.presences.get(&user.id).map(|p| p.mark()).unwrap_or(Status::Offline.into());
        let surface = palette::surface(palette::Level::Sidebar, cx);
        let username = view.username.clone().unwrap_or_default();
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .px_2()
            .child(decorated_avatar(&user.avatar, None, 48., Some(status), surface))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().truncate().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(user.name.clone()))
                    .child(div().truncate().text_color(palette::muted(cx)).child(username)),
            )
    }

    // Message, Kick, Ban, Timeout or Remove timeout, Copy ID: each dimmed when its gate fails.
    fn mod_view_actions(&self, view: &ModView, user: &User, cx: &mut Context<Self>) -> Div {
        let (g, u) = (view.guild_id.clone(), view.user_id.clone());
        let is_me = self.me.as_ref().is_some_and(|me| me.id == u);
        let member = view.member.is_some();
        let timed_out = view.member.as_ref().and_then(|m| m.timed_out_until).is_some_and(|t| t > Utc::now());
        let name = user.name.clone();
        let message = {
            let u = u.clone();
            ui::icon_button("mod-view-message", Glyph::ChatRound, "Message", cx).disabled(is_me).on_click(cx.listener(move |this, _, window, cx| this.message_user(u.clone(), window, cx)))
        };
        let kick = {
            let (g, u, name) = (g.clone(), u.clone(), name.clone());
            let can = member && self.can_manage(&g, &u, perm::KICK_MEMBERS);
            ui::icon_button("mod-view-kick", Glyph::DoorOpen, "Kick", cx).disabled(!can).on_click(cx.listener(move |this, _, _, cx| this.ask_kick(g.clone(), u.clone(), name.clone(), cx)))
        };
        let ban = {
            let (g, u, name) = (g.clone(), u.clone(), name.clone());
            let can = !user.bot && self.can_manage(&g, &u, perm::BAN_MEMBERS);
            ui::icon_button("mod-view-ban", Glyph::Gavel, "Ban", cx).disabled(!can).on_click(cx.listener(move |this, _, _, cx| this.ask_ban(g.clone(), u.clone(), name.clone(), cx)))
        };
        let can_timeout = member && self.can_timeout(&g, &u);
        let timeout = match timed_out {
            true => ui::icon_button("mod-view-timeout", Glyph::Timer, "Remove timeout", cx).disabled(!can_timeout).on_click(cx.listener({
                let (g, u) = (g.clone(), u.clone());
                move |this, _, _, cx| this.set_timeout(g.clone(), u.clone(), None, cx)
            })),
            false => ui::icon_button("mod-view-timeout", Glyph::Timer, "Timeout", cx).disabled(!can_timeout).on_click(cx.listener({
                let (g, u) = (g.clone(), u.clone());
                move |this, event: &ClickEvent, _, cx| {
                    let entries = this.timeout_entries(&g, &u);
                    this.open_menu(event.position(), entries, cx);
                }
            })),
        };
        let copy = ui::icon_button("mod-view-copy-id", Glyph::Id, "Copy ID", cx).on_click(cx.listener(move |this, _, _, cx| this.copy_text(u.clone(), cx)));
        div().flex().gap_1().px_1().child(message).child(kick).child(ban).child(timeout).child(copy)
    }

    fn mod_view_activity(&self, view: &ModView, cx: &mut Context<Self>) -> Div {
        let count = |pick: fn(&Counts) -> u64| match &view.counts {
            None => Count::Loading,
            Some(Err(_)) => Count::Failed,
            Some(Ok(c)) => Count::Ready(pick(c)),
        };
        let tab_row = |tab: Tab, glyph, label, n: Count, cx: &mut Context<Self>| {
            let (g, u) = (view.guild_id.clone(), view.user_id.clone());
            count_row(SharedString::from(format!("mod-view-{label}")), glyph, label, n, cx).when(n.navigates(), |r| r.on_click(cx.listener(move |this, _, _, cx| this.mod_view_search(&g, &u, tab, cx))))
        };
        let audit = match &view.audit {
            None => Count::Loading,
            Some(Err(_)) => Count::Failed,
            Some(Ok((entries, _))) => Count::Ready(entries.len() as u64),
        };
        section("Server activity", cx)
            .child(tab_row(Tab::Messages, Glyph::ChatRound, "Messages", count(|c| c.messages), cx))
            .child(tab_row(Tab::Links, Glyph::Link, "Links", count(|c| c.links), cx))
            .child(tab_row(Tab::Media, Glyph::Image, "Media", count(|c| c.media), cx))
            .child(count_row("mod-view-audit-log".into(), Glyph::Note, "Audit log", audit, cx).when(audit.navigates(), |r| r.on_click(cx.listener(|this, _, _, cx| this.set_mod_view_page(Page::AuditLog, cx)))))
    }

    // Messages, Links and Media open the search panel on this member's messages in the server.
    fn mod_view_search(&mut self, guild_id: &str, user_id: &str, tab: Tab, cx: &mut Context<Self>) {
        let mut params = vec![("author_id".to_owned(), user_id.to_owned()), ("include_nsfw".to_owned(), "true".to_owned())];
        let words = match tab {
            Tab::Messages => format!("from:{user_id}"),
            Tab::Links => {
                params.push(("has".into(), "link".into()));
                format!("from:{user_id} has:link")
            }
            Tab::Media => {
                params.extend(MEDIA_EXTENSIONS.iter().map(|e| ("attachment_extension".to_owned(), (*e).to_owned())));
                format!("from:{user_id} has:file")
            }
        };
        let query = crate::search::Query { params, ..Default::default() };
        self.show_search(crate::search_results::Results::new(format!("guilds/{guild_id}"), &words, query, crate::search::Sort::Newest), cx);
    }

    fn mod_view_permissions(&self, view: &ModView, guild: &Guild, roles: &[String], cx: &mut Context<Self>) -> Div {
        let bits = granted(guild, &view.user_id, roles);
        let held = mod_permissions(bits);
        let all = perms::all().filter(|p| bits & p.bit != 0).count();
        let more = ui::button("mod-view-all-perms", ui::Tone::Link, cx).small().child(format!("All ({all})")).on_click(cx.listener(|this, _, _, cx| this.set_mod_view_page(Page::Permissions, cx)));
        let head = div().flex().items_center().justify_between().pr_2().child(group_label("Mod permissions", cx)).child(more);
        let body = match held.is_empty() {
            true => div().px(px(6.)).text_color(palette::positive_ink(cx)).child("No mod permissions"),
            false => div().px(px(6.)).flex().flex_wrap().gap_1().children(held.into_iter().map(|(p, elevated)| perm_pill(p, elevated, cx))),
        };
        div().flex().flex_col().gap_1().child(head).child(body)
    }

    fn mod_view_roles(&self, view: &ModView, guild: &Guild, roles: &[String], cx: &mut Context<Self>) -> Div {
        let held = ordered_roles(guild, roles);
        let manage = self.me.as_ref().is_some_and(|me| guild.can(&me.id, perm::MANAGE_ROLES));
        let chips = held.iter().enumerate().map(|(i, r)| {
            let color = if r.color == 0 { palette::status_in(Status::Offline, cx) } else { rgb(r.color).into() };
            let chip = ui::role_chip(r.name.clone(), color, cx).id(SharedString::from(format!("mod-view-role-{}", r.id)));
            match i {
                0 => chip.tooltip(|window, cx| ui::tip("Highest role", window, cx)).into_any_element(),
                _ => chip.into_any_element(),
            }
        }).collect::<Vec<_>>();
        let add = manage.then(|| {
            let (g, u) = (view.guild_id.clone(), view.user_id.clone());
            ui::icon_button("mod-view-add-role", Glyph::Plus, "Add role", cx).small().on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                let (g, u) = (g.clone(), u.clone());
                this.open_live_menu(event.position(), move |this| this.guilds.iter().find(|x| x.id == g).map(|guild| this.role_entries(guild, &u)).unwrap_or_default(), cx);
            }))
        });
        let empty = (held.is_empty() && !manage).then(|| div().text_color(palette::muted(cx)).child("No roles"));
        section("Roles", cx).child(div().px(px(6.)).flex().flex_wrap().items_center().gap_1().children(chips).children(add).children(empty))
    }

    fn mod_view_account(&self, view: &ModView, user: &User, cx: &mut Context<Self>) -> Div {
        let member = view.member.as_ref();
        let verified = member.map(passed_verification);
        let rules = member.filter(|_| view.rules_gate).map(agreed_to_rules);
        let joined = member.and_then(|m| m.joined_at).map(|t| short_date(&t.with_timezone(&chrono::Local)));
        let (glyph, method) = view.join.as_ref().map(join_label).unwrap_or((None, "Unknown".into()));
        let footer = view.join.as_ref().and_then(|j| self.join_footer(&view.guild_id, j, cx));
        section("Account", cx)
            .children(verified.map(|ok| value_row("Passed verification level", check_mark(ok, cx), cx)))
            .children(rules.map(|ok| value_row("Agreed to rules", check_mark(ok, cx), cx)))
            .child(value_row("Discord join date", div().child(snowflake_date(&user.id)), cx))
            .child(value_row("Server join date", div().child(joined.unwrap_or_else(|| "Unknown".into())), cx))
            .child(value_row("Join method", div().flex().items_center().gap_1().children(glyph.map(|g| Icon::new(g).size(px(14.)))).child(method), cx))
            .children(footer)
    }

    // "Invited by @name", which opens Mod View on the inviter; "Added by bot" for a bot invite.
    fn join_footer(&self, guild_id: &str, join: &JoinSource, cx: &mut Context<Self>) -> Option<AnyElement> {
        let line = |text: String| div().px(px(6.)).text_size(px(12.)).text_color(palette::muted(cx)).child(text);
        if join.kind == 1 {
            return Some(line("Added by bot".into()).into_any_element());
        }
        let inviter = join.inviter_id.clone()?;
        let name = self.members.get(guild_id).and_then(|m| m.get(&inviter)).map(|u| u.name.clone()).unwrap_or_else(|| inviter.clone());
        let g = guild_id.to_owned();
        Some(
            line(format!("Invited by @{name}"))
                .id("mod-view-inviter")
                .cursor_pointer()
                .hover(|s| s.underline())
                .on_click(cx.listener(move |this, _, window, cx| this.open_mod_view(g.clone(), inviter.clone(), window, cx)))
                .into_any_element(),
        )
    }
}

#[cfg(debug_assertions)]
impl Waffle {
    // Demo scenes: everything already back, timed out 42 minutes more, unusual DMs three days ago.
    pub(crate) fn demo_mod_view(&mut self, guild_id: String, user_id: String, audit: Value, cx: &mut Context<Self>) {
        let now = Utc::now();
        let guild = self.guilds.iter().find(|g| g.id == guild_id).cloned();
        let mut view = ModView::new(guild_id, user_id.clone());
        view.member = Some(MemberFacts {
            user_id: user_id.clone(),
            joined_at: Some(now - chrono::Duration::days(100)),
            timed_out_until: Some(now + chrono::Duration::seconds(42 * 60 + 10)),
            unusual_dm_until: Some(now - chrono::Duration::days(3)),
            flags: COMPLETED_ONBOARDING,
            pending: false,
        });
        view.username = Some("wren.wav".into());
        view.join = Some(JoinSource { kind: 5, code: Some("nightshift".into()), inviter_id: Some("12582912".into()), integration_type: None });
        view.rules_gate = true;
        view.counts = Some(Ok(Counts { messages: 1284, links: 37, media: 112 }));
        apply_audit(&mut view, Ok(audit), guild.as_ref());
        self.right_panel = RightPanel::ModView(Box::new(view));
        self.tick_mod_view(cx);
    }
}

// Each REST answer folds into the view; a failure leaves its section as "missing" or "failed".
type Apply = fn(&mut ModView, Result<Value, String>, Option<&Guild>);

fn apply_join(view: &mut ModView, result: Result<Value, String>, _: Option<&Guild>) {
    view.join = result.ok().and_then(|body| parse_join(&body, &view.user_id));
}

fn apply_profile(view: &mut ModView, result: Result<Value, String>, _: Option<&Guild>) {
    view.username = result.ok().and_then(|body| body["user"]["username"].as_str().map(str::to_owned));
}

fn apply_guild(view: &mut ModView, result: Result<Value, String>, _: Option<&Guild>) {
    view.rules_gate = result.ok().is_some_and(|body| body["features"].as_array().into_iter().flatten().any(|f| f.as_str() == Some(RULES_GATE)));
}

fn apply_counts(view: &mut ModView, result: Result<Value, String>, _: Option<&Guild>) {
    view.counts = Some(result.map(|body| parse_counts(&body)));
}

fn apply_audit(view: &mut ModView, result: Result<Value, String>, guild: Option<&Guild>) {
    view.audit = Some(result.map(|body| {
        let mut page = audit::parse(&body);
        if let Some(guild) = guild {
            page.names.roles.extend(guild.roles.iter().map(|r| (r.id.clone(), r.name.clone())));
            page.names.channels.extend(guild.channels.iter().map(|c| (c.id.clone(), c.name.clone())));
        }
        (page.entries, page.names)
    }));
}

fn parse_counts(body: &Value) -> Counts {
    let total = |tab: &str| body["tabs"][tab]["total_results"].as_u64().unwrap_or(0);
    Counts { messages: total("messages"), links: total("links"), media: total("media") }
}

fn parse_join(body: &Value, user_id: &str) -> Option<JoinSource> {
    let entry = body.as_array()?.iter().find(|e| e["user_id"].as_str() == Some(user_id))?;
    let text = |key: &str| entry[key].as_str().map(str::to_owned);
    Some(JoinSource { kind: entry["join_source_type"].as_u64().unwrap_or(0), code: text("source_invite_code"), inviter_id: text("inviter_id"), integration_type: text("integration_type") })
}

pub(crate) fn can_open(permissions: u64) -> bool {
    permissions & GATE != 0
}

// What the member may do server-wide: the owner and Administrator hold every permission.
fn granted(guild: &Guild, user_id: &str, roles: &[String]) -> u64 {
    if guild.owner_id.as_deref() == Some(user_id) {
        return u64::MAX;
    }
    let bits = guild.roles.iter().filter(|r| r.id == guild.id || roles.contains(&r.id)).fold(0, |acc, r| acc | r.permissions);
    if bits & perm::ADMINISTRATOR != 0 { u64::MAX } else { bits }
}

// The mod-relevant permissions held, in the role editor's order, each with whether it's elevated.
fn mod_permissions(bits: u64) -> Vec<(&'static Permission, bool)> {
    perms::all().filter(|p| bits & p.bit & MODERATOR != 0).map(|p| (p, p.bit & ELEVATED != 0)).collect()
}

// The roles that grant `bit`: those carrying it or Administrator, @everyone included.
fn granting<'a>(guild: &'a Guild, roles: &[String], bit: u64) -> Vec<&'a Role> {
    let mut out: Vec<&Role> = guild.roles.iter().filter(|r| (r.id == guild.id || roles.contains(&r.id)) && r.permissions & (bit | perm::ADMINISTRATOR) != 0).collect();
    out.sort_by_key(|r| std::cmp::Reverse(r.position));
    out
}

// Highest first, then the rest in the server's order.
fn ordered_roles<'a>(guild: &'a Guild, roles: &[String]) -> Vec<&'a Role> {
    let mut held: Vec<&Role> = guild.roles.iter().filter(|r| r.id != guild.id && roles.contains(&r.id)).collect();
    held.sort_by_key(|r| std::cmp::Reverse(r.position));
    held
}

// Signals show only when there's one: unusual DMs (no cutoff, unlike the Members page) and a running timeout.
fn signals(member: Option<&MemberFacts>, now: DateTime<Utc>) -> Vec<Signal> {
    let Some(member) = member else { return Vec::new() };
    let dm = member.unusual_dm_until.map(Signal::UnusualDm);
    let timeout = member.timed_out_until.filter(|t| *t > now).map(Signal::TimedOut);
    dm.into_iter().chain(timeout).collect()
}

// The time left on a timeout: days and hours past a day, else a clock.
fn countdown(left: chrono::TimeDelta) -> String {
    let s = left.num_seconds().max(0);
    let (days, hours, minutes, seconds) = (s / 86_400, s % 86_400 / 3600, s % 3600 / 60, s % 60);
    match (days, hours) {
        (d, h) if d > 0 => format!("{d}d {h}h"),
        (_, h) if h > 0 => format!("{h}:{minutes:02}:{seconds:02}"),
        _ => format!("{minutes}:{seconds:02}"),
    }
}

// Not verified: Discord also passes users who are verified, phone-verified or staff; a member always has
// `joined_at`, so this reads true for every member.
fn passed_verification(member: &MemberFacts) -> bool {
    member.joined_at.is_some() || member.flags & BYPASSES_VERIFICATION != 0
}

fn agreed_to_rules(member: &MemberFacts) -> bool {
    member.flags & (BYPASSES_VERIFICATION | COMPLETED_ONBOARDING) != 0 || !member.pending
}

// Discord's join-source labels (`join_source_type`), with the glyph it leads with.
fn join_label(join: &JoinSource) -> (Option<Glyph>, String) {
    let code = join.code.clone();
    match join.kind {
        1 => (None, "Bot invite".into()),
        2 => (
            None,
            match join.integration_type.as_deref() {
                Some("twitch") => "Twitch".into(),
                Some("youtube") => "YouTube".into(),
                _ => "Integration".into(),
            },
        ),
        3 => (None, "Server discovery".into()),
        4 => (None, "Student hub".into()),
        5 => (Some(Glyph::Link), code.unwrap_or_else(|| "Invite".into())),
        6 => (Some(Glyph::Link), code.map(|c| format!("{c} (vanity)")).unwrap_or_else(|| "Vanity URL".into())),
        7 => (None, code.map(|c| format!("Manual verification ({c})")).unwrap_or_else(|| "Manual verification".into())),
        8 => (Some(Glyph::Game), "Linked channel".into()),
        _ => (None, "Unknown".into()),
    }
}

fn short_date<Tz: chrono::TimeZone>(t: &DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    t.format("%b %-d, %Y").to_string()
}

fn snowflake_date(id: &str) -> String {
    let ms = (id.parse::<u64>().unwrap_or(0) >> 22) + 1_420_070_400_000;
    DateTime::from_timestamp_millis(ms as i64).map(|t| short_date(&t.with_timezone(&chrono::Local))).unwrap_or_else(|| "Unknown".into())
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Count {
    Loading,
    Failed,
    Ready(u64),
}

impl Count {
    fn navigates(self) -> bool {
        matches!(self, Count::Ready(n) if n > 0)
    }
}

fn section(title: &'static str, cx: &App) -> Div {
    div().flex().flex_col().gap(px(2.)).child(group_label(title, cx))
}

// A line of a section (.mv-line): an optional glyph, the label, and its value on the right.
fn line(cx: &App) -> Div {
    div().min_h(px(32.)).px(px(6.)).flex().items_center().gap(px(8.)).squircle(ui::RADIUS_SM).text_color(palette::text(cx))
}

fn value_row(label: &'static str, value: impl IntoElement, cx: &App) -> Div {
    line(cx).child(div().flex_1().min_w_0().truncate().child(label)).child(div().flex_none().text_color(palette::muted(cx)).child(value))
}

fn signal_row(signal: &Signal, now: DateTime<Utc>, cx: &App) -> Div {
    let (glyph, label, value, ink) = match signal {
        Signal::UnusualDm(at) => (Glyph::ChatRound, "Unusual DM activity", crate::chat::relative(now.signed_duration_since(*at)), palette::muted(cx)),
        Signal::TimedOut(until) => (Glyph::Timer, "Timed out", countdown(until.signed_duration_since(now)), palette::danger_ink(cx)),
    };
    line(cx)
        .child(Icon::new(glyph).size(px(16.)).text_color(palette::muted(cx)))
        .child(div().flex_1().min_w_0().truncate().child(label))
        .child(div().flex_none().text_color(ink).font_features(crate::call::tabular()).child(value))
}

// A count that opens its list when there's something in it; a spinner-like wait while it loads.
fn count_row(id: SharedString, glyph: Glyph, label: &'static str, count: Count, cx: &App) -> gpui_kit::Stateful<Div> {
    let muted = palette::muted(cx);
    let value = match count {
        Count::Loading => div().child(ui::bars(SharedString::from(format!("{id}-wait")), muted, 12.)).into_any_element(),
        Count::Failed => div().text_color(palette::danger_ink(cx)).child("This is awkward").into_any_element(),
        Count::Ready(n) => div().font_features(crate::call::tabular()).child(grouped(n)).into_any_element(),
    };
    let hover = palette::hover(cx);
    line(cx)
        .id(id)
        .child(Icon::new(glyph).size(px(16.)).text_color(muted))
        .child(div().flex_1().min_w_0().truncate().child(label))
        .child(div().flex_none().flex().items_center().gap_1().text_color(muted).child(value).when(count.navigates(), |v| v.child(Icon::new(Glyph::ChevronRight).size(px(14.)))))
        .when(count.navigates(), |r| r.cursor_pointer().hover(move |s| s.bg(hover)))
}

fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn check_mark(ok: bool, cx: &App) -> Div {
    let (glyph, ink, label) = if ok { (Glyph::Checkmark, palette::positive_ink(cx), "Yes") } else { (Glyph::X, palette::danger_ink(cx), "No") };
    div().flex().items_center().gap_1().text_color(ink).child(Icon::new(glyph).size(px(16.))).child(label)
}

fn perm_pill(p: &Permission, elevated: bool, cx: &App) -> Div {
    ui::pill(cx).when(elevated, |d| d.child(Icon::new(Glyph::Shield).size(px(12.)).text_color(palette::warning_ink(cx)))).child(p.name)
}

fn permissions_page(view: &ModView, guild: &Guild, roles: &[String], cx: &App) -> AnyElement {
    if view.member.is_none() {
        return div().px_2().text_color(palette::muted(cx)).child("User is no longer a member of this server.").into_any_element();
    }
    let owner = guild.owner_id.as_deref() == Some(view.user_id.as_str());
    let bits = granted(guild, &view.user_id, roles);
    let cards = perms::GROUPS.iter().flat_map(|g| g.items.iter().map(move |p| (g.title, p))).filter(|(_, p)| bits & p.bit != 0).map(|(category, p)| {
        let by = granting(guild, roles, p.bit);
        let kind = if p.bit & ELEVATED != 0 { Some("Elevated") } else if p.bit & MODERATOR != 0 { Some("Moderator") } else { None };
        let count = if owner { "Server owner".to_owned() } else if by.len() == 1 { "1 role".to_owned() } else { format!("{} roles", by.len()) };
        let chips = by.into_iter().map(|r| {
            let color = if r.color == 0 { palette::status_in(Status::Offline, cx) } else { rgb(r.color).into() };
            ui::role_chip(r.name.clone(), color, cx)
        });
        let tag = kind.map(|k| {
            let tag = ui::tag(k, cx).id(SharedString::from(format!("mod-view-kind-{}", p.bit)));
            match k {
                "Elevated" => tag.tooltip(|window, cx| ui::tip("Elevated permissions grant higher levels of access to server settings, and should only be granted to highly trusted mods.", window, cx)).into_any_element(),
                _ => tag.into_any_element(),
            }
        });
        div()
            .p(px(12.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .squircle(ui::RADIUS_LG)
            .bg(palette::surface(palette::Level::Page, cx))
            .border_1()
            .border_color(palette::line(cx))
            .child(div().flex().items_center().gap_2().child(div().flex_1().min_w_0().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(p.name)).children(tag))
            .child(div().text_size(px(12.)).text_color(palette::muted(cx)).child(format!("{category} · {count}")))
            .child(div().text_color(palette::text(cx)).child(p.detail))
            .child(div().flex().flex_wrap().gap_1().children(chips))
    });
    div().flex().flex_col().gap_2().children(cards).into_any_element()
}

fn audit_page(view: &ModView, cx: &App) -> AnyElement {
    let (entries, names) = match &view.audit {
        None => return wait_line("mod-view-audit-wait", "Loading the action log…", cx).into_any_element(),
        Some(Err(error)) => return ui::callout(ui::Notice::Danger, Glyph::WarningCircle, "Couldn't load the action log", error.clone(), cx).into_any_element(),
        Some(Ok(page)) => page,
    };
    if entries.is_empty() {
        return div().px_2().text_color(palette::muted(cx)).child("No moderator actions on this member.").into_any_element();
    }
    let rows = entries.iter().map(|e| {
        let (glyph, ink) = match audit::tone(e.action) {
            audit::Tone::Create => (Glyph::PlusCircle, palette::status_in(Status::Online, cx)),
            audit::Tone::Update => (Glyph::Pen, palette::muted(cx)),
            audit::Tone::Delete => (Glyph::MinusBold, palette::danger_ink(cx)),
        };
        let meta = std::iter::once(snowflake_time(&e.id)).chain(e.reason.clone()).collect::<Vec<_>>().join(" · ");
        line(cx)
            .items_start()
            .py(px(6.))
            .child(div().pt(px(2.)).flex_none().text_color(ink).child(Icon::new(glyph).size(px(16.))))
            .child(div().flex_1().min_w_0().flex().flex_col().gap(px(2.)).child(sentence_spans(&audit::sentence(e, names), cx)).child(div().text_size(px(12.)).text_color(palette::muted(cx)).child(meta)))
    });
    div().flex().flex_col().gap(px(2.)).children(rows).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guild(roles: Vec<Role>) -> Guild {
        Guild {
            id: "g".into(),
            name: "Night Shift".into(),
            icon: None,
            channels: Vec::new(),
            roles,
            voice_states: Vec::new(),
            emojis: Vec::new(),
            stickers: Vec::new(),
            owner_id: Some("owner".into()),
            default_notifications: 0,
            threads: Vec::new(),
            my_roles: Vec::new(),
            premium_tier: 0,
            afk_channel_id: None,
            community: false,
        }
    }

    fn role(id: &str, position: i64, permissions: u64) -> Role {
        Role { id: id.into(), name: id.into(), color: 0, position, permissions, managed: false, icon: None }
    }

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn gate_opens_on_any_of_seven() {
        assert!(!can_open(0));
        assert!(!can_open(perm::MANAGE_MESSAGES | perm::MUTE_MEMBERS));
        for bit in [perm::ADMINISTRATOR, MANAGE_GUILD, perm::BAN_MEMBERS, perm::KICK_MEMBERS, perm::MODERATE_MEMBERS, perm::MANAGE_ROLES, perm::MANAGE_NICKNAMES] {
            assert!(can_open(bit));
        }
        // The owner's permissions come back as every bit.
        assert!(can_open(guild(Vec::new()).permissions("owner")));
    }

    #[test]
    fn mod_permissions_in_editor_order_with_elevated_marked() {
        let g = guild(vec![role("g", 0, 0), role("mod", 2, perm::MODERATE_MEMBERS | perm::MANAGE_MESSAGES | MENTION_EVERYONE | perm::SEND_MESSAGES)]);
        let held = mod_permissions(granted(&g, "u", &["mod".into()]));
        let names: Vec<_> = held.iter().map(|(p, e)| (p.name, *e)).collect();
        assert_eq!(names, vec![("Timeout members", true), ("Mention @everyone, @here and all roles", false), ("Manage messages", true)]);
        assert!(mod_permissions(granted(&g, "u", &[])).is_empty());
        assert_eq!(granted(&g, "owner", &[]), u64::MAX);
    }

    #[test]
    fn granting_counts_administrator_roles() {
        let g = guild(vec![role("g", 0, 0), role("admin", 3, perm::ADMINISTRATOR), role("mod", 2, perm::KICK_MEMBERS), role("fun", 1, 0)]);
        let by: Vec<_> = granting(&g, &["admin".into(), "mod".into(), "fun".into()], perm::KICK_MEMBERS).iter().map(|r| r.id.as_str()).collect();
        assert_eq!(by, vec!["admin", "mod"]);
    }

    #[test]
    fn countdown_shows_a_clock_under_a_day() {
        let s = chrono::TimeDelta::seconds;
        assert_eq!(countdown(s(42 * 60 + 10)), "42:10");
        assert_eq!(countdown(s(5)), "0:05");
        assert_eq!(countdown(s(3600 + 61)), "1:01:01");
        assert_eq!(countdown(s(6 * 86_400 + 23 * 3600 + 59)), "6d 23h");
        assert_eq!(countdown(s(-30)), "0:00");
    }

    #[test]
    fn signals_show_only_when_unusual_or_timed_out() {
        let now = at("2026-11-22T12:00:00Z");
        let member = MemberFacts { user_id: "u".into(), ..Default::default() };
        assert!(signals(Some(&member), now).is_empty());
        assert!(signals(None, now).is_empty());
        let lapsed = MemberFacts { timed_out_until: Some(at("2026-11-22T11:00:00Z")), ..member.clone() };
        assert!(signals(Some(&lapsed), now).is_empty());
        let flagged = MemberFacts { timed_out_until: Some(at("2026-11-22T13:00:00Z")), unusual_dm_until: Some(at("2026-11-19T12:00:00Z")), ..member };
        assert_eq!(signals(Some(&flagged), now), vec![Signal::UnusualDm(at("2026-11-19T12:00:00Z")), Signal::TimedOut(at("2026-11-22T13:00:00Z"))]);
    }

    #[test]
    fn account_rules_and_join_method() {
        let pending = MemberFacts { pending: true, ..Default::default() };
        assert!(!agreed_to_rules(&pending));
        assert!(agreed_to_rules(&MemberFacts { flags: COMPLETED_ONBOARDING, ..pending.clone() }));
        assert!(agreed_to_rules(&MemberFacts::default()));
        let body = json!([{ "user_id": "u", "join_source_type": 5, "source_invite_code": "nightshift", "inviter_id": "r" }, { "user_id": "x", "join_source_type": 3 }]);
        let join = parse_join(&body, "u").unwrap();
        let (glyph, label) = join_label(&join);
        assert!(matches!(glyph, Some(Glyph::Link)));
        assert_eq!(label, "nightshift");
        assert_eq!(join.inviter_id.as_deref(), Some("r"));
        assert_eq!(join_label(&parse_join(&body, "x").unwrap()).1, "Server discovery");
        assert!(parse_join(&body, "nobody").is_none());
        assert_eq!(join_label(&JoinSource { kind: 6, code: Some("rust".into()), inviter_id: None, integration_type: None }).1, "rust (vanity)");
    }

    #[test]
    fn counts_read_each_tab() {
        let body = json!({ "tabs": { "messages": { "total_results": 1284 }, "links": { "total_results": 37 }, "media": { "total_results": 112 } } });
        assert_eq!(parse_counts(&body), Counts { messages: 1284, links: 37, media: 112 });
        assert_eq!(grouped(1284), "1,284");
        assert_eq!(grouped(12), "12");
    }

    #[test]
    fn member_facts_read_the_wire() {
        let m = json!({ "user": { "id": "u" }, "joined_at": "2026-08-14T10:00:00.000000+00:00", "communication_disabled_until": "2026-11-22T13:00:00+00:00", "unusual_dm_activity_until": null, "flags": 2, "pending": false });
        let facts = MemberFacts::parse(&m).unwrap();
        assert_eq!(facts.timed_out_until, Some(at("2026-11-22T13:00:00Z")));
        assert_eq!(facts.joined_at, Some(at("2026-08-14T10:00:00Z")));
        assert_eq!((facts.unusual_dm_until, facts.flags), (None, 2));
    }
}
