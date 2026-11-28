// Release builds are a GUI app on Windows, so no console window opens beside it; debug builds keep it for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accounts;
mod startup;
mod boot;
mod composer;
mod gateway;
mod lookups;
mod navigation;
mod sending;
mod voice_calls;
mod app_commands;
mod app_modal;
mod command_line;
mod command_picker;
mod recent_commands;
mod components;
mod interactions;
mod active_now;
mod activities;
mod activity_launcher;
mod toast;
mod activity_frame;
mod attachments;
mod send_options;
mod app_keys;
mod crash;
mod create_channel;
mod create_server;
mod modify_attachment;
mod restart;
mod feed_keys;
mod auth;
mod cache;
mod downloads;
mod launch;
mod snapshot;
mod store;
mod tiered;
mod captcha;
mod query;
mod call;
mod emphasis;
mod call_issues;
mod call_previews;
mod channel_menu;
mod channel_settings;
mod dm_call;
mod dm_start;
mod announcements;
mod chat;
mod chat_guard;
mod composer_emoji;
#[cfg(debug_assertions)]
mod demo;
mod discord;
mod drafts;
mod frecency_sync;
mod drag;
mod favorites;
mod emoticons;
mod icons;
mod illustration;
mod images;
mod layout;
mod user_panel;
mod link_preview;
mod login;
mod mascot;
mod material;
mod forum;
mod forward;
mod events;
mod report;
mod group_dm;
mod reactions;
mod requests;
mod gif;
mod games;
mod go_live;
mod h264;
mod hidden;
mod home;
mod os_image;
mod notification_settings;
mod overlays;
mod overwrites;
mod audio;
mod playback;
mod audio_player;
mod poll;
mod inbox;
mod invite;
mod joining;
mod status_menu;
mod stickers;
mod memory;
mod name_fonts;
mod menu;
mod message_menu;
mod mod_view;
mod motion;
mod palette;
mod pip;
mod presence;
mod profile;
mod notify;
mod panel;
mod part;
mod perf;
mod completion;
use completion::{complete_mention, complete_text, wire_text};
mod rail_drag;
mod role_popout;
mod sidebar;
mod sortable;
mod search;
mod search_menu;
mod search_results;
mod server_settings;
mod settings;
mod shortcuts;
mod autoscroll;
mod frame_rate;
mod shell;
mod user_menu;
mod video;
mod viewer;
mod hotkeys;
mod key_hook;
mod sounds;
mod soundboard;
mod soundboard_fx;
mod system_message;
#[cfg(target_os = "windows")]
mod tray;
mod text_file;
mod ui;
mod setup;
mod splash;
mod stage;
mod stage_dialogs;
mod stage_view;
mod updater;
#[cfg(debug_assertions)]
mod design_system;
mod app_icon;
mod fonts;
mod voice;
mod voice_status;
mod window_controls;

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use discord::{Channel, ChannelKind, Client, Emoji, Event, Folder, Gif, Guild, ListItem, ListOp, Message, Presence, ReadState, Relationship, SendState, User};
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
    // Run as the setup .exe, or with --uninstall, Waffle installs or removes itself instead.
    if setup::run_instead() {
        return;
    }
    #[cfg(target_os = "windows")]
    if !tray::claim_single_instance() {
        return;
    }
    crash::install();
    #[cfg(feature = "dhat-heap")]
    memory::start_heap_profile();
    memory::start_reporting();
    ui::feel::install();
    gpui_kit::application().with_assets(Assets).run(move |cx| {
        gpui_kit::init(cx);
        app_keys::init(cx);
        shortcuts::init(cx);
        cx.text_system().add_fonts(fonts::bundled()).expect("bundled fonts");
        name_fonts::start(cx);
        // The design is drawn with `-webkit-font-smoothing: antialiased`; macOS smoothing would thicken every stroke.
        cx.text_system().set_font_smoothing(false);
        cx.set_http_client(std::sync::Arc::new(discord::Images::default()));
        cx.set_reduce_motion(settings::Prefs::load().reduces_motion());
        settings::Prefs::load().app_icon.apply();
        // WAVE_DESIGN=1 opens only the design system gallery, no sign-in needed.
        #[cfg(debug_assertions)]
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
            startup::install_then_open(staged, signed_in, cx);
            return;
        }
        // Signed in: a small splash window shows while the main window loads hidden behind it.
        let demo = cfg!(debug_assertions) && std::env::var_os("WAFFLE_DEMO").is_some();
        let splash = if signed_in && !demo { splash::open("Signing in…", cx) } else { None };
        startup::open_main(splash, cx);
    });
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
    ModView(Box<mod_view::ModView>),
    Hidden,
}

