// The forum scenes of the debug demo (`make demo`): fixtures and the scene each name opens.
use gpui_kit::{Context, Window};
use serde_json::{Value, json};

use super::{DISCORD_EPOCH, ForumState, PendingFile};
use crate::{
    Waffle,
    discord::{ReadState, parse_channel, parse_message},
};

// Where the demo opens the Sort & View menu: under the toolbar's right end.
const SORT_MENU_AT: (f32, f32) = (1000., 110.);

// (hours ago, title, author id, author, tags, pinned, archived, replies, opening message, pictures)
type PostFixture = (i64, &'static str, &'static str, &'static str, &'static [&'static str], bool, bool, u32, &'static str, &'static [&'static str]);

// How far we've read each post: all of it, behind with FORUM_UNREADS' count, or behind before it
// answers. Posts without one read as New when they're newer than the forum's read state.
#[derive(Clone, Copy)]
enum Read {
    All,
    Behind(Option<u32>),
    Never,
}

const READS: [Read; 8] = [Read::All, Read::Behind(Some(31)), Read::Never, Read::Behind(Some(4)), Read::All, Read::Behind(None), Read::All, Read::All];

// A snowflake from `hours` ago, so cards show believable times.
fn id_ago(hours: i64, salt: u64) -> String {
    let ms = chrono::Utc::now().timestamp_millis() - hours * 3_600_000;
    ((((ms - DISCORD_EPOCH) as u64) << 22) + salt).to_string()
}

fn tag(id: &str, name: &str, emoji: Option<&str>, moderated: bool) -> Value {
    json!({ "id": id, "name": name, "emoji_id": null, "emoji_name": emoji, "moderated": moderated })
}

