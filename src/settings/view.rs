// What the registry draws: the groups of rows for its own settings, the search results that stand in for
// the nav while you type, and the jump from a result to its row.
use std::time::Duration;

use gpui_kit::{AnyElement, App, Context, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, component::Icon, div, px};
use serde_json::json;

use super::{
    Section,
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
            let mut card = ui::settings_card(cx);
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

    fn registry_row(&self, setting: &'static Setting, line: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let row = ui::settings_row(setting.id, line, cx).child(ui::settings_text(setting.title, setting.caption, cx));
        match setting.control {
            Control::Switch(toggle) => {
                let switch = ui::switch_live(SharedString::from(format!("{}-switch", setting.id)), self.toggle_value(toggle), window, cx);
                ui::settings_choice(row, cx).child(switch).on_click(cx.listener(move |this, _, _, cx| this.flip(toggle, cx))).into_any_element()
            }
            Control::Link(url) => ui::settings_choice(row, cx)
                .child(div().text_color(palette::faint(cx)).child(Icon::new(Glyph::ChevronRight).size(px(16.))))
                .on_click(move |_, _, cx: &mut App| cx.open_url(url))
                .into_any_element(),
            Control::Choice(_) | Control::Custom => row.into_any_element(),
        }
    }

    // A choice fills its card with one radio row per option.
    fn choice_rows(&self, setting: &'static Setting, choice: Choice, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.choice_value(choice);
        div().id(setting.id).flex().flex_col().children(choice.options().iter().enumerate().map(|(i, &(value, title, detail))| {
            let row = ui::settings_row(SharedString::from(format!("{}-{value}", setting.id)), i > 0, cx).child(ui::radio(current == value, cx)).child(ui::settings_text(title, detail, cx));
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
            Toggle::FriendsAll => account.friends_all,
            Toggle::FriendsMutualFriends => account.friends_mutual_friends,
            Toggle::FriendsMutualGuilds => account.friends_mutual_guilds,
        }
    }

    fn flip(&mut self, toggle: Toggle, cx: &mut Context<Self>) {
        let on = !self.toggle_value(toggle);
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
        self.save_account_settings(patch, cx);
    }

    fn choice_value(&self, choice: Choice) -> u8 {
        match choice {
            Choice::DmFilter => self.account_settings.explicit_content_filter,
        }
    }

    fn choose_setting(&mut self, choice: Choice, value: u8, cx: &mut Context<Self>) {
        let patch = match choice {
            Choice::DmFilter => {
                self.account_settings.explicit_content_filter = value;
                json!({ "explicit_content_filter": value })
            }
        };
        self.save_account_settings(patch, cx);
    }

    // Shown at once, saved to Discord after; if Discord refuses, the earlier value comes back with a toast.
    fn save_account_settings(&mut self, patch: serde_json::Value, cx: &mut Context<Self>) {
        cx.notify();
        let Some(client) = self.client.clone() else { return };
        let before = self.account_settings.clone();
        cx.spawn(async move |this, cx| {
            if client.set_account_settings(patch).await.is_err() {
                let _ = this.update(cx, |this, cx| {
                    this.account_settings = before;
                    this.developer_mode = this.account_settings.developer_mode;
                    this.show_toast("Couldn't save that setting", cx);
                });
            }
        })
        .detach();
    }

    // The search results that replace the nav while there's a query: title, then where it lives.
    pub(super) fn setting_results(&self, query: &str, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let hits = registry::search(registry::SETTINGS, query);
        let selected = self.settings.as_ref().map_or(0, |s| s.result).min(hits.len().saturating_sub(1));
        if hits.is_empty() {
            return vec![div().px(px(10.)).py(px(8.)).text_color(palette::muted(cx)).child("No settings match").into_any_element()];
        }
        hits.into_iter()
            .enumerate()
            .map(|(i, setting)| {
                let place = format!("{} \u{203a} {}", setting.page.label(), setting.group);
                ui::menu_row(SharedString::from(format!("result-{}", setting.id)), cx)
                    .highlighted(i == selected)
                    .h_auto()
                    .py(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().truncate().text_color(palette::strong(cx)).child(setting.title))
                            .child(div().truncate().text_size(px(12.)).text_color(palette::muted(cx)).child(place)),
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
                    cx.notify();
                }
            });
        })
        .detach();
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
        cx.notify();
    }
}
