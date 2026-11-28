// A form an app asked us to fill in (INTERACTION_MODAL_CREATE), drawn in Waffle's dialog. Behaviour and
// copy follow Discord's client: the warning about who gets the form, its per-field validation, selects
// that open the same menus as a message's, file fields staged on Submit, and one error line at the top
// when the app refuses a submit.

use std::{collections::HashMap, path::PathBuf};

use gpui_kit::{
    AnyElement, Context, Entity, FocusHandle, Focusable, FontWeight, HighlightStyle, MouseButton, PathPromptOptions, SharedString, StyledText, Window,
    component::input::{Input, InputEvent, InputState, Textarea, TextareaState},
    div,
    prelude::*,
};

use crate::{
    Waffle,
    app_commands::{self, Block, Field, FieldValue, FileUpload, Modal, ModalComponent, Origin, SelectKind, SelectOption, TextInput},
    attachments::PendingFile,
    icons::Glyph,
    interactions::{InteractionKind, MODAL_FAILED, PendingInteraction},
    palette,
    ui::{self, Check},
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
    // Each file field's picked files, by field index; Submit stages them before it sends.
    files: HashMap<usize, Vec<PendingFile>>,
    // Submit is uploading the files; the interaction goes out once they're up.
    staging: bool,
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
        let initial: Vec<FieldValue> = modal.fields().iter().map(|f| self.field_initial(&modal.channel_id, f)).collect();
        let count = initial.len();
        self.app_modal = Some(AppModal { modal, editors: Vec::new(), built: false, values: initial.clone(), initial, problems: vec![None; count], sent: None, files: HashMap::new(), staging: false });
        cx.notify(); // lint-allow(global-notify): the app modal is drawn by the Overlays view, which re-renders only with the app
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
        cx.notify(); // lint-allow(global-notify): the app modal is drawn by the Overlays view, which re-renders only with the app
    }

    // A field's starting value; a user, role, channel or mentionable select starts at its `default_values`
    // that resolve here, as Discord's does.
    fn field_initial(&self, channel_id: &str, field: &Field) -> FieldValue {
        match field.component {
            ModalComponent::Select(s) if s.kind != SelectKind::String => {
                let picks = self.modal_select_defaults(channel_id, s);
                if picks.is_empty() { FieldValue::Unset } else { FieldValue::Picks(picks) }
            }
            _ => field.initial(),
        }
    }

    pub(crate) fn app_modal_channel(&self) -> Option<&str> {
        self.app_modal.as_ref().map(|m| m.modal.channel_id.as_str())
    }

    // A select field's picks, for its menu.
    pub(crate) fn app_modal_picks(&self, ix: usize) -> Vec<String> {
        match self.app_modal.as_ref().and_then(|m| m.values.get(ix)) {
            Some(FieldValue::Picks(p)) => p.clone(),
            _ => Vec::new(),
        }
    }

    // A pick in a select, radio or checkbox group, or a checkbox flip.
    pub(crate) fn pick_app_modal(&mut self, ix: usize, value: Option<String>, cx: &mut Context<Self>) {
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
            return cx.notify(); // lint-allow(global-notify): the app modal is drawn by the Overlays view, which re-renders only with the app
        }
        // Discord uploads the form's files first, then sends the submit naming them.
        let uploads: Vec<_> = (0..values.len()).flat_map(|ix| m.files.get(&ix).into_iter().flatten()).map(PendingFile::upload).collect();
        let (modal, channel_id, nonce) = (m.modal.clone(), m.modal.channel_id.clone(), crate::nonce());
        self.forget_refused_submit();
        if let Some(m) = self.app_modal.as_mut() {
            m.problems = vec![None; m.problems.len()];
            m.sent = Some(nonce.clone());
            m.staging = !uploads.is_empty();
        }
        if uploads.is_empty() {
            return self.send_app_modal(&modal, &values, &[], nonce, cx);
        }
        let Some(client) = self.client.clone() else { return };
        // GAP: Discord gives staging 2 minutes before it fails the submit; Waffle waits for the uploads.
        cx.spawn(async move |this, cx| {
            let staged = client.stage_files(&channel_id, &uploads).await;
            this.update(cx, |this, cx| {
                let Some(m) = this.app_modal.as_mut().filter(|m| m.sent.as_ref() == Some(&nonce)) else { return };
                m.staging = false;
                match staged {
                    Ok(staged) => this.send_app_modal(&modal, &values, &staged, nonce, cx),
                    Err(_) => {
                        let failed = PendingInteraction { channel_id, app: modal.application.clone(), id: None, kind: InteractionKind::Modal, failure: Some(MODAL_FAILED.to_owned()) };
                        this.interactions.insert(nonce, failed);
                        cx.notify(); // lint-allow(global-notify): the app modal is drawn by the Overlays view, which re-renders only with the app
                    }
                }
            })
            .ok();
        })
        .detach(); // lint-allow(detached-task): ends itself once its upload or file prompt resolves, and holds only a weak handle
        cx.notify(); // lint-allow(global-notify): the app modal is drawn by the Overlays view, which re-renders only with the app
    }

    fn send_app_modal(&mut self, modal: &Modal, values: &[FieldValue], uploads: &[app_commands::CommandAttachment], nonce: String, cx: &mut Context<Self>) {
        let channel_id = modal.channel_id.as_str();
        let guild_id = self.guilds.iter().find(|g| g.channels.iter().any(|c| c.id == channel_id)).map(|g| g.id.clone());
        let origin = Origin { channel_id, guild_id: guild_id.as_deref(), session_id: &self.gateway_session, nonce: &nonce };
        let body = app_commands::modal_body(modal, values, uploads, &origin);
        self.interact(nonce, modal.application.clone(), InteractionKind::Modal, body, cx);
    }

    // Browse: files join the field up to its max, within your upload limit.
    fn browse_app_modal_files(&mut self, ix: usize, max: usize, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: max > 1, prompt: None });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            this.update(cx, |this, cx| this.add_app_modal_files(ix, max, paths, cx)).ok();
        })
        .detach(); // lint-allow(detached-task): ends itself once its upload or file prompt resolves, and holds only a weak handle
    }

    fn add_app_modal_files(&mut self, ix: usize, max: usize, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let limit = self.app_modal_upload_limit();
        let Some(m) = self.app_modal.as_mut() else { return };
        let files = m.files.entry(ix).or_default();
        let refused = crate::attachments::admit(files, paths.into_iter().filter_map(PendingFile::new).collect(), limit);
        files.truncate(max);
        sync_file_value(m, ix);
        self.on_app_modal_change(cx);
        // After the recheck, which would clear it: why a file was left out.
        if let Some(slot) = refused.and_then(|why| Some((self.app_modal.as_mut()?.problems.get_mut(ix)?, why))) {
            *slot.0 = Some(slot.1);
        }
    }

    // Your per-file upload limit in the form's channel.
    fn app_modal_upload_limit(&self) -> u64 {
        let guild_tier = self.app_modal_channel().and_then(|c| self.guilds.iter().find(|g| g.channels.iter().any(|ch| ch.id == c))).map_or(0, |g| g.premium_tier);
        crate::discord::upload_limit(self.my_account.premium_type, guild_tier)
    }

    fn remove_app_modal_file(&mut self, ix: usize, file: usize, cx: &mut Context<Self>) {
        let Some(m) = self.app_modal.as_mut() else { return };
        if let Some(files) = m.files.get_mut(&ix).filter(|f| file < f.len()) {
            files.remove(file);
        }
        sync_file_value(m, ix);
        self.on_app_modal_change(cx);
    }

    fn app_modal_busy(&self) -> bool {
        self.app_modal.as_ref().is_some_and(|m| m.staging) || self.app_modal_submit().is_some_and(|failed| !failed)
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
        cx.notify(); // lint-allow(global-notify): the app modal is drawn by the Overlays view, which re-renders only with the app
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
        if m.sent.is_some() && !m.staging && self.app_modal_submit().is_none() {
            self.app_modal = None;
            return None;
        }
        let submit = self.app_modal_submit();
        let busy = self.app_modal_busy();
        let refused = submit == Some(true);
        let app = &m.modal.application;
        let values = app_modal_values(m, cx);
        let mut field_ix = 0;
        let mut blocks: Vec<AnyElement> = Vec::new();
        for block in m.modal.blocks() {
            match block {
                Block::Text(text) => blocks.push(div().text_size(ui::TEXT_SM).text_color(palette::text(cx)).child(text.to_owned()).into_any_element()),
                Block::Field(field) => {
                    let ix = field_ix;
                    field_ix += 1;
                    let editor = m.editors.get(ix).and_then(Option::as_ref);
                    let value = values.get(ix).cloned().unwrap_or(FieldValue::Unset);
                    let control = match field.component {
                        ModalComponent::Select(s) => Some(self.modal_select(ix, s, &picks_of(&value), cx)),
                        ModalComponent::FileUpload(f) => Some(self.file_field(ix, f, m.files.get(&ix).map_or(&[], Vec::as_slice), cx)),
                        _ => None,
                    };
                    blocks.push(field_row(ix, &field, editor, control, &value, m.problems.get(ix).cloned().flatten(), cx));
                }
            }
        }
        let footer = ui::row().items_stretch()
            .justify_end()
            .gap_2()
            .child(ui::Button::new("app-modal-cancel").variant(ui::ButtonVariant::Secondary).child("Cancel").on_click(cx.listener(|this, _, _, cx| this.close_app_modal(cx))))
            .child(
                ui::Button::new("app-modal-submit")
                    .loading(busy.then_some("Submitting…"))
                    .child("Submit")
                    .on_click(cx.listener(|this, _, window, cx| this.submit_app_modal(window, cx))),
            );
        let body = ui::DialogContent::new()
            .child(head(app, &m.modal.title, cx))
            .children(refused.then(|| ui::FieldError::new(crate::interactions::MODAL_FAILED, cx).text_size(ui::TEXT_SM)))
            .child(notice(&app.name, cx))
            .children(blocks)
            .child(footer);
        let backdrop = div().id("app-modal-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.dismiss_app_modal(cx)));
        Some(ui::Dialog::modal(backdrop, ui::Dialog::new("app-modal", DIALOG_WIDTH, body, window, cx), cx))
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
        .detach(); // lint-allow(detached-task): the subscription ends when the editor entity, held in AppModal, drops
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
    .detach(); // lint-allow(detached-task): the subscription ends when the editor entity, held in AppModal, drops
    Editor::Line(state)
}

// The app's tile, the form's title, and the way out.
fn head(app: &app_commands::App, title: &str, cx: &mut Context<Waffle>) -> impl IntoElement {
    ui::row()
        .gap_3()
        .child(ui::server_tile(app.icon.as_deref(), &app.name, 24., cx))
        .child(div().flex_1().min_w_0().text_size(ui::TEXT_LG).line_height(ui::LINE_LG).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(title.to_owned()))
        .child(ui::Button::new("app-modal-close").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).icon(Glyph::X).tooltip_text("Close").on_click(cx.listener(|this, _, _, cx| this.close_app_modal(cx))))
}

