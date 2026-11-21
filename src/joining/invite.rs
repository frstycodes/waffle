// The invite screen: what you see before joining from an invite link (design/explore-joining.html §0).
use gpui_kit::{AnyElement, Context, Window};

use crate::Waffle;

pub struct InviteView {}

impl Waffle {
    // An invite link was opened: show the server before joining.
    pub(crate) fn open_invite(&mut self, code: String, cx: &mut Context<Self>) {
        self.join_invite(code, cx);
    }

    pub(crate) fn invite_overlay(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<AnyElement> {
        None
    }
}

pub fn demo(_waffle: &mut Waffle, _scene: &str, _window: &mut Window, _cx: &mut Context<Waffle>) {}
