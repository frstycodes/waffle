// The middle column: channel header, transcript and composer, ported from design/messages.html and
// design/navigation.html (channel header).
//
// GAP: hover fades (row fill, toolbar, 150ms) are instant: gpui's `.hover` styles don't transition.
use std::{cell::Cell, collections::HashMap, rc::Rc, time::Duration};

use chrono::{DateTime, Local, NaiveDate};
use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, Bounds, Context, Corners, Div, Focusable, FontWeight, HighlightStyle, Hsla, KeyDownEvent, Pixels, SharedString, Stateful, StyledText, WeakEntity, Window,
    WindowControlArea,
    component::{
        ActiveTheme, Icon, Sizable, Size,
        input::{Input, Textarea},
        text::{InlineElement, InlineRenderContext, MarkdownExtensions, MarkdownNode, MarkdownParseContext, MarkdownPlugin, TextView, markdown_ast},
    },
    ListOffset, div, img, linear_color_stop, linear_gradient, list, point, prelude::*, px,
};
use gpui_motion_kit::core::{Motion, Target};

pub(crate) use crate::emphasis::emphasised;
use crate::{
    EmojiTarget, Feed, Waffle, components,
    discord::{Channel, ChannelKind, Embed, Emoji, MediaKind, Message, NameEffect, NameStyle, Poll, PollAnswer, SendState, User},
    hidden::{Hidden, Run},
    icons::Glyph,
    layout::HEADER_HEIGHT,
    mascot::Mood,
    motion,
    overlays::ComposerPicker,
    palette::{self, Level},
    settings::RoleStyle,
    system_message::{Sentence, notice_line},
    ui::{self, RADIUS_LG, RADIUS_MD, RADIUS_SM, RADIUS_XS, Squircle},
};

mod attachments;
mod composer;
mod embeds;
mod format;
mod history;
mod markdown;
mod media;
mod parts;
mod row;

pub(crate) use attachments::*;
pub(crate) use embeds::*;
pub(crate) use format::*;
pub(crate) use history::*;
pub(crate) use markdown::*;
pub(crate) use media::*;
pub(crate) use parts::*;
pub(crate) use row::*;

// How long a jumped-to message stays marked: its fill and squiggle (the flash in `jump_to`).
pub(crate) const JUMP_FLASH: Duration = Duration::from_millis(2400);

// Messages from one author closer together than this share a header, like Discord.
const GROUP_GAP_SECS: i64 = 7 * 60;
// A lone image fits inside this; galleries and cards fill the media column.
const MAX_IMAGE: (f32, f32) = (400., 320.);
const COLUMN: f32 = 432.;
// Text sizes: a message's body, Discord's "-# " subtext, an embed's text.
pub(crate) const BODY_TEXT: f32 = 16.;
const SUBTEXT: f32 = 14.;
const EMBED_TEXT: f32 = 14.;
const GALLERY_GAP: f32 = 4.;
// A Components V2 gallery's lone item spans its container, up to this tall.
const GALLERY_MAX_H: f32 = 360.;

pub(crate) const AVATAR: f32 = 40.;
// The avatar column plus the gap to the content column: where replies and continuations line up.
pub(crate) const CONTENT_INSET: f32 = AVATAR + GUTTER;
// The unread bar's room at the feed's top: 8 above it, its 36, 8 below.
const UNREAD_BAR_SPACE: f32 = 52.;
// A reply preview's line, above the message it answers.
const REPLY_HEIGHT: f32 = 20.;
// A forwarded copy's card, as the design's `.fwd`.
const FORWARD_WIDTH: f32 = 520.;

// The floating stream player's width; its height keeps 16:9.

