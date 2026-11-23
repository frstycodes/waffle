use gpui_kit::{
    App, Div, IntoElement, ParentElement, Pixels, Styled, Window,
    base::{Transition, TransitionId, transition},
    div,
};

use super::DUR_ROLL;
use crate::motion;

// Discord's HoverRoll travel: a line leaves (and the other arrives) a little past its own height.
const TRAVEL: f32 = 1.07;

// One line swapped for another on hover: `rest` rolls up and fades as `rolled` rises in from below, over 220ms.
pub fn hover_roll(id: impl Into<TransitionId>, rest: impl IntoElement, rolled: impl IntoElement, hovered: bool, height: Pixels, window: &mut Window, cx: &mut App) -> Div {
    let t = transition(id, if hovered { 1. } else { 0. }, Transition::new(DUR_ROLL).easing(motion::ease_out()), window, cx);
    let slot = |offset: f32, opacity: f32| div().absolute().left_0().right_0().min_w_0().top(height * offset).opacity(opacity);
    div()
        .relative()
        .min_w_0()
        .h(height)
        .overflow_hidden()
        .child(slot(-TRAVEL * t, 1. - t).child(rest))
        .child(slot(TRAVEL * (1. - t), t).child(rolled))
}
