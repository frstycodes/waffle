// Each kind of component drawn: a premium button's price, select fields, sections, galleries, files and containers.
use crate::ui;

use super::*;

// The SKU as fetched so far; drawing a button the first time asks for it.
pub(super) fn sku_state(sku_id: &str, ctx: &RowContext, cx: &mut App) -> Sku {
    if let Some(sku) = ctx.components.skus.borrow().get(sku_id) {
        return sku.clone();
    }
    let (this, id) = (ctx.this(), sku_id.to_owned());
    cx.defer(move |cx| {
        this.update(cx, |this, cx| this.load_sku(id, cx)).ok();
    });
    Sku::Loading
}

// GET /applications/storefront/interactions/premium-button/{sku}, read as Discord's premium button
// does: an unavailable SKU, a subscription with no plan, or a SKU with no price is "Product
// Unavailable"; otherwise "{name} - {price}" (a subscription priced by its first plan).
// Not verified: the response's field shapes (`skus[].price.{amount,currency}`, `available`,
// `subscription_plans[].{price,currency}`); Discord reads them through its stores. Discord also waits
// for the SKU's store listing (and a subscription's parent SKU); Waffle doesn't.
pub(super) fn premium_sku(body: &Value, sku_id: &str) -> Option<Sku> {
    let sku = body["skus"].as_array()?.iter().find(|s| s["id"].as_str() == Some(sku_id))?;
    let name = sku["name"].as_str()?;
    if sku["available"].as_bool() == Some(false) {
        return Some(Sku::Unavailable);
    }
    let price = if sku["type"].as_u64() == Some(SKU_SUBSCRIPTION) {
        body["subscription_plans"].as_array().and_then(|p| p.first()).and_then(|p| Some((p["price"].as_u64()?, p["currency"].as_str()?)))
    } else {
        Some((sku["price"]["amount"].as_u64(), sku["price"]["currency"].as_str())).and_then(|(a, c)| Some((a?, c?)))
    };
    Some(price.map_or(Sku::Unavailable, |(amount, currency)| Sku::Ready(format!("{name} - {}", format_price(amount, currency)))))
}

// An amount in the currency's minor units as en-US formats it.
// Not verified: Discord formats with Intl.NumberFormat; this covers the common symbols and the
// zero-decimal currencies, and falls back to the ISO code.
pub(super) fn format_price(amount: u64, currency: &str) -> String {
    let currency = currency.to_lowercase();
    let decimals = if matches!(currency.as_str(), "jpy" | "krw" | "vnd" | "clp" | "isk") { 0 } else { 2 };
    let value = amount as f64 / 10f64.powi(decimals);
    let number = format!("{value:.0$}", decimals as usize);
    match currency.as_str() {
        "usd" => format!("${number}"),
        "eur" => format!("€{number}"),
        "gbp" => format!("£{number}"),
        "jpy" => format!("¥{number}"),
        other => format!("{} {number}", other.to_uppercase()),
    }
}

// A field with the placeholder or the picks; clicking opens the options as a menu under it.
pub(super) fn select_field(message: &Message, s: &Select, path: &str, ctx: &Rc<RowContext>, cx: &App) -> AnyElement { // lint-allow(custom-component): ui::select lacks a message component's select bound to a message path
    let key = (message.id.clone(), s.custom_id.clone());
    let open = ctx.components.open.as_ref() == Some(&key);
    let busy = ctx.components.calls.get(&key).is_some_and(|c| c.failure.is_none());
    let blocked = !busy && ctx.components.busy.contains(&message.id);
    let defaults = ctx.components.defaults.get(&key).cloned().unwrap_or_else(|| s.options.iter().filter(|o| o.default).cloned().collect());
    let values = ctx.components.picks.get(&key).cloned().unwrap_or_else(|| defaults.iter().map(|o| o.value.clone()).collect());
    let known: Vec<SelectOption> = s.options.iter().cloned().chain(defaults).collect();
    let picked = labels(&values, &known, |v| ctx.name_for_id(v));
    let (this, m, select) = (ctx.this(), message.clone(), s.clone());
    let field = select_trigger(SharedString::from(format!("component-{}-{path}", message.id)), s, picked.clone(), open, cx)
        .disabled(s.disabled || blocked || message.state != SendState::Sent)
        .when(busy, |f| f.loading(Some(picked.unwrap_or_else(|| placeholder(s)))))
        .on_click(move |event, window, cx| {
            cx.stop_propagation();
            this.update(cx, |this, cx| this.open_select(SelectHost::Message(Box::new(m.clone())), select.clone(), event.position(), window, cx)).ok();
        });
    anchored_field(ctx.components.anchors.clone(), key, field.into_any_element())
}

