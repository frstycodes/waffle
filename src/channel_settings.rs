// Channel settings, ported from design/channel-settings.html: Discord's Edit Channel over the whole window.
// Overview and Permissions edits collect in one save bar and go out as PATCH /channels/{id} plus a PUT or
// DELETE per changed permission overwrite; Invites and Webhooks load when the window opens.
use gpui_kit::{
    AnyElement, App, AppContext, Context, Div, Entity, Focusable, FontWeight, IntoElement, ParentElement, ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Window,
    component::{
        ActiveTheme, Icon,
        input::{Input, InputEvent, InputState, Textarea, TextareaState},
    },
    div,
    px,
};
use reqwest::Method;
use serde_json::{Map, Value, json};

use crate::{
    Waffle,
    discord::{Channel, ChannelKind, ChannelProps, Event, Overwrite, parse_channel, perm},
    icons::Glyph,
    palette::{self, Level},
    settings::registry::{self, Control, Scope, Setting},
    overwrites::{overwrite_ops, permission_groups, private, set_private, set_tri, synced, tri},
    ui::{self, Notice, Squircle, Tone},
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Overview,
    Permissions,
    Invites,
    Webhooks,
}

impl Page {
    fn label(self) -> &'static str {
        match self {
            Page::Overview => "Overview",
            Page::Permissions => "Permissions",
            Page::Invites => "Invites",
            Page::Webhooks => "Integrations",
        }
    }

    fn glyph(self) -> Glyph {
        match self {
            Page::Overview => Glyph::Info,
            Page::Permissions => Glyph::Shield,
            Page::Invites => Glyph::Link,
            Page::Webhooks => Glyph::AppWindow,
        }
    }
}

impl Scope for Page {
    fn label(self) -> &'static str {
        Page::label(self)
    }
}

const fn cs(id: &'static str, page: Page, group: &'static str, title: &'static str, caption: Option<&'static str>, keywords: &'static [&'static str], row: &'static str) -> Setting<Page> {
    Setting { id, page, group, title, caption, keywords, control: Control::Custom, row: Some(row) }
}

// Every channel setting for search; `applies` drops the ones a channel type doesn't have.
pub static CHANNEL_SETTINGS: &[Setting<Page>] = &[
    cs("channel-name", Page::Overview, "Name and topic", "Channel name", None, &["rename", "name", "title"], "cs-name"),
    cs("channel-topic", Page::Overview, "Name and topic", "Topic", Some("Shows in the channel header"), &["description", "guidelines", "header", "about"], "cs-topic"),
    cs("bitrate", Page::Overview, "Sound and video", "Bitrate", Some("Higher sounds better and costs slow connections more"), &["audio quality", "kbps", "sound quality"], "cs-bitrate"),
    cs("user-limit", Page::Overview, "Sound and video", "User limit", Some("Members with Move members can still join"), &["max users", "capacity", "cap", "how many"], "cs-limit"),
    cs("video-quality", Page::Overview, "Sound and video", "Video quality", Some("720p sends full quality to everyone watching"), &["camera", "720p", "resolution"], "cs-quality"),
    cs("region", Page::Overview, "Sound and video", "Region", Some("Where the voice server runs"), &["server location", "rtc", "latency", "ping"], "cs-region"),
    cs("slowmode", Page::Overview, "Chat", "Slowmode", Some("Each member sends one message and starts one thread per interval"), &["rate limit", "cooldown", "spam", "slow"], "cs-slowmode"),
    cs("age-restricted", Page::Overview, "Chat", "Age-restricted", Some("People confirm they're over 18 before they see it"), &["nsfw", "18+", "adult", "content visibility"], "cs-nsfw"),
    cs("hide-threads", Page::Overview, "Chat", "Hide threads after", Some("New threads close after this long without activity"), &["archive", "inactivity", "auto archive", "threads"], "cs-archive"),
    cs("private-channel", Page::Permissions, "Permissions", "Private channel", Some("Only the roles and members you add can see it"), &["lock", "hide", "visible", "who can see", "access"], "cs-private"),
    cs("overrides", Page::Permissions, "Roles and members", "Role and member permissions", Some("Deny, inherit or allow each permission"), &["overwrite", "override", "roles", "advanced", "allow", "deny", "sync", "category"], "cs-target-list"),
    cs("invites", Page::Invites, "Invites", "Invites", Some("Revoke links to this channel"), &["invite link", "revoke", "join link"], "cs-invites"),
    cs("webhooks", Page::Webhooks, "Webhooks", "Webhooks", Some("Copy a webhook's URL or delete it"), &["integrations", "bots", "webhook url", "apps"], "cs-webhooks"),
];

// Whether a channel has this setting: voice channels trade topic and thread hiding for sound settings;
// categories keep only their name.
fn applies(id: &str, channel: &Channel) -> bool {
    let voice = matches!(channel.raw_type, 2 | 13);
    match id {
        _ if channel.kind == ChannelKind::Category => matches!(id, "channel-name" | "private-channel" | "overrides" | "invites" | "webhooks"),
        "channel-topic" | "hide-threads" => !voice,
        "bitrate" | "user-limit" | "video-quality" | "region" => voice,
        _ => true,
    }
}

// The pages our permissions on the channel open, in nav order: Discord hides the rest.
pub fn pages(perms: u64) -> Vec<Page> {
    [(Page::Overview, perm::MANAGE_CHANNELS), (Page::Permissions, perm::MANAGE_ROLES), (Page::Invites, perm::MANAGE_CHANNELS), (Page::Webhooks, perm::MANAGE_WEBHOOKS)]
        .into_iter()
        .filter(|(_, bit)| perms & bit != 0)
        .map(|(page, _)| page)
        .collect()
}

pub fn editable(channel: &Channel) -> bool {
    !matches!(channel.kind, ChannelKind::Thread | ChannelKind::Dm)
}

// Everything the save bar tracks. Name and topic live in their inputs and are read back into this.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Draft {
    pub name: String,
    pub topic: String,
    pub props: ChannelProps,
    pub overwrites: Vec<Overwrite>,
}

impl Draft {
    pub fn of(channel: &Channel) -> Self {
        Self { name: channel.name.clone(), topic: channel.topic.clone().unwrap_or_default(), props: (*channel.props).clone(), overwrites: channel.overwrites.clone() }
    }
}

