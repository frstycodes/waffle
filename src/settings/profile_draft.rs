// The Profiles page's drafts as plain data: what Discord has, what you've changed (named as the save bar
// names it), and the requests Save sends for exactly those changes, in Discord's order.
use std::path::PathBuf;

use serde_json::{Map, Value, json};

use crate::discord::CDN;

pub(crate) const DISPLAY_NAME_MAX: usize = 32;
pub(crate) const NICK_MAX: usize = 32;
pub(crate) const PRONOUNS_MAX: usize = 40;
// Discord trials 300 behind an experiment (2026-08-user-bio-max-length); 190 until the server takes more.
pub(crate) const BIO_MAX: usize = 190;
pub(crate) const RECENT_AVATARS: usize = 6;

// The main profile, as saved or as drafted.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct MainProfile {
    pub display_name: String,
    pub pronouns: String,
    pub bio: String,
    pub accent: Option<u32>,
    // The server whose tag you wear.
    pub tag_guild: Option<String>,
    pub avatar: AvatarChoice,
}

// What Save does to your avatar. What Discord has is always `Current`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum AvatarChoice {
    #[default]
    Current,
    // A recent avatar, put back by its id with no new upload.
    Recent(String),
    Upload { path: PathBuf, data: String },
    Removed,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ServerProfile {
    pub nick: String,
    pub pronouns: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RecentAvatar {
    pub id: String,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TagGuild {
    pub guild_id: String,
    pub name: String,
    pub tag: String,
    pub badge: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Field {
    DisplayName,
    Pronouns,
    Avatar,
    BannerColour,
    AboutMe,
    ServerTag,
    ServerNickname,
    ServerPronouns,
}

impl Field {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Field::DisplayName => "Display name",
            Field::Pronouns | Field::ServerPronouns => "Pronouns",
            Field::Avatar => "Avatar",
            Field::BannerColour => "Banner colour",
            Field::AboutMe => "About me",
            Field::ServerTag => "Server tag",
            Field::ServerNickname => "Server nickname",
        }
    }
}

// One call Save makes. Discord's order: the user, then the profile, then the server tag; on a server, the
// member, then the server profile.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Request {
    User(Value),
    Profile(Value),
    ServerTag(Option<String>),
    Member { guild_id: String, body: Value },
    GuildProfile { guild_id: String, body: Value },
}

pub(crate) fn main_changes(saved: &MainProfile, draft: &MainProfile) -> Vec<Field> {
    [
        (Field::DisplayName, saved.display_name.trim() != draft.display_name.trim()),
        (Field::Pronouns, saved.pronouns.trim() != draft.pronouns.trim()),
        (Field::Avatar, draft.avatar != AvatarChoice::Current),
        (Field::BannerColour, saved.accent != draft.accent),
        (Field::AboutMe, saved.bio.trim_end() != draft.bio.trim_end()),
        (Field::ServerTag, saved.tag_guild != draft.tag_guild),
    ]
    .into_iter()
    .filter_map(|(field, changed)| changed.then_some(field))
    .collect()
}

pub(crate) fn server_changes(saved: &ServerProfile, draft: &ServerProfile) -> Vec<Field> {
    [(Field::ServerNickname, saved.nick.trim() != draft.nick.trim()), (Field::ServerPronouns, saved.pronouns.trim() != draft.pronouns.trim())]
        .into_iter()
        .filter_map(|(field, changed)| changed.then_some(field))
        .collect()
}

