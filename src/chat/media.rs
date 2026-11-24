use super::*;

// The blur over a spoiler picture, over a veiled one, and the alt popout's width.
const SPOILER_BLUR: f32 = 24.;
const VEIL_BLUR: f32 = 28.;
const ALT_POPOUT_WIDTH: f32 = 260.;

// gpui only advances an animated image's frames when the img has an id to keep its frame state under,
// so every img that may hold a GIF (or animated WebP) gets one.
pub(crate) fn animated_image(source: impl Into<gpui_kit::ImageSource>) -> Stateful<gpui_kit::Img> {
    img(source).id("animated")
}

// An image that animates only while `animate`: without an id gpui holds its first frame (see `animated_image`).
pub(crate) fn moving(image: gpui_kit::Img, animate: bool) -> AnyElement {
    if animate { image.id("animated").into_any_element() } else { image.into_any_element() }
}

// The media rows of Appearance and Accessibility (docs/discord-settings-re.md). Reduced motion overrides
// GIFs and emoji to off and stickers to on interaction, as Discord's settings override does.
#[derive(Clone, Copy)]
pub(crate) struct MediaPrefs {
    // "Show images, videos, and lolcats": uploaded to Discord, and posted as links.
    pub(super) attachments: bool,
    pub(super) links: bool,
    pub(super) gifs: bool,
    pub(super) emoji: bool,
    pub(crate) stickers: crate::stickers::StickerMotion,
    pub(super) spoilers: crate::settings::Spoilers,
}

impl Waffle {
    pub(crate) fn media_prefs(&self) -> MediaPrefs {
        let (account, still) = (&self.account_settings, self.prefs.reduces_motion());
        MediaPrefs {
            attachments: account.inline_attachment_media,
            links: account.inline_embed_media,
            gifs: account.gif_auto_play && !still,
            emoji: account.animate_emoji && !still,
            stickers: crate::stickers::StickerMotion::of(account.animate_stickers, still),
            spoilers: self.prefs.spoilers,
        }
    }
}

impl RowContext {
    // Whether the spoiler under `key` (a message id for its text, a media key for an upload) shows: the
    // setting reveals them all, or only where you moderate, else a click on it does.
    pub(super) fn spoilers_revealed(&self, key: &str) -> bool {
        let shown = match self.media.spoilers {
            crate::settings::Spoilers::Always => true,
            crate::settings::Spoilers::IfModerator => self.manage_messages,
            crate::settings::Spoilers::OnClick => false,
        };
        shown || self.revealed.borrow().contains(key)
    }

    // Hover on the message row stands in for hover on its media: what plays a still GIF or sticker.
    pub(super) fn hovers(&self, message: &Message) -> bool {
        self.hovered.as_deref() == Some(message.id.as_str())
    }
}

// The emoji as the setting draws it: an animated one's still picture while animation is off.
pub(super) fn shown_emoji(emoji: &Emoji, animate: bool) -> Emoji {
    Emoji { animated: emoji.animated && animate, ..emoji.clone() }
}

// What a tile's click pages through in the viewer: the feed's media, or a Components V2 gallery's own
// items.
#[derive(Clone, Copy)]
pub(crate) enum Paging {
    Feed,
    Gallery,
}

