use super::*;

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
    // The servers you share, each with their nickname there (`mutual_guilds[]`, `{id, nick}`).
    pub mutual_guilds: Vec<MutualGuild>,
    // Your private note on them (`GET /users/@me/notes/{id}`); None until fetched or when there is none.
    pub note: Option<String>,
    // GAP: the API field behind Discord's Game Collection is unknown, so only the demo fills this (icon URLs).
    pub games: Vec<String>,
    // When they joined the server the profile was fetched for (`guild_member.joined_at`, ISO 8601).
    pub joined: Option<String>,
    pub connections: Vec<Connection>,
    // The profile effect they wear: its collectible's sku, resolved through `/collectibles-products/{sku}`.
    pub effect_sku: Option<String>,
    // The profile frame they wear (collectible type 3), and its layers once that product is fetched.
    pub frame_sku: Option<String>,
    #[serde(skip)]
    pub frame: Option<ProfileFrame>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MutualGuild {
    pub id: String,
    pub nick: Option<String>,
}

// One row of a profile's Recent activity, from their content outbox (`/content-inventory/users/{id}/outbox`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecentEntry {
    pub id: String,
    pub kind: RecentKind,
    // The game, show or track.
    pub title: String,
    // The episode, artist, or nothing for a game.
    pub subtitle: Option<String>,
    pub app_id: Option<String>,
    // Discord's traits: first time playing, one long session's hours, days in a row.
    pub first_time: bool,
    pub marathon_hours: Option<u64>,
    pub streak_days: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RecentKind {
    #[default]
    Played,
    Watched,
    Listened,
}

// An account linked on their profile (`connected_accounts[]`); `kind` is Discord's platform type ("spotify").
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Connection {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub verified: bool,
}

// A profile frame: art drawn around the card, authored on a card `inner_width` wide (Discord's 1200) and scaled to
// the real one. Staples hang `overflow_top` above and `overflow_bottom` below it; every layer reaches
// `overflow_horizontal` past each side.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProfileFrame {
    pub inner_width: f32,
    pub overflow_top: f32,
    pub overflow_bottom: f32,
    pub overflow_horizontal: f32,
    pub layers: Vec<FrameLayer>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrameLayer {
    pub src: String,
    pub kind: FrameKind,
    // Over the card's content, or behind the card.
    pub front: bool,
    pub anchor: FrameAnchor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameKind {
    // One picture pinned to the top or bottom edge, overhanging it.
    Staple,
    // One picture the card's height, cropped from the top, middle or bottom.
    Rail,
    // A picture repeated down the card.
    Border,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameAnchor {
    Top,
    Center,
    Bottom,
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
    // Its timeline in milliseconds from the intro's start: shown from `start` for `duration`, then, when it
    // loops, again every `duration + loop_delay`.
    pub start: f32,
    pub duration: f32,
    pub looping: bool,
    pub loop_delay: f32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Badge {
    pub description: String,
    pub icon: String,
    // Where a click on it goes (Nitro's page, a quest): Discord's `badges[].link`.
    #[serde(default)]
    pub link: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Member {
    pub user: User,
    pub roles: Vec<String>,
    pub presence: Presence,
    // `premium_since`: when they started boosting this server (ISO 8601), if they do.
    pub boosting_since: Option<String>,
}

// What Mod View reads off a member object beyond its user and roles.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MemberFacts {
    pub user_id: String,
    pub joined_at: Option<chrono::DateTime<chrono::Utc>>,
    pub timed_out_until: Option<chrono::DateTime<chrono::Utc>>,
    pub unusual_dm_until: Option<chrono::DateTime<chrono::Utc>>,
    pub flags: u64,
    pub pending: bool,
}

impl MemberFacts {
    pub fn parse(m: &Value) -> Option<Self> {
        let time = |key: &str| m[key].as_str().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).map(|t| t.with_timezone(&chrono::Utc));
        Some(MemberFacts {
            user_id: m["user"]["id"].as_str()?.to_owned(),
            joined_at: time("joined_at"),
            timed_out_until: time("communication_disabled_until"),
            unusual_dm_until: time("unusual_dm_activity_until"),
            flags: m["flags"].as_u64().unwrap_or(0),
            pending: m["pending"].as_bool().unwrap_or(false),
        })
    }
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
    // Discord's NotificationSettings.notify_friends_on_go_live; unset reads as off, as its client does.
    pub notify_friends_on_go_live: bool,
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
            notify_friends_on_go_live: false,
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
    // PreloadedUserSettings (settings-proto type 1) as base64; `partial` carries only the changed top-level fields.
    SettingsProto { proto: String, partial: bool },
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
    MembersChunk { guild_id: String, members: Vec<(User, Vec<String>)>, facts: Vec<MemberFacts> },
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
    // An activity started, changed who's in it, or ended (no participants).
    ActivityInstance(ActivityInstance),
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
