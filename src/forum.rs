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
    illustration::Illustration,
    menu::{Entry, emoji_face, item, label, submenu},
    part::Column,
    palette::{self, Level},
    ui::{self, Squircle},
};

// Discord's limits: tags on a post, title length, search debounce, posts per unread-count request, the
// "(N New)" cap, the guidelines' height before Expand, and pictures in a card's mosaic.
mod composer;
mod helpers;
mod tags;

use helpers::*;

// The widest the forum's toolbar and post list grow, a post's picture thumbnail, the composer preview's
// heading size, a filename's widest, and a tag or reaction chip's height.
const CONTENT_WIDTH: f32 = 880.;
const THUMB: f32 = 72.;
const PREVIEW_HEADING: f32 = 18.;
const FILE_NAME_WIDTH: f32 = 180.;
const TAG_HEIGHT: f32 = 22.;
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
    previews: HashMap<String, Message>, // lint-allow(unbounded-growth): bounded by the posts loaded from one forum, 25 per page of scrolling
    requested: HashSet<String>, // lint-allow(unbounded-growth): bounded by the posts loaded from one forum, 25 per page of scrolling
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
    unread_counts: HashMap<String, u32>, // lint-allow(unbounded-growth): bounded by the posts loaded from one forum, 25 per page of scrolling
    unreads_asked: HashMap<String, u64>, // lint-allow(unbounded-growth): bounded by the posts loaded from one forum, 25 per page of scrolling
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
                this.redraw(Column::Main, cx);
            })
            .ok();
        })
        .detach(); // lint-allow(detached-task): ends itself once its request or timer resolves, and holds only a weak handle
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
        self.redraw(Column::Main, cx);
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
                    this.redraw(Column::Main, cx);
                })
                .ok();
            })
            .detach(); // lint-allow(detached-task): ends itself once its request or timer resolves, and holds only a weak handle
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
        .detach(); // lint-allow(detached-task): the subscription ends when the search entity, held in ForumState, drops
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
        self.redraw(Column::Main, cx);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_DEBOUNCE).await;
            this.update(cx, |this, cx| {
                if this.forums.get(&id).is_some_and(|s| s.pending == pending) {
                    this.search_forum(&id, cx);
                }
            })
            .ok();
        })
        .detach(); // lint-allow(detached-task): ends itself once its request or timer resolves, and holds only a weak handle
    }

    fn search_forum(&mut self, forum_id: &str, cx: &mut Context<Self>) {
        let Some(forum) = self.forum_channel(forum_id).cloned() else { return };
        let key = SearchKey { query: self.forum_query(forum_id, cx), tags: self.forums.get(forum_id).map(|s| s.filter.clone()).unwrap_or_default(), match_all: self.forum_view(&forum).match_all };
        let history = self.can_in(forum_id, perm::READ_MESSAGE_HISTORY);
        let client = self.client.clone();
        let state = self.forums.entry(forum_id.to_owned()).or_default();
        if key.query.is_empty() && key.tags.is_empty() {
            state.found = None;
            return self.redraw(Column::Main, cx);
        }
        if state.found.as_ref().is_some_and(|f| f.key == key) {
            return;
        }
        let Some(client) = client.filter(|_| history) else { return };
        state.found = Some(Found { key: key.clone(), posts: Vec::new(), loading: true });
        self.redraw(Column::Main, cx);
        let (guild_id, id) = (self.guild_id.clone().unwrap_or_default(), forum_id.to_owned());
        cx.spawn(async move |this, cx| {
            let result = client.call(Method::GET, search_path(&id, &key), None).await;
            this.update(cx, |this, cx| {
                let Some(state) = this.forums.get_mut(&id) else { return };
                let Some(found) = state.found.as_mut().filter(|f| f.key == key) else { return };
                found.loading = false;
                let Ok(body) = result else { return this.redraw(Column::Main, cx) };
                found.posts = body["threads"].as_array().into_iter().flatten().filter_map(|t| parse_thread(t, &guild_id)).collect();
                insert_previews(state, &body);
                this.load_forum_previews(&id, cx);
                this.redraw(Column::Main, cx);
            })
            .ok();
        })
        .detach(); // lint-allow(detached-task): ends itself once its request or timer resolves, and holds only a weak handle
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
        ui::col()
            .flex_1()
            .h_full()
            .bg(palette::surface(Level::Page, cx))
            .child(self.forum_header(&forum, cx))
            .child(ui::col().flex_none().w_full().items_center().child(toolbar))
            .child(div().id("forum-scroll").flex_1().min_h_0().overflow_y_scrollbar().child(ui::col().w_full().items_center().child(body)))
            .into_any_element()
    }

    fn forum_header(&self, forum: &Channel, cx: &mut Context<Self>) -> Div {
        let (strong, faint) = (palette::strong(cx), palette::faint(cx));
        ui::row()
            .justify_between()
            .h_12()
            .flex_none()
            .pl_3()
            .pr_2()
            .gap_2()
            .border_b_1()
            .border_color(palette::line(cx))
            .window_control_area(WindowControlArea::Drag)
            .child(
                ui::row()
                    .h_8()
                    .px_1p5()
                    .flex_none()
                    .max_w(gpui_kit::relative(0.6))
                    .gap_1p5()
                    .text_size(ui::TEXT_MD)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(strong)
                    .child(Icon::new(Glyph::Forum).size(ui::ICON_XL).text_color(faint))
                    .child(crate::ui::truncating("header-forum-name", forum.name.clone())),
            )
            .children(forum.topic.clone().map(|t| ui::line(t).flex_1().pl_3().border_l_1().border_color(palette::line_strong(cx)).text_size(ui::TEXT_SM).text_color(faint)))
            .child(self.caption_clearance(true))
    }

    // The search box, Sort & View and New post; under them the search status and the tag row.
    fn forum_toolbar(&self, forum: &Channel, page: &ForumPage, search: &Entity<InputState>, cx: &mut Context<Self>) -> Div {
        let id = forum.id.clone();
        let sort = ui::button("forum-sort", ui::Tone::Ghost, cx)
            .child(Icon::new(Glyph::SortDesc).size(ui::ICON_MD))
            .child(if page.media { "Sort" } else { "Sort & View" })
            .on_click(cx.listener(move |this, _, window, cx| {
                let id = id.clone();
                this.open_live_menu(window.mouse_position(), move |this| this.sort_view_entries(&id), cx);
            }));
        let new_post = page.can_post.then(|| self.new_post_button("forum-new-post", &forum.id, cx));
        let guidelines = (page.can_post && guidelines_text(forum).is_some()).then(|| {
            let id = forum.id.clone();
            ui::icon_button("forum-guidelines", Glyph::BookOpen, "Post Guidelines", cx).on_click(cx.listener(move |this, _, window, cx| this.open_guidelines(id.clone(), window, cx)))
        });
        ui::col()
            .flex_none()
            .w_full()
            .max_w(px(CONTENT_WIDTH))
            .px_4()
            .pt_4()
            .pb_1()
            .gap_2p5()
            .child(ui::row().gap_2().child(div().flex_1().min_w_0().child(ui::search_field("forum-search", search, cx))).child(sort).children(guidelines).children(new_post))
            .children(self.search_status(forum, page, cx))
            .children((!forum.props.tags.is_empty()).then(|| self.tag_row(forum, &page.filter, cx)))
    }

    fn new_post_button(&self, key: &'static str, forum_id: &str, cx: &mut Context<Self>) -> ui::Control {
        let id = forum_id.to_owned();
        ui::button(key, ui::Tone::Primary, cx).child(Icon::new(Glyph::Plus).size(ui::ICON_MD)).child("New post").on_click(cx.listener(move |this, _, window, cx| this.open_new_post(id.clone(), window, cx)))
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
        Some(ui::row().gap_2().text_size(ui::TEXT_SM).text_color(palette::muted(cx)).child(text).child(clear))
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
            .child(Icon::new(Glyph::ChevronDown).size(ui::ICON_SM))
            .on_click(cx.listener(move |this, _, window, cx| {
                let id = id.clone();
                this.open_live_menu(window.mouse_position(), move |this| this.tag_filter_entries(&id), cx);
            }));
        ui::row()
            .gap_2()
            .child(ui::row().items_stretch().gap_1p5().overflow_hidden().children(chips))
            .child(ui::seam(cx).bg(palette::line_strong(cx)))
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
        let error = state.and_then(|s| s.error.clone()).map(|e| div().text_size(ui::TEXT_SM).text_color(palette::danger_ink(cx)).child(e));
        let more = state.filter(|s| s.has_more && !s.loading && page.words.is_empty()).map(|_| {
            let id = forum.id.clone();
            ui::row().items_stretch().justify_center().child(ui::button("forum-more", ui::Tone::Ghost, cx).child("Load more posts").on_click(cx.listener(move |this, _, _, cx| this.load_forum(id.clone(), cx))))
        });
        let is_empty = page.listing.active.is_empty() && page.listing.older.is_empty() && page.settled && !page.searching;
        let no_history = (!page.history && page.words.is_empty()).then(|| no_history_notice(forum, cx));
        let older = (!page.listing.older.is_empty()).then(|| ui::divider(Some("Older Posts"), cx));
        let held = state.map_or(0, |s| s.held_back.len());
        let new_posts = (held > 0).then(|| {
            let id = forum.id.clone();
            let button = ui::button("forum-new-posts", ui::Tone::Primary, cx).small().child(Icon::new(Glyph::ArrowUp).size(ui::ICON_SM)).child(new_posts_label(held));
            ui::row().items_stretch().justify_center().child(button.on_click(cx.listener(move |this, _, _, cx| this.show_held_back(&id, cx))))
        });
        let column = ui::col().w_full().max_w(px(CONTENT_WIDTH)).px_4().pt_2().pb_4().gap_3().children(error).children(new_posts);
        if is_empty {
            return column.child(self.forum_empty(forum, page, cx)).children(no_history);
        }
        if page.listing.active.is_empty() && page.listing.older.is_empty() && !page.settled {
            return column.children((0..4).map(|i| post_skeleton(i, cx)));
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
            return ui::empty_state("forum-empty", Illustration::SearchEmpty, "No matching posts", "Search looks at post titles and the opening messages loaded so far.", cx);
        }
        if !page.filter.is_empty() {
            let (title, body) = match page.filter.len() {
                1 => ("There are no posts with this tag yet!", "Be the first to start a conversation with this tag"),
                _ => ("There are no posts with these tags yet!", "Be the first to start a conversation with these tags"),
            };
            let id = forum.id.clone();
            let clear = ui::button("forum-empty-clear", ui::Tone::Secondary, cx).child("Clear all").on_click(cx.listener(move |this, _, _, cx| this.clear_forum_filter(&id, cx)));
            return ui::empty_state("forum-empty", Illustration::ForumEmpty, title, body, cx).child(div().pt_2().child(clear));
        }
        let new_post = page.can_post.then(|| div().pt_2().child(self.new_post_button("forum-empty-new", &forum.id, cx)));
        ui::empty_state("forum-empty", Illustration::ForumEmpty, "Make the first post!", "Be the first to start the conversation!", cx).children(new_post)
    }

    fn post_list(&self, forum: &Channel, page: &ForumPage, posts: &[Channel], cx: &mut Context<Self>) -> AnyElement {
        let previews = self.forums.get(&forum.id).map(|s| &s.previews);
        let cards: Vec<AnyElement> = posts.iter().map(|post| self.post_card(forum, page, post, previews.and_then(|p| p.get(&post.id)), cx)).collect();
        ui::col().gap_2().children(cards).into_any_element()
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
            div().line_clamp(2).text_size(ui::TEXT_SM).text_color(muted).child(StyledText::new(format!("{name}{}", m.content)).with_highlights([(0..name.len(), highlight)]))
        });
        let tags = self.card_tags(forum, page, post, cx);
        let unread = self.post_unread(&forum.id, post).map(|n| div().font_weight(FontWeight::SEMIBOLD).text_color(palette::accent_ink(cx)).child(format!("({n} New)")));
        let footer = ui::row()
            .flex_wrap()
            .gap_x_2()
            .gap_y_1()
            .text_size(ui::TEXT_XS)
            .text_color(faint)
            .children(top_reaction(first, forum).map(|(emoji, count)| reaction_chip(emoji, count, cx)))
            .child(ui::row().gap_1().child(Icon::new(Glyph::ChatRound).size(ui::ICON_SM)).child(meta.message_count.to_string()).children(unread))
            .child("\u{2022}")
            .child(post_time(post, page.view.by_creation));
        let open = post.clone();
        let (menu_post, guild_id) = (post.clone(), self.guild_id.clone().unwrap_or_default());
        ui::pressable(div().id(SharedString::from(format!("post-{}", post.id))), 0.99)
            .py_3p5()
            .px_4()
            .flex()
            .gap_4()
            .squircle(ui::RADIUS_LG)
            .bg(sidebar)
            .border_1()
            .border_color(palette::line(cx))
            .cursor_pointer()
            .hover(move |s| s.bg(lifted).border_color(edge))
            .child(
                ui::col()
                    .flex_1()
                    .gap_1p5()
                    .children(tags)
                    .child(post_title(post, &page.words, self.title_ink(&forum.id, post, cx), 16., cx))
                    .children(excerpt)
                    .child(footer),
            )
            .children(mosaic_view(&first.map(post_media).unwrap_or_default(), THUMB, 2., cx).map(|m| m.w(px(THUMB)).flex_none()))
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
        let pin = meta.pinned.then(|| div().id(SharedString::from(format!("post-pin-{}", post.id))).text_color(palette::muted(cx)).child(Icon::new(Glyph::Pin).size(ui::ICON_SM)).tooltip(|window, cx| ui::tip("Pinned", window, cx)));
        let overflow = (!rest.is_empty()).then(|| {
            div().id(SharedString::from(format!("post-tags-more-{}", post.id))).child(post_tag_badge(None, format!("+{}", rest.len()), false, cx)).tooltip(move |window, cx| ui::tip(rest_names.clone(), window, cx))
        });
        Some(
            ui::row()
                .flex_wrap()
                .gap_1p5()
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
            .child(ui::row().w_full().items_start().gap_4().children(stacks.into_iter().map(|cards| ui::col().flex_1().gap_4().children(cards))))
            .on_children_prepainted(move |bounds, _, cx| {
                let Some(row) = bounds.first() else { return };
                let next = f32::from(row.size.width);
                measure.update(cx, |w, cx| {
                    if (*w - next).abs() > 0.5 {
                        *w = next;
                        cx.notify(); // lint-allow(global-notify): notifies the measured width's own entity, not Waffle
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
        let byline = ui::row()
            .gap_1p5()
            .text_size(ui::TEXT_XS)
            .text_color(muted)
            .children(first.map(|m| ui::avatar(m.author.avatar.clone(), 20.)))
            .children(first.map(|m| ui::line(m.author.name.clone()).font_weight(FontWeight::SEMIBOLD).text_color(palette::text(cx))))
            .child(div().flex_none().child(if page.media { age } else { format!("posted {age}") }))
            .when(self.is_new_post(&forum.id, post), |row| row.child(div().flex_1()).child(ui::badge(palette::accent(), palette::on_accent()).child("New")));
        let picture = media.and_then(|(urls, height)| mosaic_view(&urls, height, 4., cx)).map(|m| m.w_full());
        let text = picture.is_none().then(|| first.map(|m| div().line_clamp(5).text_size(ui::TEXT_SM).text_color(muted).child(m.content.clone()))).flatten();
        let footer = (!page.media).then(|| {
            ui::row()
                .gap_2()
                .text_size(ui::TEXT_XS)
                .text_color(faint)
                .children(top_reaction(first, forum).map(|(emoji, count)| reaction_chip(emoji, count, cx)))
                .child(ui::row().gap_1().child(Icon::new(Glyph::ChatRound).size(ui::ICON_SM)).child(replies.to_string()).children(unread))
        });
        let title = post_title(post, &page.words, self.title_ink(&forum.id, post, cx), if page.media { 14. } else { 16. }, cx);
        let open = post.clone();
        let (menu_post, guild_id) = (post.clone(), self.guild_id.clone().unwrap_or_default());
        let card = ui::pressable(div().id(SharedString::from(format!("post-{}", post.id))), 0.99)
            .p_3p5()
            .flex()
            .flex_col()
            .gap_2()
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
}

#[cfg(debug_assertions)]
pub(crate) mod demo;

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
        let first = Message { media: (0..6).map(|i| crate::discord::Media { url: format!("{i}.png"), width: 1., height: 1., kind: MediaKind::Image, ..Default::default() }).collect(), ..Default::default() };
        assert_eq!(post_media(&first).len(), MOSAIC_MAX);
    }

    #[test]
    fn slowmode_countdown() {
        assert_eq!(countdown(5), "0:05");
        assert_eq!(countdown(21_600), "6:00:00");
    }

    #[test]
    fn post_body_names_each_file_by_index() {
        let file = |name: &str| crate::discord::Upload { path: name.into(), name: name.into(), size: 1, description: None, spoiler: false, sent: Default::default() };
        let alt = crate::discord::Upload { description: Some("The map".into()), spoiler: true, ..file("b.png") };
        let body = post_body("t", "", &[], &[file("a.png"), alt], 4320);
        assert_eq!(body["message"]["attachments"], json!([{ "id": "0", "filename": "a.png" }, { "id": "1", "filename": "b.png", "description": "The map", "is_spoiler": true }]));
        assert!(post_body("t", "m", &[], &[], 4320)["message"].get("attachments").is_none());
    }

    #[test]
    fn search_path_carries_tags_and_matching() {
        let key = SearchKey { query: "ult range".into(), tags: vec!["1".into(), "2".into()], match_all: true };
        assert_eq!(search_path("f", &key), "/channels/f/threads/search?name=ult+range&tag=1%2C2&tag_setting=match_all");
    }
}
