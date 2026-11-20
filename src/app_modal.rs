// A form an app asked us to fill in (INTERACTION_MODAL_CREATE), drawn in Waffle's dialog.

use gpui_kit::{
    AnyElement, Context, Entity, Focusable, FocusHandle, FontWeight, MouseButton, SharedString, Window,
    component::input::{Input, InputEvent, InputState, Textarea, TextareaState},
    div,
    prelude::*,
    px,
};

use crate::{
    Waffle,
    app_commands::{self, Modal, Origin, TextInput},
    icons::Glyph,
    interactions::InteractionKind,
    palette,
    ui::{self, Tone},
};

const DIALOG_WIDTH: f32 = 460.;

pub(crate) struct AppModal {
    modal: Modal,
    // One per field, built on the first render: the gateway event that opens the form has no window.
    editors: Vec<Editor>,
    // Each field's value as the app sent it: once anything differs, a click outside no longer closes it.
    initial: Vec<String>,
    // What Submit found wrong, field by field; cleared as each is fixed.
    problems: Vec<Option<String>>,
    // The nonce of the last submit: in flight while `interactions` holds it, refused once it's failed.
    sent: Option<String>,
}

enum Editor {
    Line(Entity<InputState>),
    Paragraph(Entity<TextareaState>),
}

impl Editor {
    fn value(&self, cx: &gpui_kit::App) -> String {
        match self {
            Editor::Line(s) => s.read(cx).value().to_string(),
            Editor::Paragraph(s) => s.read(cx).value().to_string(),
        }
    }

    fn focus_handle(&self, cx: &gpui_kit::App) -> FocusHandle {
        match self {
            Editor::Line(s) => s.read(cx).focus_handle(cx),
            Editor::Paragraph(s) => s.read(cx).focus_handle(cx),
        }
    }

    fn focus(&self, window: &mut Window, cx: &mut gpui_kit::App) {
        match self {
            Editor::Line(s) => s.update(cx, |s, cx| s.focus(window, cx)),
            Editor::Paragraph(s) => s.update(cx, |s, cx| s.focus(window, cx)),
        }
    }
}

impl Waffle {
    pub(crate) fn open_app_modal(&mut self, modal: Modal, cx: &mut Context<Self>) {
        self.drop_stand_in(&modal.id, cx);
        self.take_app_modal();
        let problems = vec![None; modal.fields.len()];
        let initial = modal.fields.iter().map(|f| f.value.clone().unwrap_or_default()).collect();
        self.app_modal = Some(AppModal { modal, editors: Vec::new(), initial, problems, sent: None });
        cx.notify();
    }

