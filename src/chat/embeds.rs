use super::*;

// An embed description's fade height and its Show more toggle's height; the play disc over a video still, its
// glyph and the Open Link corner button's size.
const EMBED_FADE: f32 = 72.;
const SHOW_MORE_HEIGHT: f32 = 26.;
const PLAY_DISC: f32 = 56.;
const PLAY_ICON: f32 = 26.;
const OPEN_LINK_SIZE: f32 = 32.;

// A link preview: it leads with the site, never a coloured bar, one surface step from the feed. Link
// previews keep a thumbnail at the right; large images run the card's width.
// Previews of the Discord links the message carries (link_preview.rs).
pub(super) fn link_cards(message: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> Vec<AnyElement> {
    // Discord's one setting covers both: "Show embeds and preview website links".
    if message.state != SendState::Sent || !ctx.embeds {
        return Vec::new();
    }
    let quote = |content: &str| plain(&readable_with(content, ctx, false));
    let cards = crate::link_preview::CardContext { this: ctx.this.clone(), previews: &ctx.link_previews, joined: &ctx.joined, channel_names: &ctx.channel_names, quote: &quote, stamp: format_stamp };
    crate::link_preview::cards(&message.id, &message.content, &cards, cx)
}

// Settings › Chat can turn embeds off, and Discord's SUPPRESS_EMBEDS flag removes a message's for everyone.
pub(super) fn shown_embeds<'a>(message: &'a Message, ctx: &RowContext) -> &'a [Embed] {
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
pub(super) fn embed_card(embed: &Embed, row: Option<&Message>, poll_message: Option<&str>, key: String, ctx: &RowContext, cx: &App) -> AnyElement {
    if let Some(poll) = &embed.poll {
        return poll_result(poll, poll_message, &key, ctx, cx);
    }
    let (strong, faint) = (palette::strong(cx), palette::faint(cx));
    let fill = palette::surface(Level::Sidebar, cx);
    let bar = embed.color.map_or(palette::line_strong(cx), palette::data_color);
    let width = EMBED_WIDTH.min(ctx.column);
    let inner = width - EMBED_PAD.1 - EMBED_PAD.3 - EMBED_BAR - 2.;
    let md = |part: &str, text: &str| markdown(SharedString::from(format!("{key}-{part}")), readable(text, ctx), EMBED_TEXT, ctx);
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
            .text_size(ui::TEXT_XS)
            .font_weight(FontWeight::MEDIUM)
            .text_color(faint)
            .when_some(embed.provider_url.clone(), |d, url| d.cursor_pointer().hover(|s| s.underline()).on_click(open(url)))
            .child(name)
    });
    let author = embed.author.clone().map(|name| {
        ui::row()
            .id(SharedString::from(format!("{key}-author")))
            .gap_2()
            .text_size(ui::TEXT_SM)
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(strong)
            .children(embed.author_icon.clone().map(|icon| ui::avatar(icon, 24.)))
            .child(ui::line(name).when(embed.author_url.is_some(), |d| d.cursor_pointer().hover(|s| s.underline())))
            .when_some(embed.author_url.clone(), |d, url| d.on_click(open(url)))
    });
    let title = embed.title.clone().map(|title| {
        div()
            .id(SharedString::from(format!("{key}-title")))
            .text_size(ui::TEXT_MD)
            .line_height(gpui_kit::relative(1.2))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(if embed.url.is_some() { palette::accent_ink(cx) } else { strong })
            .when_some(embed.url.clone(), |t, url| t.cursor_pointer().hover(|s| s.underline()).on_click(open(url)))
            .child(title)
    });
    let description = embed.description.as_deref().map(|text| {
        let more = format!("{key}-more");
        let long = long_description(text);
        let open_full = !long || ctx.revealed.borrow().contains(&more);
        let (reveal, refresh) = (ctx.revealed.clone(), ctx.this.clone());
        ui::col()
            .items_start()
            .child(
                div()
                    .relative()
                    .w_full()
                    .text_color(palette::text(cx))
                    .when(!open_full, |d| d.max_h(px(EMBED_CLIP)).overflow_hidden())
                    .child(md("description", text))
                    // The fade says there is more; it ends in the card's own fill.
                    .when(!open_full, |d| d.child(div().absolute().left_0().right_0().bottom_0().h(px(EMBED_FADE)).bg(linear_gradient(180., linear_color_stop(fill.opacity(0.), 0.), linear_color_stop(fill, 1.))))),
            )
            .when(long, |d| {
                d.child(
                    // lint-allow(custom-button): ui::button's smallest Ghost is 30px with the muted ink; this is a 26px toggle in the accent ink that hangs 6px left under the text
                    ui::row()
                        .id(SharedString::from(more.clone()))
                        // lint-allow(raw-margin): optical, the toggle's wash hangs left so its label lines up with the text above
                        .mt_0p5()
                        // lint-allow(raw-margin): optical, the toggle's wash hangs left so its label lines up with the text above
                        .ml_neg_1p5()
                        .h(px(SHOW_MORE_HEIGHT))
                        .px_1p5()
                        .squircle(RADIUS_XS)
                        .text_size(ui::TEXT_XS)
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
        ui::col().pt_1().gap_2p5().children(field_rows(&embed.fields, embed.thumbnail.is_some()).into_iter().map(|row| {
            ui::row().items_stretch().gap_4().children(row.into_iter().map(|i| {
                let (name, value, _) = &embed.fields[i];
                ui::col().min_h_0()
                    .flex_1()
                    .flex_basis(Pixels::ZERO)
                    .gap_0p5()
                    .child(div().text_size(ui::TEXT_XS).font_weight(FontWeight::SEMIBOLD).text_color(palette::muted(cx)).child(name.clone()))
                    .child(div().text_color(strong).child(md(&format!("field-{i}"), value)))
            }))
        }))
    });
    let thumbnail = embed.thumbnail.as_ref().map(|m| {
        let url = crate::discord::sized_image(&m.url, EMBED_THUMB, EMBED_THUMB, (m.width, m.height));
        img(url).size(px(EMBED_THUMB)).flex_none().squircle(RADIUS_SM).object_fit(gpui_kit::ObjectFit::Cover)
    });
    let text = ui::col().min_h_0()
        .flex_1()
        .gap_1p5()
        .children(provider)
        .children(author)
        .children(title)
        .children(description)
        .children(fields);
    let has_text = embed.provider.is_some() || embed.author.is_some() || embed.title.is_some() || embed.description.is_some() || !embed.fields.is_empty();
    let top = (has_text || thumbnail.is_some()).then(|| ui::row().items_start().gap_4().child(text).children(thumbnail));
    // Without inline link media the card keeps its text and thumbnail but not its big pictures.
    let media = ctx.media.links.then(|| embed_media(embed, row, &key, inner, ctx, cx)).flatten();
    let stamp = embed.timestamp.as_deref().map(|t| (format_stamp(t), day(t).map(|d| format!("{}, {}", d.format("%A, %B %-d, %Y"), clock(t))).unwrap_or_default()));
    let footer = (embed.footer.is_some() || stamp.is_some()).then(|| {
        let line = embed.footer.clone().into_iter().chain(stamp.as_ref().map(|(short, _)| short.clone())).collect::<Vec<_>>().join(" · ");
        ui::row()
            .id(SharedString::from(format!("{key}-footer")))
            .gap_2()
            .text_size(ui::TEXT_XS)
            .text_color(faint)
            .children(embed.footer_icon.clone().map(|icon| ui::avatar(icon, 20.)))
            .child(div().min_w_0().child(line))
            .when_some(stamp.map(|(_, full)| full), |d, full| d.tooltip(move |window, cx| ui::tip(full.clone(), window, cx)))
    });
    let sections = [top.map(IntoElement::into_any_element), media, footer.map(IntoElement::into_any_element)];
    let full = !embed.images.is_empty() || !embed.fields.is_empty();
    // The bar is the card's own left 4px, following its squircle like a CSS inset shadow: a strip as wide as
    // both left corners, cut to the card's corner shape, painted bar-coloured to 4px and clear past it.
    let cut = EMBED_BAR / EMBED_BAR_STRIP;
    let strip = ui::edge(div(), Corners { top_left: px(RADIUS_LG - 1.), bottom_left: px(RADIUS_LG - 1.), top_right: Pixels::ZERO, bottom_right: Pixels::ZERO })
        .absolute()
        .left_0()
        .top_0()
        .bottom_0()
        .w(px(EMBED_BAR_STRIP))
        .bg(linear_gradient(90., linear_color_stop(bar, cut), linear_color_stop(bar.opacity(0.), cut + 0.02)));
    let card = ui::panel(cx)
        .relative()
        .when(full, |d| d.w(px(width)))
        .when(!full, |d| d.self_start().min_w(px(EMBED_MIN.min(width))))
        .max_w(px(width))
        .pt(px(EMBED_PAD.0))
        .pr(px(EMBED_PAD.1))
        .pb(px(EMBED_PAD.2))
        .pl(px(EMBED_PAD.3 + EMBED_BAR))
        .gap(px(EMBED_GAP))
        .bg(fill)
        .text_size(ui::TEXT_SM)
        .line_height(gpui_kit::relative(1.45))
        .child(strip)
        .children(sections.into_iter().flatten())
        .children(row.filter(|m| !embed.inline() && remove_embeds_shown(m, ctx)).map(|m| remove_embeds_button(m, &key, ctx, cx)));
    div().pt(px(ACCESSORY_GAP)).child(card).into_any_element()
}

// While the row is hovered or keyboard-focused.
pub(super) fn remove_embeds_shown(message: &Message, ctx: &RowContext) -> bool {
    let active = ctx.hovered.as_deref() == Some(message.id.as_str()) || ctx.focused.as_deref() == Some(message.id.as_str());
    active && message.state == SendState::Sent && can_remove_embeds(message, ctx.me.as_ref().map(|me| me.id.as_str()), ctx.manage_messages)
}

// Occludes so the title or thumbnail under it stays still, and reports hover so the row keeps it.
pub(super) fn remove_embeds_button(message: &Message, key: &str, ctx: &RowContext, cx: &App) -> AnyElement {
    let (this, m) = (ctx.this.clone(), message.clone());
    ui::icon_button(SharedString::from(format!("{key}-remove")), Glyph::X, "Remove all embeds", cx)
        .small()
        .absolute()
        .top_2()
        .right_2()
        .occlude()
        .on_hover(track_message_hover(message, ctx))
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            let quick = window.modifiers().shift;
            this.update(cx, |this, cx| this.remove_embeds(&m, quick, cx)).ok();
        })
        .into_any_element()
}

