// Everything around a voice call (design/voice.html): the voice panel over the user panel with its
// connection stages, the connection details popover, the call stage with its tiles, header and
// control bar, device menus and the participant menu.
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, Context, Div, ElementId, FontFeatures, FontWeight, Hsla, ImageSource, MouseButton, MouseDownEvent, Pixels, Point, SharedString, Stateful, Window,
    component::{ActiveTheme, Icon, tooltip::Tooltip},
    div, prelude::*, px, relative, rgb,
};

use crate::{
    Waffle,
    discord::{self, User, VoiceState},
    icons::Glyph,
    layout::{HEADER_HEIGHT, RAIL_WIDTH},
    mascot,
    menu::{Entry, item, submenu},
    palette::{self, stage},
    settings::Page,
    soundboard,
    ui::{self, RADIUS_LG, RADIUS_MD, RADIUS_SM, RADIUS_XS, Squircle},
    voice::{self, Stage},
};

// .voicebar: 10px padding, a 38px status block (a 20px line over an 18px one), 8px gap, 32px
// buttons, and its 1px hairline.
pub const VOICE_HEIGHT: f32 = 99.;
pub(crate) const PANEL_PAD: f32 = 10.;
// The error bar is the voicebar without its buttons.
const ERROR_HEIGHT: f32 = 59.;
const PREVIEW_AGE: std::time::Duration = std::time::Duration::from_secs(60);
// After a failed preview fetch, how long until the next try.
const PREVIEW_RETRY: std::time::Duration = std::time::Duration::from_secs(20);
// The stage (.call): the 48px header line (FOCUS_HEAD) over a 32px header row, then 16px padding and gaps
// around the tiles and 48px controls.
// Longer than the 500 ms a still screen is repeated at, so a still screen never reads as stalled.
// The stage redraws once a second during a call, so the loader shows within a second of this.
const STALL_AFTER: Duration = Duration::from_secs(3);
const STAGE_PAD: f32 = 16.;
const STAGE_GAP: f32 = 16.;
const HEAD_HEIGHT: f32 = 32.;
const CONTROLS_HEIGHT: f32 = 48.;
// A group's inset around its buttons.
const DOCK_PAD: f32 = 4.;
// .tiles: 10px apart, 8px once the grid is three across; avatars shrink in small tiles.
const TILE_GAP: f32 = 10.;
const TILE_GAP_DENSE: f32 = 8.;
const SMALL_TILE: f32 = 240.;
// .filmstrip under a focused stream.
const STRIP_TILE: f32 = 148.;
const STRIP_GAP: f32 = 8.;
// The filmstrip's fold button.
const STRIP_TOGGLE: f32 = 28.;
const STRIP_LIMIT: usize = 12;
// Names on strip cards and the focused stream show while the pointer is anywhere on the stage.
const STAGE_GROUP: &str = "call-stage";
// The stage's header band, the app's one header line.
const FOCUS_HEAD: f32 = 48.;
const CONTROLS_IDLE: Duration = Duration::from_secs(3);
// The voice channel's chat beside the call: at least 450 wide, leaving the stage 550 (Discord's numbers).
pub(crate) const CHAT_MIN_WIDTH: f32 = 450.;
const CHAT_STAGE_ROOM: f32 = 550.;
// The connection popover's colour bands (its legend): under 80 ms, 80 to 120, over 120.
const PING_FAIR: u32 = 80;
const PING_BAD: u32 = 120;
// When the panel says "Poor connection": a bad average ping, or this much packet loss.
const LOSS_POOR_PCT: f32 = 5.;
const METER_BARS: usize = 28;

// The chat's width for a window this wide: what was asked for, within Discord's bounds. A window too
// narrow for both keeps the minimum.
pub(crate) fn chat_width(wanted: f32, window: f32) -> f32 {
    wanted.min(window - CHAT_STAGE_ROOM).max(CHAT_MIN_WIDTH)
}

// The stage's tile area (left, top, width, height), between its header and controls.
fn tile_area(stage: (f32, f32)) -> (f32, f32, f32, f32) {
    let top = FOCUS_HEAD + STAGE_GAP;
    (STAGE_PAD, top, stage.0 - STAGE_PAD * 2., (stage.1 - top - CONTROLS_HEIGHT - STAGE_GAP - STAGE_PAD).max(0.))
}

#[derive(Clone)]
enum Tile {
    User(VoiceState),
    Stream(VoiceState),
    // A stream we watched that ended, kept in its place until closed (by its streamer's id).
    Ended(String),
}

// How a tile is drawn: a 16:9 card of this width, or filling the stage edge to edge.
#[derive(Clone, Copy)]
enum TileSize {
    Card(f32),
    Fill,
}

// The stage's fixed order: streams you watch or focus (and ended ones), other streams, cameras, then
// avatars. Within each group people keep the order they joined in (`people` is in join order), so
// nothing moves when someone speaks or joins. An ended stream whose streamer is live again has its
// stream back instead.
fn order_tiles(people: Vec<VoiceState>, ended: &[String], opened: impl Fn(&str) -> bool, camera: impl Fn(&VoiceState) -> bool) -> Vec<Tile> {
    let streams = people.iter().filter(|s| s.streaming).cloned().map(Tile::Stream);
    let ended = ended.iter().filter(|id| !people.iter().any(|s| s.streaming && &s.user_id == *id)).cloned().map(Tile::Ended);
    let mut tiles: Vec<Tile> = streams.chain(ended).chain(people.iter().cloned().map(Tile::User)).collect();
    tiles.sort_by_key(|tile| match tile {
        Tile::Stream(s) if opened(&s.user_id) => 0,
        Tile::Ended(_) => 0,
        Tile::Stream(_) => 1,
        Tile::User(s) if camera(s) => 2,
        Tile::User(_) => 3,
    });
    tiles
}

// A card's identity across renders, for focus.
fn tile_key(tile: &Tile) -> String {
    match tile {
        Tile::User(s) => format!("user:{}", s.user_id),
        // Shares its stream's key, so a focused stream that ends stays the big card.
        Tile::Stream(VoiceState { user_id, .. }) | Tile::Ended(user_id) => format!("stream:{user_id}"),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceKind {
    Input,
    Output,
    Camera,
}

pub struct DeviceMenu {
    kind: DeviceKind,
    at: Point<Pixels>,
    // Opened from the call controls, so it takes the stage's dark tokens.
    on_stage: bool,
    inputs: Vec<voice::Device>,
    outputs: Vec<voice::Device>,
    cameras: Vec<voice::Device>,
}

impl DeviceMenu {
    /// Whether this is `kind`'s popover opened from the user area, not from the call stage's controls.
    pub fn opened_from_user_area(&self, kind: DeviceKind) -> bool {
        !self.on_stage && self.kind == kind
    }
}

// Where the connection stands, as the voice panel words and colours it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Link {
    Connecting,
    Connected,
    Poor,
    Lost,
}

impl Link {
    fn of(stage: Stage, stats: Option<&voice::Stats>) -> Self {
        match stage {
            Stage::Connected if stats.is_some_and(|s| s.median_ping_ms > PING_BAD || loss(s) >= LOSS_POOR_PCT) => Link::Poor,
            Stage::Connected => Link::Connected,
            Stage::NoRoute | Stage::Disconnected => Link::Lost,
            _ => Link::Connecting,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Link::Connecting => "Connecting…",
            Link::Connected => "Voice connected",
            Link::Poor => "Poor connection",
            Link::Lost => "Reconnecting…",
        }
    }

    // .voicebar's --tone: muted while it connects, green when up, amber when something needs you.
    fn tone(self, cx: &App) -> Hsla {
        match self {
            Link::Connecting => palette::muted(cx),
            Link::Connected => palette::positive_ink(cx),
            Link::Poor | Link::Lost => palette::warning_ink(cx),
        }
    }
}

fn loss(stats: &voice::Stats) -> f32 {
    stats.outbound_loss_pct.unwrap_or(0.)
}

// A ping's band in the connection graph.
fn ping_color(ping: u32, cx: &App) -> Hsla {
    match ping {
        p if p < PING_FAIR => palette::status_in(discord::Status::Online, cx),
        p if p <= PING_BAD => palette::status_in(discord::Status::Idle, cx),
        _ => palette::status_in(discord::Status::Dnd, cx),
    }
}

fn quality_word(ping: u32) -> &'static str {
    match ping {
        p if p < PING_FAIR => "Good",
        p if p <= PING_BAD => "Fair",
        _ => "Poor",
    }
}