// The message's media, then its uploads as file rows where the setting keeps them out of line.
pub(super) fn media_grid(message: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> Option<AnyElement> {
    let gallery = media_layout(message, &inline_media(message, &ctx.media), ctx.column.min(COLUMN), Paging::Feed, ctx, cx).map(|g| div().pt(px(ACCESSORY_GAP)).child(g).into_any_element());
    if ctx.media.attachments || message.media_files.is_empty() {
        return gallery;
    }
    Some(ui::col().children(gallery).children(message.media_files.iter().map(|f| file_card(message, f, cx))).into_any_element())
}

// Indexes into `message.media` drawn inline: uploads lead it (see `Message::media_files`), link media follows.
pub(super) fn inline_media(message: &Message, prefs: &MediaPrefs) -> Vec<usize> {
    let uploads = message.media_files.len();
    (0..message.media.len()).filter(|&i| if i < uploads { prefs.attachments } else { prefs.links }).collect()
}

// Media at its own shape, the items `shown` (indexes into `message.media`): one alone fits 400×320 in
// the feed and spans a gallery's `width` up to 360 tall; several tile `width` (2 side by side, 3 one tall
// and two stacked, 4 two by two, 5 or more two over three with the last counting the rest).
// With Show image descriptions on, a lone picture's alt text runs under it as a caption; in a gallery
// each described picture gets an ALT button that opens it.
pub(crate) fn media_layout(message: &Message, shown: &[usize], width: f32, paging: Paging, ctx: &Rc<RowContext>, cx: &mut App) -> Option<Div> {
    let first = &message.media[*shown.first()?];
    if shown.len() == 1 {
        let scale = match paging {
            Paging::Feed => (MAX_IMAGE.0.min(width) / first.width).min(MAX_IMAGE.1 / first.height).min(1.),
            Paging::Gallery => (width / first.width).min(GALLERY_MAX_H / first.height),
        };
        let (w, h) = (first.width * scale, first.height * scale);
        let caption = shown_alt(message, shown[0], ctx).map(|alt| div().max_w(px(w)).text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(palette::muted(cx)).child(alt));
        let frame = media_frame(message, shown[0], (w, h), ui::all(RADIUS_LG), paging, ctx, cx).flex_none().w(px(w)).h(px(h));
        return Some(ui::col().gap_1().child(frame).children(caption));
    }
    // Laid out for the feed's column, then scaled to `width`.
    let k = width / COLUMN;
    let (height, tiles) = gallery_layout(shown.len());
    let height = height * k;
    let tiles: Vec<Tile> = tiles.into_iter().map(|t| Tile { x: t.x * k, y: t.y * k, w: t.w * k, h: t.h * k }).collect();
    let rest = shown.len() - tiles.len();
    let mut grid = div().relative().flex_none().w(px(width)).h(px(height));
    let mut popout = None;
    for (i, tile) in tiles.iter().enumerate() {
        let corners = gallery_corners(*tile, width, height, RADIUS_LG);
        let counts_rest = rest > 0 && i + 1 == tiles.len();
        let item = shown[i];
        let alt = shown_alt(message, item, ctx).filter(|_| !counts_rest && !matches!(message.media[item].kind, MediaKind::Video { .. }));
        let key = format!("{}#{item}", message.id);
        if let Some(text) = alt.clone().filter(|_| ctx.alt_popout.as_deref() == Some(key.as_str())) {
            // Over the gallery, rising from the tile's ALT button; drawn last so later tiles don't cover it.
            popout = Some(alt_popout(&key, text, ctx, cx).left(px(tile.x + 6.)).bottom(px(height - tile.y - tile.h + 34.)));
        }
        grid = grid.child(
            media_frame(message, item, (tile.w, tile.h), corners, paging, ctx, cx)
                .absolute()
                .left(px(tile.x))
                .top(px(tile.y))
                .w(px(tile.w))
                .h(px(tile.h))
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
                            .text_size(ui::TEXT_LG)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("+{rest}")),
                    )
                })
                .when(alt.is_some(), |t| t.child(alt_button(key, ctx))),
        );
    }
    Some(grid.children(popout))
}

// A gallery tile or lone item with what floats over it: on your own message, Modify Attachment while
// it's hovered (pictures and videos you uploaded, not GIF embeds).
pub(super) fn media_frame(message: &Message, i: usize, size: (f32, f32), corners: Corners<Pixels>, paging: Paging, ctx: &Rc<RowContext>, cx: &mut App) -> Stateful<Div> {
    let item = &message.media[i];
    let key = format!("media-{}-{i}", message.id);
    let mine = ctx.me.as_ref().is_some_and(|me| me.id == message.author.id) && message.state == SendState::Sent;
    let editable = mine && item.id.is_some() && !matches!(item.kind, MediaKind::Gif { .. });
    let hovered = ctx.hovered_tile.as_deref() == Some(key.as_str());
    let modify = (editable && hovered).then(|| {
        let (this, msg) = (ctx.this.clone(), message.clone());
        floating_bar(SharedString::from(format!("media-actions-{}-{i}", message.id)), cx)
            .top_1p5()
            .right_1p5()
            .on_hover(track_tile_hover(ctx.this.clone(), key.clone()))
            .child(ui::icon_button(SharedString::from(format!("modify-media-{}-{i}", message.id)), Glyph::Pen, "Modify Attachment", cx).on_click(move |_, window, cx| {
                cx.stop_propagation();
                this.update(cx, |this, cx| this.open_modify_sent(msg.clone(), i, window, cx)).ok();
            }))
    });
    div()
        .id(SharedString::from(format!("media-frame-{}-{i}", message.id)))
        .relative()
        .when(editable, |f| f.on_hover(track_tile_hover(ctx.this.clone(), key)))
        .child(media_tile(message, i, size, corners, paging, ctx, cx))
        .children(modify)
}

