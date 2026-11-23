// The Direct Messages list's rows.
use gpui_kit::{AnyElement, Context, Div, FontWeight, SharedString, component::{ActiveTheme, Icon}, div, prelude::*, px};

use super::{RowState, activity_glyph, channel_row, draft_mark, row_action, row_gap};
use crate::{
    Waffle,
    discord::{Channel, Status as Status_},
    icons::Glyph,
    layout::{decorated_avatar, playing_avatar},
    palette,
    panel::{PlateRow, row_nameplate},
    sidebar::Nav,
    ui,
};

// Two lines beside a 32px avatar.
const DM_ROW_HEIGHT: f32 = 44.;
// How far past each edge of the view rows are built, so a fast scroll doesn't outrun them before the next frame.
const OVERSCAN: f32 = 400.;
// How much of the list is built before it has been laid out once and its position is known.
const FIRST_PAINT: f32 = 1200.;

impl Waffle {
    // The DM list's rows from child `first` of the channel list on: only those in view (and `OVERSCAN` either
    // side) are built, with spacers holding the rest's height. The list can run to hundreds, and every scroll
    // step redraws the sidebar, so building every row made scrolling choppy. Every row is the same height,
    // so the range follows from the scroll offset and where the block sat in the last layout.
    pub(super) fn dm_rows(&self, first: usize, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let dms = self.dm_order();
        let heights = vec![DM_ROW_HEIGHT; dms.len()];
        let scroll = &self.channels_scroll;
        let (range, before, after) = match scroll.bounds_for_item(first) {
            // Child bounds are laid out unscrolled; the offset runs negative as the list scrolls down.
            Some(block) => ui::visible_rows(&heights, row_gap(), f32::from(block.top() - scroll.bounds().top()), -f32::from(scroll.offset().y), f32::from(scroll.bounds().size.height), OVERSCAN),
            None => ui::visible_rows(&heights, row_gap(), 0., 0., FIRST_PAINT, 0.),
        };
        let spacer = |height: f32| (height > 0.).then(|| div().flex_none().h(px(height)).into_any_element());
        spacer(before).into_iter().chain(dms[range].iter().map(|c| self.dm_row(c, cx))).chain(spacer(after)).collect()
    }

