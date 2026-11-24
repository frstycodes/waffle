// Every REST call server settings makes, as plain values so tests can check path and body without a network.
// Routes and fields from discord-api-docs (read 2026-11-18) unless a line says "user client": those are
// what Discord's own app calls, undocumented. Not verified live.
use base64::Engine;
use crate::discord::Method;
use serde_json::{Map, Value, json};

use crate::discord::Client;

#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub method: Method,
    pub path: String,
    pub body: Option<Value>,
}

impl Call {
    pub(crate) fn new(method: Method, path: String, body: Option<Value>) -> Self {
        Call { method, path, body }
    }

    // Discord's refusal comes back verbatim (discord::send keeps its message).
    pub async fn send(self, client: &Client) -> Result<Value, String> {
        client.call(self.method, self.path, self.body).await
    }
}

fn get(path: String) -> Call {
    Call::new(Method::GET, path, None)
}

fn delete(path: String) -> Call {
    Call::new(Method::DELETE, path, None)
}

// Discord's image data: a base64 data URI.
pub fn image_data(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

// Overview

// What the Overview page edits. Images hold Discord's hash as loaded, or a data URI once replaced.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overview {
    pub name: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub banner: Option<String>,
    pub splash: Option<String>,
    pub system_channel_id: Option<String>,
    pub system_channel_flags: u64,
    pub afk_channel_id: Option<String>,
    pub afk_timeout: u32,
    pub default_message_notifications: u8,
    pub verification_level: u8,
    pub explicit_content_filter: u8,
    pub premium_progress_bar_enabled: bool,
}

// Discord's allowed inactive timeouts, seconds.
pub const AFK_TIMEOUTS: [u32; 5] = [60, 300, 900, 1800, 3600];

// System channel flags are "suppress" bits: a set bit turns the message off.
pub mod system_flags {
    pub const JOIN: u64 = 1 << 0;
    pub const BOOSTS: u64 = 1 << 1;
    pub const SETUP_TIPS: u64 = 1 << 2;
    pub const JOIN_STICKER_REPLIES: u64 = 1 << 3;
}

impl Overview {
    pub fn parse(g: &Value) -> Self {
        let s = |k: &str| g[k].as_str().map(str::to_owned);
        let n = |k: &str| g[k].as_u64().unwrap_or(0);
        Overview {
            name: s("name").unwrap_or_default(),
            description: s("description"),
            icon: s("icon"),
            banner: s("banner"),
            splash: s("splash"),
            system_channel_id: s("system_channel_id"),
            system_channel_flags: n("system_channel_flags"),
            afk_channel_id: s("afk_channel_id"),
            afk_timeout: g["afk_timeout"].as_u64().unwrap_or(300) as u32,
            default_message_notifications: n("default_message_notifications") as u8,
            verification_level: n("verification_level") as u8,
            explicit_content_filter: n("explicit_content_filter") as u8,
            premium_progress_bar_enabled: g["premium_progress_bar_enabled"].as_bool().unwrap_or(false),
        }
    }
}

// Only the fields the draft changed, so a save never rewrites what someone else just set.
pub fn overview_patch(saved: &Overview, draft: &Overview) -> Map<String, Value> {
    let mut out = Map::new();
    let mut put = |key: &str, before: Value, after: Value| {
        if before != after {
            out.insert(key.into(), after);
        }
    };
    put("name", json!(saved.name), json!(draft.name.trim()));
    put("description", json!(saved.description), json!(draft.description.as_deref().map(str::trim).filter(|d| !d.is_empty())));
    put("icon", json!(saved.icon), json!(draft.icon));
    put("banner", json!(saved.banner), json!(draft.banner));
    put("splash", json!(saved.splash), json!(draft.splash));
    put("system_channel_id", json!(saved.system_channel_id), json!(draft.system_channel_id));
    put("system_channel_flags", json!(saved.system_channel_flags), json!(draft.system_channel_flags));
    put("afk_channel_id", json!(saved.afk_channel_id), json!(draft.afk_channel_id));
    put("afk_timeout", json!(saved.afk_timeout), json!(draft.afk_timeout));
    put("default_message_notifications", json!(saved.default_message_notifications), json!(draft.default_message_notifications));
    put("verification_level", json!(saved.verification_level), json!(draft.verification_level));
    put("explicit_content_filter", json!(saved.explicit_content_filter), json!(draft.explicit_content_filter));
    put("premium_progress_bar_enabled", json!(saved.premium_progress_bar_enabled), json!(draft.premium_progress_bar_enabled));
    out
}

