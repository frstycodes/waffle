// Who reacted (design/explore-reactions.html), as Discord shows it: a card over a reaction pill naming the
// first three people, its "N others" opening the Reactions dialog, which lists everyone for every reaction
// on the message, a page of 100 at a time.
use std::ops::Range;

use gpui_kit::{
    AnyElement, Context, FontWeight, HighlightStyle, InteractiveText, Render, ScrollHandle, SharedString, StyledText, WeakEntity, Window, div, prelude::*, px,
};

use crate::{
    Waffle,
    call::tabular,
    discord::{Emoji, Message, Reaction, Reactor, perm},
    icons::Glyph,
    link_preview::grouped,
    menu::emoji_face,
    message_menu::shortcode,
    overlays::backdrop,
    palette,
    ui::{self, RADIUS_MD, Squircle},
};

const PAGE: usize = 100;
const DIALOG_WIDTH: f32 = 560.;
const DIALOG_HEIGHT: f32 = 600.;
const RAIL_WIDTH: f32 = 112.;
const AVATAR: f32 = 36.;
// Distance from the list's end, in pixels, at which the next page is asked for.
const PREFETCH: f32 = 400.;

/// Who reacted with one emoji on one message, as far as it has loaded.
#[derive(Default)]
pub(crate) struct Reactors {
    pub users: Vec<Reactor>,
    loading: bool,
    failed: bool,
    done: bool,
}

impl Reactors {
    pub fn loaded(users: Vec<Reactor>) -> Self {
        Reactors { users, done: true, ..Default::default() }
    }

    pub fn failed() -> Self {
        Reactors { failed: true, ..Default::default() }
    }

    // The `after` cursor for the next page: the last user loaded, while more remain and none is in flight.
    fn next_after(&self) -> Option<&str> {
        if self.loading || self.failed || self.done {
            return None;
        }
        self.users.last().map(|u| u.user.id.as_str())
    }
}

pub(crate) struct ReactionsDialog {
    channel_id: String,
    message_id: String,
    // The message as opened, for when it isn't in a loaded feed (the Inbox).
    message: Message,
    selected: Emoji,
    hovered: Option<String>,
    scroll: ScrollHandle,
}

/// The card a reaction pill shows on hover.
#[derive(Clone)]
pub(crate) struct ReactorsCard {
    this: WeakEntity<Waffle>,
    channel_id: String,
    message_id: String,
    reaction: Reaction,
}

impl ReactorsCard {
    pub fn new(this: WeakEntity<Waffle>, message: &Message, reaction: &Reaction) -> Self {
        ReactorsCard { this, channel_id: message.channel_id.clone(), message_id: message.id.clone(), reaction: reaction.clone() }
    }
}

impl Render for ReactorsCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::perf::probe("ReactorsCard");
        let Some(waffle) = self.this.upgrade() else { return div().into_any_element() };
        let waffle = waffle.read(cx);
        // Its link opened the dialog; a hoverable tooltip stays up while the pointer rests on it, so it steps aside.
        if waffle.reactions_dialog.is_some() {
            return div().into_any_element();
        }
        let key = (self.message_id.clone(), self.reaction.emoji.path());
        let names: Vec<String> = waffle.reactors.get(&key).map(|r| r.users.iter().take(3).map(|u| waffle.member_name(&u.user)).collect()).unwrap_or_default();
        let line = reactors_line(&names, self.reaction.count, &shortcode(&self.reaction.emoji));
        let strong = HighlightStyle { color: Some(palette::strong(cx)), font_weight: Some(FontWeight::SEMIBOLD), ..Default::default() };
        let link = HighlightStyle { color: Some(palette::accent_ink(cx)), font_weight: Some(FontWeight::SEMIBOLD), ..Default::default() };
        let mut runs: Vec<(Range<usize>, HighlightStyle)> = line.strong.iter().map(|r| (r.clone(), strong)).chain(line.link.clone().map(|r| (r, link))).collect();
        runs.sort_by_key(|(r, _)| r.start);
        let (this, channel_id, message_id, emoji) = (self.this.clone(), self.channel_id.clone(), self.message_id.clone(), self.reaction.emoji.clone());
        let text = InteractiveText::new("reactors-text", StyledText::new(line.text).with_highlights(runs)).on_click(line.link.into_iter().collect(), move |_, _, cx| {
            this.update(cx, |this, cx| {
                if let Some(message) = this.feed_message(&channel_id, &message_id) {
                    this.open_reactions(&message, emoji.clone(), cx);
                }
            })
            .ok();
        });
        // The top padding is the gap to the pill, inside the card's bounds so the pointer can cross it.
        div()
            .pt_2()
            .child(
                ui::surface(cx)
                    .id("reactors-card")
                    .occlude()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .max_w_80()
                    .p_2p5()
                    .pr_3p5()
                    .child(div().flex_none().child(emoji_face(&self.reaction.emoji, 36.)))
                    .child(div().min_w_0().text_size(ui::TEXT_SM).line_height(ui::LINE_SM).text_color(palette::muted(cx)).child(text)),
            )
            .into_any_element()
    }
}

