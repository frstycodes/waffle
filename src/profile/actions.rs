// What a profile lets you do in place, as Discord's does: remove and add roles, act on the friendship,
// message them from the card, and set your own status from its bubble.
use gpui_kit::{
    AnyElement, ClickEvent, Context, Div, FontWeight, MouseButton, MouseDownEvent, SharedString, Window,
    component::{
        Icon,
        input::{Input, InputEvent, InputState},
    },
    div, prelude::*, px,
};
use reqwest::Method;

use super::{Ink, ProfileCard};
use crate::{
    Waffle,
    discord::{Profile, Relation, Role, User, perm},
    icons::Glyph,
    menu::{Entry, item},
    palette,
    ui::{self, RADIUS_MD, RADIUS_XS, Squircle},
};

// Discord folds the popout's roles to two rows of its 268px column, "+N" opening the rest in place. Chips
// are measured here from their names: 12px type at about 6.5px a character, plus the dot, padding and gap.
const ROLE_ROWS: usize = 2;
const ROLE_COLUMN: f32 = 268.;
const ROLE_CHAR: f32 = 6.5;
const ROLE_CHROME: f32 = 34.;
// Room the "+N" chip and the add button keep on the last row.
const ROLE_TAIL: f32 = 64.;

impl Waffle {
    // Their roles, each with a remove ✕ for roles you can take away, then Add Role. `fold` keeps the
    // popout's two rows behind "+N".
    pub(super) fn roles_row(&self, card: &ProfileCard, profile: &Profile, fold: bool, ink: Ink, cx: &mut Context<Self>) -> Option<Div> {
        let guild = card.guild_id.as_ref().and_then(|id| self.guilds.iter().find(|g| &g.id == id))?;
        let me = self.me.as_ref()?;
        let held = self.held_roles(&guild.id, &card.user.id, profile);
        let mut roles: Vec<&Role> = guild.roles.iter().filter(|r| held.contains(&r.id)).collect();
        // lint-allow(render-work): one member's roles in this server, a few dozen at most.
        roles.sort_by_key(|r| std::cmp::Reverse(r.position));
        let can_manage = guild.can(&me.id, perm::MANAGE_ROLES);
        if roles.is_empty() && !can_manage {
            return None;
        }
        let top = guild.top_position(&me.id);
        let shown = match fold && !card.all_roles {
            true => roles_fitting(&roles.iter().map(|r| r.name.as_str()).collect::<Vec<_>>()),
            false => roles.len(),
        };
        let hidden = roles.len() - shown;
        let chips = roles.iter().take(shown).map(|role| {
            let removable = can_manage && role.position < top && !role.managed;
            self.role_chip(card, &guild.id, role, removable, ink, cx)
        }).collect::<Vec<_>>();
        let more = (hidden > 0).then(|| {
            ui::button("profile-roles-more", ui::Tone::Ghost, cx)
                .scoped(ink.tile, ink.strong.opacity(0.16), ink.text, ink.strong)
                .h_6()
                .px_2()
                .squircle(RADIUS_XS)
                .text_size(ui::TEXT_XS)
                .child(format!("+{hidden}"))
                .tooltip(|window, cx| ui::tip("View All Roles", window, cx))
                .on_click(cx.listener(|this, _, _, cx| this.show_all_roles(true, cx)))
        });
        let add = can_manage.then(|| {
            let (user, guild_id) = (card.user.clone(), guild.id.clone());
            ui::icon_button("profile-add-role", Glyph::Plus, "Add Role", cx)
                .scoped(gpui_kit::transparent_black(), ink.tile, ink.muted, ink.strong)
                .small()
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| this.open_role_picker(user.clone(), guild_id.clone(), event.position(), cx)))
        });
        Some(ui::row().flex_wrap().gap_1().children(chips).children(more).children(add))
    }

    // One role: its colour and name, a ✕ while hovered when you can remove it, Copy Role ID on right-click
    // with Developer Mode on.
    fn role_chip(&self, card: &ProfileCard, guild_id: &str, role: &Role, removable: bool, ink: Ink, cx: &mut Context<Self>) -> AnyElement {
        let color = if role.color == 0 { palette::status_in(crate::discord::Status::Offline, cx) } else { palette::data_color(role.color) };
        let group = SharedString::from(format!("profile-role-{}", role.id));
        let remove = removable.then(|| {
            let (guild, user, role_id) = (guild_id.to_owned(), card.user.id.clone(), role.id.clone());
            ui::icon_button(SharedString::from(format!("profile-role-remove-{}", role.id)), Glyph::X, "Remove Role", cx)
                .scoped(gpui_kit::transparent_black(), ink.tile, ink.muted, ink.strong)
                .size_4()
                .opacity(0.)
                .group_hover(group.clone(), |s| s.opacity(1.))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.remove_profile_role(&guild, &user, &role_id, cx);
                }))
        });
        let copy = self.developer_mode.then(|| role.id.clone());
        let chip = ui::role_chip(role.name.clone(), color, cx).bg(ink.tile).text_color(ink.text).children(remove).group(group.clone()).id(group);
        match copy {
            Some(role_id) => chip
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        let role_id = role_id.clone();
                        this.open_live_menu(event.position, move |_| vec![item("Copy Role ID", { let id = role_id.clone(); move |this, _, cx| this.copy_text(id.clone(), cx) }).icon(Glyph::Copy).entry()], cx);
                    }),
                )
                .into_any_element(),
            None => chip.into_any_element(),
        }
    }

    // The roles they hold: the member list's copy once loaded (role changes land there), else the profile's.
    fn held_roles(&self, guild_id: &str, user_id: &str, profile: &Profile) -> Vec<String> {
        self.member_roles.get(guild_id).and_then(|m| m.get(user_id)).cloned().unwrap_or_else(|| profile.roles.clone())
    }

    // Before a change from the card, the member's role list starts from what the card shows, so toggling one
    // role never drops the others.
    fn seed_member_roles(&mut self, guild_id: &str, user_id: &str) {
        let Some(roles) = self.profile_card.as_ref().and_then(|c| c.profile.as_ref()).map(|p| p.roles.clone()) else { return };
        self.member_roles.entry(guild_id.to_owned()).or_default().entry(user_id.to_owned()).or_insert(roles);
    }

    // Takes the role away now, puts it back if Discord refuses.
    fn remove_profile_role(&mut self, guild_id: &str, user_id: &str, role_id: &str, cx: &mut Context<Self>) {
        self.seed_member_roles(guild_id, user_id);
        let (g1, u1, r1, g2, u2, r2) = (guild_id.to_owned(), user_id.to_owned(), role_id.to_owned(), guild_id.to_owned(), user_id.to_owned(), role_id.to_owned());
        self.rest_optimistic(Method::DELETE, format!("/guilds/{guild_id}/members/{user_id}/roles/{role_id}"), None, move |this| this.set_role(&g1, &u1, &r1, false), move |this| this.set_role(&g2, &u2, &r2, true), cx);
        self.redraw_overlays(cx);
    }

    // Add Role: the server's roles as toggles, the ones they hold ticked.
    // GAP: Discord's picker lists only the roles they don't have, with a search field; this is the user menu's toggle list.
    fn open_role_picker(&mut self, user: User, guild_id: String, at: gpui_kit::Point<gpui_kit::Pixels>, cx: &mut Context<Self>) {
        self.seed_member_roles(&guild_id, &user.id);
        self.open_live_menu(at, move |this| this.guilds.iter().find(|g| g.id == guild_id).map(|guild| this.role_entries(guild, &user.id)).unwrap_or_default(), cx);
    }

    fn show_all_roles(&mut self, all: bool, cx: &mut Context<Self>) {
        if let Some(card) = self.profile_card.as_mut() {
            card.all_roles = all;
        }
        self.redraw_overlays(cx);
    }
}

