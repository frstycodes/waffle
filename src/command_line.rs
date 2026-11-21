// A slash command typed as plain text, Discord's `/name opt:value opt:value`: which command it runs,
// where each option's value sits, and whether the values can be sent.

use std::ops::Range;

use serde_json::{Value, json};

use crate::app_commands::{AppCommand, CommandOption, OptionKind};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CommandLine {
    // An index into the commands `parse` was given.
    pub command: usize,
    // The byte after `/name`.
    pub name_end: usize,
    // In the order they appear in the text.
    pub args: Vec<Arg>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Arg {
    // An index into the command's options.
    pub option: usize,
    // The `name:` key.
    pub key: Range<usize>,
    // The value with its surrounding spaces trimmed; empty at `key.end` when nothing is typed.
    pub value: Range<usize>,
    // Where a caret counts as being in this value: from the key's end up to the next key.
    pub extent: Range<usize>,
}

/// Reads `text` as a command from `commands`: the longest command name it starts with, as you see it or
/// untranslated (`preferred`, an `AppCommand::id`, wins between two apps' same-named commands), then
/// every ` name:` that names one of its options. A value runs until the next such key, so it can hold
/// spaces; unknown names are plain text and only an option's first key counts.
pub(crate) fn parse(text: &str, commands: &[AppCommand], preferred: Option<&str>) -> Option<CommandLine> {
    let rest = text.strip_prefix('/')?;
    let named = |c: &AppCommand| {
        [c.display_name(), c.name()]
            .into_iter()
            .filter_map(|name| {
                let after = rest.strip_prefix(name.as_str())?;
                (after.is_empty() || after.starts_with(char::is_whitespace)).then_some(name.len())
            })
            .max()
    };
    let (command, len) = commands
        .iter()
        .enumerate()
        .filter_map(|(i, c)| Some((i, named(c)?)))
        .max_by_key(|&(i, len)| (len, preferred == Some(commands[i].id.as_str()), std::cmp::Reverse(i)))?;
    let name_end = 1 + len;
    let keys = keys(text, name_end, &commands[command].options);
    let args = keys
        .iter()
        .enumerate()
        .map(|(n, &(option, ref key))| {
            let end = keys.get(n + 1).map_or(text.len(), |(_, next)| next.start);
            Arg { option, key: key.clone(), value: trimmed(text, key.end..end), extent: key.end..end }
        })
        .collect();
    Some(CommandLine { command, name_end, args })
}

// Every option key after the command name, each option's first only, in text order. A key is the
// option's name as you see it or untranslated.
fn keys(text: &str, from: usize, options: &[CommandOption]) -> Vec<(usize, Range<usize>)> {
    let mut found: Vec<(usize, Range<usize>)> = Vec::new();
    let mut previous = text[..from].chars().next_back();
    for (at, c) in text[from..].char_indices().map(|(i, c)| (from + i, c)) {
        let starts_word = previous.is_some_and(char::is_whitespace);
        previous = Some(c);
        if !starts_word {
            continue;
        }
        let key_len = |o: &CommandOption| [o.display_name(), o.name.as_str()].into_iter().find(|name| text[at..].strip_prefix(name).is_some_and(|r| r.starts_with(':'))).map(str::len);
        let hit = options.iter().enumerate().filter(|(i, _)| !found.iter().any(|(f, _)| f == i)).find_map(|(i, o)| Some((i, key_len(o)?)));
        if let Some((i, len)) = hit {
            found.push((i, at..at + len + 1));
        }
    }
    found
}

fn trimmed(text: &str, range: Range<usize>) -> Range<usize> {
    let slice = &text[range.clone()];
    let start = range.start + (slice.len() - slice.trim_start().len());
    let end = range.end - (slice.len() - slice.trim_end().len());
    if start >= end {
        return range.start..range.start;
    }
    start..end
}

impl CommandLine {
    /// The arg whose value the caret is in; at the very start of a key it still belongs to the value before.
    pub(crate) fn arg_at(&self, caret: usize) -> Option<usize> {
        self.args.iter().position(|a| a.key.end <= caret && caret <= a.extent.end)
    }

    pub(crate) fn has(&self, option: usize) -> bool {
        self.args.iter().any(|a| a.option == option)
    }

    /// Tab's box from `at` (None: outside every box): the next arg, None past the last (the end of the
    /// text), and the first from outside.
    /// Not verified: whether Discord's Tab at the end wraps to the first box.
    pub(crate) fn next_arg(&self, at: Option<usize>) -> Option<usize> {
        match at {
            Some(n) => (n + 1 < self.args.len()).then_some(n + 1),
            None => (!self.args.is_empty()).then_some(0),
        }
    }

    /// Shift-Tab's box: the previous arg, None before the first (the end of the text), the last from outside.
    /// Not verified: what Discord's Shift-Tab does on the first box.
    pub(crate) fn previous_arg(&self, at: Option<usize>) -> Option<usize> {
        match at {
            Some(n) => n.checked_sub(1),
            None => self.args.len().checked_sub(1),
        }
    }
}

/// An option Enter can't send: missing (no key at all) or with a bad value. Discord marks them all at once.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Problem {
    // An index into the command's options.
    pub option: usize,
    pub missing: bool,
    pub message: String,
}

