// The `/` picker above the composer and the option chips a picked command fills in.

use std::{collections::HashMap, ops::Range, sync::Arc, time::Duration};

use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, Context, Div, FontWeight, HighlightStyle, IntoElement, ScrollHandle, SharedString, StyledText, Task, Window,
    component::Icon,
    div, point, prelude::*, px, relative,
};
use serde_json::{Value, json};

use crate::{
    SuggestLead, Waffle,
    app_commands::{self, AppCommand, Choice, CommandIndex, CommandOption, OptionKind, Origin},
    icons::Glyph,
    interactions::InteractionKind,
    palette::{self, Level},
    ui::{self, RADIUS_LG, RADIUS_SM, RADIUS_XS, Squircle},
};

// How long typing pauses before an autocomplete option asks its app for choices.
const AUTOCOMPLETE_AFTER: Duration = Duration::from_millis(250);
const RECENT_MAX: usize = 5;
const LIST_HEIGHT: f32 = 340.;
const CHIP_HEIGHT: f32 = 28.;

#[derive(Default)]
pub(crate) struct CommandPicker {
    // By server, or by channel in a DM: the index Discord sent for it.
    indexes: HashMap<String, IndexState>,
    open: bool,
    // Esc closed the picker over this `/` text; it stays closed until the text stops being a command.
    dismissed: bool,
    query: String,
    // Which command row is chosen, counting command rows only.
    selected: usize,
    scroll: ScrollHandle,
    // GAP: kept in memory for this session only; Discord's own frecency list isn't read or written.
    recent: HashMap<String, u32>,
    pub(crate) filling: Option<Filling>,
}

pub(crate) enum IndexState {
    Loading,
    Failed(String),
    Ready(Arc<CommandIndex>),
}

/// A picked command whose options are being filled in, one chip each.
pub(crate) struct Filling {
    command: AppCommand,
    app: app_commands::App,
    // One per `command.options`, in the same order.
    inputs: Vec<OptionInput>,
    // Which options have chips, by index into `command.options`, in chip order.
    shown: Vec<usize>,
    // The chip being typed into, by position in `shown`.
    active: usize,
    highlighted: usize,
    // What the app answered for the active autocomplete option.
    suggestions: Vec<Choice>,
    awaiting: Option<String>,
    debounce: Option<Task<()>>,
    // Why the last Enter didn't run, shown in the hint bar until the next keystroke.
    blocked: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct OptionInput {
    pub text: String,
    // Set when a choice was picked: its value, sent instead of `text`.
    pub picked: Option<Value>,
}

impl CommandPicker {
    pub(crate) fn reset(&mut self) {
        self.open = false;
        self.dismissed = false;
        self.filling = None;
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) fn set_index(&mut self, scope: impl Into<String>, index: CommandIndex) {
        self.indexes.insert(scope.into(), IndexState::Ready(Arc::new(index)));
    }

    pub(crate) fn record_run(&mut self, command: &AppCommand) {
        *self.recent.entry(command_key(command)).or_default() += 1;
    }
}

impl Filling {
    fn active_option(&self) -> Option<(usize, &CommandOption)> {
        let i = *self.shown.get(self.active)?;
        Some((i, &self.command.options[i]))
    }

    fn active_input(&self) -> Option<&OptionInput> {
        self.shown.get(self.active).map(|&i| &self.inputs[i])
    }

    pub(crate) fn set_suggestions(&mut self, choices: Vec<Choice>) {
        self.suggestions = choices;
        self.highlighted = 0;
    }
}

// PICKER

impl Waffle {
    pub(crate) fn command_filling(&self) -> bool {
        self.command_picker.filling.is_some()
    }

    /// The composer changed outside of a command: opens, filters or closes the picker.
    pub(crate) fn update_command_picker(&mut self, text: &str, cx: &mut Context<Self>) {
        let picker = &mut self.command_picker;
        let query = text.strip_prefix('/').filter(|q| !q.contains(char::is_whitespace) && self.editing.is_none());
        let Some(query) = query else {
            picker.open = false;
            picker.dismissed = false;
            return;
        };
        if picker.dismissed {
            return;
        }
        if !picker.open || picker.query != query {
            picker.selected = 0;
            picker.scroll.scroll_to_item(0);
        }
        picker.open = true;
        picker.query = query.to_owned();
        self.autocomplete = None;
        self.ensure_command_index(cx);
    }

    // `/` in the composer and the picker open: the + menu's "Use an app command".
    pub(crate) fn start_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.command_picker.reset();
        self.composer.update(cx, |state, cx| {
            state.set_value("/", window, cx);
            state.set_selected_range(1..1, cx);
            state.focus(window, cx);
        });
        self.has_draft = true;
        self.update_command_picker("/", cx);
        cx.notify();
    }

    fn command_scope(&self) -> Option<(String, String, Option<String>)> {
        let channel = self.channel.as_ref()?;
        let key = self.guild_id.clone().unwrap_or_else(|| format!("dm:{}", channel.id));
        Some((key, channel.id.clone(), self.guild_id.clone()))
    }

    fn command_index(&self) -> Option<&IndexState> {
        self.command_picker.indexes.get(&self.command_scope()?.0)
    }

    fn ensure_command_index(&mut self, cx: &mut Context<Self>) {
        let Some((key, _, _)) = self.command_scope() else { return };
        if !self.command_picker.indexes.contains_key(&key) {
            self.fetch_command_index(cx);
        }
    }

