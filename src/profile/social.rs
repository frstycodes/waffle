// Where you stand with them, as Discord's profile says it under the name: their pending friend request with
// Accept and Ignore, or that you blocked or ignored them; and the mutual lists' rows with what each is doing.
use gpui_kit::{Context, Div, FontWeight, SharedString, prelude::*};

use super::{Ink, ProfileCard};
use crate::{
    Waffle,
    discord::{Relation, User},
    ui::{self, RADIUS_MD, Squircle},
};

impl Waffle {
    pub(super) fn relationship_notice(&self, card: &ProfileCard, ink: Ink, cx: &mut Context<Self>) -> Option<Div> {
        let user = &card.user;
        if self.me.as_ref().is_some_and(|me| me.id == user.id) {
            return None;
        }
        // lint-allow(custom-badge): ui::Alert::strip takes a palette fill; this notice wears the profile theme's tint (Ink), as the card's other tiles do.
        let notice = || ui::col().p_3().gap_2().squircle(RADIUS_MD).bg(ink.tile).text_size(ui::TEXT_SM).text_color(ink.text);
        match self.relation(&user.id) {
            Some(Relation::Incoming) => {
                let (accept, ignore) = (user.clone(), user.id.clone());
                let line = ui::row().flex_wrap().child(div_bold(user.name.clone(), ink)).child("\u{a0}sent you a friend request.");
                let buttons = ui::row()
                    .gap_2()
                    .child(ui::Button::new("profile-request-accept").size(ui::ButtonSize::Sm).child("Accept").on_click(cx.listener(move |this, _, _, cx| this.befriend(accept.clone(), cx))))
                    .child(ui::Button::new("profile-request-ignore").variant(ui::ButtonVariant::Secondary).size(ui::ButtonSize::Sm).child("Ignore").on_click(cx.listener(move |this, _, _, cx| this.drop_relation(&ignore, cx))));
                Some(notice().child(line).child(buttons))
            }
            Some(Relation::Blocked) => Some(notice().font_weight(FontWeight::SEMIBOLD).child("You blocked them")),
            _ if self.ignored.contains(&user.id) => {
                let id = user.id.clone();
                let unignore = ui::Button::new("profile-unignore").variant(ui::ButtonVariant::Link).child("Unignore").on_click(cx.listener(move |this, _, _, cx| this.set_ignored(id.clone(), false, cx)));
                Some(notice().flex_row().items_center().gap_1().font_weight(FontWeight::SEMIBOLD).child("You ignored them").child("•").child(unignore))
            }
            _ => None,
        }
    }

    // A mutual friend's line under their name: what they're doing, else their custom status.
    pub(super) fn presence_line(&self, user: &User) -> Option<String> {
        let presence = self.presences.get(&user.id)?;
        let doing = presence.activity.as_ref().map(|a| format!("{} {}", super::activity_text::activity_heading(a.kind), a.text));
        doing.or_else(|| presence.custom.clone())
    }

    // Mutual servers in your own server order, as Discord lists them.
    pub(super) fn mutual_servers_in_order(&self, card: &ProfileCard) -> Vec<(String, String, Option<String>, Option<String>)> {
        let Some(profile) = card.profile.as_ref() else { return Vec::new() };
        self.guilds
            .iter()
            .filter_map(|g| profile.mutual_guilds.iter().find(|m| m.id == g.id).map(|m| (g.id.clone(), g.name.clone(), g.icon.clone(), m.nick.clone())))
            .collect()
    }
}

fn div_bold(text: String, ink: Ink) -> Div {
    gpui_kit::div().font_weight(FontWeight::SEMIBOLD).text_color(ink.strong).child(SharedString::from(text))
}
