use std::{collections::HashMap, path::PathBuf, sync::OnceLock, time::Duration};

use futures::{SinkExt, StreamExt, channel::mpsc};
use serde_json::{Value, json};
use tokio::runtime::Runtime;
use tokio_tungstenite::tungstenite::Message as WsMessage;

const GATEWAY: &str = "wss://gateway.discord.gg/?v=10&encoding=json";
pub const API: &str = "https://discord.com/api/v10";
pub(crate) const CDN: &str = "https://cdn.discordapp.com";
pub(crate) const VIEW_CHANNEL: u64 = 1 << 10;

// The user-account gateway and several fields below (nameplates, guild tags, member list ops,
// read states) are undocumented; shapes are from memory of the official client. Not verified live.

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct User {
    pub id: String,
    pub name: String,
    pub avatar: String,
    pub bot: bool,
    pub tag: Option<GuildTag>,
    pub nameplate: Option<Box<Nameplate>>,
    // Avatar decoration frame drawn around the avatar.
    pub decoration: Option<String>,
    pub name_style: Option<NameStyle>,
    // The account's username (`name` is the shown one); empty where Discord didn't send it.
    #[serde(default)]
    pub username: String,
}

// Discord's display-name style: a font, an effect and the effect's colours (0xRRGGBB). Ids are its
// DisplayNameFont / DisplayNameEffect enums, read from its web client (2026-11-19).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NameStyle {
    pub font: u32,
    pub effect: NameEffect,
    pub colors: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum NameEffect {
    Solid,
    Gradient,
    Neon,
    Toon,
    Pop,
    Glow,
    Prism,
    Gummy,
}

// `display_name_styles`: { font_id, effect_id, colors }. An unknown effect draws solid, as Discord's does.
fn parse_name_style(styles: &Value) -> Option<NameStyle> {
    let styles = styles.as_object()?;
    let effect = match styles.get("effect_id").and_then(Value::as_u64) {
        Some(2) => NameEffect::Gradient,
        Some(3) => NameEffect::Neon,
        Some(4) => NameEffect::Toon,
        Some(5) => NameEffect::Pop,
        Some(6) => NameEffect::Glow,
        Some(7) => NameEffect::Prism,
        Some(8) => NameEffect::Gummy,
        _ => NameEffect::Solid,
    };
    let colors = styles.get("colors").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64).filter(|c| *c <= 0xFF_FFFF).map(|c| c as u32).collect();
    Some(NameStyle { font: styles.get("font_id").and_then(Value::as_u64).unwrap_or(11) as u32, effect, colors })
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Nameplate {
    pub art: String,
    // The moving version, as an animated image: what Discord plays where it can't play the WebM
    // (Safari, Linux, reduced motion). From the nameplate's `sku_id`.
    #[serde(default)]
    pub animated: Option<String>,
    // The tint Discord paints under the art: (dark theme, light theme), from the nameplate's palette.
    pub tint: (u32, u32),
}

