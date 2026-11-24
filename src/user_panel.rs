// The user area (.me) at the foot of the channel list, as Discord's account panel (design/user-row.html): who you
// are and what you're doing, then mute, deafen and settings. In a call the voice panel sits on top of it in the
// same card.
use std::time::{Duration, Instant};

use gpui_kit::{
    AnyElement, App, Context, Div, Hsla, IntoElement, MouseButton, MouseDownEvent, SharedString, Stateful, Task, Window,
    AnimationExt as _,
    component::{ActiveTheme, Icon},
    div, prelude::*, px,
};

use crate::{
    Waffle,
    call::DeviceKind,
    discord,
    icons::Glyph,
    layout::{PANEL_HEIGHT, decorated_avatar, nameplate, playing_avatar},
    palette,
    settings::Page,
    sidebar::Nav,
    ui::{self, Squircle},
};

const USER_ROW: f32 = 40.;
// The user area's narrow controls: its chevrons' and buttons' height.
const ME_CONTROL: f32 = 30.;
// Discord's "You are currently muted!": up for 2.5s when you talk while muted, at most every 2 minutes.
const MUTED_CUE_FOR: Duration = Duration::from_millis(2500);
const MUTED_CUE_EVERY: Duration = Duration::from_secs(120);
// Discord's red glow under a muted or deafened control: red at 12%, 24% under the pointer.
const GLOW: f32 = 0.12;
const GLOW_HOVER: f32 = 0.24;

