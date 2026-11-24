use super::*;

/// An image as Discord's media proxy scales it: `width` x `height` (logical pixels, drawn at up to 2x
/// for high-DPI screens), never larger than the original. Attachments on cdn.discordapp.com move to
/// media.discordapp.net, which resizes on request, as Discord's own client does; the signed query
/// (ex, is, hm) is kept. Other hosts come back unchanged. Not verified against every proxy host:
/// images-ext-*.discordapp.net (embeds) is assumed to take the same parameters.
pub fn sized_image(url: &str, width: f32, height: f32, original: (f32, f32)) -> String {
    let scale = (2. * width / original.0.max(1.)).min(2. * height / original.1.max(1.)).min(1.);
    let Some(proxied) = proxied(url) else { return url.to_owned() };
    if scale >= 1. {
        return proxied;
    }
    with_query(proxied, &scaled(original, scale))
}

/// A thumbnail that covers a `side` x `side` square of device pixels: the poster for a video or GIF,
/// scaled so its shorter edge is `side`. Pictures on media.discordapp.net also come back as WebP.
/// Not verified: `format=webp` is what Discord's own client asks the proxy for, not a documented parameter.
pub fn thumb_url(media: &Media, side: f32) -> String {
    let source = match &media.kind {
        MediaKind::Image => &media.url,
        MediaKind::Video { poster } | MediaKind::Gif { poster } => poster,
    };
    let Some(proxied) = proxied(source) else { return source.clone() };
    let original = (media.width.max(1.), media.height.max(1.));
    let scale = (side / original.0).max(side / original.1).min(1.);
    let webp = matches!(media.kind, MediaKind::Image) && proxied.starts_with("https://media.discordapp.net/");
    let size = if scale < 1. { scaled(original, scale) } else { String::new() };
    match (size.is_empty(), webp) {
        (true, false) => proxied,
        (true, true) => with_query(proxied, "format=webp"),
        (false, false) => with_query(proxied, &size),
        (false, true) => with_query(proxied, &format!("{size}&format=webp")),
    }
}

// The media-proxy address for `url`, or None when its host doesn't resize.
pub(super) fn proxied(url: &str) -> Option<String> {
    if let Some(rest) = url.strip_prefix("https://cdn.discordapp.com/attachments/") {
        return Some(format!("https://media.discordapp.net/attachments/{rest}"));
    }
    let resizes = url.starts_with("https://media.discordapp.net/") || (url.starts_with("https://images-ext-") && url.contains(".discordapp.net/"));
    resizes.then(|| url.to_owned())
}

pub(super) fn scaled(original: (f32, f32), scale: f32) -> String {
    let (w, h) = ((original.0 * scale).round().max(1.) as u32, (original.1 * scale).round().max(1.) as u32);
    format!("width={w}&height={h}")
}

pub(super) fn with_query(url: String, query: &str) -> String {
    let joiner = if url.contains('?') { "&" } else { "?" };
    format!("{url}{joiner}{query}")
}

/// The moving version of an avatar or decoration URL, for while Discord would animate it: an animated
/// (`a_`) avatar as its GIF, a decoration with `passthrough=true` (its APNG, as Discord's client asks for).
/// Anything else comes back as it is.
pub fn animated(url: &str) -> String {
    if url.contains("/avatars/") && url.contains("/a_") {
        return url.replacen(".png?", ".gif?", 1);
    }
    url.replacen("passthrough=false", "passthrough=true", 1)
}

pub fn avatar_url(user_id: &str, hash: Option<&str>) -> String {
    match hash.filter(|h| !h.is_empty() && *h != "0") {
        Some(hash) => format!("{CDN}/avatars/{user_id}/{hash}.png?size=128"),
        // Discord's default avatars, chosen by (id >> 22) % 6 for accounts on the new username system.
        None => format!("{CDN}/embed/avatars/{}.png", user_id.parse::<u64>().map(|id| (id >> 22) % 6).unwrap_or(0)),
    }
}

// What stands for a DM wherever it shows: a group's own icon, the other person, or (a group without
// an icon) two members on a diagonal, as Discord draws it.
#[derive(Clone, Debug, PartialEq)]
pub enum DmFace {
    Icon(String),
    One(String),
    Pair(String, String),
    // A group everyone else has left.
    Letters(String),
}

