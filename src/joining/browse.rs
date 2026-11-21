// Channels & Roles: the Customize and Browse Channels tabs, and which channels the list shows when you
// pick your own (design/explore-joining.html §4).
use gpui_kit::{AnyElement, Context, IntoElement, Window, div};

use crate::{
    Waffle,
    discord::{Channel, Guild},
};

#[derive(Default)]
pub struct BrowseView {}

impl Waffle {
    pub(crate) fn render_channels_roles(&mut self, _guild: &Guild, _window: &mut Window, _cx: &mut Context<Self>) -> AnyElement {
        div().into_any_element()
    }

    // Whether the channel list shows this channel (opt-in channels hide what you don't follow).
    pub(crate) fn shows_in_list(&self, _guild: &Guild, _channel: &Channel) -> bool {
        true
    }
}

pub fn demo(_waffle: &mut Waffle, _scene: &str, _window: &mut Window, _cx: &mut Context<Waffle>) {}
