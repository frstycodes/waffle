// Discord's Favorites pseudo-server (docs/discord-navigation-re.md §4): channels and DMs gathered from
// anywhere, with your own nicknames and categories, synced in settings-proto field 15.
use std::collections::{BTreeMap, HashSet};

use base64::Engine;
use gpui_kit::{AnyElement, Context, FontWeight, MouseButton, MouseDownEvent, Pixels, Point, Radians, SharedString, Window, component::Icon, div, point, prelude::*, px};
use reqwest::Method;
use serde_json::json;

use crate::{
    Waffle,
    discord::{Channel, ChannelKind, Guild},
    drag::{Look, RowDrag, hole, pick_up},
    icons::Glyph,
    menu::{Entry, HeadLead, header, item, submenu},
    palette,
    sidebar::{Nav, ROW_GAP, ROW_HEIGHT, RowState, category_head, channel_glyph, channel_row},
    sortable::{Board, Finished, Place},
    ui,
};

// Discord's id for the pseudo-guild; also the key Waffle remembers its last channel under.
pub(crate) const GUILD_ID: &str = "373";
pub(crate) const RAIL_KEY: &str = "favorites";
const FREE_LIMIT: usize = 3;
const NITRO_LIMIT: usize = 200;
// Every entry, categories included.
const MAX_ENTRIES: usize = 200;
const NITRO: u8 = 2;
// FavoriteChannel.type
const CHANNEL: u64 = 1;
const CATEGORY: u64 = 2;
const THREAD_TYPES: [u64; 3] = [10, 11, 12];
const VOICE_TYPES: [u64; 2] = [2, 13];
// Discord sorts voice after text inside a group by adding this to the position.
const VOICE_AFTER: u64 = 10_000;
const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;
const SETTINGS_PATH: &str = "/users/@me/settings-proto/1";
// The sortable list of categories; each group of channels is `group_list(parent)`.
const CATEGORY_LIST: &str = "fav-categories";
const CATEGORY_HEAD_HEIGHT: f32 = 28.;

// A favourite channel carried in the Favorites list; a category is a `FavoriteCategoryDrag`, so each only
// aims at its own kind's rows.
#[derive(Clone)]
pub struct FavoriteDrag;

#[derive(Clone)]
pub struct FavoriteCategoryDrag;

// settings-proto `Favorites`. Fields Waffle doesn't use (thread settings) ride along in `unknown`, so a save
// sends them back unchanged.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Favorites {
    pub channels: BTreeMap<u64, FavoriteChannel>,
    pub muted: bool,
    // Unset: shown once anything is favourited.
    pub guild_visible: Option<bool>,
    unknown: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FavoriteChannel {
    // A channel's local nickname ("" for none), or a category's name.
    pub nickname: String,
    pub kind: u64,
    pub position: u32,
    // 0 is the root; otherwise a CATEGORY entry.
    pub parent_id: u64,
    // The channel's Discord type, cached for sorting and for pruning threads.
    pub channel_type: Option<u64>,
    pub collapsed: bool,
    unknown: Vec<u8>,
}

// Where a dragged favourite goes: beside another entry of its own kind, or first in a group (0, the root,
// or a category).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Slot {
    Beside { target: u64, after: bool },
    First { parent: u64 },
}

// Whether a favourite's channel can still be opened, as the save-time prune asks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reach {
    Viewable,
    Hidden,
    Gone,
}

impl FavoriteChannel {
    pub fn is_category(&self) -> bool {
        self.kind == CATEGORY
    }

    fn is_thread(&self) -> bool {
        self.channel_type.is_some_and(|t| THREAD_TYPES.contains(&t))
    }

    fn sort_key(&self) -> u64 {
        let voice = self.channel_type.is_some_and(|t| VOICE_TYPES.contains(&t));
        u64::from(self.position) + if voice { VOICE_AFTER } else { 0 }
    }

    fn decode(buf: &[u8]) -> Option<Self> {
        let mut entry = Self::default();
        for field in wire::fields(buf)? {
            match (field.number, field.value) {
                (1, wire::Value::Bytes(b)) => entry.nickname = String::from_utf8(b.to_vec()).ok()?,
                (2, wire::Value::Varint(v)) => entry.kind = v,
                (3, wire::Value::Varint(v)) => entry.position = v as u32,
                (4, wire::Value::Fixed64(v)) => entry.parent_id = v,
                (5, wire::Value::Bytes(b)) => entry.channel_type = Some(wire::wrapped(b)?),
                (6, wire::Value::Varint(v)) => entry.collapsed = v != 0,
                _ => entry.unknown.extend_from_slice(field.raw),
            }
        }
        Some(entry)
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        if !self.nickname.is_empty() {
            wire::put_bytes(&mut out, 1, self.nickname.as_bytes());
        }
        if self.kind != 0 {
            wire::put_uint(&mut out, 2, self.kind);
        }
        if self.position != 0 {
            wire::put_uint(&mut out, 3, u64::from(self.position));
        }
        if self.parent_id != 0 {
            wire::put_fixed64(&mut out, 4, self.parent_id);
        }
        if let Some(kind) = self.channel_type {
            wire::put_bytes(&mut out, 5, &wire::wrap(kind));
        }
        if self.collapsed {
            wire::put_uint(&mut out, 6, 1);
        }
        out.extend_from_slice(&self.unknown);
        out
    }
}

impl Favorites {
    // Favorites from a whole PreloadedUserSettings: None when malformed, Some(None) when field 15 is absent.
    pub fn from_settings(settings: &[u8]) -> Option<Option<Self>> {
        let mut found = None;
        for field in wire::fields(settings)? {
            if let (15, wire::Value::Bytes(b)) = (field.number, field.value) {
                found = Some(Self::decode(b)?);
            }
        }
        Some(found)
    }

    // A PreloadedUserSettings holding only field 15: Discord merges top-level fields it is sent.
    pub fn to_settings(&self) -> Vec<u8> {
        let mut out = Vec::new();
        wire::put_bytes(&mut out, 15, &self.encode());
        out
    }

