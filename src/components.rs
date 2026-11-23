// What an app's reply carries (design/explore-app-commands.html, sections 3 to 5): who used which
// command, the thinking line, the "Only you can see this" footer, a stand-in that never got an answer,
// rows of buttons and selects, and Components V2 layouts. Selects in an app's form (app_modal.rs) open
// the same menus.
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use gpui_kit::{
    AnyElement, App, Bounds, ClickEvent, Context, Div, Entity, FontWeight, Hsla, Pixels, Point, SharedString, Window,
    component::{
        Icon,
        input::{InputEvent, InputState},
    },
    div, point,
    prelude::*,
    px,
};
use serde_json::Value;

use crate::{
    Waffle,
    app_commands::{self, Button, ButtonStyle, Component, DefaultValue, GalleryItem, Origin, Select, SelectKind, SelectOption},
    chat::{self, Paging, RowContext},
    discord::{Channel, ChannelKind, Emoji, File, Guild, Media, MediaKind, Message, SendState, User},
    icons::Glyph,
    interactions::{self, CommandStatus, InteractionKind},
    menu::{self, Entry},
    palette::{self, Level},
    ui::{self, RADIUS_LG, RADIUS_MD, Squircle},
};

mod parts;

use self::parts::*;

mod status;

pub(crate) use self::status::*;

// A select's list opens this far under its anchor.
const SELECT_GAP: f32 = 4.;
// Where the demo scene opens a select.
const DEMO_SELECT_X: f32 = 80.;
// Where the demo scene opens a select.
const DEMO_SELECT_Y: f32 = 400.;
// The height of the line naming who used a component.
const CAPTION_HEIGHT: f32 = 22.;
// The row the eye mark sits in, one text line.
const SEEN_ICON_HEIGHT: f32 = 18.;

// The prototype's `.comps`: components never run wider than this.
const MAX_WIDTH: f32 = 520.;
// Discord fits a thumbnail inside 170×170 at its own shape, no side under 85.
const THUMBNAIL_MAX: f32 = 170.;
const THUMBNAIL_MIN: f32 = 85.;
// A media gallery item Discord sent without its size is framed 16:9.
const GALLERY_ITEM: (f32, f32) = (400., 225.);
// Discord lists at most 25 options in a select.
const MAX_CHOICES: usize = 25;
// Typing pauses this long before the member search goes out, so a word costs one request.
const SEARCH_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(250);
// Discord's container: 16 padding, 8 between children, a hairline.
const CONTAINER_PAD: f32 = 16.;
const CONTAINER_GAP: f32 = 8.;
const CONTAINER_INSET: f32 = CONTAINER_PAD + 1.;
// Discord's "reduced" markup size for text inside a container.
// Not verified: the px value; Discord's class sets it in a CSS module this build didn't include.
const REDUCED_TEXT: f32 = 14.;
// The control key's message id for selects in the open app form.
pub(crate) const MODAL_KEY: &str = "app-modal";
// Discord's SKU type for a subscription.
const SKU_SUBSCRIPTION: u64 = 5;

// A control on a message: the message's id and the control's custom id.
pub(crate) type ControlKey = (String, String);

#[derive(Default)]
pub(crate) struct ComponentState {
    // The select whose menu was opened last; it reads as open only while the menu is.
    open: Option<OpenSelect>,
    // A message select's picks as they stand, and what it last sent: closing its menu sends the picks
    // when they differ. Both start at the select's defaults.
    picks: HashMap<ControlKey, Vec<String>>,
    sent: HashMap<ControlKey, Vec<String>>,
    // Each select field's bounds as last painted: its menu opens under it.
    anchors: Anchors,
    // The search atop a user, role, channel or mentionable select's menu (made on its first open), what's
    // typed in it, and the server whose members it looks up as you type.
    search: Option<Entity<InputState>>,
    query: String,
    search_guild: Option<String>,
    // Bumped per keystroke: a debounced member search goes out only if no newer one replaced it.
    search_seq: u64,
    // Premium buttons' SKUs by id, fetched when a button first draws.
    skus: Skus,
}

type Anchors = Rc<RefCell<HashMap<ControlKey, Bounds<Pixels>>>>;
type Skus = Rc<RefCell<HashMap<String, Sku>>>;

