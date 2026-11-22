// Registered games, as Discord's Activity settings page lays it out: what's playing now (or "No game
// detected" with Add it!, which lists running programs to add as a game), then every game seen here,
// each with its detection switch. Added programs can be removed.
use gpui_kit::{Context, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, component::Icon, div, prelude::FluentBuilder as _, px};

use super::Section;
use crate::{
    Waffle,
    games::{Added, Seen},
    icons::Glyph,
    palette,
    ui::{self, Tone},
};

impl Waffle {
    pub(super) fn games_page(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Section> {
        let now = chrono::Utc::now().timestamp_millis();
        let mut card = ui::settings_card(cx);
        let playing = match &self.games.mine {
            Some(game) => ui::settings_row("registered-games", false, cx).child(ui::settings_text(format!("Now playing: {}", game.name), Some("Shown on your profile while Activity status is on"), cx)),
            None => {
                let add = ui::button("game-add-it", Tone::Secondary, cx).small().child("Add it!").on_click(cx.listener(|this, _, _, cx| {
                    let open = this.settings.as_ref().is_some_and(|s| s.adding_game.is_some());
                    if let Some(view) = &mut this.settings {
                        view.adding_game = (!open).then(crate::games::addable);
                    }
                    cx.notify();
                }));
                ui::settings_row("registered-games", false, cx).child(ui::settings_text("No game detected", Some("Not seeing your game?"), cx)).child(add)
            }
        };
        card = card.child(playing);
        if let Some(programs) = self.settings.as_ref().and_then(|s| s.adding_game.clone()) {
            if programs.is_empty() {
                card = card.child(ui::settings_row("game-add-none", true, cx).child(ui::settings_text("No programs running that could be a game", None::<&str>, cx)));
            }
            for (i, (path, name)) in programs.into_iter().enumerate() {
                let added = Added { path: path.clone(), name: name.clone() };
                let row = ui::settings_choice(ui::settings_row(SharedString::from(format!("game-add-{i}")), true, cx), cx)
                    .child(ui::settings_text(name, Some(path), cx))
                    .child(div().text_color(palette::faint(cx)).child(Icon::new(Glyph::Plus).size(px(16.))))
                    .on_click(cx.listener(move |this, _, _, cx| this.add_game(added.clone(), cx)));
                card = card.child(row);
            }
        }
        let mut seen = self.prefs.games_seen.clone();
        seen.sort_by_key(|s| std::cmp::Reverse(s.last));
        let mut list = ui::settings_card(cx);
        if seen.is_empty() {
            list = list.child(ui::settings_row("games-seen-none", false, cx).child(ui::settings_text("Games you play will show here", None::<&str>, cx)));
        }
        for (i, game) in seen.into_iter().enumerate() {
            list = list.child(self.seen_row(i, game, now, window, cx));
        }
        let caption = "Turning a game off stops Waffle detecting it here; it won't show as your activity.";
        vec![Section::keyed(
            "Registered games",
            ui::settings_group("Registered games", cx).child(card).child(ui::settings_group("Added games", cx).child(list)).child(ui::settings_caption(caption, cx)),
        )]
    }

    fn seen_row(&self, i: usize, game: Seen, now: i64, window: &mut Window, cx: &mut Context<Self>) -> gpui_kit::Stateful<gpui_kit::Div> {
        let on = !self.prefs.games_off.contains(&game.id);
        let added = self.prefs.games_added.iter().any(|a| a.path == game.id);
        let playing = self.games.mine.as_ref().is_some_and(|g| g.app_id == game.id);
        let detail = match (playing, added) {
            (true, _) => "Playing now".to_owned(),
            (false, true) => format!("Added by you · last played {}", crate::panel::ago(now - game.last).to_lowercase()),
            (false, false) => format!("Last played {}", crate::panel::ago(now - game.last).to_lowercase()),
        };
        let id = game.id.clone();
        let switch = ui::switch_live(SharedString::from(format!("game-detect-{i}")), on, window, cx).on_click(cx.listener(move |this, _, _, cx| this.toggle_game_detection(&id, cx)));
        let remove = added.then(|| {
            let path = game.id.clone();
            ui::icon_button(SharedString::from(format!("game-remove-{i}")), Glyph::X, "Remove game", cx).on_click(cx.listener(move |this, _, _, cx| this.remove_game(&path, cx)))
        });
        ui::settings_row(SharedString::from(format!("game-seen-{i}")), i > 0, cx).child(ui::settings_text(game.name, Some(detail), cx)).children(remove).child(switch).when(!on, |r| r.opacity(0.7))
    }

    fn add_game(&mut self, added: Added, cx: &mut Context<Self>) {
        if let Some(view) = &mut self.settings {
            view.adding_game = None;
        }
        self.update_prefs(cx, |p| {
            if !p.games_added.contains(&added) {
                p.games_added.push(added.clone());
            }
            p.games_off.retain(|id| *id != added.path);
            if !p.games_seen.iter().any(|s| s.id == added.path) {
                p.games_seen.push(Seen { id: added.path.clone(), name: added.name.clone(), last: chrono::Utc::now().timestamp_millis() });
            }
        });
    }

    fn remove_game(&mut self, path: &str, cx: &mut Context<Self>) {
        self.update_prefs(cx, |p| {
            p.games_added.retain(|a| a.path != path);
            p.games_seen.retain(|s| s.id != path);
            p.games_off.retain(|id| id != path);
        });
    }

    fn toggle_game_detection(&mut self, id: &str, cx: &mut Context<Self>) {
        let id = id.to_owned();
        self.update_prefs(cx, |p| match p.games_off.contains(&id) {
            true => p.games_off.retain(|g| *g != id),
            false => p.games_off.push(id),
        });
    }
}
