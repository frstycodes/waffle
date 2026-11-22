// The Inbox (panels.html#inbox): Unreads, the server channels with messages you haven't read, each with
// Mark read; and Mentions, filtered by visible chips. Shown in the rail's popover and on the Inbox page.
use gpui_kit::{AnyElement, Context, Div, FontWeight, SharedString, Window, component::Icon, div, prelude::*};

use crate::{
    Waffle,
    discord::{Channel, ChannelKind, Message},
    icons::Glyph,
    illustration::Illustration,
    palette,
    ui::{self, RADIUS_SM, Squircle},
};

// Cached messages shown under each unread channel; the channel opens on the rest.
const UNREAD_PREVIEW: usize = 3;

#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) enum InboxTab {
    Unreads,
    #[default]
    Mentions,
    // Discord's saved-for-later messages.
    Saved,
}

/// What the Mentions tab includes, kept across restarts.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct MentionFilters {
    pub everyone: bool,
    pub roles: bool,
    pub this_server: bool,
}

impl Default for MentionFilters {
    fn default() -> Self {
        MentionFilters { everyone: true, roles: true, this_server: false }
    }
}

// One channel on the Unreads tab.
struct Unread {
    guild_icon: Option<String>,
    guild_name: String,
    guild_id: String,
    channel: Channel,
    mentions: u32,
    // The newest cached messages past the read line; empty when the channel hasn't been loaded.
    messages: Vec<Message>,
}

impl Waffle {
    // Unmuted text channels you can see with messages past your read line, newest activity first.
    fn unreads(&self) -> Vec<Unread> {
        let me = self.my_id();
        let mut list: Vec<(u64, Unread)> = Vec::new();
        for guild in &self.guilds {
            let settings = self.guild_settings.get(&guild.id);
            if settings.is_some_and(|s| s.is_muted()) {
                continue;
            }
            for channel in guild.channels.iter().filter(|c| c.kind == ChannelKind::Text && guild.can_view(me, c) && self.unread(&c.id)) {
                let muted = settings.is_some_and(|s| s.channel_muted(&channel.id) || channel.parent_id.as_ref().is_some_and(|p| s.channel_muted(p)));
                if muted {
                    continue;
                }
                let last_read = self.read_states.get(&channel.id).map_or(0, |r| r.last_read);
                let fresh: Vec<Message> = self.feeds.get(&channel.id).map(|f| f.messages.iter().filter(|m| m.id.parse::<u64>().is_ok_and(|id| id > last_read)).cloned().collect()).unwrap_or_default();
                let messages = fresh[fresh.len().saturating_sub(UNREAD_PREVIEW)..].to_vec();
                let last = self.last_message.get(&channel.id).copied().unwrap_or(0);
                list.push((last, Unread { guild_icon: guild.icon.clone(), guild_name: guild.name.clone(), guild_id: guild.id.clone(), channel: channel.clone(), mentions: self.mention_count(&channel.id), messages }));
            }
        }
        list.sort_by_key(|(last, _)| std::cmp::Reverse(*last));
        list.into_iter().map(|(_, u)| u).collect()
    }

    /// Closes the Inbox, opens the message's channel (a server's or a DM) and scrolls to it, flashed.
    pub(crate) fn open_message(&mut self, guild_id: Option<String>, channel_id: &str, message_id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.inbox_open = false;
        let Some((guild_id, channel)) = self.place_of(guild_id.as_deref(), channel_id) else { return };
        if self.channel.as_ref().is_none_or(|c| c.id != channel.id) {
            self.guild_id = guild_id;
            self.open_channel(channel, window, cx);
        } else {
            self.inbox_page = false;
        }
        self.jump_to(message_id, cx);
    }

    /// Where a channel lives: its server (or None for a DM) and the channel. Messages from REST, such as
    /// the mentions list, can come without `guild_id`, so a missing one is looked up by channel.
    pub(crate) fn place_of(&self, guild_id: Option<&str>, channel_id: &str) -> Option<(Option<String>, Channel)> {
        let in_guild = |g: &crate::discord::Guild| g.channels.iter().chain(&g.threads).find(|c| c.id == channel_id).map(|c| (Some(g.id.clone()), c.clone()));
        match guild_id {
            Some(id) => self.guilds.iter().find(|g| g.id == id).and_then(in_guild),
            None => self.guilds.iter().find_map(in_guild).or_else(|| self.dms.iter().find(|c| c.id == channel_id).map(|c| (None, c.clone()))),
        }
    }

    pub(crate) fn set_inbox_tab(&mut self, tab: InboxTab, cx: &mut Context<Self>) {
        self.inbox_tab = tab;
        if tab == InboxTab::Mentions && self.inbox.is_none() {
            self.fetch_mentions(cx);
        }
        if tab == InboxTab::Saved && self.saved.is_none() {
            self.fetch_saved(cx);
        }
        cx.notify();
    }

