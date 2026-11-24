// Modify Attachment, Discord's dialog for one file: its name, its alt text (pictures and videos only) and
// whether it's a spoiler. Opened from a waiting file's pencil, or from a sent picture's on your own
// message, where the name is locked and Save edits the message's attachments.
use std::path::PathBuf;

use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement, StatefulInteractiveElement, Styled, StyledImage, Window,
    component::{
        Icon,
        input::{Input, InputEvent, Textarea, TextareaState},
    },
    div,
    prelude::FluentBuilder as _,
};

use crate::{
    Waffle,
    attachments::FileKind,
    discord::{Message, MediaKind},
    icons::Glyph,
    palette,
    ui::{self, Check, RADIUS_LG, Squircle as _, Tone},
};

// Discord's textarea limit; it shows no counter or error, it just stops.
const DESCRIPTION_MAX: usize = 1024;
// A name starting with this is a spoiler however the box is set (Discord's `MO`).
const SPOILER_PREFIX: &str = "SPOILER_";

pub(crate) struct ModifyAttachment {
    target: Target,
    kind: FileKind,
    spoiler: bool,
}

enum Target {
    // A file in the composer's tray, by path.
    Pending(PathBuf),
    // The `index`th media item of a message you sent.
    Sent { message: Box<Message>, index: usize },
}

impl Waffle {
    pub(crate) fn open_modify_pending(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(file) = self.pending_files.get(index).cloned() else { return };
        self.fill_modify_inputs(&file.name, file.description.as_deref(), false, window, cx);
        self.modify_attachment = Some(ModifyAttachment { target: Target::Pending(file.path.clone()), kind: file.kind(), spoiler: file.spoiler });
        cx.notify(); // lint-allow(global-notify): the attachment dialog is drawn by the root's overlays, over every column
    }

    pub(crate) fn open_modify_sent(&mut self, message: Message, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = message.media.get(index) else { return };
        let kind = match item.kind {
            MediaKind::Video { .. } => FileKind::Video,
            _ => FileKind::Image,
        };
        let spoiler = item.spoiler;
        self.fill_modify_inputs(&file_name(&item.url), item.description.as_deref(), true, window, cx);
        self.modify_attachment = Some(ModifyAttachment { target: Target::Sent { message: Box::new(message), index }, kind, spoiler });
        cx.notify(); // lint-allow(global-notify): the attachment dialog is drawn by the root's overlays, over every column
    }

    // The filename goes in the shared dialog field; the description gets focus once the name is locked.
    fn fill_modify_inputs(&mut self, name: &str, description: Option<&str>, sent: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        self.alt_popout = None;
        self.dialog_input.update(cx, |state, cx| {
            state.set_placeholder("", window, cx);
            state.set_value(name.to_owned(), window, cx);
            if !sent {
                state.focus(window, cx);
            }
        });
        self.alt_input.update(cx, |state, cx| {
            state.set_value(description.unwrap_or_default().to_owned(), window, cx);
            if sent {
                state.focus(window, cx);
            }
        });
    }

    pub(crate) fn on_alt_input(&mut self, input: &gpui_kit::Entity<TextareaState>, event: &InputEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        let text = input.read(cx).value();
        if text.chars().count() > DESCRIPTION_MAX {
            let kept: String = text.chars().take(DESCRIPTION_MAX).collect();
            input.update(cx, |state, cx| state.set_value(kept, window, cx));
        }
    }

    fn close_modify_attachment(&mut self, cx: &mut Context<Self>) {
        self.modify_attachment = None;
        cx.notify(); // lint-allow(global-notify): the attachment dialog is drawn by the root's overlays, over every column
    }

    pub(crate) fn save_modify_attachment(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.modify_attachment.take() else { return };
        let name = self.dialog_input.read(cx).value().trim().to_owned();
        let spoiler = form.spoiler || name.starts_with(SPOILER_PREFIX);
        // Once saved, a described kind carries "" rather than nothing, as Discord's does.
        let description = form.kind.describable().then(|| self.alt_input.read(cx).value().to_string());
        match form.target {
            Target::Pending(path) => {
                if let Some(file) = self.pending_files.iter_mut().find(|f| f.path == path) {
                    if !name.is_empty() {
                        file.name = name;
                    }
                    file.description = description;
                    file.spoiler = spoiler;
                }
                cx.notify(); // lint-allow(global-notify): the attachment dialog is drawn by the root's overlays, over every column
            }
            Target::Sent { message, index } => self.patch_sent_attachment(*message, index, description.unwrap_or_default(), spoiler, cx),
        }
    }

    // Shown at once; put back with a toast if Discord refuses.
    fn patch_sent_attachment(&mut self, message: Message, index: usize, description: String, spoiler: bool, cx: &mut Context<Self>) {
        let Some(edited) = message.media.get(index).and_then(|m| m.id.clone()) else { return };
        let ids: Vec<String> = message.media.iter().filter_map(|m| m.id.clone()).chain(message.files.iter().map(|f| f.id.clone())).collect();
        let body = crate::discord::attachments_patch(&ids, &edited, &description, spoiler);
        let path = format!("/channels/{}/messages/{}", message.channel_id, message.id);
        let (channel_id, id) = (message.channel_id.clone(), message.id.clone());
        let before = message.media[index].clone();
        self.rest_optimistic(
            crate::discord::Method::PATCH,
            path,
            Some(body),
            |this| {
                if let Some(item) = this.message_mut(&channel_id, &id).and_then(|m| m.media.get_mut(index)) {
                    item.description = Some(description).filter(|d| !d.is_empty());
                    item.spoiler = spoiler;
                }
            },
            move |this| {
                if let Some(item) = this.message_mut(&message.channel_id, &message.id).and_then(|m| m.media.get_mut(index)) {
                    *item = before;
                }
            },
            cx,
        );
    }