impl Channel {
    pub fn face(&self) -> DmFace {
        if let Some(icon) = &self.icon {
            return DmFace::Icon(icon.clone());
        }
        if let Some(user) = &self.recipient {
            return DmFace::One(user.avatar.clone());
        }
        match self.recipients.as_slice() {
            [] => DmFace::Letters(self.name.clone()),
            [only] => DmFace::One(only.avatar.clone()),
            [back, front, ..] => DmFace::Pair(back.avatar.clone(), front.avatar.clone()),
        }
    }
}

pub(super) fn embed_media(v: &Value) -> Option<Media> {
    let url = v["proxy_url"].as_str().or(v["url"].as_str())?;
    Some(Media { url: url.to_owned(), width: v["width"].as_f64()? as f32, height: v["height"].as_f64()? as f32, kind: MediaKind::Image, ..Default::default() })
}

// Embed flags: CONTAINS_EXPLICIT_CONTENT, CONTAINS_GORE_CONTENT, CONTAINS_SELF_HARM_CONTENT.
pub(super) const SENSITIVE_EMBED: u64 = 1 << 4 | 1 << 6 | 1 << 7;

pub(super) fn parse_embed(e: &Value) -> Embed {
    let text = |v: &Value| v.as_str().filter(|t| !t.is_empty()).map(str::to_owned);
    if e["type"] == "poll_result" {
        return Embed { poll: Some(parse_poll_result(e)), ..Default::default() };
    }
    let video = e["type"] == "video" || e["video"].is_object();
    // A video's still is its thumbnail; it plays large, so it never sits small at the right.
    let image = embed_media(&e["image"]).or_else(|| video.then(|| embed_media(&e["thumbnail"])).flatten());
    Embed {
        url: text(&e["url"]),
        color: e["color"].as_u64().map(|c| c as u32),
        provider: text(&e["provider"]["name"]),
        provider_url: text(&e["provider"]["url"]),
        author: text(&e["author"]["name"]),
        author_icon: text(&e["author"]["proxy_icon_url"]).or_else(|| text(&e["author"]["icon_url"])),
        author_url: text(&e["author"]["url"]),
        title: text(&e["title"]),
        description: text(&e["description"]),
        fields: e["fields"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| (f["name"].as_str().unwrap_or_default().to_owned(), f["value"].as_str().unwrap_or_default().to_owned(), f["inline"].as_bool().unwrap_or(false)))
            .collect(),
        thumbnail: (!video).then(|| embed_media(&e["thumbnail"])).flatten(),
        images: image.into_iter().collect(),
        video,
        sensitive: e["flags"].as_u64().unwrap_or(0) & SENSITIVE_EMBED != 0,
        footer: text(&e["footer"]["text"]),
        footer_icon: text(&e["footer"]["proxy_icon_url"]).or_else(|| text(&e["footer"]["icon_url"])),
        timestamp: text(&e["timestamp"]),
        poll: None,
        safety_alert: e["type"] == "auto_moderation_notification",
        rich: e["type"].as_str().unwrap_or("rich") == "rich",
    }
}

// Counts arrive apart from the answers, in `results`; an answer missing there has no votes.
pub(super) fn parse_poll(p: &Value) -> Poll {
    let counts = p["results"]["answer_counts"].as_array().cloned().unwrap_or_default();
    let answers = p["answers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let id = a["answer_id"].as_u64()? as u32;
            let result = counts.iter().find(|c| c["id"].as_u64() == Some(id.into()));
            let media = &a["poll_media"];
            Some(PollAnswer {
                id,
                text: media["text"].as_str().unwrap_or_default().to_owned(),
                emoji: media["emoji"].is_object().then(|| parse_emoji(&media["emoji"])),
                count: result.and_then(|c| c["count"].as_u64()).unwrap_or(0),
                me: result.and_then(|c| c["me_voted"].as_bool()).unwrap_or(false),
            })
        })
        .collect();
    Poll {
        question: p["question"]["text"].as_str().unwrap_or_default().to_owned(),
        answers,
        expiry: p["expiry"].as_str().map(str::to_owned),
        multiselect: p["allow_multiselect"].as_bool().unwrap_or(false),
        finalized: p["results"]["is_finalized"].as_bool().unwrap_or(false),
    }
}