    fn build_app_modal_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(fields) = self.app_modal.as_ref().filter(|m| m.editors.is_empty()).map(|m| m.modal.fields.clone()) else { return };
        let editors: Vec<Editor> = fields.iter().map(|f| editor(f, window, cx)).collect();
        if let Some(first) = editors.first() {
            first.focus(window, cx);
        }
        if let Some(m) = self.app_modal.as_mut() {
            m.editors = editors;
        }
    }

    // Once Submit has named a problem, it clears as soon as it's fixed.
    fn on_app_modal_change(&mut self, cx: &mut Context<Self>) {
        let Some(m) = self.app_modal.as_ref() else { return };
        if m.problems.iter().any(Option::is_some) {
            let problems = check(&m.modal.fields, &app_modal_values(m, cx));
            if let Some(m) = self.app_modal.as_mut() {
                m.problems = problems;
            }
        }
        cx.notify();
    }

    fn submit_app_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.app_modal_busy() {
            return;
        }
        let Some(m) = self.app_modal.as_ref() else { return };
        let values = app_modal_values(m, cx);
        let problems = check(&m.modal.fields, &values);
        if let Some(ix) = problems.iter().position(Option::is_some) {
            m.editors[ix].focus(window, cx);
            if let Some(m) = self.app_modal.as_mut() {
                m.problems = problems;
            }
            return cx.notify();
        }
        let (app, body, nonce) = {
            let nonce = crate::nonce();
            let channel_id = m.modal.channel_id.as_str();
            let guild_id = self.guilds.iter().find(|g| g.channels.iter().any(|c| c.id == channel_id)).map(|g| g.id.as_str());
            let origin = Origin { channel_id, guild_id, session_id: &self.gateway_session, nonce: &nonce };
            (m.modal.application.clone(), app_commands::modal_body(&m.modal, &values, &origin), nonce)
        };
        self.forget_refused_submit();
        if let Some(m) = self.app_modal.as_mut() {
            m.problems = vec![None; m.modal.fields.len()];
            m.sent = Some(nonce.clone());
        }
        self.interact(nonce, app, InteractionKind::Modal, body, cx);
    }

    fn app_modal_busy(&self) -> bool {
        self.app_modal_submit().is_some_and(|failed| !failed)
    }

    // The last submit's state: Some(false) in flight, Some(true) refused, None when there's none to wait on.
    fn app_modal_submit(&self) -> Option<bool> {
        let nonce = self.app_modal.as_ref()?.sent.as_ref()?;
        self.interactions.get(nonce).map(|p| p.failed)
    }

    fn forget_refused_submit(&mut self) {
        let Some(nonce) = self.app_modal.as_ref().and_then(|m| m.sent.clone()) else { return };
        if self.interactions.get(&nonce).is_some_and(|p| p.failed) {
            self.interactions.remove(&nonce);
        }
    }

    /// Drops the form and what was typed; true if one was open.
    pub(crate) fn take_app_modal(&mut self) -> bool {
        self.forget_refused_submit();
        self.app_modal.take().is_some()
    }

    fn close_app_modal(&mut self, cx: &mut Context<Self>) {
        self.take_app_modal();
        cx.notify();
    }

    // A click outside closes the form only while nothing in it was typed or changed.
    fn dismiss_app_modal(&mut self, cx: &mut Context<Self>) {
        let Some(m) = self.app_modal.as_ref() else { return };
        if app_modal_values(m, cx) != m.initial {
            return;
        }
        self.close_app_modal(cx);
    }

    pub(crate) fn app_modal_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.build_app_modal_editors(window, cx);
        let m = self.app_modal.as_ref()?;
        // Sent and no longer tracked: the app took it.
        if m.sent.is_some() && self.app_modal_submit().is_none() {
            self.app_modal = None;
            return None;
        }
        let submit = self.app_modal_submit();
        let busy = submit == Some(false);
        let refused = submit == Some(true);
        let app = &m.modal.application;
        let fields: Vec<AnyElement> = m
            .modal
            .fields
            .iter()
            .zip(&m.editors)
            .zip(&m.problems)
            .enumerate()
            .map(|(ix, ((field, editor), problem))| field_row(ix, field, editor, problem.as_deref(), cx).into_any_element())
            .collect();
        let footer = div()
            .flex()
            .justify_end()
            .gap_2()
            .child(ui::button("app-modal-cancel", Tone::Secondary, cx).child("Cancel").on_click(cx.listener(|this, _, _, cx| this.close_app_modal(cx))))
            .child(
                ui::button("app-modal-submit", Tone::Primary, cx)
                    .loading(busy.then_some("Submitting…"))
                    .child("Submit")
                    .on_click(cx.listener(|this, _, window, cx| this.submit_app_modal(window, cx))),
            );
        let body = ui::dialog_body()
            .child(head(app, &m.modal.title, cx))
            .children(fields)
            .children(refused.then(|| ui::field_error(format!("{} didn't accept this. Try again.", app.name), cx)))
            .child(footer);
        let backdrop = div().id("app-modal-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.dismiss_app_modal(cx)));
        Some(ui::modal(backdrop, ui::dialog("app-modal", DIALOG_WIDTH, body, window, cx), cx))
    }

    // Demo scenes: the form after a Submit the app refused, with whatever `check` finds as typed.
    #[cfg(debug_assertions)]
    pub(crate) fn refuse_app_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.build_app_modal_editors(window, cx);
        let Some(m) = self.app_modal.as_mut() else { return };
        let nonce = "demo-refused".to_owned();
        m.sent = Some(nonce.clone());
        let pending = crate::interactions::PendingInteraction {
            channel_id: m.modal.channel_id.clone(),
            app: m.modal.application.clone(),
            id: None,
            kind: InteractionKind::Modal,
            body: serde_json::Value::Null,
            failed: true,
        };
        self.interactions.insert(nonce, pending);
        let Some(m) = self.app_modal.as_ref() else { return };
        let problems = check(&m.modal.fields, &app_modal_values(m, cx));
        if let Some(ix) = problems.iter().position(Option::is_some) {
            m.editors[ix].focus(window, cx);
        }
        if let Some(m) = self.app_modal.as_mut() {
            m.problems = problems;
        }
    }
}

fn app_modal_values(m: &AppModal, cx: &gpui_kit::App) -> Vec<String> {
    m.editors.iter().map(|e| e.value(cx)).collect()
}