pub(super) const EMBED_WIDTH: f32 = 516.;
pub(super) const EMBED_MIN: f32 = 240.;
// Discord's embed padding, top, right, bottom, left (its 2px grid padding plus the first section's 8px margin
// on top), and its 8px between sections, in the feed's Spacious proportions rounded to even pixels.
pub(super) const EMBED_PAD: (f32, f32, f32, f32) = (12., 20., 20., 16.);
pub(super) const EMBED_GAP: f32 = 12.;
pub(super) const EMBED_BAR: f32 = 4.;
// Wide enough to hold both left corners, so the bar's shape is the card's.
pub(super) const EMBED_BAR_STRIP: f32 = 2. * RADIUS_LG;
pub(super) const EMBED_THUMB: f32 = 80.;
pub(super) const EMBED_MEDIA_MAX_H: f32 = 360.;
// Discord's box for a portrait video still (YouTube Shorts, TikTok): width, height.
pub(super) const EMBED_PORTRAIT: (f32, f32) = (400., 450.);
// About twelve lines of 14px text: a longer description folds under a fade.
pub(super) const EMBED_CLIP: f32 = 252.;

// Twelve lines at roughly seventy characters each, the card's measure; gpui can't measure the text first.
pub(super) fn long_description(text: &str) -> bool {
    text.lines().map(|line| line.chars().count().div_ceil(70).max(1)).sum::<usize>() > 12
}