impl Waffle {
    // Discord's stream thumbnails (GET /streams/{key}/preview) for everyone live in the open server
    // and our call, refetched when older than a minute.
    pub(crate) fn refresh_stream_previews(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let wanted: Vec<String> = [self.guild_id.clone(), self.call.as_ref().and_then(|c| c.guild_id.clone())].into_iter().flatten().collect();
        let mut keys: Vec<(String, String)> = Vec::new();
        for guild in self.guilds.iter().filter(|g| wanted.contains(&g.id)) {
            for state in guild.voice_states.iter().filter(|v| v.streaming) {
                let Some(channel) = &state.channel_id else { continue };
                let fresh = self.stream_previews.get(&state.user_id).is_some_and(|(_, at)| at.elapsed() < PREVIEW_AGE);
                if !fresh {
                    keys.push((state.user_id.clone(), format!("guild:{}:{channel}:{}", guild.id, state.user_id)));
                }
            }
        }
        for (user_id, key) in keys {
            // Marked fetched now so a burst of events doesn't fire duplicate requests.
            let old = self.stream_previews.get(&user_id).map(|(url, _)| url.clone()).unwrap_or_default();
            self.stream_previews.insert(user_id.clone(), (old, std::time::Instant::now()));
            let client = client.clone();
            cx.spawn(async move |this, cx| {
                let result = client.call(reqwest::Method::GET, format!("/streams/{key}/preview"), None).await;
                if cfg!(debug_assertions) && std::env::var_os("WAFFLE_STREAM_DEBUG").is_some() {
                    eprintln!("stream preview {key}: {:?}", result.as_ref().map(|b| b.to_string()));
                }
                let url = match &result {
                    Ok(body) => body["url"].as_str().map(str::to_owned),
                    Err(_) => None,
                };
                let Some(url) = url else {
                    // Often just too early: the streamer uploads its first preview a few seconds
                    // after going live. Ask again soon instead of waiting for the next voice event.
                    cx.background_executor().timer(PREVIEW_RETRY).await;
                    this.update(cx, |this, cx| {
                        this.expire_stream_preview(&user_id);
                        this.refresh_stream_previews(cx);
                    })
                    .ok();
                    return;
                };
                if std::env::var_os("WAFFLE_STREAM_DEBUG").is_some() {
                    eprintln!("stream preview fetched for {key}");
                }
                this.update(cx, |this, cx| {
                    this.stream_previews.insert(user_id, (url, std::time::Instant::now()));
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    // Lets the next refresh fetch this streamer's preview again, keeping the picture shown so far.
    pub(crate) fn expire_stream_preview(&mut self, user_id: &str) {
        if let Some((_, at)) = self.stream_previews.get_mut(user_id)
            && let Some(old) = std::time::Instant::now().checked_sub(PREVIEW_AGE)
        {
            *at = old;
        }
    }

    // Tiles without a camera take their avatar's colour, as Discord's do; fetched once per person.
    pub(crate) fn refresh_avatar_colors(&mut self, cx: &mut Context<Self>) {
        if self.call.is_none() {
            return;
        }
        let wanted: Vec<(String, String)> = self.participants().into_iter().filter(|p| !self.avatar_colors.contains_key(&p.user_id)).map(|p| (p.user_id.clone(), p.user.map(|u| u.avatar).unwrap_or_else(|| self.user_avatar(&p.user_id)))).collect();
        for (user_id, avatar) in wanted {
            self.avatar_colors.insert(user_id.clone(), None);
            let url = small_png(&avatar);
            cx.spawn(async move |this, cx| {
                let color = discord::runtime().spawn(async move { avatar_color(&url).await }).await.ok().flatten();
                this.update(cx, |this, cx| {
                    this.avatar_colors.insert(user_id, color);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    // What everyone sees of our Go Live before watching, uploaded as Discord's own client does:
    // a 512px-wide JPEG shortly after going live, then every five minutes (a minute after a failure).
    pub(crate) fn upload_stream_previews(&mut self, source: voice::ShareSource, cx: &mut Context<Self>) {
        let (Some(client), Some(call), Some(me)) = (self.client.clone(), self.call.as_ref(), self.me.as_ref()) else { return };
        let key = match &call.guild_id {
            Some(guild) => format!("guild:{guild}:{}:{}", call.channel.id, me.id),
            None => format!("call:{}:{}", call.channel.id, me.id),
        };
        self.preview_generation += 1;
        let generation = self.preview_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(3)).await;
            loop {
                let live = this.update(cx, |this, _| this.preview_generation == generation && this.call.as_ref().is_some_and(|c| c.sharing.is_some()));
                if !live.unwrap_or(false) {
                    return;
                }
                let thumbnail = cx.background_spawn(async move { voice::thumbnail(source, 512).ok().and_then(|frame| jpeg_data_url(&frame)) }).await;
                let sent = match thumbnail {
                    Some(url) => {
                        let result = client.call(reqwest::Method::POST, format!("/streams/{key}/preview"), Some(serde_json::json!({ "thumbnail": url }))).await;
                        if let Err(e) = &result {
                            eprintln!("stream preview upload {key} failed: {e}");
                        } else {
                            if std::env::var_os("WAFFLE_STREAM_DEBUG").is_some() {
                                eprintln!("stream preview uploaded for {key}");
                            }
                            // Show our own new preview right away rather than at the next refresh.
                            let _ = this.update(cx, |this, cx| {
                                if let Some(me) = this.me.as_ref().map(|m| m.id.clone()) {
                                    this.expire_stream_preview(&me);
                                }
                                this.refresh_stream_previews(cx);
                            });
                        }
                        result.is_ok()
                    }
                    None => {
                        eprintln!("stream preview: couldn't grab a still of {source:?}");
                        false
                    }
                };
                cx.background_executor().timer(Duration::from_secs(if sent { 300 } else { 60 })).await;
            }
        })
        .detach();
    }

    pub(crate) fn stream_preview(&self, user_id: &str) -> Option<String> {
        self.stream_previews.get(user_id).map(|(url, _)| url.clone()).filter(|url| !url.is_empty())
    }

    // In-call warnings: a mic that sends nothing, and Advanced suppression that had to fall back to
    // RNNoise because this computer can't keep up with it.
    fn voice_warning(&self) -> Option<&'static str> {
        let call = self.call.as_ref()?;
        let silent = self.prefs.silence_warning && !self.muted && !self.deafened && call.session.input_detected() == Some(false);
        match (silent, call.session.noise_suppression_degraded()) {
            (true, _) => Some("No audio detected from your mic"),
            (false, true) => Some("Advanced noise suppression fell back to Standard"),
            _ => None,
        }
    }

    pub(crate) fn voice_panel_height(&self) -> f32 {
        match (&self.call, &self.voice_error) {
            (Some(_), _) => VOICE_HEIGHT,
            (None, Some(_)) => ERROR_HEIGHT,
            (None, None) => 0.,
        }
    }

    pub(crate) fn dismiss_call_menus(&mut self) -> bool {
        std::mem::take(&mut self.connection_info) | self.device_menu.take().is_some()
    }

    // .voicebar: the connection's state and where you are (or why it needs you), the one button
    // that leaves, then camera, share and activities.
    pub(crate) fn voice_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let Some(call) = &self.call else { return self.voice_error_panel(cx) };
        let link = Link::of(call.stage, call.stats.as_ref());
        let tone = link.tone(cx);
        let connected = call.stage == Stage::Connected;
        let ping = call.stats.as_ref().map(|s| s.ping_ms);
        let place = match call.guild_id.as_ref().and_then(|id| self.guilds.iter().find(|g| &g.id == id)) {
            Some(guild) => format!("{} · {}", call.channel.name, guild.name),
            None => call.channel.name.clone(),
        };
        let channel = call.channel.clone();
        let second: AnyElement = match (link, call.stats.as_ref(), self.voice_warning()) {
            (Link::Poor, Some(stats), _) => why_line(format!("{} ms ping, {:.0}% packet loss", stats.median_ping_ms, loss(stats)), tone).into_any_element(),
            (Link::Lost, _, _) => why_line("Lost the connection, retrying", tone).into_any_element(),
            (_, _, Some(warning)) => why_line(warning, palette::warning_ink(cx))
                .id("voice-warning")
                .cursor_pointer()
                .hover(|s| s.underline())
                .on_click(cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.open_settings(Page::VoiceVideo, window, cx);
                }))
                .into_any_element(),
            _ => div()
                .id("voice-place")
                .h(px(18.))
                .truncate()
                .text_size(px(12.))
                .text_color(palette::faint(cx))
                .hover(|s| s.underline())
                .child(place)
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    // A DM call lives in its DM's strip: go back to the chat, not a stage.
                    match channel.kind {
                        discord::ChannelKind::Dm => {
                            this.guild_id = None;
                            this.call_view = false;
                            this.open_channel(channel.clone(), window, cx);
                        }
                        _ => this.show_call(channel.clone(), window, cx),
                    }
                }))
                .into_any_element(),
        };
        let bars: AnyElement = match link {
            Link::Connecting | Link::Lost => ui::bars("voice-bars", tone, 12.),
            Link::Connected => ui::bars_still(tone, 12.).into_any_element(),
            Link::Poor => ui::bars_weak(tone, 12.).into_any_element(),
        };
        let tip: SharedString = match (link, ping) {
            (Link::Connected | Link::Poor, Some(ms)) => format!("{ms} ms ping, {} connection. Click for details", quality_word(ms).to_lowercase()).into(),
            _ => "Click for connection details".into(),
        };
        let hover = palette::hover(cx);
        let status = div()
            .id("voice-status")
            .flex_1()
            .min_w_0()
            .mx(px(-6.))
            .my(px(-4.))
            .px(px(6.))
            .py(px(4.))
            .squircle(RADIUS_SM)
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .when(self.connection_info, |d| d.bg(hover))
            .child(div().h(px(20.)).flex().items_center().gap(px(6.)).text_size(px(14.)).font_weight(FontWeight::MEDIUM).text_color(tone).child(bars).child(div().truncate().child(link.label())))
            .child(second)
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .on_click(cx.listener(|this, _, _, cx| {
                this.connection_info = !this.connection_info;
                cx.notify();
            }));
        let leave_tip = if link == Link::Connecting { "Cancel joining" } else { "Disconnect" };
        let top = div().flex().items_center().gap(px(8.)).child(status).child(ui::icon_button("hang-up", Glyph::CallEnd, leave_tip, cx).on_click(cx.listener(|this, _, _, cx| this.leave_voice(cx))));
        let wait = if link == Link::Lost { "Available once you're reconnected" } else { "Available once you're connected" };
        let ready = |label: &'static str| if connected { label } else { wait };
        let sharing = call.sharing.is_some();
        let camera = panel_action("panel-camera", Glyph::Video, ready(if call.camera_on { "Turn off camera" } else { "Turn on camera" }), connected, cx)
            .when(call.camera_on, |b| b.bg(palette::strong(cx)).text_color(palette::surface(palette::Level::Page, cx)))
            .when(connected, |b| b.on_click(cx.listener(|this, _, _, cx| this.toggle_camera(cx))));
        let share = match sharing {
            true => panel_action("panel-share", Glyph::ShareScreen, "Stream options", true, cx)
                .bg(palette::accent())
                .text_color(palette::on_accent())
                .hover(|s| s.bg(palette::accent_hover()))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_stream_menu(e.position, cx))),
            false => panel_action("panel-share", Glyph::ShareScreen, ready("Share your screen"), connected, cx).when(connected, |b| b.on_click(cx.listener(|this, _, _, cx| this.toggle_screen_share(cx)))),
        };
        let activities = panel_action("panel-activities", Glyph::Activities, ready("Activities"), connected, cx)
            .when(connected, |b| b.on_click(cx.listener(|this, _, _, cx| this.show_toast("Activities aren't in Waffle yet", cx))));
        let access = self.soundboard_access();
        let soundboard = (access != soundboard::Access::Hidden).then(|| {
            let on = connected && access == soundboard::Access::Ready;
            panel_action("panel-soundboard", Glyph::Soundboard, if connected { access.tip() } else { wait }, on, cx)
                .relative()
                .when(self.soundboard_open(), |b| b.bg(palette::press(cx)))
                .child(self.soundboard_probe(soundboard::PANEL_CONTROL))
                .when(on, |b| b.on_click(cx.listener(|this, _, window, cx| this.open_soundboard(Some(soundboard::PANEL_CONTROL), window, cx))))
        });
        let voice = div()
            .h(px(VOICE_HEIGHT))
            .flex_none()
            .p(px(PANEL_PAD))
            .flex()
            .flex_col()
            .gap(px(8.))
            .rounded_t(px(RADIUS_MD)).corner_shape(ui::SQUIRCLE)
            .relative()
            .child(ui::inset_rule(PANEL_PAD, cx))
            .child(top)
            .child(div().flex().gap(px(6.)).child(camera).child(share).children(soundboard).child(activities));
        Some(voice.into_any_element())
    }

