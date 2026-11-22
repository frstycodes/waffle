// Guild scheduled events (docs/discord-events-re.md): the sidebar's Events row and live bar, the events
// list with its cards (design/explore-forums-announcements-events.html `.ev2-*`), the event menu,
// Interested, the details dialog, the create/edit wizard, Start, End and Cancel. Behaviour and copy are
// Discord's; the look is Waffle's.
use std::{
    collections::HashMap,
    fmt::Display,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveTime, TimeZone, Timelike, Utc};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, Focusable, FontWeight, Hsla, ImageSource, InteractiveElement, IntoElement, MouseButton, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window,
    component::{
        Icon,
        calendar::Matcher,
        date_picker::{DatePicker, DatePickerEvent, DatePickerState},
        input::{Input, InputEvent, InputState, Textarea, TextareaState},
        text::TextView,
        time_field::{HourCycle, TimeField, TimeFieldEvent, TimeFieldState, TimePrecision},
    },
    div,
    prelude::FluentBuilder as _,
    px,
};
use reqwest::Method;
use serde_json::{Value, json};

use crate::{
    Waffle,
    discord::{self, EventException, Guild, ScheduledEvent, User, perm},
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
const DISCORD_EPOCH: i64 = 1_420_070_400_000;
// Occurrences listed under a series' details.
const OCCURRENCES_SHOWN: usize = 5;
// rrule.js expansion stops at now+4 years (`er`).
const SERIES_YEARS: i64 = 4;

// The layers this feature stacks: the list, an event's details over it, the wizard, the Start dialog.
#[derive(Default)]
pub(crate) struct Events {
    list: Option<String>,
    details: Option<Details>,
    wizard: Option<Wizard>,
    start: Option<Start>,
    // Your Interested on single occurrences, by (event id, recurrence id); a series' own is `me_interested`.
    rsvps: HashMap<OccurrenceKey, bool>,
}

// One occurrence: its event's id and its recurrence id.
type OccurrenceKey = (String, String);

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
    start_date: Entity<DatePickerState>,
    start_time: Entity<TimeFieldState>,
    end_date: Entity<DatePickerState>,
    end_time: Entity<TimeFieldState>,
    description: Entity<TextareaState>,
    cover: Cover,
    // Editing one occurrence of a series: only its times change, saved as an exception.
    occurrence: Option<Occurrence>,
    link: Option<String>,
    busy: bool,
}

// The wizard's cover: the event's own, a file picked to replace it, or taken off.
#[derive(Clone, Debug, PartialEq)]
enum Cover {
    Kept,
    Picked { path: PathBuf, data: String },
    Removed,
}

