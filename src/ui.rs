// Waffle's shared controls, ported from the design system (design/css/primitives.css, overlays.css). Each
// returns an element the caller keeps composing (`.on_click`, layout, extra children), so call sites never
// restate a control's look. `design_system.rs` shows every one of them in both themes, section for
// section with design/primitives.html.
//
// Pressable controls (buttons, icon buttons, chips, swatches, tab options, menu rows) live in `control`,
// fields in `field`, the sliding strips in `tabs`, tracks and waits in `feedback`, grouped settings in
// `group`; this file holds the marks, badges, people, surfaces, motion helpers and the live switch, check
// and radio.
//
// Corners are the design's squircle (`Squircle::squircle`); people, status marks, the switch, radios,
// the wave's bars and anything `rounded_full` stay round.
//
// Clipping: gpui's `overflow_hidden` clips to a plain rectangle, never to rounded corners. Anything
// painted against a rounded edge (an image, a header strip, a hover fill) must round itself to
// match, or sit inset by the surface's padding. `surface` insets its content; `media` and `edge`
// round theirs.
use std::time::Duration;

use gpui_kit::{
    Animation, AnimationElement, AnimationExt, AnyElement, App, BoxShadow, Corners, Div, ElementId, FontWeight, Hsla, ImageSource, Img, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, SharedString, SizeRefinement,
    StatefulInteractiveElement, Stateful, Styled, Window,
    base::InteractiveElementExt as _,
    component::scroll::ScrollableElement as _,
    base::{Transition, transition},
    component::Icon,
    div, img, linear_color_stop, linear_gradient, point, prelude::FluentBuilder, px, relative,
};

use crate::{discord::Status, icons::Glyph, mascot, motion, palette};

mod control;
mod dialog;
mod feedback;
mod field;
mod group;
mod tabs;
mod tri;
mod tunnel;

pub use control::{Control, Pressable, button, chip, hover_fade, icon_button, menu_row, pressable, round_button, swatch, tip};
pub use dialog::{dialog, dialog_body, modal};
pub use feedback::{meter, meter_with, progress, progress_indeterminate, progress_live, ring, skeleton, skeleton_circle, slider_live, slider_track, squiggle};
pub use field::{code_cell, drop_zone, field, field_error, field_frame, field_hint, search_button, search_field, search_frame, select, select_options, stepper, text_area, text_field};
pub use group::{settings_caption, settings_card, settings_choice, settings_close, settings_column, settings_group, SettingsHighlight, settings_nav_item, settings_page, SETTINGS_NAV_WIDTH, SETTINGS_WIDTH, settings_row, settings_nav_edge, settings_sidebar, settings_switch, settings_text, settings_title, settings_window};
pub use tabs::{Tab, pill_tabs, segmented_live, tabs};
pub use tri::{Tri, tri_state};
pub use tunnel::{TUNNEL_GRACE, Tunnel};

// Radius tiers: tags and tooltips, rows and menu items, controls, cards and tiles, dialogs. Sized for the
// squircle, whose curve hugs the corner tighter than a circular arc of the same radius.
pub const RADIUS_XS: f32 = 8.;
pub const RADIUS_SM: f32 = 11.;
pub const RADIUS_MD: f32 = 14.;
pub const RADIUS_LG: f32 = 20.;
pub const RADIUS_XL: f32 = 28.;
// CSS `corner-shape: superellipse(1.4)`. People, status marks and pills stay round.
pub const SQUIRCLE: gpui_kit::CornerShape = gpui_kit::CornerShape::Superellipse(1.4);

/// Rounds an element with the design's soft squircle.
pub trait Squircle: Styled + Sized {
    fn squircle(self, radius: f32) -> Self {
        self.rounded(px(radius)).corner_shape(SQUIRCLE)
    }
}

impl<T: Styled> Squircle for T {}

// Padding a surface keeps between its rounded edge and its rows.
pub const SURFACE_INSET: f32 = 6.;

// Motion tokens (design/tokens.css): press, hover, pop, panel.
pub const DUR_PRESS: Duration = Duration::from_millis(120);
pub const DUR_HOVER: Duration = Duration::from_millis(150);
pub const DUR_POP: Duration = Duration::from_millis(200);
pub const DUR_PANEL: Duration = Duration::from_millis(260);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    // The accent: the action the surface is for.
    Primary,
    // The quiet fill: every other action.
    Secondary,
    // No fill until hovered: low-stakes actions in dense rows.
    Ghost,
    // The destructive step itself.
    Danger,
    // Translucent black over video and images.
    OnMedia,
    // Just the accent ink, underlined on hover: "View all members".
    Link,
}

// A main action and its options chevron, joined: inner corners tighten to 4px with a 1px seam.
pub fn split(main: Control, options: Control) -> Div {
    div()
        .flex()
        .gap(px(1.))
        .child(main.rounded_l(px(RADIUS_MD)).rounded_r(px(4.)))
        .child(options.px(px(8.)).rounded_r(px(RADIUS_MD)).rounded_l(px(4.)))
}

/// Whether a list has scrolled down from its top. A pull on a list with no room to scroll still dips the offset below 0.
pub fn scrolled(scroll: &gpui_kit::ScrollHandle) -> bool {
    scroll.max_offset().y > px(0.) && scroll.offset().y < px(0.)
}

