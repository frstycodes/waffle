// The audit log as sentences: who did what to whom, each entry opening to the fields it changed.
// Action types, options and change exceptions from discord-api-docs resources/audit-log (read 2026-11-18).
use std::collections::HashMap;

use serde_json::Value;

use super::perms;

#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub key: String,
    pub old: Option<Value>,
    pub new: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: String,
    pub action: u16,
    pub user_id: Option<String>,
    pub target_id: Option<String>,
    pub changes: Vec<Change>,
    pub options: Value,
    pub reason: Option<String>,
}

// Names for the ids an entry mentions. The response brings users, webhooks, integrations, threads and
// AutoMod rules; the caller adds the server's roles and channels.
#[derive(Clone, Debug, Default)]
pub struct Directory {
    pub users: HashMap<String, String>,
    pub roles: HashMap<String, String>,
    pub channels: HashMap<String, String>,
    pub webhooks: HashMap<String, String>,
    pub integrations: HashMap<String, String>,
    pub rules: HashMap<String, String>,
}

pub struct Page {
    pub entries: Vec<Entry>,
    pub names: Directory,
}

pub fn parse(body: &Value) -> Page {
    let entries = body["audit_log_entries"].as_array().into_iter().flatten().filter_map(parse_entry).collect();
    let map = |key: &str, name: &dyn Fn(&Value) -> Option<String>| -> HashMap<String, String> {
        body[key].as_array().into_iter().flatten().filter_map(|v| Some((v["id"].as_str()?.to_owned(), name(v)?))).collect()
    };
    let plain = |v: &Value| v["name"].as_str().map(str::to_owned);
    let user = |v: &Value| v["global_name"].as_str().or(v["username"].as_str()).map(str::to_owned);
    let names = Directory { users: map("users", &user), webhooks: map("webhooks", &plain), integrations: map("integrations", &plain), rules: map("auto_moderation_rules", &plain), channels: map("threads", &plain), ..Default::default() };
    Page { entries, names }
}

fn parse_entry(e: &Value) -> Option<Entry> {
    Some(Entry {
        id: e["id"].as_str()?.to_owned(),
        action: e["action_type"].as_u64()? as u16,
        user_id: e["user_id"].as_str().map(str::to_owned),
        target_id: e["target_id"].as_str().map(str::to_owned),
        changes: e["changes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| Some(Change { key: c["key"].as_str()?.to_owned(), old: c.get("old_value").cloned(), new: c.get("new_value").cloned() }))
            .collect(),
        options: e["options"].clone(),
        reason: e["reason"].as_str().map(str::to_owned),
    })
}

// The `before` for the next page: only when this page came back full, so a short page ends the log.
pub fn next_before(page: &Page) -> Option<String> {
    (page.entries.len() >= super::api::AUDIT_PAGE).then(|| page.entries.last().map(|e| e.id.clone())).flatten()
}

// Discord's Mod View log (chunk daf6e954…): an entry joins the group above it when it repeats the group's
// action, target, actor and options less than 30 minutes after the group began, up to 50 joins.
const MERGE_WINDOW_MS: i64 = 30 * 60 * 1000;
const MERGE_CAP: usize = 50;
// Deletes, pins, moves, disconnects, reorders, bot adds, command permissions and prune stay one per row.
const NEVER_MERGE: [u16; 11] = [72, 73, 74, 75, 26, 27, 16, 33, 28, 121, 21];

// One row of merged entries: the first entry carrying every entry's changes, the newest entry's id, and
// how many entries it stands for.
#[derive(Clone, Debug, PartialEq)]
pub struct Merged {
    pub entry: Entry,
    pub last_id: String,
    pub count: usize,
}

// `entries` newest first, as Discord returns them; the rows come back newest first too.
pub fn merge(entries: &[Entry]) -> Vec<Merged> {
    let mut rows: Vec<Merged> = Vec::new();
    for e in entries.iter().rev() {
        if let Some(group) = rows.last_mut().filter(|g| joins(g, e)) {
            group.entry.changes.extend(e.changes.iter().cloned());
            (group.last_id, group.count) = (e.id.clone(), group.count + 1);
            continue;
        }
        rows.push(Merged { entry: e.clone(), last_id: e.id.clone(), count: 1 });
    }
    rows.reverse();
    rows
}

fn joins(group: &Merged, e: &Entry) -> bool {
    let first = &group.entry;
    let invite = (40..=42).contains(&e.action);
    let elapsed = super::view::snowflake_ms(&e.id) - super::view::snowflake_ms(&first.id);
    group.count <= MERGE_CAP && !NEVER_MERGE.contains(&e.action) && !invite && (first.action, &first.target_id, &first.user_id, &first.options) == (e.action, &e.target_id, &e.user_id, &e.options) && elapsed < MERGE_WINDOW_MS
}

// A sentence in runs: names are drawn strong, the rest plain.
#[derive(Clone, Debug, PartialEq)]
pub enum Seg {
    Text(String),
    Name(String),
}

// Create, change or remove: the entry's icon tone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Create,
    Update,
    Delete,
}