// A poll_result embed carries its numbers as fields named after them.
pub(super) fn parse_poll_result(e: &Value) -> PollResult {
    let fields: HashMap<&str, &str> = e["fields"].as_array().into_iter().flatten().filter_map(|f| Some((f["name"].as_str()?, f["value"].as_str()?))).collect();
    let number = |name: &str| fields.get(name).and_then(|v| v.parse().ok()).unwrap_or(0);
    let emoji = fields.get("victor_answer_emoji_id").map(|id| Emoji { id: Some((*id).to_owned()), name: fields.get("victor_answer_emoji_name").copied().unwrap_or_default().to_owned(), animated: fields.get("victor_answer_emoji_animated") == Some(&"true") })
        .or_else(|| fields.get("victor_answer_emoji_name").map(|name| Emoji::unicode(name)));
    PollResult {
        question: fields.get("poll_question_text").copied().unwrap_or_default().to_owned(),
        winner: fields.get("victor_answer_text").map(|text| ((*text).to_owned(), emoji)),
        winner_votes: number("victor_answer_votes"),
        total_votes: number("total_votes"),
    }
}

pub(super) fn parse_presence(p: &Value) -> Presence {
    let activities = p["activities"].as_array().cloned().unwrap_or_default();
    let custom = activities.iter().find(|a| a["type"] == 4).and_then(|a| {
        let text = a["state"].as_str().filter(|t| !t.is_empty())?;
        let emoji = a["emoji"]["name"].as_str().filter(|_| a["emoji"]["id"].is_null()).map(|e| format!("{e} ")).unwrap_or_default();
        Some(format!("{emoji}{text}"))
    });
    let others: Vec<&Value> = activities.iter().filter(|a| a["type"] != 4).collect();
    let line = |a: &Value| {
        let name = a["name"].as_str()?;
        let (kind, text) = match a["type"].as_u64() {
            Some(1) => (ActivityKind::Streaming, a["details"].as_str().unwrap_or(name)),
            Some(2) => (ActivityKind::Listening, a["state"].as_str().unwrap_or(name)),
            Some(3) => (ActivityKind::Watching, name),
            Some(5) => (ActivityKind::Competing, name),
            _ => (ActivityKind::Playing, name),
        };
        Some((kind, text.to_owned()))
    };
    let mut lines = others.iter().filter_map(|a| line(a));
    let first = lines.next().map(|(kind, text)| Activity { kind, text, rest: lines.collect() });
    let rich = others.first().and_then(|a| parse_rich(a));
    let clients = &p["client_status"];
    let mobile_only = !clients["mobile"].is_null() && clients["desktop"].is_null() && clients["web"].is_null();
    let streaming = others.iter().any(|a| a["type"] == 1);
    Presence { status: Status::parse(p["status"].as_str().unwrap_or("offline")), custom, activity: first, rich, mobile_only, streaming }
}

