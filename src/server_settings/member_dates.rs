// The Member since and Joined Discord filters (docs/discord-moderation-re.md §3, menu 920360 `en`): a
// preset counting back from when it was picked, or a custom range of calendar days. Discord's client
// computes a preset's start once, at the pick, so a reload keeps the same bounds.
use chrono::{DateTime, Local, Months, NaiveDate, TimeZone, Utc};
use gpui_kit::{
    AnyElement, AppContext, Context, Entity, Focusable, FontWeight, InteractiveElement, IntoElement, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window,
    component::{
        Icon,
        input::{Input, InputEvent, InputState},
    },
    div,
    prelude::FluentBuilder,
    px,
};

use super::{api::DateRange, members::Pop};
use crate::{
    Waffle,
    icons::Glyph,
    palette,
    ui::{self, Align},
};

// Discord's earliest pickable day: its launch.
pub const MIN_DAY: (i32, u32, u32) = (2015, 5, 15);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateColumn {
    Joined,
    Account,
}

impl DateColumn {
    fn key(self) -> &'static str {
        match self {
            DateColumn::Joined => "joined",
            DateColumn::Account => "account",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DateColumn::Joined => "Member since",
            DateColumn::Account => "Joined Discord",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    All,
    Hour,
    Day,
    Week,
    TwoWeeks,
    FourWeeks,
    ThreeMonths,
}

pub const PRESETS: [Preset; 7] = [Preset::All, Preset::Hour, Preset::Day, Preset::Week, Preset::TwoWeeks, Preset::FourWeeks, Preset::ThreeMonths];

impl Preset {
    pub fn label(self) -> &'static str {
        match self {
            Preset::All => "All",
            Preset::Hour => "Past 1 hr",
            Preset::Day => "Past 24 hrs",
            Preset::Week => "Past 7 days",
            Preset::TwoWeeks => "Past 2 weeks",
            Preset::FourWeeks => "Past 4 weeks",
            Preset::ThreeMonths => "Past 3 months",
        }
    }

    // Everything since this long before `now`; months are calendar months, as moment's `subtract` counts them.
    pub fn range(self, now: DateTime<Utc>) -> Option<DateRange> {
        let hours = |h: i64| Some(now - chrono::Duration::hours(h));
        let after = match self {
            Preset::All => None,
            Preset::Hour => hours(1),
            Preset::Day => hours(24),
            Preset::Week => hours(7 * 24),
            Preset::TwoWeeks => hours(14 * 24),
            Preset::FourWeeks => hours(28 * 24),
            Preset::ThreeMonths => now.checked_sub_months(Months::new(3)),
        }?;
        Some(DateRange { after_ms: Some(after.timestamp_millis()), before_ms: None })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateFilter {
    Preset(Preset),
    Custom { after: Option<NaiveDate>, before: Option<NaiveDate> },
}

impl DateFilter {
    pub fn is_active(self) -> bool {
        self != DateFilter::Preset(Preset::All)
    }

    // What the header shows beside the column's name.
    pub fn label(self) -> String {
        match self {
            DateFilter::Preset(p) => p.label().into(),
            DateFilter::Custom { after: Some(a), before: Some(b) } => format!("{} – {}", short(a), short(b)),
            DateFilter::Custom { after: Some(a), before: None } => format!("After {}", short(a)),
            DateFilter::Custom { after: None, before: Some(b) } => format!("Before {}", short(b)),
            DateFilter::Custom { .. } => "Custom range".into(),
        }
    }
}

fn short(day: NaiveDate) -> String {
    day.format("%b %-d, %Y").to_string()
}

// A custom range in the reader's days: from the start of the after day to the end of the before day.
// Discord's client sends the before day's start and drops the range when only a before day is set;
// either end alone filters here.
pub fn custom_range(after: Option<NaiveDate>, before: Option<NaiveDate>, tz: &impl TimeZone) -> Option<DateRange> {
    let start = |day: NaiveDate| tz.from_local_datetime(&day.and_hms_opt(0, 0, 0)?).earliest().map(|t| t.timestamp_millis());
    let range = DateRange { after_ms: after.and_then(start), before_ms: before.and_then(|d| d.succ_opt()).and_then(start).map(|ms| ms - 1) };
    (range.after_ms.is_some() || range.before_ms.is_some()).then_some(range)
}

// Reads a typed day: `YYYY-MM-DD`, from Discord's launch to `today`. Empty is no bound.
pub fn parse_day(text: &str, today: NaiveDate) -> Result<Option<NaiveDate>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let day = NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|_| format!("\"{text}\" isn't a date like 2024-03-12."))?;
    let min = NaiveDate::from_ymd_opt(MIN_DAY.0, MIN_DAY.1, MIN_DAY.2).expect("a real day");
    if day < min {
        return Err("Pick a day from May 15, 2015 on.".into());
    }
    if day > today {
        return Err("Pick a day up to today.".into());
    }
    Ok(Some(day))
}