    fn fetch_command_index(&mut self, cx: &mut Context<Self>) {
        let (Some(client), Some((key, channel_id, guild_id))) = (self.client.clone(), self.command_scope()) else { return };
        self.command_picker.indexes.insert(key.clone(), IndexState::Loading);
        cx.spawn(async move |this, cx| {
            let result = client.command_index(&channel_id, guild_id.as_deref()).await;
            this.update(cx, |this, cx| {
                let state = match result {
                    Ok(index) => IndexState::Ready(Arc::new(index)),
                    Err(error) => IndexState::Failed(error),
                };
                this.command_picker.indexes.insert(key, state);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn ready_index(&self) -> Option<Arc<CommandIndex>> {
        match self.command_index()? {
            IndexState::Ready(index) => Some(index.clone()),
            _ => None,
        }
    }

    fn command_rows(&self) -> Vec<ListRow> {
        self.ready_index().map(|index| listing(&index, &self.command_picker.query, &self.command_picker.recent)).unwrap_or_default()
    }

    fn move_command(&mut self, delta: isize, cx: &mut Context<Self>) {
        let rows = self.command_rows();
        let picks = pickable(&rows);
        if picks.is_empty() {
            return;
        }
        let picker = &mut self.command_picker;
        picker.selected = (picker.selected as isize + delta).rem_euclid(picks.len() as isize) as usize;
        picker.scroll.scroll_to_item(picks[picker.selected]);
        cx.notify();
    }

    fn pick_selected_command(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let rows = self.command_rows();
        let Some(&row) = pickable(&rows).get(self.command_picker.selected) else { return false };
        let ListRow::Command { command, .. } = rows[row] else { return false };
        self.pick_command(command, window, cx);
        true
    }

    /// Turns command `index` (into the open index) into a chip, with a chip for each required option.
    pub(crate) fn pick_command(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(commands) = self.ready_index() else { return };
        let Some(command) = commands.commands.get(index).cloned() else { return };
        let app = commands.apps.iter().find(|a| a.id == command.application_id).cloned().unwrap_or_else(|| app_commands::App { id: command.application_id.clone(), name: "App".into(), icon: None });
        let shown = command.options.iter().enumerate().filter(|(_, o)| o.required).map(|(i, _)| i).collect();
        let inputs = vec![OptionInput::default(); command.options.len()];
        self.command_picker.open = false;
        self.command_picker.filling = Some(Filling { command, app, inputs, shown, active: 0, highlighted: 0, suggestions: Vec::new(), awaiting: None, debounce: None, blocked: None });
        self.autocomplete = None;
        self.has_draft = true;
        self.composer.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.set_placeholder("", window, cx);
            state.focus(window, cx);
        });
        self.schedule_autocomplete(cx);
        cx.notify();
    }

    // Esc or Backspace past the first option: the command goes back to being `/name` text.
    fn drop_command(&mut self, reopen: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(filling) = self.command_picker.filling.take() else { return };
        let text = format!("/{}", filling.command.name());
        let end = text.len();
        let placeholder = self.channel.as_ref().map(crate::channel_mention).map(|c| format!("Message {c}")).unwrap_or_default();
        self.composer.update(cx, |state, cx| {
            state.set_value(text.clone(), window, cx);
            state.set_selected_range(end..end, cx);
            state.set_placeholder(placeholder, window, cx);
        });
        self.command_picker.dismissed = !reopen;
        self.update_command_picker(&text, cx);
        cx.notify();
    }

    /// Keys the picker or the option chips take before the composer: arrows, Esc, Backspace.
    pub(crate) fn on_command_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.command_picker.open {
            match key {
                "up" => self.move_command(-1, cx),
                "down" => self.move_command(1, cx),
                "escape" => {
                    self.command_picker.open = false;
                    self.command_picker.dismissed = true;
                    cx.notify();
                }
                _ => return false,
            }
            return true;
        }
        let Some(filling) = &self.command_picker.filling else { return false };
        let empty_first = filling.active == 0 && filling.active_input().is_none_or(|i| i.text.is_empty());
        match key {
            "escape" => self.drop_command(false, window, cx),
            "backspace" if empty_first => self.drop_command(true, window, cx),
            "up" | "down" => return self.move_choice(if key == "up" { -1 } else { 1 }, cx),
            _ => return false,
        }
        true
    }

    /// Tab (or Shift-Tab, `back`): picks the highlighted command, or the highlighted choice, or moves between options.
    pub(crate) fn on_command_tab(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.command_picker.open {
            return !back && self.pick_selected_command(window, cx);
        }
        let Some(filling) = &self.command_picker.filling else { return false };
        let (highlighted, active, count) = (filling.highlighted, filling.active, filling.shown.len());
        if !back && self.wants_pick() {
            self.pick_choice(highlighted, window, cx);
            return true;
        }
        if count > 0 {
            let next = if back { (active + count - 1) % count } else { (active + 1) % count };
            self.activate_option(next, window, cx);
        }
        true
    }

    /// Enter: picks the highlighted command or choice, else runs the command.
    pub(crate) fn on_command_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.command_picker.open {
            return self.pick_selected_command(window, cx);
        }
        let Some(highlighted) = self.command_picker.filling.as_ref().map(|f| f.highlighted) else { return false };
        match self.wants_pick() {
            true => self.pick_choice(highlighted, window, cx),
            false => self.run_command(window, cx),
        }
        true
    }

    // Typed text that isn't a picked choice yet, with choices to pick from: Tab and Enter take the top one.
    fn wants_pick(&self) -> bool {
        let Some(input) = self.command_picker.filling.as_ref().and_then(Filling::active_input) else { return false };
        !input.text.trim().is_empty() && input.picked.is_none() && !self.option_choices().is_empty()
    }
}

// OPTIONS

impl Waffle {
    /// The composer changed while a command is picked: the text is the active option's value.
    pub(crate) fn on_option_typed(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(filling) = &mut self.command_picker.filling else { return };
        let Some(&i) = filling.shown.get(filling.active) else { return };
        filling.inputs[i] = OptionInput { text, picked: None };
        filling.blocked = None;
        filling.highlighted = 0;
        self.schedule_autocomplete(cx);
        cx.notify();
    }

    fn activate_option(&mut self, position: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(filling) = &mut self.command_picker.filling else { return };
        let Some(&i) = filling.shown.get(position) else { return };
        filling.active = position;
        filling.highlighted = 0;
        filling.suggestions.clear();
        let text = filling.inputs[i].text.clone();
        let end = text.len();
        self.composer.update(cx, |state, cx| {
            state.set_value(text, window, cx);
            state.set_selected_range(end..end, cx);
            state.focus(window, cx);
        });
        self.schedule_autocomplete(cx);
        cx.notify();
    }

    fn add_option(&mut self, option: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(filling) = &mut self.command_picker.filling else { return };
        if !filling.shown.contains(&option) {
            filling.shown.push(option);
        }
        let position = filling.shown.iter().position(|&i| i == option).unwrap_or_default();
        self.activate_option(position, window, cx);
    }

