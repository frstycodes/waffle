// The Updates page: the version you're on, what the background updater is doing, the one action that
// moves it on (check, download, restart), the release notes, and whether downloads start by themselves.
use std::time::SystemTime;

use gpui_kit::{AnyElement, App, Context, FontWeight, IntoElement, Window, ParentElement, StatefulInteractiveElement, Styled, component::Icon, div, img, px};

use super::Section;
use crate::{
    Waffle,
    icons::Glyph,
    palette,
    ui::{self, Tone},
    updater::{CURRENT, Command, Status},
};

impl Waffle {
    pub(super) fn updates_page(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Section> {
        let status = &self.updates.status;
        let notes_version = status.version().unwrap_or(CURRENT).to_owned();
        let notes = ui::settings_choice(ui::settings_row("whats-new", true, cx).child(ui::settings_text(format!("What's new in {notes_version}"), Some("Release notes on GitHub"), cx)), cx)
            .child(div().text_color(palette::faint(cx)).child(Icon::new(Glyph::ChevronRight).size(ui::ICON_MD)))
            .on_click(move |_, _, cx: &mut App| cx.open_url(&crate::updater::release_notes(&notes_version)));
        let checked = self.updates.checked_at.map_or_else(|| "Not yet".to_owned(), |at| crate::panel::ago(elapsed_ms(at)));
        let last_checked = ui::settings_row("last-checked", true, cx).child(ui::settings_text("Last checked", None::<&str>, cx)).child(div().text_color(palette::muted(cx)).child(checked));
        let card = ui::settings_card(cx).child(self.update_status_row(window, cx)).child(notes).child(last_checked);
        vec![Section::keyed("Waffle", ui::settings_group("Waffle", cx).child(card).child(ui::settings_caption("Waffle checks for a new version about every hour.", cx)))]
    }

    // The app icon, the version, where the updater is, and the action that moves it on.
    fn update_status_row(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let status = &self.updates.status;
        let progress = match status {
            Status::Downloading { done, total: Some(total), .. } => Some(ui::progress_live("update-progress", *done as f32 / (*total).max(1) as f32, window, cx).into_any_element()),
            Status::Downloading { total: None, .. } => Some(ui::progress_indeterminate("update-progress", cx)),
            _ => None,
        };
        let text = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_0p5()
            .child(div().truncate().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(format!("Waffle {CURRENT}")))
            .child(div().text_size(ui::TEXT_XS).line_height(px(17.)).text_color(palette::muted(cx)).child(status_line(status, self.updates.updater.is_some())))
            .children(progress.map(|bar| div().mt_1p5().child(bar)));
        ui::settings_row("update-status", false, cx).child(img(self.prefs.app_icon.asset()).size_12().flex_none()).child(text).child(self.update_action(cx)).into_any_element()
    }

    fn update_action(&self, cx: &mut Context<Self>) -> AnyElement {
        match &self.updates.status {
            Status::Ready(staged) => {
                let staged = staged.clone();
                ui::button("restart-to-update-page", Tone::Primary, cx).child("Restart to update").on_click(cx.listener(move |this, _, _, cx| this.request_restart(Some(staged.clone()), cx))).into_any_element()
            }
            Status::Available(_) | Status::DownloadFailed(_) => ui::button("download-update", Tone::Primary, cx).child("Download update").on_click(cx.listener(|this, _, _, cx| this.ask_updater(Command::Download, cx))).into_any_element(),
            status => {
                let busy = matches!(status, Status::Checking | Status::Downloading { .. });
                ui::button("check-for-updates", Tone::Secondary, cx)
                    .child("Check for updates")
                    .disabled(busy)
                    .loading(matches!(status, Status::Checking).then_some("Checking"))
                    .on_click(cx.listener(|this, _, _, cx| this.ask_updater(Command::Check, cx)))
                    .into_any_element()
            }
        }
    }

    fn ask_updater(&mut self, command: Command, cx: &mut Context<Self>) {
        match &self.updates.updater {
            Some(updater) => updater.send(command),
            None => self.show_toast("Updates are off in development builds.", cx),
        }
    }

    // Turning it back on fetches what a check already found, rather than waiting for the next one.
    pub(super) fn set_auto_download(&mut self, on: bool, cx: &mut Context<Self>) {
        self.update_prefs(cx, |p| p.auto_download_updates = on);
        let Some(updater) = &self.updates.updater else { return };
        updater.set_auto(on);
        if on && matches!(self.updates.status, Status::Available(_)) {
            updater.send(Command::Download);
        }
    }
}

fn elapsed_ms(at: SystemTime) -> i64 {
    SystemTime::now().duration_since(at).map_or(0, |d| d.as_millis() as i64)
}

// The line under the version: where the updater is, in the user's words.
fn status_line(status: &Status, enabled: bool) -> String {
    match status {
        Status::Idle if !enabled => "Updates are off in this build".into(),
        Status::Idle => "Not checked yet".into(),
        Status::Checking => "Checking for updates…".into(),
        Status::UpToDate => "Up to date".into(),
        Status::Available(version) => format!("Waffle {version} is available"),
        Status::Downloading { version, done, total: Some(total) } => format!("Downloading {version}… {}%", done * 100 / (*total).max(1)),
        Status::Downloading { version, done, total: None } => format!("Downloading {version}… {} MB", done >> 20),
        Status::Ready(staged) => format!("Waffle {} is ready to install", staged.version),
        Status::CheckFailed => "Couldn't reach GitHub. Waffle will try again within the hour.".into(),
        Status::DownloadFailed(version) => format!("Couldn't download {version}. Waffle will try again within the hour."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_line_names_each_state() {
        assert_eq!(status_line(&Status::Idle, false), "Updates are off in this build");
        assert_eq!(status_line(&Status::UpToDate, true), "Up to date");
        assert_eq!(status_line(&Status::Downloading { version: "0.8.0".into(), done: 42, total: Some(100) }, true), "Downloading 0.8.0… 42%");
        assert_eq!(status_line(&Status::Downloading { version: "0.8.0".into(), done: 3 << 20, total: None }, true), "Downloading 0.8.0… 3 MB");
        assert_eq!(status_line(&Status::Downloading { version: "0.8.0".into(), done: 0, total: Some(0) }, true), "Downloading 0.8.0… 0%");
        assert_eq!(status_line(&Status::Available("0.8.0".into()), true), "Waffle 0.8.0 is available");
    }
}
