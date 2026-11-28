// The settings that live only in Discord's protobuf user settings: message requests, activity sharing, quiet mode
// (PreloadedUserSettings, `PATCH /users/@me/settings-proto/1`). Field numbers are from the protobuf-ts
// descriptors in Discord's web bundle (web.355a76e660bee7a5.js, 2026-11-22). Just enough wire format to read
// and write them: varints and length-delimited fields.
use base64::{Engine, engine::general_purpose::STANDARD};

use crate::discord::AccountSettings;

const TEXT_AND_IMAGES: u64 = 6;
const NOTIFICATIONS: u64 = 7;
const PRIVACY: u64 = 8;
// NotificationSettings.quiet_mode, a google.protobuf.BoolValue.
const QUIET_MODE: u64 = 5;
// TextAndImagesSettings.dm_spam_filter_v2, enum DmSpamFilterV2.
const DM_SPAM_FILTER_V2: u64 = 27;
// PrivacySettings.default_message_request_restricted, a google.protobuf.BoolValue.
const DEFAULT_MESSAGE_REQUEST_RESTRICTED: u64 = 17;
const BOOL_VALUE: u64 = 1;
// PrivacySettings.default_guilds_activity_restricted, enum GuildActivityStatusRestrictionDefault: OFF 0 (share
// in all servers), ON_FOR_LARGE_GUILDS 1 (only servers of 200 or fewer), ON 2 (no servers).
// Not verified: which of this and default_guilds_activity_restricted_v2 (29) Discord's select writes today.
const DEFAULT_GUILDS_ACTIVITY_RESTRICTED: u64 = 14;

// DmSpamFilterV2: DEFAULT_UNSET 0, DISABLED 1, NON_FRIENDS 2, FRIENDS_AND_NON_FRIENDS 3.
pub const SPAM_FILTER_NON_FRIENDS: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Field<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    Fixed,
}

impl AccountSettings {
    // READY's `user_settings_proto` (base64). Unset fields keep their defaults.
    pub fn apply_proto(&mut self, encoded: &str) {
        let Ok(bytes) = STANDARD.decode(encoded) else { return };
        if let Some(Field::Varint(filter)) = find(&bytes, &[TEXT_AND_IMAGES, DM_SPAM_FILTER_V2]) {
            self.dm_spam_filter = filter.min(3) as u8;
        }
        // An empty BoolValue is false.
        if let Some(Field::Bytes(restricted)) = find(&bytes, &[PRIVACY, DEFAULT_MESSAGE_REQUEST_RESTRICTED]) {
            self.message_requests = !matches!(find(restricted, &[BOOL_VALUE]), Some(Field::Varint(1..)));
        }
        if let Some(Field::Bytes(quiet)) = find(&bytes, &[NOTIFICATIONS, QUIET_MODE]) {
            self.quiet_mode = matches!(find(quiet, &[BOOL_VALUE]), Some(Field::Varint(1..)));
        }
        if let Some(Field::Varint(level)) = find(&bytes, &[PRIVACY, DEFAULT_GUILDS_ACTIVITY_RESTRICTED]) {
            self.default_guilds_activity_restricted = level.min(2) as u8;
        }
    }
}

// Each write carries only the changed field, base64 for the request body.
// Not verified: that Discord merges a partial settings proto rather than replacing the whole message.
pub fn spam_filter(value: u8) -> String {
    STANDARD.encode(message(TEXT_AND_IMAGES, &varint_field(DM_SPAM_FILTER_V2, value.into())))
}

pub fn message_requests(on: bool) -> String {
    let restricted = message(DEFAULT_MESSAGE_REQUEST_RESTRICTED, &varint_field(BOOL_VALUE, (!on).into()));
    STANDARD.encode(message(PRIVACY, &restricted))
}

pub fn guild_activity_default(level: u8) -> String {
    STANDARD.encode(message(PRIVACY, &varint_field(DEFAULT_GUILDS_ACTIVITY_RESTRICTED, level.min(2).into())))
}

// The field at `path`, nested messages first. A repeated message merges, so the last value found wins.
fn find<'a>(bytes: &'a [u8], path: &[u64]) -> Option<Field<'a>> {
    let (&number, rest) = path.split_first()?;
    let hits = fields(bytes).filter(|(n, _)| *n == number).map(|(_, f)| f);
    if rest.is_empty() {
        return hits.last();
    }
    hits.filter_map(|f| match f {
        Field::Bytes(inner) => find(inner, rest),
        _ => None,
    })
    .last()
}

