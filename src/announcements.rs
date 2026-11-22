// Announcement channels (docs/discord-announcements-re.md; design/explore-forums-announcements-events.html,
// #publish and #follow): Publish from the hover toolbar, the message menu and the bump under a fresh message,
// with its confirm and rate-limit alert; Follow from the header or the blocker that replaces the composer, with
// its destination dialog and success card; how a crosspost reads in the following server; the copy that warns
// before a published message is edited or deleted.
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use gpui_kit::{AnyElement, App, ClickEvent, Context, Div, FontWeight, SharedString, WeakEntity, Window, component::Icon, div, prelude::*, px};
use rand::seq::SliceRandom;
use reqwest::Method;
use serde_json::{Value, json};

use crate::{
    Waffle,
    discord::{Channel, ChannelKind, Guild, Message, SendState, perm},
    icons::Glyph,
    menu::{Entry, item},
    overlays::backdrop,
    palette::{self, Level},
    ui::{self, RADIUS_LG, RADIUS_MD, Squircle},
};

const ANNOUNCEMENT: u8 = 5;
const NEWS: &str = "NEWS";
const EPHEMERAL: u64 = 1 << 6;
// Discord's publish cap per channel, from its 429 copy.
const PUBLISH_LIMIT: usize = 10;
const HOUR: Duration = Duration::from_secs(60 * 60);
// The blocker's follower line shows from this many viewers, rounded down to the thousand.
const FOLLOWING_FLOOR: u64 = 1000;
const TOO_MANY_WEBHOOKS: u64 = 30007;
const DIALOG_WIDTH: f32 = 440.;
const FOLLOW_WIDTH: f32 = 480.;
const ADJECTIVES: [&str; 10] = ["spiffy", "good", "dapper", "swanky", "snazzy", "dandy", "cozy", "nifty", "neato", "lively"];

const EDIT_BODY: &str = "This message will also be updated in all servers following this channel. It may take some time before all servers are updated.";
const DELETE_BODY: &str = "This message will be deleted in all servers following this channel. It may take some time before it's removed from all servers.";
const SOMETHING_WRONG: &str = "Sorry, something went wrong.";
const TOO_MANY_WEBHOOKS_TEXT: &str = "The selected channel has too many webhooks. Please remove a webhook or select another channel.";
const FOLLOW_FAILED: &str = "Failed to follow the channel, please try again.";
const NO_WEBHOOK_SERVERS: &str = "Uh oh, you don't have webhook permissions in any server you're in. Ask your server admin to give you the permission or ask them to come follow themselves!";

#[derive(Default)]
pub(crate) struct Announcements {
    // Messages showing "Share with your followers!"; switching channels clears them, as Discord's store does.
    bumps: HashSet<String>,
    // ponytail: only Waffle's own publishes, per channel, so the hourly count runs low when other moderators
    // publish too; Discord exposes no remaining count.
    published: HashMap<String, Vec<Instant>>,
    // Follower stats by channel and when they were asked for; Discord refetches after an hour.
    stats: HashMap<String, (Instant, FollowerStats)>,
    dialog: Option<Dialog>,
    // (guild, channel) last followed into, the Follow dialog's default.
    last_destination: Option<(String, String)>,
}

#[derive(Clone, Copy, Default)]
struct FollowerStats {
    guilds_following: Option<u64>,
    users_seen_ever: Option<u64>,
}

enum Dialog {
    Publish { message: Box<Message>, publishing: bool },
    // "Uh-oh, a pop up box." with what went wrong.
    Alert(String),
    Follow(Box<Follow>),
    Followed(&'static str),
}

struct Follow {
    source: Channel,
    guild_id: Option<String>,
    channel_id: Option<String>,
    following: bool,
    error: Option<&'static str>,
}

// What a message row needs to offer Publish: the channel is an announcement channel in a NEWS server, and
// what you may do in it.
#[derive(Clone, Default)]
pub(crate) struct Newsroom {
    followable: bool,
    can_send: bool,
    manage: bool,
    bumps: HashSet<String>,
}

impl Newsroom {
    // Discord's `canPublish`: Send Messages, and your own message or Manage Messages.
    pub(crate) fn can_publish(&self, message: &Message, mine: bool) -> bool {
        self.followable && self.can_send && (mine || self.manage) && publishable(message)
    }

