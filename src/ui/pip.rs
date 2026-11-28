// A picture-in-picture player: a 16:9 picture with a header and a foot over it that ease in on hover.
use gpui_kit::{AnyElement, App, Div, ElementId, FontWeight, InteractiveElement, ParentElement, SharedString, Stateful, StatefulInteractiveElement, Styled, Window, component::Icon, div, px};

use super::{Button, ButtonScope, ButtonSize, ButtonVariant, DUR_POP, ICON_LG, RADIUS_MD, Squircle, TEXT_SM, col, hover_state, line, row, shadow_pop};
use crate::{icons::Glyph, motion};

// The player's size: 320 wide at 16:9.
pub const PIP_SIZE: (f32, f32) = (320., 180.);
// How far the header and the foot slide in from their edges as the chrome fades in, as the call stage's.
pub const CHROME_DRIFT: f32 = 8.;

// What sits over the picture: each corner is optional, and the header and foot each get a shade behind them.
#[derive(Default)]
pub struct PipChrome {
    pub top_left: Option<AnyElement>,
    pub top_right: Option<AnyElement>,
    pub bottom_left: Option<AnyElement>,
    pub bottom_right: Option<AnyElement>,
}

// A floating 16:9 player whose header and foot fade in and slide 8px from their edges while it's hovered, and
// back out when the pointer leaves.
// Keywords: pip, picture in picture, floating player, mini player, video overlay, stream window, hover chrome
pub fn pip(id: impl Into<ElementId>, picture: AnyElement, chrome: PipChrome, window: &mut Window, cx: &mut App) -> Stateful<Div> {
    let id = id.into();
    let (hovered, on_hover) = hover_state(id.clone(), window, cx);
    let shown = pip_reveal(&id, hovered, window, cx);
    pip_at(id, picture, chrome, shown, cx).on_hover(on_hover)
}

// How far over a player's picture its controls are shown (0 hidden, 1 shown), easing toward `shown` over 200ms.
// Keywords: pip reveal, hover fade, controls fade
pub fn pip_reveal(id: &ElementId, shown: bool, window: &mut Window, cx: &mut App) -> f32 {
    gpui_kit::base::transition(super::control::part(id, "chrome"), if shown { 1. } else { 0. }, gpui_kit::base::Transition::new(DUR_POP).easing(motion::ease_out()), window, cx)
}

// The dark fade behind a player's controls: 70% black at the edge, clear by 40% of the way in.
// Keywords: pip shade, scrim, gradient behind controls
pub fn pip_shade(angle: f32) -> gpui_kit::Background {
    gpui_kit::linear_gradient(angle, gpui_kit::linear_color_stop(gpui_kit::black().opacity(0.7), 0.), gpui_kit::linear_color_stop(gpui_kit::transparent_black(), 0.4))
}

// The player with its chrome held at `shown` (0 hidden, 1 shown), for a still of any point in the hover.
// Keywords: pip still, picture in picture state
pub fn pip_at(id: impl Into<ElementId>, picture: AnyElement, chrome: PipChrome, shown: f32, cx: &App) -> Stateful<Div> {
    let drift = CHROME_DRIFT * (1. - shown);
    let edge = |start: Option<AnyElement>, end: Option<AnyElement>, offset: f32| row().relative().top(px(offset)).gap_2().child(row().flex_1().children(start)).children(end.map(|e| div().flex_none().child(e)));
    let PipChrome { top_left, top_right, bottom_left, bottom_right } = chrome;
    let overlay = col()
        .absolute()
        .inset_0()
        .p_2()
        .justify_between()
        .bg(pip_shade(180.))
        .opacity(shown)
        .child(edge(top_left, top_right, -drift))
        .child(edge(bottom_left, bottom_right, drift));
    let foot = div().absolute().left_0().right_0().bottom_0().h_1_2().bg(pip_shade(0.)).opacity(shown);
    div()
        .id(id)
        // Floats over the app: what's under it must not take the hover or the click.
        .occlude()
        .relative()
        .w(px(PIP_SIZE.0))
        .h(px(PIP_SIZE.1))
        .flex_none()
        .squircle(RADIUS_MD)
        .overflow_hidden()
        .bg(gpui_kit::black())
        .shadow(shadow_pop(cx))
        .cursor_pointer()
        .child(picture)
        .child(foot)
        .child(overlay)
}

// The header's way back: a chevron and where it leads, in white.
// Keywords: pip title, back to call
pub fn pip_title(text: impl Into<SharedString>) -> Div {
    row().gap_1p5().text_size(TEXT_SM).font_weight(FontWeight::SEMIBOLD).text_color(gpui_kit::white()).child(Icon::new(Glyph::ChevronLeft).size(ICON_LG)).child(line(text.into()))
}

// A name over the picture, in white, cut short before it reaches the foot's controls.
// Keywords: pip label, pip name
pub fn pip_label(text: impl Into<SharedString>) -> Div {
    line(text.into()).text_size(TEXT_SM).font_weight(FontWeight::SEMIBOLD).text_color(gpui_kit::white())
}

// A control over the picture with no fill of its own: white ink that brightens under the pointer.
// Keywords: pip button, bare icon button, video control
pub fn pip_button(id: impl Into<ElementId>, glyph: Glyph, tip: impl Into<SharedString>) -> Button {
    let clear = gpui_kit::transparent_black();
    let scope = ButtonScope::Themed { rest: clear, hover: clear, ink: gpui_kit::white().opacity(0.85), ink_hover: gpui_kit::white() };
    Button::new(id).variant(ButtonVariant::Ghost).size(ButtonSize::IconLg).scope(scope).icon(glyph).tooltip_text(tip.into())
}
