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
    layout::{decorated_avatar, nameplate_art, nameplate_tint},
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
            _ => self.guild_id.is_some(),
        }
    }

    // The side panel's width while it shows. A DM has no member list; its side panel is only ever search results.
    pub fn panel_column_width(&self) -> Option<f32> {
        match &self.right_panel {
            _ if !self.panel_shown() => None,
            RightPanel::Hidden => None,
            RightPanel::Members | RightPanel::Activity => Some(self.panel_width),
            RightPanel::Search(_) => Some(crate::search_results::PANEL_WIDTH),
            RightPanel::ModView(_) => Some(self.panel_width.max(crate::mod_view::PANEL_WIDTH)),
        }
    }

    pub fn render_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.close_mod_view_if_barred();
        let width = self.panel_column_width()?;
        // Its header sits on the window's top line beside the channel header's: "Members", or the search's
        // query with a way back to the members.
        let (head, body, foot): (Div, AnyElement, Option<AnyElement>) = match &self.right_panel {
            RightPanel::Hidden => return None,
            RightPanel::Members => (self.panel_head("Members".into(), false, cx), self.member_list(window, cx), None),
            RightPanel::Activity => (self.panel_head("Activity".into(), true, cx), self.activity_cards(usize::MAX, cx), None),
            RightPanel::Search(results) => {
                let head = self.panel_frame(cx).child(self.search_head(results, self.back_button(cx), cx));
                (head, self.search_body(results, cx), self.search_pager(results, cx))
            }
            RightPanel::ModView(view) => (self.panel_frame(cx).child(self.mod_view_head(view, cx)), self.mod_view_body(view, cx), None),
        };
        let line = palette::line(cx);
        let scroll = self.panel_scroll.clone();
        Some(
            div()
                .relative()
                .w(px(width))
                .h_full()
                .flex_none()
                .flex()
                .flex_col()
                // The sidebar's tone: the step from the feed's tone is the only edge it needs.
                .bg(palette::surface(palette::Level::Sidebar, cx))
                .text_color(palette::text(cx))
                .text_size(px(14.))
                // The list stops under the header, a hard edge with the sidebar's hairline once it has scrolled.
                .child(ui::hard_edge(move || ui::scrolled(&scroll), cx).child(head))
                .child(ui::scroll_area(div().id("right-panel").pt(px(8.)).px_2().pb_2().flex().flex_col().gap(px(2.)).child(body), &self.panel_scroll))
                .child(self.resize_handle(Edge::Panel, cx))
                .children(foot.map(|pager| div().flex_none().py(px(8.)).border_t_1().border_color(line).child(pager)))
                .into_any_element(),
        )
    }

    // The column's header row, clear of Windows' caption buttons at the window's top right; `back` adds the
    // way back to the members.
    fn panel_head(&self, title: SharedString, back: bool, cx: &mut Context<Self>) -> Div {
        let back = back.then(|| self.back_button(cx));
        self.panel_frame(cx).child(div().flex_1().min_w_0().truncate().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).child(title)).children(back)
    }

    pub(crate) fn panel_frame(&self, cx: &App) -> Div {
        div()
            .h(px(crate::layout::HEADER_HEIGHT))
            .pl_4()
            .pr(px(8. + crate::window_controls::WIDTH))
            .flex()
            .items_center()
            .gap_2()
            .window_control_area(gpui_kit::WindowControlArea::Drag)
            .text_color(palette::strong(cx))
    }

    fn back_button(&self, cx: &mut Context<Self>) -> ui::Control {
        ui::icon_button("close-panel", Glyph::X, "Back to members", cx).occlude().on_click(cx.listener(|this, _, _, cx| {
            this.right_panel = RightPanel::Members;
            cx.notify();
        }))
    }

    // Who is here, grouped by top role, then online and offline (navigation.css .members).
    fn member_list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let activity = self.activity_section(window, cx);
        let Some(guild) = self.guild() else { return div().into_any_element() };
        let Some(list) = self.member_lists.get(&guild.id).filter(|l| !l.is_empty()) else { return members_loading(cx).into_any_element() };
        let mut first = activity.is_none();
        let rows = list.iter().filter_map(|item| match item {
            ListItem::Group { id, count } => {
                let count = self.member_groups.get(&guild.id).and_then(|g| g.get(id)).copied().unwrap_or(*count);
                let name = match id.as_str() {
                    "online" => "Online".to_string(),
                    "offline" => "Offline".to_string(),
                    "" => return None,
                    role => guild.roles.iter().find(|r| r.id == role).map(|r| r.name.clone()).unwrap_or_else(|| "Members".into()),
                };
                let spaced = !std::mem::take(&mut first);
                Some(group_label(format!("{name} · {count}"), cx).when(spaced, |l| l.mt_3()).into_any_element())
            }
            ListItem::Member(member) => Some(self.member_row(member, cx)),
        });
        div().flex().flex_col().gap(px(2.)).children(activity).children(rows.collect::<Vec<_>>()).into_any_element()
    }

    pub(crate) fn load_activity_feed(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            // No feed (an error, or none for this account) leaves the section out.
            let Ok(feed) = client.activity_feed().await else { return };
            this.update(cx, |this, cx| {
                this.activity_feed = feed;
                cx.notify();
            })
            .ok();
        })
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
            .child(Nav::Caret.icon().size(px(11.)).rotate(Radians(chevron.target().x_px().to_radians())))
            .child(format!("Activity · {count}"))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.folded.remove(&key) {
                    this.folded.insert(key.clone());
                }
                cx.notify();
            }));
        let all = category_action("activity-all", Icon::new(Glyph::ChevronRight), None, "Show all activity", cx).on_click(cx.listener(|this, _, _, cx| {
            this.right_panel = RightPanel::Activity;
            cx.notify();
        }));
        let alpha = fold.target().alpha();
        let cards = (!folded || alpha > 0.05).then(|| div().opacity(alpha).child(self.activity_cards(ACTIVITY_SHOWN, cx)));
        Some(div().flex().flex_col().gap(px(2.)).child(category_head("activity-head".into(), label, false, cx).child(all)).children(cards))
    }

    fn activity_cards(&self, limit: usize, cx: &mut Context<Self>) -> AnyElement {
        let now = chrono::Utc::now().timestamp_millis();
        let cards: Vec<AnyElement> = self.guild_activity().into_iter().take(limit).enumerate().map(|(i, (entry, member))| self.feed_card(i, entry, member, now, cx)).collect();
        div().flex().flex_col().gap(px(8.)).children(cards).into_any_element()
    }

    // One card: who (avatar, name in their role's colour), what, and when, with its art on the right.
    fn feed_card(&self, i: usize, entry: &FeedEntry, member: &Member, now: i64, cx: &mut Context<Self>) -> AnyElement {
        let dark = cx.theme().is_dark();
        let (strong, muted, hover) = (palette::strong(cx), palette::muted(cx), palette::hover(cx));
        let name_ink = self.role_color(self.guild_id.as_ref(), &member.user.id, Some(&member.roles), dark).unwrap_or(strong);
        let when = if entry.live { "Now".to_owned() } else { ago(now - entry.at) };
        let meta = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .min_w_0()
            .text_size(px(12.))
            .font_weight(FontWeight::NORMAL)
            .text_color(muted)
            .children(activity_glyph(entry.kind).map(|icon| icon.size(px(14.)).flex_none().when(entry.live, |i| i.text_color(palette::status_in(Status::Online, cx)))))
            .child(div().flex_none().child(when))
            .when_some(entry.streak_days, |m, days| m.child(div().w(px(6.)).flex_none()).child(Icon::default().data(BOLT).size(px(14.)).flex_none()).child(div().truncate().child(format!("{days}× streak"))));
        let image = entry.image.clone().or_else(|| self.app_icon(entry.app_id.as_deref()?, cx));
        let art = match image {
            Some(url) => img(url).size(px(44.)).flex_none().object_fit(gpui_kit::ObjectFit::Cover).squircle(ui::RADIUS_MD).into_any_element(),
            None => div()
                .size(px(44.))
                .flex_none()
                .squircle(ui::RADIUS_MD)
                .bg(palette::selected(cx))
                .flex()
                .items_center()
                .justify_center()
                .text_color(muted)
                .children(activity_glyph(entry.kind).or_else(|| activity_glyph(ActivityKind::Playing)).map(|icon| icon.size(px(20.))))
                .into_any_element(),
        };
        div()
            .id(SharedString::from(format!("activity-{i}")))
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(10.))
            .py(px(8.))
            .squircle(ui::RADIUS_LG)
            .bg(palette::surface(palette::Level::Page, cx))
            .border_1()
            .border_color(palette::line(cx))
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(div().flex().items_center().gap(px(6.)).min_w_0().text_size(px(12.)).child(ui::avatar(member.user.avatar.clone(), 14.).flex_none()).child(div().truncate().text_color(name_ink).child(member.user.name.clone())))
                    .child(div().truncate().font_weight(FontWeight::MEDIUM).text_color(strong).child(entry.title.clone()))
                    .child(meta),
            )
            .child(art)
            .on_click({
                let user = member.user.clone();
                cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| this.open_profile(user.clone(), event.position() - gpui_kit::point(px(320.), px(0.)), cx))
            })
            .into_any_element()
    }

    // A member (.mem): avatar with status, the name in their top role's colour, one line of activity.
    // Offline dims the avatar and drops the name to faint; the open profile's member keeps the fill.
    fn member_row(&self, member: &Member, cx: &mut Context<Self>) -> AnyElement {
        let dark = cx.theme().is_dark();
        let offline = member.presence.status == Status::Offline;
        let (strong, faint, hover) = (palette::strong(cx), palette::faint(cx), palette::hover(cx));
        let name_ink = match offline {
            true => faint,
            false => self.role_color(self.guild_id.as_ref(), &member.user.id, Some(&member.roles), dark).unwrap_or(strong),
        };
        let surface = palette::surface(palette::Level::Sidebar, cx);
        let owner = self.guild().is_some_and(|g| g.owner_id.as_ref() == Some(&member.user.id));
        let selected = self.profile_card.as_ref().is_some_and(|p| p.user.id == member.user.id);
        let crown = ui::mix(palette::status_in(Status::Idle, cx), strong, 0.4);
        let name_line = |key: String| {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .min_w_0()
                .child(crate::name_fonts::styled(ui::truncating(key, member.user.name.clone()).font_weight(FontWeight::NORMAL).text_color(name_ink), member.user.name_style.as_ref()))
                .children(member.user.tag.as_ref().map(|tag| crate::chat::guild_tag(tag, cx)))
                .when(owner, |r| r.child(Icon::new(Glyph::Crown).size(px(14.)).flex_none().text_color(crown)))
                .when(member.user.bot, |r| r.child(ui::tag("App", cx)))
        };
        let status = self.member_status(member);
        let names = |key: String| div().min_w_0().flex().flex_col().child(name_line(key)).children(status.clone().map(|s| s.line(cx)));
        // The row's fill in each state, laid solid over the panel, for the nameplate's mask to wash in.
        let fill = |wash: gpui_kit::Hsla| ui::mix(surface, wash.opacity(1.), wash.a);
        let fills = match selected {
            true => (fill(palette::selected(cx)), None),
            false => (surface, Some(fill(hover))),
        };
        let plate = member.user.nameplate.as_ref().map(|plate| member_nameplate(plate, names(format!("member-name-mask-{}", member.user.id)), fills, self.panel_width - MEMBER_LIST_INSET, selected, dark));
        let plated = plate.is_some();
        div()
            .id(SharedString::from(format!("member-{}", member.user.id)))
            .relative()
            .h(px(MEMBER_ROW))
            .flex_none()
            .px_2()
            // Discord's room for the art: names stop 32px short of a nameplated row's end.
            .when(plated, |r| r.pr(px(NAMEPLATE_END)))
            .flex()
            .items_center()
            .gap(px(10.))
            .squircle(ui::RADIUS_SM)
            .group(MEMBER_ROW_GROUP)
            .cursor_pointer()
            .when(selected, |r| r.bg(palette::selected(cx)))
            .when(!selected, |r| r.hover(move |s| s.bg(hover)))
            .children(plate)
            .child(div().when(offline, |a| a.opacity(0.5)).child(decorated_avatar(&member.user.avatar, member.user.decoration.as_deref(), 32., Some(member.presence.mark()), surface)))
            .child(names(format!("member-name-{}", member.user.id)).relative().flex_1())
            .on_click({
                let user = member.user.clone();
                cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| this.open_profile(user.clone(), event.position() - gpui_kit::point(px(320.), px(0.)), cx))
            })
            .on_mouse_down(gpui_kit::MouseButton::Right, {
                let (user, guild_id) = (member.user.clone(), self.guild_id.clone());
                cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_user_menu(user.clone(), guild_id.clone(), crate::user_menu::Surface::ServerChat, e.position, cx))
            })
            .into_any_element()
    }
}

