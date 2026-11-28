// The prompt, confirmation and notice dialogs that menu actions open.
use super::*;

// A dialog's 420px less its padding, the well's and the hit's avatar column.
pub(super) const PREVIEW_COLUMN: f32 = 420. - 48. - 24. - 48.;

// A one-field dialog (nickname, note) or a confirmation (kick, ban, remove friend).
pub struct Dialog {
    title: SharedString,
    body: SharedString,
    confirm: SharedString,
    pub(crate) danger: bool,
    // Leads the title with the warning mark: for what can't be undone.
    warn: bool,
    pub(crate) input: bool,
    // A key that confirms it from anywhere, drawn on the confirm button (D for a message's delete).
    key: Option<&'static str>,
    // The message it acts on, drawn as search draws a hit (Discord's unpin confirm).
    preview: Option<Box<crate::discord::Message>>,
    // A live part of the dialog (the camera preview), drawn under the body.
    view: Option<gpui_kit::AnyView>,
    // Discord's "Protip:" under the preview.
    tip: Option<SharedString>,
    // A muted line under the body.
    note: Option<SharedString>,
    other: Other,
    on_confirm: OnConfirm,
}

// The button beside the confirm: Cancel, none (a notice the confirm only closes), or a second way through
// (Discord's "No, just disconnect quietly").
pub(super) enum Other {
    Cancel,
    CancelAs(SharedString),
    None,
    Instead(SharedString, OnConfirm),
}

// The field with its focus ring, which it holds while its dialog is open: the dialog focuses it on open.
pub(super) fn field(cx: &App) -> Div {
    let ring = ui::shadow(palette::accent_soft(cx), 0., 0., 0., 3., false);
    ui::Input::frame(cx).border_color(palette::accent()).shadow(vec![ring])
}

impl Waffle {
    // gpui's window and context ride along with the dialog's five fields.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prompt(
        &mut self,
        title: impl Into<SharedString>,
        body: impl Into<SharedString>,
        value: &str,
        confirm: impl Into<SharedString>,
        on_confirm: impl Fn(&mut Waffle, String, &mut Window, &mut Context<Waffle>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.menu = None;
        self.dialog_input.update(cx, |state, cx| {
            state.set_value(value.to_owned(), window, cx);
            state.focus(window, cx);
        });
        self.dialog = Some(Dialog { title: title.into(), body: body.into(), confirm: confirm.into(), danger: false, warn: false, input: true, key: None, preview: None, view: None, tip: None, note: None, other: Other::Cancel, on_confirm: Rc::new(on_confirm) });
        cx.notify(); // lint-allow(global-notify): the menu and its dialogs are drawn by the root, over the columns
    }

    pub(crate) fn confirm(
        &mut self,
        title: impl Into<SharedString>,
        body: impl Into<SharedString>,
        confirm: impl Into<SharedString>,
        on_confirm: impl Fn(&mut Waffle, &mut Window, &mut Context<Waffle>) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.menu = None;
        let on_confirm = Rc::new(on_confirm);
        self.dialog = Some(Dialog {
            title: title.into(),
            body: body.into(),
            confirm: confirm.into(),
            danger: true,
            warn: false,
            input: false,
            key: None,
            preview: None,
            view: None,
            tip: None,
            note: None,
            other: Other::Cancel,
            on_confirm: Rc::new(move |this, _, window, cx| on_confirm(this, window, cx)),
        });
        cx.notify(); // lint-allow(global-notify): the menu and its dialogs are drawn by the root, over the columns
    }

    pub(crate) fn alert(&mut self, title: impl Into<SharedString>, body: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.confirm(title, body, "Okay", |_, _, _| {}, cx);
        if let Some(dialog) = &mut self.dialog {
            dialog.danger = false;
            dialog.other = Other::None;
        }
    }

    // A confirm for a deletion that can't be undone: the title leads with the warning mark.
    pub(crate) fn confirm_delete(
        &mut self,
        title: impl Into<SharedString>,
        body: impl Into<SharedString>,
        confirm: impl Into<SharedString>,
        on_confirm: impl Fn(&mut Waffle, &mut Window, &mut Context<Waffle>) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.confirm(title, body, confirm, on_confirm, cx);
        if let Some(dialog) = &mut self.dialog {
            dialog.warn = true;
        }
    }

    // A confirm whose other button acts too, in place of Cancel.
    pub(crate) fn set_dialog_instead(&mut self, label: impl Into<SharedString>, action: impl Fn(&mut Waffle, &mut Window, &mut Context<Waffle>) + 'static) {
        if let Some(dialog) = &mut self.dialog {
            dialog.other = Other::Instead(label.into(), Rc::new(move |this, _, window, cx| action(this, window, cx)));
        }
    }

    // A notice: one button, which only closes it.
    pub(crate) fn notice(&mut self, title: impl Into<SharedString>, body: impl Into<SharedString>, button: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.confirm(title, body, button, |_, _, _| {}, cx);
        if let Some(dialog) = &mut self.dialog {
            dialog.danger = false;
            dialog.other = Other::None;
        }
    }

