// How the call stage fits the space it has: the control bar drops Invite, then its second section, as it narrows
// (Discord's widths, `Tray`), and the voice chat floats over the stage once docking it beside the stage would crowd it.
use super::{grid::{CARETS_UNTIL, Tray}, *};

// The stage keeps at least this width beside a docked chat; narrower, the chat floats over it instead.
const CHAT_DOCKS_FROM: f32 = 480.;
// The chat is never wider than this, so on a big screen it always docks beside the stage rather than float.
pub(super) const CHAT_MAX_WIDTH: f32 = 640.;
// The floating chat's inset from the stage's edges, and how far it slides in from the right as it opens.
const FLOAT_INSET: f32 = 8.;
const FLOAT_SLIDE: f32 = 16.;
// The red button: wider and taller than the bar's other controls, so leaving is the easiest press to find.
const LEAVE_WIDTH: f32 = 64.;

impl Waffle {
    // Whether the voice chat floats over the stage rather than taking its width.
    pub(crate) fn call_chat_floats(&self, window: &Window) -> bool {
        self.call_stage_room(window) - self.call_chat_width(window) < CHAT_DOCKS_FROM
    }

    // The stage's width with no chat beside it.
    fn call_stage_room(&self, window: &Window) -> f32 {
        let beside = if self.call_fullscreen { 0. } else { RAIL_WIDTH + self.channels_width };
        f32::from(window.viewport_size().width) - beside
    }

    // The voice chat as a card over the stage's right edge, sliding in from it as it opens.
    pub(super) fn floating_chat(&self, panel: AnyElement, width: f32, reveal: f32, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let width = width.min(self.call_stage_room(window) - FLOAT_INSET * 2.);
        self.above_game(
            // lint-allow(custom-surface, custom-mark): ui::surface insets its content and rings it, and this card's own panel fills it and slides in by a computed offset
            div()
                .id("call-chat-float")
                .occlude()
                .absolute()
                .top_2()
                .bottom_2()
                .right(px(FLOAT_INSET - (1. - reveal) * FLOAT_SLIDE))
                .w(px(width))
                .opacity(reveal)
                .squircle(RADIUS_LG)
                .overflow_hidden()
                // Opaque under the chat's own fill, which docked beside the stage could let the stage show through.
                .bg(palette::surface(palette::Level::Page, cx))
                .shadow(ui::shadow_pop(cx))
                .child(panel),
        )
    }

