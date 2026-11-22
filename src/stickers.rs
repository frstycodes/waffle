// Stickers (media.html#stickers): drawn bare at 160px in chat, picked from the Stickers tab of the GIF
// picker (recently used, each server's, then Discord's own packs), sent on click as a message of their own.
use gpui_kit::{
    AnyElement, App, Context, Div, Entity, FontWeight, ObjectFit, ScrollHandle, SharedString, StyledImage, Window,
    component::{
        Icon,
        input::{InputEvent, InputState as SearchState},
    },
    div, img,
    prelude::*,
    px, relative,
};

use crate::{
    Waffle,
    discord::{Message, SendState, Sticker},
    icons::Glyph,
    palette,
    ui::{self, RADIUS_LG, RADIUS_MD, RADIUS_SM, RADIUS_XS, Squircle},
};

const IN_CHAT: f32 = 160.;
// Fetched at twice the drawn size so they stay sharp on Retina.
const FETCH: u32 = 320;
// The picker's cells are half that; animated stickers run near 1 MB at 320.
const FETCH_SMALL: u32 = 160;
const RECENT: usize = 8;

// Discord's "Play sticker animations" (`animate_stickers`: 0 always, 1 on interaction, 2 never). Reduced
// motion overrides it to on interaction, whatever it was (docs/discord-settings-re.md).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum StickerMotion {
    Always,
    OnHover,
    Never,
}

impl StickerMotion {
    pub(crate) fn of(setting: u8, reduce_motion: bool) -> Self {
        match setting {
            _ if reduce_motion => Self::OnHover,
            0 => Self::Always,
            2 => Self::Never,
            _ => Self::OnHover,
        }
    }

