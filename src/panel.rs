// The side panel (design/css/panels.css .panel): one right-hand frame for the member list and search
// results, with a fixed head and a scrolling body.
use std::collections::HashMap;

use gpui_kit::{
    AnyElement, App, Context, Div, FontWeight, Radians, SharedString, Window, linear_color_stop, linear_gradient, relative,
    component::{ActiveTheme, Icon},
    div, img, prelude::*, px,
};
use gpui_motion_kit::core::{Motion, Target};

use crate::{
    Part, Waffle, Edge, RightPanel,
    discord::{ActivityKind, FeedEntry, ListItem, Member, Status},
    icons::Glyph,
    layout::{decorated_avatar, nameplate_art, nameplate_tint, playing_avatar},
    motion, palette,
    sidebar::{Nav, activity_glyph, category_action, category_head, channel_glyph, hover_ease},
    ui::{self, Squircle},
};

// The section shows this many cards; its chevron opens the rest.
const ACTIVITY_SHOWN: usize = 3;

impl Waffle {
    // Whether the side panel is on screen: a server's (unless hidden), or search results anywhere.
    pub(crate) fn panel_shown(&self) -> bool {
        match self.right_panel {
            RightPanel::Search(_) | RightPanel::ModView(_) => true,
            RightPanel::Hidden => false,
            RightPanel::Members => self.guild_id.is_some() || self.open_group().is_some() || self.dm_recipient().is_some(),
            RightPanel::Activity => self.guild_id.is_some(),
        }
    }

    // The side panel's width while it shows. A 1:1 DM's "members" are its person's profile.
    pub fn panel_column_width(&self) -> Option<f32> {
        match &self.right_panel {
            _ if !self.panel_shown() => None,
            RightPanel::Hidden => None,
            RightPanel::Members | RightPanel::Activity => Some(self.panel_width),
            RightPanel::Search(_) => Some(crate::search_results::PANEL_WIDTH),
            RightPanel::ModView(_) => Some(self.panel_width.max(crate::mod_view::PANEL_WIDTH)),
        }
    }

    // 0 hidden to 1 shown: the panel's width rides one critically damped spring, the call chat's, so a toggle
    // mid-slide reverses from where it is and reduced motion lands at once.
    fn panel_reveal(&self, window: &mut Window, cx: &mut App) -> f32 {
        let target = if self.panel_shown() { 1. } else { 0. };
        gpui_kit::base::spring("panel-reveal", target, gpui_kit::base::Spring::new(ui::DUR_PANEL), window, cx)
    }