pub fn guild(guild_id: &str) -> Call {
    get(format!("/guilds/{guild_id}?with_counts=true"))
}

// None when nothing changed: the save bar has nothing to send.
pub fn edit_guild(guild_id: &str, saved: &Overview, draft: &Overview) -> Option<Call> {
    let patch = overview_patch(saved, draft);
    (!patch.is_empty()).then(|| Call::new(Method::PATCH, format!("/guilds/{guild_id}"), Some(Value::Object(patch))))
}

// Turning Community on: Discord requires verification of at least "low", the media filter on for
// everyone, and a rules and an updates channel ("1" asks Discord to create one).
pub fn enable_community(guild_id: &str, features: &[String], rules_channel_id: Option<&str>, updates_channel_id: Option<&str>) -> Call {
    let mut features = features.to_vec();
    if !features.iter().any(|f| f == "COMMUNITY") {
        features.push("COMMUNITY".into());
    }
    Call::new(
        Method::PATCH,
        format!("/guilds/{guild_id}"),
        Some(json!({
            "features": features,
            "verification_level": 1,
            "explicit_content_filter": 2,
            "rules_channel_id": rules_channel_id.unwrap_or("1"),
            "public_updates_channel_id": updates_channel_id.unwrap_or("1"),
        })),
    )
}

pub fn disable_community(guild_id: &str, features: &[String]) -> Call {
    let features: Vec<&String> = features.iter().filter(|f| *f != "COMMUNITY").collect();
    Call::new(Method::PATCH, format!("/guilds/{guild_id}"), Some(json!({ "features": features })))
}

// User client: the owner's delete, which Discord confirms with the server's name typed out.
pub fn delete_guild(guild_id: &str) -> Call {
    Call::new(Method::POST, format!("/guilds/{guild_id}/delete"), Some(json!({})))
}

// Roles

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoleDraft {
    pub name: String,
    pub permissions: u64,
    pub color: u32,
    // A second colour makes a gradient; a third, Discord's holographic style. Needs ENHANCED_ROLE_COLORS.
    pub secondary_color: Option<u32>,
    pub tertiary_color: Option<u32>,
    pub hoist: bool,
    pub mentionable: bool,
    // A hash as loaded, or a data URI once replaced. Needs ROLE_ICONS.
    pub icon: Option<String>,
    pub unicode_emoji: Option<String>,
}

impl RoleDraft {
    pub fn parse(r: &Value) -> Self {
        let colors = &r["colors"];
        RoleDraft {
            name: r["name"].as_str().unwrap_or_default().to_owned(),
            permissions: r["permissions"].as_str().and_then(|p| p.parse().ok()).unwrap_or(0),
            color: colors["primary_color"].as_u64().or(r["color"].as_u64()).unwrap_or(0) as u32,
            secondary_color: colors["secondary_color"].as_u64().map(|c| c as u32),
            tertiary_color: colors["tertiary_color"].as_u64().map(|c| c as u32),
            hoist: r["hoist"].as_bool().unwrap_or(false),
            mentionable: r["mentionable"].as_bool().unwrap_or(false),
            icon: r["icon"].as_str().map(str::to_owned),
            unicode_emoji: r["unicode_emoji"].as_str().map(str::to_owned),
        }
    }
}

pub fn role_patch(saved: &RoleDraft, draft: &RoleDraft) -> Map<String, Value> {
    let mut out = Map::new();
    let mut put = |key: &str, before: Value, after: Value| {
        if before != after {
            out.insert(key.into(), after);
        }
    };
    put("name", json!(saved.name), json!(draft.name.trim()));
    // Permissions travel as a decimal string: the bitfield outgrows a JSON-safe integer.
    put("permissions", json!(saved.permissions.to_string()), json!(draft.permissions.to_string()));
    let colors = |r: &RoleDraft| json!({ "primary_color": r.color, "secondary_color": r.secondary_color, "tertiary_color": r.tertiary_color });
    put("colors", colors(saved), colors(draft));
    put("hoist", json!(saved.hoist), json!(draft.hoist));
    put("mentionable", json!(saved.mentionable), json!(draft.mentionable));
    put("icon", json!(saved.icon), json!(draft.icon));
    put("unicode_emoji", json!(saved.unicode_emoji), json!(draft.unicode_emoji));
    out
}

pub fn roles(guild_id: &str) -> Call {
    get(format!("/guilds/{guild_id}/roles"))
}

pub fn role_member_counts(guild_id: &str) -> Call {
    get(format!("/guilds/{guild_id}/roles/member-counts"))
}

