// What the registry draws: the groups of rows for its own settings, the search results that stand in for
// the nav while you type, and the jump from a result to its row.
use std::time::Duration;

use gpui_kit::{AnyElement, App, Context, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, component::Icon, div};
use serde_json::json;

use super::{
    RoleStyle, Section, Spoilers,
    registry::{self, Choice, Control, Setting, Toggle},
};
use crate::{Waffle, icons::Glyph, palette, ui};

// How long a jumped-to row keeps its highlight.
const HIGHLIGHT_FOR: Duration = Duration::from_millis(1600);

impl Waffle {
    // A section per group on `page` whose rows the registry draws (no `Custom` setting in it).
    pub(super) fn registry_sections(&mut self, page: super::Page, window: &mut Window, cx: &mut Context<Self>) -> Vec<Section> {
        let mut out = Vec::new();
        for group in registry::groups(registry::SETTINGS, page) {
            let settings: Vec<&'static Setting> = registry::SETTINGS.iter().filter(|s| s.page == page && s.group == group).collect();
            if settings.iter().any(|s| s.control == Control::Custom) {
                continue;
            }
            let mut card = ui::Card::new(cx);
            for (i, setting) in settings.iter().enumerate() {
                card = match setting.control {
                    Control::Choice(choice) => card.child(self.choice_rows(setting, choice, cx)),
                    _ => card.child(self.registry_row(setting, i > 0, window, cx)),
                };
            }
            let caption = match settings.first().map(|s| s.control) {
                Some(Control::Choice(_)) => settings[0].caption,
                Some(Control::Link(_)) => Some("These open Discord in your browser."),
                _ => None,
            };
            out.push(Section::keyed(group, ui::settings_group(group, cx).child(card).children(caption.map(|c| ui::settings_caption(c, cx)))));
        }
        out
    }