// Discord's warning, always shown, the app's name in bold.
fn notice(app: &str, cx: &gpui_kit::App) -> impl IntoElement {
    let lead = "This form will be submitted to ";
    let text = format!("{lead}{app}. Do not share passwords or other sensitive information.");
    let bold = HighlightStyle { font_weight: Some(FontWeight::BOLD), ..Default::default() };
    ui::row()
        .items_start()
        .gap_2()
        .p_3()
        .rounded_lg()
        .bg(palette::hover(cx))
        .text_size(ui::TEXT_SM)
        .text_color(palette::text(cx))
        .child(div().flex_none().pt_0p5().text_color(palette::muted(cx)).child(gpui_kit::component::Icon::new(Glyph::Warning).size(ui::ICON_MD)))
        .child(div().min_w_0().child(StyledText::new(text).with_highlights(vec![(lead.len()..lead.len() + app.len(), bold)])))
}

// `control` is a select's or file field's, drawn by the caller, which knows the server and the files.
fn field_row(ix: usize, field: &Field, editor: Option<&Editor>, control: Option<AnyElement>, value: &FieldValue, problem: Option<String>, cx: &mut Context<Waffle>) -> AnyElement {
    let control = match (field.component, editor, control) {
        (_, _, Some(control)) => control,
        (ModalComponent::TextInput(_), Some(editor), _) => text_frame(ix, editor, problem.is_some(), cx),
        (ModalComponent::RadioGroup(g), _, _) => choices(ix, &g.options, false, value, cx),
        (ModalComponent::CheckboxGroup(g), _, _) => choices(ix, &g.options, true, value, cx),
        (ModalComponent::Checkbox { .. }, _, _) => checkbox(ix, *value == FieldValue::Checked(true), cx),
        _ => div().into_any_element(),
    };
    ui::Field::marked(field.label.to_owned(), field.required(), cx)
        .children(field.description.map(|d| div().text_size(ui::TEXT_XS).text_color(palette::muted(cx)).child(d.to_owned())))
        .child(control)
        .children(problem.map(|p| ui::FieldError::new(p, cx)))
        .into_any_element()
}