fn fields(mut bytes: &[u8]) -> impl Iterator<Item = (u64, Field<'_>)> {
    std::iter::from_fn(move || {
        let key = varint(&mut bytes)?;
        let field = match key & 7 {
            0 => Field::Varint(varint(&mut bytes)?),
            2 => {
                let len = usize::try_from(varint(&mut bytes)?).ok().filter(|&len| len <= bytes.len())?;
                let (value, rest) = bytes.split_at(len);
                bytes = rest;
                Field::Bytes(value)
            }
            1 => {
                bytes = bytes.get(8..)?;
                Field::Fixed
            }
            5 => {
                bytes = bytes.get(4..)?;
                Field::Fixed
            }
            _ => return None,
        };
        Some((key >> 3, field))
    })
}

fn varint(bytes: &mut &[u8]) -> Option<u64> {
    let mut value = 0;
    for shift in (0..64).step_by(7) {
        let (&byte, rest) = bytes.split_first()?;
        *bytes = rest;
        value |= u64::from(byte & 0x7f) << shift;
        if byte < 0x80 {
            return Some(value);
        }
    }
    None
}

fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(value as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn varint_field(number: u64, value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    put_varint(&mut out, number << 3);
    put_varint(&mut out, value);
    out
}

fn message(number: u64, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    put_varint(&mut out, (number << 3) | 2);
    put_varint(&mut out, body.len() as u64);
    out.extend_from_slice(body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(encoded: &str) -> AccountSettings {
        let mut settings = AccountSettings::default();
        settings.apply_proto(encoded);
        settings
    }

    #[test]
    fn writes_only_the_changed_field() {
        // text_and_images (6) { dm_spam_filter_v2 (27) = 3 }
        assert_eq!(STANDARD.decode(spam_filter(3)).unwrap(), [0x32, 3, 0xd8, 0x01, 3]);
        // privacy (8) { default_message_request_restricted (17) { value (1) = true } }
        assert_eq!(STANDARD.decode(message_requests(false)).unwrap(), [0x42, 5, 0x8a, 0x01, 2, 0x08, 1]);
    }

    #[test]
    fn reads_back_what_it_writes() {
        assert_eq!(read(&spam_filter(1)).dm_spam_filter, 1);
        assert!(!read(&message_requests(false)).message_requests);
        assert!(read(&message_requests(true)).message_requests);
        assert_eq!(read(&guild_activity_default(1)).default_guilds_activity_restricted, 1);
        assert_eq!(read(&guild_activity_default(9)).default_guilds_activity_restricted, 2);
    }

    #[test]
    fn reads_quiet_mode() {
        let quiet = |on: u64| STANDARD.encode(message(NOTIFICATIONS, &message(QUIET_MODE, &varint_field(BOOL_VALUE, on))));
        assert!(read(&quiet(1)).quiet_mode);
        assert!(!read(&quiet(0)).quiet_mode);
        assert!(!read(&spam_filter(1)).quiet_mode);
    }

    #[test]
    fn reads_past_other_fields_and_keeps_defaults() {
        // versions (1) { client_version (1) = 300 }, a fixed64 at 30, then privacy with an empty BoolValue.
        let mut bytes = message(1, &varint_field(1, 300));
        bytes.extend([(30 << 3 | 1) as u8, 0, 0, 0, 0, 0, 0, 0, 0]);
        bytes.extend(message(PRIVACY, &message(DEFAULT_MESSAGE_REQUEST_RESTRICTED, &[])));
        let settings = read(&STANDARD.encode(&bytes));
        assert!(settings.message_requests);
        assert_eq!(settings.dm_spam_filter, AccountSettings::default().dm_spam_filter);
        assert_eq!(read("not base64!").dm_spam_filter, 0);
    }

    #[test]
    fn a_later_occurrence_wins_and_truncation_stops_cleanly() {
        let mut bytes = message(TEXT_AND_IMAGES, &varint_field(DM_SPAM_FILTER_V2, 1));
        bytes.extend(message(TEXT_AND_IMAGES, &varint_field(DM_SPAM_FILTER_V2, 3)));
        assert_eq!(read(&STANDARD.encode(&bytes)).dm_spam_filter, 3);
        assert_eq!(find(&[0x32, 9, 0xd8], &[TEXT_AND_IMAGES, DM_SPAM_FILTER_V2]), None);
    }
}