#[derive(Clone)]
struct OpenSelect {
    host: SelectHost,
    select: Select,
}

// Where a select lives: on a message, whose app hears from it when its menu closes, or in the open
// app form (by field index), where picks wait for Submit.
#[derive(Clone)]
pub(crate) enum SelectHost {
    Message(Box<Message>),
    Modal(usize),
}

impl SelectHost {
    fn key(&self, select: &Select) -> ControlKey {
        match self {
            SelectHost::Message(m) => (m.id.clone(), select.custom_id.clone()),
            SelectHost::Modal(_) => (MODAL_KEY.to_owned(), select.custom_id.clone()),
        }
    }
}

// A premium button's label: Discord's dots until its SKU loads, then "{sku} - {price}" or "Product
// Unavailable".
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Sku {
    Loading,
    Ready(String),
    Unavailable,
}

const PRODUCT_UNAVAILABLE: &str = "Product Unavailable";

// What message rows read: the open select, the picks, and each control's in-flight or failed interaction.
#[derive(Default)]
pub(crate) struct ComponentView {
    open: Option<ControlKey>,
    picks: HashMap<ControlKey, Vec<String>>,
    // User, role, channel and mentionable selects' `default_values` that resolve here.
    defaults: HashMap<ControlKey, Vec<SelectOption>>,
    calls: HashMap<ControlKey, Call>,
    // Messages with an interaction in flight: all their controls are disabled.
    busy: HashSet<String>,
    anchors: Anchors,
    skus: Skus,
}

struct Call {
    // Discord's line once it failed.
    failure: Option<String>,
}

// The server or DM a select picks from.
#[derive(Clone, Copy)]
struct Place<'a> {
    guild: Option<&'a Guild>,
    dm: Option<&'a Channel>,
}

impl Waffle {
    pub(crate) fn component_view(&self, messages: &[Message]) -> ComponentView {
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
        let defaults = messages
            .iter()
            .flat_map(|m| selects(&m.components).into_iter().filter(|s| !s.default_values.is_empty()).map(move |s| (m, s)))
            .map(|(m, s)| ((m.id.clone(), s.custom_id.clone()), self.select_defaults(self.message_place(m), s)))
            .collect();
        let state = &self.component_state;
        ComponentView {
            open: self.open_select_key(),
            picks: state.picks.clone(),
            defaults,
            calls,
            busy,
            anchors: state.anchors.clone(),
            skus: state.skus.clone(),
        }
    }