    pub(crate) fn plays(self, hovered: bool) -> bool {
        match self {
            Self::Always => true,
            Self::OnHover => hovered,
            Self::Never => false,
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) enum PickerTab {
    #[default]
    Gifs,
    Stickers,
}

struct Section {
    lead: Lead,
    name: SharedString,
    stickers: Vec<Sticker>,
}

enum Lead {
    Recent,
    Server { icon: Option<String>, name: String },
    // Discord's own packs: their first sticker stands for them.
    Pack(Sticker),
}

impl Lead {
    fn render(&self, size: f32, motion: StickerMotion, cx: &App) -> AnyElement {
        match self {
            Lead::Recent => Icon::new(Glyph::Recent).size(px(size * 0.75)).into_any_element(),
            Lead::Server { icon, name } => ui::server_tile(icon.as_deref(), name, size, cx),
            Lead::Pack(cover) => picture(cover, size, motion.plays(false)),
        }
    }
}

impl Waffle {
    pub(crate) fn open_stickers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.gif_picker.is_none() {
            self.open_gifs(window, cx);
        }
        self.set_picker_tab(PickerTab::Stickers, window, cx);
    }

    pub(crate) fn set_picker_tab(&mut self, tab: PickerTab, window: &mut Window, cx: &mut Context<Self>) {
        self.picker_tab = tab;
        let search = match tab {
            PickerTab::Gifs => &self.gif_search,
            PickerTab::Stickers => &self.sticker_search,
        };
        search.update(cx, |state, cx| state.focus(window, cx));
        if tab == PickerTab::Stickers && self.sticker_packs.as_ref().is_none_or(|packs| packs.is_err()) {
            self.fetch_sticker_packs(cx);
        }
        cx.notify();
    }

    fn fetch_sticker_packs(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        self.sticker_packs = None;
        cx.spawn(async move |this, cx| {
            let result = client.sticker_packs().await;
            this.update(cx, |this, cx| {
                this.sticker_packs = Some(result.map_err(Into::into));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn on_sticker_search(&mut self, _: &Entity<SearchState>, event: &InputEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let InputEvent::Change = event {
            cx.notify();
        }
    }

    /// Sends a sticker on its own, shown at once and settled when Discord answers.
    pub(crate) fn send_sticker(&mut self, sticker: Sticker, cx: &mut Context<Self>) {
        let (Some(client), Some(channel), Some(me)) = (self.client.clone(), self.channel.clone(), self.me.clone()) else { return };
        self.gif_picker = None;
        self.update_prefs(cx, |p| p.record_sticker(&sticker));
        let nonce = crate::nonce();
        let reply = self.replying.take();
        let ping = self.reply_ping;
        let pending = Message {
            id: format!("pending-{nonce}"),
            channel_id: channel.id.clone(),
            guild_id: self.guild_id.clone(),
            author: me,
            timestamp: chrono::Utc::now().to_rfc3339(),
            reply_to: reply.as_ref().map(|r| r.id.clone()),
            reply: reply.clone().map(Box::new),
            nonce: Some(nonce.clone()),
            state: SendState::Sending,
            stickers: vec![sticker.clone()],
            ..Default::default()
        };
        self.push_local(pending, cx);
        cx.spawn(async move |this, cx| {
            let result = client.send_sticker(&channel.id, &sticker.id, &nonce, reply.as_ref().map(|r| r.id.as_str()), ping).await;
            this.update(cx, |this, cx| this.settle(&channel.id, &nonce, result, cx)).ok();
        })
        .detach();
    }

    // Recently used (before you search), this server's, your other servers' (Nitro only, as Discord allows),
    // then Discord's packs; a search keeps the stickers whose name or tags match.
    fn sticker_sections(&self, query: &str) -> Vec<Section> {
        let query = query.trim();
        let mut sections = Vec::new();
        if query.is_empty() {
            let recent = self.prefs.frequent_stickers(RECENT);
            if !recent.is_empty() {
                sections.push(Section { lead: Lead::Recent, name: "Recently used".into(), stickers: recent });
            }
        }
        let here = self.guild_id.as_deref();
        let mut guilds: Vec<_> = self.guilds.iter().filter(|g| !g.stickers.is_empty() && (Some(g.id.as_str()) == here || self.my_account.premium)).collect();
        guilds.sort_by_key(|g| Some(g.id.as_str()) != here);
        for guild in guilds {
            sections.push(Section { lead: Lead::Server { icon: guild.icon.clone(), name: guild.name.clone() }, name: guild.name.clone().into(), stickers: guild.stickers.clone() });
        }
        for pack in self.sticker_packs.iter().flatten().flatten() {
            sections.push(Section { lead: Lead::Pack(pack.stickers[0].clone()), name: pack.name.clone().into(), stickers: pack.stickers.clone() });
        }
        // GAP: Lottie stickers (most of Discord's own packs) are left out until there's a Lottie player.
        for section in &mut sections {
            section.stickers.retain(|s| s.url(FETCH_SMALL).is_some() && (query.is_empty() || s.matches(query)));
            if let (Lead::Pack(_), Some(first)) = (&section.lead, section.stickers.first()) {
                section.lead = Lead::Pack(first.clone());
            }
        }
        sections.retain(|s| !s.stickers.is_empty());
        sections
    }

    // The Stickers tab: search, then a rail of packs beside the grid, and the hovered sticker named at the foot.
    pub(crate) fn sticker_tab(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let query = self.sticker_search.read(cx).value().to_string();
        let sections = self.sticker_sections(&query);
        let scroll = window.use_keyed_state("sticker-scroll", cx, |_, _| ScrollHandle::new()).read(cx).clone();
        let hovered = window.use_keyed_state("sticker-hovered", cx, |_, _| None::<(Sticker, SharedString)>);
        let motion = self.media_prefs().stickers;
        let preview = hovered.read(cx).clone().or_else(|| sections.first().and_then(|s| Some((s.stickers.first()?.clone(), s.name.clone()))));
        let head = div().p_2p5().child(ui::search_field("sticker-search", &self.sticker_search, cx));
        let loading = self.sticker_packs.is_none() && sections.is_empty();
        let body: AnyElement = match (&self.sticker_packs, sections.is_empty()) {
            _ if loading => note("Loading stickers…", cx),
            (Some(Err(error)), true) => note(error.clone(), cx),
            (_, true) if !query.trim().is_empty() => note(format!("No stickers match \u{201c}{}\u{201d}", query.trim()), cx),
            (_, true) => note("No stickers yet", cx),
            _ => div().flex_1().min_h_0().flex().child(self.sticker_rail(&sections, &scroll, motion, cx)).child(sticker_grid(sections, &scroll, &hovered, motion, cx)).into_any_element(),
        };
        vec![head.into_any_element(), body, sticker_foot(preview, motion, cx).into_any_element()]
    }

    fn sticker_rail(&self, sections: &[Section], scroll: &ScrollHandle, motion: StickerMotion, cx: &mut Context<Self>) -> impl IntoElement {
        let in_view = scroll.top_item();
        div()
            .id("sticker-rail")
            .w_12()
            .flex_none()
            .px_1p5()
            .pb_1p5()
            .flex()
            .flex_col()
            .items_center()
            .gap_0p5()
            .overflow_y_scroll()
            .border_r_1()
            .border_color(palette::line(cx))
            .children(sections.iter().enumerate().flat_map(|(i, section)| {
                let (selected, hover, strong) = (palette::selected(cx), palette::hover(cx), palette::strong(cx));
                let scroll = scroll.clone();
                // A line sets Discord's packs apart from yours.
                let first_pack = matches!(section.lead, Lead::Pack(_)) && i > 0 && !matches!(sections[i - 1].lead, Lead::Pack(_));
                let line = first_pack.then(|| div().w_5().h(ui::HAIRLINE).my_1().flex_none().bg(palette::line(cx)).into_any_element());
                let tab = div()
                    .id(("sticker-pack", i))
                    .size_9()
                    .flex_none()
                    .squircle(RADIUS_SM)
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .text_color(if i == in_view { strong } else { palette::muted(cx) })
                    .when(i == in_view, |d| d.bg(selected))
                    .when(i != in_view, |d| d.hover(move |s| s.bg(hover).text_color(strong)))
                    .child(section.lead.render(24., motion, cx))
                    .tooltip({
                        let name = section.name.clone();
                        move |window, cx| ui::tip(name.clone(), window, cx)
                    })
                    .on_click(cx.listener(move |_, _, _, cx| {
                        scroll.scroll_to_top_of_item(i);
                        cx.notify();
                    }));
                line.into_iter().chain([tab.into_any_element()])
            }))
    }
}

fn sticker_grid(sections: Vec<Section>, scroll: &ScrollHandle, hovered: &Entity<Option<(Sticker, SharedString)>>, motion: StickerMotion, cx: &mut Context<Waffle>) -> impl IntoElement {
    div()
        .id("sticker-scroll")
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_y_scroll()
        .track_scroll(scroll)
        .px_2()
        .pb_2()
        .children(sections.into_iter().map(|section| {
            let mut cells = Vec::with_capacity(section.stickers.len());
            for (i, sticker) in section.stickers.iter().enumerate() {
                cells.push(sticker_cell(sticker, &section.name, format!("sticker-{}-{i}", section.name), hovered, motion, cx));
            }
            div()
                .flex()
                .flex_col()
                .mb_1()
                .child(div().flex().items_center().gap_1p5().py_1p5().px_1().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(palette::muted(cx)).child(section.lead.render(16., motion, cx)).child(section.name.clone()))
                .child(div().flex().flex_wrap().children(cells))
        }))
}

// Four to a row; animated ones carry a small play mark (the design's answer to "nothing says it moves").
fn sticker_cell(sticker: &Sticker, pack: &SharedString, key: String, hovered: &Entity<Option<(Sticker, SharedString)>>, motion: StickerMotion, cx: &mut Context<Waffle>) -> AnyElement {
    let plays = motion.plays(hovered.read(cx).as_ref().is_some_and(|(h, _)| h.id == sticker.id));
    let (picked, shown) = (sticker.clone(), (sticker.clone(), pack.clone()));
    let hovered = hovered.clone();
    let mark = sticker.animated().then(|| {
        div()
            .absolute()
            .right_1()
            .bottom_1()
            .size_5()
            .squircle(RADIUS_XS)
            .flex()
            .items_center()
            .justify_center()
            .bg(palette::surface(palette::Level::Overlay, cx))
            .border_1()
            .border_color(palette::line(cx))
            .text_color(palette::muted(cx))
            .child(Icon::new(Glyph::Play).size(ui::ICON_XS))
    });
    ui::pressable(div().id(SharedString::from(key)), 0.95)
        .relative()
        .w(relative(0.25))
        .h(px(84.))
        .p_2()
        .squircle(RADIUS_MD)
        .cursor_pointer()
        .hover(|s| s.bg(palette::selected(cx)))
        .child(picture(sticker, 68., plays))
        .children(mark)
        .on_hover(move |on: &bool, _, cx| {
            if *on {
                hovered.update(cx, |h, cx| {
                    *h = Some(shown.clone());
                    cx.notify();
                });
            }
        })
        .on_click(cx.listener(move |this, _, _, cx| this.send_sticker(picked.clone(), cx)))
        .into_any_element()
}

fn sticker_foot(preview: Option<(Sticker, SharedString)>, motion: StickerMotion, cx: &App) -> Div {
    div()
        .min_h(px(60.))
        .flex_none()
        .py_1p5()
        .px_3()
        .flex()
        .items_center()
        .gap_2p5()
        .border_t_1()
        .border_color(palette::line(cx))
        .children(preview.map(|(sticker, pack)| {
            let detail = if sticker.animated() { format!("{pack} · Animated") } else { pack.to_string() };
            div()
                .flex()
                .items_center()
                .gap_2p5()
                .min_w_0()
                .child(div().size_11().flex_none().child(picture(&sticker, 44., motion.plays(true))))
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().truncate().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(format!(":{}:", sticker.name)))
                        .child(div().truncate().text_size(ui::TEXT_XS).text_color(palette::muted(cx)).child(detail)),
                )
        }))
}

fn note(text: impl Into<SharedString>, cx: &App) -> AnyElement {
    div().flex_1().p_4().text_color(palette::muted(cx)).child(text.into()).into_any_element()
}

// The sticker's picture at `size`, contained, still unless `animate`; one that fails to load shows the
// sticker mark instead.
fn picture(sticker: &Sticker, size: f32, animate: bool) -> AnyElement {
    let Some(url) = sticker.url(FETCH_SMALL) else { return stand_in(size) };
    let image = img(url).size(px(size)).object_fit(ObjectFit::Contain).with_fallback(move || stand_in(size));
    div().size(px(size)).flex().items_center().justify_center().child(crate::chat::moving(image, animate)).into_any_element()
}

fn stand_in(size: f32) -> AnyElement {
    div().size(px(size)).flex().items_center().justify_center().child(Icon::new(Glyph::Sticker).size(px(size * 0.45))).into_any_element()
}

/// A sticker in a message: bare at 160px, still unless `animate`. One that can't be drawn keeps its name
/// in a dashed outline.
pub(crate) fn in_chat(sticker: &Sticker, animate: bool, cx: &App) -> AnyElement {
    let (line, muted, faint) = (palette::line_strong(cx), palette::muted(cx), palette::faint(cx));
    let name = sticker.name.clone();
    let lottie = sticker.url(FETCH).is_none();
    let missing = move || {
        let text = if lottie { format!("Can't play :{name}: here") } else { format!("Couldn't load :{name}:") };
        div()
            .size(px(IN_CHAT))
            .p_3()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_1()
            .squircle(RADIUS_LG)
            .border(px(1.5))
            .border_dashed()
            .border_color(line)
            .text_size(ui::TEXT_XS)
            .text_color(muted)
            .child(div().text_color(faint).child(Icon::new(Glyph::Sticker).size_7()))
            .child(div().text_center().child(text))
            .into_any_element()
    };
    let picture: AnyElement = match sticker.url(FETCH) {
        None => missing(),
        Some(url) => crate::chat::moving(img(url).size_full().object_fit(ObjectFit::Contain).with_fallback(missing), animate),
    };
    div().mt_1().size(px(IN_CHAT)).flex_none().child(picture).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::StickerMotion;

    #[test]
    fn reduced_motion_overrides_sticker_animation_to_on_interaction() {
        assert_eq!([0, 1, 2].map(|v| StickerMotion::of(v, false)), [StickerMotion::Always, StickerMotion::OnHover, StickerMotion::Never]);
        assert_eq!([0, 1, 2].map(|v| StickerMotion::of(v, true)), [StickerMotion::OnHover; 3]);
        assert!(StickerMotion::OnHover.plays(true) && !StickerMotion::OnHover.plays(false) && !StickerMotion::Never.plays(true));
    }
}
