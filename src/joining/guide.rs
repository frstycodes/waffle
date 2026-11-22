// The Server Guide: welcome, Get Started to-dos and Resources, plus the to-do bar in a to-do's channel
// (design/explore-joining.html §5).
use std::collections::HashSet;

use gpui_kit::{
    AnyElement, Context, FontWeight, IntoElement, ScrollHandle, SharedString, Window, WindowControlArea,
    component::{ActiveTheme, Icon},
    div, img, linear_color_stop, linear_gradient, prelude::*, px,
};

use super::{ActionKind, GuildPage, MemberAction, OptionEmoji, Welcome, api, features};
use crate::{
    Waffle,
    channel_settings::Load,
    discord::{Channel, ChannelKind, Guild, Message},
    icons::Glyph,
    mascot::Mood,
    palette::{self, Level},
    ui::{self, RADIUS_LG, RADIUS_MD, RADIUS_SM, Squircle, Tone},
};

const BANNER_HEIGHT: f32 = 140.;

#[derive(Default)]
pub struct GuideView {
    scroll: ScrollHandle,
}

impl Waffle {
    pub(crate) fn render_guide(&mut self, guild: &Guild, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if !self.join.welcome.contains_key(&guild.id) {
            self.fetch_welcome(&guild.id, cx);
        }
        let body = match self.join.welcome.get(&guild.id) {
            Some(Load::Ready(welcome)) => self.guide_body(guild, welcome, cx).into_any_element(),
            Some(Load::Failed(error)) => guide_failed(guild.id.clone(), error.clone(), cx).into_any_element(),
            _ => guide_skeleton(cx).into_any_element(),
        };
        let scroll = self.join.guide.scroll.clone();
        let header = div()
            .occlude()
            .h(px(crate::layout::HEADER_HEIGHT))
            .pl_4()
            .pr_3()
            .flex()
            .items_center()
            .gap_2()
            .window_control_area(WindowControlArea::Drag)
            .text_size(ui::TEXT_MD)
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette::strong(cx))
            .child(Icon::new(Glyph::Signature).size(ui::ICON_XL).text_color(palette::faint(cx)))
            .child(div().min_w_0().truncate().child(format!("{} Server Guide", guild.name)))
            .child(div().ml_auto().child(self.caption_clearance(true)));
        let column = div().w_full().max_w(px(720.)).mx_auto().flex().flex_col().gap_6().child(banner(guild)).child(body);
        div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(palette::surface(Level::Page, cx))
            .text_color(palette::text(cx))
            .text_size(ui::TEXT_SM)
            .child(ui::hard_edge(move || ui::scrolled(&scroll), cx).child(header))
            .child(ui::scroll_area(div().id("guide-page").pt_2().px_6().pb_8().child(column), &self.join.guide.scroll))
            .into_any_element()
    }

    fn guide_body(&self, guild: &Guild, welcome: &Welcome, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let todos = todos(guild, welcome);
        let done = self.join.done_actions.get(&guild.id).cloned().unwrap_or_default();
        let new_member = Self::is_new_member(guild) && !todos.is_empty();
        let all_set = !new_member && welcome.resources.is_empty();
        let get_started = new_member.then(|| {
            let rows = todos.iter().map(|todo| todo_row(guild, todo, done.contains(&todo.channel_id), cx)).collect::<Vec<_>>();
            section("Get Started", cx).child(div().flex().flex_col().p_1p5().squircle(RADIUS_LG).bg(palette::surface(Level::Group, cx)).children(rows))
        });
        let resources = (!welcome.resources.is_empty()).then(|| {
            let cards = welcome.resources.iter().enumerate().filter_map(|(ix, r)| Some(resource_card(ix, find_channel(guild, &r.channel_id)?, &r.title, r.description.clone(), cx))).collect::<Vec<_>>();
            section("Resources", cx).child(div().grid().grid_cols(2).gap_2().children(cards))
        });
        let explore = all_set.then(|| self.all_set(guild, cx));
        div().flex().flex_col().gap_6().children(self.welcome_card(guild, welcome, cx)).children(get_started).children(resources).children(explore)
    }

    fn welcome_card(&self, guild: &Guild, welcome: &Welcome, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let message = welcome.message.clone()?;
        let author = welcome.author_ids.first()?;
        let (name, avatar) = self.guide_author(&guild.id, author);
        // Not the chat's markdown: that renderer needs a message row's context. Mentions and formatting show raw.
        Some(
            div()
                .flex()
                .gap_3()
                .p_4()
                .squircle(RADIUS_LG)
                .bg(palette::surface(Level::Group, cx))
                .child(ui::avatar(avatar, 40.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(name))
                        .child(div().text_size(ui::TEXT_LEAD).line_height(ui::LINE_LEAD).text_color(palette::text(cx)).child(message)),
                ),
        )
    }

    // A welcome author's server name and avatar, as the member list or recent messages know them.
    fn guide_author(&self, guild_id: &str, user_id: &str) -> (String, String) {
        let member = self.members.get(guild_id).and_then(|m| m.get(user_id)).cloned();
        match member.or_else(|| self.known_users().into_iter().find(|u| u.id == user_id)) {
            Some(user) => (user.name, user.avatar),
            None => (user_id.to_owned(), crate::discord::avatar_url(user_id, None)),
        }
    }

    fn all_set(&self, guild: &Guild, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let first = crate::sidebar::channel_groups(guild, self.my_id()).into_iter().flat_map(|(_, c)| c).find(|c| c.kind == ChannelKind::Text);
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap_3()
            .py_6()
            .child(div().text_size(ui::TEXT_LG).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child("You're all set!"))
            .children(first.map(|channel| ui::button("guide-explore", Tone::Primary, cx).child("Explore the Server").on_click(cx.listener(move |this, _, window, cx| this.open_channel(channel.clone(), window, cx)))))
    }

    // Over the composer in a to-do's channel: its title and action, then what's next.
    pub(crate) fn guide_action_bar(&mut self, channel: &Channel, cx: &mut Context<Self>) -> Option<AnyElement> {
        let guild = self.guild_of(&channel.id)?;
        if !Self::is_new_member(guild) {
            return None;
        }
        let Some(Load::Ready(welcome)) = self.join.welcome.get(&guild.id) else { return None };
        let todos = todos(guild, welcome);
        let todo = todos.iter().find(|t| t.channel_id == channel.id)?;
        let empty = HashSet::new();
        let done = self.join.done_actions.get(&guild.id).unwrap_or(&empty);
        let (strong, ink) = (palette::strong(cx), palette::accent_ink(cx));
        let bar = div()
            .id("guide-action-bar")
            .mx_4()
            .min_h_11()
            .pl_3p5()
            .pr_1p5()
            .py_1p5()
            .flex()
            .items_center()
            .gap_2p5()
            .squircle(RADIUS_SM)
            .bg(ui::mix(palette::surface(Level::Page, cx), palette::accent(), 0.16))
            .text_size(ui::TEXT_SM)
            .text_color(strong);
        if !done.contains(&todo.channel_id) {
            let send = (todo.kind == ActionKind::Chat).then(|| {
                ui::button("guide-send", Tone::Primary, cx).small().child("Send a message").on_click(cx.listener(|this, _, window, cx| this.composer.update(cx, |state, cx| state.focus(window, cx))))
            });
            return Some(
                bar.child(Icon::new(Glyph::ChatRound).size(ui::ICON_LG).text_color(ink))
                    .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).child(todo.title.clone()))
                    .children(send)
                    .into_any_element(),
            );
        }
        let next = next_todo(&todos, done)?;
        let target = find_channel(guild, &next.channel_id)?.clone();
        Some(
            bar.child(Icon::new(Glyph::CheckCircle).size(ui::ICON_LG).text_color(ink))
                .child(div().flex_1().min_w_0().flex().gap_1().child("Next:").child(div().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).child(next.title.clone())))
                .child(ui::button("guide-go", Tone::Secondary, cx).small().child("Go").on_click(cx.listener(move |this, _, window, cx| this.open_channel(target.clone(), window, cx))))
                .into_any_element(),
        )
    }

    // A VIEW to-do completes when its channel opens. Opening any channel of a guide server also fetches its
    // welcome, so the Get Started row has its progress.
    pub(crate) fn note_channel_viewed(&mut self, channel: &Channel, cx: &mut Context<Self>) {
        let Some(guild) = self.guild_of(&channel.id) else { return };
        if !Self::is_new_member(guild) {
            return;
        }
        let (guild_id, guide) = (guild.id.clone(), has_guide(guild));
        if guide {
            self.fetch_welcome(&guild_id, cx);
        }
        if self.todo_kind(&guild_id, &channel.id) == Some(ActionKind::View) {
            self.complete_todo(&guild_id, &channel.id, cx);
        }
    }

    // A CHAT to-do completes on your own message in its channel, or in a post of its forum.
    pub(crate) fn note_message_sent(&mut self, message: &Message, cx: &mut Context<Self>) {
        let Some(guild_id) = &message.guild_id else { return };
        if message.author.id != self.my_id() {
            return;
        }
        let Some(guild) = self.guilds.iter().find(|g| &g.id == guild_id) else { return };
        if !Self::is_new_member(guild) {
            return;
        }
        let forum = guild
            .threads
            .iter()
            .find(|t| t.id == message.channel_id)
            .and_then(|t| t.parent_id.as_ref())
            .and_then(|parent| guild.channels.iter().find(|c| &c.id == parent && c.kind == ChannelKind::Forum));
        let channel_id = forum.map_or_else(|| message.channel_id.clone(), |f| f.id.clone());
        let guild_id = guild_id.clone();
        if self.todo_kind(&guild_id, &channel_id) == Some(ActionKind::Chat) {
            self.complete_todo(&guild_id, &channel_id, cx);
        }
    }

    // (done, total) to-dos while you're a new member, for the Get Started row.
    pub(crate) fn guide_progress(&self, guild: &Guild) -> Option<(usize, usize)> {
        if !has_guide(guild) || !Self::is_new_member(guild) {
            return None;
        }
        let Some(Load::Ready(welcome)) = self.join.welcome.get(&guild.id) else { return None };
        let todos = todos(guild, welcome);
        if todos.is_empty() {
            return None;
        }
        let empty = HashSet::new();
        Some(progress(&todos, self.join.done_actions.get(&guild.id).unwrap_or(&empty)))
    }

    fn guild_of(&self, channel_id: &str) -> Option<&Guild> {
        self.guilds.iter().find(|g| g.channels.iter().any(|c| c.id == channel_id))
    }

    fn todo_kind(&self, guild_id: &str, channel_id: &str) -> Option<ActionKind> {
        let Some(Load::Ready(welcome)) = self.join.welcome.get(guild_id) else { return None };
        welcome.actions.iter().find(|a| a.channel_id == channel_id).map(|a| a.kind)
    }

    // Marked here first and kept if the call fails, as Discord's client does.
    fn complete_todo(&mut self, guild_id: &str, channel_id: &str, cx: &mut Context<Self>) {
        if !self.join.done_actions.entry(guild_id.to_owned()).or_default().insert(channel_id.to_owned()) {
            return;
        }
        self.server_call(api::complete_action(guild_id, channel_id), |_, _, _| {}, cx);
        cx.notify();
    }
}

