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

/// Reads `text` as a command from `commands`: the longest command name it starts with (`preferred`, an
/// `AppCommand::id`, wins between two apps' same-named commands), then every ` name:` that names one of its
/// options. A value runs until the next such key, so it can hold spaces; unknown names are plain text and
/// only an option's first key counts.
pub(crate) fn parse(text: &str, commands: &[AppCommand], preferred: Option<&str>) -> Option<CommandLine> {
    let rest = text.strip_prefix('/')?;
    let named = |c: &AppCommand| {
        let name = c.name();
        let after = rest.strip_prefix(name.as_str())?;
        (after.is_empty() || after.starts_with(char::is_whitespace)).then_some(name.len())
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

// Every option key after the command name, each option's first only, in text order.
fn keys(text: &str, from: usize, options: &[CommandOption]) -> Vec<(usize, Range<usize>)> {
    let mut found: Vec<(usize, Range<usize>)> = Vec::new();
    let mut previous = text[..from].chars().next_back();
    for (at, c) in text[from..].char_indices().map(|(i, c)| (from + i, c)) {
        let starts_word = previous.is_some_and(char::is_whitespace);
        previous = Some(c);
        if !starts_word {
            continue;
        }
        let hit = options.iter().enumerate().find(|(i, o)| !found.iter().any(|(f, _)| f == i) && text[at..].strip_prefix(o.name.as_str()).is_some_and(|r| r.starts_with(':')));
        if let Some((i, o)) = hit {
            found.push((i, at..at + o.name.len() + 1));
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
}

/// Why a command can't run yet.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Blocked {
    // A required option with no key at all: its index into the command's options.
    Missing(usize),
    // An arg (by position in `CommandLine::args`) that's empty though required, or can't be sent.
    Invalid(usize, String),
}

impl Blocked {
    pub(crate) fn message(&self, command: &AppCommand) -> String {
        match self {
            Self::Missing(option) => missing(&command.options[*option]),
            Self::Invalid(_, why) => why.clone(),
        }
    }
}

fn missing(option: &CommandOption) -> String {
    format!("Fill in {} to run this command", option.name)
}

/// Every given option's value, or the first thing that stops the command: a bad or empty value in text
/// order, then a required option that was never given.
pub(crate) fn command_values(command: &AppCommand, line: &CommandLine, value_of: impl Fn(&Arg) -> ArgValue) -> Result<Vec<(String, Value)>, Blocked> {
    let mut values = Vec::new();
    for (n, arg) in line.args.iter().enumerate() {
        let option = &command.options[arg.option];
        match resolve(option, &value_of(arg)) {
            Ok(Some(value)) => values.push((option.name.clone(), value)),
            Ok(None) if option.required => return Err(Blocked::Invalid(n, missing(option))),
            Ok(None) => {}
            Err(why) => return Err(Blocked::Invalid(n, why)),
        }
    }
    if let Some(option) = first_missing(command, line) {
        return Err(Blocked::Missing(option));
    }
    Ok(values)
}

/// The first required option the line has no key for.
pub(crate) fn first_missing(command: &AppCommand, line: &CommandLine) -> Option<usize> {
    (0..command.options.len()).find(|&i| command.options[i].required && !line.has(i))
}

/// What a value holds: its text (a mention chip's id, for a mention), and the value of a picked choice
/// whose label it still is.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ArgValue {
    pub text: String,
    pub picked: Option<Value>,
}

/// An option's value as Discord takes it: `Ok(None)` when it's left empty, `Err` with why it can't be sent.
pub(crate) fn resolve(option: &CommandOption, input: &ArgValue) -> Result<Option<Value>, String> {
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

// Not verified: Discord's client sends the focused option's raw text as a string, whatever its type,
// alongside every other option that already has a value.
pub(crate) fn autocomplete_values(command: &AppCommand, line: &CommandLine, focused: usize, value_of: impl Fn(&Arg) -> ArgValue) -> Vec<(String, Value)> {
    line.args
        .iter()
        .enumerate()
        .filter_map(|(n, arg)| {
            let option = &command.options[arg.option];
            match n == focused {
                true => Some((option.name.clone(), json!(value_of(arg).text))),
                false => resolve(option, &value_of(arg)).ok().flatten().map(|v| (option.name.clone(), v)),
            }
        })
        .collect()
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

    fn plain(text: &str) -> impl Fn(&Arg) -> ArgValue + '_ {
        move |a| ArgValue { text: text[a.value.clone()].to_owned(), picked: None }
    }

    #[test]
    fn running_needs_every_required_option_and_valid_values() {
        let cmds = commands();
        let text = "/play volume:80";
        let line = parse(text, &cmds, None).unwrap();
        assert_eq!(command_values(&cmds[0], &line, plain(text)), Err(Blocked::Missing(0)));
        assert_eq!(Blocked::Missing(0).message(&cmds[0]), "Fill in query to run this command");
        let text = "/play query: volume:80";
        let line = parse(text, &cmds, None).unwrap();
        assert_eq!(command_values(&cmds[0], &line, plain(text)), Err(Blocked::Invalid(0, "Fill in query to run this command".into())));
        let text = "/play query:song volume:loud";
        let line = parse(text, &cmds, None).unwrap();
        assert_eq!(command_values(&cmds[0], &line, plain(text)), Err(Blocked::Invalid(1, "Enter a whole number".into())));
        let text = "/play volume:80 query:a song";
        let line = parse(text, &cmds, None).unwrap();
        assert_eq!(command_values(&cmds[0], &line, plain(text)), Ok(vec![("volume".into(), json!(80)), ("query".into(), json!("a song"))]));
    }

    #[test]
    fn validates_numbers_choices_and_picks() {
        let text = |t: &str| ArgValue { text: t.into(), picked: None };
        assert_eq!(resolve(&option("n", OptionKind::Integer, true), &text("42")), Ok(Some(json!(42))));
        assert!(resolve(&option("n", OptionKind::Integer, true), &text("4.2")).is_err());
        assert_eq!(resolve(&option("n", OptionKind::Number, true), &text("4.5")), Ok(Some(json!(4.5))));
        assert!(resolve(&option("n", OptionKind::Number, true), &text("NaN")).is_err());
        assert_eq!(resolve(&option("b", OptionKind::Boolean, true), &text("TRUE")), Ok(Some(json!(true))));
        assert!(resolve(&option("u", OptionKind::User, true), &text("mira")).is_err());
        assert_eq!(resolve(&option("u", OptionKind::User, true), &text(mention_id("<@!8388608>"))), Ok(Some(json!("8388608"))));
        let picked = ArgValue { text: "Midnight".into(), picked: Some(json!("id-7")) };
        assert_eq!(resolve(&option("q", OptionKind::String, true), &picked), Ok(Some(json!("id-7"))));
        let mut sized = option("size", OptionKind::String, true);
        sized.choices = vec![crate::app_commands::Choice { name: "Large".into(), name_localized: None, value: json!("l") }];
        assert_eq!(resolve(&sized, &text("large")), Ok(Some(json!("l"))));
        assert!(resolve(&sized, &text("huge")).is_err());
        assert!(resolve(&option("f", OptionKind::Attachment, false), &text("")).is_err());
    }
}
