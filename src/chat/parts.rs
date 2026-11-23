use super::*;

// The reference above a reply, in the content column; clicking it jumps to the original and flashes it.
// Discord's reply spine: from the preview's middle it runs left to over the avatar's centre, rounds
// the corner and drops to just above the avatar, so the preview lines up with the name below it.
pub(super) fn reply_spine(cx: &App) -> Div {
    div()
        .absolute()
        .left(px(AVATAR / 2. - 1.))
        .top(px(REPLY_HEIGHT / 2.))
        .w(px(CONTENT_INSET - AVATAR / 2. - 4.))
        .h(px(REPLY_HEIGHT / 2. + 4.))
        .border_t_2()
        .border_l_2()
        .rounded_tl_md()
        .border_color(palette::line_strong(cx))
}

pub(super) fn reply_preview(reply: &Message, target: Option<String>, ctx: &Rc<RowContext>, cx: &App) -> AnyElement {
    let (faint, muted, text) = (palette::faint(cx), palette::muted(cx), palette::text(cx));
    let hidden = ctx.hidden_replies.get(&reply.id).map(|hidden| match hidden {
        Hidden::Blocked => "a blocked message",
        Hidden::Ignored => "an ignored message",
    });
    let quote: Vec<AnyElement> = match hidden {
        Some(label) => vec![ui::line(label).into_any_element()],
        None => vec![ui::avatar(reply.author.avatar.clone(), 16.).into_any_element(), div().flex_none().font_weight(FontWeight::MEDIUM).text_color(muted).child(ctx.name_of(&reply.author)).into_any_element(), reply_snippet(reply, ctx, cx)],
    };
    let this = ctx.this.clone();
    ui::row()
        .id(SharedString::from(format!("reply-{}", reply.id)))
        .relative()
        .h(px(REPLY_HEIGHT))
        // Compact draws it in the text column, with no avatar for the spine to reach.
        .when(!ctx.compact, |r| r.pl(px(CONTENT_INSET)).child(reply_spine(cx)))
        .mb_0p5()
        .gap_1p5()
        .cursor_pointer()
        .text_size(ui::TEXT_SM)
        .text_color(faint)
        .hover(move |s| s.text_color(text))
        .children(quote)
        .on_click(move |_, _, cx| { cx.stop_propagation();
            let Some(id) = target.clone() else { return };
            this.update(cx, |this, cx| this.jump_to(id, cx)).ok();
        })
        .into_any_element()
}

pub(super) fn reply_snippet(reply: &Message, ctx: &Rc<RowContext>, cx: &App) -> AnyElement {
    match reply.content.is_empty() {
        true => ui::row().flex_none().gap_1().text_color(palette::muted(cx)).child(Icon::new(attachment_glyph(reply)).size(ui::ICON_SM)).child(attachment_kind(reply)).into_any_element(),
        false => ui::line(plain(&readable_with(&reply.content, ctx, false))).into_any_element(),
    }
}

// A forwarded copy (design/messages.html `.fwd`): the label, the original's text, media and files, then
// where it came from, which jumps to the original. A source outside your servers shows its time alone.
pub(super) fn forward_card(original: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    let faint = palette::faint(cx);
    let label = ui::row()
        .gap_1p5()
        .text_size(ui::TEXT_XS)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette::muted(cx))
        .child(Icon::new(Glyph::Forward).size(ui::ICON_SM))
        .child("Forwarded");
    let name = ctx.channel_names.get(&original.channel_id).cloned();
    let glyph = if original.guild_id.is_some() { Glyph::Hashtag } else { Glyph::User };
    let stamp = format_stamp(&original.timestamp);
    let source = ui::row()
        .id(SharedString::from(format!("forward-source-{}", original.id)))
        .gap_1()
        .text_size(ui::TEXT_XS)
        .text_color(faint)
        .when_some(name.clone(), |s, name| s.child(Icon::new(glyph).size(ui::ICON_SM)).child(format!("{name} · {stamp}")))
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
    ui::col()
        .mt_1()
        .max_w(px(FORWARD_WIDTH))
        .pt_2()
        .px_3()
        .pb_2p5()
        .squircle(RADIUS_MD)
        .bg(palette::strong(cx).opacity(0.05))
        .gap_1()
        .child(label)
        .children(content(original, ctx, cx))
        .children(media_grid(original, ctx, cx))
        .children(original.stickers.iter().map(|s| crate::stickers::in_chat(s, ctx.media.stickers.plays(false), cx)))
        .children(shown_embeds(original, ctx).iter().enumerate().map(|(i, e)| embed_card(e, None, None, format!("forward-embed-{}-{i}", original.id), ctx, cx)))
        .children(files(original, ctx, cx))
        .child(source)
        .into_any_element()
}