    // The panel column inside a clip at the window's right edge: it sits against the clip's right side, so it slides in with the edge, and keeps its full width while the
    // clip grows, so its rows never reflow. Closing keeps drawing the members it was showing until it's gone.
    pub(crate) fn slid_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let reveal = self.panel_reveal(window, cx);
        if reveal <= 0.001 {
            return None;
        }
        let width = self.panel_column_width().unwrap_or(self.panel_width);
        let part = self.part(crate::part::Column::Panel, gpui_kit::StyleRefinement::default().w(px(width)).h_full().flex_none(), cx);
        Some(ui::row().relative().w(px(width * reveal)).h_full().flex_none().overflow_hidden().child(div().absolute().top_0().right_0().h_full().w(px(width)).child(part)).into_any_element())
    }

    pub fn render_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.close_mod_view_if_barred();
        // Hidden only follows the members, so a closing panel keeps drawing them while it slides out.
        let width = self.panel_column_width().unwrap_or(self.panel_width);
        if matches!(self.right_panel, RightPanel::Members | RightPanel::Hidden) && self.dm_recipient().is_some() {
            return self.dm_profile(window, cx);
        }
        // Its header sits on the window's top line beside the channel header's: "Members", or the search's
        // query with a way back to the members.
        let (head, body, foot): (Div, AnyElement, Option<AnyElement>) = match &self.right_panel {
            RightPanel::Members | RightPanel::Hidden if self.guild_id.is_none() => (self.panel_head("Members".into(), None, Vec::new(), false, cx), self.group_member_list(cx), self.group_invite_foot(cx)),
            RightPanel::Members | RightPanel::Hidden => (self.panel_head("Members".into(), None, Vec::new(), false, cx), self.member_list(window, cx), None),
            RightPanel::Activity => (self.panel_head("Activity".into(), None, Vec::new(), true, cx), self.activity_cards(usize::MAX, cx), None),
            RightPanel::Search(results) => {
                let head = self.panel_frame(cx).child(self.search_head(results, self.back_button(cx), cx));
                (head, self.search_body(results, cx), self.search_pager(results, cx))
            }
            RightPanel::ModView(view) => (self.mod_view_head(view, cx), self.mod_view_body(view, cx), None),
        };
        let line = palette::line(cx);
        let scroll = self.panel_scroll.clone();
        Some(
            ui::col()
                .relative()
                .w(px(width))
                .h_full()
                .flex_none()
                // The sidebar's tone: the step from the feed's tone is the only edge it needs.
                .bg(palette::surface(palette::Level::Sidebar, cx))
                .text_color(palette::text(cx))
                .text_size(ui::TEXT_SM)
                // The list stops under the header, a hard edge with the sidebar's hairline once it has scrolled.
                .child(ui::hard_edge(move || ui::scrolled(&scroll), cx).child(head))
                .child(ui::ScrollArea::new(ui::col().id("right-panel").pt_2().px_2().pb_2().gap_0p5().child(body), &self.panel_scroll))
                .child(self.resize_handle(Edge::Panel, cx))
                .children(foot.map(|pager| div().flex_none().py_2().border_t_1().border_color(line).child(pager)))
                .into_any_element(),
        )
    }

    // The column's header row, clear of Windows' caption buttons at the window's top right: an optional
    // leading control, the title, the panel's own tools, and with `close` the way back to the members.
    pub(crate) fn panel_head(&self, title: SharedString, lead: Option<ui::Button>, tools: Vec<ui::Button>, close: bool, cx: &mut Context<Self>) -> Div {
        let close = close.then(|| self.back_button(cx));
        self.panel_frame(cx)
            .children(lead)
            .child(ui::line(title).flex_1().text_size(ui::TEXT_MD).font_weight(FontWeight::SEMIBOLD))
            .children(tools)
            .children(close)
    }

    pub(crate) fn panel_frame(&self, cx: &App) -> Div {
        ui::row()
            .h(px(crate::layout::HEADER_HEIGHT))
            .pl_4()
            .pr(px(PANEL_END_INSET + crate::window_controls::WIDTH))
            .gap_2()
            .window_control_area(gpui_kit::WindowControlArea::Drag)
            .text_color(palette::strong(cx))
    }

    fn back_button(&self, cx: &mut Context<Self>) -> ui::Button {
        ui::Button::new("close-panel").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).icon(Glyph::X).tooltip_text("Back to members").occlude().on_click(cx.listener(|this, _, _, cx| {
            this.right_panel = RightPanel::Members;
            cx.notify(); // lint-allow(global-notify): the panel's width and header follow which panel is open
        }))
    }

    // A group DM's members, you among them, the owner crowned. Its people have no roles or server: just online
    // and offline, as Discord lists a group.
    // Where the side panel's member rows sit: its width less the list's inset, on the sidebar tone.
    fn side_spot(&self, cx: &App) -> RowSpot {
        RowSpot { width: self.panel_width - MEMBER_LIST_INSET, surface: palette::surface(palette::Level::Sidebar, cx), profile_x: None }
    }

    fn group_member_list(&self, cx: &mut Context<Self>) -> AnyElement {
        // Before READY nobody's presence is known, and listing them all as offline would say otherwise.
        let Some(group) = self.open_group().filter(|_| self.live()) else { return div().into_any_element() };
        let people = self.me.iter().chain(&group.recipients);
        let rows: Vec<AnyElement> = people
            .map(|user| Member { user: user.clone(), roles: Vec::new(), presence: self.presences.get(&user.id).cloned().unwrap_or_default(), boosting_since: None })
            .map(|member| self.member_row(&member, self.side_spot(cx), cx))
            .collect();
        ui::col().gap_0p5().child(group_label(format!("Members · {}", rows.len()), cx)).children(rows).into_any_element()
    }

    // "Invite to Group DM" under the list while the group has room.
    fn group_invite_foot(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let group = self.open_group()?.clone();
        if group.recipients.len() + 1 >= crate::group_dm::LIMIT {
            return None;
        }
        let invite = ui::Button::new("group-invite").variant(ui::ButtonVariant::Ghost)
            .w_full()
            .child(Icon::new(Glyph::GroupPlus).size(ui::ICON_LG))
            .child("Invite to Group DM")
            .on_click(cx.listener(move |this, _, window, cx| this.open_group_picker(Some(group.clone()), window, cx)));
        Some(div().px_2().child(invite).into_any_element())
    }

    // Who is here, grouped by top role, then online and offline (navigation.css .members).
    fn member_list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let activity = self.activity_section(window, cx);
        let Some(guild) = self.guild() else { return div().into_any_element() };
        let Some(list) = self.member_lists.get(&guild.id).filter(|l| !l.is_empty()) else { return members_loading(cx).into_any_element() };
        let mut first = activity.is_none();
        let spot = self.side_spot(cx);
        // Each entry with its height: a group's label (12px more after the first), or a member's row.
        let entries: Vec<(f32, Entry)> = list
            .iter()
            .filter_map(|item| match item {
                ListItem::Group { id, count } => {
                    let count = self.member_groups.get(&guild.id).and_then(|g| g.get(id)).copied().unwrap_or(*count);
                    let name = match id.as_str() {
                        "online" => "Online".to_string(),
                        "offline" => "Offline".to_string(),
                        "" => return None,
                        role => guild.roles.iter().find(|r| r.id == role).map(|r| r.name.clone()).unwrap_or_else(|| "Members".into()),
                    };
                    let spaced = !std::mem::take(&mut first);
                    Some((GROUP_LABEL + if spaced { GROUP_SPACE } else { 0. }, Entry::Group(format!("{name} · {count}"), spaced)))
                }
                ListItem::Member(member) => Some((member_row_height(), Entry::Member(member))),
            })
            .collect();
        // A server's list runs to thousands: only the entries in view are built (see `ui::visible_rows`). Where
        // the block starts under the activity cards is read as it paints, so the next frame knows.
        let gap = ui::density_pad(2.);
        let top = window.use_keyed_state("member-rows-top", cx, |_, _| None::<f32>);
        let scroll = self.panel_scroll.clone();
        let heights: Vec<f32> = entries.iter().map(|(height, _)| *height).collect();
        let (range, before, after) = match *top.read(cx) {
            Some(at) => ui::visible_rows(&heights, gap, at, -f32::from(scroll.offset().y), f32::from(scroll.bounds().size.height), ROWS_OVERSCAN),
            None => ui::visible_rows(&heights, gap, 0., 0., ROWS_FIRST_PAINT, 0.),
        };
        let measure = gpui_kit::canvas(|_, _, _| {}, move |bounds, _, _, cx| {
            // Painted where it scrolled to; the content's own position takes the offset back out.
            let at = f32::from(bounds.top() - scroll.bounds().top() - scroll.offset().y);
            top.update(cx, |top, _| *top = Some(at));
        });
        let spacer = |height: f32| (height > 0.).then(|| div().flex_none().h(px(height)));
        let rows = entries[range].iter().map(|(_, entry)| match entry {
            Entry::Group(label, spaced) => div().when(*spaced, |d| d.pt_3()).child(group_label(label.clone(), cx)).into_any_element(),
            Entry::Member(member) => self.member_row(member, spot, cx),
        });
        let block = ui::col().relative().gap(px(gap)).child(measure.absolute().size_0()).children(spacer(before)).children(rows.collect::<Vec<_>>()).children(spacer(after));
        ui::col().gap(px(gap)).children(activity).child(block).into_any_element()
    }

    pub(crate) fn load_activity_feed(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            // No feed (an error, or none for this account) leaves the section out.
            let Ok(feed) = client.activity_feed().await else { return };
            this.update(cx, |this, cx| {
                this.activity_feed = feed;
                this.redraw(crate::part::Column::Panel, cx);
            })
            .ok();
        })
        // lint-allow(detached-task): ends when the feed request returns, and holds only a WeakEntity.
        .detach();
    }

    // The feed's entries by members of the open server, each with that member, in Discord's order.
    fn guild_activity(&self) -> Vec<(&FeedEntry, &Member)> {
        let Some(list) = self.guild_id.as_ref().and_then(|g| self.member_lists.get(g)) else { return Vec::new() };
        let members: HashMap<&str, &Member> = list
            .iter()
            .filter_map(|item| match item {
                ListItem::Member(m) => Some((m.user.id.as_str(), m)),
                ListItem::Group { .. } => None,
            })
            .collect();
        self.activity_feed.iter().filter_map(|e| Some((e, *members.get(e.author_id.as_str())?))).collect()
    }

    // "Activity · N" over the member groups: folds from its label like a channel category, and its
    // chevron opens every card in the panel.
    fn activity_section(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Div> {
        let count = self.guild_activity().len();
        if count == 0 {
            return None;
        }
        let key = format!("activity:{}", self.guild_id.clone().unwrap_or_default());
        let folded = self.folded.contains(&key);
        let [fold, chevron] = self.motions.frame(
            [
                (Part::Fold(key.clone()), Motion::new(Target::default().opacity(if folded { 0. } else { 1. })).transition(motion::fold())),
                // The chevron's angle in degrees rides the x channel; it is read back, never applied as an offset.
                (Part::Chevron(key.clone()), Motion::new(Target::default().x(if folded { -90. } else { 0. })).transition(hover_ease())),
            ],
            window,
            cx,
        );
        let label = div()
            .id("activity-section")
            .child(Nav::Caret.icon().size(px(crate::sidebar::CARET_SIZE)).rotate(Radians(chevron.target().x_px().to_radians())))
            .child(format!("Activity · {count}"))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.folded.remove(&key) {
                    this.folded.insert(key.clone());
                }
                this.redraw(crate::part::Column::Panel, cx);
            }));
        let all = category_action("activity-all", Icon::new(Glyph::ChevronRight), None, "Show all activity", cx).on_click(cx.listener(|this, _, _, cx| {
            this.right_panel = RightPanel::Activity;
            cx.notify(); // lint-allow(global-notify): the panel's width and header follow which panel is open
        }));
        let alpha = fold.target().alpha();
        let cards = (!folded || alpha > 0.05).then(|| div().opacity(alpha).child(self.activity_cards(ACTIVITY_SHOWN, cx)));
        Some(ui::col().gap_0p5().child(category_head("activity-head".into(), label, false, cx).child(all)).children(cards))
    }

    fn activity_cards(&self, limit: usize, cx: &mut Context<Self>) -> AnyElement {
        let now = chrono::Utc::now().timestamp_millis();
        let cards: Vec<AnyElement> = self.guild_activity().into_iter().take(limit).enumerate().map(|(i, (entry, member))| self.feed_card(i, entry, member, now, cx)).collect();
        ui::col().gap_2().children(cards).into_any_element()
    }

    // One card: who (avatar, name in their role's colour), what, and when, with its art on the right.
    fn feed_card(&self, i: usize, entry: &FeedEntry, member: &Member, now: i64, cx: &mut Context<Self>) -> AnyElement {
        let dark = cx.theme().is_dark();
        let (strong, muted) = (palette::strong(cx), palette::muted(cx));
        let (role_ink, _) = crate::chat::role_marks(self.prefs.role_style, self.role_color(self.guild_id.as_ref(), &member.user.id, Some(&member.roles), dark));
        let name_ink = role_ink.unwrap_or(strong);
        let when = if entry.live { "Now".to_owned() } else { ago(now - entry.at) };
        let meta = ui::row()
            .gap_1()
            .text_size(ui::TEXT_XS)
            .font_weight(FontWeight::NORMAL)
            .text_color(muted)
            .children(activity_glyph(entry.kind).map(|icon| icon.size(ui::ICON_SM).flex_none().when(entry.live, |i| i.text_color(palette::status_in(Status::Online, cx)))))
            .child(div().flex_none().child(when))
            .when_some(entry.streak_days, |m, days| m.child(div().w_1p5().flex_none()).child(Icon::default().data(BOLT).size_3p5().flex_none()).child(ui::line(format!("{days}× streak"))));
        let image = entry.image.clone().or_else(|| self.app_icon(entry.app_id.as_deref()?, cx));
        let art = match image {
            Some(url) => img(url).size_11().flex_none().object_fit(gpui_kit::ObjectFit::Cover).squircle(ui::RADIUS_MD).into_any_element(),
            None => ui::row()
                .size_11()
                .flex_none()
                .squircle(ui::RADIUS_MD)
                .bg(palette::selected(cx))
                .justify_center()
                .text_color(muted)
                .children(activity_glyph(entry.kind).or_else(|| activity_glyph(ActivityKind::Playing)).map(|icon| icon.size(ui::ICON_XL)))
                .into_any_element(),
        };
        ui::Card::interactive(SharedString::from(format!("activity-{i}")), cx)
            .on_page(cx)
            .gap_2p5()
            .px_2p5()
            .py_2()
            .child(
                ui::col().min_h_0()
                    .flex_1()
                    .gap_px()
                    .child(ui::row().gap_1p5().text_size(ui::TEXT_XS).child(ui::Avatar::new(member.user.avatar.clone(), 14.).flex_none()).child(ui::line(member.user.name.clone()).text_color(name_ink)))
                    .child(ui::line(entry.title.clone()).font_weight(FontWeight::MEDIUM).text_color(strong))
                    .child(meta),
            )
            .child(art)
            .on_click({
                let user = member.user.clone();
                cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| this.open_profile(user.clone(), beside_panel(event.position()), cx))
            })
            .into_any_element()
    }

    // A member (.mem): avatar with status, the name in their top role's colour, one line of activity.
    // Offline dims the avatar and drops the name to faint; the open profile's member keeps the fill.
    pub(crate) fn member_row(&self, member: &Member, spot: RowSpot, cx: &mut Context<Self>) -> AnyElement {
        let dark = cx.theme().is_dark();
        let offline = member.presence.status == Status::Offline;
        let (strong, hover) = (palette::strong(cx), palette::hover(cx));
        let (role_ink, dot) = crate::chat::role_marks(self.prefs.role_style, self.role_color(self.guild_id.as_ref(), &member.user.id, Some(&member.roles), dark));
        // Offline keeps its role colour; the whole row fades instead, as Discord's does.
        let name_ink = role_ink.unwrap_or(strong);
        let name_style = member.user.name_style.as_ref().filter(|_| self.prefs.display_name_styles);
        let surface = spot.surface;
        let owner = match self.guild() {
            Some(guild) => guild.owner_id.as_ref() == Some(&member.user.id),
            None => self.open_group().is_some_and(|g| g.props.owner_id.as_ref() == Some(&member.user.id)),
        };
        let selected = self.profile_card.as_ref().is_some_and(|p| !p.panel && p.user.id == member.user.id);
        let crown = palette::warning_ink(cx);
        let boost = palette::boost_ink(cx);
        let boosting = member.boosting_since.as_deref().and_then(boost_date);
        let name_line = |key: String| {
            // Discord's order and spacing: the name, then its tag, APP, crown and gem 4px apart.
            ui::row()
                .gap_1()
                .child(crate::name_fonts::styled(ui::truncating(key, member.user.name.clone()).text_size(ui::TEXT_MD).line_height(ui::LINE_SM).font_weight(FontWeight::MEDIUM).text_color(name_ink), name_style))
                .children(dot.map(crate::chat::role_dot))
                .children(member.user.tag.as_ref().map(crate::chat::guild_tag))
                .when(member.user.bot, |r| r.child(ui::Badge::new("App").variant(ui::BadgeVariant::Accent)))
                .when(owner, |r| {
                    let id = SharedString::from(format!("member-owner-{}", member.user.id));
                    r.child(div().id(id).flex_none().child(Icon::new(Glyph::Crown).size(ui::ICON_SM).text_color(crown)).tooltip(ui::Tooltip::text("Server Owner")))
                })
                .children(boosting.clone().map(|since| {
                    div()
                        .id(SharedString::from(format!("member-boost-{}", member.user.id)))
                        .flex_none()
                        .child(Icon::new(Glyph::Gem).size(ui::ICON_SM).text_color(boost))
                        .tooltip(move |window, cx| ui::Tooltip::new(format!("Server boosting since {since}")).build(window, cx))
                }))
        };
        let status = self.member_status(member);
        let names = |key: String| ui::col().child(name_line(key)).children(status.clone().map(|s| s.line(cx)));
        // The row's fill in each state, laid solid over the panel, for the nameplate's mask to wash in.
        let fill = |wash: gpui_kit::Hsla| ui::mix(surface, wash.opacity(1.), wash.a);
        let fills = match selected {
            true => (fill(palette::selected(cx)), None),
            false => (surface, Some(fill(hover))),
        };
        // Discord animates the avatar, decoration and nameplate while the row is hovered or its profile is open.
        let animate = (selected || self.member_hover.as_deref() == Some(member.user.id.as_str())) && crate::presence::may_animate();
        let row = PlateRow { group: MEMBER_ROW_GROUP.into(), height: member_row_height(), width: spot.width, fills, selected, animate, dark };
        let plate = member.user.nameplate.as_ref().map(|plate| row_nameplate(plate, names(format!("member-name-mask-{}", member.user.id)), row));
        let plated = plate.is_some();
        let hover_id = member.user.id.clone();
        // lint-allow(custom-button): ui::Item::new is a flat 48px row; a member row also carries a nameplate, its fade and a per-row hover plate.
        ui::row()
            .id(SharedString::from(format!("member-{}", member.user.id)))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                let next = hovered.then(|| hover_id.clone());
                if this.member_hover != next && (*hovered || this.member_hover.as_deref() == Some(hover_id.as_str())) {
                    this.member_hover = next;
                    this.redraw(crate::part::Column::Panel, cx);
                }
            }))
            .relative()
            .h(px(member_row_height()))
            .flex_none()
            .px_2()
            // Discord's room for the art: names stop 32px short of a nameplated row's end.
            .when(plated, |r| r.pr(px(NAMEPLATE_END)))
            .gap_3()
            .squircle(ui::RADIUS_SM)
            // Offline members fade to 30% until hovered (in the hover style below), as Discord's do.
            .when(offline && !selected, |r| r.opacity(0.3))
            .group(MEMBER_ROW_GROUP)
            .cursor_pointer()
            .when(selected, |r| r.bg(palette::selected(cx)))
            .when(!selected, |r| r.hover(move |s| if offline { s.bg(hover).opacity(1.) } else { s.bg(hover) }))
            .children(plate)
            .child(member_face(member, animate, surface))
            .child(names(format!("member-name-{}", member.user.id)).relative().flex_1())
            .on_click({
                let user = member.user.clone();
                cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| {
                    let at = event.position();
                    let at = spot.profile_x.map_or_else(|| beside_panel(at), |x| gpui_kit::point(x, at.y));
                    this.open_profile(user.clone(), at, cx)
                })
            })
            .on_mouse_down(gpui_kit::MouseButton::Right, {
                let (user, guild_id) = (member.user.clone(), self.guild_id.clone());
                let surface = if guild_id.is_some() { crate::user_menu::Surface::ServerChat } else { crate::user_menu::Surface::Group };
                cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_user_menu(user.clone(), guild_id.clone(), surface, e.position, cx))
            })
            .into_any_element()
    }
}