#[derive(Clone)]
pub(crate) struct Typer {
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
    // A built-in command (/tts): its glyph.
    Command(icons::Glyph),
}

struct Autocomplete {
    token: String,
    items: Vec<Suggestion>,
    selected: usize,
}

// Where Waffle keeps its files (store.rs).
use store::data_dir;

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
    // lint-allow(unbounded-growth): one entry per person who spoke in a call, a few dozen at most; replaced on each speak
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
    // Reconnects in a row without a frame in between, which stretch the wait before the next.
    rejoins: u8,
}

// Discord's active-stream states (its web client, 2026-11-21), less ENDED, which is `Call::ended`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum WatchState {
    // Since when: Discord fails a watch with no STREAM_CREATE in 5 s (12 s on a Stage); None once it came.
    Connecting(Option<Instant>),
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
// A dropped stream is rejoined after this long, more each time in a row.
const REJOIN_AFTER: Duration = Duration::from_secs(2);
// The streamer's voice state stops saying they're live while we still watch: Discord closes the
// stream this long after, unless they're live again.
const LIVE_LAPSE: Duration = Duration::from_secs(180);

impl Watching {
    pub(crate) fn new(streamer_id: String, frame: Option<Arc<RenderImage>>, watch: Option<voice::StreamWatch>) -> Self {
        let state = if watch.is_some() { WatchState::Connecting(Some(Instant::now())) } else { WatchState::Active };
        Self { streamer_id, frame, arrivals: Default::default(), watch, state, rejoins: 0 }
    }

    fn ended(&self) -> bool {
        self.watch.as_ref().is_some_and(voice::StreamWatch::ended)
    }
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
    VoiceStatus,
    // A reaction to someone's status, avatar or activity from their profile.
    ProfileReaction,
}