fn has_guide(guild: &Guild) -> bool {
    [features::COMMUNITY, features::SERVER_GUIDE, features::ONBOARDING].iter().all(|f| guild.has_feature(f))
}

fn find_channel<'a>(guild: &'a Guild, id: &str) -> Option<&'a Channel> {
    guild.channels.iter().find(|c| c.id == id)
}

// The to-dos Discord shows: those whose channel we know.
fn todos<'a>(guild: &Guild, welcome: &'a Welcome) -> Vec<&'a MemberAction> {
    welcome.actions.iter().filter(|a| find_channel(guild, &a.channel_id).is_some()).collect()
}

fn progress(todos: &[&MemberAction], done: &HashSet<String>) -> (usize, usize) {
    (todos.iter().filter(|t| done.contains(&t.channel_id)).count(), todos.len())
}

fn next_todo<'a>(todos: &[&'a MemberAction], done: &HashSet<String>) -> Option<&'a MemberAction> {
    todos.iter().copied().find(|t| !done.contains(&t.channel_id))
}

// The guild's splash under a scrim, else a gradient from its name's hue (white reads on both ends).
fn banner(guild: &Guild) -> impl IntoElement + use<> {
    let hue = ui::name_hue(&guild.name);
    let cover = guild.splash.clone().map(|url| ui::media(url, ui::all(RADIUS_LG)).absolute().inset_0().size_full().object_fit(gpui_kit::ObjectFit::Cover));
    // gpui has no text shadow: the scrim alone keeps the title legible over a bright splash.
    let scrim = ui::edge(div().absolute().inset_0(), ui::all(RADIUS_LG)).bg(linear_gradient(180., linear_color_stop(gpui_kit::transparent_black(), 0.3), linear_color_stop(gpui_kit::black().opacity(0.55), 1.)));
    div()
        .relative()
        .h(px(BANNER_HEIGHT))
        .flex_none()
        .squircle(RADIUS_LG)
        .bg(linear_gradient(135., linear_color_stop(palette::oklch(0.53, 0.12, hue), 0.), linear_color_stop(palette::oklch(0.41, 0.13, hue + 35.), 1.)))
        .children(cover)
        .child(scrim)
        .child(
            div()
                .absolute()
                .left_5()
                .right_5()
                .bottom_4()
                .truncate()
                .text_size(px(24.))
                .font_weight(FontWeight::BOLD)
                .text_color(gpui_kit::white())
                .child(format!("Welcome to {}", guild.name)),
        )
}