// Field names from the diself crate's model (src/model/user.rs), not from Discord's docs (the endpoint is undocumented).
// A server profile (`guild_member_profile`) overrides the main one field by field, as Discord's display profile
// does: its banner, accent, theme colours, bio and pronouns where set, and `guild_badges` join the badges.
pub(crate) fn parse_profile(user_id: &str, body: &Value) -> Profile {
    let user = &body["user"];
    let profile = &body["user_profile"];
    let server = &body["guild_member_profile"];
    let ext = |hash: &str| if hash.starts_with("a_") { "gif" } else { "png" };
    let server_banner = server["banner"].as_str().zip(server["guild_id"].as_str()).map(|(hash, guild)| format!("{CDN}/guilds/{guild}/users/{user_id}/banners/{hash}.{}?size=600", ext(hash)));
    let banner = server_banner.or_else(|| user["banner"].as_str().map(|hash| format!("{CDN}/banners/{user_id}/{hash}.{}?size=600", ext(hash))));
    let colors = |v: &Value| match v.as_array().map(|c| c.iter().filter_map(Value::as_u64).collect::<Vec<_>>()).as_deref() {
        Some([top, bottom]) => Some((*top as u32, *bottom as u32)),
        _ => None,
    };
    let theme = colors(&server["theme_colors"]).or_else(|| colors(&profile["theme_colors"]));
    let text = |v: &Value| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned);
    let badge = |b: &Value| Some(Badge { description: b["description"].as_str().unwrap_or_default().to_owned(), icon: format!("{CDN}/badge-icons/{}.png", b["icon"].as_str()?), link: b["link"].as_str().map(str::to_owned) });
    let badges = body["badges"].as_array().into_iter().flatten().chain(body["guild_badges"].as_array().into_iter().flatten()).filter_map(badge).collect();
    let mutual_friends: Vec<User> = body["mutual_friends"].as_array().into_iter().flatten().map(parse_user).collect();
    let mutual_guilds: Vec<MutualGuild> = body["mutual_guilds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| Some(MutualGuild { id: g["id"].as_str()?.to_owned(), nick: g["nick"].as_str().map(str::to_owned) }))
        .collect();
    let mutual_friend_count = body["mutual_friend_count"].as_u64().or(body["mutual_friends_count"].as_u64()).map_or(mutual_friends.len(), |n| n as usize);
    Profile {
        username: user["username"].as_str().unwrap_or_default().to_owned(),
        bio: text(&server["bio"]).or_else(|| text(&profile["bio"])).or_else(|| text(&user["bio"])),
        pronouns: text(&server["pronouns"]).or_else(|| text(&profile["pronouns"])),
        banner,
        accent: server["accent_color"].as_u64().or(user["accent_color"].as_u64()).or(user["banner_color"].as_str().and_then(|c| u32::from_str_radix(c.trim_start_matches('#'), 16).ok().map(u64::from))).map(|c| c as u32),
        roles: strings(&body["guild_member"]["roles"]),
        decoration: user.is_object().then(|| parse_user(user).decoration).flatten(),
        theme,
        badges,
        mutual_friends,
        mutual_friend_count,
        mutual_server_count: mutual_guilds.len(),
        mutual_guilds,
        note: None,
        games: Vec::new(),
        joined: body["guild_member"]["joined_at"].as_str().map(str::to_owned),
        connections: body["connected_accounts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| {
                Some(Connection { kind: c["type"].as_str()?.to_owned(), id: c["id"].as_str()?.to_owned(), name: c["name"].as_str().unwrap_or_default().to_owned(), verified: c["verified"].as_bool() == Some(true) })
            })
            .collect(),
        effect_sku: effect_sku(&body["guild_member_profile"]["collectibles"]).or_else(|| effect_sku(&profile["collectibles"])),
        frame_sku: collectible_sku(&body["guild_member_profile"]["collectibles"], PROFILE_FRAME).or_else(|| collectible_sku(&profile["collectibles"], PROFILE_FRAME)),
        frame: None,
    }
}

// A user's content outbox as Recent activity rows. Discord's `content_type`: 1 played and 3 top game, 2 watched,
// 4 listened and 5 a listening session; its trait types: 1 first time, 2 duration in seconds, 8 days in a row.
// Not verified against a live response: read from Discord's client, which renders these fields.
pub(crate) fn parse_outbox(body: &Value) -> Vec<RecentEntry> {
    let trait_of = |e: &Value, kind: u64| e["traits"].as_array().and_then(|t| t.iter().find(|t| t["type"].as_u64() == Some(kind))).cloned();
    body["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let extra = &e["extra"];
            let (kind, title, subtitle) = match e["content_type"].as_u64()? {
                1 | 3 => (RecentKind::Played, extra["game_name"].as_str()?.to_owned(), None),
                2 => (RecentKind::Watched, extra["media_title"].as_str()?.to_owned(), extra["media_subtitle"].as_str().map(str::to_owned)),
                4 | 5 => {
                    let media = &extra["entries"][0]["media"];
                    let media = if media.is_object() { media } else { &extra["media"] };
                    (RecentKind::Listened, media["title"].as_str()?.to_owned(), media["artists"][0]["name"].as_str().map(str::to_owned))
                }
                _ => return None,
            };
            let hours = trait_of(e, 2).and_then(|t| t["duration_seconds"].as_u64()).map(|s| (s + 1800) / 3600).filter(|h| *h > 0);
            Some(RecentEntry {
                id: e["id"].as_str()?.to_owned(),
                kind,
                title,
                subtitle,
                app_id: extra["application_id"].as_str().map(str::to_owned),
                first_time: trait_of(e, 1).is_some_and(|t| t["first_time"].as_bool() == Some(true)),
                marathon_hours: hours,
                streak_days: trait_of(e, 8).and_then(|t| t["streak_count_days"].as_u64()).filter(|d| *d > 1),
            })
        })
        .collect()
}