#[derive(Clone, Copy, PartialEq)]
enum Edge {
    Channels,
    Panel,
    // The nav column of the settings views (app, server, channel), one width for all three.
    Settings,
    // The voice channel's chat beside the call, growing leftward.
    CallChat,
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
    launch: launch::Launch,
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
    // Folded runs of joins opened to their lines, by the run's first message.
    open_join_runs: HashSet<String>,
    forward_dialog: Option<forward::ForwardDialog>,
    // Our interactions in flight, by the nonce we sent.
    interactions: HashMap<String, interactions::PendingInteraction>,
    command_picker: command_picker::CommandPicker,
    component_state: components::ComponentState,
    app_modal: Option<app_modal::AppModal>,
    announcements: announcements::Announcements,
    events: events::Events,
    report_dialog: Option<report::ReportDialog>,
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
    favorite_board: sortable::Board,
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
    // lint-allow(unbounded-growth): one entry per server visited, bounded by the servers the user is in
    last_channel: HashMap<Option<String>, String>,
    feeds: HashMap<String, Feed>,
    list: ListState,
    // Where each channel left scrolled up was read from (a message id and how far into it), so coming
    // back opens there; a channel left at the bottom has none and opens on the newest message.
    scroll_anchors: HashMap<String, (String, gpui_kit::Pixels)>,
    read_states: HashMap<String, ReadState>,
    // lint-allow(unbounded-growth): one u64 per channel the gateway reported, bounded by the user's channel count
    last_message: HashMap<String, u64>,
    presences: HashMap<String, Presence>,
    // lint-allow(unbounded-growth): one entry per server whose member list was opened, bounded by the user's server count
    member_roles: HashMap<String, HashMap<String, Vec<String>>>,
    // lint-allow(unbounded-growth): one member list per server opened, bounded by the user's server count
    member_lists: HashMap<String, Vec<ListItem>>,
    // lint-allow(unbounded-growth): one group-count map per server opened, bounded by the user's server count
    member_groups: HashMap<String, HashMap<String, u32>>,
    // lint-allow(unbounded-growth): one pair per (server, channel) member range asked for, bounded by the user's channel count
    requested_members: HashSet<(String, String)>,
    // lint-allow(unbounded-growth): one entry per channel with someone typing; each typer expires after TYPING_TTL, bounded by the user channel count
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
    // The voice panel's open popover: the connection's details, or noise suppression.
    voice_popover: Option<call::VoicePopover>,
    muted_cue: user_panel::MutedCue,
    device_menu: Option<call::DeviceMenu>,
    menu: Option<menu::OpenMenu>,
    // Go Live thumbnails by streamer: the CDN url and when it was fetched.
    // lint-allow(unbounded-growth): one thumbnail url per streamer seen, a handful of live streams at a time
    stream_previews: HashMap<String, (String, Instant, u32)>,
    // Forum channels' archived posts and post previews, by forum id.
    // lint-allow(unbounded-growth): one forum state per forum channel opened, bounded by the user's forum channels
    forums: HashMap<String, forum::ForumState>,
    developer_mode: bool,
    account_settings: crate::discord::AccountSettings,
    my_account: crate::discord::MyAccount,
    // Notification settings per server, "@me" for DMs.
    guild_settings: HashMap<String, discord::GuildSettings>,
    ignored: HashSet<String>,
    // Folds of hidden messages opened with Show, by the run's first message id.
    shown_folds: HashSet<String>,
    // The open feed's folds as last drawn; when they change, the list re-measures their rows.
    fold_layout: std::cell::RefCell<hidden::FoldLayout>,
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
    // The ⌘/ Keyboard combos dialog (shortcuts.rs).
    keyboard_combos: bool,
    create_channel: Option<create_channel::CreateChannel>,
    create_server: Option<create_server::CreateServer>,
    invite: Option<invite::Invite>,
    notification_settings: Option<notification_settings::NotificationSettings>,
    stage_topic: Option<stage_dialogs::StageTopic>,
    modify_attachment: Option<modify_attachment::ModifyAttachment>,
    dialog_input: Entity<SearchState>,
    // Modify Attachment's Description (Alt Text).
    alt_input: Entity<TextareaState>,
    // A short notice at the bottom, e.g. why a moderation action failed.
    toast: Option<SharedString>,
    // A toast that carries a way back (Rejoin): its line under the title and its one action.
    toast_action: Option<toast::ToastAction>,
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
    // lint-allow(unbounded-growth): one entry per live stream we have heard of, replaced on every STREAM_UPDATE
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
    stage_ui: crate::call::StageUi,
    // Our stream's self-view: shown, and which corner of the stage it sits in.
    // Our local volume for each streamer's Go Live (0..=2), kept across watches.
    // lint-allow(unbounded-growth): one f32 per streamer the user adjusted, a few dozen at most
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
    // New messages in the call's voice channel, floating over the stage while its chat is closed.
    call_previews: call_previews::CallPreviews,
    // When the call channel's unread count last changed, for the chat button's highlight.
    call_unread_at: Option<Instant>,
    // Voice channel → when its current session began (Unix seconds), from CHANNEL_INFO.
    voice_start: HashMap<String, u64>,
    // Voice channel → its status as Discord sends it (custom emoji as `<:name:id>`); absent when it has none.
    voice_status: HashMap<String, String>,
    voice_status_dialog: Option<voice_status::VoiceStatusDialog>,
    // Servers whose channel info we've asked for since connecting.
    channel_info_requested: HashSet<String>,
    // Each call participant's avatar colour for their tile; None while loading or when it failed.
    // lint-allow(unbounded-growth): one colour per person seen in a call, a few dozen at most
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
    // lint-allow(unbounded-growth): one entry per mention or emoji picked in the composer this session, a few hundred at most
    mention_wire: HashMap<String, String>,
    autocomplete: Option<Autocomplete>,
    // Discord's "Hold Up!" over the composer: Enter on a mass ping waits for Send Now.
    hold_up: Option<send_options::HoldUp>,
    // The suggestion list's scroll, kept on the highlighted row.
    autocomplete_scroll: gpui_kit::ScrollHandle,
    channels_scroll: gpui_kit::ScrollHandle,
    rail_scroll: gpui_kit::ScrollHandle,
    panel_scroll: gpui_kit::ScrollHandle,
    flash: Option<String>,
    muted: bool,
    deafened: bool,
    call: Option<Call>,
    activities: activities::Activities,
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
    profile_card: Option<profile::ProfileCard>,
    role_popout: role_popout::RolePopouts,
    status_menu: bool,
    // Debug builds only: the design system shown in place of the whole app.
    #[cfg(debug_assertions)]
    design_system: Option<Entity<design_system::DesignSystem>>,
    // The channel or DM row under the pointer, which shows its hover-only controls.
    hovered_row: Option<String>,
    // The message under the pointer, which shows its toolbar and, on a continuation, its time.
    hovered_message: Option<String>,
    // The attachment tile under the pointer (a waiting file, a sent picture), which shows its actions.
    hovered_tile: Option<String>,
    // The sent picture whose ALT popout is open, as `{message id}#{index}`.
    alt_popout: Option<String>,
    // The message with keyboard focus in the feed (feed_keys.rs); kept only while `feed_focus` holds focus.
    focused_message: Option<String>,
    feed_focus: FocusHandle,
    // The call card shown big (`call::tile_key`), or None for the even grid.
    call_focus: Option<String>,
    // The call stage's measured card area, which the grid is fitted to.
    // Profiles by user and server, shown at once from the last fetch while a fresh one loads.
    profiles: query::Query<discord::Profile>,
    // Profile effect layers by sku; None while fetching or after it failed, which draws no effect.
    profile_effects: profile::ProfileEffects,
    // Previews of Discord's own links (messages, invites), shared with the rows that draw them.
    link_previews: link_preview::Previews,
    // Who reacted, by message and emoji (`Emoji::path`), fetched on a pill's first hover; dropped when it changes.
    reactors: HashMap<(String, String), reactions::Reactors>,
    reactions_dialog: Option<reactions::ReactionsDialog>,
    games: games::Games,
    emoji_picker: Option<EmojiTarget>,
    emoji_search: Entity<SearchState>,
    soundboard: soundboard::Soundboard,
    switcher: Option<usize>,
    switcher_input: Entity<SearchState>,
    // The read position when the open channel was opened: messages after it sit under the NEW line.
    unread_marker: Option<u64>,
    // Scrolled well above the newest message: shows Discord's "Jump to Present" bar.
    scrolled_up: bool,
    // The floating composer's height, measured as it paints; the feed keeps that much room at its foot.
    composer_height: Rc<std::cell::Cell<f32>>,
    guard: chat_guard::GuardClock,
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
    history: home::History<home::Stop>,
    folded: HashSet<String>,
    favorites: favorites::Favorites,
    // Set while the channel list shows Favorites: the channel opened from it, "" before one is.
    favorites_open: Option<String>,
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
    options: discord::SendOptions,
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

// One entry per person, in the order given, with you moved to the end.
fn mention_order(users: impl Iterator<Item = User>, me: Option<&str>) -> Vec<User> {
    let mut seen = std::collections::HashSet::new();
    let (mine, others): (Vec<User>, Vec<User>) = users.filter(|u| seen.insert(u.id.clone())).partition(|u| Some(u.id.as_str()) == me);
    others.into_iter().chain(mine).collect()
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