// The select field: its picks, else its placeholder in the faint ink.
pub(super) fn select_trigger(id: SharedString, s: &Select, picked: Option<String>, open: bool, cx: &App) -> ui::Control {
    let empty = picked.is_none();
    ui::select(id, None, picked.unwrap_or_else(|| placeholder(s)), open, cx).when(empty, |f| f.text_color(palette::faint(cx)))
}

pub(super) fn placeholder(s: &Select) -> String {
    s.placeholder.clone().unwrap_or_else(|| "Make a selection".into())
}

// The field with a canvas recording where it painted, so its menu opens under it.
pub(super) fn anchored_field(anchors: Anchors, key: ControlKey, field: AnyElement) -> AnyElement { // lint-allow(custom-component): ui::dropdown lacks an anchor wrapper keyed by a control key
    let canvas = gpui_kit::canvas(
        move |b, _, _| {
            anchors.borrow_mut().insert(key.clone(), b);
        },
        |_, _, _, _| {},
    );
    div().relative().w_full().child(canvas.absolute().size_full()).child(field).into_any_element()
}

// The labels behind `values`: from `known` options, then `fallback`, else the raw id.
pub(super) fn labels(values: &[String], known: &[SelectOption], fallback: impl Fn(&str) -> Option<String>) -> Option<String> {
    let labels: Vec<String> = values.iter().map(|v| known.iter().find(|o| &o.value == v).map(|o| o.label.clone()).or_else(|| fallback(v)).unwrap_or_else(|| v.clone())).collect();
    (!labels.is_empty()).then(|| labels.join(", "))
}

// Text on the left; a thumbnail or a button beside it on the right.
pub(super) fn section(message: &Message, children: &[Component], accessory: Option<&Component>, path: &str, room: Room, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    let text: Vec<AnyElement> = children.iter().enumerate().map(|(i, c)| component(message, c, format!("{path}-{i}"), room, ctx, cx)).collect();
    let failed = failed_line(message, accessory, ctx, cx);
    let accessory = accessory.map(|a| component(message, a, format!("{path}-a"), room, ctx, cx));
    let row = ui::row()
        .items_start()
        .gap_3p5()
        .child(ui::col().min_h_0().flex_1().gap_1().text_color(palette::strong(cx)).children(text))
        .children(accessory.map(|a| div().flex_none().child(a)));
    ui::col().child(row).children(failed).into_any_element()
}

pub(super) fn thumbnail(message: &Message, url: &str, spoiler: bool, size: (Option<u32>, Option<u32>), path: &str, ctx: &Rc<RowContext>, cx: &App) -> AnyElement {
    let (w, h) = thumbnail_size(size.0, size.1);
    let corners = ui::all(RADIUS_MD);
    div()
        .relative()
        .flex_none()
        .w(px(w))
        .h(px(h))
        .child(ui::media(resolve(message, url), corners).size_full().object_fit(gpui_kit::ObjectFit::Cover).bg(palette::selected(cx)))
        .when(spoiler, |d| d.children(chat::veil(format!("thumb-{}-{path}", message.id), "Spoiler", corners, ctx)))
        .into_any_element()
}