// User client: the ids of everyone holding a role (Manage Members in the role editor).
pub fn role_member_ids(guild_id: &str, role_id: &str) -> Call {
    get(format!("/guilds/{guild_id}/roles/{role_id}/member-ids"))
}

// Discord's "Create role" makes "new role" with the server's defaults; the editor opens on it.
pub fn create_role(guild_id: &str) -> Call {
    Call::new(Method::POST, format!("/guilds/{guild_id}/roles"), Some(json!({ "name": "new role", "permissions": "0", "colors": { "primary_color": 0 } })))
}

pub fn edit_role(guild_id: &str, role_id: &str, saved: &RoleDraft, draft: &RoleDraft) -> Option<Call> {
    let patch = role_patch(saved, draft);
    (!patch.is_empty()).then(|| Call::new(Method::PATCH, format!("/guilds/{guild_id}/roles/{role_id}"), Some(Value::Object(patch))))
}

pub fn delete_role(guild_id: &str, role_id: &str) -> Call {
    delete(format!("/guilds/{guild_id}/roles/{role_id}"))
}

// `order` is the roles as listed after a drag, highest first, @everyone last. Discord's positions run the
// other way (@everyone is 0), so each role's position is its distance from the bottom. Only roles whose
// position changed are sent; None when the drag put everything back.
pub fn reorder_roles(guild_id: &str, order: &[(&str, i64)]) -> Option<Call> {
    let bottom = order.len() as i64 - 1;
    let moved: Vec<Value> = order.iter().enumerate().filter(|(i, (_, position))| bottom - *i as i64 != *position).map(|(i, (id, _))| json!({ "id": id, "position": bottom - i as i64 })).collect();
    (!moved.is_empty()).then(|| Call::new(Method::PATCH, format!("/guilds/{guild_id}/roles"), Some(Value::Array(moved))))
}

pub fn add_member_role(guild_id: &str, user_id: &str, role_id: &str) -> Call {
    Call::new(Method::PUT, format!("/guilds/{guild_id}/members/{user_id}/roles/{role_id}"), None)
}

pub fn remove_member_role(guild_id: &str, user_id: &str, role_id: &str) -> Call {
    delete(format!("/guilds/{guild_id}/members/{user_id}/roles/{role_id}"))
}

// Emoji, stickers, soundboard

pub fn emojis(guild_id: &str) -> Call {
    get(format!("/guilds/{guild_id}/emojis"))
}

// `image` is image data (image_data); Discord takes up to 256 KiB.
pub fn create_emoji(guild_id: &str, name: &str, image: String) -> Call {
    Call::new(Method::POST, format!("/guilds/{guild_id}/emojis"), Some(json!({ "name": name, "image": image, "roles": [] })))
}

pub fn delete_emoji(guild_id: &str, emoji_id: &str) -> Call {
    delete(format!("/guilds/{guild_id}/emojis/{emoji_id}"))
}

pub fn stickers(guild_id: &str) -> Call {
    get(format!("/guilds/{guild_id}/stickers"))
}

// Creating a sticker is a multipart upload; `tags` is the related emoji's name, as Discord's uploader fills it.
// `file` is a PNG, APNG, GIF or Lottie JSON up to 512 KiB.
pub async fn create_sticker(client: &Client, guild_id: &str, name: &str, description: &str, tags: &str, file_name: String, file: Vec<u8>) -> Result<Value, String> {
    let fields = vec![("name", name.to_owned()), ("description", description.to_owned()), ("tags", tags.to_owned())];
    client.call_form(format!("/guilds/{guild_id}/stickers"), fields, file_name, file).await
}

pub fn delete_sticker(guild_id: &str, sticker_id: &str) -> Call {
    delete(format!("/guilds/{guild_id}/stickers/{sticker_id}"))
}

pub fn sounds(guild_id: &str) -> Call {
    get(format!("/guilds/{guild_id}/soundboard-sounds"))
}

// `sound` is an mp3 or ogg as a data URI, up to 512 KiB and 5.2 seconds; volume 0-1.
pub fn create_sound(guild_id: &str, name: &str, sound: String, volume: f32, emoji: Option<&str>) -> Call {
    Call::new(Method::POST, format!("/guilds/{guild_id}/soundboard-sounds"), Some(json!({ "name": name, "sound": sound, "volume": volume, "emoji_name": emoji })))
}

pub fn delete_sound(guild_id: &str, sound_id: &str) -> Call {
    delete(format!("/guilds/{guild_id}/soundboard-sounds/{sound_id}"))
}

// Widget, template, vanity

