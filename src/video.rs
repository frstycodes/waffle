// Discord's inline video player: the poster and a play button until played, then the clip under a
// control bar (play/pause, time, scrubber, volume, fullscreen) that hides while it plays and the
// pointer rests. One video plays at a time, and a video stops when it scrolls out of view.
use std::{
    cell::Cell,
    collections::HashMap,
    f32::consts::TAU,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, Bounds, Context, Entity, FocusHandle, FontFeatures, Global, ImageSource, KeyDownEvent, MouseButton,
    ObjectFit, Pixels, Render, RenderImage, SharedString, WeakEntity, Window, canvas,
    component::Icon,
    div, img, linear_color_stop, linear_gradient, prelude::*, px, relative,
};
use gpui_motion_kit::{
    Motions,
    core::{Bezier, Easing, Motion, Spring, Target, Transition},
};

use crate::{icons::Glyph, playback::Playback, ui};

mod chrome;


// The volume knob's width.
const VOLUME_KNOB: f32 = 12.;
// The gap between loading dots.
const DOT_GAP: f32 = 3.;

// The bar hides after the pointer rests this long while the video plays.
const IDLE: Duration = Duration::from_secs(2);
// Not painted for this long while playing: scrolled away or another channel opened.
const OFFSCREEN: Duration = Duration::from_millis(250);
const TICK: Duration = Duration::from_millis(8);
// Bigger files open in the browser rather than being held in memory.
const MAX_BYTES: u64 = 300 << 20;
const SKIP: Duration = Duration::from_secs(5);
// Seeks while dragging the scrubber, at most this often; each restarts decoding at a keyframe.
const SCRUB_EVERY: Duration = Duration::from_millis(60);
const SLOT: f32 = 24.;
const ICON: f32 = 20.;
const CENTER: f32 = 48.;
const RADIUS: f32 = 8.;

// The players in the chat, by message and attachment, and which one is playing or fullscreen.
struct Videos {
    // lint-allow(unbounded-growth): one player per video attachment shown this session, a few hundred at most
    players: HashMap<String, Entity<VideoPlayer>>,
    playing: Option<WeakEntity<VideoPlayer>>,
    fullscreen: Option<Entity<VideoPlayer>>,
    // Discord keeps one volume for every video, audio file and voice message.
    level: Level,
}

impl Global for Videos {}

impl Default for Videos {
    fn default() -> Self {
        Self { players: HashMap::new(), playing: None, fullscreen: None, level: Level { volume: 1., muted: false } }
    }
}

#[derive(Clone, Copy)]
struct Level {
    volume: f32,
    muted: bool,
}

impl Level {
    fn heard(self) -> f32 {
        if self.muted { 0. } else { self.volume }
    }
}

// The inline player for one video attachment, kept across renders so it remembers where it was.
// lint-allow(custom-component): ui::media is an image, not a player; this wraps a cached VideoPlayer entity and builds no design-system piece itself
pub fn tile(key: String, url: &str, poster: &str, cx: &mut App) -> AnyElement {
    if let Some(player) = cx.try_global::<Videos>().and_then(|v| v.players.get(&key)).cloned() {
        return player.into_any_element();
    }
    let (url, poster) = (url.to_owned(), poster.to_owned());
    // lint-allow(render-work): built once per attachment, then every later render takes it from the map above
    let player = cx.new(|cx| VideoPlayer::new(url, poster, cx));
    cx.default_global::<Videos>().players.insert(key, player.clone());
    player.into_any_element()
}

// The shared volume and mute, for the audio players.
pub fn volume(cx: &App) -> (f32, bool) {
    cx.try_global::<Videos>().map_or((1., false), |v| (v.level.volume, v.level.muted))
}

pub fn set_volume(volume: f32, muted: bool, cx: &mut App) {
    cx.default_global::<Videos>().level = Level { volume, muted };
}

// Drives the player kept under `key` from outside it (the media viewer's keys).
pub fn toggle(key: &str, cx: &mut App) {
    with_player(key, cx, VideoPlayer::toggle);
}

pub fn pause(key: &str, cx: &mut App) {
    with_player(key, cx, VideoPlayer::pause);
}