fn text_frame(ix: usize, editor: &Editor, error: bool, cx: &gpui_kit::App) -> AnyElement {
    let id = SharedString::from(format!("app-modal-field-{ix}"));
    let focus = editor.focus_handle(cx);
    match editor {
        Editor::Line(s) => ui::Input::new(id, Input::new(s).appearance(false), &focus).error(error).into_any_element(),
        Editor::Paragraph(s) => ui::Textarea::new(id, Textarea::new(s).appearance(false).px_0().text_size(ui::TEXT_MD), &focus).error(error).into_any_element(),
    }
}

fn picks_of(value: &FieldValue) -> Vec<String> {
    match value {
        FieldValue::Picks(p) => p.clone(),
        _ => Vec::new(),
    }
}

// A file field's value: the files' paths, or unset with none (sent as null).
fn sync_file_value(m: &mut AppModal, ix: usize) {
    let paths: Vec<String> = m.files.get(&ix).into_iter().flatten().map(|f| f.path.to_string_lossy().into_owned()).collect();
    if let Some(slot) = m.values.get_mut(ix) {
        *slot = if paths.is_empty() { FieldValue::Unset } else { FieldValue::Picks(paths) };
    }
}

impl Waffle {
    // Discord's file upload field: one file at a max of one shows as its row; otherwise the drop zone (or,
    // full, "File upload limit reached") over the picked files, each removable.
    // GAP: no dropping files onto it; browse only.
    fn file_field(&self, ix: usize, field: &FileUpload, files: &[PendingFile], cx: &mut Context<Self>) -> AnyElement {
        // Not verified: Discord's default when the app sends no `max_values`.
        let max = field.max.unwrap_or(1).max(1) as usize;
        let rows: Vec<AnyElement> = files.iter().enumerate().map(|(i, f)| file_row(ix, i, f, cx)).collect();
        if max == 1 && files.len() == 1 {
            return ui::col().children(rows).into_any_element();
        }
        let size = crate::chat::format_size(self.app_modal_upload_limit());
        let zone = if files.len() >= max {
            drop_zone(Glyph::CheckCircle, div().text_color(palette::strong(cx)).child("File upload limit reached"), "Remove some files to upload new ones.".to_owned(), cx)
        } else {
            let browse = ui::Button::new(SharedString::from(format!("app-modal-browse-{ix}"))).variant(ui::ButtonVariant::Link)
                .child("browse")
                .on_click(cx.listener(move |this, _, window, cx| this.browse_app_modal_files(ix, max, window, cx)));
            let lead = ui::row().flex_wrap().gap_1().child(drop_lead(max)).child(browse);
            drop_zone(Glyph::Upload, lead, upload_hint(field.min.unwrap_or(1), max as u32, &size), cx)
        };
        ui::col().gap_2().child(zone).children(rows).into_any_element()
    }
}