// One occurrence of a series, with its exception applied.
#[derive(Clone, Debug, PartialEq)]
struct Occurrence {
    recurrence_id: String,
    original: DateTime<Utc>,
    start: DateTime<Utc>,
    end: Option<DateTime<Utc>>,
    canceled: bool,
    // An exception already exists for it, so changes PATCH it.
    excepted: bool,
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

// `/users/@me/scheduled-events`: the series you're Interested in, and your answers on single occurrences.
fn rsvps_of(list: &Value) -> (Vec<String>, Vec<(OccurrenceKey, bool)>) {
    let (mut series, mut single) = (Vec::new(), Vec::new());
    for answer in list.as_array().into_iter().flatten() {
        let Some(event) = answer["guild_scheduled_event_id"].as_str() else { continue };
        let on = answer["response"].as_u64().unwrap_or(1) == 1;
        match answer["guild_scheduled_event_exception_id"].as_str() {
            Some(occurrence) => single.push(((event.to_owned(), occurrence.to_owned()), on)),
            None if on => series.push(event.to_owned()),
            None => {}
        }
    }
    (series, single)
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

// Series

// `G3`: an occurrence's recurrence id is a snowflake of its start, floored to the second.
pub(crate) fn recurrence_id(start: DateTime<Utc>) -> String {
    let ms = start.timestamp() * 1000;
    (((ms - DISCORD_EPOCH).max(0) as u64) << 22).to_string()
}

// The series' starts from `first`, as rrule.js expands Discord's rule, up to `horizon`.
fn series_starts(rule: &Value, first: DateTime<Utc>, horizon: DateTime<Utc>) -> Vec<DateTime<Utc>> {
    let interval = rule["interval"].as_u64().unwrap_or(1).max(1) as i64;
    let weekdays: Vec<u64> = rule["by_weekday"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
    let frequency = Frequency::of_rule(rule);
    let step = |k: i64| -> Option<DateTime<Utc>> {
        match frequency {
            Frequency::Once => (k == 0).then_some(first),
            Frequency::Weekly | Frequency::Biweekly => Some(first + Duration::weeks(k * interval)),
            Frequency::Daily | Frequency::Weekdays => Some(first + Duration::days(k * interval)).filter(|t| weekdays.is_empty() || weekdays.contains(&u64::from(t.weekday().num_days_from_monday()))),
            Frequency::Monthly => nth_weekday_in_month(rule, first, k),
            Frequency::Yearly => first.with_year(first.year() + k as i32),
        }
    };
    let most = 366 * SERIES_YEARS + 1;
    (0..most).map(step).take_while(|t| t.is_none_or(|t| t <= horizon)).flatten().collect()
}

// Monthly on the nth weekday (`by_n_weekday`), `months` after the first, at the first's time; none when
// that month has no such day (a fifth Friday).
fn nth_weekday_in_month(rule: &Value, first: DateTime<Utc>, months: i64) -> Option<DateTime<Utc>> {
    let nth = &rule["by_n_weekday"][0];
    let (n, day) = (nth["n"].as_u64()? as u32, nth["day"].as_u64()? as u32);
    let index = i64::from(first.year()) * 12 + i64::from(first.month0()) + months;
    let month_start = NaiveDate::from_ymd_opt(index.div_euclid(12) as i32, index.rem_euclid(12) as u32 + 1, 1)?;
    let offset = (day + 7 - month_start.weekday().num_days_from_monday()) % 7;
    let date = month_start.with_day(1 + offset + n.checked_sub(1)? * 7)?;
    Some(date.and_time(first.time()).and_utc())
}

// The occurrences starting from `from`, each with its exception applied.
fn occurrences(event: &ScheduledEvent, from: DateTime<Utc>, limit: usize) -> Vec<Occurrence> {
    let (Some(rule), Some((first, end))) = (event.recurrence.as_ref(), span(event)) else { return Vec::new() };
    let length = event.end.as_ref().map(|_| end - first);
    let horizon = Utc::now().max(from) + Duration::days(365 * SERIES_YEARS);
    series_starts(rule, first, horizon)
        .into_iter()
        .map(|original| occurrence(&event.exceptions, original, length))
        .filter(|o| o.start.max(o.original) >= from)
        .take(limit)
        .collect()
}

fn occurrence(exceptions: &[EventException], original: DateTime<Utc>, length: Option<Duration>) -> Occurrence {
    let recurrence_id = recurrence_id(original);
    let exception = exceptions.iter().find(|x| x.id == recurrence_id);
    let start = exception.and_then(|x| x.start.as_deref()).and_then(parse_time).unwrap_or(original);
    let end = exception.and_then(|x| x.end.as_deref()).and_then(parse_time).or(length.map(|l| start + l));
    Occurrence { recurrence_id, original, start, end, canceled: exception.is_some_and(|x| x.canceled), excepted: exception.is_some() }
}

// "This event" on a series' card and menu: the next occurrence still on.
fn next_occurrence(event: &ScheduledEvent, now: DateTime<Utc>) -> Option<Occurrence> {
    occurrences(event, now, usize::MAX).into_iter().find(|o| !o.canceled)
}

fn occurrence_link(event: &ScheduledEvent, recurrence_id: &str) -> String {
    format!("{}/{recurrence_id}", event_link(event))
}

// The request that writes an occurrence's exception (`496092`): a new one names the occurrence by its
// original start; an existing one is patched by its id.
// Not verified: that an exception's id is the occurrence's recurrence id; the doc addresses both the same way.
fn exception_request(event: &ScheduledEvent, occurrence: &Occurrence, start: DateTime<Utc>, end: Option<DateTime<Utc>>, canceled: bool) -> (Method, String, Value) {
    let base = format!("/guilds/{}/scheduled-events/{}/exceptions", event.guild_id, event.id);
    let mut body = json!({ "scheduled_start_time": start.to_rfc3339(), "scheduled_end_time": end.map(|e| e.to_rfc3339()), "is_canceled": canceled });
    if occurrence.excepted {
        return (Method::PATCH, format!("{base}/{}", occurrence.recurrence_id), body);
    }
    body["original_scheduled_start_time"] = json!(occurrence.original.to_rfc3339());
    (Method::POST, base, body)
}

// Restore Event: an exception that only canceled is deleted; one that also moved the occurrence keeps the
// move and drops the cancel.
fn restore_request(event: &ScheduledEvent, occurrence: &Occurrence) -> (Method, String, Option<Value>) {
    if occurrence.start != occurrence.original {
        let (method, path, body) = exception_request(event, occurrence, occurrence.start, occurrence.end, false);
        return (method, path, Some(body));
    }
    (Method::DELETE, format!("/guilds/{}/scheduled-events/{}/exceptions/{}", event.guild_id, event.id, occurrence.recurrence_id), None)
}

// The exception as it stands once `body` lands; none once it's deleted.
fn exception_from(recurrence_id: &str, body: Option<&Value>) -> Option<EventException> {
    let body = body?;
    Some(EventException {
        id: recurrence_id.to_owned(),
        start: body["scheduled_start_time"].as_str().map(str::to_owned),
        end: body["scheduled_end_time"].as_str().map(str::to_owned),
        canceled: body["is_canceled"].as_bool().unwrap_or(false),
    })
}

// Covers

// A hash is Discord's; the wizard's picked file comes as an absolute path, the demo's art as a bundled one.
fn cover_source(event: &ScheduledEvent) -> Option<ImageSource> {
    let image = event.image.as_deref()?;
    if Path::new(image).is_absolute() {
        return Some(PathBuf::from(image).into());
    }
    match image.contains('/') {
        true => Some(image.to_owned().into()),
        false => event.cover_url().map(Into::into),
    }
}

// The create/PATCH body's `image`: the picked file's data URI, null to take the cover off, left out to keep it.
fn cover_field(cover: &Cover) -> Option<Value> {
    match cover {
        Cover::Kept => None,
        Cover::Picked { data, .. } => Some(json!(data)),
        Cover::Removed => Some(Value::Null),
    }
}

// What the wizard's preview shows: the picked file, nothing once removed, else the event's own.
fn wizard_image(cover: &Cover, kept: Option<String>) -> Option<String> {
    match cover {
        Cover::Kept => kept,
        Cover::Picked { path, .. } => Some(path.to_string_lossy().into_owned()),
        Cover::Removed => None,
    }
}

// Wizard dates

fn local_instant(day: NaiveDate, at: NaiveTime) -> Option<DateTime<Utc>> {
    Local.from_local_datetime(&day.and_time(at)).earliest().map(|t| t.with_timezone(&Utc))
}

fn picked_instant(date: &Entity<DatePickerState>, time: &Entity<TimeFieldState>, cx: &App) -> Option<DateTime<Utc>> {
    let day = date.read(cx).date_time().start()?.date();
    local_instant(day, time.read(cx).time())
}

// A date field's calendar, past days off; the date reads as the cards write it.
fn date_state(at: DateTime<Local>, window: &mut Window, cx: &mut Context<Waffle>) -> Entity<DatePickerState> {
    let today = Local::now().date_naive();
    let state = cx.new(|cx| {
        let mut state = DatePickerState::new(window, cx).date_format("%a, %b %-d, %Y").disabled_matcher(Matcher::custom(move |d| *d < today));
        state.set_date(at.date_naive(), window, cx);
        state
    });
    cx.subscribe(&state, |_, _, _: &DatePickerEvent, cx| cx.notify()).detach();
    state
}

fn time_state(at: DateTime<Local>, window: &mut Window, cx: &mut Context<Waffle>) -> Entity<TimeFieldState> {
    let state = cx.new(|cx| {
        let mut state = TimeFieldState::new(window, cx).precision(TimePrecision::Minute).hour_cycle(HourCycle::H12);
        state.set_time(at.time(), window, cx);
        state
    });
    cx.subscribe(&state, |_, _, _: &TimeFieldEvent, cx| cx.notify()).detach();
    state
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
        self.call_json(Method::GET, path, None, then, cx);
    }

    fn call_json(&mut self, method: Method, path: String, body: Option<Value>, then: impl FnOnce(&mut Waffle, Result<Value, String>, &mut Context<Waffle>) + 'static, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            let result = client.call(method, path, body).await;
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
        // Not verified: the response is a list of `{guild_scheduled_event_id, guild_scheduled_event_exception_id,
        // response, …}` for this server, the exception id set on single-occurrence answers.
        self.fetch_json(
            format!("/users/@me/scheduled-events?guild_ids={guild_id}"),
            move |this, result, _| {
                let Ok(list) = result else { return };
                let (series, single) = rsvps_of(&list);
                this.events.rsvps.extend(single);
                let Some(guild) = this.guilds.iter_mut().find(|x| x.id == g) else { return };
                for event in &mut guild.events {
                    event.me_interested = series.contains(&event.id);
                }
            },
            cx,
        );
    }

    // Whether you're Interested in one occurrence: its own answer, else the series'.
    fn occurrence_interested(&self, event: &ScheduledEvent, recurrence_id: &str) -> bool {
        self.events.rsvps.get(&(event.id.clone(), recurrence_id.to_owned())).copied().unwrap_or(event.me_interested)
    }

    // One occurrence's Interested (`929267.QC`): differing from the series is an answer on the occurrence
    // (`UNINTERESTED` opts out of a series you follow); matching it again drops that answer.
    fn set_occurrence_interested(&mut self, guild_id: String, event_id: String, recurrence_id: String, on: bool, cx: &mut Context<Self>) {
        let Some(series) = self.scheduled_event(&guild_id, &event_id).map(|e| e.me_interested) else { return };
        let path = format!("/guilds/{guild_id}/scheduled-events/{event_id}/{recurrence_id}/users/@me");
        let (method, body) = if on == series { (Method::DELETE, None) } else { (Method::PUT, Some(json!({ "response": u8::from(on) }))) };
        let key = (event_id, recurrence_id);
        let before = self.events.rsvps.get(&key).copied();
        let undo = key.clone();
        self.rest_optimistic(
            method,
            path,
            body,
            move |this| {
                match on == series {
                    true => this.events.rsvps.remove(&key),
                    false => this.events.rsvps.insert(key, on),
                };
            },
            move |this| {
                match before {
                    Some(was) => this.events.rsvps.insert(undo, was),
                    None => this.events.rsvps.remove(&undo),
                };
            },
            cx,
        );
    }

    // A series' Interested asks which: all of it or the next occurrence.
    fn open_series_interested(&mut self, guild_id: String, event_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let at = window.mouse_position();
        self.open_live_menu(at, move |this| this.interested_entries(&guild_id, &event_id, None), cx);
    }

    // "All events in this series" / "Only this event", ticked as they stand. `recurrence_id` picks the
    // occurrence; none means the next one.
    fn interested_entries(&self, guild_id: &str, event_id: &str, recurrence_id: Option<&str>) -> Vec<Entry> {
        let Some(event) = self.scheduled_event(guild_id, event_id) else { return Vec::new() };
        let Some(occurrence) = recurrence_id.map(str::to_owned).or_else(|| next_occurrence(event, Utc::now()).map(|o| o.recurrence_id)) else { return Vec::new() };
        let (series, single) = (event.me_interested, self.occurrence_interested(event, &occurrence));
        let (g, e) = (guild_id.to_owned(), event_id.to_owned());
        let (g2, e2) = (g.clone(), e.clone());
        vec![
            item("All events in this series", move |this, _, cx| this.toggle_interested(g.clone(), e.clone(), cx)).checked(series).entry(),
            item("Only this event", move |this, _, cx| this.set_occurrence_interested(g2.clone(), e2.clone(), occurrence.clone(), !single, cx)).checked(single).entry(),
        ]
    }

    pub(crate) fn open_event_details(&mut self, guild_id: String, event_id: String, cx: &mut Context<Self>) {
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
        // A series' items split into this (the next occurrence) and all of it.
        let next = event.recurrence.as_ref().and_then(|_| next_occurrence(event, Utc::now())).map(|o| o.recurrence_id);
        if manage && !canceled {
            let (g, e) = ids();
            let all = item(if next.is_some() { "Edit all Events in this Series" } else { "Edit Event" }, move |this, window, cx| this.open_event_wizard(g.clone(), Some(e.clone()), window, cx));
            out.push(match next.clone() {
                Some(occurrence) => {
                    let (g, e) = ids();
                    let this_one = item("Edit this Event", move |this, window, cx| this.open_occurrence_editor(g.clone(), e.clone(), occurrence.clone(), window, cx)).entry();
                    submenu("Edit Event", vec![this_one, all.entry()]).icon(Glyph::Pen).entry()
                }
                None => all.icon(Glyph::Pen).entry(),
            });
        }
        let link = event_link(event);
        let copy_all = item(if next.is_some() { "Copy Link for all Events" } else { "Copy Event Link" }, move |this, _, cx| this.copy_event_link(link.clone(), cx));
        out.push(match next.clone() {
            Some(occurrence) => {
                let link = occurrence_link(event, &occurrence);
                let only = item("Copy Link for Only this Event", move |this, _, cx| this.copy_event_link(link.clone(), cx)).entry();
                submenu("Copy Event Link", vec![only, copy_all.entry()]).icon(Glyph::Link).entry()
            }
            None => copy_all.icon(Glyph::Link).entry(),
        });
        if manage && !active && !canceled {
            let (g, e) = ids();
            out.push(Entry::Separator);
            let all = item(if next.is_some() { "Cancel all Events in this Series" } else { "Cancel Event" }, move |this, _, cx| this.confirm_cancel_event(g.clone(), e.clone(), cx)).danger();
            out.push(match next {
                Some(occurrence) => {
                    let (g, e) = ids();
                    let this_one = item("Cancel this Event", move |this, _, cx| this.confirm_cancel_occurrence(g.clone(), e.clone(), occurrence.clone(), cx)).danger().entry();
                    submenu("Cancel Event", vec![this_one, all.entry()]).danger().icon(Glyph::CalendarX).entry()
                }
                None => all.icon(Glyph::CalendarX).entry(),
            });
        }
        out
    }

    // An occurrence row's menu (`924037` on a recurrence): edit or cancel this one, Restore a canceled one,
    // its link, and its Interested.
    fn occurrence_menu(&self, guild_id: &str, event_id: &str, recurrence_id: &str) -> Vec<Entry> {
        let Some(event) = self.scheduled_event(guild_id, event_id) else { return Vec::new() };
        let Some(occurrence) = occurrences(event, Utc::now(), usize::MAX).into_iter().find(|o| o.recurrence_id == recurrence_id) else { return Vec::new() };
        let manage = self.may_manage(event) && event.status != ScheduledEvent::CANCELED;
        let ids = || (guild_id.to_owned(), event_id.to_owned(), recurrence_id.to_owned());
        let mut out = Vec::new();
        if manage && !occurrence.canceled {
            let (g, e, r) = ids();
            out.push(item("Edit this Event", move |this, window, cx| this.open_occurrence_editor(g.clone(), e.clone(), r.clone(), window, cx)).icon(Glyph::Pen).entry());
        }
        if manage && occurrence.canceled {
            let (g, e, r) = ids();
            out.push(item("Restore Event", move |this, _, cx| this.restore_occurrence(&g, &e, &r, cx)).icon(Glyph::Calendar).entry());
        }
        let link = occurrence_link(event, recurrence_id);
        out.push(item("Copy Event Link", move |this, _, cx| this.copy_event_link(link.clone(), cx)).icon(Glyph::Link).entry());
        if !occurrence.canceled {
            let label = if self.occurrence_interested(event, recurrence_id) { "Mark as Not Interested" } else { "Mark as Interested" };
            out.push(submenu(label, self.interested_entries(guild_id, event_id, Some(recurrence_id))).icon(Glyph::Bell).entry());
        }
        if manage && !occurrence.canceled {
            let (g, e, r) = ids();
            out.push(Entry::Separator);
            out.push(item("Cancel this Event", move |this, _, cx| this.confirm_cancel_occurrence(g.clone(), e.clone(), r.clone(), cx)).danger().icon(Glyph::CalendarX).entry());
        }
        out
    }

    fn open_occurrence_menu(&mut self, guild_id: String, event_id: String, recurrence_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let at = window.mouse_position();
        self.open_live_menu(at, move |this| this.occurrence_menu(&guild_id, &event_id, &recurrence_id), cx);
    }

    fn find_occurrence(&self, guild_id: &str, event_id: &str, recurrence_id: &str) -> Option<(ScheduledEvent, Occurrence)> {
        let event = self.scheduled_event(guild_id, event_id)?;
        let occurrence = occurrences(event, Utc::now(), usize::MAX).into_iter().find(|o| o.recurrence_id == recurrence_id)?;
        Some((event.clone(), occurrence))
    }

    // Writes an occurrence's exception at once, putting the old one back if Discord refuses.
    fn send_exception(&mut self, event: &ScheduledEvent, recurrence_id: String, (method, path, body): (Method, String, Option<Value>), cx: &mut Context<Self>) {
        let after = exception_from(&recurrence_id, body.as_ref());
        let before = event.exceptions.iter().find(|x| x.id == recurrence_id).cloned();
        let (g, e) = (event.guild_id.clone(), event.id.clone());
        let (g2, e2, r2) = (g.clone(), e.clone(), recurrence_id.clone());
        let swap = |this: &mut Waffle, g: &str, e: &str, id: &str, exception: Option<EventException>| {
            let Some(event) = this.scheduled_event_mut(g, e) else { return };
            event.exceptions.retain(|x| x.id != id);
            event.exceptions.extend(exception);
        };
        self.rest_optimistic(method, path, body, move |this| swap(this, &g, &e, &recurrence_id, after), move |this| swap(this, &g2, &e2, &r2, before), cx);
    }

    fn confirm_cancel_occurrence(&mut self, guild_id: String, event_id: String, recurrence_id: String, cx: &mut Context<Self>) {
        self.confirm_delete(
            "Cancel Event?",
            "Are you sure you want to cancel this event?",
            "Cancel Event",
            move |this, _, cx| {
                let Some((event, occurrence)) = this.find_occurrence(&guild_id, &event_id, &recurrence_id) else { return };
                let (method, path, body) = exception_request(&event, &occurrence, occurrence.start, occurrence.end, true);
                this.send_exception(&event, recurrence_id.clone(), (method, path, Some(body)), cx);
            },
            cx,
        );
        self.set_dialog_cancel("Never Mind");
    }

    fn restore_occurrence(&mut self, guild_id: &str, event_id: &str, recurrence_id: &str, cx: &mut Context<Self>) {
        let Some((event, occurrence)) = self.find_occurrence(guild_id, event_id, recurrence_id) else { return };
        let request = restore_request(&event, &occurrence);
        self.send_exception(&event, recurrence_id.to_owned(), request, cx);
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

    // Starting (`163765`): a stage event opens its stage, made first when it's gone; the rest switch to live.
    // Then you join the event's channel.
    fn start_event(&mut self, cx: &mut Context<Self>) {
        let Some(start) = self.events.start.take() else { return };
        let Some(event) = self.scheduled_event(&start.guild_id, &start.event_id).cloned() else { return };
        let (g, e) = (event.guild_id.clone(), event.id.clone());
        if event.entity_type != ScheduledEvent::STAGE {
            let path = format!("/guilds/{g}/scheduled-events/{e}");
            return self.rest(Method::PATCH, path, Some(json!({ "status": ScheduledEvent::ACTIVE })), move |this, cx| this.event_started(&g, &e, cx), cx);
        }
        let stage = event.channel_id.as_deref().and_then(|c| self.event_guild(&g)?.channel(c)).cloned();
        match stage {
            Some(stage) => self.open_stage(&event, &stage.id, start.notify, cx),
            None => self.create_stage(event, start.notify, cx),
        }
    }

    fn open_stage(&mut self, event: &ScheduledEvent, channel_id: &str, notify: bool, cx: &mut Context<Self>) {
        let body = json!({ "channel_id": channel_id, "topic": event.name, "privacy_level": PRIVATE, "guild_scheduled_event_id": event.id, "send_start_notification": notify });
        let (g, e) = (event.guild_id.clone(), event.id.clone());
        self.rest(Method::POST, "/stage-instances".into(), Some(body), move |this, cx| this.event_started(&g, &e, cx), cx);
    }

    // The stage is named after the event (`name.substring(0,100)`).
    // Not verified: whether Discord also points the event at the new stage; here only the stage instance names it.
    fn create_stage(&mut self, event: ScheduledEvent, notify: bool, cx: &mut Context<Self>) {
        let name: String = event.name.chars().take(100).collect();
        let path = format!("/guilds/{}/channels", event.guild_id);
        self.call_json(
            Method::POST,
            path,
            Some(json!({ "name": name, "type": STAGE_TYPE })),
            move |this, result, cx| {
                let stage = match result.map(|v| discord::parse_channel(&v, &event.guild_id)) {
                    Ok(Some(stage)) => stage,
                    Ok(None) => return,
                    Err(error) => return this.show_toast(error, cx),
                };
                if let Some(guild) = this.guilds.iter_mut().find(|g| g.id == event.guild_id) {
                    if guild.channel(&stage.id).is_none() {
                        guild.channels.push(stage.clone());
                    }
                    if let Some(listed) = guild.events.iter_mut().find(|e| e.id == event.id) {
                        listed.channel_id = Some(stage.id.clone());
                    }
                }
                this.open_stage(&event, &stage.id, notify, cx);
            },
            cx,
        );
    }

    fn event_started(&mut self, guild_id: &str, event_id: &str, cx: &mut Context<Self>) {
        let Some(event) = self.scheduled_event_mut(guild_id, event_id) else { return };
        event.status = ScheduledEvent::ACTIVE;
        let channel_id = event.channel_id.clone();
        let Some(channel) = channel_id.and_then(|c| self.event_guild(guild_id)?.channel(&c).cloned()) else { return };
        self.join_voice_in(Some(guild_id.to_owned()), channel, cx);
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
        let series = self.scheduled_event(&guild_id, &event_id).is_some_and(|e| e.recurrence.is_some());
        let (body, button) = match series {
            true => ("Are you sure you want to cancel this event? Canceling this event will also cancel all events in this series.", "Cancel all Events"),
            false => ("Are you sure you want to cancel this event?", "Cancel Event"),
        };
        self.confirm_delete("Cancel Event?", body, button, move |this, _, cx| this.cancel_event(&guild_id, &event_id, cx), cx);
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

    // Every layer closes, for a jump elsewhere; your occurrence answers stay.
    fn leave_events(&mut self) {
        let rsvps = std::mem::take(&mut self.events.rsvps);
        self.events = Events { rsvps, ..Events::default() };
    }

    // The empty list's "Server Settings > Roles".
    fn open_roles_settings(&mut self, guild_id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_events();
        self.open_server_settings(guild_id, window, cx);
        self.open_server_page(crate::server_settings::access::Page::Roles, cx);
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
        let location = input(event.as_ref().and_then(|e| e.location.clone()).unwrap_or_default(), "Add a location, link, or something.", window, cx);
        let topic = input(event.as_ref().map(|e| e.name.clone()).unwrap_or_default(), "What's your event?", window, cx);
        let (start_date, start_time) = (date_state(start, window, cx), time_state(start, window, cx));
        let (end_date, end_time) = (date_state(end, window, cx), time_state(end, window, cx));
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
            cover: Cover::Kept,
            occurrence: None,
            link: None,
            busy: false,
        });
        cx.notify();
    }

    // "Edit this Event": the wizard on one occurrence's times, saved as its exception.
    // Not verified: Discord's single-occurrence modal (`462166`) wasn't fetched; the exception carries only times,
    // so only times are offered here.
    fn open_occurrence_editor(&mut self, guild_id: String, event_id: String, recurrence_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, occurrence)) = self.find_occurrence(&guild_id, &event_id, &recurrence_id) else { return };
        self.open_event_wizard(guild_id, Some(event_id), window, cx);
        let Some(w) = self.events.wizard.as_mut() else { return };
        w.step = Step::Info;
        let (start, end) = (occurrence.start.with_timezone(&Local), occurrence.end.unwrap_or(occurrence.start + Duration::hours(1)).with_timezone(&Local));
        let pickers = (w.start_date.clone(), w.start_time.clone(), w.end_date.clone(), w.end_time.clone());
        w.occurrence = Some(occurrence);
        pickers.0.update(cx, |s, cx| s.set_date(start.date_naive(), window, cx));
        pickers.1.update(cx, |s, cx| s.set_time(start.time(), window, cx));
        pickers.2.update(cx, |s, cx| s.set_date(end.date_naive(), window, cx));
        pickers.3.update(cx, |s, cx| s.set_time(end.time(), window, cx));
    }