    // The control bar for the width its slot has this frame (`Tray`): Invite at the left, then the mic and camera,
    // share, activities, soundboard and More, Disconnect, and the volume and full screen at the right.
    pub(super) fn call_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.weak_entity();
        div().w_full().h(px(CONTROLS_HEIGHT)).child(ui::sized(move |size, window, cx| {
            let tray = Tray::for_width(f32::from(size.width));
            let carets = f32::from(window.viewport_size().width) > CARETS_UNTIL;
            this.update(cx, |waffle, cx| waffle.tray(tray, carets, cx).into_any_element()).unwrap_or_else(|_| div().into_any_element())
        }))
    }

    fn tray(&self, tray: Tray, carets: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let call = self.call.as_ref().expect("in a call");
        let connected = call.stage == Stage::Connected;
        // While a game runs the control stays pressed, its app's icon in the corner.
        let running = self.running_app_icon(cx);
        let launcher = if self.activities.launcher.is_some() || running.is_some() { Control::Open } else { Control::Rest };
        let activities = control("call-activities", Glyph::ActivitiesFill, "Start An Activity", launcher, connected)
            .relative()
            .children(running)
            .when(connected, |b| b.on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, window, cx| this.toggle_launcher(e.position, window, cx))));
        let access = self.soundboard_access();
        let soundboard = (access != soundboard::Access::Hidden).then(|| {
            let on = connected && access == soundboard::Access::Ready;
            control("call-soundboard", Glyph::Soundboard, access.tip(), if self.soundboard_open() { Control::Open } else { Control::Rest }, on)
                .relative()
                .child(self.soundboard_probe(soundboard::CALL_CONTROL))
                .when(on, |b| b.on_click(cx.listener(|this, _, window, cx| this.open_soundboard(Some(soundboard::CALL_CONTROL), window, cx))))
        });
        let more = control("call-more", Glyph::More, "More Options", if self.menu.is_some() { Control::Open } else { Control::Rest }, true)
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_call_more(e.position, cx)))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_call_more(e.position, cx)));
        let invite = (tray == Tray::Full).then(|| call.guild_id.clone().map(|guild| (guild, call.channel.id.clone()))).flatten().map(|(guild, channel)| {
            stage_icon_button("call-invite", Glyph::GroupPlus, "Invite to Voice", false, cx).on_click(cx.listener(move |this, _, window, cx| this.show_invite(guild.clone(), channel.clone(), window, cx)))
        });
        let fullscreen = self.call_fullscreen;
        let fullscreen = stage_icon_button("call-fullscreen", if fullscreen { Glyph::FullscreenExit } else { Glyph::Fullscreen }, if fullscreen { "Exit Full Screen" } else { "Full Screen" }, false, cx)
            .on_click(cx.listener(move |this, _, window, cx| this.set_call_fullscreen(!fullscreen, window, cx)));
        // A focused stream's volume (or a focused person's) sits beside it, as Discord's.
        let me = self.me.as_ref().map(|m| m.id.clone());
        let volume = self.call_focus.as_deref().and_then(|key| match key.split_once(':') {
            Some(("stream", id)) if call.watched(id).is_some() => Some((id.to_owned(), true)),
            Some(("user", id)) if me.as_deref() != Some(id) => Some((id.to_owned(), false)),
            _ => None,
        });
        let volume = volume.map(|(id, stream)| {
            stage_icon_button("call-focus-volume", Glyph::VolumeHigh, if stream { "Stream Volume" } else { "User Volume" }, false, cx).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| match stream {
                    true => this.open_stream_volume(id.clone(), e.position, cx),
                    false => this.voice_user_menu(&id, e.position, cx),
                }),
            )
        });
        // Floats over the focused tile: without occluding, a press on any control also reached the tile under it,
        // whose toggle re-focused (and re-watched) what Stop watching ended.
        keeps_chrome(div().id("call-controls"), cx)
            .occlude()
            .relative()
            .h(px(CONTROLS_HEIGHT))
            .flex()
            .justify_center()
            .items_center()
            .gap_3()
            .child(control_group().child(self.mic_control(carets, cx)).child(self.camera_control(carets, cx)))
            .when(tray != Tray::Minimal, |bar| bar.child(control_group().child(self.share_control(cx)).child(activities).children(soundboard).child(more)))
            .child(self.leave_control(cx))
            .child(ui::row().absolute().left_0().top_0().h(px(CONTROLS_HEIGHT)).children(invite))
            .child(ui::row().absolute().right_0().top_0().h(px(CONTROLS_HEIGHT)).gap_2().children(volume).child(fullscreen))
    }

    // The one red button. Watching the stream shown big, it stops watching it rather than leaving, as
    // Discord's. One element across both, so a press that spans the switch still lands, and the glyph
    // cross-fades into the new action.
    fn leave_control(&self, cx: &mut Context<Self>) -> ui::Pressable {
        let call = self.call.as_ref().expect("in a call");
        let me = self.me.as_ref().map(|m| m.id.as_str());
        let watching = self.focused_stream().filter(|id| call.watched(id).is_some() && me != Some(id.as_str()));
        let (glyph, tip) = match watching {
            Some(_) => (Glyph::ScreenOff, "Stop watching"),
            None => self.red_button_look(),
        };
        let icon = div().child(Icon::new(glyph).size(ui::ICON_XL)).with_animation(
            ElementId::Name(SharedString::from(format!("call-leave-{tip}"))),
            Animation::new(ui::DUR_POP).with_easing(|t| crate::motion::ease_out().sample(t)),
            |icon, t| icon.opacity(t).scale(0.25 + 0.75 * t),
        );
        ui::pressable(div().id("call-leave"), 0.92)
            .w(px(LEAVE_WIDTH))
            .h(px(CONTROLS_HEIGHT))
            .flex_none()
            .squircle(RADIUS_MD)
            .border_1()
            .border_color(stage::wash(0.2))
            .flex()
            .items_center()
            .justify_center()
            .bg(palette::danger())
            .text_color(gpui_kit::white())
            .cursor_pointer()
            .hover(|s| s.bg(palette::danger_hover()))
            .child(icon)
            .tooltip(move |window, cx| ui::tip(tip, window, cx))
            .on_click(cx.listener(move |this, _, _, cx| match watching.clone() {
                Some(id) => {
                    this.call_focus = None;
                    this.unwatch_stream(&id, cx);
                }
                None => this.press_red_button(cx),
            }))
    }

    // More Options: what didn't earn a button of its own, then Disconnect, as Discord's.
    fn open_call_more(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.open_live_menu(
            at,
            move |this| {
                let Some(call) = &this.call else { return Vec::new() };
                let mut entries = Vec::new();
                if let Some(guild) = call.guild_id.clone() {
                    let id = call.channel.id.clone();
                    let on = this.call_previews_enabled(&id);
                    entries.push(item("Show New Message Previews", move |this, _, cx| this.toggle_call_previews(&id, cx)).checked(on).entry());
                    let channel = call.channel.id.clone();
                    entries.push(item("Invite to Voice", move |this, window, cx| this.show_invite(guild.clone(), channel.clone(), window, cx)).icon(Glyph::GroupPlus).entry());
                    entries.push(Entry::Separator);
                }
                entries.push(item("Disconnect", |this, _, cx| this.leave_voice(cx)).danger().icon(Glyph::CallEnd).entry());
                entries
            },
            cx,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chat_stops_widening_and_docks_on_a_big_screen() {
        assert_eq!(chat_width(2000., 3000.), CHAT_MAX_WIDTH);
        assert_eq!(chat_width(500., 3000.), 500.);
        assert_eq!(chat_width(100., 3000.), CHAT_MIN_WIDTH);
        // A 1600-wide window with the default rail and channel list: the widest chat still leaves the stage room.
        assert!(1600. - (RAIL_WIDTH + crate::layout::CHANNELS_WIDTH) - chat_width(5000., 1600.) >= CHAT_DOCKS_FROM);
    }
}
