// Release builds are a GUI app on Windows, so no console window opens beside it; debug builds keep it for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accounts;
mod app_commands;
mod app_modal;
mod command_line;
mod command_picker;
mod recent_commands;
mod components;
mod interactions;
mod active_now;
mod attachments;
mod app_keys;
mod crash;
mod create_channel;
mod restart;
mod feed_keys;
mod auth;
mod cache;
mod captcha;
mod query;
mod call;
mod channel_menu;
mod channel_settings;
mod dm_call;
mod dm_start;
mod chat;
mod composer_emoji;
#[cfg(debug_assertions)]
mod demo;
mod discord;
mod drafts;
mod frecency_sync;
mod drag;
mod icons;
mod illustration;
mod images;
mod layout;
mod link_preview;
mod login;
mod mascot;
mod material;
mod forum;
mod forward;
mod group_dm;
mod reactions;
mod requests;
mod gif;
mod games;
mod go_live;
mod h264;
mod home;
mod os_image;
mod notification_settings;
mod overlays;
mod overwrites;
mod playback;
mod poll;
mod inbox;
mod joining;
mod status_menu;
mod stickers;
mod memory;
mod name_fonts;
mod menu;
mod message_menu;
mod motion;
mod palette;
mod pip;
mod presence;
mod profile;
mod notify;
mod panel;
mod part;
mod rail_drag;
mod sidebar;
mod sortable;
mod search;
mod search_menu;
mod search_results;
mod server_settings;
mod settings;
mod shell;
mod user_menu;
mod video;
mod viewer;
mod hotkeys;
mod key_hook;
mod sounds;
#[cfg(target_os = "windows")]
mod tray;
mod text_file;
mod ui;
mod splash;
mod stage;
mod stage_dialogs;
mod stage_view;
mod updater;
mod design_system;
mod app_icon;
mod voice;
mod window_controls;

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use discord::{Channel, ChannelKind, Client, Emoji, Event, Folder, Gif, Guild, ListItem, ListOp, Message, Presence, Profile, ReadState, Relationship, SendState, User};
use futures::StreamExt;
use gpui_kit::{
    AppContext, Bounds, Context, Entity, FocusHandle, Hsla, RenderImage, ListAlignment, ListOffset, ListState, SharedString, Task, TitlebarOptions, Window, WindowBounds,
    WindowOptions,
    component::input::{InlineToken, InputContent, InputEvent, InputState as SearchState, TextareaState},
    px, size,
};
use gpui_motion_kit::Motions;
use icons::Assets;
use login::Login;

const TYPING_TTL: Duration = Duration::from_secs(10);
const TYPING_SEND_EVERY: Duration = Duration::from_secs(8);
const DRAFTS_SAVE_AFTER: Duration = Duration::from_millis(500);

// Counts every allocation so the memory report can show Rust's own share (see memory.rs); the
// dhat-heap build records where each one comes from instead.
#[cfg(not(feature = "dhat-heap"))]
#[global_allocator]
static ALLOCATOR: memory::Counting = memory::Counting;
#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

fn main() {
    #[cfg(target_os = "windows")]
    if !tray::claim_single_instance() {
        return;
    }
    crash::install();
    #[cfg(feature = "dhat-heap")]
    memory::start_heap_profile();
    memory::start_reporting();
    gpui_kit::application().with_assets(Assets).run(move |cx| {
        gpui_kit::init(cx);
        app_keys::init(cx);
        cx.text_system()
            .add_fonts(
                [
                    include_bytes!("../assets/fonts/Gabarito-Regular.ttf").as_slice(),
                    include_bytes!("../assets/fonts/Gabarito-Medium.ttf"),
                    include_bytes!("../assets/fonts/Gabarito-SemiBold.ttf"),
                    include_bytes!("../assets/fonts/Gabarito-Bold.ttf"),
                    include_bytes!("../assets/fonts/Gabarito-ExtraBold.ttf"),
                    include_bytes!("../assets/fonts/Figtree-Regular.ttf"),
                    include_bytes!("../assets/fonts/Figtree-Medium.ttf"),
                    include_bytes!("../assets/fonts/Figtree-SemiBold.ttf"),
                    include_bytes!("../assets/fonts/Figtree-Bold.ttf"),
                    include_bytes!("../assets/fonts/RobotoMono-Regular.ttf"),
                    include_bytes!("../assets/fonts/RobotoMono-Bold.ttf"),
                ]
                .map(std::borrow::Cow::Borrowed)
                .to_vec(),
            )
            .expect("bundled fonts");
        name_fonts::start(cx);
        // The design is drawn with `-webkit-font-smoothing: antialiased`; macOS smoothing would thicken every stroke.
        cx.text_system().set_font_smoothing(false);
        cx.set_http_client(std::sync::Arc::new(discord::Images::default()));
        cx.set_reduce_motion(material::system_reduce_motion() || settings::Prefs::load().reduce_motion);
        settings::Prefs::load().app_icon.apply();
        // WAVE_DESIGN=1 opens only the design system gallery, no sign-in needed.
        if std::env::var_os("WAVE_DESIGN").is_some() {
            design_system::open(cx);
            return;
        }

        #[cfg(debug_assertions)]
        if std::env::var("WAFFLE_DEMO").as_deref() == Ok("updating") {
            demo::updating_splash(cx);
            return;
        }
        updater::clean_up();
        let signed_in = accounts::startup_token(&settings::Prefs::load()).is_some();
        if let Some(staged) = updater::enabled().then(updater::pending).flatten() {
            install_then_open(staged, signed_in, cx);
            return;
        }
        // Signed in: a small splash window shows while the main window loads hidden behind it.
        let demo = cfg!(debug_assertions) && std::env::var_os("WAFFLE_DEMO").is_some();
        let splash = if signed_in && !demo { splash::open("Signing in…", cx) } else { None };
        open_main(splash, cx);
    });
}

// An update downloaded in the background last run goes in before anything signs in, then this run
// relaunches as the new build. A failed install opens Waffle as usual.
fn install_then_open(staged: updater::Staged, signed_in: bool, cx: &mut gpui_kit::App) {
    let splash = splash::open(format!("Installing {}…", staged.version), cx);
    cx.spawn(async move |cx| {
        match cx.background_spawn(async move { updater::install(&staged) }).await {
            // Opens the new build once this process is gone, so the two never overlap.
            Ok(()) => return cx.update(|cx| cx.restart()),
            Err(e) => eprintln!("[updater] install failed: {e}"),
        }
        cx.update(|cx| {
            if signed_in {
                return open_main(splash, cx);
            }
            // Signing in happens in the main window, so the splash steps aside, after the main
            // window exists: on Windows gpui quits when its last window closes, and closing the
            // splash first left none open for a moment, so signed-out starts quit at once.
            open_main(None, cx);
            if let Some(splash) = splash {
                splash.update(cx, |_, window, _| window.remove_window()).ok();
            }
        });
    })
    .detach();
}

fn open_main(splash: Option<gpui_kit::WindowHandle<splash::Splash>>, cx: &mut gpui_kit::App) {
    let options = WindowOptions {
        show: splash.is_none(),
        focus: splash.is_none(),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1280.), px(820.)), cx))),
        window_min_size: Some(size(px(940.), px(520.))),
        titlebar: Some(TitlebarOptions { title: Some("Waffle".into()), appears_transparent: true, traffic_light_position: Some(layout::TRAFFIC_LIGHTS) }),
        window_background: material::window_background(),
        // Every drag area moves the window and zooms on double click itself; AppKit's own titlebar
        // handling covers only its 28pt strip and changed between macOS 26 and 27.
        app_owns_titlebar_drag: true,
        ..Default::default()
    };
    let handle = gpui_kit::open_window(options, cx, |window, cx| {
        palette::sync(window, cx);
        #[cfg(target_os = "windows")]
        tray::install(window, cx);
        #[cfg(target_os = "macos")]
        keep_running_in_dock(window, cx);
        cx.new(|cx| shell::Shell::new(window, splash, cx))
    })
    .expect("open window");
    if material::setting_name().is_some() {
        // gpui-component's root paints the theme background over the whole window, hiding the native
        // material; Root's own style is applied last, so it can clear that.
        handle
            .0
            .downcast::<gpui_kit::base::Root>()
            .expect("kit root")
            .update(cx, |root, _, _| {
                use gpui_kit::{Refineable as _, prelude::*};
                root.style().refine(&gpui_kit::StyleRefinement::default().bg(gpui_kit::transparent_black()));
            })
            .expect("clear root background");
    }
    cx.activate(true);
    background_watch(cx);
    idle_sweep(cx);
}

// In view, images are only swept when a column redraws, so what was browsed before the window went
// still would stay decoded. Sweeps on a timer and hands freed heap pages back when it let go of any.
const IDLE_SWEEP: Duration = Duration::from_secs(15);

fn idle_sweep(cx: &mut gpui_kit::App) {
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(IDLE_SWEEP).await;
            if !cx.update(images::sweep_all) {
                continue;
            }
            // After the deferred image drops have run and the GPU has let go of the textures.
            cx.background_executor().timer(Duration::from_millis(100)).await;
            cx.background_executor().spawn(async { memory::compact() }).await;
        }
    })
    .detach();
}

// Background mode, unless a video is playing: minimized or closed to the tray, the app gives memory
// back at once (frees decoded images and their GPU textures, then pages everything out). Unfocused or
// fully covered, it waits `BACKGROUND_AFTER` first: fully covered it does the same, merely unfocused it
// only compacts its heaps, since paging out a window still in view would make the next click hitch.
const BACKGROUND_AFTER: Duration = Duration::from_secs(5);
// While a video plays in the background nothing announces it stopping, so check this often.
const VIDEO_CHECK: Duration = Duration::from_secs(5);

// Runs on presence changes rather than a timer (see presence::changes): redraws so GIFs stop or start
// (see gif.rs), and releases memory as above. While minimized or in the tray it only waits.
fn background_watch(cx: &mut gpui_kit::App) {
    let mut changes = presence::changes();
    cx.spawn(async move |cx| {
        // When the app went to the background, and the presence state memory was last released in:
        // each new out-of-sight state (covered, then minimized) releases again.
        let (mut since, mut released_in, mut compacted): (Option<Instant>, Option<u64>, bool) = (None, None, false);
        loop {
            let video = cx.update(|cx| video::is_playing(cx));
            let state = presence::generation();
            let mut wait = None;
            if !presence::background() {
                (since, released_in, compacted) = (None, None, false);
            } else if video {
                since = None;
                wait = Some(VIDEO_CHECK);
            } else {
                let at = *since.get_or_insert_with(Instant::now);
                let left = BACKGROUND_AFTER.saturating_sub(at.elapsed());
                if presence::out_of_sight() && (presence::hidden() || left.is_zero()) {
                    if released_in != Some(state) {
                        released_in = Some(state);
                        release_memory(cx).await;
                    }
                } else if !left.is_zero() {
                    wait = Some(left);
                } else if !std::mem::replace(&mut compacted, true) {
                    cx.background_executor().spawn(async { memory::compact() }).await;
                }
            }
            let changed = match wait {
                None => match changes.next().await {
                    Some(()) => true,
                    None => return,
                },
                Some(wait) => {
                    let timer = cx.background_executor().timer(wait);
                    futures::select_biased! {
                        change = changes.next() => match change {
                            Some(()) => true,
                            None => return,
                        },
                        _ = futures::FutureExt::fuse(timer) => false,
                    }
                }
            };
            // Redraw so GIFs stop or start, but not for going out of sight: that would reload what was just freed.
            if changed && !presence::out_of_sight() {
                cx.update(|cx| cx.refresh_windows());
            }
        }
    })
    .detach();
}

// Frees decoded images (RAM and GPU), lets GIF players notice and drop their frames, then pages the
// rest out. Downloaded image files stay cached; paging takes them out of RAM too.
async fn release_memory(cx: &mut gpui_kit::AsyncApp) {
    cx.update(release_caches);
    cx.background_executor().timer(Duration::from_millis(100)).await;
    cx.background_executor()
        .spawn(async {
            let before = memory::process().map_or(0, |p| p.private_working_set);
            memory::trim();
            let after = memory::process().map_or(0, |p| p.private_working_set);
            if std::env::var_os("WAFFLE_MEMORY").is_some() {
                eprintln!("background: gave back memory, {} MB -> {} MB", before >> 20, after >> 20);
            }
        })
        .await;
}

// Decoded images and their GPU textures; GIF players see them go and drop their frames. Whatever is
// on screen decodes again from the download cache when next drawn.
fn release_caches(cx: &mut gpui_kit::App) {
    images::clear_all(cx);
}

// macOS: closing the window hides Waffle and keeps it signed in; the Dock icon or ⌘Tab brings it
// back and ⌘Q quits. Minimized, hidden or fully covered, it frees its caches and hands freed heap
// pages back (there is no working set to empty, the system compresses idle pages itself), unless a
// video is playing. Shown again, it redraws from scratch: the cached app view still points at the
// freed images' atlas tiles.
#[cfg(target_os = "macos")]
fn keep_running_in_dock(window: &mut gpui_kit::Window, cx: &mut gpui_kit::App) {
    window.on_window_should_close(cx, |_, cx| {
        cx.hide();
        false
    });
    let released = std::rc::Rc::new(std::cell::Cell::new(false));
    window
        .observe_window_visibility(move |visibility, window, cx| {
            if visibility.is_visible() {
                if released.replace(false) {
                    window.refresh();
                }
                return;
            }
            if video::is_playing(cx) || released.replace(true) {
                return;
            }
            release_caches(cx);
            cx.spawn(async move |cx| {
                // After the deferred image drops have run and the GPU has let go of the textures.
                cx.background_executor().timer(Duration::from_millis(100)).await;
                cx.background_executor().spawn(async { memory::compact() }).await;
            })
            .detach();
        })
        .detach();
}

enum Status {
    SignedOut(Login),
    Connecting,
    Online,
    Reconnecting(SharedString),
}

// One channel's messages, kept after you leave it so coming back is instant.
#[derive(Default)]
struct Feed {
    // Shared, copied on write: each render hands the list a cheap clone of the pointer, and a
    // change copies the messages only if the last frame still holds them.
    messages: Arc<Vec<Message>>,
    loaded: bool,
    fetching: bool,
    has_more: bool,
    loading_older: bool,
    error: Option<SharedString>,
}

enum RightPanel {
    Members,
    // Every Activity card, from the section's chevron.
    Activity,
    Search(Box<search_results::Results>),
    Hidden,
}

#[derive(Clone)]
struct Typer {
    user_id: String,
    name: String,
    until: Instant,
}

#[derive(Clone)]
struct Suggestion {
    label: String,
    insert: String,
    // The wire form substituted on send: `<@id>`, `<#id>` or `<:name:id>`.
    wire: String,
    lead: SuggestLead,
    // A muted note at the row's end: the server a custom emoji comes from.
    detail: Option<String>,
}

#[derive(Clone)]
enum SuggestLead {
    Avatar(String),
    Channel,
    Emoji(discord::Emoji),
}

struct Autocomplete {
    token: String,
    items: Vec<Suggestion>,
    selected: usize,
}

// Where Waffle keeps its files: Application Support on macOS, %APPDATA% on Windows,
// $XDG_DATA_HOME (or ~/.local/share) elsewhere.
fn data_dir() -> Option<std::path::PathBuf> {
    let var = |name| std::env::var_os(name).map(std::path::PathBuf::from);
    let dir = if cfg!(target_os = "macos") {
        var("HOME")?.join("Library/Application Support/Waffle")
    } else if cfg!(target_os = "windows") {
        var("APPDATA")?.join("Waffle")
    } else {
        var("XDG_DATA_HOME").or_else(|| var("HOME").map(|h| h.join(".local/share")))?.join("waffle")
    };
    Some(dir)
}

// The quietest mic level the meters show, in dBFS.
const SELF_VIEW_INTERVAL: Duration = Duration::from_millis(66);
const SPLASH_BEAT: Duration = Duration::from_millis(320);
// The splash never outstays this: the main window opens even if Discord is slow to answer.
const SPLASH_LIMIT: Duration = Duration::from_secs(25);
const SILENCE_DB: f32 = -100.;

// A voice channel we're in. Dropping it leaves the channel.
struct Call {
    session: voice::VoiceSession,
    guild_id: Option<String>,
    channel: Channel,
    connected: bool,
    stage: voice::Stage,
    stats: Option<voice::Stats>,
    // Our processed mic level and whether the voice gate is open.
    level: (f32, bool),
    speaking: HashSet<String>,
    // When each person last stopped talking, for Discord's tile order.
    last_spoke: HashMap<String, Instant>,
    // Latest camera frame per user, ours included.
    cameras: HashMap<String, Arc<RenderImage>>,
    camera_on: bool,
    // Go Lives we're watching, in the order we started them.
    watching: Vec<Watching>,
    // The stream shown large in Focus layout; the first watched when unset.
    focus: Option<String>,
    // Watched streams that ended, by streamer: each keeps a card until closed, and rejoins if they go live again.
    ended: Vec<String>,
    // Our own Go Live; dropping it ends the stream.
    sharing: Option<voice::GoLive>,
    // What we're sharing, as the Go Live panel names it ("Screen 1", an app, a window).
    share_label: Option<String>,
    // The source behind `sharing`, kept so a quality change can restart the same stream.
    share_source: Option<voice::ShareSource>,
    live_since: Option<Instant>,
    // When we joined, for the call header's timer.
    joined: Instant,
    // Our own Go Live from its local frames, for the panel thumbnail and the self-view.
    self_view: Option<(Arc<RenderImage>, Instant)>,
    // Discord's AV error conditions now holding, by user and code: the voice engine's, and the
    // ready timeouts the app keeps (a camera's under "camera:{id}").
    issues: HashSet<(String, u16)>,
    _events: Task<()>,
}

impl Call {
    fn watched(&self, streamer_id: &str) -> Option<&Watching> {
        self.watching.iter().find(|w| w.streamer_id == streamer_id)
    }

    // The stream shown large: the focused one, else the first watched.
    fn focused(&self) -> Option<&Watching> {
        self.focus.as_deref().and_then(|id| self.watched(id)).or(self.watching.first())
    }

    fn unwatch(&mut self, streamer_id: &str) -> Option<Watching> {
        let at = self.watching.iter().position(|w| w.streamer_id == streamer_id)?;
        self.issues.remove(&(format!("stream:{streamer_id}"), voice::issues::RECEIVER_READY_TIMEOUT));
        if self.focus.as_deref() == Some(streamer_id) {
            self.focus = None;
        }
        Some(self.watching.remove(at))
    }
}

// A call to offer back: where it was, and when it ended.
#[derive(Clone)]
pub(crate) struct VoiceNotice {
    guild_id: Option<String>,
    channel: Channel,
    at: Instant,
    why: NoticeWhy,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoticeWhy {
    // Another client of ours joined voice and took the call.
    Elsewhere,
    // Waffle restarted mid-call; the call is offered back rather than rejoined unasked.
    Restarted,
}

// How long after a call ends another client's voice state still explains why.
const TAKEN_ELSEWHERE_WINDOW: Duration = Duration::from_secs(10);

// Someone's Go Live we're watching. Dropping it stops the stream.
pub(crate) struct Watching {
    pub(crate) streamer_id: String,
    pub(crate) frame: Option<Arc<RenderImage>>,
    // When recent frames arrived, for the header's health dot.
    pub(crate) arrivals: std::collections::VecDeque<Instant>,
    // None for our own Go Live, shown from its local frames: watching it through Discord would
    // end it on stop, since leaving a stream and ending one are the same gateway op.
    watch: Option<voice::StreamWatch>,
    pub(crate) state: WatchState,
    // Reconnects in a row without a frame in between, so a stream that keeps dropping fails instead
    // of retrying forever.
    rejoins: u8,
}

// Discord's active-stream states (its web client, 2026-11-21), less ENDED, which is `Call::ended`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum WatchState {
    // Since when: Discord fails a watch that hasn't connected in 5 s (12 s on a Stage).
    Connecting(Instant),
    Active,
    Reconnecting,
    // With Discord's error code, shown on the tile.
    Failed(u16),
}

