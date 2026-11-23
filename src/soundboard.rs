// Discord's soundboard in a call (docs/discord-soundboard-re.md, design/explore-voice-extras.html §3-4): the
// picker that plays a sound to everyone, hearing the sounds others play, and the emoji that rises on the tile
// of whoever played one.
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    time::{Duration, Instant},
};

use gpui_kit::{
    Anchor, Animation, AnimationExt, AnyElement, App, Bounds, Context, Entity, FontWeight, Hsla, MouseButton, Pixels, Point, ScrollHandle, SharedString, Task, Window, canvas,
    component::{Icon, input::InputEvent},
    div, img, linear_color_stop, linear_gradient, point, prelude::*, px,
};

use crate::{
    SearchState, Waffle,
    discord::{Emoji, SoundboardSound, VoiceEffect, perm},
    soundboard_fx::{self, Family},
    icons::Glyph,
    overlays::backdrop,
    palette,
    settings::Fill,
    ui::{self, RADIUS_SM, Squircle},
};

mod picker;

use self::picker::*;

// The category rail's width.
const RAIL_WIDTH: f32 = 52.;
// The volume card hangs this far from the top.
const VOLUME_TOP: f32 = 56.;
// The volume card's width.
const VOLUME_WIDTH: f32 = 240.;
// The buttons over a hovered sound.
const TOOL_SIZE: f32 = 22.;
// Where the demo scene opens the picker.
const DEMO_AT: f32 = 640.;

const FAVORITES_LIMIT: usize = 250;
const FREQUENT_COUNT: usize = 6;
const PICKER_SIZE: (f32, f32) = (452., 420.);
// The tile burst (design §3): six copies, 90ms apart, each rising 220px over 1.8s.
// The playing label and the soundboard ring stay at least this long, past a short effect.
const LABEL_LIFE: Duration = Duration::from_millis(2400);
const MAX_EFFECTS: usize = 50;
// How long a sound counts as playing when its file didn't say.
const FALLBACK_LENGTH: Duration = Duration::from_secs(3);

pub const NITRO_LOCKED: &str = "Use custom soundboard sounds across voice channels. Only with Nitro.";

// What the soundboard knows for this session: every server's sounds and Discord's own, the files fetched so
// far, what's playing, the effects on tiles, and the picker while it's open.
pub struct Soundboard {
    pub(crate) picker: Option<Picker>,
    pub(crate) search: Entity<SearchState>,
    // lint-allow(unbounded-growth): one entry per joined server, a few dozen at most
    guild_sounds: HashMap<String, Vec<SoundboardSound>>,
    defaults: Vec<SoundboardSound>,
    defaults_requested: bool,
    // lint-allow(unbounded-growth): one id per joined server, a few dozen at most
    requested: HashSet<String>,
    // lint-allow(unbounded-growth): the files of sounds played this session, a few dozen at most, each a few seconds long
    clips: HashMap<String, Clip>,
    // Sound id to when it stops playing.
    playing: HashMap<String, Instant>,
    effects: Vec<TileEffect>,
    next_id: u64,
    // Plays on their way to Discord; dropping one cancels it, which leaving the call does.
    sending: HashMap<u64, Task<()>>,
    // Where each soundboard control was last painted, so the picker opens above the one used.
    anchors: Rc<RefCell<HashMap<&'static str, Bounds<Pixels>>>>,
}

pub(crate) struct Picker {
    from: Option<&'static str>,
    // Where it opens when no control is named (the shortcut opens it at the pointer, as Discord's).
    at: Point<Pixels>,
    volume_open: bool,
    over_limit: bool,
    scroll: ScrollHandle,
}

#[derive(Clone)]
struct Clip {
    bytes: bytes::Bytes,
    length: Duration,
}

struct TileEffect {
    id: u64,
    user_id: String,
    emoji: Option<Emoji>,
    family: Family,
    // The sound's name while we hear it play.
    playing: Option<String>,
}

// Whether you can play sounds in the call, as the voice tray's button says it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Access {
    // No Use Soundboard or Speak here: Discord hides the button.
    Hidden,
    Off(&'static str),
    Ready,
}