pub fn skip(key: &str, forward: bool, cx: &mut App) {
    with_player(key, cx, move |p, cx| p.skip(forward, cx));
}

pub fn toggle_mute(key: &str, cx: &mut App) {
    with_player(key, cx, |p, cx| p.set_level(Level { muted: !p.level.muted, ..p.level }, cx));
}

fn with_player(key: &str, cx: &mut App, f: impl FnOnce(&mut VideoPlayer, &mut Context<VideoPlayer>)) {
    let Some(player) = cx.try_global::<Videos>().and_then(|v| v.players.get(key)).cloned() else { return };
    player.update(cx, f);
}

// A video is playing somewhere, so the app shouldn't drop to background mode.
pub fn is_playing(cx: &App) -> bool {
    cx.try_global::<Videos>().and_then(|v| v.playing.as_ref()?.upgrade()).is_some_and(|p| p.read(cx).is_playing())
}

// The video shown alone over the whole window, when one is fullscreen.
pub fn fullscreen(cx: &App) -> Option<AnyElement> {
    cx.try_global::<Videos>()?.fullscreen.clone().map(|player| player.into_any_element())
}

pub struct VideoPlayer {
    url: String,
    poster: String,
    stage: Stage,
    frame: Option<Arc<RenderImage>>,
    level: Level,
    focus: FocusHandle,
    pointer_at: Instant,
    hovered: bool,
    hot: Option<Control>,
    pressed: Option<Control>,
    scrub: Option<Scrub>,
    scrub_bounds: Rc<Cell<Bounds<Pixels>>>,
    volume_bounds: Rc<Cell<Bounds<Pixels>>>,
    adjusting_volume: bool,
    fullscreen: bool,
    os_fullscreen: bool,
    // Set when the ticker asks for a render, cleared when the player paints: still set a while later
    // means it isn't on screen.
    unpainted_since: Rc<Cell<Option<Instant>>>,
    notified_at: Instant,
    bar_was_shown: bool,
    ticking: bool,
    motions: Motions<Control>,
}

enum Stage {
    // Not loaded: never played, or set aside for another video, to pick up again at `resume`.
    Poster { resume: Duration },
    Loading,
    Ready(Box<Playback>),
    Failed(SharedString),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Control {
    Center,
    Play,
    Volume,
    VolumePanel,
    Fullscreen,
    Scrubber,
    Knob,
    Bar,
}

struct Scrub {
    at: Duration,
    resume: bool,
    sought_at: Instant,
}

// Drag payloads for the scrubber and the volume slider; drawn as nothing.
#[derive(Clone)]
struct ScrubDrag;
#[derive(Clone)]
struct VolumeDrag;

impl Render for ScrubDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl Render for VolumeDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl VideoPlayer {
    fn new(url: String, poster: String, cx: &mut Context<Self>) -> Self {
        let level = cx.default_global::<Videos>().level;
        Self {
            url,
            poster,
            stage: Stage::Poster { resume: Duration::ZERO },
            frame: None,
            level,
            focus: cx.focus_handle(),
            pointer_at: Instant::now(),
            hovered: false,
            hot: None,
            pressed: None,
            scrub: None,
            scrub_bounds: Rc::default(),
            volume_bounds: Rc::default(),
            adjusting_volume: false,
            fullscreen: false,
            os_fullscreen: false,
            unpainted_since: Rc::default(),
            notified_at: Instant::now(),
            bar_was_shown: false,
            ticking: false,
            motions: Motions::new(),
        }
    }

    fn playback(&mut self) -> Option<&mut Playback> {
        match &mut self.stage {
            Stage::Ready(playback) => Some(playback),
            _ => None,
        }
    }

    fn is_playing(&self) -> bool {
        matches!(&self.stage, Stage::Ready(p) if p.is_playing())
    }

    fn toggle(&mut self, cx: &mut Context<Self>) {
        match &self.stage {
            Stage::Poster { resume } => self.load(*resume, cx),
            Stage::Loading => {}
            Stage::Ready(p) if p.is_playing() => self.pause(cx),
            Stage::Ready(_) => self.play(cx),
            Stage::Failed(_) => cx.open_url(&self.url),
        }
    }

