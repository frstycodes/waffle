// What an event says about itself, derived: its phase and wording, the repeat rules and occurrences,
// the calendar files and links, and the permissions that gate it.
use super::*;

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

pub(super) fn parse_time(iso: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(iso).ok().map(|t| t.with_timezone(&Utc))
}

// moment's `LT`: "7:00 PM".
pub(super) fn clock<Tz: TimeZone>(t: &DateTime<Tz>) -> String
where
    Tz::Offset: Display,
{
    t.format("%-I:%M %p").to_string()
}

// Discord's date line (`974930.CC`): "Today at" / "Tomorrow at" inside a day, else "Sat Nov 28th · 7:00 PM",
// with the year when it isn't this one.
pub(super) fn date_line<Tz: TimeZone>(t: &DateTime<Tz>, now: &DateTime<Tz>) -> String
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
pub(super) fn range_line<Tz: TimeZone>(start: &DateTime<Tz>, end: Option<&DateTime<Tz>>, now: &DateTime<Tz>) -> String
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

pub(super) fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

pub(super) fn phase_ink(phase: Phase, cx: &gpui_kit::App) -> Hsla {
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

pub(super) fn manages(perms: u64, me: &str, event: &ScheduledEvent) -> bool {
    perms & perm::MANAGE_EVENTS != 0 || (perms & perm::CREATE_EVENTS != 0 && event.creator_id.as_deref() == Some(me))
}

pub(super) fn within_start_window(event: &ScheduledEvent, now: DateTime<Utc>) -> bool {
    parse_time(&event.start).is_some_and(|s| s <= now + Duration::minutes(START_WINDOW_MINUTES))
}

pub(crate) fn event_link(event: &ScheduledEvent) -> String {
    format!("https://discord.com/events/{}/{}", event.guild_id, event.id)
}

// The list's events, in the store's order: live first, then by start, then by id. Discord lists only
// upcoming ones; canceled occurrences stay, drawn as canceled. Events in channels you can't see are hidden.
pub(super) fn listed<'a>(guild: &'a Guild, me: &str) -> Vec<&'a ScheduledEvent> {
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
pub(super) fn rsvps_of(list: &Value) -> (Vec<String>, Vec<(OccurrenceKey, bool)>) {
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

pub(super) fn upcoming_count(guild: &Guild, me: &str) -> usize {
    listed(guild, me).iter().filter(|e| e.upcoming()).count()
}

pub(super) fn events_title(count: usize) -> String {
    match count {
        0 => "Events".into(),
        1 => "1 Event".into(),
        n => format!("{n} Events"),
    }
}

impl Frequency {
    pub(super) const ALL: [Frequency; 7] = [Frequency::Once, Frequency::Weekly, Frequency::Biweekly, Frequency::Monthly, Frequency::Yearly, Frequency::Daily, Frequency::Weekdays];

    // Weekdays only for a start on a weekday.
    pub(super) fn options(start: NaiveDate) -> Vec<Frequency> {
        let weekday = start.weekday().num_days_from_monday() < 5;
        Self::ALL.into_iter().filter(|f| *f != Frequency::Weekdays || weekday).collect()
    }

    pub(super) fn label(self, start: NaiveDate) -> String {
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
    pub(super) fn of_rule(rule: &Value) -> Frequency {
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
    pub(super) fn rule(self, start: DateTime<Utc>, local_day: NaiveDate) -> Option<Value> {
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

pub(super) fn nth_word(n: u32) -> &'static str {
    ["first", "second", "third", "fourth", "fifth"].get(n.saturating_sub(1) as usize).copied().unwrap_or("last")
}

pub(super) fn repeat_label(event: &ScheduledEvent) -> Option<String> {
    let rule = event.recurrence.as_ref()?;
    let start = parse_time(&event.start)?.with_timezone(&Local).date_naive();
    Some(Frequency::of_rule(rule)).filter(|f| *f != Frequency::Once).map(|f| f.label(start))
}

// Calendar export

pub(super) const RRULE_DAYS: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];

pub(super) fn rrule(rule: &Value) -> Option<String> {
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

pub(super) fn ics_stamp(t: DateTime<Utc>) -> String {
    t.format("%Y%m%dT%H%M%SZ").to_string()
}

pub(super) fn ics_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace("\r\n", "\\n").replace('\n', "\\n")
}

// RFC 5545 folds content lines at 75 octets, continuing with a space.
pub(super) fn ics_fold(line: &str) -> String {
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
pub(super) fn span(event: &ScheduledEvent) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
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

pub(super) fn calendar_url(kind: Calendar, event: &ScheduledEvent, place: &str) -> Option<String> {
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

pub(super) fn ics_file_name(name: &str) -> String {
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
pub(super) fn series_starts(rule: &Value, first: DateTime<Utc>, horizon: DateTime<Utc>) -> Vec<DateTime<Utc>> {
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
pub(super) fn nth_weekday_in_month(rule: &Value, first: DateTime<Utc>, months: i64) -> Option<DateTime<Utc>> {
    let nth = &rule["by_n_weekday"][0];
    let (n, day) = (nth["n"].as_u64()? as u32, nth["day"].as_u64()? as u32);
    let index = i64::from(first.year()) * 12 + i64::from(first.month0()) + months;
    let month_start = NaiveDate::from_ymd_opt(index.div_euclid(12) as i32, index.rem_euclid(12) as u32 + 1, 1)?;
    let offset = (day + 7 - month_start.weekday().num_days_from_monday()) % 7;
    let date = month_start.with_day(1 + offset + n.checked_sub(1)? * 7)?;
    Some(date.and_time(first.time()).and_utc())
}

// The occurrences starting from `from`, each with its exception applied.
pub(super) fn occurrences(event: &ScheduledEvent, from: DateTime<Utc>, limit: usize) -> Vec<Occurrence> {
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

pub(super) fn occurrence(exceptions: &[EventException], original: DateTime<Utc>, length: Option<Duration>) -> Occurrence {
    let recurrence_id = recurrence_id(original);
    let exception = exceptions.iter().find(|x| x.id == recurrence_id);
    let start = exception.and_then(|x| x.start.as_deref()).and_then(parse_time).unwrap_or(original);
    let end = exception.and_then(|x| x.end.as_deref()).and_then(parse_time).or(length.map(|l| start + l));
    Occurrence { recurrence_id, original, start, end, canceled: exception.is_some_and(|x| x.canceled), excepted: exception.is_some() }
}

// "This event" on a series' card and menu: the next occurrence still on.
pub(super) fn next_occurrence(event: &ScheduledEvent, now: DateTime<Utc>) -> Option<Occurrence> {
    occurrences(event, now, usize::MAX).into_iter().find(|o| !o.canceled)
}

pub(super) fn occurrence_link(event: &ScheduledEvent, recurrence_id: &str) -> String {
    format!("{}/{recurrence_id}", event_link(event))
}

// The request that writes an occurrence's exception (`496092`): a new one names the occurrence by its
// original start; an existing one is patched by its id.
// Not verified: that an exception's id is the occurrence's recurrence id; the doc addresses both the same way.
pub(super) fn exception_request(event: &ScheduledEvent, occurrence: &Occurrence, start: DateTime<Utc>, end: Option<DateTime<Utc>>, canceled: bool) -> (Method, String, Value) {
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
pub(super) fn restore_request(event: &ScheduledEvent, occurrence: &Occurrence) -> (Method, String, Option<Value>) {
    if occurrence.start != occurrence.original {
        let (method, path, body) = exception_request(event, occurrence, occurrence.start, occurrence.end, false);
        return (method, path, Some(body));
    }
    (Method::DELETE, format!("/guilds/{}/scheduled-events/{}/exceptions/{}", event.guild_id, event.id, occurrence.recurrence_id), None)
}

// The exception as it stands once `body` lands; none once it's deleted.
pub(super) fn exception_from(recurrence_id: &str, body: Option<&Value>) -> Option<EventException> {
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
pub(super) fn cover_source(event: &ScheduledEvent) -> Option<ImageSource> {
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
pub(super) fn cover_field(cover: &Cover) -> Option<Value> {
    match cover {
        Cover::Kept => None,
        Cover::Picked { data, .. } => Some(json!(data)),
        Cover::Removed => Some(Value::Null),
    }
}

// What the wizard's preview shows: the picked file, nothing once removed, else the event's own.
pub(super) fn wizard_image(cover: &Cover, kept: Option<String>) -> Option<String> {
    match cover {
        Cover::Kept => kept,
        Cover::Picked { path, .. } => Some(path.to_string_lossy().into_owned()),
        Cover::Removed => None,
    }
}

// Wizard dates

pub(super) fn local_instant(day: NaiveDate, at: NaiveTime) -> Option<DateTime<Utc>> {
    Local.from_local_datetime(&day.and_time(at)).earliest().map(|t| t.with_timezone(&Utc))
}

pub(super) fn picked_instant(date: &Entity<CalendarState>, time: &Entity<TimeFieldState>, cx: &App) -> Option<DateTime<Utc>> {
    let day = date.read(cx).date().start()?;
    local_instant(day, time.read(cx).time())
}

// A date field's calendar, past days off; the date reads as the cards write it.
pub(super) fn date_state(at: DateTime<Local>, window: &mut Window, cx: &mut Context<Waffle>) -> Entity<CalendarState> {
    let today = Local::now().date_naive();
    let state = cx.new(|cx| {
        let mut state = CalendarState::new(window, cx).disabled_matcher(Matcher::custom(move |d| *d < today));
        state.set_date(at.date_naive(), window, cx);
        state
    });
    cx.subscribe(&state, |_, _, _: &CalendarEvent, cx| cx.notify()).detach(); // lint-allow(detached-task): the subscription ends when the picker entity, held in the wizard, drops
    state
}

pub(super) fn time_state(at: DateTime<Local>, window: &mut Window, cx: &mut Context<Waffle>) -> Entity<TimeFieldState> {
    let state = cx.new(|cx| {
        let mut state = TimeFieldState::new(window, cx).precision(TimePrecision::Minute).hour_cycle(HourCycle::H12);
        state.set_time(at.time(), window, cx);
        state
    });
    cx.subscribe(&state, |_, _, _: &TimeFieldEvent, cx| cx.notify()).detach(); // lint-allow(detached-task): the subscription ends when the picker entity, held in the wizard, drops
    state
}

// `jd`: the next whole hour at least an hour away.
pub(super) fn default_start(now: DateTime<Local>) -> DateTime<Local> {
    let later = now + Duration::hours(1);
    let hour = later.with_minute(0).and_then(|t| t.with_second(0)).and_then(|t| t.with_nanosecond(0)).unwrap_or(later);
    if hour < later { hour + Duration::hours(1) } else { hour }
}
