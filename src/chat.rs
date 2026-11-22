// The middle column: channel header, transcript and composer, ported from design/messages.html and
// design/navigation.html (channel header).
//
// GAP: hover fades (row fill, toolbar, 150ms) are instant: gpui's `.hover` styles don't transition.
use std::{collections::HashMap, f32::consts::TAU, rc::Rc, time::Duration};

use chrono::{DateTime, Local, NaiveDate};
use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, Bounds, Context, Corners, Div, Focusable, FontWeight, HighlightStyle, Hsla, KeyDownEvent, Pixels, SharedString, Stateful, StyledText, WeakEntity, Window,
    WindowControlArea,
    component::{
        ActiveTheme, Icon, Sizable, Size,
        input::{Input, Textarea},
        text::{InlineElement, InlineRenderContext, MarkdownExtensions, MarkdownNode, MarkdownParseContext, MarkdownPlugin, TextView, markdown_ast},
        tooltip::Tooltip,
    },
    div, img, linear_color_stop, linear_gradient, list, point, prelude::*, px,
};
use gpui_motion_kit::core::{Motion, Target};

use crate::{
    EmojiTarget, Feed, RightPanel, Waffle,
    discord::{Channel, ChannelKind, Embed, Emoji, MediaKind, Message, NameEffect, NameStyle, Poll, PollAnswer, SendState, User},
    icons::Glyph,
    layout::HEADER_HEIGHT,
    mascot::Mood,
    motion,
    overlays::ComposerPicker,
    palette::{self, Level},
    system_message::{Sentence, notice_line},
    ui::{self, RADIUS_LG, RADIUS_MD, RADIUS_SM, RADIUS_XS, Squircle},
};

// How long a jumped-to message stays marked: its fill and squiggle (the flash in `jump_to`).
pub(crate) const JUMP_FLASH: Duration = Duration::from_millis(2400);

// Messages from one author closer together than this share a header, like Discord.
const GROUP_GAP_SECS: i64 = 7 * 60;
// A lone image fits inside this; galleries and cards fill the media column.
const MAX_IMAGE: (f32, f32) = (400., 320.);
const COLUMN: f32 = 432.;
const GALLERY_GAP: f32 = 4.;
pub(crate) const AVATAR: f32 = 40.;
// The avatar column plus the gap to the content column: where replies and continuations line up.
const CONTENT_INSET: f32 = AVATAR + 16.;
// The unread bar's room at the feed's top: 8 above it, its 36, 8 below.
const UNREAD_BAR_SPACE: f32 = 52.;
// A reply preview's line, above the message it answers.
const REPLY_HEIGHT: f32 = 22.;
// A forwarded copy's card, as the design's `.fwd`.
const FORWARD_WIDTH: f32 = 520.;

// The floating stream player's width; its height keeps 16:9.