impl Access {
    pub fn tip(self) -> &'static str {
        match self {
            Access::Off(why) => why,
            _ => "Open Soundboard",
        }
    }
}

impl Soundboard {
    pub fn new(window: &mut Window, cx: &mut Context<Waffle>) -> Self {
        let search = cx.new(|cx| SearchState::new(window, cx).placeholder("Find the perfect sound"));
        cx.subscribe_in(&search, window, |_, _, _: &InputEvent, _, cx| cx.notify()).detach(); // lint-allow(detached-task): the subscription ends when the search input or Waffle is dropped
        Soundboard {
            picker: None,
            search,
            guild_sounds: HashMap::new(),
            defaults: Vec::new(),
            defaults_requested: false,
            requested: HashSet::new(),
            clips: HashMap::new(),
            playing: HashMap::new(),
            effects: Vec::new(),
            next_id: 0,
            sending: HashMap::new(),
            anchors: Rc::default(),
        }
    }

    pub fn store_sounds(&mut self, guild_id: String, sounds: Vec<SoundboardSound>, all: bool) {
        let list = self.guild_sounds.entry(guild_id).or_default();
        if all {
            list.clear();
        }
        for sound in sounds {
            match list.iter_mut().find(|s| s.id == sound.id) {
                Some(old) => *old = sound,
                None => list.push(sound),
            }
        }
    }

    pub fn remove_sound(&mut self, guild_id: &str, sound_id: &str) {
        if let Some(list) = self.guild_sounds.get_mut(guild_id) {
            list.retain(|s| s.id != sound_id);
        }
    }

    pub fn close_picker(&mut self) -> bool {
        self.picker.take().is_some()
    }

    pub fn stop_playing(&mut self) {
        crate::sounds::stop_clips();
        self.playing.clear();
        for effect in &mut self.effects {
            effect.playing = None;
        }
    }

    // Leaving or switching channel: nothing keeps playing, and plays not yet sent are dropped.
    pub fn leave_call(&mut self) {
        self.stop_playing();
        self.sending.clear();
        self.effects.clear();
        self.picker = None;
    }

    fn sound(&self, id: &str) -> Option<&SoundboardSound> {
        self.defaults.iter().chain(self.guild_sounds.values().flatten()).find(|s| s.id == id)
    }

    fn is_playing(&self, sound_id: &str) -> bool {
        self.playing.get(sound_id).is_some_and(|until| *until > Instant::now())
    }

    fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    // A painted control's place, for the picker to open over.
    fn probe(&self, key: &'static str) -> impl IntoElement {
        let anchors = self.anchors.clone();
        canvas(move |bounds, _, _| _ = anchors.borrow_mut().insert(key, bounds), |_, _, _, _| {}).absolute().size_full()
    }
}

// Discord's soundboard volume: stored as amplitude, shown (and played at) its perceptual value, both 0..=100
// for the slider's range (`M` and `w` in its client).
pub fn perceptual(amplitude: f32) -> f32 {
    if amplitude < 100. { 100. * (amplitude.max(0.) / 100.).powf(1. / 2.8) } else { 100. * (20. * (amplitude / 100.).log10() / 6. + 1.) }
}

pub fn amplitude(perceptual: f32) -> f32 {
    100. * (perceptual.clamp(0., 100.) / 100.).powf(2.8)
}

pub fn slider_fraction(amplitude: f32) -> f32 {
    (perceptual(amplitude) / 100.).clamp(0., 1.)
}

// The gain a sound plays at: its own volume, the soundboard setting, and output volume up to 100%.
pub fn playback_volume(sound_volume: f32, soundboard_volume: f32, output_volume: f32) -> f32 {
    (sound_volume * perceptual(soundboard_volume) / 100. * output_volume.min(1.)).min(1.)
}

// Discord's rule for one sound in a channel: its own server's and Discord's sounds always; another server's
// with Nitro, where Use External Sounds is allowed (always in DMs).
pub fn can_use(sound: &SoundboardSound, channel_guild: Option<&str>, nitro: bool, external: bool) -> bool {
    let home = sound.is_default() || channel_guild == Some(sound.guild_id.as_str());
    sound.available && (home || (nitro && (channel_guild.is_none() || external)))
}

