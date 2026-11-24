// The channel list: categories, channel rows, threads and the drag between them.
use super::*;

// CHANNEL LIST

pub(super) const THREAD_HEIGHT: f32 = 28.;

// A row and whether it stays in view while its category is folded.
pub(super) struct ChannelRow {
    pub(super) keep: bool,
    pub(super) element: AnyElement,
}

#[derive(Clone, Copy)]
pub(crate) struct RowState {
    pub selected: bool,
    pub unread: bool,
    pub muted: bool,
    // Full ink without the unread pill (the voice channel you're in).
    pub strong: bool,
}

// A sidebar row (.ch): muted ink at rest; selected is a fill, unread is bold with the pill on the
// list's edge, muted is faint. `group` names the row for its hover-revealed actions.
pub(crate) fn channel_row(id: SharedString, group: SharedString, state: RowState, cx: &App) -> Stateful<Div> {
    let RowState { selected, unread, muted, strong } = state;
    let (hover, text) = (palette::hover(cx), palette::text(cx));
    let ink = match (selected || unread || strong, muted) {
        (true, _) => palette::strong(cx),
        (false, true) => palette::faint(cx),
        (false, false) => palette::muted(cx),
    };
    // lint-allow(custom-button): ui::list_row has fixed heights (36/48/60) and no unread pill; a channel row follows UI density.
    ui::row()
        .id(id)
        .group(group)
        .relative()
        .h(px(row_height()))
        .w_full()
        .flex_none()
        .px_2()
        .gap_2()
        .squircle(ui::RADIUS_SM)
        .cursor_pointer()
        .font_weight(if unread { FontWeight::MEDIUM } else { FontWeight::NORMAL })
        .text_color(ink)
        .when(selected, |r| r.bg(palette::selected(cx)))
        .when(!selected, |r| r.hover(move |s| s.bg(hover).text_color(text)))
        // Pinned to the name line's centre: a two-line row (a voice status under the name) would drop it to the top.
        .when(unread && !selected, |r| r.child(div().absolute().left_neg_2().top(px((row_height() - 8.) / 2.)).w_1().h_2().rounded_r_sm().bg(palette::strong(cx))))
}

// A category's header row (.cathead): the label, a button that folds the category unless `static_`,
// and room on its right for the add button. `group` names the row so the add can show on hover.
pub(crate) fn category_head(group: SharedString, label: Stateful<Div>, static_: bool, cx: &App) -> Div {
    let (hover, strong) = (palette::hover(cx), palette::strong(cx));
    ui::row().group(group).relative().child(
        label
            .flex_1()
            .min_w_0()
            .h_7()
            .px_1p5()
            .squircle(ui::RADIUS_SM)
            .flex()
            .items_center()
            .gap_1()
            .whitespace_nowrap()
            .text_size(ui::TEXT_XS)
            .font_weight(FontWeight::MEDIUM)
            .text_color(palette::faint(cx))
            .when(!static_, |l| l.cursor_pointer().hover(move |s| s.bg(hover).text_color(strong))),
    )
}

// The + at a category's end: creates a channel there (or, on Direct messages, starts a conversation).
// With `reveal_on`, it shows only while that header group is hovered.
pub(super) fn cat_add(id: impl Into<gpui_kit::ElementId>, reveal_on: Option<SharedString>, tooltip: &'static str, cx: &App) -> Stateful<Div> {
    category_action(id, Nav::Plus.icon(), reveal_on, tooltip, cx)
}

// A category head's trailing action: a 24px target with a 14px glyph, so every head's action is one size.
pub(crate) fn category_action(id: impl Into<gpui_kit::ElementId>, icon: Icon, reveal_on: Option<SharedString>, tooltip: &'static str, cx: &App) -> Stateful<Div> {
    let strong = palette::strong(cx);
    // lint-allow(custom-button, custom-mark): ui::icon_button fills on hover and takes a Glyph; this action only strengthens its ink and sits at the head's end.
    ui::row()
        .id(id)
        .absolute()
        .right_0p5()
        .size_6()
        .squircle(ui::RADIUS_XS)
        .justify_center()
        .cursor_pointer()
        .text_color(palette::muted(cx))
        .hover(move |s| s.text_color(strong))
        .when_some(reveal_on, |add, group| add.opacity(0.).group_hover(group, |s| s.opacity(1.)))
        .child(icon.size(ui::ICON_SM))
        .tooltip(move |window, cx| ui::tip(tooltip, window, cx))
}

// A 24px action inside a row (.ch .acts button).
pub(super) fn row_action(id: SharedString, icon: Icon, tooltip: &'static str, cx: &App) -> Stateful<Div> {
    let strong = palette::strong(cx);
    // lint-allow(custom-button): ui::icon_button fills on hover and takes a Glyph; a row's action only strengthens its ink.
    ui::row()
        .id(id)
        .size_6()
        .squircle(ui::RADIUS_XS)
        .justify_center()
        .text_color(palette::muted(cx))
        .hover(move |s| s.text_color(strong))
        .child(icon.size(ui::ICON_MD))
        .tooltip(move |window, cx| ui::tip(tooltip, window, cx))
}

