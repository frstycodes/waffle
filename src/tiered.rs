// One cache, three tiers: memory, then disk, then whoever fetches. Both tiers are capped in bytes and give up
// their least recently used entries first, never a pinned one (the open channel and its neighbours); both
// register their caps with `store`'s budget. A value has a fetch time, and is fresh for `fresh_for` after
// it: older ones still show while the caller refetches (stale-while-revalidate).
//
// Disk reads never run on the caller's thread. `get` answers from memory and, on a miss, asks the IO thread;
// what it finds lands in memory and `store::landed` tells the app to look again. `read_blocking` is the
// same read for code already off the UI thread (the image loader).
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::store::{self, Disk, Gauge};

// How a value becomes bytes on disk and back. `json` for serde types, `bytes` for files kept as they came.
pub struct Codec<V> {
    pub encode: fn(&V) -> Option<Vec<u8>>,
    pub decode: fn(&[u8]) -> Option<V>,
}

impl<V> Clone for Codec<V> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<V> Copy for Codec<V> {}

impl<V: serde::Serialize + serde::de::DeserializeOwned> Codec<V> {
    pub fn json() -> Self {
        Self { encode: |v| serde_json::to_vec(v).ok(), decode: |b| serde_json::from_slice(b).ok() }
    }
}

impl Codec<bytes::Bytes> {
    pub fn bytes() -> Self {
        Self { encode: |v| Some(v.to_vec()), decode: |b| Some(bytes::Bytes::copy_from_slice(b)) }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Caps {
    pub memory: usize,
    pub disk: u64,
    pub fresh_for: Duration,
}

struct Slot<V> {
    value: V,
    bytes: usize,
    fetched_ms: u64,
    used: u64,
}

// A value read from disk: itself, its fetch time and its weight in memory.
pub type Read<V> = (V, u64, usize);

type Inbox<V> = Arc<Mutex<HashMap<String, Read<V>>>>;

pub struct Tiered<V> {
    caps: Caps,
    codec: Codec<V>,
    gauge: Arc<Gauge>,
    disk: Option<Arc<Disk>>,
    // The account the memory tier was filled under; `follow` keeps it in step with `store::account`.
    account: Option<String>,
    follow: bool,
    slots: HashMap<String, Slot<V>>,
    bytes: usize,
    clock: u64,
    pins: Arc<Mutex<HashSet<String>>>,
    // Keys already looked for on disk this account, found or not.
    asked: HashSet<String>,
    // Disk reads that came back, waiting to move into memory.
    inbox: Inbox<V>,
}

impl<V: Clone + Send + 'static> Tiered<V> {
    // `name` is its folder under the account's cache and its line in the budget.
    pub fn new(name: &'static str, caps: Caps, codec: Codec<V>) -> Self {
        Self::at(name, caps, codec, store::accounts_root())
    }

    pub(crate) fn at(name: &'static str, caps: Caps, codec: Codec<V>, root: Option<PathBuf>) -> Self {
        let gauge = store::register(name, caps.memory, caps.disk);
        let disk = (caps.disk > 0 && root.is_some()).then(|| Arc::new(Disk::new(name, caps.disk, root, gauge.clone())));
        Self {
            caps,
            codec,
            gauge,
            disk,
            account: None,
            follow: true,
            slots: HashMap::new(),
            bytes: 0,
            clock: 0,
            pins: Arc::default(),
            asked: HashSet::new(),
            inbox: Arc::default(),
        }
    }

    // Tests: a fixed account instead of the signed-in one.
    #[cfg(test)]
    pub(crate) fn for_account(mut self, account: &str) -> Self {
        self.follow = false;
        self.account = Some(account.to_owned());
        self
    }

    // A new account starts with empty memory: one account's values never show under another.
    fn follow_account(&mut self) {
        if !self.follow {
            return;
        }
        let now = store::account();
        if now != self.account {
            self.account = now;
            self.clear_memory();
        }
    }

    pub fn clear_memory(&mut self) {
        self.slots.clear();
        self.bytes = 0;
        self.asked.clear();
        if let Ok(mut inbox) = self.inbox.lock() {
            inbox.clear();
        }
        self.gauge.set_memory(0);
    }

    // The value for `key` from memory, however old. A miss asks the disk tier in the background.
    pub fn get(&mut self, key: &str) -> Option<V> {
        self.follow_account();
        self.drain();
        self.clock += 1;
        if let Some(slot) = self.slots.get_mut(key) {
            slot.used = self.clock;
            return Some(slot.value.clone());
        }
        self.ask_disk(key);
        None
    }

    // Memory only, without touching recency (for callers that can't borrow mutably).
    pub fn peek(&self, key: &str) -> Option<V> {
        if let Some(slot) = self.slots.get(key) {
            return Some(slot.value.clone());
        }
        self.inbox.lock().ok()?.get(key).map(|(v, _, _)| v.clone())
    }

    // The keys held in memory.
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.slots.keys()
    }