impl Waffle {
    pub(crate) fn soundboard_access(&self) -> Access {
        let Some(call) = &self.call else { return Access::Hidden };
        let me = self.me.as_ref().map(|m| m.id.as_str()).unwrap_or_default();
        let guild = call.guild_id.as_ref().and_then(|id| self.guilds.iter().find(|g| &g.id == id));
        if let Some(guild) = guild {
            let needed = perm::USE_SOUNDBOARD | perm::SPEAK;
            if guild.channel_permissions(me, &call.channel) & needed != needed {
                return Access::Hidden;
            }
            if guild.voice_states.iter().any(|v| v.user_id == me && v.server_muted) {
                return Access::Off("Soundboard disabled when server muted");
            }
        }
        // GAP: "Soundboard disabled when suppressed" needs the voice state's `suppress`, which VoiceState
        // doesn't keep yet (it only matters on stage channels, which Waffle doesn't join).
        if self.deafened {
            return Access::Off("Soundboard disabled when deafened");
        }
        Access::Ready
    }

    // Not verified: that Nitro Basic (premium type 3) lacks soundboard-everywhere; only full Nitro (2) gets it here.
    fn has_nitro_soundboard(&self) -> bool {
        self.my_account.premium_type == 2
    }

    fn sound_usable(&self, sound: &SoundboardSound) -> bool {
        let Some(call) = &self.call else { return false };
        let me = self.me.as_ref().map(|m| m.id.as_str()).unwrap_or_default();
        let guild = call.guild_id.as_ref().and_then(|id| self.guilds.iter().find(|g| &g.id == id));
        let external = guild.is_some_and(|g| g.channel_permissions(me, &call.channel) & perm::USE_EXTERNAL_SOUNDS != 0);
        can_use(sound, call.guild_id.as_deref(), self.has_nitro_soundboard(), external)
    }

    fn sound_gain(&self, sound_volume: f32) -> f32 {
        playback_volume(sound_volume, self.prefs.soundboard_volume, self.prefs.output_volume)
    }

    // Each server's soundboard over the gateway, once per session, and Discord's own sounds once, as its
    // client does when the picker opens and when voice connects.
    pub(crate) fn fetch_soundboard(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let wanted: Vec<String> = self.guilds.iter().map(|g| g.id.clone()).filter(|id| !self.soundboard.requested.contains(id)).collect();
        if !wanted.is_empty() {
            client.request_soundboard_sounds(&wanted);
            self.soundboard.requested.extend(wanted);
        }
        if std::mem::replace(&mut self.soundboard.defaults_requested, true) {
            return;
        }
        cx.spawn(async move |this, cx| {
            let result = client.default_soundboard_sounds().await;
            this.update(cx, |this, cx| match result {
                Ok(sounds) => {
                    this.soundboard.defaults = sounds;
                    cx.notify(); // lint-allow(global-notify): the soundboard picker is drawn by the root's overlays, over every column
                }
                // Asked again next time the picker opens.
                Err(_) => this.soundboard.defaults_requested = false,
            })
            .ok();
        })
        .detach(); // lint-allow(detached-task): one fetch: ends when the request returns, or when Waffle is dropped (update fails)
    }

    pub(crate) fn toggle_soundboard_key(&mut self, _: &crate::app_keys::ToggleSoundboard, window: &mut Window, cx: &mut Context<Self>) {
        if self.soundboard.close_picker() {
            return cx.notify(); // lint-allow(global-notify): the soundboard picker is drawn by the root's overlays, over every column
        }
        self.open_soundboard(None, window, cx);
    }

