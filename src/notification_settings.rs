// Discord's server Notification Settings dialog (docs/discord-notifications-re.md, section A): mute,
// the server's level, the suppress and push switches, and per-channel overrides. Every control saves at
// once; Done only closes. Opened from the server's menu and its name dropdown.
use std::collections::HashSet;

use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, FontWeight, InteractiveElement, IntoElement, MouseButton, ParentElement, SharedString, Stateful, StatefulInteractiveElement, Styled, Window,
    component::{
        Icon,
        input::{InputEvent, InputState},
    },
    div,
    prelude::FluentBuilder as _,
    px, Div,
};
use serde_json::{Value, json};

use crate::{
    Waffle,
    channel_menu::{MUTE_FOR, effective, mute_body, until},
    discord::{Channel, ChannelKind, Guild, GuildSettings},
    icons::Glyph,
    palette,
    ui::{self, Tone},
};

const WIDTH: f32 = 600.;
const MAX_HEIGHT: f32 = 720.;
const FOREVER: i64 = -1;
const LEVELS: [(&str, u8); 3] = [("All Messages", 0), ("Only @mentions", 1), ("Nothing", 2)];
const SEGMENTS: [&str; 3] = ["All", "Mentions", "Nothing"];
// Discord's notify_highlights values for the Suppress Highlights switch.
const HIGHLIGHTS_DISABLED: u8 = 1;
const HIGHLIGHTS_ENABLED: u8 = 2;
// The mute-duration select's height: the dropdown hangs from its bottom edge.
const SELECT_HEIGHT: f32 = 40.;

pub(crate) struct NotificationSettings {
    guild_id: String,
    // Overrides picked this session that Discord doesn't hold yet (still the default).
    added: Vec<String>,
    open: Option<Popover>,
    search: Entity<InputState>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Popover {
    MuteFor,
    Channels,
}

// One row of the overrides list.
struct OverrideRow {
    channel: Channel,
    subtitle: String,
    level: u8,
    muted: bool,
}

impl Waffle {
    pub(crate) fn open_notification_settings(&mut self, guild_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search channels"));
        cx.subscribe(&search, |_, _, _: &InputEvent, cx| cx.notify()).detach();
        self.menu = None;
        self.notification_settings = Some(NotificationSettings { guild_id, added: Vec::new(), open: None, search });
        cx.notify();
    }

    fn close_notification_settings(&mut self, cx: &mut Context<Self>) {
        self.notification_settings = None;
        cx.notify();
    }

    // Esc: the open dropdown first, then the dialog. False when the dialog isn't open.
    pub(crate) fn dismiss_notification_settings(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(view) = self.notification_settings.as_mut() else { return false };
        if view.open.take().is_none() {
            self.notification_settings = None;
        }
        cx.notify();
        true
    }

    fn toggle_popover(&mut self, popover: Popover, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.notification_settings.as_mut() else { return };
        view.open = if view.open == Some(popover) { None } else { Some(popover) };
        if view.open == Some(Popover::Channels) {
            view.search.update(cx, |s, cx| {
                s.set_value("", window, cx);
                s.focus(window, cx);
            });
        }
        cx.notify();
    }

    fn close_popover(&mut self, cx: &mut Context<Self>) {
        if let Some(view) = self.notification_settings.as_mut() {
            view.open = None;
        }
        cx.notify();
    }

    fn patch_open_guild(&mut self, body: Value, cx: &mut Context<Self>) {
        let Some(guild_id) = self.notification_settings.as_ref().map(|v| v.guild_id.clone()) else { return };
        self.patch_guild_settings(guild_id, body, cx);
    }

    fn patch_open_override(&mut self, channel_id: &str, body: Value, cx: &mut Context<Self>) {
        let Some(guild_id) = self.notification_settings.as_ref().map(|v| v.guild_id.clone()) else { return };
        self.patch_channel_override(guild_id, channel_id, body, cx);
    }

    fn open_guild_muted(&self) -> bool {
        self.notification_settings.as_ref().is_some_and(|v| self.settings_for(&v.guild_id).is_muted())
    }

