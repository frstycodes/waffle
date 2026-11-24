// The Advanced page's own rows: with Developer mode on, the crash logs Waffle kept (crash.rs), each one
// saved anywhere through the system save panel, and the folder one click away.
use gpui_kit::{App, Context, ParentElement, StatefulInteractiveElement, Styled, div, prelude::FluentBuilder as _};

use super::Section;
use crate::{
    Waffle, palette,
    ui::{self, Tone},
};

// The newest few; the folder holds the rest.
const SHOWN: usize = 5;

impl Waffle {
    pub(super) fn advanced_page(&self, cx: &mut Context<Self>) -> Vec<Section> {
        if !self.developer_mode {
            return Vec::new();
        }
        let logs = crate::crash::saved();
        let mut card = ui::settings_card(cx);
        if logs.is_empty() {
            card = card.child(ui::settings_row("crash-logs", false, cx).child(ui::settings_text("No crashes recorded", None::<&str>, cx)));
        }
        for (i, log) in logs.iter().take(SHOWN).enumerate() {
            let when = chrono::DateTime::from_timestamp(log.unix, 0).map(|t| t.with_timezone(&chrono::Local).format("%b %-d, %H:%M").to_string()).unwrap_or_default();
            let path = log.path.clone();
            let save = ui::button(("crash-save", i), Tone::Secondary, cx).small().child("Save…").on_click(cx.listener(move |this, _, _, cx| this.save_crash_log(path.clone(), cx)));
            let row = ui::settings_row(if i == 0 { "crash-logs".to_owned() } else { format!("crash-log-{i}") }, i > 0, cx);
            card = card.child(row.child(ui::settings_text(when, Some(log.summary.clone()), cx)).child(save));
        }
        let folder = crate::crash::dir();
        let show = (!logs.is_empty()).then(|| {
            ui::settings_choice(ui::settings_row("crash-folder", true, cx).child(ui::settings_text("Show in folder", Some(format!("{} saved", logs.len())), cx)), cx)
                .child(div().text_color(palette::faint(cx)).child(gpui_kit::component::Icon::new(crate::icons::Glyph::ChevronRight).size(ui::ICON_MD)))
                .when_some(folder, |row, dir| row.on_click(move |_, _, cx: &mut App| cx.reveal_path(&dir)))
        });
        let caption = "Waffle keeps its last 20 crashes. Attach one when you report a bug; it holds no messages or tokens.";
        vec![Section::keyed("Crash logs", ui::settings_group("Crash logs", cx).child(card.children(show)).child(ui::settings_caption(caption, cx)))]
    }

    fn save_crash_log(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        // Windows sets USERPROFILE, not HOME.
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
        let downloads = home.map(|h| std::path::PathBuf::from(h).join("Downloads")).unwrap_or_default();
        let name = path.file_name().and_then(|n| n.to_str()).map(|n| format!("waffle-{n}"));
        let target = cx.prompt_for_new_path(&downloads, name.as_deref());
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(to))) = target.await else { return };
            let (from, dest) = (path.clone(), to.clone());
            let result = cx.background_executor().spawn(async move { std::fs::copy(&from, &dest) }).await;
            this.update(cx, |this, cx| match result {
                Ok(_) => cx.reveal_path(&to),
                Err(e) => this.show_toast(format!("Couldn't save the crash log: {e}"), cx),
            })
            .ok();
        })
        // lint-allow(detached-task): ends after the one save-dialog answer and copy, and only updates the view if it is still alive
        .detach();
    }
}