// `forum` (list) and `forum-gallery`: a forum with six tags, guidelines, a template and eight posts,
// previews and unread counts filled in; `forum-search`, `forum-sort` and `forum-new-post` open its
// search, Sort & View and New post (with the guidelines and a file).
pub(crate) fn open(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    let forum_json = json!({
        "id": "forum", "type": 15, "name": "game-feedback", "parent_id": "c0", "position": 1,
        "topic": "Ideas and bugs for the next patch. One post per idea, tag it.\n\n## Before you post\n- Search first: add to an existing post instead of starting a new one.\n- **Bugs**: say what you did, what happened and what you expected.\n- **Ideas**: one per post, so they can be ranked by reactions.\n\nPosts without a tag are closed after a day.",
        "template": "**What happened?**\n\n**What did you expect?**\n",
        "rate_limit_per_user": 600,
        "flags": 16, "default_forum_layout": (if scene == "forum-gallery" { 2 } else { 1 }),
        "default_reaction_emoji": { "emoji_id": null, "emoji_name": "\u{1f44d}" },
        "available_tags": [
            tag("t-bug", "Bug", Some("\u{1f41b}"), false),
            tag("t-idea", "Idea", Some("\u{1f4a1}"), false),
            tag("t-balance", "Balance", Some("\u{2696}\u{fe0f}"), false),
            tag("t-ranked", "Ranked", None, false),
            tag("t-maps", "Maps", None, false),
            tag("t-solved", "Solved", Some("\u{2705}"), true),
        ],
    });
    let Some(forum) = parse_channel(&forum_json, "1") else { return };
    let posts: [PostFixture; 8] = [
        (72, "Read me: how ideas get triaged before every patch", "4194304", "kiwi", &["t-idea"], true, false, 12, "Every week we pick the top five ideas by reactions. Tag yours or it won't be seen.", &[]),
        (2, "Show the healer's ult range on the minimap while it charges, and on the scoreboard after a wipe so people can see who stood outside", "8388608", "Wren", &["t-idea", "t-ranked", "t-maps", "t-balance", "t-bug"], false, false, 48, "Half our wipes are people standing one step outside the ring. A faint circle would fix most of it.", &["demo/banner.png"]),
        (5, "Tank shield uptime is out of hand", "12582912", "moss lantern", &["t-balance", "t-ranked"], false, false, 9, "Shield uptime went from 40% to 85% and every ranked match is now a staring contest.", &[]),
        (20, "Night versions of every map", "16777216", "toma reed", &["t-maps", "t-idea"], false, false, 17, "Fog and torches on the big ones. Mockups attached.", &["brand/icons/blueprint.png", "brand/icons/riso.png", "brand/icons/paper.png", "demo/banner.png"]),
        (30, "Crash when alt-tabbing during the loading screen", "20971520", "ferris", &["t-bug"], false, false, 4, "Happens every time on the second map load. Logs below.", &["brand/icons/riso.png"]),
        (50, "Queue times for duo above Diamond", "8388608", "Wren", &[], false, false, 2, "Forty minutes last night. Anyone else?", &[]),
        (400, "Ping the minimap when a teammate is downed", "12582912", "moss lantern", &["t-ranked", "t-solved"], false, true, 1, "Archived after 3 days without replies. Reply to bring it back.", &[]),
        (900, "Audio cuts out after 2 hours of play", "16777216", "toma reed", &["t-bug"], false, true, 6, "Restarting the game fixes it until the next long session.", &["brand/icons/paper.png"]),
    ];
    let mut state = ForumState::default();
    let mut active = Vec::new();
    for (i, (hours, title, author_id, author, tags, pinned, archived, replies, content, pictures)) in posts.into_iter().enumerate() {
        let id = id_ago(hours, i as u64);
        let last = id_ago(hours / 3, 100 + i as u64);
        let post_json = json!({
            "id": id, "type": 11, "name": title, "parent_id": "forum", "guild_id": "1", "owner_id": author_id,
            "last_message_id": last, "message_count": replies, "applied_tags": tags, "flags": (if pinned { 2 } else { 0 }),
            "thread_metadata": { "archived": archived, "locked": false },
        });
        let Some(post) = parse_channel(&post_json, "1") else { continue };
        let attachments: Vec<Value> = pictures.iter().enumerate().map(|(n, url)| json!({ "id": format!("a{i}-{n}"), "filename": "shot.png", "url": url, "proxy_url": url, "width": 1200, "height": 800, "content_type": "image/png" })).collect();
        let face = ["\u{1f525}", "\u{1f44d}", "\u{2764}\u{fe0f}"][i % 3];
        let reactions = if i % 3 == 2 { json!([]) } else { json!([{ "emoji": { "id": null, "name": face }, "count": 3 + i * 7, "me": false }]) };
        let first = json!({
            "id": id, "channel_id": id, "guild_id": "1", "content": content, "timestamp": "2026-11-20T10:00:00Z", "type": 0,
            "author": { "id": author_id, "username": author, "global_name": author, "avatar": null },
            "attachments": attachments, "embeds": [], "reactions": reactions,
        });
        if let Some(message) = parse_message(&first) {
            state.previews.insert(post.id.clone(), message);
        }
        let read = match READS[i] {
            Read::All => Some(last.parse().unwrap_or(0)),
            Read::Behind(_) => Some(id.parse().unwrap_or(0)),
            Read::Never => None,
        };
        if let Some(last_read) = read {
            waffle.read_states.insert(post.id.clone(), ReadState { last_read, mentions: 0 });
        }
        if let Read::Behind(Some(count)) = READS[i] {
            state.unread_counts.insert(post.id.clone(), count);
        }
        match archived {
            true => state.archived.push(post),
            false => active.push(post),
        }
    }
    let forum_read = id_ago(10, 0).parse().unwrap_or(0);
    waffle.read_states.insert("forum".into(), ReadState { last_read: forum_read, mentions: 0 });
    waffle.forums.insert("forum".into(), state);
    if let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") {
        guild.channels.push(forum.clone());
        guild.threads.extend(active);
    }
    waffle.open_channel(forum, window, cx);
    match scene {
        "forum-search" => {
            let search = waffle.ensure_forum_search("forum", window, cx);
            search.update(cx, |s, cx| s.set_value("minimap", window, cx));
            waffle.forums.entry("forum".into()).or_default().filter = vec!["t-idea".into()];
        }
        "forum-sort" => waffle.open_live_menu(gpui_kit::point(gpui_kit::px(SORT_MENU_AT.0), gpui_kit::px(SORT_MENU_AT.1)), |this| this.sort_view_entries("forum"), cx),
        "forum-new-post" => {
            waffle.open_new_post("forum".into(), window, cx);
            if let Some(composer) = waffle.composer_mut() {
                composer.tags = vec!["t-idea".into()];
                composer.files = vec![PendingFile { path: "brand/icons/blueprint.png".into(), name: "night-map-mockup.png".into(), size: 184_320, spoiler: false, description: None }];
            }
        }
        _ => {}
    }
}