// The pencil on a row whose channel holds an unsent draft.
pub(super) fn draft_mark() -> Icon {
    Icon::new(Glyph::Pen).size(ui::ICON_SM)
}

// The type glyph a channel leads with (Discord's raw types: 5 announcement, 13 stage, 16 media); a private
// channel's carries a padlock in its corner.
pub(crate) fn channel_glyph(channel: &Channel) -> Icon {
    if channel.private {
        return Icon::new(match (channel.kind, channel.raw_type) {
            (_, 5) => Glyph::AnnouncementLocked,
            (_, 13) => Glyph::StageLocked,
            (_, 16) => Glyph::MediaLocked,
            (ChannelKind::Voice, _) => Glyph::VoiceLocked,
            (ChannelKind::Forum, _) => Glyph::ForumLocked,
            _ => Glyph::TextLocked,
        });
    }
    match (channel.kind, channel.raw_type) {
        (_, 5) => Nav::Megaphone.icon(),
        (_, 13) => Nav::Stage.icon(),
        (_, 16) => Nav::Media.icon(),
        (ChannelKind::Voice, _) => Icon::new(Glyph::VolumeHigh),
        (ChannelKind::Forum, _) => Icon::new(Glyph::Forum),
        _ => Icon::new(Glyph::Hashtag),
    }
}

// The channel's name; a live stage's topic is its second line, as Discord's.
pub(super) fn channel_label(channel: &Channel, topic: Option<String>, cx: &App) -> AnyElement {
    let name = ui::truncating(format!("channel-name-{}", channel.id), channel.name.clone()).flex_1();
    let Some(topic) = topic else { return name.into_any_element() };
    let topic = ui::line(topic).text_size(ui::TEXT_XS).line_height(ui::LINE_XS).font_weight(FontWeight::NORMAL).text_color(palette::muted(cx));
    ui::col().min_h_0().flex_1().child(name).child(topic).into_any_element()
}

// The DM list's header: a field-shaped button that opens the quick switcher.
pub(super) fn dm_find(cx: &App) -> Stateful<Div> {
    ui::search_button("dm-find", "Find or start a conversation", cx).occlude()
}

// Every channel we can see, by category (uncategorised first), with no pins, opt-in or Active Now: what
// the server guide, Browse Channels and the shortcuts list from.
pub(crate) fn channel_groups(guild: &Guild, me: &str) -> Vec<(Option<Channel>, Vec<Channel>)> {
    let visible = |c: &&Channel| matches!(c.kind, ChannelKind::Text | ChannelKind::Voice | ChannelKind::Forum) && guild.can_view(me, c);
    let uncategorised: Vec<Channel> = guild.channels.iter().filter(visible).filter(|c| c.parent_id.is_none()).cloned().collect();
    let mut groups = vec![(None, uncategorised)];
    for category in guild.channels.iter().filter(|c| c.kind == ChannelKind::Category) {
        let children = guild.channels.iter().filter(visible).filter(|c| c.parent_id.as_ref() == Some(&category.id)).cloned().collect();
        groups.push((Some(category.clone()), children));
    }
    groups.retain(|(_, channels)| !channels.is_empty());
    groups
}

pub(crate) enum Section {
    Pinned,
    Uncategorised,
    Category(Box<Channel>),
    ActiveNow,
}

