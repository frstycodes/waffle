// How often Waffle sends, after Discord's own client: read acks wait and carry the newest message,
// member-list subscriptions go out only when they change, presence updates are capped, and reconnects
// back off.
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use tokio::time::Instant;

use super::*;

// Discord's auto-read timer, and the most member lists it keeps subscribed per server.
const ACK_DELAY: Duration = Duration::from_secs(3);
const CHANNELS_PER_GUILD: usize = 5;
// Discord's presence commit: at most this many updates in this long.
const PRESENCE_LIMIT: usize = 5;
const PRESENCE_WINDOW: Duration = Duration::from_secs(20);
// Discord's gateway backoff: from 1s, doubling with jitter, up to a minute. Past this many failed
// connects in a row, the session is given up and the next connect identifies anew.
const RECONNECT_MIN: Duration = Duration::from_secs(1);
const RECONNECT_MAX: Duration = Duration::from_secs(60);
pub(super) const RESUME_FAILS: u32 = 4;

impl Client {
    // Discord's auto-read: the first new message in a channel starts its timer, later ones only move
    // the target, and one POST carries the newest. A mention that starts the timer acks at once.
    pub fn ack_soon(&self, channel_id: &str, message_id: &str, immediate: bool) {
        let first = self.acks.lock().is_ok_and(|mut acks| acks.insert(channel_id.to_owned(), message_id.to_owned()).is_none());
        if !first {
            return;
        }
        let (client, channel) = (self.clone(), channel_id.to_owned());
        runtime().spawn(async move {
            if !immediate {
                tokio::time::sleep(ACK_DELAY).await;
            }
            let Some(message) = client.acks.lock().ok().and_then(|mut acks| acks.remove(&channel)) else { return };
            client.ack(&channel, &message).await;
        });
    }

    // The open channel's member list (op 37, GUILD_SUBSCRIPTIONS_BULK), sent only when it changes.
    pub fn request_member_list(&self, guild_id: &str, channel_id: &str) {
        let Some(op) = self.subscriptions.lock().ok().and_then(|mut subs| subs.open(guild_id, channel_id)) else { return };
        self.gateway(op);
    }
}

pub(super) type SharedSubscriptions = Arc<Mutex<Subscriptions>>;

// What this gateway session is subscribed to, per server. A fresh session (READY) starts empty.
#[derive(Default)]
pub(super) struct Subscriptions(HashMap<String, Subscribed>); // lint-allow(unbounded-growth): one entry per server opened, cleared on every READY

#[derive(Default)]
pub(super) struct Subscribed {
    features: bool,
    channels: VecDeque<String>,
}

impl Subscriptions {
    pub(super) fn clear(&mut self) {
        self.0.clear();
    }

    // The op 37 for opening `channel_id`: None when it's already among the server's last five, as
    // Discord's client skips an unchanged subscription. Typing, activities and threads go once per server.
    pub(super) fn open(&mut self, guild_id: &str, channel_id: &str) -> Option<Value> {
        let sub = self.0.entry(guild_id.to_owned()).or_default();
        if let Some(at) = sub.channels.iter().position(|c| c == channel_id) {
            let channel = sub.channels.remove(at)?;
            sub.channels.push_back(channel);
            return None;
        }
        sub.channels.push_back(channel_id.to_owned());
        if sub.channels.len() > CHANNELS_PER_GUILD {
            sub.channels.pop_front();
        }
        let channels: serde_json::Map<String, Value> = sub.channels.iter().map(|c| (c.clone(), json!([[0, 99]]))).collect();
        let mut entry = json!({ "channels": channels });
        if !std::mem::replace(&mut sub.features, true) {
            for feature in ["typing", "activities", "threads"] {
                entry[feature] = true.into();
            }
        }
        Some(json!({ "op": 37, "d": { "subscriptions": { guild_id: entry } } }))
    }
}

// Presence updates (op 3) on their way out: an unchanged one isn't sent, and past five in twenty
// seconds the newest waits for the oldest to age out.
#[derive(Default)]
pub(super) struct Presence {
    expiries: VecDeque<Instant>,
    last: Option<Value>,
    pending: Option<Value>,
}

impl Presence {
    pub(super) fn offer(&mut self, update: Value, now: Instant) -> Option<Value> {
        if self.last.as_ref() == Some(&update) {
            self.pending = None;
            return None;
        }
        self.pending = Some(update);
        self.release(now)
    }

    pub(super) fn release(&mut self, now: Instant) -> Option<Value> {
        while self.expiries.front().is_some_and(|at| *at <= now) {
            self.expiries.pop_front();
        }
        if self.expiries.len() >= PRESENCE_LIMIT {
            return None;
        }
        let update = self.pending.take()?;
        self.expiries.push_back(now + PRESENCE_WINDOW);
        self.last = Some(update.clone());
        Some(update)
    }

    // When a held update may go out.
    pub(super) fn wake(&self) -> Option<Instant> {
        self.pending.as_ref()?;
        self.expiries.front().copied()
    }
}

// The wait before reconnect attempt `fails` (1 = the first since the session was last up).
pub(super) fn reconnect_wait(fails: u32, jitter: f64) -> Duration {
    RECONNECT_MIN.saturating_mul(1 << fails.saturating_sub(1).min(6)).mul_f64(1. + jitter.clamp(0., 1.)).min(RECONNECT_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscriptions_send_only_changes() {
        let mut subs = Subscriptions::default();
        let first = subs.open("g", "a").expect("first channel subscribes");
        assert_eq!(first["op"], 37);
        assert_eq!(first["d"]["subscriptions"]["g"]["typing"], true);
        assert!(subs.open("g", "a").is_none(), "unchanged");
        let second = subs.open("g", "b").expect("a new channel subscribes");
        assert!(second["d"]["subscriptions"]["g"]["typing"].is_null(), "features go once per server");
        assert_eq!(second["d"]["subscriptions"]["g"]["channels"].as_object().map(|c| c.len()), Some(2));
        for c in ["c", "d", "e", "f"] {
            subs.open("g", c);
        }
        let kept = &subs.0["g"].channels;
        assert_eq!(kept.len(), CHANNELS_PER_GUILD);
        assert!(!kept.contains(&"a".to_owned()), "the least recent drops out");
        subs.clear();
        assert!(subs.open("g", "b").is_some(), "a fresh session subscribes again");
    }

    #[test]
    fn presence_caps_five_in_twenty_seconds() {
        let mut presence = Presence::default();
        let now = Instant::now();
        let update = |n: u32| json!({ "op": 3, "d": { "n": n } });
        assert!(presence.offer(update(0), now).is_some());
        assert!(presence.offer(update(0), now).is_none(), "unchanged");
        for n in 1..5 {
            assert!(presence.offer(update(n), now).is_some());
        }
        assert!(presence.offer(update(5), now).is_none(), "sixth waits");
        assert!(presence.offer(update(6), now).is_none(), "newest replaces it");
        assert_eq!(presence.wake(), Some(now + PRESENCE_WINDOW));
        assert_eq!(presence.release(now + PRESENCE_WINDOW), Some(update(6)));
        assert_eq!(presence.wake(), None);
    }

    #[test]
    fn reconnects_back_off_from_a_second_to_a_minute() {
        assert_eq!(reconnect_wait(1, 0.), RECONNECT_MIN);
        assert_eq!(reconnect_wait(3, 0.), RECONNECT_MIN * 4);
        assert_eq!(reconnect_wait(3, 1.), RECONNECT_MIN * 8);
        assert_eq!(reconnect_wait(40, 0.), RECONNECT_MAX);
    }
}