impl Waffle {
    // The friend button: Add Friend and Accept act at once; a friend or a pending request opens Discord's
    // Friend Actions menu (Remove Friend, Cancel Request, Ignore).
    pub(super) fn friend_click(&mut self, user: User, at: gpui_kit::Point<gpui_kit::Pixels>, cx: &mut Context<Self>) {
        match self.relation(&user.id) {
            None => self.befriend(user, cx),
            Some(Relation::Blocked) => {}
            Some(_) => self.open_live_menu(at, move |this| this.friend_entries(&user), cx),
        }
    }

    fn friend_entries(&self, user: &User) -> Vec<Entry> {
        match self.relation(&user.id) {
            Some(Relation::Friend) => vec![self.friend_action_item("Remove Friend", user, true)],
            Some(Relation::Outgoing) => vec![self.friend_action_item("Cancel Friend Request", user, false)],
            Some(Relation::Incoming) => {
                let friend = user.clone();
                vec![item("Accept", move |this, _, cx| this.befriend(friend.clone(), cx)).icon(Glyph::GroupPlus).entry(), self.friend_action_item("Ignore", user, false)]
            }
            None | Some(Relation::Blocked) => Vec::new(),
        }
    }

    // Removing a friend asks first, as the user menu does; cancelling or ignoring a request doesn't.
    fn friend_action_item(&self, label: &'static str, user: &User, confirm: bool) -> Entry {
        let (id, who) = (user.id.clone(), user.name.clone());
        item(label, move |this, _, cx| {
            if !confirm {
                return this.drop_relation(&id, cx);
            }
            let id = id.clone();
            this.confirm(format!("Remove '{who}'"), format!("Are you sure you want to remove {who} from your friends?"), "Remove Friend", move |this, _, cx| this.drop_relation(&id, cx), cx);
        })
        .icon(Glyph::UserMinus)
        .danger()
        .entry()
    }

