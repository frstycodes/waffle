// A 1:1 DM's side panel: the person's profile, as Discord shows it beside the conversation.
use gpui_kit::{AnyElement, Context, SharedString, Window, div, prelude::*, px};

use super::{FullTab, Header, Ink, ProfileCard, profile_key};
use crate::{Edge, RightPanel, Waffle, discord::{Channel, User}, icons::Glyph, palette, ui::{self, Squircle}};

impl Waffle {
    // The person in the open 1:1 DM; a group DM has its member list instead.
    pub(crate) fn dm_recipient(&self) -> Option<&User> {
        self.channel.as_ref().filter(|c| c.raw_type == 1)?.recipient.as_ref()
    }

    // The side column under the full-width channel header: their profile's header and sections as a
    // rounded panel floating 8px in from the column's top, right and foot (flush on its left, where the feed's message highlights meet it), in their profile's colours (the sidebar's tone
    // without a theme). The banner, avatar and decoration play while the pointer is over it.
    pub(crate) fn dm_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let user = self.dm_recipient()?.clone();
        self.fetch_profile(user.clone(), None, cx);
        let profile = self.profiles.peek(&profile_key(&user.id, None));
        // A cached profile skips the fetch that would ask for its effect.
        self.fetch_profile_effect(profile.as_ref().and_then(|p| p.effect_sku.clone()), cx);
        let (hovered, on_hover) = ui::hover_state(SharedString::from(format!("dm-profile-hover-{}", user.id)), window, cx);
        // The panel keeps its card in the profile slot while nothing else is open there, so its popovers, toast,
        // folded roles and expanded status have somewhere to live.
        match self.profile_card.as_mut() {
            Some(card) if card.panel && card.user.id == user.id => card.profile = profile.clone(),
            Some(card) if !card.panel => {}
            _ => self.profile_card = Some(ProfileCard { panel: true, ..ProfileCard::new(user.clone(), None, Default::default(), profile.clone(), None) }),
        }
        let pinned = match self.profile_card.as_ref() {
            Some(c) if c.panel && c.user.id == user.id => self.profile_card.take(),
            _ => None,
        };
        let card = match pinned {
            Some(card) => card,
            None => ProfileCard::new(user, None, Default::default(), profile, None),
        };
        let profile = card.profile.clone().unwrap_or_default();
        let ink = Ink::new(profile.theme, cx);
        let content = ui::col().id("dm-profile").child(self.profile_header(&card, &profile, Header::Panel, hovered, ink, cx)).child(self.panel_sections(&card, &profile, ink, cx));
        let target = card.user.clone();
        let view_full = ui::button("dm-profile-view-full", ui::Tone::Secondary, cx).w_full().justify_center().child("View Full Profile").on_click(cx.listener(move |this, _, _, cx| this.open_full_profile(target.clone(), FullTab::Activity, cx)));
        // The panel's width: its column less the 8px gap to the window's edge, and less the frame's reach past each
        // side when it wears one: Discord shrinks the whole panel so the frame fits its column.
        let column_width = self.panel_column_width().unwrap_or(super::WIDTH) - PANEL_INSET;
        let fit = FrameFit::of(profile.frame.as_ref().filter(|_| f32::from(window.viewport_size().height) >= super::Surface::Panel.frame_min()), column_width);
        let dressing = self.dressing(&card, super::Surface::Panel, fit.width, hovered, window, cx);
        let panel = ui::col().relative().min_h_0().id("dm-profile-panel").group(super::CARD_GROUP).flex_1().squircle(ui::RADIUS_LG).on_hover(on_hover);
        let panel = match ink.border {
            Some(_) => panel.bg(ink.wash()),
            None => panel.bg(palette::surface(palette::Level::Sidebar, cx)),
        };
        let panel = panel.child(ui::scroll_area(content, &self.panel_scroll)).child(div().flex_none().p_4().child(view_full)).children(dressing.effect);
        let panel = ui::col().relative().flex_1().min_h_0().children(dressing.back).child(panel).children(dressing.front);
        let column = ui::col()
            .relative()
            .size_full()
            .pt(px(PANEL_INSET + fit.top))
            .pr(px(PANEL_INSET + fit.side))
            .pb(px(PANEL_INSET + fit.bottom))
            .pl(px(fit.side))
            .bg(palette::surface(palette::Level::Page, cx));
        let column = column.child(panel).child(self.resize_handle(Edge::Panel, cx));
        if card.panel {
            self.profile_card = Some(card);
        }
        Some(column.into_any_element())
    }

    // Discord's side panel body: name, mutuals, bio, Member Since, then what they're doing.
    fn panel_sections(&self, card: &ProfileCard, profile: &crate::discord::Profile, ink: Ink, cx: &mut Context<Self>) -> gpui_kit::Div {
        ui::col()
            .px_4()
            .pb_3()
            .gap_3()
            .child(self.clickable_name(card, profile, ink, cx))
            .children(self.relationship_notice(card, ink, cx))
            .children(self.mutuals_row(&card.user, profile, ink, cx))
            .children(profile.bio.as_ref().map(|bio| super::bio_view(&card.user.id, bio, ink)))
            .children(self.member_since(card, profile, ink, cx).map(|row| super::section(if card.user.bot { "Created On" } else { "Member Since" }, row, ink)))
            .children(self.profile_activity(card, ink, cx))
    }

    // The header's side panel toggle: in a 1:1 DM it shows the person's profile, in a server or group the
    // member list.
    pub(crate) fn members_toggle(&self, channel: &Channel, cx: &mut Context<Self>) -> Option<ui::Control> {
        let dm = self.dm_recipient().is_some();
        if !dm && self.guild_id.is_none() && channel.raw_type != 3 {
            return None;
        }
        let on = matches!(self.right_panel, RightPanel::Members);
        let (glyph, tip) = match (dm, on) {
            (true, true) => (Glyph::UserCircle, "Hide User Profile"),
            (true, false) => (Glyph::UserCircle, "Show User Profile"),
            (false, true) => (Glyph::Users, "Hide Member List"),
            (false, false) => (Glyph::Users, "Show Member List"),
        };
        let toggle = ui::icon_button("members-toggle", glyph, tip, cx).occlude().when(on, |b| b.bg(palette::selected(cx)).text_color(palette::strong(cx)));
        Some(toggle.on_click(cx.listener(|this, _, _, cx| {
            this.right_panel = if matches!(this.right_panel, RightPanel::Members) { RightPanel::Hidden } else { RightPanel::Members };
            cx.notify(); // lint-allow(global-notify): showing or hiding the side panel resizes the main column beside it
        })))
    }

    // Where the channel header goes: across the top, over the feed and the profile, while a DM's profile
    // shows (as Discord's); otherwise inside the chat column, beside the member list's own header.
    pub(crate) fn place_header<E>(&self, header: E) -> (Option<E>, Option<E>) {
        match self.header_over_profile() {
            true => (None, Some(header)),
            false => (Some(header), None),
        }
    }

    pub(crate) fn header_over_profile(&self) -> bool {
        self.panel_column_width().is_some() && self.dm_recipient().is_some()
    }
}