    // The voicebar's danger state once a call is gone: what happened, and a way to dismiss it.
    fn voice_error_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let error = self.voice_error.clone()?;
        let tone = palette::danger_ink(cx);
        Some(
            div()
                .h(px(ERROR_HEIGHT))
                .p(px(PANEL_PAD))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded_t(px(RADIUS_MD)).corner_shape(ui::SQUIRCLE)
                .relative()
                .child(ui::inset_rule(PANEL_PAD, cx))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(div().h(px(20.)).flex().items_center().text_size(px(14.)).font_weight(FontWeight::MEDIUM).text_color(tone).child("Voice disconnected"))
                        .child(why_line(error, tone)),
                )
                .child(ui::icon_button("dismiss-voice", Glyph::X, "Dismiss", cx).on_click(cx.listener(|this, _, _, cx| {
                    this.voice_error = None;
                    cx.notify();
                })))
                .into_any_element(),
        )
    }

    // The stream menu, opened from the share button while we're live.
    pub(crate) fn open_stream_menu(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.open_live_menu(at, |this| this.stream_menu(), cx);
    }

    fn stream_menu(&self) -> Vec<Entry> {
        let (res, fps, audio) = (self.prefs.stream_resolution, self.prefs.stream_fps, self.prefs.stream_audio);
        let mut quality: Vec<Entry> = [(720, "720p"), (1080, "1080p"), (1440, "1440p"), (0, "Source")]
            .into_iter()
            .map(|(value, label)| item(label, move |this, _, cx| this.set_stream_quality(Some(value), None, cx)).checked(res == value).entry())
            .collect();
        quality.push(Entry::Separator);
        quality.extend([15, 30, 60].into_iter().map(|value| item(format!("{value} FPS"), move |this, _, cx| this.set_stream_quality(None, Some(value), cx)).checked(fps == value).entry()));
        vec![
            item("Change Stream", |this, _, cx| this.open_share_picker(cx)).hint("Pick another window or screen").icon(Glyph::ShareScreen).entry(),
            submenu("Stream Quality", quality).hint(quality_label(res, fps)).entry(),
            item("Share Stream Audio", |this, _, cx| {
                this.update_prefs(cx, |p| p.stream_audio = !p.stream_audio);
                this.restart_share(cx);
            })
            .checked(audio)
            .entry(),
            Entry::Separator,
            item("Stop Streaming", |this, _, cx| this.toggle_screen_share(cx)).icon(Glyph::ScreenOff).danger().entry(),
        ]
    }

    fn set_stream_quality(&mut self, resolution: Option<u32>, fps: Option<u32>, cx: &mut Context<Self>) {
        self.update_prefs(cx, |p| {
            p.stream_resolution = resolution.unwrap_or(p.stream_resolution);
            p.stream_fps = fps.unwrap_or(p.stream_fps);
        });
        self.restart_share(cx);
    }

    pub(crate) fn toggle_noise_suppression(&mut self, cx: &mut Context<Self>) {
        self.update_prefs(cx, |p| {
            p.noise_suppression = !p.noise_suppression;
            p.input_profile = crate::settings::InputProfile::Custom;
        });
    }

    pub(crate) fn toggle_camera(&mut self, cx: &mut Context<Self>) {
        let Some(call) = &mut self.call else { return };
        let on = !call.camera_on;
        let camera = on.then(|| self.prefs.camera.clone().or_else(|| voice::cameras().into_iter().next().map(|c| c.id))).flatten();
        if on && camera.is_none() {
            self.voice_error = Some("No camera found".into());
            return cx.notify();
        }
        match call.session.set_camera(camera) {
            Ok(()) => call.camera_on = on,
            Err(error) => self.voice_error = Some(error.into()),
        }
        if !on && let Some(me) = self.me.as_ref().map(|m| m.id.clone()) && let Some(frame) = call.cameras.remove(&me) {
            cx.drop_image(frame, None);
        }
        cx.notify();
    }

    // Opening a voice channel you're in shows its call, as clicking it in Discord does.
    pub(crate) fn show_call(&mut self, channel: discord::Channel, window: &mut Window, cx: &mut Context<Self>) {
        if self.call.as_ref().is_none_or(|c| c.channel.id != channel.id) {
            self.join_voice(channel.clone(), cx);
        }
        let guild_id = self.guilds.iter().find(|g| g.channels.iter().any(|c| c.id == channel.id)).map(|g| g.id.clone());
        if let Some(guild_id) = guild_id
            && self.guild_id.as_ref() != Some(&guild_id)
        {
            self.open_guild(Some(guild_id), window, cx);
        }
        self.call_view = true;
        cx.notify();
    }

    pub(crate) fn participants(&self) -> Vec<VoiceState> {
        let Some(call) = &self.call else { return Vec::new() };
        let all = match &call.guild_id {
            Some(guild_id) => self.guilds.iter().find(|g| &g.id == guild_id).map(|g| g.voice_states.as_slice()).unwrap_or_default(),
            None => self.dm_voice.as_slice(),
        };
        let states: Vec<VoiceState> = all
            .iter()
            .filter(|v| v.channel_id.as_ref() == Some(&call.channel.id))
            .cloned()
            .collect();
        if states.is_empty()
            && let Some(me) = &self.me
        {
            return vec![VoiceState { guild_id: None, channel_id: Some(call.channel.id.clone()), user_id: me.id.clone(), user: Some(me.clone()), muted: self.muted, deafened: self.deafened, server_muted: false, server_deafened: false, streaming: false, camera: false }];
        }
        states
    }

    pub(crate) fn speaking(&self, user_id: &str) -> bool {
        let Some(call) = &self.call else { return false };
        match self.me.as_ref().is_some_and(|m| m.id == user_id) {
            true => call.level.1 && !self.muted && !self.deafened,
            false => call.speaking.contains(user_id),
        }
    }

    fn ordered_tiles(&self) -> Vec<Tile> {
        let Some(call) = &self.call else { return Vec::new() };
        let focused = self.focused_stream();
        let me = self.me.as_ref().map(|m| m.id.as_str());
        order_tiles(
            self.participants(),
            &call.ended,
            |id| call.watched(id).is_some() || focused.as_deref() == Some(id),
            |s| s.camera || call.cameras.contains_key(&s.user_id) || (me == Some(s.user_id.as_str()) && call.camera_on),
        )
    }

    // The call view, with the voice channel's chat beside it when open (the stage shrinks; the
    // chat never covers the stream).
    pub(crate) fn render_call_with_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let call_channel = self.call.as_ref()?.channel.clone();
        let chat_open = self.call_chat_shown();
        let drawer = chat_open.then(|| self.call_chat_drawer(&call_channel, window, cx));
        let call = self.render_call(window, cx)?;
        let previews = self.render_call_previews(cx);
        Some(div().size_full().flex().child(div().relative().flex_1().min_w_0().h_full().child(call).children(previews)).children(drawer).into_any_element())
    }

    fn call_chat_drawer(&mut self, channel: &discord::Channel, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let messages = self.render_messages(0., cx).into_any_element();
        let composer = self.render_composer(window, cx).into_any_element();
        div()
            .relative()
            .w(px(self.call_chat_width(window)))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .border_l_1()
            .border_color(theme.sidebar_border)
            .child(
                div()
                    .h(px(HEADER_HEIGHT))
                    .flex_none()
                    .pl_4()
                    .pr(px(8.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .window_control_area(gpui_kit::WindowControlArea::Drag)
                    .border_b_1()
                    .border_color(theme.sidebar_border)
                    .child(Icon::new(Glyph::VolumeHigh).size(px(20.)).text_color(theme.muted_foreground))
                    .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(channel.name.clone()))
                    .child(ui::icon_button("call-chat-close", Glyph::X, "Close Chat", cx).occlude().on_click(cx.listener(|this, _, _, cx| {
                        this.call_chat = false;
                        cx.notify();
                    })))
                    .child(self.caption_clearance(true)),
            )
            .child(div().relative().flex_1().min_h_0().child(messages))
            .child(composer)
            .child(self.resize_handle(crate::Edge::CallChat, cx))
            .with_animation("call-chat-in", Animation::new(Duration::from_millis(220)), |d, t| d.opacity(t.min(1.)).ml(px((1. - crate::motion::back_out(t)) * 24.)))
            .into_any_element()
    }

    // Shows or hides the voice channel's text chat beside the call, as Discord's chat button does.
    pub(crate) fn toggle_call_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.call_chat_shown() {
            self.call_chat = false;
            return cx.notify();
        }
        let Some(channel) = self.call.as_ref().map(|c| c.channel.clone()) else { return };
        self.open_call_chat(channel, window, cx);
    }

    pub(crate) fn call_chat_width(&self, window: &Window) -> f32 {
        chat_width(self.prefs.call_chat_width, f32::from(window.viewport_size().width))
    }

    // .call: the stage, dark in both themes. The header, the tiles and the control bar; a focused
    // card instead fills the stage edge to edge under the header, the controls floating over it.
    pub(crate) fn render_call(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.call.as_ref()?;
        let tiles = self.ordered_tiles();
        // .call.autohide: 260ms out, drifting 6px toward its edge; any move brings it back.
        // The chrome steps aside whenever the pointer has left the stage, and with a stream focused,
        // as Discord's does, also while it rests.
        let menus_closed = self.device_menu.is_none() && self.menu.is_none();
        let rest = match self.focused_stream() {
            Some(_) => self.call_idle(),
            None => !self.call_pointer_inside && menus_closed,
        };
        let shown = gpui_kit::base::transition("call-chrome", if rest { 0. } else { 1. }, gpui_kit::base::Transition::new(ui::DUR_PANEL), window, cx);
        let focused = self.call_focus.as_ref().and_then(|key| tiles.iter().find(|t| tile_key(t) == *key));
        if let Some(big) = focused {
            return Some(self.focus_stage(big, &tiles, shown, cx));
        }
        let viewport = window.viewport_size();
        let chat = if self.call_chat_shown() { self.call_chat_width(window) } else { 0. };
        let stage_size = match self.call_fullscreen {
            true => (f32::from(viewport.width) - chat, f32::from(viewport.height)),
            false => (f32::from(viewport.width) - RAIL_WIDTH - self.channels_width - chat, f32::from(viewport.height) - 1.),
        };
        let (_, _, width, height) = tile_area(stage_size);
        let body = self.card_stage(&tiles, (width, height), cx);
        let stage = div().flex_1().min_h_0().flex().flex_col().gap(px(STAGE_GAP)).pt(px(STAGE_GAP)).px(px(STAGE_PAD)).pb(px(STAGE_PAD));
        Some(
            self.stage_root(cx)
                .flex()
                .flex_col()
                .child(self.stage_head(cx).opacity(shown))
                .child(stage.child(div().flex_1().min_h_0().child(body)).child(div().opacity(shown).child(self.call_controls(cx))))
                .into_any_element(),
        )
    }

    // The stage's own surface, tracking the pointer for the chrome's autohide.
    fn stage_root(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("call-view")
            .group(STAGE_GROUP)
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(stage::bg())
            .text_color(stage::text())
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.call_pointer_inside = *hovered;
                if !*hovered {
                    this.call_pointer_at = Instant::now().checked_sub(CONTROLS_IDLE * 2).unwrap_or_else(Instant::now);
                }
                cx.notify();
            }))
            .on_mouse_move(cx.listener(|this, _: &gpui_kit::MouseMoveEvent, _, cx| {
                let was_idle = this.call_idle() || !this.call_pointer_inside;
                this.call_pointer_at = Instant::now();
                this.call_pointer_inside = true;
                if was_idle {
                    cx.notify();
                }
            }))
    }

    // .call.focused: the header as a 48px band (over the stream in full screen), the focused card
    // edge to edge, letterboxed only on the axis that doesn't fit, and the controls floating over its
    // foot. Everyone else waits behind the pill on its bottom edge, or sits in a strip under it.
    fn focus_stage(&self, big: &Tile, tiles: &[Tile], shown: f32, cx: &mut Context<Self>) -> AnyElement {
        let key = tile_key(big);
        let others: Vec<&Tile> = tiles.iter().filter(|t| tile_key(t) != key).take(STRIP_LIMIT).collect();
        let strip = !self.call_strip_hidden && !others.is_empty();
        let overlay_head = self.call_fullscreen;
        let drift = 6. * (1. - shown);
        let band = CONTROLS_HEIGHT + STAGE_PAD * 2.;
        let head = self.stage_head(cx);
        let (in_flow, floating) = if overlay_head { (None, Some(head)) } else { (Some(head), None) };
        let toggle_bottom = if strip { STRIP_GAP } else { band + STRIP_GAP };
        let media = div()
            .relative()
            .flex_1()
            .min_h_0()
            .bg(gpui_kit::black())
            .child(self.tile(big, TileSize::Fill, cx))
            .when(!others.is_empty(), |d| d.child(div().absolute().left_0().right_0().bottom(px(toggle_bottom)).flex().justify_center().opacity(shown).child(self.strip_toggle(strip, cx))));
        self.stage_root(cx)
            .flex()
            .flex_col()
            .children(in_flow)
            .child(media)
            .when(strip, |d| {
                // Centred while it fits; wider than the stage, it starts at the left edge and scrolls.
                let cards: Vec<AnyElement> = others.into_iter().map(|t| self.tile(t, TileSize::Card(STRIP_TILE), cx)).collect();
                d.child(div().id("filmstrip").flex_none().flex().overflow_x_scroll().pt(px(STRIP_GAP)).px(px(STAGE_PAD)).child(div().mx_auto().flex_none().flex().gap(px(STRIP_GAP)).children(cards)))
                    .child(div().h(px(band)).flex_none())
            })
            .children(floating.map(|head| div().absolute().top(px(-drift)).left_0().right_0().h(px(FOCUS_HEAD + 24.)).opacity(shown).bg(scrim(180.)).child(head)))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom(px(-drift))
                    .pt(px(40.))
                    .px(px(STAGE_PAD))
                    .pb(px(STAGE_PAD))
                    .opacity(shown)
                    .when(!strip, |d| d.bg(scrim(0.)))
                    .child(self.call_controls(cx)),
            )
            .into_any_element()
    }

    // The grid, fitted to the space it actually has.
    fn card_stage(&self, tiles: &[Tile], fallback: (f32, f32), cx: &mut Context<Self>) -> AnyElement {
        let measured = self.call_area.get();
        let (width, height) = if measured.0 > 0. { measured } else { fallback };
        let body = self.tile_grid(tiles, width, height, cx);
        let (area, this) = (self.call_area.clone(), cx.weak_entity());
        let measure = gpui_kit::canvas(
            move |bounds, _, cx| {
                let size = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                if (size.0 - area.get().0).abs() > 0.5 || (size.1 - area.get().1).abs() > 0.5 {
                    area.set(size);
                    this.update(cx, |_, cx| cx.notify()).ok();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0();
        div().relative().size_full().child(measure).child(body).into_any_element()
    }

    // Clicking a card makes it the big one; clicking it again goes back to the grid. A stream you
    // aren't watching starts playing as it comes forward.
    fn toggle_card(&mut self, tile: &Tile, cx: &mut Context<Self>) {
        let key = tile_key(tile);
        if self.call_focus.as_ref() == Some(&key) {
            self.call_focus = None;
            return cx.notify();
        }
        if let Tile::Stream(state) = tile
            && self.call.as_ref().is_some_and(|c| c.watched(&state.user_id).is_none())
            && !self.me.as_ref().is_some_and(|m| m.id == state.user_id)
        {
            self.watch_stream(state.user_id.clone(), cx);
        }
        self.call_focus = Some(key);
        cx.notify();
    }

    // .tiles: the size that makes the biggest 16:9 tiles fit, 10px apart (8px three across and up).
    fn tile_grid(&self, tiles: &[Tile], width: f32, height: f32, cx: &mut Context<Self>) -> AnyElement {
        let n = tiles.len().max(1);
        let gap = |cols: usize| if cols >= 3 { TILE_GAP_DENSE } else { TILE_GAP };
        let size = |cols: usize| {
            let rows = n.div_ceil(cols) as f32;
            let w = (width - gap(cols) * (cols as f32 - 1.)) / cols as f32;
            w.min((height - gap(cols) * (rows - 1.)) / rows * 16. / 9.)
        };
        let cols = (1..=n).max_by(|a, b| size(*a).total_cmp(&size(*b))).unwrap_or(1);
        let tile_w = size(cols).max(124.);
        let rendered: Vec<AnyElement> = tiles.iter().map(|t| self.tile(t, TileSize::Card(tile_w), cx)).collect();
        div().size_full().flex().flex_wrap().items_center().justify_center().content_center().gap(px(gap(cols))).children(rendered).into_any_element()
    }

    // Full screen shows the call alone: no servers, channels or user panel.
    pub(crate) fn set_call_fullscreen(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.call_fullscreen = on;
        if window.is_fullscreen() != on {
            window.toggle_fullscreen();
        }
        cx.notify();
    }

    // Still pointer for a while, and nothing open that belongs to the bar.
    fn call_idle(&self) -> bool {
        self.call_pointer_at.elapsed() > CONTROLS_IDLE && self.device_menu.is_none() && self.menu.is_none()
    }

    // The header line over the stage: it drags the window, and clears Windows' caption buttons while the
    // stage runs to the window's right edge.
    fn stage_head(&self, cx: &mut Context<Self>) -> Div {
        div()
            .h(px(FOCUS_HEAD))
            .flex_none()
            .pl(px(STAGE_PAD))
            .pr(px(8.))
            .flex()
            .items_center()
            // Drag area: every control inside it occludes it, or Windows' caption takes its clicks.
            .window_control_area(gpui_kit::WindowControlArea::Drag)
            .child(div().flex_1().min_w_0().child(self.call_header(cx)))
            .child(self.caption_clearance(!self.call_chat_shown()))
    }

    pub(crate) fn call_chat_shown(&self) -> bool {
        self.call_chat && self.call.as_ref().is_some_and(|call| self.channel.as_ref().is_some_and(|c| c.id == call.channel.id))
    }

    // .callhead: the room (or whose screen), how long the call has run, who's in it, then the view
    // controls. While watching one stream: its quality, how smoothly it plays and a way to stop.
    fn call_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let call = self.call.as_ref().expect("in a call");
        let people = self.participants();
        let watching = call.focused();
        let several = call.watching.len() > 1;
        // A focused stream names the room, then whose it is and what they're playing, as Discord's header.
        let title = match self.focused_stream() {
            Some(id) => match self.activity_name(&id) {
                Some(app) => format!("{} • {} playing {app}", call.channel.name, self.user_name(&id)),
                None => format!("{} • {}'s screen", call.channel.name, self.user_name(&id)),
            },
            None => call.channel.name.clone(),
        };
        let glyph = Glyph::VolumeHigh;
        let timer = div()
            .h(px(22.))
            .px(px(8.))
            .flex_none()
            .squircle(RADIUS_XS)
            .flex()
            .items_center()
            .gap(px(6.))
            .bg(stage::wash(0.08))
            .text_color(stage::muted())
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .font_features(tabular())
            .child(div().size(px(6.)).rounded_full().bg(stage::online()))
            .child(clock(call.joined.elapsed().as_secs()));
        let faces: Vec<String> = people.iter().take(3).map(|p| p.user.as_ref().map(|u| u.avatar.clone()).unwrap_or_else(|| self.user_avatar(&p.user_id))).collect();
        let count = people.len();
        let who = div()
            .id("call-people")
            .h(px(32.))
            .pl(px(4.))
            .pr(px(8.))
            .flex_none()
            .squircle(RADIUS_SM)
            .flex()
            .items_center()
            .gap(px(6.))
            .text_color(stage::muted())
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .child(ui::avatar_stack(faces, 22., stage::bg()))
            .child(count.to_string())
            .tooltip(move |window, cx| Tooltip::new(format!("{count} in {}", if count == 1 { "the call alone" } else { "the call" })).build(window, cx));
        let health = watching.filter(|w| !several && !w.arrivals.is_empty()).map(|w| {
            let (health, fps) = stream_health(&w.arrivals, Instant::now());
            let (label, color) = match health {
                // No frames because the streamer paused isn't a fault.
                Health::Stalled if self.stream_paused(&w.streamer_id) => ("Paused", stage::muted()),
                Health::Smooth => ("Smooth", stage::online()),
                Health::Choppy => ("Choppy", stage::idle()),
                Health::Stalled => ("Stalled", stage::dnd()),
            };
            // GAP: the frame rate isn't reported by the voice engine, so only the resolution shows.
            let quality = w.frame.as_ref().map(|f| format!("{}p · ", f.size(0).height.0)).unwrap_or_default();
            div()
                .id("stream-health")
                .flex_none()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(12.))
                .text_color(stage::muted())
                .child(div().size(px(6.)).rounded_full().bg(color))
                .child(format!("{quality}{label}"))
                .tooltip(move |window, cx| Tooltip::new(format!("{label}: {fps:.0} frames per second")).build(window, cx))
        });
        let stop = watching.map(|_| stage_button("stop-watching", if several { "Stop all" } else { "Stop watching" }).occlude().on_click(cx.listener(|this, _, _, cx| this.stop_watching(cx))));
        // A DM call's stage is opened from its window; this is the way back to the chat (Esc too).
        let back = call.guild_id.is_none().then(|| stage_button("back-to-chat", "Back to chat").occlude().on_click(cx.listener(|this, _, window, cx| this.back_to_chat(window, cx))));
        let chat_open = self.call_chat_shown();
        let chat_button = stage_icon_button("call-chat", Glyph::ChatRound, if chat_open { "Hide Chat" } else { "Show Chat" }, chat_open || self.call_chat_highlighted())
            .relative()
            .children(self.call_chat_badge())
            .occlude()
            .on_click(cx.listener(|this, _, window, cx| this.toggle_call_chat(window, cx)));
        div()
            .h(px(HEAD_HEIGHT))
            .flex()
            .items_center()
            .gap(px(8.))
            .text_color(stage::strong())
            .child(Icon::new(glyph).size(px(20.)).text_color(stage::muted()))
            .child(div().min_w_0().truncate().text_size(px(16.)).font_weight(FontWeight::MEDIUM).child(title))
            .child(timer)
            .children(health)
            .child(div().flex_1())
            .children(back)
            .children(stop)
            .child(who)
            .child(div().w(px(1.)).h(px(20.)).mx(px(2.)).flex_none().bg(stage::wash(0.14)))
            .child(chat_button)
            .into_any_element()
    }

    // On the chat button: mentions in red, else unread in grey, only for the voice channel we're in.
    fn call_chat_badge(&self) -> Option<Div> {
        let (mentions, unread) = self.call_chat_counts()?;
        let badge = match (mentions, unread) {
            (0, 0) => return None,
            (0, unread) => ui::count_pill(unread, stage::faint(), stage::bg()),
            (mentions, _) => ui::count_badge(mentions),
        };
        Some(div().absolute().right(px(-4.)).bottom(px(-4.)).rounded_full().border_2().border_color(stage::bg()).child(badge))
    }

    // The chevron above the filmstrip that folds it away and back, as Discord's.
    fn strip_toggle(&self, shown: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let (glyph, tip) = if shown { (Glyph::ChevronDown, "Hide everyone") } else { (Glyph::ChevronUp, "Show everyone") };
        div()
            .id("strip-toggle")
            .h(px(STRIP_TOGGLE))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded_full()
            .bg(gpui_kit::black().opacity(0.6))
            .text_color(gpui_kit::white())
            .cursor_pointer()
            .hover(|s| s.bg(gpui_kit::black().opacity(0.75)))
            .child(Icon::new(glyph).size(px(14.)))
            .child(Icon::new(Glyph::Users).size(px(16.)))
            .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
            .on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.call_strip_hidden = !this.call_strip_hidden;
                cx.notify();
            }))
    }

    // The focused card's stream, if a stream is the one shown big.
    fn focused_stream(&self) -> Option<String> {
        self.call_focus.as_deref()?.strip_prefix("stream:").map(str::to_owned)
    }

    fn stream_paused(&self, streamer_id: &str) -> bool {
        let suffix = format!(":{streamer_id}");
        self.stream_paused.iter().any(|key| key.ends_with(&suffix))
    }

    // Everyone watching a streamer, other than the streamer.
    pub(crate) fn viewers(&self, streamer_id: &str) -> Vec<Viewer> {
        let suffix = format!(":{streamer_id}");
        let ids = self.stream_viewers.iter().find(|(key, _)| key.ends_with(&suffix)).map(|(_, v)| v.as_slice()).unwrap_or_default();
        ids.iter().filter(|id| *id != streamer_id).map(|id| Viewer { name: self.user_name(id).into(), avatar: self.user_avatar(id) }).collect()
    }

    // What someone is playing or streaming, from their presence, for name tags.
    fn activity_name(&self, user_id: &str) -> Option<String> {
        let activity = self.presences.get(user_id)?.activity.as_ref()?;
        matches!(activity.kind, discord::ActivityKind::Playing | discord::ActivityKind::Streaming | discord::ActivityKind::Competing).then(|| activity.text.clone())
    }

    fn open_stream_volume(&mut self, streamer_id: String, at: Point<Pixels>, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.open_live_menu(
            at,
            move |this| {
                let volume = this.stream_volumes.get(&streamer_id).copied().unwrap_or(1.);
                let id = streamer_id.clone();
                vec![crate::menu::slider("Stream volume", format!("stream-volume-{streamer_id}"), volume / 2., move |this, f, cx| this.set_stream_volume(&id, f * 2., cx))]
            },
            cx,
        );
    }

    fn set_stream_volume(&mut self, streamer_id: &str, volume: f32, cx: &mut Context<Self>) {
        self.stream_volumes.insert(streamer_id.to_owned(), volume);
        if let Some(call) = &self.call {
            call.session.set_stream_volume(streamer_id, volume);
        }
        cx.notify();
    }

    // A watched stream that ended, in its card's place: that it ended and why, and Close stream to clear
    // it. While it's up, the streamer going live again brings us back in as a viewer, as Discord's does.
    fn ended_tile(&self, streamer: &str, size: TileSize, cx: &mut Context<Self>) -> AnyElement {
        let (fill, w) = match size {
            TileSize::Card(w) => (false, w),
            TileSize::Fill => (true, f32::INFINITY),
        };
        let radius = if fill { 0. } else { RADIUS_LG };
        let small = w <= STRIP_TILE + 0.5;
        let card = div()
            .id(SharedString::from(format!("ended-{streamer}")))
            .relative()
            .map(|d| match size {
                TileSize::Card(w) => d.w(px(w)).h(px(w * 9. / 16.)).flex_none().squircle(radius).overflow_hidden(),
                TileSize::Fill => d.size_full(),
            })
            .bg(stage::ended())
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .p(px(12.))
            .text_center();
        if small {
            return card.child(Icon::new(Glyph::ScreenOff).size(px(24.)).text_color(stage::muted())).child(tile_ring(false, false, radius)).into_any_element();
        }
        let name = self.user_name(streamer);
        let line = match (self.participants().iter().any(|s| s.user_id == streamer), self.activity_name(streamer)) {
            (false, _) => format!("{name} left the call"),
            (true, Some(app)) => format!("{name} stopped sharing {app}"),
            (true, None) => format!("{name} stopped streaming"),
        };
        let close = streamer.to_owned();
        card.child(div().text_size(px(16.)).font_weight(FontWeight::MEDIUM).text_color(stage::strong()).child("Stream ended"))
            .child(div().text_size(px(12.)).text_color(stage::muted()).child(line))
            .child(div().mt(px(6.)).child(stage_button("ended-close", "Close stream").on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.close_ended_stream(&close, cx);
            }))))
            .when(!fill, |d| d.child(tile_ring(false, false, radius)))
            .with_animation(SharedString::from(format!("ended-in-{streamer}")), Animation::new(ui::DUR_PANEL), |d, t| d.opacity(t.min(1.)))
            .into_any_element()
    }

    fn close_ended_stream(&mut self, streamer: &str, cx: &mut Context<Self>) {
        if let Some(call) = &mut self.call {
            call.ended.retain(|id| id != streamer);
        }
        if self.focused_stream().as_deref() == Some(streamer) {
            self.call_focus = None;
        }
        cx.notify();
    }

    // Over a stream not yet watched: Watch stream switches to it alone; while others play, the button
    // beside it adds it to them, as Discord's.
    fn watch_actions(&self, tile: &Tile, alongside: bool, cx: &mut Context<Self>) -> Div {
        let (only, also) = (tile.clone(), tile.clone());
        let switch = watch_button("Watch stream").id("watch-only").cursor_pointer().on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.stop_watching(cx);
            this.call_focus = None;
            this.toggle_card(&only, cx);
        }));
        div().flex().items_center().gap(px(6.)).child(switch).when(alongside, |d| {
            d.child(
                watch_button_look()
                    .id("watch-alongside")
                    .w(px(30.))
                    .justify_center()
                    .cursor_pointer()
                    .child(Icon::new(Glyph::Plus).size(px(16.)))
                    .tooltip(|window, cx| ui::tip("Watch alongside", window, cx))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if let Tile::Stream(state) = &also {
                            this.watch_stream(state.user_id.clone(), cx);
                        }
                    })),
            )
        })
    }

    // .tile: a person (camera, or their avatar on their own tint) or someone's Go Live. Every card
    // carries a name chip (a stream's led by the screen glyph); only our own stream says LIVE, with its
    // quality. A stream not yet opened shows Watch at rest. Speaking draws the green ring above
    // everything. `TileSize::Fill` is the focused card: the whole stage, no corners or edge line.
    fn tile(&self, tile: &Tile, size: TileSize, cx: &mut Context<Self>) -> AnyElement {
        let call = self.call.as_ref().expect("in a call");
        let (fill, w) = match size {
            TileSize::Card(w) => (false, w),
            TileSize::Fill => (true, f32::INFINITY),
        };
        let radius = if fill { 0. } else { RADIUS_LG };
        let (Tile::User(state) | Tile::Stream(state)) = tile else {
            let Tile::Ended(streamer) = tile else { unreachable!() };
            return self.ended_tile(streamer, size, cx);
        };
        let user = state.user.clone().unwrap_or_else(|| User {
            id: state.user_id.clone(),
            name: self.user_name(&state.user_id),
            avatar: discord::avatar_url(&state.user_id, None),
            ..Default::default()
        });
        let me = self.me.as_ref().is_some_and(|m| m.id == state.user_id);
        let (muted, deafened) = if me { (self.muted || self.deafened, self.deafened) } else { (state.muted, state.deafened) };
        let local_muted = !me && self.prefs.local_mutes.contains(&state.user_id);
        let is_stream = matches!(tile, Tile::Stream(_));
        let talking = !is_stream && self.speaking(&state.user_id);
        let camera = (!is_stream).then(|| call.cameras.get(&state.user_id)).flatten();
        let media = is_stream || camera.is_some();
        // A stream card plays the stream once it's watched (our own from the local capture), and shows
        // Discord's preview still until then.
        // A watched stream that stopped delivering goes back to the loader instead of freezing on its
        // last frame; a paused one keeps it.
        let stalled = !me && call.watched(&state.user_id).is_some_and(|w| w.arrivals.back().is_some_and(|at| at.elapsed() > STALL_AFTER)) && !self.stream_paused(&state.user_id);
        let live_frame = match me {
            true => call.self_view.as_ref().map(|(frame, _)| frame.clone()),
            false => call.watched(&state.user_id).and_then(|w| w.frame.clone()).filter(|_| !stalled),
        };
        let loading = is_stream && live_frame.is_none() && (me || call.watched(&state.user_id).is_some());
        // Focused, the picture keeps its whole frame (bars on the axis that doesn't fit); a card is
        // cropped to fill.
        let fit = if fill { gpui_kit::ObjectFit::Contain } else { gpui_kit::ObjectFit::Cover };
        // A strip card under a focused one: no name at rest, as Discord's.
        let small = w <= STRIP_TILE + 0.5;
        let face: AnyElement = match (is_stream, camera) {
            // A stream that isn't playing, small: Discord's screen glyph on black.
            (true, _) if small && live_frame.is_none() => {
                div().size_full().squircle(radius).bg(gpui_kit::black()).flex().items_center().justify_center().child(Icon::new(Glyph::Screen).size(px(28.)).text_color(gpui_kit::white())).into_any_element()
            }
            // Playing, the picture letterboxes on black; the gradient only holds the place before it.
            (true, _) => match fill || live_frame.is_some() || loading {
                true => div().relative().size_full().when(!fill, |d| d.squircle(radius)).bg(gpui_kit::black()),
                false => stream_fill(radius).size_full(),
            }
            .map(|f| match live_frame {
                Some(frame) => f.child(ui::media(ImageSource::Render(frame), ui::all(radius)).absolute().inset_0().size_full().object_fit(gpui_kit::ObjectFit::Contain)),
                // Discord's preview is a stale, soft still: once we're waiting on the real picture, say so.
                None if loading => f.flex().items_center().justify_center().child(mascot::loader(SharedString::from(format!("stream-loading-{}", state.user_id)), if fill { 64. } else { 40. }, palette::logo(), cx)),
                None => f.children(self.stream_preview(&state.user_id).map(|url| ui::media(url, ui::all(radius)).absolute().inset_0().size_full().object_fit(fit))),
            })
            .into_any_element(),
            // Our own camera reads as a mirror, as every call app shows it; others see it unflipped.
            (false, Some(frame)) => ui::media(ImageSource::Render(frame.clone()), ui::all(radius)).size_full().object_fit(fit).when(me, mirrored).into_any_element(),
            (false, None) => div()
                .size_full()
                .when(!fill, |d| d.squircle(radius))
                .bg(self.tile_fill(&state.user_id))
                .flex()
                .items_center()
                .justify_center()
                .child(ui::avatar(user.avatar.clone(), if fill { 96. } else if w < SMALL_TILE { 40. } else { 64. }).when(local_muted, |a| a.opacity(0.5)))
                .into_any_element(),
        };
        let watching = call.watched(&state.user_id).is_some();
        let watchable = is_stream && !watching && !me;
        let label = match (is_stream, self.activity_name(&state.user_id)) {
            (true, Some(app)) => format!("{} · {app}", user.name),
            _ => user.name.clone(),
        };
        let group = SharedString::from(format!("tile-{}-{is_stream}", state.user_id));
        let name = tile_name(
            div()
                .when(is_stream, |d| d.child(Icon::new(Glyph::Screen).size(px(14.)).text_color(gpui_kit::white())))
                .when(!is_stream && state.server_muted, |d| d.child(vmark(Glyph::MicOff, None, MarkKind::Server)))
                .when(!is_stream && !state.server_muted && muted && !deafened, |d| d.child(Icon::new(Glyph::MicOff).size(px(14.)).text_color(stage::off_ink())))
                .when(!is_stream && deafened, |d| d.child(Icon::new(Glyph::HeadphonesOff).size(px(14.)).text_color(stage::off_ink())))
                .child(div().min_w_0().truncate().child(label))
                .when(local_muted, |d| d.child(vmark(Glyph::VolumeOff, Some("Muted for you"), MarkKind::Local))),
        )
        .max_w(if fill { relative(0.5) } else { px(w - 16.).into() })
        .when(small || fill, |n| n.opacity(0.).group_hover(STAGE_GROUP, |s| s.opacity(1.)));
        // With the name hidden on a strip card, a mute or deafen still shows, as a chip in its corner.
        let off_chip = (small && !is_stream && (muted || deafened || state.server_muted)).then(|| {
            div()
                .absolute()
                .left(px(6.))
                .bottom(px(6.))
                .size(px(24.))
                .squircle(RADIUS_XS)
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui_kit::black().opacity(0.6))
                .child(Icon::new(if deafened { Glyph::HeadphonesOff } else { Glyph::MicOff }).size(px(14.)).text_color(stage::off_ink()))
        });
        // Our own stream: its quality beside LIVE, as Discord's "720P 30FPS LIVE".
        let own_live = (is_stream && me && !small).then(|| {
            let quality = format!("{} FPS", quality_label(self.prefs.stream_resolution, self.prefs.stream_fps));
            div().absolute().left(px(8.)).top(px(8.)).flex().gap(px(4.)).child(ui::badge(gpui_kit::black().opacity(0.6), gpui_kit::white()).font_features(tabular()).child(quality)).child(ui::live_badge())
        });
        let streamer = state.user_id.clone();
        let menu_user = state.user_id.clone();
        let card = tile.clone();
        let viewers = if is_stream { self.viewers(&state.user_id) } else { Vec::new() };
        let alongside = call.watching.iter().any(|w| w.streamer_id != state.user_id);
        div()
            .id(group.clone())
            .group(group.clone())
            .relative()
            .map(|d| match size {
                TileSize::Card(w) => d.w(px(w)).h(px(w * 9. / 16.)).flex_none().squircle(radius).overflow_hidden(),
                TileSize::Fill => d.size_full(),
            })
            .child(face)
            // Scrims only while hovered, so pictures aren't dimmed at rest; the chips carry their own fill.
            .when(media && !fill, |d| d.child(scrim_top(radius).opacity(0.).group_hover(group.clone(), |s| s.opacity(1.))).child(scrim_bottom(radius).opacity(0.).group_hover(group.clone(), |s| s.opacity(1.))))
            .children(own_live)
            .when(!viewers.is_empty(), |d| d.child(viewer_chip(viewers, &state.user_id)))
            .when(watchable && !small, |d| {
                d.child(div().absolute().inset_0().when(!fill, |d| d.squircle(radius)).bg(gpui_kit::black().opacity(0.45)).flex().items_center().justify_center().child(self.watch_actions(tile, alongside, cx)))
            })
            .child(name)
            .children(off_chip)
            .children((!is_stream).then(|| self.soundboard_tile_effect(&state.user_id, cx)).flatten())
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_card(&card, cx)))
            .when(is_stream && watching && !me && !small && !fill, |d| {
                let streamer = streamer.clone();
                let volume_id = streamer.clone();
                d.child(
                    div()
                        .absolute()
                        .right(px(8.))
                        .bottom(px(6.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .opacity(0.)
                        .group_hover(group.clone(), |s| s.opacity(1.))
                        .child(ui::round_button(SharedString::from(format!("card-volume-{streamer}")), Glyph::VolumeHigh, "Stream volume", 28.).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.open_stream_volume(volume_id.clone(), e.position, cx);
                            }),
                        ))
                        .child(ui::button(SharedString::from(format!("card-unwatch-{streamer}")), ui::Tone::OnMedia, cx).small().child("Stop watching").on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.unwatch_stream(&streamer, cx);
                        }))),
                )
            })
            // Every card answers the pointer with a brighter edge; the focused one has no edge at all.
            .when(!fill || talking, |d| d.child(tile_ring(talking, local_muted, radius).when(!talking, |r| r.group_hover(group.clone(), |s| s.border_color(stage::wash(0.2))))))
            .on_mouse_down(MouseButton::Right, cx.listener(move |this, e: &MouseDownEvent, _, cx| this.voice_user_menu(&menu_user, e.position, cx)))
            .into_any_element()
    }

    // A person's tile tint (oklch 0.24 0.02, their hue): their avatar's hue once fetched, else one
    // from their id so the grid isn't one grey.
    fn tile_fill(&self, user_id: &str) -> Hsla {
        let hue = match self.avatar_colors.get(user_id).copied().flatten() {
            Some(color) => oklch_hue(color),
            None => user_id.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32)) as f32 % 360.,
        };
        palette::oklch(0.24, 0.02, hue)
    }

    // .controls: mic and camera as split buttons with their device menus, share, activities, more,
    // then the one red fill that leaves.
    fn call_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let call = self.call.as_ref().expect("in a call");
        let muted = self.muted || self.deafened;
        let connected = call.stage == Stage::Connected;
        let sharing = call.sharing.is_some();
        let menu_open = |kind: DeviceKind| self.device_menu.as_ref().is_some_and(|m| m.on_stage && m.kind == kind);
        let mic_state = if muted { Control::Off } else { Control::Rest };
        let camera_state = if call.camera_on { Control::On } else { Control::Rest };
        let mic = split_control(
            control("call-mic", if muted { Glyph::MicOffFill } else { Glyph::MicFill }, if muted { "Unmute" } else { "Mute" }, mic_state, true).on_click(cx.listener(|this, _, _, cx| this.toggle_mute(cx))),
            control("call-mic-devices", Glyph::ChevronUpBold, "Microphone options", if menu_open(DeviceKind::Input) { Control::Open } else { Control::Rest }, true)
                .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_stage_device_menu(DeviceKind::Input, e.position, cx))),
        );
        let camera = split_control(
            control("call-camera", if call.camera_on { Glyph::VideoFill } else { Glyph::VideoOffFill }, if call.camera_on { "Turn off camera" } else { "Turn on camera" }, camera_state, connected)
                .when(connected, |b| b.on_click(cx.listener(|this, _, _, cx| this.toggle_camera(cx)))),
            control("call-camera-devices", Glyph::ChevronUpBold, "Camera options", if menu_open(DeviceKind::Camera) { Control::Open } else { Control::Rest }, true)
                .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_stage_device_menu(DeviceKind::Camera, e.position, cx))),
        );
        // Live, the main half stops the stream and the chevron holds its options, as Discord's.
        let share_state = if sharing { Control::Active } else { Control::Rest };
        let share = split_control(
            control("call-share", Glyph::ShareScreenFill, if sharing { "Stop streaming" } else { "Share your screen" }, share_state, connected)
                .when(connected, |b| b.on_click(cx.listener(|this, _, _, cx| this.toggle_screen_share(cx)))),
            control("call-share-options", Glyph::ChevronUpBold, "Stream options", if self.menu.is_some() && sharing { Control::Open } else { Control::Rest }, sharing)
                .when(sharing, |b| b.on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_stream_menu(e.position, cx)))),
        );
        let activities = control("call-activities", Glyph::ActivitiesFill, "Activities", Control::Rest, connected)
            .when(connected, |b| b.on_click(cx.listener(|this, _, _, cx| this.show_toast("Activities aren't in Waffle yet", cx))));
        let access = self.soundboard_access();
        let soundboard = (access != soundboard::Access::Hidden).then(|| {
            let on = connected && access == soundboard::Access::Ready;
            control("call-soundboard", Glyph::Soundboard, access.tip(), if self.soundboard_open() { Control::Open } else { Control::Rest }, on)
                .relative()
                .child(self.soundboard_probe(soundboard::CALL_CONTROL))
                .when(on, |b| b.on_click(cx.listener(|this, _, window, cx| this.open_soundboard(Some(soundboard::CALL_CONTROL), window, cx))))
        });
        let more = control("call-more", Glyph::More, "More options", Control::Rest, true).on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_call_more(e.position, cx)));
        let leave = self.leave_control(cx);
        // Full screen at the bar's right end, as Discord's.
        let fullscreen = self.call_fullscreen;
        let fullscreen = stage_icon_button("call-fullscreen", if fullscreen { Glyph::FullscreenExit } else { Glyph::Fullscreen }, if fullscreen { "Exit full screen (Esc)" } else { "Full screen" }, false)
            .on_click(cx.listener(move |this, _, window, cx| this.set_call_fullscreen(!fullscreen, window, cx)));
        // A focused stream's volume sits beside it, as Discord's.
        let volume = self.focused_stream().filter(|id| call.watched(id).is_some()).map(|id| {
            stage_icon_button("call-stream-volume", Glyph::VolumeHigh, "Stream volume", false).on_mouse_down(MouseButton::Left, cx.listener(move |this, e: &MouseDownEvent, _, cx| this.open_stream_volume(id.clone(), e.position, cx)))
        });
        div()
            .relative()
            .h(px(CONTROLS_HEIGHT))
            .flex()
            .justify_center()
            .items_center()
            .gap(px(8.))
            .child(control_group().child(mic).child(camera))
            .child(control_group().child(share).children(soundboard).child(activities).child(more))
            .child(leave)
            .child(div().absolute().right_0().top(px((CONTROLS_HEIGHT - 32.) / 2.)).flex().gap(px(4.)).children(volume).child(fullscreen))
    }

    // The one red button. Watching the stream shown big, it stops watching it rather than leaving, as
    // Discord's. One element across both, so a press that spans the switch still lands, and the glyph
    // cross-fades into the new action.
    fn leave_control(&self, cx: &mut Context<Self>) -> ui::Pressable {
        let call = self.call.as_ref().expect("in a call");
        let me = self.me.as_ref().map(|m| m.id.as_str());
        let watching = self.focused_stream().filter(|id| call.watched(id).is_some() && me != Some(id.as_str()));
        let (glyph, tip) = match watching {
            Some(_) => (Glyph::ScreenOff, "Stop watching"),
            None => (Glyph::CallEnd, "Leave call"),
        };
        let icon = div().child(Icon::new(glyph).size(px(20.))).with_animation(
            ElementId::Name(SharedString::from(format!("call-leave-{tip}"))),
            Animation::new(ui::DUR_POP).with_easing(|t| crate::motion::ease_out().sample(t)),
            |icon, t| icon.opacity(t).scale(0.25 + 0.75 * t),
        );
        ui::pressable(div().id("call-leave"), 0.92)
            .w(px(56.))
            .h(px(CONTROLS_HEIGHT))
            .flex_none()
            .squircle(10. + DOCK_PAD)
            .flex()
            .items_center()
            .justify_center()
            .bg(palette::danger())
            .text_color(gpui_kit::white())
            .cursor_pointer()
            .hover(|s| s.bg(palette::danger_hover()))
            .child(icon)
            .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
            .on_click(cx.listener(move |this, _, _, cx| match watching.clone() {
                Some(id) => {
                    this.call_focus = None;
                    this.unwatch_stream(&id, cx);
                }
                None => this.leave_voice(cx),
            }))
    }

    // More options: what didn't earn a button of its own.
    fn open_call_more(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.open_live_menu(
            at,
            |this| {
                let Some(call) = &this.call else { return Vec::new() };
                let mut entries = Vec::new();
                if let Some(guild) = call.guild_id.clone() {
                    let channel = call.channel.id.clone();
                    entries.push(item("Invite to voice", move |this, window, cx| this.show_invite(guild.clone(), channel.clone(), window, cx)).icon(Glyph::GroupPlus).entry());
                }
                if this.soundboard_access() == soundboard::Access::Ready {
                    entries.push(item("Soundboard", |this, window, cx| this.open_soundboard(Some(soundboard::CALL_CONTROL), window, cx)).icon(Glyph::Soundboard).entry());
                }
                if this.call_focus.is_some() {
                    let hidden = this.call_strip_hidden;
                    entries.push(
                        item(if hidden { "Show everyone" } else { "Hide everyone" }, |this, _, cx| {
                            this.call_strip_hidden = !this.call_strip_hidden;
                            cx.notify();
                        })
                        .hint("The cards under the focused one")
                        .icon(Glyph::Users)
                        .entry(),
                    );
                }
                if call.guild_id.is_some() {
                    let id = call.channel.id.clone();
                    let on = this.call_previews_enabled(&id);
                    entries.push(item("Show New Message Previews", move |this, _, cx| this.toggle_call_previews(&id, cx)).checked(on).entry());
                }
                entries
            },
            cx,
        );
    }

    pub(crate) fn open_device_menu(&mut self, kind: DeviceKind, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.show_device_menu(kind, at, false, cx);
    }

    fn open_stage_device_menu(&mut self, kind: DeviceKind, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.show_device_menu(kind, at, true, cx);
    }

    fn show_device_menu(&mut self, kind: DeviceKind, at: Point<Pixels>, on_stage: bool, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let (inputs, outputs, cameras) = match kind {
            DeviceKind::Input => (voice::input_devices(), voice::output_devices(), Vec::new()),
            DeviceKind::Output => (Vec::new(), voice::output_devices(), Vec::new()),
            DeviceKind::Camera => (Vec::new(), Vec::new(), voice::cameras()),
        };
        self.device_menu = Some(DeviceMenu { kind, at, on_stage, inputs, outputs, cameras });
        cx.notify();
    }

    fn choose_device(&mut self, kind: DeviceKind, id: Option<String>, cx: &mut Context<Self>) {
        self.update_prefs(cx, |p| match kind {
            DeviceKind::Input => p.input_device = id,
            DeviceKind::Output => p.output_device = id,
            DeviceKind::Camera => p.camera = id,
        });
        if kind == DeviceKind::Camera
            && let Some(call) = &self.call
            && call.camera_on
            && let Err(error) = call.session.set_camera(self.prefs.camera.clone())
        {
            self.voice_error = Some(error.into());
        }
    }

    pub(crate) fn render_call_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::new();
        out.extend(self.connection_overlay(cx));
        out.extend(self.device_overlay(window, cx));
        out
    }

    // .connpop: how the connection is doing, ping over the last few minutes, the numbers behind it,
    // and how the call is secured.
    fn connection_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.connection_info {
            return None;
        }
        let call = self.call.as_ref()?;
        let stats = call.stats.clone().unwrap_or_default();
        let measured = call.stats.is_some();
        let (strong, muted, faint) = (palette::strong(cx), palette::muted(cx), palette::faint(cx));
        let link = Link::of(call.stage, call.stats.as_ref());
        let verdict = match (link, measured) {
            (Link::Connected | Link::Poor, true) => quality_word(stats.median_ping_ms),
            (Link::Lost, _) => "Reconnecting",
            _ => "Connecting",
        };
        let verdict_ink = match verdict {
            "Good" => palette::positive_ink(cx),
            "Connecting" => muted,
            _ => palette::warning_ink(cx),
        };
        let peak = stats.last_pings.iter().copied().max().unwrap_or(0).max(160) as f32;
        let graph = div()
            .h(px(56.))
            .p(px(6.))
            .squircle(RADIUS_SM)
            .bg(palette::surface(palette::Level::Page, cx))
            .flex()
            .items_end()
            .gap(px(2.))
            .children(stats.last_pings.iter().map(|&p| div().flex_1().min_w(px(2.)).h(relative((p as f32 / peak).clamp(0.06, 1.))).rounded_t(px(2.)).rounded_b(px(1.)).bg(ping_color(p, cx))));
        let swatch = |color: Hsla, text: &'static str| div().flex().items_center().gap(px(4.)).child(div().size(px(8.)).rounded(px(2.)).bg(color)).child(text);
        let legend = div()
            .flex()
            .flex_wrap()
            .gap(px(12.))
            .text_size(px(12.))
            .text_color(muted)
            .child(swatch(ping_color(0, cx), "Under 80 ms"))
            .child(swatch(ping_color(PING_FAIR, cx), "80–120 ms"))
            .child(swatch(ping_color(PING_BAD + 1, cx), "Over 120 ms"));
        let dash = |value: String| if measured { value } else { "—".into() };
        let row = |label: &'static str, value: String| {
            div()
                .min_h(px(26.))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .child(div().flex_none().text_color(muted).child(label))
                .child(div().min_w_0().truncate().text_color(strong).font_weight(FontWeight::MEDIUM).font_features(tabular()).child(value))
        };
        let (lock_ink, secured) = match stats.dave {
            true => (palette::positive_ink(cx), "Voice and video in this call are end-to-end encrypted. Only the people in it can hear them."),
            false => (muted, "Voice and video are encrypted on the way to Discord's servers, not end to end."),
        };
        let card = div()
            .id("connection-card")
            .occlude()
            .absolute()
            .left(px(8.))
            .bottom(px(self.panel_stack_height() + 16.))
            .w(px(300.))
            .p(px(16.))
            .squircle(RADIUS_LG)
            .bg(palette::surface(palette::Level::Overlay, cx))
            .shadow(ui::shadow_pop(cx))
            .flex()
            .flex_col()
            .gap(px(12.))
            .text_size(px(14.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().flex().items_baseline().justify_between().gap(px(8.)).child(div().text_color(strong).font_weight(FontWeight::MEDIUM).child("Connection")).child(div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(verdict_ink).child(verdict)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(graph)
                    // GAP: the engine keeps its last 20 heartbeat pings, about 13.75s apart, so the graph spans roughly five minutes.
                    .child(div().flex().justify_between().text_size(px(12.)).text_color(faint).child("5 min ago").child("Now"))
                    .child(legend),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(row("Ping", dash(format!("{} ms", stats.median_ping_ms))))
                    .child(row("Packet loss", dash(stats.outbound_loss_pct.map_or("—".into(), |loss| format!("{loss:.1}%")))))
                    .child(row("Server", dash(stats.endpoint.clone()))),
            )
            .child(
                div()
                    .pt(px(12.))
                    .border_t_1()
                    .border_color(palette::line(cx))
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .text_color(muted)
                    .child(Icon::new(Glyph::HashtagLock).size(px(16.)).flex_none().text_color(lock_ink))
                    .child(div().flex_1().min_w_0().child(secured)),
            )
            .child(div().text_size(px(12.)).text_color(faint).child("Updated every 2 seconds"))
;
        // It opens above the user area, so it rises from its bottom-left corner.
        Some(call_backdrop("connection-backdrop", cx).child(ui::rise(card.transform_origin(ui::corner(false, true)), "connection-card", Duration::ZERO)).into_any_element())
    }

    // .devmenu, opened above its chevron: the devices to pick from, volume (with the live level for
    // the mic), noise suppression, and the way to the full settings. Over the stage it stays dark.
    fn device_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.device_menu.as_ref()?;
        let ink = Inks::of(menu.on_stage, cx);
        let kind = menu.kind;
        let mut body: Vec<AnyElement> = Vec::new();
        let group = |body: &mut Vec<AnyElement>, title: &'static str, which: DeviceKind, devices: &[voice::Device], chosen: Option<String>, cx: &mut Context<Self>| {
            body.push(menu_label(title, &ink).into_any_element());
            let mut options: Vec<(Option<String>, String)> = Vec::new();
            if which != DeviceKind::Camera {
                options.push((None, "Default".into()));
            }
            options.extend(devices.iter().map(|d| (Some(d.id.clone()), d.name.clone())));
            body.extend(options.into_iter().enumerate().map(|(i, (id, name))| {
                let on = id == chosen;
                choice_row(SharedString::from(format!("device-{title}-{i}")), &ink)
                    .child(ui::radio_with(on, 18., ink.line_strong))
                    .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::NORMAL).text_color(ink.strong).child(name))
                    .on_click(cx.listener(move |this, _, _, cx| this.choose_device(which, id.clone(), cx)))
                    .into_any_element()
            }));
        };
        match kind {
            DeviceKind::Input => {
                group(&mut body, "Input device", DeviceKind::Input, &menu.inputs, self.prefs.input_device.clone(), cx);
                group(&mut body, "Output device", DeviceKind::Output, &menu.outputs, self.prefs.output_device.clone(), cx);
            }
            DeviceKind::Output => group(&mut body, "Output device", DeviceKind::Output, &menu.outputs, self.prefs.output_device.clone(), cx),
            DeviceKind::Camera => group(&mut body, "Camera", DeviceKind::Camera, &menu.cameras, self.prefs.camera.clone(), cx),
        }
        body.push(menu_rule(&ink).into_any_element());
        if kind != DeviceKind::Camera {
            let (label, value, key) = match kind {
                DeviceKind::Output => ("Output volume", self.prefs.output_volume, "menu-output-volume"),
                _ => ("Input volume", self.prefs.input_volume, "menu-input-volume"),
            };
            let level = (kind == DeviceKind::Input).then(|| self.call.as_ref().map(|c| c.level.0)).flatten();
            body.push(
                self.call_menu_slider(SharedString::from(key), label, value / 2., &ink, move |this, f, cx| {
                    this.update_prefs(cx, |p| match kind {
                        DeviceKind::Output => p.output_volume = f.clamp(0., 1.) * 2.,
                        _ => p.input_volume = f.clamp(0., 1.) * 2.,
                    })
                }, cx)
                .children(level.map(|db| ui::meter_with((db + 100.) / 100., METER_BARS, ink.lit, ink.line_strong).h(px(14.)).gap(px(2.))))
                .into_any_element(),
            );
        }
        if kind == DeviceKind::Input {
            let noise = self.prefs.noise_suppression;
            body.push(
                choice_row("device-noise", &ink)
                    .child(div().flex_1().min_w_0().child(div().font_weight(FontWeight::NORMAL).text_color(ink.strong).child("Noise suppression")).child(div().text_size(px(12.)).text_color(ink.muted).child("Filters fans and keys on this device")))
                    .child(ui::switch_live("device-noise-switch", noise, window, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_noise_suppression(cx)))
                    .into_any_element(),
            );
            body.push(menu_rule(&ink).into_any_element());
        }
        let settings_label = if kind == DeviceKind::Camera { "Video settings" } else { "Voice settings" };
        body.push(
            choice_row("device-settings", &ink)
                .child(Icon::new(Glyph::Settings).size(px(18.)).text_color(ink.muted))
                .child(div().flex_1().font_weight(FontWeight::NORMAL).text_color(ink.text).child(settings_label))
                .child(Icon::new(Glyph::ChevronRight).size(px(14.)).text_color(ink.faint))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.device_menu = None;
                    this.open_settings(Page::VoiceVideo, window, cx);
                }))
                .into_any_element(),
        );
        let viewport = window.viewport_size();
        let card = div()
            .id("device-menu")
            .occlude()
            .absolute()
            .left(px((f32::from(menu.at.x) - 150.).clamp(8., f32::from(viewport.width) - 308.)))
            .bottom(px((f32::from(viewport.height) - f32::from(menu.at.y) + 12.).max(8.)))
            .w(px(300.))
            .p(px(6.))
            .squircle(RADIUS_LG)
            .bg(ink.overlay)
            .shadow(ui::shadow_pop_on(ink.line_strong, cx))
            .flex()
            .flex_col()
            .gap(px(1.))
            .text_size(px(14.))
            .text_color(ink.text)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(body);
        // It opens above its chevron, so it rises from its bottom edge's centre.
        let card = ui::rise(card.transform_origin(gpui_kit::point(gpui_kit::relative(0.5), gpui_kit::relative(1.))), "device-menu", Duration::ZERO);
        Some(call_backdrop("device-backdrop", cx).child(card).into_any_element())
    }

    // .mslider: the label and its percentage over the slider; click or drag anywhere on it.
    fn call_menu_slider(&self, key: SharedString, label: &'static str, fraction: f32, ink: &Inks, on_set: impl Fn(&mut Waffle, f32, &mut Context<Waffle>) + Clone + 'static, cx: &mut Context<Self>) -> Div {
        let bounds = self.slider_bounds.clone();
        let record = bounds.clone();
        let (down_key, move_key, record_key) = (key.clone(), key.clone(), key.clone());
        let set = move |this: &mut Waffle, key: &SharedString, x: Pixels, cx: &mut Context<Waffle>| {
            let Some(b) = bounds.borrow().get(key).copied() else { return };
            on_set(this, ((x - b.left()) / b.size.width).clamp(0., 1.), cx);
        };
        let set_drag = set.clone();
        let slider = div()
            .id(SharedString::from(format!("menu-slider-{key}")))
            .relative()
            .w_full()
            .cursor_pointer()
            .child(gpui_kit::canvas(move |b, _, _| { record.borrow_mut().insert(record_key.clone(), b); }, |_, _, _, _| {}).absolute().size_full())
            .child(ui::slider_track(fraction, cx))
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, e: &MouseDownEvent, _, cx| set(this, &down_key, e.position.x, cx)))
            .on_drag(MenuSliderDrag(key.clone()), |drag, _, _, cx| cx.new(|_| drag.clone()))
            .on_drag_move::<MenuSliderDrag>(cx.listener(move |this, e: &gpui_kit::DragMoveEvent<MenuSliderDrag>, _, cx| {
                if e.drag(cx).0 == move_key {
                    set_drag(this, &move_key, e.event.position.x, cx);
                }
            }));
        div()
            .px(px(10.))
            .pt(px(6.))
            .pb(px(10.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(div().flex().justify_between().font_weight(FontWeight::NORMAL).text_color(ink.text).child(label).child(div().text_color(ink.muted).font_features(tabular()).child(format!("{:.0}%", fraction * 100.))))
            .child(slider)
    }

    // Right-click on anyone in voice opens Discord's server user menu with its voice items.
    pub(crate) fn voice_user_menu(&mut self, user_id: &str, at: Point<Pixels>, cx: &mut Context<Self>) {
        let found = self.guilds.iter().find_map(|g| g.voice_states.iter().find(|v| v.user_id == user_id).map(|v| (g.id.clone(), v.user.clone())));
        let guild_id = found.as_ref().map(|(g, _)| g.clone()).or_else(|| self.call.as_ref().and_then(|c| c.guild_id.clone()));
        let user = found.and_then(|(_, u)| u).unwrap_or_else(|| User {
            id: user_id.to_owned(),
            name: self.user_name(user_id),
            avatar: discord::avatar_url(user_id, None),
            ..Default::default()
        });
        // Anyone in voice, server or DM call, gets Discord's voice items (volume, local mute).
        let surface = crate::user_menu::Surface::ServerVoice;
        self.open_user_menu(user, guild_id, surface, at, cx);
    }

    pub(crate) fn set_user_volume(&mut self, user_id: &str, volume: f32, cx: &mut Context<Self>) {
        if let Some(call) = &self.call {
            call.session.set_user_volume(user_id, volume);
        }
        let user_id = user_id.to_owned();
        self.update_prefs(cx, |p| {
            p.user_volumes.insert(user_id, volume);
        });
    }

    pub(crate) fn toggle_local_mute(&mut self, user_id: &str, cx: &mut Context<Self>) {
        let muted = !self.prefs.local_mutes.iter().any(|u| u == user_id);
        if let Some(call) = &self.call {
            call.session.set_user_muted(user_id, muted);
        }
        let user_id = user_id.to_owned();
        self.update_prefs(cx, |p| match muted {
            true => p.local_mutes.push(user_id),
            false => p.local_mutes.retain(|u| *u != user_id),
        });
    }
}