    // The popout's foot: "Message @name", a real field once clicked; Enter sends it to your DM with them.
    // lint-allow(custom-component): ui::text_field is the settings field frame; this one wears the card's themed fill.
    pub(super) fn message_field(&self, card: &ProfileCard, ink: Ink, cx: &mut Context<Self>) -> Div {
        let user = card.user.clone();
        let content = match &card.message {
            Some(input) => Input::new(input).appearance(false).text_size(ui::TEXT_SM).into_any_element(),
            None => ui::line(format!("Message @{}", user.name)).text_size(ui::TEXT_SM).text_color(ink.muted).into_any_element(),
        };
        let field = ui::row()
            .id("profile-message")
            .h_11()
            .px_3p5()
            .gap_2()
            .squircle(RADIUS_MD)
            .bg(ink.tile)
            .text_color(ink.strong)
            .cursor_text()
            .child(div().flex_1().min_w_0().child(content))
            .child(Icon::new(Glyph::Emoji).size(ui::ICON_MD).flex_none().text_color(ink.muted))
            .when(card.message.is_none(), |f| f.on_click(cx.listener(move |this, _, window, cx| this.start_profile_message(&user, window, cx))));
        div().flex_none().px_4().pt_1().pb_3().child(field)
    }

    fn start_profile_message(&mut self, user: &User, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(format!("Message @{}", user.name)));
        let user_id = user.id.clone();
        cx.subscribe_in(&input, window, move |this, input, event: &InputEvent, window, cx| {
            if !matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                return;
            }
            let text = input.read(cx).value().trim().to_owned();
            if text.is_empty() {
                return;
            }
            input.update(cx, |state, cx| state.set_value("", window, cx));
            this.send_profile_message(user_id.clone(), text, cx);
        })
        // lint-allow(detached-task): the subscription lives as long as the field, dropped with the card.
        .detach();
        input.update(cx, |state, cx| state.focus(window, cx));
        if let Some(card) = self.profile_card.as_mut() {
            card.message = Some(input);
        }
        self.redraw_overlays(cx);
    }

    // Sends to the DM with them, opening it first if there's none; the message lands there over the gateway.
    fn send_profile_message(&mut self, user_id: String, text: String, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let known = self.dms.iter().find(|d| d.recipient.as_ref().is_some_and(|r| r.id == user_id)).map(|d| d.id.clone());
        cx.spawn(async move |this, cx| {
            let channel = match known {
                Some(id) => id,
                None => match client.open_dm(&user_id).await {
                    Ok(dm) => {
                        let id = dm.id.clone();
                        this.update(cx, |this, _| if !this.dms.iter().any(|d| d.id == dm.id) { this.dms.insert(0, dm) }).ok();
                        id
                    }
                    Err(_) => return,
                },
            };
            // GAP: a failed send is dropped silently; Discord's popout shows nothing either (not verified).
            let _ = client.send_message(&channel, &text, &crate::nonce(), None, false, Default::default()).await;
        })
        // lint-allow(detached-task): ends when the send returns, and holds only a WeakEntity.
        .detach();
    }

    // Your own bubble: your status opens the editor; without one, "Add Status" in its place, as Discord's.
    pub(super) fn own_status_bubble(&self, ink: Ink, cx: &mut Context<Self>) -> Div {
        let text = self.custom_status.as_ref().map(|_| self.status_line()).filter(|s| !s.is_empty());
        let empty = text.is_none();
        let content = div()
            .min_w_0()
            .line_clamp(2)
            .text_color(if empty { ink.muted } else { ink.text })
            .font_weight(if empty { FontWeight::MEDIUM } else { FontWeight::NORMAL })
            .child(text.unwrap_or_else(|| "Add Status".into()));
        let fill = if ink.border.is_some() { ui::mix(ink.top, ink.strong, 0.08) } else { palette::surface(palette::Level::Raised, cx) };
        ui::row().items_start().child(
            ui::thought_bubble("profile-own-status", content, fill, palette::line(cx))
                .max_w(px(OWN_BUBBLE_MAX))
                .cursor_pointer()
                .on_click(cx.listener(|this, _, window, cx| {
                    this.profile_card = None;
                    this.open_status_editor(window, cx);
                })),
        )
    }
}

// Discord's bubble caps at 181px.
const OWN_BUBBLE_MAX: f32 = 181.;

// How many of these role names fit the popout's two rows, leaving the last row room for "+N" and Add Role.
fn roles_fitting(names: &[&str]) -> usize {
    let (mut row, mut used) = (0, 0.);
    for (i, name) in names.iter().enumerate() {
        let width = (name.chars().count() as f32 * ROLE_CHAR + ROLE_CHROME).min(ROLE_COLUMN);
        let room = if row + 1 == ROLE_ROWS { ROLE_COLUMN - ROLE_TAIL } else { ROLE_COLUMN };
        if used + width > room {
            row += 1;
            used = 0.;
            if row == ROLE_ROWS {
                return i;
            }
        }
        used += width;
    }
    names.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_fold_to_two_rows() {
        assert_eq!(roles_fitting(&["Rustacean"]), 1);
        assert_eq!(roles_fitting(&[]), 0);
        let long = ["very long role name number"; 14];
        let fits = roles_fitting(&long);
        assert!((1..14).contains(&fits), "{fits}");
    }
}
