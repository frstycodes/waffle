// The `/` picker above the composer, and the hint bar while a command is typed out as
// `/name option:value`. The composer stays plain text: everything here is read back from it.

use std::{collections::HashMap, ops::Range, sync::Arc, time::Duration};

use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, Context, Div, FontWeight, HighlightStyle, IntoElement, ScrollHandle, SharedString, StyledText, Task, Window,
    component::{
        Icon,
        input::{InlineToken, InputContent},
    },
    div, point, prelude::*, px, relative,
};
use serde_json::{Value, json};

use crate::{
    SuggestLead, Waffle,
    app_commands::{self, AppCommand, Choice, CommandIndex, CommandOption, OptionKind, Origin},
    command_line::{self, Arg, ArgValue, Blocked, CommandLine},
    icons::Glyph,
    interactions::InteractionKind,
    palette::{self, Level},
    recent_commands::{self, RecentCommands},
    ui::{self, RADIUS_LG, RADIUS_SM, Squircle},
};

// How long typing pauses before an autocomplete option asks its app for choices.
const AUTOCOMPLETE_AFTER: Duration = Duration::from_millis(250);
const RECENT_MAX: usize = 5;
const LIST_HEIGHT: f32 = 340.;

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
    pub(crate) recent: RecentCommands,
    // The command last picked from the list: when two apps share a name, the text means this one.
    preferred: Option<String>,
    // After Tab: where the option name being typed starts.
    naming: Option<usize>,
    // The row chosen in the hint bar's list (choices, or option names while naming).
    highlighted: usize,
    // What the app suggested, and for which option.
    suggestions: Option<(String, Vec<Choice>)>,
    awaiting: Option<String>,
    debounce: Option<Task<()>>,
    // Why the last Enter didn't run, and the text it was about: shown until the text changes.
    blocked: Option<(SharedString, String)>,
    // Choices picked by label whose value differs: (option, label, value).
    picked: Vec<(String, String, Value)>,
    // Esc hid the hint bar; the next edit brings it back.
    hidden: bool,
    // The options that had a key at the last edit. Only a new key becomes a chip, so undoing a chip sticks.
    keyed: Vec<usize>,
    // Bumped by each Enter that can't run: the composer shakes once per bump.
    pub(crate) shake: usize,
}

pub(crate) enum IndexState {
    Loading,
    Failed(String),
    Ready(Arc<CommandIndex>),
}

impl CommandPicker {
    pub(crate) fn load() -> Self {
        Self { recent: RecentCommands::load(), ..Default::default() }
    }

    pub(crate) fn reset(&mut self) {
        self.open = false;
        self.dismissed = false;
        self.forget_typing();
    }

    // The composer stopped being a command, or ran one.
    fn forget_typing(&mut self) {
        self.naming = None;
        self.suggestions = None;
        self.awaiting = None;
        self.debounce = None;
        self.blocked = None;
        self.picked.clear();
        self.hidden = false;
        self.keyed.clear();
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) fn set_index(&mut self, scope: impl Into<String>, index: CommandIndex) {
        self.indexes.insert(scope.into(), IndexState::Ready(Arc::new(index)));
    }
}

/// An option name's chip in the composer: atomic, so Backspace takes the whole `name:`. Its id is its
/// text, so sending the text as is (a draft, a channel without the index) keeps `name:`.
pub(crate) fn option_token(name: &str) -> InlineToken {
    let text = format!("{name}:");
    InlineToken::new(text.clone(), text).with_label(name.to_owned())
}

pub(crate) fn is_option_token(token: &InlineToken) -> bool {
    token.id() == token.text()
}

/// The composer read as a command: the index it names a command from, the parse and the caret.
struct Typed {
    index: Arc<CommandIndex>,
    line: CommandLine,
    content: InputContent,
    caret: usize,
}

impl Typed {
    fn command(&self) -> &AppCommand {
        &self.index.commands[self.line.command]
    }

    fn text(&self) -> &str {
        self.content.text()
    }

    // The arg whose value the caret is in.
    fn arg(&self) -> Option<(usize, &Arg)> {
        let n = self.line.arg_at(self.caret)?;
        Some((n, &self.line.args[n]))
    }

    fn option(&self, arg: &Arg) -> &CommandOption {
        &self.command().options[arg.option]
    }

