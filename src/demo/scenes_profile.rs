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
];

pub(super) fn demo_profile_scene(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    if scene == "profile-dm" {
        return demo_profile_dm(waffle, window, cx);
    }
    if scene == "profile-full-empty" {
        return demo_full_empty(waffle, cx);
    }
    demo_profile(waffle, cx);
    waffle.presences.entry("8388608".into()).or_default().custom = Some("🍯 drizzling honey on everything".into());
    let worst = scene.ends_with("-worst");
    if let Some(card) = waffle.profile_card.as_mut() {
        card.full = match scene {
            "profile-full" | "profile-full-plain" | "profile-full-note" | "profile-full-worst" => Some(FullTab::Activity),
            "profile-full-friends" => Some(FullTab::MutualFriends),
            "profile-full-servers" => Some(FullTab::MutualServers),
            _ => None,
        };
        if let Some(profile) = card.profile.as_mut() {
            profile.mutual_guilds = vec![crate::discord::MutualGuild { id: "1".into(), nick: Some("wrenny".into()) }];
            profile.note = (scene == "profile-full-note").then(|| "Met in the gpui server. Owes me a waffle recipe.".into());
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
    if worst {
        waffle.presences.entry("8388608".into()).or_default().custom = Some("currently translating the entire gpui codebase into a sea shanty, verse 47 of 312, send snacks".into());
        if let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") {
            guild.roles.extend((0..14).map(|r| crate::discord::Role { id: format!("wr{r}"), name: format!("very long role name number {r}"), color: 0x3BA55C + r * 0x0B0B0B, position: 10 + r as i64, permissions: 0, managed: false, icon: None }));
        }
    }
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
