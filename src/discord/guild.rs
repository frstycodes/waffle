use super::*;

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

    pub(super) fn permissions_with(&self, user: &str, roles: &[String]) -> u64 {
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
    pub(super) fn overwrite_source<'a>(&'a self, channel: &'a Channel) -> &'a Channel {
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

pub(super) fn parse_forum_emoji(id: &Value, name: &Value) -> Option<Emoji> {
    match (id.as_str(), name.as_str()) {
        (Some(id), name) => Some(Emoji { id: Some(id.to_owned()), name: name.unwrap_or_default().to_owned(), animated: false }),
        (None, Some(name)) if !name.is_empty() => Some(Emoji { id: None, name: name.to_owned(), animated: false }),
        _ => None,
    }
}

// Discord's channel type for a stage channel, which `ChannelKind` folds into Voice.
pub const STAGE_TYPE: u8 = 13;

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
