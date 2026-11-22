// Which app commands you use, ranked as Discord's client ranks them: its synced frecency (settings proto
// type 2, FrecencyUserSettings `application_command_frecency` = 7 and `application_frecency` = 9, read
// from its bundle's descriptors), with this session's runs replayed on top, as its store does.
// When that proto can't be fetched, the history Waffle kept from earlier runs stands in.
// GAP: runs from here aren't written back to Discord's settings, so other clients don't see them.
use std::{collections::HashMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::app_commands::{AppCommand, CommandIndex};

const DAY_MS: u64 = 86_400_000;
// Discord's Frecency: the newest 10 uses count.
const MAX_SAMPLES: usize = 10;
// The browser's first section: the top 5 by score.
const TOP: usize = 5;
// Discord's `numFrequentlyItems` for both stores.
const FREQUENT: usize = 100;
// Discord's `computeBonus`: 1 for commands, 100 for apps.
const COMMAND_BONUS: f64 = 1.;
const APP_BONUS: f64 = 100.;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Usage {
    pub total_uses: u32,
    // Milliseconds since the epoch, oldest first.
    pub recent_uses: Vec<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct History {
    pub commands: HashMap<String, Usage>,
    pub apps: HashMap<String, Usage>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum Source {
    // Only what Waffle saw run from here.
    #[default]
    Local,
    Synced,
}

#[derive(Default)]
pub(crate) struct RecentCommands {
    history: History,
    pub(crate) source: Source,
    // Runs since the synced copy was read: replayed over it, as Discord's `pendingUsages`.
    pending: Vec<(String, String, u64)>,
    // None keeps the history in memory only (demo scenes).
    path: Option<PathBuf>,
}

impl RecentCommands {
    pub(crate) fn load() -> Self {
        Self::at(crate::data_dir().map(|dir| dir.join("command-frecency.json")))
    }

    fn at(path: Option<PathBuf>) -> Self {
        let history = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        Self { history, path, ..Default::default() }
    }

    /// Discord's synced history replaces ours; runs since are kept on top.
    pub(crate) fn sync(&mut self, synced: History) {
        self.history = synced;
        self.source = Source::Synced;
        for (command, app, at) in self.pending.clone() {
            track(&mut self.history.commands, &command, at);
            track(&mut self.history.apps, &app, at);
        }
        self.save();
    }

    pub(crate) fn record(&mut self, command: &AppCommand, guild: Option<&str>, now: u64) {
        let key = key(command, guild);
        track(&mut self.history.commands, &key, now);
        track(&mut self.history.apps, &command.application_id, now);
        self.pending.push((key, command.application_id.clone(), now));
        self.save();
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string(&self.history) {
            let _ = std::fs::write(path, text);
        }
    }

    /// How each command and app in `index` ranks here, and the browser's top commands.
    pub(crate) fn ranks(&self, index: &CommandIndex, guild: Option<&str>, now: u64) -> Ranks {
        let command: Vec<f64> = index.commands.iter().map(|c| score(&self.history.commands, &key(c, guild), COMMAND_BONUS, now)).collect();
        let app = index.apps.iter().map(|a| score(&self.history.apps, &a.id, APP_BONUS, now)).collect();
        let frequent = here(frequently(&self.history.commands, COMMAND_BONUS, now), guild);
        let mut top: Vec<usize> = (0..index.commands.len()).filter(|&i| frequent.contains(&command_id(&index.commands[i]))).collect();
        top.sort_by(|&a, &b| command[b].total_cmp(&command[a]));
        top.truncate(TOP);
        Ranks { command, app, top }
    }
}

/// What the picker sorts by: each command's and app's score (higher first), and the top commands.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Ranks {
    pub command: Vec<f64>,
    pub app: Vec<f64>,
    pub top: Vec<usize>,
}

// Discord's id for a flattened subcommand joins the path with NULs.
fn command_id(command: &AppCommand) -> String {
    std::iter::once(command.id.as_str()).chain(command.path.iter().skip(1).map(String::as_str)).collect::<Vec<_>>().join("\0")
}

/// Discord's frecency key: a server's own command is counted per server.
pub(crate) fn key(command: &AppCommand, guild: Option<&str>) -> String {
    let id = command_id(command);
    match (&command.guild_id, guild) {
        (Some(_), Some(guild)) => format!("{id}:{guild}"),
        _ => id,
    }
}

// Discord's `RG`: keys scoped to another server drop out, the rest lose their scope.
fn here(keys: Vec<String>, guild: Option<&str>) -> Vec<String> {
    keys.into_iter()
        .filter_map(|k| match k.split_once(':') {
            Some((id, scope)) => (guild == Some(scope)).then(|| id.to_owned()),
            None => Some(k),
        })
        .collect()
}

fn track(history: &mut HashMap<String, Usage>, key: &str, at: u64) {
    let usage = history.entry(key.to_owned()).or_default();
    usage.total_uses += 1;
    usage.recent_uses.push(at);
    usage.recent_uses.sort_unstable();
    let excess = usage.recent_uses.len().saturating_sub(MAX_SAMPLES);
    usage.recent_uses.drain(..excess);
}

// Discord's `computeWeight`, by whole days since a use.
fn weight(days: u64) -> f64 {
    match days {
        0..=3 => 100.,
        4..=15 => 70.,
        16..=30 => 50.,
        31..=45 => 30.,
        46..=80 => 10.,
        _ => 1.,
    }
}

fn score(history: &HashMap<String, Usage>, key: &str, bonus: f64, now: u64) -> f64 {
    let Some(usage) = history.get(key) else { return 0. };
    usage.recent_uses.iter().take(MAX_SAMPLES).map(|&at| bonus / 100. * weight(now.saturating_sub(at) / DAY_MS)).sum()
}

// Discord's `frequently`: keys by frecency, highest first; frecency = ceil(total × score / samples).
fn frequently(history: &HashMap<String, Usage>, bonus: f64, now: u64) -> Vec<String> {
    let mut ranked: Vec<(f64, &String)> = history
        .iter()
        .filter(|(_, u)| !u.recent_uses.is_empty())
        .map(|(k, u)| ((f64::from(u.total_uses) * (score(history, k, bonus, now) / u.recent_uses.len() as f64)).ceil(), k))
        .collect();
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(b.1)));
    ranked.into_iter().take(FREQUENT).map(|(_, k)| k.clone()).collect()
}

