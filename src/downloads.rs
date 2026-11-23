// Image files as downloaded (compressed). Memory holds the most recently used within its cap: the image
// cache frees decoded images that leave the screen, and this lets them come back without the network.
// Disk keeps more, content-addressed by URL under the signed-in account, so avatars and icons survive a
// restart. CDN URLs name their content (a hash in the path), so a stored file never goes stale.
use std::{
    sync::{Mutex, OnceLock},
    time::Duration,
};

use bytes::Bytes;

use crate::tiered::{Caps, Codec, Tiered};

const MEMORY: usize = 32 << 20;
const DISK: u64 = 200 << 20;

fn files() -> &'static Mutex<Tiered<Bytes>> {
    static FILES: OnceLock<Mutex<Tiered<Bytes>>> = OnceLock::new();
    FILES.get_or_init(|| Mutex::new(Tiered::new("images", Caps { memory: MEMORY, disk: DISK, fresh_for: Duration::MAX }, Codec::bytes())))
}

// Memory only: for the UI thread.
pub fn get(url: &str) -> Option<Bytes> {
    files().lock().ok()?.get(url)
}

// Memory, else disk. Blocks on the read, so only from a thread that isn't the UI's (the image loader).
pub fn get_or_read(url: &str) -> Option<Bytes> {
    let reader = {
        let mut files = files().lock().ok()?;
        if let Some(bytes) = files.get(url) {
            return Some(bytes);
        }
        files.disk_reader()?
    };
    let read = reader.read(url)?;
    let bytes = read.0.clone();
    if let Ok(mut files) = files().lock() {
        files.insert_read(url, read);
    }
    Some(bytes)
}

pub fn put(url: String, bytes: Bytes) {
    if let Ok(mut files) = files().lock() {
        files.put(&url, bytes);
    }
}

pub fn bytes() -> usize {
    files().lock().map_or(0, |f| f.memory_bytes())
}
