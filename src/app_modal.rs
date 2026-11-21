// A form an app asked us to fill in (INTERACTION_MODAL_CREATE), drawn in Waffle's dialog. Behaviour and
// copy follow Discord's client: the warning about who gets the form, its per-field validation, and one
// error line at the top when the app refuses a submit.

use gpui_kit::{
    AnyElement, Context, Entity, FocusHandle, Focusable, FontWeight, HighlightStyle, MouseButton, SharedString, StyledText, Window,
    component::input::{Input, InputEvent, InputState, Textarea, TextareaState},
    div,
    prelude::*,
    px,
};

use crate::{
    Waffle,
    app_commands::{self, Block, Field, FieldValue, Modal, ModalComponent, Origin, SelectKind, SelectOption, TextInput},
    icons::Glyph,
    interactions::InteractionKind,
    palette,
    ui::{self, Check, Tone},
};

const DIALOG_WIDTH: f32 = 460.;
// Not verified: Discord fills a missing bound with the API's defaults in its length message; these are
// the API's (0 and 4000) for a text input.
const DEFAULT_MIN_LENGTH: u32 = 0;
const DEFAULT_MAX_LENGTH: u32 = 4000;

pub(crate) struct AppModal {
    modal: Modal,
    // One per field in `Modal::fields` order: a text input's editor, built on the first render (the
    // gateway event that opens the form has no window); None for other fields.
    editors: Vec<Option<Editor>>,
    built: bool,
    // Each field's value as last set: what non-text fields hold, and what text fields started with.
    values: Vec<FieldValue>,
    // Each field's value as the app sent it: once anything differs, a click outside no longer closes it.
    initial: Vec<FieldValue>,
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
        self.answered_by_modal(modal.nonce.as_deref(), &modal.id, cx);
        self.take_app_modal();
        let initial: Vec<FieldValue> = modal.fields().iter().map(Field::initial).collect();
        let count = initial.len();
        self.app_modal = Some(AppModal { modal, editors: Vec::new(), built: false, values: initial.clone(), initial, problems: vec![None; count], sent: None });
        cx.notify();
    }

    fn build_app_modal_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(modal) = self.app_modal.as_ref().filter(|m| !m.built).map(|m| m.modal.clone()) else { return };
        let editors: Vec<Option<Editor>> = modal
            .fields()
            .iter()
            .map(|f| match f.component {
                ModalComponent::TextInput(input) => Some(editor(input, window, cx)),
                _ => None,
            })
            .collect();
        // Discord focuses the first field.
        if let Some(first) = editors.iter().flatten().next() {
            first.focus(window, cx);
        }
        if let Some(m) = self.app_modal.as_mut() {
            m.editors = editors;
            m.built = true;
        }
    }

    // Once Submit has named a problem, it clears as soon as it's fixed.
    fn on_app_modal_change(&mut self, cx: &mut Context<Self>) {
        let Some(m) = self.app_modal.as_ref() else { return };
        if m.problems.iter().any(Option::is_some) {
            let problems = check(&m.modal.fields(), &app_modal_values(m, cx));
            if let Some(m) = self.app_modal.as_mut() {
                m.problems = problems;
            }
        }
        cx.notify();
    }

    // A pick in a select, radio or checkbox group, or a checkbox flip.
    fn pick_app_modal(&mut self, ix: usize, value: Option<String>, cx: &mut Context<Self>) {
        let Some(m) = self.app_modal.as_mut() else { return };
        let Some(field) = m.modal.fields().get(ix).map(|f| f.component.clone()) else { return };
        let current = m.values.get(ix).cloned().unwrap_or(FieldValue::Unset);
        let next = picked(&field, &current, value);
        if let Some(slot) = m.values.get_mut(ix) {
            *slot = next;
        }
        self.on_app_modal_change(cx);
    }

    fn submit_app_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.app_modal_busy() {
            return;
        }
        let Some(m) = self.app_modal.as_ref() else { return };
        let values = app_modal_values(m, cx);
        let problems = check(&m.modal.fields(), &values);
        if let Some(ix) = problems.iter().position(Option::is_some) {
            if let Some(Some(editor)) = m.editors.get(ix) {
                editor.focus(window, cx);
            }
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
            m.problems = vec![None; m.problems.len()];
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
        self.interactions.get(nonce).map(|p| p.failed())
    }

    fn forget_refused_submit(&mut self) {
        let Some(nonce) = self.app_modal.as_ref().and_then(|m| m.sent.clone()) else { return };
        if self.interactions.get(&nonce).is_some_and(|p| p.failed()) {
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

    // Not verified: whether Discord's modal closes on a click outside (its generic modal layer wasn't
    // checked). Waffle keeps its own rule: only while nothing in it was typed or changed.
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
        let values = app_modal_values(m, cx);
        let mut field_ix = 0;
        let mut blocks: Vec<AnyElement> = Vec::new();
        for block in m.modal.blocks() {
            match block {
                Block::Text(text) => blocks.push(div().text_size(px(14.)).text_color(palette::text(cx)).child(text.to_owned()).into_any_element()),
                Block::Field(field) => {
                    let ix = field_ix;
                    field_ix += 1;
                    let editor = m.editors.get(ix).and_then(Option::as_ref);
                    let value = values.get(ix).cloned().unwrap_or(FieldValue::Unset);
                    blocks.push(field_row(ix, &field, editor, &value, m.problems.get(ix).cloned().flatten(), cx));
                }
            }
        }
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
            .children(refused.then(|| ui::field_error(crate::interactions::MODAL_FAILED, cx).text_size(px(14.))))
            .child(notice(&app.name, cx))
            .children(blocks)
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
            failure: Some(crate::interactions::MODAL_FAILED.to_owned()),
        };
        self.interactions.insert(nonce, pending);
        let Some(m) = self.app_modal.as_ref() else { return };
        let problems = check(&m.modal.fields(), &app_modal_values(m, cx));
        if let Some(Some(editor)) = problems.iter().position(Option::is_some).and_then(|ix| m.editors.get(ix)) {
            editor.focus(window, cx);
        }
        if let Some(m) = self.app_modal.as_mut() {
            m.problems = problems;
        }
    }
}

