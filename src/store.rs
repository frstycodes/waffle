// Where Waffle keeps what it remembers between runs, and how much: every cache registers its memory and
// disk caps with one budget (`register`), so the whole footprint can be summed (`totals`); every file
// lives under the signed-in account (`accounts/<id>/`), so two accounts on one machine never share
// anything and logging out deletes it all (`forget_account`); every disk read and write runs on one IO
// thread (`io`), never the UI's, and every write lands whole or not at all (`write_atomic`).
//
// `Disk` is the disk tier the caches share: files named by a hash of their key, each carrying its key so a
// collision reads as a miss, evicted least recently used once the tier passes its byte cap, except pinned
// keys. `tiered::Tiered` puts a memory tier in front of it.
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use futures::channel::mpsc as signal;

// Application Support on macOS, %APPDATA% on Windows, $XDG_DATA_HOME (or ~/.local/share) elsewhere.
pub fn data_dir() -> Option<PathBuf> {
    let var = |name| std::env::var_os(name).map(PathBuf::from);
    let dir = if cfg!(target_os = "macos") {
        var("HOME")?.join("Library/Application Support/Waffle")
    } else if cfg!(target_os = "windows") {
        var("APPDATA")?.join("Waffle")
    } else {
        var("XDG_DATA_HOME").or_else(|| var("HOME").map(|h| h.join(".local/share")))?.join("waffle")
    };
    Some(dir)
}

// Every account's own folder sits under this.
pub fn accounts_root() -> Option<PathBuf> {
    Some(data_dir()?.join("accounts"))
}

// One cache's share of the budget: its caps and what it holds right now.
pub struct Gauge {
    pub name: &'static str,
    pub memory_cap: usize,
    pub disk_cap: u64,
    memory: AtomicUsize,
    disk: AtomicU64,
}

impl Gauge {
    pub fn memory(&self) -> usize {
        self.memory.load(Ordering::Relaxed)
    }

    pub fn disk(&self) -> u64 {
        self.disk.load(Ordering::Relaxed)
    }

    pub(crate) fn set_memory(&self, bytes: usize) {
        self.memory.store(bytes, Ordering::Relaxed);
    }

    fn set_disk(&self, bytes: u64) {
        self.disk.store(bytes, Ordering::Relaxed);
    }
}

static GAUGES: Mutex<Vec<Arc<Gauge>>> = Mutex::new(Vec::new());

// Registers a cache's caps with the budget. A name registered again (a rebuilt view) shares one gauge.
pub fn register(name: &'static str, memory_cap: usize, disk_cap: u64) -> Arc<Gauge> {
    let Ok(mut gauges) = GAUGES.lock() else {
        return Arc::new(Gauge { name, memory_cap, disk_cap, memory: AtomicUsize::new(0), disk: AtomicU64::new(0) });
    };
    if let Some(gauge) = gauges.iter().find(|g| g.name == name && g.memory_cap == memory_cap && g.disk_cap == disk_cap) {
        return gauge.clone();
    }
    let gauge = Arc::new(Gauge { name, memory_cap, disk_cap, memory: AtomicUsize::new(0), disk: AtomicU64::new(0) });
    gauges.push(gauge.clone());
    gauge
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub memory_cap: usize,
    pub disk_cap: u64,
    pub memory: usize,
    pub disk: u64,
}

// The most every cache together may hold, and what they hold now.
pub fn totals() -> Totals {
    let Ok(gauges) = GAUGES.lock() else { return Totals::default() };
    gauges.iter().fold(Totals::default(), |t, g| Totals {
        memory_cap: t.memory_cap + g.memory_cap,
        disk_cap: t.disk_cap + g.disk_cap,
        memory: t.memory + g.memory(),
        disk: t.disk + g.disk(),
    })
}

// One line for the memory report.
pub fn report() -> String {
    const MB: f64 = (1 << 20) as f64;
    let t = totals();
    format!("caches {:.1}/{:.0} MB in memory, {:.1}/{:.0} MB on disk", t.memory as f64 / MB, t.memory_cap as f64 / MB, t.disk as f64 / MB, t.disk_cap as f64 / MB)
}

static ACCOUNT: Mutex<Option<String>> = Mutex::new(None);

// Account ids are snowflakes; anything else never names a folder.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 24 && id.bytes().all(|b| b.is_ascii_digit())
}