impl Waffle {
    pub fn render_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let main = ui::col().min_h_0().relative().flex_1().h_full().bg(palette::surface(Level::Page, cx));
        let Some(channel) = self.channel.clone() else {
            return main.child(empty_state("idle", Mood::Idle, "No channel open", "Pick a server on the left, then a channel to start reading.", cx));
        };
        let header = self.channel_header(&channel, window, cx);
        let unread_bar = self.unread_bar(&channel.id, cx);
        // The feed starts below the unread bar, so it never covers the first rows: opened on its first unread,
        // the NEW line sits just under the bar.
        let top = if unread_bar.is_some() { UNREAD_BAR_SPACE } else { 0. };
        let body = match self.feeds.get(&channel.id) {
            Some(Feed { loaded: true, messages, .. }) if messages.is_empty() => match self.history_start(&channel, false) {
                // Nothing said yet: the header rests at the foot of the feed, where the first message will land.
                Some(start) => ui::col().size_full().justify_end().pt(px(top)).pb(px(self.composer_height.get() + 32.)).child(crate::dm_start::channel_start::history_start_header(&start, cx.weak_entity(), cx)).into_any_element(),
                None => empty_state("empty", Mood::Happy, "No messages yet", "Be the first to say something.", cx),
            },
            Some(Feed { loaded: true, .. }) => self.render_messages(top, cx).into_any_element(),
            Some(Feed { error: Some(error), .. }) => empty_state("failed", crate::illustration::Illustration::LoadFailed, "Couldn't load messages", error.clone(), cx),
            _ => skeleton(cx),
        };
        let gate = self.composer_gate(&channel, window, cx);
        let composer = match gate.or_else(|| self.follow_blocker(&channel, cx)) {
            Some(blocker) => blocker,
            None => self.render_composer(window, cx).into_any_element(),
        };
        let (inline, above) = self.place_header(header);
        let chat = ui::col().min_h_0()
            .relative()
            .flex_1()
            .h_full()
            .key_context("Chat")
            .on_action(cx.listener(Self::focus_search))
            // The feed stops under the header, a hard edge. A DM's profile sits under the header instead of beside it.
            .children(inline)
            // The composer floats over the feed, which scrolls on under its glass.
            .child(div().relative().flex_1().min_h_0().child(body).children(unread_bar).child(composer));
        // The members column beside the feed runs to the window's top with a header of its own on the same line.
        let panel = self.panel_column_width().map(|w| self.part(crate::part::Column::Panel, gpui_kit::StyleRefinement::default().w(px(w)).h_full().flex_none(), cx));
        let drop_zone = self.drop_zone(&channel, cx);
        main.children(above).child(ui::row().items_stretch().flex_1().min_h_0().child(chat.child(drop_zone)).children(panel))
    }

    // Where you are as breadcrumbs, the topic, then the channel's tools (design: navigation.html's chanbar).
    fn channel_header(&self, channel: &Channel, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let (strong, faint, selected) = (palette::strong(cx), palette::faint(cx), palette::selected(cx));
        let parent = self.thread_parent(channel).map(|parent| {
            crumb("thread-parent", cx)
                .child(Icon::new(Glyph::Hashtag).size(ui::ICON_SM))
                .child(ui::truncating("header-thread-parent", parent.name.clone()))
                .on_click(cx.listener(move |this, _, window, cx| this.open_channel(parent.clone(), window, cx)))
        });
        let lead = match channel.kind {
            ChannelKind::Dm => ui::dm_face(&channel.face(), 24., palette::surface(Level::Page, cx)),
            ChannelKind::Thread => Icon::new(Glyph::Thread).size(ui::ICON_XL).text_color(faint).into_any_element(),
            _ if channel.raw_type == 5 => Icon::new(Glyph::Megaphone).size(ui::ICON_XL).text_color(faint).into_any_element(),
            _ => Icon::new(Glyph::Hashtag).size(ui::ICON_XL).text_color(faint).into_any_element(),
        };
        let here = ui::row()
            .h_8()
            .px_1p5()
            .gap_1p5()
            .text_size(ui::TEXT_MD)
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(strong)
            .child(lead)
            .child(ui::truncating("header-channel-name", channel.name.clone()));
        // Thread names are often whole sentences: the crumbs cap at 60% and truncate rather than push the tools off.
        let crumbs = ui::row()
            .flex_none()
            .max_w(gpui_kit::relative(0.6))
            .gap_0p5()
            .when_some(parent, |c, parent| c.child(parent).child(Icon::new(Glyph::ChevronRight).size(ui::ICON_SM).text_color(faint)))
            .child(here);
        let topic = channel.topic.clone().map(|topic| div().flex_1().min_w_0().pl_3().border_l_1().border_color(palette::line_strong(cx)).text_size(ui::TEXT_SM).text_color(palette::muted(cx)).child(ui::truncating("header-topic", topic)));
        let threads = (channel.kind == ChannelKind::Text && self.guild_id.is_some()).then(|| {
            let id = channel.id.clone();
            ui::icon_button("threads-button", Glyph::Thread, "Threads", cx)
                .occlude()
                .on_mouse_down(gpui_kit::MouseButton::Left, cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_threads_popout(id.clone(), e.position, cx)))
        });
        // Mute and notification level for this channel, the same entries as its right-click menu; the bell
        // shows a slash while it's muted. Shift+click mutes or unmutes without the menu, as Discord's does.
        let bell = self.guild_id.clone().filter(|_| matches!(channel.kind, ChannelKind::Text | ChannelKind::Voice)).map(|guild_id| {
            let muted = self.guild_settings.get(&guild_id).is_some_and(|s| s.channel_muted(&channel.id));
            let (label, glyph) = if muted { ("Unmute or change notifications", Glyph::BellSlash) } else { ("Notification Settings", Glyph::Bell) };
            let open = channel.clone();
            ui::icon_button("notify-button", glyph, label, cx)
                .occlude()
                .on_mouse_down(gpui_kit::MouseButton::Left, cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| {
                    if e.modifiers.shift {
                        return this.patch_channel_override(guild_id.clone(), &open.id, serde_json::json!({ "muted": !muted }), cx);
                    }
                    this.open_notify_menu(open.clone(), guild_id.clone(), e.position, cx)
                }))
        });
        let pins_x = self.pins_button_x.clone();
        let pins = ui::icon_button("pins-button", Glyph::Pin, "Pinned Messages", cx)
            .occlude()
            .child(gpui_kit::canvas(move |bounds, _, _| pins_x.set(f32::from(bounds.center().x)), |_, _, _, _| {}).absolute().size_full())
            .when(self.pins.is_some(), |b| b.bg(selected).text_color(strong))
            .on_mouse_down(gpui_kit::MouseButton::Left, cx.listener(|this, e: &gpui_kit::MouseDownEvent, _, cx| {
                this.popover_at = e.position;
                this.open_pins(cx);
            }));
        // Discord's two call buttons; hidden once we're in this DM's call (its window holds it), and while a
        // call goes on here without us (the Join pill takes their place).
        let join = self.dm_join_pill(channel, cx);
        let can_call = channel.kind == ChannelKind::Dm && join.is_none() && self.call.as_ref().is_none_or(|c| c.channel.id != channel.id);
        let calls = can_call.then(|| {
            let (voice, video) = (channel.clone(), channel.clone());
            [
                ui::icon_button("dm-voice-call", Glyph::Phone, "Start Voice Call", cx).occlude().on_click(cx.listener(move |this, _, _, cx| this.start_dm_call(voice.clone(), false, cx))),
                ui::icon_button("dm-video-call", Glyph::Video, "Start Video Call", cx).occlude().on_click(cx.listener(move |this, _, _, cx| this.start_dm_call(video.clone(), true, cx))),
            ]
        });
        let add_members = (channel.kind == ChannelKind::Dm).then(|| {
            let tip = if channel.raw_type == 3 { "Add Members" } else { "Add Friends to DM" };
            let from = channel.clone();
            ui::icon_button("dm-add-members", Glyph::GroupPlus, tip, cx).occlude().on_click(cx.listener(move |this, _, window, cx| this.open_group_picker(Some(from.clone()), window, cx)))
        });
        let members = self.members_toggle(channel, cx);
        let search = Some(self.channel_search(window, cx));
        // Occludes, like the composer: the feed scrolls under the header and must not take its hover or clicks.
        let bar = ui::row()
            .occlude()
            .h(px(HEADER_HEIGHT))
            .pl_2()
            .pr_2()
            .gap_2()
            // Drag area: every control inside it occludes it, or Windows' caption takes its clicks.
            .window_control_area(WindowControlArea::Drag)
            .child(crumbs)
            // The topic stretches; without one, a spacer keeps the tools at the far end.
            .child(topic.unwrap_or_else(|| div().flex_1().min_w_0()))
            .child(ui::row().flex_none().gap_0p5().children(self.follow_button(channel, cx)).children(join).children(calls.into_iter().flatten()).children(threads).children(bell).child(pins).children(add_members).children(members).children(search))
            .child(self.caption_clearance(!self.panel_shown()));
        ui::hard_edge(|| false, cx).child(bar)
    }

    // The header's search: one element that is a search button when closed and opens into the field, open
    // while it has focus or a query. Its width and fill run both ways on a quick ease-out, but only on a change:
    // present at mount, it simply is what it is.
    fn channel_search(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // Closed it's the search box's icon alone: 12px padding either side of the 16px glyph; open, room for filter chips.
        const CLOSED: f32 = 40.;
        const OPEN: f32 = 260.;
        // How far under the field the popover hangs.
        const SEARCH_POPOVER_TOP: f32 = 46.;
        let handle = self.search_input.read(cx).focus_handle(cx);
        let focused = handle.is_focused(window);
        let open = focused || !self.search_input.read(cx).value().is_empty();
        // (open when last drawn, how many times it has changed): the count keys the animation, so one change
        // plays once and later frames keep its end state.
        let state = window.use_keyed_state("search-open-state", cx, |_, _| (open, 0usize));
        let (was_open, changes) = *state.read(cx);
        if was_open != open {
            state.update(cx, |s, _| *s = (open, changes + 1));
        }
        let changes = state.read(cx).1;
        let hover = palette::hover(cx);
        let avatars = self.search_avatars();
        let el = div()
            .id("search-field")
            .w(px(if open { OPEN } else { CLOSED }))
            // lint-allow(raw-margin): an optical 4px past the tool buttons, which the row's own gap_0p5 can't give one child
            .ml_1()
            .flex_none()
            .occlude()
            .squircle(RADIUS_MD)
            .child(ui::search_frame("channel-search", Input::new(&self.search_input).token(move |token, _, cx| crate::search_menu::render_token(token, &avatars, cx)), &handle).quiet(!open))
            .capture_key_down(cx.listener(Self::on_search_key))
            .when(!open, |d| {
                d.cursor_pointer().hover(move |s| s.bg(hover)).on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&handle, cx);
                    this.redraw(crate::part::Column::Main, cx);
                }))
            });
        let el = if changes == 0 {
            el.into_any_element()
        } else {
            let (from, to) = if open { (CLOSED, OPEN) } else { (OPEN, CLOSED) };
            el.with_animation(SharedString::from(format!("search-{changes}")), Animation::new(Duration::from_millis(150)).with_easing(gpui_kit::ease_out_quint()), move |el, t| el.w(px(from + (to - from) * t)))
                .into_any_element()
        };
        // The popover hangs from the field's right edge and rises from that corner.
        // Deferred and absolute, the popover lies outside this element's bounds, so mouse-down-out fires on a
        // press inside it too; its last drawn bounds let that press through instead of blurring and unmounting it.
        let popover_bounds = window.use_keyed_state("search-popover-bounds", cx, |_, _| None::<Bounds<Pixels>>);
        let popover = focused.then(|| self.search_popover(cx)).flatten().map(|card| {
            let bounds = popover_bounds.clone();
            let card = div().absolute().top(px(SEARCH_POPOVER_TOP)).right_0().on_children_prepainted(move |children, _, cx| bounds.update(cx, |b, _| *b = children.first().copied())).child(card);
            // lint-allow(raw-floating): hangs at absolute offsets from the field, not anchored; the card occludes itself.
            gpui_kit::deferred(ui::rise(card.transform_origin(ui::corner(true, false)), "search-popover-in", Duration::ZERO)).with_priority(2)
        });
        if popover.is_none() {
            popover_bounds.update(cx, |b, _| *b = None);
        }
        div()
            .relative()
            .flex_none()
            .on_mouse_down_out(cx.listener(move |_, _, window, cx| {
                let in_popover = popover_bounds.read(cx).is_some_and(|b| b.contains(&window.mouse_position()));
                if focused && !in_popover {
                    window.blur(cx);
                }
            }))
            .child(el)
            .children(popover)
            .into_any_element()
    }

    // The bar over the feed while unread messages wait: jump to the first, or mark the channel read.
    fn unread_bar(&self, channel_id: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        let read = self.unread_marker?;
        // Gone once the channel is read (a jump, Mark as read, or Discord's ack from another client).
        if !self.unread(channel_id) {
            return None;
        }
        let feed = self.feeds.get(channel_id)?;
        // What you sent yourself is never news to you.
        let me = self.my_id();
        let fresh: Vec<&Message> = feed.messages.iter().filter(|m| crate::order(&m.id) > read && m.author.id != me).collect();
        let first = fresh.first()?;
        // Not every unread is loaded while the oldest loaded one is still unread: Discord's count then reads "N+".
        let estimated = feed.has_more && feed.messages.first().is_some_and(|m| crate::order(&m.id) > read);
        let label = unread_label(fresh.len(), estimated, &first.timestamp);
        let tint = ui::mix(palette::surface(Level::Page, cx), palette::accent(), 0.16);
        let press = palette::accent().opacity(0.14);
        // lint-allow(custom-button): ui::button has no tone that washes the accent over a tinted bar
        let button = move |id: &'static str| ui::row().id(id).h_7().px_2().gap_1p5().squircle(RADIUS_XS).cursor_pointer().hover(move |s| s.bg(press));
        let (first_id, channel_id) = (first.id.clone(), channel_id.to_owned());
        Some(
            // lint-allow(custom-badge, custom-mark): ui::callout is inline, not floating over a list; ui::floating_bar docks at the foot, this one hangs from the feed's top
            ui::row()
                .id("unread-bar")
                // Floats over the feed: the messages under it must not take its hover or clicks.
                .occlude()
                .absolute()
                .top_2()
                .left_2()
                .right_2()
                .min_h_9()
                .pl_3()
                .pr_1()
                .gap_2()
                .squircle(RADIUS_SM)
                .bg(tint)
                .text_size(ui::TEXT_SM)
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::accent_ink(cx))
                .child(
                    button("unread-jump")
                        .flex_1()
                        .min_w_0()
                        // lint-allow(raw-margin): optical, pulls the jump button's wash to the bar's edge past its pl_3
                        .ml_neg_2()
                        .child(Icon::new(Glyph::ArrowUp).size(ui::ICON_MD))
                        .child(ui::line(label))
                        // Discord's body only jumps to the first unread; reading it there is what marks it read.
                        .on_click(cx.listener(move |this, _, _, cx| this.jump_to(first_id.clone(), cx))),
                )
                // Read now; the NEW line stays until the channel is left or you post, as Discord's does.
                .child(button("unread-read").child("Mark As Read").child(Icon::new(Glyph::Checkmark).size(ui::ICON_MD)).on_click(cx.listener(move |this, _, _, cx| {
                    this.mark_channel_read(&channel_id, cx);
                    // lint-allow(global-notify): reading a channel clears its unread bar here and its dot in the nav column
                    cx.notify();
                })))
                .into_any_element(),
        )
    }

    /// `top`: how far a header floating over the feed reaches down; the first message rests below it.
    pub(crate) fn render_messages(&self, top: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(channel) = &self.channel else { return div().into_any_element() };
        let Some(feed) = self.feeds.get(&channel.id) else { return div().into_any_element() };
        // A pointer copy: the feed is copy-on-write, so this frame shares it rather than cloning every message.
        let messages = feed.messages.clone();
        let ctx = self.row_context(&messages, Some(channel), feed.has_more, COLUMN, cx);
        let folds = self.folds(&messages);
        self.refold(&channel.id, &messages, &folds);
        let arrivals = self.arrivals.clone();
        let messages = list(self.list.clone(), move |ix, window, cx| {
            // Near the oldest loaded message, the next page loads; `load_older` keeps it to one at a time.
            if ix < PREFETCH_ROWS && ctx.has_more {
                let this = ctx.this.clone();
                cx.defer(move |cx| {
                    this.update(cx, |this, cx| this.load_older(cx)).ok();
                });
            }
            let message = &messages[ix];
            let previous = ix.checked_sub(1).map(|i| &messages[i]);
            let new_day = day(&message.timestamp) != previous.and_then(|p| day(&p.timestamp));
            let starts_group = new_day || message.reply.is_some() || message.reply_deleted || previous.is_none_or(|p| crate::system_message::is_notice(p) || !continues(p, message));
            let row = match crate::hidden::run_at(&folds, ix) {
                None => feed_row(&messages, ix, starts_group, &ctx, cx),
                Some(run) => folded_row(&messages, ix, run, starts_group, &ctx, cx),
            };
            // One accent line above the first message after your read position.
            let first_unread = ctx.unread_marker.is_some_and(|read| crate::order(&message.id) > read && previous.is_none_or(|p| crate::order(&p.id) <= read));
            let start = ctx.history_start.as_ref().filter(|_| ix == 0).map(|s| crate::dm_start::channel_start::history_start_header(s, ctx.this.clone(), cx));
            // While older history loads, placeholders above the oldest message the reader can scroll into.
            let loading = (ix == 0 && ctx.has_more).then(|| history_skeleton(cx));
            let row = match new_day || first_unread || start.is_some() || loading.is_some() {
                false => row,
                true => ui::col()
                    .w_full()
                    .children(loading)
                    .children(start)
                    .when(new_day, |d| d.child(day_divider(&message.timestamp, cx)))
                    .when(first_unread, |d| d.child(unread_line()))
                    .child(row)
                    .into_any_element(),
            };
            let mut arrivals = arrivals.borrow_mut();
            if !arrivals.fresh.contains(&message.id) {
                return row;
            }
            // A new message pops up from the composer with a little bounce.
            let frame = arrivals.motions.frame_one(
                message.id.clone(),
                Motion::new(Target::default()).initial(Target::default().y(8.).opacity(0.)).transition(motion::spring()),
                window,
                cx,
            );
            if !frame.is_animating() {
                arrivals.fresh.remove(&message.id);
                arrivals.motions.remove(&message.id);
            }
            frame.apply(div().w_full().child(row)).into_any_element()
        })
        .size_full()
        .pt(px(top + 12.))
        // Clear of the composer and the typing pill above it, so the last message rests in view.
        .pb(px(self.composer_height.get() + 32.))
        ;
        // The visible message area, which inline GIFs are clipped to (see gif.rs): the list between the header and
        // the floating composer, which GIFs must not paint over. Pinned to the top left: an absolute
        // element without offsets sits where it would flow, after the list.
        let composer = self.composer_height.clone();
        // The Jump to Present capsule floats 44px above the composer; GIFs stop short of it too.
        let jump = if self.shows_jump_to_present() { 44. } else { 0. };
        div()
            .id("feed")
            .track_focus(&self.feed_focus)
            .on_key_down(cx.listener(Self::on_feed_key))
            .relative()
            .size_full()
            .child(messages)
            .child(gpui_kit::component::scroll::Scrollbar::vertical(&self.list))
            .child(
                gpui_kit::canvas(|_, _, _| {}, move |bounds, _, _, _| {
                    let top = px(top).min(bounds.size.height);
                    let covered = (px(composer.get() + 16. + jump) + top).min(bounds.size.height);
                    crate::gif::set_chat_clip(gpui_kit::Bounds { origin: gpui_kit::point(bounds.origin.x, bounds.origin.y + top), size: gpui_kit::size(bounds.size.width, bounds.size.height - covered) });
                })
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .into_any_element()
    }

    // One surface at the bottom of the chat: bars it gains (reply, attachments) stack inside it above
    // the text, one hairline apart, and popovers rise from its top edge.
    // What every message row reads besides the message: names, colours, who's mentioned, hover and edit
    // state. `column` caps media width: the feed's column, or a narrower panel's.
    /// A row context for messages from any server (the Inbox): channel and role names from all of them.
    pub(crate) fn row_context_everywhere(&self, messages: &[Message], column: f32, cx: &mut Context<Self>) -> Rc<RowContext> {
        let mut ctx = self.row_context(messages, None, false, column, cx);
        if let Some(ctx) = Rc::get_mut(&mut ctx) {
            for guild in &self.guilds {
                ctx.channel_names.extend(guild.channels.iter().map(|c| (c.id.clone(), c.name.clone())));
                ctx.role_names.extend(guild.roles.iter().map(|r| (r.id.clone(), r.name.clone())));
                ctx.names.extend(self.members.get(&guild.id).into_iter().flat_map(|m| m.values()).map(|u| (u.id.clone(), u.name.clone())));
            }
        }
        ctx
    }

    pub(crate) fn row_context(&self, messages: &[Message], channel: Option<&Channel>, has_more: bool, column: f32, cx: &mut Context<Self>) -> Rc<RowContext> {
        Rc::new(RowContext {
            activity_cards: Rc::new(self.chat_cards(messages, cx)),
            me: self.me.clone(),
            uploads: self.uploads.clone(),
            previews: self.file_previews.clone(),
            embeds: self.account_settings.render_embeds,
            reactions: self.account_settings.render_reactions,
            compact: self.account_settings.message_display_compact,
            compact_avatars: self.prefs.compact_avatars,
            font_size: f32::from(self.prefs.chat_font_size),
            group_spacing: f32::from(self.prefs.group_spacing(self.account_settings.message_display_compact)),
            role_style: self.prefs.role_style,
            underline_links: self.prefs.underline_links,
            name_styles: self.prefs.display_name_styles,
            media: self.media_prefs(),
            // Later entries win: the server's member cache last, so nicknames beat the account names
            // that history loaded over REST carries (it has no member objects).
            names: messages
                .iter()
                .flat_map(|m| std::iter::once(&m.author).chain(&m.mentions))
                .chain(self.me.iter())
                .chain(self.guild_id.as_ref().and_then(|g| self.members.get(g)).into_iter().flat_map(|m| m.values()))
                .map(|u| (u.id.clone(), u.name.clone()))
                .collect(),
            // In a server Discord keeps the role colour and draws only the style's font.
            style_ground: self.guild_id.is_none().then(|| palette::surface(palette::Level::Page, cx)),
            colors: messages.iter().filter_map(|m| Some((m.author.id.clone(), self.role_color(m.guild_id.as_ref().or(self.guild_id.as_ref()), &m.author.id, m.roles.as_ref(), cx.theme().is_dark())?))).collect(),
            mentions_me: messages.iter().filter(|m| self.mentions_me(m)).map(|m| m.id.clone()).collect(),
            hidden_replies: messages.iter().filter_map(|m| m.reply.as_deref()).filter_map(|r| Some((r.id.clone(), self.hidden_author(&r.author.id)?))).collect(),
            channel_names: self.guild().into_iter().flat_map(|g| &g.channels).map(|c| (c.id.clone(), c.name.clone())).chain(self.forward_sources(messages)).collect(),
            role_names: self.guild().map(|g| g.roles.iter().map(|r| (r.id.clone(), r.name.clone())).collect()).unwrap_or_default(),
            role_inks: std::sync::Arc::new(self.guild().map(|g| g.roles.iter().filter(|r| r.color != 0).map(|r| (r.id.clone(), palette::role_ink(r.color, cx.theme().is_dark()))).collect()).unwrap_or_default()),
            mention_fill: palette::accent_soft(cx),
            mention_hover: palette::accent().opacity(0.26),
            mention_ink: palette::accent_ink(cx),
            flash: self.flash.clone(),
            editing: self.editing.as_ref().map(|m| m.id.clone()),
            hovered: self.hovered_message.clone(),
            hovered_tile: self.hovered_tile.clone(),
            alt_popout: self.alt_popout.clone(),
            image_descriptions: self.prefs.show_image_descriptions,
            focused: self.focused_message.clone(),
            copied: self.copied.clone(),
            revealed: self.revealed.clone(),
            text_files: self.text_files.clone(),
            link_previews: self.link_previews.clone(),
            joined: self.guilds.iter().map(|g| g.id.clone()).collect(),
            unread_marker: self.unread_marker,
            this: cx.weak_entity(),
            has_more,
            history_start: channel.and_then(|c| self.history_start(c, has_more)),
            column,
            quick_reactions: self.prefs.frequent_emojis(3, &["💯", "😆", "💖"]),
            shift: self.shift_held,
            manage_messages: channel.is_some_and(|c| self.can_in(&c.id, crate::discord::perm::MANAGE_MESSAGES)),
            facts: crate::message_menu::Facts {
                mine: false,
                manage: channel.is_some_and(|c| self.can_in(&c.id, crate::discord::perm::MANAGE_MESSAGES)),
                pin: channel.is_some_and(|c| self.can_in(&c.id, crate::discord::perm::PIN_MESSAGES)),
                in_dm: channel.is_some_and(|c| self.dms.iter().any(|d| d.id == c.id)),
                threads: channel.is_some_and(|c| c.kind == ChannelKind::Text && self.can_in(&c.id, crate::discord::perm::CREATE_PUBLIC_THREADS)),
                developer: self.developer_mode,
                saved: false,
            },
            saved_ids: self.saved_ids.clone(),
            poll_picks: self.poll_picks.clone(),
            poll_peek: self.poll_peek.clone(),
            components: self.component_view(messages),
            announce: self.newsroom(channel),
            notices: self.notice_facts(messages, channel),
        })
    }

    // The composer's +: a file, or a poll where you may post one (always in a DM).
    fn attach_entries(&self) -> Vec<crate::menu::Entry> {
        let polls = self.guild_id.is_none() || self.channel.as_ref().is_some_and(|c| self.can_in(&c.id, crate::discord::perm::SEND_POLLS));
        let poll = crate::menu::item("Create poll", |this, window, cx| this.open_poll_creator(window, cx)).icon(Glyph::Checks);
        vec![
            crate::menu::item("Upload a file", |this, window, cx| this.upload(window, cx)).icon(Glyph::Upload).entry(),
            (if polls { poll } else { poll.hint("You can't create polls here").disabled() }).entry(),
            crate::menu::item("Use an app command", |this, window, cx| this.start_command(window, cx)).icon(Glyph::AppWindow).hint("Or type / in your message").entry(),
        ]
    }
}

