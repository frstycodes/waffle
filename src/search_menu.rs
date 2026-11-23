// The header search's popover (design/overlays.html#search): with the field focused and empty it offers
// the filters, each with its syntax, and your history; typing a filter's word opens its values (people,
// channels, kinds of attachment, a calendar for dates). A pick becomes an atomic token in the field whose
// text is the filter's syntax with an id, so Enter parses picked and typed filters alike.
use std::collections::HashMap;

use chrono::{Datelike, Months, NaiveDate};
use gpui_kit::{
    AnyElement, App, Context, Entity, FontWeight, KeyDownEvent, MouseButton, SharedString, Window,
    component::{
        Icon,
        input::{InlineToken, InlineTokenContext, InputContent, InputEvent},
    },
    div, prelude::*, px,
};

use crate::{
    RightPanel, Waffle, SearchState,
    discord::ChannelKind,
    icons::Glyph,
    palette,
    search::{self, Key, Saved},
    ui::{self, RADIUS_XS, Squircle},
};
use crate::part::Column;

// The tallest the search popover grows.
const POPOVER_MAX: f32 = 520.;
// A filter chip's height.
const CHIP_HEIGHT: f32 = 22.;

pub(crate) const WIDTH: f32 = 360.;
const VALUES_SHOWN: usize = 10;
const CALENDAR_DAY: f32 = 36.;

pub(crate) enum Row {
    Filter(Key),
    More,
    Recent(usize),
    Value(Pick),
}

#[derive(Clone)]
pub(crate) struct Pick {
    // The token's text: `from:42`, `has:image`.
    text: String,
    label: String,
    lead: Lead,
}

#[derive(Clone)]
enum Lead {
    Avatar(String),
    Glyph(Glyph),
}

pub(crate) enum Menu {
    Home(Vec<Row>),
    // Filters whose word starts with what's typed, or one filter's values.
    List(SharedString, Vec<Row>),
    Calendar(Key),
    Hidden,
}

impl Menu {
    fn rows(&self) -> &[Row] {
        match self {
            Menu::Home(rows) | Menu::List(_, rows) => rows,
            Menu::Calendar(_) | Menu::Hidden => &[],
        }
    }
}

impl Waffle {
    pub(crate) fn search_menu(&self, cx: &App) -> Menu {
        let state = self.search_input.read(cx);
        let text = state.value();
        let token_end = state.tokens().last().map_or(0, |s| s.range().end);
        if let Some((key, typed, _)) = search::typing(&text, token_end) {
            if key.is_date() {
                return Menu::Calendar(key);
            }
            return Menu::List(format!("{}:", key.word()).into(), self.search_values(key, typed));
        }
        let keys = self.search_keys(&search::keys_matching(&text, token_end));
        if !keys.is_empty() {
            return Menu::List("Filters".into(), keys.into_iter().map(Row::Filter).collect());
        }
        if search::last_word_start(&text) < text.len() {
            return Menu::Hidden;
        }
        let mut rows: Vec<Row> = self.search_keys(&Key::MAIN).into_iter().map(Row::Filter).collect();
        rows.push(Row::More);
        if self.search_more {
            rows.extend(Key::MORE.into_iter().map(Row::Filter));
        }
        if text.trim().is_empty() {
            // A DM can't take a server search that names a channel.
            let history = self.search_history();
            rows.extend((0..history.len()).filter(|&i| self.guild_id.is_some() || !history[i].text.split_whitespace().any(|w| w.starts_with("in:"))).map(Row::Recent));
        }
        Menu::Home(rows)
    }