impl Waffle {
    // Discord's line under a member (its ActivityStatus): one primary item, a Go Live first, then the first
    // activity, then being in voice in this server; "+N" when more than one applies; the custom status after
    // it. Hovering it lists every item. Nothing while offline.
    fn member_status(&self, member: &Member) -> Option<MemberStatus> {
        let voice = self.guild().and_then(|g| {
            let state = g.voice_states.iter().find(|v| v.user_id == member.user.id)?;
            let channel = g.channels.iter().find(|c| Some(&c.id) == state.channel_id.as_ref())?;
            Some((state.streaming, channel.clone()))
        });
        activity_status(&member.user, &member.presence, voice)
    }
}

// The same line for anyone: `voice` is the channel they're in and whether they're live in it. The user
// panel draws yours with it.
pub(crate) fn activity_status(user: &crate::discord::User, presence: &crate::discord::Presence, voice: Option<(bool, crate::discord::Channel)>) -> Option<MemberStatus> {
    if presence.status == Status::Offline {
        return None;
    }
    let custom = presence.custom.clone();
    let activities: Vec<(ActivityKind, String)> = presence.activity.iter().flat_map(|a| std::iter::once((a.kind, a.text.clone())).chain(a.rest.iter().cloned())).collect();
    // The hover list, in Discord's order: the stream, each activity, then voice (when not streaming).
    let stream = voice.as_ref().filter(|(streaming, _)| *streaming).map(|(_, channel)| StatusItem { icon: StatusIcon::Screen, text: "Sharing their screen".into(), channel: Some(channel.clone()) });
    let in_voice = voice.as_ref().filter(|(streaming, _)| !*streaming).map(|(_, channel)| StatusItem { icon: StatusIcon::Channel(Box::new(channel.clone())), text: voice_word(channel).into(), channel: Some(channel.clone()) });
    let items: Vec<StatusItem> = stream
        .into_iter()
        .chain(activities.iter().map(|(kind, text)| StatusItem { icon: StatusIcon::Activity(*kind), text: text.clone(), channel: None }))
        .chain(in_voice)
        .filter(|_| !user.bot)
        .collect();
    let (icon, text, sources) = match (&voice, activities.first()) {
        // Live: what they're playing names the stream, and isn't counted again.
        (Some((true, _)), first) => {
            let game = first.filter(|(kind, _)| *kind == ActivityKind::Playing).map(|(_, text)| text.clone());
            let sources = 1 + activities.len() - usize::from(game.is_some());
            (StatusIcon::Screen, game.unwrap_or_else(|| "Sharing their screen".into()), sources)
        }
        (_, Some((kind, text))) => (StatusIcon::Activity(*kind), text.clone(), items.len()),
        (Some((false, channel)), None) => (StatusIcon::Channel(Box::new(channel.clone())), voice_word(channel).to_owned(), 1),
        (None, None) => return custom.map(|custom| MemberStatus { id: user.id.clone(), icon: None, text: None, more: 0, custom: Some(custom), items: Vec::new() }),
    };
    // Bots show no activity icon.
    let icon = (!user.bot).then_some(icon);
    Some(MemberStatus { id: user.id.clone(), icon, text: Some(text), more: sources.saturating_sub(1), custom, items })
}

