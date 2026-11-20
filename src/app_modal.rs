// A form an app asked us to fill in (INTERACTION_MODAL_CREATE), drawn in Waffle's dialog.

use gpui_kit::gpui::Context;

use crate::{app_commands::Modal, Waffle};

// TODO(app-modal agent): the open form and its fields.
pub(crate) struct AppModal {}

impl Waffle {
    pub(crate) fn open_app_modal(&mut self, modal: Modal, cx: &mut Context<Self>) {
        self.drop_stand_in(&modal.id, cx);
    }
}
