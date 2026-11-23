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
use crate::part::Column;

mod typing;
mod autocomplete;
mod render;
mod boxes;
mod listing;

pub(crate) use self::{autocomplete::now_ms, boxes::{END_CHIP, OPTION_GAP}, render::{RailTint, rail_slot}};
use self::{autocomplete::*, listing::*};

// The command rail's width.
const RAIL_WIDTH: f32 = 52.;
// The tallest a choice or option-name list grows before it scrolls.
const CHOICE_LIST_MAX: f32 = 240.;
// The active option box's focus ring width.
const FOCUS_RING: f32 = 3.;
// A value placeholder's gap after its key chip.
const PLACEHOLDER_NUDGE: f32 = 2.;
// A popup sits 8px above the composer, plus its hairline.
const POPUP_RAISE: f32 = 9.;

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
        self.redraw(Column::Main, cx);
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
                    this.redraw(Column::Main, cx);
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
                this.redraw(Column::Main, cx);
            })
            .ok();
        })
        .detach();
        self.redraw(Column::Main, cx);
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
        self.redraw(Column::Main, cx);
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
                    self.redraw(Column::Main, cx);
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
        self.redraw(Column::Main, cx);
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
                self.redraw(Column::Main, cx);
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