pub fn tone(action: u16) -> Tone {
    match action {
        12 | 15 | 20..=23 | 27 | 32 | 42 | 52 | 62 | 72 | 73 | 82 | 85 | 92 | 102 | 112 | 132 | 142..=146 | 165 | 193 => Tone::Delete,
        10 | 13 | 28 | 30 | 40 | 50 | 60 | 80 | 83 | 90 | 100 | 110 | 130 | 140 | 163 | 166 | 190 | 192 => Tone::Create,
        _ => Tone::Update,
    }
}

// The "Filter by action" list, in Discord's order.
pub const ACTIONS: &[(u16, &str)] = &[
    (1, "Update server"),
    (10, "Create channel"),
    (11, "Update channel"),
    (12, "Delete channel"),
    (13, "Create channel permission"),
    (14, "Update channel permission"),
    (15, "Delete channel permission"),
    (20, "Kick member"),
    (21, "Prune members"),
    (22, "Ban member"),
    (23, "Unban member"),
    (24, "Update member"),
    (25, "Update member roles"),
    (26, "Move member"),
    (27, "Disconnect member"),
    (28, "Add bot"),
    (30, "Create role"),
    (31, "Update role"),
    (32, "Delete role"),
    (40, "Create invite"),
    (41, "Update invite"),
    (42, "Delete invite"),
    (50, "Create webhook"),
    (51, "Update webhook"),
    (52, "Delete webhook"),
    (60, "Create emoji"),
    (61, "Update emoji"),
    (62, "Delete emoji"),
    (72, "Delete message"),
    (73, "Bulk delete messages"),
    (74, "Pin message"),
    (75, "Unpin message"),
    (80, "Add app"),
    (81, "Update app"),
    (82, "Remove app"),
    (83, "Start stage"),
    (84, "Update stage"),
    (85, "End stage"),
    (90, "Create sticker"),
    (91, "Update sticker"),
    (92, "Delete sticker"),
    (100, "Create event"),
    (101, "Update event"),
    (102, "Cancel event"),
    (110, "Create thread"),
    (111, "Update thread"),
    (112, "Delete thread"),
    (121, "Update app command permissions"),
    (130, "Create sound"),
    (131, "Update sound"),
    (132, "Delete sound"),
    (140, "Create AutoMod rule"),
    (141, "Update AutoMod rule"),
    (142, "Delete AutoMod rule"),
    (143, "AutoMod blocked a message"),
    (144, "AutoMod flagged a message"),
    (145, "AutoMod timed out a member"),
    (146, "AutoMod quarantined a member"),
    (163, "Create onboarding question"),
    (164, "Update onboarding question"),
    (165, "Delete onboarding question"),
    (166, "Set up onboarding"),
    (167, "Update onboarding"),
    (190, "Set up server guide"),
    (191, "Update server guide"),
    (192, "Create voice channel status"),
    (193, "Delete voice channel status"),
];