    // A mention chip filling the value sends its id; a picked choice still showing its label sends its value.
    fn value(&self, arg: &Arg, picked: &[(String, String, Value)]) -> ArgValue {
        let mention = self.content.tokens().iter().find(|t| t.range() == arg.value && !is_option_token(t.token()));
        if let Some(mention) = mention {
            return ArgValue { text: command_line::mention_id(mention.token().id()).to_owned(), picked: None };
        }
        let text = self.text()[arg.value.clone()].to_owned();
        let name = &self.option(arg).name;
        let picked = picked.iter().find(|(o, label, _)| o == name && *label == text).map(|(_, _, v)| v.clone());
        ArgValue { text, picked }
    }

    fn is_mention(&self, arg: &Arg) -> bool {
        self.content.tokens().iter().any(|t| t.range() == arg.value && !is_option_token(t.token()))
    }

    // The option name typed since Tab, while the caret is still on it.
    fn naming(&self, start: Option<usize>) -> Option<&str> {
        let typed = self.text().get(start?..self.caret)?;
        (!typed.contains(|c: char| c.is_whitespace() || c == ':')).then_some(typed)
    }

    // The options with no key yet that match `query`, required first.
    fn open_options(&self, query: &str) -> Vec<usize> {
        let options = &self.command().options;
        let mut open: Vec<usize> = (0..options.len()).filter(|&i| !self.line.has(i) && find_ci(&options[i].name, query).is_some()).collect();
        open.sort_by_key(|&i| !options[i].required);
        open
    }
}

// PICKER

impl Waffle {
    /// The composer changed: opens, filters or closes the picker over `/query` text.
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
        self.ready_index().map(|index| listing(&index, &self.command_picker.query, self.command_picker.recent.runs())).unwrap_or_default()
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

    /// Writes command `index` (into the open index) as `/name `, then its first required option's key.
    pub(crate) fn pick_command(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(commands) = self.ready_index() else { return };
        let Some(command) = commands.commands.get(index) else { return };
        let text = format!("/{} ", command.name());
        let end = text.len();
        let first = command.options.iter().find(|o| o.required).map(|o| option_token(&o.name));
        let picker = &mut self.command_picker;
        picker.reset();
        picker.preferred = Some(command.id.clone());
        // Nothing required: straight to the list of what can be added.
        picker.naming = (first.is_none() && !command.options.is_empty()).then_some(end);
        self.composer.update(cx, |state, cx| {
            state.set_value(text, window, cx);
            state.set_selected_range(end..end, cx);
            if let Some(first) = first {
                state.replace_with_token(first, window, cx).ok();
            }
            state.focus(window, cx);
        });
        self.on_composer_change(window, cx);
    }

    /// Keys the picker or the hint bar take before the composer: arrows and Esc.
    pub(crate) fn on_command_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
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
        let Some(typed) = self.typed_command(cx).filter(|_| !self.command_picker.hidden) else { return false };
        match key {
            "escape" if self.command_picker.naming.is_some() => self.command_picker.naming = None,
            "escape" => self.command_picker.hidden = true,
            "up" | "down" => return self.move_highlight(&typed, if key == "up" { -1 } else { 1 }, cx),
            _ => return false,
        }
        cx.notify();
        true
    }

    /// Tab: picks the highlighted command, option name or choice, or leaves the value for the option list.
    /// Shift-Tab (`back`) only keeps the text field from outdenting.
    pub(crate) fn on_command_tab(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.command_picker.open {
            return !back && self.pick_selected_command(window, cx);
        }
        let Some(typed) = self.typed_command(cx) else { return false };
        if back {
            return true;
        }
        if self.pick_highlighted(&typed, window, cx) {
            return true;
        }
        self.leave_value(window, cx);
        true
    }

    /// Enter: picks the highlighted command, option name or choice, else runs the command.
    pub(crate) fn on_command_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.command_picker.open {
            return self.pick_selected_command(window, cx);
        }
        let Some(typed) = self.typed_command(cx) else { return false };
        if !self.pick_highlighted(&typed, window, cx) {
            self.run_command(typed, window, cx);
        }
        true
    }

    fn pick_highlighted(&mut self, typed: &Typed, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let highlighted = self.command_picker.highlighted;
        if let Some(query) = typed.naming(self.command_picker.naming) {
            let Some(&option) = typed.open_options(query).get(highlighted) else { return false };
            self.pick_name(option, window, cx);
            return true;
        }
        if !self.wants_pick(typed) {
            return false;
        }
        self.pick_choice(highlighted, window, cx);
        true
    }

    // A typed value that isn't a pick yet, with choices to pick from: Tab and Enter take the highlighted one.
    fn wants_pick(&self, typed: &Typed) -> bool {
        let Some((_, arg)) = typed.arg() else { return false };
        let value = typed.value(arg, &self.command_picker.picked);
        !value.text.trim().is_empty() && value.picked.is_none() && !typed.is_mention(arg) && !self.option_choices(typed).is_empty()
    }
}

