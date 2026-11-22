// Create Channel, Discord's dialog: the type, the name with the type's glyph in front, and Private channel,
// which hides it from @everyone. A private channel, or a Stage, then asks for its members and roles (its
// moderators) in a second step, as Discord does. Opened from a category's + (in that category) or the server menu (uncategorised). Copy, gating, normalization and overwrites follow
// docs/discord-create-channel-re.md.
use std::{cmp::Reverse, collections::HashSet};

use gpui_kit::{
    AnyElement, AppContext as _, Context, Entity, FontWeight, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement, SharedString, StatefulInteractiveElement, Styled, Subscription, Window,
    component::{
        ActiveTheme as _, Icon,
        input::{Input, InputEvent, InputState as SearchState},
    },
    div,
    prelude::FluentBuilder as _,
    px,
};
use reqwest::Method;
use serde_json::{Value, json};

use crate::{
    Waffle,
    discord::{Guild, ListItem, User, VIEW_CHANNEL, perm},
    icons::Glyph,
    palette,
    sidebar::Nav,
    ui::{self, Check, Squircle as _, Tone},
};

// Discord's API channel types.
const TEXT: u64 = 0;
const VOICE: u64 = 2;
const ANNOUNCEMENT: u64 = 5;
const STAGE: u64 = 13;
const FORUM: u64 = 15;
const MEDIA: u64 = 16;

// In Discord's order; `offered_types` drops the ones the server doesn't allow.
const TYPES: [(u64, &str, &str); 6] = [
    (TEXT, "Text", "Send messages, images, GIFs, emoji, opinions, and puns"),
    (VOICE, "Voice", "Hang out together with voice, video, and screen share"),
    (FORUM, "Forum", "Create a space for organized discussions"),
    (MEDIA, "Media", "Share videos, photos, and files with your members."),
    (ANNOUNCEMENT, "Announcement", "Important updates for people in and out of the server"),
    (STAGE, "Stage", "Host events, panels, and Q&As for an audience"),
];

// Discord's `maxLength`. Not verified: it counts UTF-16 units; this counts chars.
const NAME_MAX: usize = 100;
const STAGE_MODERATOR: u64 = perm::MANAGE_CHANNELS | perm::MUTE_MEMBERS | perm::MOVE_MEMBERS;
// GAP: rows past this many members aren't listed; searching narrows to the rest.
const MEMBER_CAP: usize = 100;
const ACCESS_LIST_HEIGHT: f32 = 320.;

pub(crate) struct CreateChannel {
    guild_id: String,
    parent: Option<String>,
    category: Option<String>,
    kind: u64,
    private: bool,
    picked: Vec<Pick>,
    // On the second step: who may see it, or Stage's moderators.
    choosing_access: bool,
    search: Entity<SearchState>,
    _subscriptions: [Subscription; 2],
}

// Overwrite target types on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Role = 0,
    Member = 1,
}

#[derive(Clone, Debug, PartialEq)]
struct Pick {
    id: String,
    target: Target,
    name: String,
}

// A row in the access picker.
struct Candidate {
    pick: Pick,
    mark: Mark,
    detail: SharedString,
}

enum Mark {
    Role(u32),
    Avatar(String),
}

// Who is creating the channel, for whether they need an overwrite of their own to see it.
struct Creator<'a> {
    id: &'a str,
    roles: &'a [String],
    // Owner or Administrator: sees every channel anyway.
    elevated: bool,
}

