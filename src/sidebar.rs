// The server rail and the channel list (design/css/navigation.css: .rail, .srv, .folder, .side, .ch).
use std::time::Duration;

use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, Context, Div, FontWeight, Hsla, Radians, SharedString, Stateful, TooltipPlacement, Window,
    base::{Transition as Tween, transition},
    component::{ActiveTheme, Icon},
    div, img, point, prelude::*, px, relative, svg,
};
use gpui_motion_kit::core::{Bezier, Easing, Motion, Target, Transition};

use crate::{
    Waffle, Edge, Part, RailEntry, Status,
    discord::{ActivityKind, Channel, ChannelKind, Folder, Guild, GuildSettings, OnStage, Status as Status_, channel_flag},
    drag::{RowDrag, hole},
    icons::Glyph,
    layout::RAIL_WIDTH,
    motion, palette,
    rail_drag::RailDrag,
    sortable::{Board, Place},
    stage::STAGE,
    ui::{self, Squircle},
};

mod channels;
mod dm_row;
mod preview;
mod rail;
mod tiles;
mod voice;
use preview::ServerPreview;
// What the rest of the app reaches through `crate::sidebar`, and what the parts below share.
pub(crate) use channels::*;
pub(crate) use tiles::*;
use voice::*;

// Rail geometry: 44px tiles 8px apart (at Default density), centred in the 68px rail, so the state pill
// sits on its edge.
pub(crate) const TILE: f32 = 44.;
// How far a header control sits below the window's top: a 34px control centred in the 48px header row.
const HEADER_INSET: f32 = 7.;
pub(crate) const TILE_X: f32 = (RAIL_WIDTH - TILE) / 2.;

// Spacing that follows UI density (`ui::density_pad`).
fn rail_gap() -> f32 {
    ui::density_pad(8.)
}

// A channel row: its 16px of text plus padding, 32px at Default density.
pub(crate) fn row_height() -> f32 {
    16. + ui::density_pad(16.)
}

pub(crate) fn row_gap() -> f32 {
    ui::density_pad(2.)
}
// A live stage's row: its name with the topic under it.
const STAGE_ROW_HEIGHT: f32 = 44.;
// A person's row under a voice channel, and the slot that waits for one being moved there.
const VOICE_ROW: f32 = 32.;
// Where a voice person's profile opens, above the click.
const PROFILE_LIFT: f32 = 40.;
// The gap before a category or the DM heading, at Default density.
const SECTION_GAP: f32 = 16.;
// The channel header's left inset, before the traffic lights' overhang.
const HEADER_PAD: f32 = 8.;
// A category's fold chevron.
pub(crate) const CARET_SIZE: f32 = 11.;
// The server name button that heads the channel list.
const HEADER_BUTTON: f32 = 34.;
// A folder's glyph when it is open, and the LIVE mark beside a voice person.
const FOLDER_GLYPH: f32 = 22.;
const LIVE_MARK_HEIGHT: f32 = 18.;
// An audience row's indent (a person's row with its avatar slot).
const AUDIENCE_INDENT: f32 = 37.;
// A thread row's indent, and the 1.5px line of its branch or a drop slot's outline.
const THREAD_INDENT: f32 = 34.;
const THICK_LINE: f32 = 1.5;
// A speaking ring sits this far clear of an avatar, and the avatar's own size in a voice row.
const RING_CLEARANCE: f32 = 3.;
const VOICE_AVATAR: f32 = 24.;
// The speaking ring around it, `RING_CLEARANCE` out on every side.
const VOICE_RING: f32 = 30.;


// Hover and state changes: --dur-hover on --ease-out.
pub(crate) fn hover_ease() -> Transition {
    Transition::Tween(ui::DUR_HOVER, Easing::Bezier(Bezier::new(0.23, 1., 0.32, 1.).expect("monotonic bezier")))
}

// ICONS

// MOVE(icons): glyphs the navigation chrome needs that `icons::Glyph` lacks yet (window controls and
// voice marks included); promote each into `Glyph` and drop this.
#[derive(Clone, Copy)]
pub(crate) enum Nav {
    Megaphone,
    Media,
    Stage,
    BellSlash,
    Compass,
    Plus,
    Caret,
    Close,
    MicOffFill,
    DeafenedFill,
    CameraFill,
    GameFill,
    MusicFill,
    TvFill,
    Mic,
    Headphones,
    HeadphonesOff,
    MicOff,
    Settings,
    Minimize,
    Maximize,
    Restore,
}