// Discord's "Drop {maxValues} here or browse".
// Not verified: the plural's words; the message table holds it flattened.
fn drop_lead(max: usize) -> String {
    if max == 1 { "Drop a file here or".to_owned() } else { format!("Drop up to {max} files here or") }
}

// Discord's line under the drop zone (`fw`, no file types).
// Not verified: the words of "{maxValues}" (a plural in Discord's table).
fn upload_hint(min: u32, max: u32, size: &str) -> String {
    if min > 1 {
        return if min == max { format!("Upload {min} files under {size}.") } else { format!("Upload {min}–{max} files under {size}.") };
    }
    let count = if max == 1 { "a file".to_owned() } else { format!("up to {max} files") };
    format!("Upload {count} under {size}.")
}

fn drop_zone(glyph: Glyph, lead: gpui_kit::Div, hint: String, cx: &gpui_kit::App) -> AnyElement {
    ui::col()
        .items_center()
        .gap_1()
        .p_4()
        .rounded_lg()
        .border_1()
        .border_dashed()
        .border_color(palette::line(cx))
        .text_size(ui::TEXT_SM)
        .text_color(palette::muted(cx))
        .child(div().text_color(palette::strong(cx)).child(gpui_kit::component::Icon::new(glyph).size(ui::ICON_XL)))
        .child(lead)
        .child(div().text_size(ui::TEXT_XS).child(hint))
        .into_any_element()
}

