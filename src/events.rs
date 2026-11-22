// Guild scheduled events (docs/discord-events-re.md): the sidebar's Events row and live bar, the events
// list with its cards (design/explore-forums-announcements-events.html `.ev2-*`), the event menu,
// Interested, the details dialog, the create/edit wizard, Start, End and Cancel. Behaviour and copy are
// Discord's; the look is Waffle's.
use std::fmt::Display;

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike, Utc};
use gpui_kit::{
    AnyElement, AppContext as _, Context, Entity, Focusable, FontWeight, Hsla, InteractiveElement, IntoElement, MouseButton, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window,
    component::{
        Icon,
        input::{Input, InputEvent, InputState, Textarea, TextareaState},
    },
    div,
    prelude::FluentBuilder as _,
    px,
};
use reqwest::Method;
use serde_json::{Value, json};

use crate::{
    Waffle,
    discord::{self, Guild, ScheduledEvent, User, perm},
    icons::Glyph,
    mascot::Mood,
    menu::{Entry, item, submenu},
    palette,
    ui::{self, Squircle as _, Tone},
};

// Discord's card shows Start this close to the start time (`withinStartWindow`).
const START_WINDOW_MINUTES: i64 = 15;
// Inside this, the status line counts down (`upcomingEvent`).
const SOON_MINUTES: i64 = 60;
const TOPIC_MAX: usize = 100;
const LOCATION_MAX: usize = 100;
const DESCRIPTION_MAX: usize = 1000;
const LIST_WIDTH: f32 = 560.;
const DETAILS_WIDTH: f32 = 600.;
const WIZARD_WIDTH: f32 = 480.;
const PRIVATE: u8 = 2;
const STAGE_TYPE: u8 = 13;

// The layers this feature stacks: the list, an event's details over it, the wizard, the Start dialog.
#[derive(Default)]
pub(crate) struct Events {
    list: Option<String>,
    details: Option<Details>,
    wizard: Option<Wizard>,
    start: Option<Start>,
}

struct Details {
    guild_id: String,
    event_id: String,
    tab: usize,
    users: Option<Result<Vec<User>, String>>,
}

struct Start {
    guild_id: String,
    event_id: String,
    notify: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Step {
    Location,
    Info,
    Review,
    Success,
}

struct Wizard {
    guild_id: String,
    editing: Option<String>,
    // The event has started: its location type is fixed.
    started: bool,
    step: Step,
    entity_type: u8,
    channel_id: Option<String>,
    frequency: Frequency,
    location: Entity<InputState>,
    topic: Entity<InputState>,
    start_date: Entity<InputState>,
    start_time: Entity<InputState>,
    end_date: Entity<InputState>,
    end_time: Entity<InputState>,
    description: Entity<TextareaState>,
    link: Option<String>,
    busy: bool,
}

// What the card's status line says (`103355`): its state, the coloured lead and the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Scheduled,
    Soon,
    Live,
    Ended,
    Canceled,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct When {
    pub phase: Phase,
    pub lead: Option<String>,
    pub rest: Option<String>,
}

// Discord's recurrence options (`974930.Xx`), in its order. Every weekend is staff-only and left out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Frequency {
    Once,
    Weekly,
    Biweekly,
    Monthly,
    Yearly,
    Daily,
    Weekdays,
}

#[derive(Clone, Copy)]
enum Calendar {
    Google,
    Ics,
    Outlook,
}

#[derive(Default)]
struct Problems {
    location: Option<&'static str>,
    topic: Option<&'static str>,
    start: Option<&'static str>,
    end: Option<&'static str>,
    // Over the limit: the field's count turns red, and Next waits.
    description: bool,
}

impl Problems {
    fn step_ok(&self, step: Step) -> bool {
        match step {
            Step::Location => self.location.is_none(),
            _ => self.location.is_none() && self.topic.is_none() && self.start.is_none() && self.end.is_none() && !self.description,
        }
    }
}

// Derivation

pub(crate) fn when<Tz: TimeZone>(event: &ScheduledEvent, now: &DateTime<Tz>) -> When
where
    Tz::Offset: Display,
{
    let tz = now.timezone();
    let Some(start) = parse_time(&event.start).map(|s| s.with_timezone(&tz)) else { return When { phase: Phase::Scheduled, lead: None, rest: None } };
    let end = event.end.as_deref().and_then(parse_time).map(|e| e.with_timezone(&tz));
    match event.status {
        ScheduledEvent::CANCELED => When { phase: Phase::Canceled, lead: Some("Canceled".into()), rest: Some(date_line(&start, now)) },
        ScheduledEvent::COMPLETED => When { phase: Phase::Ended, lead: Some("Ended".into()), rest: Some(date_line(&start, now)) },
        ScheduledEvent::ACTIVE => {
            let lead = if event.entity_type == ScheduledEvent::EXTERNAL { "Happening Now" } else { "Live Now" };
            let ends = end.map(|end| if end.date_naive() == now.date_naive() { format!("Ends {}", clock(&end)) } else { format!("Ends {}", date_line(&end, now)) });
            When { phase: Phase::Live, lead: Some(lead.into()), rest: ends }
        }
        _ => {
            let seconds = (start.clone() - now.clone()).num_seconds();
            if seconds > SOON_MINUTES * 60 {
                return When { phase: Phase::Scheduled, lead: None, rest: Some(range_line(&start, end.as_ref(), now)) };
            }
            let minutes = (seconds + 59).div_euclid(60);
            let lead = if minutes > 0 { format!("Starting in {minutes}m") } else { "Starting soon".into() };
            When { phase: Phase::Soon, lead: Some(lead), rest: None }
        }
    }
}

fn parse_time(iso: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(iso).ok().map(|t| t.with_timezone(&Utc))
}

// moment's `LT`: "7:00 PM".
fn clock<Tz: TimeZone>(t: &DateTime<Tz>) -> String
where
    Tz::Offset: Display,
{
    t.format("%-I:%M %p").to_string()
}

// Discord's date line (`974930.CC`): "Today at" / "Tomorrow at" inside a day, else "Sat Nov 28th · 7:00 PM",
// with the year when it isn't this one.
fn date_line<Tz: TimeZone>(t: &DateTime<Tz>, now: &DateTime<Tz>) -> String
where
    Tz::Offset: Display,
{
    let ahead = t.clone() - now.clone();
    let (day, today) = (t.date_naive(), now.date_naive());
    if day == today {
        return format!("Today at {}", clock(t));
    }
    if ahead < Duration::days(1) && today.succ_opt() == Some(day) {
        return format!("Tomorrow at {}", clock(t));
    }
    let year = if day.year() != today.year() { format!(", {}", day.year()) } else { String::new() };
    format!("{} {}{year} · {}", t.format("%a %b"), ordinal(day.day()), clock(t))
}

// "{start} — {end}", the end as a time alone on the same day.
fn range_line<Tz: TimeZone>(start: &DateTime<Tz>, end: Option<&DateTime<Tz>>, now: &DateTime<Tz>) -> String
where
    Tz::Offset: Display,
{
    let from = date_line(start, now);
    match end {
        Some(end) if end.date_naive() == start.date_naive() => format!("{from} — {}", clock(end)),
        Some(end) => format!("{from} — {}", date_line(end, now)),
        None => from,
    }
}

fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

fn phase_ink(phase: Phase, cx: &gpui_kit::App) -> Hsla {
    match phase {
        Phase::Live => palette::positive_ink(cx),
        Phase::Soon => palette::accent_ink(cx),
        Phase::Canceled => palette::danger_ink(cx),
        Phase::Scheduled | Phase::Ended => palette::muted(cx),
    }
}

pub(crate) fn can_create(guild: &Guild, me: &str) -> bool {
    guild.can(me, perm::CREATE_EVENTS)
}

// `931991._`: Manage Events covers every event; Create Events covers your own.
pub(crate) fn can_manage(guild: &Guild, me: &str, event: &ScheduledEvent) -> bool {
    manages(guild.permissions(me), me, event)
}

fn manages(perms: u64, me: &str, event: &ScheduledEvent) -> bool {
    perms & perm::MANAGE_EVENTS != 0 || (perms & perm::CREATE_EVENTS != 0 && event.creator_id.as_deref() == Some(me))
}

fn within_start_window(event: &ScheduledEvent, now: DateTime<Utc>) -> bool {
    parse_time(&event.start).is_some_and(|s| s <= now + Duration::minutes(START_WINDOW_MINUTES))
}

pub(crate) fn event_link(event: &ScheduledEvent) -> String {
    format!("https://discord.com/events/{}/{}", event.guild_id, event.id)
}

// The list's events, in the store's order: live first, then by start, then by id. Discord lists only
// upcoming ones; canceled occurrences stay, drawn as canceled. Events in channels you can't see are hidden.
fn listed<'a>(guild: &'a Guild, me: &str) -> Vec<&'a ScheduledEvent> {
    let mut events: Vec<&ScheduledEvent> = guild
        .events
        .iter()
        .filter(|e| e.status != ScheduledEvent::COMPLETED)
        .filter(|e| e.channel_id.as_deref().and_then(|c| guild.channel(c)).is_none_or(|c| guild.can_view(me, c)))
        .collect();
    events.sort_by_key(|e| (e.status != ScheduledEvent::ACTIVE, parse_time(&e.start), e.id.clone()));
    events
}

fn upcoming_count(guild: &Guild, me: &str) -> usize {
    listed(guild, me).iter().filter(|e| e.upcoming()).count()
}

fn events_title(count: usize) -> String {
    match count {
        0 => "Events".into(),
        1 => "1 Event".into(),
        n => format!("{n} Events"),
    }
}

impl Frequency {
    const ALL: [Frequency; 7] = [Frequency::Once, Frequency::Weekly, Frequency::Biweekly, Frequency::Monthly, Frequency::Yearly, Frequency::Daily, Frequency::Weekdays];

    // Weekdays only for a start on a weekday.
    fn options(start: NaiveDate) -> Vec<Frequency> {
        let weekday = start.weekday().num_days_from_monday() < 5;
        Self::ALL.into_iter().filter(|f| *f != Frequency::Weekdays || weekday).collect()
    }