impl Waffle {
    pub(crate) fn user_panel(&mut self, me: discord::User, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // Hovering anywhere on the panel, its buttons too, rolls the second line to your username and plays your
        // avatar, decoration and nameplate, as Discord's does.
        let (hovered, on_hover) = ui::hover_state("me-hover", window, cx);
        let animate = hovered && crate::presence::may_animate();
        let rail = palette::surface(palette::Level::Rail, cx);
        let (card, ring) = (crate::material::card(cx), crate::material::behind_sidebar(rail));
        let voice = self.my_voice_state();
        let (server_muted, server_deafened, suppressed) = voice.map_or((false, false, false), |v| (v.server_muted, v.server_deafened, v.suppress));
        let mic_denied = server_muted || suppressed;
        let deafened = self.deafened || server_deafened;
        let muted = self.muted || deafened || mic_denied;
        let has_plate = me.nameplate.is_some();
        // Over nameplate art a muted or deafened control sits on a plate of black at 22% so its red reads.
        let plate = |on: bool| (has_plate && on).then(|| gpui_kit::black().opacity(0.22));
        let mic_tip = match (suppressed, server_muted, muted) {
            (true, _, _) => "Suppressed",
            (false, true, _) => "Server Muted",
            (false, false, true) => "Unmute",
            (false, false, false) => "Mute",
        };
        let deaf_tip = match (server_deafened, deafened) {
            (true, _) => "Server Deafened",
            (false, true) => "Undeafen",
            (false, false) => "Deafen",
        };
        // Push to talk in a call: a small ring at the mic's corner, red at rest and green while you talk.
        let ptt = (self.prefs.push_to_talk && self.call.is_some()).then(|| self.call.as_ref().is_some_and(|c| c.level.1 && !muted));
        let mic = me_button("mic", if muted { Nav::MicOff } else { Nav::Mic }, muted, mic_denied, mic_tip, cx)
            .when_some(ptt, |b, talking| b.child(ptt_ring(talking, cx)))
            .when(!mic_denied, |b| b.on_click(cx.listener(|this, _, _, cx| this.toggle_mute(cx))))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_device_menu(DeviceKind::Input, e.position, cx)))
            .when_some(plate(muted), |b, fill| b.bg(fill));
        let headphones = me_button("deafen", if deafened { Nav::HeadphonesOff } else { Nav::Headphones }, deafened, server_deafened, deaf_tip, cx)
            .when(!server_deafened, |b| b.on_click(cx.listener(|this, _, _, cx| this.toggle_deafen(cx))))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_device_menu(DeviceKind::Output, e.position, cx)))
            .when_some(plate(deafened), |b, fill| b.bg(fill));
        // A chevron beside each opens that device's popover, as Discord's does; right-clicking the button itself
        // opens the same one.
        let menu_open = |kind: DeviceKind| self.device_menu.as_ref().is_some_and(|m| m.opened_from_user_area(kind));
        let mic_options = me_chevron("mic-options", "me-mic", "Input Options", menu_open(DeviceKind::Input), muted, cx)
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_device_menu(DeviceKind::Input, e.position, cx)))
            .when_some(plate(muted), |b, fill| b.bg(fill));
        let output_options = me_chevron("output-options", "me-deafen", "Output Options", menu_open(DeviceKind::Output), deafened, cx)
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_device_menu(DeviceKind::Output, e.position, cx)))
            .when_some(plate(deafened), |b, fill| b.bg(fill));
        // In a call, settings opens on Voice & Video, as Discord's does.
        let page = if self.call.is_some() { Page::VoiceVideo } else { Page::Account };
        let settings = me_button("settings", Nav::Settings, false, false, "User Settings", cx).on_click(cx.listener(move |this, _, window, cx| this.open_settings(page, window, cx)));
        // The card's bottom corners, and its top ones too when nothing sits above; outside them, the sidebar.
        let stacked = self.panel_stack_height() > PANEL_HEIGHT;
        let top = if stacked { 0. } else { ui::RADIUS_MD };
        let corners = [top, top, ui::RADIUS_MD, ui::RADIUS_MD];
        let nameplate = me.nameplate.as_ref().map(|plate| nameplate(plate, palette::surface(palette::Level::Sidebar, cx), corners, cx.theme().is_dark(), animate));
        // Live on Go Live, your mark turns to the streaming one; talking in a call rings your face.
        let mark = match self.call.as_ref().is_some_and(|c| c.sharing.is_some()) {
            true => Some(discord::Mark::Streaming),
            false => self.my_mark().map(Into::into),
        };
        let speaking = !muted && self.call.is_some() && self.speaking(&me.id);
        let face = match animate {
            true => playing_avatar("me", &me.avatar, me.decoration.as_deref(), 32., mark, ring),
            false => decorated_avatar(&me.avatar, me.decoration.as_deref(), 32., mark, ring),
        };
        let face = div().flex_none().rounded_full().when(speaking, |d| d.shadow(ui::speaking_ring(card))).child(face);
        let username = if me.username.is_empty() { me.name.clone() } else { me.username.clone() };
        let second = ui::hover_roll("me-roll", self.my_status_line(&me, cx), ui::line(username), hovered, ui::LINE_XS, window, cx);
        let who = ui::list_row("me", me.name.clone(), cx)
            .height(ui::RowHeight::Exact(USER_ROW))
            .flex_1()
            .p_1()
            .squircle(ui::RADIUS_SM)
            .gap_2()
            .control(|c| c.highlighted(self.status_menu))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_status_menu(cx)))
            .lead(face)
            .title_element(ui::truncating("me-name", me.name.clone()).line_height(ui::LINE_DETAIL))
            .subtitle(div().w_full().text_color(palette::faint(cx)).child(second));
        let voice = self.voice_panel(window, cx);
        ui::col() // lint-allow(custom-badge, custom-mark): ui::surface adds a ring and shadow this docked card doesn't draw
            .absolute()
            .left_2()
            .right_2()
            .bottom_2()
            .squircle(ui::RADIUS_MD)
            .overflow_hidden()
            .bg(card)
            .text_size(ui::TEXT_SM)
            .children(self.update_row(cx))
            .children(voice)
            .child(
                div()
                    .id("me-panel")
                    .relative()
                    .h(px(PANEL_HEIGHT))
                    .flex_none()
                    .on_hover(on_hover)
                    .children(nameplate)
                    .child(
                        ui::row()
                            .relative()
                            .size_full()
                            // 6px on every side, so a hover fill never meets the card's edge.
                            .p_1p5()
                            .gap_1()
                            .child(who)
                            .child(ui::row().relative().gap_2().child(split_control("me-mic", mic, mic_options).children(self.muted_cue.showing().then(muted_cue))).child(split_control("me-deafen", headphones, output_options)).child(settings)),
                    ),
            )
            .into_any_element()
    }

    // Your own voice state in the call you're in: what a moderator did to it (server mute, server deafen, stage
    // audience).
    pub(crate) fn my_voice_state(&self) -> Option<&discord::VoiceState> {
        let call = self.call.as_ref()?;
        let me = self.me.as_ref()?;
        let guild = self.guilds.iter().find(|g| Some(&g.id) == call.guild_id.as_ref())?;
        guild.voice_states.iter().find(|v| v.user_id == me.id)
    }

    // Talking while muted (the mic's gate opened): Discord's green "You are currently muted!" over the mic for
    // 2.5s, at most every 2 minutes, never while a moderator holds the mic.
    pub(crate) fn heard_while_muted(&mut self, cx: &mut Context<Self>) {
        let denied = self.my_voice_state().is_some_and(|v| v.server_muted || v.suppress);
        if !self.muted || denied || !self.muted_cue.due(Instant::now()) {
            return;
        }
        self.muted_cue.shown = Some(Instant::now());
        self.muted_cue.hide = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(MUTED_CUE_FOR).await;
            this.update(cx, |this, cx| this.redraw(crate::part::Column::Nav, cx)).ok();
        }));
        self.redraw(crate::part::Column::Nav, cx);
    }

    // In a stage channel's audience (Discord's `suppress`).
    pub(crate) fn in_stage_audience(&self) -> bool {
        self.call.as_ref().is_some_and(|c| c.channel.raw_type == discord::STAGE_TYPE) && self.my_voice_state().is_some_and(|v| v.suppress)
    }

    // The panel's second line at rest, Discord's: what you're doing (a stream, your game, the call you're in) with
    // your custom status after it, else your custom status, else your status word. Invisible shows no activity.
    fn my_status_line(&self, me: &discord::User, cx: &App) -> AnyElement {
        let custom = self.custom_status.as_ref().map(|_| self.status_line()).filter(|line| !line.is_empty());
        let activity = self.shared_game().map(|game| discord::Activity { kind: discord::ActivityKind::Playing, text: game.name.clone(), rest: Vec::new() });
        let presence = discord::Presence { status: self.my_status, custom, activity, ..Default::default() };
        let voice = self.call.as_ref().map(|call| (call.sharing.is_some(), call.channel.clone()));
        match crate::panel::activity_status(me, &presence, voice) {
            Some(status) => status.line(cx).into_any_element(),
            None => ui::line(self.status_line()).into_any_element(),
        }
    }
}