pub fn widget(guild_id: &str) -> Call {
    get(format!("/guilds/{guild_id}/widget"))
}

pub fn edit_widget(guild_id: &str, enabled: bool, channel_id: Option<&str>) -> Call {
    Call::new(Method::PATCH, format!("/guilds/{guild_id}/widget"), Some(json!({ "enabled": enabled, "channel_id": channel_id })))
}

pub fn templates(guild_id: &str) -> Call {
    get(format!("/guilds/{guild_id}/templates"))
}

pub fn create_template(guild_id: &str, name: &str, description: &str) -> Call {
    Call::new(Method::POST, format!("/guilds/{guild_id}/templates"), Some(json!({ "name": name, "description": description })))
}

// Sync: the template takes on the server's current channels, roles and settings.
pub fn sync_template(guild_id: &str, code: &str) -> Call {
    Call::new(Method::PUT, format!("/guilds/{guild_id}/templates/{code}"), None)
}

pub fn delete_template(guild_id: &str, code: &str) -> Call {
    delete(format!("/guilds/{guild_id}/templates/{code}"))
}

// Apps

pub fn integrations(guild_id: &str) -> Call {
    get(format!("/guilds/{guild_id}/integrations"))
}

pub fn remove_integration(guild_id: &str, integration_id: &str) -> Call {
    delete(format!("/guilds/{guild_id}/integrations/{integration_id}"))
}

pub fn webhooks(guild_id: &str) -> Call {
    get(format!("/guilds/{guild_id}/webhooks"))
}

// Discord's "New webhook" names it for you; the editor renames it.
pub fn create_webhook(channel_id: &str, name: &str) -> Call {
    Call::new(Method::POST, format!("/channels/{channel_id}/webhooks"), Some(json!({ "name": name })))
}

pub fn delete_webhook(webhook_id: &str) -> Call {
    delete(format!("/webhooks/{webhook_id}"))
}

// AutoMod

pub mod automod {
    pub const KEYWORD: u8 = 1;
    pub const SPAM: u8 = 3;
    pub const KEYWORD_PRESET: u8 = 4;
    pub const MENTION_SPAM: u8 = 5;
    pub const BLOCK_MESSAGE: u8 = 1;
}