// Discord's nameplate palettes, read from its web client (module 641886, 2026-11-14).
fn palette_tint(name: &str) -> (u32, u32) {
    match name {
        "crimson" => (0x900007, 0xE7040F),
        "berry" => (0x893A99, 0xB11FCF),
        "sky" => (0x0080B7, 0x56CCFF),
        "teal" => (0x086460, 0x7DEED7),
        "forest" => (0x2D5401, 0x6AA624),
        "bubble_gum" => (0xDC3E97, 0xF957B3),
        "violet" => (0x730BC8, 0x972FED),
        "cobalt" => (0x0131C2, 0x4278FF),
        "clover" => (0x047B20, 0x63CD5A),
        "lemon" => (0xF6CD12, 0xFED400),
        "white" => (0xFFFFFF, 0xFFFFFF),
        _ => (0x000000, 0x000000),
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct GuildTag {
    pub text: String,
    pub badge: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Status {
    Online,
    Idle,
    Dnd,
    #[default]
    Offline,
}

impl Status {
    fn parse(s: &str) -> Self {
        match s {
            "online" => Status::Online,
            "idle" => Status::Idle,
            "dnd" => Status::Dnd,
            _ => Status::Offline,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Status::Online => "Online",
            Status::Idle => "Idle",
            Status::Dnd => "Do Not Disturb",
            Status::Offline => "Invisible",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Presence {
    pub status: Status,
    // The custom status ("🍡 brb"), apart from what they're doing.
    pub custom: Option<String>,
    // The first activity other than the custom status, with "+N" for the rest.
    pub activity: Option<Activity>,
    // The first non-custom activity in full, for the profile's rich presence card.
    pub rich: Option<RichPresence>,
    // Signed in on a phone and nowhere else.
    pub mobile_only: bool,
    // Any activity of the Streaming kind (a Twitch or YouTube link), not a Go Live screen share.
    pub streaming: bool,
}

impl Presence {
    // What the avatar corner shows: Discord swaps online for a phone when they are only on mobile,
    // and any shown status for the streaming mark while they stream.
    pub fn mark(&self) -> Mark {
        match self.status {
            Status::Offline => Mark::Status(Status::Offline),
            _ if self.streaming => Mark::Streaming,
            Status::Online if self.mobile_only => Mark::Mobile,
            status => Mark::Status(status),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Status(Status),
    Mobile,
    Streaming,
}

impl From<Status> for Mark {
    fn from(status: Status) -> Self {
        Mark::Status(status)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RichPresence {
    pub kind: ActivityKind,
    pub name: String,
    pub app_id: Option<String>,
    pub details: Option<String>,
    pub state: Option<String>,
    pub image: Option<String>,
    // (current, max) party size.
    pub party: Option<(u64, u64)>,
    // Unix milliseconds the activity started.
    pub started: Option<i64>,
    // Unix milliseconds it ends: a Spotify track's end, for its progress.
    pub ends: Option<i64>,
    // Discord's party id: people sharing one play together (Active Now groups them).
    pub party_id: Option<String>,
    pub joinable: bool,
}

// One card of the member list's Activity section: what someone played, watched or heard lately.
#[derive(Clone, Debug, PartialEq)]
pub struct FeedEntry {
    pub author_id: String,
    pub kind: ActivityKind,
    pub title: String,
    // Album art or a show's poster; a game has none here, only its application (for its icon).
    pub image: Option<String>,
    pub app_id: Option<String>,
    pub live: bool,
    // Unix milliseconds: when it ended, else when it started, else when the entry was made.
    pub at: i64,
    pub streak_days: Option<u64>,
}

// What Discord's member list shows under a name: an icon by kind, the text, and "+N" for more.
#[derive(Clone, Debug, PartialEq)]
pub struct Activity {
    pub kind: ActivityKind,
    pub text: String,
    // The other activities after the first, in Discord's order, for "+N" and its hover list.
    pub rest: Vec<(ActivityKind, String)>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ActivityKind {
    Custom,
    #[default]
    Playing,
    Streaming,
    Listening,
    Watching,
    Competing,
}

#[derive(Clone, Debug)]
pub struct Role {
    pub id: String,
    pub name: String,
    pub color: u32,
    pub position: i64,
    pub permissions: u64,
    // Owned by an integration (a bot's role); nobody can hand these out.
    pub managed: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Guild {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub channels: Vec<Channel>,
    pub roles: Vec<Role>,
    pub voice_states: Vec<VoiceState>,
    // Stage channels that are live right now.
    pub stage_instances: Vec<StageInstance>,
    pub emojis: Vec<Emoji>,
    pub stickers: Vec<Sticker>,
    pub owner_id: Option<String>,
    // The server's own notification default (0 all messages, 1 only mentions), used until we pick one.
    pub default_notifications: u8,
    // Active threads we know of (joined ones come in READY; more arrive as they're used).
    pub threads: Vec<Channel>,
    // Our own roles here, for working out what moderation we may do.
    pub my_roles: Vec<String>,
    // Server boost level 0-3: sets the highest voice bitrate.
    pub premium_tier: u8,
    // People idling here don't count as voice activity (Discord's server icon badge skips it).
    pub afk_channel_id: Option<String>,
    // Our own timeout here (communication_disabled_until); it may already have passed.
    pub timed_out_until: Option<chrono::DateTime<chrono::Utc>>,
    // Scheduled events, from READY / GUILD_CREATE and the GUILD_SCHEDULED_EVENT_* events.
    pub events: Vec<ScheduledEvent>,
    // A Community server's community-updates channel, which can't be an announcement channel.
    pub public_updates_channel_id: Option<String>,
    // Which join, boost and reminder notices the server hides or keeps quiet (Discord's SystemChannelFlags).
    pub system_channel_flags: u64,
    // Discord's feature flags (COMMUNITY, GUILD_ONBOARDING, MEMBER_VERIFICATION_GATE_ENABLED, ...).
    pub features: Vec<String>,
    pub vanity_url_code: Option<String>,
    // The invite splash: the image a server shows behind its invites and onboarding.
    pub splash: Option<String>,
    pub rules_channel_id: Option<String>,
    // Our own membership here: pending rules, onboarding and to-do flags, join date.
    pub me: Membership,
}

// Our member entry's join state. `flags` are Discord's guild member flags (see `joining::flags`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Membership {
    pub pending: bool,
    pub flags: u64,
    pub joined_at: Option<String>,
}

impl Membership {
    pub fn parse(m: &Value) -> Self {
        Membership { pending: m["pending"].as_bool().unwrap_or(false), flags: m["flags"].as_u64().unwrap_or(0), joined_at: m["joined_at"].as_str().map(str::to_owned) }
    }
}

impl Guild {
    pub fn has_feature(&self, feature: &str) -> bool {
        self.features.iter().any(|f| f == feature)
    }
}

// A guild scheduled event (docs/discord-events-re.md). Status 1 scheduled, 2 active, 3 completed, 4 canceled;
// entity type 1 stage, 2 voice, 3 somewhere else (`location`).
#[derive(Clone, Debug, Default)]
pub struct ScheduledEvent {
    pub id: String,
    pub guild_id: String,
    pub channel_id: Option<String>,
    pub creator_id: Option<String>,
    pub creator: Option<User>,
    pub name: String,
    pub description: Option<String>,
    // Cover image hash; the url is `event_cover_url`.
    pub image: Option<String>,
    // ISO 8601, as Discord sends them.
    pub start: String,
    pub end: Option<String>,
    pub status: u8,
    pub entity_type: u8,
    pub location: Option<String>,
    // How many marked Interested, and whether we did; neither comes with the event itself.
    pub interested: u32,
    pub me_interested: bool,
    // Discord's recurrence_rule as sent (start, frequency, interval, by_weekday, by_n_weekday, …).
    pub recurrence: Option<Value>,
    // A series' per-occurrence changes (guild_scheduled_event_exceptions): moved or canceled occurrences.
    pub exceptions: Vec<EventException>,
}

// One occurrence of a series changed: its id is the occurrence's recurrence id.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EventException {
    pub id: String,
    pub start: Option<String>,
    pub end: Option<String>,
    pub canceled: bool,
}

pub fn parse_event_exception(e: &Value) -> Option<EventException> {
    Some(EventException {
        id: e["event_exception_id"].as_str()?.to_owned(),
        start: e["scheduled_start_time"].as_str().map(str::to_owned),
        end: e["scheduled_end_time"].as_str().map(str::to_owned),
        canceled: e["is_canceled"].as_bool().unwrap_or(false),
    })
}

impl ScheduledEvent {
    pub const SCHEDULED: u8 = 1;
    pub const ACTIVE: u8 = 2;
    pub const COMPLETED: u8 = 3;
    pub const CANCELED: u8 = 4;
    pub const STAGE: u8 = 1;
    pub const VOICE: u8 = 2;
    pub const EXTERNAL: u8 = 3;

    pub fn upcoming(&self) -> bool {
        !matches!(self.status, Self::COMPLETED | Self::CANCELED)
    }

    pub fn cover_url(&self) -> Option<String> {
        self.image.as_ref().map(|hash| format!("{CDN}/guild-events/{}/{hash}.png?size=1024", self.id))
    }
}

pub fn parse_scheduled_event(e: &Value) -> Option<ScheduledEvent> {
    Some(ScheduledEvent {
        id: e["id"].as_str()?.to_owned(),
        guild_id: e["guild_id"].as_str()?.to_owned(),
        channel_id: e["channel_id"].as_str().map(str::to_owned),
        creator_id: e["creator_id"].as_str().map(str::to_owned),
        creator: e["creator"].is_object().then(|| parse_user(&e["creator"])),
        name: e["name"].as_str().unwrap_or_default().to_owned(),
        description: e["description"].as_str().filter(|d| !d.is_empty()).map(str::to_owned),
        image: e["image"].as_str().map(str::to_owned),
        start: e["scheduled_start_time"].as_str().unwrap_or_default().to_owned(),
        end: e["scheduled_end_time"].as_str().map(str::to_owned),
        status: e["status"].as_u64().unwrap_or(1) as u8,
        entity_type: e["entity_type"].as_u64().unwrap_or(0) as u8,
        location: e["entity_metadata"]["location"].as_str().map(str::to_owned),
        interested: e["user_count"].as_u64().unwrap_or(0) as u32,
        me_interested: false,
        recurrence: e["recurrence_rule"].is_object().then(|| e["recurrence_rule"].clone()),
        exceptions: e["guild_scheduled_event_exceptions"].as_array().into_iter().flatten().filter_map(parse_event_exception).collect(),
    })
}

// Your notification settings for one server, or for DMs under the key "@me" (Discord's
// user_guild_settings). Message notification levels: 0 all, 1 mentions, 2 nothing, 3 inherit.
#[derive(Clone, Debug, PartialEq)]
pub struct GuildSettings {
    pub muted: bool,
    // RFC 3339 end of a timed mute; None for "until I turn it back on".
    pub mute_end: Option<String>,
    // The length the mute was picked for, in seconds (-1 forever); None when Discord sent none.
    pub mute_window: Option<i64>,
    pub notifications: u8,
    pub suppress_everyone: bool,
    pub suppress_roles: bool,
    // Discord's highlights setting: 0 unset, 1 disabled (Suppress Highlights on), 2 enabled.
    pub notify_highlights: u8,
    pub mute_scheduled_events: bool,
    pub mobile_push: bool,
    pub hide_muted: bool,
    // user_guild_settings flags; bit 14 (16384) is "pick your channels" (opt-in channels on).
    pub flags: u64,
    pub channels: HashMap<String, ChannelSettings>,
}

// Mobile push is on until turned off, as in Discord.
impl Default for GuildSettings {
    fn default() -> Self {
        Self {
            muted: false,
            mute_end: None,
            mute_window: None,
            notifications: 0,
            suppress_everyone: false,
            suppress_roles: false,
            notify_highlights: 0,
            mute_scheduled_events: false,
            mobile_push: true,
            hide_muted: false,
            flags: 0,
            channels: HashMap::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChannelSettings {
    pub muted: bool,
    pub mute_end: Option<String>,
    pub notifications: u8,
    // channel_overrides flags; bit 12 (4096) follows the channel (or category) when opt-in is on.
    pub flags: u64,
}

impl GuildSettings {
    // A timed mute that has run out no longer counts, as in Discord.
    pub fn is_muted(&self) -> bool {
        self.muted && still_muted(self.mute_end.as_deref())
    }

    pub fn channel_muted(&self, channel_id: &str) -> bool {
        self.channels.get(channel_id).is_some_and(|c| c.muted && still_muted(c.mute_end.as_deref()))
    }
}

fn still_muted(end: Option<&str>) -> bool {
    end.and_then(|e| chrono::DateTime::parse_from_rfc3339(e).ok()).is_none_or(|end| end > chrono::Utc::now())
}

pub fn parse_guild_settings(g: &Value) -> (String, GuildSettings) {
    let key = g["guild_id"].as_str().unwrap_or("@me").to_owned();
    let channels = g["channel_overrides"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            Some((
                c["channel_id"].as_str()?.to_owned(),
                ChannelSettings {
                    muted: c["muted"].as_bool().unwrap_or(false),
                    mute_end: c["mute_config"]["end_time"].as_str().map(str::to_owned),
                    notifications: c["message_notifications"].as_u64().unwrap_or(3) as u8,
                    flags: c["flags"].as_u64().unwrap_or(0),
                },
            ))
        })
        .collect();
    let settings = GuildSettings {
        muted: g["muted"].as_bool().unwrap_or(false),
        mute_end: g["mute_config"]["end_time"].as_str().map(str::to_owned),
        mute_window: g["mute_config"]["selected_time_window"].as_i64(),
        notifications: g["message_notifications"].as_u64().unwrap_or(0) as u8,
        suppress_everyone: g["suppress_everyone"].as_bool().unwrap_or(false),
        suppress_roles: g["suppress_roles"].as_bool().unwrap_or(false),
        notify_highlights: g["notify_highlights"].as_u64().unwrap_or(0) as u8,
        mute_scheduled_events: g["mute_scheduled_events"].as_bool().unwrap_or(false),
        mobile_push: g["mobile_push"].as_bool().unwrap_or(true),
        hide_muted: g["hide_muted_channels"].as_bool().unwrap_or(false),
        flags: g["flags"].as_u64().unwrap_or(0),
        channels,
    };
    (key, settings)
}

// Discord's permission bits used by the menus.
pub mod perm {
    pub const CREATE_INSTANT_INVITE: u64 = 1;
    pub const MANAGE_CHANNELS: u64 = 1 << 4;
    pub const KICK_MEMBERS: u64 = 1 << 1;
    pub const BAN_MEMBERS: u64 = 1 << 2;
    pub const ADMINISTRATOR: u64 = 1 << 3;
    pub const MUTE_MEMBERS: u64 = 1 << 22;
    pub const DEAFEN_MEMBERS: u64 = 1 << 23;
    pub const MOVE_MEMBERS: u64 = 1 << 24;
    pub const REQUEST_TO_SPEAK: u64 = 1 << 32;
    // Discord's client calls you a stage moderator only with all three.
    pub const STAGE_MODERATOR: u64 = MANAGE_CHANNELS | MUTE_MEMBERS | MOVE_MEMBERS;
    pub const SEND_MESSAGES: u64 = 1 << 11;
    pub const SEND_MESSAGES_IN_THREADS: u64 = 1 << 38;
    pub const BYPASS_SLOWMODE: u64 = 1 << 52;
    pub const MANAGE_MESSAGES: u64 = 1 << 13;
    pub const CHANGE_NICKNAME: u64 = 1 << 26;
    pub const MANAGE_NICKNAMES: u64 = 1 << 27;
    pub const MANAGE_ROLES: u64 = 1 << 28;
    pub const MANAGE_WEBHOOKS: u64 = 1 << 29;
    pub const MANAGE_THREADS: u64 = 1 << 34;
    pub const CREATE_PUBLIC_THREADS: u64 = 1 << 35;
    pub const MODERATE_MEMBERS: u64 = 1 << 40;
    pub const SEND_POLLS: u64 = 1 << 49;
    pub const SET_VOICE_CHANNEL_STATUS: u64 = 1 << 48;
    pub const PIN_MESSAGES: u64 = 1 << 51;
    pub const CONNECT: u64 = 1 << 20;
    pub const SPEAK: u64 = 1 << 21;
    pub const USE_SOUNDBOARD: u64 = 1 << 42;
    pub const USE_EXTERNAL_SOUNDS: u64 = 1 << 45;
    pub const READ_MESSAGE_HISTORY: u64 = 1 << 16;
    pub const MANAGE_EVENTS: u64 = 1 << 33;
    pub const CREATE_EVENTS: u64 = 1 << 44;
    pub const SEND_TTS_MESSAGES: u64 = 1 << 12;
    pub const MENTION_EVERYONE: u64 = 1 << 17;
}

pub mod message_flag {
    pub const SUPPRESS_EMBEDS: u64 = 1 << 2;
    pub const EPHEMERAL: u64 = 1 << 6;
    pub const IS_VOICE_MESSAGE: u64 = 1 << 13;
}

impl Guild {
    // Server-wide permissions (no channel overwrites): owner and Administrator have everything.
    pub fn permissions(&self, me: &str) -> u64 {
        self.permissions_with(me, &self.my_roles)
    }

    fn permissions_with(&self, user: &str, roles: &[String]) -> u64 {
        if self.owner_id.as_deref() == Some(user) {
            return u64::MAX;
        }
        let bits = self.roles.iter().filter(|r| r.id == self.id || roles.contains(&r.id)).fold(0, |acc, r| acc | r.permissions);
        if bits & perm::ADMINISTRATOR != 0 { u64::MAX } else { bits }
    }

    pub fn can(&self, me: &str, bit: u64) -> bool {
        self.permissions(me) & bit != 0
    }

    // Discord's order: server permissions, then the @everyone overwrite, then all our roles' overwrites
    // together, then one for us personally. Threads follow their parent channel.
    pub fn channel_permissions(&self, me: &str, channel: &Channel) -> u64 {
        self.channel_permissions_with(me, &self.my_roles, channel)
    }

    // `user` holding `roles`: our own permissions with `my_roles`, someone else's with theirs from the member list.
    pub fn channel_permissions_with(&self, user: &str, roles: &[String], channel: &Channel) -> u64 {
        // Roles not loaded yet: don't hide anything rather than hide everything.
        if self.roles.is_empty() {
            return u64::MAX;
        }
        let base = self.permissions_with(user, roles);
        if base == u64::MAX {
            return base;
        }
        let channel = self.overwrite_source(channel);
        let apply = |bits: u64, allow: u64, deny: u64| (bits & !deny) | allow;
        let mut bits = base;
        if let Some(o) = channel.overwrites.iter().find(|o| o.id == self.id) {
            bits = apply(bits, o.allow, o.deny);
        }
        let (allow, deny) = channel.overwrites.iter().filter(|o| roles.contains(&o.id)).fold((0, 0), |(a, d), o| (a | o.allow, d | o.deny));
        bits = apply(bits, allow, deny);
        if let Some(o) = channel.overwrites.iter().find(|o| o.id == user) {
            bits = apply(bits, o.allow, o.deny);
        }
        bits
    }

    // Threads follow their parent channel's overwrites.
    fn overwrite_source<'a>(&'a self, channel: &'a Channel) -> &'a Channel {
        match (&channel.thread, &channel.parent_id) {
            (Some(_), Some(parent)) => self.channels.iter().find(|c| &c.id == parent).unwrap_or(channel),
            _ => channel,
        }
    }

    // The roles whose members may post in `channel`, highest first: Administrator, or the send bit after
    // the @everyone overwrite and the role's own. A member's personal overwrites aren't counted. Leaves out
    // @everyone and integration roles.
    pub fn posting_roles(&self, channel: &Channel) -> Vec<&Role> {
        let bit = send_permission(channel);
        let source = self.overwrite_source(channel);
        let everyone = self.roles.iter().find(|r| r.id == self.id).map_or(0, |r| r.permissions);
        let overwrite = |bits: u64, id: &str| source.overwrites.iter().find(|o| o.id == id && !o.member).map_or(bits, |o| (bits & !o.deny) | o.allow);
        let mut roles: Vec<&Role> = self
            .roles
            .iter()
            .filter(|r| r.id != self.id && !r.managed)
            .filter(|r| {
                let base = everyone | r.permissions;
                base & perm::ADMINISTRATOR != 0 || overwrite(overwrite(base, &self.id), &r.id) & bit != 0
            })
            .collect();
        roles.sort_by_key(|r| std::cmp::Reverse(r.position));
        roles
    }

    // A channel or one of the threads we know of.
    pub fn stage(&self, channel_id: &str) -> Option<&StageInstance> {
        self.stage_instances.iter().find(|s| s.channel_id == channel_id)
    }

    pub fn is_stage_moderator(&self, me: &str, channel: &Channel) -> bool {
        self.is_stage_moderator_with(me, &self.my_roles, channel)
    }

    pub fn is_stage_moderator_with(&self, user: &str, roles: &[String], channel: &Channel) -> bool {
        self.channel_permissions_with(user, roles, channel) & perm::STAGE_MODERATOR == perm::STAGE_MODERATOR
    }

    pub fn channel(&self, id: &str) -> Option<&Channel> {
        self.channels.iter().chain(&self.threads).find(|c| c.id == id)
    }

    /// Discord sends every channel in a server, including ones we can't open; fetching those is a 403.
    pub fn can_view(&self, me: &str, channel: &Channel) -> bool {
        self.channel_permissions(me, channel) & VIEW_CHANNEL != 0
    }

    // Discord's voice move rule (Modify Guild Member, and its menu code): moving ourselves is joining,
    // which needs Connect; moving someone else needs Move members where they are and where they go,
    // and Connect there. Its menus leave out full channels.
    pub fn can_move(&self, me: &str, user_id: &str, to: &Channel) -> bool {
        let present = self.voice_states.iter().filter(|v| v.channel_id.as_deref() == Some(to.id.as_str())).count();
        if to.props.user_limit > 0 && present >= to.props.user_limit as usize {
            return false;
        }
        let has = |channel: &Channel, bit: u64| self.channel_permissions(me, channel) & bit != 0;
        if user_id == me {
            return has(to, perm::CONNECT);
        }
        let from = self.voice_states.iter().find(|v| v.user_id == user_id).and_then(|v| self.channel(v.channel_id.as_deref()?));
        from.is_some_and(|c| has(c, perm::MOVE_MEMBERS)) && has(to, perm::MOVE_MEMBERS) && has(to, perm::CONNECT)
    }

    // Where an invite is made: the first text channel we may invite from, one everyone sees if there is one.
    pub fn invite_channel(&self, me: &str) -> Option<&Channel> {
        let bits = VIEW_CHANNEL | perm::CREATE_INSTANT_INVITE;
        self.channels.iter().filter(|c| c.kind == ChannelKind::Text && self.channel_permissions(me, c) & bits == bits).min_by_key(|c| c.private)
    }

    // The highest role we hold; Discord lets us manage only roles below it.
    pub fn top_position(&self, me: &str) -> i64 {
        if self.owner_id.as_deref() == Some(me) {
            return i64::MAX;
        }
        self.roles.iter().filter(|r| self.my_roles.contains(&r.id)).map(|r| r.position).max().unwrap_or(0)
    }
}

// Posting in a thread takes its own bit, not Send Messages.
pub fn send_permission(channel: &Channel) -> u64 {
    match channel.thread {
        Some(_) => perm::SEND_MESSAGES_IN_THREADS,
        None => perm::SEND_MESSAGES,
    }
}

#[derive(Clone, Debug, Default)]
pub struct VoiceState {
    pub guild_id: Option<String>,
    pub channel_id: Option<String>,
    pub user_id: String,
    pub user: Option<User>,
    pub muted: bool,
    pub deafened: bool,
    // Muted or deafened by a moderator, which only a moderator can undo.
    pub server_muted: bool,
    pub server_deafened: bool,
    // Live on Go Live, or camera on.
    pub streaming: bool,
    pub camera: bool,
    // Stage channels: in the audience (Discord's `suppress`), and when they asked to speak
    // (`request_to_speak_timestamp`, ISO 8601). See docs/discord-stage-re.md.
    pub suppress: bool,
    pub requested_at: Option<String>,
}

// Where someone stands on a stage, read from `suppress` and `request_to_speak_timestamp` exactly as
// Discord's client derives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnStage {
    Audience,
    HandRaised,
    // A moderator invited them; they haven't accepted yet.
    Invited,
    Speaker,
}

impl VoiceState {
    pub fn on_stage(&self) -> OnStage {
        match (self.suppress, self.requested_at.is_some()) {
            (true, false) => OnStage::Audience,
            (true, true) => OnStage::HandRaised,
            (false, true) => OnStage::Invited,
            (false, false) => OnStage::Speaker,
        }
    }
}

// A live stage: Discord's stage instance, keyed by its channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageInstance {
    pub channel_id: String,
    pub topic: String,
}

pub fn parse_stage_instance(v: &Value) -> Option<StageInstance> {
    Some(StageInstance { channel_id: v["channel_id"].as_str()?.to_owned(), topic: v["topic"].as_str().unwrap_or_default().to_owned() })
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub kind: ChannelKind,
    pub parent_id: Option<String>,
    pub position: i64,
    pub topic: Option<String>,
    pub last_message_id: Option<String>,
    // @everyone can't view it: drawn with a lock.
    pub private: bool,
    // A 1:1 DM's other person, for their avatar and status dot.
    pub recipient: Option<User>,
    // Everyone in a DM but us; a group's faces come from the first two.
    #[serde(default)]
    pub recipients: Vec<User>,
    // A group DM's own icon.
    #[serde(default)]
    pub icon: Option<String>,
    pub thread: Option<ThreadMeta>,
    // Discord's own channel type number (0 text, 2 voice, 5 announcement, 15 forum, …).
    #[serde(default)]
    pub raw_type: u8,
    // Per-role and per-member permission changes on top of the server's; `id` is a role or user id.
    #[serde(default)]
    pub overwrites: Vec<Overwrite>,
    // Boxed: rarely read, and it keeps every Channel-carrying event small.
    #[serde(default)]
    pub props: Box<ChannelProps>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Overwrite {
    pub id: String,
    pub allow: u64,
    pub deny: u64,
    // Discord's overwrite type 1: `id` is a member, not a role.
    #[serde(default)]
    pub member: bool,
}

// What channel settings edit beyond name and topic (Discord's Modify Channel fields).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChannelProps {
    pub nsfw: bool,
    // Slowmode, seconds (rate_limit_per_user).
    pub slowmode: u32,
    // Voice and stage only.
    pub bitrate: u32,
    pub user_limit: u32,
    // None: Discord picks automatically.
    pub rtc_region: Option<String>,
    // video_quality_mode 2: 720p for everyone.
    pub video_full: bool,
    // Minutes until an idle thread hides (default_auto_archive_duration); 0 when Discord sent none.
    pub archive_after: u32,
    // Forum and media channels (docs/discord-forums-re.md): the tags a post can carry, whether one is
    // required (flag 16), and the defaults for Sort & View.
    pub tags: Vec<ForumTag>,
    pub require_tag: bool,
    // default_sort_order 1: newest post first instead of latest activity.
    pub sort_by_creation: bool,
    // default_forum_layout 2 (gallery); media channels are always a gallery.
    pub gallery: bool,
    // default_tag_setting "match_all" instead of "match_some".
    pub match_all: bool,
    pub default_reaction: Option<Emoji>,
    // Forum only: what a new post's message starts as.
    #[serde(default)]
    pub template: String,
    // A group DM's owner, the only one who may remove members.
    #[serde(default)]
    pub owner_id: Option<String>,
    // A DM from someone we aren't friends with, waiting in Message Requests (is_message_request).
    #[serde(default)]
    pub message_request: bool,
    // Filtered into Message Requests' Spam tab (is_spam).
    #[serde(default)]
    pub spam: bool,
}

impl Channel {
    // Waits in Message Requests (either tab) instead of the DM list.
    pub fn is_request(&self) -> bool {
        self.props.message_request || self.props.spam
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ForumTag {
    pub id: String,
    pub name: String,
    pub emoji: Option<Emoji>,
    // Only people with Manage Threads may apply it.
    pub moderated: bool,
}

fn parse_forum_emoji(id: &Value, name: &Value) -> Option<Emoji> {
    match (id.as_str(), name.as_str()) {
        (Some(id), name) => Some(Emoji { id: Some(id.to_owned()), name: name.unwrap_or_default().to_owned(), animated: false }),
        (None, Some(name)) if !name.is_empty() => Some(Emoji { id: None, name: name.to_owned(), animated: false }),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ChannelKind {
    Text,
    Voice,
    Category,
    Dm,
    // A thread or forum post; `Channel::thread` carries its state.
    Thread,
    // A forum or media channel: a list of posts, each a thread.
    Forum,
    Other,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ThreadMeta {
    pub archived: bool,
    pub locked: bool,
    // We're a member: Discord lists joined threads under their channel.
    pub joined: bool,
    pub message_count: u32,
    pub owner_id: Option<String>,
    // Our thread-member settings: muted, and the notification flags (2 all, 4 mentions, 8 nothing).
    pub muted: bool,
    pub flags: u64,
    // A forum post's tags (ids into its forum's `ChannelProps::tags`), and whether it's pinned (channel flag 2).
    #[serde(default)]
    pub applied_tags: Vec<String>,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Media {
    pub url: String,
    pub width: f32,
    pub height: f32,
    pub kind: MediaKind,
    // The attachment's id; None for media from an embed.
    #[serde(default)]
    pub id: Option<String>,
    // Alt text.
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub spoiler: bool,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum MediaKind {
    #[default]
    Image,
    // `poster` is a still frame to show until the inline player starts.
    Video { poster: String },
    // A GIF embed: a silent looping MP4 that plays inline, with a still to show until it's ready.
    Gif { poster: String },
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Embed {
    pub url: Option<String>,
    pub color: Option<u32>,
    pub provider: Option<String>,
    #[serde(default)]
    pub provider_url: Option<String>,
    pub author: Option<String>,
    pub author_icon: Option<String>,
    pub author_url: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub fields: Vec<(String, String, bool)>,
    // The small picture at the top right.
    pub thumbnail: Option<Media>,
    // The large picture under the text; embeds sharing a url add theirs here, up to four.
    #[serde(default)]
    pub images: Vec<Media>,
    // A video embed (YouTube): `images` holds its still, drawn with a play button.
    #[serde(default)]
    pub video: bool,
    // Flagged explicit, gore or self-harm: the media stays blurred until revealed.
    #[serde(default)]
    pub sensitive: bool,
    pub footer: Option<String>,
    pub footer_icon: Option<String>,
    // RFC 3339; Discord prints it after the footer text.
    pub timestamp: Option<String>,
    // A closed poll's result (type poll_result); the other fields stay empty.
    #[serde(default)]
    pub poll: Option<PollResult>,
    // An AutoMod safety alert (type auto_moderation_notification): a raid or mention-raid card.
    #[serde(default)]
    pub safety_alert: bool,
    // Discord's type `rich` (a bot's or webhook's own embed), not an unfurled link.
    #[serde(default)]
    pub rich: bool,
}

impl Embed {
    // Discord's inline predicate: media shown bare, without the card (and without the card's ✕).
    pub fn inline(&self) -> bool {
        (!self.images.is_empty() || self.video) && !self.rich && self.author.is_none() && self.title.is_none()
    }
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PollResult {
    pub question: String,
    // None when nobody won: no votes, or a tie at the top.
    pub winner: Option<(String, Option<Emoji>)>,
    pub winner_votes: u64,
    pub total_votes: u64,
}

#[derive(Clone, Debug)]
pub struct Folder {
    pub id: String,
    pub name: Option<String>,
    pub color: Option<u32>,
    pub guild_ids: Vec<String>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct File {
    pub name: String,
    pub url: String,
    pub size: u64,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub spoiler: bool,
    // A voice message's length and loudness, as Discord sends them with the recording.
    #[serde(default)]
    pub duration_secs: Option<f32>,
    // One loudness byte per sample (0-255), up to 256; empty when missing or not valid base64.
    #[serde(default)]
    pub waveform: Vec<u8>,
}

impl File {
    // Discord plays a file inline by its name alone, not its content type.
    pub fn is_audio(&self) -> bool {
        let lower = self.name.to_ascii_lowercase();
        !self.url.is_empty() && [".mp3", ".m4a", ".ogg", ".opus", ".wav", ".flac"].iter().any(|ext| lower.ends_with(ext))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Emoji {
    pub id: Option<String>,
    pub name: String,
    pub animated: bool,
}

/// Your own custom status: what your user settings hold, and what goes out in your presence.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomStatus {
    pub text: String,
    pub emoji: Option<Emoji>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl CustomStatus {
    // A setting with neither text nor emoji, or one already past its expiry, is no status.
    fn parse(v: &Value, now: chrono::DateTime<chrono::Utc>) -> Option<Self> {
        let text = v["text"].as_str().unwrap_or_default().to_owned();
        let emoji = v["emoji_name"].as_str().filter(|n| !n.is_empty()).map(|name| Emoji { id: v["emoji_id"].as_str().map(str::to_owned), name: name.to_owned(), animated: false });
        let expires_at = v["expires_at"].as_str().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).map(|t| t.with_timezone(&chrono::Utc));
        if (text.is_empty() && emoji.is_none()) || expires_at.is_some_and(|t| t <= now) {
            return None;
        }
        Some(CustomStatus { text, emoji, expires_at })
    }

    /// The user-settings shape (`PATCH /users/@me/settings`).
    pub fn setting(&self) -> Value {
        json!({
            "text": (!self.text.is_empty()).then_some(&self.text),
            "emoji_name": self.emoji.as_ref().map(|e| &e.name),
            "emoji_id": self.emoji.as_ref().and_then(|e| e.id.as_ref()),
            "expires_at": self.expires_at.map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        })
    }

    // Not verified: the activity's shape, taken from what Discord's own client sends.
    fn activity(&self) -> Value {
        json!({
            "type": 4,
            "name": "Custom Status",
            "state": (!self.text.is_empty()).then_some(&self.text),
            "emoji": self.emoji.as_ref().map(|e| json!({ "name": e.name, "id": e.id, "animated": e.animated })),
        })
    }
}

impl Emoji {
    pub fn unicode(name: &str) -> Self {
        Emoji { id: None, name: name.into(), animated: false }
    }

    pub fn url(&self) -> Option<String> {
        let id = self.id.as_ref()?;
        Some(format!("{CDN}/emojis/{id}.{}?size=48", if self.animated { "gif" } else { "webp" }))
    }

    // `key` percent-encoded for a URL path, as reaction routes need for unicode emoji.
    pub fn path(&self) -> String {
        let mut url = reqwest::Url::parse("https://x/").expect("static URL");
        if let Ok(mut segments) = url.path_segments_mut() {
            segments.pop_if_empty().push(&self.key());
        }
        url.path().trim_start_matches('/').to_owned()
    }

    // The REST path segment: `name:id` for custom emoji, the character itself for unicode.
    fn key(&self) -> String {
        match &self.id {
            Some(id) => format!("{}:{id}", self.name),
            None => self.name.clone(),
        }
    }
}

// The pseudo server Discord files its own soundboard sounds under.
pub const DEFAULT_SOUNDS: &str = "0";

// A soundboard sound: a server's, or one of Discord's own (`guild_id` "0").
#[derive(Clone, Debug, PartialEq)]
pub struct SoundboardSound {
    pub id: String,
    pub name: String,
    pub guild_id: String,
    // 0..=1, set by whoever uploaded it.
    pub volume: f32,
    pub emoji: Option<Emoji>,
    // False once the server lost the boost level that held it.
    pub available: bool,
}

impl SoundboardSound {
    pub fn parse(v: &Value) -> Option<Self> {
        Some(SoundboardSound {
            id: id_string(&v["sound_id"])?,
            name: v["name"].as_str().unwrap_or_default().to_owned(),
            guild_id: id_string(&v["guild_id"]).unwrap_or_else(|| DEFAULT_SOUNDS.into()),
            volume: v["volume"].as_f64().unwrap_or(1.) as f32,
            emoji: sound_emoji(&v["emoji_id"], &v["emoji_name"]),
            available: v["available"].as_bool().unwrap_or(true),
        })
    }

    pub fn url(&self) -> String {
        soundboard_url(&self.id)
    }

    pub fn is_default(&self) -> bool {
        self.guild_id == DEFAULT_SOUNDS
    }
}

pub fn soundboard_url(sound_id: &str) -> String {
    format!("{CDN}/soundboard-sounds/{sound_id}")
}

// Someone played a sound or an emoji in a voice channel (VOICE_CHANNEL_EFFECT_SEND).
#[derive(Clone, Debug, PartialEq)]
pub struct VoiceEffect {
    pub channel_id: String,
    pub user_id: String,
    pub sound_id: Option<String>,
    // 0..=1; Discord's client falls back to 1.
    pub sound_volume: f32,
    pub emoji: Option<Emoji>,
    // 0 premium, 1 basic.
    pub animation_type: Option<u8>,
    // Not verified: what the id selects; Discord's client passes it to its premium animation.
    pub animation_id: Option<u64>,
    // The sound's name.
    pub name: Option<String>,
    pub source_guild_id: Option<String>,
}

fn parse_voice_effect(d: &Value) -> Option<VoiceEffect> {
    Some(VoiceEffect {
        channel_id: id_string(&d["channel_id"])?,
        user_id: id_string(&d["user_id"])?,
        sound_id: id_string(&d["sound_id"]),
        sound_volume: d["sound_volume"].as_f64().unwrap_or(1.) as f32,
        emoji: d["emoji"].is_object().then(|| sound_emoji(&d["emoji"]["id"], &d["emoji"]["name"])).flatten(),
        animation_type: d["animation_type"].as_u64().map(|t| t as u8),
        animation_id: d["animation_id"].as_u64(),
        name: d["name"].as_str().map(str::to_owned),
        source_guild_id: id_string(&d["source_guild_id"]),
    })
}

// Snowflakes arrive as strings, but Discord's default sounds have small numeric ids that may come as numbers.
// Not verified: which form the default sounds use on the wire; both are read.
fn id_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

// A sound's emoji: a server's by id, or a Unicode one by its character.
fn sound_emoji(id: &Value, name: &Value) -> Option<Emoji> {
    let id = id_string(id);
    let name = name.as_str().unwrap_or_default().to_owned();
    (id.is_some() || !name.is_empty()).then_some(Emoji { id, name, animated: false })
}

/// Someone who reacted, with the handle the Reactions dialog shows under their name (`User` keeps only the
/// display name).
#[derive(Clone, Debug, Default)]
pub struct Reactor {
    pub user: User,
    pub username: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Reaction {
    pub emoji: Emoji,
    pub count: u32,
    pub me: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SendState {
    #[default]
    Sent,
    Sending,
    Failed,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Message {
    pub id: String,
    pub channel_id: String,
    pub guild_id: Option<String>,
    pub author: User,
    // The author's roles in this guild when the payload carried them (gateway messages do, REST history doesn't).
    pub roles: Option<Vec<String>>,
    pub content: String,
    pub timestamp: String,
    pub edited: bool,
    // Attachments plus image, GIF and video embeds, in order: what the gallery pages through.
    pub media: Vec<Media>,
    // Rich link cards (articles, YouTube, bots).
    pub embeds: Vec<Embed>,
    pub files: Vec<File>,
    // The uploads among `media`, as files: `media_files[i]` is `media[i]`, since attachments lead `media`.
    #[serde(default)]
    pub media_files: Vec<File>,
    pub reactions: Vec<Reaction>,
    pub reply_to: Option<String>,
    pub reply: Option<Box<Message>>,
    pub mentions: Vec<User>,
    pub mention_roles: Vec<String>,
    pub mention_everyone: bool,
    pub nonce: Option<String>,
    pub state: SendState,
    pub pinned: bool,
    // The thread started from this message, for Discord's thread chip.
    pub thread: Option<Channel>,
    // A DM call's record (message type 3): drawn as one line, not a message.
    #[serde(default)]
    pub call: Option<CallRecord>,
    #[serde(default)]
    pub stickers: Vec<Sticker>,
    // A forwarded message's copy of the original: its id, channel_id and guild_id are the original's; its
    // author is unknown, since Discord's snapshot leaves it out.
    #[serde(default)]
    pub forward: Option<Box<Message>>,
    #[serde(default)]
    pub poll: Option<Poll>,
    // Discord's "X pinned a message" notice (type 6): never drawn, it names who pinned `reply_to`.
    #[serde(default)]
    pub pin_notice: bool,
    // An app's answer to a command, button or form: who triggered it.
    #[serde(default)]
    pub interaction: Option<crate::app_commands::InteractionMeta>,
    #[serde(default)]
    pub components: Vec<crate::app_commands::Component>,
    // The app that sent it (or whose interaction it answers); `None` from a person.
    #[serde(default)]
    pub application_id: Option<String>,
    // Why our command's stand-in failed: Discord's `interactionError`, shown in its first 3 seconds.
    #[serde(default)]
    pub interaction_error: Option<String>,
    // Discord's message type (0 default, 7 join, 18 thread created…); anything outside the user-message
    // types is a system notice.
    #[serde(default)]
    pub kind: u8,
    // Discord's message flags: SUPPRESS_EMBEDS (1 << 2), `SUPPRESS_NOTIFICATIONS`, and `app_commands::EPHEMERAL`,
    // `LOADING` (the app is thinking), `COMPONENTS_V2`.
    #[serde(default)]
    pub flags: u64,
    // The referenced message's channel: a thread-created notice's thread, a thread starter's parent.
    #[serde(default)]
    pub reference_channel: Option<String>,
    // Discord sends `referenced_message: null` when the referenced message was deleted, and leaves the
    // field out when it couldn't load it; a thread starter words the two differently.
    #[serde(default)]
    pub reference_deleted: bool,
    // Who added a bot, on the bot's join notice.
    #[serde(default)]
    pub actor: Option<User>,
    // How many of `media`'s last entries came from embeds (images, GIFs), so an update can replace them.
    #[serde(default)]
    pub embed_media: usize,
    // Sent with /tts: read aloud to whoever has the channel open.
    #[serde(default)]
    pub tts: bool,
    // A crosspost's source, from its message_reference: the followed server and channel.
    #[serde(default)]
    pub crosspost_from: Option<CrosspostSource>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CrosspostSource {
    pub guild_id: String,
    pub channel_id: String,
}

// Message flag SUPPRESS_NOTIFICATIONS: sent with @silent, so nobody gets a push or desktop notification.
pub const SUPPRESS_NOTIFICATIONS: u64 = 1 << 12;

impl Message {
    pub const CROSSPOSTED: u64 = 1;
    pub const IS_CROSSPOST: u64 = 2;
    pub const SOURCE_MESSAGE_DELETED: u64 = 8;

    pub fn embeds_suppressed(&self) -> bool {
        self.flags & message_flag::SUPPRESS_EMBEDS != 0
    }

    // Its audio files play in the voice player, and it can't be edited.
    pub fn is_voice(&self) -> bool {
        self.flags & message_flag::IS_VOICE_MESSAGE != 0
    }

    pub fn silent(&self) -> bool {
        self.flags & SUPPRESS_NOTIFICATIONS != 0
    }

    pub fn send_options(&self) -> SendOptions {
        SendOptions { silent: self.silent(), tts: self.tts }
    }
}

/// Per-send options Create Message carries beside the content: @silent's flag and /tts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SendOptions {
    pub silent: bool,
    pub tts: bool,
}

impl SendOptions {
    pub fn flags(self) -> u64 {
        if self.silent { SUPPRESS_NOTIFICATIONS } else { 0 }
    }

    fn apply(self, body: &mut Value) {
        if self.silent {
            body["flags"] = json!(self.flags());
        }
        if self.tts {
            body["tts"] = json!(true);
        }
    }
}

/// A live poll as a message carries it, its counts folded into the answers.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Poll {
    pub question: String,
    pub answers: Vec<PollAnswer>,
    // RFC 3339; None for a poll that never ends.
    pub expiry: Option<String>,
    pub multiselect: bool,
    // Discord has closed it and the counts are final.
    pub finalized: bool,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PollAnswer {
    pub id: u32,
    pub text: String,
    pub emoji: Option<Emoji>,
    pub count: u64,
    // You voted for it.
    pub me: bool,
}

/// A poll to post: Create Message's `poll` before Discord gives its answers ids.
#[derive(Clone, Debug, PartialEq)]
pub struct PollDraft {
    pub question: String,
    pub answers: Vec<(String, Option<Emoji>)>,
    pub hours: u32,
    pub multiselect: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StickerFormat {
    #[default]
    Png,
    Apng,
    Lottie,
    Gif,
}

/// A sticker as messages, servers and packs carry it.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Sticker {
    pub id: String,
    pub name: String,
    pub format: StickerFormat,
    // Discord's related emoji or keywords, comma-separated; search matches them too.
    #[serde(default)]
    pub tags: String,
}

impl Sticker {
    fn parse(v: &Value) -> Option<Self> {
        let format = match v["format_type"].as_u64() {
            Some(2) => StickerFormat::Apng,
            Some(3) => StickerFormat::Lottie,
            Some(4) => StickerFormat::Gif,
            _ => StickerFormat::Png,
        };
        Some(Sticker { id: v["id"].as_str()?.to_owned(), name: v["name"].as_str().unwrap_or_default().to_owned(), format, tags: v["tags"].as_str().unwrap_or_default().to_owned() })
    }

    /// The picture at `size`, or None for Lottie: the CDN serves those only as JSON, which gpui can't draw.
    /// APNG comes as an animated WebP, which gpui plays (checked against the media proxy, 2026-11-19).
    // Not verified: GIF stickers at `.gif` (no GIF sticker was at hand to fetch).
    pub fn url(&self, size: u32) -> Option<String> {
        let id = &self.id;
        match self.format {
            StickerFormat::Lottie => None,
            StickerFormat::Gif => Some(format!("https://media.discordapp.net/stickers/{id}.gif?size={size}")),
            StickerFormat::Apng => Some(format!("https://media.discordapp.net/stickers/{id}.webp?size={size}&animated=true")),
            StickerFormat::Png => Some(format!("https://media.discordapp.net/stickers/{id}.png?size={size}")),
        }
    }

    pub fn animated(&self) -> bool {
        self.format != StickerFormat::Png
    }

    pub fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        self.name.to_lowercase().contains(&query) || self.tags.to_lowercase().contains(&query)
    }
}

/// One of Discord's own sticker packs (`GET /sticker-packs`).
#[derive(Clone, Debug)]
pub struct StickerPack {
    pub name: String,
    pub stickers: Vec<Sticker>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CallRecord {
    // Everyone who joined; the caller is the message's author.
    pub participants: Vec<String>,
    // None while the call is still going.
    pub ended: Option<String>,
}

// What a profile popout shows beyond the user object. From GET /users/{id}/profile (user-account API, Not verified).
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Profile {
    pub username: String,
    pub bio: Option<String>,
    pub pronouns: Option<String>,
    pub banner: Option<String>,
    pub accent: Option<u32>,
    pub roles: Vec<String>,
    pub decoration: Option<String>,
    // The card's gradient, top to bottom (`user_profile.theme_colors`); None paints the plain surface.
    pub theme: Option<(u32, u32)>,
    pub badges: Vec<Badge>,
    pub mutual_friends: Vec<User>,
    pub mutual_friend_count: usize,
    pub mutual_server_count: usize,
    // GAP: the API field behind Discord's Game Collection is unknown, so only the demo fills this (icon URLs).
    pub games: Vec<String>,
    // When they joined the server the profile was fetched for (`guild_member.joined_at`, ISO 8601).
    pub joined: Option<String>,
    pub connections: Vec<Connection>,
    // The profile effect they wear: its collectible's sku, resolved through `/collectibles-products/{sku}`.
    pub effect_sku: Option<String>,
}

// An account linked on their profile (`connected_accounts[]`); `kind` is Discord's platform type ("spotify").
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Connection {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub verified: bool,
}

// One image of a profile effect, placed in the effect's own pixels (`effects[]` of its config).
#[derive(Clone, Debug, PartialEq)]
pub struct EffectLayer {
    pub src: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub z: i64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Badge {
    pub description: String,
    pub icon: String,
}

#[derive(Clone, Debug)]
pub struct Member {
    pub user: User,
    pub roles: Vec<String>,
    pub presence: Presence,
    // `premium_since`: when they started boosting this server (ISO 8601), if they do.
    pub boosting_since: Option<String>,
}

// Lists are almost all members; boxing them would allocate per row to shrink the rare group headers.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
pub enum ListItem {
    Group { id: String, count: u32 },
    Member(Member),
}

#[derive(Clone, Debug)]
pub enum ListOp {
    Sync { start: usize, items: Vec<ListItem> },
    Insert { index: usize, item: ListItem },
    Update { index: usize, item: ListItem },
    Delete { index: usize },
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ReadState {
    pub last_read: u64,
    pub mentions: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    Friend,
    Blocked,
    Incoming,
    Outgoing,
}

#[derive(Clone, Debug)]
pub struct Relationship {
    pub user: User,
    pub kind: Relation,
    // When it became this relationship (ISO 8601): Friends Since for a friend.
    pub since: Option<String>,
}

// One result from Discord's GIF search (its Tenor proxy). `preview` is an animated GIF gpui can draw.
#[derive(Clone, Debug)]
pub struct Gif {
    pub url: String,
    pub preview: String,
    pub width: f32,
    pub height: f32,
}

pub struct Ready {
    pub me: User,
    pub status: Status,
    pub custom_status: Option<CustomStatus>,
    pub session_id: String,
    pub guilds: Vec<Guild>,
    pub dms: Vec<Channel>,
    pub read_states: Vec<(String, ReadState)>,
    pub presences: Vec<(String, Presence)>,
    // The rail's order and folders. A folder without an id is a single, unfoldered server.
    pub folders: Vec<Folder>,
    pub relationships: Vec<Relationship>,
    // Settings › Advanced › Developer Mode: shows the Copy … ID items.
    pub developer_mode: bool,
    pub account_settings: AccountSettings,
    pub my_account: MyAccount,
    pub guild_settings: Vec<(String, GuildSettings)>,
    // Users we've ignored (Discord's newer, softer block).
    pub ignored: Vec<String>,
}

// What Settings › My account edits about you, beyond the `User` every screen draws.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MyAccount {
    pub username: String,
    pub global_name: Option<String>,
    pub banner: Option<String>,
    // Nitro: Discord takes banners and animated avatars only from subscribers.
    pub premium: bool,
    // 0 none, 1 Nitro Classic, 2 Nitro, 3 Nitro Basic: sets the upload cap (`upload_limit`).
    pub premium_type: u8,
}

impl MyAccount {
    pub fn parse(user: &Value) -> Self {
        let id = user["id"].as_str().unwrap_or_default();
        let banner = user["banner"].as_str().map(|hash| format!("{CDN}/banners/{id}/{hash}.{}?size=600", if hash.starts_with("a_") { "gif" } else { "png" }));
        Self {
            username: user["username"].as_str().unwrap_or_default().to_owned(),
            global_name: user["global_name"].as_str().map(str::to_owned),
            banner,
            premium: user["premium_type"].as_u64().is_some_and(|t| t > 0),
            premium_type: user["premium_type"].as_u64().unwrap_or(0) as u8,
        }
    }
}

// The account-wide settings Waffle shows, from the legacy `user_settings` object (READY, USER_SETTINGS_UPDATE).
// Field names are Discord's; `default_guilds_restricted` and the friend sources are inverted or split in the UI.
#[derive(Clone, Debug, PartialEq)]
pub struct AccountSettings {
    pub developer_mode: bool,
    pub render_embeds: bool,
    pub render_reactions: bool,
    pub show_current_game: bool,
    pub default_guilds_restricted: bool,
    // 0 scans no DMs, 1 DMs from non-friends, 2 every DM.
    pub explicit_content_filter: u8,
    pub friends_all: bool,
    pub friends_mutual_friends: bool,
    pub friends_mutual_guilds: bool,
    // Chat and accessibility rows Discord syncs (docs/discord-settings-re.md). Believed, not checked against a
    // live READY: the legacy object carries these keys under the same names as the protobuf fields.
    pub message_display_compact: bool,
    pub inline_attachment_media: bool,
    pub inline_embed_media: bool,
    pub gif_auto_play: bool,
    pub animate_emoji: bool,
    // 0 always, 1 on interaction, 2 never.
    pub animate_stickers: u8,
    pub convert_emoticons: bool,
    // From the settings proto (settings::proto): DmSpamFilterV2, 0 while unset.
    pub dm_spam_filter: u8,
    // Message Requests on: `default_message_request_restricted` off.
    pub message_requests: bool,
    // Servers whose members can't DM you, and servers that don't see your activity.
    pub restricted_guilds: Vec<String>,
    pub activity_restricted_guilds: Vec<String>,
    // Whether servers you join go on the activity list: 0 none, 1 those over 200 members, 2 all.
    pub default_guilds_activity_restricted: u8,
}

impl Default for AccountSettings {
    fn default() -> Self {
        Self {
            developer_mode: false,
            render_embeds: true,
            render_reactions: true,
            show_current_game: true,
            default_guilds_restricted: false,
            explicit_content_filter: 1,
            friends_all: true,
            friends_mutual_friends: true,
            friends_mutual_guilds: true,
            message_display_compact: false,
            inline_attachment_media: true,
            inline_embed_media: true,
            gif_auto_play: true,
            animate_emoji: true,
            animate_stickers: 0,
            convert_emoticons: true,
            dm_spam_filter: 0,
            message_requests: true,
            restricted_guilds: Vec::new(),
            activity_restricted_guilds: Vec::new(),
            default_guilds_activity_restricted: 0,
        }
    }
}

impl AccountSettings {
    // Takes the fields `v` carries and keeps the rest: USER_SETTINGS_UPDATE sends only what changed.
    pub fn apply(&mut self, v: &Value) {
        let flag = |key: &str, into: &mut bool| {
            if let Some(b) = v[key].as_bool() {
                *into = b;
            }
        };
        flag("developer_mode", &mut self.developer_mode);
        flag("render_embeds", &mut self.render_embeds);
        flag("render_reactions", &mut self.render_reactions);
        flag("show_current_game", &mut self.show_current_game);
        flag("default_guilds_restricted", &mut self.default_guilds_restricted);
        flag("message_display_compact", &mut self.message_display_compact);
        flag("inline_attachment_media", &mut self.inline_attachment_media);
        flag("inline_embed_media", &mut self.inline_embed_media);
        flag("gif_auto_play", &mut self.gif_auto_play);
        flag("animate_emoji", &mut self.animate_emoji);
        flag("convert_emoticons", &mut self.convert_emoticons);
        if let Some(when) = v["animate_stickers"].as_u64() {
            self.animate_stickers = when.min(2) as u8;
        }
        if v["restricted_guilds"].is_array() {
            self.restricted_guilds = strings(&v["restricted_guilds"]);
        }
        if v["activity_restricted_guild_ids"].is_array() {
            self.activity_restricted_guilds = strings(&v["activity_restricted_guild_ids"]);
        }
        if let Some(level) = v["explicit_content_filter"].as_u64() {
            self.explicit_content_filter = level.min(2) as u8;
        }
        if v["friend_source_flags"].is_object() {
            let sources = &v["friend_source_flags"];
            self.friends_all = sources["all"].as_bool().unwrap_or(false);
            self.friends_mutual_friends = self.friends_all || sources["mutual_friends"].as_bool().unwrap_or(false);
            self.friends_mutual_guilds = self.friends_all || sources["mutual_guilds"].as_bool().unwrap_or(false);
        }
    }

    pub fn friend_sources(&self) -> Value {
        json!({ "all": self.friends_all, "mutual_friends": self.friends_mutual_friends, "mutual_guilds": self.friends_mutual_guilds })
    }
}

pub enum Event {
    Ready(Box<Ready>),
    // Account settings changed here or in another client: only the changed fields.
    UserSettings(Value),
    // A Go Live stream by its key: who is watching, and whether the streamer paused it (Not verified
    // that `paused` is sent here; a stalled stream reads as Stalled without it).
    StreamUpdate { key: String, viewers: Vec<String>, paused: bool },
    // When each voice channel's current session began (Unix seconds); `all` replaces the server's list.
    VoiceStartTimes { guild_id: String, times: Vec<(String, Option<u64>)>, all: bool },
    // Each voice channel's status ("" for none); `all` replaces the server's list, as CHANNEL_INFO does.
    VoiceStatuses { guild_id: String, statuses: Vec<(String, String)>, all: bool },
    // A channel or thread created or changed in a server.
    ChannelUpsert { guild_id: String, channel: Channel },
    ChannelDelete { guild_id: String, id: String },
    // A scheduled event created or changed, deleted, or someone marked / unmarked Interested.
    ScheduledEvent(ScheduledEvent),
    ScheduledEventDelete { guild_id: String, id: String },
    // One occurrence's exception written (Some) or removed (None, by id).
    ScheduledEventException { guild_id: String, event_id: String, id: String, exception: Option<EventException> },
    ScheduledEventUser { guild_id: String, event_id: String, user_id: String, added: bool },
    // FORUM_UNREADS: unread message counts for a forum's posts, (post id, count).
    ForumUnreads { forum_id: String, counts: Vec<(String, u32)> },
    // A post someone just started in a forum (THREAD_CREATE with `newly_created`).
    ForumPostCreated { forum_id: String, post_id: String, owner_id: String },
    // A member's roles, nickname or server avatar changed.
    MemberUpdate { guild_id: String, user: User, roles: Vec<String>, timed_out_until: Option<chrono::DateTime<chrono::Utc>>, membership: Membership },
    // A friend, block or request was added, changed or removed (`None` kind = removed).
    RelationshipChange { user_id: String, relationship: Option<Relationship> },
    // A DM or group DM was opened, changed (name, icon, owner, request state) or closed.
    DmUpsert(Channel),
    DmDelete(String),
    // Someone joined or left a group DM (CHANNEL_RECIPIENT_ADD / _REMOVE).
    RecipientChange { channel_id: String, user: User, added: bool },
    // Our membership of a thread changed (joined or left).
    ThreadJoined { id: String, joined: bool },
    GuildSettings((String, GuildSettings)),
    Batch(Vec<Event>),
    GuildCreate(Guild),
    MessageCreate(Box<Message>),
    // Partial: apply with `apply_update`.
    MessageUpdate(Value),
    MessageDelete { channel_id: String, id: String },
    Reaction { channel_id: String, message_id: String, user_id: String, emoji: Emoji, added: bool },
    PollVote { channel_id: String, message_id: String, user_id: String, answer_id: u32, added: bool },
    Typing { channel_id: String, user_id: String, name: Option<String> },
    Presence { user_id: String, presence: Presence },
    MemberList { guild_id: String, groups: Vec<(String, u32)>, ops: Vec<ListOp> },
    MembersChunk { guild_id: String, members: Vec<(User, Vec<String>)> },
    // `manual`: Mark Unread from any of our devices, carrying the mention count after `message_id`.
    Ack { channel_id: String, message_id: u64, manual: Option<u32> },
    // With the session it belongs to: ours, or another client of ours.
    VoiceState(VoiceState, Option<String>),
    // A stage went live or changed topic (`Some`), or ended (`None`).
    StageInstance { guild_id: String, channel_id: String, instance: Option<StageInstance> },
    // A DM or group call started or changed; `ringing` is who it rings now.
    Call { channel_id: String, ringing: Vec<String> },
    CallEnded { channel_id: String },
    // Raw voice server info (endpoint, token) for the voice engine.
    VoiceServer(Value),
    // A sound or emoji played in a voice channel.
    VoiceEffect(VoiceEffect),
    // A server's whole soundboard (`all`), or sounds added or changed on it.
    SoundboardSounds { guild_id: String, sounds: Vec<SoundboardSound>, all: bool },
    SoundboardSoundDelete { guild_id: String, sound_id: String },
    Disconnected(String),
    // A dropped connection picked its session back up: nothing to reload.
    Resumed,
    AuthFailed,
    // Our interaction (by the nonce we sent) was received, answered, or refused; `id` is Discord's id
    // for it, which the app's reply carries in `interaction_metadata.id`.
    InteractionCreate { nonce: String, id: String },
    InteractionSuccess { nonce: String },
    // `reason_code` is Discord's ReasonCodes enum (2 = the app didn't answer in time).
    InteractionFailure { nonce: String, reason_code: Option<u64> },
    ModalCreate(crate::app_commands::Modal),
    Autocomplete { nonce: String, choices: Vec<crate::app_commands::Choice> },
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    token: String,
    commands: mpsc::UnboundedSender<Value>,
    // The voice engine's view of this gateway: it sends its own ops and sees VOICE_* / STREAM_* dispatches.
    pub voice: crate::voice::GatewayHandle,
}

// One runtime for the gateway, REST and QR sign-in; GPUI awaits its results on its own executor.
pub fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| Runtime::new().expect("tokio runtime"))
}

pub fn connect(token: String) -> (Client, mpsc::UnboundedReceiver<Event>) {
    let (commands, command_rx) = mpsc::unbounded();
    let (voice, voice_out) = crate::voice::GatewayHandle::new();
    let client = Client { http: reqwest::Client::new(), token: token.clone(), commands, voice: voice.clone() };
    let (tx, rx) = mpsc::unbounded();
    runtime().spawn(run_gateway(token, tx, Outbound { commands: command_rx, voice, voice_out }));
    (client, rx)
}

impl Client {
    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http.request(method, format!("{API}{path}")).header("Authorization", &self.token)
    }

    async fn run(&self, req: reqwest::RequestBuilder) -> Result<Value, String> {
        runtime().spawn(send(req)).await.map_err(|e| e.to_string())?
    }

    /// Sends a raw gateway payload (lazy member list, member requests, voice state updates).
    pub fn gateway(&self, payload: Value) {
        let _ = self.commands.unbounded_send(payload);
    }

    // Discord's REQUEST_CHANNEL_INFO (op 43): voice channel statuses and start times, answered by CHANNEL_INFO.
    pub fn request_channel_info(&self, guild_id: &str) {
        self.gateway(json!({ "op": 43, "d": { "guild_id": guild_id, "fields": ["status", "voice_start_time"] } }));
    }

    // Discord's REQUEST_SOUNDBOARD_SOUNDS (op 31): each server's soundboard, answered by SOUNDBOARD_SOUNDS.
    pub fn request_soundboard_sounds(&self, guild_ids: &[String]) {
        self.gateway(json!({ "op": 31, "d": { "guild_ids": guild_ids } }));
    }

    pub async fn default_soundboard_sounds(&self) -> Result<Vec<SoundboardSound>, String> {
        let body = self.run(self.request(reqwest::Method::GET, "/soundboard-default-sounds")).await?;
        Ok(body.as_array().into_iter().flatten().filter_map(SoundboardSound::parse).collect())
    }

    // Plays `sound` to everyone in the voice channel. Dropping the future cancels the request, as
    // Discord's client does when you leave the channel before it went out.
    pub async fn send_soundboard_sound(&self, channel_id: &str, sound: &SoundboardSound) -> Result<(), String> {
        let req = self.request(reqwest::Method::POST, &format!("/channels/{channel_id}/send-soundboard-sound")).json(&soundboard_play_body(sound));
        let mut send = AbortOnDrop(runtime().spawn(send(req)));
        (&mut send.0).await.map_err(|e| e.to_string())?.map(|_| ())
    }

    // The sound file from the CDN: an mp3 or ogg, which its URL doesn't say.
    pub async fn soundboard_file(&self, sound_id: &str) -> Result<bytes::Bytes, String> {
        let get = self.http.get(soundboard_url(sound_id));
        let mut fetch = AbortOnDrop(runtime().spawn(async move {
            let res = get.send().await?.error_for_status()?;
            res.bytes().await
        }));
        (&mut fetch.0).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())
    }

    pub fn request_member_list(&self, guild_id: &str, channel_id: &str) {
        self.gateway(json!({
            "op": 14,
            "d": { "guild_id": guild_id, "typing": true, "activities": true, "threads": true, "channels": { channel_id: [[0, 99]] } }
        }));
    }

    // Discord's REQUEST_FORUM_UNREADS (op 28): unread counts since each ack, answered by FORUM_UNREADS. Callers send at most 180.
    pub fn request_forum_unreads(&self, guild_id: &str, forum_id: &str, threads: &[(String, u64)]) {
        let threads: Vec<Value> = threads.iter().map(|(id, ack)| json!({ "thread_id": id, "ack_message_id": ack.to_string() })).collect();
        self.gateway(json!({ "op": 28, "d": { "guild_id": guild_id, "channel_id": forum_id, "threads": threads } }));
    }

    pub fn request_members(&self, guild_id: &str, user_ids: Vec<String>) {
        self.gateway(json!({ "op": 8, "d": { "guild_id": guild_id, "user_ids": user_ids, "presences": false } }));
    }

    // Members whose username or nickname starts with `query`; they arrive as GUILD_MEMBERS_CHUNK.
    // Not verified: a user account's op 8 is believed to accept `query` and `limit` as a bot's does.
    pub fn search_members(&self, guild_id: &str, query: &str, limit: u32) {
        self.gateway(json!({ "op": 8, "d": { "guild_id": guild_id, "query": query, "limit": limit, "presences": false } }));
    }

    /// Oldest first. `before` pages back through history; `around` loads the window a reply jumps to.
    pub async fn messages(&self, channel_id: &str, before: Option<&str>, around: Option<&str>) -> Result<Vec<Message>, String> {
        let mut path = format!("/channels/{channel_id}/messages?limit=50");
        if let Some(id) = before {
            path.push_str(&format!("&before={id}"));
        }
        if let Some(id) = around {
            path.push_str(&format!("&around={id}"));
        }
        let body = self.run(self.request(reqwest::Method::GET, &path)).await?;
        let mut messages: Vec<Message> = body.as_array().into_iter().flatten().filter_map(parse_message).collect();
        messages.reverse();
        Ok(messages)
    }

    /// `ping_author`: whether a reply mentions the person replied to (Discord's "@ ON / OFF").
    pub async fn send_message(&self, channel_id: &str, content: &str, nonce: &str, reply_to: Option<&str>, ping_author: bool, options: SendOptions) -> Result<Message, String> {
        let mut body = message_body(content, nonce, reply_to, ping_author, &[]);
        options.apply(&mut body);
        let req = self.request(reqwest::Method::POST, &format!("/channels/{channel_id}/messages")).json(&body);
        self.run(req).await.and_then(|v| parse_message(&v).ok_or_else(|| "Unexpected response".into()))
    }

    /// Forwards `original` into `channel_id`: a message that is only a type-1 reference to it. `guild_id` is
    /// the original's server, which history loaded over REST doesn't carry on the message.
    pub async fn forward_message(&self, channel_id: &str, original: &Message, guild_id: Option<&str>, nonce: &str) -> Result<Message, String> {
        let body = forward_body(original, guild_id, nonce);
        let req = self.request(reqwest::Method::POST, &format!("/channels/{channel_id}/messages")).json(&body);
        self.run(req).await.and_then(|v| parse_message(&v).ok_or_else(|| "Unexpected response".into()))
    }

    /// A message that is one sticker, as Discord's picker sends it: no text.
    pub async fn send_sticker(&self, channel_id: &str, sticker_id: &str, nonce: &str, reply_to: Option<&str>, ping_author: bool) -> Result<Message, String> {
        let mut body = message_body("", nonce, reply_to, ping_author, &[]);
        body["sticker_ids"] = json!([sticker_id]);
        let req = self.request(reqwest::Method::POST, &format!("/channels/{channel_id}/messages")).json(&body);
        self.run(req).await.and_then(|v| parse_message(&v).ok_or_else(|| "Unexpected response".into()))
    }

    /// Discord's "Wave to say hi!": a sticker sent as a reply to a join notice, pinging the joiner.
    pub async fn greet(&self, channel_id: &str, guild_id: &str, message_id: &str, sticker_id: &str) -> Result<Message, String> {
        let body = json!({ "sticker_ids": [sticker_id], "message_reference": { "guild_id": guild_id, "channel_id": channel_id, "message_id": message_id } });
        let req = self.request(reqwest::Method::POST, &format!("/channels/{channel_id}/greet")).json(&body);
        self.run(req).await.and_then(|v| parse_message(&v).ok_or_else(|| "Unexpected response".into()))
    }

    /// A message that is one poll; `layout_type` 1 is Discord's only layout.
    pub async fn send_poll(&self, channel_id: &str, poll: &PollDraft, nonce: &str) -> Result<Message, String> {
        let mut body = message_body("", nonce, None, false, &[]);
        let answers: Vec<Value> = poll
            .answers
            .iter()
            .map(|(text, emoji)| match emoji {
                // Only one field: the id for a server's emoji, the character for a Unicode one.
                Some(Emoji { id: Some(id), .. }) => json!({ "poll_media": { "text": text, "emoji": { "id": id } } }),
                Some(e) => json!({ "poll_media": { "text": text, "emoji": { "name": e.name } } }),
                None => json!({ "poll_media": { "text": text } }),
            })
            .collect();
        body["poll"] = json!({ "question": { "text": poll.question }, "answers": answers, "duration": poll.hours, "allow_multiselect": poll.multiselect, "layout_type": 1 });
        let req = self.request(reqwest::Method::POST, &format!("/channels/{channel_id}/messages")).json(&body);
        self.run(req).await.and_then(|v| parse_message(&v).ok_or_else(|| "Unexpected response".into()))
    }

    /// The server's index (a DM's comes from the channel) merged with your user-installed apps' index,
    /// as Discord's client merges them. Either alone still lists; both failing is the context's error.
    pub async fn command_index(&self, channel_id: &str, guild_id: Option<&str>) -> Result<crate::app_commands::CommandIndex, String> {
        use crate::app_commands::{CommandIndex, parse_index};
        let path = match guild_id {
            Some(g) => format!("/guilds/{g}/application-command-index"),
            None => format!("/channels/{channel_id}/application-command-index"),
        };
        let context = runtime().spawn(fetch_index(self.request(reqwest::Method::GET, &path)));
        let user = runtime().spawn(fetch_index(self.request(reqwest::Method::GET, "/users/@me/application-command-index")));
        let (context, user) = futures::join!(context, user);
        let (context, user) = (context.map_err(|e| e.to_string())?.map(|v| parse_index(&v)), user.map_err(|e| e.to_string())?.map(|v| parse_index(&v)));
        match (context, user) {
            (Ok(context), Ok(user)) => Ok(CommandIndex::merge(context, user)),
            (Ok(only), Err(_)) | (Err(_), Ok(only)) => Ok(only),
            (Err(error), Err(_)) => Err(error),
        }
    }

    /// Answers 204; what happens next arrives as INTERACTION_* dispatches matched by the body's nonce.
    /// No retries: a refusal is the interaction's failure. Component presses and autocomplete give up
    /// after 3 s, as Discord's client does; commands and modal submits wait.
    pub async fn interact(&self, body: Value) -> Result<(), InteractError> {
        let mut req = self.request(reqwest::Method::POST, "/interactions").json(&body);
        if matches!(body["type"].as_u64(), Some(3 | 4)) {
            req = req.timeout(Duration::from_secs(3));
        }
        runtime().spawn(send_interaction(req)).await.unwrap_or_else(|_| Err(InteractError::default()))
    }

    /// Uploads files for an app command's ATTACHMENT options or a form's file fields the way messages do (Cloud Uploads): a slot
    /// per file, a PUT of each, and the uploaded names to pass to `app_commands::attach_files`.
    pub async fn stage_files(&self, channel_id: &str, files: &[Upload]) -> Result<Vec<crate::app_commands::CommandAttachment>, String> {
        let uploaded = self.put_uploads(channel_id, files).await?;
        Ok(uploaded
            .into_iter()
            .map(|a| crate::app_commands::CommandAttachment { spoiler: a.spoiler || a.filename.starts_with("SPOILER_"), filename: a.filename, uploaded_filename: a.uploaded_filename })
            .collect())
    }

    // A slot per file, an abortable PUT of each, and each file as Create Message names it.
    async fn put_uploads(&self, channel_id: &str, files: &[Upload]) -> Result<Vec<SentAttachment>, String> {
        let req = self.request(reqwest::Method::POST, &format!("/channels/{channel_id}/attachments")).json(&attachments_request(files));
        let slots = self.run(req).await?;
        let slots = parse_upload_slots(&slots, files.len())?;
        for (file, (url, _)) in files.iter().zip(&slots) {
            // Cancel drops this future; the guard stops the PUT with it.
            let mut put = AbortOnDrop(runtime().spawn(put_file(self.http.clone(), url.clone(), file.path.clone(), file.sent.clone())));
            (&mut put.0).await.map_err(|e| e.to_string())??;
        }
        Ok(files
            .iter()
            .zip(slots)
            .map(|(f, (_, uploaded_filename))| SentAttachment { filename: f.name.clone(), uploaded_filename, description: f.description.clone(), spoiler: f.spoiler })
            .collect())
    }

    /// The FRECENCY_AND_FAVORITES settings proto (type 2) as bytes: `GET /users/@me/settings-proto/2`
    /// answers `{settings: base64}`, as Discord's client reads it (`web#594061`).
    pub async fn frecency_settings(&self) -> Result<Vec<u8>, String> {
        use base64::Engine;
        let body = self.run(self.request(reqwest::Method::GET, "/users/@me/settings-proto/2")).await?;
        let settings = body["settings"].as_str().ok_or("No settings in the answer")?;
        base64::engine::general_purpose::STANDARD.decode(settings).map_err(|e| e.to_string())
    }

    /// Discord's own packs, for everyone.
    pub async fn sticker_packs(&self) -> Result<Vec<StickerPack>, String> {
        let body = self.run(self.request(reqwest::Method::GET, "/sticker-packs")).await?;
        Ok(body["sticker_packs"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| StickerPack { name: p["name"].as_str().unwrap_or_default().to_owned(), stickers: p["stickers"].as_array().into_iter().flatten().filter_map(Sticker::parse).collect() })
            .filter(|p| !p.stickers.is_empty())
            .collect())
    }

    /// One message carrying all the files, as Discord's own client sends it (docs.discord.food, Cloud
    /// Uploads): upload URLs for every file, a PUT of each to its URL, then the message naming them.
    /// Each file's `sent` counts its bytes as they go out.
    /// `reply`: the message replied to, and whether the reply pings its author.
    pub async fn upload(&self, channel_id: &str, files: &[Upload], content: &str, nonce: &str, reply: Option<(&str, bool)>, options: SendOptions) -> Result<Message, String> {
        let uploaded = self.put_uploads(channel_id, files).await?;
        let mut body = message_body(content, nonce, reply.map(|(id, _)| id), reply.is_some_and(|(_, ping)| ping), &uploaded);
        options.apply(&mut body);
        let req = self.request(reqwest::Method::POST, &format!("/channels/{channel_id}/messages")).json(&body);
        self.run(req).await.and_then(|v| parse_message(&v).ok_or_else(|| "Unexpected response".into()))
    }

    /// Recent messages that mention you, across servers (the title bar's Inbox).
    /// Recent mentions of you; `everyone` and `roles` include @everyone/@here and role mentions, `guild_id`
    /// keeps one server's.
    pub async fn mentions(&self, everyone: bool, roles: bool, guild_id: Option<&str>) -> Result<Vec<Message>, String> {
        let mut path = format!("/users/@me/mentions?limit=25&roles={roles}&everyone={everyone}");
        if let Some(guild) = guild_id {
            path.push_str(&format!("&guild_id={guild}"));
        }
        let body = self.run(self.request(reqwest::Method::GET, &path)).await?;
        Ok(body.as_array().into_iter().flatten().filter_map(parse_message).collect())
    }

    /// One of your settings-proto blobs (base64), by Discord's settings type.
    pub async fn settings_proto(&self, kind: u8) -> Result<String, String> {
        let body = self.run(self.request(reqwest::Method::GET, &format!("/users/@me/settings-proto/{kind}"))).await?;
        body["settings"].as_str().map(str::to_owned).ok_or_else(|| "Unexpected response".into())
    }

    /// Merges `settings` (base64, only the fields to change) into a settings-proto blob.
    pub async fn patch_settings_proto(&self, kind: u8, settings: String) -> Result<(), String> {
        let req = self.request(reqwest::Method::PATCH, &format!("/users/@me/settings-proto/{kind}")).json(&serde_json::json!({ "settings": settings }));
        self.run(req).await.map(drop)
    }

    /// Your saved-for-later messages, newest saved first (Discord's bookmarks, 25 a page).
    pub async fn saved_messages(&self) -> Result<Vec<Message>, String> {
        let body = self.run(self.request(reqwest::Method::GET, "/users/@me/saved-messages/bookmarks?limit=25")).await?;
        Ok(body["results"].as_array().into_iter().flatten().filter_map(|r| parse_message(&r["message"])).collect())
    }

    /// The ids of the messages you saved, as the client loads them on connect.
    pub async fn saved_message_ids(&self) -> Result<Vec<String>, String> {
        let body = self.run(self.request(reqwest::Method::GET, "/users/@me/saved-messages?bookmark_ids=true")).await?;
        Ok(bookmark_ids(&body["bookmark_ids"]))
    }

    /// Discord's GIF search; an empty query returns what's trending. Shapes from memory: Not verified.
    pub async fn gifs(&self, query: &str) -> Result<Vec<Gif>, String> {
        let mut url = reqwest::Url::parse(&format!("{API}/gifs/{}", if query.is_empty() { "trending-gifs" } else { "search" })).map_err(|e| e.to_string())?;
        url.query_pairs_mut().append_pair("provider", "tenor").append_pair("locale", "en-US").append_pair("media_format", "gif");
        if !query.is_empty() {
            url.query_pairs_mut().append_pair("q", query);
        }
        let body = self.run(self.http.get(url).header("Authorization", &self.token)).await?;
        Ok(body
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|g| {
                Some(Gif {
                    url: g["url"].as_str()?.to_owned(),
                    preview: g["gif_src"].as_str().or(g["src"].as_str())?.to_owned(),
                    width: g["width"].as_f64().unwrap_or(200.) as f32,
                    height: g["height"].as_f64().unwrap_or(150.) as f32,
                })
            })
            .collect())
    }

    /// The member list's Activity feed: everyone this account knows, not one server's; the panel keeps
    /// the entries whose author is in the open server.
    pub async fn activity_feed(&self) -> Result<Vec<FeedEntry>, String> {
        let body = self.run(self.request(reqwest::Method::GET, "/content-inventory/users/@me")).await?;
        Ok(parse_activity_feed(&body))
    }

    /// Newest pin first. Discord moved pins to `/messages/pins`, which wraps each as `{ pinned_at, message }`;
    /// the old `/pins` route is deprecated.
    // ponytail: first page only (50); page with `before` = the last `pinned_at` if a channel needs more.
    pub async fn pins(&self, channel_id: &str) -> Result<Vec<Message>, String> {
        let body = self.run(self.request(reqwest::Method::GET, &format!("/channels/{channel_id}/messages/pins?limit=50"))).await?;
        Ok(parse_pins(&body))
    }

    pub async fn edit_message(&self, channel_id: &str, id: &str, content: &str) -> Result<(), String> {
        let req = self.request(reqwest::Method::PATCH, &format!("/channels/{channel_id}/messages/{id}")).json(&json!({ "content": content }));
        self.run(req).await.map(|_| ())
    }

    // `PUT /channels/{id}/voice-status`, 204 on success; "" clears it. Discord's refusal comes back as its message.
    pub async fn set_voice_channel_status(&self, channel_id: &str, status: &str) -> Result<(), String> {
        let req = self.request(reqwest::Method::PUT, &format!("/channels/{channel_id}/voice-status")).json(&json!({ "status": status }));
        self.run(req).await.map(|_| ())
    }

    pub async fn delete_message(&self, channel_id: &str, id: &str) -> Result<(), String> {
        self.run(self.request(reqwest::Method::DELETE, &format!("/channels/{channel_id}/messages/{id}"))).await.map(|_| ())
    }

    pub async fn react(&self, channel_id: &str, message_id: &str, emoji: &Emoji, add: bool) -> Result<(), String> {
        let mut url = reqwest::Url::parse(&format!("{API}/channels/{channel_id}/messages/{message_id}/reactions")).map_err(|e| e.to_string())?;
        // path_segments_mut percent-encodes the emoji, which unicode reactions need.
        url.path_segments_mut().map_err(|_| "Bad URL")?.push(&emoji.key()).push("@me");
        let method = if add { reqwest::Method::PUT } else { reqwest::Method::DELETE };
        self.run(self.http.request(method, url).header("Authorization", &self.token)).await.map(|_| ())
    }

    /// One page of who reacted with `emoji`, in reaction order, starting after the user id `after`.
    pub async fn reactors(&self, channel_id: &str, message_id: &str, emoji: &Emoji, after: Option<&str>, limit: usize) -> Result<Vec<Reactor>, String> {
        let after = after.map(|id| format!("&after={id}")).unwrap_or_default();
        let path = format!("/channels/{channel_id}/messages/{message_id}/reactions/{}?limit={limit}{after}", emoji.path());
        let body = self.run(self.request(reqwest::Method::GET, &path)).await?;
        let reactor = |v: &Value| Reactor { user: parse_user(v), username: v["username"].as_str().unwrap_or_default().to_owned() };
        Ok(body.as_array().map(|users| users.iter().map(reactor).collect()).unwrap_or_default())
    }

    pub async fn typing(&self, channel_id: &str) {
        let _ = self.run(self.request(reqwest::Method::POST, &format!("/channels/{channel_id}/typing"))).await;
    }

    pub async fn ack(&self, channel_id: &str, message_id: &str) {
        let req = self.request(reqwest::Method::POST, &format!("/channels/{channel_id}/messages/{message_id}/ack")).json(&json!({ "token": null }));
        let _ = self.run(req).await;
    }

    /// Discord's Mark Unread: acks `ack_id`, just before the message marked, with the mentions after it.
    /// The gateway echoes it to every device as a manual MESSAGE_ACK.
    pub async fn ack_manual(&self, channel_id: &str, ack_id: u64, mention_count: u32) {
        let req = self.request(reqwest::Method::POST, &format!("/channels/{channel_id}/messages/{ack_id}/ack")).json(&json!({ "manual": true, "mention_count": mention_count }));
        let _ = self.run(req).await;
    }

    /// Clears one mention from Recent Mentions on every device; the channel's read state stays.
    pub async fn delete_mention(&self, message_id: &str) -> Result<(), String> {
        self.run(self.request(reqwest::Method::DELETE, &format!("/users/@me/mentions/{message_id}"))).await.map(|_| ())
    }

    pub async fn profile(&self, user_id: &str, guild_id: Option<&str>) -> Result<Profile, String> {
        let mut path = format!("/users/{user_id}/profile?with_mutual_guilds=true&with_mutual_friends=true");
        if let Some(guild) = guild_id {
            path.push_str(&format!("&guild_id={guild}"));
        }
        let body = self.run(self.request(reqwest::Method::GET, &path)).await?;
        Ok(parse_profile(user_id, &body))
    }

    // A profile effect's layers, from the collectibles product its sku names.
    pub async fn profile_effect(&self, sku: &str) -> Result<Vec<EffectLayer>, String> {
        let body = self.run(self.request(reqwest::Method::GET, &format!("/collectibles-products/{sku}"))).await?;
        Ok(parse_profile_effect(&body))
    }

    /// Opens (or finds) the DM channel with a user.
    pub async fn open_dm(&self, user_id: &str) -> Result<Channel, String> {
        self.open_private(&[user_id.to_owned()]).await
    }

    /// The DM with one recipient, or a new group DM with several (type 3 in the answer).
    pub async fn open_private(&self, recipients: &[String]) -> Result<Channel, String> {
        let req = self.request(reqwest::Method::POST, "/users/@me/channels").json(&json!({ "recipients": recipients }));
        let body = self.run(req).await?;
        parse_dm(&body, &Value::Null).ok_or_else(|| "Unexpected response".into())
    }

    /// Changes your status everywhere: the gateway presence now, the saved setting for next time.
    /// `custom` rides along so the presence update doesn't wipe your custom status.
    pub async fn set_status(&self, status: Status, custom: Option<&CustomStatus>, game: Option<&crate::games::Game>) {
        self.send_presence(status, custom, game);
        let req = self.request(reqwest::Method::PATCH, "/users/@me/settings").json(&json!({ "status": status_wire(status) }));
        let _ = self.run(req).await;
    }

    // A custom status travels as a type-4 activity in the presence, the game you're playing as type 0.
    pub fn send_presence(&self, status: Status, custom: Option<&CustomStatus>, game: Option<&crate::games::Game>) {
        let activities: Vec<Value> = custom.map(CustomStatus::activity).into_iter().chain(game.map(crate::games::Game::activity)).collect();
        self.gateway(json!({ "op": 3, "d": { "status": status_wire(status), "since": 0, "activities": activities, "afk": false } }));
    }

    /// An application's icon hash; `/rpc` is the public subset of the application, readable by anyone.
    pub async fn app_icon(&self, app_id: &str) -> Result<Option<String>, String> {
        let body = self.run(self.request(reqwest::Method::GET, &format!("/applications/{app_id}/rpc"))).await?;
        Ok(body["icon"].as_str().map(str::to_owned))
    }

    /// Saves account settings: `patch` holds only the fields that changed.
    // Not verified: the legacy endpoint still accepting each field Settings sends (it accepts `status`).
    pub async fn set_account_settings(&self, patch: Value) -> Result<(), String> {
        let req = self.request(reqwest::Method::PATCH, "/users/@me/settings").json(&patch);
        self.run(req).await.map(|_| ())
    }

    /// Changes your account (PATCH /users/@me): display name, username (with `password`), avatar, banner.
    /// Images go as data URIs. Returns the updated user.
    pub async fn modify_me(&self, patch: Value) -> Result<Value, String> {
        let req = self.request(reqwest::Method::PATCH, "/users/@me").json(&patch);
        self.run(req).await
    }

    /// Your profile as its editor loads it, raw: `GET /users/{id}/profile`, with the server profile when `guild_id` is set.
    pub async fn my_profile(&self, user_id: &str, guild_id: Option<&str>) -> Result<Value, String> {
        let guild = guild_id.map(|g| format!("&guild_id={g}")).unwrap_or_default();
        self.run(self.request(reqwest::Method::GET, &format!("/users/{user_id}/profile?with_mutual_guilds=false{guild}"))).await
    }

    /// Saves your main profile (PATCH /users/@me/profile): `bio`, `pronouns`, `accent_color`. Returns the profile.
    pub async fn modify_my_profile(&self, patch: Value) -> Result<Value, String> {
        self.run(self.request(reqwest::Method::PATCH, "/users/@me/profile").json(&patch)).await
    }

    /// Saves your profile on one server (PATCH /guilds/{id}/profile/@me): `pronouns`, and for Nitro `bio` and `banner`.
    pub async fn modify_guild_profile(&self, guild_id: &str, patch: Value) -> Result<Value, String> {
        self.run(self.request(reqwest::Method::PATCH, &format!("/guilds/{guild_id}/profile/@me")).json(&patch)).await
    }

    /// Saves your member entry on one server (PATCH /guilds/{id}/members/@me): `nick`, and for Nitro the server avatar.
    pub async fn modify_my_member(&self, guild_id: &str, patch: Value) -> Result<Value, String> {
        self.run(self.request(reqwest::Method::PATCH, &format!("/guilds/{guild_id}/members/@me")).json(&patch)).await
    }

    /// Wears a server's tag next to your name, or none.
    pub async fn set_server_tag(&self, guild_id: Option<&str>) -> Result<Value, String> {
        let body = json!({ "identity_guild_id": guild_id, "identity_enabled": guild_id.is_some() });
        self.run(self.request(reqwest::Method::PUT, "/users/@me/clan").json(&body)).await
    }

    /// A server's public profile, which carries its tag and badge.
    // Not verified: `GET /guilds/{id}/profile` and its `tag` / `badge` fields.
    pub async fn guild_profile(&self, guild_id: &str) -> Result<Value, String> {
        self.run(self.request(reqwest::Method::GET, &format!("/guilds/{guild_id}/profile"))).await
    }

    /// Your last avatars: `{avatars: [{id, storage_hash, description}]}`.
    pub async fn recent_avatars(&self) -> Result<Value, String> {
        self.run(self.request(reqwest::Method::GET, "/users/@me/avatars")).await
    }

    pub async fn delete_recent_avatar(&self, avatar_id: &str) -> Result<(), String> {
        self.run(self.request(reqwest::Method::DELETE, &format!("/users/@me/avatars/{avatar_id}"))).await.map(|_| ())
    }

    /// Saves the rail's order and folders. Loose servers are folders without an id.
    // Not verified: the legacy settings endpoint still accepting `guild_folders` (it accepts `status`).
    pub async fn set_guild_folders(&self, folders: Vec<Folder>) {
        let wire: Vec<Value> = folders
            .iter()
            .map(|f| json!({ "id": f.id.parse::<u64>().ok(), "name": f.name, "color": f.color, "guild_ids": f.guild_ids }))
            .collect();
        let positions: Vec<&String> = folders.iter().flat_map(|f| &f.guild_ids).collect();
        let req = self.request(reqwest::Method::PATCH, "/users/@me/settings").json(&json!({ "guild_folders": wire, "guild_positions": positions }));
        let _ = self.run(req).await;
    }

    /// Any REST call the menus make: moderation, relationships, notes. `body` None sends no JSON.
    pub async fn call(&self, method: reqwest::Method, path: String, body: Option<Value>) -> Result<Value, String> {
        let req = self.request(method, &path);
        let req = match body {
            Some(body) => req.json(&body),
            None => req,
        };
        self.run(req).await
    }

    /// A call whose failure the caller reads itself: Discord's status and body as sent, with none of `call`'s
    /// retries (a publish's 429 carries the minutes to wait in `retry_after`).
    pub async fn call_raw(&self, method: reqwest::Method, path: String, body: Option<Value>) -> Result<(u16, Value), String> {
        let req = self.request(method, &path);
        let req = match body {
            Some(body) => req.json(&body),
            None => req,
        };
        let sent = async move {
            let res = req.send().await.map_err(|e| e.to_string())?;
            let status = res.status().as_u16();
            Ok((status, res.json().await.unwrap_or(Value::Null)))
        };
        runtime().spawn(sent).await.map_err(|e| e.to_string())?
    }

    /// The message a link points at, through the history endpoint (`around`, one message), as Discord's
    /// client does. None: it's gone, or the channel isn't one you can read (403, or no history access, which
    /// returns an empty list).
    pub async fn linked_message(&self, channel_id: &str, message_id: &str) -> Result<Option<Message>, String> {
        let req = self.request(reqwest::Method::GET, &format!("/channels/{channel_id}/messages?around={message_id}&limit=1"));
        let body = runtime().spawn(send_or_gone(req)).await.map_err(|e| e.to_string())??;
        Ok(body.and_then(|b| b.as_array()?.iter().filter_map(parse_message).find(|m| m.id == message_id)))
    }

    /// An invite's server card. None: the code is unknown or expired (404).
    pub async fn invite(&self, code: &str) -> Result<Option<Invite>, String> {
        let req = self.request(reqwest::Method::GET, &format!("/invites/{code}?with_counts=true"));
        let body = runtime().spawn(send_or_gone(req)).await.map_err(|e| e.to_string())??;
        Ok(body.as_ref().and_then(parse_invite))
    }

    /// Joins the invite's server (`POST /invites/{code}`) and returns its id; already a member succeeds too.
    /// Discord usually wants a captcha first (see `call_with_captcha`).
    pub async fn join_invite(&self, code: &str, captcha_url: mpsc::UnboundedSender<String>) -> Result<String, String> {
        let body = self.call_with_captcha(reqwest::Method::POST, format!("/invites/{code}"), Some(json!({})), captcha_url).await?;
        joined_guild(&body).ok_or_else(|| "That invite isn't to a server".into())
    }

    /// A call Discord may answer with a captcha (joining a server, sending a friend request): its page goes
    /// out on `captcha_url` for the browser, and the call is retried with the solved key, as the sign-in
    /// does (`auth::exchange_ticket`). Other failures read as `send` words them.
    pub async fn call_with_captcha(&self, method: reqwest::Method, path: String, body: Option<Value>, captcha_url: mpsc::UnboundedSender<String>) -> Result<Value, ApiError> {
        let client = self.clone();
        runtime().spawn(async move { client.captcha_loop(method, &path, body, captcha_url).await }).await.map_err(|e| ApiError::from(e.to_string()))?
    }

    async fn captcha_loop(&self, method: reqwest::Method, path: &str, body: Option<Value>, captcha_url: mpsc::UnboundedSender<String>) -> Result<Value, ApiError> {
        let mut solved: Option<(crate::captcha::Challenge, String)> = None;
        for _ in 0..3 {
            let mut req = self.request(method.clone(), path);
            if let Some(body) = &body {
                req = req.json(body);
            }
            if let Some((challenge, answer)) = &solved {
                for (name, value) in challenge.headers(answer) {
                    req = req.header(name, value);
                }
            }
            let res = req.send().await.map_err(|e| e.to_string())?;
            let status = res.status();
            let body: Value = res.json().await.unwrap_or(Value::Null);
            if status.is_success() {
                return Ok(body);
            }
            let Some(challenge) = crate::captcha::Challenge::from_response(&body) else {
                eprintln!("http: {status} {path} (code {}): {body}", body["code"]);
                return Err(ApiError { status: status.as_u16(), code: body["code"].as_u64(), text: error_text(status.as_u16(), &body) });
            };
            let pending = crate::captcha::serve(&challenge).await?;
            let _ = captcha_url.unbounded_send(pending.url.clone());
            solved = Some((challenge, pending.solved().await?));
        }
        Err(ApiError::from("Discord kept asking for a captcha".to_owned()))
    }

    /// Marks several channels read at once (a server's "Mark As Read").
    // A form upload with one file, as sticker creation takes it.
    pub async fn call_form(&self, path: String, fields: Vec<(&'static str, String)>, file_name: String, bytes: Vec<u8>) -> Result<Value, String> {
        let form = fields.into_iter().fold(reqwest::multipart::Form::new(), |form, (key, value)| form.text(key, value));
        let form = form.part("file", reqwest::multipart::Part::bytes(bytes).file_name(file_name));
        self.run(self.request(reqwest::Method::POST, &path).multipart(form)).await
    }

    // A JSON body with files, as `payload_json` and `files[n]` parts; the body names each by its index.
    pub async fn call_files(&self, path: String, payload: Value, files: Vec<Upload>) -> Result<Value, String> {
        let req = self.request(reqwest::Method::POST, &path);
        let sent = async move {
            let mut form = reqwest::multipart::Form::new().text("payload_json", payload.to_string());
            for (i, file) in files.into_iter().enumerate() {
                let bytes = std::fs::read(&file.path).map_err(|e| format!("{}: {e}", file.name))?;
                form = form.part(format!("files[{i}]"), reqwest::multipart::Part::bytes(bytes).file_name(file.name));
            }
            send(req.multipart(form)).await
        };
        runtime().spawn(sent).await.map_err(|e| e.to_string())?
    }

    pub async fn ack_bulk(&self, reads: Vec<(String, u64)>) {
        let read_states: Vec<Value> = reads.into_iter().map(|(channel, message)| json!({ "channel_id": channel, "message_id": message.to_string(), "read_state_type": 0 })).collect();
        let req = self.request(reqwest::Method::POST, "/read-states/ack-bulk").json(&json!({ "read_states": read_states }));
        let _ = self.run(req).await;
    }

    /// `scope` is `guilds/{id}` or, for a DM, `channels/{id}`. `params` are Discord's search filters, the
    /// page and the sort (docs.discord.food, Search Guild Messages): 25 hits a page, `offset` in hits.
    pub async fn search(&self, scope: &str, content: &str, params: &[(String, String)]) -> Result<SearchPage, String> {
        let mut url = reqwest::Url::parse(&format!("{API}/{scope}/messages/search")).map_err(|e| e.to_string())?;
        {
            let mut pairs = url.query_pairs_mut();
            if !content.is_empty() {
                pairs.append_pair("content", content);
            }
            for (name, value) in params {
                pairs.append_pair(name, value);
            }
        }
        let body = self.run(self.http.get(url).header("Authorization", &self.token)).await?;
        // Each hit is a small array of messages around the match; the match is the one marked `hit`.
        let messages = body["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|around| around.as_array().and_then(|a| a.iter().find(|m| m["hit"].as_bool() == Some(true)).or_else(|| a.first())).and_then(parse_message))
            .collect();
        Ok(SearchPage { messages, total: body["total_results"].as_u64().unwrap_or(0) as u32 })
    }
}

#[derive(Clone, Debug, Default)]
pub struct SearchPage {
    pub messages: Vec<Message>,
    pub total: u32,
}

// Native GPUI ships no HTTP client (only its web build has one), so `img(url)` loads nothing
// until the app installs one. GET-only: it serves avatars, icons and attachments from the CDN.
#[derive(Default)]
pub struct Images(reqwest::Client);

// Image files as downloaded (compressed), most recently used last, within `CAP` bytes. The image
// cache frees decoded images that leave the screen; this lets them come back without the network.
pub mod downloads {
    use std::{collections::VecDeque, sync::Mutex};

    const CAP: usize = 32 << 20;

    static FILES: Mutex<VecDeque<(String, bytes::Bytes)>> = Mutex::new(VecDeque::new());

    pub fn get(url: &str) -> Option<bytes::Bytes> {
        let mut files = FILES.lock().ok()?;
        let index = files.iter().position(|(u, _)| u == url)?;
        let entry = files.remove(index)?;
        let bytes = entry.1.clone();
        files.push_back(entry);
        Some(bytes)
    }

    pub fn put(url: String, bytes: bytes::Bytes) {
        let Ok(mut files) = FILES.lock() else { return };
        files.retain(|(u, _)| *u != url);
        files.push_back((url, bytes));
        while files.iter().map(|(_, b)| b.len()).sum::<usize>() > CAP && files.len() > 1 {
            files.pop_front();
        }
    }

    pub fn bytes() -> usize {
        FILES.lock().map(|f| f.iter().map(|(_, b)| b.len()).sum()).unwrap_or(0)
    }
}

impl gpui_kit::http_client::HttpClient for Images {
    fn user_agent(&self) -> Option<&gpui_kit::http_client::http::HeaderValue> {
        None
    }

    fn proxy(&self) -> Option<&gpui_kit::http_client::Url> {
        None
    }

    fn send(
        &self,
        req: gpui_kit::http_client::Request<gpui_kit::http_client::AsyncBody>,
    ) -> futures::future::BoxFuture<'static, gpui_kit::http_client::Result<gpui_kit::http_client::Response<gpui_kit::http_client::AsyncBody>>> {
        let url = req.uri().to_string();
        if let Some(bytes) = downloads::get(&url).or_else(|| demo_media(&url)) {
            return Box::pin(async move { Ok(gpui_kit::http_client::Response::builder().status(200).body(bytes.to_vec().into())?) });
        }
        let get = self.0.get(&url);
        Box::pin(async move {
            // gpui drops this future when the image cache lets go of a load; the download stops with it.
            let mut fetch = AbortOnDrop(runtime().spawn(async move {
                let res = get.send().await?;
                Ok::<_, reqwest::Error>((res.status().as_u16(), res.bytes().await?))
            }));
            let (status, bytes) = (&mut fetch.0).await??;
            if cfg!(debug_assertions) && std::env::var_os("WAFFLE_IMAGE_DEBUG").is_some() && status >= 400 {
                eprintln!("image {status} {url}");
            }
            if status == 200 {
                downloads::put(url, bytes.clone());
            }
            Ok(gpui_kit::http_client::Response::builder().status(status).body(bytes.to_vec().into())?)
        })
    }
}

struct AbortOnDrop<T>(tokio::task::JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

// Made-up media a WAFFLE_DEMO scene serves in place of the network (debug builds only).
pub static DEMO_MEDIA: OnceLock<fn(&str) -> Option<bytes::Bytes>> = OnceLock::new();

pub fn demo_media(url: &str) -> Option<bytes::Bytes> {
    if !cfg!(debug_assertions) {
        return None;
    }
    DEMO_MEDIA.get()?(url)
}

// Discord's HTTP failures (docs.discord.com, Opcodes and Status Codes, HTTP): 429 waits out
// `retry_after` and tries again; a 5xx is retried once, only for methods that are safe to repeat, so a
// message never sends twice; 401 means the token is no longer accepted. Every other failure shows
// Discord's own message (its JSON `message`, field errors included).
pub async fn send(req: reqwest::RequestBuilder) -> Result<Value, String> {
    let idempotent = req.try_clone().and_then(|r| r.build().ok()).is_some_and(|r| matches!(*r.method(), reqwest::Method::GET | reqwest::Method::PUT | reqwest::Method::DELETE | reqwest::Method::HEAD));
    let mut req = req;
    let mut retries = 0;
    loop {
        // None for a streamed body (uploads): those aren't retried.
        let again = req.try_clone();
        let res = req.send().await.map_err(|e| e.to_string())?;
        let status = res.status();
        let url = res.url().path().to_owned();
        let header_wait = res.headers().get(reqwest::header::RETRY_AFTER).and_then(|v| v.to_str().ok()?.parse::<f64>().ok());
        let body: Value = res.json().await.unwrap_or(Value::Null);
        if status.is_success() {
            return Ok(body);
        }
        if let (Some(wait), Some(next)) = (retry_wait(status.as_u16(), &body, header_wait, idempotent, retries), again) {
            eprintln!("http: {status} {url}, retrying in {wait:?}");
            tokio::time::sleep(wait).await;
            req = next;
            retries += 1;
            continue;
        }
        eprintln!("http: {status} {url} (code {}): {body}", body["code"]);
        return Err(error_text(status.as_u16(), &body));
    }
}

// Longer than this and the caller hears "rate limited" instead of a frozen action.
const MAX_RETRY_WAIT: f64 = 10.;

fn retry_wait(status: u16, body: &Value, header_wait: Option<f64>, idempotent: bool, retries: u32) -> Option<Duration> {
    match status {
        429 if retries < 2 => {
            let secs = body["retry_after"].as_f64().or(header_wait).unwrap_or(1.);
            (secs <= MAX_RETRY_WAIT).then(|| Duration::from_secs_f64(secs.max(0.)))
        }
        500 | 502 | 503 | 504 if idempotent && retries == 0 => Some(Duration::from_secs(1)),
        _ => None,
    }
}

// Discord's client tries a command index this many times before it gives up.
const INDEX_TRIES: u32 = 3;

// A command index GET: 202 (Discord still building it) waits 5 s, 429 waits `retry_after`.
async fn fetch_index(req: reqwest::RequestBuilder) -> Result<Value, String> {
    let mut req = req;
    let mut tries = 1;
    loop {
        let again = req.try_clone();
        let res = req.send().await.map_err(|e| e.to_string())?;
        let status = res.status().as_u16();
        let header_wait = res.headers().get(reqwest::header::RETRY_AFTER).and_then(|v| v.to_str().ok()?.parse::<f64>().ok());
        let body: Value = res.json().await.unwrap_or(Value::Null);
        if let (Some(wait), Some(next)) = (index_wait(status, &body, header_wait).filter(|_| tries < INDEX_TRIES), again) {
            tokio::time::sleep(wait).await;
            req = next;
            tries += 1;
            continue;
        }
        return match status {
            // Not Discord's wording: its client shows no text for an index that never finished building.
            202 => Err("Commands are still loading. Try again in a moment.".into()),
            200..=299 => Ok(body),
            _ => Err(error_text(status, &body)),
        };
    }
}

fn index_wait(status: u16, body: &Value, header_wait: Option<f64>) -> Option<Duration> {
    match status {
        202 => Some(Duration::from_secs(5)),
        429 => Some(Duration::from_secs_f64(body["retry_after"].as_f64().or(header_wait).unwrap_or(1.).max(0.))),
        _ => None,
    }
}

/// Why `POST /interactions` was refused, as Discord's client reads it (`web#964363`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InteractError {
    pub status: Option<u16>,
    // Discord's error code (`body.code`), its `errorCode`.
    pub code: Option<u64>,
    // The server's words, shown on a command's stand-in.
    pub message: Option<String>,
    // The command index is out of date: refetch it before the next command.
    pub stale_index: bool,
    // Seconds, on a 429.
    pub retry_after: Option<f64>,
}

const INVALID_FORM_BODY: u64 = 50035;
// Not verified: Discord's UNKNOWN_INTEGRATION is believed to be API code 10005.
const UNKNOWN_INTEGRATION: u64 = 10005;

async fn send_interaction(req: reqwest::RequestBuilder) -> Result<(), InteractError> {
    let res = req.send().await.map_err(|_| InteractError::default())?;
    let status = res.status().as_u16();
    let body: Value = res.json().await.unwrap_or(Value::Null);
    if (200..300).contains(&status) {
        return Ok(());
    }
    eprintln!("http: {status} /interactions (code {}): {body}", body["code"]);
    Err(interact_error(status, &body))
}

fn interact_error(status: u16, body: &Value) -> InteractError {
    let code = body["code"].as_u64();
    let retry_after = body["retry_after"].as_f64();
    if !(400..500).contains(&status) || body.is_null() {
        return InteractError { status: Some(status), code, retry_after, ..Default::default() };
    }
    if code == Some(INVALID_FORM_BODY) && body["errors"].is_object() {
        let first = first_form_error(&body["errors"]);
        let stale = first.is_some_and(|e| matches!(e["code"].as_str(), Some("INTERACTION_APPLICATION_COMMAND_INVALID_VERSION" | "INTERACTION_APPLICATION_COMMAND_INVALID")));
        return InteractError { status: Some(status), code: None, message: first.and_then(|e| e["message"].as_str()).map(str::to_owned), stale_index: stale, retry_after };
    }
    let message = body["message"].as_str().map(str::to_owned);
    if code == Some(UNKNOWN_INTEGRATION) {
        return InteractError { status: Some(status), code: None, message, stale_index: true, retry_after };
    }
    InteractError { status: Some(status), code, message, stale_index: false, retry_after }
}

// The first `_errors[0]` anywhere in Discord's nested form errors, depth first.
fn first_form_error(errors: &Value) -> Option<&Value> {
    if let Some(first) = errors["_errors"].get(0) {
        return Some(first);
    }
    errors.as_object()?.iter().filter(|(key, _)| *key != "_errors").find_map(|(_, inner)| inner.is_object().then(|| first_form_error(inner)).flatten())
}

const SESSION_EXPIRED: &str = "Your session has expired. Sign in again.";

// A failed call as Discord answered it: the HTTP status (0 when no answer came), its JSON error code, and
// the text `error_text` words it as. For callers that word errors themselves (Add Friend).
#[derive(Clone, Debug, PartialEq)]
pub struct ApiError {
    pub status: u16,
    pub code: Option<u64>,
    pub text: String,
}

impl From<String> for ApiError {
    fn from(text: String) -> Self {
        ApiError { status: 0, code: None, text }
    }
}

impl From<ApiError> for String {
    fn from(error: ApiError) -> Self {
        error.text
    }
}

fn error_text(status: u16, body: &Value) -> String {
    match status {
        401 => SESSION_EXPIRED.to_owned(),
        // 40002 (verify your account), 50035 (invalid form) and the rest read as Discord words them.
        _ => api_error(body).unwrap_or_else(|| reqwest::StatusCode::from_u16(status).map_or_else(|_| status.to_string(), |s| s.to_string())),
    }
}

// A file on its way to Discord. `name` is what the message shows; `description` and `spoiler` ride on the
// message's attachment entry.
#[derive(Clone, Debug)]
pub struct Upload {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub description: Option<String>,
    pub spoiler: bool,
    pub sent: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

/// The most you can upload per file here: the higher of what your Nitro and the server's boost level
/// allow (docs.discord.food, Uploading Files: 10 MiB default; Nitro 500, Classic and Basic 50; boost
/// level 2 50, level 3 100).
pub fn upload_limit(premium_type: u8, guild_tier: u8) -> u64 {
    let user = match premium_type {
        2 => 500,
        1 | 3 => 50,
        _ => 10,
    };
    let guild = match guild_tier {
        3 => 100,
        2 => 50,
        _ => 10,
    };
    user.max(guild) << 20
}

/// The longest message text Discord takes: 4000 characters with Nitro, 2000 otherwise.
// Not verified: whether Discord counts code points or UTF-16 units; code points are counted here.
pub fn message_limit(premium_type: u8) -> usize {
    if premium_type == 2 { 4000 } else { 2000 }
}

// Discord caps a message at 10 attachments.
pub const MAX_FILES: usize = 10;

// `POST /channels/{id}/attachments`: one slot per file, its index as the id the response echoes.
fn attachments_request(files: &[Upload]) -> Value {
    let files: Vec<Value> = files.iter().enumerate().map(|(i, f)| json!({ "id": i.to_string(), "filename": f.name, "file_size": f.size })).collect();
    json!({ "files": files })
}

// (upload_url, upload_filename) per file, in the request's order.
fn parse_upload_slots(body: &Value, count: usize) -> Result<Vec<(String, String)>, String> {
    let mut slots: Vec<(usize, String, String)> = body["attachments"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(i, a)| {
            let at = a["id"].as_str().and_then(|id| id.parse().ok()).or(a["id"].as_u64().map(|id| id as usize)).unwrap_or(i);
            Some((at, a["upload_url"].as_str()?.to_owned(), a["upload_filename"].as_str()?.to_owned()))
        })
        .collect();
    slots.sort_by_key(|s| s.0);
    if slots.len() != count {
        return Err("Discord didn't return an upload URL for every file".into());
    }
    Ok(slots.into_iter().map(|(_, url, name)| (url, name)).collect())
}

// Create Message's body; `attachments` are files already uploaded.
fn message_body(content: &str, nonce: &str, reply_to: Option<&str>, ping_author: bool, attachments: &[SentAttachment]) -> Value {
    let mut body = json!({ "content": content, "nonce": nonce });
    if let Some(id) = reply_to {
        body["message_reference"] = json!({ "message_id": id });
        body["allowed_mentions"] = json!({ "parse": ["users", "roles", "everyone"], "replied_user": ping_author });
    }
    if !attachments.is_empty() {
        let list: Vec<Value> = attachments.iter().enumerate().map(|(i, a)| a.entry(i)).collect();
        body["attachments"] = json!(list);
    }
    body
}

// An uploaded file as Create Message names it.
pub struct SentAttachment {
    pub filename: String,
    // The upload slot's `upload_filename`.
    pub uploaded_filename: String,
    pub description: Option<String>,
    pub spoiler: bool,
}

impl SentAttachment {
    // Discord's own serializer: the index as the id, `description` only when set (an empty one goes as
    // is), `is_spoiler` only when true.
    fn entry(&self, index: usize) -> Value {
        let mut entry = json!({ "id": index.to_string(), "filename": self.filename, "uploaded_filename": self.uploaded_filename });
        if let Some(description) = &self.description {
            entry["description"] = json!(description);
        }
        if self.spoiler {
            entry["is_spoiler"] = json!(true);
        }
        entry
    }
}

// Edit Message's body for Modify Attachment after sending: every attachment the message keeps, by id,
// with the edited one carrying its alt text and spoiler (sent true or false).
// Not verified: that leaving an attachment out deletes it, which is why all of them go.
pub fn attachments_patch(ids: &[String], edited: &str, description: &str, spoiler: bool) -> Value {
    let list: Vec<Value> = ids
        .iter()
        .map(|id| match id == edited {
            true => json!({ "id": id, "description": description, "is_spoiler": spoiler }),
            false => json!({ "id": id }),
        })
        .collect();
    json!({ "attachments": list })
}

// Not verified: this is the body as the API reference describes a forward (a type-1 reference and no
// content); it hasn't been checked against a request from Discord's own client.
// Discord's play body: the sound and its emoji, and the sound's server unless it's one of Discord's own.
fn soundboard_play_body(sound: &SoundboardSound) -> Value {
    let emoji = sound.emoji.as_ref();
    let mut body = json!({ "sound_id": sound.id, "emoji_id": emoji.and_then(|e| e.id.clone()), "emoji_name": emoji.map(|e| e.name.clone()).filter(|n| !n.is_empty()) });
    if !sound.is_default() {
        body["source_guild_id"] = json!(sound.guild_id);
    }
    body
}

fn forward_body(original: &Message, guild_id: Option<&str>, nonce: &str) -> Value {
    let mut reference = json!({ "type": 1, "message_id": original.id, "channel_id": original.channel_id });
    if let Some(guild_id) = guild_id {
        reference["guild_id"] = json!(guild_id);
    }
    json!({ "nonce": nonce, "message_reference": reference })
}

// The PUT to a signed storage URL: no Authorization (the URL is the credential), streamed in chunks so
// `sent` follows the bytes out. Content-Length is set so the body isn't sent chunked.
async fn put_file(http: reqwest::Client, url: String, path: PathBuf, sent: std::sync::Arc<std::sync::atomic::AtomicU64>) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    let bytes = bytes::Bytes::from(tokio::fs::read(&path).await.map_err(|e| e.to_string())?);
    let len = bytes.len();
    sent.store(0, Ordering::Relaxed);
    let chunks = (0..len).step_by(64 << 10).map(move |at| bytes.slice(at..(at + (64 << 10)).min(len)));
    let stream = futures::stream::iter(chunks).map(move |chunk| {
        sent.fetch_add(chunk.len() as u64, Ordering::Relaxed);
        Ok::<_, std::io::Error>(chunk)
    });
    let res = http.put(url).header(reqwest::header::CONTENT_LENGTH, len).body(reqwest::Body::wrap_stream(stream)).send().await.map_err(|e| e.to_string())?;
    if res.status().is_success() {
        return Ok(());
    }
    let status = res.status();
    Err(format!("Upload failed: {status}"))
}

// Discord's refusal in its own words. "Invalid Form Body" says nothing alone, so the first field's
// message follows it: "Invalid Form Body: name: Must be between 1 and 100 in length."
/// A REST error that means the token itself was refused (HTTP 401).
pub fn is_unauthorized(error: &str) -> bool {
    error == SESSION_EXPIRED
}

fn api_error(body: &Value) -> Option<String> {
    fn first_field(errors: &Value, path: &str) -> Option<String> {
        if let Some(message) = errors["_errors"][0]["message"].as_str() {
            return Some(format!("{path}: {message}"));
        }
        errors.as_object()?.iter().find_map(|(key, inner)| first_field(inner, &if path.is_empty() { key.clone() } else { format!("{path}.{key}") }))
    }
    let message = body["message"].as_str()?;
    Some(match first_field(&body["errors"], "") {
        Some(detail) => format!("{message}: {detail}"),
        None => message.to_owned(),
    })
}

// `send` for lookups where 403 and 404 are answers ("not there for you"), not failures to retry.
async fn send_or_gone(req: reqwest::RequestBuilder) -> Result<Option<Value>, String> {
    let res = req.send().await.map_err(|e| e.to_string())?;
    let status = res.status();
    if matches!(status.as_u16(), 403 | 404) {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(status.to_string());
    }
    res.json().await.map(Some).map_err(|e| e.to_string())
}

// A server invite's card: `GET /invites/{code}?with_counts=true`.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
// Cached previews predate some fields.
#[serde(default)]
pub struct Invite {
    pub guild_id: String,
    pub name: String,
    pub icon: Option<String>,
    pub online: Option<u64>,
    pub members: Option<u64>,
    pub features: Vec<String>,
    pub splash: Option<String>,
    // The invite lets you pick a server nickname before joining (Discord's `is_nickname_changeable`).
    pub nickname_changeable: bool,
}

// A server's invite splash on the CDN, wide enough for a full-window background.
pub fn splash_url(guild_id: &str, hash: &str) -> String {
    format!("{CDN}/splashes/{guild_id}/{hash}.jpg?size=2048")
}

// The server a joined invite was to; a group DM invite has none.
fn joined_guild(body: &Value) -> Option<String> {
    body["guild"]["id"].as_str().or(body["guild_id"].as_str()).map(str::to_owned)
}

// Group DM invites carry no guild; they get the invalid card, as there is no server to show.
pub fn parse_invite(v: &Value) -> Option<Invite> {
    let guild = &v["guild"];
    let id = guild["id"].as_str()?;
    Some(Invite {
        guild_id: id.to_owned(),
        name: guild["name"].as_str().unwrap_or_default().to_owned(),
        icon: guild["icon"].as_str().map(|hash| format!("{CDN}/icons/{id}/{hash}.png?size=96")),
        online: v["approximate_presence_count"].as_u64(),
        members: v["approximate_member_count"].as_u64(),
        features: strings(&guild["features"]),
        splash: guild["splash"].as_str().map(|hash| splash_url(id, hash)),
        nickname_changeable: v["is_nickname_changeable"].as_bool().unwrap_or(false),
    })
}

// What the app and the voice engine send through the gateway, and where voice dispatches go.
struct Outbound {
    commands: mpsc::UnboundedReceiver<Value>,
    voice: crate::voice::GatewayHandle,
    voice_out: mpsc::UnboundedReceiver<String>,
}

// A drop RESUMEs the session (its id, the URL READY named and the last sequence) so Discord replays
// what was missed and keeps everything bound to it, a voice connection included: a fresh IDENTIFY
// ends the old session, and the voice server closes with 4014. Only an invalid session identifies anew.
async fn run_gateway(token: String, tx: mpsc::UnboundedSender<Event>, mut outbound: Outbound) {
    let mut resume: Option<Resume> = None;
    loop {
        let reason = match session(&token, &tx, &mut outbound, &mut resume).await {
            Ok(()) => return,
            Err(GatewayError::AuthFailed) => {
                let _ = tx.unbounded_send(Event::AuthFailed);
                return;
            }
            Err(GatewayError::Fatal(reason)) => {
                let _ = tx.unbounded_send(Event::Disconnected(reason));
                return;
            }
            Err(GatewayError::Dropped(reason)) => reason,
        };
        if tx.unbounded_send(Event::Disconnected(reason)).is_err() {
            return;
        }
        // A resumable drop goes straight back; a fresh session waits, as Discord asks after op 9.
        let wait = if resume.is_some() { Duration::from_millis(500) } else { Duration::from_secs(3) };
        tokio::time::sleep(wait).await;
    }
}

struct Resume {
    session_id: String,
    url: String,
    seq: Option<u64>,
}

impl Resume {
    fn payload(&self, token: &str) -> String {
        json!({ "op": 6, "d": { "token": token, "session_id": self.session_id, "seq": self.seq } }).to_string()
    }
}

// What each gateway close code means for us, from Discord's close event code table (docs.discord.com,
// Opcodes and Status Codes; 4015/4016 from docs.discord.food). Anything else, a network drop included,
// resumes.
#[derive(Debug, PartialEq, Eq)]
enum Close {
    Resume,
    // The session is gone: identify anew.
    Identify,
    // 4004: the token was refused.
    AuthFailed,
    // Reconnecting can't help (bot-only shard and intent codes, wrong API version): stop and say so.
    Fatal,
}

fn close_kind(code: u16) -> Close {
    match code {
        4004 => Close::AuthFailed,
        // 4007 invalid seq and 4009 session timed out invalidate the session; 4015 too many sessions,
        // 4016 connection request canceled.
        4007 | 4009 | 4015 | 4016 => Close::Identify,
        4010..=4014 => Close::Fatal,
        _ => Close::Resume,
    }
}

fn gateway_close_text(code: u16, reason: &str) -> String {
    let what = match code {
        4000 => "Unknown error",
        4001 => "Unknown opcode",
        4002 => "Decode error",
        4003 => "Not authenticated",
        4004 => "Authentication failed",
        4005 => "Already authenticated",
        4007 => "Invalid sequence",
        4008 => "Rate limited",
        4009 => "Session timed out",
        4010 => "Invalid shard",
        4011 => "Sharding required",
        4012 => "Invalid API version",
        4013 => "Invalid intents",
        4014 => "Disallowed intents",
        4015 => "Too many sessions",
        4016 => "Connection request canceled",
        _ if reason.is_empty() => "Connection closed",
        _ => reason,
    };
    format!("{what} ({code})")
}

enum GatewayError {
    AuthFailed,
    Dropped(String),
    Fatal(String),
}

async fn session(token: &str, tx: &mpsc::UnboundedSender<Event>, outbound: &mut Outbound, resume: &mut Option<Resume>) -> Result<(), GatewayError> {
    let dropped = |e: &dyn std::fmt::Display| GatewayError::Dropped(e.to_string());
    let url = resume.as_ref().map(|r| format!("{}/?v=10&encoding=json", r.url.trim_end_matches('/'))).unwrap_or_else(|| GATEWAY.to_owned());
    let (ws, _) = tokio_tungstenite::connect_async(url.as_str()).await.map_err(|e| {
        eprintln!("gateway: connect failed: {e}");
        dropped(&e)
    })?;
    let (mut sink, mut stream) = ws.split();
    let mut seq: Option<u64> = resume.as_ref().and_then(|r| r.seq);
    let mut heartbeat = tokio::time::interval(Duration::from_secs(3600));
    heartbeat.tick().await;
    // A beat with no ACK before the next one means a zombie socket (typically after sleep or a network
    // change): Discord's guidance is to drop it and reconnect rather than wait for TCP to notice.
    let mut acked = true;
    let mut greeted = false;

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if !acked {
                    return Err(GatewayError::Dropped("No heartbeat ACK".into()));
                }
                acked = false;
                let beat = json!({ "op": 1, "d": seq }).to_string();
                sink.send(WsMessage::Text(beat)).await.map_err(|e| dropped(&e))?;
            }
            // Held until IDENTIFY or RESUME is out: anything earlier is answered with a 4003 close.
            Some(command) = outbound.commands.next(), if greeted => {
                sink.send(WsMessage::Text(command.to_string())).await.map_err(|e| dropped(&e))?;
            }
            Some(text) = outbound.voice_out.next(), if greeted => {
                sink.send(WsMessage::Text(text)).await.map_err(|e| dropped(&e))?;
            }
            frame = stream.next() => {
                let frame = match frame {
                    None => return Err(GatewayError::Dropped("Connection closed".into())),
                    Some(frame) => frame.map_err(|e| dropped(&e))?,
                };
                let text = match frame {
                    WsMessage::Text(text) => text,
                    WsMessage::Close(None) => return Err(GatewayError::Dropped("Connection closed".into())),
                    WsMessage::Close(Some(close)) => {
                        let code = u16::from(close.code);
                        let text = gateway_close_text(code, &close.reason);
                        eprintln!("gateway: closed {text}");
                        return Err(match close_kind(code) {
                            Close::AuthFailed => GatewayError::AuthFailed,
                            Close::Fatal => GatewayError::Fatal(text),
                            Close::Identify => {
                                *resume = None;
                                GatewayError::Dropped(text)
                            }
                            Close::Resume => GatewayError::Dropped(text),
                        });
                    }
                    _ => continue,
                };
                let payload: Value = serde_json::from_str(&text).map_err(|e| dropped(&e))?;
                if let Some(s) = payload["s"].as_u64() {
                    seq = Some(s);
                    if let Some(resume) = resume.as_mut() {
                        resume.seq = seq;
                    }
                }
                match payload["op"].as_u64() {
                    Some(10) => {
                        let ms = payload["d"]["heartbeat_interval"].as_u64().unwrap_or(41_250);
                        // The first beat waits a random share of the interval (Discord's jitter), then every interval.
                        let period = Duration::from_millis(ms);
                        let first = period.mul_f64(rand::random::<f64>());
                        heartbeat = tokio::time::interval_at(tokio::time::Instant::now() + first, period);
                        // After a sleep, beats missed while asleep would fire back to back and fail the ACK check.
                        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                        let hello = resume.as_ref().map(|r| r.payload(token)).unwrap_or_else(|| identify(token));
                        sink.send(WsMessage::Text(hello)).await.map_err(|e| dropped(&e))?;
                        greeted = true;
                    }
                    Some(11) => acked = true,
                    // The gateway asking for a beat now.
                    Some(1) => sink.send(WsMessage::Text(json!({ "op": 1, "d": seq }).to_string())).await.map_err(|e| dropped(&e))?,
                    Some(7) => return Err(GatewayError::Dropped("Gateway asked to reconnect".into())),
                    // Invalid session: `d` says whether it can still be resumed.
                    Some(9) => {
                        if payload["d"].as_bool() != Some(true) {
                            *resume = None;
                        }
                        return Err(GatewayError::Dropped("Session expired".into()));
                    }
                    Some(0) => {
                        let kind = payload["t"].as_str().unwrap_or("");
                        match kind {
                            "READY" => {
                                let d = &payload["d"];
                                // No resume URL: resume through the regular gateway.
                                let url = d["resume_gateway_url"].as_str().unwrap_or("wss://gateway.discord.gg").to_owned();
                                *resume = d["session_id"].as_str().map(|id| Resume { session_id: id.to_owned(), url, seq });
                            }
                            "RESUMED" => _ = tx.unbounded_send(Event::Resumed),
                            _ => {}
                        }
                        outbound.voice.dispatch(kind, &payload["d"]);
                        let Some(event) = dispatch(kind, &payload["d"]) else { continue };
                        if tx.unbounded_send(event).is_err() {
                            return Ok(());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn status_wire(status: Status) -> &'static str {
    match status {
        Status::Online => "online",
        Status::Idle => "idle",
        Status::Dnd => "dnd",
        Status::Offline => "invisible",
    }
}

// capabilities 0 asks for the legacy READY shape: full guild objects and user objects in DMs.
fn identify(token: &str) -> String {
    json!({
        "op": 2,
        "d": {
            "token": token,
            "capabilities": 0,
            "properties": { "os": std::env::consts::OS, "browser": "Waffle", "device": "" },
            "presence": { "status": "online", "since": 0, "activities": [], "afk": false },
            "compress": false,
        }
    })
    .to_string()
}

fn dispatch(kind: &str, d: &Value) -> Option<Event> {
    let s = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    match kind {
        "READY" => Some(Event::Ready(Box::new(parse_ready(d)))),
        "USER_SETTINGS_UPDATE" => Some(Event::UserSettings(d.clone())),
        "GUILD_CREATE" => parse_guild(d).map(Event::GuildCreate),
        "THREAD_CREATE" if d["newly_created"].as_bool() == Some(true) => {
            let guild_id = d["guild_id"].as_str()?.to_owned();
            let channel = parse_channel(d, &guild_id)?;
            let created = Event::ForumPostCreated { forum_id: s(&d["parent_id"]), post_id: channel.id.clone(), owner_id: s(&d["owner_id"]) };
            Some(Event::Batch(vec![Event::ChannelUpsert { guild_id, channel }, created]))
        }
        // Not verified: `threads[].thread_id` and `threads[].count`; only the request's shape was read.
        "FORUM_UNREADS" => Some(Event::ForumUnreads {
            forum_id: s(&d["channel_id"]),
            counts: d["threads"].as_array().into_iter().flatten().filter_map(|t| Some((t["thread_id"].as_str()?.to_owned(), t["count"].as_u64().unwrap_or(0) as u32))).collect(),
        }),
        "CHANNEL_CREATE" | "CHANNEL_UPDATE" if d["guild_id"].is_null() && matches!(d["type"].as_u64(), Some(1 | 3)) => parse_dm(d, &Value::Null).map(Event::DmUpsert),
        "CHANNEL_DELETE" if d["guild_id"].is_null() => Some(Event::DmDelete(d["id"].as_str()?.to_owned())),
        "CHANNEL_RECIPIENT_ADD" | "CHANNEL_RECIPIENT_REMOVE" => Some(Event::RecipientChange { channel_id: d["channel_id"].as_str()?.to_owned(), user: parse_user(&d["user"]), added: kind == "CHANNEL_RECIPIENT_ADD" }),
        "CHANNEL_CREATE" | "CHANNEL_UPDATE" | "THREAD_CREATE" | "THREAD_UPDATE" => {
            let guild_id = d["guild_id"].as_str()?.to_owned();
            parse_channel(d, &guild_id).map(|channel| Event::ChannelUpsert { guild_id, channel })
        }
        "CHANNEL_DELETE" | "THREAD_DELETE" => Some(Event::ChannelDelete { guild_id: d["guild_id"].as_str()?.to_owned(), id: d["id"].as_str()?.to_owned() }),
        "GUILD_SCHEDULED_EVENT_CREATE" | "GUILD_SCHEDULED_EVENT_UPDATE" => parse_scheduled_event(d).map(Event::ScheduledEvent),
        // Not verified: the exception payload carries `guild_id` and `event_id` beside the exception's own fields.
        "GUILD_SCHEDULED_EVENT_EXCEPTION_CREATE" | "GUILD_SCHEDULED_EVENT_EXCEPTION_UPDATE" | "GUILD_SCHEDULED_EVENT_EXCEPTION_DELETE" => {
            let exception = parse_event_exception(d)?;
            Some(Event::ScheduledEventException {
                guild_id: d["guild_id"].as_str()?.to_owned(),
                event_id: d["event_id"].as_str()?.to_owned(),
                id: exception.id.clone(),
                exception: (kind != "GUILD_SCHEDULED_EVENT_EXCEPTION_DELETE").then_some(exception),
            })
        }
        "GUILD_SCHEDULED_EVENT_DELETE" => Some(Event::ScheduledEventDelete { guild_id: d["guild_id"].as_str()?.to_owned(), id: d["id"].as_str()?.to_owned() }),
        "GUILD_SCHEDULED_EVENT_USER_ADD" | "GUILD_SCHEDULED_EVENT_USER_REMOVE" => Some(Event::ScheduledEventUser {
            guild_id: s(&d["guild_id"]),
            event_id: s(&d["guild_scheduled_event_id"]),
            user_id: s(&d["user_id"]),
            added: kind == "GUILD_SCHEDULED_EVENT_USER_ADD",
        }),
        "GUILD_MEMBER_UPDATE" => Some(Event::MemberUpdate {
            guild_id: d["guild_id"].as_str()?.to_owned(),
            user: parse_member_user(d, &d["user"], d["guild_id"].as_str()),
            roles: strings(&d["roles"]),
            timed_out_until: timeout_end(d),
            membership: Membership::parse(d),
        }),
        "RELATIONSHIP_ADD" | "RELATIONSHIP_UPDATE" => {
            let kind = match d["type"].as_u64()? {
                1 => Relation::Friend,
                2 => Relation::Blocked,
                3 => Relation::Incoming,
                4 => Relation::Outgoing,
                _ => return None,
            };
            let user = if d["user"].is_object() { parse_user(&d["user"]) } else { User { id: d["id"].as_str()?.to_owned(), ..Default::default() } };
            Some(Event::RelationshipChange { user_id: d["id"].as_str()?.to_owned(), relationship: Some(Relationship { user, kind, since: d["since"].as_str().map(str::to_owned) }) })
        }
        "RELATIONSHIP_REMOVE" => Some(Event::RelationshipChange { user_id: d["id"].as_str()?.to_owned(), relationship: None }),
        "THREAD_MEMBER_UPDATE" => Some(Event::ThreadJoined { id: d["id"].as_str()?.to_owned(), joined: true }),
        "THREAD_LIST_SYNC" => {
            let guild_id = d["guild_id"].as_str()?.to_owned();
            // Sent with many threads at once; each becomes an upsert below via Batch.
            let threads: Vec<Event> = d["threads"].as_array().into_iter().flatten().filter_map(|t| parse_channel(t, &guild_id)).map(|channel| Event::ChannelUpsert { guild_id: guild_id.clone(), channel }).collect();
            Some(Event::Batch(threads))
        }
        "USER_GUILD_SETTINGS_UPDATE" => Some(Event::GuildSettings(parse_guild_settings(d))),
        "MESSAGE_CREATE" => parse_message(d).map(|m| Event::MessageCreate(Box::new(m))),
        "INTERACTION_CREATE" => Some(Event::InteractionCreate { nonce: d["nonce"].as_str()?.to_owned(), id: s(&d["id"]) }),
        "INTERACTION_SUCCESS" => Some(Event::InteractionSuccess { nonce: d["nonce"].as_str()?.to_owned() }),
        "INTERACTION_FAILURE" => Some(Event::InteractionFailure { nonce: d["nonce"].as_str()?.to_owned(), reason_code: d["reason_code"].as_u64() }),
        "INTERACTION_MODAL_CREATE" => crate::app_commands::parse_modal(d).map(Event::ModalCreate),
        "APPLICATION_COMMAND_AUTOCOMPLETE_RESPONSE" => Some(Event::Autocomplete { nonce: d["nonce"].as_str()?.to_owned(), choices: crate::app_commands::parse_choices(d) }),
        "MESSAGE_UPDATE" => Some(Event::MessageUpdate(d.clone())),
        "MESSAGE_DELETE" => Some(Event::MessageDelete { channel_id: s(&d["channel_id"]), id: s(&d["id"]) }),
        "MESSAGE_REACTION_ADD" | "MESSAGE_REACTION_REMOVE" => Some(Event::Reaction {
            channel_id: s(&d["channel_id"]),
            message_id: s(&d["message_id"]),
            user_id: s(&d["user_id"]),
            emoji: parse_emoji(&d["emoji"]),
            added: kind == "MESSAGE_REACTION_ADD",
        }),
        "MESSAGE_POLL_VOTE_ADD" | "MESSAGE_POLL_VOTE_REMOVE" => Some(Event::PollVote {
            channel_id: s(&d["channel_id"]),
            message_id: s(&d["message_id"]),
            user_id: s(&d["user_id"]),
            answer_id: d["answer_id"].as_u64()? as u32,
            added: kind == "MESSAGE_POLL_VOTE_ADD",
        }),
        "TYPING_START" => Some(Event::Typing {
            channel_id: s(&d["channel_id"]),
            user_id: s(&d["user_id"]),
            name: d["member"]["user"].is_object().then(|| parse_member_user(&d["member"], &d["member"]["user"], d["guild_id"].as_str()).name),
        }),
        "PRESENCE_UPDATE" => Some(Event::Presence { user_id: s(&d["user"]["id"]), presence: parse_presence(d) }),
        "GUILD_MEMBER_LIST_UPDATE" => Some(Event::MemberList {
            guild_id: s(&d["guild_id"]),
            // Group counts arrive here; the group items inside the list ops carry only ids.
            groups: d["groups"].as_array().into_iter().flatten().map(|g| (s(&g["id"]), g["count"].as_u64().unwrap_or(0) as u32)).collect(),
            ops: d["ops"].as_array().into_iter().flatten().filter_map(|op| parse_list_op(op, d["guild_id"].as_str())).collect(),
        }),
        "GUILD_MEMBERS_CHUNK" => Some(Event::MembersChunk {
            guild_id: s(&d["guild_id"]),
            members: d["members"].as_array().into_iter().flatten().map(|m| (parse_member_user(m, &m["user"], d["guild_id"].as_str()), strings(&m["roles"]))).collect(),
        }),
        "MESSAGE_ACK" => Some(Event::Ack {
            channel_id: s(&d["channel_id"]),
            message_id: snowflake(&d["message_id"]),
            manual: d["manual"].as_bool().unwrap_or(false).then(|| d["mention_count"].as_u64().unwrap_or(0) as u32),
        }),
        "VOICE_STATE_UPDATE" => Some(Event::VoiceState(parse_voice_state(d), d["session_id"].as_str().map(str::to_owned))),
        "STAGE_INSTANCE_CREATE" | "STAGE_INSTANCE_UPDATE" => Some(Event::StageInstance { guild_id: s(&d["guild_id"]), channel_id: s(&d["channel_id"]), instance: parse_stage_instance(d) }),
        "STAGE_INSTANCE_DELETE" => Some(Event::StageInstance { guild_id: s(&d["guild_id"]), channel_id: s(&d["channel_id"]), instance: None }),
        "CHANNEL_INFO" => {
            let channels = || d["channels"].as_array().into_iter().flatten();
            Some(Event::Batch(vec![
                Event::VoiceStartTimes { guild_id: s(&d["guild_id"]), times: channels().map(|c| (s(&c["id"]), c["voice_start_time"].as_u64())).collect(), all: true },
                Event::VoiceStatuses { guild_id: s(&d["guild_id"]), statuses: channels().map(|c| (s(&c["id"]), s(&c["status"]))).collect(), all: true },
            ]))
        }
        "VOICE_CHANNEL_STATUS_UPDATE" => Some(Event::VoiceStatuses { guild_id: s(&d["guild_id"]), statuses: vec![(s(&d["id"]), s(&d["status"]))], all: false }),
        "VOICE_CHANNEL_START_TIME_UPDATE" => Some(Event::VoiceStartTimes { guild_id: s(&d["guild_id"]), times: vec![(s(&d["id"]), d["voice_start_time"].as_u64())], all: false }),
        "STREAM_UPDATE" => Some(Event::StreamUpdate { key: s(&d["stream_key"]), viewers: strings(&d["viewer_ids"]), paused: d["paused"].as_bool().unwrap_or(false) }),
        "VOICE_SERVER_UPDATE" => Some(Event::VoiceServer(d.clone())),
        "CALL_CREATE" | "CALL_UPDATE" => Some(Event::Call { channel_id: s(&d["channel_id"]), ringing: strings(&d["ringing"]) }),
        "CALL_DELETE" => Some(Event::CallEnded { channel_id: s(&d["channel_id"]) }),
        "VOICE_CHANNEL_EFFECT_SEND" => parse_voice_effect(d).map(Event::VoiceEffect),
        // Not verified: the reply's shape, `{ guild_id, soundboard_sounds }`, is the public API's.
        "SOUNDBOARD_SOUNDS" | "GUILD_SOUNDBOARD_SOUNDS_UPDATE" => Some(Event::SoundboardSounds {
            guild_id: id_string(&d["guild_id"])?,
            sounds: d["soundboard_sounds"].as_array().into_iter().flatten().filter_map(SoundboardSound::parse).collect(),
            all: true,
        }),
        "GUILD_SOUNDBOARD_SOUND_CREATE" | "GUILD_SOUNDBOARD_SOUND_UPDATE" => {
            let sound = SoundboardSound::parse(d)?;
            Some(Event::SoundboardSounds { guild_id: sound.guild_id.clone(), sounds: vec![sound], all: false })
        }
        "GUILD_SOUNDBOARD_SOUND_DELETE" => Some(Event::SoundboardSoundDelete { guild_id: id_string(&d["guild_id"])?, sound_id: id_string(&d["sound_id"])? }),
        _ => None,
    }
}

fn parse_ready(d: &Value) -> Ready {
    let entries = d["read_state"]["entries"].as_array().or(d["read_state"].as_array());
    Ready {
        me: parse_user(&d["user"]),
        status: Status::parse(d["user_settings"]["status"].as_str().unwrap_or("online")),
        custom_status: CustomStatus::parse(&d["user_settings"]["custom_status"], chrono::Utc::now()),
        session_id: d["session_id"].as_str().unwrap_or_default().to_owned(),
        guilds: d["guilds"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .filter_map(|(i, g)| {
                let mut guild = parse_guild(g)?;
                let member = my_member(g, &d["merged_members"][i], d["user"]["id"].as_str().unwrap_or_default());
                guild.my_roles = strings(&member["roles"]);
                guild.timed_out_until = timeout_end(&member);
                guild.me = Membership::parse(&member);
                Some(guild)
            })
            .collect(),
        dms: d["private_channels"].as_array().into_iter().flatten().filter_map(|c| parse_dm(c, &d["users"])).collect(),
        read_states: entries
            .into_iter()
            .flatten()
            .filter_map(|r| {
                let id = r["id"].as_str()?.to_owned();
                Some((id, ReadState { last_read: snowflake(&r["last_message_id"]), mentions: r["mention_count"].as_u64().unwrap_or(0) as u32 }))
            })
            .collect(),
        developer_mode: d["user_settings"]["developer_mode"].as_bool().unwrap_or(false),
        my_account: MyAccount::parse(&d["user"]),
        account_settings: {
            let mut settings = AccountSettings::default();
            settings.apply(&d["user_settings"]);
            // Not verified: READY carries the settings proto as base64 under `user_settings_proto`.
            settings.apply_proto(d["user_settings_proto"].as_str().unwrap_or_default());
            settings
        },
        guild_settings: d["user_guild_settings"]["entries"].as_array().or(d["user_guild_settings"].as_array()).into_iter().flatten().map(parse_guild_settings).collect(),
        ignored: d["relationships"].as_array().into_iter().flatten().filter(|r| r["user_ignored"].as_bool() == Some(true)).filter_map(|r| r["id"].as_str().map(str::to_owned)).collect(),
        relationships: d["relationships"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| {
                let kind = match r["type"].as_u64()? {
                    1 => Relation::Friend,
                    2 => Relation::Blocked,
                    3 => Relation::Incoming,
                    4 => Relation::Outgoing,
                    _ => return None,
                };
                // Legacy READY embeds the user; newer shapes reference READY.users by id.
                let user = if r["user"].is_object() { parse_user(&r["user"]) } else { d["users"].as_array()?.iter().find(|u| u["id"] == r["id"]).map(parse_user)? };
                Some(Relationship { user, kind, since: r["since"].as_str().map(str::to_owned) })
            })
            .collect(),
        folders: d["user_settings"]["guild_folders"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| Folder {
                id: f["id"].as_u64().map(|n| n.to_string()).or(f["id"].as_str().map(str::to_owned)).unwrap_or_default(),
                name: f["name"].as_str().filter(|n| !n.is_empty()).map(str::to_owned),
                color: f["color"].as_u64().map(|c| c as u32),
                guild_ids: strings(&f["guild_ids"]),
            })
            .collect(),
        presences: d["presences"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| Some((p["user"]["id"].as_str().or(p["user_id"].as_str())?.to_owned(), parse_presence(p))))
            .collect(),
    }
}

fn parse_guild(g: &Value) -> Option<Guild> {
    let id = g["id"].as_str()?;
    let props = if g["name"].is_string() { g } else { &g["properties"] };
    let mut channels: Vec<Channel> = g["channels"].as_array().into_iter().flatten().filter_map(|c| parse_channel(c, id)).collect();
    channels.sort_by_key(|c| c.position);
    Some(Guild {
        id: id.to_owned(),
        name: props["name"].as_str()?.to_owned(),
        icon: props["icon"].as_str().map(|hash| format!("{CDN}/icons/{id}/{hash}.png?size=96")),
        channels,
        roles: g["roles"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| {
                Some(Role {
                    id: r["id"].as_str()?.to_owned(),
                    name: r["name"].as_str().unwrap_or_default().to_owned(),
                    color: r["color"].as_u64().unwrap_or(0) as u32,
                    position: r["position"].as_i64().unwrap_or(0),
                    permissions: permission(&r["permissions"]),
                    managed: r["managed"].as_bool().unwrap_or(false),
                })
            })
            .collect(),
        // Guild payloads leave out each voice state's guild_id.
        voice_states: g["voice_states"].as_array().into_iter().flatten().map(|v| VoiceState { guild_id: Some(id.to_owned()), ..parse_voice_state(v) }).collect(),
        stage_instances: g["stage_instances"].as_array().into_iter().flatten().filter_map(parse_stage_instance).collect(),
        emojis: g["emojis"].as_array().into_iter().flatten().map(parse_emoji).collect(),
        // Not verified: READY's user-account guild objects carrying `stickers` the way bot GUILD_CREATE does.
        stickers: g["stickers"].as_array().into_iter().flatten().filter_map(Sticker::parse).collect(),
        owner_id: props["owner_id"].as_str().or(g["owner_id"].as_str()).map(str::to_owned),
        default_notifications: props["default_message_notifications"].as_u64().or(g["default_message_notifications"].as_u64()).unwrap_or(0) as u8,
        threads: g["threads"].as_array().into_iter().flatten().filter_map(|t| parse_channel(t, id)).collect(),
        my_roles: Vec::new(),
        premium_tier: props["premium_tier"].as_u64().or(g["premium_tier"].as_u64()).unwrap_or(0) as u8,
        afk_channel_id: props["afk_channel_id"].as_str().or(g["afk_channel_id"].as_str()).map(str::to_owned),
        timed_out_until: None,
        system_channel_flags: props["system_channel_flags"].as_u64().or(g["system_channel_flags"].as_u64()).unwrap_or(0),
        features: strings(if props["features"].is_array() { &props["features"] } else { &g["features"] }),
        splash: props["splash"].as_str().or(g["splash"].as_str()).map(|hash| splash_url(id, hash)),
        rules_channel_id: props["rules_channel_id"].as_str().or(g["rules_channel_id"].as_str()).map(str::to_owned),
        me: Membership::default(),
        events: g["guild_scheduled_events"].as_array().into_iter().flatten().filter_map(parse_scheduled_event).collect(),
        public_updates_channel_id: props["public_updates_channel_id"].as_str().or(g["public_updates_channel_id"].as_str()).map(str::to_owned),
        vanity_url_code: props["vanity_url_code"].as_str().or(g["vanity_url_code"].as_str()).map(str::to_owned),
    })
}

// READY lists our own member entry per server, either inside each guild or in `merged_members`.
fn my_member(guild: &Value, merged: &Value, me: &str) -> Value {
    let mine = |list: &Value| list.as_array().into_iter().flatten().find(|m| m["user_id"].as_str().or(m["user"]["id"].as_str()) == Some(me)).cloned();
    mine(&guild["members"]).or_else(|| mine(merged)).unwrap_or(Value::Null)
}

fn timeout_end(member: &Value) -> Option<chrono::DateTime<chrono::Utc>> {
    let end = member["communication_disabled_until"].as_str()?;
    Some(chrono::DateTime::parse_from_rfc3339(end).ok()?.with_timezone(&chrono::Utc))
}

pub fn parse_thread(c: &Value, guild_id: &str) -> Option<Channel> {
    parse_channel(c, guild_id)
}

pub(crate) fn parse_channel(c: &Value, guild_id: &str) -> Option<Channel> {
    let kind = match c["type"].as_u64()? {
        0 | 5 => ChannelKind::Text,
        2 | 13 => ChannelKind::Voice,
        4 => ChannelKind::Category,
        10..=12 => ChannelKind::Thread,
        15 | 16 => ChannelKind::Forum,
        _ => ChannelKind::Other,
    };
    let meta = &c["thread_metadata"];
    let thread = (kind == ChannelKind::Thread).then(|| ThreadMeta {
        archived: meta["archived"].as_bool().unwrap_or(false),
        locked: meta["locked"].as_bool().unwrap_or(false),
        joined: c["member"].is_object(),
        message_count: c["message_count"].as_u64().unwrap_or(0) as u32,
        owner_id: c["owner_id"].as_str().map(str::to_owned),
        muted: c["member"]["muted"].as_bool().unwrap_or(false),
        flags: c["member"]["flags"].as_u64().unwrap_or(0),
        applied_tags: strings(&c["applied_tags"]),
        pinned: c["flags"].as_u64().unwrap_or(0) & 2 != 0,
    });
    let overwrites: Vec<Overwrite> = c["permission_overwrites"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|o| Some(Overwrite { id: o["id"].as_str()?.to_owned(), allow: permission(&o["allow"]), deny: permission(&o["deny"]), member: o["type"].as_u64() == Some(1) }))
        .collect();
    let private = overwrites.iter().any(|o| o.id == guild_id && o.deny & VIEW_CHANNEL != 0);
    Some(Channel {
        id: c["id"].as_str()?.to_owned(),
        name: c["name"].as_str().unwrap_or_default().to_owned(),
        kind,
        parent_id: c["parent_id"].as_str().map(str::to_owned),
        position: c["position"].as_i64().unwrap_or(0),
        topic: c["topic"].as_str().filter(|t| !t.is_empty()).map(str::to_owned),
        last_message_id: c["last_message_id"].as_str().map(str::to_owned),
        private,
        recipient: None,
        recipients: Vec::new(),
        icon: None,
        thread,
        raw_type: c["type"].as_u64().unwrap_or(0) as u8,
        overwrites,
        props: Box::new(ChannelProps {
            nsfw: c["nsfw"].as_bool().unwrap_or(false),
            slowmode: c["rate_limit_per_user"].as_u64().unwrap_or(0) as u32,
            bitrate: c["bitrate"].as_u64().unwrap_or(0) as u32,
            user_limit: c["user_limit"].as_u64().unwrap_or(0) as u32,
            rtc_region: c["rtc_region"].as_str().map(str::to_owned),
            video_full: c["video_quality_mode"].as_u64() == Some(2),
            archive_after: c["default_auto_archive_duration"].as_u64().unwrap_or(0) as u32,
            tags: c["available_tags"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|t| {
                    Some(ForumTag {
                        id: t["id"].as_str()?.to_owned(),
                        name: t["name"].as_str().unwrap_or_default().to_owned(),
                        emoji: parse_forum_emoji(&t["emoji_id"], &t["emoji_name"]),
                        moderated: t["moderated"].as_bool().unwrap_or(false),
                    })
                })
                .collect(),
            require_tag: c["flags"].as_u64().unwrap_or(0) & 16 != 0,
            sort_by_creation: c["default_sort_order"].as_u64() == Some(1),
            gallery: c["type"].as_u64() == Some(16) || c["default_forum_layout"].as_u64() == Some(2),
            match_all: c["default_tag_setting"].as_str() == Some("match_all"),
            default_reaction: parse_forum_emoji(&c["default_reaction_emoji"]["emoji_id"], &c["default_reaction_emoji"]["emoji_name"]),
            template: c["template"].as_str().unwrap_or_default().to_owned(),
            ..Default::default()
        }),
    })
}

fn parse_dm(c: &Value, users: &Value) -> Option<Channel> {
    let recipients: Vec<User> = match c["recipients"].as_array() {
        Some(list) => list.iter().map(parse_user).collect(),
        None => c["recipient_ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|rid| users.as_array()?.iter().find(|u| u["id"] == *rid).map(parse_user))
            .collect(),
    };
    let names: Vec<&str> = recipients.iter().map(|u| u.name.as_str()).collect();
    let name = c["name"].as_str().filter(|n| !n.is_empty()).map(str::to_owned).unwrap_or_else(|| names.join(", "));
    let id = c["id"].as_str()?.to_owned();
    let icon = c["icon"].as_str().map(|hash| format!("{CDN}/channel-icons/{id}/{hash}.png?size=128"));
    Some(Channel {
        id,
        name,
        kind: ChannelKind::Dm,
        parent_id: None,
        position: 0,
        topic: None,
        last_message_id: c["last_message_id"].as_str().map(str::to_owned),
        private: false,
        recipient: (recipients.len() == 1 && c["type"].as_u64() != Some(3)).then(|| recipients[0].clone()),
        recipients,
        icon,
        thread: None,
        raw_type: c["type"].as_u64().unwrap_or(1) as u8,
        overwrites: Vec::new(),
        props: Box::new(ChannelProps {
            owner_id: c["owner_id"].as_str().map(str::to_owned),
            message_request: c["is_message_request"].as_bool().unwrap_or(false),
            spam: c["is_spam"].as_bool().unwrap_or(false),
            ..Default::default()
        }),
    })
}

// `/messages/pins` lists `{ items: [{ pinned_at, message }] }`; the deprecated `/pins` a bare message array.
fn parse_pins(body: &Value) -> Vec<Message> {
    match body["items"].as_array() {
        Some(items) => items.iter().filter_map(|pin| parse_message(&pin["message"])).collect(),
        None => body.as_array().into_iter().flatten().filter_map(parse_message).collect(),
    }
}

// An attachment's `flags` bit for a spoiler (Discord's IS_SPOILER).
const ATTACHMENT_SPOILER: u64 = 8;

pub fn parse_message(m: &Value) -> Option<Message> {
    use base64::Engine;
    let mut media = Vec::new();
    let mut files = Vec::new();
    let mut media_files = Vec::new();
    for a in m["attachments"].as_array().into_iter().flatten() {
        let url = a["url"].as_str().unwrap_or_default().to_owned();
        let kind = a["content_type"].as_str().unwrap_or_default();
        let id = a["id"].as_str().unwrap_or_default().to_owned();
        let description = a["description"].as_str().filter(|d| !d.is_empty()).map(str::to_owned);
        let spoiler = a["flags"].as_u64().unwrap_or(0) & ATTACHMENT_SPOILER != 0;
        let file = File {
            name: a["filename"].as_str().unwrap_or("file").to_owned(),
            url: url.clone(),
            size: a["size"].as_u64().unwrap_or(0),
            id: id.clone(),
            spoiler,
            duration_secs: a["duration_secs"].as_f64().map(|d| d as f32),
            waveform: a["waveform"].as_str().and_then(|w| base64::engine::general_purpose::STANDARD.decode(w).ok()).unwrap_or_default(),
        };
        let (Some(w), Some(h)) = (a["width"].as_f64(), a["height"].as_f64()) else {
            files.push(file);
            continue;
        };
        media_files.push(file);
        let kind = match kind.starts_with("video/") {
            // Discord's media proxy returns a still frame for a video when asked for an image format.
            true => {
                let proxy = a["proxy_url"].as_str().unwrap_or(&url);
                MediaKind::Video { poster: format!("{proxy}{}format=jpeg", if proxy.contains('?') { "&" } else { "?" }) }
            }
            false => MediaKind::Image,
        };
        media.push(Media { url, width: w as f32, height: h as f32, kind, id: Some(id), description, spoiler });
    }
    let flags = m["flags"].as_u64().unwrap_or(0);
    let attachment_media = media.len();
    let embeds = if flags & message_flag::SUPPRESS_EMBEDS == 0 { parse_embeds(&m["embeds"], &mut media) } else { Vec::new() };
    let embed_media = media.len() - attachment_media;
    // Reference type 1 is a forward; only 0 (or none, on older messages) is a reply.
    // A crosspost's reference points at its source in the followed channel, not at a message it answers.
    let is_crosspost = m["flags"].as_u64().unwrap_or(0) & Message::IS_CROSSPOST != 0;
    let is_reply = !is_crosspost && matches!(m["message_reference"]["type"].as_u64(), None | Some(0));
    let reply = (is_reply && m["referenced_message"].is_object()).then(|| parse_message(&m["referenced_message"])).flatten().map(Box::new);
    Some(Message {
        id: m["id"].as_str()?.to_owned(),
        channel_id: m["channel_id"].as_str()?.to_owned(),
        guild_id: m["guild_id"].as_str().map(str::to_owned),
        author: parse_member_user(&m["member"], &m["author"], m["guild_id"].as_str()),
        roles: m["member"]["roles"].is_array().then(|| strings(&m["member"]["roles"])),
        content: m["content"].as_str().unwrap_or_default().to_owned(),
        timestamp: m["timestamp"].as_str().unwrap_or_default().to_owned(),
        edited: m["edited_timestamp"].is_string(),
        media,
        embeds,
        files,
        media_files,
        reactions: m["reactions"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|r| Reaction { emoji: parse_emoji(&r["emoji"]), count: r["count"].as_u64().unwrap_or(1) as u32, me: r["me"].as_bool().unwrap_or(false) })
            .collect(),
        reply_to: m["message_reference"]["message_id"].as_str().filter(|_| is_reply).map(str::to_owned),
        reply,
        mentions: m["mentions"].as_array().into_iter().flatten().map(parse_user).collect(),
        mention_roles: strings(&m["mention_roles"]),
        mention_everyone: m["mention_everyone"].as_bool().unwrap_or(false),
        tts: m["tts"].as_bool().unwrap_or(false),
        nonce: m["nonce"].as_str().map(str::to_owned),
        state: SendState::Sent,
        pinned: m["pinned"].as_bool().unwrap_or(false),
        thread: m["thread"].is_object().then(|| parse_channel(&m["thread"], m["guild_id"].as_str().unwrap_or_default())).flatten(),
        call: (m["type"] == 3).then(|| CallRecord { participants: strings(&m["call"]["participants"]), ended: m["call"]["ended_timestamp"].as_str().map(str::to_owned) }),
        stickers: m["sticker_items"].as_array().or(m["stickers"].as_array()).into_iter().flatten().filter_map(Sticker::parse).collect(),
        poll: m["poll"].is_object().then(|| parse_poll(&m["poll"])),
        forward: parse_forward(m),
        pin_notice: m["type"] == 6,
        kind: m["type"].as_u64().unwrap_or(0) as u8,
        flags,
        reference_channel: m["message_reference"]["channel_id"].as_str().map(str::to_owned),
        reference_deleted: m["message_reference"].is_object() && m.get("referenced_message").is_some_and(Value::is_null),
        // Not verified: the wire key. Discord's web client reads `message.actor`.
        actor: m["actor"].is_object().then(|| parse_user(&m["actor"])),
        embed_media,
        interaction: crate::app_commands::parse_interaction(m),
        components: crate::app_commands::parse_components(&m["components"]),
        application_id: m["application_id"].as_str().map(str::to_owned),
        interaction_error: None,
        crosspost_from: is_crosspost.then(|| crosspost_source(&m["message_reference"])).flatten(),
    })
}

fn crosspost_source(reference: &Value) -> Option<CrosspostSource> {
    Some(CrosspostSource { guild_id: reference["guild_id"].as_str()?.to_owned(), channel_id: reference["channel_id"].as_str()?.to_owned() })
}

// Image and GIF embeds join `media` (what the gallery pages through); the rest come back as cards.
fn parse_embeds(list: &Value, media: &mut Vec<Media>) -> Vec<Embed> {
    let mut embeds = Vec::new();
    for e in list.as_array().into_iter().flatten() {
        match e["type"].as_str().unwrap_or("rich") {
            "image" => media.extend(embed_media(&e["thumbnail"]).or_else(|| embed_media(&e["image"]))),
            // GIF pickers (Tenor, Giphy) send a looping mp4 with a still thumbnail.
            "gifv" => {
                let poster = embed_media(&e["thumbnail"]);
                let video = e["video"]["url"].as_str();
                if let (Some(poster), Some(video)) = (poster, video) {
                    media.push(Media { url: video.to_owned(), width: poster.width, height: poster.height, kind: MediaKind::Gif { poster: poster.url }, ..Default::default() });
                }
            }
            _ => {
                let embed = parse_embed(e);
                // Discord folds embeds that share a url into the first one's gallery.
                let first = embed.url.as_ref().and_then(|url| embeds.iter_mut().find(|f: &&mut Embed| f.url.as_ref() == Some(url)));
                match first {
                    Some(first) => first.images.extend(embed.images.into_iter().take(4usize.saturating_sub(first.images.len()))),
                    None => embeds.push(embed),
                }
            }
        }
    }
    embeds
}

// A forward's snapshot carries the original's content, media and time but no id, author or channel;
// those come from the reference.
fn parse_forward(m: &Value) -> Option<Box<Message>> {
    let reference = &m["message_reference"];
    if reference["type"].as_u64() != Some(1) {
        return None;
    }
    let mut original = m["message_snapshots"][0]["message"].clone();
    if !original.is_object() {
        return None;
    }
    original["id"] = reference["message_id"].clone();
    original["channel_id"] = reference["channel_id"].clone();
    original["guild_id"] = reference["guild_id"].clone();
    parse_message(&original).map(Box::new)
}

/// Merges a partial MESSAGE_UPDATE (edits, late embeds) into a cached message.
pub fn apply_update(message: &mut Message, d: &Value) {
    if let Some(full) = d["author"].is_object().then(|| parse_message(d)).flatten() {
        let reactions = std::mem::take(&mut message.reactions);
        *message = Message { reactions, ..full };
        return;
    }
    if let Some(content) = d["content"].as_str() {
        message.content = content.to_owned();
        message.edited = true;
    }
    if let Some(flags) = d["flags"].as_u64() {
        message.flags = flags;
    }
    if d["embeds"].is_array() || message.embeds_suppressed() {
        message.media.truncate(message.media.len().saturating_sub(message.embed_media));
        let before = message.media.len();
        message.embeds = if message.embeds_suppressed() { Vec::new() } else { parse_embeds(&d["embeds"], &mut message.media) };
        message.embed_media = message.media.len() - before;
    }
}

// A member as that server shows them: their server nickname and server avatar win over the account's.
pub fn parse_member_user(member: &Value, user: &Value, guild_id: Option<&str>) -> User {
    let mut parsed = parse_user(user);
    if let Some(nick) = member["nick"].as_str().filter(|n| !n.is_empty()) {
        parsed.name = nick.to_owned();
    }
    if let (Some(hash), Some(guild_id)) = (member["avatar"].as_str().filter(|h| !h.is_empty()), guild_id) {
        parsed.avatar = format!("{CDN}/guilds/{guild_id}/users/{}/avatars/{hash}.png?size=128", parsed.id);
    }
    if let Some(style) = parse_name_style(&member["display_name_styles"]) {
        parsed.name_style = Some(style);
    }
    parsed
}

pub fn parse_user(user: &Value) -> User {
    let id = user["id"].as_str().unwrap_or_default().to_owned();
    let name = user["global_name"].as_str().or(user["username"].as_str()).unwrap_or("Unknown").to_owned();
    let avatar = avatar_url(&id, user["avatar"].as_str());
    let guild = if user["primary_guild"].is_object() { &user["primary_guild"] } else { &user["clan"] };
    let tag = guild["tag"].as_str().filter(|_| guild["identity_enabled"].as_bool() != Some(false)).map(|text| GuildTag {
        text: text.to_owned(),
        badge: match (guild["identity_guild_id"].as_str(), guild["badge"].as_str()) {
            (Some(gid), Some(badge)) => Some(format!("{CDN}/guild-tag-badges/{gid}/{badge}.png?size=32")),
            _ => None,
        },
    });
    let plate = &user["collectibles"]["nameplate"];
    let nameplate = plate["asset"].as_str().map(|asset| Nameplate {
        art: format!("{CDN}/assets/collectibles/{asset}static.png"),
        animated: plate["sku_id"].as_str().map(|sku| format!("{CDN}/media/v1/collectibles-shop/{sku}/animated")),
        tint: palette_tint(plate["palette"].as_str().unwrap_or_default()),
    }).map(Box::new);
    let decoration = user["avatar_decoration_data"]["asset"].as_str().map(|asset| format!("{CDN}/avatar-decoration-presets/{asset}.png?size=96&passthrough=false"));
    let name_style = parse_name_style(&user["display_name_styles"]);
    let username = user["username"].as_str().unwrap_or_default().to_owned();
    User { id, name, avatar, bot: user["bot"].as_bool().unwrap_or(false), tag, nameplate, decoration, name_style, username }

}

/// An image as Discord's media proxy scales it: `width` x `height` (logical pixels, drawn at up to 2x
/// for high-DPI screens), never larger than the original. Attachments on cdn.discordapp.com move to
/// media.discordapp.net, which resizes on request, as Discord's own client does; the signed query
/// (ex, is, hm) is kept. Other hosts come back unchanged. Not verified against every proxy host:
/// images-ext-*.discordapp.net (embeds) is assumed to take the same parameters.
pub fn sized_image(url: &str, width: f32, height: f32, original: (f32, f32)) -> String {
    let scale = (2. * width / original.0.max(1.)).min(2. * height / original.1.max(1.)).min(1.);
    let Some(proxied) = proxied(url) else { return url.to_owned() };
    if scale >= 1. {
        return proxied;
    }
    with_query(proxied, &scaled(original, scale))
}

/// A thumbnail that covers a `side` x `side` square of device pixels: the poster for a video or GIF,
/// scaled so its shorter edge is `side`. Pictures on media.discordapp.net also come back as WebP.
/// Not verified: `format=webp` is what Discord's own client asks the proxy for, not a documented parameter.
pub fn thumb_url(media: &Media, side: f32) -> String {
    let source = match &media.kind {
        MediaKind::Image => &media.url,
        MediaKind::Video { poster } | MediaKind::Gif { poster } => poster,
    };
    let Some(proxied) = proxied(source) else { return source.clone() };
    let original = (media.width.max(1.), media.height.max(1.));
    let scale = (side / original.0).max(side / original.1).min(1.);
    let webp = matches!(media.kind, MediaKind::Image) && proxied.starts_with("https://media.discordapp.net/");
    let size = if scale < 1. { scaled(original, scale) } else { String::new() };
    match (size.is_empty(), webp) {
        (true, false) => proxied,
        (true, true) => with_query(proxied, "format=webp"),
        (false, false) => with_query(proxied, &size),
        (false, true) => with_query(proxied, &format!("{size}&format=webp")),
    }
}

// The media-proxy address for `url`, or None when its host doesn't resize.
fn proxied(url: &str) -> Option<String> {
    if let Some(rest) = url.strip_prefix("https://cdn.discordapp.com/attachments/") {
        return Some(format!("https://media.discordapp.net/attachments/{rest}"));
    }
    let resizes = url.starts_with("https://media.discordapp.net/") || (url.starts_with("https://images-ext-") && url.contains(".discordapp.net/"));
    resizes.then(|| url.to_owned())
}

fn scaled(original: (f32, f32), scale: f32) -> String {
    let (w, h) = ((original.0 * scale).round().max(1.) as u32, (original.1 * scale).round().max(1.) as u32);
    format!("width={w}&height={h}")
}

fn with_query(url: String, query: &str) -> String {
    let joiner = if url.contains('?') { "&" } else { "?" };
    format!("{url}{joiner}{query}")
}

/// The moving version of an avatar or decoration URL, for while Discord would animate it: an animated
/// (`a_`) avatar as its GIF, a decoration with `passthrough=true` (its APNG, as Discord's client asks for).
/// Anything else comes back as it is.
pub fn animated(url: &str) -> String {
    if url.contains("/avatars/") && url.contains("/a_") {
        return url.replacen(".png?", ".gif?", 1);
    }
    url.replacen("passthrough=false", "passthrough=true", 1)
}

pub fn avatar_url(user_id: &str, hash: Option<&str>) -> String {
    match hash.filter(|h| !h.is_empty() && *h != "0") {
        Some(hash) => format!("{CDN}/avatars/{user_id}/{hash}.png?size=128"),
        // Discord's default avatars, chosen by (id >> 22) % 6 for accounts on the new username system.
        None => format!("{CDN}/embed/avatars/{}.png", user_id.parse::<u64>().map(|id| (id >> 22) % 6).unwrap_or(0)),
    }
}

// What stands for a DM wherever it shows: a group's own icon, the other person, or (a group without
// an icon) two members on a diagonal, as Discord draws it.
#[derive(Clone, Debug, PartialEq)]
pub enum DmFace {
    Icon(String),
    One(String),
    Pair(String, String),
    // A group everyone else has left.
    Letters(String),
}

impl Channel {
    pub fn face(&self) -> DmFace {
        if let Some(icon) = &self.icon {
            return DmFace::Icon(icon.clone());
        }
        if let Some(user) = &self.recipient {
            return DmFace::One(user.avatar.clone());
        }
        match self.recipients.as_slice() {
            [] => DmFace::Letters(self.name.clone()),
            [only] => DmFace::One(only.avatar.clone()),
            [back, front, ..] => DmFace::Pair(back.avatar.clone(), front.avatar.clone()),
        }
    }
}

fn embed_media(v: &Value) -> Option<Media> {
    let url = v["proxy_url"].as_str().or(v["url"].as_str())?;
    Some(Media { url: url.to_owned(), width: v["width"].as_f64()? as f32, height: v["height"].as_f64()? as f32, kind: MediaKind::Image, ..Default::default() })
}

// Embed flags: CONTAINS_EXPLICIT_CONTENT, CONTAINS_GORE_CONTENT, CONTAINS_SELF_HARM_CONTENT.
const SENSITIVE_EMBED: u64 = 1 << 4 | 1 << 6 | 1 << 7;

fn parse_embed(e: &Value) -> Embed {
    let text = |v: &Value| v.as_str().filter(|t| !t.is_empty()).map(str::to_owned);
    if e["type"] == "poll_result" {
        return Embed { poll: Some(parse_poll_result(e)), ..Default::default() };
    }
    let video = e["type"] == "video" || e["video"].is_object();
    // A video's still is its thumbnail; it plays large, so it never sits small at the right.
    let image = embed_media(&e["image"]).or_else(|| video.then(|| embed_media(&e["thumbnail"])).flatten());
    Embed {
        url: text(&e["url"]),
        color: e["color"].as_u64().map(|c| c as u32),
        provider: text(&e["provider"]["name"]),
        provider_url: text(&e["provider"]["url"]),
        author: text(&e["author"]["name"]),
        author_icon: text(&e["author"]["proxy_icon_url"]).or_else(|| text(&e["author"]["icon_url"])),
        author_url: text(&e["author"]["url"]),
        title: text(&e["title"]),
        description: text(&e["description"]),
        fields: e["fields"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| (f["name"].as_str().unwrap_or_default().to_owned(), f["value"].as_str().unwrap_or_default().to_owned(), f["inline"].as_bool().unwrap_or(false)))
            .collect(),
        thumbnail: (!video).then(|| embed_media(&e["thumbnail"])).flatten(),
        images: image.into_iter().collect(),
        video,
        sensitive: e["flags"].as_u64().unwrap_or(0) & SENSITIVE_EMBED != 0,
        footer: text(&e["footer"]["text"]),
        footer_icon: text(&e["footer"]["proxy_icon_url"]).or_else(|| text(&e["footer"]["icon_url"])),
        timestamp: text(&e["timestamp"]),
        poll: None,
        safety_alert: e["type"] == "auto_moderation_notification",
        rich: e["type"].as_str().unwrap_or("rich") == "rich",
    }
}

// Counts arrive apart from the answers, in `results`; an answer missing there has no votes.
fn parse_poll(p: &Value) -> Poll {
    let counts = p["results"]["answer_counts"].as_array().cloned().unwrap_or_default();
    let answers = p["answers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let id = a["answer_id"].as_u64()? as u32;
            let result = counts.iter().find(|c| c["id"].as_u64() == Some(id.into()));
            let media = &a["poll_media"];
            Some(PollAnswer {
                id,
                text: media["text"].as_str().unwrap_or_default().to_owned(),
                emoji: media["emoji"].is_object().then(|| parse_emoji(&media["emoji"])),
                count: result.and_then(|c| c["count"].as_u64()).unwrap_or(0),
                me: result.and_then(|c| c["me_voted"].as_bool()).unwrap_or(false),
            })
        })
        .collect();
    Poll {
        question: p["question"]["text"].as_str().unwrap_or_default().to_owned(),
        answers,
        expiry: p["expiry"].as_str().map(str::to_owned),
        multiselect: p["allow_multiselect"].as_bool().unwrap_or(false),
        finalized: p["results"]["is_finalized"].as_bool().unwrap_or(false),
    }
}

// A poll_result embed carries its numbers as fields named after them.
fn parse_poll_result(e: &Value) -> PollResult {
    let fields: HashMap<&str, &str> = e["fields"].as_array().into_iter().flatten().filter_map(|f| Some((f["name"].as_str()?, f["value"].as_str()?))).collect();
    let number = |name: &str| fields.get(name).and_then(|v| v.parse().ok()).unwrap_or(0);
    let emoji = fields.get("victor_answer_emoji_id").map(|id| Emoji { id: Some((*id).to_owned()), name: fields.get("victor_answer_emoji_name").copied().unwrap_or_default().to_owned(), animated: fields.get("victor_answer_emoji_animated") == Some(&"true") })
        .or_else(|| fields.get("victor_answer_emoji_name").map(|name| Emoji::unicode(name)));
    PollResult {
        question: fields.get("poll_question_text").copied().unwrap_or_default().to_owned(),
        winner: fields.get("victor_answer_text").map(|text| ((*text).to_owned(), emoji)),
        winner_votes: number("victor_answer_votes"),
        total_votes: number("total_votes"),
    }
}

fn parse_presence(p: &Value) -> Presence {
    let activities = p["activities"].as_array().cloned().unwrap_or_default();
    let custom = activities.iter().find(|a| a["type"] == 4).and_then(|a| {
        let text = a["state"].as_str().filter(|t| !t.is_empty())?;
        let emoji = a["emoji"]["name"].as_str().filter(|_| a["emoji"]["id"].is_null()).map(|e| format!("{e} ")).unwrap_or_default();
        Some(format!("{emoji}{text}"))
    });
    let others: Vec<&Value> = activities.iter().filter(|a| a["type"] != 4).collect();
    let line = |a: &Value| {
        let name = a["name"].as_str()?;
        let (kind, text) = match a["type"].as_u64() {
            Some(1) => (ActivityKind::Streaming, a["details"].as_str().unwrap_or(name)),
            Some(2) => (ActivityKind::Listening, a["state"].as_str().unwrap_or(name)),
            Some(3) => (ActivityKind::Watching, name),
            Some(5) => (ActivityKind::Competing, name),
            _ => (ActivityKind::Playing, name),
        };
        Some((kind, text.to_owned()))
    };
    let mut lines = others.iter().filter_map(|a| line(a));
    let first = lines.next().map(|(kind, text)| Activity { kind, text, rest: lines.collect() });
    let rich = others.first().and_then(|a| parse_rich(a));
    let clients = &p["client_status"];
    let mobile_only = !clients["mobile"].is_null() && clients["desktop"].is_null() && clients["web"].is_null();
    let streaming = others.iter().any(|a| a["type"] == 1);
    Presence { status: Status::parse(p["status"].as_str().unwrap_or("offline")), custom, activity: first, rich, mobile_only, streaming }
}

// Field names from the diself crate's model (src/model/user.rs), not from Discord's docs (the endpoint is undocumented).
pub(crate) fn parse_profile(user_id: &str, body: &Value) -> Profile {
    let user = &body["user"];
    let profile = &body["user_profile"];
    let banner = user["banner"].as_str().map(|hash| {
        let ext = if hash.starts_with("a_") { "gif" } else { "png" };
        format!("{CDN}/banners/{user_id}/{hash}.{ext}?size=600")
    });
    let theme = match profile["theme_colors"].as_array().map(|c| c.iter().filter_map(Value::as_u64).collect::<Vec<_>>()).as_deref() {
        Some([top, bottom]) => Some((*top as u32, *bottom as u32)),
        _ => None,
    };
    let badges = body["badges"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|b| Some(Badge { description: b["description"].as_str().unwrap_or_default().to_owned(), icon: format!("{CDN}/badge-icons/{}.png", b["icon"].as_str()?) }))
        .collect();
    let mutual_friends: Vec<User> = body["mutual_friends"].as_array().into_iter().flatten().map(parse_user).collect();
    let mutual_friend_count = body["mutual_friend_count"].as_u64().or(body["mutual_friends_count"].as_u64()).map_or(mutual_friends.len(), |n| n as usize);
    Profile {
        username: user["username"].as_str().unwrap_or_default().to_owned(),
        bio: profile["bio"].as_str().or(user["bio"].as_str()).filter(|b| !b.is_empty()).map(str::to_owned),
        pronouns: profile["pronouns"].as_str().filter(|p| !p.is_empty()).map(str::to_owned),
        banner,
        accent: user["accent_color"].as_u64().or(user["banner_color"].as_str().and_then(|c| u32::from_str_radix(c.trim_start_matches('#'), 16).ok().map(u64::from))).map(|c| c as u32),
        roles: strings(&body["guild_member"]["roles"]),
        decoration: user.is_object().then(|| parse_user(user).decoration).flatten(),
        theme,
        badges,
        mutual_friends,
        mutual_friend_count,
        mutual_server_count: body["mutual_guilds"].as_array().map_or(0, Vec::len),
        games: Vec::new(),
        joined: body["guild_member"]["joined_at"].as_str().map(str::to_owned),
        connections: body["connected_accounts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| {
                Some(Connection { kind: c["type"].as_str()?.to_owned(), id: c["id"].as_str()?.to_owned(), name: c["name"].as_str().unwrap_or_default().to_owned(), verified: c["verified"].as_bool() == Some(true) })
            })
            .collect(),
        effect_sku: effect_sku(&body["guild_member_profile"]["collectibles"]).or_else(|| effect_sku(&profile["collectibles"])),
    }
}

// The unexpired profile effect in a `collectibles[]` list, `{sku_id, type, expires_at}`.
// Not verified: that the profile effect's collectible type is 1 (Discord's enum value wasn't read).
fn effect_sku(collectibles: &Value) -> Option<String> {
    const PROFILE_EFFECT: u64 = 1;
    let now = chrono::Utc::now();
    collectibles
        .as_array()?
        .iter()
        .filter(|c| c["type"].as_u64() == Some(PROFILE_EFFECT))
        .filter(|c| c["expires_at"].as_str().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).is_none_or(|t| t > now))
        .find_map(|c| c["sku_id"].as_str().map(str::to_owned))
}

// Not verified: that the product carries the effect's config (`effects[]` of `{src, width, height, position,
// zIndex, randomizedSources}`) on one of its `items`; a body without it gives no layers and the card draws none.
// GAP: Discord picks one random `randomizedSources` entry per open; this takes the first.
fn parse_profile_effect(body: &Value) -> Vec<EffectLayer> {
    let item = body["items"].as_array().into_iter().flatten().find(|i| i["effects"].is_array());
    let Some(effects) = item.and_then(|i| i["effects"].as_array()) else { return Vec::new() };
    let mut layers: Vec<EffectLayer> = effects
        .iter()
        .filter_map(|e| {
            let src = e["randomizedSources"][0]["src"].as_str().or(e["src"].as_str())?.to_owned();
            let num = |v: &Value| v.as_f64().unwrap_or(0.) as f32;
            Some(EffectLayer { src, x: num(&e["position"]["x"]), y: num(&e["position"]["y"]), width: num(&e["width"]), height: num(&e["height"]), z: e["zIndex"].as_i64().unwrap_or(0) })
        })
        .filter(|l| l.width > 0. && l.height > 0.)
        .collect();
    layers.sort_by_key(|l| l.z);
    layers
}

pub fn app_icon_url(app_id: &str, icon: &str) -> String {
    format!("{CDN}/app-icons/{app_id}/{icon}.png?size=64")
}

// Large asset keys: "mp:…" is a media proxy path, "spotify:…" an album cover, anything else an app asset id.
fn activity_image(a: &Value) -> Option<String> {
    let key = a["assets"]["large_image"].as_str()?;
    if let Some(path) = key.strip_prefix("mp:") {
        return Some(format!("https://media.discordapp.net/{path}"));
    }
    // Not verified: Spotify covers resolve on Spotify's image CDN, as Discord's activity asset docs describe.
    if let Some(id) = key.strip_prefix("spotify:") {
        return Some(format!("https://i.scdn.co/image/{id}"));
    }
    Some(format!("{CDN}/app-assets/{}/{key}.png", a["application_id"].as_str()?))
}

fn parse_rich(a: &Value) -> Option<RichPresence> {
    const JOIN: u64 = 1 << 1;
    let text = |k: &str| a[k].as_str().filter(|t| !t.is_empty()).map(str::to_owned);
    let size = a["party"]["size"].as_array().map(|s| s.iter().filter_map(Value::as_u64).collect::<Vec<_>>());
    Some(RichPresence {
        kind: match a["type"].as_u64() {
            Some(1) => ActivityKind::Streaming,
            Some(2) => ActivityKind::Listening,
            Some(3) => ActivityKind::Watching,
            Some(5) => ActivityKind::Competing,
            _ => ActivityKind::Playing,
        },
        name: text("name")?,
        app_id: text("application_id"),
        details: text("details"),
        state: text("state"),
        image: activity_image(a),
        party: match size.as_deref() {
            Some([current, max]) => Some((*current, *max)),
            _ => None,
        },
        started: a["timestamps"]["start"].as_i64(),
        ends: a["timestamps"]["end"].as_i64(),
        party_id: a["party"]["id"].as_str().map(str::to_owned),
        joinable: a["party"]["id"].is_string() && (a["flags"].as_u64().unwrap_or(0) & JOIN != 0 || a["secrets"]["join"].is_string()),
    })
}

fn parse_list_op(op: &Value, guild_id: Option<&str>) -> Option<ListOp> {
    let index = op["index"].as_u64().map(|i| i as usize);
    Some(match op["op"].as_str()? {
        "SYNC" => ListOp::Sync {
            start: op["range"][0].as_u64().unwrap_or(0) as usize,
            items: op["items"].as_array().into_iter().flatten().filter_map(|item| parse_list_item(item, guild_id)).collect(),
        },
        "INSERT" => ListOp::Insert { index: index?, item: parse_list_item(&op["item"], guild_id)? },
        "UPDATE" => ListOp::Update { index: index?, item: parse_list_item(&op["item"], guild_id)? },
        "DELETE" => ListOp::Delete { index: index? },
        _ => return None,
    })
}

fn parse_list_item(item: &Value, guild_id: Option<&str>) -> Option<ListItem> {
    if let Some(group) = item["group"].as_object() {
        return Some(ListItem::Group { id: group.get("id")?.as_str()?.to_owned(), count: group.get("count").and_then(Value::as_u64).unwrap_or(0) as u32 });
    }
    let m = &item["member"];
    m.is_object().then(|| ListItem::Member(Member { user: parse_member_user(m, &m["user"], guild_id), roles: strings(&m["roles"]), presence: parse_presence(&m["presence"]), boosting_since: m["premium_since"].as_str().map(str::to_owned) }))
}

fn parse_voice_state(v: &Value) -> VoiceState {
    let user = &v["member"]["user"];
    VoiceState {
        guild_id: v["guild_id"].as_str().map(str::to_owned),
        channel_id: v["channel_id"].as_str().map(str::to_owned),
        user_id: v["user_id"].as_str().unwrap_or_default().to_owned(),
        user: user.is_object().then(|| parse_member_user(&v["member"], user, v["guild_id"].as_str())),
        muted: v["self_mute"].as_bool().unwrap_or(false) || v["mute"].as_bool().unwrap_or(false),
        deafened: v["self_deaf"].as_bool().unwrap_or(false) || v["deaf"].as_bool().unwrap_or(false),
        server_muted: v["mute"].as_bool().unwrap_or(false),
        server_deafened: v["deaf"].as_bool().unwrap_or(false),
        streaming: v["self_stream"].as_bool().unwrap_or(false),
        camera: v["self_video"].as_bool().unwrap_or(false),
        suppress: v["suppress"].as_bool().unwrap_or(false),
        requested_at: v["request_to_speak_timestamp"].as_str().map(str::to_owned),
    }
}

fn parse_emoji(e: &Value) -> Emoji {
    Emoji { id: e["id"].as_str().map(str::to_owned), name: e["name"].as_str().unwrap_or("?").to_owned(), animated: e["animated"].as_bool().unwrap_or(false) }
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_owned)).collect()
}

// Permission bitfields arrive as strings (they overflow JS numbers); older payloads used numbers.
fn permission(v: &Value) -> u64 {
    v.as_str().and_then(|s| s.parse().ok()).or(v.as_u64()).unwrap_or(0)
}

// `GET /content-inventory/users/@me` answers `{ entries: [{ content }] }`; each content is a Content
// Inventory Entry as discord-userdoccers documents it (resources/components). Not verified against a
// live response: Waffle never signs in for development.
pub fn parse_activity_feed(body: &Value) -> Vec<FeedEntry> {
    body["entries"].as_array().into_iter().flatten().filter_map(|e| parse_feed_entry(&e["content"])).collect()
}

fn parse_feed_entry(c: &Value) -> Option<FeedEntry> {
    let extra = &c["extra"];
    let text = |v: &Value| v.as_str().map(str::trim).filter(|t| !t.is_empty()).map(str::to_owned);
    let url = |v: &Value| text(v).filter(|u| u.starts_with("https://"));
    let media = if extra["entries"][0]["media"].is_object() { &extra["entries"][0]["media"] } else { extra };
    let (kind, title, image) = match c["content_type"].as_u64()? {
        1 | 3 => (ActivityKind::Playing, text(&extra["game_name"])?, None),
        2 => (ActivityKind::Watching, text(&extra["media_title"])?, url(&extra["media_assets_large_image"])),
        4 | 5 => (ActivityKind::Listening, text(&media["title"])?, url(&media["image_url"])),
        8 => (ActivityKind::Playing, text(&extra["activity_name"])?, None),
        _ => return None,
    };
    let traits = c["traits"].as_array().cloned().unwrap_or_default();
    let trait_of = |kind: u64| traits.iter().find(|t| t["type"].as_u64() == Some(kind)).cloned();
    let time = |v: &Value| v.as_str().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).map(|t| t.timestamp_millis());
    let made = (snowflake(&c["id"]) >> 22) as i64 + 1_420_070_400_000;
    Some(FeedEntry {
        author_id: text(&c["author_id"])?,
        kind,
        title,
        image,
        app_id: text(&extra["application_id"]),
        live: trait_of(3).is_some_and(|t| t["is_live"].as_bool() == Some(true)),
        at: time(&c["ended_at"]).or_else(|| time(&c["started_at"])).unwrap_or(made),
        streak_days: trait_of(8).and_then(|t| t["streak_count_days"].as_u64()).filter(|&d| d > 1),
    })
}

pub fn snowflake(v: &Value) -> u64 {
    v.as_str().and_then(|s| s.parse().ok()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_info_carries_start_times_and_statuses() {
        let info = json!({ "guild_id": "g", "channels": [{ "id": "a", "status": "lofi <:waffle:42>", "voice_start_time": 1700 }, { "id": "b", "status": null }] });
        let Some(Event::Batch(events)) = dispatch("CHANNEL_INFO", &info) else { panic!("a batch") };
        let [Event::VoiceStartTimes { guild_id, times, all: true }, Event::VoiceStatuses { statuses, all: true, .. }] = events.as_slice() else { panic!("times then statuses") };
        assert_eq!(guild_id, "g");
        assert_eq!(times, &[("a".to_owned(), Some(1700)), ("b".to_owned(), None)]);
        assert_eq!(statuses, &[("a".to_owned(), "lofi <:waffle:42>".to_owned()), ("b".to_owned(), String::new())]);
        let update = json!({ "id": "a", "guild_id": "g", "status": "" });
        let Some(Event::VoiceStatuses { guild_id, statuses, all: false }) = dispatch("VOICE_CHANNEL_STATUS_UPDATE", &update) else { panic!("one status") };
        assert_eq!((guild_id.as_str(), statuses), ("g", vec![("a".to_owned(), String::new())]));
    }

    #[test]
    fn display_name_styles_parse_with_member_override() {
        let user = serde_json::json!({ "id": "1", "global_name": "Yumiko", "display_name_styles": { "font_id": 12, "effect_id": 2, "colors": [2797222, 16762000, -1, 16777216] } });
        let style = parse_user(&user).name_style.expect("styled");
        assert_eq!(style, NameStyle { font: 12, effect: NameEffect::Gradient, colors: vec![0x2AAEA6, 0xFFC490] }, "out-of-range colours dropped");
        assert_eq!(parse_user(&serde_json::json!({ "id": "1", "display_name_styles": null })).name_style, None);
        let unknown = serde_json::json!({ "id": "1", "display_name_styles": { "effect_id": 1001 } });
        assert_eq!(parse_user(&unknown).name_style, Some(NameStyle { font: 11, effect: NameEffect::Solid, colors: vec![] }));
        let member = serde_json::json!({ "display_name_styles": { "font_id": 3, "effect_id": 8, "colors": [255] } });
        assert_eq!(parse_member_user(&member, &user, Some("g")).name_style.map(|s| s.effect), Some(NameEffect::Gummy), "the server's style wins");
    }

    #[test]
    fn pin_notice_names_the_pinned_message() {
        let notice = json!({ "id": "9", "channel_id": "1", "type": 6, "author": { "id": "5", "username": "frsty" }, "message_reference": { "message_id": "3" } });
        let notice = parse_message(&notice).expect("parses");
        assert!(notice.pin_notice);
        assert_eq!(notice.reply_to.as_deref(), Some("3"));
        assert!(!parse_message(&json!({ "id": "3", "channel_id": "1", "type": 0, "author": { "id": "5" } })).expect("parses").pin_notice);
    }

    #[test]
    fn send_options_ride_on_the_body_and_come_back_parsed() {
        let mut body = message_body("hi", "n", None, false, &[]);
        SendOptions { silent: true, tts: true }.apply(&mut body);
        assert_eq!((body["flags"].as_u64(), body["tts"].as_bool()), (Some(4096), Some(true)));
        let mut plain = message_body("hi", "n", None, false, &[]);
        SendOptions::default().apply(&mut plain);
        assert!(plain.get("flags").is_none() && plain.get("tts").is_none());
        let sent = parse_message(&json!({ "id": "1", "channel_id": "c", "author": { "id": "u" }, "flags": 4096, "tts": true })).expect("parses");
        assert_eq!(sent.send_options(), SendOptions { silent: true, tts: true });
    }

    fn upload(name: &str, size: u64) -> Upload {
        Upload { path: PathBuf::from(format!("/tmp/{name}")), name: name.into(), size, description: None, spoiler: false, sent: Default::default() }
    }

    #[test]
    fn http_failures_retry_only_where_discord_allows() {
        let limited = json!({ "message": "You are being rate limited.", "retry_after": 0.5, "global": false });
        assert_eq!(retry_wait(429, &limited, None, false, 0), Some(Duration::from_millis(500)), "429 retries even a POST");
        assert_eq!(retry_wait(429, &limited, None, false, 2), None, "two retries at most");
        assert_eq!(retry_wait(429, &json!({ "retry_after": 60.0 }), None, true, 0), None, "too long to wait");
        assert_eq!(retry_wait(429, &Value::Null, Some(2.), true, 0), Some(Duration::from_secs(2)), "Retry-After header");
        assert_eq!(retry_wait(502, &Value::Null, None, true, 0), Some(Duration::from_secs(1)));
        assert_eq!(retry_wait(502, &Value::Null, None, false, 0), None, "a POST never repeats on 5xx");
        assert_eq!(retry_wait(502, &Value::Null, None, true, 1), None);
        for status in [400, 401, 403, 404, 405] {
            assert_eq!(retry_wait(status, &Value::Null, None, true, 0), None, "{status}");
        }
        assert_eq!(error_text(403, &json!({ "message": "Missing Permissions", "code": 50013 })), "Missing Permissions");
        assert_eq!(error_text(404, &Value::Null), "404 Not Found");
    }

    #[test]
    fn command_index_waits_on_202_and_429() {
        assert_eq!(index_wait(202, &Value::Null, None), Some(Duration::from_secs(5)));
        assert_eq!(index_wait(429, &json!({ "retry_after": 1.5 }), None), Some(Duration::from_millis(1500)));
        assert_eq!(index_wait(200, &Value::Null, None), None);
        assert_eq!(index_wait(500, &Value::Null, None), None);
    }

    #[test]
    fn interaction_refusals_read_as_discords_client_reads_them() {
        let stale = json!({ "code": 50035, "message": "Invalid Form Body",
            "errors": { "data": { "_errors": [{ "code": "INTERACTION_APPLICATION_COMMAND_INVALID_VERSION", "message": "This command is outdated, please try again in a few minutes" }] } } });
        let e = interact_error(400, &stale);
        assert!(e.stale_index);
        assert_eq!(e.message.as_deref(), Some("This command is outdated, please try again in a few minutes"));
        assert_eq!(e.code, None);
        let other = interact_error(403, &json!({ "code": 50001, "message": "Missing Access" }));
        assert_eq!((other.code, other.message.as_deref(), other.stale_index), (Some(50001), Some("Missing Access"), false));
        assert!(interact_error(400, &json!({ "code": 10005, "message": "Unknown Integration" })).stale_index);
        let limited = interact_error(429, &json!({ "message": "You are being rate limited.", "retry_after": 2.0 }));
        assert_eq!((limited.status, limited.retry_after), (Some(429), Some(2.0)));
        assert_eq!(interact_error(500, &Value::Null), InteractError { status: Some(500), ..Default::default() });
    }

    #[test]
    fn every_gateway_close_code_has_its_documented_outcome() {
        for code in [1000, 1001, 1006, 4000, 4001, 4002, 4003, 4005, 4008] {
            assert_eq!(close_kind(code), Close::Resume, "{code}");
        }
        for code in [4007, 4009, 4015, 4016] {
            assert_eq!(close_kind(code), Close::Identify, "{code}");
        }
        for code in 4010..=4014 {
            assert_eq!(close_kind(code), Close::Fatal, "{code}");
        }
        assert_eq!(close_kind(4004), Close::AuthFailed);
        assert_eq!(gateway_close_text(4009, ""), "Session timed out (4009)");
        assert!(is_unauthorized(&error_text(401, &json!({ "message": "401: Unauthorized", "code": 0 }))));
        assert!(!is_unauthorized(&error_text(403, &json!({ "message": "Missing Access", "code": 50001 }))));
    }

    #[test]
    fn pins_parse_both_the_new_and_the_deprecated_shape() {
        let message = |id: &str| json!({ "id": id, "channel_id": "c", "author": { "id": "u" } });
        let new = json!({ "items": [{ "pinned_at": "2026-01-01T00:00:00Z", "message": message("2") }, { "pinned_at": "x", "message": message("1") }], "has_more": false });
        assert_eq!(parse_pins(&new).iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["2", "1"]);
        assert_eq!(parse_pins(&json!([message("3")])).len(), 1);
    }

    #[test]
    fn upload_limit_is_the_higher_of_nitro_and_boosts() {
        assert_eq!(upload_limit(0, 0), 10 << 20);
        assert_eq!(upload_limit(0, 1), 10 << 20, "boost level 1 adds nothing");
        assert_eq!(upload_limit(3, 0), 50 << 20, "Nitro Basic");
        assert_eq!(upload_limit(1, 3), 100 << 20, "the server's level 3 beats Classic");
        assert_eq!(upload_limit(2, 3), 500 << 20, "Nitro beats every server");
    }

    #[test]
    fn asks_for_one_upload_url_per_file() {
        let body = attachments_request(&[upload("cat.png", 1200), upload("SPOILER_notes.txt", 3)]);
        assert_eq!(body, json!({ "files": [{ "id": "0", "filename": "cat.png", "file_size": 1200 }, { "id": "1", "filename": "SPOILER_notes.txt", "file_size": 3 }] }));
    }

    #[test]
    fn upload_urls_come_back_in_request_order() {
        let body = json!({ "attachments": [
            { "id": 1, "upload_url": "https://up/b", "upload_filename": "u/b.txt" },
            { "id": 0, "upload_url": "https://up/a", "upload_filename": "u/a.png" },
        ] });
        assert_eq!(parse_upload_slots(&body, 2).unwrap(), vec![("https://up/a".into(), "u/a.png".into()), ("https://up/b".into(), "u/b.txt".into())]);
        assert!(parse_upload_slots(&body, 3).is_err(), "a missing slot is an error, never a message without the file");
    }

    #[test]
    fn the_message_names_each_uploaded_file() {
        let sent = |filename: &str, description: Option<&str>, spoiler| SentAttachment { filename: filename.into(), uploaded_filename: format!("u/{filename}"), description: description.map(Into::into), spoiler };
        let body = message_body("look", "n1", Some("42"), false, &[sent("cat.png", None, false), sent("end.png", Some("The boss falls"), true), sent("blank.png", Some(""), false)]);
        assert_eq!(
            body["attachments"],
            json!([
                { "id": "0", "filename": "cat.png", "uploaded_filename": "u/cat.png" },
                { "id": "1", "filename": "end.png", "uploaded_filename": "u/end.png", "description": "The boss falls", "is_spoiler": true },
                { "id": "2", "filename": "blank.png", "uploaded_filename": "u/blank.png", "description": "" },
            ]),
            "description only when set, is_spoiler only when true, never a SPOILER_ name"
        );
        assert_eq!(body["message_reference"], json!({ "message_id": "42" }));
        assert_eq!(body["allowed_mentions"]["replied_user"], json!(false));
        assert!(message_body("hi", "n2", None, true, &[]).get("attachments").is_none(), "plain text carries no attachments key");
    }

    #[test]
    fn modifying_a_sent_attachment_lists_every_one() {
        let ids = ["10".to_owned(), "11".to_owned(), "12".to_owned()];
        assert_eq!(
            attachments_patch(&ids, "11", "A pier at dusk", false),
            json!({ "attachments": [{ "id": "10" }, { "id": "11", "description": "A pier at dusk", "is_spoiler": false }, { "id": "12" }] })
        );
        assert_eq!(attachments_patch(&ids[..1], "10", "", true), json!({ "attachments": [{ "id": "10", "description": "", "is_spoiler": true }] }));
    }

    #[test]
    fn reads_alt_text_and_spoiler_flags() {
        let m = parse_message(&json!({ "id": "1", "channel_id": "c", "author": { "id": "u" }, "attachments": [
            { "id": "a1", "url": "a.png", "width": 10, "height": 20, "content_type": "image/png", "description": "A cat", "flags": 8 },
            { "id": "a2", "url": "b.png", "width": 10, "height": 20, "content_type": "image/png", "description": "" },
            { "id": "a3", "url": "c.zip", "filename": "c.zip", "size": 5, "flags": 8 },
        ] }))
        .unwrap();
        assert_eq!((m.media[0].id.as_deref(), m.media[0].description.as_deref(), m.media[0].spoiler), (Some("a1"), Some("A cat"), true));
        assert_eq!((m.media[1].description.as_deref(), m.media[1].spoiler), (None, false));
        assert_eq!((m.files[0].id.as_str(), m.files[0].spoiler), ("a3", true));
    }

    #[test]
    fn reads_the_activity_feed() {
        let body = json!({ "entries": [
            { "content": { "id": "1300000000000000000", "author_id": "7", "content_type": 1, "ended_at": "2026-11-17T10:00:00+00:00",
                "extra": { "type": "played_game_extra", "game_name": "EA SPORTS FC 26" }, "traits": [{ "type": 8, "streak_count_days": 6 }] } },
            { "content": { "id": "1300000000000000001", "author_id": "8", "content_type": 5,
                "extra": { "entries": [{ "media": { "title": "Bad Apple!!", "image_url": "https://i.scdn.co/image/x" } }] }, "traits": [{ "type": 3, "is_live": true }] } },
            { "content": { "id": "1", "author_id": "9", "content_type": 9, "extra": {} } }
        ]});
        let feed = parse_activity_feed(&body);
        assert_eq!(feed.len(), 2);
        assert_eq!((feed[0].title.as_str(), feed[0].streak_days, feed[0].kind), ("EA SPORTS FC 26", Some(6), ActivityKind::Playing));
        assert_eq!(feed[0].at, 1_794_909_600_000);
        assert_eq!((feed[1].kind, feed[1].live, feed[1].image.as_deref()), (ActivityKind::Listening, true, Some("https://i.scdn.co/image/x")));
        assert_eq!(feed[1].at, (1_300_000_000_000_000_001i64 >> 22) + 1_420_070_400_000);
    }

    #[test]
    fn parses_both_ready_shapes() {
        let legacy = json!({ "id": "1", "name": "Rust", "icon": "abc", "channels": [
            { "id": "b", "type": 0, "name": "general", "position": 2 },
            { "id": "a", "type": 4, "name": "Text", "position": 1 },
        ]});
        let guild = parse_guild(&legacy).unwrap();
        assert_eq!(guild.name, "Rust");
        assert_eq!(guild.icon.as_deref(), Some("https://cdn.discordapp.com/icons/1/abc.png?size=96"));
        assert_eq!(guild.channels[0].kind, ChannelKind::Category);

        let deduped = json!({ "id": "2", "properties": { "name": "Zed", "icon": null }, "channels": [] });
        let guild = parse_guild(&deduped).unwrap();
        assert_eq!(guild.name, "Zed");
        assert!(guild.icon.is_none());

        let users = json!([{ "id": "9", "username": "ferris", "global_name": "Ferris", "avatar": "h" }]);
        let dm = parse_dm(&json!({ "id": "d", "type": 1, "recipient_ids": ["9"] }), &users).unwrap();
        assert_eq!(dm.name, "Ferris");
        assert_eq!(dm.recipient.unwrap().avatar, "https://cdn.discordapp.com/avatars/9/h.png?size=128");
    }

    #[test]
    fn group_dm_faces() {
        let users = json!([]);
        let group = |extra: Value| {
            let mut c = json!({ "id": "g", "type": 3, "recipients": [{ "id": "1", "username": "a", "avatar": "x" }, { "id": "2", "username": "b", "avatar": "y" }, { "id": "3", "username": "c" }] });
            c.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            parse_dm(&c, &users).unwrap()
        };
        let plain = group(json!({}));
        assert_eq!(plain.recipients.len(), 3);
        assert_eq!(plain.face(), DmFace::Pair(avatar_url("1", Some("x")), avatar_url("2", Some("y"))));
        assert_eq!(group(json!({ "icon": "i" })).face(), DmFace::Icon("https://cdn.discordapp.com/channel-icons/g/i.png?size=128".into()));

        let left = parse_dm(&json!({ "id": "g", "type": 3, "name": "squad", "recipients": [{ "id": "1", "username": "a", "avatar": "x" }] }), &users).unwrap();
        assert!(left.recipient.is_none());
        assert_eq!(left.face(), DmFace::One(avatar_url("1", Some("x"))));
        let alone = parse_dm(&json!({ "id": "g", "type": 3, "name": "squad", "recipients": [] }), &users).unwrap();
        assert_eq!(alone.face(), DmFace::Letters("squad".into()));
    }

    #[test]
    fn dm_events_and_request_flags() {
        let Some(Event::DmUpsert(dm)) = dispatch("CHANNEL_CREATE", &json!({ "id": "d", "type": 1, "is_message_request": true, "recipients": [{ "id": "1", "username": "a" }] })) else { panic!("a DM create is a DmUpsert") };
        assert!(dm.props.message_request && !dm.props.spam && dm.is_request());
        let Some(Event::DmUpsert(group)) = dispatch("CHANNEL_UPDATE", &json!({ "id": "g", "type": 3, "owner_id": "1", "is_spam": true, "recipients": [] })) else { panic!("a group update is a DmUpsert") };
        assert_eq!(group.props.owner_id.as_deref(), Some("1"));
        assert!(group.is_request());
        assert!(matches!(dispatch("CHANNEL_DELETE", &json!({ "id": "d", "type": 1 })), Some(Event::DmDelete(id)) if id == "d"));
        assert!(matches!(dispatch("CHANNEL_CREATE", &json!({ "id": "c", "type": 0, "guild_id": "s" })), Some(Event::ChannelUpsert { .. })));
        let Some(Event::RecipientChange { channel_id, user, added }) = dispatch("CHANNEL_RECIPIENT_REMOVE", &json!({ "channel_id": "g", "user": { "id": "2", "username": "b" } })) else { panic!("recipient remove") };
        assert_eq!((channel_id.as_str(), user.id.as_str(), added), ("g", "2", false));
    }

    #[test]
    fn default_avatar_when_hash_missing() {
        assert_eq!(avatar_url("4194304", None), "https://cdn.discordapp.com/embed/avatars/1.png");
        assert_eq!(avatar_url("1", Some("0")), "https://cdn.discordapp.com/embed/avatars/0.png");
    }

    #[test]
    fn sizes_images_through_the_media_proxy() {
        let signed = "https://cdn.discordapp.com/attachments/1/2/shot.png?ex=a&is=b&hm=c&";
        assert_eq!(
            sized_image(signed, 400., 225., (3840., 2160.)),
            "https://media.discordapp.net/attachments/1/2/shot.png?ex=a&is=b&hm=c&&width=800&height=450"
        );
        assert_eq!(sized_image("https://cdn.discordapp.com/attachments/1/2/a.png", 100., 100., (150., 150.)), "https://media.discordapp.net/attachments/1/2/a.png", "never upscaled");
        assert_eq!(sized_image("https://example.com/a.png", 10., 10., (999., 999.)), "https://example.com/a.png");
    }

    #[test]
    fn thumbnails_cover_a_square_through_the_proxy() {
        let photo = Media { url: "https://cdn.discordapp.com/attachments/1/2/a.png?ex=a&is=b&hm=c&".into(), width: 4000., height: 3000., kind: MediaKind::Image, ..Default::default() };
        assert_eq!(thumb_url(&photo, 112.), "https://media.discordapp.net/attachments/1/2/a.png?ex=a&is=b&hm=c&&width=149&height=112&format=webp", "shorter edge fills the square");
        let tall = Media { width: 300., height: 1200., ..photo.clone() };
        assert!(thumb_url(&tall, 112.).ends_with("width=112&height=448&format=webp"));
        let small = Media { width: 64., height: 64., ..photo.clone() };
        assert_eq!(thumb_url(&small, 112.), "https://media.discordapp.net/attachments/1/2/a.png?ex=a&is=b&hm=c&&format=webp", "never upscaled");
        let clip = Media { url: "https://cdn.discordapp.com/attachments/1/2/v.mp4".into(), width: 1920., height: 1080., kind: MediaKind::Video { poster: "https://media.discordapp.net/attachments/1/2/v.mp4?format=jpeg".into() }, ..Default::default() };
        assert_eq!(thumb_url(&clip, 96.), "https://media.discordapp.net/attachments/1/2/v.mp4?format=jpeg&width=171&height=96", "video keeps its proxy still, sized");
        let elsewhere = Media { url: "https://example.com/a.png".into(), ..photo };
        assert_eq!(thumb_url(&elsewhere, 96.), "https://example.com/a.png");
    }

    #[test]
    fn locks_channels_hidden_from_everyone() {
        let hidden = json!({ "id": "c", "type": 0, "permission_overwrites": [{ "id": "g", "type": 0, "deny": "1024", "allow": "0" }] });
        let open = json!({ "id": "c", "type": 0, "permission_overwrites": [{ "id": "g", "type": 0, "deny": "2048", "allow": "0" }] });
        assert!(parse_channel(&hidden, "g").unwrap().private);
        assert!(!parse_channel(&open, "g").unwrap().private);
    }

    #[test]
    fn form_errors_name_the_field() {
        let body = json!({ "code": 50035, "message": "Invalid Form Body", "errors": { "name": { "_errors": [{ "code": "BASE_TYPE_BAD_LENGTH", "message": "Must be between 1 and 100 in length." }] } } });
        assert_eq!(api_error(&body).as_deref(), Some("Invalid Form Body: name: Must be between 1 and 100 in length."));
        assert_eq!(api_error(&json!({ "message": "Missing Permissions", "code": 50013 })).as_deref(), Some("Missing Permissions"));
        assert_eq!(api_error(&Value::Null), None);
    }

    #[test]
    fn joined_invite_names_its_server() {
        assert_eq!(joined_guild(&json!({ "code": "x", "guild": { "id": "7" } })).as_deref(), Some("7"));
        assert_eq!(joined_guild(&json!({ "code": "x", "guild_id": "7" })).as_deref(), Some("7"));
        assert_eq!(joined_guild(&json!({ "code": "x", "channel": { "id": "9" } })), None);
    }

    #[test]
    fn hides_channels_we_cannot_view() {
        let role = |id: &str, permissions: u64| Role { id: id.into(), name: id.into(), color: 0, position: 0, permissions, managed: false };
        let channel = |id: &str, overwrites: Value| parse_channel(&json!({ "id": id, "type": 0, "permission_overwrites": overwrites }), "g").unwrap();
        let deny_everyone = json!({ "id": "g", "type": 0, "deny": "1024", "allow": "0" });
        let guild = Guild {
            id: "g".into(),
            name: "g".into(),
            icon: None,
            channels: vec![
                channel("open", json!([])),
                channel("staff", json!([deny_everyone, { "id": "mods", "type": 0, "allow": "1024", "deny": "0" }])),
                channel("mine", json!([deny_everyone, { "id": "me", "type": 1, "allow": "1024", "deny": "0" }])),
                channel("secret", json!([deny_everyone])),
                channel("kicked", json!([{ "id": "me", "type": 1, "allow": "0", "deny": "1024" }])),
            ],
            roles: vec![role("g", VIEW_CHANNEL), role("mods", 0), role("admins", perm::ADMINISTRATOR)],
            voice_states: Vec::new(),
            stage_instances: Vec::new(),
            emojis: Vec::new(),
            stickers: Vec::new(),
            owner_id: Some("owner".into()),
            premium_tier: 0,
            afk_channel_id: None,
            timed_out_until: None,
            system_channel_flags: 0,
            events: Vec::new(),
            public_updates_channel_id: None,
            vanity_url_code: None,
            threads: Vec::new(),
            my_roles: Vec::new(),
            default_notifications: Default::default(),
            ..Default::default()
        };
        fn visible(g: &Guild) -> Vec<&str> {
            g.channels.iter().filter(|c| g.can_view("me", c)).map(|c| c.id.as_str()).collect()
        }
        assert_eq!(visible(&guild), ["open", "mine"]);
        let modded = Guild { my_roles: vec!["mods".into()], ..guild.clone() };
        assert_eq!(visible(&modded), ["open", "staff", "mine"]);
        let admin = Guild { my_roles: vec!["admins".into()], ..guild.clone() };
        assert_eq!(visible(&admin).len(), 5);
        assert!(guild.can_view("owner", &guild.channels[3]));
    }

    #[test]
    fn manage_messages_follows_owner_admin_roles_and_overwrites() {
        let role = |id: &str, permissions: u64| Role { id: id.into(), name: id.into(), color: 0, position: 0, permissions, managed: false };
        let channel = |id: &str, overwrites: Value| parse_channel(&json!({ "id": id, "type": 0, "permission_overwrites": overwrites }), "g").unwrap();
        let thread = parse_channel(&json!({ "id": "t", "type": 11, "parent_id": "locked", "thread_metadata": {} }), "g").unwrap();
        let guild = Guild {
            id: "g".into(),
            name: "g".into(),
            icon: None,
            channels: vec![
                channel("open", json!([])),
                channel("locked", json!([{ "id": "mods", "type": 0, "allow": "0", "deny": "8192" }])),
                channel("mine", json!([{ "id": "mods", "type": 0, "allow": "0", "deny": "8192" }, { "id": "me", "type": 1, "allow": "8192", "deny": "0" }])),
                channel("everyone", json!([{ "id": "g", "type": 0, "allow": "8192", "deny": "0" }])),
            ],
            roles: vec![role("g", VIEW_CHANNEL), role("mods", perm::MANAGE_MESSAGES), role("admins", perm::ADMINISTRATOR)],
            voice_states: Vec::new(),
            stage_instances: Vec::new(),
            emojis: Vec::new(),
            stickers: Vec::new(),
            owner_id: Some("owner".into()),
            premium_tier: 0,
            afk_channel_id: None,
            timed_out_until: None,
            system_channel_flags: 0,
            events: Vec::new(),
            public_updates_channel_id: None,
            vanity_url_code: None,
            threads: vec![thread],
            my_roles: Vec::new(),
            default_notifications: Default::default(),
            ..Default::default()
        };
        let can = |g: &Guild, me: &str, id: &str| g.channel_permissions(me, g.channel(id).unwrap()) & perm::MANAGE_MESSAGES != 0;
        let all = ["open", "locked", "mine", "everyone", "t"];
        let allowed = |g: &Guild, me: &str| all.iter().copied().filter(|id| can(g, me, id)).collect::<Vec<_>>();

        assert_eq!(allowed(&guild, "me"), ["mine", "everyone"]);
        assert_eq!(allowed(&guild, "owner"), all);
        assert_eq!(allowed(&Guild { my_roles: vec!["admins".into()], ..guild.clone() }, "me"), all);
        // The role grants it server-wide; a role overwrite takes it back, a member overwrite beats that, and threads follow their parent.
        assert_eq!(allowed(&Guild { my_roles: vec!["mods".into()], ..guild.clone() }, "me"), ["open", "mine", "everyone"]);
    }

    #[test]
    fn voice_moves_and_invites_follow_channel_permissions() {
        let role = |id: &str, permissions: u64| Role { id: id.into(), name: id.into(), color: 0, position: 0, permissions, managed: false };
        let channel = |id: &str, kind: u8, limit: u32, overwrites: Value| parse_channel(&json!({ "id": id, "type": kind, "user_limit": limit, "permission_overwrites": overwrites }), "g").unwrap();
        let in_voice = |user: &str, channel: &str| VoiceState { user_id: user.into(), channel_id: Some(channel.into()), ..Default::default() };
        let deny = |bits: u64| json!([{ "id": "mods", "type": 0, "allow": "0", "deny": bits.to_string() }]);
        let guild = Guild {
            id: "g".into(),
            name: "g".into(),
            icon: None,
            channels: vec![
                channel("rules", 0, 0, deny(perm::CREATE_INSTANT_INVITE)),
                channel("chat", 0, 0, json!([])),
                channel("a", 2, 0, json!([])),
                channel("b", 2, 0, json!([])),
                channel("full", 2, 1, json!([])),
                channel("locked", 2, 0, deny(perm::MOVE_MEMBERS)),
                channel("closed", 2, 0, deny(perm::CONNECT)),
            ],
            roles: vec![role("g", VIEW_CHANNEL | perm::CONNECT | perm::CREATE_INSTANT_INVITE), role("mods", perm::MOVE_MEMBERS)],
            voice_states: vec![in_voice("them", "a"), in_voice("other", "full"), in_voice("held", "locked")],
            emojis: Vec::new(),
            stickers: Vec::new(),
            owner_id: Some("owner".into()),
            premium_tier: 0,
            afk_channel_id: None,
            timed_out_until: None,
            system_channel_flags: 0,
            events: Vec::new(),
            public_updates_channel_id: None,
            vanity_url_code: None,
            threads: Vec::new(),
            my_roles: vec!["mods".into()],
            default_notifications: Default::default(),
            ..Default::default()
        };
        let to = |g: &Guild, me: &str, who: &str| ["b", "full", "locked", "closed"].into_iter().filter(|id| g.can_move(me, who, g.channel(id).unwrap())).collect::<Vec<_>>();

        // Move members on both ends and Connect on the destination; full channels are out.
        assert_eq!(to(&guild, "me", "them"), ["b"]);
        // Not from a channel where Move members is denied.
        assert_eq!(to(&guild, "me", "held"), Vec::<&str>::new());
        // Without the role, nobody else moves.
        assert_eq!(to(&Guild { my_roles: Vec::new(), ..guild.clone() }, "me", "them"), Vec::<&str>::new());
        // Ourselves: Connect is enough.
        assert_eq!(to(&guild, "me", "me"), ["b", "locked"]);
        assert_eq!(to(&guild, "owner", "them"), ["b", "locked", "closed"]);

        assert_eq!(guild.invite_channel("me").map(|c| c.id.as_str()), Some("chat"));
        assert_eq!(guild.invite_channel("owner").map(|c| c.id.as_str()), Some("rules"));
        let no_invites = Guild { roles: vec![role("g", VIEW_CHANNEL)], my_roles: Vec::new(), ..guild.clone() };
        assert_eq!(no_invites.invite_channel("me").map(|c| c.id.as_str()), None);
    }

    #[test]
    fn sorts_attachments_into_images_and_files() {
        let m = parse_message(&json!({ "id": "1", "channel_id": "c", "author": { "id": "u" }, "attachments": [
            { "url": "a.png", "width": 10, "height": 20, "content_type": "image/png", "description": "a red door" },
            { "url": "b.zip", "filename": "b.zip", "size": 5 },
        ], "referenced_message": { "id": "0", "channel_id": "c", "author": { "id": "v", "username": "zoe" }, "content": "hi" },
           "message_reference": { "message_id": "0" } }))
        .unwrap();
        assert_eq!(m.media.len(), 1);
        assert_eq!(m.media[0].description.as_deref(), Some("a red door"));
        assert_eq!(m.files[0].name, "b.zip");
        assert_eq!(m.reply.unwrap().author.name, "zoe");
        assert_eq!(m.reply_to.as_deref(), Some("0"));
    }

    #[test]
    fn folds_poll_counts_into_answers() {
        let m = parse_message(&json!({ "id": "1", "channel_id": "c", "author": { "id": "u" }, "poll": {
            "question": { "text": "Map?" }, "expiry": "2026-11-20T21:00:00Z", "allow_multiselect": true,
            "answers": [
                { "answer_id": 1, "poll_media": { "text": "Peaks", "emoji": { "name": "🏔️" } } },
                { "answer_id": 2, "poll_media": { "text": "Harbor" } },
            ],
            "results": { "is_finalized": false, "answer_counts": [{ "id": 1, "count": 3, "me_voted": true }] },
        } }))
        .unwrap();
        let poll = m.poll.unwrap();
        assert_eq!((poll.question.as_str(), poll.multiselect, poll.finalized), ("Map?", true, false));
        assert_eq!(poll.answers[0], PollAnswer { id: 1, text: "Peaks".into(), emoji: Some(Emoji::unicode("🏔️")), count: 3, me: true });
        assert_eq!((poll.answers[1].count, poll.answers[1].me, poll.answers[1].emoji.is_none()), (0, false, true));
    }

    #[test]
    fn a_forward_carries_its_snapshot_and_is_no_reply() {
        let m = parse_message(&json!({ "id": "2", "channel_id": "dest", "author": { "id": "u" }, "content": "",
            "message_reference": { "type": 1, "message_id": "9", "channel_id": "src", "guild_id": "g" },
            "message_snapshots": [{ "message": { "content": "look", "timestamp": "2026-11-19T23:02:00+00:00",
                "attachments": [{ "url": "a.png", "width": 10, "height": 20, "content_type": "image/png" }] } }] }))
        .unwrap();
        assert_eq!(m.reply_to, None, "a type-1 reference is a forward, not a reply");
        let original = m.forward.expect("forward");
        assert_eq!((original.id.as_str(), original.channel_id.as_str(), original.guild_id.as_deref()), ("9", "src", Some("g")));
        assert_eq!(original.content, "look");
        assert_eq!(original.media.len(), 1);

        let body = forward_body(&original, Some("g"), "n");
        assert_eq!(body["message_reference"], json!({ "type": 1, "message_id": "9", "channel_id": "src", "guild_id": "g" }));
        assert!(body.get("content").is_none());
    }

    #[test]
    fn reads_voice_messages() {
        let m = parse_message(&json!({ "id": "1", "channel_id": "c", "author": { "id": "u" }, "flags": 8192,
            "attachments": [{ "id": "9", "filename": "voice-message.ogg", "url": "https://cdn/v.ogg", "size": 9000, "duration_secs": 4.2, "waveform": "AP8=" }] }))
        .unwrap();
        assert!(m.is_voice());
        let file = &m.files[0];
        assert_eq!((file.id.as_str(), file.duration_secs, file.waveform.as_slice()), ("9", Some(4.2), [0u8, 255].as_slice()));
        assert!(file.is_audio());
        let plain = parse_message(&json!({ "id": "2", "channel_id": "c", "author": { "id": "u" },
            "attachments": [{ "filename": "notes.PDF", "url": "u", "size": 1, "waveform": "not base64!" }] }))
        .unwrap();
        assert!(!plain.is_voice() && !plain.files[0].is_audio() && plain.files[0].waveform.is_empty());
    }

    #[test]
    fn sorts_embeds_by_kind() {
        let m = parse_message(&json!({ "id": "1", "channel_id": "c", "author": { "id": "u" },
            "attachments": [{ "url": "v.mp4", "proxy_url": "https://media/v.mp4", "width": 640, "height": 360, "content_type": "video/mp4" }],
            "embeds": [
                { "type": "gifv", "thumbnail": { "url": "t.png", "width": 200, "height": 100 }, "video": { "url": "g.mp4" } },
                { "type": "image", "thumbnail": { "url": "i.png", "width": 50, "height": 50 } },
                { "type": "video", "title": "Clip", "provider": { "name": "YouTube" }, "thumbnail": { "url": "y.jpg", "width": 1280, "height": 720 } },
            ] }))
        .unwrap();
        assert_eq!(m.media.len(), 3);
        assert_eq!(m.media[0].kind, MediaKind::Video { poster: "https://media/v.mp4?format=jpeg".into() });
        assert_eq!(m.media[1].url, "g.mp4");
        assert_eq!(m.media[1].kind, MediaKind::Gif { poster: "t.png".into() });
        assert_eq!(m.media_files.iter().map(|f| f.url.as_str()).collect::<Vec<_>>(), ["v.mp4"], "only uploads have files");
        assert_eq!(m.embeds.len(), 1);
        assert_eq!(m.embeds[0].provider.as_deref(), Some("YouTube"));
    }

    fn embedded() -> Value {
        json!({ "id": "1", "channel_id": "c", "author": { "id": "u" }, "content": "look",
            "attachments": [{ "url": "a.png", "width": 10, "height": 10, "content_type": "image/png" }],
            "embeds": [
                { "type": "image", "thumbnail": { "url": "i.png", "width": 50, "height": 50 } },
                { "type": "article", "url": "https://a", "title": "A" },
            ] })
    }

    #[test]
    fn suppressed_embeds_parse_to_none_and_keep_attachments() {
        let mut m = embedded();
        m["flags"] = json!(4);
        let m = parse_message(&m).unwrap();
        assert!(m.embeds_suppressed());
        assert!(m.embeds.is_empty());
        assert_eq!(m.media.iter().map(|m| m.url.as_str()).collect::<Vec<_>>(), ["a.png"]);
    }

    #[test]
    fn update_with_suppress_flag_clears_embeds_and_their_media() {
        let mut m = parse_message(&embedded()).unwrap();
        assert_eq!((m.embeds.len(), m.media.len(), m.embed_media), (1, 2, 1));
        apply_update(&mut m, &json!({ "id": "1", "channel_id": "c", "flags": 4 }));
        assert!(m.embeds.is_empty());
        assert_eq!(m.media.iter().map(|m| m.url.as_str()).collect::<Vec<_>>(), ["a.png"]);
        assert_eq!((m.flags, m.content.as_str(), m.edited), (4, "look", false));
    }

    #[test]
    fn late_embeds_update_replaces_the_old_ones() {
        let mut m = parse_message(&embedded()).unwrap();
        apply_update(&mut m, &json!({ "id": "1", "channel_id": "c", "embeds": [{ "type": "article", "url": "https://b", "title": "B" }] }));
        assert_eq!(m.embeds.iter().map(|e| e.title.as_deref()).collect::<Vec<_>>(), [Some("B")]);
        assert_eq!(m.media.iter().map(|m| m.url.as_str()).collect::<Vec<_>>(), ["a.png"]);
    }

    #[test]
    fn inline_embeds_are_bare_media_without_a_title_or_author() {
        let pic = || json!({ "url": "p", "width": 100, "height": 50 });
        let inline = |e: Value| parse_embed(&e).inline();
        assert!(inline(json!({ "type": "video", "thumbnail": pic(), "video": { "url": "v.mp4" } })));
        assert!(inline(json!({ "type": "article", "image": pic() })));
        assert!(!inline(json!({ "type": "video", "title": "Clip", "thumbnail": pic(), "video": { "url": "v" } })), "titled: a card");
        assert!(!inline(json!({ "type": "rich", "image": pic() })), "a bot's embed is always a card");
        assert!(!inline(json!({ "image": pic() })), "no type means rich");
        assert!(!inline(json!({ "type": "article", "author": { "name": "x" }, "image": pic() })));
        assert!(!inline(json!({ "type": "link", "description": "text only" })), "no media");
    }

    #[test]
    fn embeds_split_thumbnail_from_image_and_fold_shared_urls() {
        let pic = |url: &str| json!({ "url": url, "width": 100, "height": 50 });
        let m = parse_message(&json!({ "id": "1", "channel_id": "c", "author": { "id": "u" }, "embeds": [
            { "url": "https://a", "title": "Set", "thumbnail": pic("t"), "image": pic("1"), "flags": 16 },
            { "url": "https://a", "image": pic("2") },
            { "url": "https://a", "image": pic("3") },
            { "url": "https://a", "image": pic("4") },
            { "url": "https://a", "image": pic("5") },
            { "type": "video", "url": "https://v", "thumbnail": pic("still") },
            { "type": "poll_result", "fields": [
                { "name": "poll_question_text", "value": "Map?" }, { "name": "victor_answer_votes", "value": "3" },
                { "name": "total_votes", "value": "5" }, { "name": "victor_answer_text", "value": "Peaks" }, { "name": "victor_answer_emoji_name", "value": "🏔️" } ] },
        ] }))
        .unwrap();
        assert_eq!(m.embeds.len(), 3);
        let set = &m.embeds[0];
        assert_eq!(set.thumbnail.as_ref().map(|t| t.url.as_str()), Some("t"));
        assert_eq!(set.images.iter().map(|i| i.url.as_str()).collect::<Vec<_>>(), ["1", "2", "3", "4"]);
        assert!(set.sensitive && !set.video);
        let video = &m.embeds[1];
        assert!(video.video && video.thumbnail.is_none());
        assert_eq!(video.images[0].url, "still");
        let poll = m.embeds[2].poll.as_ref().unwrap();
        assert_eq!((poll.question.as_str(), poll.winner_votes, poll.total_votes), ("Map?", 3, 5));
        assert_eq!(poll.winner, Some(("Peaks".into(), Some(Emoji::unicode("🏔️")))));
    }

    #[test]
    fn stickers_parse_from_messages_and_pick_their_format() {
        let m = parse_message(&json!({ "id": "1", "channel_id": "2", "author": { "id": "3" }, "sticker_items": [
            { "id": "10", "name": "wave", "format_type": 1 },
            { "id": "11", "name": "dance", "format_type": 4 },
            { "id": "12", "name": "lottie", "format_type": 3 },
            { "id": "13", "name": "bounce", "format_type": 2 },
        ] }))
        .expect("message");
        let urls: Vec<_> = m.stickers.iter().map(|s| s.url(160)).collect();
        assert_eq!(urls[0].as_deref(), Some("https://media.discordapp.net/stickers/10.png?size=160"));
        assert_eq!(urls[1].as_deref(), Some("https://media.discordapp.net/stickers/11.gif?size=160"));
        assert_eq!(urls[2], None);
        assert_eq!(urls[3].as_deref(), Some("https://media.discordapp.net/stickers/13.webp?size=160&animated=true"));
        assert!(!m.stickers[0].animated() && m.stickers[1].animated());
        let tagged = Sticker { name: "Wumpus wave".into(), tags: "👋, hello".into(), ..Sticker::default() };
        assert!(tagged.matches("wave") && tagged.matches("HELLO") && !tagged.matches("bye"));
    }

    #[test]
    fn custom_status_reads_settings_and_drops_expired() {
        let now = chrono::Utc::now();
        let soon = (now + chrono::Duration::hours(1)).to_rfc3339();
        let past = (now - chrono::Duration::hours(1)).to_rfc3339();
        let s = CustomStatus::parse(&json!({ "text": "brb", "emoji_name": "🍡", "expires_at": soon }), now).expect("live status");
        assert_eq!((s.text.as_str(), s.emoji.as_ref().map(|e| e.name.as_str())), ("brb", Some("🍡")));
        assert!(CustomStatus::parse(&json!({ "text": "old", "expires_at": past }), now).is_none());
        assert!(CustomStatus::parse(&json!({ "text": "", "emoji_name": null }), now).is_none());
        assert!(CustomStatus::parse(&Value::Null, now).is_none());
        let emoji_only = CustomStatus::parse(&json!({ "emoji_name": "x", "emoji_id": "1" }), now).expect("emoji alone counts");
        assert_eq!(emoji_only.setting()["text"], Value::Null);
        assert_eq!(emoji_only.activity()["emoji"]["id"], "1");
    }

    #[test]
    fn animated_swaps_only_moving_art() {
        assert_eq!(animated(&avatar_url("1", Some("a_abc"))), format!("{CDN}/avatars/1/a_abc.gif?size=128"));
        assert_eq!(animated(&avatar_url("1", Some("abc"))), avatar_url("1", Some("abc")), "a still avatar stays a PNG");
        assert_eq!(animated(&format!("{CDN}/avatar-decoration-presets/x.png?size=96&passthrough=false")), format!("{CDN}/avatar-decoration-presets/x.png?size=96&passthrough=true"));
    }

    #[test]
    fn presence_keeps_custom_status_apart() {
        let p = parse_presence(&json!({ "status": "dnd", "activities": [{ "type": 0, "name": "Apex" }, { "type": 4, "state": "m!help" }] }));
        assert_eq!(p.status, Status::Dnd);
        assert_eq!(p.custom.as_deref(), Some("m!help"));
        assert_eq!(p.activity.unwrap().text, "Apex");
        let p = parse_presence(&json!({ "status": "idle", "activities": [{ "type": 2, "name": "Spotify", "state": "Hozier" }, { "type": 0, "name": "Apex" }] }));
        assert_eq!(p.activity, Some(Activity { kind: ActivityKind::Listening, text: "Hozier".into(), rest: vec![(ActivityKind::Playing, "Apex".into())] }));
    }

    #[test]
    fn presence_mark_follows_device_and_stream() {
        let mark = |v: Value| parse_presence(&v).mark();
        assert_eq!(mark(json!({ "status": "online", "client_status": { "mobile": "online" } })), Mark::Mobile);
        assert_eq!(mark(json!({ "status": "online", "client_status": { "mobile": "online", "desktop": "idle" } })), Mark::Status(Status::Online));
        assert_eq!(mark(json!({ "status": "idle", "client_status": { "mobile": "idle" } })), Mark::Status(Status::Idle));
        assert_eq!(mark(json!({ "status": "online" })), Mark::Status(Status::Online));
        let stream = json!([{ "type": 0, "name": "Apex" }, { "type": 1, "name": "Twitch", "url": "https://twitch.tv/x" }]);
        assert_eq!(mark(json!({ "status": "dnd", "client_status": { "mobile": "dnd" }, "activities": stream })), Mark::Streaming);
        assert_eq!(mark(json!({ "status": "offline", "activities": stream })), Mark::Status(Status::Offline));
    }

    #[test]
    fn parses_profile() {
        let body = json!({
            "user": { "id": "9", "username": "aayurene", "global_name": "Aayurene", "banner": "a_b", "avatar_decoration_data": { "asset": "deco" } },
            "user_profile": { "bio": "LUCKY VICKY <a:aWonUwu:1328465377218269257>", "pronouns": "WON/YOUNG", "theme_colors": [4980812, 11558570] },
            "badges": [{ "id": "premium", "description": "Subscriber since", "icon": "2ba85e8026a8614b640c2837bcdfe21b", "link": "https://discord.com/settings/premium" }, { "id": "broken" }],
            "mutual_friends": [{ "id": "1", "username": "a" }, { "id": "2", "username": "b" }],
            "mutual_friend_count": 5,
            "mutual_guilds": [{ "id": "g", "nick": null }],
            "guild_member": { "roles": ["r"], "joined_at": "2022-06-18T10:00:00+00:00" },
            "connected_accounts": [{ "type": "spotify", "id": "s1", "name": "wren", "verified": true }, { "type": "steam", "id": "7656" }, { "name": "no type" }],
            "guild_member_profile": { "collectibles": [{ "sku_id": "old", "type": 1, "expires_at": "2001-01-01T00:00:00+00:00" }] },
        });
        let p = parse_profile("9", &body);
        assert_eq!(p.username, "aayurene");
        assert_eq!(p.pronouns.as_deref(), Some("WON/YOUNG"));
        assert_eq!(p.banner.as_deref(), Some("https://cdn.discordapp.com/banners/9/a_b.gif?size=600"));
        assert_eq!(p.theme, Some((0x4C004C, 0xB05EAA)));
        assert_eq!(p.badges.len(), 1);
        assert_eq!(p.badges[0].icon, "https://cdn.discordapp.com/badge-icons/2ba85e8026a8614b640c2837bcdfe21b.png");
        assert_eq!((p.mutual_friends.len(), p.mutual_friend_count, p.mutual_server_count), (2, 5, 1));
        assert!(p.decoration.unwrap().contains("/avatar-decoration-presets/deco.png"));
        assert_eq!(p.roles, ["r"]);
        assert_eq!(p.joined.as_deref(), Some("2022-06-18T10:00:00+00:00"));
        assert_eq!(p.connections.iter().map(|c| (c.kind.as_str(), c.name.as_str(), c.verified)).collect::<Vec<_>>(), [("spotify", "wren", true), ("steam", "", false)]);
        assert_eq!(p.effect_sku, None);
        let worn = parse_profile("9", &json!({ "user_profile": { "collectibles": [{ "sku_id": "frame", "type": 2 }, { "sku_id": "fx", "type": 1, "expires_at": null }] } }));
        assert_eq!(worn.effect_sku.as_deref(), Some("fx"));
        let bare = parse_profile("9", &json!({ "user": { "id": "9" }, "user_profile": { "theme_colors": [1] } }));
        assert_eq!((bare.theme, bare.mutual_friend_count, bare.badges.len()), (None, 0, 0));
    }

    #[test]
    fn relationships_carry_since() {
        let since = "2023-08-09T12:00:00+00:00";
        let Some(Event::RelationshipChange { relationship: Some(r), .. }) = dispatch("RELATIONSHIP_ADD", &json!({ "id": "5", "type": 1, "since": since })) else { panic!("no relationship") };
        assert_eq!((r.kind, r.since.as_deref()), (Relation::Friend, Some(since)));
        let ready = parse_ready(&json!({ "user": { "id": "1" }, "relationships": [{ "id": "5", "type": 1, "since": since, "user": { "id": "5" } }] }));
        assert_eq!(ready.relationships[0].since.as_deref(), Some(since));
    }

    #[test]
    fn parses_profile_effect() {
        let body = json!({ "items": [{ "type": 1, "effects": [
            { "src": "https://x/top.png", "width": 450, "height": 880, "position": { "x": 0, "y": 0 }, "zIndex": 2 },
            { "src": "https://x/base.png", "randomizedSources": [{ "src": "https://x/a.png" }], "width": 450, "height": 200, "position": { "x": 0, "y": 40 }, "zIndex": 1 },
            { "src": "https://x/empty.png" },
        ]}]});
        let layers = parse_profile_effect(&body);
        assert_eq!(layers.iter().map(|l| l.src.as_str()).collect::<Vec<_>>(), ["https://x/a.png", "https://x/top.png"]);
        assert_eq!((layers[0].y, layers[0].height), (40., 200.));
        assert!(parse_profile_effect(&json!({ "items": [] })).is_empty());
    }

    #[test]
    fn parses_rich_presence() {
        let p = parse_presence(&json!({ "status": "dnd", "activities": [
            { "type": 4, "state": "busy" },
            { "type": 0, "name": "PUBG: BATTLEGROUNDS", "application_id": "530196305138417685", "details": "Normal, Erangel, 58/99", "state": "4 Squad",
              "party": { "id": "p", "size": [4, 4] }, "timestamps": { "start": 1700000000000i64 }, "flags": 2, "assets": { "large_image": "123" } },
        ]}));
        let rich = p.rich.unwrap();
        assert_eq!(rich.name, "PUBG: BATTLEGROUNDS");
        assert_eq!(rich.party, Some((4, 4)));
        assert_eq!(rich.started, Some(1700000000000));
        assert!(rich.joinable);
        assert_eq!(rich.image.as_deref(), Some("https://cdn.discordapp.com/app-assets/530196305138417685/123.png"));
        let proxied = parse_rich(&json!({ "name": "x", "assets": { "large_image": "mp:external/abc/https/x.png" } })).unwrap();
        assert_eq!(proxied.image.as_deref(), Some("https://media.discordapp.net/external/abc/https/x.png"));
        assert!(!proxied.joinable);
    }

    #[test]
    fn parses_member_list_ops() {
        let op = parse_list_op(&json!({ "op": "SYNC", "range": [0, 99], "items": [
            { "group": { "id": "online", "count": 2 } },
            { "member": { "user": { "id": "1", "username": "a" }, "nick": "Nick", "avatar": "h", "roles": ["r"], "presence": { "status": "online" } } },
        ]}), Some("g"))
        .unwrap();
        let ListOp::Sync { items, .. } = op else { panic!("sync") };
        assert!(matches!(&items[1], ListItem::Member(m) if m.roles == ["r"] && m.user.name == "Nick" && m.user.avatar.contains("/guilds/g/users/1/avatars/h.png")));
    }

    #[test]
    fn invites_read_their_server_and_counts() {
        let body = json!({ "code": "waffle", "guild": { "id": "9", "name": "Dumpling Hall", "icon": "ff" }, "approximate_member_count": 8311, "approximate_presence_count": 1204 });
        let invite = parse_invite(&body).unwrap();
        assert_eq!(invite, Invite { guild_id: "9".into(), name: "Dumpling Hall".into(), icon: Some("https://cdn.discordapp.com/icons/9/ff.png?size=96".into()), online: Some(1204), members: Some(8311), ..Default::default() });
        assert_eq!(parse_invite(&json!({ "code": "gdm", "channel": { "id": "5", "type": 3 } })), None, "a group DM invite has no server card");
    }

    #[test]
    fn voice_effect_reads_the_sound_and_its_emoji() {
        let d = json!({
            "channel_id": "10", "guild_id": "1", "user_id": "7", "sound_id": "33", "sound_volume": 0.5,
            "emoji": { "id": null, "name": "🦆" }, "animation_type": 1, "animation_id": 4, "name": "quack", "source_guild_id": "2"
        });
        let Some(Event::VoiceEffect(effect)) = dispatch("VOICE_CHANNEL_EFFECT_SEND", &d) else { panic!("not an effect") };
        assert_eq!(effect, VoiceEffect {
            channel_id: "10".into(),
            user_id: "7".into(),
            sound_id: Some("33".into()),
            sound_volume: 0.5,
            emoji: Some(Emoji::unicode("🦆")),
            animation_type: Some(1),
            animation_id: Some(4),
            name: Some("quack".into()),
            source_guild_id: Some("2".into()),
        });
        let bare = parse_voice_effect(&json!({ "channel_id": "10", "user_id": "7", "emoji": { "id": "55", "name": "pog" } })).unwrap();
        assert_eq!((bare.sound_id, bare.sound_volume, bare.emoji), (None, 1., Some(Emoji { id: Some("55".into()), name: "pog".into(), animated: false })), "an emoji alone, at full volume");
    }

    #[test]
    fn soundboard_sounds_reply_and_updates() {
        let d = json!({ "guild_id": "1", "soundboard_sounds": [
            { "sound_id": "33", "name": "quack", "volume": 0.8, "emoji_id": null, "emoji_name": "🦆", "guild_id": "1", "available": true },
            { "name": "no id" }
        ] });
        let Some(Event::SoundboardSounds { guild_id, sounds, all }) = dispatch("SOUNDBOARD_SOUNDS", &d) else { panic!("not sounds") };
        assert_eq!((guild_id.as_str(), all), ("1", true));
        assert_eq!(sounds, vec![SoundboardSound { id: "33".into(), name: "quack".into(), guild_id: "1".into(), volume: 0.8, emoji: Some(Emoji::unicode("🦆")), available: true }]);
        let Some(Event::SoundboardSounds { all: false, sounds, .. }) = dispatch("GUILD_SOUNDBOARD_SOUND_UPDATE", &json!({ "sound_id": "33", "name": "honk", "guild_id": "1" })) else { panic!("not an update") };
        assert_eq!(sounds[0].name, "honk");
        assert!(matches!(dispatch("GUILD_SOUNDBOARD_SOUND_DELETE", &json!({ "sound_id": "33", "guild_id": "1" })), Some(Event::SoundboardSoundDelete { sound_id, .. }) if sound_id == "33"));
    }

    #[test]
    fn default_sounds_are_server_zero_and_play_without_a_source() {
        let sound = SoundboardSound::parse(&json!({ "sound_id": 1, "name": "quack", "volume": 1.0, "emoji_name": "🦆" })).unwrap();
        assert_eq!((sound.id.as_str(), sound.is_default()), ("1", true), "a numeric id reads as a string");
        assert_eq!(soundboard_play_body(&sound), json!({ "sound_id": "1", "emoji_id": null, "emoji_name": "🦆" }));
        let custom = SoundboardSound { guild_id: "9".into(), emoji: Some(Emoji { id: Some("55".into()), name: String::new(), animated: false }), ..sound };
        assert_eq!(soundboard_play_body(&custom), json!({ "sound_id": "1", "emoji_id": "55", "emoji_name": null, "source_guild_id": "9" }));
        assert_eq!(custom.url(), "https://cdn.discordapp.com/soundboard-sounds/1");
    }

    #[test]
    fn a_manual_ack_carries_its_mention_count() {
        let manual = dispatch("MESSAGE_ACK", &json!({ "channel_id": "7", "message_id": "42", "manual": true, "mention_count": 3, "version": 9 }));
        assert!(matches!(manual, Some(Event::Ack { message_id: 42, manual: Some(3), .. })));
        let read = dispatch("MESSAGE_ACK", &json!({ "channel_id": "7", "message_id": "42", "version": 9 }));
        assert!(matches!(read, Some(Event::Ack { manual: None, .. })));
    }
}

// `bookmark_ids` from GET /users/@me/saved-messages. Not verified: each entry's shape; read as either a
// message id or a save with `message_id`.
fn bookmark_ids(list: &Value) -> Vec<String> {
    list.as_array().into_iter().flatten().filter_map(|v| v.as_str().or_else(|| v["message_id"].as_str())).map(str::to_owned).collect()
}

#[cfg(test)]
mod saved_tests {
    #[test]
    fn bookmark_ids_read_ids_or_saves() {
        let list = serde_json::json!(["1", { "channel_id": "9", "message_id": "2" }, 3]);
        assert_eq!(super::bookmark_ids(&list), ["1", "2"]);
    }
}
