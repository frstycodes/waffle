// The `/` picker above the composer, and the hint bar while a command is typed out as
// `/name option:value`. The composer stays text with chips (each box a key chip, its value, an end chip):
// everything here is read back from it.
// Behaviour follows Discord's client (docs/discord-app-commands-re.md, section 1).

use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

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
    ComposerState, SuggestLead, Waffle,
    app_commands::{self, AppCommand, Choice, CommandIndex, CommandOption, OptionKind, Origin},
    attachments::{self, PendingFile},
    command_line::{self, Arg, ArgValue, CommandLine, Mark, Problem},
    drafts::CommandPick,
    icons::Glyph,
    interactions::InteractionKind,
    palette::{self, Level},
    recent_commands::{self, Ranks, RecentCommands},
    send_options,
    ui::{self, RADIUS_LG, RADIUS_SM, Squircle},
};

// Discord's autocomplete debounce: `debounce(…, 500, {leading: true, trailing: true})`.
const AUTOCOMPLETE_DEBOUNCE: Duration = Duration::from_millis(500);
// Discord's skeleton rows while an app's choices load.
const AUTOCOMPLETE_SKELETONS: usize = 5;
// Discord's inline list (`limit: 20`).
const MATCH_LIMIT: usize = 20;
const LIST_HEIGHT: f32 = 340.;
// An attachment option's chip id: this prefix, then the option's name.
const FILE_TOKEN: &str = "file:";

#[derive(Default)]
pub(crate) struct CommandPicker {
    // By server, or by channel in a DM: the index Discord sent for it.
    indexes: HashMap<String, IndexState>,
    // Scopes whose index went out of date (a gateway reconnect, a bad-version error): refetched on next use.
    stale: HashSet<String>,
    open: bool,
    // Esc closed the picker over this `/` text; it stays closed until the text stops being a command.
    dismissed: bool,
    query: String,
    // Which command row is chosen, counting command rows only.
    selected: usize,
    // The browser's rail narrowed the list to one app (an index into `CommandIndex::apps`).
    filter: Option<usize>,
    scroll: ScrollHandle,
    pub(crate) recent: RecentCommands,
    // Discord's synced frecency was asked for this session.
    frecency_requested: bool,
    // The command last picked from the list: when two apps share a name, the text means this one.
    preferred: Option<String>,
    // The row chosen in the hint bar's list (choices, or option names outside the boxes).
    highlighted: usize,
    // Discord's option-name list focuses no row until a name is typed (`AUTO_WHEN_FILTERED`): Tab or an
    // arrow chooses one, and Enter meanwhile runs the command.
    names_chosen: bool,
    autocomplete: Autocomplete,
    // Options the last Enter found missing or bad: their boxes stay red until they're fixed.
    errored: Vec<usize>,
    // Choices picked by label, kept with the draft.
    picked: Vec<CommandPick>,
    // Files attached to ATTACHMENT options, by option name.
    files: Vec<(String, PendingFile)>,
    // Uploading a run's files: Enter waits.
    staging: bool,
    // Esc hid the hint bar; the next edit brings it back.
    hidden: bool,
    // The options that had a key at the last edit. Only a new key becomes a chip, so undoing a chip sticks.
    keyed: Vec<usize>,
    // The command the text last became (its required boxes added), and whether its lone optional option
    // already took the text typed after it.
    activated: Option<usize>,
    shortcut_taken: bool,
    // The composer's selection as last seen, and the option the caret was in then.
    selection: Range<usize>,
    focused: Option<usize>,
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
        self.filter = None;
        self.preferred = None;
        self.forget_typing();
    }

    /// A restored draft's command: the one picked from the list, and the choices picked for its options.
    pub(crate) fn restore(&mut self, command: Option<String>, picks: Vec<CommandPick>) {
        self.reset();
        self.preferred = command;
        self.picked = picks;
    }

    // The composer stopped being a command, or ran one.
    fn forget_typing(&mut self) {
        self.autocomplete = Autocomplete::default();
        self.errored.clear();
        self.picked.clear();
        self.files.clear();
        self.staging = false;
        self.hidden = false;
        self.keyed.clear();
        self.activated = None;
        self.shortcut_taken = false;
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    /// Discord marks every index stale on each gateway (re)connect and after a bad-version error.
    pub(crate) fn mark_indexes_stale(&mut self) {
        self.stale = self.indexes.keys().cloned().collect();
    }

    pub(crate) fn set_index(&mut self, scope: impl Into<String>, index: CommandIndex) {
        self.indexes.insert(scope.into(), IndexState::Ready(Arc::new(index)));
    }
}

/// An option name's chip in the composer, the start of its box: atomic, so the whole `name:` goes at
/// once. Its id is its text, so sending the text as is (a channel without the index) keeps `name:`.
pub(crate) fn option_token(key: &str) -> InlineToken {
    InlineToken::new(key.to_owned(), key.to_owned())
}

pub(crate) fn is_option_token(token: &InlineToken) -> bool {
    token.id() == token.text()
}

const END_TOKEN: &str = "option-end";

/// The chip closing an option's value, so text typed past it is outside the box. Its text is a space:
/// sent or saved as text, the command still reads `/name a:1 b:2`.
pub(crate) fn end_token() -> InlineToken {
    InlineToken::new(END_TOKEN, " ")
}

pub(crate) fn is_end_token(token: &InlineToken) -> bool {
    token.id() == END_TOKEN
}

fn key_text(option: &CommandOption) -> String {
    format!("{}:", option.display_name())
}

// The composer's key and end chips, as the parser reads boxes from them.
fn marks(content: &InputContent) -> Vec<(Range<usize>, Mark)> {
    let mark = |t: &InlineToken| match (is_option_token(t), is_end_token(t)) {
        (true, _) => Some(Mark::Key),
        (_, true) => Some(Mark::End),
        _ => None,
    };
    content.tokens().iter().filter_map(|span| Some((span.range(), mark(span.token())?))).collect()
}

// `head` then an empty box for each key, each a key chip and an end chip: Discord's pills for the
// options a command starts with.
fn with_boxes(head: &str, keys: &[String]) -> InputContent {
    let text = keys.iter().fold(head.to_owned(), |text, key| text + key + " ");
    let mut at = head.len();
    keys.iter().fold(InputContent::new(text), |content, key| {
        let key_range = at..at + key.len();
        at = key_range.end + 1;
        content.with_token(key_range.clone(), option_token(key)).and_then(|c| c.with_token(key_range.end..at, end_token())).expect("chips match the text they were built with")
    })
}

// Empty boxes for `keys` written into the composer at `at`.
fn insert_boxes(state: &mut ComposerState, at: usize, keys: &[String], window: &mut Window, cx: &mut Context<ComposerState>) {
    let mut at = at;
    for key in keys {
        for token in [option_token(key), end_token()] {
            let len = token.text().len();
            state.replace_range_with_token(at..at, token, window, cx).ok();
            at += len;
        }
    }
}

// Where the caret lands once each range is replaced by text of the given length: shifted by the edits
// before it, and pulled to the start of one it sits inside or at the end of.
fn moved_caret(caret: usize, edits: impl Iterator<Item = (Range<usize>, usize)>) -> usize {
    let (mut at, mut shift) = (caret, 0isize);
    for (range, len) in edits {
        if caret > range.end {
            shift += len as isize - range.len() as isize;
        } else if caret > range.start {
            at = range.start;
        }
    }
    at.saturating_add_signed(shift)
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

    // A chip filling the value: a mention, or an attachment's file.
    fn value_token(&self, arg: &Arg) -> Option<&InlineToken> {
        self.content.tokens().iter().find(|t| t.range() == arg.value && !is_option_token(t.token())).map(|t| t.token())
    }

    // Discord's option-name list shows while the caret is outside every box, past the command name. This
    // is where the name typed there starts, and the name so far.
    fn naming(&self) -> Option<(usize, &str)> {
        let query = self.line.name_query(self.text(), self.caret)?;
        Some((query.start, &self.text()[query]))
    }

    // Discord's filter: options with no value yet whose shown name starts with what's typed (case
    // matters), required first.
    fn open_options(&self, query: &str) -> Vec<usize> {
        let options = &self.command().options;
        let mut open: Vec<usize> = (0..options.len()).filter(|&i| !self.line.has(i) && options[i].display_name().starts_with(query)).collect();
        open.sort_by_key(|&i| !options[i].required);
        open
    }
}

// PICKER