impl RowContext {
    // The name this server shows for someone: their nickname once the member is known.
    pub(crate) fn name_of(&self, user: &User) -> String {
        self.names.get(&user.id).cloned().unwrap_or_else(|| user.name.clone())
    }

    // A person, role or channel this row context knows by id.
    pub(crate) fn name_for_id(&self, id: &str) -> Option<String> {
        self.names.get(id).or_else(|| self.role_names.get(id)).or_else(|| self.channel_names.get(id)).cloned()
    }

    pub(crate) fn column(&self) -> f32 {
        self.column
    }

    pub(crate) fn this(&self) -> WeakEntity<Waffle> {
        self.this.clone()
    }

    // A name's line box at the chat's text size: 22px at 16, Discord's 1.375.
    fn name_line(&self) -> f32 {
        (self.font_size * 1.375).round()
    }

    // The body's first line box (the row's 1.45 line height), which the time beside it centres in.
    pub(crate) fn body_line(&self) -> f32 {
        (self.font_size * 1.45).round()
    }

    fn role_marks(&self, user_id: &str) -> (Option<Hsla>, Option<Hsla>) {
        role_marks(self.role_style, self.colors.get(user_id).copied())
    }
}

// Role colors: the name's ink and the dot after it, as each style shows someone's role colour.
pub(crate) fn role_marks(style: RoleStyle, color: Option<Hsla>) -> (Option<Hsla>, Option<Hsla>) {
    match style {
        RoleStyle::InNames => (color, None),
        RoleStyle::NextToNames => (None, color),
        RoleStyle::Hidden => (None, None),
    }
}