// Discord's fit: the media's shape inside 170×170 without growing it, each side at least 85 (the
// picture covers the rest). One without a size takes the 85 square.
// Not verified: the size Discord gives a thumbnail whose media came without one.
pub(super) fn thumbnail_size(width: Option<u32>, height: Option<u32>) -> (f32, f32) {
    let (Some(w), Some(h)) = (width.filter(|w| *w > 0), height.filter(|h| *h > 0)) else { return (THUMBNAIL_MIN, THUMBNAIL_MIN) };
    let (w, h) = (w as f32, h as f32);
    let scale = (THUMBNAIL_MAX / w).min(THUMBNAIL_MAX / h).min(1.);
    ((w * scale).max(THUMBNAIL_MIN), (h * scale).max(THUMBNAIL_MIN))
}

// The feed's media grid over the gallery's items at their own sizes, `width` wide; the viewer pages
// through these alone.
pub(super) fn gallery(message: &Message, items: &[GalleryItem], path: &str, width: f32, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    let media = items
        .iter()
        .map(|i| {
            let (w, h) = match (i.width, i.height) {
                (Some(w), Some(h)) if w > 0 && h > 0 => (w as f32, h as f32),
                _ => GALLERY_ITEM,
            };
            Media { url: resolve(message, &i.url), width: w, height: h, kind: MediaKind::Image, id: None, description: i.description.clone(), spoiler: i.spoiler }
        })
        .collect();
    let grid = Message { id: format!("{}-gallery-{path}", message.id), channel_id: message.channel_id.clone(), media, ..Default::default() };
    let shown: Vec<usize> = (0..grid.media.len()).collect();
    chat::media_layout(&grid, &shown, width, Paging::Gallery, ctx, cx).map_or_else(|| div().into_any_element(), IntoElement::into_any_element)
}

// The file card, sized from the attachment it points at, else from the component's own `size`; a
// spoiler covers it until clicked.
#[allow(clippy::too_many_arguments)]
pub(super) fn file(message: &Message, url: &str, name: Option<&str>, size: Option<u64>, spoiler: bool, path: &str, ctx: &Rc<RowContext>, cx: &App) -> AnyElement {
    let attached = attachment_name(url).and_then(|n| message.files.iter().find(|f| f.name == n)).cloned();
    let file = attached.unwrap_or_else(|| File {
        name: name.map(str::to_owned).or_else(|| url.rsplit('/').next().map(str::to_owned)).unwrap_or_default(),
        url: url.to_owned(),
        size: size.unwrap_or(0),
        spoiler,
        ..Default::default()
    });
    let card = chat::file_card(message, &file, cx);
    if !spoiler {
        return card;
    }
    div().relative().child(card).children(chat::veil(format!("file-{}-{path}", message.id), "Spoiler", ui::all(RADIUS_LG), ctx).map(|v| v.top_2())).into_any_element()
}

pub(super) fn separator(divider: bool, large: bool, cx: &App) -> AnyElement {
    div().py(px(if large { 8. } else { 2. })).when(divider, |d| d.child(div().h(ui::HAIRLINE).bg(palette::line(cx)))).into_any_element() // lint-allow(custom-divider): ui::divider lacks a bare hairline with its own padding that follows a component's size
}

// A raised card at Discord's padding and gap; the app's accent colour becomes a light tint and a tinted
// hairline, never a stripe.
pub(super) fn container(message: &Message, accent: Option<u32>, children: &[Component], path: &str, width: f32, ctx: &Rc<RowContext>, cx: &mut App) -> Div {
    let raised = palette::surface(Level::Raised, cx);
    let (fill, edge) = match accent {
        Some(color) => {
            let tint = palette::data_color(color);
            (ui::mix(raised, tint, 0.07), tint.opacity(0.35))
        }
        None => (raised, palette::line(cx)),
    };
    let room = Room { width: width - 2. * CONTAINER_INSET, reduced: true };
    let children: Vec<AnyElement> = children.iter().enumerate().map(|(i, c)| component(message, c, format!("{path}-{i}"), room, ctx, cx)).collect();
    ui::col().relative().w_full().p(px(CONTAINER_PAD)).gap(px(CONTAINER_GAP)).squircle(RADIUS_LG).bg(fill).border_1().border_color(edge).children(children)
}
