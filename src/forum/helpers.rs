// What the forum computes and draws without the app: arranging and searching posts, snowflake times, the
// picture mosaic, the post body Discord takes, and the small chips and labels.
use super::*;

pub(super) fn insert_previews(state: &mut ForumState, body: &Value) {
    for m in body["first_messages"].as_array().into_iter().flatten().filter_map(parse_message) {
        state.previews.insert(m.channel_id.clone(), m);
    }
}

// Not verified: that the server takes an empty `name` with only tags; Discord's client sends both.
pub(super) fn search_path(forum_id: &str, key: &SearchKey) -> String {
    let mut url = reqwest::Url::parse("https://x/").expect("static URL");
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("name", &key.query);
        if !key.tags.is_empty() {
            query.append_pair("tag", &key.tags.join(","));
        }
        query.append_pair("tag_setting", if key.match_all { "match_all" } else { "match_some" });
    }
    format!("/channels/{forum_id}/threads/search?{}", url.query().unwrap_or_default())
}

// The posts to show, Discord's way: tag filter and sort over everything loaded, pinned first, then
// archived ones under Older Posts. A text search also matches the opening messages we hold, adds
// Discord's own hits, and drops the Older Posts split.
pub(super) fn arrange(posts: Vec<Channel>, found: &[Channel], view: ForumView, filter: &[String], query: &str, previews: &HashMap<String, Message>) -> Listing {
    let words = words(query);
    let mut posts: Vec<Channel> = posts.into_iter().filter(|p| tags_match(p, filter, view.match_all) && words_match(p, previews.get(&p.id), &words)).collect();
    for hit in found.iter().filter(|h| tags_match(h, filter, view.match_all)) {
        if !posts.iter().any(|p| p.id == hit.id) {
            posts.push(hit.clone());
        }
    }
    posts.sort_by_key(|p| Reverse((p.thread.as_ref().is_some_and(|t| t.pinned), sort_key(p, view.by_creation))));
    if !words.is_empty() {
        return Listing { active: posts, older: Vec::new() };
    }
    let (older, active) = posts.into_iter().partition(|p| p.thread.as_ref().is_some_and(|t| t.archived));
    Listing { active, older }
}

// Match Some: any picked tag; Match All: every one. A post without tags never matches a filter.
pub(super) fn tags_match(post: &Channel, filter: &[String], match_all: bool) -> bool {
    if filter.is_empty() {
        return true;
    }
    let applied = post.thread.as_ref().map_or(&[][..], |t| t.applied_tags.as_slice());
    match match_all {
        true => filter.iter().all(|t| applied.contains(t)),
        false => filter.iter().any(|t| applied.contains(t)),
    }
}

pub(super) fn words(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_lowercase).collect()
}

pub(super) fn words_match(post: &Channel, first: Option<&Message>, words: &[String]) -> bool {
    let haystack = format!("{} {}", post.name, first.map_or("", |m| m.content.as_str())).to_lowercase();
    words.iter().all(|w| haystack.contains(w.as_str()))
}

// Recently Active: the last message, or the post itself; Date Posted: the post's own id.
pub(super) fn sort_key(post: &Channel, by_creation: bool) -> u64 {
    match by_creation {
        true => snowflake(&post.id),
        false => post.last_message_id.as_deref().map_or_else(|| snowflake(&post.id), snowflake),
    }
}

pub(super) fn snowflake(id: &str) -> u64 {
    id.parse().unwrap_or(0)
}

pub(super) fn snowflake_ms(id: &str) -> i64 {
    (snowflake(id) >> 22) as i64 + DISCORD_EPOCH
}

pub(super) fn ago(id: &str) -> String {
    crate::panel::ago(chrono::Utc::now().timestamp_millis() - snowflake_ms(id))
}

pub(super) fn post_age(post: &Channel) -> String {
    ago(&post.id)
}