// Discord's collectible types: 0 avatar decoration, 1 profile effect, 2 nameplate, 3 profile frame.
const PROFILE_EFFECT: u64 = 1;
const PROFILE_FRAME: u64 = 3;

// The unexpired profile effect in a `collectibles[]` list, `{sku_id, type, expires_at}`.
pub(super) fn effect_sku(collectibles: &Value) -> Option<String> {
    collectible_sku(collectibles, PROFILE_EFFECT)
}

// The unexpired collectible of `kind` in a `collectibles[]` list.
pub(super) fn collectible_sku(collectibles: &Value, kind: u64) -> Option<String> {
    let now = chrono::Utc::now();
    collectibles
        .as_array()?
        .iter()
        .filter(|c| c["type"].as_u64() == Some(kind))
        .filter(|c| c["expires_at"].as_str().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).is_none_or(|t| t > now))
        .find_map(|c| c["sku_id"].as_str().map(str::to_owned))
}

// Not verified: that the product carries the effect's config (`effects[]` of `{src, width, height, position,
// zIndex, randomizedSources}`) on one of its `items`; a body without it gives no layers and the card draws none.
// GAP: Discord picks one random `randomizedSources` entry per open; this takes the first.
pub(super) fn parse_profile_effect(body: &Value) -> Vec<EffectLayer> {
    let item = body["items"].as_array().into_iter().flatten().find(|i| i["effects"].is_array());
    let Some(effects) = item.and_then(|i| i["effects"].as_array()) else { return Vec::new() };
    let mut layers: Vec<EffectLayer> = effects
        .iter()
        .filter_map(|e| {
            let src = e["randomizedSources"][0]["src"].as_str().or(e["src"].as_str())?.to_owned();
            let num = |v: &Value| v.as_f64().unwrap_or(0.) as f32;
            Some(EffectLayer { src, x: num(&e["position"]["x"]), y: num(&e["position"]["y"]), width: num(&e["width"]), height: num(&e["height"]), z: e["zIndex"].as_i64().unwrap_or(0) })
        })
        .filter(|l| l.width > 0. && l.height > 0.)
        .collect();
    layers.sort_by_key(|l| l.z);
    layers
}

// A frame product's layers (`items[]` of type 3), each drawn from `media/v1/collectibles-shop/{sku}/{layer}/static`.
// Read from Discord's client; not checked against a live product.
pub(super) fn parse_profile_frame(sku: &str, body: &Value) -> Option<ProfileFrame> {
    let item = body["items"].as_array()?.iter().find(|i| i["type"].as_u64() == Some(PROFILE_FRAME) || i["layers"].is_array())?;
    let num = |v: &Value, or: f32| v.as_f64().map_or(or, |n| n as f32);
    let layers = item["layers"]
        .as_array()?
        .iter()
        .filter_map(|l| {
            let kind = match l["type"].as_str()? {
                "staple" => FrameKind::Staple,
                "rail" => FrameKind::Rail,
                "border" => FrameKind::Border,
                _ => return None,
            };
            let anchor = match l["anchor"].as_str() {
                Some("top") => FrameAnchor::Top,
                Some("bottom") => FrameAnchor::Bottom,
                _ => FrameAnchor::Center,
            };
            let id = l["id"].as_str()?;
            Some(FrameLayer { src: format!("{CDN}/media/v1/collectibles-shop/{sku}/{id}/static"), kind, front: l["order"].as_str() == Some("front"), anchor })
        })
        .collect::<Vec<_>>();
    (!layers.is_empty()).then(|| ProfileFrame {
        inner_width: num(&item["inner_width"], 1200.),
        overflow_top: num(&item["overflow_top"], 0.),
        overflow_bottom: num(&item["overflow_bottom"], 0.),
        overflow_horizontal: num(&item["overflow_horizontal"], 0.),
        layers,
    })
}