    fn label(self, start: NaiveDate) -> String {
        let day = start.format("%A");
        match self {
            Frequency::Once => "Does not repeat".into(),
            Frequency::Weekly => format!("Weekly on {day}"),
            Frequency::Biweekly => format!("Every other {day}"),
            Frequency::Monthly => format!("Monthly on the {} {day}", nth_word(start.day().div_ceil(7))),
            Frequency::Yearly => format!("Annually on {}", start.format("%b %-d")),
            Frequency::Daily => "Every day".into(),
            Frequency::Weekdays => "Every weekday (Monday to Friday)".into(),
        }
    }

    // Back from Discord's rule, by its rrule.js frequency (YEARLY 0, MONTHLY 1, WEEKLY 2, DAILY 3).
    fn of_rule(rule: &Value) -> Frequency {
        match (rule["frequency"].as_u64(), rule["interval"].as_u64().unwrap_or(1)) {
            (Some(0), _) => Frequency::Yearly,
            (Some(1), _) => Frequency::Monthly,
            (Some(2), 2) => Frequency::Biweekly,
            (Some(2), _) => Frequency::Weekly,
            (Some(3), _) if rule["by_weekday"].as_array().is_some_and(|d| d.len() == 5) => Frequency::Weekdays,
            (Some(3), _) => Frequency::Daily,
            _ => Frequency::Once,
        }
    }

    // Discord's `recurrence_rule` (`794782`). Weekday numbers are rrule.js's (MO 0 … SU 6).
    // Not verified: which day the server reads `by_weekday` / `by_n_weekday` in. They're taken from the UTC
    // start, and weekdays-only shifts by the UTC day offset as `974930` does.
    fn rule(self, start: DateTime<Utc>, local_day: NaiveDate) -> Option<Value> {
        let day = start.weekday().num_days_from_monday();
        let shift = (start.date_naive() - local_day).num_days();
        let (frequency, interval, by_weekday, by_n_weekday, by_month, by_month_day) = match self {
            Frequency::Once => return None,
            Frequency::Weekly => (2, 1, json!([day]), Value::Null, Value::Null, Value::Null),
            Frequency::Biweekly => (2, 2, json!([day]), Value::Null, Value::Null, Value::Null),
            Frequency::Monthly => (1, 1, Value::Null, json!([{ "n": start.day().div_ceil(7), "day": day }]), Value::Null, Value::Null),
            Frequency::Yearly => (0, 1, Value::Null, Value::Null, json!([start.month()]), json!([start.day()])),
            Frequency::Daily => (3, 1, Value::Null, Value::Null, Value::Null, Value::Null),
            Frequency::Weekdays => (3, 1, json!((0..5).map(|d: i64| (d + shift).rem_euclid(7)).collect::<Vec<_>>()), Value::Null, Value::Null, Value::Null),
        };
        Some(json!({
            "start": start.to_rfc3339(),
            "end": null,
            "frequency": frequency,
            "interval": interval,
            "by_weekday": by_weekday,
            "by_n_weekday": by_n_weekday,
            "by_month": by_month,
            "by_month_day": by_month_day,
            "by_year_day": null,
            "count": null,
        }))
    }
}

fn nth_word(n: u32) -> &'static str {
    ["first", "second", "third", "fourth", "fifth"].get(n.saturating_sub(1) as usize).copied().unwrap_or("last")
}

fn repeat_label(event: &ScheduledEvent) -> Option<String> {
    let rule = event.recurrence.as_ref()?;
    let start = parse_time(&event.start)?.with_timezone(&Local).date_naive();
    Some(Frequency::of_rule(rule)).filter(|f| *f != Frequency::Once).map(|f| f.label(start))
}

// Calendar export

const RRULE_DAYS: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];

fn rrule(rule: &Value) -> Option<String> {
    let freq = ["YEARLY", "MONTHLY", "WEEKLY", "DAILY"].get(rule["frequency"].as_u64()? as usize)?;
    let mut parts = vec![format!("FREQ={freq}"), format!("INTERVAL={}", rule["interval"].as_u64().unwrap_or(1))];
    let day = |d: &Value| d.as_u64().and_then(|d| RRULE_DAYS.get(d as usize)).copied();
    let days: Vec<String> = rule["by_weekday"].as_array().into_iter().flatten().filter_map(|d| day(d).map(str::to_owned)).collect();
    let nth: Vec<String> = rule["by_n_weekday"].as_array().into_iter().flatten().filter_map(|d| Some(format!("{}{}", d["n"].as_i64()?, day(&d["day"])?))).collect();
    let byday = if nth.is_empty() { days } else { nth };
    if !byday.is_empty() {
        parts.push(format!("BYDAY={}", byday.join(",")));
    }
    let list = |key: &str| rule[key].as_array().map(|v| v.iter().filter_map(Value::as_u64).map(|n| n.to_string()).collect::<Vec<_>>().join(",")).filter(|s| !s.is_empty());
    if let Some(months) = list("by_month") {
        parts.push(format!("BYMONTH={months}"));
    }
    if let Some(days) = list("by_month_day") {
        parts.push(format!("BYMONTHDAY={days}"));
    }
    Some(parts.join(";"))
}

fn ics_stamp(t: DateTime<Utc>) -> String {
    t.format("%Y%m%dT%H%M%SZ").to_string()
}

fn ics_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace("\r\n", "\\n").replace('\n', "\\n")
}

// RFC 5545 folds content lines at 75 octets, continuing with a space.
fn ics_fold(line: &str) -> String {
    let mut out = String::new();
    let mut width = 0;
    for c in line.chars() {
        if width + c.len_utf8() > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += c.len_utf8();
    }
    out
}

// An event without an end lasts an hour, as Discord's calendar links have it.
fn span(event: &ScheduledEvent) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let start = parse_time(&event.start)?;
    let end = event.end.as_deref().and_then(parse_time).unwrap_or(start + Duration::hours(1));
    Some((start, end))
}

pub(crate) fn ics(event: &ScheduledEvent, place: &str, now: DateTime<Utc>) -> Option<String> {
    let (start, end) = span(event)?;
    let link = event_link(event);
    let description = match &event.description {
        Some(d) => format!("{d}\n\n{link}"),
        None => link.clone(),
    };
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_owned(),
        "VERSION:2.0".to_owned(),
        "PRODID:-//Waffle//Scheduled events//EN".to_owned(),
        "BEGIN:VEVENT".to_owned(),
        format!("UID:{}@discord.com", event.id),
        format!("DTSTAMP:{}", ics_stamp(now)),
        format!("DTSTART:{}", ics_stamp(start)),
        format!("DTEND:{}", ics_stamp(end)),
        format!("SUMMARY:{}", ics_escape(&event.name)),
        format!("DESCRIPTION:{}", ics_escape(&description)),
        format!("LOCATION:{}", ics_escape(place)),
        format!("URL:{link}"),
    ];
    lines.extend(event.recurrence.as_ref().and_then(rrule).map(|r| format!("RRULE:{r}")));
    lines.extend(["END:VEVENT".to_owned(), "END:VCALENDAR".to_owned()]);
    Some(lines.iter().map(|l| ics_fold(l)).collect::<Vec<_>>().join("\r\n") + "\r\n")
}

fn calendar_url(kind: Calendar, event: &ScheduledEvent, place: &str) -> Option<String> {
    let (start, end) = span(event)?;
    let details = event.description.clone().map_or_else(|| event_link(event), |d| format!("{d}\n\n{}", event_link(event)));
    let url = match kind {
        Calendar::Google => {
            let mut params = vec![("action", "TEMPLATE".to_owned()), ("text", event.name.clone()), ("dates", format!("{}/{}", ics_stamp(start), ics_stamp(end))), ("details", details), ("location", place.to_owned())];
            params.extend(event.recurrence.as_ref().and_then(rrule).map(|r| ("recur", format!("RRULE:{r}"))));
            reqwest::Url::parse_with_params("https://calendar.google.com/calendar/render", &params)
        }
        Calendar::Outlook => {
            let params = [
                ("path", "/calendar/action/compose".to_owned()),
                ("rru", "addevent".to_owned()),
                ("subject", event.name.clone()),
                ("startdt", start.to_rfc3339()),
                ("enddt", end.to_rfc3339()),
                ("body", details),
                ("location", place.to_owned()),
            ];
            reqwest::Url::parse_with_params("https://outlook.live.com/calendar/0/deeplink/compose", &params)
        }
        Calendar::Ics => return None,
    };
    url.ok().map(String::from)
}

fn ics_file_name(name: &str) -> String {
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
    let safe = safe.trim();
    format!("{}.ics", if safe.is_empty() { "event" } else { safe })
}

// Wizard dates

// GAP: Waffle has no date or time picker; the wizard takes "2026-11-28" and "7:00 PM" (or "19:00") as text.
const DATE_FORMAT: &str = "%Y-%m-%d";

fn parse_clock(text: &str) -> Option<NaiveTime> {
    let text = text.trim().to_uppercase().replace(' ', "");
    let (body, meridiem) = match (text.strip_suffix("AM"), text.strip_suffix("PM")) {
        (Some(body), _) => (body, "AM"),
        (_, Some(body)) => (body, "PM"),
        _ => return NaiveTime::parse_from_str(&text, "%H:%M").ok(),
    };
    // chrono won't read an hour without its minutes: "7 PM" is "7:00 PM".
    let body = if body.contains(':') { body.to_owned() } else { format!("{body}:00") };
    NaiveTime::parse_from_str(&format!("{body}{meridiem}"), "%I:%M%p").ok()
}

fn parse_local(date: &str, time: &str) -> Option<DateTime<Utc>> {
    let day = NaiveDate::parse_from_str(date.trim(), DATE_FORMAT).ok()?;
    let at = NaiveDateTime::new(day, parse_clock(time)?);
    Local.from_local_datetime(&at).earliest().map(|t| t.with_timezone(&Utc))
}

// `jd`: the next whole hour at least an hour away.
fn default_start(now: DateTime<Local>) -> DateTime<Local> {
    let later = now + Duration::hours(1);
    let hour = later.with_minute(0).and_then(|t| t.with_second(0)).and_then(|t| t.with_nanosecond(0)).unwrap_or(later);
    if hour < later { hour + Duration::hours(1) } else { hour }
}

impl Waffle {
    fn scheduled_event(&self, guild_id: &str, event_id: &str) -> Option<&ScheduledEvent> {
        self.guilds.iter().find(|g| g.id == guild_id)?.events.iter().find(|e| e.id == event_id)
    }

    fn scheduled_event_mut(&mut self, guild_id: &str, event_id: &str) -> Option<&mut ScheduledEvent> {
        self.guilds.iter_mut().find(|g| g.id == guild_id)?.events.iter_mut().find(|e| e.id == event_id)
    }