impl Nav {
    pub(crate) fn icon(self) -> Icon {
        Icon::default().data(self.svg())
    }

    fn svg(self) -> &'static [u8] {
        match self {
            Nav::Megaphone => include_bytes!("../assets/icons/reicon/bullhorn.svg"),
            Nav::Media => include_bytes!("../assets/icons/reicon/gallery.svg"),
            Nav::Stage => include_bytes!("../assets/icons/reicon/podcast.svg"),
            Nav::BellSlash => include_bytes!("../assets/icons/reicon/bell-off2.svg"),
            Nav::Compass => include_bytes!("../assets/icons/reicon/compass.svg"),
            Nav::Plus => include_bytes!("../assets/icons/reicon/add.svg"),
            Nav::Caret => include_bytes!("../assets/icons/reicon/chevron-down.svg"),
            Nav::Close => include_bytes!("../assets/icons/reicon/xmark.svg"),
            Nav::MicOffFill => include_bytes!("../assets/icons/reicon/muted.svg"),
            Nav::DeafenedFill => include_bytes!("../assets/icons/reicon/deafened.svg"),
            Nav::CameraFill => include_bytes!("../assets/icons/reicon/video.svg"),
            Nav::GameFill => include_bytes!("../assets/icons/reicon/gamepad.svg"),
            Nav::MusicFill => include_bytes!("../assets/icons/reicon/music-notes.svg"),
            Nav::TvFill => include_bytes!("../assets/icons/reicon/tv.svg"),
            Nav::Mic => include_bytes!("../assets/icons/reicon/microphone2.svg"),
            Nav::Headphones => include_bytes!("../assets/icons/reicon/headphone.svg"),
            Nav::HeadphonesOff => include_bytes!("../assets/icons/reicon/deafened.svg"),
            Nav::MicOff => include_bytes!("../assets/icons/reicon/muted.svg"),
            Nav::Settings => include_bytes!("../assets/icons/reicon/gear2.svg"),
            Nav::Minimize => include_bytes!("../assets/icons/reicon/minus.svg"),
            Nav::Maximize => include_bytes!("../assets/icons/reicon/win-max.svg"),
            Nav::Restore => include_bytes!("../assets/icons/reicon/win-restore.svg"),
        }
    }
}

// An activity's kind as its filled glyph: what someone is playing, hearing, streaming or watching.
pub(crate) fn activity_glyph(kind: ActivityKind) -> Option<Icon> {
    match kind {
        ActivityKind::Playing | ActivityKind::Competing => Some(Nav::GameFill.icon()),
        ActivityKind::Listening => Some(Nav::MusicFill.icon()),
        // Discord draws a TV for both.
        ActivityKind::Streaming | ActivityKind::Watching => Some(Nav::TvFill.icon()),
        ActivityKind::Custom => None,
    }
}

// A row still loading, names of varying length: a DM's avatar and name, or a channel's icon and name.
fn list_skeleton(i: usize, channels: bool, _cx: &App) -> AnyElement {
    let width = [0.62, 0.45, 0.7, 0.52, 0.38, 0.58, 0.48, 0.66][i % 8];
    let mark = match channels {
        true => ui::Skeleton::new(SharedString::from(format!("list-skel-mark-{i}"))).radius(4.).size(ui::ICON_LG).flex_none(),
        false => ui::Skeleton::circle(SharedString::from(format!("list-skel-mark-{i}")), 32.),
    };
    ui::row()
        .h(px(if channels { row_height() } else { 42. }))
        .px_2()
        .gap(px(if channels { 8. } else { 12. }))
        .child(mark)
        .child(ui::Skeleton::new(SharedString::from(format!("list-skel-name-{i}"))).radius(4.).h_3().w(relative(width)))
        .into_any_element()
}

