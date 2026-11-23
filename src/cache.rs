// Discord's message cache: the newest messages of each channel kept on disk under the account, so reopening
// a channel after a restart shows its messages at once while the fresh copy loads. The open channel and its
// neighbours are pinned, so a burst of other channels never evicts them.
use std::time::Duration;

use crate::{
    discord::{Message, SendState},
    tiered::{Caps, Codec, Tiered},
};

// Matches one history page, so a cached feed looks exactly like a fetched one.
const KEEP: usize = 50;

pub type Messages = Tiered<Vec<Message>>;

pub fn tiers() -> Messages {
    // Memory holds only what was just read or written: the feeds themselves live in the app's state.
    Tiered::new("messages", Caps { memory: 4 << 20, disk: 64 << 20, fresh_for: Duration::ZERO }, Codec::json())
}

// Channel ids are snowflakes; anything else is never cached.
fn valid(channel_id: &str) -> bool {
    crate::store::valid_id(channel_id)
}

// What may be kept: messages Discord has accepted, the newest page of them, and no call still ringing, which
// would claim a call is going on now.
pub fn keepable(messages: &[Message]) -> Vec<Message> {
    let sent: Vec<&Message> = messages.iter().filter(|m| m.state == SendState::Sent && m.call.as_ref().is_none_or(|c| c.ended.is_some())).collect();
    sent[sent.len().saturating_sub(KEEP)..].iter().map(|m| (*m).clone()).collect()
}

// Writes in the background.
pub fn save(tiers: &mut Messages, channel_id: &str, messages: &[Message]) {
    if valid(channel_id) {
        tiers.put(channel_id, keepable(messages));
    }
}

// The channel's cached messages if memory has them; a miss starts a disk read that lands later.
pub fn load(tiers: &mut Messages, channel_id: &str) -> Option<Vec<Message>> {
    valid(channel_id).then(|| tiers.get(channel_id))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discord::CallRecord;

    #[test]
    fn only_snowflakes_are_cached() {
        let mut t: Messages = Tiered::at("messages-test", Caps { memory: 1 << 20, disk: 0, fresh_for: Duration::ZERO }, Codec::json(), None);
        save(&mut t, "../etc/passwd", &[]);
        assert!(t.peek("../etc/passwd").is_none());
        save(&mut t, "123", &[Message { id: "1".into(), ..Default::default() }]);
        assert_eq!(load(&mut t, "123").map(|m| m.len()), Some(1));
    }

    #[test]
    fn unsent_messages_and_live_calls_are_never_kept() {
        let live = Message { id: "2".into(), call: Some(CallRecord { participants: vec![], ended: None }), ..Default::default() };
        let ended = Message { id: "3".into(), call: Some(CallRecord { participants: vec![], ended: Some("then".into()) }), ..Default::default() };
        let sending = Message { id: "4".into(), state: SendState::Sending, ..Default::default() };
        let kept = keepable(&[Message { id: "1".into(), ..Default::default() }, live, ended, sending]);
        assert_eq!(kept.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["1", "3"]);
    }
}
