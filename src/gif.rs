// Discord's GIFs (Tenor, Klipy, Giphy) arrive as silent looping MP4s, which Discord plays inline.
//
// Drawing: gpui redraws a view together with every view above it, so a GIF animating inside the app
// would rebuild the whole window for every frame. Instead the app draws a placeholder (`tile`): the
// still, plus a note of where it landed and how it's clipped. The moving frames are painted by
// `GifLayer`, a view beside the app (see shell.rs) whose redraws leave the app's cached drawing to
// be replayed. Menus, dialogs and the like are part of the app and would sit under the layer, so
// while one is open inline GIFs pause and show their still; the media viewer's own GIF keeps going.
//
// Playing: each GIF on screen gets a player whose decoder thread runs a few pictures ahead of the
// clock and loops the clip (short clips are decoded once and replayed). Players stop and free their
// pictures when their GIF scrolls away, is covered by another window or an overlay, or the window
// loses focus. The downloaded clips (small, compressed) sit in a shared cache with a size cap.
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    ops::ControlFlow,
    rc::Rc,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

use gpui_kit::{
    AnyElement, App, Bounds, Context, Corners, ObjectFit, Pixels, Render, RenderImage, WeakEntity, Window, canvas, div, img, point, prelude::*,
    px, size,
};

use crate::h264::{Track, bgra};

// Wider clips are scaled down as they're decoded. Discord's GIF clips are usually 498 px and drawn at
// up to 400 px, which the GPU scales for free, so they're left alone: scaling on the CPU costs more
// than the decode itself.
const MAX_WIDTH: u32 = 640;
// Pictures decoded ahead of the clock: enough to ride out a slow one.
const AHEAD: usize = 4;
const TICK: Duration = Duration::from_millis(15);
// Downloaded clips kept for scrolling back (compressed MP4s, typically 0.2–3 MB each).
const CLIP_CACHE_BYTES: usize = 48 << 20;
// A clip whose pictures fit in this much is decoded once and replayed from memory while it's on
// screen (most GIFs: a few seconds at ~500 px); longer ones are decoded again each loop.
const REPLAY_BYTES: usize = 12 << 20;
// The fastest a GIF animates; most GIFs are 10-15 fps, and each frame still costs a redraw.
const MIN_FRAME: Duration = Duration::from_millis(66);
// A broken clip that reports no length would otherwise loop as fast as it decodes.
const MIN_LOOP: Duration = Duration::from_millis(100);

// Where the app's last layout put each GIF. Rebuilt whenever the part holding them renders; kept while
// gpui replays its cached drawing, which means the layout didn't change.
#[derive(Clone)]
struct Slot {
    url: String,
    bounds: Bounds<Pixels>,
    // What gpui clips the slot to where it's painted (the scrolling chat, the viewer).
    mask: Bounds<Pixels>,
    corners: Corners<Pixels>,
    // Drawn above the overlays (the media viewer), so it plays while one is open; clipped to its own
    // frame rather than the chat.
    top: bool,
    // Off holds the still: GIF autoplay is off and the GIF isn't hovered.
    moving: bool,
}

#[derive(Default)]
struct Layout {
    slots: HashMap<String, Slot>,
    // A menu, dialog, popover or settings is open over the app.
    covered: bool,
    // The visible message area, which inline GIFs are clipped to.
    chat: Option<Bounds<Pixels>>,
    // What floats over the message area (a message's hover toolbar): inline GIFs are clipped around it.
    above: Vec<Bounds<Pixels>>,
}

impl Layout {
    // What a slot is clipped to, in window coordinates: its own clip, and inline ones the message area.
    fn clip(&self, slot: &Slot) -> Bounds<Pixels> {
        match (slot.top, self.chat) {
            (false, Some(chat)) => slot.mask.intersect(&chat),
            _ => slot.mask,
        }
    }
}

// The decode thread's last time per picture (decode, convert, scale), for the debug log.
static DECODE_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

// Times the app rendered and the GIF layer drew, for the memory report (see `draws_since_last`).
static APP_RENDERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static LAYER_DRAWS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// (app renders, GIF layer draws) since the last call.
pub fn draws_since_last() -> (usize, usize) {
    use std::sync::atomic::Ordering::Relaxed;
    (APP_RENDERS.swap(0, Relaxed), LAYER_DRAWS.swap(0, Relaxed))
}

