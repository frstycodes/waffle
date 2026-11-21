// What the composer becomes when you can't send (design/explore-chat-guard.html): a locked card in place of
// the input row, and slowmode's chip, countdown and queued send. The draft is never touched, so it comes
// back when the guard lifts.
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use chrono::{DateTime, Local, Utc};
use gpui_kit::{AnyElement, Context, Div, FontWeight, HighlightStyle, Hsla, SharedString, Stateful, StyledText, Task, component::{ActiveTheme, Icon, tooltip::Tooltip}, div, prelude::*, px};

use crate::{
    Waffle,
    discord::{Channel, ChannelKind, Guild, Relation, Relationship, perm},
    icons::Glyph,
    palette,
    ui::{self, Tone},
};

// Why you can't send in the open channel, in precedence order.
#[derive(Clone, Debug, PartialEq)]
pub enum Guard {
    Blocked { user_id: String, name: String },
    TimedOut(DateTime<Utc>),
    // `roles`: (name, colour) of up to every role that may post, highest first.
    NoPermission { channel: String, roles: Vec<(String, u32)> },
}

pub fn guard(channel: &Channel, guild: Option<&Guild>, me: &str, relationships: &[Relationship], now: DateTime<Utc>) -> Option<Guard> {
    let Some(guild) = guild else {
        let user = channel.recipient.as_ref()?;
        let blocked = relationships.iter().any(|r| r.kind == Relation::Blocked && r.user.id == user.id);
        return blocked.then(|| Guard::Blocked { user_id: user.id.clone(), name: user.name.clone() });
    };
    if let Some(until) = guild.timed_out_until.filter(|until| *until > now) {
        return Some(Guard::TimedOut(until));
    }
    if guild.channel_permissions(me, channel) & crate::discord::send_permission(channel) != 0 {
        return None;
    }
    let roles = guild.posting_roles(channel).into_iter().map(|r| (r.name.clone(), r.color)).collect();
    Some(Guard::NoPermission { channel: channel.name.clone(), roles })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Slowmode {
    Interval(u32),
    Immune(u32),
    Counting { interval: u32, left: Duration },
}

const IMMUNE: u64 = perm::MANAGE_CHANNELS | perm::MANAGE_MESSAGES | perm::BYPASS_SLOWMODE;

// `ready_at`: when our last send here lets us send again.
pub fn slowmode(channel: &Channel, guild: &Guild, me: &str, ready_at: Option<Instant>, now: Instant) -> Option<Slowmode> {
    let interval = channel.props.slowmode;
    if interval == 0 || !matches!(channel.kind, ChannelKind::Text | ChannelKind::Thread) {
        return None;
    }
    if guild.channel_permissions(me, channel) & IMMUNE != 0 {
        return Some(Slowmode::Immune(interval));
    }
    match ready_at.map(|at| at.saturating_duration_since(now)).filter(|left| !left.is_zero()) {
        Some(left) => Some(Slowmode::Counting { interval, left }),
        None => Some(Slowmode::Interval(interval)),
    }
}

// Slowmode countdowns and the one queued send, with the tasks that move them along.
#[derive(Default)]
pub struct GuardClock {
    ready_at: HashMap<String, Instant>,
    // The channel whose draft sends itself when its countdown ends.
    queued: Option<String>,
    ticking: bool,
    timeout_wake: Option<(DateTime<Utc>, Task<()>)>,
}

impl Waffle {
    pub(crate) fn composer_guard(&self) -> Option<Guard> {
        let (channel, me) = (self.channel.as_ref()?, self.me.as_ref()?);
        guard(channel, self.guild(), &me.id, &self.relationships, Utc::now())
    }

    fn open_slowmode(&self) -> Option<Slowmode> {
        let (channel, me, guild) = (self.channel.as_ref()?, self.me.as_ref()?, self.guild()?);
        slowmode(channel, guild, &me.id, self.guard.ready_at.get(&channel.id).copied(), Instant::now())
    }

    // GAP: a refusal (HTTP 429, code 20016) can't restart the countdown from its `retry_after`: `discord::send`
    // waits out short ones itself and reports longer ones as text only.
    pub(crate) fn slowmode_sent(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        let Some(me) = self.me.as_ref() else { return };
        let Some((guild, channel)) = self.guilds.iter().find_map(|g| Some((g, g.channel(channel_id)?))) else { return };
        let Some(Slowmode::Interval(interval)) = slowmode(channel, guild, &me.id, None, Instant::now()) else { return };
        self.start_countdown(channel_id, Duration::from_secs(interval.into()), cx);
    }

    pub(crate) fn start_countdown(&mut self, channel_id: &str, left: Duration, cx: &mut Context<Self>) {
        self.guard.ready_at.insert(channel_id.to_owned(), Instant::now() + left);
        self.start_slowmode_ticker(cx);
    }

    // Enter while the countdown runs: the draft stays and sends itself when it ends.
    pub(crate) fn hold_for_slowmode(&mut self, channel_id: &str, cx: &mut Context<Self>) -> bool {
        if !matches!(self.open_slowmode(), Some(Slowmode::Counting { .. })) {
            return false;
        }
        self.guard.queued = Some(channel_id.to_owned());
        cx.notify();
        true
    }

    fn start_slowmode_ticker(&mut self, cx: &mut Context<Self>) {
        if self.guard.ticking {
            return;
        }
        self.guard.ticking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if !this.update(cx, |this, cx| this.tick_slowmode(cx)).unwrap_or(false) {
                    break;
                }
            }
        })
        .detach();
    }

    // One second on: ended countdowns go, a queued draft whose countdown ended sends. False stops the ticker.
    fn tick_slowmode(&mut self, cx: &mut Context<Self>) -> bool {
        let now = Instant::now();
        self.guard.ready_at.retain(|_, at| *at > now);
        let due = self.guard.queued.take_if(|id| !self.guard.ready_at.contains_key(id));
        let open = self.channel.as_ref().map(|c| c.id.clone());
        if due.is_some() && due == open && self.editing.is_none() {
            let (this, main) = (cx.weak_entity(), self.main_window);
            // The main window's root is this view, so it can only be borrowed once this update ends.
            cx.defer(move |cx| {
                main.update(cx, |_, window, cx| this.update(cx, |this, cx| this.submit(window, cx))).ok();
            });
        }
        cx.notify();
        self.guard.ticking = !self.guard.ready_at.is_empty();
        self.guard.ticking
    }

    fn cancel_queued(&mut self, cx: &mut Context<Self>) {
        self.guard.queued = None;
        cx.notify();
    }

    // Redraws as the minutes left change, and once more when the timeout ends, which lifts the guard.
    fn wake_for_timeout(&mut self, until: DateTime<Utc>, cx: &mut Context<Self>) {
        if self.guard.timeout_wake.as_ref().is_some_and(|(at, _)| *at == until) {
            return;
        }
        let task = cx.spawn(async move |this, cx| {
            loop {
                let left = (until - Utc::now()).to_std().unwrap_or_default();
                let step = match left.as_millis() % 60_000 {
                    0 => Duration::from_secs(60),
                    ms => Duration::from_millis(ms as u64),
                };
                cx.background_executor().timer(step.min(left)).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() || left <= step {
                    break;
                }
            }
        });
        self.guard.timeout_wake = Some((until, task));
    }

    // The locked card's row, in place of the input row; None while you can send.
    pub(crate) fn locked_row(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let guard = self.composer_guard()?;
        let (strong, muted) = (palette::strong(cx), palette::muted(cx));
        let (glyph, title, detail, action): (Glyph, String, StyledText, Option<AnyElement>) = match guard {
            Guard::Blocked { user_id, name } => {
                let unblock = ui::button("guard-unblock", Tone::Secondary, cx).small().child("Unblock").on_click(cx.listener(move |this, _, _, cx| this.drop_relation(&user_id, cx)));
                (Glyph::Prohibit, format!("You blocked {name}"), StyledText::new("Neither of you can message the other."), Some(unblock.into_any_element()))
            }
            Guard::TimedOut(until) => {
                self.wake_for_timeout(until, cx);
                let left = (until - Utc::now()).to_std().unwrap_or_default();
                let detail = format!("{}. You can still read along, and your draft is kept.", time_left(left));
                (Glyph::Timer, format!("You're timed out until {}", until_label(until.with_timezone(&Local), Local::now())), StyledText::new(detail), None)
            }
            Guard::NoPermission { channel, roles } => {
                let dark = cx.theme().is_dark();
                let parts = posting_parts(&roles);
                (Glyph::HashtagLock, format!("You can't send messages in #{channel}"), role_line(&parts, |color| role_color(color, strong, dark)), None)
            }
        };
        let row = div()
            .min_h(px(48.))
            .pl(px(14.))
            .pr(px(8.))
            .flex()
            .items_center()
            .gap(px(10.))
            .text_color(muted)
            .child(div().flex_none().child(Icon::new(glyph).size(px(20.))))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .py(px(9.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(strong).child(title))
                    .child(div().text_size(px(12.)).line_height(px(17.)).child(detail)),
            )
            .children(action);
        Some(row.into_any_element())
    }

    // The faint chip before the pickers: the interval, the countdown after a send, or Immune.
    pub(crate) fn slowmode_chip(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let state = self.open_slowmode()?;
        let ready_at = self.channel.as_ref().and_then(|c| self.guard.ready_at.get(&c.id).copied());
        let (ink, label): (Hsla, SharedString) = match state {
            Slowmode::Interval(interval) => (palette::faint(cx), interval_label(interval).into()),
            Slowmode::Immune(_) => (palette::faint(cx), "Immune".into()),
            Slowmode::Counting { left, .. } => (palette::muted(cx), crate::call::clock(ceil_secs(left)).into()),
        };
        let chip = div()
            .id("slowmode")
            .flex_none()
            .h(px(32.))
            .px(px(6.))
            .flex()
            .items_center()
            .gap(px(4.))
            .whitespace_nowrap()
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .font_features(crate::call::tabular())
            .text_color(ink)
            .child(Icon::new(Glyph::Timer).size(px(16.)))
            .child(label)
            .tooltip(move |window, cx| {
                Tooltip::element(move |_, cx| {
                    let left = ready_at.map(|at| at.saturating_duration_since(Instant::now()));
                    let (title, detail) = slowmode_tip(state, left);
                    div().flex().flex_col().child(title).child(div().text_size(px(12.)).text_color(palette::muted(cx)).child(detail))
                })
                .build(window, cx)
            });
        Some(chip)
    }

    // "Sends when slowmode ends, in 1:58", in the reply bar's slot; X cancels and keeps the draft.
    pub(crate) fn queued_bar(&self, cx: &mut Context<Self>) -> Option<Div> {
        let channel = self.channel.as_ref()?;
        if self.guard.queued.as_ref() != Some(&channel.id) {
            return None;
        }
        let Some(Slowmode::Counting { left, .. }) = self.open_slowmode() else { return None };
        let accent = palette::accent_ink(cx);
        let clock = crate::call::clock(ceil_secs(left));
        let label = StyledText::new(format!("Sends when slowmode ends, in {clock}")).with_highlights([(
            "Sends when slowmode ends, in ".len().."Sends when slowmode ends, in ".len() + clock.len(),
            HighlightStyle { color: Some(palette::strong(cx)), font_weight: Some(FontWeight::SEMIBOLD), ..Default::default() },
        )]);
        let bar = crate::chat::bar_row(cx)
            .child(div().flex_none().text_color(accent).child(Icon::new(Glyph::Clock).size(px(16.))))
            .child(div().flex_1().min_w_0().truncate().font_features(crate::call::tabular()).child(label))
            .child(ui::icon_button("cancel-queued", Glyph::X, "Cancel", cx).small().on_click(cx.listener(|this, _, _, cx| this.cancel_queued(cx))));
        Some(bar)
    }
}

