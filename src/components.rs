// What an app's reply carries (design/explore-app-commands.html, sections 3 to 5): who used which
// command, the thinking line, the "Only you can see this" footer, a stand-in that never got an answer,
// rows of buttons and selects, and Components V2 layouts.
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use gpui_kit::{
    AnyElement, App, Bounds, Context, Div, Entity, FontWeight, Hsla, Pixels, Point, SharedString, Window,
    component::{
        Icon,
        input::{InputEvent, InputState},
    },
    div, point,
    prelude::*,
    px,
};

use crate::{
    Waffle,
    app_commands::{self, Button, ButtonStyle, Component, GalleryItem, Origin, Select, SelectKind, SelectOption},
    chat::{self, Paging, RowContext},
    discord::{Channel, ChannelKind, Emoji, File, Media, MediaKind, Message, SendState, User},
    icons::Glyph,
    interactions::{self, CommandStatus, InteractionKind},
    menu::{self, Entry},
    palette::{self, Level},
    ui::{self, RADIUS_LG, RADIUS_MD, Squircle},
};

// The prototype's `.comps`: components never run wider than this.
const MAX_WIDTH: f32 = 520.;
const THUMBNAIL: f32 = 72.;
// A media gallery item has no size of its own; a lone one is framed 16:9.
const GALLERY_ITEM: (f32, f32) = (400., 225.);
// Discord lists at most 25 options in a select.
const MAX_CHOICES: usize = 25;
// Typing pauses this long before the member search goes out, so a word costs one request.
const SEARCH_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(250);
// A container's padding and hairline on each side.
const CONTAINER_INSET: f32 = 16. + 1.;

// A control on a message: the message's id and the control's custom id.
pub(crate) type ControlKey = (String, String);

#[derive(Default)]
pub(crate) struct ComponentState {
    // The select whose menu was opened last; it reads as open only while the menu is.
    open: Option<ControlKey>,
    // A multi-select's picks before Done, and what a select last sent, shown until the message changes.
    picks: HashMap<ControlKey, Vec<String>>,
    // Each select field's bounds as last painted: its menu opens under it.
    anchors: Anchors,
    // The search atop a user, role, channel or mentionable select's menu (made on its first open), what's
    // typed in it, and the server whose members it looks up as you type.
    search: Option<Entity<InputState>>,
    query: String,
    search_guild: Option<String>,
    // Bumped per keystroke: a debounced member search goes out only if no newer one replaced it.
    search_seq: u64,
}

type Anchors = Rc<RefCell<HashMap<ControlKey, Bounds<Pixels>>>>;

// What message rows read: the open select, the picks, and each control's in-flight or failed interaction.
#[derive(Default)]
pub(crate) struct ComponentView {
    open: Option<ControlKey>,
    picks: HashMap<ControlKey, Vec<String>>,
    calls: HashMap<ControlKey, Call>,
    // Messages with an interaction in flight: all their controls are disabled.
    busy: HashSet<String>,
    anchors: Anchors,
}

struct Call {
    // Discord's line once it failed.
    failure: Option<String>,
}

impl Waffle {
    pub(crate) fn component_view(&self) -> ComponentView {
        let calls = self
            .interactions
            .values()
            .filter_map(|p| match &p.kind {
                InteractionKind::Component { message_id, custom_id } => Some(((message_id.clone(), custom_id.clone()), Call { failure: p.failure.clone() })),
                _ => None,
            })
            .collect();
        let busy = self
            .interactions
            .values()
            .filter_map(|p| match &p.kind {
                InteractionKind::Component { message_id, .. } if !p.failed() => Some(message_id.clone()),
                _ => None,
            })
            .collect();
        ComponentView { open: self.component_state.open.clone().filter(|_| self.menu.is_some()), picks: self.component_state.picks.clone(), calls, busy, anchors: self.component_state.anchors.clone() }
    }