// Indexes of the fields on each row: consecutive inline fields share one, three across, two when a
// thumbnail takes the right edge; a field that isn't inline takes a row of its own.
pub(super) fn field_rows(fields: &[(String, String, bool)], thumbnail: bool) -> Vec<Vec<usize>> {
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
pub(super) fn embed_media(embed: &Embed, row: Option<&Message>, key: &str, inner: f32, ctx: &RowContext, cx: &App) -> Option<AnyElement> {
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
                d.child(ui::row().absolute().inset_0().justify_center().child(
                    ui::row().size(px(PLAY_DISC)).rounded_full().justify_center().bg(on_media(0.62)).text_color(gpui_kit::white()).child(Icon::new(Glyph::Play).size(px(PLAY_ICON))),
                ))
                .children(embed.url.clone().map(|url| open_link_button(url, row, &format!("{key}-open-{i}"), ctx, cx)))
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
    Some(div().relative().w(px(width)).h(px(height)).flex_none().child(body).when(veiled, |d| d.child(media_veil(veil, "Sensitive content · Show", ui::all(RADIUS_SM), ctx))).into_any_element())
}

// Discord's Open Link corner on a video still. In a feed row it reports hover, since occluding takes it from the row.
pub(super) fn open_link_button(url: String, row: Option<&Message>, id: &str, ctx: &RowContext, cx: &App) -> AnyElement {
    let this = ctx.this.clone();
    ui::icon_button(SharedString::from(id.to_owned()), Glyph::Open, "Open Link", cx)
        .tone(ui::Tone::OnMedia, cx)
        .sized(OPEN_LINK_SIZE)
        .absolute()
        .top_2()
        .right_2()
        .occlude()
        .when_some(row, |d, m| d.on_hover(track_message_hover(m, ctx)))
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            this.update(cx, |this, cx| this.open_link(&url, window, cx)).ok();
        })
        .into_any_element()
}