fn ceil_secs(left: Duration) -> u64 {
    left.as_millis().div_ceil(1000) as u64
}

fn plural(n: u64, unit: &str) -> String {
    match n {
        1 => format!("1 {unit}"),
        n => format!("{n} {unit}s"),
    }
}

// The chip's interval: "30s", "2m", "1h", "1h 30m".
pub fn interval_label(secs: u32) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    let parts = [(h, "h"), (m, "m"), (s, "s")];
    parts.iter().filter(|(n, _)| *n > 0).map(|(n, unit)| format!("{n}{unit}")).collect::<Vec<_>>().join(" ")
}

// "One message every 2 minutes", "every minute", "every 90 seconds".
pub fn interval_words(secs: u32) -> String {
    let (n, unit) = match secs {
        s if s % 3600 == 0 => (s / 3600, "hour"),
        s if s % 60 == 0 => (s / 60, "minute"),
        s => (s, "second"),
    };
    match n {
        1 => format!("One message every {unit}"),
        n => format!("One message every {n} {unit}s"),
    }
}

fn slowmode_tip(state: Slowmode, left: Option<Duration>) -> (String, String) {
    match state {
        Slowmode::Interval(interval) => ("Slowmode is on".into(), interval_words(interval)),
        Slowmode::Immune(interval) => ("Slowmode is on, but you're immune".into(), interval_words(interval)),
        Slowmode::Counting { interval, left: at_render } => {
            let left = ceil_secs(left.unwrap_or(at_render));
            (format!("You can send again in {}", crate::call::clock(left)), format!("Slowmode: {}", interval_words(interval).to_lowercase()))
        }
    }
}

