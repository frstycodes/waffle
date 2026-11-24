// The small surfaces a profile opens over itself, one at a time, as Discord's: the role picker, the guild tag's
// server card, the note, and a reply to their status, avatar or activity. Reactions go through the emoji picker.
// Both send to your DM with them as Discord's text does, then say so in a toast.
use std::time::Duration;

use gpui_kit::{
    AnyElement, Context, Div, Entity, FontWeight, Pixels, Point, SharedString, Window,
    component::{
        input::{InputEvent, InputState, Textarea},
    },
    div, img, prelude::*, px,
};
use reqwest::Method;

use super::{Ink, ProfileCard};
use crate::{
    EmojiTarget, Waffle,
    discord::{Emoji, User, perm},
    icons::Glyph,
    palette,
    ui::{self},
};

// The popovers' widths, Discord's role picker at 280 and the reply field a little wider than the bubble.
const ROLE_PICKER_WIDTH: f32 = 280.;
const REPLY_WIDTH: f32 = 300.;
const TAG_CARD_WIDTH: f32 = 260.;
const NOTE_WIDTH: f32 = 280.;
// The role list scrolls past this.
const ROLE_LIST_MAX: f32 = 280.;
// How long "Reply sent" stays, as Discord's (2.7s).
const TOAST_FOR: Duration = Duration::from_millis(2700);

// What a reply or reaction answers: their custom status (its text), their avatar, or an activity (its name).
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Source {
    Status(String),
    Avatar,
    Activity(String),
}

// The one popover open over a card.
pub(crate) enum CardPop {
    Roles { search: Entity<InputState>, at: Point<Pixels> },
    Tag { at: Point<Pixels> },
    Note { at: Point<Pixels> },
    Reply { source: Source, input: Entity<InputState>, at: Point<Pixels> },
}