/// A header with a hard edge over the list below it: a faint hairline along that edge shows once the list has scrolled
/// under it. `scrolled` is asked as the frame paints, after the list's layout: asked while rendering, it answers for
/// the frame before, and a list that settles without a re-render (a pull with no room to scroll) leaves it stale.
pub fn hard_edge(scrolled: impl Fn() -> bool + 'static, cx: &App) -> Div {
    let line = palette::line(cx);
    let rule = gpui_kit::canvas(|_, _, _| {}, move |bounds, _, window, _| {
        if scrolled() {
            window.paint_quad(gpui_kit::fill(bounds, line));
        }
    });
    div().relative().flex_none().w_full().child(rule.absolute().bottom_0().left_0().right_0().h(px(1.)))
}

/// A vertical scroll area moving `list` on `scroll`, with the kit's scrollbar over it. Not `overflow_y_scrollbar`: that
/// tracks a handle of its own, which nothing outside can read.
pub fn scroll_area(list: Stateful<Div>, scroll: &gpui_kit::ScrollHandle) -> Div {
    div()
        .relative()
        .flex_1()
        .min_h_0()
        .child(list.size_full().overflow_y_scroll().lock_scroll_axis().track_scroll(scroll))
        .vertical_scrollbar(scroll)
}


thread_local! {
    // Labels whose text overflowed their box when last painted, by key; they show the full text on hover.
    static CLIPPED: std::cell::RefCell<std::collections::HashSet<SharedString>> = Default::default();
}

/// A one-line label that truncates with an ellipsis and, when it does, shows its full text in a tooltip.
/// It measures its text against its own width as it paints, so the tooltip appears from the next frame on
/// and only for labels that are actually cut. `key` must be unique among labels on screen.
pub fn truncating(key: impl Into<SharedString>, text: impl Into<SharedString>) -> Stateful<Div> {
    let (key, text): (SharedString, SharedString) = (key.into(), text.into());
    let clipped = CLIPPED.with(|c| c.borrow().contains(&key));
    let (measure_key, measure_text) = (key.clone(), text.clone());
    let measure = gpui_kit::canvas(|_, _, _| {}, move |bounds, _, window, _| {
        let style = window.text_style();
        let size = style.font_size.to_pixels(window.rem_size());
        let width = window.text_system().layout_line(&measure_text, size, &[style.to_run(measure_text.len())], None).width;
        let over = width > bounds.size.width + px(0.5);
        CLIPPED.with(|c| {
            let mut c = c.borrow_mut();
            if over {
                c.insert(measure_key.clone());
            } else {
                c.remove(&measure_key);
            }
        });
    })
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    div()
        .id(ElementId::Name(key))
        .relative()
        .min_w_0()
        .truncate()
        .child(text.clone())
        .child(measure)
        .when(clipped, |d| gpui_kit::StatefulInteractiveElement::tooltip(d, move |window, cx| gpui_kit::component::tooltip::Tooltip::new(text.clone()).build(window, cx)))
}

pub fn live_badge() -> Div {
    badge(palette::live(), gpui_kit::white()).child("LIVE")
}

// A small solid label (quality, tags, NEW). GAP: the design tracks these 0.04–0.06em; gpui text has no
// letter spacing.
pub fn badge(fill: Hsla, ink: Hsla) -> Div {
    div().h(px(20.)).px(px(7.)).flex_none().squircle(RADIUS_XS).bg(fill).text_color(ink).text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).flex().items_center()
}

// What something is (App, Host, New): accent ink on the accent tint, uppercase 11px.
pub fn tag(text: impl Into<SharedString>, cx: &App) -> Div {
    let text: SharedString = text.into();
    badge(palette::accent_soft(cx), palette::accent_ink(cx)).h(px(18.)).px(px(6.)).gap(px(3.)).child(text.to_uppercase())
}

// A tag led by a small glyph (Host's crown).
pub fn tag_with(glyph: Glyph, text: impl Into<SharedString>, cx: &App) -> Div {
    let text: SharedString = text.into();
    badge(palette::accent_soft(cx), palette::accent_ink(cx)).h(px(18.)).px(px(6.)).gap(px(3.)).child(Icon::new(glyph).size(px(11.))).child(text.to_uppercase())
}

// A count asks for you, so it takes the alert red. A capsule that grows with the number.
pub fn count_badge(count: u32) -> Div {
    let text = if count > 99 { "99+".to_owned() } else { count.to_string() };
    div()
        .min_w(px(18.))
        .h(px(18.))
        .px(px(5.))
        .flex_none()
        .rounded_full()
        .bg(palette::alert())
        .text_color(gpui_kit::white())
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .line_height(px(18.))
        .flex()
        .items_center()
        .justify_center()
        .child(text)
}

// Something unread, without a count.
pub fn unread_dot() -> Div {
    div().size(px(8.)).flex_none().rounded_full().bg(palette::alert())
}