    fn set_mention_filters(&mut self, change: impl FnOnce(&mut MentionFilters), cx: &mut Context<Self>) {
        self.update_prefs(cx, |p| change(&mut p.mention_filters));
        self.fetch_mentions(cx);
    }

    /// ⌘I / Ctrl+I: the rail's Inbox popover, opened or closed.
    pub(crate) fn toggle_inbox(&mut self, cx: &mut Context<Self>) {
        if self.inbox_open {
            self.inbox_open = false;
        } else {
            self.open_inbox(cx);
        }
        cx.notify();
    }

    /// ⌘⇧E / Ctrl+Shift+E: reads the channel at the top of Unreads, as its own Mark read does.
    pub(crate) fn mark_top_unread_read(&mut self, cx: &mut Context<Self>) {
        let Some(top) = self.unreads().into_iter().next() else { return };
        self.mark_channel_read(&top.channel.id, cx);
    }

    fn mark_all_unreads(&mut self, cx: &mut Context<Self>) {
        let channels: Vec<String> = self.unreads().into_iter().map(|u| u.channel.id).collect();
        self.mark_channels_read(&channels, cx);
    }

    /// The Inbox's title row (Mark all read on Unreads) over its tabs, the Unreads tab counting channels. The
    /// page has its own header, so it takes the row without the title.
    pub(crate) fn inbox_head(&self, titled: bool, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let unread = self.unreads().len();
        let mark_all = (self.inbox_tab == InboxTab::Unreads).then(|| {
            ui::button("inbox-mark-all", ui::Tone::Ghost, cx)
                .small()
                .disabled(unread == 0)
                .child(Icon::new(Glyph::Checks).size(ui::ICON_MD))
                .child("Mark all read")
                .on_click(cx.listener(|this, _, _, cx| this.mark_all_unreads(cx)))
        });
        let title = div()
            .h_8()
            .flex()
            .items_center()
            .gap_2()
            .when(titled, |t| t.child(Icon::new(Glyph::Inbox).size(ui::ICON_XL).text_color(palette::faint(cx))).child(div().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child("Inbox")))
            .child(div().flex_1())
            .children(mark_all);
        const TABS: [InboxTab; 3] = [InboxTab::Unreads, InboxTab::Mentions, InboxTab::Saved];
        let items = [ui::Tab::new("Unreads").count((unread > 0).then_some(unread as u32)), ui::Tab::new("Mentions"), ui::Tab::new("Saved")];
        let selected = TABS.iter().position(|t| *t == self.inbox_tab).unwrap_or(1);
        let this = cx.entity().downgrade();
        let tabs = ui::tabs("inbox-tabs", &items, selected, move |i, _, cx| {
            this.update(cx, |this, cx| this.set_inbox_tab(TABS[i], cx)).ok();
        }, window, cx);
        div().flex().flex_col().gap_1().child(title).child(tabs)
    }

    /// The open tab's list: Mentions under its filter chips (at most `limit`), or the unread channels.
    pub(crate) fn inbox_body(&self, limit: Option<usize>, cx: &mut Context<Self>) -> AnyElement {
        match self.inbox_tab {
            InboxTab::Mentions => div().flex().flex_col().gap_2().child(self.mention_filters(cx)).child(self.message_cards(&self.inbox, limit, ("You're all caught up!", "Mentions of you and your roles land here."), cx)).into_any_element(),
            InboxTab::Saved => self.message_cards(&self.saved, limit, ("No saved messages", "Save a message from its menu to find it here later."), cx),
            InboxTab::Unreads => self.unread_list(cx),
        }
    }

    // Visible chips, as the design asks, instead of Discord's settings menu.
    fn mention_filters(&self, cx: &mut Context<Self>) -> Div {
        let filters = self.prefs.mention_filters;
        let everyone = ui::chip("mentions-everyone", Some(Glyph::At), "Include @everyone", filters.everyone, cx).on_click(cx.listener(|this, _, _, cx| this.set_mention_filters(|f| f.everyone = !f.everyone, cx)));
        let roles = ui::chip("mentions-roles", Some(Glyph::Users), "Include roles", filters.roles, cx).on_click(cx.listener(|this, _, _, cx| this.set_mention_filters(|f| f.roles = !f.roles, cx)));
        // Only meaningful with a server open.
        let here = self.guild_id.is_some().then(|| {
            ui::chip("mentions-here", Some(Glyph::Filter), "This server only", filters.this_server, cx).on_click(cx.listener(|this, _, _, cx| this.set_mention_filters(|f| f.this_server = !f.this_server, cx)))
        });
        div().flex().flex_wrap().gap_1p5().child(everyone).child(roles).children(here)
    }