impl Waffle {
    pub(super) fn card_popover(&self, card: &ProfileCard, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (content, at, corner) = match card.pop.as_ref()? {
            CardPop::Roles { search, at } => (self.role_picker(card, search, window, cx)?, *at, gpui_kit::Anchor::TopLeft),
            CardPop::Tag { at } => (self.tag_card(card, cx)?, *at, gpui_kit::Anchor::TopLeft),
            CardPop::Note { at } => (self.note_popover(card, cx)?, *at, gpui_kit::Anchor::TopLeft),
            CardPop::Reply { source, input, at } => (reply_field(&card.user, source, input, cx), *at, gpui_kit::Anchor::TopLeft),
        };
        Some(ui::floating_at("profile-pop", at, corner, ui::surface(cx).on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| cx.stop_propagation()).child(content)).into_any_element())
    }

    pub(crate) fn close_card_pop(&mut self, cx: &mut Context<Self>) -> bool {
        let open = self.profile_card.as_mut().and_then(|c| c.pop.take()).is_some();
        if open {
            self.redraw_overlays(cx);
        }
        open
    }

    fn set_card_pop(&mut self, pop: CardPop, cx: &mut Context<Self>) {
        if let Some(card) = self.profile_card.as_mut() {
            card.pop = Some(pop);
        }
        self.redraw_overlays(cx);
    }

    // Add Role: a search over the roles you can give them that they don't hold yet.
    pub(crate) fn open_role_picker(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search roles"));
        cx.observe(&search, |this, _, cx| this.redraw_overlays(cx))
            // lint-allow(detached-task): the observation lives as long as the field, dropped with the popover.
            .detach();
        search.update(cx, |state, cx| state.focus(window, cx));
        self.set_card_pop(CardPop::Roles { search, at }, cx);
    }

    fn role_picker(&self, card: &ProfileCard, search: &Entity<InputState>, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let guild = card.guild_id.as_ref().and_then(|id| self.guilds.iter().find(|g| &g.id == id))?;
        let me = self.me.as_ref()?;
        let held = self.member_roles.get(&guild.id).and_then(|m| m.get(&card.user.id)).cloned().or_else(|| card.profile.as_ref().map(|p| p.roles.clone())).unwrap_or_default();
        let top = guild.top_position(&me.id);
        let query = search.read(cx).value().trim().to_lowercase();
        let roles: Vec<_> = crate::user_menu::role_choices(guild)
            .into_iter()
            .filter(|r| !held.contains(&r.id) && !r.managed && r.position < top && guild.can(&me.id, perm::MANAGE_ROLES))
            .filter(|r| query.is_empty() || r.name.to_lowercase().contains(&query))
            .collect();
        let rows = roles.iter().map(|role| {
            let color = if role.color == 0 { palette::status_in(crate::discord::Status::Offline, cx) } else { palette::data_color(role.color) };
            let (gid, uid, rid) = (guild.id.clone(), card.user.id.clone(), role.id.clone());
            ui::menu_row(SharedString::from(format!("role-pick-{}", role.id)), cx)
                .gap_2()
                .child(div().size_2p5().flex_none().rounded_full().bg(color))
                .child(ui::line(role.name.clone()))
                .on_click(cx.listener(move |this, _, _, cx| this.add_profile_role(&gid, &uid, &rid, cx)))
        });
        let empty = roles.is_empty().then(|| div().px_2().py_3().text_size(ui::TEXT_SM).text_color(palette::muted(cx)).child("No roles"));
        // lint-allow(unvirtualized-list): a server's assignable roles, a few dozen at most (Discord caps a server at 250).
        let list = ui::col().id("role-pick-list").max_h(px(ROLE_LIST_MAX)).overflow_y_scroll().children(rows).children(empty);
        let _ = window;
        Some(ui::col().w(px(ROLE_PICKER_WIDTH)).gap_1().child(ui::search_field("role-pick-search", search, cx)).child(list).into_any_element())
    }

    fn add_profile_role(&mut self, guild_id: &str, user_id: &str, role_id: &str, cx: &mut Context<Self>) {
        self.seed_member_roles(guild_id, user_id);
        let (g1, u1, r1, g2, u2, r2) = (guild_id.to_owned(), user_id.to_owned(), role_id.to_owned(), guild_id.to_owned(), user_id.to_owned(), role_id.to_owned());
        self.rest_optimistic(Method::PUT, format!("/guilds/{guild_id}/members/{user_id}/roles/{role_id}"), None, move |this| this.set_role(&g1, &u1, &r1, true), move |this| this.set_role(&g2, &u2, &r2, false), cx);
        self.close_card_pop(cx);
    }

    // The guild tag's card: the badge and tag, the server it belongs to, and the way there when you're in it.
    // GAP: Discord's card also shows the server's member counts and a Join button for servers you're not in;
    // those come from a server profile request that isn't wired.
    pub(crate) fn open_tag_card(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.set_card_pop(CardPop::Tag { at }, cx);
    }

    fn tag_card(&self, card: &ProfileCard, cx: &mut Context<Self>) -> Option<AnyElement> {
        let tag = card.user.tag.as_ref()?;
        let guild = tag.guild_id.as_ref().and_then(|id| self.guilds.iter().find(|g| &g.id == id));
        let head = ui::row()
            .gap_2()
            .children(tag.badge.clone().map(|badge| img(badge).size_6().flex_none()))
            .child(div().text_size(ui::TEXT_LG).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(tag.text.clone()));
        let place = guild.map(|g| ui::row().gap_2().child(ui::server_tile(g.icon.as_deref(), &g.name, ui::AVATAR_SM, cx)).child(ui::line(g.name.clone()).text_color(palette::text(cx))));
        let go = guild.map(|g| {
            let id = g.id.clone();
            ui::button("tag-card-go", ui::Tone::Primary, cx).w_full().justify_center().child("Go to Server").on_click(cx.listener(move |this, _, window, cx| {
                this.profile_card = None;
                this.open_guild(Some(id.clone()), window, cx);
            }))
        });
        Some(ui::col().w(px(TAG_CARD_WIDTH)).p_2().gap_3().child(head).children(place).children(go).into_any_element())
    }

    // The note in a popover over the card, as Discord's note button opens it.
    pub(crate) fn open_note_popover(&mut self, current: String, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_profile_note(current, window, cx);
        self.set_card_pop(CardPop::Note { at }, cx);
    }

    fn note_popover(&self, card: &ProfileCard, cx: &mut Context<Self>) -> Option<AnyElement> {
        let input = card.note.as_ref()?;
        let focus = gpui_kit::Focusable::focus_handle(input.read(cx), cx);
        let field = ui::text_area("note-pop-field", Textarea::new(input).appearance(false).px_0().text_size(ui::TEXT_SM), &focus);
        let title = div().text_size(ui::TEXT_XS).font_weight(FontWeight::SEMIBOLD).text_color(palette::text(cx)).child("Note (only visible to you)");
        Some(ui::col().w(px(NOTE_WIDTH)).p_1().gap_1p5().child(title).child(field).into_any_element())
    }

    // Reply: a field under what you're answering; Enter sends it to your DM with them.
    pub(crate) fn open_reply(&mut self, user: &User, source: Source, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(reply_placeholder(&user.name, &source)));
        let (target, answered) = (user.clone(), source.clone());
        cx.subscribe_in(&input, window, move |this, input, event: &InputEvent, _, cx| {
            if !matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                return;
            }
            let text = input.read(cx).value().trim().to_owned();
            if text.is_empty() {
                return;
            }
            let content = interaction_text("Replied to", &target.name, &answered, &text);
            this.send_profile_message(target.id.clone(), content, cx);
            this.close_card_pop(cx);
            this.card_toast("Reply sent", cx);
        })
        // lint-allow(detached-task): the subscription lives as long as the field, dropped with the popover.
        .detach();
        input.update(cx, |state, cx| state.focus(window, cx));
        self.set_card_pop(CardPop::Reply { source, input, at }, cx);
    }

    // Send Reaction: the emoji picker; the pick goes to your DM with them as Discord's reaction text.
    pub(super) fn open_reaction(&mut self, source: Source, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(card) = self.profile_card.as_mut() {
            card.reacting = Some(source);
        }
        self.open_emoji_picker(EmojiTarget::ProfileReaction, window, cx);
    }

    pub(crate) fn send_profile_reaction(&mut self, emoji: Emoji, cx: &mut Context<Self>) {
        let Some(card) = self.profile_card.as_mut() else { return };
        let Some(source) = card.reacting.take() else { return };
        let user = card.user.clone();
        let face = match &emoji.id {
            Some(id) => format!("<{}:{}:{id}>", if emoji.animated { "a" } else { "" }, emoji.name),
            None => emoji.name.clone(),
        };
        self.send_profile_message(user.id.clone(), interaction_text("Reacted to", &user.name, &source, &face), cx);
        self.card_toast("Reaction sent", cx);
    }

    // A toast over the card's banner for a moment.
    fn card_toast(&mut self, text: &'static str, cx: &mut Context<Self>) {
        let Some(card) = self.profile_card.as_mut() else { return };
        card.toast = Some(text);
        let user = card.user.id.clone();
        self.redraw_overlays(cx);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TOAST_FOR).await;
            this.update(cx, |this, cx| {
                if let Some(card) = this.profile_card.as_mut().filter(|c| c.user.id == user && c.toast == Some(text)) {
                    card.toast = None;
                    this.redraw_overlays(cx);
                }
            })
            .ok();
        })
        // lint-allow(detached-task): ends after one timer, holding only a WeakEntity.
        .detach();
    }

    // React and Reply, shown while the pointer is over what they answer (`group`); others' profiles only.
    pub(super) fn interaction_toolbar(&self, card: &ProfileCard, source: Source, group: &'static str, ink: Ink, cx: &mut Context<Self>) -> Option<Div> {
        if self.me.as_ref().is_some_and(|me| me.id == card.user.id) || card.user.bot {
            return None;
        }
        let (react_source, reply_source, user) = (source.clone(), source.clone(), card.user.clone());
        let button = |id: &str, glyph, tip: &'static str, cx: &mut Context<Self>| {
            ui::icon_button(SharedString::from(format!("{id}-{group}")), glyph, tip, cx).scoped(ui::mix(ink.top, ink.strong, 0.06), ink.tile, ink.muted, ink.strong).small()
        };
        let react = button("profile-react", Glyph::Emoji, "Send Reaction", cx).on_click(cx.listener(move |this, _, window, cx| this.open_reaction(react_source.clone(), window, cx)));
        let reply = button("profile-reply", Glyph::Reply, "Reply", cx).on_click(cx.listener(move |this, event: &gpui_kit::ClickEvent, window, cx| this.open_reply(&user, reply_source.clone(), event.position(), window, cx)));
        Some(ui::surface_in(ui::mix(ink.top, ink.strong, 0.06), ink.tile, cx).flex_row().items_center().gap_0p5().opacity(0.).group_hover(group, |s| s.opacity(1.)).child(react).child(reply))
    }
}