// A role: its colour as a dot, never as the text's colour.
pub fn role_chip(name: impl Into<SharedString>, color: Hsla, cx: &App) -> Div {
    div()
        .h(px(24.))
        .px(px(8.))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.))
        .squircle(RADIUS_XS)
        .bg(palette::selected(cx))
        .text_color(palette::text(cx))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .whitespace_nowrap()
        .child(div().size(px(10.)).rounded_full().bg(color))
        .child(name.into())
}

// A keycap: the raised fill, a hairline ring and a deeper bottom edge, so it reads as a key you could press.
pub fn kbd(key: impl Into<SharedString>, cx: &App) -> Div {
    let line = palette::line_strong(cx);
    key_cap(key).bg(palette::surface(palette::Level::Raised, cx)).shadow(vec![shadow(line, 0., -1., 0., 0., true), shadow(line, 0., 0., 0., 1., false)]).text_color(palette::strong(cx))
}

// A key cap on a coloured fill (a danger button): a wash of `ink` with an inset edge, lettered in `ink`,
// so it takes the fill's colour instead of sitting on it as a raised cap.
pub fn kbd_translucent(key: impl Into<SharedString>, ink: Hsla) -> Div {
    key_cap(key).bg(ink.opacity(0.16)).shadow(vec![shadow(ink.opacity(0.24), 0., 0., 0., 1., true)]).text_color(ink)
}

fn key_cap(key: impl Into<SharedString>) -> Div {
    div()
        .min_w(px(22.))
        .h(px(22.))
        .px(px(6.))
        .flex_none()
        .squircle(RADIUS_XS)
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .whitespace_nowrap()
        .flex()
        .items_center()
        .justify_center()
        .child(key.into())
}

// A shortcut: its keys side by side.
pub fn keys<I: Into<SharedString>>(keys: impl IntoIterator<Item = I>, cx: &App) -> Div {
    div().flex().items_center().gap(px(4.)).children(keys.into_iter().map(|key| kbd(key, cx)))
}

// The segmented thumb's lift: a soft drop plus, on dark, a lit top edge; on light, a hairline.
pub(crate) fn seg_lift(cx: &App) -> Vec<BoxShadow> {
    let shade = palette::shade(cx);
    if dark(cx) {
        return vec![shadow(shade.opacity(0.4), 0., 1., 3., 0., false), shadow(gpui_kit::white().opacity(0.14), 0., 0.5, 0., 0., true)];
    }
    vec![shadow(shade.opacity(0.2), 0., 1., 3., 0., false), shadow(shade.opacity(0.1), 0., 0., 0., 0.5, false)]
}

// What floats (menus, popovers, toasts): a long soft drop and a hairline ring in place of a border.
pub fn shadow_pop(cx: &App) -> Vec<BoxShadow> {
    shadow_pop_on(palette::line_strong(cx), cx)
}

// The pop shadow with its hairline in `ring`, for a scope with its own line colour (the call stage).
pub fn shadow_pop_on(ring: Hsla, cx: &App) -> Vec<BoxShadow> {
    let drop = palette::shade(cx).opacity(if dark(cx) { 0.45 } else { 0.16 });
    vec![shadow(drop, 0., 12., 32., 0., false), shadow(ring, 0., 0., 0., 1., false)]
}

pub fn shadow(color: Hsla, x: f32, y: f32, blur: f32, spread: f32, inset: bool) -> BoxShadow {
    BoxShadow { color, offset: point(px(x), px(y)), blur_radius: px(blur), spread_radius: px(spread), inset }
}

fn knob_shadow() -> Vec<BoxShadow> {
    let black = gpui_kit::black();
    vec![shadow(black.opacity(0.22), 0., 2., 6., 0., false), shadow(black.opacity(0.06), 0., 0., 0., 0.5, false)]
}

// The switch: a wide pill with a capsule knob, honey when on (iOS-style). The knob springs across (300ms
// on the touch spring) while the track's fill fades to the accent, and while pressed it swells into glass.
pub fn switch_live(id: impl Into<ElementId>, on: bool, window: &mut Window, cx: &mut App) -> Stateful<Div> {
    let id: ElementId = id.into();
    let held = window.use_keyed_state(control::part(&id, "held"), cx, |_, _| false);
    let pressed = *held.read(cx);
    let (down, up, up_out) = (hold(&held, true, window), hold(&held, false, window), hold(&held, false, window));
    switch_track(&id, on, pressed, window, cx)
        .id(id.clone())
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, down)
        .on_mouse_up(MouseButton::Left, up)
        .on_mouse_up_out(MouseButton::Left, up_out)
}

// A mouse listener that records whether the switch is held down.
fn hold<E: 'static>(held: &gpui_kit::Entity<bool>, value: bool, window: &Window) -> impl Fn(&E, &mut Window, &mut App) + 'static {
    window.listener_for(held, move |h: &mut bool, _: &E, _, cx| {
        *h = value;
        cx.notify();
    })
}