pub fn app_icon_url(app_id: &str, icon: &str) -> String {
    format!("{CDN}/app-icons/{app_id}/{icon}.png?size=64")
}

// Large asset keys: "mp:…" is a media proxy path, "spotify:…" an album cover, anything else an app asset id.
pub(super) fn activity_image(a: &Value) -> Option<String> {
    let key = a["assets"]["large_image"].as_str()?;
    if let Some(path) = key.strip_prefix("mp:") {
        return Some(format!("https://media.discordapp.net/{path}"));
    }
    // Not verified: Spotify covers resolve on Spotify's image CDN, as Discord's activity asset docs describe.
    if let Some(id) = key.strip_prefix("spotify:") {
        return Some(format!("https://i.scdn.co/image/{id}"));
    }
    Some(format!("{CDN}/app-assets/{}/{key}.png", a["application_id"].as_str()?))
}

pub(super) fn parse_rich(a: &Value) -> Option<RichPresence> {
    const JOIN: u64 = 1 << 1;
    let text = |k: &str| a[k].as_str().filter(|t| !t.is_empty()).map(str::to_owned);
    let size = a["party"]["size"].as_array().map(|s| s.iter().filter_map(Value::as_u64).collect::<Vec<_>>());
    Some(RichPresence {
        kind: match a["type"].as_u64() {
            Some(1) => ActivityKind::Streaming,
            Some(2) => ActivityKind::Listening,
            Some(3) => ActivityKind::Watching,
            Some(5) => ActivityKind::Competing,
            _ => ActivityKind::Playing,
        },
        name: text("name")?,
        app_id: text("application_id"),
        details: text("details"),
        state: text("state"),
        image: activity_image(a),
        party: match size.as_deref() {
            Some([current, max]) => Some((*current, *max)),
            _ => None,
        },
        started: a["timestamps"]["start"].as_i64(),
        ends: a["timestamps"]["end"].as_i64(),
        party_id: a["party"]["id"].as_str().map(str::to_owned),
        joinable: a["party"]["id"].is_string() && (a["flags"].as_u64().unwrap_or(0) & JOIN != 0 || a["secrets"]["join"].is_string()),
    })
}

pub(super) fn parse_list_op(op: &Value, guild_id: Option<&str>) -> Option<ListOp> {
    let index = op["index"].as_u64().map(|i| i as usize);
    Some(match op["op"].as_str()? {
        "SYNC" => ListOp::Sync {
            start: op["range"][0].as_u64().unwrap_or(0) as usize,
            items: op["items"].as_array().into_iter().flatten().filter_map(|item| parse_list_item(item, guild_id)).collect(),
        },
        "INSERT" => ListOp::Insert { index: index?, item: parse_list_item(&op["item"], guild_id)? },
        "UPDATE" => ListOp::Update { index: index?, item: parse_list_item(&op["item"], guild_id)? },
        "DELETE" => ListOp::Delete { index: index? },
        _ => return None,
    })
}

pub(super) fn parse_list_item(item: &Value, guild_id: Option<&str>) -> Option<ListItem> {
    if let Some(group) = item["group"].as_object() {
        return Some(ListItem::Group { id: group.get("id")?.as_str()?.to_owned(), count: group.get("count").and_then(Value::as_u64).unwrap_or(0) as u32 });
    }
    let m = &item["member"];
    m.is_object().then(|| ListItem::Member(Member { user: parse_member_user(m, &m["user"], guild_id), roles: strings(&m["roles"]), presence: parse_presence(&m["presence"]), boosting_since: m["premium_since"].as_str().map(str::to_owned) }))
}

