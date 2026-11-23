// Discord's two inline audio players (docs/discord-voice-messages-re.md): a voice message's pill (play,
// waveform, time left, speed, volume) and an audio file's card (name, play, time, seek bar, volume).
// One voice message plays at a time; audio files and videos don't take part. Speed and where each voice
// message was left are kept on this device; volume is the one videos use.
use std::{
    cell::Cell,
    collections::HashMap,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use gpui_kit::{
    AnyElement, Anchor, App, Bounds, Context, Entity, FocusHandle, FontFeatures, FontWeight, Global, Hsla, KeyDownEvent, MouseButton, MouseDownEvent, Pixels, Render,
    SharedString, WeakEntity, Window, canvas, component::Icon, div, fill, point, prelude::*, px, size,
};

use crate::{
    Waffle,
    audio::{self, AudioPlayback},
    discord::{File, Message},
    icons::Glyph,
    palette::{self, Level},
    settings::Prefs,
    ui::{self, RADIUS_LG, RADIUS_SM, RADIUS_XL, RADIUS_XS, Squircle},
};

mod view;


// The widest the player grows.
const PLAYER_MAX: f32 = 440.;
// The volume card's width.
const VOLUME_WIDTH: f32 = 136.;
// The volume card's left edge from its button: centred over it.
const VOLUME_LEFT: f32 = -54.;

const RATES: [f32; 4] = [0.75, 1., 1.5, 2.];
const TICK: Duration = Duration::from_millis(16);
// Not painted for this long while playing: scrolled away or another channel opened.
const OFFSCREEN: Duration = Duration::from_millis(250);
// The end rewinds at once, and shows the whole length again this long after.
const SETTLE: Duration = Duration::from_millis(500);
const LINGER: Duration = Duration::from_millis(150);
const SKIP: Duration = Duration::from_secs(5);
const RESUME_KEPT: usize = 25;
// ponytail: whole files are held decoded in memory; a stream-from-disk decoder if big files show up.
const MAX_BYTES: u64 = 100 << 20;
// Discord's waveform: 2px bars on a 6px pitch, 32px tall, 40px wide at half a second growing to 294 at 45.
const BAR: f32 = 2.;
const PITCH: f32 = 6.;
const WAVE_HEIGHT: f32 = 32.;
// Discord's volume slider is perceptual: amplitude = position^2.8.
const CURVE: f32 = 2.8;

#[derive(Default)]
struct Players {
    players: HashMap<String, Entity<AudioPlayer>>,
    voice: Option<WeakEntity<AudioPlayer>>,
}

impl Global for Players {}

// The player for one audio file of `message`, kept across renders so it remembers where it was.
pub fn tile(message: &Message, file: &File, waffle: WeakEntity<Waffle>, cx: &mut App) -> AnyElement {
    let key = format!("{}-{}", message.id, file.url);
    if let Some(player) = cx.try_global::<Players>().and_then(|p| p.players.get(&key)).cloned() {
        return player.into_any_element();
    }
    let (file, voice) = (file.clone(), message.is_voice());
    let player = cx.new(|cx| AudioPlayer::new(key.clone(), file, voice, waffle, cx));
    cx.default_global::<Players>().players.insert(key, player.clone());
    player.into_any_element()
}

pub struct AudioPlayer {
    key: String,
    file: File,
    voice: bool,
    waffle: WeakEntity<Waffle>,
    stage: Stage,
    // Played since it last ended: the voice pill counts down and colours what's been heard.
    played: bool,
    // A voice message's saved spot, sought to once it loads.
    resume: Duration,
    // Dragging across the waveform, and whether to play again on release.
    scrubbing: Option<bool>,
    // The speaker and the slider card under the pointer, and the slider held: any keeps the card open.
    volume_hold: [bool; 3],
    volume_open: bool,
    focus: FocusHandle,
    unpainted_since: Rc<Cell<Option<Instant>>>,
    ticking: bool,
}

enum Stage {
    Idle,
    Loading { play: bool },
    Ready(Box<AudioPlayback>),
    Failed,
}

#[derive(Clone)]
struct WaveDrag;

impl Render for WaveDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl AudioPlayer {
    fn new(key: String, file: File, voice: bool, waffle: WeakEntity<Waffle>, cx: &mut Context<Self>) -> Self {
        let resume = waffle.upgrade().and_then(|w| saved_spot(&w.read(cx).prefs, &file.id)).unwrap_or_default();
        Self {
            key,
            file,
            voice,
            waffle,
            stage: Stage::Idle,
            played: !resume.is_zero(),
            resume,
            scrubbing: None,
            volume_hold: [false; 3],
            volume_open: false,
            focus: cx.focus_handle(),
            unpainted_since: Rc::default(),
            ticking: false,
        }
    }

    fn playback(&mut self) -> Option<&mut AudioPlayback> {
        match &mut self.stage {
            Stage::Ready(playback) => Some(playback),
            _ => None,
        }
    }

    fn is_playing(&self) -> bool {
        matches!(&self.stage, Stage::Ready(p) if p.is_playing())
    }

    fn duration(&self) -> Option<Duration> {
        match &self.stage {
            Stage::Ready(p) => Some(p.duration()),
            _ => self.file.duration_secs.filter(|d| *d > 0.).map(Duration::from_secs_f32),
        }
    }

    fn position(&self) -> Duration {
        match &self.stage {
            Stage::Ready(p) => p.position(),
            _ => self.resume,
        }
    }

    fn rate(&self, cx: &App) -> f32 {
        self.waffle.upgrade().map_or(1., |w| w.read(cx).prefs.voice_rate)
    }

    fn toggle(&mut self, cx: &mut Context<Self>) {
        match &self.stage {
            Stage::Idle => self.load(true, cx),
            Stage::Loading { .. } => self.stage = Stage::Loading { play: true },
            Stage::Ready(p) if p.is_playing() => self.pause(cx),
            Stage::Ready(_) => self.play(cx),
            Stage::Failed => cx.open_url(&self.file.url),
        }
        cx.notify();
    }

    fn load(&mut self, play: bool, cx: &mut Context<Self>) {
        self.stage = Stage::Loading { play };
        let url = self.file.url.clone();
        cx.spawn(async move |this, cx| {
            let opened = crate::discord::runtime().spawn(open(url)).await.unwrap_or_else(|e| Err(e.to_string()));
            this.update(cx, |this, cx| this.loaded(opened, cx)).ok();
        })
        .detach();
        cx.notify();
    }

    fn loaded(&mut self, opened: Result<AudioPlayback, String>, cx: &mut Context<Self>) {
        let Stage::Loading { play } = self.stage else { return };
        let mut playback = match opened {
            Ok(playback) => playback,
            Err(error) => {
                eprintln!("[audio] {}: {error}", self.file.name);
                self.stage = Stage::Failed;
                return cx.notify();
            }
        };
        if !self.resume.is_zero() {
            playback.seek(self.resume);
        }
        self.stage = Stage::Ready(Box::new(playback));
        self.sync(cx);
        if play {
            self.play(cx);
        }
        cx.notify();
    }

    fn play(&mut self, cx: &mut Context<Self>) {
        if self.voice {
            let me = cx.entity();
            let previous = cx.default_global::<Players>().voice.replace(me.downgrade());
            if let Some(previous) = previous.and_then(|p| p.upgrade()).filter(|p| *p != me) {
                previous.update(cx, |p, cx| p.pause(cx));
            }
        }
        self.sync(cx);
        let Some(playback) = self.playback() else { return };
        playback.play();
        self.played = true;
        self.unpainted_since.set(None);
        self.start_ticking(cx);
        cx.notify();
    }

    fn pause(&mut self, cx: &mut Context<Self>) {
        let Some(playback) = self.playback() else { return };
        playback.pause();
        let at = playback.position();
        self.remember(at, cx);
        cx.notify();
    }

    fn seek(&mut self, at: Duration, cx: &mut Context<Self>) {
        let Some(playback) = self.playback() else { return };
        playback.seek(at);
        self.played = true;
        cx.notify();
    }

    // Speed and volume are shared: whichever player changed them, the one playing follows.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let rate = if self.voice { self.rate(cx) } else { 1. };
        let amplitude = heard(cx);
        let Some(playback) = self.playback() else { return };
        playback.set_rate(rate);
        playback.set_volume(amplitude);
    }

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
        let offscreen = self.unpainted_since.get().is_some_and(|t| t.elapsed() > OFFSCREEN);
        self.sync(cx);
        let Some(playback) = self.playback() else {
            self.ticking = false;
            return false;
        };
        if playback.is_ended() {
            self.ended(cx);
        } else if offscreen && playback.is_playing() {
            self.pause(cx);
        }
        if self.unpainted_since.get().is_none() {
            self.unpainted_since.set(Some(Instant::now()));
        }
        cx.notify();
        self.ticking = self.is_playing();
        self.ticking
    }

    // A voice message rewinds and forgets its spot, then shows its whole length again; an audio file
    // stays at the end under Play again.
    fn ended(&mut self, cx: &mut Context<Self>) {
        if !self.voice {
            return;
        }
        if let Some(playback) = self.playback() {
            playback.seek(Duration::ZERO);
            playback.pause();
        }
        self.remember(Duration::ZERO, cx);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTLE).await;
            this.update(cx, |this, cx| {
                if !this.is_playing() && this.position().is_zero() {
                    this.played = false;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    // Discord keeps a voice message's spot only between half a second in and 95% through.
    fn remember(&mut self, at: Duration, cx: &mut Context<Self>) {
        self.resume = at;
        let (Some(duration), true) = (self.duration(), self.voice) else { return };
        let id = self.file.id.clone();
        let keep = at.as_secs_f32() > 0.5 && at < duration.mul_f32(0.95);
        self.waffle
            .update(cx, |w, _| {
                set_spot(&mut w.prefs, &id, keep.then_some(at));
                w.prefs.save();
            })
            .ok();
    }

    fn next_rate(&mut self, cx: &mut Context<Self>) {
        let rate = self.rate(cx);
        let next = RATES[RATES.iter().position(|r| *r == rate).map_or(0, |i| (i + 1) % RATES.len())];
        self.waffle
            .update(cx, |w, _| {
                w.prefs.voice_rate = next;
                w.prefs.save();
            })
            .ok();
        self.sync(cx);
        redraw_others(cx.entity(), cx);
        cx.notify();
    }

    fn set_volume(&mut self, position: f32, cx: &mut Context<Self>) {
        crate::video::set_volume(position.clamp(0., 1.).powf(CURVE), false, cx);
        self.sync(cx);
        redraw_others(cx.entity(), cx);
        cx.notify();
    }

    fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        let (volume, muted) = crate::video::volume(cx);
        crate::video::set_volume(volume, !muted, cx);
        self.sync(cx);
        redraw_others(cx.entity(), cx);
        cx.notify();
    }

    fn hold_volume(&mut self, part: usize, hovered: bool, cx: &mut Context<Self>) {
        self.volume_hold[part] = hovered;
        if hovered {
            self.volume_open = true;
            return cx.notify();
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(LINGER).await;
            this.update(cx, |this, cx| {
                if !this.volume_hold.contains(&true) {
                    this.volume_open = false;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn begin_scrub(&mut self, fraction: f32, cx: &mut Context<Self>) {
        if matches!(self.stage, Stage::Idle) {
            self.load(false, cx);
        }
        let resume = self.is_playing();
        if let Some(playback) = self.playback() {
            playback.pause();
        }
        self.scrubbing = Some(resume);
        self.scrub(fraction, cx);
    }

    fn scrub(&mut self, fraction: f32, cx: &mut Context<Self>) {
        let Some(duration) = self.duration() else { return };
        let at = duration.mul_f32(fraction.clamp(0., 1.));
        self.resume = at;
        self.seek(at, cx);
    }

    fn end_scrub(&mut self, cx: &mut Context<Self>) {
        let Some(resume) = self.scrubbing.take() else { return };
        if resume {
            self.play(cx);
        } else {
            let at = self.position();
            self.remember(at, cx);
        }
    }

    fn key_down(&mut self, e: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(duration) = self.duration() else { return };
        let at = self.position();
        match e.keystroke.key.as_str() {
            "space" => self.toggle(cx),
            "left" => self.seek(at.saturating_sub(SKIP), cx),
            "right" => self.seek((at + SKIP).min(duration), cx),
            digit if digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit() => {
                self.seek(duration.mul_f32(f32::from(digit.as_bytes()[0] - b'0') / 10.), cx);
                if !self.is_playing() {
                    self.toggle(cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Render for AudioPlayer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::perf::probe("AudioPlayer");
        let painted = self.unpainted_since.clone();
        let body = if self.voice { self.voice_pill(window, cx) } else { self.file_card(window, cx) };
        body.child(canvas(|_, _, _| {}, move |_, _, _, _| painted.set(None)).absolute().size_0())
    }
}


fn paint_waveform(bounds: Bounds<Pixels>, levels: &[f32], played: f32, rest: Hsla, heard: Hsla, window: &mut Window) {
    let split = bounds.size.width * played;
    for (i, level) in levels.iter().enumerate() {
        let height = (22. * level + 2.).max(2.);
        let x = px(i as f32 * PITCH);
        let bar = |from: Pixels, width: Pixels| Bounds::new(point(bounds.left() + from, bounds.top() + px((WAVE_HEIGHT - height) / 2.)), size(width, px(height)));
        window.paint_quad(fill(bar(x, px(BAR)), rest).corner_radii(ui::HAIRLINE));
        let heard_width = (split - x).clamp(Pixels::ZERO, px(BAR));
        if heard_width > Pixels::ZERO {
            window.paint_quad(fill(bar(x, heard_width), heard).corner_radii(ui::HAIRLINE));
        }
    }
}

fn tabular() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".into(), 1)]))
}

fn heard(cx: &App) -> f32 {
    let (volume, muted) = crate::video::volume(cx);
    if muted { 0. } else { volume }
}

fn redraw_others(me: Entity<AudioPlayer>, cx: &mut App) {
    let others: Vec<_> = cx.try_global::<Players>().into_iter().flat_map(|p| p.players.values()).filter(|p| **p != me).cloned().collect();
    for player in others {
        player.update(cx, |_, cx| cx.notify());
    }
}

// Discord's waveform width for a clip this long, snapped to whole bars.
fn wave_width(secs: f32) -> f32 {
    let width = if secs <= 0.5 { 40. } else if secs >= 45. { 294. } else { (secs - 0.5) / 44.5 * 254. + 40. };
    PITCH * ((width + 4.) / PITCH).floor() - 4.
}

// Each bar the average of its share of the samples; short waveforms are padded with silence.
fn resample(samples: &[u8], bars: usize) -> Vec<f32> {
    let level = |s: &[u8]| s.iter().map(|b| f32::from(*b)).sum::<f32>() / s.len().max(1) as f32 / 255.;
    if samples.len() <= bars {
        return samples.iter().map(|b| f32::from(*b) / 255.).chain(std::iter::repeat(0.)).take(bars).collect();
    }
    let edge = |k: usize| ((k * samples.len()) as f32 / bars as f32).round() as usize;
    (0..bars).map(|k| level(&samples[edge(k)..edge(k + 1)])).collect()
}

// The demo's voice message left part-way through.
#[cfg(debug_assertions)]
pub fn demo_spot(prefs: &mut Prefs, id: &str, secs: f32) {
    set_spot(prefs, id, Some(Duration::from_secs_f32(secs)));
}

fn saved_spot(prefs: &Prefs, id: &str) -> Option<Duration> {
    prefs.voice_resume.iter().find(|(k, _)| k == id).map(|(_, at)| Duration::from_secs_f32(*at))
}

fn set_spot(prefs: &mut Prefs, id: &str, at: Option<Duration>) {
    if id.is_empty() {
        return;
    }
    prefs.voice_resume.retain(|(k, _)| k != id);
    let Some(at) = at else { return };
    prefs.voice_resume.push((id.to_owned(), at.as_secs_f32()));
    let over = prefs.voice_resume.len().saturating_sub(RESUME_KEPT);
    prefs.voice_resume.drain(..over);
}

async fn open(url: String) -> Result<AudioPlayback, String> {
    let bytes = match crate::discord::demo_media(&url) {
        Some(bytes) => bytes.to_vec(),
        None => download(&url).await?,
    };
    tokio::task::spawn_blocking(move || AudioPlayback::open(audio::decode(&bytes)?)).await.map_err(|e| e.to_string())?
}

async fn download(url: &str) -> Result<Vec<u8>, String> {
    let response = reqwest::get(url).await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    if response.content_length().is_some_and(|len| len > MAX_BYTES) {
        return Err("Too large to play here".into());
    }
    Ok(response.bytes().await.map_err(|e| e.to_string())?.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waveform_width_follows_length_on_the_bar_pitch() {
        assert_eq!(wave_width(0.2), 38.);
        assert_eq!(wave_width(60.), 290.);
        assert_eq!((wave_width(20.) + 4.) % PITCH, 0.);
    }

    #[test]
    fn resample_averages_down_and_pads_up() {
        assert_eq!(resample(&[255, 0, 255, 0], 2), vec![0.5, 0.5]);
        assert_eq!(resample(&[255], 3), vec![1., 0., 0.]);
        assert_eq!(resample(&[], 2), vec![0., 0.]);
    }

    #[test]
    fn keeps_the_last_25_spots() {
        let mut prefs = Prefs::default();
        for i in 0..30 {
            set_spot(&mut prefs, &i.to_string(), Some(Duration::from_secs(2)));
        }
        assert_eq!(prefs.voice_resume.len(), RESUME_KEPT);
        assert_eq!(saved_spot(&prefs, "29"), Some(Duration::from_secs(2)));
        assert_eq!(saved_spot(&prefs, "0"), None);
        set_spot(&mut prefs, "29", None);
        assert_eq!(saved_spot(&prefs, "29"), None);
    }
}
