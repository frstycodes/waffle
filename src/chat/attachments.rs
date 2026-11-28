use super::*;

// A spoiler's blur, a reaction's side padding (with the add pill's), the code copy button's side, and how far
// the hover bar rides above its row.
// Discord's reaction: 20px emoji, 2px above and below, 6px each side, inside a 1px border.
const REACTION_PAD: f32 = 6.;
const REACTION_HEIGHT: f32 = 26.;
const REACTION_ADD_PAD: f32 = 6.;
const COPY_BUTTON: f32 = 28.;
const ACTIONS_LIFT: f32 = 18.;

// Audio plays in place; text files read in place (the fetch starts here, when the row first renders);
// any other file, or one that turned out not to be text, is a card with its icon, name, size and one action.
pub(super) fn files(message: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> Vec<AnyElement> {
    message
        .files
        .iter()
        .map(|file| {
            if message.state == SendState::Sent && file.is_audio() {
                return crate::audio_player::tile(message, file, ctx.this.clone(), cx);
            }
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
            crate::text_file::inline(format!("text-{}-{}", message.id, file.name), file, state, text_files.view(&file.url), copied, ctx.this.clone(), cx).into_any_element()
        })
        .collect()
}

// The files of a message still uploading: each with its picture, its bytes out of its size as a bar so
// a big file visibly moves, and Cancel, which drops the whole message as Discord's does.
pub(super) fn upload_cards(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> Vec<AnyElement> {
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
            let picture = file_picture(ctx.previews.get(&file.path).cloned().flatten(), kind, 40., file.spoiler, cx);
            let status = match job.error.is_some() {
                true => format_size(file.size),
                false => format!("{} of {}", format_size(sent), format_size(file.size)),
            };
            let card = ui::row()
                .w_full()
                .max_w(px(COLUMN))
                .p_3()
                .gap_3()
                .squircle(RADIUS_LG)
                .bg(palette::surface(Level::Sidebar, cx))
                .border_1()
                .border_color(palette::line(cx))
                .child(picture)
                .child(
                    ui::col().min_h_0()
                        .flex_1()
                        .gap_1p5()
                        .child(ui::row().items_baseline().gap_2().child(ui::line(file.name.clone()).flex_1().text_size(ui::TEXT_SM).font_weight(FontWeight::MEDIUM).text_color(strong)).child(div().flex_none().text_size(ui::TEXT_XS).text_color(faint).child(status)))
                        .when(job.error.is_none(), |d| d.child(ui::Progress::new(fraction))),
                )
                .when(job.error.is_none(), |d| {
                    let (this, cancel) = (ctx.this.clone(), message.clone());
                    d.child(ui::Button::new(SharedString::from(format!("cancel-upload-{}-{ix}", message.id))).variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconXs).icon(Glyph::X).tooltip_text("Cancel upload").on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        this.update(cx, |this, cx| this.delete(cancel.clone(), cx)).ok();
                    }))
                });
            div().pt(px(ACCESSORY_GAP)).child(card).into_any_element()
        })
        .collect()
}

// A file's tile picture: the OS's thumbnail or type icon once drawn, a quiet well until then. A
// spoiler is veiled, as Discord blurs it.
pub(super) fn file_picture(picture: Option<std::sync::Arc<gpui_kit::RenderImage>>, kind: crate::attachments::FileKind, side: f32, spoiler: bool, cx: &App) -> AnyElement {
    let well = div().relative().size(px(side)).flex_none().squircle(RADIUS_XS).overflow_hidden();
    let well = match picture {
        // Thumbnails fill the tile; icons keep their own transparent margins.
        Some(image) => well.child(ui::media(image, ui::all(RADIUS_XS)).size_full().object_fit(gpui_kit::ObjectFit::Cover)),
        None => well.bg(palette::surface(Level::Raised, cx)).text_color(palette::muted(cx)).flex().items_center().justify_center().child(Icon::new(kind.glyph()).size(px(side * 0.5))),
    };
    well.when(spoiler, |w| {
        w.child(ui::row().absolute().inset_0().bg(palette::surface(Level::Sidebar, cx)).justify_center().text_color(palette::strong(cx)).child(Icon::new(Glyph::EyeSlash).size(px(side * 0.4))))
    })
    .into_any_element()
}

