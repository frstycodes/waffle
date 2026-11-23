use super::*;

// An icon's angle in degrees, eased over the pop duration whenever `degrees` changes.
pub(super) fn turn(id: String, degrees: f32, window: &mut Window, cx: &mut App) -> Radians {
    let degrees = gpui_kit::base::transition(id, degrees, gpui_kit::base::Transition::new(ui::DUR_POP).easing(motion::ease_out()), window, cx);
    Radians(degrees.to_radians())
}

// The mic wave's gap between bars and its tallest bar, and the sensitivity handle's half width and its pulse ring's reach.
const WAVE_GAP: f32 = 3.;
const WAVE_HEIGHT: f32 = 40.;
const HANDLE_HALF: f32 = 3.;
const PULSE_REACH: f32 = 5.;
const PULSE_WIDTH: f32 = 3.;
// How far above its place revealed content starts.
const REVEAL_DROP: f32 = 4.;

// The mic test draws your voice as the wave: 40 bars swelling from the middle, accent while live.
pub(super) fn mic_wave(level_db: f32, live: bool, cx: &App) -> Div {
    const BARS: usize = 40;
    let level = ((level_db + 100.) / 100.).clamp(0., 1.);
    // Quieter than a whisper reads as silence, so the wave stays flat in a quiet room.
    let voice = ((level - 0.35) / 0.55).clamp(0., 1.);
    let (rest, on) = (palette::line_strong(cx), palette::accent());
    ui::row().flex_1().h_10().gap(px(WAVE_GAP)).children((0..BARS).map(move |i| {
        let shape = (i as f32 / BARS as f32 * std::f32::consts::PI).sin();
        let jitter = 0.6 + 0.6 * ((i as f32 * 12.9898 + level_db * 78.233).sin() * 0.5 + 0.5);
        let l = if live { (voice * shape * jitter).clamp(0.1, 1.) } else { 0.1 };
        div().flex_1().min_w_0p5().h(px(WAVE_HEIGHT * l)).rounded_full().bg(if live && l > 0.15 { on } else { rest })
    }))
}

// The input sensitivity track: the live level lit over a bar split at the threshold, the handle on
// the threshold. With no threshold (automatic) the whole bar lights green up to the level.
pub(super) fn sensitivity_track(threshold: Option<f32>, level_db: f32, cx: &App) -> Div {
    let level = ((level_db + 100.) / 100.).clamp(0., 1.);
    let (rest, online) = (palette::line_strong(cx), palette::status_in(crate::discord::Status::Online, cx));
    let part = |from: f32, to: f32, fill: Hsla| {
        let (from, to) = (from.clamp(0., 1.), to.clamp(0., 1.));
        div().absolute().top_0().bottom_0().left(relative(from)).w(relative((to - from).max(0.))).bg(fill).when(from == 0., |d| d.rounded_l_full()).when(to >= 1., |d| d.rounded_r_full())
    };
    let bar = div().absolute().left_0().right_0().top_2().h_2();
    let bar = match threshold {
        None => bar.child(part(0., 1., rest)).child(part(0., level, online)),
        Some(th) => bar
            .child(part(0., th, rest))
            .child(part(th, 1., online.opacity(0.35)))
            .child(part(0., level.min(th), palette::faint(cx)))
            .when(level > th, |d| d.child(part(th, level, online))),
    };
    div()
        .relative()
        .w_full()
        .h_6()
        .child(bar)
        // lint-allow(raw-margin): the handle's centre sits on the threshold, so it shifts back half its width, which no gap or padding can do
        .children(threshold.map(|th| div().absolute().top_0().left(relative(th.clamp(0., 1.))).ml(px(-HANDLE_HALF)).w_1p5().h_6().rounded(px(HANDLE_HALF)).bg(palette::accent())))
}

// The recorder's listening ring: 3px of accent tint 4px out, breathing.
pub(super) fn pulse_ring(id: SharedString, cx: &App) -> AnyElement {
    // lint-allow(custom-mark): ui::badged marks a face's corner, this is a ring around a whole control, which it can't wrap
    let ring = div().absolute().top(px(-PULSE_REACH)).bottom(px(-PULSE_REACH)).left(px(-PULSE_REACH)).right(px(-PULSE_REACH)).squircle(ui::RADIUS_MD + 4.).border(px(PULSE_WIDTH)).border_color(palette::accent_soft(cx));
    if cx.reduce_motion() {
        return ring.into_any_element();
    }
    // lint-allow(endless-animation): shown only while a keybind is recording; the ring is dropped when recording ends
    ring.with_animation(id, Animation::new(Duration::from_millis(2400)).repeat(), |ring, t| ring.opacity(0.2 + 0.8 * (0.5 - 0.5 * (t * std::f32::consts::TAU).cos())))
        .into_any_element()
}

// Content arriving in place: a fade down from 4px above, on the house ease-out.
pub(super) fn reveal(id: impl Into<ElementId>, el: impl IntoElement) -> AnyElement {
    div()
        .relative()
        .child(el)
        .with_animation(id, Animation::new(ui::DUR_POP).with_easing(|t| motion::ease_out().sample(t)), |el, t| el.top(px(-REVEAL_DROP * (1. - t))).opacity(t))
        .into_any_element()
}

pub(super) fn ring(color: Hsla, width: f32) -> BoxShadow {
    BoxShadow { color, offset: Default::default(), blur_radius: Default::default(), spread_radius: px(width), inset: false }
}

pub(super) fn inset_ring(color: Hsla, width: f32) -> BoxShadow {
    BoxShadow { inset: true, ..ring(color, width) }
}