impl Waffle {
    pub(crate) fn open_create_channel(&mut self, guild_id: String, kind: u64, parent: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(guild) = self.guilds.iter().find(|g| g.id == guild_id) else { return };
        let category = parent.as_ref().and_then(|p| guild.channels.iter().find(|c| &c.id == p)).map(|c| c.name.clone());
        let kind = if offered_types(guild, self.my_id()).contains(&kind) { kind } else { TEXT };
        self.menu = None;
        self.dialog_input.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.set_placeholder(placeholder(kind), window, cx);
            state.focus(window, cx);
        });
        let search = cx.new(|cx| SearchState::new(window, cx).placeholder("Search members and roles"));
        let subscriptions = [
            cx.subscribe_in(&self.dialog_input, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.shape_channel_name(window, cx);
                }
            }),
            cx.subscribe_in(&search, window, |_, _, _: &InputEvent, _, cx| cx.notify()),
        ];
        self.create_channel = Some(CreateChannel { guild_id, parent, category, kind, private: false, picked: Vec::new(), choosing_access: false, search, _subscriptions: subscriptions });
        cx.notify();
    }

    fn close_create_channel(&mut self, cx: &mut Context<Self>) {
        self.create_channel = None;
        cx.notify();
    }

    // Normalizes the name as typed, as Discord does on every keystroke.
    fn shape_channel_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(kind) = self.create_channel.as_ref().map(|f| f.kind) else { return };
        self.dialog_input.update(cx, |state, cx| reshape(state, kind, window, cx));
        cx.notify();
    }

    fn set_channel_kind(&mut self, kind: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.create_channel.as_mut() else { return };
        form.kind = kind;
        if !allows_private(kind) {
            form.private = false;
        }
        self.dialog_input.update(cx, |state, cx| {
            state.set_placeholder(placeholder(kind), window, cx);
            reshape(state, kind, window, cx);
        });
        cx.notify();
    }

    fn toggle_create_private(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.create_channel.as_mut() else { return };
        form.private = !form.private;
        let to_text = form.private && form.kind == ANNOUNCEMENT;
        if to_text {
            return self.set_channel_kind(TEXT, window, cx);
        }
        cx.notify();
    }

    fn toggle_access(&mut self, pick: Pick, cx: &mut Context<Self>) {
        let Some(form) = self.create_channel.as_mut() else { return };
        match form.picked.iter().position(|p| p.id == pick.id) {
            Some(at) => _ = form.picked.remove(at),
            None => form.picked.push(pick),
        }
        cx.notify();
    }

    // The first step's primary action: Next into the access step for a private channel or a Stage,
    // otherwise create.
    pub(crate) fn advance_create_channel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let named = !self.dialog_input.read(cx).value().trim().is_empty();
        let Some(form) = self.create_channel.as_mut().filter(|_| named) else { return };
        if form.choosing_access || !needs_access_step(form) {
            return self.submit_create_channel(cx);
        }
        form.choosing_access = true;
        form.search.update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    fn back_to_channel_info(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.create_channel.as_mut() else { return };
        form.choosing_access = false;
        self.dialog_input.update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    pub(crate) fn submit_create_channel(&mut self, cx: &mut Context<Self>) {
        let name = self.dialog_input.read(cx).value().trim().to_owned();
        let Some(form) = self.create_channel.as_ref().filter(|_| !name.is_empty()) else { return };
        let Some(guild) = self.guilds.iter().find(|g| g.id == form.guild_id) else { return };
        let me = self.my_id();
        let creator = Creator { id: me, roles: &guild.my_roles, elevated: guild.permissions(me) == u64::MAX };
        let body = create_body(&name, form.kind, form.private, form.parent.as_deref(), &guild.id, &form.picked, &creator);
        let path = format!("/guilds/{}/channels", form.guild_id);
        self.create_channel = None;
        self.rest(Method::POST, path, Some(body), |_, _| {}, cx);
    }

    // Roles you can hand access to (not @everyone, not a bot's), highest first, then the members Waffle
    // has seen here, by name. GAP: `User` keeps only the display name, so a member's line says "Member"
    // where Discord shows the username.
    fn access_candidates(&self, guild: &Guild, query: &str) -> Vec<Candidate> {
        let query = query.trim().to_lowercase();
        let hit = |name: &str| query.is_empty() || name.to_lowercase().contains(&query);
        let me = self.my_id();
        let mut roles: Vec<_> = guild.roles.iter().filter(|r| r.id != guild.id && !r.managed && hit(&r.name)).collect();
        roles.sort_by_key(|r| Reverse(r.position));
        let listed = self.member_lists.get(&guild.id).into_iter().flatten().filter_map(|item| match item {
            ListItem::Member(m) => Some(&m.user),
            ListItem::Group { .. } => None,
        });
        let known = self.members.get(&guild.id).into_iter().flat_map(|m| m.values());
        let mut seen = HashSet::new();
        let mut users: Vec<&User> = listed.chain(known).filter(|u| u.id != me && hit(&u.name) && seen.insert(u.id.as_str())).collect();
        users.sort_by_cached_key(|u| u.name.to_lowercase());
        users.truncate(MEMBER_CAP);
        let roles = roles.into_iter().map(|r| Candidate { pick: Pick { id: r.id.clone(), target: Target::Role, name: r.name.clone() }, mark: Mark::Role(r.color), detail: "Role".into() });
        let users = users.into_iter().map(|u| Candidate { pick: Pick { id: u.id.clone(), target: Target::Member, name: u.name.clone() }, mark: Mark::Avatar(u.avatar.clone()), detail: "Member".into() });
        roles.chain(users).collect()
    }

    pub(crate) fn render_create_channel(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.create_channel.as_ref()?;
        let guild = self.guilds.iter().find(|g| g.id == form.guild_id)?;
        let step = match form.choosing_access {
            true => self.access_step(form, guild, window, cx),
            false => self.info_step(form, guild, window, cx),
        };
        let body = ui::dialog_body()
            .gap(px(20.))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| match e.keystroke.key.as_str() {
                "enter" => this.advance_create_channel(window, cx),
                "escape" => this.close_create_channel(cx),
                _ => {}
            }))
            .children(step);
        let backdrop = div().id("create-channel-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_create_channel(cx)));
        Some(ui::modal(backdrop, ui::dialog("create-channel", 460., body, window, cx), cx))
    }

    // Step one: the type, the name, and Private channel.
    fn info_step(&self, form: &CreateChannel, guild: &Guild, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let rows: Vec<AnyElement> = offered_types(guild, self.my_id()).into_iter().map(|kind| type_row(kind, form.private, kind == form.kind, window, cx).into_any_element()).collect();
        let empty = self.dialog_input.read(cx).value().trim().is_empty();
        let focus = gpui_kit::Focusable::focus_handle(self.dialog_input.read(cx), cx);
        let name = ui::text_field("create-channel-name", Input::new(&self.dialog_input).appearance(false), &focus).leading(type_icon(form.kind, form.private).size(px(16.)));
        let primary = if needs_access_step(form) { "Next" } else { "Create channel" };
        let actions = div()
            .flex()
            .gap(px(8.))
            .child(ui::button("create-channel-cancel", Tone::Secondary, cx).flex_1().child("Cancel").on_click(cx.listener(|this, _, _, cx| this.close_create_channel(cx))))
            .child(ui::button("create-channel-submit", Tone::Primary, cx).flex_1().disabled(empty).child(primary).on_click(cx.listener(|this, _, window, cx| this.advance_create_channel(window, cx))));
        let category = form.category.clone().map(|c| SharedString::from(format!("in {c}")));
        let mut step = vec![
            dialog_head("Create channel", None, category, cx).into_any_element(),
            group(section_label("Channel type", cx)).child(div().flex().flex_col().gap(px(2.)).children(rows)).into_any_element(),
            group(section_label("Channel name", cx)).child(name).when(form.kind == ANNOUNCEMENT, |g| g.child(announcement_note(cx))).into_any_element(),
        ];
        if allows_private(form.kind) {
            step.push(private_row(form.private, window, cx).into_any_element());
        }
        step.push(actions.into_any_element());
        step
    }

    // Step two: who may see the private channel, or the Stage's moderators. Nothing picked is allowed
    // (Discord's Skip): the channel is then yours alone, or a Stage with no extra moderators.
    fn access_step(&self, form: &CreateChannel, guild: &Guild, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let stage = form.kind == STAGE;
        let title = if stage { "Add Stage moderators" } else { "Add members or roles" };
        let name: SharedString = self.dialog_input.read(cx).value().trim().to_owned().into();
        let primary = if form.picked.is_empty() { "Skip" } else { "Create channel" };
        let actions = div()
            .flex()
            .gap(px(8.))
            .child(ui::button("create-channel-back", Tone::Secondary, cx).flex_1().child("Back").on_click(cx.listener(|this, _, window, cx| this.back_to_channel_info(window, cx))))
            .child(ui::button("create-channel-create", Tone::Primary, cx).flex_1().child(primary).on_click(cx.listener(|this, _, _, cx| this.submit_create_channel(cx))));
        vec![
            dialog_head(title, Some(type_icon(form.kind, form.private)), Some(name), cx).into_any_element(),
            self.access_picker(form, guild, window, cx),
            actions.into_any_element(),
        ]
    }

    // Who may see a private channel, or Stage's moderators: the count, a search, the picks as pills, and
    // every role and member to tick.
    fn access_picker(&self, form: &CreateChannel, guild: &Guild, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let stage = form.kind == STAGE;
        let query = form.search.read(cx).value().to_string();
        let candidates = self.access_candidates(guild, &query);
        let muted = palette::muted(cx);
        let head = section_label(if stage { "Moderators" } else { "Members and roles" }, cx)
            .flex()
            .items_baseline()
            .child(div().ml_auto().text_size(px(12.)).font_weight(FontWeight::NORMAL).text_color(muted).child(access_count(form.picked.len(), stage)));
        let explainer = stage.then(|| {
            div().text_size(px(12.)).line_height(px(17.)).text_color(muted).child("Stage moderators are Speakers who can add and remove other Speakers. They can also start the Stage event. Stage moderators don't have to be mods of the server.")
        });
        let pills = (!form.picked.is_empty()).then(|| div().flex().flex_wrap().gap(px(6.)).children(form.picked.iter().map(|p| pick_pill(p, cx).into_any_element())));
        let list: AnyElement = match candidates.is_empty() {
            true => div().py(px(16.)).text_center().text_color(muted).child(format!("No members or roles match “{}”", query.trim())).into_any_element(),
            false => div()
                .id("create-channel-access")
                .flex()
                .flex_col()
                .gap(px(2.))
                .max_h(px(ACCESS_LIST_HEIGHT))
                .overflow_y_scroll()
                .children(candidates.into_iter().map(|c| {
                    let on = form.picked.iter().any(|p| p.id == c.pick.id);
                    candidate_row(c, on, window, cx)
                }))
                .into_any_element(),
        };
        group(head).children(explainer).child(ui::search_field("create-channel-search", &form.search, cx)).children(pills).child(list).into_any_element()
    }
}