fn call_backdrop(id: &'static str, cx: &mut Context<Waffle>) -> Stateful<Div> {
    div().id(id).absolute().inset_0().occlude().on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _, _, cx| {
            this.dismiss_call_menus();
            cx.notify();
        }),
    )
}

// The few tokens a menu reads, from the stage or from the theme.
struct Inks {
    overlay: Hsla,
    strong: Hsla,
    text: Hsla,
    muted: Hsla,
    faint: Hsla,
    hover: Hsla,
    line: Hsla,
    line_strong: Hsla,
    // A meter's lit bars: on, hot, clipping.
    lit: [Hsla; 3],
}

impl Inks {
    fn of(on_stage: bool, cx: &App) -> Self {
        match on_stage {
            true => Inks { overlay: stage::overlay(), strong: stage::strong(), text: stage::text(), muted: stage::muted(), faint: stage::faint(), hover: stage::wash(0.05), line: stage::wash(0.08), line_strong: stage::wash(0.14), lit: [discord::Status::Online, discord::Status::Idle, discord::Status::Dnd].map(palette::status) },
            false => Inks {
                overlay: palette::surface(palette::Level::Overlay, cx),
                strong: palette::strong(cx),
                text: palette::text(cx),
                muted: palette::muted(cx),
                faint: palette::faint(cx),
                hover: palette::hover(cx),
                line: palette::line(cx),
                line_strong: palette::line_strong(cx),
                lit: [discord::Status::Online, discord::Status::Idle, discord::Status::Dnd].map(|s| palette::status_in(s, cx)),
            },
        }
    }
}