    fn open_select_key(&self) -> Option<ControlKey> {
        let open = self.component_state.open.as_ref().filter(|_| self.menu.is_some())?;
        Some(open.host.key(&open.select))
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

    // A link button goes through Discord's leaving-Discord confirm; a Discord link opens here.
    // GAP: Discord's "Trust {domain} links from now on" checkbox; every outside link asks.
    fn open_link_button(&mut self, url: String, window: &mut Window, cx: &mut Context<Self>) {
        if crate::link_preview::parse(&url).is_some() {
            return self.open_link(&url, window, cx);
        }
        let body = format!("This link is taking you to the following website\n\n{url}");
        self.confirm("Leaving Discord", body, "Visit Site", move |_, _, cx| cx.open_url(&url), cx);
        if let Some(dialog) = self.dialog.as_mut() {
            dialog.danger = false;
        }
    }

    // Opens the select's menu under its field and as wide (at `fallback` before the field has painted).
    // Any other select's menu closes first, sending its picks.
    pub(crate) fn open_select(&mut self, host: SelectHost, select: Select, fallback: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        self.settle_select(cx);
        let key = host.key(&select);
        let anchor = self.component_state.anchors.borrow().get(&key).copied();
        let at = anchor.map_or(fallback, |b| point(b.left(), b.bottom() + px(SELECT_GAP)));
        self.component_state.open = Some(OpenSelect { host: host.clone(), select: select.clone() });
        let search = (select.kind != SelectKind::String).then(|| self.reset_select_search(&host, &select, window, cx));
        self.open_live_menu(at, move |this| this.select_entries(&host, &select, search.clone()), cx);
        if let Some(bounds) = anchor {
            self.set_menu_width(f32::from(bounds.size.width));
        }
    }

    // Once the select's menu has closed: a message select sends its picks if they changed since it last
    // sent, it has at least `min_values`, and a required one isn't empty (Discord's send on close).
    pub(crate) fn settle_select(&mut self, cx: &mut Context<Self>) {
        if self.menu.is_some() {
            return;
        }
        let Some(OpenSelect { host: SelectHost::Message(message), select }) = self.component_state.open.take() else { return };
        let key = (message.id.clone(), select.custom_id.clone());
        let defaults: Vec<String> = self.select_defaults(self.message_place(&message), &select).into_iter().map(|o| o.value).collect();
        let picks = self.component_state.picks.get(&key).cloned().unwrap_or_else(|| defaults.clone());
        let sent = self.component_state.sent.get(&key).cloned().unwrap_or(defaults);
        if !should_send(&picks, &sent, &select) {
            return cx.notify(); // lint-allow(global-notify): a message's select draws in the feed, the thread panel and the pins popover
        }
        self.component_state.sent.insert(key, picks.clone());
        self.use_component(&message, &Component::Select(select), picks, cx);
    }

    // The menu's search field, emptied, and the server it looks members up in.
    fn reset_select_search(&mut self, host: &SelectHost, select: &Select, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        let lists_members = matches!(select.kind, SelectKind::User | SelectKind::Mentionable);
        self.component_state.search_guild = self.host_place(host).guild.map(|g| g.id.clone()).filter(|_| lists_members);
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
        cx.notify(); // lint-allow(global-notify): the open select's list is drawn by the root's overlays, over every column
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

    // Options with their emoji and description, under the search when there is one. A multi-select
    // checks its picks and, at `max_values`, disables the rest; it has no Done: closing the menu sends.
    fn select_entries(&self, host: &SelectHost, select: &Select, search: Option<Entity<InputState>>) -> Vec<Entry> {
        let picks = self.host_picks(host, select);
        let multi = is_multi(select);
        let full = picks.len() >= select.max as usize;
        let place = self.host_place(host);
        let mut entries: Vec<Entry> = self
            .select_choices(place, select)
            .into_iter()
            .map(|option| {
                let picked = picks.contains(&option.value);
                let (h, s, value) = (host.clone(), select.clone(), option.value.clone());
                let mut item = menu::item(option.label, move |this, _, cx| this.pick_select_option(&h, &s, &value, cx));
                if let Some(hint) = option.description {
                    item = item.hint(hint);
                }
                if let Some(emoji) = option.emoji {
                    item = item.emoji(as_emoji(&emoji));
                }
                if multi {
                    item = item.checked(picked);
                }
                if multi && full && !picked {
                    item = item.disabled();
                }
                item.entry()
            })
            .collect();
        if entries.is_empty() {
            entries.push(menu::item(self.select_empty(place, select), |_, _, _| {}).disabled().entry());
        }
        if let Some(input) = search {
            entries.insert(0, Entry::Search(input));
        }
        entries
    }

    // A user, role, channel or mentionable select's choices that match the search, at most 25.
    fn select_choices(&self, place: Place, select: &Select) -> Vec<SelectOption> {
        if select.kind == SelectKind::String {
            return select.options.clone();
        }
        let query = self.component_state.query.to_lowercase();
        self.all_choices(place, select).into_iter().filter(|c| c.matches.contains(&query)).take(MAX_CHOICES).map(|c| c.option).collect()
    }

    fn all_choices(&self, place: Place, select: &Select) -> Vec<Choice> {
        match (place.dm, place.guild) {
            (Some(dm), _) => self.dm_choices(dm, select),
            (None, Some(guild)) => self.server_choices(guild, select),
            (None, None) => Vec::new(),
        }
    }

    // What an empty menu says instead of showing nothing.
    fn select_empty(&self, place: Place, select: &Select) -> &'static str {
        if !self.component_state.query.is_empty() {
            return "No matches";
        }
        if select.kind == SelectKind::Role && place.dm.is_some() {
            return "No roles in a DM";
        }
        "Nothing to pick here"
    }

    // In a DM: its recipients and you, or the DM itself as its one channel. It has no roles.
    fn dm_choices(&self, dm: &Channel, select: &Select) -> Vec<Choice> {
        match select.kind {
            SelectKind::User | SelectKind::Mentionable => sorted(dm.recipients.iter().chain(self.me.as_ref()).map(Choice::member).collect()),
            SelectKind::Channel if channel_listed(dm, &select.channel_types) => vec![Choice::named(&dm.name, &dm.id)],
            SelectKind::Channel | SelectKind::Role | SelectKind::String => Vec::new(),
        }
    }

    // GAP: members come from what this server has cached (the member list's, and what a search fetched).
    fn server_choices(&self, guild: &Guild, select: &Select) -> Vec<Choice> {
        let users = sorted(self.members.get(&guild.id).into_iter().flat_map(|m| m.values()).map(Choice::member).collect());
        // The @everyone role shares the server's id.
        let roles = guild.roles.iter().filter(|r| r.id != guild.id).map(|r| Choice::named(&r.name, &r.id));
        let channels = guild.channels.iter().filter(|c| channel_listed(c, &select.channel_types)).map(|c| Choice::named(&c.name, &c.id));
        match select.kind {
            SelectKind::User => users,
            SelectKind::Role => roles.collect(),
            SelectKind::Mentionable => users.into_iter().chain(roles).collect(),
            SelectKind::Channel => channels.collect(),
            SelectKind::String => Vec::new(),
        }
    }

    // What a select starts with: a string select's `default` options, else its `default_values` that
    // resolve here, as Discord's `er` does (a user it doesn't know, a role or channel outside this server,
    // or a channel of a type the select doesn't list, is dropped).
    // GAP: Waffle knows only the members it has cached, fewer than Discord's user store.
    fn select_defaults(&self, place: Place, select: &Select) -> Vec<SelectOption> {
        if select.kind == SelectKind::String {
            return select.options.iter().filter(|o| o.default).cloned().collect();
        }
        select.default_values.iter().filter_map(|d| self.default_choice(place, d, &select.channel_types)).collect()
    }

    fn default_choice(&self, place: Place, d: &DefaultValue, channel_types: &[u8]) -> Option<SelectOption> {
        match d.kind.as_str() {
            "user" => {
                let members = place.guild.and_then(|g| self.members.get(&g.id)).into_iter().flat_map(|m| m.values());
                let dm = place.dm.into_iter().flat_map(|c| &c.recipients);
                let user = members.chain(dm).chain(self.me.as_ref()).find(|u| u.id == d.id)?;
                Some(choice(&user.name, &user.id))
            }
            "role" => place.guild?.roles.iter().find(|r| r.id == d.id).map(|r| choice(&r.name, &r.id)),
            "channel" => place.guild?.channels.iter().find(|c| c.id == d.id && channel_listed(c, channel_types)).map(|c| choice(&c.name, &c.id)),
            _ => None,
        }
    }

    fn message_place(&self, message: &Message) -> Place<'_> {
        let guild_id = message.guild_id.as_ref().or(self.guild_id.as_ref());
        self.place(&message.channel_id, guild_id.map(String::as_str))
    }