impl Waffle {
    pub fn render_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let main = div().relative().flex_1().min_w_0().h_full().flex().flex_col().bg(palette::surface(Level::Page, cx));
        let Some(channel) = self.channel.clone() else {
            return main.child(empty_state("idle", Mood::Idle, "No channel open", "Pick a server on the left, then a channel to start reading.", cx));
        };
        let header = self.channel_header(&channel, window, cx);
        let unread_bar = self.unread_bar(&channel.id, cx);
        // The feed starts below the unread bar, so it never covers the first rows: opened on its first unread,
        // the NEW line sits just under the bar.
        let top = if unread_bar.is_some() { UNREAD_BAR_SPACE } else { 0. };
        let body = match self.feeds.get(&channel.id) {
            Some(Feed { loaded: true, messages, .. }) if messages.is_empty() && channel.kind == ChannelKind::Dm => match self.dm_start(&channel, false) {
                // Nothing said yet: the header rests at the foot of the feed, where the first message will land.
                Some(start) => div().size_full().flex().flex_col().justify_end().pt(px(top)).pb(px(self.composer_height.get() + 32.)).child(crate::dm_start::dm_start_header(&start, cx.weak_entity(), cx)).into_any_element(),
                None => empty_state("empty", Mood::Happy, "No messages yet", "Be the first to say something.", cx),
            },
            Some(Feed { loaded: true, messages, .. }) if messages.is_empty() => empty_state("empty", Mood::Happy, "No messages yet", "Be the first to say something.", cx),
            Some(Feed { loaded: true, .. }) => self.render_messages(top, cx).into_any_element(),
            Some(Feed { error: Some(error), .. }) => empty_state("failed", crate::illustration::Illustration::LoadFailed, "Couldn't load messages", error.clone(), cx),
            _ => skeleton(cx),
        };
        let chat = div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .key_context("Chat")
            .on_action(cx.listener(Self::focus_search))
            // The feed stops under the header, a hard edge.
            .child(header)
            // The composer floats over the feed, which scrolls on under its glass.
            .child(div().relative().flex_1().min_h_0().child(body).children(unread_bar).child(match self.composer_gate(&channel, window, cx) {
                Some(gate) => gate,
                None => self.render_composer(window, cx).into_any_element(),
            }));
        // The members column beside the feed runs to the window's top with a header of its own on the same line.
        let panel = self.panel_column_width().map(|w| self.part(crate::part::Column::Panel, gpui_kit::StyleRefinement::default().w(px(w)).h_full().flex_none(), cx));
        let drop_zone = self.drop_zone(&channel, cx);
        main.child(div().flex_1().min_h_0().flex().child(chat.child(drop_zone)).children(panel))
    }

    // Where you are as breadcrumbs, the topic, then the channel's tools (design: navigation.html's chanbar).
    fn channel_header(&self, channel: &Channel, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let (strong, faint, selected) = (palette::strong(cx), palette::faint(cx), palette::selected(cx));
        let parent = self.thread_parent(channel).map(|parent| {
            crumb("thread-parent", cx)
                .child(Icon::new(Glyph::Hashtag).size(px(14.)))
                .child(ui::truncating("header-thread-parent", parent.name.clone()))
                .on_click(cx.listener(move |this, _, window, cx| this.open_channel(parent.clone(), window, cx)))
        });
        let lead = match channel.kind {
            ChannelKind::Dm => ui::dm_face(&channel.face(), 24., palette::surface(Level::Page, cx)),
            ChannelKind::Thread => Icon::new(Glyph::Thread).size(px(20.)).text_color(faint).into_any_element(),
            _ => Icon::new(Glyph::Hashtag).size(px(20.)).text_color(faint).into_any_element(),
        };
        let here = div()
            .h(px(32.))
            .px(px(6.))
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(16.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(strong)
            .child(lead)
            .child(ui::truncating("header-channel-name", channel.name.clone()));
        // Thread names are often whole sentences: the crumbs cap at 60% and truncate rather than push the tools off.
        let crumbs = div()
            .flex_none()
            .max_w(gpui_kit::relative(0.6))
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(2.))
            .when_some(parent, |c, parent| c.child(parent).child(Icon::new(Glyph::ChevronRight).size(px(14.)).text_color(faint)))
            .child(here);
        let topic = channel.topic.clone().map(|topic| div().flex_1().min_w_0().pl(px(12.)).border_l_1().border_color(palette::line_strong(cx)).text_size(px(14.)).text_color(palette::muted(cx)).child(ui::truncating("header-topic", topic)));
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
        let members_on = matches!(self.right_panel, RightPanel::Members);
        let members = (self.guild_id.is_some() || channel.raw_type == 3).then(|| {
            ui::icon_button("members-toggle", Glyph::Users, if members_on { "Hide Member List" } else { "Show Member List" }, cx)
                .occlude()
                .when(members_on, |b| b.bg(selected).text_color(strong))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.right_panel = if matches!(this.right_panel, RightPanel::Members) { RightPanel::Hidden } else { RightPanel::Members };
                    cx.notify();
                }))
        });
        let search = Some(self.channel_search(window, cx));
        // Occludes, like the composer: the feed scrolls under the header and must not take its hover or clicks.
        let bar = div()
            .occlude()
            .h(px(HEADER_HEIGHT))
            .pl(px(8.))
            .pr(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            // Drag area: every control inside it occludes it, or Windows' caption takes its clicks.
            .window_control_area(WindowControlArea::Drag)
            .child(crumbs)
            .children(topic)
            .child(div().ml_auto().flex_none().flex().items_center().gap(px(2.)).children(join).children(calls.into_iter().flatten()).children(threads).children(bell).child(pins).children(add_members).children(members).children(search))
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
            .ml(px(4.))
            .flex_none()
            .occlude()
            .squircle(RADIUS_MD)
            .child(ui::search_frame("channel-search", Input::new(&self.search_input).token(move |token, _, cx| crate::search_menu::render_token(token, &avatars, cx)), &handle).quiet(!open))
            .capture_key_down(cx.listener(Self::on_search_key))
            .when(!open, |d| {
                d.cursor_pointer().hover(move |s| s.bg(hover)).on_click(cx.listener(move |_, _, window, cx| {
                    window.focus(&handle, cx);
                    cx.notify();
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
            let card = div().absolute().top(px(46.)).right_0().on_children_prepainted(move |children, _, cx| bounds.update(cx, |b, _| *b = children.first().copied())).child(card);
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
        let button = move |id: &'static str| div().id(id).h(px(28.)).px(px(8.)).flex().items_center().gap(px(6.)).squircle(RADIUS_XS).cursor_pointer().hover(move |s| s.bg(press));
        let (first_id, channel_id) = (first.id.clone(), channel_id.to_owned());
        Some(
            div()
                .id("unread-bar")
                // Floats over the feed: the messages under it must not take its hover or clicks.
                .occlude()
                .absolute()
                .top(px(8.))
                .left(px(8.))
                .right(px(8.))
                .min_h(px(36.))
                .pl(px(12.))
                .pr(px(4.))
                .flex()
                .items_center()
                .gap(px(8.))
                .squircle(RADIUS_SM)
                .bg(tint)
                .text_size(px(14.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::accent_ink(cx))
                .child(
                    button("unread-jump")
                        .flex_1()
                        .min_w_0()
                        .ml(px(-8.))
                        .child(Icon::new(Glyph::ArrowUp).size(px(16.)))
                        .child(div().min_w_0().truncate().child(label))
                        // Discord's body only jumps to the first unread; reading it there is what marks it read.
                        .on_click(cx.listener(move |this, _, _, cx| this.jump_to(first_id.clone(), cx))),
                )
                // Read now; the NEW line stays until the channel is left or you post, as Discord's does.
                .child(button("unread-read").child("Mark as read").child(Icon::new(Glyph::Checkmark).size(px(16.))).on_click(cx.listener(move |this, _, _, cx| {
                    this.mark_channel_read(&channel_id, cx);
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
        let arrivals = self.arrivals.clone();
        let messages = list(self.list.clone(), move |ix, window, cx| {
            if ix == 0 && ctx.has_more {
                let this = ctx.this.clone();
                cx.defer(move |cx| {
                    this.update(cx, |this, cx| this.load_older(cx)).ok();
                });
            }
            let message = &messages[ix];
            let previous = ix.checked_sub(1).map(|i| &messages[i]);
            let new_day = day(&message.timestamp) != previous.and_then(|p| day(&p.timestamp));
            let starts_group = new_day || message.reply.is_some() || previous.is_none_or(|p| crate::system_message::is_notice(p) || !continues(p, message));
            let row = match crate::system_message::is_notice(message) {
                true => crate::system_message::row(&messages, ix, &ctx, cx),
                false => message_row(message, starts_group, &ctx, cx),
            };
            // One accent line above the first message after your read position.
            let first_unread = ctx.unread_marker.is_some_and(|read| crate::order(&message.id) > read && previous.is_none_or(|p| crate::order(&p.id) <= read));
            let start = ctx.dm_start.as_ref().filter(|_| ix == 0).map(|s| crate::dm_start::dm_start_header(s, ctx.this.clone(), cx));
            let row = match new_day || first_unread || start.is_some() {
                false => row,
                true => div()
                    .w_full()
                    .flex()
                    .flex_col()
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
            me: self.me.clone(),
            uploads: self.uploads.clone(),
            previews: self.file_previews.clone(),
            embeds: self.account_settings.render_embeds,
            reactions: self.account_settings.render_reactions,
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
            channel_names: self.guild().into_iter().flat_map(|g| &g.channels).map(|c| (c.id.clone(), c.name.clone())).chain(self.forward_sources(messages)).collect(),
            role_names: self.guild().map(|g| g.roles.iter().map(|r| (r.id.clone(), r.name.clone())).collect()).unwrap_or_default(),
            mention_fill: palette::accent_soft(cx),
            mention_hover: palette::accent().opacity(0.26),
            mention_ink: palette::accent_ink(cx),
            flash: self.flash.clone(),
            editing: self.editing.as_ref().map(|m| m.id.clone()),
            hovered: self.hovered_message.clone(),
            focused: self.focused_message.clone(),
            copied: self.copied.clone(),
            revealed: self.revealed.clone(),
            text_files: self.text_files.clone(),
            link_previews: self.link_previews.clone(),
            joined: self.guilds.iter().map(|g| g.id.clone()).collect(),
            unread_marker: self.unread_marker,
            this: cx.weak_entity(),
            has_more,
            dm_start: channel.and_then(|c| self.dm_start(c, has_more)),
            column,
            quick_reactions: self.prefs.frequent_emojis(3, &["💯", "👍", "👎"]),
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
        ]
    }

    pub(crate) fn render_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.composer.read(cx).focus_handle(cx).is_focused(window);
        let (strong, muted) = (palette::strong(cx), palette::muted(cx));
        let open_picker = self.composer_picker();
        let row = div()
            .min_h(px(48.))
            .py(px(7.))
            .px(px(6.))
            .flex()
            .items_end()
            .gap(px(4.))
            .child(ui::icon_button("attach", Glyph::PlusCircle, "Upload a file or create a poll", cx).on_click(cx.listener(|this, _, window, cx| this.open_menu(window.mouse_position(), this.attach_entries(), cx))))
            // XSmall gives the editor 0/4px padding of its own; with these, the design's 4/6.
            .child(div().flex_1().min_w_0().child(Textarea::new(&self.composer).appearance(false).with_size(Size::XSmall).py(px(4.)).px(px(2.)).text_size(px(15.)).line_height(px(22.)).text_color(strong).token(mention_chip(cx))))
            .children([(ComposerPicker::Gifs, "composer-gif", Glyph::Gif, "Open GIF picker"), (ComposerPicker::Stickers, "composer-sticker", Glyph::Sticker, "Open sticker picker"), (ComposerPicker::Emoji, "composer-emoji", Glyph::Emoji, "Select Emoji")].map(|(picker, id, glyph, label)| {
                ui::icon_button(id, glyph, label, cx).toggled(open_picker == Some(picker), cx).on_click(cx.listener(move |this, _, window, cx| this.show_composer_picker(picker, window, cx)))
            }));
        let card = div()
            .relative()
            .mx(px(16.))
            .flex()
            .flex_col()
            .squircle(RADIUS_LG)
            // Glass: the feed shows through, blurred, as it scrolls under.
            .bg(palette::surface(Level::Raised, cx).opacity(0.72))
            .backdrop_blur(px(24.))
            .backdrop_saturate(1.6)
            // The ring-less elevation: the hairline below is the edge, and the pop shadow's own 1px ring
            // on top of it drew a line three times as heavy as the design's.
            .shadow(ui::shadow_pop_on(gpui_kit::transparent_black(), cx))
            // A hairline: one device pixel on Retina; a full point read heavy around the glass.
            .border(px(0.5))
            .border_color(if focused { palette::line_strong(cx) } else { palette::line(cx) })
            .children(self.autocomplete_popup(cx))
            .children(self.request_bar(cx))
            .children(self.composer_bar(cx))
            .children(self.attachment_tray(cx))
            .child(row);
        let guide_bar = self.channel.clone().and_then(|channel| self.guide_action_bar(&channel, cx));
        let typers = self.typers();
        let names: Vec<&str> = typers.iter().map(|t| t.name.as_str()).collect();
        // A glass pill just above the composer, out of the flow, so the box never moves when someone types.
        // Out of the way while suggestions are open, which rise over the same spot.
        let typing = (!names.is_empty() && self.autocomplete.is_none()).then(|| {
            div().absolute().left(px(28.)).top(px(-28.)).max_w(gpui_kit::relative(0.6)).h(px(24.)).px(px(10.)).flex().items_center().gap(px(8.)).rounded_full()
                .bg(palette::surface(Level::Raised, cx).opacity(0.72))
                .backdrop_blur(px(16.))
                .text_size(px(12.))
                .text_color(muted)
                .child(ui::bars("typing", palette::accent_ink(cx), 12.))
                .child(div().min_w_0().truncate().child(emphasised(&typing_parts(&names), strong)))
        });
        let (height, this) = (self.composer_height.clone(), cx.weak_entity());
        let measure = gpui_kit::canvas(
            move |bounds, _, cx| {
                let h = f32::from(bounds.size.height);
                if (h - height.get()).abs() > 0.5 {
                    height.set(h);
                    this.update(cx, |_, cx| cx.notify()).ok();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0();
        let note = self.send_error.clone().map(|error| div().mx(px(16.)).mt(px(6.)).text_size(px(12.)).text_color(palette::danger_ink(cx)).child(error));
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .flex()
            .flex_col()
            .capture_key_down(cx.listener(Self::on_composer_key))
            // Before the text field's own Tab, which indents and would never reach on_composer_key.
            .capture_action(cx.listener(|this, _: &gpui_kit::component::input::IndentInline, window, cx| {
                if this.autocomplete.is_some() {
                    this.accept_suggestion(window, cx);
                    cx.stop_propagation();
                }
            }))
            // Before the text field's own Paste, which drops anything that isn't text.
            .capture_action(cx.listener(|this, _: &gpui_kit::component::input::Paste, window, cx| {
                if this.paste_files(window, cx) {
                    cx.stop_propagation();
                }
            }))
            .child(measure)
            .children(self.jump_to_present(cx))
            // Occludes: the composer floats over the feed, which must not take the hover or the click under it.
            // A Server Guide to-do bar rides on top, inside the measured stack so the feed leaves room for it.
            .child(div().relative().occlude().flex().flex_col().gap(px(8.)).children(guide_bar).child(card).children(typing))
            .children(note)
            // 8px off the window's foot, level with the user card in the left column.
            .child(div().h(px(8.)))
    }

    // A mention badge's click: a person's profile beside it, or the channel it names.
    fn open_mention(&mut self, url: &str, at: gpui_kit::Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = url.strip_prefix("waffle://user/") {
            let Some(user) = self.known_users().into_iter().find(|u| u.id == id) else { return };
            return self.open_profile(user, at, cx);
        }
        let Some(id) = url.strip_prefix("waffle://channel/") else { return };
        let channel = self.guild().and_then(|g| g.channels.iter().find(|c| c.id == id).cloned());
        if let Some(channel) = channel {
            self.open_channel(channel, window, cx);
        }
    }

    fn shows_jump_to_present(&self) -> bool {
        self.scrolled_up && self.list.item_count() > 0
    }

    // A small capsule 12px above the composer, out of the flow, so it never opens a gap.
    fn jump_to_present(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        if !self.shows_jump_to_present() {
            return None;
        }
        let overlay = palette::surface(Level::Overlay, cx);
        let hover = ui::mix(overlay, palette::strong(cx), 0.06);
        // Occludes what's under it, or a click would land on the message behind as well.
        let capsule = ui::pressable(div().id("jump-to-present"), 0.97)
            .occlude()
            .h(px(32.))
            .px(px(14.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded_full()
            .bg(overlay)
            .shadow(ui::shadow_pop(cx))
            .cursor_pointer()
            .whitespace_nowrap()
            .text_size(px(14.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(palette::strong(cx))
            .hover(move |s| s.bg(hover))
            .child(Icon::new(Glyph::ArrowDown).size(px(16.)))
            .child("Jump to present")
            .on_click(cx.listener(|this, _, _, cx| {
                this.list.scroll_to_end();
                this.scrolled_up = false;
                cx.notify();
            }));
        Some(div().absolute().left_0().right_0().top(px(-44.)).flex().justify_center().child(capsule).with_animation(
            "jump-to-present-in",
            Animation::new(ui::DUR_POP).with_easing(gpui_kit::ease_out_quint()),
            |d, t| d.opacity(t).mt(px((1. - t) * 8.)),
        ))
    }

    // The reply or edit bar at the top of the composer.
    fn composer_bar(&self, cx: &mut Context<Self>) -> Option<Div> {
        let strong = palette::strong(cx);
        let label = match (&self.replying, &self.editing) {
            (Some(reply), _) => emphasised(&[("Replying to ".into(), false), (self.member_name(&reply.author), true)], strong),
            (_, Some(_)) => emphasised(&[("Editing message".into(), true)], strong),
            _ => return None,
        };
        let bar = div()
            .min_h(px(40.))
            .py(px(4.))
            .pl(px(14.))
            .pr(px(6.))
            .flex()
            .items_center()
            .gap(px(6.))
            .min_w_0()
            .border_b_1()
            .border_color(palette::line(cx))
            .text_size(px(14.))
            .text_color(palette::muted(cx))
            .child(div().flex_1().min_w_0().truncate().child(label));
        let bar = match self.replying.is_some() {
            true => bar.child(self.ping_toggle(cx)),
            false => bar.child(key_hint("Esc", "Cancel", cx)).child(key_hint("Enter", "Save", cx)),
        };
        Some(bar.child(ui::icon_button("cancel-compose", Glyph::X, "Cancel", cx).small().on_click(cx.listener(|this, _, window, cx| this.cancel_compose(window, cx)))))
    }

    // Whether the reply notifies its author; it remembers your last choice.
    fn ping_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let on = self.reply_ping;
        let hover = palette::hover(cx);
        div()
            .id("reply-ping")
            .h(px(28.))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(4.))
            .squircle(RADIUS_XS)
            .cursor_pointer()
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(if on { palette::accent_ink(cx) } else { palette::faint(cx) })
            .hover(move |s| s.bg(hover))
            .child(Icon::new(Glyph::AtBold).size(px(16.)))
            .child(if on { "On" } else { "Off" })
            .tooltip(|window, cx| Tooltip::new("Mention the author of this message").build(window, cx))
            .on_click(cx.listener(|this, _, _, cx| {
                this.reply_ping = !this.reply_ping;
                cx.notify();
            }))
    }

    // Files waiting to send: borderless wells inside the composer, never cards in a card. Each shows
    // the OS's own picture of it (a thumbnail, else its type's icon), its name and size, a spoiler toggle
    // and a remove button.
    fn attachment_tray(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        if self.pending_files.is_empty() {
            return None;
        }
        let (selected, strong, faint) = (palette::selected(cx), palette::strong(cx), palette::faint(cx));
        let chips = self.pending_files.iter().enumerate().map(|(i, file)| {
            let preview = file_picture(self.file_previews.get(&file.path).cloned().flatten(), file.kind(), 48., file.spoiler, cx);
            let (spoiler_glyph, spoiler_tip) = match file.spoiler {
                true => (Glyph::EyeSlash, "Remove Spoiler"),
                false => (Glyph::Eye, "Mark as Spoiler"),
            };
            div()
                .w(px(240.))
                .flex_none()
                .py(px(6.))
                .pl(px(6.))
                .pr(px(4.))
                .flex()
                .items_center()
                .gap(px(10.))
                .squircle(RADIUS_MD)
                .bg(selected)
                .child(preview)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(div().truncate().text_size(px(14.)).font_weight(FontWeight::MEDIUM).text_color(strong).child(file.name.clone()))
                        .child(div().truncate().text_size(px(12.)).text_color(faint).child(format_size(file.size))),
                )
                .child(ui::icon_button(SharedString::from(format!("spoiler-file-{i}")), spoiler_glyph, spoiler_tip, cx).small().on_click(cx.listener(move |this, _, _, cx| this.toggle_spoiler(i, cx))))
                .child(ui::icon_button(SharedString::from(format!("remove-file-{i}")), Glyph::X, "Remove Attachment", cx).small().on_click(cx.listener(move |this, _, _, cx| this.remove_file(i, cx))))
        });
        Some(div().id("composer-tray").flex().gap(px(8.)).p(px(10.)).overflow_x_scroll().border_b_1().border_color(palette::line(cx)).children(chips))
    }

    // While files are dragged over the chat: the whole pane says where they'll go, and takes the drop
    // anywhere, the composer included. Invisible otherwise; its hitbox blocks nothing beneath.
    fn drop_zone(&self, channel: &Channel, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let limit = format_size(crate::discord::upload_limit(self.my_account.premium_type, self.guild().map_or(0, |g| g.premium_tier)));
        let accent = palette::accent_ink(cx);
        let card = div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .squircle(RADIUS_LG)
            .bg(palette::surface(Level::Raised, cx).opacity(0.92))
            .border_2()
            .border_color(accent)
            .child(div().text_color(accent).child(Icon::new(Glyph::Upload).size(px(40.))))
            .child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(format!("Drop to upload to {}", crate::channel_mention(channel))))
            .child(div().text_size(px(14.)).text_color(palette::muted(cx)).child(format!("Up to {} files, {limit} each", crate::discord::MAX_FILES)));
        div()
            .id("drop-zone")
            .absolute()
            .inset_0()
            .p(px(12.))
            .opacity(if cfg!(debug_assertions) && std::env::var("WAFFLE_DEMO").as_deref() == Ok("attachments-drop") { 1. } else { 0. })
            .drag_over::<gpui_kit::ExternalPaths>(|style, _, _, _| style.opacity(1.))
            .on_drop(cx.listener(|this, paths: &gpui_kit::ExternalPaths, window, cx| this.add_files(paths.paths().to_vec(), window, cx)))
            .child(card)
    }

    // Arrows drive the suggestion list (Tab and Enter accept elsewhere); Esc cancels a reply or edit; Up in an empty box edits your last message.
    fn on_composer_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        if self.autocomplete.is_some() {
            match key {
                "up" => self.move_suggestion(-1, cx),
                "down" => self.move_suggestion(1, cx),
                "escape" => self.autocomplete = None,
                _ => return,
            }
            return cx.stop_propagation();
        }
        match key {
            "escape" if self.replying.is_some() || self.editing.is_some() => self.cancel_compose(window, cx),
            "up" if !self.has_draft && self.edit_last(window, cx) => {}
            _ => return,
        }
        cx.stop_propagation();
    }

    // Suggestions above the composer: its width, 8px above it, rising from it.
    fn autocomplete_popup(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let auto = self.autocomplete.as_ref()?;
        let (hover, selected, strong, muted, text) = (palette::hover(cx), palette::selected(cx), palette::strong(cx), palette::muted(cx), palette::text(cx));
        let heading: SharedString = match auto.token.chars().next() {
            Some('#') => "Text Channels".into(),
            Some(':') => format!("Emoji matching {}", auto.token).into(),
            _ => "Members".into(),
        };
        let rows = auto.items.iter().enumerate().map(|(i, item)| {
            let lead = match &item.lead {
                crate::SuggestLead::Avatar(url) => ui::avatar(url.clone(), 28.).into_any_element(),
                crate::SuggestLead::Channel => div().size(px(28.)).flex_none().rounded_full().bg(selected).text_color(muted).flex().items_center().justify_center().child(Icon::new(Glyph::Hashtag).size(px(16.))).into_any_element(),
                crate::SuggestLead::Emoji(emoji) => div().size(px(28.)).flex_none().flex().items_center().justify_center().child(crate::menu::emoji_face(emoji, 22.)).into_any_element(),
            };
            let detail = item.detail.clone().map(|d| div().flex_none().max_w(px(180.)).truncate().text_size(px(12.)).text_color(muted).child(d));
            div()
                .id(SharedString::from(format!("suggest-{i}")))
                .min_h(px(40.))
                .py(px(6.))
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(10.))
                .squircle(RADIUS_SM)
                .cursor_pointer()
                .text_color(text)
                .when(i == auto.selected, |r| r.bg(selected))
                .when(i != auto.selected, |r| r.hover(move |s| s.bg(hover)))
                .child(lead)
                .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::MEDIUM).text_color(strong).child(item.label.clone()))
                .children(detail)
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(auto) = &mut this.autocomplete {
                        auto.selected = i;
                    }
                    this.accept_suggestion(window, cx);
                }))
        });
        // msg-pop: it grows from 95% at its bottom centre as it fades in.
        Some(
            div()
                .absolute()
                .left(px(-1.))
                .right(px(-1.))
                .bottom(gpui_kit::relative(1.))
                .mb(px(9.))
                .p(px(6.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .max_h(px(340.))
                .squircle(RADIUS_LG)
                .bg(palette::surface(Level::Overlay, cx))
                .shadow(ui::shadow_pop(cx))
                .text_size(px(14.))
                .child(div().flex_none().pt(px(6.)).px(px(10.)).pb(px(4.)).text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(muted).child(heading))
                // The rows scroll inside the popup's height rather than spilling out over the composer.
                .child(div().id("autocomplete-list").flex_1().min_h_0().flex().flex_col().gap(px(2.)).overflow_y_scroll().track_scroll(&self.autocomplete_scroll).children(rows))
                .transform_origin(point(gpui_kit::relative(0.5), gpui_kit::relative(1.)))
                .with_animation("autocomplete", Animation::new(ui::DUR_POP).with_easing(gpui_kit::ease_out_quint()), |el, t| el.opacity(t).scale(0.95 + 0.05 * t)),
        )
    }
}

impl RowContext {
    // The name this server shows for someone: their nickname once the member is known.
    pub(crate) fn name_of(&self, user: &User) -> String {
        self.names.get(&user.id).cloned().unwrap_or_else(|| user.name.clone())
    }

    pub(crate) fn this(&self) -> WeakEntity<Waffle> {
        self.this.clone()
    }
}

pub(crate) struct RowContext {
    pub(crate) me: Option<User>,
    // Files going out with pending messages, by nonce, and their tile pictures.
    uploads: HashMap<String, crate::UploadJob>,
    previews: HashMap<std::path::PathBuf, Option<std::sync::Arc<gpui_kit::RenderImage>>>,
    // Settings › Chat: Discord's render_embeds and render_reactions.
    embeds: bool,
    reactions: bool,
    names: HashMap<String, String>,
    // The chat's surface, for drawing authors' display-name styles on; None in a server.
    style_ground: Option<Hsla>,
    colors: HashMap<String, Hsla>,
    mentions_me: std::collections::HashSet<String>,
    pub(crate) channel_names: HashMap<String, String>,
    role_names: HashMap<String, String>,
    mention_fill: Hsla,
    mention_hover: Hsla,
    mention_ink: Hsla,
    flash: Option<String>,
    editing: Option<String>,
    hovered: Option<String>,
    focused: Option<String>,
    copied: Option<String>,
    revealed: Rc<std::cell::RefCell<std::collections::HashSet<String>>>,
    text_files: Rc<std::cell::RefCell<crate::text_file::TextFiles>>,
    link_previews: crate::link_preview::Previews,
    // Servers you're in, so an invite to one reads "Joined".
    joined: std::collections::HashSet<String>,
    unread_marker: Option<u64>,
    this: WeakEntity<Waffle>,
    has_more: bool,
    dm_start: Option<crate::dm_start::DmStart>,
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
    pub(crate) notices: crate::system_message::Facts,
}

// More than a few rows below the viewport: the feed is scrolled away from its newest message.
pub(crate) fn away_from_present(visible_end: usize, count: usize) -> bool {
    visible_end + 4 < count
}

// An avatar column and a content column; a continuation shows its time in the gutter on hover, and the
// toolbar floats at the row's top right.
fn message_row(message: &Message, starts_group: bool, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    let media = media_grid(message, ctx, cx);
    let group = SharedString::from(format!("msg-{}", message.id));
    let mine = ctx.me.as_ref().is_some_and(|me| me.id == message.author.id);
    let mention = ctx.mentions_me.contains(&message.id);
    // Held at the hover fill while it's being edited, or flashed after a jump to it.
    let held = ctx.flash.as_ref() == Some(&message.id) || ctx.editing.as_ref() == Some(&message.id);
    // From state, not `group_hover`, which gpui resolves only while painting (see `track_row_hover`).
    let hovered = ctx.hovered.as_deref() == Some(message.id.as_str());
    let focused = ctx.focused.as_deref() == Some(message.id.as_str());
    let hover = palette::hover(cx);
    // Keyboard focus: the selected fill, a step past hover, kept under the pointer.
    let rest = focused.then(|| palette::selected(cx));
    // Lighter than the accent's soft fill: the rim carries the signal.
    let soft = palette::accent_soft(cx);
    let soft = Hsla { a: soft.a * 0.6, ..soft };
    let mention_hover = Hsla { a: soft.a + 0.04, ..soft };
    let ink = match message.state {
        SendState::Sent => palette::text(cx),
        SendState::Sending => palette::faint(cx),
        SendState::Failed => palette::muted(cx),
    };
    let pending = message.state != SendState::Sent;
    let landed = ctx.flash.as_ref() == Some(&message.id);
    let body = div()
        .relative()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .text_size(px(16.))
        .text_color(ink)
        .children(content(message, ctx, cx))
        .children(message.forward.as_ref().map(|original| forward_card(original, ctx, cx)))
        .children(media.map(|m| div().when(pending, |d| d.opacity(0.6)).child(m)))
        .children(message.stickers.iter().map(|s| div().when(pending, |d| d.opacity(0.6)).child(crate::stickers::in_chat(s, cx))))
        .children(message.poll.as_ref().map(|p| poll_card(message, p, ctx, cx)))
        .children(shown_embeds(message, ctx).iter().enumerate().map(|(i, e)| embed_card(e, Some(message), message.reply_to.as_deref(), format!("embed-{}-{i}", message.id), ctx, cx)))
        .children(link_cards(message, ctx, cx))
        .children(message.thread.as_ref().map(|t| thread_card(t, ctx, cx)))
        .children(files(message, ctx, cx))
        .children(upload_cards(message, ctx, cx))
        .children(reactions(message, ctx, cx).filter(|_| ctx.reactions))
        .when(message.state == SendState::Failed, |b| b.child(send_failure(message, ctx, cx)))
        // Where a jump landed: a squiggle under the message, gone with the flash.
        .when(landed, |b| b.child(div().absolute().left_0().right_0().bottom(px(-6.)).h(px(8.)).child(ui::squiggle(SharedString::from(format!("squiggle-{}", message.id)), message.id.parse().unwrap_or(0), palette::accent_ink(cx), JUMP_FLASH))));
    let row = div()
        .id(group.clone())
        .group(group.clone())
        .relative()
        .w_full()
        // A mention runs to the feed's edges, so its rim meets the sidebar; its content keeps its place.
        .px(px(if mention { 24. } else { 16. }))
        .py(px(2.))
        .squircle(RADIUS_SM)
        .flex()
        .flex_col()
        .line_height(gpui_kit::relative(1.45))
        // Mentioning you: the tint and a 2px accent bar down the left edge, square like Discord's.
        // `hovered` too: the toolbar occludes the row, so the row's own `.hover` drops while the pointer is on it.
        .when(mention, |r| r.rounded(px(0.)).bg(rest.unwrap_or(if hovered { mention_hover } else { soft })).hover(move |s| s.bg(rest.unwrap_or(mention_hover))).child(div().absolute().left_0().top_0().bottom_0().w(px(2.)).bg(palette::accent())))
        .when(!mention, |r| r.when(held || focused || hovered, |r| r.bg(rest.unwrap_or(hover))).hover(move |s| s.bg(rest.unwrap_or(hover))))
        .on_hover(track_message_hover(message, ctx));
    // Last, so it paints over the message's own content where it overlaps it.
    let toolbar = hovered.then(|| actions(message, mine, ctx, cx));
    if !starts_group {
        let gutter = div()
            .w(px(AVATAR))
            .flex_none()
            .flex()
            .justify_end()
            .line_height(px(23.))
            .text_size(px(12.))
            .text_color(palette::faint(cx))
            .whitespace_nowrap()
            .map(|g| match message.state {
                SendState::Sending => g.child(Icon::new(Glyph::Clock).size(px(14.)).mt(px(4.))),
                _ => g.when(hovered, |g| g.child(gutter_clock(&message.timestamp))),
            });
        return inset(with_message_menu(row.child(div().flex().gap(px(16.)).child(gutter).child(body)).children(toolbar), message, ctx), mention);
    }
    let row = row
        .mt(px(12.))
        .pt(px(4.))
        .children(message.reply.as_ref().map(|reply| reply_preview(reply, message.reply_to.clone(), ctx, cx)))
        .child(
            div()
                .flex()
                .gap(px(16.))
                .child(profile_trigger(format!("avatar-{}", message.id), &message.author, ctx).flex_none().mt(px(2.)).child(crate::layout::decorated_avatar(
                    &message.author.avatar,
                    message.author.decoration.as_deref(),
                    AVATAR,
                    None,
                    palette::surface(Level::Page, cx),
                )))
                .child(div().flex_1().min_w_0().flex().flex_col().child(message_head(message, ctx, cx)).child(body)),
        )
        .children(toolbar);
    inset(with_message_menu(row, message, ctx), mention)
}

// A search result: the message as the feed draws it (head, text, media, embeds, files), without the
// toolbar, reactions or reply chrome, on a card that jumps to it.
pub(crate) fn search_hit(message: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> Div {
    let media = hit_media(message, ctx, cx);
    let body = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .text_size(px(15.))
        .line_height(gpui_kit::relative(1.45))
        .text_color(palette::text(cx))
        .child(message_head(message, ctx, cx))
        .children(content(message, ctx, cx))
        .children(message.forward.as_ref().map(|original| forward_card(original, ctx, cx)))
        .children(media.map(|m| div().flex_none().child(m)))
        .children(message.embeds.iter().enumerate().map(|(i, e)| embed_card(e, None, message.reply_to.as_deref(), format!("hit-embed-{}-{i}", message.id), ctx, cx)))
        .children(files(message, ctx, cx));
    div()
        .flex()
        .gap(px(12.))
        .child(ui::avatar(message.author.avatar.clone(), 36.).flex_none().mt(px(2.)))
        .child(body)
}

// Result pictures: each in its own frame at its own shape, smaller than the feed's, side by side when
// there are several. Only a picture past `MEDIA_RATIO` (a long screenshot, a panorama) is cropped.
const HIT_MEDIA_ONE: (f32, f32) = (280., 220.);
const HIT_MEDIA_MANY: (f32, f32) = (150., 120.);
const MEDIA_RATIO: (f32, f32) = (1. / 2.5, 2.5);

fn hit_media(message: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> Option<AnyElement> {
    if message.media.is_empty() {
        return None;
    }
    let (max_w, max_h) = if message.media.len() == 1 { HIT_MEDIA_ONE } else { HIT_MEDIA_MANY };
    let max = (max_w.min(ctx.column), max_h);
    let tiles: Vec<AnyElement> = (0..message.media.len())
        .map(|i| {
            let item = &message.media[i];
            let (w, h) = fit_frame((item.width, item.height), max);
            div().flex_none().w(px(w)).h(px(h)).child(media_tile(message, i, (w, h), ui::all(RADIUS_MD), ctx, cx)).into_any_element()
        })
        .collect();
    Some(div().mt(px(8.)).flex().flex_wrap().gap(px(4.)).children(tiles).into_any_element())
}

// The frame for a picture of `size` within `max`: its own shape, never larger than it is, with the shape
// held between 1:2.5 and 2.5:1. An unknown size gets a square.
fn fit_frame((width, height): (f32, f32), (max_w, max_h): (f32, f32)) -> (f32, f32) {
    if width <= 0. || height <= 0. {
        return (max_h, max_h);
    }
    let ratio = (width / height).clamp(MEDIA_RATIO.0, MEDIA_RATIO.1);
    let w = max_w.min(max_h * ratio).min(width);
    (w, w / ratio)
}

// Rows sit 8px in from the feed's edges, so their hover fill reads as a rounded card; a mention is flush.
pub(crate) fn inset(row: AnyElement, flush: bool) -> AnyElement {
    div().w_full().when(!flush, |d| d.px(px(8.))).child(row).into_any_element()
}

// Name in its top role's colour, tags, the time, and a pin when pinned.
pub(crate) fn message_head(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> impl IntoElement + use<> {
    let name_color = ctx.colors.get(&message.author.id).copied().unwrap_or(palette::strong(cx));
    let name = ctx.name_of(&message.author);
    let runs = ctx.style_ground.zip(message.author.name_style.as_ref()).map(|(ground, style)| name_runs(&name, style, ground)).unwrap_or_default();
    // GAP: gpui text reports no baseline to taffy, so `items_baseline` would line up box bottoms and
    // lift the name. One 22px line box, centred, with the smaller time nudged 1px, shares a baseline.
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .min_w_0()
        .line_height(px(22.))
        .child(
            crate::name_fonts::styled(profile_trigger(format!("name-{}", message.id), &message.author, ctx).min_w_0().truncate().text_size(px(16.)).font_weight(FontWeight::MEDIUM), message.author.name_style.as_ref())
                .text_color(name_color)
                .hover(|s| s.underline())
                .child(StyledText::new(name).with_highlights(runs)),
        )
        .children(message.author.tag.as_ref().map(|tag| guild_tag(tag, cx)))
        .when(message.author.bot, |h| h.child(ui::badge(palette::accent_soft(cx), palette::accent_ink(cx)).h(px(18.)).px(px(6.)).gap(px(3.)).child(Icon::new(Glyph::Checkmark).size(px(11.))).child("APP")))
        .child(div().flex_none().relative().top(px(1.)).whitespace_nowrap().text_size(px(12.)).text_color(palette::faint(cx)).child(format_stamp(&message.timestamp)))
        .when(message.pinned, |h| h.child(div().flex_none().self_center().text_color(palette::muted(cx)).child(Icon::new(Glyph::Pin).size(px(14.)))))
}

// Clicking an avatar or name opens their profile popout at the pointer, as in Discord.
pub(crate) fn profile_trigger(id: String, user: &User, ctx: &Rc<RowContext>) -> Stateful<Div> {
    let (this, user) = (ctx.this.clone(), user.clone());
    let (menu_this, menu_user) = (this.clone(), user.clone());
    div()
        .id(SharedString::from(id))
        .cursor_pointer()
        .on_click(move |event, _, cx| { cx.stop_propagation();
            let at = event.position();
            this.update(cx, |this, cx| this.open_profile(user.clone(), at, cx)).ok();
        })
        // The author's menu, not the message's: Discord stops the click here.
        .on_mouse_down(gpui_kit::MouseButton::Right, move |event, _, cx| {
            cx.stop_propagation();
            let at = event.position;
            menu_this.update(cx, |this, cx| this.open_author_menu(menu_user.clone(), at, cx)).ok();
        })
}

fn track_message_hover(message: &Message, ctx: &RowContext) -> impl Fn(&bool, &mut Window, &mut App) + 'static {
    let (this, id) = (ctx.this.clone(), message.id.clone());
    move |hovered, window, cx| {
        // The row and its toolbar both report, and moving between them fires a leave and an enter in either
        // order: leaves apply now, enters after the event, so the enter always wins.
        if *hovered {
            let (this, id) = (this.clone(), id.clone());
            window.defer(cx, move |_, cx| set_hovered_message(&this, Some(id), cx));
            return;
        }
        let leaving = this.read_with(cx, |this, _| this.hovered_message.as_ref() == Some(&id)).unwrap_or(false);
        if leaving {
            set_hovered_message(&this, None, cx);
        }
    }
}

fn set_hovered_message(this: &WeakEntity<Waffle>, id: Option<String>, cx: &mut App) {
    this.update(cx, |this, cx| {
        this.hovered_message = id;
        cx.notify();
    })
    .ok();
}

// Right-click opens the message menu; a plain click on the row's own space focuses it. Controls inside
// stop their clicks, and a drag (selecting text) or a double click isn't a plain click. Alt+click anywhere
// on the row marks it unread, as in Discord: taken in the capture phase, so no link, avatar or image under
// the pointer also opens.
fn with_message_menu(row: Stateful<Div>, message: &Message, ctx: &Rc<RowContext>) -> AnyElement {
    let (this, message) = (ctx.this.clone(), message.clone());
    let (focus_this, id) = (this.clone(), message.id.clone());
    let (unread_this, unread) = (this.clone(), message.clone());
    row.capture_any_mouse_down(move |event, _, cx| {
        if event.button != gpui_kit::MouseButton::Left || !event.modifiers.alt {
            return;
        }
        if !crate::message_menu::MessageAction::MarkUnread.allowed(&unread, &Default::default()) {
            return;
        }
        cx.stop_propagation();
        unread_this.update(cx, |this, cx| this.mark_unread(&unread, cx)).ok();
    })
    .on_mouse_down(gpui_kit::MouseButton::Right, move |event, _, cx| {
        let at = event.position;
        this.update(cx, |this, cx| this.open_message_menu(message.clone(), at, cx)).ok();
    })
    .on_click(move |event, window, cx| {
        if !plain_click(event) {
            return;
        }
        focus_this.update(cx, |this, cx| this.focus_message(id.clone(), window, cx)).ok();
    })
    .into_any_element()
}

fn plain_click(event: &gpui_kit::ClickEvent) -> bool {
    let gpui_kit::ClickEvent::Mouse(click) = event else { return false };
    let moved = click.up.position - click.down.position;
    click.up.click_count == 1 && f32::from(moved.x).abs() < 4. && f32::from(moved.y).abs() < 4.
}

// The reference above a reply, in the content column; clicking it jumps to the original and flashes it.
// Discord's reply spine: from the preview's middle it runs left to over the avatar's centre, rounds
// the corner and drops to just above the avatar, so the preview lines up with the name below it.
fn reply_spine(cx: &App) -> Div {
    div()
        .absolute()
        .left(px(AVATAR / 2. - 1.))
        .top(px(REPLY_HEIGHT / 2.))
        .w(px(CONTENT_INSET - AVATAR / 2. - 4.))
        .h(px(REPLY_HEIGHT / 2. + 4.))
        .border_t(px(2.))
        .border_l(px(2.))
        .rounded_tl(px(6.))
        .border_color(palette::line_strong(cx))
}

fn reply_preview(reply: &Message, target: Option<String>, ctx: &Rc<RowContext>, cx: &App) -> AnyElement {
    let (faint, muted, text) = (palette::faint(cx), palette::muted(cx), palette::text(cx));
    let snippet = match reply.content.is_empty() {
        true => div().flex_none().flex().items_center().gap(px(4.)).text_color(muted).child(Icon::new(attachment_glyph(reply)).size(px(14.))).child(attachment_kind(reply)),
        false => div().min_w_0().truncate().child(plain(&readable_with(&reply.content, ctx, false))),
    };
    let this = ctx.this.clone();
    div()
        .id(SharedString::from(format!("reply-{}", reply.id)))
        .relative()
        .h(px(REPLY_HEIGHT))
        .pl(px(CONTENT_INSET))
        .mb(px(2.))
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(6.))
        .cursor_pointer()
        .text_size(px(14.))
        .text_color(faint)
        .hover(move |s| s.text_color(text))
        .child(reply_spine(cx))
        .child(ui::avatar(reply.author.avatar.clone(), 16.))
        .child(div().flex_none().font_weight(FontWeight::MEDIUM).text_color(muted).child(ctx.name_of(&reply.author)))
        .child(snippet)
        .on_click(move |_, _, cx| { cx.stop_propagation();
            let Some(id) = target.clone() else { return };
            this.update(cx, |this, cx| this.jump_to(id, cx)).ok();
        })
        .into_any_element()
}

// A forwarded copy (design/messages.html `.fwd`): the label, the original's text, media and files, then
// where it came from, which jumps to the original. A source outside your servers shows its time alone.
fn forward_card(original: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    let faint = palette::faint(cx);
    let label = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette::muted(cx))
        .child(Icon::new(Glyph::Forward).size(px(14.)))
        .child("Forwarded");
    let name = ctx.channel_names.get(&original.channel_id).cloned();
    let glyph = if original.guild_id.is_some() { Glyph::Hashtag } else { Glyph::User };
    let stamp = format_stamp(&original.timestamp);
    let source = div()
        .id(SharedString::from(format!("forward-source-{}", original.id)))
        .flex()
        .items_center()
        .gap(px(4.))
        .text_size(px(12.))
        .text_color(faint)
        .when_some(name.clone(), |s, name| s.child(Icon::new(glyph).size(px(14.))).child(format!("{name} · {stamp}")))
        .when(name.is_none(), |s| s.child(stamp));
    let source = match name {
        Some(_) => {
            let (this, guild_id, channel_id, id) = (ctx.this.clone(), original.guild_id.clone(), original.channel_id.clone(), original.id.clone());
            source.cursor_pointer().hover(|s| s.text_color(palette::muted(cx))).on_click(move |_, window, cx| { cx.stop_propagation();
                this.update(cx, |this, cx| this.open_message(guild_id.clone(), &channel_id, id.clone(), window, cx)).ok();
            })
        }
        None => source,
    };
    div()
        .mt(px(4.))
        .max_w(px(FORWARD_WIDTH))
        .pt(px(8.))
        .px(px(12.))
        .pb(px(10.))
        .squircle(RADIUS_MD)
        .bg(palette::strong(cx).opacity(0.05))
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(label)
        .children(content(original, ctx, cx))
        .children(media_grid(original, ctx, cx))
        .children(original.stickers.iter().map(|s| crate::stickers::in_chat(s, cx)))
        .children(shown_embeds(original, ctx).iter().enumerate().map(|(i, e)| embed_card(e, None, None, format!("forward-embed-{}-{i}", original.id), ctx, cx)))
        .children(files(original, ctx, cx))
        .child(source)
        .into_any_element()
}

// What a reply without text quotes instead: the kind of attachment it carries.
fn attachment_kind(message: &Message) -> &'static str {
    match message.media.first().map(|m| &m.kind) {
        Some(MediaKind::Image) => "Image",
        Some(MediaKind::Video { .. }) => "Video",
        Some(MediaKind::Gif { .. }) => "GIF",
        None if !message.stickers.is_empty() => "Sticker",
        None if !message.files.is_empty() => "File",
        None => "Attachment",
    }
}

fn attachment_glyph(message: &Message) -> Glyph {
    if message.media.is_empty() && !message.stickers.is_empty() {
        return Glyph::Sticker;
    }
    match message.media.is_empty() && !message.files.is_empty() {
        true => Glyph::File,
        false => Glyph::Image,
    }
}

// Discord drops the text of a message that is nothing but the link its GIF or image embed shows.
fn only_media_link(message: &Message) -> bool {
    let text = message.content.trim();
    let is_link = (text.starts_with("https://") || text.starts_with("http://")) && !text.contains(char::is_whitespace);
    is_link && !message.media.is_empty() && message.embeds.is_empty()
}

// A DM call's record: one line in the history. A call you missed is the only red one, and it offers Call back.
pub(crate) fn call_line(message: &Message, call: &crate::discord::CallRecord, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    let me = ctx.me.as_ref().map(|m| m.id.as_str()).unwrap_or_default();
    let missed = call.ended.is_some() && message.author.id != me && !call.participants.iter().any(|p| p == me);
    let lasted = call.ended.as_deref().and_then(|end| Some((local(end)? - local(&message.timestamp)?).num_seconds())).map(crate::dm_call::lasted);
    let s = Sentence::new(&message.id, cx);
    let (sentence, action) = match (missed, lasted) {
        (true, Some(lasted)) => (s.text("You missed a call from ").name(&message.author, ctx).text(&format!(" that lasted {lasted}.")), Some("Call back")),
        (true, None) => (s.text("You missed a call from ").name(&message.author, ctx).text("."), Some("Call back")),
        (false, Some(lasted)) => (s.name(&message.author, ctx).text(&format!(" started a call that lasted {lasted}.")), None),
        (false, None) => (s.name(&message.author, ctx).text(" started a call."), Some("Join")),
    };
    let ink = if missed { palette::danger_ink(cx) } else { palette::positive_ink(cx) };
    let sentence = match action {
        Some(label) => {
            let (this, channel_id) = (ctx.this.clone(), message.channel_id.clone());
            let link = sentence.link(label).text_color(palette::accent_ink(cx)).on_click(move |_, _, cx| { cx.stop_propagation();
                this.update(cx, |this, cx| this.answer_dm_call(&channel_id, cx)).ok();
            });
            sentence.text(" \u{2014} ").part(link)
        }
        None => sentence,
    };
    notice_line(&message.id, Glyph::Phone, ink, sentence, Some(format_stamp(&message.timestamp)), Vec::new(), cx)
}

// Discord's pin notice: who pinned, a link to the message, a link to the pins.
pub(crate) fn pin_line(message: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    let s = Sentence::new(&message.id, cx).name(&message.author, ctx).text(" pinned ");
    let (this, target) = (ctx.this.clone(), message.reply_to.clone());
    let jump = s.link("a message").on_click(move |_, _, cx| { cx.stop_propagation();
        let Some(id) = target.clone() else { return };
        this.update(cx, |this, cx| this.jump_to(id, cx)).ok();
    });
    let s = s.part(jump).text(" to this channel. See all ");
    let this = ctx.this.clone();
    let pins = s.link("pinned messages").on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| { cx.stop_propagation();
        this.update(cx, |this, cx| {
            this.popover_at = gpui_kit::point(px(this.pins_button_x.get()), px(0.));
            this.open_pins(cx);
        })
        .ok();
    });
    let sentence = s.part(pins).text(".");
    notice_line(&message.id, Glyph::Pin, palette::muted(cx), sentence, Some(format_stamp(&message.timestamp)), Vec::new(), cx)
}

// A poll-result notice's cards, under its line.
pub(crate) fn poll_results(message: &Message, ctx: &RowContext, cx: &App) -> Vec<AnyElement> {
    message.embeds.iter().enumerate().filter_map(|(i, e)| Some(poll_result(e.poll.as_ref()?, message.reply_to.as_deref(), &format!("poll-result-{}-{i}", message.id), ctx, cx))).collect()
}

// A thread starter's parent message, whole but inert: no toolbar, reactions or thread chip, as Discord draws it.
pub(crate) fn starter_message(parent: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> Div {
    let body = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .text_size(px(16.))
        .text_color(palette::text(cx))
        .child(message_head(parent, ctx, cx))
        .children(content(parent, ctx, cx))
        .children(parent.forward.as_ref().map(|original| forward_card(original, ctx, cx)))
        .children(media_grid(parent, ctx, cx))
        .children(parent.stickers.iter().map(|s| crate::stickers::in_chat(s, cx)))
        .children(parent.poll.as_ref().map(|p| poll_card(parent, p, ctx, cx)))
        .children(shown_embeds(parent, ctx).iter().enumerate().map(|(i, e)| embed_card(e, None, parent.reply_to.as_deref(), format!("starter-embed-{}-{i}", parent.id), ctx, cx)))
        .children(files(parent, ctx, cx));
    div()
        .flex()
        .flex_col()
        .line_height(gpui_kit::relative(1.45))
        .children(parent.reply.as_ref().map(|reply| reply_preview(reply, parent.reply_to.clone(), ctx, cx)))
        .child(
            div()
                .flex()
                .gap(px(16.))
                .child(profile_trigger(format!("starter-avatar-{}", parent.id), &parent.author, ctx).flex_none().mt(px(2.)).child(ui::avatar(parent.author.avatar.clone(), AVATAR)))
                .child(body),
        )
}

pub(crate) fn content(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> Option<AnyElement> {
    if message.content.is_empty() || only_media_link(message) {
        return None;
    }
    // A message of only custom emoji renders them big, as Discord's "jumbo" emoji.
    let emoji = custom_emoji(&message.content);
    if !emoji.is_empty() && strip_emoji(&message.content).trim().is_empty() && emoji.len() <= 27 {
        return Some(div().flex().flex_wrap().gap_1().children(emoji.into_iter().filter_map(|e| e.url()).enumerate().map(|(i, url)| img(url).id(("jumbo", i)).size(px(48.)))).into_any_element());
    }
    // Spoilers stay covered until the message is clicked; "-# " lines are Discord's small grey subtext.
    let revealed = ctx.revealed.borrow().contains(&message.id);
    let has_spoiler = message.content.contains("||");
    let (body, subtext) = split_subtext(&message.content);
    let text = hide_spoilers(&readable(&body, ctx), revealed);
    let view = markdown(SharedString::from(format!("md-{}", message.id)), text, ctx);
    let (reveal, id) = (ctx.revealed.clone(), message.id.clone());
    let refresh = ctx.this.clone();
    let faint = palette::faint(cx);
    Some(
        div()
            .id(SharedString::from(format!("content-{}", message.id)))
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .when(has_spoiler && !revealed, |c| {
                c.cursor_pointer().on_click(move |_, _, cx| { cx.stop_propagation();
                    reveal.borrow_mut().insert(id.clone());
                    refresh.update(cx, |_, cx| cx.notify()).ok();
                })
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_baseline()
                    // Capped at the row's width so long lines wrap instead of running off the side.
                    // A code block spans the message's width, as Discord's do; plain text wraps to its own.
                    .when(!body.trim().is_empty(), |c| c.child(div().min_w_0().max_w_full().when(body.contains("```"), |d| d.w_full()).child(view)))
                    .when(message.edited, |c| c.child(div().ml(px(4.)).whitespace_nowrap().text_size(px(12.)).text_color(faint).child("(edited)"))),
            )
            .children(subtext.into_iter().map(|line| div().text_size(px(12.)).text_color(faint).child(line)))
            .into_any_element(),
    )
}

// Mentions, role and channel links draw as badges (messages.css `.at`): accent ink on the soft fill.
struct MentionBadges {
    this: WeakEntity<Waffle>,
    fill: Hsla,
    hover: Hsla,
    ink: Hsla,
}

#[derive(Clone)]
struct Mention {
    url: String,
    label: String,
}

impl MarkdownPlugin for MentionBadges {
    fn name(&self) -> &str {
        "mention"
    }

    fn parse(&self, node: &markdown_ast::Node, _: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let markdown_ast::Node::Link(link) = node else { return None };
        if !link.url.starts_with("waffle://") {
            return None;
        }
        let label = node.to_string();
        Some(MarkdownNode::new("mention", Mention { url: link.url.clone(), label: label.clone() }).text(label))
    }

    fn render_inline(&self, node: &MarkdownNode, _: &InlineRenderContext, _: &mut Window, _: &mut App) -> Option<InlineElement> {
        let mention = node.data::<Mention>()?.clone();
        let (this, hover) = (self.this.clone(), self.hover);
        let badge = mention_badge(mention.label.into(), self.fill, self.ink)
            .id(SharedString::from(format!("at-{}", mention.url)))
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .on_click(move |event, window, cx| { cx.stop_propagation();
                let url = mention.url.clone();
                this.update(cx, |this, cx| this.open_mention(&url, event.position(), window, cx)).ok();
            });
        Some(InlineElement::new(badge))
    }
}

// A mention as messages draw it; the composer's mention chips wear the same.
fn mention_badge(label: SharedString, fill: Hsla, ink: Hsla) -> Div {
    div().px(px(3.)).rounded(px(4.)).bg(fill).text_color(ink).font_weight(FontWeight::MEDIUM).whitespace_nowrap().child(label)
}

// The composer's mention tokens, in the message badge; the hover fill marks one inside the selection.
fn mention_chip(cx: &App) -> impl Fn(&gpui_kit::component::input::InlineTokenContext, &mut Window, &mut App) -> Div + 'static {
    let (fill, selected, ink) = (palette::accent_soft(cx), palette::accent().opacity(0.26), palette::accent_ink(cx));
    move |token, _, _| mention_badge(token.token().label().clone(), if token.is_selected() { selected } else { fill }, ink).h(token.line_height()).flex().items_center()
}

// Discord markdown with mentions, channel links and code copy buttons; messages and embeds share it.
fn markdown(id: SharedString, text: String, ctx: &RowContext) -> TextView {
    let this = ctx.this.clone();
    let (copy_target, copied) = (ctx.this.clone(), ctx.copied.clone());
    let badges = MentionBadges { this: ctx.this.clone(), fill: ctx.mention_fill, hover: ctx.mention_hover, ink: ctx.mention_ink };
    TextView::markdown(id, discord_breaks(&text))
        // Constant: the plugin only changes how mentions draw, never what parses.
        .markdown_extensions(MarkdownExtensions::default().parser_revision(1).plugin(badges))
        // messages.css `.codeblock { margin-top: 6px }`: a block opening a message clears the name row.
        .style(gpui_kit::component::text::TextViewStyle::default().code_block(gpui_kit::StyleRefinement::default().mt(px(6.))))
        .selectable(true)
        .code_block_actions(move |block, _, cx| copy_button(block.code().to_string(), copied.as_deref() == Some(block.code().as_ref()), copy_target.clone(), cx))
        .on_link_click(move |url, _, window, cx| {
            let url = url.to_string();
            if let Some(channel_id) = url.strip_prefix("waffle://channel/") {
                this.update(cx, |this, cx| {
                    let channel = this.guild().and_then(|g| g.channels.iter().find(|c| c.id == channel_id).cloned());
                    if let Some(channel) = channel {
                        this.open_channel(channel, window, cx);
                    }
                })
                .ok();
                return;
            }
            if !url.starts_with("waffle://") {
                this.update(cx, |this, cx| this.open_link(&url, window, cx)).ok();
            }
        })
}

// Discord keeps every line break and a line's leading spaces; markdown joins single breaks into a
// space and drops indentation. Each break becomes a hard break and leading spaces non-breaking ones,
// outside fenced code, which already keeps both.
pub(crate) fn discord_breaks(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 8);
    let mut in_fence = false;
    let mut lines = text.split('\n').peekable();
    while let Some(line) = lines.next() {
        let fence = line.trim_start().starts_with("```");
        if in_fence || fence {
            out.push_str(line);
        } else {
            let indent = line.len() - line.trim_start_matches(' ').len();
            out.extend(std::iter::repeat_n('\u{a0}', indent));
            out.push_str(&line[indent..]);
        }
        if fence {
            in_fence = !in_fence;
        }
        let Some(next) = lines.peek() else { break };
        let breaks = !in_fence && !fence && !line.trim().is_empty() && !next.trim().is_empty() && !next.trim_start().starts_with("```");
        out.push_str(if breaks { "  \n" } else { "\n" });
    }
    out
}

fn split_subtext(content: &str) -> (String, Vec<String>) {
    let (sub, body): (Vec<&str>, Vec<&str>) = content.lines().partition(|l| l.starts_with("-# "));
    (body.join("\n"), sub.into_iter().map(|l| l.trim_start_matches("-# ").to_owned()).collect())
}

// `||secret||` → a solid bar of the same length until revealed (gpui's markdown can't paint a cover).
fn hide_spoilers(text: &str, revealed: bool) -> String {
    let parts: Vec<&str> = text.split("||").collect();
    // An odd number of `||` leaves the last one unmatched: it stays literal text.
    let paired = if parts.len().is_multiple_of(2) { parts.len() - 1 } else { parts.len() };
    let mut out = String::with_capacity(text.len());
    for (i, part) in parts.iter().enumerate() {
        match (i % 2 == 1 && i < paired, revealed) {
            (true, true) => out.push_str(part),
            (true, false) => {
                out.push('`');
                out.extend(std::iter::repeat_n('█', part.chars().count().clamp(1, 40)));
                out.push('`');
            }
            (false, _) if i >= paired => {
                out.push_str("||");
                out.push_str(part);
            }
            (false, _) => out.push_str(part),
        }
    }
    out
}

// A user's server tag (Discord's guild tag): its badge and text, as it follows their name.
pub(crate) fn guild_tag(tag: &crate::discord::GuildTag, cx: &App) -> Div {
    ui::badge(palette::selected(cx), palette::strong(cx)).h(px(18.)).px(px(6.)).gap(px(3.)).children(tag.badge.clone().map(|b| img(b).size(px(12.)))).child(tag.text.clone())
}

// A display name in its Discord style's colours, one run per character, each colour stepped in lightness
// until it reads on `ground` as Discord's client does (3:1, 2.5:1 for the multi-colour fills). GAP: gpui
// text has no gradient fill, stroke or glow, so gradients and prisms step per character and neon, toon
// and pop draw in their main colour; the style's font isn't drawn yet.
pub(crate) fn name_runs(name: &str, style: &NameStyle, ground: Hsla) -> Vec<(std::ops::Range<usize>, HighlightStyle)> {
    let colors = match style.colors.is_empty() {
        true => default_name_colors(style.effect),
        false => style.colors.as_slice(),
    };
    let ratio = if matches!(style.effect, NameEffect::Gradient | NameEffect::Glow | NameEffect::Prism) { 2.5 } else { 3. };
    let inks: Vec<Hsla> = colors.iter().map(|c| readable_on(gpui_kit::rgb(*c).into(), ground, ratio)).collect();
    if inks.is_empty() {
        return Vec::new();
    }
    let count = name.chars().count().max(1) as f32;
    let mut letter = 0;
    name.char_indices()
        .enumerate()
        .map(|(i, (at, ch))| {
            let along = (i as f32 + 0.5) / count;
            let ink = match style.effect {
                NameEffect::Gummy => inks[letter % inks.len()],
                // Stops spread over 10%..90%, as Discord lays them.
                NameEffect::Gradient | NameEffect::Glow => along_stops(&inks, ((along - 0.1) / 0.8).clamp(0., 1.)),
                // One cycle through every colour and back to the first.
                NameEffect::Prism => along_stops(&[inks.as_slice(), &inks[..1]].concat(), along),
                NameEffect::Solid | NameEffect::Neon | NameEffect::Toon | NameEffect::Pop => inks[0],
            };
            letter += usize::from(!ch.is_whitespace());
            (at..at + ch.len_utf8(), HighlightStyle { color: Some(ink), ..Default::default() })
        })
        .collect()
}

// A style sent without colours draws its effect's defaults (Discord's web client, 2026-11-19); solid's is
// the theme's own text colour.
fn default_name_colors(effect: NameEffect) -> &'static [u32] {
    match effect {
        NameEffect::Solid => &[],
        NameEffect::Gradient | NameEffect::Glow => &[0x2AAEA6, 0xFFC490],
        NameEffect::Gummy => &[0xE9A9D5, 0xA9DDF0, 0xB9A7EE, 0xB9EACB],
        NameEffect::Prism => &[0xFE9242, 0x35EC7E, 0xFFE047, 0xFF4CD2, 0x5865F2],
        NameEffect::Toon => &[0xF42098],
        NameEffect::Neon => &[0x691DED],
        NameEffect::Pop => &[0x0FCF86],
    }
}

// The colour at `t` (0..1) along evenly spaced stops.
fn along_stops(stops: &[Hsla], t: f32) -> Hsla {
    let span = t * (stops.len() - 1) as f32;
    let from = (span.floor() as usize).min(stops.len() - 1);
    ui::mix(stops[from], stops[(from + 1).min(stops.len() - 1)], span - from as f32)
}

// Discord's clamp: 0.05 lighter (on a dark ground) or darker per step, at most ten steps.
fn readable_on(mut ink: Hsla, ground: Hsla, ratio: f32) -> Hsla {
    let lighten = palette::luminance(ground) <= 0.5;
    for _ in 0..10 {
        if palette::contrast(ink, ground) >= ratio || (lighten && ink.l >= 0.95) || (!lighten && ink.l <= 0.05) {
            break;
        }
        ink.l += if lighten { 0.05 } else { -0.05 };
    }
    ink
}

// Not sent: the text stays, with why and the two ways out inline.
fn send_failure(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> AnyElement {
    let (retry_this, retry) = (ctx.this.clone(), message.clone());
    let (delete_this, delete) = (ctx.this.clone(), message.clone());
    let action = |id: String, label: &'static str| ui::button(SharedString::from(id), ui::Tone::Ghost, cx).small().h(px(28.)).child(label);
    div()
        .mt(px(4.))
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x(px(8.))
        .gap_y(px(4.))
        .text_size(px(14.))
        .font_weight(FontWeight::NORMAL)
        .text_color(palette::danger_ink(cx))
        .child(Icon::new(Glyph::WarningCircle).size(px(16.)))
        .child(match message.nonce.as_ref().and_then(|n| ctx.uploads.get(n)?.error.as_ref()) {
            Some(error) => format!("Not sent: {error}"),
            None => "Not sent.".into(),
        })
        .child(action(format!("retry-{}", message.id), "Resend message").on_click(move |_, _, cx| { cx.stop_propagation();
            retry_this.update(cx, |this, cx| this.retry(retry.clone(), cx)).ok();
        }))
        .child(action(format!("discard-{}", message.id), "Delete message").on_click(move |_, _, cx| { cx.stop_propagation();
            delete_this.update(cx, |this, cx| this.delete(delete.clone(), cx)).ok();
        }))
        .into_any_element()
}

// The card under the message that started a thread: its name and how many messages it holds.
fn thread_card(thread: &Channel, ctx: &Rc<RowContext>, cx: &App) -> AnyElement {
    let meta = thread.thread.as_ref();
    let count = meta.map_or(0, |t| t.message_count);
    let archived = meta.is_some_and(|t| t.archived);
    let (this, open) = (ctx.this.clone(), thread.clone());
    let edge = palette::line_strong(cx);
    ui::pressable(div().id(SharedString::from(format!("thread-card-{}", thread.id))), 0.99)
        .mt(px(8.))
        .w_full()
        .max_w(px(COLUMN))
        .py(px(10.))
        .px(px(12.))
        .flex()
        .items_center()
        .gap(px(8.))
        .min_w_0()
        .squircle(RADIUS_LG)
        .bg(palette::surface(Level::Sidebar, cx))
        .border_1()
        .border_color(palette::line(cx))
        .hover(move |s| s.border_color(edge))
        .cursor_pointer()
        .text_size(px(14.))
        .child(div().min_w_0().truncate().font_weight(FontWeight::MEDIUM).text_color(if archived { palette::muted(cx) } else { palette::strong(cx) }).child(thread.name.clone()))
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(2.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::muted(cx))
                .child(format!("{count} {}", if count == 1 { "message" } else { "messages" }))
                .child(Icon::new(Glyph::ChevronRight).size(px(14.))),
        )
        .on_click(move |_, window, cx| { cx.stop_propagation();
            this.update(cx, |this, cx| this.open_channel(open.clone(), window, cx)).ok();
        })
        .into_any_element()
}

// gpui only advances an animated image's frames when the img has an id to keep its frame state under,
// so every img that may hold a GIF (or animated WebP) gets one.
pub(crate) fn animated_image(source: impl Into<gpui_kit::ImageSource>) -> Stateful<gpui_kit::Img> {
    img(source).id("animated")
}

// Media at its own shape: one alone fits 400×320; several tile the 432px column (2 side by side,
// 3 one tall and two stacked, 4 two by two, 5 or more two over three with the last counting the rest).
fn media_grid(message: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> Option<AnyElement> {
    let first = message.media.first()?;
    if message.media.len() == 1 {
        let scale = (MAX_IMAGE.0.min(ctx.column) / first.width).min(MAX_IMAGE.1 / first.height).min(1.);
        let (w, h) = (first.width * scale, first.height * scale);
        return Some(div().mt(px(8.)).flex_none().w(px(w)).h(px(h)).child(media_tile(message, 0, (w, h), ui::all(RADIUS_LG), ctx, cx)).into_any_element());
    }
    // Laid out for the feed's column, then scaled down to fit a narrower one.
    let k = (ctx.column / COLUMN).min(1.);
    let (height, tiles) = gallery_layout(message.media.len());
    let (height, width) = (height * k, COLUMN * k);
    let tiles: Vec<Tile> = tiles.into_iter().map(|t| Tile { x: t.x * k, y: t.y * k, w: t.w * k, h: t.h * k }).collect();
    let rest = message.media.len() - tiles.len();
    let mut grid = div().mt(px(8.)).relative().flex_none().w(px(width)).h(px(height));
    for (i, tile) in tiles.iter().enumerate() {
        let corners = gallery_corners(*tile, width, height, RADIUS_LG);
        let counts_rest = rest > 0 && i + 1 == tiles.len();
        grid = grid.child(
            div()
                .absolute()
                .left(px(tile.x))
                .top(px(tile.y))
                .w(px(tile.w))
                .h(px(tile.h))
                .child(media_tile(message, i, (tile.w, tile.h), corners, ctx, cx))
                .when(counts_rest, |t| {
                    t.child(
                        ui::edge(div(), corners)
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(on_media(0.55))
                            .text_color(gpui_kit::white())
                            .text_size(px(20.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("+{rest}")),
                    )
                }),
        );
    }
    Some(grid.into_any_element())
}

// One picture, video or GIF, rounded to `corners`; a click opens the viewer at it.
fn media_tile(message: &Message, i: usize, (w, h): (f32, f32), corners: Corners<Pixels>, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    let item = &message.media[i];
    // A video plays where it is; clicks on it belong to its player.
    if let MediaKind::Video { poster } = &item.kind {
        return div().size_full().child(crate::video::tile(format!("{}-{i}", message.id), &item.url, poster, cx)).into_any_element();
    }
    // Fetched at the size it's drawn rather than full resolution; the viewer still opens the original.
    let sized = |url: &str| crate::discord::sized_image(url, w, h, (item.width, item.height));
    let picture = match &item.kind {
        MediaKind::Gif { poster } => div()
            .relative()
            .size_full()
            .child(crate::gif::rounded_tile(format!("{}-{i}-{}", message.id, item.url), &item.url, &sized(poster), gpui_kit::size(px(w), px(h)), corners, cx))
            .child(media_tag("GIF").top(px(8.)).left(px(8.)))
            .into_any_element(),
        _ => ui::edge(animated_image(sized(&item.url)), corners).size_full().object_fit(gpui_kit::ObjectFit::Cover).into_any_element(),
    };
    let (this, url) = (ctx.this.clone(), item.url.clone());
    ui::edge(div(), corners)
        .id(SharedString::from(format!("media-{}-{i}", message.id)))
        .size_full()
        // A cover-fit picture whose real shape differs from the one Discord reported spills past the tile.
        .overflow_hidden()
        .bg(palette::selected(cx))
        .cursor_pointer()
        .hover(|s| s.opacity(0.92))
        .child(picture)
        .on_click(move |_, window, cx| { cx.stop_propagation();
            this.update(cx, |this, cx| this.open_viewer(&url, window, cx)).ok();
        })
        .into_any_element()
}

// A label over media (GIF, a duration): fixed dark values, since it sits on pictures in both themes.
fn media_tag(text: &'static str) -> Div {
    div().absolute().h(px(20.)).px(px(6.)).flex().items_center().gap(px(4.)).squircle(RADIUS_XS).bg(on_media(0.72)).text_color(gpui_kit::white()).text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).child(text)
}

// The design's media scrim, oklch(0.14 0.004 260), at `alpha`.
fn on_media(alpha: f32) -> Hsla {
    Hsla::from(gpui_kit::rgb(0x08090B)).opacity(alpha)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Tile {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

// The gallery's height and its tiles for `count` items in the media column; past five, five tiles show.
fn gallery_layout(count: usize) -> (f32, Vec<Tile>) {
    let tile = |x, y, w, h| Tile { x, y, w, h };
    let half = (COLUMN - GALLERY_GAP) / 2.;
    match count {
        0 | 1 => (0., Vec::new()),
        2 => {
            let h = COLUMN / 2.;
            (h, vec![tile(0., 0., half, h), tile(half + GALLERY_GAP, 0., half, h)])
        }
        3 => {
            let h = COLUMN * 2. / 3.;
            let wide = (COLUMN - GALLERY_GAP) * 2. / 3.;
            let (narrow, row) = (COLUMN - GALLERY_GAP - wide, (h - GALLERY_GAP) / 2.);
            let right = wide + GALLERY_GAP;
            (h, vec![tile(0., 0., wide, h), tile(right, 0., narrow, row), tile(right, row + GALLERY_GAP, narrow, row)])
        }
        4 => {
            let h = COLUMN * 3. / 4.;
            let row = (h - GALLERY_GAP) / 2.;
            (h, [(0., 0.), (1., 0.), (0., 1.), (1., 1.)].map(|(c, r)| tile(c * (half + GALLERY_GAP), r * (row + GALLERY_GAP), half, row)).to_vec())
        }
        _ => {
            let h = COLUMN * 2. / 3.;
            let top = (h - GALLERY_GAP) * 3. / 5.;
            let (bottom, third) = (h - GALLERY_GAP - top, (COLUMN - 2. * GALLERY_GAP) / 3.);
            let mut tiles = vec![tile(0., 0., half, top), tile(half + GALLERY_GAP, 0., half, top)];
            tiles.extend((0..3).map(|i| tile(i as f32 * (third + GALLERY_GAP), top + GALLERY_GAP, third, bottom)));
            (h, tiles)
        }
    }
}

// The gallery's outer corners take the card radius; inner ones a hairline 2px.
fn gallery_corners(tile: Tile, width: f32, height: f32, outer: f32) -> Corners<Pixels> {
    let at = |a: f32, b: f32| (a - b).abs() < 0.5;
    let (left, top, right, bottom) = (at(tile.x, 0.), at(tile.y, 0.), at(tile.x + tile.w, width), at(tile.y + tile.h, height));
    let radius = |edge: bool| px(if edge { outer } else { 2. });
    Corners { top_left: radius(top && left), top_right: radius(top && right), bottom_left: radius(bottom && left), bottom_right: radius(bottom && right) }
}

// A link preview: it leads with the site, never a coloured bar, one surface step from the feed. Link
// previews keep a thumbnail at the right; large images run the card's width.
// Previews of the Discord links the message carries (link_preview.rs).
fn link_cards(message: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> Vec<AnyElement> {
    // Discord's one setting covers both: "Show embeds and preview website links".
    if message.state != SendState::Sent || !ctx.embeds {
        return Vec::new();
    }
    let quote = |content: &str| plain(&readable_with(content, ctx, false));
    let cards = crate::link_preview::CardContext { this: ctx.this.clone(), previews: &ctx.link_previews, joined: &ctx.joined, channel_names: &ctx.channel_names, quote: &quote, stamp: format_stamp };
    crate::link_preview::cards(&message.id, &message.content, &cards, cx)
}

// Settings › Chat can turn embeds off, and Discord's SUPPRESS_EMBEDS flag removes a message's for everyone.
fn shown_embeds<'a>(message: &'a Message, ctx: &RowContext) -> &'a [Embed] {
    if !ctx.embeds || message.embeds_suppressed() {
        return &[];
    }
    &message.embeds
}

// Discord's ✕ on embed cards: the author or a moderator, on a message that has something besides its
// embeds (text, stickers, a poll, a forward), so a bot's embed-only post keeps them. Not on Discord's
// own messages, ephemeral ones or a thread's starter copy (type 21).
// Not verified: Discord also counts components and activities, which Waffle doesn't parse.
pub(crate) fn can_remove_embeds(message: &Message, me: Option<&str>, manage_messages: bool) -> bool {
    let allowed = me == Some(message.author.id.as_str()) || manage_messages;
    let other_content = !message.content.is_empty() || !message.stickers.is_empty() || message.poll.is_some() || message.forward.is_some();
    allowed && message.author.id != "1" && message.flags & crate::discord::message_flag::EPHEMERAL == 0 && message.kind != 21 && other_content
}

// A rich embed (design/embeds.html), Discord's embed object field for field. Its colour is Discord's bar
// down the left edge, so a bot's colours read the way its users know them. Text sits beside an 80px
// thumbnail; pictures, the footer and the time run the card's width under it. `row` is the feed message
// that carries it; forwards and search hits pass None and get no ✕.
fn embed_card(embed: &Embed, row: Option<&Message>, poll_message: Option<&str>, key: String, ctx: &RowContext, cx: &App) -> AnyElement {
    if let Some(poll) = &embed.poll {
        return poll_result(poll, poll_message, &key, ctx, cx);
    }
    let (strong, faint) = (palette::strong(cx), palette::faint(cx));
    let fill = palette::surface(Level::Sidebar, cx);
    let bar = embed.color.map_or(palette::line_strong(cx), |c| gpui_kit::rgb(c).into());
    let width = EMBED_WIDTH.min(ctx.column);
    let inner = width - 2. * EMBED_PAD.1 - EMBED_BAR - 2.;
    let md = |part: &str, text: &str| markdown(SharedString::from(format!("{key}-{part}")), readable(text, ctx), ctx);
    let open = |url: String| {
        let this = ctx.this.clone();
        move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
            cx.stop_propagation();
            this.update(cx, |this, cx| this.open_link(&url, window, cx)).ok();
        }
    };

    let provider = embed.provider.clone().map(|name| {
        div()
            .id(SharedString::from(format!("{key}-provider")))
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(faint)
            .when_some(embed.provider_url.clone(), |d, url| d.cursor_pointer().hover(|s| s.underline()).on_click(open(url)))
            .child(name)
    });
    let author = embed.author.clone().map(|name| {
        div()
            .id(SharedString::from(format!("{key}-author")))
            .flex()
            .items_center()
            .gap(px(8.))
            .min_w_0()
            .text_size(px(14.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(strong)
            .children(embed.author_icon.clone().map(|icon| ui::avatar(icon, 24.)))
            .child(div().min_w_0().truncate().when(embed.author_url.is_some(), |d| d.cursor_pointer().hover(|s| s.underline())).child(name))
            .when_some(embed.author_url.clone(), |d, url| d.on_click(open(url)))
    });
    let title = embed.title.clone().map(|title| {
        div()
            .id(SharedString::from(format!("{key}-title")))
            .text_size(px(16.))
            .line_height(gpui_kit::relative(1.2))
            .font_weight(FontWeight::BOLD)
            .text_color(if embed.url.is_some() { palette::accent_ink(cx) } else { strong })
            .when_some(embed.url.clone(), |t, url| t.cursor_pointer().hover(|s| s.underline()).on_click(open(url)))
            .child(title)
    });
    let description = embed.description.as_deref().map(|text| {
        let more = format!("{key}-more");
        let long = long_description(text);
        let open_full = !long || ctx.revealed.borrow().contains(&more);
        let (reveal, refresh) = (ctx.revealed.clone(), ctx.this.clone());
        div()
            .flex()
            .flex_col()
            .items_start()
            .child(
                div()
                    .relative()
                    .w_full()
                    .text_color(palette::text(cx))
                    .when(!open_full, |d| d.max_h(px(EMBED_CLIP)).overflow_hidden())
                    .child(md("description", text))
                    // The fade says there is more; it ends in the card's own fill.
                    .when(!open_full, |d| d.child(div().absolute().left_0().right_0().bottom_0().h(px(72.)).bg(linear_gradient(180., linear_color_stop(fill.opacity(0.), 0.), linear_color_stop(fill, 1.))))),
            )
            .when(long, |d| {
                d.child(
                    div()
                        .id(SharedString::from(more.clone()))
                        .mt(px(2.))
                        .ml(px(-6.))
                        .h(px(26.))
                        .px(px(6.))
                        .flex()
                        .items_center()
                        .squircle(RADIUS_XS)
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::accent_ink(cx))
                        .cursor_pointer()
                        .hover(|s| s.bg(palette::hover(cx)))
                        .child(if open_full { "Show less" } else { "Show more" })
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            let mut revealed = reveal.borrow_mut();
                            if !revealed.remove(&more) {
                                revealed.insert(more.clone());
                            }
                            drop(revealed);
                            refresh.update(cx, |_, cx| cx.notify()).ok();
                        }),
                )
            })
    });
    let fields = (!embed.fields.is_empty()).then(|| {
        div().mt(px(4.)).flex().flex_col().gap(px(10.)).children(field_rows(&embed.fields, embed.thumbnail.is_some()).into_iter().map(|row| {
            div().flex().gap(px(16.)).children(row.into_iter().map(|i| {
                let (name, value, _) = &embed.fields[i];
                div()
                    .flex_1()
                    .flex_basis(px(0.))
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::muted(cx)).child(name.clone()))
                    .child(div().text_color(strong).child(md(&format!("field-{i}"), value)))
            }))
        }))
    });
    let thumbnail = embed.thumbnail.as_ref().map(|m| {
        let url = crate::discord::sized_image(&m.url, EMBED_THUMB, EMBED_THUMB, (m.width, m.height));
        img(url).size(px(EMBED_THUMB)).flex_none().squircle(RADIUS_SM).object_fit(gpui_kit::ObjectFit::Cover)
    });
    let text = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(6.))
        .children(provider)
        .children(author)
        .children(title)
        .children(description)
        .children(fields);
    let has_text = embed.provider.is_some() || embed.author.is_some() || embed.title.is_some() || embed.description.is_some() || !embed.fields.is_empty();
    let top = (has_text || thumbnail.is_some()).then(|| div().flex().items_start().gap(px(16.)).child(text).children(thumbnail));
    let media = embed_media(embed, row, &key, inner, ctx, cx);
    let stamp = embed.timestamp.as_deref().map(|t| (format_stamp(t), day(t).map(|d| format!("{}, {}", d.format("%A, %B %-d, %Y"), clock(t))).unwrap_or_default()));
    let footer = (embed.footer.is_some() || stamp.is_some()).then(|| {
        let line = embed.footer.clone().into_iter().chain(stamp.as_ref().map(|(short, _)| short.clone())).collect::<Vec<_>>().join(" · ");
        div()
            .id(SharedString::from(format!("{key}-footer")))
            .flex()
            .items_center()
            .gap(px(8.))
            .min_w_0()
            .text_size(px(12.))
            .text_color(faint)
            .children(embed.footer_icon.clone().map(|icon| ui::avatar(icon, 20.)))
            .child(div().min_w_0().child(line))
            .when_some(stamp.map(|(_, full)| full), |d, full| d.tooltip(move |window, cx| Tooltip::new(full.clone()).build(window, cx)))
    });
    let sections = [top.map(IntoElement::into_any_element), media, footer.map(IntoElement::into_any_element)];
    let full = !embed.images.is_empty() || !embed.fields.is_empty();
    // The bar is the card's own left 4px, following its squircle like a CSS inset shadow: a strip as wide as
    // both left corners, cut to the card's corner shape, painted bar-coloured to 4px and clear past it.
    let cut = EMBED_BAR / EMBED_BAR_STRIP;
    let strip = ui::edge(div(), Corners { top_left: px(RADIUS_LG - 1.), bottom_left: px(RADIUS_LG - 1.), top_right: px(0.), bottom_right: px(0.) })
        .absolute()
        .left_0()
        .top_0()
        .bottom_0()
        .w(px(EMBED_BAR_STRIP))
        .bg(linear_gradient(90., linear_color_stop(bar, cut), linear_color_stop(bar.opacity(0.), cut + 0.02)));
    div()
        .relative()
        .mt(px(8.))
        .when(full, |d| d.w(px(width)))
        .when(!full, |d| d.self_start().min_w(px(EMBED_MIN.min(width))))
        .max_w(px(width))
        .py(px(EMBED_PAD.0))
        .pl(px(EMBED_PAD.1 + EMBED_BAR))
        .pr(px(EMBED_PAD.1))
        .flex()
        .flex_col()
        .gap(px(12.))
        .squircle(RADIUS_LG)
        .bg(fill)
        .border_1()
        .border_color(palette::line(cx))
        .text_size(px(14.))
        .line_height(gpui_kit::relative(1.45))
        .child(strip)
        .children(sections.into_iter().flatten())
        .children(row.filter(|m| !embed.inline() && remove_embeds_shown(m, ctx)).map(|m| remove_embeds_button(m, &key, ctx, cx)))
        .into_any_element()
}

// While the row is hovered or keyboard-focused.
fn remove_embeds_shown(message: &Message, ctx: &RowContext) -> bool {
    let active = ctx.hovered.as_deref() == Some(message.id.as_str()) || ctx.focused.as_deref() == Some(message.id.as_str());
    active && message.state == SendState::Sent && can_remove_embeds(message, ctx.me.as_ref().map(|me| me.id.as_str()), ctx.manage_messages)
}

// Occludes so the title or thumbnail under it stays still, and reports hover so the row keeps it.
fn remove_embeds_button(message: &Message, key: &str, ctx: &RowContext, cx: &App) -> AnyElement {
    let (this, m) = (ctx.this.clone(), message.clone());
    ui::icon_button(SharedString::from(format!("{key}-remove")), Glyph::X, "Remove all embeds", cx)
        .small()
        .absolute()
        .top(px(8.))
        .right(px(8.))
        .occlude()
        .on_hover(track_message_hover(message, ctx))
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            let quick = window.modifiers().shift;
            this.update(cx, |this, cx| this.remove_embeds(&m, quick, cx)).ok();
        })
        .into_any_element()
}

const EMBED_WIDTH: f32 = 520.;
const EMBED_MIN: f32 = 240.;
// Vertical, horizontal.
const EMBED_PAD: (f32, f32) = (14., 16.);
const EMBED_BAR: f32 = 4.;
// Wide enough to hold both left corners, so the bar's shape is the card's.
const EMBED_BAR_STRIP: f32 = 2. * RADIUS_LG;
const EMBED_THUMB: f32 = 80.;
const EMBED_MEDIA_MAX_H: f32 = 360.;
// Discord's box for a portrait video still (YouTube Shorts, TikTok): width, height.
const EMBED_PORTRAIT: (f32, f32) = (400., 450.);
// About twelve lines of 14px text: a longer description folds under a fade.
const EMBED_CLIP: f32 = 252.;

// Twelve lines at roughly seventy characters each, the card's measure; gpui can't measure the text first.
fn long_description(text: &str) -> bool {
    text.lines().map(|line| line.chars().count().div_ceil(70).max(1)).sum::<usize>() > 12
}

// Indexes of the fields on each row: consecutive inline fields share one, three across, two when a
// thumbnail takes the right edge; a field that isn't inline takes a row of its own.
fn field_rows(fields: &[(String, String, bool)], thumbnail: bool) -> Vec<Vec<usize>> {
    let per = if thumbnail { 2 } else { 3 };
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut run = Vec::new();
    for (i, (_, _, inline)) in fields.iter().enumerate() {
        if !inline {
            rows.extend((!run.is_empty()).then(|| std::mem::take(&mut run)));
            rows.push(vec![i]);
            continue;
        }
        run.push(i);
        if run.len() == per {
            rows.push(std::mem::take(&mut run));
        }
    }
    rows.extend((!run.is_empty()).then_some(run));
    rows
}

// The embed's pictures at the card's inner width: one at its own shape up to 360 tall, or a gallery of
// up to four. A video's still carries the play button and Open Link; a portrait one (Shorts) takes
// Discord's taller box. A sensitive one stays blurred.
// GAP: Discord swaps the still for the provider's player (a YouTube iframe); gpui has no web view, so
// Play opens the video in the browser too.
fn embed_media(embed: &Embed, row: Option<&Message>, key: &str, inner: f32, ctx: &RowContext, cx: &App) -> Option<AnyElement> {
    let first = embed.images.first()?;
    let tile = |i: usize, (w, h): (f32, f32), corners: Corners<Pixels>| {
        let m = &embed.images[i];
        let (this, url) = (ctx.this.clone(), if embed.video { embed.url.clone().unwrap_or_else(|| m.url.clone()) } else { m.url.clone() });
        let video = embed.video;
        ui::edge(div(), corners)
            .id(SharedString::from(format!("{key}-media-{i}")))
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(palette::selected(cx))
            .cursor_pointer()
            .hover(|s| s.opacity(0.92))
            .child(ui::edge(img(crate::discord::sized_image(&m.url, w, h, (m.width, m.height))), corners).size_full().object_fit(gpui_kit::ObjectFit::Cover))
            .when(video, |d| {
                d.child(div().absolute().inset_0().flex().items_center().justify_center().child(
                    div().size(px(56.)).rounded_full().flex().items_center().justify_center().bg(on_media(0.62)).text_color(gpui_kit::white()).child(Icon::new(Glyph::Play).size(px(26.))),
                ))
                .children(embed.url.clone().map(|url| open_link_button(url, row, &format!("{key}-open-{i}"), ctx)))
            })
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                this.update(cx, |this, cx| if video { this.open_link(&url, window, cx) } else { this.open_viewer(&url, window, cx) }).ok();
            })
    };
    let portrait = embed.video && embed.images.len() == 1 && first.height > first.width;
    let (width, height, body) = if portrait {
        let (w, h) = fit_frame((first.width, first.height), (EMBED_PORTRAIT.0.min(inner), EMBED_PORTRAIT.1));
        (w, h, div().size_full().child(tile(0, (w, h), ui::all(RADIUS_SM))))
    } else if embed.images.len() == 1 {
        let h = (inner * first.height / first.width.max(1.)).min(EMBED_MEDIA_MAX_H);
        (inner, h, div().size_full().child(tile(0, (inner, h), ui::all(RADIUS_SM))))
    } else {
        let k = inner / COLUMN;
        let (height, tiles) = gallery_layout(embed.images.len().min(4));
        let height = height * k;
        let mut grid = div().relative().size_full();
        for (i, t) in tiles.into_iter().enumerate() {
            let t = Tile { x: t.x * k, y: t.y * k, w: t.w * k, h: t.h * k };
            grid = grid.child(div().absolute().left(px(t.x)).top(px(t.y)).w(px(t.w)).h(px(t.h)).child(tile(i, (t.w, t.h), gallery_corners(t, inner, height, RADIUS_SM))));
        }
        (inner, height, grid)
    };
    let veil = format!("{key}-veil");
    let veiled = embed.sensitive && !ctx.revealed.borrow().contains(&veil);
    let (reveal, refresh) = (ctx.revealed.clone(), ctx.this.clone());
    Some(
        div()
            .relative()
            .w(px(width))
            .h(px(height))
            .flex_none()
            .child(body)
            .when(veiled, |d| {
                d.child(
                    div()
                        .id(SharedString::from(veil.clone()))
                        .absolute()
                        .inset_0()
                        .squircle(RADIUS_SM)
                        .bg(on_media(0.35))
                        .backdrop_blur(px(28.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            reveal.borrow_mut().insert(veil.clone());
                            refresh.update(cx, |_, cx| cx.notify()).ok();
                        })
                        .child(
                            div()
                                .h(px(32.))
                                .px(px(12.))
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .squircle(RADIUS_MD)
                                .bg(on_media(0.72))
                                .text_color(gpui_kit::white())
                                .text_size(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(Icon::new(Glyph::EyeSlash).size(px(16.)))
                                .child("Sensitive content · Show"),
                        ),
                )
            })
            .into_any_element(),
    )
}

// Discord's Open Link corner on a video still. In a feed row it reports hover, since occluding takes it from the row.
fn open_link_button(url: String, row: Option<&Message>, id: &str, ctx: &RowContext) -> AnyElement {
    let this = ctx.this.clone();
    div()
        .id(SharedString::from(id.to_owned()))
        .absolute()
        .top(px(8.))
        .right(px(8.))
        .size(px(32.))
        .flex()
        .items_center()
        .justify_center()
        .squircle(RADIUS_SM)
        .bg(on_media(0.55))
        .hover(|s| s.bg(on_media(0.75)))
        .text_color(gpui_kit::white())
        .cursor_pointer()
        .occlude()
        .when_some(row, |d, m| d.on_hover(track_message_hover(m, ctx)))
        .tooltip(|window, cx| Tooltip::new("Open Link").build(window, cx))
        .child(Icon::new(Glyph::Open).size(px(18.)))
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            this.update(cx, |this, cx| this.open_link(&url, window, cx)).ok();
        })
        .into_any_element()
}