// .voicebar .why: the second line, in the status's tone, saying what needs you.
fn why_line(text: impl Into<SharedString>, tone: Hsla) -> Div {
    div().h(px(18.)).truncate().text_size(px(12.)).text_color(tone).child(text.into())
}

// .voicebar .acts .btn: equal-width 32px buttons; disabled until connected, saying why.
fn panel_action(id: &'static str, glyph: Glyph, tip: &'static str, enabled: bool, cx: &App) -> Stateful<Div> {
    let (fill, press, ink) = (palette::selected(cx), palette::press(cx), palette::strong(cx));
    div()
        .id(id)
        .flex_1()
        .min_w_0()
        .h(px(32.))
        .squircle(RADIUS_SM)
        .flex()
        .items_center()
        .justify_center()
        .bg(fill)
        .text_color(ink)
        .child(Icon::new(glyph).size(px(18.)))
        .when(enabled, |b| b.cursor_pointer().hover(move |s| s.bg(press)))
        .when(!enabled, |b| b.opacity(0.45).cursor(gpui_kit::CursorStyle::OperationNotAllowed))
        .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
}

// .iconbtn on the stage: muted until hovered; `pressed` holds the selected fill.
fn stage_icon_button(id: &'static str, glyph: Glyph, tip: &'static str, pressed: bool) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(32.))
        .flex_none()
        .squircle(RADIUS_SM)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(if pressed { stage::strong() } else { stage::muted() })
        .when(pressed, |b| b.bg(stage::wash(0.08)))
        .hover(|s| s.bg(stage::wash(0.05)).text_color(stage::strong()))
        .active(|s| s.bg(stage::wash(0.09)))
        .child(Icon::new(glyph).size(px(20.)))
        .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
}