// Discord's words for being in voice: a DM's is a call, a stage channel's a stage.
fn voice_word(channel: &crate::discord::Channel) -> &'static str {
    voice_word_for(channel.kind, channel.raw_type)
}

fn voice_word_for(kind: crate::discord::ChannelKind, raw_type: u8) -> &'static str {
    match kind {
        crate::discord::ChannelKind::Dm => "In a call",
        _ if raw_type == crate::discord::STAGE_TYPE => "In a stage",
        _ => "In voice",
    }
}

#[derive(Clone)]
pub(crate) struct MemberStatus {
    id: String,
    icon: Option<StatusIcon>,
    text: Option<String>,
    more: usize,
    custom: Option<String>,
    // Everything they're doing, for the hover list.
    items: Vec<StatusItem>,
}

#[derive(Clone)]
struct StatusItem {
    icon: StatusIcon,
    text: String,
    // Being in voice names the channel after the text: "In voice (🔊 lounge)".
    channel: Option<crate::discord::Channel>,
}

#[derive(Clone)]
enum StatusIcon {
    Activity(ActivityKind),
    Screen,
    Channel(Box<crate::discord::Channel>),
}

impl StatusIcon {
    // As Discord's: every item's icon is 12px in the online green. A stream is our share glyph rather than
    // Discord's TV.
    fn icon(&self, green: gpui_kit::Hsla) -> Option<Icon> {
        let icon = match self {
            StatusIcon::Activity(kind) => activity_glyph(*kind)?,
            StatusIcon::Screen => Icon::new(Glyph::ShareScreen),
            StatusIcon::Channel(channel) => channel_glyph(channel),
        };
        Some(icon.size(ui::ICON_XS).flex_none().text_color(green))
    }
}