    // A button press or a select's picks, sent to the app that owns the message.
    fn use_component(&mut self, message: &Message, component: &Component, values: Vec<String>, cx: &mut Context<Self>) {
        let Some(custom_id) = custom_id(component) else { return };
        if self.message_busy(&message.id) {
            return;
        }
        self.forget_failed_on(&message.id);
        let application_id = message.application_id.clone().unwrap_or_else(|| message.author.id.clone());
        let nonce = crate::nonce();
        let guild_id = message.guild_id.clone().or_else(|| self.guild_id.clone());
        let origin = Origin { channel_id: &message.channel_id, guild_id: guild_id.as_deref(), session_id: &self.gateway_session, nonce: &nonce };
        let body = app_commands::component_body(&application_id, &message.id, message.flags, component, &values, &origin);
        let icon = Some(message.author.avatar.clone()).filter(|a| !a.is_empty());
        let app = app_commands::App { id: application_id, name: message.author.name.clone(), icon };
        self.interact(nonce, app, InteractionKind::Component { message_id: message.id.clone(), custom_id }, body, cx);
    }

    // Opens the select's menu under its field and as wide (at `fallback` before the field has painted).
    fn open_select(&mut self, message: Message, select: Select, fallback: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let anchor = self.component_state.anchors.borrow().get(&(message.id.clone(), select.custom_id.clone())).copied();
        let at = anchor.map_or(fallback, |b| point(b.left(), b.bottom() + px(4.)));
        self.component_state.open = Some((message.id.clone(), select.custom_id.clone()));
        let search = (select.kind != SelectKind::String).then(|| self.reset_select_search(&message, &select, window, cx));
        self.open_live_menu(at, move |this| this.select_entries(&message, &select, search.clone()), cx);
        if let Some(bounds) = anchor {
            self.set_menu_width(f32::from(bounds.size.width));
        }
    }

    // The menu's search field, emptied, and the server it looks members up in.
    fn reset_select_search(&mut self, message: &Message, select: &Select, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        let lists_members = matches!(select.kind, SelectKind::User | SelectKind::Mentionable);
        self.component_state.search_guild = self.select_guild(message).map(|g| g.id.clone()).filter(|_| lists_members);
        self.component_state.query.clear();
        let input = match self.component_state.search.clone() {
            Some(input) => input,
            None => self.new_select_search(window, cx),
        };
        input.update(cx, |state, cx| state.set_value("", window, cx));
        input
    }

