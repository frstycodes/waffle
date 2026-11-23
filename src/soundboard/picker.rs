// The picker's contents: its sections of sounds, a tile for each, and the volume card.
use crate::ui;

use super::*;

impl Waffle {
    pub(super) fn picker_content(&self, picker: &Picker, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let sections = self.sound_sections(cx);
        let muted = self.prefs.soundboard_volume <= 0.;
        let volume = ui::row()
            .id("soundboard-volume")
            .size_7()
            .flex_none()
            .squircle(8.)
            .justify_center()
            .cursor_pointer()
            .text_color(palette::muted(cx))
            .hover(|s| s.bg(palette::hover(cx)).text_color(palette::strong(cx)))
            .when(picker.volume_open, |b| b.bg(palette::selected(cx)).text_color(palette::strong(cx)))
            .child(Icon::new(if muted { Glyph::VolumeOff } else { Glyph::VolumeHigh }).size(ui::ICON_MD))
            .tooltip(|window, cx| ui::tip("Control volume", window, cx))
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(picker) = &mut this.soundboard.picker {
                    picker.volume_open = !picker.volume_open;
                }
                cx.notify(); // lint-allow(global-notify): the soundboard picker is drawn by the root's overlays, over every column
            }));
        let head = div().flex_none().pt_3().px_3().pb_2().child(ui::search_field("soundboard-search", &self.soundboard.search, cx).trailing(volume));
        let in_view = picker.scroll.top_item();
        let rail = ui::col()
            .id("soundboard-rail")
            .w(px(RAIL_WIDTH))
            .flex_none()
            .px_2()
            .pt_1()
            .pb_2()
            .items_center()
            .gap_1p5()
            .overflow_y_scroll()
            .border_r_1()
            .border_color(palette::line(cx))
            .children(sections.iter().enumerate().flat_map(|(i, section)| {
                let line = section.first_locked.then(|| div().w_5().h(ui::HAIRLINE).flex_none().bg(palette::line(cx)).into_any_element());
                let scroll = picker.scroll.clone();
                let (selected, hover) = (palette::accent_soft(cx), palette::hover(cx));
                let tip: SharedString = if section.locked { NITRO_LOCKED.into() } else { section.name.clone() };
                let tab = ui::row()
                    .id(("soundboard-tab", i))
                    .size_9()
                    .flex_none()
                    .squircle(RADIUS_SM)
                    .justify_center()
                    .cursor_pointer()
                    .text_color(if i == in_view { palette::accent_ink(cx) } else { palette::muted(cx) })
                    .when(i == in_view, |d| d.bg(selected))
                    .when(i != in_view, |d| d.bg(palette::selected(cx)).hover(move |s| s.bg(hover)))
                    .when(section.locked, |d| d.opacity(0.5))
                    .child(section.lead.render(cx))
                    .tooltip(move |window, cx| ui::tip(tip.clone(), window, cx))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        scroll.scroll_to_top_of_item(i);
                        cx.notify(); // lint-allow(global-notify): the soundboard picker is drawn by the root's overlays, over every column
                    }));
                line.into_iter().chain([tab.into_any_element()])
            }));
        let over_limit = picker.over_limit.then(|| ui::callout(ui::Notice::Warning, Glyph::Star, "Oh no!", format!("You cannot have more than {FAVORITES_LIMIT} favorites."), cx).mt_2());
        let list = div()
            .id("soundboard-list")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .track_scroll(&picker.scroll)
            .px_3()
            .pb_3()
            .children(sections.into_iter().map(|section| self.sound_section(section, cx)));
        let volume_card = picker.volume_open.then(|| self.volume_card(cx).into_any_element());
        let mut content = vec![
            head.into_any_element(),
            ui::row().items_stretch()
                .flex_1()
                .min_h_0()
                .child(rail)
                .child(ui::col().flex_1().h_full().relative().children(over_limit).child(list).child(gpui_kit::component::scroll::Scrollbar::vertical(&picker.scroll)))
                .into_any_element(),
        ];
        content.extend(volume_card);
        content
    }

    // "Soundboard Volume" under the search field's speaker: the same value as Settings › Voice & Video.
    pub(super) fn volume_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let value = format!("{}%", perceptual(self.prefs.soundboard_volume).round() as i32);
        let slider = self.range("soundboard-picker-volume".into(), slider_fraction(self.prefs.soundboard_volume), Fill::Accent, |this, f, cx| this.update_prefs(cx, |p| p.soundboard_volume = amplitude(f * 100.)), cx);
        ui::col()
            .id("soundboard-volume-card")
            .occlude()
            .absolute()
            .top(px(VOLUME_TOP))
            .right_3()
            .w(px(VOLUME_WIDTH))
            .p_3()
            .squircle(RADIUS_SM + 4.)
            .bg(palette::surface(palette::Level::Overlay, cx))
            .shadow(ui::shadow_pop(cx))
            .gap_2()
            .child(ui::row().items_stretch().justify_between().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(palette::muted(cx)).child("Soundboard Volume").child(div().text_color(palette::strong(cx)).child(value)))
            .child(slider)
    }

    pub(super) fn sound_section(&self, section: Section, cx: &mut Context<Self>) -> AnyElement {
        let header = ui::row()
            .gap_1p5()
            .pt_2p5()
            .pb_1p5()
            .px_0p5()
            .text_size(ui::TEXT_XS)
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette::muted(cx))
            .child(section.lead.small(cx))
            .child(section.name.clone());
        let rows = section.sounds.chunks(3).map(|row| {
            let tiles = row.iter().map(|s| self.sound_tile(&section.key, s, section.locked, cx).into_any_element());
            let fillers = (row.len()..3).map(|_| div().flex_1().into_any_element());
            ui::row().items_stretch().gap_1p5().children(tiles.chain(fillers).collect::<Vec<_>>())
        }).collect::<Vec<_>>();
        div()
            .when(section.first_locked, |d| d.mt_3().border_t_1().border_color(palette::line(cx)))
            .when(section.locked, |d| d.opacity(0.5))
            .child(header)
            .child(ui::col().gap_1p5().children(rows))
            .into_any_element()
    }

    // .snd: emoji and name; hovered, Preview and the star over its right end on a fade into its fill. Playing
    // draws the accent edge. A locked sound only says why.
    pub(super) fn sound_tile(&self, section: &str, sound: &SoundboardSound, locked: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let group = SharedString::from(format!("snd-{section}-{}", sound.id));
        let overlay = palette::surface(palette::Level::Overlay, cx);
        let strong = palette::strong(cx);
        let (rest, hover) = (ui::mix(overlay, strong, 0.08), ui::mix(overlay, strong, 0.14));
        let edge = if self.soundboard.is_playing(&sound.id) { palette::accent() } else { gpui_kit::transparent_black() };
        let tile = ui::row()
            .id(group.clone())
            .group(group.clone())
            .relative()
            .flex_1()
            .h_9()
            .px_2()
            .squircle(RADIUS_SM)
            .border_1()
            .border_color(edge)
            .bg(rest)
            .gap_1p5()
            .text_size(ui::TEXT_XS)
            .font_weight(FontWeight::MEDIUM)
            .text_color(strong)
            .child(div().flex_none().child(emoji_face(sound.emoji.as_ref(), 16.)))
            .child(ui::line(sound.name.clone()).flex_1());
        if locked {
            return tile.cursor(gpui_kit::CursorStyle::OperationNotAllowed).tooltip(|window, cx| ui::tip(NITRO_LOCKED, window, cx));
        }
        let favorite = self.prefs.soundboard_favorites.contains(&sound.id);
        let silent = self.prefs.soundboard_volume <= 0.;
        let preview_tip = if silent { "Increase your Soundboard volume setting to preview" } else { "Preview" };
        let previewed = sound.clone();
        let preview = tool(format!("{group}-preview"), Glyph::VolumeHigh, preview_tip, None, cx).when(!silent, |b| {
            b.on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.preview_sound(&previewed, cx);
            }))
        });
        let starred = sound.id.clone();
        let star = tool(format!("{group}-star"), Glyph::Star, if favorite { "Unfavorite" } else { "Favorite" }, favorite.then(|| palette::accent_ink(cx)), cx).on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.toggle_favorite(starred.clone(), cx);
        }));
        let tools = ui::row()
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .pl_4()
            .pr_1()
            .gap_0p5()
            .bg(linear_gradient(90., linear_color_stop(hover.opacity(0.), 0.), linear_color_stop(hover, 0.25)))
            .opacity(0.)
            .group_hover(group.clone(), |s| s.opacity(1.))
            .child(preview)
            .child(star);
        let played = sound.clone();
        tile.cursor_pointer().hover(move |s| s.bg(hover)).child(tools).on_click(cx.listener(move |this, _, _, cx| this.play_soundboard_sound(played.clone(), cx)))
    }

    // Favorites, Frequently Used, each server's sounds we may play, the servers we can't below a line, then
    // Discord's own. A search filters them all by name or emoji, without the first two.
    pub(super) fn sound_sections(&self, cx: &App) -> Vec<Section> {
        let query = self.soundboard.search.read(cx).value().trim().to_lowercase();
        let matches = |s: &&SoundboardSound| query.is_empty() || s.name.to_lowercase().contains(&query) || s.emoji.as_ref().is_some_and(|e| e.name.to_lowercase().contains(&query));
        let known = |id: &String| self.soundboard.sound(id).filter(|s| self.sound_usable(s)).cloned();
        let mut sections = Vec::new();
        if query.is_empty() {
            let favorites: Vec<SoundboardSound> = self.prefs.soundboard_favorites.iter().filter_map(known).collect();
            sections.push(Section::new("favorites", Lead::Glyph(Glyph::Star), "Favorites", favorites));
            let mut uses = self.prefs.soundboard_uses.clone();
            uses.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
            let frequent: Vec<SoundboardSound> = uses.iter().filter_map(|(id, _)| known(id)).take(FREQUENT_COUNT).collect();
            sections.push(Section::new("frequent", Lead::Glyph(Glyph::Clock), "Frequently Used", frequent));
        }
        let call_guild = self.call.as_ref().and_then(|c| c.guild_id.clone());
        // The call's own server first, then the rest in rail order.
        let mut guilds: Vec<_> = self.guilds.iter().collect();
        guilds.sort_by_key(|g| Some(&g.id) != call_guild.as_ref());
        let (mut open, mut locked) = (Vec::new(), Vec::new());
        for guild in guilds {
            let sounds: Vec<SoundboardSound> = self.soundboard.guild_sounds.get(&guild.id).into_iter().flatten().filter(|s| s.available).filter(matches).cloned().collect();
            if sounds.is_empty() {
                continue;
            }
            let usable = sounds.iter().all(|s| self.sound_usable(s));
            let section = Section::new(&guild.id, Lead::Server { icon: guild.icon.clone(), name: guild.name.clone() }, &guild.name, sounds);
            if usable { open.push(section) } else { locked.push(section) }
        }
        sections.extend(open);
        if let Some(first) = locked.first_mut() {
            first.first_locked = true;
        }
        sections.extend(locked.into_iter().map(|s| Section { locked: true, ..s }));
        let defaults: Vec<SoundboardSound> = self.soundboard.defaults.iter().filter(matches).cloned().collect();
        sections.push(Section::new("defaults", Lead::Glyph(Glyph::Soundboard), "Discord Sounds", defaults));
        sections.retain(|s| !s.sounds.is_empty());
        sections
    }
}
pub(super) struct Section {
    pub(super) key: String,
    pub(super) lead: Lead,
    pub(super) name: SharedString,
    pub(super) sounds: Vec<SoundboardSound>,
    pub(super) locked: bool,
    pub(super) first_locked: bool,
}