    pub(super) fn registry_row(&self, setting: &'static Setting, line: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if let Control::Switch(toggle) = setting.control {
            return self.toggle_row(setting, toggle, line, window, cx);
        }
        let row = ui::Item::setting(setting.id, line, cx).child(ui::settings_text(setting.title, setting.caption, cx));
        match setting.control {
            Control::Link(url) => ui::settings_choice(row, cx)
                .child(div().text_color(palette::faint(cx)).child(Icon::new(Glyph::ChevronRight).size(ui::ICON_MD)))
                .on_click(move |_, _, cx: &mut App| cx.open_url(url))
                .into_any_element(),
            Control::Switch(_) | Control::Choice(_) | Control::Custom => row.into_any_element(),
        }
    }

    // A switch row bound to `toggle`, for registry switches and the custom rows that hold one. A blocked
    // one dims its switch and says why under its title.
    pub(super) fn toggle_row(&self, setting: &'static Setting, toggle: Toggle, line: bool, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let blocked = self.toggle_blocked(toggle);
        let row = ui::Item::setting(setting.id, line, cx).child(ui::settings_text(setting.title, blocked.or(setting.caption), cx));
        let switch = ui::Switch::new(SharedString::from(format!("{}-switch", setting.id))).checked(self.toggle_value(toggle));
        if blocked.is_some() {
            return row.child(switch.opacity(0.45)).into_any_element();
        }
        ui::settings_choice(row, cx).child(switch).on_click(cx.listener(move |this, _, _, cx| this.flip(toggle, cx))).into_any_element()
    }

    // A choice fills its card with one radio row per option.
    fn choice_rows(&self, setting: &'static Setting, choice: Choice, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.choice_value(choice);
        ui::col().id(setting.id).children(choice.options().iter().enumerate().map(|(i, &(value, title, detail))| {
            let row = ui::Item::setting(SharedString::from(format!("{}-{value}", setting.id)), i > 0, cx).child(ui::RadioGroupItem::mark(current == value)).child(ui::settings_text(title, detail, cx));
            ui::settings_choice(row, cx).on_click(cx.listener(move |this, _, _, cx| this.choose_setting(choice, value, cx)))
        }))
    }

    pub(super) fn toggle_value(&self, toggle: Toggle) -> bool {
        let account = &self.account_settings;
        match toggle {
            Toggle::DesktopNotifications => self.prefs.desktop_notifications,
            Toggle::DockBadge => self.prefs.dock_badge,
            Toggle::Clock24 => self.prefs.clock_24h,
            Toggle::ImageDescriptions => self.prefs.show_image_descriptions,
            Toggle::AutoDownloadUpdates => self.prefs.auto_download_updates,
            Toggle::RenderEmbeds => account.render_embeds,
            Toggle::RenderReactions => account.render_reactions,
            Toggle::TtsCommand => self.prefs.tts_command,
            Toggle::DeveloperMode => account.developer_mode,
            Toggle::ShareActivity => account.show_current_game,
            Toggle::DmsFromServerMembers => !account.default_guilds_restricted,
            Toggle::MessageRequests => account.message_requests,
            Toggle::FriendsAll => account.friends_all,
            Toggle::FriendsMutualFriends => account.friends_mutual_friends,
            Toggle::FriendsMutualGuilds => account.friends_mutual_guilds,
            Toggle::InlineEmbedMedia => account.inline_embed_media,
            Toggle::InlineAttachmentMedia => account.inline_attachment_media,
            // Compact shows it on whatever's stored; cozy always has avatars, as in Discord.
            Toggle::CompactAvatars => !account.message_display_compact || self.prefs.compact_avatars,
            Toggle::ConvertEmoticons => account.convert_emoticons,
            Toggle::SendButton => self.prefs.send_button,
            Toggle::UnderlineLinks => self.prefs.underline_links,
            Toggle::DisplayNameStyles => self.prefs.display_name_styles,
            Toggle::DesaturateUserColors => self.prefs.desaturate_user_colors,
            Toggle::HighContrast => self.prefs.high_contrast,
            Toggle::SwitchIcons => self.prefs.switch_icons,
            Toggle::ReduceMotion => self.prefs.reduces_motion(),
            Toggle::ReduceMotionSync => self.prefs.reduce_motion_sync,
            // Reduced motion turns these off without touching what's stored, as Discord does.
            Toggle::GifAutoPlay => account.gif_auto_play && !self.prefs.reduces_motion(),
            Toggle::AnimateEmoji => account.animate_emoji && !self.prefs.reduces_motion(),
        }
    }

    // Why a switch can't be flipped right now, shown under its title; None when it can.
    pub(super) fn toggle_blocked(&self, toggle: Toggle) -> Option<&'static str> {
        match toggle {
            Toggle::CompactAvatars if !self.account_settings.message_display_compact => Some("To hide avatars, go to Accessibility and select Compact under Chat message display."),
            Toggle::ReduceMotion if self.prefs.reduce_motion_sync => Some("Following your computer's setting"),
            Toggle::GifAutoPlay | Toggle::AnimateEmoji if self.prefs.reduces_motion() => Some("This setting has been automatically disabled because of your reduced motion setting."),
            _ => None,
        }
    }

    fn flip(&mut self, toggle: Toggle, cx: &mut Context<Self>) {
        let on = !self.toggle_value(toggle);
        let before = self.account_settings.clone();
        let account = &mut self.account_settings;
        let patch = match toggle {
            Toggle::DesktopNotifications => return self.update_prefs(cx, |p| p.desktop_notifications = on),
            Toggle::DockBadge => {
                self.update_prefs(cx, |p| p.dock_badge = on);
                return self.update_badge();
            }
            Toggle::Clock24 => return self.update_prefs(cx, |p| p.clock_24h = on),
            Toggle::ImageDescriptions => {
                // Captions under single pictures change row heights.
                self.list.remeasure();
                return self.update_prefs(cx, |p| p.show_image_descriptions = on);
            }
            Toggle::TtsCommand => return self.update_prefs(cx, |p| p.tts_command = on),
            Toggle::AutoDownloadUpdates => return self.set_auto_download(on, cx),
            Toggle::CompactAvatars => return self.update_prefs(cx, |p| p.compact_avatars = on),
            Toggle::SendButton => return self.update_prefs(cx, |p| p.send_button = on),
            Toggle::UnderlineLinks => return self.update_prefs(cx, |p| p.underline_links = on),
            Toggle::DisplayNameStyles => return self.update_prefs(cx, |p| p.display_name_styles = on),
            Toggle::DesaturateUserColors => return self.update_prefs(cx, |p| p.desaturate_user_colors = on),
            Toggle::HighContrast => return self.update_prefs(cx, |p| p.high_contrast = on),
            Toggle::SwitchIcons => return self.update_prefs(cx, |p| p.switch_icons = on),
            Toggle::ReduceMotion => return self.update_prefs(cx, |p| p.reduce_motion = on),
            // Turning sync off keeps what the computer had, as Discord copies the OS value.
            Toggle::ReduceMotionSync => {
                return self.update_prefs(cx, |p| {
                    if !on {
                        p.reduce_motion = p.reduces_motion();
                    }
                    p.reduce_motion_sync = on;
                });
            }
            Toggle::InlineEmbedMedia => {
                account.inline_embed_media = on;
                json!({ "inline_embed_media": on })
            }
            Toggle::InlineAttachmentMedia => {
                account.inline_attachment_media = on;
                json!({ "inline_attachment_media": on })
            }
            Toggle::ConvertEmoticons => {
                account.convert_emoticons = on;
                json!({ "convert_emoticons": on })
            }
            Toggle::GifAutoPlay => {
                account.gif_auto_play = on;
                json!({ "gif_auto_play": on })
            }
            Toggle::AnimateEmoji => {
                account.animate_emoji = on;
                json!({ "animate_emoji": on })
            }
            Toggle::RenderEmbeds => {
                account.render_embeds = on;
                json!({ "render_embeds": on })
            }
            Toggle::RenderReactions => {
                account.render_reactions = on;
                json!({ "render_reactions": on })
            }
            Toggle::DeveloperMode => {
                account.developer_mode = on;
                self.developer_mode = on;
                json!({ "developer_mode": on })
            }
            Toggle::ShareActivity => {
                account.show_current_game = on;
                self.publish_presence();
                json!({ "show_current_game": on })
            }
            Toggle::DmsFromServerMembers => {
                account.default_guilds_restricted = !on;
                json!({ "default_guilds_restricted": !on })
            }
            Toggle::MessageRequests => {
                account.message_requests = on;
                return self.save_settings_proto(before, super::proto::message_requests(on), cx);
            }
            // Everyone covers the other two, as in Discord: turning it on turns them on.
            Toggle::FriendsAll => {
                account.friends_all = on;
                if on {
                    (account.friends_mutual_friends, account.friends_mutual_guilds) = (true, true);
                }
                json!({ "friend_source_flags": account.friend_sources() })
            }
            Toggle::FriendsMutualFriends | Toggle::FriendsMutualGuilds => {
                if toggle == Toggle::FriendsMutualFriends {
                    account.friends_mutual_friends = on;
                } else {
                    account.friends_mutual_guilds = on;
                }
                account.friends_all &= on;
                json!({ "friend_source_flags": account.friend_sources() })
            }
        };
        self.save_account_settings(before, patch, cx);
    }

    fn choice_value(&self, choice: Choice) -> u8 {
        match choice {
            Choice::DmFilter => self.account_settings.explicit_content_filter,
            Choice::Spoilers => self.prefs.spoilers as u8,
            Choice::RoleStyle => self.prefs.role_style as u8,
            Choice::StickerAnimation => self.account_settings.animate_stickers,
            // GAP: unset, Discord derives the filter from age and the explicit-content setting; we show its usual result.
            Choice::SpamFilter => match self.account_settings.dm_spam_filter {
                0 => super::proto::SPAM_FILTER_NON_FRIENDS,
                set => set,
            },
        }
    }

    fn choose_setting(&mut self, choice: Choice, value: u8, cx: &mut Context<Self>) {
        let before = self.account_settings.clone();
        let patch = match choice {
            Choice::DmFilter => {
                self.account_settings.explicit_content_filter = value;
                json!({ "explicit_content_filter": value })
            }
            Choice::Spoilers => {
                let spoilers = [Spoilers::OnClick, Spoilers::IfModerator, Spoilers::Always][value.min(2) as usize];
                return self.update_prefs(cx, |p| p.spoilers = spoilers);
            }
            Choice::RoleStyle => {
                let style = [RoleStyle::InNames, RoleStyle::NextToNames, RoleStyle::Hidden][value.min(2) as usize];
                return self.update_prefs(cx, |p| p.role_style = style);
            }
            Choice::StickerAnimation => {
                self.account_settings.animate_stickers = value;
                json!({ "animate_stickers": value })
            }
            Choice::SpamFilter => {
                self.account_settings.dm_spam_filter = value;
                return self.save_settings_proto(before, super::proto::spam_filter(value), cx);
            }
        };
        self.save_account_settings(before, patch, cx);
    }

    // Shown at once, saved to Discord after; if Discord refuses, `before` (the settings ahead of the change)
    // comes back with a toast.
    pub(crate) fn save_account_settings(&mut self, before: crate::discord::AccountSettings, patch: serde_json::Value, cx: &mut Context<Self>) {
        // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
        cx.notify();
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            if client.set_account_settings(patch).await.is_err() {
                let _ = this.update(cx, |this, cx| {
                    this.account_settings = before;
                    this.developer_mode = this.account_settings.developer_mode;
                    this.publish_presence();
                    this.show_toast("Couldn't save that setting", cx);
                });
            }
        })
        // lint-allow(detached-task): a one-shot task that ends when its request answers, and it updates through a WeakEntity so a closed view drops the result
        .detach();
    }

    // The same for a setting that lives only in the settings proto: `settings` is the base64 of the change.
    pub(super) fn save_settings_proto(&mut self, before: crate::discord::AccountSettings, settings: String, cx: &mut Context<Self>) {
        // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
        cx.notify();
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            if client.call(crate::discord::Method::PATCH, "/users/@me/settings-proto/1".into(), Some(json!({ "settings": settings }))).await.is_err() {
                let _ = this.update(cx, |this, cx| {
                    this.account_settings = before;
                    this.show_toast("Couldn't save that setting", cx);
                });
            }
        })
        // lint-allow(detached-task): a one-shot task that ends when its request answers, and it updates through a WeakEntity so a closed view drops the result
        .detach();
    }

    // The search results that replace the nav while there's a query: title, then where it lives.
    pub(super) fn setting_results(&self, query: &str, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let hits = registry::search(registry::SETTINGS, query);
        let selected = self.settings.as_ref().map_or(0, |s| s.result).min(hits.len().saturating_sub(1));
        if hits.is_empty() {
            return vec![div().px_2p5().py_2().text_color(palette::muted(cx)).child("No settings match").into_any_element()];
        }
        hits.into_iter()
            .enumerate()
            .map(|(i, setting)| {
                let place = format!("{} \u{203a} {}", setting.page.label(), setting.group);
                ui::DropdownMenuItem::new(SharedString::from(format!("result-{}", setting.id)), cx)
                    .highlighted(i == selected)
                    .h_auto()
                    .py_1p5()
                    .child(
                        ui::col().min_h_0()
                            .flex_1()
                            .child(ui::line(setting.title).text_color(palette::strong(cx)))
                            .child(ui::line(place).text_size(ui::TEXT_XS).text_color(palette::muted(cx))),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| this.jump_to_setting(setting, window, cx)))
                    .into_any_element()
            })
            .collect()
    }

    // Opens the setting's page scrolled to its group, and lights its row for a moment.
    pub(super) fn jump_to_setting(&mut self, setting: &'static Setting, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = &mut self.settings else { return };
        (view.page, view.section, view.select, view.recording, view.asking, view.result) = (setting.page, 0, None, None, None, 0);
        view.pending_jump = Some(setting.group);
        self.settings_search.update(cx, |state, cx| state.set_value("", window, cx));
        cx.set_global(ui::SettingsHighlight(Some(SharedString::from(setting.row_id()))));
        let row = setting.row_id();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HIGHLIGHT_FOR).await;
            let _ = this.update(cx, |_, cx| {
                if cx.try_global::<ui::SettingsHighlight>().and_then(|h| h.0.as_deref()) == Some(row) {
                    cx.set_global(ui::SettingsHighlight(None));
                    // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
                    cx.notify();
                }
            });
        })
        // lint-allow(detached-task): a one-shot task that ends when its request answers, and it updates through a WeakEntity so a closed view drops the result
        .detach();
        // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
        cx.notify();
    }

    // ⌘F (Ctrl+F) focuses search; while it has a query, arrows move through results and Enter opens one.
    pub(super) fn settings_search_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        if key.key == "escape" && self.cancel_profile_edit(cx) {
            return cx.stop_propagation();
        }
        let focus = gpui_kit::Focusable::focus_handle(self.settings_search.read(cx), cx);
        if key.key == "f" && (key.modifiers.platform || key.modifiers.control) {
            window.focus(&focus, cx);
            return cx.stop_propagation();
        }
        let query = self.settings_search.read(cx).value().to_string();
        let hits = registry::search(registry::SETTINGS, &query);
        if hits.is_empty() || !focus.is_focused(window) {
            return;
        }
        let Some(view) = &mut self.settings else { return };
        match key.key.as_str() {
            "down" => view.result = (view.result + 1).min(hits.len() - 1),
            "up" => view.result = view.result.saturating_sub(1),
            "enter" => {
                let setting = hits[view.result.min(hits.len() - 1)];
                self.jump_to_setting(setting, window, cx);
            }
            _ => return,
        }
        cx.stop_propagation();
        // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
        cx.notify();
    }
}
