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
    AnyElement, App,
    base::{CalendarEvent, CalendarState, Matcher}, AppContext as _, Context, Entity, Focusable, FontWeight, Hsla, ImageSource, InteractiveElement, IntoElement, MouseButton, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window,
    component::{
        Icon,
        input::{Input, InputEvent, InputState, Textarea, TextareaState},
        text::TextView,
        time_field::{HourCycle, TimeFieldEvent, TimeFieldState, TimePrecision},
    },
    div,
    prelude::FluentBuilder as _,
    px,
};
use crate::discord::Method;
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

mod manage;
mod schedule;
mod views;
mod wizard;

pub(crate) use schedule::{can_create, can_manage, event_link, ics, recurrence_id, when};
use schedule::*;
use wizard::*;

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
    start_date: Entity<CalendarState>,
    start_time: Entity<TimeFieldState>,
    end_date: Entity<CalendarState>,
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