// The switch's track and knob, `pressed` coming from whoever holds it (the switch, or its toggle row).
fn switch_track(id: &ElementId, on: bool, pressed: bool, window: &mut Window, cx: &mut App) -> Div {
    let spring = || Transition::new(Duration::from_millis(300)).easing(motion::ease_spring());
    let travel = transition(control::part(id, "travel"), if on { 1. } else { 0. }, spring(), window, cx);
    let swell = transition(control::part(id, "swell"), if pressed { 1. } else { 0. }, spring(), window, cx);
    let fill = transition(control::part(id, "fill"), if on { 1. } else { 0. }, Transition::new(DUR_POP).easing(motion::ease_out()), window, cx);
    let glass = hover_fade(control::part(id, "glass"), pressed, window, cx);
    let knob = stretch(div().absolute().top(px(2.)).left(px(2.)).w(px(22.)).h(px(20.)).translate_x(px(14. * travel)), 1. + 0.18 * swell, 1. + 0.25 * swell)
        .rounded_full()
        .bg(mix(gpui_kit::white(), gpui_kit::white().opacity(0.42), glass))
        .shadow(knob_glass(glass))
        // The backdrop blur keeps to the knob's unscaled box: gpui's blur doesn't follow transforms.
        .when(glass > 0., |k| k.backdrop_blur(px(2. * glass)).backdrop_saturate(1. + 0.8 * glass));
    div().w(px(40.)).h(px(24.)).flex_none().relative().rounded_full().bg(mix(palette::track(cx), palette::accent(), fill.clamp(0., 1.))).child(knob)
}

// The knob's shadows, from resting (`t` 0) to pressed glass (`t` 1): a lit inner rim and a deeper drop.
fn knob_glass(t: f32) -> Vec<BoxShadow> {
    if t <= 0. {
        return knob_shadow();
    }
    let (white, black) = (gpui_kit::white(), gpui_kit::black());
    vec![
        shadow(white.opacity(0.7 * t), 0., 0., 0., t, true),
        shadow(white.opacity(0.9 * t), 0., t, t, 0., true),
        shadow(black.opacity(0.22 + 0.03 * t), 0., 2. + 2. * t, 6. + 6. * t, 0., false),
    ]
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Check {
    Off,
    On,
    // Some of a group chosen: a minus bar in place of the tick.
    Mixed,
}

pub fn check(state: Check, cx: &App) -> Div {
    let filled = state != Check::Off;
    let accent = palette::accent();
    check_box(state, if filled { accent } else { palette::line_strong(cx) }, if filled { accent } else { gpui_kit::transparent_black() })
        .when(state == Check::On, |d| d.child(Icon::new(Glyph::Checkmark).size(px(13.))))
}

// A checkbox that fades its fill and pops its tick in on the spring (200ms), as the design's does.
pub fn check_live(id: impl Into<ElementId>, state: Check, window: &mut Window, cx: &mut App) -> Div {
    let id: ElementId = id.into();
    let filled = hover_fade(control::part(&id, "fill"), state != Check::Off, window, cx);
    let tick = transition(control::part(&id, "tick"), if state == Check::On { 1. } else { 0. }, Transition::new(DUR_POP).easing(motion::ease_spring()), window, cx);
    let accent = palette::accent();
    check_box(state, mix(palette::line_strong(cx), accent, filled), accent.opacity(filled))
        .when(state != Check::Mixed && tick > 0.01, |d| d.child(Icon::new(Glyph::Checkmark).size(px(13.)).scale(0.5 + 0.5 * tick).text_color(palette::on_accent().opacity(tick.min(1.)))))
}

fn check_box(state: Check, edge: Hsla, fill: Hsla) -> Div {
    div()
        .size(px(20.))
        .flex_none()
        .squircle(6.)
        .border(px(1.5))
        .border_color(edge)
        .bg(fill)
        .text_color(palette::on_accent())
        .flex()
        .items_center()
        .justify_center()
        .when(state == Check::Mixed, |d| d.child(div().w(px(10.)).h(px(2.)).squircle(1.).bg(palette::on_accent())))
}

// Off: a thin ring. On: the ring thickens to an accent band around the surface's colour.
pub fn radio(on: bool, cx: &App) -> Div {
    radio_with(on, 20., palette::line_strong(cx))
}

// A radio at `size` (menus use 18) with its resting ring in `line`, for scopes with their own line colour.
pub fn radio_with(on: bool, size: f32, line: Hsla) -> Div {
    radio_ring(if on { 1. } else { 0. }, size, line)
}

// A radio whose band thickens on the spring when chosen.
pub fn radio_live(id: impl Into<ElementId>, on: bool, window: &mut Window, cx: &mut App) -> Div {
    let id: ElementId = id.into();
    let band = transition(control::part(&id, "band"), if on { 1. } else { 0. }, Transition::new(DUR_POP).easing(motion::ease_spring()), window, cx);
    radio_ring(band, 20., palette::line_strong(cx))
}

// The chosen band leaves an 8px hole at any size: 6px at 20, 5px at 18, as the design's two radios.
fn radio_ring(band: f32, size: f32, line: Hsla) -> Div {
    let on_width = (size - 8.) / 2.;
    let color = mix(line, palette::accent(), band.clamp(0., 1.));
    div().size(px(size)).flex_none().rounded_full().border(px((1.5 + (on_width - 1.5) * band).max(1.))).border_color(color)
}

// A setting that applies at once: its title and a line of detail, the switch at the end. The whole row
// is the hit area and fills on hover, and holding the row swells the switch's knob; the caller toggles
// on click.
pub fn toggle_row(id: impl Into<ElementId>, title: impl Into<SharedString>, detail: Option<SharedString>, on: bool, window: &mut Window, cx: &mut App) -> Control {
    let id: ElementId = id.into();
    let pressed = control::pressed(&id, window, cx);
    let switch = switch_track(&control::part(&id, "switch"), on, pressed, window, cx);
    control::choice(id, cx).child(choice_text(title, detail, cx)).child(switch)
}

// One of a set to pick from: the check or radio first, then its title and detail.
pub fn pick_row(id: impl Into<ElementId>, mark: impl IntoElement, title: impl Into<SharedString>, detail: Option<SharedString>, cx: &App) -> Control {
    control::choice(id, cx).child(mark).child(choice_text(title, detail, cx))
}

fn choice_text(title: impl Into<SharedString>, detail: Option<SharedString>, cx: &App) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(div().font_weight(FontWeight::NORMAL).text_color(palette::strong(cx)).child(title.into()))
        .when_some(detail, |d, detail| d.child(div().text_size(px(12.)).text_color(palette::muted(cx)).child(detail)))
}