    pub(crate) fn confirm_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.dialog.take() else { return };
        let value = self.dialog_input.read(cx).value().to_string();
        (dialog.on_confirm)(self, value, window, cx);
        cx.notify(); // lint-allow(global-notify): a confirmed dialog can run any action, which changes what any column draws
    }

    pub(crate) fn set_dialog_key(&mut self, key: &'static str) {
        if let Some(dialog) = &mut self.dialog {
            dialog.key = Some(key);
        }
    }

    pub(crate) fn set_dialog_view(&mut self, view: gpui_kit::AnyView) {
        if let Some(dialog) = &mut self.dialog {
            dialog.view = Some(view);
        }
    }

    pub(crate) fn set_dialog_preview(&mut self, message: crate::discord::Message, tip: impl Into<SharedString>) {
        if let Some(dialog) = &mut self.dialog {
            dialog.preview = Some(Box::new(message));
            dialog.tip = Some(tip.into());
        }
    }

    // The dismiss button's label when it isn't "Cancel" (Discord's "Never Mind").
    pub(crate) fn set_dialog_cancel(&mut self, label: impl Into<SharedString>) {
        if let Some(dialog) = &mut self.dialog {
            dialog.other = Other::CancelAs(label.into());
        }
    }

    pub(crate) fn set_dialog_note(&mut self, note: impl Into<SharedString>) {
        if let Some(dialog) = &mut self.dialog {
            dialog.note = Some(note.into());
        }
    }

    pub(crate) fn dialog_key(&self) -> Option<&'static str> {
        self.dialog.as_ref().and_then(|d| d.key)
    }

    pub(super) fn dialog_other(&self, cx: &mut Context<Self>) -> Option<ui::Button> {
        let button = |label: SharedString| ui::Button::new("dialog-cancel").variant(ui::ButtonVariant::Secondary).child(label);
        let dismiss = |label: SharedString| {
            button(label).on_click(cx.listener(|this, _, _, cx| {
                this.dialog = None;
                cx.notify(); // lint-allow(global-notify): the menu and its dialogs are drawn by the root, over the columns
            }))
        };
        match &self.dialog.as_ref()?.other {
            Other::None => None,
            Other::Cancel => Some(dismiss("Cancel".into())),
            Other::CancelAs(label) => Some(dismiss(label.clone())),
            Other::Instead(label, _) => Some(button(label.clone()).on_click(cx.listener(|this, _, window, cx| {
                let Some(Dialog { other: Other::Instead(_, action), .. }) = this.dialog.take() else { return };
                action(this, String::new(), window, cx);
                cx.notify(); // lint-allow(global-notify): the dialog's second action can change what any column draws
            }))),
        }
    }

    pub(crate) fn render_dialog(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.dialog.as_ref()?;
        let submit = cx.listener(|this, _, window, cx| this.confirm_dialog(window, cx));
        let variant = if dialog.danger { ui::ButtonVariant::Destructive } else { ui::ButtonVariant::Default };
        let preview = dialog.preview.as_deref().map(|m| {
            let ctx = self.row_context(std::slice::from_ref(m), None, false, PREVIEW_COLUMN, cx);
            div().p_3().squircle(RADIUS_MD).bg(palette::surface(palette::Level::Page, cx)).child(crate::chat::search_hit(m, &ctx, cx))
        });
        let body = ui::DialogContent::new()
            .child(
                ui::row()
                    .gap_2p5()
                    .text_size(ui::TEXT_LG)
                    .line_height(ui::LINE_LG)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette::strong(cx))
                    .when(dialog.warn, |d| d.child(div().text_color(palette::danger_ink(cx)).child(Icon::new(Glyph::Warning).size(px(WARN_MARK)))))
                    .child(dialog.title.clone()),
            )
            .when(!dialog.body.is_empty(), |d| d.child(div().text_size(ui::TEXT_SM).line_height(ui::LINE_SM).text_color(palette::muted(cx)).child(dialog.body.clone())))
            .children(dialog.view.clone())
            .children(dialog.note.clone().map(|note| div().text_size(ui::TEXT_XS).line_height(ui::LINE_XS).text_color(palette::faint(cx)).child(note)))
            .children(preview)
            .children(dialog.tip.clone().map(|tip| {
                div().text_size(ui::TEXT_SM).line_height(ui::LINE_SM).text_color(palette::text(cx)).child(div().font_weight(FontWeight::SEMIBOLD).text_color(palette::positive_ink(cx)).child("Protip:")).child(tip)
            }))
            .when(dialog.input, |d| d.child(field(cx).child(div().flex_1().min_w_0().child(Input::new(&self.dialog_input).appearance(false)))))
            .child(
                ui::row().items_stretch()
                    .justify_end()
                    .gap_2()
                    .children(self.dialog_other(cx))
                    .child(ui::Button::new("dialog-confirm").variant(variant).child(dialog.confirm.clone()).when_some(dialog.key, |b, key| b.children(key.split('+').map(|k| ui::Kbd::new(crate::settings::key_name(k)).translucent(gpui_kit::white())))).on_click(submit)),
            );
        let backdrop = div().id("dialog-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
            this.dialog = None;
            cx.notify(); // lint-allow(global-notify): the menu and its dialogs are drawn by the root, over the columns
        }));
        Some(ui::Dialog::modal(backdrop, ui::Dialog::new("dialog", 420., body, window, cx), cx))
    }
}