// TYPING A COMMAND

impl Waffle {
    fn typed_command(&self, cx: &App) -> Option<Typed> {
        if self.editing.is_some() {
            return None;
        }
        let index = self.ready_index()?;
        let state = self.composer.read(cx);
        let content = state.content();
        let line = command_line::parse(content.text(), &index.commands, self.command_picker.preferred.as_deref())?;
        Some(Typed { index, line, caret: state.cursor(), content })
    }

    pub(crate) fn command_typed(&self, cx: &App) -> bool {
        self.typed_command(cx).is_some()
    }

    /// The composer's text changed: false when it isn't a command. A command's option keys become chips,
    /// and an autocomplete option asks its app again.
    pub(crate) fn on_command_text(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(typed) = self.typed_command(cx) else {
            self.command_picker.forget_typing();
            return false;
        };
        let picker = &mut self.command_picker;
        picker.hidden = false;
        picker.highlighted = 0;
        if picker.blocked.as_ref().is_some_and(|(text, _)| text.as_ref() != typed.text()) {
            picker.blocked = None;
        }
        if typed.naming(picker.naming).is_none() {
            picker.naming = None;
        }
        self.chip_keys(&typed, window, cx);
        self.schedule_autocomplete(cx);
        true
    }

    // Keys just typed or pasted as text become chips, the same as picked ones.
    fn chip_keys(&mut self, typed: &Typed, window: &mut Window, cx: &mut Context<Self>) {
        let keyed = std::mem::replace(&mut self.command_picker.keyed, typed.line.args.iter().map(|a| a.option).collect());
        let chipped = |key: &Range<usize>| typed.content.tokens().iter().any(|t| t.range() == *key);
        let keys: Vec<(Range<usize>, String)> =
            typed.line.args.iter().filter(|a| !keyed.contains(&a.option) && !chipped(&a.key)).map(|a| (a.key.clone(), typed.option(a).name.clone())).collect();
        if keys.is_empty() {
            return;
        }
        let caret = typed.caret;
        self.composer.update(cx, |state, cx| {
            for (key, name) in keys {
                state.replace_range_with_token(key, option_token(&name), window, cx).ok();
            }
            // A chip has its key's length, so the caret goes back where it was.
            state.set_selected_range(caret..caret, cx);
        });
    }

    fn move_highlight(&mut self, typed: &Typed, delta: isize, cx: &mut Context<Self>) -> bool {
        let count = match typed.naming(self.command_picker.naming) {
            Some(query) => typed.open_options(query).len(),
            None => self.option_choices(typed).len(),
        };
        if count == 0 {
            return false;
        }
        let picker = &mut self.command_picker;
        picker.highlighted = (picker.highlighted as isize + delta).rem_euclid(count as isize) as usize;
        cx.notify();
        true
    }

    // Tab in a value: a space after it, and the options not given yet to pick a name from.
    fn leave_value(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(typed) = self.typed_command(cx) else { return };
        if typed.open_options("").is_empty() {
            return;
        }
        let at = typed.arg().map_or(typed.text().len(), |(_, a)| a.value.end);
        let space = !typed.text()[..at].ends_with(char::is_whitespace);
        self.command_picker.naming = Some(at + usize::from(space));
        self.command_picker.highlighted = 0;
        self.composer.update(cx, |state, cx| {
            state.set_selected_range(at..at, cx);
            if space {
                state.insert(" ", window, cx);
            }
        });
        cx.notify();
    }

    // The name typed since Tab becomes `option`'s chip, the caret after it for the value.
    fn pick_name(&mut self, option: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(typed) = self.typed_command(cx) else { return };
        let Some(start) = self.command_picker.naming.take() else { return };
        let token = option_token(&typed.command().options[option].name);
        self.composer.update(cx, |state, cx| {
            state.replace_range_with_token(start..typed.caret, token, window, cx).ok();
        });
        cx.notify();
    }

