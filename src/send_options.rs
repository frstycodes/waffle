// Discord's per-send options as its composer reads them (docs/discord-send-options-re.md): a leading @silent,
// the @everyone / @here check ("Hold Up!"), and /tts.
use std::sync::LazyLock;

use regex::Regex;

use crate::{
    Waffle,
    discord::{ChannelKind, Message, SendOptions, perm},
};

const SILENT: &str = "@silent";
const TTS: &str = "/tts";
// Discord asks only when the mention would reach more people than this.
const HOLD_UP_OVER: u32 = 30;
pub const TTS_DESCRIPTION: &str = "Use text-to-speech to read the message to all members currently viewing the channel.";

/// What the composer sends for `typed`: the content without its `/tts` and `@silent` prefixes, and the
/// options they ask for. `tts` is false when /tts isn't on offer, leaving the text as typed.
pub fn parse(typed: &str, tts_allowed: bool) -> (String, SendOptions) {
    let (content, tts) = match strip_tts(typed).filter(|_| tts_allowed) {
        Some(text) => (text, true),
        None => (typed, false),
    };
    match strip_silent(content) {
        Some(rest) => (rest.to_owned(), SendOptions { silent: true, tts }),
        None => (content.to_owned(), SendOptions { silent: false, tts }),
    }
}

/// Discord's `^@silent(\s|$)`: the rest, trimmed.
pub fn strip_silent(content: &str) -> Option<&str> {
    let rest = content.strip_prefix(SILENT)?;
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim())
}

/// `/tts <text>`: the text. Discord's built-in command takes it as an option; Waffle has no slash commands,
/// so the typed prefix stands in for it.
pub fn strip_tts(content: &str) -> Option<&str> {
    let text = content.strip_prefix(TTS)?.strip_prefix(char::is_whitespace)?.trim();
    (!text.is_empty()).then_some(text)
}

/// The composer offers /tts while what's typed is the start of it.
pub fn offers_tts(typed: &str) -> bool {
    typed.starts_with('/') && TTS.starts_with(typed)
}

/// Discord's "Hold Up!" before a mass ping.
#[derive(Clone, Debug, PartialEq)]
pub struct HoldUp {
    pub mention: &'static str,
    pub count: u32,
    pub thread: bool,
}

impl HoldUp {
    /// `groups` are the server's member-list groups (id, count); @here leaves out "offline".
    pub fn check<'a>(content: &str, groups: impl IntoIterator<Item = (&'a str, u32)>, can_mention_everyone: bool, thread: bool) -> Option<Self> {
        if !can_mention_everyone {
            return None;
        }
        let mention = everyone_mention(content)?;
        let count = groups.into_iter().filter(|(id, _)| mention == "@everyone" || *id != "offline").map(|(_, n)| n).sum();
        (count > HOLD_UP_OVER).then_some(Self { mention, count, thread })
    }

    /// The rest of the body after the bold mention.
    pub fn after_mention(&self) -> String {
        let place = if self.thread { " in this thread" } else { "" };
        format!(", you are about to mention {}+ people{place}. Are you sure?", leading_digit(self.count))
    }
}

/// The first @everyone or @here outside inline code and code blocks; role mentions (`<@&id>`) never count.
pub fn everyone_mention(content: &str) -> Option<&'static str> {
    static MENTION: LazyLock<Regex> = LazyLock::new(|| Regex::new("@(everyone|here)").expect("valid regex"));
    let prose = outside_code(content);
    let found = MENTION.captures(&prose)?;
    Some(if &found[1] == "everyone" { "@everyone" } else { "@here" })
}

// Drops every backtick run and what it encloses up to the same run; an unclosed run is plain text, as markdown has it.
fn outside_code(content: &str) -> String {
    let mut prose = String::new();
    let mut rest = content;
    while let Some(start) = rest.find('`') {
        prose.push_str(&rest[..start]);
        let after = &rest[start..];
        let fence = &after[..after.len() - after.trim_start_matches('`').len()];
        match after[fence.len()..].find(fence) {
            Some(end) => rest = &after[fence.len() * 2 + end..],
            None => {
                prose.push_str(after);
                return prose;
            }
        }
    }
    prose.push_str(rest);
    prose
}