// Text, voice and forum always; media for monetized community servers; announcement with NEWS; stage
// for community servers where you can manage roles and channels. Store, LFG and App aren't offered.
fn offered_types(guild: &Guild, me: &str) -> Vec<u64> {
    let community = guild.has_feature("COMMUNITY");
    let monetized = guild.has_feature("CREATOR_MONETIZABLE") || guild.has_feature("CREATOR_MONETIZABLE_PROVISIONAL");
    let stage_bits = perm::MANAGE_ROLES | perm::MANAGE_CHANNELS;
    TYPES
        .iter()
        .map(|t| t.0)
        .filter(|&kind| match kind {
            MEDIA => community && monetized,
            ANNOUNCEMENT => guild.has_feature("NEWS"),
            STAGE => community && guild.permissions(me) & stage_bits == stage_bits,
            _ => true,
        })
        .collect()
}

fn needs_access_step(form: &CreateChannel) -> bool {
    form.private || form.kind == STAGE
}

fn allows_private(kind: u64) -> bool {
    !matches!(kind, ANNOUNCEMENT | STAGE)
}

// Discord's LIMITED_CHANNEL_NAME types (of the ones offered): their names are slugs.
fn is_text_like(kind: u64) -> bool {
    matches!(kind, TEXT | ANNOUNCEMENT | FORUM | MEDIA)
}