// The channel list in order: Pinned channels (server order, voice after text), the uncategorised
// channels, each category, then Active Now. Pinned channels leave their category and always show.
// With opt-in on, the rest show only when they or their category are in our list, `keep` holds them,
// or a joined active thread under them is `noticed`. Voice channels we haven't added that have
// people in them leave their category for Active Now (Discord's `voiceChannelsCategory`, #791513).
pub(crate) fn channel_sections(guild: &Guild, me: &str, settings: Option<&GuildSettings>, keep: impl Fn(&Channel) -> bool, noticed: impl Fn(&Channel) -> bool) -> Vec<(Section, Vec<Channel>)> {
    let visible = |c: &&Channel| matches!(c.kind, ChannelKind::Text | ChannelKind::Voice | ChannelKind::Forum) && guild.can_view(me, c);
    let pinned = |c: &Channel| settings.is_some_and(|s| s.pinned(&c.id));
    let opt_in = settings.filter(|s| s.opt_in());
    let added = |c: &Channel| opt_in.is_none_or(|s| s.opted_in(&c.id) || c.parent_id.as_ref().is_some_and(|p| s.opted_in(p)));
    let active_now = |c: &Channel| opt_in.is_some() && c.kind == ChannelKind::Voice && !added(c) && guild.voice_states.iter().any(|v| v.channel_id.as_ref() == Some(&c.id));
    let listed = |c: &Channel| added(c) || keep(c) || has_noticed_thread(guild, c, &noticed);
    let shown = |c: &&Channel| visible(c) && !pinned(c) && !active_now(c) && listed(c);
    let mut pins: Vec<Channel> = guild.channels.iter().filter(visible).filter(|c| pinned(c)).cloned().collect();
    pins.sort_by_key(|c| (c.kind == ChannelKind::Voice, c.position));
    let uncategorised = guild.channels.iter().filter(shown).filter(|c| c.parent_id.is_none()).cloned().collect();
    let mut sections = vec![(Section::Pinned, pins), (Section::Uncategorised, uncategorised)];
    for category in guild.channels.iter().filter(|c| c.kind == ChannelKind::Category) {
        let children = guild.channels.iter().filter(shown).filter(|c| c.parent_id.as_ref() == Some(&category.id)).cloned().collect();
        sections.push((Section::Category(Box::new(category.clone())), children));
    }
    let mut voice: Vec<Channel> = guild.channels.iter().filter(visible).filter(|c| !pinned(c) && active_now(c)).cloned().collect();
    let category_position = |c: &Channel| c.parent_id.as_ref().and_then(|p| guild.channels.iter().find(|x| &x.id == p)).map_or(-1, |p| p.position);
    voice.sort_by_key(|c| (category_position(c), c.position));
    sections.push((Section::ActiveNow, voice));
    sections.retain(|(_, channels)| !channels.is_empty());
    sections
}

// Discord's `eN` (#791513): a joined, unarchived thread under the channel that is `noticed`.
// GAP: Discord also counts a thread you recently visited and read; Waffle keeps no visit history.
pub(super) fn has_noticed_thread(guild: &Guild, channel: &Channel, noticed: impl Fn(&Channel) -> bool) -> bool {
    guild
        .threads
        .iter()
        .filter(|t| t.parent_id.as_ref() == Some(&channel.id))
        .any(|t| t.thread.as_ref().is_some_and(|m| m.joined && !m.archived) && noticed(t))
}

// Stable, so each group keeps the recency order it came in.
pub(super) fn pinned_first(dms: &[Channel], pinned: impl Fn(&str) -> bool) -> Vec<Channel> {
    let mut out = dms.to_vec();
    out.sort_by_key(|c| !pinned(&c.id));
    out
}

pub(super) fn channel_board(waffle: &mut Waffle) -> &mut Board {
    &mut waffle.channel_board
}

// A category's sortable list; channels outside any category share one.
pub(crate) fn channel_list(category: Option<&Channel>) -> SharedString {
    category.map_or_else(|| "channels".into(), |c| format!("category-{}", c.id).into())
}

// The open server's name as one button that drops the server menu, heading the channel list.
pub(crate) fn server_name_button(name: SharedString, cx: &mut Context<Waffle>) -> Stateful<Div> {
    let hover = palette::hover(cx);
    // lint-allow(custom-button): ui::button(Tone::Ghost) is 36px with a 14px label; this one is the header's full-width name button, 16px semibold.
    ui::row()
        .id("server-header")
        .occlude()
        .flex_1()
        .h(px(HEADER_BUTTON))
        .px_2()
        .squircle(ui::RADIUS_SM)
        .gap_2()
        .cursor_pointer()
        .text_size(ui::TEXT_MD)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette::strong(cx))
        .hover(move |s| s.bg(hover))
        .child(ui::truncating("server-name", name).flex_1())
        .child(Nav::Caret.icon().size_4().text_color(palette::muted(cx)))
        .on_mouse_down(gpui_kit::MouseButton::Left, cx.listener(|this, _: &gpui_kit::MouseDownEvent, _, cx| {
            // Drops from the button's bottom edge, at its left.
            let at = gpui_kit::point(px(RAIL_WIDTH + 8.), px(crate::layout::HEADER_HEIGHT - 4.));
            if this.in_favorites() {
                return this.open_favorites_menu(at, cx);
            }
            let Some(guild_id) = this.guild_id.clone() else { return };
            this.open_server_menu(guild_id, at, true, cx);
        }))
}

