// The image cache every `img()` in the window loads through (avatars, emoji, attachments, embeds,
// GIF stills). gpui's default cache keeps every image ever shown, decoded, in RAM and on the GPU
// until the app quits; this one lets go of images that are no longer drawn.
//
// An image is drawn by loading it, every frame it's on screen, so each load stamps the current frame.
// What's on screen is what the cache's last render drew (a part that isn't redrawing replays it), so
// an image missing from that render and not drawn for `KEEP_FOR` is freed (RAM and GPU); the time
// keeps what was just scrolled past. `sweep_all` runs this on a timer too, so an idle window lets go of
// what it browsed earlier without having to redraw. Reloading an evicted image skips the network when
// its file is still in discord::Images' download cache.
//
// Each of the window's parts (part.rs) has a cache of its own; the app's own drawing (overlays) uses
// `cache`. `clear_all()` drops everything; the app calls it when it goes out of sight (see main.rs).
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::Arc,
    time::{Duration, Instant},
};

use gpui_kit::{App, AppContext, Entity, Global, ImageCache, ImageCacheError, ImageCacheItem, RenderImage, Resource, WeakEntity, Window};

const KEEP_FOR: Duration = Duration::from_secs(10);
// How often eviction looks through the cache.
const SWEEP_EVERY: Duration = Duration::from_secs(2);

pub struct Images {
    items: HashMap<u64, Item>,
    frame: u64,
    swept_at: Instant,
    // This cache's place in `LAST`.
    slot: usize,
}

struct Item {
    load: ImageCacheItem,
    drawn_frame: u64,
    drawn_at: Instant,
}

#[derive(Default)]
struct Shared {
    app: Option<Entity<Images>>,
    // lint-allow(unbounded-growth): one weak handle per image cache, one for each part of the window that draws apart, a handful at most
    all: Vec<WeakEntity<Images>>,
}

impl Global for Shared {}

// The app's own image cache, created on first use.
pub fn cache(cx: &mut App) -> Entity<Images> {
    if let Some(images) = cx.default_global::<Shared>().app.clone() {
        return images;
    }
    let images = new_cache(cx);
    cx.global_mut::<Shared>().app = Some(images.clone());
    images
}

// A cache of its own, for a part of the window that draws apart from the rest.
pub fn new_cache(cx: &mut App) -> Entity<Images> {
    let slot = cx.default_global::<Shared>().all.len();
    let images = cx.new(|_| Images { items: HashMap::new(), frame: 0, swept_at: Instant::now(), slot });
    cx.global_mut::<Shared>().all.push(images.downgrade());
    images
}

// Frees every cache's images.
pub fn clear_all(cx: &mut App) {
    let all: Vec<Entity<Images>> = cx.default_global::<Shared>().all.iter().filter_map(WeakEntity::upgrade).collect();
    for images in all {
        images.update(cx, |images, cx| images.clear(cx));
    }
}

// Every cache's sweep, for when the window isn't redrawing; returns whether anything was freed.
pub fn sweep_all(cx: &mut App) -> bool {
    let all: Vec<Entity<Images>> = cx.default_global::<Shared>().all.iter().filter_map(WeakEntity::upgrade).collect();
    // Counted, not `any`: every cache sweeps.
    all.into_iter().filter(|images| images.update(cx, |images, cx| images.sweep(cx))).count() > 0
}

pub struct Stats {
    pub images: usize,
    pub decoded_bytes: usize,
}

// Each cache's last sweep, for the memory report (which runs off the UI thread).
static LAST: std::sync::Mutex<Vec<Stats>> = std::sync::Mutex::new(Vec::new());

pub fn last_stats() -> Stats {
    let sum = |all: &Vec<Stats>| Stats { images: all.iter().map(|s| s.images).sum(), decoded_bytes: all.iter().map(|s| s.decoded_bytes).sum() };
    LAST.lock().map(|all| sum(&all)).unwrap_or(Stats { images: 0, decoded_bytes: 0 })
}

impl Images {
    fn publish(&self) {
        if let Ok(mut last) = LAST.lock() {
            if last.len() <= self.slot {
                last.resize_with(self.slot + 1, || Stats { images: 0, decoded_bytes: 0 });
            }
            last[self.slot] = self.stats();
        }
    }
}

impl Images {
    // Called once per window render, before children load their images.
    pub fn next_frame(&mut self, cx: &mut App) {
        // Before the frame moves on: `frame` is still the render on screen.
        if self.swept_at.elapsed() >= SWEEP_EVERY {
            self.sweep(cx);
        }
        self.frame += 1;
    }

    // Returns whether anything was freed.
    fn sweep(&mut self, cx: &mut App) -> bool {
        self.swept_at = Instant::now();
        let frame = self.frame;
        let stale: Vec<u64> = self.items.iter().filter(|(_, item)| item.drawn_frame != frame && item.drawn_at.elapsed() > KEEP_FOR).map(|(key, _)| *key).collect();
        for key in &stale {
            if let Some(item) = self.items.remove(key) {
                free(item, cx);
            }
        }
        self.publish();
        !stale.is_empty()
    }

    // Frees every image; whatever is on screen loads again (from the download cache) when next drawn.
    pub fn clear(&mut self, cx: &mut App) {
        for (_, item) in std::mem::take(&mut self.items) {
            free(item, cx);
        }
        self.publish();
    }

    // Drops a load still in flight, which stops its download. Finished images are left to eviction.
    pub fn cancel(&mut self, url: &str) {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        Resource::Uri(url.to_owned().into()).hash(&mut hasher);
        let key = hasher.finish();
        if self.items.get(&key).is_some_and(|item| item.load.get().is_none()) {
            self.items.remove(&key);
        }
    }

    pub fn stats(&self) -> Stats {
        let decoded_bytes = self
            .items
            .values()
            .filter_map(|item| item.load.get()?.ok())
            .map(|image| (0..image.frame_count()).map(|i| image.as_bytes(i).map_or(0, <[u8]>::len)).sum::<usize>())
            .sum();
        Stats { images: self.items.len(), decoded_bytes }
    }
}

fn free(item: Item, cx: &mut App) {
    if let Some(Ok(image)) = item.load.get() {
        // Deferred: gpui takes a window out of `App` while it renders or runs its callbacks, and
        // `drop_image(_, None)` only reaches windows still in `App`, so its atlas pages would never be freed.
        cx.defer(move |cx| cx.drop_image(image, None));
    }
}

impl ImageCache for Images {
    fn load(&mut self, resource: &Resource, window: &mut Window, cx: &mut App) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        resource.hash(&mut hasher);
        let frame = self.frame;
        let item = self.items.entry(hasher.finish()).or_insert_with(|| Item { load: ImageCacheItem::new(resource, cx), drawn_frame: frame, drawn_at: Instant::now() });
        item.drawn_frame = frame;
        item.drawn_at = Instant::now();
        item.load.use_image(window)
    }
}
