// Worst-case fixture data for checking each screen without a Discord account (debug builds only):
// WAFFLE_DEMO=chat | chat-compact | chat-large-text | busy | message-focus | message-focus-menu | message-focus-delete | message-menu-theirs | user-menu-voice | user-menu-voice-denied | mention | mention-tab | mention-enter | reactors | reactions-dialog | text-files | text-expanded | text-viewer | link-previews | activity | activity-all | rail-dms | rail-dms-live | gallery | viewer | viewer-video | pins | header-search | search | search-from | search-date | search-results | search-filters | search-dm | search-dm-results | profile | profile-plain | profile-popout | profile-popout-worst | profile-full | profile-full-plain | profile-full-friends | profile-full-servers | profile-full-empty | profile-full-note | profile-full-worst | profile-dm | emoji | switcher | settings | voice-settings | keybinds | keyboard-combos | appearance | appearance-scrolled | settings-accessibility | notifications | content-social | chat-settings | settings-contrast | settings-search | status | status-custom | status-durations | status-editor | stickers | stickers-chat | accounts | unread | emoji-suggest | emoji-accept | notify-bell | jump | inbox | inbox-unreads | notification-settings | notification-settings-muted | inbox-mark-read | mark-unread | home | add-friend | create-channel | create-channel-private | create-channel-community | create-channel-stage | create-server | create-server-template | create-server-customize | create-server-join | create-server-limit | invite | invite-sent | invite-settings | invite-vanity | invite-paused | advanced | zoom | home-presence | folder | stream-card | call-streams | call-streams-ended | call-streams-focus | call-streams-strip | call-streams-full | voice-elsewhere | call-incoming | call-banner | call-joinable | call-dm | call-dm-camera | sidebar-resize | sidebar-voice | sidebar-voice-error | sidebar-voice-solid | sidebar-voice-fullscreen | dm-empty | group-empty | group-start | login | scanned | failed | empty | update-ready | updates | updates-downloading | updates-ready | channel-settings | channel-settings-search | channel-settings-perms | channel-settings-voice | channel-settings-voice-perms | channel-settings-invites | channel-settings-dirty | load | server-settings | server-settings-engagement | server-settings-roles | server-settings-role | server-settings-role-perms | server-settings-audit | server-settings-bans | server-settings-automod | server-settings-invites | server-settings-dirty | attachments | attachments-drop | forwarded | forward-dialog | poll | poll-creator | app-replies | components | components-users | components-v2 | command-picker | command-picker-search | command-options | command-options-loading | command-options-failed | command-options-empty | command-options-tab | command-options-errors | app-modal | app-modal-error | drafts | friends-empty | friends-all-empty | friends-pending-empty | friends-blocked-empty | pins-empty | search-empty | inbox-empty | inbox-unreads-empty | forum-empty | load-failed | invite | invite-plain | invite-invalid | onboarding | onboarding-question | onboarding-dropdown | onboarding-rules | onboarding-required | onboarding-plain | rules | rules-dialog | rules-error | rules-long | roles | roles-browse | roles-list | guide | guide-done | guide-channel | guide-next | call-streams-failed | call-streams-reconnecting | call-streams-issues | call-streams-hidden | call-activity | inbox-saved | games | stage | stage-audience | stage-requests | stage-invited | stage-waiting | stage-start | stage-start-dialog | stage-edit | stage-end | stage-full | requests | requests-spam | requests-empty | request-open | active-now | active-now-empty | group-dm | group-picker | group-edit | group-leave | profile-stranger | profile-editor | profile-editor-server | activity-privacy | embed-suppress | system-messages | system-messages-open | chat-media-settings | voice-messages | sidebar-voice-chat | call-previews | call-previews-unread | call-chat | soundboard | soundboard-effect | voice-status | voice-status-set | voice-status-dialog | voice-status-dialog-long | updating (the launch splash alone). | send-options | tts | blocked-messages | blocked-messages-shown | guard-slowmode | guard-slowmode-counting | guard-slowmode-queued | guard-immune | guard-no-permission | guard-timeout | guard-blocked-dm
use gpui_kit::{
    Context, Window,
    component::input::{InlineToken, InputContent},
};