    fn shows_bump(&self, message: &Message, mine: bool) -> bool {
        self.bumps.contains(&message.id) && !published(message) && self.can_publish(message, mine)
    }
}

// Discord's publishable message: a sent DEFAULT message, not a poll, ephemeral or a forward.
pub(crate) fn publishable(message: &Message) -> bool {
    message.state == SendState::Sent && message.kind == 0 && message.poll.is_none() && message.flags & EPHEMERAL == 0 && message.forward.is_none()
}

pub(crate) fn published(message: &Message) -> bool {
    message.flags & Message::CROSSPOSTED != 0
}

pub(crate) fn is_crosspost(message: &Message) -> bool {
    message.flags & Message::IS_CROSSPOST != 0
}

fn followable(guild: &Guild, channel: &Channel) -> bool {
    channel.raw_type == ANNOUNCEMENT && guild.features.iter().any(|f| f == NEWS)
}

// Discord's 429 math for publishing: whole minutes, rounded down.
pub(crate) fn retry_minutes(body: &Value) -> u64 {
    (body["retry_after"].as_f64().unwrap_or(0.).max(0.) / 60.).floor() as u64
}

fn rate_limited(minutes: u64) -> String {
    format!("You've reached your 10 published messages per-hour limit. But we love the enthusiasm, so please try again in {}.", plural(minutes, "minute"))
}

// What deleting a message asks: a published one is deleted in every following server too.
pub(crate) fn delete_body(message: &Message) -> &'static str {
    if published(message) { DELETE_BODY } else { "Are you sure you want to delete this message?" }
}

fn plural(n: u64, noun: &str) -> String {
    format!("{} {noun}{}", thousands(n), if n == 1 { "" } else { "s" })
}

fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

// Not verified: the wire names; Discord's client reads them as `guildsFollowing` and `usersSeenEver`.
fn parse_stats(body: &Value) -> FollowerStats {
    let read = |snake: &str, camel: &str| body[snake].as_u64().or(body[camel].as_u64());
    FollowerStats { guilds_following: read("guilds_following", "guildsFollowing"), users_seen_ever: read("users_seen_ever", "usersSeenEver") }
}

// The confirm's body, with the server count in bold once it's known.
fn publish_text(guilds_following: Option<u64>) -> Vec<(String, bool)> {
    let tail = "You can edit or delete this message later, and it will update correspondingly in each following server.";
    match guilds_following {
        Some(n) => vec![("This message will be sent to ".into(), false), (plural(n, "server"), true), (format!(" following this channel. {tail}"), false)],
        None => vec![(format!("This message will be sent to all servers following this channel. {tail}"), false)],
    }
}

// "Over 2,000 other people are also following.", from 1,000 viewers on.
fn following_line(stats: FollowerStats) -> Option<String> {
    let seen = stats.users_seen_ever.filter(|n| *n >= FOLLOWING_FLOOR)?;
    Some(format!("Over {} other people are also following.", thousands(seen / 1000 * 1000)))
}

// A new server's channel: the one named general, else the first.
fn default_channel(channels: &[&Channel]) -> Option<String> {
    channels.iter().find(|c| c.name == "general").or(channels.first()).map(|c| c.id.clone())
}

fn succeeded(result: &Result<(u16, Value), String>) -> bool {
    matches!(result, Ok((status, _)) if (200..300).contains(status))
}

impl Waffle {
    fn channel_by_id(&self, id: &str) -> Option<(&Guild, &Channel)> {
        self.guilds.iter().find_map(|g| Some((g, g.channel(id)?)))
    }

    pub(crate) fn newsroom(&self, channel: Option<&Channel>) -> Newsroom {
        let Some((guild, channel)) = channel.and_then(|c| self.channel_by_id(&c.id)) else { return Newsroom::default() };
        if !followable(guild, channel) {
            return Newsroom::default();
        }
        let bits = guild.channel_permissions(self.my_id(), channel);
        Newsroom { followable: true, can_send: bits & perm::SEND_MESSAGES != 0, manage: bits & perm::MANAGE_MESSAGES != 0, bumps: self.announcements.bumps.clone() }
    }

    fn newsroom_of(&self, message: &Message) -> Newsroom {
        let channel = self.channel_by_id(&message.channel_id).map(|(_, c)| c.clone());
        self.newsroom(channel.as_ref())
    }