    fn host_place(&self, host: &SelectHost) -> Place<'_> {
        match host {
            SelectHost::Message(m) => self.message_place(m),
            SelectHost::Modal(_) => self.app_modal_channel().map_or(Place { guild: None, dm: None }, |c| self.place(c, None)),
        }
    }

    // `channel_id`'s DM, else the server that has it (or `guild_id`).
    fn place(&self, channel_id: &str, guild_id: Option<&str>) -> Place<'_> {
        let dm = self.dms.iter().find(|c| c.id == channel_id);
        let guild = match guild_id {
            Some(id) => self.guilds.iter().find(|g| g.id == id),
            None => self.guilds.iter().find(|g| g.channels.iter().any(|c| c.id == channel_id)),
        };
        Place { guild: guild.filter(|_| dm.is_none()), dm }
    }

    // A select's picks as they stand: a message select's, else its defaults; a form field's value.
    fn host_picks(&self, host: &SelectHost, select: &Select) -> Vec<String> {
        match host {
            SelectHost::Message(m) => {
                let key = host.key(select);
                self.component_state.picks.get(&key).cloned().unwrap_or_else(|| self.select_defaults(self.message_place(m), select).into_iter().map(|o| o.value).collect())
            }
            SelectHost::Modal(ix) => self.app_modal_picks(*ix),
        }
    }

    // What a form's select field holds now: the open app form's defaults resolved here.
    pub(crate) fn modal_select_defaults(&self, channel_id: &str, select: &Select) -> Vec<String> {
        self.select_defaults(self.place(channel_id, None), select).into_iter().map(|o| o.value).collect()
    }

    // Demo scenes: `picks` checked and the select's menu open, once its field has painted.
    #[cfg(debug_assertions)]
    pub(crate) fn demo_open_select(&mut self, message: Message, custom_id: &str, picks: &[&str], window: &mut Window, cx: &mut Context<Self>) {
        let Some(select) = selects(&message.components).into_iter().find(|s| s.custom_id == custom_id).cloned() else { return };
        if !picks.is_empty() {
            self.component_state.picks.insert((message.id.clone(), custom_id.to_owned()), picks.iter().map(|p| (*p).to_owned()).collect());
        }
        self.open_select(SelectHost::Message(Box::new(message)), select, point(px(DEMO_SELECT_X), px(DEMO_SELECT_Y)), window, cx);
    }

    // Demo scenes: a premium button's SKU as if it had loaded.
    #[cfg(debug_assertions)]
    pub(crate) fn demo_sku(&mut self, sku_id: &str, sku: Sku) {
        self.component_state.skus.borrow_mut().insert(sku_id.to_owned(), sku);
    }

    // A multi pick toggles and waits for the menu to close; a single pick closes it, which sends.
    fn pick_select_option(&mut self, host: &SelectHost, select: &Select, value: &str, cx: &mut Context<Self>) {
        let multi = is_multi(select);
        if let SelectHost::Modal(ix) = host {
            self.pick_app_modal(*ix, Some(value.to_owned()), cx);
            if !multi {
                self.menu = None;
                self.component_state.open = None;
            }
            return;
        }
        let key = host.key(select);
        let picks = if multi { toggle_pick(&self.host_picks(host, select), value, select.max) } else { vec![value.to_owned()] };
        self.component_state.picks.insert(key, picks);
        if multi {
            return cx.notify(); // lint-allow(global-notify): the open select's list is drawn by the root's overlays, over every column
        }
        self.menu = None;
        self.settle_select(cx);
    }

    // Fetches a premium button's SKU once; the button redraws when it lands. Discord shows the dots
    // until it has the SKU, its listing and plans, so a failed fetch keeps them.
    fn load_sku(&mut self, sku_id: String, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        if self.component_state.skus.borrow().contains_key(&sku_id) {
            return;
        }
        self.component_state.skus.borrow_mut().insert(sku_id.clone(), Sku::Loading);
        cx.spawn(async move |this, cx| {
            let path = format!("/applications/storefront/interactions/premium-button/{sku_id}");
            let body = client.call(reqwest::Method::GET, path, None).await;
            this.update(cx, |this, cx| {
                if let Some(sku) = body.ok().and_then(|b| premium_sku(&b, &sku_id)) {
                    this.component_state.skus.borrow_mut().insert(sku_id, sku);
                }
                cx.notify(); // lint-allow(global-notify): a message's price tag draws in the feed, the thread panel and the pins popover
            })
            .ok();
        })
        .detach();
    }

    // A select field in the app form: the same field and menu as a message's, its picks the form's.
    pub(crate) fn modal_select(&self, ix: usize, select: &Select, picks: &[String], cx: &mut Context<Self>) -> AnyElement {
        let host = SelectHost::Modal(ix);
        let key = host.key(select);
        let open = self.open_select_key().as_ref() == Some(&key);
        let place = self.host_place(&host);
        let all = if select.kind == SelectKind::String { Vec::new() } else { self.all_choices(place, select) };
        let known: Vec<SelectOption> = select.options.iter().cloned().chain(all.into_iter().map(|c| c.option)).collect();
        let picked = labels(picks, &known, |_| None);
        let anchors = self.component_state.anchors.clone();
        let s = select.clone();
        let field = select_trigger(SharedString::from(format!("app-modal-select-{ix}")), select, picked, open, cx)
            .disabled(select.disabled)
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| this.open_select(SelectHost::Modal(ix), s.clone(), event.position(), window, cx)));
        anchored_field(anchors, key, field.into_any_element())
    }
}