// WAFFLE_GIF_FORCE=1 (debugging): GIFs play and the window draws even when unfocused or covered.
pub fn forced() -> bool {
    std::env::var_os("WAFFLE_GIF_FORCE").is_some()
}

fn debug() -> bool {
    std::env::var_os("WAFFLE_GIF_DEBUG").is_some()
}

thread_local! {
    static DEBUG_AT: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
    // Frames the layer painted last time it drew, for the debug log.
    static PAINTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };

    static LAYOUT: RefCell<Layout> = RefCell::new(Layout::default());
    static LAYER: RefCell<Option<WeakEntity<GifLayer>>> = const { RefCell::new(None) };
}

/// The app is about to lay itself out again: forget where its overlays' GIFs were.
pub fn begin_layout() {
    APP_RENDERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    LAYOUT.with(|l| l.borrow_mut().slots.retain(|_, slot| !slot.top));
}

/// The main column (part.rs) is about to lay itself out again: forget where its GIFs were. It draws
/// apart from the app, so each forgets only its own; a replayed drawing keeps its slots.
pub fn begin_inline_layout() {
    LAYOUT.with(|l| {
        let mut layout = l.borrow_mut();
        layout.slots.retain(|_, slot| slot.top);
        layout.chat = None;
        layout.above.clear();
    });
}

/// Marks its parent as floating over the message area, so inline GIFs never paint over it. The layer
/// paints above the app, so without this a GIF under a hover toolbar would cover the toolbar.
pub fn above_gifs() -> impl IntoElement {
    canvas(|bounds, _, _| LAYOUT.with(|l| l.borrow_mut().above.push(bounds)), |_, _, _, _| {}).absolute().inset_0()
}

/// Where the message area is on screen this layout.
pub fn set_chat_clip(bounds: Bounds<Pixels>) {
    LAYOUT.with(|l| l.borrow_mut().chat = Some(bounds));
}

/// Whether an overlay is open over the app this layout.
pub fn set_covered(covered: bool) {
    LAYOUT.with(|l| l.borrow_mut().covered = covered);
}

// The GIF for `url` where the app shows it: the still (`poster`), and a slot the layer plays the clip
// over while `moving`. `key` must be unique to where it's shown. gpui can't clip pictures to a rounded
// parent, so the still and the layer's frames are both rounded to `corners`.
pub fn rounded_tile(key: String, url: &str, poster: &str, cell: gpui_kit::Size<Pixels>, corners: Corners<Pixels>, moving: bool, _cx: &mut App) -> AnyElement {
    placeholder(key, url, poster, cell, corners, false, moving)
}

// A tile drawn above every overlay (the media viewer): it keeps playing while one is open, and is
// clipped to its own frame rather than the chat.
pub fn top_tile(key: String, url: &str, poster: &str, cell: gpui_kit::Size<Pixels>, corners: Corners<Pixels>, _cx: &mut App) -> AnyElement {
    placeholder(key, url, poster, cell, corners, true, true)
}

