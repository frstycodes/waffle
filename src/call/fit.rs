// How the call stage fits the space it has: the control bar folds what matters least into its menu as it
// narrows, and the voice chat floats over the stage once docking it beside the stage would crowd it.
use super::*;

// The bar's slot widths it folds below. Whole, the bar is ~400 wide and its corner (stream volume, full screen)
// needs as much again on the far side to keep the bar centred; folded to mic, camera, More and leave, ~250.
const FULL_FROM: f32 = 560.;
const COMPACT_FROM: f32 = 420.;
// The stage keeps at least this width beside a docked chat; narrower, the chat floats over it instead.
const CHAT_DOCKS_FROM: f32 = 480.;
// The chat is never wider than this, so on a big screen it always docks beside the stage rather than float.
pub(super) const CHAT_MAX_WIDTH: f32 = 640.;
// The floating chat's inset from the stage's edges, and how far it slides in from the right as it opens.
const FLOAT_INSET: f32 = 8.;
const FLOAT_SLIDE: f32 = 16.;
// The red button: wider than the bar's other controls, so leaving is the easiest press to find.
const LEAVE_WIDTH: f32 = 56.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Dock {
    Full,
    // Share, soundboard and activities fold into More.
    Compact,
    // Full screen and the stream's volume fold in too.
    Tight,
}

