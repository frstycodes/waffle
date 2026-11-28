// shadcn/ui's DatePicker: the date and time fields.
#![allow(unused_imports)]
use gpui_kit::*;

use super::*;
use gpui_kit::base::{CalendarState, TimeFieldState};
use crate::{icons::Glyph, illustration::Illustration, palette};

// The picked day in the dropdown trigger; the calendar opens under it and closes on a pick, a click
// outside or the trigger again.
// Keywords: datepicker, shadcn
pub struct DatePicker;

impl DatePicker {
    // The picked day in the dropdown trigger; the calendar opens under it and closes on a pick, a click
    // outside or the trigger again.
    pub fn date(id: impl Into<ElementId>, calendar: &Entity<CalendarState>, error: bool, window: &mut Window, cx: &mut App) -> AnyElement {
        super::date_field(id, calendar, error, window, cx)
    }

    // The picked time in the dropdown trigger; the list steps every quarter hour, opens scrolled to the
    // pick, and keeps an off-step time someone already set.
    pub fn time(id: impl Into<ElementId>, state: &Entity<TimeFieldState>, error: bool, window: &mut Window, cx: &mut App) -> AnyElement {
        super::time_field(id, state, error, window, cx)
    }
}

