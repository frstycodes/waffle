// Invite friends to a server, Discord's dialog: friends to send the link to, the link, and its settings.
use gpui_kit::{AnyElement, Context, Window};

use crate::Waffle;

pub(crate) struct Invite;

impl Waffle {
    pub(crate) fn open_invite(&mut self, _guild_id: String, _channel_id: String, _window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        self.invite = Some(Invite);
        cx.notify();
    }

    pub(crate) fn render_invite(&self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<AnyElement> {
        let _form = self.invite.as_ref()?;
        None
    }
}

pub(crate) fn demo(waffle: &mut Waffle, _scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    waffle.open_invite("1".into(), "c1".into(), window, cx);
}