// Each field's value now: a text field's editor (an untouched empty one stays unset, so it goes out as
// null), else what was picked.
fn app_modal_values(m: &AppModal, cx: &gpui_kit::App) -> Vec<FieldValue> {
    m.values
        .iter()
        .enumerate()
        .map(|(ix, value)| match m.editors.get(ix).and_then(Option::as_ref) {
            Some(editor) => {
                let text = editor.value(cx);
                if text.is_empty() && m.initial.get(ix) == Some(&FieldValue::Unset) { FieldValue::Unset } else { FieldValue::Text(text) }
            }
            None => value.clone(),
        })
        .collect()
}

// A field's editor, prefilled with the app's value and stopped at its max length as Discord's native
// `maxLength` does. Enter in a one-line field submits the form.
fn editor(field: &TextInput, window: &mut Window, cx: &mut Context<Waffle>) -> Editor {
    let placeholder = field.placeholder.clone().unwrap_or_default();
    let value = field.value.clone().unwrap_or_default();
    let max = field.max;
    if field.paragraph {
        let state = cx.new(|cx| TextareaState::new(window, cx).placeholder(placeholder));
        state.update(cx, |s, cx| s.set_value(value, window, cx));
        cx.subscribe_in(&state, window, move |this: &mut Waffle, state, event: &InputEvent, window, cx| {
            if !matches!(event, InputEvent::Change) {
                return;
            }
            // The textarea has no validator: past the max, the end is cut back.
            let text = state.read(cx).value().to_string();
            if let Some(cut) = max.and_then(|max| cut_utf16(&text, max)) {
                state.update(cx, |s, cx| s.set_value(cut, window, cx));
            }
            this.on_app_modal_change(cx);
        })
        .detach();
        return Editor::Paragraph(state);
    }
    let state = cx.new(|cx| {
        let state = InputState::new(window, cx).placeholder(placeholder);
        match max {
            Some(max) => state.validate(move |text, _| utf16_len(text) <= max as usize),
            None => state,
        }
    });
    state.update(cx, |s, cx| s.set_value(value, window, cx));
    cx.subscribe_in(&state, window, |this: &mut Waffle, _, event: &InputEvent, window, cx| match event {
        InputEvent::PressEnter { .. } => this.submit_app_modal(window, cx),
        InputEvent::Change => this.on_app_modal_change(cx),
        _ => {}
    })
    .detach();
    Editor::Line(state)
}

