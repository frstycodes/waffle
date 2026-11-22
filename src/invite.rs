// Invite friends to a server, Discord's dialog: friends to send the link to, the link, and its settings.
// Waffle opens the settings in place under the link, where Discord swaps to a second page, and says when
// the server's invites are paused, which Discord's dialog doesn't.
use std::{collections::HashMap, time::Duration};

use gpui_kit::{
    AnyElement, App, Context, Entity, FocusHandle, FontWeight, KeyDownEvent, MouseButton, SharedString, Subscription, Window,
    component::{
        Icon,
        input::{InputEvent, InputState as SearchState},
    },
    div,
    prelude::*,
    px,
};
use reqwest::Method;
use serde_json::json;

use crate::{
    Waffle,
    discord::{Channel, ChannelKind, Guild, Relation, Relationship, perm},
    icons::Glyph,
    layout::avatar_with_status,
    palette,
    profile::profile_key,
    ui::{self, Notice, RADIUS_LG, RADIUS_MD, Squircle as _, Tone},
};

const WIDTH: f32 = 480.;
// Friends on the left, the link and its settings on the right.
const WIDE: f32 = 820.;
// Rows listed at most, as Discord caps its suggestions.
const ROW_CAP: usize = 100;
// Four and a half rows; past that the list scrolls.
const LIST_HEIGHT: f32 = 360.;
const FACE: f32 = 32.;
const COPIED_FOR: Duration = Duration::from_secs(1);

// Discord's MAX_AGE_OPTIONS (seconds) and max-uses options, minus the experiment-only 14, 30 and 60 days.
const AGES: [(u32, &str); 7] = [(1800, "30 minutes"), (3600, "1 hour"), (21600, "6 hours"), (43200, "12 hours"), (86400, "1 day"), (604_800, "7 days"), (0, "Never")];
const USES: [(u32, &str); 7] = [(0, "No limit"), (1, "1 use"), (5, "5 uses"), (10, "10 uses"), (25, "25 uses"), (50, "50 uses"), (100, "100 uses")];

const FALLBACK_ERROR: &str = "Seems like something's gone wrong. Sorry about that.";
const TOO_MANY_INVITES: &str = "The maximum number of server invites have been generated. Admins can manage the list of invitations.";