fn section(title: &'static str, cx: &mut Context<Waffle>) -> gpui_kit::Div {
    div().flex().flex_col().gap_2().child(div().text_size(ui::TEXT_MD).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(title))
}

fn todo_row(guild: &Guild, todo: &MemberAction, done: bool, cx: &mut Context<Waffle>) -> impl IntoElement + use<> {
    let channel = find_channel(guild, &todo.channel_id).cloned();
    let name = channel.as_ref().map(|c| format!("#{}", c.name)).unwrap_or_default();
    let status = if done { "completed" } else { "not completed" };
    let title_ink = if done { palette::muted(cx) } else { palette::strong(cx) };
    let state = div().size(px(22.)).flex_none().rounded_full().flex().items_center().justify_center();
    let state = match done {
        true => state.bg(cx.theme().success).text_color(gpui_kit::white()).child(Icon::new(Glyph::Checkmark).size(ui::ICON_SM)),
        false => state.border(px(1.5)).border_color(palette::line_strong(cx)),
    };
    ui::menu_row(SharedString::from(format!("guide-todo-{}", todo.channel_id)), cx)
        .min_h(px(56.))
        .px_3()
        .py_2()
        .gap_3()
        .squircle(RADIUS_MD)
        .aria_label(format!("{}, {status}", todo.title))
        .child(emoji_tile(todo.emoji.as_ref(), cx))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().truncate().font_weight(FontWeight::SEMIBOLD).text_color(title_ink).child(todo.title.clone()))
                .child(div().truncate().text_size(ui::TEXT_DETAIL).text_color(palette::muted(cx)).child(name)),
        )
        .child(state)
        .when_some(channel, |row, channel| row.on_click(cx.listener(move |this, _, window, cx| this.open_channel(channel.clone(), window, cx))))
}