// The alt text to show for item `i`: the setting is on, there is some, and it isn't a hidden spoiler.
pub(super) fn shown_alt(message: &Message, i: usize, ctx: &RowContext) -> Option<String> {
    let item = message.media.get(i)?;
    let hidden = item.spoiler && !ctx.spoilers_revealed(&spoiler_key(message, i));
    item.description.clone().filter(|alt| ctx.image_descriptions && !alt.is_empty() && !hidden)
}

pub(super) fn spoiler_key(message: &Message, i: usize) -> String {
    format!("{}#media-{i}", message.id)
}

// Discord's ALT button at a gallery tile's bottom left; it opens and closes the description's popout.
pub(super) fn alt_button(key: String, ctx: &RowContext) -> Stateful<Div> {
    let open = ctx.alt_popout.as_deref() == Some(key.as_str());
    let this = ctx.this.clone();
    ui::media_label()
        .child("ALT")
        .id(SharedString::from(format!("alt-{key}")))
        .occlude()
        .absolute()
        .left_1p5()
        .bottom_1p5()
        .cursor_pointer()
        .tooltip(|window, cx| ui::tip("Image Description (Alt Text)", window, cx))
        // On press, judged by the state it was drawn with: the popout's own press-outside close fires too.
        .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            let next = (!open).then(|| key.clone());
            this.update(cx, |this, cx| {
                this.alt_popout = next;
                cx.notify();
            })
            .ok();
        })
}

// "Image Description (Alt Text)" over the gallery; a press anywhere else closes it.
pub(super) fn alt_popout(key: &str, text: String, ctx: &RowContext, cx: &App) -> Stateful<Div> {
    let (this, key) = (ctx.this.clone(), key.to_owned());
    // lint-allow(custom-surface): ui::surface sits in the flow; this popout hangs absolutely over a gallery tile and sizes itself
    ui::col()
        .id(SharedString::from(format!("alt-popout-{key}")))
        .occlude()
        .absolute()
        .w(px(ALT_POPOUT_WIDTH))
        .px_3p5()
        .py_3()
        .gap_1()
        .squircle(RADIUS_MD)
        .bg(palette::surface(Level::Overlay, cx))
        .shadow(ui::shadow_pop(cx))
        .child(div().text_size(ui::TEXT_XS).font_weight(FontWeight::SEMIBOLD).text_color(palette::muted(cx)).child("Image Description (Alt Text)"))
        .child(div().text_size(ui::TEXT_SM).line_height(gpui_kit::relative(1.45)).text_color(palette::text(cx)).child(text))
        .on_mouse_down_out(move |_, _, cx| {
            this.update(cx, |this, cx| {
                if this.alt_popout.as_deref() == Some(key.as_str()) {
                    this.alt_popout = None;
                    cx.notify();
                }
            })
            .ok();
        })
}