impl Dock {
    fn for_width(width: f32) -> Self {
        if width >= FULL_FROM {
            Dock::Full
        } else if width >= COMPACT_FROM {
            Dock::Compact
        } else {
            Dock::Tight
        }
    }
}

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

    // The control bar, folded to the width its slot has this frame (`Dock`).
    pub(super) fn call_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.weak_entity();
        div().w_full().h(px(CONTROLS_HEIGHT)).child(ui::sized(move |size, _, cx| {
            let fold = Dock::for_width(f32::from(size.width));
            this.update(cx, |waffle, cx| waffle.dock(fold, cx).into_any_element()).unwrap_or_else(|_| div().into_any_element())
        }))
    }

    fn dock(&self, fold: Dock, cx: &mut Context<Self>) -> Stateful<Div> {
        let call = self.call.as_ref().expect("in a call");
        let connected = call.stage == Stage::Connected;
        let (mic, camera, share) = (self.mic_control(cx), self.camera_control(cx), self.share_control(cx));
        // While a game runs the control stays pressed, its app's icon in the corner.
        let running = self.running_app_icon(cx);
        let launcher = if self.activities.launcher.is_some() || running.is_some() { Control::Open } else { Control::Rest };
        let activities = control("call-activities", Glyph::ActivitiesFill, "Activities", launcher, connected)
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
        let more = control("call-more", Glyph::More, "More options", Control::Rest, true).on_mouse_down(MouseButton::Left, cx.listener(move |this, e: &MouseDownEvent, _, cx| this.open_call_more(e.position, fold, cx)));
        let leave = self.leave_control(cx);
        // Full screen at the bar's right end, as Discord's.
        let fullscreen = self.call_fullscreen;
        let fullscreen = stage_icon_button("call-fullscreen", if fullscreen { Glyph::FullscreenExit } else { Glyph::Fullscreen }, if fullscreen { "Exit full screen (Esc)" } else { "Full screen" }, false)
            .on_click(cx.listener(move |this, _, window, cx| this.set_call_fullscreen(!fullscreen, window, cx)));
        // A focused stream's volume sits beside it, as Discord's.
        let volume = self.focused_stream().filter(|id| call.watched(id).is_some()).map(|id| {
            stage_icon_button("call-stream-volume", Glyph::VolumeHigh, "Stream volume", false).on_mouse_down(MouseButton::Left, cx.listener(move |this, e: &MouseDownEvent, _, cx| this.open_stream_volume(id.clone(), e.position, cx)))
        });
        // Floats over the focused card in Focus layout: without occluding, a press on any control
        // also reached the card under it, whose toggle re-focused (and re-watched) what Stop watching ended.
        keeps_chrome(div().id("call-controls"), cx)
            .occlude()
            .relative()
            .h(px(CONTROLS_HEIGHT))
            .flex()
            .justify_center()
            .items_center()
            .gap_2()
            .child(control_group().child(mic).child(camera))
            .child(match fold {
                Dock::Full => control_group().child(share).children(soundboard).child(activities).child(more),
                Dock::Compact | Dock::Tight => control_group().child(more),
            })
            .child(leave)
            .when(fold != Dock::Tight, |bar| bar.child(div().absolute().right_0().top_0().h(px(CONTROLS_HEIGHT)).flex().items_center().gap_1().children(volume).child(fullscreen)))
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
            .squircle(10. + DOCK_PAD)
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

    // More options: what didn't earn a button of its own.
    // What the bar has folded away leads the menu: share and activities from a compact bar, then full screen
    // and the stream's volume from a tight one.
    fn open_call_more(&mut self, at: Point<Pixels>, fold: Dock, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.open_live_menu(
            at,
            move |this| {
                let Some(call) = &this.call else { return Vec::new() };
                let mut entries = Vec::new();
                if fold != Dock::Full && call.stage == Stage::Connected {
                    let sharing = call.sharing.is_some();
                    entries.push(item(if sharing { "Stop streaming" } else { "Share your screen" }, |this, _, cx| this.toggle_screen_share(cx)).icon(Glyph::ShareScreenFill).entry());
                    entries.push(item("Activities", move |this, window, cx| this.toggle_launcher(at, window, cx)).icon(Glyph::ActivitiesFill).entry());
                }
                if fold == Dock::Tight {
                    let full = this.call_fullscreen;
                    let glyph = if full { Glyph::FullscreenExit } else { Glyph::Fullscreen };
                    entries.push(item(if full { "Exit full screen" } else { "Full screen" }, move |this, window, cx| this.set_call_fullscreen(!full, window, cx)).icon(glyph).entry());
                    if let Some(id) = this.focused_stream().filter(|id| call.watched(id).is_some()) {
                        entries.push(item("Stream volume", move |this, _, cx| this.open_stream_volume(id.clone(), at, cx)).icon(Glyph::VolumeHigh).entry());
                    }
                }
                if let Some(guild) = call.guild_id.clone() {
                    let channel = call.channel.id.clone();
                    entries.push(item("Invite to voice", move |this, window, cx| this.show_invite(guild.clone(), channel.clone(), window, cx)).icon(Glyph::GroupPlus).entry());
                }
                if this.soundboard_access() == soundboard::Access::Ready {
                    entries.push(item("Soundboard", |this, window, cx| this.open_soundboard(Some(soundboard::CALL_CONTROL), window, cx)).icon(Glyph::Soundboard).entry());
                }
                if this.call_focus.is_some() {
                    let hidden = this.call_strip_hidden;
                    entries.push(
                        item(if hidden { "Show everyone" } else { "Hide everyone" }, |this, _, cx| {
                            this.call_strip_hidden = !this.call_strip_hidden;
                            cx.notify(); // lint-allow(global-notify): the stage is the main column's, or full screen the root's
                        })
                        .hint("The cards under the focused one")
                        .icon(Glyph::Users)
                        .entry(),
                    );
                }
                if call.guild_id.is_some() {
                    let id = call.channel.id.clone();
                    let on = this.call_previews_enabled(&id);
                    entries.push(item("Show New Message Previews", move |this, _, cx| this.toggle_call_previews(&id, cx)).checked(on).entry());
                }
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
    fn the_bar_folds_as_it_narrows() {
        assert_eq!(Dock::for_width(900.), Dock::Full);
        assert_eq!(Dock::for_width(FULL_FROM), Dock::Full);
        assert_eq!(Dock::for_width(FULL_FROM - 1.), Dock::Compact);
        assert_eq!(Dock::for_width(COMPACT_FROM), Dock::Compact);
        assert_eq!(Dock::for_width(COMPACT_FROM - 1.), Dock::Tight);
        assert_eq!(Dock::for_width(0.), Dock::Tight);
    }

    #[test]
    fn the_chat_stops_widening_and_docks_on_a_big_screen() {
        assert_eq!(chat_width(2000., 3000.), CHAT_MAX_WIDTH);
        assert_eq!(chat_width(500., 3000.), 500.);
        assert_eq!(chat_width(100., 3000.), CHAT_MIN_WIDTH);
        // A 1600-wide window with the default rail and channel list: the widest chat still leaves the stage room.
        assert!(1600. - (RAIL_WIDTH + crate::layout::CHANNELS_WIDTH) - chat_width(5000., 1600.) >= CHAT_DOCKS_FROM);
    }
}