// "52 minutes left", rounded up so it never reads 0 while the timeout runs.
pub fn time_left(left: Duration) -> String {
    let minutes = left.as_secs().div_ceil(60).max(1);
    match minutes {
        m if m < 60 => format!("{} left", plural(m, "minute")),
        m if m < 48 * 60 => format!("{} left", plural(m.div_ceil(60), "hour")),
        m => format!("{} left", plural(m.div_ceil(24 * 60), "day")),
    }
}

// "10:41 PM" today, "Nov 24 at 10:41 PM" on another day.
fn until_label(until: DateTime<Local>, now: DateTime<Local>) -> String {
    let time = crate::chat::clock(&until.to_rfc3339());
    match until.date_naive() == now.date_naive() {
        true => time,
        false => format!("{} at {time}", until.format("%b %-d")),
    }
}

// The no-permission line as (text, role colour) runs: "Only " Moderators " and " Organisers " can post here.",
// up to two roles named, then "and N more".
pub fn posting_parts(roles: &[(String, u32)]) -> Vec<(String, Option<u32>)> {
    let text = |s: &str| (s.to_owned(), None);
    let role = |(name, color): &(String, u32)| (name.clone(), Some(*color));
    let mut parts = vec![text("Only ")];
    match roles {
        [] => return vec![text("Only some members can post here.")],
        [one] => parts.push(role(one)),
        [a, b] => parts.extend([role(a), text(" and "), role(b)]),
        [a, b, rest @ ..] => parts.extend([role(a), text(", "), role(b), text(&format!(" and {} more", rest.len()))]),
    }
    parts.push(text(" can post here."));
    parts
}