fn placeholder(kind: u64) -> &'static str {
    match kind {
        VOICE | STAGE => "New Channel",
        FORUM => "forum-discussion",
        _ => "new-channel",
    }
}

fn type_icon(kind: u64, private: bool) -> Icon {
    match (kind, private) {
        (VOICE, false) => Icon::new(Glyph::VolumeHigh),
        (VOICE, true) => Icon::new(Glyph::VoiceLocked),
        (FORUM, false) => Icon::new(Glyph::Forum),
        (FORUM, true) => Icon::new(Glyph::ForumLocked),
        (MEDIA, false) => Nav::Media.icon(),
        (MEDIA, true) => Icon::new(Glyph::MediaLocked),
        (ANNOUNCEMENT, false) => Nav::Megaphone.icon(),
        (ANNOUNCEMENT, true) => Icon::new(Glyph::AnnouncementLocked),
        (STAGE, false) => Nav::Stage.icon(),
        (STAGE, true) => Icon::new(Glyph::StageLocked),
        (_, false) => Icon::new(Glyph::Hashtag),
        (_, true) => Icon::new(Glyph::TextLocked),
    }
}

fn type_label(kind: u64) -> (&'static str, &'static str) {
    TYPES.iter().find(|t| t.0 == kind).map_or(("", ""), |t| (t.1, t.2))
}