// A field's editor, prefilled with the app's value. Enter in a one-line field submits the form.
fn editor(field: &TextInput, window: &mut Window, cx: &mut Context<Waffle>) -> Editor {
    let placeholder = field.placeholder.clone().unwrap_or_default();
    let value = field.value.clone().unwrap_or_default();
    if field.paragraph {
        let state = cx.new(|cx| TextareaState::new(window, cx).placeholder(placeholder));
        state.update(cx, |s, cx| s.set_value(value, window, cx));
        cx.subscribe_in(&state, window, |this: &mut Waffle, _, event: &InputEvent, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.on_app_modal_change(cx);
            }
        })
        .detach();
        return Editor::Paragraph(state);
    }
    let state = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
    state.update(cx, |s, cx| s.set_value(value, window, cx));
    cx.subscribe_in(&state, window, |this: &mut Waffle, _, event: &InputEvent, window, cx| match event {
        InputEvent::PressEnter { .. } => this.submit_app_modal(window, cx),
        InputEvent::Change => this.on_app_modal_change(cx),
        _ => {}
    })
    .detach();
    Editor::Line(state)
}

// The app's tile, the form's title, who's asking, and the way out.
fn head(app: &app_commands::App, title: &str, cx: &mut Context<Waffle>) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_3()
        .child(ui::server_tile(app.icon.as_deref(), &app.name, 40., cx))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().text_size(px(20.)).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(title.to_owned()))
                .child(div().text_size(px(14.)).text_color(palette::muted(cx)).child(format!("{} is asking", app.name))),
        )
        .child(ui::icon_button("app-modal-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.close_app_modal(cx))))
}

fn field_row(ix: usize, field: &TextInput, editor: &Editor, problem: Option<&str>, cx: &mut Context<Waffle>) -> impl IntoElement {
    let id = SharedString::from(format!("app-modal-field-{ix}"));
    let focus = editor.focus_handle(cx);
    let frame = match editor {
        Editor::Line(s) => ui::text_field(id, Input::new(s).appearance(false), &focus),
        Editor::Paragraph(s) => ui::text_area(id, Textarea::new(s).appearance(false).px_0().text_size(px(16.)), &focus),
    };
    let length = editor.value(cx).chars().count();
    let count = field.max.map(|max| (format!("{length} / {max}"), length > max as usize));
    let under = (problem.is_some() || count.is_some()).then_some(under_field(problem, count, cx));
    ui::field_marked(field.label.clone(), field.required, cx).child(frame.error(problem.is_some())).children(under)
}

// What's wrong (if anything) at the start, the running count at the end.
fn under_field(problem: Option<&str>, count: Option<(String, bool)>, cx: &gpui_kit::App) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(px(12.))
        .child(div().flex_1().min_w_0().children(problem.map(|p| ui::field_error(p.to_owned(), cx))))
        .children(count.map(|(text, over)| div().flex_none().text_color(if over { palette::danger_ink(cx) } else { palette::faint(cx) }).child(text)))
}

/// What stops each field from submitting, in field order.
fn check(fields: &[TextInput], values: &[String]) -> Vec<Option<String>> {
    fields.iter().zip(values).map(|(f, v)| problem(f, v)).collect()
}

// Not verified: Discord measures min/max_length in characters (not UTF-16 units), and skips min_length
// on an optional field left empty.
fn problem(field: &TextInput, value: &str) -> Option<String> {
    if value.trim().is_empty() {
        return field.required.then(|| "Fill this in to submit".to_owned());
    }
    let length = value.chars().count() as u32;
    if let Some(min) = field.min.filter(|&min| length < min) {
        return Some(format!("Use at least {}", characters(min)));
    }
    if let Some(max) = field.max.filter(|&max| length > max) {
        return Some(format!("Keep it to {}", characters(max)));
    }
    None
}

fn characters(n: u32) -> String {
    format!("{n} {}", if n == 1 { "character" } else { "characters" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(required: bool, min: Option<u32>, max: Option<u32>) -> TextInput {
        TextInput { custom_id: "q".into(), label: "Question".into(), paragraph: false, placeholder: None, value: None, required, min, max }
    }

    #[test]
    fn check_names_each_fields_problem() {
        let fields = [input(true, None, None), input(false, Some(5), None), input(true, Some(2), Some(4)), input(true, Some(1), Some(3))];
        let values = ["  ", "", "abcde", "ab"].map(String::from);
        assert_eq!(check(&fields, &values), vec![Some("Fill this in to submit".into()), None, Some("Keep it to 4 characters".into()), None]);
        assert_eq!(problem(&input(false, Some(5), None), "abc"), Some("Use at least 5 characters".into()));
        assert_eq!(problem(&input(true, Some(1), None), "é"), None);
        assert_eq!(problem(&input(true, None, Some(1)), "éé"), Some("Keep it to 1 character".into()));
    }
}