impl Waffle {
    // Discord's line under a member (its ActivityStatus): one primary item, a Go Live first, then the first
    // activity, then being in voice in this server; "+N" when more than one applies; the custom status after
    // it. Nothing while offline.
    fn member_status(&self, member: &Member) -> Option<MemberStatus> {
        let presence = &member.presence;
        if presence.status == Status::Offline {
            return None;
        }
        let voice = self.guild().and_then(|g| {
            let state = g.voice_states.iter().find(|v| v.user_id == member.user.id)?;
            let channel = g.channels.iter().find(|c| Some(&c.id) == state.channel_id.as_ref())?;
            Some((state.streaming, channel.clone()))
        });
        let activities = presence.activity.as_ref().map_or(0, |a| a.more + 1);
        let custom = presence.custom.clone();
        let (icon, text, live, sources) = match (&voice, &presence.activity) {
            // Live: what they're playing names the stream, and isn't counted again.
            (Some((true, _)), activity) => {
                let game = activity.as_ref().filter(|a| a.kind == ActivityKind::Playing).map(|a| a.text.clone());
                let sources = 1 + activities - usize::from(game.is_some());
                (activity_glyph(ActivityKind::Streaming), game.unwrap_or_else(|| "Sharing their screen".into()), true, sources)
            }
            (_, Some(activity)) => (activity_glyph(activity.kind), activity.text.clone(), activity.kind == ActivityKind::Streaming, activities + usize::from(voice.is_some())),
            (Some((false, channel)), None) => (Some(channel_glyph(channel)), "In voice".to_owned(), false, 1),
            (None, None) => return custom.map(|custom| MemberStatus { icon: None, text: None, live: false, more: 0, custom: Some(custom) }),
        };
        // Bots show no activity icon.
        let icon = icon.filter(|_| !member.user.bot);
        Some(MemberStatus { icon, text: Some(text), live, more: sources.saturating_sub(1), custom })
    }
}

