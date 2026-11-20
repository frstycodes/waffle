// The `/` picker above the composer and the option chips a picked command fills in.

use gpui_kit::gpui::Context;

use crate::{app_commands::Choice, Waffle};

// TODO(command-picker agent): picker and option state.
#[derive(Default)]
pub(crate) struct CommandPicker {}

impl Waffle {
    pub(crate) fn on_autocomplete(&mut self, _nonce: String, _choices: Vec<Choice>, _cx: &mut Context<Self>) {}
}