// A picked file: its name and Discord's Remove.
fn file_row(ix: usize, i: usize, file: &PendingFile, cx: &mut Context<Waffle>) -> AnyElement {
    ui::row()
        .gap_2()
        .py_1()
        .child(div().flex_none().text_color(palette::muted(cx)).child(gpui_kit::component::Icon::new(Glyph::File).size(ui::ICON_LG)))
        .child(ui::line(file.name.clone()).flex_1().text_size(ui::TEXT_SM).text_color(palette::strong(cx)))
        .child(ui::Button::new(SharedString::from(format!("app-modal-file-{ix}-{i}"))).variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconXs).icon(Glyph::Trash).tooltip_text("Remove").on_click(cx.listener(move |this, _, _, cx| this.remove_app_modal_file(ix, i, cx))))
        .into_any_element()
}

// A select, radio or checkbox group's options as rows: a tick for several, a radio for one.
fn choices(ix: usize, options: &[SelectOption], several: bool, value: &FieldValue, cx: &mut Context<Waffle>) -> AnyElement {
    let picked: &[String] = match value {
        FieldValue::Picks(p) => p,
        _ => &[],
    };
    let rows = options.iter().enumerate().map(|(i, o)| {
        let on = picked.contains(&o.value);
        let mark = if several { ui::Checkbox::mark(if on { Check::On } else { Check::Off }).into_any_element() } else { ui::RadioGroupItem::mark(on).into_any_element() };
        let pick = o.value.clone();
        ui::row()
            .id(SharedString::from(format!("app-modal-choice-{ix}-{i}")))
            .items_start()
            .gap_2p5()
            .py_1p5()
            .cursor_pointer()
            .child(mark)
            .child(
                ui::col()
                    .child(div().text_size(ui::TEXT_SM).text_color(palette::strong(cx)).child(o.label.clone()))
                    .children(o.description.clone().map(|d| div().text_size(ui::TEXT_XS).text_color(palette::muted(cx)).child(d))),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.pick_app_modal(ix, Some(pick.clone()), cx)))
    });
    ui::col().children(rows).into_any_element()
}

fn checkbox(ix: usize, on: bool, cx: &mut Context<Waffle>) -> AnyElement {
    div()
        .id(SharedString::from(format!("app-modal-check-{ix}")))
        .cursor_pointer()
        .child(ui::Checkbox::mark(if on { Check::On } else { Check::Off }))
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
    fn file_fields_word_their_limits_as_discord_does() {
        assert_eq!(upload_hint(1, 1, "10.0 MB"), "Upload a file under 10.0 MB.");
        assert_eq!(upload_hint(1, 3, "10.0 MB"), "Upload up to 3 files under 10.0 MB.");
        assert_eq!(upload_hint(2, 2, "10.0 MB"), "Upload 2 files under 10.0 MB.");
        assert_eq!(upload_hint(2, 4, "10.0 MB"), "Upload 2–4 files under 10.0 MB.");
        assert_eq!(drop_lead(1), "Drop a file here or");
    }

    #[test]
    fn cut_utf16_stops_at_whole_characters() {
        assert_eq!(cut_utf16("abc", 3), None);
        assert_eq!(cut_utf16("abcd", 3).as_deref(), Some("abc"));
        assert_eq!(cut_utf16("a😀b", 2).as_deref(), Some("a"));
    }
}
