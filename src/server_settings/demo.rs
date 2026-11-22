// WAFFLE_DEMO scenes for server settings: made-up roles, bans, invites, rules and an audit log covering
// many action types, loaded in place of Discord's answers.
use gpui_kit::{Context, Window};
use serde_json::{Value, json};

use super::{
    access::Page,
    api::{AuditQuery, Overview},
    audit,
    lists::{self, AuditView},
    member_dates::{DateColumn, DateFilter, Preset},
    members,
    roles::{self, RoleEdit, RoleTab},
    view::Info,
};
use crate::{Waffle, channel_settings::Load};

pub fn open(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    waffle.open_server_settings("1".into(), window, cx);
    let page = match scene {
        "server-settings-roles" | "server-settings-role" | "server-settings-role-perms" => Page::Roles,
        "server-settings-audit" => Page::AuditLog,
        "server-settings-bans" => Page::Bans,
        "server-settings-engagement" => Page::Engagement,
        "server-settings-automod" => Page::AutoMod,
        "server-settings-invites" => Page::Invites,
        "server-settings-members" | "server-settings-members-bulk" | "server-settings-members-filter" => Page::Members,
        _ => Page::Profile,
    };
    let Some(view) = waffle.server_settings.as_mut() else { return };
    let saved = Overview { name: "Night Owls".into(), description: Some("Late-night builders, gpui tinkerers and people who should be asleep.".into()), system_channel_id: Some("general".into()), afk_channel_id: Some("hub".into()), afk_timeout: 900, default_message_notifications: 1, verification_level: 1, explicit_content_filter: 2, premium_progress_bar_enabled: true, ..Default::default() };
    view.info = Load::Ready(Info { saved: saved.clone(), features: vec!["COMMUNITY".into(), "ENHANCED_ROLE_COLORS".into(), "INVITE_SPLASH".into()], premium_tier: 2, boosts: 9, vanity: None, rules_channel_id: Some("general".into()), updates_channel_id: Some("staff".into()) });
    view.draft = saved.clone();
    view.widget = Load::Ready((true, Some("general".into())));
    view.roles.list = Load::Ready(roles::sorted(role_values().iter().filter_map(roles::parse_role).collect()));
    view.roles.counts = [("r1", 4u64), ("r2", 61), ("r3", 12), ("r4", 1), ("bot", 1)].iter().map(|(k, v)| (k.to_string(), *v)).collect();
    fill_lists(&mut view.lists);
    view.loaded.extend([Page::Profile, Page::Engagement, Page::Roles, Page::AuditLog, Page::Bans, Page::AutoMod, Page::Invites, Page::Emoji, Page::Stickers, Page::Soundboard, Page::Integrations, Page::Template, Page::Onboarding, Page::Members]);
    view.page = page;
    let (name, description) = (view.name.clone(), view.description.clone());
    name.update(cx, |s, cx| s.set_value(saved.name.clone(), window, cx));
    description.update(cx, |s, cx| s.set_value(saved.description.clone().unwrap_or_default(), window, cx));
    if matches!(scene, "server-settings-role" | "server-settings-role-perms") {
        let Some(view) = waffle.server_settings.as_mut() else { return };
        let Load::Ready(list) = &view.roles.list else { return };
        let Some(row) = list.iter().find(|r| r.id == "r3").cloned() else { return };
        let tab = if scene == "server-settings-role-perms" { RoleTab::Permissions } else { RoleTab::Display };
        view.roles.name.update(cx, |s, cx| s.set_value(row.role.name.clone(), window, cx));
        view.roles.open = Some(RoleEdit { id: row.id.clone(), tab, saved: row.role.clone(), draft: row.role.clone(), members: Load::Ready(vec!["8388608".into(), "12582912".into()]), asking_delete: false });
    }
    if scene == "server-settings-members-filter"
        && let Some(m) = waffle.server_settings.as_mut().map(|v| &mut v.lists.members)
    {
        filter_members_demo(m, Preset::FourWeeks);
    }
    if scene == "server-settings-members-bulk"
        && let Some(m) = waffle.server_settings.as_mut().map(|v| &mut v.lists.members)
        && let Load::Ready(rows) = &m.rows
    {
        m.selected = rows.iter().take(2).map(|r| r.id.clone()).collect();
    }
    if scene == "server-settings-dirty" {
        let Some(view) = waffle.server_settings.as_mut() else { return };
        view.name.update(cx, |s, cx| s.set_value("Night Owls Club", window, cx));
        view.draft.system_channel_flags = super::api::system_flags::SETUP_TIPS;
    }
    cx.notify();
}

fn role_values() -> Vec<Value> {
    let role = |id: &str, name: &str, position: i64, color: u32, permissions: u64, managed: bool| json!({ "id": id, "name": name, "position": position, "colors": { "primary_color": color }, "permissions": permissions.to_string(), "managed": managed, "hoist": position > 2 });
    vec![
        role("r1", "Council", 5, 0xE91E63, 1 << 3, false),
        role("bot", "Clip bot", 4, 0, (1 << 11) | (1 << 14), true),
        role("r3", "Moderators", 3, 0x3498DB, (1 << 1) | (1 << 2) | (1 << 7) | (1 << 13) | (1 << 40), false),
        role("r2", "Rustacean", 2, 0xE67E22, 1 << 10, false),
        role("r4", "A role whose name keeps going well past where any sensible label would stop", 1, 0x1ABC9C, 0, false),
        role("1", "@everyone", 0, 0, (1 << 10) | (1 << 11) | (1 << 6) | (1 << 16) | (1 << 20) | (1 << 21), false),
    ]
}