impl Waffle {
    /// The composer changed: opens, filters or closes the picker over `/query` text. With a space the
    /// query only stays a query while it names no command yet (`/mu q` finds `music queue`).
    pub(crate) fn update_command_picker(&mut self, text: &str, cx: &mut Context<Self>) {
        let names_command = self.typed_command(cx).is_some();
        let picker = &mut self.command_picker;
        let query = text.strip_prefix('/').filter(|q| self.editing.is_none() && (!q.contains(char::is_whitespace) || !names_command));
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
        if !query.is_empty() {
            picker.filter = None;
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
        self.ensure_command_frecency(cx);
        let Some((key, _, _)) = self.command_scope() else { return };
        if !self.command_picker.indexes.contains_key(&key) || self.command_picker.stale.remove(&key) {
            self.fetch_command_index(cx);
        }
    }

    // Discord's synced command and app frecency, once a session. Until it lands, or if it never
    // does, the history kept from runs here ranks the picker.
    fn ensure_command_frecency(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone().filter(|_| !self.command_picker.frecency_requested) else { return };
        self.command_picker.frecency_requested = true;
        cx.spawn(async move |this, cx| {
            let synced = client.frecency_settings().await.and_then(|proto| recent_commands::decode(&proto).ok_or_else(|| "Unreadable frecency settings".to_owned()));
            this.update(cx, |this, cx| match synced {
                Ok(history) => {
                    this.command_picker.recent.sync(history);
                    cx.notify();
                }
                Err(error) => eprintln!("command frecency: {error}; ranking by this device's runs"),
            })
            .ok();
        })
        .detach();
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

    fn command_ranks(&self, index: &CommandIndex) -> Ranks {
        self.command_picker.recent.ranks(index, self.guild_id.as_deref(), now_ms())
    }

    fn command_rows(&self) -> Vec<ListRow> {
        let Some(index) = self.ready_index() else { return Vec::new() };
        let picker = &self.command_picker;
        let rows = listing(&index, &picker.query, &self.command_ranks(&index), picker.filter);
        with_built_ins(rows, &picker.query, picker.filter, self.tts_allowed())
    }

    // The picker shows unless a query with spaces matches nothing: then it's just text, as in Discord.
    fn picker_visible(&self) -> bool {
        let picker = &self.command_picker;
        picker.open && !(picker.query.contains(char::is_whitespace) && self.ready_index().is_some() && self.command_rows().is_empty())
    }

    // The browser clamps at its ends (Discord's `web#989161`); the typed list wraps.
    // Not verified: whether Discord's inline list wraps.
    fn move_command(&mut self, delta: isize, cx: &mut Context<Self>) {
        let rows = self.command_rows();
        let picks = pickable(&rows);
        if picks.is_empty() {
            return;
        }
        let picker = &mut self.command_picker;
        let last = picks.len() as isize - 1;
        let next = picker.selected as isize + delta;
        picker.selected = match picker.query.is_empty() {
            true => next.clamp(0, last),
            false => next.rem_euclid(last + 1),
        } as usize;
        picker.scroll.scroll_to_item(picks[picker.selected]);
        cx.notify();
    }

    fn pick_selected_command(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let rows = self.command_rows();
        let Some(&row) = pickable(&rows).get(self.command_picker.selected) else { return false };
        match rows[row] {
            ListRow::Command { command, .. } => self.pick_command(command, window, cx),
            ListRow::Tts { .. } => self.pick_tts(window, cx),
            ListRow::Header(_) => return false,
        }
        true
    }

    // /tts goes in as the typed prefix `send_options::parse` reads on send; the picker stays closed over it.
    fn pick_tts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let head = format!("{} ", send_options::TTS);
        let picker = &mut self.command_picker;
        picker.reset();
        picker.dismissed = true;
        self.composer.update(cx, |state, cx| {
            state.set_value(head.clone(), window, cx);
            state.set_selected_range(head.len()..head.len(), cx);
            state.focus(window, cx);
        });
        self.on_composer_change(window, cx);
    }

    /// Writes command `index` (into the open index) as `/name ` and a box for every required option, as
    /// Discord does, the caret in the first.
    pub(crate) fn pick_command(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(commands) = self.ready_index() else { return };
        let Some(command) = commands.commands.get(index) else { return };
        let keys: Vec<String> = command.options.iter().filter(|o| o.required).map(key_text).collect();
        let head = format!("/{} ", command.display_name());
        let caret = head.len() + keys.first().map_or(0, String::len);
        let content = with_boxes(&head, &keys);
        let picker = &mut self.command_picker;
        picker.reset();
        picker.preferred = Some(command.id.clone());
        picker.activated = Some(index);
        self.composer.update(cx, |state, cx| {
            state.set_value(content, window, cx);
            state.set_selected_range(caret..caret, cx);
            state.focus(window, cx);
        });
        self.on_composer_change(window, cx);
    }

    /// Keys the picker or the hint bar take before the composer: arrows, Ctrl-P/N and Esc.
    pub(crate) fn on_command_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        if self.picker_visible() {
            match key {
                "up" | "ctrl-p" => self.move_command(-1, cx),
                "down" | "ctrl-n" => self.move_command(1, cx),
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
            "escape" => self.command_picker.hidden = true,
            "up" | "ctrl-p" => return self.move_highlight(&typed, -1, cx),
            "down" | "ctrl-n" => return self.move_highlight(&typed, 1, cx),
            _ => return false,
        }
        cx.notify();
        true
    }

    /// Tab: the open list takes it first (a command, option name or choice); otherwise Tab and Shift-Tab
    /// (`back`) select the next or previous box's whole value, as Discord's `select(editor, path)` does.
    /// Past the last box Tab puts the caret at the end of the text; before the first, Shift-Tab puts it
    /// after the command name.
    pub(crate) fn on_command_tab(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.picker_visible() {
            return !back && self.pick_selected_command(window, cx);
        }
        let Some(typed) = self.typed_command(cx) else { return false };
        if !back && self.pick_highlighted(&typed, false, window, cx) {
            return true;
        }
        let target = if back { typed.line.previous_arg(typed.caret) } else { typed.line.next_arg(typed.caret) };
        match target {
            Some(n) => self.select_box(typed.line.args[n].inner(), cx),
            None if !back => self.caret_to_end(window, cx),
            None if typed.caret > typed.line.name_end => self.select_box(typed.line.name_end..typed.line.name_end, cx),
            None => {}
        }
        true
    }

    /// Enter: picks the highlighted command, option name or choice, else runs the command.
    pub(crate) fn on_command_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.picker_visible() {
            return self.pick_selected_command(window, cx);
        }
        let Some(typed) = self.typed_command(cx) else { return false };
        if !self.pick_highlighted(&typed, true, window, cx) && !self.command_picker.staging {
            self.run_command(typed, window, cx);
        }
        true
    }

    fn pick_highlighted(&mut self, typed: &Typed, enter: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.command_picker.hidden {
            return false;
        }
        let highlighted = self.command_picker.highlighted;
        if let Some((_, query)) = typed.naming() {
            let open = typed.open_options(query);
            if open.is_empty() || (enter && !self.names_on(query)) {
                return false;
            }
            if !self.names_on(query) {
                self.command_picker.names_chosen = true;
                self.command_picker.highlighted = 0;
                cx.notify();
                return true;
            }
            let Some(&option) = open.get(highlighted) else { return false };
            self.pick_name(option, window, cx);
            return true;
        }
        if !self.wants_pick(typed) {
            return false;
        }
        self.pick_choice(highlighted, window, cx);
        true
    }

    // Whether the option-name list has a row chosen.
    fn names_on(&self, query: &str) -> bool {
        !query.is_empty() || self.command_picker.names_chosen
    }

    // The caret's option lists something to pick that its value isn't yet: Tab and Enter take it.
    fn wants_pick(&self, typed: &Typed) -> bool {
        let Some((_, arg)) = typed.arg() else { return false };
        let option = typed.option(arg);
        let value = self.arg_value(typed, arg);
        if self.option_choices(typed).is_empty() || value.picked.is_some() || typed.value_token(arg).is_some() {
            return false;
        }
        match option.kind {
            OptionKind::Attachment => true,
            OptionKind::User | OptionKind::Channel | OptionKind::Role | OptionKind::Mentionable => !value.text.trim().is_empty() && command_line::resolve(option, &value).is_err(),
            _ => option.autocomplete || !matches!(command_line::resolve(option, &value), Ok(Some(_))),
        }
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
        let line = command_line::parse(content.text(), &marks(&content), &index.commands, self.command_picker.preferred.as_deref())?;
        Some(Typed { index, line, caret: state.cursor(), content })
    }

    pub(crate) fn command_typed(&self, cx: &App) -> bool {
        self.typed_command(cx).is_some()
    }

    /// A restored draft's command; a `/` draft also needs the index to read it as one.
    pub(crate) fn restore_command(&mut self, command: Option<String>, picks: Vec<CommandPick>, cx: &mut Context<Self>) {
        self.command_picker.restore(command, picks);
        if self.composer.read(cx).value().starts_with('/') {
            self.ensure_command_index(cx);
        }
    }

    /// What a draft keeps of the command typed into it, if it is one.
    /// GAP: attached files aren't kept; their chips come back as plain text.
    pub(crate) fn command_draft(&self, cx: &App) -> (Option<String>, Vec<CommandPick>) {
        if !self.command_typed(cx) {
            return (None, Vec::new());
        }
        (self.command_picker.preferred.clone(), self.command_picker.picked.clone())
    }

    // What an option's value holds: a chip's id, a picked choice, an attached file, or typed text, with a
    // typed @name, #name or @everyone resolved to its id.
    fn arg_value(&self, typed: &Typed, arg: &Arg) -> ArgValue {
        let option = typed.option(arg);
        let token = typed.value_token(arg);
        let text = typed.text()[arg.value.clone()].to_owned();
        if option.kind == OptionKind::Attachment {
            let chipped = token.is_some_and(|t| t.id().strip_prefix(FILE_TOKEN) == Some(option.name.as_str()));
            let file = chipped.then(|| self.command_picker.files.iter().position(|(o, _)| *o == option.name)).flatten();
            return ArgValue { text, file, ..Default::default() };
        }
        if let Some(token) = token {
            let id = command_line::mention_id(token.id()).to_owned();
            return ArgValue { channel_type: self.channel_type(&id), text: id, ..Default::default() };
        }
        let picked = self.command_picker.picked.iter().find(|p| p.option == option.name && p.label == text).map(|p| p.value.clone());
        let resolved = picked.is_none().then(|| self.resolve_name(option.kind, text.trim())).flatten();
        let text = resolved.unwrap_or(text);
        ArgValue { channel_type: self.channel_type(&text), text, picked, file: None }
    }

    // Discord resolves typed text to a mention for people, channels and roles.
    // Not verified: its matching rules; this takes an exact name, ignoring case.
    fn resolve_name(&self, kind: OptionKind, text: &str) -> Option<String> {
        let guild = self.guild();
        let roles = || guild.into_iter().flat_map(|g| g.roles.iter());
        let role = |name: &str| match name {
            "@everyone" => guild.map(|g| g.id.clone()),
            _ => roles().find(|r| r.name.eq_ignore_ascii_case(name.trim_start_matches('@'))).map(|r| r.id.clone()),
        };
        let user = |name: &str| {
            let name = name.trim_start_matches('@');
            let members = self.guild_id.as_ref().and_then(|g| self.members.get(g)).into_iter().flat_map(|m| m.values());
            let mut people = members.chain(self.channel.iter().flat_map(|c| c.recipients.iter()));
            people.find(|u| u.username.eq_ignore_ascii_case(name) || u.name.eq_ignore_ascii_case(name)).map(|u| u.id.clone())
        };
        match kind {
            OptionKind::Role => role(text),
            OptionKind::User => user(text),
            OptionKind::Mentionable => role(text).or_else(|| user(text)),
            OptionKind::Channel => {
                let name = text.trim_start_matches('#');
                guild.into_iter().flat_map(|g| g.channels.iter().chain(&g.threads)).find(|c| c.name.eq_ignore_ascii_case(name)).map(|c| c.id.clone())
            }
            _ => None,
        }
    }

    fn channel_type(&self, id: &str) -> Option<u8> {
        self.guild()?.channels.iter().chain(&self.guild()?.threads).find(|c| c.id == id).map(|c| c.raw_type)
    }

    /// The caret or selection moved, typing or not: the hint bar and the boxes follow it, and an
    /// autocomplete option it lands in asks its app for choices. A box the caret leaves empty turns red,
    /// as Discord validates the option it leaves with `allowEmptyValues: false`.
    pub(crate) fn on_composer_selection(&mut self, cx: &mut Context<Self>) {
        let selection = self.composer.read(cx).selected_range();
        if self.command_picker.selection == selection {
            return;
        }
        self.command_picker.selection = selection;
        self.command_picker.names_chosen = false;
        let Some(typed) = self.typed_command(cx) else { return };
        let focused = typed.arg().map(|(_, a)| a.option);
        if self.command_picker.focused != focused {
            let left = self.command_picker.focused.and_then(|o| typed.line.args.iter().find(|a| a.option == o));
            if let Some(left) = left.filter(|a| matches!(command_line::resolve(typed.option(a), &self.arg_value(&typed, a)), Ok(None)))
                && !self.command_picker.errored.contains(&left.option)
            {
                self.command_picker.errored.push(left.option);
            }
            self.command_picker.focused = focused;
            self.command_picker.highlighted = 0;
            self.schedule_autocomplete(cx);
        }
        cx.notify();
    }

    /// The composer's text changed: false when it isn't a command. Option keys become chips, a command
    /// just typed gets its required boxes, and an autocomplete option asks its app again.
    pub(crate) fn on_command_text(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(typed) = self.typed_command(cx) else {
            self.command_picker.forget_typing();
            return false;
        };
        let picker = &mut self.command_picker;
        picker.hidden = false;
        picker.highlighted = 0;
        picker.names_chosen = false;
        self.box_new_keys(&typed, window, cx);
        if self.activate(&typed, window, cx) {
            return true;
        }
        self.keep_errors(&typed);
        self.schedule_autocomplete(cx);
        true
    }