pub(crate) const REQUIRED: &str = "This option is required. Specify a value.";

/// Every given option's value, or every problem: bad or empty-but-required values in text order, then
/// each required option never given, in definition order.
pub(crate) fn validate(command: &AppCommand, line: &CommandLine, value_of: impl Fn(&Arg) -> ArgValue) -> Result<Vec<(String, Value)>, Vec<Problem>> {
    let mut values = Vec::new();
    let mut problems = Vec::new();
    for arg in &line.args {
        let option = &command.options[arg.option];
        match resolve(option, &value_of(arg)) {
            Ok(Some(value)) => values.push((option.name.clone(), value)),
            Ok(None) if option.required => problems.push(Problem { option: arg.option, missing: false, message: REQUIRED.into() }),
            Ok(None) => {}
            Err(message) => problems.push(Problem { option: arg.option, missing: false, message }),
        }
    }
    problems.extend(missing(command, line).into_iter().map(|option| Problem { option, missing: true, message: REQUIRED.into() }));
    match problems.is_empty() {
        true => Ok(values),
        false => Err(problems),
    }
}

/// The required options the line has no key for, in definition order.
pub(crate) fn missing(command: &AppCommand, line: &CommandLine) -> Vec<usize> {
    (0..command.options.len()).filter(|&i| command.options[i].required && !line.has(i)).collect()
}

/// What a value holds: its text (a mention chip's id, or the id a typed name resolved to), the value of a
/// picked choice whose label it still is, a channel's type, and an attached file's index.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ArgValue {
    pub text: String,
    pub picked: Option<Value>,
    pub channel_type: Option<u8>,
    pub file: Option<usize>,
}

// JavaScript's Number.MAX_SAFE_INTEGER: Discord's client refuses integers past it.
const SAFE_INTEGER: f64 = 9_007_199_254_740_991.;

