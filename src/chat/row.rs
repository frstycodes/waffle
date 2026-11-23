use super::*;

// An avatar column and a content column; a continuation shows its time in the gutter on hover, and the
// toolbar floats at the row's top right. Compact display puts each message on one line after its time.
pub(super) fn message_row(message: &Message, starts_group: bool, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    // A Components V2 message lays out its own media and files.
    let v2 = message.flags & crate::app_commands::COMPONENTS_V2 != 0;
    let ephemeral = components::is_ephemeral(message);
    let media = media_grid(message, ctx, cx).filter(|_| !v2);
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
    let text = content(message, ctx, cx);
    let lead = match ctx.compact {
        true => compact_line(message, starts_group, text, ctx, cx),
        false => text,
    };
    let body = ui::col()
        .relative()
        .flex_1()
        .text_size(px(ctx.font_size))
        .text_color(ink)
        .children(message.reply.as_ref().filter(|_| ctx.compact).map(|reply| reply_preview(reply, message.reply_to.clone(), ctx, cx)))
        .children(lead)
        .children(components::interaction_status(message, ctx, cx))
        .children(crate::announcements::publish_bump(message, mine, &ctx.announce, ctx.this.clone(), cx))
        .children(message.forward.as_ref().map(|original| forward_card(original, ctx, cx)))
        .children(media.map(|m| div().when(pending, |d| d.opacity(0.6)).child(m)))
        .children(message.stickers.iter().map(|s| div().when(pending, |d| d.opacity(0.6)).child(crate::stickers::in_chat(s, ctx.media.stickers.plays(ctx.hovers(message)), cx))))
        .children(message.poll.as_ref().map(|p| poll_card(message, p, ctx, cx)))
        .children(message.activity.as_ref().map(|a| crate::activities::activity_message_card(message, a, &ctx.activity_cards, ctx.this.clone(), cx)))
        .children(shown_embeds(message, ctx).iter().enumerate().map(|(i, e)| embed_card(e, Some(message), message.reply_to.as_deref(), format!("embed-{}-{i}", message.id), ctx, cx)))
        .children(link_cards(message, ctx, cx))
        .children(message.thread.as_ref().map(|t| thread_card(t, ctx, cx)))
        .when(!v2, |b| b.children(files(message, ctx, cx)))
        .children(upload_cards(message, ctx, cx))
        .children(components::message_components(message, ctx, cx))
        .children(reactions(message, ctx, cx).filter(|_| ctx.reactions))
        // A command's stand-in says why in its status line.
        .when(message.state == SendState::Failed && !components::is_failed_stand_in(message), |b| b.child(send_failure(message, ctx, cx)))
        .when(ephemeral, |b| b.child(components::only_you(message, ctx, cx)))
        // Where a jump landed: a squiggle under the message, gone with the flash.
        .when(landed, |b| b.child(div().absolute().left_0().right_0().bottom_neg_1p5().h_2().child(ui::squiggle(SharedString::from(format!("squiggle-{}", message.id)), message.id.parse().unwrap_or(0), palette::accent_ink(cx), JUMP_FLASH))));
    let row = ui::col()
        .id(group.clone())
        .group(group.clone())
        .relative()
        .w_full()
        // A mention runs to the feed's edges, so its rim meets the sidebar; its content keeps its place.
        .px(px(if mention { 24. } else { 16. }))
        .py_0p5()
        .squircle(RADIUS_SM)
        .line_height(gpui_kit::relative(1.45))
        .when(ephemeral, |r| r.bg(components::ephemeral_tint(cx)))
        // Mentioning you: the tint and a 2px accent bar down the left edge, square like Discord's.
        // `hovered` too: the toolbar occludes the row, so the row's own `.hover` drops while the pointer is on it.
        .when(mention, |r| r.rounded_none().bg(rest.unwrap_or(if hovered { mention_hover } else { soft })).hover(move |s| s.bg(rest.unwrap_or(mention_hover))).child(div().absolute().left_0().top_0().bottom_0().w_0p5().bg(palette::accent())))
        .when(!mention, |r| r.when(held || focused || hovered, |r| r.bg(rest.unwrap_or(hover))).hover(move |s| s.bg(rest.unwrap_or(hover))))
        .on_hover(track_message_hover(message, ctx));
    // Last, so it paints over the message's own content where it overlaps it.
    let toolbar = hovered.then(|| actions(message, mine, ctx, cx));
    if ctx.compact {
        let line = ui::row().items_stretch().gap_2().child(compact_time(message, ctx, cx)).child(body);
        return inset(with_message_menu(row.when(starts_group, |r| r.mt(px(ctx.group_spacing))).child(line).children(toolbar), message, ctx), mention);
    }
    if !starts_group {
        let gutter = ui::row().items_stretch()
            .w(px(AVATAR))
            .flex_none()
            .justify_end()
            .line_height(px(ctx.body_line()))
            .text_size(ui::TEXT_XS)
            .text_color(palette::faint(cx))
            .whitespace_nowrap()
            .map(|g| match message.state {
                SendState::Sending => g.child(Icon::new(Glyph::Clock).size(ui::ICON_SM).mt_1()),
                _ => g.when(hovered, |g| g.child(gutter_clock(&message.timestamp))),
            });
        return inset(with_message_menu(row.child(ui::row().items_stretch().gap_4().child(gutter).child(body)).children(toolbar), message, ctx), mention);
    }
    let row = row
        .mt(px(ctx.group_spacing))
        .pt_1()
        .children(message.reply.as_ref().map(|reply| reply_preview(reply, message.reply_to.clone(), ctx, cx)))
        .children(components::used_command(message, ctx, cx))
        .child(
            ui::row().items_stretch()
                .gap_4()
                .child(author_trigger(format!("avatar-{}", message.id), message, ctx).flex_none().mt_0p5().child(crate::layout::decorated_avatar(
                    &message.author.avatar,
                    message.author.decoration.as_deref(),
                    AVATAR,
                    None,
                    palette::surface(Level::Page, cx),
                )))
                .child(ui::col().flex_1().child(message_head(message, ctx, cx)).child(body)),
        )
        .children(toolbar);
    inset(with_message_menu(row, message, ctx), mention)
}