    pub(crate) fn render_modify_attachment(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.modify_attachment.as_ref()?;
        let sent = matches!(form.target, Target::Sent { .. });
        let forced = self.dialog_input.read(cx).value().starts_with(SPOILER_PREFIX);
        let strong = palette::strong(cx);
        let head = ui::row()
            .child(div().flex_1().text_size(ui::TEXT_LG).line_height(ui::LINE_LG).font_weight(gpui_kit::FontWeight::SEMIBOLD).text_color(strong).child("Modify Attachment"))
            .child(ui::icon_button("modify-attachment-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.close_modify_attachment(cx))));
        let name_focus = gpui_kit::Focusable::focus_handle(self.dialog_input.read(cx), cx);
        let name = ui::text_field("modify-attachment-name", Input::new(&self.dialog_input).appearance(false).disabled(sent), &name_focus).disabled(sent);
        let description = form.kind.describable().then(|| {
            let focus = gpui_kit::Focusable::focus_handle(self.alt_input.read(cx), cx);
            ui::field("Description (Alt Text)", cx).child(ui::text_area("modify-attachment-alt", Textarea::new(&self.alt_input).appearance(false).px_0().text_size(ui::TEXT_SM), &focus))
        });
        let checked = if form.spoiler || forced { Check::On } else { Check::Off };
        let spoiler = ui::row()
            .id("modify-attachment-spoiler")
            .gap_2p5()
            .text_color(if forced { palette::muted(cx) } else { strong })
            .child(ui::check_live("modify-attachment-spoiler-box", checked, window, cx))
            .child("Mark as spoiler")
            .when(forced, |row| row.opacity(0.6))
            .when(!forced, |row| {
                row.cursor_pointer().on_click(cx.listener(|this, _, _, cx| {
                    if let Some(form) = this.modify_attachment.as_mut() {
                        form.spoiler = !form.spoiler;
                    }
                    cx.notify(); // lint-allow(global-notify): the attachment dialog is drawn by the root's overlays, over every column
                }))
            });
        let actions = ui::row().items_stretch()
            .justify_end()
            .gap_2()
            .child(ui::button("modify-attachment-cancel", Tone::Secondary, cx).child("Cancel").on_click(cx.listener(|this, _, _, cx| this.close_modify_attachment(cx))))
            .child(ui::button("modify-attachment-save", Tone::Primary, cx).child("Save").on_click(cx.listener(|this, _, _, cx| this.save_modify_attachment(cx))));
        let body = ui::dialog_body()
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if e.keystroke.key == "escape" {
                    this.close_modify_attachment(cx);
                }
            }))
            .child(head)
            .child(self.modify_preview(form, cx))
            .child(ui::field("Filename", cx).child(name))
            .children(description)
            .child(spoiler)
            .child(actions);
        let backdrop = div().id("modify-attachment-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_modify_attachment(cx)));
        Some(ui::modal(backdrop, ui::dialog("modify-attachment", 440., body, window, cx), cx))
    }

    // The picture as the tile shows it, or the file's type icon.
    fn modify_preview(&self, form: &ModifyAttachment, cx: &Context<Self>) -> impl IntoElement + use<> {
        let well = ui::row().h_40().flex_none().overflow_hidden().squircle(RADIUS_LG).bg(palette::selected(cx)).text_color(palette::muted(cx)).justify_center();
        let corners = ui::all(RADIUS_LG);
        match &form.target {
            Target::Pending(path) => match self.file_previews.get(path).cloned().flatten() {
                Some(picture) => well.child(ui::media(picture, corners).size_full().object_fit(gpui_kit::ObjectFit::Cover)),
                None => well.child(Icon::new(form.kind.glyph()).size_12()),
            },
            Target::Sent { message, index } => {
                let item = &message.media[*index];
                let url = match &item.kind {
                    MediaKind::Video { poster } | MediaKind::Gif { poster } => poster.clone(),
                    MediaKind::Image => item.url.clone(),
                };
                well.child(ui::media(crate::discord::sized_image(&url, 392., 160., (item.width, item.height)), corners).size_full().object_fit(gpui_kit::ObjectFit::Cover))
            }
        }
    }
}

impl FileKind {
    // Discord offers alt text only for pictures and videos.
    pub fn describable(self) -> bool {
        self != FileKind::Other
    }

    pub fn glyph(self) -> Glyph {
        match self {
            FileKind::Image => Glyph::Image,
            FileKind::Video => Glyph::Video,
            FileKind::Other => Glyph::File,
        }
    }
}

// A CDN URL's last path segment: the attachment's filename.
fn file_name(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.rsplit('/').next().unwrap_or(path).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sent_file_is_named_by_its_url() {
        assert_eq!(file_name("https://cdn.discordapp.com/attachments/1/2/pier.jpg?ex=a&is=b"), "pier.jpg");
        assert_eq!(file_name("pier.jpg"), "pier.jpg");
    }
}