impl Section {
    pub(super) fn new(key: &str, lead: Lead, name: &str, sounds: Vec<SoundboardSound>) -> Self {
        Section { key: key.into(), lead, name: name.to_owned().into(), sounds, locked: false, first_locked: false }
    }
}

pub(super) enum Lead {
    Glyph(Glyph),
    Server { icon: Option<String>, name: String },
}

impl Lead {
    pub(super) fn render(&self, cx: &App) -> AnyElement {
        match self {
            Lead::Glyph(glyph) => Icon::new(*glyph).size(ui::ICON_LG).into_any_element(),
            Lead::Server { icon, name } => ui::server_tile(icon.as_deref(), name, 36., cx),
        }
    }

    pub(super) fn small(&self, cx: &App) -> AnyElement {
        match self {
            Lead::Glyph(glyph) => Icon::new(*glyph).size(ui::ICON_SM).into_any_element(),
            Lead::Server { icon, name } => ui::server_tile(icon.as_deref(), name, 16., cx),
        }
    }
}

// A server's emoji as its image, a Unicode one as text.
pub(super) fn emoji_face(emoji: Option<&Emoji>, size: f32) -> AnyElement {
    match emoji {
        Some(e) if e.id.is_some() => img(e.url().unwrap_or_default()).size(px(size)).into_any_element(),
        Some(e) => div().text_size(px(size)).line_height(px(size)).child(e.name.clone()).into_any_element(),
        None => Icon::new(Glyph::Soundboard).size(px(size)).into_any_element(),
    }
}

// The 22px buttons over a hovered sound.
pub(super) fn tool(id: String, glyph: Glyph, tip: &'static str, ink: Option<Hsla>, cx: &App) -> gpui_kit::Stateful<gpui_kit::Div> {
    let strong = palette::strong(cx);
    ui::row()
        .id(SharedString::from(id))
        .size(px(TOOL_SIZE))
        .squircle(6.)
        .justify_center()
        .cursor_pointer()
        .text_color(ink.unwrap_or_else(|| palette::muted(cx)))
        .hover(move |s| s.bg(palette::hover(cx)).text_color(ink.unwrap_or(strong)))
        .child(Icon::new(glyph).size(ui::ICON_SM))
        .tooltip(move |window, cx| ui::tip(tip, window, cx))
}

// Copies of the emoji rising from the tile's foot, staggered, each easing out as it fades (design §3 `.burst`).
// One effect over a tile: every copy of the emoji follows its family's keyframes (`soundboard_fx`),