    fn decode(buf: &[u8]) -> Option<Self> {
        let mut favorites = Self::default();
        for field in wire::fields(buf)? {
            match (field.number, field.value) {
                (1, wire::Value::Bytes(b)) => {
                    let (id, entry) = decode_map_entry(b)?;
                    favorites.channels.insert(id, entry);
                }
                (2, wire::Value::Varint(v)) => favorites.muted = v != 0,
                (3, wire::Value::Bytes(b)) => favorites.guild_visible = Some(wire::wrapped(b)? != 0),
                _ => favorites.unknown.extend_from_slice(field.raw),
            }
        }
        Some(favorites)
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for (id, entry) in &self.channels {
            let mut pair = Vec::new();
            wire::put_fixed64(&mut pair, 1, *id);
            wire::put_bytes(&mut pair, 2, &entry.encode());
            wire::put_bytes(&mut out, 1, &pair);
        }
        if self.muted {
            wire::put_uint(&mut out, 2, 1);
        }
        if let Some(visible) = self.guild_visible {
            wire::put_bytes(&mut out, 3, &wire::wrap(u64::from(visible)));
        }
        out.extend_from_slice(&self.unknown);
        out
    }

    pub fn shown(&self) -> bool {
        self.guild_visible.unwrap_or(!self.channels.is_empty())
    }

    // Uncategorised channels first, then each category by position; inside each, position order with voice last.
    pub fn sections(&self) -> Vec<(Option<u64>, Vec<u64>)> {
        let mut out = vec![(None, self.members(0))];
        out.extend(self.ordered_categories().into_iter().map(|c| (Some(c), self.members(c))));
        out
    }

    fn ordered_categories(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self.channels.iter().filter(|(_, e)| e.is_category()).map(|(id, _)| *id).collect();
        ids.sort_by_key(|id| (self.channels[id].position, *id));
        ids
    }

    // The channels shown under `parent` (0, the root, also takes those whose parent isn't a category), in order.
    fn members(&self, parent: u64) -> Vec<u64> {
        let mut ids: Vec<u64> = self.channels.iter().filter(|(_, e)| !e.is_category() && self.group_of(e) == parent).map(|(id, _)| *id).collect();
        ids.sort_by_key(|id| (self.channels[id].sort_key(), *id));
        ids
    }

    fn group_of(&self, entry: &FavoriteChannel) -> u64 {
        match self.channels.get(&entry.parent_id) {
            Some(parent) if parent.is_category() => entry.parent_id,
            _ => 0,
        }
    }

    // A drag let go of: `moved` goes into `slot`, and every group it left or joined is renumbered from 0 in
    // its new order. Categories only reorder among themselves (none goes inside another); a channel lands in
    // the group of the channel it's dropped beside. False when refused or nothing changed.
    pub fn arrange(&mut self, moved: u64, slot: Slot) -> bool {
        let Some(entry) = self.channels.get(&moved) else { return false };
        let is_category = entry.is_category();
        let from = self.group_of(entry);
        let (to, anchor) = match slot {
            Slot::First { parent } => (parent, None),
            Slot::Beside { target, after } => match self.channels.get(&target) {
                Some(t) if target != moved && t.is_category() == is_category => (self.group_of(t), Some((target, after))),
                _ => return false,
            },
        };
        let valid_parent = to == 0 || (!is_category && self.channels.get(&to).is_some_and(FavoriteChannel::is_category));
        if !valid_parent {
            return false;
        }
        let before = self.channels.clone();
        let mut order = if is_category { self.ordered_categories() } else { self.members(to) };
        order.retain(|&id| id != moved);
        let at = anchor.and_then(|(target, after)| Some(order.iter().position(|&id| id == target)? + usize::from(after))).unwrap_or(0);
        order.insert(at, moved);
        if !is_category {
            self.channels.get_mut(&moved).expect("checked above").parent_id = to;
        }
        self.renumber(&order);
        if !is_category && from != to {
            let left = self.members(from);
            self.renumber(&left);
        }
        self.channels != before
    }

    fn renumber(&mut self, order: &[u64]) {
        for (position, id) in order.iter().enumerate() {
            if let Some(entry) = self.channels.get_mut(id) {
                entry.position = position as u32;
            }
        }
    }

    pub fn categories(&self) -> Vec<(u64, String)> {
        self.sections().into_iter().filter_map(|(c, _)| c).map(|c| (c, self.channels[&c].nickname.clone())).collect()
    }

    // The count the over-limit alert names, when one more channel would break a limit.
    pub fn over_limit(&self, premium_type: u8) -> Option<usize> {
        let limit = if premium_type == NITRO { NITRO_LIMIT } else { FREE_LIMIT };
        let channels = self.channels.values().filter(|e| !e.is_category()).count();
        if channels >= limit {
            return Some(limit);
        }
        (self.channels.len() >= MAX_ENTRIES).then_some(MAX_ENTRIES)
    }

    fn next_position(&self) -> u32 {
        self.channels.values().map(|e| e.position + 1).max().unwrap_or(0)
    }

    // Adding shows Favorites again if it was hidden.
    pub fn add_channel(&mut self, id: u64, channel_type: u64, parent_id: u64) {
        let hidden = !self.shown();
        let position = self.next_position();
        self.channels.insert(id, FavoriteChannel { kind: CHANNEL, position, parent_id, channel_type: Some(channel_type), ..Default::default() });
        if hidden {
            self.guild_visible = Some(true);
        }
    }

    // Category names are unique ignoring case: an existing one is reused.
    pub fn add_category(&mut self, name: &str, id: u64) -> u64 {
        if let Some((&existing, _)) = self.channels.iter().find(|(_, e)| e.is_category() && e.nickname.eq_ignore_ascii_case(name)) {
            return existing;
        }
        let position = self.next_position();
        self.channels.insert(id, FavoriteChannel { nickname: name.to_owned(), kind: CATEGORY, position, ..Default::default() });
        id
    }