// `cell` is the size the caller laid the tile out at; a slot never paints past it.
fn placeholder(key: String, url: &str, poster: &str, cell: gpui_kit::Size<Pixels>, corners: Corners<Pixels>, top: bool, moving: bool) -> AnyElement {
    let url = url.to_owned();
    div()
        .relative()
        .size_full()
        .child(crate::ui::edge(img(poster.to_owned()).size_full().object_fit(ObjectFit::Cover), corners))
        .child(
            // Pinned to the top left: an absolute element without offsets sits where it would flow,
            // which here is below the still. Recorded while painting, where positions are final.
            canvas(
                |_, _, _| {},
                move |bounds, _, window, cx| {
                    // GAP: in a gallery the measured bounds have come out taller than the cell (the layer
                    // then painted over the next message), cause not found; the cell's size bounds it.
                    let bounds = Bounds { origin: bounds.origin, size: bounds.size.min(&cell) };
                    let slot = Slot { url: url.clone(), bounds, mask: window.content_mask().bounds.intersect(&bounds), corners, top, moving };
                    LAYOUT.with(|l| l.borrow_mut().slots.insert(key.clone(), slot));
                    wake(cx);
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
        .into_any_element()
}

// Starts the layer's ticker if it's idle (a new slot may want playing).
fn wake(cx: &mut App) {
    let Some(layer) = LAYER.with(|l| l.borrow().as_ref().and_then(WeakEntity::upgrade)) else { return };
    cx.defer(move |cx| layer.update(cx, |layer, cx| layer.ensure_ticking(cx)));
}

pub struct GifLayer {
    // Shared with the paint closure, which runs after the app has placed this frame's slots.
    players: Rc<RefCell<HashMap<String, Player>>>,
    ticking: bool,
}

struct Player {
    url: String,
    state: State,
    frame: Option<Arc<RenderImage>>,
}

enum State {
    Loading,
    Playing(Run),
    // Can't be played: the still stays.
    Failed,
}

struct Run {
    pictures: mpsc::Receiver<Picture>,
    next: Option<Picture>,
    started: Instant,
    shown_at: Option<Instant>,
    // Pictures of a replayed clip: they stay on the GPU while it plays, so later loops don't upload
    // them again, and are freed when it stops.
    kept: Vec<Arc<RenderImage>>,
}

struct Picture {
    at: Duration,
    image: Arc<RenderImage>,
    // Part of a replayed clip (see `Run::kept`).
    kept: bool,
}

impl GifLayer {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let layer = cx.entity().downgrade();
        LAYER.with(|l| *l.borrow_mut() = Some(layer));
        Self { players: Rc::default(), ticking: false }
    }

    fn ensure_ticking(&mut self, cx: &mut Context<Self>) {
        if self.ticking {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let Ok(true) = this.update(cx, |this, cx| this.tick(cx)) else { break };
            }
        })
        .detach();
    }

    // Starts players for the slots that should move, stops the rest, and advances the playing ones.
    // Returns whether to keep ticking.
    fn tick(&mut self, cx: &mut Context<Self>) -> bool {
        let wanted: HashMap<String, String> = LAYOUT.with(|l| {
            let layout = l.borrow();
            layout.slots.iter().filter(|(_, slot)| plays(slot, &layout)).map(|(key, slot)| (key.clone(), slot.url.clone())).collect()
        });
        let mut players = self.players.borrow_mut();
        let gone: Vec<String> = players.keys().filter(|key| wanted.get(*key).is_none_or(|url| *url != players[*key].url)).cloned().collect();
        let mut changed = !gone.is_empty();
        for key in gone {
            if let Some(player) = players.remove(&key) {
                player.stop(cx);
            }
        }
        for (key, url) in wanted {
            players.entry(key.clone()).or_insert_with(|| start(key, url, cx));
        }
        for player in players.values_mut() {
            changed |= player.advance(cx);
        }
        let keep = !players.is_empty();
        if debug() && DEBUG_AT.with(|at| at.get().is_none_or(|t: Instant| t.elapsed() > Duration::from_secs(1))) {
            DEBUG_AT.with(|at| at.set(Some(Instant::now())));
            let (slots, covered) = LAYOUT.with(|l| (l.borrow().slots.len(), l.borrow().covered));
            LAYOUT.with(|l| {
                let layout = l.borrow();
                for slot in layout.slots.values() {
                    let (b, c) = (slot.bounds, layout.clip(slot));
                    eprintln!(
                        "gif slot: y {:.0}..{:.0}, clip y {:.0}..{:.0}, chat {:?}",
                        f32::from(b.top()), f32::from(b.bottom()), f32::from(c.top()), f32::from(c.bottom()),
                        layout.chat.map(|ch| (f32::from(ch.top()).round(), f32::from(ch.bottom()).round()))
                    );
                }
            });
            let states: Vec<&str> = players.values().map(|p| match (&p.state, p.frame.is_some()) {
                (State::Loading, _) => "loading",
                (State::Playing(run), true) if !run.kept.is_empty() => "playing (replay)",
                (State::Playing(_), true) => "playing (decoding)",
                (State::Playing(_), false) => "starting",
                (State::Failed, _) => "failed",
            }).collect();
            eprintln!(
                "gif: {slots} slots, covered {covered}, focused {}, players {states:?}, painted {}, decode {:.1} ms/frame",
                crate::presence::may_animate(),
                PAINTED.with(std::cell::Cell::get),
                DECODE_US.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1000.
            );
        }
        drop(players);
        if changed {
            cx.notify();
        }
        self.ticking = keep;
        keep
    }
}

