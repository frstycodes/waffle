// Discord's forum and media channels (docs/discord-forums-re.md): a list or gallery of posts (each a
// thread). Active posts arrive over the gateway once the channel is subscribed with threads on; older
// ones come from THREAD_SEARCH (archived, by last message time, 25 at a time); each card's preview is
// the post's first message from FORUM_POSTS, fetched ten at a time as Discord does. Search, tag
// filters and Sort & View live here for the session, per forum, like Discord's.
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    ops::Range,
    path::PathBuf,
    time::{Duration, Instant},
};

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    AnyElement, Context, Div, Entity, Focusable, FontWeight, HighlightStyle, KeyDownEvent, MouseButton, SharedString, Stateful, StyledText, Window, WindowControlArea,
    component::{
        Icon,
        input::{Input, InputEvent, InputState, Textarea, TextareaState},
        text::TextView,
    },
    div,
    prelude::*,
    px,
};
use reqwest::Method;
use serde_json::{Value, json};

use crate::{
    Waffle,
    attachments::{FileKind, PendingFile},
    discord::{Channel, ChannelKind, Emoji, ForumTag, MediaKind, Message, parse_message, parse_thread, perm},
    icons::Glyph,
    mascot::Mood,
    menu::{Entry, emoji_face, item, label, submenu},
    palette::{self, Level},
    ui::{self, Squircle},
};

// Discord's limits: tags on a post, title length, search debounce, posts per unread-count request, the
// "(N New)" cap, the guidelines' height before Expand, and pictures in a card's mosaic.
const MAX_TAGS: usize = 5;
const MAX_UNREAD_REQUEST: usize = 180;
const UNREAD_CAP: u32 = 25;
const GUIDELINES_HEIGHT: f32 = 220.;
const MOSAIC_MAX: usize = 4;
const MAX_TITLE: usize = 100;
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(350);
// default_auto_archive_duration's fallback: 3 days, in minutes.
const ARCHIVE_AFTER: u32 = 4320;
const MEDIA_CHANNEL: u8 = 16;
const DISCORD_EPOCH: i64 = 1_420_070_400_000;

#[derive(Default)]
pub struct ForumState {
    archived: Vec<Channel>,
    previews: HashMap<String, Message>,
    requested: HashSet<String>,
    has_more: bool,
    loading: bool,
    error: Option<SharedString>,
    // Sort & View overrides; `None` follows the forum's defaults.
    view: Option<ForumView>,
    filter: Vec<String>,
    search: Option<Entity<InputState>>,
    // Bumped on each change; a debounced search runs only while it's still the latest.
    pending: u64,
    found: Option<Found>,
    composer: Option<Composer>,
    // FORUM_UNREADS counts, and the ack each post was last asked about.
    unread_counts: HashMap<String, u32>,
    unreads_asked: HashMap<String, u64>,
    // Other people's new posts, kept out of the list until "N New Posts" resorts it.
    held_back: Vec<String>,
    guidelines_seen: bool,
    cooldown_until: Option<Instant>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ForumView {
    by_creation: bool,
    gallery: bool,
    match_all: bool,
}

impl ForumView {
    fn of(forum: &Channel) -> Self {
        Self { by_creation: forum.props.sort_by_creation, gallery: forum.props.gallery, match_all: forum.props.match_all }
    }
}

// Discord's thread search for one query and tag filter.
struct Found {
    key: SearchKey,
    posts: Vec<Channel>,
    loading: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SearchKey {
    query: String,
    tags: Vec<String>,
    match_all: bool,
}

pub struct Composer {
    title: Entity<InputState>,
    body: Entity<TextareaState>,
    tags: Vec<String>,
    files: Vec<PendingFile>,
    guidelines: bool,
    guidelines_full: bool,
    previewing: bool,
    error: Option<SharedString>,
    sending: bool,
}

// What the forum shows: active posts (pinned first) and, unless searching by text, Older Posts.
#[derive(Debug, Default)]
struct Listing {
    active: Vec<Channel>,
    older: Vec<Channel>,
}

// Everything a render needs, derived once.
struct ForumPage {
    view: ForumView,
    media: bool,
    filter: Vec<String>,
    words: Vec<String>,
    query: String,
    listing: Listing,
    can_post: bool,
    history: bool,
    settled: bool,
    searching: bool,
}

impl Waffle {
    // The forum's posts we know of: active ones from the gateway, then archived ones, unsorted.
    fn forum_posts(&self, forum_id: &str) -> Vec<Channel> {
        let mut posts: Vec<Channel> = self.guilds.iter().flat_map(|g| g.threads.iter()).filter(|t| t.parent_id.as_deref() == Some(forum_id)).cloned().collect();
        if let Some(state) = self.forums.get(forum_id) {
            posts.extend(state.archived.iter().filter(|a| !posts.iter().any(|p| p.id == a.id)).cloned().collect::<Vec<_>>());
        }
        posts
    }

    fn forum_channel(&self, forum_id: &str) -> Option<&Channel> {
        self.guilds.iter().find_map(|g| g.channel(forum_id)).or(self.channel.as_ref().filter(|c| c.id == forum_id))
    }

    fn forum_view(&self, forum: &Channel) -> ForumView {
        self.forums.get(&forum.id).and_then(|s| s.view).unwrap_or_else(|| ForumView::of(forum))
    }

    // A post wherever we hold it: the server's threads, a forum's archived posts or its search results.
    pub(crate) fn forum_post(&self, post_id: &str) -> Option<Channel> {
        let threads = self.guilds.iter().flat_map(|g| g.threads.iter());
        let held = self.forums.values().flat_map(|s| s.archived.iter().chain(s.found.iter().flat_map(|f| f.posts.iter())));
        threads.chain(held).find(|p| p.id == post_id).cloned()
    }

    fn update_post(&mut self, post_id: &str, change: impl Fn(&mut Channel)) {
        let threads = self.guilds.iter_mut().flat_map(|g| g.threads.iter_mut());
        let held = self.forums.values_mut().flat_map(|s| s.archived.iter_mut().chain(s.found.iter_mut().flat_map(|f| f.posts.iter_mut())));
        threads.chain(held).chain(self.channel.as_mut()).filter(|p| p.id == post_id).for_each(&change);
    }