/// An option's value as Discord takes it: `Ok(None)` when it's left empty, `Err` with Discord's message.
pub(crate) fn resolve(option: &CommandOption, input: &ArgValue) -> Result<Option<Value>, String> {
    if option.kind == OptionKind::Attachment {
        return Ok(input.file.map(|i| json!(i)));
    }
    if let Some(value) = &input.picked {
        return Ok(Some(value.clone()));
    }
    let text = input.text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    if !option.choices.is_empty() {
        let choice = option.choices.iter().find(|c| c.display_name().eq_ignore_ascii_case(text) || c.name.eq_ignore_ascii_case(text));
        return choice.map(|c| Some(c.value.clone())).ok_or_else(|| "Not a valid choice.".into());
    }
    let snowflake = text.bytes().all(|b| b.is_ascii_digit());
    match option.kind {
        OptionKind::Integer => {
            let n = number(text).filter(|n| n.fract() == 0. && n.abs() <= SAFE_INTEGER).ok_or("Input a valid integer.")?;
            in_range(option, n)?;
            Ok(Some(json!(n as i64)))
        }
        OptionKind::Number => {
            let n = number(text).ok_or("Input a valid number.")?;
            in_range(option, n)?;
            Ok(Some(json!(n)))
        }
        OptionKind::Boolean => match text.to_lowercase().as_str() {
            "true" => Ok(Some(json!(true))),
            "false" => Ok(Some(json!(false))),
            _ => Err("Pick either true or false.".into()),
        },
        // Not verified: Discord's copy for a channel of a type the option doesn't take; this is its
        // generic one for channels.
        OptionKind::Channel if snowflake && input.channel_type.is_some_and(|t| !option.channel_types.is_empty() && !option.channel_types.contains(&t)) => Err("Not a valid channel.".into()),
        OptionKind::User | OptionKind::Channel | OptionKind::Role | OptionKind::Mentionable if snowflake => Ok(Some(json!(text))),
        OptionKind::User => Err("Not a valid user.".into()),
        OptionKind::Channel => Err("Not a valid channel.".into()),
        OptionKind::Role => Err("Not a valid role.".into()),
        OptionKind::Mentionable => Err("Not a valid user or role.".into()),
        OptionKind::String => {
            in_length(option, text)?;
            Ok(Some(json!(text)))
        }
        OptionKind::Attachment => Err("Not a valid value.".into()),
    }
}

// Not verified: Discord parses numbers with your locale's separators (`P.ib(locale, text)`); this reads
// a `.` decimal point only.
fn number(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|n| n.is_finite())
}

fn in_range(option: &CommandOption, n: f64) -> Result<(), String> {
    match (option.min_value, option.max_value) {
        (Some(minimum), Some(maximum)) if n < minimum || n > maximum => Err(format!("Enter a number between {minimum} and {maximum}.")),
        (Some(minimum), None) if n < minimum => Err(format!("Enter a number greater than or equal to {minimum}.")),
        (None, Some(maximum)) if n > maximum => Err(format!("Enter a number less than or equal to {maximum}.")),
        _ => Ok(()),
    }
}

// Lengths in UTF-16 units, as Discord's client counts them (`.length`).
fn in_length(option: &CommandOption, text: &str) -> Result<(), String> {
    let len = text.encode_utf16().count() as u32;
    match (option.min_length, option.max_length) {
        (Some(value), Some(max)) if value == max && len != value => Err(format!("Enter {value} characters.")),
        (Some(minimum), Some(maximum)) if len < minimum || len > maximum => Err(format!("Enter between {minimum} and {maximum} characters.")),
        (Some(minimum), None) if len < minimum => Err(format!("Enter {minimum} or more characters.")),
        (None, Some(maximum)) if len > maximum => Err(format!("Enter {maximum} or fewer characters.")),
        _ => Ok(()),
    }
}

/// A type-4 body's values, as Discord's client builds them: the focused option's raw text whatever its
/// type, every other option that resolves, attachments never.
pub(crate) fn autocomplete_values(command: &AppCommand, line: &CommandLine, focused: usize, value_of: impl Fn(&Arg) -> ArgValue) -> Vec<(String, Value)> {
    line.args
        .iter()
        .enumerate()
        .filter(|(_, arg)| command.options[arg.option].kind != OptionKind::Attachment)
        .filter_map(|(n, arg)| {
            let option = &command.options[arg.option];
            match n == focused {
                true => Some((option.name.clone(), json!(value_of(arg).text))),
                false => resolve(option, &value_of(arg)).ok().flatten().map(|v| (option.name.clone(), v)),
            }
        })
        .collect()
}