// The Modify Channel body: only fields that changed, in Discord's names.
pub fn patch_body(before: &Channel, draft: &Draft) -> Map<String, Value> {
    let mut body = Map::new();
    let (old, new) = (&before.props, &draft.props);
    if draft.name.trim() != before.name {
        body.insert("name".into(), json!(draft.name.trim()));
    }
    if draft.topic.trim() != before.topic.as_deref().unwrap_or_default() {
        body.insert("topic".into(), if draft.topic.trim().is_empty() { Value::Null } else { json!(draft.topic.trim()) });
    }
    if new.nsfw != old.nsfw {
        body.insert("nsfw".into(), json!(new.nsfw));
    }
    if new.slowmode != old.slowmode {
        body.insert("rate_limit_per_user".into(), json!(new.slowmode));
    }
    if new.bitrate != old.bitrate {
        body.insert("bitrate".into(), json!(new.bitrate));
    }
    if new.user_limit != old.user_limit {
        body.insert("user_limit".into(), json!(new.user_limit));
    }
    if new.rtc_region != old.rtc_region {
        body.insert("rtc_region".into(), json!(new.rtc_region));
    }
    if new.video_full != old.video_full {
        body.insert("video_quality_mode".into(), json!(if new.video_full { 2 } else { 1 }));
    }
    if new.archive_after != old.archive_after {
        body.insert("default_auto_archive_duration".into(), json!(new.archive_after));
    }
    body
}

// Discord's caps by boost level (Modify Channel docs); stages top out at 64 kbps.
pub fn bitrate_cap(premium_tier: u8, stage: bool) -> u32 {
    if stage {
        return 64_000;
    }
    match premium_tier {
        0 => 96_000,
        1 => 128_000,
        2 => 256_000,
        _ => 384_000,
    }
}

// Discord client's slowmode stops; the API takes 0-21600 seconds.
const SLOWMODE: [(u32, &str); 14] = [
    (0, "Off"),
    (5, "5 seconds"),
    (10, "10 seconds"),
    (15, "15 seconds"),
    (30, "30 seconds"),
    (60, "1 minute"),
    (120, "2 minutes"),
    (300, "5 minutes"),
    (600, "10 minutes"),
    (900, "15 minutes"),
    (1800, "30 minutes"),
    (3600, "1 hour"),
    (7200, "2 hours"),
    (21600, "6 hours"),
];

// default_auto_archive_duration, minutes.
const ARCHIVE: [(u32, &str); 4] = [(60, "1 hour"), (1440, "24 hours"), (4320, "3 days"), (10080, "1 week")];

// "Never", "Expired", or how long is left ("in 3 days").
pub fn expires_label(expires_at: Option<&str>, now: chrono::DateTime<chrono::Utc>) -> String {
    let Some(end) = expires_at.and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()) else { return "Never".into() };
    let left = end.with_timezone(&chrono::Utc) - now;
    let (days, hours, minutes) = (left.num_days(), left.num_hours(), left.num_minutes());
    match () {
        _ if minutes <= 0 => "Expired".into(),
        _ if days >= 1 => format!("in {days} {}", if days == 1 { "day" } else { "days" }),
        _ if hours >= 1 => format!("in {hours}h"),
        _ => format!("in {minutes}m"),
    }
}

#[derive(Clone, Debug)]
pub struct Invite {
    code: String,
    inviter: String,
    uses: u64,
    max_uses: u64,
    expires_at: Option<String>,
}

fn parse_invite(v: &Value) -> Option<Invite> {
    let inviter = &v["inviter"];
    Some(Invite {
        code: v["code"].as_str()?.to_owned(),
        inviter: inviter["global_name"].as_str().or(inviter["username"].as_str()).unwrap_or("Unknown").to_owned(),
        uses: v["uses"].as_u64().unwrap_or(0),
        max_uses: v["max_uses"].as_u64().unwrap_or(0),
        expires_at: v["expires_at"].as_str().map(str::to_owned),
    })
}

#[derive(Clone, Debug)]
pub struct Webhook {
    id: String,
    name: String,
    by: Option<String>,
    // Incoming webhooks only; ones that follow a channel have no token.
    url: Option<String>,
}

fn parse_webhook(v: &Value) -> Option<Webhook> {
    let id = v["id"].as_str()?.to_owned();
    let url = v["token"].as_str().map(|token| format!("https://discord.com/api/webhooks/{id}/{token}"));
    let user = &v["user"];
    Some(Webhook { name: v["name"].as_str().unwrap_or("Webhook").to_owned(), by: user["global_name"].as_str().or(user["username"].as_str()).map(str::to_owned), url, id })
}

#[derive(Clone, Debug, PartialEq)]
pub enum Load<T> {
    Loading,
    Ready(T),
    Failed(String),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Select {
    Slowmode,
    Archive,
    Region,
    AddRole,
}

#[derive(Clone, PartialEq, Debug)]
enum Save {
    Idle,
    Saving,
    Failed(String),
}

pub struct ChannelSettings {
    guild_id: String,
    channel_id: String,
    page: Page,
    name: Entity<InputState>,
    topic: Entity<TextareaState>,
    draft: Draft,
    // The role or member whose overwrite the permissions page shows.
    target: Option<String>,
    select: Option<Select>,
    save: Save,
    // Close was asked for with unsaved changes: the bar asks to save or discard.
    closing: bool,
    asking_delete: bool,
    invites: Load<Vec<Invite>>,
    webhooks: Load<Vec<Webhook>>,
    regions: Vec<(String, String)>,
    scroll: ScrollHandle,
    search: Entity<InputState>,
}

// Demo scenes: a page, and made-up invites, webhooks and regions in place of Discord's.
#[cfg(debug_assertions)]
impl ChannelSettings {
    pub fn demo(&mut self, page: Page) {
        let invite = |code: &str, inviter: &str, uses, max_uses, expires_at: Option<&str>| Invite { code: code.into(), inviter: inviter.into(), uses, max_uses, expires_at: expires_at.map(Into::into) };
        let in_days = |d: i64| (chrono::Utc::now() + chrono::Duration::days(d) + chrono::Duration::minutes(5)).to_rfc3339();
        let (three, seven) = (in_days(3), in_days(7));
        self.page = page;
        self.invites = Load::Ready(vec![
            invite("7Kq2xQ", "Wren", 12, 25, Some(&three)),
            invite("zP0aWq8rTt4Y", "Bartholomew Featherstonehaugh-Wright", 0, 10, Some(&seven)),
            invite("hangout", "kiwi", 412, 0, None),
        ]);
        self.webhooks = Load::Ready(vec![
            Webhook { id: "w1".into(), name: "Clip bot".into(), by: Some("Wren".into()), url: Some("https://discord.com/api/webhooks/w1/demo".into()) },
            Webhook { id: "w2".into(), name: "Release notes from the build server that runs every night".into(), by: Some("ferris".into()), url: None },
        ]);
        self.regions = [("rotterdam", "Rotterdam"), ("us-east", "US East"), ("japan", "Japan")].map(|(id, name)| (id.to_owned(), name.to_owned())).to_vec();
        self.target = Some("r2".into());
    }
}

#[cfg(debug_assertions)]
impl Waffle {
    pub(crate) fn demo_channel_search(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = &self.channel_settings else { return };
        view.search.update(cx, |s, cx| s.set_value(query.to_owned(), window, cx));
    }