// A control and its chevron as one split pill: round on the outside, square where they meet, 1px apart.
// Pointing at either half lifts both, as Discord's pair does.
fn split_control(group: &'static str, button: ui::Control, chevron: Stateful<Div>) -> Div {
    ui::row()
        .group(group)
        .gap_px()
        .child(button.rounded_tr_none().rounded_br_none())
        .child(chevron.rounded_tl_none().rounded_bl_none())
}

// The narrow chevron beside a user-area control that opens its device popover: quieter and half as wide, its
// caret turned up while the popover is open. `glow` (muted, deafened) puts it on the control's red.
fn me_chevron(id: &'static str, group: &'static str, label: &'static str, open: bool, glow: bool, cx: &App) -> Stateful<Div> {
    let (hover, strong, selected) = (palette::hover(cx), palette::strong(cx), palette::selected(cx));
    let (rest, lit) = glow_fills(glow, selected);
    let ink = match (glow, open) {
        (true, _) => palette::danger_ink(cx),
        (false, true) => strong,
        (false, false) => palette::faint(cx),
    };
    let hover_ink = if glow { ink } else { strong };
    ui::row() // lint-allow(custom-button): ui::icon_button is square, and this chevron is a 16px sliver joined to its control
        .id(id)
        .w_4()
        .h(px(ME_CONTROL))
        .flex_none()
        .squircle(ui::RADIUS_XS)
        .justify_center()
        .cursor_pointer()
        .text_color(ink)
        .when_some(rest, |b, fill| b.bg(fill))
        .when(!glow && !open, |b| b.group_hover(group, move |s| s.bg(hover)))
        .when(open, |b| b.bg(lit))
        .hover(move |s| s.bg(lit).text_color(hover_ink))
        .child(Icon::new(if open { Glyph::ChevronUp } else { Glyph::ChevronDown }).size(ui::ICON_XS))
        .tooltip(move |window, cx| ui::tip(label, window, cx))
}

