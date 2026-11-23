use super::*;

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
    // lint-allow(unbounded-growth): capped at four pictures per embed, as the comment above says.
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
    pub(super) fn parse(v: &Value, now: chrono::DateTime<chrono::Utc>) -> Option<Self> {
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
    pub(super) fn activity(&self) -> Value {
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
    pub(super) fn key(&self) -> String {
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

pub(super) fn parse_voice_effect(d: &Value) -> Option<VoiceEffect> {
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
pub(super) fn id_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

// A sound's emoji: a server's by id, or a Unicode one by its character.
pub(super) fn sound_emoji(id: &Value, name: &Value) -> Option<Emoji> {
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
    // A reply whose original was deleted: Discord's "Original message was deleted".
    #[serde(default)]
    pub reply_deleted: bool,
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
    // An activity this message stands for (a launch's post, or an invite to one).
    #[serde(default)]
    pub activity: Option<MessageActivity>,
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

    pub(super) fn apply(self, body: &mut Value) {
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
    pub(super) fn parse(v: &Value) -> Option<Self> {
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
