// The Server Guide: welcome, Get Started to-dos and Resources, plus the to-do bar in a to-do's channel
// (design/explore-joining.html §5).
use gpui_kit::{AnyElement, Context, IntoElement, Window, div};

use crate::{
    Waffle,
    discord::{Channel, Guild},
};

#[derive(Default)]
pub struct GuideView {}

impl Waffle {
    pub(crate) fn render_guide(&mut self, _guild: &Guild, _window: &mut Window, _cx: &mut Context<Self>) -> AnyElement {
        div().into_any_element()
    }

    // Over the composer in a to-do's channel: its title and action, then what's next.
    pub(crate) fn guide_action_bar(&mut self, _channel: &Channel, _cx: &mut Context<Self>) -> Option<AnyElement> {
        None
    }

    // A VIEW to-do completes when its channel opens.
    pub(crate) fn note_channel_viewed(&mut self, _channel: &Channel, _cx: &mut Context<Self>) {}

    // (done, total) to-dos while you're a new member, for the Get Started row.
    pub(crate) fn guide_progress(&self, _guild: &Guild) -> Option<(usize, usize)> {
        None
    }
}

pub fn demo(_waffle: &mut Waffle, _scene: &str, _window: &mut Window, _cx: &mut Context<Waffle>) {}