    // Opens the picker over the control `from`, or at the pointer.
    pub(crate) fn open_soundboard(&mut self, from: Option<&'static str>, window: &mut Window, cx: &mut Context<Self>) {
        if self.soundboard_access() != Access::Ready {
            return;
        }
        self.dismiss_overlay(cx);
        self.soundboard.picker = Some(Picker { from, at: window.mouse_position(), volume_open: false, over_limit: false, scroll: ScrollHandle::new() });
        self.soundboard.search.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.focus(window, cx);
        });
        self.fetch_soundboard(cx);
        cx.notify(); // lint-allow(global-notify): the soundboard picker is drawn by the root's overlays, over every column
    }

    // Click: plays to everyone in the call, here at once (Discord doesn't wait for its echo), then tells Discord.
    fn play_soundboard_sound(&mut self, sound: SoundboardSound, cx: &mut Context<Self>) {
        if self.soundboard_access() != Access::Ready || !self.sound_usable(&sound) {
            return;
        }
        let (Some(call), Some(me)) = (&self.call, self.me.as_ref()) else { return };
        let (channel_id, me) = (call.channel.id.clone(), me.id.clone());
        self.play_clip(format!("{me}-{}", sound.id), sound.id.clone(), self.sound_gain(sound.volume), Some(channel_id.clone()), cx);
        self.add_tile_effect(me, sound.emoji.clone(), Family::random(), Some(sound.name.clone()), cx);
        let id = sound.id.clone();
        self.update_prefs(cx, |p| match p.soundboard_uses.iter_mut().find(|(s, _)| *s == id) {
            Some((_, n)) => *n += 1,
            None => p.soundboard_uses.push((id, 1)),
        });
        let Some(client) = self.client.clone() else { return };
        let key = self.soundboard.next_id();
        // A failed play is silent, as Discord's: it has no error UI for it.
        let send = cx.spawn(async move |this, cx| {
            let _ = client.send_soundboard_sound(&channel_id, &sound).await;
            this.update(cx, |this, _| this.soundboard.sending.remove(&key)).ok();
        });
        self.soundboard.sending.insert(key, send);
    }

    // Preview: only you hear it, and Discord isn't told.
    fn preview_sound(&mut self, sound: &SoundboardSound, cx: &mut Context<Self>) {
        self.play_clip(format!("preview-{}", sound.id), sound.id.clone(), self.sound_gain(sound.volume), None, cx);
    }

    // Someone else's sound in our channel: heard unless we're deafened or muted their soundboard (self-mute
    // doesn't stop it); their emoji rises on their tile either way.
    pub(crate) fn on_voice_effect(&mut self, effect: VoiceEffect, cx: &mut Context<Self>) {
        let Some(call) = &self.call else { return };
        if call.channel.id != effect.channel_id || self.me.as_ref().is_some_and(|m| m.id == effect.user_id) {
            return;
        }
        let heard = !self.deafened && !self.prefs.soundboard_mutes.contains(&effect.user_id);
        let sound = effect.sound_id.clone().filter(|_| heard);
        if let Some(sound_id) = &sound {
            let channel = Some(effect.channel_id.clone());
            self.play_clip(format!("{}-{sound_id}", effect.user_id), sound_id.clone(), self.sound_gain(effect.sound_volume), channel, cx);
        }
        // Discord draws the burst only for an effect that carries its animation.
        let emoji = effect.emoji.filter(|_| effect.animation_type.is_some() && effect.animation_id.is_some());
        let name = sound.and_then(|id| effect.name.clone().or_else(|| self.soundboard.sound(&id).map(|s| s.name.clone())));
        if emoji.is_some() || name.is_some() {
            self.add_tile_effect(effect.user_id, emoji, Family::of(effect.animation_type, effect.animation_id), name, cx);
        }
    }

    // Plays a sound's file, fetching it the first time; `channel` drops it if we've left that call by then.
    fn play_clip(&mut self, key: String, sound_id: String, gain: f32, channel: Option<String>, cx: &mut Context<Self>) {
        if gain <= 0. {
            return;
        }
        if let Some(clip) = self.soundboard.clips.get(&sound_id).cloned() {
            return self.start_clip(key, sound_id, clip, gain, cx);
        }
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            let Ok(bytes) = client.soundboard_file(&sound_id).await else { return };
            let measured = bytes.clone();
            let length = cx.background_executor().spawn(async move { crate::sounds::clip_length(&measured) }).await;
            this.update(cx, |this, cx| {
                let clip = Clip { bytes, length: length.unwrap_or(FALLBACK_LENGTH) };
                this.soundboard.clips.insert(sound_id.clone(), clip.clone());
                let here = this.call.as_ref().map(|c| &c.channel.id);
                if channel.is_none() || channel.as_ref() == here {
                    this.start_clip(key, sound_id, clip, gain, cx);
                }
            })
            .ok();
        })
        .detach(); // lint-allow(detached-task): one clip fetch: ends when the request returns, or when Waffle is dropped
    }

    fn start_clip(&mut self, key: String, sound_id: String, clip: Clip, gain: f32, cx: &mut Context<Self>) {
        crate::sounds::play_clip(key, clip.bytes, gain);
        self.soundboard.playing.insert(sound_id, Instant::now() + clip.length);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(clip.length).await;
            this.update(cx, |this, cx| {
                this.soundboard.playing.retain(|_, until| *until > Instant::now());
                cx.notify(); // lint-allow(global-notify): the soundboard's playing sounds and tile effects draw on the call tiles and in its picker
            })
            .ok();
        })
        .detach(); // lint-allow(detached-task): a one-shot timer: ends after the clip's length, or when Waffle is dropped
        cx.notify(); // lint-allow(global-notify): the soundboard's playing sounds and tile effects draw on the call tiles and in its picker
    }

    fn add_tile_effect(&mut self, user_id: String, emoji: Option<Emoji>, family: Family, playing: Option<String>, cx: &mut Context<Self>) {
        let id = self.soundboard.next_id();
        let life = family.duration().max(LABEL_LIFE);
        let effects = &mut self.soundboard.effects;
        effects.push(TileEffect { id, user_id, emoji, family, playing });
        let over = effects.len().saturating_sub(MAX_EFFECTS);
        effects.drain(..over);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(life).await;
            this.update(cx, |this, cx| {
                this.soundboard.effects.retain(|e| e.id != id);
                cx.notify(); // lint-allow(global-notify): the soundboard's playing sounds and tile effects draw on the call tiles and in its picker
            })
            .ok();
        })
        .detach(); // lint-allow(detached-task): a one-shot timer: ends after the effect's life, or when Waffle is dropped
        cx.notify(); // lint-allow(global-notify): the soundboard's playing sounds and tile effects draw on the call tiles and in its picker
    }

    fn toggle_favorite(&mut self, sound_id: String, cx: &mut Context<Self>) {
        let favorite = self.prefs.soundboard_favorites.contains(&sound_id);
        if !favorite && self.prefs.soundboard_favorites.len() >= FAVORITES_LIMIT {
            if let Some(picker) = &mut self.soundboard.picker {
                picker.over_limit = true;
            }
            return cx.notify(); // lint-allow(global-notify): the soundboard picker is drawn by the root's overlays, over every column
        }
        self.update_prefs(cx, |p| match favorite {
            true => p.soundboard_favorites.retain(|id| *id != sound_id),
            false => p.soundboard_favorites.push(sound_id),
        });
    }

    // The soundboard control in the call bar and the voice panel: hidden without permission, off (with
    // Discord's reason) while deafened or server muted.
    pub(crate) fn soundboard_probe(&self, key: &'static str) -> impl IntoElement {
        self.soundboard.probe(key)
    }

    pub(crate) fn soundboard_open(&self) -> bool {
        self.soundboard.picker.is_some()
    }

    // Whether a sound of theirs is playing now: their tile takes the soundboard ring.
    pub(crate) fn soundboard_playing(&self, user_id: &str) -> bool {
        self.soundboard.effects.iter().any(|e| e.user_id == user_id && e.playing.is_some())
    }

    // What rises on a person's call tile while their sound plays.
    pub(crate) fn soundboard_tile_effect(&self, user_id: &str, tile: (f32, f32), cx: &App) -> Option<AnyElement> {
        let effects: Vec<&TileEffect> = self.soundboard.effects.iter().filter(|e| e.user_id == user_id).collect();
        let latest = effects.iter().rev().find_map(|e| e.playing.clone());
        if effects.is_empty() {
            return None;
        }
        let bursts: Vec<AnyElement> = match cx.reduce_motion() {
            true => Vec::new(),
            false => effects.iter().filter_map(|e| Some(effect_scene(e.id, e.family, e.emoji.as_ref()?, tile))).collect(),
        };
        let label = latest.map(|name| {
            ui::media_label()
                .absolute()
                .top_2()
                .left_2()
                .max_w(gpui_kit::relative(0.6))
                .child(Icon::new(Glyph::Soundboard).size(ui::ICON_SM))
                .child(ui::line(name))
        });
        Some(div().absolute().inset_0().overflow_hidden().children(bursts).children(label).into_any_element())
    }

    pub(crate) fn soundboard_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let picker = self.soundboard.picker.as_ref()?;
        let anchored = picker.from.and_then(|key| self.soundboard.anchors.borrow().get(key).copied());
        let at = match (picker.from, anchored) {
            (_, Some(b)) => point(b.center().x, b.top() - ui::WINDOW_MARGIN),
            // The control hasn't painted yet: place it once it has.
            (Some(_), None) => {
                window.request_animation_frame();
                picker.at
            }
            (None, None) => picker.at,
        };
        let card = ui::surface(cx)
            .p_0()
            .gap_0()
            .id("soundboard")
            .occlude()
            .w(px(PICKER_SIZE.0))
            .h(px(PICKER_SIZE.1))
            .relative()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(self.picker_content(picker, cx));
        let card = ui::rise(card.transform_origin(gpui_kit::point(gpui_kit::relative(0.5), gpui_kit::relative(1.))), "soundboard", Duration::ZERO);
        Some(backdrop("soundboard-backdrop", cx).child(ui::floating_at("soundboard-float", at, Anchor::BottomCenter, card)).into_any_element())
    }
}

