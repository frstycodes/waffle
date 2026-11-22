// The list pages: expressions, members and prune, invites, apps and webhooks, AutoMod, the audit log,
// bans, community and onboarding, and the template. Each loads when first opened and acts at once:
// what can't be taken back asks in place first, and what can offers Undo.
use std::collections::HashSet;

use gpui_kit::{
    AnyElement, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window,
    component::{
        Icon,
        input::{InputEvent, InputState},
    },
    div, px,
};
use serde_json::Value;

use super::{
    access::{self, Page},
    api::{self, AuditQuery, AutomodAction, AutomodRule, Trigger, automod},
    audit::{self, Directory},
    perms,
    view::{Select, failed, link_out, loading_line, mime_of, plural, sentence_spans, snowflake_time},
};
use crate::{
    Waffle,
    channel_settings::{Load, expires_label},
    discord::{ChannelKind, avatar_url, perm},
    icons::Glyph,
    palette,
    ui::{self, Tone},
};

#[derive(Clone, Debug, PartialEq)]
pub struct Expression {
    pub id: String,
    pub name: String,
    pub url: Option<String>,
    pub by_id: Option<String>,
    pub by: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Person {
    pub id: String,
    pub name: String,
    pub avatar: Option<String>,
    // A ban's reason, or a member's join date.
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Invite {
    pub code: String,
    pub inviter: String,
    pub channel: String,
    pub uses: u64,
    pub max_uses: u64,
    pub expires_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hook {
    pub id: String,
    pub name: String,
    pub channel_id: String,
    pub by: Option<String>,
    pub url: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub trigger_type: u8,
    pub words: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Template {
    pub code: String,
    pub name: String,
    pub dirty: bool,
}

#[derive(Default)]
pub struct AuditView {
    pub entries: Vec<audit::Entry>,
    pub names: Directory,
    pub query: AuditQuery,
    pub next: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
    pub open: HashSet<String>,
}

pub struct Lists {
    pub emojis: Load<Vec<Expression>>,
    pub stickers: Load<Vec<Expression>>,
    pub sounds: Load<Vec<Expression>>,
    pub members: Load<Vec<Person>>,
    pub member_search: Entity<InputState>,
    pub prune_days: u8,
    pub prune_count: Option<Load<u64>>,
    pub invites: Load<Vec<Invite>>,
    pub integrations: Load<Vec<Expression>>,
    pub webhooks: Load<Vec<Hook>>,
    pub rules: Load<Vec<Rule>>,
    pub words: Entity<InputState>,
    pub audit: AuditView,
    pub bans: Load<Vec<Person>>,
    pub ban_search: Entity<InputState>,
    pub onboarding: Load<Value>,
    pub templates: Load<Vec<Template>>,
    pub template_name: Entity<InputState>,
    // The row asking "are you sure" in place: its id.
    pub asking: Option<String>,
}

impl Lists {
    pub fn new(window: &mut Window, cx: &mut Context<Waffle>) -> Self {
        let mut input = |placeholder: &'static str| {
            let state = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
            cx.subscribe(&state, |_, _, _: &InputEvent, cx| cx.notify()).detach();
            state
        };
        Lists {
            emojis: Load::Loading,
            stickers: Load::Loading,
            sounds: Load::Loading,
            members: Load::Ready(Vec::new()),
            member_search: input("Search by name or id"),
            prune_days: 7,
            prune_count: None,
            invites: Load::Loading,
            integrations: Load::Loading,
            webhooks: Load::Loading,
            rules: Load::Loading,
            words: input("Words to block, separated by commas"),
            audit: AuditView { loading: true, ..Default::default() },
            bans: Load::Loading,
            ban_search: input("Search bans by name or id"),
            onboarding: Load::Loading,
            templates: Load::Loading,
            template_name: input("Who is this server for? A school club, an art crew"),
            asking: None,
        }
    }
}

fn name_of(user: &Value) -> Option<String> {
    user["global_name"].as_str().or(user["username"].as_str()).map(str::to_owned)
}

pub fn parse_emoji(v: &Value) -> Option<Expression> {
    let id = v["id"].as_str()?.to_owned();
    let ext = if v["animated"].as_bool().unwrap_or(false) { "gif" } else { "png" };
    Some(Expression { url: Some(format!("https://cdn.discordapp.com/emojis/{id}.{ext}?size=48")), name: v["name"].as_str().unwrap_or_default().to_owned(), by_id: v["user"]["id"].as_str().map(str::to_owned), by: name_of(&v["user"]), id })
}

pub fn parse_sticker(v: &Value) -> Option<Expression> {
    let id = v["id"].as_str()?.to_owned();
    Some(Expression { url: Some(format!("https://media.discordapp.net/stickers/{id}.png?size=80")), name: v["name"].as_str().unwrap_or_default().to_owned(), by_id: v["user"]["id"].as_str().map(str::to_owned), by: name_of(&v["user"]), id })
}

pub fn parse_sound(v: &Value) -> Option<Expression> {
    let sound = crate::discord::SoundboardSound::parse(v)?;
    Some(Expression { id: sound.id, name: sound.name, url: None, by_id: v["user"]["id"].as_str().map(str::to_owned), by: name_of(&v["user"]) })
}

pub fn parse_member(v: &Value) -> Option<Person> {
    let user = &v["user"];
    let id = user["id"].as_str()?.to_owned();
    let name = v["nick"].as_str().map(str::to_owned).or_else(|| name_of(user)).unwrap_or_else(|| id.clone());
    Some(Person { avatar: user["avatar"].as_str().map(|h| avatar_url(&id, Some(h))), note: v["joined_at"].as_str().map(|t| t.get(..10).unwrap_or(t).to_owned()), name, id })
}

pub fn parse_ban(v: &Value) -> Option<Person> {
    let user = &v["user"];
    let id = user["id"].as_str()?.to_owned();
    Some(Person { name: name_of(user).unwrap_or_else(|| id.clone()), avatar: user["avatar"].as_str().map(|h| avatar_url(&id, Some(h))), note: v["reason"].as_str().map(str::to_owned), id })
}

pub fn parse_invite(v: &Value) -> Option<Invite> {
    Some(Invite {
        code: v["code"].as_str()?.to_owned(),
        inviter: name_of(&v["inviter"]).unwrap_or_else(|| "Unknown".into()),
        channel: v["channel"]["name"].as_str().map(|n| format!("#{n}")).unwrap_or_default(),
        uses: v["uses"].as_u64().unwrap_or(0),
        max_uses: v["max_uses"].as_u64().unwrap_or(0),
        expires_at: v["expires_at"].as_str().map(str::to_owned),
    })
}

pub fn parse_integration(v: &Value) -> Option<Expression> {
    Some(Expression { id: v["id"].as_str()?.to_owned(), name: v["name"].as_str().unwrap_or("App").to_owned(), url: None, by_id: None, by: name_of(&v["user"]) })
}

pub fn parse_hook(v: &Value) -> Option<Hook> {
    if crate::announcements::is_follower_webhook(v) {
        return None;
    }
    let id = v["id"].as_str()?.to_owned();
    Some(Hook {
        url: v["token"].as_str().map(|token| format!("https://discord.com/api/webhooks/{id}/{token}")),
        name: v["name"].as_str().unwrap_or("Webhook").to_owned(),
        channel_id: v["channel_id"].as_str().unwrap_or_default().to_owned(),
        by: name_of(&v["user"]),
        id,
    })
}

pub fn parse_rule(v: &Value) -> Option<Rule> {
    Some(Rule {
        id: v["id"].as_str()?.to_owned(),
        name: v["name"].as_str().unwrap_or_default().to_owned(),
        enabled: v["enabled"].as_bool().unwrap_or(false),
        trigger_type: v["trigger_type"].as_u64().unwrap_or(0) as u8,
        words: v["trigger_metadata"]["keyword_filter"].as_array().into_iter().flatten().filter_map(|w| w.as_str().map(str::to_owned)).collect(),
    })
}

pub fn parse_template(v: &Value) -> Option<Template> {
    Some(Template { code: v["code"].as_str()?.to_owned(), name: v["name"].as_str().unwrap_or_default().to_owned(), dirty: v["is_dirty"].as_bool().unwrap_or(false) })
}

fn list<T>(result: Result<Value, String>, parse: fn(&Value) -> Option<T>) -> Load<Vec<T>> {
    match result {
        Ok(v) => Load::Ready(v.as_array().into_iter().flatten().filter_map(parse).collect()),
        Err(e) => Load::Failed(e),
    }
}

// Discord's preset rules, in its order: what "Set up" creates for each.
fn preset(trigger_type: u8) -> AutomodRule {
    let block = vec![AutomodAction::Block { custom_message: None }];
    let (name, trigger) = match trigger_type {
        automod::MENTION_SPAM => ("Block mention spam", Trigger::MentionSpam { limit: 20, raid_protection: true }),
        automod::SPAM => ("Block suspected spam content", Trigger::Spam),
        _ => ("Block commonly flagged words", Trigger::Preset { presets: vec![1, 2, 3], allow: Vec::new() }),
    };
    AutomodRule { name: name.into(), enabled: true, trigger, actions: block, exempt_roles: Vec::new(), exempt_channels: Vec::new() }
}

pub fn custom_words(text: &str) -> Vec<String> {
    text.split(',').map(str::trim).filter(|w| !w.is_empty()).map(str::to_owned).collect()
}

const PRUNE_DAYS: [u8; 4] = [1, 7, 14, 30];

impl Waffle {
    fn lists(&mut self) -> Option<&mut Lists> {
        self.server_settings.as_mut().map(|v| &mut v.lists)
    }

    // Asks Discord for what `page` lists, once per opening (Undo and refreshes reload it).
    pub(crate) fn load_server_page(&mut self, page: Page, cx: &mut Context<Self>) {
        let Some(guild_id) = self.server_settings.as_ref().map(|v| v.guild_id.clone()) else { return };
        let g = guild_id.as_str();
        match page {
            Page::Engagement => self.server_call(api::widget(g), |this, result, _| {
                if let Some(view) = &mut this.server_settings {
                    view.widget = match result {
                        Ok(w) => Load::Ready((w["enabled"].as_bool().unwrap_or(false), w["channel_id"].as_str().map(str::to_owned))),
                        Err(e) => Load::Failed(e),
                    };
                }
            }, cx),
            Page::Roles => self.load_roles(cx),
            Page::Emoji => self.server_call(api::emojis(g), |this, r, _| _ = this.lists().map(|l| l.emojis = list(r, parse_emoji)), cx),
            Page::Stickers => self.server_call(api::stickers(g), |this, r, _| _ = this.lists().map(|l| l.stickers = list(r, parse_sticker)), cx),
            Page::Soundboard => self.server_call(api::sounds(g), |this, r, _| {
                // Discord wraps this list: { items: [...] }.
                let r = r.map(|v| v.get("items").cloned().unwrap_or(v));
                _ = this.lists().map(|l| l.sounds = list(r, parse_sound));
            }, cx),
            Page::Invites => self.server_call(api::invites(g), |this, r, _| _ = this.lists().map(|l| l.invites = list(r, parse_invite)), cx),
            Page::Integrations => {
                self.server_call(api::integrations(g), |this, r, _| _ = this.lists().map(|l| l.integrations = list(r, parse_integration)), cx);
                let guild = guild_id.clone();
                self.server_call(api::webhooks(g), move |this, r, _| {
                    this.store_followed_channels(guild, &r);
                    _ = this.lists().map(|l| l.webhooks = list(r, parse_hook));
                }, cx);
            }
            Page::AutoMod => self.server_call(api::automod_rules(g), |this, r, _| _ = this.lists().map(|l| l.rules = list(r, parse_rule)), cx),
            Page::AuditLog => {
                if let Some(l) = self.lists() {
                    l.audit = AuditView { query: std::mem::take(&mut l.audit.query), loading: true, ..Default::default() };
                }
                self.load_audit(cx);
            }
            Page::Bans => self.server_call(api::bans(g, None), |this, r, _| _ = this.lists().map(|l| l.bans = list(r, parse_ban)), cx),
            Page::Onboarding => self.server_call(api::onboarding(g), |this, r, _| _ = this.lists().map(|l| l.onboarding = match r { Ok(v) => Load::Ready(v), Err(e) => Load::Failed(e) }), cx),
            Page::Template => self.server_call(api::templates(g), |this, r, _| _ = this.lists().map(|l| l.templates = list(r, parse_template)), cx),
            _ => {}
        }
    }

    // Appends the next page of the log (or the first, after a filter change).
    fn load_audit(&mut self, cx: &mut Context<Self>) {
        let Some(view) = &self.server_settings else { return };
        let call = api::audit_log(&view.guild_id, &view.lists.audit.query);
        let guild = self.guilds.iter().find(|g| g.id == view.guild_id).cloned();
        self.server_call(call, move |this, result, _| {
            let Some(l) = this.lists() else { return };
            l.audit.loading = false;
            match result {
                Ok(body) => {
                    let mut page = audit::parse(&body);
                    if let Some(guild) = &guild {
                        page.names.roles.extend(guild.roles.iter().map(|r| (r.id.clone(), r.name.clone())));
                        page.names.channels.extend(guild.channels.iter().map(|c| (c.id.clone(), c.name.clone())));
                    }
                    l.audit.next = audit::next_before(&page);
                    l.audit.entries.extend(page.entries);
                    merge(&mut l.audit.names, page.names);
                    l.audit.error = None;
                }
                Err(e) => l.audit.error = Some(e),
            }
        }, cx);
    }

    pub(crate) fn list_pick(&mut self, select: Select, id: &str, cx: &mut Context<Self>) {
        let Some(view) = &mut self.server_settings else { return };
        view.select = None;
        let some = (!id.is_empty()).then(|| id.to_owned());
        match select {
            Select::AuditUser | Select::AuditAction => {
                let mut query = std::mem::take(&mut view.lists.audit.query);
                if select == Select::AuditUser {
                    query.user_id = some;
                } else {
                    query.action_type = id.parse().ok();
                }
                query.before = None;
                view.lists.audit = AuditView { query, loading: true, names: std::mem::take(&mut view.lists.audit.names), ..Default::default() };
                self.load_audit(cx);
            }
            Select::PruneDays => {
                view.lists.prune_days = id.parse().unwrap_or(7);
                view.lists.prune_count = None;
            }
            _ => {}
        }
        cx.notify();
    }

    fn ask(&mut self, id: impl Into<String>, cx: &mut Context<Self>) {
        let id = id.into();
        if let Some(l) = self.lists() {
            l.asking = if l.asking.as_deref() == Some(id.as_str()) { None } else { Some(id) };
        }
        cx.notify();
    }

    fn asking(&self, id: &str) -> bool {
        self.server_settings.as_ref().and_then(|v| v.lists.asking.as_deref()) == Some(id)
    }

    // Runs a change that can't be taken back, then drops the row and reloads the page from Discord.
    fn act_and_reload(&mut self, call: api::Call, page: Page, cx: &mut Context<Self>) {
        self.server_call(call, move |this, result, cx| {
            if let Some(l) = this.lists() {
                l.asking = None;
            }
            match result {
                Ok(_) => this.reload_server_page(page, cx),
                Err(error) => this.show_toast(error, cx),
            }
        }, cx);
    }

    pub(crate) fn list_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        match page {
            Page::Emoji | Page::Stickers | Page::Soundboard => self.ss_expressions_page(page, cx),
            Page::Members => self.ss_members_page(cx),
            Page::Invites => self.ss_invites_page(cx),
            Page::Access => vec![link_out("access", "Rules screening is set up on Discord", "Server rules members agree to before chatting, and age restriction, use routes outside Discord's public API.", "https://discord.com/app", cx)],
            Page::Integrations => self.ss_integrations_page(cx),
            Page::AuditLog => self.ss_audit_page(cx),
            Page::Bans => self.ss_bans_page(cx),
            Page::AutoMod => self.ss_automod_page(window, cx),
            Page::Community => self.ss_community_page(cx),
            Page::Onboarding => self.ss_onboarding_page(cx),
            Page::Insights => {
                let guild_id = self.server_settings.as_ref().map(|v| v.guild_id.clone()).unwrap_or_default();
                vec![link_out("insights", "Server insights open on Discord", "Discord doesn't let user apps read a server's analytics.", api::insights_url(&guild_id), cx)]
            }
            Page::Template => self.ss_template_page(cx),
            _ => Vec::new(),
        }
    }

    fn ss_expressions_page(&mut self, page: Page, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(view) = self.server_settings.as_ref() else { return Vec::new() };
        let guild_id = view.guild_id.clone();
        let (items, noun, cap, row_id) = match page {
            Page::Emoji => (&view.lists.emojis, "emoji", 50, "ss-emoji"),
            Page::Stickers => (&view.lists.stickers, "sticker", 5, "ss-stickers"),
            _ => (&view.lists.sounds, "sound", 8, "ss-sounds"),
        };
        let items = match items {
            Load::Loading => return vec![loading_line("Loading…", cx)],
            Load::Failed(error) => return vec![failed("Couldn't load these", error, cx)],
            Load::Ready(items) => items.clone(),
        };
        let can_upload = self.with_access(&guild_id, |me| me.has(perms::MANAGE_GUILD_EXPRESSIONS | perms::CREATE_GUILD_EXPRESSIONS)).unwrap_or(false);
        let upload = ui::button(SharedString::from(format!("{row_id}-upload")), Tone::Primary, cx).small().disabled(!can_upload).child(Icon::new(Glyph::Upload).size(px(16.))).child(format!("Upload {noun}")).on_click(cx.listener(move |this, _, _, cx| this.upload_expression(page, cx)));
        let head = ui::settings_row(row_id, false, cx).child(ui::settings_text(format!("{} of {cap} slots used", items.len()), Some(match page {
            Page::Emoji => "PNG, JPG or GIF up to 256 KB. Names are letters, numbers and underscores.",
            Page::Stickers => "PNG, APNG or GIF, 320 × 320, up to 512 KB.",
            _ => "MP3 or OGG up to 512 KB and 5 seconds.",
        }), cx)).child(upload);
        let rows: Vec<AnyElement> = items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let editable = self.with_access(&guild_id, |me| access::can_edit_expression(me, item.by_id.as_deref())).unwrap_or(false);
                let mark: AnyElement = match &item.url {
                    Some(url) => gpui_kit::img(url.clone()).size(px(32.)).flex_none().into_any_element(),
                    None => div().text_color(palette::muted(cx)).child(Icon::new(Glyph::Soundboard).size(px(20.))).into_any_element(),
                };
                let label = if page == Page::Emoji { format!(":{}:", item.name) } else { item.name.clone() };
                let ask_id = format!("x-{}", item.id);
                let delete = editable.then(|| self.ss_delete_control(&ask_id, &label, match page {
                    Page::Emoji => api::delete_emoji(&guild_id, &item.id),
                    Page::Stickers => api::delete_sticker(&guild_id, &item.id),
                    _ => api::delete_sound(&guild_id, &item.id),
                }, page, cx));
                ui::settings_row(SharedString::from(format!("ss-expr-{}", item.id)), i > 0, cx).child(mark).child(ui::settings_text(label, item.by.as_ref().map(|b| format!("Uploaded by {b}")), cx)).children(delete).into_any_element()
            })
            .collect();
        let empty = rows.is_empty().then(|| ui::settings_caption(format!("No {noun}s yet."), cx));
        vec![ui::settings_card(cx).child(head).into_any_element(), ui::settings_group("Uploaded", cx).child(ui::settings_card(cx).children(rows)).children(empty).into_any_element()]
    }