    // Adds `option`'s chip at the end, after a space: the hint bar's optional chips, or a missing one on Enter.
    fn add_option(&mut self, option: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(typed) = self.typed_command(cx) else { return };
        let token = option_token(&typed.command().options[option].name);
        let end = typed.text().len();
        let space = !typed.text().ends_with(char::is_whitespace);
        self.command_picker.naming = None;
        self.composer.update(cx, |state, cx| {
            state.set_selected_range(end..end, cx);
            if space {
                state.insert(" ", window, cx);
            }
            state.replace_with_token(token, window, cx).ok();
            state.focus(window, cx);
        });
        cx.notify();
    }

    // The highlighted choice replaces the value (a person, channel or role as a mention chip), then on to the
    // next option, as Tab would.
    fn pick_choice(&mut self, choice: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(typed) = self.typed_command(cx) else { return };
        let Some((_, arg)) = typed.arg() else { return };
        let Some(choice) = self.option_choices(&typed).into_iter().nth(choice) else { return };
        let name = typed.option(arg).name.clone();
        let range = arg.key.end..arg.value.end.max(arg.key.end);
        let picked = &mut self.command_picker.picked;
        picked.retain(|(o, _, _)| *o != name);
        if choice.token.is_none() {
            picked.push((name, choice.label.clone(), choice.value));
        }
        self.composer.update(cx, |state, cx| {
            state.set_selected_range(range, cx);
            match choice.token {
                Some(token) => {
                    state.replace_with_token(token, window, cx).ok();
                }
                None => state.replace(choice.label, window, cx),
            }
        });
        self.leave_value(window, cx);
    }

    /// What the caret's option offers under the hint bar, filtered by what's typed.
    fn option_choices(&self, typed: &Typed) -> Vec<OptionChoice> {
        let Some((_, arg)) = typed.arg() else { return Vec::new() };
        let option = typed.option(arg);
        let typed_text = typed.text()[arg.value.clone()].trim();
        if option.autocomplete {
            let suggested = self.command_picker.suggestions.as_ref().filter(|(o, _)| *o == option.name);
            return suggested.into_iter().flat_map(|(_, choices)| choices).map(|c| OptionChoice::plain(&c.name, c.value.clone())).collect();
        }
        if !option.choices.is_empty() {
            return option.choices.iter().filter(|c| find_ci(&c.name, typed_text).is_some()).map(|c| OptionChoice::plain(&c.name, c.value.clone())).collect();
        }
        match option.kind {
            OptionKind::Boolean => [("True", true), ("False", false)].into_iter().filter(|(name, _)| find_ci(name, typed_text).is_some_and(|r| r.start == 0)).map(|(name, v)| OptionChoice::plain(name, json!(v))).collect(),
            OptionKind::User => self.mention_choices('@', typed_text),
            OptionKind::Channel => self.mention_choices('#', typed_text),
            OptionKind::Role => self.role_choices(typed_text),
            OptionKind::Mentionable => self.mention_choices('@', typed_text).into_iter().chain(self.role_choices(typed_text)).collect(),
            _ => Vec::new(),
        }
    }

    // The composer's own @ and # suggestions, picked as the same mention chips.
    fn mention_choices(&self, lead: char, typed: &str) -> Vec<OptionChoice> {
        let token = typed.rsplit(char::is_whitespace).next().unwrap_or_default().trim_start_matches(lead);
        let Some(auto) = self.suggest(&format!("{lead}{token}")) else { return Vec::new() };
        auto.items
            .into_iter()
            .map(|s| {
                // Not verified: user, channel, role and mentionable values go out as snowflake strings.
                let value = json!(command_line::mention_id(&s.wire));
                OptionChoice { token: Some(InlineToken::new(s.wire, s.insert.clone())), label: s.insert, value, lead: ChoiceLead::Suggest(s.lead) }
            })
            .collect()
    }

    fn role_choices(&self, typed: &str) -> Vec<OptionChoice> {
        let Some(guild) = self.guild() else { return Vec::new() };
        let typed = typed.trim_start_matches('@');
        let mut roles: Vec<OptionChoice> = guild
            .roles
            .iter()
            .filter(|r| r.id != guild.id && find_ci(&r.name, typed).is_some())
            .map(|r| {
                let label = format!("@{}", r.name);
                OptionChoice { token: Some(InlineToken::new(format!("<@&{}>", r.id), label.clone())), label, value: json!(r.id), lead: ChoiceLead::Role(r.color) }
            })
            .collect();
        roles.truncate(8);
        roles
    }