// What a reply without text quotes instead: the kind of attachment it carries.
pub(super) fn attachment_kind(message: &Message) -> &'static str {
    match message.media.first().map(|m| &m.kind) {
        Some(MediaKind::Image) => "Image",
        Some(MediaKind::Video { .. }) => "Video",
        Some(MediaKind::Gif { .. }) => "GIF",
        None if !message.stickers.is_empty() => "Sticker",
        None if !message.files.is_empty() => "File",
        None => "Attachment",
    }
}

pub(super) fn attachment_glyph(message: &Message) -> Glyph {
    if message.media.is_empty() && !message.stickers.is_empty() {
        return Glyph::Sticker;
    }
    match message.media.is_empty() && !message.files.is_empty() {
        true => Glyph::File,
        false => Glyph::Image,
    }
}

// Discord drops the text of a message that is nothing but the link its GIF or image embed shows.
pub(super) fn only_media_link(message: &Message) -> bool {
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
    let body = ui::col()
        .flex_1()
        .text_size(ui::TEXT_MD)
        .text_color(palette::text(cx))
        .child(message_head(parent, ctx, cx))
        .children(content(parent, ctx, cx))
        .children(parent.forward.as_ref().map(|original| forward_card(original, ctx, cx)))
        .children(media_grid(parent, ctx, cx))
        .children(parent.stickers.iter().map(|s| crate::stickers::in_chat(s, ctx.media.stickers.plays(false), cx)))
        .children(parent.poll.as_ref().map(|p| poll_card(parent, p, ctx, cx)))
        .children(shown_embeds(parent, ctx).iter().enumerate().map(|(i, e)| embed_card(e, None, parent.reply_to.as_deref(), format!("starter-embed-{}-{i}", parent.id), ctx, cx)))
        .children(files(parent, ctx, cx));
    ui::col()
        .line_height(gpui_kit::relative(1.45))
        .children(parent.reply.as_ref().map(|reply| reply_preview(reply, parent.reply_to.clone(), ctx, cx)))
        .child(
            ui::row().items_stretch()
                .gap_4()
                .child(profile_trigger(format!("starter-avatar-{}", parent.id), &parent.author, ctx).flex_none().mt_0p5().child(ui::avatar(parent.author.avatar.clone(), AVATAR)))
                .child(body),
        )
}