    // Demo scenes fill an option as if typed into its chip, adding the chip if it's optional.
    #[cfg(debug_assertions)]
    pub(crate) fn fill_option(&mut self, name: &str, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(option) = self.command_picker.filling.as_ref().and_then(|f| f.command.options.iter().position(|o| o.name == name)) else { return };
        self.add_option(option, window, cx);
        self.composer.update(cx, |state, cx| {
            state.set_value(text.to_owned(), window, cx);
            state.set_selected_range(text.len()..text.len(), cx);
        });
        self.on_option_typed(text.to_owned(), cx);
    }

    fn move_choice(&mut self, delta: isize, cx: &mut Context<Self>) -> bool {
        let count = self.option_choices().len();
        let Some(filling) = &mut self.command_picker.filling else { return false };
        if count == 0 {
            return false;
        }
        filling.highlighted = (filling.highlighted as isize + delta).rem_euclid(count as isize) as usize;
        cx.notify();
        true
    }

    fn pick_choice(&mut self, choice: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(choice) = self.option_choices().into_iter().nth(choice) else { return };
        let Some(filling) = &mut self.command_picker.filling else { return };
        let Some(&i) = filling.shown.get(filling.active) else { return };
        filling.inputs[i] = OptionInput { text: choice.label, picked: Some(choice.value) };
        filling.blocked = None;
        filling.suggestions.clear();
        match filling.active + 1 < filling.shown.len() {
            true => {
                let next = filling.active + 1;
                self.activate_option(next, window, cx);
            }
            false => {
                let text = filling.inputs[i].text.clone();
                let end = text.len();
                self.composer.update(cx, |state, cx| {
                    state.set_value(text, window, cx);
                    state.set_selected_range(end..end, cx);
                });
                cx.notify();
            }
        }
    }

    /// What the active option offers under the hint bar, filtered by what's typed.
    fn option_choices(&self) -> Vec<OptionChoice> {
        let Some(filling) = &self.command_picker.filling else { return Vec::new() };
        let (Some((_, option)), Some(input)) = (filling.active_option(), filling.active_input()) else { return Vec::new() };
        let typed = input.text.trim();
        if option.autocomplete {
            return filling.suggestions.iter().map(|c| OptionChoice::plain(&c.name, c.value.clone())).collect();
        }
        if !option.choices.is_empty() {
            return option.choices.iter().filter(|c| find_ci(&c.name, typed).is_some()).map(|c| OptionChoice::plain(&c.name, c.value.clone())).collect();
        }
        match option.kind {
            OptionKind::Boolean => [("True", true), ("False", false)].into_iter().filter(|(name, _)| find_ci(name, typed).is_some_and(|r| r.start == 0)).map(|(name, v)| OptionChoice::plain(name, json!(v))).collect(),
            OptionKind::User => self.mention_choices('@', typed),
            OptionKind::Channel => self.mention_choices('#', typed),
            OptionKind::Role => self.role_choices(typed),
            OptionKind::Mentionable => self.mention_choices('@', typed).into_iter().chain(self.role_choices(typed)).collect(),
            _ => Vec::new(),
        }
    }

    // The composer's own @ and # suggestions, sending the id they mention.
    fn mention_choices(&self, lead: char, typed: &str) -> Vec<OptionChoice> {
        let token = typed.rsplit(char::is_whitespace).next().unwrap_or_default();
        let Some(auto) = self.suggest(&format!("{lead}{token}")) else { return Vec::new() };
        auto.items
            .into_iter()
            .map(|s| {
                let id = s.wire.trim_start_matches(['<', '@', '#', '!']).trim_end_matches('>').to_owned();
                // Not verified: user, channel, role and mentionable values go out as snowflake strings.
                OptionChoice { label: s.insert, value: json!(id), lead: ChoiceLead::Suggest(s.lead) }
            })
            .collect()
    }

    fn role_choices(&self, typed: &str) -> Vec<OptionChoice> {
        let Some(guild) = self.guild() else { return Vec::new() };
        let mut roles: Vec<OptionChoice> = guild
            .roles
            .iter()
            .filter(|r| r.id != guild.id && find_ci(&r.name, typed).is_some())
            .map(|r| OptionChoice { label: format!("@{}", r.name), value: json!(r.id), lead: ChoiceLead::Role(r.color) })
            .collect();
        roles.truncate(8);
        roles
    }

    fn schedule_autocomplete(&mut self, cx: &mut Context<Self>) {
        let Some(filling) = &mut self.command_picker.filling else { return };
        if !filling.active_option().is_some_and(|(_, o)| o.autocomplete) {
            filling.debounce = None;
            return;
        }
        filling.debounce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(AUTOCOMPLETE_AFTER).await;
            this.update(cx, |this, cx| this.request_autocomplete(cx)).ok();
        }));
    }

    // A type-4 body straight to Discord: the answer lands in `on_autocomplete`, matched by nonce.
    fn request_autocomplete(&mut self, cx: &mut Context<Self>) {
        let (Some(client), Some((_, channel_id, guild_id))) = (self.client.clone(), self.command_scope()) else { return };
        let session = self.gateway_session.clone();
        let Some(filling) = &mut self.command_picker.filling else { return };
        let Some((focused, option)) = filling.active_option() else { return };
        let name = option.name.clone();
        let values = autocomplete_values(&filling.command.options, &filling.inputs, &filling.shown, focused);
        let nonce = crate::nonce();
        let origin = Origin { channel_id: &channel_id, guild_id: guild_id.as_deref(), session_id: &session, nonce: &nonce };
        let body = app_commands::command_body(&filling.command, &values, Some(&name), &origin);
        filling.awaiting = Some(nonce);
        cx.spawn(async move |_, _| client.interact(body).await.ok()).detach();
    }

    pub(crate) fn on_autocomplete(&mut self, nonce: String, choices: Vec<Choice>, cx: &mut Context<Self>) {
        let Some(filling) = &mut self.command_picker.filling else { return };
        if filling.awaiting.as_deref() != Some(nonce.as_str()) {
            return;
        }
        filling.set_suggestions(choices);
        cx.notify();
    }

    fn run_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, channel_id, guild_id)) = self.command_scope() else { return };
        let Some(filling) = &mut self.command_picker.filling else { return };
        let values = match command_values(&filling.command.options, &filling.inputs, &filling.shown) {
            Ok(values) => values,
            Err((position, why)) => {
                filling.blocked = Some(why);
                let moved = position != filling.active;
                if moved {
                    self.activate_option(position, window, cx);
                }
                return cx.notify();
            }
        };
        let Some(filling) = self.command_picker.filling.take() else { return };
        let nonce = crate::nonce();
        let session = self.gateway_session.clone();
        let origin = Origin { channel_id: &channel_id, guild_id: guild_id.as_deref(), session_id: &session, nonce: &nonce };
        let body = app_commands::command_body(&filling.command, &values, None, &origin);
        self.command_picker.record_run(&filling.command);
        self.interact(nonce, filling.app, InteractionKind::Command, body, cx);
        let placeholder = self.channel.as_ref().map(crate::channel_mention).map(|c| format!("Message {c}")).unwrap_or_default();
        self.composer.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.set_placeholder(placeholder, window, cx);
        });
        self.has_draft = !self.pending_files.is_empty();
        self.drafts.clear(&channel_id);
        cx.notify();
    }
}