impl MemberStatus {
    // Beside a custom status the primary item is its icon alone, as Discord's member list draws it.
    pub(crate) fn line(self, cx: &App) -> gpui_kit::Stateful<Div> {
        let beside_custom = self.custom.is_some();
        let primary = self.icon.is_some() || (self.text.is_some() && !beside_custom);
        let green = palette::positive_ink(cx);
        let items = self.items.clone();
        ui::row()
            .id(SharedString::from(format!("member-status-{}", self.id)))
            .gap_1()
            .text_size(ui::TEXT_XS)
            .text_color(palette::faint(cx))
            .children(self.icon.and_then(|icon| icon.icon(green)).map(|icon| {
                let tip = div().id(SharedString::from(format!("member-status-icon-{}", self.id))).flex_none().child(icon);
                tip.when(!items.is_empty(), |d| d.tooltip(move |_, cx| cx.new(|_| ActivityTip(items.clone())).into()))
            }))
            .children(self.text.filter(|_| !beside_custom).map(ui::line))
            // Beside a custom status the count takes the green, as Discord's does.
            .when(self.more > 0, |d| d.child(div().flex_none().when(beside_custom, |c| c.text_color(green)).child(format!("+{}", self.more))))
            .when(primary && beside_custom, |d| d.child(div().flex_none().child("•")))
            .children(self.custom.map(ui::line))
    }
}

