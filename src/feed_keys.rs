// Keyboard focus in the message feed, as in Discord: click a message's empty space, then move with the
// arrows and act on it by key (`MessageAction`). Esc goes back to the composer, and so does typing.
use gpui_kit::{Context, KeyDownEvent, Keystroke, Window};

use crate::{Waffle, discord::Message, message_menu::MessageAction};

const SECONDARY: &str = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };

impl Waffle {
    pub(crate) fn focus_message(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.focused_message = Some(id);
        window.focus(&self.feed_focus, cx);
        cx.notify();
    }

    pub(crate) fn on_feed_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some((ix, message)) = self.focused() else { return };
        let keystroke = &event.keystroke;
        let combo = combo(keystroke);
        // A confirm is up: keys go to it at the root (its own key, Esc), not to the message.
        if self.dialog.is_some() {
            return;
        }
        match combo.as_str() {
            "up" | "down" => self.step_focus(ix, combo == "down", cx),
            "escape" => self.leave_feed(None, window, cx),
            _ => match MessageAction::for_combo(&combo) {
                Some(action) if action.allowed(&message, &self.message_facts(&message)) => self.run_message_action(action, &message, keystroke.modifiers.shift, window, cx),
                Some(_) => {}
                None => match typed(keystroke) {
                    Some(text) => self.leave_feed(Some(text), window, cx),
                    None => return,
                },
            },
        }
        cx.stop_propagation();
    }

    // Focusing the header field opens its popover (search_menu.rs).
    pub(crate) fn focus_search(&mut self, _: &crate::app_keys::FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.focused_message = None;
        self.search_input.update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    fn focused(&self) -> Option<(usize, Message)> {
        let id = self.focused_message.as_ref()?;
        let messages = &self.feeds.get(&self.channel.as_ref()?.id)?.messages;
        let ix = messages.iter().position(|m| &m.id == id)?;
        Some((ix, messages[ix].clone()))
    }

    fn step_focus(&mut self, ix: usize, down: bool, cx: &mut Context<Self>) {
        let Some(messages) = self.channel.as_ref().and_then(|c| self.feeds.get(&c.id)).map(|f| f.messages.clone()) else { return };
        let Some(next) = step(ix, messages.len(), down) else { return };
        self.focused_message = Some(messages[next].id.clone());
        self.list.scroll_to_reveal_item(next);
        cx.notify();
    }

    // Back to the composer, carrying the key that started typing.
    fn leave_feed(&mut self, text: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.focused_message = None;
        self.composer.update(cx, |state, cx| {
            state.focus(window, cx);
            if let Some(text) = text {
                state.insert(text, window, cx);
            }
        });
        cx.notify();
    }
}

fn step(ix: usize, len: usize, down: bool) -> Option<usize> {
    let next = if down { ix + 1 } else { ix.checked_sub(1)? };
    (next < len).then_some(next)
}

// A keystroke in `MessageAction::keys`' form. A typed symbol counts as itself, so `+` matches whether the
// layout needs Shift for it or not.
pub(crate) fn combo(keystroke: &Keystroke) -> String {
    let m = keystroke.modifiers;
    let secondary = if cfg!(target_os = "macos") { m.platform } else { m.control };
    let symbol = keystroke.key_char.as_deref().filter(|c| !secondary && !m.control && c.chars().count() == 1 && c.chars().all(|c| c.is_ascii_punctuation()));
    let (key, shift) = match symbol {
        Some("+") => ("plus", false),
        Some(c) => (c, false),
        None => (keystroke.key.as_str(), m.shift),
    };
    let mods = [(m.control && !secondary, "ctrl"), (m.alt, "alt"), (shift, "shift"), (secondary, "secondary"), (m.platform && !secondary, "cmd")];
    mods.into_iter().filter(|(on, _)| *on).map(|(_, name)| name).chain([key]).collect::<Vec<_>>().join("+")
}

// A combo as this platform writes it: ⌃⌥⇧⌘ order on macOS, Ctrl+Alt+Shift elsewhere (a literal `ctrl` reads as Ctrl too).
pub(crate) fn platform_combo(combo: &str) -> String {
    let parts: Vec<&str> = combo.split('+').collect();
    let order: &[&str] = if cfg!(target_os = "macos") { &["ctrl", "alt", "shift", "secondary"] } else { &["ctrl", "secondary", "alt", "shift"] };
    let key = parts.last().copied().unwrap_or_default();
    order.iter().copied().filter(|m| parts[..parts.len() - 1].contains(m)).chain([key]).map(|p| if p == "secondary" { SECONDARY } else { p }).collect::<Vec<_>>().join("+")
}

