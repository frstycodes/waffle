// The media viewer: every image and video in the open channel over the whole window, the current one
// on the stage (videos play there) and a strip of thumbnails along the foot to jump between them.
// Thumbnails come small from Discord's media proxy and only for the strip's visible stretch; loads that
// scroll out of it are dropped. Full-size loads are the current item and its two neighbours.
use std::{cell::Cell, ops::Range, time::Duration};

use gpui_kit::{
    Animation, AnimationExt, AnyElement, Context, Keystroke, MouseButton, ObjectFit, ScrollHandle, SharedString, Window, component::Icon, div, img, point, prelude::*, px,
};

use crate::{
    Waffle,
    discord::{self, Media, MediaKind},
    icons::Glyph,
    layout::TITLE_HEIGHT,
    ui::{self, Squircle},
};

const THUMB: f32 = 56.;
const GAP: f32 = 8.;
const STRIDE: f32 = THUMB + GAP;
// Room around the thumbnails for the selection ring, which sits RING_OUT outside the current one.
const PAD: f32 = 4.;
const RING_OUT: f32 = 3.;
// Thumbnails loaded either side of the visible stretch, so a short scroll finds them ready.
const MARGIN: usize = 4;
// Space the stage leaves for the top buttons and for the counter and strip below it.
const STAGE_INSET: (f32, f32) = (200., 220.);

pub(crate) struct Viewer {
    pub(crate) items: Vec<Media>,
    pub(crate) index: usize,
    // Where the selection ring slides from.
    from: usize,
    strip: ScrollHandle,
    // Scroll the current thumbnail into view on the next frame the strip has a size.
    reveal: Cell<bool>,
    // Thumbnails asked for last frame, so the ones scrolled away can be dropped.
    loaded: Cell<Range<usize>>,
    channel_id: String,
    // Messages in the whole channel carrying an image or a video, from Discord's search; None until it answers.
    // GAP: search counts messages, not files, so a message with several images counts once.
    channel_total: Option<usize>,
}

impl Waffle {
    pub(crate) fn open_viewer(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(channel) = self.channel.clone() else { return };
        let Some(feed) = self.feeds.get(&channel.id) else { return };
        let items = feed_media(&feed.messages);
        let Some(index) = items.iter().position(|m| m.url == url) else { return };
        self.open_viewer_on(items, index, channel.id.clone(), window, cx);
        self.count_channel_media(&channel, cx);
    }

    // The viewer at `items[index]`, paging through `items` alone. A Components V2 gallery passes no
    // `channel_id`, so the channel's history never replaces its items.
    pub(crate) fn open_viewer_on(&mut self, items: Vec<Media>, index: usize, channel_id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.viewer = Some(Viewer { items, index, from: index, strip: ScrollHandle::new(), reveal: Cell::new(true), loaded: Cell::new(0..0), channel_id, channel_total: None });
        window.focus(&self.focus, cx);
        cx.notify();
    }

