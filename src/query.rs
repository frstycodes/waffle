// What we fetch over REST, cached the way Discord's stores behave: the last value shows at once (from
// memory, else from disk once the background read lands, so it survives a restart), a background fetch
// refreshes it when it's stale or known to have changed, and the fresh value replaces it in place. Once
// something has been seen, it never shows a loading state again. The tiers are `tiered::Tiered`'s.
use std::{collections::HashSet, time::Duration};

use serde::{Serialize, de::DeserializeOwned};

use crate::tiered::{Caps, Codec, Tiered};

// Values are small JSON (profiles, previews, icon URLs).
const MEMORY: usize = 2 << 20;
const DISK: u64 = 16 << 20;

pub struct Query<V> {
    tiers: Tiered<V>,
    // Known to have changed (a gateway update): kept on screen, but fetched again on next use.
    changed: HashSet<String>,
    in_flight: HashSet<String>,
}

impl<V: Clone + Serialize + DeserializeOwned + Send + 'static> Query<V> {
    // `name` is its folder under the account's cache directory.
    pub fn new(name: &'static str, fresh_for: Duration) -> Self {
        Self::from_tiers(Tiered::new(name, Caps { memory: MEMORY, disk: DISK, fresh_for }, Codec::json()))
    }

    pub(crate) fn from_tiers(tiers: Tiered<V>) -> Self {
        Self { tiers, changed: HashSet::new(), in_flight: HashSet::new() }
    }

    #[cfg(test)]
    pub(crate) fn memory_only(fresh_for: Duration) -> Self {
        Self::from_tiers(Tiered::at("query-test", Caps { memory: MEMORY, disk: 0, fresh_for }, Codec::json(), None))
    }

    // The last value we have for `key`, however old. A first miss reads disk in the background.
    pub fn cached(&mut self, key: &str) -> Option<V> {
        self.tiers.get(key)
    }

    // Demo fixtures: held in memory as fresh, never written to disk.
    pub fn seed(&mut self, key: &str, value: V) {
        self.tiers.seed(key, value);
    }

    // What is already in memory, for callers that can't borrow mutably.
    pub fn peek(&self, key: &str) -> Option<V> {
        self.tiers.peek(key)
    }

    // Disk reads that have landed, into memory; true when any did.
    pub fn drain(&mut self) -> bool {
        self.tiers.drain()
    }

    // Whether to fetch `key` now: never fetched, stale, or changed, and no fetch already running. A yes
    // counts as the fetch starting; finish it with `store` or `failed`.
    pub fn should_fetch(&mut self, key: &str) -> bool {
        if self.in_flight.contains(key) {
            return false;
        }
        self.tiers.drain();
        if !self.changed.contains(key) && self.tiers.fresh(key) {
            return false;
        }
        self.in_flight.insert(key.to_owned());
        true
    }

    pub fn store(&mut self, key: &str, value: V) {
        self.in_flight.remove(key);
        self.changed.remove(key);
        self.tiers.put(key, value);
    }

    // The fetch failed; whatever was cached stays, and the next use tries again.
    pub fn failed(&mut self, key: &str) {
        self.in_flight.remove(key);
    }

    // Marks every key `matches` picks as changed, keeping their values until the refetch lands.
    pub fn changed(&mut self, matches: impl Fn(&str) -> bool) {
        // Only values we hold matter: anything not in memory is fetched on next use anyway.
        let keys: Vec<String> = self.tiers.keys().filter(|k| matches(k)).cloned().collect();
        self.changed.extend(keys);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetches_once_then_serves_the_cache() {
        let mut q: Query<String> = Query::memory_only(Duration::from_secs(300));
        assert_eq!(q.cached("1"), None);
        assert!(q.should_fetch("1"));
        assert!(!q.should_fetch("1"), "a running fetch isn't started twice");
        q.store("1", "a".into());
        assert_eq!(q.cached("1").as_deref(), Some("a"));
        assert!(!q.should_fetch("1"), "fresh values aren't refetched");
    }

    #[test]
    fn stale_and_changed_values_show_while_they_refetch() {
        let mut q: Query<String> = Query::memory_only(Duration::ZERO);
        q.store("1", "old".into());
        assert!(q.should_fetch("1"), "stale");
        q.failed("1");
        assert_eq!(q.cached("1").as_deref(), Some("old"), "a failed fetch keeps the old value");

        let mut q: Query<String> = Query::memory_only(Duration::from_secs(300));
        q.store("1-2", "old".into());
        q.changed(|k| k.starts_with("1-"));
        assert_eq!(q.cached("1-2").as_deref(), Some("old"));
        assert!(q.should_fetch("1-2"), "changed");
    }
}