impl Entry {
    fn change(&self, key: &str) -> Option<&Change> {
        self.changes.iter().find(|c| c.key == key)
    }

    // A deleted thing's name lives only in its changes' old values.
    fn changed_name(&self) -> Option<String> {
        let c = self.change("name").or_else(|| self.change("code"))?;
        c.new.as_ref().or(c.old.as_ref())?.as_str().map(str::to_owned)
    }

    fn option(&self, key: &str) -> Option<&str> {
        self.options[key].as_str()
    }
}

impl Directory {
    fn user(&self, id: Option<&str>) -> String {
        id.and_then(|id| self.users.get(id)).cloned().unwrap_or_else(|| "Unknown user".into())
    }

    fn channel(&self, id: Option<&str>, fallback: Option<String>) -> String {
        let name = id.and_then(|id| self.channels.get(id)).cloned().or(fallback).unwrap_or_else(|| "deleted channel".into());
        format!("#{name}")
    }

    fn role(&self, id: Option<&str>, fallback: Option<String>) -> String {
        id.and_then(|id| self.roles.get(id)).cloned().or(fallback).unwrap_or_else(|| "deleted role".into())
    }
}

fn count(n: u64, one: &str, many: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {many}") }
}

pub fn sentence(e: &Entry, d: &Directory) -> Vec<Seg> {
    let t = |s: &str| Seg::Text(s.to_owned());
    let n = Seg::Name;
    let target = e.target_id.as_deref();
    let actor = if (143..=146).contains(&e.action) { "AutoMod".to_owned() } else { d.user(e.user_id.as_deref()) };
    let channel_opt = || d.channel(e.option("channel_id"), None);
    let number = |key: &str| e.option(key).and_then(|c| c.parse::<u64>().ok()).unwrap_or(1);
    let named = |fallback: &str| e.changed_name().unwrap_or_else(|| fallback.to_owned());
    let lookup = |map: &HashMap<String, String>, fallback: &str| target.and_then(|id| map.get(id)).cloned().unwrap_or_else(|| named(fallback));
    let verb = |create: &str, update: &str, delete: &str| match tone(e.action) {
        Tone::Create => create.to_owned(),
        Tone::Update => update.to_owned(),
        Tone::Delete => delete.to_owned(),
    };
    let mut out = vec![n(actor)];
    let rest: Vec<Seg> = match e.action {
        1 => vec![t(" updated the server")],
        10..=12 => vec![t(&verb(" created the channel ", " updated the channel ", " deleted the channel ")), n(d.channel(target, e.changed_name()))],
        13..=15 => {
            let who = if e.option("type") == Some("1") { d.user(e.option("id")) } else { d.role(e.option("id"), e.option("role_name").map(str::to_owned)) };
            vec![t(&verb(" added permission overrides for ", " updated permission overrides for ", " removed permission overrides for ")), n(who), t(" in "), n(d.channel(target, None))]
        }
        16 => vec![t(" reordered channels")],
        20 => vec![t(" kicked "), n(d.user(target))],
        21 => vec![t(&format!(" pruned {} for {} of inactivity", count(number("members_removed"), "member", "members"), count(number("delete_member_days"), "day", "days")))],
        22 => vec![t(" banned "), n(d.user(target))],
        23 => vec![t(" unbanned "), n(d.user(target))],
        24 => member_update(e, d.user(target)),
        25 => role_update(e, d.user(target)),
        26 => vec![t(&format!(" moved {} to ", count(number("count"), "member", "members"))), n(channel_opt())],
        27 => vec![t(&format!(" disconnected {} from voice", count(number("count"), "member", "members")))],
        28 => vec![t(" added the bot "), n(d.user(target))],
        30..=32 => vec![t(&verb(" created the role ", " updated the role ", " deleted the role ")), n(d.role(target, e.changed_name()))],
        33 => vec![t(" reordered roles")],
        40..=42 => vec![t(&verb(" created the invite ", " updated the invite ", " deleted the invite ")), n(named("invite"))],
        50..=52 => vec![t(&verb(" created the webhook ", " updated the webhook ", " deleted the webhook ")), n(lookup(&d.webhooks, "webhook"))],
        60..=62 => vec![t(&verb(" added the emoji ", " renamed the emoji ", " deleted the emoji ")), n(named("emoji"))],
        72 => vec![t(&format!(" deleted {} by ", count(number("count"), "message", "messages"))), n(d.user(target)), t(" in "), n(channel_opt())],
        73 => vec![t(&format!(" bulk deleted {} in ", count(number("count"), "message", "messages"))), n(d.channel(target, None))],
        74 | 75 => vec![t(if e.action == 74 { " pinned a message by " } else { " unpinned a message by " }), n(d.user(target)), t(" in "), n(channel_opt())],
        80..=82 => vec![t(&verb(" added the app ", " updated the app ", " removed the app ")), n(lookup(&d.integrations, "an app"))],
        83..=85 => vec![t(&verb(" started a stage in ", " updated the stage in ", " ended the stage in ")), n(channel_opt())],
        90..=92 => vec![t(&verb(" added the sticker ", " updated the sticker ", " deleted the sticker ")), n(named("sticker"))],
        100..=102 => vec![t(&verb(" created the event ", " updated the event ", " cancelled the event ")), n(named("event"))],
        110..=112 => vec![t(&verb(" created the thread ", " updated the thread ", " deleted the thread ")), n(d.channel(target, e.changed_name()))],
        121 => vec![t(" updated command permissions for "), n(e.option("application_id").and_then(|id| d.users.get(id)).cloned().unwrap_or_else(|| "an app".into()))],
        130..=132 => vec![t(&verb(" added the sound ", " updated the sound ", " deleted the sound ")), n(named("sound"))],
        140..=142 => vec![t(&verb(" created the AutoMod rule ", " updated the AutoMod rule ", " deleted the AutoMod rule ")), n(lookup(&d.rules, "rule"))],
        143..=146 => {
            let what = match e.action {
                143 => " blocked a message by ",
                144 => " flagged a message by ",
                145 => " timed out ",
                _ => " quarantined ",
            };
            let mut segs = vec![t(what), n(d.user(target))];
            if e.option("channel_id").is_some() {
                segs.extend([t(" in "), n(channel_opt())]);
            }
            if let Some(rule) = e.option("auto_moderation_rule_name") {
                segs.extend([t(" for breaking "), n(rule.to_owned())]);
            }
            segs
        }
        150 => vec![t(" requested creator monetization")],
        151 => vec![t(" accepted the creator monetization terms")],
        163..=165 => vec![t(&verb(" added an onboarding question", " updated an onboarding question", " deleted an onboarding question"))],
        166 | 167 => vec![t(if e.action == 166 { " set up onboarding" } else { " updated onboarding" })],
        190 | 191 => vec![t(if e.action == 190 { " set up the server guide" } else { " updated the server guide" })],
        192 => vec![t(" set a voice channel status in "), n(channel_opt())],
        193 => vec![t(" removed a voice channel status in "), n(channel_opt())],
        other => vec![t(&format!(" made a change Waffle can't show yet (action {other})"))],
    };
    out.extend(rest);
    out
}

