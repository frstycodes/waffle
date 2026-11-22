// Channels & Roles: the Customize and Browse Channels tabs, and which channels the list shows when you
// pick your own (design/explore-joining.html §4).
use std::{collections::HashSet, time::Duration};

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    AnyElement, Context, Div, Entity, FontWeight, SharedString, Task, Window, WindowControlArea,
    component::{
        Icon,
        input::{InputEvent, InputState},
    },
    div, img, prelude::*, px,
};
use serde_json::{Value, json};

use super::{GuildPage, Onboarding, Prompt, api, features, settings_flags::{OPT_IN_CHANNELS_ON, OPT_IN_ENABLED}};
use crate::{
    Waffle,
    channel_settings::Load,
    discord::{Channel, Guild, GuildSettings},
    icons::Glyph,
    mascot::Mood,
    palette,
    sidebar::{channel_glyph, channel_sections},
    ui::{self, Check, Squircle},
};

// channel_overrides[].flags: Discord clears it with OPT_IN_ENABLED when you unfollow.
const FAVORITED: u64 = 1 << 11;
const SAVE_AFTER: Duration = Duration::from_secs(1);
const UNDER_FOLLOWED: &str = "You must unfollow this category to add or remove specific channels.";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BrowseTab {
    #[default]
    Customize,
    Browse,
}

#[derive(Default)]
pub struct BrowseView {
    pub tab: BrowseTab,
    search: Option<Entity<InputState>>,
    // The pending answers save; replacing it drops (cancels) the one before.
    save: Option<Task<()>>,
    // The server whose answers last saved, for the "Saved" line.
    saved: Option<String>,
}

impl Waffle {
    pub(crate) fn render_channels_roles(&mut self, guild: &Guild, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let has_prompts = guild.has_feature(features::ONBOARDING_HAS_PROMPTS);
        let tab = if has_prompts { self.join.browse.tab } else { BrowseTab::Browse };
        let body = match tab {
            BrowseTab::Customize => self.customize_tab(guild, cx),
            BrowseTab::Browse => self.browse_tab(guild, window, cx),
        };
        div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(palette::surface(palette::Level::Page, cx))
            .text_color(palette::text(cx))
            .text_size(px(14.))
            // The main view's header: a hard edge, never a hairline.
            .child(ui::hard_edge(|| false, cx).child(self.channels_roles_header(has_prompts, tab, window, cx)))
            .child(div().id("channels-roles").flex_1().min_h_0().overflow_y_scrollbar().pt(px(16.)).px_4().pb_6().child(div().w_full().max_w(px(760.)).mx_auto().child(body)))
            .into_any_element()
    }