// What the active option offers: a fixed choice, an app's suggestion, True/False, a person, channel or role.
struct OptionChoice {
    label: String,
    value: Value,
    lead: ChoiceLead,
}

enum ChoiceLead {
    None,
    Suggest(SuggestLead),
    Role(u32),
}

impl OptionChoice {
    fn plain(label: &str, value: Value) -> Self {
        Self { label: label.to_owned(), value, lead: ChoiceLead::None }
    }
}

// RENDERING

impl Waffle {
    /// The picker or the active option's hint bar, floating above the composer card.
    pub(crate) fn command_popup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.command_picker.open {
            return Some(self.picker_popup(cx));
        }
        self.command_picker.filling.as_ref().map(|f| self.options_popup(f, cx))
    }

    fn picker_popup(&self, cx: &mut Context<Self>) -> AnyElement {
        let body = match self.command_index() {
            None | Some(IndexState::Loading) => loading_rows(cx).into_any_element(),
            Some(IndexState::Failed(error)) => self.index_error(error, cx).into_any_element(),
            Some(IndexState::Ready(index)) if index.commands.is_empty() => picker_note("No apps here have commands", "Apps added to this server show their commands here.", cx).into_any_element(),
            Some(IndexState::Ready(index)) => self.picker_body(index, cx),
        };
        let foot = popup_foot(cx)
            .child(ui::key_hint(div().flex().gap(px(4.)).child(ui::kbd_glyph(Glyph::ArrowUp, cx)).child(ui::kbd_glyph(Glyph::ArrowDown, cx)), "Move", cx))
            .child(ui::key_hint(ui::kbd("Tab", cx), "Pick", cx))
            .child(ui::key_hint(ui::kbd("Esc", cx), "Close", cx));
        popup("command-picker", [body, foot.into_any_element()], cx)
    }

    fn picker_body(&self, index: &CommandIndex, cx: &mut Context<Self>) -> AnyElement {
        let rows = listing(index, &self.command_picker.query, &self.command_picker.recent);
        if rows.is_empty() {
            return picker_note(format!("No commands match /{}", self.command_picker.query), "Try another name, or a word from what it does.", cx).into_any_element();
        }
        let picks = pickable(&rows);
        let chosen = picks.get(self.command_picker.selected).copied();
        let current = chosen.map(|row| section_of(&rows, row));
        let rail = rows.iter().enumerate().filter_map(|(row, r)| match r {
            ListRow::Header(section) => Some(self.rail_tile(index, *section, row, current == Some(*section), &picks, cx)),
            ListRow::Command { .. } => None,
        }).collect::<Vec<_>>();
        let list = rows.iter().enumerate().map(|(row, r)| match r {
            ListRow::Header(section) => section_header(index, *section, cx).into_any_element(),
            ListRow::Command { command, name, description } => {
                let pick = picks.iter().position(|&p| p == row).unwrap_or_default();
                self.command_row(index, *command, name.clone(), description.clone(), chosen == Some(row), pick, cx).into_any_element()
            }
        }).collect::<Vec<_>>();
        div()
            .flex()
            .min_h_0()
            .child(div().w(px(52.)).flex_none().py(px(8.)).flex().flex_col().items_center().gap(px(4.)).border_r_1().border_color(palette::line(cx)).children(rail))
            .child(div().id("command-list").flex_1().min_w_0().max_h(px(LIST_HEIGHT)).p(px(6.)).flex().flex_col().gap(px(2.)).overflow_y_scroll().track_scroll(&self.command_picker.scroll).children(list))
            .into_any_element()
    }

    // A section's tile in the rail; clicking it jumps the list to that section.
    fn rail_tile(&self, index: &CommandIndex, section: Section, row: usize, on: bool, picks: &[usize], cx: &mut Context<Self>) -> AnyElement {
        let (selected, hover) = (palette::selected(cx), palette::hover(cx));
        let (face, name): (AnyElement, SharedString) = match section {
            Section::Recent => (Icon::new(Glyph::Recent).size(px(18.)).into_any_element(), "Recent".into()),
            Section::App(a) => (ui::server_tile(index.apps[a].icon.as_deref(), &index.apps[a].name, 28., cx), index.apps[a].name.clone().into()),
        };
        let first = picks.iter().position(|&p| p > row).unwrap_or_default();
        div()
            .id(SharedString::from(format!("command-rail-{row}")))
            .size(px(36.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .squircle(RADIUS_SM)
            .cursor_pointer()
            .text_color(if on { palette::strong(cx) } else { palette::muted(cx) })
            .when(on, |d| d.bg(selected))
            .when(!on, |d| d.hover(move |s| s.bg(hover)))
            .child(face)
            .tooltip(move |window, cx| ui::tip(name.clone(), window, cx))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.command_picker.selected = first;
                this.command_picker.scroll.scroll_to_item(row);
                cx.notify();
            }))
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn command_row(&self, index: &CommandIndex, command: usize, name: Option<Range<usize>>, description: Option<Range<usize>>, on: bool, pick: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = &index.commands[command];
        let app = index.apps.iter().find(|a| a.id == c.application_id);
        let (strong, muted, faint, accent) = (palette::strong(cx), palette::muted(cx), palette::faint(cx), palette::accent_ink(cx));
        let mark = |range: Option<Range<usize>>, shift: usize| range.map(|r| (r.start + shift..r.end + shift, HighlightStyle { color: Some(accent), ..Default::default() }));
        let args: Vec<&str> = c.options.iter().filter(|o| o.required).map(|o| o.name.as_str()).collect();
        let title = div()
            .flex()
            .items_baseline()
            .gap(px(6.))
            .min_w_0()
            .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).text_color(strong).child(StyledText::new(format!("/{}", c.name())).with_highlights(mark(name, 1))))
            .when(!args.is_empty(), |d| d.child(div().min_w_0().truncate().text_size(px(13.)).text_color(faint).child(args.join(" "))));
        let what = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(title)
            .child(div().truncate().text_size(px(13.)).text_color(muted).child(StyledText::new(c.description.clone()).with_highlights(mark(description, 0))));
        let app_name = app.map(|a| a.name.clone()).unwrap_or_default();
        ui::menu_row(SharedString::from(format!("command-{command}")), cx)
            .highlighted(on)
            .min_h(px(44.))
            .gap(px(12.))
            .child(ui::server_tile(app.and_then(|a| a.icon.as_deref()), &app_name, 32., cx))
            .child(what)
            .child(div().flex_none().text_size(px(12.)).text_color(faint).child(app_name))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.command_picker.selected = pick;
                this.pick_selected_command(window, cx);
            }))
    }

    fn index_error(&self, error: &str, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .p(px(14.))
            .flex()
            .items_center()
            .gap(px(12.))
            .child(div().text_color(palette::danger_ink(cx)).child(Icon::new(Glyph::WarningCircle).size(px(20.))))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child("Couldn't load this server's commands"))
                    .child(div().truncate().text_size(px(13.)).text_color(palette::muted(cx)).child(error.to_owned())),
            )
            .child(ui::button("command-index-retry", ui::Tone::Secondary, cx).small().child("Try again").on_click(cx.listener(|this, _, _, cx| this.fetch_command_index(cx))))
    }

    fn options_popup(&self, filling: &Filling, cx: &mut Context<Self>) -> AnyElement {
        let choices = self.option_choices();
        let list = (!choices.is_empty()).then(|| {
            let rows = choices.into_iter().enumerate().map(|(i, choice)| self.choice_row(i, choice, i == filling.highlighted, cx));
            div().id("command-choices").max_h(px(240.)).p(px(6.)).flex().flex_col().gap(px(2.)).overflow_y_scroll().children(rows)
        });
        let from = filling.active_option().is_some_and(|(_, o)| o.autocomplete && !filling.suggestions.is_empty()).then(|| div().flex_1().truncate().child(format!("Suggestions from {}", filling.app.name)));
        let next = if self.wants_pick() { "Pick" } else { "Next option" };
        let foot = popup_foot(cx)
            .children(from)
            .child(div().flex_1())
            .child(ui::key_hint(ui::kbd("Tab", cx), next, cx))
            .child(ui::key_hint(ui::kbd("Enter", cx), if self.wants_pick() { "Pick" } else { "Run" }, cx));
        let hint = self.hint_bar(filling, cx).into_any_element();
        popup("command-options", [Some(hint), list.map(IntoElement::into_any_element), Some(foot.into_any_element())].into_iter().flatten(), cx)
    }

    // What the active option wants, and the optional ones still to add.
    fn hint_bar(&self, filling: &Filling, cx: &mut Context<Self>) -> Div {
        let (strong, muted, faint, danger) = (palette::strong(cx), palette::muted(cx), palette::faint(cx), palette::danger_ink(cx));
        let optional: Vec<(usize, String)> = filling.command.options.iter().enumerate().filter(|(i, o)| !o.required && !filling.shown.contains(i)).map(|(i, o)| (i, o.name.clone())).collect();
        let add = (!optional.is_empty()).then(|| {
            div().flex_none().flex().items_center().gap(px(6.)).text_size(px(12.)).text_color(faint).child("Optional").children(optional.into_iter().map(|(i, name)| {
                ui::chip(SharedString::from(format!("add-option-{i}")), Some(Glyph::Plus), name, false, cx)
                    .h(px(24.))
                    .px(px(8.))
                    .text_size(px(12.))
                    .on_click(cx.listener(move |this, _, window, cx| this.add_option(i, window, cx)))
            }))
        });
        let head = div().flex().items_center().gap(px(10.)).min_w_0();
        let head = match filling.active_option() {
            Some((_, option)) => head
                .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).text_color(strong).child(option.name.clone()))
                .child(div().flex_1().min_w_0().truncate().text_color(muted).child(option.description.clone()))
                .when(option.required, |d| d.child(div().flex_none().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(danger).child("Required"))),
            None => head.child(div().flex_1().min_w_0().truncate().text_color(muted).child(filling.command.description.clone())),
        };
        let problem = filling.blocked.clone().or_else(|| {
            let (i, option) = filling.active_option()?;
            resolve(option, &filling.inputs[i]).err()
        });
        div()
            .px(px(14.))
            .py(px(10.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .border_b_1()
            .border_color(palette::line(cx))
            .text_size(px(14.))
            .child(head.children(add))
            .children(problem.map(|why| div().flex().items_center().gap(px(6.)).text_size(px(13.)).text_color(danger).child(Icon::new(Glyph::WarningCircle).size(px(16.))).child(why)))
    }

    fn choice_row(&self, i: usize, choice: OptionChoice, on: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let lead = match &choice.lead {
            ChoiceLead::None => None,
            ChoiceLead::Suggest(lead) => Some(crate::chat::suggest_lead(lead, cx)),
            ChoiceLead::Role(color) => Some(div().size(px(28.)).flex_none().flex().items_center().justify_center().child(div().size(px(12.)).rounded_full().bg(role_dot(*color, cx))).into_any_element()),
        };
        ui::menu_row(SharedString::from(format!("command-choice-{i}")), cx)
            .highlighted(on)
            .children(lead)
            .child(div().flex_1().min_w_0().truncate().text_color(palette::strong(cx)).child(choice.label))
            .on_click(cx.listener(move |this, _, window, cx| this.pick_choice(i, window, cx)))
    }

    /// The composer's text area while a command is picked: its chip, then one per option, the active one
    /// holding `field` (the composer's own text area) so typing edits its value.
    pub(crate) fn command_chips(&self, field: AnyElement, cx: &mut Context<Self>) -> Option<Div> {
        let filling = self.command_picker.filling.as_ref()?;
        let accent = palette::accent_ink(cx);
        let command = div()
            .h(px(CHIP_HEIGHT))
            .pl(px(4.))
            .pr(px(10.))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .squircle(RADIUS_XS)
            .bg(palette::accent_soft(cx))
            .text_color(accent)
            .font_weight(FontWeight::SEMIBOLD)
            .child(ui::server_tile(filling.app.icon.as_deref(), &filling.app.name, 20., cx))
            .child(format!("/{}", filling.command.name()));
        let mut field = Some(field);
        let chips: Vec<AnyElement> = filling
            .shown
            .iter()
            .enumerate()
            .map(|(position, &i)| {
                let active = position == filling.active;
                let value = if active { field.take() } else { None };
                self.option_chip(filling, position, i, value, cx)
            })
            .collect();
        // No option to type into: the field still takes keys (Enter runs, Backspace drops the command).
        let trailing = field.map(|f| div().flex_1().min_w(px(40.)).child(f));
        Some(div().flex_1().min_w_0().py(px(3.)).flex().flex_wrap().items_center().gap(px(6.)).child(command).children(chips).children(trailing))
    }

    fn option_chip(&self, filling: &Filling, position: usize, i: usize, field: Option<AnyElement>, cx: &mut Context<Self>) -> AnyElement {
        let option = &filling.command.options[i];
        let input = &filling.inputs[i];
        let active = field.is_some();
        let invalid = resolve(option, input).is_err() && !input.text.is_empty();
        let missing = filling.blocked.is_some() && active;
        let edge = match (active, invalid || missing) {
            (_, true) => palette::danger_ink(cx),
            (true, false) => palette::accent_ink(cx),
            (false, false) => palette::line_strong(cx),
        };
        let key = ui::edge(div(), ui::left(RADIUS_XS - 1.))
            .h_full()
            .px(px(8.))
            .flex()
            .flex_none()
            .items_center()
            .bg(palette::selected(cx))
            .text_size(px(13.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette::muted(cx))
            .child(option.name.clone());
        let value = match field {
            Some(field) => Some(div().flex_1().min_w(px(80.)).px(px(6.)).child(field).into_any_element()),
            None if input.text.is_empty() => None,
            None => Some(div().px(px(8.)).max_w(px(200.)).truncate().text_color(if invalid { palette::danger_ink(cx) } else { palette::strong(cx) }).child(input.text.clone()).into_any_element()),
        };
        div()
            .id(SharedString::from(format!("option-chip-{i}")))
            .h(px(CHIP_HEIGHT))
            .flex()
            .items_center()
            .squircle(RADIUS_XS)
            .border_1()
            .border_color(edge)
            .when(active, |d| d.flex_1().min_w(px(160.)).shadow(vec![ui::shadow(palette::accent_soft(cx), 0., 0., 0., 3., false)]))
            .when(!active, |d| d.flex_none().cursor_pointer().on_click(cx.listener(move |this, _, window, cx| this.activate_option(position, window, cx))))
            .child(key)
            .children(value)
            .into_any_element()
    }
}

fn role_dot(color: u32, cx: &App) -> gpui_kit::Hsla {
    match color {
        0 => palette::muted(cx),
        color => gpui_kit::rgb(color).into(),
    }
}

// The surface both popups share: the composer's width, 8px above it, rising from its foot.
fn popup(id: &'static str, content: impl IntoIterator<Item = AnyElement>, cx: &App) -> AnyElement {
    div()
        .id(id)
        .occlude()
        .absolute()
        .left(px(-1.))
        .right(px(-1.))
        .bottom(relative(1.))
        .mb(px(9.))
        .flex()
        .flex_col()
        .squircle(RADIUS_LG)
        .bg(palette::surface(Level::Overlay, cx))
        .shadow(ui::shadow_pop(cx))
        .text_size(px(14.))
        .children(content)
        .transform_origin(point(relative(0.5), relative(1.)))
        .with_animation(id, Animation::new(ui::DUR_POP).with_easing(gpui_kit::ease_out_quint()), |el, t| el.opacity(t).scale(0.95 + 0.05 * t))
        .into_any_element()
}

fn popup_foot(cx: &App) -> Div {
    div().flex().items_center().gap(px(14.)).px(px(14.)).py(px(8.)).border_t_1().border_color(palette::line(cx)).text_size(px(12.)).text_color(palette::faint(cx))
}

fn section_header(index: &CommandIndex, section: Section, cx: &App) -> Div {
    let header = div().flex_none().flex().items_center().gap(px(8.)).pt(px(8.)).px(px(10.)).pb(px(4.)).text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::muted(cx));
    match section {
        Section::Recent => header.child(Icon::new(Glyph::Recent).size(px(14.))).child("Recent"),
        Section::App(a) => header.child(ui::server_tile(index.apps[a].icon.as_deref(), &index.apps[a].name, 16., cx)).child(index.apps[a].name.clone()),
    }
}