// Whether a slot's GIF should be moving: allowed to, its window focused, the slot on screen, uncovered
// by other windows and, unless it sits above them, by the app's overlays.
fn plays(slot: &Slot, layout: &Layout) -> bool {
    if !slot.moving {
        return false;
    }
    let visible = slot.bounds.intersect(&layout.clip(slot));
    if forced() {
        return visible.size.width > px(0.) && visible.size.height > px(0.);
    }
    crate::presence::may_animate()
        && (!layout.covered || slot.top)
        && visible.size.width > px(0.)
        && visible.size.height > px(0.)
        && crate::presence::is_visible(visible)
}

fn start(key: String, url: String, cx: &mut Context<GifLayer>) -> Player {
    if let Some(clip) = cached_clip(&url) {
        return Player::playing(url, clip);
    }
    let fetch_url = url.clone();
    cx.spawn(async move |this, cx| {
        let fetch = crate::discord::runtime().spawn(async move {
            let res = reqwest::get(&fetch_url).await.ok()?;
            res.status().is_success().then_some(())?;
            Some((fetch_url, res.bytes().await.ok()?))
        });
        let fetched = fetch.await.ok().flatten();
        this.update(cx, |this, cx| {
            let mut players = this.players.borrow_mut();
            let Some(player) = players.get_mut(&key).filter(|p| matches!(p.state, State::Loading)) else { return };
            match fetched {
                Some((url, bytes)) => {
                    let clip: Arc<[u8]> = Arc::from(bytes.as_ref());
                    cache_clip(url.clone(), clip.clone());
                    *player = Player::playing(url, clip);
                }
                None => player.state = State::Failed,
            }
            drop(players);
            this.ensure_ticking(cx);
        })
        .ok();
    })
    .detach();
    Player { url, state: State::Loading, frame: None }
}

impl Player {
    fn playing(url: String, clip: Arc<[u8]>) -> Self {
        PLAYING.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let run = Run { pictures: decode_looping(clip), next: None, started: Instant::now(), shown_at: None, kept: Vec::new() };
        Self { url, state: State::Playing(run), frame: None }
    }

    // Shows the picture that's due, if any; returns whether the frame changed.
    fn advance(&mut self, cx: &mut App) -> bool {
        let State::Playing(run) = &mut self.state else { return false };
        if run.shown_at.is_some_and(|at| at.elapsed() < MIN_FRAME) {
            return false;
        }
        let now = run.started.elapsed();
        let mut ended = false;
        let mut due: Option<Picture> = None;
        loop {
            let picture = match run.next.take() {
                Some(picture) => picture,
                None => match run.pictures.try_recv() {
                    Ok(picture) => picture,
                    Err(error) => {
                        ended = error == mpsc::TryRecvError::Disconnected;
                        break;
                    }
                },
            };
            if picture.at > now {
                run.next = Some(picture);
                break;
            }
            if picture.kept && !run.kept.iter().any(|k| Arc::ptr_eq(k, &picture.image)) {
                run.kept.push(picture.image.clone());
            } else if !picture.kept && !run.kept.is_empty() {
                // The clip turned out too long to replay: it streams, so nothing stays kept.
                for image in run.kept.drain(..) {
                    if !self.frame.as_ref().is_some_and(|f| Arc::ptr_eq(f, &image)) {
                        cx.drop_image(image, None);
                    }
                }
            }
            // A picture the clock has already passed never reached the GPU; dropping it is enough.
            due = Some(picture);
        }
        if ended && run.next.is_none() && due.is_none() {
            // The decoder gave up (a clip it can't read): keep the last picture, or the still.
            PLAYING.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            self.state = State::Failed;
            return false;
        }
        let Some(picture) = due else { return false };
        run.shown_at = Some(Instant::now());
        if let Some(old) = self.frame.replace(picture.image)
            && !run.kept.iter().any(|k| Arc::ptr_eq(k, &old))
        {
            // Each picture is its own GPU texture; the old one must be freed or the atlas grows for good.
            cx.drop_image(old, None);
        }
        true
    }

