// Discord's forum and media channels: a list of posts (each a thread), newest activity first.
// Active posts arrive over the gateway once the channel is subscribed with threads on; older ones
// come from THREAD_SEARCH (archived, by last message time, 25 at a time); each card's preview is
// the post's first message from FORUM_POSTS, fetched ten at a time as Discord does.
use std::collections::HashMap;

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    AnyElement, Context, FontWeight, HighlightStyle, SharedString, StyledText, Window, WindowControlArea,
    component::Icon,
    div, prelude::*, px,
};
use reqwest::Method;
use serde_json::{Value, json};

use crate::{
    Waffle,
    discord::{Channel, MediaKind, Message, parse_message, parse_thread},
    icons::Glyph,
    mascot::Mood,
    palette::{self, Level},
    ui::{self, Squircle},
};

#[derive(Default)]
pub struct ForumState {
    archived: Vec<Channel>,
    previews: HashMap<String, Message>,
    requested: std::collections::HashSet<String>,
    has_more: bool,
    loading: bool,
    error: Option<SharedString>,
}

impl Waffle {
    // The forum's posts: active ones we know of, then archived ones, newest activity first.
    fn forum_posts(&self, forum_id: &str) -> Vec<Channel> {
        let mut posts: Vec<Channel> = self.guilds.iter().flat_map(|g| g.threads.iter()).filter(|t| t.parent_id.as_deref() == Some(forum_id)).cloned().collect();
        if let Some(state) = self.forums.get(forum_id) {
            posts.extend(state.archived.iter().filter(|a| !posts.iter().any(|p| p.id == a.id)).cloned().collect::<Vec<_>>());
        }
        posts.sort_by_key(|p| std::cmp::Reverse(p.last_message_id.as_deref().and_then(|id| id.parse::<u64>().ok()).unwrap_or(0)));
        posts
    }