#[derive(Clone, Debug, PartialEq)]
pub enum AutomodAction {
    // Shown to the member when their message is blocked; at most 150 characters.
    Block { custom_message: Option<String> },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Trigger {
    Keyword { words: Vec<String>, regex: Vec<String>, allow: Vec<String> },
    Spam,
    Preset { presets: Vec<u8>, allow: Vec<String> },
    MentionSpam { limit: u8, raid_protection: bool },
}

#[derive(Clone, Debug, PartialEq)]
pub struct AutomodRule {
    pub name: String,
    pub enabled: bool,
    pub trigger: Trigger,
    pub actions: Vec<AutomodAction>,
    pub exempt_roles: Vec<String>,
    pub exempt_channels: Vec<String>,
}

impl AutomodRule {
    pub fn body(&self) -> Value {
        use automod::*;
        let (trigger_type, metadata) = match &self.trigger {
            Trigger::Keyword { words, regex, allow } => (KEYWORD, json!({ "keyword_filter": words, "regex_patterns": regex, "allow_list": allow })),
            Trigger::Spam => (SPAM, json!({})),
            Trigger::Preset { presets, allow } => (KEYWORD_PRESET, json!({ "presets": presets, "allow_list": allow })),
            Trigger::MentionSpam { limit, raid_protection } => (MENTION_SPAM, json!({ "mention_total_limit": limit, "mention_raid_protection_enabled": raid_protection })),
        };
        let actions: Vec<Value> = self
            .actions
            .iter()
            .map(|a| match a {
                AutomodAction::Block { custom_message } => json!({ "type": BLOCK_MESSAGE, "metadata": { "custom_message": custom_message } }),
            })
            .collect();
        // Event type 1: a member sends or edits a message.
        let event_type = 1;
        json!({
            "name": self.name,
            "enabled": self.enabled,
            "event_type": event_type,
            "trigger_type": trigger_type,
            "trigger_metadata": metadata,
            "actions": actions,
            "exempt_roles": self.exempt_roles,
            "exempt_channels": self.exempt_channels,
        })
    }
}

pub fn automod_rules(guild_id: &str) -> Call {
    get(format!("/guilds/{guild_id}/auto-moderation/rules"))
}

pub fn create_automod_rule(guild_id: &str, rule: &AutomodRule) -> Call {
    Call::new(Method::POST, format!("/guilds/{guild_id}/auto-moderation/rules"), Some(rule.body()))
}

pub fn set_automod_enabled(guild_id: &str, rule_id: &str, enabled: bool) -> Call {
    Call::new(Method::PATCH, format!("/guilds/{guild_id}/auto-moderation/rules/{rule_id}"), Some(json!({ "enabled": enabled })))
}

pub fn delete_automod_rule(guild_id: &str, rule_id: &str) -> Call {
    delete(format!("/guilds/{guild_id}/auto-moderation/rules/{rule_id}"))
}

// Moderation

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AuditQuery {
    pub user_id: Option<String>,
    pub action_type: Option<u16>,
    // The oldest entry shown so far: the next page is everything before it.
    pub before: Option<String>,
}

pub const AUDIT_PAGE: usize = 50;

pub fn audit_log(guild_id: &str, query: &AuditQuery) -> Call {
    let mut path = format!("/guilds/{guild_id}/audit-logs?limit={AUDIT_PAGE}");
    if let Some(user) = &query.user_id {
        path.push_str(&format!("&user_id={user}"));
    }
    if let Some(action) = query.action_type {
        path.push_str(&format!("&action_type={action}"));
    }
    if let Some(before) = &query.before {
        path.push_str(&format!("&before={before}"));
    }
    get(path)
}

// A page of bans, oldest first; `after` is the last user id of the page before.
pub fn bans(guild_id: &str, after: Option<&str>) -> Call {
    let mut path = format!("/guilds/{guild_id}/bans?limit=1000");
    if let Some(after) = after {
        path.push_str(&format!("&after={after}"));
    }
    get(path)
}

pub fn unban(guild_id: &str, user_id: &str) -> Call {
    delete(format!("/guilds/{guild_id}/bans/{user_id}"))
}

// `delete_seconds`: how much of their recent messages goes with them, up to 7 days.
pub fn ban(guild_id: &str, user_id: &str, delete_seconds: u32) -> Call {
    Call::new(Method::PUT, format!("/guilds/{guild_id}/bans/{user_id}"), Some(json!({ "delete_message_seconds": delete_seconds.min(604_800) })))
}

pub fn kick(guild_id: &str, user_id: &str) -> Call {
    delete(format!("/guilds/{guild_id}/members/{user_id}"))
}

// Discord bans at most 200 users per call.
pub const BULK_BAN_MAX: usize = 200;

// Not verified: the answer `{banned_users, failed_users}` (public API docs, not run). Discord's own client
// calls `/bulk-ban/v2` instead and learns the outcome over the gateway.
pub fn bulk_ban(guild_id: &str, user_ids: &[String], delete_seconds: u32) -> Call {
    let user_ids = &user_ids[..user_ids.len().min(BULK_BAN_MAX)];
    Call::new(Method::POST, format!("/guilds/{guild_id}/bulk-ban"), Some(json!({ "user_ids": user_ids, "delete_message_seconds": delete_seconds.min(604_800) })))
}

// `until`: an ISO 8601 time at most 28 days away; None lifts the timeout.
pub fn timeout(guild_id: &str, user_id: &str, until: Option<&str>) -> Call {
    Call::new(Method::PATCH, format!("/guilds/{guild_id}/members/{user_id}"), Some(json!({ "communication_disabled_until": until })))
}

// The members table (user client, docs/discord-moderation-re.md §3): Discord's sort codes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MemberSort {
    #[default]
    JoinedNewest = 1,
    JoinedOldest = 2,
    AccountNewest = 3,
    AccountOldest = 4,
}

// A safety signal a member can carry; a search for several keeps a member with any of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Signal {
    UnusualDmActivity,
    TimedOut,
    UnusualAccountActivity,
    Quarantined,
}

impl Signal {
    // In the table's order.
    pub const ALL: [Signal; 4] = [Signal::UnusualDmActivity, Signal::TimedOut, Signal::UnusualAccountActivity, Signal::Quarantined];

    // Not verified: Discord's client sends these ranges; whether they are epoch milliseconds was not read.
    fn wire(self, now_ms: i64) -> (&'static str, Value) {
        match self {
            Signal::UnusualDmActivity => ("unusual_dm_activity_until", json!({ "range": { "gte": now_ms - UNUSUAL_DM_WINDOW_MS } })),
            Signal::TimedOut => ("communication_disabled_until", json!({ "range": { "gte": now_ms } })),
            Signal::UnusualAccountActivity => ("unusual_account_activity", json!(true)),
            Signal::Quarantined => ("automod_quarantined_username", json!(true)),
        }
    }
}

