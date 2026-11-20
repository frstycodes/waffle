// Discord's per-channel drafts: what you typed and didn't send waits in its channel, across restarts.
use std::{collections::HashMap, path::PathBuf};

use gpui_kit::component::input::{InlineToken, InputContent};
use serde::{Deserialize, Serialize};

use crate::discord::Message;

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Draft {
    text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    chips: Vec<Chip>,
    // A slash command typed out: the command picked from the list (its id), when two apps share its name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) command: Option<String>,
    // And the choices picked for its options, whose labels alone don't say what they send.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) picks: Vec<CommandPick>,
    // GAP: the reply target stays in memory only; a restart drops it.
    #[serde(skip)]
    pub(crate) reply: Option<Message>,
}

/// A choice picked for a typed command's option: while the option's value still reads `label`, it sends `value`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct CommandPick {
    pub option: String,
    pub label: String,
    pub value: serde_json::Value,
}

// A mention chip: its byte range in the text, its wire form (`<@id>`) and the text it shows.
#[derive(Clone, Serialize, Deserialize)]
struct Chip {
    start: usize,
    end: usize,
    id: String,
    text: String,
}

impl Draft {
    pub(crate) fn new(content: &InputContent, reply: Option<Message>) -> Self {
        let chips = content.tokens().iter().map(|span| Chip { start: span.range().start, end: span.range().end, id: span.token().id().to_string(), text: span.token().text().to_string() }).collect();
        Self { text: content.text().to_string(), chips, command: None, picks: Vec::new(), reply }
    }

    pub(crate) fn with_command(self, command: Option<String>, picks: Vec<CommandPick>) -> Self {
        Self { command, picks, ..self }
    }

    // A chip that no longer fits its text is dropped, keeping its words as plain text.
    pub(crate) fn content(&self) -> InputContent {
        self.chips.iter().fold(InputContent::new(self.text.clone()), |content, chip| {
            content.clone().with_token(chip.start..chip.end, InlineToken::new(chip.id.clone(), chip.text.clone())).unwrap_or(content)
        })
    }

    fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }
}

#[derive(Default)]
pub(crate) struct Drafts {
    by_channel: HashMap<String, Draft>,
    // None keeps the drafts in memory only (demo scenes).
    path: Option<PathBuf>,
}

impl Drafts {
    pub(crate) fn load() -> Self {
        Self::at(crate::data_dir().map(|dir| dir.join("drafts.json")))
    }

    fn at(path: Option<PathBuf>) -> Self {
        let by_channel = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        Self { by_channel, path }
    }

    pub(crate) fn get(&self, channel_id: &str) -> Option<&Draft> {
        self.by_channel.get(channel_id)
    }

    pub(crate) fn has(&self, channel_id: &str) -> bool {
        self.by_channel.contains_key(channel_id)
    }

    // An empty draft clears the channel's.
    pub(crate) fn set(&mut self, channel_id: &str, draft: Draft) {
        if draft.is_empty() {
            return self.clear(channel_id);
        }
        self.by_channel.insert(channel_id.to_owned(), draft);
    }

    pub(crate) fn clear(&mut self, channel_id: &str) {
        self.by_channel.remove(channel_id);
    }

    pub(crate) fn save(&self) {
        let Some(path) = &self.path else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string(&self.by_channel) {
            let _ = std::fs::write(path, text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drafts_survive_a_restart_with_their_chips_and_clear_on_send() {
        let path = std::env::temp_dir().join(format!("waffle-drafts-{}.json", std::process::id()));
        let mut drafts = Drafts::at(Some(path.clone()));
        let typed = InputContent::new("hi @Ferris ").with_token(3..10, InlineToken::new("<@42>", "@Ferris")).unwrap();
        drafts.set("1", Draft::new(&typed, None));
        let pick = CommandPick { option: "query".into(), label: "Midnight Arcade".into(), value: serde_json::json!("track:42") };
        drafts.set("3", Draft::new(&InputContent::new("/play query:Midnight Arcade"), None).with_command(Some("10".into()), vec![pick.clone()]));
        drafts.set("2", Draft::new(&InputContent::new("  "), None));
        drafts.save();

        let mut reopened = Drafts::at(Some(path.clone()));
        assert_eq!(reopened.get("1").map(Draft::content), Some(typed), "text and chip come back");
        assert!(!reopened.has("2"), "a blank box is no draft");
        let command = reopened.get("3").unwrap();
        assert_eq!((command.command.as_deref(), command.picks.as_slice()), (Some("10"), [pick].as_slice()), "the picked command and choice come back");

        reopened.clear("1");
        reopened.save();
        assert!(!Drafts::at(Some(path.clone())).has("1"), "sending clears it on disk too");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_chip_that_no_longer_fits_becomes_text() {
        let draft = Draft { text: "hi @Fe".into(), chips: vec![Chip { start: 3, end: 10, id: "<@42>".into(), text: "@Ferris".into() }], ..Default::default() };
        let content = draft.content();
        assert_eq!(content.text().as_ref(), "hi @Fe");
        assert!(content.tokens().is_empty());
    }
}