// The card's time: last activity, or the post's age when sorted by Date Posted. Its tooltip says when it was posted.
pub(super) fn post_time(post: &Channel, by_creation: bool) -> Stateful<Div> {
    let since = if by_creation { post.id.as_str() } else { post.last_message_id.as_deref().unwrap_or(&post.id) };
    let posted = chrono::DateTime::from_timestamp_millis(snowflake_ms(&post.id)).map(|t| t.with_timezone(&chrono::Local).format("%-d %B %Y %H:%M").to_string()).unwrap_or_default();
    div().id(SharedString::from(format!("post-time-{}", post.id))).child(ago(since)).tooltip(move |window, cx| ui::Tooltip::new(format!("Posted {posted}")).build(window, cx))
}

// The first message's pictures, four at most: its images and videos' posters, else an embed's thumbnail.
pub(super) fn post_media(first: &Message) -> Vec<String> {
    let attached: Vec<String> = first
        .media
        .iter()
        .take(MOSAIC_MAX)
        .map(|media| match &media.kind {
            MediaKind::Image => media.url.clone(),
            MediaKind::Video { poster } | MediaKind::Gif { poster } => poster.clone(),
        })
        .collect();
    if !attached.is_empty() {
        return attached;
    }
    first.embeds.iter().find_map(|e| e.images.first().or(e.thumbnail.as_ref())).map(|m| m.url.clone()).into_iter().collect()
}

// How a card lays out its pictures: one, two side by side, one beside a stack of two, or two by two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mosaic {
    One,
    Pair,
    OneTwo,
    Grid,
}

impl Mosaic {
    pub(super) fn of(count: usize) -> Option<Self> {
        match count {
            0 => None,
            1 => Some(Self::One),
            2 => Some(Self::Pair),
            3 => Some(Self::OneTwo),
            _ => Some(Self::Grid),
        }
    }
}

pub(super) fn mosaic_view(urls: &[String], height: f32, gap: f32, cx: &Context<Waffle>) -> Option<Div> {
    let layout = Mosaic::of(urls.len())?;
    let radius = if layout == Mosaic::One { ui::RADIUS_MD } else { ui::RADIUS_XS };
    let fill = palette::selected(cx);
    let tile = |url: &String| div().flex_1().min_w_0().min_h_0().child(ui::media(url.clone(), ui::all(radius)).size_full().bg(fill).object_fit(gpui_kit::ObjectFit::Cover));
    let row = |urls: &[String]| ui::row().items_stretch().flex_1().min_h_0().gap(px(gap)).children(urls.iter().map(tile));
    let frame = ui::row().items_stretch().h(px(height)).gap(px(gap));
    Some(match layout {
        Mosaic::One | Mosaic::Pair => frame.children(urls.iter().map(tile)),
        Mosaic::OneTwo => frame.child(tile(&urls[0])).child(ui::col().min_h_0().flex_1().gap(px(gap)).children(urls[1..3].iter().map(tile))),
        Mosaic::Grid => frame.flex_col().child(row(&urls[0..2])).child(row(&urls[2..4])),
    })
}

// Discord's "(N New)" count: capped at 25+, and 1+ while the count is unknown.
pub(super) fn new_count_label(count: Option<u32>) -> String {
    match count {
        Some(n) if n >= UNREAD_CAP => format!("{UNREAD_CAP}+"),
        Some(n) if n > 0 => n.to_string(),
        _ => "1+".to_owned(),
    }
}

pub(super) fn new_posts_label(count: usize) -> String {
    match count {
        1 => "1 New Post".to_owned(),
        n => format!("{n} New Posts"),
    }
}

pub(super) fn guidelines_text(forum: &Channel) -> Option<&str> {
    forum.topic.as_deref().map(str::trim).filter(|t| !t.is_empty())
}

// Slowmode's clock: m:ss, or h:mm:ss from an hour.
pub(super) fn countdown(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    match h {
        0 => format!("{m}:{s:02}"),
        _ => format!("{h}:{m:02}:{s:02}"),
    }
}