// Where a member row is drawn: its width and the surface under it, which a nameplate's mask paints, and
// the x its profile opens at (None: left of the click, clear of the side panel).
#[derive(Clone, Copy)]
pub(crate) struct RowSpot {
    pub width: f32,
    pub surface: gpui_kit::Hsla,
    pub profile_x: Option<gpui_kit::Pixels>,
}

// The hover list over a member's activity line, as Discord's: each thing they're doing on its own line.
struct ActivityTip(Vec<StatusItem>);

impl Render for ActivityTip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::perf::probe("ActivityTip");
        let green = palette::positive_ink_inverted(cx);
        let rows = self.0.iter().map(|item| {
            let channel = item.channel.as_ref().map(|channel| {
                ui::row().gap_0p5().child("(").child(channel_glyph(channel).size_3p5().flex_none()).child(ui::line(channel.name.clone())).child(")")
            });
            ui::row().gap_1p5().children(item.icon.icon(green)).child(div().flex_none().child(item.text.clone())).children(channel)
        });
        // lint-allow(custom-badge): ui::Tooltip is one line of text; this tip lists each thing someone is doing, with its icon.
        ui::col()
            .max_w(px(ACTIVITY_TIP_WIDTH))
            .px_2p5()
            .py_2()
            .gap_1p5()
            .squircle(ui::RADIUS_SM)
            .bg(palette::strong(cx))
            .text_color(palette::surface(palette::Level::Page, cx))
            .text_size(ui::TEXT_XS)
            .font_weight(FontWeight::MEDIUM)
            .children(rows)
    }
}