    fn event_guild(&self, guild_id: &str) -> Option<&Guild> {
        self.guilds.iter().find(|g| g.id == guild_id)
    }

    fn may_manage(&self, event: &ScheduledEvent) -> bool {
        self.event_guild(&event.guild_id).is_some_and(|g| can_manage(g, self.my_id(), event))
    }

    // Where it happens, as words: the channel's name or the location.
    fn event_place(&self, event: &ScheduledEvent) -> String {
        match event.entity_type {
            ScheduledEvent::EXTERNAL => event.location.clone().unwrap_or_default(),
            _ => event.channel_id.as_deref().and_then(|c| self.event_guild(&event.guild_id)?.channel(c)).map(|c| c.name.clone()).unwrap_or_default(),
        }
    }

    fn event_creator(&self, event: &ScheduledEvent) -> Option<User> {
        let id = event.creator_id.as_deref()?;
        event.creator.clone().or_else(|| self.members.get(&event.guild_id)?.get(id).cloned())
    }

    // A GET whose body the caller wants; a failure comes back as the error text.
    fn fetch_json(&mut self, path: String, then: impl FnOnce(&mut Waffle, Result<Value, String>, &mut Context<Waffle>) + 'static, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            let result = client.call(Method::GET, path, None).await;
            this.update(cx, |this, cx| {
                then(this, result, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn open_events(&mut self, guild_id: String, cx: &mut Context<Self>) {
        self.menu = None;
        self.events.list = Some(guild_id.clone());
        self.fetch_event_counts(&guild_id, cx);
        cx.notify();
    }

    // On open, as Discord's list does: each event's Interested count, then which of them you marked.
    fn fetch_event_counts(&mut self, guild_id: &str, cx: &mut Context<Self>) {
        let ids: Vec<String> = self.event_guild(guild_id).map(|g| g.events.iter().filter(|e| e.upcoming()).map(|e| e.id.clone()).collect()).unwrap_or_default();
        for id in ids {
            let (g, e) = (guild_id.to_owned(), id.clone());
            self.fetch_json(
                format!("/guilds/{guild_id}/scheduled-events/{id}/users/counts"),
                move |this, result, _| {
                    let Some(count) = result.ok().and_then(|v| v["guild_scheduled_event_count"].as_u64()) else { return };
                    if let Some(event) = this.scheduled_event_mut(&g, &e) {
                        event.interested = count as u32;
                    }
                },
                cx,
            );
        }
        let g = guild_id.to_owned();
        // Not verified: the response is a list of `{guild_scheduled_event_id, response, …}` for this server.
        self.fetch_json(
            format!("/users/@me/scheduled-events?guild_ids={guild_id}"),
            move |this, result, _| {
                let Ok(list) = result else { return };
                let mine: Vec<String> = list.as_array().into_iter().flatten().filter(|r| r["response"].as_u64().unwrap_or(1) == 1).filter_map(|r| r["guild_scheduled_event_id"].as_str().map(str::to_owned)).collect();
                let Some(guild) = this.guilds.iter_mut().find(|x| x.id == g) else { return };
                for event in &mut guild.events {
                    event.me_interested = mine.contains(&event.id);
                }
            },
            cx,
        );
    }

    fn open_event_details(&mut self, guild_id: String, event_id: String, cx: &mut Context<Self>) {
        self.menu = None;
        self.events.details = Some(Details { guild_id, event_id, tab: 0, users: None });
        cx.notify();
    }

    fn set_details_tab(&mut self, tab: usize, cx: &mut Context<Self>) {
        let Some(details) = self.events.details.as_mut() else { return };
        details.tab = tab;
        let load = tab == 1 && details.users.is_none();
        let path = format!("/guilds/{}/scheduled-events/{}/users?limit=100&with_member=true", details.guild_id, details.event_id);
        let (guild_id, event_id) = (details.guild_id.clone(), details.event_id.clone());
        cx.notify();
        if !load {
            return;
        }
        self.fetch_json(
            path,
            move |this, result, _| {
                let Some(details) = this.events.details.as_mut().filter(|d| d.event_id == event_id) else { return };
                let users = result.map(|list| list.as_array().into_iter().flatten().map(|r| discord::parse_member_user(&r["member"], &r["user"], Some(&guild_id))).collect());
                details.users = Some(users);
            },
            cx,
        );
    }

    fn set_interested(&mut self, guild_id: &str, event_id: &str, on: bool) {
        let Some(event) = self.scheduled_event_mut(guild_id, event_id).filter(|e| e.me_interested != on) else { return };
        event.me_interested = on;
        event.interested = if on { event.interested + 1 } else { event.interested.saturating_sub(1) };
    }

    fn toggle_interested(&mut self, guild_id: String, event_id: String, cx: &mut Context<Self>) {
        let Some(on) = self.scheduled_event(&guild_id, &event_id).map(|e| e.me_interested) else { return };
        let path = format!("/guilds/{guild_id}/scheduled-events/{event_id}/users/@me");
        let (method, body) = if on { (Method::DELETE, None) } else { (Method::PUT, Some(json!({ "response": 1 }))) };
        let (g, e) = (guild_id.clone(), event_id.clone());
        self.rest_optimistic(method, path, body, move |this| this.set_interested(&guild_id, &event_id, !on), move |this| this.set_interested(&g, &e, on), cx);
    }

    fn event_menu(&self, guild_id: &str, event_id: &str) -> Vec<Entry> {
        let Some(event) = self.scheduled_event(guild_id, event_id) else { return Vec::new() };
        let manage = self.may_manage(event);
        let (active, canceled) = (event.status == ScheduledEvent::ACTIVE, event.status == ScheduledEvent::CANCELED);
        let ids = || (guild_id.to_owned(), event_id.to_owned());
        let mut out = Vec::new();
        if !active {
            let calendar = |label: &'static str, glyph: Glyph, kind: Calendar| {
                let (g, e) = ids();
                item(label, move |this, _, cx| this.add_to_calendar(&g, &e, kind, cx)).icon(glyph).entry()
            };
            out.push(submenu("Add to Calendar", vec![calendar("Google", Glyph::Open, Calendar::Google), calendar("Download ICS", Glyph::Download, Calendar::Ics), calendar("Outlook", Glyph::Open, Calendar::Outlook)]).icon(Glyph::CalendarAdd).entry());
        }
        if manage && !active && !canceled {
            let (g, e) = ids();
            out.push(item("Start Event", move |this, _, cx| this.open_start_event(g.clone(), e.clone(), cx)).icon(Glyph::Play).entry());
        }
        if manage && !canceled {
            let (g, e) = ids();
            out.push(item("Edit Event", move |this, window, cx| this.open_event_wizard(g.clone(), Some(e.clone()), window, cx)).icon(Glyph::Pen).entry());
        }
        let link = event_link(event);
        out.push(item("Copy Event Link", move |this, _, cx| this.copy_event_link(link.clone(), cx)).icon(Glyph::Link).entry());
        if manage && !active && !canceled {
            let (g, e) = ids();
            out.push(Entry::Separator);
            out.push(item("Cancel Event", move |this, _, cx| this.confirm_cancel_event(g.clone(), e.clone(), cx)).danger().icon(Glyph::CalendarX).entry());
        }
        out
    }

    // The server menu's "Create Event" (`499817`), after Create Channel and Create Category.
    pub(crate) fn create_event_entry(&self, guild_id: &str) -> Option<Entry> {
        let guild = self.event_guild(guild_id).filter(|g| can_create(g, self.my_id()))?;
        let gid = guild.id.clone();
        Some(item("Create Event", move |this, window, cx| this.open_event_wizard(gid.clone(), None, window, cx)).icon(Glyph::CalendarAdd).entry())
    }

    fn open_event_menu(&mut self, guild_id: String, event_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let at = window.mouse_position();
        self.open_live_menu(at, move |this| this.event_menu(&guild_id, &event_id), cx);
    }

    fn copy_event_link(&mut self, link: String, cx: &mut Context<Self>) {
        self.copy_text(link, cx);
        self.show_toast("Link Copied!", cx);
    }

    fn add_to_calendar(&mut self, guild_id: &str, event_id: &str, kind: Calendar, cx: &mut Context<Self>) {
        let Some(event) = self.scheduled_event(guild_id, event_id).cloned() else { return };
        let place = self.event_place(&event);
        if let Some(url) = calendar_url(kind, &event, &place) {
            return cx.open_url(&url);
        }
        let Some(text) = ics(&event, &place, Utc::now()) else { return };
        // Windows sets USERPROFILE, not HOME.
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
        let Some(path) = home.map(|h| std::path::PathBuf::from(h).join("Downloads").join(ics_file_name(&event.name))) else { return };
        match std::fs::write(&path, text) {
            Ok(()) => self.show_toast(format!("Saved {} to Downloads", ics_file_name(&event.name)), cx),
            Err(e) => self.show_toast(format!("Couldn't save the event: {e}"), cx),
        }
    }

    fn open_start_event(&mut self, guild_id: String, event_id: String, cx: &mut Context<Self>) {
        self.menu = None;
        self.events.start = Some(Start { guild_id, event_id, notify: true });
        cx.notify();
    }

    // Stage events open the stage; the rest switch to live (`163765`).
    // GAP: Discord creates a missing stage channel first and then joins the event's channel; neither happens here.
    fn start_event(&mut self, cx: &mut Context<Self>) {
        let Some(start) = self.events.start.take() else { return };
        let Some(event) = self.scheduled_event(&start.guild_id, &start.event_id).cloned() else { return };
        let path = format!("/guilds/{}/scheduled-events/{}", event.guild_id, event.id);
        let (g, e) = (event.guild_id.clone(), event.id.clone());
        let live = move |this: &mut Waffle, _: &mut Context<Waffle>| {
            if let Some(event) = this.scheduled_event_mut(&g, &e) {
                event.status = ScheduledEvent::ACTIVE;
            }
        };
        if event.entity_type == ScheduledEvent::STAGE {
            let body = json!({ "channel_id": event.channel_id, "topic": event.name, "privacy_level": PRIVATE, "guild_scheduled_event_id": event.id, "send_start_notification": start.notify });
            return self.rest(Method::POST, "/stage-instances".into(), Some(body), live, cx);
        }
        self.rest(Method::PATCH, path, Some(json!({ "status": ScheduledEvent::ACTIVE })), live, cx);
    }

    fn confirm_end_event(&mut self, guild_id: String, event_id: String, cx: &mut Context<Self>) {
        self.confirm("End Event", "Are you sure you want to end the event?", "Yes, end the event", move |this, _, cx| {
            let (g, e) = (guild_id.clone(), event_id.clone());
            let path = format!("/guilds/{guild_id}/scheduled-events/{event_id}");
            this.rest(
                Method::PATCH,
                path,
                Some(json!({ "status": ScheduledEvent::COMPLETED })),
                move |this, _| {
                    if let Some(event) = this.scheduled_event_mut(&g, &e) {
                        event.status = ScheduledEvent::COMPLETED;
                    }
                },
                cx,
            );
        }, cx);
    }

    // Discord's Cancel Event deletes the event rather than setting status 4.
    fn confirm_cancel_event(&mut self, guild_id: String, event_id: String, cx: &mut Context<Self>) {
        self.confirm_delete("Cancel Event?", "Are you sure you want to cancel this event?", "Cancel Event", move |this, _, cx| this.cancel_event(&guild_id, &event_id, cx), cx);
        self.set_dialog_cancel("Never Mind");
    }

    fn cancel_event(&mut self, guild_id: &str, event_id: &str, cx: &mut Context<Self>) {
        let Some(event) = self.scheduled_event(guild_id, event_id).cloned() else { return };
        if self.events.details.as_ref().is_some_and(|d| d.event_id == event_id) {
            self.events.details = None;
        }
        let (g, e) = (guild_id.to_owned(), event_id.to_owned());
        let path = format!("/guilds/{guild_id}/scheduled-events/{event_id}");
        self.rest_optimistic(
            Method::DELETE,
            path,
            None,
            move |this| {
                if let Some(guild) = this.guilds.iter_mut().find(|x| x.id == g) {
                    guild.events.retain(|x| x.id != e);
                }
            },
            move |this| {
                if let Some(guild) = this.guilds.iter_mut().find(|x| x.id == event.guild_id) {
                    guild.events.push(event);
                }
            },
            cx,
        );
    }

    // Escape and outside clicks close the topmost layer only.
    pub(crate) fn close_events(&mut self) -> bool {
        let layers = &mut self.events;
        layers.start.take().is_some() || layers.wizard.take().is_some() || layers.details.take().is_some() || layers.list.take().is_some()
    }

    fn close_event_layer(&mut self, cx: &mut Context<Self>) {
        self.close_events();
        cx.notify();
    }
}

// The wizard
impl Waffle {
    pub(crate) fn open_event_wizard(&mut self, guild_id: String, editing: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        let event = editing.as_deref().and_then(|id| self.scheduled_event(&guild_id, id)).cloned();
        let local = |iso: &str| parse_time(iso).map(|t| t.with_timezone(&Local));
        let start = event.as_ref().and_then(|e| local(&e.start)).unwrap_or_else(|| default_start(Local::now()));
        let end = event.as_ref().and_then(|e| e.end.as_deref()).and_then(local).unwrap_or(start + Duration::hours(1));
        let input = |value: String, placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            let state = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
            state.update(cx, |s, cx| s.set_value(value, window, cx));
            cx.subscribe(&state, |_, _, _: &InputEvent, cx| cx.notify()).detach();
            state
        };
        let day = |t: DateTime<Local>| t.format(DATE_FORMAT).to_string();
        let location = input(event.as_ref().and_then(|e| e.location.clone()).unwrap_or_default(), "Add a location, link, or something.", window, cx);
        let topic = input(event.as_ref().map(|e| e.name.clone()).unwrap_or_default(), "What's your event?", window, cx);
        let (start_date, start_time) = (input(day(start), "2026-11-28", window, cx), input(clock(&start), "7:00 PM", window, cx));
        let (end_date, end_time) = (input(day(end), "2026-11-28", window, cx), input(clock(&end), "8:00 PM", window, cx));
        let description = cx.new(|cx| TextareaState::new(window, cx).placeholder("Tell people a little more about your event. Markdown, new lines, and links are supported."));
        description.update(cx, |s, cx| s.set_value(event.as_ref().and_then(|e| e.description.clone()).unwrap_or_default(), window, cx));
        cx.subscribe(&description, |_, _, _: &InputEvent, cx| cx.notify()).detach();
        let first_channel = self.event_guild(&guild_id).and_then(|g| event_channels(g, self.my_id(), ScheduledEvent::VOICE).first().map(|c| c.0.clone()));
        let entity_type = event.as_ref().map_or(ScheduledEvent::VOICE, |e| e.entity_type);
        self.events.wizard = Some(Wizard {
            guild_id,
            editing,
            started: event.as_ref().is_some_and(|e| e.status == ScheduledEvent::ACTIVE),
            step: Step::Location,
            entity_type,
            channel_id: event.as_ref().map_or(first_channel, |e| e.channel_id.clone()),
            frequency: event.as_ref().and_then(|e| e.recurrence.as_ref()).map_or(Frequency::Once, Frequency::of_rule),
            location,
            topic,
            start_date,
            start_time,
            end_date,
            end_time,
            description,
            link: None,
            busy: false,
        });
        cx.notify();
    }

    // Demo scenes jump straight to a step with a topic typed.
    #[cfg(debug_assertions)]
    pub(crate) fn fill_event_wizard(&mut self, topic: &str, entity_type: u8, location: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(wizard) = self.events.wizard.as_mut() else { return };
        wizard.step = Step::Info;
        wizard.entity_type = entity_type;
        wizard.frequency = Frequency::Weekly;
        let (topic_state, location_state) = (wizard.topic.clone(), wizard.location.clone());
        topic_state.update(cx, |s, cx| s.set_value(topic.to_owned(), window, cx));
        location_state.update(cx, |s, cx| s.set_value(location.to_owned(), window, cx));
    }

    fn wizard_times(&self, cx: &Context<Self>) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
        let Some(w) = self.events.wizard.as_ref() else { return (None, None) };
        let read = |s: &Entity<InputState>| s.read(cx).value().to_string();
        (parse_local(&read(&w.start_date), &read(&w.start_time)), parse_local(&read(&w.end_date), &read(&w.end_time)))
    }

    // Discord's validation (`688218`), in its words.
    fn wizard_problems(&self, cx: &Context<Self>) -> Problems {
        let Some(w) = self.events.wizard.as_ref() else { return Problems::default() };
        let external = w.entity_type == ScheduledEvent::EXTERNAL;
        let location = w.location.read(cx).value().trim().to_owned();
        let topic = w.topic.read(cx).value().trim().to_owned();
        let (start, end) = self.wizard_times(cx);
        let now = Utc::now();
        let location = match external {
            true if location.is_empty() => Some("Either a location or channel must be specified."),
            true if location.chars().count() > LOCATION_MAX => Some("Locations can be at most 100 characters."),
            false if w.channel_id.is_none() => Some("Either a location or channel must be specified."),
            _ => None,
        };
        let topic = match topic.chars().count() {
            0 => Some("Topic must be specified."),
            n if n > TOPIC_MAX => Some("Topics can be at most 100 characters."),
            _ => None,
        };
        let horizon = if w.frequency == Frequency::Once { Duration::days(365) } else { Duration::days(365 * 4) };
        let start_problem = match start {
            None => Some("Events must have a start date and time."),
            Some(s) if w.editing.is_none() && s <= now => Some("The start time must be in the future."),
            Some(s) if s > now + horizon => Some("The start time is too far in the future."),
            _ => None,
        };
        let end = match (external, start, end) {
            (false, _, _) => None,
            (true, _, None) => Some("End date and time are required."),
            (true, Some(s), Some(e)) if e <= s => Some("The end time must be after the start time."),
            (true, _, Some(e)) if e <= now => Some("The end time must be in the future."),
            _ => None,
        };
        let description = w.description.read(cx).value().chars().count() > DESCRIPTION_MAX;
        Problems { location, topic, start: start_problem, end, description }
    }

    fn wizard_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let problems = self.wizard_problems(cx);
        let Some(w) = self.events.wizard.as_mut() else { return };
        let next = match (w.step, forward) {
            (Step::Location, true) => Step::Info,
            (Step::Info, true) => Step::Review,
            (Step::Info, false) => Step::Location,
            (Step::Review, false) => Step::Info,
            (step, _) => step,
        };
        if forward && !problems.step_ok(w.step) {
            return;
        }
        w.step = next;
        cx.notify();
    }