    // Delete, then in place: Keep / Delete <name>.
    fn ss_delete_control(&self, ask_id: &str, label: &str, call: api::Call, page: Page, cx: &mut Context<Self>) -> AnyElement {
        let ask = ask_id.to_owned();
        if !self.asking(ask_id) {
            return ui::icon_button(SharedString::from(format!("ss-{ask_id}")), Glyph::Trash, format!("Delete {label}"), cx).on_click(cx.listener(move |this, _, _, cx| this.ask(ask.clone(), cx))).into_any_element();
        }
        let keep = ui::button(SharedString::from(format!("ss-keep-{ask_id}")), Tone::Secondary, cx).small().child("Keep").on_click(cx.listener(move |this, _, _, cx| this.ask(ask.clone(), cx)));
        let call = std::cell::Cell::new(Some(call));
        let yes = ui::button(SharedString::from(format!("ss-yes-{ask_id}")), Tone::Danger, cx).small().child("Delete").on_click(cx.listener(move |this, _, _, cx| {
            if let Some(call) = call.take() {
                this.act_and_reload(call, page, cx);
            }
        }));
        div().flex().gap(px(6.)).flex_none().child(keep).child(yes).into_any_element()
    }

    fn upload_expression(&mut self, page: Page, cx: &mut Context<Self>) {
        let Some(guild_id) = self.server_settings.as_ref().map(|v| v.guild_id.clone()) else { return };
        let client = self.client.clone();
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let Ok(bytes) = std::fs::read(&path) else { return };
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("upload");
            let name: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).take(32).collect();
            let result = match page {
                Page::Emoji => Some(api::create_emoji(&guild_id, &name, api::image_data(mime_of(&path), &bytes))),
                Page::Soundboard => Some(api::create_sound(&guild_id, &name, api::image_data(mime_of(&path), &bytes), 1., None)),
                _ => None,
            };
            let outcome = match (result, &client) {
                (Some(call), Some(client)) => call.send(client).await,
                (None, Some(client)) => {
                    let file = path.file_name().and_then(|n| n.to_str()).unwrap_or("sticker.png").to_owned();
                    api::create_sticker(client, &guild_id, &name, "", &name, file, bytes).await
                }
                _ => Err("Not connected to Discord.".into()),
            };
            this.update(cx, |this, cx| match outcome {
                Ok(_) => this.reload_server_page(page, cx),
                Err(error) => this.show_toast(error, cx),
            })
            .ok();
        })
        .detach();
    }

    fn ss_members_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(view) = self.server_settings.as_ref() else { return Vec::new() };
        let guild_id = view.guild_id.clone();
        let l = &view.lists;
        let search = ui::search_field("ss-member-search-field", &l.member_search, cx);
        let find = ui::button("ss-member-find", Tone::Secondary, cx).small().child("Search").on_click(cx.listener(|this, _, _, cx| this.search_members(cx)));
        let (kick, ban, timeout) = self.with_access(&guild_id, |me| (me.has(perm::KICK_MEMBERS), me.has(perm::BAN_MEMBERS), me.has(perm::MODERATE_MEMBERS))).unwrap_or_default();
        let results: Vec<AnyElement> = match &l.members {
            Load::Loading => vec![loading_line("Searching…", cx)],
            Load::Failed(error) => vec![failed("Couldn't search members", error, cx)],
            Load::Ready(people) => people
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let avatar: AnyElement = match &p.avatar {
                        Some(url) => ui::avatar(url.clone(), 32.).flex_none().into_any_element(),
                        None => ui::monogram(&p.name, 32.).flex_none().into_any_element(),
                    };
                    let (g, id, name) = (guild_id.clone(), p.id.clone(), p.name.clone());
                    let mut row = ui::settings_row(SharedString::from(format!("ss-member-{}", p.id)), i > 0, cx).child(avatar).child(ui::settings_text(p.name.clone(), p.note.as_ref().map(|d| format!("Joined {d}")), cx));
                    if timeout {
                        let (g, id, name) = (g.clone(), id.clone(), name.clone());
                        row = row.child(ui::button(SharedString::from(format!("ss-timeout-{id}")), Tone::Secondary, cx).small().child("Time out 1 hour").on_click(cx.listener(move |this, _, _, cx| {
                            let until = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
                            let (g2, id2, label) = (g.clone(), id.clone(), format!("Timed out {name} for an hour"));
                            this.server_call(api::timeout(&g, &id, Some(&until)), move |this, r, cx| match r {
                                Ok(_) => this.offer_undo(label, api::timeout(&g2, &id2, None), cx),
                                Err(e) => this.show_toast(e, cx),
                            }, cx);
                        })));
                    }
                    if kick {
                        row = row.child(self.ss_delete_control(&format!("kick-{id}"), &name, api::kick(&g, &id), Page::Members, cx));
                    }
                    if ban {
                        row = row.child(self.ss_ban_control(&g, &id, &name, cx));
                    }
                    row.into_any_element()
                })
                .collect(),
        };
        let mut out = vec![div().flex().gap(px(12.)).items_center().child(div().flex_1().child(search)).child(find).into_any_element()];
        if !results.is_empty() {
            out.push(ui::settings_group("Results", cx).child(ui::settings_card(cx).children(results)).into_any_element());
        }
        if kick {
            out.push(self.ss_prune_group(cx));
        }
        out
    }

    fn ss_ban_control(&self, guild_id: &str, user_id: &str, name: &str, cx: &mut Context<Self>) -> AnyElement {
        let ask_id = format!("ban-{user_id}");
        let ask = ask_id.clone();
        if !self.asking(&ask_id) {
            return ui::button(SharedString::from(format!("ss-{ask_id}")), Tone::Secondary, cx).small().child("Ban").on_click(cx.listener(move |this, _, _, cx| this.ask(ask.clone(), cx))).into_any_element();
        }
        let (g, u, label) = (guild_id.to_owned(), user_id.to_owned(), format!("Banned {name}"));
        let keep = ui::button(SharedString::from(format!("ss-keep-{ask_id}")), Tone::Secondary, cx).small().child("Keep").on_click(cx.listener(move |this, _, _, cx| this.ask(ask.clone(), cx)));
        let yes = ui::button(SharedString::from(format!("ss-yes-{ask_id}")), Tone::Danger, cx).small().child(format!("Ban {name}")).on_click(cx.listener(move |this, _, _, cx| {
            let (g2, u2, label) = (g.clone(), u.clone(), label.clone());
            this.server_call(api::ban(&g, &u, 0), move |this, r, cx| {
                if let Some(l) = this.lists() {
                    l.asking = None;
                }
                match r {
                    Ok(_) => this.offer_undo(label, api::unban(&g2, &u2), cx),
                    Err(e) => this.show_toast(e, cx),
                }
            }, cx);
        }));
        div().flex().gap(px(6.)).flex_none().child(keep).child(yes).into_any_element()
    }

    fn search_members(&mut self, cx: &mut Context<Self>) {
        let Some(view) = &mut self.server_settings else { return };
        let query = view.lists.member_search.read(cx).value().trim().to_owned();
        if query.is_empty() {
            return;
        }
        view.lists.members = Load::Loading;
        let call = api::search_members(&view.guild_id, &query);
        self.server_call(call, |this, r, _| _ = this.lists().map(|l| l.members = list(r, parse_member)), cx);
        cx.notify();
    }

    fn ss_prune_group(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(view) = self.server_settings.as_ref() else { return div().into_any_element() };
        let (days, guild_id) = (view.lists.prune_days, view.guild_id.clone());
        let options: Vec<(String, String)> = PRUNE_DAYS.iter().map(|d| (d.to_string(), format!("Inactive for {}", plural(u64::from(*d), "day", "days")))).collect();
        let pick = div().w(px(220.)).flex_none().child(self.ss_select(Select::PruneDays, &options, &days.to_string(), cx));
        let action: AnyElement = match &view.lists.prune_count {
            None => ui::button("ss-prune-count", Tone::Secondary, cx).small().child("Count members").on_click(cx.listener(move |this, _, _, cx| {
                let g = guild_id.clone();
                if let Some(l) = this.lists() {
                    l.prune_count = Some(Load::Loading);
                }
                this.server_call(api::prune_count(&g, days, &[]), |this, r, _| _ = this.lists().map(|l| l.prune_count = Some(match r { Ok(v) => Load::Ready(v["pruned"].as_u64().unwrap_or(0)), Err(e) => Load::Failed(e) })), cx);
            })).into_any_element(),
            Some(Load::Loading) => loading_line("Counting…", cx),
            Some(Load::Failed(e)) => failed("Couldn't count", e, cx),
            Some(Load::Ready(0)) => ui::settings_caption("Nobody would be removed.", cx).into_any_element(),
            Some(Load::Ready(n)) => ui::button("ss-prune-go", Tone::Danger, cx).small().child(format!("Remove {}", plural(*n, "member", "members"))).on_click(cx.listener(move |this, _, _, cx| {
                if let Some(l) = this.lists() {
                    l.prune_count = None;
                }
                this.act_and_reload(api::prune(&guild_id, days, &[]), Page::Members, cx);
            })).into_any_element(),
        };
        let row = ui::settings_row("ss-prune", false, cx).child(ui::settings_text("Prune inactive members", Some("Removes members without a role who haven't been online. They can rejoin with an invite."), cx));
        ui::settings_group("Prune", cx).child(ui::settings_card(cx).child(row).child(ui::settings_row("ss-prune-pick", true, cx).child(pick).child(div().flex_1()).child(action))).into_any_element()
    }

    fn ss_invites_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(view) = self.server_settings.as_ref() else { return Vec::new() };
        let guild_id = view.guild_id.clone();
        let invites = match &view.lists.invites {
            Load::Loading => return vec![loading_line("Loading invites…", cx)],
            Load::Failed(error) => return vec![failed("Couldn't load invites", error, cx)],
            Load::Ready(list) => list.clone(),
        };
        let now = chrono::Utc::now();
        let rows: Vec<AnyElement> = invites
            .iter()
            .enumerate()
            .map(|(i, invite)| {
                let uses = if invite.max_uses == 0 { plural(invite.uses, "use", "uses") } else { format!("{} of {} uses", invite.uses, invite.max_uses) };
                let revoke = self.ss_delete_control(&format!("invite-{}", invite.code), &invite.code, api::revoke_invite(&invite.code), Page::Invites, cx);
                ui::settings_row(SharedString::from(format!("ss-invite-{}", invite.code)), i > 0, cx)
                    .child(ui::settings_text(invite.code.clone(), Some(format!("{} · {} · {uses}", invite.inviter, invite.channel)), cx))
                    .child(div().w(px(90.)).flex_none().text_color(palette::muted(cx)).child(expires_label(invite.expires_at.as_deref(), now)))
                    .child(revoke)
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty().then(|| ui::settings_caption("No active invite links.", cx));
        let (g1, g2) = (guild_id.clone(), guild_id);
        let pause = ui::settings_row("ss-pause-invites", false, cx)
            .child(ui::settings_text("Pause invites", Some("Nobody can join with an invite for 24 hours. Useful during a raid."), cx))
            .child(ui::button("ss-pause-24", Tone::Secondary, cx).small().child("Pause for 24 hours").on_click(cx.listener(move |this, _, _, cx| {
                let until = (chrono::Utc::now() + chrono::Duration::hours(24)).to_rfc3339();
                let g = g1.clone();
                this.server_call(api::pause_invites(&g, Some(&until)), move |this, r, cx| match r {
                    Ok(_) => this.offer_undo("Invites paused for 24 hours", api::pause_invites(&g, None), cx),
                    Err(e) => this.show_toast(e, cx),
                }, cx);
            })))
            .child(ui::button("ss-resume", Tone::Ghost, cx).small().child("Resume").on_click(cx.listener(move |this, _, _, cx| this.act_and_reload(api::pause_invites(&g2, None), Page::Invites, cx))));
        vec![
            ui::settings_group("Invite links", cx).child(ui::settings_card(cx).id("ss-invites").children(rows)).children(empty).into_any_element(),
            ui::settings_group("Pause", cx).child(ui::settings_card(cx).child(pause)).into_any_element(),
        ]
    }

    fn ss_integrations_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(view) = self.server_settings.as_ref() else { return Vec::new() };
        let guild_id = view.guild_id.clone();
        let (manage_guild, manage_hooks) = self.with_access(&guild_id, |me| (me.has(perms::MANAGE_GUILD), me.has(perm::MANAGE_WEBHOOKS))).unwrap_or_default();
        let mut out = Vec::new();
        if manage_guild {
            let apps: Vec<AnyElement> = match &view.lists.integrations {
                Load::Loading => vec![loading_line("Loading apps…", cx)],
                Load::Failed(e) => vec![failed("Couldn't load apps", e, cx)],
                Load::Ready(apps) => apps
                    .iter()
                    .enumerate()
                    .map(|(i, app)| {
                        let remove = self.ss_delete_control(&format!("app-{}", app.id), &app.name, api::remove_integration(&guild_id, &app.id), Page::Integrations, cx);
                        ui::settings_row(SharedString::from(format!("ss-app-{}", app.id)), i > 0, cx).child(ui::icon_tile(Glyph::AppWindow, palette::selected(cx), palette::strong(cx)).flex_none()).child(ui::settings_text(app.name.clone(), app.by.as_ref().map(|b| format!("Added by {b}")), cx)).child(remove).into_any_element()
                    })
                    .collect(),
            };
            out.push(ui::settings_group("Bots and apps", cx).child(ui::settings_card(cx).id("ss-integrations").children(apps)).into_any_element());
        }
        if manage_hooks {
            let channels = self.text_channels(&guild_id, ChannelKind::Text);
            let hooks: Vec<AnyElement> = match &view.lists.webhooks {
                Load::Loading => vec![loading_line("Loading webhooks…", cx)],
                Load::Failed(e) => vec![failed("Couldn't load webhooks", e, cx)],
                Load::Ready(hooks) => hooks
                    .iter()
                    .enumerate()
                    .map(|(i, hook)| {
                        let channel = channels.iter().find(|(id, _)| *id == hook.channel_id).map(|(_, n)| n.clone()).unwrap_or_default();
                        let copy = hook.url.clone().map(|url| ui::button(SharedString::from(format!("ss-hook-copy-{}", hook.id)), Tone::Secondary, cx).small().child("Copy URL").on_click(cx.listener(move |this, _, _, cx| this.copy_text(url.clone(), cx))));
                        let remove = self.ss_delete_control(&format!("hook-{}", hook.id), &hook.name, api::delete_webhook(&hook.id), Page::Integrations, cx);
                        let detail = match &hook.by {
                            Some(by) => format!("{channel} · made by {by}"),
                            None => channel,
                        };
                        ui::settings_row(SharedString::from(format!("ss-hook-{}", hook.id)), i > 0, cx).child(ui::settings_text(hook.name.clone(), Some(detail), cx)).children(copy).child(remove).into_any_element()
                    })
                    .collect(),
            };
            let first = channels.first().map(|(id, _)| id.clone());
            let create = first.map(|channel| {
                ui::button("ss-hook-new", Tone::Secondary, cx).small().child(Icon::new(Glyph::Plus).size(px(16.))).child("New webhook").on_click(cx.listener(move |this, _, _, cx| this.act_and_reload(api::create_webhook(&channel, "Captain Hook"), Page::Integrations, cx)))
            });
            out.push(self.followed_channels_group(&guild_id, cx));
            out.push(ui::settings_group("Webhooks", cx).child(ui::settings_card(cx).id("ss-webhooks").children(hooks)).child(div().flex().justify_end().children(create)).into_any_element());
        }
        out.push(link_out("app-directory", "App directory", "Find and add apps on Discord.", format!("https://discord.com/application-directory?guild_id={guild_id}"), cx));
        out
    }

    fn ss_automod_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(view) = self.server_settings.as_ref() else { return Vec::new() };
        let guild_id = view.guild_id.clone();
        let rules = match &view.lists.rules {
            Load::Loading => return vec![loading_line("Loading rules…", cx)],
            Load::Failed(e) => return vec![failed("Couldn't load AutoMod rules", e, cx)],
            Load::Ready(rules) => rules.clone(),
        };
        let mut rows: Vec<AnyElement> = Vec::new();
        for (i, kind) in [automod::MENTION_SPAM, automod::SPAM, automod::KEYWORD_PRESET].into_iter().enumerate() {
            let template = preset(kind);
            match rules.iter().find(|r| r.trigger_type == kind) {
                Some(rule) => rows.push(self.ss_rule_row(&guild_id, rule, i > 0, window, cx)),
                None => {
                    let call = api::create_automod_rule(&guild_id, &template);
                    let call = std::cell::Cell::new(Some(call));
                    rows.push(
                        ui::settings_row(SharedString::from(format!("ss-automod-{kind}")), i > 0, cx)
                            .child(ui::settings_text(template.name.clone(), Some("Not set up"), cx))
                            .child(ui::button(SharedString::from(format!("ss-automod-setup-{kind}")), Tone::Secondary, cx).small().child("Set up").on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(call) = call.take() {
                                    this.act_and_reload(call, Page::AutoMod, cx);
                                }
                            })))
                            .into_any_element(),
                    );
                }
            }
        }
        let custom: Vec<AnyElement> = rules.iter().filter(|r| r.trigger_type == automod::KEYWORD).enumerate().map(|(i, r)| self.ss_rule_row(&guild_id, r, i > 0, window, cx)).collect();
        let words = ui::search_field("ss-automod-words", &view.lists.words, cx);
        let add = ui::button("ss-automod-add", Tone::Primary, cx).small().child("Create rule").on_click(cx.listener(move |this, _, _, cx| {
            let Some(view) = &this.server_settings else { return };
            let words = custom_words(&view.lists.words.read(cx).value());
            if words.is_empty() {
                return;
            }
            let rule = AutomodRule { name: "Block custom words".into(), enabled: true, trigger: Trigger::Keyword { words, regex: Vec::new(), allow: Vec::new() }, actions: vec![AutomodAction::Block { custom_message: None }], exempt_roles: Vec::new(), exempt_channels: Vec::new() };
            let g = view.guild_id.clone();
            this.act_and_reload(api::create_automod_rule(&g, &rule), Page::AutoMod, cx);
        }));
        vec![
            ui::settings_group("Rules", cx).child(ui::settings_card(cx).id("ss-automod").children(rows)).child(ui::settings_caption("Matching messages are blocked before they post. Admins and members with Manage server are exempt.", cx)).into_any_element(),
            ui::settings_group("Custom words", cx).child(ui::settings_card(cx).children(custom)).child(div().flex().gap(px(12.)).items_center().child(div().flex_1().child(words)).child(add)).child(ui::settings_caption("Up to six custom word rules. Separate words with commas; * matches any letters.", cx)).into_any_element(),
        ]
    }

    fn ss_rule_row(&self, guild_id: &str, rule: &Rule, line: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let (g, id, enabled) = (guild_id.to_owned(), rule.id.clone(), rule.enabled);
        let detail = if rule.words.is_empty() { None } else { Some(rule.words.join(", ")) };
        let toggle = ui::switch_live(SharedString::from(format!("ss-rule-switch-{}", rule.id)), enabled, window, cx);
        let delete = self.ss_delete_control(&format!("rule-{}", rule.id), &rule.name, api::delete_automod_rule(guild_id, &rule.id), Page::AutoMod, cx);
        let row = ui::settings_row(SharedString::from(format!("ss-rule-{}", rule.id)), line, cx).child(ui::settings_text(rule.name.clone(), detail, cx)).child(delete).child(toggle);
        // Low stakes and reversible: flips at once, flips back if Discord refuses.
        ui::settings_choice(row, cx)
            .on_click(cx.listener(move |this, _, _, cx| {
                let flip = |this: &mut Waffle, on: bool| {
                    if let Some(Load::Ready(rules)) = this.lists().map(|l| &mut l.rules)
                        && let Some(r) = rules.iter_mut().find(|r| r.id == id) {
                            r.enabled = on;
                        }
                };
                flip(this, !enabled);
                let id2 = id.clone();
                this.server_call(api::set_automod_enabled(&g, &id, !enabled), move |this, r, cx| {
                    if let Err(e) = r {
                        if let Some(Load::Ready(rules)) = this.lists().map(|l| &mut l.rules)
                            && let Some(r) = rules.iter_mut().find(|r| r.id == id2) {
                                r.enabled = enabled;
                            }
                        this.show_toast(e, cx);
                    }
                }, cx);
                cx.notify();
            }))
            .into_any_element()
    }

    fn ss_audit_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(view) = self.server_settings.as_ref() else { return Vec::new() };
        let a = &view.lists.audit;
        let mut users: Vec<(String, String)> = vec![(String::new(), "All members".into())];
        let mut known: Vec<(String, String)> = a.names.users.iter().map(|(id, n)| (id.clone(), n.clone())).collect();
        known.sort_by_key(|x| x.1.to_lowercase());
        users.extend(known);
        let mut actions: Vec<(String, String)> = vec![(String::new(), "All actions".into())];
        actions.extend(audit::ACTIONS.iter().map(|(n, l)| (n.to_string(), (*l).to_owned())));
        let user = a.query.user_id.clone().unwrap_or_default();
        let action = a.query.action_type.map(|n| n.to_string()).unwrap_or_default();
        let filters = div()
            .flex()
            .gap(px(12.))
            .child(div().flex_1().child(ui::field("Filter by member", cx).child(self.ss_select(Select::AuditUser, &users, &user, cx))))
            .child(div().flex_1().child(ui::field("Filter by action", cx).child(self.ss_select(Select::AuditAction, &actions, &action, cx))));
        let Some(view) = self.server_settings.as_ref() else { return Vec::new() };
        let a = &view.lists.audit;
        let mut out = vec![filters.into_any_element()];
        if let Some(error) = &a.error {
            out.push(failed("Couldn't load the audit log", error, cx));
        }
        if a.entries.is_empty() && a.loading {
            out.push(loading_line("Loading the audit log…", cx));
            return out;
        }
        if a.entries.is_empty() && a.error.is_none() {
            out.push(ui::settings_caption("Nothing here. Discord keeps 45 days of changes.", cx).into_any_element());
            return out;
        }
        let rows: Vec<AnyElement> = a.entries.iter().enumerate().map(|(i, e)| self.ss_audit_row(e, &a.names, a.open.contains(&e.id), i > 0, cx)).collect();
        out.push(ui::settings_card(cx).id("ss-audit").children(rows).into_any_element());
        if a.next.is_some() {
            out.push(div().flex().justify_center().child(ui::button("ss-audit-more", Tone::Secondary, cx).loading(a.loading.then_some("Loading")).child("Load more").on_click(cx.listener(|this, _, _, cx| {
                if let Some(l) = this.lists() {
                    l.audit.query.before = l.audit.next.take();
                    l.audit.loading = true;
                }
                this.load_audit(cx);
            }))).into_any_element());
        }
        out
    }

    fn ss_audit_row(&self, e: &audit::Entry, names: &Directory, open: bool, line: bool, cx: &mut Context<Self>) -> AnyElement {
        let (glyph, ink) = match audit::tone(e.action) {
            audit::Tone::Create => (Glyph::PlusCircle, palette::status_in(crate::discord::Status::Online, cx)),
            audit::Tone::Update => (Glyph::Pen, palette::muted(cx)),
            audit::Tone::Delete => (Glyph::MinusBold, palette::danger_ink(cx)),
        };
        let details = audit::details(e, names);
        let id = e.id.clone();
        let chevron = (!details.is_empty()).then(|| div().text_color(palette::muted(cx)).child(Icon::new(if open { Glyph::ChevronDown } else { Glyph::ChevronRight }).size(px(16.))));
        let head = div()
            .flex()
            .items_start()
            .gap(px(12.))
            .w_full()
            .child(div().pt(px(2.)).text_color(ink).flex_none().child(Icon::new(glyph).size(px(18.))))
            .child(div().flex_1().min_w_0().flex().flex_col().gap(px(2.)).child(sentence_spans(&audit::sentence(e, names), cx)).child(div().text_size(px(12.)).text_color(palette::muted(cx)).child(snowflake_time(&e.id))))
            .children(chevron);
        let body = open.then(|| {
            div().pl(px(30.)).pt(px(8.)).flex().flex_col().gap(px(4.)).children(details.into_iter().map(|d| {
                let change = match (&d.old, &d.new) {
                    (Some(old), Some(new)) => div().flex().items_center().gap(px(6.)).child(div().text_color(palette::muted(cx)).line_through().child(old.clone())).child(Icon::new(Glyph::ArrowRight).size(px(12.))).child(div().text_color(palette::strong(cx)).child(new.clone())),
                    (None, Some(new)) => div().text_color(palette::strong(cx)).child(new.clone()),
                    (Some(old), None) => div().text_color(palette::muted(cx)).line_through().child(old.clone()),
                    (None, None) => div().text_color(palette::muted(cx)).child("Cleared"),
                };
                div().flex().gap(px(8.)).text_size(px(14.)).child(div().w(px(150.)).flex_none().text_color(palette::muted(cx)).child(d.label)).child(div().flex_1().min_w_0().child(change))
            }))
        });
        let row = ui::settings_row(SharedString::from(format!("ss-audit-{}", e.id)), line, cx).flex_col().items_stretch().child(head).children(body);
        ui::settings_choice(row, cx)
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(l) = this.lists()
                    && !l.audit.open.remove(&id) {
                        l.audit.open.insert(id.clone());
                    }
                cx.notify();
            }))
            .into_any_element()
    }

    fn ss_bans_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(view) = self.server_settings.as_ref() else { return Vec::new() };
        let guild_id = view.guild_id.clone();
        let bans = match &view.lists.bans {
            Load::Loading => return vec![loading_line("Loading bans…", cx)],
            Load::Failed(e) => return vec![failed("Couldn't load bans", e, cx)],
            Load::Ready(bans) => bans.clone(),
        };
        let query = view.lists.ban_search.read(cx).value().to_lowercase();
        let search = ui::search_field("ss-ban-search", &view.lists.ban_search, cx);
        let shown: Vec<&Person> = bans.iter().filter(|b| query.is_empty() || b.name.to_lowercase().contains(&query) || b.id.contains(&query)).collect();
        let rows: Vec<AnyElement> = shown
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let avatar: AnyElement = match &b.avatar {
                    Some(url) => ui::avatar(url.clone(), 32.).flex_none().into_any_element(),
                    None => ui::monogram(&b.name, 32.).flex_none().into_any_element(),
                };
                let (g, id, label) = (guild_id.clone(), b.id.clone(), format!("Unbanned {}", b.name));
                let unban = ui::button(SharedString::from(format!("ss-unban-{}", b.id)), Tone::Secondary, cx).small().child("Unban").on_click(cx.listener(move |this, _, _, cx| {
                    let (g2, id2, label) = (g.clone(), id.clone(), label.clone());
                    if let Some(Load::Ready(bans)) = this.lists().map(|l| &mut l.bans) {
                        bans.retain(|b| b.id != id2);
                    }
                    this.server_call(api::unban(&g, &id), move |this, r, cx| match r {
                        Ok(_) => this.offer_undo(label, api::ban(&g2, &id2, 0), cx),
                        Err(e) => {
                            this.show_toast(e, cx);
                            this.reload_server_page(Page::Bans, cx);
                        }
                    }, cx);
                    cx.notify();
                }));
                ui::settings_row(SharedString::from(format!("ss-ban-{}", b.id)), i > 0, cx).child(avatar).child(ui::settings_text(b.name.clone(), Some(b.note.clone().unwrap_or_else(|| "No reason given".into())), cx)).child(unban).into_any_element()
            })
            .collect();
        let empty = rows.is_empty().then(|| ui::settings_caption(if bans.is_empty() { "Nobody is banned." } else { "No bans match." }, cx));
        vec![search.into_any_element(), ui::settings_group(plural(bans.len() as u64, "ban", "bans"), cx).child(ui::settings_card(cx).id("ss-bans").children(rows)).children(empty).into_any_element()]
    }

    fn ss_community_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(view) = self.server_settings.as_ref() else { return Vec::new() };
        let guild_id = view.guild_id.clone();
        let Load::Ready(info) = &view.info else { return Vec::new() };
        let features = info.features.clone();
        let channels = self.text_channels(&guild_id, ChannelKind::Text);
        let name = |id: &Option<String>| id.as_ref().and_then(|id| channels.iter().find(|(c, _)| c == id)).map(|(_, n)| n.clone()).unwrap_or_else(|| "None".into());
        if !features.iter().any(|f| f == "COMMUNITY") {
            let enable = ui::button("ss-community-enable", Tone::Primary, cx).child("Enable Community").on_click(cx.listener(move |this, _, window, cx| {
                let call = api::enable_community(&guild_id, &features, None, None);
                this.server_call(call, |this, r, cx| match r {
                    Ok(body) => {
                        if let Some(view) = &mut this.server_settings {
                            view.info = Load::Ready(super::view::Info::parse(&body));
                        }
                    }
                    Err(e) => this.show_toast(e, cx),
                }, cx);
                let _ = window;
            }));
            let rows = [("Verified email to chat", "Turns on the low verification level"), ("Scan every image", "Explicit media is filtered for everyone"), ("Rules and updates channels", "Discord creates #rules and #moderator-only")].map(|(t, d)| ui::settings_row(SharedString::from(format!("ss-community-{t}")), true, cx).child(Icon::new(Glyph::CheckCircle).size(px(18.))).child(ui::settings_text(t, Some(d), cx)).into_any_element());
            return vec![
                ui::settings_card(cx).id("ss-community").child(ui::settings_row("ss-community-head", false, cx).child(ui::settings_text("Are you building a community?", Some("Community servers get onboarding, insights, announcement channels and Discovery."), cx))).children(rows).into_any_element(),
                div().flex().justify_end().child(enable).into_any_element(),
            ];
        }
        let disable = ui::button("ss-community-disable", Tone::Secondary, cx).small().child("Disable Community").on_click(cx.listener(move |this, _, _, cx| {
            let call = api::disable_community(&guild_id, &features);
            this.server_call(call, |this, r, cx| match r {
                Ok(body) => {
                    if let Some(view) = &mut this.server_settings {
                        view.info = Load::Ready(super::view::Info::parse(&body));
                    }
                }
                Err(e) => this.show_toast(e, cx),
            }, cx);
        }));
        vec![
            ui::settings_group("Channels", cx)
                .child(ui::settings_card(cx).id("ss-community").child(ui::settings_row("ss-rules", false, cx).child(ui::settings_text("Rules channel", Some("Where members read the rules"), cx)).child(super::view::value_text(name(&info.rules_channel_id), cx))).child(ui::settings_row("ss-updates", true, cx).child(ui::settings_text("Community updates channel", Some("Where Discord sends notices to moderators"), cx)).child(super::view::value_text(name(&info.updates_channel_id), cx))))
                .into_any_element(),
            ui::settings_card(cx).child(ui::settings_row("ss-community-off", false, cx).child(ui::settings_text("Disable Community", Some("Onboarding, insights and Discovery switch off."), cx)).child(disable)).into_any_element(),
        ]
    }

    fn ss_onboarding_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(view) = self.server_settings.as_ref() else { return Vec::new() };
        let onboarding = match &view.lists.onboarding {
            Load::Loading => return vec![loading_line("Loading onboarding…", cx)],
            Load::Failed(e) => return vec![failed("Couldn't load onboarding", e, cx)],
            Load::Ready(v) => v.clone(),
        };
        let enabled = onboarding["enabled"].as_bool().unwrap_or(false);
        let prompts = onboarding["prompts"].as_array().map_or(0, Vec::len) as u64;
        let defaults = onboarding["default_channel_ids"].as_array().map_or(0, Vec::len) as u64;
        vec![
            ui::settings_card(cx)
                .child(ui::settings_row("onboarding", false, cx).child(ui::settings_text("Onboarding", Some(if enabled { "On: new members answer questions before they arrive" } else { "Off" }), cx)))
                .child(ui::settings_row("ss-onboarding-prompts", true, cx).child(ui::settings_text("Questions", Some(plural(prompts, "question", "questions")), cx)))
                .child(ui::settings_row("ss-onboarding-defaults", true, cx).child(ui::settings_text("Default channels", Some(plural(defaults, "channel", "channels")), cx)))
                .into_any_element(),
            link_out("onboarding-edit", "Edit questions and channels on Discord", "Onboarding's question builder isn't in Waffle yet.", "https://discord.com/app", cx),
        ]
    }

    fn ss_template_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(view) = self.server_settings.as_ref() else { return Vec::new() };
        let guild_id = view.guild_id.clone();
        let templates = match &view.lists.templates {
            Load::Loading => return vec![loading_line("Loading the template…", cx)],
            Load::Failed(e) => return vec![failed("Couldn't load the template", e, cx)],
            Load::Ready(t) => t.clone(),
        };
        let copies = ui::settings_card(cx)
            .child(ui::settings_row("ss-template-copies", false, cx).child(Icon::new(Glyph::CheckCircle).size(px(18.))).child(ui::settings_text("Copies channels, topics, roles, permissions and default settings", None::<&str>, cx)))
            .child(ui::settings_row("ss-template-skips", true, cx).child(Icon::new(Glyph::XCircle).size(px(18.))).child(ui::settings_text("Not messages, members, bots, the icon or boosts", None::<&str>, cx)));
        let mut out = vec![ui::settings_group("What it copies", cx).child(copies).into_any_element()];
        match templates.first() {
            Some(t) => {
                let (g, code) = (guild_id.clone(), t.code.clone());
                let link = format!("https://discord.new/{}", t.code);
                let sync = ui::button("ss-template-sync", Tone::Secondary, cx).small().child("Sync").on_click(cx.listener(move |this, _, _, cx| this.act_and_reload(api::sync_template(&g, &code), Page::Template, cx)));
                let copy = ui::button("ss-template-copy", Tone::Ghost, cx).small().child("Copy link").on_click(cx.listener(move |this, _, _, cx| this.copy_text(link.clone(), cx)));
                let delete = self.ss_delete_control(&format!("template-{}", t.code), &t.name, api::delete_template(&guild_id, &t.code), Page::Template, cx);
                let detail = if t.dirty { "The server changed since the last sync." } else { "In sync with the server." };
                out.push(ui::settings_group("Template", cx).child(ui::settings_card(cx).id("ss-template").child(ui::settings_row("ss-template-row", false, cx).child(ui::settings_text(format!("discord.new/{}", t.code), Some(format!("{} · {detail}", t.name)), cx)).child(sync).child(copy).child(delete))).into_any_element());
            }
            None => {
                let field = ui::search_field("ss-template-name", &view.lists.template_name, cx);
                let generate = ui::button("ss-template-make", Tone::Primary, cx).small().child("Generate template").on_click(cx.listener(move |this, _, _, cx| {
                    let Some(view) = &this.server_settings else { return };
                    let name = view.lists.template_name.read(cx).value().trim().to_owned();
                    if name.is_empty() {
                        return this.show_toast("Give the template a title first.", cx);
                    }
                    this.act_and_reload(api::create_template(&guild_id, &name, ""), Page::Template, cx);
                }));
                out.push(ui::settings_group("Template", cx).child(div().id("ss-template").flex().gap(px(12.)).items_center().child(div().flex_1().child(field)).child(generate)).into_any_element());
            }
        }
        out
    }
}