    pub(crate) fn load_forum(&mut self, forum_id: String, cx: &mut Context<Self>) {
        self.request_forum_unreads(&forum_id);
        let history = self.can_in(&forum_id, perm::READ_MESSAGE_HISTORY);
        let Some(client) = self.client.clone() else { return };
        let state = self.forums.entry(forum_id.clone()).or_default();
        if state.loading || !history {
            return;
        }
        state.loading = true;
        let offset = state.archived.len();
        let guild_id = self.guild_id.clone().unwrap_or_default();
        cx.spawn(async move |this, cx| {
            let path = format!("/channels/{forum_id}/threads/search?archived=true&sort_by=last_message_time&sort_order=desc&limit=25&offset={offset}");
            let result = client.call(Method::GET, path, None).await;
            this.update(cx, |this, cx| {
                let state = this.forums.entry(forum_id.clone()).or_default();
                state.loading = false;
                match result {
                    Ok(body) => {
                        state.archived.extend(body["threads"].as_array().into_iter().flatten().filter_map(|t| parse_thread(t, &guild_id)));
                        state.has_more = body["has_more"].as_bool().unwrap_or(false);
                        insert_previews(state, &body);
                        state.error = None;
                    }
                    Err(error) => state.error = Some(error.into()),
                }
                this.load_forum_previews(&forum_id, cx);
                this.request_forum_unreads(&forum_id);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // Unread counts for the posts we've read before that have newer messages, each asked once per ack.
    // Not verified: which posts Discord's client includes; this sends the ones with a read state behind.
    fn request_forum_unreads(&mut self, forum_id: &str) {
        let Some(client) = self.client.clone() else { return };
        let guild_id = self.guild_id.clone().unwrap_or_default();
        let behind: Vec<(String, u64)> = self.forum_posts(forum_id).iter().filter_map(|p| Some((p.id.clone(), self.read_behind(p)?))).collect();
        let state = self.forums.entry(forum_id.to_owned()).or_default();
        let threads: Vec<(String, u64)> = behind.into_iter().filter(|(id, ack)| state.unreads_asked.get(id) != Some(ack)).take(MAX_UNREAD_REQUEST).collect();
        if threads.is_empty() {
            return;
        }
        state.unreads_asked.extend(threads.iter().cloned());
        client.request_forum_unreads(&guild_id, forum_id, &threads);
    }

    // Our ack in a post whose latest message is past it.
    fn read_behind(&self, post: &Channel) -> Option<u64> {
        let ack = self.read_states.get(&post.id)?.last_read;
        (post.last_message_id.as_deref().map_or(0, snowflake) > ack).then_some(ack)
    }

    pub(crate) fn on_forum_unreads(&mut self, forum_id: &str, counts: Vec<(String, u32)>) {
        if let Some(state) = self.forums.get_mut(forum_id) {
            state.unread_counts.extend(counts);
        }
    }

    // Someone else's new post waits behind "N New Posts"; our own go straight in.
    pub(crate) fn on_forum_post_created(&mut self, forum_id: &str, post_id: String, owner_id: &str) {
        if owner_id == self.my_id() {
            return;
        }
        let Some(state) = self.forums.get_mut(forum_id) else { return };
        if !state.held_back.contains(&post_id) {
            state.held_back.push(post_id);
        }
    }

    fn show_held_back(&mut self, forum_id: &str, cx: &mut Context<Self>) {
        if let Some(state) = self.forums.get_mut(forum_id) {
            state.held_back.clear();
        }
        cx.notify();
    }

    // First messages for posts that don't have one yet, ten per request.
    fn load_forum_previews(&mut self, forum_id: &str, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let posts = self.forum_posts(forum_id);
        let state = self.forums.entry(forum_id.to_owned()).or_default();
        let found = state.found.iter().flat_map(|f| f.posts.iter());
        let wanted: Vec<String> = posts.iter().chain(found).map(|p| p.id.clone()).filter(|id| !state.previews.contains_key(id) && !state.requested.contains(id)).collect();
        for batch in wanted.chunks(10) {
            state.requested.extend(batch.iter().cloned());
            let (client, forum, ids) = (client.clone(), forum_id.to_owned(), batch.to_vec());
            cx.spawn(async move |this, cx| {
                let Ok(body) = client.call(Method::POST, format!("/channels/{forum}/post-data"), Some(json!({ "thread_ids": ids }))).await else { return };
                this.update(cx, |this, cx| {
                    let state = this.forums.entry(forum.clone()).or_default();
                    for (id, post) in body["threads"].as_object().into_iter().flatten() {
                        if let Some(first) = parse_message(&post["first_message"]) {
                            state.previews.insert(id.clone(), first);
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    // The search box, made the first time the forum draws.
    fn ensure_forum_search(&mut self, forum_id: &str, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(search) = self.forums.get(forum_id).and_then(|s| s.search.clone()) {
            return search;
        }
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search posts").validate(|text, _| text.chars().count() <= MAX_TITLE));
        let id = forum_id.to_owned();
        cx.subscribe(&search, move |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.queue_forum_search(&id, cx);
            }
        })
        .detach();
        self.forums.entry(forum_id.to_owned()).or_default().search = Some(search.clone());
        search
    }

    fn forum_query(&self, forum_id: &str, cx: &Context<Self>) -> String {
        self.forums.get(forum_id).and_then(|s| s.search.as_ref()).map(|s| s.read(cx).value().trim().to_owned()).unwrap_or_default()
    }

    // Local matching shows at once; Discord's search follows after the debounce.
    fn queue_forum_search(&mut self, forum_id: &str, cx: &mut Context<Self>) {
        let state = self.forums.entry(forum_id.to_owned()).or_default();
        state.pending += 1;
        let (pending, id) = (state.pending, forum_id.to_owned());
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_DEBOUNCE).await;
            this.update(cx, |this, cx| {
                if this.forums.get(&id).is_some_and(|s| s.pending == pending) {
                    this.search_forum(&id, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn search_forum(&mut self, forum_id: &str, cx: &mut Context<Self>) {
        let Some(forum) = self.forum_channel(forum_id).cloned() else { return };
        let key = SearchKey { query: self.forum_query(forum_id, cx), tags: self.forums.get(forum_id).map(|s| s.filter.clone()).unwrap_or_default(), match_all: self.forum_view(&forum).match_all };
        let history = self.can_in(forum_id, perm::READ_MESSAGE_HISTORY);
        let client = self.client.clone();
        let state = self.forums.entry(forum_id.to_owned()).or_default();
        if key.query.is_empty() && key.tags.is_empty() {
            state.found = None;
            return cx.notify();
        }
        if state.found.as_ref().is_some_and(|f| f.key == key) {
            return;
        }
        let Some(client) = client.filter(|_| history) else { return };
        state.found = Some(Found { key: key.clone(), posts: Vec::new(), loading: true });
        cx.notify();
        let (guild_id, id) = (self.guild_id.clone().unwrap_or_default(), forum_id.to_owned());
        cx.spawn(async move |this, cx| {
            let result = client.call(Method::GET, search_path(&id, &key), None).await;
            this.update(cx, |this, cx| {
                let Some(state) = this.forums.get_mut(&id) else { return };
                let Some(found) = state.found.as_mut().filter(|f| f.key == key) else { return };
                found.loading = false;
                let Ok(body) = result else { return cx.notify() };
                found.posts = body["threads"].as_array().into_iter().flatten().filter_map(|t| parse_thread(t, &guild_id)).collect();
                insert_previews(state, &body);
                this.load_forum_previews(&id, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn toggle_forum_filter(&mut self, forum_id: &str, tag_id: &str, cx: &mut Context<Self>) {
        let state = self.forums.entry(forum_id.to_owned()).or_default();
        match state.filter.iter().position(|t| t == tag_id) {
            Some(at) => drop(state.filter.remove(at)),
            None => state.filter.push(tag_id.to_owned()),
        }
        self.queue_forum_search(forum_id, cx);
    }

    fn clear_forum_filter(&mut self, forum_id: &str, cx: &mut Context<Self>) {
        self.forums.entry(forum_id.to_owned()).or_default().filter.clear();
        self.queue_forum_search(forum_id, cx);
    }

    fn change_forum_view(&mut self, forum_id: &str, change: impl Fn(&mut ForumView), cx: &mut Context<Self>) {
        let Some(forum) = self.forum_channel(forum_id) else { return };
        let mut view = self.forum_view(forum);
        change(&mut view);
        self.forums.entry(forum_id.to_owned()).or_default().view = Some(view);
        self.queue_forum_search(forum_id, cx);
    }

    fn reset_forum_view(&mut self, forum_id: &str, cx: &mut Context<Self>) {
        self.forums.entry(forum_id.to_owned()).or_default().view = None;
        self.queue_forum_search(forum_id, cx);
    }

    fn forum_page(&self, forum: &Channel, cx: &Context<Self>) -> ForumPage {
        let state = self.forums.get(&forum.id);
        let view = self.forum_view(forum);
        let query = self.forum_query(&forum.id, cx);
        let filter = state.map(|s| s.filter.clone()).unwrap_or_default();
        let empty = HashMap::new();
        let previews = state.map_or(&empty, |s| &s.previews);
        let found = state.and_then(|s| s.found.as_ref());
        let held = state.map_or(&[][..], |s| s.held_back.as_slice());
        let posts = self.forum_posts(&forum.id).into_iter().filter(|p| !held.contains(&p.id)).collect();
        let listing = arrange(posts, found.map_or(&[][..], |f| f.posts.as_slice()), view, &filter, &query, previews);
        ForumPage {
            view,
            media: forum.raw_type == MEDIA_CHANNEL,
            words: words(&query),
            filter,
            query,
            listing,
            can_post: self.can_in(&forum.id, perm::SEND_MESSAGES),
            history: self.can_in(&forum.id, perm::READ_MESSAGE_HISTORY),
            settled: state.is_some_and(|s| !s.loading),
            searching: found.is_some_and(|f| f.loading),
        }
    }

    // Discord's "New" badge: a post started since we last read the forum that we haven't opened.
    // Not verified: Discord's exact rule (ForumPostUnreadStore) wasn't read; this follows its read states.
    fn is_new_post(&self, forum_id: &str, post: &Channel) -> bool {
        let Some(forum_read) = self.read_states.get(forum_id) else { return false };
        let mine = post.thread.as_ref().and_then(|t| t.owner_id.as_deref()) == Some(self.my_id());
        !mine && !self.read_states.contains_key(&post.id) && snowflake(&post.id) > forum_read.last_read
    }

    // "(N New)" for a post with messages past our ack: FORUM_UNREADS' count, or "1+" before it answers.
    fn post_unread(&self, forum_id: &str, post: &Channel) -> Option<String> {
        self.read_behind(post)?;
        let count = self.forums.get(forum_id).and_then(|s| s.unread_counts.get(&post.id)).copied();
        Some(new_count_label(count))
    }

    // Discord mutes a card's title once there's nothing unread in it.
    fn title_ink(&self, forum_id: &str, post: &Channel, cx: &Context<Self>) -> gpui_kit::Hsla {
        match self.is_new_post(forum_id, post) || self.read_behind(post).is_some() {
            true => palette::strong(cx),
            false => palette::muted(cx),
        }
    }

    pub(crate) fn render_forum(&mut self, forum: Channel, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let search = self.ensure_forum_search(&forum.id, window, cx);
        let page = self.forum_page(&forum, cx);
        let toolbar = self.forum_toolbar(&forum, &page, &search, cx);
        let body = self.forum_body(&forum, &page, window, cx);
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(palette::surface(Level::Page, cx))
            .child(self.forum_header(&forum, cx))
            .child(toolbar)
            .child(div().id("forum-scroll").flex_1().min_h_0().overflow_y_scrollbar().child(body))
            .into_any_element()
    }

    fn forum_header(&self, forum: &Channel, cx: &mut Context<Self>) -> Div {
        let (strong, faint) = (palette::strong(cx), palette::faint(cx));
        div()
            .h(px(48.))
            .flex_none()
            .pl(px(12.))
            .pr(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .border_b_1()
            .border_color(palette::line(cx))
            .window_control_area(WindowControlArea::Drag)
            .child(
                div()
                    .h(px(32.))
                    .px(px(6.))
                    .flex_none()
                    .max_w(gpui_kit::relative(0.6))
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(strong)
                    .child(Icon::new(Glyph::Forum).size(px(20.)).text_color(faint))
                    .child(crate::ui::truncating("header-forum-name", forum.name.clone())),
            )
            .children(forum.topic.clone().map(|t| div().flex_1().min_w_0().pl(px(12.)).border_l_1().border_color(palette::line_strong(cx)).truncate().text_size(px(14.)).text_color(faint).child(t)))
            .child(div().ml_auto().child(self.caption_clearance(true)))
    }

    // The search box, Sort & View and New post; under them the search status and the tag row.
    fn forum_toolbar(&self, forum: &Channel, page: &ForumPage, search: &Entity<InputState>, cx: &mut Context<Self>) -> Div {
        let id = forum.id.clone();
        let sort = ui::button("forum-sort", ui::Tone::Ghost, cx)
            .child(Icon::new(Glyph::SortDesc).size(px(16.)))
            .child(if page.media { "Sort" } else { "Sort & View" })
            .on_click(cx.listener(move |this, _, window, cx| {
                let id = id.clone();
                this.open_live_menu(window.mouse_position(), move |this| this.sort_view_entries(&id), cx);
            }));
        let new_post = page.can_post.then(|| self.new_post_button("forum-new-post", &forum.id, cx));
        let guidelines = (page.can_post && guidelines_text(forum).is_some()).then(|| {
            let id = forum.id.clone();
            ui::icon_button("forum-guidelines", Glyph::Book, "Post Guidelines", cx).on_click(cx.listener(move |this, _, window, cx| this.open_guidelines(id.clone(), window, cx)))
        });
        div()
            .flex_none()
            .w_full()
            .max_w(px(880.))
            .mx_auto()
            .px(px(16.))
            .pt(px(16.))
            .pb(px(4.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(div().flex().items_center().gap(px(8.)).child(div().flex_1().min_w_0().child(ui::search_field("forum-search", search, cx))).child(sort).children(guidelines).children(new_post))
            .children(self.search_status(forum, page, cx))
            .children((!forum.props.tags.is_empty()).then(|| self.tag_row(forum, &page.filter, cx)))
    }

    fn new_post_button(&self, key: &'static str, forum_id: &str, cx: &mut Context<Self>) -> ui::Control {
        let id = forum_id.to_owned();
        ui::button(key, ui::Tone::Primary, cx).child(Icon::new(Glyph::Plus).size(px(16.))).child("New post").on_click(cx.listener(move |this, _, window, cx| this.open_new_post(id.clone(), window, cx)))
    }

    // Discord's line under the search: "Searching...", "No matching posts" or the count, then Clear.
    fn search_status(&self, forum: &Channel, page: &ForumPage, cx: &mut Context<Self>) -> Option<Div> {
        if page.query.is_empty() {
            return None;
        }
        let count = page.listing.active.len() + page.listing.older.len();
        let text = match count {
            0 if page.searching => "Searching...".to_owned(),
            0 => "No matching posts".to_owned(),
            1 => format!("1 post with \u{201c}{}\u{201d}", page.query),
            n => format!("{n} posts with \u{201c}{}\u{201d}", page.query),
        };
        let id = forum.id.clone();
        let clear = ui::button("forum-search-clear", ui::Tone::Link, cx).child("Clear").on_click(cx.listener(move |this, _, window, cx| {
            if let Some(search) = this.forums.get(&id).and_then(|s| s.search.clone()) {
                search.update(cx, |s, cx| s.set_value("", window, cx));
            }
            this.queue_forum_search(&id, cx);
        }));
        Some(div().flex().items_center().gap(px(8.)).text_size(px(14.)).text_color(palette::muted(cx)).child(text).child(clear))
    }

    // One clipped line of tag chips, then "All" or "N selected", which opens every tag.
    fn tag_row(&self, forum: &Channel, filter: &[String], cx: &mut Context<Self>) -> Div {
        let chips: Vec<AnyElement> = forum
            .props
            .tags
            .iter()
            .map(|tag| {
                let (forum_id, tag_id) = (forum.id.clone(), tag.id.clone());
                tag_chip(SharedString::from(format!("forum-tag-{}", tag.id)), tag, filter.contains(&tag.id), cx)
                    .flex_none()
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_forum_filter(&forum_id, &tag_id, cx)))
                    .into_any_element()
            })
            .collect();
        let id = forum.id.clone();
        let all = ui::button("forum-tags-all", ui::Tone::Ghost, cx)
            .small()
            .flex_none()
            .child(if filter.is_empty() { "All".to_owned() } else { format!("{} selected", filter.len()) })
            .child(Icon::new(Glyph::ChevronDown).size(px(14.)))
            .on_click(cx.listener(move |this, _, window, cx| {
                let id = id.clone();
                this.open_live_menu(window.mouse_position(), move |this| this.tag_filter_entries(&id), cx);
            }));
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .min_w_0()
            .child(div().flex().gap(px(6.)).min_w_0().overflow_hidden().children(chips))
            .child(div().flex_none().w(px(1.)).h(px(20.)).mx(px(2.)).bg(palette::line_strong(cx)))
            .child(all)
    }

    // Select Tags: every tag you could pick (moderated ones only for Manage Threads), then Clear all.
    fn tag_filter_entries(&self, forum_id: &str) -> Vec<Entry> {
        let Some(forum) = self.forum_channel(forum_id) else { return Vec::new() };
        let filter = self.forums.get(forum_id).map(|s| s.filter.clone()).unwrap_or_default();
        let manage = self.can_in(forum_id, perm::MANAGE_THREADS);
        let mut out = vec![label(if filter.is_empty() { "Select Tags".to_owned() } else { format!("Select Tags ({})", filter.len()) })];
        out.extend(forum.props.tags.iter().filter(|t| manage || !t.moderated).map(|tag| {
            let (id, tag_id) = (forum_id.to_owned(), tag.id.clone());
            tag_item(tag, move |this, _, cx| this.toggle_forum_filter(&id, &tag_id, cx)).checked(filter.contains(&tag.id)).entry()
        }));
        out.push(Entry::Separator);
        let id = forum_id.to_owned();
        let clear = item("Clear all", move |this, _, cx| this.clear_forum_filter(&id, cx)).icon(Glyph::X);
        out.push(if filter.is_empty() { clear.disabled() } else { clear }.entry());
        out
    }

    // Sort & View: Discord's three groups and Reset to default. Media channels are always a gallery.
    fn sort_view_entries(&self, forum_id: &str) -> Vec<Entry> {
        let Some(forum) = self.forum_channel(forum_id) else { return Vec::new() };
        let view = self.forum_view(forum);
        let choice = |text: &'static str, glyph: Glyph, on: bool, change: fn(&mut ForumView)| {
            let id = forum_id.to_owned();
            item(text, move |this, _, cx| this.change_forum_view(&id, change, cx)).icon(glyph).radio(on)
        };
        let mut out = vec![
            label("Sort by"),
            choice("Recently Active", Glyph::Recent, !view.by_creation, |v| v.by_creation = false).entry(),
            choice("Date Posted", Glyph::Calendar, view.by_creation, |v| v.by_creation = true).entry(),
            Entry::Separator,
        ];
        if forum.raw_type != MEDIA_CHANNEL {
            out.extend([
                label("View as"),
                choice("List", Glyph::Rows, !view.gallery, |v| v.gallery = false).entry(),
                choice("Gallery", Glyph::Gallery, view.gallery, |v| v.gallery = true).entry(),
                Entry::Separator,
            ]);
        }
        if !forum.props.tags.is_empty() {
            out.extend([
                label("Tag Matching"),
                choice("Match Some", Glyph::Tag, !view.match_all, |v| v.match_all = false).hint("Posts with any picked tag").entry(),
                choice("Match All", Glyph::Filter, view.match_all, |v| v.match_all = true).hint("Posts with every picked tag").entry(),
                Entry::Separator,
            ]);
        }
        let id = forum_id.to_owned();
        out.push(item("Reset to default", move |this, _, cx| this.reset_forum_view(&id, cx)).icon(Glyph::Retry).entry());
        out
    }

    fn forum_body(&self, forum: &Channel, page: &ForumPage, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let state = self.forums.get(&forum.id);
        let error = state.and_then(|s| s.error.clone()).map(|e| div().text_size(px(14.)).text_color(palette::danger_ink(cx)).child(e));
        let more = state.filter(|s| s.has_more && !s.loading && page.words.is_empty()).map(|_| {
            let id = forum.id.clone();
            div().flex().justify_center().child(ui::button("forum-more", ui::Tone::Ghost, cx).child("Load more posts").on_click(cx.listener(move |this, _, _, cx| this.load_forum(id.clone(), cx))))
        });
        let is_empty = page.listing.active.is_empty() && page.listing.older.is_empty() && page.settled && !page.searching;
        let no_history = (!page.history && page.words.is_empty()).then(|| no_history_notice(forum, cx));
        let older = (!page.listing.older.is_empty()).then(|| ui::divider(Some("Older Posts"), cx));
        let held = state.map_or(0, |s| s.held_back.len());
        let new_posts = (held > 0).then(|| {
            let id = forum.id.clone();
            let button = ui::button("forum-new-posts", ui::Tone::Primary, cx).small().child(Icon::new(Glyph::ArrowUp).size(px(14.))).child(new_posts_label(held));
            div().flex().justify_center().child(button.on_click(cx.listener(move |this, _, _, cx| this.show_held_back(&id, cx))))
        });
        let column = div().w_full().max_w(px(880.)).mx_auto().px(px(16.)).pt(px(8.)).pb(px(16.)).flex().flex_col().gap(px(12.)).children(error).children(new_posts);
        if is_empty {
            return column.child(self.forum_empty(forum, page, cx)).children(no_history);
        }
        let (active, older_posts) = match page.view.gallery || page.media {
            true => (self.gallery(forum, page, &page.listing.active, "active", window, cx), self.gallery(forum, page, &page.listing.older, "older", window, cx)),
            false => (self.post_list(forum, page, &page.listing.active, cx), self.post_list(forum, page, &page.listing.older, cx)),
        };
        column.child(active).children(older).child(older_posts).children(no_history).children(more)
    }

    // No posts, none with these tags, or none matching the search.
    fn forum_empty(&self, forum: &Channel, page: &ForumPage, cx: &mut Context<Self>) -> Div {
        if !page.query.is_empty() {
            return ui::empty_state("forum-empty", Mood::Idle, "No matching posts", "Search looks at post titles and the opening messages loaded so far.", cx);
        }
        if !page.filter.is_empty() {
            let (title, body) = match page.filter.len() {
                1 => ("There are no posts with this tag yet!", "Be the first to start a conversation with this tag"),
                _ => ("There are no posts with these tags yet!", "Be the first to start a conversation with these tags"),
            };
            let id = forum.id.clone();
            let clear = ui::button("forum-empty-clear", ui::Tone::Secondary, cx).child("Clear all").on_click(cx.listener(move |this, _, _, cx| this.clear_forum_filter(&id, cx)));
            return ui::empty_state("forum-empty", Mood::Idle, title, body, cx).child(div().mt(px(8.)).child(clear));
        }
        let new_post = page.can_post.then(|| div().mt(px(8.)).child(self.new_post_button("forum-empty-new", &forum.id, cx)));
        ui::empty_state("forum-empty", Mood::Idle, "Make the first post!", "Be the first to start the conversation!", cx).children(new_post)
    }

    fn post_list(&self, forum: &Channel, page: &ForumPage, posts: &[Channel], cx: &mut Context<Self>) -> AnyElement {
        let previews = self.forums.get(&forum.id).map(|s| &s.previews);
        let cards: Vec<AnyElement> = posts.iter().map(|post| self.post_card(forum, page, post, previews.and_then(|p| p.get(&post.id)), cx)).collect();
        div().flex().flex_col().gap(px(8.)).children(cards).into_any_element()
    }

    // A list card: pin, New and tags, the title, the opening message, then reaction, replies and time.
    fn post_card(&self, forum: &Channel, page: &ForumPage, post: &Channel, first: Option<&Message>, cx: &mut Context<Self>) -> AnyElement {
        let meta = post.thread.clone().unwrap_or_default();
        let (strong, text, muted, faint) = (palette::strong(cx), palette::text(cx), palette::muted(cx), palette::faint(cx));
        let sidebar = palette::surface(Level::Sidebar, cx);
        let (edge, lifted) = (palette::line_strong(cx), ui::mix(sidebar, strong, 0.03));
        let excerpt = first.map(|m| {
            let name = format!("{}: ", m.author.name);
            let highlight = HighlightStyle { color: Some(text), font_weight: Some(FontWeight::MEDIUM), ..Default::default() };
            div().line_clamp(2).text_size(px(14.)).text_color(muted).child(StyledText::new(format!("{name}{}", m.content)).with_highlights([(0..name.len(), highlight)]))
        });
        let tags = self.card_tags(forum, page, post, cx);
        let unread = self.post_unread(&forum.id, post).map(|n| div().font_weight(FontWeight::SEMIBOLD).text_color(palette::accent_ink(cx)).child(format!("({n} New)")));
        let footer = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x(px(8.))
            .gap_y(px(4.))
            .text_size(px(12.))
            .text_color(faint)
            .children(top_reaction(first, forum).map(|(emoji, count)| reaction_chip(emoji, count, cx)))
            .child(div().flex().items_center().gap(px(4.)).child(Icon::new(Glyph::ChatRound).size(px(14.))).child(meta.message_count.to_string()).children(unread))
            .child("\u{2022}")
            .child(post_time(post, page.view.by_creation));
        let open = post.clone();
        let (menu_post, guild_id) = (post.clone(), self.guild_id.clone().unwrap_or_default());
        ui::pressable(div().id(SharedString::from(format!("post-{}", post.id))), 0.99)
            .py(px(14.))
            .px(px(16.))
            .flex()
            .gap(px(16.))
            .squircle(ui::RADIUS_LG)
            .bg(sidebar)
            .border_1()
            .border_color(palette::line(cx))
            .cursor_pointer()
            .hover(move |s| s.bg(lifted).border_color(edge))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .children(tags)
                    .child(post_title(post, &page.words, self.title_ink(&forum.id, post, cx), 16., cx))
                    .children(excerpt)
                    .child(footer),
            )
            .children(mosaic_view(&first.map(post_media).unwrap_or_default(), 72., 2., cx).map(|m| m.w(px(72.)).flex_none()))
            .on_click(cx.listener(move |this, _, window, cx| this.open_channel(open.clone(), window, cx)))
            .on_mouse_down(MouseButton::Right, cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_thread_menu(menu_post.clone(), guild_id.clone(), e.position, cx)))
            .into_any_element()
    }

    // "New", the pin, the first three tags (those filtered by take the accent) and "+N" naming the rest.
    fn card_tags(&self, forum: &Channel, page: &ForumPage, post: &Channel, cx: &mut Context<Self>) -> Option<Div> {
        let meta = post.thread.as_ref()?;
        let applied: Vec<&ForumTag> = meta.applied_tags.iter().filter_map(|id| forum.props.tags.iter().find(|t| &t.id == id)).collect();
        let is_new = self.is_new_post(&forum.id, post);
        if applied.is_empty() && !meta.pinned && !is_new {
            return None;
        }
        let (shown, rest) = applied.split_at(applied.len().min(3));
        let rest_names = rest.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", ");
        let pin = meta.pinned.then(|| div().id(SharedString::from(format!("post-pin-{}", post.id))).text_color(palette::muted(cx)).child(Icon::new(Glyph::Pin).size(px(14.))).tooltip(|window, cx| ui::tip("Pinned", window, cx)));
        let overflow = (!rest.is_empty()).then(|| {
            div().id(SharedString::from(format!("post-tags-more-{}", post.id))).child(post_tag_badge(None, format!("+{}", rest.len()), false, cx)).tooltip(move |window, cx| ui::tip(rest_names.clone(), window, cx))
        });
        Some(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(6.))
                .children(is_new.then(|| ui::badge(palette::accent(), palette::on_accent()).child("New")))
                .children(pin)
                .children(shown.iter().map(|t| post_tag_badge(t.emoji.as_ref(), t.name.clone(), page.filter.contains(&t.id), cx)))
                .children(overflow),
        )
    }

    // Discord's masonry: 320–450px columns 16px apart, each card going under the shortest column.
    fn gallery(&self, forum: &Channel, page: &ForumPage, posts: &[Channel], key: &str, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if posts.is_empty() {
            return div().into_any_element();
        }
        let measured = window.use_keyed_state(SharedString::from(format!("forum-gallery-{}-{key}", forum.id)), cx, |_, _| 0f32);
        let width = Some(*measured.read(cx)).filter(|w| *w > 0.).unwrap_or(848.);
        let columns = gallery_columns(width);
        let column_width = (width - 16. * (columns - 1) as f32) / columns as f32;
        let media_ratio = if page.media { 9. / 16. } else { 2. / 3. };
        let previews = self.forums.get(&forum.id).map(|s| &s.previews);
        let mut heights = vec![0f32; columns];
        let mut stacks: Vec<Vec<AnyElement>> = (0..columns).map(|_| Vec::new()).collect();
        for post in posts {
            let first = previews.and_then(|p| p.get(&post.id));
            let media = first.map(post_media).unwrap_or_default();
            let image_height = (!media.is_empty()).then_some((column_width - 28.) * media_ratio);
            let shortest = (0..columns).min_by(|a, b| heights[*a].total_cmp(&heights[*b])).unwrap_or(0);
            heights[shortest] += 60. + image_height.unwrap_or(100.) + 24. + 36.;
            stacks[shortest].push(self.gallery_card(forum, page, post, first, image_height.map(|h| (media, h)), cx));
        }
        let measure = measured.clone();
        div()
            .w_full()
            .child(div().w_full().flex().items_start().gap(px(16.)).children(stacks.into_iter().map(|cards| div().flex_1().min_w_0().flex().flex_col().gap(px(16.)).children(cards))))
            .on_children_prepainted(move |bounds, _, cx| {
                let Some(row) = bounds.first() else { return };
                let next = f32::from(row.size.width);
                measure.update(cx, |w, cx| {
                    if (*w - next).abs() > 0.5 {
                        *w = next;
                        cx.notify();
                    }
                });
            })
            .into_any_element()
    }

    // A gallery card: who and when, the title, then the post's picture or its text. Media channels
    // lead with the picture.
    fn gallery_card(&self, forum: &Channel, page: &ForumPage, post: &Channel, first: Option<&Message>, media: Option<(Vec<String>, f32)>, cx: &mut Context<Self>) -> AnyElement {
        let (strong, muted, faint) = (palette::strong(cx), palette::muted(cx), palette::faint(cx));
        let sidebar = palette::surface(Level::Sidebar, cx);
        let (edge, lifted) = (palette::line_strong(cx), ui::mix(sidebar, strong, 0.03));
        let replies = post.thread.as_ref().map_or(0, |t| t.message_count);
        let unread = self.post_unread(&forum.id, post).map(|n| div().font_weight(FontWeight::SEMIBOLD).text_color(palette::accent_ink(cx)).child(format!("({n} New)")));
        let age = post_age(post);
        let byline = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .min_w_0()
            .text_size(px(12.))
            .text_color(muted)
            .children(first.map(|m| ui::avatar(m.author.avatar.clone(), 20.)))
            .children(first.map(|m| div().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).text_color(palette::text(cx)).child(m.author.name.clone())))
            .child(div().flex_none().child(if page.media { age } else { format!("posted {age}") }))
            .children(self.is_new_post(&forum.id, post).then(|| ui::badge(palette::accent(), palette::on_accent()).ml_auto().child("New")));
        let picture = media.and_then(|(urls, height)| mosaic_view(&urls, height, 4., cx)).map(|m| m.w_full());
        let text = picture.is_none().then(|| first.map(|m| div().line_clamp(5).text_size(px(14.)).text_color(muted).child(m.content.clone()))).flatten();
        let footer = (!page.media).then(|| {
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(12.))
                .text_color(faint)
                .children(top_reaction(first, forum).map(|(emoji, count)| reaction_chip(emoji, count, cx)))
                .child(div().flex().items_center().gap(px(4.)).child(Icon::new(Glyph::ChatRound).size(px(14.))).child(replies.to_string()).children(unread))
        });
        let title = post_title(post, &page.words, self.title_ink(&forum.id, post, cx), if page.media { 14. } else { 16. }, cx);
        let open = post.clone();
        let (menu_post, guild_id) = (post.clone(), self.guild_id.clone().unwrap_or_default());
        let card = ui::pressable(div().id(SharedString::from(format!("post-{}", post.id))), 0.99)
            .p(px(14.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .squircle(ui::RADIUS_LG)
            .bg(sidebar)
            .border_1()
            .border_color(palette::line(cx))
            .cursor_pointer()
            .hover(move |s| s.bg(lifted).border_color(edge));
        let card = match page.media {
            true => card.children(picture).child(byline).child(title),
            false => card.child(byline).child(title).children(picture).children(text).children(footer),
        };
        card.on_click(cx.listener(move |this, _, window, cx| this.open_channel(open.clone(), window, cx)))
            .on_mouse_down(MouseButton::Right, cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_thread_menu(menu_post.clone(), guild_id.clone(), e.position, cx)))
            .into_any_element()
    }

    // New Post: a title, the opening message (starting as the forum's template), files and up to five
    // tags. Post Guidelines open by themselves the first time per forum.
    fn open_new_post(&mut self, forum_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let (template, has_guidelines) = self.forum_channel(&forum_id).map_or((String::new(), false), |f| (f.props.template.trim().to_owned(), guidelines_text(f).is_some()));
        let title = cx.new(|cx| InputState::new(window, cx).placeholder("Title").validate(|text, _| text.chars().count() <= MAX_TITLE));
        let body = cx.new(|cx| TextareaState::new(window, cx).placeholder("Message").default_value(template));
        cx.subscribe(&title, |_, _, _: &InputEvent, cx| cx.notify()).detach();
        cx.subscribe(&body, |_, _, _: &InputEvent, cx| cx.notify()).detach();
        title.update(cx, |s, cx| s.focus(window, cx));
        self.menu = None;
        for state in self.forums.values_mut() {
            state.composer = None;
        }
        let state = self.forums.entry(forum_id).or_default();
        let guidelines = has_guidelines && !state.guidelines_seen;
        state.guidelines_seen = true;
        state.composer = Some(Composer { title, body, tags: Vec::new(), files: Vec::new(), guidelines, guidelines_full: false, previewing: false, error: None, sending: false });
        cx.notify();
    }

    fn open_guidelines(&mut self, forum_id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.open_new_post(forum_id, window, cx);
        self.change_composer(|c| c.guidelines = true, cx);
    }

    fn open_composer(&self) -> Option<(&str, &Composer)> {
        self.forums.iter().find_map(|(id, s)| Some((id.as_str(), s.composer.as_ref()?)))
    }

    fn composer_mut(&mut self) -> Option<&mut Composer> {
        self.forums.values_mut().find_map(|s| s.composer.as_mut())
    }

    fn change_composer(&mut self, change: impl FnOnce(&mut Composer), cx: &mut Context<Self>) {
        if let Some(composer) = self.composer_mut() {
            change(composer);
        }
        cx.notify();
    }

    fn close_new_post(&mut self, cx: &mut Context<Self>) {
        for state in self.forums.values_mut() {
            state.composer = None;
        }
        cx.notify();
    }

    fn toggle_post_draft_tag(&mut self, tag_id: &str, cx: &mut Context<Self>) {
        let Some(composer) = self.composer_mut() else { return };
        match composer.tags.iter().position(|t| t == tag_id) {
            Some(at) => drop(composer.tags.remove(at)),
            None if composer.tags.len() < MAX_TAGS => composer.tags.push(tag_id.to_owned()),
            None => {}
        }
        composer.error = None;
        cx.notify();
    }

    fn pick_post_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions { files: true, directories: false, multiple: true, prompt: None });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            this.update(cx, |this, cx| this.add_post_files(paths, cx)).ok();
        })
        .detach();
    }

    // The chat composer's caps: your upload limit per file and ten files.
    fn add_post_files(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let limit = crate::discord::upload_limit(self.my_account.premium_type, self.guild().map_or(0, |g| g.premium_tier));
        let files = paths.into_iter().filter_map(PendingFile::new).collect();
        self.change_composer(|c| c.error = crate::attachments::admit(&mut c.files, files, limit).map(Into::into), cx);
    }

    fn cooldown_left(&self, forum_id: &str) -> Option<Duration> {
        let until = self.forums.get(forum_id)?.cooldown_until?;
        until.checked_duration_since(Instant::now()).filter(|left| !left.is_zero())
    }

    // Not verified: that Manage Messages or Manage Channel skip a forum's post slowmode, as they do a channel's.
    fn skips_slowmode(&self, forum_id: &str) -> bool {
        self.can_in(forum_id, perm::MANAGE_MESSAGES) || self.can_in(forum_id, perm::MANAGE_CHANNELS)
    }

    // After a post, the forum's slowmode runs down, ticking the composer's clock each second.
    fn start_post_cooldown(&mut self, forum_id: &str, cx: &mut Context<Self>) {
        let slowmode = self.forum_channel(forum_id).map_or(0, |f| f.props.slowmode);
        if slowmode == 0 || self.skips_slowmode(forum_id) {
            return;
        }
        self.forums.entry(forum_id.to_owned()).or_default().cooldown_until = Some(Instant::now() + Duration::from_secs(slowmode.into()));
        let id = forum_id.to_owned();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let ticking = this.update(cx, |this, cx| {
                    cx.notify();
                    this.cooldown_left(&id).is_some()
                });
                if !ticking.unwrap_or(false) {
                    break;
                }
            }
        })
        .detach();
    }

    fn submit_new_post(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((forum_id, composer)) = self.open_composer().map(|(id, c)| (id.to_owned(), c)) else { return };
        if composer.sending || self.cooldown_left(&forum_id).is_some() {
            return;
        }
        let (name, content, tags) = (composer.title.read(cx).value().trim().to_owned(), composer.body.read(cx).value().trim().to_owned(), composer.tags.clone());
        let files: Vec<crate::discord::Upload> = composer.files.iter().map(PendingFile::upload).collect();
        let (require_tag, archive_after) = self.forum_channel(&forum_id).map_or((false, 0), |f| (f.props.require_tag, f.props.archive_after));
        if let Some(error) = post_error(&name, !content.is_empty() || !files.is_empty(), require_tag && tags.is_empty()) {
            return self.change_composer(|c| c.error = Some(error.into()), cx);
        }
        let Some(client) = self.client.clone() else { return };
        self.change_composer(|c| c.sending = true, cx);
        let guild_id = self.guild_id.clone().unwrap_or_default();
        let body = post_body(&name, &content, &tags, &files, if archive_after > 0 { archive_after } else { ARCHIVE_AFTER });
        cx.spawn_in(window, async move |this, cx| {
            let path = format!("/channels/{forum_id}/threads?use_nested_fields=true");
            let created = match files.is_empty() {
                true => client.call(Method::POST, path, Some(body)).await,
                false => client.call_files(path, body, files).await,
            };
            this.update_in(cx, |this, window, cx| {
                let post = match created {
                    Ok(thread) => parse_thread(&thread, &guild_id),
                    Err(error) => return this.change_composer(|c| (c.sending, c.error) = (false, Some(error.into())), cx),
                };
                this.close_new_post(cx);
                this.start_post_cooldown(&forum_id, cx);
                let Some(post) = post else { return this.show_toast("Couldn't open the new post", cx) };
                this.on_event(crate::discord::Event::ChannelUpsert { guild_id: guild_id.clone(), channel: post.clone() }, cx);
                this.open_channel(post, window, cx);
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn render_new_post(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (forum_id, composer) = self.open_composer()?;
        let forum = self.forum_channel(forum_id)?;
        let (strong, muted) = (palette::strong(cx), palette::muted(cx));
        let guidelines = composer.guidelines.then(|| self.guidelines_panel(forum, composer.guidelines_full, window, cx)).flatten();
        let fields = match composer.previewing {
            true => vec![self.post_preview(forum, composer, cx)],
            false => self.post_fields(forum, composer, cx),
        };
        let error = composer.error.clone().map(|e| ui::field_error(e, cx));
        let cooling = self.cooldown_left(forum_id);
        let slowmode = (forum.props.slowmode > 0 && !self.skips_slowmode(forum_id)).then(|| slowmode_clock(cooling.map_or(forum.props.slowmode.into(), |left| left.as_secs() + 1), cx));
        let preview = ui::button("new-post-preview", ui::Tone::Ghost, cx)
            .child(Icon::new(if composer.previewing { Glyph::EyeSlash } else { Glyph::Eye }).size(px(16.)))
            .child(if composer.previewing { "Exit Preview" } else { "Preview" })
            .on_click(cx.listener(|this, _, _, cx| this.change_composer(|c| c.previewing = !c.previewing, cx)));
        let actions = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(preview)
            .children(slowmode)
            .child(div().flex_1())
            .child(ui::button("new-post-cancel", ui::Tone::Secondary, cx).child("Cancel").on_click(cx.listener(|this, _, _, cx| this.close_new_post(cx))))
            .child(
                ui::button("new-post-submit", ui::Tone::Primary, cx)
                    .loading(composer.sending.then_some("Posting"))
                    .disabled(cooling.is_some())
                    .child("Post")
                    .on_click(cx.listener(|this, _, window, cx| this.submit_new_post(window, cx))),
            );
        let reopen = (!composer.guidelines && guidelines_text(forum).is_some())
            .then(|| ui::icon_button("new-post-guidelines", Glyph::Book, "Post Guidelines", cx).on_click(cx.listener(|this, _, _, cx| this.change_composer(|c| c.guidelines = true, cx))));
        let head = div()
            .flex()
            .items_start()
            .gap(px(4.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().text_size(px(20.)).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD).text_color(strong).child("New post"))
                    .child(div().truncate().text_size(px(14.)).text_color(muted).child(format!("in {}", forum.name))),
            )
            .children(reopen)
            .child(ui::icon_button("new-post-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.close_new_post(cx))));
        let body = ui::dialog_body()
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if e.keystroke.key == "escape" {
                    this.close_new_post(cx);
                }
            }))
            .child(head)
            .children(guidelines)
            .children(fields)
            .children(error)
            .child(actions);
        let backdrop = div().id("new-post-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_new_post(cx)));
        Some(ui::modal(backdrop, ui::dialog("new-post", 520., body, window, cx), cx))
    }

    // Title, tags, message and files, as you write them.
    fn post_fields(&self, forum: &Channel, composer: &Composer, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let manage = self.can_in(&forum.id, perm::MANAGE_THREADS);
        let full = composer.tags.len() >= MAX_TAGS;
        let title_focus = composer.title.read(cx).focus_handle(cx);
        let title_count = composer.title.read(cx).value().chars().count();
        let title = ui::field("Title", cx)
            .child(ui::text_field("new-post-title", Input::new(&composer.title).appearance(false), &title_focus))
            .child(ui::field_hint("", Some(format!("{title_count} / {MAX_TITLE}").into()), cx));
        let body_focus = composer.body.read(cx).focus_handle(cx);
        let message = ui::field("Message", cx).child(ui::text_area("new-post-body", Textarea::new(&composer.body).appearance(false).px_0().text_size(px(14.)), &body_focus));
        let usable: Vec<&ForumTag> = forum.props.tags.iter().filter(|t| manage || !t.moderated).collect();
        let tags = (!usable.is_empty()).then(|| {
            let chips: Vec<AnyElement> = usable
                .iter()
                .map(|tag| {
                    let (on, tag_id) = (composer.tags.contains(&tag.id), tag.id.clone());
                    let chip = tag_chip(SharedString::from(format!("new-post-tag-{}", tag.id)), tag, on, cx).disabled(full && !on);
                    chip.on_click(cx.listener(move |this, _, _, cx| this.toggle_post_draft_tag(&tag_id, cx))).into_any_element()
                })
                .collect();
            ui::field(format!("Tags \u{b7} pick up to {MAX_TAGS}"), cx).child(div().flex().flex_wrap().gap(px(6.)).children(chips))
        });
        let files: Vec<AnyElement> = composer
            .files
            .iter()
            .enumerate()
            .map(|(i, file)| {
                let path = file.path.clone();
                let remove = ui::icon_button(SharedString::from(format!("new-post-file-remove-{i}")), Glyph::X, "Remove", cx)
                    .on_click(cx.listener(move |this, _, _, cx| this.change_composer(|c| c.files.retain(|f| f.path != path), cx)));
                file_chip(file, cx).child(remove).into_any_element()
            })
            .collect();
        let add = ui::button("new-post-attach", ui::Tone::Secondary, cx).small().child(Icon::new(Glyph::Upload).size(px(14.))).child("Add files").on_click(cx.listener(|this, _, window, cx| this.pick_post_files(window, cx)));
        let attachments = ui::field("Files", cx).child(div().flex().flex_wrap().items_center().gap(px(6.)).children(files).child(add));
        [Some(title.into_any_element()), tags.map(IntoElement::into_any_element), Some(message.into_any_element()), Some(attachments.into_any_element())].into_iter().flatten().collect()
    }

    // The post as its card will read: tags, title, the message rendered, and its files.
    fn post_preview(&self, forum: &Channel, composer: &Composer, cx: &mut Context<Self>) -> AnyElement {
        let (title, content) = (composer.title.read(cx).value().trim().to_owned(), composer.body.read(cx).value().to_string());
        let tags: Vec<Div> = forum.props.tags.iter().filter(|t| composer.tags.contains(&t.id)).map(|t| post_tag_badge(t.emoji.as_ref(), t.name.clone(), false, cx)).collect();
        let (heading, ink) = match title.is_empty() {
            true => ("Untitled post".to_owned(), palette::faint(cx)),
            false => (title, palette::strong(cx)),
        };
        div()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .squircle(ui::RADIUS_LG)
            .bg(palette::surface(Level::Sidebar, cx))
            .border_1()
            .border_color(palette::line(cx))
            .child(div().flex().flex_wrap().gap(px(6.)).children(tags))
            .child(div().text_size(px(18.)).font_weight(FontWeight::SEMIBOLD).text_color(ink).child(heading))
            .child(TextView::markdown("new-post-preview-body", crate::chat::discord_breaks(&content)).selectable(true))
            .child(div().flex().flex_wrap().gap(px(6.)).children(composer.files.iter().map(|f| file_chip(f, cx).pr(px(10.)))))
            .into_any_element()
    }

    // Post Guidelines: the forum's topic as markdown, 220px tall until expanded; its pencil opens the
    // forum's settings for Manage Channels.
    fn guidelines_panel(&self, forum: &Channel, full: bool, window: &mut Window, cx: &mut Context<Self>) -> Option<Div> {
        let topic = guidelines_text(forum)?;
        let edit = self.can_in(&forum.id, perm::MANAGE_CHANNELS).then(|| {
            let (channel, guild_id) = (forum.clone(), self.guild_id.clone().unwrap_or_default());
            ui::icon_button("new-post-guidelines-edit", Glyph::Pen, "Edit Post Guidelines", cx).on_click(cx.listener(move |this, _, window, cx| {
                this.close_new_post(cx);
                this.open_channel_settings(channel.clone(), guild_id.clone(), window, cx);
            }))
        });
        let close = ui::icon_button("new-post-guidelines-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.change_composer(|c| c.guidelines = false, cx)));
        let measured = window.use_keyed_state(SharedString::from(format!("new-post-guidelines-{}", forum.id)), cx, |_, _| 0f32);
        let overflows = *measured.read(cx) > GUIDELINES_HEIGHT;
        let measure = measured.clone();
        let text = div()
            .max_h(px(if full { GUIDELINES_HEIGHT * 2. } else { GUIDELINES_HEIGHT }))
            .child(div().child(TextView::markdown("new-post-guidelines-markdown", crate::chat::discord_breaks(topic)).selectable(true)))
            .on_children_prepainted(move |bounds, _, cx| {
                let Some(content) = bounds.first() else { return };
                let next = f32::from(content.size.height);
                measure.update(cx, |h, cx| {
                    if (*h - next).abs() > 0.5 {
                        *h = next;
                        cx.notify();
                    }
                });
            })
            .id("new-post-guidelines-text")
            .overflow_y_scrollbar();
        let expand = overflows.then(|| {
            ui::button("new-post-guidelines-expand", ui::Tone::Link, cx)
                .child(if full { "Collapse" } else { "Expand" })
                .on_click(cx.listener(|this, _, _, cx| this.change_composer(|c| c.guidelines_full = !c.guidelines_full, cx)))
        });
        let head = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(Icon::new(Glyph::Book).size(px(16.)).text_color(palette::muted(cx)))
            .child(div().flex_1().ml(px(4.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child("Post Guidelines"))
            .children(edit)
            .child(close);
        Some(
            div()
                .p(px(12.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .squircle(ui::RADIUS_MD)
                .bg(palette::surface(Level::Sidebar, cx))
                .border_1()
                .border_color(palette::line(cx))
                .child(head)
                .child(text)
                .children(expand),
        )
    }

    // Edit Tags on a post's menu, for its author and Manage Threads: each tick saves at once, and at
    // five the unticked ones are disabled.
    pub(crate) fn edit_tags_entry(&self, post: &Channel, manage: bool, owner: bool) -> Option<Entry> {
        if !(manage || owner) {
            return None;
        }
        let forum = self.forum_channel(post.parent_id.as_deref()?).filter(|f| f.kind == ChannelKind::Forum)?;
        let applied = post.thread.as_ref().map(|t| t.applied_tags.clone()).unwrap_or_default();
        let full = applied.len() >= MAX_TAGS;
        let rows: Vec<Entry> = forum
            .props
            .tags
            .iter()
            .filter(|t| manage || !t.moderated)
            .map(|tag| {
                let on = applied.contains(&tag.id);
                let (post_id, tag_id) = (post.id.clone(), tag.id.clone());
                let row = tag_item(tag, move |this, _, cx| this.toggle_post_tag(&post_id, &tag_id, cx)).checked(on);
                (if full && !on { row.disabled() } else { row }).entry()
            })
            .collect();
        (!rows.is_empty()).then(|| submenu("Edit Tags", rows).icon(Glyph::Tag).entry())
    }

    fn toggle_post_tag(&mut self, post_id: &str, tag_id: &str, cx: &mut Context<Self>) {
        let Some(post) = self.forum_post(post_id).or_else(|| self.channel.clone().filter(|c| c.id == post_id)) else { return };
        let meta = post.thread.clone().unwrap_or_default();
        let mut tags = meta.applied_tags.clone();
        match tags.iter().position(|t| t == tag_id) {
            Some(at) => drop(tags.remove(at)),
            None if tags.len() < MAX_TAGS => tags.push(tag_id.to_owned()),
            None => return,
        }
        self.set_post_tags(post_id, meta.applied_tags, tags, meta.archived, cx);
    }

    // Discord reopens an archived post before changing its tags (unarchiveThreadIfNecessary).
    fn set_post_tags(&mut self, post_id: &str, before: Vec<String>, tags: Vec<String>, archived: bool, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let applied = tags.clone();
        self.update_post(post_id, |p| {
            if let Some(meta) = p.thread.as_mut() {
                meta.applied_tags = applied.clone();
                meta.archived = false;
            }
        });
        cx.notify();
        let id = post_id.to_owned();
        cx.spawn(async move |this, cx| {
            let path = format!("/channels/{id}");
            let reopened = match archived {
                true => client.call(Method::PATCH, path.clone(), Some(json!({ "archived": false }))).await.map(|_| ()),
                false => Ok(()),
            };
            let result = match reopened {
                Ok(()) => client.call(Method::PATCH, path, Some(json!({ "applied_tags": tags }))).await.map(|_| ()),
                Err(error) => Err(error),
            };
            let Err(error) = result else { return };
            this.update(cx, |this, cx| {
                this.update_post(&id, |p| {
                    if let Some(meta) = p.thread.as_mut() {
                        meta.applied_tags = before.clone();
                        meta.archived = archived;
                    }
                });
                this.show_toast(error, cx);
            })
            .ok();
        })
        .detach();
    }
}

fn insert_previews(state: &mut ForumState, body: &Value) {
    for m in body["first_messages"].as_array().into_iter().flatten().filter_map(parse_message) {
        state.previews.insert(m.channel_id.clone(), m);
    }
}

// Not verified: that the server takes an empty `name` with only tags; Discord's client sends both.
fn search_path(forum_id: &str, key: &SearchKey) -> String {
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
fn arrange(posts: Vec<Channel>, found: &[Channel], view: ForumView, filter: &[String], query: &str, previews: &HashMap<String, Message>) -> Listing {
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
fn tags_match(post: &Channel, filter: &[String], match_all: bool) -> bool {
    if filter.is_empty() {
        return true;
    }
    let applied = post.thread.as_ref().map_or(&[][..], |t| t.applied_tags.as_slice());
    match match_all {
        true => filter.iter().all(|t| applied.contains(t)),
        false => filter.iter().any(|t| applied.contains(t)),
    }
}

fn words(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_lowercase).collect()
}

fn words_match(post: &Channel, first: Option<&Message>, words: &[String]) -> bool {
    let haystack = format!("{} {}", post.name, first.map_or("", |m| m.content.as_str())).to_lowercase();
    words.iter().all(|w| haystack.contains(w.as_str()))
}

// Recently Active: the last message, or the post itself; Date Posted: the post's own id.
fn sort_key(post: &Channel, by_creation: bool) -> u64 {
    match by_creation {
        true => snowflake(&post.id),
        false => post.last_message_id.as_deref().map_or_else(|| snowflake(&post.id), snowflake),
    }
}

fn snowflake(id: &str) -> u64 {
    id.parse().unwrap_or(0)
}

fn snowflake_ms(id: &str) -> i64 {
    (snowflake(id) >> 22) as i64 + DISCORD_EPOCH
}

fn ago(id: &str) -> String {
    crate::panel::ago(chrono::Utc::now().timestamp_millis() - snowflake_ms(id))
}

fn post_age(post: &Channel) -> String {
    ago(&post.id)
}

// The card's time: last activity, or the post's age when sorted by Date Posted. Its tooltip says when it was posted.
fn post_time(post: &Channel, by_creation: bool) -> Stateful<Div> {
    let since = if by_creation { post.id.as_str() } else { post.last_message_id.as_deref().unwrap_or(&post.id) };
    let posted = chrono::DateTime::from_timestamp_millis(snowflake_ms(&post.id)).map(|t| t.with_timezone(&chrono::Local).format("%-d %B %Y %H:%M").to_string()).unwrap_or_default();
    div().id(SharedString::from(format!("post-time-{}", post.id))).child(ago(since)).tooltip(move |window, cx| ui::tip(format!("Posted {posted}"), window, cx))
}

// The first message's pictures, four at most: its images and videos' posters, else an embed's thumbnail.
fn post_media(first: &Message) -> Vec<String> {
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
enum Mosaic {
    One,
    Pair,
    OneTwo,
    Grid,
}

impl Mosaic {
    fn of(count: usize) -> Option<Self> {
        match count {
            0 => None,
            1 => Some(Self::One),
            2 => Some(Self::Pair),
            3 => Some(Self::OneTwo),
            _ => Some(Self::Grid),
        }
    }
}

fn mosaic_view(urls: &[String], height: f32, gap: f32, cx: &Context<Waffle>) -> Option<Div> {
    let layout = Mosaic::of(urls.len())?;
    let radius = if layout == Mosaic::One { ui::RADIUS_MD } else { ui::RADIUS_XS };
    let fill = palette::selected(cx);
    let tile = |url: &String| div().flex_1().min_w_0().min_h_0().child(ui::media(url.clone(), ui::all(radius)).size_full().bg(fill).object_fit(gpui_kit::ObjectFit::Cover));
    let row = |urls: &[String]| div().flex_1().min_h_0().flex().gap(px(gap)).children(urls.iter().map(tile));
    let frame = div().h(px(height)).flex().gap(px(gap));
    Some(match layout {
        Mosaic::One | Mosaic::Pair => frame.children(urls.iter().map(tile)),
        Mosaic::OneTwo => frame.child(tile(&urls[0])).child(div().flex_1().min_w_0().flex().flex_col().gap(px(gap)).children(urls[1..3].iter().map(tile))),
        Mosaic::Grid => frame.flex_col().child(row(&urls[0..2])).child(row(&urls[2..4])),
    })
}

// Discord's "(N New)" count: capped at 25+, and 1+ while the count is unknown.
fn new_count_label(count: Option<u32>) -> String {
    match count {
        Some(n) if n >= UNREAD_CAP => format!("{UNREAD_CAP}+"),
        Some(n) if n > 0 => n.to_string(),
        _ => "1+".to_owned(),
    }
}

fn new_posts_label(count: usize) -> String {
    match count {
        1 => "1 New Post".to_owned(),
        n => format!("{n} New Posts"),
    }
}

fn guidelines_text(forum: &Channel) -> Option<&str> {
    forum.topic.as_deref().map(str::trim).filter(|t| !t.is_empty())
}

// Slowmode's clock: m:ss, or h:mm:ss from an hour.
fn countdown(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    match h {
        0 => format!("{m}:{s:02}"),
        _ => format!("{h}:{m:02}:{s:02}"),
    }
}

// The most-used reaction on the opening message, else the forum's default reaction with no count.
// GAP: Discord's burst (super) reactions aren't parsed, so only normal counts rank.
fn top_reaction(first: Option<&Message>, forum: &Channel) -> Option<(Emoji, Option<u32>)> {
    let top = first.and_then(|m| m.reactions.iter().max_by_key(|r| r.count));
    match top {
        Some(r) => Some((r.emoji.clone(), Some(r.count))),
        None => forum.props.default_reaction.clone().map(|e| (e, None)),
    }
}

// Title, two lines at most; search words light up.
fn post_title(post: &Channel, words: &[String], ink: gpui_kit::Hsla, size: f32, cx: &Context<Waffle>) -> Div {
    let hit = HighlightStyle { background_color: Some(palette::accent_soft(cx)), color: Some(palette::accent_ink(cx)), ..Default::default() };
    let ranges = hits(&post.name, words);
    div().line_clamp(2).text_ellipsis().text_size(px(size)).font_weight(FontWeight::MEDIUM).text_color(ink).child(StyledText::new(post.name.clone()).with_highlights(ranges.into_iter().map(|r| (r, hit))))
}

// Byte ranges of each search word in `text`, case-insensitively; none where lowercasing changes lengths.
fn hits(text: &str, words: &[String]) -> Vec<Range<usize>> {
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
fn gallery_columns(width: f32) -> usize {
    let t = width / 450.;
    ((t + (width / 320. - t) / 2.).floor() as usize).max(1)
}

// `has_message`: text or files.
fn post_error(name: &str, has_message: bool, missing_tag: bool) -> Option<&'static str> {
    match () {
        _ if name.is_empty() => Some("A title is required"),
        _ if !has_message => Some("A message is required"),
        _ if missing_tag => Some("A tag is required to create a post in this channel"),
        _ => None,
    }
}

// Discord's create-post body; files go alongside as `files[n]`, each named here by its index.
fn post_body(name: &str, content: &str, tags: &[String], files: &[crate::discord::Upload], archive_after: u32) -> Value {
    let mut message = json!({ "content": content });
    if !files.is_empty() {
        message["attachments"] = files.iter().enumerate().map(|(i, f)| json!({ "id": i.to_string(), "filename": f.name })).collect();
    }
    json!({ "name": name, "auto_archive_duration": archive_after, "applied_tags": tags, "message": message })
}

// A file waiting in the composer: its kind, name and size; callers add the remove button.
fn file_chip(file: &PendingFile, cx: &Context<Waffle>) -> Div {
    let glyph = match file.kind() {
        FileKind::Image => Glyph::Image,
        FileKind::Video => Glyph::Video,
        FileKind::Other => Glyph::File,
    };
    div()
        .h(px(32.))
        .pl(px(10.))
        .pr(px(2.))
        .flex()
        .items_center()
        .gap(px(6.))
        .squircle(ui::RADIUS_SM)
        .bg(palette::selected(cx))
        .text_size(px(13.))
        .text_color(palette::text(cx))
        .child(Icon::new(glyph).size(px(14.)).text_color(palette::muted(cx)))
        .child(div().max_w(px(180.)).truncate().child(file.name.clone()))
        .child(div().flex_none().text_color(palette::faint(cx)).child(crate::chat::format_size(file.size)))
}

fn slowmode_clock(secs: u64, cx: &Context<Waffle>) -> Stateful<Div> {
    div()
        .id("new-post-slowmode")
        .flex()
        .items_center()
        .gap(px(4.))
        .text_size(px(12.))
        .text_color(palette::muted(cx))
        .child(Icon::new(Glyph::Timer).size(px(14.)))
        .child(countdown(secs))
        .tooltip(|window, cx| ui::tip("Slowmode is enabled", window, cx))
}

fn tag_chip(id: SharedString, tag: &ForumTag, on: bool, cx: &Context<Waffle>) -> ui::Control {
    ui::chip_lead(id, tag.emoji.as_ref().map(|e| emoji_face(e, 16.)), tag.name.clone(), on, cx)
}

fn tag_item(tag: &ForumTag, act: impl Fn(&mut Waffle, &mut Window, &mut Context<Waffle>) + 'static) -> crate::menu::Item {
    let row = item(tag.name.clone(), act);
    match tag.emoji.clone() {
        Some(emoji) => row.emoji(emoji),
        None => row,
    }
}

// A tag on a card; one being filtered by takes the accent.
fn post_tag_badge(emoji: Option<&Emoji>, name: impl Into<SharedString>, hit: bool, cx: &Context<Waffle>) -> Div {
    let (fill, ink) = if hit { (palette::accent_soft(cx), palette::accent_ink(cx)) } else { (palette::selected(cx), palette::text(cx)) };
    ui::badge(fill, ink).h(px(22.)).gap(px(4.)).children(emoji.map(|e| emoji_face(e, 14.))).child(name.into())
}

fn reaction_chip(emoji: Emoji, count: Option<u32>, cx: &Context<Waffle>) -> Div {
    ui::badge(palette::selected(cx), palette::text(cx)).h(px(22.)).gap(px(4.)).child(emoji_face(&emoji, 14.)).children(count.map(|c| c.to_string()))
}

fn no_history_notice(forum: &Channel, cx: &Context<Waffle>) -> Div {
    let name = format!("#{}", forum.name);
    let text = format!("You do not have permission to view the post history of {name}.");
    let bold = HighlightStyle { font_weight: Some(FontWeight::SEMIBOLD), color: Some(palette::strong(cx)), ..Default::default() };
    let at = text.find(&name).unwrap_or(0);
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(ui::divider(Some("Older Posts"), cx))
        .child(div().text_center().text_size(px(14.)).text_color(palette::muted(cx)).child(StyledText::new(text.clone()).with_highlights([(at..at + name.len(), bold)])))
}

#[cfg(debug_assertions)]
pub(crate) mod demo {
    use gpui_kit::{Context, Window};
    use serde_json::{Value, json};

    use super::{DISCORD_EPOCH, ForumState, PendingFile};
    use crate::{
        Waffle,
        discord::{ReadState, parse_channel, parse_message},
    };

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
            "forum-sort" => waffle.open_live_menu(gpui_kit::point(gpui_kit::px(1000.), gpui_kit::px(110.)), |this| this.sort_view_entries("forum"), cx),
            "forum-new-post" => {
                waffle.open_new_post("forum".into(), window, cx);
                if let Some(composer) = waffle.composer_mut() {
                    composer.tags = vec!["t-idea".into()];
                    composer.files = vec![PendingFile { path: "brand/icons/blueprint.png".into(), name: "night-map-mockup.png".into(), size: 184_320, spoiler: false }];
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::discord::parse_channel;

    fn post(id: &str, last: &str, tags: &[&str], pinned: bool, archived: bool) -> Channel {
        let c = json!({ "id": id, "type": 11, "name": format!("post {id}"), "parent_id": "f", "last_message_id": last, "applied_tags": tags, "flags": (if pinned { 2 } else { 0 }), "thread_metadata": { "archived": archived } });
        parse_channel(&c, "g").expect("valid thread")
    }

    fn ids(posts: &[Channel]) -> Vec<&str> {
        posts.iter().map(|p| p.id.as_str()).collect()
    }

    const LIST: ForumView = ForumView { by_creation: false, gallery: false, match_all: false };

    #[test]
    fn pinned_first_then_activity_then_older_posts() {
        let posts = vec![post("1", "50", &[], false, false), post("2", "20", &[], true, false), post("3", "90", &[], false, true), post("4", "60", &[], false, false)];
        let listing = arrange(posts.clone(), &[], LIST, &[], "", &HashMap::new());
        assert_eq!(ids(&listing.active), ["2", "4", "1"]);
        assert_eq!(ids(&listing.older), ["3"]);
        let by_date = arrange(posts, &[], ForumView { by_creation: true, ..LIST }, &[], "", &HashMap::new());
        assert_eq!(ids(&by_date.active), ["2", "4", "1"]);
    }

    #[test]
    fn tag_matching_some_and_all() {
        let posts = vec![post("1", "1", &["a"], false, false), post("2", "2", &["a", "b"], false, false), post("3", "3", &[], false, false)];
        let filter = ["a".to_owned(), "b".to_owned()];
        assert_eq!(ids(&arrange(posts.clone(), &[], LIST, &filter, "", &HashMap::new()).active), ["2", "1"]);
        assert_eq!(ids(&arrange(posts, &[], ForumView { match_all: true, ..LIST }, &filter, "", &HashMap::new()).active), ["2"]);
    }

    #[test]
    fn text_search_matches_previews_adds_server_hits_and_drops_older() {
        let posts = vec![post("1", "1", &[], false, false), post("2", "2", &[], false, true)];
        let mut previews = HashMap::new();
        previews.insert("2".to_owned(), Message { content: "The Minimap pings".into(), ..Default::default() });
        let server = [post("9", "9", &[], false, false)];
        let listing = arrange(posts, &server, LIST, &[], "minimap", &previews);
        assert_eq!(ids(&listing.active), ["9", "2"]);
        assert!(listing.older.is_empty());
    }

    #[test]
    fn hits_are_case_insensitive_and_merged() {
        assert_eq!(hits("Minimap on the MAP", &words("map mini")), vec![0..4, 4..7, 15..18]);
        assert!(hits("İstanbul map", &words("map")).is_empty());
    }

    #[test]
    fn gallery_columns_follow_discords_formula() {
        assert_eq!(gallery_columns(300.), 1);
        assert_eq!(gallery_columns(848.), 2);
        assert_eq!(gallery_columns(1400.), 3);
    }

    #[test]
    fn post_errors_in_discords_order() {
        assert_eq!(post_error("", false, true), Some("A title is required"));
        assert_eq!(post_error("t", false, true), Some("A message is required"));
        assert_eq!(post_error("t", true, true), Some("A tag is required to create a post in this channel"));
        assert_eq!(post_error("t", true, false), None);
    }

    #[test]
    fn unread_count_caps_at_25_and_falls_back_to_1() {
        assert_eq!(new_count_label(Some(3)), "3");
        assert_eq!(new_count_label(Some(25)), "25+");
        assert_eq!(new_count_label(Some(140)), "25+");
        assert_eq!(new_count_label(Some(0)), "1+");
        assert_eq!(new_count_label(None), "1+");
    }

    #[test]
    fn mosaic_layout_by_picture_count() {
        assert_eq!(Mosaic::of(0), None);
        assert_eq!(Mosaic::of(1), Some(Mosaic::One));
        assert_eq!(Mosaic::of(2), Some(Mosaic::Pair));
        assert_eq!(Mosaic::of(3), Some(Mosaic::OneTwo));
        assert_eq!(Mosaic::of(4), Some(Mosaic::Grid));
        let first = Message { media: (0..6).map(|i| crate::discord::Media { url: format!("{i}.png"), width: 1., height: 1., kind: MediaKind::Image }).collect(), ..Default::default() };
        assert_eq!(post_media(&first).len(), MOSAIC_MAX);
    }

    #[test]
    fn slowmode_countdown() {
        assert_eq!(countdown(5), "0:05");
        assert_eq!(countdown(21_600), "6:00:00");
    }

    #[test]
    fn post_body_names_each_file_by_index() {
        let file = |name: &str| crate::discord::Upload { path: name.into(), name: name.into(), size: 1, sent: Default::default() };
        let body = post_body("t", "", &[], &[file("a.png"), file("b.png")], 4320);
        assert_eq!(body["message"]["attachments"], json!([{ "id": "0", "filename": "a.png" }, { "id": "1", "filename": "b.png" }]));
        assert!(post_body("t", "m", &[], &[], 4320)["message"].get("attachments").is_none());
    }

    #[test]
    fn search_path_carries_tags_and_matching() {
        let key = SearchKey { query: "ult range".into(), tags: vec!["1".into(), "2".into()], match_all: true };
        assert_eq!(search_path("f", &key), "/channels/f/threads/search?name=ult+range&tag=1%2C2&tag_setting=match_all");
    }
}