// A closed poll's result: the winner, its share, and the way back to the poll.
fn poll_result(poll: &crate::discord::PollResult, poll_message: Option<&str>, key: &str, ctx: &RowContext, cx: &App) -> AnyElement {
    let share = (poll.winner_votes * 100).checked_div(poll.total_votes).unwrap_or(0);
    let votes = if poll.winner_votes == 1 { "vote" } else { "votes" };
    let (emoji, headline, line) = match &poll.winner {
        Some((text, emoji)) => (emoji.clone(), text.clone(), format!("Won with {} {votes} · {share}%", poll.winner_votes)),
        None => (None, "No winner".to_owned(), (if poll.total_votes == 0 { "Nobody voted" } else { "Tied at the top" }).to_owned()),
    };
    let mark = match emoji {
        Some(e) => match e.url() {
            Some(url) => img(url).size(px(28.)).into_any_element(),
            None => div().text_size(px(28.)).line_height(px(28.)).child(e.name).into_any_element(),
        },
        None => div().text_size(px(28.)).line_height(px(28.)).child("📊").into_any_element(),
    };
    let (this, target) = (ctx.this.clone(), poll_message.map(str::to_owned));
    div()
        .mt(px(8.))
        .w_full()
        .max_w(px(EMBED_WIDTH.min(ctx.column)))
        .py(px(12.))
        .pl(px(14.))
        .pr(px(12.))
        .flex()
        .items_center()
        .gap(px(12.))
        .squircle(RADIUS_LG)
        .bg(palette::surface(Level::Sidebar, cx))
        .border_1()
        .border_color(palette::line(cx))
        .child(mark)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().truncate().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(headline))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .text_size(px(12.))
                        .text_color(palette::muted(cx))
                        .when(poll.winner.is_some(), |d| d.child(Icon::new(Glyph::CheckCircle).size(px(14.)).text_color(palette::positive_ink(cx))))
                        .child(line),
                ),
        )
        .when_some(target, |d, id| {
            d.child(ui::button(SharedString::from(format!("{key}-view-poll")), ui::Tone::Secondary, cx).small().child("View poll").on_click(move |_, _, cx| {
                cx.stop_propagation();
                this.update(cx, |this, cx| this.jump_to(id.clone(), cx)).ok();
            }))
        })
        .into_any_element()
}