    fn new_select_search(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        cx.subscribe(&input, |this: &mut Self, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.search_select(input.read(cx).value().to_string(), cx);
            }
        })
        .detach();
        self.component_state.search = Some(input.clone());
        input
    }

    // Filters the open menu at once; in a server, a pause in typing also asks the gateway for members
    // matching it, whose chunk lands in `members` and so in the menu.
    fn search_select(&mut self, query: String, cx: &mut Context<Self>) {
        let state = &mut self.component_state;
        state.query = query.trim().to_owned();
        state.search_seq += 1;
        cx.notify();
        if state.query.is_empty() || state.search_guild.is_none() {
            return;
        }
        let seq = state.search_seq;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_DEBOUNCE).await;
            this.update(cx, |this, _| this.send_member_search(seq)).ok();
        })
        .detach();
    }

    fn send_member_search(&self, seq: u64) {
        let state = &self.component_state;
        let (Some(client), Some(guild_id)) = (&self.client, &state.search_guild) else { return };
        if seq != state.search_seq || self.menu.is_none() {
            return;
        }
        client.search_members(guild_id, &state.query, MAX_CHOICES as u32);
    }

    // Options with their emoji and description, under the search when there is one; a multi-select
    // checks its picks and ends with Done.
    fn select_entries(&self, message: &Message, select: &Select, search: Option<Entity<InputState>>) -> Vec<Entry> {
        let key = (message.id.clone(), select.custom_id.clone());
        let picks = self.component_state.picks.get(&key).cloned().unwrap_or_default();
        let multi = is_multi(select);
        let full = picks.len() >= select.max as usize;
        let mut entries: Vec<Entry> = self
            .select_choices(message, select)
            .into_iter()
            .map(|option| {
                let picked = picks.contains(&option.value);
                let (m, s, value) = (message.clone(), select.clone(), option.value.clone());
                let mut item = menu::item(option.label, move |this, _, cx| this.pick_option(&m, &s, &value, cx));
                if let Some(hint) = option.description {
                    item = item.hint(hint);
                }
                if let Some(emoji) = option.emoji {
                    item = item.emoji(as_emoji(&emoji));
                }
                if multi {
                    item = item.checked(picked);
                }
                // At the cap, the rest wait until a pick is cleared.
                if multi && full && !picked {
                    item = item.disabled();
                }
                item.entry()
            })
            .collect();
        if entries.is_empty() {
            entries.push(menu::item(self.select_empty(message, select), |_, _, _| {}).disabled().entry());
        }
        if let Some(input) = search {
            entries.insert(0, Entry::Search(input));
        }
        if !multi {
            return entries;
        }
        let (m, s) = (message.clone(), select.clone());
        let done = menu::item("Done", move |this, _, cx| this.finish_select(&m, &s, cx)).brand();
        let done = match can_finish(picks.len(), select.min) {
            true => done,
            false => done.hint(format!("Pick at least {}", select.min)).disabled(),
        };
        entries.push(Entry::Separator);
        entries.push(done.entry());
        entries
    }

    // A user, role, channel or mentionable select's choices that match the search, at most 25.
    fn select_choices(&self, message: &Message, select: &Select) -> Vec<SelectOption> {
        if select.kind == SelectKind::String {
            return select.options.clone();
        }
        let all = match self.select_dm(message) {
            Some(dm) => self.dm_choices(dm, select.kind),
            None => self.server_choices(message, select.kind),
        };
        let query = self.component_state.query.to_lowercase();
        all.into_iter().filter(|c| c.matches.contains(&query)).take(MAX_CHOICES).map(|c| c.option).collect()
    }

    // What an empty menu says instead of showing nothing.
    fn select_empty(&self, message: &Message, select: &Select) -> &'static str {
        if !self.component_state.query.is_empty() {
            return "No matches";
        }
        if select.kind == SelectKind::Role && self.select_dm(message).is_some() {
            return "No roles in a DM";
        }
        "Nothing to pick here"
    }

    fn select_dm(&self, message: &Message) -> Option<&Channel> {
        self.dms.iter().find(|c| c.id == message.channel_id)
    }

    // In a DM: its recipients and you, or the DM itself as its one channel. It has no roles.
    fn dm_choices(&self, dm: &Channel, kind: SelectKind) -> Vec<Choice> {
        match kind {
            SelectKind::User | SelectKind::Mentionable => sorted(dm.recipients.iter().chain(self.me.as_ref()).map(Choice::member).collect()),
            SelectKind::Channel => vec![Choice::named(&dm.name, &dm.id)],
            SelectKind::Role | SelectKind::String => Vec::new(),
        }
    }

    // GAP: members come from what this server has cached (the member list's, and what a search fetched).
    fn server_choices(&self, message: &Message, kind: SelectKind) -> Vec<Choice> {
        let Some(guild) = self.select_guild(message) else { return Vec::new() };
        let users = sorted(self.members.get(&guild.id).into_iter().flat_map(|m| m.values()).map(Choice::member).collect());
        // The @everyone role shares the server's id.
        let roles = guild.roles.iter().filter(|r| r.id != guild.id).map(|r| Choice::named(&r.name, &r.id));
        let channels = guild.channels.iter().filter(|c| c.kind != ChannelKind::Category).map(|c| Choice::named(&c.name, &c.id));
        match kind {
            SelectKind::User => users,
            SelectKind::Role => roles.collect(),
            SelectKind::Mentionable => users.into_iter().chain(roles).collect(),
            SelectKind::Channel => channels.collect(),
            SelectKind::String => Vec::new(),
        }
    }

    fn select_guild(&self, message: &Message) -> Option<&crate::discord::Guild> {
        let guild_id = message.guild_id.as_ref().or(self.guild_id.as_ref())?;
        self.guilds.iter().find(|g| &g.id == guild_id)
    }

    // Demo scenes: `picks` checked and the select's menu open, once its field has painted.
    #[cfg(debug_assertions)]
    pub(crate) fn demo_open_select(&mut self, message: Message, custom_id: &str, picks: &[&str], window: &mut Window, cx: &mut Context<Self>) {
        let Some(select) = find_select(&message.components, custom_id) else { return };
        self.component_state.picks.insert((message.id.clone(), custom_id.to_owned()), picks.iter().map(|p| (*p).to_owned()).collect());
        self.open_select(message, select, point(px(80.), px(400.)), window, cx);
    }

    fn pick_option(&mut self, message: &Message, select: &Select, value: &str, cx: &mut Context<Self>) {
        let key = (message.id.clone(), select.custom_id.clone());
        if is_multi(select) {
            let picks = self.component_state.picks.entry(key).or_default();
            *picks = toggle_pick(picks, value, select.max);
            cx.notify();
            return;
        }
        self.component_state.picks.insert(key, vec![value.to_owned()]);
        self.component_state.open = None;
        self.use_component(message, &Component::Select(select.clone()), vec![value.to_owned()], cx);
    }

    fn finish_select(&mut self, message: &Message, select: &Select, cx: &mut Context<Self>) {
        let key = (message.id.clone(), select.custom_id.clone());
        let picks = self.component_state.picks.get(&key).cloned().unwrap_or_default();
        if !can_finish(picks.len(), select.min) {
            return;
        }
        self.component_state.open = None;
        self.use_component(message, &Component::Select(select.clone()), picks, cx);
    }
}