fn loading_rows(cx: &App) -> Div {
    let row = |n: usize, width: f32| {
        div()
            .min_h(px(44.))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(12.))
            .child(ui::skeleton(SharedString::from(format!("command-skeleton-tile-{n}")), RADIUS_SM, cx).size(px(32.)).flex_none())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(ui::skeleton(SharedString::from(format!("command-skeleton-name-{n}")), 4., cx).w(px(width * 0.4)).h(px(12.)))
                    .child(ui::skeleton(SharedString::from(format!("command-skeleton-desc-{n}")), 4., cx).w(px(width)).h(px(10.))),
            )
    };
    div().p(px(6.)).flex().flex_col().gap(px(2.)).child(row(0, 260.)).child(row(1, 200.)).child(row(2, 230.))
}

fn picker_note(title: impl Into<SharedString>, body: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .px(px(16.))
        .py(px(14.))
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(title.into()))
        .child(div().text_size(px(13.)).text_color(palette::muted(cx)).child(body.into()))
}

// LISTING

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Section {
    Recent,
    // An index into `CommandIndex::apps`.
    App(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ListRow {
    Header(Section),
    // `command` indexes `CommandIndex::commands`; the ranges are what matched, in the name and description.
    Command { command: usize, name: Option<Range<usize>>, description: Option<Range<usize>> },
}

fn command_key(command: &AppCommand) -> String {
    format!("{}:{}", command.id, command.name())
}

/// The picker's rows for `query`: Recent (most run first), then each app's commands, best match first.
/// A name match outranks a description match, and a name that starts with the query outranks both.
pub(crate) fn listing(index: &CommandIndex, query: &str, recent: &HashMap<String, u32>) -> Vec<ListRow> {
    let hits: Vec<Hit> = index.commands.iter().enumerate().filter_map(|(i, c)| hit(i, c, query)).collect();
    let mut recents: Vec<(u32, &Hit)> = hits.iter().filter_map(|h| recent.get(&command_key(&index.commands[h.command])).map(|&runs| (runs, h))).collect();
    recents.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.rank.cmp(&b.1.rank)));
    recents.truncate(RECENT_MAX);
    let recent_ids: Vec<usize> = recents.iter().map(|(_, h)| h.command).collect();
    let mut rows = Vec::new();
    if !recents.is_empty() {
        rows.push(ListRow::Header(Section::Recent));
        rows.extend(recents.iter().map(|&(_, h)| h.row()));
    }
    let mut groups: Vec<(u8, usize, Vec<&Hit>)> = index
        .apps
        .iter()
        .enumerate()
        .filter_map(|(a, app)| {
            let mut mine: Vec<&Hit> = hits.iter().filter(|h| index.commands[h.command].application_id == app.id && !recent_ids.contains(&h.command)).collect();
            mine.sort_by_key(|h| (h.rank, h.command));
            Some((mine.first()?.rank, a, mine))
        })
        .collect();
    groups.sort_by_key(|(best, a, _)| (*best, *a));
    for (_, a, mine) in groups {
        rows.push(ListRow::Header(Section::App(a)));
        rows.extend(mine.into_iter().map(Hit::row));
    }
    rows
}