// Not verified: Discord's dot size; 10px as in design/explore-settings.html.
pub(crate) fn role_dot(color: Hsla) -> Div {
    div().size_2p5().flex_none().rounded_full().bg(color)
}

pub(crate) struct RowContext {
    pub(crate) me: Option<User>,
    // Files going out with pending messages, by nonce, and their tile pictures.
    uploads: HashMap<String, crate::UploadJob>,
    previews: HashMap<std::path::PathBuf, Option<std::sync::Arc<gpui_kit::RenderImage>>>, // lint-allow(unbounded-growth): a snapshot cloned per frame, one entry per pending upload
    // Settings › Chat: Discord's render_embeds and render_reactions.
    embeds: bool,
    reactions: bool,
    // Accessibility and Chat settings: Discord's message display, text size, group spacing, role colours,
    // link underlines, display-name styles and image descriptions.
    compact: bool,
    compact_avatars: bool,
    font_size: f32,
    group_spacing: f32,
    role_style: RoleStyle,
    underline_links: bool,
    name_styles: bool,
    media: MediaPrefs,
    names: HashMap<String, String>, // lint-allow(unbounded-growth): a per-frame snapshot of the visible messages' authors
    // The chat's surface, for drawing authors' display-name styles on; None in a server.
    style_ground: Option<Hsla>,
    colors: HashMap<String, Hsla>,
    mentions_me: std::collections::HashSet<String>,
    // Replied-to messages by someone blocked or ignored, by id: the reply bar doesn't quote them.
    hidden_replies: HashMap<String, Hidden>,
    pub(crate) channel_names: HashMap<String, String>, // lint-allow(unbounded-growth): a per-frame snapshot of this server's channels
    role_names: HashMap<String, String>, // lint-allow(unbounded-growth): a per-frame snapshot of this server's roles
    // A role mention wears its role's colour, as Discord's; roles without one keep the mention badge.
    role_inks: std::sync::Arc<HashMap<String, Hsla>>,
    mention_fill: Hsla,
    mention_hover: Hsla,
    mention_ink: Hsla,
    flash: Option<String>,
    editing: Option<String>,
    hovered: Option<String>,
    // The attachment tile under the pointer, and the picture whose ALT popout is open.
    hovered_tile: Option<String>,
    alt_popout: Option<String>,
    // Settings › Chat › Show image descriptions.
    image_descriptions: bool,
    focused: Option<String>,
    copied: Option<String>,
    revealed: Rc<std::cell::RefCell<std::collections::HashSet<String>>>,
    text_files: Rc<std::cell::RefCell<crate::text_file::TextFiles>>,
    link_previews: crate::link_preview::Previews,
    // Servers you're in, so an invite to one reads "Joined".
    joined: std::collections::HashSet<String>,
    unread_marker: Option<u64>,
    this: WeakEntity<Waffle>,
    // What each activity a message posts is doing now, for its live card.
    activity_cards: Rc<crate::activities::ChatCards>,
    has_more: bool,
    history_start: Option<crate::dm_start::channel_start::HistoryStart>,
    // The toolbar's three quick reactions: most used, padded with 💯 😆 💖.
    quick_reactions: Vec<Emoji>,
    shift: bool,
    manage_messages: bool,
    // What the channel lets you do to a message, for the Shift toolbar (`mine` set per message).
    facts: crate::message_menu::Facts,
    saved_ids: std::rc::Rc<std::collections::HashSet<String>>,
    // The widest media may run.
    column: f32,
    poll_picks: HashMap<String, Vec<u32>>,
    poll_peek: std::collections::HashSet<String>,
    pub(crate) components: crate::components::ComponentView,
    announce: crate::announcements::Newsroom,
    pub(crate) notices: crate::system_message::Facts,
}