// One column's filter, its custom-range fields, and whether the menu shows them.
pub struct DateMenu {
    pub filter: DateFilter,
    pub after: Entity<InputState>,
    pub before: Entity<InputState>,
    pub custom_open: bool,
    pub error: Option<String>,
}

impl DateMenu {
    pub fn new(window: &mut Window, cx: &mut Context<Waffle>) -> Self {
        let mut input = |placeholder: &'static str| {
            let state = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
            cx.subscribe(&state, |_, _, _: &InputEvent, cx| cx.notify()).detach();
            state
        };
        DateMenu { filter: DateFilter::Preset(Preset::All), after: input("YYYY-MM-DD"), before: input("YYYY-MM-DD"), custom_open: false, error: None }
    }
}

impl Waffle {
    fn date_menu(&mut self, column: DateColumn) -> Option<&mut DateMenu> {
        let m = &mut self.server_settings.as_mut()?.lists.members;
        Some(match column {
            DateColumn::Joined => &mut m.joined_menu,
            DateColumn::Account => &mut m.account_menu,
        })
    }

    fn set_date_filter(&mut self, column: DateColumn, filter: DateFilter, range: Option<DateRange>, cx: &mut Context<Self>) {
        let Some(menu) = self.date_menu(column) else { return };
        (menu.filter, menu.error) = (filter, None);
        let Some(m) = self.server_settings.as_mut().map(|v| &mut v.lists.members) else { return };
        m.pop = None;
        match column {
            DateColumn::Joined => m.filter.joined = range,
            DateColumn::Account => m.filter.account = range,
        }
        self.load_members(cx);
    }

    fn pick_date_preset(&mut self, column: DateColumn, preset: Preset, cx: &mut Context<Self>) {
        self.set_date_filter(column, DateFilter::Preset(preset), preset.range(Utc::now()), cx);
    }

    fn toggle_custom_range(&mut self, column: DateColumn, open: bool, cx: &mut Context<Self>) {
        if let Some(menu) = self.date_menu(column) {
            (menu.custom_open, menu.error) = (open, None);
        }
        cx.notify();
    }

    fn apply_custom_range(&mut self, column: DateColumn, cx: &mut Context<Self>) {
        let Some(menu) = self.date_menu(column) else { return };
        let today = Local::now().date_naive();
        let days = parse_day(&menu.after.read(cx).value(), today).and_then(|a| parse_day(&menu.before.read(cx).value(), today).map(|b| (a, b)));
        let (after, before) = match days {
            Ok((Some(a), Some(b))) if b < a => return self.date_error(column, "The before date comes after the after date.".into(), cx),
            Ok(days) => days,
            Err(error) => return self.date_error(column, error, cx),
        };
        let filter = if after.is_none() && before.is_none() { DateFilter::Preset(Preset::All) } else { DateFilter::Custom { after, before } };
        self.set_date_filter(column, filter, custom_range(after, before, &Local), cx);
    }