fn member_update(e: &Entry, member: String) -> Vec<Seg> {
    let t = |s: &str| Seg::Text(s.to_owned());
    if let Some(c) = e.change("communication_disabled_until") {
        let lifted = c.new.as_ref().is_none_or(Value::is_null);
        return if lifted { vec![t(" removed the timeout for "), Seg::Name(member)] } else { vec![t(" timed out "), Seg::Name(member)] };
    }
    if e.change("nick").is_some() {
        return vec![t(" changed the nickname of "), Seg::Name(member)];
    }
    let flag = |key: &str| e.change(key).and_then(|c| c.new.as_ref()).and_then(Value::as_bool);
    match (flag("mute"), flag("deaf")) {
        (Some(on), _) => vec![t(if on { " server muted " } else { " server unmuted " }), Seg::Name(member)],
        (_, Some(on)) => vec![t(if on { " server deafened " } else { " server undeafened " }), Seg::Name(member)],
        _ => vec![t(" updated "), Seg::Name(member)],
    }
}

// `$add` / `$remove` carry the roles as [{ id, name }].
fn role_names(e: &Entry, key: &str) -> Vec<String> {
    e.change(key).and_then(|c| c.new.as_ref()).and_then(Value::as_array).into_iter().flatten().filter_map(|r| r["name"].as_str().map(str::to_owned)).collect()
}