    fn search_values(&self, key: Key, typed: &str) -> Vec<Row> {
        let typed = typed.to_lowercase();
        let word = key.word();
        let pick = |value: &str, label: &str, lead: Lead| Row::Value(Pick { text: format!("{word}:{value}"), label: label.to_owned(), lead });
        let words = |list: &[&str], glyph: fn(&str) -> Glyph| list.iter().filter(|w| w.starts_with(&typed)).map(|w| pick(w, w, Lead::Glyph(glyph(w)))).collect();
        match key {
            Key::From | Key::Mentions => {
                let mut users: Vec<crate::discord::User> = self.search_people().into_iter().filter(|u| u.name.to_lowercase().contains(&typed)).collect();
                // Names that start with what's typed first, then by name, so the top row is the likely pick.
                users.sort_by_key(|u| (!u.name.to_lowercase().starts_with(&typed), u.name.to_lowercase()));
                users.into_iter().take(VALUES_SHOWN).map(|u| pick(&u.id, &u.name, Lead::Avatar(u.avatar.clone()))).collect()
            }
            Key::In => self
                .guild()
                .into_iter()
                .flat_map(|g| g.channels.iter())
                .filter(|c| matches!(c.kind, ChannelKind::Text | ChannelKind::Forum) && c.name.to_lowercase().contains(&typed))
                .take(VALUES_SHOWN)
                .map(|c| pick(&c.id, &c.name, Lead::Glyph(Glyph::Hashtag)))
                .collect(),
            Key::Has => words(&search::HAS, has_glyph),
            Key::Pinned => words(&["true", "false"], |_| Glyph::Pin),
            Key::AuthorType => words(&search::AUTHOR_TYPES, |w| match w {
                "bot" => Glyph::Id,
                "webhook" => Glyph::Link,
                _ => Glyph::User,
            }),
            Key::Before | Key::After | Key::During => Vec::new(),
        }
    }

    pub(crate) fn search_history(&self) -> &[Saved] {
        self.prefs.search_history.get(self.my_id()).map_or(&[], Vec::as_slice)
    }