// A user-panel control: Waffle's own mic, headphones and settings icons, popping in when they switch. `on`
// (muted, deafened) puts it on Discord's red glow; `denied` (a moderator's doing) adds the prohibited mark.
fn me_button(id: &'static str, icon: Nav, on: bool, denied: bool, label: &'static str, cx: &App) -> ui::Control {
    let face = icon.icon().size(ui::ICON_XL).with_animation(SharedString::from(format!("{id}-{on}")), gpui_kit::Animation::new(ui::DUR_POP).with_easing(crate::motion::back_out), |icon, t| icon.size(ui::ICON_XL * (0.7 + 0.3 * t)));
    let danger = palette::danger_ink(cx);
    let control = ui::icon_slot(id, label, cx).sized(ME_CONTROL).relative().text_color(if on { danger } else { palette::strong(cx) }).child(face);
    let control = match on {
        true => control.scoped(palette::danger().opacity(GLOW), palette::danger().opacity(GLOW_HOVER), danger, danger),
        false => control,
    };
    control.when(denied, |c| c.child(div().absolute().right_0p5().bottom_0p5().child(Icon::new(Glyph::Prohibit).size(ui::ICON_XS).text_color(danger))))
}

// A chevron's fills: at rest (only on the red glow) and lit (itself hovered, or its popover open).
fn glow_fills(glow: bool, selected: Hsla) -> (Option<Hsla>, Hsla) {
    match glow {
        true => (Some(palette::danger().opacity(GLOW)), palette::danger().opacity(GLOW_HOVER)),
        false => (None, selected),
    }
}

// When the muted cue last showed, and the timer that takes it down.
#[derive(Default)]
pub(crate) struct MutedCue {
    shown: Option<Instant>,
    hide: Option<Task<()>>,
}

impl MutedCue {
    // Whether it may show again: never shown, or last shown 2 minutes ago or more.
    fn due(&self, now: Instant) -> bool {
        self.shown.is_none_or(|at| now.duration_since(at) >= MUTED_CUE_EVERY)
    }

    fn showing(&self) -> bool {
        self.shown.is_some_and(|at| at.elapsed() < MUTED_CUE_FOR)
    }
}

// The green callout over the mic, floating above the panel (the card clips its own children).
fn muted_cue() -> AnyElement {
    // lint-allow(custom-badge): ui::tip is a tooltip view only gpui's hover opens; this one is forced open, in Discord's green
    let bubble = div()
        .px_2p5()
        .py_1p5()
        .squircle(ui::RADIUS_SM)
        .bg(palette::success())
        .text_color(gpui_kit::white())
        .text_size(ui::TEXT_SM)
        .font_weight(gpui_kit::FontWeight::MEDIUM)
        .child("You are currently muted!");
    let callout = div().id("muted-cue").occlude().pb_2().child(bubble);
    // lint-allow(raw-floating): ui::floating hangs its content below the parent's top-left; this callout sits above the mic
    gpui_kit::deferred(gpui_kit::anchored().anchor(gpui_kit::Anchor::BottomLeft).snap_to_window_with_margin(ui::FLOAT_EDGES).child(callout)).into_any_element()
}

// Push to talk's mark at the mic's corner: a 2px ring, red until your key opens the mic, then green.
fn ptt_ring(talking: bool, cx: &App) -> Div {
    let ring = if talking { palette::positive_ink(cx) } else { palette::danger_ink(cx) };
    // lint-allow(custom-mark): ui::status_mark's ring is presence grey; this is push to talk's red or green ring
    div().absolute().right_1p5().bottom_1p5().size_2().rounded_full().border_2().border_color(ring)
}

#[cfg(test)]
mod tests {
    use super::{MUTED_CUE_EVERY, MutedCue};
    use std::time::{Duration, Instant};

    #[test]
    fn muted_cue_waits_two_minutes_between_showings() {
        let now = Instant::now();
        assert!(MutedCue::default().due(now));
        let cue = MutedCue { shown: Some(now), hide: None };
        assert!(!cue.due(now + Duration::from_secs(30)));
        assert!(cue.due(now + MUTED_CUE_EVERY));
    }
}