pub(crate) fn main_requests(saved: &MainProfile, draft: &MainProfile) -> Vec<Request> {
    let changes = main_changes(saved, draft);
    let mut user = Map::new();
    let mut profile = Map::new();
    let mut tag = None;
    for field in changes {
        match field {
            Field::DisplayName => {
                user.insert("global_name".into(), text_or_null(&draft.display_name));
            }
            Field::Avatar => match &draft.avatar {
                AvatarChoice::Recent(id) => {
                    user.insert("avatar_id".into(), json!(id));
                }
                AvatarChoice::Upload { data, .. } => {
                    user.insert("avatar".into(), json!(data));
                }
                AvatarChoice::Removed => {
                    user.insert("avatar".into(), Value::Null);
                }
                AvatarChoice::Current => {}
            },
            // Not verified: Discord clearing pronouns and bio with an empty string rather than null.
            Field::Pronouns => {
                profile.insert("pronouns".into(), json!(draft.pronouns.trim()));
            }
            Field::AboutMe => {
                profile.insert("bio".into(), json!(draft.bio.trim_end()));
            }
            Field::BannerColour => {
                profile.insert("accent_color".into(), json!(draft.accent));
            }
            Field::ServerTag => tag = Some(draft.tag_guild.clone()),
            Field::ServerNickname | Field::ServerPronouns => {}
        }
    }
    let mut out = Vec::new();
    if !user.is_empty() {
        out.push(Request::User(Value::Object(user)));
    }
    if !profile.is_empty() {
        out.push(Request::Profile(Value::Object(profile)));
    }
    out.extend(tag.map(Request::ServerTag));
    out
}

pub(crate) fn server_requests(guild_id: &str, saved: &ServerProfile, draft: &ServerProfile) -> Vec<Request> {
    let changes = server_changes(saved, draft);
    let mut out = Vec::new();
    // Not verified: an empty nick sent as null resetting it to your display name.
    if changes.contains(&Field::ServerNickname) {
        out.push(Request::Member { guild_id: guild_id.to_owned(), body: json!({ "nick": text_or_null(&draft.nick) }) });
    }
    if changes.contains(&Field::ServerPronouns) {
        out.push(Request::GuildProfile { guild_id: guild_id.to_owned(), body: json!({ "pronouns": draft.pronouns.trim() }) });
    }
    out
}

// "Display name, About me": what the save bar says changed.
pub(crate) fn changed_names(fields: &[Field]) -> String {
    fields.iter().map(|f| f.label()).collect::<Vec<_>>().join(", ")
}

fn text_or_null(text: &str) -> Value {
    let text = text.trim();
    if text.is_empty() { Value::Null } else { json!(text) }
}

// From `GET /users/{id}/profile`.
// Not verified: `user.primary_guild.identity_guild_id` on this endpoint's user, and `user_profile.accent_color`.
pub(crate) fn main_from(body: &Value) -> MainProfile {
    let (user, profile) = (&body["user"], &body["user_profile"]);
    let guild = &user["primary_guild"];
    let text = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    MainProfile {
        display_name: text(&user["global_name"]),
        pronouns: text(&profile["pronouns"]),
        bio: profile["bio"].as_str().or(user["bio"].as_str()).unwrap_or_default().to_owned(),
        accent: profile["accent_color"].as_u64().or(user["accent_color"].as_u64()).map(|c| c as u32),
        tag_guild: guild["identity_guild_id"].as_str().filter(|_| guild["identity_enabled"].as_bool() != Some(false)).map(str::to_owned),
        avatar: AvatarChoice::Current,
    }
}

// From `GET /users/{id}/profile?guild_id=…`.
// Not verified: `guild_member.nick` and `guild_member_profile.pronouns` on this endpoint.
pub(crate) fn server_from(body: &Value) -> ServerProfile {
    ServerProfile { nick: body["guild_member"]["nick"].as_str().unwrap_or_default().to_owned(), pronouns: body["guild_member_profile"]["pronouns"].as_str().unwrap_or_default().to_owned() }
}

pub(crate) fn recent_from(body: &Value, user_id: &str) -> Vec<RecentAvatar> {
    body["avatars"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let (id, hash) = (a["id"].as_str()?, a["storage_hash"].as_str()?);
            Some(RecentAvatar { id: id.to_owned(), url: format!("{CDN}/avatars/{user_id}/archived/{id}/{hash}.png?size=128") })
        })
        .take(RECENT_AVATARS)
        .collect()
}