#[derive(Clone)]
struct MemberStatus {
    icon: Option<Icon>,
    text: Option<String>,
    live: bool,
    more: usize,
    custom: Option<String>,
}

impl MemberStatus {
    // Beside a custom status the primary item is its icon alone, as Discord's member list draws it.
    fn line(self, cx: &App) -> Div {
        let beside_custom = self.custom.is_some();
        let primary = self.icon.is_some() || (self.text.is_some() && !beside_custom);
        div()
            .flex()
            .items_center()
            .gap_1()
            .min_w_0()
            .text_size(px(12.))
            .text_color(palette::faint(cx))
            .children(self.icon.map(|icon| icon.size(px(16.)).flex_none().when(self.live, |i| i.text_color(palette::accent_ink(cx)))))
            .children(self.text.filter(|_| !beside_custom).map(|text| ui::truncating(format!("member-status-{text}"), text)))
            .when(self.more > 0, |d| d.child(div().flex_none().child(format!("+{}", self.more))))
            .when(primary && beside_custom, |d| d.child(div().flex_none().child("•")))
            .children(self.custom.map(|custom| ui::truncating(format!("member-custom-{custom}"), custom)))
    }
}

const MEMBER_ROW: f32 = 44.;
const MEMBER_ROW_GROUP: &str = "member-row";
const NAMEPLATE_END: f32 = 32.;
// The member list's padding on both sides: a row is the panel's width less this.
const MEMBER_LIST_INSET: f32 = 16.;