// One picture, video or GIF, rounded to `corners`; a click opens the viewer at it. A spoiler stays
// veiled until clicked.
pub(super) fn media_tile(message: &Message, i: usize, (w, h): (f32, f32), corners: Corners<Pixels>, paging: Paging, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    let item = &message.media[i];
    let cover = (item.spoiler && !ctx.spoilers_revealed(&spoiler_key(message, i))).then(|| reveal_veil(message, i, corners, ctx));
    // A video plays where it is; clicks on it belong to its player.
    if let MediaKind::Video { poster } = &item.kind {
        return div().relative().size_full().child(crate::video::tile(format!("{}-{i}", message.id), &item.url, poster, cx)).children(cover).into_any_element();
    }
    // Not verified: with autoplay off Discord plays a GIF while it's hovered. GAP: Waffle plays it while
    // its message is hovered, the only hover state rows keep.
    let plays = ctx.media.gifs || ctx.hovers(message);
    // Fetched at the size it's drawn rather than full resolution; the viewer still opens the original.
    let sized = |url: &str| crate::discord::sized_image(url, w, h, (item.width, item.height));
    let picture = match &item.kind {
        MediaKind::Gif { poster } => div()
            .relative()
            .size_full()
            .child(crate::gif::rounded_tile(format!("{}-{i}-{}", message.id, item.url), &item.url, &sized(poster), gpui_kit::size(px(w), px(h)), corners, plays, cx))
            .child(media_tag("GIF").top_2().left_2())
            .into_any_element(),
        _ => div()
            .relative()
            .size_full()
            .child(moving(ui::edge(img(sized(&item.url)), corners).size_full().object_fit(gpui_kit::ObjectFit::Cover), plays))
            .when(!plays && is_gif(&item.url), |d| d.child(media_tag("GIF").top_2().left_2()))
            .into_any_element(),
    };
    let (this, url) = (ctx.this.clone(), item.url.clone());
    let own = matches!(paging, Paging::Gallery).then(|| message.media.clone());
    let (menu_this, menu_message, menu_url) = (ctx.this.clone(), message.clone(), item.url.clone());
    ui::edge(div(), corners)
        .id(SharedString::from(format!("media-{}-{i}", message.id)))
        .relative()
        .size_full()
        // A cover-fit picture whose real shape differs from the one Discord reported spills past the tile.
        .overflow_hidden()
        .bg(palette::selected(cx))
        .cursor_pointer()
        .hover(|s| s.opacity(0.92))
        .child(picture)
        .children(cover)
        // The message's menu with Discord's image items; the row's own handler would open it without them.
        .on_mouse_down(gpui_kit::MouseButton::Right, move |event, _, cx| {
            cx.stop_propagation();
            let target = Some(crate::message_menu::MenuTarget::Image(menu_url.clone()));
            menu_this.update(cx, |this, cx| this.open_message_menu_on(menu_message.clone(), target, event.position, cx)).ok();
        })
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            this.update(cx, |this, cx| match own.clone() {
                Some(items) => this.open_viewer_on(items, i, String::new(), window, cx),
                None => this.open_viewer(&url, window, cx),
            })
            .ok();
        })
        .into_any_element()
}

// `media_veil` until `key` is revealed; it occludes, so nothing under it answers the pointer while covered.
pub(crate) fn veil(key: String, label: &'static str, corners: Corners<Pixels>, ctx: &RowContext) -> Option<Stateful<Div>> {
    let covered = !ctx.revealed.borrow().contains(&key);
    covered.then(|| media_veil(key, label, corners, ctx))
}

pub(super) fn is_gif(url: &str) -> bool {
    url.split(['?', '#']).next().is_some_and(|path| path.to_ascii_lowercase().ends_with(".gif"))
}

// A blur over media until it's clicked (a spoiler, sensitive content); the click adds `key` to the
// revealed set.
pub(super) fn media_veil(key: String, label: &'static str, corners: Corners<Pixels>, ctx: &RowContext) -> Stateful<Div> {
    let (reveal, refresh) = (ctx.revealed.clone(), ctx.this.clone());
    ui::edge(div(), corners)
        .id(SharedString::from(format!("veil-{key}")))
        .occlude()
        .absolute()
        .inset_0()
        .bg(on_media(0.35))
        .backdrop_blur(px(VEIL_BLUR))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            reveal.borrow_mut().insert(key.clone());
            refresh.update(cx, |_, cx| cx.notify()).ok();
        })
        .child(ui::media_label().h_8().px_3().gap_1p5().squircle(RADIUS_MD).text_size(ui::TEXT_SM).child(Icon::new(Glyph::EyeSlash).size(ui::ICON_MD)).child(label))
}

// A spoiler's cover: blurred and dimmed, labelled, and a click lifts it (and nothing else).
pub(super) fn reveal_veil(message: &Message, i: usize, corners: Corners<Pixels>, ctx: &RowContext) -> AnyElement {
    let (reveal, key, this) = (ctx.revealed.clone(), spoiler_key(message, i), ctx.this.clone());
    let (channel_id, id) = (message.channel_id.clone(), message.id.clone());
    ui::edge(spoiler_veil(), corners)
        .id(SharedString::from(format!("veil-{key}")))
        .occlude()
        .cursor_pointer()
        .child(media_label("SPOILER").px_2p5().h_7().text_size(ui::TEXT_XS))
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            reveal.borrow_mut().insert(key.clone());
            // Its alt caption may appear, which changes the row's height.
            this.update(cx, |this, cx| {
                this.message_mut(&channel_id, &id);
                cx.notify();
            })
            .ok();
        })
        .into_any_element()
}