// A 32px avatar plus padding that follows UI density: 44px at Default.
// A group's label, and the space above every label after the first (`mt_3`).
const GROUP_LABEL: f32 = 28.;
// The side panel's gap past its header controls, and how far its profile card opens beside it.
const PANEL_END_INSET: f32 = 8.;
// The hover list over a member's activity, a nameplate's fade at its end, and the avatar's slot before the names.
const ACTIVITY_TIP_WIDTH: f32 = 280.;
const NAMEPLATE_FADE: f32 = 50.;
const NAMEPLATE_LEAD: f32 = 42.;
const GROUP_SPACE: f32 = 12.;
// How far past the view's edges member rows are built, and how much is built before the list's place is known.
const ROWS_OVERSCAN: f32 = 400.;
const ROWS_FIRST_PAINT: f32 = 1200.;

// An entry of the member list, built only while near the view.
enum Entry<'a> {
    Group(String, bool),
    Member(&'a Member),
}

fn member_row_height() -> f32 {
    32. + ui::density_pad(12.)
}

const MEMBER_ROW_GROUP: &str = "member-row";
const NAMEPLATE_END: f32 = 32.;
// The member list's padding on both sides: a row is the panel's width less this.
const MEMBER_LIST_INSET: f32 = 16.;

// The row a nameplate is drawn behind: its hover group, size, fills at rest and hovered (laid solid, for
// the mask to wash in), and whether it's selected and playing.
pub(crate) struct PlateRow {
    pub group: SharedString,
    pub height: f32,
    pub width: f32,
    pub fills: (gpui_kit::Hsla, Option<gpui_kit::Hsla>),
    pub selected: bool,
    pub animate: bool,
    pub dark: bool,
}

// A nameplate behind a member or DM row, as Discord draws it there: the palette colour fading in from nothing
// (8% from a fifth of the way to halfway, 20% at the end), and over it the art pinned right at 60% (90%
// while selected), masked to 30% up to 2px past the names (name line and activity, whichever is wider)
// and rising to full over the next 50px. gpui has no masks, so the mask is laid over the art instead: the
// row as it looks without art (its fill, `fills`: at rest and on hover, under the tint) at 70% to the
// names' end, fading out over 50px. Every layer rounds itself, so nothing has to paint the corners off.
pub(crate) fn row_nameplate(plate: &crate::discord::Nameplate, names: Div, row: PlateRow) -> AnyElement {
    let PlateRow { group, height, width: row_width, fills, selected, animate, dark } = row;
    let tint = nameplate_tint(plate, dark);
    let r = px(ui::RADIUS_SM);
    let left = gpui_kit::Corners { top_left: r, bottom_left: r, ..Default::default() };
    // gpui gradients take two stops, so the four-stop tint is three pieces side by side.
    let piece = |width: f32, from: f32, to: f32| div().h_full().w(relative(width)).bg(linear_gradient(90., linear_color_stop(tint.opacity(from), 0.), linear_color_stop(tint.opacity(to), 1.)));
    let tint_layer = || {
        ui::row().items_stretch()
            .absolute()
            .top_0()
            .bottom_0()
            .left_0()
            .child(ui::edge(piece(0.2, 0., 0.08), left))
            .child(piece(0.3, 0.08, 0.08))
            .child(ui::edge(piece(0.5, 0.08, 0.2), gpui_kit::Corners { top_right: r, bottom_right: r, ..Default::default() }))
    };
    let (rest, hovered) = fills;
    // Past the names the tint sits near its flat 8%, so the fade's colour is the fill under that.
    let under = |fill: gpui_kit::Hsla| ui::mix(fill, tint, 0.08);
    let fade = move |fill: gpui_kit::Hsla| linear_gradient(90., linear_color_stop(under(fill).opacity(0.7), 0.), linear_color_stop(under(fill).opacity(0.), 1.));
    let ramp = div()
        .absolute()
        .top_0()
        .bottom_0()
        .left(relative(1.))
        .w(px(NAMEPLATE_FADE))
        .bg(fade(rest))
        .when_some(hovered, |d, h| d.group_hover(group.clone(), move |s| s.bg(fade(h))));
    // The mask ends where the names do: an invisible copy of them, laid out as the row lays out the real
    // ones (and stopping as short of the end), sizes the held part, which shows the row's own fill and
    // tint at the row's width, clipped to that part.
    let backdrop = ui::edge(tint_layer().w(px(row_width)).bg(rest), left).when_some(hovered, |d, h| d.group_hover(group, move |s| s.bg(h)));
    let held = ui::row()
        .relative()
        .overflow_hidden()
        .opacity(0.7)
        .pl_2()
        .child(backdrop)
        // The avatar and the gap after it.
        .child(div().w(px(NAMEPLATE_LEAD)).flex_none())
        .child(names.opacity(0.))
        .child(div().w_0p5().flex_none());
    div()
        .absolute()
        .inset_0()
        .child(tint_layer().right_0())
        .child(nameplate_art(plate, height, r, r, animate).opacity(if selected { 0.9 } else { 0.6 }))
        // Clipped to the row: on a long name the fade runs past its end.
        .child(ui::row().items_stretch().absolute().inset_0().overflow_hidden().pr(px(NAMEPLATE_END - 2.)).child(ui::row().items_stretch().relative().child(held).child(ramp)))
        .into_any_element()
}