// Discord's AV error codes for watching (its web client's error table, 2026-11-21).
pub(crate) const STREAM_FAILED_TO_START: u16 = 2001;
pub(crate) const STREAM_VIEW_LOW_FPS: u16 = 2002;
pub(crate) const STREAM_RECONNECTING: u16 = 2007;

const WATCH_TIMEOUT: Duration = Duration::from_secs(5);
const STAGE_WATCH_TIMEOUT: Duration = Duration::from_secs(12);
const STAGE_CHANNEL: u8 = 13;
// A dropped connection is rejoined after this long, this many times in a row.
const REJOIN_AFTER: Duration = Duration::from_secs(2);
const MAX_REJOINS: u8 = 3;
// The streamer's voice state stops saying they're live while we still watch: Discord closes the
// stream this long after, unless they're live again.
const LIVE_LAPSE: Duration = Duration::from_secs(180);

impl Watching {
    pub(crate) fn new(streamer_id: String, frame: Option<Arc<RenderImage>>, watch: Option<voice::StreamWatch>) -> Self {
        let state = if watch.is_some() { WatchState::Connecting(Instant::now()) } else { WatchState::Active };
        Self { streamer_id, frame, arrivals: Default::default(), watch, state, rejoins: 0 }
    }

    fn ended(&self) -> bool {
        self.watch.as_ref().is_some_and(voice::StreamWatch::ended)
    }
}

// A profile popout opened from an avatar or name, at the click point.
struct ProfileCard {
    user: User,
    guild_id: Option<String>,
    at: gpui_kit::Point<gpui_kit::Pixels>,
    profile: Option<Profile>,
    // Every activity card, after "Show more".
    all_activity: bool,
}

// Where a picked emoji goes.
#[derive(Clone)]
enum EmojiTarget {
    Composer,
    Reaction(Box<Message>),
    CustomStatus,
    // An answer's emoji in the poll creator, by row.
    PollAnswer(usize),
    ForwardNote,
}

#[derive(Clone, Copy, PartialEq)]
enum Edge {
    Channels,
    Panel,
    // The nav column of the settings views (app, server, channel), one width for all three.
    Settings,
}