// How long after `unusual_dm_activity_until` a member still counts as flagged: Discord's client uses 2 days.
pub const UNUSUAL_DM_WINDOW_MS: i64 = 2 * 24 * 60 * 60 * 1000;

// What the members table asks for; the default lists recent members.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MemberSearch {
    pub usernames: Vec<String>,
    pub user_ids: Vec<String>,
    pub signals: Vec<Signal>,
    pub sort: MemberSort,
    // Member since: when they joined the server.
    pub joined: Option<DateRange>,
    // Joined Discord: when their account was made, sent as snowflake bounds.
    pub account: Option<DateRange>,
}

// Epoch milliseconds, either end open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateRange {
    pub after_ms: Option<i64>,
    pub before_ms: Option<i64>,
}

impl DateRange {
    fn range(self, bound: impl Fn(i64) -> Value) -> Value {
        let mut range = Map::new();
        if let Some(after) = self.after_ms {
            range.insert("gte".into(), bound(after));
        }
        if let Some(before) = self.before_ms {
            range.insert("lte".into(), bound(before));
        }
        Value::Object(range)
    }
}

// The smallest snowflake made at `ms`, as Discord's `fromTimestamp` builds it.
pub fn snowflake_at(ms: i64) -> String {
    (((ms - DISCORD_EPOCH_MS).max(0) as u64) << 22).to_string()
}

const DISCORD_EPOCH_MS: i64 = 1_420_070_400_000;

// Where the next chunk starts: the last member already loaded.
#[derive(Clone, Debug, PartialEq)]
pub struct MemberCursor {
    pub joined_at_ms: i64,
    pub user_id: String,
}

// A 202 answer carries `retry_after` (seconds) while Discord indexes the server.
pub fn members_search(guild_id: &str, search: &MemberSearch, limit: usize, after: Option<&MemberCursor>, now_ms: i64) -> Call {
    let safety: Map<String, Value> = search.signals.iter().map(|s| s.wire(now_ms)).map(|(k, v)| (k.to_owned(), v)).collect();
    let mut and_query = Map::new();
    if !search.usernames.is_empty() {
        and_query.insert("usernames".into(), json!({ "or_query": search.usernames }));
    }
    let mut user_id = Map::new();
    if !search.user_ids.is_empty() {
        user_id.insert("or_query".into(), json!(search.user_ids));
    }
    if let Some(account) = search.account {
        user_id.insert("range".into(), account.range(|ms| json!(snowflake_at(ms))));
    }
    if !user_id.is_empty() {
        and_query.insert("user_id".into(), Value::Object(user_id));
    }
    if let Some(joined) = search.joined {
        and_query.insert("guild_joined_at".into(), json!({ "range": joined.range(|ms| json!(ms)) }));
    }
    let or_query = if safety.is_empty() { json!({}) } else { json!({ "safety_signals": safety }) };
    let mut body = json!({ "or_query": or_query, "and_query": and_query, "sort": search.sort as u8, "limit": limit });
    // Not verified: the cursor's `guild_joined_at` unit (epoch milliseconds assumed, as the client keeps it).
    if let Some(after) = after {
        body["after"] = json!({ "guild_joined_at": after.joined_at_ms, "user_id": after.user_id });
    }
    Call::new(Method::POST, format!("/guilds/{guild_id}/members-search"), Some(body))
}

// How each member joined, for a search answer that left it out (user client).
pub fn members_supplemental(guild_id: &str, user_ids: &[String]) -> Call {
    Call::new(Method::POST, format!("/guilds/{guild_id}/members/supplemental"), Some(json!({ "user_ids": user_ids })))
}

pub fn prune_count(guild_id: &str, days: u8, roles: &[String]) -> Call {
    get(format!("/guilds/{guild_id}/prune?days={days}&include_roles={}", roles.join(",")))
}

pub fn prune(guild_id: &str, days: u8, roles: &[String]) -> Call {
    Call::new(Method::POST, format!("/guilds/{guild_id}/prune"), Some(json!({ "days": days, "include_roles": roles, "compute_prune_count": false })))
}

pub fn invites(guild_id: &str) -> Call {
    get(format!("/guilds/{guild_id}/invites"))
}

pub fn revoke_invite(code: &str) -> Call {
    delete(format!("/invites/{code}"))
}

// "Pause invites": `until` an ISO time up to 24 hours away, None to resume.
pub fn pause_invites(guild_id: &str, until: Option<&str>) -> Call {
    Call::new(Method::PUT, format!("/guilds/{guild_id}/incident-actions"), Some(json!({ "invites_disabled_until": until, "dms_disabled_until": null })))
}

