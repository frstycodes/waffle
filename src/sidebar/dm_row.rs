// The Direct Messages list's rows.
use gpui_kit::{AnyElement, Context, Div, FontWeight, SharedString, component::{ActiveTheme, Icon}, div, prelude::*, px};

use super::{RowState, channel_row, draft_mark, row_action, row_gap};
use crate::{
    Waffle,
    discord::{Channel, Mark},
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
        let presence = channel.recipient.as_ref().and_then(|u| self.presence_of(&u.id));
        let ring = crate::material::sidebar_fill(cx);
        let hovered = self.hovered_row.as_deref() == Some(channel.id.as_str());
        // Discord plays the avatar, decoration and nameplate while the row is hovered or open.
        let animate = (selected || hovered) && crate::presence::may_animate();
        let typing = self.typing.get(&channel.id).is_some_and(|t| t.iter().any(|t| t.until > std::time::Instant::now()));
        // In a 1:1 DM Discord shows typing on the avatar, as its presence mark turned into the typing pill.
        let dm_typing = typing && channel.recipient.is_some();
        let mark = if dm_typing { Some(Mark::Typing) } else { presence.as_ref().map(|p| p.mark()) };
        let face = match &channel.recipient {
            Some(user) if animate => playing_avatar(&format!("dm-{}", channel.id), &user.avatar, user.decoration.as_deref(), 32., mark, ring).morph(format!("dm-presence-{}", channel.id)).into_any_element(),
            Some(user) => decorated_avatar(&user.avatar, user.decoration.as_deref(), 32., mark, ring).morph(format!("dm-presence-{}", channel.id)).into_any_element(),
            None => ui::dm_face(&channel.face(), 32., ring),
        };
        let faint = palette::faint(cx);
        let call_line = self.dm_call_line(&channel.id, cx);
        // Built twice on a nameplated row: the second, invisible, sizes the nameplate's mask (`key` keeps ids apart).
        // Discord's second line: only what they're doing (the member list's activity line), never the status
        // word; a group counts its members.
        let line = |key: &str, cx: &gpui_kit::App| -> Option<Div> {
            let line = match (typing && !dm_typing, presence.clone(), channel.recipient.as_ref()) {
                (true, _, _) => Some(div().child(ui::Spinner::new(SharedString::from(format!("dm-typing-{}{key}", channel.id)), faint, 10.)).child("Typing…")),
                (false, Some(p), Some(user)) => crate::panel::activity_status(user, &p, None).map(|status| div().child(status.line(cx))),
                (false, Some(_), None) => None,
                // A group always counts us and at least one other, so never "1 Member".
                (false, None, _) => (!channel.recipients.is_empty()).then(|| div().child(format!("{} Members", channel.recipients.len() + 1))),
            };
            call_line.map(|(glyph, label, ink)| div().child(Icon::new(glyph).size(ui::ICON_SM).flex_none().text_color(ink)).child(div().text_color(ink).child(label))).or(line)
        };
        let names = |key: &str, cx: &gpui_kit::App| {
            ui::col().min_h_0()
                .flex_1()
                .child(
                    ui::row()
                        .gap_1()
                        .child(ui::line(channel.name.clone()).text_size(ui::TEXT_MD).line_height(ui::LINE_SM).font_weight(FontWeight::MEDIUM))
                        .children(channel.recipient.as_ref().and_then(|u| u.tag.as_ref()).map(|tag| crate::chat::guild_tag(tag).flex_none())),
                )
                .children(line(key, cx).map(|l| l.flex().items_center().gap_1().min_w_0().text_size(ui::TEXT_XS).text_color(faint)))
        };
        // The open conversation keeps its nameplate, still; it moves only under the pointer.
        let plate = channel.recipient.as_ref().and_then(|u| u.nameplate.as_ref()).filter(|_| selected || hovered).map(|plate| {
            // The row's fills laid solid on the sidebar's own surface, as the user panel's nameplate is.
            let surface = palette::surface(palette::Level::Sidebar, cx);
            let fill = |wash: gpui_kit::Hsla| ui::mix(surface, wash.opacity(1.), wash.a);
            let fills = if selected { (fill(palette::selected(cx)), None) } else { (surface, Some(fill(palette::hover(cx)))) };
            let row = PlateRow { group: format!("dm-{}", channel.id).into(), height: DM_ROW_HEIGHT, width: self.channels_width - 16., fills, selected, animate: hovered && animate, dark: cx.theme().is_dark() };
            row_nameplate(plate, names("-mask", cx).flex_initial(), row)
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
        self.track_row_hover(row, &channel.id, cx)
            .h(px(DM_ROW_HEIGHT))
            .gap_3()
            .font_weight(FontWeight::NORMAL)
            .children(plate)
            .child(div().relative().child(face))
            .child(names("", cx).relative())
            .when(!selected && !hovered && self.drafts.has(&channel.id), |r| r.child(ui::row().items_stretch().flex_none().text_color(faint).child(draft_mark())))
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