pub(crate) fn tag_from(body: &Value, guild_id: &str, name: &str) -> Option<TagGuild> {
    let tag = body["tag"].as_str().filter(|t| !t.is_empty())?;
    let badge = body["badge"].as_str().map(|b| format!("{CDN}/clan-badges/{guild_id}/{b}.png?size=32"));
    Some(TagGuild { guild_id: guild_id.to_owned(), name: name.to_owned(), tag: tag.to_owned(), badge })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved() -> MainProfile {
        MainProfile { display_name: "Juno".into(), pronouns: String::new(), bio: "Plays support.".into(), accent: Some(0x3498DB), tag_guild: Some("g1".into()), avatar: AvatarChoice::Current }
    }

    #[test]
    fn only_real_changes_count() {
        let base = saved();
        assert!(main_changes(&base, &base).is_empty());
        let mut draft = base.clone();
        draft.display_name = " Juno ".into();
        draft.bio = "Plays support.\n".into();
        assert!(main_changes(&base, &draft).is_empty(), "whitespace alone isn't a change");
        draft.display_name = "Juniper".into();
        draft.accent = None;
        draft.avatar = AvatarChoice::Recent("a1".into());
        assert_eq!(main_changes(&base, &draft), [Field::DisplayName, Field::Avatar, Field::BannerColour]);
        assert_eq!(changed_names(&main_changes(&base, &draft)), "Display name, Avatar, Banner colour");
        let server = ServerProfile { nick: "J".into(), pronouns: String::new() };
        assert_eq!(server_changes(&server, &ServerProfile { nick: "J".into(), pronouns: "she/her".into() }), [Field::ServerPronouns]);
    }

    #[test]
    fn save_sends_only_what_changed_to_its_endpoint_in_discords_order() {
        let base = saved();
        assert!(main_requests(&base, &base).is_empty());
        let mut draft = base.clone();
        draft.tag_guild = None;
        draft.bio = "Heals.".into();
        draft.display_name = "  ".into();
        draft.avatar = AvatarChoice::Recent("a1".into());
        assert_eq!(
            main_requests(&base, &draft),
            [Request::User(json!({ "global_name": null, "avatar_id": "a1" })), Request::Profile(json!({ "bio": "Heals." })), Request::ServerTag(None)]
        );
        let mut colour = base.clone();
        colour.accent = None;
        colour.avatar = AvatarChoice::Removed;
        assert_eq!(main_requests(&base, &colour), [Request::User(json!({ "avatar": null })), Request::Profile(json!({ "accent_color": null }))]);
        let server = ServerProfile::default();
        let draft = ServerProfile { nick: "Juno on night shift".into(), pronouns: "they/them".into() };
        assert_eq!(
            server_requests("g1", &server, &draft),
            [Request::Member { guild_id: "g1".into(), body: json!({ "nick": "Juno on night shift" }) }, Request::GuildProfile { guild_id: "g1".into(), body: json!({ "pronouns": "they/them" }) }]
        );
    }

    #[test]
    fn loads_profile_avatars_and_tags() {
        let body = json!({ "user": { "global_name": "Juno", "primary_guild": { "identity_guild_id": "g1", "identity_enabled": true } }, "user_profile": { "bio": "hi", "pronouns": "she/her", "accent_color": 255 } });
        assert_eq!(main_from(&body), MainProfile { display_name: "Juno".into(), pronouns: "she/her".into(), bio: "hi".into(), accent: Some(255), tag_guild: Some("g1".into()), avatar: AvatarChoice::Current });
        let avatars = json!({ "avatars": (0..8).map(|i| json!({ "id": format!("a{i}"), "storage_hash": "h" })).collect::<Vec<_>>() });
        let recent = recent_from(&avatars, "u");
        assert_eq!(recent.len(), RECENT_AVATARS);
        assert_eq!(recent[0].url, format!("{CDN}/avatars/u/archived/a0/h.png?size=128"));
        assert!(tag_from(&json!({ "tag": "" }), "g", "G").is_none());
        assert_eq!(tag_from(&json!({ "tag": "NITE" }), "g", "Night Shift").map(|t| t.tag), Some("NITE".into()));
    }
}