    // Asks Discord's search how many messages in the channel carry an image and a video, for the counter.
    fn count_channel_media(&mut self, channel: &discord::Channel, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let scope = match &self.guild_id {
            Some(guild) => format!("guilds/{guild}"),
            None => format!("channels/{}", channel.id),
        };
        let channel_id = channel.id.clone();
        let params = |has: &str| {
            let mut params = vec![("has".to_owned(), has.to_owned())];
            if scope.starts_with("guilds/") {
                params.push(("channel_id".to_owned(), channel_id.clone()));
            }
            params
        };
        let (images, videos) = (params("image"), params("video"));
        cx.spawn(async move |this, cx| {
            let (images, videos) = futures::join!(client.search(&scope, "", &images), client.search(&scope, "", &videos));
            let (Ok(images), Ok(videos)) = (images, videos) else { return };
            this.update(cx, |this, cx| {
                let Some(viewer) = this.viewer.as_mut().filter(|v| v.channel_id == channel_id) else { return };
                viewer.channel_total = Some((images.total + videos.total) as usize);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // The open channel's history grew: the viewer takes in what was added, keeping its place.
    pub(crate) fn refresh_viewer(&mut self, cx: &mut Context<Self>) {
        let Some(viewer) = &mut self.viewer else { return };
        let Some(feed) = self.feeds.get(&viewer.channel_id) else { return };
        let items = feed_media(&feed.messages);
        let Some(current) = viewer.items.get(viewer.index).map(|m| m.url.clone()) else { return };
        let Some(index) = items.iter().position(|m| m.url == current) else { return };
        viewer.from = viewer.from + index - viewer.index.min(index);
        viewer.index = index;
        viewer.items = items;
        viewer.reveal.set(true);
        cx.notify();
    }

    pub(crate) fn close_viewer(&mut self, cx: &mut Context<Self>) {
        if let Some(key) = self.viewer.as_ref().and_then(Viewer::video_key) {
            crate::video::pause(&key, cx);
        }
        self.viewer = None;
        cx.notify();
    }

    pub(crate) fn step_viewer(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(viewer) = &self.viewer else { return };
        // Nearing the oldest loaded, older history loads so stepping back carries on.
        if delta < 0 && viewer.index as isize + delta <= MARGIN as isize {
            self.load_older(cx);
        }
        let Some(viewer) = &self.viewer else { return };
        let last = viewer.items.len() as isize - 1;
        self.show_in_viewer((viewer.index as isize + delta).clamp(0, last) as usize, cx);
    }

    fn show_in_viewer(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(viewer) = &mut self.viewer else { return };
        if index == viewer.index {
            return;
        }
        let leaving = viewer.video_key();
        viewer.from = viewer.index;
        viewer.index = index;
        viewer.reveal.set(true);
        if let Some(key) = leaving {
            crate::video::pause(&key, cx);
        }
        cx.notify();
    }

    // ←/→ step, Space plays or pauses a video, Shift+←/→ skip it 5s, M mutes it, Esc closes. True if handled.
    pub(crate) fn on_viewer_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(viewer) = &self.viewer else { return false };
        let video = viewer.video_key();
        match (keystroke.key.as_str(), keystroke.modifiers.shift, video) {
            ("left" | "right", true, Some(key)) => crate::video::skip(&key, keystroke.key == "right", cx),
            ("left", _, _) => self.step_viewer(-1, cx),
            ("right", _, _) => self.step_viewer(1, cx),
            ("space", _, Some(key)) => crate::video::toggle(&key, cx),
            ("m", _, Some(key)) => crate::video::toggle_mute(&key, cx),
            ("escape", _, _) => self.close_viewer(cx),
            _ => return false,
        }
        true
    }

    pub(crate) fn viewer_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let viewer = self.viewer.as_ref()?;
        let item = viewer.items.get(viewer.index)?.clone();
        let (index, count) = (viewer.index, viewer.items.len());
        // Counted from the newest, as the media is: the channel's older media sits before what's loaded.
        let total = viewer.channel_total.map_or(count, |total| total.max(count));
        let position = total - (count - index) + 1;
        let viewport = window.viewport_size();
        let viewport = (f32::from(viewport.width), f32::from(viewport.height));
        let (w, h) = stage_size(&item, viewport);
        let stage = match &item.kind {
            MediaKind::Image => crate::chat::animated_image(discord::sized_image(&item.url, w, h, (item.width, item.height))).w(px(w)).h(px(h)).squircle(ui::RADIUS_MD).into_any_element(),
            MediaKind::Gif { poster } => div().w(px(w)).h(px(h)).squircle(ui::RADIUS_MD).overflow_hidden().child(crate::gif::top_tile(format!("viewer-{}", item.url), &item.url, poster, gpui_kit::size(px(w), px(h)), ui::all(ui::RADIUS_MD), cx)).into_any_element(),
            MediaKind::Video { poster } => div().w(px(w)).h(px(h)).child(crate::video::tile(video_key(&item), &item.url, poster, cx)).into_any_element(),
        };
        let stage = div()
            .id("viewer-stage")
            .relative()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(stage)
            .children(neighbours(viewer, viewport))
            .with_animation(SharedString::from(format!("viewer-{index}")), Animation::new(Duration::from_millis(140)), |el, t| el.opacity(t));
        let strip = self.strip(viewer, viewport.0 - 96., window, cx);
        let nav = |id: &'static str, glyph: Glyph, enabled: bool, delta: isize| {
            div()
                .id(id)
                .size(px(48.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui_kit::white().opacity(if enabled { 0.12 } else { 0.04 }))
                .text_color(gpui_kit::white().opacity(if enabled { 1. } else { 0.3 }))
                .when(enabled, |b| b.cursor_pointer().hover(|s| s.bg(gpui_kit::white().opacity(0.2))))
                .child(Icon::new(glyph).size(px(26.)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, _, cx| this.step_viewer(delta, cx)))
        };
        let url = item.url.clone();
        let top = div()
            .absolute()
            .top(px(TITLE_HEIGHT + 8.))
            .right(px(16.))
            .flex()
            .gap_2()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(top_button("viewer-open").px_3().gap_1p5().child(Icon::new(Glyph::Open).size(px(16.))).child("Open in Browser").on_click(move |_, _, cx| cx.open_url(&url)))
            .child(top_button("viewer-close").w(px(32.)).justify_center().child(Icon::new(Glyph::X).size(px(18.))).on_click(cx.listener(|this, _, _, cx| this.close_viewer(cx))));
        let foot = div()
            .absolute()
            .bottom(px(16.))
            .left_0()
            .right_0()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .child(div().text_size(px(14.)).text_color(gpui_kit::white().opacity(0.8)).child(format!("{position} / {total}")))
            .child(strip);
        Some(
            div()
                .id("viewer")
                .absolute()
                .occlude()
                .inset_0()
                .bg(gpui_kit::black().opacity(0.88))
                .flex()
                .items_center()
                .justify_between()
                .px_6()
                .pb(px(STAGE_INSET.1 / 2. - 50.))
                // Keys reach here first while the video has focus (it takes focus when clicked).
                .capture_key_down(cx.listener(|this, event: &gpui_kit::KeyDownEvent, _, cx| {
                    if this.on_viewer_key(&event.keystroke, cx) {
                        cx.stop_propagation();
                    }
                }))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_viewer(cx)))
                .child(nav("viewer-prev", Glyph::ChevronLeft, index > 0, -1))
                .child(stage)
                .child(nav("viewer-next", Glyph::ChevronRight, index + 1 < count, 1))
                .child(top)
                .child(foot)
                .with_animation("viewer-open", Animation::new(Duration::from_millis(160)), |el, t| el.opacity(t))
                .into_any_element(),
        )
    }

    // Every item as a thumbnail in a quiet row, scrolling sideways, the current one ringed. Only the
    // visible stretch (plus MARGIN either side) loads its picture; the rest are blank tiles.
    fn strip(&self, viewer: &Viewer, max_width: f32, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let count = viewer.items.len();
        let content = content_width(count);
        let width = content.min(max_width);
        let bounds = viewer.strip.bounds();
        let view = f32::from(bounds.size.width);
        if viewer.reveal.get() && view > 0. {
            viewer.reveal.set(false);
            let scroll = -f32::from(viewer.strip.offset().x);
            if let Some(to) = reveal_offset(viewer.index, scroll, view, content) {
                viewer.strip.set_offset(point(px(-to), px(0.)));
            }
        }
        let scroll = -f32::from(viewer.strip.offset().x);
        let range = visible_range(scroll, view, count, viewer.index);
        let side = THUMB * window.scale_factor();
        let gone = viewer.loaded.replace(range.clone());
        let dropped: Vec<String> = gone.filter(|i| !range.contains(i)).filter_map(|i| viewer.items.get(i)).map(|m| discord::thumb_url(m, side)).collect();
        if !dropped.is_empty() {
            crate::images::cache(cx).update(cx, |images, _| dropped.iter().for_each(|url| images.cancel(url)));
        }
        let thumbs = viewer.items.iter().enumerate().map(|(i, item)| {
            let current = i == viewer.index;
            let picture = range.contains(&i).then(|| img(discord::thumb_url(item, side)).absolute().inset_0().size(px(THUMB)).object_fit(ObjectFit::Cover).squircle(ui::RADIUS_XS));
            let is_video = matches!(item.kind, MediaKind::Video { .. });
            div()
                .id(("viewer-thumb", i))
                .relative()
                .size(px(THUMB))
                .flex_none()
                .overflow_hidden()
                .squircle(ui::RADIUS_XS)
                .bg(gpui_kit::white().opacity(0.06))
                .cursor_pointer()
                .opacity(if current { 1. } else { 0.55 })
                .when(!current, |t| t.hover(|s| s.opacity(0.85)))
                .children(picture)
                .when(is_video, |t| t.child(play_mark()))
                .on_click(cx.listener(move |this, _, _, cx| this.show_in_viewer(i, cx)))
        });
        let (from, to) = (thumb_x(viewer.from), thumb_x(viewer.index));
        let ring = div()
            .absolute()
            .top(px(PAD - RING_OUT))
            .left(px(to - RING_OUT))
            .size(px(THUMB + 2. * RING_OUT))
            .squircle(ui::RADIUS_XS + RING_OUT)
            .border_2()
            .border_color(gpui_kit::white())
            .with_animation(
                SharedString::from(format!("viewer-ring-{}-{}", viewer.from, viewer.index)),
                Animation::new(Duration::from_millis(200)).with_easing(|t| crate::motion::ease_move().sample(t)),
                move |ring, t| ring.left(px(from + (to - from) * t - RING_OUT)),
            );
        div()
            .id("viewer-strip")
            .w(px(width))
            .h(px(THUMB + 2. * PAD))
            .overflow_x_scroll()
            .track_scroll(&viewer.strip)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().relative().w(px(content)).h_full().p(px(PAD)).flex().gap(px(GAP)).children(thumbs).child(ring))
            .into_any_element()
    }
}

