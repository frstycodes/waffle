// The emoji picker: its sections, rail, grid and the pick.
use super::*;

pub(super) struct EmojiSection {
    pub(super) lead: SectionLead,
    pub(super) name: SharedString,
    // What the foot says an emoji from here is.
    pub(super) source: SharedString,
    pub(super) items: Vec<Emoji>,
}

// Recent and the Unicode groups are marked by an icon, a server's own emoji by its icon.
pub(super) enum SectionLead {
    Glyph(Glyph),
    Server { icon: Option<String>, name: String },
}

impl SectionLead {
    pub(super) fn render(&self, cx: &App) -> AnyElement {
        match self {
            SectionLead::Glyph(glyph) => Icon::new(*glyph).size(ui::ICON_LG).into_any_element(),
            SectionLead::Server { icon, name } => ui::server_tile(icon.as_deref(), name, 24., cx),
        }
    }

    pub(super) fn small(&self, cx: &App) -> AnyElement {
        match self {
            SectionLead::Glyph(glyph) => Icon::new(*glyph).size(ui::ICON_SM).into_any_element(),
            SectionLead::Server { icon, name } => ui::server_tile(icon.as_deref(), name, 16., cx),
        }
    }
}

// A Unicode group's name, and the icon that marks it in the picker's rail.
pub(super) fn group_name(group: emojis::Group) -> (&'static str, Glyph) {
    use emojis::Group::*;
    match group {
        SmileysAndEmotion => ("Smileys & Emotion", Glyph::Emoji),
        PeopleAndBody => ("People", Glyph::HandWaving),
        AnimalsAndNature => ("Nature", Glyph::Leaf),
        FoodAndDrink => ("Food & Drink", Glyph::Hamburger),
        TravelAndPlaces => ("Travel & Places", Glyph::Airplane),
        Activities => ("Activities", Glyph::SoccerBall),
        Objects => ("Objects", Glyph::Lightbulb),
        Symbols => ("Symbols", Glyph::Heart),
        Flags => ("Flags", Glyph::Flag),
    }
}

impl Waffle {
    pub(crate) fn on_emoji_search(&mut self, _: &Entity<SearchState>, _: &InputEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.redraw_overlays(cx);
    }