    // Whether `key` is in memory and younger than `fresh_for`.
    pub fn fresh(&self, key: &str) -> bool {
        self.slots.get(key).is_some_and(|s| store::now_ms().saturating_sub(s.fetched_ms) < self.caps.fresh_for.as_millis() as u64)
    }

    // A fetched value: into memory now, onto disk in the background.
    pub fn put(&mut self, key: &str, value: V) {
        self.follow_account();
        let encoded = (self.codec.encode)(&value);
        let fetched_ms = store::now_ms();
        let bytes = encoded.as_ref().map_or(0, Vec::len) + key.len();
        self.asked.insert(key.to_owned());
        self.remember(key, value, fetched_ms, bytes);
        if let (Some(disk), Some(account), Some(encoded)) = (self.disk.clone(), self.account.clone(), encoded) {
            let (key, pins) = (key.to_owned(), self.pins.clone());
            store::io(move || {
                let mut file = fetched_ms.to_le_bytes().to_vec();
                file.extend_from_slice(&encoded);
                let pinned = pins.lock().map(|p| p.clone()).unwrap_or_default();
                disk.write(&account, &key, &file, &pinned);
            });
        }
    }

    // Held in memory as fresh and never written (demo fixtures, values the caller keeps elsewhere).
    pub fn seed(&mut self, key: &str, value: V) {
        self.follow_account();
        let bytes = (self.codec.encode)(&value).map_or(0, |e| e.len()) + key.len();
        self.asked.insert(key.to_owned());
        self.remember(key, value, store::now_ms(), bytes);
    }

    // Keys neither tier may evict; replaces the last set.
    pub fn pin(&mut self, keys: impl IntoIterator<Item = String>) {
        if let Ok(mut pins) = self.pins.lock() {
            *pins = keys.into_iter().collect();
        }
    }

    // Moves landed disk reads into memory; true when any did.
    pub fn drain(&mut self) -> bool {
        !self.drain_keys().is_empty()
    }

    // The same, naming the keys that landed.
    pub fn drain_keys(&mut self) -> Vec<String> {
        let landed: Vec<_> = self.inbox.lock().map(|mut i| i.drain().collect()).unwrap_or_default();
        let mut keys = Vec::with_capacity(landed.len());
        for (key, (value, fetched_ms, bytes)) in landed {
            // A value put since the read started is newer.
            if !self.slots.contains_key(&key) {
                self.remember(&key, value, fetched_ms, bytes);
                keys.push(key);
            }
        }
        keys
    }

    // The disk tier and account, for `read_blocking` from another thread.
    pub fn disk_reader(&mut self) -> Option<DiskReader<V>> {
        self.follow_account();
        Some(DiskReader { disk: self.disk.clone()?, account: self.account.clone()?, codec: self.codec })
    }

    // A value read off the UI thread by a `DiskReader`, into memory.
    pub fn insert_read(&mut self, key: &str, (value, fetched_ms, bytes): (V, u64, usize)) {
        self.asked.insert(key.to_owned());
        if !self.slots.contains_key(key) {
            self.remember(key, value, fetched_ms, bytes);
        }
    }

    pub fn memory_bytes(&self) -> usize {
        self.bytes
    }

    fn ask_disk(&mut self, key: &str) {
        if self.asked.contains(key) {
            return;
        }
        let Some(reader) = self.disk_reader() else { return };
        self.asked.insert(key.to_owned());
        let (inbox, key) = (self.inbox.clone(), key.to_owned());
        store::io(move || {
            let Some(read) = reader.read(&key) else { return };
            if let Ok(mut inbox) = inbox.lock() {
                inbox.insert(key, read);
            }
            store::landed();
        });
    }