/// Discord's shown count: cut to its leading digit, with thousands separators (1234 → "1,000").
pub fn leading_digit(n: u32) -> String {
    let step = 10u64.pow(n.max(1).ilog10());
    crate::link_preview::grouped(u64::from(n) / step * step)
}

/// What Discord says aloud for a /tts message: "{name} said {text}".
pub fn utterance(name: &str, content: &str, names: impl Fn(&str) -> Option<String>) -> String {
    format!("{name} said {}", spoken(content, names))
}

// Not verified: a rough plain-text pass, not Discord's own. Mentions read as names (`names` maps a user id),
// custom emoji as their name, spoilers as "Spoiler"; other markup characters are dropped.
fn spoken(content: &str, names: impl Fn(&str) -> Option<String>) -> String {
    static SPOILER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\|\|[\s\S]*?\|\|").expect("valid regex"));
    static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<(@!?|@&|#|a?:(\w+):)(\d+)>").expect("valid regex"));
    let text = SPOILER.replace_all(content, "Spoiler");
    let text = TAG.replace_all(&text, |c: &regex::Captures| match (&c[1], c.get(2)) {
        (_, Some(emoji)) => emoji.as_str().to_owned(),
        ("@" | "@!", _) => names(&c[3]).unwrap_or_default(),
        _ => String::new(),
    });
    let text: String = text.chars().filter(|c| !matches!(c, '*' | '_' | '~' | '`' | '>' | '#')).collect();
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Speaks `text` with the OS voice off the UI thread: `say` on macOS, System.Speech on Windows, nothing elsewhere.
pub fn speak(text: String) {
    std::thread::spawn(move || {
        let _ = speak_now(&text);
    });
}

#[cfg(target_os = "macos")]
fn speak_now(text: &str) -> std::io::Result<()> {
    use std::io::Write;
    // On stdin, not as an argument: text starting with "-" would read as a flag.
    let mut say = std::process::Command::new("say").stdin(std::process::Stdio::piped()).spawn()?;
    say.stdin.take().map(|mut stdin| stdin.write_all(text.as_bytes())).transpose()?;
    say.wait().map(drop)
}

#[cfg(target_os = "windows")]
fn speak_now(text: &str) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // Through the environment, not the script: the text is never parsed as PowerShell.
    // Not verified: run on Windows.
    let script = "Add-Type -AssemblyName System.Speech; (New-Object System.Speech.Synthesis.SpeechSynthesizer).Speak($env:WAFFLE_TTS)";
    std::process::Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", script]).env("WAFFLE_TTS", text).creation_flags(CREATE_NO_WINDOW).status().map(drop)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn speak_now(_: &str) -> std::io::Result<()> {
    Ok(())
}

impl Waffle {
    // Whether we hold `bit` in the open server channel; never true in a DM.
    fn can_in_open_channel(&self, bit: u64) -> bool {
        let (Some(guild), Some(channel)) = (self.guild(), &self.channel) else { return false };
        guild.channel_permissions(self.my_id(), channel) & bit != 0
    }

    /// Discord's /tts predicate: a server channel, the setting on, and Send Text-to-Speech Messages.
    pub(crate) fn tts_allowed(&self) -> bool {
        self.prefs.tts_command && self.can_in_open_channel(perm::SEND_TTS_MESSAGES)
    }

    pub(crate) fn hold_up_for(&self, content: &str) -> Option<HoldUp> {
        let (guild, channel) = (self.guild()?, self.channel.as_ref()?);
        let groups = self.member_groups.get(&guild.id)?.iter().map(|(id, n)| (id.as_str(), *n));
        HoldUp::check(content, groups, self.can_in_open_channel(perm::MENTION_EVERYONE), channel.kind == ChannelKind::Thread)
    }

    /// Reads a /tts message aloud when it lands in the open channel from someone else, with the setting on.
    pub(crate) fn speak_tts(&self, message: &Message, is_open: bool, mine: bool) {
        if !message.tts || !is_open || mine || !self.prefs.tts_command {
            return;
        }
        let name = self.member_name(&message.author);
        let names = |id: &str| message.mentions.iter().find(|u| u.id == id).map(|u| self.member_name(u));
        speak(utterance(&name, &message.content, names));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_strips_only_a_leading_word() {
        assert_eq!(strip_silent("@silent late-night patch"), Some("late-night patch"));
        assert_eq!(strip_silent("@silent"), Some(""));
        assert_eq!(strip_silent("@silent\n  hi "), Some("hi"));
        assert_eq!(strip_silent("@silently"), None);
        assert_eq!(strip_silent(" @silent hi"), None);
        assert_eq!(strip_silent("hi @silent"), None);
    }

    #[test]
    fn tts_needs_text_and_permission() {
        assert_eq!(strip_tts("/tts dinner's ready"), Some("dinner's ready"));
        assert_eq!(strip_tts("/tts "), None);
        assert_eq!(strip_tts("/ttsx hi"), None);
        assert_eq!(parse("/tts hi", true), ("hi".into(), SendOptions { silent: false, tts: true }));
        assert_eq!(parse("/tts hi", false), ("/tts hi".into(), SendOptions::default()));
        assert_eq!(parse("/tts @silent hi", true), ("hi".into(), SendOptions { silent: true, tts: true }));
        assert_eq!(parse("@silent hi", false), ("hi".into(), SendOptions { silent: true, tts: false }));
    }

    #[test]
    fn tts_row_follows_the_typed_prefix() {
        assert!(offers_tts("/"));
        assert!(offers_tts("/tt"));
        assert!(offers_tts("/tts"));
        assert!(!offers_tts("/tts "));
        assert!(!offers_tts("/x"));
        assert!(!offers_tts(""));
    }

    #[test]
    fn everyone_outside_code_only() {
        assert_eq!(everyone_mention("@everyone brackets are up"), Some("@everyone"));
        assert_eq!(everyone_mention("hey @here and @everyone"), Some("@here"));
        assert_eq!(everyone_mention("`@everyone` is how"), None);
        assert_eq!(everyone_mention("```\n@here\n``` then"), None);
        assert_eq!(everyone_mention("``a ` @here`` b"), None);
        assert_eq!(everyone_mention("unclosed ` @here"), Some("@here"));
        assert_eq!(everyone_mention("<@&123> ping the role"), None);
    }

    #[test]
    fn hold_up_needs_permission_and_more_than_thirty() {
        let groups = [("r1", 20), ("online", 150), ("offline", 1100)];
        let everyone = HoldUp::check("@everyone go", groups, true, false).unwrap();
        assert_eq!(everyone.count, 1270);
        assert_eq!(everyone.after_mention(), ", you are about to mention 1,000+ people. Are you sure?");
        assert_eq!(HoldUp::check("@here go", groups, true, true).unwrap().after_mention(), ", you are about to mention 100+ people in this thread. Are you sure?");
        assert_eq!(HoldUp::check("@everyone go", groups, false, false), None);
        assert_eq!(HoldUp::check("@here go", [("online", 20), ("offline", 500)], true, false), None);
        assert_eq!(HoldUp::check("@everyone go", [("online", 30)], true, false), None);
        assert!(HoldUp::check("@everyone go", [("online", 31)], true, false).is_some());
        assert_eq!(HoldUp::check("hello", groups, true, false), None);
    }

    #[test]
    fn count_keeps_its_leading_digit() {
        assert_eq!(leading_digit(1234), "1,000");
        assert_eq!(leading_digit(57), "50");
        assert_eq!(leading_digit(31), "30");
        assert_eq!(leading_digit(9), "9");
        assert_eq!(leading_digit(98_765), "90,000");
    }

    #[test]
    fn utterance_reads_names_not_markup() {
        let names = |id: &str| (id == "42").then(|| "Ferris".to_owned());
        assert_eq!(utterance("Mira", "**dinner** for <@42> <:waffle:9> ||ending||", names), "Mira said dinner for Ferris waffle Spoiler");
    }
}
