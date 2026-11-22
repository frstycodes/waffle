// Date and time pickers drawn as Waffle's own dropdowns: the select trigger, and under it the menu
// surface with a calendar (gpui-base's unstyled one) or a list of times. The accent marks only the
// picked day or time.
use chrono::{Month, NaiveTime, Timelike};
use gpui_kit::{
    AnyElement, App, Bounds, ElementId, Entity, FontWeight, InteractiveElement, IntoElement, ParentElement, Pixels, ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Window,
    base::{Calendar, CalendarItem, CalendarItemKind, CalendarItemState, CalendarState, TimeFieldState},
    component::Icon,
    div, point, prelude::FluentBuilder, px, size,
};

use super::{RADIUS_SM, Squircle, control::part};
use crate::{icons::Glyph, palette};

const CELL: f32 = 36.;
const TRIGGER: f32 = 40.;
const STEP_MINUTES: u32 = 15;

// The picked day in the dropdown trigger; the calendar opens under it and closes on a pick, a click
// outside or the trigger again.
pub fn date_field(id: impl Into<ElementId>, calendar: &Entity<CalendarState>, error: bool, window: &mut Window, cx: &mut App) -> AnyElement {
    let id = id.into();
    let open = window.use_keyed_state(part(&id, "open"), cx, |_, _| false);
    let is_open = *open.read(cx);
    let value = calendar.read(cx).date().start().map_or_else(|| "Pick a date".to_owned(), |day| day.format("%a, %b %-d, %Y").to_string());
    let toggle = open.clone();
    let trigger = super::select(part(&id, "trigger"), Some(Glyph::Calendar), value, is_open, cx).invalid(error, cx).on_click(move |_, _, cx| set_open(&toggle, None, cx));
    let popover = is_open.then(|| {
        let close = open.clone();
        let grid = Calendar::new(part(&id, "grid"), calendar).label(calendar_label).item(move |item, state, _, cx| calendar_item(item, state, &close, cx));
        let card = super::surface(cx).id(part(&id, "calendar")).occlude().p(px(10.)).gap(px(4.)).child(grid);
        drop_layer(&id, card, open, 0.)
    });
    div().relative().w_full().child(trigger).children(popover).into_any_element()
}

// The picked time in the dropdown trigger; the list steps every quarter hour, opens scrolled to the
// pick, and keeps an off-step time someone already set.
pub fn time_field(id: impl Into<ElementId>, state: &Entity<TimeFieldState>, error: bool, window: &mut Window, cx: &mut App) -> AnyElement {
    let id = id.into();
    let open = window.use_keyed_state(part(&id, "open"), cx, |_, _| false);
    let list = window.use_keyed_state(part(&id, "list"), cx, |_, _| ScrollHandle::new()).read(cx).clone();
    let is_open = *open.read(cx);
    let picked = to_minute(state.read(cx).time());
    let times = time_options(picked);
    let toggle = open.clone();
    let at = times.iter().position(|t| *t == picked).unwrap_or(0);
    let scroll = list.clone();
    let trigger = super::select(part(&id, "trigger"), Some(Glyph::Clock), clock(picked), is_open, cx).invalid(error, cx).on_click(move |_, _, cx| {
        scroll.scroll_to_item(at);
        set_open(&toggle, None, cx);
    });
    let popover = is_open.then(|| {
        let (strong, accent_ink) = (palette::strong(cx), palette::accent_ink(cx));
        let rows = times.into_iter().enumerate().map(|(ix, time)| {
            let (state, close, selected) = (state.clone(), open.clone(), time == picked);
            super::menu_row(SharedString::from(format!("{id:?}-time-{ix}")), cx)
                .when(selected, |d| d.text_color(strong))
                .child(div().flex_1().font_features(crate::call::tabular()).child(clock(time)))
                .child(div().text_color(accent_ink).when(!selected, |d| d.invisible()).child(Icon::new(Glyph::Checkmark).size(px(16.))))
                .on_click(move |_, window, cx| {
                    state.update(cx, |s, cx| s.set_time(time, window, cx));
                    set_open(&close, Some(false), cx);
                    window.refresh();
                })
        });
        let card = super::surface(cx).id(part(&id, "times")).occlude().w_full().max_h(px(280.)).overflow_y_scroll().track_scroll(&list).children(rows);
        drop_layer(&id, card, open, 160.)
    });
    div().relative().w_full().child(trigger).children(popover).into_any_element()
}

fn clock(time: NaiveTime) -> String {
    time.format("%-I:%M %p").to_string()
}

// Every quarter hour of the day, plus `picked` in its place when it falls between steps.
fn time_options(picked: NaiveTime) -> Vec<NaiveTime> {
    let mut times: Vec<NaiveTime> = (0..24 * 60 / STEP_MINUTES).filter_map(|n| NaiveTime::from_hms_opt(n * STEP_MINUTES / 60, n * STEP_MINUTES % 60, 0)).collect();
    let picked = to_minute(picked);
    if let Err(at) = times.binary_search(&picked) {
        times.insert(at, picked);
    }
    times
}

fn to_minute(time: NaiveTime) -> NaiveTime {
    time.with_second(0).and_then(|t| t.with_nanosecond(0)).unwrap_or(time)
}

fn set_open(open: &Entity<bool>, to: Option<bool>, cx: &mut App) {
    open.update(cx, |open, cx| {
        *open = to.unwrap_or(!*open);
        cx.notify();
    });
}