// The card's sentence, and which parts of it are drawn strong or as the dialog's link.
#[derive(Debug, PartialEq)]
pub(crate) struct Line {
    pub text: String,
    pub strong: Vec<Range<usize>>,
    pub link: Option<Range<usize>>,
}

// ":fire: reacted by Mira, Jun, Theo, and 589 others": three names at most, the rest counted from the
// reaction and linked to the dialog; three or fewer, "Mira, Jun and Theo". With no names (loading, or the
// fetch failed) the count alone: "3 people reacted with :fire:".
pub(crate) fn reactors_line(names: &[String], count: u32, code: &str) -> Line {
    let mut line = Line { text: String::new(), strong: Vec::new(), link: None };
    let push = |line: &mut Line, text: &str, strong: bool| {
        let start = line.text.len();
        line.text.push_str(text);
        if strong {
            line.strong.push(start..line.text.len());
        }
    };
    let shown = &names[..names.len().min(3)];
    if shown.is_empty() {
        push(&mut line, &if count == 1 { "1 person".to_owned() } else { format!("{} people", grouped(count.into())) }, false);
        push(&mut line, " reacted with ", false);
        push(&mut line, code, true);
        return line;
    }
    let others = (count as usize).max(names.len()) - shown.len();
    push(&mut line, code, true);
    push(&mut line, " reacted by ", false);
    for (i, name) in shown.iter().enumerate() {
        if i > 0 {
            push(&mut line, if i + 1 == shown.len() && others == 0 { " and " } else { ", " }, false);
        }
        push(&mut line, name, true);
    }
    if others > 0 {
        push(&mut line, ", and ", false);
        let start = line.text.len();
        push(&mut line, &format!("{} {}", grouped(others as u64), if others == 1 { "other" } else { "others" }), false);
        line.link = Some(start..line.text.len());
    }
    line
}

// The dialog's rail: most used first, ties in the message's order.
pub(crate) fn rail_order(reactions: &[Reaction]) -> Vec<&Reaction> {
    let mut order: Vec<&Reaction> = reactions.iter().collect();
    order.sort_by_key(|r| std::cmp::Reverse(r.count));
    order
}

// You first, where your Remove is; everyone else in the order they reacted.
fn people_order<'a>(users: &'a [Reactor], me: &str) -> Vec<&'a Reactor> {
    let (mine, rest): (Vec<&Reactor>, Vec<&Reactor>) = users.iter().partition(|u| u.user.id == me);
    mine.into_iter().chain(rest).collect()
}

impl Waffle {
    /// The first page of who reacted, once per reaction until it changes.
    pub(crate) fn load_reactors(&mut self, channel_id: &str, message_id: &str, emoji: &Emoji, cx: &mut Context<Self>) {
        if self.reactors.contains_key(&(message_id.to_owned(), emoji.path())) {
            return;
        }
        self.fetch_reactors(channel_id, message_id, emoji, None, cx);
    }