// A live poll (design/messages.html `.poll`): pick and Vote, or its results once you've voted, peeked or it closed.
fn poll_card(message: &Message, poll: &Poll, ctx: &RowContext, cx: &App) -> AnyElement {
    let now = chrono::Utc::now();
    let (closed, voted, pending) = (poll.closed(now), poll.voted(), message.state != SendState::Sent);
    let results = closed || voted || ctx.poll_peek.contains(&message.id);
    let picks = ctx.poll_picks.get(&message.id).cloned().unwrap_or_default();
    let options: Vec<AnyElement> = match results {
        true => poll.answers.iter().zip(poll.shares()).map(|(answer, share)| poll_share(answer, share, cx)).collect(),
        false => poll.answers.iter().map(|answer| poll_choice(message, poll, answer, picks.contains(&answer.id), pending, ctx, cx)).collect(),
    };
    let key = |part: &str| SharedString::from(format!("poll-{}-{part}", message.id));
    let (channel_id, message_id, this) = (message.channel_id.clone(), message.id.clone(), ctx.this.clone());
    let act = move |picks: Vec<u32>| {
        let (channel_id, message_id, this) = (channel_id.clone(), message_id.clone(), this.clone());
        move |_: &gpui_kit::ClickEvent, _: &mut Window, cx: &mut App| {
            cx.stop_propagation();
            this.update(cx, |this, cx| this.vote(channel_id.clone(), message_id.clone(), picks.clone(), cx)).ok();
        }
    };
    let (peek_id, peek_this) = (message.id.clone(), ctx.this.clone());
    let peek = move |_: &gpui_kit::ClickEvent, _: &mut Window, cx: &mut App| {
        cx.stop_propagation();
        peek_this.update(cx, |this, cx| this.toggle_poll_results(peek_id.clone(), cx)).ok();
    };
    let actions = div().ml_auto().flex().gap(px(8.)).when(!pending && !closed, |d| match (voted, results) {
        (true, _) => d.child(ui::button(key("remove"), ui::Tone::Ghost, cx).small().child("Remove vote").on_click(act(Vec::new()))),
        (false, true) => d.child(ui::button(key("back"), ui::Tone::Ghost, cx).small().child("Back to vote").on_click(peek)),
        (false, false) => d
            .child(ui::button(key("peek"), ui::Tone::Ghost, cx).small().child("Show results").on_click(peek))
            .child(ui::button(key("vote"), ui::Tone::Primary, cx).small().disabled(picks.is_empty()).child("Vote").on_click(act(picks.clone()))),
    });
    div()
        .mt(px(8.))
        .w_full()
        .max_w(px(COLUMN.min(ctx.column)))
        .p(px(14.))
        .flex()
        .flex_col()
        .gap(px(12.))
        .squircle(RADIUS_LG)
        .bg(palette::surface(Level::Sidebar, cx))
        .border_1()
        .border_color(palette::line(cx))
        .when(pending, |d| d.opacity(0.6))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(div().text_size(px(16.)).line_height(px(20.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(poll.question.clone()))
                .child(div().text_size(px(12.)).text_color(palette::faint(cx)).child(poll.prompt(now))),
        )
        .child(div().flex().flex_col().gap(px(6.)).children(options))
        .child(div().flex().flex_wrap().items_center().gap(px(8.)).text_size(px(12.)).text_color(palette::faint(cx)).child(poll.foot(now)).child(actions))
        .into_any_element()
}

// An answer to pick before voting: its radio (a check where several may be picked), emoji and text.
fn poll_choice(message: &Message, poll: &Poll, answer: &PollAnswer, on: bool, pending: bool, ctx: &RowContext, cx: &App) -> AnyElement {
    let mark = if poll.multiselect { ui::check(if on { ui::Check::On } else { ui::Check::Off }, cx) } else { ui::radio(on, cx) };
    let lead = div().flex_none().flex().items_center().gap(px(10.)).child(mark).children(answer.emoji.as_ref().map(|e| crate::menu::emoji_face(e, 20.)));
    let (this, message_id, answer_id, multiselect) = (ctx.this.clone(), message.id.clone(), answer.id, poll.multiselect);
    ui::pick_row(SharedString::from(format!("poll-{}-answer-{answer_id}", message.id)), lead, answer.text.clone(), None, cx)
        .toggled(on, cx)
        .disabled(pending)
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            this.update(cx, |this, cx| this.pick_poll_answer(message_id.clone(), answer_id, multiselect, cx)).ok();
        })
        .into_any_element()
}