    // A removed category's channels move to the root.
    pub fn remove(&mut self, id: u64) {
        self.channels.remove(&id);
        for entry in self.channels.values_mut().filter(|e| e.parent_id == id) {
            entry.parent_id = 0;
        }
    }

    // Discord's save-time prune: channels that are gone (threads excepted) or no longer viewable, and parents
    // that aren't categories.
    pub fn prune(&mut self, reach: impl Fn(u64) -> Reach) {
        self.channels.retain(|&id, e| {
            e.is_category()
                || match reach(id) {
                    Reach::Viewable => true,
                    Reach::Hidden => false,
                    Reach::Gone => e.is_thread(),
                }
        });
        let categories: HashSet<u64> = self.channels.iter().filter(|(_, e)| e.is_category()).map(|(id, _)| *id).collect();
        for entry in self.channels.values_mut().filter(|e| e.parent_id != 0 && !categories.contains(&e.parent_id)) {
            entry.parent_id = 0;
        }
    }
}

fn decode_map_entry(buf: &[u8]) -> Option<(u64, FavoriteChannel)> {
    let (mut id, mut entry) = (None, FavoriteChannel::default());
    for field in wire::fields(buf)? {
        match (field.number, field.value) {
            (1, wire::Value::Fixed64(v)) => id = Some(v),
            (2, wire::Value::Bytes(b)) => entry = FavoriteChannel::decode(b)?,
            _ => {}
        }
    }
    Some((id?, entry))
}

// Not verified: how Discord mints a new favourites category's id; a snowflake for now keeps it unique.
fn new_category_id() -> u64 {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    now.saturating_sub(DISCORD_EPOCH_MS) << 22
}

impl Waffle {
    pub(crate) fn in_favorites(&self) -> bool {
        let Some(opened) = &self.favorites_open else { return false };
        self.guild_id.as_deref() == Some(GUILD_ID) || self.channel.as_ref().is_some_and(|c| &c.id == opened)
    }

    pub(crate) fn is_favorite(&self, channel_id: &str) -> bool {
        channel_id.parse().is_ok_and(|id| self.favorites.channels.get(&id).is_some_and(|e| !e.is_category()))
    }

    // A favourite's channel and its server (None for a DM).
    fn favorite_channel(&self, id: u64) -> Option<(Channel, Option<&Guild>)> {
        let id = id.to_string();
        if let Some((guild, channel)) = self.guilds.iter().find_map(|g| Some((g, g.channel(&id)?))) {
            return Some((channel.clone(), Some(guild)));
        }
        self.dms.iter().find(|d| d.id == id).map(|d| (d.clone(), None))
    }

    fn reach(&self, id: u64) -> Reach {
        match self.favorite_channel(id) {
            None => Reach::Gone,
            Some((_, None)) => Reach::Viewable,
            Some((channel, Some(guild))) if channel.kind == ChannelKind::Thread || guild.can_view(self.my_id(), &channel) => Reach::Viewable,
            Some(_) => Reach::Hidden,
        }
    }

    // The rail tile's badge: mentions summed across favourites; muted Favorites drops the unread pill.
    pub(crate) fn favorites_badge(&self) -> (bool, u32) {
        let channels = self.favorites.channels.iter().filter(|(_, e)| !e.is_category()).map(|(id, _)| id.to_string());
        let (unread, mentions) = channels.fold((false, 0), |(unread, mentions), id| (unread || self.unread(&id), mentions + self.mention_count(&id)));
        (unread && !self.favorites.muted, mentions)
    }

    pub(crate) fn open_favorites(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let remembered = self.last_channel.get(&Some(GUILD_ID.to_owned())).and_then(|id| id.parse::<u64>().ok());
        let first = self.favorites.sections().into_iter().flat_map(|(_, ids)| ids).find(|&id| self.favorite_channel(id).is_some_and(|(c, _)| c.kind != ChannelKind::Voice));
        let target = remembered.filter(|&id| self.is_favorite(&id.to_string())).or(first).and_then(|id| self.favorite_channel(id)).map(|(c, g)| (c, g.map(|g| g.id.clone())));
        if let Some((channel, guild_id)) = target {
            return self.open_favorite(channel, guild_id, window, cx);
        }
        self.favorites_open = Some(String::new());
        self.guild_id = Some(GUILD_ID.to_owned());
        self.channel = None;
        cx.notify();
    }

    // The real channel opens in its own server, for its messages and permissions, while the list stays on Favorites.
    fn open_favorite(&mut self, channel: Channel, guild_id: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.guild_id = guild_id;
        self.favorites_open = Some(channel.id.clone());
        self.last_channel.insert(Some(GUILD_ID.to_owned()), channel.id.clone());
        self.open_channel(channel, window, cx);
    }

    // LOAD AND SAVE