    fn channels_roles_header(&self, has_prompts: bool, tab: BrowseTab, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let tabs: Vec<BrowseTab> = if has_prompts { vec![BrowseTab::Customize, BrowseTab::Browse] } else { vec![BrowseTab::Browse] };
        let items: Vec<ui::Tab> = tabs.iter().map(|t| ui::Tab::new(if *t == BrowseTab::Customize { "Customize" } else { "Browse Channels" })).collect();
        let selected = tabs.iter().position(|t| *t == tab).unwrap_or_default();
        let this = cx.entity().downgrade();
        let strip = ui::pill_tabs("channels-roles-tabs", &items, selected, move |i, _, cx| {
            this.update(cx, |this, cx| {
                this.join.browse.tab = tabs[i];
                cx.notify();
            })
            .ok();
        }, window, cx);
        div()
            .occlude()
            .h(px(crate::layout::HEADER_HEIGHT))
            .pl_4()
            .pr_3()
            .flex()
            .items_center()
            .gap_3()
            .window_control_area(WindowControlArea::Drag)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .pr_3()
                    .border_r_1()
                    .border_color(palette::line_strong(cx))
                    .whitespace_nowrap()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette::strong(cx))
                    .child(Icon::new(Glyph::Search).size(px(20.)).text_color(palette::faint(cx)))
                    .child(page_title(has_prompts)),
            )
            .child(div().min_w_0().overflow_hidden().occlude().child(strip))
            .child(div().ml_auto().child(self.caption_clearance(true)))
    }

    // CUSTOMIZE

    fn customize_tab(&mut self, guild: &Guild, cx: &mut Context<Self>) -> AnyElement {
        if !self.join.onboarding.contains_key(&guild.id) {
            self.fetch_onboarding(&guild.id, |_, _| {}, cx);
        }
        let onboarding = match self.join.onboarding.get(&guild.id) {
            Some(Load::Ready(onboarding)) => onboarding.clone(),
            Some(Load::Failed(error)) => return ui::field_error(error.clone(), cx).into_any_element(),
            _ => return ui::progress_indeterminate("customize-loading", cx),
        };
        if onboarding.prompts.is_empty() {
            return ui::empty_state("customize-empty", Mood::Idle, "There are no customization options yet.", "You can add and remove channels from Browse Channels.", cx).into_any_element();
        }
        let count = onboarding.prompts.len();
        let title = if count == 1 { "1 Customization Question".to_owned() } else { format!("{count} Customization Questions") };
        let picked: HashSet<&str> = onboarding.responses.iter().map(String::as_str).collect();
        // Discord lists the questions asked only here before the ones onboarding asked.
        let mut prompts: Vec<&Prompt> = onboarding.prompts.iter().collect();
        prompts.sort_by_key(|p| p.in_onboarding);
        let saved = self.join.browse.saved.as_deref() == Some(guild.id.as_str());
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(div().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(title))
                    .child(div().text_color(palette::muted(cx)).child("Answer questions to get access to more channels and roles.")),
            )
            .children(prompts.into_iter().map(|prompt| self.prompt_card(&guild.id, prompt, &picked, cx)))
            .when(saved, |d| d.child(div().flex().items_center().gap(px(6.)).text_size(px(13.)).text_color(palette::muted(cx)).child(Icon::new(Glyph::Checkmark).size(px(14.))).child("Saved")))
            .into_any_element()
    }

    fn prompt_card(&self, guild_id: &str, prompt: &Prompt, picked: &HashSet<&str>, cx: &mut Context<Self>) -> AnyElement {
        let chips: Vec<_> = prompt.options.iter().map(|option| {
            let on = picked.contains(option.id.as_str());
            let unicode = option.emoji.as_ref().filter(|e| e.id.is_none()).and_then(|e| e.name.clone());
            let label = match unicode {
                Some(emoji) => format!("{emoji} {}", option.title),
                None => option.title.clone(),
            };
            let custom = option.emoji.as_ref().and_then(|e| e.url());
            let (guild_id, prompt_id, option_id) = (guild_id.to_owned(), prompt.id.clone(), option.id.clone());
            ui::chip(SharedString::from(format!("pick-{}", option.id)), None, label, on, cx)
                .when_some(custom, |c, url| c.child(img(url).size(px(18.)).flex_none()))
                .on_click(cx.listener(move |this, _, _, cx| this.pick_option(&guild_id, &prompt_id, &option_id, cx)))
        }).collect();
        div()
            .p(px(16.))
            .squircle(ui::RADIUS_LG)
            .bg(palette::surface(palette::Level::Group, cx))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(prompt.title.clone()))
                    .child(div().text_size(px(13.)).text_color(palette::muted(cx)).child(if prompt.single_select { "Pick one" } else { "Choose all that apply." })),
            )
            .child(div().flex().flex_wrap().gap(px(8.)).children(chips))
            .into_any_element()
    }

    fn pick_option(&mut self, guild_id: &str, prompt_id: &str, option_id: &str, cx: &mut Context<Self>) {
        let Some(Load::Ready(onboarding)) = self.join.onboarding.get_mut(guild_id) else { return };
        let Some(prompt) = onboarding.prompts.iter().find(|p| p.id == prompt_id) else { return };
        toggle_pick(prompt, &mut onboarding.responses, option_id);
        self.join.browse.saved = None;
        cx.notify();
        let guild_id = guild_id.to_owned();
        self.join.browse.save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_AFTER).await;
            this.update(cx, |this, cx| this.save_answers(guild_id, cx)).ok();
        }));
    }

    fn save_answers(&mut self, guild_id: String, cx: &mut Context<Self>) {
        let Some(Load::Ready(onboarding)) = self.join.onboarding.get(&guild_id) else { return };
        let picked: HashSet<String> = onboarding.responses.iter().cloned().collect();
        let body = Onboarding::responses_body(&onboarding.prompts, &picked, chrono::Utc::now().timestamp_millis());
        self.server_call(api::answer(&guild_id, false, body), move |this, result, cx| {
            match result {
                Ok(_) => this.join.browse.saved = Some(guild_id),
                Err(error) => this.show_toast(error, cx),
            }
            cx.notify();
        }, cx);
    }

    // BROWSE CHANNELS

    fn browse_tab(&mut self, guild: &Guild, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let search = self.browse_search(window, cx);
        let query = search.read(cx).value().trim().to_lowercase();
        let settings = self.settings_for(&guild.id);
        let sections = channel_sections(guild, self.my_id());
        let all: Vec<&Channel> = sections.iter().flat_map(|(_, channels)| channels).collect();
        let shown = all.iter().filter(|c| self.shows_in_list(guild, c)).count();
        let tally = div()
            .flex_none()
            .flex()
            .gap(px(4.))
            .whitespace_nowrap()
            .text_color(palette::muted(cx))
            .child("You see")
            .child(div().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(shown.to_string()))
            .child(format!("of {}", all.len()));
        let mut cards = Vec::new();
        for (category, channels) in &sections {
            let matches: Vec<&Channel> = channels.iter().filter(|c| query.is_empty() || c.name.to_lowercase().contains(&query)).collect();
            if !matches.is_empty() {
                cards.push(self.category_card(guild, &settings, category.as_ref(), &matches, window, cx));
            }
        }
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(div().flex().items_center().gap(px(12.)).child(div().flex_1().min_w_0().child(ui::search_field("browse-search", &search, cx))).child(tally))
            .children(cards)
            .child(div().text_size(px(13.)).text_color(palette::muted(cx)).child("Tip: Channels with mentions will always appear on your channel list, even if you hide them."))
            .into_any_element()
    }

    fn browse_search(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(search) = &self.join.browse.search {
            return search.clone();
        }
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search Channels"));
        cx.subscribe(&search, |_, _, _: &InputEvent, cx| cx.notify()).detach();
        self.join.browse.search = Some(search.clone());
        search
    }

    fn category_card(&self, guild: &Guild, settings: &GuildSettings, category: Option<&Channel>, channels: &[&Channel], window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let category_followed = category.is_some_and(|c| followed(settings, &c.id));
        let head = category.map(|category| {
            let state = match (category_followed, channels.iter().any(|c| followed(settings, &c.id))) {
                (true, _) => Check::On,
                (false, true) => Check::Mixed,
                (false, false) => Check::Off,
            };
            let (guild_id, id) = (guild.id.clone(), category.id.clone());
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .pt(px(10.))
                .px(px(12.))
                .pb(px(6.))
                .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::muted(cx)).child(category.name.clone()))
                .child(
                    div()
                        .id(SharedString::from(format!("follow-category-{}", category.id)))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .cursor_pointer()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text(cx))
                        .child(ui::check_live(SharedString::from(format!("follow-category-check-{}", category.id)), state, window, cx))
                        .child("Follow Category")
                        .on_click(cx.listener(move |this, _, _, cx| this.set_followed(&guild_id, &id, !category_followed, cx))),
                )
        });
        div()
            .p(px(ui::SURFACE_INSET))
            .squircle(ui::RADIUS_LG)
            .bg(palette::surface(palette::Level::Group, cx))
            .flex()
            .flex_col()
            .children(head)
            .children(channels.iter().map(|channel| self.browse_row(guild, settings, channel, category_followed, window, cx)))
            .into_any_element()
    }

    // A channel and its follow checkbox; the whole row toggles it unless the rules or its category hold it.
    fn browse_row(&self, guild: &Guild, settings: &GuildSettings, channel: &Channel, under_followed: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let rules = guild.rules_channel_id.as_deref() == Some(channel.id.as_str());
        let own = followed(settings, &channel.id);
        let held = rules || under_followed;
        let state = if held || own { Check::On } else { Check::Off };
        let hover = palette::hover(cx);
        let why = if rules { Some("Everyone sees this channel.") } else if under_followed { Some(UNDER_FOLLOWED) } else { None };
        let (guild_id, id) = (guild.id.clone(), channel.id.clone());
        let text = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(div().truncate().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(channel.name.clone()))
            .when_some(channel.topic.clone().filter(|t| !t.is_empty()), |d, topic| d.child(div().truncate().text_size(px(13.)).text_color(palette::muted(cx)).child(topic)));
        div()
            .id(SharedString::from(format!("browse-{}", channel.id)))
            .w_full()
            .min_h(px(44.))
            .px(px(12.))
            .py(px(8.))
            .flex()
            .items_center()
            .gap(px(12.))
            .squircle(ui::RADIUS_MD)
            .child(channel_glyph(channel).size(px(20.)).flex_none().text_color(palette::faint(cx)))
            .child(text)
            .when(rules, |r| r.child(div().flex_none().text_size(px(12.)).text_color(palette::muted(cx)).child("Everyone sees this")))
            .child(div().flex_none().when(held, |d| d.opacity(0.5)).child(ui::check_live(SharedString::from(format!("browse-check-{}", channel.id)), state, window, cx)))
            .when_some(why, |r, why| r.tooltip(move |window, cx| ui::tip(why, window, cx)))
            .when(!held, |r| r.cursor_pointer().hover(move |s| s.bg(hover)).on_click(cx.listener(move |this, _, _, cx| this.set_followed(&guild_id, &id, !own, cx))))
            .into_any_element()
    }

    fn set_followed(&mut self, guild_id: &str, id: &str, follow: bool, cx: &mut Context<Self>) {
        let body = follow_body(&self.settings_for(guild_id), id, follow);
        self.patch_guild_settings(guild_id.to_owned(), body, cx);
    }

    // Whether the channel list shows this channel (opt-in channels hide what you don't follow).
    pub(crate) fn shows_in_list(&self, guild: &Guild, channel: &Channel) -> bool {
        let always = guild.rules_channel_id.as_deref() == Some(channel.id.as_str())
            || self.mention_count(&channel.id) > 0
            || self.channel.as_ref().is_some_and(|c| c.id == channel.id)
            || self.call.as_ref().is_some_and(|c| c.channel.id == channel.id);
        listed(self.guild_settings.get(&guild.id), &channel.id, channel.parent_id.as_deref(), always)
    }
}