fn reply_field(user: &User, source: &Source, input: &Entity<InputState>, cx: &App) -> AnyElement {
    let _ = (user, source);
    let field = ui::search_frame("reply-field", gpui_kit::component::input::Input::new(input), &gpui_kit::Focusable::focus_handle(input.read(cx), cx));
    ui::col().w(px(REPLY_WIDTH)).p_1().child(field).into_any_element()
}

use gpui_kit::App;

fn reply_placeholder(name: &str, source: &Source) -> String {
    match source {
        Source::Status(_) => format!("Reply to {name}'s status"),
        Source::Avatar => format!("Reply to {name}'s avatar"),
        Source::Activity(_) => format!("Reply to {name}'s activity"),
    }
}

// Discord's quote of what was answered, as a small-text header (`-#`) in a block quote, then the message:
//   > -# *Replied to Wren's status*
//   > 🍯 drizzling honey on everything
//   nice
pub(crate) fn interaction_text(verb: &str, name: &str, source: &Source, message: &str) -> String {
    let (what, detail) = match source {
        Source::Status(text) => ("status", Some(text.as_str())),
        Source::Avatar => ("avatar", None),
        Source::Activity(name) => ("activity", Some(name.as_str())),
    };
    let quote = detail.filter(|d| !d.is_empty()).map(|d| format!("\n> {d}")).unwrap_or_default();
    format!("> -# *{verb} {name}'s {what}*{quote}\n{message}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_quote_what_they_answer() {
        assert_eq!(interaction_text("Replied to", "Wren", &Source::Status("brb".into()), "ok"), "> -# *Replied to Wren's status*\n> brb\nok");
        assert_eq!(interaction_text("Reacted to", "Wren", &Source::Avatar, "🔥"), "> -# *Reacted to Wren's avatar*\n🔥");
        assert_eq!(interaction_text("Replied to", "Wren", &Source::Activity("Celeste".into()), "gg"), "> -# *Replied to Wren's activity*\n> Celeste\ngg");
        assert_eq!(reply_placeholder("Wren", &Source::Avatar), "Reply to Wren's avatar");
    }
}