// A message in a run from someone blocked or ignored: the run's first draws the fold, the rest nothing until
// Show opens it; shown, each is its usual row, faded.
pub(super) fn feed_row(messages: &[Message], ix: usize, starts_group: bool, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    match crate::system_message::is_notice(&messages[ix]) {
        true => crate::system_message::row(messages, ix, ctx, cx),
        false => message_row(&messages[ix], starts_group, ctx, cx),
    }
}

pub(super) fn folded_row(messages: &[Message], ix: usize, run: &Run, starts_group: bool, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    let row = run.shown.then(|| div().opacity(0.72).child(feed_row(messages, ix, starts_group, ctx, cx)));
    div().w_full().when(ix == run.start, |d| d.child(fold_line(&messages[run.start].id, run, ctx, cx))).children(row).into_any_element()
}

// design/explore-chat-guard.html `.fold`: one quiet line in the content column, and Show / Hide.
pub(super) fn fold_line(first_id: &str, run: &Run, ctx: &Rc<RowContext>, cx: &App) -> AnyElement {
    let (glyph, word) = match run.hidden {
        Hidden::Blocked => (Glyph::Prohibit, "blocked"),
        Hidden::Ignored => (Glyph::EyeSlash, "ignored"),
    };
    let label = match run.len {
        1 => format!("1 {word} message"),
        n => format!("{n} {word} messages"),
    };
    let (this, key) = (ctx.this.clone(), first_id.to_owned());
    let toggle = ui::button(SharedString::from(format!("fold-{first_id}")), ui::Tone::Ghost, cx)
        .small()
        .h_7()
        .px_2()
        .ml_0p5()
        .text_size(ui::TEXT_SM)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette::info_ink(cx))
        .child(if run.shown { "Hide" } else { "Show" })
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            this.update(cx, |this, cx| this.toggle_fold(key.clone(), cx)).ok();
        });
    let line = ui::row()
        .mt_1p5()
        .min_h_8()
        .pl(px(16. + CONTENT_INSET))
        .pr_4()
        .gap_2()
        .text_size(ui::TEXT_SM)
        .text_color(palette::faint(cx))
        .child(Icon::new(glyph).size(ui::ICON_MD))
        .child(label)
        .child(toggle);
    inset(line.into_any_element(), false)
}