// .btn.btn--sm on the stage: the quiet fill.
fn stage_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(30.))
        .px(px(10.))
        .flex_none()
        .squircle(RADIUS_SM)
        .flex()
        .items_center()
        .gap(px(8.))
        .cursor_pointer()
        .bg(stage::wash(0.08))
        .hover(|s| s.bg(stage::wash(0.09)))
        .text_color(stage::strong())
        .text_size(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .whitespace_nowrap()
        .child(label)
}

// .preview .watch: the light button over a dimmed stream.
pub(crate) fn watch_button(label: &'static str) -> Div {
    watch_button_look().px(px(10.)).gap(px(8.)).child(Icon::new(Glyph::Play).size(px(16.))).child(label)
}

fn watch_button_look() -> Div {
    div()
        .h(px(30.))
        .squircle(RADIUS_SM)
        .flex()
        .items_center()
        .bg(stage::strong())
        .hover(|s| s.bg(gpui_kit::white()))
        .text_color(stage::on_ink())
        .text_size(px(14.))
        .font_weight(FontWeight::MEDIUM)
}

// A call control's state: .cbtn at rest, .off (coral), .on (light), .active (accent), the open
// chevron ([aria-expanded]). The red .leave is its own button.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Control {
    Rest,
    Off,
    On,
    Active,
    Open,
}