// The line above an app's reply: "[avatar] Mira used /play". A component's reply names no command.
pub(crate) fn used_command(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> Option<AnyElement> {
    let meta = message.interaction.as_ref()?;
    let command = meta.command.as_ref()?;
    let you = ctx.me.as_ref().is_some_and(|me| me.id == meta.user.id);
    let name = if you { "You".to_owned() } else { ctx.name_of(&meta.user) };
    let strong = palette::strong(cx);
    Some(
        div()
            .pl(px(chat::CONTENT_INSET))
            .h(px(22.))
            .flex()
            .items_center()
            .gap(px(6.))
            .min_w_0()
            .text_size(px(13.))
            .text_color(palette::muted(cx))
            .child(ui::avatar(meta.user.avatar.clone(), 16.))
            .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).text_color(strong).child(name))
            .child(div().flex_none().child("used"))
            .child(div().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).text_color(palette::accent_ink(cx)).child(format!("/{command}")))
            .into_any_element(),
    )
}

// The line under a command's stand-in or a deferred reply: Discord's "Sending command...", "{app} is
// thinking...", or why it never answered.
pub(crate) fn interaction_status(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> Option<AnyElement> {
    let waiting = |text: String| {
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .italic()
            .text_color(palette::muted(cx))
            .child(ui::bars(SharedString::from(format!("thinking-{}", message.id)), palette::accent_ink(cx), 14.))
            .child(text)
            .into_any_element()
    };
    // Discord names the app by its bot's username.
    let app = Some(message.author.username.clone()).filter(|u| !u.is_empty()).unwrap_or_else(|| ctx.name_of(&message.author));
    Some(match interactions::command_status(message, interactions::now_ms())? {
        CommandStatus::Sending => waiting("Sending command...".into()),
        CommandStatus::Thinking => waiting(format!("{app} is thinking...")),
        CommandStatus::TimedOut => failure_line(interactions::DID_NOT_RESPOND.into(), cx),
        CommandStatus::Failed(why) => failure_line(why, cx),
    })
}

pub(crate) fn is_ephemeral(message: &Message) -> bool {
    message.flags & app_commands::EPHEMERAL != 0 || is_failed_stand_in(message)
}

// Our command's local stand-in, which the app never answered: only we ever saw it.
pub(crate) fn is_failed_stand_in(message: &Message) -> bool {
    message.state == SendState::Failed && interactions::is_stand_in(message)
}

// An ephemeral reply's tint: a 4% wash of the strong ink.
pub(crate) fn ephemeral_tint(cx: &App) -> Hsla {
    palette::strong(cx).opacity(0.04)
}

// "Only you can see this · Dismiss message"; dismissing drops it here, as Discord's does.
pub(crate) fn only_you(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> AnyElement {
    let (this, channel_id, id) = (ctx.this(), message.channel_id.clone(), message.id.clone());
    div()
        .mt(px(6.))
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(px(12.))
        .text_color(palette::muted(cx))
        .child(Icon::new(Glyph::Eye).size(px(14.)))
        .child("Only you can see this ·")
        .child(ui::button(SharedString::from(format!("dismiss-{}", message.id)), ui::Tone::Link, cx).text_size(px(12.)).child("Dismiss message").on_click(move |_, _, cx| {
            cx.stop_propagation();
            this.update(cx, |this, cx| {
                this.remove_message(&channel_id, &id);
                cx.notify();
            })
            .ok();
        }))
        .into_any_element()
}

// Discord's red failure line: an error icon and what went wrong. No retry.
fn failure_line(text: String, cx: &App) -> AnyElement {
    div()
        .mt(px(4.))
        .flex()
        .items_center()
        .gap(px(6.))
        .text_color(palette::danger_ink(cx))
        .child(Icon::new(Glyph::WarningCircle).size(px(16.)).flex_none())
        .child(div().min_w_0().child(text))
        .into_any_element()
}

// Under a row or section: the line for the failed interaction on one of its controls.
fn failed_line<'a>(message: &Message, controls: impl IntoIterator<Item = &'a Component>, ctx: &RowContext, cx: &App) -> Option<AnyElement> {
    let failure = controls.into_iter().filter_map(custom_id).find_map(|id| ctx.components.calls.get(&(message.id.clone(), id)).and_then(|c| c.failure.clone()))?;
    Some(failure_line(failure, cx))
}

// Under the message body: rows of buttons and selects, or (with COMPONENTS_V2) the whole message.
pub(crate) fn message_components(message: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> Option<AnyElement> {
    if message.components.is_empty() {
        return None;
    }
    let width = MAX_WIDTH.min(ctx.column());
    let children: Vec<AnyElement> = message.components.iter().enumerate().map(|(i, c)| component(message, c, i.to_string(), width, ctx, cx)).collect();
    Some(div().mt(px(8.)).w_full().max_w(px(width)).flex().flex_col().gap(px(8.)).children(children).into_any_element())
}

// `path` keeps element ids apart: each component's index, from the top level down. `width` is the room
// it has, which a gallery fills.
fn component(message: &Message, c: &Component, path: String, width: f32, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    match c {
        Component::Row(children) => action_row(message, children, &path, width, ctx, cx),
        Component::Button(b) => button(message, b, &path, ctx, cx),
        Component::Select(s) => select_field(message, s, &path, ctx, cx),
        Component::Section { children, accessory } => section(message, children, accessory.as_deref(), &path, width, ctx, cx),
        Component::Text(text) => chat::rich_text(&format!("component-{}-{path}", message.id), text, ctx, cx),
        Component::Thumbnail { url, .. } => thumbnail(message, url),
        Component::Gallery(items) => gallery(message, items, &path, width, ctx, cx),
        Component::File { url, name, .. } => file(message, url, name.as_deref(), cx),
        Component::Separator { divider, large } => separator(*divider, *large, cx),
        Component::Container { accent, spoiler, children } => container(message, *accent, children, &path, width, ctx, cx)
            .when(*spoiler, |c| c.children(chat::veil(format!("spoiler-{}-{path}", message.id), "Spoiler", ui::all(RADIUS_LG), ctx)))
            .into_any_element(),
    }
}

// Up to five buttons, wrapping, or one select; a failed press says so under the row with a retry.
fn action_row(message: &Message, children: &[Component], path: &str, width: f32, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    let controls: Vec<AnyElement> = children.iter().enumerate().map(|(i, c)| component(message, c, format!("{path}-{i}"), width, ctx, cx)).collect();
    let failed = failed_line(message, children, ctx, cx);
    div()
        .flex()
        .flex_col()
        .child(div().flex().flex_wrap().gap(px(8.)).children(controls))
        .children(failed)
        .into_any_element()
}

fn button(message: &Message, b: &Button, path: &str, ctx: &Rc<RowContext>, cx: &App) -> AnyElement {
    let call = b.custom_id.as_ref().and_then(|id| ctx.components.calls.get(&(message.id.clone(), id.clone())));
    let busy = call.is_some_and(|c| c.failure.is_none());
    // Another control on this message is in flight.
    let blocked = !busy && ctx.components.busy.contains(&message.id);
    let label = b.label.clone().unwrap_or_default();
    let link = b.style == ButtonStyle::Link;
    let (this, m, press) = (ctx.this(), message.clone(), b.clone());
    ui::button(SharedString::from(format!("component-{}-{path}", message.id)), tone(b.style), cx)
        .small()
        .disabled(b.disabled || blocked || message.state != SendState::Sent)
        .children(b.emoji.as_deref().map(|e| menu::emoji_face(&as_emoji(e), 18.)))
        .when(!label.is_empty(), |c| c.child(label.clone()))
        .when(link, |c| c.child(Icon::new(Glyph::Open).size(px(14.))))
        .when(busy, |c| c.loading(Some(label)))
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            if let Some(url) = press.url.as_ref().filter(|_| press.style == ButtonStyle::Link) {
                return cx.open_url(url);
            }
            this.update(cx, |this, cx| this.use_component(&m, &Component::Button(press.clone()), Vec::new(), cx)).ok();
        })
        .into_any_element()
}

