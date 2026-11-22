// Discord's Keybinds: system-wide shortcuts that work while another app is in front, any set of keys
// held together (= + Backspace, Shift + Cmd + M). A listen-only keyboard hook (key_hook) reports the
// keys held; nothing is taken from other apps.
use crate::key_hook;
use futures::channel::mpsc;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Action {
    PushToTalk,
    PushToMute,
    ToggleMute,
    ToggleDeafen,
    ToggleVoiceMode,
    Disconnect,
    ToggleScreenShare,
    Unassigned,
}

// Toggle Screen Share's default: ⌘⇧L on macOS, Ctrl+Shift+L elsewhere.
pub const SHARE_KEYS: &str = if cfg!(target_os = "macos") { "shift+cmd+l" } else { "ctrl+shift+l" };

impl Action {
    pub const ALL: [Action; 8] = [
        Action::Unassigned,
        Action::PushToTalk,
        Action::PushToMute,
        Action::ToggleVoiceMode,
        Action::ToggleMute,
        Action::ToggleDeafen,
        Action::Disconnect,
        Action::ToggleScreenShare,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Action::PushToTalk => "Push to Talk (Normal)",
            Action::PushToMute => "Push to Mute",
            Action::ToggleMute => "Toggle Mute",
            Action::ToggleDeafen => "Toggle Deafen",
            Action::ToggleVoiceMode => "Toggle Voice Activity Mode",
            Action::Disconnect => "Disconnect From Voice",
            Action::ToggleScreenShare => "Toggle Screen Share",
            Action::Unassigned => "Unassigned",
        }
    }

    // Held actions act on press and release; the rest only on press.
    pub fn held(self) -> bool {
        matches!(self, Action::PushToTalk | Action::PushToMute)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Keybind {
    pub action: Action,
    // Key names joined by "+" in the order pressed, e.g. "shift+cmd+m" or "=+backspace"; None until
    // recorded. Files from the global-hotkey days ("alt+ArrowUp") parse too.
    pub keys: Option<String>,
}

// Matches the keys held against every keybind, and records new ones.
#[derive(Default)]
pub struct Hotkeys {
    binds: Vec<(Vec<&'static str>, Action)>,
    held: Vec<&'static str>,
    active: Vec<(Vec<&'static str>, Action)>,
    recorded: Vec<&'static str>,
    // The hook is installed; on macOS that waits for Input Monitoring permission.
    pub listening: bool,
}

impl Hotkeys {
    // The receiver yields the keys held each time they change, and an empty list once listening.
    pub fn start() -> (Self, mpsc::UnboundedReceiver<Vec<&'static str>>) {
        let (tx, rx) = mpsc::unbounded();
        key_hook::start(tx);
        (Self::default(), rx)
    }

    // Replaces every binding; returns the keybinds whose keys Waffle doesn't know.
    pub fn bind(&mut self, keybinds: &[Keybind]) -> Vec<String> {
        self.binds.clear();
        let mut unknown = Vec::new();
        for keybind in keybinds.iter().filter(|k| k.action != Action::Unassigned) {
            let Some(text) = &keybind.keys else { continue };
            match parse(text) {
                Some(keys) => self.binds.push((keys, keybind.action)),
                None => unknown.push(text.clone()),
            }
        }
        unknown
    }

    // A bind starts when a press of one of its keys completes it: all its keys held, with exactly its
    // modifiers. Other keys held don't stop it, so the bind still works mid-game with W held down. It stops
    // when one of its keys is let go, so typing other keys mid push-to-talk keeps talking.
    pub fn update(&mut self, held: Vec<&'static str>) -> Vec<(Action, bool)> {
        self.listening = true;
        let mut changes = Vec::new();
        self.active.retain(|(keys, action)| {
            let still = keys.iter().all(|k| held.contains(k));
            if !still {
                changes.push((*action, false));
            }
            still
        });
        let pressed: Vec<&str> = held.iter().copied().filter(|k| !self.held.contains(k)).collect();
        for (keys, action) in self.binds.iter().filter(|(keys, _)| pressed.iter().any(|k| keys.contains(k)) && completes(keys, &held)) {
            if !self.active.iter().any(|(k, a)| k == keys && a == action) {
                self.active.push((keys.clone(), *action));
                changes.push((*action, true));
            }
        }
        self.held = held;
        changes
    }

    // While recording: the most keys held at once, committed when all are let go. Off resets it.
    pub fn record(&mut self, on: bool) -> Option<String> {
        if !on {
            self.recorded.clear();
            return None;
        }
        if !self.held.is_empty() && self.held.len() >= self.recorded.len() {
            self.recorded = self.held.clone();
        }
        if !self.held.is_empty() || self.recorded.is_empty() {
            return None;
        }
        Some(std::mem::take(&mut self.recorded).join("+"))
    }
}

fn completes(bind: &[&str], held: &[&str]) -> bool {
    let modifier = |k: &&&str| MODIFIERS.contains(*k);
    bind.iter().all(|k| held.contains(k)) && held.iter().filter(modifier).all(|k| bind.contains(k))
}

const MODIFIERS: [&str; 4] = ["ctrl", "alt", "shift", "cmd"];

// A saved keybind as key names, or None if any part isn't a key Waffle can listen for.
pub fn parse(keys: &str) -> Option<Vec<&'static str>> {
    keys.split('+').map(key_named).collect()
}

fn key_named(part: &str) -> Option<&'static str> {
    let lower = part.to_lowercase();
    let name = lower.strip_prefix("arrow").unwrap_or(&lower);
    KEYS.iter().find(|k| k.0 == name).map(|k| k.0)
}

// The key a platform keycode stands for (macOS virtual keycode, Windows virtual-key code).
pub fn key_for_code(code: u16) -> Option<&'static str> {
    KEYS.iter().find(|k| if cfg!(target_os = "macos") { k.1 } else { k.2 } == code).map(|k| k.0)
}