// A nameplate behind a member row, as Discord draws it there: the palette colour fading in from nothing
// (8% from a fifth of the way to halfway, 20% at the end), and over it the art pinned right at 60% (90%
// while selected), masked to 30% up to 2px past the names (name line and activity, whichever is wider)
// and rising to full over the next 50px. gpui has no masks, so the mask is laid over the art instead: the
// row as it looks without art (its fill, `fills`: at rest and on hover, under the tint) at 70% to the
// names' end, fading out over 50px. Every layer rounds itself, so nothing has to paint the corners off.
fn member_nameplate(plate: &crate::discord::Nameplate, names: Div, fills: (gpui_kit::Hsla, Option<gpui_kit::Hsla>), row_width: f32, selected: bool, dark: bool) -> AnyElement {
    let tint = nameplate_tint(plate, dark);
    let r = px(ui::RADIUS_SM);
    let left = gpui_kit::Corners { top_left: r, bottom_left: r, ..Default::default() };
    // gpui gradients take two stops, so the four-stop tint is three pieces side by side.
    let piece = |width: f32, from: f32, to: f32| div().h_full().w(relative(width)).bg(linear_gradient(90., linear_color_stop(tint.opacity(from), 0.), linear_color_stop(tint.opacity(to), 1.)));
    let tint_layer = || {
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .left_0()
            .flex()
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
        .w(px(50.))
        .bg(fade(rest))
        .when_some(hovered, |d, h| d.group_hover(MEMBER_ROW_GROUP, move |s| s.bg(fade(h))));
    // The mask ends where the names do: an invisible copy of them, laid out as the row lays out the real
    // ones (and stopping as short of the end), sizes the held part, which shows the row's own fill and
    // tint at the row's width, clipped to that part.
    let backdrop = ui::edge(tint_layer().w(px(row_width)).bg(rest), left).when_some(hovered, |d, h| d.group_hover(MEMBER_ROW_GROUP, move |s| s.bg(h)));
    let held = div()
        .relative()
        .flex()
        .items_center()
        .min_w_0()
        .overflow_hidden()
        .opacity(0.7)
        .pl_2()
        .child(backdrop)
        // The avatar and the gap after it.
        .child(div().w(px(42.)).flex_none())
        .child(names.opacity(0.))
        .child(div().w(px(2.)).flex_none());
    div()
        .absolute()
        .inset_0()
        .child(tint_layer().right_0())
        .child(nameplate_art(plate, MEMBER_ROW, r, r).opacity(if selected { 0.9 } else { 0.6 }))
        // Clipped to the row: on a long name the fade runs past its end.
        .child(div().absolute().inset_0().overflow_hidden().flex().pr(px(NAMEPLATE_END - 2.)).child(div().relative().flex().min_w_0().child(held).child(ramp)))
        .into_any_element()
}

// A group heading (.cat): "Online · 4".
pub(crate) fn group_label(text: impl Into<SharedString>, cx: &App) -> Div {
    div().h(px(28.)).flex_none().px(px(6.)).flex().items_center().whitespace_nowrap().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(palette::faint(cx)).child(text.into())
}

// A wait names what it waits on, beside the mini wave (.pwait).
pub(crate) fn wait_line(id: &'static str, text: &'static str, cx: &App) -> Div {
    div().min_h(px(32.)).px_2().flex().items_center().gap_2().text_color(palette::muted(cx)).font_weight(FontWeight::NORMAL).child(ui::bars(id, palette::muted(cx), 12.)).child(text)
}

// The list before Discord sends it: the static label, no false count, rows shaped like the real ones.
fn members_loading(cx: &App) -> Div {
    let fill = palette::selected(cx);
    let bar = move |w: f32, h: f32| div().w(gpui_kit::relative(w)).h(px(h)).squircle(ui::RADIUS_XS).bg(fill);
    // GAP: the design sweeps one shimmer band across all skeletons; these hold still.
    let row = move |name: f32, line: Option<f32>| {
        div().h(px(44.)).px_2().flex().items_center().gap(px(10.)).child(div().size(px(32.)).flex_none().rounded_full().bg(fill)).child(div().flex_1().flex().flex_col().gap(px(6.)).child(bar(name, 12.)).children(line.map(|w| bar(w, 8.))))
    };
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
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

#[cfg(test)]
mod tests {
    use super::ago;

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