// A photo avatar.
pub fn avatar(url: impl Into<SharedString>, size: f32) -> Img {
    img(url.into()).size(px(size)).flex_none().rounded_full()
}

// A DM's face at `size` (design: navigation.html's .gav). A pair puts the first member top left and the
// second bottom right at 0.69 of the size, the front one ringed in `ring`, the surface behind it.
pub fn dm_face(face: &crate::discord::DmFace, size: f32, ring: Hsla) -> AnyElement {
    use crate::discord::DmFace;
    match face {
        DmFace::Icon(url) | DmFace::One(url) => avatar(url.clone(), size).into_any_element(),
        DmFace::Letters(name) => monogram(name, size).into_any_element(),
        DmFace::Pair(back, front) => {
            let small = (size * 0.69).round();
            let gap = if size >= 48. { 3. } else { 2. };
            div()
                .relative()
                .flex_none()
                .size(px(size))
                .child(div().absolute().top_0().left_0().child(avatar(back.clone(), small)))
                .child(div().absolute().right_0().bottom_0().size(px(small)).child(self::gap(ring, gap)).child(avatar(front.clone(), small)))
                .into_any_element()
        }
    }
}

// An avatar without a photo: the name's initial on a gradient picked from the name. Below 24px it
// drops the initial.
pub fn monogram(name: &str, size: f32) -> Div {
    let hue = name_hue(name);
    let initial: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    div()
        .size(px(size))
        .flex_none()
        .rounded_full()
        .bg(linear_gradient(135., linear_color_stop(palette::oklch(0.53, 0.12, hue), 0.), linear_color_stop(palette::oklch(0.41, 0.13, hue + 35.), 1.)))
        .flex()
        .items_center()
        .justify_center()
        .text_color(gpui_kit::white())
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(size * 0.38))
        .when(size >= 24., |d| d.child(initial))
}

// The design's avatar hue: a stable hash of the name onto the colour wheel.
pub fn name_hue(name: &str) -> f32 {
    name.chars().fold(7_u32, |h, c| (h * 31 + c as u32) % 360) as f32
}

// An avatar with its presence mark at the corner. `ring` is the surface behind it.
pub fn with_status(avatar: impl IntoElement, status: Status, size: f32, ring: Hsla, cx: &App) -> Div {
    let mark = (size * 0.3).clamp(9., 16.);
    let inset = size * 0.04 - 2.;
    div().relative().flex_none().size(px(size)).child(avatar).child(div().absolute().right(px(inset)).bottom(px(inset)).child(status_mark(status, mark, ring, cx)))
}

// Speaking: a gap in the surface's colour, then the accent ring.
pub fn speaking_ring(ring: Hsla) -> Vec<BoxShadow> {
    vec![shadow(palette::accent(), 0., 0., 0., 4., false), shadow(ring, 0., 0., 0., 2., false)]
}

// Overlapping avatars, each ringed in `ring` (the surface colour) so they read apart.
pub fn avatar_stack(urls: impl IntoIterator<Item = String>, size: f32, ring: Hsla) -> Div {
    stacked(urls.into_iter().map(move |url| avatar(url, size).into_any_element()), size, ring)
}

// Avatars of one `size` (photos, monograms, a "+3"), overlapped 6px and ringed in the surface's colour.
// Placed absolutely in a box sized to the overlap, not as a flex row with -6px margins: taffy 0.13's flex
// intrinsic sizing reads a negative margin as the item asking to shrink below its basis, so such a row
// measures too narrow (four 24px avatars at -6px measure 0 wide, at -1px one avatar wide) and whatever
// follows slides underneath.
pub fn stacked(avatars: impl IntoIterator<Item = AnyElement>, size: f32, ring: Hsla) -> Div {
    let avatars: Vec<AnyElement> = avatars.into_iter().collect();
    let step = size - 6.;
    let width = size + step * avatars.len().saturating_sub(1) as f32;
    div().relative().flex_none().w(px(width)).h(px(size)).children(avatars.into_iter().enumerate().map(move |(i, avatar)| {
        div().absolute().top_0().left(px(step * i as f32)).size(px(size)).child(gap(ring, 2.)).child(avatar)
    }))
}