// How Discord prints a keybind: upper-case keys joined by " + ".
pub fn display(keys: &str) -> String {
    let Some(parsed) = parse(keys) else { return keys.split('+').map(str::to_uppercase).collect::<Vec<_>>().join(" + ") };
    parsed
        .iter()
        .map(|&key| match key {
            // The platform key and Alt go by their own names on each system, as Discord shows them.
            "cmd" if cfg!(target_os = "macos") => "CMD".to_owned(),
            "cmd" => "WIN".to_owned(),
            "alt" if cfg!(target_os = "macos") => "OPTION".to_owned(),
            k => match k.strip_prefix("numpad") {
                Some(digit) => format!("NUMPAD {digit}"),
                None => k.to_uppercase(),
            },
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

const NONE: u16 = u16::MAX;

// (name, macOS virtual keycode, Windows virtual-key code). Names follow gpui's keystrokes; a
// modifier's left and right keys share one. macOS codes are positions on an ANSI board; Windows
// punctuation codes follow the layout, so on a non-US layout [ may read as another key.
const KEYS: &[(&str, u16, u16)] = &[
    ("ctrl", 0x3B, 0xA2),
    ("ctrl", 0x3E, 0xA3),
    ("ctrl", NONE, 0x11),
    ("alt", 0x3A, 0xA4),
    ("alt", 0x3D, 0xA5),
    ("alt", NONE, 0x12),
    ("shift", 0x38, 0xA0),
    ("shift", 0x3C, 0xA1),
    ("shift", NONE, 0x10),
    ("cmd", 0x37, 0x5B),
    ("cmd", 0x36, 0x5C),
    ("a", 0x00, 0x41),
    ("b", 0x0B, 0x42),
    ("c", 0x08, 0x43),
    ("d", 0x02, 0x44),
    ("e", 0x0E, 0x45),
    ("f", 0x03, 0x46),
    ("g", 0x05, 0x47),
    ("h", 0x04, 0x48),
    ("i", 0x22, 0x49),
    ("j", 0x26, 0x4A),
    ("k", 0x28, 0x4B),
    ("l", 0x25, 0x4C),
    ("m", 0x2E, 0x4D),
    ("n", 0x2D, 0x4E),
    ("o", 0x1F, 0x4F),
    ("p", 0x23, 0x50),
    ("q", 0x0C, 0x51),
    ("r", 0x0F, 0x52),
    ("s", 0x01, 0x53),
    ("t", 0x11, 0x54),
    ("u", 0x20, 0x55),
    ("v", 0x09, 0x56),
    ("w", 0x0D, 0x57),
    ("x", 0x07, 0x58),
    ("y", 0x10, 0x59),
    ("z", 0x06, 0x5A),
    ("1", 0x12, 0x31),
    ("2", 0x13, 0x32),
    ("3", 0x14, 0x33),
    ("4", 0x15, 0x34),
    ("5", 0x17, 0x35),
    ("6", 0x16, 0x36),
    ("7", 0x1A, 0x37),
    ("8", 0x1C, 0x38),
    ("9", 0x19, 0x39),
    ("0", 0x1D, 0x30),
    ("`", 0x32, 0xC0),
    ("-", 0x1B, 0xBD),
    ("=", 0x18, 0xBB),
    ("[", 0x21, 0xDB),
    ("]", 0x1E, 0xDD),
    ("\\", 0x2A, 0xDC),
    (";", 0x29, 0xBA),
    ("'", 0x27, 0xDE),
    (",", 0x2B, 0xBC),
    (".", 0x2F, 0xBE),
    ("/", 0x2C, 0xBF),
    ("space", 0x31, 0x20),
    ("enter", 0x24, 0x0D),
    ("tab", 0x30, 0x09),
    ("backspace", 0x33, 0x08),
    ("escape", 0x35, 0x1B),
    ("delete", 0x75, 0x2E),
    ("insert", 0x72, 0x2D),
    ("home", 0x73, 0x24),
    ("end", 0x77, 0x23),
    ("pageup", 0x74, 0x21),
    ("pagedown", 0x79, 0x22),
    ("left", 0x7B, 0x25),
    ("up", 0x7E, 0x26),
    ("right", 0x7C, 0x27),
    ("down", 0x7D, 0x28),
    ("f1", 0x7A, 0x70),
    ("f2", 0x78, 0x71),
    ("f3", 0x63, 0x72),
    ("f4", 0x76, 0x73),
    ("f5", 0x60, 0x74),
    ("f6", 0x61, 0x75),
    ("f7", 0x62, 0x76),
    ("f8", 0x64, 0x77),
    ("f9", 0x65, 0x78),
    ("f10", 0x6D, 0x79),
    ("f11", 0x67, 0x7A),
    ("f12", 0x6F, 0x7B),
    ("f13", 0x69, 0x7C),
    ("f14", 0x6B, 0x7D),
    ("f15", 0x71, 0x7E),
    ("f16", 0x6A, 0x7F),
    ("f17", 0x40, 0x80),
    ("f18", 0x4F, 0x81),
    ("f19", 0x50, 0x82),
    ("numpad0", 0x52, 0x60),
    ("numpad1", 0x53, 0x61),
    ("numpad2", 0x54, 0x62),
    ("numpad3", 0x55, 0x63),
    ("numpad4", 0x56, 0x64),
    ("numpad5", 0x57, 0x65),
    ("numpad6", 0x58, 0x66),
    ("numpad7", 0x59, 0x67),
    ("numpad8", 0x5B, 0x68),
    ("numpad9", 0x5C, 0x69),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keystrokes_round_trip() {
        assert_eq!(parse("shift+cmd+m"), Some(vec!["shift", "cmd", "m"]));
        assert_eq!(parse("alt+ArrowUp"), Some(vec!["alt", "up"]), "files from the global-hotkey days still parse");
        assert_eq!(parse("PageUp"), Some(vec!["pageup"]));
        assert_eq!(parse("=+backspace"), Some(vec!["=", "backspace"]));
        assert_eq!(parse("[+\\"), Some(vec!["[", "\\"]));
        assert_eq!(parse("shift+hyper"), None);
        assert!(parse(SHARE_KEYS).is_some());
        let (cmd, alt) = if cfg!(target_os = "macos") { ("CMD", "OPTION") } else { ("WIN", "ALT") };
        assert_eq!(display("shift+cmd+m"), format!("SHIFT + {cmd} + M"));
        assert_eq!(display("alt+ArrowUp"), format!("{alt} + UP"));
        assert_eq!(display("=+backspace"), "= + BACKSPACE");
        assert_eq!(display("[+\\"), "[ + \\");
        assert_eq!(display("numpad0"), "NUMPAD 0");
    }

    #[test]
    fn every_key_has_a_code_and_codes_are_unique() {
        for platform in [1, 2] {
            let code = |k: &(&str, u16, u16)| if platform == 1 { k.1 } else { k.2 };
            let codes: Vec<u16> = KEYS.iter().map(code).filter(|&c| c != NONE).collect();
            assert!(codes.iter().enumerate().all(|(i, c)| !codes[i + 1..].contains(c)), "a code maps to two keys");
            assert!(KEYS.iter().all(|k| KEYS.iter().any(|o| o.0 == k.0 && code(o) != NONE)), "a key with no code on one platform");
        }
        assert_eq!(key_for_code(if cfg!(target_os = "macos") { 0x18 } else { 0xBB }), Some("="));
    }

    fn bound(keys: &[&str]) -> Hotkeys {
        let mut hotkeys = Hotkeys::default();
        let binds: Vec<Keybind> = keys.iter().zip([Action::PushToTalk, Action::ToggleMute]).map(|(k, action)| Keybind { action, keys: Some((*k).into()) }).collect();
        assert!(hotkeys.bind(&binds).is_empty());
        hotkeys
    }

    #[test]
    fn chords_fire_on_press_and_stop_on_release() {
        let mut h = bound(&["=+backspace", "[+\\"]);
        assert_eq!(h.update(vec!["="]), vec![]);
        assert_eq!(h.update(vec!["=", "backspace"]), vec![(Action::PushToTalk, true)]);
        assert_eq!(h.update(vec!["=", "backspace", "a"]), vec![], "typing mid push-to-talk keeps talking");
        assert_eq!(h.update(vec!["=", "a"]), vec![(Action::PushToTalk, false)]);
        assert_eq!(h.update(vec![]), vec![]);
        assert_eq!(h.update(vec!["\\"]), vec![]);
        assert_eq!(h.update(vec!["\\", "["]), vec![(Action::ToggleMute, true)], "pressed in either order");
        assert_eq!(h.update(vec!["\\", "[", "shift"]), vec![]);
        assert_eq!(h.update(vec!["\\"]), vec![(Action::ToggleMute, false)]);
        assert_eq!(h.update(vec!["\\", "["]), vec![(Action::ToggleMute, true)]);
    }

    #[test]
    fn modifier_binds_match_exactly() {
        let mut h = bound(&["`", "shift+cmd+m"]);
        assert_eq!(h.update(vec!["cmd", "`"]), vec![], "a bare key held with a modifier is something else");
        h.update(vec![]);
        assert_eq!(h.update(vec!["`"]), vec![(Action::PushToTalk, true)]);
        assert_eq!(h.update(vec![]), vec![(Action::PushToTalk, false)]);
        h.update(vec!["shift", "cmd"]);
        assert_eq!(h.update(vec!["shift", "cmd", "m"]), vec![(Action::ToggleMute, true)]);
        assert_eq!(h.update(vec!["shift", "cmd"]), vec![(Action::ToggleMute, false)]);
        assert_eq!(h.update(vec!["shift", "cmd"]), vec![], "a release never fires");
    }

    #[test]
    fn other_keys_held_dont_block_a_bind() {
        let mut h = bound(&["`", "shift+cmd+m"]);
        h.update(vec!["w"]);
        assert_eq!(h.update(vec!["w", "`"]), vec![(Action::PushToTalk, true)], "push to talk while moving");
        h.update(vec!["w"]);
        h.update(vec!["w", "shift", "cmd"]);
        assert_eq!(h.update(vec!["w", "shift", "cmd", "m"]), vec![(Action::ToggleMute, true)]);
        h.update(vec![]);
        h.update(vec!["shift", "m"]);
        assert_eq!(h.update(vec!["m"]), vec![], "letting go of a modifier isn't a press");
        assert_eq!(h.update(vec!["m", "w"]), vec![], "a press of another key doesn't complete a bind");
        h.update(vec![]);
        assert_eq!(h.update(vec!["alt", "shift", "cmd", "m"]), vec![], "an extra modifier is another shortcut");
    }

    #[test]
    fn rebinding_mid_press_still_releases() {
        let mut h = bound(&["`"]);
        h.update(vec!["`"]);
        h.bind(&[]);
        assert_eq!(h.update(vec![]), vec![(Action::PushToTalk, false)]);
    }

    #[test]
    fn records_the_whole_chord_once_released() {
        let mut h = Hotkeys::default();
        for (held, expect) in [(vec!["="], None), (vec!["=", "backspace"], None), (vec!["backspace"], None), (vec![], Some("=+backspace"))] {
            h.update(held);
            assert_eq!(h.record(true).as_deref(), expect);
        }
        h.update(vec!["shift"]);
        h.record(true);
        h.update(vec![]);
        assert_eq!(h.record(false), None, "stopping drops what was half recorded");
        h.update(vec!["`"]);
        h.record(true);
        h.update(vec![]);
        assert_eq!(h.record(true).as_deref(), Some("`"));
    }
}