// An answer's result: its share as a bar behind it, the percentage at the end, and a check on yours.
fn poll_share(answer: &PollAnswer, share: u32, cx: &App) -> AnyElement {
    let strong = palette::strong(cx);
    let bar = if answer.me { palette::accent_soft(cx) } else { strong.opacity(0.10) };
    div()
        .relative()
        .min_h(px(44.))
        .px(px(12.))
        .py(px(8.))
        .flex()
        .items_center()
        .gap(px(10.))
        .squircle(RADIUS_MD)
        .bg(palette::hover(cx))
        .text_size(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(if answer.me { strong } else { palette::text(cx) })
        .when(share > 0, |d| d.child(div().absolute().left_0().top_0().bottom_0().w(gpui_kit::relative(share as f32 / 100.)).squircle(RADIUS_MD).bg(bar)))
        .children(answer.emoji.as_ref().map(|e| div().flex_none().child(crate::menu::emoji_face(e, 20.))))
        .child(div().flex_1().min_w_0().child(answer.text.clone()))
        .when(answer.me, |d| d.child(Icon::new(Glyph::CheckCircle).size(px(18.)).flex_none().text_color(palette::accent_ink(cx))))
        .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).text_color(strong).child(format!("{share}%")))
        .into_any_element()
}

// Text files read in place (the fetch starts here, when the row first renders); any other file, or one
// that turned out not to be text, is a card with its icon, name, size and one action.
fn files(message: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> Vec<AnyElement> {
    message
        .files
        .iter()
        .map(|file| {
            let readable = message.state == SendState::Sent && crate::text_file::is_text_name(&file.name);
            let text_files = ctx.text_files.borrow();
            let state = text_files.files.get(&file.url);
            if !readable || matches!(state, Some(crate::text_file::TextFile::Unreadable)) {
                return file_card(message, file, cx);
            }
            if state.is_none() {
                let (this, url) = (ctx.this.clone(), file.url.clone());
                cx.defer(move |cx| {
                    this.update(cx, |this, cx| this.load_text_file(url, cx)).ok();
                });
            }
            let copied = ctx.copied.as_deref() == Some(file.url.as_str());
            crate::text_file::inline(format!("text-{}-{}", message.id, file.name), file, state, text_files.expanded.contains(&file.url), copied, ctx.this.clone(), cx).into_any_element()
        })
        .collect()
}

// The files of a message still uploading: each with its picture, its bytes out of its size as a bar so
// a big file visibly moves, and Cancel, which drops the whole message as Discord's does.
fn upload_cards(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> Vec<AnyElement> {
    if message.state == SendState::Sent {
        return Vec::new();
    }
    let Some(job) = message.nonce.as_ref().and_then(|n| ctx.uploads.get(n)) else { return Vec::new() };
    let (strong, faint) = (palette::strong(cx), palette::faint(cx));
    job.files
        .iter()
        .enumerate()
        .map(|(ix, file)| {
            let sent = file.sent.load(std::sync::atomic::Ordering::Relaxed).min(file.size);
            let fraction = if file.size == 0 { 1. } else { sent as f32 / file.size as f32 };
            let kind = crate::attachments::FileKind::of(&file.path);
            let picture = file_picture(ctx.previews.get(&file.path).cloned().flatten(), kind, 40., false, cx);
            let status = match job.error.is_some() {
                true => format_size(file.size),
                false => format!("{} of {}", format_size(sent), format_size(file.size)),
            };
            div()
                .mt(px(8.))
                .w_full()
                .max_w(px(COLUMN))
                .p(px(12.))
                .flex()
                .items_center()
                .gap(px(12.))
                .squircle(RADIUS_LG)
                .bg(palette::surface(Level::Sidebar, cx))
                .border_1()
                .border_color(palette::line(cx))
                .child(picture)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(div().flex().items_baseline().gap(px(8.)).child(div().flex_1().min_w_0().truncate().text_size(px(14.)).font_weight(FontWeight::MEDIUM).text_color(strong).child(file.name.clone())).child(div().flex_none().text_size(px(12.)).text_color(faint).child(status)))
                        .when(job.error.is_none(), |d| d.child(ui::progress(fraction, cx))),
                )
                .when(job.error.is_none(), |d| {
                    let (this, cancel) = (ctx.this.clone(), message.clone());
                    d.child(ui::icon_button(SharedString::from(format!("cancel-upload-{}-{ix}", message.id)), Glyph::X, "Cancel upload", cx).small().on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        this.update(cx, |this, cx| this.delete(cancel.clone(), cx)).ok();
                    }))
                })
                .into_any_element()
        })
        .collect()
}