// A field with the placeholder or the picks; clicking opens the options as a menu under it.
fn select_field(message: &Message, s: &Select, path: &str, ctx: &Rc<RowContext>, cx: &App) -> AnyElement {
    let key = (message.id.clone(), s.custom_id.clone());
    let open = ctx.components.open.as_ref() == Some(&key);
    let busy = ctx.components.calls.get(&key).is_some_and(|c| c.failure.is_none());
    let blocked = !busy && ctx.components.busy.contains(&message.id);
    let picked = picked_labels(s, ctx.components.picks.get(&key), ctx);
    let shown = picked.clone().or_else(|| s.placeholder.clone()).unwrap_or_else(|| "Make a selection".into());
    let (anchors, anchor_key) = (ctx.components.anchors.clone(), key.clone());
    let (this, m, select) = (ctx.this(), message.clone(), s.clone());
    let field = ui::select(SharedString::from(format!("component-{}-{path}", message.id)), None, shown.clone(), open, cx)
        .disabled(s.disabled || blocked || message.state != SendState::Sent)
        .when(picked.is_none(), |f| f.text_color(palette::faint(cx)))
        .when(busy, |f| f.loading(Some(shown)))
        .on_click(move |event, window, cx| {
            cx.stop_propagation();
            this.update(cx, |this, cx| this.open_select(m.clone(), select.clone(), event.position(), window, cx)).ok();
        });
    div().relative().w_full().child(gpui_kit::canvas(move |b, _, _| {
            anchors.borrow_mut().insert(anchor_key.clone(), b);
        }, |_, _, _, _| {}).absolute().size_full()).child(field).into_any_element()
}

