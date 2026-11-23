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

use crate::{icons::Glyph, palette, playback::Playback, ui};

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
pub fn tile(key: String, url: &str, poster: &str, cx: &mut App) -> AnyElement {
    if let Some(player) = cx.try_global::<Videos>().and_then(|v| v.players.get(&key)).cloned() {
        return player.into_any_element();
    }
    let (url, poster) = (url.to_owned(), poster.to_owned());
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
        .detach();
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
        .detach();
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
    if length <= px(0.) { 0. } else { (offset / length).clamp(0., 1.) }
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
        let center = center.render(|f| div().absolute().inset_0().flex().items_center().justify_center().child(f.apply(div().child(self.center_button(window, cx)))));
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

impl VideoPlayer {
    // The round play button over the poster, and over the last frame once the video ends.
    fn center_button(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let loading = matches!(self.stage, Stage::Loading);
        let size = self.press_frame(Control::Center, CENTER, window, cx);
        let lift = size.target().y_px();
        let side = size.target().width_px().unwrap_or(CENTER);
        let face = div()
            .relative()
            .top(px(lift))
            .size(px(side))
            .rounded_full()
            .bg(gpui_kit::black().opacity(0.6))
            .flex()
            .items_center()
            .justify_center()
            .text_color(gpui_kit::white())
            .child(match loading {
                true => loading_dots(),
                false => Icon::new(Glyph::Play).size(px(side / 2.)).into_any_element(),
            });
        let ended = matches!(&self.stage, Stage::Ready(p) if !p.is_playing() && p.is_ended());
        self.control(Control::Center, if ended { "Play Again" } else { "Play" }, div().size(px(CENTER)).flex().items_center().justify_center().child(face), |this, _, cx| this.toggle(cx), cx)
    }

    // Discord's bar on a gradient: play/pause, then time and scrubber, then volume and fullscreen.
    fn bar(&mut self, alpha: f32, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let (position, duration) = match &self.stage {
            Stage::Ready(p) => (p.position(), p.duration()),
            _ => (Duration::ZERO, Duration::ZERO),
        };
        let position = self.scrub.as_ref().map_or(position, |s| s.at);
        let playing = self.is_playing();
        let play = self.icon_button(Control::Play, if playing { Glyph::Pause } else { Glyph::Play }, if playing { "Pause" } else { "Play" }, |this, _, cx| this.toggle(cx), window, cx);
        let scrubber = self.scrubber(if duration.is_zero() { 0. } else { position.as_secs_f32() / duration.as_secs_f32() }, window, cx);
        let tabular = FontFeatures(Arc::new(vec![("tnum".into(), 1)]));
        let time = div()
            .flex_none()
            .flex()
            .gap_1()
            .text_size(ui::TEXT_XS)
            .font_features(tabular)
            .text_color(gpui_kit::white())
            .child(crate::playback::time_label(position))
            .child(div().text_color(gpui_kit::white().opacity(0.6)).child("/"))
            .child(crate::playback::time_label(duration));
        let shade = |top: f32, bottom: f32| linear_gradient(180., linear_color_stop(gpui_kit::black().opacity(top), 0.), linear_color_stop(gpui_kit::black().opacity(bottom), 1.));
        div()
            .id("video-bar")
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .opacity(alpha)
            .when(!self.fullscreen, |d| d.rounded_b(px(RADIUS)))
            // CSS's three-stop linear-gradient(180deg, transparent, rgba(0,0,0,.68) 45%, rgba(0,0,0,.92)), as two halves.
            .child(div().absolute().top_0().left_0().right_0().h(relative(0.45)).bg(shade(0., 0.68)))
            .child(div().absolute().top(relative(0.45)).bottom_0().left_0().right_0().bg(shade(0.68, 0.92)).when(!self.fullscreen, |d| d.rounded_b(px(RADIUS))))
            .child(
                div()
                    .relative()
                    .pt_7()
                    .pb_2p5()
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(play)
                    // The time sits above the scrubber so even a narrow video shows it whole; volume
                    // and full screen live in the top corners (see `corners`).
                    .child(div().flex_1().min_w_0().flex().flex_col().gap_0p5().child(time).child(scrubber)),
            )
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .into_any_element()
    }

    // Hover lifts a control 1px; pressing shrinks it to 94%, springing back with a slight overshoot.
    fn press_frame(&mut self, control: Control, side: f32, window: &mut Window, cx: &App) -> gpui_motion_kit::core::Frame {
        let (hot, pressed) = (self.hot == Some(control), self.pressed == Some(control));
        let target = Target::default().y(if hot && !pressed { -1. } else { 0. }).width(if pressed { side * 0.94 } else { side });
        *self.motions.frame_one(control, Motion::new(target).transition(pop()), window, cx)
    }

    fn icon_button(
        &mut self,
        control: Control,
        glyph: Glyph,
        label: &'static str,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let frame = self.press_frame(control, SLOT, window, cx);
        let scale = frame.target().width_px().unwrap_or(SLOT) / SLOT;
        let face = div()
            .relative()
            .top(px(frame.target().y_px()))
            .text_color(gpui_kit::white())
            .opacity(if self.hot == Some(control) { 1. } else { 0.86 })
            .child(Icon::new(glyph).size(px(ICON * scale)));
        let slot = div().size(px(SLOT)).flex_none().flex().items_center().justify_center().child(face);
        self.control(control, label, slot, action, cx)
    }

    // Wires a control's hover, press and click; the click stays off the video under it.
    fn control(&mut self, control: Control, label: &'static str, body: gpui_kit::Div, action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static, cx: &mut Context<Self>) -> AnyElement {
        body.id(SharedString::from(format!("video-{control:?}")))
            .cursor_pointer()
            .tooltip(move |window, cx| ui::tip(label, window, cx))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                match *hovered {
                    true => this.hot = Some(control),
                    false if this.hot == Some(control) => this.hot = None,
                    false => {}
                }
                cx.notify();
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                this.pressed = Some(control);
                cx.stop_propagation();
                cx.notify();
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                this.pressed = None;
                cx.notify();
            }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                if this.pressed == Some(control) {
                    this.pressed = None;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                window.focus(&this.focus, cx);
                action(this, window, cx);
            }))
            .into_any_element()
    }

    // A 4px track with a accent fill; the knob springs in while the pointer is on it or dragging.
    fn scrubber(&mut self, fraction_played: f32, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let active = self.hot == Some(Control::Scrubber) || self.scrub.is_some();
        let knob = self.motions.frame_one(Control::Knob, Motion::new(Target::default().width(if active { 12. } else { 0. })).transition(pop()), window, cx);
        let knob_side = knob.target().width_px().unwrap_or(0.).max(0.);
        let played = relative(fraction_played.clamp(0., 1.));
        let record = self.scrub_bounds.clone();
        div()
            .id("video-scrubber")
            .relative()
            .flex_1()
            .min_w_10()
            .h_4()
            .cursor_pointer()
            .child(canvas(move |bounds, _, _| record.set(bounds), |_, _, _, _| {}).absolute().size_full())
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top_1p5()
                    .h_1()
                    .rounded_xs()
                    .bg(gpui_kit::white().opacity(0.24))
                    .child(div().h_full().w(played).rounded_xs().bg(palette::accent())),
            )
            .child(div().absolute().top(px(8. - knob_side / 2.)).left(played).ml(px(-knob_side / 2.)).size(px(knob_side)).rounded_full().bg(gpui_kit::white()))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                match *hovered {
                    true => this.hot = Some(Control::Scrubber),
                    false if this.hot == Some(Control::Scrubber) => this.hot = None,
                    false => {}
                }
                cx.notify();
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &gpui_kit::MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.begin_scrub(e.position.x, cx);
            }))
            .on_drag(ScrubDrag, |drag, _, _, cx| cx.new(|_| drag.clone()))
            .on_drag_move::<ScrubDrag>(cx.listener(|this, e: &gpui_kit::DragMoveEvent<ScrubDrag>, _, cx| this.scrub_to(e.event.position.x, e.bounds, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| this.end_scrub(cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| this.end_scrub(cx)))
            .into_any_element()
    }

    // The speaker mutes on click; hovering it raises Discord's vertical volume slider above it.
    // Shown while the pointer is over the video (in full screen, until it rests), or while one of
    // them is in use.
    fn corners_shown(&self) -> bool {
        let busy = self.adjusting_volume || matches!(self.hot, Some(Control::Volume | Control::VolumePanel | Control::Fullscreen));
        busy || (self.hovered && (!self.fullscreen || self.pointer_at.elapsed() < IDLE))
    }

    // Full screen in the top-left corner and volume in the top-right, each on a dark pill so they
    // read over any picture.
    fn corners(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let full = self.icon_button(
            Control::Fullscreen,
            if self.fullscreen { Glyph::FullscreenExit } else { Glyph::Fullscreen },
            if self.fullscreen { "Exit Full Screen" } else { "Full Screen" },
            |this, window, cx| this.set_fullscreen(!this.fullscreen, window, cx),
            window,
            cx,
        );
        let volume = self.volume(window, cx);
        // A click on a pill's edge stays off the video under it.
        let pill = || {
            div().absolute().top_2().p_1().rounded(px(RADIUS)).bg(gpui_kit::black().opacity(0.56)).on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        };
        div()
            .absolute()
            .inset_0()
            .child(pill().left_2().child(full))
            .child(pill().right_2().child(volume))
            .into_any_element()
    }

    fn volume(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let glyph = if self.level.heard() == 0. { Glyph::VolumeOff } else { Glyph::VolumeHigh };
        let label = if self.level.muted { "Unmute" } else { "Mute" };
        let button = self.icon_button(Control::Volume, glyph, label, |this, _, cx| this.set_level(Level { muted: !this.level.muted, ..this.level }, cx), window, cx);
        let open = matches!(self.hot, Some(Control::Volume | Control::VolumePanel)) || self.adjusting_volume;
        let hidden = Target::default().opacity(0.).y(-6.);
        let panel = self.motions.frame_one(Control::VolumePanel, Motion::new(Target::default()).initial(hidden).exit(hidden).present(open).transition(pop()), window, cx);
        let level = relative(self.level.heard());
        let record = self.volume_bounds.clone();
        let slider = div()
            .id("video-volume-panel")
            .w_8()
            .h_24()
            .py_3()
            .rounded(px(RADIUS))
            .bg(gpui_kit::black().opacity(0.8))
            .flex()
            .justify_center()
            .cursor_pointer()
            .child(
                div()
                    .relative()
                    .w_1()
                    .h_full()
                    .rounded_xs()
                    .bg(gpui_kit::white().opacity(0.24))
                    .child(canvas(move |bounds, _, _| record.set(bounds), |_, _, _, _| {}).absolute().size_full())
                    .child(div().absolute().bottom_0().left_0().right_0().h(level).rounded_xs().bg(palette::accent()))
                    .child(div().absolute().bottom(level).left_neg_1().mb_neg_1p5().size_3().rounded_full().bg(gpui_kit::white())),
            )
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &gpui_kit::MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.adjusting_volume = true;
                this.volume_to(e.position.y, this.volume_bounds.get(), cx);
            }))
            .on_drag(VolumeDrag, |drag, _, _, cx| cx.new(|_| drag.clone()))
            .on_drag_move::<VolumeDrag>(cx.listener(|this, e: &gpui_kit::DragMoveEvent<VolumeDrag>, _, cx| {
                if this.adjusting_volume {
                    this.volume_to(e.event.position.y, this.volume_bounds.get(), cx);
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                this.adjusting_volume = false;
                cx.notify();
            }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| {
                if this.adjusting_volume {
                    this.adjusting_volume = false;
                    cx.notify();
                }
            }))
            .on_click(|_, _, cx| cx.stop_propagation());
        // The speaker is in the top corner, so the panel opens below it. The gap between them belongs to
        // the panel, so moving down to it never closes it.
        let reach = div()
            .id("video-volume-reach")
            .absolute()
            .top(px(SLOT))
            .left(px((SLOT - 32.) / 2.))
            .pt_2()
            .child(panel.render(|f| f.apply(slider)))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                match *hovered {
                    true => this.hot = Some(Control::VolumePanel),
                    false if this.hot == Some(Control::VolumePanel) => this.hot = None,
                    false => {}
                }
                cx.notify();
            }));
        div().relative().flex_none().child(button).when(panel.is_mounted(), |d| d.child(reach)).into_any_element()
    }
}

fn loading_dots() -> AnyElement {
    div()
        .flex()
        .gap(px(3.))
        .children((0..3).map(|i| {
            div().size_1p5().rounded_full().bg(gpui_kit::white()).with_animation(
                SharedString::from(format!("video-loading-{i}")),
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