    fn schedule_autocomplete(&mut self, cx: &mut Context<Self>) {
        let autocompletes = self.typed_command(cx).is_some_and(|t| t.arg().is_some_and(|(_, a)| t.option(a).autocomplete));
        let picker = &mut self.command_picker;
        if !autocompletes {
            picker.debounce = None;
            return;
        }
        picker.debounce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(AUTOCOMPLETE_AFTER).await;
            this.update(cx, |this, cx| this.request_autocomplete(cx)).ok();
        }));
    }

    // A type-4 body straight to Discord: the answer lands in `on_autocomplete`, matched by nonce.
    fn request_autocomplete(&mut self, cx: &mut Context<Self>) {
        let (Some(client), Some((_, channel_id, guild_id))) = (self.client.clone(), self.command_scope()) else { return };
        let Some(typed) = self.typed_command(cx) else { return };
        let Some((focused, arg)) = typed.arg() else { return };
        let name = typed.option(arg).name.clone();
        let picked = &self.command_picker.picked;
        let values = command_line::autocomplete_values(typed.command(), &typed.line, focused, |a| typed.value(a, picked));
        let nonce = crate::nonce();
        let session = self.gateway_session.clone();
        let origin = Origin { channel_id: &channel_id, guild_id: guild_id.as_deref(), session_id: &session, nonce: &nonce };
        let body = app_commands::command_body(typed.command(), &values, Some(&name), &origin);
        self.command_picker.awaiting = Some(nonce);
        cx.spawn(async move |_, _| client.interact(body).await.ok()).detach();
    }

    pub(crate) fn on_autocomplete(&mut self, nonce: String, choices: Vec<Choice>, cx: &mut Context<Self>) {
        let Some(typed) = self.typed_command(cx) else { return };
        let Some((_, arg)) = typed.arg() else { return };
        let option = typed.option(arg).name.clone();
        let picker = &mut self.command_picker;
        if picker.awaiting.as_deref() != Some(nonce.as_str()) {
            return;
        }
        picker.suggestions = Some((option, choices));
        picker.highlighted = 0;
        cx.notify();
    }

    // Demo scenes stand in for an app's autocomplete answer.
    #[cfg(debug_assertions)]
    pub(crate) fn suggest_for_option(&mut self, option: &str, choices: Vec<Choice>, cx: &mut Context<Self>) {
        self.command_picker.suggestions = Some((option.to_owned(), choices));
        cx.notify();
    }

    fn run_command(&mut self, typed: Typed, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, channel_id, guild_id)) = self.command_scope() else { return };
        let command = typed.command().clone();
        let picked = &self.command_picker.picked;
        let values = match command_line::command_values(&command, &typed.line, |a| typed.value(a, picked)) {
            Ok(values) => values,
            Err(blocked) => return self.block_command(&typed, blocked, window, cx),
        };
        let app = typed.index.apps.iter().find(|a| a.id == command.application_id).cloned();
        let app = app.unwrap_or_else(|| app_commands::App { id: command.application_id.clone(), name: "App".into(), icon: None });
        let nonce = crate::nonce();
        let session = self.gateway_session.clone();
        let origin = Origin { channel_id: &channel_id, guild_id: guild_id.as_deref(), session_id: &session, nonce: &nonce };
        let body = app_commands::command_body(&command, &values, None, &origin);
        self.command_picker.recent.record(&command);
        self.interact(nonce, app, InteractionKind::Command, body, cx);
        self.command_picker.reset();
        self.command_picker.preferred = None;
        self.composer.update(cx, |state, cx| state.set_value("", window, cx));
        self.has_draft = !self.pending_files.is_empty();
        self.drafts.clear(&channel_id);
        self.save_drafts_soon(cx);
        cx.notify();
    }

    // Enter that can't run: the composer shakes and the caret goes where the problem is, a missing option's
    // chip added at the end.
    fn block_command(&mut self, typed: &Typed, blocked: Blocked, window: &mut Window, cx: &mut Context<Self>) {
        let why = blocked.message(typed.command());
        match blocked {
            Blocked::Missing(option) => self.add_option(option, window, cx),
            Blocked::Invalid(n, _) => {
                let at = typed.line.args[n].value.end;
                self.composer.update(cx, |state, cx| state.set_selected_range(at..at, cx));
            }
        }
        let text = self.composer.read(cx).value();
        let picker = &mut self.command_picker;
        picker.blocked = Some((text, why));
        picker.hidden = false;
        picker.naming = None;
        picker.shake += 1;
        cx.notify();
    }
}