// What a select shows once something is picked: option labels, or the names behind user, role and
// channel ids. Options the app marks default show until you pick.
fn picked_labels(s: &Select, picks: Option<&Vec<String>>, ctx: &RowContext) -> Option<String> {
    let values: Vec<String> = match picks.filter(|p| !p.is_empty()) {
        Some(picks) => picks.clone(),
        None => s.options.iter().filter(|o| o.default).map(|o| o.value.clone()).collect(),
    };
    let labels: Vec<String> = values.iter().map(|v| s.options.iter().find(|o| &o.value == v).map(|o| o.label.clone()).or_else(|| ctx.name_for_id(v)).unwrap_or_else(|| v.clone())).collect();
    (!labels.is_empty()).then(|| labels.join(", "))
}

// Text on the left; a thumbnail or a button beside it on the right.
fn section(message: &Message, children: &[Component], accessory: Option<&Component>, path: &str, width: f32, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    let text: Vec<AnyElement> = children.iter().enumerate().map(|(i, c)| component(message, c, format!("{path}-{i}"), width, ctx, cx)).collect();
    let failed = failed_line(message, accessory, ctx, cx);
    let accessory = accessory.map(|a| component(message, a, format!("{path}-a"), width, ctx, cx));
    let row = div()
        .flex()
        .items_start()
        .gap(px(14.))
        .child(div().flex_1().min_w_0().flex().flex_col().gap(px(4.)).text_color(palette::strong(cx)).children(text))
        .children(accessory.map(|a| div().flex_none().child(a)));
    div().flex().flex_col().child(row).children(failed).into_any_element()
}

fn thumbnail(message: &Message, url: &str) -> AnyElement {
    ui::media(resolve(message, url), ui::all(RADIUS_MD)).size(px(THUMBNAIL)).flex_none().object_fit(gpui_kit::ObjectFit::Cover).into_any_element()
}