impl Viewer {
    // The player for the current item when it's a video.
    fn video_key(&self) -> Option<String> {
        self.items.get(self.index).filter(|m| matches!(m.kind, MediaKind::Video { .. })).map(video_key)
    }
}

// Its own player, apart from the one in the chat, so the two never draw one entity twice.
fn video_key(item: &Media) -> String {
    format!("viewer-{}", item.url)
}

// Fit within the window less the room for the controls, at most 2x the original.
fn stage_size(item: &Media, viewport: (f32, f32)) -> (f32, f32) {
    let (max_w, max_h) = (viewport.0 - STAGE_INSET.0, viewport.1 - STAGE_INSET.1);
    let scale = (max_w / item.width.max(1.)).min(max_h / item.height.max(1.)).min(2.);
    (item.width * scale, item.height * scale)
}

// The items either side of the current one, loaded unseen at their stage size so stepping shows them at once.
fn neighbours(viewer: &Viewer, viewport: (f32, f32)) -> Vec<AnyElement> {
    let around = [viewer.index.checked_sub(1), viewer.index.checked_add(1)];
    around
        .into_iter()
        .flatten()
        .filter_map(|i| viewer.items.get(i))
        .map(|item| {
            let (w, h) = stage_size(item, viewport);
            let url = match &item.kind {
                MediaKind::Image => discord::sized_image(&item.url, w, h, (item.width, item.height)),
                MediaKind::Video { poster } | MediaKind::Gif { poster } => poster.clone(),
            };
            img(url).absolute().size(px(1.)).opacity(0.).into_any_element()
        })
        .collect()
}