    fn load_more_reactors(&mut self, channel_id: &str, message_id: &str, emoji: &Emoji, cx: &mut Context<Self>) {
        let Some(after) = self.reactors.get(&(message_id.to_owned(), emoji.path())).and_then(|r| r.next_after().map(str::to_owned)) else { return };
        self.fetch_reactors(channel_id, message_id, emoji, Some(after), cx);
    }

    fn fetch_reactors(&mut self, channel_id: &str, message_id: &str, emoji: &Emoji, after: Option<String>, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let key = (message_id.to_owned(), emoji.path());
        let entry = self.reactors.entry(key.clone()).or_default();
        entry.loading = true;
        entry.failed = false;
        let (channel_id, message_id, emoji) = (channel_id.to_owned(), message_id.to_owned(), emoji.clone());
        cx.spawn(async move |this, cx| {
            let result = client.reactors(&channel_id, &message_id, &emoji, after.as_deref(), PAGE).await;
            this.update(cx, |this, cx| {
                // Dropped or moved on while in flight: the reaction changed, so this page is stale.
                let Some(entry) = this.reactors.get_mut(&key) else { return };
                if !entry.loading || entry.users.last().map(|u| u.user.id.as_str()) != after.as_deref() {
                    return;
                }
                entry.loading = false;
                match result {
                    Ok(page) => {
                        entry.done = page.len() < PAGE;
                        entry.users.extend(page);
                    }
                    Err(_) => entry.failed = true,
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// A reaction changed: someone leaving drops out of the loaded list; someone joining isn't placed
    /// (the event carries no user object), so that list loads again.
    pub(crate) fn reactors_changed(&mut self, message_id: &str, emoji: &Emoji, user_id: &str, added: bool) {
        let key = (message_id.to_owned(), emoji.path());
        if added {
            self.reactors.remove(&key);
        } else if let Some(entry) = self.reactors.get_mut(&key) {
            entry.users.retain(|u| u.user.id != user_id);
        }
    }

    pub(crate) fn feed_message(&self, channel_id: &str, message_id: &str) -> Option<Message> {
        self.feeds.get(channel_id)?.messages.iter().find(|m| m.id == message_id).cloned()
    }

    pub(crate) fn open_reactions(&mut self, message: &Message, emoji: Emoji, cx: &mut Context<Self>) {
        self.load_reactors(&message.channel_id, &message.id, &emoji, cx);
        self.reactions_dialog = Some(ReactionsDialog {
            channel_id: message.channel_id.clone(),
            message_id: message.id.clone(),
            message: message.clone(),
            selected: emoji,
            hovered: None,
            scroll: ScrollHandle::new(),
        });
        cx.notify();
    }

    fn select_reaction(&mut self, emoji: Emoji, cx: &mut Context<Self>) {
        let Some(dialog) = self.reactions_dialog.as_mut() else { return };
        dialog.selected = emoji.clone();
        dialog.scroll = ScrollHandle::new();
        let (channel_id, message_id) = (dialog.channel_id.clone(), dialog.message_id.clone());
        self.load_reactors(&channel_id, &message_id, &emoji, cx);
        cx.notify();
    }

    fn close_reactions(&mut self, cx: &mut Context<Self>) {
        self.reactions_dialog = None;
        cx.notify();
    }

    fn reactions_scrolled(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = &self.reactions_dialog else { return };
        let (scrolled, max) = (-f32::from(dialog.scroll.offset().y), f32::from(dialog.scroll.max_offset().y));
        if max - scrolled > PREFETCH {
            return;
        }
        let (channel_id, message_id, emoji) = (dialog.channel_id.clone(), dialog.message_id.clone(), dialog.selected.clone());
        self.load_more_reactors(&channel_id, &message_id, &emoji, cx);
    }

    fn retry_reactors(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = &self.reactions_dialog else { return };
        let (channel_id, message_id, emoji) = (dialog.channel_id.clone(), dialog.message_id.clone(), dialog.selected.clone());
        let after = self.reactors.get(&(message_id.clone(), emoji.path())).and_then(|r| r.users.last()).map(|u| u.user.id.clone());
        self.fetch_reactors(&channel_id, &message_id, &emoji, after, cx);
    }

    // Yours goes through the pill's own toggle. Someone else's (Manage Messages) leaves the list at once and
    // comes back if Discord refuses; the count follows the gateway's MESSAGE_REACTION_REMOVE, as for anyone's.
    fn remove_reactor(&mut self, user_id: String, cx: &mut Context<Self>) {
        let Some(dialog) = &self.reactions_dialog else { return };
        let (channel_id, message_id, emoji) = (dialog.channel_id.clone(), dialog.message_id.clone(), dialog.selected.clone());
        if user_id == self.my_id() {
            if let Some(message) = self.feed_message(&channel_id, &message_id) {
                self.toggle_reaction(&message, emoji, cx);
            }
            return;
        }
        let key = (message_id.clone(), emoji.path());
        let Some((at, user)) = self.reactors.get(&key).and_then(|r| r.users.iter().position(|u| u.user.id == user_id).map(|at| (at, r.users[at].clone()))) else { return };
        let path = format!("/channels/{channel_id}/messages/{message_id}/reactions/{}/{user_id}", emoji.path());
        let undo_key = key.clone();
        self.rest_optimistic(
            reqwest::Method::DELETE,
            path,
            None,
            move |this| {
                if let Some(entry) = this.reactors.get_mut(&key) {
                    entry.users.retain(|u| u.user.id != user_id);
                }
            },
            move |this| {
                if let Some(entry) = this.reactors.get_mut(&undo_key) {
                    entry.users.insert(at.min(entry.users.len()), user);
                }
            },
            cx,
        );
    }

    fn hover_reactor(&mut self, user_id: &str, hovered: bool, cx: &mut Context<Self>) {
        let Some(dialog) = self.reactions_dialog.as_mut() else { return };
        if hovered {
            dialog.hovered = Some(user_id.to_owned());
        } else if dialog.hovered.as_deref() == Some(user_id) {
            dialog.hovered = None;
        }
        cx.notify();
    }

    // Header, then the reactions down the left by count and the selected one's people on the right.
    pub(crate) fn reactions_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.reactions_dialog.as_ref()?;
        let message = self.feed_message(&dialog.channel_id, &dialog.message_id).unwrap_or_else(|| dialog.message.clone());
        let order = rail_order(&message.reactions);
        let Some(selected) = order.iter().find(|r| r.emoji == dialog.selected).or(order.first()).map(|r| r.emoji.clone()) else {
            self.reactions_dialog = None;
            return None;
        };
        let line = palette::line(cx);
        let total: u64 = message.reactions.iter().map(|r| u64::from(r.count)).sum();
        let head = div()
            .flex()
            .items_center()
            .gap_3()
            .pl_6()
            .pr_5()
            .pt_5()
            .pb_3p5()
            .border_b_1()
            .border_color(line)
            .child(
                // GAP: gpui's `items_baseline` doesn't line up mixed sizes, so the count's taller line box puts its
                // baseline on the title's: half-leading plus (ascent - descent) / 2 of each size, top-aligned.
                div()
                    .flex_1()
                    .flex()
                    .items_start()
                    .gap_2()
                    .child(div().text_size(ui::TEXT_LG).line_height(ui::LINE_LG).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child("Reactions"))
                    .child(div().text_size(ui::TEXT_SM).line_height(ui::LINE_XL).text_color(palette::muted(cx)).font_features(tabular()).child(grouped(total))),
            )
            .child(ui::icon_button("reactions-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.close_reactions(cx))));
        let tabs: Vec<AnyElement> = order.iter().map(|r| reaction_tab(r, r.emoji == selected, cx)).collect();
        let rail = div()
            .id("reactions-rail")
            .w(px(RAIL_WIDTH))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .border_r_1()
            .border_color(line)
            .py_2p5()
            .px_2()
            .flex()
            .flex_col()
            .gap_0p5()
            .children(tabs);
        let people = div()
            .id("reactions-people")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .track_scroll(&dialog.scroll)
            .on_scroll(cx.listener(|this, _, _, cx| this.reactions_scrolled(cx)))
            .p_2()
            .flex()
            .flex_col()
            .children(self.reactor_rows(&message, &selected, cx));
        let body = div().h(px(DIALOG_HEIGHT)).flex().flex_col().child(head).child(div().flex_1().min_h_0().flex().child(rail).child(people));
        let (channel_id, message_id) = (message.channel_id.clone(), message.id.clone());
        self.load_reactors(&channel_id, &message_id, &selected, cx);
        Some(ui::modal(backdrop("reactions-backdrop", cx), ui::dialog("reactions-dialog", DIALOG_WIDTH, body, window, cx), cx))
    }

    // The people who used `emoji`, then where the list stands: loading, more to come, or failed.
    fn reactor_rows(&self, message: &Message, emoji: &Emoji, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let entry = self.reactors.get(&(message.id.clone(), emoji.path()));
        let users = entry.map(|r| r.users.as_slice()).unwrap_or_default();
        let manage = self.can_in(&message.channel_id, perm::MANAGE_MESSAGES);
        let hovered = self.reactions_dialog.as_ref().and_then(|d| d.hovered.as_deref());
        let me = self.my_id();
        let mut rows: Vec<AnyElement> = people_order(users, me)
            .into_iter()
            .map(|reactor| {
                let user = &reactor.user;
                let mine = user.id == me;
                let removable = mine || (manage && hovered == Some(user.id.as_str()));
                let detail = if mine && !reactor.username.is_empty() { format!("{} · you", reactor.username) } else { reactor.username.clone() };
                self.reactor_row(user, self.member_name(user), detail, removable, cx)
            })
            .collect();
        let failed = entry.is_some_and(|r| r.failed);
        let more = entry.is_none_or(|r| !r.done && !r.failed);
        if failed {
            rows.push(failed_row(cx));
        } else if more {
            let count = if users.is_empty() { 6 } else { 1 };
            rows.extend((0..count).map(|i| skeleton_row(i, cx)));
        }
        rows
    }

    fn reactor_row(&self, user: &crate::discord::User, name: String, detail: String, removable: bool, cx: &mut Context<Self>) -> AnyElement {
        let id = user.id.clone();
        let hover_id = id.clone();
        let who = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(div().truncate().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(name))
            .when(!detail.is_empty(), |w| w.child(div().truncate().text_size(ui::TEXT_DETAIL).text_color(palette::muted(cx)).child(detail)));
        let remove = removable.then(|| {
            ui::button(SharedString::from(format!("reactor-remove-{id}")), ui::Tone::Secondary, cx)
                .small()
                .child("Remove")
                .on_click(cx.listener(move |this, _, _, cx| this.remove_reactor(id.clone(), cx)))
        });
        let hover = palette::hover(cx);
        div()
            .id(SharedString::from(format!("reactor-{}", user.id)))
            .flex()
            .flex_none()
            .items_center()
            .gap_3()
            .min_h(px(52.))
            .px_2p5()
            .py_1p5()
            .squircle(RADIUS_MD)
            .hover(move |s| s.bg(hover))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| this.hover_reactor(&hover_id, *hovered, cx)))
            .child(ui::avatar(user.avatar.clone(), AVATAR))
            .child(who)
            .children(remove)
            .into_any_element()
    }
}

// One reaction in the rail: the emoji, then its count; yours in the accent ink.
fn reaction_tab(reaction: &Reaction, on: bool, cx: &mut Context<Waffle>) -> AnyElement {
    let emoji = reaction.emoji.clone();
    let code = shortcode(&emoji);
    ui::menu_row(SharedString::from(format!("reactions-tab-{}", emoji.path())), cx)
        .toggled(on, cx)
        .h_10()
        .text_size(ui::TEXT_LEAD)
        .font_weight(FontWeight::SEMIBOLD)
        .child(div().w(px(26.)).flex_none().flex().justify_center().child(emoji_face(&emoji, 22.)))
        .child(div().font_features(tabular()).when(reaction.me, |d| d.text_color(palette::accent_ink(cx))).child(grouped(reaction.count.into())))
        .tooltip(move |window, cx| ui::tip(code.clone(), window, cx))
        .on_click(cx.listener(move |this, _, _, cx| this.select_reaction(emoji.clone(), cx)))
        .into_any_element()
}

fn skeleton_row(i: usize, cx: &mut Context<Waffle>) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_3()
        .min_h(px(52.))
        .px_2p5()
        .child(ui::skeleton_circle(("reactor-skeleton-face", i), AVATAR, cx))
        .child(ui::skeleton(("reactor-skeleton-name", i), 6., cx).w(px(140.)).h_3())
        .into_any_element()
}

fn failed_row(cx: &mut Context<Waffle>) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .min_h(px(52.))
        .px_2p5()
        .text_color(palette::muted(cx))
        .child("Couldn't load everyone who reacted.")
        .child(ui::button("reactors-retry", ui::Tone::Link, cx).child("Try again").on_click(cx.listener(|this, _, _, cx| this.retry_reactors(cx))))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    fn user(id: &str) -> Reactor {
        Reactor { user: crate::discord::User { id: id.into(), ..Default::default() }, ..Default::default() }
    }

    #[test]
    fn reactors_line_names_three_then_links_the_rest() {
        let text = |n: &[&str], count| reactors_line(&names(n), count, ":fire:").text;
        assert_eq!(text(&["Mira"], 1), ":fire: reacted by Mira");
        assert_eq!(text(&["Mira", "Jun"], 2), ":fire: reacted by Mira and Jun");
        assert_eq!(text(&["Mira", "Jun", "Theo"], 3), ":fire: reacted by Mira, Jun and Theo");
        assert_eq!(text(&["Mira", "Jun", "Theo", "Ana"], 4), ":fire: reacted by Mira, Jun, Theo, and 1 other");
        assert_eq!(text(&["Mira", "Jun", "Theo"], 592), ":fire: reacted by Mira, Jun, Theo, and 589 others");
        assert_eq!(text(&["Mira", "Jun", "Theo"], 1_235), ":fire: reacted by Mira, Jun, Theo, and 1,232 others");
        assert_eq!(text(&[], 1), "1 person reacted with :fire:");
        assert_eq!(text(&[], 3), "3 people reacted with :fire:");
        let line = reactors_line(&names(&["Mira", "Jun", "Theo"]), 6, ":fire:");
        assert_eq!(line.strong.iter().map(|r| &line.text[r.clone()]).collect::<Vec<_>>(), [":fire:", "Mira", "Jun", "Theo"]);
        assert_eq!(line.link.map(|r| &line.text[r]), Some("3 others"));
        assert_eq!(reactors_line(&names(&["Mira", "Jun"]), 2, ":fire:").link, None);
    }

    #[test]
    fn next_page_starts_after_the_last_user_until_done() {
        let mut r = Reactors { users: vec![user("1"), user("2")], ..Default::default() };
        assert_eq!(r.next_after(), Some("2"));
        r.loading = true;
        assert_eq!(r.next_after(), None, "one page in flight at a time");
        r.loading = false;
        r.failed = true;
        assert_eq!(r.next_after(), None, "a failure waits for Try again");
        assert_eq!(Reactors::loaded(vec![user("1")]).next_after(), None);
        assert_eq!(Reactors::default().next_after(), None, "the first page isn't a next page");
    }

    #[test]
    fn rail_runs_by_count_and_you_lead_the_people() {
        let reaction = |name: &str, count| Reaction { emoji: Emoji::unicode(name), count, me: false };
        let reactions = [reaction("a", 2), reaction("b", 9), reaction("c", 2), reaction("d", 5)];
        assert_eq!(rail_order(&reactions).iter().map(|r| r.emoji.name.as_str()).collect::<Vec<_>>(), ["b", "d", "a", "c"]);
        let users = [user("1"), user("2"), user("me"), user("3")];
        assert_eq!(people_order(&users, "me").iter().map(|u| u.user.id.as_str()).collect::<Vec<_>>(), ["me", "1", "2", "3"]);
    }
}