// More than a few rows below the viewport: the feed is scrolled away from its newest message.
pub(crate) fn away_from_present(visible_end: usize, count: usize) -> bool {
    visible_end + 4 < count
}

#[cfg(test)]
mod tests {
    #[test]
    fn media_settings_split_uploads_from_link_media() {
        use crate::discord::{File, Media, MediaKind, Message};
        let media = |url: &str| Media { url: url.into(), width: 1., height: 1., kind: MediaKind::Image, ..Default::default() };
        let message = Message { media: vec![media("a.png"), media("b.png"), media("link.png")], media_files: vec![File { name: "a.png".into(), url: "a.png".into(), size: 1, ..Default::default() }, File { name: "b.png".into(), url: "b.png".into(), size: 1, ..Default::default() }], ..Default::default() };
        let mut prefs = super::MediaPrefs { attachments: true, links: true, gifs: true, emoji: true, stickers: crate::stickers::StickerMotion::Always, spoilers: crate::settings::Spoilers::OnClick };
        assert_eq!(super::inline_media(&message, &prefs), [0, 1, 2]);
        prefs.attachments = false;
        assert_eq!(super::inline_media(&message, &prefs), [2]);
        prefs = super::MediaPrefs { attachments: true, links: false, ..prefs };
        assert_eq!(super::inline_media(&message, &prefs), [0, 1]);
        assert!(super::is_gif("https://cdn.discordapp.com/attachments/1/2/Dance.GIF?ex=a") && !super::is_gif("https://x/gif.png?f=.gif"));
    }