// The app's tile, the form's title, and the way out.
fn head(app: &app_commands::App, title: &str, cx: &mut Context<Waffle>) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_3()
        .child(ui::server_tile(app.icon.as_deref(), &app.name, 24., cx))
        .child(div().flex_1().min_w_0().text_size(px(20.)).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(title.to_owned()))
        .child(ui::icon_button("app-modal-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.close_app_modal(cx))))
}

// Discord's warning, always shown, the app's name in bold.
fn notice(app: &str, cx: &gpui_kit::App) -> impl IntoElement {
    let lead = "This form will be submitted to ";
    let text = format!("{lead}{app}. Do not share passwords or other sensitive information.");
    let bold = HighlightStyle { font_weight: Some(FontWeight::BOLD), ..Default::default() };
    div()
        .flex()
        .items_start()
        .gap(px(8.))
        .p(px(12.))
        .rounded(px(8.))
        .bg(palette::hover(cx))
        .text_size(px(14.))
        .text_color(palette::text(cx))
        .child(div().flex_none().mt(px(2.)).text_color(palette::muted(cx)).child(gpui_kit::component::Icon::new(Glyph::Warning).size(px(16.))))
        .child(div().min_w_0().child(StyledText::new(text).with_highlights(vec![(lead.len()..lead.len() + app.len(), bold)])))
}

fn field_row(ix: usize, field: &Field, editor: Option<&Editor>, value: &FieldValue, problem: Option<String>, cx: &mut Context<Waffle>) -> AnyElement {
    let control = match (field.component, editor) {
        (ModalComponent::TextInput(_), Some(editor)) => text_frame(ix, editor, problem.is_some(), cx),
        (ModalComponent::Select(s), _) if s.kind == SelectKind::String => choices(ix, &s.options, s.max > 1, value, cx),
        (ModalComponent::RadioGroup(g), _) => choices(ix, &g.options, false, value, cx),
        (ModalComponent::CheckboxGroup(g), _) => choices(ix, &g.options, true, value, cx),
        (ModalComponent::Checkbox { .. }, _) => checkbox(ix, *value == FieldValue::Checked(true), cx),
        // GAP: user, role, channel and mentionable selects and file uploads need pickers Waffle's form
        // doesn't have yet; they go out unset.
        _ => div().text_size(px(14.)).text_color(palette::muted(cx)).child("Waffle can't fill in this field yet.").into_any_element(),
    };
    ui::field_marked(field.label.to_owned(), field.required(), cx)
        .children(field.description.map(|d| div().text_size(px(12.)).text_color(palette::muted(cx)).child(d.to_owned())))
        .child(control)
        .children(problem.map(|p| ui::field_error(p, cx)))
        .into_any_element()
}

fn text_frame(ix: usize, editor: &Editor, error: bool, cx: &gpui_kit::App) -> AnyElement {
    let id = SharedString::from(format!("app-modal-field-{ix}"));
    let focus = editor.focus_handle(cx);
    match editor {
        Editor::Line(s) => ui::text_field(id, Input::new(s).appearance(false), &focus).error(error).into_any_element(),
        Editor::Paragraph(s) => ui::text_area(id, Textarea::new(s).appearance(false).px_0().text_size(px(16.)), &focus).error(error).into_any_element(),
    }
}

// A select, radio or checkbox group's options as rows: a tick for several, a radio for one.
fn choices(ix: usize, options: &[SelectOption], several: bool, value: &FieldValue, cx: &mut Context<Waffle>) -> AnyElement {
    let picked: &[String] = match value {
        FieldValue::Picks(p) => p,
        _ => &[],
    };
    let rows = options.iter().enumerate().map(|(i, o)| {
        let on = picked.contains(&o.value);
        let mark = if several { ui::check(if on { Check::On } else { Check::Off }, cx) } else { ui::radio(on, cx) };
        let pick = o.value.clone();
        div()
            .id(SharedString::from(format!("app-modal-choice-{ix}-{i}")))
            .flex()
            .items_start()
            .gap(px(10.))
            .py(px(6.))
            .cursor_pointer()
            .child(mark)
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().text_size(px(14.)).text_color(palette::strong(cx)).child(o.label.clone()))
                    .children(o.description.clone().map(|d| div().text_size(px(12.)).text_color(palette::muted(cx)).child(d))),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.pick_app_modal(ix, Some(pick.clone()), cx)))
    });
    div().flex().flex_col().children(rows).into_any_element()
}