    // Not verified: whether READY carries `user_settings_proto` with `capabilities: 0`; Waffle fetches it instead.
    pub(crate) fn load_favorites(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            let Ok(body) = client.call(Method::GET, SETTINGS_PATH.to_owned(), None).await else { return };
            let proto = body["settings"].as_str().unwrap_or_default().to_owned();
            this.update(cx, |this, cx| this.apply_settings_proto(&proto, false, cx)).ok();
        })
        .detach();
    }

    pub(crate) fn apply_settings_proto(&mut self, proto: &str, partial: bool, cx: &mut Context<Self>) {
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(proto) else { return };
        match Favorites::from_settings(&bytes) {
            Some(Some(favorites)) => self.favorites = favorites,
            Some(None) if !partial => self.favorites = Favorites::default(),
            _ => return,
        }
        cx.notify();
    }

    // Not verified: whether the server replaces the favorite_channels map wholesale (removals rely on it);
    // Discord's client sends the whole Favorites message the same way.
    fn save_favorites(&mut self, cx: &mut Context<Self>) {
        let mut favorites = std::mem::take(&mut self.favorites);
        favorites.prune(|id| self.reach(id));
        self.favorites = favorites;
        let settings = base64::engine::general_purpose::STANDARD.encode(self.favorites.to_settings());
        cx.notify();
        self.rest(Method::PATCH, SETTINGS_PATH.to_owned(), Some(json!({ "settings": settings })), |_, _| {}, cx);
    }

    fn add_favorite(&mut self, id: u64, channel_type: u64, parent_id: u64, cx: &mut Context<Self>) {
        if let Some(limit) = self.favorites.over_limit(self.my_account.premium_type) {
            let upsell = if limit == FREE_LIMIT { format!(" Get Nitro to favorite up to {NITRO_LIMIT}.") } else { String::new() };
            return self.alert("Oh no!", format!("You cannot have more than {limit} favorite channels.{upsell}"), cx);
        }
        self.favorites.add_channel(id, channel_type, parent_id);
        self.show_toast("Added to Favorites!", cx);
        self.save_favorites(cx);
    }

    fn remove_favorite(&mut self, id: u64, cx: &mut Context<Self>) {
        self.favorites.remove(id);
        self.save_favorites(cx);
    }

    fn edit_favorite(&mut self, id: u64, change: impl FnOnce(&mut FavoriteChannel), cx: &mut Context<Self>) {
        let Some(entry) = self.favorites.channels.get_mut(&id) else { return };
        change(entry);
        self.save_favorites(cx);
    }

    fn hide_favorites(&mut self, cx: &mut Context<Self>) {
        self.favorites.guild_visible = Some(false);
        if self.in_favorites() {
            self.favorites_open = None;
            self.open_home(cx);
        }
        self.save_favorites(cx);
    }

    // A favourite let go of over the list: beside a row, or first in a category from its header.
    pub(crate) fn finish_favorite_sort(&mut self, done: Finished, cx: &mut Context<Self>) {
        let (Some(hint), Ok(moved)) = (done.hint, done.dragged.parse::<u64>()) else { return };
        let slot = match hint.target.map(|t| t.parse::<u64>()) {
            Some(Ok(target)) => Slot::Beside { target, after: hint.place == Place::After },
            Some(Err(_)) => return,
            None => match hint.list.strip_prefix("fav-group-").and_then(|p| p.parse().ok()) {
                Some(parent) => Slot::First { parent },
                None => return,
            },
        };
        if self.favorites.arrange(moved, slot) {
            self.save_favorites(cx);
        }
    }

    // Beside the carried channel: "Move to Work" while it's aimed beside a channel in another category.
    fn favorite_move_tag(&self) -> Option<SharedString> {
        let moved = self.favorites.channels.get(&self.favorite_board.dragged()?.parse().ok()?)?;
        let over = self.favorites.channels.get(&self.favorite_board.hint()?.target.as_deref()?.parse().ok()?)?;
        let to = self.favorites.group_of(over);
        if moved.is_category() || to == 0 || to == self.favorites.group_of(moved) {
            return None;
        }
        Some(format!("Move to {}", self.favorites.channels[&to].nickname).into())
    }

    fn carrying_favorite_channel(&self) -> bool {
        self.favorite_board.dragged().and_then(|d| self.favorites.channels.get(&d.parse().ok()?)).is_some_and(|e| !e.is_category())
    }

    // MENUS

    /// "Add to favorites" in a channel, thread or DM menu: a submenu of your categories once you have some.
    pub(crate) fn favorite_add_entry(&self, channel: &Channel) -> Option<Entry> {
        let is_self = channel.recipient.as_ref().is_some_and(|u| u.id == self.my_id());
        if channel.kind == ChannelKind::Category || is_self || self.is_favorite(&channel.id) {
            return None;
        }
        let id: u64 = channel.id.parse().ok()?;
        let kind = u64::from(channel.raw_type);
        let add = move |parent: u64| move |this: &mut Waffle, _: &mut Window, cx: &mut Context<Waffle>| this.add_favorite(id, kind, parent, cx);
        let categories = self.favorites.categories();
        if categories.is_empty() {
            return Some(item("Add to favorites", add(0)).icon(Glyph::Star).entry());
        }
        let mut rows = vec![item("Favorites", add(0)).icon(Glyph::Star).entry(), Entry::Separator];
        rows.extend(categories.into_iter().map(|(category, name)| item(name, add(category)).icon(Glyph::Folder).entry()));
        Some(submenu("Add to favorites", rows).icon(Glyph::Star).entry())
    }

    pub(crate) fn favorite_remove_entry(&self, channel: &Channel) -> Option<Entry> {
        if !self.is_favorite(&channel.id) {
            return None;
        }
        let id: u64 = channel.id.parse().ok()?;
        Some(item("Remove from favorites", move |this, _, cx| this.remove_favorite(id, cx)).danger().icon(Glyph::Star).entry())
    }

    fn open_favorite_menu(&mut self, id: u64, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.open_live_menu(at, move |this| this.favorite_menu(id), cx);
    }

    // A channel's menu inside Favorites.
    fn favorite_menu(&self, id: u64) -> Vec<Entry> {
        let (Some((channel, guild)), Some(entry)) = (self.favorite_channel(id), self.favorites.channels.get(&id)) else { return Vec::new() };
        let guild_id = guild.map(|g| g.id.clone());
        let glyph = match channel.kind {
            ChannelKind::Voice => Glyph::VolumeHigh,
            ChannelKind::Dm => Glyph::ChatRound,
            _ => Glyph::Hashtag,
        };
        let mut out = vec![header(HeadLead::Tile(glyph), favorite_name(&channel, entry), guild.map(|g| SharedString::from(g.name.clone())))];

        let cid = channel.id.clone();
        let read = item("Mark as read", move |this, _, cx| this.mark_channel_read(&cid, cx)).icon(Glyph::Checks);
        out.push(if self.unread(&channel.id) { read } else { read.disabled() }.entry());
        out.push(Entry::Separator);

        let current = entry.nickname.clone();
        out.push(item("Change nickname", move |this, window, cx| this.rename_favorite(id, "Change nickname", "Shown in Favorites instead of the channel's name.", &current, window, cx)).icon(Glyph::Pen).entry());
        out.push(self.favorite_move_entry(id, entry.parent_id));
        if let Some(gid) = guild_id.clone() {
            let open = channel.clone();
            out.push(item("Go to server", move |this, window, cx| {
                this.open_guild(Some(gid.clone()), window, cx);
                this.open_channel(open.clone(), window, cx);
            })
            .icon(Glyph::Open)
            .entry());
        }
        out.push(Entry::Separator);

        out.extend(self.channel_notify_entries(&channel, guild_id.as_deref().unwrap_or("@me")).into_iter().next());
        out.push(Entry::Separator);

        out.extend(self.favorite_remove_entry(&channel));
        if self.developer_mode {
            let cid = channel.id.clone();
            out.push(Entry::Separator);
            out.push(item("Copy channel ID", move |this, _, cx| this.copy_text(cid.clone(), cx)).icon(Glyph::Id).entry());
        }
        out
    }

    fn favorite_move_entry(&self, id: u64, parent_id: u64) -> Entry {
        let move_to = move |parent: u64| move |this: &mut Waffle, _: &mut Window, cx: &mut Context<Waffle>| this.edit_favorite(id, |e| e.parent_id = parent, cx);
        let none = item("No category", move_to(0)).icon(Glyph::Minus);
        let mut rows = vec![if parent_id == 0 { none.disabled() } else { none }.entry(), Entry::Separator];
        rows.extend(self.favorites.categories().into_iter().map(|(category, name)| {
            let row = item(name, move_to(category)).icon(Glyph::Folder);
            if parent_id == category { row.disabled() } else { row }.entry()
        }));
        submenu("Move to", rows).icon(Glyph::MoveTo).entry()
    }

    fn open_favorite_category_menu(&mut self, id: u64, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.open_live_menu(at, move |this| this.favorite_category_menu(id), cx);
    }

    fn favorite_category_menu(&self, id: u64) -> Vec<Entry> {
        let Some(entry) = self.favorites.channels.get(&id) else { return Vec::new() };
        let current = entry.nickname.clone();
        vec![
            item("Rename category", move |this, window, cx| this.rename_favorite(id, "Rename category", "Category name", &current, window, cx)).icon(Glyph::Pen).entry(),
            Entry::Separator,
            item("Remove category", move |this, _, cx| {
                this.confirm("Remove category", "Its channels will be uncategorized.", "Remove category", move |this, _, cx| this.remove_favorite(id, cx), cx);
            })
            .hint("Channels will be uncategorized")
            .danger()
            .icon(Glyph::Trash)
            .entry(),
        ]
    }

    /// The Favorites header's dropdown.
    pub(crate) fn open_favorites_menu(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.open_live_menu(at, |_| {
            vec![
                item("Create category", |this, window, cx| this.create_favorite_category(window, cx)).icon(Glyph::FolderPlus).entry(),
                Entry::Separator,
                item("Hide favorites", |this, _, cx| this.hide_favorites(cx)).danger().icon(Glyph::EyeSlash).entry(),
            ]
        }, cx);
    }

    fn create_favorite_category(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt("Create category", "Category name", "", "Create", |this, name, _, cx| {
            let name = name.trim();
            if name.is_empty() {
                return;
            }
            this.favorites.add_category(name, new_category_id());
            this.save_favorites(cx);
        }, window, cx);
    }

    fn rename_favorite(&mut self, id: u64, title: &'static str, body: &'static str, current: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt(title, body, current, "Save", move |this, name, _, cx| this.edit_favorite(id, |e| e.nickname = name.trim().to_owned(), cx), window, cx);
    }

    // THE CHANNEL LIST

    // Uncategorised channels, then each category with its channels as one sortable item, so a carried
    // category takes its channels along.
    pub(crate) fn favorite_rows(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if self.favorites.channels.is_empty() {
            return vec![favorites_empty(cx)];
        }
        self.drag_motion.set_tag(self.favorite_move_tag());
        let sections = self.favorites.sections();
        let category_order = sections.iter().filter_map(|(c, _)| c.map(|c| c.to_string())).collect();
        let categories = self.favorite_board.list(CATEGORY_LIST, category_order, window, cx);
        let mut rows = Vec::new();
        for (category, ids) in sections {
            let parent = category.unwrap_or(0);
            let collapsed = category.and_then(|c| self.favorites.channels.get(&c)).is_some_and(|e| e.collapsed);
            let shown: Vec<(String, AnyElement)> = ids.into_iter().filter_map(|id| Some((id.to_string(), self.favorite_row(id, parent, collapsed, cx)?))).collect();
            let list = self.favorite_board.list(group_list(parent), shown.iter().map(|(key, _)| key.clone()).collect(), window, cx);
            let channel_rows: Vec<AnyElement> = shown.into_iter().map(|(key, row)| list.item(&key, row)).collect();
            let Some(category) = category else {
                // With no uncategorised channels, a carried channel still needs somewhere to land at the top.
                let landing = channel_rows.is_empty() && self.carrying_favorite_channel();
                rows.extend(landing.then(|| Board::head::<_, FavoriteDrag, _>(div().h(px(ROW_HEIGHT)), favorite_board, group_list(0), cx).into_any_element()));
                rows.extend(channel_rows);
                continue;
            };
            let key = category.to_string();
            let section = div().flex().flex_col().gap(px(ROW_GAP)).child(self.favorite_category_head(category, collapsed, cx)).children(channel_rows);
            let section = Board::target(section, favorite_board, CATEGORY_LIST, key.clone(), |_: &FavoriteCategoryDrag| false, cx);
            let spaced = !rows.is_empty();
            rows.push(div().when(spaced, |d| d.mt(px(14.))).child(categories.item(&key, section)).into_any_element());
        }
        rows
    }

    // A category's header folds it on click and carries it (and its channels) when dragged; a channel
    // dropped on it goes first inside.
    fn favorite_category_head(&self, id: u64, collapsed: bool, cx: &mut Context<Self>) -> AnyElement {
        let name = self.favorites.channels.get(&id).map(|e| e.nickname.clone()).unwrap_or_default();
        let carry_key = format!("fav-cathead-{id}");
        let carried = self.drag_motion.row(&carry_key);
        let label = div().id(SharedString::from(format!("fav-cat-{id}"))).relative().children(self.drag_motion.spot(&carry_key));
        let label = match carried {
            RowDrag::Hole => hole(label, cx),
            _ => {
                let degrees: f32 = if collapsed { -90. } else { 0. };
                let target_ink = palette::accent_ink(cx);
                label
                    .when(carried == RowDrag::Hidden, |l| l.invisible())
                    .child(Nav::Caret.icon().size(px(11.)).rotate(Radians(degrees.to_radians())))
                    .child(ui::truncating(format!("fav-cat-name-{id}"), name.clone()))
                    .on_click(cx.listener(move |this, _, _, cx| this.edit_favorite(id, |e| e.collapsed = !e.collapsed, cx)))
                    .drag_over::<FavoriteDrag>(move |s, _, _, _| s.text_color(target_ink))
                    .on_mouse_down(MouseButton::Right, cx.listener(move |this, e: &MouseDownEvent, _, cx| this.open_favorite_category_menu(id, e.position, cx)))
            }
        };
        let size = point(px(self.channels_width - 16.), px(CATEGORY_HEAD_HEIGHT));
        let ghost = pick_up(cx.weak_entity(), carry_key.into(), Look::Category(name.into()), size, 1.05, 0., true);
        let label = label.on_drag(FavoriteCategoryDrag, Board::lift(cx.weak_entity(), favorite_board, id.to_string(), ghost));
        let label = Board::head::<_, FavoriteDrag, _>(label, favorite_board, group_list(id), cx);
        category_head(format!("fav-cathead-{id}").into(), label, false, cx).into_any_element()
    }

    // A favourite: its nickname or name, then its server's name muted at the end, or its mention count. Folded
    // away with its category unless it's open or has news.
    fn favorite_row(&self, id: u64, parent: u64, collapsed: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (channel, guild) = self.favorite_channel(id)?;
        let entry = self.favorites.channels.get(&id)?;
        let selected = self.in_favorites() && self.channel.as_ref().is_some_and(|c| c.id == channel.id);
        let guild_id = guild.map(|g| g.id.clone());
        let muted = self.guild_settings.get(guild_id.as_deref().unwrap_or("@me")).is_some_and(|s| s.channel_muted(&channel.id));
        let mentions = self.mention_count(&channel.id);
        let unread = self.unread(&channel.id) && !muted;
        if collapsed && !(selected || unread || mentions > 0) {
            return None;
        }
        let origin = guild.map(|g| g.name.clone()).filter(|_| mentions == 0);
        let faint = palette::faint(cx);
        let name = favorite_name(&channel, entry);
        let carry_key = format!("fav-{}", channel.id);
        let carried = self.drag_motion.row(&carry_key);
        let row = channel_row(carry_key.clone().into(), format!("fav-row-{}", channel.id).into(), RowState { selected, unread, muted, strong: false }, cx).children(self.drag_motion.spot(&carry_key));
        let row = match carried {
            RowDrag::Hole => hole(row.bg(gpui_kit::transparent_black()), cx),
            _ => row
                .when(carried == RowDrag::Hidden, |r| r.invisible())
                .child(favorite_glyph(&channel).size(px(18.)).flex_none().text_color(if selected { palette::muted(cx) } else { faint }))
                .child(ui::truncating(format!("fav-name-{}", channel.id), name.clone()).flex_1())
                .children(origin.map(|o| div().flex_none().max_w(px(96.)).truncate().text_size(px(12.)).font_weight(FontWeight::NORMAL).text_color(faint).child(o)))
                .when(mentions > 0, |r| r.child(ui::count_badge(mentions))),
        };
        let size = point(px(self.channels_width - 16.), px(ROW_HEIGHT));
        let look = Look::Channel { channel: Box::new(Channel { name, ..channel.clone() }), people: Vec::new() };
        let ghost = pick_up(cx.weak_entity(), carry_key.into(), look, size, 1.05, 0., true);
        let row = row.on_drag(FavoriteDrag, Board::lift(cx.weak_entity(), favorite_board, id.to_string(), ghost));
        let row = Board::target(row, favorite_board, group_list(parent), id.to_string(), |_: &FavoriteDrag| false, cx);
        let row = row.on_mouse_down(MouseButton::Right, cx.listener(move |this, e: &MouseDownEvent, _, cx| this.open_favorite_menu(id, e.position, cx)));
        let row = match channel.kind {
            ChannelKind::Voice => row.on_click(cx.listener(move |this, _, _, cx| this.join_voice_in(guild_id.clone(), channel.clone(), cx))),
            _ => row.on_click(cx.listener(move |this, _, window, cx| this.open_favorite(channel.clone(), guild_id.clone(), window, cx))),
        };
        Some(row.into_any_element())
    }
}