// The account whose caches are in use; None (signed out, or not known yet) keeps every cache in memory only.
pub fn set_account(id: Option<&str>) {
    if let Ok(mut account) = ACCOUNT.lock() {
        *account = id.filter(|id| valid_id(id)).map(str::to_owned);
    }
}

pub fn account() -> Option<String> {
    ACCOUNT.lock().ok()?.clone()
}

pub fn account_dir(id: &str) -> Option<PathBuf> {
    valid_id(id).then_some(())?;
    Some(accounts_root()?.join(id))
}

// Logging out: everything kept for `id` goes, and its caches stop writing.
pub fn forget_account(id: &str) {
    if account().as_deref() == Some(id) {
        set_account(None);
    }
    let Some(dir) = account_dir(id) else { return };
    io(move || {
        let _ = std::fs::remove_dir_all(dir);
    });
}

// Caches from before they were kept per account: whose they were is unknown, so they go.
pub fn sweep_unowned() {
    let Some(dir) = data_dir().map(|d| d.join("cache")) else { return };
    io(move || {
        let _ = std::fs::remove_dir_all(dir);
    });
}

type Job = Box<dyn FnOnce() + Send>;

// Runs `job` on the IO thread, after every job queued before it, so a read sees the writes before it.
pub fn io(job: impl FnOnce() + Send + 'static) {
    static QUEUE: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();
    let queue = QUEUE.get_or_init(|| {
        let (send, receive) = mpsc::channel::<Job>();
        let spawned = std::thread::Builder::new().name("store-io".into()).spawn(move || {
            while let Ok(job) = receive.recv() {
                job();
            }
        });
        if let Err(e) = spawned {
            eprintln!("store: no IO thread: {e}");
        }
        Mutex::new(send)
    });
    if let Ok(queue) = queue.lock() {
        let _ = queue.send(Box::new(job));
    }
}

// Something read from disk has landed in a cache's memory: whoever draws it should look again.
static LANDED: Mutex<Option<signal::UnboundedSender<()>>> = Mutex::new(None);

pub fn landed() {
    if let Ok(Some(send)) = LANDED.lock().as_deref() {
        let _ = send.unbounded_send(());
    }
}

// The one listener for `landed` (the app); a second call replaces the first.
pub fn on_landed() -> signal::UnboundedReceiver<()> {
    let (send, receive) = signal::unbounded();
    if let Ok(mut slot) = LANDED.lock() {
        *slot = Some(send);
    }
    receive
}

// Writes beside the target, then renames over it: a crash mid-write leaves the old file, never half a new one.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().ok_or(std::io::ErrorKind::InvalidInput)?.to_string_lossy();
    let temp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&temp, bytes)?;
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

// A key's file name: a hash, so any key (a URL) names a file safely.
fn stem(key: &str) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(key.as_bytes()).iter().take(16).map(|b| format!("{b:02x}")).collect()
}

// A cache's files under one account, least recently used first out once they pass `cap` bytes.
pub struct Disk {
    name: &'static str,
    cap: u64,
    // Production: `accounts_root()`; tests point it at a temporary folder.
    root: Option<PathBuf>,
    gauge: Arc<Gauge>,
    state: Mutex<DiskState>,
}

#[derive(Default)]
struct DiskState {
    account: Option<String>,
    dir: Option<PathBuf>,
    // File stem → (bytes, last used).
    files: HashMap<String, (u64, u64)>,
    total: u64,
    clock: u64,
}

impl Disk {
    pub fn new(name: &'static str, cap: u64, root: Option<PathBuf>, gauge: Arc<Gauge>) -> Self {
        Self { name, cap, root, gauge, state: Mutex::new(DiskState::default()) }
    }