    fn unread_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let unreads = self.unreads();
        if unreads.is_empty() {
            return ui::empty_state("unreads-empty", Illustration::InboxEmpty, "You're all caught up", "New messages in channels you follow will gather here.", cx).into_any_element();
        }
        let messages: Vec<Message> = unreads.iter().flat_map(|u| u.messages.iter().cloned()).collect();
        let ctx = self.row_context_everywhere(&messages, 380., cx);
        let mut groups = Vec::with_capacity(unreads.len());
        for unread in unreads {
            groups.push(self.unread_group(unread, &ctx, cx));
        }
        div().flex().flex_col().gap_3().children(groups).into_any_element()
    }

    // The channel's row (server, name, what's new, Mark read), then its newest cached messages.
    fn unread_group(&self, unread: Unread, ctx: &std::rc::Rc<crate::chat::RowContext>, cx: &mut Context<Self>) -> Div {
        let new = whats_new(unread.messages.len(), unread.mentions);
        let channel_id = unread.channel.id.clone();
        let (open_guild, open_channel) = (unread.guild_id.clone(), unread.channel.clone());
        let head = div()
            .id(SharedString::from(format!("unread-{}", unread.channel.id)))
            .px_2()
            .py_1()
            .squircle(RADIUS_SM)
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .hover(|s| s.bg(palette::hover(cx)))
            .child(ui::server_tile(unread.guild_icon.as_deref(), &unread.guild_name, 24., cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().truncate().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(format!("#{}", unread.channel.name)))
                    .child(div().truncate().text_size(ui::TEXT_XS).text_color(palette::muted(cx)).child(format!("{} · {new}", unread.guild_name))),
            )
            .child(ui::button(SharedString::from(format!("unread-read-{}", unread.channel.id)), ui::Tone::Ghost, cx).small().child(Icon::new(Glyph::Checkmark).size(ui::ICON_MD)).child("Mark read").on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.mark_channel_read(&channel_id, cx);
            })))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.inbox_open = false;
                this.guild_id = Some(open_guild.clone());
                this.open_channel(open_channel.clone(), window, cx);
            }));
        let mut rows = Vec::with_capacity(unread.messages.len());
        for m in unread.messages {
            rows.push(self.unread_message(m, &unread.guild_id, ctx, cx));
        }
        div().flex().flex_col().gap_1().child(head).children(rows)
    }

    fn unread_message(&self, m: Message, guild_id: &str, ctx: &std::rc::Rc<crate::chat::RowContext>, cx: &mut Context<Self>) -> AnyElement {
        let (guild_id, channel_id, message_id) = (guild_id.to_owned(), m.channel_id.clone(), m.id.clone());
        let text = crate::chat::content(&m, ctx, cx).unwrap_or_else(|| div().text_color(palette::muted(cx)).child("Attachment").into_any_element());
        div()
            .id(SharedString::from(format!("unread-msg-{}", m.id)))
            .ml_8()
            .px_2()
            .py_1()
            .squircle(RADIUS_SM)
            .flex()
            .gap_2()
            .cursor_pointer()
            .hover(|s| s.bg(palette::hover(cx)))
            .child(ui::avatar(m.author.avatar.clone(), 24.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().flex().items_baseline().gap_2().child(div().truncate().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(m.author.name.clone())).child(div().flex_none().text_size(ui::TEXT_XS).text_color(palette::faint(cx)).child(crate::chat::format_stamp(&m.timestamp))))
                    .child(div().text_size(ui::TEXT_SM).line_clamp(2).text_color(palette::text(cx)).child(text)),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.open_message(Some(guild_id.clone()), &channel_id, message_id.clone(), window, cx)))
            .into_any_element()
    }
}

// What a channel on Unreads has: cached new messages (capped at the preview, so "3+") and mentions.
fn whats_new(cached: usize, mentions: u32) -> String {
    let mentions = match mentions {
        0 => None,
        1 => Some("1 mention".to_owned()),
        m => Some(format!("{m} mentions")),
    };
    let messages = (cached > 0).then(|| format!("{cached}{} new", if cached == UNREAD_PREVIEW { "+" } else { "" }));
    match (messages, mentions) {
        (None, None) => "New messages".to_owned(),
        (Some(m), None) | (None, Some(m)) => m,
        (Some(m), Some(n)) => format!("{m} · {n}"),
    }
}

#[cfg(test)]
mod tests {
    use super::whats_new;

    #[test]
    fn says_what_is_new() {
        assert_eq!(whats_new(0, 0), "New messages");
        assert_eq!(whats_new(0, 1), "1 mention");
        assert_eq!(whats_new(2, 0), "2 new");
        assert_eq!(whats_new(3, 4), "3+ new · 4 mentions");
    }
}