struct Hit {
    command: usize,
    // 0 for a name starting with the query, 1 for one containing it, 2 for a description match.
    rank: u8,
    name: Option<Range<usize>>,
    description: Option<Range<usize>>,
}

impl Hit {
    fn row(&self) -> ListRow {
        ListRow::Command { command: self.command, name: self.name.clone(), description: self.description.clone() }
    }
}

fn hit(command: usize, c: &AppCommand, query: &str) -> Option<Hit> {
    if query.is_empty() {
        return Some(Hit { command, rank: 0, name: None, description: None });
    }
    let name = find_ci(&c.name(), query);
    let description = find_ci(&c.description, query);
    let rank = match (&name, &description) {
        (Some(r), _) if r.start == 0 => 0,
        (Some(_), _) => 1,
        (None, Some(_)) => 2,
        (None, None) => return None,
    };
    Some(Hit { command, rank, name, description })
}

// The row index of every command row, in order: what ↑/↓ step through.
fn pickable(rows: &[ListRow]) -> Vec<usize> {
    rows.iter().enumerate().filter(|(_, r)| matches!(r, ListRow::Command { .. })).map(|(i, _)| i).collect()
}

fn section_of(rows: &[ListRow], row: usize) -> Section {
    rows[..=row]
        .iter()
        .rev()
        .find_map(|r| match r {
            ListRow::Header(s) => Some(*s),
            ListRow::Command { .. } => None,
        })
        .unwrap_or(Section::Recent)
}

