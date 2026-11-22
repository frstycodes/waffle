// Settings › Accessibility: Discord's five sections (docs/discord-settings-re.md §2). Choice groups (Role colors,
// Play sticker animations) come from the registry; the rest is drawn here.
use gpui_kit::{Context, Window};

use super::Section;
use crate::Waffle;

impl Waffle {
    pub(super) fn accessibility_page(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> Vec<Section> {
        Vec::new()
    }
}