// The gap between the panel and the window's right edge (`pr_2`).
const PANEL_INSET: f32 = 8.;

// Room a frame needs around the panel: the panel's width once it fits, and the frame's reach past its top, foot
// and each side, all at the frame's scale for that width.
struct FrameFit {
    width: f32,
    top: f32,
    bottom: f32,
    side: f32,
}

impl FrameFit {
    fn of(frame: Option<&crate::discord::ProfileFrame>, column: f32) -> Self {
        let Some(frame) = frame.filter(|f| f.inner_width > 0.) else { return Self { width: column, top: 0., bottom: 0., side: 0. } };
        // The card plus twice its overhang fills the column: w + 2·h·(w / inner) = column.
        let width = column / (1. + 2. * frame.overflow_horizontal / frame.inner_width);
        let k = width / frame.inner_width;
        Self { width, top: frame.overflow_top * k, bottom: frame.overflow_bottom * k, side: frame.overflow_horizontal * k }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_shrinks_the_panel_to_fit() {
        let none = FrameFit::of(None, 300.);
        assert_eq!((none.width, none.side), (300., 0.));
        let frame = crate::discord::ProfileFrame { inner_width: 1200., overflow_top: 200., overflow_bottom: 100., overflow_horizontal: 60., layers: Vec::new() };
        let fit = FrameFit::of(Some(&frame), 330.);
        assert!((fit.width + 2. * fit.side - 330.).abs() < 0.01);
        assert!((fit.width - 300.).abs() < 0.01 && (fit.top - 50.).abs() < 0.01 && (fit.bottom - 25.).abs() < 0.01);
    }
}
