// The server settings window on the shared settings shell. The profile, Engagement, Boost perks and
// Safety pages edit one draft of the server (the save bar sends only what changed); a role's editor keeps
// its own draft in the same bar. List pages load when first opened and act at once, confirming deletes in
// place and offering Undo where Discord can take the change back.
use std::collections::HashSet;

use gpui_kit::{
    AnyElement, App, AppContext, Context, Div, Entity, Focusable, FontWeight, IntoElement, ParentElement, ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Window,
    component::{
        Icon,
        input::{Input, InputEvent, InputState, Textarea, TextareaState},
    },
    div, px,
};
use serde_json::Value;

use super::{
    access::{self, Me, Page},
    api::{self, Call, Overview},
    audit,
    lists::Lists,
    registry::SERVER_SETTINGS,
    roles::RolesView,
};
use crate::{
    Waffle,
    channel_settings::Load,
    discord::ChannelKind,
    icons::Glyph,
    palette::{self, Level},
    settings::registry,
    ui::{self, Notice, Tone},
};

// What GET /guilds/{id} tells the window beyond the draft.
#[derive(Clone, Debug, Default)]
pub struct Info {
    pub saved: Overview,
    pub features: Vec<String>,
    pub premium_tier: u8,
    pub boosts: u64,
    pub vanity: Option<String>,
    pub rules_channel_id: Option<String>,
    pub updates_channel_id: Option<String>,
}