use crate::{
    Waffle, Feed, Status,
    drafts::Draft,
    discord::{
        Activity, ActivityKind, Channel, ChannelKind, Connection, Embed, Emoji, Event, Folder, Guild, GuildTag, ListItem, ListOp, Media, MediaKind, Member, Message, Presence, Reaction, Reactor, ReadState, Ready, Relation, Relationship, Role,
        Status as Presence_, User, VoiceState, avatar_url,
    },
    login::Login,
};

mod guard;
use guard::demo_guard;

mod chat_feed;
mod fixtures;
mod scenes_apps;
mod scenes_calls;
mod scenes_chat;
mod scenes_files;
mod scenes_media;
mod scenes_panels;
mod scenes_people;
mod scenes_activity;
mod scenes_profile;

use chat_feed::*;
use fixtures::*;
use scenes_apps::*;
use scenes_calls::*;
use scenes_chat::*;
use scenes_files::*;
use scenes_media::*;
use scenes_panels::*;
use scenes_people::*;
use scenes_activity::*;
use scenes_profile::*;

// The launch splash mid-download, without the main window.
pub fn updating_splash(cx: &mut gpui_kit::App) {
    crate::splash::open("Installing 0.5.0…", cx);
}

pub fn load(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    // Fixture drafts stay in memory, away from the real drafts file.
    waffle.drafts = crate::drafts::Drafts::default();
    waffle.command_picker = Default::default();
    let me = me_user();
    match scene {
        "login" => return waffle.status = Status::SignedOut(Login::qr("https://discord.com/ra/demo-fingerprint-0123456789abcdef")),
        "scanned" => return waffle.status = Status::SignedOut(Login::Scanned(Box::new(me))),
        "failed" => return waffle.status = Status::SignedOut(Login::Failed("Invalid ticket".into())),
        // Sign-in offering a newer build: downloading and downloaded after a failed sign-in, and ready on the code screen.
        "failed-update" | "failed-update-ready" | "login-update-ready" => {
            use crate::updater::{Staged, Status as Update};
            waffle.updates.status = match scene {
                "failed-update" => Update::Downloading { version: "0.17.0".into(), done: 13 << 20, total: Some(31 << 20) },
                _ => Update::Ready(Staged::demo("0.17.0")),
            };
            let login = match scene {
                "login-update-ready" => Login::qr("https://discord.com/ra/demo-fingerprint-0123456789abcdef"),
                _ => Login::Failed("Invalid remote authentication ticket".into()),
            };
            return waffle.status = Status::SignedOut(login);
        }
        _ => {}
    }
    let load = scene == "load";
    waffle.on_event(Event::Ready(Box::new(ready(me))), cx);
    if load {
        load_guilds(waffle);
    }
    let (groups, items) = if load { load_members() } else { (vec![("r1".into(), 2), ("online".into(), 3), ("offline".into(), 1)], members()) };
    waffle.on_event(Event::MemberList { guild_id: "1".into(), groups, ops: vec![ListOp::Sync { start: 0, items }] }, cx);
    waffle.guild_id = Some("1".into());
    if scene == "empty" {
        return;
    }
    let messages = match scene {
        _ if load => load_messages(),
        "embeds" => embed_messages(),
        "blocked-messages" | "blocked-messages-shown" => blocked_messages(),
        "chat-compact" | "chat-large-text" => display_messages(),
        "system-messages" | "system-messages-open" => system_messages(),
        "embed-suppress" => embed_suppress_messages(),
        _ => messages(),
    };
    waffle.feeds.insert("general".into(), Feed { messages: std::sync::Arc::new(messages), loaded: true, ..Default::default() });
    let general = waffle.guild().and_then(|g| g.channels.iter().find(|c| c.id == "general").cloned());
    if let Some(channel) = general {
        waffle.open_channel(channel, window, cx);
    }
    waffle.on_event(Event::Typing { channel_id: "general".into(), user_id: "8388608".into(), name: Some("Wren".into()) }, cx);
    chat_scenes(waffle, scene, window, cx);
    panel_scenes(waffle, scene, window, cx);
    call_scenes(waffle, scene, window, cx);
}

// Where a fixture menu or scroll offset sits, in window pixels.
fn demo_at(x: f32, y: f32) -> gpui_kit::Point<gpui_kit::Pixels> {
    // lint-allow(raw-px): fixture coordinates for where a demo menu opens, not layout
    gpui_kit::point(gpui_kit::px(x), gpui_kit::px(y))
}
