// Profile popout, DM side panel and full profile scenes, with worst-case data beside the full one.
use super::*;
use crate::profile::FullTab;

pub(super) const PROFILE_SCENES: &[&str] = &[
    "profile-popout",
    "profile-popout-worst",
    "profile-full",
    "profile-full-plain",
    "profile-full-friends",
    "profile-full-servers",
    "profile-full-empty",
    "profile-full-note",
    "profile-full-worst",
    "profile-dm",
    "profile-roles",
    "profile-reply",
    "profile-tag",
    "profile-note",
    "profile-status-long",
    "profile-gif",
    "profile-gif-still",
    "profile-frame",
    "profile-frame-full",
    "profile-frame-dm",
    "profile-effect",
    "profile-effect-full",
    "profile-effect-dm",
];

pub(super) fn demo_profile_scene(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    if scene.starts_with("profile-effect") {
        waffle.seed_profile_effect(EFFECT_SKU, effect_fixture());
    }
    if scene == "profile-dm" || scene == "profile-frame-dm" || scene == "profile-effect-dm" {
        demo_profile_dm(waffle, window, cx);
        let key = crate::profile::profile_key("8388608", None);
        if let Some(mut profile) = waffle.profiles.peek(&key) {
            match scene {
                "profile-frame-dm" => profile.frame = Some(frame_fixture()),
                "profile-effect-dm" => profile.effect_sku = Some(EFFECT_SKU.into()),
                _ => {}
            }
            waffle.profiles.seed(&key, profile);
        }
        return;
    }
    if scene == "profile-full-empty" {
        return demo_full_empty(waffle, cx);
    }
    demo_profile(waffle, cx);
    waffle.presences.entry("8388608".into()).or_default().custom = Some("🍯 drizzling honey on everything".into());
    let worst = scene.ends_with("-worst");
    if let Some(card) = waffle.profile_card.as_mut() {
        card.full = match scene {
            "profile-full" | "profile-full-plain" | "profile-full-note" | "profile-full-worst" | "profile-frame-full" | "profile-effect-full" => Some(FullTab::Activity),
            "profile-full-friends" => Some(FullTab::MutualFriends),
            "profile-full-servers" => Some(FullTab::MutualServers),
            _ => None,
        };
        if let Some(profile) = card.profile.as_mut() {
            profile.mutual_guilds = vec![crate::discord::MutualGuild { id: "1".into(), nick: Some("wrenny".into()) }];
            profile.note = (scene == "profile-full-note").then(|| "Met in the gpui server. Owes me a waffle recipe.".into());
            if scene.starts_with("profile-frame") {
                profile.frame = Some(frame_fixture());
            }
            if scene.starts_with("profile-effect") {
                profile.effect_sku = Some(EFFECT_SKU.into());
            }
            if scene.starts_with("profile-gif") {
                profile.banner = Some("demo/banner.gif".into());
            }
            if scene == "profile-full-plain" {
                (profile.theme, profile.banner) = (None, None);
            }
            if worst {
                worst_case(profile);
            }
        }
        if worst {
            card.user.name = "Wren Alexandria Lark-Featherstonehaugh the Third of Honeymoor".into();
        }
    }
    if let Some(card) = waffle.profile_card.as_mut() {
        card.recent = Some(recent_fixture());
    }
    let at = demo_at(700., 180.);
    match scene {
        "profile-roles" => waffle.open_role_picker(demo_at(560., 600.), window, cx),
        "profile-reply" => waffle.open_reply(&user("8388608", "Wren"), crate::profile::Source::Status("🍯 drizzling honey on everything".into()), at, window, cx),
        "profile-tag" => waffle.open_tag_card(demo_at(640., 330.), cx),
        "profile-note" => waffle.open_note_popover(String::new(), demo_at(600., 290.), window, cx),
        "profile-status-long" => waffle.presences.entry("8388608".into()).or_default().custom = Some("currently translating the entire gpui codebase into a sea shanty, verse 47 of 312".into()),
        _ => {}
    }
    if scene == "profile-gif-still" {
        waffle.account_settings.gif_auto_play = false;
    }
    if worst {
        waffle.presences.entry("8388608".into()).or_default().custom = Some("currently translating the entire gpui codebase into a sea shanty, verse 47 of 312, send snacks".into());
        if let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") {
            guild.roles.extend((0..14).map(|r| crate::discord::Role { id: format!("wr{r}"), name: format!("very long role name number {r}"), color: 0x3BA55C + r * 0x0B0B0B, position: 10 + r as i64, permissions: 0, managed: false, icon: None }));
        }
    }
}