// What the caret's option offers: a fixed choice, an app's suggestion, True/False, a person, channel or role.
struct OptionChoice {
    label: String,
    value: Value,
    // People, channels and roles go in as mention chips.
    token: Option<InlineToken>,
    lead: ChoiceLead,
}

enum ChoiceLead {
    None,
    Suggest(SuggestLead),
    Role(u32),
}

impl OptionChoice {
    fn plain(label: &str, value: Value) -> Self {
        Self { label: label.to_owned(), value, token: None, lead: ChoiceLead::None }
    }
}

// RENDERING

impl Waffle {
    /// The picker, or the hint bar for the command being typed, floating above the composer card.
    pub(crate) fn command_popup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.command_picker.open {
            return Some(self.picker_popup(cx));
        }
        let typed = self.typed_command(cx).filter(|_| !self.command_picker.hidden)?;
        Some(self.options_popup(&typed, cx))
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
        let rows = listing(index, &self.command_picker.query, self.command_picker.recent.runs());
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
            Section::App(a) => (app_tile(&index.apps[a], 28., cx), index.apps[a].name.clone().into()),
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
        let app = app.cloned().unwrap_or_else(|| app_commands::App { id: c.application_id.clone(), name: String::new(), icon: None });
        ui::menu_row(SharedString::from(format!("command-{command}")), cx)
            .highlighted(on)
            .min_h(px(44.))
            .gap(px(12.))
            .child(app_tile(&app, 32., cx))
            .child(what)
            .child(div().flex_none().text_size(px(12.)).text_color(faint).child(app.name))
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

    // The hint bar: the option names to add after Tab, or what the caret's option wants and its choices.
    fn options_popup(&self, typed: &Typed, cx: &mut Context<Self>) -> AnyElement {
        if let Some(query) = typed.naming(self.command_picker.naming) {
            return self.names_popup(typed, query, cx);
        }
        let highlighted = self.command_picker.highlighted;
        let choices = self.option_choices(typed);
        let list = (!choices.is_empty()).then(|| {
            let rows = choices.into_iter().enumerate().map(|(i, choice)| self.choice_row(i, choice, i == highlighted, cx));
            div().id("command-choices").max_h(px(240.)).p(px(6.)).flex().flex_col().gap(px(2.)).overflow_y_scroll().children(rows)
        });
        let app = typed.index.apps.iter().find(|a| a.id == typed.command().application_id).map(|a| a.name.clone()).unwrap_or_default();
        let suggesting = typed.arg().is_some_and(|(_, a)| typed.option(a).autocomplete && list.is_some());
        let from = suggesting.then(|| div().flex_1().truncate().child(format!("Suggestions from {app}")));
        let pick = self.wants_pick(typed);
        let foot = popup_foot(cx)
            .children(from)
            .child(div().flex_1())
            .child(ui::key_hint(ui::kbd("Tab", cx), if pick { "Pick" } else { "Next option" }, cx))
            .child(ui::key_hint(ui::kbd("Enter", cx), if pick { "Pick" } else { "Run" }, cx));
        let hint = self.hint_bar(typed, cx).into_any_element();
        popup("command-options", [Some(hint), list.map(IntoElement::into_any_element), Some(foot.into_any_element())].into_iter().flatten(), cx)
    }

    // After Tab: the options still to give, filtered by the name being typed, required first.
    fn names_popup(&self, typed: &Typed, query: &str, cx: &mut Context<Self>) -> AnyElement {
        let options = typed.open_options(query);
        let body = match options.is_empty() {
            true => picker_note(format!("No options match {query}"), "Keep typing to add it to the value instead.", cx).into_any_element(),
            false => {
                let rows = options.into_iter().enumerate().map(|(i, option)| self.name_row(&typed.command().options[option], option, i == self.command_picker.highlighted, cx));
                div().id("command-option-names").max_h(px(240.)).p(px(6.)).flex().flex_col().gap(px(2.)).overflow_y_scroll().children(rows).into_any_element()
            }
        };
        let head = div().px(px(14.)).py(px(10.)).border_b_1().border_color(palette::line(cx)).text_color(palette::muted(cx)).child(format!("Options for /{}", typed.command().name()));
        let foot = popup_foot(cx)
            .child(ui::key_hint(div().flex().gap(px(4.)).child(ui::kbd_glyph(Glyph::ArrowUp, cx)).child(ui::kbd_glyph(Glyph::ArrowDown, cx)), "Move", cx))
            .child(div().flex_1())
            .child(ui::key_hint(ui::kbd("Tab", cx), "Add", cx))
            .child(ui::key_hint(ui::kbd("Esc", cx), "Close", cx));
        popup("command-option-names-popup", [head.into_any_element(), body, foot.into_any_element()], cx)
    }