    fn load(&mut self, resume: Duration, cx: &mut Context<Self>) {
        self.stage = Stage::Loading;
        let url = self.url.clone();
        cx.spawn(async move |this, cx| {
            let opened = crate::discord::runtime().spawn(open(url)).await.unwrap_or_else(|e| Err(e.to_string()));
            this.update(cx, |this, cx| match opened {
                Ok(mut playback) => {
                    playback.set_volume(this.level.heard());
                    if !resume.is_zero() {
                        playback.seek(resume);
                    }
                    this.stage = Stage::Ready(Box::new(playback));
                    this.play(cx);
                }
                Err(error) => {
                    this.stage = Stage::Failed(error.into());
                    cx.notify();
                }
            })
            .ok();
        })
        .detach(); // lint-allow(detached-task): one open: ends when it returns, or when the player is dropped
        cx.notify();
    }

    fn play(&mut self, cx: &mut Context<Self>) {
        let me = cx.entity();
        let previous = cx.default_global::<Videos>().playing.replace(me.downgrade());
        if let Some(previous) = previous.and_then(|p| p.upgrade()).filter(|p| *p != me) {
            previous.update(cx, |p, cx| p.unload(cx));
        }
        let Some(playback) = self.playback() else { return };
        playback.play();
        self.pointer_at = Instant::now();
        self.unpainted_since.set(None);
        self.start_ticking(cx);
        cx.notify();
    }

    // Only the latest video stays loaded (its whole file, decoder and audio); the one before goes back
    // to its poster, remembering where it was.
    fn unload(&mut self, cx: &mut Context<Self>) {
        let Stage::Ready(playback) = &self.stage else { return };
        let resume = if playback.is_ended() { Duration::ZERO } else { playback.position() };
        self.stage = Stage::Poster { resume };
        if let Some(frame) = self.frame.take() {
            cx.drop_image(frame, None);
        }
        cx.notify();
    }

    fn pause(&mut self, cx: &mut Context<Self>) {
        if let Some(playback) = self.playback() {
            playback.pause();
        }
        cx.notify();
    }

    fn seek(&mut self, at: Duration, cx: &mut Context<Self>) {
        let Some(playback) = self.playback() else { return };
        playback.seek(at);
        self.start_ticking(cx);
        cx.notify();
    }

    fn skip(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(playback) = self.playback() else { return };
        let at = playback.position();
        self.seek(if forward { at + SKIP } else { at.saturating_sub(SKIP) }, cx);
    }

    fn set_level(&mut self, level: Level, cx: &mut Context<Self>) {
        self.level = level;
        if let Some(playback) = self.playback() {
            playback.set_volume(level.heard());
        }
        cx.default_global::<Videos>().level = level;
        cx.notify();
    }

    fn set_fullscreen(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.fullscreen = on;
        self.os_fullscreen = false;
        let me = on.then(|| cx.entity());
        cx.default_global::<Videos>().fullscreen = me;
        if window.is_fullscreen() != on {
            window.toggle_fullscreen();
        }
        if on {
            window.focus(&self.focus, cx);
        }
        self.pointer_at = Instant::now();
        window.refresh();
        cx.notify();
    }

    // Polls for the next picture while playing (or until a seek's picture arrives), and pauses a video
    // that's no longer on screen. Ends itself when there's nothing left to wait for.
    fn start_ticking(&mut self, cx: &mut Context<Self>) {
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
        .detach(); // lint-allow(detached-task): the tick loop ends when tick says stop or the player is dropped
    }