fn role_update(e: &Entry, member: String) -> Vec<Seg> {
    let t = |s: &str| Seg::Text(s.to_owned());
    let (added, removed) = (role_names(e, "$add"), role_names(e, "$remove"));
    match (added.as_slice(), removed.as_slice()) {
        ([role], []) => vec![t(" gave "), Seg::Name(member), t(" the role "), Seg::Name(role.clone())],
        ([], [role]) => vec![t(" removed the role "), Seg::Name(role.clone()), t(" from "), Seg::Name(member)],
        _ => vec![t(" updated roles for "), Seg::Name(member)],
    }
}

// One changed field, ready to show: a label and its old and new values as text.
#[derive(Clone, Debug, PartialEq)]
pub struct Detail {
    pub label: String,
    pub old: Option<String>,
    pub new: Option<String>,
}

pub fn details(e: &Entry, d: &Directory) -> Vec<Detail> {
    let mut out = Vec::new();
    for c in &e.changes {
        match c.key.as_str() {
            "id" | "guild_id" => {}
            "permissions" | "allow" | "deny" => {
                let bits = |v: &Option<Value>| v.as_ref().and_then(|v| v.as_str().and_then(|s| s.parse().ok()).or(v.as_u64())).unwrap_or(0u64);
                let (old, new) = (bits(&c.old), bits(&c.new));
                let label = match c.key.as_str() {
                    "allow" => "Allowed",
                    "deny" => "Denied",
                    _ => "Permissions",
                };
                if new & !old != 0 {
                    out.push(Detail { label: format!("{label}: added"), old: None, new: Some(perms::names(new & !old).join(", ")) });
                }
                if old & !new != 0 {
                    out.push(Detail { label: format!("{label}: removed"), old: Some(perms::names(old & !new).join(", ")), new: None });
                }
            }
            "$add" | "$remove" => {
                let names = role_names(e, &c.key).join(", ");
                let added = c.key == "$add";
                out.push(Detail { label: if added { "Roles added" } else { "Roles removed" }.into(), old: (!added).then(|| names.clone()), new: added.then_some(names) });
            }
            key => out.push(Detail { label: label(key), old: c.old.as_ref().and_then(|v| value(key, v, d)), new: c.new.as_ref().and_then(|v| value(key, v, d)) }),
        }
    }
    if let Some(reason) = &e.reason {
        out.push(Detail { label: "Reason".into(), old: None, new: Some(reason.clone()) });
    }
    out
}