    // A conversation: the person, then one line on what they're doing.
    pub(super) fn dm_row(&self, channel: &Channel, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.channel.as_ref().is_some_and(|c| c.id == channel.id);
        let unread = self.unread(&channel.id);
        let mentions = self.mention_count(&channel.id);
        let presence = channel.recipient.as_ref().and_then(|u| self.presence_of(&u.id));
        let status = presence.as_ref().map(|p| p.status);
        let offline = status == Some(Status_::Offline);
        let ring = crate::material::sidebar_fill(cx);
        let hovered = self.hovered_row.as_deref() == Some(channel.id.as_str());
        // Discord plays the avatar, decoration and nameplate while the row is hovered or open.
        let animate = (selected || hovered) && crate::presence::may_animate();
        let mark = presence.as_ref().map(|p| p.mark());
        let face = match &channel.recipient {
            Some(user) if animate => playing_avatar(&format!("dm-{}", channel.id), &user.avatar, user.decoration.as_deref(), 32., mark, ring),
            Some(user) => decorated_avatar(&user.avatar, user.decoration.as_deref(), 32., mark, ring),
            None => ui::dm_face(&channel.face(), 32., ring),
        };
        let typing = self.typing.get(&channel.id).is_some_and(|t| t.iter().any(|t| t.until > std::time::Instant::now()));
        let faint = palette::faint(cx);
        let call_line = self.dm_call_line(&channel.id, cx);
        // Built twice on a nameplated row: the second, invisible, sizes the nameplate's mask (`key` keeps ids apart).
        let line = |key: &str| -> Option<Div> {
            let line = match (typing, presence.clone()) {
                (true, _) => Some(div().child(ui::bars(SharedString::from(format!("dm-typing-{}{key}", channel.id)), faint, 10.)).child("Typing…")),
                (false, Some(p)) => match (p.activity, p.status) {
                    (Some(a), _) => Some(div().children(activity_glyph(a.kind).map(|i| i.size(ui::ICON_XS).flex_none())).child(ui::line(a.text))),
                    (None, Status_::Offline) => None,
                    (None, s) => Some(div().child(s.label())),
                },
                // A group always counts us and at least one other, so never "1 member".
                (false, None) => (!channel.recipients.is_empty()).then(|| div().child(format!("{} members", channel.recipients.len() + 1))),
            };
            call_line.map(|(glyph, label, ink)| div().child(Icon::new(glyph).size(ui::ICON_SM).flex_none().text_color(ink)).child(div().text_color(ink).child(label))).or(line)
        };
        let names = |key: &str| {
            ui::col().min_h_0()
                .flex_1()
                .child(ui::line(channel.name.clone()).font_weight(if unread { FontWeight::MEDIUM } else { FontWeight::NORMAL }))
                .children(line(key).map(|l| l.flex().items_center().gap_1().min_w_0().text_size(ui::TEXT_XS).text_color(faint)))
        };
        // Only while hovered, so the list stays quiet at rest.
        let plate = channel.recipient.as_ref().and_then(|u| u.nameplate.as_ref()).filter(|_| hovered).map(|plate| {
            // The row's fills laid solid on the sidebar's own surface, as the user panel's nameplate is.
            let surface = palette::surface(palette::Level::Sidebar, cx);
            let fill = |wash: gpui_kit::Hsla| ui::mix(surface, wash.opacity(1.), wash.a);
            let fills = if selected { (fill(palette::selected(cx)), None) } else { (surface, Some(fill(palette::hover(cx)))) };
            let row = PlateRow { group: format!("dm-{}", channel.id).into(), height: DM_ROW_HEIGHT, width: self.channels_width - 16., fills, selected, animate, dark: cx.theme().is_dark() };
            row_nameplate(plate, names("-mask").flex_initial(), row)
        });
        let state = RowState { selected, unread, muted: false, strong: false };
        let (open, close_id) = (channel.clone(), channel.id.clone());
        let group: SharedString = format!("dm-{}", channel.id).into();
        let pinned = self.guild_settings.get("@me").is_some_and(|s| s.pinned(&channel.id));
        // Hovered, the close button takes the count's and the pin's place.
        let close = row_action(SharedString::from(format!("dm-close-{}", channel.id)), Nav::Close.icon(), "Close", cx)
            .flex_none()
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.close_dm(&close_id, cx);
            }));
        let row = channel_row(SharedString::from(channel.id.clone()), group.clone(), state, cx);
        self.track_row_hover(row, &channel.id, true, cx)
            .h(px(DM_ROW_HEIGHT))
            .gap_2p5()
            .font_weight(FontWeight::NORMAL)
            .children(plate)
            .child(div().relative().when(offline, |a| a.opacity(0.5)).child(face))
            .child(names("").relative())
            .when(mentions > 0 && !hovered, |r| r.child(ui::count_badge(mentions)))
            .when(mentions == 0 && !selected && !hovered && self.drafts.has(&channel.id), |r| r.child(ui::row().items_stretch().flex_none().text_color(faint).child(draft_mark())))
            .when(pinned && !hovered, |r| r.child(ui::row().items_stretch().flex_none().text_color(faint).child(Icon::new(Glyph::Pin).size(ui::ICON_SM))))
            .when(hovered, |r| r.child(close))
            .on_click(cx.listener(move |this, _, window, cx| this.open_channel(open.clone(), window, cx)))
            .on_mouse_down(gpui_kit::MouseButton::Right, {
                let channel = channel.clone();
                cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_dm_menu(channel.clone(), e.position, cx))
            })
            .into_any_element()
    }
}