    // A key typed or pasted as text becomes a box, as Discord's `pY` turns `name:` into a pill: the key a
    // chip, and an end chip after its value in place of the spaces trailing it. A caret in those spaces
    // lands at the value's end, inside. Only a newly keyed option is boxed, so undoing a box sticks.
    fn box_new_keys(&mut self, typed: &Typed, window: &mut Window, cx: &mut Context<Self>) {
        let keyed = std::mem::replace(&mut self.command_picker.keyed, typed.line.args.iter().map(|a| a.option).collect());
        let chipped = |key: &Range<usize>| typed.content.tokens().iter().any(|t| t.range() == *key);
        let mut edits: Vec<(Range<usize>, InlineToken)> = Vec::new();
        for arg in typed.line.args.iter().filter(|a| !keyed.contains(&a.option)) {
            if !chipped(&arg.key) {
                edits.push((arg.key.clone(), option_token(&typed.text()[arg.key.clone()])));
            }
            if arg.end.is_none() {
                edits.push((arg.value.end..arg.extent.end, end_token()));
            }
        }
        if edits.is_empty() {
            return;
        }
        let ends = edits.iter().filter(|(_, token)| is_end_token(token)).map(|(range, token)| (range.clone(), token.text().len()));
        let caret = moved_caret(typed.caret, ends);
        edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
        self.composer.update(cx, |state, cx| {
            for (range, token) in edits {
                state.replace_range_with_token(range, token, window, cx).ok();
            }
            state.set_selected_range(caret..caret, cx);
        });
    }

    // The text just became a command (`/name ` typed by hand): Discord adds every required option's
    // box, or, for a command whose one option is optional, turns the text typed after it into that
    // option's value. True when it edited the text (the edit comes back as another change).
    fn activate(&mut self, typed: &Typed, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let after = &typed.text()[typed.line.name_end..];
        if after.is_empty() {
            return false;
        }
        let picker = &mut self.command_picker;
        if picker.activated != Some(typed.line.command) {
            picker.activated = Some(typed.line.command);
            picker.shortcut_taken = false;
            let keys: Vec<String> = typed.command().options.iter().filter(|o| o.required).map(key_text).collect();
            if typed.line.args.is_empty() && after.trim().is_empty() && typed.caret == typed.text().len() && !keys.is_empty() {
                let end = typed.text().len();
                let caret = end + keys[0].len();
                self.composer.update(cx, |state, cx| {
                    insert_boxes(state, end, &keys, window, cx);
                    state.set_selected_range(caret..caret, cx);
                });
                return true;
            }
        }
        let options = &typed.command().options;
        let lone = matches!(options.as_slice(), [only] if !only.required && only.kind != OptionKind::Attachment);
        if !lone || picker.shortcut_taken || !typed.line.args.is_empty() || after.trim().is_empty() {
            return false;
        }
        picker.shortcut_taken = true;
        let (at, end) = (typed.text().len() - after.trim_start().len(), typed.text().len());
        let key = key_text(&options[0]);
        let caret = typed.caret + key.len();
        self.composer.update(cx, |state, cx| {
            state.replace_range_with_token(end..end, end_token(), window, cx).ok();
            state.replace_range_with_token(at..at, option_token(&key), window, cx).ok();
            state.set_selected_range(caret..caret, cx);
        });
        true
    }

    // An errored box stays red until its value would send.
    fn keep_errors(&mut self, typed: &Typed) {
        let still: Vec<usize> = typed.line.args.iter().filter(|a| self.command_picker.errored.contains(&a.option) && !self.value_sends(typed, a)).map(|a| a.option).collect();
        self.command_picker.errored = still;
    }

    fn value_sends(&self, typed: &Typed, arg: &Arg) -> bool {
        matches!(command_line::resolve(typed.option(arg), &self.arg_value(typed, arg)), Ok(Some(_)))
    }

    // Why the caret's box can't send, as Discord's header says it in place of the description: checked
    // live once it has a value, or, empty, since the caret left it or the last Enter marked it.
    fn box_problem(&self, typed: &Typed, arg: &Arg) -> Option<String> {
        let resolved = command_line::resolve(typed.option(arg), &self.arg_value(typed, arg));
        match resolved {
            Err(why) => Some(why),
            Ok(None) if self.command_picker.errored.contains(&arg.option) => Some(command_line::empty_message(typed.option(arg))),
            _ => None,
        }
    }

    fn move_highlight(&mut self, typed: &Typed, delta: isize, cx: &mut Context<Self>) -> bool {
        let (count, unchosen) = match typed.naming() {
            Some((_, query)) => (typed.open_options(query).len(), !self.names_on(query)),
            None => (self.option_choices(typed).len(), false),
        };
        if count == 0 {
            return false;
        }
        let picker = &mut self.command_picker;
        picker.names_chosen = true;
        picker.highlighted = match unchosen {
            true if delta < 0 => count - 1,
            true => 0,
            false => (picker.highlighted as isize + delta).rem_euclid(count as isize) as usize,
        };
        cx.notify();
        true
    }

    // Tab onto a box selects `range`, its whole value, so typing replaces it.
    fn select_box(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.command_picker.highlighted = 0;
        self.composer.update(cx, |state, cx| state.set_selected_range(range, cx));
        cx.notify();
    }

    // Past the last box, Discord puts the caret at the end of the text, outside every box, where the
    // option-name list shows. A command with no box yet gets the space after its name first.
    fn caret_to_end(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(typed) = self.typed_command(cx) else { return };
        let end = typed.text().len();
        let space = !typed.text().ends_with(char::is_whitespace) && !typed.open_options("").is_empty();
        self.command_picker.highlighted = 0;
        self.composer.update(cx, |state, cx| {
            state.set_selected_range(end..end, cx);
            if space {
                state.insert(" ", window, cx);
            }
        });
        cx.notify();
    }

    // The name typed outside the boxes becomes `option`'s empty box, the caret in it.
    fn pick_name(&mut self, option: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(typed) = self.typed_command(cx) else { return };
        let Some((start, _)) = typed.naming() else { return };
        let key = key_text(&typed.command().options[option]);
        let space = !typed.text()[..start].ends_with(char::is_whitespace);
        let at = start + usize::from(space);
        let caret = typed.caret;
        self.composer.update(cx, |state, cx| {
            state.set_selected_range(start..caret, cx);
            state.replace(if space { " " } else { "" }, window, cx);
            insert_boxes(state, at, std::slice::from_ref(&key), window, cx);
            state.set_selected_range(at + key.len()..at + key.len(), cx);
        });
        cx.notify();
    }

    // Adds `option`'s empty box at the end, the caret in it: the hint bar's optional chips.
    fn add_option(&mut self, option: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(typed) = self.typed_command(cx) else { return };
        let key = key_text(&typed.command().options[option]);
        let end = typed.text().len();
        let space = !typed.text().ends_with(char::is_whitespace);
        let at = end + usize::from(space);
        self.composer.update(cx, |state, cx| {
            state.set_selected_range(end..end, cx);
            if space {
                state.insert(" ", window, cx);
            }
            insert_boxes(state, at, std::slice::from_ref(&key), window, cx);
            state.set_selected_range(at + key.len()..at + key.len(), cx);
            state.focus(window, cx);
        });
        cx.notify();
    }