// Discord's name rule (module 842937): runs of whitespace, `-` or `~` become one `-`, leading dashes go,
// ASCII punctuation is dropped, then lowercase. In that order, so punctuation dropped after the first two
// steps can leave a leading or doubled dash, as in Discord.
fn slug(name: &str) -> String {
    const PUNCTUATION: &str = "\\'!\"#$%&()*+,./:;<=>?@[]^`{|}~";
    let mut collapsed = String::with_capacity(name.len());
    let mut in_run = false;
    for c in name.chars() {
        // JS `\s` also matches the BOM, which Rust's whitespace doesn't.
        let joins = c.is_whitespace() || c == '\u{feff}' || c == '-' || c == '~';
        if joins && !in_run {
            collapsed.push('-');
        }
        if !joins {
            collapsed.push(c);
        }
        in_run = joins;
    }
    collapsed.trim_start_matches('-').chars().filter(|c| !PUNCTUATION.contains(*c)).collect::<String>().to_lowercase()
}

fn shape_name(kind: u64, name: &str) -> String {
    let shaped = if is_text_like(kind) { slug(name) } else { name.to_owned() };
    shaped.chars().take(NAME_MAX).collect()
}

// Re-shapes the field's text and keeps the caret after what was typed before it.
fn reshape(state: &mut SearchState, kind: u64, window: &mut Window, cx: &mut Context<SearchState>) {
    let value = state.value();
    let shaped = shape_name(kind, &value);
    if shaped == value.as_ref() {
        return;
    }
    let before = value.get(..state.cursor()).unwrap_or(&value);
    let mut caret = shape_name(kind, before).len().min(shaped.len());
    while !shaped.is_char_boundary(caret) {
        caret -= 1;
    }
    state.set_value(shaped, window, cx);
    state.set_selected_range(caret..caret, cx);
}

fn create_body(name: &str, kind: u64, private: bool, parent: Option<&str>, guild_id: &str, picked: &[Pick], creator: &Creator) -> Value {
    let mut body = json!({ "name": name, "type": kind, "permission_overwrites": permission_overwrites(kind, private, guild_id, picked, creator) });
    if let Some(parent) = parent {
        body["parent_id"] = json!(parent);
    }
    body
}