impl Info {
    pub fn parse(g: &Value) -> Self {
        Info {
            saved: Overview::parse(g),
            features: g["features"].as_array().into_iter().flatten().filter_map(|f| f.as_str().map(str::to_owned)).collect(),
            premium_tier: g["premium_tier"].as_u64().unwrap_or(0) as u8,
            boosts: g["premium_subscription_count"].as_u64().unwrap_or(0),
            vanity: g["vanity_url_code"].as_str().map(str::to_owned),
            rules_channel_id: g["rules_channel_id"].as_str().map(str::to_owned),
            updates_channel_id: g["public_updates_channel_id"].as_str().map(str::to_owned),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Select {
    SystemChannel,
    AfkChannel,
    AfkTimeout,
    WidgetChannel,
    AuditUser,
    AuditAction,
    PruneDays,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Save {
    Idle,
    Saving,
    Failed(String),
}

// A change that went out and can be taken back from the page it happened on.
#[derive(Clone, Debug)]
pub struct Undo {
    pub label: String,
    pub call: Call,
    pub page: Page,
}

pub struct ServerSettings {
    pub guild_id: String,
    pub page: Page,
    pub scroll: ScrollHandle,
    pub info: Load<Info>,
    pub draft: Overview,
    pub name: Entity<InputState>,
    pub description: Entity<TextareaState>,
    pub search: Entity<InputState>,
    pub save: Save,
    pub closing: bool,
    pub select: Option<Select>,
    pub roles: RolesView,
    pub lists: Lists,
    pub undo: Option<Undo>,
    // Pages whose lists were asked for, so each loads once.
    pub loaded: HashSet<Page>,
    pub widget: Load<(bool, Option<String>)>,
    pub asking_delete: bool,
    pub delete_name: Entity<InputState>,
}

impl Waffle {
    pub(crate) fn server_me(&self, guild_id: &str) -> Option<(String, bool, u64, i64)> {
        let me = self.me.as_ref()?;
        let guild = self.guilds.iter().find(|g| g.id == guild_id)?;
        Some((me.id.clone(), guild.owner_id.as_deref() == Some(me.id.as_str()), guild.permissions(&me.id), guild.top_position(&me.id)))
    }

    // What this member may do here, with the server's features once they've loaded.
    pub(crate) fn with_access<R>(&self, guild_id: &str, then: impl FnOnce(&Me) -> R) -> Option<R> {
        let (id, owner, permissions, top_position) = self.server_me(guild_id)?;
        let features = match self.server_settings.as_ref().map(|v| &v.info) {
            Some(Load::Ready(info)) => info.features.clone(),
            _ => Vec::new(),
        };
        Some(then(&Me { id: &id, owner, permissions, top_position, features: &features }))
    }

    pub(crate) fn can_open_server_settings(&self, guild_id: &str) -> bool {
        self.with_access(guild_id, |me| me.any_page()).unwrap_or(false)
    }

    pub(crate) fn open_server_settings(&mut self, guild_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(page) = self.with_access(&guild_id, |me| me.nav().first().and_then(|(_, pages)| pages.first().copied())).flatten() else { return };
        let mut input = |placeholder: &'static str, cx: &mut Context<Self>| {
            let state = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
            cx.subscribe(&state, |_, _, _: &InputEvent, cx| cx.notify()).detach();
            state
        };
        let name = input("Server name", cx);
        let search = input("Search", cx);
        let delete_name = input("Server name", cx);
        let description = cx.new(|cx| TextareaState::new(window, cx).placeholder("How did your server get started? Why should people join?"));
        cx.subscribe(&description, |_, _, _: &InputEvent, cx| cx.notify()).detach();
        let roles = RolesView::new(window, cx);
        let lists = Lists::new(window, cx);
        self.server_settings = Some(ServerSettings {
            guild_id: guild_id.clone(),
            page,
            scroll: ScrollHandle::new(),
            info: Load::Loading,
            draft: Overview::default(),
            name,
            description,
            search,
            save: Save::Idle,
            closing: false,
            select: None,
            roles,
            lists,
            undo: None,
            loaded: HashSet::new(),
            widget: Load::Loading,
            asking_delete: false,
            delete_name,
        });
        self.menu = None;
        self.load_server_info(window, cx);
        self.open_server_page(page, cx);
        cx.notify();
    }

    fn load_server_info(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = &self.server_settings else { return };
        let guild_id = view.guild_id.clone();
        let window_handle = window.window_handle();
        self.server_call(api::guild(&guild_id), move |this, result, cx| {
            let Some(view) = this.server_settings.as_mut() else { return };
            view.info = match result {
                Ok(body) => Load::Ready(Info::parse(&body)),
                Err(error) => Load::Failed(error),
            };
            if let Load::Ready(info) = &view.info {
                view.draft = info.saved.clone();
            }
            let (name, description, draft) = (view.name.clone(), view.description.clone(), view.draft.clone());
            window_handle
                .update(cx, |_, window, cx| {
                    name.update(cx, |s, cx| s.set_value(draft.name.clone(), window, cx));
                    description.update(cx, |s, cx| s.set_value(draft.description.clone().unwrap_or_default(), window, cx));
                })
                .ok();
        }, cx);
    }

    // Runs a call off the UI thread and hands its result back, Discord's error text untouched.
    pub(crate) fn server_call(&mut self, call: Call, then: impl FnOnce(&mut Waffle, Result<Value, String>, &mut Context<Waffle>) + 'static, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else {
            return then(self, Err("Not connected to Discord.".into()), cx);
        };
        cx.spawn(async move |this, cx| {
            let result = call.send(&client).await;
            this.update(cx, |this, cx| {
                then(this, result, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn open_server_page(&mut self, page: Page, cx: &mut Context<Self>) {
        let Some(view) = &mut self.server_settings else { return };
        (view.page, view.select, view.asking_delete) = (page, None, false);
        view.scroll.set_offset(gpui_kit::point(px(0.), px(0.)));
        if view.loaded.insert(page) {
            self.load_server_page(page, cx);
        }
        cx.notify();
    }

    pub(crate) fn close_server_settings(&mut self, force: bool, cx: &mut Context<Self>) {
        if !force && self.server_dirty(cx) {
            if let Some(view) = &mut self.server_settings {
                view.closing = true;
            }
            return cx.notify();
        }
        self.server_settings = None;
        cx.notify();
    }

    fn current_overview(&self, cx: &App) -> Option<(Overview, Overview)> {
        let view = self.server_settings.as_ref()?;
        let Load::Ready(info) = &view.info else { return None };
        let description = view.description.read(cx).value().to_string();
        let draft = Overview { name: view.name.read(cx).value().to_string(), description: (!description.trim().is_empty()).then_some(description), ..view.draft.clone() };
        Some((info.saved.clone(), draft))
    }

    pub(crate) fn server_dirty(&self, cx: &App) -> bool {
        let overview = self.current_overview(cx).is_some_and(|(saved, draft)| !api::overview_patch(&saved, &draft).is_empty());
        overview || self.role_dirty(cx)
    }

    pub(crate) fn edit_server_draft(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Overview)) {
        if let Some(view) = &mut self.server_settings {
            change(&mut view.draft);
            view.select = None;
            if view.save != Save::Saving {
                view.save = Save::Idle;
            }
        }
        cx.notify();
    }

    fn reset_server_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = &mut self.server_settings else { return };
        let Load::Ready(info) = &view.info else { return };
        view.draft = info.saved.clone();
        (view.save, view.closing) = (Save::Idle, false);
        let (name, description, saved) = (view.name.clone(), view.description.clone(), info.saved.clone());
        name.update(cx, |s, cx| s.set_value(saved.name.clone(), window, cx));
        description.update(cx, |s, cx| s.set_value(saved.description.clone().unwrap_or_default(), window, cx));
        self.reset_role_draft(window, cx);
        cx.notify();
    }

    // The server and the open role go out together; nothing local changes until Discord answers.
    fn save_server_settings(&mut self, close_after: bool, cx: &mut Context<Self>) {
        let Some(view) = &self.server_settings else { return };
        let guild_id = view.guild_id.clone();
        let mut calls = Vec::new();
        if let Some((saved, draft)) = self.current_overview(cx) {
            if draft.name.trim().is_empty() {
                if let Some(view) = &mut self.server_settings {
                    view.save = Save::Failed("Server name can't be empty.".into());
                }
                return cx.notify();
            }
            calls.extend(api::edit_guild(&guild_id, &saved, &draft).map(|c| (c, Saved::Guild)));
        }
        calls.extend(self.role_save_call(cx).map(|c| (c, Saved::Role)));
        if let Some(view) = &mut self.server_settings {
            view.save = Save::Saving;
        }
        cx.notify();
        self.run_saves(calls, close_after, cx);
    }

    fn run_saves(&mut self, mut calls: Vec<(Call, Saved)>, close_after: bool, cx: &mut Context<Self>) {
        if calls.is_empty() {
            if let Some(view) = &mut self.server_settings {
                view.save = Save::Idle;
            }
            if close_after {
                self.server_settings = None;
            }
            return cx.notify();
        }
        let (call, kind) = calls.remove(0);
        self.server_call(call, move |this, result, cx| {
            match result {
                Ok(body) => {
                    this.apply_saved(kind, &body);
                    this.run_saves(calls, close_after, cx);
                }
                Err(error) => {
                    if let Some(view) = &mut this.server_settings {
                        view.save = Save::Failed(error);
                    }
                }
            }
        }, cx);
    }

    fn apply_saved(&mut self, kind: Saved, body: &Value) {
        match kind {
            Saved::Guild => {
                let fresh = Info::parse(body);
                if let Some(guild) = self.server_settings.as_ref().and_then(|v| self.guilds.iter_mut().find(|g| g.id == v.guild_id)) {
                    guild.name = fresh.saved.name.clone();
                }
                if let Some(view) = &mut self.server_settings {
                    view.draft = Overview { name: view.draft.name.clone(), description: view.draft.description.clone(), ..fresh.saved.clone() };
                    if let Load::Ready(info) = &mut view.info {
                        info.saved = fresh.saved;
                    }
                }
            }
            Saved::Role => self.role_saved(body),
        }
    }

    pub(crate) fn render_server_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let view = self.server_settings.as_ref()?;
        let (page, scroll, guild_id) = (view.page, view.scroll.clone(), view.guild_id.clone());
        let community = self.with_access(&guild_id, |me| me.feature("COMMUNITY")).unwrap_or(false);
        let sections: Vec<AnyElement> = match &view.info {
            Load::Failed(error) => vec![ui::callout(Notice::Danger, Glyph::WarningCircle, "Couldn't load this server", error.clone(), cx).into_any_element()],
            _ => self.server_page(page, window, cx),
        };
        let undo = self.undo_bar(page, cx);
        let body = ui::settings_page("server-settings-scroll", &scroll).child(ui::settings_column(div().flex().flex_col().gap_8().child(ui::settings_title(page.label(community), cx)).children(undo).children(sections)));
        let main = div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(palette::surface(Level::Page, cx))
            .child(body)
            .child(gpui_kit::component::scroll::Scrollbar::vertical(&scroll))
            .children(self.server_save_bar(cx))
            .children(self.member_bulk_bar(cx))
            .child(ui::settings_close("server-settings-close", cx.listener(|this, _, _, cx| this.close_server_settings(false, cx)), cx));
        let nav = self.server_settings_nav(&guild_id, cx).into_any_element();
        Some(self.settings_frame("server-settings", nav, main, cx).into_any_element())
    }

    fn server_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if matches!(self.server_settings.as_ref().map(|v| &v.info), Some(Load::Loading)) && matches!(page, Page::Profile | Page::Engagement | Page::BoostPerks | Page::Safety | Page::Community | Page::Delete) {
            return vec![loading_line("Loading the server…", cx)];
        }
        match page {
            Page::Profile => self.ss_profile_page(cx),
            Page::Tag => vec![link_out("server-tag", "Server tags are set up on Discord", "Choosing a tag's name, badge and colour isn't in Discord's public API yet.", "https://discord.com/app", cx)],
            Page::Engagement => self.ss_engagement_page(window, cx),
            Page::BoostPerks => self.ss_boost_page(window, cx),
            Page::Safety => self.ss_safety_page(cx),
            Page::Roles => self.ss_roles_page(window, cx),
            Page::Delete => self.ss_delete_page(cx),
            other => self.list_page(other, window, cx),
        }
    }

    fn server_settings_nav(&self, guild_id: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.server_settings.as_ref().expect("server settings are open");
        let guild = self.guilds.iter().find(|g| g.id == guild_id);
        let name = guild.map(|g| g.name.clone()).unwrap_or_default();
        let icon: AnyElement = match guild.and_then(|g| g.icon.clone()) {
            Some(url) => ui::avatar(url, 40.).flex_none().into_any_element(),
            None => ui::monogram(&name, 40.).flex_none().into_any_element(),
        };
        let head = div()
            .flex()
            .items_center()
            .gap_2p5()
            .px_1p5()
            .pb_3()
            .min_w_0()
            .child(icon)
            .child(div().flex_1().min_w_0().flex().flex_col().line_height(px(18.)).child(div().truncate().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(name)).child(div().truncate().text_size(ui::TEXT_XS).text_color(palette::muted(cx)).child("Server settings")));
        let search = ui::search_field("ss-search", &view.search, cx).into_any_element();
        let query = view.search.read(cx).value().to_string();
        let mut out = vec![head.into_any_element(), div().pb_2().child(search).into_any_element()];
        if !query.trim().is_empty() {
            out.extend(self.server_search_results(&query, cx));
            return ui::settings_sidebar("server-settings-nav", &self.settings_nav_scroll, cx).children(out);
        }
        let community = self.with_access(guild_id, |me| me.feature("COMMUNITY")).unwrap_or(false);
        let nav = self.with_access(guild_id, |me| me.nav()).unwrap_or_default();
        for (title, pages) in nav {
            if let Some(title) = title {
                out.push(div().px_2p5().pt_3p5().pb_1().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(palette::muted(cx)).child(title).into_any_element());
            } else if out.len() > 2 {
                out.push(div().h(ui::HAIRLINE).mx_2p5().my_2().flex_none().bg(palette::line(cx)).into_any_element());
            }
            for page in pages {
                let item = if page == Page::Delete {
                    ui::menu_row("ss-nav-Delete", cx).danger(cx).toggled(view.page == page, cx).child(Icon::new(Glyph::Trash).size(ui::ICON_LG)).child(page.label(community))
                } else {
                    ui::settings_nav_item(SharedString::from(format!("ss-nav-{page:?}")), glyph(page), page.label(community), view.page == page, cx)
                };
                out.push(item.on_click(cx.listener(move |this, _, _, cx| this.open_server_page(page, cx))).into_any_element());
            }
        }
        ui::settings_sidebar("server-settings-nav", &self.settings_nav_scroll, cx).children(out)
    }

    fn server_search_results(&self, query: &str, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(guild_id) = self.server_settings.as_ref().map(|v| v.guild_id.clone()) else { return Vec::new() };
        let hits: Vec<_> = registry::search(SERVER_SETTINGS, query).into_iter().filter(|s| self.with_access(&guild_id, |me| me.can_open(s.page)).unwrap_or(false)).collect();
        if hits.is_empty() {
            return vec![div().px_2p5().py_2().text_color(palette::muted(cx)).child("No settings match").into_any_element()];
        }
        hits.into_iter()
            .map(|setting| {
                let place = format!("{} \u{203a} {}", registry::Scope::label(setting.page), setting.group);
                ui::menu_row(SharedString::from(format!("ss-result-{}", setting.id)), cx)
                    .h_auto()
                    .py_1p5()
                    .child(div().flex_1().min_w_0().flex().flex_col().child(div().truncate().text_color(palette::strong(cx)).child(setting.title)).child(div().truncate().text_size(ui::TEXT_XS).text_color(palette::muted(cx)).child(place)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(view) = &this.server_settings {
                            view.search.update(cx, |s, cx| s.set_value("", window, cx));
                        }
                        cx.set_global(ui::SettingsHighlight(Some(SharedString::from(setting.row_id()))));
                        this.open_server_page(setting.page, cx);
                    }))
                    .into_any_element()
            })
            .collect()
    }

    // The profile: icon, banner, name and description.
    fn ss_profile_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let view = self.server_settings.as_ref().expect("server settings are open");
        let draft = &view.draft;
        let guild_id = view.guild_id.clone();
        let banner_open = self.with_access(&guild_id, |me| me.feature(access::BANNER.0)).unwrap_or(false);
        let icon_row = image_row("ss-icon", "Server icon", "Square, at least 512 × 512. Shows in the server list and on invites.", draft.icon.is_some(), true, ImageField::Icon, false, cx);
        let banner_row = image_row("ss-banner", "Banner", if banner_open { "16:9, at least 960 × 540. Sits behind the server name above your channels." } else { "Unlocks at boost level 2." }, draft.banner.is_some(), banner_open, ImageField::Banner, true, cx);
        let name_focus = view.name.read(cx).focus_handle(cx);
        let name = ui::field("Server name", cx).child(ui::text_field("ss-name", Input::new(&view.name).appearance(false), &name_focus));
        let description_focus = view.description.read(cx).focus_handle(cx);
        let count = view.description.read(cx).value().chars().count();
        let description = ui::field("Description", cx)
            .child(ui::text_area("ss-description", Textarea::new(&view.description).appearance(false).px_0().text_size(ui::TEXT_SM), &description_focus).error(count > 120))
            .child(ui::field_hint("Shows on invites and in Server Discovery.", Some(format!("{count} / 120").into()), cx));
        vec![
            ui::settings_group("Icon and banner", cx).child(ui::settings_card(cx).child(icon_row).child(banner_row)).into_any_element(),
            ui::settings_group("Name", cx).child(ui::settings_card(cx).p_4().gap_4().child(name).child(description)).into_any_element(),
        ]
    }

    fn ss_engagement_page(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        use api::system_flags::*;
        let view = self.server_settings.as_ref().expect("server settings are open");
        let draft = view.draft.clone();
        let channels = self.text_channels(&view.guild_id, ChannelKind::Text);
        let voice = self.text_channels(&view.guild_id, ChannelKind::Voice);
        let mut none = vec![(String::new(), "No system messages".to_owned())];
        none.extend(channels.iter().cloned());
        let system = ui::settings_row("ss-system-channel", false, cx).child(ui::settings_text("System messages channel", Some("Where Discord posts joins, boosts and tips"), cx)).child(div().w(px(220.)).flex_none().child(self.ss_select(Select::SystemChannel, &none, draft.system_channel_id.as_deref().unwrap_or_default(), cx)));
        let flag_rows = [(JOIN, "Welcome people when they join", "A random greeting with their name", "ss-flag-join"), (JOIN_STICKER_REPLIES, "Suggest a sticker reply to welcomes", "Members see a wave button under the greeting", "ss-flag-sticker"), (BOOSTS, "Thank people who boost", "A message when someone boosts this server", "ss-flag-boost"), (SETUP_TIPS, "Send setup tips", "Helpful tips for setting up the server", "ss-flag-tips")].map(|(bit, title, detail, id)| {
            // The flags are "suppress" bits: on means the message is sent.
            let on = draft.system_channel_flags & bit == 0;
            ui::settings_switch(id, title, detail, on, true, window, cx).on_click(cx.listener(move |this, _, _, cx| this.edit_server_draft(cx, |d| d.system_channel_flags ^= bit))).into_any_element()
        });
        let notifications = [(0u8, "All messages", "Every new message counts as unread"), (1, "Only @mentions", "Recommended once a server has more than a few dozen people")].map(|(level, title, detail)| {
            let row = ui::settings_row(SharedString::from(format!("ss-notify-{level}")), level > 0, cx).child(ui::radio(draft.default_message_notifications == level, cx)).child(ui::settings_text(title, Some(detail), cx));
            ui::settings_choice(row, cx).on_click(cx.listener(move |this, _, _, cx| this.edit_server_draft(cx, |d| d.default_message_notifications = level))).into_any_element()
        });
        let mut afk = vec![(String::new(), "No inactive channel".to_owned())];
        afk.extend(voice);
        let timeouts: Vec<(String, String)> = api::AFK_TIMEOUTS.iter().map(|s| (s.to_string(), minutes(*s))).collect();
        let afk_channel = ui::settings_row("ss-afk-channel", false, cx).child(ui::settings_text("Inactive channel", Some("Where idle members are moved, muted"), cx)).child(div().w(px(220.)).flex_none().child(self.ss_select(Select::AfkChannel, &afk, draft.afk_channel_id.as_deref().unwrap_or_default(), cx)));
        let afk_timeout = ui::settings_row("ss-afk-timeout", true, cx).child(ui::settings_text("Move after", Some("Counted from their last sound"), cx)).child(div().w(px(220.)).flex_none().child(self.ss_select(Select::AfkTimeout, &timeouts, &draft.afk_timeout.to_string(), cx)));
        let mut out = vec![
            ui::settings_group("System messages", cx).child(ui::settings_card(cx).child(system).children(flag_rows)).into_any_element(),
            ui::settings_group("Default notifications", cx).child(ui::settings_card(cx).children(notifications)).child(ui::settings_caption("New members start here; anyone can change it for themselves.", cx)).into_any_element(),
            ui::settings_group("Inactive voice", cx).child(ui::settings_card(cx).child(afk_channel).child(afk_timeout)).into_any_element(),
        ];
        out.push(self.ss_widget_group(&channels, window, cx));
        out
    }

    // The widget saves at once: it's a separate endpoint from the server's own settings.
    fn ss_widget_group(&self, channels: &[(String, String)], window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let view = self.server_settings.as_ref().expect("server settings are open");
        let card = match &view.widget {
            Load::Loading => ui::settings_card(cx).child(ui::settings_row("ss-widget-loading", false, cx).child(loading_line("Loading the widget…", cx))),
            Load::Failed(error) => ui::settings_card(cx).child(ui::settings_row("ss-widget-error", false, cx).child(ui::settings_text("Couldn't load the widget", Some(error.clone()), cx))),
            Load::Ready((enabled, channel)) => {
                let (enabled, channel) = (*enabled, channel.clone());
                let toggle = {
                    let channel = channel.clone();
                    ui::settings_switch("ss-widget", "Show a widget on your website", "Online members, voice channels and an invite", enabled, false, window, cx).on_click(cx.listener(move |this, _, _, cx| this.set_widget(!enabled, channel.clone(), cx)))
                };
                let mut options = vec![(String::new(), "No invite".to_owned())];
                options.extend(channels.iter().cloned());
                let invite = ui::settings_row("ss-widget-channel", true, cx).child(ui::settings_text("Invite channel", Some("Where people who join from the widget land"), cx)).child(div().w(px(220.)).flex_none().child(self.ss_select(Select::WidgetChannel, &options, channel.as_deref().unwrap_or_default(), cx)));
                ui::settings_card(cx).child(toggle).child(invite)
            }
        };
        ui::settings_group("Server widget", cx).child(card).into_any_element()
    }

    pub(crate) fn set_widget(&mut self, enabled: bool, channel: Option<String>, cx: &mut Context<Self>) {
        let Some(view) = &mut self.server_settings else { return };
        let before = std::mem::replace(&mut view.widget, Load::Ready((enabled, channel.clone())));
        let call = api::edit_widget(&view.guild_id, enabled, channel.as_deref());
        self.server_call(call, move |this, result, cx| {
            if let Err(error) = result {
                if let Some(view) = &mut this.server_settings {
                    view.widget = before;
                }
                this.show_toast(error, cx);
            }
        }, cx);
        cx.notify();
    }

    fn ss_boost_page(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let view = self.server_settings.as_ref().expect("server settings are open");
        let Load::Ready(info) = &view.info else { return Vec::new() };
        let draft = view.draft.clone();
        let next = [2u64, 7, 14].get(info.premium_tier as usize).copied();
        let progress = match next {
            Some(goal) => format!("{} · {} more to level {}", plural(info.boosts, "boost", "boosts"), goal.saturating_sub(info.boosts), info.premium_tier + 1),
            None => format!("{} · the top level", plural(info.boosts, "boost", "boosts")),
        };
        let level = ui::settings_row("ss-boost-level", false, cx).child(ui::settings_text("Boost level", Some(progress), cx)).child(value_text(if info.premium_tier == 0 { "None".into() } else { format!("Level {}", info.premium_tier) }, cx));
        let bar = ui::settings_switch("ss-progress-bar", "Show the boost progress bar", "Above your channels, so members see how close the next level is", draft.premium_progress_bar_enabled, true, window, cx)
            .on_click(cx.listener(|this, _, _, cx| this.edit_server_draft(cx, |d| d.premium_progress_bar_enabled = !d.premium_progress_bar_enabled)));
        let splash_open = info.features.iter().any(|f| f == access::INVITE_SPLASH.0);
        let splash = image_row("ss-splash", "Invite background", if splash_open { "16:9, shown behind your invite page." } else { "Unlocks at boost level 1." }, draft.splash.is_some(), splash_open, ImageField::Splash, false, cx);
        let vanity_open = info.features.iter().any(|f| f == access::VANITY_URL.0);
        let vanity = ui::settings_row("ss-vanity", false, cx).child(ui::settings_text(
            match &info.vanity {
                Some(code) => format!("discord.gg/{code}"),
                None => "No custom link".into(),
            },
            Some(if vanity_open { "Anyone with the link can join. Change it on Discord; the public API can't set it." } else { "Unlocks at boost level 3." }),
            cx,
        ));
        let vanity = match (&info.vanity, vanity_open) {
            (Some(code), true) => {
                let link = format!("https://discord.gg/{code}");
                vanity.child(ui::button("ss-vanity-copy", Tone::Secondary, cx).small().child("Copy link").on_click(cx.listener(move |this, _, _, cx| this.copy_text(link.clone(), cx))))
            }
            _ => vanity,
        };
        vec![
            ui::settings_group("Progress", cx).child(ui::settings_card(cx).child(level).child(bar)).into_any_element(),
            ui::settings_group("Invite background", cx).child(ui::settings_card(cx).child(splash)).into_any_element(),
            ui::settings_group("Custom invite link", cx).child(ui::settings_card(cx).child(vanity)).child(ui::settings_caption("Locked rows say which level opens them.", cx)).into_any_element(),
        ]
    }

    fn ss_safety_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let view = self.server_settings.as_ref().expect("server settings are open");
        let draft = view.draft.clone();
        let levels = [(0u8, "Anyone", "No checks"), (1, "Verified email", "Their account has a confirmed email"), (2, "Verified email, on Discord 5 minutes", "Their account is older than five minutes"), (3, "Verified email, here 10 minutes", "They've been a member for ten minutes; slows raids the most"), (4, "Verified phone", "Their account has a confirmed phone number")];
        let level_rows = levels.map(|(value, title, detail)| {
            let row = ui::settings_row(SharedString::from(format!("ss-verify-{value}")), value > 0, cx).child(ui::radio(draft.verification_level == value, cx)).child(ui::settings_text(title, Some(detail), cx));
            ui::settings_choice(row, cx).on_click(cx.listener(move |this, _, _, cx| this.edit_server_draft(cx, |d| d.verification_level = value))).into_any_element()
        });
        let filters = [(0u8, "Don't scan", "Images aren't checked"), (1, "Members without a role", "Images from members with no role are scanned"), (2, "Everyone", "Every image is scanned before it posts")];
        let filter_rows = filters.map(|(value, title, detail)| {
            let row = ui::settings_row(SharedString::from(format!("ss-filter-{value}")), value > 0, cx).child(ui::radio(draft.explicit_content_filter == value, cx)).child(ui::settings_text(title, Some(detail), cx));
            ui::settings_choice(row, cx).on_click(cx.listener(move |this, _, _, cx| this.edit_server_draft(cx, |d| d.explicit_content_filter = value))).into_any_element()
        });
        vec![
            ui::settings_group("Who can chat", cx).child(ui::settings_card(cx).children(level_rows)).child(ui::settings_caption("Members with a role skip these checks.", cx)).into_any_element(),
            ui::settings_group("Sensitive images", cx).child(ui::settings_card(cx).children(filter_rows)).into_any_element(),
        ]
    }

    fn ss_delete_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let view = self.server_settings.as_ref().expect("server settings are open");
        let name = self.guilds.iter().find(|g| g.id == view.guild_id).map(|g| g.name.clone()).unwrap_or_default();
        let typed = view.delete_name.read(cx).value().to_string();
        let matches = typed.trim() == name.trim() && !name.is_empty();
        let focus = view.delete_name.read(cx).focus_handle(cx);
        let guild_id = view.guild_id.clone();
        let field = ui::field(format!("Type {name} to confirm"), cx).child(ui::text_field("ss-delete-name", Input::new(&view.delete_name).appearance(false), &focus));
        let delete = ui::button("ss-delete-server", Tone::Danger, cx).disabled(!matches).child("Delete server").on_click(cx.listener(move |this, _, _, cx| {
            let guild_id = guild_id.clone();
            this.server_call(api::delete_guild(&guild_id), move |this, result, cx| match result {
                Ok(_) => {
                    this.server_settings = None;
                    this.guilds.retain(|g| g.id != guild_id);
                }
                Err(error) => this.show_toast(error, cx),
            }, cx);
        }));
        vec![
            ui::callout(Notice::Danger, Glyph::Trash, format!("Deleting {name} can't be undone"), "Every channel, message and role goes, for everyone.", cx).into_any_element(),
            ui::settings_card(cx).p_4().gap_4().child(field).child(div().flex().justify_end().child(delete)).into_any_element(),
        ]
    }