// What covers a spoiler picture: a blur and a dim over it.
pub(super) fn spoiler_veil() -> Div {
    ui::row().absolute().inset_0().justify_center().bg(on_media(0.3)).backdrop_blur(px(SPOILER_BLUR))
}

// A label over media (GIF, a duration): fixed dark values, since it sits on pictures in both themes.
pub(super) fn media_tag(text: &'static str) -> Div {
    media_label(text).absolute()
}

// The same label placed by its parent (a row of tags).
pub(super) fn media_label(text: &'static str) -> Div {
    ui::media_label().child(text)
}

// A small bar of icon buttons floating over a picture's corner: it takes the pointer, so its caller
// keeps the hover with the bar's own `on_hover`.
pub(super) fn floating_bar(id: SharedString, cx: &App) -> Stateful<Div> {
    ui::toolbar(id, cx).items_stretch().absolute()
}

pub(super) fn track_tile_hover(this: WeakEntity<Waffle>, key: String) -> impl Fn(&bool, &mut Window, &mut App) + 'static {
    move |hovered, window, cx| {
        // A tile and its bar both report; as with message rows, leaves apply now and enters after the event.
        if *hovered {
            let (this, key) = (this.clone(), key.clone());
            window.defer(cx, move |_, cx| set_hovered_tile(&this, Some(key), cx));
            return;
        }
        let leaving = this.read_with(cx, |this, _| this.hovered_tile.as_ref() == Some(&key)).unwrap_or(false);
        if leaving {
            set_hovered_tile(&this, None, cx);
        }
    }
}

pub(super) fn set_hovered_tile(this: &WeakEntity<Waffle>, key: Option<String>, cx: &mut App) {
    this.update(cx, |this, cx| {
        if this.hovered_tile != key {
            this.hovered_tile = key;
            cx.notify();
        }
    })
    .ok();
}

// The design's media scrim, oklch(0.14 0.004 260), at `alpha`.
pub(super) fn on_media(alpha: f32) -> Hsla {
    palette::media_scrim().opacity(alpha)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Tile {
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) w: f32,
    pub(super) h: f32,
}

// Discord takes ten attachments; a "+N" tile shows only past that.
const GALLERY_MAX: usize = 10;

// The gallery's height and its tiles for `count` items in the media column, as Discord's mosaic.
pub(super) fn gallery_layout(count: usize) -> (f32, Vec<Tile>) {
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
        // Discord's mosaic past four: a lead row (one wide or two halves, or none) over rows of three squares, up to ten.
        n => {
            let n = n.min(GALLERY_MAX);
            let lead = [0, 2, 0, 1, 2, 0, 1][(n - 4).min(6)];
            let third = (COLUMN - 2. * GALLERY_GAP) / 3.;
            let lead_h = if lead == 1 { COLUMN * 9. / 16. } else { half };
            let mut tiles: Vec<Tile> = (0..lead).map(|i| tile(i as f32 * (half + GALLERY_GAP), 0., if lead == 1 { COLUMN } else { half }, lead_h)).collect();
            let top = if lead > 0 { lead_h + GALLERY_GAP } else { 0. };
            tiles.extend((0..n - lead).map(|i| tile((i % 3) as f32 * (third + GALLERY_GAP), top + (i / 3) as f32 * (third + GALLERY_GAP), third, third)));
            (top + ((n - lead) / 3) as f32 * (third + GALLERY_GAP) - GALLERY_GAP, tiles)
        }
    }
}

// The gallery's outer corners take the card radius; inner ones a hairline 2px.
pub(super) fn gallery_corners(tile: Tile, width: f32, height: f32, outer: f32) -> Corners<Pixels> {
    let at = |a: f32, b: f32| (a - b).abs() < 0.5;
    let (left, top, right, bottom) = (at(tile.x, 0.), at(tile.y, 0.), at(tile.x + tile.w, width), at(tile.y + tile.h, height));
    let radius = |edge: bool| px(if edge { outer } else { 2. });
    Corners { top_left: radius(top && left), top_right: radius(top && right), bottom_left: radius(bottom && left), bottom_right: radius(bottom && right) }
}