    // Drops the decoder (its thread ends at its next send) and frees every picture.
    fn stop(self, cx: &mut App) {
        if let State::Playing(run) = self.state {
            PLAYING.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            drop(run.pictures);
            if let Some(next) = run.next {
                cx.drop_image(next.image, None);
            }
            for image in run.kept {
                cx.drop_image(image, None);
            }
        }
        if let Some(frame) = self.frame {
            // Harmless if it was a kept picture already freed above.
            cx.drop_image(frame, None);
        }
    }
}

impl Render for GifLayer {
    // Paints the current frame of every playing GIF over its slot, clipped like the slot. Paint runs
    // after the app has placed this frame's slots, so positions are always current.
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        crate::perf::probe("GifLayer");
        let players = self.players.clone();
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let players = players.borrow();
                LAYOUT.with(|l| {
                    let layout = l.borrow();
                    let mut painted = 0;
                    for (key, slot) in &layout.slots {
                        let Some(frame) = players.get(key).and_then(|p| p.frame.clone()) else { continue };
                        if !plays(slot, &layout) {
                            continue;
                        }
                        let image = cover(slot.bounds, frame.size(0));
                        let holes: &[Bounds<Pixels>] = if slot.top { &[] } else { &layout.above };
                        for piece in subtract(layout.clip(slot), holes) {
                            window.with_content_mask(Some(gpui_kit::ContentMask { bounds: piece }), |window| {
                                let _ = window.paint_image(slot.bounds, image, slot.corners, frame.clone(), 0, false);
                            });
                        }
                        painted += 1;
                    }
                    PAINTED.with(|p| p.set(painted));
                    LAYER_DRAWS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                });
            },
        )
        // Over the whole window: pinned, since it follows the app in the shell's layout.
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }
}

// `area` less every rectangle in `holes`, as up to four bands per hole (above, below, left, right).
fn subtract(area: Bounds<Pixels>, holes: &[Bounds<Pixels>]) -> Vec<Bounds<Pixels>> {
    let mut pieces = vec![area];
    for hole in holes {
        pieces = pieces
            .into_iter()
            .flat_map(|r| {
                if !r.intersects(hole) {
                    return vec![r];
                }
                let (top, bottom) = (hole.top().max(r.top()), hole.bottom().min(r.bottom()));
                [
                    Bounds::from_corners(r.origin, point(r.right(), top)),
                    Bounds::from_corners(point(r.left(), bottom), point(r.right(), r.bottom())),
                    Bounds::from_corners(point(r.left(), top), point(hole.left().max(r.left()), bottom)),
                    Bounds::from_corners(point(hole.right().min(r.right()), top), point(r.right(), bottom)),
                ]
                .into_iter()
                .filter(|b| b.size.width > px(0.) && b.size.height > px(0.))
                .collect()
            })
            .collect();
    }
    pieces
}

// The image's bounds when it covers `bounds` (centred, cropped), as object-fit: cover draws it.
fn cover(bounds: Bounds<Pixels>, image: gpui_kit::Size<gpui_kit::DevicePixels>) -> Bounds<Pixels> {
    let (iw, ih) = (image.width.0.max(1) as f32, image.height.0.max(1) as f32);
    let (bw, bh) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let scale = (bw / iw).max(bh / ih);
    let (w, h) = (iw * scale, ih * scale);
    Bounds { origin: point(bounds.origin.x + px((bw - w) / 2.), bounds.origin.y + px((bh - h) / 2.)), size: size(px(w), px(h)) }
}

// Plays the clip in a loop on its own thread, each loop's times following on from the last, staying
// `AHEAD` pictures in front. The first loop is decoded; if its pictures fit in `REPLAY_BYTES` they're
// kept and replayed, so a short GIF costs no CPU after one loop. Ends when the receiver is dropped
// (taking any kept pictures with it) or the clip can't be decoded.
// A clip's pictures with their times, kept to play again (see `REPLAY_BYTES`).
type Replay = Vec<(Duration, Arc<RenderImage>)>;