    fn tick(&mut self, cx: &mut Context<Self>) -> bool {
        let offscreen = self.unpainted_since.get().is_some_and(|t| t.elapsed() > OFFSCREEN) && !self.fullscreen;
        let bar_shown = self.bar_shown();
        let Stage::Ready(playback) = &mut self.stage else {
            self.ticking = false;
            return false;
        };
        let was_playing = playback.is_playing();
        if was_playing && (offscreen || playback.is_ended()) {
            playback.pause();
        }
        let shown = playback.poll();
        let waiting = playback.is_playing() || playback.is_seeking();
        let changed = shown.is_some() || was_playing != playback.is_playing() || bar_shown != self.bar_was_shown;
        if let Some(old) = shown.and_then(|frame| self.frame.replace(frame)) {
            // Each picture is its own GPU texture; the old one must be freed or the atlas grows for good.
            cx.drop_image(old, None);
        }
        // Renders redraw the whole window: only for a new picture, a state change, or the bar's clock.
        if changed || (bar_shown && self.notified_at.elapsed() > Duration::from_millis(100)) {
            self.bar_was_shown = bar_shown;
            self.notified_at = Instant::now();
            if self.unpainted_since.get().is_none() {
                self.unpainted_since.set(Some(Instant::now()));
            }
            cx.notify();
        }
        self.ticking = waiting;
        waiting
    }

    fn begin_scrub(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let Some(playback) = self.playback() else { return };
        let resume = playback.is_playing();
        playback.pause();
        self.scrub = Some(Scrub { at: Duration::ZERO, resume, sought_at: Instant::now().checked_sub(SCRUB_EVERY).unwrap_or_else(Instant::now) });
        self.scrub_to(x, self.scrub_bounds.get(), cx);
    }

    fn scrub_to(&mut self, x: Pixels, bounds: Bounds<Pixels>, cx: &mut Context<Self>) {
        let Some(duration) = self.playback().map(|p| p.duration()) else { return };
        let Some(scrub) = &mut self.scrub else { return };
        scrub.at = duration.mul_f32(fraction(x - bounds.left(), bounds.size.width));
        if scrub.sought_at.elapsed() >= SCRUB_EVERY {
            scrub.sought_at = Instant::now();
            let at = scrub.at;
            self.seek(at, cx);
        }
        cx.notify();
    }

    fn end_scrub(&mut self, cx: &mut Context<Self>) {
        let Some(scrub) = self.scrub.take() else { return };
        self.seek(scrub.at, cx);
        if scrub.resume {
            self.play(cx);
        }
    }