// A closed poll's result: the winner, its share, and the way back to the poll.
pub(super) fn poll_result(poll: &crate::discord::PollResult, poll_message: Option<&str>, key: &str, ctx: &RowContext, cx: &App) -> AnyElement {
    let share = (poll.winner_votes * 100).checked_div(poll.total_votes).unwrap_or(0);
    let votes = if poll.winner_votes == 1 { "vote" } else { "votes" };
    let (emoji, headline, line) = match &poll.winner {
        Some((text, emoji)) => (emoji.clone(), text.clone(), format!("Won with {} {votes} · {share}%", poll.winner_votes)),
        None => (None, "No winner".to_owned(), (if poll.total_votes == 0 { "Nobody voted" } else { "Tied at the top" }).to_owned()),
    };
    let mark = match emoji {
        Some(e) => match e.url() {
            Some(url) => img(url).size_7().into_any_element(),
            None => div().text_size(ui::TEXT_XL).line_height(ui::LINE_XL).child(e.name).into_any_element(),
        },
        None => div().text_size(ui::TEXT_XL).line_height(ui::LINE_XL).child("📊").into_any_element(),
    };
    let (this, target) = (ctx.this.clone(), poll_message.map(str::to_owned));
    let card = ui::row()
        .w_full()
        .max_w(px(EMBED_WIDTH.min(ctx.column)))
        .py_3()
        .pl_3p5()
        .pr_3()
        .gap_3()
        .squircle(RADIUS_LG)
        .bg(palette::surface(Level::Sidebar, cx))
        .border_1()
        .border_color(palette::line(cx))
        .child(mark)
        .child(
            ui::col().min_h_0()
                .flex_1()
                .child(ui::line(headline).text_size(ui::TEXT_MD).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)))
                .child(
                    ui::row()
                        .gap_1()
                        .text_size(ui::TEXT_XS)
                        .text_color(palette::muted(cx))
                        .when(poll.winner.is_some(), |d| d.child(Icon::new(Glyph::CheckCircle).size(ui::ICON_SM).text_color(palette::positive_ink(cx))))
                        .child(line),
                ),
        )
        .when_some(target, |d, id| {
            d.child(ui::button(SharedString::from(format!("{key}-view-poll")), ui::Tone::Secondary, cx).small().child("View Poll").on_click(move |_, _, cx| {
                cx.stop_propagation();
                this.update(cx, |this, cx| this.jump_to(id.clone(), cx)).ok();
            }))
        });
    div().pt(px(ACCESSORY_GAP)).child(card).into_any_element()
}