/// FrecencyUserSettings, as `GET /users/@me/settings-proto/2` carries it (base64 in `settings`).
pub(crate) fn decode(proto: &[u8]) -> Option<History> {
    let mut history = History::default();
    for (field, value) in fields(proto)? {
        match (field, value) {
            (7, Field::Bytes(b)) => history.commands = usage_map(b)?,
            (9, Field::Bytes(b)) => history.apps = usage_map(b)?,
            _ => {}
        }
    }
    Some(history)
}

// `{ 1: map<string, FrecencyItem> }`; a FrecencyItem is `{ 1: total_uses, 2: repeated recent_uses, 3: frecency, 4: score }`.
fn usage_map(proto: &[u8]) -> Option<HashMap<String, Usage>> {
    let mut map = HashMap::new();
    for (field, value) in fields(proto)? {
        let (1, Field::Bytes(entry)) = (field, value) else { continue };
        let (mut key, mut usage) = (String::new(), Usage::default());
        for (field, value) in fields(entry)? {
            match (field, value) {
                (1, Field::Bytes(k)) => key = String::from_utf8(k.to_vec()).ok()?,
                (2, Field::Bytes(item)) => usage = item_usage(item)?,
                _ => {}
            }
        }
        map.insert(key, usage);
    }
    Some(map)
}

fn item_usage(proto: &[u8]) -> Option<Usage> {
    let mut usage = Usage::default();
    for (field, value) in fields(proto)? {
        match (field, value) {
            (1, Field::Varint(n)) => usage.total_uses = n as u32,
            (2, Field::Varint(at)) => usage.recent_uses.push(at),
            // proto3 packs repeated scalars.
            (2, Field::Bytes(mut packed)) => {
                while !packed.is_empty() {
                    usage.recent_uses.push(varint(&mut packed)?);
                }
            }
            _ => {}
        }
    }
    usage.recent_uses.retain(|&at| at > 0);
    usage.recent_uses.sort_unstable();
    Some(usage)
}

enum Field<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    Fixed,
}

fn fields(mut proto: &[u8]) -> Option<Vec<(u64, Field<'_>)>> {
    let mut out = Vec::new();
    while !proto.is_empty() {
        let tag = varint(&mut proto)?;
        let value = match tag & 7 {
            0 => Field::Varint(varint(&mut proto)?),
            1 => skip(&mut proto, 8)?,
            2 => {
                let len = usize::try_from(varint(&mut proto)?).ok()?;
                let (bytes, rest) = (proto.get(..len)?, proto.get(len..)?);
                proto = rest;
                Field::Bytes(bytes)
            }
            5 => skip(&mut proto, 4)?,
            _ => return None,
        };
        out.push((tag >> 3, value));
    }
    Some(out)
}

fn skip<'a>(proto: &mut &'a [u8], n: usize) -> Option<Field<'a>> {
    *proto = proto.get(n..)?;
    Some(Field::Fixed)
}