// Discord's overwrites (module 147036): @everyone (the server's id) loses what the picks gain. The creator
// gets their own unless a role of theirs was picked or they're owner or Administrator. Not verified: that
// Discord appends the creator for Stage too; the doc observed it for private channels.
fn permission_overwrites(kind: u64, private: bool, guild_id: &str, picked: &[Pick], creator: &Creator) -> Vec<Value> {
    let grant = match kind {
        ANNOUNCEMENT => return vec![overwrite(guild_id, Target::Role, 0, perm::SEND_MESSAGES)],
        STAGE => STAGE_MODERATOR,
        VOICE if private => VIEW_CHANNEL | perm::CONNECT,
        _ if private => VIEW_CHANNEL,
        _ => return Vec::new(),
    };
    let everyone = (kind != STAGE).then(|| overwrite(guild_id, Target::Role, 0, grant));
    let picks = picked.iter().map(|p| overwrite(&p.id, p.target, grant, 0));
    let covered = creator.elevated || picked.iter().any(|p| p.id == creator.id || (p.target == Target::Role && creator.roles.contains(&p.id)));
    let creator = (!covered).then(|| overwrite(creator.id, Target::Member, grant, 0));
    everyone.into_iter().chain(picks).chain(creator).collect()
}

// Bitfields go as strings, as Discord's HTTP layer sends them.
fn overwrite(id: &str, target: Target, allow: u64, deny: u64) -> Value {
    json!({ "id": id, "type": target as u8, "allow": allow.to_string(), "deny": deny.to_string() })
}

fn access_count(picked: usize, stage: bool) -> String {
    match picked {
        0 if stage => "Just you, so far".into(),
        0 => "Only you, so far".into(),
        n => format!("{n} added"),
    }
}

// The title, and under it where the channel goes ("in General") or, on the access step, the channel itself.
fn dialog_head(title: &'static str, icon: Option<Icon>, sub: Option<SharedString>, cx: &mut Context<Waffle>) -> impl IntoElement {
    let (strong, muted) = (palette::strong(cx), palette::muted(cx));
    let sub = sub.map(|text| div().flex().items_center().gap(px(4.)).text_size(px(14.)).text_color(muted).children(icon.map(|i| i.size(px(14.)))).child(text));
    div()
        .flex()
        .items_start()
        .child(div().flex_1().min_w_0().flex().flex_col().gap(px(2.)).child(div().text_size(px(20.)).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD).text_color(strong).child(title)).children(sub))
        .child(ui::icon_button("create-channel-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.close_create_channel(cx))))
}

fn group(label: gpui_kit::Div) -> gpui_kit::Div {
    div().flex().flex_col().gap(px(8.)).child(label)
}

fn section_label(text: &'static str, cx: &Context<Waffle>) -> gpui_kit::Div {
    div().flex().gap(px(6.)).text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(text)
}

fn announcement_note(cx: &Context<Waffle>) -> impl IntoElement {
    div().text_size(px(12.)).line_height(px(17.)).text_color(palette::muted(cx)).child("Users can Follow this channel in their own servers. Posts you \"Publish\" from this channel will appear in those servers.")
}

// One type: radio, then the glyph inline with its name and its line under; the chosen one carries the selected fill.
fn type_row(kind: u64, private: bool, on: bool, window: &mut Window, cx: &mut Context<Waffle>) -> impl IntoElement {
    let (selected, hover, strong, muted) = (palette::selected(cx), palette::hover(cx), palette::strong(cx), palette::muted(cx));
    let (name, hint) = type_label(kind);
    div()
        .id(SharedString::from(format!("create-channel-type-{kind}")))
        .flex()
        .items_center()
        .gap(px(12.))
        .px(px(12.))
        .py(px(10.))
        .squircle(ui::RADIUS_MD)
        .cursor_pointer()
        .when(on, |r| r.bg(selected))
        .when(!on, |r| r.hover(move |s| s.bg(hover)))
        .child(ui::radio_live(SharedString::from(format!("create-channel-radio-{kind}")), on, window, cx))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().flex().items_center().gap(px(6.)).text_color(strong).child(type_icon(kind, private && allows_private(kind)).size(px(18.))).child(div().font_weight(FontWeight::MEDIUM).child(name)))
                .child(div().text_size(px(12.)).line_height(px(16.)).text_color(muted).child(hint)),
        )
        .on_click(cx.listener(move |this, _, window, cx| this.set_channel_kind(kind, window, cx)))
}

