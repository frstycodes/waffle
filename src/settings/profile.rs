// My account's editable parts: display name and username in place, avatar and banner from a file. Anything
// Discord gates behind Nitro or sells (banners for non-subscribers, animated avatars, decorations, effects,
// nameplates) goes to Discord instead.
use base64::Engine;
use gpui_kit::{AnyElement, AppContext, Context, Entity, Focusable, IntoElement, ParentElement, SharedString, Styled, Window, component::input::{InputEvent, InputState}, div, prelude::FluentBuilder};
use serde_json::{Value, json};

use crate::{Waffle, palette, ui};

const DISCORD: &str = "https://discord.com/app";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ProfileField {
    DisplayName,
    Username,
}

// A field being edited in place: its input, and for a username the password Discord asks for.
pub(crate) struct ProfileEdit {
    pub field: ProfileField,
    pub input: Entity<InputState>,
    pub password: Option<Entity<InputState>>,
    pub saving: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ProfileImage {
    Avatar,
    Banner,
}

impl Waffle {
    pub(super) fn start_profile_edit(&mut self, field: ProfileField, window: &mut Window, cx: &mut Context<Self>) {
        let value = match field {
            ProfileField::DisplayName => self.my_account.global_name.clone().or_else(|| self.me.as_ref().map(|m| m.name.clone())).unwrap_or_default(),
            ProfileField::Username => self.my_account.username.clone(),
        };
        let input = cx.new(|cx| InputState::new(window, cx).default_value(value));
        let password = (field == ProfileField::Username).then(|| cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("Current password")));
        for state in std::iter::once(&input).chain(password.as_ref()) {
            cx.subscribe_in(state, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.save_profile_edit(window, cx);
                }
            })
            .detach();
        }
        input.update(cx, |state, cx| {
            state.focus(window, cx);
            state.select_all(window, cx);
        });
        if let Some(view) = &mut self.settings {
            view.editing = Some(ProfileEdit { field, input, password, saving: false });
        }
        cx.notify();
    }

    // Esc puts the field back as it was.
    pub(super) fn cancel_profile_edit(&mut self, cx: &mut Context<Self>) -> bool {
        let editing = self.settings.as_mut().and_then(|v| v.editing.take()).is_some();
        if editing {
            cx.notify();
        }
        editing
    }

    fn save_profile_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.settings.as_mut().and_then(|v| v.editing.as_mut()) else { return };
        if edit.saving {
            return;
        }
        let value = edit.input.read(cx).value().trim().to_owned();
        let patch = match edit.field {
            ProfileField::DisplayName => json!({ "global_name": if value.is_empty() { Value::Null } else { Value::from(value) } }),
            ProfileField::Username => {
                let password = edit.password.as_ref().map(|p| p.read(cx).value().to_string()).unwrap_or_default();
                if password.is_empty() {
                    if let Some(p) = &edit.password {
                        p.update(cx, |state, cx| state.focus(window, cx));
                    }
                    return;
                }
                json!({ "username": value, "password": password })
            }
        };
        edit.saving = true;
        self.send_profile_patch(patch, cx);
    }

    pub(super) fn pick_profile_image(&mut self, image: ProfileImage, cx: &mut Context<Self>) {
        if image == ProfileImage::Banner && !self.my_account.premium {
            self.show_toast("Banners need Nitro: set yours on Discord", cx);
            return cx.open_url(DISCORD);
        }
        self.prompt_profile_image(cx, move |this, _, uri, cx| {
            let key = if image == ProfileImage::Avatar { "avatar" } else { "banner" };
            this.send_profile_patch(json!({ key: uri }), cx);
        });
    }

    // Asks for a picture and hands over its file and data URI. Animated ones need Nitro, so those go to Discord.
    pub(super) fn prompt_profile_image(&mut self, cx: &mut Context<Self>, then: impl FnOnce(&mut Self, std::path::PathBuf, String, &mut Context<Self>) + 'static) {
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        let premium = self.my_account.premium;
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let result = image_data_uri(&path);
            let _ = this.update(cx, |this, cx| match result {
                Ok((_, animated)) if animated && !premium => {
                    this.show_toast("Animated avatars need Nitro: set yours on Discord", cx);
                    cx.open_url(DISCORD);
                }
                Ok((uri, _)) => then(this, path, uri, cx),
                Err(e) => this.show_toast(e, cx),
            });
        })
        .detach();
    }

    // Saves, then takes Discord's answer as the truth: your name, avatar and banner everywhere.
    fn send_profile_patch(&mut self, patch: Value, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = client.modify_me(patch).await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(user) => {
                        this.my_account = crate::discord::MyAccount::parse(&user);
                        let updated = crate::discord::parse_user(&user);
                        if let Some(me) = &mut this.me {
                            (me.name, me.avatar) = (updated.name, updated.avatar);
                        }
                        if let Some(view) = &mut this.settings {
                            view.editing = None;
                        }
                    }
                    Err(e) => {
                        if let Some(edit) = this.settings.as_mut().and_then(|v| v.editing.as_mut()) {
                            edit.saving = false;
                        }
                        // Two-factor and captcha checks happen on Discord's own pages.
                        let message = if needs_discord(&e) { "Discord needs to confirm it's you: finish on Discord".to_owned() } else { e };
                        this.show_toast(message, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    // The value of a field, or its input while it's being edited (with the password under it for a username).
    pub(super) fn profile_value(&self, field: ProfileField, label: &'static str, value: SharedString, cx: &mut Context<Self>) -> AnyElement {
        let editing = self.settings.as_ref().and_then(|v| v.editing.as_ref()).filter(|e| e.field == field);
        let Some(edit) = editing else {
            return super::field_text(label, value, cx).into_any_element();
        };
        let id = if field == ProfileField::Username { "edit-username" } else { "edit-name" };
        ui::col()
            .flex_1()
            .gap_1p5()
            .child(super::field_label(label, cx))
            .child(ui::text_field(id, gpui_kit::component::input::Input::new(&edit.input).appearance(false), &edit.input.read(cx).focus_handle(cx)))
            .when_some(edit.password.as_ref(), |d, password| {
                d.child(ui::text_field("edit-password", gpui_kit::component::input::Input::new(password).appearance(false), &password.read(cx).focus_handle(cx)))
                    .child(div().text_size(ui::TEXT_XS).text_color(palette::muted(cx)).child("Discord asks for your password to change your username."))
            })
            .into_any_element()
    }
}

fn needs_discord(error: &str) -> bool {
    let e = error.to_lowercase();
    e.contains("captcha") || e.contains("two-factor") || e.contains("mfa") || e.contains("verify")
}

// A picked image as Discord's image data (a data URI), and whether it's animated.
pub(crate) fn image_data_uri(path: &std::path::Path) -> Result<(String, bool), String> {
    let mime = match path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => return Err("Pick a PNG, JPEG, GIF or WebP image".to_owned()),
    };
    let bytes = std::fs::read(path).map_err(|e| format!("Couldn't read that file: {e}"))?;
    let uri = format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes));
    Ok((uri, mime == "image/gif"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn images_become_data_uris_and_gifs_count_as_animated() {
        let dir = std::env::temp_dir().join(format!("waffle-profile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("a.PNG");
        std::fs::write(&png, [1u8, 2, 3]).unwrap();
        assert_eq!(super::image_data_uri(&png).unwrap(), ("data:image/png;base64,AQID".to_owned(), false));
        let gif = dir.join("b.gif");
        std::fs::write(&gif, [0u8]).unwrap();
        assert!(super::image_data_uri(&gif).unwrap().1);
        assert!(super::image_data_uri(&dir.join("c.txt")).is_err());
        std::fs::remove_dir_all(dir).ok();
    }
}