fn fill_lists(l: &mut lists::Lists) {
    let user = |id: &str, name: &str| json!({ "id": id, "username": name, "global_name": name });
    l.bans = Load::Ready(
        [("31", "spam_bot_4471", Some("Posting scam links in every channel")), ("32", "Bartholomew Featherstonehaugh-Wright", None), ("33", "mx", Some("Raid"))]
            .iter()
            .filter_map(|(id, name, reason)| lists::parse_ban(&json!({ "user": user(id, name), "reason": reason })))
            .collect(),
    );
    l.invites = Load::Ready(
        [("7Kq2xQ", "Wren", "general", 12, 25, Some("2099-01-01T00:00:00Z")), ("hangout", "kiwi", "hangout", 412, 0, None)]
            .iter()
            .filter_map(|(code, by, ch, uses, max, exp)| lists::parse_invite(&json!({ "code": code, "inviter": user("1", by), "channel": { "name": ch }, "uses": uses, "max_uses": max, "expires_at": exp })))
            .collect(),
    );
    l.rules = Load::Ready(vec![
        lists::Rule { id: "a1".into(), name: "Block mention spam".into(), enabled: true, trigger_type: 5, words: Vec::new() },
        lists::Rule { id: "a2".into(), name: "Block custom words".into(), enabled: false, trigger_type: 1, words: vec!["free nitro".into(), "airdrop*".into()] },
    ]);
    let rows: Vec<members::Member> = member_results().iter().filter_map(members::parse_result).collect();
    (l.members.total, l.members.rows) = (rows.len() as u64, Load::Ready(rows));
    l.emojis = Load::Ready(Vec::new());
    l.stickers = Load::Ready(Vec::new());
    l.sounds = Load::Ready(Vec::new());
    l.integrations = Load::Ready(Vec::new());
    l.webhooks = Load::Ready(Vec::new());
    l.templates = Load::Ready(Vec::new());
    l.onboarding = Load::Ready(json!({ "enabled": true, "prompts": [{}, {}], "default_channel_ids": ["general"] }));
    let page = audit::parse(&audit_log());
    let mut names = page.names;
    names.channels.extend([("general", "👋-general"), ("hub", "study-hall"), ("lounge", "hangout")].map(|(a, b)| (a.to_owned(), b.to_owned())));
    names.roles.extend([("r1", "Council"), ("r3", "Moderators")].map(|(a, b)| (a.to_owned(), b.to_owned())));
    l.audit = AuditView { entries: page.entries, names, query: AuditQuery::default(), next: Some("1".into()), loading: false, error: None, open: [(1_572_600_000_000_000_000u64 - 5 * 40_000_000_000_000).to_string()].into_iter().collect() };
}

// Six members-search entries, newest member first, covering every signal and join method shape: an invite,
// the vanity URL, a bot invite, Server discovery and a Twitch integration. Times count back from now so the
// signals stay live.
fn member_results() -> Vec<Value> {
    let now = chrono::Utc::now();
    let ago = |minutes: i64| (now - chrono::Duration::minutes(minutes)).to_rfc3339();
    let made = |minutes: i64| ((((now - chrono::Duration::minutes(minutes)).timestamp_millis() - 1_420_070_400_000) as u64) << 22).to_string();
    const DAY: i64 = 24 * 60;
    let entry = |id: String, username: &str, global: Option<&str>, joined: i64, extra: Value, join: Value| {
        let mut member = json!({ "user": { "id": id, "username": username, "global_name": global }, "joined_at": ago(joined), "roles": [], "flags": 0 });
        merge(&mut member, extra);
        let mut out = json!({ "member": member });
        merge(&mut out, join);
        out
    };
    vec![
        entry(made(60), "free_nitro_drop", None, 6, json!({ "flags": 128, "user": { "public_flags": 1 << 20 } }), json!({ "join_source_type": 5, "source_invite_code": "x7Kq2" })),
        entry(made(2 * DAY), "bartholomew.featherstonehaugh.wright", Some("Bartholomew Featherstonehaugh-Wright"), 120, json!({ "unusual_dm_activity_until": ago(-DAY), "roles": ["r2"] }), json!({ "join_source_type": 3 })),
        entry(made(400 * DAY), "clipbot", Some("Clip bot"), 20 * DAY, json!({ "user": { "bot": true }, "roles": ["r2"] }), json!({ "join_source_type": 1 })),
        entry(made(7 * 365 * DAY), "moss", Some("moss lantern"), 95 * DAY, json!({ "communication_disabled_until": ago(-42), "roles": ["r2"] }), json!({ "join_source_type": 6, "source_invite_code": "nightowls" })),
        entry(made(5 * 365 * DAY), "kiwi", Some("kiwi"), 300 * DAY, json!({}), json!({ "join_source_type": 2, "integration_type": "twitch" })),
        entry(made(7 * 365 * DAY + 40 * DAY), "wren", Some("Wren"), 2 * 365 * DAY, json!({ "nick": "Wren (mod)", "roles": ["r1", "r2"] }), json!({ "join_source_type": 5, "source_invite_code": "7Kq2xQ" })),
    ]
}