    pub(crate) fn on_search_input(&mut self, _: &Entity<SearchState>, event: &InputEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            InputEvent::Change => {
                self.search_pick = None;
                self.chip_search_filters(false, window, cx);
                self.redraw(Column::Main, cx);
            }
            InputEvent::PressEnter { .. } => {
                let menu = self.search_menu(cx);
                match self.search_pick(&menu) {
                    Some(i) => self.accept_search_row(i, window, cx),
                    None if matches!(menu, Menu::Calendar(_)) => self.pick_search_day(self.search_day, window, cx),
                    None => self.run_search(window, cx),
                }
            }
            _ => {}
        }
    }

    pub(crate) fn on_search_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        if key == "escape" {
            window.blur(cx);
            return cx.stop_propagation();
        }
        let menu = self.search_menu(cx);
        if let Menu::Calendar(_) = menu {
            let days = match key {
                "left" => -1,
                "right" => 1,
                "up" => -7,
                "down" => 7,
                _ => return,
            };
            self.search_day = (self.search_day + chrono::Duration::days(days)).min(today());
            self.redraw(Column::Main, cx);
            return cx.stop_propagation();
        }
        let count = menu.rows().len();
        if count == 0 {
            return;
        }
        let step = match key {
            "down" => 1,
            "up" => count - 1,
            _ => return,
        };
        self.search_pick = Some(self.search_pick(&menu).map_or(if step == 1 { 0 } else { count - 1 }, |i| (i + step) % count));
        self.redraw(Column::Main, cx);
        cx.stop_propagation();
    }

    // The keyboard's row: the one moved to, else the first of a list of values or matching filters, so
    // Enter takes the top match. The home list starts with none, so Enter searches what's typed.
    fn search_pick(&self, menu: &Menu) -> Option<usize> {
        self.search_pick.or_else(|| matches!(menu, Menu::List(_, rows) if !rows.is_empty()).then_some(0))
    }

    // Filters written out as text become tokens. While typing, a person or channel waits for the space
    // after it; `submit` (Enter) takes one at the very end too.
    fn chip_search_filters(&mut self, submit: bool, window: &mut Window, cx: &mut Context<Self>) {
        let mut content = self.search_input.read(cx).content();
        if submit {
            content = content.tokens().iter().fold(InputContent::new(format!("{} ", content.text())), |next, t| next.clone().with_token(t.range(), t.token().clone()).unwrap_or(next));
        }
        let users = self.search_people();
        let channels = self.guild().map(|g| g.channels.clone()).unwrap_or_default();
        let resolve = |key: Key, value: &str| match key {
            Key::From | Key::Mentions => users.iter().find(|u| u.id == value || u.name.eq_ignore_ascii_case(value)).map(|u| (u.id.clone(), u.name.clone())),
            Key::In => channels.iter().find(|c| c.id == value || c.name.eq_ignore_ascii_case(value)).map(|c| (c.id.clone(), c.name.clone())),
            _ => None,
        };
        let Some(next) = search::chip(&content, resolve) else { return };
        self.search_input.update(cx, |state, cx| state.set_value(next, window, cx));
    }

    // Filters' Add: types `key:` after what's in the field and focuses it, so its values open.
    pub(crate) fn type_search_filter(&mut self, key: Key, window: &mut Window, cx: &mut Context<Self>) {
        let content = self.search_input.read(cx).content();
        let text = content.text();
        let gap = if text.is_empty() || text.ends_with(' ') { "" } else { " " };
        let next = content.tokens().iter().fold(InputContent::new(format!("{text}{gap}{}:", key.word())), |next, t| next.clone().with_token(t.range(), t.token().clone()).unwrap_or(next));
        self.search_input.update(cx, |state, cx| {
            state.set_value(next, window, cx);
            state.focus(window, cx);
        });
        if let RightPanel::Search(results) = &mut self.right_panel {
            results.close_filters();
        }
        self.search_pick = None;
        cx.notify(); // lint-allow(global-notify): closes the filters card in the side panel and clears the pick in the header's search menu
    }

    fn accept_search_row(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let menu = self.search_menu(cx);
        let Some(row) = menu.rows().get(index) else { return };
        match row {
            Row::Filter(key) => self.replace_search_word(&format!("{}:", key.word()), None, window, cx),
            Row::More => {
                self.search_more = !self.search_more;
                self.redraw(Column::Main, cx);
            }
            Row::Value(pick) => {
                let pick = pick.clone();
                self.replace_search_word(&pick.text, Some(&pick.label), window, cx);
            }
            Row::Recent(i) => {
                let Some(saved) = self.search_history().get(*i).cloned() else { return };
                self.search_input.update(cx, |state, cx| state.set_value(saved.to_content(), window, cx));
                self.run_search(window, cx);
            }
        }
    }

    fn pick_search_day(&mut self, day: NaiveDate, window: &mut Window, cx: &mut Context<Self>) {
        let Menu::Calendar(key) = self.search_menu(cx) else { return };
        self.replace_search_word(&format!("{}:{}", key.word(), search::day_text(day)), Some(&search::day_label(day)), window, cx);
    }

    // Swaps the word being typed for `text`: a token labelled `label` and a space after it, or plain text
    // (a filter's `key:`) that opens that filter's values. The caret ends after it.
    fn replace_search_word(&mut self, text: &str, label: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        let content = self.search_input.read(cx).content();
        let start = search::last_word_start(content.text());
        let kept = content.text()[..start].to_owned();
        let spaced = if label.is_some() { " " } else { "" };
        let next = content.tokens().iter().filter(|s| s.range().end <= start).fold(InputContent::new(format!("{kept}{text}{spaced}")), |next, span| {
            next.clone().with_token(span.range(), span.token().clone()).unwrap_or(next)
        });
        let next = match label {
            Some(label) => {
                let token = InlineToken::new(text.to_owned(), text.to_owned()).with_label(label.to_owned());
                next.clone().with_token(start..start + text.len(), token).unwrap_or(next)
            }
            None => next,
        };
        self.search_input.update(cx, |state, cx| {
            state.set_value(next, window, cx);
            state.focus(window, cx);
        });
        self.search_pick = None;
        self.search_day = today();
        self.redraw(Column::Main, cx);
    }

    pub(crate) fn run_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.chip_search_filters(true, window, cx);
        let content = self.search_input.read(cx).content();
        let query = content.text().trim().to_owned();
        if self.client.is_none() {
            return;
        }
        // A server searches all its channels; a DM searches itself.
        let scope = match (&self.guild_id, &self.channel) {
            (Some(guild), _) => format!("guilds/{guild}"),
            (None, Some(dm)) => format!("channels/{}", dm.id),
            (None, None) => return,
        };
        if query.is_empty() {
            return;
        }
        let users = self.known_users();
        let members = self.guild_id.as_ref().and_then(|g| self.members.get(g)).cloned().unwrap_or_default();
        let channels = self.guild().map(|g| g.channels.clone()).unwrap_or_default();
        let parsed = search::parse(
            &query,
            |name| {
                let name = name.to_lowercase();
                members.values().chain(users.iter()).find(|u| u.name.to_lowercase() == name || u.id == name).map(|u| u.id.clone())
            },
            |name| {
                let name = name.to_lowercase();
                channels.iter().find(|c| c.name.to_lowercase() == name || c.id == name).map(|c| c.id.clone())
            },
        );
        if !parsed.unresolved.is_empty() {
            self.show_toast(format!("Couldn't find {}", parsed.unresolved.join(", ")), cx);
        }
        let account = self.my_id().to_owned();
        self.update_prefs(cx, |p| search::remember(p.search_history.entry(account).or_default(), Saved::from_content(&content)));
        window.blur(cx);
        let sort = match &self.right_panel {
            RightPanel::Search(results) => results.sort(),
            _ => search::Sort::default(),
        };
        self.show_search(crate::search_results::Results::new(scope, &query, parsed, sort), cx);
    }

    fn clear_search_history(&mut self, cx: &mut Context<Self>) {
        let account = self.my_id().to_owned();
        self.update_prefs(cx, |p| {
            p.search_history.remove(&account);
        });
    }

    // Avatars by user id, for tokens: the field's and the history's.
    pub(crate) fn search_avatars(&self) -> HashMap<String, String> {
        self.search_people().into_iter().map(|u| (u.id, u.avatar)).collect()
    }

    // Who `from:` and `mentions:` offer, each once: a server's members, or the people in this DM and you.
    pub(crate) fn search_people(&self) -> Vec<crate::discord::User> {
        let mut seen = std::collections::HashSet::new();
        let people: Vec<crate::discord::User> = match &self.guild_id {
            Some(guild) => self.members.get(guild).into_iter().flat_map(|m| m.values().cloned()).chain(self.known_users()).collect(),
            None => self.channel.iter().flat_map(|c| c.recipients.iter().cloned().chain(c.recipient.clone())).chain(self.me.clone()).collect(),
        };
        people.into_iter().filter(|u| seen.insert(u.id.clone())).collect()
    }

    // The filters this place takes: a DM is one channel, so no `in:`.
    pub(crate) fn search_keys(&self, keys: &[Key]) -> Vec<Key> {
        keys.iter().copied().filter(|k| self.guild_id.is_some() || *k != Key::In).collect()
    }

    // The popover under the field, right-aligned to it. None while the menu has nothing to offer.
    pub(crate) fn search_popover(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.search_menu(cx);
        let avatars = self.search_avatars();
        let pick = self.search_pick(&menu);
        let body: Vec<AnyElement> = match &menu {
            Menu::Hidden => return None,
            Menu::List(_, rows) if rows.is_empty() => vec![group("No matches", cx).into_any_element()],
            Menu::List(heading, rows) => std::iter::once(group(heading.clone(), cx).into_any_element()).chain(rows.iter().enumerate().map(|(i, row)| self.search_row(i, row, pick, &avatars, cx))).collect(),
            Menu::Home(rows) => {
                let mut out = vec![group("Filters", cx).into_any_element()];
                let first_recent = rows.iter().position(|r| matches!(r, Row::Recent(_)));
                for (i, row) in rows.iter().enumerate() {
                    if Some(i) == first_recent {
                        out.push(div().px_2().py_1().child(ui::divider(None, cx)).into_any_element());
                        let clear = ui::icon_button("search-history-clear", Glyph::Trash, "Clear history", cx).small().on_click(cx.listener(|this, _, _, cx| this.clear_search_history(cx)));
                        out.push(group("History", cx).child(ui::row().flex_1().justify_end().child(clear)).into_any_element());
                    }
                    out.push(self.search_row(i, row, pick, &avatars, cx));
                }
                out
            }
            Menu::Calendar(key) => vec![self.calendar(*key, cx)],
        };
        let card = ui::surface(cx)
            .id("search-popover")
            .w(px(WIDTH))
            .max_h(px(POPOVER_MAX))
            .overflow_y_scroll()
            .occlude()
            // Clicks inside keep the field focused; the field's own mouse-down-out closes it.
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                cx.stop_propagation();
                window.prevent_default();
            })
            .children(body);
        Some(card.into_any_element())
    }

    fn search_row(&self, index: usize, row: &Row, pick: Option<usize>, avatars: &HashMap<String, String>, cx: &mut Context<Self>) -> AnyElement {
        let (muted, strong) = (palette::muted(cx), palette::strong(cx));
        let base = ui::menu_row(("search-row", index), cx).highlighted(pick == Some(index)).on_click(cx.listener(move |this, _, window, cx| this.accept_search_row(index, window, cx)));
        let lead = |glyph: Glyph| div().flex_none().text_color(muted).child(Icon::new(glyph).size(ui::ICON_LG));
        match row {
            Row::Filter(key) => base
                .child(lead(key_glyph(*key)))
                .child(
                    ui::col().min_h_0()
                        .flex_1()
                        .child(ui::line(key.title()).text_color(strong))
                        .child(ui::row().gap_1().text_size(ui::TEXT_XS).text_color(muted).child(div().flex_none().font_weight(FontWeight::SEMIBOLD).text_color(strong).child(format!("{}:", key.word()))).child(ui::line(key.takes()))),
                )
                .into_any_element(),
            Row::More => base
                .child(lead(Glyph::Filter))
                .child(ui::col().min_h_0().flex_1().child(div().text_color(strong).child("More filters")).child(ui::line("dates, author type, and more").text_size(ui::TEXT_XS).text_color(muted)))
                .child(div().text_color(palette::faint(cx)).child(Icon::new(if self.search_more { Glyph::ChevronUp } else { Glyph::ChevronDown }).size(ui::ICON_MD)))
                .into_any_element(),
            Row::Recent(i) => {
                let Some(saved) = self.search_history().get(*i) else { return div().into_any_element() };
                let parts = saved.parts().into_iter().map(|(key, text)| match key {
                    Some(key) => {
                        let avatar = matches!(key, "from" | "mentions").then(|| saved_id(saved, text).and_then(|id| avatars.get(&id).cloned())).flatten();
                        token_chip(key, text, avatar, false, cx).into_any_element()
                    }
                    None => ui::line(text.to_owned()).into_any_element(),
                });
                base.child(lead(Glyph::Search)).child(ui::row().flex_1().overflow_hidden().gap_1().children(parts)).into_any_element()
            }
            Row::Value(pick) => {
                let lead_el = match &pick.lead {
                    Lead::Avatar(url) => ui::avatar(url.clone(), 24.).flex_none().into_any_element(),
                    Lead::Glyph(glyph) => lead(*glyph).into_any_element(),
                };
                base.child(lead_el).child(ui::line(pick.label.clone()).flex_1().text_color(strong)).into_any_element()
            }
        }
    }

    // A month of days for `before:` `after:` `during:`. The keyboard's day carries the selected fill; days
    // after today can't be picked.
    fn calendar(&self, key: Key, cx: &mut Context<Self>) -> AnyElement {
        let (now, cursor) = (today(), self.search_day);
        let first = cursor.with_day(1).expect("first of month");
        let blanks = first.weekday().num_days_from_monday() as usize;
        let length = (first + Months::new(1) - first).num_days() as u32;
        let (selected, hover, accent_ink, faint, text) = (palette::selected(cx), palette::hover(cx), palette::accent_ink(cx), palette::faint(cx), palette::text(cx));
        let cell = || ui::row().w(px(CALENDAR_DAY)).h_8().justify_center();
        let weekdays = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].map(|d| cell().text_size(ui::TEXT_XS).font_weight(FontWeight::SEMIBOLD).text_color(faint).child(d));
        let days = (1..=length).map(|n| {
            let day = first.with_day(n).expect("day in month");
            let future = day > now;
            cell()
                .id(("search-day", n))
                .squircle(RADIUS_XS)
                .text_color(if future { faint.opacity(0.5) } else if day == now { accent_ink } else { text })
                .when(day == now, |c| c.font_weight(FontWeight::SEMIBOLD))
                .when(day == cursor, |c| c.bg(selected))
                .when(!future && day != cursor, |c| c.cursor_pointer().hover(move |s| s.bg(hover)))
                .when(!future, |c| c.on_click(cx.listener(move |this, _, window, cx| this.pick_search_day(day, window, cx))))
                .child(n.to_string())
        });
        let step = |id: &'static str, glyph, tip, months: i32| {
            ui::icon_button(id, glyph, tip, cx).small().on_click(cx.listener(move |this, _, _, cx| {
                let moved = if months < 0 { this.search_day.checked_sub_months(Months::new(1)) } else { this.search_day.checked_add_months(Months::new(1)) };
                this.search_day = moved.unwrap_or(this.search_day).min(today());
                this.redraw(Column::Main, cx);
            }))
        };
        ui::col()
            .child(
                group(format!("{}:", key.word()), cx)
                    .child(ui::row().flex_1().justify_end().font_weight(FontWeight::NORMAL).text_color(palette::muted(cx)).child(first.format("%B %Y").to_string()))
                    .child(step("search-month-back", Glyph::ChevronLeft, "Previous month", -1))
                    .child(step("search-month-next", Glyph::ChevronRight, "Next month", 1)),
            )
            .child(ui::row().justify_center().child(ui::row().items_stretch().w(px(CALENDAR_DAY * 7. + 12.)).pb_1().flex_wrap().gap_0p5().children(weekdays).children((0..blanks).map(|_| cell())).children(days)))
            .into_any_element()
    }
}