// The feed's media grid over the gallery's items, `width` wide; the viewer pages through these alone.
fn gallery(message: &Message, items: &[GalleryItem], path: &str, width: f32, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    let media = items.iter().map(|i| Media { url: resolve(message, &i.url), width: GALLERY_ITEM.0, height: GALLERY_ITEM.1, kind: MediaKind::Image }).collect();
    let spoilers: Vec<bool> = items.iter().map(|i| i.spoiler).collect();
    let grid = Message { id: format!("{}-gallery-{path}", message.id), channel_id: message.channel_id.clone(), media, ..Default::default() };
    chat::media_layout(&grid, width, Paging::Gallery { spoilers: &spoilers }, ctx, cx).map_or_else(|| div().into_any_element(), IntoElement::into_any_element)
}

// The file card, sized from the attachment it points at when the message carries one.
fn file(message: &Message, url: &str, name: Option<&str>, cx: &App) -> AnyElement {
    let attached = attachment_name(url).and_then(|n| message.files.iter().find(|f| f.name == n)).cloned();
    let file = attached.unwrap_or_else(|| File { name: name.map(str::to_owned).or_else(|| url.rsplit('/').next().map(str::to_owned)).unwrap_or_default(), url: url.to_owned(), size: 0 });
    chat::file_card(message, &file, cx)
}

fn separator(divider: bool, large: bool, cx: &App) -> AnyElement {
    div().py(px(if large { 8. } else { 2. })).when(divider, |d| d.child(div().h(px(1.)).bg(palette::line(cx)))).into_any_element()
}

// A raised card; the app's accent colour becomes a light tint and a tinted hairline, never a stripe.
fn container(message: &Message, accent: Option<u32>, children: &[Component], path: &str, width: f32, ctx: &Rc<RowContext>, cx: &mut App) -> Div {
    let raised = palette::surface(Level::Raised, cx);
    let (fill, edge) = match accent {
        Some(color) => {
            let tint = Hsla::from(gpui_kit::rgb(color));
            (ui::mix(raised, tint, 0.07), tint.opacity(0.35))
        }
        None => (raised, palette::line(cx)),
    };
    let inner = width - 2. * CONTAINER_INSET;
    let children: Vec<AnyElement> = children.iter().enumerate().map(|(i, c)| component(message, c, format!("{path}-{i}"), inner, ctx, cx)).collect();
    div().relative().w_full().py(px(14.)).px(px(16.)).flex().flex_col().gap(px(10.)).squircle(RADIUS_LG).bg(fill).border_1().border_color(edge).children(children)
}

#[cfg(debug_assertions)]
fn find_select(components: &[Component], custom_id: &str) -> Option<Select> {
    components.iter().find_map(|c| match c {
        Component::Select(s) if s.custom_id == custom_id => Some(s.clone()),
        Component::Row(children) | Component::Container { children, .. } | Component::Section { children, .. } => find_select(children, custom_id),
        _ => None,
    })
}

fn custom_id(c: &Component) -> Option<String> {
    match c {
        Component::Button(b) => b.custom_id.clone(),
        Component::Select(s) => Some(s.custom_id.clone()),
        _ => None,
    }
}

// An app's button styles in Waffle's tones; a link opens the browser and says so with its icon.
fn tone(style: ButtonStyle) -> ui::Tone {
    match style {
        ButtonStyle::Primary => ui::Tone::Primary,
        ButtonStyle::Success => ui::Tone::Positive,
        ButtonStyle::Danger => ui::Tone::Danger,
        ButtonStyle::Secondary | ButtonStyle::Link | ButtonStyle::Premium => ui::Tone::Secondary,
    }
}

fn is_multi(select: &Select) -> bool {
    select.max > 1
}

// A multi-select's picks after clicking `value`: cleared if picked, added while under `max`.
fn toggle_pick(picks: &[String], value: &str, max: u32) -> Vec<String> {
    if picks.iter().any(|p| p == value) {
        return picks.iter().filter(|p| *p != value).cloned().collect();
    }
    if picks.len() >= max as usize {
        return picks.to_vec();
    }
    picks.iter().cloned().chain(std::iter::once(value.to_owned())).collect()
}

fn can_finish(count: usize, min: u32) -> bool {
    count >= min as usize
}

// A user, role or channel option and the lowercase text a search matches it on: its label, and for a
// member their account username too (a server search matches either, and the label may be a nickname).
struct Choice {
    option: SelectOption,
    matches: String,
}