fn favorite_board(waffle: &mut Waffle) -> &mut Board {
    &mut waffle.favorite_board
}

fn group_list(parent: u64) -> SharedString {
    format!("fav-group-{parent}").into()
}

fn favorite_name(channel: &Channel, entry: &FavoriteChannel) -> String {
    if entry.nickname.is_empty() { channel.name.clone() } else { entry.nickname.clone() }
}

fn favorite_glyph(channel: &Channel) -> Icon {
    match channel.kind {
        ChannelKind::Dm => Icon::new(Glyph::ChatRound),
        _ => channel_glyph(channel),
    }
}

fn favorites_empty(cx: &mut Context<Waffle>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(8.))
        .py(px(32.))
        .px(px(16.))
        .text_center()
        .text_size(px(14.))
        .text_color(palette::muted(cx))
        .child(div().text_color(palette::faint(cx)).child(Icon::new(Glyph::Star).size(px(32.))))
        .child(div().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child("Favorites will show up here!"))
        .child("Right-click any channel and pick Add to favorites.")
        .into_any_element()
}

// The protobuf wire format, just enough for settings-proto: varints, fixed64 and length-delimited fields.
mod wire {
    pub enum Value<'a> {
        Varint(u64),
        Fixed64(u64),
        Bytes(&'a [u8]),
        Fixed32,
    }

