// Search results in the side panel (design/overlays.html#search): the count with Filters and Sort, hits
// grouped by channel and drawn as the feed draws them, and the pages. Filters opens every filter at once:
// fixed values toggle as chips, people, channels and dates show their tokens and add through the field.
use gpui_kit::{AnyElement, App, Context, Div, FontWeight, MouseButton, SharedString, Window, component::Icon, div, prelude::*, px};

use crate::{
    RightPanel, Waffle,
    discord::{Message, SearchPage},
    icons::Glyph,
    menu, palette,
    search::{self, Key, Sort},
    search_menu::token_chip,
    ui::{self, RADIUS_MD, Squircle},
};

// Wider than the member list: results carry pictures.
pub(crate) const PANEL_WIDTH: f32 = 420.;
// The hits' content column: the panel less its padding, the card's, and the avatar beside it.
pub(crate) const HIT_COLUMN: f32 = PANEL_WIDTH - 16. - 24. - 48.;

pub(crate) struct Results {
    scope: String,
    content: String,
    params: Vec<(String, String)>,
    filters: usize,
    page: usize,
    sort: Sort,
    filters_open: bool,
    hits: Option<Result<SearchPage, SharedString>>,
}

impl Results {
    pub(crate) fn new(scope: String, query: &str, parsed: search::Query, sort: Sort) -> Self {
        let filters = query.split_whitespace().filter(|w| w.split_once(':').is_some_and(|(k, v)| !v.is_empty() && Key::from_word(k).is_some())).count();
        Self { scope, content: parsed.content, params: parsed.params, filters, page: 1, sort, filters_open: false, hits: None }
    }

    // Demo scenes: a page already back.
    #[cfg(debug_assertions)]
    pub(crate) fn demo(messages: Vec<Message>, total: u32, filters: usize, filters_open: bool) -> Self {
        Self { scope: String::new(), content: String::new(), params: Vec::new(), filters, page: 1, sort: Sort::Newest, filters_open, hits: Some(Ok(SearchPage { messages, total })) }
    }

    // Demo results have no scope and stay wherever they're shown.
    pub(crate) fn searched(&self, scope: &str) -> bool {
        self.scope.is_empty() || self.scope == scope
    }

    pub(crate) fn close_filters(&mut self) {
        self.filters_open = false;
    }

    pub(crate) fn sort(&self) -> Sort {
        self.sort
    }
}

impl Waffle {
    fn results(&mut self) -> Option<&mut Results> {
        match &mut self.right_panel {
            RightPanel::Search(results) => Some(results),
            _ => None,
        }
    }

    pub(crate) fn show_search(&mut self, results: Results, cx: &mut Context<Self>) {
        self.right_panel = RightPanel::Search(Box::new(results));
        self.fetch_search(cx);
    }

