// The rows atop a server's channel list: Get Started, Finish joining, Server Guide, Channels & Roles
// (design/explore-joining.html §3).
use gpui_kit::{AnyElement, Context, FontWeight, SharedString, Stateful, Div, Window, component::Icon, div, prelude::*, relative};

use super::{GuildPage, browse::page_title, features, settings_flags::OPT_IN_CHANNELS_ON};
use crate::{
    Waffle,
    discord::Guild,
    icons::Glyph,
    palette,
    sidebar::{RowState, channel_row},
    ui::{self, Squircle},
};

impl Waffle {
    pub(crate) fn guild_entry_rows(&mut self, guild: &Guild, _window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut rows = Vec::new();
        if let Some((done, total)) = self.guide_progress(guild).filter(|(_, total)| *total > 0) {
            rows.push(get_started(done, total, cx));
        }
        if let Some((answered, total)) = self.onboarding_progress(&guild.id) {
            let guild_id = guild.id.clone();
            let label: SharedString = format!("{answered} of {total} questions answered").into();
            let row = self
                .entry_row("finish-joining", Glyph::Note, "Finish joining", None, cx)
                .child(progress_dots(answered, total, cx))
                .tooltip(move |window, cx| ui::tip(label.clone(), window, cx))
                .on_click(cx.listener(move |this, _, _, cx| this.open_onboarding(guild_id.clone(), cx)));
            rows.push(row.into_any_element());
        }
        let community = guild.has_feature(features::COMMUNITY);
        if community && guild.has_feature(features::SERVER_GUIDE) && guild.has_feature(features::ONBOARDING) {
            rows.push(self.page_row("server-guide", Glyph::Signature, "Server Guide", GuildPage::Guide, cx));
        }
        let has_prompts = guild.has_feature(features::ONBOARDING_HAS_PROMPTS);
        let opted_in = self.guild_settings.get(&guild.id).is_some_and(|s| s.flags & OPT_IN_CHANNELS_ON != 0);
        if community && (has_prompts || opted_in) {
            rows.push(self.page_row("channels-roles", Glyph::Search, page_title(has_prompts), GuildPage::ChannelsRoles, cx));
        }
        if !rows.is_empty() {
            rows.push(div().h(ui::HAIRLINE).mx_2p5().my_1p5().bg(palette::line(cx)).into_any_element());
        }
        rows
    }

    fn page_row(&self, id: &'static str, glyph: Glyph, label: &'static str, page: GuildPage, cx: &mut Context<Self>) -> AnyElement {
        self.entry_row(id, glyph, label, Some(page), cx).on_click(cx.listener(move |this, _, _, cx| this.open_guild_page(page, cx))).into_any_element()
    }

    // A channel row's look with a page glyph in place of the channel's; selected while its page is open.
    fn entry_row(&self, id: &'static str, glyph: Glyph, label: &'static str, page: Option<GuildPage>, cx: &mut Context<Self>) -> Stateful<Div> {
        let selected = page.is_some_and(|p| self.guild_page == Some(p));
        let state = RowState { selected, unread: false, muted: false, strong: false };
        channel_row(id.into(), format!("entry-{id}").into(), state, cx)
            .child(Icon::new(glyph).size(ui::ICON_LG).flex_none().text_color(if selected { palette::muted(cx) } else { palette::faint(cx) }))
            .child(ui::truncating(format!("entry-name-{id}"), label).flex_1())
    }
}

// The new member's to-dos: a raised card with the count and a success-coloured bar; opens the Server Guide.
fn get_started(done: usize, total: usize, cx: &mut Context<Waffle>) -> AnyElement {
    let fraction = done as f32 / total as f32;
    let (line, hover) = (palette::line(cx), palette::hover(cx));
    div()
        .id("get-started")
        .flex_none()
        .mt_0p5()
        .mb_1p5()
        .px_3()
        .py_2p5()
        .flex()
        .flex_col()
        .gap_2()
        .squircle(ui::RADIUS_MD)
        .bg(palette::surface(palette::Level::Raised, cx))
        .border_1()
        .border_color(line)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(div().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child("Get Started"))
                .child(div().text_size(ui::TEXT_XS).text_color(palette::muted(cx)).child(format!("{done} of {total}"))),
        )
        .child(div().h_1p5().w_full().rounded_full().bg(palette::line_strong(cx)).child(div().h_full().w(relative(fraction)).rounded_full().bg(palette::positive_ink(cx))))
        .on_click(cx.listener(|this, _, _, cx| this.open_guild_page(GuildPage::Guide, cx)))
        .into_any_element()
}

fn progress_dots(answered: usize, total: usize, cx: &mut Context<Waffle>) -> Div {
    let (accent, rest) = (palette::accent(), palette::line_strong(cx));
    div().flex_none().flex().gap_1().children((0..total).map(|i| div().size_1p5().rounded_full().bg(if i < answered { accent } else { rest })))
}