fn top_button(id: &'static str) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .h(px(32.))
        .squircle(ui::RADIUS_SM)
        .flex()
        .items_center()
        .cursor_pointer()
        .text_color(gpui_kit::white())
        .bg(gpui_kit::white().opacity(0.12))
        .hover(|s| s.bg(gpui_kit::white().opacity(0.2)))
}

fn play_mark() -> impl IntoElement {
    div().absolute().inset_0().flex().items_center().justify_center().child(
        div().size(px(24.)).rounded_full().bg(gpui_kit::black().opacity(0.6)).flex().items_center().justify_center().text_color(gpui_kit::white()).child(Icon::new(Glyph::Play).size(px(14.))),
    )
}

fn content_width(count: usize) -> f32 {
    count as f32 * STRIDE - GAP + 2. * PAD
}

fn thumb_x(index: usize) -> f32 {
    PAD + index as f32 * STRIDE
}

// The thumbnails to load: those within the visible stretch, MARGIN more either side. Before the strip
// has been laid out (`view` 0) it's the ones around `current`.
fn visible_range(scroll: f32, view: f32, count: usize, current: usize) -> Range<usize> {
    let (first, last) = if view > 0. {
        (((scroll - PAD) / STRIDE).floor().max(0.) as usize, ((scroll + view - PAD) / STRIDE).ceil().max(0.) as usize)
    } else {
        (current, current + 1)
    };
    first.saturating_sub(MARGIN).min(count)..(last + MARGIN).min(count)
}