fn merge(into: &mut Directory, from: Directory) {
    into.users.extend(from.users);
    into.roles.extend(from.roles);
    into.channels.extend(from.channels);
    into.webhooks.extend(from.webhooks);
    into.integrations.extend(from.integrations);
    into.rules.extend(from.rules);
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_lists() {
        let e = parse_emoji(&json!({ "id": "5", "name": "wave", "animated": true, "user": { "id": "u", "username": "mika" } })).unwrap();
        assert_eq!((e.url.as_deref(), e.by.as_deref()), (Some("https://cdn.discordapp.com/emojis/5.gif?size=48"), Some("mika")));
        let b = parse_ban(&json!({ "reason": null, "user": { "id": "9", "username": "spam_bot" } })).unwrap();
        assert_eq!((b.name.as_str(), b.note), ("spam_bot", None));
        let m = parse_member(&json!({ "nick": "Wren", "joined_at": "2026-01-02T03:04:05Z", "user": { "id": "3", "username": "wren" } })).unwrap();
        assert_eq!((m.name.as_str(), m.note.as_deref()), ("Wren", Some("2026-01-02")));
        let r = parse_rule(&json!({ "id": "1", "name": "Words", "enabled": true, "trigger_type": 1, "trigger_metadata": { "keyword_filter": ["a", "b"] } })).unwrap();
        assert_eq!(r.words, vec!["a", "b"]);
        let h = parse_hook(&json!({ "id": "7", "token": "t", "channel_id": "c" })).unwrap();
        assert_eq!(h.url.as_deref(), Some("https://discord.com/api/webhooks/7/t"));
    }

    #[test]
    fn custom_words_split() {
        assert_eq!(custom_words(" spoiler,  , leak* ,x"), vec!["spoiler", "leak*", "x"]);
        assert!(custom_words(" , ").is_empty());
    }

    #[test]
    fn presets_match_their_trigger() {
        assert_eq!(preset(automod::MENTION_SPAM).body()["trigger_type"], 5);
        assert_eq!(preset(automod::SPAM).body()["trigger_type"], 3);
        assert_eq!(preset(automod::KEYWORD_PRESET).body()["trigger_metadata"]["presets"], json!([1, 2, 3]));
    }
}