// At most `cap` faces; past that the last slot becomes a "+n" chip in `more_fill`, so the stack stays a
// glance, not a roster.
pub fn capped_stack(urls: &[String], cap: usize, size: f32, ring: Hsla, more_fill: Hsla, more_ink: Hsla) -> Div {
    let shown = if urls.len() > cap { cap - 1 } else { urls.len() };
    let more = urls.len() - shown;
    let faces = urls[..shown].iter().map(|url| avatar(url.clone(), size).into_any_element());
    let more_chip = (more > 0).then(|| {
        div()
            .size(px(size))
            .rounded_full()
            .bg(more_fill)
            .text_color(more_ink)
            .text_size(px((size * 0.43).round().max(11.)))
            .font_weight(FontWeight::SEMIBOLD)
            .flex()
            .items_center()
            .justify_center()
            .child(format!("+{more}"))
            .into_any_element()
    });
    stacked(faces.chain(more_chip), size, ring)
}

// Paints `el` in the surface colour it stands in for; over the native sidebar, where that colour is
// clear, it cuts its shape out of whatever was painted under it instead, so the material shows through.
pub fn cut_out(el: Div, surface: Hsla) -> Div {
    match surface.a > 0. {
        true => el.bg(surface),
        false => el.backdrop_erase(),
    }
}

// The gap `spread` wide around a round mark or an overlapping avatar, separating it from what's under
// it. Goes first among the parent's children, so it paints before the mark.
pub fn gap(surface: Hsla, spread: f32) -> Div {
    cut_out(div().absolute().top(px(-spread)).left(px(-spread)).right(px(-spread)).bottom(px(-spread)).rounded_full(), surface)
}

// The rail's tile proportion (a 44px tile at RADIUS_LG), kept at every size a server tile is drawn.
const SERVER_TILE_ROUNDING: f32 = RADIUS_LG / 44.;

// A server's icon as a squircle tile at `size`, or its initials on the selected fill when it has none.
pub fn server_tile(icon: Option<&str>, name: &str, size: f32, cx: &App) -> AnyElement {
    let radius = size * SERVER_TILE_ROUNDING;
    if let Some(url) = icon {
        return img(url.to_owned()).size(px(size)).flex_none().squircle(radius).into_any_element();
    }
    div()
        .size(px(size))
        .flex_none()
        .squircle(radius)
        .bg(palette::selected(cx))
        .text_color(palette::strong(cx))
        .text_size(px((size * 0.38).round()))
        .font_weight(FontWeight::SEMIBOLD)
        .flex()
        .items_center()
        .justify_center()
        .child(crate::sidebar::initials(name))
        .into_any_element()
}

// A server tile with `badge` (a channel's kind) on a round mark at its bottom-right, ringed 2px in
// `surface`, the colour the tile sits on, so the mark reads cut out of the tile.
pub fn server_tile_badged(icon: Option<&str>, name: &str, size: f32, badge: Glyph, surface: Hsla, cx: &App) -> Div {
    let mark = (size * 0.45).round();
    let ring = 2.;
    let corner = -(mark / 2. - size * 0.12);
    let badge = cut_out(div().absolute().right(px(corner - ring)).bottom(px(corner - ring)).size(px(mark + 2. * ring)).rounded_full(), surface)
        .flex()
        .items_center()
        .justify_center()
        .text_color(palette::muted(cx))
        .child(Icon::new(badge).size(px((mark * 0.66).round())));
    div().relative().flex_none().size(px(size)).child(server_tile(icon, name, size, cx)).child(badge)
}

// A presence mark for an avatar's corner. Never colour alone: online is a dot, idle a moon, do not
// disturb a bar, offline a ring. `ring` is the surface it sits on: a 2px gap around the mark.
pub fn status_mark(status: Status, size: f32, ring: Hsla, cx: &App) -> Div {
    let color = palette::status_in(status, cx);
    let mark = div().absolute().inset_0().rounded_full();
    let mark = match status {
        Status::Online => mark.bg(color),
        // The moon's bite runs past the disc's edge into the ring, which is the same colour.
        Status::Idle => mark.bg(color).child(cut_out(div().absolute().top(px(size * (0.28 - 0.346))).left(px(size * (0.28 - 0.346))).size(px(size * 0.692)).rounded_full(), ring)),
        Status::Dnd => mark.bg(color).flex().items_center().justify_center().child(cut_out(div().w(px(size * 0.55)).h(px(size * 0.22)), ring)),
        Status::Offline => mark.child(cut_out(div().absolute().inset_0().rounded_full(), ring)).child(div().absolute().inset_0().rounded_full().border(px(2.5)).border_color(color)),
    };
    div().size(px(size)).flex_none().relative().child(gap(ring, 2.)).child(mark)
}

// A floating surface (menu, popover, picker): rounded, ringed and shadowed, with its content inset so
// nothing reaches the rounded edge. A menu card is `surface(cx).w(px(248.))`.
pub fn surface(cx: &App) -> Div {
    div().squircle(RADIUS_LG).bg(palette::surface(palette::Level::Overlay, cx)).shadow(shadow_pop(cx)).p(px(SURFACE_INSET)).flex().flex_col().gap(px(1.))
}