fn muted_line(text: &'static str, cx: &App) -> AnyElement {
    div().px_2().py_2().text_color(cx.theme().muted_foreground).child(text).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discord::{ChannelSettings, guild_flag};

    fn channel(id: &str, kind: u8, position: i64, parent: Option<&str>) -> Channel {
        crate::discord::parse_channel(&serde_json::json!({ "id": id, "type": kind, "position": position, "parent_id": parent }), "g").unwrap()
    }

    // Owned by "me", so every channel is viewable.
    fn guild(channels: Vec<Channel>) -> Guild {
        Guild {
            id: "g".into(),
            name: "g".into(),
            icon: None,
            channels,
            roles: Vec::new(),
            voice_states: Vec::new(),
            emojis: Vec::new(),
            stickers: Vec::new(),
            owner_id: Some("me".into()),
            default_notifications: 0,
            threads: Vec::new(),
            my_roles: Vec::new(),
            premium_tier: 0,
            afk_channel_id: None,
            features: vec!["COMMUNITY".into()],
            ..Default::default()
        }
    }

    fn settings(guild_flags: u64, channels: &[(&str, u64)]) -> GuildSettings {
        let channels = channels.iter().map(|&(id, flags)| (id.to_owned(), ChannelSettings { flags, ..Default::default() })).collect();
        GuildSettings { flags: guild_flags, channels, ..Default::default() }
    }

    fn ids(sections: &[(Section, Vec<Channel>)]) -> Vec<(String, Vec<&str>)> {
        sections
            .iter()
            .map(|(section, channels)| {
                let name = match section {
                    Section::Pinned => "pinned".to_owned(),
                    Section::Uncategorised => "loose".to_owned(),
                    Section::Category(c) => c.id.clone(),
                    Section::ActiveNow => "active".to_owned(),
                };
                (name, channels.iter().map(|c| c.id.as_str()).collect())
            })
            .collect()
    }

    #[test]
    fn pinned_channels_leave_their_category_and_opt_in_hides_the_rest() {
        let guild = guild(vec![
            channel("loose", 0, 0, None),
            channel("cat", 4, 1, None),
            channel("lounge", 2, 2, Some("cat")),
            channel("general", 0, 3, Some("cat")),
            channel("memes", 0, 4, Some("cat")),
            channel("art", 0, 5, Some("cat")),
            channel("info", 4, 6, None),
            channel("rules", 0, 7, Some("info")),
        ]);
        let pins = settings(0, &[("lounge", channel_flag::FAVORITED), ("general", channel_flag::FAVORITED)]);
        let all = channel_sections(&guild, "me", Some(&pins), |_| false, |_| false);
        assert_eq!(ids(&all), [("pinned".into(), vec!["general", "lounge"]), ("loose".into(), vec!["loose"]), ("cat".into(), vec!["memes", "art"]), ("info".into(), vec!["rules"])]);

        let opt_in = settings(guild_flag::OPT_IN_CHANNELS_ON, &[("lounge", channel_flag::FAVORITED), ("memes", channel_flag::OPT_IN_ENABLED), ("info", channel_flag::OPT_IN_ENABLED)]);
        let listed = channel_sections(&guild, "me", Some(&opt_in), |c| c.id == "art", |_| false);
        assert_eq!(ids(&listed), [("pinned".into(), vec!["lounge"]), ("cat".into(), vec!["memes", "art"]), ("info".into(), vec!["rules"])]);
    }

    fn thread(id: &str, parent: &str, joined: bool, archived: bool) -> Channel {
        let mut json = serde_json::json!({ "id": id, "type": 11, "parent_id": parent, "thread_metadata": { "archived": archived } });
        if joined {
            json["member"] = serde_json::json!({});
        }
        crate::discord::parse_channel(&json, "g").unwrap()
    }

    #[test]
    fn opt_in_keeps_a_channel_whose_joined_active_thread_is_noticed() {
        let mut guild = guild(vec![channel("cat", 4, 0, None), channel("general", 0, 1, Some("cat")), channel("art", 0, 2, Some("cat")), channel("memes", 0, 3, Some("cat")), channel("clips", 0, 4, Some("cat")), channel("quiet", 0, 5, Some("cat"))]);
        guild.threads = vec![thread("art-t", "art", true, false), thread("memes-t", "memes", false, false), thread("clips-t", "clips", true, true), thread("quiet-t", "quiet", true, false)];
        let opt_in = settings(guild_flag::OPT_IN_CHANNELS_ON, &[("general", channel_flag::OPT_IN_ENABLED)]);
        let noticed = |t: &Channel| t.id != "quiet-t";
        let listed = channel_sections(&guild, "me", Some(&opt_in), |_| false, noticed);
        assert_eq!(ids(&listed), [("cat".into(), vec!["general", "art"])], "an unjoined, archived or read thread doesn't hold its channel");
    }

    #[test]
    fn opt_in_gathers_occupied_unadded_voice_into_active_now_last() {
        let mut guild = guild(vec![
            channel("hangout", 2, 0, None),
            channel("b", 4, 2, None),
            channel("b-voice", 2, 0, Some("b")),
            channel("a", 4, 1, None),
            channel("a-text", 0, 0, Some("a")),
            channel("a-voice", 2, 3, Some("a")),
            channel("a-lounge", 2, 1, Some("a")),
            channel("a-empty", 2, 2, Some("a")),
            channel("a-added", 2, 4, Some("a")),
            channel("a-pinned", 2, 5, Some("a")),
        ]);
        let state = |user: &str, channel: &str| crate::discord::VoiceState { user_id: user.into(), channel_id: Some(channel.into()), ..Default::default() };
        guild.voice_states = ["hangout", "b-voice", "a-voice", "a-lounge", "a-lounge", "a-added", "a-pinned"].iter().enumerate().map(|(i, c)| state(&i.to_string(), c)).collect();
        let opt_in = settings(guild_flag::OPT_IN_CHANNELS_ON, &[("a-text", channel_flag::OPT_IN_ENABLED), ("a-added", channel_flag::OPT_IN_ENABLED), ("a-pinned", channel_flag::FAVORITED)]);
        let listed = channel_sections(&guild, "me", Some(&opt_in), |_| false, |_| false);
        assert_eq!(
            ids(&listed),
            [("pinned".into(), vec!["a-pinned"]), ("a".into(), vec!["a-text", "a-added"]), ("active".into(), vec!["hangout", "a-lounge", "a-voice", "b-voice"])],
            "by category position, then channel position; added, pinned and empty channels stay out"
        );

        guild.voice_states.retain(|v| v.channel_id.as_deref() != Some("a-voice"));
        let left = channel_sections(&guild, "me", Some(&opt_in), |_| false, |_| false);
        assert_eq!(left.last().map(|(_, c)| c.iter().map(|c| c.id.as_str()).collect::<Vec<_>>()), Some(vec!["hangout", "a-lounge", "b-voice"]), "empties leave as people do");

        let all = channel_sections(&guild, "me", Some(&settings(0, &[])), |_| false, |_| false);
        assert!(!all.iter().any(|(s, _)| matches!(s, Section::ActiveNow)), "no Active Now with opt-in off");
    }

    #[test]
    fn pinned_dms_come_first_and_keep_their_recency_order() {
        let dms: Vec<Channel> = ["a", "b", "c", "d"].iter().map(|id| channel(id, 1, 0, None)).collect();
        let order = pinned_first(&dms, |id| id == "c" || id == "b");
        assert_eq!(order.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["b", "c", "a", "d"]);
    }

    #[test]
    fn folder_minis_fill_the_tile_for_how_many_there_are() {
        let centre = (TILE - MINI) / 2.;
        assert_eq!(mini_slot(0, 1), (centre, centre), "one sits in the middle");
        assert_eq!([mini_slot(0, 2).1, mini_slot(1, 2).1], [centre, centre], "two share the middle row");
        assert_eq!(mini_slot(2, 3).0, centre, "the third of three centres under the pair");
        assert_eq!(mini_slot(3, 4), (MINI_INSET + MINI + MINI_GAP, MINI_INSET + MINI + MINI_GAP), "four fill the grid");
    }

    #[test]
    fn server_badge_follows_discords_media_state() {
        let state = |user: &str, channel: &str, streaming, camera| crate::discord::VoiceState { user_id: user.into(), channel_id: Some(channel.into()), streaming, camera, ..Default::default() };
        let all = |_: &crate::discord::VoiceState| true;
        let kind = |m: Option<Media>| m.map(|m| (m.kind, m.connected));
        assert_eq!(kind(media_state(&[], "me", None, false, all)), None, "an empty server has no badge");
        assert_eq!(kind(media_state(&[state("a", "c", false, false)], "me", None, false, all)), Some((MediaKind::Voice, false)), "people in voice: speaker");
        let busy = [state("a", "c", false, true), state("b", "c", true, false), state("d", "c", false, false)];
        assert_eq!(kind(media_state(&busy, "me", None, false, all)), Some((MediaKind::Screen, false)), "a stream outranks cameras and voice");
        assert_eq!(kind(media_state(&busy[..1], "me", None, false, all)), Some((MediaKind::Video, false)), "a camera outranks voice");
        assert_eq!(kind(media_state(&busy, "me", None, true, all)), None, "a muted server stays quiet");
        assert_eq!(kind(media_state(&busy, "me", None, false, |s| s.user_id == "d")), Some((MediaKind::Voice, false)), "uncounted people (blocked, AFK, hidden) don't light it");
        let mine = [state("me", "c", false, false), state("b", "other", true, false)];
        assert_eq!(kind(media_state(&mine, "me", Some("c"), true, all)), Some((MediaKind::Voice, true)), "connected: green speaker, even muted, and others' streams don't count");
        let streaming = [state("me", "c", true, false)];
        assert_eq!(kind(media_state(&streaming, "me", Some("c"), false, all)), Some((MediaKind::Screen, true)), "your own stream");
    }

    #[test]
    fn mute_and_deafen_marks_fill_their_own_slots() {
        let state = |muted, deafened, server_muted, server_deafened| crate::discord::VoiceState { muted, deafened, server_muted, server_deafened, ..Default::default() };
        assert_eq!(voice_marks(&state(true, true, false, false)), [VoiceMark::Muted, VoiceMark::Deafened]);
        assert_eq!(voice_marks(&state(false, true, false, false)), [VoiceMark::Muted, VoiceMark::Deafened]);
        assert_eq!(voice_marks(&state(true, false, false, false)), [VoiceMark::Muted]);
        // A server mute and deafen arrive as mute and deaf, which also read as muted and deafened here.
        assert_eq!(voice_marks(&state(true, true, true, true)), [VoiceMark::ServerMuted, VoiceMark::ServerDeafened], "both server marks show");
        assert_eq!(voice_marks(&state(false, true, false, true)), [VoiceMark::ServerDeafened], "a server deafen alone isn't a self-mute");
        assert_eq!(voice_marks(&state(true, true, true, false)), [VoiceMark::ServerMuted, VoiceMark::Deafened], "server mute beside a self-deafen");
        assert_eq!(voice_marks(&state(true, false, true, false)), [VoiceMark::ServerMuted]);
        assert!(voice_marks(&state(false, false, false, false)).is_empty());
        let live = crate::discord::VoiceState { muted: true, deafened: true, camera: true, streaming: true, ..Default::default() };
        assert_eq!(voice_marks(&live), [VoiceMark::Muted, VoiceMark::Deafened, VoiceMark::Camera, VoiceMark::Live], "live keeps the mute and deafen marks");
    }

    #[test]
    fn voice_row_joins_first_then_opens_the_stage() {
        assert_eq!(voice_row_click(None, "lounge"), VoiceRowClick::Join);
        assert_eq!(voice_row_click(Some("gaming"), "lounge"), VoiceRowClick::Join);
        assert_eq!(voice_row_click(Some("lounge"), "lounge"), VoiceRowClick::OpenStage);
    }

    #[test]
    fn viewer_line_names_two_then_counts() {
        let names = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(viewer_line(&[]), None);
        assert_eq!(viewer_line(&names(&["Theo"])).as_deref(), Some("Theo watching"));
        assert_eq!(viewer_line(&names(&["Theo", "Ivy"])).as_deref(), Some("Theo and Ivy watching"));
        assert_eq!(viewer_line(&names(&["Theo", "Ivy", "Sam"])).as_deref(), Some("Theo, Ivy and Sam watching"));
        assert_eq!(viewer_line(&names(&["Theo", "Ivy", "Sam", "Ada"])).as_deref(), Some("Theo, Ivy and 2 others watching"));
    }

    #[test]
    fn live_for_drops_zero_units() {
        assert_eq!(live_for(Duration::from_secs(59)), "Just started");
        assert_eq!(live_for(Duration::from_secs(24 * 60 + 59)), "24 min");
        assert_eq!(live_for(Duration::from_secs(2 * 3600)), "2 hr");
        assert_eq!(live_for(Duration::from_secs(3600 + 5 * 60)), "1 hr 5 min");
    }

    #[test]
    fn stream_meta_joins_what_it_has() {
        assert_eq!(stream_meta(Some("Blender"), Some(Duration::from_secs(24 * 60))).as_deref(), Some("Blender · 24 min"));
        assert_eq!(stream_meta(None, Some(Duration::from_secs(24 * 60))).as_deref(), Some("24 min"));
        assert_eq!(stream_meta(Some("Blender"), None).as_deref(), Some("Blender"));
        assert_eq!(stream_meta(None, None), None);
    }
}

#[cfg(test)]
mod rail_dm_tests {
    #[test]
    fn leaving_tiles_hold_their_place() {
        let same = |a: &&str, b: &&str| a == b;
        assert_eq!(super::keep_leaving(vec!["a", "c"], vec!["a", "b", "c"], same), ["a", "b", "c"]);
        assert_eq!(super::keep_leaving(vec!["n", "a"], vec!["a", "b"], same), ["n", "a", "b"]);
        assert_eq!(super::keep_leaving(vec![], vec!["a", "b"], same), ["a", "b"]);
    }
}