// A group heading (.cat): "Online · 4".
pub(crate) fn group_label(text: impl Into<SharedString>, cx: &App) -> Div {
    ui::row().h_7().flex_none().px_1p5().whitespace_nowrap().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(palette::faint(cx)).child(text.into())
}

// A wait names what it waits on, beside the mini wave (.pwait).
pub(crate) fn wait_line(id: &'static str, text: &'static str, cx: &App) -> Div {
    ui::row().min_h_8().px_2().gap_2().text_color(palette::muted(cx)).font_weight(FontWeight::NORMAL).child(ui::Spinner::new(id, palette::muted(cx), 12.)).child(text)
}

// The list before Discord sends it: the static label, no false count, rows shaped like the real ones.
fn members_loading(cx: &App) -> Div {
    let fill = palette::selected(cx);
    let bar = move |w: f32, h: f32| div().w(gpui_kit::relative(w)).h(px(h)).squircle(ui::RADIUS_XS).bg(fill);
    // GAP: the design sweeps one shimmer band across all skeletons; these hold still.
    let row = move |name: f32, line: Option<f32>| {
        ui::row().h_11().px_2().gap_2p5().child(div().size_8().flex_none().rounded_full().bg(fill)).child(ui::col().min_h_0().flex_1().gap_1p5().child(bar(name, 12.)).children(line.map(|w| bar(w, 8.))))
    };
    ui::col()
        .gap_0p5()
        .child(wait_line("members-wait", "Loading members…", cx))
        .child(group_label("Online", cx))
        .child(row(0.7, Some(0.45)))
        .child(row(0.52, None))
        .child(row(0.8, Some(0.6)))
        .child(row(0.4, None))
}

const BOLT: &[u8] = include_bytes!("../assets/icons/reicon/bolt.svg");

// How long ago, in the feed's steps: minutes, hours, days, then weeks.
pub(crate) fn ago(ms: i64) -> String {
    let minutes = (ms / 60_000).max(0);
    match minutes {
        0 => "Just now".to_owned(),
        1..60 => format!("{minutes}m ago"),
        60..1440 => format!("{}h ago", minutes / 60),
        1440..10080 => format!("{}d ago", minutes / 1440),
        _ => format!("{}w ago", minutes / 10080),
    }
}

// Discord's gem tooltip date, from `premium_since`.
// Where a profile opens for a click in the panel: its card's width to the left, so it sits beside the panel.
fn beside_panel(click: gpui_kit::Point<gpui_kit::Pixels>) -> gpui_kit::Point<gpui_kit::Pixels> {
    gpui_kit::point(click.x - px(crate::profile::WIDTH), click.y)
}

fn boost_date(iso: &str) -> Option<String> {
    let day = chrono::NaiveDate::parse_from_str(iso.get(..10)?, "%Y-%m-%d").ok()?;
    Some(crate::search::day_label(day))
}

fn member_face(member: &Member, animate: bool, surface: gpui_kit::Hsla) -> AnyElement {
    // Offline shows no mark at all, as Discord's member list does.
    let mark = (member.presence.status != Status::Offline).then(|| member.presence.mark());
    let (avatar, decoration) = (&member.user.avatar, member.user.decoration.as_deref());
    let face = match animate {
        true => playing_avatar(&format!("member-{}", member.user.id), avatar, decoration, 32., mark, surface),
        false => decorated_avatar(avatar, decoration, 32., mark, surface),
    };
    face.morph(format!("member-presence-{}", member.user.id)).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{ago, voice_word_for};
    use crate::discord::{ChannelKind, STAGE_TYPE};

    #[test]
    fn voice_word_names_calls_and_stages() {
        assert_eq!(voice_word_for(ChannelKind::Dm, 1), "In a call");
        assert_eq!(voice_word_for(ChannelKind::Voice, STAGE_TYPE), "In a stage");
        assert_eq!(voice_word_for(ChannelKind::Voice, 2), "In voice");
    }

    #[test]
    fn ago_steps_up_by_unit() {
        assert_eq!(ago(-5), "Just now");
        assert_eq!(ago(59_000), "Just now");
        assert_eq!(ago(5 * 60_000), "5m ago");
        assert_eq!(ago(3 * 3_600_000), "3h ago");
        assert_eq!(ago(26 * 3_600_000), "1d ago");
        assert_eq!(ago(15 * 86_400_000), "2w ago");
    }
}