// A file's tile picture: the OS's thumbnail or type icon once drawn, a quiet well until then. A
// spoiler is veiled, as Discord blurs it.
fn file_picture(picture: Option<std::sync::Arc<gpui_kit::RenderImage>>, kind: crate::attachments::FileKind, side: f32, spoiler: bool, cx: &App) -> AnyElement {
    let well = div().relative().size(px(side)).flex_none().squircle(RADIUS_XS).overflow_hidden();
    let well = match picture {
        // Thumbnails fill the tile; icons keep their own transparent margins.
        Some(image) => well.child(ui::media(image, ui::all(RADIUS_XS)).size_full().object_fit(gpui_kit::ObjectFit::Cover)),
        None => well.bg(palette::surface(Level::Raised, cx)).text_color(palette::muted(cx)).flex().items_center().justify_center().child(Icon::new(match kind {
            crate::attachments::FileKind::Video => Glyph::Video,
            crate::attachments::FileKind::Image => Glyph::Image,
            crate::attachments::FileKind::Other => Glyph::File,
        }).size(px(side * 0.5))),
    };
    well.when(spoiler, |w| {
        w.child(div().absolute().inset_0().bg(palette::surface(Level::Sidebar, cx).opacity(0.85)).backdrop_blur(px(8.)).flex().items_center().justify_center().text_color(palette::strong(cx)).child(Icon::new(Glyph::EyeSlash).size(px(side * 0.4))))
    })
    .into_any_element()
}