// .cbtn: 40x40 inside its group, a squircle; icons 20px (the chevron 12px on a 20px strip). Pressed, it sinks to 92%.
// The group is opaque, so a button at rest has no fill of its own.
fn control(id: &'static str, glyph: Glyph, tip: &'static str, state: Control, enabled: bool) -> ui::Pressable {
    let (fill, hover, ink) = match state {
        Control::Rest => (gpui_kit::transparent_black(), stage::wash(0.08), gpui_kit::white()),
        Control::Off => (stage::off(false), stage::off(true), stage::off_ink()),
        Control::On => (stage::on(false), stage::on(true), stage::on_ink()),
        Control::Active => (palette::accent(), palette::accent_hover(), palette::on_accent()),
        Control::Open => (stage::wash(0.12), stage::wash(0.12), gpui_kit::white()),
    };
    let chevron = matches!(glyph, Glyph::ChevronUpBold);
    let side = CONTROLS_HEIGHT - DOCK_PAD * 2.;
    ui::pressable(div().id(id), if enabled { 0.92 } else { 1. })
        .w(px(if chevron { 20. } else { side }))
        .h(px(side))
        .flex_none()
        .squircle(10.)
        .flex()
        .items_center()
        .justify_center()
        .bg(fill)
        .text_color(if chevron && state == Control::Rest { stage::muted() } else { ink })
        .child(Icon::new(glyph).size(px(if chevron { 12. } else { 20. })))
        .when(enabled, |b| b.cursor_pointer().hover(move |s| s.bg(hover).text_color(ink)))
        .when(!enabled, |b| b.opacity(0.4).cursor(gpui_kit::CursorStyle::OperationNotAllowed))
        .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
}

fn mirrored<E: Styled>(mut el: E) -> E {
    el.style().scale.width = Some(-1.);
    el
}

// .csplit: the main button with its chevron tucked flush beside it.
fn split_control(main: impl IntoElement, chevron: impl IntoElement) -> Div {
    div().flex().child(main).child(chevron)
}

// .cgroup: related controls on one opaque plate with a hairline, so they read over any picture.
fn control_group() -> Div {
    div()
        .h(px(CONTROLS_HEIGHT))
        .flex()
        .items_center()
        .gap(px(2.))
        .p(px(DOCK_PAD))
        .squircle(10. + DOCK_PAD)
        .bg(stage::dock())
        .border_1()
        .border_color(stage::wash(0.08))
}

// .tile .name: a dark chip in the tile's foot, 12px white, so it reads over any picture.
fn tile_name(content: Div) -> Div {
    content
        .absolute()
        .left(px(8.))
        .bottom(px(8.))
        .h(px(24.))
        .px(px(8.))
        .squircle(RADIUS_XS)
        .bg(gpui_kit::black().opacity(0.6))
        .flex()
        .items_center()
        .gap(px(6.))
        .min_w_0()
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(gpui_kit::white())
}

// The tile's edge, drawn above its media so a stream can't hide it: a faint 1px line, or while
// speaking a 2px green ring with a dark inner line (neutral when they're muted for you).
fn tile_ring(speaking: bool, local_muted: bool, radius: f32) -> Div {
    let ring = div().absolute().inset_0().squircle(radius);
    if !speaking {
        return ring.border_1().border_color(stage::wash(0.06));
    }
    // The stage is always dark, so the dark theme's green.
    let color = if local_muted { stage::wash(0.42) } else { palette::status(crate::discord::Status::Online) };
    ring.border(px(2.)).border_color(color).child(div().absolute().inset_0().squircle((radius - 2.).max(0.)).border(px(2.)).border_color(stage::bg().opacity(0.6)))
}