// sized and placed against the tile so it reads the same on a small card and a focused one.
fn effect_scene(effect: u64, family: Family, emoji: &Emoji, (width, height): (f32, f32)) -> AnyElement {
    let total = family.duration();
    let copies = (0..family.copies()).map(|i| {
        let size = height * 0.34;
        ui::row()
            .absolute()
            .left_0()
            .top_0()
            .size(px(size))
            .justify_center()
            .child(emoji_face(Some(emoji), size * 0.9))
            .with_animation(("soundboard-fx", (effect as usize) * 64 + i), Animation::new(total), move |el, t| {
                let Some(p) = soundboard_fx::particle(family, i, t * total.as_secs_f32(), effect) else { return el.opacity(0.) };
                let scale = p.size / 0.34;
                let mut el = el
                    .translate(point(px(p.x * width - size / 2.), px(p.y * height - size / 2.)))
                    .rotate(gpui_kit::Radians(p.rotate.to_radians()))
                    .opacity(p.opacity);
                el.style().scale = gpui_kit::SizeRefinement { width: Some(scale * p.scale_x), height: Some(scale * p.scale_y) };
                el
            })
            .into_any_element()
    });
    div().absolute().inset_0().children(copies.collect::<Vec<_>>()).into_any_element()
}

// Made-up soundboard for the demo scenes: Night Owls' sounds, gpui's (locked without Nitro), Discord's own.
pub fn demo(waffle: &mut Waffle, open: bool, window: &mut Window, cx: &mut Context<Waffle>) {
    let sound = |id: &str, guild: &str, name: &str, emoji: &str| SoundboardSound { id: id.into(), name: name.into(), guild_id: guild.into(), volume: 1., emoji: Some(Emoji::unicode(emoji)), available: true };
    let board = &mut waffle.soundboard;
    board.store_sounds("1".into(), vec![sound("101", "1", "airhorn", "📯"), sound("102", "1", "mira laughing extremely loudly", "😂"), sound("103", "1", "sad trombone", "🎺"), sound("104", "1", "bruh", "🚨")], true);
    board.store_sounds("2".into(), vec![sound("201", "2", "yippee", "🎉"), sound("202", "2", "vine boom", "💥")], true);
    board.defaults = vec![sound("1", "0", "quack", "🦆"), sound("2", "0", "airhorn", "🔊"), sound("3", "0", "cricket", "🦗"), sound("4", "0", "golf clap", "👏"), sound("5", "0", "sad horn", "👻"), sound("6", "0", "ba dum tss", "🥁")];
    board.defaults_requested = true;
    waffle.prefs.soundboard_favorites = vec!["1".into(), "6".into()];
    waffle.prefs.soundboard_uses = vec![("101".into(), 9), ("103".into(), 4)];
    waffle.soundboard.playing.insert("1".into(), Instant::now() + Duration::from_secs(3600));
    if open {
        waffle.soundboard.picker = Some(Picker { from: Some(CALL_CONTROL), at: point(px(DEMO_AT), px(DEMO_AT)), volume_open: false, over_limit: false, scroll: ScrollHandle::new() });
        waffle.soundboard.search.update(cx, |state, cx| state.focus(window, cx));
    } else {
        // Each family in turn, on a loop, so the scene shows them all.
        cx.spawn(async move |this, cx| {
            for family in [Family::Launch, Family::Parade, Family::Glide, Family::Pop, Family::Rain].into_iter().cycle() {
                let alive = this.update(cx, |this, cx| this.add_tile_effect("8388608".into(), Some(Emoji::unicode("🦆")), family, Some("quack".into()), cx)).is_ok();
                if !alive {
                    return;
                }
                cx.background_executor().timer(family.duration() + Duration::from_millis(500)).await;
            }
        })
        .detach(); // lint-allow(detached-task): the demo loop ends when Waffle is dropped (update fails)
    }
}