pub(crate) fn page_title(has_prompts: bool) -> &'static str {
    if has_prompts { "Channels & Roles" } else { "Browse Channels" }
}

fn followed(settings: &GuildSettings, id: &str) -> bool {
    settings.channels.get(id).is_some_and(|c| c.flags & OPT_IN_ENABLED != 0)
}

// With opt-in on, a channel shows when it or its category is followed, or `always` holds (the rules,
// mentions, the open channel, the voice channel you're in). With it off, everything shows.
fn listed(settings: Option<&GuildSettings>, id: &str, parent: Option<&str>, always: bool) -> bool {
    let Some(settings) = settings.filter(|s| s.flags & OPT_IN_CHANNELS_ON != 0) else { return true };
    always || followed(settings, id) || parent.is_some_and(|p| followed(settings, p))
}

// Follows or unfollows a channel or category, keeping its override's other bits; the first follow while
// opt-in is off turns it on in the same body.
// Not verified: Discord itself sends this through the bulk PATCH /users/@me/guilds/settings
// ({"guilds": {id: body}}); Waffle sends the same body to the per-server route.
fn follow_body(settings: &GuildSettings, id: &str, follow: bool) -> Value {
    let current = settings.channels.get(id).map_or(0, |c| c.flags);
    let flags = if follow { current | OPT_IN_ENABLED } else { current & !(OPT_IN_ENABLED | FAVORITED) };
    let mut body = json!({ "channel_overrides": { id: { "flags": flags } } });
    if settings.flags & OPT_IN_CHANNELS_ON == 0 {
        body["flags"] = json!(settings.flags | OPT_IN_CHANNELS_ON);
    }
    body
}