    pub(crate) fn render_notification_settings(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let view = self.notification_settings.as_ref()?;
        let guild = self.guilds.iter().find(|g| g.id == view.guild_id)?;
        let settings = self.settings_for(&view.guild_id);
        let scroller = ui::col()
            .id("ns-scroll")
            .flex_1()
            .overflow_y_scroll()
            .px_6()
            .pt_1()
            .pb_4()
            .gap_5()
            .child(self.ns_mute_card(view, guild, &settings, window, cx))
            .child(ns_levels(&settings, window, cx))
            .child(ns_switches(&settings, window, cx))
            .child(ns_push_card(&settings, window, cx))
            .child(self.ns_overrides(view, guild, &settings, window, cx));
        let done = ui::row().items_stretch().justify_end().px_6().pt_3().pb_5().child(ui::button("ns-done", Tone::Primary, cx).child("Done").on_click(cx.listener(|this, _, _, cx| this.close_notification_settings(cx))));
        let height = (f32::from(window.viewport_size().height) * 0.85).min(MAX_HEIGHT);
        let body = ui::col().h(px(height)).child(ns_head(guild, cx)).child(scroller).child(done);
        let card = div().id("ns-card").occlude().child(ui::dialog("notification-settings", WIDTH, body, window, cx));
        let backdrop = div().id("ns-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_notification_settings(cx)));
        Some(ui::modal(backdrop, card, cx))
    }

    fn ns_mute_card(&self, view: &NotificationSettings, guild: &Guild, settings: &GuildSettings, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let muted = settings.is_muted();
        let toggle = cx.listener(move |this, _, _, cx| {
            let body = if muted { json!({ "muted": false }) } else { mute_body(FOREVER) };
            this.patch_open_guild(body, cx);
        });
        let title = format!("Mute {}", guild.name);
        let mute = switch_row("ns-mute", title, Some("Muting a server prevents unread indicators and notifications from appearing unless you are mentioned."), muted, true, false, window, cx).on_click(toggle);
        let card = ui::settings_card(cx).child(mute);
        if !muted {
            return card;
        }
        card.child(self.ns_mute_duration(view, settings, cx))
    }

    // While muted: when it ends, and the length it was muted for, changeable from a dropdown.
    fn ns_mute_duration(&self, view: &NotificationSettings, settings: &GuildSettings, cx: &mut Context<Self>) -> Stateful<Div> {
        let open = view.open == Some(Popover::MuteFor);
        let window_seconds = settings.mute_window.unwrap_or(FOREVER);
        let picked = MUTE_FOR.iter().find(|(_, s)| *s == window_seconds).map_or(MUTE_FOR[MUTE_FOR.len() - 1].0, |(label, _)| label);
        let options: Vec<(String, String)> = MUTE_FOR.iter().map(|(label, s)| (s.to_string(), (*label).to_owned())).collect();
        let this = cx.entity();
        let on_choose = move |value: String, _: &mut Window, cx: &mut App| {
            let Ok(seconds) = value.parse::<i64>() else { return };
            this.update(cx, |this, cx| {
                this.close_popover(cx);
                this.patch_open_guild(mute_body(seconds), cx);
            });
        };
        let this = cx.entity();
        let dismiss = move |_: &mut Window, cx: &mut App| this.update(cx, |this, cx| this.close_popover(cx));
        let select = div()
            .relative()
            .w(px(220.))
            .flex_none()
            .child(ui::select("ns-mute-for", Some(Glyph::Clock), picked, open, cx).on_click(cx.listener(|this, _, window, cx| this.toggle_popover(Popover::MuteFor, window, cx))))
            .when(open, |d| d.child(ui::select_options("ns-mute-for-options", &options, &window_seconds.to_string(), SELECT_HEIGHT, on_choose, dismiss, cx)));
        ui::settings_row("ns-mute-duration", true, cx).child(ui::settings_text("Mute duration", Some(muted_until(settings)), cx)).child(select)
    }

    fn ns_overrides(&self, view: &NotificationSettings, guild: &Guild, settings: &GuildSettings, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let rows = override_rows(guild, settings, &view.added);
        let taken: HashSet<&str> = rows.iter().map(|r| r.channel.id.as_str()).collect();
        let open = view.open == Some(Popover::Channels);
        let picker = div()
            .relative()
            .child(ui::select("ns-pick", Some(Glyph::Search), "Select a channel or category…", open, cx).text_color(palette::muted(cx)).on_click(cx.listener(|this, _, window, cx| this.toggle_popover(Popover::Channels, window, cx))))
            .when(open, |d| d.child(self.ns_channel_picker(view, guild, &taken, cx)));
        let guild_muted = settings.is_muted();
        let card = ui::settings_card(cx);
        let card = if rows.is_empty() {
            card.child(ui::settings_row("ns-override-empty", false, cx).min_h(px(52.)).text_color(palette::muted(cx)).child("Add a channel to override its default notification settings"))
        } else {
            card.children(rows.iter().enumerate().map(|(i, row)| ns_override_row(row, i > 0, guild_muted, window, cx)))
        };
        ui::settings_group("Notification Overrides", cx).child(picker).child(card)
    }

    // The searchable list of channels and categories that have no override yet; a pick adds its row.
    fn ns_channel_picker(&self, view: &NotificationSettings, guild: &Guild, taken: &HashSet<&str>, cx: &mut Context<Self>) -> AnyElement {
        let query = view.search.read(cx).value().trim().to_lowercase();
        let sections = override_choices(guild, taken, &query);
        let muted = palette::muted(cx);
        let mut list = ui::col().id("ns-pick-list").max_h(px(260.)).overflow_y_scroll().gap_px();
        if sections.is_empty() {
            list = list.child(div().px_2p5().py_2().text_color(muted).child("No channels found"));
        }
        for (header, channels) in sections {
            list = list.when_some(header, |l, h| l.child(div().px_2p5().pt_2().pb_1().text_size(ui::TEXT_XS).font_weight(FontWeight::SEMIBOLD).text_color(muted).child(h)));
            list = list.children(channels.into_iter().map(|c| {
                let id = c.id.clone();
                ui::menu_row(SharedString::from(format!("ns-pick-{}", c.id)), cx)
                    .child(div().flex_none().text_color(muted).child(channel_icon(&c).size_4()))
                    .child(ui::line(c.name.clone()).flex_1())
                    .on_click(cx.listener(move |this, _, _, cx| this.add_override(&id, cx)))
            }));
        }
        let panel = ui::surface(cx).id("ns-pick-panel").occlude().gap_1().child(ui::search_field("ns-pick-search", &view.search, cx)).child(list);
        let this = cx.entity();
        ui::dropdown("ns-pick-dropdown", panel, SELECT_HEIGHT, move |_, cx| this.update(cx, |this, cx| this.close_popover(cx)))
    }

    // Adds a row locally; Discord hears nothing until one of its controls changes.
    fn add_override(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        let Some(view) = self.notification_settings.as_mut() else { return };
        if !view.added.iter().any(|id| id == channel_id) {
            view.added.push(channel_id.to_owned());
        }
        view.open = None;
        cx.notify();
    }

    fn remove_override(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        if let Some(view) = self.notification_settings.as_mut() {
            view.added.retain(|id| id != channel_id);
        }
        self.patch_open_override(channel_id, json!({ "message_notifications": 3, "muted": false }), cx);
    }
}

fn ns_head(guild: &Guild, cx: &mut Context<Waffle>) -> Div {
    ui::row()
        .gap_3()
        .pl_6()
        .pr_4()
        .pt_5()
        .pb_3()
        .child(ui::server_tile(guild.icon.as_deref(), &guild.name, 40., cx))
        .child(
            ui::col()
                .flex_1()
                .gap_0p5()
                .child(div().text_size(ui::TEXT_LG).line_height(ui::LINE_LG).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child("Notification Settings"))
                .child(ui::line(guild.name.clone()).text_size(ui::TEXT_XS).text_color(palette::muted(cx))),
        )
        .child(ui::icon_button("ns-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.close_notification_settings(cx))))
}

fn ns_levels(settings: &GuildSettings, window: &mut Window, cx: &mut Context<Waffle>) -> Div {
    let locked = settings.is_muted();
    let rows = LEVELS.iter().enumerate().map(|(i, &(title, level))| {
        let on = settings.notifications == level;
        let row = ui::settings_row(SharedString::from(format!("ns-level-{level}")), i > 0, cx).min_h_12().child(ui::radio_live(SharedString::from(format!("ns-level-radio-{level}")), on, window, cx)).child(ui::settings_text(title, None::<&str>, cx));
        if locked {
            return row.opacity(0.45);
        }
        ui::settings_choice(row, cx).on_click(cx.listener(move |this, _, _, cx| this.patch_open_guild(json!({ "message_notifications": level }), cx)))
    }).collect::<Vec<_>>();
    ui::settings_group("Server Notification Settings", cx).child(ui::settings_card(cx).children(rows))
}

fn ns_switches(settings: &GuildSettings, window: &mut Window, cx: &mut Context<Waffle>) -> Div {
    let muted = settings.is_muted();
    // Not verified: how Discord shows an unset highlights value (0); read here as highlights on.
    let suppress_highlights = muted || settings.notify_highlights == HIGHLIGHTS_DISABLED;
    let simple = |field: &'static str, on: bool| move |this: &mut Waffle, _: &gpui_kit::ClickEvent, _: &mut Window, cx: &mut Context<Waffle>| this.patch_open_guild(json!({ field: !on }), cx);
    let everyone = switch_row("ns-suppress-everyone", "Suppress @everyone and @here", None, settings.suppress_everyone, true, false, window, cx).on_click(cx.listener(simple("suppress_everyone", settings.suppress_everyone)));
    let roles = switch_row("ns-suppress-roles", "Suppress All Role @mentions", None, settings.suppress_roles, true, true, window, cx).on_click(cx.listener(simple("suppress_roles", settings.suppress_roles)));
    let highlights = switch_row("ns-suppress-highlights", "Suppress Highlights", Some("Highlights provide occasional updates when your friends are chatting in busy servers, and more."), suppress_highlights, !muted, true, window, cx)
        .on_click(cx.listener(move |this, _, _, cx| {
            if this.open_guild_muted() {
                return;
            }
            let value = if suppress_highlights { HIGHLIGHTS_ENABLED } else { HIGHLIGHTS_DISABLED };
            this.patch_open_guild(json!({ "notify_highlights": value }), cx);
        }));
    let events = switch_row("ns-mute-events", "Mute New Events", None, settings.mute_scheduled_events, true, true, window, cx).on_click(cx.listener(simple("mute_scheduled_events", settings.mute_scheduled_events)));
    ui::settings_card(cx).child(everyone).child(roles).child(highlights).child(events)
}

fn ns_push_card(settings: &GuildSettings, window: &mut Window, cx: &mut Context<Waffle>) -> Div {
    let muted = settings.is_muted();
    let on = !muted && settings.mobile_push;
    let push = switch_row("ns-mobile-push", "Mobile Push Notifications", None, on, !muted, false, window, cx).on_click(cx.listener(move |this, _, _, cx| {
        if !this.open_guild_muted() {
            this.patch_open_guild(json!({ "mobile_push": !on }), cx);
        }
    }));
    ui::settings_card(cx).child(push)
}

// A switch setting; disabled, it dims and stops answering the pointer (the caller's click still checks).
#[allow(clippy::too_many_arguments)]
fn switch_row(id: &'static str, title: impl Into<SharedString>, detail: Option<&'static str>, on: bool, enabled: bool, line: bool, window: &mut Window, cx: &mut App) -> Stateful<Div> {
    let switch = ui::switch_live(SharedString::from(format!("{id}-switch")), on, window, cx);
    let row = ui::settings_row(id, line, cx).min_h(px(52.)).child(ui::settings_text(title, detail, cx));
    if !enabled {
        return row.opacity(0.45).child(switch);
    }
    ui::settings_choice(row, cx).child(switch)
}

// A channel's or category's override: where it sits, its level, its mute and a remove button.
fn ns_override_row(row: &OverrideRow, line: bool, guild_muted: bool, window: &mut Window, cx: &mut Context<Waffle>) -> AnyElement {
    let id = row.channel.id.clone();
    let name = row.channel.name.clone();
    let (strong, muted) = (palette::strong(cx), palette::muted(cx));
    let place = ui::col()
        .flex_1()
        .gap_px()
        .child(ui::line(name.clone()).text_color(strong))
        .child(ui::line(row.subtitle.clone()).text_size(ui::TEXT_XS).text_color(muted));
    let this = cx.entity();
    let level_id = id.clone();
    let selected = if guild_muted { SEGMENTS.len() } else { usize::from(row.level) };
    let levels = ui::segmented_live(SharedString::from(format!("ns-override-level-{id}")), &SEGMENTS, selected, move |i, _, cx| {
        this.update(cx, |this, cx| {
            if !this.open_guild_muted() {
                this.patch_open_override(&level_id, json!({ "message_notifications": i }), cx);
            }
        })
    }, window, cx);
    let (mute_id, is_muted) = (id.clone(), row.muted);
    let mute_tip = if is_muted { format!("Unmute {name}") } else { format!("Mute {name}") };
    let mute = ui::icon_button(SharedString::from(format!("ns-override-mute-{id}")), if is_muted { Glyph::BellSlash } else { Glyph::Bell }, mute_tip.clone(), cx)
        .toggled(is_muted, cx)
        .aria_label(mute_tip)
        .on_click(cx.listener(move |this, _, _, cx| this.patch_open_override(&mute_id, json!({ "muted": !is_muted }), cx)));
    let remove = ui::icon_button(SharedString::from(format!("ns-override-remove-{id}")), Glyph::X, "Remove override", cx)
        .aria_label(format!("Remove notification override for {name}"))
        .on_click(cx.listener(move |this, _, _, cx| this.remove_override(&id, cx)));
    ui::settings_row(SharedString::from(format!("ns-override-{}", row.channel.id)), line, cx)
        .min_h(px(56.))
        .gap_3()
        .child(div().flex_none().text_color(muted).child(channel_icon(&row.channel).size(ui::ICON_LG)))
        .child(place)
        .child(div().flex_none().when(guild_muted, |d| d.opacity(0.45)).child(levels))
        .child(ui::row().items_stretch().flex_none().gap_0p5().child(mute).child(remove))
        .into_any_element()
}

fn muted_until(settings: &GuildSettings) -> String {
    let end = settings.mute_end.as_deref().and_then(|e| chrono::DateTime::parse_from_rfc3339(e).ok());
    match end {
        Some(end) => format!("Muted until {}", until(end.with_timezone(&chrono::Local), chrono::Local::now())),
        None => "Muted until you turn it back on".to_owned(),
    }
}

fn channel_icon(channel: &Channel) -> Icon {
    if channel.kind == ChannelKind::Category {
        return Icon::new(Glyph::Folder);
    }
    crate::sidebar::channel_glyph(channel)
}

// What can carry an override: channels (not threads or DMs) and categories with something in them.
fn can_override(guild: &Guild, channel: &Channel) -> bool {
    match channel.kind {
        ChannelKind::Category => guild.channels.iter().any(|c| c.parent_id.as_ref() == Some(&channel.id)),
        ChannelKind::Thread | ChannelKind::Dm => false,
        _ => true,
    }
}

// The rows Discord holds an override for (a level set or a mute), in channel order, then the ones added here.
fn override_rows(guild: &Guild, settings: &GuildSettings, added: &[String]) -> Vec<OverrideRow> {
    let held = guild.channels.iter().filter(|c| settings.channels.get(&c.id).is_some_and(|o| o.notifications != 3 || o.muted));
    let mut seen = HashSet::new();
    let fresh = added.iter().filter_map(|id| guild.channels.iter().find(|c| &c.id == id));
    held.chain(fresh)
        .filter(|c| can_override(guild, c) && seen.insert(c.id.clone()))
        .map(|channel| {
            let current = settings.channels.get(&channel.id).cloned().unwrap_or_default();
            let raw_level = if settings.channels.contains_key(&channel.id) { current.notifications } else { 3 };
            OverrideRow { subtitle: place_of(guild, channel), level: effective(raw_level, settings.notifications), muted: current.muted, channel: channel.clone() }
        })
        .collect()
}

// "in {category}", "No Category", or how many channels a category holds.
fn place_of(guild: &Guild, channel: &Channel) -> String {
    if channel.kind == ChannelKind::Category {
        let count = guild.channels.iter().filter(|c| c.parent_id.as_ref() == Some(&channel.id)).count();
        return if count == 1 { "1 channel".to_owned() } else { format!("{count} channels") };
    }
    match channel.parent_id.as_ref().and_then(|p| guild.channels.iter().find(|c| &c.id == p)) {
        Some(category) => format!("in {}", category.name),
        None => "No Category".to_owned(),
    }
}

// The picker's sections: channels outside any category first, then each category followed by its
// channels, each matching `query` and without an override row yet.
fn override_choices(guild: &Guild, taken: &HashSet<&str>, query: &str) -> Vec<(Option<String>, Vec<Channel>)> {
    let fits = |c: &&Channel| can_override(guild, c) && !taken.contains(c.id.as_str()) && c.name.to_lowercase().contains(query);
    let loose: Vec<Channel> = guild.channels.iter().filter(|c| c.parent_id.is_none() && c.kind != ChannelKind::Category).filter(fits).cloned().collect();
    let mut sections = vec![(None, loose)];
    for category in guild.channels.iter().filter(|c| c.kind == ChannelKind::Category) {
        let mut rows: Vec<Channel> = std::iter::once(category).filter(fits).cloned().collect();
        rows.extend(guild.channels.iter().filter(|c| c.parent_id.as_ref() == Some(&category.id)).filter(fits).cloned());
        sections.push((Some(category.name.clone()), rows));
    }
    sections.retain(|(_, rows)| !rows.is_empty());
    sections
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discord::ChannelSettings;

    fn channel(id: &str, kind: ChannelKind, parent: Option<&str>) -> Channel {
        Channel {
            id: id.into(),
            name: id.into(),
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

    fn guild() -> Guild {
        let channels = vec![channel("cat", ChannelKind::Category, None), channel("empty", ChannelKind::Category, None), channel("a", ChannelKind::Text, Some("cat")), channel("b", ChannelKind::Text, Some("cat")), channel("loose", ChannelKind::Voice, None)];
        Guild { id: "g".into(), name: "g".into(), channels, ..Default::default() }
    }

    #[test]
    fn overrides_list_held_then_added_without_defaults() {
        let mut settings = GuildSettings { notifications: 1, ..Default::default() };
        settings.channels.insert("a".into(), ChannelSettings { notifications: 2, ..Default::default() });
        settings.channels.insert("b".into(), ChannelSettings { notifications: 3, muted: false, mute_end: None, flags: 0 });
        settings.channels.insert("cat".into(), ChannelSettings { notifications: 3, muted: true, mute_end: None, flags: 0 });
        let rows = override_rows(&guild(), &settings, &["loose".into(), "a".into()]);
        let ids: Vec<&str> = rows.iter().map(|r| r.channel.id.as_str()).collect();
        assert_eq!(ids, ["cat", "a", "loose"]);
        assert_eq!((rows[0].level, rows[0].muted, rows[0].subtitle.as_str()), (1, true, "2 channels"));
        assert_eq!((rows[1].level, rows[1].subtitle.as_str()), (2, "in cat"));
        assert_eq!((rows[2].level, rows[2].subtitle.as_str()), (1, "No Category"));
    }

    #[test]
    fn picker_skips_taken_empty_categories_and_misses() {
        let guild = guild();
        let taken: HashSet<&str> = ["a"].into_iter().collect();
        let sections = override_choices(&guild, &taken, "");
        let flat: Vec<(Option<&str>, Vec<&str>)> = sections.iter().map(|(h, rows)| (h.as_deref(), rows.iter().map(|c| c.id.as_str()).collect())).collect();
        assert_eq!(flat, [(None, vec!["loose"]), (Some("cat"), vec!["cat", "b"])]);
        assert!(override_choices(&guild, &taken, "zzz").is_empty());
    }
}