/// Where `needle` first occurs in `haystack`, ignoring case, as a byte range of `haystack`.
pub(crate) fn find_ci(haystack: &str, needle: &str) -> Option<Range<usize>> {
    if needle.is_empty() {
        return Some(0..0);
    }
    let fold = |c: char| c.to_lowercase();
    haystack.char_indices().find_map(|(start, _)| {
        let mut rest = haystack[start..].char_indices();
        let mut end = start;
        for n in needle.chars() {
            let (at, h) = rest.next()?;
            if !fold(h).eq(fold(n)) {
                return None;
            }
            end = start + at + h.len_utf8();
        }
        Some(start..end)
    })
}

// VALUES

/// An option's value as Discord takes it: `Ok(None)` when it's left empty, `Err` with why it can't be sent.
pub(crate) fn resolve(option: &CommandOption, input: &OptionInput) -> Result<Option<Value>, String> {
    if option.kind == OptionKind::Attachment {
        return Err("Attachments aren't supported yet".into());
    }
    if let Some(value) = &input.picked {
        return Ok(Some(value.clone()));
    }
    let text = input.text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    if !option.choices.is_empty() {
        return option.choices.iter().find(|c| c.name.eq_ignore_ascii_case(text)).map(|c| Some(c.value.clone())).ok_or_else(|| "Pick one of the choices".into());
    }
    match option.kind {
        OptionKind::Integer => text.parse::<i64>().map(|n| Some(json!(n))).map_err(|_| "Enter a whole number".into()),
        OptionKind::Number => text.parse::<f64>().ok().filter(|n| n.is_finite()).map(|n| Some(json!(n))).ok_or_else(|| "Enter a number".into()),
        OptionKind::Boolean => match text.to_lowercase().as_str() {
            "true" => Ok(Some(json!(true))),
            "false" => Ok(Some(json!(false))),
            _ => Err("Pick True or False".into()),
        },
        OptionKind::User | OptionKind::Channel | OptionKind::Role | OptionKind::Mentionable if text.bytes().all(|b| b.is_ascii_digit()) => Ok(Some(json!(text))),
        OptionKind::User => Err("Pick someone from the list".into()),
        OptionKind::Channel => Err("Pick a channel from the list".into()),
        OptionKind::Role => Err("Pick a role from the list".into()),
        OptionKind::Mentionable => Err("Pick someone or a role from the list".into()),
        OptionKind::String | OptionKind::Attachment => Ok(Some(json!(text))),
    }
}