    // The message menu's Publish, or its disabled Published.
    pub(crate) fn publish_entry(&self, message: &Message) -> Option<Entry> {
        let mine = message.author.id == self.my_id();
        if !self.newsroom_of(message).can_publish(message, mine) {
            return None;
        }
        if published(message) {
            return Some(item("Published", |_, _, _| {}).icon(Glyph::Megaphone).disabled().entry());
        }
        let m = message.clone();
        Some(item("Publish", move |this, _, cx| this.open_publish(m.clone(), cx)).icon(Glyph::Megaphone).entry())
    }

    // MESSAGE_CREATE: the bump goes under a message you may publish, unless this channel's was turned off.
    pub(crate) fn offer_publish_bump(&mut self, message: &Message) {
        if self.prefs.publish_bump_hidden.contains(&message.channel_id) || published(message) {
            return;
        }
        let mine = message.author.id == self.my_id();
        if self.newsroom_of(message).can_publish(message, mine) {
            self.announcements.bumps.insert(message.id.clone());
        }
    }

    pub(crate) fn announcement_channel_opened(&mut self, channel: &Channel, cx: &mut Context<Self>) {
        self.announcements.bumps.clear();
        if self.newsroom(Some(channel)).followable {
            self.fetch_follower_stats(&channel.id, cx);
        }
    }