fn label(key: &str) -> String {
    let known = match key {
        "name" => "Name",
        "topic" => "Topic",
        "nsfw" => "Age-restricted",
        "rate_limit_per_user" => "Slowmode",
        "color" => "Colour",
        "colors" => "Colours",
        "hoist" => "Shown separately",
        "mentionable" => "Mentionable",
        "nick" => "Nickname",
        "afk_timeout" => "Inactive timeout",
        "afk_channel_id" => "Inactive channel",
        "system_channel_id" => "System messages channel",
        "rules_channel_id" => "Rules channel",
        "communication_disabled_until" => "Timed out until",
        "verification_level" => "Verification level",
        "explicit_content_filter" => "Media content filter",
        "default_message_notifications" => "Default notifications",
        "max_uses" => "Max uses",
        "max_age" => "Expires after",
        "bitrate" => "Bitrate",
        "user_limit" => "User limit",
        "position" => "Position",
        "icon_hash" | "avatar_hash" => "Icon",
        "banner_hash" => "Banner",
        "vanity_url_code" => "Custom invite link",
        "mute" => "Server muted",
        "deaf" => "Server deafened",
        _ => "",
    };
    if !known.is_empty() {
        return known.into();
    }
    let words = key.replace('_', " ");
    let mut chars = words.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

fn duration(seconds: u64) -> String {
    match seconds {
        0 => "Off".into(),
        s if s % 3600 == 0 => count(s / 3600, "hour", "hours"),
        s if s % 60 == 0 => count(s / 60, "minute", "minutes"),
        s => count(s, "second", "seconds"),
    }
}

fn value(key: &str, v: &Value, d: &Directory) -> Option<String> {
    Some(match v {
        Value::Null => return None,
        Value::Bool(b) => if *b { "On" } else { "Off" }.into(),
        Value::Number(num) => match (key, num.as_u64()) {
            ("color", Some(c)) => format!("#{c:06X}"),
            ("afk_timeout" | "rate_limit_per_user" | "max_age", Some(s)) => duration(s),
            _ => num.to_string(),
        },
        Value::String(s) if key.ends_with("channel_id") => d.channel(Some(s), None),
        Value::String(s) => s.clone(),
        Value::Object(o) if key == "colors" => o.values().filter_map(Value::as_u64).map(|c| format!("#{c:06X}")).collect::<Vec<_>>().join(", "),
        Value::Array(items) => items.iter().filter_map(|i| i["name"].as_str().or(i.as_str()).map(str::to_owned)).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn dir() -> Directory {
        let mut d = Directory::default();
        d.users.insert("u1".into(), "Mika".into());
        d.users.insert("u2".into(), "Oren".into());
        d.roles.insert("r1".into(), "Moderators".into());
        d.channels.insert("c1".into(), "general".into());
        d
    }

    fn entry(action: u16, target: Option<&str>, changes: Value, options: Value) -> Entry {
        parse_entry(&json!({ "id": "9", "action_type": action, "user_id": "u1", "target_id": target, "changes": changes, "options": options })).unwrap()
    }

    fn plain(segs: &[Seg]) -> String {
        segs.iter().map(|s| match s {
            Seg::Text(t) | Seg::Name(t) => t.as_str(),
        })
        .collect()
    }

    fn say(e: &Entry) -> String {
        plain(&sentence(e, &dir()))
    }

    #[test]
    fn sentences_for_each_kind() {
        let none = json!([]);
        assert_eq!(say(&entry(1, Some("g"), none.clone(), json!({}))), "Mika updated the server");
        assert_eq!(say(&entry(10, Some("c1"), none.clone(), json!({}))), "Mika created the channel #general");
        assert_eq!(say(&entry(12, Some("gone"), json!([{ "key": "name", "old_value": "old-chat" }]), json!({}))), "Mika deleted the channel #old-chat");
        assert_eq!(say(&entry(14, Some("c1"), none.clone(), json!({ "id": "r1", "type": "0", "role_name": "Moderators" }))), "Mika updated permission overrides for Moderators in #general");
        assert_eq!(say(&entry(13, Some("c1"), none.clone(), json!({ "id": "u2", "type": "1" }))), "Mika added permission overrides for Oren in #general");
        assert_eq!(say(&entry(22, Some("u2"), none.clone(), json!({}))), "Mika banned Oren");
        assert_eq!(say(&entry(21, None, none.clone(), json!({ "delete_member_days": "7", "members_removed": "12" }))), "Mika pruned 12 members for 7 days of inactivity");
        assert_eq!(say(&entry(72, Some("u2"), none.clone(), json!({ "channel_id": "c1", "count": "1" }))), "Mika deleted 1 message by Oren in #general");
        assert_eq!(say(&entry(26, None, none.clone(), json!({ "channel_id": "c1", "count": "3" }))), "Mika moved 3 members to #general");
        assert_eq!(say(&entry(32, Some("rx"), json!([{ "key": "name", "old_value": "Helpers" }]), json!({}))), "Mika deleted the role Helpers");
        assert_eq!(say(&entry(42, None, json!([{ "key": "code", "old_value": "abc123" }]), json!({}))), "Mika deleted the invite abc123");
        assert_eq!(
            say(&entry(143, Some("u2"), none.clone(), json!({ "channel_id": "c1", "auto_moderation_rule_name": "Block links", "auto_moderation_rule_trigger_type": "1" }))),
            "AutoMod blocked a message by Oren in #general for breaking Block links"
        );
        assert_eq!(say(&entry(192, None, none.clone(), json!({ "channel_id": "c1", "status": "lofi" }))), "Mika set a voice channel status in #general");
        assert_eq!(say(&entry(193, None, none.clone(), json!({ "channel_id": "c1" }))), "Mika removed a voice channel status in #general");
        assert_eq!(say(&entry(999, None, none, json!({}))), "Mika made a change Waffle can't show yet (action 999)");
    }

    #[test]
    fn member_and_role_updates() {
        let timeout = entry(24, Some("u2"), json!([{ "key": "communication_disabled_until", "new_value": "2026-11-19T10:00:00Z" }]), json!({}));
        assert_eq!(say(&timeout), "Mika timed out Oren");
        let lifted = entry(24, Some("u2"), json!([{ "key": "communication_disabled_until", "old_value": "2026-11-19T10:00:00Z" }]), json!({}));
        assert_eq!(say(&lifted), "Mika removed the timeout for Oren");
        let gave = entry(25, Some("u2"), json!([{ "key": "$add", "new_value": [{ "id": "r1", "name": "Moderators" }] }]), json!({}));
        assert_eq!(say(&gave), "Mika gave Oren the role Moderators");
        let took = entry(25, Some("u2"), json!([{ "key": "$remove", "new_value": [{ "id": "r1", "name": "Moderators" }] }]), json!({}));
        assert_eq!(say(&took), "Mika removed the role Moderators from Oren");
        let names: Vec<Seg> = sentence(&gave, &dir()).into_iter().filter(|s| matches!(s, Seg::Name(_))).collect();
        assert_eq!(names, vec![Seg::Name("Mika".into()), Seg::Name("Oren".into()), Seg::Name("Moderators".into())]);
    }

    #[test]
    fn change_details() {
        let e = Entry {
            reason: Some("cleanup".into()),
            ..entry(
                31,
                Some("r1"),
                json!([
                    { "key": "permissions", "old_value": "6", "new_value": "1099511627780" },
                    { "key": "color", "old_value": 0, "new_value": 13125402 },
                    { "key": "hoist", "old_value": false, "new_value": true },
                ]),
                json!({}),
            )
        };
        let d = details(&e, &dir());
        let row = |label: &str| d.iter().find(|r| r.label == label).cloned().unwrap();
        assert_eq!(row("Permissions: added").new.as_deref(), Some("Timeout members"));
        assert_eq!(row("Permissions: removed").old.as_deref(), Some("Kick, approve and reject members"));
        assert_eq!((row("Colour").old, row("Colour").new), (Some("#000000".into()), Some("#C8471A".into())));
        assert_eq!(row("Shown separately").new.as_deref(), Some("On"));
        assert_eq!(row("Reason").new.as_deref(), Some("cleanup"));
        let afk = details(&entry(1, None, json!([{ "key": "afk_timeout", "old_value": 300, "new_value": 3600 }, { "key": "system_channel_id", "new_value": "c1" }]), json!({})), &dir());
        assert_eq!(afk[0], Detail { label: "Inactive timeout".into(), old: Some("5 minutes".into()), new: Some("1 hour".into()) });
        assert_eq!(afk[1].new.as_deref(), Some("#general"));
    }

    #[test]
    fn parse_and_paging() {
        let entries: Vec<Value> = (0..50).map(|i| json!({ "id": format!("{}", 100 - i), "action_type": 1, "user_id": "u1" })).collect();
        let body = json!({ "audit_log_entries": entries, "users": [{ "id": "u1", "username": "mika", "global_name": "Mika" }], "webhooks": [{ "id": "w", "name": "Deploys" }] });
        let page = parse(&body);
        assert_eq!(page.names.users["u1"], "Mika");
        assert_eq!(page.names.webhooks["w"], "Deploys");
        assert_eq!(next_before(&page).as_deref(), Some("51"));
        let short = parse(&json!({ "audit_log_entries": [{ "id": "5", "action_type": 1 }] }));
        assert_eq!(next_before(&short), None);
        assert!(ACTIONS.windows(2).all(|w| w[0].0 < w[1].0));
    }

    // An entry `minutes` after Discord's epoch, so its id carries that time.
    fn at(minutes: i64, action: u16, target: &str, actor: &str) -> Entry {
        let id = ((minutes * 60_000) << 22).to_string();
        parse_entry(&json!({ "id": id, "action_type": action, "user_id": actor, "target_id": target, "changes": [{ "key": "nick", "new_value": minutes }] })).unwrap()
    }

    fn newest_first(mut entries: Vec<Entry>) -> Vec<Entry> {
        entries.reverse();
        entries
    }

    #[test]
    fn merges_repeats_within_half_an_hour() {
        let rows = merge(&newest_first(vec![at(0, 24, "t", "u1"), at(10, 24, "t", "u1"), at(29, 24, "t", "u1"), at(30, 24, "t", "u1")]));
        assert_eq!(rows.iter().map(|r| r.count).collect::<Vec<_>>(), vec![1, 3]);
        assert_eq!((rows[1].entry.changes.len(), rows[1].last_id.as_str()), (3, at(29, 24, "t", "u1").id.as_str()));
        // A different target, actor or action starts a new row.
        let rows = merge(&newest_first(vec![at(0, 24, "t", "u1"), at(1, 24, "x", "u1"), at(2, 24, "x", "u2"), at(3, 25, "x", "u2")]));
        assert_eq!(rows.len(), 4);
    }

    #[test]
    fn never_merges_deletes_pins_moves_or_prune() {
        for action in [72, 73, 74, 75, 26, 21, 40] {
            assert_eq!(merge(&newest_first(vec![at(0, action, "t", "u1"), at(1, action, "t", "u1")])).len(), 2, "{action}");
        }
    }

    #[test]
    fn merge_caps_at_fifty_joins() {
        let rows = merge(&newest_first((0..60).map(|i| at(i / 10, 24, "t", "u1")).collect()));
        assert_eq!(rows.iter().map(|r| r.count).collect::<Vec<_>>(), vec![9, 51]);
    }

    #[test]
    fn tones_follow_the_verb() {
        for (action, label) in ACTIONS {
            let expected = match label.split_whitespace().next().unwrap() {
                "Create" | "Add" | "Start" | "Set" => Tone::Create,
                "Delete" | "Remove" | "Kick" | "Ban" | "Unban" | "Prune" | "Disconnect" | "Cancel" | "End" | "Bulk" | "Clear" | "AutoMod" => Tone::Delete,
                _ => Tone::Update,
            };
            assert_eq!(tone(*action), expected, "{label}");
        }
    }
}