fn private_row(on: bool, window: &mut Window, cx: &mut Context<Waffle>) -> impl IntoElement {
    let (strong, muted) = (palette::strong(cx), palette::muted(cx));
    div()
        .id("create-channel-private")
        .flex()
        .items_center()
        .gap(px(16.))
        .cursor_pointer()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().flex().items_center().gap(px(6.)).text_color(strong).child(Icon::new(Glyph::HashtagLock).size(px(16.))).child(div().font_weight(FontWeight::MEDIUM).child("Private channel")))
                .child(div().text_size(px(12.)).line_height(px(17.)).text_color(muted).child("Only selected members and roles will be able to view this channel.")),
        )
        .child(ui::switch_live("create-channel-private-switch", on, window, cx))
        .on_click(cx.listener(|this, _, window, cx| this.toggle_create_private(window, cx)))
}

fn pick_pill(pick: &Pick, cx: &mut Context<Waffle>) -> impl IntoElement {
    let remove = pick.clone();
    div()
        .h(px(26.))
        .pl(px(8.))
        .pr(px(1.))
        .flex()
        .items_center()
        .gap(px(2.))
        .squircle(ui::RADIUS_XS)
        .bg(palette::selected(cx))
        .text_color(palette::strong(cx))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .whitespace_nowrap()
        .child(pick.name.clone())
        .child(
            ui::icon_button(SharedString::from(format!("create-channel-unpick-{}", pick.id)), Glyph::X, format!("Remove {}", pick.name), cx)
                .small()
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_access(remove.clone(), cx))),
        )
}

fn candidate_row(candidate: Candidate, on: bool, window: &mut Window, cx: &mut Context<Waffle>) -> AnyElement {
    let id = candidate.pick.id.clone();
    let mark: AnyElement = match &candidate.mark {
        Mark::Avatar(url) => ui::avatar(url.clone(), 32.).into_any_element(),
        Mark::Role(color) => {
            let dot = if *color == 0 { palette::faint(cx) } else { palette::role_ink(*color, cx.theme().is_dark()) };
            div().size(px(32.)).flex_none().rounded_full().bg(palette::selected(cx)).flex().items_center().justify_center().child(div().size(px(12.)).rounded_full().bg(dot)).into_any_element()
        }
    };
    let text = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(div().truncate().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(candidate.pick.name.clone()))
        .child(div().truncate().text_size(px(12.)).text_color(palette::muted(cx)).child(candidate.detail));
    let pick = candidate.pick;
    ui::menu_row(SharedString::from(format!("create-channel-access-{id}")), cx)
        .child(ui::check_live(SharedString::from(format!("create-channel-check-{id}")), if on { Check::On } else { Check::Off }, window, cx))
        .child(mark)
        .child(text)
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_access(pick.clone(), cx)))
        .into_any_element()
}