    // The highlighted choice replaces the value (a person, channel or role as a mention chip), then on to
    // the next box, as Discord does. An attachment's row opens the file picker instead.
    fn pick_choice(&mut self, choice: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(typed) = self.typed_command(cx) else { return };
        let Some((n, arg)) = typed.arg() else { return };
        let Some(choice) = self.option_choices(&typed).into_iter().nth(choice) else { return };
        let name = typed.option(arg).name.clone();
        if choice.attach {
            return self.choose_command_file(name, window, cx);
        }
        let range = arg.inner();
        let picked = &mut self.command_picker.picked;
        picked.retain(|p| p.option != name);
        if choice.token.is_none() {
            picked.push(CommandPick { option: name, label: choice.label.clone(), value: choice.value });
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
        self.next_box_after(n, window, cx);
    }

    fn next_box_after(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(typed) = self.typed_command(cx) else { return };
        match typed.line.args.get(n + 1) {
            Some(next) => self.select_box(next.inner(), cx),
            None => self.caret_to_end(window, cx),
        }
    }

    /// Backspace and Delete (`back`: Backspace) where a box changes what they do, as Discord's `v` does
    /// with its pills: in an empty box, or an attachment's, they remove the box; at a value's edge they
    /// step out of it and act on what's beyond; just outside a box they step into it, deleting its last
    /// (or first) character, or removing it when it's empty. True when handled here; false lets the
    /// composer delete, from a caret this may have moved. A selection is always the composer's.
    pub(crate) fn on_command_delete(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(typed) = self.typed_command(cx) else { return false };
        if !self.composer.read(cx).selected_range().is_empty() {
            return false;
        }
        let caret = typed.caret;
        let removable = |a: &Arg| a.inner().is_empty() || typed.option(a).kind == OptionKind::Attachment;
        let mut beyond = caret;
        if let Some((_, arg)) = typed.arg().filter(|(_, a)| a.end.is_some()) {
            if removable(arg) {
                return self.remove_box(arg.span(), window, cx);
            }
            let inner = arg.inner();
            if caret != if back { inner.start } else { inner.end } {
                return false;
            }
            beyond = if back { arg.key.start } else { arg.span().end };
        }
        let neighbour = typed.line.args.iter().filter(|a| a.end.is_some()).find(|a| if back { a.span().end == beyond } else { a.key.start == beyond });
        let into = match neighbour {
            Some(arg) if removable(arg) => return self.remove_box(arg.span(), window, cx),
            Some(arg) => if back { arg.inner().end } else { arg.inner().start },
            None if !back && beyond == typed.text().len() => return true,
            None => beyond,
        };
        if into != caret {
            self.composer.update(cx, |state, cx| state.set_selected_range(into..into, cx));
        }
        false
    }

    fn remove_box(&mut self, span: Range<usize>, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.composer.update(cx, |state, cx| {
            state.set_selected_range(span, cx);
            state.replace("", window, cx);
        });
        true
    }

    // The composer's own file picker, one file, for an ATTACHMENT option: it fills the option's value
    // as a chip naming the file, within the same upload limit as a message.
    fn choose_command_file(&mut self, option: String, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            this.update_in(cx, |this, window, cx| this.attach_command_file(option, path, window, cx)).ok();
        })
        .detach();
    }

    fn attach_command_file(&mut self, option: String, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(file) = PendingFile::new(path) else { return };
        let limit = crate::discord::upload_limit(self.my_account.premium_type, self.guild().map_or(0, |g| g.premium_tier));
        let mut admitted = Vec::new();
        self.send_error = attachments::admit(&mut admitted, vec![file], limit).map(Into::into);
        let Some(file) = admitted.pop() else { return cx.notify() };
        let Some(typed) = self.typed_command(cx) else { return };
        let Some((n, arg)) = typed.line.args.iter().enumerate().find(|(_, a)| typed.option(a).name == option) else { return };
        let token = InlineToken::new(format!("{FILE_TOKEN}{option}"), file.name.clone());
        let range = arg.inner();
        let picker = &mut self.command_picker;
        picker.files.retain(|(o, _)| *o != option);
        picker.files.push((option, file));
        self.composer.update(cx, |state, cx| {
            state.set_selected_range(range, cx);
            state.replace_with_token(token, window, cx).ok();
            state.focus(window, cx);
        });
        self.next_box_after(n, window, cx);
    }

    /// What the caret's option offers under the hint bar, filtered by what's typed.
    fn option_choices(&self, typed: &Typed) -> Vec<OptionChoice> {
        let Some((_, arg)) = typed.arg() else { return Vec::new() };
        let option = typed.option(arg);
        let typed_text = typed.text()[arg.value.clone()].trim();
        if option.kind == OptionKind::Attachment {
            return match self.arg_value(typed, arg).file {
                Some(_) => Vec::new(),
                None => vec![OptionChoice { attach: true, lead: ChoiceLead::Glyph(Glyph::Upload), ..OptionChoice::plain(ATTACH_FILE, Value::Null) }],
            };
        }
        if option.autocomplete {
            return match self.suggestions(typed) {
                Some(Suggestions::Ready(choices)) => choices.iter().map(|c| OptionChoice::plain(c.display_name(), c.value.clone())).collect(),
                _ => Vec::new(),
            };
        }
        if !option.choices.is_empty() {
            return option.choices.iter().filter(|c| find_ci(c.display_name(), typed_text).is_some()).map(|c| OptionChoice::plain(c.display_name(), c.value.clone())).collect();
        }
        match option.kind {
            OptionKind::Boolean => [("True", true), ("False", false)].into_iter().filter(|(name, _)| find_ci(name, typed_text).is_some_and(|r| r.start == 0)).map(|(name, v)| OptionChoice::plain(name, json!(v))).collect(),
            OptionKind::User => self.mention_choices('@', typed_text),
            OptionKind::Channel => self.channel_choices(option, typed_text),
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
                let value = json!(command_line::mention_id(&s.wire));
                OptionChoice { token: Some(InlineToken::new(s.wire, s.insert.clone())), label: s.insert, value, lead: ChoiceLead::Suggest(s.lead), attach: false }
            })
            .collect()
    }

    // Channels the option takes (`channel_types`), as # suggestions.
    fn channel_choices(&self, option: &CommandOption, typed: &str) -> Vec<OptionChoice> {
        let mut channels = self.mention_choices('#', typed);
        if !option.channel_types.is_empty() {
            channels.retain(|c| c.value.as_str().and_then(|id| self.channel_type(id)).is_none_or(|t| option.channel_types.contains(&t)));
        }
        channels
    }

    // Roles by name, @everyone (sent as the server's id) among them, as Discord offers it.
    fn role_choices(&self, typed: &str) -> Vec<OptionChoice> {
        let Some(guild) = self.guild() else { return Vec::new() };
        let typed = typed.trim_start_matches('@');
        let mut roles: Vec<OptionChoice> = guild
            .roles
            .iter()
            .filter(|r| find_ci(&r.name, typed).is_some())
            .map(|r| {
                let label = if r.name.starts_with('@') { r.name.clone() } else { format!("@{}", r.name) };
                OptionChoice { token: Some(InlineToken::new(format!("<@&{}>", r.id), label.clone())), label, value: json!(r.id), lead: ChoiceLead::Role(r.color), attach: false }
            })
            .collect();
        roles.truncate(8);
        roles
    }

    fn run_command(&mut self, typed: Typed, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, channel_id, _)) = self.command_scope() else { return };
        let command = typed.command().clone();
        let values = match command_line::validate(&command, &typed.line, |a| self.arg_value(&typed, a)) {
            Ok(values) => values,
            Err(problems) => return self.block_command(&typed, problems, window, cx),
        };
        let (values, uploads) = self.staged_values(&command, values);
        let app = typed.index.apps.iter().find(|a| a.id == command.application_id).cloned();
        let app = app.unwrap_or_else(|| app_commands::App { id: command.application_id.clone(), name: "App".into(), icon: None });
        if uploads.is_empty() {
            return self.send_command(command, app, values, Vec::new(), window, cx);
        }
        let Some(client) = self.client.clone() else { return };
        self.command_picker.staging = true;
        self.send_error = None;
        cx.spawn_in(window, async move |this, cx| {
            let staged = client.stage_files(&channel_id, &uploads).await;
            this.update_in(cx, |this, window, cx| {
                this.command_picker.staging = false;
                match staged {
                    Ok(files) => this.send_command(command, app, values, files, window, cx),
                    Err(error) => {
                        this.send_error = Some(format!("Couldn't upload the file: {error}").into());
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    // Attachment values hold an index into the picker's files; on the wire it's the file's index among
    // those this run uploads.
    fn staged_values(&self, command: &AppCommand, mut values: Vec<(String, Value)>) -> (Vec<(String, Value)>, Vec<crate::discord::Upload>) {
        let mut uploads = Vec::new();
        for (name, value) in &mut values {
            let attachment = command.options.iter().any(|o| o.name == *name && o.kind == OptionKind::Attachment);
            let Some((_, file)) = value.as_u64().filter(|_| attachment).and_then(|i| self.command_picker.files.get(i as usize)) else { continue };
            *value = json!(uploads.len());
            uploads.push(file.upload());
        }
        (values, uploads)
    }

    fn send_command(&mut self, command: AppCommand, app: app_commands::App, values: Vec<(String, Value)>, files: Vec<app_commands::CommandAttachment>, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, channel_id, guild_id)) = self.command_scope() else { return };
        let nonce = crate::nonce();
        let session = self.gateway_session.clone();
        let origin = Origin { channel_id: &channel_id, guild_id: guild_id.as_deref(), session_id: &session, nonce: &nonce };
        let mut body = app_commands::command_body(&command, &values, None, &origin);
        app_commands::attach_files(&mut body, &files);
        self.command_picker.recent.record(&command, guild_id.as_deref(), now_ms());
        self.interact(nonce, app, InteractionKind::Command, body, cx);
        self.command_picker.reset();
        self.composer.update(cx, |state, cx| state.set_value("", window, cx));
        self.has_draft = !self.pending_files.is_empty();
        self.drafts.clear(&channel_id);
        self.save_drafts_soon(cx);
        cx.notify();
    }

    // Enter that can't run, as Discord handles it: a box for every missing required option, every bad
    // box marked, the first one's value selected. Discord shakes the window; Waffle shakes the composer.
    fn block_command(&mut self, typed: &Typed, problems: Vec<Problem>, window: &mut Window, cx: &mut Context<Self>) {
        let missing: Vec<String> = problems.iter().filter(|p| p.missing).map(|p| key_text(&typed.command().options[p.option])).collect();
        if !missing.is_empty() {
            let end = typed.text().len();
            let space = !typed.text().ends_with(char::is_whitespace);
            self.composer.update(cx, |state, cx| {
                state.set_selected_range(end..end, cx);
                if space {
                    state.insert(" ", window, cx);
                }
                insert_boxes(state, end + usize::from(space), &missing, window, cx);
            });
        }
        let first = problems[0].option;
        let picker = &mut self.command_picker;
        picker.errored = problems.iter().map(|p| p.option).collect();
        picker.hidden = false;
        picker.shake += 1;
        if let Some(typed) = self.typed_command(cx)
            && let Some(arg) = typed.line.args.iter().find(|a| a.option == first)
        {
            let value = arg.inner();
            self.composer.update(cx, |state, cx| state.set_selected_range(value, cx));
        }
        cx.notify();
    }
}

// AUTOCOMPLETE

// An app's answers for the option the caret is in: Discord caches them per query and drops the cache
// when the other options' values change.
#[derive(Default)]
struct Autocomplete {
    // `command_line::autocomplete_context` the answers belong to.
    context: String,
    answers: HashMap<String, Vec<Choice>>,
    failed: HashSet<String>,
    // Requests in flight by nonce: their context and query.
    awaiting: HashMap<String, (String, String)>,
    debounce: Debounce,
    timer: Option<Task<()>>,
}

impl Autocomplete {
    fn follow(&mut self, context: &str) {
        if self.context != context {
            self.context = context.to_owned();
            self.answers.clear();
            self.failed.clear();
        }
    }
}

/// lodash's `debounce(…, wait, {leading: true, trailing: true})`: the first call fires at once, calls
/// within `wait` of the last one wait, and the end of a pause fires once more if any did.
#[derive(Default, Debug)]
pub(crate) struct Debounce {
    // When the pause ends, while one is running.
    pub until: Option<Instant>,
    // A call came during the pause.
    trailing: bool,
}

impl Debounce {
    /// True when this call fires now.
    pub(crate) fn call(&mut self, now: Instant, wait: Duration) -> bool {
        let pausing = self.until.is_some_and(|until| now < until);
        self.until = Some(now + wait);
        self.trailing = pausing;
        !pausing
    }

    /// The pause ended: true when a call is owed.
    pub(crate) fn expire(&mut self) -> bool {
        self.until = None;
        std::mem::take(&mut self.trailing)
    }
}

enum Suggestions<'a> {
    Loading,
    Failed,
    Ready(&'a [Choice]),
}

// The option the caret is in, when its app autocompletes it: the arg, the query and the cache context.
struct AutocompleteTarget {
    arg: usize,
    query: String,
    context: String,
}

impl Waffle {
    fn autocomplete_target(&self, typed: &Typed) -> Option<AutocompleteTarget> {
        let (n, arg) = typed.arg()?;
        typed.option(arg).autocomplete.then_some(())?;
        let context = command_line::autocomplete_context(typed.command(), &typed.line, n, typed.text());
        Some(AutocompleteTarget { arg: n, query: typed.text()[arg.value.clone()].to_owned(), context })
    }

    fn suggestions(&self, typed: &Typed) -> Option<Suggestions<'_>> {
        let target = self.autocomplete_target(typed)?;
        let ac = &self.command_picker.autocomplete;
        if ac.context != target.context {
            return Some(Suggestions::Loading);
        }
        Some(match ac.answers.get(&target.query) {
            Some(choices) => Suggestions::Ready(choices),
            None if ac.failed.contains(&target.query) => Suggestions::Failed,
            None => Suggestions::Loading,
        })
    }

    fn schedule_autocomplete(&mut self, cx: &mut Context<Self>) {
        let target = self.typed_command(cx).and_then(|t| self.autocomplete_target(&t));
        let ac = &mut self.command_picker.autocomplete;
        let Some(target) = target else {
            ac.timer = None;
            return;
        };
        ac.follow(&target.context);
        let now = Instant::now();
        let fire = ac.debounce.call(now, AUTOCOMPLETE_DEBOUNCE);
        ac.timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(AUTOCOMPLETE_DEBOUNCE).await;
            this.update(cx, |this, cx| {
                if this.command_picker.autocomplete.debounce.expire() {
                    this.request_autocomplete(cx);
                }
            })
            .ok();
        }));
        if fire {
            self.request_autocomplete(cx);
        }
    }

    // A type-4 body straight to Discord, unless the cache has this query: the answer lands in
    // `on_autocomplete`, matched by nonce.
    fn request_autocomplete(&mut self, cx: &mut Context<Self>) {
        let (Some(client), Some((_, channel_id, guild_id))) = (self.client.clone(), self.command_scope()) else { return };
        let Some(typed) = self.typed_command(cx) else { return };
        let Some(target) = self.autocomplete_target(&typed) else { return };
        let ac = &self.command_picker.autocomplete;
        if ac.context == target.context && ac.answers.contains_key(&target.query) {
            return;
        }
        let name = typed.command().options[typed.line.args[target.arg].option].name.clone();
        let values = command_line::autocomplete_values(typed.command(), &typed.line, target.arg, |a| self.arg_value(&typed, a));
        let nonce = crate::nonce();
        let session = self.gateway_session.clone();
        let origin = Origin { channel_id: &channel_id, guild_id: guild_id.as_deref(), session_id: &session, nonce: &nonce };
        let body = app_commands::command_body(typed.command(), &values, Some(&name), &origin);
        let ac = &mut self.command_picker.autocomplete;
        ac.failed.remove(&target.query);
        ac.awaiting.insert(nonce.clone(), (target.context, target.query));
        cx.spawn(async move |this, cx| {
            if client.interact(body).await.is_err() {
                this.update(cx, |this, cx| this.autocomplete_failed(&nonce, cx)).ok();
            }
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn on_autocomplete(&mut self, nonce: String, choices: Vec<Choice>, cx: &mut Context<Self>) {
        let ac = &mut self.command_picker.autocomplete;
        let Some((context, query)) = ac.awaiting.remove(&nonce) else { return };
        if context == ac.context {
            ac.answers.insert(query, choices);
            self.command_picker.highlighted = 0;
        }
        cx.notify();
    }

    /// INTERACTION_FAILURE (or a refused POST) for an autocomplete request: "Loading options failed".
    pub(crate) fn autocomplete_failed(&mut self, nonce: &str, cx: &mut Context<Self>) {
        let ac = &mut self.command_picker.autocomplete;
        let Some((context, query)) = ac.awaiting.remove(nonce) else { return };
        if context == ac.context {
            ac.failed.insert(query);
        }
        cx.notify();
    }

    // Demo scenes stand in for an app's autocomplete answer for the caret's option: `None` fails it.
    #[cfg(debug_assertions)]
    pub(crate) fn suggest_for_option(&mut self, choices: Option<Vec<Choice>>, cx: &mut Context<Self>) {
        let Some(target) = self.typed_command(cx).and_then(|t| self.autocomplete_target(&t)) else { return };
        let ac = &mut self.command_picker.autocomplete;
        ac.timer = None;
        ac.follow(&target.context);
        match choices {
            Some(choices) => {
                ac.answers.insert(target.query, choices);
            }
            None => {
                ac.failed.insert(target.query);
            }
        }
        cx.notify();
    }
}

// Discord's attachment pill reads this until a file is chosen.
const ATTACH_FILE: &str = "Please attach a file";

// What the caret's option offers: a fixed choice, an app's suggestion, True/False, a person, channel or
// role, or a file to attach.
struct OptionChoice {
    label: String,
    value: Value,
    // People, channels and roles go in as mention chips.
    token: Option<InlineToken>,
    lead: ChoiceLead,
    // Picking opens the file picker.
    attach: bool,
}

enum ChoiceLead {
    None,
    Suggest(SuggestLead),
    Role(u32),
    Glyph(Glyph),
}

impl OptionChoice {
    fn plain(label: &str, value: Value) -> Self {
        Self { label: label.to_owned(), value, token: None, lead: ChoiceLead::None, attach: false }
    }
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

// RENDERING

impl Waffle {
    /// The picker, or the hint bar for the command being typed, floating above the composer card.
    pub(crate) fn command_popup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.command_picker.open {
            return self.picker_visible().then(|| self.picker_popup(cx));
        }
        let typed = self.typed_command(cx).filter(|_| !self.command_picker.hidden)?;
        Some(self.options_popup(&typed, cx))
    }

    fn picker_popup(&self, cx: &mut Context<Self>) -> AnyElement {
        let body = match self.command_index() {
            None | Some(IndexState::Loading) => loading_rows(3, cx).into_any_element(),
            Some(IndexState::Failed(error)) => self.index_error(error, cx).into_any_element(),
            Some(IndexState::Ready(index)) if index.commands.is_empty() && self.command_rows().is_empty() => picker_note("No apps here have commands", "Apps added to this server show their commands here.", cx).into_any_element(),
            Some(IndexState::Ready(index)) => self.command_picker_body(index, cx),
        };
        let foot = popup_foot(cx)
            .child(ui::key_hint(ui::row().items_stretch().gap_1().child(ui::kbd_glyph(Glyph::ArrowUp, cx)).child(ui::kbd_glyph(Glyph::ArrowDown, cx)), "Move", cx))
            .child(ui::key_hint(ui::kbd("Tab", cx), "Pick", cx))
            .child(ui::key_hint(ui::kbd("Esc", cx), "Close", cx));
        popup("command-picker", [body, foot.into_any_element()], cx)
    }

    fn command_picker_body(&self, index: &CommandIndex, cx: &mut Context<Self>) -> AnyElement {
        let query = self.command_picker.query.clone();
        let ranks = self.command_ranks(index);
        let rows = self.command_rows();
        if rows.is_empty() {
            return picker_note(format!("No commands match /{query}"), "Try another name, or a word from what it does.", cx).into_any_element();
        }
        let picks = pickable(&rows);
        let chosen = picks.get(self.command_picker.selected).copied();
        let list = rows.iter().enumerate().map(|(row, r)| match r {
            ListRow::Header(section) => section_header(index, *section, &query, cx).into_any_element(),
            ListRow::Command { command, name, description } => {
                let pick = picks.iter().position(|&p| p == row).unwrap_or_default();
                self.command_row(index, *command, name.clone(), description.clone(), chosen == Some(row), pick, cx).into_any_element()
            }
            ListRow::Tts { name } => {
                let pick = picks.iter().position(|&p| p == row).unwrap_or_default();
                self.tts_row(name.clone(), chosen == Some(row), pick, cx).into_any_element()
            }
        });
        let list = ui::col().id("command-list").flex_1().max_h(px(LIST_HEIGHT)).p_1p5().gap_0p5().overflow_y_scroll().track_scroll(&self.command_picker.scroll).children(list);
        if !query.is_empty() {
            return list.into_any_element();
        }
        // The browser: a rail of its sections beside the list.
        let current = self.command_picker.filter.map(Section::App).or_else(|| chosen.map(|row| section_of(&rows, row)));
        let sections = browser_sections(index, &ranks);
        let mut rail: Vec<AnyElement> = Vec::new();
        for section in sections {
            rail.push(self.rail_tile(index, section, current == Some(section), cx));
            if section == Section::Frecency {
                rail.push(div().w_6().h(ui::HAIRLINE).my_0p5().bg(palette::line(cx)).into_any_element());
            }
        }
        ui::row().items_stretch()
            .min_h_0()
            .child(ui::col().w(px(52.)).flex_none().py_2().items_center().gap_1().border_r_1().border_color(palette::line(cx)).children(rail))
            .child(list)
            .into_any_element()
    }

    // A section's tile in the rail: an app narrows the list to it; Recent, or the app already chosen,
    // shows everything again.
    fn rail_tile(&self, index: &CommandIndex, section: Section, on: bool, cx: &mut Context<Self>) -> AnyElement {
        let (selected, hover) = (palette::selected(cx), palette::hover(cx));
        let (face, name): (AnyElement, SharedString) = match section {
            Section::App(a) => (app_tile(&index.apps[a], 28., cx), index.apps[a].name.clone().into()),
            _ => (Icon::new(Glyph::Recent).size(ui::ICON_LG).into_any_element(), "Recent".into()),
        };
        let id = match section {
            Section::App(a) => format!("command-rail-{a}"),
            _ => "command-rail-recent".into(),
        };
        ui::row()
            .id(SharedString::from(id))
            .size_9()
            .flex_none()
            .justify_center()
            .squircle(RADIUS_SM)
            .cursor_pointer()
            .text_color(if on { palette::strong(cx) } else { palette::muted(cx) })
            .when(on, |d| d.bg(selected))
            .when(!on, |d| d.hover(move |s| s.bg(hover)))
            .child(face)
            .tooltip(move |window, cx| ui::tip(name.clone(), window, cx))
            .on_click(cx.listener(move |this, _, _, cx| {
                let picker = &mut this.command_picker;
                picker.filter = match section {
                    Section::App(a) if picker.filter != Some(a) => Some(a),
                    _ => None,
                };
                picker.selected = 0;
                picker.scroll.scroll_to_item(0);
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
        let args: Vec<&str> = c.options.iter().filter(|o| o.required).map(CommandOption::display_name).collect();
        let title = ui::row()
            .items_baseline()
            .gap_1p5()
            .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).text_color(strong).child(StyledText::new(format!("/{}", c.display_name())).with_highlights(mark(name, 1))))
            .when(!args.is_empty(), |d| d.child(ui::line(args.join(" ")).text_size(ui::TEXT_DETAIL).text_color(faint)));
        let what = ui::col()
            .flex_1()
            .child(title)
            .child(div().min_w_0().truncate().child(StyledText::new(c.description.clone()).with_highlights(mark(description, 0))).text_size(ui::TEXT_DETAIL).text_color(muted));
        let app = app.cloned().unwrap_or_else(|| app_commands::App { id: c.application_id.clone(), name: String::new(), icon: None });
        ui::menu_row(SharedString::from(format!("command-{command}")), cx)
            .highlighted(on)
            .min_h_11()
            .gap_3()
            .child(app_tile(&app, 32., cx))
            .child(what)
            .child(div().flex_none().text_size(ui::TEXT_XS).text_color(faint).child(app.name))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.command_picker.selected = pick;
                this.pick_selected_command(window, cx);
            }))
    }

    // Discord's built-in /tts, drawn like an app's command.
    fn tts_row(&self, name: Option<Range<usize>>, on: bool, pick: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let (strong, muted, faint, accent) = (palette::strong(cx), palette::muted(cx), palette::faint(cx), palette::accent_ink(cx));
        let name = name.map(|r| (r.start + 1..r.end + 1, HighlightStyle { color: Some(accent), ..Default::default() }));
        let title = ui::row()
            .items_baseline()
            .gap_1p5()
            .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).text_color(strong).child(StyledText::new(send_options::TTS).with_highlights(name)))
            .child(ui::line("message").text_size(ui::TEXT_DETAIL).text_color(faint));
        let what = ui::col().flex_1().child(title).child(ui::line(send_options::TTS_DESCRIPTION).text_size(ui::TEXT_DETAIL).text_color(muted));
        ui::menu_row("command-tts", cx)
            .highlighted(on)
            .min_h_11()
            .gap_3()
            .child(ui::row().size_8().flex_none().justify_center().child(crate::chat::suggest_lead(&SuggestLead::Command(Glyph::VolumeHigh), cx)))
            .child(what)
            .child(div().flex_none().text_size(ui::TEXT_XS).text_color(faint).child(BUILT_IN))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.command_picker.selected = pick;
                this.pick_selected_command(window, cx);
            }))
    }

    fn index_error(&self, error: &str, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        ui::row()
            .p_3p5()
            .gap_3()
            .child(div().text_color(palette::danger_ink(cx)).child(Icon::new(Glyph::WarningCircle).size(ui::ICON_XL)))
            .child(
                ui::col()
                    .flex_1()
                    .child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child("Couldn't load this server's commands"))
                    .child(ui::line(error.to_owned()).text_size(ui::TEXT_DETAIL).text_color(palette::muted(cx))),
            )
            .child(ui::button("command-index-retry", ui::Tone::Secondary, cx).small().child("Try again").on_click(cx.listener(|this, _, _, cx| this.fetch_command_index(cx))))
    }

    // The hint bar: the option names outside the boxes, or what the caret's option wants and its choices.
    fn options_popup(&self, typed: &Typed, cx: &mut Context<Self>) -> AnyElement {
        if let Some((_, query)) = typed.naming()
            && !typed.open_options(query).is_empty()
        {
            return self.names_popup(typed, query, cx);
        }
        let highlighted = self.command_picker.highlighted;
        let list = match self.suggestions(typed) {
            Some(Suggestions::Loading) => Some(loading_rows(AUTOCOMPLETE_SKELETONS, cx).into_any_element()),
            Some(Suggestions::Failed) => Some(list_note("Loading options failed", cx).into_any_element()),
            Some(Suggestions::Ready([])) => Some(list_note("No options match your search", cx).into_any_element()),
            _ => {
                let choices = self.option_choices(typed);
                (!choices.is_empty()).then(|| {
                    let rows = choices.into_iter().enumerate().map(|(i, choice)| self.choice_row(i, choice, i == highlighted, cx));
                    ui::col().id("command-choices").max_h(px(240.)).p_1p5().gap_0p5().overflow_y_scroll().children(rows).into_any_element()
                })
            }
        };
        let app = typed.index.apps.iter().find(|a| a.id == typed.command().application_id).map(|a| a.name.clone()).unwrap_or_default();
        let suggesting = matches!(self.suggestions(typed), Some(Suggestions::Ready([_, ..])));
        let from = suggesting.then(|| ui::line(format!("Suggestions from {app}")).flex_1());
        let pick = self.wants_pick(typed);
        let foot = popup_foot(cx)
            .children(from)
            .child(div().flex_1())
            .when(self.command_picker.staging, |d| d.child("Uploading…"))
            .child(ui::key_hint(ui::kbd("Tab", cx), if pick { "Pick" } else { "Next option" }, cx))
            .child(ui::key_hint(ui::kbd("Enter", cx), if pick { "Pick" } else { "Run" }, cx));
        let hint = self.hint_bar(typed, cx).into_any_element();
        popup("command-options", [Some(hint), list, Some(foot.into_any_element())].into_iter().flatten(), cx)
    }

    // Outside the boxes: the options still to give whose name starts with what's typed, under Discord's
    // headers.
    fn names_popup(&self, typed: &Typed, query: &str, cx: &mut Context<Self>) -> AnyElement {
        let options = typed.open_options(query);
        let command = typed.command();
        let required = options.iter().filter(|&&o| command.options[o].required).count();
        let mut rows: Vec<AnyElement> = Vec::new();
        for (i, &option) in options.iter().enumerate() {
            if i == 0 && required > 0 {
                rows.push(list_header("Required options", cx).into_any_element());
            }
            if i == required {
                rows.push(list_header(if required > 0 { "Other options" } else { "Options" }, cx).into_any_element());
            }
            let on = self.names_on(query) && i == self.command_picker.highlighted;
            rows.push(self.name_row(&command.options[option], option, on, cx).into_any_element());
        }
        let body = ui::col().id("command-option-names").max_h(px(240.)).p_1p5().gap_0p5().overflow_y_scroll().children(rows);
        let foot = popup_foot(cx)
            .child(ui::key_hint(ui::row().items_stretch().gap_1().child(ui::kbd_glyph(Glyph::ArrowUp, cx)).child(ui::kbd_glyph(Glyph::ArrowDown, cx)), "Move", cx))
            .child(div().flex_1())
            .child(ui::key_hint(ui::kbd("Tab", cx), "Add", cx))
            .child(ui::key_hint(ui::kbd("Esc", cx), "Close", cx));
        popup("command-option-names-popup", [body.into_any_element(), foot.into_any_element()], cx)
    }

    fn name_row(&self, option: &CommandOption, i: usize, on: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        ui::menu_row(SharedString::from(format!("command-option-{i}")), cx)
            .highlighted(on)
            .gap_2p5()
            .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(option.display_name().to_owned()))
            .child(ui::line(option.description.clone()).flex_1().text_color(palette::muted(cx)))
            .on_click(cx.listener(move |this, _, window, cx| this.pick_name(i, window, cx)))
    }

    // Discord's header bar: the app, the command, the caret's option with its description (or, when its
    // value can't send, the reason in red in its place), and the optional options still to add.
    fn hint_bar(&self, typed: &Typed, cx: &mut Context<Self>) -> Div {
        let (strong, muted, faint, danger) = (palette::strong(cx), palette::muted(cx), palette::faint(cx), palette::danger_ink(cx));
        let command = typed.command();
        let optional: Vec<(usize, String)> = command.options.iter().enumerate().filter(|(i, o)| !o.required && !typed.line.has(*i)).map(|(i, o)| (i, o.display_name().to_owned())).collect();
        let add = (!optional.is_empty()).then(|| {
            let count = format!("+{} optional", optional.len());
            ui::row().flex_none().gap_1p5().text_size(ui::TEXT_XS).text_color(faint).child(count).children(optional.into_iter().map(|(i, name)| {
                ui::chip(SharedString::from(format!("add-option-{i}")), Some(Glyph::Plus), name, false, cx)
                    .h_6()
                    .px_2()
                    .text_size(ui::TEXT_XS)
                    .on_click(cx.listener(move |this, _, window, cx| this.add_option(i, window, cx)))
            }))
        });
        let app = typed.index.apps.iter().find(|a| a.id == command.application_id).cloned();
        let head = ui::row()
            .gap_2p5()
            .children(app.map(|a| app_tile(&a, 20., cx)))
            .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).text_color(strong).child(format!("/{}", command.display_name())));
        let head = match typed.arg() {
            Some((_, arg)) => {
                let option = typed.option(arg);
                let problem = self.box_problem(typed, arg);
                let detail = match &problem {
                    Some(why) => ui::row().flex_1().gap_1p5().text_color(danger).child(Icon::new(Glyph::WarningCircle).size(ui::ICON_MD)).child(ui::line(why.clone())),
                    None => ui::line(option.description.clone()).flex_1().text_color(muted),
                };
                head.child(div().flex_none().font_weight(FontWeight::SEMIBOLD).text_color(strong).child(option.display_name().to_owned())).child(detail).when(option.required && problem.is_none(), |d| d.child(required_mark(cx)))
            }
            None => head.child(ui::line(command.description.clone()).flex_1().text_color(muted)),
        };
        ui::col().px_3p5().py_2p5().gap_1p5().border_b_1().border_color(palette::line(cx)).text_size(ui::TEXT_SM).child(head.children(add))
    }

    fn choice_row(&self, i: usize, choice: OptionChoice, on: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let lead = match &choice.lead {
            ChoiceLead::None => None,
            ChoiceLead::Suggest(lead) => Some(crate::chat::suggest_lead(lead, cx)),
            ChoiceLead::Role(color) => Some(ui::row().size_7().flex_none().justify_center().child(div().size_3().rounded_full().bg(role_dot(*color, cx))).into_any_element()),
            ChoiceLead::Glyph(glyph) => Some(ui::row().size_7().flex_none().justify_center().text_color(palette::muted(cx)).child(Icon::new(*glyph).size(ui::ICON_LG)).into_any_element()),
        };
        ui::menu_row(SharedString::from(format!("command-choice-{i}")), cx)
            .highlighted(on)
            .children(lead)
            .child(ui::line(choice.label).flex_1().text_color(palette::strong(cx)))
            .on_click(cx.listener(move |this, _, window, cx| this.pick_choice(i, window, cx)))
    }
}