fn varint(proto: &mut &[u8]) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let (&byte, rest) = proto.split_first()?;
        *proto = rest;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000_000;

    fn command(id: &str, app: &str, path: &str, guild: Option<&str>) -> AppCommand {
        AppCommand { id: id.into(), application_id: app.into(), path: path.split(' ').map(str::to_owned).collect(), guild_id: guild.map(str::to_owned), ..Default::default() }
    }

    fn put_varint(out: &mut Vec<u8>, mut n: u64) {
        loop {
            let byte = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                out.push(byte);
                return;
            }
            out.push(byte | 0x80);
        }
    }

    fn put_bytes(out: &mut Vec<u8>, field: u64, bytes: &[u8]) {
        put_varint(out, field << 3 | 2);
        put_varint(out, bytes.len() as u64);
        out.extend_from_slice(bytes);
    }

    fn usage_entry(key: &str, total: u64, uses: &[u64]) -> Vec<u8> {
        let mut item = Vec::new();
        put_varint(&mut item, 1 << 3);
        put_varint(&mut item, total);
        let mut packed = Vec::new();
        for &at in uses {
            put_varint(&mut packed, at);
        }
        put_bytes(&mut item, 2, &packed);
        put_varint(&mut item, 3 << 3);
        put_varint(&mut item, 99);
        let mut entry = Vec::new();
        put_bytes(&mut entry, 1, key.as_bytes());
        put_bytes(&mut entry, 2, &item);
        entry
    }

    #[test]
    fn decodes_command_and_app_frecency() {
        let mut commands = Vec::new();
        put_bytes(&mut commands, 1, &usage_entry("10\0queue", 4, &[NOW - DAY_MS, NOW - 2 * DAY_MS]));
        let mut apps = Vec::new();
        put_bytes(&mut apps, 1, &usage_entry("w", 9, &[NOW]));
        let mut proto = Vec::new();
        put_bytes(&mut proto, 2, b"favorite gifs, skipped");
        put_bytes(&mut proto, 7, &commands);
        put_bytes(&mut proto, 9, &apps);
        let history = decode(&proto).unwrap();
        assert_eq!(history.commands["10\0queue"], Usage { total_uses: 4, recent_uses: vec![NOW - 2 * DAY_MS, NOW - DAY_MS] });
        assert_eq!(history.apps["w"].total_uses, 9);
        assert_eq!(decode(&[0x3a, 0x05, 0x01]), None, "a length past the end");
    }

    #[test]
    fn scores_by_age_as_discord_weighs_them() {
        let mut history = HashMap::new();
        track(&mut history, "a", NOW - 2 * DAY_MS);
        track(&mut history, "a", NOW - 20 * DAY_MS);
        track(&mut history, "b", NOW - 100 * DAY_MS);
        assert_eq!(score(&history, "a", COMMAND_BONUS, NOW), 1.5);
        assert_eq!(score(&history, "b", APP_BONUS, NOW), 1.);
        assert_eq!(frequently(&history, COMMAND_BONUS, NOW), ["a", "b"]);
        for n in 0..12 {
            track(&mut history, "c", NOW - n);
        }
        assert_eq!(history["c"].recent_uses.len(), MAX_SAMPLES);
        assert_eq!(history["c"].total_uses, 12);
    }

    #[test]
    fn server_commands_count_per_server() {
        let mine = command("5", "p", "poll", Some("g1"));
        assert_eq!(key(&mine, Some("g1")), "5:g1");
        assert_eq!(key(&command("10", "w", "music queue", None), Some("g1")), "10\0queue");
        assert_eq!(here(vec!["5:g1".into(), "5:g2".into(), "7".into()], Some("g1")), ["5", "7"]);
        assert_eq!(here(vec!["5:g1".into()], None), Vec::<String>::new());
    }

    #[test]
    fn the_top_five_come_from_the_synced_history_plus_new_runs() {
        let index = CommandIndex {
            apps: vec![crate::app_commands::App { id: "w".into(), name: "Wavebot".into(), icon: None }],
            commands: (0..7).map(|i| command(&i.to_string(), "w", &format!("c{i}"), None)).collect(),
        };
        let mut recent = RecentCommands::default();
        recent.record(&index.commands[6], None, NOW);
        let mut synced = History::default();
        for i in 0..6 {
            synced.commands.insert(i.to_string(), Usage { total_uses: 1, recent_uses: vec![NOW - (i as u64) * 10 * DAY_MS] });
        }
        recent.sync(synced);
        let ranks = recent.ranks(&index, None, NOW);
        assert_eq!(recent.source, Source::Synced);
        assert_eq!(ranks.top, [0, 6, 1, 2, 3]);
        assert!(ranks.app[0] > 0.);
    }

    #[test]
    fn local_history_survives_a_restart() {
        let path = std::env::temp_dir().join(format!("waffle-command-frecency-{}.json", std::process::id()));
        let play = command("10", "w", "play", None);
        let mut recent = RecentCommands::at(Some(path.clone()));
        recent.record(&play, None, NOW);
        assert_eq!(RecentCommands::at(Some(path.clone())).history.commands["10"].total_uses, 1);
        let _ = std::fs::remove_file(path);
    }
}
