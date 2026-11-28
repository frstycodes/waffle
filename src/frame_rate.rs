// A framerate counter for testing, switched on under Advanced › Developer. While it is on the window keeps drawing
// every frame (the counter asks for the next one), so the figure is the rate the app can sustain right now: it
// dips when something heavy runs, where an idle window would show nothing. Off, it draws nothing and asks for nothing.
use std::{collections::VecDeque, time::{Duration, Instant}};

use gpui_kit::{Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window, component::ActiveTheme, div};

use crate::{Waffle, palette, ui};

// Below this the app is visibly dropping frames.
const SLOW: usize = 45;
const WINDOW: Duration = Duration::from_secs(1);

pub struct FrameRate {
    on: bool,
    frames: VecDeque<Instant>,
    _follows_setting: Subscription,
}

impl FrameRate {
    pub fn new(app: &Entity<Waffle>, cx: &mut Context<Self>) -> Self {
        // Follows the setting: a change of it is the only thing here that redraws the counter from outside.
        let follows = cx.observe(app, |this, app, cx| {
            let on = app.read(cx).prefs.show_fps;
            if on != this.on {
                this.on = on;
                this.frames.clear();
                cx.notify();
            }
        });
        Self { on: app.read(cx).prefs.show_fps, frames: VecDeque::new(), _follows_setting: follows }
    }
}

impl Render for FrameRate {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::perf::probe("FrameRate");
        if !self.on {
            return div().into_any_element();
        }
        let now = Instant::now();
        self.frames.push_back(now);
        while self.frames.front().is_some_and(|at| now.duration_since(*at) > WINDOW) {
            self.frames.pop_front();
        }
        // lint-allow(endless-animation): only while the setting is on; turning it off draws nothing and asks for nothing.
        window.request_animation_frame();
        let fps = self.frames.len();
        let ink = if fps < SLOW { palette::danger_ink(cx) } else { palette::strong(cx) };
        let badge = ui::Badge::new(format!("{fps} fps")).variant(ui::BadgeVariant::Secondary).bg(palette::surface(palette::Level::Overlay, cx)).text_color(ink).font_family(cx.theme().mono_font_family.clone());
        ui::row().absolute().top_2().right_2().child(badge).into_any_element()
    }
}