    fn fetch_follower_stats(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        if self.announcements.stats.get(channel_id).is_some_and(|(asked, _)| asked.elapsed() < HOUR) {
            return;
        }
        let Some(client) = self.client.clone() else { return };
        self.announcements.stats.insert(channel_id.to_owned(), (Instant::now(), FollowerStats::default()));
        let id = channel_id.to_owned();
        cx.spawn(async move |this, cx| {
            let Ok(body) = client.call(Method::GET, format!("/channels/{id}/follower-stats"), None).await else { return };
            this.update(cx, |this, cx| {
                this.announcements.stats.insert(id, (Instant::now(), parse_stats(&body)));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn follower_stats(&self, channel_id: &str) -> FollowerStats {
        self.announcements.stats.get(channel_id).map(|(_, stats)| *stats).unwrap_or_default()
    }

    fn published_this_hour(&self, channel_id: &str) -> usize {
        self.announcements.published.get(channel_id).map_or(0, |times| times.iter().filter(|t| t.elapsed() < HOUR).count())
    }

    pub(crate) fn open_publish(&mut self, message: Message, cx: &mut Context<Self>) {
        self.announcements.bumps.remove(&message.id);
        self.fetch_follower_stats(&message.channel_id, cx);
        self.announcements.dialog = Some(Dialog::Publish { message: Box::new(message), publishing: false });
        cx.notify();
    }

    fn dismiss_publish_bump(&mut self, message_id: &str, cx: &mut Context<Self>) {
        self.announcements.bumps.remove(message_id);
        cx.notify();
    }

    fn hide_publish_bumps(&mut self, channel_id: String, cx: &mut Context<Self>) {
        let in_channel: Vec<String> = self.feeds.get(&channel_id).map(|f| f.messages.iter().map(|m| m.id.clone()).collect()).unwrap_or_default();
        self.announcements.bumps.retain(|id| !in_channel.contains(id));
        if !self.prefs.publish_bump_hidden.contains(&channel_id) {
            self.prefs.publish_bump_hidden.push(channel_id);
            self.prefs.save();
        }
        cx.notify();
    }

    fn publish(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let Some(Dialog::Publish { message, publishing }) = &mut self.announcements.dialog else { return };
        if *publishing {
            return;
        }
        *publishing = true;
        let message = (**message).clone();
        cx.spawn(async move |this, cx| {
            let result = client.call_raw(Method::POST, format!("/channels/{}/messages/{}/crosspost", message.channel_id, message.id), None).await;
            this.update(cx, |this, cx| {
                this.settle_publish(&message, result);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    // The CROSSPOSTED flag shows at once; Discord's MESSAGE_UPDATE carries it too.
    fn settle_publish(&mut self, message: &Message, result: Result<(u16, Value), String>) {
        self.announcements.dialog = match result {
            Ok((status, _)) if (200..300).contains(&status) => {
                self.announcements.published.entry(message.channel_id.clone()).or_default().push(Instant::now());
                if let Some(m) = self.message_mut(&message.channel_id, &message.id) {
                    m.flags |= Message::CROSSPOSTED;
                }
                None
            }
            Ok((429, body)) => Some(Dialog::Alert(rate_limited(retry_minutes(&body)))),
            _ => Some(Dialog::Alert(SOMETHING_WRONG.into())),
        };
    }

    // Saving an edit to a published message asks first, in Discord's words.
    // GAP: Discord's 3-edits-an-hour alert (code 20022) isn't read; the failed edit shows Waffle's usual error.
    pub(crate) fn save_edit(&mut self, original: Message, content: String, cx: &mut Context<Self>) {
        if !published(&original) {
            return self.edit(original, content, cx);
        }
        self.confirm("Are you sure?", EDIT_BODY, "Confirm", move |this, _, cx| this.edit(original.clone(), content.clone(), cx), cx);
        if let Some(dialog) = &mut self.dialog {
            dialog.danger = false;
        }
    }

    pub(crate) fn open_follow(&mut self, source: Channel, cx: &mut Context<Self>) {
        let (guild_id, channel_id) = self.default_destination();
        self.announcements.dialog = Some(Dialog::Follow(Box::new(Follow { source, guild_id, channel_id, following: false, error: None })));
        cx.notify();
    }

    // Every server with a text channel you can manage webhooks in, and those channels.
    fn follow_destinations(&self) -> Vec<(&Guild, Vec<&Channel>)> {
        let me = self.my_id();
        self.guilds
            .iter()
            .map(|g| (g, g.channels.iter().filter(|c| c.kind == ChannelKind::Text && c.raw_type == 0 && g.can_view(me, c) && g.channel_permissions(me, c) & perm::MANAGE_WEBHOOKS != 0).collect::<Vec<_>>()))
            .filter(|(_, channels)| !channels.is_empty())
            .collect()
    }

    // The last place followed into while it still qualifies, else the first server's general.
    fn default_destination(&self) -> (Option<String>, Option<String>) {
        let destinations = self.follow_destinations();
        let still_open = self.announcements.last_destination.as_ref().filter(|(guild, channel)| destinations.iter().any(|(g, cs)| g.id == *guild && cs.iter().any(|c| c.id == *channel)));
        if let Some((guild, channel)) = still_open {
            return (Some(guild.clone()), Some(channel.clone()));
        }
        destinations.first().map_or((None, None), |(g, channels)| (Some(g.id.clone()), default_channel(channels)))
    }

    fn follow_mut(&mut self) -> Option<&mut Follow> {
        match &mut self.announcements.dialog {
            Some(Dialog::Follow(follow)) => Some(follow.as_mut()),
            _ => None,
        }
    }

    fn pick_follow_guild(&mut self, guild_id: String, cx: &mut Context<Self>) {
        let channel_id = self.follow_destinations().into_iter().find(|(g, _)| g.id == guild_id).and_then(|(_, channels)| default_channel(&channels));
        if let Some(follow) = self.follow_mut() {
            (follow.guild_id, follow.channel_id, follow.error) = (Some(guild_id), channel_id, None);
        }
        cx.notify();
    }

    fn pick_follow_channel(&mut self, channel_id: String, cx: &mut Context<Self>) {
        if let Some(follow) = self.follow_mut() {
            (follow.channel_id, follow.error) = (Some(channel_id), None);
        }
        cx.notify();
    }

    fn open_follow_guilds(&mut self, at: gpui_kit::Point<gpui_kit::Pixels>, cx: &mut Context<Self>) {
        let entries = self
            .follow_destinations()
            .into_iter()
            .map(|(g, _)| {
                let id = g.id.clone();
                item(g.name.clone(), move |this, _, cx| this.pick_follow_guild(id.clone(), cx)).server(g.icon.clone(), g.name.clone()).entry()
            })
            .collect();
        self.open_menu(at, entries, cx);
    }

    fn open_follow_channels(&mut self, at: gpui_kit::Point<gpui_kit::Pixels>, cx: &mut Context<Self>) {
        let Some(guild_id) = self.follow_mut().and_then(|f| f.guild_id.clone()) else { return };
        let entries = self
            .follow_destinations()
            .into_iter()
            .filter(|(g, _)| g.id == guild_id)
            .flat_map(|(_, channels)| channels)
            .map(|c| {
                let id = c.id.clone();
                item(c.name.clone(), move |this, _, cx| this.pick_follow_channel(id.clone(), cx)).icon(Glyph::Hashtag).entry()
            })
            .collect();
        self.open_menu(at, entries, cx);
    }

    fn follow(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let Some(follow) = self.follow_mut() else { return };
        let (Some(guild_id), Some(channel_id)) = (follow.guild_id.clone(), follow.channel_id.clone()) else { return };
        if follow.following {
            return;
        }
        (follow.following, follow.error) = (true, None);
        let source = follow.source.id.clone();
        cx.spawn(async move |this, cx| {
            let result = client.call_raw(Method::POST, format!("/channels/{source}/followers"), Some(json!({ "webhook_channel_id": channel_id }))).await;
            this.update(cx, |this, cx| {
                this.settle_follow(guild_id, channel_id, result);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn settle_follow(&mut self, guild_id: String, channel_id: String, result: Result<(u16, Value), String>) {
        if succeeded(&result) {
            self.announcements.last_destination = Some((guild_id, channel_id));
            let adjective = ADJECTIVES.choose(&mut rand::thread_rng()).copied().unwrap_or("snazzy");
            self.announcements.dialog = Some(Dialog::Followed(adjective));
            return;
        }
        let too_many = matches!(&result, Ok((_, body)) if body["code"].as_u64() == Some(TOO_MANY_WEBHOOKS));
        if let Some(follow) = self.follow_mut() {
            (follow.following, follow.error) = (false, Some(if too_many { TOO_MANY_WEBHOOKS_TEXT } else { FOLLOW_FAILED }));
        }
    }

    #[cfg(debug_assertions)]
    pub(crate) fn demo_announcement_stats(&mut self, channel_id: &str, publishes: usize) {
        self.announcements.stats.insert(channel_id.into(), (Instant::now(), FollowerStats { guilds_following: Some(12), users_seen_ever: Some(2_480) }));
        self.announcements.published.insert(channel_id.into(), vec![Instant::now(); publishes]);
    }

    #[cfg(debug_assertions)]
    pub(crate) fn demo_publish_limit(&mut self, retry_after: f64) {
        self.announcements.dialog = Some(Dialog::Alert(rate_limited(retry_minutes(&json!({ "retry_after": retry_after })))));
    }

    pub(crate) fn dismiss_announcement_dialog(&mut self) -> bool {
        self.announcements.dialog.take().is_some()
    }

    fn close_announcement_dialog(&mut self, cx: &mut Context<Self>) {
        self.announcements.dialog = None;
        cx.notify();
    }

    // The header's Follow, on announcement channels in NEWS servers; members see it too.
    pub(crate) fn follow_button(&self, channel: &Channel, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.newsroom(Some(channel)).followable {
            return None;
        }
        let source = channel.clone();
        Some(
            ui::button("follow-button", ui::Tone::Secondary, cx)
                .small()
                .occlude()
                .child(Icon::new(Glyph::Plus).size(px(16.)))
                .child("Follow")
                .on_click(cx.listener(move |this, _, _, cx| this.open_follow(source.clone(), cx)))
                .into_any_element(),
        )
    }

    // In place of the composer for someone who can't post here: Follow, and how many already do.
    // GAP: Discord adds Join Server for someone previewing the server; Waffle has no preview.
    pub(crate) fn follow_blocker(&self, channel: &Channel, cx: &mut Context<Self>) -> Option<AnyElement> {
        let newsroom = self.newsroom(Some(channel));
        if !newsroom.followable || newsroom.can_send {
            return None;
        }
        let source = channel.clone();
        let text = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child("Follow to get this channel's updates in your own server."))
            .children(following_line(self.follower_stats(&channel.id)).map(|line| div().text_size(px(12.)).text_color(palette::muted(cx)).child(line)));
        let card = div()
            .id("follow-blocker")
            .occlude()
            .mx(px(16.))
            .min_h(px(56.))
            .py(px(10.))
            .pl(px(16.))
            .pr(px(10.))
            .flex()
            .items_center()
            .gap(px(12.))
            .squircle(RADIUS_LG)
            .bg(palette::surface(Level::Raised, cx))
            .border_1()
            .border_color(palette::line(cx))
            .text_size(px(14.))
            .child(Icon::new(Glyph::Megaphone).size(px(20.)).text_color(palette::muted(cx)))
            .child(text)
            .child(ui::button("follow-blocker-button", ui::Tone::Primary, cx).child("Follow").on_click(cx.listener(move |this, _, _, cx| this.open_follow(source.clone(), cx))));
        Some(div().absolute().left_0().right_0().bottom_0().pb(px(8.)).child(card).into_any_element())
    }

    pub(crate) fn announcement_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.announcements.dialog.as_ref()?;
        let (width, body) = match dialog {
            Dialog::Publish { message, publishing } => (DIALOG_WIDTH, self.publish_dialog(message, *publishing, cx)),
            Dialog::Alert(text) => (DIALOG_WIDTH, alert_dialog(text.clone(), cx)),
            Dialog::Follow(follow) => (FOLLOW_WIDTH, self.follow_dialog(follow, cx)),
            Dialog::Followed(adjective) => (DIALOG_WIDTH, followed_dialog(adjective, cx)),
        };
        Some(ui::modal(backdrop("announcement-backdrop", cx), ui::dialog("announcement-dialog", width, body, window, cx), cx))
    }

    fn publish_dialog(&self, message: &Message, publishing: bool, cx: &mut Context<Self>) -> Div {
        let text = publish_text(self.follower_stats(&message.channel_id).guilds_following);
        ui::dialog_body()
            .child(dialog_title("Are you sure?", cx))
            .child(div().line_height(px(20.)).text_color(palette::muted(cx)).child(crate::chat::emphasised(&text, palette::strong(cx))))
            .child(quota(self.published_this_hour(&message.channel_id), cx))
            .child(
                footer()
                    .child(self.cancel_button("publish-cancel", cx))
                    .child(ui::button("publish-confirm", ui::Tone::Primary, cx).loading(publishing.then_some("Publishing…")).child("Confirm").on_click(cx.listener(|this, _, _, cx| this.publish(cx)))),
            )
    }

    fn cancel_button(&self, id: &'static str, cx: &mut Context<Self>) -> ui::Control {
        ui::button(id, ui::Tone::Secondary, cx).child("Cancel").on_click(cx.listener(|this, _, _, cx| this.close_announcement_dialog(cx)))
    }

    fn follow_dialog(&self, follow: &Follow, cx: &mut Context<Self>) -> Div {
        let destinations = self.follow_destinations();
        let guild = destinations.iter().find(|(g, _)| follow.guild_id.as_ref() == Some(&g.id));
        let channel = guild.and_then(|(_, channels)| channels.iter().find(|c| follow.channel_id.as_ref() == Some(&c.id)));
        let source_guild = self.channel_by_id(&follow.source.id).map(|(g, _)| g);
        let head = div()
            .flex()
            .items_start()
            .gap(px(12.))
            .child(dialog_title("Add this channel's updates to your server!", cx).flex_1().min_w_0())
            .child(ui::icon_button("follow-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.close_announcement_dialog(cx))));
        let source = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .py(px(10.))
            .px(px(12.))
            .squircle(RADIUS_MD)
            .bg(palette::surface(Level::Page, cx))
            .children(source_guild.map(|g| ui::server_tile(g.icon.as_deref(), &g.name, 32., cx)))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .children(source_guild.map(|g| div().truncate().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(g.name.clone())))
                    .child(div().flex().items_center().gap(px(4.)).text_size(px(12.)).text_color(palette::muted(cx)).child(Icon::new(Glyph::Megaphone).size(px(14.))).child(follow.source.name.clone())),
            );
        let intro = div()
            .text_color(palette::muted(cx))
            .child("Select where you want these updates to go. You can remove this at any time in Server Settings > Integrations > Channels Followed.")
            .when(follow.source.props.nsfw, |d| d.child(div().text_color(palette::danger_ink(cx)).child("Note: This is an age-restricted channel.")));
        let server_select = ui::select("follow-server", None, guild.map_or(SharedString::from("Select a server"), |(g, _)| g.name.clone().into()), false, cx)
            .disabled(destinations.is_empty())
            .on_click(cx.listener(|this, e: &ClickEvent, _, cx| this.open_follow_guilds(e.position(), cx)));
        let channel_select = ui::select("follow-channel", Some(Glyph::Hashtag), channel.map_or(SharedString::from("Select a channel"), |c| c.name.clone().into()), false, cx)
            .disabled(guild.is_none())
            .on_click(cx.listener(|this, e: &ClickEvent, _, cx| this.open_follow_channels(e.position(), cx)));
        let hint = match follow.error {
            Some(error) => ui::field_error(error, cx),
            None if destinations.is_empty() => div().text_size(px(12.)).text_color(palette::danger_ink(cx)).child(NO_WEBHOOK_SERVERS),
            None => ui::field_hint("Only servers and channels you have webhook permissions in are shown.", None, cx),
        };
        let ready = channel.is_some();
        ui::dialog_body()
            .child(head)
            .child(source)
            .child(intro)
            .child(ui::field("Send to", cx).child(server_select))
            .child(ui::field("Select channel", cx).child(channel_select).child(hint))
            .child(
                footer().child(self.cancel_button("follow-cancel", cx)).child(
                    ui::button("follow-confirm", ui::Tone::Primary, cx)
                        .disabled(!ready)
                        .loading(follow.following.then_some("Following…"))
                        .child("Follow")
                        .on_click(cx.listener(|this, _, _, cx| this.follow(cx))),
                ),
            )
    }
}

fn dialog_title(text: impl Into<SharedString>, cx: &App) -> Div {
    div().text_size(px(20.)).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(text.into())
}

fn footer() -> Div {
    div().flex().justify_end().gap(px(8.))
}

// Better than Discord: how many of the hour's ten Waffle has used here.
fn quota(used: usize, cx: &App) -> Div {
    let (on, off) = (palette::muted(cx), palette::line_strong(cx));
    let pips = (0..PUBLISH_LIMIT).map(|i| div().w(px(10.)).h(px(4.)).rounded(px(2.)).bg(if i < used { on } else { off }));
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(px(12.))
        .text_color(palette::muted(cx))
        .child(div().flex().gap(px(3.)).children(pips))
        .child(format!("{} of {PUBLISH_LIMIT} published this hour", used.min(PUBLISH_LIMIT)))
}

fn alert_dialog(text: String, cx: &mut Context<Waffle>) -> Div {
    ui::dialog_body()
        .child(dialog_title("Uh-oh, a pop up box.", cx))
        .child(div().text_color(palette::muted(cx)).child(text))
        .child(footer().child(ui::button("publish-alert-okay", ui::Tone::Primary, cx).child("Okay").on_click(cx.listener(|this, _, _, cx| this.close_announcement_dialog(cx)))))
}

fn followed_dialog(adjective: &str, cx: &mut Context<Waffle>) -> Div {
    ui::dialog_body()
        .items_center()
        .text_center()
        .child(crate::mascot::mascot_at("followed-mascot", crate::mascot::Mood::Happy, 0.8, cx))
        .child(dialog_title(format!("What a nice addition to your server. Lookin' real {adjective} in here."), cx))
        .child(div().text_color(palette::muted(cx)).child("The most important updates from this channel will now show up directly in your server. Enjoy!"))
        .child(ui::button("followed-okay", ui::Tone::Primary, cx).child("Got it!").on_click(cx.listener(|this, _, _, cx| this.close_announcement_dialog(cx))))
}

// The hover toolbar's Publish; once published it reads "Published" and is disabled.
pub(crate) fn publish_button(message: &Message, mine: bool, newsroom: &Newsroom, this: WeakEntity<Waffle>, cx: &App) -> Option<AnyElement> {
    if !newsroom.can_publish(message, mine) {
        return None;
    }
    let done = published(message);
    let msg = message.clone();
    Some(
        ui::icon_button(SharedString::from(format!("publish-{}", message.id)), Glyph::Megaphone, if done { "Published" } else { "Publish" }, cx)
            .disabled(done)
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                this.update(cx, |this, cx| this.open_publish(msg.clone(), cx)).ok();
            })
            .into_any_element(),
    )
}

// "Share with your followers!" under a message just sent: Publish, dismiss, and Don't show again for the channel.
pub(crate) fn publish_bump(message: &Message, mine: bool, newsroom: &Newsroom, this: WeakEntity<Waffle>, cx: &App) -> Option<AnyElement> {
    if !newsroom.shows_bump(message, mine) {
        return None;
    }
    let (publish, dismiss, hide) = (this.clone(), this.clone(), this);
    let (msg, id, channel_id) = (message.clone(), message.id.clone(), message.channel_id.clone());
    let strip = div()
        .mt(px(6.))
        .max_w(px(520.))
        .py(px(8.))
        .pl(px(12.))
        .pr(px(10.))
        .flex()
        .items_center()
        .gap(px(8.))
        .squircle(RADIUS_MD)
        .bg(palette::selected(cx))
        .text_size(px(14.))
        .text_color(palette::text(cx))
        .child(Icon::new(Glyph::Megaphone).size(px(16.)).text_color(palette::muted(cx)))
        .child(div().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child("Share with your followers!"))
        .child(ui::button(SharedString::from(format!("bump-publish-{}", message.id)), ui::Tone::Link, cx).child("Publish").on_click(move |_, _, cx| {
            cx.stop_propagation();
            publish.update(cx, |this, cx| this.open_publish(msg.clone(), cx)).ok();
        }))
        .child(div().flex_1())
        .child(ui::icon_button(SharedString::from(format!("bump-dismiss-{}", message.id)), Glyph::X, "Dismiss", cx).small().on_click(move |_, _, cx| {
            cx.stop_propagation();
            dismiss.update(cx, |this, cx| this.dismiss_publish_bump(&id, cx)).ok();
        }));
    let faint = palette::faint(cx);
    let never = div()
        .id(SharedString::from(format!("bump-hide-{}", message.id)))
        .mt(px(4.))
        .w_auto()
        .text_size(px(12.))
        .text_color(faint)
        .cursor_pointer()
        .hover(|s| s.underline())
        .child("Don't show again")
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            hide.update(cx, |this, cx| this.hide_publish_bumps(channel_id.clone(), cx)).ok();
        });
    Some(div().flex().flex_col().items_start().child(strip).child(never).into_any_element())
}

// A crosspost's author tag, where a bot's reads APP.
// GAP: Discord gives its own community-updates server OFFICIAL, and clicking the author opens the source
// server's card; Waffle opens the webhook's profile.
pub(crate) fn server_tag(cx: &App) -> Div {
    ui::tag("Server", cx)
}

pub(crate) fn source_deleted(cx: &App) -> AnyElement {
    div().italic().text_color(palette::muted(cx)).child("[Original Message Deleted]").into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_after_rounds_down_to_minutes() {
        assert_eq!(retry_minutes(&json!({ "retry_after": 1415.3 })), 23);
        assert_eq!(retry_minutes(&json!({ "retry_after": 59.9 })), 0);
        assert_eq!(retry_minutes(&json!({})), 0);
        assert!(rate_limited(23).ends_with("try again in 23 minutes."));
        assert!(rate_limited(1).ends_with("try again in 1 minute."));
    }

    #[test]
    fn publish_needs_a_plain_message_and_the_right_to_send() {
        let newsroom = Newsroom { followable: true, can_send: true, manage: false, bumps: HashSet::new() };
        let plain = Message { kind: 0, ..Default::default() };
        assert!(newsroom.can_publish(&plain, true));
        assert!(!newsroom.can_publish(&plain, false), "someone else's needs Manage Messages");
        assert!(Newsroom { manage: true, ..newsroom.clone() }.can_publish(&plain, false));
        assert!(!Newsroom { can_send: false, manage: true, ..newsroom.clone() }.can_publish(&plain, true));
        assert!(!Newsroom::default().can_publish(&plain, true), "not an announcement channel in a NEWS server");
        assert!(!newsroom.can_publish(&Message { kind: 19, ..Default::default() }, true), "replies aren't DEFAULT");
        assert!(!newsroom.can_publish(&Message { flags: EPHEMERAL, ..Default::default() }, true));
        assert!(!newsroom.can_publish(&Message { forward: Some(Box::default()), ..Default::default() }, true));
        assert!(!newsroom.can_publish(&Message { state: SendState::Sending, ..Default::default() }, true));
    }

    #[test]
    fn follower_counts_read_like_discord() {
        assert_eq!(following_line(FollowerStats { users_seen_ever: Some(2_480), ..Default::default() }).as_deref(), Some("Over 2,000 other people are also following."));
        assert_eq!(following_line(FollowerStats { users_seen_ever: Some(999), ..Default::default() }), None);
        assert_eq!(publish_text(Some(1))[1].0, "1 server");
        assert_eq!(publish_text(Some(12))[1].0, "12 servers");
    }
}
