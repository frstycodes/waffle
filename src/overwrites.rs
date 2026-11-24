// Permission overwrites as Discord edits them, for any settings page that grants per role or member
// (channel settings now; server settings' role pages later): the deny / inherit / allow model, the
// requests that save a changed set, category sync, the Private switch, and Discord web's permission groups.
use crate::discord::Method;
use serde_json::{Value, json};

use crate::{
    discord::{Overwrite, VIEW_CHANNEL, perm},
    ui::Tri,
};

#[derive(Clone, Debug, PartialEq)]
pub enum OverwriteOp {
    Put(Overwrite),
    Delete(String),
}

impl OverwriteOp {
    pub fn request(&self, channel_id: &str) -> (Method, String, Option<Value>) {
        match self {
            OverwriteOp::Put(o) => (
                Method::PUT,
                format!("/channels/{channel_id}/permissions/{}", o.id),
                Some(json!({ "allow": o.allow.to_string(), "deny": o.deny.to_string(), "type": if o.member { 1 } else { 0 } })),
            ),
            OverwriteOp::Delete(id) => (Method::DELETE, format!("/channels/{channel_id}/permissions/{id}"), None),
        }
    }
}

// One PUT per overwrite that's new or changed, one DELETE per overwrite that's gone.
pub fn overwrite_ops(before: &[Overwrite], after: &[Overwrite]) -> Vec<OverwriteOp> {
    let puts = after.iter().filter(|o| before.iter().find(|b| b.id == o.id) != Some(o)).cloned().map(OverwriteOp::Put);
    let deletes = before.iter().filter(|b| !after.iter().any(|o| o.id == b.id)).map(|b| OverwriteOp::Delete(b.id.clone()));
    puts.chain(deletes).collect()
}

// Discord calls a channel synced when its overwrites match its category's exactly, in any order.
pub fn synced(channel: &[Overwrite], category: &[Overwrite]) -> bool {
    let sorted = |list: &[Overwrite]| {
        let mut list = list.to_vec();
        list.sort_by(|a, b| a.id.cmp(&b.id));
        list
    };
    sorted(channel) == sorted(category)
}

pub fn tri(overwrites: &[Overwrite], id: &str, bit: u64) -> Tri {
    match overwrites.iter().find(|o| o.id == id) {
        Some(o) if o.deny & bit != 0 => Tri::Deny,
        Some(o) if o.allow & bit != 0 => Tri::Allow,
        _ => Tri::Inherit,
    }
}

pub fn set_tri(overwrites: &mut Vec<Overwrite>, id: &str, member: bool, bit: u64, value: Tri) {
    let index = match overwrites.iter().position(|o| o.id == id) {
        Some(i) => i,
        None => {
            overwrites.push(Overwrite { id: id.to_owned(), member, ..Default::default() });
            overwrites.len() - 1
        }
    };
    let o = &mut overwrites[index];
    o.allow &= !bit;
    o.deny &= !bit;
    match value {
        Tri::Allow => o.allow |= bit,
        Tri::Deny => o.deny |= bit,
        Tri::Inherit => {}
    }
}

// Discord's Private channel switch: @everyone (whose role id is the server's id) denied View channel.
pub fn private(overwrites: &[Overwrite], everyone: &str) -> bool {
    tri(overwrites, everyone, VIEW_CHANNEL) == Tri::Deny
}

pub fn set_private(overwrites: &mut Vec<Overwrite>, everyone: &str, raw_type: u8, on: bool) {
    let bits = private_bits(raw_type);
    set_tri(overwrites, everyone, false, bits, if on { Tri::Deny } else { Tri::Inherit });
}

// Discord's Advanced permissions, group for group as Discord web lists them (2026-11-18).
const GENERAL: &[(u64, &str)] = &[(VIEW_CHANNEL, "View channel"), (perm::MANAGE_CHANNELS, "Manage channel"), (perm::MANAGE_ROLES, "Manage permissions"), (perm::MANAGE_WEBHOOKS, "Manage webhooks")];
const MEMBERSHIP: &[(u64, &str)] = &[(perm::CREATE_INSTANT_INVITE, "Create invite")];
const SEND_MESSAGES: u64 = 1 << 11;
const TEXT: &[(u64, &str)] = &[
    (SEND_MESSAGES, "Send messages"),
    (1 << 38, "Send messages in threads"),
    (perm::CREATE_PUBLIC_THREADS, "Create public threads"),
    (1 << 36, "Create private threads"),
    (1 << 14, "Embed links"),
    (1 << 15, "Attach files"),
    (1 << 6, "Add reactions"),
    (1 << 18, "Use external emoji"),
    (1 << 37, "Use external stickers"),
    (1 << 17, "Mention @everyone, @here and all roles"),
    (perm::MANAGE_MESSAGES, "Manage messages"),
    (perm::PIN_MESSAGES, "Pin messages"),
    (1 << 52, "Bypass slowmode"),
    (perm::MANAGE_THREADS, "Manage threads"),
    (1 << 16, "Read message history"),
    (1 << 12, "Send text-to-speech messages"),
    (1 << 46, "Send voice messages"),
    (1 << 49, "Create polls"),
];
// A voice channel's own chat: no threads or pins.
const VOICE_CHAT: &[(u64, &str)] = &[
    (SEND_MESSAGES, "Send messages"),
    (1 << 14, "Embed links"),
    (1 << 15, "Attach files"),
    (1 << 6, "Add reactions"),
    (1 << 18, "Use external emoji"),
    (1 << 37, "Use external stickers"),
    (1 << 17, "Mention @everyone, @here and all roles"),
    (perm::MANAGE_MESSAGES, "Manage messages"),
    (1 << 52, "Bypass slowmode"),
    (1 << 16, "Read message history"),
    (1 << 12, "Send text-to-speech messages"),
    (1 << 46, "Send voice messages"),
    (1 << 49, "Create polls"),
];
const VOICE: &[(u64, &str)] = &[
    (perm::CONNECT, "Connect"),
    (1 << 21, "Speak"),
    (1 << 9, "Video"),
    (1 << 42, "Use soundboard"),
    (1 << 45, "Use external sounds"),
    (1 << 25, "Use voice activity"),
    (1 << 8, "Priority speaker"),
    (perm::MUTE_MEMBERS, "Mute members"),
    (perm::DEAFEN_MEMBERS, "Deafen members"),
    (perm::MOVE_MEMBERS, "Move members"),
    (1 << 48, "Set voice channel status"),
];
const EVENTS: &[(u64, &str)] = &[(1 << 44, "Create events"), (1 << 33, "Manage events")];
const APPS: &[(u64, &str)] = &[(1 << 31, "Use application commands"), (1 << 39, "Use activities"), (1 << 50, "Use external apps")];

