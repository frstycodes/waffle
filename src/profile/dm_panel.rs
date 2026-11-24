// A 1:1 DM's side panel: the person's profile, as Discord shows it beside the conversation.
use gpui_kit::{AnyElement, Context, SharedString, Window, linear_color_stop, linear_gradient, prelude::*};

use super::{Ink, profile_key};
use crate::{Edge, ProfileCard, RightPanel, Waffle, discord::{Channel, User}, icons::Glyph, palette, ui::{self, Squircle}};

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
        let (hovered, on_hover) = ui::hover_state(SharedString::from(format!("dm-profile-hover-{}", user.id)), window, cx);
        let card = ProfileCard { user, guild_id: None, at: Default::default(), profile, all_activity: false, avatar_full: false };
        let profile = card.profile.clone().unwrap_or_default();
        let ink = Ink::new(profile.theme, cx);
        let content = ui::col().id("dm-profile").child(self.profile_header(&card, &profile, ui::RADIUS_LG, hovered, ink, cx)).child(self.profile_sections(&card, &profile, ink, cx));
        let panel = ui::col().min_h_0().id("dm-profile-panel").flex_1().squircle(ui::RADIUS_LG).on_hover(on_hover);
        let panel = match ink.border {
            Some(_) => panel.bg(linear_gradient(180., linear_color_stop(ink.top, 0.), linear_color_stop(ink.bottom, 1.))),
            None => panel.bg(palette::surface(palette::Level::Sidebar, cx)),
        };
        let panel = panel.child(ui::scroll_area(content, &self.panel_scroll));
        let column = ui::col().relative().size_full().pt_2().pr_2().pb_2().bg(palette::surface(palette::Level::Page, cx));
        Some(column.child(panel).child(self.resize_handle(Edge::Panel, cx)).into_any_element())
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