fn checkbox(ix: usize, on: bool, cx: &mut Context<Waffle>) -> AnyElement {
    div()
        .id(SharedString::from(format!("app-modal-check-{ix}")))
        .cursor_pointer()
        .child(ui::check(if on { Check::On } else { Check::Off }, cx))
        .on_click(cx.listener(move |this, _, _, cx| this.pick_app_modal(ix, None, cx)))
        .into_any_element()
}

// The value after a click on `option` (None: a checkbox flip). A single pick replaces, a multi pick
// toggles and stops at the field's max as Discord's disables the rest.
fn picked(field: &ModalComponent, current: &FieldValue, option: Option<String>) -> FieldValue {
    let picks = match current {
        FieldValue::Picks(p) => p.clone(),
        _ => Vec::new(),
    };
    let max = match field {
        ModalComponent::Select(s) => Some(s.max),
        ModalComponent::CheckboxGroup(g) => g.max,
        ModalComponent::RadioGroup(_) => Some(1),
        ModalComponent::Checkbox { .. } => return FieldValue::Checked(*current != FieldValue::Checked(true)),
        _ => return current.clone(),
    };
    let Some(option) = option else { return current.clone() };
    let next = if picks.contains(&option) {
        picks.into_iter().filter(|p| *p != option).collect()
    } else if max == Some(1) {
        vec![option]
    } else if max.is_some_and(|max| picks.len() >= max as usize) {
        picks
    } else {
        [picks, vec![option]].concat()
    };
    // A radio stays picked once picked.
    if next.is_empty() && matches!(field, ModalComponent::RadioGroup(_)) {
        return current.clone();
    }
    FieldValue::Picks(next)
}

/// What stops each field from submitting, in field order (Discord's validators).
fn check(fields: &[Field], values: &[FieldValue]) -> Vec<Option<String>> {
    fields.iter().zip(values).map(|(f, v)| problem(f, v)).collect()
}

const REQUIRED: &str = "This field is required.";

// Discord's rules: empty means no characters at all (whitespace counts), lengths are UTF-16 units, and
// an empty optional field passes whatever its bounds.
fn problem(field: &Field, value: &FieldValue) -> Option<String> {
    let required = field.required();
    let picks = match value {
        FieldValue::Picks(p) => p.len() as u32,
        _ => 0,
    };
    let count_problem = |min: Option<u32>, max: Option<u32>| {
        if picks == 0 {
            return required.then(|| REQUIRED.to_owned());
        }
        if let Some(min) = min.filter(|&min| picks < min) {
            return Some(format!("Select at least {min}"));
        }
        max.filter(|&max| picks > max).map(|max| format!("Select at most {max}"))
    };
    match field.component {
        ModalComponent::TextInput(t) => {
            let text = match value {
                FieldValue::Text(t) => t.as_str(),
                _ => "",
            };
            if text.is_empty() {
                return required.then(|| REQUIRED.to_owned());
            }
            let length = utf16_len(text) as u32;
            let outside = t.min.is_some_and(|min| length < min) || t.max.is_some_and(|max| length > max);
            outside.then(|| format!("Must be between {} and {} in length.", t.min.unwrap_or(DEFAULT_MIN_LENGTH), t.max.unwrap_or(DEFAULT_MAX_LENGTH)))
        }
        ModalComponent::Select(s) => count_problem(Some(s.min), Some(s.max)),
        ModalComponent::CheckboxGroup(g) => count_problem(g.min, g.max),
        ModalComponent::RadioGroup(_) | ModalComponent::FileUpload(_) => (picks == 0 && required).then(|| REQUIRED.to_owned()),
        _ => None,
    }
}

// JavaScript's `.length`: UTF-16 code units, which Discord counts.
fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