fn decode_looping(clip: Arc<[u8]>) -> mpsc::Receiver<Picture> {
    let (send, receive) = mpsc::sync_channel(AHEAD);
    let decode = move || {
        let mut offset = Duration::ZERO;
        let mut replay: Option<(Duration, Replay)> = None;
        // Only the first loop can turn out short enough to replay; after that every loop streams.
        let mut first_loop = true;
        loop {
            if let Some((length, pictures)) = &replay {
                for (at, image) in pictures {
                    if send.send(Picture { at: offset + *at, image: image.clone(), kept: true }).is_err() {
                        return;
                    }
                }
                offset += *length;
                continue;
            }
            let Ok(mut track) = Track::open(clip.clone()) else { return };
            let length = track.duration().max(MIN_LOOP);
            let (mut shown, mut receiver_gone) = (0usize, false);
            let (mut kept, mut kept_bytes) = (first_loop.then(Vec::new), 0usize);
            first_loop = false;
            let mut since = Instant::now();
            let mut last_sent: Option<Duration> = None;
            let _ = track.decode(0, |at, yuv| {
                // Every picture has to be decoded (each builds on the last), but one shown sooner
                // than `MIN_FRAME` after the previous would be skipped by the player anyway: don't
                // pay for converting it.
                if last_sent.is_some_and(|last| at < last + MIN_FRAME) {
                    return ControlFlow::Continue(());
                }
                last_sent = Some(at);
                let Some(picture) = bgra(yuv, MAX_WIDTH) else { return ControlFlow::Continue(()) };
                DECODE_US.store(since.elapsed().as_micros() as u64, std::sync::atomic::Ordering::Relaxed);
                kept_bytes += picture.as_raw().len();
                let image = Arc::new(RenderImage::new([image::Frame::new(picture)]));
                if kept_bytes > REPLAY_BYTES {
                    kept = None;
                }
                if let Some(kept) = &mut kept {
                    kept.push((at, image.clone()));
                }
                if send.send(Picture { at: offset + at, image, kept: kept.is_some() }).is_err() {
                    receiver_gone = true;
                    return ControlFlow::Break(());
                }
                since = Instant::now();
                shown += 1;
                ControlFlow::Continue(())
            });
            if receiver_gone || shown == 0 {
                return;
            }
            replay = kept.map(|pictures| (length, pictures));
            offset += length;
        }
    };
    let _ = std::thread::Builder::new().name("gif-decode".into()).spawn(decode);
    receive
}

// GIFs decoding right now (each holds up to `AHEAD` + 2 pictures), for the memory report.
static PLAYING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub struct Stats {
    pub playing: usize,
    pub clip_cache_bytes: usize,
}

pub fn stats() -> Stats {
    let clip_cache_bytes = CLIPS.lock().map(|c| c.iter().map(|(_, clip)| clip.len()).sum()).unwrap_or(0);
    Stats { playing: PLAYING.load(std::sync::atomic::Ordering::Relaxed), clip_cache_bytes }
}

// Downloaded clips, least recently used first, within `CLIP_CACHE_BYTES`.
static CLIPS: Mutex<VecDeque<(String, Arc<[u8]>)>> = Mutex::new(VecDeque::new());

fn cached_clip(url: &str) -> Option<Arc<[u8]>> {
    let mut clips = CLIPS.lock().ok()?;
    let index = clips.iter().position(|(u, _)| u == url)?;
    let entry = clips.remove(index)?;
    let clip = entry.1.clone();
    clips.push_back(entry);
    Some(clip)
}

