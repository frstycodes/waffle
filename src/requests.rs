// Message Requests (docs/discord-message-requests-re.md, design/explore-dm-social.html §3): DMs from people
// you aren't friends with, and the ones Discord filtered as spam, wait here until you accept or drop them.
// The row under Friends, the page with its two tabs, the bar over a request's composer, and the calls.
//
// GAP: Discord opens a clicked request in a side panel beside the list; ours opens it as the main chat.
use std::collections::HashSet;

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{AnyElement, Context, Div, FontWeight, SharedString, Window, WindowControlArea, component::Icon, div, prelude::*, px};
use crate::discord::Method;
use serde_json::json;

use crate::{
    Waffle,
    chat::emphasised,
    discord::Channel,
    icons::Glyph,
    mascot::Mood,
    palette,
    profile::profile_key,
    sidebar::{RowState, channel_row},
    ui::{self},
};

const REQUEST_ERROR: &str = "Uh oh! Something went wrong while updating this message request. Please wait a moment and try again.";
const SPAM_ERROR: &str = "Uh oh! Something went wrong while updating this spam message. Please wait a moment and try again.";
// Discord rejects at most this many channels per batch-reject call.
const BATCH: usize = 50;
const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RequestsTab {
    #[default]
    Requests,
    Spam,
}

impl RequestsTab {
    fn holds(self, dm: &Channel) -> bool {
        match self {
            RequestsTab::Requests => !dm.props.spam,
            RequestsTab::Spam => dm.props.spam,
        }
    }
}

impl Waffle {
    // Under Friends while anything waits, requests or spam; the badge counts requests only, as Discord's.
    pub(crate) fn requests_row(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.dm_requests.is_empty() {
            return None;
        }
        let pending = self.dm_requests.iter().filter(|d| RequestsTab::Requests.holds(d)).count() as u32;
        let open_request = self.channel.as_ref().is_some_and(Channel::is_request);
        let selected = self.guild_id.is_none() && (open_request || (self.channel.is_none() && self.requests_page.is_some()));
        let state = RowState { selected, unread: false, muted: false, strong: false };
        Some(
            channel_row("requests-row".into(), "requests-row".into(), state, cx)
                .h_10()
                .gap_3()
                .child(Icon::new(Glyph::Envelope).size(ui::ICON_XL).flex_none())
                .child(ui::line("Message Requests").flex_1())
                .when(pending > 0, |r| r.child(ui::Badge::count(pending)))
                .on_click(cx.listener(|this, _, _, cx| this.open_requests(RequestsTab::Requests, cx)))
                .into_any_element(),
        )
    }

    pub(crate) fn open_requests(&mut self, tab: RequestsTab, cx: &mut Context<Self>) {
        self.inbox_page = false;
        self.guild_id = None;
        self.channel = None;
        self.call_view = false;
        self.requests_page = Some(tab);
        self.guild_page = None;
        // Usernames and mutual servers come from each person's profile.
        let people: Vec<_> = self.dm_requests.iter().filter_map(|d| d.recipient.clone()).collect();
        for person in people {
            self.fetch_profile(person, None, cx);
        }
        cx.notify(); // lint-allow(global-notify): opening the page changes the rail, the channel list and the main column
    }

    // One tab's DMs, newest message first.
    fn requests_in(&self, tab: RequestsTab) -> Vec<Channel> {
        let mut dms: Vec<Channel> = self.dm_requests.iter().filter(|d| tab.holds(d)).cloned().collect();
        dms.sort_by_key(|d| std::cmp::Reverse(self.last_message_of(d)));
        dms
    }

    fn last_message_of(&self, dm: &Channel) -> u64 {
        let listed = dm.last_message_id.as_deref().and_then(|id| id.parse().ok()).unwrap_or(0);
        self.last_message.get(&dm.id).copied().unwrap_or(0).max(listed)
    }