// A search result: the message as the feed draws it (head, text, media, embeds, files), without the
// toolbar, reactions or reply chrome, on a card that jumps to it.
pub(crate) fn search_hit(message: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> Div {
    let media = hit_media(message, ctx, cx);
    let body = ui::col()
        .flex_1()
        .text_size(ui::TEXT_LEAD)
        .line_height(gpui_kit::relative(1.45))
        .text_color(palette::text(cx))
        .child(message_head(message, ctx, cx))
        .children(content(message, ctx, cx))
        .children(message.forward.as_ref().map(|original| forward_card(original, ctx, cx)))
        .children(media.map(|m| div().flex_none().child(m)))
        .children(message.embeds.iter().enumerate().map(|(i, e)| embed_card(e, None, message.reply_to.as_deref(), format!("hit-embed-{}-{i}", message.id), ctx, cx)))
        .children(files(message, ctx, cx));
    ui::row().items_stretch()
        .gap_3()
        .child(ui::avatar(message.author.avatar.clone(), 36.).flex_none().mt_0p5())
        .child(body)
}

// Result pictures: each in its own frame at its own shape, smaller than the feed's, side by side when
// there are several. Only a picture past `MEDIA_RATIO` (a long screenshot, a panorama) is cropped.
pub(super) const HIT_MEDIA_ONE: (f32, f32) = (280., 220.);
pub(super) const HIT_MEDIA_MANY: (f32, f32) = (150., 120.);
pub(super) const MEDIA_RATIO: (f32, f32) = (1. / 2.5, 2.5);

pub(super) fn hit_media(message: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> Option<AnyElement> {
    if message.media.is_empty() {
        return None;
    }
    let (max_w, max_h) = if message.media.len() == 1 { HIT_MEDIA_ONE } else { HIT_MEDIA_MANY };
    let max = (max_w.min(ctx.column), max_h);
    let tiles: Vec<AnyElement> = (0..message.media.len())
        .map(|i| {
            let item = &message.media[i];
            let (w, h) = fit_frame((item.width, item.height), max);
            div().flex_none().w(px(w)).h(px(h)).child(media_tile(message, i, (w, h), ui::all(RADIUS_MD), Paging::Feed, ctx, cx)).into_any_element()
        })
        .collect();
    Some(ui::row().items_stretch().mt_2().flex_wrap().gap_1().children(tiles).into_any_element())
}

// The frame for a picture of `size` within `max`: its own shape, never larger than it is, with the shape
// held between 1:2.5 and 2.5:1. An unknown size gets a square.
pub(super) fn fit_frame((width, height): (f32, f32), (max_w, max_h): (f32, f32)) -> (f32, f32) {
    if width <= 0. || height <= 0. {
        return (max_h, max_h);
    }
    let ratio = (width / height).clamp(MEDIA_RATIO.0, MEDIA_RATIO.1);
    let w = max_w.min(max_h * ratio).min(width);
    (w, w / ratio)
}

// Rows sit 8px in from the feed's edges, so their hover fill reads as a rounded card; a mention is flush.
pub(crate) fn inset(row: AnyElement, flush: bool) -> AnyElement {
    div().w_full().when(!flush, |d| d.px_2()).child(row).into_any_element()
}

// The author, the time, and a pin when pinned.
pub(crate) fn message_head(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> impl IntoElement + use<> {
    // GAP: gpui text reports no baseline to taffy, so `items_baseline` would line up box bottoms and
    // lift the name. One line box, centred, with the smaller time nudged 1px, shares a baseline.
    ui::row()
        .gap_2()
        .line_height(px(ctx.name_line()))
        .child(author(message, ctx, cx))
        .when(message.silent(), |h| h.child(silent_mark(&message.id, cx)))
        .child(div().flex_none().relative().top_px().whitespace_nowrap().text_size(ui::TEXT_XS).text_color(palette::faint(cx)).child(format_stamp(&message.timestamp)))
        .when(message.pinned, |h| h.child(div().flex_none().self_center().text_color(palette::muted(cx)).child(Icon::new(Glyph::Pin).size(ui::ICON_SM))))
}

// A crosspost's author is its source server, whose card opens instead of the webhook's profile.
pub(super) fn author_trigger(id: String, message: &Message, ctx: &Rc<RowContext>) -> Stateful<Div> {
    crate::announcements::source_trigger(id.clone(), message, ctx.this.clone()).unwrap_or_else(|| profile_trigger(id, &message.author, ctx))
}

// The name in its display-name style (unless those are off) and its role colour or dot, per Role colors,
// then the server tag and APP for a bot.
pub(super) fn author(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> Div {
    let (ink, dot) = ctx.role_marks(&message.author.id);
    let name = ctx.name_of(&message.author);
    let style = message.author.name_style.as_ref().filter(|_| ctx.name_styles);
    let runs = ctx.style_ground.zip(style).map(|(ground, style)| name_runs(&name, style, ground)).unwrap_or_default();
    ui::row()
        .gap_2()
        .child(
            crate::name_fonts::styled(author_trigger(format!("name-{}", message.id), message, ctx).min_w_0().truncate().text_size(px(ctx.font_size)).font_weight(FontWeight::MEDIUM), style)
                .text_color(ink.unwrap_or(palette::strong(cx)))
                .hover(|s| s.underline())
                .child(StyledText::new(name).with_highlights(runs)),
        )
        .children(dot.map(role_dot))
        .children(message.author.tag.as_ref().map(|tag| guild_tag(tag, cx)))
        .when(crate::announcements::is_crosspost(message), |h| h.child(crate::announcements::server_tag(message, cx)))
        .when(message.author.bot && !crate::announcements::is_crosspost(message), |h| h.child(ui::badge(palette::accent_soft(cx), palette::accent_ink(cx)).h(px(18.)).px_1p5().gap(px(3.)).child(Icon::new(Glyph::Checkmark).size(px(11.))).child("APP")))
}

// Not verified: Discord's compact time column and avatar; 52px and 18px in design/explore-settings.html,
// the column widened to fit "12:59 PM".
pub(super) const COMPACT_TIME: f32 = 56.;
pub(super) const COMPACT_AVATAR: f32 = 18.;

// Compact's time column: every message shows its own time, so names and text line up down the feed.
pub(super) fn compact_time(message: &Message, ctx: &RowContext, cx: &App) -> Div {
    ui::row()
        .w(px(COMPACT_TIME))
        .flex_none()
        .h(px(ctx.body_line()))
        .whitespace_nowrap()
        .text_size(ui::TEXT_XS)
        .text_color(palette::faint(cx))
        .map(|t| match message.state {
            SendState::Sending => t.child(Icon::new(Glyph::Clock).size(ui::ICON_SM)),
            _ => t.child(clock(&message.timestamp)),
        })
}

// Compact's first line: a group's first message leads with its author (a small avatar when Show user
// avatars is on) and the text follows on the same line; a continuation is its text alone.
pub(super) fn compact_line(message: &Message, starts_group: bool, text: Option<AnyElement>, ctx: &Rc<RowContext>, cx: &App) -> Option<AnyElement> {
    if !starts_group {
        return text;
    }
    let line = ctx.body_line();
    let avatar = ctx.compact_avatars.then(|| profile_trigger(format!("avatar-{}", message.id), &message.author, ctx).flex_none().h(px(line)).flex().items_center().child(ui::avatar(message.author.avatar.clone(), COMPACT_AVATAR)));
    Some(
        ui::row()
            .items_start()
            .gap_1p5()
            .children(avatar)
            .child(author(message, ctx, cx).flex_none().max_w(gpui_kit::relative(0.4)).h(px(line)))
            .children(text.map(|text| div().flex_1().min_w_0().child(text)))
            .into_any_element(),
    )
}

// @silent's sleeping bell beside the name.
pub(super) fn silent_mark(id: &str, cx: &App) -> impl IntoElement + use<> {
    div()
        .id(SharedString::from(format!("silent-{id}")))
        .flex_none()
        .self_center()
        .text_color(palette::faint(cx))
        .tooltip(|window, cx| ui::tip("This is a @silent message.", window, cx))
        .child(Icon::new(Glyph::BellZ).size(ui::ICON_MD))
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

pub(super) fn track_message_hover(message: &Message, ctx: &RowContext) -> impl Fn(&bool, &mut Window, &mut App) + 'static {
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

pub(super) fn set_hovered_message(this: &WeakEntity<Waffle>, id: Option<String>, cx: &mut App) {
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
pub(super) fn with_message_menu(row: Stateful<Div>, message: &Message, ctx: &Rc<RowContext>) -> AnyElement {
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

pub(super) fn plain_click(event: &gpui_kit::ClickEvent) -> bool {
    let gpui_kit::ClickEvent::Mouse(click) = event else { return false };
    let moved = click.up.position - click.down.position;
    click.up.click_count == 1 && f32::from(moved.x).abs() < 4. && f32::from(moved.y).abs() < 4.
}