// What a key types into the composer: printable text without ⌘ or Ctrl.
fn typed(keystroke: &Keystroke) -> Option<String> {
    let m = keystroke.modifiers;
    if m.platform || m.control {
        return None;
    }
    keystroke.key_char.clone().filter(|c| !c.is_empty() && !c.chars().any(char::is_control))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        discord::{Message, SendState},
        message_menu::Facts,
    };

    fn key(s: &'static str) -> Keystroke {
        Keystroke::parse(s).expect("valid keystroke")
    }

    fn typed_key(s: &'static str, ch: &str) -> Keystroke {
        Keystroke { key_char: Some(ch.into()), ..key(s) }
    }

    fn action(k: Keystroke) -> Option<MessageAction> {
        MessageAction::for_combo(&combo(&k))
    }

    #[test]
    fn keys_route_to_discords_actions() {
        let sec = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
        assert_eq!(action(typed_key("e", "e")), Some(MessageAction::Edit));
        assert_eq!(action(typed_key("r", "r")), Some(MessageAction::Reply));
        assert_eq!(action(typed_key("f", "f")), Some(MessageAction::Forward));
        assert_eq!(action(typed_key("p", "p")), Some(MessageAction::Pin));
        assert_eq!(action(typed_key("t", "t")), Some(MessageAction::Thread));
        assert_eq!(action(typed_key("d", "d")), Some(MessageAction::Delete));
        assert_eq!(action(typed_key("a", "a")), Some(MessageAction::React));
        // `+` with Shift on a US board, or bare on one that has the key.
        assert_eq!(action(typed_key("shift-=", "+")), Some(MessageAction::React));
        assert_eq!(action(typed_key("+", "+")), Some(MessageAction::React));
        assert_eq!(action(typed_key("c", "c")), Some(MessageAction::CopyText));
        assert_eq!(action(key(format!("{sec}-c").leak())), Some(MessageAction::CopyText));
        assert_eq!(action(key(format!("{sec}-shift-c").leak())), Some(MessageAction::CopyId));
        assert_eq!(action(key("alt-enter")), Some(MessageAction::MarkUnread));
        for k in ["backspace", "delete", "shift-backspace", "shift-delete", "shift-d"] {
            assert_eq!(action(key(k)), Some(MessageAction::Delete), "{k}");
        }
        assert_eq!(action(typed_key("x", "x")), None);
        assert_eq!(action(key("enter")), None);
    }

    #[test]
    fn every_shown_shortcut_routes_back_to_its_action() {
        for a in MessageAction::ALL {
            assert_eq!(MessageAction::for_combo(a.keys()[0]), Some(a));
        }
    }

    #[test]
    fn caps_use_the_platforms_order() {
        if cfg!(target_os = "macos") {
            assert_eq!(platform_combo("shift+secondary+c"), "shift+cmd+c");
        } else {
            assert_eq!(platform_combo("shift+secondary+c"), "ctrl+shift+c");
        }
        assert_eq!(platform_combo("alt+enter"), "alt+enter");
        assert_eq!(platform_combo("plus"), "plus");
    }

    #[test]
    fn typing_leaves_the_feed_but_shortcuts_dont() {
        assert_eq!(typed(&typed_key("h", "h")), Some("h".into()));
        assert_eq!(typed(&typed_key("shift-h", "H")), Some("H".into()));
        assert_eq!(typed(&key("cmd-v")), None);
        assert_eq!(typed(&key("escape")), None);
    }

    #[test]
    fn focus_moves_one_message_and_stops_at_the_ends() {
        assert_eq!(step(0, 3, true), Some(1));
        assert_eq!(step(2, 3, true), None);
        assert_eq!(step(0, 3, false), None);
        assert_eq!(step(2, 3, false), Some(1));
    }

    #[test]
    fn actions_follow_the_menus_permissions() {
        let sent = Message { content: "hi".into(), state: SendState::Sent, ..Default::default() };
        let theirs_in_server = Facts::default();
        let mine = Facts { mine: true, ..Default::default() };
        let moderator = Facts { manage: true, pin: true, threads: true, ..Default::default() };
        let dm = Facts { in_dm: true, ..Default::default() };

        assert!(!MessageAction::Edit.allowed(&sent, &theirs_in_server));
        assert!(MessageAction::Edit.allowed(&sent, &mine));
        assert!(!MessageAction::Delete.allowed(&sent, &theirs_in_server));
        assert!(MessageAction::Delete.allowed(&sent, &mine));
        assert!(MessageAction::Delete.allowed(&sent, &moderator));
        assert!(!MessageAction::Pin.allowed(&sent, &theirs_in_server));
        assert!(MessageAction::Pin.allowed(&sent, &dm));
        assert!(MessageAction::Pin.allowed(&sent, &moderator));
        assert!(!MessageAction::Thread.allowed(&sent, &theirs_in_server));
        assert!(MessageAction::Thread.allowed(&sent, &moderator));
        assert!(!MessageAction::CopyId.allowed(&sent, &theirs_in_server));
        assert!(MessageAction::CopyId.allowed(&sent, &Facts { developer: true, ..Default::default() }));
        assert!(MessageAction::Reply.allowed(&sent, &theirs_in_server));
        assert!(MessageAction::Forward.allowed(&sent, &theirs_in_server));

        let empty = Message { state: SendState::Sent, ..Default::default() };
        assert!(!MessageAction::CopyText.allowed(&empty, &mine));
        let failed = Message { content: "hi".into(), state: SendState::Failed, ..Default::default() };
        assert!(MessageAction::Delete.allowed(&failed, &mine));
        assert!(!MessageAction::Reply.allowed(&failed, &mine));
        let sending = Message { state: SendState::Sending, ..Default::default() };
        assert!(MessageAction::ALL.iter().all(|a| !a.allowed(&sending, &mine)));
    }
}