    #[test]
    fn unread_bar_says_how_many_since_when() {
        let now = chrono::Utc::now().to_rfc3339();
        assert!(super::unread_label(1, false, &now).starts_with("1 new message since "));
        assert!(super::unread_label(21, false, &now).starts_with("21 new messages since "));
        assert!(super::unread_label(50, true, &now).starts_with("50+ new messages since "));
        assert!(!super::unread_label(2, false, &now).contains(" on "), "today needs no date");
        let lastweek = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
        assert!(super::unread_label(2, false, &lastweek).contains(" on "), "another day adds it");
    }

    #[test]
    fn name_styles_colour_per_letter_and_stay_readable() {
        use crate::discord::{NameEffect, NameStyle};
        let (dark, light): (gpui_kit::Hsla, gpui_kit::Hsla) = (crate::palette::data_color(0x1E1F22), crate::palette::data_color(0xFFFFFF));
        let inks = |name: &str, effect, colors: Vec<u32>, ground| super::name_runs(name, &NameStyle { font: 11, effect, colors }, ground).into_iter().map(|(r, h)| (r, h.color.unwrap())).collect::<Vec<_>>();
        let gummy = inks("a bé", NameEffect::Gummy, vec![0xFF0000, 0x0000FF], dark);
        assert_eq!(gummy.iter().map(|(r, _)| r.clone()).collect::<Vec<_>>(), [0..1, 1..2, 2..3, 3..5], "one run per character, by byte");
        assert_eq!(gummy[0].1, gummy[3].1, "letters alternate colours, spaces skipped");
        assert_ne!(gummy[0].1, gummy[2].1);
        let gradient = inks("Wren", NameEffect::Gradient, vec![0x2AAEA6, 0xFFC490], dark);
        assert_ne!(gradient[0].1, gradient[3].1, "the ends differ");
        for (_, ink) in inks("Wren", NameEffect::Solid, vec![0xFFFF00], light) {
            assert!(crate::palette::contrast(ink, light) >= 3., "yellow darkened on white");
        }
        assert!(inks("Wren", NameEffect::Solid, vec![], dark).is_empty(), "solid without colours keeps the theme's");
        assert_eq!(inks("Wren", NameEffect::Neon, vec![], dark).len(), 4, "other effects fall back to Discord's defaults");
    }