    fn date_error(&mut self, column: DateColumn, error: String, cx: &mut Context<Self>) {
        if let Some(menu) = self.date_menu(column) {
            menu.error = Some(error);
        }
        cx.notify();
    }

    // The column's header: a menu trigger naming the active filter for members with Manage server, plain
    // text for everyone else.
    pub(super) fn date_header(&self, column: DateColumn, filterable: bool, cx: &mut Context<Self>) -> AnyElement {
        let Some(m) = self.server_settings.as_ref().map(|v| &v.lists.members) else { return div().into_any_element() };
        if !filterable {
            return div().truncate().child(column.label()).into_any_element();
        }
        let menu = match column {
            DateColumn::Joined => &m.joined_menu,
            DateColumn::Account => &m.account_menu,
        };
        let open = m.pop == Some(Pop::Date(column));
        let active = menu.filter.is_active();
        let ink = if active || open { palette::accent_ink(cx) } else { palette::muted(cx) };
        let trigger = div()
            .id(SharedString::from(format!("ss-date-{}", column.key())))
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(4.))
            .cursor_pointer()
            .text_color(ink)
            .hover(|s| s.text_color(palette::strong(cx)))
            .child(div().flex_none().child(column.label()))
            .when(active, |d| d.child(div().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).child(menu.filter.label())))
            .child(Icon::new(if active { Glyph::Filter } else { Glyph::ChevronDown }).size(px(12.)).flex_none())
            .on_click(cx.listener(move |this, _, _, cx| this.member_pop(Some(Pop::Date(column)), cx)));
        let list = open.then(|| ui::menu_dropdown(SharedString::from(format!("ss-date-menu-{}", column.key())), 20., Align::Start, self.date_menu_rows(column, menu, cx), self.member_dismiss(cx), cx));
        div().relative().min_w_0().child(trigger).children(list).into_any_element()
    }

    fn date_menu_rows(&self, column: DateColumn, menu: &DateMenu, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let key = column.key();
        let accent = palette::accent_ink(cx);
        if menu.custom_open {
            let field = |id: &str, label: &'static str, state: &Entity<InputState>, cx: &mut Context<Self>| {
                let focus = state.read(cx).focus_handle(cx);
                ui::field(label, cx).px(px(10.)).child(ui::text_field(SharedString::from(format!("ss-date-{key}-{id}")), Input::new(state).appearance(false), &focus)).into_any_element()
            };
            let back = ui::menu_row(SharedString::from(format!("ss-date-{key}-back")), cx).child(Icon::new(Glyph::ChevronLeft).size(px(16.))).child("Custom range").on_click(cx.listener(move |this, _, _, cx| this.toggle_custom_range(column, false, cx)));
            let apply = ui::button(SharedString::from(format!("ss-date-{key}-apply")), ui::Tone::Primary, cx).small().child("Apply").on_click(cx.listener(move |this, _, _, cx| this.apply_custom_range(column, cx)));
            let error = menu.error.clone().map(|e| ui::field_error(e, cx).px(px(10.)).into_any_element());
            return [back.into_any_element(), field("after", "After date", &menu.after, cx), field("before", "Before date", &menu.before, cx)]
                .into_iter()
                .chain(error)
                .chain([div().px(px(10.)).py(px(6.)).flex().justify_end().child(apply).into_any_element()])
                .collect();
        }
        let chosen = |p: Preset| menu.filter == DateFilter::Preset(p);
        let mut rows: Vec<AnyElement> = PRESETS
            .into_iter()
            .map(|preset| {
                ui::menu_row(SharedString::from(format!("ss-date-{key}-{preset:?}")), cx)
                    .child(div().flex_1().min_w_0().truncate().child(preset.label()))
                    .child(div().text_color(accent).when(!chosen(preset), |d| d.invisible()).child(Icon::new(Glyph::Checkmark).size(px(16.))))
                    .on_click(cx.listener(move |this, _, _, cx| this.pick_date_preset(column, preset, cx)))
                    .into_any_element()
            })
            .collect();
        let custom = matches!(menu.filter, DateFilter::Custom { .. });
        rows.push(div().py(px(4.)).child(ui::divider(None, cx)).into_any_element());
        rows.push(
            ui::menu_row(SharedString::from(format!("ss-date-{key}-custom")), cx)
                .child(div().flex_1().min_w_0().truncate().child(if custom { menu.filter.label() } else { "Custom range".into() }))
                .child(div().text_color(if custom { accent } else { palette::muted(cx) }).child(Icon::new(if custom { Glyph::Checkmark } else { Glyph::ChevronRight }).size(px(16.))))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_custom_range(column, true, cx)))
                .into_any_element(),
        );
        rows
    }
}