struct Drag {
    edge: Edge,
    start_x: f32,
    start_width: f32,
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum Part {
    Pill(String),
    RailDm(String),
    Fold(String),
    Chevron(String),
    Banner,
}

type ComposerState = TextareaState;

pub struct Waffle {
    status: Status,
    client: Option<Client>,
    // The connected account's token, filed under its id once READY names the user.
    token: Option<String>,
    _session: Option<Task<()>>,
    me: Option<User>,
    my_status: discord::Status,
    custom_status: Option<discord::CustomStatus>,
    // When a status picked "For 1 hour" and the like falls back to Online.
    status_until: Option<chrono::DateTime<chrono::Local>>,
    status_editor: Option<status_menu::StatusEditor>,
    poll_creator: Option<poll::PollCreator>,
    // Answers picked on a poll before Vote, and polls showing their results before you vote, by message.
    poll_picks: HashMap<String, Vec<u32>>,
    poll_peek: HashSet<String>,
    forward_dialog: Option<forward::ForwardDialog>,
    // Our interactions in flight, by the nonce we sent.
    interactions: HashMap<String, interactions::PendingInteraction>,
    command_picker: command_picker::CommandPicker,
    component_state: components::ComponentState,
    app_modal: Option<app_modal::AppModal>,
    group_dialog: Option<group_dm::GroupDialog>,
    status_flyout: status_menu::Flyout,
    status_input: Entity<SearchState>,
    clear_after: status_menu::ClearAfter,
    guilds: Vec<Guild>,
    folders: Vec<Folder>,
    open_folders: HashSet<String>,
    // The dragged server has left the folder it came from this drag; that folder folds shut too.
    rail_left_folder: bool,
    // Reorderable lists by drag (sortable.rs): the rail and its folders, the channel list, roles.
    rail_board: sortable::Board,
    channel_board: sortable::Board,
    role_board: sortable::Board,
    // Where a dragged channel would land: the row's id and above/below it.
    // What a drag in the rail or channel list carries, and what it is landing.
    drag_motion: drag::DragMotion,
    // Every user seen as their server shows them, keyed by guild then user, for voice rows and tiles.
    members: HashMap<String, HashMap<String, User>>,
    dms: Vec<Channel>,
    // DMs waiting in Message Requests, requests and spam both; kept out of `dms` until accepted.
    dm_requests: Vec<Channel>,
    // None shows direct messages.
    guild_id: Option<String>,
    channel: Option<Channel>,
    last_channel: HashMap<Option<String>, String>,
    feeds: HashMap<String, Feed>,
    list: ListState,
    // Where each channel left scrolled up was read from (a message id and how far into it), so coming
    // back opens there; a channel left at the bottom has none and opens on the newest message.
    scroll_anchors: HashMap<String, (String, gpui_kit::Pixels)>,
    read_states: HashMap<String, ReadState>,
    last_message: HashMap<String, u64>,
    presences: HashMap<String, Presence>,
    member_roles: HashMap<String, HashMap<String, Vec<String>>>,
    member_lists: HashMap<String, Vec<ListItem>>,
    member_groups: HashMap<String, HashMap<String, u32>>,
    requested_members: HashSet<(String, String)>,
    typing: HashMap<String, Vec<Typer>>,
    last_typing_sent: Option<Instant>,
    right_panel: RightPanel,
    composer: Entity<ComposerState>,
    search_input: Entity<SearchState>,
    // The search popover: the keyboard's row, whether More filters is open, the calendar's day.
    search_pick: Option<usize>,
    search_more: bool,
    search_day: chrono::NaiveDate,
    has_draft: bool,
    // Each channel's unsent composer, and its pending write to disk.
    drafts: drafts::Drafts,
    drafts_save: Option<Task<()>>,
    send_error: Option<SharedString>,
    replying: Option<Message>,
    // Discord's "@ ON / OFF" on the reply bar: whether the reply pings its author.
    reply_ping: bool,
    // Files picked, pasted or dropped, waiting to go out with the next message.
    pending_files: Vec<attachments::PendingFile>,
    // Each pending file's tile picture by path; None once the OS had none to give.
    file_previews: HashMap<PathBuf, Option<Arc<RenderImage>>>,
    // Uploads in flight or failed, by the nonce of the message carrying them, kept for Resend.
    uploads: HashMap<String, UploadJob>,
    // Each running upload's task by nonce; dropping it (Cancel, Delete) stops the bytes going out.
    upload_tasks: HashMap<String, Task<()>>,
    talk_key_down: bool,
    prefs: settings::Prefs,
    settings: Option<settings::SettingsView>,
    channel_settings: Option<channel_settings::ChannelSettings>,
    server_settings: Option<server_settings::view::ServerSettings>,
    settings_search: Entity<SearchState>,
    hotkeys: Option<hotkeys::Hotkeys>,
    // Keybinds the system refused because another app holds them.
    keybind_errors: Vec<String>,
    // Our mic level in dBFS, from the call or the mic test.
    mic_level: f32,
    mic_test: Option<settings::MicTestRun>,
    // Each slider's last painted bounds, so a click or drag maps to a value.
    slider_bounds: Rc<RefCell<HashMap<SharedString, gpui_kit::Bounds<gpui_kit::Pixels>>>>,
    // The open voice channel's call view replaces the chat.
    call_view: bool,
    connection_info: bool,
    device_menu: Option<call::DeviceMenu>,
    menu: Option<menu::OpenMenu>,
    // Go Live thumbnails by streamer: the CDN url and when it was fetched.
    stream_previews: HashMap<String, (String, Instant)>,
    // Forum channels' archived posts and post previews, by forum id.
    forums: HashMap<String, forum::ForumState>,
    developer_mode: bool,
    account_settings: crate::discord::AccountSettings,
    my_account: crate::discord::MyAccount,
    // Notification settings per server, "@me" for DMs.
    guild_settings: HashMap<String, discord::GuildSettings>,
    ignored: HashSet<String>,
    // A DM call we started: ring them once the call connects.
    ring_on_connect: Option<String>,
    // The DM call we're ringing out on; its ringtone plays until someone joins, we leave, or a minute passes.
    ringing_out: Option<String>,
    // DM and group calls going on now, by channel, with who each still rings.
    dm_calls: HashMap<String, Vec<String>>,
    // Who is in each DM call: voice states with no server.
    dm_voice: Vec<discord::VoiceState>,
    // The DM call ringing us; its ringtone plays until we answer or decline, or it stops ringing.
    ringing_in: Option<String>,
    // The DM call we just left, its length, and when: its window reads "Call ended" briefly (dm_call.rs).
    dm_call_ended: Option<(String, u64, Instant)>,
    // A video call we started: the camera turns on once it connects.
    camera_on_connect: bool,
    // Voice channels whose member list is collapsed (Discord's "Hide Names").
    hidden_voice_names: HashSet<String>,
    // Shift held: Discord's hover bar shows Delete instead of More.
    shift_held: bool,
    // The stream hover card under the pointer, which keeps it open off the row.
    stream_card_hover: Option<String>,
    // The open stream card's bounds as last drawn, the pointer's tunnel toward it, and the row whose
    // card the tunnel is holding open.
    stream_card_bounds: Rc<std::cell::Cell<gpui_kit::Bounds<gpui_kit::Pixels>>>,
    stream_tunnel: ui::Tunnel,
    stream_hold: Option<String>,
    activity_feed: Vec<discord::FeedEntry>,
    // A stream to start watching as soon as the call we're joining connects.
    watch_on_connect: Option<String>,
    // The window is in front; notifications for the open channel are skipped then.
    window_active: bool,
    // The open channel should land on its first unread once its messages are in.
    unread_scroll_pending: bool,
    // The channel last marked unread by hand: auto-read leaves it alone until it's opened again or we send there.
    // Not verified: Discord's exact rule for when the open channel reads itself again after Mark Unread.
    pub(crate) held_unread: Option<String>,
    dialog: Option<menu::Dialog>,
    create_channel: Option<create_channel::CreateChannel>,
    notification_settings: Option<notification_settings::NotificationSettings>,
    stage_topic: Option<stage_dialogs::StageTopic>,
    dialog_input: Entity<SearchState>,
    // A short notice at the bottom, e.g. why a moderation action failed.
    toast: Option<SharedString>,
    // The Friends page's Add friend tab: the username field and how the last request went.
    add_friend_input: Entity<SearchState>,
    add_friend_note: Entity<TextareaState>,
    add_friend_result: Option<Result<SharedString, SharedString>>,
    // Set while a one-second repaint runs for Active Now's play clocks.
    active_now_ticking: Rc<std::cell::Cell<bool>>,
    // The gateway refused the token once and REST accepted it; set until the next READY.
    auth_retried: bool,
    // The background updater: its status (a staged update installs on Restart or the next launch).
    updates: updater::Updates,
    // The Go Live picker: None closed, Some(None) loading, Some(Some(..)) listed.
    share_picker: Option<go_live::SharePicker>,
    // The source picked in the Go Live window, waiting for "Go Live".
    share_choice: Option<voice::ShareSource>,
    share_choice_label: Option<String>,
    // Who watches each Go Live we've heard of, by stream key (STREAM_UPDATE).
    stream_viewers: HashMap<String, Vec<String>>,
    // Go Lives their streamer paused, by stream key (STREAM_UPDATE).
    stream_paused: HashSet<String>,
    // When we first saw each Go Live by stream key; Discord sends no start time.
    stream_live_since: HashMap<String, Instant>,
    // When the pointer last moved over the call view; the bar and header fade out after a while.
    call_pointer_at: Instant,
    call_pointer_inside: bool,
    // On the stage's own chrome (controls, header buttons): it stays up while the pointer rests there.
    call_pointer_on_chrome: bool,
    call_strip_hidden: bool,
    // Our stream's self-view: shown, and which corner of the stage it sits in.
    // Our local volume for each streamer's Go Live (0..=2), kept across watches.
    stream_volumes: HashMap<String, f32>,
    // Where a released self-view glides from, and when it let go.
    // The launch window while the hidden main window signs in and loads; None once it's gone.
    splash: Option<gpui_kit::WindowHandle<splash::Splash>>,
    main_window: gpui_kit::AnyWindowHandle,
    // A server joined from an invite, opened when its GUILD_CREATE arrives.
    joining: Option<String>,
    // Bumped on each Go Live so an earlier stream's preview uploads stop.
    preview_generation: u64,
    // The call view filling the screen on its own, as Discord's full screen does.
    call_fullscreen: bool,
    // The voice channel's own chat, open beside the call.
    call_chat: bool,
    // Voice channel → when its current session began (Unix seconds), from CHANNEL_INFO.
    voice_start: HashMap<String, u64>,
    // Servers whose channel info we've asked for since connecting.
    channel_info_requested: HashSet<String>,
    // Each call participant's avatar colour for their tile; None while loading or when it failed.
    avatar_colors: HashMap<String, Option<Hsla>>,
    pins: Option<Option<Result<Vec<Message>, SharedString>>>,
    // Messages whose spoilers were clicked open.
    revealed: Rc<RefCell<HashSet<String>>>,
    text_files: Rc<RefCell<text_file::TextFiles>>,
    relationships: Vec<Relationship>,
    friends_tab: home::FriendsTab,
    // Your recent mentions once fetched (None while loading); kept after the popover closes for the page.
    inbox: Option<Option<Result<Vec<Message>, SharedString>>>,
    // Your saved-for-later messages once fetched for the Inbox's Saved tab, and the ids of every saved
    // one (loaded on connect) for Save's label. Rc so each render's row context shares it.
    saved: Option<Option<Result<Vec<Message>, SharedString>>>,
    saved_ids: std::rc::Rc<HashSet<String>>,
    // A reaction frecency save is waiting out FRECENCY_SAVE_DELAY, so a burst of reactions sends once.
    frecency_save_pending: bool,
    // The inbox popover, rising from the rail's Inbox tile.
    inbox_open: bool,
    inbox_tab: inbox::InboxTab,
    // A jump waiting for the opened channel's history to land.
    pending_jump: Option<(String, String)>,
    // Where the pins popover or a reaction's emoji picker was opened from, so it rises from its trigger.
    popover_at: gpui_kit::Point<gpui_kit::Pixels>,
    // The full Inbox page in the content column ("See all").
    inbox_page: bool,
    // The Message Requests page, on its tab; None while it's closed.
    requests_page: Option<requests::RequestsTab>,
    // Joining a server: invite screen, onboarding, rules, Channels & Roles and the Server Guide.
    join: joining::Join,
    // A server page open in the content column instead of a channel (Server Guide, Channels & Roles).
    guild_page: Option<joining::GuildPage>,
    // The GIF picker: None closed, Some(None) loading, Some(Some(..)) results.
    gif_picker: Option<Option<Result<Vec<Gif>, SharedString>>>,
    // Which tab the GIF picker shows; stickers share its frame.
    picker_tab: stickers::PickerTab,
    sticker_packs: Option<Result<Vec<discord::StickerPack>, SharedString>>,
    sticker_search: Entity<SearchState>,
    gif_search: Entity<SearchState>,
    // The channel open when Waffle last quit, reopened once the gateway is ready.
    restore: Option<(Option<String>, String)>,
    editing: Option<Message>,
    mention_wire: HashMap<String, String>,
    autocomplete: Option<Autocomplete>,
    // The suggestion list's scroll, kept on the highlighted row.
    autocomplete_scroll: gpui_kit::ScrollHandle,
    channels_scroll: gpui_kit::ScrollHandle,
    rail_scroll: gpui_kit::ScrollHandle,
    panel_scroll: gpui_kit::ScrollHandle,
    flash: Option<String>,
    muted: bool,
    deafened: bool,
    call: Option<Call>,
    voice_error: Option<SharedString>,
    // The banner over the main column after another client of ours took the call.
    voice_notice: Option<VoiceNotice>,
    // The call that just ended, for a notice that explains it after the fact.
    last_call: Option<VoiceNotice>,
    // Our gateway session, to tell our voice state apart from another client's.
    gateway_session: String,
    // The call connecting now replaced one in the same server (our pick or a moderator's move):
    // it ended that one without a disconnect, and connecting sounds as a move.
    call_switched: bool,
    viewer: Option<viewer::Viewer>,
    profile_card: Option<ProfileCard>,
    status_menu: bool,
    // The channel or DM row under the pointer, which shows its hover-only controls.
    hovered_row: Option<String>,
    // The message under the pointer, which shows its toolbar and, on a continuation, its time.
    hovered_message: Option<String>,
    // The message with keyboard focus in the feed (feed_keys.rs); kept only while `feed_focus` holds focus.
    focused_message: Option<String>,
    feed_focus: FocusHandle,
    // The call card shown big (`call::tile_key`), or None for the even grid.
    call_focus: Option<String>,
    // The call stage's measured card area, which the grid is fitted to.
    call_area: Rc<std::cell::Cell<(f32, f32)>>,
    // Profiles by user and server, shown at once from the last fetch while a fresh one loads.
    profiles: query::Query<discord::Profile>,
    // Previews of Discord's own links (messages, invites), shared with the rows that draw them.
    link_previews: link_preview::Previews,
    // Who reacted, by message and emoji (`Emoji::path`), fetched on a pill's first hover; dropped when it changes.
    reactors: HashMap<(String, String), reactions::Reactors>,
    reactions_dialog: Option<reactions::ReactionsDialog>,
    games: games::Games,
    emoji_picker: Option<EmojiTarget>,
    emoji_search: Entity<SearchState>,
    switcher: Option<usize>,
    switcher_input: Entity<SearchState>,
    // The read position when the open channel was opened: messages after it sit under the NEW line.
    unread_marker: Option<u64>,
    // Scrolled well above the newest message: shows Discord's "Jump to Present" bar.
    scrolled_up: bool,
    // The floating composer's height, measured as it paints; the feed keeps that much room at its foot.
    composer_height: Rc<std::cell::Cell<f32>>,
    // The header pin button's centre, where Pinned Messages opens from wherever it's asked for.
    pins_button_x: Rc<std::cell::Cell<f32>>,
    pins_list: ListState,
    pip: pip::Pip,
    copied: Option<String>,
    focus: FocusHandle,
    channels_width: f32,
    settings_width: f32,
    // The settings views' nav list; one is open at a time.
    settings_nav_scroll: gpui_kit::ScrollHandle,
    panel_width: f32,
    drag: Option<Drag>,
    back: Vec<(Option<String>, Channel)>,
    forward: Vec<(Option<String>, Channel)>,
    folded: HashSet<String>,
    hovered: Option<String>,
    // The member row under the pointer: its avatar and decoration animate, as Discord's do.
    member_hover: Option<String>,
    motions: Motions<Part>,
    // The DM tiles on the rail with their counts, leaving ones included until their exit settles; None until
    // DMs first load, so the ones there at launch never animate in.
    rail_dms_shown: Option<Vec<(Channel, u32)>>,
    // Live messages pop in; history never animates. Shared with the list's row closure.
    arrivals: Rc<RefCell<Arrivals>>,
    parts: part::Parts,
}

#[derive(Default)]
struct Arrivals {
    fresh: HashSet<String>,
    motions: Motions<String>,
}

impl Waffle {
    fn new(window: &mut Window, splash: Option<gpui_kit::WindowHandle<splash::Splash>>, cx: &mut Context<Self>) -> Self {
        cx.observe_window_appearance(window, |_, window, cx| palette::sync(window, cx)).detach();
        cx.observe_window_activation(window, |this: &mut Self, window, cx| {
            this.window_active = window.is_window_active();
            // Coming back to the newest message of the open channel reads it.
            this.ack_open(cx);
            cx.notify();
        })
        .detach();
        #[allow(unused_mut)]
        let mut waffle = Self::build(window, splash, cx);
        layout::apply_zoom(window, waffle.prefs.zoom, cx);
        let main = restart::Main(cx.entity().downgrade());
        cx.set_global(main);
        waffle.watch_for_updates(cx);
        cx.on_focus_out(&waffle.feed_focus, window, |this, _, _, cx| {
            this.focused_message = None;
            cx.notify();
        })
        .detach();
        #[cfg(debug_assertions)]
        if let Ok(scene) = std::env::var("WAFFLE_DEMO") {
            demo::load(&mut waffle, &scene, window, cx);
            return waffle;
        }
        waffle.start_hotkeys(cx);
        match accounts::startup_token(&waffle.prefs) {
            Some(token) => waffle.connect(token, cx),
            None => waffle.sign_in(cx),
        }
        if waffle.splash.is_some() {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(SPLASH_LIMIT).await;
                this.update(cx, |this, cx| this.finish_splash(cx)).ok();
            })
            .detach();
        }
        waffle
    }

    // Everything one signed-in session owns; switching accounts builds it afresh.
    fn build(window: &mut Window, splash: Option<gpui_kit::WindowHandle<splash::Splash>>, cx: &mut Context<Self>) -> Self {
        let composer = cx.new(|cx| {
            let mut state = ComposerState::new(window, cx).placeholder("Message").submit_on_enter(true);
            state.set_auto_grow(1, 10, cx);
            state
        });
        cx.subscribe_in(&composer, window, Self::on_composer).detach();
        cx.observe(&composer, |this, _, cx| this.on_composer_selection(cx)).detach();
        let search_input = cx.new(|cx| SearchState::new(window, cx).placeholder("Search"));
        cx.subscribe_in(&search_input, window, Self::on_search_input).detach();
        // Typing filters the picker and switcher; the views read the inputs' text on render.
        let emoji_search = cx.new(|cx| SearchState::new(window, cx).placeholder("Find the perfect emoji"));
        cx.subscribe_in(&emoji_search, window, Self::on_emoji_search).detach();
        let feed_focus = cx.focus_handle();
        // Esc closing a focused popup (search, ⌘K) leaves nothing focused, and app keys only reach the root
        // through the focus path, so take focus back.
        cx.on_focus_lost(window, |this, window, cx| this.focus.focus(window, cx)).detach();
        let switcher_input = cx.new(|cx| SearchState::new(window, cx).placeholder("Where would you like to go?"));
        let gif_search = cx.new(|cx| SearchState::new(window, cx).placeholder("Search Tenor"));
        cx.subscribe_in(&gif_search, window, Self::on_gif_search).detach();
        let sticker_search = cx.new(|cx| SearchState::new(window, cx).placeholder("Search stickers"));
        cx.subscribe_in(&sticker_search, window, Self::on_sticker_search).detach();
        cx.subscribe_in(&switcher_input, window, Self::on_switcher_input).detach();
        let settings_search = cx.new(|cx| SearchState::new(window, cx).placeholder("Search settings"));
        let dialog_input = cx.new(|cx| SearchState::new(window, cx));
        // Enter in the field submits its dialog; the input keeps the key from reaching the dialog itself.
        cx.subscribe_in(&dialog_input, window, |this: &mut Self, _, event: &InputEvent, window, cx| {
            if this.stage_topic.is_some() {
                return this.stage_topic_input(event, window, cx);
            }
            if !matches!(event, InputEvent::PressEnter { .. }) {
                return;
            }
            if this.create_channel.is_some() {
                return this.submit_create_channel(cx);
            }
            if this.dialog.as_ref().is_some_and(|d| d.input) {
                this.confirm_dialog(window, cx);
            }
        })
        .detach();
        let status_input = cx.new(|cx| SearchState::new(window, cx).placeholder("What's on your mind?"));
        cx.subscribe_in(&status_input, window, Self::on_status_input).detach();
        let add_friend_input = cx.new(|cx| SearchState::new(window, cx).placeholder("username"));
        cx.subscribe_in(&add_friend_input, window, Self::on_add_friend_input).detach();
        let add_friend_note = cx.new(|cx| TextareaState::new(window, cx).placeholder("Say hi, or how you know each other"));
        cx.observe(&add_friend_note, |_, _, cx| cx.notify()).detach();
        cx.subscribe(&settings_search, |this: &mut Self, _, _: &InputEvent, cx| {
            // A new query starts at its best result.
            if let Some(view) = this.settings.as_mut() {
                view.reset_result();
            }
            cx.notify();
        })
        .detach();
        let mut waffle = Self {
            status: Status::Connecting,
            client: None,
            token: None,
            _session: None,
            me: None,
            my_status: discord::Status::Online,
            custom_status: None,
            status_until: None,
            status_editor: None,
            poll_creator: None,
            poll_picks: HashMap::new(),
            poll_peek: HashSet::new(),
            forward_dialog: None,
            interactions: HashMap::new(),
            command_picker: command_picker::CommandPicker::load(),
            component_state: Default::default(),
            app_modal: None,
            group_dialog: None,
            status_flyout: status_menu::Flyout::default(),
            status_input,
            clear_after: status_menu::ClearAfter::default(),
            guilds: Vec::new(),
            folders: Vec::new(),
            open_folders: HashSet::new(),
            rail_left_folder: false,
            rail_board: Default::default(),
            channel_board: Default::default(),
            role_board: Default::default(),
            drag_motion: Default::default(),
            members: HashMap::new(),
            dms: Vec::new(),
            dm_requests: Vec::new(),
            guild_id: None,
            channel: None,
            last_channel: HashMap::new(),
            feeds: HashMap::new(),
            list: ListState::new(0, ListAlignment::Bottom, px(600.)),
            scroll_anchors: HashMap::new(),
            read_states: HashMap::new(),
            last_message: HashMap::new(),
            presences: HashMap::new(),
            member_roles: HashMap::new(),
            member_lists: HashMap::new(),
            member_groups: HashMap::new(),
            requested_members: HashSet::new(),
            typing: HashMap::new(),
            last_typing_sent: None,
            right_panel: RightPanel::Members,
            composer,
            search_input,
            search_pick: None,
            search_more: false,
            search_day: search_menu::today(),
            has_draft: false,
            drafts: drafts::Drafts::load(),
            drafts_save: None,
            send_error: None,
            replying: None,
            reply_ping: true,
            pending_files: Vec::new(),
            file_previews: HashMap::new(),
            uploads: HashMap::new(),
            upload_tasks: HashMap::new(),
            talk_key_down: false,
            prefs: settings::Prefs::load(),
            settings: None,
            channel_settings: None,
            server_settings: None,
            settings_search,
            hotkeys: None,
            keybind_errors: Vec::new(),
            mic_level: SILENCE_DB,
            mic_test: None,
            slider_bounds: Rc::default(),
            call_view: false,
            connection_info: false,
            device_menu: None,
            menu: None,
            stream_previews: HashMap::new(),
            forums: HashMap::new(),
            developer_mode: false,
            account_settings: Default::default(),
            my_account: Default::default(),
            guild_settings: HashMap::new(),
            ignored: HashSet::new(),
            ring_on_connect: None,
            ringing_out: None,
            dm_calls: HashMap::new(),
            dm_voice: Vec::new(),
            ringing_in: None,
            dm_call_ended: None,
            camera_on_connect: false,
            hidden_voice_names: HashSet::new(),
            shift_held: false,
            stream_card_hover: None,
            stream_card_bounds: Rc::default(),
            stream_tunnel: ui::Tunnel::default(),
            stream_hold: None,
            activity_feed: Vec::new(),
            watch_on_connect: None,
            window_active: true,
            unread_scroll_pending: false,
            held_unread: None,
            dialog: None,
            create_channel: None,
            notification_settings: None,
            stage_topic: None,
            dialog_input,
            toast: None,
            add_friend_input,
            add_friend_note,
            add_friend_result: None,
            active_now_ticking: Rc::default(),
            auth_retried: false,
            share_picker: None,
            share_choice: None,
            share_choice_label: None,
            stream_viewers: HashMap::new(),
            stream_paused: HashSet::new(),
            stream_live_since: HashMap::new(),
            call_pointer_at: Instant::now(),
            call_pointer_inside: false,
            call_pointer_on_chrome: false,
            // Discord's focused view hides everyone else behind the pill on the stream's bottom edge.
            call_strip_hidden: true,
            preview_generation: 0,
            splash,
            main_window: window.window_handle(),
            joining: None,
            stream_volumes: HashMap::new(),
            call_fullscreen: false,
            call_chat: false,
            voice_start: HashMap::new(),
            channel_info_requested: HashSet::new(),
            avatar_colors: HashMap::new(),
            pins: None,
            revealed: Rc::default(),
            text_files: Rc::default(),
            relationships: Vec::new(),
            friends_tab: home::FriendsTab::Online,
            inbox: None,
            saved: None,
            saved_ids: Default::default(),
            frecency_save_pending: false,
            inbox_open: false,
            inbox_tab: inbox::InboxTab::default(),
            pending_jump: None,
            popover_at: gpui_kit::Point::default(),
            inbox_page: false,
            requests_page: None,
            join: joining::Join::default(),
            guild_page: None,
            gif_picker: None,
            picker_tab: stickers::PickerTab::default(),
            sticker_packs: None,
            sticker_search,
            gif_search,
            restore: None,
            editing: None,
            mention_wire: HashMap::new(),
            autocomplete: None,
            autocomplete_scroll: gpui_kit::ScrollHandle::new(),
            channels_scroll: gpui_kit::ScrollHandle::new(),
            rail_scroll: gpui_kit::ScrollHandle::new(),
            panel_scroll: gpui_kit::ScrollHandle::new(),
            flash: None,
            muted: false,
            deafened: false,
            call: None,
            voice_error: None,
            voice_notice: None,
            last_call: None,
            gateway_session: String::new(),
            call_switched: false,
            viewer: None,
            profile_card: None,
            status_menu: false,
            updates: updater::Updates::default(),
            hovered_row: None,
            hovered_message: None,
            focused_message: None,
            feed_focus,
            call_focus: None,
            call_area: Rc::new(std::cell::Cell::new((0., 0.))),
            profiles: query::Query::new("profiles", Duration::from_secs(5 * 60)),
            link_previews: link_preview::previews(),
            reactors: HashMap::new(),
            reactions_dialog: None,
            games: Default::default(),
            composer_height: Rc::new(std::cell::Cell::new(86.)),
            pins_button_x: Rc::default(),
            pins_list: ListState::new(0, ListAlignment::Top, px(500.)),
            pip: Default::default(),
            emoji_picker: None,
            emoji_search,
            switcher: None,
            switcher_input,
            unread_marker: None,
            scrolled_up: false,
            copied: None,
            focus: cx.focus_handle(),
            channels_width: layout::CHANNELS_WIDTH,
            settings_width: ui::SETTINGS_NAV_WIDTH,
            settings_nav_scroll: gpui_kit::ScrollHandle::new(),
            panel_width: layout::PANEL_WIDTH,
            drag: None,
            back: Vec::new(),
            forward: Vec::new(),
            folded: HashSet::new(),
            hovered: None,
            member_hover: None,
            motions: Motions::new(),
            rail_dms_shown: None,
            arrivals: Rc::default(),
            parts: part::Parts::default(),
        };
        let weak = cx.weak_entity();
        waffle.list.set_scroll_handler(move |event, _, cx| {
            let away = chat::away_from_present(event.visible_range.end, event.count);
            weak.update(cx, |this, cx| {
                if this.scrolled_up != away {
                    this.scrolled_up = away;
                    cx.notify();
                }
                // Reaching the newest message is what reads the channel, as in Discord.
                if !away {
                    this.ack_open(cx);
                }
            })
            .ok();
        });
        waffle.apply_prefs();
        waffle
    }

    fn splash_stage(&mut self, step: usize, stage: &'static str, cx: &mut Context<Self>) {
        if let Some(splash) = self.splash {
            splash.update(cx, |splash, _, cx| splash.set(step, stage, cx)).ok();
        }
    }

    // Servers are in: say so, then open Waffle a beat later so the step registers.
    fn splash_ready(&mut self, cx: &mut Context<Self>) {
        if self.splash.is_none() {
            return;
        }
        self.splash_stage(1, "Loading your servers…", cx);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SPLASH_BEAT).await;
            this.update(cx, |this, cx| this.splash_stage(2, "Opening Waffle…", cx)).ok();
            cx.background_executor().timer(SPLASH_BEAT).await;
            this.update(cx, |this, cx| this.finish_splash(cx)).ok();
        })
        .detach();
    }

    // Closes the launch window and brings the main window forward, after this update finishes
    // (a window can't be shown from inside its own update).
    fn finish_splash(&mut self, cx: &mut Context<Self>) {
        let Some(splash) = self.splash.take() else { return };
        let main = self.main_window;
        cx.defer(move |cx| {
            splash.update(cx, |_, window, _| window.remove_window()).ok();
            main.update(cx, |_, window, _| window.activate_window()).ok();
        });
    }

    fn connect(&mut self, token: String, cx: &mut Context<Self>) {
        self.splash_stage(0, "Signing in…", cx);
        let (client, mut events) = discord::connect(token.clone());
        self.token = Some(token);
        self.client = Some(client);
        self.watch_games(cx);
        self.status = Status::Connecting;
        self._session = Some(cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this.update(cx, |this, cx| this.on_event(event, cx)).is_err() {
                    break;
                }
            }
        }));
        cx.notify();
    }

    // The gateway closed with 4004. Only a token the REST API also refuses is forgotten (that sends you to
    // sign in); anything else, a network error included, reconnects with the same token.
    fn verify_rejected_token(&mut self, cx: &mut Context<Self>) {
        let (Some(client), Some(token)) = (self.client.clone(), self.token.clone()) else { return self.sign_in(cx) };
        // A second 4004 before any READY: the token reads fine over REST but the gateway still refuses it.
        if std::mem::replace(&mut self.auth_retried, true) {
            self.forget_rejected_account();
            return self.sign_in(cx);
        }
        self.status = Status::Reconnecting("Checking your sign-in…".into());
        cx.spawn(async move |this, cx| {
            let refused = client.call(reqwest::Method::GET, "/users/@me".into(), None).await.err().is_some_and(|e| crate::discord::is_unauthorized(&e));
            this.update(cx, |this, cx| match refused {
                true => {
                    eprintln!("gateway: token refused (4004 and REST 401), signing out");
                    this.forget_rejected_account();
                    this.sign_in(cx);
                }
                false => {
                    eprintln!("gateway: 4004 but the token still works, reconnecting");
                    this.connect(token, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn sign_in(&mut self, cx: &mut Context<Self>) {
        // Signing in happens in the main window, so the splash steps aside.
        self.finish_splash(cx);
        self.client = None;
        self.token = None;
        self.me = None;
        self.channel = None;
        self.guilds.clear();
        self.dms.clear();
        self.dm_requests.clear();
        self.feeds.clear();
        self.settings = None;
        self.status = Status::SignedOut(Login::Preparing);
        let mut events = auth::sign_in();
        // Replacing the task drops the gateway's receiver, which stops the old session.
        self._session = Some(cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this.update(cx, |this, cx| this.on_auth(event, cx)).is_err() {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn on_auth(&mut self, event: auth::AuthEvent, cx: &mut Context<Self>) {
        let login = match event {
            auth::AuthEvent::SignedIn(token) => return self.connect(token, cx),
            auth::AuthEvent::Qr(url) => Login::qr(&url),
            auth::AuthEvent::Scanned(user) => Login::Scanned(user),
            auth::AuthEvent::Captcha(url) => {
                cx.open_url(&url);
                Login::Captcha(url.into())
            }
            auth::AuthEvent::Failed(error) => Login::Failed(error.into()),
        };
        self.status = Status::SignedOut(login);
        cx.notify();
    }

    pub fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::UserSettings(changed) => {
                self.account_settings.apply(&changed);
                self.developer_mode = self.account_settings.developer_mode;
                cx.notify();
            }
            Event::Ready(ready) => {
                let ready = *ready;
                self.channel_info_requested.clear();
                self.command_picker.mark_indexes_stale();
                self.auth_retried = false;
                // Demo scenes set their own place; the last channel a real session saved would override it.
                if self.me.is_none() && std::env::var_os("WAFFLE_DEMO").is_none() {
                    self.restore = home::load_last_channel(&ready.me.id);
                }
                self.my_status = ready.status;
                self.remember_account(&ready.me);
                self.gateway_session = ready.session_id.clone();
                self.fetch_saved_ids(cx);
                self.load_reaction_frecency(cx);
                self.me = Some(ready.me);
                self.custom_status = ready.custom_status;
                self.expire_custom_status_later(cx);
                // The identify went out with no activities; a game already running goes back in.
                if self.games.mine.is_some() {
                    self.publish_presence();
                }
                for channel in ready.guilds.iter().flat_map(|g| &g.channels).chain(&ready.dms) {
                    if let Some(id) = channel.last_message_id.as_deref().and_then(|id| id.parse().ok()) {
                        self.last_message.insert(channel.id.clone(), id);
                    }
                }
                if cfg!(debug_assertions) && std::env::var_os("WAFFLE_STREAM_DEBUG").is_some() {
                    for guild in &ready.guilds {
                        let (voice, live) = (guild.voice_states.len(), guild.voice_states.iter().filter(|v| v.streaming).count());
                        if voice > 0 {
                            eprintln!("ready guild {}: {voice} in voice, {live} streaming", guild.id);
                        }
                    }
                }
                // Streams already live when we connect: their start is unknown, so count from now.
                for guild in &ready.guilds {
                    for state in guild.voice_states.iter().filter(|v| v.streaming) {
                        if let Some(channel) = &state.channel_id {
                            self.stream_live_since.entry(format!("guild:{}:{channel}:{}", guild.id, state.user_id)).or_insert_with(Instant::now);
                        }
                    }
                }
                self.guilds = ready.guilds;
                self.folders = ready.folders;
                (self.dm_requests, self.dms) = ready.dms.into_iter().partition(Channel::is_request);
                self.dms.sort_by_key(|c| std::cmp::Reverse(self.last_message.get(&c.id).copied().unwrap_or(0)));
                self.read_states = ready.read_states.into_iter().collect();
                self.relationships = ready.relationships;
                self.developer_mode = ready.developer_mode;
                self.account_settings = ready.account_settings;
                self.my_account = ready.my_account;
                self.guild_settings = ready.guild_settings.into_iter().collect();
                self.ignored = ready.ignored.into_iter().collect();
                self.presences = ready.presences.into_iter().collect();
                self.status = Status::Online;
                self.load_activity_feed(cx);
                self.splash_ready(cx);
                self.rejoin_after_restart(cx);
            }
            Event::Batch(events) => {
                for event in events {
                    self.on_event(event, cx);
                }
            }
            Event::ChannelUpsert { guild_id, channel } => {
                let Some(guild) = self.guilds.iter_mut().find(|g| g.id == guild_id) else { return };
                let list = if channel.kind == ChannelKind::Thread { &mut guild.threads } else { &mut guild.channels };
                match list.iter_mut().find(|c| c.id == channel.id) {
                    Some(existing) => {
                        // THREAD_UPDATE carries no membership: keep whether we'd joined.
                        let joined = existing.thread.as_ref().is_some_and(|t| t.joined);
                        *existing = channel;
                        if let Some(thread) = existing.thread.as_mut() {
                            thread.joined |= joined;
                        }
                    }
                    None => list.push(channel),
                }
                guild.channels.sort_by_key(|c| c.position);
            }
            Event::ChannelDelete { guild_id, id } => {
                if let Some(guild) = self.guilds.iter_mut().find(|g| g.id == guild_id) {
                    guild.channels.retain(|c| c.id != id);
                    guild.threads.retain(|c| c.id != id);
                }
                if self.channel.as_ref().is_some_and(|c| c.id == id) {
                    self.channel = None;
                }
            }
            Event::MemberUpdate { guild_id, user, roles, membership } => {
                self.profile_changed(&user.id);
                if self.me.as_ref().is_some_and(|me| me.id == user.id)
                    && let Some(guild) = self.guilds.iter_mut().find(|g| g.id == guild_id)
                {
                    guild.my_roles = roles.clone();
                    guild.me = membership;
                }
                self.member_roles.entry(guild_id.clone()).or_default().insert(user.id.clone(), roles);
                self.members.entry(guild_id.clone()).or_default().insert(user.id.clone(), user);
                self.apply_members(&guild_id);
            }
            Event::RelationshipChange { user_id, relationship } => {
                // An update may arrive without the user object; keep the one we have.
                let known = self.relationships.iter().find(|r| r.user.id == user_id).map(|r| r.user.clone());
                self.relationships.retain(|r| r.user.id != user_id);
                if let Some(mut relationship) = relationship {
                    if relationship.user.name.is_empty()
                        && let Some(user) = known
                    {
                        relationship.user = user;
                    }
                    self.relationships.push(relationship);
                }
            }
            Event::DmUpsert(dm) => self.upsert_dm(dm, cx),
            Event::DmDelete(id) => {
                self.dms.retain(|d| d.id != id);
                self.dm_requests.retain(|d| d.id != id);
                if self.channel.as_ref().is_some_and(|c| c.id == id) {
                    self.open_home(cx);
                }
            }
            Event::RecipientChange { channel_id, user, added } => {
                if !added && self.me.as_ref().is_some_and(|me| me.id == user.id) {
                    return self.on_event(Event::DmDelete(channel_id), cx);
                }
                let open = self.channel.as_mut().filter(|c| c.id == channel_id);
                for dm in self.dms.iter_mut().filter(|d| d.id == channel_id).chain(open) {
                    dm.recipients.retain(|r| r.id != user.id);
                    if added {
                        dm.recipients.push(user.clone());
                    }
                }
            }
            Event::ThreadJoined { id, joined } => {
                for thread in self.guilds.iter_mut().flat_map(|g| g.threads.iter_mut()).filter(|t| t.id == id) {
                    if let Some(meta) = thread.thread.as_mut() {
                        meta.joined = joined;
                    }
                }
            }
            Event::GuildSettings((key, settings)) => {
                self.guild_settings.insert(key, settings);
            }
            Event::GuildCreate(mut guild) => {
                let joined = self.joining.take_if(|id| *id == guild.id);
                // GUILD_CREATE carries no member entry for us: keep the roles we know (READY, or
                // GUILD_MEMBER_UPDATE), or take them from the member cache, so permissions hold.
                let me = self.me.as_ref().map(|m| m.id.clone()).unwrap_or_default();
                let known_roles = self.member_roles.get(&guild.id).and_then(|r| r.get(&me)).cloned();
                match self.guilds.iter_mut().find(|g| g.id == guild.id) {
                    Some(existing) => {
                        if guild.my_roles.is_empty() {
                            guild.my_roles = std::mem::take(&mut existing.my_roles);
                        }
                        // Same for our membership state: GUILD_CREATE doesn't carry it.
                        guild.me = std::mem::take(&mut existing.me);
                        for thread in existing.threads.drain(..) {
                            if !guild.threads.iter().any(|t| t.id == thread.id) {
                                guild.threads.push(thread);
                            }
                        }
                        *existing = guild;
                    }
                    None => {
                        if guild.my_roles.is_empty() {
                            guild.my_roles = known_roles.unwrap_or_default();
                        }
                        self.guilds.push(guild);
                    }
                }
                if let Some(id) = joined {
                    self.open_joined_guild(id, cx);
                }
            }
            Event::MessageCreate(message) => {
                self.watch_deferred(&message.id, message.flags, cx);
                self.on_message(*message, cx);
            }
            Event::MessageUpdate(patch) => {
                let (channel_id, id) = (patch["channel_id"].as_str().unwrap_or_default(), patch["id"].as_str().unwrap_or_default());
                if let Some(m) = self.message_mut(channel_id, id) {
                    discord::apply_update(m, &patch);
                }
                if let Some(flags) = self.message_mut(channel_id, id).map(|m| m.flags) {
                    self.watch_deferred(id, flags, cx);
                }
            }
            Event::MessageDelete { channel_id, id } => self.remove_message(&channel_id, &id),
            Event::Reaction { channel_id, message_id, user_id, emoji, added } => {
                let mine = self.me.as_ref().is_some_and(|me| me.id == user_id);
                self.reactors_changed(&message_id, &emoji, &user_id, added);
                if let Some(m) = self.message_mut(&channel_id, &message_id) {
                    adjust_reaction(m, &emoji, added, mine, true);
                }
            }
            Event::PollVote { channel_id, message_id, user_id, answer_id, added } => {
                let mine = self.me.as_ref().is_some_and(|me| me.id == user_id);
                if let Some(poll) = self.message_mut(&channel_id, &message_id).and_then(|m| m.poll.as_mut()) {
                    poll::apply_vote(poll, answer_id, added, mine);
                }
            }
            Event::Typing { channel_id, user_id, name } => self.on_typing(channel_id, user_id, name, cx),
            Event::Presence { user_id, presence } => {
                self.presences.insert(user_id, presence);
            }
            Event::MemberList { guild_id, groups, ops } => {
                if !groups.is_empty() {
                    self.member_groups.insert(guild_id.clone(), groups.into_iter().collect());
                }
                let list = self.member_lists.entry(guild_id.clone()).or_default();
                for op in ops {
                    apply_list_op(list, op);
                }
                let roles = self.member_roles.entry(guild_id.clone()).or_default();
                let known = self.members.entry(guild_id.clone()).or_default();
                for item in list.iter() {
                    if let ListItem::Member(m) = item {
                        known.insert(m.user.id.clone(), m.user.clone());
                        roles.insert(m.user.id.clone(), m.roles.clone());
                        self.presences.insert(m.user.id.clone(), m.presence.clone());
                    }
                }
                self.apply_members(&guild_id);
            }
            Event::MembersChunk { guild_id, members } => {
                let roles = self.member_roles.entry(guild_id.clone()).or_default();
                let known = self.members.entry(guild_id.clone()).or_default();
                for (user, member_roles) in members {
                    roles.insert(user.id.clone(), member_roles);
                    known.insert(user.id.clone(), user);
                }
                self.apply_members(&guild_id);
            }
            Event::Ack { channel_id, message_id, manual: Some(mentions) } => {
                if self.channel.as_ref().is_some_and(|c| c.id == channel_id) {
                    self.unread_marker = Some(message_id);
                }
                self.read_states.insert(channel_id, ReadState { last_read: message_id, mentions });
            }
            Event::Ack { channel_id, message_id, manual: None } => {
                let read = self.read_states.entry(channel_id).or_default();
                read.last_read = read.last_read.max(message_id);
                read.mentions = 0;
            }
            Event::VoiceState(state, session) => {
                let ours = self.call.as_ref().map(|c| c.channel.id.clone());
                let me = self.me.as_ref().map(|m| m.id.clone());
                if Some(&state.user_id) == me.as_ref() && state.channel_id.is_some() && session.is_some_and(|s| s != self.gateway_session) {
                    self.taken_elsewhere();
                }
                // Someone answering our ring joins the DM call: the ringtone stops.
                if state.channel_id.is_some() && state.channel_id == self.ringing_out && Some(&state.user_id) != me.as_ref() {
                    self.stop_ringing_out();
                }
                let mine = Some(&state.user_id) == me.as_ref();
                // Answered on another device: this one stops ringing too.
                if mine && state.channel_id.is_some() && state.channel_id == self.ringing_in {
                    self.stop_ringing_in();
                }
                let states = match &state.guild_id {
                    Some(id) => match self.guilds.iter_mut().find(|g| &g.id == id) {
                        Some(guild) => &mut guild.voice_states,
                        None => return,
                    },
                    None => &mut self.dm_voice,
                };
                let was_here = states.iter().any(|v| v.user_id == state.user_id && v.channel_id == ours);
                let is_here = ours.is_some() && state.channel_id == ours;
                let was_live = states.iter().any(|v| v.user_id == state.user_id && v.channel_id == ours && v.streaming);
                let is_live = is_here && state.streaming;
                let silent = self.call.as_ref().is_some_and(|c| c.channel.raw_type == 13);
                if let Some(cue) = member_cue(mine, was_here, is_here).filter(|_| !silent) {
                    sounds::play(cue);
                } else if !mine && !was_live && is_live {
                    sounds::play(sounds::Cue::StreamStarted);
                } else if was_live && !is_live && self.call.as_ref().is_some_and(|c| c.watched(&state.user_id).is_some()) {
                    sounds::play(sounds::Cue::StreamEnded);
                }
                let live_key = state.channel_id.as_ref().filter(|_| state.streaming).map(|c| match &state.guild_id {
                    Some(guild) => format!("guild:{guild}:{c}:{}", state.user_id),
                    None => format!("call:{c}:{}", state.user_id),
                });
                let suffix = format!(":{}", state.user_id);
                self.stream_live_since.retain(|key, _| !key.ends_with(&suffix) || Some(key) == live_key.as_ref());
                self.stream_paused.retain(|key| !key.ends_with(&suffix) || Some(key) == live_key.as_ref());
                if let Some(key) = live_key {
                    self.stream_live_since.entry(key).or_insert_with(Instant::now);
                }
                upsert_voice_state(states, state);
            }
            Event::Call { channel_id, ringing } => self.on_dm_call(channel_id, ringing, cx),
            Event::CallEnded { channel_id } => self.on_dm_call_ended(&channel_id),
            Event::VoiceServer(_) => {}
            Event::StageInstance { guild_id, channel_id, instance } => {
                if let Some(guild) = self.guilds.iter_mut().find(|g| g.id == guild_id) {
                    guild.stage_instances.retain(|s| s.channel_id != channel_id);
                    guild.stage_instances.extend(instance);
                }
            }
            Event::VoiceStartTimes { guild_id, times, all } => {
                if all
                    && let Some(guild) = self.guilds.iter().find(|g| g.id == guild_id)
                {
                    for channel in &guild.channels {
                        self.voice_start.remove(&channel.id);
                    }
                }
                for (channel, start) in times {
                    match start {
                        Some(start) => self.voice_start.insert(channel, start),
                        None => self.voice_start.remove(&channel),
                    };
                }
            }
            Event::StreamUpdate { key, viewers, paused } => {
                match paused {
                    true => self.stream_paused.insert(key.clone()),
                    false => self.stream_paused.remove(&key),
                };
                let before = self.stream_viewers.get(&key).map_or(0, Vec::len);
                if self.in_stream(&key)
                    && let Some(cue) = viewer_cue(before, viewers.len())
                {
                    sounds::play(cue);
                }
                self.stream_viewers.insert(key, viewers);
            }
            Event::Disconnected(reason) => {
                self.splash_stage(0, "Reconnecting…", cx);
                self.status = Status::Reconnecting(reason.into());
            }
            Event::Resumed => {
                self.status = Status::Online;
                self.command_picker.mark_indexes_stale();
            }
            Event::AuthFailed => return self.verify_rejected_token(cx),
            Event::InteractionCreate { nonce, id } => self.on_interaction_create(nonce, id, cx),
            Event::InteractionSuccess { nonce } => self.on_interaction_success(nonce, cx),
            Event::InteractionFailure { nonce, reason_code } => {
                self.autocomplete_failed(&nonce, cx);
                self.interaction_failed(&nonce, reason_code, cx);
            }
            Event::ModalCreate(modal) => self.open_app_modal(modal, cx),
            Event::Autocomplete { nonce, choices } => self.on_autocomplete(nonce, choices, cx),
        }
        self.resolve_voice_users();
        self.refresh_stream_previews(cx);
        self.refresh_avatar_colors(cx);
        self.update_badge();
        cx.notify();
    }

    // READY's voice states carry only user ids: fill them from members we know, and ask the
    // gateway once for the rest so every voice row and tile shows the real name and avatar.
    fn resolve_voice_users(&mut self) {
        let known: Vec<User> = self.known_users().into_iter().chain(self.me.clone()).collect();
        for state in self.dm_voice.iter_mut().filter(|v| v.user.is_none()) {
            state.user = known.iter().find(|u| u.id == state.user_id).cloned();
        }
        let mut missing: Vec<(String, String)> = Vec::new();
        for guild in &mut self.guilds {
            let known = self.members.get(&guild.id);
            for state in guild.voice_states.iter_mut().filter(|v| v.user.is_none()) {
                match known.and_then(|k| k.get(&state.user_id)) {
                    Some(user) => state.user = Some(user.clone()),
                    None if self.requested_members.insert((guild.id.clone(), state.user_id.clone())) => missing.push((guild.id.clone(), state.user_id.clone())),
                    None => {}
                }
            }
        }
        let Some(client) = &self.client else { return };
        let mut by_guild: HashMap<String, Vec<String>> = HashMap::new();
        for (guild_id, user_id) in missing {
            by_guild.entry(guild_id).or_default().push(user_id);
        }
        for (guild_id, ids) in by_guild {
            for chunk in ids.chunks(100) {
                client.request_members(&guild_id, chunk.to_vec());
            }
        }
    }

    fn on_message(&mut self, message: Message, cx: &mut Context<Self>) {
        self.note_message_sent(&message, cx);
        let channel_id = message.channel_id.clone();
        let mine = self.me.as_ref().is_some_and(|me| me.id == message.author.id);
        let is_open = self.channel.as_ref().is_some_and(|c| c.id == channel_id);
        if let Ok(id) = message.id.parse() {
            self.last_message.insert(channel_id.clone(), id);
            // Sending reads the channel up to our message, as Discord's server does; without it our own
            // message would mark the DM unread and raise its tile in the rail.
            if mine {
                self.read_states.entry(channel_id.clone()).or_default().last_read = id;
                // Posting reads the channel, and the NEW line goes, as in Discord.
                if is_open {
                    self.unread_marker = None;
                }
            }
        }
        if let (Some(guild_id), Some(roles)) = (&message.guild_id, &message.roles) {
            self.member_roles.entry(guild_id.clone()).or_default().insert(message.author.id.clone(), roles.clone());
        }
        if let Some(typers) = self.typing.get_mut(&channel_id) {
            typers.retain(|t| t.user_id != message.author.id);
        }
        let is_dm = self.dms.iter().any(|d| d.id == channel_id);
        if !mine && !is_open && (self.mentions_me(&message) || is_dm) {
            self.read_states.entry(channel_id.clone()).or_default().mentions += 1;
        }
        let kind = self.notify_kind(&message, is_open && self.window_active);
        if kind != notify::Notify::None {
            sounds::play(if is_dm { sounds::Cue::Message } else { sounds::Cue::Mention });
            if !self.window_active || !is_open {
                self.notify(&message);
            }
        }
        if let Some(pos) = self.dms.iter().position(|d| d.id == channel_id) {
            let dm = self.dms.remove(pos);
            self.dms.insert(0, dm);
        }
        // An app's reply to our command takes its "is thinking…" stand-in's place.
        let nonce = self.interaction_reply_nonce(&message).or_else(|| message.nonce.clone());
        let Some(feed) = self.feeds.get_mut(&channel_id) else { return };
        // Our own optimistic copy, sent with this nonce: swap in the real one in place.
        if let Some(pending) = Arc::make_mut(&mut feed.messages).iter_mut().find(|m| m.state != SendState::Sent && m.nonce.is_some() && m.nonce == nonce) {
            *pending = message;
            return;
        }
        if feed.messages.iter().any(|m| m.id == message.id) {
            return;
        }
        let at = feed.messages.len();
        Arc::make_mut(&mut feed.messages).push(message.clone());
        if is_open {
            self.arrivals.borrow_mut().fresh.insert(message.id);
            self.list.splice(at..at, 1);
            self.ack_open(cx);
        }
    }

    fn on_typing(&mut self, channel_id: String, user_id: String, name: Option<String>, cx: &mut Context<Self>) {
        if self.me.as_ref().is_some_and(|me| me.id == user_id) {
            return;
        }
        let name = name.unwrap_or_else(|| self.user_name(&user_id));
        let typers = self.typing.entry(channel_id).or_default();
        typers.retain(|t| t.user_id != user_id);
        typers.push(Typer { user_id, name, until: Instant::now() + TYPING_TTL });
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TYPING_TTL).await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
    }

    fn open_guild(&mut self, guild_id: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.guild_id = guild_id.clone();
        self.guild_page = None;
        self.refresh_stream_previews(cx);
        self.request_channel_info();
        let remembered = self.last_channel.get(&guild_id).cloned();
        // In sidebar order, and only channels we can open (Discord also lists hidden ones).
        let channels: Vec<Channel> = match &guild_id {
            Some(_) => self.guild().map(|g| sidebar::channel_sections(g, self.my_id()).into_iter().flat_map(|(_, c)| c).collect()).unwrap_or_default(),
            None => self.dms.clone(),
        };
        let target = remembered
            .and_then(|id| channels.iter().find(|c| c.id == id).cloned())
            .or_else(|| channels.into_iter().find(|c| matches!(c.kind, ChannelKind::Text | ChannelKind::Dm)));
        match target {
            Some(channel) => self.open_channel(channel, window, cx),
            None => {
                self.channel = None;
                cx.notify();
            }
        }
    }

    pub(crate) fn open_channel(&mut self, channel: Channel, window: &mut Window, cx: &mut Context<Self>) {
        self.inbox_page = false;
        self.requests_page = None;
        self.held_unread = None;
        self.guild_page = None;
        self.note_channel_viewed(&channel, cx);
        // Results belong to the server or DM they searched: another place closes them.
        let scope = match &self.guild_id {
            Some(guild) => format!("guilds/{guild}"),
            None => format!("channels/{}", channel.id),
        };
        if matches!(&self.right_panel, RightPanel::Search(results) if !results.searched(&scope)) {
            self.right_panel = RightPanel::Members;
        }
        if let Some(current) = self.channel.clone().filter(|c| c.id != channel.id) {
            self.back.push((self.guild_id.clone(), current));
            self.forward.clear();
        }
        self.show_channel(channel, window, cx);
    }

    /// Back/forward in the title bar, like a browser's history of channels.
    fn navigate(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (from, to) = if forward { (&mut self.forward, &mut self.back) } else { (&mut self.back, &mut self.forward) };
        let Some((guild_id, channel)) = from.pop() else { return };
        if let Some(current) = self.channel.clone() {
            to.push((self.guild_id.clone(), current));
        }
        self.guild_id = guild_id;
        self.show_channel(channel, window, cx);
    }

    fn show_channel(&mut self, channel: Channel, window: &mut Window, cx: &mut Context<Self>) {
        self.call_view = false;
        let placeholder = format!("Message {}", channel_mention(&channel));
        self.composer.update(cx, |state, cx| state.set_placeholder(placeholder, window, cx));
        self.last_channel.insert(self.guild_id.clone(), channel.id.clone());
        home::save_last_channel(self.my_id(), self.guild_id.as_deref(), &channel.id);
        let id = channel.id.clone();
        if let (Some(client), Some(guild_id)) = (&self.client, &self.guild_id) {
            client.request_member_list(guild_id, &id);
        }
        // The channel we're leaving keeps what arrived while it was open.
        if let Some(previous) = self.channel.as_ref().filter(|c| c.id != id).and_then(|c| self.feeds.get(&c.id).map(|f| (c.id.clone(), f)))
            && previous.1.loaded
        {
            cache::save(&previous.0, &previous.1.messages);
        }
        if !self.feeds.contains_key(&id)
            && let Some(messages) = cache::load(&id)
        {
            self.feeds.insert(id.clone(), Feed { messages: Arc::new(messages), loaded: true, has_more: true, ..Default::default() });
        }
        self.load_dm_profile(&channel, cx);
        // Before the reset: the draft keeps the command typed into it.
        self.stash_draft(cx);
        self.command_picker.reset();
        self.clear_failed_interactions();
        if let Some(previous) = self.channel.as_ref().map(|c| c.id.clone()).filter(|p| *p != id) {
            match self.scroll_anchor() {
                Some(anchor) => self.scroll_anchors.insert(previous, anchor),
                None => self.scroll_anchors.remove(&previous),
            };
        }
        let is_forum = channel.kind == ChannelKind::Forum;
        self.channel = Some(channel);
        self.editing = None;
        self.restore_draft(window, cx);
        // Opening a channel or DM puts the cursor in its composer, as Discord's.
        if !is_forum {
            self.composer.update(cx, |state, cx| state.focus(window, cx));
        }
        self.send_error = None;
        self.autocomplete = None;
        self.scrolled_up = false;
        // Cached channels show at once; every open also refreshes in the background.
        // Messages after this sit under Discord's red NEW line until the channel is left.
        self.unread_marker = self.unread(&id).then(|| self.read_states.get(&id).map(|r| r.last_read)).flatten();
        if self.channel.as_ref().is_some_and(|c| c.kind == ChannelKind::Forum) {
            if !self.forums.contains_key(&id) {
                self.load_forum(id, cx);
            }
            return cx.notify();
        }
        let count = self.feeds.get(&id).map_or(0, |f| f.messages.len());
        self.list.reset(count);
        // With news, the channel opens on its first unread, under the NEW line, and stays unread until you
        // scroll down to the newest message (Discord's auto-read needs the bottom); otherwise it's read now.
        // Without, it opens where it was left.
        self.unread_scroll_pending = self.unread_marker.is_some();
        self.scroll_to_first_unread();
        if self.unread_marker.is_none()
            && let Some(anchor) = self.scroll_anchors.get(&id).cloned()
        {
            self.scroll_to_anchor(anchor);
        }
        self.fetch_latest(&id, cx);
        if self.unread_marker.is_none() {
            self.ack_open(cx);
        }
        cx.notify();
    }

    // The message at the top of the open feed and how far into it, while scrolled up from the newest.
    fn scroll_anchor(&self) -> Option<(String, gpui_kit::Pixels)> {
        if !self.scrolled_up {
            return None;
        }
        let top = self.list.logical_scroll_top();
        let feed = self.feeds.get(&self.channel.as_ref()?.id)?;
        Some((feed.messages.get(top.item_ix)?.id.clone(), top.offset_in_item))
    }

    // Back to an anchor, if its message is still in the feed.
    fn scroll_to_anchor(&mut self, (message_id, offset): (String, gpui_kit::Pixels)) {
        let Some(feed) = self.channel.as_ref().and_then(|c| self.feeds.get(&c.id)) else { return };
        let Some(item_ix) = feed.messages.iter().position(|m| m.id == message_id) else { return };
        self.list.scroll_to(ListOffset { item_ix, offset_in_item: offset });
        self.scrolled_up = true;
    }

    // Puts the first unread message near the top, once it's loaded.
    fn scroll_to_first_unread(&mut self) {
        let (Some(read), Some(channel)) = (self.unread_marker, self.channel.as_ref()) else { return };
        if !self.unread_scroll_pending {
            return;
        }
        let Some(feed) = self.feeds.get(&channel.id) else { return };
        let Some(first) = feed.messages.iter().position(|m| order(&m.id) > read) else { return };
        // Its row opens with the NEW line, at the feed's top, just under the unread bar.
        self.list.scroll_to(ListOffset { item_ix: first, offset_in_item: px(0.) });
        self.unread_scroll_pending = false;
    }

    /// Hovering a channel warms its cache so the click shows messages instantly.
    fn prefetch(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        if self.feeds.get(channel_id).is_some_and(|f| f.loaded || f.fetching) {
            return;
        }
        self.fetch_latest(channel_id, cx);
    }

    fn fetch_latest(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let feed = self.feeds.entry(channel_id.to_owned()).or_default();
        if feed.fetching {
            return;
        }
        feed.fetching = true;
        let id = channel_id.to_owned();
        cx.spawn(async move |this, cx| {
            let result = client.messages(&id, None, None).await;
            this.update(cx, |this, cx| this.merge_latest(&id, result, cx)).ok();
        })
        .detach();
    }

    fn merge_latest(&mut self, channel_id: &str, result: Result<Vec<Message>, String>, cx: &mut Context<Self>) {
        let is_open = self.channel.as_ref().is_some_and(|c| c.id == channel_id);
        // Read before the feed changes: the reset below would otherwise drop a reader scrolled up back to the bottom.
        let anchor = is_open.then(|| self.scroll_anchor()).flatten();
        let feed = self.feeds.entry(channel_id.to_owned()).or_default();
        feed.fetching = false;
        let fetched = match result {
            Ok(fetched) => fetched,
            Err(error) => {
                feed.error = Some(error.into());
                return cx.notify();
            }
        };
        let newest = fetched.last().map(|m| order(&m.id)).unwrap_or(0);
        let kept: Vec<Message> = feed.messages.iter().filter(|m| m.state != SendState::Sent || order(&m.id) > newest).cloned().collect();
        let unchanged = kept.is_empty() && feed.loaded && feed.messages.len() == fetched.len();
        let reached_start = feed.has_more && fetched.len() < 50;
        feed.has_more = fetched.len() == 50;
        feed.messages = Arc::new(fetched.into_iter().chain(kept).collect());
        feed.loaded = true;
        feed.error = None;
        cache::save(channel_id, &feed.messages);
        let count = feed.messages.len();
        let authors: Vec<String> = feed.messages.iter().map(|m| m.author.id.clone()).collect();
        if is_open && !unchanged {
            self.list.reset(count);
            if let Some(anchor) = anchor {
                self.scroll_to_anchor(anchor);
            }
            self.scroll_to_first_unread();
            // Tried once the messages are in: a first unread that never loads mustn't hold off reading.
            self.unread_scroll_pending = false;
        } else if is_open && reached_start {
            // The first row now carries the DM's start header.
            self.list.remeasure_items(0..1.min(count));
        }
        self.request_missing_roles(authors);
        if self.pending_jump.as_ref().is_some_and(|(channel, _)| channel == channel_id)
            && let Some((_, message_id)) = self.pending_jump.take()
        {
            self.jump_to(message_id, cx);
        }
        cx.notify();
    }

    fn load_older(&mut self, cx: &mut Context<Self>) {
        let (Some(client), Some(channel)) = (self.client.clone(), self.channel.clone()) else { return };
        let Some(feed) = self.feeds.get_mut(&channel.id) else { return };
        if !feed.has_more || feed.loading_older || !feed.loaded {
            return;
        }
        let Some(oldest) = feed.messages.first().map(|m| m.id.clone()) else { return };
        feed.loading_older = true;
        cx.spawn(async move |this, cx| {
            let result = client.messages(&channel.id, Some(&oldest), None).await;
            this.update(cx, |this, cx| {
                let is_open = this.channel.as_ref().is_some_and(|c| c.id == channel.id);
                let Some(feed) = this.feeds.get_mut(&channel.id) else { return };
                feed.loading_older = false;
                let Ok(older) = result else { return };
                feed.has_more = older.len() == 50;
                let added = older.len();
                let authors: Vec<String> = older.iter().map(|m| m.author.id.clone()).collect();
                Arc::make_mut(&mut feed.messages).splice(0..0, older);
                if is_open && added == 0 && !feed.has_more {
                    this.list.remeasure_items(0..1.min(feed.messages.len()));
                }
                if is_open && added > 0 {
                    // Keep the same message under the reader's eye while history grows above it.
                    let top = this.list.logical_scroll_top();
                    this.list.splice(0..0, added);
                    this.list.scroll_to(ListOffset { item_ix: top.item_ix + added, offset_in_item: top.offset_in_item });
                }
                if added > 0 {
                    this.refresh_viewer(cx);
                }
                this.request_missing_roles(authors);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // History from REST carries no member roles; ask the gateway for the authors we can't colour yet.
    // History from REST names authors by their account; once their member entries arrive, show
    // them as the server does (nickname and server avatar).
    fn apply_members(&mut self, guild_id: &str) {
        let Some(known) = self.members.get(guild_id) else { return };
        let stale = |m: &Message| m.guild_id.as_deref() == Some(guild_id) && known.get(&m.author.id).is_some_and(|k| k.name != m.author.name || k.avatar != m.author.avatar);
        // Member lists update often: only a feed that actually changes is copied for writing.
        for feed in self.feeds.values_mut().filter(|f| f.messages.iter().any(stale)) {
            for message in Arc::make_mut(&mut feed.messages).iter_mut().filter(|m| stale(m)) {
                let member = &known[&message.author.id];
                message.author.name.clone_from(&member.name);
                message.author.avatar.clone_from(&member.avatar);
            }
        }
    }

    fn request_missing_roles(&mut self, authors: Vec<String>) {
        if let Some(guild_id) = self.guild_id.clone() {
            self.apply_members(&guild_id);
        }
        let (Some(client), Some(guild_id)) = (&self.client, &self.guild_id) else { return };
        let known = self.member_roles.get(guild_id);
        let mut missing: Vec<String> = Vec::new();
        for id in authors {
            if known.is_some_and(|k| k.contains_key(&id)) || !self.requested_members.insert((guild_id.clone(), id.clone())) {
                continue;
            }
            missing.push(id);
        }
        for chunk in missing.chunks(100) {
            client.request_members(guild_id, chunk.to_vec());
        }
    }

    /// Scrolls to the replied-to message and flashes it; loads the history around it first if needed.
    fn jump_to(&mut self, message_id: String, cx: &mut Context<Self>) {
        let Some(channel) = self.channel.clone() else { return };
        // A channel just opened is still fetching its newest page, which lands after this and resets the
        // list to the bottom: wait for it (`merge_latest` runs the jump).
        if self.feeds.get(&channel.id).is_some_and(|f| f.fetching) {
            self.pending_jump = Some((channel.id.clone(), message_id));
            return;
        }
        let position = self.feeds.get(&channel.id).and_then(|f| f.messages.iter().position(|m| m.id == message_id));
        if let Some(ix) = position {
            self.list.scroll_to(ListOffset { item_ix: ix, offset_in_item: px(0.) });
            self.flash = Some(message_id.clone());
            // The row's height is known only once it's laid out: then pull it down to the middle of the view.
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(32)).await;
                this.update(cx, |this, cx| {
                    let (Some(row), view) = (this.list.bounds_for_item(ix), this.list.viewport_bounds()) else { return };
                    let spare = view.size.height - row.size.height;
                    if spare > px(0.) {
                        this.list.scroll_by(row.top() - view.top() - spare / 2.);
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(chat::JUMP_FLASH).await;
                this.update(cx, |this, cx| {
                    if this.flash.as_ref() == Some(&message_id) {
                        this.flash = None;
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
            return cx.notify();
        }
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            let Ok(around) = client.messages(&channel.id, None, Some(&message_id)).await else { return };
            this.update(cx, |this, cx| {
                let feed = this.feeds.entry(channel.id.clone()).or_default();
                feed.messages = Arc::new(around);
                feed.has_more = true;
                this.list.reset(feed.messages.len());
                this.jump_to(message_id, cx);
            })
            .ok();
        })
        .detach();
    }

    // Reads the open channel up to its newest message, only while that message is in view and the window
    // has focus (Discord's auto-read).
    fn ack_open(&mut self, cx: &mut Context<Self>) {
        if self.scrolled_up || !self.window_active || self.unread_scroll_pending {
            return;
        }
        let (Some(client), Some(channel)) = (self.client.clone(), self.channel.as_ref()) else { return };
        if self.held_unread.as_ref() == Some(&channel.id) {
            return;
        }
        let Some(&last) = self.last_message.get(&channel.id) else { return };
        let read = self.read_states.entry(channel.id.clone()).or_default();
        if read.last_read >= last && read.mentions == 0 {
            return;
        }
        read.last_read = last;
        read.mentions = 0;
        let id = channel.id.clone();
        discord::runtime().spawn(async move { client.ack(&id, &last.to_string()).await });
        let _ = cx;
    }

    fn on_composer(&mut self, _: &Entity<ComposerState>, event: &InputEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            InputEvent::Change => self.on_composer_change(window, cx),
            InputEvent::PressEnter { shift: false, .. } if self.on_command_enter(window, cx) => {}
            InputEvent::PressEnter { shift: false, .. } if self.autocomplete.is_some() => self.accept_suggestion(window, cx),
            InputEvent::PressEnter { shift: false, .. } => self.submit(window, cx),
            _ => {}
        }
    }

    // A slash command typed out gets the hint bar instead of mention suggestions, and sends no typing.
    fn on_composer_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).value();
        self.has_draft = !text.trim().is_empty() || !self.pending_files.is_empty();
        self.stash_draft(cx);
        self.update_command_picker(&text, cx);
        let command = self.on_command_text(window, cx);
        self.autocomplete = (!command && !self.command_picker.is_open()).then(|| self.suggest(&text)).flatten();
        if !command {
            self.send_typing();
        }
        cx.notify();
    }

    fn send_typing(&mut self) {
        let (Some(client), Some(channel)) = (self.client.clone(), self.channel.as_ref()) else { return };
        if !self.has_draft || self.last_typing_sent.is_some_and(|t| t.elapsed() < TYPING_SEND_EVERY) {
            return;
        }
        self.last_typing_sent = Some(Instant::now());
        let id = channel.id.clone();
        discord::runtime().spawn(async move { client.typing(&id).await });
    }

    /// Enter in the composer: saves the edit in progress, otherwise sends (optimistically).
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.held_unread = None;
        let (Some(client), Some(channel), Some(me)) = (self.client.clone(), self.channel.clone(), self.me.clone()) else { return };
        let typed = wire_text(&self.composer.read(cx).content());
        let typed = typed.trim();
        if typed.is_empty() && self.pending_files.is_empty() {
            return;
        }
        let content = self.to_wire(typed);
        self.composer.update(cx, |state, cx| state.set_value("", window, cx));
        self.has_draft = false;
        self.send_error = None;
        self.last_typing_sent = None;
        if let Some(editing) = self.editing.take() {
            self.restore_draft(window, cx);
            return self.edit(editing, content, cx);
        }
        self.drafts.clear(&channel.id);
        self.save_drafts_soon(cx);
        // Over Discord's limit, the text goes as a message.txt instead of being refused.
        if content.chars().count() > discord::message_limit(self.my_account.premium_type) {
            match attachments::save_message_text(&content).ok().and_then(attachments::PendingFile::new) {
                Some(file) => {
                    self.pending_files.push(file);
                    return self.send_files(String::new(), cx);
                }
                None => {
                    self.send_error = Some("Couldn't turn the long message into a file".into());
                    return cx.notify();
                }
            }
        }
        if !self.pending_files.is_empty() {
            return self.send_files(content, cx);
        }
        let nonce = nonce();
        let reply = self.replying.take();
        let ping = self.reply_ping;
        let pending = Message {
            id: format!("pending-{nonce}"),
            channel_id: channel.id.clone(),
            guild_id: self.guild_id.clone(),
            author: me,
            content: content.clone(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            reply_to: reply.as_ref().map(|r| r.id.clone()),
            reply: reply.clone().map(Box::new),
            nonce: Some(nonce.clone()),
            state: SendState::Sending,
            ..Default::default()
        };
        self.push_local(pending, cx);
        cx.spawn(async move |this, cx| {
            let result = client.send_message(&channel.id, &content, &nonce, reply.as_ref().map(|r| r.id.as_str()), ping).await;
            this.update(cx, |this, cx| this.settle(&channel.id, &nonce, result, cx)).ok();
        })
        .detach();
    }

    fn push_local(&mut self, message: Message, cx: &mut Context<Self>) {
        let is_open = self.channel.as_ref().is_some_and(|c| c.id == message.channel_id);
        let feed = self.feeds.entry(message.channel_id.clone()).or_default();
        let at = feed.messages.len();
        self.arrivals.borrow_mut().fresh.insert(message.id.clone());
        Arc::make_mut(&mut feed.messages).push(message);
        if is_open {
            self.list.splice(at..at, 1);
            self.list.scroll_to_end();
        }
        cx.notify();
    }

    // The REST reply and the gateway echo race; whichever lands first replaces the pending copy.
    fn settle(&mut self, channel_id: &str, nonce: &str, result: Result<Message, String>, cx: &mut Context<Self>) {
        // Detached, not dropped: settle runs inside that task.
        if let Some(task) = self.upload_tasks.remove(nonce) {
            task.detach();
        }
        // Before the pending lookup: when the gateway echo landed first there is no pending copy left,
        // and the job (with its upload cards) would stay forever.
        if result.is_ok() {
            self.forget_upload(nonce);
        }
        let pending = self.feeds.get_mut(channel_id).and_then(|f| Arc::make_mut(&mut f.messages).iter_mut().find(|m| m.nonce.as_deref() == Some(nonce) && m.state != SendState::Sent));
        let Some(pending) = pending else { return };
        match result {
            Ok(sent) => *pending = sent,
            Err(error) => {
                pending.state = SendState::Failed;
                match self.uploads.get_mut(nonce) {
                    Some(job) => job.error = Some(error),
                    None => self.send_error = Some(format!("Couldn't send: {error}").into()),
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn retry(&mut self, message: Message, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let Some(nonce) = message.nonce.clone() else { return };
        if let Some(m) = self.message_mut(&message.channel_id, &message.id) {
            m.state = SendState::Sending;
        }
        self.send_error = None;
        if self.uploads.contains_key(&nonce) {
            return self.start_upload(nonce, cx);
        }
        cx.spawn(async move |this, cx| {
            let result = match (message.stickers.first(), poll::draft_of(&message)) {
                (Some(sticker), _) => client.send_sticker(&message.channel_id, &sticker.id, &nonce, message.reply_to.as_deref(), true).await,
                (None, Some(draft)) => client.send_poll(&message.channel_id, &draft, &nonce).await,
                (None, None) => client.send_message(&message.channel_id, &message.content, &nonce, message.reply_to.as_deref(), true).await,
            };
            this.update(cx, |this, cx| this.settle(&message.channel_id, &nonce, result, cx)).ok();
        })
        .detach();
        cx.notify();
    }

    fn edit(&mut self, original: Message, content: String, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        if let Some(m) = self.message_mut(&original.channel_id, &original.id) {
            m.content = content.clone();
            m.edited = true;
        }
        cx.spawn(async move |this, cx| {
            if client.edit_message(&original.channel_id, &original.id, &content).await.is_ok() {
                return;
            }
            this.update(cx, |this, cx| {
                if let Some(m) = this.message_mut(&original.channel_id, &original.id) {
                    *m = original;
                }
                this.send_error = Some("Couldn't save your edit".into());
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn delete(&mut self, message: Message, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        self.remove_message(&message.channel_id, &message.id);
        cx.notify();
        if message.state != SendState::Sent {
            // Unsent: also how an upload is cancelled mid-flight.
            if let Some(nonce) = message.nonce.as_deref() {
                self.upload_tasks.remove(nonce);
                self.forget_upload(nonce);
            }
            return;
        }
        cx.spawn(async move |this, cx| {
            if client.delete_message(&message.channel_id, &message.id).await.is_ok() {
                return;
            }
            this.update(cx, |this, cx| {
                this.send_error = Some("Couldn't delete that message".into());
                this.fetch_latest(&message.channel_id, cx);
            })
            .ok();
        })
        .detach();
    }

    fn toggle_reaction(&mut self, message: &Message, emoji: Emoji, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let add = !message.reactions.iter().any(|r| r.emoji == emoji && r.me);
        if add {
            self.update_prefs(cx, |p| p.record_emoji(&emoji));
            self.save_reaction_frecency(cx);
        }
        let (channel_id, message_id) = (message.channel_id.clone(), message.id.clone());
        let me = self.my_id().to_owned();
        self.reactors_changed(&message_id, &emoji, &me, add);
        if let Some(m) = self.message_mut(&channel_id, &message_id) {
            adjust_reaction(m, &emoji, add, true, false);
        }
        cx.spawn(async move |this, cx| {
            if client.react(&channel_id, &message_id, &emoji, add).await.is_ok() {
                return;
            }
            this.update(cx, |this, cx| {
                this.reactors_changed(&message_id, &emoji, &me, !add);
                if let Some(m) = this.message_mut(&channel_id, &message_id) {
                    adjust_reaction(m, &emoji, !add, true, false);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// The + button: pick files to attach; they wait in the composer until sent, as in Discord.
    fn upload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions { files: true, directories: false, multiple: true, prompt: None });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            this.update_in(cx, |this, window, cx| this.add_files(paths, window, cx)).ok();
        })
        .detach();
    }

    /// Picked, dropped or pasted files join the composer within Discord's caps, which say what they left out.
    fn add_files(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let files = paths.into_iter().filter_map(attachments::PendingFile::new).collect();
        let limit = discord::upload_limit(self.my_account.premium_type, self.guild().map_or(0, |g| g.premium_tier));
        self.send_error = attachments::admit(&mut self.pending_files, files, limit).map(Into::into);
        self.has_draft |= !self.pending_files.is_empty();
        self.load_previews(cx);
        self.composer.update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    // Tile pictures, drawn by the OS off the UI thread: images and videos as thumbnails where it can,
    // everything else (and any it can't) as the icon its file manager shows.
    fn load_previews(&mut self, cx: &mut Context<Self>) {
        const SIDE: u32 = 96;
        for file in &self.pending_files {
            if self.file_previews.contains_key(&file.path) {
                continue;
            }
            self.file_previews.insert(file.path.clone(), None);
            let (path, kind) = (file.path.clone(), file.kind());
            let picture = cx.background_spawn(async move {
                let thumb = (kind != attachments::FileKind::Other).then(|| os_image::thumbnail(&path, SIDE)).flatten();
                thumb.or_else(|| os_image::file_icon(&path, SIDE))
            });
            let path = file.path.clone();
            cx.spawn(async move |this, cx| {
                let picture = picture.await;
                this.update(cx, |this, cx| {
                    this.file_previews.insert(path, picture);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    /// Cmd/Ctrl+V: files copied in Finder or Explorer, a picture (a screenshot) or text over 4000
    /// characters (as message.txt) attach; other text is left to the composer's own paste. True when it
    /// attached something.
    fn paste_files(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(item) = cx.read_from_clipboard() else { return false };
        let paths = match attachments::classify(&item) {
            attachments::Pasted::Text => return false,
            attachments::Pasted::Files(paths) => paths,
            attachments::Pasted::LongText(text) => match attachments::save_message_text(&text) {
                Ok(path) => vec![path],
                Err(error) => {
                    self.send_error = Some(format!("Couldn't attach that text: {error}").into());
                    cx.notify();
                    return true;
                }
            },
            attachments::Pasted::Image(image) => match attachments::save_pasted_image(&image) {
                Ok(path) => vec![path],
                Err(error) => {
                    self.send_error = Some(format!("Couldn't paste that image: {error}").into());
                    cx.notify();
                    return true;
                }
            },
        };
        self.add_files(paths, window, cx);
        true
    }

    fn send_files(&mut self, content: String, cx: &mut Context<Self>) {
        let (Some(channel), Some(me)) = (self.channel.clone(), self.me.clone()) else { return };
        // Previews stay until the upload settles: the feed's upload cards show them too.
        let files: Vec<discord::Upload> = std::mem::take(&mut self.pending_files).iter().map(attachments::PendingFile::upload).collect();
        let nonce = nonce();
        let reply = self.replying.take();
        let pending = Message {
            id: format!("pending-{nonce}"),
            channel_id: channel.id.clone(),
            guild_id: self.guild_id.clone(),
            author: me,
            content: content.clone(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            reply_to: reply.as_ref().map(|r| r.id.clone()),
            reply: reply.clone().map(Box::new),
            nonce: Some(nonce.clone()),
            state: SendState::Sending,
            ..Default::default()
        };
        let job = UploadJob { channel_id: channel.id.clone(), files, content, reply_to: reply.map(|r| r.id), ping: self.reply_ping, error: None };
        self.uploads.insert(nonce.clone(), job);
        self.push_local(pending, cx);
        self.start_upload(nonce, cx);
    }

    // Runs (or re-runs, for Resend) the upload under `nonce`, repainting its progress as bytes go out.
    fn start_upload(&mut self, nonce: String, cx: &mut Context<Self>) {
        let (Some(client), Some(job)) = (self.client.clone(), self.uploads.get_mut(&nonce)) else { return };
        job.error = None;
        let job = job.clone();
        let ticking = nonce.clone();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(100)).await;
                let sending = this.update(cx, |this, cx| {
                    cx.notify();
                    this.uploads.get(&ticking).is_some_and(|job| job.error.is_none())
                });
                if !sending.unwrap_or(false) {
                    break;
                }
            }
        })
        .detach();
        let task = cx.spawn({
            let nonce = nonce.clone();
            async move |this, cx| {
                let result = client.upload(&job.channel_id, &job.files, &job.content, &nonce, job.reply_to.as_deref(), job.ping).await;
                this.update(cx, |this, cx| this.settle(&job.channel_id, &nonce, result, cx)).ok();
            }
        });
        self.upload_tasks.insert(nonce, task);
        cx.notify();
    }

    fn forget_upload(&mut self, nonce: &str) {
        for file in self.uploads.remove(nonce).into_iter().flat_map(|job| job.files) {
            self.file_previews.remove(&file.path);
        }
    }

    fn remove_file(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.pending_files.len() {
            let file = self.pending_files.remove(index);
            self.file_previews.remove(&file.path);
        }
        self.send_error = None;
        self.has_draft = !self.pending_files.is_empty() || !self.composer.read(cx).value().trim().is_empty();
        cx.notify();
    }

    fn toggle_spoiler(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(file) = self.pending_files.get_mut(index) {
            file.spoiler = !file.spoiler;
        }
        cx.notify();
    }

    fn open_pins(&mut self, cx: &mut Context<Self>) {
        let (Some(client), Some(channel)) = (self.client.clone(), self.channel.clone()) else { return };
        self.pins = Some(None);
        self.pins_list.reset(0);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = client.pins(&channel.id).await;
            this.update(cx, |this, cx| {
                if this.pins.is_some() {
                    this.pins = Some(Some(result.map_err(Into::into)));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Alt+↑/↓: the previous or next text channel in the sidebar's order, as in Discord.
    fn step_channel(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let order: Vec<Channel> = match self.guild() {
            Some(guild) => sidebar::channel_sections(guild, self.my_id()).into_iter().flat_map(|(_, c)| c).filter(|c| c.kind == ChannelKind::Text).collect(),
            None => self.dms.clone(),
        };
        let current = self.channel.as_ref().and_then(|c| order.iter().position(|o| o.id == c.id)).unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, order.len() as isize - 1);
        if let Some(channel) = order.get(next as usize).cloned() {
            self.open_channel(channel, window, cx);
        }
    }

    fn set_talk_key(&mut self, down: bool) {
        if self.talk_key_down == down {
            return;
        }
        self.talk_key_down = down;
        if let Some(call) = &self.call {
            call.session.set_talk_key(down);
            crate::sounds::play(if down { crate::sounds::Cue::PttStart } else { crate::sounds::Cue::PttStop });
        }
    }

    fn start_reply(&mut self, message: Message, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = None;
        self.reply_ping = true;
        self.replying = Some(message);
        self.composer.update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    fn start_edit(&mut self, message: Message, window: &mut Window, cx: &mut Context<Self>) {
        self.replying = None;
        let text = self.shown_text(&message.content);
        self.editing = Some(message);
        self.composer.update(cx, |state, cx| {
            state.set_value(text, window, cx);
            state.focus(window, cx);
        });
        self.has_draft = true;
        cx.notify();
    }

    fn cancel_compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.autocomplete = None;
        if self.editing.take().is_some() {
            self.restore_draft(window, cx);
            return cx.notify();
        }
        self.replying = None;
        cx.notify();
    }

    // Keeps the open channel's composer as its draft; an edit in progress is not one.
    fn stash_draft(&mut self, cx: &mut Context<Self>) {
        let Some(channel_id) = self.channel.as_ref().map(|c| c.id.clone()) else { return };
        if self.editing.is_some() {
            return;
        }
        let (command, picks) = self.command_draft(cx);
        let draft = drafts::Draft::new(&self.composer.read(cx).content(), self.replying.clone()).with_command(command, picks);
        self.drafts.set(&channel_id, draft);
        self.save_drafts_soon(cx);
    }

    // Puts the open channel's draft back in the composer (or empties it), caret at the end.
    fn restore_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let draft = self.channel.as_ref().and_then(|c| self.drafts.get(&c.id)).cloned().unwrap_or_default();
        let content = draft.content();
        let end = content.text().len();
        self.has_draft = !content.text().trim().is_empty() || !self.pending_files.is_empty();
        self.composer.update(cx, |state, cx| {
            state.set_value(content, window, cx);
            state.set_selected_range(end..end, cx);
        });
        self.restore_command(draft.command, draft.picks, cx);
        self.replying = draft.reply;
    }

    // Typing rewrites the drafts file at most once per pause.
    fn save_drafts_soon(&mut self, cx: &mut Context<Self>) {
        self.drafts_save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DRAFTS_SAVE_AFTER).await;
            this.update(cx, |this, _| this.drafts.save()).ok();
        }));
    }

    /// Up arrow in an empty composer edits your last message, like Discord.
    fn edit_last(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let (Some(me), Some(channel)) = (&self.me, &self.channel) else { return false };
        let last = self.feeds.get(&channel.id).and_then(|f| f.messages.iter().rev().find(|m| m.author.id == me.id && m.state == SendState::Sent)).cloned();
        let Some(last) = last else { return false };
        self.start_edit(last, window, cx);
        true
    }

    fn suggest(&self, text: &str) -> Option<Autocomplete> {
        let last = text.rsplit(char::is_whitespace).next()?;
        if let Some(query) = composer_emoji::emoji_query(last) {
            let items: Vec<Suggestion> = self
                .emoji_suggestions(query)
                .into_iter()
                .map(|m| {
                    let shown = format!(":{}:", m.name);
                    let wire = match &m.emoji.id {
                        Some(id) => format!("<{}:{}:{id}>", if m.emoji.animated { "a" } else { "" }, m.emoji.name),
                        None => m.emoji.name.clone(),
                    };
                    Suggestion { label: shown, insert: wire.clone(), wire, lead: SuggestLead::Emoji(m.emoji), detail: m.server }
                })
                .collect();
            return (!items.is_empty()).then(|| Autocomplete { token: last.to_owned(), items, selected: 0 });
        }
        // A bare `@` or `#` already lists everyone or every channel, as Discord does.
        let token = Some(last).filter(|t| t.starts_with('@') || t.starts_with('#'))?;
        let query = token[1..].to_lowercase();
        let mut items: Vec<Suggestion> = Vec::new();
        if token.starts_with('#') {
            for c in self.guild().map(|g| g.channels.as_slice()).unwrap_or_default() {
                if c.kind == ChannelKind::Text && c.name.to_lowercase().contains(&query) {
                    items.push(Suggestion { label: c.name.clone(), insert: format!("#{}", c.name), wire: format!("<#{}>", c.id), lead: SuggestLead::Channel, detail: None });
                }
            }
        } else {
            for user in self.mention_candidates() {
                if user.name.to_lowercase().contains(&query) && !items.iter().any(|s| s.wire == format!("<@{}>", user.id)) {
                    items.push(Suggestion { label: user.name.clone(), insert: format!("@{}", user.name), wire: format!("<@{}>", user.id), lead: SuggestLead::Avatar(user.avatar), detail: None });
                }
            }
        }
        items.truncate(8);
        (!items.is_empty()).then(|| Autocomplete { token: token.to_owned(), items, selected: 0 })
    }

    fn accept_suggestion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(auto) = self.autocomplete.take() else { return };
        let Some(pick) = auto.items.get(auto.selected) else { return };
        // An emoji goes in as itself: the character, or a custom one's `:name:` sent as its `<:name:id>`.
        if let SuggestLead::Emoji(emoji) = &pick.lead {
            let shown = if emoji.id.is_some() { pick.label.clone() } else { pick.wire.clone() };
            if emoji.id.is_some() {
                self.mention_wire.insert(shown.clone(), pick.wire.clone());
            }
            let emoji = emoji.clone();
            self.update_prefs(cx, |p| p.record_emoji(&emoji));
            self.composer.update(cx, |state, cx| complete_text(state, &auto.token, &shown, window, cx));
            self.has_draft = true;
            return cx.notify();
        }
        let chip = InlineToken::new(pick.wire.clone(), pick.insert.clone());
        self.composer.update(cx, |state, cx| complete_mention(state, &auto.token, chip, window, cx));
        cx.notify();
    }

    fn move_suggestion(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(auto) = &mut self.autocomplete else { return };
        auto.selected = (auto.selected as isize + delta).rem_euclid(auto.items.len() as isize) as usize;
        self.autocomplete_scroll.scroll_to_item(auto.selected);
        cx.notify();
    }

    // "@Ferris" → "<@id>": longest first, so "@Ann Marie" wins over "@Ann".
    fn to_wire(&self, text: &str) -> String {
        let mut pairs: Vec<_> = self.mention_wire.iter().collect();
        pairs.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));
        pairs.into_iter().fold(text.to_owned(), |text, (shown, wire)| text.replace(shown.as_str(), wire))
    }

    fn shown_text(&self, text: &str) -> String {
        self.mention_wire.iter().fold(text.to_owned(), |text, (shown, wire)| text.replace(wire.as_str(), shown))
    }

    // Who `@` offers: in a DM or group DM only its own people, in a server everyone known there; you last.
    fn mention_candidates(&self) -> Vec<User> {
        let me = self.me.as_ref().map(|m| m.id.as_str());
        match self.channel.as_ref().filter(|_| self.guild_id.is_none()) {
            Some(dm) => mention_order(dm.recipients.iter().cloned().chain(dm.recipient.clone()).chain(self.me.clone()), me),
            None => {
                let members = self.guild_id.as_ref().and_then(|g| self.member_lists.get(g)).into_iter().flatten().filter_map(|i| match i {
                    ListItem::Member(m) => Some(m.user.clone()),
                    ListItem::Group { .. } => None,
                });
                let authors = self.channel.as_ref().and_then(|c| self.feeds.get(&c.id)).into_iter().flat_map(|f| f.messages.iter().rev().map(|m| m.author.clone()));
                mention_order(members.chain(authors), me)
            }
        }
    }

    pub(crate) fn known_users(&self) -> Vec<User> {
        let mut users: Vec<User> = Vec::new();
        if let Some(list) = self.guild_id.as_ref().and_then(|g| self.member_lists.get(g)) {
            users.extend(list.iter().filter_map(|i| match i {
                ListItem::Member(m) => Some(m.user.clone()),
                ListItem::Group { .. } => None,
            }));
        }
        if let Some(feed) = self.channel.as_ref().and_then(|c| self.feeds.get(&c.id)) {
            users.extend(feed.messages.iter().rev().map(|m| m.author.clone()));
        }
        users.extend(self.dms.iter().flat_map(|d| d.recipients.iter().cloned().chain(d.recipient.clone())));
        users
    }

    // Asked once per server per connection, when it's opened, as Discord does.
    fn request_channel_info(&mut self) {
        let (Some(guild_id), Some(client)) = (self.guild_id.clone(), self.client.as_ref()) else { return };
        if self.channel_info_requested.insert(guild_id.clone()) {
            client.request_channel_info(&guild_id);
        }
    }

    // How long a voice channel's current session has run, from Discord's start time.
    pub(crate) fn voice_elapsed(&self, channel_id: &str) -> Option<u64> {
        let start = *self.voice_start.get(channel_id)?;
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
        Some(now.saturating_sub(start))
    }

    fn user_avatar(&self, user_id: &str) -> String {
        self.known_users().into_iter().find(|u| u.id == user_id).map(|u| u.avatar).unwrap_or_else(|| discord::avatar_url(user_id, None))
    }

    fn user_name(&self, user_id: &str) -> String {
        self.known_users().into_iter().find(|u| u.id == user_id).map(|u| u.name).unwrap_or_else(|| "Someone".into())
    }

    // A message changed in place (edit, reaction, embed, pin). gpui's list keeps each row's
    // measured height until told otherwise, so the open channel's row is re-measured.
    pub(crate) fn message_mut(&mut self, channel_id: &str, id: &str) -> Option<&mut Message> {
        let open = self.channel.as_ref().is_some_and(|c| c.id == channel_id);
        let feed = self.feeds.get_mut(channel_id)?;
        let ix = feed.messages.iter().position(|m| m.id == id)?;
        if open {
            self.list.remeasure_items(ix..ix + 1);
        }
        Arc::make_mut(&mut feed.messages).get_mut(ix)
    }

    fn remove_message(&mut self, channel_id: &str, id: &str) {
        let is_open = self.channel.as_ref().is_some_and(|c| c.id == channel_id);
        let Some(feed) = self.feeds.get_mut(channel_id) else { return };
        let Some(ix) = feed.messages.iter().position(|m| m.id == id) else { return };
        Arc::make_mut(&mut feed.messages).remove(ix);
        if is_open {
            self.list.splice(ix..ix + 1, 0);
        }
    }

    fn guild(&self) -> Option<&Guild> {
        let id = self.guild_id.as_ref()?;
        self.guilds.iter().find(|g| &g.id == id)
    }

    fn mentions_me(&self, message: &Message) -> bool {
        let Some(me) = &self.me else { return false };
        message.mention_everyone || message.mentions.iter().any(|u| u.id == me.id) || self.mentions_role(message)
    }

    // A thread's parent channel, for the header breadcrumb and the sidebar.
    fn thread_parent(&self, channel: &Channel) -> Option<Channel> {
        if channel.kind != ChannelKind::Thread {
            return None;
        }
        let parent = channel.parent_id.as_ref()?;
        self.guilds.iter().flat_map(|g| g.channels.iter()).find(|c| &c.id == parent).cloned()
    }

    // Whether the message pings one of our roles in its server.
    fn mentions_role(&self, message: &Message) -> bool {
        let Some(me) = &self.me else { return false };
        let mine = self.guild_id_of(message).and_then(|g| self.member_roles.get(&g)).and_then(|r| r.get(&me.id));
        mine.is_some_and(|roles| message.mention_roles.iter().any(|r| roles.contains(r)))
    }

    /// The colour of the user's highest coloured role, clamped so the name stays readable in this theme.
    // The name the open server shows for someone: their nickname once the member is known.
    pub(crate) fn member_name(&self, user: &discord::User) -> String {
        let member = self.guild_id.as_ref().and_then(|g| self.members.get(g)).and_then(|m| m.get(&user.id));
        member.map_or_else(|| user.name.clone(), |m| m.name.clone())
    }

    fn role_color(&self, guild_id: Option<&String>, user_id: &str, hint: Option<&Vec<String>>, dark: bool) -> Option<Hsla> {
        let guild = self.guilds.iter().find(|g| Some(&g.id) == guild_id)?;
        let roles = hint.or_else(|| self.member_roles.get(&guild.id)?.get(user_id))?;
        guild.roles.iter().filter(|r| r.color != 0 && roles.contains(&r.id)).max_by_key(|r| r.position).map(|r| palette::role_ink(r.color, dark))
    }

    fn unread(&self, channel_id: &str) -> bool {
        let (Some(last), Some(read)) = (self.last_message.get(channel_id), self.read_states.get(channel_id)) else { return false };
        *last > read.last_read
    }

    /// Our user id, or "" before READY.
    pub(crate) fn my_id(&self) -> &str {
        self.me.as_ref().map_or("", |m| m.id.as_str())
    }

    fn mention_count(&self, channel_id: &str) -> u32 {
        self.read_states.get(channel_id).map_or(0, |r| r.mentions)
    }

    // A muted server, or muted channel, keeps its mention count but drops its unread pill, as in Discord.
    fn guild_badge(&self, guild: &Guild) -> (bool, u32) {
        let settings = self.guild_settings.get(&guild.id);
        let server_muted = settings.is_some_and(|s| s.is_muted());
        let me = self.my_id();
        let text = guild.channels.iter().filter(|c| c.kind == ChannelKind::Text && guild.can_view(me, c));
        text.fold((false, 0), |(unread, mentions), c| {
            let muted = server_muted || settings.is_some_and(|s| s.channel_muted(&c.id) || c.parent_id.as_ref().is_some_and(|p| s.channel_muted(p)));
            (unread || (self.unread(&c.id) && !muted), mentions + self.mention_count(&c.id))
        })
    }

    fn typers(&self) -> Vec<Typer> {
        let Some(channel) = &self.channel else { return Vec::new() };
        let now = Instant::now();
        self.typing.get(&channel.id).map(|t| t.iter().filter(|t| t.until > now).cloned().collect()).unwrap_or_default()
    }

    // The call Waffle's own restart left (restart.rs), offered back under the notice bar. Once only: a
    // channel gone or hidden is dropped.
    fn rejoin_after_restart(&mut self, cx: &mut Context<Self>) {
        let Some((guild_id, channel_id)) = self.prefs.rejoin_voice.take() else { return };
        self.prefs.save();
        let channel = self.guilds.iter().find(|g| g.id == guild_id).and_then(|g| g.channels.iter().find(|c| c.id == channel_id)).cloned();
        if let Some(channel) = channel {
            self.voice_notice = Some(VoiceNotice { guild_id: Some(guild_id), channel, at: Instant::now(), why: NoticeWhy::Restarted });
            cx.notify();
        }
    }

    fn join_voice(&mut self, channel: Channel, cx: &mut Context<Self>) {
        self.join_voice_in(self.guild_id.clone(), channel, cx);
    }

    // The call's server, not the one on screen: a move can come while another server is open.
    fn follow_voice_move(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        let guild_id = self.call.as_ref().and_then(|c| c.guild_id.clone());
        let channel = self.guilds.iter().find(|g| Some(&g.id) == guild_id.as_ref()).and_then(|g| g.channels.iter().find(|c| c.id == channel_id)).cloned();
        match channel {
            Some(channel) => self.join_voice_in(guild_id, channel, cx),
            // We can't show that channel, so we don't stay in it: Discord still has us there.
            None => {
                if let Some(client) = &self.client {
                    client.voice.leave_voice(guild_id.as_deref(), self.muted, self.deafened);
                }
                self.leave_voice(cx);
                self.voice_error = Some("Moved to a voice channel Waffle can't show".into());
            }
        }
    }

    fn join_voice_in(&mut self, guild_id: Option<String>, channel: Channel, cx: &mut Context<Self>) {
        let (Some(client), Some(me)) = (self.client.clone(), self.me.clone()) else { return };
        if self.call.as_ref().is_some_and(|c| c.channel.id == channel.id) {
            return;
        }
        let switched = guild_id.is_some() && self.call.as_ref().is_some_and(|c| c.guild_id == guild_id);
        if let Some(call) = &self.call {
            call.session.supersede();
        }
        match switched {
            true => _ = self.end_call(cx),
            false => self.leave_voice(cx),
        }
        self.call_switched = switched;
        self.voice_error = None;
        self.voice_notice = None;
        // Its playback is outside the call's echo cancellation (see `close_settings`). Before the
        // join, so the call starts with the deafen state the test held.
        self.stop_mic_test();
        let (session, mut events) = voice::join(me.id, guild_id.clone(), channel.id.clone(), client.voice.clone(), self.prefs.voice_settings());
        for (user_id, volume) in &self.prefs.user_volumes {
            session.set_user_volume(user_id, *volume);
        }
        for user_id in &self.prefs.local_mutes {
            session.set_user_muted(user_id, true);
        }
        if self.muted || self.deafened {
            session.set_flags(true, self.deafened);
        }
        let task = cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this.update(cx, |this, cx| this.on_voice(event, cx)).is_err() {
                    break;
                }
            }
        });
        // Ticks the "Voice Connected" timer once a second while in the call.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let in_call = this.update(cx, |this, cx| {
                    cx.notify();
                    this.call.is_some()
                });
                if !matches!(in_call, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
        self.call = Some(Call { session, guild_id, channel, connected: false, stage: voice::Stage::AwaitingEndpoint, stats: None, level: (SILENCE_DB, false), speaking: HashSet::new(), last_spoke: HashMap::new(), cameras: HashMap::new(), camera_on: false, watching: Vec::new(), focus: None, ended: Vec::new(), sharing: None, share_label: None, share_source: None, live_since: None, self_view: None, issues: HashSet::new(), joined: Instant::now(), _events: task });
        cx.notify();
    }

    // Another client of ours joined voice. The voice server can drop this one (4006) before that
    // client's voice state reaches us, so a call that ended moments ago is explained after the fact.
    fn taken_elsewhere(&mut self) {
        if self.call.is_some() {
            return;
        }
        let Some(last) = self.last_call.take().filter(|c| c.at.elapsed() < TAKEN_ELSEWHERE_WINDOW) else { return };
        self.voice_error = None;
        self.voice_notice = Some(last);
    }

    fn reconnect_voice(&mut self, cx: &mut Context<Self>) {
        let Some(notice) = self.voice_notice.take() else { return };
        self.join_voice_in(notice.guild_id, notice.channel, cx);
    }

    fn ring_out(&mut self, channel: String, cx: &mut Context<Self>) {
        sounds::ring(sounds::Cue::CallCalling);
        self.ringing_out = Some(channel.clone());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(60)).await;
            this.update(cx, |this, _| {
                if this.ringing_out.as_ref() == Some(&channel) {
                    this.stop_ringing_out();
                }
            })
            .ok();
        })
        .detach();
    }

    fn stop_ringing_out(&mut self) {
        if self.ringing_out.take().is_some() {
            sounds::stop_ring();
        }
    }

    fn leave_voice(&mut self, cx: &mut Context<Self>) {
        let stage = self.call.as_ref().is_some_and(|c| c.channel.raw_type == 13);
        if self.end_call(cx) && !stage {
            sounds::play(sounds::Cue::Disconnect);
        }
    }

    // Ends the call without a sound; whether there was one.
    fn end_call(&mut self, cx: &mut Context<Self>) -> bool {
        self.call_fullscreen = false;
        self.camera_on_connect = false;
        self.stop_ringing_out();
        let Some(call) = self.call.take() else { return false };
        self.call_focus = None;
        if call.guild_id.is_none() {
            let name = self.dms.iter().find(|d| d.id == call.channel.id).map_or_else(|| call.channel.name.clone(), |d| d.name.clone());
            self.show_call_ended(name, call.joined.elapsed().as_secs(), cx);
        }
        release_frames(call, cx);
        cx.notify();
        true
    }

    fn on_voice(&mut self, event: voice::VoiceEvent, cx: &mut Context<Self>) {
        // A stream picked from the hover card before we'd joined: watch it once connected.
        let mut start_watch: Option<String> = None;
        let mut lapsed: Option<String> = None;
        let mut ready_checks: Vec<String> = Vec::new();
        let Some(call) = &mut self.call else { return };
        match event {
            voice::VoiceEvent::Connected => {
                if self.ring_on_connect.as_ref() == Some(&call.channel.id) {
                    let channel = self.ring_on_connect.take().unwrap_or_default();
                    self.ring_dm(channel, cx);
                }
                start_watch = self.watch_on_connect.take();
                let Some(call) = &mut self.call else { return };
                call.connected = true;
                call.stage = voice::Stage::Connected;
                // Stages make no join or leave sounds (docs/discord-stage-re.md).
                if call.channel.raw_type != 13 {
                    sounds::play(connected_cue(self.call_switched));
                }
                if std::mem::take(&mut self.camera_on_connect) {
                    self.toggle_camera(cx);
                }
            }
            voice::VoiceEvent::Stage(stage) => call.stage = stage,
            voice::VoiceEvent::Stats(stats) => call.stats = Some(stats),
            voice::VoiceEvent::InputLevel { db, gate_open } => {
                call.level = (db, gate_open);
                if self.mic_test.is_none() {
                    self.mic_level = db;
                }
            }
            voice::VoiceEvent::Speaking { user_id, speaking: true } => {
                call.speaking.insert(user_id);
            }
            voice::VoiceEvent::Speaking { user_id, speaking: false } => {
                call.speaking.remove(&user_id);
                call.last_spoke.insert(user_id, Instant::now());
            }
            voice::VoiceEvent::VideoState { user_id, streaming, camera } => {
                let guild_id = call.guild_id.clone();
                let state = self.guilds.iter_mut().filter(|g| Some(&g.id) == guild_id.as_ref()).flat_map(|g| g.voice_states.iter_mut()).find(|v| v.user_id == user_id);
                if let Some(state) = state {
                    state.streaming = streaming;
                    state.camera = camera;
                }
                // Live again while their ended card is still up: rejoin, as Discord does. Close stream clears it.
                if streaming && call.ended.contains(&user_id) {
                    start_watch = Some(user_id.clone());
                }
                if !streaming && call.watched(&user_id).is_some_and(|w| !w.ended()) {
                    lapsed = Some(user_id.clone());
                }
                // A camera tile is drawn while it has a frame: without this, a camera turned off froze on its last one.
                if camera && !call.cameras.contains_key(&user_id) {
                    ready_checks.push(format!("camera:{user_id}"));
                }
                if !camera {
                    call.issues.remove(&(format!("camera:{user_id}"), voice::issues::RECEIVER_READY_TIMEOUT));
                }
                if !camera && let Some(frame) = call.cameras.remove(&user_id) {
                    cx.drop_image(frame, None);
                }
            }
            voice::VoiceEvent::StreamFrame { user_id, source: voice::VideoSource::GoLive } => {
                // Taken whatever happens next: an untaken frame holds back the next one's event.
                let Some(frame) = call.session.take_frame(&user_id, voice::VideoSource::GoLive) else { return };
                let mine = self.me.as_ref().is_some_and(|m| m.id == user_id);
                let watching_self = call.watched(&user_id).is_some();
                if mine && self.prefs.hidden_self_streams.contains(&call.channel.id) {
                    if let Some((old, _)) = call.self_view.take() {
                        cx.drop_image(old, None);
                    }
                    return;
                }
                // The self-view runs at 15 fps: a full-rate copy of every frame isn't worth it for a thumbnail.
                if mine && !watching_self {
                    if call.self_view.as_ref().is_some_and(|(_, at)| at.elapsed() < SELF_VIEW_INTERVAL) {
                        return;
                    }
                    let Some(image) = render_image(frame) else { return };
                    if let Some((old, _)) = call.self_view.replace((image, Instant::now())) {
                        cx.drop_image(old, None);
                    }
                    return cx.notify();
                }
                call.issues.remove(&(format!("stream:{user_id}"), voice::issues::RECEIVER_READY_TIMEOUT));
                let Some(watching) = call.watching.iter_mut().find(|w| w.streamer_id == user_id) else { return };
                let Some(image) = render_image(frame) else { return };
                watching.rejoins = 0;
                watching.state = WatchState::Active;
                watching.arrivals.push_back(Instant::now());
                while watching.arrivals.len() > 120 {
                    watching.arrivals.pop_front();
                }
                if let Some(old) = watching.frame.replace(image) {
                    // Each frame is a new GPU texture; the old one must be freed or the atlas grows forever.
                    cx.drop_image(old, None);
                }
            }
            voice::VoiceEvent::StreamFrame { user_id, source: voice::VideoSource::Camera } => {
                let Some(frame) = call.session.take_frame(&user_id, voice::VideoSource::Camera) else { return };
                call.issues.remove(&(format!("camera:{user_id}"), voice::issues::RECEIVER_READY_TIMEOUT));
                let Some(image) = render_image(frame) else { return };
                if let Some(old) = call.cameras.insert(user_id, image) {
                    cx.drop_image(old, None);
                }
            }
            voice::VoiceEvent::Issue { user_id, code, active } => {
                match active {
                    true => call.issues.insert((user_id, code)),
                    false => call.issues.remove(&(user_id, code)),
                };
            }
            voice::VoiceEvent::StreamConnected { user_id } => {
                if let Some(watching) = call.watching.iter_mut().find(|w| w.streamer_id == user_id)
                    && matches!(watching.state, WatchState::Connecting(_) | WatchState::Reconnecting)
                {
                    watching.state = WatchState::Active;
                }
                ready_checks.push(format!("stream:{user_id}"));
            }
            voice::VoiceEvent::StreamEnded { user_id, reason } => {
                if reason != "Stopped" && !is_delete_reason(&reason) {
                    eprintln!("voice: stream ended: {reason}");
                }
                // A stream that was replaced or watched again ends after its successor started; only the
                // one whose own handle has ended is over.
                if self.me.as_ref().is_some_and(|m| m.id == user_id) {
                    let capture_failed = call.issues.contains(&(user_id.clone(), voice::issues::SCREENSHARE_OS_ERROR));
                    if call.sharing.as_ref().is_some_and(voice::GoLive::ended) {
                        self.end_share(cx);
                        if capture_failed {
                            self.voice_error = Some(format!("Your system stopped the screen capture. Error code: {}", voice::issues::SCREENSHARE_OS_ERROR).into());
                        }
                    }
                    return;
                }
                if !call.watched(&user_id).is_some_and(Watching::ended) {
                    return;
                }
                return self.on_watch_ended(user_id, reason, cx);
            }
            voice::VoiceEvent::Moved(channel_id) => return self.follow_voice_move(&channel_id, cx),
            voice::VoiceEvent::Disconnected(reason) => {
                let stage = self.call.as_ref().is_some_and(|c| c.channel.raw_type == 13);
                if let Some(call) = self.call.take() {
                    self.last_call = Some(VoiceNotice { guild_id: call.guild_id.clone(), channel: call.channel.clone(), at: Instant::now(), why: NoticeWhy::Elsewhere });
                    release_frames(call, cx);
                }
                self.call_focus = None;
                self.stop_ringing_out();
                if !stage {
                    sounds::play(sounds::Cue::Disconnect);
                }
                match reason.as_str() {
                    "Left" => {}
                    voice::ANOTHER_LOCATION => self.taken_elsewhere(),
                    _ => self.voice_error = Some(reason.into()),
                }
            }
        }
        if let Some(streamer) = start_watch {
            self.watch_stream(streamer, cx);
        }
        if let Some(streamer) = lapsed {
            self.on_live_lapse(streamer, cx);
        }
        for key in ready_checks {
            self.check_ready(key, cx);
        }
        cx.notify();
    }

    // Adds `streamer_id`'s Go Live to the streams we watch and focuses it; Discord lets you watch several.
    fn watch_stream(&mut self, streamer_id: String, cx: &mut Context<Self>) {
        let mine = self.me.as_ref().is_some_and(|m| m.id == streamer_id);
        let Some(call) = &mut self.call else { return };
        call.ended.retain(|id| *id != streamer_id);
        call.focus = Some(streamer_id.clone());
        if call.watched(&streamer_id).is_some() {
            return cx.notify();
        }
        let watch = match mine {
            true if call.sharing.is_some() => None,
            true => return cx.notify(),
            false => match call.session.watch_stream(&streamer_id) {
                Ok(watch) => Some(watch),
                Err(error) => {
                    self.voice_error = Some(error.into());
                    return cx.notify();
                }
            },
        };
        let timeout = if call.channel.raw_type == STAGE_CHANNEL { STAGE_WATCH_TIMEOUT } else { WATCH_TIMEOUT };
        call.watching.push(Watching::new(streamer_id.clone(), None, watch));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(timeout).await;
            this.update(cx, |this, cx| this.watch_timed_out(&streamer_id, timeout, cx)).ok();
        })
        .detach();
        cx.notify();
    }

    fn watch_timed_out(&mut self, streamer_id: &str, timeout: Duration, cx: &mut Context<Self>) {
        let Some(watching) = self.call.as_mut().and_then(|c| c.watching.iter_mut().find(|w| w.streamer_id == streamer_id)) else { return };
        if matches!(watching.state, WatchState::Connecting(since) if since.elapsed() >= timeout) {
            watching.state = WatchState::Failed(STREAM_FAILED_TO_START);
            cx.notify();
        }
    }

    // A watched stream's connection ended, handled as Discord handles STREAM_DELETE: unavailable or a
    // dropped connection reconnects, unauthorized fails, full closes with a notice, anything else ends.
    fn on_watch_ended(&mut self, streamer_id: String, reason: String, cx: &mut Context<Self>) {
        let name = self.user_name(&streamer_id);
        let Some(watching) = self.call.as_mut().and_then(|c| c.watching.iter_mut().find(|w| w.streamer_id == streamer_id)) else { return };
        match reason.as_str() {
            voice::STREAM_DELETE_FULL => {
                self.unwatch_stream(&streamer_id, cx);
                self.voice_error = Some(format!("{name}'s stream is full").into());
            }
            voice::STREAM_DELETE_UNAUTHORIZED => fail_watch(watching, cx),
            // Already failed: the delete that follows a failure keeps it failed, as Discord's.
            _ if matches!(watching.state, WatchState::Failed(_)) => {}
            voice::STREAM_DELETE_UNAVAILABLE => self.reconnect_watch(streamer_id, cx),
            _ if is_delete_reason(&reason) => self.end_watch(streamer_id, cx),
            // Never connected: the stream server didn't answer or refused us.
            _ if matches!(watching.state, WatchState::Connecting(_)) => fail_watch(watching, cx),
            _ => self.reconnect_watch(streamer_id, cx),
        }
    }

    fn reconnect_watch(&mut self, streamer_id: String, cx: &mut Context<Self>) {
        let Some(watching) = self.call.as_mut().and_then(|c| c.watching.iter_mut().find(|w| w.streamer_id == streamer_id)) else { return };
        if watching.rejoins >= MAX_REJOINS {
            return fail_watch(watching, cx);
        }
        watching.state = WatchState::Reconnecting;
        // The tile shows the reconnecting state rather than a frozen picture.
        if let Some(frame) = watching.frame.take() {
            cx.drop_image(frame, None);
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(REJOIN_AFTER).await;
            this.update(cx, |this, cx| this.rewatch(streamer_id, cx)).ok();
        })
        .detach();
        cx.notify();
    }

    fn rewatch(&mut self, streamer_id: String, cx: &mut Context<Self>) {
        let live = self.participants().iter().any(|s| s.user_id == streamer_id && s.streaming);
        let Some(call) = &mut self.call else { return };
        // Stopped or watched again by hand meanwhile.
        let Some(watching) = call.watching.iter_mut().find(|w| w.streamer_id == streamer_id && w.state == WatchState::Reconnecting) else { return };
        if !live {
            return self.end_watch(streamer_id, cx);
        }
        match call.session.watch_stream(&streamer_id) {
            Ok(watch) => {
                watching.watch = Some(watch);
                watching.rejoins += 1;
            }
            Err(_) => fail_watch(watching, cx),
        }
        cx.notify();
    }

    // A stream connected, or a camera turned on, with no picture READY_TIMEOUT later: Discord's 2012
    // (`key` is "stream:{id}" or "camera:{id}"). The first frame clears it.
    fn check_ready(&mut self, key: String, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(voice::issues::READY_TIMEOUT).await;
            this.update(cx, |this, cx| {
                let Some(call) = &mut this.call else { return };
                let pictured = match key.split_once(':') {
                    Some(("stream", id)) => call.watched(id).is_none_or(|w| !w.arrivals.is_empty() || w.state != WatchState::Active),
                    Some((_, id)) => call.cameras.contains_key(id),
                    None => true,
                };
                if !pictured {
                    call.issues.insert((key, voice::issues::RECEIVER_READY_TIMEOUT));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    // The streamer's voice state says they stopped while we still watch: closed after LIVE_LAPSE
    // unless they're live again, as Discord does.
    fn on_live_lapse(&mut self, streamer_id: String, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(LIVE_LAPSE).await;
            this.update(cx, |this, cx| {
                let live = this.participants().iter().any(|s| s.user_id == streamer_id && s.streaming);
                let ended = this.call.as_ref().is_some_and(|c| c.ended.contains(&streamer_id));
                if !live && !ended && this.call.as_ref().is_some_and(|c| c.watched(&streamer_id).is_some()) {
                    this.unwatch_stream(&streamer_id, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn end_watch(&mut self, streamer_id: String, cx: &mut Context<Self>) {
        let focused = self.call_focus.as_deref() == Some(format!("stream:{streamer_id}").as_str());
        let Some(call) = &mut self.call else { return };
        if let Some(frame) = call.unwatch(&streamer_id).and_then(|w| w.frame) {
            cx.drop_image(frame, None);
        }
        sounds::play(sounds::Cue::StreamEnded);
        // In focus, its card stays, saying who stopped, until closed; elsewhere it just goes, as Discord's.
        if focused && !call.ended.contains(&streamer_id) {
            call.ended.push(streamer_id);
        }
        cx.notify();
    }

    fn unwatch_stream(&mut self, streamer_id: &str, cx: &mut Context<Self>) {
        if let Some(frame) = self.call.as_mut().and_then(|c| c.unwatch(streamer_id)).and_then(|w| w.frame) {
            cx.drop_image(frame, None);
        }
        cx.notify();
    }

    // Whether `key` is our own Go Live or the one we're watching: the stream whose viewers Discord announces.
    fn in_stream(&self, key: &str) -> bool {
        let Some(call) = &self.call else { return false };
        let mine = call.sharing.is_some() && self.me.as_ref().is_some_and(|m| key.ends_with(&format!(":{}", m.id)));
        // Discord announces viewers only for the single stream you're in.
        let watched = matches!(call.watching.as_slice(), [only] if key.ends_with(&format!(":{}", only.streamer_id)));
        mine || watched
    }

    fn stop_watching(&mut self, cx: &mut Context<Self>) {
        let Some(call) = &mut self.call else { return };
        call.focus = None;
        if self.call_focus.as_deref().is_some_and(|key| key.starts_with("stream:")) {
            self.call_focus = None;
        }
        for frame in call.watching.drain(..).filter_map(|w| w.frame) {
            cx.drop_image(frame, None);
        }
        cx.notify();
    }

    /// Stops sharing, or opens Discord's picker of what to share: the whole screen, an app or a window.
    fn toggle_screen_share(&mut self, cx: &mut Context<Self>) {
        let Some(call) = &self.call else { return };
        if call.sharing.is_some() {
            return self.end_share(cx);
        }
        self.open_share_picker(cx);
    }

    // Stops our Go Live, or clears it away once it ended on its own.
    fn end_share(&mut self, cx: &mut Context<Self>) {
        let Some(call) = &mut self.call else { return };
        call.sharing = None;
        call.share_source = None;
        call.live_since = None;
        if let Some((frame, _)) = call.self_view.take() {
            cx.drop_image(frame, None);
        }
        sounds::play(sounds::Cue::StreamEnded);
        if let Some(me) = self.me.as_ref().map(|m| m.id.clone()) {
            self.unwatch_stream(&me, cx);
        }
        cx.notify();
    }

    fn start_share(&mut self, source: voice::ShareSource, cx: &mut Context<Self>) {
        self.close_share_picker(cx);
        let label = self.share_choice_label.take();
        if self.go_live(source, cx) {
            if let Some(call) = &mut self.call {
                call.share_label = label;
                call.live_since = Some(Instant::now());
            }
            sounds::play(sounds::Cue::StreamStarted);
        }
        cx.notify();
    }

    // Starts (or restarts) our Go Live of `source` with the current quality prefs.
    fn go_live(&mut self, source: voice::ShareSource, cx: &mut Context<Self>) -> bool {
        let Some(call) = &mut self.call else { return false };
        let options = voice::GoLiveOptions { resolution: self.prefs.stream_resolution, fps: self.prefs.stream_fps, audio: self.prefs.stream_audio };
        match call.session.go_live_with_options(source, options, call.sharing.take()) {
            Ok(live) => {
                call.sharing = Some(live);
                call.share_source = Some(source);
                self.upload_stream_previews(source, cx);
                true
            }
            Err(error) => {
                self.voice_error = Some(error.into());
                false
            }
        }
    }

    // Quality and sound changes take effect by restarting the same stream; viewers rejoin it.
    pub(crate) fn restart_share(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut() else { return };
        let Some(source) = call.share_source.filter(|_| call.sharing.is_some()) else { return };
        self.go_live(source, cx);
        cx.notify();
    }

    // Mute and deafen apply to the call as well as the buttons; deafening also mutes, as in Discord.
    fn sync_voice_flags(&self) {
        let Some(call) = &self.call else { return };
        call.session.set_flags(self.muted || self.deafened, self.deafened);
    }

    // `key` names what was copied (the code itself, a file's URL), so its button shows Copied for a moment.
    fn copy_code(&mut self, code: String, key: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(code));
        self.copied = Some(key.clone());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(1500)).await;
            this.update(cx, |this, cx| {
                if this.copied.as_ref() == Some(&key) {
                    this.copied = None;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// The rail in Discord's order: folders and loose servers as the user arranged them, then any others.
    fn rail_entries(&self) -> Vec<RailEntry> {
        let mut placed: HashSet<&str> = HashSet::new();
        let mut entries = Vec::new();
        for folder in &self.folders {
            let guilds: Vec<usize> = folder.guild_ids.iter().filter_map(|id| self.guilds.iter().position(|g| &g.id == id)).collect();
            placed.extend(folder.guild_ids.iter().map(String::as_str));
            match (folder.id.is_empty() || folder.id == "0", guilds.len()) {
                (_, 0) => {}
                (true, _) => entries.extend(guilds.into_iter().map(RailEntry::Guild)),
                (false, _) => entries.push(RailEntry::Folder(folder.clone(), guilds)),
            }
        }
        entries.extend(self.guilds.iter().enumerate().filter(|(_, g)| !placed.contains(g.id.as_str())).map(|(i, _)| RailEntry::Guild(i)));
        entries
    }

    fn set_hovered(&mut self, key: &str, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            self.hovered = Some(key.to_owned());
        } else if self.hovered.as_deref() == Some(key) {
            self.hovered = None;
        }
        cx.notify();
    }
}

// Servers by their index in `Waffle::guilds`: the rail is built every frame, and a cloned `Guild`
// carries all its channels.
enum RailEntry {
    Guild(usize),
    Folder(Folder, Vec<usize>),
}

// gpui's render images are BGRA, which is what the decoder produces, so the bytes go in as they are.
// Takes the pixels without a copy when nothing else holds the frame (a decoded remote picture).
fn render_image(frame: Arc<voice::VideoFrame>) -> Option<Arc<RenderImage>> {
    let (width, height) = (frame.width, frame.height);
    let bgra = Arc::try_unwrap(frame).map_or_else(|shared| shared.bgra.clone(), |frame| frame.bgra);
    let buffer = image::RgbaImage::from_raw(width, height, bgra)?;
    Some(Arc::new(RenderImage::new([image::Frame::new(buffer)])))
}

/// Applies a reaction change. `from_gateway` echoes of our own toggles are already counted.
fn adjust_reaction(message: &mut Message, emoji: &Emoji, added: bool, mine: bool, from_gateway: bool) {
    if from_gateway && mine && message.reactions.iter().any(|r| &r.emoji == emoji && r.me == added) {
        return;
    }
    if from_gateway && mine && !added && !message.reactions.iter().any(|r| &r.emoji == emoji) {
        return;
    }
    match message.reactions.iter().position(|r| &r.emoji == emoji) {
        Some(ix) => {
            let r = &mut message.reactions[ix];
            r.count = if added { r.count + 1 } else { r.count.saturating_sub(1) };
            if mine {
                r.me = added;
            }
            if r.count == 0 {
                message.reactions.remove(ix);
            }
        }
        None if added => message.reactions.push(discord::Reaction { emoji: emoji.clone(), count: 1, me: mine }),
        None => {}
    }
}

fn apply_list_op(list: &mut Vec<ListItem>, op: ListOp) {
    match op {
        ListOp::Sync { start, items } => {
            if list.len() < start {
                list.resize(start, ListItem::Group { id: String::new(), count: 0 });
            }
            let end = (start + items.len()).min(list.len());
            list.splice(start..end, items);
        }
        ListOp::Insert { index, item } if index <= list.len() => list.insert(index, item),
        ListOp::Update { index, item } if index < list.len() => list[index] = item,
        ListOp::Delete { index } if index < list.len() => {
            list.remove(index);
        }
        _ => {}
    }
}

// Pending ids sort after every real snowflake.
fn order(id: &str) -> u64 {
    id.parse().unwrap_or(u64::MAX)
}

// A channel as the composer and drop zone name it: @someone in a DM, #channel elsewhere.
fn channel_mention(channel: &Channel) -> String {
    match channel.kind {
        ChannelKind::Dm => format!("@{}", channel.name),
        _ => format!("#{}", channel.name),
    }
}

// A message's files on their way, kept until Discord takes it so Resend can run it again.
#[derive(Clone)]
struct UploadJob {
    channel_id: String,
    files: Vec<discord::Upload>,
    content: String,
    reply_to: Option<String>,
    ping: bool,
    // Discord's refusal, verbatim, once it fails.
    error: Option<String>,
}

pub(crate) fn nonce() -> String {
    let millis = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    // A snowflake-shaped number, as Discord's own clients send.
    ((millis.saturating_sub(1_420_070_400_000) << 22) | u64::from(std::process::id() % 4096)).to_string()
}

// One entry per person in a server's (or the DMs') voice list, since Discord has each user in one
// channel at a time. Updated in place while they stay in the channel, so the list keeps join order and
// the call stage never reshuffles on a mute or a Go Live.
fn upsert_voice_state(states: &mut Vec<discord::VoiceState>, state: discord::VoiceState) {
    if let Some(slot) = states.iter_mut().find(|v| v.user_id == state.user_id && state.channel_id.is_some() && v.channel_id == state.channel_id) {
        *slot = state;
        return;
    }
    states.retain(|v| v.user_id != state.user_id);
    if state.channel_id.is_some() {
        states.push(state);
    }
}

// Swaps the `@`/`#` query that ends the draft for a mention chip, then a space, leaving the caret after both.
pub(crate) fn complete_mention(state: &mut ComposerState, query: &str, chip: InlineToken, window: &mut Window, cx: &mut Context<ComposerState>) {
    let end = state.value().len();
    if !state.value().ends_with(query) {
        return;
    }
    state.set_selected_range(end - query.len()..end, cx);
    if state.replace_with_token(chip, window, cx).is_ok() {
        state.insert(" ", window, cx);
    }
}

// Replaces the `query` that ends the draft with `text` and a space.
pub(crate) fn complete_text(state: &mut ComposerState, query: &str, text: &str, window: &mut Window, cx: &mut Context<ComposerState>) {
    let end = state.value().len();
    if !state.value().ends_with(query) {
        return;
    }
    state.set_selected_range(end - query.len()..end, cx);
    // `replace` swaps out the selection; `insert` would add at the caret and leave the typed code behind.
    state.replace(format!("{text} "), window, cx);
}

// The draft as Discord takes it: each mention chip becomes its `<@id>` / `<#id>`, kept as the token's id.
// A slash command's end chip is the space it shows.
fn wire_text(content: &InputContent) -> String {
    let mut text = content.text().to_string();
    for span in content.tokens().iter().rev() {
        let token = span.token();
        let wire = if command_picker::is_end_token(token) { token.text() } else { token.id() };
        text.replace_range(span.range(), wire);
    }
    text
}

// One entry per person, in the order given, with you moved to the end.
fn mention_order(users: impl Iterator<Item = User>, me: Option<&str>) -> Vec<User> {
    let mut seen = std::collections::HashSet::new();
    let (mine, others): (Vec<User>, Vec<User>) = users.filter(|u| seen.insert(u.id.clone())).partition(|u| Some(u.id.as_str()) == me);
    others.into_iter().chain(mine).collect()
}

#[cfg(test)]
mod tests {
    fn voice(user: &str, channel: Option<&str>, guild: Option<&str>) -> discord::VoiceState {
        discord::VoiceState { user_id: user.into(), channel_id: channel.map(Into::into), guild_id: guild.map(Into::into), ..Default::default() }
    }

    #[test]
    fn a_person_is_listed_once_in_voice() {
        // Guild payloads list voice states without a guild id; live updates carry one.
        let mut states = vec![voice("me", Some("a"), None), voice("pal", Some("a"), None)];
        upsert_voice_state(&mut states, voice("me", Some("a"), Some("g")));
        assert_eq!(states.iter().filter(|v| v.user_id == "me").count(), 1);
        assert_eq!(states[0].user_id, "me", "an update in the same channel keeps its place");
        upsert_voice_state(&mut states, voice("me", Some("b"), Some("g")));
        assert_eq!(states.iter().filter(|v| v.user_id == "me").count(), 1);
        assert_eq!(states.last().unwrap().channel_id.as_deref(), Some("b"), "a move rejoins at the end");
        upsert_voice_state(&mut states, voice("me", None, Some("g")));
        assert!(states.iter().all(|v| v.user_id != "me"), "leaving drops them");
    }

    use super::*;

    struct Draft(Entity<ComposerState>);

    impl gpui_kit::Render for Draft {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui_kit::IntoElement {
            gpui_kit::component::input::Textarea::new(&self.0)
        }
    }

    #[gpui_kit::test]
    fn a_picked_mention_becomes_a_chip_with_the_caret_after_its_space(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::component::init);
        let (draft, cx) = cx.add_window_view(|window, cx| Draft(cx.new(|cx| ComposerState::new(window, cx))));
        let state = cx.read(|cx| draft.read(cx).0.clone());
        cx.update(|window, cx| {
            state.update(cx, |state, cx| {
                state.set_value("hi @fe", window, cx);
                complete_mention(state, "@fe", InlineToken::new("<@42>", "@Ferris"), window, cx);
            })
        });
        let (content, caret) = cx.read(|cx| (state.read(cx).content(), state.read(cx).cursor()));
        assert_eq!(content.text().as_ref(), "hi @Ferris ");
        assert_eq!(caret, "hi @Ferris ".len(), "caret after the chip and its space");
        assert_eq!(content.tokens().len(), 1);
        assert_eq!(content.tokens()[0].range(), 3..10);
        assert_eq!(wire_text(&content), "hi <@42> ");
    }

    #[test]
    fn mentions_list_you_last_once() {
        let user = |id: &str| User { id: id.into(), name: id.into(), ..Default::default() };
        let order = mention_order([user("me"), user("pal"), user("me"), user("friend")].into_iter(), Some("me"));
        assert_eq!(order.iter().map(|u| u.id.as_str()).collect::<Vec<_>>(), ["pal", "friend", "me"]);
    }

    fn member(id: &str) -> ListItem {
        ListItem::Member(discord::Member { user: User { id: id.into(), ..Default::default() }, roles: vec![], presence: Presence::default(), boosting_since: None })
    }

    fn ids(list: &[ListItem]) -> Vec<String> {
        list.iter()
            .map(|i| match i {
                ListItem::Member(m) => m.user.id.clone(),
                ListItem::Group { id, .. } => format!("g:{id}"),
            })
            .collect()
    }

    #[test]
    fn member_list_ops_keep_order() {
        let mut list = Vec::new();
        apply_list_op(&mut list, ListOp::Sync { start: 0, items: vec![ListItem::Group { id: "online".into(), count: 2 }, member("a"), member("b")] });
        apply_list_op(&mut list, ListOp::Insert { index: 2, item: member("c") });
        apply_list_op(&mut list, ListOp::Delete { index: 1 });
        apply_list_op(&mut list, ListOp::Update { index: 2, item: member("d") });
        assert_eq!(ids(&list), ["g:online", "c", "d"]);
        apply_list_op(&mut list, ListOp::Sync { start: 0, items: vec![member("x")] });
        assert_eq!(ids(&list), ["x", "c", "d"]);
    }

    #[test]
    fn reactions_count_once_per_toggle() {
        let mut m = Message::default();
        let thumbs = Emoji::unicode("👍");
        adjust_reaction(&mut m, &thumbs, true, true, false);
        // The gateway echo of our own optimistic add.
        adjust_reaction(&mut m, &thumbs, true, true, true);
        assert_eq!(m.reactions[0].count, 1);
        adjust_reaction(&mut m, &thumbs, true, false, true);
        assert_eq!(m.reactions[0].count, 2);
        adjust_reaction(&mut m, &thumbs, false, true, false);
        assert_eq!((m.reactions[0].count, m.reactions[0].me), (1, false));
        adjust_reaction(&mut m, &thumbs, false, true, true);
        assert_eq!(m.reactions[0].count, 1);
    }

    #[test]
    fn pending_sorts_after_real_ids() {
        assert!(order("pending-1") > order("1300000000000000000"));
    }

    #[test]
    fn nonce_looks_like_a_snowflake() {
        assert!(nonce().parse::<u64>().unwrap() > 1 << 40);
    }
}

// The cue for someone's voice state changing, seen from our call. Others arriving or going sound as
// a join or a leave, from or to another channel too. Our own come from the call: a join when it
// connects, or a move when it switched channels in the same server (see `connected_cue`).
fn member_cue(mine: bool, was_here: bool, is_here: bool) -> Option<sounds::Cue> {
    match (mine, was_here, is_here) {
        (true, ..) => None,
        (false, false, true) => Some(sounds::Cue::UserJoin),
        (false, true, false) => Some(sounds::Cue::UserLeave),
        _ => None,
    }
}

fn connected_cue(switched: bool) -> sounds::Cue {
    if switched { sounds::Cue::UserMoved } else { sounds::Cue::UserJoin }
}

// Discord's cue when the viewer count of the stream we're in changes (it stays quiet past 25).
fn viewer_cue(before: usize, after: usize) -> Option<sounds::Cue> {
    match () {
        _ if before > 25 => None,
        _ if after > before => Some(sounds::Cue::StreamUserJoined),
        _ if after < before => Some(sounds::Cue::StreamUserLeft),
        _ => None,
    }
}

#[cfg(test)]
mod viewer_tests {
    use super::*;

    #[test]
    fn viewer_cues() {
        assert_eq!(viewer_cue(0, 1), Some(sounds::Cue::StreamUserJoined));
        assert_eq!(viewer_cue(2, 1), Some(sounds::Cue::StreamUserLeft));
        assert_eq!(viewer_cue(1, 1), None);
        assert_eq!(viewer_cue(30, 31), None);
    }

    // A channel hop is one move for us, and a join or leave for whoever we're in the call with.
    #[test]
    fn voice_state_cues() {
        assert_eq!(member_cue(false, false, true), Some(sounds::Cue::UserJoin), "someone hops in from another channel");
        assert_eq!(member_cue(false, true, false), Some(sounds::Cue::UserLeave), "someone hops out to another channel");
        assert_eq!(member_cue(false, true, true), None);
        assert_eq!(member_cue(true, true, false), None, "our own hop sounds once, from the call");
        assert_eq!(connected_cue(true), sounds::Cue::UserMoved);
        assert_eq!(connected_cue(false), sounds::Cue::UserJoin);
    }
}

// A finished call's video frames, handed back: each is a texture in gpui's atlas until dropped, so a
// call that ended without this kept every last stream, camera and self-view frame on the GPU.
fn release_frames(mut call: Call, cx: &mut gpui_kit::App) {
    let frames = call.watching.drain(..).filter_map(|w| w.frame).chain(call.cameras.drain().map(|(_, frame)| frame)).chain(call.self_view.take().map(|(frame, _)| frame));
    for frame in frames {
        cx.drop_image(frame, None);
    }
}

fn fail_watch(watching: &mut Watching, cx: &mut Context<Waffle>) {
    watching.state = WatchState::Failed(STREAM_FAILED_TO_START);
    if let Some(frame) = watching.frame.take() {
        cx.drop_image(frame, None);
    }
    cx.notify();
}

// STREAM_DELETE's own reasons, as opposed to a transport error that dropped the connection.
fn is_delete_reason(reason: &str) -> bool {
    [voice::STREAM_DELETE_ENDED, voice::STREAM_DELETE_RATE_LIMITED, "user_requested"].contains(&reason)
}