// .chrome over a focused stream: black fading out from the top (180deg) or bottom (0deg) edge.
fn scrim(angle: f32) -> gpui_kit::Background {
    gpui_kit::linear_gradient(angle, gpui_kit::linear_color_stop(gpui_kit::black().opacity(0.6), 0.), gpui_kit::linear_color_stop(gpui_kit::transparent_black(), 1.))
}

// .tile.has-media::before: a smooth scrim under the labels, never a hard strip.
fn scrim_top(radius: f32) -> Div {
    ui::edge(div().absolute().top_0().left_0().right_0().h(relative(0.3)), ui::top(radius)).bg(gpui_kit::linear_gradient(180., gpui_kit::linear_color_stop(gpui_kit::black().opacity(0.35), 0.), gpui_kit::linear_color_stop(gpui_kit::transparent_black(), 1.)))
}

fn scrim_bottom(radius: f32) -> Div {
    ui::edge(div().absolute().bottom_0().left_0().right_0().h(relative(0.38)), ui::bottom(radius)).bg(gpui_kit::linear_gradient(180., gpui_kit::linear_color_stop(gpui_kit::transparent_black(), 0.), gpui_kit::linear_color_stop(gpui_kit::black().opacity(0.55), 1.)))
}

// .tile .viewers: who's watching, top right; a few faces and a "+n", the full list on hover.
const VIEWER_FACES: usize = 4;

#[derive(Clone)]
pub(crate) struct Viewer {
    name: SharedString,
    avatar: String,
}

pub(crate) fn viewer_chip(viewers: Vec<Viewer>, streamer_id: &str) -> Stateful<Div> {
    let avatars: Vec<String> = viewers.iter().map(|v| v.avatar.clone()).collect();
    div()
        .id(SharedString::from(format!("viewers-{streamer_id}")))
        .absolute()
        .right(px(8.))
        .top(px(8.))
        .h(px(28.))
        .pl(px(8.))
        .pr(px(4.))
        .rounded_full()
        .flex()
        .items_center()
        .gap(px(6.))
        .bg(stage::dock())
        .text_color(stage::muted())
        .child(Icon::new(Glyph::EyeBold).size(px(14.)))
        .child(ui::capped_stack(&avatars, VIEWER_FACES, 20., stage::dock(), stage::overlay(), stage::strong()))
        .tooltip(move |_, cx| cx.new(|_| ViewerList(viewers.clone())).into())
}

// The hover list behind the viewer chip: everyone watching, face and name.
struct ViewerList(Vec<Viewer>);

impl Render for ViewerList {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let rows = self.0.iter().map(|v| {
            div().flex().items_center().gap(px(8.)).child(ui::avatar(v.avatar.clone(), 20.)).child(div().truncate().child(v.name.clone()))
        });
        div()
            .max_w(px(240.))
            .max_h(px(320.))
            .overflow_hidden()
            .p(px(10.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .squircle(RADIUS_MD)
            .bg(stage::overlay())
            .text_color(stage::text())
            .text_size(px(14.))
            .child(div().text_color(stage::muted()).text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).child(format!("Watching · {}", self.0.len())))
            .children(rows)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MarkKind {
    // A moderator's lock: solid red.
    Server,
    // Your own choice, so neutral.
    Local,
}

// .vmark: why you can't hear someone, as a chip; on tiles it can carry the words.
fn vmark(glyph: Glyph, words: Option<&'static str>, kind: MarkKind) -> Div {
    let base = div()
        .h(px(18.))
        .min_w(px(18.))
        .flex_none()
        .squircle(6.)
        .flex()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .whitespace_nowrap()
        .text_color(gpui_kit::white())
        .when(words.is_some(), |d| d.pl(px(4.)).pr(px(6.)))
        .when(words.is_none(), |d| d.px(px(3.)))
        .child(Icon::new(glyph).size(px(12.)))
        .children(words);
    match kind {
        MarkKind::Server => base.bg(palette::danger()),
        MarkKind::Local => base.bg(stage::bg().opacity(0.72)).border_1().border_color(stage::wash(0.16)),
    }
}

// .tile.stream: a stream before its first preview arrives.
pub(crate) fn stream_fill(radius: f32) -> Div {
    let glow = gpui_kit::linear_gradient(135., gpui_kit::linear_color_stop(Hsla::from(rgb(0x0097A1)).opacity(0.7), 0.), gpui_kit::linear_color_stop(Hsla::from(rgb(0x9D2398)).opacity(0.7), 1.));
    div().relative().squircle(radius).bg(rgb(0x1D1E39)).child(div().absolute().inset_0().squircle(radius).bg(glow))
}

// .mlabel
fn menu_label(text: &'static str, ink: &Inks) -> Div {
    div().pt(px(6.)).px(px(10.)).pb(px(2.)).text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(ink.muted).child(text)
}

// .devmenu hr
fn menu_rule(ink: &Inks) -> Div {
    div().h(px(1.)).mx(px(4.)).my(px(4.)).flex_none().bg(ink.line)
}

// .choice.vrow: a 34px menu row holding a radio or a switch.
fn choice_row(id: impl Into<ElementId>, ink: &Inks) -> Stateful<Div> {
    let hover = ink.hover;
    div().id(id).min_h(px(34.)).px(px(10.)).py(px(6.)).squircle(RADIUS_SM).flex().items_center().gap(px(10.)).cursor_pointer().hover(move |s| s.bg(hover))
}

#[derive(Clone)]
struct MenuSliderDrag(SharedString);

impl gpui_kit::Render for MenuSliderDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

pub(crate) fn tabular() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".into(), 1)]))
}

fn oklch_hue(color: Hsla) -> f32 {
    let rgba = gpui_kit::Rgba::from(color);
    let lin = |c: f32| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    let (r, g, b) = (lin(rgba.r), lin(rgba.g), lin(rgba.b));
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    let (a, bb) = (1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s, 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s);
    bb.atan2(a).to_degrees().rem_euclid(360.)
}

// A BGRA frame as the data URL Discord's preview upload takes.
fn jpeg_data_url(frame: &voice::VideoFrame) -> Option<String> {
    use base64::Engine;
    let rgb: Vec<u8> = frame.bgra.as_chunks::<4>().0.iter().flat_map(|[b, g, r, _]| [*r, *g, *b]).collect();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 80).encode(&rgb, frame.width, frame.height, image::ExtendedColorType::Rgb8).ok()?;
    Some(format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(jpeg)))
}

// Discord's CDN serves any avatar as a small PNG, whatever its original format.
fn small_png(avatar: &str) -> String {
    let base = avatar.split('?').next().unwrap_or(avatar);
    let stem = base.rsplit_once('.').map_or(base, |(stem, _)| stem);
    format!("{stem}.png?size=32")
}

async fn avatar_color(url: &str) -> Option<Hsla> {
    let bytes = reqwest::get(url).await.ok()?.bytes().await.ok()?;
    let rgba = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let (r, g, b) = average_color(rgba.as_raw())?;
    let mut color = Hsla::from(gpui_kit::Rgba { r: r as f32 / 255., g: g as f32 / 255., b: b as f32 / 255., a: 1. });
    // Dark enough for white names and the avatar to stand out, as on Discord's tiles.
    color.l = color.l.clamp(0.2, 0.45);
    Some(color)
}

// The alpha-weighted mean colour of RGBA pixels; None when fully transparent.
fn average_color(rgba: &[u8]) -> Option<(u8, u8, u8)> {
    let (mut sum, mut weight) = ([0u64; 3], 0u64);
    for px in rgba.as_chunks::<4>().0 {
        let a = px[3] as u64;
        for (total, channel) in sum.iter_mut().zip(px) {
            *total += *channel as u64 * a;
        }
        weight += a;
    }
    (weight > 0).then(|| ((sum[0] / weight) as u8, (sum[1] / weight) as u8, (sum[2] / weight) as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatar_colour_and_url() {
        assert_eq!(average_color(&[255, 0, 0, 255, 0, 0, 255, 255, 9, 9, 9, 0]), Some((127, 0, 127)));
        assert_eq!(average_color(&[1, 2, 3, 0]), None);
        assert_eq!(small_png("https://cdn.discordapp.com/avatars/1/a_b.gif?size=128"), "https://cdn.discordapp.com/avatars/1/a_b.png?size=32");
        assert_eq!(small_png("https://cdn.discordapp.com/embed/avatars/3.png"), "https://cdn.discordapp.com/embed/avatars/3.png?size=32");
        let frame = voice::VideoFrame { width: 2, height: 1, bgra: vec![0, 0, 255, 255, 255, 0, 0, 255] };
        assert!(jpeg_data_url(&frame).is_some_and(|url| url.starts_with("data:image/jpeg;base64,/9j/")));
    }
}

// "1080p 60" or "Source 30", how Waffle names a stream's quality.
fn quality_label(resolution: u32, fps: u32) -> String {
    match resolution {
        0 => format!("Source {fps}"),
        r => format!("{r}p {fps}"),
    }
}

// A running time: "4:05", or "1:02:09" past an hour.
pub(crate) fn clock(secs: u64) -> String {
    match secs / 3600 {
        0 => format!("{}:{:02}", secs / 60, secs % 60),
        h => format!("{h}:{:02}:{:02}", secs / 60 % 60, secs % 60),
    }
}

#[cfg(test)]
mod pip_tests {
    use super::*;

    #[test]
    fn clocks_and_quality() {
        assert_eq!(clock(245), "4:05");
        assert_eq!(clock(3729), "1:02:09");
        assert_eq!(quality_label(1080, 60), "1080p 60");
        assert_eq!(quality_label(0, 30), "Source 30");
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Health {
    Smooth,
    Choppy,
    Stalled,
}

// How a watched stream is doing, from when its last frames arrived: no frame for a second is
// stalled; under 15 frames a second over the last two is choppy.
// ponytail: frame rate only; add decode lag once the engine reports it.
pub fn stream_health(arrivals: &std::collections::VecDeque<Instant>, now: Instant) -> (Health, f32) {
    let Some(last) = arrivals.back() else { return (Health::Stalled, 0.) };
    let window = Duration::from_secs(2);
    let recent = arrivals.iter().filter(|t| now.duration_since(**t) <= window).count();
    let fps = recent as f32 / window.as_secs_f32();
    let health = match () {
        _ if now.duration_since(*last) > Duration::from_secs(1) => Health::Stalled,
        _ if fps < 15. => Health::Choppy,
        _ => Health::Smooth,
    };
    (health, fps)
}

#[cfg(test)]
mod health_tests {
    use super::*;

    #[test]
    fn health_from_arrivals() {
        let now = Instant::now();
        let at = |ms: u64| now - Duration::from_millis(ms);
        let smooth: std::collections::VecDeque<Instant> = (0..60).map(|i| at(i * 33)).rev().collect();
        assert_eq!(stream_health(&smooth, now).0, Health::Smooth);
        let choppy: std::collections::VecDeque<Instant> = (0..10).map(|i| at(i * 200)).rev().collect();
        assert_eq!(stream_health(&choppy, now).0, Health::Choppy);
        let stalled: std::collections::VecDeque<Instant> = [at(1500)].into();
        assert_eq!(stream_health(&stalled, now).0, Health::Stalled);
        assert_eq!(stream_health(&Default::default(), now).0, Health::Stalled);
    }
}

#[cfg(test)]
mod order_tests {
    use super::*;

    fn person(id: &str, streaming: bool, camera: bool) -> VoiceState {
        VoiceState { guild_id: None, channel_id: Some("c".into()), user_id: id.into(), user: None, muted: false, deafened: false, server_muted: false, server_deafened: false, streaming, camera }
    }

    #[test]
    fn streams_first_then_cameras_then_avatars_in_join_order() {
        // Joined in this order: a (avatar), b (streams), c (camera), d (streams, watched), e (avatar).
        let people = vec![person("a", false, false), person("b", true, false), person("c", false, true), person("d", true, false), person("e", false, false)];
        let keys: Vec<String> = order_tiles(people.clone(), &[], |id| id == "d", |s| s.camera).iter().map(tile_key).collect();
        assert_eq!(keys, ["stream:d", "stream:b", "user:c", "user:a", "user:b", "user:d", "user:e"]);
        // Someone speaking or a mute changes nothing; a new joiner lands at the end of their group.
        let mut joined = people;
        joined.push(person("f", true, false));
        let keys: Vec<String> = order_tiles(joined.clone(), &[], |id| id == "d", |s| s.camera).iter().map(tile_key).collect();
        assert_eq!(keys, ["stream:d", "stream:b", "stream:f", "user:c", "user:a", "user:b", "user:d", "user:e", "user:f"]);
        // An ended stream keeps a card up front until its streamer is live again; they may have left.
        let keys: Vec<String> = order_tiles(joined.clone(), &["gone".into()], |_| false, |s| s.camera).iter().map(tile_key).collect();
        assert_eq!(&keys[..2], ["stream:gone", "stream:b"]);
        let keys: Vec<String> = order_tiles(joined, &["b".into()], |_| false, |s| s.camera).iter().map(tile_key).collect();
        assert_eq!(keys.iter().filter(|k| *k == "stream:b").count(), 1, "live again: the stream, not the ended card");
    }
}