#[cfg(test)]
mod tests {
    use chrono::FixedOffset;

    use super::*;

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text).unwrap().with_timezone(&Utc)
    }

    fn ms(text: &str) -> i64 {
        at(text).timestamp_millis()
    }

    #[test]
    fn presets_count_back_from_now() {
        let now = at("2026-11-22T12:00:00Z");
        assert_eq!(Preset::All.range(now), None);
        let after = |p: Preset| p.range(now).and_then(|r| r.after_ms);
        assert_eq!(after(Preset::Hour), Some(ms("2026-11-22T11:00:00Z")));
        assert_eq!(after(Preset::Day), Some(ms("2026-11-21T12:00:00Z")));
        assert_eq!(after(Preset::Week), Some(ms("2026-11-15T12:00:00Z")));
        assert_eq!(after(Preset::TwoWeeks), Some(ms("2026-11-08T12:00:00Z")));
        assert_eq!(after(Preset::FourWeeks), Some(ms("2026-10-25T12:00:00Z")));
        assert_eq!(after(Preset::ThreeMonths), Some(ms("2026-08-22T12:00:00Z")));
        assert!(PRESETS.iter().filter_map(|p| p.range(now)).all(|r| r.before_ms.is_none()));
    }

    #[test]
    fn preset_becomes_the_search_body() {
        let range = Preset::Week.range(at("2026-11-22T12:00:00Z"));
        let search = super::super::api::MemberSearch { joined: range, account: range, ..Default::default() };
        let body = super::super::api::members_search("1", &search, 1, None, 0).body.unwrap();
        assert_eq!(body["and_query"]["guild_joined_at"], serde_json::json!({ "range": { "gte": ms("2026-11-15T12:00:00Z") } }));
        let snowflake = super::super::api::snowflake_at(ms("2026-11-15T12:00:00Z"));
        assert_eq!(body["and_query"]["user_id"], serde_json::json!({ "range": { "gte": snowflake } }));
    }

    #[test]
    fn custom_range_covers_whole_days() {
        let day = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        let tz = FixedOffset::east_opt(2 * 3600).unwrap();
        let range = custom_range(Some(day("2024-03-01")), Some(day("2024-03-02")), &tz).unwrap();
        assert_eq!(range, DateRange { after_ms: Some(ms("2024-02-29T22:00:00Z")), before_ms: Some(ms("2024-03-02T22:00:00Z") - 1) });
        assert_eq!(custom_range(None, Some(day("2024-03-02")), &tz).unwrap().after_ms, None);
        assert_eq!(custom_range(None, None, &tz), None);
    }

    #[test]
    fn typed_days() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 22).unwrap();
        assert_eq!(parse_day(" ", today), Ok(None));
        assert_eq!(parse_day("2015-05-15", today), Ok(NaiveDate::from_ymd_opt(2015, 5, 15)));
        assert!(parse_day("2015-05-14", today).is_err());
        assert!(parse_day("2026-11-23", today).is_err());
        assert!(parse_day("12/03/2024", today).is_err());
    }
}
