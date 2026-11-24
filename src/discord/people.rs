use super::*;

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
    // When they started boosting the server this user was read from (the member's `premium_since`).
    #[serde(default)]
    pub boosting_since: Option<String>,
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
pub(super) fn parse_name_style(styles: &Value) -> Option<NameStyle> {
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
pub(super) fn palette_tint(name: &str) -> (u32, u32) {
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
    pub(super) fn parse(s: &str) -> Self {
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
    // Typing in a DM: the avatar's mark becomes the typing pill.
    Typing,
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
    // The role's own icon image. GAP: a `unicode_emoji` icon isn't read.
    pub icon: Option<String>,
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
    // guild_flag bits.
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
    // channel_flag bits.
    pub flags: u64,
}

// Bits of a channel override's `flags` (docs/discord-navigation-re.md §2, §3, §5).
pub mod channel_flag {
    // Pinned: a pinned DM, or a channel pinned to the top of its server.
    pub const FAVORITED: u64 = 2048;
    // In our channel list while the server has opt-in on.
    pub const OPT_IN_ENABLED: u64 = 4096;
}

// Bits of a server's user-settings `flags`. Opt-in on is Show all channels unticked.
pub mod guild_flag {
    pub const OPT_IN_CHANNELS_OFF: u64 = 8192;
    pub const OPT_IN_CHANNELS_ON: u64 = 16384;
}

impl GuildSettings {
    // A timed mute that has run out no longer counts, as in Discord.
    pub fn is_muted(&self) -> bool {
        self.muted && still_muted(self.mute_end.as_deref())
    }

    pub fn channel_muted(&self, channel_id: &str) -> bool {
        self.channels.get(channel_id).is_some_and(|c| c.muted && still_muted(c.mute_end.as_deref()))
    }

    pub fn channel_flags(&self, channel_id: &str) -> u64 {
        self.channels.get(channel_id).map_or(0, |c| c.flags)
    }

    pub fn pinned(&self, channel_id: &str) -> bool {
        self.channel_flags(channel_id) & channel_flag::FAVORITED != 0
    }

    pub fn opted_in(&self, channel_id: &str) -> bool {
        self.channel_flags(channel_id) & channel_flag::OPT_IN_ENABLED != 0
    }

    pub fn opt_in(&self) -> bool {
        self.flags & guild_flag::OPT_IN_CHANNELS_ON != 0
    }
}

pub(super) fn still_muted(end: Option<&str>) -> bool {
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

