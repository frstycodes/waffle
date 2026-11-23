// The guard-* scenes: what stops a message from going out (slowmode, a timeout, a missing permission, a
// blocked DM).
use super::*;

pub(super) fn demo_guard(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    use crate::discord::{Overwrite, VIEW_CHANNEL, perm};
    if scene == "guard-blocked-dm" {
        let wren = user("8388608", "Wren");
        waffle.relationships.push(Relationship { user: wren, kind: Relation::Blocked, since: None });
        let dm = waffle.dms.iter().find(|c| c.id == "dm").cloned();
        waffle.feeds.insert("dm".into(), Feed { loaded: true, ..Default::default() });
        waffle.restore = None;
        waffle.guild_id = None;
        if let Some(dm) = dm {
            waffle.open_channel(dm, window, cx);
        }
        return;
    }
    let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") else { return };
    guild.owner_id = Some("8388608".into());
    for role in guild.roles.iter_mut() {
        role.permissions = match role.id.as_str() {
            "1" => VIEW_CHANNEL | perm::SEND_MESSAGES,
            "r1" => perm::MANAGE_MESSAGES,
            _ => 0,
        };
    }
    if scene == "guard-timeout" {
        guild.timed_out_until = Some(chrono::Utc::now() + chrono::Duration::minutes(52));
    }
    if scene == "guard-immune" {
        guild.my_roles = vec!["r1".into()];
    }
    let Some(general) = guild.channels.iter_mut().find(|c| c.id == "general") else { return };
    match scene {
        "guard-no-permission" => general.overwrites = vec![
            Overwrite { id: "1".into(), allow: 0, deny: perm::SEND_MESSAGES, member: false },
            Overwrite { id: "r1".into(), allow: perm::SEND_MESSAGES, deny: 0, member: false },
            Overwrite { id: "r2".into(), allow: perm::SEND_MESSAGES, deny: 0, member: false },
        ],
        "guard-timeout" => {}
        _ => general.props.slowmode = 120,
    }
    let Some(general) = guild.channels.iter().find(|c| c.id == "general").cloned() else { return };
    waffle.open_channel(general, window, cx);
    if scene == "guard-slowmode-counting" || scene == "guard-slowmode-queued" {
        waffle.start_countdown("general", std::time::Duration::from_secs(118), cx);
    }
    if scene == "guard-slowmode-queued" {
        waffle.drafts.set("general", Draft::new(&InputContent::new("gg, rematch at 9?"), None));
        waffle.restore_draft(window, cx);
        waffle.hold_for_slowmode("general", cx);
    }
}