impl Waffle {
    pub fn render_channels(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rows: Vec<AnyElement> = Vec::new();
        match (&self.status, self.guild().cloned()) {
            // Until the gateway's first READY: the list's shape, shimmering.
            (Status::Connecting, guild) if guild.as_ref().map_or(self.dms.is_empty(), |g| g.channels.is_empty()) => rows.extend((0..8).map(|i| list_skeleton(i, guild.is_some(), cx))),
            _ if self.in_favorites() => rows.extend(self.favorite_rows(window, cx)),
            (_, Some(guild)) => {
                // Beside the carried channel: "Move to Archive" while it's aimed into another category.
                let tag = self.channel_board.dragged().zip(self.channel_board.hint()).and_then(|(moved, hint)| self.category_move(moved, hint.target.as_deref()?));
                self.drag_motion.set_tag(tag);
                rows.extend(self.event_rows(&guild, cx));
                rows.extend(self.guild_entry_rows(&guild, window, cx));
                for (i, (section, channels)) in self.sidebar_sections(&guild).into_iter().enumerate() {
                    let category = match section {
                        Section::Pinned => {
                            rows.push(self.static_section("pinned-head", "Pinned channels", &channels, &guild, window, cx).into_any_element());
                            continue;
                        }
                        Section::ActiveNow => {
                            rows.push(self.static_section("active-now-head", "Active now", &channels, &guild, window, cx).when(i > 0, |s| s.pt_4()).into_any_element());
                            continue;
                        }
                        Section::Uncategorised => None,
                        Section::Category(category) => Some(*category),
                    };
                    let order: Vec<String> = channels.iter().map(|c| c.id.clone()).collect();
                    let list = self.channel_board.list(channel_list(category.as_ref()), order, window, cx);
                    // A channel's first row is the item; its people and threads move with it.
                    let channel_rows = channels
                        .iter()
                        .flat_map(|c| {
                            let rows = self.channel_rows(c, &guild, window, cx);
                            let list = &list;
                            rows.into_iter().enumerate().map(move |(i, row)| ChannelRow {
                                keep: row.keep,
                                element: if i == 0 { list.item(&c.id, row.element) } else { list.follow(&c.id, row.element) },
                            })
                        })
                        .collect::<Vec<_>>();
                    match category {
                        Some(category) => rows.push(self.section(category, channel_rows, i > 0, window, cx)),
                        None => {
                            // Below Pinned channels, spaced like a category.
                            if i > 0 {
                                rows.push(div().h(px(SECTION_GAP - row_gap())).into_any_element());
                            }
                            rows.extend(channel_rows.into_iter().map(|row: ChannelRow| row.element));
                        }
                    }
                }
            }
            (_, None) => {
                rows.push(div().pb_2().child(dm_find(cx).on_click(cx.listener(|this, _, window, cx| this.open_switcher(window, cx)))).into_any_element());
                rows.push(self.friends_row(cx));
                rows.extend(self.requests_row(cx));
                let start = cat_add("dm-start", None, "Start a conversation", cx).on_click(cx.listener(|this, _, window, cx| this.open_switcher(window, cx)));
                rows.push(div().pt(px(ui::density_pad(SECTION_GAP))).child(category_head("dm-head".into(), div().id("dm-head").child("Direct messages"), true, cx).child(start)).into_any_element());
                match self.dms.is_empty() {
                    true => rows.push(muted_line("No direct messages yet", cx)),
                    false => rows.extend(self.dm_rows(rows.len(), cx)),
                }
            }
        }
        let reconnecting = match &self.status {
            Status::Reconnecting(reason) => Some(reason.clone()),
            _ => None,
        };
        let banner = self.motions.frame_one(
            Part::Banner,
            Motion::new(Target::default())
                .initial(Target::default().y(10.).opacity(0.))
                .exit(Target::default().y(10.).opacity(0.))
                .present(reconnecting.is_some())
                .transition(motion::spring()),
            window,
            cx,
        );
        let warning = cx.theme().warning;
        let banner = banner.render(|frame| {
            ui::col().px_2().pb_2().child(
                frame.apply(
                    // lint-allow(custom-badge): ui::callout leads with an icon and a title; this banner is a dot and one line, slid in by its motion frame.
                    ui::row()
                        .px_3()
                        .py_2()
                        .squircle(ui::RADIUS_MD)
                        .bg(warning.opacity(0.12))
                        .text_size(ui::TEXT_XS)
                        .gap_2()
                        .child(ui::unread_dot().size_1p5().bg(warning))
                        .child(ui::line(format!("Reconnecting… {}", reconnecting.clone().unwrap_or_default()))),
                ),
            )
        });
        // The header row: 8px in, like the list below, so the name lines up with the channel rows; below
        // 100% zoom the traffic lights outgrow the rail and push it further in.
        let header = ui::row()
            .h(px(crate::layout::HEADER_HEIGHT))
            .px_2()
            .pl(px(HEADER_PAD + crate::layout::traffic_lights_overhang(window)))
            .window_control_area(gpui_kit::WindowControlArea::Drag)
            .child(self.channel_list_head(cx));
        ui::col()
            .relative()
            .w(px(self.channels_width))
            .h_full()
            .flex_none()
            .bg(crate::material::sidebar_fill(cx))
            .text_color(palette::muted(cx))
            .text_size(ui::TEXT_SM)
            // The list stops under the header, a hard edge (ui::hard_edge).
            .child(ui::hard_edge(self.sidebar_scrolled(window), cx).child(header))
            .child(ui::scroll_area(
                ui::col()
                    .id("channels")
                    .pt_2()
                    .px_2()
                    .pb_2()
                    .gap(px(row_gap()))
                    .children(rows)
,
                &self.channels_scroll,
            ))
            .child(banner)
            .child(div().flex_none().h(self.user_panel_clearance()))
            .child(self.resize_handle(Edge::Channels, cx))
    }