// Discord's send-on-close rule for a message select.
// Not verified: what `required` adds in a message; Discord's validator wasn't read, so an empty pick of
// a required select is held back like one under `min_values`.
fn should_send(picks: &[String], sent: &[String], select: &Select) -> bool {
    let changed = picks.len() != sent.len() || picks.iter().any(|p| !sent.contains(p));
    changed && picks.len() >= select.min as usize && !(select.required && picks.is_empty())
}

// An empty `channel_types` lists every channel but categories.
// Not verified: which types Discord lists when the select names none.
fn channel_listed(channel: &Channel, types: &[u8]) -> bool {
    if types.is_empty() {
        return channel.kind != ChannelKind::Category;
    }
    types.contains(&channel.raw_type)
}

// The room a component has: its width (a gallery fills it) and whether it sits in a container, where
// Discord draws text at its reduced size.
#[derive(Clone, Copy)]
struct Room {
    width: f32,
    reduced: bool,
}

// Under the message body: rows of buttons and selects, or (with COMPONENTS_V2) the whole message.
pub(crate) fn message_components(message: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> Option<AnyElement> {
    if message.components.is_empty() {
        return None;
    }
    let width = MAX_WIDTH.min(ctx.column());
    let room = Room { width, reduced: false };
    let children: Vec<AnyElement> = message.components.iter().enumerate().map(|(i, c)| component(message, c, i.to_string(), room, ctx, cx)).collect();
    Some(ui::col().mt_2().w_full().max_w(px(width)).gap_2().children(children).into_any_element())
}

// `path` keeps element ids apart: each component's index, from the top level down.
fn component(message: &Message, c: &Component, path: String, room: Room, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    match c {
        Component::Row(children) => action_row(message, children, &path, room, ctx, cx),
        Component::Button(b) => button(message, b, &path, ctx, cx),
        Component::Select(s) => select_field(message, s, &path, ctx, cx),
        Component::Section { children, accessory } => section(message, children, accessory.as_deref(), &path, room, ctx, cx),
        Component::Text(text) => {
            let size = if room.reduced { REDUCED_TEXT } else { chat::BODY_TEXT };
            chat::rich_text(&format!("component-{}-{path}", message.id), text, size, ctx, cx)
        }
        Component::Thumbnail { url, spoiler, width, height, .. } => thumbnail(message, url, *spoiler, (*width, *height), &path, ctx, cx),
        Component::Gallery(items) => gallery(message, items, &path, room.width, ctx, cx),
        Component::File { url, name, size, spoiler } => file(message, url, name.as_deref(), *size, *spoiler, &path, ctx, cx),
        Component::Separator { divider, large } => separator(*divider, *large, cx),
        Component::Container { accent, spoiler, children } => container(message, *accent, children, &path, room.width, ctx, cx)
            .when(*spoiler, |c| c.children(chat::veil(format!("spoiler-{}-{path}", message.id), "Spoiler", ui::all(RADIUS_LG), ctx)))
            .into_any_element(),
    }
}

// Up to five buttons, wrapping, or one select; a failed press says so under the row.
fn action_row(message: &Message, children: &[Component], path: &str, room: Room, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    let controls: Vec<AnyElement> = children.iter().enumerate().map(|(i, c)| component(message, c, format!("{path}-{i}"), room, ctx, cx)).collect();
    let failed = failed_line(message, children, ctx, cx);
    ui::col()
        .child(ui::row().items_stretch().flex_wrap().gap_2().children(controls))
        .children(failed)
        .into_any_element()
}

// Discord's button: a link opens through the leaving confirm with its window icon; a premium one shows
// the shop mark and its SKU's name and price. In flight, the label hides under the dots at the same width.
fn button(message: &Message, b: &Button, path: &str, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    let call = b.custom_id.as_ref().and_then(|id| ctx.components.calls.get(&(message.id.clone(), id.clone())));
    let busy = call.is_some_and(|c| c.failure.is_none());
    // Another control on this message is in flight.
    let blocked = !busy && ctx.components.busy.contains(&message.id);
    let premium = b.sku_id.clone().filter(|_| b.style == ButtonStyle::Premium);
    let sku = premium.as_ref().map(|id| sku_state(id, ctx, cx));
    let label = match &sku {
        Some(Sku::Ready(label)) => label.clone(),
        Some(Sku::Unavailable) => PRODUCT_UNAVAILABLE.to_owned(),
        Some(Sku::Loading) => String::new(),
        None => b.label.clone().unwrap_or_default(),
    };
    let link = b.style == ButtonStyle::Link;
    let (this, m, press) = (ctx.this(), message.clone(), b.clone());
    // GAP: Waffle can't buy, so a premium button never opens Discord's SKU modal; it stays disabled.
    // GAP: no shop glyph in Waffle's icon set; the tag stands in for Discord's ShopIcon.
    ui::button(SharedString::from(format!("component-{}-{path}", message.id)), tone(b.style), cx)
        .small()
        .disabled(b.disabled || blocked || premium.is_some() || message.state != SendState::Sent)
        .when(premium.is_some(), |c| c.child(Icon::new(Glyph::Tag).size(ui::ICON_SM)))
        .children(b.emoji.as_deref().map(|e| menu::emoji_face(&as_emoji(e), 18.)))
        .when(!label.is_empty(), |c| c.child(label.clone()))
        .when(link, |c| c.child(Icon::new(Glyph::Open).size(ui::ICON_SM)))
        .busy(busy || sku == Some(Sku::Loading))
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            if let Some(url) = press.url.clone().filter(|_| press.style == ButtonStyle::Link) {
                this.update(cx, |this, cx| this.open_link_button(url, window, cx)).ok();
                return;
            }
            this.update(cx, |this, cx| this.use_component(&m, &Component::Button(press.clone()), Vec::new(), cx)).ok();
        })
        .into_any_element()
}