    // Text or voice channels of the server as (id, "#name") options, in channel order.
    pub(crate) fn text_channels(&self, guild_id: &str, kind: ChannelKind) -> Vec<(String, String)> {
        let Some(guild) = self.guilds.iter().find(|g| g.id == guild_id) else { return Vec::new() };
        guild
            .channels
            .iter()
            .filter(|c| if kind == ChannelKind::Voice { matches!(c.raw_type, 2 | 13) } else { c.kind == ChannelKind::Text && !matches!(c.raw_type, 2 | 13) })
            .map(|c| (c.id.clone(), if kind == ChannelKind::Voice { c.name.clone() } else { format!("#{}", c.name) }))
            .collect()
    }

    pub(crate) fn ss_select(&self, select: Select, options: &[(String, String)], chosen: &str, cx: &mut Context<Self>) -> AnyElement {
        let open = self.server_settings.as_ref().and_then(|v| v.select) == Some(select);
        let value = options.iter().find(|(id, _)| id == chosen).map(|(_, n)| n.clone()).or_else(|| options.first().map(|(_, n)| n.clone())).unwrap_or_default();
        let trigger = ui::select(SharedString::from(format!("ss-select-{select:?}")), None, value, open, cx).on_click(cx.listener(move |this, _, _, cx| {
            if let Some(v) = &mut this.server_settings {
                v.select = if v.select == Some(select) { None } else { Some(select) };
            }
            cx.notify();
        }));
        let list = open.then(|| {
            let (this, close) = (cx.entity(), cx.entity());
            let dismiss = move |_: &mut Window, cx: &mut App| {
                close.update(cx, |this, cx| {
                    if let Some(v) = &mut this.server_settings {
                        v.select = None;
                    }
                    cx.notify();
                })
            };
            ui::select_options(SharedString::from(format!("ss-options-{select:?}")), options, chosen, 40., move |id, _, cx| this.update(cx, |this, cx| this.ss_pick(select, &id, cx)), dismiss, cx)
        });
        div().relative().w_full().child(trigger).children(list).into_any_element()
    }