fn role_color(color: u32, plain: Hsla, dark: bool) -> Hsla {
    match color {
        0 => plain,
        c => palette::role_ink(c, dark),
    }
}

fn role_line(parts: &[(String, Option<u32>)], ink: impl Fn(u32) -> Hsla) -> StyledText {
    let mut text = String::new();
    let mut runs = Vec::new();
    for (part, color) in parts {
        let start = text.len();
        text.push_str(part);
        if let Some(color) = color {
            runs.push((start..text.len(), HighlightStyle { color: Some(ink(*color)), font_weight: Some(FontWeight::SEMIBOLD), ..Default::default() }));
        }
    }
    StyledText::new(text).with_highlights(runs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discord::{Overwrite, Role, User, VIEW_CHANNEL};

    fn role(id: &str, position: i64, permissions: u64) -> Role {
        Role { id: id.into(), name: id.into(), color: 0, position, permissions, managed: false }
    }

    fn text_channel(slowmode: u32, overwrites: Vec<Overwrite>) -> Channel {
        let mut channel = crate::discord::parse_channel(&serde_json::json!({ "id": "c", "name": "general", "type": 0, "rate_limit_per_user": slowmode }), "g").unwrap();
        channel.overwrites = overwrites;
        channel
    }

    fn guild(everyone: u64, roles: Vec<Role>, mine: &[&str], channel: Channel) -> Guild {
        let mut all = vec![role("g", 0, everyone)];
        all.extend(roles);
        Guild {
            id: "g".into(),
            name: "g".into(),
            icon: None,
            channels: vec![channel],
            roles: all,
            voice_states: Vec::new(),
            emojis: Vec::new(),
            stickers: Vec::new(),
            owner_id: Some("owner".into()),
            default_notifications: 0,
            threads: Vec::new(),
            my_roles: mine.iter().map(|r| r.to_string()).collect(),
            premium_tier: 0,
            afk_channel_id: None,
            timed_out_until: None,
        }
    }

    fn deny_everyone_send() -> Overwrite {
        Overwrite { id: "g".into(), allow: 0, deny: perm::SEND_MESSAGES, member: false }
    }

    #[test]
    fn guard_order_is_blocked_then_timeout_then_permission() {
        let now = Utc::now();
        let mira = User { id: "mira".into(), name: "Mira".into(), ..Default::default() };
        let mut dm = text_channel(0, Vec::new());
        dm.recipient = Some(mira.clone());
        let blocked = [Relationship { user: mira, kind: Relation::Blocked }];
        assert_eq!(guard(&dm, None, "me", &blocked, now), Some(Guard::Blocked { user_id: "mira".into(), name: "Mira".into() }));
        assert_eq!(guard(&dm, None, "me", &[], now), None);

        let channel = text_channel(0, vec![deny_everyone_send()]);
        let mut server = guild(VIEW_CHANNEL | perm::SEND_MESSAGES, vec![role("mods", 2, perm::MANAGE_MESSAGES)], &[], channel);
        server.channels[0].overwrites.push(Overwrite { id: "mods".into(), allow: perm::SEND_MESSAGES, deny: 0, member: false });
        let open = server.channels[0].clone();
        assert_eq!(guard(&open, Some(&server), "me", &[], now), Some(Guard::NoPermission { channel: "general".into(), roles: vec![("mods".into(), 0)] }));

        let until = now + chrono::Duration::minutes(52);
        server.timed_out_until = Some(until);
        assert_eq!(guard(&open, Some(&server), "me", &[], now), Some(Guard::TimedOut(until)), "a timeout outranks the missing permission");
        server.timed_out_until = Some(now - chrono::Duration::minutes(1));
        assert!(matches!(guard(&open, Some(&server), "me", &[], now), Some(Guard::NoPermission { .. })), "a past timeout no longer counts");

        let free = guild(VIEW_CHANNEL | perm::SEND_MESSAGES, Vec::new(), &[], text_channel(0, Vec::new()));
        assert_eq!(guard(&free.channels[0], Some(&free), "me", &[], now), None);
    }

    #[test]
    fn posting_roles_follow_overwrites_highest_first() {
        let mut bot = role("bot", 9, perm::SEND_MESSAGES);
        bot.managed = true;
        let roles = vec![role("helpers", 1, 0), role("admins", 5, perm::ADMINISTRATOR), role("mods", 3, 0), role("muted", 4, perm::SEND_MESSAGES), bot];
        let overwrites = vec![
            deny_everyone_send(),
            Overwrite { id: "mods".into(), allow: perm::SEND_MESSAGES, deny: 0, member: false },
            Overwrite { id: "helpers".into(), allow: perm::SEND_MESSAGES, deny: 0, member: false },
            Overwrite { id: "someone".into(), allow: perm::SEND_MESSAGES, deny: 0, member: true },
        ];
        let server = guild(VIEW_CHANNEL | perm::SEND_MESSAGES, roles, &[], text_channel(0, overwrites));
        let names: Vec<&str> = server.posting_roles(&server.channels[0]).iter().map(|r| r.id.as_str()).collect();
        assert_eq!(names, ["admins", "mods", "helpers"], "muted's own Send loses to the @everyone deny; bots are left out");
    }

    #[test]
    fn posting_line_names_two_roles_then_counts() {
        let roles = |n: usize| (0..n).map(|i| (format!("R{i}"), 0)).collect::<Vec<_>>();
        let line = |n: usize| posting_parts(&roles(n)).into_iter().map(|(t, _)| t).collect::<String>();
        assert_eq!(line(0), "Only some members can post here.");
        assert_eq!(line(1), "Only R0 can post here.");
        assert_eq!(line(2), "Only R0 and R1 can post here.");
        assert_eq!(line(5), "Only R0, R1 and 3 more can post here.");
    }

    #[test]
    fn slowmode_states() {
        let now = Instant::now();
        let channel = text_channel(120, Vec::new());
        let member = guild(VIEW_CHANNEL | perm::SEND_MESSAGES, vec![role("mods", 1, perm::MANAGE_MESSAGES), role("bypass", 1, perm::BYPASS_SLOWMODE)], &[], channel.clone());
        assert_eq!(slowmode(&channel, &member, "me", None, now), Some(Slowmode::Interval(120)));
        assert_eq!(slowmode(&channel, &member, "me", Some(now + Duration::from_secs(90)), now), Some(Slowmode::Counting { interval: 120, left: Duration::from_secs(90) }));
        assert_eq!(slowmode(&channel, &member, "me", Some(now), now), Some(Slowmode::Interval(120)), "an ended countdown is idle");
        for mine in ["mods", "bypass"] {
            let immune = Guild { my_roles: vec![mine.into()], ..member.clone() };
            assert_eq!(slowmode(&channel, &immune, "me", Some(now + Duration::from_secs(90)), now), Some(Slowmode::Immune(120)));
        }
        assert_eq!(slowmode(&text_channel(0, Vec::new()), &member, "me", None, now), None);
    }

    #[test]
    fn words_for_time() {
        assert_eq!(interval_label(30), "30s");
        assert_eq!(interval_label(120), "2m");
        assert_eq!(interval_label(3600), "1h");
        assert_eq!(interval_label(5400), "1h 30m");
        assert_eq!(interval_words(120), "One message every 2 minutes");
        assert_eq!(interval_words(60), "One message every minute");
        assert_eq!(interval_words(90), "One message every 90 seconds");
        assert_eq!(time_left(Duration::from_secs(52 * 60 - 20)), "52 minutes left");
        assert_eq!(time_left(Duration::from_secs(10)), "1 minute left");
        assert_eq!(time_left(Duration::from_secs(3 * 3600)), "3 hours left");
        assert_eq!(time_left(Duration::from_secs(7 * 86400)), "7 days left");
    }
}