    fn volume_to(&mut self, y: Pixels, bounds: Bounds<Pixels>, cx: &mut Context<Self>) {
        let volume = fraction(bounds.bottom() - y, bounds.size.height);
        self.set_level(Level { volume, muted: volume == 0. }, cx);
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "space" | "k" => self.toggle(cx),
            "left" | "j" => self.skip(false, cx),
            "right" | "l" => self.skip(true, cx),
            "m" => self.set_level(Level { muted: !self.level.muted, ..self.level }, cx),
            "f" => self.set_fullscreen(!self.fullscreen, window, cx),
            "escape" if self.fullscreen => self.set_fullscreen(false, window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    fn bar_shown(&self) -> bool {
        let Stage::Ready(playback) = &self.stage else { return false };
        let busy = self.scrub.is_some() || self.adjusting_volume || self.hot.is_some();
        let pointer_near = (self.hovered || self.fullscreen) && self.pointer_at.elapsed() < IDLE;
        !playback.is_playing() || busy || pointer_near
    }
}

// Downloads the clip and opens it for playback, off the UI thread.
async fn open(url: String) -> Result<Playback, String> {
    if let Some(bytes) = crate::discord::demo_media(&url) {
        let bytes: Arc<[u8]> = bytes.to_vec().into();
        return tokio::task::spawn_blocking(move || Playback::open(bytes)).await.map_err(|e| e.to_string())?;
    }
    let response = reqwest::get(&url).await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    if response.content_length().is_some_and(|len| len > MAX_BYTES) {
        return Err("Too large to play here".into());
    }
    let bytes: Arc<[u8]> = response.bytes().await.map_err(|e| e.to_string())?.to_vec().into();
    tokio::task::spawn_blocking(move || Playback::open(bytes)).await.map_err(|e| e.to_string())?
}

fn fraction(offset: Pixels, length: Pixels) -> f32 {
    if length <= Pixels::ZERO { 0. } else { (offset / length).clamp(0., 1.) }
}

// Controls answer with a quick spring that overshoots a touch and settles inside ~250ms.
fn pop() -> Transition {
    Transition::Spring(Spring::visual(Duration::from_millis(200), 0.3).expect("valid spring"))
}

// Fades for the bar and play button, on the house enter curve.
fn fade() -> Transition {
    Transition::Tween(Duration::from_millis(180), Easing::Bezier(Bezier::new(0.23, 1., 0.32, 1.).expect("monotonic bezier")))
}

impl Render for VideoPlayer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::perf::probe("VideoPlayer");
        if self.fullscreen {
            // Leaving the OS full screen (green button, Esc on some platforms) leaves ours too.
            match window.is_fullscreen() {
                true => self.os_fullscreen = true,
                false if self.os_fullscreen => self.set_fullscreen(false, window, cx),
                false => {}
            }
        }
        let picture = match &self.frame {
            Some(frame) => img(ImageSource::Render(frame.clone())),
            None => img(self.poster.clone()),
        };
        let ended = matches!(&self.stage, Stage::Ready(p) if !p.is_playing() && p.is_ended());
        let center_shown = matches!(self.stage, Stage::Poster { .. } | Stage::Loading) || (ended && self.scrub.is_none());
        let bar_shown = self.bar_shown();
        let [center, bar] = self.motions.frame(
            [
                (Control::Center, Motion::new(Target::default()).initial(Target::default().opacity(0.)).exit(Target::default().opacity(0.)).present(center_shown).transition(fade())),
                (Control::Bar, Motion::new(Target::default().opacity(if bar_shown { 1. } else { 0. })).transition(fade())),
            ],
            window,
            cx,
        );
        let painted = self.unpainted_since.clone();
        // The centring lives on the full-size layer: the motion wrapper doesn't take the layer's size.
        let center = center.render(|f| ui::row().absolute().inset_0().justify_center().child(f.apply(div().child(self.center_button(window, cx)))));
        let failed = match &self.stage {
            Stage::Failed(error) => Some(error.clone()),
            _ => None,
        };
        div()
            .id("video")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(gpui_kit::black())
            .when(!self.fullscreen, |d| d.rounded(px(RADIUS)))
            .cursor_pointer()
            .child(canvas(move |_, _, _| painted.set(None), |_, _, _, _| {}).absolute().size_full())
            .child(picture.size_full().object_fit(ObjectFit::Contain).when(!self.fullscreen, |i| i.rounded(px(RADIUS))))
            .child(center)
            .children(failed.map(failed_note))
            .when(bar.target().alpha() > 0.01, |d| d.child(self.bar(bar.target().alpha(), window, cx)))
            .when(self.corners_shown(), |d| d.child(self.corners(window, cx)))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.hovered = *hovered;
                cx.notify();
            }))
            .on_mouse_move(cx.listener(|this, _, _, cx| {
                let was_hidden = !this.bar_shown();
                this.pointer_at = Instant::now();
                if was_hidden {
                    cx.notify();
                }
            }))
            .on_click(cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                window.focus(&this.focus, cx);
                this.toggle(cx);
            }))
            .on_key_down(cx.listener(Self::on_key))
    }
}


fn loading_dots() -> AnyElement {
    ui::row().items_stretch()
        .gap(px(DOT_GAP))
        .children((0..3).map(|i| {
            div().size_1p5().rounded_full().bg(gpui_kit::white()).with_animation(
                SharedString::from(format!("video-loading-{i}")),
                // lint-allow(endless-animation): drawn only while the clip loads; the view swaps it out when it is ready or has failed
                Animation::new(Duration::from_millis(1200)).repeat(),
                move |dot, t| dot.opacity(0.35 + 0.65 * ((t - i as f32 * 0.18) * TAU).sin().max(0.)),
            )
        }))
        .into_any_element()
}

// Shown when the clip can't play here (another codec, too big, gone): clicking opens it instead.
fn failed_note(error: SharedString) -> AnyElement {
    div()
        .absolute()
        .left_2()
        .bottom_2()
        .px_2()
        .py_1()
        .rounded_sm()
        .bg(gpui_kit::black().opacity(0.7))
        .text_size(ui::TEXT_SM)
        .text_color(gpui_kit::white())
        .child(format!("Can't play this here ({error}). Click to open it."))
        .into_any_element()
}