// The permission groups Discord lists for a channel type (raw type: 2 voice, 4 category, 13 stage).
// GAP: stage and category groups follow the docs' channel-type columns; only text and voice were seen live.
pub fn permission_groups(raw_type: u8) -> Vec<(&'static str, &'static [(u64, &'static str)])> {
    match raw_type {
        2 => vec![("General", GENERAL), ("Membership", MEMBERSHIP), ("Voice", VOICE), ("Voice chat", VOICE_CHAT), ("Events", EVENTS), ("Apps", APPS)],
        13 => vec![("General", GENERAL), ("Membership", MEMBERSHIP), ("Stage", VOICE), ("Events", EVENTS), ("Apps", APPS)],
        4 => vec![("General", GENERAL), ("Membership", MEMBERSHIP), ("Text", TEXT), ("Voice", VOICE), ("Events", EVENTS), ("Apps", APPS)],
        _ => vec![("General", GENERAL), ("Membership", MEMBERSHIP), ("Text", TEXT), ("Apps", APPS)],
    }
}

// Private channel denies @everyone View channel, and Connect too where there's voice (Discord web's switch).
pub fn private_bits(raw_type: u8) -> u64 {
    if matches!(raw_type, 2 | 13) { VIEW_CHANNEL | perm::CONNECT } else { VIEW_CHANNEL }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ow(id: &str, allow: u64, deny: u64) -> Overwrite {
        Overwrite { id: id.into(), allow, deny, member: false }
    }

    #[test]
    fn overwrite_ops_put_changes_and_delete_removals() {
        let before = vec![ow("a", 1, 0), ow("b", 0, 2), ow("c", 4, 0)];
        let after = vec![ow("a", 1, 0), ow("b", 0, 6), ow("d", 0, 1024)];
        assert_eq!(overwrite_ops(&before, &after), vec![OverwriteOp::Put(ow("b", 0, 6)), OverwriteOp::Put(ow("d", 0, 1024)), OverwriteOp::Delete("c".into())]);
        let member = Overwrite { member: true, ..ow("u", 8, 0) };
        let (method, path, body) = OverwriteOp::Put(member).request("c");
        assert_eq!((method, path.as_str(), body), (Method::PUT, "/channels/c/permissions/u", Some(json!({ "allow": "8", "deny": "0", "type": 1 }))));
    }

    #[test]
    fn tri_state_edits_one_bit() {
        let mut list = vec![ow("r", 0b11, 0)];
        set_tri(&mut list, "r", false, 0b01, Tri::Deny);
        assert_eq!(list, vec![ow("r", 0b10, 0b01)]);
        assert_eq!(tri(&list, "r", 0b01), Tri::Deny);
        assert_eq!(tri(&list, "r", 0b10), Tri::Allow);
        set_tri(&mut list, "g", false, VIEW_CHANNEL, Tri::Deny);
        assert!(private(&list, "g"));
        set_tri(&mut list, "g", false, VIEW_CHANNEL, Tri::Inherit);
        assert!(!private(&list, "g"));
    }

    #[test]
    fn private_voice_also_denies_connect() {
        let mut list = Vec::new();
        set_private(&mut list, "g", 2, true);
        assert_eq!(list, vec![ow("g", 0, VIEW_CHANNEL | perm::CONNECT)]);
        assert!(private(&list, "g"));
        set_private(&mut list, "g", 2, false);
        assert_eq!(list, vec![ow("g", 0, 0)]);
        set_private(&mut list, "g", 0, true);
        assert_eq!(list, vec![ow("g", 0, VIEW_CHANNEL)]);
    }

    #[test]
    fn sync_ignores_order() {
        assert!(synced(&[ow("a", 1, 0), ow("b", 0, 1)], &[ow("b", 0, 1), ow("a", 1, 0)]));
        assert!(!synced(&[ow("a", 1, 0)], &[ow("a", 0, 1)]));
    }

}