// A floating surface's entrance (menus, popovers, pickers, dialogs): it fades in and grows from 0.95 over
// the pop timing, after `delay` (a submenu's intent delay). It grows from the top-left corner unless the
// caller set a `transform_origin` toward its trigger.
pub fn rise<E:IntoElement + Styled + 'static>(mut el: E, id: impl Into<ElementId>, delay: Duration) -> AnimationElement<E> {
    if el.style().transform_origin.x.is_none() {
        el = el.transform_origin(point(relative(0.), relative(0.)));
    }
    let lead = delay.as_secs_f32() / (delay + DUR_POP).as_secs_f32();
    el.with_animation(id, Animation::new(delay + DUR_POP), move |el, t| {
        let t = motion::ease_out().sample(((t - lead) / (1. - lead)).clamp(0., 1.));
        el.opacity(t).scale(0.95 + 0.05 * t)
    })
}

// A transform origin at one of an element's corners: where a floating surface rises from, toward its trigger.
pub fn corner(right: bool, bottom: bool) -> gpui_kit::Point<gpui_kit::DefiniteLength> {
    point(relative(if right { 1. } else { 0. }), relative(if bottom { 1. } else { 0. }))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Notice {
    Accent,
    Info,
    Warning,
    Danger,
    Success,
}

// An inline notice inside the view it's about: a tinted surface, a leading icon, a title and a line.
// An action button, if any, goes last and centres itself.
pub fn callout(kind: Notice, glyph: Glyph, title: impl Into<SharedString>, body: impl Into<SharedString>, cx: &App) -> Div {
    let (tone, tint) = notice_colors(kind, cx);
    callout_frame(tone, tint, glyph).child(callout_text(title, body, cx))
}

// A callout whose actions sit under its text (`actions` is laid out by the caller), for notices that
// offer more than one way on.
pub fn callout_with(kind: Notice, glyph: Glyph, title: impl Into<SharedString>, body: impl Into<SharedString>, actions: impl IntoElement, cx: &App) -> Div {
    let (tone, tint) = notice_colors(kind, cx);
    callout_frame(tone, tint, glyph).child(callout_text(title, body, cx).child(actions))
}

fn notice_colors(kind: Notice, cx: &App) -> (Hsla, Hsla) {
    match kind {
        Notice::Accent => (palette::accent_ink(cx), palette::accent_soft(cx)),
        Notice::Info => (palette::info_ink(cx), palette::info_tint()),
        Notice::Warning => (palette::warning_ink(cx), palette::warning_tint()),
        Notice::Danger => (palette::danger_ink(cx), palette::danger_tint()),
        Notice::Success => (palette::positive_ink(cx), palette::success_tint()),
    }
}

fn callout_frame(tone: Hsla, tint: Hsla, glyph: Glyph) -> Div {
    div()
        .w_full()
        .py(px(12.))
        .px(px(14.))
        .squircle(RADIUS_LG)
        .bg(tint)
        .flex()
        .gap(px(12.))
        .items_start()
        .child(div().mt(px(1.)).text_color(tone).flex_none().child(Icon::new(glyph).size(px(20.))))
}

fn callout_text(title: impl Into<SharedString>, body: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .text_size(px(14.))
        .child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(title.into()))
        .child(div().text_color(palette::text(cx)).child(body.into()))
}

// A toast: one line (and an optional detail) on the floating surface, rising in over 240ms from 8px lower
// and 0.97. GAP: the design also un-blurs it from 4px (`filter: blur`); gpui has no element blur.
pub fn toast(id: impl Into<ElementId>, glyph: Glyph, ink: Hsla, title: impl Into<SharedString>, detail: Option<SharedString>, cx: &App) -> AnyElement {
    let (strong, muted) = (palette::strong(cx), palette::muted(cx));
    div()
        .relative()
        .w_full()
        .max_w(px(380.))
        .flex()
        .items_center()
        .gap(px(12.))
        .py(px(10.))
        .pr(px(10.))
        .pl(px(14.))
        .squircle(RADIUS_LG)
        .bg(palette::surface(palette::Level::Overlay, cx))
        .shadow(shadow_pop(cx))
        .child(div().flex_none().text_color(ink).child(Icon::new(glyph).size(px(20.))))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .font_weight(FontWeight::NORMAL)
                .text_color(strong)
                .child(title.into())
                .when_some(detail, |d, detail| d.child(div().text_size(px(12.)).font_weight(FontWeight::NORMAL).text_color(muted).child(detail))),
        )
        .with_animation(id, Animation::new(Duration::from_millis(240)).with_easing(|t| motion::ease_out().sample(t)), |toast, t| toast.opacity(t).translate_y(px(8. * (1. - t))).scale(0.97 + 0.03 * t))
        .into_any_element()
}

// An empty or waiting state: the mascot, what's missing, and room for one action.
pub fn empty_state(id: &'static str, mood: mascot::Mood, title: impl Into<SharedString>, body: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(8.))
        .py(px(32.))
        .px(px(16.))
        .text_center()
        .child(div().mb(px(8.)).child(mascot::mascot_at(id, mood, 0.6, cx)))
        .child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(title.into()))
        .child(div().max_w(px(320.)).text_color(palette::muted(cx)).child(body.into()))
}