pub(crate) fn content(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> Option<AnyElement> {
    if message.flags & Message::SOURCE_MESSAGE_DELETED != 0 {
        return Some(crate::announcements::source_deleted(cx));
    }
    // A lone link whose media the setting keeps out of line stays as its text.
    if message.content.is_empty() || (ctx.media.links && only_media_link(message)) {
        return None;
    }
    // A message of only custom emoji renders them big, as Discord's "jumbo" emoji.
    let emoji = custom_emoji(&message.content);
    if !emoji.is_empty() && strip_emoji(&message.content).trim().is_empty() && emoji.len() <= 27 {
        return Some(ui::row().items_stretch().flex_wrap().gap_1().children(emoji.iter().filter_map(|e| shown_emoji(e, ctx.media.emoji).url()).enumerate().map(|(i, url)| img(url).id(("jumbo", i)).size_12())).into_any_element());
    }
    // Spoilers stay covered until the message is clicked, unless the setting shows them; "-# " lines are
    // Discord's small grey subtext.
    let revealed = ctx.spoilers_revealed(&message.id);
    let has_spoiler = message.content.contains("||");
    let text = hide_spoilers(&readable(&message.content, ctx), revealed);
    let mut blocks = text_blocks(&format!("md-{}", message.id), &text, ctx.font_size, ctx, cx);
    // Only "(edited)" needs the last block sized to its content, so it can sit beside it. Unedited text stays a
    // column child at the feed's width: a content-sized block once measured near zero wide and wrapped every
    // glyph onto its own line.
    let last = message.edited.then(|| blocks.pop()).flatten();
    let (reveal, id) = (ctx.revealed.clone(), message.id.clone());
    let refresh = ctx.this.clone();
    let faint = palette::faint(cx);
    Some(
        ui::col()
            .id(SharedString::from(format!("content-{}", message.id)))
            .w_full()
            .when(has_spoiler && !revealed, |c| {
                c.cursor_pointer().on_click(move |_, _, cx| { cx.stop_propagation();
                    reveal.borrow_mut().insert(id.clone());
                    refresh.update(cx, |_, cx| cx.notify()).ok();
                })
            })
            .children(blocks)
            .when(message.edited, |c| {
                c.child(ui::row().flex_wrap().items_baseline().children(last).child(div().ml_1().whitespace_nowrap().text_size(ui::TEXT_XS).text_color(faint).child("(edited)")))
            })
            .into_any_element(),
    )
}

// Not sent: the text stays, with why and the two ways out inline.
pub(super) fn send_failure(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> AnyElement {
    let (retry_this, retry) = (ctx.this.clone(), message.clone());
    let (delete_this, delete) = (ctx.this.clone(), message.clone());
    let action = |id: String, label: &'static str| ui::button(SharedString::from(id), ui::Tone::Ghost, cx).small().h_7().child(label);
    ui::row()
        .mt_1()
        .flex_wrap()
        .gap_x_2()
        .gap_y_1()
        .text_size(ui::TEXT_SM)
        .font_weight(FontWeight::NORMAL)
        .text_color(palette::danger_ink(cx))
        .child(Icon::new(Glyph::WarningCircle).size(ui::ICON_MD))
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
pub(super) fn thread_card(thread: &Channel, ctx: &Rc<RowContext>, cx: &App) -> AnyElement {
    let meta = thread.thread.as_ref();
    let count = meta.map_or(0, |t| t.message_count);
    let archived = meta.is_some_and(|t| t.archived);
    let (this, open) = (ctx.this.clone(), thread.clone());
    let edge = palette::line_strong(cx);
    ui::pressable(div().id(SharedString::from(format!("thread-card-{}", thread.id))), 0.99)
        .mt_2()
        .w_full()
        .max_w(px(COLUMN))
        .py_2p5()
        .px_3()
        .flex()
        .items_center()
        .gap_2()
        .min_w_0()
        .squircle(RADIUS_LG)
        .bg(palette::surface(Level::Sidebar, cx))
        .border_1()
        .border_color(palette::line(cx))
        .hover(move |s| s.border_color(edge))
        .cursor_pointer()
        .text_size(ui::TEXT_SM)
        .child(ui::line(thread.name.clone()).font_weight(FontWeight::MEDIUM).text_color(if archived { palette::muted(cx) } else { palette::strong(cx) }))
        .child(
            ui::row()
                .flex_none()
                .gap_0p5()
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::muted(cx))
                .child(format!("{count} {}", if count == 1 { "message" } else { "messages" }))
                .child(Icon::new(Glyph::ChevronRight).size(ui::ICON_SM)),
        )
        .on_click(move |_, window, cx| { cx.stop_propagation();
            this.update(cx, |this, cx| this.open_channel(open.clone(), window, cx)).ok();
        })
        .into_any_element()
}