    #[test]
    fn inlines_custom_emoji_only() {
        let out = inline_emoji("hi <a:aWonUwu:1328465377218269257> <@1> <:x:2> <unclosed");
        assert!(out.starts_with(r#"hi <img src="https://cdn.discordapp.com/emojis/1328465377218269257.gif?size=48""#), "{out}");
        assert!(out.contains(" <@1> "), "{out}");
        assert!(out.contains("emojis/2.webp"), "{out}");
        assert!(out.ends_with(" <unclosed"), "{out}");
    }

    use super::*;

    #[test]
    fn away_from_present_needs_rows_below() {
        assert!(!away_from_present(0, 0));
        assert!(!away_from_present(10, 12));
        assert!(away_from_present(10, 20));
    }

    #[test]
    fn bare_media_links_are_hidden() {
        let gif = crate::discord::Media { url: "https://t/x.mp4".into(), width: 1., height: 1., kind: crate::discord::MediaKind::Image, ..Default::default() };
        let msg = |content: &str, media: bool| Message { content: content.into(), media: if media { vec![gif.clone()] } else { vec![] }, ..Default::default() };
        assert!(only_media_link(&msg("https://klipy.com/gifs/x", true)));
        assert!(!only_media_link(&msg("look https://klipy.com/gifs/x", true)));
        assert!(!only_media_link(&msg("https://example.com", false)));
    }

    #[test]
    fn jumbo_emoji_detection() {
        assert_eq!(custom_emoji("<:blob:1> <a:dance:2>").len(), 2);
        let dance = emoji_image("a:dance:2", true).unwrap();
        assert!(dance.contains("/emojis/2.gif") && dance.contains(r#"alt=":dance:""#) && dance.contains(r#"width="22""#));
        assert!(emoji_image(":blob:1", true).unwrap().contains("/emojis/1.webp"));
        assert!(emoji_image("a:dance:2", false).unwrap().contains("/emojis/2.webp"));
        assert_eq!(emoji_image(":bad:x1", true), None);
        let jumbo = |t: &str| jumbo_parts(t).map(|p| p.len());
        assert_eq!(jumbo("🧇 🍯<:blob:1>"), Some(3));
        assert_eq!(jumbo("👨‍👩‍👧"), Some(1));
        assert_eq!(jumbo("hi 🧇"), None);
        assert_eq!(jumbo("<@5> 🧇"), None);
        assert_eq!(jumbo(&"🧇".repeat(30)), Some(30));
        assert_eq!(jumbo(&"🧇".repeat(31)), None);
    }

    #[test]
    fn reply_quotes_are_plain() {
        assert_eq!(plain("the **new** [@Wren](waffle://user/1) `list`\nok"), "the new @Wren list ok");
    }

    #[test]
    fn timestamps_read_like_discord() {
        assert_eq!(relative(chrono::TimeDelta::seconds(-7200)), "in 2 hours");
        assert_eq!(relative(chrono::TimeDelta::seconds(60)), "1 minute ago");
        assert!(discord_timestamp("0:d").is_some_and(|t| t.starts_with('`')));
        assert!(discord_timestamp("nonsense").is_none());
    }

    #[test]
    fn embed_fields_share_rows_three_across_or_two_beside_a_thumbnail() {
        let f = |inline: bool| (String::new(), String::new(), inline);
        let fields = [f(true), f(true), f(true), f(true), f(false), f(true)];
        assert_eq!(field_rows(&fields, false), vec![vec![0, 1, 2], vec![3], vec![4], vec![5]]);
        assert_eq!(field_rows(&fields, true), vec![vec![0, 1], vec![2, 3], vec![4], vec![5]]);
        assert!(field_rows(&[], false).is_empty());
    }

    #[test]
    fn long_embed_descriptions_fold() {
        assert!(!long_description("short"));
        assert!(!long_description(&"line\n".repeat(12)));
        assert!(long_description(&"line\n".repeat(13)));
        assert!(long_description(&"x".repeat(70 * 12 + 1)));
    }

    #[test]
    fn spoilers_cover_until_revealed() {
        assert_eq!(hide_spoilers("a ||bc|| d", false), "a `██` d");
        assert_eq!(hide_spoilers("a ||bc|| d", true), "a bc d");
        assert_eq!(hide_spoilers("a || b", false), "a || b");
    }

    #[test]
    fn subtext_lines_split_out_in_order() {
        assert_eq!(split_subtext("hi\n-# small\n-# **two**\nafter"), [(false, "hi".into()), (true, "small\n**two**".into()), (false, "after".into())]);
        assert_eq!(split_subtext("-# only"), [(true, "only".to_owned())]);
        assert_eq!(split_subtext("```\n-# code\n```"), [(false, "```\n-# code\n```".to_owned())]);
        assert_eq!(split_subtext("-#tight\n -# indented"), [(false, "-#tight\n -# indented".to_owned())]);
    }

    #[test]
    fn line_breaks_and_indents_survive_markdown() {
        let nb = '\u{a0}';
        assert_eq!(discord_breaks("CPU\n   Cooler\n   Board"), format!("CPU  \n{nb}{nb}{nb}Cooler  \n{nb}{nb}{nb}Board"));
        assert_eq!(discord_breaks("a\n\nb"), "a\n\nb", "a blank line stays a paragraph break");
        assert_eq!(discord_breaks("x\n```\n  code\nmore\n```\ny"), "x\n```\n  code\nmore\n```\ny", "fenced code is left alone");
        assert_eq!(discord_breaks("one line"), "one line");
    }

    #[test]
    fn sizes_read_like_discord() {
        assert_eq!(format_size(512), "512 bytes");
        assert_eq!(format_size(2048), "2 KB");
        assert_eq!(format_size(3 * 1024 * 1024), "3.0 MB");
    }

    #[test]
    fn galleries_fill_the_column_with_four_round_corners() {
        for count in 2..=12 {
            let (height, tiles) = gallery_layout(count);
            assert_eq!(tiles.len(), count.min(10), "{count} items");
            for t in &tiles {
                assert!(t.x >= 0. && t.y >= 0. && t.x + t.w <= COLUMN + 0.01 && t.y + t.h <= height + 0.01, "{count}: {t:?} spills");
            }
            // Tiles and their gaps cover the column: nothing overlaps, nothing is left empty but the gaps.
            let area: f32 = tiles.iter().map(|t| t.w * t.h).sum();
            assert!(area < COLUMN * height && area > COLUMN * height * 0.9, "{count}: area {area}");
            let round = tiles.iter().map(|t| gallery_corners(*t, COLUMN, height, RADIUS_LG)).flat_map(|c| [c.top_left, c.top_right, c.bottom_left, c.bottom_right]).filter(|r| *r == px(RADIUS_LG)).count();
            assert_eq!(round, 4, "{count} items");
        }
    }

    #[test]
    fn typing_reads_like_the_design() {
        let text = |names: &[&str]| typing_parts(names).into_iter().map(|(t, _)| t).collect::<String>();
        assert_eq!(text(&["Mira"]), "Mira is typing…");
        assert_eq!(text(&["Mira", "Theo"]), "Mira and Theo are typing…");
        assert_eq!(text(&["Mira", "Theo", "Ana"]), "Mira, Theo and Ana are typing…");
        assert_eq!(text(&["a", "b", "c", "d"]), "Several people are typing…");
        assert!(typing_parts(&["Mira"])[0].1);
    }

    #[test]
    fn stamps_read_like_discords_header() {
        let now = Local::now();
        assert_eq!(format_stamp(&now.to_rfc3339()), clock(&now.to_rfc3339()));
        assert!(format_stamp(&(now - chrono::TimeDelta::days(1)).to_rfc3339()).starts_with("Yesterday at "));
        let old = now - chrono::TimeDelta::days(30);
        assert_eq!(format_stamp(&old.to_rfc3339()), format!("{} {}", old.format("%m/%d/%Y"), clock(&old.to_rfc3339())));
        assert!(!gutter_clock(&now.to_rfc3339()).contains('M'));
    }

    #[test]
    fn result_pictures_keep_their_shape() {
        use super::fit_frame;
        let max = (280., 220.);
        // A 3:2 photo fits the width; a square fits the height; neither is cropped.
        assert_eq!(fit_frame((1200., 800.), max), (280., 280. / 1.5));
        assert_eq!(fit_frame((800., 800.), max), (220., 220.));
        // Small pictures aren't blown up.
        assert_eq!(fit_frame((100., 50.), max), (100., 50.));
        // A long screenshot is held at 1:2.5 and cropped; an unknown size is a square.
        assert_eq!(fit_frame((400., 4000.), max), (88., 220.));
        assert_eq!(fit_frame((0., 0.), max), (220., 220.));
    }
    #[test]
    fn discord_underline_and_quote_rest() {
        assert_eq!(discord_underline("a __b__ `__c__` ____ __d"), "a <u>b</u> `__c__` ____ __d");
        assert_eq!(discord_quote_rest("hi\n>>> one\ntwo"), "hi\n> one\n> two");
        assert_eq!(discord_quote_rest("```\n>>> x\n```"), "```\n>>> x\n```");
    }

    #[test]
    fn links_are_underlined_outside_code() {
        assert_eq!(underline_links("see https://a.io/x. ok"), "see <u><https://a.io/x></u>. ok");
        assert_eq!(underline_links("[docs](https://a.io) and <https://b.io>"), "<u>[docs](https://a.io)</u> and <u><https://b.io></u>");
        assert_eq!(underline_links("[docs](<https://a.io>)"), "<u>[docs](<https://a.io>)</u>");
        assert_eq!(underline_links("`https://a.io` ```\nhttps://b.io\n```"), "`https://a.io` ```\nhttps://b.io\n```");
        assert_eq!(underline_links("[@wren](waffle://user/1) <img src=\"https://cdn/e.png\" />"), "[@wren](waffle://user/1) <img src=\"https://cdn/e.png\" />");
        assert_eq!(underline_links("an `unclosed https://a.io"), "an `unclosed <u><https://a.io></u>");
    }

    #[test]
    fn code_runs_close_only_on_their_own_length() {
        let upper = |text: &str| outside_code(text, str::to_uppercase);
        assert_eq!(upper("a ``b ` c`` d"), "A ``b ` c`` D");
        assert_eq!(upper("a ```\nb\n``` c `d` e"), "A ```\nb\n``` C `d` E");
    }

    #[test]
    fn remove_embeds_needs_author_or_manage_messages_and_other_content() {
        use crate::discord::{Message, Poll, User, message_flag};
        let mine = Message { author: User { id: "me".into(), ..Default::default() }, content: "look".into(), ..Default::default() };
        let theirs = Message { author: User { id: "them".into(), ..Default::default() }, ..mine.clone() };
        assert!(can_remove_embeds(&mine, Some("me"), false));
        assert!(!can_remove_embeds(&theirs, Some("me"), false));
        assert!(can_remove_embeds(&theirs, Some("me"), true), "Manage Messages");
        let embed_only = Message { content: String::new(), ..mine.clone() };
        assert!(!can_remove_embeds(&embed_only, Some("me"), true), "a bot's embed-only post");
        assert!(can_remove_embeds(&Message { poll: Some(Poll::default()), ..embed_only }, Some("me"), false), "a poll counts");
        assert!(!can_remove_embeds(&Message { flags: message_flag::EPHEMERAL, ..mine.clone() }, Some("me"), true));
        assert!(!can_remove_embeds(&Message { author: User { id: "1".into(), ..Default::default() }, ..mine.clone() }, Some("1"), true), "Discord's own");
        assert!(!can_remove_embeds(&Message { kind: 21, ..mine }, Some("me"), true), "thread starter copy");
    }
}
