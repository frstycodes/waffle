// Reaction frecency synced through Discord's user settings: settings-proto type 2
// (FrecencyUserSettings), field 13 `emoji_reaction_frecency`, an EmojiFrecency whose field 1 maps an
// emoji key to a FrecencyItem { total_uses: 1 uint32, recent_uses: 2 packed uint64 (unix ms),
// frecency: 3, score: 4 }. Schema read from Discord's web client (2026-11-21).
use base64::{Engine, engine::general_purpose::STANDARD};

use crate::discord::Emoji;

pub const SETTINGS_TYPE: u8 = 2;
const REACTION_FRECENCY: u64 = 13;

#[derive(Clone, Debug, PartialEq)]
pub struct Use {
    pub key: String,
    pub total: u32,
    // Unix milliseconds, oldest first.
    pub recent: Vec<u64>,
}

pub fn decode(settings: &str) -> Option<Vec<Use>> {
    let bytes = STANDARD.decode(settings).ok()?;
    let frecency = fields(&bytes)?.into_iter().find(|(n, _)| *n == REACTION_FRECENCY).and_then(|(_, v)| v.bytes())?;
    let mut out = Vec::new();
    for (n, entry) in fields(frecency)? {
        let Some(entry) = entry.bytes().filter(|_| n == 1) else { continue };
        let (mut key, mut item) = (None, None);
        for (n, v) in fields(entry)? {
            match n {
                1 => key = v.bytes().and_then(|b| String::from_utf8(b.to_vec()).ok()),
                2 => item = v.bytes(),
                _ => {}
            }
        }
        let (Some(key), Some(item)) = (key, item) else { continue };
        let mut found = Use { key, total: 0, recent: Vec::new() };
        for (n, v) in fields(item)? {
            match (n, v) {
                (1, Value::Varint(total)) => found.total = total as u32,
                (2, Value::Bytes(packed)) => found.recent.extend(varints(packed)?),
                (2, Value::Varint(at)) => found.recent.push(at),
                _ => {}
            }
        }
        out.push(found);
    }
    Some(out)
}

// A FrecencyUserSettings carrying only field 13, for PATCH: the server merges it over the rest.
pub fn encode(uses: &[Use]) -> String {
    let mut map = Vec::new();
    for u in uses {
        let mut item = Vec::new();
        if u.total > 0 {
            tag(&mut item, 1, 0);
            varint(&mut item, u64::from(u.total));
        }
        if !u.recent.is_empty() {
            let mut packed = Vec::new();
            for at in &u.recent {
                varint(&mut packed, *at);
            }
            bytes_field(&mut item, 2, &packed);
        }
        let mut entry = Vec::new();
        bytes_field(&mut entry, 1, u.key.as_bytes());
        bytes_field(&mut entry, 2, &item);
        bytes_field(&mut map, 1, &entry);
    }
    let mut out = Vec::new();
    bytes_field(&mut out, REACTION_FRECENCY, &map);
    STANDARD.encode(out)
}

// Discord keys a custom emoji by its id and a Unicode one by its name ("thumbsup", "100").
// Not verified: Discord's emoji names come from its own table, which isn't in the downloaded bundle;
// they match gemoji's word shortcodes ("thumbsup" over "+1"), so the first of those stands in.
pub fn key(emoji: &Emoji) -> Option<String> {
    if let Some(id) = &emoji.id {
        return Some(id.clone());
    }
    let found = emojis::get(&emoji.name)?;
    found.shortcodes().find(|s| s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).map(str::to_owned)
}

// `custom` finds a server emoji by id; one from a server you're not in is skipped.
pub fn emoji(key: &str, custom: impl Fn(&str) -> Option<Emoji>) -> Option<Emoji> {
    if key.bytes().all(|b| b.is_ascii_digit()) {
        return custom(key);
    }
    emojis::get_by_shortcode(key).map(|e| Emoji::unicode(e.as_str()))
}

enum Value<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
}

impl<'a> Value<'a> {
    fn bytes(self) -> Option<&'a [u8]> {
        match self {
            Value::Bytes(b) => Some(b),
            Value::Varint(_) => None,
        }
    }
}

// Top-level fields of a message; fixed-width ones are skipped. None if malformed.
fn fields(mut buf: &[u8]) -> Option<Vec<(u64, Value<'_>)>> {
    let mut out = Vec::new();
    while !buf.is_empty() {
        let head = read_varint(&mut buf)?;
        let (number, wire) = (head >> 3, head & 7);
        match wire {
            0 => out.push((number, Value::Varint(read_varint(&mut buf)?))),
            1 => buf = buf.get(8..)?,
            2 => {
                let len = usize::try_from(read_varint(&mut buf)?).ok()?;
                out.push((number, Value::Bytes(buf.get(..len)?)));
                buf = &buf[len..];
            }
            5 => buf = buf.get(4..)?,
            _ => return None,
        }
    }
    Some(out)
}

fn varints(mut buf: &[u8]) -> Option<Vec<u64>> {
    let mut out = Vec::new();
    while !buf.is_empty() {
        out.push(read_varint(&mut buf)?);
    }
    Some(out)
}

fn read_varint(buf: &mut &[u8]) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let (&byte, rest) = buf.split_first()?;
        *buf = rest;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

fn varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn tag(out: &mut Vec<u8>, number: u64, wire: u64) {
    varint(out, number << 3 | wire);
}

fn bytes_field(out: &mut Vec<u8>, number: u64, bytes: &[u8]) {
    tag(out, number, 2);
    varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_skips_other_fields() {
        let uses = vec![Use { key: "thumbsup".into(), total: 7, recent: vec![1_700_000_000_000, 1_700_000_100_000] }, Use { key: "1234".into(), total: 1, recent: vec![5] }];
        let encoded = encode(&uses);
        assert_eq!(decode(&encoded), Some(uses.clone()));
        // Another field (6, emoji_frecency) before ours is ignored.
        let mut raw = Vec::new();
        bytes_field(&mut raw, 6, &[8, 1]);
        raw.extend(STANDARD.decode(&encoded).unwrap());
        assert_eq!(decode(&STANDARD.encode(raw)), Some(uses));
    }

    #[test]
    fn keys_use_discord_names() {
        assert_eq!(key(&Emoji::unicode("👍")).as_deref(), Some("thumbsup"));
        assert_eq!(key(&Emoji { id: Some("42".into()), name: "blob".into(), animated: false }).as_deref(), Some("42"));
        assert_eq!(emoji("thumbsup", |_| None), Some(Emoji::unicode("👍")));
    }
}