// The most-used reaction on the opening message, else the forum's default reaction with no count.
// GAP: Discord's burst (super) reactions aren't parsed, so only normal counts rank.
pub(super) fn top_reaction(first: Option<&Message>, forum: &Channel) -> Option<(Emoji, Option<u32>)> {
    let top = first.and_then(|m| m.reactions.iter().max_by_key(|r| r.count));
    match top {
        Some(r) => Some((r.emoji.clone(), Some(r.count))),
        None => forum.props.default_reaction.clone().map(|e| (e, None)),
    }
}

// Title, two lines at most; search words light up.
pub(super) fn post_title(post: &Channel, words: &[String], ink: gpui_kit::Hsla, size: f32, cx: &Context<Waffle>) -> Div {
    let hit = HighlightStyle { background_color: Some(palette::accent_soft(cx)), color: Some(palette::accent_ink(cx)), ..Default::default() };
    let ranges = hits(&post.name, words);
    // lint-allow(raw-truncate): ui::line clamps to one line; a post's title clamps to two
    div().line_clamp(2).text_ellipsis().text_size(px(size)).font_weight(FontWeight::MEDIUM).text_color(ink).child(StyledText::new(post.name.clone()).with_highlights(ranges.into_iter().map(|r| (r, hit))))
}

// Byte ranges of each search word in `text`, case-insensitively; none where lowercasing changes lengths.
pub(super) fn hits(text: &str, words: &[String]) -> Vec<Range<usize>> {
    let lower = text.to_lowercase();
    if lower.len() != text.len() {
        return Vec::new();
    }
    let mut ranges: Vec<Range<usize>> = words.iter().flat_map(|w| lower.match_indices(w.as_str()).map(|(at, m)| at..at + m.len()).collect::<Vec<_>>()).collect();
    ranges.sort_by_key(|r| r.start);
    ranges.dedup_by(|next, kept| next.start < kept.end);
    ranges
}

// Discord's media layout: columns = max(floor(t + (w/320 − t)/2), 1) where t = w/450.
pub(super) fn gallery_columns(width: f32) -> usize {
    let t = width / 450.;
    ((t + (width / 320. - t) / 2.).floor() as usize).max(1)
}

// `has_message`: text or files.
pub(super) fn post_error(name: &str, has_message: bool, missing_tag: bool) -> Option<&'static str> {
    match () {
        _ if name.is_empty() => Some("A title is required"),
        _ if !has_message => Some("A message is required"),
        _ if missing_tag => Some("A tag is required to create a post in this channel"),
        _ => None,
    }
}

// Discord's create-post body; files go alongside as `files[n]`, each named here by its index.
pub(super) fn post_body(name: &str, content: &str, tags: &[String], files: &[crate::discord::Upload], archive_after: u32) -> Value {
    let mut message = json!({ "content": content });
    if !files.is_empty() {
        message["attachments"] = files.iter().enumerate().map(|(i, f)| attachment_entry(i, f)).collect();
    }
    json!({ "name": name, "auto_archive_duration": archive_after, "applied_tags": tags, "message": message })
}

// Discord's serializer, as for chat uploads: alt text only when set, `is_spoiler` only when true.
pub(super) fn attachment_entry(index: usize, file: &crate::discord::Upload) -> Value {
    let mut entry = json!({ "id": index.to_string(), "filename": file.name });
    if let Some(description) = &file.description {
        entry["description"] = json!(description);
    }
    if file.spoiler {
        entry["is_spoiler"] = json!(true);
    }
    entry
}

// A file waiting in the composer: its kind, name and size; callers add the remove button.
// lint-allow(custom-component): ui::Toggle is a pressable filter; this is a static file label with its size
pub(super) fn file_chip(file: &PendingFile, cx: &Context<Waffle>) -> Div {
    let glyph = match file.kind() {
        FileKind::Image => Glyph::Image,
        FileKind::Video => Glyph::Video,
        FileKind::Other => Glyph::File,
    };
    // lint-allow(custom-badge): ui::Badge is a 20px solid label; this is a 32px file row with an icon and its size
    ui::row()
        .h_8()
        .pl_2p5()
        .pr_0p5()
        .gap_1p5()
        .squircle(ui::RADIUS_SM)
        .bg(palette::selected(cx))
        .text_size(ui::TEXT_DETAIL)
        .text_color(palette::text(cx))
        .child(Icon::new(glyph).size(ui::ICON_SM).text_color(palette::muted(cx)))
        .child(ui::line(file.name.clone()).max_w(px(FILE_NAME_WIDTH)))
        .child(div().flex_none().text_color(palette::faint(cx)).child(crate::chat::format_size(file.size)))
}

