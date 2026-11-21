// Add a Server, Discord's dialog: Create My Own or a template, the intent question, name and icon, or Join a Server.
use gpui_kit::{AnyElement, Context, Window};

use crate::Waffle;

pub(crate) struct CreateServer;

impl Waffle {
    pub(crate) fn open_create_server(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        self.create_server = Some(CreateServer);
        cx.notify();
    }

    pub(crate) fn render_create_server(&self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<AnyElement> {
        let _form = self.create_server.as_ref()?;
        None
    }
}

pub(crate) fn demo(waffle: &mut Waffle, _scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    waffle.open_create_server(window, cx);
}