pub(crate) fn demo(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    let (features, kind, name): (&[&str], u64, &str) = match scene {
        "create-channel-community" => (&["COMMUNITY", "NEWS", "CREATOR_MONETIZABLE"], ANNOUNCEMENT, "announcements"),
        "create-channel-stage" => (&["COMMUNITY"], STAGE, "Town Hall"),
        "create-channel-private" => (&[], TEXT, "staff-lounge"),
        _ => (&[], TEXT, ""),
    };
    if let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") {
        guild.features = features.iter().map(|f| f.to_string()).collect();
    }
    waffle.open_create_channel("1".into(), kind, Some("c0".into()), window, cx);
    waffle.dialog_input.update(cx, |state, cx| state.set_value(name, window, cx));
    let Some(guild) = waffle.guilds.iter().find(|g| g.id == "1") else { return };
    let candidates = waffle.access_candidates(guild, "");
    let role = candidates.iter().find(|c| c.pick.target == Target::Role).map(|c| c.pick.clone());
    let member = candidates.iter().find(|c| c.pick.target == Target::Member).map(|c| c.pick.clone());
    let Some(form) = waffle.create_channel.as_mut() else { return };
    match scene {
        "create-channel-private" => {
            form.private = true;
            form.picked = role.into_iter().chain(member).collect();
            form.choosing_access = true;
        }
        "create-channel-stage" => {
            form.picked = member.into_iter().collect();
            form.choosing_access = true;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick(id: &str, target: Target) -> Pick {
        Pick { id: id.into(), target, name: id.into() }
    }

    fn ow(id: &str, target: u8, allow: u64, deny: u64) -> Value {
        json!({ "id": id, "type": target, "allow": allow.to_string(), "deny": deny.to_string() })
    }

    const MEMBER: Creator<'static> = Creator { id: "me", roles: &[], elevated: false };

    #[test]
    fn slug_follows_discords_rule() {
        assert_eq!(slug("Hello World"), "hello-world");
        assert_eq!(slug("  --a  ~ b--"), "a-b-");
        assert_eq!(slug("What's up?! #1"), "whats-up-1");
        assert_eq!(slug("!-a"), "-a");
        assert_eq!(slug("a - ! - b"), "a--b");
        assert_eq!(slug("Café 🎮 Ünïcode"), "café-🎮-ünïcode");
        assert_eq!(slug("a\tb\u{feff}c"), "a-b-c");
        assert_eq!(shape_name(VOICE, "Lounge Room!"), "Lounge Room!");
        assert_eq!(shape_name(TEXT, &"x".repeat(150)).len(), NAME_MAX);
    }

    #[test]
    fn private_text_hides_from_everyone_and_shows_to_picks_and_creator() {
        let picks = [pick("mods", Target::Role), pick("ana", Target::Member)];
        let got = permission_overwrites(TEXT, true, "g", &picks, &MEMBER);
        assert_eq!(got, vec![ow("g", 0, 0, VIEW_CHANNEL), ow("mods", 0, VIEW_CHANNEL, 0), ow("ana", 1, VIEW_CHANNEL, 0), ow("me", 1, VIEW_CHANNEL, 0)]);
    }

    #[test]
    fn private_voice_adds_connect() {
        let bits = VIEW_CHANNEL | perm::CONNECT;
        let got = permission_overwrites(VOICE, true, "g", &[pick("ana", Target::Member)], &MEMBER);
        assert_eq!(got, vec![ow("g", 0, 0, bits), ow("ana", 1, bits, 0), ow("me", 1, bits, 0)]);
    }

    #[test]
    fn announcement_stops_everyone_sending() {
        assert_eq!(permission_overwrites(ANNOUNCEMENT, false, "g", &[], &MEMBER), vec![ow("g", 0, 0, perm::SEND_MESSAGES)]);
        assert!(permission_overwrites(TEXT, false, "g", &[pick("ana", Target::Member)], &MEMBER).is_empty());
    }

    #[test]
    fn stage_makes_picks_moderators_and_leaves_everyone() {
        let got = permission_overwrites(STAGE, false, "g", &[pick("ana", Target::Member)], &MEMBER);
        assert_eq!(got, vec![ow("ana", 1, STAGE_MODERATOR, 0), ow("me", 1, STAGE_MODERATOR, 0)]);
    }

    #[test]
    fn creator_needs_no_overwrite_when_covered() {
        let roles = ["mods".to_owned()];
        let in_role = Creator { id: "me", roles: &roles, elevated: false };
        assert_eq!(permission_overwrites(TEXT, true, "g", &[pick("mods", Target::Role)], &in_role).len(), 2);
        let admin = Creator { id: "me", roles: &[], elevated: true };
        assert_eq!(permission_overwrites(TEXT, true, "g", &[], &admin), vec![ow("g", 0, 0, VIEW_CHANNEL)]);
    }

    #[test]
    fn body_sends_parent_only_from_a_category() {
        let body = create_body("news", TEXT, false, Some("cat"), "g", &[], &MEMBER);
        assert_eq!(body, json!({ "name": "news", "type": 0, "permission_overwrites": [], "parent_id": "cat" }));
        assert!(create_body("news", TEXT, false, None, "g", &[], &MEMBER).get("parent_id").is_none());
    }
}