    // Moves to `account`'s folder, listing what's there the first time. Off the UI thread only.
    fn open<'a>(&self, state: &'a mut DiskState, account: &str) -> Option<&'a Path> {
        if state.account.as_deref() != Some(account) {
            *state = DiskState::default();
            valid_id(account).then_some(())?;
            let dir = self.root.as_ref()?.join(account).join("cache").join(self.name);
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let Ok(meta) = entry.metadata() else { continue };
                if name.ends_with(".tmp") {
                    // Left by a crash mid-write.
                    let _ = std::fs::remove_file(entry.path());
                    continue;
                }
                let used = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64);
                state.total += meta.len();
                state.files.insert(name, (meta.len(), used));
            }
            state.clock = now_ms();
            state.account = Some(account.to_owned());
            state.dir = Some(dir);
            self.gauge.set_disk(state.total);
        }
        state.dir.as_deref()
    }

    // The bytes stored under `key`, or None: missing, unreadable, or another key's file.
    pub fn read(&self, account: &str, key: &str) -> Option<Vec<u8>> {
        let name = stem(key);
        let path = {
            let mut state = self.state.lock().ok()?;
            let path = self.open(&mut state, account)?.join(&name);
            state.clock += 1;
            let clock = state.clock;
            state.files.get_mut(&name)?.1 = clock;
            path
        };
        let file = std::fs::read(&path).ok()?;
        let split = file.iter().position(|b| *b == b'\n')?;
        if &file[..split] != key.as_bytes() {
            return None;
        }
        if let Ok(f) = std::fs::File::options().write(true).open(&path) {
            let _ = f.set_modified(SystemTime::now());
        }
        Some(file[split + 1..].to_vec())
    }

    // Stores `bytes` under `key`, then evicts the least recently used unpinned files past the cap.
    pub fn write(&self, account: &str, key: &str, bytes: &[u8], pinned: &HashSet<String>) {
        let Ok(mut state) = self.state.lock() else { return };
        let Some(dir) = self.open(&mut state, account).map(Path::to_path_buf) else { return };
        let name = stem(key);
        let mut file = Vec::with_capacity(key.len() + 1 + bytes.len());
        file.extend_from_slice(key.as_bytes());
        file.push(b'\n');
        file.extend_from_slice(bytes);
        if write_atomic(&dir.join(&name), &file).is_err() {
            return;
        }
        state.clock += 1;
        let clock = state.clock;
        if let Some((old, _)) = state.files.insert(name.clone(), (file.len() as u64, clock)) {
            state.total -= old;
        }
        state.total += file.len() as u64;
        let keep: HashSet<String> = pinned.iter().map(|k| stem(k)).chain([name]).collect();
        while state.total > self.cap {
            let Some(oldest) = state.files.iter().filter(|(n, _)| !keep.contains(*n)).min_by_key(|(_, (_, used))| *used).map(|(n, _)| n.clone()) else { break };
            if let Some((size, _)) = state.files.remove(&oldest) {
                state.total -= size;
                let _ = std::fs::remove_file(dir.join(&oldest));
            }
        }
        self.gauge.set_disk(state.total);
    }

    #[cfg(test)]
    pub fn total(&self) -> u64 {
        self.state.lock().map_or(0, |s| s.total)
    }
}

#[cfg(test)]
pub(crate) fn temp_root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("waffle-store-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disk(root: &Path, cap: u64) -> Disk {
        Disk::new("t", cap, Some(root.to_path_buf()), register("store-test", 0, cap))
    }

    #[test]
    fn evicts_least_recently_used_by_bytes_sparing_pins() {
        let root = temp_root("evict");
        let d = disk(&root, 3 * 110);
        let none = HashSet::new();
        for key in ["a", "b", "c"] {
            d.write("1", key, &[0; 100], &none);
        }
        assert!(d.read("1", "a").is_some(), "a is now the most recently used");
        d.write("1", "d", &[0; 100], &none);
        assert!(d.read("1", "b").is_none(), "b was least recently used");
        assert!(d.total() <= 3 * 110);
        let pinned: HashSet<String> = ["c".to_owned()].into();
        d.write("1", "e", &[0; 100], &pinned);
        d.write("1", "f", &[0; 100], &pinned);
        assert!(d.read("1", "c").is_some(), "pinned files stay");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn accounts_never_share_files() {
        let root = temp_root("accounts");
        let d = disk(&root, 1 << 20);
        d.write("1", "k", b"one", &HashSet::new());
        assert_eq!(d.read("2", "k"), None);
        assert_eq!(d.read("1", "k").as_deref(), Some(&b"one"[..]));
        assert_eq!(d.read("../x", "k"), None, "only snowflakes name folders");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_file_for_another_key_is_a_miss() {
        let root = temp_root("collide");
        let d = disk(&root, 1 << 20);
        d.write("1", "k", b"v", &HashSet::new());
        let path = root.join("1").join("cache").join("t").join(stem("k"));
        std::fs::write(&path, b"other\nv").unwrap();
        assert_eq!(d.read("1", "k"), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_budget_sums_every_cache() {
        let before = totals();
        let g = register("store-test-sum", 10, 20);
        g.set_memory(4);
        let after = totals();
        assert_eq!(after.memory_cap - before.memory_cap, 10);
        assert_eq!(after.disk_cap - before.disk_cap, 20);
        assert!(after.memory >= 4);
    }
}