pub(super) fn slowmode_clock(secs: u64, cx: &Context<Waffle>) -> Stateful<Div> {
    ui::row()
        .id("new-post-slowmode")
        .gap_1()
        .text_size(ui::TEXT_XS)
        .text_color(palette::muted(cx))
        .child(Icon::new(Glyph::Timer).size(ui::ICON_SM))
        .child(countdown(secs))
        .tooltip(ui::Tooltip::text("Slowmode is enabled"))
}

pub(super) fn tag_chip(id: SharedString, tag: &ForumTag, on: bool) -> ui::Toggle {
    ui::Toggle::new(id).pressed(on).when_some(tag.emoji.as_ref().map(|e| emoji_face(e, 16.)), |t, l| t.lead(l)).child(tag.name.clone())
}

pub(super) fn tag_item(tag: &ForumTag, act: impl Fn(&mut Waffle, &mut Window, &mut Context<Waffle>) + 'static) -> crate::menu::Item {
    let row = item(tag.name.clone(), act);
    match tag.emoji.clone() {
        Some(emoji) => row.emoji(emoji),
        None => row,
    }
}

// A tag on a card; one being filtered by takes the accent.
pub(super) fn post_tag_badge(emoji: Option<&Emoji>, name: impl Into<SharedString>, hit: bool, cx: &Context<Waffle>) -> ui::Badge {
    let (fill, ink) = if hit { (palette::accent_soft(cx), palette::accent_ink(cx)) } else { (palette::selected(cx), palette::text(cx)) };
    ui::Badge::empty().colors(fill, ink).h(px(TAG_HEIGHT)).gap_1().children(emoji.map(|e| emoji_face(e, 14.))).child(name.into())
}

pub(super) fn reaction_chip(emoji: Emoji, count: Option<u32>) -> ui::Badge {
    ui::Badge::empty().variant(ui::BadgeVariant::Secondary).h(px(TAG_HEIGHT)).gap_1().child(emoji_face(&emoji, 14.)).children(count.map(|c| c.to_string()))
}

pub(super) fn no_history_notice(forum: &Channel, cx: &Context<Waffle>) -> Div {
    let name = format!("#{}", forum.name);
    let text = format!("You do not have permission to view the post history of {name}.");
    let bold = HighlightStyle { font_weight: Some(FontWeight::SEMIBOLD), color: Some(palette::strong(cx)), ..Default::default() };
    let at = text.find(&name).unwrap_or(0);
    ui::col()
        .gap_2()
        .child(ui::Separator::new().label("Older Posts"))
        .child(div().text_center().text_size(ui::TEXT_SM).text_color(palette::muted(cx)).child(StyledText::new(text.clone()).with_highlights([(at..at + name.len(), bold)])))
}

// A post card still loading: title, two lines of its opening message, and the foot.
pub(super) fn post_skeleton(i: usize, cx: &gpui_kit::App) -> AnyElement {
    let line = |part: &str, width: f32| ui::Skeleton::new(SharedString::from(format!("post-skel-{part}-{i}"))).radius(4.).h_3().w(gpui_kit::relative(width));
    ui::col()
        .p_4()
        .gap_2p5()
        .squircle(ui::RADIUS_MD)
        .bg(palette::surface(Level::Sidebar, cx))
        .child(line("title", [0.5, 0.35, 0.6, 0.42][i % 4]).h_4())
        .child(line("body", 0.9))
        .child(line("body2", 0.7))
        .child(line("foot", 0.25))
        .into_any_element()
}