    // `raw` is the whole field, key included, so an unknown one can be written back as it came.
    pub struct Field<'a> {
        pub number: u64,
        pub value: Value<'a>,
        pub raw: &'a [u8],
    }

    pub fn fields(mut buf: &[u8]) -> Option<Vec<Field<'_>>> {
        let mut out = Vec::new();
        while !buf.is_empty() {
            let start = buf;
            let key = varint(&mut buf)?;
            let value = match key & 7 {
                0 => Value::Varint(varint(&mut buf)?),
                1 => Value::Fixed64(u64::from_le_bytes(take(&mut buf, 8)?.try_into().ok()?)),
                2 => {
                    let len = usize::try_from(varint(&mut buf)?).ok()?;
                    Value::Bytes(take(&mut buf, len)?)
                }
                5 => {
                    take(&mut buf, 4)?;
                    Value::Fixed32
                }
                _ => return None,
            };
            out.push(Field { number: key >> 3, value, raw: &start[..start.len() - buf.len()] });
        }
        Some(out)
    }

    // A google.protobuf wrapper's value (BoolValue, Int32Value): field 1, absent meaning zero.
    pub fn wrapped(buf: &[u8]) -> Option<u64> {
        let value = fields(buf)?.into_iter().find_map(|f| match (f.number, f.value) {
            (1, Value::Varint(v)) => Some(v),
            _ => None,
        });
        Some(value.unwrap_or(0))
    }

    pub fn wrap(value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        if value != 0 {
            put_uint(&mut out, 1, value);
        }
        out
    }

    fn varint(buf: &mut &[u8]) -> Option<u64> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let (&byte, rest) = buf.split_first()?;
            *buf = rest;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }

    fn take<'a>(buf: &mut &'a [u8], len: usize) -> Option<&'a [u8]> {
        if buf.len() < len {
            return None;
        }
        let (head, rest) = buf.split_at(len);
        *buf = rest;
        Some(head)
    }

    fn put_varint(out: &mut Vec<u8>, mut value: u64) {
        while value >= 0x80 {
            out.push((value as u8) | 0x80);
            value >>= 7;
        }
        out.push(value as u8);
    }

    fn put_key(out: &mut Vec<u8>, number: u64, wire_type: u64) {
        put_varint(out, (number << 3) | wire_type);
    }

    pub fn put_uint(out: &mut Vec<u8>, number: u64, value: u64) {
        put_key(out, number, 0);
        put_varint(out, value);
    }

    pub fn put_fixed64(out: &mut Vec<u8>, number: u64, value: u64) {
        put_key(out, number, 1);
        out.extend_from_slice(&value.to_le_bytes());
    }

    pub fn put_bytes(out: &mut Vec<u8>, number: u64, bytes: &[u8]) {
        put_key(out, number, 2);
        put_varint(out, bytes.len() as u64);
        out.extend_from_slice(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Favorites { 5: {"gen", channel, position 2, parent 9, channel_type 0, unknown field 7}, 9: {"Work",
    // category, collapsed}, muted, guild_visible true, unknown field 4 }, laid out as the encoder writes it.
    fn favorites_bytes() -> Vec<u8> {
        let channel = [&[0x0a, 3, b'g', b'e', b'n', 0x10, 1, 0x18, 2, 0x21, 9, 0, 0, 0, 0, 0, 0, 0, 0x2a, 0][..], &[0x38, 5]].concat();
        let category = [0x0a, 4, b'W', b'o', b'r', b'k', 0x10, 2, 0x30, 1];
        let entry = |id: u8, value: &[u8]| [&[0x09, id, 0, 0, 0, 0, 0, 0, 0, 0x12, value.len() as u8][..], value].concat();
        let (a, b) = (entry(5, &channel), entry(9, &category));
        [&[0x0a, a.len() as u8][..], &a, &[0x0a, b.len() as u8], &b, &[0x10, 1, 0x1a, 2, 0x08, 1, 0x20, 1]].concat()
    }

    #[test]
    fn favorites_round_trip_byte_for_byte() {
        let bytes = favorites_bytes();
        let favorites = Favorites::decode(&bytes).expect("decodes");
        let channel = &favorites.channels[&5];
        assert_eq!((channel.nickname.as_str(), channel.kind, channel.position, channel.parent_id, channel.channel_type), ("gen", CHANNEL, 2, 9, Some(0)));
        assert!(favorites.channels[&9].is_category() && favorites.channels[&9].collapsed);
        assert!(favorites.muted);
        assert_eq!(favorites.guild_visible, Some(true));
        assert_eq!(favorites.encode(), bytes);
    }

    #[test]
    fn settings_skip_other_fields_and_find_field_15() {
        let favorites = favorites_bytes();
        let settings = [&[0x0a, 2, 0x08, 1, 0x7a, favorites.len() as u8][..], &favorites, &[0x18, 0x96, 0x01]].concat();
        let found = Favorites::from_settings(&settings).expect("well formed").expect("has favorites");
        assert_eq!(found.channels.len(), 2);
        assert_eq!(Favorites::from_settings(&found.to_settings()), Some(Some(found)));
        assert_eq!(Favorites::from_settings(&[0x0a, 2, 0x08, 1]), Some(None));
        assert_eq!(Favorites::from_settings(&[0x7a, 9, 1]), None);
    }

    #[test]
    fn limits_count_channels_per_tier_and_every_entry_overall() {
        let mut favorites = Favorites::default();
        for id in 1..=3 {
            favorites.add_channel(id, 0, 0);
        }
        assert_eq!(favorites.over_limit(0), Some(FREE_LIMIT));
        assert_eq!(favorites.over_limit(NITRO), None);
        favorites.remove(3);
        for id in 100..110 {
            favorites.add_category(&format!("c{id}"), id);
        }
        assert_eq!(favorites.over_limit(0), None, "categories don't count against the tier");
        for id in 1000..1188 {
            favorites.add_channel(id, 0, 0);
        }
        assert_eq!(favorites.channels.len(), MAX_ENTRIES);
        assert_eq!(favorites.over_limit(NITRO), Some(MAX_ENTRIES));
    }

    #[test]
    fn sections_put_loose_first_and_voice_after_text() {
        let mut favorites = Favorites::default();
        favorites.add_channel(1, 2, 0);
        favorites.add_channel(2, 0, 0);
        let work = favorites.add_category("Work", 50);
        assert_eq!(favorites.add_category("work", 51), work);
        favorites.add_channel(3, 0, work);
        favorites.add_channel(4, 0, 77);
        assert_eq!(favorites.sections(), vec![(None, vec![2, 4, 1]), (Some(work), vec![3])]);
    }

    #[test]
    fn prune_drops_gone_and_hidden_but_keeps_gone_threads() {
        let mut favorites = Favorites::default();
        favorites.add_channel(1, 0, 0);
        favorites.add_channel(2, 0, 0);
        favorites.add_channel(3, 11, 0);
        favorites.add_channel(4, 0, 9);
        favorites.prune(|id| match id {
            1 => Reach::Gone,
            2 => Reach::Hidden,
            _ => Reach::Viewable,
        });
        assert_eq!(favorites.channels.keys().copied().collect::<Vec<_>>(), [3, 4]);
        assert_eq!(favorites.channels[&4].parent_id, 0);
        favorites.prune(|_| Reach::Gone);
        assert_eq!(favorites.channels.keys().copied().collect::<Vec<_>>(), [3]);
    }

    // Root 1, 2, 3; Work (50) holding 4, 5; Play (60) empty.
    fn arranged() -> Favorites {
        let mut favorites = Favorites::default();
        for id in 1..=3 {
            favorites.add_channel(id, 0, 0);
        }
        let work = favorites.add_category("Work", 50);
        favorites.add_category("Play", 60);
        favorites.add_channel(4, 0, work);
        favorites.add_channel(5, 0, work);
        favorites
    }

    fn positions(favorites: &Favorites, ids: &[u64]) -> Vec<u32> {
        ids.iter().map(|id| favorites.channels[id].position).collect()
    }

    #[test]
    fn arrange_moves_within_a_group_and_renumbers_it() {
        let mut favorites = arranged();
        assert!(favorites.arrange(3, Slot::Beside { target: 1, after: false }));
        assert_eq!(favorites.sections()[0], (None, vec![3, 1, 2]));
        assert_eq!(positions(&favorites, &[3, 1, 2]), [0, 1, 2]);
        assert!(favorites.arrange(3, Slot::Beside { target: 2, after: true }));
        assert_eq!(favorites.sections()[0], (None, vec![1, 2, 3]));
        assert!(!favorites.arrange(3, Slot::Beside { target: 2, after: true }), "already there");
        assert!(!favorites.arrange(3, Slot::Beside { target: 3, after: false }), "onto itself");
    }

    #[test]
    fn arrange_moves_across_groups_and_renumbers_both() {
        let mut favorites = arranged();
        assert!(favorites.arrange(1, Slot::Beside { target: 4, after: true }));
        assert_eq!(favorites.sections(), vec![(None, vec![2, 3]), (Some(50), vec![4, 1, 5]), (Some(60), vec![])]);
        assert_eq!(favorites.channels[&1].parent_id, 50);
        assert_eq!(positions(&favorites, &[2, 3]), [0, 1]);
        assert_eq!(positions(&favorites, &[4, 1, 5]), [0, 1, 2]);
        assert!(favorites.arrange(5, Slot::First { parent: 60 }));
        assert_eq!(favorites.sections()[2], (Some(60), vec![5]));
        assert_eq!(positions(&favorites, &[4, 1]), [0, 1]);
        assert!(favorites.arrange(4, Slot::First { parent: 0 }));
        assert_eq!(favorites.sections()[0], (None, vec![4, 2, 3]));
        assert_eq!(favorites.channels[&4].parent_id, 0);
    }

    #[test]
    fn arrange_reorders_categories_but_never_nests_them() {
        let mut favorites = arranged();
        assert!(favorites.arrange(60, Slot::Beside { target: 50, after: false }));
        assert_eq!(favorites.categories().iter().map(|(id, _)| *id).collect::<Vec<_>>(), [60, 50]);
        assert_eq!(positions(&favorites, &[60, 50]), [0, 1]);
        let before = favorites.clone();
        assert!(!favorites.arrange(60, Slot::First { parent: 50 }), "a category into a category");
        assert!(!favorites.arrange(60, Slot::Beside { target: 4, after: false }), "a category among channels");
        assert!(!favorites.arrange(1, Slot::Beside { target: 50, after: false }), "a channel among categories");
        assert!(!favorites.arrange(1, Slot::First { parent: 2 }), "a channel into a channel");
        assert_eq!(favorites, before);
    }

    #[test]
    fn adding_shows_hidden_favorites_and_removing_a_category_frees_its_channels() {
        let mut favorites = Favorites { guild_visible: Some(false), ..Default::default() };
        let work = favorites.add_category("Work", 50);
        favorites.add_channel(1, 0, work);
        assert_eq!(favorites.guild_visible, Some(true));
        favorites.remove(work);
        assert_eq!(favorites.channels[&1].parent_id, 0);
    }
}