// A divider, with an optional label naming what follows.
pub fn divider(label: Option<&'static str>, cx: &App) -> Div {
    let line = || div().flex_1().h(px(1.)).bg(palette::line(cx));
    div()
        .w_full()
        .flex()
        .items_center()
        .gap(px(12.))
        .child(line())
        .when_some(label, |d, label| d.child(div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(palette::faint(cx)).child(label)).child(line()))
}

// The hairline along a stacked section's foot, held in from the card's edges by `inset`. The section
// must be `relative`.
pub fn inset_rule(inset: f32, cx: &App) -> Div {
    div().absolute().bottom_0().left(px(inset)).right(px(inset)).h(px(1.)).bg(palette::line(cx))
}

// A squircle holding an icon, for row leads and settings.
pub fn icon_tile(glyph: Glyph, fill: Hsla, ink: Hsla) -> Div {
    div().size(px(36.)).flex_none().squircle(RADIUS_MD).bg(fill).text_color(ink).flex().items_center().justify_center().child(Icon::new(glyph).size(px(20.)))
}

// The mini wave: three bars, the mark for speaking, typing, loading and live.
pub fn bars(id: impl Into<ElementId>, color: Hsla, height: f32) -> AnyElement {
    let frame = move |t: f32| {
        div().h(px(height)).flex().items_center().gap(px(2.)).children([0.55_f32, 1., 0.7].iter().enumerate().map(move |(i, base)| {
            let phase = (t * std::f32::consts::TAU + i as f32 * 2.1).sin().abs();
            div().w(px(3.)).h(px(height * base * (0.35 + 0.65 * phase))).rounded_full().bg(color)
        }))
    };
    frame(0.).with_animation(id, Animation::new(Duration::from_millis(900)).repeat(), move |_, t| frame(t)).into_any_element()
}

// The mini wave at rest (`.bars.still`): connected, in a call.
pub fn bars_still(color: Hsla, height: f32) -> Div {
    still_wave(color, height, false)
}

// The resting wave with all but its first bar dimmed (`.bars.weak`): connected, on a poor link.
pub fn bars_weak(color: Hsla, height: f32) -> Div {
    still_wave(color, height, true)
}

fn still_wave(color: Hsla, height: f32, weak: bool) -> Div {
    div().h(px(height)).flex_none().flex().items_center().gap(px(2.)).children([0.55_f32, 1., 0.7].into_iter().enumerate().map(move |(i, base)| {
        let ink = if weak && i > 0 { color.opacity(0.3) } else { color };
        div().w(px(3.)).h(px(height * base)).rounded_full().bg(ink)
    }))
}

// An image placed against a rounded container's edge, rounded to `corners` itself because gpui
// can't clip children to rounded corners.
pub fn media(source: impl Into<ImageSource>, corners: Corners<Pixels>) -> Img {
    edge(img(source), corners)
}

// Corner sets for `media` and edge strips: all four, or only the top or bottom pair.
pub fn all(radius: f32) -> Corners<Pixels> {
    Corners::all(px(radius))
}

pub fn top(radius: f32) -> Corners<Pixels> {
    Corners { top_left: px(radius), top_right: px(radius), bottom_left: px(0.), bottom_right: px(0.) }
}

pub fn left(radius: f32) -> Corners<Pixels> {
    Corners { top_left: px(radius), top_right: px(0.), bottom_left: px(radius), bottom_right: px(0.) }
}

pub fn bottom(radius: f32) -> Corners<Pixels> {
    Corners { top_left: px(0.), top_right: px(0.), bottom_left: px(radius), bottom_right: px(radius) }
}

// A strip (header, footer, scroll area) that runs edge to edge of a rounded container, rounded to
// match on the corners it touches, in the container's squircle.
pub fn edge<E: Styled>(el: E, corners: Corners<Pixels>) -> E {
    el.rounded_tl(corners.top_left).rounded_tr(corners.top_right).rounded_bl(corners.bottom_left).rounded_br(corners.bottom_right).corner_shape(SQUIRCLE)
}

fn dark(cx: &App) -> bool {
    use gpui_kit::component::ActiveTheme;
    cx.theme().is_dark()
}

// CSS `color-mix` in sRGB: `t` of `to` over `from`, for fills that follow a transition.
pub fn mix(from: Hsla, to: Hsla, t: f32) -> Hsla {
    let (a, b) = (gpui_kit::Rgba::from(from), gpui_kit::Rgba::from(to));
    let lerp = |x: f32, y: f32| x + (y - x) * t;
    gpui_kit::Rgba { r: lerp(a.r, b.r), g: lerp(a.g, b.g), b: lerp(a.b, b.b), a: lerp(a.a, b.a) }.into()
}

// Scales an element by `x` and `y` about its transform origin: gpui's `scale` builder is uniform only.
fn stretch<E: Styled>(mut el: E, x: f32, y: f32) -> E {
    el.style().scale = SizeRefinement { width: Some(x), height: Some(y) };
    el
}