pub(crate) fn file_card(message: &Message, file: &crate::discord::File, cx: &App) -> AnyElement {
    let (strong, faint, muted) = (palette::strong(cx), palette::faint(cx), palette::muted(cx));
    let (open, download) = (file.url.clone(), file.url.clone());
    let card = ui::row()
        .w_full()
        .max_w(px(COLUMN))
        .p_3()
        .gap_3()
        .squircle(RADIUS_LG)
        .bg(palette::surface(Level::Sidebar, cx))
        .border_1()
        .border_color(palette::line(cx))
        .when(message.state != SendState::Sent, |d| d.opacity(0.6))
        .child(ui::row().items_stretch().w_10().flex_none().justify_center().text_color(muted).child(Icon::new(Glyph::File).size_9()))
        .child(
            ui::col().min_h_0()
                .flex_1()
                .child(
                    ui::line(file.name.clone())
                        .id(SharedString::from(format!("file-{}-{}", message.id, file.name)))
                        .cursor_pointer()
                        .text_size(ui::TEXT_SM)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(strong)
                        .hover(|s| s.underline())
                        .on_click(move |_, _, cx| { cx.stop_propagation(); cx.open_url(&open) }),
                )
                // A Components V2 file that matches no attachment comes without a size.
                .child(ui::row().gap_1p5().text_size(ui::TEXT_XS).text_color(faint).when(file.size > 0, |d| d.child(format_size(file.size))).when(file.spoiler, |d| d.child(ui::Badge::new("Spoiler").variant(ui::BadgeVariant::Accent)))),
        )
        .child(ui::Button::new(SharedString::from(format!("download-{}-{}", message.id, file.name))).variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).icon(Glyph::Download).tooltip_text("Download").on_click(move |_, _, cx| { cx.stop_propagation(); cx.open_url(&download) }));
    div().pt(px(ACCESSORY_GAP)).child(card).into_any_element()
}

// Sits in the corner of every code block; it turns to a check, in the online ink, for a moment after.
// GAP: the design gives code blocks a header line (language, then Copy with its label); TextView owns
// that layout and only offers this corner slot, so the button stays icon-only.
pub(super) fn copy_button(code: String, copied: bool, this: WeakEntity<Waffle>, _cx: &App) -> AnyElement {
    ui::Button::new(SharedString::from(format!("copy-{}", code.len()))).variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::Square(COPY_BUTTON)).icon(if copied { Glyph::Checkmark } else { Glyph::Copy }).tooltip_text(if copied { "Copied" } else { "Copy" })
        .succeeded(copied)
        .on_click(move |_, _, cx| { cx.stop_propagation();
            this.update(cx, |this, cx| this.copy_code(code.clone(), code.clone(), cx)).ok();
        })
        .into_any_element()
}