// A dropdown's card under its trigger, at least `min_width` wide, rising from the trigger. The trigger
// sits outside the card's box, so a press on it reads as outside too and would close the card just
// before the trigger's own toggle reopened it: the card's last bounds place the trigger.
fn drop_layer(id: &ElementId, card: impl IntoElement, open: Entity<bool>, min_width: f32) -> AnyElement {
    let bounds = std::rc::Rc::new(std::cell::Cell::new(None::<Bounds<Pixels>>));
    let record = bounds.clone();
    let layer = div()
        .absolute()
        .top(px(TRIGGER + 4.))
        .left_0()
        .when(min_width > 0., |d| d.right_0().min_w(px(min_width)))
        .child(gpui_kit::canvas(move |b, _, _| record.set(Some(b)), |_, _, _, _| {}).absolute().size_full())
        .child(card)
        .on_mouse_down_out(move |event, _, cx| {
            let trigger = bounds.get().map(|b| Bounds::new(point(b.left(), b.top() - px(TRIGGER + 4.)), size(b.size.width, px(TRIGGER))));
            if !trigger.is_some_and(|t| t.contains(&event.position)) {
                set_open(&open, Some(false), cx);
            }
        });
    let id = SharedString::from(format!("{id:?}-in"));
    gpui_kit::deferred(super::rise(layer.transform_origin(point(gpui_kit::relative(0.5), gpui_kit::relative(0.))), id, std::time::Duration::ZERO)).with_priority(2).into_any_element()
}

fn calendar_label(kind: CalendarItemKind, value: i32) -> SharedString {
    let month = || Month::try_from(value as u8).map(|m| m.name()).unwrap_or_default();
    match kind {
        CalendarItemKind::Previous => "Previous month".into(),
        CalendarItemKind::Next => "Next month".into(),
        CalendarItemKind::Weekday => ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"][value.rem_euclid(7) as usize].into(),
        CalendarItemKind::MonthToggle => month().into(),
        CalendarItemKind::Month => month().get(..3).unwrap_or_default().to_owned().into(),
        _ => value.to_string().into(),
    }
}

// One calendar cell: chevrons for paging, the month and year as quiet toggles, days as squircle cells
// with the accent fill only on the picked one and accent ink on today.
fn calendar_item(item: CalendarItem, state: CalendarItemState, close: &Entity<bool>, cx: &App) -> AnyElement {
    let (strong, text, faint, hover) = (palette::strong(cx), palette::text(cx), palette::faint(cx), palette::hover(cx));
    let (accent_soft, accent_ink) = (palette::accent_soft(cx), palette::accent_ink(cx));
    let cell = item.flex().items_center().justify_center().flex_none().squircle(RADIUS_SM).text_size(px(14.)).font_features(crate::call::tabular());
    let live = !state.is_disabled();
    match state.kind() {
        CalendarItemKind::Previous | CalendarItemKind::Next => {
            let glyph = if state.kind() == CalendarItemKind::Previous { Glyph::ChevronLeft } else { Glyph::ChevronRight };
            cell.size(px(CELL)).clear_children().text_color(palette::muted(cx)).child(Icon::new(glyph).size(px(16.))).when(live, |c| c.cursor_pointer().hover(|s| s.bg(hover))).when(!live, |c| c.opacity(0.4)).into_any_element()
        }
        CalendarItemKind::MonthToggle | CalendarItemKind::YearToggle => cell
            .h(px(CELL))
            .px(px(8.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(strong)
            .cursor_pointer()
            .when(state.kind() == CalendarItemKind::MonthToggle, |c| c.ml_auto())
            .when(state.kind() == CalendarItemKind::YearToggle, |c| c.mr_auto())
            .map(|c| if state.is_active() { c.bg(accent_soft).text_color(accent_ink) } else { c.hover(|s| s.bg(hover)) })
            .into_any_element(),
        CalendarItemKind::Weekday => cell.w(px(CELL)).h(px(28.)).text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(faint).into_any_element(),
        CalendarItemKind::Day => {
            let close = close.clone();
            cell.size(px(CELL))
                .text_color(if state.is_muted() { faint } else { text })
                .when(state.is_disabled(), |c| c.opacity(0.45))
                .when(state.is_today() && !state.is_active(), |c| c.text_color(accent_ink).font_weight(FontWeight::SEMIBOLD))
                .map(|c| match (state.is_active(), live) {
                    (true, _) => c.bg(palette::accent()).text_color(palette::on_accent()).font_weight(FontWeight::SEMIBOLD),
                    (false, true) => c.cursor_pointer().hover(|s| s.bg(hover)),
                    (false, false) => c,
                })
                .when(live, |c| c.on_click(move |_, _, cx| set_open(&close, Some(false), cx)))
                .into_any_element()
        }
        CalendarItemKind::Month | CalendarItemKind::Year => cell
            .h(px(40.))
            .text_color(text)
            .cursor_pointer()
            .map(|c| if state.is_active() { c.bg(palette::accent()).text_color(palette::on_accent()).font_weight(FontWeight::SEMIBOLD) } else { c.hover(|s| s.bg(hover)) })
            .into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_options_step_by_quarter_hour_and_keep_an_off_step_pick() {
        let at = |h, m| NaiveTime::from_hms_opt(h, m, 0).unwrap();
        let on_step = time_options(at(17, 0));
        assert_eq!(on_step.len(), 96);
        assert_eq!((on_step[0], on_step[95]), (at(0, 0), at(23, 45)));
        let off_step = time_options(NaiveTime::from_hms_opt(17, 7, 42).unwrap());
        assert_eq!(off_step.len(), 97);
        let ix = off_step.iter().position(|t| *t == at(17, 7)).unwrap();
        assert_eq!((off_step[ix - 1], off_step[ix + 1]), (at(17, 0), at(17, 15)));
    }
}