impl Choice {
    fn named(label: &str, value: &str) -> Self {
        Choice { option: choice(label, value), matches: label.to_lowercase() }
    }

    fn member(user: &User) -> Self {
        Choice { option: choice(&user.name, &user.id), matches: format!("{}\n{}", user.name, user.username).to_lowercase() }
    }
}

fn sorted(mut choices: Vec<Choice>) -> Vec<Choice> {
    choices.sort_by_key(|c| c.option.label.to_lowercase());
    choices
}


fn choice(label: &str, value: &str) -> SelectOption {

    SelectOption { label: label.to_owned(), value: value.to_owned(), description: None, emoji: None, default: false }
}

// A component's emoji: a Unicode character, or a custom emoji's CDN url as `app_commands` writes it.
fn as_emoji(emoji: &str) -> Emoji {
    let Some(file) = emoji.split("/emojis/").nth(1).filter(|_| emoji.starts_with("http")) else { return Emoji::unicode(emoji) };
    let (id, rest) = file.split_once('.').unwrap_or((file, ""));
    Emoji { id: Some(id.to_owned()), name: String::new(), animated: rest.starts_with("gif") }
}

// Not verified: a V2 message's media points at its own attachments as `attachment://<name>`; Discord is
// believed to send them resolved, so this only matters if it doesn't.
fn resolve(message: &Message, url: &str) -> String {
    let Some(name) = attachment_name(url) else { return url.to_owned() };
    let in_path = |u: &str| u.split('?').next().is_some_and(|p| p.ends_with(&format!("/{name}")));
    message.media.iter().map(|m| m.url.as_str()).chain(message.files.iter().map(|f| f.url.as_str())).find(|u| in_path(u)).unwrap_or(url).to_owned()
}

fn attachment_name(url: &str) -> Option<&str> {
    url.strip_prefix("attachment://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_styles_take_waffle_tones() {
        assert_eq!(tone(ButtonStyle::Primary), ui::Tone::Primary);
        assert_eq!(tone(ButtonStyle::Success), ui::Tone::Positive);
        assert_eq!(tone(ButtonStyle::Danger), ui::Tone::Danger);
        for neutral in [ButtonStyle::Secondary, ButtonStyle::Link, ButtonStyle::Premium] {
            assert_eq!(tone(neutral), ui::Tone::Secondary);
        }
    }

    #[test]
    fn multi_select_picks_toggle_up_to_max_and_finish_at_min() {
        let picks = toggle_pick(&[], "games", 2);
        let picks = toggle_pick(&picks, "music", 2);
        assert_eq!(picks, ["games", "music"]);
        assert_eq!(toggle_pick(&picks, "movies", 2), picks, "a full select ignores another pick");
        assert_eq!(toggle_pick(&picks, "games", 2), ["music"]);
        assert!(!can_finish(1, 2));
        assert!(can_finish(2, 2));
        assert!(can_finish(0, 0));
    }

    #[test]
    fn component_emoji_reads_unicode_and_cdn_urls() {
        assert_eq!(as_emoji("🎮").name, "🎮");
        let custom = as_emoji("https://cdn.discordapp.com/emojis/123.gif?size=48");
        assert_eq!((custom.id.as_deref(), custom.animated), (Some("123"), true));
    }

    #[test]
    fn members_match_on_their_shown_name_and_username() {
        let mira = Choice::member(&User { id: "1".into(), name: "Mira (host)".into(), username: "mira_k".into(), ..Default::default() });
        assert!(mira.matches.contains("host"));
        assert!(mira.matches.contains("mira_k"));
        assert!(!mira.matches.contains("theo"));
        assert_eq!(Choice::named("Moderators", "9").matches, "moderators");
    }

    #[test]
    fn attachment_urls_resolve_to_the_message_files() {

        let message = Message { files: vec![File { name: "log.txt".into(), url: "https://cdn.discordapp.com/attachments/1/2/log.txt?ex=1".into(), size: 3 }], ..Default::default() };
        assert_eq!(resolve(&message, "attachment://log.txt"), message.files[0].url);
        assert_eq!(resolve(&message, "https://x/y.png"), "https://x/y.png");
    }
}