// Community

pub fn onboarding(guild_id: &str) -> Call {
    get(format!("/guilds/{guild_id}/onboarding"))
}

// Server Insights has no API a user account can read: the page links to Discord's.
pub fn insights_url(guild_id: &str) -> String {
    format!("https://discord.com/developers/servers/{guild_id}/analytics")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overview() -> Overview {
        Overview { name: "Night Shift".into(), icon: Some("abc".into()), afk_timeout: 300, system_channel_id: Some("10".into()), ..Default::default() }
    }

    #[test]
    fn overview_patch_sends_only_changes() {
        let saved = overview();
        assert!(edit_guild("1", &saved, &saved).is_none());
        let mut draft = saved.clone();
        draft.name = "  Night Owls ".into();
        draft.system_channel_flags = system_flags::JOIN | system_flags::SETUP_TIPS;
        let call = edit_guild("1", &saved, &draft).unwrap();
        assert_eq!(call.method, Method::PATCH);
        assert_eq!(call.path, "/guilds/1");
        assert_eq!(call.body.unwrap(), json!({ "name": "Night Owls", "system_channel_flags": 5 }));
    }

    #[test]
    fn overview_patch_clears_with_null() {
        let saved = overview();
        let mut draft = saved.clone();
        draft.icon = None;
        draft.system_channel_id = None;
        draft.description = Some("   ".into());
        assert_eq!(Value::Object(overview_patch(&saved, &draft)), json!({ "icon": null, "system_channel_id": null }));
    }

    #[test]
    fn overview_round_trips_discord_guild() {
        let g = json!({ "name": "Night Shift", "icon": "abc", "afk_timeout": 300, "system_channel_id": "10", "system_channel_flags": 0 });
        assert!(overview_patch(&Overview::parse(&g), &overview()).is_empty());
    }

    #[test]
    fn role_patch_sends_permissions_as_string_and_colors_whole() {
        let saved = RoleDraft { name: "Mods".into(), permissions: 1 << 40, color: 0xC8471A, ..Default::default() };
        let mut draft = saved.clone();
        draft.permissions |= 1 << 2;
        draft.secondary_color = Some(0x3355FF);
        let body = Value::Object(role_patch(&saved, &draft));
        assert_eq!(body, json!({ "permissions": ((1u64 << 40) | 4).to_string(), "colors": { "primary_color": 0xC8471A, "secondary_color": 0x3355FF, "tertiary_color": null } }));
    }

    #[test]
    fn role_parse_reads_colors_or_legacy_color() {
        let r = RoleDraft::parse(&json!({ "name": "A", "permissions": "8", "color": 5 }));
        assert_eq!((r.permissions, r.color, r.secondary_color), (8, 5, None));
        let r = RoleDraft::parse(&json!({ "name": "A", "permissions": "0", "color": 5, "colors": { "primary_color": 7, "secondary_color": 9 } }));
        assert_eq!((r.color, r.secondary_color), (7, Some(9)));
    }

    #[test]
    fn reorder_sends_only_moved_roles() {
        // Listed highest first; Admin dragged below Mods.
        let order = [("mods", 2), ("admin", 3), ("members", 1), ("everyone", 0)];
        let call = reorder_roles("1", &order).unwrap();
        assert_eq!(call.body.unwrap(), json!([{ "id": "mods", "position": 3 }, { "id": "admin", "position": 2 }]));
        assert!(reorder_roles("1", &[("a", 1), ("everyone", 0)]).is_none());
    }

    #[test]
    fn automod_bodies() {
        let rule = AutomodRule {
            name: "Mention spam".into(),
            enabled: true,
            trigger: Trigger::MentionSpam { limit: 20, raid_protection: true },
            actions: vec![AutomodAction::Block { custom_message: Some("Too many mentions".into()) }],
            exempt_roles: vec!["9".into()],
            exempt_channels: vec![],
        };
        let body = create_automod_rule("1", &rule).body.unwrap();
        assert_eq!(body["trigger_type"], 5);
        assert_eq!(body["event_type"], 1);
        assert_eq!(body["trigger_metadata"], json!({ "mention_total_limit": 20, "mention_raid_protection_enabled": true }));
        assert_eq!(body["actions"][0], json!({ "type": 1, "metadata": { "custom_message": "Too many mentions" } }));
        assert_eq!(set_automod_enabled("1", "77", false).body.unwrap(), json!({ "enabled": false }));
    }

    #[test]
    fn audit_log_query() {
        assert_eq!(audit_log("1", &AuditQuery::default()).path, "/guilds/1/audit-logs?limit=50");
        let q = AuditQuery { user_id: Some("5".into()), action_type: Some(22), before: Some("900".into()) };
        assert_eq!(audit_log("1", &q).path, "/guilds/1/audit-logs?limit=50&user_id=5&action_type=22&before=900");
    }

    #[test]
    fn members_search_body() {
        let recent = members_search("1", &MemberSearch::default(), 250, None, 0);
        assert_eq!((recent.method, recent.path.as_str()), (Method::POST, "/guilds/1/members-search"));
        assert_eq!(recent.body.unwrap(), json!({ "or_query": {}, "and_query": {}, "sort": 1, "limit": 250 }));
        let search = MemberSearch { usernames: vec!["wren".into()], user_ids: vec!["123456789012345678".into()], signals: vec![Signal::TimedOut, Signal::Quarantined], sort: MemberSort::AccountOldest, ..Default::default() };
        let after = MemberCursor { joined_at_ms: 5, user_id: "9".into() };
        let body = members_search("1", &search, 60, Some(&after), 1_000).body.unwrap();
        assert_eq!(body["or_query"], json!({ "safety_signals": { "communication_disabled_until": { "range": { "gte": 1_000 } }, "automod_quarantined_username": true } }));
        assert_eq!(body["and_query"], json!({ "usernames": { "or_query": ["wren"] }, "user_id": { "or_query": ["123456789012345678"] } }));
        assert_eq!((body["sort"].as_u64(), &body["after"]), (Some(4), &json!({ "guild_joined_at": 5, "user_id": "9" })));
        let dm = members_search("1", &MemberSearch { signals: vec![Signal::UnusualDmActivity], ..Default::default() }, 1, None, UNUSUAL_DM_WINDOW_MS).body.unwrap();
        assert_eq!(dm["or_query"]["safety_signals"]["unusual_dm_activity_until"], json!({ "range": { "gte": 0 } }));
        assert_eq!(members_supplemental("1", &["2".into()]).body.unwrap(), json!({ "user_ids": ["2"] }));
    }

    #[test]
    fn members_search_date_ranges() {
        let day = 86_400_000;
        let search = MemberSearch {
            user_ids: vec!["123456789012345678".into()],
            joined: Some(DateRange { after_ms: Some(DISCORD_EPOCH_MS + day), before_ms: None }),
            account: Some(DateRange { after_ms: Some(DISCORD_EPOCH_MS + day), before_ms: Some(DISCORD_EPOCH_MS + 2 * day) }),
            ..Default::default()
        };
        let body = members_search("1", &search, 1, None, 0).body.unwrap();
        assert_eq!(body["and_query"]["guild_joined_at"], json!({ "range": { "gte": DISCORD_EPOCH_MS + day } }));
        let (from, to) = ((day as u64) << 22, (2 * day as u64) << 22);
        assert_eq!(body["and_query"]["user_id"], json!({ "or_query": ["123456789012345678"], "range": { "gte": from.to_string(), "lte": to.to_string() } }));
        assert_eq!(snowflake_at(0), "0");
    }

    #[test]
    fn bulk_ban_caps_at_200() {
        let ids: Vec<String> = (0..250).map(|i| i.to_string()).collect();
        let call = bulk_ban("1", &ids, 3600);
        assert_eq!((call.method, call.path.as_str()), (Method::POST, "/guilds/1/bulk-ban"));
        let body = call.body.unwrap();
        assert_eq!((body["user_ids"].as_array().unwrap().len(), body["delete_message_seconds"].as_u64()), (200, Some(3600)));
    }

    #[test]
    fn small_calls() {
        assert_eq!(ban("1", "2", 999_999).body.unwrap(), json!({ "delete_message_seconds": 604_800 }));
        assert_eq!(revoke_invite("abc"), Call::new(Method::DELETE, "/invites/abc".into(), None));
        assert_eq!(create_webhook("4", "Bot").body.unwrap(), json!({ "name": "Bot" }));
        assert_eq!(image_data("image/png", b"hi"), "data:image/png;base64,aGk=");
        let community = enable_community("1", &["NEWS".into()], None, Some("8")).body.unwrap();
        assert_eq!(community["features"], json!(["NEWS", "COMMUNITY"]));
        assert_eq!((community["rules_channel_id"].as_str(), community["public_updates_channel_id"].as_str()), (Some("1"), Some("8")));
        assert_eq!(disable_community("1", &["NEWS".into(), "COMMUNITY".into()]).body.unwrap(), json!({ "features": ["NEWS"] }));
    }
}