// `text` cut to `max` UTF-16 units (never inside a character), or None when it fits.
fn cut_utf16(text: &str, max: u32) -> Option<String> {
    let mut units = 0;
    for (at, c) in text.char_indices() {
        units += c.len_utf16();
        if units > max as usize {
            return Some(text[..at].to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_commands::{ChoiceGroup, Select};

    fn input(required: bool, min: Option<u32>, max: Option<u32>) -> ModalComponent {
        ModalComponent::TextInput(TextInput { custom_id: "q".into(), label: "Question".into(), required, min, max, ..Default::default() })
    }

    fn field(component: &ModalComponent) -> Field<'_> {
        Field { label: "Question", description: None, component }
    }

    fn text(t: &str) -> FieldValue {
        FieldValue::Text(t.into())
    }

    #[test]
    fn text_fields_check_as_discord_does() {
        let required = input(true, None, None);
        assert_eq!(problem(&field(&required), &FieldValue::Unset), Some(REQUIRED.into()));
        assert_eq!(problem(&field(&required), &text("  ")), None, "whitespace counts as filled");
        let optional = input(false, Some(5), None);
        assert_eq!(problem(&field(&optional), &text("")), None);
        assert_eq!(problem(&field(&optional), &text("abc")), Some("Must be between 5 and 4000 in length.".into()));
        let bounded = input(true, Some(2), Some(4));
        assert_eq!(problem(&field(&bounded), &text("abcde")), Some("Must be between 2 and 4 in length.".into()));
        assert_eq!(problem(&field(&bounded), &text("ab")), None);
        // One emoji is two UTF-16 units.
        assert_eq!(problem(&field(&input(true, None, Some(1))), &text("😀")), Some("Must be between 0 and 1 in length.".into()));
        assert_eq!(problem(&field(&input(true, None, Some(2))), &text("😀")), None);
    }

    #[test]
    fn pick_fields_check_counts() {
        let select = ModalComponent::Select(Select {
            kind: SelectKind::String,
            custom_id: "s".into(),
            placeholder: None,
            options: Vec::new(),
            min: 2,
            max: 3,
            disabled: false,
            required: false,
            default_values: Vec::new(),
            channel_types: Vec::new(),
        });
        assert_eq!(problem(&field(&select), &FieldValue::Unset), None, "optional unless required");
        assert_eq!(problem(&field(&select), &FieldValue::Picks(vec!["a".into()])), Some("Select at least 2".into()));
        let radio = ModalComponent::RadioGroup(ChoiceGroup { required: true, ..Default::default() });
        assert_eq!(problem(&field(&radio), &FieldValue::Unset), Some(REQUIRED.into()));
    }

    #[test]
    fn picks_toggle_and_stop_at_max() {
        let group = ModalComponent::CheckboxGroup(ChoiceGroup { max: Some(2), ..Default::default() });
        let two = FieldValue::Picks(vec!["a".into(), "b".into()]);
        assert_eq!(picked(&group, &FieldValue::Unset, Some("a".into())), FieldValue::Picks(vec!["a".into()]));
        assert_eq!(picked(&group, &two, Some("c".into())), two, "full: the rest are disabled");
        assert_eq!(picked(&group, &two, Some("a".into())), FieldValue::Picks(vec!["b".into()]));
        let radio = ModalComponent::RadioGroup(ChoiceGroup::default());
        assert_eq!(picked(&radio, &FieldValue::Picks(vec!["a".into()]), Some("b".into())), FieldValue::Picks(vec!["b".into()]));
        assert_eq!(picked(&radio, &FieldValue::Picks(vec!["a".into()]), Some("a".into())), FieldValue::Picks(vec!["a".into()]));
        let check = ModalComponent::Checkbox { custom_id: "c".into(), default: false };
        assert_eq!(picked(&check, &FieldValue::Checked(false), None), FieldValue::Checked(true));
    }

    #[test]
    fn cut_utf16_stops_at_whole_characters() {
        assert_eq!(cut_utf16("abc", 3), None);
        assert_eq!(cut_utf16("abcd", 3).as_deref(), Some("abc"));
        assert_eq!(cut_utf16("a😀b", 2).as_deref(), Some("a"));
    }
}
