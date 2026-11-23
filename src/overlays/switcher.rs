// The quick switcher: its results, keys and rows.
use super::*;

impl Waffle {
    pub(crate) fn on_switcher_input(&mut self, _: &Entity<SearchState>, event: &InputEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            InputEvent::PressEnter { .. } => self.accept_switcher(window, cx),
            InputEvent::Change => {
                self.switcher = Some(0);
                self.redraw_overlays(cx);
            }
            _ => {}
        }
    }

    pub(crate) fn open_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.switcher = Some(0);
        self.switcher_input.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.focus(window, cx);
        });
        self.redraw_overlays(cx);
    }

    // Every text channel and DM, best name matches first; empty query lists DMs then the open server.
    pub(super) fn switcher_results(&self, cx: &Context<Self>) -> Vec<(Option<String>, Channel, SharedString)> {
        let query = self.switcher_input.read(cx).value().to_lowercase();
        let mut results: Vec<(Option<String>, Channel, SharedString)> = Vec::new();
        for dm in &self.dms {
            results.push((None, dm.clone(), "Direct Messages".into()));
        }
        for guild in &self.guilds {
            for channel in guild.channels.iter().filter(|c| c.kind == ChannelKind::Text && guild.can_view(self.my_id(), c)) {
                results.push((Some(guild.id.clone()), channel.clone(), guild.name.clone().into()));
            }
        }
        if query.is_empty() {
            results.retain(|(guild, _, _)| guild.is_none() || guild == &self.guild_id);
        } else {
            results.retain(|(_, c, _)| c.name.to_lowercase().contains(&query));
            results.sort_by_key(|(_, c, _)| !c.name.to_lowercase().starts_with(&query));
        }
        results.truncate(12);
        results
    }

    pub(super) fn accept_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected) = self.switcher else { return };
        let Some((guild_id, channel, _)) = self.switcher_results(cx).into_iter().nth(selected) else { return };
        self.switcher = None;
        self.guild_id = guild_id;
        self.open_channel(channel, window, cx);
        self.composer.update(cx, |state, cx| state.focus(window, cx));
    }

    pub(crate) fn on_switcher_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected) = self.switcher else { return };
        let count = self.switcher_results(cx).len().max(1);
        match event.keystroke.key.as_str() {
            "down" => self.switcher = Some((selected + 1) % count),
            "up" => self.switcher = Some((selected + count - 1) % count),
            "escape" => self.switcher = None,
            "enter" => return self.accept_switcher(window, cx),
            _ => return,
        }
        cx.stop_propagation();
        self.redraw_overlays(cx);
    }

    // The quick switcher (navigation.css `.switcher`): a focused field, results grouped by kind with
    // the match in bold, arrows to pick, Enter to go.
    pub(super) fn switcher_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let selected = self.switcher?;
        let query = self.switcher_input.read(cx).value().to_string();
        let results = self.switcher_results(cx);
        let mut list: Vec<AnyElement> = Vec::new();
        let mut last_group = None;
        for (i, (guild_id, channel, place)) in results.into_iter().enumerate() {
            let group = if channel.recipient.is_some() || channel.kind == ChannelKind::Dm { "People" } else { "Channels" };
            if last_group != Some(group) {
                list.push(div().pt_2p5().px_2p5().pb_1().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(palette::faint(cx)).child(group).into_any_element());
                last_group = Some(group);
            }
            list.push(self.switcher_row(i == selected, guild_id, channel, place, &query, cx).into_any_element());
        }
        let is_empty = list.is_empty();
        let field = div().flex_none().child(ui::search_field("switcher-search", &self.switcher_input, cx));
        let hint = |keys: Vec<AnyElement>, label: &'static str| ui::row().gap_1().children(keys).child(label);
        let key = |glyph: Glyph| crate::ui::kbd("", cx).child(Icon::new(glyph).size(ui::ICON_XS)).into_any_element();
        let foot = ui::row()
            .pt_2p5()
            .px_2p5()
            .pb_1()
            .flex_wrap()
            .gap_x_4()
            .gap_y_2()
            .border_t_1()
            .border_color(palette::line(cx))
            .text_size(ui::TEXT_XS)
            .text_color(palette::muted(cx))
            .when(!is_empty, |f| f.child(hint(vec![key(Glyph::ChevronUp), key(Glyph::ChevronDown)], "Move")).child(hint(vec![crate::ui::kbd("Enter", cx).into_any_element()], "Open")))
            .child(hint(vec![crate::ui::kbd("Esc", cx).into_any_element()], "Close"));
        let body: AnyElement = if is_empty {
            crate::ui::empty_state("switcher-empty", crate::mascot::Mood::Waiting, format!("Nothing called “{query}”"), "Check the spelling, or search messages in a channel instead.", cx).into_any_element()
        } else {
            ui::col().id("switcher-list").gap_0p5().max_h(px(SWITCHER_LIST_HEIGHT)).overflow_y_scroll().children(list).into_any_element()
        };
        let body = ui::col()
            .p_2p5()
            .gap_2()
            .capture_key_down(cx.listener(Self::on_switcher_key))
            .child(field)
            .child(body)
            .child(foot);
        Some(
            backdrop("switcher-backdrop", cx)
                .bg(palette::scrim(cx))
                .child(ui::row().items_stretch().absolute().top(px(SWITCHER_TOP)).left_0().right_0().justify_center().child(ui::dialog("switcher", SWITCHER_WIDTH, body, window, cx)))
                .into_any_element(),
        )
    }

    // A channel's mark in a list of places: the person with their status, a group's faces, else the kind's glyph.
    pub(crate) fn channel_lead(&self, channel: &Channel, cx: &App) -> AnyElement {
        match (&channel.recipient, channel.kind) {
            (Some(user), _) => {
                let status = self.presences.get(&user.id).map(|p| p.mark());
                avatar_with_status(&user.avatar, 20., status, palette::surface(palette::Level::Overlay, cx))
            }
            (None, ChannelKind::Dm) => ui::dm_face(&channel.face(), 20., palette::surface(palette::Level::Overlay, cx)),
            (None, kind) => {
                let glyph = match kind {
                    ChannelKind::Dm => Glyph::User,
                    ChannelKind::Voice => Glyph::VolumeHigh,
                    _ if channel.private => Glyph::HashtagLock,
                    _ => Glyph::Hashtag,
                };
                div().text_color(palette::faint(cx)).child(Icon::new(glyph).size(ui::ICON_LG)).into_any_element()
            }
        }
    }

    pub(super) fn switcher_row(&self, active: bool, guild_id: Option<String>, channel: Channel, place: SharedString, query: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let lead = self.channel_lead(&channel, cx);
        let name: SharedString = channel.name.clone().into();
        let strong = palette::strong(cx);
        let mark = name.to_lowercase().find(&query.to_lowercase()).filter(|_| !query.is_empty()).map(|at| at..at + query.len()).filter(|r| name.is_char_boundary(r.start) && name.is_char_boundary(r.end));
        let label = StyledText::new(name.clone()).with_highlights(mark.map(|r| (r, HighlightStyle { color: Some(strong), font_weight: Some(FontWeight::SEMIBOLD), ..Default::default() })));
        ui::list_row(SharedString::from(format!("switch-{}", channel.id)), name.clone(), cx)
            .height(ui::RowHeight::Dense)
            .px_2p5()
            .flex_none()
            .gap_2p5()
            .control(|c| c.toggled(active, cx))
            .title_ink(if active { strong } else { palette::text(cx) })
            .title_element(rich_line(label).flex_1())
            .lead(lead)
            .trailing(div().pl_3().text_size(ui::TEXT_XS).text_color(palette::faint(cx)).child(place))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.switcher = None;
                this.guild_id = guild_id.clone();
                this.open_channel(channel.clone(), window, cx);
            }))
    }
}