    pub(crate) fn render_requests_page(&self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let tab = self.requests_page.unwrap_or_default();
        let spam = self.dm_requests.iter().filter(|d| RequestsTab::Spam.holds(d)).count();
        let items = [ui::Tab::new("Requests"), ui::Tab::new(if spam > 0 { format!("Spam ({spam})") } else { "Spam".into() })];
        const TABS: [RequestsTab; 2] = [RequestsTab::Requests, RequestsTab::Spam];
        let this = cx.entity().downgrade();
        let tabs = ui::ToggleGroup::new("requests-tabs").items(&items).value(TABS.iter().position(|t| *t == tab).unwrap_or_default()).on_value_change(move |i, _, cx| {
            this.update(cx, |this, cx| this.open_requests(TABS[i], cx)).ok();
        });
        let header = ui::row()
            .occlude()
            .h(px(crate::layout::HEADER_HEIGHT))
            .pl_4()
            .pr_3()
            .gap_3()
            .window_control_area(WindowControlArea::Drag)
            .child(
                ui::row()
                    .gap_2()
                    .pr_3()
                    .border_r_1()
                    .border_color(palette::line_strong(cx))
                    .whitespace_nowrap()
                    .text_size(ui::TEXT_MD)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette::strong(cx))
                    .child(Icon::new(Glyph::Envelope).size(ui::ICON_XL).text_color(palette::faint(cx)))
                    .child("Message Requests"),
            )
            .child(div().min_w_0().overflow_hidden().occlude().child(tabs))
            .child(ui::row().flex_1().justify_end().child(self.caption_clearance(true)));
        let dms = self.requests_in(tab);
        let body = match (dms.is_empty(), tab) {
            (true, RequestsTab::Requests) => ui::Empty::new("requests-empty", Mood::Idle, "No pending requests", "There are no pending message requests.", cx).into_any_element(),
            (true, RequestsTab::Spam) => ui::Empty::new("requests-empty", Mood::Idle, "No spam", "There are no spam requests.", cx).into_any_element(),
            (false, _) => {
                let heading = match tab {
                    RequestsTab::Requests => format!("Pending Requests · {}", dms.len()),
                    RequestsTab::Spam => format!("Spam · {}", dms.len()),
                };
                let clear = ui::Button::new("requests-clear").variant(ui::ButtonVariant::Link).text_size(ui::TEXT_XS).child("Clear All").on_click(cx.listener(move |this, _, _, cx| this.clear_requests(tab, cx)));
                let label = ui::row().pt_2().px_2().pb_1().justify_between().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(palette::faint(cx)).child(heading).child(clear);
                let rows: Vec<AnyElement> = dms.iter().map(|dm| self.message_request_row(dm, tab, cx)).collect();
                ui::col().gap_0p5().child(label).children(rows).into_any_element()
            }
        };
        ui::col().min_h_0()
            .relative()
            .flex_1()
            .h_full()
            .bg(palette::surface(palette::Level::Page, cx))
            .text_color(palette::text(cx))
            .text_size(ui::TEXT_SM)
            .child(ui::hard_edge(|| false, cx).child(header))
            .children(self.voice_notice_bar(cx))
            .child(div().id("requests-list").flex_1().min_h_0().overflow_y_scrollbar().pt_4().px_4().pb_6().child(body))
            .into_any_element()
    }

    // GAP: Discord shows up to three mutual server icons and swaps the line for a message preview on hover
    // (`/users/@me/message-requests/supplemental-data`); the profile gives us only the count.
    fn message_request_row(&self, dm: &Channel, tab: RequestsTab, cx: &mut Context<Self>) -> AnyElement {
        let page = palette::surface(palette::Level::Page, cx);
        let name = dm.recipient.as_ref().map_or(dm.name.clone(), |u| u.name.clone());
        let profile = dm.recipient.as_ref().and_then(|u| self.profiles.peek(&profile_key(&u.id, None)));
        let line: Vec<String> = [profile.as_ref().map(|p| p.username.clone()).filter(|u| !u.is_empty()), profile.as_ref().map(|p| mutual_servers(p.mutual_server_count))].into_iter().flatten().collect();
        let last = self.last_message_of(dm);
        let when = (last > 0).then(|| crate::panel::ago(chrono::Utc::now().timestamp_millis() - snowflake_ms(last) as i64));
        let id = dm.id.clone();
        ui::Item::new(SharedString::from(format!("request-{}", dm.id)), name, cx)
            .height(ui::RowHeight::Fit)
            .min_h_16()
            .px_2()
            .py_2()
            .lead(ui::dm_face(&dm.face(), 40., page))
            .subtitle(line.join(" · "))
            .trailing(ui::row().items_stretch().gap_2().on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| cx.stop_propagation()).children(self.request_actions(&dm.id, tab, cx)))
            .when_some(when, |row, when| row.stamp(when))
            .on_click(cx.listener(move |this, _, window, cx| this.open_request(&id, window, cx)))
            .into_any_element()
    }

    fn request_actions(&self, id: &str, tab: RequestsTab, cx: &mut Context<Self>) -> [ui::Button; 2] {
        let (a, b) = (id.to_owned(), id.to_owned());
        match tab {
            RequestsTab::Requests => [
                ui::Button::new(SharedString::from(format!("ignore-{id}"))).variant(ui::ButtonVariant::Secondary).size(ui::ButtonSize::Sm).child("Ignore").on_click(cx.listener(move |this, _, _, cx| this.ignore_request(&a, cx))),
                ui::Button::new(SharedString::from(format!("accept-{id}"))).size(ui::ButtonSize::Sm).child("Accept DM").on_click(cx.listener(move |this, _, _, cx| this.accept_request(&b, cx))),
            ],
            RequestsTab::Spam => [
                ui::Button::new(SharedString::from(format!("not-spam-{id}"))).variant(ui::ButtonVariant::Secondary).size(ui::ButtonSize::Sm).child("Not spam").on_click(cx.listener(move |this, _, _, cx| this.accept_request(&a, cx))),
                ui::Button::new(SharedString::from(format!("report-spam-{id}"))).variant(ui::ButtonVariant::Destructive).size(ui::ButtonSize::Sm).child("Report Spam").on_click(cx.listener(move |this, _, _, cx| this.report_spam(&b, cx))),
            ],
        }
    }

    fn open_request(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dm) = self.dm_requests.iter().find(|d| d.id == id).cloned() else { return };
        self.guild_id = None;
        self.open_channel(dm, window, cx);
    }

    // Over an open request's composer: the same two choices as its row.
    pub(crate) fn request_bar(&self, cx: &mut Context<Self>) -> Option<Div> {
        let dm = self.channel.as_ref().filter(|c| c.is_request())?;
        let tab = if dm.props.spam { RequestsTab::Spam } else { RequestsTab::Requests };
        let strong = palette::strong(cx);
        let label = match tab {
            RequestsTab::Requests => emphasised(&[("Message request from ".into(), false), (dm.recipient.as_ref().map_or(dm.name.clone(), |u| u.name.clone()), true)], strong),
            RequestsTab::Spam => emphasised(&[("This might be spam".into(), true)], strong),
        };
        Some(
            ui::row()
                .min_h_11()
                .py_1p5()
                .pl_3p5()
                .pr_2()
                .gap_2()
                .border_b_1()
                .border_color(palette::line(cx))
                .text_size(ui::TEXT_SM)
                .text_color(palette::muted(cx))
                .child(Icon::new(if dm.props.spam { Glyph::Shield } else { Glyph::Envelope }).size(ui::ICON_MD).text_color(palette::faint(cx)))
                .child(crate::overlays::rich_line(label).flex_1())
                .children(self.request_actions(&dm.id, tab, cx)),
        )
    }

    pub(crate) fn accept_request(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(before) = self.dm_requests.iter().find(|d| d.id == id).cloned() else { return };
        let mut accepted = before.clone();
        accepted.props.message_request = false;
        accepted.props.spam = false;
        self.upsert_dm(accepted, cx);
        // GAP: Not spam skips Discord's "Confirm this isn't spam" step and its ham report; it only accepts.
        self.request_call(Method::PUT, format!("/channels/{id}/recipients/@me"), Some(json!({ "consent_status": 2 })), before, cx);
    }

    pub(crate) fn ignore_request(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(before) = self.drop_request(id, cx) else { return };
        self.request_call(Method::DELETE, format!("/channels/{id}/recipients/@me"), None, before, cx);
    }

    // Not verified: Discord first files a FIRST_DM report on the opening message through its reporting
    // flow; that endpoint isn't traced, so only the close that follows it runs.
    pub(crate) fn report_spam(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(before) = self.drop_request(id, cx) else { return };
        self.request_call(Method::DELETE, format!("/channels/{id}"), None, before, cx);
    }

    // Off the list at once; an open request goes back to the page.
    fn drop_request(&mut self, id: &str, cx: &mut Context<Self>) -> Option<Channel> {
        let at = self.dm_requests.iter().position(|d| d.id == id)?;
        let dm = self.dm_requests.remove(at);
        if self.channel.as_ref().is_some_and(|c| c.id == id) {
            self.open_requests(if dm.props.spam { RequestsTab::Spam } else { RequestsTab::Requests }, cx);
        }
        cx.notify(); // lint-allow(global-notify): the requests count shows on the rail, in the channel list and on the page
        Some(dm)
    }

    // The call behind an optimistic change; if Discord refuses, `before` comes back with Discord's toast.
    fn request_call(&mut self, method: Method, path: String, body: Option<serde_json::Value>, before: Channel, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            if client.call(method, path, body).await.is_ok() {
                return;
            }
            this.update(cx, |this, cx| {
                let spam = before.props.spam;
                this.upsert_dm(before, cx);
                this.show_toast(if spam { SPAM_ERROR } else { REQUEST_ERROR }, cx);
            })
            .ok();
        })
        // lint-allow(detached-task): ends when Discord answers the request, and holds only a WeakEntity.
        .detach();
    }

    // Rejects a whole tab, 50 at a time and one batch after another, as Discord does. A failed batch puts it
    // and every batch after it back.
    fn clear_requests(&mut self, tab: RequestsTab, cx: &mut Context<Self>) {
        let cleared = self.requests_in(tab);
        let ids: HashSet<&str> = cleared.iter().map(|d| d.id.as_str()).collect();
        self.dm_requests.retain(|d| !ids.contains(d.id.as_str()));
        cx.notify(); // lint-allow(global-notify): the requests count shows on the rail, in the channel list and on the page
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            for (i, batch) in cleared.chunks(BATCH).enumerate() {
                let ids: Vec<&str> = batch.iter().map(|d| d.id.as_str()).collect();
                if client.call(Method::PUT, "/channels/recipients/@me/batch-reject".into(), Some(json!({ "channel_ids": ids }))).await.is_ok() {
                    continue;
                }
                let rest = cleared[i * BATCH..].to_vec();
                this.update(cx, |this, cx| {
                    for dm in rest {
                        this.upsert_dm(dm, cx);
                    }
                    this.show_toast(if tab == RequestsTab::Spam { SPAM_ERROR } else { REQUEST_ERROR }, cx);
                })
                .ok();
                return;
            }
        })
        // lint-allow(detached-task): ends after its last batch (or the first refusal), and holds only a WeakEntity.
        .detach();
    }
}

fn mutual_servers(count: usize) -> String {
    match count {
        0 => "No Mutual Servers".into(),
        1 => "1 Mutual Server".into(),
        n => format!("{n} Mutual Servers"),
    }
}

pub(crate) fn snowflake_ms(id: u64) -> u64 {
    (id >> 22) + DISCORD_EPOCH_MS
}

// The id a message sent at `ms` (Unix milliseconds) would get.
pub(crate) fn snowflake_at(ms: u64) -> u64 {
    ms.saturating_sub(DISCORD_EPOCH_MS) << 22
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutual_line_reads_like_discord() {
        assert_eq!(mutual_servers(0), "No Mutual Servers");
        assert_eq!(mutual_servers(1), "1 Mutual Server");
        assert_eq!(mutual_servers(4), "4 Mutual Servers");
    }

    #[test]
    fn snowflake_time_round_trips() {
        let ms = 1_763_800_000_000;
        assert_eq!(snowflake_ms(snowflake_at(ms)), ms);
        assert_eq!(snowflake_ms(175928847299117063), 1462015105796);
    }
}