    /// What heads the channel list: the open server's name, or "Home" (its search leads the list below).
    pub(crate) fn channel_list_head(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.in_favorites() {
            return server_name_button("Favorites".into(), cx).into_any_element();
        }
        match self.guild() {
            Some(guild) => server_name_button(guild.name.clone().into(), cx).into_any_element(),
            None => ui::line("Home")
                .flex_1()
                .px_2()
                .text_size(ui::TEXT_MD)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::strong(cx))
                .into_any_element(),
        }
    }

    // A category folds from its label: height and fade together on the fold curve, the chevron turns.
    // Folded, its unread and selected channels stay in view.
    pub(super) fn section(&mut self, category: Channel, rows: Vec<ChannelRow>, spaced: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let folded = self.folded.contains(&category.id);
        let [fold, chevron] = self.motions.frame(
            [
                (Part::Fold(category.id.clone()), Motion::new(Target::default().opacity(if folded { 0. } else { 1. })).transition(motion::fold())),
                // The chevron's angle in degrees rides the x channel; it is read back, never applied as an offset.
                (Part::Chevron(category.id.clone()), Motion::new(Target::default().x(if folded { -90. } else { 0. })).transition(hover_ease())),
            ],
            window,
            cx,
        );
        let degrees = chevron.target().x_px();
        let target_ink = palette::accent_ink(cx);
        let id = category.id.clone();
        let label = div()
            .id(SharedString::from(format!("section-{}", category.id)))
            .child(Nav::Caret.icon().size(px(CARET_SIZE)).rotate(Radians(degrees.to_radians())))
            .child(ui::truncating(format!("category-name-{}", category.id), category.name.clone()))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.folded.remove(&id) {
                    this.folded.insert(id.clone());
                }
                this.redraw(crate::part::Column::Nav, cx);
            }))
            .drag_over::<crate::drag::ChannelDrag>(move |s, _, _, _| s.text_color(target_ink))
            .on_mouse_down(gpui_kit::MouseButton::Right, {
                let category = category.clone();
                cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| {
                    let Some(guild_id) = this.guild_id.clone() else { return };
                    this.open_channel_menu(category.clone(), guild_id, e.position, cx);
                })
            });
        let label = Board::head::<_, crate::drag::ChannelDrag, _>(label, channel_board, channel_list(Some(&category)), cx);
        let can_add = self.guild_id.as_ref().is_some_and(|g| self.can_arrange_channels(g));
        let group: SharedString = format!("cathead-{}", category.id).into();
        let head = category_head(group.clone(), label, false, cx).when(can_add, |head| {
            let category = category.clone();
            head.child(cat_add(SharedString::from(format!("section-add-{}", category.id)), Some(group), "Create channel", cx).on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    let Some(guild_id) = this.guild_id.clone() else { return };
                    cx.stop_propagation();
                    this.open_create_channel(guild_id, 0, Some(category.id.clone()), window, cx);
                }),
            ))
        });
        let alpha = fold.target().alpha();
        let gone = folded && alpha <= 0.05;
        let rows = rows.into_iter().filter(|row| !gone || row.keep).map(|row| match row.keep {
            true => row.element,
            false => div().opacity(alpha).child(row.element).into_any_element(),
        });
        ui::col().when(spaced, |g| g.pt(px(ui::density_pad(SECTION_GAP)))).gap(px(row_gap())).child(head).children(rows).into_any_element()
    }

    pub(super) fn channel_rows(&self, channel: &Channel, guild: &Guild, window: &mut Window, cx: &mut Context<Self>) -> Vec<ChannelRow> {
        let selected = self.channel.as_ref().is_some_and(|c| c.id == channel.id);
        let settings = self.guild_settings.get(&guild.id);
        let muted = settings.is_some_and(|s| s.channel_muted(&channel.id) || channel.parent_id.as_ref().is_some_and(|p| s.channel_muted(p)));
        // Discord's "Hide Muted Channels" keeps the open channel and pinned channels visible.
        if muted && !selected && settings.is_some_and(|s| s.hide_muted && !s.pinned(&channel.id)) {
            return Vec::new();
        }
        let is_voice = channel.kind == ChannelKind::Voice;
        let in_call = self.call.as_ref().is_some_and(|c| c.channel.id == channel.id);
        // A voice channel's chat marks it unread only while you're in it; mentions show always, as Discord's.
        let unread = self.unread(&channel.id) && !muted && (!is_voice || in_call);
        let mentions = self.mention_count(&channel.id);
        let hovered = self.hovered_row.as_deref() == Some(channel.id.as_str());
        // A voice channel's buttons show on hover only, over its call timer.
        let actions_shown = hovered || (selected && !is_voice);
        let state = RowState { selected, unread, muted, strong: in_call };
        let elapsed = self.call.as_ref().filter(|c| !actions_shown && c.channel.id == channel.id && c.connected).and_then(|_| self.voice_elapsed(&channel.id)).map(|secs| {
            div().flex_none().text_size(ui::TEXT_XS).font_weight(FontWeight::NORMAL).text_color(palette::positive_ink(cx)).child(crate::call::clock(secs))
        });
        let group: SharedString = format!("ch-{}", channel.id).into();
        // An unsent draft outranks the muted bell in the meta slot.
        let meta = match self.drafts.has(&channel.id) {
            true => Some(draft_mark()),
            false => muted.then(|| Nav::BellSlash.icon().size_4()),
        };
        let faint = palette::faint(cx);
        let topic = guild.stage(&channel.id).map(|s| s.topic.clone());
        let carried = self.drag_motion.row(&channel.id);
        let welcomes = channel.kind == ChannelKind::Voice && self.welcomes_voice_drop(channel, guild);
        let row = channel_row(SharedString::from(channel.id.clone()), group.clone(), state, cx).children(self.drag_motion.spot(&channel.id));
        // A voice channel's status (or the way to set one) is a second line inside the row's fill and hit area.
        let status_line = self.voice_row_status(channel, cx);
        let row = match carried {
            RowDrag::Hole => hole(row.bg(gpui_kit::transparent_black()), cx),
            _ => {
                let line = ui::row()
                    .w_full()
                    .gap_2()
                    .child(channel_glyph(channel).size(ui::ICON_LG).flex_none().text_color(match (welcomes, selected) {
                        (true, _) => palette::accent_ink(cx),
                        (false, true) => palette::muted(cx),
                        (false, false) => faint,
                    }))
                    .child(channel_label(channel, topic.clone(), cx))
                    .children(elapsed)
                    .children(meta.filter(|_| !actions_shown).map(|icon| ui::row().items_stretch().flex_none().text_color(faint).child(icon)))
                    .when(actions_shown, |r| r.child(self.row_actions(channel, guild, cx)))
                    .when(mentions > 0, |r| r.child(ui::count_badge(mentions)));
                let row = row.when(carried == RowDrag::Hidden, |r| r.invisible()).when(welcomes, |r| r.bg(palette::accent_soft(cx)).text_color(palette::strong(cx)));
                match status_line {
                    Some(status) => row.h_auto().py(px((row_height() - 20.) / 2.)).flex_col().items_stretch().gap_0p5().child(line.min_h_5()).child(status),
                    None => row.when(topic.is_some(), |r| r.h(px(STAGE_ROW_HEIGHT))).child(line.h_full()),
                }
            }
        };
        let row = self.track_row_hover(self.channel_draggable(row, channel, guild, cx), &channel.id, cx);
        let keep = selected || unread;
        if !is_voice {
            let open = channel.clone();
            let row = row
                .on_click(cx.listener(move |this, _, window, cx| this.open_channel(open.clone(), window, cx)))
                .on_mouse_down(gpui_kit::MouseButton::Right, {
                    let (channel, guild_id) = (channel.clone(), guild.id.clone());
                    cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_channel_menu(channel.clone(), guild_id.clone(), e.position, cx))
                });
            let mut rows = vec![ChannelRow { keep, element: row.into_any_element() }];
            rows.extend(self.thread_rows(channel, guild, cx));
            return rows;
        }
        let join = channel.clone();
        let row = self
            .voice_drop_zone(row, channel.id.clone().into(), channel, cx)
            .on_click(cx.listener(move |this, _, window, cx| {
                // With mentions waiting, Discord joins and opens the chat.
                if this.mention_count(&join.id) > 0 {
                    this.show_call(join.clone(), window, cx);
                    return this.open_call_chat(join.clone(), window, cx);
                }
                let in_call = this.call.as_ref().map(|c| c.channel.id.as_str());
                match voice_row_click(in_call, &join.id) {
                    VoiceRowClick::Join => this.join_voice(join.clone(), cx),
                    VoiceRowClick::OpenStage => this.show_call(join.clone(), window, cx),
                }
            }))
            .on_mouse_down(gpui_kit::MouseButton::Right, {
                let (channel, guild_id) = (channel.clone(), guild.id.clone());
                cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_channel_menu(channel.clone(), guild_id.clone(), e.position, cx))
            });
        let mut rows = vec![ChannelRow { keep: keep || in_call, element: row.into_any_element() }];
        // A carried channel takes its people along in the ghost.
        if carried == RowDrag::Rest {
            rows.extend(self.voice_rows(channel, guild, in_call, window, cx).into_iter().map(|element| ChannelRow { keep: in_call, element }));
        }
        rows
    }

    // Rows show their hover-only controls from this, not from `group_hover`: gpui resolves group hover
    // only while painting, so a `display` toggled by it paints children that were never prepainted.
    pub(super) fn track_row_hover(&self, row: Stateful<Div>, id: &str, cx: &mut Context<Self>) -> Stateful<Div> {
        let id = id.to_owned();
        row.on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            // Hover only shows the row's controls: like Discord's client, it fetches nothing.
            if !*hovered && this.hovered_row.as_deref() != Some(id.as_str()) {
                return;
            }
            this.hovered_row = hovered.then(|| id.clone());
            this.redraw(crate::part::Column::Nav, cx);
        }))
    }

    // Open Chat (voice channels you may join), invite and options, shown in the row's meta slot while
    // it's hovered or selected.
    pub(super) fn row_actions(&self, channel: &Channel, guild: &Guild, cx: &mut Context<Self>) -> Div {
        let can_chat = channel.kind == ChannelKind::Voice && self.can_in(&channel.id, crate::discord::perm::CONNECT);
        let open_chat = can_chat.then(|| {
            let open = channel.clone();
            row_action(SharedString::from(format!("open-chat-{}", channel.id)), Icon::new(Glyph::ChatRound), "Open Chat", cx).on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open_call_chat(open.clone(), window, cx);
            }))
        });
        let (gid, cid) = (guild.id.clone(), channel.id.clone());
        let (menu_channel, menu_guild) = (channel.clone(), guild.id.clone());
        let invite = row_action(SharedString::from(format!("invite-{}", channel.id)), Icon::new(Glyph::GroupPlus), "Invite people", cx)
            .on_click(cx.listener(move |this, _, window, cx| this.show_invite(gid.clone(), cid.clone(), window, cx)));
        let options = row_action(SharedString::from(format!("options-{}", channel.id)), Icon::new(Glyph::Settings), "Edit channel", cx).on_mouse_down(
            gpui_kit::MouseButton::Left,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open_channel_settings(menu_channel.clone(), menu_guild.clone(), window, cx);
            }),
        );
        ui::row().items_stretch()
            .flex_none()
            .gap_0p5()
            .children(open_chat)
            .child(invite)
            .child(options)
    }

    // With Manage Channels, a channel can be dragged above or below another, or onto a category.
    pub(super) fn channel_draggable(&self, row: Stateful<Div>, channel: &Channel, guild: &Guild, cx: &mut Context<Self>) -> Stateful<Div> {
        if !self.can_arrange_channels(&guild.id) {
            return row;
        }
        if self.guild_settings.get(&guild.id).is_some_and(|s| s.pinned(&channel.id)) {
            let (this, guild_id, channel_id) = (cx.weak_entity(), guild.id.clone(), channel.id.clone());
            return row.on_drag(crate::drag::PinnedDrag, move |_, _, _, cx| {
                this.update(cx, |this, cx| this.ask_unpin(guild_id.clone(), channel_id.clone(), cx)).ok();
                cx.new(|_| crate::drag::NoGhost)
            });
        }
        let drag = crate::drag::ChannelDrag { channel_id: channel.id.clone(), guild_id: guild.id.clone() };
        let people = match channel.kind {
            ChannelKind::Voice => self.voice_people(channel, guild),
            _ => Vec::new(),
        };
        // The ghost is the row and its people, 30px each under it 1px apart.
        let size = point(px(self.channels_width - 16.), px(row_height() + people.len() as f32 * 31.));
        let look = crate::drag::Look::Channel { channel: Box::new(channel.clone()), people };
        let ghost = crate::drag::pick_up(cx.weak_entity(), channel.id.clone().into(), look, size, 1.05, 0., true);
        let row = row.on_drag(drag, Board::lift(cx.weak_entity(), channel_board, channel.id.clone(), ghost));
        Board::target(row, channel_board, channel_list(self.guild().and_then(|g| g.channels.iter().find(|c| Some(&c.id) == channel.parent_id.as_ref()))), channel.id.clone(), |_: &crate::drag::ChannelDrag| false, cx)
    }

    // Discord's dialog for a drag on a pinned row: unpin it, or leave it.
    pub(super) fn ask_unpin(&mut self, guild_id: String, channel_id: String, cx: &mut Context<Self>) {
        let body = "Unpin this channel before reordering. Pinning is only for you, but channel order applies to everyone.";
        self.confirm("Trying to reorder a pinned channel?", body, "Unpin channel", move |this, _, cx| {
            let flags = this.settings_for(&guild_id).channel_flags(&channel_id) & !channel_flag::FAVORITED;
            this.set_channel_flags(guild_id.clone(), &channel_id, flags, cx);
        }, cx);
        if let Some(dialog) = &mut self.dialog {
            dialog.danger = false;
        }
    }

    // Pinned channels or Active Now under a static head: no fold, no add, not in any drag list.
    pub(super) fn static_section(&mut self, id: &'static str, title: &'static str, channels: &[Channel], guild: &Guild, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let head = category_head(id.into(), div().id(id).child(title), true, cx);
        let mut rows = Vec::new();
        for channel in channels {
            rows.extend(self.channel_rows(channel, guild, window, cx).into_iter().map(|row| row.element));
        }
        ui::col().gap(px(row_gap())).child(head).children(rows)
    }

    // The channel list's sections as drawn: what's open, in a call or mentioning us stays in view
    // under opt-in, and so does a channel whose joined thread is unread or mentions us.
    pub(crate) fn sidebar_sections(&self, guild: &Guild) -> Vec<(Section, Vec<Channel>)> {
        let open = self.channel.as_ref().map(|c| c.id.as_str());
        let in_call = self.call.as_ref().map(|c| c.channel.id.as_str());
        let rules = guild.rules_channel_id.as_deref();
        let keep = |c: &Channel| open == Some(c.id.as_str()) || in_call == Some(c.id.as_str()) || rules == Some(c.id.as_str()) || self.mention_count(&c.id) > 0;
        let noticed = |t: &Channel| self.unread(&t.id) || self.mention_count(&t.id) > 0;
        channel_sections(guild, self.my_id(), self.guild_settings.get(&guild.id), keep, noticed)
    }

    // DMs as listed and stepped through: pinned first, each group newest first.
    pub(crate) fn dm_order(&self) -> Vec<Channel> {
        let settings = self.guild_settings.get("@me");
        pinned_first(&self.dms, |id| settings.is_some_and(|s| s.pinned(id)))
    }

    // "Move to Archive": the tag a channel carries over a row in another category.
    pub(super) fn category_move(&self, moved: &str, over: &str) -> Option<SharedString> {
        let channels = &self.guild()?.channels;
        let parent_of = |id: &str| channels.iter().find(|c| c.id == id).and_then(|c| c.parent_id.clone());
        let to = parent_of(over)?;
        if parent_of(moved).as_ref() == Some(&to) {
            return None;
        }
        let name = &channels.iter().find(|c| c.id == to)?.name;
        Some(format!("Move to {name}").into())
    }

    // The threads you've joined (and the one you have open), hung under their channel on a hairline
    // tree: each draws its branch from the row above, and all but the last carry the trunk on.
    pub(super) fn thread_rows(&self, channel: &Channel, guild: &Guild, cx: &mut Context<Self>) -> Vec<ChannelRow> {
        let open = self.channel.as_ref().map(|c| c.id.clone());
        let threads: Vec<&Channel> = guild
            .threads
            .iter()
            .filter(|t| t.parent_id.as_ref() == Some(&channel.id))
            .filter(|t| open.as_ref() == Some(&t.id) || t.thread.as_ref().is_some_and(|m| m.joined && !m.archived))
            .collect();
        let count = threads.len();
        let line = match crate::material::active() {
            true => palette::faint(cx).opacity(0.5),
            false => ui::mix(palette::faint(cx), palette::surface(palette::Level::Sidebar, cx), 0.5),
        };
        threads
            .into_iter()
            .enumerate()
            .map(|(i, thread)| {
                let selected = open.as_ref() == Some(&thread.id);
                let unread = self.unread(&thread.id);
                let target = thread.clone();
                let (menu_thread, guild_id) = (thread.clone(), guild.id.clone());
                // The first branch starts just below its parent, never touching the parent's fill.
                let gap = if i == 0 { -4. } else { row_gap() };
                // lint-allow(custom-mark): ui::divider draws straight rules; this is an elbow that joins the thread to its channel.
                let branch = div().absolute().left_4().top(px(-gap)).w_3().h(px(THREAD_HEIGHT / 2. + gap)).border_l(px(THICK_LINE)).border_b(px(THICK_LINE)).rounded_bl_lg().border_color(line);
                let trunk = (i + 1 < count).then(|| div().absolute().left_4().top(px(THREAD_HEIGHT / 2. - 8.)).bottom_0().w(px(THICK_LINE)).bg(line));
                let state = RowState { selected, unread, muted: false, strong: false };
                let row = channel_row(SharedString::from(format!("thread-{}", thread.id)), format!("thread-{}", thread.id).into(), state, cx)
                    .h(px(THREAD_HEIGHT))
                    .pl(px(THREAD_INDENT))
                    .child(branch)
                    .children(trunk)
                    .child(ui::truncating(format!("thread-name-{}", thread.id), thread.name.clone()).flex_1())
                    .on_click(cx.listener(move |this, _, window, cx| this.open_channel(target.clone(), window, cx)))
                    .on_mouse_down(gpui_kit::MouseButton::Right, cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_thread_menu(menu_thread.clone(), guild_id.clone(), e.position, cx)));
                ChannelRow { keep: selected || unread, element: row.into_any_element() }
            })
            .collect()
    }
}
