// How often each app command was run from here, across restarts: the picker's Recent section.
// GAP: Discord's own frecency list isn't read or written, so runs from other clients don't count.
use std::{collections::HashMap, path::PathBuf};

use crate::app_commands::AppCommand;

#[derive(Default)]
pub(crate) struct RecentCommands {
    runs: HashMap<String, u32>,
    // None keeps the counts in memory only (demo scenes).
    path: Option<PathBuf>,
}

impl RecentCommands {
    pub(crate) fn load() -> Self {
        Self::at(crate::data_dir().map(|dir| dir.join("recent-commands.json")))
    }

    fn at(path: Option<PathBuf>) -> Self {
        let runs = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        Self { runs, path }
    }

    pub(crate) fn runs(&self) -> &HashMap<String, u32> {
        &self.runs
    }

    pub(crate) fn record(&mut self, command: &AppCommand) {
        *self.runs.entry(key(command)).or_default() += 1;
        self.save();
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string(&self.runs) {
            let _ = std::fs::write(path, text);
        }
    }
}

// App and command id; subcommands share their root's id, so the path tells them apart.
pub(crate) fn key(command: &AppCommand) -> String {
    format!("{}:{}:{}", command.application_id, command.id, command.name())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_survive_a_restart() {
        let path = std::env::temp_dir().join(format!("waffle-recent-commands-{}.json", std::process::id()));
        let play = AppCommand { id: "10".into(), application_id: "w".into(), path: vec!["play".into()], ..Default::default() };
        let mut recent = RecentCommands::at(Some(path.clone()));
        recent.record(&play);
        recent.record(&play);
        assert_eq!(RecentCommands::at(Some(path.clone())).runs().get("w:10:play"), Some(&2));
        let _ = std::fs::remove_file(path);
    }
}