// OPTION BOXES

/// The transparent space an option's name chip leaves before itself, so neighbouring boxes don't touch.
pub(crate) const OPTION_GAP: f32 = 8.;
// How far a box runs past its value's last glyph: the start of its end chip.
const VALUE_PAD: f32 = 5.;
/// An end chip's width: the box's padding after its value, then a gap before what follows it.
pub(crate) const END_CHIP: f32 = VALUE_PAD + 3.;
const BOX_RADIUS: f32 = 6.;
const PLACEHOLDER_SIZE: f32 = 13.;

// One option drawn as a box: its name chip, then its value, as byte offsets into the composer's text.
struct OptionBox {
    start: usize,
    key_end: usize,
    value_end: usize,
    active: bool,
    invalid: bool,
    // Drawn in an empty value: an attachment's "Please attach a file".
    placeholder: Option<SharedString>,
}

impl Waffle {
    /// Each typed option's box, drawn over the composer's text after it lays out: the text field can't
    /// border a text range itself, so this reads where its range landed and strokes around it. Discord's
    /// "+N optional" trails the last box.
    pub(crate) fn option_boxes(&self, cx: &App) -> Option<impl IntoElement + use<>> {
        let typed = self.typed_command(cx)?;
        let active = typed.line.arg_at(typed.caret);
        let errored = &self.command_picker.errored;
        let boxes: Vec<OptionBox> = typed
            .line
            .args
            .iter()
            .enumerate()
            .map(|(n, arg)| {
                let option = typed.option(arg);
                let value = self.arg_value(&typed, arg);
                // Discord shows an error only on a box that isn't selected.
                let invalid = active != Some(n) && match command_line::resolve(option, &value) {
                    Err(_) => true,
                    Ok(None) => errored.contains(&arg.option),
                    Ok(Some(_)) => false,
                };
                let placeholder = (option.kind == OptionKind::Attachment && arg.value.is_empty()).then(|| SharedString::from(ATTACH_FILE));
                OptionBox { start: arg.key.start, key_end: arg.key.end, value_end: arg.inner().end, active: active == Some(n), invalid, placeholder }
            })
            .collect();
        let unset = typed.command().options.iter().enumerate().filter(|(i, o)| !o.required && !typed.line.has(*i)).count();
        let trailing = (unset > 0 && !typed.line.args.is_empty()).then(|| (typed.text().len(), SharedString::from(format!("+{unset} optional"))));
        let composer = self.composer.clone();
        let (line, accent, ring, danger, faint) = (palette::line_strong(cx), palette::accent_ink(cx), palette::accent_soft(cx), palette::danger_ink(cx), palette::faint(cx));
        let paint = move |bounds: gpui_kit::Bounds<gpui_kit::Pixels>, _: (), window: &mut Window, cx: &mut App| {
            // Read every position first: painting text needs `cx` back.
            let state = composer.read(cx);
            let at = |offset: usize| state.range_to_bounds(&(offset..offset));
            let placed: Vec<_> = boxes.iter().filter_map(|b| Some((b, at(b.start)?, at(b.key_end)?, at(b.value_end)?))).collect();
            let trailing = trailing.as_ref().and_then(|(offset, text)| Some((at(*offset)?, text)));
            let shaped = |text: &SharedString, window: &mut Window| {
                let run = gpui_kit::TextRun { color: faint, ..window.text_style().to_run(text.len()) };
                window.text_system().shape_line(text.clone(), px(PLACEHOLDER_SIZE), &[run], None)
            };
            window.with_content_mask(Some(gpui_kit::ContentMask { bounds }), |window| {
                for (b, start, key_end, end) in placed {
                    let placeholder = b.placeholder.as_ref().map(|p| shaped(p, window));
                    // An empty value's room is the key chip's own padding and this one.
                    let right = match &placeholder {
                        Some(p) => key_end.left() + p.width + px(VALUE_PAD + 2.),
                        None => end.left() + px(VALUE_PAD),
                    };
                    let edge = if b.invalid { danger } else if b.active { accent } else { line };
                    for rect in box_rows(start, right, end, bounds) {
                        if b.active {
                            window.paint_quad(gpui_kit::quad(rect.dilate(px(3.)), gpui_kit::Corners::all(px(BOX_RADIUS + 3.)), gpui_kit::transparent_black(), px(3.), ring, gpui_kit::BorderStyle::Solid));
                        }
                        window.paint_quad(gpui_kit::quad(rect, gpui_kit::Corners::all(px(BOX_RADIUS)), gpui_kit::transparent_black(), px(1.), edge, gpui_kit::BorderStyle::Solid));
                    }
                    if let Some(p) = placeholder {
                        p.paint(point(key_end.left() + px(2.), key_end.top()), key_end.size.height, gpui_kit::TextAlign::Left, None, window, cx).ok();
                    }
                }
                if let Some((end, text)) = trailing {
                    shaped(text, window).paint(point(end.left() + px(OPTION_GAP), end.top()), end.size.height, gpui_kit::TextAlign::Left, None, window, cx).ok();
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
    div().flex_none().text_size(ui::TEXT_XS).font_weight(FontWeight::SEMIBOLD).text_color(palette::danger_ink(cx)).child("Required")
}

// An app's icon, or its initial on a colour of its own.
fn app_tile(app: &app_commands::App, size: f32, cx: &App) -> AnyElement {
    ui::app_tile(app.icon.as_deref(), &app.name, &app.id, size, cx)
}

fn role_dot(color: u32, cx: &App) -> gpui_kit::Hsla {
    match color {
        0 => palette::muted(cx),
        color => palette::data_color(color),
    }
}

// The surface both popups share: the composer's width, 8px above it, rising from its foot.
fn popup(id: &'static str, content: impl IntoIterator<Item = AnyElement>, cx: &App) -> AnyElement {
    ui::col()
        .id(id)
        .occlude()
        .absolute()
        .left_neg_px()
        .right_neg_px()
        .bottom(relative(1.))
        .mb(px(9.))
        .squircle(RADIUS_LG)
        .bg(palette::surface(Level::Overlay, cx))
        .shadow(ui::shadow_pop(cx))
        .text_size(ui::TEXT_SM)
        .children(content)
        .transform_origin(point(relative(0.5), relative(1.)))
        .with_animation(id, Animation::new(ui::DUR_POP).with_easing(gpui_kit::ease_out_quint()), |el, t| el.opacity(t).scale(0.95 + 0.05 * t))
        .into_any_element()
}

fn popup_foot(cx: &App) -> Div {
    ui::row().gap_3p5().px_3p5().py_2().border_t_1().border_color(palette::line(cx)).text_size(ui::TEXT_XS).text_color(palette::faint(cx))
}

fn list_header(title: &'static str, cx: &App) -> Div {
    div().flex_none().pt_2().px_2p5().pb_1().text_size(ui::TEXT_XS).font_weight(FontWeight::SEMIBOLD).text_color(palette::muted(cx)).child(title)
}

fn list_note(text: &'static str, cx: &App) -> Div {
    div().px_4().py_3p5().text_color(palette::muted(cx)).child(text)
}

fn section_header(index: &CommandIndex, section: Section, query: &str, cx: &App) -> Div {
    let header = ui::row().flex_none().gap_2().pt_2().px_2p5().pb_1().text_size(ui::TEXT_XS).font_weight(FontWeight::SEMIBOLD).text_color(palette::muted(cx));
    match section {
        Section::Frecency => header.child(Icon::new(Glyph::Recent).size(ui::ICON_SM)).child("Recent"),
        Section::App(a) => header.child(app_tile(&index.apps[a], 16., cx)).child(index.apps[a].name.clone()),
        // Discord's "Commands matching **{prefix}**".
        Section::Matching => header.child("Commands matching").child(div().text_color(palette::strong(cx)).child(query.to_owned())),
        Section::BuiltIn => header.child(BUILT_IN),
    }
}

fn loading_rows(count: usize, cx: &App) -> Div {
    let widths = [260., 200., 230., 180., 240.];
    let row = |n: usize| {
        let width = widths[n % widths.len()];
        ui::row()
            .min_h_11()
            .px_2p5()
            .gap_3()
            .child(ui::skeleton(SharedString::from(format!("command-skeleton-tile-{n}")), RADIUS_SM, cx).size_8().flex_none())
            .child(
                ui::col()
                    .gap_1p5()
                    .child(ui::skeleton(SharedString::from(format!("command-skeleton-name-{n}")), 4., cx).w(px(width * 0.4)).h_3())
                    .child(ui::skeleton(SharedString::from(format!("command-skeleton-desc-{n}")), 4., cx).w(px(width)).h_2p5()),
            )
    };
    ui::col().p_1p5().gap_0p5().children((0..count).map(row))
}

fn picker_note(title: impl Into<SharedString>, body: impl Into<SharedString>, cx: &App) -> Div {
    ui::col()
        .px_4()
        .py_3p5()
        .gap_0p5()
        .child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(title.into()))
        .child(div().text_size(ui::TEXT_DETAIL).text_color(palette::muted(cx)).child(body.into()))
}

// LISTING

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Section {
    // The browser's top commands by Discord's frecency.
    Frecency,
    // An index into `CommandIndex::apps`.
    App(usize),
    // The typed list's single header.
    Matching,
    // Discord's own commands, after every app's.
    BuiltIn,
}

const BUILT_IN: &str = "Built-In";

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ListRow {
    Header(Section),
    // `command` indexes `CommandIndex::commands`; the ranges are what matched, in the name and description.
    Command { command: usize, name: Option<Range<usize>>, description: Option<Range<usize>> },
    // The built-in /tts; `name` is what matched in it, past the `/`.
    Tts { name: Option<Range<usize>> },
}

/// Discord lists its built-in commands in the same picker, as the last section ("Built-ins last"). Only
/// /tts is built in here, offered while the query is the start of it and it's allowed; none under an app
/// filter, since the rail has no Built-In tile.
/// GAP: Discord ranks built-ins by match tier with everything else; the typed list puts /tts last.
fn with_built_ins(mut rows: Vec<ListRow>, query: &str, filter: Option<usize>, tts_allowed: bool) -> Vec<ListRow> {
    if !tts_allowed || filter.is_some() || !send_options::offers_tts(&format!("/{query}")) {
        return rows;
    }
    let name = (!query.is_empty()).then_some(0..query.len());
    if query.is_empty() {
        rows.push(ListRow::Header(Section::BuiltIn));
    } else if rows.is_empty() {
        rows.push(ListRow::Header(Section::Matching));
    }
    rows.push(ListRow::Tts { name });
    rows
}

/// The picker's rows. A query gives Discord's inline list: one flat list of up to 20 by match tier, then
/// frecency score, then name. Bare `/` gives the browser: the top commands, then every app's (top ones
/// again among them), apps by their own frecency then name, `filter` keeping one app.
/// GAP: Discord pages each browser section 10 at a time as it scrolls; all rows render here.
pub(crate) fn listing(index: &CommandIndex, query: &str, ranks: &Ranks, filter: Option<usize>) -> Vec<ListRow> {
    if !query.is_empty() {
        return matching(index, query, ranks);
    }
    let mut rows = Vec::new();
    if !ranks.top.is_empty() && filter.is_none() {
        rows.push(ListRow::Header(Section::Frecency));
        rows.extend(ranks.top.iter().map(|&c| plain_row(c)));
    }
    for a in app_order(index, ranks) {
        if filter.is_some_and(|f| f != a) {
            continue;
        }
        let mut mine: Vec<usize> = (0..index.commands.len()).filter(|&c| index.commands[c].application_id == index.apps[a].id).collect();
        mine.sort_by_cached_key(|&c| index.commands[c].display_name().to_lowercase());
        rows.push(ListRow::Header(Section::App(a)));
        rows.extend(mine.into_iter().map(plain_row));
    }
    rows
}

fn plain_row(command: usize) -> ListRow {
    ListRow::Command { command, name: None, description: None }
}

// Apps with commands, by app frecency score (highest first), then name.
fn app_order(index: &CommandIndex, ranks: &Ranks) -> Vec<usize> {
    let mut apps: Vec<usize> = (0..index.apps.len()).filter(|&a| index.commands.iter().any(|c| c.application_id == index.apps[a].id)).collect();
    apps.sort_by(|&a, &b| ranks.app[b].total_cmp(&ranks.app[a]).then_with(|| index.apps[a].name.to_lowercase().cmp(&index.apps[b].name.to_lowercase())));
    apps
}

// The rail: Recent when there is one, then each app in the browser's order.
fn browser_sections(index: &CommandIndex, ranks: &Ranks) -> Vec<Section> {
    let recent = (!ranks.top.is_empty()).then_some(Section::Frecency);
    recent.into_iter().chain(app_order(index, ranks).into_iter().map(Section::App)).collect()
}

fn matching(index: &CommandIndex, query: &str, ranks: &Ranks) -> Vec<ListRow> {
    let q = query.to_lowercase();
    let mut hits: Vec<(u8, usize)> = index
        .commands
        .iter()
        .enumerate()
        .filter(|(_, c)| keeps_trailing_space(c, &q))
        .filter_map(|(i, c)| {
            let app = index.apps.iter().find(|a| a.id == c.application_id);
            Some((match_tier(c, app, &q)?, i))
        })
        .collect();
    hits.sort_by(|a, b| a.0.cmp(&b.0).then(ranks.command[b.1].total_cmp(&ranks.command[a.1])).then_with(|| index.commands[a.1].display_name().to_lowercase().cmp(&index.commands[b.1].display_name().to_lowercase())));
    hits.truncate(MATCH_LIMIT);
    if hits.is_empty() {
        return Vec::new();
    }
    let rows = hits.into_iter().map(|(tier, i)| {
        let c = &index.commands[i];
        let name = (tier <= 2).then(|| find_ci(&c.display_name(), query.trim_end())).flatten();
        let description = (tier == 7).then(|| find_ci(&c.description, query)).flatten();
        ListRow::Command { command: i, name, description }
    });
    std::iter::once(ListRow::Header(Section::Matching)).chain(rows).collect()
}

// Discord: with a trailing space (`/music `) only commands named exactly that, or starting with it, stay.
fn keeps_trailing_space(c: &AppCommand, q: &str) -> bool {
    if !q.ends_with(' ') {
        return true;
    }
    [c.name().to_lowercase(), c.display_name().to_lowercase()].iter().any(|n| n == q.trim_end() || n.starts_with(q))
}

/// Discord's match tier for `q` (lowercased), lower first; None when nothing matches. The command's own
/// tiers and its app's (5 name prefix, 6 name contains, 8 description, 11 fuzzy name) compete; the
/// lower wins.
/// GAP: tier 8 (the app's description) needs the index's app descriptions, which aren't parsed.
pub(crate) fn match_tier(c: &AppCommand, app: Option<&app_commands::App>, q: &str) -> Option<u8> {
    let app_tier = app.and_then(|app| {
        let name = app.name.to_lowercase();
        match () {
            _ if name.starts_with(q) => Some(5),
            _ if name.contains(q) => Some(6),
            _ if fuzzy(q, &name) => Some(11),
            _ => None,
        }
    });
    match (command_tier(c, q), app_tier) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

fn command_tier(c: &AppCommand, q: &str) -> Option<u8> {
    let (r, s) = (c.name().to_lowercase(), c.display_name().to_lowercase());
    if r.starts_with(q) || s.starts_with(q) {
        return Some(0);
    }
    let (first, rest) = q.split_once(' ').unwrap_or((q, ""));
    let rest_of = |name: &str| name.split_once(' ').map_or(String::new(), |(_, rest)| rest.to_owned());
    if (r.starts_with(first) && rest_of(&r).starts_with(rest)) || (s.starts_with(first) && rest_of(&s).starts_with(rest)) {
        return Some(1);
    }
    if r.contains(q) || s.contains(q) {
        return Some(2);
    }
    let option_names = || c.options.iter().flat_map(|o| std::iter::once(o.name.as_str()).chain(o.name_localized.as_deref()));
    if option_names().any(|n| n.starts_with(q) || format!("{r} {n}").starts_with(q) || format!("{s} {n}").starts_with(q)) {
        return Some(3);
    }
    if option_names().any(|n| n.contains(q)) {
        return Some(4);
    }
    let description = c.description.to_lowercase();
    if description.contains(q) {
        return Some(7);
    }
    if fuzzy(q, &r) || fuzzy(q, &s) {
        return Some(9);
    }
    if option_names().any(|n| fuzzy(q, n)) {
        return Some(10);
    }
    fuzzy(q, &description).then_some(12)
}

/// The `fuzzysearch` package Discord uses: every char of `needle` in `haystack`, in order.
pub(crate) fn fuzzy(needle: &str, haystack: &str) -> bool {
    let mut rest = haystack.chars();
    needle.chars().all(|n| rest.any(|h| h == n))
}

// The row index of every command row, in order: what ↑/↓ step through.
fn pickable(rows: &[ListRow]) -> Vec<usize> {
    rows.iter().enumerate().filter(|(_, r)| !matches!(r, ListRow::Header(_))).map(|(i, _)| i).collect()
}

fn section_of(rows: &[ListRow], row: usize) -> Section {
    rows[..=row]
        .iter()
        .rev()
        .find_map(|r| match r {
            ListRow::Header(s) => Some(*s),
            ListRow::Command { .. } | ListRow::Tts { .. } => None,
        })
        .unwrap_or(Section::Frecency)
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
        let mut play = command("1", "w", "play", "Play a song");
        play.options = vec![CommandOption { name: "query".into(), ..Default::default() }];
        CommandIndex {
            apps: vec![app("w", "Wavebot"), app("p", "Pollster")],
            commands: vec![
                play,
                command("2", "w", "queue", "Songs coming up, and who placed them"),
                command("3", "w", "playlist", "Queue a playlist"),
                command("4", "p", "place-vote", "Vote in a poll"),
                command("5", "p", "poll", "Start a poll"),
                command("6", "w", "music queue", "What's next"),
            ],
        }
    }

    fn ranks(index: &CommandIndex) -> Ranks {
        Ranks { command: vec![0.; index.commands.len()], app: vec![0.; index.apps.len()], top: Vec::new() }
    }

    fn names(index: &CommandIndex, rows: &[ListRow]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                ListRow::Header(Section::Frecency) => "#recent".to_owned(),
                ListRow::Header(Section::Matching) => "#matching".to_owned(),
                ListRow::Header(Section::App(a)) => format!("#{}", index.apps[*a].name),
                ListRow::Header(Section::BuiltIn) => "#built-in".to_owned(),
                ListRow::Command { command, .. } => index.commands[*command].name(),
                ListRow::Tts { .. } => "tts".to_owned(),
            })
            .collect()
    }

    #[test]
    fn lists_tts_as_the_last_section() {
        let index = index();
        let ranks = ranks(&index);
        let rows = |query: &str, filter, allowed| names(&index, &with_built_ins(listing(&index, query, &ranks, filter), query, filter, allowed));
        assert_eq!(rows("", None, true).last_chunk::<2>().unwrap(), &["#built-in".to_owned(), "tts".to_owned()]);
        let typed = rows("tt", None, true);
        assert_eq!((typed.first().unwrap().as_str(), typed.last().unwrap().as_str()), ("#matching", "tts"));
        assert!(!rows("pl", None, true).contains(&"tts".to_owned()));
        assert!(!rows("", None, false).contains(&"tts".to_owned()));
        assert!(!rows("", Some(0), true).contains(&"tts".to_owned()));
        assert!(!rows("tts ", None, true).contains(&"tts".to_owned()));
        assert_eq!(with_built_ins(Vec::new(), "tt", None, true)[1], ListRow::Tts { name: Some(0..2) });
    }

    #[test]
    fn finds_case_insensitively_by_byte_range() {
        assert_eq!(find_ci("Midnight Arcade", "arc"), Some(9..12));
        assert_eq!(find_ci("Ärger", "är"), Some(0..3));
        assert_eq!(find_ci("play", "x"), None);
    }

    #[test]
    fn tiers_follow_discords_order() {
        let index = index();
        let (play, queue, music) = (&index.commands[0], &index.commands[1], &index.commands[5]);
        let wavebot = Some(&index.apps[0]);
        assert_eq!(match_tier(play, wavebot, "pl"), Some(0));
        assert_eq!(match_tier(music, wavebot, "mu q"), Some(1));
        assert_eq!(match_tier(queue, wavebot, "eue"), Some(2));
        assert_eq!(match_tier(play, wavebot, "que"), Some(3));
        assert_eq!(match_tier(play, wavebot, "play q"), Some(3), "the name then an option name counts as an option prefix");
        assert_eq!(match_tier(play, wavebot, "uer"), Some(4));
        assert_eq!(match_tier(queue, wavebot, "wave"), Some(5));
        assert_eq!(match_tier(queue, wavebot, "bot"), Some(6));
        assert_eq!(match_tier(queue, wavebot, "placed"), Some(7));
        assert_eq!(match_tier(&index.commands[2], None, "pyst"), Some(9));
        assert_eq!(match_tier(play, None, "qry"), Some(10));
        assert_eq!(match_tier(queue, wavebot, "wvbt"), Some(11));
        assert_eq!(match_tier(queue, None, "scmg"), Some(12));
        assert_eq!(match_tier(queue, None, "zz"), None);
    }

    #[test]
    fn a_query_lists_one_flat_list_by_tier_then_score_then_name() {
        let index = index();
        let mut ranks = ranks(&index);
        let rows = listing(&index, "pl", &ranks, None);
        assert_eq!(names(&index, &rows), ["#matching", "place-vote", "play", "playlist", "queue", "poll"]);
        ranks.command[2] = 5.;
        assert_eq!(names(&index, &listing(&index, "pl", &ranks, None))[1..4], ["playlist", "place-vote", "play"]);
        let queue = listing(&index, "pl", &ranks, None).into_iter().find(|r| matches!(r, ListRow::Command { command: 1, .. })).unwrap();
        assert_eq!(queue, ListRow::Command { command: 1, name: None, description: Some(25..27) });
    }

    #[test]
    fn a_trailing_space_keeps_only_that_name_and_longer() {
        let index = index();
        assert_eq!(names(&index, &listing(&index, "music ", &ranks(&index), None)), ["#matching", "music queue"]);
        assert!(listing(&index, "nothing here", &ranks(&index), None).is_empty());
    }

    #[test]
    fn at_most_twenty_match() {
        let mut index = index();
        index.commands = (0..30).map(|i| command(&i.to_string(), "w", &format!("cmd{i:02}"), "")).collect();
        assert_eq!(listing(&index, "cmd", &ranks(&index), None).len(), 1 + MATCH_LIMIT);
    }

    #[test]
    fn the_browser_shows_top_commands_again_in_their_app() {
        let index = index();
        let mut ranks = ranks(&index);
        ranks.top = vec![4];
        ranks.app = vec![0., 1.];
        let rows = listing(&index, "", &ranks, None);
        assert_eq!(names(&index, &rows), ["#recent", "poll", "#Pollster", "place-vote", "poll", "#Wavebot", "music queue", "play", "playlist", "queue"]);
        assert_eq!(browser_sections(&index, &ranks), [Section::Frecency, Section::App(1), Section::App(0)]);
        assert_eq!(names(&index, &listing(&index, "", &ranks, Some(1))), ["#Pollster", "place-vote", "poll"]);
    }

    #[test]
    fn fuzzy_is_an_in_order_subsequence() {
        assert!(fuzzy("pst", "playlist"));
        assert!(!fuzzy("tsp", "playlist"));
        assert!(fuzzy("", "x"));
        assert!(!fuzzy("xx", "x"));
    }

    #[test]
    fn debounce_fires_at_the_start_and_end_of_a_pause() {
        let wait = Duration::from_millis(500);
        let t0 = Instant::now();
        let mut d = Debounce::default();
        assert!(d.call(t0, wait), "leading");
        assert!(!d.expire(), "no call since the leading one");
        assert!(d.call(t0 + wait * 2, wait), "a fresh pause leads again");
        assert!(!d.call(t0 + wait * 2 + Duration::from_millis(100), wait));
        assert!(!d.call(t0 + wait * 2 + Duration::from_millis(300), wait));
        assert_eq!(d.until, Some(t0 + wait * 2 + Duration::from_millis(800)), "each call extends the pause");
        assert!(d.expire(), "trailing");
        assert!(d.call(t0 + wait * 4, wait), "leading again after the trailing call");
    }

    #[test]
    fn an_option_chip_sends_its_own_text_and_an_end_chip_a_space() {
        let token = option_token("query:");
        assert!(is_option_token(&token) && !is_end_token(&token));
        assert_eq!(token.text().as_ref(), "query:");
        assert!(!is_option_token(&InlineToken::new("<@42>", "@Ferris")));
        assert!(is_end_token(&end_token()) && !is_option_token(&end_token()));
        assert_eq!(end_token().text().as_ref(), " ");
    }

    #[test]
    fn a_picked_command_starts_with_empty_closed_boxes() {
        let content = with_boxes("/play ", &["query:".into(), "volume:".into()]);
        assert_eq!(content.text().as_ref(), "/play query: volume: ");
        assert_eq!(marks(&content), [(6..12, Mark::Key), (12..13, Mark::End), (13..20, Mark::Key), (20..21, Mark::End)]);
    }

    #[test]
    fn boxing_a_typed_key_keeps_the_caret_in_its_value() {
        // `/play query:abc  volume:` with the caret after `abc `: the two spaces become the end chip.
        assert_eq!(moved_caret(16, [(15..17, 1)].into_iter()), 15, "in the spaces: the value's end");
        assert_eq!(moved_caret(24, [(15..17, 1), (24..24, 1)].into_iter()), 23, "typing at the end: before the new end chip");
        assert_eq!(moved_caret(12, [(15..17, 1)].into_iter()), 12);
    }
}