// Every select in a component tree, depth first.
fn selects(components: &[Component]) -> Vec<&Select> {
    components
        .iter()
        .flat_map(|c| match c {
            Component::Select(s) => vec![s],
            Component::Row(children) | Component::Container { children, .. } | Component::Section { children, .. } => selects(children),
            _ => Vec::new(),
        })
        .collect()
}

fn custom_id(c: &Component) -> Option<String> {
    match c {
        Component::Button(b) => b.custom_id.clone(),
        Component::Select(s) => Some(s.custom_id.clone()),
        _ => None,
    }
}

// An app's button colours in Waffle's tones: Discord draws Premium in the brand colour like Primary.
fn tone(style: ButtonStyle) -> ui::Tone {
    match style {
        ButtonStyle::Primary | ButtonStyle::Premium => ui::Tone::Primary,
        ButtonStyle::Success => ui::Tone::Positive,
        ButtonStyle::Danger => ui::Tone::Danger,
        ButtonStyle::Secondary | ButtonStyle::Link => ui::Tone::Secondary,
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
    use serde_json::json;

    use super::*;

    fn select(min: u32, max: u32, required: bool) -> Select {
        Select { kind: SelectKind::String, custom_id: "s".into(), placeholder: None, options: Vec::new(), min, max, disabled: false, required, default_values: Vec::new(), channel_types: Vec::new() }
    }

    fn picks(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_owned()).collect()
    }

    #[test]
    fn button_styles_take_waffle_tones() {
        assert_eq!(tone(ButtonStyle::Primary), ui::Tone::Primary);
        assert_eq!(tone(ButtonStyle::Premium), ui::Tone::Primary, "Discord draws premium in the brand colour");
        assert_eq!(tone(ButtonStyle::Success), ui::Tone::Positive);
        assert_eq!(tone(ButtonStyle::Danger), ui::Tone::Danger);
        for neutral in [ButtonStyle::Secondary, ButtonStyle::Link] {
            assert_eq!(tone(neutral), ui::Tone::Secondary);
        }
    }

    #[test]
    fn multi_select_picks_toggle_up_to_max() {
        let p = toggle_pick(&[], "games", 2);
        let p = toggle_pick(&p, "music", 2);
        assert_eq!(p, ["games", "music"]);
        assert_eq!(toggle_pick(&p, "movies", 2), p, "a full select ignores another pick");
        assert_eq!(toggle_pick(&p, "games", 2), ["music"]);
    }

    #[test]
    fn a_closed_menu_sends_only_changed_picks_that_meet_min() {
        let s = select(2, 3, false);
        assert!(should_send(&picks(&["a", "b"]), &[], &s));
        assert!(!should_send(&picks(&["a"]), &[], &s), "under min_values: held back");
        assert!(!should_send(&picks(&["b", "a"]), &picks(&["a", "b"]), &s), "same picks in another order");
        assert!(should_send(&picks(&["a", "c"]), &picks(&["a", "b"]), &s));
        assert!(should_send(&[], &picks(&["a"]), &select(0, 3, false)), "clearing an optional select sends []");
        assert!(!should_send(&[], &picks(&["a"]), &select(0, 3, true)));
    }

    #[test]
    fn channel_selects_list_their_types() {
        let channel = |kind, raw_type| Channel {
            id: String::new(),
            name: String::new(),
            kind,
            parent_id: None,
            position: 0,
            topic: None,
            last_message_id: None,
            private: false,
            recipient: None,
            recipients: Vec::new(),
            icon: None,
            thread: None,
            raw_type,
            overwrites: Vec::new(),
            props: Default::default(),
        };
        assert!(channel_listed(&channel(ChannelKind::Text, 0), &[]));
        assert!(!channel_listed(&channel(ChannelKind::Category, 4), &[]));
        assert!(channel_listed(&channel(ChannelKind::Category, 4), &[4]));
        assert!(!channel_listed(&channel(ChannelKind::Text, 5), &[0]), "announcement isn't text");
        assert!(channel_listed(&channel(ChannelKind::Voice, 2), &[0, 2]));
    }

    #[test]
    fn thumbnails_keep_their_shape_between_85_and_170() {
        assert_eq!(thumbnail_size(Some(400), Some(400)), (170., 170.));
        assert_eq!(thumbnail_size(Some(340), Some(170)), (170., 85.));
        assert_eq!(thumbnail_size(Some(1000), Some(100)), (170., 85.), "the short side stops at 85");
        assert_eq!(thumbnail_size(Some(100), Some(120)), (100., 120.), "small media isn't grown");
        assert_eq!(thumbnail_size(Some(40), Some(40)), (85., 85.));
        assert_eq!(thumbnail_size(None, Some(10)), (85., 85.));
    }

    #[test]
    fn ephemerality_reasons_read_as_discord_writes_them() {
        assert_eq!(ephemerality_reason(4), "You are sending messages too quickly");
        assert_eq!(ephemerality_reason(18), "You cannot use external apps in this channel");
        assert_eq!(ephemerality_reason(0), "An unknown error occurred");
        assert_eq!(ephemerality_reason(99), "An unknown error occurred");
    }

    #[test]
    fn premium_buttons_read_the_storefront() {
        let one_off = json!({ "skus": [{ "id": "7", "name": "Map pack", "type": 3, "price": { "amount": 499, "currency": "usd" } }] });
        assert_eq!(premium_sku(&one_off, "7"), Some(Sku::Ready("Map pack - $4.99".into())));
        let sub = json!({ "skus": [{ "id": "8", "name": "Pro", "type": 5 }], "subscription_plans": [{ "price": 300, "currency": "eur" }] });
        assert_eq!(premium_sku(&sub, "8"), Some(Sku::Ready("Pro - €3.00".into())));
        let no_plan = json!({ "skus": [{ "id": "8", "name": "Pro", "type": 5 }], "subscription_plans": [] });
        assert_eq!(premium_sku(&no_plan, "8"), Some(Sku::Unavailable));
        let gone = json!({ "skus": [{ "id": "7", "name": "Map pack", "available": false }] });
        assert_eq!(premium_sku(&gone, "7"), Some(Sku::Unavailable));
        assert_eq!(premium_sku(&one_off, "9"), None, "not loaded: Discord keeps the dots");
        assert_eq!(format_price(1500, "jpy"), "¥1500");
        assert_eq!(format_price(250, "cad"), "CAD 2.50");
    }

    #[test]
    fn select_labels_resolve_values() {
        let known = vec![choice("Games", "games")];
        assert_eq!(labels(&picks(&["games", "9"]), &known, |v| (v == "9").then(|| "Mira".to_owned())).as_deref(), Some("Games, Mira"));
        assert_eq!(labels(&picks(&["x"]), &known, |_| None).as_deref(), Some("x"));
        assert_eq!(labels(&[], &known, |_| None), None);
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
        let message = Message { files: vec![File { name: "log.txt".into(), url: "https://cdn.discordapp.com/attachments/1/2/log.txt?ex=1".into(), size: 3, ..Default::default() }], ..Default::default() };
        assert_eq!(resolve(&message, "attachment://log.txt"), message.files[0].url);
        assert_eq!(resolve(&message, "https://x/y.png"), "https://x/y.png");
    }
}