    fn remember(&mut self, key: &str, value: V, fetched_ms: u64, bytes: usize) {
        self.clock += 1;
        if let Some(old) = self.slots.insert(key.to_owned(), Slot { value, bytes, fetched_ms, used: self.clock }) {
            self.bytes -= old.bytes;
        }
        self.bytes += bytes;
        let pinned = self.pins.lock().map(|p| p.clone()).unwrap_or_default();
        while self.bytes > self.caps.memory {
            let oldest = self.slots.iter().filter(|(k, _)| k.as_str() != key && !pinned.contains(*k)).min_by_key(|(_, s)| s.used).map(|(k, _)| k.clone());
            let Some(oldest) = oldest else { break };
            if let Some(slot) = self.slots.remove(&oldest) {
                self.bytes -= slot.bytes;
            }
        }
        self.gauge.set_memory(self.bytes);
    }
}

// Reads one cache's disk tier from a thread that isn't the UI's.
pub struct DiskReader<V> {
    disk: Arc<Disk>,
    account: String,
    codec: Codec<V>,
}

impl<V> DiskReader<V> {
    // The value, its fetch time and its weight in memory; a corrupt file is a miss.
    pub fn read(&self, key: &str) -> Option<(V, u64, usize)> {
        let file = self.disk.read(&self.account, key)?;
        let fetched_ms = u64::from_le_bytes(file.get(..8)?.try_into().ok()?);
        let value = (self.codec.decode)(&file[8..])?;
        Some((value, fetched_ms, file.len() - 8 + key.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(memory: usize) -> Caps {
        Caps { memory, disk: 1 << 20, fresh_for: Duration::from_secs(60) }
    }

    // Waits for the IO thread to finish what was queued before this.
    fn settle() {
        let (send, receive) = std::sync::mpsc::channel();
        store::io(move || {
            let _ = send.send(());
        });
        let _ = receive.recv_timeout(Duration::from_secs(5));
    }

    #[test]
    fn memory_evicts_by_bytes_least_recently_used_first() {
        // Each value weighs its JSON ("\"xxxx\"", 6) plus its key (1).
        let mut t: Tiered<String> = Tiered::at("tiered-test-mem", caps(3 * 7), Codec::json(), None);
        for key in ["a", "b", "c"] {
            t.put(key, "xxxx".into());
        }
        assert!(t.get("a").is_some());
        t.put("d", "xxxx".into());
        assert!(t.peek("b").is_none(), "b was least recently used");
        assert!(t.peek("a").is_some() && t.peek("c").is_some() && t.peek("d").is_some());
        assert!(t.memory_bytes() <= 3 * 7);
    }

    #[test]
    fn pinned_keys_stay_in_memory() {
        let mut t: Tiered<String> = Tiered::at("tiered-test-pin", caps(2 * 7), Codec::json(), None);
        t.pin(["a".to_owned()]);
        for key in ["a", "b", "c", "d"] {
            t.put(key, "xxxx".into());
        }
        assert!(t.peek("a").is_some(), "pinned");
        assert!(t.peek("d").is_some());
        assert!(t.peek("b").is_none() && t.peek("c").is_none());
    }

    #[test]
    fn disk_reads_land_in_the_background_per_account() {
        let root = store::temp_root("tiered");
        let mut one: Tiered<String> = Tiered::at("tiered-test-disk", caps(1 << 20), Codec::json(), Some(root.clone())).for_account("1");
        one.put("k", "kept".into());
        settle();
        let mut again: Tiered<String> = Tiered::at("tiered-test-disk", caps(1 << 20), Codec::json(), Some(root.clone())).for_account("1");
        assert_eq!(again.get("k"), None, "the first ask goes to disk, off this thread");
        settle();
        assert_eq!(again.get("k").as_deref(), Some("kept"));
        assert!(again.fresh("k"));
        let mut other: Tiered<String> = Tiered::at("tiered-test-disk", caps(1 << 20), Codec::json(), Some(root.clone())).for_account("2");
        assert_eq!(other.get("k"), None);
        settle();
        assert_eq!(other.get("k"), None, "another account never sees it");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_corrupt_file_is_a_miss() {
        let root = store::temp_root("tiered-corrupt");
        let mut t: Tiered<Vec<u32>> = Tiered::at("tiered-test-corrupt", caps(1 << 20), Codec::json(), Some(root.clone())).for_account("1");
        t.put("k", vec![1, 2]);
        settle();
        let dir = root.join("1").join("cache").join("tiered-test-corrupt");
        for file in std::fs::read_dir(&dir).unwrap().flatten() {
            std::fs::write(file.path(), b"k\n\x01\x02not json").unwrap();
        }
        let reader = Tiered::<Vec<u32>>::at("tiered-test-corrupt", caps(1 << 20), Codec::json(), Some(root.clone())).for_account("1").disk_reader().unwrap();
        assert!(reader.read("k").is_none());
        let _ = std::fs::remove_dir_all(root);
    }
}