    fn fetch_search(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let Some(results) = self.results() else { return };
        results.hits = None;
        let (page, sort) = (results.page, results.sort);
        let mut params = results.params.clone();
        params.extend(sort.params());
        if page > 1 {
            params.push(("offset".into(), search::offset(page).to_string()));
        }
        let (scope, content) = (results.scope.clone(), results.content.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = client.search(&scope, &content, &params).await;
            this.update(cx, |this, cx| {
                // Only the page and order still asked for: a later click wins over a slow earlier one.
                if let Some(results) = this.results().filter(|r| r.page == page && r.sort == sort && r.scope == scope) {
                    results.hits = Some(result.map_err(Into::into));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn set_search_page(&mut self, page: usize, cx: &mut Context<Self>) {
        let Some(results) = self.results() else { return };
        results.page = page;
        self.fetch_search(cx);
    }

    fn set_search_sort(&mut self, sort: Sort, cx: &mut Context<Self>) {
        let Some(results) = self.results() else { return };
        (results.sort, results.page) = (sort, 1);
        self.fetch_search(cx);
    }

    // The panel's header row: the count, Filters with how many are on, Sort, and the way back to members.
    pub(crate) fn search_head(&self, results: &Results, close: impl IntoElement, cx: &mut Context<Self>) -> Div {
        let title = match &results.hits {
            None => "Searching…".to_owned(),
            Some(Err(_)) => "Search failed".to_owned(),
            Some(Ok(page)) if page.total == 1 => "1 Result".to_owned(),
            Some(Ok(page)) => format!("{} Results", page.total),
        };
        let filters_label = if results.filters == 0 { "Filters".to_owned() } else { format!("Filters ({})", results.filters) };
        let filters = ui::button("search-filters", ui::Tone::Secondary, cx)
            .small()
            .when(results.filters_open, |b| b.bg(palette::selected(cx)))
            .child(Icon::new(Glyph::Filter).size(ui::ICON_SM))
            .child(filters_label)
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(results) = this.results() {
                    results.filters_open = !results.filters_open;
                }
                cx.notify();
            }));
        let sort = ui::button("search-sort", ui::Tone::Secondary, cx).small().child(Icon::new(Glyph::ArrowDown).size(ui::ICON_SM)).child(results.sort.label()).on_click(cx.listener(|this, event: &gpui_kit::ClickEvent, _, cx| {
            let current = this.results().map(|r| r.sort).unwrap_or_default();
            let entries = Sort::ALL.into_iter().map(|sort| menu::Entry::Item(menu::item(sort.label(), move |this, _, cx| this.set_search_sort(sort, cx)).radio(sort == current))).collect();
            this.open_menu(event.position(), entries, cx);
        }));
        let popover = results.filters_open.then(|| {
            // Under the header, inside the panel's right edge.
            let card = div().absolute().top(px(crate::layout::HEADER_HEIGHT - 6.)).right(px(-(crate::window_controls::WIDTH))).child(self.filters_popover(cx));
            // lint-allow(raw-floating): hangs at absolute offsets under the header, not anchored; the card occludes itself.
            gpui_kit::deferred(ui::rise(card.transform_origin(ui::corner(true, false)), "search-filters-in", std::time::Duration::ZERO)).with_priority(2)
        });
        div()
            .relative()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap_1p5()
            .children(popover)
            .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(title))
            .child(filters)
            .child(sort)
            .child(close)
    }

    pub(crate) fn search_body(&self, results: &Results, cx: &mut Context<Self>) -> AnyElement {
        let muted = palette::muted(cx);
        let page = match &results.hits {
            None => return div().min_h_8().px_2().flex().items_center().gap_2().text_color(muted).child(ui::bars("search-wait", muted, 12.)).child("Searching…").into_any_element(),
            Some(Err(error)) => return div().px_2().text_color(palette::danger_ink(cx)).child(error.clone()).into_any_element(),
            Some(Ok(page)) if page.messages.is_empty() => return ui::empty_state("search-empty", crate::illustration::Illustration::SearchEmpty, "We searched far and wide.", "Unfortunately, no results were found.", cx).into_any_element(),
            Some(Ok(page)) => page,
        };
        let ctx = self.row_context(&page.messages, None, false, HIT_COLUMN, cx);
        let mut out: Vec<AnyElement> = Vec::new();
        for (i, hit) in page.messages.iter().enumerate() {
            // A DM's hits are all from it: no channel headings.
            if self.guild_id.is_some() && (i == 0 || page.messages[i - 1].channel_id != hit.channel_id) {
                out.push(self.hit_channel(hit, cx));
            }
            out.push(self.hit_card(hit, &ctx, cx));
        }
        // flex_none: in the panel's scrolling column, cards would otherwise shrink to fit and clip their pictures.
        div().flex_none().flex().flex_col().gap_2().children(out.into_iter().map(|el| div().flex_none().child(el))).into_any_element()
    }

    // A run of hits from one channel opens with its name, and its category at the right.
    fn hit_channel(&self, hit: &Message, cx: &App) -> AnyElement {
        let guild = self.guild();
        let channel = guild.and_then(|g| g.channels.iter().find(|c| c.id == hit.channel_id));
        let category = channel.and_then(|c| c.parent_id.as_ref()).and_then(|p| guild?.channels.iter().find(|c| &c.id == p));
        let muted = palette::muted(cx);
        div()
            .pt_2()
            .px_1()
            .flex()
            .items_center()
            .gap_1p5()
            .text_color(palette::strong(cx))
            .font_weight(FontWeight::SEMIBOLD)
            .child(div().text_color(muted).child(Icon::new(Glyph::Hashtag).size(ui::ICON_MD)))
            .child(div().flex_1().min_w_0().truncate().child(channel.map_or_else(|| "unknown channel".to_owned(), |c| c.name.clone())))
            .children(category.map(|c| div().flex_none().flex().items_center().gap_1().text_size(ui::TEXT_XS).font_weight(FontWeight::NORMAL).text_color(muted).child(Icon::new(Glyph::Folder).size(ui::ICON_SM)).child(c.name.clone())))
            .into_any_element()
    }

    fn hit_card(&self, hit: &Message, ctx: &std::rc::Rc<crate::chat::RowContext>, cx: &mut Context<Self>) -> AnyElement {
        let (hover, page) = (palette::hover(cx), palette::surface(palette::Level::Page, cx));
        let channel = match self.guild() {
            Some(guild) => guild.channels.iter().find(|c| c.id == hit.channel_id).cloned(),
            None => self.channel.clone().filter(|c| c.id == hit.channel_id),
        };
        let message_id = hit.id.clone();
        div()
            .id(SharedString::from(format!("hit-{}", hit.id)))
            .p_3()
            .squircle(RADIUS_MD)
            .bg(page)
            .border_1()
            .border_color(palette::line(cx))
            .cursor_pointer()
            .hover(move |s| s.bg(ui::mix(page, hover, 1.)))
            .flex()
            .flex_col()
            .child(crate::chat::search_hit(hit, ctx, cx).flex_none())
            .on_click(cx.listener(move |this, _, window, cx| {
                let Some(channel) = channel.clone() else { return };
                if this.channel.as_ref().is_none_or(|c| c.id != channel.id) {
                    this.open_channel(channel, window, cx);
                }
                this.jump_to(message_id.clone(), cx);
            }))
            .into_any_element()
    }

    // Previous, the page buttons, Next; nothing for a single page.
    pub(crate) fn search_pager(&self, results: &Results, cx: &mut Context<Self>) -> Option<AnyElement> {
        let total = results.hits.as_ref()?.as_ref().ok()?.total;
        let count = search::page_count(total);
        if count <= 1 {
            return None;
        }
        let current = results.page;
        let step = |id: &'static str, label: &'static str, back: bool, to: usize, enabled: bool, cx: &mut Context<Self>| {
            let chevron = Icon::new(if back { Glyph::ChevronLeft } else { Glyph::ChevronRight }).size(ui::ICON_SM);
            let button = ui::button(id, ui::Tone::Ghost, cx).small().disabled(!enabled);
            let button = if back { button.child(chevron).child(label) } else { button.child(label).child(chevron) };
            button.on_click(cx.listener(move |this, _, _, cx| {
                if enabled {
                    this.set_search_page(to, cx);
                }
            }))
        };
        let (accent, hover, text, faint) = (palette::accent(), palette::hover(cx), palette::text(cx), palette::faint(cx));
        let numbers = search::pages(current, count).into_iter().enumerate().map(|(i, page)| {
            let cell = div().min_w_7().h_7().px_1().flex().items_center().justify_center().rounded_full().text_size(ui::TEXT_SM);
            match page {
                None => cell.id(("search-gap", i)).text_color(faint).child("…").into_any_element(),
                Some(page) if page == current => cell.id(("search-page", page)).bg(accent).text_color(gpui_kit::white()).font_weight(FontWeight::SEMIBOLD).child(page.to_string()).into_any_element(),
                Some(page) => cell.id(("search-page", page)).text_color(text).cursor_pointer().hover(move |s| s.bg(hover)).child(page.to_string()).on_click(cx.listener(move |this, _, _, cx| this.set_search_page(page, cx))).into_any_element(),
            }
        }).collect::<Vec<_>>();
        let (previous, next) = (step("search-previous", "Previous", true, current.saturating_sub(1), current > 1, cx), step("search-next", "Next", false, current + 1, current < count, cx));
        Some(
            div()
                .flex()
                .items_center()
                .justify_center()
                .gap_0p5()
                .child(previous)
                .children(numbers)
                .child(next)
                .into_any_element(),
        )
    }

    // Every filter in one card. Fixed values toggle in place and search again; people, channels and dates
    // list their tokens (× removes) and Add types the filter into the field to pick from its values.
    fn filters_popover(&self, cx: &mut Context<Self>) -> AnyElement {
        let content = self.search_input.read(cx).content();
        let avatars = self.search_avatars();
        let (muted, strong) = (palette::muted(cx), palette::strong(cx));
        let keys = self.search_keys(&Key::MAIN.into_iter().chain(Key::MORE).collect::<Vec<_>>());
        let rows = keys.into_iter().map(|key| {
            let word = key.word();
            let on: Vec<(String, String)> = content.tokens().iter().filter_map(|t| t.token().text().split_once(':').filter(|(k, _)| *k == word).map(|(_, v)| (v.to_owned(), t.token().label().to_string()))).collect();
            let options: Option<&[&str]> = match key {
                Key::Has => Some(&search::HAS),
                Key::AuthorType => Some(&search::AUTHOR_TYPES),
                Key::Pinned => Some(&["true", "false"]),
                _ => None,
            };
            let values: Vec<AnyElement> = match options {
                Some(options) => options
                    .iter()
                    .map(|value| {
                        let picked = on.iter().any(|(v, _)| v == value);
                        let token = format!("{word}:{value}");
                        ui::chip(SharedString::from(format!("filter-{token}")), None, *value, picked, cx).small().on_click(cx.listener(move |this, _, window, cx| this.toggle_search_filter(&token, picked, window, cx))).into_any_element()
                    })
                    .collect(),
                None => on
                    .iter()
                    .map(|(value, label)| {
                        let avatar = matches!(key, Key::From | Key::Mentions).then(|| avatars.get(value).cloned()).flatten();
                        let token = format!("{word}:{value}");
                        let remove = ui::icon_button(SharedString::from(format!("filter-x-{token}")), Glyph::X, "Remove", cx).small().on_click(cx.listener(move |this, _, window, cx| this.toggle_search_filter(&token, true, window, cx)));
                        token_chip(word, label, avatar, true, cx).pr_0p5().child(remove).into_any_element()
                    })
                    .chain(std::iter::once(ui::chip(SharedString::from(format!("filter-add-{word}")), Some(Glyph::Plus), "Add", false, cx).small().on_click(cx.listener(move |this, _, window, cx| this.type_search_filter(key, window, cx))).into_any_element()))
                    .collect(),
            };
            div()
                .px_2p5()
                .py_2()
                .flex()
                .flex_col()
                .gap_1p5()
                .child(div().flex().items_center().gap_2().text_color(strong).child(div().text_color(muted).child(Icon::new(crate::search_menu::key_glyph(key)).size(ui::ICON_MD))).child(key.title()))
                .child(div().flex().flex_wrap().gap_1p5().children(values))
        });
        ui::surface(cx)
            .id("search-filters-card")
            .w(px(360.))
            .max_h(px(560.))
            .overflow_y_scroll()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                if let Some(results) = this.results() {
                    results.filters_open = false;
                }
                cx.notify();
            }))
            .children(rows)
            .into_any_element()
    }

    fn toggle_search_filter(&mut self, token: &str, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        let content = self.search_input.read(cx).content();
        let label = token.split_once(':').map_or(token, |(_, v)| v);
        let next = if on { search::without_token(&content, token) } else { search::with_token(&content, token, label) };
        self.search_input.update(cx, |state, cx| state.set_value(next, window, cx));
        self.run_search(window, cx);
        if let Some(results) = self.results() {
            results.filters_open = true;
        }
    }
}