// The table as Discord would answer a Member since filter: only the rows that joined inside it, with the
// menu open on the preset.
fn filter_members_demo(m: &mut members::Members, preset: Preset) {
    let range = preset.range(chrono::Utc::now());
    let after = range.and_then(|r| r.after_ms).unwrap_or(i64::MIN);
    (m.joined_menu.filter, m.filter.joined, m.pop) = (DateFilter::Preset(preset), range, Some(members::Pop::Date(DateColumn::Joined)));
    if let Load::Ready(rows) = &mut m.rows {
        rows.retain(|r| r.joined_at.is_some_and(|t| t.timestamp_millis() >= after));
        m.total = rows.len() as u64;
    }
}

// Copies `extra`'s keys onto `into`, object by object.
fn merge(into: &mut Value, extra: Value) {
    let (Some(into), Value::Object(extra)) = (into.as_object_mut(), extra) else { return };
    for (key, value) in extra {
        match into.get_mut(&key) {
            Some(slot) if slot.is_object() && value.is_object() => merge(slot, value),
            _ => _ = into.insert(key, value),
        }
    }
}

// Made-up entries, newest first; ids are snowflakes from November 2026.
fn audit_log() -> Value {
    let base: u64 = 1_572_600_000_000_000_000;
    let id = |n: u64| (base - n * 40_000_000_000_000).to_string();
    json!({
        "users": [
            { "id": "4194304", "username": "kiwi", "global_name": "kiwi" },
            { "id": "8388608", "username": "wren", "global_name": "Wren" },
            { "id": "12582912", "username": "moss", "global_name": "moss lantern" },
            { "id": "31", "username": "spam_bot_4471" },
        ],
        "webhooks": [{ "id": "w1", "name": "Release notes" }],
        "audit_log_entries": [
            { "id": id(1), "action_type": 22, "user_id": "8388608", "target_id": "31", "reason": "Posting scam links in every channel" },
            { "id": id(2), "action_type": 25, "user_id": "4194304", "target_id": "12582912", "changes": [{ "key": "$add", "new_value": [{ "id": "r3", "name": "Moderators" }] }] },
            { "id": id(3), "action_type": 31, "user_id": "4194304", "target_id": "r3", "changes": [{ "key": "permissions", "old_value": "6", "new_value": "1099511635974" }, { "key": "color", "old_value": 0, "new_value": 3447003 }] },
            { "id": id(4), "action_type": 24, "user_id": "8388608", "target_id": "12582912", "changes": [{ "key": "communication_disabled_until", "new_value": "2026-11-19T10:00:00Z" }] },
            { "id": id(5), "action_type": 1, "user_id": "4194304", "changes": [{ "key": "name", "old_value": "Night Shift", "new_value": "Night Owls" }, { "key": "afk_timeout", "old_value": 300, "new_value": 900 }, { "key": "system_channel_id", "new_value": "general" }] },
            { "id": id(6), "action_type": 72, "user_id": "8388608", "target_id": "31", "options": { "channel_id": "general", "count": "14" } },
            { "id": id(7), "action_type": 14, "user_id": "4194304", "target_id": "general", "options": { "id": "r3", "type": "0", "role_name": "Moderators" }, "changes": [{ "key": "allow", "old_value": "0", "new_value": "8192" }] },
            { "id": id(8), "action_type": 143, "user_id": "4194304", "target_id": "31", "options": { "channel_id": "general", "auto_moderation_rule_name": "Block mention spam", "auto_moderation_rule_trigger_type": "5" } },
            { "id": id(9), "action_type": 40, "user_id": "8388608", "changes": [{ "key": "code", "new_value": "7Kq2xQ" }, { "key": "max_uses", "new_value": 25 }] },
            { "id": id(10), "action_type": 50, "user_id": "4194304", "target_id": "w1", "changes": [{ "key": "name", "new_value": "Release notes" }] },
            { "id": id(11), "action_type": 32, "user_id": "4194304", "target_id": "r9", "changes": [{ "key": "name", "old_value": "Helpers" }] },
            { "id": id(12), "action_type": 26, "user_id": "8388608", "options": { "channel_id": "hub", "count": "3" } },
            { "id": id(13), "action_type": 21, "user_id": "4194304", "options": { "delete_member_days": "30", "members_removed": "17" } },
            { "id": id(14), "action_type": 192, "user_id": "12582912", "options": { "channel_id": "lounge", "status": "lofi and bug fixing" } },
            { "id": id(15), "action_type": 23, "user_id": "8388608", "target_id": "33" },
            { "id": id(16), "action_type": 60, "user_id": "4194304", "changes": [{ "key": "name", "new_value": "ferris_wave" }] },
        ],
    })
}