    pub(crate) fn load_forum(&mut self, forum_id: String, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let state = self.forums.entry(forum_id.clone()).or_default();
        if state.loading {
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
                        for m in body["first_messages"].as_array().into_iter().flatten().filter_map(parse_message) {
                            state.previews.insert(m.channel_id.clone(), m);
                        }
                        state.error = None;
                    }
                    Err(error) => state.error = Some(error.into()),
                }
                this.load_forum_previews(&forum_id, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // First messages for posts that don't have one yet, ten per request.
    fn load_forum_previews(&mut self, forum_id: &str, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let posts = self.forum_posts(forum_id);
        let state = self.forums.entry(forum_id.to_owned()).or_default();
        let wanted: Vec<String> = posts.iter().map(|p| p.id.clone()).filter(|id| !state.previews.contains_key(id) && !state.requested.contains(id)).collect();
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

    // Discord's New Post: a title, then the opening message.
    fn new_post(&mut self, forum_id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt("New Post", "Post title", "", "Next", move |this, title, window, cx| {
            if title.trim().is_empty() {
                return;
            }
            let (forum, title) = (forum_id.clone(), title.trim().to_owned());
            this.prompt("New Post", format!("Opening message for \u{201c}{title}\u{201d}"), "", "Post", move |this, content, window, cx| {
                let Some(client) = this.client.clone() else { return };
                let (forum, name) = (forum.clone(), title.clone());
                let guild_id = this.guild_id.clone().unwrap_or_default();
                cx.spawn_in(window, async move |this, cx| {
                    let body: Value = json!({ "name": name, "message": { "content": content } });
                    let created = client.call(Method::POST, format!("/channels/{forum}/threads"), Some(body)).await;
                    this.update_in(cx, |this, window, cx| match created.ok().and_then(|t| parse_thread(&t, &guild_id)) {
                        Some(post) => {
                            this.on_event(crate::discord::Event::ChannelUpsert { guild_id: guild_id.clone(), channel: post.clone() }, cx);
                            this.open_channel(post, window, cx);
                        }
                        None => this.show_toast("Couldn't create the post", cx),
                    })
                    .ok();
                })
                .detach();
            }, window, cx);
        }, window, cx);
    }

    // design/panels.css's forum: the channel header, then New post and the posts in one 880px column.
    // GAP: the design's post search and tag filter chips have nothing behind them yet (no post search
    // state, tags aren't parsed), so the bar holds New post alone.
    pub(crate) fn render_forum(&mut self, forum: Channel, cx: &mut Context<Self>) -> AnyElement {
        let posts = self.forum_posts(&forum.id);
        let state = self.forums.get(&forum.id);
        let (strong, faint) = (palette::strong(cx), palette::faint(cx));
        let header = div()
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
            .child(div().ml_auto().child(self.caption_clearance(true)));
        let forum_id = forum.id.clone();
        let new_post = ui::button("forum-new-post", ui::Tone::Primary, cx)
            .child(Icon::new(Glyph::Plus).size(px(16.)))
            .child("New post")
            .on_click(cx.listener(move |this, _, window, cx| this.new_post(forum_id.clone(), window, cx)));
        let cards: Vec<AnyElement> = posts.iter().map(|post| self.post_card(post, state.and_then(|s| s.previews.get(&post.id)), cx)).collect();
        let empty = posts.is_empty() && state.is_some_and(|s| !s.loading);
        let loading = posts.is_empty() && state.is_none_or(|s| s.loading);
        let more = state.filter(|s| s.has_more && !s.loading).map(|_| {
            let id = forum.id.clone();
            ui::button("forum-more", ui::Tone::Ghost, cx).child("Load more posts").on_click(cx.listener(move |this, _, _, cx| this.load_forum(id.clone(), cx)))
        });
        let error = state.and_then(|s| s.error.clone()).map(|e| div().text_size(px(14.)).text_color(palette::danger_ink(cx)).child(e));
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(palette::surface(Level::Page, cx))
            .child(header)
            .child(
                div().id("forum-scroll").flex_1().min_h_0().overflow_y_scrollbar().child(
                    div()
                        .w_full()
                        .max_w(px(880.))
                        .mx_auto()
                        .p(px(16.))
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .when(!empty, |d| d.child(div().flex().justify_end().child(new_post)))
                        .children(error)
                        .when(empty, |d| d.child(ui::empty_state("forum-empty", Mood::Idle, "No posts yet", "Start the first one. Each post gets its own thread.", cx)))
                        .when(loading, |d| d.children((0..4).map(|i| post_skeleton(i, cx))))
                        .child(div().flex().flex_col().gap(px(8.)).children(cards))
                        .children(more.map(|m| div().flex().justify_center().child(m))),
                ),
            )
            .into_any_element()
    }

    // A post: its title, the opening message, who started it and how many replies, and its image.
    fn post_card(&self, post: &Channel, first: Option<&Message>, cx: &mut Context<Self>) -> AnyElement {
        let replies = post.thread.as_ref().map_or(0, |t| t.message_count);
        let archived = post.thread.as_ref().is_some_and(|t| t.archived);
        let (strong, text, muted, faint) = (palette::strong(cx), palette::text(cx), palette::muted(cx), palette::faint(cx));
        let sidebar = palette::surface(Level::Sidebar, cx);
        let (edge, lifted) = (palette::line_strong(cx), ui::mix(sidebar, strong, 0.03));
        let thumb = first.and_then(|m| m.media.first()).map(|m| match &m.kind {
            MediaKind::Image => m.url.clone(),
            MediaKind::Video { poster } | MediaKind::Gif { poster } => poster.clone(),
        });
        let excerpt = first.map(|m| {
            let name = format!("{}: ", m.author.name);
            let highlight = HighlightStyle { color: Some(text), font_weight: Some(FontWeight::MEDIUM), ..Default::default() };
            div().line_clamp(2).text_size(px(14.)).text_color(muted).child(StyledText::new(format!("{name}{}", m.content)).with_highlights([(0..name.len(), highlight)]))
        });
        let starter = first.map(|m| div().flex().items_center().gap(px(4.)).child(ui::avatar(m.author.avatar.clone(), 20.)).child(div().max_w(px(140.)).truncate().font_weight(FontWeight::NORMAL).text_color(muted).child(m.author.name.clone())));
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
                    .child(div().truncate().text_size(px(16.)).font_weight(FontWeight::MEDIUM).text_color(if archived { muted } else { strong }).child(post.name.clone()))
                    .children(excerpt)
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_x(px(12.))
                            .gap_y(px(4.))
                            .text_size(px(12.))
                            .text_color(faint)
                            .children(starter)
                            .child(div().flex().items_center().gap(px(4.)).child(Icon::new(Glyph::ChatRound).size(px(14.))).child(replies.to_string())),
                    ),
            )
            .children(thumb.map(|url| ui::media(url, ui::all(ui::RADIUS_MD)).size(px(88.)).flex_none().object_fit(gpui_kit::ObjectFit::Cover)))
            .on_click(cx.listener(move |this, _, window, cx| this.open_channel(open.clone(), window, cx)))
            .on_mouse_down(gpui_kit::MouseButton::Right, cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_thread_menu(menu_post.clone(), guild_id.clone(), e.position, cx)))
            .into_any_element()
    }
}

// A post card still loading: title, two lines of its opening message, and the foot.
fn post_skeleton(i: usize, cx: &gpui_kit::App) -> AnyElement {
    let line = |part: &str, width: f32| ui::skeleton(SharedString::from(format!("post-skel-{part}-{i}")), 4., cx).h(px(12.)).w(gpui_kit::relative(width));
    div()
        .p(px(16.))
        .flex()
        .flex_col()
        .gap(px(10.))
        .squircle(ui::RADIUS_MD)
        .bg(palette::surface(Level::Sidebar, cx))
        .child(line("title", [0.5, 0.35, 0.6, 0.42][i % 4]).h(px(16.)))
        .child(line("body", 0.9))
        .child(line("body2", 0.7))
        .child(line("foot", 0.25))
        .into_any_element()
}
