// The rows atop a server's channel list: Get Started, Finish joining, Server Guide, Channels & Roles
// (design/explore-joining.html §3).
use gpui_kit::{AnyElement, Context, Window};

use crate::{Waffle, discord::Guild};

impl Waffle {
    pub(crate) fn guild_entry_rows(&mut self, _guild: &Guild, _window: &mut Window, _cx: &mut Context<Self>) -> Vec<AnyElement> {
        Vec::new()
    }
}