pub(super) fn parse_voice_state(v: &Value) -> VoiceState {
    let user = &v["member"]["user"];
    VoiceState {
        guild_id: v["guild_id"].as_str().map(str::to_owned),
        channel_id: v["channel_id"].as_str().map(str::to_owned),
        user_id: v["user_id"].as_str().unwrap_or_default().to_owned(),
        user: user.is_object().then(|| parse_member_user(&v["member"], user, v["guild_id"].as_str())),
        muted: v["self_mute"].as_bool().unwrap_or(false) || v["mute"].as_bool().unwrap_or(false),
        deafened: v["self_deaf"].as_bool().unwrap_or(false) || v["deaf"].as_bool().unwrap_or(false),
        server_muted: v["mute"].as_bool().unwrap_or(false),
        server_deafened: v["deaf"].as_bool().unwrap_or(false),
        streaming: v["self_stream"].as_bool().unwrap_or(false),
        camera: v["self_video"].as_bool().unwrap_or(false),
        suppress: v["suppress"].as_bool().unwrap_or(false),
        requested_at: v["request_to_speak_timestamp"].as_str().map(str::to_owned),
    }
}

pub(super) fn parse_emoji(e: &Value) -> Emoji {
    Emoji { id: e["id"].as_str().map(str::to_owned), name: e["name"].as_str().unwrap_or("?").to_owned(), animated: e["animated"].as_bool().unwrap_or(false) }
}

pub(super) fn strings(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_owned)).collect()
}

// Permission bitfields arrive as strings (they overflow JS numbers); older payloads used numbers.
pub(super) fn permission(v: &Value) -> u64 {
    v.as_str().and_then(|s| s.parse().ok()).or(v.as_u64()).unwrap_or(0)
}

// `GET /content-inventory/users/@me` answers `{ entries: [{ content }] }`; each content is a Content
// Inventory Entry as discord-userdoccers documents it (resources/components). Not verified against a
// live response: Waffle never signs in for development.
pub fn parse_activity_feed(body: &Value) -> Vec<FeedEntry> {
    body["entries"].as_array().into_iter().flatten().filter_map(|e| parse_feed_entry(&e["content"])).collect()
}

pub(super) fn parse_feed_entry(c: &Value) -> Option<FeedEntry> {
    let extra = &c["extra"];
    let text = |v: &Value| v.as_str().map(str::trim).filter(|t| !t.is_empty()).map(str::to_owned);
    let url = |v: &Value| text(v).filter(|u| u.starts_with("https://"));
    let media = if extra["entries"][0]["media"].is_object() { &extra["entries"][0]["media"] } else { extra };
    let (kind, title, image) = match c["content_type"].as_u64()? {
        1 | 3 => (ActivityKind::Playing, text(&extra["game_name"])?, None),
        2 => (ActivityKind::Watching, text(&extra["media_title"])?, url(&extra["media_assets_large_image"])),
        4 | 5 => (ActivityKind::Listening, text(&media["title"])?, url(&media["image_url"])),
        8 => (ActivityKind::Playing, text(&extra["activity_name"])?, None),
        _ => return None,
    };
    let traits = c["traits"].as_array().cloned().unwrap_or_default();
    let trait_of = |kind: u64| traits.iter().find(|t| t["type"].as_u64() == Some(kind)).cloned();
    let time = |v: &Value| v.as_str().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).map(|t| t.timestamp_millis());
    let made = (snowflake(&c["id"]) >> 22) as i64 + 1_420_070_400_000;
    Some(FeedEntry {
        author_id: text(&c["author_id"])?,
        kind,
        title,
        image,
        app_id: text(&extra["application_id"]),
        live: trait_of(3).is_some_and(|t| t["is_live"].as_bool() == Some(true)),
        at: time(&c["ended_at"]).or_else(|| time(&c["started_at"])).unwrap_or(made),
        streak_days: trait_of(8).and_then(|t| t["streak_count_days"].as_u64()).filter(|&d| d > 1),
    })
}

pub fn snowflake(v: &Value) -> u64 {
    v.as_str().and_then(|s| s.parse().ok()).unwrap_or(0)
}