/// Every shown option's value, or the first one that stops the command from running: its position
/// in `shown` and why.
pub(crate) fn command_values(options: &[CommandOption], inputs: &[OptionInput], shown: &[usize]) -> Result<Vec<(String, Value)>, (usize, String)> {
    let mut values = Vec::new();
    for (position, &i) in shown.iter().enumerate() {
        let option = &options[i];
        match resolve(option, &inputs[i]) {
            Ok(Some(value)) => values.push((option.name.clone(), value)),
            Ok(None) if option.required => return Err((position, format!("Fill in {} to run this command", option.name))),
            Ok(None) => {}
            Err(why) => return Err((position, why)),
        }
    }
    Ok(values)
}

// Not verified: Discord's client sends the focused option's raw text as a string, whatever its type,
// alongside every other option that already has a value.
fn autocomplete_values(options: &[CommandOption], inputs: &[OptionInput], shown: &[usize], focused: usize) -> Vec<(String, Value)> {
    shown
        .iter()
        .filter_map(|&i| match i == focused {
            true => Some((options[i].name.clone(), json!(inputs[i].text))),
            false => resolve(&options[i], &inputs[i]).ok().flatten().map(|v| (options[i].name.clone(), v)),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(id: &str, app: &str, name: &str, description: &str) -> AppCommand {
        AppCommand { id: id.into(), application_id: app.into(), path: name.split(' ').map(str::to_owned).collect(), description: description.into(), ..Default::default() }
    }

    fn option(name: &str, kind: OptionKind, required: bool) -> CommandOption {
        CommandOption { kind, name: name.into(), description: String::new(), required, choices: Vec::new(), autocomplete: false }
    }

    fn index() -> CommandIndex {
        let app = |id: &str, name: &str| app_commands::App { id: id.into(), name: name.into(), icon: None };
        CommandIndex {
            apps: vec![app("w", "Wavebot"), app("p", "Pollster")],
            commands: vec![
                command("1", "w", "play", "Play a song"),
                command("2", "w", "queue", "Songs coming up, and who placed them"),
                command("3", "w", "playlist", "Queue a playlist"),
                command("4", "p", "place-vote", "Vote in a poll"),
                command("5", "p", "poll", "Start a poll"),
            ],
        }
    }

    fn names(index: &CommandIndex, rows: &[ListRow]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                ListRow::Header(Section::Recent) => "#recent".to_owned(),
                ListRow::Header(Section::App(a)) => format!("#{}", index.apps[*a].name),
                ListRow::Command { command, .. } => index.commands[*command].name(),
            })
            .collect()
    }

    #[test]
    fn finds_case_insensitively_by_byte_range() {
        assert_eq!(find_ci("Midnight Arcade", "arc"), Some(9..12));
        assert_eq!(find_ci("Ärger", "är"), Some(0..3));
        assert_eq!(find_ci("play", "x"), None);
    }

    #[test]
    fn lists_recent_first_then_apps_by_best_match() {
        let index = index();
        let recent = HashMap::from([(command_key(&index.commands[0]), 3)]);
        let rows = listing(&index, "pl", &recent);
        assert_eq!(names(&index, &rows), ["#recent", "play", "#Wavebot", "playlist", "queue", "#Pollster", "place-vote"]);
        let queue = rows.iter().find(|r| matches!(r, ListRow::Command { command: 1, .. })).unwrap();
        assert_eq!(*queue, ListRow::Command { command: 1, name: None, description: Some(25..27) });
    }

    #[test]
    fn a_name_prefix_outranks_another_app_order() {
        let index = index();
        let rows = listing(&index, "vote", &HashMap::new());
        assert_eq!(names(&index, &rows), ["#Pollster", "place-vote"]);
        let rows = listing(&index, "po", &HashMap::new());
        assert_eq!(names(&index, &rows), ["#Pollster", "poll", "place-vote"]);
    }

    #[test]
    fn validates_numbers_choices_and_picks() {
        let text = |t: &str| OptionInput { text: t.into(), picked: None };
        assert_eq!(resolve(&option("n", OptionKind::Integer, true), &text("42")), Ok(Some(json!(42))));
        assert!(resolve(&option("n", OptionKind::Integer, true), &text("4.2")).is_err());
        assert_eq!(resolve(&option("n", OptionKind::Number, true), &text("4.5")), Ok(Some(json!(4.5))));
        assert!(resolve(&option("n", OptionKind::Number, true), &text("NaN")).is_err());
        assert_eq!(resolve(&option("b", OptionKind::Boolean, true), &text("TRUE")), Ok(Some(json!(true))));
        assert!(resolve(&option("u", OptionKind::User, true), &text("mira")).is_err());
        let picked = OptionInput { text: "@Mira".into(), picked: Some(json!("8388608")) };
        assert_eq!(resolve(&option("u", OptionKind::User, true), &picked), Ok(Some(json!("8388608"))));
        let mut sized = option("size", OptionKind::String, true);
        sized.choices = vec![Choice { name: "Large".into(), value: json!("l") }];
        assert_eq!(resolve(&sized, &text("large")), Ok(Some(json!("l"))));
        assert!(resolve(&sized, &text("huge")).is_err());
        assert!(resolve(&option("f", OptionKind::Attachment, false), &text("")).is_err());
    }

    #[test]
    fn a_missing_required_option_blocks_and_names_itself() {
        let options = vec![option("query", OptionKind::String, true), option("volume", OptionKind::Integer, false)];
        let inputs = vec![OptionInput::default(), OptionInput { text: "80".into(), picked: None }];
        assert_eq!(command_values(&options, &inputs, &[0, 1]), Err((0, "Fill in query to run this command".into())));
        let inputs = vec![OptionInput { text: "song".into(), picked: None }, OptionInput::default()];
        assert_eq!(command_values(&options, &inputs, &[0, 1]), Ok(vec![("query".into(), json!("song"))]));
    }
}