    // Pick a cover, held as a data URI until the save goes out.
    fn pick_cover(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let Ok(bytes) = std::fs::read(&path) else { return };
            let data = crate::server_settings::api::image_data(crate::server_settings::view::mime_of(&path), &bytes);
            this.update(cx, |this, cx| this.set_wizard(cx, |w| w.cover = Cover::Picked { path, data })).ok();
        })
        .detach();
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
        (picked_instant(&w.start_date, &w.start_time, cx), picked_instant(&w.end_date, &w.end_time, cx))
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
        if self.events.wizard.as_ref().is_some_and(|w| w.occurrence.is_some()) {
            return self.send_occurrence(cx);
        }
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
        let mut body = json!({
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
        if let Some(image) = cover_field(&w.cover) {
            body["image"] = image;
        }
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

    // One occurrence's new times, as its exception; the wizard closes once it's sent.
    fn send_occurrence(&mut self, cx: &mut Context<Self>) {
        if !self.wizard_problems(cx).step_ok(Step::Review) {
            return;
        }
        let (start, end) = self.wizard_times(cx);
        let Some(w) = self.events.wizard.take() else { return };
        let (Some(start), Some(occurrence)) = (start, w.occurrence) else { return };
        let Some(event) = w.editing.and_then(|id| self.scheduled_event(&w.guild_id, &id).cloned()) else { return };
        let end = if event.entity_type == ScheduledEvent::EXTERNAL { end } else { occurrence.end.map(|e| start + (e - occurrence.start)) };
        let (method, path, body) = exception_request(&event, &occurrence, start, end, false);
        self.send_exception(&event, occurrence.recurrence_id.clone(), (method, path, Some(body)), cx);
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
                    .child(ui::live_badge())
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
        let gid = guild.id.clone();
        let roles = guild.can(self.my_id(), perm::MANAGE_ROLES).then(|| {
            let link = div()
                .id("events-roles-link")
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::accent_ink(cx))
                .cursor_pointer()
                .hover(|s| s.underline())
                .child("Server Settings > Roles")
                .on_click(cx.listener(move |this, _, window, cx| this.open_roles_settings(gid.clone(), window, cx)));
            div().max_w(px(320.)).text_center().flex().flex_wrap().justify_center().gap_x(px(4.)).text_size(px(12.)).text_color(palette::faint(cx)).child("You can give other people permission to create events in").child(link).child(".")
        });
        ui::empty_state("events-empty", Mood::Idle, "There are no upcoming events.", "Schedule an event for any planned activity in your server.", cx).children(roles).into_any_element()
    }

    // One card (`.ev2`): when, who and how many; the name and two lines of description; where, and the actions.
    // `actions` off draws the wizard's preview.
    fn event_card(&self, event: &ScheduledEvent, now: &DateTime<Local>, actions: bool, cx: &mut Context<Self>) -> AnyElement {
        let when = when(event, now);
        let ink = phase_ink(when.phase, cx);
        let (strong, muted) = (palette::strong(cx), palette::muted(cx));
        let canceled = when.phase == Phase::Canceled;
        let cover = cover_source(event).map(|source| ui::media(source, ui::all(ui::RADIUS_MD)).w_full().h(px(140.)).mb(px(6.)));
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
            .children(event.description.as_deref().map(|d| description_view(&format!("event-card-md-{}", event.id), d, cx).max_h(px(40.)).overflow_hidden()))
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
                            this.leave_events();
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
            let series = event.recurrence.is_some();
            let next = series.then(|| next_occurrence(event, Utc::now())).flatten();
            let on = next.as_ref().map_or(event.me_interested, |o| self.occurrence_interested(event, &o.recurrence_id));
            out.push(
                ui::button(key("interested"), Tone::Secondary, cx)
                    .small()
                    .toggled(on, cx)
                    .child(Icon::new(if on { Glyph::BellRinging } else { Glyph::Bell }).size(px(16.)))
                    .child("Interested")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        match series {
                            true => this.open_series_interested(g.clone(), e.clone(), window, cx),
                            false => this.toggle_interested(g.clone(), e.clone(), cx),
                        }
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
                        this.leave_events();
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
        let cover = cover_source(event).map(|source| ui::media(source, ui::all(ui::RADIUS_LG)).w_full().h(px(160.)));
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
            .children(event.description.as_deref().map(|d| description_view(&format!("event-info-md-{}", event.id), d, cx)))
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
            .children(self.occurrence_rows(event, cx))
            .into_any_element()
    }

    // A series' next occurrences, each with its menu; canceled ones stay, marked, so they can be restored.
    fn occurrence_rows(&self, event: &ScheduledEvent, cx: &mut Context<Self>) -> Option<AnyElement> {
        let list = occurrences(event, Utc::now(), OCCURRENCES_SHOWN);
        if list.is_empty() {
            return None;
        }
        let now = Local::now();
        let rows = list.into_iter().map(|o| {
            let (g, e, r) = (event.guild_id.clone(), event.id.clone(), o.recurrence_id.clone());
            let start = o.start.with_timezone(&Local);
            let interested = !o.canceled && self.occurrence_interested(event, &o.recurrence_id);
            let more = ui::icon_button(SharedString::from(format!("event-occurrence-more-{}", o.recurrence_id)), Glyph::MoreVertical, "More event options", cx).on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open_occurrence_menu(g.clone(), e.clone(), r.clone(), window, cx);
            }));
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .min_w_0()
                .py(px(4.))
                .child(div().flex_none().text_color(if o.canceled { palette::danger_ink(cx) } else { palette::muted(cx) }).child(Icon::new(if o.canceled { Glyph::CalendarX } else { Glyph::Calendar }).size(px(16.))))
                .child(div().flex_1().min_w_0().truncate().text_color(if o.canceled { palette::muted(cx) } else { palette::text(cx) }).when(o.canceled, |d| d.line_through()).child(date_line(&start, &now)))
                .when(o.canceled, |d| d.child(div().flex_none().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::danger_ink(cx)).child("Canceled")))
                .when(interested, |d| d.child(div().flex_none().text_color(palette::accent_ink(cx)).child(Icon::new(Glyph::BellRinging).size(px(14.)))))
                .child(more)
        });
        let head = div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(palette::muted(cx)).child("Events in this series");
        Some(div().flex().flex_col().gap(px(2.)).pt(px(4.)).child(head).children(rows).into_any_element())
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
        let (title, subtitle) = match (w.step, &w.occurrence) {
            (_, Some(_)) => ("Edit this Event", Some("Only this event in the series changes.")),
            (Step::Location, None) => ("Where is your event?", Some("So no one gets lost on where to go.")),
            (Step::Info, None) => ("What's your event about?", Some("Fill out the details of your event.")),
            (Step::Review, None) => ("Here's a preview of your event.", None),
            (Step::Success, None) => ("All set. Now share your event!", None),
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
        let step = match (w.step, w.occurrence.is_some()) {
            (_, true) => self.wizard_times_fields(w, &problems, window, cx),
            (Step::Location, false) => self.wizard_location(w, &problems, cx),
            (Step::Info, false) => self.wizard_info(w, &problems, window, cx),
            (Step::Review, false) => self.wizard_review(w, cx),
            (Step::Success, false) => wizard_success(w, cx),
        };
        let ok = problems.step_ok(if w.occurrence.is_some() { Step::Review } else { w.step });
        let body = ui::dialog_body()
            .child(head)
            .when(w.step != Step::Success && w.occurrence.is_none(), |d| d.child(stepper(w.step, cx)))
            .child(step)
            .when(w.step != Step::Success, |d| d.child(wizard_foot(w, ok, cx)));
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

    fn wizard_info(&self, w: &Wizard, problems: &Problems, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let typed_topic = !w.topic.read(cx).value().trim().is_empty();
        let focus = w.topic.read(cx).focus_handle(cx);
        let topic_field = ui::text_field("event-topic", Input::new(&w.topic).appearance(false), &focus).error(problems.topic.is_some() && typed_topic);
        let topic = ui::field("Event Topic", cx).child(topic_field).children(problems.topic.filter(|_| typed_topic).map(|p| ui::field_error(p, cx)));
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
            .child(self.wizard_times_fields(w, problems, window, cx))
            .child(frequency)
            .child(description)
            .child(self.wizard_cover(w, cx))
            .into_any_element()
    }

    // Start Date and Start Time, then End Date and End Time for somewhere else, each pair's error under it.
    fn wizard_times_fields(&self, w: &Wizard, problems: &Problems, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let pair = |a: AnyElement, b: AnyElement| div().flex().gap(px(12.)).child(div().flex_1().min_w_0().child(a)).child(div().flex_1().min_w_0().child(b));
        let start = pair(
            ui::field("Start Date", cx).child(date_field(&w.start_date, problems.start.is_some(), window, cx)).into_any_element(),
            ui::field("Start Time", cx).child(time_field(&w.start_time, problems.start.is_some(), window, cx)).into_any_element(),
        );
        let end = (w.entity_type == ScheduledEvent::EXTERNAL).then(|| {
            pair(
                ui::field("End Date", cx).child(date_field(&w.end_date, problems.end.is_some(), window, cx)).into_any_element(),
                ui::field("End Time", cx).child(time_field(&w.end_time, problems.end.is_some(), window, cx)).into_any_element(),
            )
        });
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(start)
            .children(problems.start.map(|p| ui::field_error(p, cx)))
            .children(end)
            .children(problems.end.map(|p| ui::field_error(p, cx)))
            .into_any_element()
    }

    // "Cover Image": Upload Cover Image, or the picked or current cover with Change Image and Remove Image.
    fn wizard_cover(&self, w: &Wizard, cx: &mut Context<Self>) -> AnyElement {
        let kept = w.editing.as_deref().and_then(|id| self.scheduled_event(&w.guild_id, id)).and_then(|e| e.image.clone());
        let preview = ScheduledEvent { id: w.editing.clone().unwrap_or_default(), image: wizard_image(&w.cover, kept), ..Default::default() };
        let hint = ui::field_hint("We recommend an image that's at least 800px wide and 320px tall.", None, cx);
        let field = ui::field("Cover Image", cx);
        let Some(source) = cover_source(&preview) else {
            let upload = ui::button("event-cover-upload", Tone::Secondary, cx).small().child(Icon::new(Glyph::Upload).size(px(16.))).child("Upload Cover Image").on_click(cx.listener(|this, _, _, cx| this.pick_cover(cx)));
            return field.child(div().flex().child(upload)).child(hint).into_any_element();
        };
        let change = ui::button("event-cover-change", Tone::Secondary, cx).small().child("Change Image").on_click(cx.listener(|this, _, _, cx| this.pick_cover(cx)));
        let remove = ui::button("event-cover-remove", Tone::Ghost, cx).small().child("Remove Image").on_click(cx.listener(|this, _, _, cx| this.set_wizard(cx, |w| w.cover = Cover::Removed)));
        field
            .child(ui::media(source, ui::all(ui::RADIUS_MD)).w_full().h(px(120.)))
            .child(div().flex().gap(px(8.)).child(change).child(remove))
            .child(hint)
            .into_any_element()
    }

    fn wizard_review(&self, w: &Wizard, cx: &mut Context<Self>) -> AnyElement {
        let (start, end) = self.wizard_times(cx);
        let external = w.entity_type == ScheduledEvent::EXTERNAL;
        let kept = w.editing.as_deref().and_then(|id| self.scheduled_event(&w.guild_id, id)).and_then(|e| e.image.clone());
        let preview = ScheduledEvent {
            id: w.editing.clone().unwrap_or_else(|| "preview".into()),
            image: wizard_image(&w.cover, kept),
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

// A description as Discord renders it: markdown, line breaks kept, custom emoji as images (the profile bio's way).
fn description_view(id: &str, text: &str, cx: &App) -> gpui_kit::Div {
    let view = TextView::markdown(SharedString::from(id.to_owned()), crate::chat::inline_emoji(&crate::chat::discord_breaks(text)));
    div().min_w_0().text_size(px(14.)).line_height(px(20.)).text_color(palette::text(cx)).child(view)
}

// gpui-kit's pickers in Waffle's field frame: the frame's edge takes the accent while the picker has focus
// (or its calendar is open) and the danger ink on an error. The calendar is deferred, so the dialog can't clip it.
fn picker_frame(focused: bool, error: bool, cx: &App) -> gpui_kit::Div {
    let edge = match (error, focused) {
        (true, _) => palette::danger_ink(cx),
        (false, true) => palette::accent(),
        (false, false) => palette::line_strong(cx),
    };
    ui::field_frame(cx).w_full().border_color(edge)
}

fn date_field(state: &Entity<DatePickerState>, error: bool, window: &mut Window, cx: &App) -> gpui_kit::Div {
    let focused = state.read(cx).focus_handle(cx).contains_focused(window, cx);
    picker_frame(focused, error, cx).px_0().child(DatePicker::new(state).appearance(false))
}

fn time_field(state: &Entity<TimeFieldState>, error: bool, window: &mut Window, cx: &App) -> gpui_kit::Div {
    let focused = state.read(cx).focus_handle(cx).contains_focused(window, cx);
    let field = TimeField::new(state).border_0().px_0().h_full().bg(gpui_kit::transparent_black());
    picker_frame(focused, error, cx).child(div().flex_none().text_color(palette::faint(cx)).child(Icon::new(Glyph::Clock).size(px(16.)))).child(field)
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
        _ if w.occurrence.is_some() => "Save Event",
        (Step::Review, Some(_)) => "Save Event",
        (Step::Review, None) => "Create Event",
        _ => "Next",
    };
    let occurrence = w.occurrence.is_some();
    let review = w.step == Step::Review || occurrence;
    let busy = w.busy.then_some(if w.editing.is_some() { "Saving…" } else { "Creating…" });
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .when(w.step != Step::Location && !occurrence, |d| d.child(ui::button("event-wizard-back", Tone::Secondary, cx).small().child("Back").on_click(cx.listener(|this, _, _, cx| this.wizard_step(false, cx)))))
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

    fn weekly(start: DateTime<Utc>) -> ScheduledEvent {
        let mut e = event(&start.to_rfc3339(), None, 1, ScheduledEvent::VOICE);
        e.recurrence = Frequency::Weekly.rule(start, start.date_naive());
        e
    }

    #[test]
    fn recurrence_id_is_the_start_second_as_a_snowflake() {
        let start = Utc.with_ymd_and_hms(2026, 11, 28, 19, 0, 0).unwrap();
        let id = recurrence_id(start + Duration::milliseconds(750));
        assert_eq!(id, recurrence_id(start));
        let ms = (id.parse::<u64>().unwrap() >> 22) as i64 + DISCORD_EPOCH;
        assert_eq!(ms, start.timestamp_millis());
        assert_eq!(recurrence_id(Utc.with_ymd_and_hms(2015, 1, 1, 0, 0, 1).unwrap()), (1000u64 << 22).to_string());
    }

    #[test]
    fn series_expands_and_applies_exceptions() {
        let start = Utc.with_ymd_and_hms(2026, 11, 28, 19, 0, 0).unwrap();
        let mut e = weekly(start);
        let second = start + Duration::weeks(1);
        e.exceptions = vec![EventException { id: recurrence_id(second), start: Some(second.to_rfc3339()), end: None, canceled: true }];
        let list = occurrences(&e, start, 3);
        assert_eq!(list.iter().map(|o| o.original).collect::<Vec<_>>(), [start, second, start + Duration::weeks(2)]);
        assert_eq!(list.iter().map(|o| o.canceled).collect::<Vec<_>>(), [false, true, false]);
        assert!(list[1].excepted && !list[0].excepted);
        assert_eq!(next_occurrence(&e, start + Duration::hours(1)).map(|o| o.original), Some(start + Duration::weeks(2)));

        let monthly = json!({ "frequency": 1, "interval": 1, "by_n_weekday": [{ "n": 4, "day": 5 }] });
        let starts = series_starts(&monthly, start, start + Duration::days(70));
        assert_eq!(starts.iter().map(|t| t.date_naive().to_string()).collect::<Vec<_>>(), ["2026-11-28", "2026-12-26", "2027-01-23"]);
    }

    #[test]
    fn exceptions_post_new_patch_existing_and_restore() {
        let start = Utc.with_ymd_and_hms(2026, 11, 28, 19, 0, 0).unwrap();
        let e = weekly(start);
        let fresh = occurrence(&[], start, None);
        let (method, path, body) = exception_request(&e, &fresh, start, None, true);
        assert_eq!((method, path.as_str()), (Method::POST, "/guilds/1/scheduled-events/42/exceptions"));
        assert_eq!(body, json!({ "original_scheduled_start_time": start.to_rfc3339(), "scheduled_start_time": start.to_rfc3339(), "scheduled_end_time": null, "is_canceled": true }));

        let canceled = occurrence(&[EventException { id: recurrence_id(start), start: None, end: None, canceled: true }], start, None);
        let (method, path, body) = exception_request(&e, &canceled, start, None, true);
        assert_eq!((method, path), (Method::PATCH, format!("/guilds/1/scheduled-events/42/exceptions/{}", recurrence_id(start))));
        assert!(body.get("original_scheduled_start_time").is_none());
        let (method, path, body) = restore_request(&e, &canceled);
        assert_eq!((method, path, body), (Method::DELETE, format!("/guilds/1/scheduled-events/42/exceptions/{}", recurrence_id(start)), None));

        let later = start + Duration::hours(2);
        let moved = occurrence(&[EventException { id: recurrence_id(start), start: Some(later.to_rfc3339()), end: None, canceled: true }], start, None);
        let (method, _, body) = restore_request(&e, &moved);
        assert_eq!(method, Method::PATCH);
        assert_eq!(body.as_ref().and_then(|b| b["is_canceled"].as_bool()), Some(false));
        assert_eq!(exception_from("7", body.as_ref()).map(|x| (x.start, x.canceled)), Some((Some(later.to_rfc3339()), false)));
    }

    #[test]
    fn cover_sends_data_null_or_nothing() {
        let picked = Cover::Picked { path: PathBuf::from("/tmp/cover.png"), data: "data:image/png;base64,AQID".into() };
        assert_eq!(cover_field(&picked), Some(json!("data:image/png;base64,AQID")));
        assert_eq!(cover_field(&Cover::Removed), Some(Value::Null));
        assert_eq!(cover_field(&Cover::Kept), None);
        assert_eq!(wizard_image(&picked, Some("hash".into())).as_deref(), Some("/tmp/cover.png"));
        assert_eq!(wizard_image(&Cover::Removed, Some("hash".into())), None);
    }

    #[test]
    fn rsvps_split_series_from_single_occurrences() {
        let list = json!([
            { "guild_scheduled_event_id": "1", "response": 1 },
            { "guild_scheduled_event_id": "2", "response": 0 },
            { "guild_scheduled_event_id": "1", "guild_scheduled_event_exception_id": "9", "response": 0 },
        ]);
        let (series, single) = rsvps_of(&list);
        assert_eq!(series, ["1"]);
        assert_eq!(single, [(("1".to_owned(), "9".to_owned()), false)]);
    }
}