    fn wizard_submit(&mut self, cx: &mut Context<Self>) {
        let editing_series = self.events.wizard.as_ref().and_then(|w| w.editing.as_deref().map(|id| (w.guild_id.clone(), id.to_owned())));
        let series = editing_series.and_then(|(g, id)| self.scheduled_event(&g, &id).cloned()).filter(|e| e.recurrence.is_some());
        let schedule_changed = series.is_some_and(|e| {
            let (start, end) = self.wizard_times(cx);
            start.map(|s| s.to_rfc3339()) != parse_time(&e.start).map(|s| s.to_rfc3339()) || end.map(|s| s.to_rfc3339()) != e.end.as_deref().and_then(parse_time).map(|s| s.to_rfc3339())
        });
        if !schedule_changed {
            return self.send_event(cx);
        }
        self.confirm("Edit all Events in this Series", "Are you sure you want to edit this event? Editing this event will also update all events in this series.", "Save Event", |this, _, cx| this.send_event(cx), cx);
        self.set_dialog_cancel("Never Mind");
    }

    fn send_event(&mut self, cx: &mut Context<Self>) {
        if !self.wizard_problems(cx).step_ok(Step::Review) {
            return;
        }
        let (start, end) = self.wizard_times(cx);
        let Some(w) = self.events.wizard.as_ref() else { return };
        let Some(start) = start else { return };
        let external = w.entity_type == ScheduledEvent::EXTERNAL;
        let description = w.description.read(cx).value().trim().to_owned();
        let body = json!({
            "name": w.topic.read(cx).value().trim(),
            "description": description,
            "privacy_level": PRIVATE,
            "scheduled_start_time": start.to_rfc3339(),
            "scheduled_end_time": if external { end.map(|e| e.to_rfc3339()) } else { None },
            "entity_type": w.entity_type,
            "channel_id": if external { None } else { w.channel_id.clone() },
            "entity_metadata": if external { json!({ "location": w.location.read(cx).value().trim() }) } else { Value::Null },
            "recurrence_rule": w.frequency.rule(start, start.with_timezone(&Local).date_naive()),
        });
        let (method, path) = match &w.editing {
            Some(id) => (Method::PATCH, format!("/guilds/{}/scheduled-events/{id}", w.guild_id)),
            None => (Method::POST, format!("/guilds/{}/scheduled-events", w.guild_id)),
        };
        let editing = w.editing.is_some();
        let Some(client) = self.client.clone() else { return };
        if let Some(w) = self.events.wizard.as_mut() {
            w.busy = true;
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = client.call(method, path, Some(body)).await;
            this.update(cx, |this, cx| {
                if let Some(w) = this.events.wizard.as_mut() {
                    w.busy = false;
                }
                match result.map(|v| discord::parse_scheduled_event(&v)) {
                    Ok(Some(event)) => {
                        let link = event_link(&event);
                        this.on_event(discord::Event::ScheduledEvent(event), cx);
                        match (editing, this.events.wizard.as_mut()) {
                            (false, Some(w)) => {
                                w.step = Step::Success;
                                w.link = Some(link);
                            }
                            _ => this.events.wizard = None,
                        }
                    }
                    Ok(None) => this.events.wizard = None,
                    Err(error) => this.show_toast(error, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn set_wizard(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Wizard)) {
        if let Some(w) = self.events.wizard.as_mut() {
            change(w);
        }
        cx.notify();
    }

    fn open_frequency_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (start, _) = self.wizard_times(cx);
        let day = start.map_or_else(|| Local::now().date_naive(), |s| s.with_timezone(&Local).date_naive());
        let at = window.mouse_position();
        self.open_live_menu(
            at,
            move |this| {
                let chosen = this.events.wizard.as_ref().map(|w| w.frequency);
                Frequency::options(day)
                    .into_iter()
                    .map(|f| item(f.label(day), move |this, _, cx| this.set_wizard(cx, |w| w.frequency = f)).radio(chosen == Some(f)).entry())
                    .collect()
            },
            cx,
        );
    }

    fn open_event_channel_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(w) = self.events.wizard.as_ref() else { return };
        let Some(guild) = self.event_guild(&w.guild_id) else { return };
        let (chosen, glyph) = (w.channel_id.clone(), if w.entity_type == ScheduledEvent::VOICE { Some(Glyph::VolumeHigh) } else { None });
        let entries: Vec<Entry> = event_channels(guild, self.my_id(), w.entity_type)
            .into_iter()
            .map(|(id, name)| {
                let on = chosen.as_ref() == Some(&id);
                let row = item(name, move |this, _, cx| this.set_wizard(cx, |w| w.channel_id = Some(id.clone()))).radio(on);
                match glyph {
                    Some(glyph) => row.icon(glyph).entry(),
                    None => row.entry(),
                }
            })
            .collect();
        self.open_menu(window.mouse_position(), entries, cx);
    }
}

// The channels an event of `entity_type` can be held in: voice or stage channels you can see and join.
fn event_channels(guild: &Guild, me: &str, entity_type: u8) -> Vec<(String, String)> {
    let stage = entity_type == ScheduledEvent::STAGE;
    guild
        .channels
        .iter()
        .filter(|c| c.kind == discord::ChannelKind::Voice && is_stage(c) == stage)
        .filter(|c| guild.channel_permissions(me, c) & (discord::VIEW_CHANNEL | perm::CONNECT) == discord::VIEW_CHANNEL | perm::CONNECT)
        .map(|c| (c.id.clone(), c.name.clone()))
        .collect()
}

fn is_stage(channel: &discord::Channel) -> bool {
    channel.raw_type == STAGE_TYPE
}

// Rendering
impl Waffle {
    pub(crate) fn events_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::new();
        out.extend(self.events_list_dialog(window, cx));
        out.extend(self.event_details_dialog(window, cx));
        out.extend(self.event_wizard_dialog(window, cx));
        out.extend(self.start_event_dialog(window, cx));
        out
    }

    // The sidebar's top: a live event's bar, then the Events row (shown when there's something to see or
    // you could add one).
    pub(crate) fn event_rows(&self, guild: &Guild, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let me = self.my_id();
        let count = upcoming_count(guild, me);
        let mut out: Vec<AnyElement> = Vec::new();
        if let Some(live) = guild.events.iter().find(|e| e.status == ScheduledEvent::ACTIVE) {
            out.push(self.live_bar(guild, live, cx));
        }
        if count == 0 && !can_create(guild, me) {
            return out;
        }
        let gid = guild.id.clone();
        let state = crate::sidebar::RowState { selected: self.events.list.as_deref() == Some(guild.id.as_str()), unread: false, muted: false, strong: false };
        let row = crate::sidebar::channel_row(SharedString::from(format!("upcoming-events-{}", guild.id)), "events-row".into(), state, cx)
            .child(Icon::new(Glyph::Calendar).size(px(20.)).flex_none())
            .child(div().flex_1().min_w_0().truncate().child(events_title(count)))
            .on_click(cx.listener(move |this, _, _, cx| this.open_events(gid.clone(), cx)));
        out.push(row.into_any_element());
        out
    }

    fn live_bar(&self, guild: &Guild, event: &ScheduledEvent, cx: &mut Context<Self>) -> AnyElement {
        let channel = event.channel_id.as_deref().and_then(|c| guild.channel(c)).cloned();
        let listening = channel.as_ref().map(|c| guild.voice_states.iter().filter(|v| v.channel_id.as_deref() == Some(c.id.as_str())).count());
        let line = match (&channel, listening) {
            (Some(c), Some(n)) if is_stage(c) => format!("{} · {n} listening", c.name),
            (Some(c), Some(n)) => format!("{} · {n} in voice", c.name),
            _ => self.event_place(event),
        };
        let lead = match &channel {
            Some(c) => crate::sidebar::channel_glyph(c),
            None => Icon::new(Glyph::MapPoint),
        };
        let (g, e) = (event.guild_id.clone(), event.id.clone());
        let join = channel.clone().map(|c| {
            let label = if is_stage(&c) { "Join stage" } else { "Join" };
            ui::button("events-live-join", Tone::Primary, cx).small().h(px(28.)).child(label).on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.join_voice(c.clone(), cx);
            }))
        });
        div()
            .id("events-live-bar")
            .mb(px(8.))
            .p(px(10.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .squircle(ui::RADIUS_MD)
            .bg(palette::accent_soft(cx))
            .text_color(palette::text(cx))
            .cursor_pointer()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .min_w_0()
                    .child(ui::live_badge().gap(px(4.)).child(ui::bars_still(gpui_kit::white(), 10.)))
                    .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(event.name.clone())),
            )
            .child(div().flex().items_center().gap(px(4.)).min_w_0().text_size(px(12.)).text_color(palette::muted(cx)).child(lead.size(px(14.))).child(div().min_w_0().truncate().child(line)))
            .children(join)
            .on_click(cx.listener(move |this, _, _, cx| this.open_event_details(g.clone(), e.clone(), cx)))
            .into_any_element()
    }

    fn events_list_dialog(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let guild_id = self.events.list.as_deref()?;
        let guild = self.event_guild(guild_id)?;
        let me = self.my_id();
        let events = listed(guild, me);
        let title = events_title(upcoming_count(guild, me));
        let gid = guild_id.to_owned();
        let create = can_create(guild, me).then(|| {
            div().flex().items_center().gap(px(12.)).child(div().w(px(1.)).h(px(20.)).bg(palette::line(cx))).child(
                ui::button("events-create", Tone::Primary, cx).small().child(Icon::new(Glyph::Plus).size(px(16.))).child("Create Event").on_click(cx.listener(move |this, _, window, cx| this.open_event_wizard(gid.clone(), None, window, cx))),
            )
        });
        let head = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(div().text_color(palette::muted(cx)).child(Icon::new(Glyph::Calendar).size(px(20.))))
            .child(div().flex_1().min_w_0().truncate().text_size(px(20.)).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(title))
            .children(create)
            .child(ui::icon_button("events-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.close_event_layer(cx))));
        let now = Local::now();
        let body: AnyElement = match events.is_empty() {
            true => self.events_empty(guild, cx),
            false => div()
                .id("events-list")
                .max_h(px(560.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(12.))
                .children(events.into_iter().map(|e| self.event_card(e, &now, true, cx)))
                .into_any_element(),
        };
        let backdrop = div().id("events-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_event_layer(cx)));
        Some(ui::modal(backdrop, ui::dialog("events", LIST_WIDTH, ui::dialog_body().child(head).child(body), window, cx), cx))
    }

    fn events_empty(&self, guild: &Guild, cx: &mut Context<Self>) -> AnyElement {
        // GAP: Discord links "Server Settings > Roles"; it reads as plain text here.
        let roles = guild.can(self.my_id(), perm::MANAGE_ROLES).then(|| div().max_w(px(320.)).text_center().text_size(px(12.)).text_color(palette::faint(cx)).child("You can give other people permission to create events in Server Settings > Roles."));
        ui::empty_state("events-empty", Mood::Idle, "There are no upcoming events.", "Schedule an event for any planned activity in your server.", cx).children(roles).into_any_element()
    }

    // One card (`.ev2`): when, who and how many; the name and two lines of description; where, and the actions.
    // `actions` off draws the wizard's preview.
    fn event_card(&self, event: &ScheduledEvent, now: &DateTime<Local>, actions: bool, cx: &mut Context<Self>) -> AnyElement {
        let when = when(event, now);
        let ink = phase_ink(when.phase, cx);
        let (strong, muted) = (palette::strong(cx), palette::muted(cx));
        let canceled = when.phase == Phase::Canceled;
        let cover = event.cover_url().map(|url| ui::media(url, ui::all(ui::RADIUS_MD)).w_full().h(px(140.)).mb(px(6.)));
        let head = div()
            .flex()
            .items_start()
            .gap(px(8.))
            .min_w_0()
            .child(div().mt(px(1.)).text_color(ink).child(Icon::new(if canceled { Glyph::CalendarX } else { Glyph::Calendar }).size(px(18.))))
            .child(when_column(&when, repeat_label(event), ink, cx))
            .child(self.who(event, cx));
        let card = div()
            .id(SharedString::from(format!("event-card-{}", event.id)))
            .flex()
            .flex_col()
            .gap(px(6.))
            .p(px(16.))
            .min_w_0()
            .flex_none()
            .squircle(ui::RADIUS_LG)
            .bg(palette::surface(palette::Level::Page, cx))
            .border_1()
            .border_color(palette::line(cx))
            .children(cover)
            .child(head)
            .child(div().mt(px(2.)).text_size(px(18.)).line_height(px(22.)).font_weight(FontWeight::SEMIBOLD).text_color(if canceled { muted } else { strong }).child(event.name.clone()))
            .children(event.description.clone().map(|d| div().text_size(px(14.)).line_height(px(20.)).line_clamp(2).text_color(palette::text(cx)).child(d)))
            .child(self.card_foot(event, actions, cx));
        if !actions {
            return card.into_any_element();
        }
        let (g, e) = (event.guild_id.clone(), event.id.clone());
        let hover = palette::line_strong(cx);
        card.cursor_pointer().hover(move |s| s.border_color(hover)).on_click(cx.listener(move |this, _, _, cx| this.open_event_details(g.clone(), e.clone(), cx))).into_any_element()
    }

    // The creator's avatar and the Interested count, with the tooltip naming both.
    fn who(&self, event: &ScheduledEvent, cx: &mut Context<Self>) -> AnyElement {
        let creator = self.event_creator(event);
        let tip = match &creator {
            Some(user) => format!("Created by {} · {} people are interested", user.name, event.interested),
            None => format!("{} people are interested", event.interested),
        };
        div()
            .id(SharedString::from(format!("event-who-{}", event.id)))
            .flex_none()
            .h(px(20.))
            .flex()
            .items_center()
            .gap(px(4.))
            .text_size(px(12.))
            .text_color(palette::muted(cx))
            .children(creator.map(|u| ui::avatar(u.avatar, 20.).mr(px(4.))))
            .child(Icon::new(Glyph::Users).size(px(13.)))
            .child(event.interested.to_string())
            .tooltip(move |window, cx| ui::tip(tip.clone(), window, cx))
            .into_any_element()
    }

    fn card_foot(&self, event: &ScheduledEvent, actions: bool, cx: &mut Context<Self>) -> AnyElement {
        div()
            .mt(px(6.))
            .pt(px(12.))
            .border_t_1()
            .border_color(palette::line(cx))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(self.event_location(event, actions, cx))
            .when(actions, |d| d.children(self.event_actions(event, true, cx)))
            .into_any_element()
    }

    // The channel (opens it) or the place (a link opens in the browser).
    fn event_location(&self, event: &ScheduledEvent, live: bool, cx: &mut Context<Self>) -> AnyElement {
        let channel = event.channel_id.as_deref().and_then(|c| self.event_guild(&event.guild_id)?.channel(c)).cloned();
        let icon = match &channel {
            Some(c) => crate::sidebar::channel_glyph(c),
            None => Icon::new(Glyph::MapPoint),
        };
        let place = self.event_place(event);
        let link = place.starts_with("http://") || place.starts_with("https://");
        let text = palette::text(cx);
        div()
            .id(SharedString::from(format!("event-place-{}", event.id)))
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(14.))
            .text_color(text)
            .child(div().flex_none().text_color(palette::muted(cx)).child(icon.size(px(18.))))
            .child(div().min_w_0().truncate().child(place.clone()))
            .when(live && (channel.is_some() || link), |d| {
                d.cursor_pointer().hover(|s| s.underline()).on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    match channel.clone() {
                        Some(c) => {
                            this.events = Events::default();
                            this.open_channel(c, window, cx);
                        }
                        None => cx.open_url(&place),
                    }
                }))
            })
            .into_any_element()
    }

    // The footer's buttons (`428978.Zq`): more, then Interested, Start, Join or End Event as the state allows.
    fn event_actions(&self, event: &ScheduledEvent, more: bool, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let manage = self.may_manage(event);
        let (active, canceled) = (event.status == ScheduledEvent::ACTIVE, event.status == ScheduledEvent::CANCELED);
        let external = event.entity_type == ScheduledEvent::EXTERNAL;
        let key = |part: &str| SharedString::from(format!("event-{part}-{}-{}", event.id, more));
        let ids = || (event.guild_id.clone(), event.id.clone());
        let mut out: Vec<AnyElement> = Vec::new();
        if more {
            let (g, e) = ids();
            out.push(
                ui::icon_button(key("more"), Glyph::MoreVertical, "More event options", cx)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.open_event_menu(g.clone(), e.clone(), window, cx);
                    }))
                    .into_any_element(),
            );
        }
        if canceled {
            return out;
        }
        if !active {
            let (g, e) = ids();
            let on = event.me_interested;
            out.push(
                ui::button(key("interested"), Tone::Secondary, cx)
                    .small()
                    .toggled(on, cx)
                    .child(Icon::new(if on { Glyph::BellRinging } else { Glyph::Bell }).size(px(16.)))
                    .child("Interested")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.toggle_interested(g.clone(), e.clone(), cx);
                    }))
                    .into_any_element(),
            );
        }
        if !active && manage && within_start_window(event, Utc::now()) {
            let (g, e) = ids();
            out.push(
                ui::button(key("start"), Tone::Primary, cx)
                    .small()
                    .child("Start")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.open_start_event(g.clone(), e.clone(), cx);
                    }))
                    .into_any_element(),
            );
        }
        let channel = event.channel_id.as_deref().and_then(|c| self.event_guild(&event.guild_id)?.channel(c)).cloned();
        if let Some(channel) = channel.filter(|_| active && !external) {
            let joined = self.call.as_ref().is_some_and(|c| c.channel.id == channel.id);
            let label = match (joined, is_stage(&channel)) {
                (true, _) => "Go To Channel",
                (false, true) => "Listen In",
                (false, false) => "Join",
            };
            out.push(
                ui::button(key("join"), Tone::Primary, cx)
                    .small()
                    .child(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.events = Events::default();
                        match joined {
                            true => this.open_channel(channel.clone(), window, cx),
                            false => this.join_voice(channel.clone(), cx),
                        }
                    }))
                    .into_any_element(),
            );
        }
        if active && external && manage {
            let (g, e) = ids();
            out.push(
                ui::button(key("end"), Tone::Secondary, cx)
                    .small()
                    .child("End Event")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.confirm_end_event(g.clone(), e.clone(), cx);
                    }))
                    .into_any_element(),
            );
        }
        out
    }

    fn event_details_dialog(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let details = self.events.details.as_ref()?;
        let event = self.scheduled_event(&details.guild_id, &details.event_id)?;
        let cover = event.cover_url().map(|url| ui::media(url, ui::all(ui::RADIUS_LG)).w_full().h(px(160.)));
        let head = div()
            .flex()
            .items_start()
            .gap(px(12.))
            .child(div().flex_1().min_w_0().text_size(px(20.)).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(event.name.clone()))
            .child(ui::icon_button("event-details-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.close_event_layer(cx))));
        let items = [ui::Tab::new("Event Info"), ui::Tab::new(format!("{} Interested", event.interested))];
        let this = cx.entity().downgrade();
        let tabs = ui::tabs("event-details-tabs", &items, details.tab, move |i, _, cx| {
            this.update(cx, |this, cx| this.set_details_tab(i, cx)).ok();
        }, window, cx);
        let body = match details.tab {
            0 => self.event_info(event, cx),
            _ => self.interested_list(event, details.users.as_ref(), cx),
        };
        let foot = div().flex().items_center().justify_end().gap(px(8.)).children(self.event_actions(event, true, cx));
        let body = ui::dialog_body().children(cover).child(head).child(tabs).child(body).child(foot);
        let backdrop = div().id("event-details-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_event_layer(cx)));
        Some(ui::modal(backdrop, ui::dialog("event-details", DETAILS_WIDTH, body, window, cx), cx))
    }

    fn event_info(&self, event: &ScheduledEvent, cx: &mut Context<Self>) -> AnyElement {
        let now = Local::now();
        let when = when(event, &now);
        let ink = phase_ink(when.phase, cx);
        let (start, end) = (parse_time(&event.start).map(|t| t.with_timezone(&Local)), event.end.as_deref().and_then(parse_time).map(|t| t.with_timezone(&Local)));
        let dates = start.map(|s| range_line(&s, end.as_ref(), &now)).unwrap_or_default();
        let location = self.event_location(event, true, cx);
        let fact = |icon: AnyElement, text: String| div().flex().items_center().gap(px(10.)).min_w_0().child(div().flex_none().w(px(20.)).flex().justify_center().text_color(palette::muted(cx)).child(icon)).child(div().min_w_0().child(text));
        let creator = self.event_creator(event);
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(div().flex().items_start().gap(px(8.)).child(div().mt(px(1.)).text_color(ink).child(Icon::new(Glyph::Calendar).size(px(18.)))).child(when_column(&when, repeat_label(event), ink, cx)))
            .children(event.description.clone().map(|d| div().text_size(px(14.)).line_height(px(20.)).text_color(palette::text(cx)).child(d)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .text_size(px(14.))
                    .text_color(palette::text(cx))
                    .child(fact(Icon::new(Glyph::Clock).size(px(18.)).into_any_element(), dates))
                    .child(div().flex().child(location))
                    .children(creator.map(|u| fact(ui::avatar(u.avatar.clone(), 20.).into_any_element(), format!("Created by {}", u.name))))
                    .child(fact(Icon::new(Glyph::Users).size(px(18.)).into_any_element(), format!("{} people are interested", event.interested))),
            )
            .into_any_element()
    }

    fn interested_list(&self, event: &ScheduledEvent, users: Option<&Result<Vec<User>, String>>, cx: &mut Context<Self>) -> AnyElement {
        let note = |text: String| div().py(px(24.)).text_center().text_color(palette::muted(cx)).child(text).into_any_element();
        let users = match users {
            None => return div().flex().flex_col().gap(px(8.)).children((0..3).map(|i| ui::skeleton(SharedString::from(format!("event-user-skeleton-{i}")), ui::RADIUS_SM, cx).h(px(36.)))).into_any_element(),
            Some(Err(_)) => return note("We are having problems looking up who is interested in this event. Please try again later.".into()),
            Some(Ok(users)) if users.is_empty() => return note("No one's interested in this event yet.".into()),
            Some(Ok(users)) => users,
        };
        let more = (event.interested as usize).saturating_sub(users.len());
        let rows = users.iter().map(|u| {
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(8.))
                .py(px(6.))
                .min_w_0()
                .child(ui::avatar(u.avatar.clone(), 32.))
                .child(div().min_w_0().truncate().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(u.name.clone()))
        });
        div()
            .id("event-interested")
            .max_h(px(360.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(2.))
            .children(rows)
            .when(more > 0, |d| d.child(div().px(px(8.)).pt(px(8.)).text_size(px(12.)).text_color(palette::muted(cx)).child(format!("{more} more people are interested."))))
            .into_any_element()
    }

    fn start_event_dialog(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let start = self.events.start.as_ref()?;
        let event = self.scheduled_event(&start.guild_id, &start.event_id)?;
        let notify = (event.entity_type == ScheduledEvent::STAGE).then(|| {
            let mark = ui::check(if start.notify { ui::Check::On } else { ui::Check::Off }, cx);
            ui::pick_row("event-start-notify", mark, "Notify @everyone", None, cx).on_click(cx.listener(|this, _, _, cx| {
                if let Some(start) = this.events.start.as_mut() {
                    start.notify = !start.notify;
                }
                cx.notify();
            }))
        });
        let foot = div()
            .flex()
            .justify_end()
            .gap(px(8.))
            .child(ui::button("event-start-cancel", Tone::Secondary, cx).child("Cancel").on_click(cx.listener(|this, _, _, cx| this.close_event_layer(cx))))
            .child(ui::button("event-start-go", Tone::Primary, cx).child("Start Event").on_click(cx.listener(|this, _, _, cx| this.start_event(cx))));
        let body = ui::dialog_body()
            .child(div().text_size(px(20.)).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(event.name.clone()))
            .child(div().text_color(palette::muted(cx)).child("You are about to start this Private event"))
            .children(notify)
            .child(foot);
        let backdrop = div().id("event-start-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_event_layer(cx)));
        Some(ui::modal(backdrop, ui::dialog("event-start", 420., body, window, cx), cx))
    }

    fn event_wizard_dialog(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let w = self.events.wizard.as_ref()?;
        let (title, subtitle) = match w.step {
            Step::Location => ("Where is your event?", Some("So no one gets lost on where to go.")),
            Step::Info => ("What's your event about?", Some("Fill out the details of your event.")),
            Step::Review => ("Here's a preview of your event.", None),
            Step::Success => ("All set. Now share your event!", None),
        };
        let head = div()
            .flex()
            .items_start()
            .gap(px(12.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().text_size(px(20.)).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(title))
                    .children(subtitle.map(|s| div().text_size(px(14.)).text_color(palette::muted(cx)).child(s))),
            )
            .child(ui::icon_button("event-wizard-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.close_event_layer(cx))));
        let problems = self.wizard_problems(cx);
        let step = match w.step {
            Step::Location => self.wizard_location(w, &problems, cx),
            Step::Info => self.wizard_info(w, &problems, cx),
            Step::Review => self.wizard_review(w, cx),
            Step::Success => wizard_success(w, cx),
        };
        let body = ui::dialog_body()
            .child(head)
            .when(w.step != Step::Success, |d| d.child(stepper(w.step, cx)))
            .child(step)
            .when(w.step != Step::Success, |d| d.child(wizard_foot(w, problems.step_ok(w.step), cx)));
        let backdrop = div().id("event-wizard-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_event_layer(cx)));
        Some(ui::modal(backdrop, ui::dialog("event-wizard", WIZARD_WIDTH, body, window, cx), cx))
    }

    fn wizard_location(&self, w: &Wizard, problems: &Problems, cx: &mut Context<Self>) -> AnyElement {
        let guild = self.event_guild(&w.guild_id);
        let community = guild.is_some_and(|g| g.features.iter().any(|f| f == "COMMUNITY"));
        let me = self.my_id();
        let options = [
            (ScheduledEvent::STAGE, "Stage Channel", "Great for larger community audio events."),
            (ScheduledEvent::VOICE, "Voice Channel", "Hang out with voice, video, screenshare, and Go Live."),
            (ScheduledEvent::EXTERNAL, "Somewhere Else", "Text channel, external link, or in-person location."),
        ];
        let rows: Vec<AnyElement> = options.into_iter().filter(|(kind, ..)| *kind != ScheduledEvent::STAGE || community).map(|(kind, name, hint)| {
            let empty = kind != ScheduledEvent::EXTERNAL && guild.is_none_or(|g| event_channels(g, me, kind).is_empty());
            let detail = if empty { "There are no available channels." } else { hint };
            let on = w.entity_type == kind;
            ui::pick_row(SharedString::from(format!("event-kind-{kind}")), ui::radio(on, cx), name, Some(detail.into()), cx)
                .disabled(empty || w.started)
                .on_click(cx.listener(move |this, _, _, cx| {
                    let first = this.events.wizard.as_ref().and_then(|w| this.event_guild(&w.guild_id)).and_then(|g| event_channels(g, this.my_id(), kind).first().map(|c| c.0.clone()));
                    this.set_wizard(cx, |w| {
                        if w.entity_type != kind {
                            w.channel_id = first;
                        }
                        w.entity_type = kind;
                    });
                }))
                .into_any_element()
        }).collect();
        let place: AnyElement = match w.entity_type {
            ScheduledEvent::EXTERNAL => {
                let focus = w.location.read(cx).focus_handle(cx);
                ui::field("Enter a location", cx).child(ui::text_field("event-location", Input::new(&w.location).appearance(false), &focus)).into_any_element()
            }
            kind => {
                let name = w.channel_id.as_deref().and_then(|c| guild?.channel(c)).map_or("Select a channel".to_owned(), |c| c.name.clone());
                let glyph = (kind == ScheduledEvent::VOICE).then_some(Glyph::VolumeHigh);
                ui::field("Select a channel", cx)
                    .child(ui::select("event-channel", glyph, name, false, cx).on_click(cx.listener(|this, _, window, cx| this.open_event_channel_menu(window, cx))))
                    .child(ui::field_hint("Events in private channels will only be visible to people who can access that channel.", None, cx))
                    .into_any_element()
            }
        };
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .when(w.started, |d| d.child(ui::callout(ui::Notice::Info, Glyph::Info, "Location", "You cannot edit the location type of an event after it starts.", cx)))
            .child(div().flex().flex_col().gap(px(2.)).children(rows))
            .child(place)
            .when(w.step == Step::Location && w.entity_type == ScheduledEvent::EXTERNAL, |d| d.children(problems.location.filter(|_| !w.location.read(cx).value().is_empty()).map(|p| ui::field_error(p, cx))))
            .into_any_element()
    }

    fn wizard_info(&self, w: &Wizard, problems: &Problems, cx: &mut Context<Self>) -> AnyElement {
        let text = |id: &'static str, state: &Entity<InputState>, error: bool, cx: &mut Context<Self>| {
            let focus = state.read(cx).focus_handle(cx);
            ui::text_field(id, Input::new(state).appearance(false), &focus).error(error)
        };
        let typed_topic = !w.topic.read(cx).value().trim().is_empty();
        let topic = ui::field("Event Topic", cx).child(text("event-topic", &w.topic, problems.topic.is_some() && typed_topic, cx)).children(problems.topic.filter(|_| typed_topic).map(|p| ui::field_error(p, cx)));
        let pair = |a: AnyElement, b: AnyElement| div().flex().gap(px(12.)).child(div().flex_1().min_w_0().child(a)).child(div().flex_1().min_w_0().child(b));
        let start = pair(
            ui::field("Start Date", cx).child(text("event-start-date", &w.start_date, problems.start.is_some(), cx).leading(Icon::new(Glyph::Calendar).size(px(16.)))).into_any_element(),
            ui::field("Start Time", cx).child(text("event-start-time", &w.start_time, problems.start.is_some(), cx).leading(Icon::new(Glyph::Clock).size(px(16.)))).into_any_element(),
        );
        let external = w.entity_type == ScheduledEvent::EXTERNAL;
        let end = external.then(|| {
            pair(
                ui::field("End Date", cx).child(text("event-end-date", &w.end_date, problems.end.is_some(), cx).leading(Icon::new(Glyph::Calendar).size(px(16.)))).into_any_element(),
                ui::field("End Time", cx).child(text("event-end-time", &w.end_time, problems.end.is_some(), cx).leading(Icon::new(Glyph::Clock).size(px(16.)))).into_any_element(),
            )
        });
        let (start_at, _) = self.wizard_times(cx);
        let day = start_at.map_or_else(|| Local::now().date_naive(), |s| s.with_timezone(&Local).date_naive());
        let frequency = ui::field("Event Frequency", cx).child(ui::select("event-frequency", Some(Glyph::Repeat), w.frequency.label(day), false, cx).on_click(cx.listener(|this, _, window, cx| this.open_frequency_menu(window, cx))));
        let focus = w.description.read(cx).focus_handle(cx);
        let length = w.description.read(cx).value().chars().count();
        let description = ui::field("Description", cx)
            .child(ui::text_area("event-description", Textarea::new(&w.description).appearance(false).px_0().text_size(px(14.)), &focus).error(length > DESCRIPTION_MAX))
            .child(ui::field_hint("", Some(format!("{length} / {DESCRIPTION_MAX}").into()), cx));
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(topic)
            .child(start)
            .children(problems.start.map(|p| ui::field_error(p, cx)))
            .children(end)
            .children(problems.end.map(|p| ui::field_error(p, cx)))
            .child(ui::field_hint("Dates as 2026-11-28, times as 7:00 PM or 19:00, in your time zone.", None, cx))
            .child(frequency)
            .child(description)
            .into_any_element()
    }

    fn wizard_review(&self, w: &Wizard, cx: &mut Context<Self>) -> AnyElement {
        let (start, end) = self.wizard_times(cx);
        let external = w.entity_type == ScheduledEvent::EXTERNAL;
        let preview = ScheduledEvent {
            id: "preview".into(),
            guild_id: w.guild_id.clone(),
            channel_id: if external { None } else { w.channel_id.clone() },
            creator_id: self.me.as_ref().map(|m| m.id.clone()),
            creator: self.me.clone(),
            name: w.topic.read(cx).value().trim().to_owned(),
            description: Some(w.description.read(cx).value().trim().to_owned()).filter(|d| !d.is_empty()),
            start: start.map(|s| s.to_rfc3339()).unwrap_or_default(),
            end: if external { end.map(|e| e.to_rfc3339()) } else { None },
            status: ScheduledEvent::SCHEDULED,
            entity_type: w.entity_type,
            location: external.then(|| w.location.read(cx).value().trim().to_owned()),
            recurrence: start.and_then(|s| w.frequency.rule(s, s.with_timezone(&Local).date_naive())),
            ..Default::default()
        };
        let channel = self.event_place(&preview);
        let note = match external {
            true => div().child("This event will auto start when it's time."),
            false => div().flex().flex_wrap().gap(px(4.)).child("When it's time, visit").child(div().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(format!("#{channel}"))).child("to start the event!"),
        };
        div().flex().flex_col().gap(px(12.)).child(self.event_card(&preview, &Local::now(), false, cx)).child(note.text_size(px(14.)).text_color(palette::muted(cx))).into_any_element()
    }
}

// The status line's column: the coloured lead and the rest on one line, the repeat under it.
fn when_column(when: &When, repeat: Option<String>, ink: Hsla, cx: &gpui_kit::App) -> AnyElement {
    let muted = palette::muted(cx);
    let line = div()
        .flex()
        .flex_wrap()
        .gap(px(4.))
        .children(when.lead.clone().map(|lead| div().font_weight(FontWeight::SEMIBOLD).text_color(ink).child(lead)))
        .when(when.lead.is_some() && when.rest.is_some(), |d| d.child("·"))
        .children(when.rest.clone());
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .text_size(px(14.))
        .line_height(px(20.))
        .text_color(muted)
        .child(line)
        .children(repeat.map(|r| div().flex().items_center().gap(px(4.)).text_size(px(12.)).line_height(px(18.)).child(Icon::new(Glyph::Repeat).size(px(13.))).child(r)))
        .into_any_element()
}

// `.cm-stepper`: Location, Event Info, Review; done steps take a tick, the current one the accent.
fn stepper(step: Step, cx: &gpui_kit::App) -> AnyElement {
    let at = match step {
        Step::Location => 0,
        Step::Info => 1,
        _ => 2,
    };
    let items = ["Location", "Event Info", "Review"].into_iter().enumerate().map(|(i, label)| {
        let (done, current) = (i < at, i == at);
        let mark = div()
            .size(px(20.))
            .flex_none()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(11.))
            .map(|d| match (done, current) {
                (true, _) => d.bg(palette::selected(cx)).text_color(palette::strong(cx)).child(Icon::new(Glyph::Checkmark).size(px(12.))),
                (_, true) => d.bg(palette::accent()).text_color(palette::on_accent()).child((i + 1).to_string()),
                _ => d.border(px(1.5)).border_color(palette::line_strong(cx)).child((i + 1).to_string()),
            });
        let ink = if current { palette::strong(cx) } else if done { palette::muted(cx) } else { palette::faint(cx) };
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(ink)
            .when(i > 0, |d| d.child(div().w(px(20.)).h(px(1.5)).mr(px(2.)).bg(palette::line_strong(cx))))
            .child(mark)
            .child(label)
    });
    div().flex().items_center().gap(px(8.)).children(items).into_any_element()
}

fn wizard_foot(w: &Wizard, ok: bool, cx: &mut Context<Waffle>) -> AnyElement {
    let primary = match (w.step, &w.editing) {
        (Step::Review, Some(_)) => "Save Event",
        (Step::Review, None) => "Create Event",
        _ => "Next",
    };
    let review = w.step == Step::Review;
    let busy = w.busy.then_some(if w.editing.is_some() { "Saving…" } else { "Creating…" });
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .when(w.step != Step::Location, |d| d.child(ui::button("event-wizard-back", Tone::Secondary, cx).small().child("Back").on_click(cx.listener(|this, _, _, cx| this.wizard_step(false, cx)))))
        .child(div().flex_1())
        .child(ui::button("event-wizard-cancel", Tone::Secondary, cx).child("Cancel").on_click(cx.listener(|this, _, _, cx| this.close_event_layer(cx))))
        .child(ui::button("event-wizard-next", Tone::Primary, cx).disabled(!ok || w.busy).loading(busy).child(primary).on_click(cx.listener(move |this, _, _, cx| match review {
            true => this.wizard_submit(cx),
            false => this.wizard_step(true, cx),
        })))
        .into_any_element()
}

fn wizard_success(w: &Wizard, cx: &mut Context<Waffle>) -> AnyElement {
    let link = w.link.clone().unwrap_or_default();
    let field = ui::field_frame(cx)
        .pr(px(4.))
        .child(div().flex_1().min_w_0().truncate().text_size(px(14.)).child(link.clone()))
        .child(ui::button("event-link-copy", Tone::Primary, cx).small().child("Copy").on_click(cx.listener(move |this, _, _, cx| this.copy_event_link(link.clone(), cx))));
    div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .child(div().text_color(palette::muted(cx)).child("Copy the event link below to invite people to your event. This also acts as a server invite link."))
        .child(field)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(start: &str, end: Option<&str>, status: u8, entity_type: u8) -> ScheduledEvent {
        ScheduledEvent { id: "42".into(), guild_id: "1".into(), name: "Movie night, part 2".into(), start: start.into(), end: end.map(Into::into), status, entity_type, ..Default::default() }
    }

    #[test]
    fn when_text_follows_discords_status_line() {
        let now = Utc.with_ymd_and_hms(2026, 11, 22, 18, 0, 0).unwrap();
        let soon = when(&event("2026-11-22T18:12:00Z", None, ScheduledEvent::SCHEDULED, ScheduledEvent::VOICE), &now);
        assert_eq!(soon, When { phase: Phase::Soon, lead: Some("Starting in 12m".into()), rest: None });
        let late = when(&event("2026-11-22T17:50:00Z", None, ScheduledEvent::SCHEDULED, ScheduledEvent::VOICE), &now);
        assert_eq!(late.lead.as_deref(), Some("Starting soon"));
        let live = when(&event("2026-11-22T17:00:00Z", Some("2026-11-22T23:00:00Z"), ScheduledEvent::ACTIVE, ScheduledEvent::EXTERNAL), &now);
        assert_eq!(live, When { phase: Phase::Live, lead: Some("Happening Now".into()), rest: Some("Ends 11:00 PM".into()) });
        let stage = when(&event("2026-11-22T17:00:00Z", None, ScheduledEvent::ACTIVE, ScheduledEvent::STAGE), &now);
        assert_eq!((stage.phase, stage.lead.as_deref(), stage.rest), (Phase::Live, Some("Live Now"), None));
        let canceled = when(&event("2026-11-21T18:00:00Z", None, ScheduledEvent::CANCELED, ScheduledEvent::VOICE), &now);
        assert_eq!(canceled, When { phase: Phase::Canceled, lead: Some("Canceled".into()), rest: Some("Sat Nov 21st · 6:00 PM".into()) });
        let tomorrow = when(&event("2026-11-23T17:00:00Z", None, ScheduledEvent::SCHEDULED, ScheduledEvent::VOICE), &now);
        assert_eq!(tomorrow.rest.as_deref(), Some("Tomorrow at 5:00 PM"));
        let later = when(&event("2026-11-28T19:00:00Z", Some("2026-11-28T23:00:00Z"), ScheduledEvent::SCHEDULED, ScheduledEvent::EXTERNAL), &now);
        assert_eq!((later.phase, later.rest.as_deref()), (Phase::Scheduled, Some("Sat Nov 28th · 7:00 PM — 11:00 PM")));
        let next_year = when(&event("2027-01-02T09:00:00Z", None, ScheduledEvent::SCHEDULED, ScheduledEvent::VOICE), &now);
        assert_eq!(next_year.rest.as_deref(), Some("Sat Jan 2nd, 2027 · 9:00 AM"));
    }

    #[test]
    fn manage_needs_manage_events_or_your_own_event_with_create() {
        let mut mine = event("2026-11-22T18:00:00Z", None, 1, 2);
        mine.creator_id = Some("me".into());
        assert!(manages(perm::MANAGE_EVENTS, "me", &event("2026-11-22T18:00:00Z", None, 1, 2)));
        assert!(manages(perm::CREATE_EVENTS, "me", &mine));
        assert!(!manages(perm::CREATE_EVENTS, "someone", &mine));
        assert!(!manages(0, "me", &mine));
    }

    #[test]
    fn ics_carries_times_escapes_and_the_series_rule() {
        let mut e = event("2026-11-28T19:00:00Z", Some("2026-11-28T23:00:00Z"), 1, ScheduledEvent::EXTERNAL);
        e.description = Some("Bring snacks; and chairs, please\nSee you".into());
        let rule = Frequency::Weekly.rule(parse_time(&e.start).unwrap(), NaiveDate::from_ymd_opt(2026, 11, 28).unwrap());
        e.recurrence = rule;
        let now = Utc.with_ymd_and_hms(2026, 11, 22, 18, 0, 0).unwrap();
        let text = ics(&e, "twitch.tv/nightowls", now).unwrap();
        assert!(text.starts_with("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n"));
        assert!(text.contains("\r\nDTSTART:20261128T190000Z\r\nDTEND:20261128T230000Z\r\n"));
        assert!(text.contains("SUMMARY:Movie night\\, part 2\r\n"));
        assert!(text.replace("\r\n ", "").contains("\r\nDESCRIPTION:Bring snacks\\; and chairs\\, please\\nSee you\\n\\nhttps://discord.com/events/1/42\r\n"));
        assert!(text.contains("\r\nRRULE:FREQ=WEEKLY;INTERVAL=1;BYDAY=SA\r\n"));
        assert!(text.ends_with("END:VEVENT\r\nEND:VCALENDAR\r\n"));
        assert!(text.split("\r\n").all(|l| l.len() <= 75));
    }

    #[test]
    fn frequency_round_trips_through_discords_rule() {
        let start = Utc.with_ymd_and_hms(2026, 11, 28, 19, 0, 0).unwrap();
        let day = start.date_naive();
        for f in Frequency::options(day) {
            let back = f.rule(start, day).map_or(Frequency::Once, |r| Frequency::of_rule(&r));
            assert_eq!(back, f);
        }
        assert!(!Frequency::options(day).contains(&Frequency::Weekdays));
        assert_eq!(Frequency::Monthly.label(day), "Monthly on the fourth Saturday");
        assert_eq!(Frequency::Yearly.label(day), "Annually on Nov 28");
    }

    #[test]
    fn clock_reads_both_styles() {
        assert_eq!(parse_clock("7:00 pm"), NaiveTime::from_hms_opt(19, 0, 0));
        assert_eq!(parse_clock("7 PM"), NaiveTime::from_hms_opt(19, 0, 0));
        assert_eq!(parse_clock("19:30"), NaiveTime::from_hms_opt(19, 30, 0));
        assert_eq!(parse_clock("noonish"), None);
    }
}