fn file_card(message: &Message, file: &crate::discord::File, cx: &App) -> AnyElement {
    let (strong, faint, muted) = (palette::strong(cx), palette::faint(cx), palette::muted(cx));
    let (open, download) = (file.url.clone(), file.url.clone());
    div()
        .mt(px(8.))
        .w_full()
        .max_w(px(COLUMN))
        .p(px(12.))
        .flex()
        .items_center()
        .gap(px(12.))
        .squircle(RADIUS_LG)
        .bg(palette::surface(Level::Sidebar, cx))
        .border_1()
        .border_color(palette::line(cx))
        .when(message.state != SendState::Sent, |d| d.opacity(0.6))
        .child(div().w(px(40.)).flex_none().flex().justify_center().text_color(muted).child(Icon::new(Glyph::File).size(px(36.))))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .id(SharedString::from(format!("file-{}-{}", message.id, file.name)))
                        .truncate()
                        .cursor_pointer()
                        .text_size(px(14.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(strong)
                        .hover(|s| s.underline())
                        .child(file.name.clone())
                        .on_click(move |_, _, cx| { cx.stop_propagation(); cx.open_url(&open) }),
                )
                .child(div().text_size(px(12.)).text_color(faint).child(format_size(file.size))),
        )
        .child(ui::icon_button(SharedString::from(format!("download-{}-{}", message.id, file.name)), Glyph::Download, "Download", cx).on_click(move |_, _, cx| { cx.stop_propagation(); cx.open_url(&download) }))
        .into_any_element()
}

// Sits in the corner of every code block; it turns to a check, in the online ink, for a moment after.
// GAP: the design gives code blocks a header line (language, then Copy with its label); TextView owns
// that layout and only offers this corner slot, so the button stays icon-only.
fn copy_button(code: String, copied: bool, this: WeakEntity<Waffle>, cx: &App) -> AnyElement {
    let (hover, strong) = (palette::hover(cx), palette::strong(cx));
    div()
        .id(SharedString::from(format!("copy-{}", code.len())))
        .h(px(28.))
        .w(px(28.))
        .justify_center()
        .flex()
        .items_center()
        .squircle(RADIUS_XS)
        .cursor_pointer()
        .text_color(if copied { palette::positive_ink(cx) } else { palette::muted(cx) })
        .when(copied, |b| b.hover(move |s| s.bg(hover)))
        .when(!copied, |b| b.hover(move |s| s.bg(hover).text_color(strong)))
        .child(Icon::new(if copied { Glyph::Checkmark } else { Glyph::Copy }).size(px(16.)))
        .tooltip(move |window, cx| Tooltip::new(if copied { "Copied" } else { "Copy" }).build(window, cx))
        .on_click(move |_, _, cx| { cx.stop_propagation();
            this.update(cx, |this, cx| this.copy_code(code.clone(), code.clone(), cx)).ok();
        })
        .into_any_element()
}