// Where to scroll so thumbnail `index` shows whole: None when it already does, else centred, kept
// within the strip's ends.
fn reveal_offset(index: usize, scroll: f32, view: f32, content: f32) -> Option<f32> {
    let (start, end) = (thumb_x(index) - RING_OUT, thumb_x(index) + THUMB + RING_OUT);
    if start >= scroll && end <= scroll + view {
        return None;
    }
    let centred = thumb_x(index) + THUMB / 2. - view / 2.;
    Some(centred.clamp(0., (content - view).max(0.)))
}

// Every image and video in a feed, oldest first: attachments, forwarded ones, and embed images.
fn feed_media(messages: &[discord::Message]) -> Vec<Media> {
    messages.iter().flat_map(|m| std::iter::once(m).chain(m.forward.as_deref())).flat_map(|m| m.media.iter().chain(m.embeds.iter().filter(|e| !e.video).flat_map(|e| &e.images)).cloned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_the_visible_stretch_and_a_margin() {
        // 400px shows thumbnails 0..=6 (the 7th starts at 388); MARGIN more after, none before 0.
        assert_eq!(visible_range(0., 400., 100, 0), 0..11);
        // Scrolled 640px: the first visible is 9 (starts at 580, ends 636 + gap), minus MARGIN.
        assert_eq!(visible_range(640., 400., 100, 0), 5..21);
        assert_eq!(visible_range(5000., 400., 20, 0), 20..20, "past the end loads nothing");
        assert_eq!(visible_range(0., 0., 20, 10), 6..15, "before layout: MARGIN either side of the current one");
        assert_eq!(visible_range(0., 0., 3, 1), 0..3);
    }

    #[test]
    fn reveals_only_what_is_out_of_view() {
        let content = content_width(20);
        assert_eq!(reveal_offset(2, 0., 400., content), None, "already whole on screen");
        assert_eq!(reveal_offset(10, 0., 400., content), Some(thumb_x(10) + THUMB / 2. - 200.), "centred");
        assert_eq!(reveal_offset(19, 0., 400., content), Some(content - 400.), "kept within the end");
        assert_eq!(reveal_offset(0, 300., 400., content), Some(0.), "kept within the start");
        assert_eq!(reveal_offset(6, 0., 400., content), Some(thumb_x(6) + THUMB / 2. - 200.), "a cut-off one counts as out of view");
    }

    #[test]
    fn stage_fits_the_window_without_passing_2x() {
        let media = |w, h| Media { url: String::new(), width: w, height: h, kind: MediaKind::Image };
        assert_eq!(stage_size(&media(4000., 2000.), (1400., 900.)), (1200., 600.));
        assert_eq!(stage_size(&media(100., 100.), (1400., 900.)), (200., 200.));
    }
}