// The probe keys for the controls that open the picker.
pub const CALL_CONTROL: &str = "call";
pub const PANEL_CONTROL: &str = "panel";

#[cfg(test)]
mod tests {
    use super::*;

    fn sound(guild: &str) -> SoundboardSound {
        SoundboardSound { id: "1".into(), name: "quack".into(), guild_id: guild.into(), volume: 1., emoji: None, available: true }
    }

    #[test]
    fn volume_scale_round_trips_and_defaults_to_full() {
        assert_eq!(perceptual(100.), 100.);
        assert_eq!(perceptual(0.), 0.);
        for p in [5., 25., 50., 80.] {
            assert!((perceptual(amplitude(p)) - p).abs() < 0.01, "{p}");
        }
        // Discord's curve: half the slider is about 14% amplitude.
        assert!((amplitude(50.) - 14.36).abs() < 0.01);
    }

    #[test]
    fn playback_volume_follows_discords_formula() {
        assert_eq!(playback_volume(1., 100., 1.), 1.);
        assert!((playback_volume(0.5, 100., 1.) - 0.5).abs() < 1e-6);
        assert!((playback_volume(1., amplitude(50.), 1.) - 0.5).abs() < 1e-4, "the setting plays at its perceptual value");
        assert!((playback_volume(1., 100., 0.4) - 0.4).abs() < 1e-6, "output volume under 100% lowers it");
        assert_eq!(playback_volume(1., 100., 2.), 1., "output over 100% doesn't raise it");
        assert_eq!(playback_volume(0.8, 400., 1.), 1., "capped at 1");
        assert_eq!(playback_volume(1., 0., 1.), 0., "0 is the soundboard muted");
    }

    #[test]
    fn other_servers_sounds_need_nitro_and_external_sounds() {
        let (home, other, default) = (sound("9"), sound("7"), sound("0"));
        assert!(can_use(&home, Some("9"), false, false));
        assert!(can_use(&default, Some("9"), false, false));
        assert!(!can_use(&other, Some("9"), false, true), "no Nitro");
        assert!(!can_use(&other, Some("9"), true, false), "Use External Sounds denied");
        assert!(can_use(&other, Some("9"), true, true));
        assert!(can_use(&other, None, true, false), "a DM call needs only Nitro");
        assert!(!can_use(&SoundboardSound { available: false, ..home }, Some("9"), true, true), "unavailable");
    }
}