// One click adds or removes yours; yours carry the accent, the rest stay neutral. Add stays last.
fn reactions(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> Option<AnyElement> {
    if message.reactions.is_empty() {
        return None;
    }
    let pills = message.reactions.iter().map(|r| {
        let (this, msg, emoji) = (ctx.this.clone(), message.clone(), r.emoji.clone());
        let (hover_this, hover_msg, hover_emoji) = (ctx.this.clone(), message.clone(), r.emoji.clone());
        let card = crate::reactions::ReactorsCard::new(ctx.this.clone(), message, r);
        reaction_pill(SharedString::from(format!("react-{}-{}", message.id, r.emoji.name)), r.me, cx)
            .px(px(9.))
            .child(emoji_face(&r.emoji, 18., 16.))
            .child(r.count.to_string())
            .on_hover(move |hovered, _, cx| {
                if *hovered {
                    hover_this.update(cx, |this, cx| this.load_reactors(&hover_msg.channel_id, &hover_msg.id, &hover_emoji, cx)).ok();
                }
            })
            // Hoverable, so the pointer can cross into the card and click its "N others".
            .hoverable_tooltip(move |_, cx| cx.new(|_| card.clone()).into())
            .on_click(move |_, _, cx| { cx.stop_propagation();
                this.update(cx, |this, cx| this.toggle_reaction(&msg, emoji.clone(), cx)).ok();
            })
    });
    let (this, msg) = (ctx.this.clone(), message.clone());
    let add = reaction_pill(SharedString::from(format!("react-add-{}", message.id)), false, cx)
        .px(px(7.))
        .child(Icon::new(Glyph::EmojiAdd).size(px(18.)))
        .tooltip(|window, cx| Tooltip::new("Add Reaction").build(window, cx))
        .on_click(move |_, window, cx| { cx.stop_propagation();
            this.update(cx, |this, cx| this.open_emoji_picker(EmojiTarget::Reaction(Box::new(msg.clone())), window, cx)).ok();
        });
    Some(div().mt(px(6.)).flex().flex_wrap().gap(px(6.)).children(pills).child(add).into_any_element())
}

// A custom emoji's picture, or a unicode one drawn as text.
fn emoji_face(emoji: &Emoji, image: f32, text: f32) -> AnyElement {
    match emoji.url() {
        Some(url) => img(url).size(px(image)).into_any_element(),
        None => div().text_size(px(text)).line_height(px(text)).child(emoji.name.clone()).into_any_element(),
    }
}

fn reaction_pill(id: SharedString, mine: bool, cx: &App) -> ui::Pressable {
    let edge = palette::line_strong(cx);
    ui::pressable(div().id(id), 0.92)
        .h(px(28.))
        .flex()
        .items_center()
        .gap(px(6.))
        .squircle(RADIUS_SM)
        .border_1()
        .cursor_pointer()
        .text_size(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .when(mine, |p| p.bg(palette::accent_soft(cx)).border_color(palette::accent().opacity(0.55)).text_color(palette::accent_ink(cx)))
        .when(!mine, |p| p.bg(palette::selected(cx)).border_color(gpui_kit::transparent_black()).text_color(palette::muted(cx)).hover(move |s| s.border_color(edge)))
}

// The hover toolbar: three quick reactions, then react, reply, thread and more. With Shift held, edit
// and delete take the quick reactions' place behind a Shift keycap; the rest stay put.
fn actions(message: &Message, mine: bool, ctx: &Rc<RowContext>, cx: &App) -> AnyElement {
    use crate::message_menu::MessageAction as A;
    let (hover, line) = (palette::hover(cx), palette::line(cx));
    let sep = || div().w(px(1.)).h(px(20.)).mx(px(2.)).bg(line);
    let sent = message.state == SendState::Sent;
    let facts = crate::message_menu::Facts { mine, saved: ctx.saved_ids.contains(&message.id), ..ctx.facts };
    let mut bar = div()
        .id(SharedString::from(format!("actions-{}", message.id)))
        // Rides 18px into the row above: block the pointer there and keep this row hovered, or the pointer hands
        // the hover (and the bar) to that row. The wheel still reaches the feed under it.
        .block_mouse_except_scroll()
        .on_hover(track_message_hover(message, ctx))
        .absolute()
        .top(px(-18.))
        // A flush mention row is 8px wider each side; the bar stays where it sits on other rows.
        .right(px(if ctx.mentions_me.contains(&message.id) { 20. } else { 12. }))
        .p(px(2.))
        .flex()
        .items_center()
        .gap(px(2.))
        .squircle(RADIUS_MD)
        .bg(palette::surface(Level::Overlay, cx))
        .shadow(ui::shadow_pop(cx))
        .child(crate::gif::above_gifs());
    // One button per action, doing what its menu row does; held Shift skips the confirms, as Discord's.
    let action = |action: A, cx: &App| {
        let (glyph, tip) = match action {
            A::Edit => (Glyph::Pen, "Edit"),
            A::Reply => (Glyph::Reply, "Reply"),
            A::Forward => (Glyph::Forward, "Forward"),
            A::Thread => (Glyph::ThreadNew, "Create Thread"),
            A::React => (Glyph::Emoji, "Add Reaction"),
            A::CopyText => (Glyph::Copy, "Copy Text"),
            A::Pin if message.pinned => (Glyph::PinSlash, "Unpin Message"),
            A::Pin => (Glyph::Pin, "Pin Message"),
            A::MarkUnread => (Glyph::Envelope, "Mark Unread"),
            A::Save if facts.saved => (Glyph::Bookmark, "Remove from Saved"),
            A::Save => (Glyph::Bookmark, "Save Message"),
            A::Delete => (Glyph::Trash, "Delete"),
            A::CopyId => (Glyph::Id, "Copy Message ID"),
        };
        let (this, msg) = (ctx.this.clone(), message.clone());
        let button = ui::icon_button(SharedString::from(format!("{action:?}-{}", message.id)), glyph, tip, cx).on_click(move |_, window, cx| {
            cx.stop_propagation();
            let quick = window.modifiers().shift;
            this.update(cx, |this, cx| this.run_message_action(action, &msg, quick, window, cx)).ok();
        });
        match action {
            A::Delete => {
                let (danger, danger_ink) = (palette::danger(), palette::danger_ink(cx));
                button.text_color(danger_ink).hover(move |s| s.bg(danger).text_color(gpui_kit::white()))
            }
            _ => button,
        }
    };
    let offer = |a: A| a.allowed(message, &facts).then_some(a);
    if !sent {
        // A failed send: Delete (and the menu's Resend).
        bar = bar.children(offer(A::Delete).map(|a| action(a, cx)));
    } else if ctx.shift {
        // Shift: the menu's direct actions laid out on the bar, in Discord's order.
        let (this, msg) = (ctx.this.clone(), message.clone());
        let link = ui::icon_button(SharedString::from(format!("link-{}", message.id)), Glyph::Link, "Copy Message Link", cx).on_click(move |_, _, cx| {
            cx.stop_propagation();
            this.update(cx, |this, cx| this.copy_text(this.message_link(&msg), cx)).ok();
        });
        bar = bar
            .children(offer(A::CopyId).map(|a| action(a, cx)))
            .child(link)
            .children([A::MarkUnread, A::Save, A::Pin, A::Thread, A::Reply, A::React, A::Edit, A::Forward, A::Delete].into_iter().filter_map(offer).map(|a| action(a, cx)));
    } else {
        for quick in ctx.quick_reactions.iter().cloned() {
            let (this, msg, emoji) = (ctx.this.clone(), message.clone(), quick.clone());
            let face = emoji_face(&quick, 20., 18.);
            bar = bar.child(
                ui::pressable(div().id(SharedString::from(format!("quick-{}-{}", message.id, quick.name))), 0.92)
                    .size(px(32.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .squircle(RADIUS_SM)
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .child(face)
                    .tooltip(|window, cx| Tooltip::new("Click to react").build(window, cx))
                    .on_click(move |_, _, cx| { cx.stop_propagation();
                        this.update(cx, |this, cx| this.toggle_reaction(&msg, emoji.clone(), cx)).ok();
                    }),
            );
        }
        bar = bar.child(sep()).children([A::React, A::Edit, A::Reply, A::Forward, A::Thread].into_iter().filter_map(offer).map(|a| action(a, cx)));
    }
    if !ctx.shift || !sent {
        let (this, msg) = (ctx.this.clone(), message.clone());
        bar = bar.child(ui::icon_button(SharedString::from(format!("more-{}", message.id)), Glyph::MoreVertical, "More", cx).on_click(move |event, _, cx| { cx.stop_propagation();
            let at = event.position();
            this.update(cx, |this, cx| this.open_message_menu(msg.clone(), at, cx)).ok();
        }));
    }
    // GAP: not `deferred`: inside the chat list a deferred bar never painted. It overlaps only the message
    // above, which the list paints first, so it draws on top without it.
    bar.into_any_element()
}

/// Discord's wire syntax made readable: mentions become links the transcript styles and routes,
/// custom emoji become inline images at Discord's 1.375em (animated ones play).
fn readable(content: &str, ctx: &RowContext) -> String {
    readable_with(content, ctx, true)
}

// `inline_emoji` false keeps custom emoji as `:name:`, for one-line plain text such as reply quotes.
fn readable_with(content: &str, ctx: &RowContext, inline_emoji: bool) -> String {
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let Some(end) = tail.find('>') else {
            out.push_str(tail);
            return out;
        };
        let token = &tail[1..end];
        let replacement = if let Some(id) = token.strip_prefix("@&") {
            ctx.role_names.get(id).map(|n| format!("[@{n}](waffle://role/{id})"))
        } else if let Some(id) = token.strip_prefix("@!").or_else(|| token.strip_prefix('@')) {
            Some(format!("[@{}](waffle://user/{id})", ctx.names.get(id).map_or("unknown-user", String::as_str)))
        } else if let Some(id) = token.strip_prefix('#') {
            ctx.channel_names.get(id).map(|n| format!("[#{n}](waffle://channel/{id})"))
        } else if let Some(stamp) = token.strip_prefix("t:") {
            discord_timestamp(stamp)
        } else if let Some(name) = token.strip_prefix("a:").or_else(|| token.strip_prefix(':')) {
            match (inline_emoji, emoji_image(token)) {
                (true, Some(html)) => Some(html),
                _ => name.split(':').next().map(|n| format!(":{n}:")),
            }
        } else {
            None
        };
        match replacement {
            Some(r) => out.push_str(&r),
            None => out.push_str(&tail[..=end]),
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    let out = crate::link_preview::channel_chips(&out, &ctx.channel_names);
    // ponytail: plain word match, like the tokens above it also matches inside code spans.
    out.replace("@everyone", "[@everyone](waffle://everyone)").replace("@here", "[@here](waffle://here)")
}

// One line of plain text for the reply quote: markdown markers and link targets dropped.
fn plain(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    let (mut skipping_target, mut after_bracket) = (false, false);
    for c in markdown.chars() {
        match c {
            '(' if after_bracket => skipping_target = true,
            ')' if skipping_target => skipping_target = false,
            _ if skipping_target => {}
            '*' | '_' | '`' | '~' | '[' | ']' => {}
            '\n' => out.push(' '),
            _ => out.push(c),
        }
        after_bracket = c == ']';
    }
    out
}

// `<t:1731590400:R>` and friends, rendered in the reader's time zone like Discord does.
fn discord_timestamp(stamp: &str) -> Option<String> {
    let (secs, style) = stamp.split_once(':').unwrap_or((stamp, "f"));
    let time = DateTime::from_timestamp(secs.parse().ok()?, 0)?.with_timezone(&Local);
    Some(format!("`{}`", match style {
        "t" => time.format("%H:%M").to_string(),
        "T" => time.format("%H:%M:%S").to_string(),
        "d" => time.format("%d/%m/%Y").to_string(),
        "D" => time.format("%-d %B %Y").to_string(),
        "F" => time.format("%A, %-d %B %Y %H:%M").to_string(),
        "R" => relative(Local::now().signed_duration_since(time)),
        _ => time.format("%-d %B %Y %H:%M").to_string(),
    }))
}

fn relative(delta: chrono::TimeDelta) -> String {
    let (secs, future) = (delta.num_seconds().abs(), delta.num_seconds() < 0);
    let (n, unit) = match secs {
        s if s < 60 => (s, "second"),
        s if s < 3600 => (s / 60, "minute"),
        s if s < 86_400 => (s / 3600, "hour"),
        s if s < 2_592_000 => (s / 86_400, "day"),
        s if s < 31_536_000 => (s / 2_592_000, "month"),
        s => (s / 31_536_000, "year"),
    };
    let unit = if n == 1 { unit.to_string() } else { format!("{unit}s") };
    if future { format!("in {n} {unit}") } else { format!("{n} {unit} ago") }
}

// Custom emoji as inline images and nothing else changed: markdown outside a message (a profile bio).
pub(crate) fn inline_emoji(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        let tail = &rest[start..];
        let Some(end) = tail.find('>') else { break };
        out.push_str(&rest[..start]);
        match emoji_image(&tail[1..end]) {
            Some(html) => out.push_str(&html),
            None => out.push_str(&tail[..=end]),
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

// `a:name:id` or `:name:id` as an inline image the markdown view lays out in the text flow. Its
// images carry element ids, so animated emoji play. 22px is Discord's 1.375em at 16px text.
fn emoji_image(token: &str) -> Option<String> {
    let animated = token.starts_with("a:");
    let mut parts = token.trim_start_matches("a:").trim_start_matches(':').split(':');
    let (name, id) = (parts.next()?, parts.next()?);
    if parts.next().is_some() || name.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let url = Emoji { id: Some(id.to_owned()), name: name.to_owned(), animated }.url()?;
    Some(format!(r#"<img src="{url}" alt=":{name}:" width="22" height="22" />"#))
}

fn custom_emoji(content: &str) -> Vec<Emoji> {
    let mut out = Vec::new();
    let mut rest = content;
    while let Some(start) = rest.find('<') {
        let tail = &rest[start..];
        let Some(end) = tail.find('>') else { break };
        let token = &tail[1..end];
        let animated = token.starts_with("a:");
        let parts: Vec<&str> = token.trim_start_matches("a:").trim_start_matches(':').split(':').collect();
        if (token.starts_with(':') || animated) && parts.len() == 2 {
            out.push(Emoji { id: Some(parts[1].to_owned()), name: parts[0].to_owned(), animated });
        }
        rest = &tail[end + 1..];
    }
    out
}

fn strip_emoji(content: &str) -> String {
    let mut out = String::new();
    let mut rest = content;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let Some(end) = tail.find('>') else { break };
        let token = &tail[1..end];
        if !(token.starts_with(':') || token.starts_with("a:")) {
            out.push_str(&tail[..=end]);
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

// Who is typing, names emphasised: "Mira is typing…", "Mira and Theo are typing…", then "Several people".
fn typing_parts(names: &[&str]) -> Vec<(String, bool)> {
    let name = |n: &str| (n.to_owned(), true);
    let word = |w: &str| (w.to_owned(), false);
    match names {
        [] => Vec::new(),
        [one] => vec![name(one), word(" is typing…")],
        [a, b] => vec![name(a), word(" and "), name(b), word(" are typing…")],
        [a, b, c] => vec![name(a), word(", "), name(b), word(" and "), name(c), word(" are typing…")],
        _ => vec![word("Several people are typing…")],
    }
}

// One line of text with some runs in strong semibold, as the design's <b>.
pub(crate) fn emphasised(parts: &[(String, bool)], strong: Hsla) -> StyledText {
    let mut text = String::new();
    let mut runs = Vec::new();
    for (part, bold) in parts {
        let start = text.len();
        text.push_str(part);
        if *bold {
            runs.push((start..text.len(), HighlightStyle { color: Some(strong), font_weight: Some(FontWeight::MEDIUM), ..Default::default() }));
        }
    }
    StyledText::new(text).with_highlights(runs)
}

// A keycap and what it does, for the edit bar.
fn key_hint(key: &'static str, label: &'static str, cx: &App) -> Div {
    div().flex_none().flex().items_center().gap(px(6.)).text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(palette::muted(cx)).child(ui::kbd(key, cx)).child(label)
}

// Placeholders shaped like the real transcript, sweeping in step.
fn skeleton(cx: &App) -> AnyElement {
    let bar = palette::selected(cx);
    let row = |w: f32| {
        div()
            .flex()
            .gap(px(16.))
            .child(div().size(px(AVATAR)).flex_none().rounded_full().bg(bar))
            .child(div().flex_1().flex().flex_col().gap_2().pt_1().child(div().w(px(120.)).h(px(12.)).rounded_full().bg(bar)).child(div().w(px(w)).h(px(12.)).rounded_full().bg(bar)))
    };
    div()
        .size_full()
        .px(px(24.))
        .pb_4()
        .flex()
        .flex_col()
        .justify_end()
        .gap_6()
        .children([360., 220., 420., 280.].map(row))
        .with_animation("skeleton", Animation::new(Duration::from_millis(1100)).repeat(), |el, t| el.opacity(0.6 + 0.4 * (t * TAU).cos().abs()))
        .into_any_element()
}

// One accent line where unread starts, ending in a New badge.
// Discord's unread divider (design/unread-line.html): a red rule into a NEW tag whose point runs back along
// it. The tag is one shape (`icons::NEW_TAG`), NEW centred on its body, clear of the point.
fn unread_line() -> AnyElement {
    let alert = palette::alert();
    let tag = div()
        .relative()
        .flex_none()
        .w(px(40.))
        .h(px(16.))
        // The rule runs on under the tag to its body, so it meets the sharp tip at full thickness.
        .child(div().absolute().left(px(-1.)).w(px(9.)).top(px(7.5)).h(px(1.)).bg(alert))
        .child(gpui_kit::svg().path(crate::icons::NEW_TAG).absolute().inset_0().size_full().text_color(alert))
        .child(div().absolute().top_0().bottom_0().left(px(8.)).right_0().flex().items_center().justify_center().text_color(gpui_kit::white()).text_size(px(11.)).font_weight(FontWeight::BOLD).child("NEW"));
    div().w_full().px(px(16.)).my(px(8.)).flex().items_center().child(div().flex_1().h(px(1.)).bg(alert)).child(tag).into_any_element()
}

// Between dates, inside the feed's gutters, never edge to edge.
fn day_divider(timestamp: &str, cx: &App) -> AnyElement {
    let line = || div().flex_1().h(px(1.)).bg(palette::line(cx));
    div()
        .w_full()
        .px(px(16.))
        .mt(px(16.))
        .mb(px(8.))
        .flex()
        .items_center()
        .gap(px(10.))
        .whitespace_nowrap()
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(palette::faint(cx))
        .child(line())
        .child(day(timestamp).map(|d| d.format("%A, %B %-d, %Y").to_string()).unwrap_or_default())
        .child(line())
        .into_any_element()
}

fn empty_state(id: &'static str, art: impl Into<ui::Art>, title: &'static str, detail: impl Into<SharedString>, cx: &App) -> AnyElement {
    div().size_full().flex().items_center().justify_center().child(ui::empty_state(id, art, title, detail, cx)).into_any_element()
}

fn continues(previous: &Message, message: &Message) -> bool {
    let (Ok(a), Ok(b)) = (DateTime::parse_from_rfc3339(&previous.timestamp), DateTime::parse_from_rfc3339(&message.timestamp)) else {
        return false;
    };
    previous.author.id == message.author.id && (b - a).num_seconds() < GROUP_GAP_SECS
}

pub(crate) fn local(timestamp: &str) -> Option<DateTime<Local>> {
    Some(DateTime::parse_from_rfc3339(timestamp).ok()?.with_timezone(&Local))
}

pub(crate) fn day(timestamp: &str) -> Option<NaiveDate> {
    Some(local(timestamp)?.date_naive())
}

// Settings › Chat › Use a 24-hour clock. A global, not a parameter: the formatters sit deep under
// rendering code that has no prefs to hand.
static CLOCK_24: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn set_clock_24(on: bool) {
    CLOCK_24.store(on, std::sync::atomic::Ordering::Relaxed);
}

fn time_format(gutter: bool) -> &'static str {
    match (CLOCK_24.load(std::sync::atomic::Ordering::Relaxed), gutter) {
        (true, _) => "%H:%M",
        (false, false) => "%-I:%M %p",
        (false, true) => "%-I:%M",
    }
}

// "Today at 9:41 PM", "Yesterday at 11:58 PM", the weekday within the week, then the date.
pub(crate) fn format_stamp(timestamp: &str) -> String {
    let Some(time) = local(timestamp) else { return String::new() };
    let clock = time.format(time_format(false));
    match (Local::now().date_naive() - time.date_naive()).num_days() {
        0 => format!("Today at {clock}"),
        1 => format!("Yesterday at {clock}"),
        2..=6 => format!("{} at {clock}", time.format("%A")),
        _ => format!("{} at {clock}", time.format("%B %-d, %Y")),
    }
}

// "9:41 PM", or "21:41".
// Discord's bar text: "21 new messages since 4:08 PM", "50+ new messages since…" when not all are loaded,
// and " on November 3, 2026" after the time when the first unread is from another day.
fn unread_label(count: usize, estimated: bool, since: &str) -> String {
    let messages = match (estimated, count) {
        (true, n) => format!("{n}+ new messages"),
        (false, 1) => "1 new message".to_owned(),
        (false, n) => format!("{n} new messages"),
    };
    let today = chrono::Local::now().date_naive();
    match day(since).filter(|d| *d != today) {
        Some(d) => format!("{messages} since {} on {}", clock(since), d.format("%B %-d, %Y")),
        None => format!("{messages} since {}", clock(since)),
    }
}

pub(crate) fn clock(timestamp: &str) -> String {
    local(timestamp).map(|t| t.format(time_format(false)).to_string()).unwrap_or_default()
}

// The continuation gutter's "9:41": the half of the day is the header's to say.
fn gutter_clock(timestamp: &str) -> String {
    local(timestamp).map(|t| t.format(time_format(true)).to_string()).unwrap_or_default()
}

pub(crate) fn format_size(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.0} KB", b as f64 / (1 << 10) as f64),
        b => format!("{b} bytes"),
    }
}

// A breadcrumb back up the hierarchy: muted until hovered.
fn crumb(id: &'static str, cx: &App) -> Stateful<Div> {
    let (hover, strong) = (palette::hover(cx), palette::strong(cx));
    div()
        .id(id)
        .occlude()
        .h(px(32.))
        .px(px(6.))
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(6.))
        .squircle(RADIUS_SM)
        .cursor_pointer()
        .whitespace_nowrap()
        .font_weight(FontWeight::NORMAL)
        .text_color(palette::muted(cx))
        .hover(move |s| s.bg(hover).text_color(strong))
}

#[cfg(test)]
mod tests {
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
        let (dark, light): (gpui_kit::Hsla, gpui_kit::Hsla) = (gpui_kit::rgb(0x1E1F22).into(), gpui_kit::rgb(0xFFFFFF).into());
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
        let gif = crate::discord::Media { url: "https://t/x.mp4".into(), width: 1., height: 1., kind: crate::discord::MediaKind::Image };
        let msg = |content: &str, media: bool| Message { content: content.into(), media: if media { vec![gif.clone()] } else { vec![] }, ..Default::default() };
        assert!(only_media_link(&msg("https://klipy.com/gifs/x", true)));
        assert!(!only_media_link(&msg("look https://klipy.com/gifs/x", true)));
        assert!(!only_media_link(&msg("https://example.com", false)));
    }

    #[test]
    fn jumbo_emoji_detection() {
        assert_eq!(custom_emoji("<:blob:1> <a:dance:2>").len(), 2);
        let dance = emoji_image("a:dance:2").unwrap();
        assert!(dance.contains("/emojis/2.gif") && dance.contains(r#"alt=":dance:""#) && dance.contains(r#"width="22""#));
        assert!(emoji_image(":blob:1").unwrap().contains("/emojis/1.webp"));
        assert_eq!(emoji_image(":bad:x1"), None);
        assert!(strip_emoji("<:blob:1> <a:dance:2>").trim().is_empty());
        assert_eq!(strip_emoji("hi <:blob:1> <@5>"), "hi  <@5>");
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
        assert_eq!(split_subtext("hi\n-# small"), ("hi".into(), vec!["small".into()]));
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
        for count in 2..=7 {
            let (height, tiles) = gallery_layout(count);
            assert_eq!(tiles.len(), count.min(5), "{count} items");
            for t in &tiles {
                assert!(t.x >= 0. && t.y >= 0. && t.x + t.w <= COLUMN + 0.01 && t.y + t.h <= height + 0.01, "{count}: {t:?} spills");
            }
            // Tiles and their gaps cover the column: nothing overlaps, nothing is left empty but the gaps.
            let area: f32 = tiles.iter().map(|t| t.w * t.h).sum();
            assert!(area < COLUMN * height && area > (COLUMN - 2. * GALLERY_GAP) * (height - GALLERY_GAP) * 0.95, "{count}: area {area}");
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
    fn stamps_read_like_the_design() {
        let now = Local::now();
        assert!(format_stamp(&now.to_rfc3339()).starts_with("Today at "));
        assert!(format_stamp(&(now - chrono::TimeDelta::days(1)).to_rfc3339()).starts_with("Yesterday at "));
        let old = format_stamp(&(now - chrono::TimeDelta::days(30)).to_rfc3339());
        assert!(old.contains(", 20") && old.contains(" at "), "{old}");
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