// A chip click: single-select swaps, multi-select toggles; a required question keeps its last answer.
fn toggle_pick(prompt: &Prompt, responses: &mut Vec<String>, option_id: &str) {
    let ours = |r: &String| prompt.options.iter().any(|o| o.id == *r);
    let had = responses.iter().any(|r| r == option_id);
    if had && prompt.required && responses.iter().filter(|r| ours(r)).count() == 1 {
        return;
    }
    if prompt.single_select {
        responses.retain(|r| !ours(r));
    } else {
        responses.retain(|r| r != option_id);
    }
    if !had {
        responses.push(option_id.to_owned());
    }
}

pub fn demo(waffle: &mut Waffle, scene: &str, _window: &mut Window, cx: &mut Context<Waffle>) {
    waffle.join.browse.tab = BrowseTab::Customize;
    if scene == "roles" {
        return waffle.open_guild_page(GuildPage::ChannelsRoles, cx);
    }
    // Opt-in on, following media and the Voice category: long-named and staff stay off the list unless pinned.
    let mut settings = waffle.settings_for("1");
    settings.flags |= OPT_IN_CHANNELS_ON;
    for id in ["media", "c1"] {
        settings.channels.entry(id.into()).or_insert_with(|| crate::discord::ChannelSettings { notifications: 3, ..Default::default() }).flags |= OPT_IN_ENABLED;
    }
    waffle.guild_settings.insert("1".into(), settings);
    if scene == "roles-browse" {
        waffle.join.browse.tab = BrowseTab::Browse;
        waffle.open_guild_page(GuildPage::ChannelsRoles, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discord::ChannelSettings;

    fn settings(flags: u64, followed: &[&str]) -> GuildSettings {
        let channels = followed.iter().map(|id| (id.to_string(), ChannelSettings { flags: OPT_IN_ENABLED | FAVORITED, ..Default::default() })).collect();
        GuildSettings { flags, channels, ..Default::default() }
    }

    #[test]
    fn opt_in_lists_followed_channels_their_categories_and_pinned_ones() {
        assert!(listed(None, "c", Some("cat"), false));
        assert!(listed(Some(&settings(0, &[])), "c", Some("cat"), false));
        let on = settings(OPT_IN_CHANNELS_ON, &["followed", "cat2"]);
        assert!(!listed(Some(&on), "c", Some("cat"), false));
        assert!(listed(Some(&on), "c", Some("cat"), true));
        assert!(listed(Some(&on), "followed", None, false));
        assert!(listed(Some(&on), "child", Some("cat2"), false));
    }

    #[test]
    fn following_keeps_other_bits_and_turns_opt_in_on() {
        let body = follow_body(&settings(0, &[]), "c", true);
        assert_eq!(body["channel_overrides"]["c"]["flags"], OPT_IN_ENABLED);
        assert_eq!(body["flags"], OPT_IN_CHANNELS_ON);
        let mut on = settings(OPT_IN_CHANNELS_ON, &["c"]);
        on.channels.get_mut("c").unwrap().flags |= 1;
        let body = follow_body(&on, "c", false);
        assert_eq!(body["channel_overrides"]["c"]["flags"], 1);
        assert!(body.get("flags").is_none());
    }

    #[test]
    fn picks_swap_toggle_and_keep_a_required_answer() {
        let ob = super::super::fixtures::onboarding();
        let (why, games) = (&ob.prompts[0], &ob.prompts[1]);
        let mut picks = vec!["o-ranked".to_owned(), "o-hades".to_owned()];
        toggle_pick(why, &mut picks, "o-clips");
        assert_eq!(picks, ["o-hades", "o-clips"]);
        toggle_pick(why, &mut picks, "o-clips");
        assert_eq!(picks, ["o-hades", "o-clips"]);
        toggle_pick(games, &mut picks, "o-hades");
        toggle_pick(games, &mut picks, "o-rift");
        assert_eq!(picks, ["o-clips", "o-rift"]);
    }
}