// A filter as a chip, in the field and in history: its word, then the avatar and name it stands for.
pub(crate) fn token_chip(key: &str, label: &str, avatar: Option<String>, in_field: bool, cx: &App) -> gpui_kit::Div {
    let (fill, word) = if in_field { (palette::accent_soft(cx), palette::accent_ink(cx)) } else { (palette::selected(cx), palette::muted(cx)) };
    ui::pill(cx)
        .h(px(CHIP_HEIGHT))
        .px_1p5()
        .gap_1()
        .font_weight(FontWeight::NORMAL)
        .bg(fill)
        .text_size(ui::TEXT_SM)
        .text_color(palette::strong(cx))
        .child(div().font_weight(FontWeight::SEMIBOLD).text_color(word).child(format!("{key}:")))
        .children(avatar.map(|url| ui::avatar(url, 16.)))
        .child(div().whitespace_nowrap().child(shown(key, label)))
}

// The field's tokens: `token_chip`, with the avatar looked up by the id in the token's text.
pub(crate) fn render_token(context: &InlineTokenContext, avatars: &HashMap<String, String>, cx: &App) -> gpui_kit::Div {
    let token = context.token();
    let (key, value) = token.text().split_once(':').unwrap_or(("", token.text()));
    let avatar = matches!(key, "from" | "mentions").then(|| avatars.get(value).cloned()).flatten();
    let chip = token_chip(key, token.label(), avatar, true, cx);
    if context.is_selected() { chip.bg(palette::accent().opacity(0.3)) } else { chip }
}

