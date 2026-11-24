// A 1:1 DM's side panel: the person's profile, as Discord shows it beside the conversation.
use gpui_kit::{AnyElement, Context, SharedString, Window, div, prelude::*};

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
        // GAP: a failed fetch is retried on the panel's next redraw; fine while redraws are occasional.
        self.fetch_profile(user.clone(), None, cx);
        let profile = self.profiles.peek(&profile_key(&user.id, None));
        let (hovered, on_hover) = ui::hover_state(SharedString::from(format!("dm-profile-hover-{}", user.id)), window, cx);
        let card = ProfileCard::new(user, None, Default::default(), profile, None);
        let profile = card.profile.clone().unwrap_or_default();
        let ink = Ink::new(profile.theme, cx);
        let content = ui::col().id("dm-profile").child(self.profile_header(&card, &profile, Header::Panel, hovered, ink, cx)).child(self.panel_sections(&card, &profile, ink, cx));
        let target = card.user.clone();
        let view_full = ui::button("dm-profile-view-full", ui::Tone::Secondary, cx).w_full().justify_center().child("View Full Profile").on_click(cx.listener(move |this, _, _, cx| this.open_full_profile(target.clone(), FullTab::Activity, cx)));
        let panel = ui::col().min_h_0().id("dm-profile-panel").group(super::CARD_GROUP).flex_1().squircle(ui::RADIUS_LG).on_hover(on_hover);
        let panel = match ink.border {
            Some(_) => panel.bg(ink.wash()),
            None => panel.bg(palette::surface(palette::Level::Sidebar, cx)),
        };
        let panel = panel.child(ui::scroll_area(content, &self.panel_scroll)).child(div().flex_none().p_4().child(view_full));
        let column = ui::col().relative().size_full().pt_2().pr_2().pb_2().bg(palette::surface(palette::Level::Page, cx));
        Some(column.child(panel).child(self.resize_handle(Edge::Panel, cx)).into_any_element())
    }

    // Discord's side panel body: name, mutuals, bio, Member Since, then what they're doing.
    fn panel_sections(&self, card: &ProfileCard, profile: &crate::discord::Profile, ink: Ink, cx: &mut Context<Self>) -> gpui_kit::Div {
        ui::col()
            .px_4()
            .pb_3()
            .gap_3()
            .child(self.clickable_name(card, profile, ink, cx))
            .children(self.mutuals_row(&card.user, profile, ink, cx))
            .children(profile.bio.as_ref().map(|bio| super::bio_view(&card.user.id, bio, ink)))
            .children(self.member_since(card, profile, ink, cx))
            .children(self.profile_activity_cards(card, ink, cx))
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
        match self.panel_column_width().is_some() && self.dm_recipient().is_some() {
            true => (None, Some(header)),
            false => (Some(header), None),
        }
    }
}