    fn ss_pick(&mut self, select: Select, id: &str, cx: &mut Context<Self>) {
        let some = (!id.is_empty()).then(|| id.to_owned());
        match select {
            Select::SystemChannel => self.edit_server_draft(cx, |d| d.system_channel_id = some),
            Select::AfkChannel => self.edit_server_draft(cx, |d| d.afk_channel_id = some),
            Select::AfkTimeout => self.edit_server_draft(cx, |d| d.afk_timeout = id.parse().unwrap_or(300)),
            Select::WidgetChannel => {
                let enabled = matches!(self.server_settings.as_ref().map(|v| &v.widget), Some(Load::Ready((true, _))));
                if let Some(view) = &mut self.server_settings {
                    view.select = None;
                }
                self.set_widget(enabled, some, cx);
            }
            Select::AuditUser | Select::AuditAction | Select::PruneDays => self.list_pick(select, id, cx),
        }
    }

    // Pick an image, then hold it in the draft as a data URI until the save goes out.
    fn pick_image(&mut self, field: ImageField, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let Ok(bytes) = std::fs::read(&path) else { return };
            let data = api::image_data(mime(&path), &bytes);
            this.update(cx, |this, cx| this.edit_server_draft(cx, |d| *field.slot(d) = Some(data))).ok();
        })
        .detach();
    }

    fn server_save_bar(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let view = self.server_settings.as_ref()?;
        let dirty = self.server_dirty(cx);
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
        let reset = ui::button("ss-reset", Tone::Ghost, cx).disabled(saving).child(if closing { "Discard changes" } else { "Reset" }).on_click(cx.listener(move |this, _, window, cx| {
            if closing {
                this.close_server_settings(true, cx);
            } else {
                this.reset_server_draft(window, cx);
            }
        }));
        let save = ui::button("ss-save", Tone::Primary, cx).loading(saving.then_some("Saving")).child("Save changes").on_click(cx.listener(move |this, _, _, cx| this.save_server_settings(closing, cx)));
        let bar = ui::floating_bar("ss-savebar", cx)
            .child(div().text_color(ink).flex_none().child(Icon::new(glyph).size(ui::ICON_XL)))
            .child(div().flex_1().min_w_0().flex().flex_col().child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(title)).children(detail.map(|d| div().text_size(ui::TEXT_XS).line_height(px(17.)).text_color(palette::muted(cx)).child(d))))
            .child(div().flex().gap_2().flex_none().child(reset).child(save));
        Some(ui::foot_dock(bar, "ss-savebar-in"))
    }

    // The last reversible change on this page, with its Undo.
    fn undo_bar(&self, page: Page, cx: &mut Context<Self>) -> Option<AnyElement> {
        let undo = self.server_settings.as_ref()?.undo.clone().filter(|u| u.page == page)?;
        let button = ui::button("ss-undo", Tone::Secondary, cx).small().child("Undo").on_click(cx.listener(move |this, _, _, cx| {
            if let Some(view) = &mut this.server_settings {
                view.undo = None;
            }
            this.server_call(undo.call.clone(), move |this, result, cx| match result {
                Ok(_) => this.reload_server_page(page, cx),
                Err(error) => this.show_toast(error, cx),
            }, cx);
        }));
        let dismiss = ui::icon_button("ss-undo-dismiss", Glyph::X, "Dismiss", cx).on_click(cx.listener(|this, _, _, cx| {
            if let Some(view) = &mut this.server_settings {
                view.undo = None;
            }
            cx.notify();
        }));
        Some(ui::callout_with(Notice::Info, Glyph::CheckCircle, self.server_settings.as_ref()?.undo.as_ref()?.label.clone(), "", div().flex().gap_1().child(button).child(dismiss), cx).into_any_element())
    }

    pub(crate) fn offer_undo(&mut self, label: impl Into<String>, call: Call, cx: &mut Context<Self>) {
        if let Some(view) = &mut self.server_settings {
            view.undo = Some(Undo { label: label.into(), call, page: view.page });
        }
        cx.notify();
    }

    pub(crate) fn reload_server_page(&mut self, page: Page, cx: &mut Context<Self>) {
        if let Some(view) = &mut self.server_settings {
            view.loaded.remove(&page);
        }
        if self.server_settings.as_ref().is_some_and(|v| v.page == page) {
            self.open_server_page(page, cx);
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Saved {
    Guild,
    Role,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImageField {
    Icon,
    Banner,
    Splash,
}

impl ImageField {
    fn slot(self, draft: &mut Overview) -> &mut Option<String> {
        match self {
            ImageField::Icon => &mut draft.icon,
            ImageField::Banner => &mut draft.banner,
            ImageField::Splash => &mut draft.splash,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn image_row(id: &'static str, title: &'static str, detail: &'static str, has: bool, open: bool, field: ImageField, line: bool, cx: &mut Context<Waffle>) -> gpui_kit::Stateful<Div> {
    let upload = ui::button(SharedString::from(format!("{id}-upload")), Tone::Secondary, cx).small().disabled(!open).child(Icon::new(Glyph::Upload).size(ui::ICON_MD)).child(if has { "Change" } else { "Upload" }).on_click(cx.listener(move |this, _, _, cx| this.pick_image(field, cx)));
    let remove = has.then(|| ui::button(SharedString::from(format!("{id}-remove")), Tone::Ghost, cx).small().child("Remove").on_click(cx.listener(move |this, _, _, cx| this.edit_server_draft(cx, |d| *field.slot(d) = None))));
    ui::settings_row(id, line, cx).child(ui::settings_text(title, Some(detail), cx)).child(div().flex().gap_2().flex_none().child(upload).children(remove))
}

fn mime(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref() {
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("mp3") => "audio/mpeg",
        Some("ogg") => "audio/ogg",
        _ => "image/jpeg",
    }
}

pub(crate) fn mime_of(path: &std::path::Path) -> &'static str {
    mime(path)
}

pub fn glyph(page: Page) -> Glyph {
    match page {
        Page::Profile => Glyph::Info,
        Page::Tag => Glyph::Tag,
        Page::Engagement => Glyph::ChatRound,
        Page::BoostPerks => Glyph::Stream,
        Page::Emoji => Glyph::Emoji,
        Page::Stickers => Glyph::Image,
        Page::Soundboard => Glyph::Soundboard,
        Page::Members => Glyph::Users,
        Page::Roles => Glyph::Id,
        Page::Invites => Glyph::Link,
        Page::Access => Glyph::DoorOpen,
        Page::Integrations => Glyph::AppWindow,
        Page::Safety => Glyph::Shield,
        Page::AuditLog => Glyph::Note,
        Page::Bans => Glyph::Gavel,
        Page::AutoMod => Glyph::Prohibit,
        Page::Community => Glyph::GroupPlus,
        Page::Onboarding => Glyph::HandWaving,
        Page::Insights => Glyph::Activities,
        Page::Template => Glyph::Copy,
        Page::Delete => Glyph::Trash,
    }
}


pub fn value_text(text: String, cx: &App) -> Div {
    div().flex_none().whitespace_nowrap().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(text)
}

pub fn loading_line(text: &'static str, cx: &App) -> AnyElement {
    div().px_1().text_color(palette::muted(cx)).child(text).into_any_element()
}

pub fn failed(title: &'static str, error: &str, cx: &App) -> AnyElement {
    ui::callout(Notice::Danger, Glyph::WarningCircle, title, error.to_owned(), cx).into_any_element()
}

// A row that sends the member to Discord for what a user client can't do through the API.
pub fn link_out(id: &'static str, title: &'static str, detail: &'static str, url: impl Into<String>, cx: &mut Context<Waffle>) -> AnyElement {
    let url = url.into();
    let open = ui::button(SharedString::from(format!("{id}-open")), Tone::Secondary, cx).small().child("Open Discord").child(Icon::new(Glyph::Open).size(ui::ICON_SM)).on_click(move |_, _, cx| cx.open_url(&url));
    ui::settings_card(cx).child(ui::settings_row(id, false, cx).child(ui::settings_text(title, Some(detail), cx)).child(open)).into_any_element()
}

pub fn plural(n: u64, one: &str, many: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {many}") }
}

fn minutes(seconds: u32) -> String {
    match seconds {
        3600 => "1 hour".into(),
        s => plural(u64::from(s / 60), "minute", "minutes"),
    }
}

// A snowflake's creation time, in the reader's local time.
pub fn snowflake_time(id: &str) -> String {
    chrono::DateTime::from_timestamp_millis(snowflake_ms(id)).map(|t| t.with_timezone(&chrono::Local).format("%-d %b %Y, %H:%M").to_string()).unwrap_or_default()
}

// When a snowflake was made, in epoch milliseconds.
pub fn snowflake_ms(id: &str) -> i64 {
    ((id.parse::<u64>().unwrap_or(0) >> 22) + 1_420_070_400_000) as i64
}

pub fn sentence_spans(segs: &[audit::Seg], cx: &App) -> Div {
    let (strong, text) = (palette::strong(cx), palette::text(cx));
    div().flex().flex_wrap().min_w_0().children(segs.iter().map(|s| match s {
        audit::Seg::Name(n) => div().font_weight(FontWeight::MEDIUM).text_color(strong).child(n.clone()),
        audit::Seg::Text(t) => div().text_color(text).whitespace_nowrap().child(t.clone()),
    }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn info_reads_guild() {
        let info = Info::parse(&json!({ "name": "Night Shift", "features": ["BANNER", "COMMUNITY"], "premium_tier": 2, "premium_subscription_count": 9, "approximate_member_count": 120, "vanity_url_code": null }));
        assert_eq!((info.saved.name.as_str(), info.premium_tier, info.boosts), ("Night Shift", 2, 9));
        assert_eq!(info.features, vec!["BANNER", "COMMUNITY"]);
    }

    #[test]
    fn labels() {
        assert_eq!(minutes(300), "5 minutes");
        assert_eq!(minutes(60), "1 minute");
        assert_eq!(minutes(3600), "1 hour");
        // 2015-01-01T00:00:00Z plus zero: Discord's epoch.
        assert!(snowflake_time("0").contains("2015") || snowflake_time("0").contains("2014"));
    }
}