fn emoji_tile(emoji: Option<&OptionEmoji>, cx: &mut Context<Waffle>) -> impl IntoElement + use<> {
    let tile = div().size_9().flex_none().squircle(RADIUS_SM).bg(palette::surface(Level::Raised, cx)).flex().items_center().justify_center();
    match (emoji.and_then(OptionEmoji::url), emoji.and_then(|e| e.name.clone())) {
        (Some(url), _) => tile.child(img(url).size_5()),
        (None, Some(name)) => tile.text_size(px(18.)).child(name),
        (None, None) => tile.text_color(palette::faint(cx)).child(Icon::new(Glyph::Hashtag).size(ui::ICON_LG)),
    }
}

fn resource_card(ix: usize, channel: &Channel, title: &str, description: Option<String>, cx: &mut Context<Waffle>) -> impl IntoElement + use<> {
    let (group, channel) = (palette::surface(Level::Group, cx), channel.clone());
    let hover = ui::mix(group, palette::strong(cx), 0.04);
    ui::pressable(div().id(("guide-resource", ix)), 0.98)
        .p_3p5()
        .flex()
        .flex_col()
        .gap_1p5()
        .squircle(16.)
        .bg(group)
        .hover(move |s| s.bg(hover))
        .cursor_pointer()
        .child(div().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(title.to_owned()))
        .children(description.map(|d| div().text_size(ui::TEXT_DETAIL).line_height(ui::LINE_DETAIL).text_color(palette::muted(cx)).child(d)))
        .on_click(cx.listener(move |this, _, window, cx| this.open_channel(channel.clone(), window, cx)))
}

