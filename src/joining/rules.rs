// The rules gate: the composer bar for members who haven't agreed yet, and its dialog (design/explore-joining.html §1).
use gpui_kit::{AnyElement, Context, Window};

use crate::{Waffle, discord::Channel};

pub struct RulesView {}

impl Waffle {
    // Replaces the composer while you can't talk in this channel's server yet.
    pub(crate) fn composer_gate(&mut self, _channel: &Channel, _window: &mut Window, _cx: &mut Context<Self>) -> Option<AnyElement> {
        None
    }

    pub(crate) fn rules_overlay(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<AnyElement> {
        None
    }
}

pub fn demo(_waffle: &mut Waffle, _scene: &str, _window: &mut Window, _cx: &mut Context<Waffle>) {}