// A honey ribbon stapled over the top edge, in front, and rails down both sides behind the card.
fn frame_fixture() -> crate::discord::ProfileFrame {
    use crate::discord::{FrameAnchor, FrameKind, FrameLayer, ProfileFrame};
    ProfileFrame {
        inner_width: 1200.,
        overflow_top: 200.,
        overflow_bottom: 0.,
        overflow_horizontal: 48.,
        layers: vec![
            FrameLayer { src: "demo/frame-rail.png".into(), kind: FrameKind::Rail, front: false, anchor: FrameAnchor::Center },
            FrameLayer { src: "demo/frame-top.png".into(), kind: FrameKind::Staple, front: true, anchor: FrameAnchor::Top },
        ],
    }
}

const EFFECT_SKU: &str = "demo-effect";

// Honey sparkles over the whole card with a glow and a dot in each corner, to show the card's rounding clips them.
fn effect_fixture() -> Vec<crate::discord::EffectLayer> {
    vec![crate::discord::EffectLayer { src: "demo/effect.png".into(), x: 0., y: 0., width: 450., height: 880., z: 0, start: 0., duration: 0., looping: false, loop_delay: 0. }]
}

// A game played for the first time and long, a show, and a listening session, as Recent activity shows them.
fn recent_fixture() -> Vec<crate::discord::RecentEntry> {
    use crate::discord::{RecentEntry, RecentKind};
    // Snowflakes minted a few hours and days ago, for "3h ago" and "2d ago".
    let minted = |ago_ms: i64| (((chrono::Utc::now().timestamp_millis() - ago_ms - 1_420_070_400_000) as u64) << 22).to_string();
    vec![
        RecentEntry { id: minted(3 * 3_600_000), kind: RecentKind::Played, title: "Celeste".into(), app_id: None, first_time: true, marathon_hours: Some(3), ..Default::default() },
        RecentEntry { id: minted(2 * 86_400_000), kind: RecentKind::Watched, title: "Frieren: Beyond Journey's End".into(), subtitle: Some("Season 1, Episode 3".into()), ..Default::default() },
        RecentEntry { id: minted(5 * 86_400_000), kind: RecentKind::Listened, title: "Aqua Regia".into(), subtitle: Some("Sleep Token".into()), streak_days: Some(4), ..Default::default() },
    ]
}

// Many badges, roles and mutuals, no bio.
fn worst_case(profile: &mut crate::discord::Profile) {
    profile.bio = None;
    profile.username = "wren.alexandria.lark.featherstonehaugh".into();
    profile.pronouns = Some("she/they/whatever you like really".into());
    profile.badges = (0..9).map(|i| crate::discord::Badge { description: format!("Badge {i}"), icon: "brand/icons/peek.png".into(), link: None }).collect();
    profile.roles.extend((0..14).map(|r| format!("wr{r}")));
    profile.mutual_friend_count = 128;
    profile.mutual_friends = (0..40).map(|i| user(&format!("{}", 30_000_000 + i), &format!("friend with a rather long display name {i}"))).collect();
}

// Someone you share nothing with: no banner, theme, bio or activity, and each tab's empty line.
fn demo_full_empty(waffle: &mut Waffle, cx: &mut Context<Waffle>) {
    waffle.guild_id = None;
    waffle.open_full_profile(user("20971520", "ferris"), FullTab::Activity, cx);
    if let Some(card) = waffle.profile_card.as_mut() {
        card.profile = Some(crate::discord::Profile { username: "ferris.rs".into(), ..Default::default() });
    }
}

// Wren's DM with her profile in the side panel.
fn demo_profile_dm(waffle: &mut Waffle, window: &mut Window, cx: &mut Context<Waffle>) {
    demo_profile(waffle, cx);
    let profile = waffle.profile_card.take().and_then(|c| c.profile);
    let wren = user("8388608", "Wren");
    waffle.presences.entry(wren.id.clone()).or_default().custom = Some("🍯 drizzling honey on everything".into());
    if let Some(mut profile) = profile {
        profile.mutual_guilds = vec![crate::discord::MutualGuild { id: "1".into(), nick: None }];
        waffle.profiles.seed(&crate::profile::profile_key(&wren.id, None), profile);
    }
    let mut dm = channel("dm-wren", "Wren", ChannelKind::Dm, None);
    (dm.raw_type, dm.recipient, dm.recipients) = (1, Some(wren.clone()), vec![wren]);
    waffle.dms.insert(0, dm.clone());
    waffle.feeds.insert(dm.id.clone(), Feed { messages: std::sync::Arc::new(Vec::new()), loaded: true, ..Default::default() });
    waffle.restore = None;
    waffle.guild_id = None;
    waffle.right_panel = crate::RightPanel::Members;
    waffle.open_channel(dm, window, cx);
}