// One click adds or removes yours; yours carry the accent, the rest stay neutral. Add stays last.
pub(super) fn reactions(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> Option<AnyElement> {
    if message.reactions.is_empty() {
        return None;
    }
    let pills = message.reactions.iter().map(|r| {
        let (this, msg, emoji) = (ctx.this.clone(), message.clone(), r.emoji.clone());
        let (hover_this, hover_msg, hover_emoji) = (ctx.this.clone(), message.clone(), r.emoji.clone());
        let card = crate::reactions::ReactorsCard::new(ctx.this.clone(), message, r);
        reaction_pill(SharedString::from(format!("react-{}-{}", message.id, r.emoji.name)), r.me, cx)
            .px(px(REACTION_PAD))
            .child(emoji_face(&shown_emoji(&r.emoji, ctx.media.emoji), 20., 18.))
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
        .px(px(REACTION_ADD_PAD))
        .child(Icon::new(Glyph::EmojiAdd).size(ui::ICON_LG))
        .tooltip(ui::Tooltip::text("Add Reaction"))
        .on_click(move |_, window, cx| { cx.stop_propagation();
            this.update(cx, |this, cx| this.open_emoji_picker(EmojiTarget::Reaction(Box::new(msg.clone())), window, cx)).ok();
        });
    Some(div().pt(px(REACTIONS_GAP)).child(ui::row().items_stretch().flex_wrap().gap(px(ACCESSORY_GAP)).children(pills).child(add)).into_any_element())
}

// A custom emoji's picture, or a unicode one drawn as text.
pub(super) fn emoji_face(emoji: &Emoji, image: f32, text: f32) -> AnyElement {
    match emoji.url() {
        Some(url) => img(url).size(px(image)).into_any_element(),
        None => div().text_size(px(text)).line_height(px(text)).child(emoji.name.clone()).into_any_element(),
    }
}

pub(super) fn reaction_pill(id: SharedString, mine: bool, cx: &App) -> ui::Pressable {
    let edge = palette::line_strong(cx);
    ui::pressable(div().id(id), 0.92)
        .h(px(REACTION_HEIGHT))
        .flex()
        .items_center()
        .gap_1p5()
        .squircle(RADIUS_SM)
        .border_1()
        .cursor_pointer()
        .text_size(ui::TEXT_SM)
        .font_weight(FontWeight::SEMIBOLD)
        .when(mine, |p| p.bg(palette::accent_soft(cx)).border_color(palette::accent().opacity(0.55)).text_color(palette::accent_ink(cx)))
        .when(!mine, |p| p.bg(palette::selected(cx)).border_color(gpui_kit::transparent_black()).text_color(palette::muted(cx)).hover(move |s| s.border_color(edge)))
}

// The hover toolbar, in Discord's order: three quick reactions, then react, reply or edit, forward (or
// create thread) and more. Shift lays the menu's direct actions out instead; a failed send offers Retry and Delete.
pub(super) fn actions(message: &Message, mine: bool, ctx: &Rc<RowContext>, cx: &App) -> AnyElement {
    use crate::message_menu::MessageAction as A;
    let (hover, line) = (palette::hover(cx), palette::line(cx));
    let sep = || ui::Separator::new().orientation(ui::Orientation::Vertical).bg(line);
    let sent = message.state == SendState::Sent;
    let facts = crate::message_menu::Facts { mine, saved: ctx.saved_ids.contains(&message.id), ..ctx.facts };
    // lint-allow(custom-mark, custom-surface): ui::toolbar occludes the wheel too, and this bar must let the feed scroll under it; it also rides above its row's edge
    let mut bar = ui::row()
        .id(SharedString::from(format!("actions-{}", message.id)))
        // Rides 18px into the row above: block the pointer there and keep this row hovered, or the pointer hands
        // the hover (and the bar) to that row. The wheel still reaches the feed under it.
        .block_mouse_except_scroll()
        .on_hover(track_message_hover(message, ctx))
        .absolute()
        .top(px(-ACTIONS_LIFT))
        // A flush mention row is 8px wider each side; the bar stays where it sits on other rows.
        .right(px(if ctx.mentions_me.contains(&message.id) { 20. } else { 12. }))
        .p_0p5()
        .gap_0p5()
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
            A::Save if facts.saved => (Glyph::Bookmark, "Remove Bookmark"),
            A::Save => (Glyph::Bookmark, "Bookmark Message"),
            A::Delete => (Glyph::Trash, "Delete"),
            A::CopyId => (Glyph::Id, "Copy Message ID"),
        };
        let (this, msg) = (ctx.this.clone(), message.clone());
        let button = ui::Button::new(SharedString::from(format!("{action:?}-{}", message.id))).variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).icon(glyph).tooltip_text(tip).on_click(move |_, window, cx| {
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
        // A failed send: Discord's Retry and Delete, nothing else.
        let (this, msg) = (ctx.this.clone(), message.clone());
        let retry = ui::Button::new(SharedString::from(format!("retry-{}", message.id))).variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).icon(Glyph::Retry).tooltip_text("Retry").on_click(move |_, _, cx| {
            cx.stop_propagation();
            this.update(cx, |this, cx| this.retry(msg.clone(), cx)).ok();
        });
        bar = bar.when(message.state == SendState::Failed, |b| b.child(retry)).children(offer(A::Delete).map(|a| action(a, cx)));
    } else if ctx.shift {
        // Shift: the menu's direct actions laid out on the bar, in Discord's order.
        let (this, msg) = (ctx.this.clone(), message.clone());
        let link = ui::Button::new(SharedString::from(format!("link-{}", message.id))).variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).icon(Glyph::Link).tooltip_text("Copy Message Link").on_click(move |_, _, cx| {
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
                    .size_8()
                    .flex()
                    .items_center()
                    .justify_center()
                    .squircle(RADIUS_SM)
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .child(face)
                    .tooltip(ui::Tooltip::text("Click to react"))
                    .on_click(move |_, _, cx| { cx.stop_propagation();
                        this.update(cx, |this, cx| this.toggle_reaction(&msg, emoji.clone(), cx)).ok();
                    }),
            );
        }
        // Discord's order: Reply on others' messages, Edit on yours, and Create Thread only where Forward isn't offered.
        let forward = offer(A::Forward);
        let reply = offer(A::Reply).filter(|_| !mine);
        let thread = offer(A::Thread).filter(|_| forward.is_none());
        bar = bar.child(sep()).children([offer(A::React), reply, offer(A::Edit), forward, thread].into_iter().flatten().map(|a| action(a, cx)));
        bar = bar.children(crate::announcements::publish_button(message, mine, &ctx.announce, ctx.this.clone(), cx));
    }
    if !ctx.shift && sent {
        let (this, msg) = (ctx.this.clone(), message.clone());
        bar = bar.child(ui::Button::new(SharedString::from(format!("more-{}", message.id))).variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).icon(Glyph::MoreVertical).tooltip_text("More").on_click(move |event, _, cx| { cx.stop_propagation();
            let at = event.position();
            this.update(cx, |this, cx| this.open_message_menu(msg.clone(), at, cx)).ok();
        }));
    }
    // GAP: not `deferred`: inside the chat list a deferred bar never painted. It overlaps only the message
    // above, which the list paints first, so it draws on top without it.
    bar.into_any_element()
}