pub(crate) struct Invite {
    guild_id: String,
    channel_id: String,
    // The link made for this dialog; None until the first one comes back.
    code: Option<String>,
    // The server's custom code, used while we may not create invites.
    vanity: Option<String>,
    settings: LinkSettings,
    // The settings being edited under the link; nothing changes until Generate.
    draft: Option<LinkSettings>,
    select: Option<Select>,
    creating: bool,
    error: Option<String>,
    // Keyed by row (`Row::key`): absent is Invite.
    sends: HashMap<String, Delivery>,
    // Bumped by each copy, so only the latest one's timer puts "Copy" back.
    copied: Option<u32>,
    copies: u32,
    search: Entity<SearchState>,
    friends_scroll: gpui_kit::ScrollHandle,
    focus: FocusHandle,
    _subscription: Subscription,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LinkSettings {
    max_age: u32,
    max_uses: u32,
    temporary: bool,
}

impl Default for LinkSettings {
    fn default() -> Self {
        Self { max_age: 604_800, max_uses: 0, temporary: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Select {
    Age,
    Uses,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Delivery {
    Sending,
    Sent,
}

// Where a row's invite goes: a DM or group DM we have, or a friend whose DM opens first.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Channel(String),
    User(String),
}

#[derive(Clone, Debug)]
struct Row {
    key: String,
    target: Target,
    name: String,
    // A person's id (for their avatar, status and username); None for a group DM.
    user_id: Option<String>,
    avatar: String,
    // A group DM's channel, for its faces and member count.
    group: Option<Channel>,
}

impl Waffle {
    pub(crate) fn open_invite(&mut self, guild_id: String, channel_id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        let me = self.my_id().to_owned();
        let Some(guild) = self.guilds.iter().find(|g| g.id == guild_id) else { return };
        let Some(channel) = target_channel(guild, &channel_id, &me) else { return self.invite_permission_required(cx) };
        let can_create = guild.channel_permissions(&me, channel) & perm::CREATE_INSTANT_INVITE != 0;
        let vanity = guild.vanity_url_code.clone().filter(|_| !can_create);
        if !can_create && vanity.is_none() {
            return self.invite_permission_required(cx);
        }
        let channel_id = channel.id.clone();
        let search = cx.new(|cx| SearchState::new(window, cx).placeholder("Search for friends"));
        let subscription = cx.subscribe_in(&search, window, |_, _, _: &InputEvent, _, cx| cx.notify());
        let focus = cx.focus_handle();
        match self.invite_rows("").is_empty() {
            true => window.focus(&focus, cx),
            false => search.update(cx, |state, cx| state.focus(window, cx)),
        }
        self.invite = Some(Invite {
            guild_id,
            channel_id,
            code: None,
            vanity,
            settings: LinkSettings::default(),
            draft: None,
            select: None,
            creating: false,
            error: None,
            sends: HashMap::new(),
            copied: None,
            copies: 0,
            search,
            friends_scroll: gpui_kit::ScrollHandle::new(),
            focus,
            _subscription: subscription,
        });
        if can_create {
            self.create_invite(LinkSettings::default(), cx);
        }
        cx.notify();
    }

    // Discord's answer for someone who may not invite and has no custom link to hand out.
    fn invite_permission_required(&mut self, cx: &mut Context<Self>) {
        self.confirm("Invite permission required", "To add a friend to this server, ask someone with invite permissions for help.", "Got It", |_, _, _| {}, cx);
        if let Some(dialog) = &mut self.dialog {
            dialog.danger = false;
        }
    }

    fn close_invite(&mut self, cx: &mut Context<Self>) {
        self.invite = None;
        cx.notify();
    }

    // Discord sends `validate` (the last code for the channel) on open so the server can hand it back; we
    // keep no codes between dialogs, so each open makes a fresh link.
    fn create_invite(&mut self, settings: LinkSettings, cx: &mut Context<Self>) {
        let Some(invite) = self.invite.as_mut() else { return };
        if invite.vanity.is_some() && settings == LinkSettings::NEVER {
            (invite.code, invite.settings, invite.draft, invite.error) = (None, settings, None, None);
            return cx.notify();
        }
        let Some(client) = self.client.clone() else { return };
        (invite.creating, invite.error) = (true, None);
        let channel_id = invite.channel_id.clone();
        let body = json!({ "max_age": settings.max_age, "max_uses": settings.max_uses, "temporary": settings.temporary });
        cx.spawn(async move |this, cx| {
            let result = client.call(Method::POST, format!("/channels/{channel_id}/invites"), Some(body)).await;
            this.update(cx, |this, cx| {
                let Some(invite) = this.invite.as_mut().filter(|i| i.channel_id == channel_id) else { return };
                invite.creating = false;
                match result.map(|v| v["code"].as_str().map(str::to_owned)) {
                    Ok(Some(code)) => (invite.code, invite.vanity, invite.settings, invite.draft) = (Some(code), None, settings, None),
                    Ok(None) => invite.error = Some(FALLBACK_ERROR.into()),
                    Err(error) => invite.error = Some(invite_error(&error)),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    // Opens the friend's DM first when there isn't one, then posts the link as a plain message.
    fn send_invite(&mut self, row: Row, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let Some(invite) = self.invite.as_mut() else { return };
        let Some(url) = invite.url() else { return };
        invite.sends.insert(row.key.clone(), Delivery::Sending);
        let channel_id = invite.channel_id.clone();
        let known = match &row.target {
            Target::Channel(id) => Some(id.clone()),
            Target::User(user) => self.dms.iter().find(|d| d.recipient.as_ref().is_some_and(|r| &r.id == user)).map(|d| d.id.clone()),
        };
        cx.spawn(async move |this, cx| {
            let result = async {
                let (dm, opened) = match (known, &row.target) {
                    (Some(id), _) => (id, None),
                    (None, Target::User(user)) => {
                        let dm = client.open_dm(user).await?;
                        (dm.id.clone(), Some(dm))
                    }
                    (None, Target::Channel(id)) => (id.clone(), None),
                };
                client.send_message(&dm, &url, &crate::nonce(), None, false).await?;
                Ok::<_, String>(opened)
            }
            .await;
            this.update(cx, |this, cx| {
                if let Ok(Some(dm)) = &result
                    && !this.dms.iter().any(|d| d.id == dm.id)
                {
                    this.dms.insert(0, dm.clone());
                }
                let Some(invite) = this.invite.as_mut().filter(|i| i.channel_id == channel_id) else { return };
                match result {
                    Ok(_) => invite.sends.insert(row.key.clone(), Delivery::Sent),
                    Err(_) => invite.sends.remove(&row.key),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn copy_invite(&mut self, cx: &mut Context<Self>) {
        let Some(invite) = self.invite.as_mut() else { return };
        let Some(url) = invite.url() else { return };
        invite.copies += 1;
        let copy = invite.copies;
        invite.copied = Some(copy);
        self.copy_text(url, cx);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FOR).await;
            this.update(cx, |this, cx| {
                if let Some(invite) = this.invite.as_mut().filter(|i| i.copied == Some(copy)) {
                    invite.copied = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn edit_invite(&mut self, edit: impl FnOnce(&mut Invite), cx: &mut Context<Self>) {
        if let Some(invite) = self.invite.as_mut() {
            edit(invite);
        }
        cx.notify();
    }

    // DMs with messages (newest first), then group DMs, then the remaining friends; bots and blocked people left out.
    fn invite_rows(&self, query: &str) -> Vec<Row> {
        let mut rows = invite_rows(&self.dms, &self.relationships);
        let query = query.trim().to_lowercase();
        if !query.is_empty() {
            rows.retain(|r| r.name.to_lowercase().contains(&query) || self.username(r).is_some_and(|u| u.to_lowercase().contains(&query)));
        }
        rows.truncate(ROW_CAP);
        rows
    }

    // A person's handle, once their profile has been read; `User` keeps only the display name.
    fn username(&self, row: &Row) -> Option<String> {
        let id = row.user_id.as_deref()?;
        self.profiles.peek(&profile_key(id, None)).map(|p| p.username).filter(|u| !u.is_empty())
    }

    pub(crate) fn render_invite(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let invite = self.invite.as_ref()?;
        let guild = self.guilds.iter().find(|g| g.id == invite.guild_id)?;
        let channel = guild.channel(&invite.channel_id)?;
        let paused = guild.has_feature("INVITES_DISABLED");
        let friends = self.friends_section(invite, paused, cx);
        // The wide layout has room for the link's settings, so they show without Edit invite link.
        let always = friends.is_some() && invite.url().is_some();
        let draft = invite.draft.or(always.then_some(invite.settings));
        let link = div().flex().flex_col().gap(px(20.)).child(link_section(invite, paused, !always, cx)).children(draft.map(|draft| settings_panel(invite, draft, always, window, cx)));
        let (columns, width) = match friends {
            Some(friends) => (div().flex().items_start().gap(px(32.)).child(div().flex_1().min_w_0().child(friends)).child(link.flex_1().min_w_0()), WIDE),
            None => (link, WIDTH),
        };
        let body = ui::dialog_body()
            .gap(px(20.))
            .track_focus(&invite.focus)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if e.keystroke.key == "escape" {
                    this.close_invite(cx);
                }
            }))
            .child(invite_head(guild, channel, cx))
            .when(channel.private, |d| d.child(private_warning(cx)))
            .when(paused, |d| d.child(ui::callout(Notice::Accent, Glyph::Pause, "Invites are paused for this server.", "Nobody can join with a link until they're resumed in Server settings.", cx)))
            .child(columns);
        let backdrop = div().id("invite-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_invite(cx)));
        Some(ui::modal(backdrop, ui::dialog("invite", width, body, window, cx), cx))
    }

    // The search and the rows; nothing at all when there's nobody to invite.
    fn friends_section(&self, invite: &Invite, paused: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.invite_rows("").is_empty() {
            return None;
        }
        let query = invite.search.read(cx).value().to_string();
        let rows = self.invite_rows(&query);
        let can_send = !paused && invite.url().is_some();
        let list: AnyElement = match rows.is_empty() {
            true => div().py(px(16.)).text_center().text_color(palette::muted(cx)).child("No results found").into_any_element(),
            false => {
                let items: Vec<AnyElement> = rows.into_iter().map(|row| self.invite_friend_row(row, invite, can_send, cx)).collect();
                let list = div().id("invite-friends").flex().flex_col().gap(px(2.)).max_h(px(LIST_HEIGHT)).overflow_y_scroll().track_scroll(&invite.friends_scroll).children(items);
                div().relative().child(list).child(ui::more_below(&invite.friends_scroll, palette::surface(palette::Level::Raised, cx))).into_any_element()
            }
        };
        Some(div().flex().flex_col().gap(px(8.)).child(ui::search_field("invite-search", &invite.search, cx)).child(list).into_any_element())
    }

    fn invite_friend_row(&self, row: Row, invite: &Invite, can_send: bool, cx: &mut Context<Self>) -> AnyElement {
        let surface = palette::surface(palette::Level::Raised, cx);
        let face = match &row.group {
            Some(group) => ui::dm_face(&group.face(), FACE, surface),
            None => avatar_with_status(&row.avatar, FACE, row.user_id.as_ref().and_then(|id| self.presences.get(id)).map(|p| p.mark()), surface),
        };
        let detail = match &row.group {
            Some(group) => Some(members(group.recipients.len() + 1)),
            None => self.username(&row),
        };
        let text = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(ui::truncating(format!("invite-name-{}", row.key), row.name.clone()).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)))
            .children(detail.map(|d| div().truncate().text_size(px(12.)).text_color(palette::muted(cx)).child(d)));
        let send = invite.sends.get(&row.key).copied();
        let button = invite_button(row, send, can_send, cx);
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(8.))
            .py(px(6.))
            .squircle(RADIUS_MD)
            .child(face)
            .child(text)
            .child(button)
            .into_any_element()
    }
}

impl Invite {
    fn url(&self) -> Option<String> {
        self.code.as_deref().or(self.vanity.as_deref()).map(|code| format!("https://discord.gg/{code}"))
    }
}

impl LinkSettings {
    const NEVER: Self = Self { max_age: 0, max_uses: 0, temporary: false };
}

// The channel recipients land in: the one asked for (a thread's parent) if we can see it, else the
// first we may invite from.
fn target_channel<'a>(guild: &'a Guild, channel_id: &str, me: &str) -> Option<&'a Channel> {
    let asked = guild.channel(channel_id).map(|c| match (&c.thread, &c.parent_id) {
        (Some(_), Some(parent)) => guild.channel(parent).unwrap_or(c),
        _ => c,
    });
    asked.filter(|c| c.kind != ChannelKind::Category && guild.can_view(me, c)).or_else(|| guild.invite_channel(me))
}

fn invite_rows(dms: &[Channel], relationships: &[Relationship]) -> Vec<Row> {
    let blocked = |id: &str| relationships.iter().any(|r| r.user.id == id && r.kind == Relation::Blocked);
    let newest = |c: &Channel| std::cmp::Reverse(c.last_message_id.as_deref().and_then(|id| id.parse::<u64>().ok()).unwrap_or(0));
    let mut people: Vec<&Channel> = dms.iter().filter(|c| c.last_message_id.is_some() && c.recipient.as_ref().is_some_and(|u| !u.bot && !blocked(&u.id))).collect();
    people.sort_by_key(|c| newest(c));
    let mut groups: Vec<&Channel> = dms.iter().filter(|c| c.kind == ChannelKind::Dm && c.recipient.is_none() && !c.recipients.is_empty()).collect();
    groups.sort_by_key(|c| newest(c));
    let mut rows: Vec<Row> = people
        .into_iter()
        .filter_map(|c| {
            let user = c.recipient.as_ref()?;
            Some(Row { key: user.id.clone(), target: Target::Channel(c.id.clone()), name: user.name.clone(), user_id: Some(user.id.clone()), avatar: user.avatar.clone(), group: None })
        })
        .chain(groups.into_iter().map(|c| Row { key: c.id.clone(), target: Target::Channel(c.id.clone()), name: c.name.clone(), user_id: None, avatar: String::new(), group: Some(c.clone()) }))
        .collect();
    let friends: Vec<Row> = relationships
        .iter()
        .filter(|r| r.kind == Relation::Friend && !r.user.bot && !rows.iter().any(|row| row.key == r.user.id))
        .map(|r| Row { key: r.user.id.clone(), target: Target::User(r.user.id.clone()), name: r.user.name.clone(), user_id: Some(r.user.id.clone()), avatar: r.user.avatar.clone(), group: None })
        .collect();
    rows.extend(friends);
    rows
}

// "Invite friends to Night Owls", then the channel they'll land in with its glyph.
fn invite_head(guild: &Guild, channel: &Channel, cx: &mut Context<Waffle>) -> AnyElement {
    let (strong, muted) = (palette::strong(cx), palette::muted(cx));
    let land = div()
        .flex()
        .items_center()
        .gap(px(4.))
        .min_w_0()
        .text_size(px(14.))
        .text_color(muted)
        .child(crate::sidebar::channel_glyph(channel).size(px(16.)))
        .child(div().flex_none().child("Recipients will land in"))
        .child(div().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).text_color(strong).child(channel.name.clone()));
    div()
        .flex()
        .items_start()
        .gap(px(12.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(div().text_size(px(20.)).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD).text_color(strong).child(format!("Invite friends to {}", guild.name)))
                .child(land),
        )
        .child(ui::icon_button("invite-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.close_invite(cx))))
        .into_any_element()
}

fn private_warning(cx: &App) -> AnyElement {
    div()
        .flex()
        .items_start()
        .gap(px(8.))
        .text_size(px(14.))
        .text_color(palette::text(cx))
        .child(div().flex_none().mt(px(1.)).text_color(palette::warning_ink(cx)).child(Icon::new(Glyph::WarningCircle).size(px(18.))))
        .child(div().flex_1().min_w_0().child("This channel is private, only select members and roles can view this channel."))
        .into_any_element()
}

// Invite, a wave while it sends, then Sent with a check; a failed send comes back as Invite.
fn invite_button(row: Row, send: Option<Delivery>, can_send: bool, cx: &mut Context<Waffle>) -> AnyElement {
    let button = ui::button(SharedString::from(format!("invite-send-{}", row.key)), Tone::Secondary, cx).small();
    match send {
        Some(Delivery::Sent) => button.disabled(true).child(Icon::new(Glyph::Checkmark).size(px(16.))).child("Sent").into_any_element(),
        Some(Delivery::Sending) => button.loading(Some("Invite")).into_any_element(),
        None if !can_send => button.disabled(true).child("Invite").into_any_element(),
        None => button.child("Invite").on_click(cx.listener(move |this, _, _, cx| this.send_invite(row.clone(), cx))).into_any_element(),
    }
}

// The link to copy, the line saying how long it lasts with Edit invite link, and what went wrong.
fn link_section(invite: &Invite, paused: bool, editable: bool, cx: &mut Context<Waffle>) -> AnyElement {
    let url = invite.url();
    let (strong, faint) = (palette::strong(cx), palette::faint(cx));
    let value: AnyElement = match &url {
        Some(url) => div().flex_1().min_w_0().truncate().text_color(if paused { faint } else { strong }).child(url.clone()).into_any_element(),
        None => div().flex_1().min_w_0().child(ui::skeleton("invite-link-loading", ui::RADIUS_XS, cx).h(px(14.)).w(px(180.))).into_any_element(),
    };
    let copy_off = paused || url.is_none();
    let copy = ui::button("invite-copy", Tone::Primary, cx)
        .small()
        .disabled(copy_off)
        .child(if invite.copied.is_some() { "Copied" } else { "Copy" })
        .when(!copy_off, |b| b.on_click(cx.listener(|this, _, _, cx| this.copy_invite(cx))));
    let field = ui::field_frame(cx).pr(px(5.)).text_size(px(14.)).when(paused, |d| d.bg(palette::selected(cx))).child(value).child(copy);
    let line = match (&invite.code, &invite.vanity) {
        (None, Some(_)) => "Using this server's custom invite link.".to_owned(),
        _ => expiry_line(invite.settings.max_age, invite.settings.max_uses),
    };
    let edit = div()
        .id("invite-edit")
        .flex_none()
        .cursor_pointer()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette::accent_ink(cx))
        .hover(|s| s.underline())
        .child("Edit invite link")
        .on_click(cx.listener(|this, _, _, cx| this.edit_invite(|i| (i.draft, i.select) = (Some(i.settings), None), cx)));
    let hint = div().flex().flex_wrap().gap_x(px(4.)).text_size(px(13.)).line_height(px(18.)).text_color(palette::muted(cx)).child(line).when(url.is_some() && editable, |d| d.child(edit));
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(strong).child("Or, send a server invite link to a friend"))
        .child(field)
        .when(invite.draft.is_none() || !editable, |d| d.child(hint))
        .children(invite.error.clone().map(|e| ui::field_error(e, cx)))
        .into_any_element()
}

// The link's settings under it: a draft until Generate a new link. Opened by Edit invite link, or `always`
// shown in the wide layout, where Cancel appears only once the draft differs from the live link.
fn settings_panel(invite: &Invite, draft: LinkSettings, always: bool, window: &mut Window, cx: &mut Context<Waffle>) -> AnyElement {
    let cancellable = !always || draft != invite.settings;
    let age = settings_select(Select::Age, &AGES, draft.max_age, invite.select, cx);
    let uses = settings_select(Select::Uses, &USES, draft.max_uses, invite.select, cx);
    let temporary = ui::toggle_row(
        "invite-temporary",
        "Grant temporary membership",
        Some("Temporary members are automatically kicked when they disconnect unless a role has been assigned.".into()),
        draft.temporary,
        window,
        cx,
    )
    .on_click(cx.listener(|this, _, _, cx| {
        this.edit_invite(
            |i| {
                let d = i.draft.get_or_insert(i.settings);
                d.temporary = !d.temporary;
            },
            cx,
        )
    }));
    let actions = div()
        .flex()
        .justify_end()
        .gap(px(8.))
        .when(cancellable, |d| d.child(ui::button("invite-settings-cancel", Tone::Secondary, cx).child("Cancel").on_click(cx.listener(|this, _, _, cx| this.edit_invite(|i| (i.draft, i.select) = (None, None), cx)))))
        .child(
            ui::button("invite-generate", Tone::Primary, cx)
                .loading(invite.creating.then_some("Generating"))
                .child("Generate a new link")
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.invite.as_ref().is_some_and(|i| !i.creating) {
                        this.create_invite(draft, cx);
                    }
                })),
        );
    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .p(px(16.))
        .squircle(RADIUS_LG)
        .bg(palette::hover(cx))
        .child(div().flex().gap(px(12.)).child(div().flex_1().min_w_0().child(ui::field("Expire after", cx).child(age))).child(div().flex_1().min_w_0().child(ui::field("Max number of uses", cx).child(uses))))
        .child(temporary.mx(px(-12.)))
        .child(actions)
        .into_any_element()
}

// One of the two dropdowns: its trigger, and its list rising from it while open.
fn settings_select(select: Select, options: &[(u32, &str)], chosen: u32, open: Option<Select>, cx: &mut Context<Waffle>) -> AnyElement {
    let open = open == Some(select);
    let label = options.iter().find(|(v, _)| *v == chosen).map_or("", |(_, l)| l);
    let trigger = ui::select(SharedString::from(format!("invite-select-{select:?}")), None, label.to_owned(), open, cx)
        .on_click(cx.listener(move |this, _, _, cx| this.edit_invite(|i| i.select = if i.select == Some(select) { None } else { Some(select) }, cx)));
    let list = open.then(|| {
        let options: Vec<(String, String)> = options.iter().map(|(v, l)| (v.to_string(), (*l).to_owned())).collect();
        let (pick, close) = (cx.entity(), cx.entity());
        let dismiss = move |_: &mut Window, cx: &mut App| close.update(cx, |this, cx| this.edit_invite(|i| i.select = None, cx));
        let on_choose = move |value: String, _: &mut Window, cx: &mut App| {
            let value = value.parse().unwrap_or(0);
            pick.update(cx, |this, cx| {
                this.edit_invite(
                    |i| {
                        let d = i.draft.get_or_insert(i.settings);
                        match select {
                            Select::Age => d.max_age = value,
                            Select::Uses => d.max_uses = value,
                        }
                        i.select = None;
                    },
                    cx,
                )
            })
        };
        ui::select_options(SharedString::from(format!("invite-options-{select:?}")), &options, &chosen.to_string(), 40., on_choose, dismiss, cx)
    });
    div().relative().w_full().child(trigger).children(list).into_any_element()
}

fn members(count: usize) -> String {
    count_of(count as u32, "member", "members")
}

fn count_of(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

// Discord's expiry line under the link (module 735547 `Be(maxAge, maxUses)`).
fn expiry_line(max_age: u32, max_uses: u32) -> String {
    let uses = count_of(max_uses, "use", "uses");
    if max_age == 0 {
        return match max_uses {
            0 => "Your invite link will never expire.".into(),
            _ => format!("Your invite link expires after {uses}."),
        };
    }
    let span = match max_age {
        a if a < 3600 => count_of(a / 60, "minute", "minutes"),
        a if a < 86400 => count_of(a / 3600, "hour", "hours"),
        a => count_of(a / 86400, "day", "days"),
    };
    match max_uses {
        0 => format!("Your invite link expires in {span}."),
        _ => format!("Your invite link expires in {span}, or after {uses}."),
    }
}

// Discord's words for TOO_MANY_INVITES (30016), else its own message.
// Not verified: the error text for 30016 ("Maximum number of invites reached"); `send` keeps only the
// message, not the code, so the text is what's matched.
fn invite_error(error: &str) -> String {
    if error.starts_with("Maximum number of invites") {
        return TOO_MANY_INVITES.into();
    }
    if error.trim().is_empty() {
        return FALLBACK_ERROR.into();
    }
    error.to_owned()
}

pub(crate) fn demo(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    demo_friends(waffle);
    if let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") {
        match scene {
            "invite-vanity" => {
                (guild.owner_id, guild.vanity_url_code) = (Some("0".into()), Some("nightowls".into()));
                let everyone = guild.id.clone();
                guild.roles.iter_mut().filter(|r| r.id == everyone).for_each(|r| r.permissions = crate::discord::VIEW_CHANNEL);
            }
            "invite-paused" => guild.features.push("INVITES_DISABLED".into()),
            _ => {}
        }
    }
    waffle.open_invite("1".into(), "general".into(), window, cx);
    let keys: Vec<String> = waffle.invite_rows("").into_iter().take(2).map(|r| r.key).collect();
    let Some(invite) = waffle.invite.as_mut() else { return };
    if invite.vanity.is_none() {
        invite.code = Some("hTKzmak".into());
    }
    match scene {
        "invite-sent" => invite.sends.extend(keys.into_iter().map(|k| (k, Delivery::Sent))),
        "invite-settings" => invite.draft = Some(LinkSettings { max_age: 86400, max_uses: 5, ..invite.settings }),
        _ => {}
    }
}

// The fixtures hold one DM and two friends: add DMs with history, a group, a few more friends (one
// with a very long name) and their usernames.
fn demo_friends(waffle: &mut Waffle) {
    use crate::discord::{Profile, User, avatar_url};
    let person = |id: &str, name: &str| User { id: id.into(), name: name.into(), avatar: avatar_url(id, None), ..Default::default() };
    let dm = |id: &str, user: User, last: &str| Channel {
        id: id.into(),
        name: user.name.clone(),
        kind: ChannelKind::Dm,
        parent_id: None,
        position: 0,
        topic: None,
        last_message_id: Some(last.into()),
        private: false,
        recipient: Some(user.clone()),
        recipients: vec![user],
        icon: None,
        thread: None,
        raw_type: 1,
        overwrites: Vec::new(),
        props: Default::default(),
    };
    let friends = [
        ("29360128", "Juniper Vale"),
        ("33554432", "Bartholomew Featherstonehaugh-Montgomery the Third of Lower Wensleydale"),
        ("37748736", "pixel"),
        ("41943040", "Rosa"),
        ("46137344", "Tsubasa"),
        ("50331648", "Ines"),
        ("54525952", "Mochi"),
        ("58720256", "Odette"),
        ("62914560", "Kofi"),
        ("67108864", "Lark"),
    ];
    for (id, name) in friends {
        waffle.relationships.push(Relationship { user: person(id, name), kind: Relation::Friend });
    }
    if let Some(wren) = waffle.dms.iter_mut().find(|d| d.id == "dm") {
        wren.last_message_id = Some("950".into());
    }
    waffle.dms.push(dm("dm-rosa", person("41943040", "Rosa"), "990"));
    let mut group = dm("group", person("37748736", "pixel"), "980");
    (group.name, group.recipient, group.raw_type) = ("weekend raid".into(), None, 3);
    group.recipients.push(person("29360128", "Juniper Vale"));
    waffle.dms.push(group);
    let usernames = [("8388608", "wren"), ("12582912", "mosslantern"), ("29360128", "juniper.vale"), ("33554432", "bart"), ("37748736", "pixel_"), ("41943040", "rosa.m")];
    for (id, username) in usernames {
        waffle.profiles.seed(&profile_key(id, None), Profile { username: username.into(), ..Default::default() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_line_reads_like_discord() {
        assert_eq!(expiry_line(1800, 0), "Your invite link expires in 30 minutes.");
        assert_eq!(expiry_line(3600, 0), "Your invite link expires in 1 hour.");
        assert_eq!(expiry_line(21600, 1), "Your invite link expires in 6 hours, or after 1 use.");
        assert_eq!(expiry_line(86400, 5), "Your invite link expires in 1 day, or after 5 uses.");
        assert_eq!(expiry_line(604_800, 0), "Your invite link expires in 7 days.");
        assert_eq!(expiry_line(0, 0), "Your invite link will never expire.");
        assert_eq!(expiry_line(0, 1), "Your invite link expires after 1 use.");
        assert_eq!(expiry_line(0, 100), "Your invite link expires after 100 uses.");
    }

    #[test]
    fn too_many_invites_reads_as_discord_words_it() {
        assert_eq!(invite_error("Maximum number of invites reached (1000)"), TOO_MANY_INVITES);
        assert_eq!(invite_error("Missing Permissions"), "Missing Permissions");
        assert_eq!(invite_error(""), FALLBACK_ERROR);
    }

    #[test]
    fn rows_put_recent_dms_then_groups_then_friends() {
        use crate::discord::User;
        let user = |id: &str| User { id: id.into(), name: id.into(), ..Default::default() };
        let dm = |id: &str, who: Option<&str>, last: Option<&str>| Channel {
            id: id.into(),
            name: id.into(),
            kind: ChannelKind::Dm,
            parent_id: None,
            position: 0,
            topic: None,
            last_message_id: last.map(Into::into),
            private: false,
            recipient: who.map(user),
            recipients: vec![user("x")],
            icon: None,
            thread: None,
            raw_type: 1,
            overwrites: Vec::new(),
            props: Default::default(),
        };
        let dms = [dm("d-old", Some("a"), Some("5")), dm("d-new", Some("b"), Some("9")), dm("d-quiet", Some("c"), None), dm("g", None, Some("1"))];
        let rel = |id: &str, kind| Relationship { user: user(id), kind };
        let relationships = [rel("a", Relation::Friend), rel("c", Relation::Friend), rel("d", Relation::Friend), rel("e", Relation::Blocked)];
        let keys: Vec<String> = invite_rows(&dms, &relationships).into_iter().map(|r| r.key).collect();
        assert_eq!(keys, ["b", "a", "g", "c", "d"]);
    }
}