// A live poll (design/messages.html `.poll`): pick and Vote, or its results once you've voted, peeked or it closed.
pub(super) fn poll_card(message: &Message, poll: &Poll, ctx: &RowContext, cx: &App) -> AnyElement {
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
    // lint-allow(raw-margin): pushes the buttons to the end of a wrapping footer, where flex_1 would take the footer's text space
    let actions = ui::row().items_stretch().ml_auto().gap_2().when(!pending && !closed, |d| match (voted, results) {
        (true, _) => d.child(ui::button(key("remove"), ui::Tone::Ghost, cx).small().child("Remove Vote").on_click(act(Vec::new()))),
        (false, true) => d.child(ui::button(key("back"), ui::Tone::Ghost, cx).small().child("Go back to vote").on_click(peek)),
        (false, false) => d
            .child(ui::button(key("peek"), ui::Tone::Ghost, cx).small().child("Show results").on_click(peek))
            .child(ui::button(key("vote"), ui::Tone::Primary, cx).small().disabled(picks.is_empty()).child("Vote").on_click(act(picks.clone()))),
    });
    let card = ui::col()
        .w_full()
        .max_w(px(COLUMN.min(ctx.column)))
        .p_3p5()
        .gap_3()
        .squircle(RADIUS_LG)
        .bg(palette::surface(Level::Sidebar, cx))
        .border_1()
        .border_color(palette::line(cx))
        .when(pending, |d| d.opacity(0.6))
        .child(
            ui::col()
                .gap_1()
                .child(div().text_size(ui::TEXT_MD).line_height(ui::LINE_SM).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(poll.question.clone()))
                .child(div().text_size(ui::TEXT_XS).text_color(palette::faint(cx)).child(poll.prompt(now))),
        )
        .child(ui::col().gap_1p5().children(options))
        .child(ui::row().flex_wrap().gap_2().text_size(ui::TEXT_XS).text_color(palette::faint(cx)).child(poll.foot(now)).child(actions));
    div().pt(px(ACCESSORY_GAP)).child(card).into_any_element()
}

// An answer to pick before voting: its radio (a check where several may be picked), emoji and text.
pub(super) fn poll_choice(message: &Message, poll: &Poll, answer: &PollAnswer, on: bool, pending: bool, ctx: &RowContext, cx: &App) -> AnyElement {
    let mark = if poll.multiselect { ui::check(if on { ui::Check::On } else { ui::Check::Off }, cx) } else { ui::radio(on, cx) };
    let lead = ui::row().flex_none().gap_2p5().child(mark).children(answer.emoji.as_ref().map(|e| crate::menu::emoji_face(e, 20.)));
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
pub(super) fn poll_share(answer: &PollAnswer, share: u32, cx: &App) -> AnyElement {
    let strong = palette::strong(cx);
    let bar = if answer.me { palette::accent_soft(cx) } else { strong.opacity(0.10) };
    // lint-allow(custom-badge): ui::badge is a small solid label; this is a result row with its share drawn as a fill behind the text
    ui::row()
        .relative()
        .min_h_11()
        .px_3()
        .py_2()
        .gap_2p5()
        .squircle(RADIUS_MD)
        .bg(palette::hover(cx))
        .text_size(ui::TEXT_SM)
        .font_weight(FontWeight::MEDIUM)
        .text_color(if answer.me { strong } else { palette::text(cx) })
        // lint-allow(custom-mark): ui::progress_bar draws a thin track; this fill runs behind the row's text, as wide as the share
        .when(share > 0, |d| d.child(div().absolute().left_0().top_0().bottom_0().w(gpui_kit::relative(share as f32 / 100.)).squircle(RADIUS_MD).bg(bar)))
        .children(answer.emoji.as_ref().map(|e| div().flex_none().child(crate::menu::emoji_face(e, 20.))))
        .child(div().flex_1().min_w_0().child(answer.text.clone()))
        .when(answer.me, |d| d.child(Icon::new(Glyph::CheckCircle).size(ui::ICON_LG).flex_none().text_color(palette::accent_ink(cx))))
        .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).text_color(strong).child(format!("{share}%")))
        .into_any_element()
}