    // An unsaved edit, so the save bar shows.
    pub(crate) fn demo_channel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = &mut self.channel_settings else { return };
        view.name.update(cx, |s, cx| s.set_value("general-chat", window, cx));
        self.edit_draft(cx, |d| d.props.nsfw = true);
    }
}

impl Waffle {
    fn channel_perms(&self, guild_id: &str, channel: &Channel) -> u64 {
        let (Some(me), Some(guild)) = (self.me.as_ref(), self.guilds.iter().find(|g| g.id == guild_id)) else { return 0 };
        guild.channel_permissions(&me.id, channel)
    }

    pub(crate) fn channel_setting_pages(&self, guild_id: &str, channel: &Channel) -> Vec<Page> {
        if !editable(channel) {
            return Vec::new();
        }
        pages(self.channel_perms(guild_id, channel))
    }

    fn settings_channel(&self) -> Option<&Channel> {
        let view = self.channel_settings.as_ref()?;
        self.guilds.iter().find(|g| g.id == view.guild_id)?.channels.iter().find(|c| c.id == view.channel_id)
    }

    pub(crate) fn open_channel_settings(&mut self, channel: Channel, guild_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(page) = self.channel_setting_pages(&guild_id, &channel).first().copied() else { return };
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Channel name"));
        name.update(cx, |s, cx| s.set_value(channel.name.clone(), window, cx));
        let topic = cx.new(|cx| TextareaState::new(window, cx).placeholder("Let everyone know how to use this channel"));
        topic.update(cx, |s, cx| s.set_value(channel.topic.clone().unwrap_or_default(), window, cx));
        cx.subscribe(&name, |_, _, _: &InputEvent, cx| cx.notify()).detach();
        cx.subscribe(&topic, |_, _, _: &InputEvent, cx| cx.notify()).detach();
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        cx.subscribe(&search, |_, _, _: &InputEvent, cx| cx.notify()).detach();
        let target = channel.overwrites.first().map(|o| o.id.clone());
        self.channel_settings = Some(ChannelSettings {
            guild_id: guild_id.clone(),
            channel_id: channel.id.clone(),
            page,
            name,
            topic,
            draft: Draft::of(&channel),
            target,
            select: None,
            save: Save::Idle,
            closing: false,
            asking_delete: false,
            invites: Load::Loading,
            webhooks: Load::Loading,
            regions: Vec::new(),
            scroll: ScrollHandle::new(),
            search,
        });
        self.menu = None;
        self.load_channel_lists(&channel, cx);
        cx.notify();
    }