fn shown(key: &str, label: &str) -> String {
    if key == "in" { format!("#{label}") } else { label.to_owned() }
}

// The id behind a saved token's label: the token text's value.
fn saved_id(saved: &Saved, label: &str) -> Option<String> {
    let t = saved.tokens.iter().find(|t| t.label == label)?;
    saved.text.get(t.start..t.end)?.split_once(':').map(|(_, id)| id.to_owned())
}

fn group(label: impl Into<SharedString>, cx: &App) -> gpui_kit::Div {
    ui::section_label(label, ui::Heading::Muted, cx).px_2p5().pt_2().pb_1()
}

pub(crate) fn key_glyph(key: Key) -> Glyph {
    match key {
        Key::From => Glyph::User,
        Key::In => Glyph::Hashtag,
        Key::Has => Glyph::Link,
        Key::Mentions => Glyph::At,
        Key::Before | Key::After | Key::During => Glyph::Calendar,
        Key::Pinned => Glyph::Pin,
        Key::AuthorType => Glyph::Id,
    }
}

fn has_glyph(kind: &str) -> Glyph {
    match kind {
        "link" => Glyph::Link,
        "embed" => Glyph::AppWindow,
        "image" => Glyph::Image,
        "video" => Glyph::Video,
        "sound" => Glyph::VolumeHigh,
        "sticker" => Glyph::Emoji,
        "poll" => Glyph::Checks,
        "forward" => Glyph::ArrowRight,
        _ => Glyph::File,
    }
}

pub(crate) fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}