fn cache_clip(url: String, clip: Arc<[u8]>) {
    let Ok(mut clips) = CLIPS.lock() else { return };
    clips.retain(|(u, _)| *u != url);
    clips.push_back((url, clip));
    while clips.iter().map(|(_, c)| c.len()).sum::<usize>() > CLIP_CACHE_BYTES && clips.len() > 1 {
        clips.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Collects the first `count` pictures the looping decoder produces.
    fn first(clip: &'static [u8], count: usize) -> Vec<Picture> {
        let pictures = decode_looping(Arc::from(clip));
        (0..count).map(|_| pictures.recv_timeout(Duration::from_secs(5)).expect("a picture")).collect()
    }

    // A Discord-style GIF: H.264 in MP4, 480 wide, 15 frames over one second; it loops seamlessly.
    #[test]
    fn gifs_clip_around_what_floats_over_them() {
        let b = |x: f32, y: f32, w: f32, h: f32| Bounds::new(point(px(x), px(y)), size(px(w), px(h)));
        let area = |pieces: &[Bounds<Pixels>]| pieces.iter().map(|p| f32::from(p.size.width) * f32::from(p.size.height)).sum::<f32>();
        // A toolbar over the top-right corner of a 100x100 GIF takes its 40x20 out and nothing else.
        let pieces = subtract(b(0., 0., 100., 100.), &[b(70., -10., 40., 30.)]);
        assert_eq!(area(&pieces), 100. * 100. - 30. * 20.);
        assert!(pieces.iter().all(|p| !p.intersects(&b(70., -10., 40., 30.))));
        // Nothing over it: whole.
        assert_eq!(subtract(b(0., 0., 10., 10.), &[b(20., 20., 5., 5.)]), vec![b(0., 0., 10., 10.)]);
    }

    #[test]
    fn decodes_scaled_pictures_and_loops() {
        let pictures = first(include_bytes!("../testdata/gifv.mp4"), 31);
        let size = |p: &Picture| p.image.size(0);
        assert_eq!((size(&pictures[0]).width.0, size(&pictures[0]).height.0), (480, 270), "under MAX_WIDTH: kept at its own size");
        assert!(pictures.windows(2).all(|w| w[1].at > w[0].at), "times only move forward, across loops too");
        assert_eq!(pictures[15].at, Duration::from_secs(1), "the second loop starts where the first ended");
        assert!(pictures[30].at >= Duration::from_secs(2));
    }

    // Reordered B-frames used to make the decoder bail halfway; every frame still comes out, in order.
    #[test]
    fn b_frame_clips_play_every_frame() {
        let pictures = first(include_bytes!("../testdata/gifv-bframes.mp4"), 16);
        assert!(pictures.windows(2).all(|w| w[1].at > w[0].at));
        assert_eq!(pictures[15].at, Duration::from_secs(1));
    }

    // A short clip is decoded once: the second loop replays the same pictures.
    #[test]
    fn short_clips_replay_instead_of_decoding_again() {
        let pictures = first(include_bytes!("../testdata/gifv.mp4"), 30);
        assert!(Arc::ptr_eq(&pictures[0].image, &pictures[15].image), "loop two reuses loop one's pictures");
        assert_eq!(pictures[15].at, Duration::from_secs(1));
    }

    // Only the first loop's pictures can be marked for replay: later loops of a clip too long to
    // replay must not be, or the player would keep every one of them.
    #[test]
    fn only_the_first_loop_is_offered_for_replay() {
        let pictures = first(include_bytes!("../testdata/gifv.mp4"), 45);
        // This clip is short, so it replays: loops two and three are loop one's pictures again.
        assert!(pictures[15..].iter().all(|p| p.kept));
        assert!(Arc::ptr_eq(&pictures[30].image, &pictures[0].image));
    }

    // The decoder stays a few pictures ahead instead of decoding the whole clip.
    #[test]
    fn decoder_holds_only_a_few_pictures() {
        let pictures = decode_looping(Arc::from(&include_bytes!("../testdata/gifv.mp4")[..]));
        // A fixed wait from the start flaked in the full suite: time from the first picture, so
        // opening the clip and starting the decoder don't count, and wait far longer than four
        // pictures take, so a decoder the channel didn't stop would get through the clip.
        pictures.recv_timeout(Duration::from_secs(5)).expect("a picture");
        std::thread::sleep(Duration::from_secs(1));
        // Draining lets the decoder slip in one or two more; a whole loop would be 15.
        let held = pictures.try_iter().count();
        assert!((AHEAD..AHEAD + 3).contains(&held), "bounded by the channel, not the clip: {held}");
    }

    #[test]
    fn clip_cache_keeps_recent_clips_within_its_cap() {
        let big: Arc<[u8]> = Arc::from(vec![0u8; CLIP_CACHE_BYTES / 2 + 1]);
        cache_clip("test://a".into(), big.clone());
        cache_clip("test://b".into(), big.clone());
        assert!(cached_clip("test://a").is_none(), "the older clip made room");
        assert!(cached_clip("test://b").is_some());
    }
}