    pub(crate) fn open_emoji_picker(&mut self, target: EmojiTarget, window: &mut Window, cx: &mut Context<Self>) {
        let composer = matches!(target, EmojiTarget::Composer);
        self.emoji_picker = Some(target);
        self.popover_at = window.mouse_position();
        self.emoji_search.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.focus(window, cx);
        });
        self.redraw_overlays(cx);
        // The composer's emoji button shows the picker as open.
        if composer {
            self.redraw(Column::Main, cx);
        }
    }

    pub(super) fn pick_emoji(&mut self, emoji: Emoji, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.emoji_picker.take() else { return };
        match target {
            EmojiTarget::Reaction(message) => self.toggle_reaction(&message, emoji, cx),
            EmojiTarget::PollAnswer(ix) => self.set_poll_emoji(ix, emoji),
            EmojiTarget::CustomStatus => {
                if let Some(editor) = self.status_editor.as_mut() {
                    editor.emoji = Some(emoji);
                }
            }
            EmojiTarget::VoiceStatus => {
                let shown = self.shown_emoji(&emoji);
                self.insert_voice_status_emoji(shown, window, cx);
            }
            EmojiTarget::ForwardNote => {
                let shown = self.shown_emoji(&emoji);
                if let Some(dialog) = &self.forward_dialog {
                    dialog.note.update(cx, |state, cx| {
                        state.insert(shown, window, cx);
                        state.focus(window, cx);
                    });
                }
            }
            EmojiTarget::Composer => {
                let shown = self.shown_emoji(&emoji);
                // insert, not set_value: that would drop the draft's mention chips and put the caret first.
                self.composer.update(cx, |state, cx| {
                    let end = state.value().len();
                    state.set_selected_range(end..end, cx);
                    state.insert(shown, window, cx);
                    state.focus(window, cx);
                });
                self.has_draft = true;
            }
        }
        cx.notify(); // lint-allow(global-notify): a pick closes the picker and changes the chat's reactions, draft or poll
    }

    // An emoji as a field shows it: itself, or `:name:` for a server's own, which `to_wire` turns back into
    // its `<:name:id>` when sent.
    pub(super) fn shown_emoji(&mut self, emoji: &Emoji) -> String {
        let Some(id) = &emoji.id else { return emoji.name.clone() };
        let shown = format!(":{}:", emoji.name);
        let wire = format!("<{}:{}:{id}>", if emoji.animated { "a" } else { "" }, emoji.name);
        self.mention_wire.insert(shown.clone(), wire);
        shown
    }

    // The emoji picker (messages.css `.picker`): search on top, categories down the left, the grid in
    // the middle and the hovered emoji named at the foot. Recent first, then the server's own, then Unicode.
    // For a reaction it opens above and left of the click, kept in the window; from the composer it's the
    // composer picker's Emoji tab.
    pub(super) fn emoji_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if matches!(self.emoji_picker.as_ref()?, EmojiTarget::Composer) {
            return None;
        }
        let picker = picker_frame("emoji-picker", PICKER_SIZE.1, cx).children(self.emoji_content(window, cx));
        Some(backdrop("emoji-backdrop", cx).child(ui::floating_at("emoji-float", self.popover_at, gpui_kit::Anchor::BottomRight, ui::rise(picker.transform_origin(ui::corner(true, true)), "emoji-picker", Duration::ZERO))).into_any_element())
    }

    // Search, the category rail beside the grid, and the hovered emoji named at the foot.
    pub(super) fn emoji_content(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let sections = self.emoji_sections(cx);
                let scroll = window.use_keyed_state("emoji-scroll-handle", cx, |_, _| ScrollHandle::new()).read(cx).clone();
        let hovered = window.use_keyed_state("emoji-hovered", cx, |_, _| None::<(Emoji, SharedString)>);
        let preview = hovered.read(cx).clone().or_else(|| sections.first().and_then(|s| Some((s.items.first()?.clone(), s.source.clone()))));
        let in_view = scroll.top_item();
        let rail = ui::col()
            .id("emoji-rail")
            .w_12()
            .flex_none()
            .px_1p5()
            .pb_1p5()
            .items_center()
            .gap_0p5()
            .overflow_y_scroll()
            .border_r_1()
            .border_color(palette::line(cx))
            .children(sections.iter().enumerate().flat_map(|(i, section)| {
                let scroll = scroll.clone();
                // A line sets the server's own emoji apart from the Unicode groups after them.
                let after_server = i > 0 && matches!(sections[i - 1].lead, SectionLead::Server { .. }) && !matches!(section.lead, SectionLead::Server { .. });
                let line = after_server.then(|| ui::col().py_1().items_center().flex_none().child(ui::divider(None, cx).w_5()).into_any_element());
                let tab = ui::icon_slot(("emoji-cat", i), section.name.clone(), cx)
                    .sized(36.)
                    .toggled(i == in_view, cx)
                    .child(section.lead.render(cx))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        scroll.scroll_to_top_of_item(i);
                        this.redraw_overlays(cx);
                    }));
                line.into_iter().chain([tab.into_any_element()])
            }));
        let cell = |emoji: Emoji, source: SharedString, key: String| {
            let face = match emoji.url() {
                Some(url) => img(url).size_7().into_any_element(),
                None => div().text_size(px(EMOJI_GLYPH)).line_height(px(EMOJI_GLYPH)).child(emoji.name.clone()).into_any_element(),
            };
            let (picked, shown) = (emoji.clone(), (emoji, source));
            let hovered = hovered.clone();
            // lint-allow(custom-button): ui::icon_button is a square taking a Glyph; an emoji cell is a ninth of the grid's width.
            ui::row()
                .id(SharedString::from(key))
                .w(relative(1. / 9.))
                .h_10()
                .squircle(RADIUS_SM)
                .justify_center()
                .cursor_pointer()
                .hover(|s| s.bg(palette::selected(cx)))
                .child(face)
                .on_hover(move |on: &bool, _, cx| {
                    if *on {
                        hovered.update(cx, |h, cx| {
                            *h = Some(shown.clone());
                            cx.notify(); // lint-allow(global-notify): notifies the hovered entry's own entity, not Waffle
                        });
                    }
                })
                .on_click(cx.listener(move |this, _, window, cx| this.pick_emoji(picked.clone(), window, cx)))
        };
        // lint-allow(unvirtualized-list): the Unicode groups hold under 2,000 emoji and a server adds a few hundred at most.
        let grid = div()
            .id("emoji-scroll")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .px_2()
            .pb_2()
            .children(sections.into_iter().map(|section| {
                // GAP: the section label sticks to the top of the grid in the design; gpui has no sticky.
                ui::col()
                    .pb_1()
                    .child(
                        ui::row()
                            .gap_1p5()
                            .py_1p5()
                            .px_1()
                            .text_size(ui::TEXT_XS)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette::muted(cx))
                            .child(section.lead.small(cx))
                            .child(section.name.clone()),
                    )
                    .child(ui::row().items_stretch().flex_wrap().children(section.items.into_iter().enumerate().map(|(i, e)| cell(e, section.source.clone(), format!("emoji-{}-{i}", section.name)))))
            }));
        let foot = ui::row()
            .min_h(px(EMOJI_FOOT_HEIGHT))
            .flex_none()
            .py_1p5()
            .px_3()
            .gap_2p5()
            .border_t_1()
            .border_color(palette::line(cx))
            .children(preview.map(|(emoji, source)| {
                let face = match emoji.url() {
                    Some(url) => img(url).size_7().into_any_element(),
                    None => div().text_size(ui::TEXT_XL).line_height(ui::LINE_XL).child(emoji.name.clone()).into_any_element(),
                };
                ui::row()
                    .gap_2p5()
                    .child(ui::row().items_stretch().w_8().flex_none().justify_center().child(face))
                    .child(
                        ui::col()
                            .child(ui::line(crate::message_menu::shortcode(&emoji)).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)))
                            .child(div().text_size(ui::TEXT_XS).text_color(palette::muted(cx)).child(source)),
                    )
            }));
        // GAP: the design's skin-tone button sits beside search; Waffle has no skin tones yet.
        let head = div().p_2p5().child(ui::search_field("emoji-search", &self.emoji_search, cx));
        vec![
            head.into_any_element(),
            ui::row().items_stretch().flex_1().min_h_0().child(rail).child(ui::row().items_stretch().flex_1().h_full().relative().child(grid).child(gpui_kit::component::scroll::Scrollbar::vertical(&scroll))).into_any_element(),
            foot.into_any_element(),
        ]
    }

    // What the picker lists for the current search: recent (only before you search), the server's own, then Unicode by group.
    pub(super) fn emoji_sections(&self, cx: &App) -> Vec<EmojiSection> {
        let query = self.emoji_search.read(cx).value().to_lowercase();
        let unicode: SharedString = "Unicode emoji".into();
        let mut sections = Vec::new();
        if query.is_empty() {
            let recent = self.prefs.frequent_emojis(18, &crate::message_menu::QUICK_PAD);
            sections.push(EmojiSection { lead: SectionLead::Glyph(Glyph::Recent), name: "Recently used".into(), source: unicode.clone(), items: recent });
        }
        if let Some(guild) = self.guild() {
            let custom: Vec<Emoji> = guild.emojis.iter().filter(|e| query.is_empty() || e.name.to_lowercase().contains(&query)).cloned().collect();
            if !custom.is_empty() {
                let name: SharedString = guild.name.clone().into();
                sections.push(EmojiSection { lead: SectionLead::Server { icon: guild.icon.clone(), name: guild.name.clone() }, name: name.clone(), source: name, items: custom });
            }
        }
        for group in emojis::Group::iter() {
            let items: Vec<Emoji> = group
                .emojis()
                .filter(|e| query.is_empty() || e.name().contains(&query) || e.shortcodes().any(|s| s.contains(&query)))
                .map(|e| Emoji::unicode(e.as_str()))
                .collect();
            if !items.is_empty() {
                let (name, glyph) = group_name(group);
                sections.push(EmojiSection { lead: SectionLead::Glyph(glyph), name: name.into(), source: unicode.clone(), items });
            }
        }
        sections
    }
}