    fn load_channel_lists(&mut self, channel: &Channel, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let (id, voice) = (channel.id.clone(), channel.kind == ChannelKind::Voice);
        cx.spawn(async move |this, cx| {
            let invites = client.call(Method::GET, format!("/channels/{id}/invites"), None).await;
            let webhooks = client.call(Method::GET, format!("/channels/{id}/webhooks"), None).await;
            let regions = if voice { client.call(Method::GET, "/voice/regions".into(), None).await.ok() } else { None };
            this.update(cx, |this, cx| {
                let Some(view) = this.channel_settings.as_mut().filter(|v| v.channel_id == id) else { return };
                view.invites = load_list(invites, parse_invite);
                view.webhooks = load_list(webhooks, parse_webhook);
                view.regions = regions
                    .and_then(|r| r.as_array().cloned())
                    .into_iter()
                    .flatten()
                    .filter(|r| !r["deprecated"].as_bool().unwrap_or(false))
                    .filter_map(|r| Some((r["id"].as_str()?.to_owned(), r["name"].as_str()?.to_owned())))
                    .collect();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn current_draft(&self, cx: &App) -> Option<Draft> {
        let view = self.channel_settings.as_ref()?;
        Some(Draft { name: view.name.read(cx).value().to_string(), topic: view.topic.read(cx).value().to_string(), ..view.draft.clone() })
    }

    fn channel_dirty(&self, cx: &App) -> bool {
        let (Some(channel), Some(draft)) = (self.settings_channel(), self.current_draft(cx)) else { return false };
        !patch_body(channel, &draft).is_empty() || !overwrite_ops(&channel.overwrites, &draft.overwrites).is_empty()
    }

    pub(crate) fn close_channel_settings(&mut self, force: bool, cx: &mut Context<Self>) {
        if !force && self.channel_dirty(cx) {
            if let Some(view) = &mut self.channel_settings {
                view.closing = true;
            }
            return cx.notify();
        }
        self.channel_settings = None;
        cx.notify();
    }

    fn edit_draft(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Draft)) {
        if let Some(view) = &mut self.channel_settings {
            change(&mut view.draft);
            view.select = None;
            if view.save != Save::Saving {
                view.save = Save::Idle;
            }
        }
        cx.notify();
    }

    fn reset_channel_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(channel) = self.settings_channel().cloned() else { return };
        let Some(view) = &mut self.channel_settings else { return };
        view.draft = Draft::of(&channel);
        (view.save, view.closing) = (Save::Idle, false);
        view.name.update(cx, |s, cx| s.set_value(channel.name.clone(), window, cx));
        view.topic.update(cx, |s, cx| s.set_value(channel.topic.clone().unwrap_or_default(), window, cx));
        cx.notify();
    }

    // Sends the edits, then reads the channel back so what's shown is what Discord kept. Nothing changes
    // locally until Discord answers; a refusal stays in the bar in Discord's words.
    fn save_channel_settings(&mut self, close_after: bool, cx: &mut Context<Self>) {
        let (Some(client), Some(channel), Some(draft)) = (self.client.clone(), self.settings_channel().cloned(), self.current_draft(cx)) else { return };
        let Some(view) = &mut self.channel_settings else { return };
        if draft.name.trim().is_empty() {
            view.save = Save::Failed("Channel name can't be empty.".into());
            return cx.notify();
        }
        view.save = Save::Saving;
        let guild_id = view.guild_id.clone();
        let body = patch_body(&channel, &draft);
        let mut requests: Vec<(Method, String, Option<Value>)> = Vec::new();
        if !body.is_empty() {
            requests.push((Method::PATCH, format!("/channels/{}", channel.id), Some(Value::Object(body))));
        }
        requests.extend(overwrite_ops(&channel.overwrites, &draft.overwrites).iter().map(|op| op.request(&channel.id)));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let mut failed = None;
            for (method, path, body) in requests {
                if let Err(error) = client.call(method, path, body).await {
                    failed = Some(error);
                    break;
                }
            }
            let fresh = client.call(Method::GET, format!("/channels/{}", channel.id), None).await.ok().and_then(|v| parse_channel(&v, &guild_id));
            this.update(cx, |this, cx| {
                if let Some(fresh) = fresh.clone() {
                    this.on_event(Event::ChannelUpsert { guild_id: guild_id.clone(), channel: fresh }, cx);
                }
                let Some(view) = this.channel_settings.as_mut().filter(|v| v.channel_id == channel.id) else { return };
                match failed {
                    Some(error) => view.save = Save::Failed(error),
                    None => {
                        view.save = Save::Idle;
                        if let Some(fresh) = &fresh {
                            view.draft = Draft { name: view.draft.name.clone(), topic: view.draft.topic.clone(), ..Draft::of(fresh) };
                        }
                        if close_after {
                            this.channel_settings = None;
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn render_channel_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let channel = self.settings_channel().cloned()?;
        let view = self.channel_settings.as_ref()?;
        let (page, scroll, guild_id) = (view.page, view.scroll.clone(), view.guild_id.clone());
        let sections: Vec<AnyElement> = match page {
            Page::Overview => self.overview_page(&channel, window, cx),
            Page::Permissions => self.permissions_page(&channel, &guild_id, window, cx),
            Page::Invites => self.invites_page(cx),
            Page::Webhooks => self.webhooks_page(cx),
        };
        let body = ui::settings_page("channel-settings-scroll", &scroll)
            .child(ui::settings_column(ui::settings_title(page.label(), cx)))
            .children(sections.into_iter().map(ui::settings_column));
        let main = div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(palette::surface(Level::Page, cx))
            .child(body)
            .child(gpui_kit::component::scroll::Scrollbar::vertical(&scroll))
            .children(self.save_bar(cx))
            .child(ui::settings_close("channel-settings-close", cx.listener(|this, _, _, cx| this.close_channel_settings(false, cx)), cx));
        let nav = self.channel_settings_nav(&channel, &guild_id, cx).into_any_element();
        Some(self.settings_frame("channel-settings", nav, main, cx).into_any_element())
    }

    fn channel_settings_nav(&self, channel: &Channel, guild_id: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.channel_settings.as_ref().expect("channel settings are open");
        let category = self.guilds.iter().find(|g| g.id == guild_id).and_then(|g| g.channels.iter().find(|c| Some(&c.id) == channel.parent_id.as_ref())).map(|c| c.name.clone());
        let head = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(6.))
            .pb(px(12.))
            .min_w_0()
            .child(ui::icon_tile(channel_glyph(channel), palette::selected(cx), palette::strong(cx)).size(px(40.)).flex_none())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .line_height(px(18.))
                    .child(div().truncate().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(channel.name.clone()))
                    .child(div().truncate().text_size(px(12.)).text_color(palette::muted(cx)).child(category.unwrap_or_else(|| "Channel settings".into()))),
            );
        let items: Vec<AnyElement> = self
            .channel_setting_pages(guild_id, channel)
            .into_iter()
            .map(|page| {
                ui::settings_nav_item(SharedString::from(format!("cs-nav-{page:?}")), page.glyph(), page.label(), view.page == page, cx)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(view) = &mut this.channel_settings {
                            (view.page, view.select, view.asking_delete) = (page, None, false);
                            view.scroll.set_offset(gpui_kit::point(px(0.), px(0.)));
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        let can_delete = self.channel_perms(guild_id, channel) & perm::MANAGE_CHANNELS != 0;
        let delete = can_delete.then(|| {
            let label = if channel.kind == ChannelKind::Category { "Delete category" } else { "Delete channel" };
            ui::menu_row("cs-nav-delete", cx)
                .danger(cx)
                .child(Icon::new(Glyph::Trash).size(px(18.)))
                .child(label)
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(view) = &mut this.channel_settings {
                        view.asking_delete = !view.asking_delete;
                    }
                    cx.notify();
                }))
        });
        let ask = if view.asking_delete { Some(self.delete_ask(channel, cx).into_any_element()) } else { None };
        let rule = div().h(px(1.)).mx(px(10.)).my(px(8.)).flex_none().bg(palette::line(cx));
        let search = div().flex_none().pb(px(12.)).child(ui::search_field("channel-settings-search", &view.search, cx));
        let query = view.search.read(cx).value().to_string();
        let items = if query.trim().is_empty() { items } else { self.channel_setting_results(channel, guild_id, &query, cx) };
        ui::settings_sidebar("channel-settings-nav", &self.settings_nav_scroll, cx)
            .child(head)
            .child(search)
            .children(items)
            .child(rule)
            .children(delete)
            .children(ask)
    }

    // Search results in place of the nav: only settings this channel has and this user may open.
    fn channel_setting_results(&self, channel: &Channel, guild_id: &str, query: &str, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let open = self.channel_setting_pages(guild_id, channel);
        let hits: Vec<&'static Setting<Page>> = registry::search(CHANNEL_SETTINGS, query).into_iter().filter(|s| open.contains(&s.page) && applies(s.id, channel)).collect();
        if hits.is_empty() {
            return vec![div().px(px(10.)).py(px(8.)).text_color(palette::muted(cx)).child("No settings match").into_any_element()];
        }
        hits.into_iter()
            .map(|setting| {
                let place = format!("{} \u{203a} {}", setting.page.label(), setting.group);
                ui::menu_row(SharedString::from(format!("cs-result-{}", setting.id)), cx)
                    .h_auto()
                    .py(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().truncate().text_color(palette::strong(cx)).child(setting.title))
                            .child(div().truncate().text_size(px(12.)).text_color(palette::muted(cx)).child(place)),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| this.jump_to_channel_setting(setting, window, cx)))
                    .into_any_element()
            })
            .collect()
    }

    // Opens the setting's page and lights its row for a moment, as user settings' search does.
    fn jump_to_channel_setting(&mut self, setting: &'static Setting<Page>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = &mut self.channel_settings else { return };
        (view.page, view.select, view.asking_delete) = (setting.page, None, false);
        view.scroll.set_offset(gpui_kit::point(px(0.), px(0.)));
        view.search.update(cx, |state, cx| state.set_value("", window, cx));
        let row = setting.row_id();
        cx.set_global(ui::SettingsHighlight(Some(SharedString::from(row))));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(1600)).await;
            let _ = this.update(cx, |_, cx| {
                if cx.try_global::<ui::SettingsHighlight>().and_then(|h| h.0.as_deref()) == Some(row) {
                    cx.set_global(ui::SettingsHighlight(None));
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    // Delete asks in place, as Log out does in settings: what goes, then Keep or Delete.
    fn delete_ask(&self, channel: &Channel, cx: &mut Context<Self>) -> impl IntoElement {
        let id = channel.id.clone();
        let what = if channel.kind == ChannelKind::Category { format!("Delete the {} category? Its channels stay, without a category.", channel.name) } else { format!("Delete #{}? Its messages go with it. This can't be undone.", channel.name) };
        div()
            .my(px(4.))
            .p(px(10.))
            .squircle(ui::RADIUS_MD)
            .bg(palette::danger().opacity(0.1))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(div().text_color(palette::strong(cx)).child(what))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(ui::button("cs-delete-keep", Tone::Secondary, cx).small().flex_1().child("Keep").on_click(cx.listener(|this, _, _, cx| {
                        if let Some(view) = &mut this.channel_settings {
                            view.asking_delete = false;
                        }
                        cx.notify();
                    })))
                    .child(ui::button("cs-delete-yes", Tone::Danger, cx).small().flex_1().child("Delete").on_click(cx.listener(move |this, _, _, cx| {
                        let id = id.clone();
                        this.rest(Method::DELETE, format!("/channels/{id}"), None, move |this, cx| {
                            this.on_event(Event::ChannelDelete { guild_id: this.channel_settings.as_ref().map(|v| v.guild_id.clone()).unwrap_or_default(), id: id.clone() }, cx);
                            this.channel_settings = None;
                        }, cx);
                    }))),
            )
    }

    fn overview_page(&self, channel: &Channel, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let view = self.channel_settings.as_ref().expect("channel settings are open");
        let props = view.draft.props.clone();
        let voice = matches!(channel.raw_type, 2 | 13);
        let category = channel.kind == ChannelKind::Category;
        let forum = matches!(channel.raw_type, 15 | 16);
        let mut out = Vec::new();

        let name_focus = view.name.read(cx).focus_handle(cx);
        let name = ui::field("Channel name", cx).child(ui::text_field("cs-name", Input::new(&view.name).appearance(false), &name_focus).leading(Icon::new(channel_glyph(channel)).size(px(16.))));
        let topic = (!voice && !category).then(|| {
            let focus = view.topic.read(cx).focus_handle(cx);
            let count = view.topic.read(cx).value().chars().count();
            let cap = if forum { 4096 } else { 1024 };
            ui::field(if forum { "Post guidelines" } else { "Topic" }, cx)
                .child(ui::text_area("cs-topic", Textarea::new(&view.topic).appearance(false).px_0().text_size(px(14.)), &focus).error(count > cap))
                .child(ui::field_hint("Shows in the channel header.", Some(format!("{count} / {cap}").into()), cx))
        });
        out.push(ui::settings_group(if category || voice { "Name" } else { "Name and topic" }, cx).child(ui::settings_card(cx).p(px(16.)).gap(px(16.)).child(name).children(topic)).into_any_element());
        if category {
            return out;
        }

        if voice {
            let cap = self.guilds.iter().find(|g| g.id == view.guild_id).map_or(96_000, |g| bitrate_cap(g.premium_tier, channel.raw_type == 13));
            let bitrate = props.bitrate.clamp(8_000, cap);
            let fraction = (bitrate - 8_000) as f32 / (cap - 8_000) as f32;
            let bitrate_row = ui::settings_row("cs-bitrate", false, cx)
                .flex_col()
                .items_stretch()
                .gap(px(12.))
                .child(div().flex().items_start().gap(px(16.)).child(ui::settings_text("Bitrate", Some("Higher sounds better and costs slow connections more"), cx)).child(value_text(format!("{} kbps", bitrate / 1000), cx)))
                .child(self.range("cs-bitrate".into(), fraction, crate::settings::Fill::Accent, move |this, f, cx| {
                    let kbps = (8. + f.clamp(0., 1.) * (cap - 8_000) as f32 / 1000.).round() as u32;
                    this.edit_draft(cx, |d| d.props.bitrate = kbps * 1000);
                }, cx));
            let limit_max = if channel.raw_type == 13 { 10_000 } else { 99 };
            let limit = props.user_limit.min(limit_max);
            let limit_row = ui::settings_row("cs-limit", true, cx)
                .flex_col()
                .items_stretch()
                .gap(px(12.))
                .child(div().flex().items_start().gap(px(16.)).child(ui::settings_text("User limit", Some("Members with Move members can still join"), cx)).child(value_text(if limit == 0 { "No limit".into() } else { limit.to_string() }, cx)))
                .child(self.range("cs-limit".into(), limit as f32 / limit_max as f32, crate::settings::Fill::Accent, move |this, f, cx| {
                    let n = (f.clamp(0., 1.) * limit_max as f32).round() as u32;
                    this.edit_draft(cx, |d| d.props.user_limit = n);
                }, cx));
            let quality = ui::settings_row("cs-quality", true, cx)
                .child(ui::settings_text("Video quality", Some("720p sends full quality to everyone watching"), cx))
                .child(ui::segmented_live("cs-quality-seg", &["Auto", "720p"], props.video_full as usize, {
                    let this = cx.entity();
                    move |i, _, cx| this.update(cx, |this, cx| this.edit_draft(cx, |d| d.props.video_full = i == 1))
                }, window, cx));
            let mut regions = vec![(String::new(), "Automatic".to_owned())];
            regions.extend(view.regions.iter().cloned());
            let chosen = props.rtc_region.clone().unwrap_or_default();
            let region_name = regions.iter().find(|(id, _)| *id == chosen).map_or(chosen.clone(), |(_, n)| n.clone());
            let region = ui::settings_row("cs-region", true, cx)
                .child(ui::settings_text("Region", Some("Where the voice server runs"), cx))
                .child(div().w(px(220.)).flex_none().child(self.cs_select(Select::Region, &regions, &chosen, region_name, cx)));
            out.push(ui::settings_group("Sound and video", cx).child(ui::settings_card(cx).child(bitrate_row).child(limit_row).child(quality).child(region)).into_any_element());
        }

        let slow_options: Vec<(String, String)> = SLOWMODE.iter().map(|(s, l)| (s.to_string(), (*l).to_owned())).collect();
        let slow_name = SLOWMODE.iter().find(|(s, _)| *s == props.slowmode).map_or(format!("{} seconds", props.slowmode), |(_, l)| (*l).to_owned());
        let slowmode = ui::settings_row("cs-slowmode", false, cx)
            .child(ui::settings_text("Slowmode", Some("Each member sends one message and starts one thread per interval"), cx))
            .child(div().w(px(220.)).flex_none().child(self.cs_select(Select::Slowmode, &slow_options, &props.slowmode.to_string(), slow_name, cx)));
        let nsfw = ui::settings_switch("cs-nsfw", "Age-restricted", "People confirm they're over 18 before they see it", props.nsfw, true, window, cx).on_click(cx.listener(|this, _, _, cx| this.edit_draft(cx, |d| d.props.nsfw = !d.props.nsfw)));
        let archive = (!voice).then(|| {
            let options: Vec<(String, String)> = ARCHIVE.iter().map(|(m, l)| (m.to_string(), (*l).to_owned())).collect();
            let current = if props.archive_after == 0 { 4320 } else { props.archive_after };
            let label = ARCHIVE.iter().find(|(m, _)| *m == current).map_or("3 days", |(_, l)| l).to_owned();
            ui::settings_row("cs-archive", true, cx)
                .child(ui::settings_text("Hide threads after", Some("New threads close after this long without activity"), cx))
                .child(div().w(px(220.)).flex_none().child(self.cs_select(Select::Archive, &options, &current.to_string(), label, cx)))
        });
        let chat = ui::settings_card(cx).child(slowmode).child(nsfw).children(archive);
        out.push(ui::settings_group(if voice { "Text chat" } else { "Chat" }, cx).child(chat).child(ui::settings_caption("Members with Bypass slowmode aren't slowed down.", cx)).into_any_element());
        out
    }

    // A dropdown on this window: the trigger, and its list rising from it while open.
    fn cs_select(&self, select: Select, options: &[(String, String)], chosen: &str, value: String, cx: &mut Context<Self>) -> AnyElement {
        let open = self.channel_settings.as_ref().and_then(|v| v.select) == Some(select);
        let trigger = ui::select(SharedString::from(format!("cs-select-{select:?}")), None, value, open, cx).on_click(cx.listener(move |this, _, _, cx| {
            if let Some(v) = &mut this.channel_settings {
                v.select = if v.select == Some(select) { None } else { Some(select) };
            }
            cx.notify();
        }));
        let list = open.then(|| {
            let (this, close) = (cx.entity(), cx.entity());
            let dismiss = move |_: &mut Window, cx: &mut App| {
                close.update(cx, |this, cx| {
                    if let Some(v) = &mut this.channel_settings {
                        v.select = None;
                    }
                    cx.notify();
                })
            };
            ui::select_options(SharedString::from(format!("cs-options-{select:?}")), options, chosen, 40., move |id, _, cx| this.update(cx, |this, cx| this.pick(select, &id, cx)), dismiss, cx)
        });
        div().relative().w_full().child(trigger).children(list).into_any_element()
    }

    fn pick(&mut self, select: Select, id: &str, cx: &mut Context<Self>) {
        match select {
            Select::Slowmode => self.edit_draft(cx, |d| d.props.slowmode = id.parse().unwrap_or(0)),
            Select::Archive => self.edit_draft(cx, |d| d.props.archive_after = id.parse().unwrap_or(4320)),
            Select::Region => self.edit_draft(cx, |d| d.props.rtc_region = (!id.is_empty()).then(|| id.to_owned())),
            Select::AddRole => {
                let id = id.to_owned();
                self.edit_draft(cx, |d| {
                    if !d.overwrites.iter().any(|o| o.id == id) {
                        d.overwrites.push(Overwrite { id: id.clone(), ..Default::default() });
                    }
                });
                if let Some(view) = &mut self.channel_settings {
                    view.target = Some(id);
                }
            }
        }
    }

    fn permissions_page(&self, channel: &Channel, guild_id: &str, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let view = self.channel_settings.as_ref().expect("channel settings are open");
        let Some(guild) = self.guilds.iter().find(|g| g.id == guild_id) else { return Vec::new() };
        let overwrites = view.draft.overwrites.clone();
        let mut out = Vec::new();

        let parent = guild.channels.iter().find(|c| Some(&c.id) == channel.parent_id.as_ref());
        if let Some(parent) = parent {
            let notice = if synced(&overwrites, &parent.overwrites) {
                ui::callout(Notice::Info, Glyph::Link, format!("Synced with {}", parent.name), "Changes to the category apply here too.", cx)
            } else {
                let parent_overwrites = parent.overwrites.clone();
                let sync = ui::button("cs-sync", Tone::Secondary, cx).small().child(format!("Sync with {}", parent.name)).on_click(cx.listener(move |this, _, _, cx| {
                    let next = parent_overwrites.clone();
                    this.edit_draft(cx, |d| d.overwrites = next);
                }));
                ui::callout(Notice::Warning, Glyph::Link, format!("Not synced with {}", parent.name), "This channel's permissions differ from its category.", cx).child(div().flex_none().child(sync))
            };
            out.push(notice.into_any_element());
        }

        let (everyone, raw_type) = (guild.id.clone(), channel.raw_type);
        let private_on = private(&overwrites, &everyone);
        let private_row = ui::settings_switch("cs-private", "Private channel", "Only the roles and members you add can see it", private_on, false, window, cx).on_click(cx.listener(move |this, _, _, cx| {
            let everyone = everyone.clone();
            this.edit_draft(cx, |d| set_private(&mut d.overwrites, &everyone, raw_type, !private_on));
        }));
        out.push(ui::settings_card(cx).child(private_row).into_any_element());

        // Targets: @everyone first, then roles highest first, then members.
        let role = |id: &str| guild.roles.iter().find(|r| r.id == id);
        // Discord always lists @everyone, overwrite or not; editing it creates one.
        let mut shown = overwrites.clone();
        if !shown.iter().any(|o| o.id == guild.id) {
            shown.push(Overwrite { id: guild.id.clone(), ..Default::default() });
        }
        let mut targets: Vec<&Overwrite> = shown.iter().collect();
        targets.sort_by_key(|o| (o.member, if o.id == guild.id { i64::MIN } else { -role(&o.id).map_or(0, |r| r.position) }));
        let selected = view.target.clone().filter(|t| shown.iter().any(|o| &o.id == t)).or_else(|| targets.first().map(|o| o.id.clone()));
        let name_of = |o: &Overwrite| -> String {
            match role(&o.id) {
                Some(r) => r.name.clone(),
                None if o.member => self.overwrite_member_name(guild_id, &o.id),
                None => "Unknown role".into(),
            }
        };
        let dark = cx.theme().is_dark();
        let rows: Vec<AnyElement> = targets
            .iter()
            .map(|o| {
                let id = o.id.clone();
                let color = role(&o.id).filter(|r| r.color != 0).map_or(palette::faint(cx), |r| palette::role_ink(r.color, dark));
                let mark = if o.member { div().child(Icon::new(Glyph::User).size(px(16.))).text_color(palette::muted(cx)) } else { div().size(px(12.)).m(px(2.)).rounded_full().bg(color) };
                ui::menu_row(SharedString::from(format!("cs-target-{id}")), cx)
                    .toggled(selected.as_deref() == Some(o.id.as_str()), cx)
                    .child(mark.flex_none())
                    .child(div().flex_1().min_w_0().truncate().child(name_of(o)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(view) = &mut this.channel_settings {
                            view.target = Some(id.clone());
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        let addable: Vec<(String, String)> = {
            let mut roles: Vec<_> = guild.roles.iter().filter(|r| !overwrites.iter().any(|o| o.id == r.id)).collect();
            roles.sort_by_key(|r| -r.position);
            roles.into_iter().map(|r| (r.id.clone(), r.name.clone())).collect()
        };
        let add = (!addable.is_empty()).then(|| div().mt(px(8.)).child(self.cs_select(Select::AddRole, &addable, "", "Add a role".into(), cx)));
        let list = div()
            .w(px(200.))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(div().px(px(4.)).pb(px(4.)).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child("Roles and members"))
            .children(rows)
            .children(add);

        let editor = selected.and_then(|id| shown.iter().find(|o| o.id == id).cloned()).map(|target| {
            let who = name_of(&target);
            let groups: Vec<AnyElement> = permission_groups(channel.raw_type)
                .into_iter()
                .map(|(title, perms)| {
                    let rows = perms.iter().enumerate().map(|(i, &(bit, label))| {
                        let (id, member) = (target.id.clone(), target.member);
                        ui::settings_row(SharedString::from(format!("cs-perm-{title}-{bit}")), i > 0, cx)
                            .child(ui::settings_text(label, None::<&str>, cx))
                            .child(ui::tri_state(SharedString::from(format!("cs-tri-{title}-{bit}")), tri(&overwrites, &target.id, bit), {
                                let this = cx.entity();
                                move |value, _, cx| {
                                    let id = id.clone();
                                    this.update(cx, |this, cx| this.edit_draft(cx, |d| set_tri(&mut d.overwrites, &id, member, bit, value)));
                                }
                            }, cx))
                    });
                    ui::settings_group(title, cx).child(ui::settings_card(cx).children(rows)).into_any_element()
                })
                .collect();
            let remove_id = target.id.clone();
            let remove = (target.id != guild.id).then(|| {
                ui::button("cs-remove-target", Tone::Ghost, cx).small().child(Icon::new(Glyph::Trash).size(px(16.))).child(format!("Remove {who}")).on_click(cx.listener(move |this, _, _, cx| {
                    let id = remove_id.clone();
                    this.edit_draft(cx, |d| d.overwrites.retain(|o| o.id != id));
                }))
            });
            div().flex_1().min_w_0().flex().flex_col().gap(px(24.)).children(groups).child(div().flex().justify_end().children(remove))
        });
        out.push(div().flex().items_start().gap(px(24.)).child(list).children(editor).into_any_element());
        out
    }

    fn overwrite_member_name(&self, guild_id: &str, user_id: &str) -> String {
        self.members.get(guild_id).and_then(|m| m.get(user_id)).map_or_else(|| "Member".into(), |u| u.name.clone())
    }

    fn invites_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let view = self.channel_settings.as_ref().expect("channel settings are open");
        let invites = match &view.invites {
            Load::Loading => return vec![loading_line("Loading invites…", cx)],
            Load::Failed(error) => return vec![ui::callout(Notice::Danger, Glyph::WarningCircle, "Couldn't load invites", error.clone(), cx).into_any_element()],
            Load::Ready(list) if list.is_empty() => return vec![ui::settings_caption("No invites to this channel yet. Invite people from the channel's menu.", cx).into_any_element()],
            Load::Ready(list) => list.clone(),
        };
        let now = chrono::Utc::now();
        let rows = invites.into_iter().enumerate().map(|(i, invite)| {
            let uses = if invite.max_uses == 0 { invite.uses.to_string() } else { format!("{} of {}", invite.uses, invite.max_uses) };
            let code = invite.code.clone();
            ui::settings_row(SharedString::from(format!("cs-invite-{}", invite.code)), i > 0, cx)
                .child(ui::settings_text(invite.code.clone(), Some(invite.inviter.clone()), cx))
                .child(div().w(px(80.)).flex_none().text_color(palette::text(cx)).child(uses))
                .child(div().w(px(90.)).flex_none().text_color(palette::muted(cx)).child(expires_label(invite.expires_at.as_deref(), now)))
                .child(ui::button(SharedString::from(format!("cs-revoke-{}", invite.code)), Tone::Secondary, cx).small().child("Revoke").on_click(cx.listener(move |this, _, _, cx| {
                    let code = code.clone();
                    this.rest(Method::DELETE, format!("/invites/{code}"), None, move |this, cx| {
                        if let Some(Load::Ready(list)) = this.channel_settings.as_mut().map(|v| &mut v.invites) {
                            list.retain(|i| i.code != code);
                        }
                        cx.notify();
                    }, cx);
                })))
        });
        vec![ui::settings_card(cx).children(rows).into_any_element()]
    }

    fn webhooks_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let view = self.channel_settings.as_ref().expect("channel settings are open");
        let hooks = match &view.webhooks {
            Load::Loading => return vec![loading_line("Loading webhooks…", cx)],
            Load::Failed(error) => return vec![ui::callout(Notice::Danger, Glyph::WarningCircle, "Couldn't load webhooks", error.clone(), cx).into_any_element()],
            Load::Ready(list) if list.is_empty() => return vec![ui::settings_caption("No webhooks post here. Creating one stays in Discord for now.", cx).into_any_element()],
            Load::Ready(list) => list.clone(),
        };
        let rows = hooks.into_iter().enumerate().map(|(i, hook)| {
            let (id, url) = (hook.id.clone(), hook.url.clone());
            let copy = url.map(|url| ui::button(SharedString::from(format!("cs-hook-copy-{}", hook.id)), Tone::Secondary, cx).small().child("Copy URL").on_click(cx.listener(move |this, _, _, cx| this.copy_text(url.clone(), cx))));
            ui::settings_row(SharedString::from(format!("cs-hook-{}", hook.id)), i > 0, cx)
                .child(ui::icon_tile(Glyph::AppWindow, palette::selected(cx), palette::strong(cx)).flex_none())
                .child(ui::settings_text(hook.name.clone(), hook.by.as_ref().map(|by| format!("Made by {by}")), cx))
                .children(copy)
                .child(ui::icon_button(SharedString::from(format!("cs-hook-del-{}", hook.id)), Glyph::Trash, format!("Delete {}", hook.name), cx).on_click(cx.listener(move |this, _, _, cx| {
                    let id = id.clone();
                    this.rest(Method::DELETE, format!("/webhooks/{id}"), None, move |this, cx| {
                        if let Some(Load::Ready(list)) = this.channel_settings.as_mut().map(|v| &mut v.webhooks) {
                            list.retain(|h| h.id != id);
                        }
                        cx.notify();
                    }, cx);
                })))
        });
        vec![ui::settings_group("Webhooks", cx).child(ui::settings_card(cx).children(rows)).child(ui::settings_caption("Creating webhooks and following channels stay in Discord for now.", cx)).into_any_element()]
    }

    // Floats over the page bottom while there's something to save, saving, or a refusal to show.
    fn save_bar(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let view = self.channel_settings.as_ref()?;
        let dirty = self.channel_dirty(cx);
        if !dirty && view.save == Save::Idle {
            return None;
        }
        let saving = view.save == Save::Saving;
        let (glyph, ink, title, detail): (Glyph, _, &str, Option<String>) = match &view.save {
            Save::Failed(error) => (Glyph::WarningCircle, palette::danger_ink(cx), "Couldn't save", Some(error.clone())),
            Save::Saving => (Glyph::Pen, palette::muted(cx), "Saving…", None),
            Save::Idle if view.closing => (Glyph::Pen, palette::accent_ink(cx), "Save before you close?", Some("Your changes are lost otherwise.".into())),
            Save::Idle => (Glyph::Pen, palette::muted(cx), "Unsaved changes", None),
        };
        let closing = view.closing;
        let reset = ui::button("cs-reset", Tone::Ghost, cx)
            .disabled(saving)
            .child(if closing { "Discard changes" } else { "Reset" })
            .on_click(cx.listener(move |this, _, window, cx| {
                if closing {
                    this.close_channel_settings(true, cx);
                } else {
                    this.reset_channel_draft(window, cx);
                }
            }));
        let save = ui::button("cs-save", Tone::Primary, cx).loading(saving.then_some("Saving")).child("Save changes").on_click(cx.listener(move |this, _, _, cx| this.save_channel_settings(closing, cx)));
        let bar = div()
            .w_full()
            .max_w(px(ui::SETTINGS_WIDTH))
            .py(px(10.))
            .pl(px(16.))
            .pr(px(10.))
            .squircle(ui::RADIUS_LG)
            .bg(palette::surface(Level::Overlay, cx))
            .shadow(ui::shadow_pop(cx))
            .flex()
            .items_center()
            .gap(px(12.))
            .child(div().text_color(ink).flex_none().child(Icon::new(glyph).size(px(20.))))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(title))
                    .children(detail.map(|d| div().text_size(px(12.)).line_height(px(17.)).text_color(palette::muted(cx)).child(d))),
            )
            .child(div().flex().gap(px(8.)).flex_none().child(reset).child(save));
        Some(ui::rise(div().absolute().bottom(px(16.)).left(px(40.)).right(px(40.)).child(bar), "cs-savebar-in", std::time::Duration::ZERO))
    }
}

fn channel_glyph(channel: &Channel) -> Glyph {
    match (channel.kind, channel.raw_type) {
        (ChannelKind::Category, _) => Glyph::Folder,
        (ChannelKind::Forum, _) => Glyph::Forum,
        (_, 2 | 13) => Glyph::VolumeHigh,
        _ if channel.private => Glyph::HashtagLock,
        _ => Glyph::Hashtag,
    }
}

fn load_list<T>(result: Result<Value, String>, parse: fn(&Value) -> Option<T>) -> Load<Vec<T>> {
    match result {
        Ok(v) => Load::Ready(v.as_array().into_iter().flatten().filter_map(parse).collect()),
        Err(e) => Load::Failed(e),
    }
}


fn value_text(text: String, cx: &App) -> Div {
    div().flex_none().whitespace_nowrap().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(text)
}

fn loading_line(text: &'static str, cx: &App) -> AnyElement {
    div().px(px(4.)).text_color(palette::muted(cx)).child(text).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(overwrites: Vec<Overwrite>) -> Channel {
        parse_channel(&json!({ "id": "c", "type": 0, "name": "clips", "topic": "Plays", "rate_limit_per_user": 0 }), "g").map(|c| Channel { overwrites, ..c }).unwrap()
    }

    #[test]
    fn patch_sends_only_what_changed() {
        let before = channel(Vec::new());
        assert!(patch_body(&before, &Draft::of(&before)).is_empty());
        let mut draft = Draft::of(&before);
        draft.name = "  highlights ".into();
        draft.topic = "".into();
        draft.props.slowmode = 30;
        draft.props.video_full = true;
        let body = Value::Object(patch_body(&before, &draft));
        assert_eq!(body, json!({ "name": "highlights", "topic": null, "rate_limit_per_user": 30, "video_quality_mode": 2 }));
    }

    #[test]
    fn search_finds_channel_settings_by_synonym() {
        let first = |q: &str| registry::search(CHANNEL_SETTINGS, q).first().map(|s| s.id);
        assert_eq!(first("nsfw"), Some("age-restricted"));
        assert_eq!(first("cooldown"), Some("slowmode"));
        assert_eq!(first("kbps"), Some("bitrate"));
        let mut ids: Vec<&str> = CHANNEL_SETTINGS.iter().map(|s| s.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), CHANNEL_SETTINGS.len());
    }

    #[test]
    fn voice_and_text_settings_stay_apart() {
        let text = channel(Vec::new());
        let voice = Channel { raw_type: 2, kind: ChannelKind::Voice, ..channel(Vec::new()) };
        assert!(applies("channel-topic", &text) && !applies("bitrate", &text));
        assert!(applies("bitrate", &voice) && !applies("channel-topic", &voice));
    }

    #[test]
    fn pages_follow_permissions() {
        assert_eq!(pages(0), Vec::<Page>::new());
        assert_eq!(pages(perm::MANAGE_ROLES), vec![Page::Permissions]);
        assert_eq!(pages(perm::MANAGE_CHANNELS | perm::MANAGE_WEBHOOKS), vec![Page::Overview, Page::Invites, Page::Webhooks]);
        assert_eq!(pages(u64::MAX).len(), 4);
    }

    #[test]
    fn caps_and_labels() {
        assert_eq!(bitrate_cap(0, false), 96_000);
        assert_eq!(bitrate_cap(3, false), 384_000);
        assert_eq!(bitrate_cap(3, true), 64_000);
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-18T12:00:00Z").unwrap().with_timezone(&chrono::Utc);
        assert_eq!(expires_label(None, now), "Never");
        assert_eq!(expires_label(Some("2026-11-21T12:30:00Z"), now), "in 3 days");
        assert_eq!(expires_label(Some("2026-11-18T14:00:00+00:00"), now), "in 2h");
        assert_eq!(expires_label(Some("2026-11-18T11:00:00Z"), now), "Expired");
    }
}