fn guide_skeleton(cx: &mut Context<Waffle>) -> impl IntoElement + use<> {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(ui::skeleton("guide-skeleton-welcome", RADIUS_LG, cx).h(px(88.)).w_full())
        .child(ui::skeleton("guide-skeleton-todos", RADIUS_LG, cx).h(px(180.)).w_full())
}

fn guide_failed(guild_id: String, error: String, cx: &mut Context<Waffle>) -> impl IntoElement + use<> {
    ui::empty_state("guide-failed", Mood::Idle, "Couldn't load the Server Guide", error, cx).child(ui::button("guide-retry", Tone::Secondary, cx).child("Try again").on_click(cx.listener(move |this, _, _, cx| {
        this.join.welcome.remove(&guild_id);
        this.fetch_welcome(&guild_id, cx);
        cx.notify();
    })))
}

// guide: the page with one to-do done. guide-done: every to-do done. guide-channel: #general with its CHAT
// to-do bar. guide-next: a done VIEW to-do's channel showing what's next.
pub fn demo(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    if let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") {
        guild.me.pending = false;
    }
    let channel = |waffle: &Waffle, id: &str| waffle.guilds.iter().find(|g| g.id == "1").and_then(|g| find_channel(g, id)).cloned();
    match scene {
        "guide-done" => {
            waffle.join.done_actions.insert("1".into(), ["general", "media", "long"].map(str::to_owned).into());
            waffle.open_guild_page(GuildPage::Guide, cx);
        }
        "guide-channel" => {
            waffle.join.done_actions.insert("1".into(), HashSet::new());
            if let Some(general) = channel(waffle, "general") {
                waffle.open_channel(general, window, cx);
            }
        }
        "guide-next" => {
            waffle.join.done_actions.insert("1".into(), HashSet::new());
            waffle.feeds.insert("media".into(), crate::Feed { loaded: true, ..Default::default() });
            if let Some(media) = channel(waffle, "media") {
                waffle.open_channel(media, window, cx);
            }
        }
        _ => {
            waffle.join.done_actions.insert("1".into(), ["media".to_owned()].into());
            waffle.open_guild_page(GuildPage::Guide, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(channel_id: &str) -> MemberAction {
        MemberAction { channel_id: channel_id.into(), kind: ActionKind::View, title: channel_id.into(), description: None, emoji: None }
    }

    #[test]
    fn progress_counts_done_todos_and_next_is_the_first_undone() {
        let actions = [action("a"), action("b"), action("c")];
        let todos: Vec<&MemberAction> = actions.iter().collect();
        let done: HashSet<String> = ["b".to_owned(), "elsewhere".to_owned()].into();
        assert_eq!(progress(&todos, &done), (1, 3));
        assert_eq!(next_todo(&todos, &done).map(|t| t.channel_id.as_str()), Some("a"));
        let all: HashSet<String> = ["a", "b", "c"].map(str::to_owned).into();
        assert_eq!(progress(&todos, &all), (3, 3));
        assert!(next_todo(&todos, &all).is_none());
    }
}