    fn name_row(&self, option: &CommandOption, i: usize, on: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        ui::menu_row(SharedString::from(format!("command-option-{i}")), cx)
            .highlighted(on)
            .gap(px(10.))
            .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(option.name.clone()))
            .child(div().flex_1().min_w_0().truncate().text_color(palette::muted(cx)).child(option.description.clone()))
            .when(option.required, |d| d.child(required_mark(cx)))
            .on_click(cx.listener(move |this, _, window, cx| this.pick_name(i, window, cx)))
    }

    // What the caret's option wants, and the optional ones still to add.
    fn hint_bar(&self, typed: &Typed, cx: &mut Context<Self>) -> Div {
        let (strong, muted, faint, danger) = (palette::strong(cx), palette::muted(cx), palette::faint(cx), palette::danger_ink(cx));
        let command = typed.command();
        let optional: Vec<(usize, String)> = command.options.iter().enumerate().filter(|(i, o)| !o.required && !typed.line.has(*i)).map(|(i, o)| (i, o.name.clone())).collect();
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
        let head = match typed.arg() {
            Some((_, arg)) => {
                let option = typed.option(arg);
                head.child(div().flex_none().font_weight(FontWeight::SEMIBOLD).text_color(strong).child(option.name.clone()))
                    .child(div().flex_1().min_w_0().truncate().text_color(muted).child(option.description.clone()))
                    .when(option.required, |d| d.child(required_mark(cx)))
            }
            None => head.child(div().flex_1().min_w_0().truncate().text_color(muted).child(command.description.clone())),
        };
        let problem = self.command_picker.blocked.as_ref().map(|(_, why)| why.clone()).or_else(|| {
            let (_, arg) = typed.arg()?;
            command_line::resolve(typed.option(arg), &typed.value(arg, &self.command_picker.picked)).err().filter(|_| !arg.value.is_empty())
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
}

// OPTION BOXES

/// The transparent space an option's name chip leaves before itself, so neighbouring boxes don't touch.
pub(crate) const OPTION_GAP: f32 = 8.;
// How far a box runs past its value's last glyph, into the space before the next option.
const VALUE_PAD: f32 = 5.;
// An empty value still shows room to type in.
const EMPTY_VALUE: f32 = 10.;
const BOX_RADIUS: f32 = 6.;

// One option drawn as a box: its name chip, then its value, as byte offsets into the composer's text.
struct OptionBox {
    start: usize,
    key_end: usize,
    value_end: usize,
    active: bool,
    invalid: bool,
}

impl Waffle {
    /// Each typed option's box, drawn over the composer's text after it lays out: the text field can't
    /// border a text range itself, so this reads where its range landed and strokes around it.
    pub(crate) fn option_boxes(&self, cx: &App) -> Option<impl IntoElement + use<>> {
        let typed = self.typed_command(cx)?;
        let active = typed.naming(self.command_picker.naming).is_none().then(|| typed.line.arg_at(typed.caret)).flatten();
        let picked = &self.command_picker.picked;
        let boxes: Vec<OptionBox> = typed
            .line
            .args
            .iter()
            .enumerate()
            .map(|(n, arg)| {
                let invalid = !arg.value.is_empty() && command_line::resolve(typed.option(arg), &typed.value(arg, picked)).is_err();
                OptionBox { start: arg.key.start, key_end: arg.key.end, value_end: arg.value.end.max(arg.key.end), active: active == Some(n), invalid }
            })
            .collect();
        let composer = self.composer.clone();
        let (line, accent, ring, danger) = (palette::line_strong(cx), palette::accent_ink(cx), palette::accent_soft(cx), palette::danger_ink(cx));
        let paint = move |bounds: gpui_kit::Bounds<gpui_kit::Pixels>, _: (), window: &mut Window, cx: &mut App| {
            let state = composer.read(cx);
            let at = |offset: usize| state.range_to_bounds(&(offset..offset));
            window.with_content_mask(Some(gpui_kit::ContentMask { bounds }), |window| {
                for b in &boxes {
                    let (Some(start), Some(key_end), Some(end)) = (at(b.start), at(b.key_end), at(b.value_end)) else { continue };
                    let right = if b.value_end == b.key_end { key_end.left() + px(EMPTY_VALUE) } else { end.left() + px(VALUE_PAD) };
                    let edge = if b.invalid { danger } else if b.active { accent } else { line };
                    for rect in box_rows(start, right, end, bounds) {
                        if b.active {
                            window.paint_quad(gpui_kit::quad(rect.dilate(px(3.)), gpui_kit::Corners::all(px(BOX_RADIUS + 3.)), gpui_kit::transparent_black(), px(3.), ring, gpui_kit::BorderStyle::Solid));
                        }
                        window.paint_quad(gpui_kit::quad(rect, gpui_kit::Corners::all(px(BOX_RADIUS)), gpui_kit::transparent_black(), px(1.), edge, gpui_kit::BorderStyle::Solid));
                    }
                }
            });
        };
        Some(gpui_kit::canvas(|_, _, _| {}, paint).absolute().inset_0())
    }
}

// An option's box, one rectangle per line it wraps across, a pixel outside its line so the border clears
// the name chip, which starts `OPTION_GAP` into its token.
fn box_rows(start: gpui_kit::Bounds<gpui_kit::Pixels>, right: gpui_kit::Pixels, end: gpui_kit::Bounds<gpui_kit::Pixels>, clip: gpui_kit::Bounds<gpui_kit::Pixels>) -> Vec<gpui_kit::Bounds<gpui_kit::Pixels>> {
    let row = |left: gpui_kit::Pixels, right: gpui_kit::Pixels, top: gpui_kit::Pixels, height: gpui_kit::Pixels| {
        gpui_kit::Bounds::from_corners(point(left - px(1.), top - px(1.)), point(right, top + height + px(1.)))
    };
    let left = start.left() + px(OPTION_GAP);
    let height = start.size.height;
    if (end.top() - start.top()).abs() < height / 2. {
        return vec![row(left, right, start.top(), height)];
    }
    let mut rows = vec![row(left, clip.right(), start.top(), height)];
    let mut top = start.top() + height;
    while top + height / 2. < end.top() {
        rows.push(row(clip.left(), clip.right(), top, height));
        top += height;
    }
    rows.push(row(clip.left(), right, end.top(), height));
    rows
}

fn required_mark(cx: &App) -> Div {
    div().flex_none().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::danger_ink(cx)).child("Required")
}

// An app's icon, or its initial on a colour of its own.
fn app_tile(app: &app_commands::App, size: f32, cx: &App) -> AnyElement {
    ui::app_tile(app.icon.as_deref(), &app.name, &app.id, size, cx)
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
        Section::App(a) => header.child(app_tile(&index.apps[a], 16., cx)).child(index.apps[a].name.clone()),
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

/// The picker's rows for `query`: Recent (most run first), then each app's commands, best match first.
/// A name match outranks a description match, and a name that starts with the query outranks both.
pub(crate) fn listing(index: &CommandIndex, query: &str, recent: &HashMap<String, u32>) -> Vec<ListRow> {
    let hits: Vec<Hit> = index.commands.iter().enumerate().filter_map(|(i, c)| hit(i, c, query)).collect();
    let mut recents: Vec<(u32, &Hit)> = hits.iter().filter_map(|h| recent.get(&recent_commands::key(&index.commands[h.command])).map(|&runs| (runs, h))).collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn command(id: &str, app: &str, name: &str, description: &str) -> AppCommand {
        AppCommand { id: id.into(), application_id: app.into(), path: name.split(' ').map(str::to_owned).collect(), description: description.into(), ..Default::default() }
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
        let recent = HashMap::from([(recent_commands::key(&index.commands[0]), 3)]);
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
    fn an_option_chip_sends_its_own_text() {
        let token = option_token("query");
        assert!(is_option_token(&token));
        assert_eq!((token.text().as_ref(), token.label().as_ref()), ("query:", "query"));
        assert!(!is_option_token(&InlineToken::new("<@42>", "@Ferris")));
    }
}