/// Discord's autocomplete cache context: the focused option, then the others' `name=value`, NUL-joined.
/// Answers cached under one context are dropped when it changes.
pub(crate) fn autocomplete_context(command: &AppCommand, line: &CommandLine, focused: usize, text: &str) -> String {
    let others = line.args.iter().enumerate().filter(|(n, _)| *n != focused).map(|(_, a)| format!("{}={}", command.options[a.option].name, &text[a.value.clone()]));
    std::iter::once(command.options[line.args[focused].option].name.clone()).chain(others).collect::<Vec<_>>().join("\0")
}

/// A mention chip's wire form (`<@id>`, `<@!id>`, `<@&id>`, `<#id>`) as the bare id an option takes.
pub(crate) fn mention_id(wire: &str) -> &str {
    wire.trim_start_matches(['<', '@', '#', '!', '&']).trim_end_matches('>')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn option(name: &str, kind: OptionKind, required: bool) -> CommandOption {
        CommandOption { kind, name: name.into(), required, ..Default::default() }
    }

    fn command(id: &str, path: &str, options: Vec<CommandOption>) -> AppCommand {
        AppCommand { id: id.into(), path: path.split(' ').map(str::to_owned).collect(), options, ..Default::default() }
    }

    fn commands() -> Vec<AppCommand> {
        vec![
            command("1", "play", vec![option("query", OptionKind::String, true), option("volume", OptionKind::Integer, false)]),
            command("2", "music queue", vec![]),
            command("3", "music queue clear", vec![option("all", OptionKind::Boolean, false)]),
            command("4", "play", vec![option("song", OptionKind::String, true)]),
        ]
    }

    fn values(text: &str, line: &CommandLine) -> Vec<(String, String)> {
        let options = &commands()[line.command].options;
        line.args.iter().map(|a| (options[a.option].name.clone(), text[a.value.clone()].to_owned())).collect()
    }

    #[test]
    fn values_run_until_the_next_known_key() {
        let text = "/play query:midnight ar  volume:80";
        let line = parse(text, &commands(), None).unwrap();
        assert_eq!(line.command, 0);
        assert_eq!(line.name_end, 5);
        assert_eq!(values(text, &line), [("query".into(), "midnight ar".into()), ("volume".into(), "80".into())]);
        assert_eq!(line.args[0].key, 6..12);
        assert_eq!(line.args[0].extent, 12..25);
    }

    #[test]
    fn unknown_names_quotes_and_glued_keys_are_plain_text() {
        let text = "/play query:\"a b\" mood:calm xvolume:3";
        let line = parse(text, &commands(), None).unwrap();
        assert_eq!(values(text, &line), [("query".into(), "\"a b\" mood:calm xvolume:3".into())]);
    }

    #[test]
    fn only_an_options_first_key_counts() {
        let text = "/play query:a query:b";
        let line = parse(text, &commands(), None).unwrap();
        assert_eq!(values(text, &line), [("query".into(), "a query:b".into())]);
    }

    #[test]
    fn the_longest_command_name_wins_and_preferred_breaks_ties() {
        let line = parse("/music queue clear all:true", &commands(), None).unwrap();
        assert_eq!(line.command, 2);
        assert_eq!(parse("/music queue", &commands(), None).unwrap().command, 1);
        assert_eq!(parse("/music queuex", &commands(), None), None);
        assert_eq!(parse("play", &commands(), None), None);
        assert_eq!(parse("/play ", &commands(), Some("4")).unwrap().command, 3);
        assert_eq!(parse("/play ", &commands(), None).unwrap().command, 0);
    }

    #[test]
    fn localized_command_and_option_names_read_too() {
        let mut spielen = command("1", "play", vec![CommandOption { name_localized: Some("suche".into()), ..option("query", OptionKind::String, true) }]);
        spielen.path_localized = vec![Some("spielen".into())];
        let cmds = vec![spielen];
        let text = "/spielen suche:abc";
        let line = parse(text, &cmds, None).unwrap();
        assert_eq!((line.name_end, line.args[0].option, &text[line.args[0].value.clone()]), (8, 0, "abc"));
        assert_eq!(parse("/play query:x", &cmds, None).unwrap().args.len(), 1);
    }

    #[test]
    fn an_empty_value_sits_at_its_keys_end() {
        let text = "/play query:  ";
        let line = parse(text, &commands(), None).unwrap();
        assert_eq!(line.args[0].value, 12..12);
    }

    #[test]
    fn the_caret_belongs_to_the_value_it_sits_in() {
        let text = "/play query:abc volume:8";
        let line = parse(text, &commands(), None).unwrap();
        assert_eq!(line.arg_at(3), None, "in the command name");
        assert_eq!(line.arg_at(12), Some(0));
        assert_eq!(line.arg_at(16), Some(0), "the start of the next key");
        assert_eq!(line.arg_at(text.len()), Some(1));
    }

    #[test]
    fn tab_walks_the_boxes_and_out_past_the_last() {
        let line = parse("/play query:a volume:2", &commands(), None).unwrap();
        assert_eq!(line.next_arg(None), Some(0));
        assert_eq!(line.next_arg(Some(0)), Some(1));
        assert_eq!(line.next_arg(Some(1)), None);
        assert_eq!(line.previous_arg(None), Some(1));
        assert_eq!(line.previous_arg(Some(1)), Some(0));
        assert_eq!(line.previous_arg(Some(0)), None);
        let bare = parse("/music queue", &commands(), None).unwrap();
        assert_eq!((bare.next_arg(None), bare.previous_arg(None)), (None, None));
    }

    fn plain(text: &str) -> impl Fn(&Arg) -> ArgValue + '_ {
        move |a| ArgValue { text: text[a.value.clone()].to_owned(), ..Default::default() }
    }

    #[test]
    fn enter_reports_every_problem_at_once() {
        let cmds = vec![command("9", "x", vec![option("a", OptionKind::String, true), option("n", OptionKind::Integer, false), option("b", OptionKind::User, true), option("c", OptionKind::Boolean, true)])];
        let text = "/x n:loud a:";
        let line = parse(text, &cmds, None).unwrap();
        let problems = validate(&cmds[0], &line, plain(text)).unwrap_err();
        let summary: Vec<(usize, bool, &str)> = problems.iter().map(|p| (p.option, p.missing, p.message.as_str())).collect();
        assert_eq!(summary, [(1, false, "Input a valid integer."), (0, false, REQUIRED), (2, true, REQUIRED), (3, true, REQUIRED)]);
    }

    #[test]
    fn values_go_out_once_everything_resolves() {
        let cmds = commands();
        let text = "/play volume:80 query:a song";
        let line = parse(text, &cmds, None).unwrap();
        assert_eq!(validate(&cmds[0], &line, plain(text)), Ok(vec![("volume".into(), json!(80)), ("query".into(), json!("a song"))]));
    }

    fn text(t: &str) -> ArgValue {
        ArgValue { text: t.into(), ..Default::default() }
    }

    #[test]
    fn validates_with_discords_messages() {
        assert_eq!(resolve(&option("n", OptionKind::Integer, true), &text("42")), Ok(Some(json!(42))));
        assert_eq!(resolve(&option("n", OptionKind::Integer, true), &text("4.2")), Err("Input a valid integer.".into()));
        assert_eq!(resolve(&option("n", OptionKind::Integer, true), &text("9007199254740993")), Err("Input a valid integer.".into()));
        assert_eq!(resolve(&option("n", OptionKind::Number, true), &text("4.5")), Ok(Some(json!(4.5))));
        assert_eq!(resolve(&option("n", OptionKind::Number, true), &text("NaN")), Err("Input a valid number.".into()));
        assert_eq!(resolve(&option("b", OptionKind::Boolean, true), &text("TRUE")), Ok(Some(json!(true))));
        assert_eq!(resolve(&option("b", OptionKind::Boolean, true), &text("yes")), Err("Pick either true or false.".into()));
        assert_eq!(resolve(&option("u", OptionKind::User, true), &text("mira")), Err("Not a valid user.".into()));
        assert_eq!(resolve(&option("r", OptionKind::Role, true), &text("mods")), Err("Not a valid role.".into()));
        assert_eq!(resolve(&option("m", OptionKind::Mentionable, true), &text("x")), Err("Not a valid user or role.".into()));
        assert_eq!(resolve(&option("u", OptionKind::User, true), &text(mention_id("<@!8388608>"))), Ok(Some(json!("8388608"))));
        let picked = ArgValue { text: "Midnight".into(), picked: Some(json!("id-7")), ..Default::default() };
        assert_eq!(resolve(&option("q", OptionKind::String, true), &picked), Ok(Some(json!("id-7"))));
        let mut sized = option("size", OptionKind::String, true);
        sized.choices = vec![crate::app_commands::Choice { name: "Large".into(), name_localized: Some("Groß".into()), value: json!("l") }];
        assert_eq!(resolve(&sized, &text("large")), Ok(Some(json!("l"))));
        assert_eq!(resolve(&sized, &text("Groß")), Ok(Some(json!("l"))));
        assert_eq!(resolve(&sized, &text("huge")), Err("Not a valid choice.".into()));
    }

    #[test]
    fn number_and_length_bounds() {
        let ranged = |min, max| CommandOption { min_value: min, max_value: max, ..option("n", OptionKind::Integer, true) };
        assert_eq!(resolve(&ranged(Some(0.), Some(100.)), &text("101")), Err("Enter a number between 0 and 100.".into()));
        assert_eq!(resolve(&ranged(Some(0.), Some(100.)), &text("100")), Ok(Some(json!(100))));
        assert_eq!(resolve(&ranged(Some(1.5), None), &text("1")), Err("Enter a number greater than or equal to 1.5.".into()));
        assert_eq!(resolve(&ranged(None, Some(-1.)), &text("0")), Err("Enter a number less than or equal to -1.".into()));
        let sized = |min, max| CommandOption { min_length: min, max_length: max, ..option("s", OptionKind::String, true) };
        assert_eq!(resolve(&sized(Some(4), Some(4)), &text("abc")), Err("Enter 4 characters.".into()));
        assert_eq!(resolve(&sized(Some(2), Some(3)), &text("abcd")), Err("Enter between 2 and 3 characters.".into()));
        assert_eq!(resolve(&sized(Some(2), None), &text("a")), Err("Enter 2 or more characters.".into()));
        assert_eq!(resolve(&sized(None, Some(1)), &text("😀")), Err("Enter 1 or fewer characters.".into()), "one emoji is two UTF-16 units");
        assert_eq!(resolve(&sized(None, Some(2)), &text("😀")), Ok(Some(json!("😀"))));
    }

    #[test]
    fn channel_types_and_files() {
        let voice_only = CommandOption { channel_types: vec![2], ..option("c", OptionKind::Channel, true) };
        let channel = |t| ArgValue { text: "55".into(), channel_type: Some(t), ..Default::default() };
        assert_eq!(resolve(&voice_only, &channel(0)), Err("Not a valid channel.".into()));
        assert_eq!(resolve(&voice_only, &channel(2)), Ok(Some(json!("55"))));
        let file = option("f", OptionKind::Attachment, true);
        assert_eq!(resolve(&file, &text("")), Ok(None));
        assert_eq!(resolve(&file, &ArgValue { file: Some(1), ..text("a.png") }), Ok(Some(json!(1))));
    }

    #[test]
    fn autocomplete_sends_the_raw_focus_and_keys_its_cache_on_the_rest() {
        let cmds = vec![command("9", "x", vec![option("q", OptionKind::Integer, true), option("n", OptionKind::Integer, false), option("f", OptionKind::Attachment, false)])];
        let text = "/x q:ab n:3 f:pic";
        let line = parse(text, &cmds, None).unwrap();
        assert_eq!(autocomplete_values(&cmds[0], &line, 0, plain(text)), [("q".into(), json!("ab")), ("n".into(), json!(3))]);
        assert_eq!(autocomplete_context(&cmds[0], &line, 0, text), "q\0n=3\0f=pic");
    }
}
