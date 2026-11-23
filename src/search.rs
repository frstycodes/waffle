// Discord's search syntax: `from:` `mentions:` `has:` `in:` `before:` `after:` `during:` `pinned:` `authorType:`
// become the search endpoint's filters; whatever is left is the text to find. A filter picked from the
// popover sits in the field as an atomic token whose text is that same syntax with an id (`from:42`), so
// typed and picked filters parse alike.
use std::ops::Range;

use chrono::{NaiveDate, TimeZone, Utc};
use gpui_kit::component::input::{InlineToken, InputContent};
use serde::{Deserialize, Serialize};

// Discord's epoch (2015-01-01) in milliseconds; a message id is (ms since it) << 22.
const DISCORD_EPOCH: i64 = 1_420_070_400_000;
// What `has:` takes, as the popover words them. `forward` is the API's `snapshot`.
pub const HAS: [&str; 9] = ["link", "embed", "file", "image", "video", "sound", "sticker", "poll", "forward"];
pub const AUTHOR_TYPES: [&str; 3] = ["user", "bot", "webhook"];
const HISTORY_CAP: usize = 10;
// Discord's page of hits, and the deepest offset it serves (docs.discord.food: offset ≤ 9975).
pub const PAGE_SIZE: usize = 25;
const MAX_OFFSET: usize = 9975;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    From,
    In,
    Has,
    Mentions,
    Before,
    After,
    During,
    Pinned,
    AuthorType,
}

impl Key {
    pub const MAIN: [Key; 4] = [Key::From, Key::In, Key::Has, Key::Mentions];
    pub const MORE: [Key; 5] = [Key::Before, Key::After, Key::During, Key::Pinned, Key::AuthorType];

    pub fn word(self) -> &'static str {
        match self {
            Key::From => "from",
            Key::In => "in",
            Key::Has => "has",
            Key::Mentions => "mentions",
            Key::Before => "before",
            Key::After => "after",
            Key::During => "during",
            Key::Pinned => "pinned",
            Key::AuthorType => "authorType",
        }
    }

    pub fn from_word(word: &str) -> Option<Key> {
        Key::MAIN.into_iter().chain(Key::MORE).find(|k| k.word().eq_ignore_ascii_case(word))
    }

    pub fn title(self) -> &'static str {
        match self {
            Key::From => "From a specific user",
            Key::In => "Sent in a specific channel",
            Key::Has => "Includes a specific type of data",
            Key::Mentions => "Mentions a specific user",
            Key::Before => "Sent before a date",
            Key::After => "Sent after a date",
            Key::During => "Sent on a date",
            Key::Pinned => "Pinned or not",
            Key::AuthorType => "Sent by a user, bot or webhook",
        }
    }

    // What the filter takes, shown after its word.
    pub fn takes(self) -> &'static str {
        match self {
            Key::From | Key::Mentions => "user",
            Key::In => "channel",
            Key::Has => "link, embed or file",
            Key::Before | Key::After | Key::During => "date",
            Key::Pinned => "true or false",
            Key::AuthorType => "user, bot or webhook",
        }
    }

    pub fn is_date(self) -> bool {
        matches!(self, Key::Before | Key::After | Key::During)
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Query {
    pub content: String,
    // (parameter, value) pairs, in Discord's names, ready for the URL. A name may repeat: Discord takes arrays.
    // lint-allow(unbounded-growth): the filters of one parsed query, a handful at most, dropped with the query
    pub params: Vec<(String, String)>,
    // Filters we couldn't resolve, e.g. an unknown user, to tell the reader about.
    // lint-allow(unbounded-growth): the tokens of one parsed query that named no filter, a handful at most, dropped with the query
    pub unresolved: Vec<String>,
}

// `user` and `channel` resolve a name (without @ or #) or an id to an id.
pub fn parse(input: &str, user: impl Fn(&str) -> Option<String>, channel: impl Fn(&str) -> Option<String>) -> Query {
    let mut query = Query::default();
    let mut words: Vec<&str> = Vec::new();
    for token in input.split_whitespace() {
        let Some((key, value)) = token.split_once(':').filter(|(_, v)| !v.is_empty()) else {
            words.push(token);
            continue;
        };
        let Some(key) = Key::from_word(key) else {
            words.push(token);
            continue;
        };
        let value = value.trim_start_matches(['@', '#']);
        let param = match key {
            Key::From => user(value).map(|id| ("author_id", id)),
            Key::Mentions => user(value).map(|id| ("mentions", id)),
            Key::In => channel(value).map(|id| ("channel_id", id)),
            Key::Has => HAS.contains(&value).then(|| ("has", if value == "forward" { "snapshot".to_owned() } else { value.to_owned() })),
            Key::Pinned => (value == "true" || value == "false").then(|| ("pinned", value.to_owned())),
            Key::AuthorType => AUTHOR_TYPES.contains(&value).then(|| ("author_type", value.to_owned())),
            Key::Before => day(value).map(|d| ("max_id", snowflake(d))),
            Key::After => day(value).map(|d| ("min_id", snowflake(d + chrono::Duration::days(1)))),
            Key::During => {
                if let Some(d) = day(value) {
                    query.params.push(("min_id".into(), snowflake(d)));
                    query.params.push(("max_id".into(), snowflake(d + chrono::Duration::days(1))));
                    continue;
                }
                None
            }
        };
        match param {
            Some((name, value)) => query.params.push((name.into(), value)),
            None => query.unresolved.push(token.to_owned()),
        }
    }
    query.content = words.join(" ");
    query
}

// The filter being typed at the end of `text`: its key, the value so far, and the byte where its word
// starts. Nothing while the caret sits right after a token (`token_end`), whose own text is `key:id`.
pub fn typing(text: &str, token_end: usize) -> Option<(Key, &str, usize)> {
    let start = last_word_start(text);
    if start < token_end {
        return None;
    }
    let (key, value) = text[start..].split_once(':')?;
    Some((Key::from_word(key)?, value.trim_start_matches(['@', '#']), start))
}

// The filters whose word starts with the half-typed last word, for "fr" → `from:`.
pub fn keys_matching(text: &str, token_end: usize) -> Vec<Key> {
    let start = last_word_start(text);
    let word = &text[start..];
    if start < token_end || word.is_empty() || word.contains(':') {
        return Vec::new();
    }
    Key::MAIN.into_iter().chain(Key::MORE).filter(|k| k.word().to_ascii_lowercase().starts_with(&word.to_ascii_lowercase())).collect()
}

pub fn last_word_start(text: &str) -> usize {
    text.char_indices().rev().find(|(_, c)| c.is_whitespace()).map_or(0, |(i, c)| i + c.len_utf8())
}

// Which order results come in: Discord's `sort_by` and `sort_order`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    #[default]
    Newest,
    Oldest,
    Relevant,
}

impl Sort {
    pub const ALL: [Sort; 3] = [Sort::Newest, Sort::Oldest, Sort::Relevant];

    pub fn label(self) -> &'static str {
        match self {
            Sort::Newest => "Newest first",
            Sort::Oldest => "Oldest first",
            Sort::Relevant => "Most relevant",
        }
    }

    pub fn params(self) -> [(String, String); 2] {
        let (by, order) = match self {
            Sort::Newest => ("timestamp", "desc"),
            Sort::Oldest => ("timestamp", "asc"),
            Sort::Relevant => ("relevance", "desc"),
        };
        [("sort_by".into(), by.into()), ("sort_order".into(), order.into())]
    }
}

// How many pages `total` hits make, within what Discord will page to.
pub fn page_count(total: u32) -> usize {
    (total as usize).div_ceil(PAGE_SIZE).min(MAX_OFFSET / PAGE_SIZE + 1)
}

pub fn offset(page: usize) -> usize {
    (page.saturating_sub(1) * PAGE_SIZE).min(MAX_OFFSET)
}

// The page buttons for 1-based `current` of `count`: the ends, the current page and its neighbours, `None`
// where pages are skipped. On page 1 of 36: 1 2 3 … 36.
pub fn pages(current: usize, count: usize) -> Vec<Option<usize>> {
    if count <= 7 {
        return (1..=count).map(Some).collect();
    }
    let around = if current <= 3 { 1..=3.max(current + 1) } else if current + 2 >= count { count - 2..=count } else { current - 1..=current + 1 };
    let mut shown: Vec<usize> = std::iter::once(1).chain(around).chain(std::iter::once(count)).collect();
    shown.dedup();
    let mut out = Vec::new();
    for (i, page) in shown.iter().enumerate() {
        if i > 0 && page - shown[i - 1] > 1 {
            out.push(None);
        }
        out.push(Some(*page));
    }
    out
}

// A filter value from a fixed set, as (value for the token's text, label): `has:` kinds, true/false,
// author types, dates.
pub fn fixed_value(key: Key, value: &str) -> Option<(String, String)> {
    let value = value.to_ascii_lowercase();
    match key {
        Key::Has if HAS.contains(&value.as_str()) => Some((value.clone(), value)),
        Key::Pinned if value == "true" || value == "false" => Some((value.clone(), value)),
        Key::AuthorType if AUTHOR_TYPES.contains(&value.as_str()) => Some((value.clone(), value)),
        Key::Before | Key::After | Key::During => day(&value).map(|d| (day_text(d), day_label(d))),
        _ => None,
    }
}

// Turns filters written out as text (typed, or pasted from elsewhere) into tokens: `from:wren` or
// `from: wren` once a space follows it, a fixed value (`has:image`) even at the very end. `resolve` maps a
// person or channel to (id, label). None when nothing new was found.
pub fn chip(content: &InputContent, resolve: impl Fn(Key, &str) -> Option<(String, String)>) -> Option<InputContent> {
    let text = content.text().as_ref();
    let tokens = content.tokens();
    let words: Vec<Range<usize>> = word_ranges(text).filter(|w| !tokens.iter().any(|t| t.range().contains(&w.start))).collect();
    let mut found: Vec<(Range<usize>, InlineToken)> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let word = words[i].clone();
        i += 1;
        let Some((key, value)) = text[word.clone()].split_once(':') else { continue };
        let Some(key) = Key::from_word(key) else { continue };
        // `from: wren`: the value is the next word.
        let (end, value) = match (value.is_empty(), words.get(i)) {
            (false, _) => (word.end, value),
            (true, Some(next)) => {
                i += 1;
                (next.end, &text[next.clone()])
            }
            (true, None) => continue,
        };
        let value = value.trim_start_matches(['@', '#']);
        let fixed = fixed_value(key, value);
        let closed = text[end..].starts_with(char::is_whitespace) || (end == text.len() && fixed.is_some());
        let Some((id, label)) = closed.then(|| fixed.or_else(|| resolve(key, value))).flatten() else { continue };
        let token = format!("{}:{id}", key.word());
        found.push((word.start..end, InlineToken::new(token.clone(), token).with_label(label)));
    }
    if found.is_empty() {
        return None;
    }
    let mut items: Vec<(Range<usize>, InlineToken)> = tokens.iter().map(|s| (s.range(), s.token().clone())).chain(found).collect();
    items.sort_by_key(|(range, _)| range.start);
    let (mut out, mut spans, mut at) = (String::new(), Vec::new(), 0);
    for (range, token) in items {
        out.push_str(&text[at..range.start]);
        let start = out.len();
        out.push_str(token.text());
        spans.push((start..out.len(), token));
        at = range.end;
    }
    out.push_str(&text[at..]);
    spans.into_iter().try_fold(InputContent::new(out), |content, (range, token)| content.with_token(range, token)).ok()
}

// The field without the token whose text is `token` (and the space after it).
pub fn without_token(content: &InputContent, token: &str) -> InputContent {
    let text = content.text().as_ref();
    let Some(gone) = content.tokens().iter().find(|t| t.token().text().as_ref() == token).map(|t| t.range()) else { return content.clone() };
    let end = if text[gone.end..].starts_with(' ') { gone.end + 1 } else { gone.end };
    let out = format!("{}{}", &text[..gone.start], &text[end..]);
    let shift = end - gone.start;
    content.tokens().iter().filter(|t| t.range() != gone).fold(InputContent::new(out), |next, t| {
        let range = t.range();
        let range = if range.start >= end { range.start - shift..range.end - shift } else { range };
        next.clone().with_token(range, t.token().clone()).unwrap_or(next)
    })
}

// The field with a token for `token` (labelled `label`) added after its last token, before the free text.
pub fn with_token(content: &InputContent, token: &str, label: &str) -> InputContent {
    let text = content.text().as_ref();
    let at = content.tokens().last().map_or(0, |t| t.range().end);
    let at = if text[at..].starts_with(' ') { at + 1 } else { at };
    let gap = if at > 0 && !text[..at].ends_with(' ') { " " } else { "" };
    let out = format!("{}{gap}{token} {}", &text[..at], &text[at..]);
    let start = at + gap.len();
    let shift = gap.len() + token.len() + 1;
    let next = content.tokens().iter().fold(InputContent::new(out), |next, t| {
        let range = t.range();
        let range = if range.start >= at { range.start + shift..range.end + shift } else { range };
        next.clone().with_token(range, t.token().clone()).unwrap_or(next)
    });
    next.clone().with_token(start..start + token.len(), InlineToken::new(token.to_owned(), token.to_owned()).with_label(label.to_owned())).unwrap_or(next)
}

fn word_ranges(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    text.split_whitespace().map(move |w| {
        let start = w.as_ptr() as usize - text.as_ptr() as usize;
        start..start + w.len()
    })
}

pub fn day_text(day: NaiveDate) -> String {
    day.format("%Y-%m-%d").to_string()
}

pub fn day_label(day: NaiveDate) -> String {
    day.format("%b %-d, %Y").to_string()
}

fn day(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()
}

// The first possible message id on a UTC day.
fn snowflake(day: NaiveDate) -> String {
    let ms = Utc.from_utc_datetime(&day.and_hms_opt(0, 0, 0).expect("midnight")).timestamp_millis();
    (((ms - DISCORD_EPOCH).max(0) as u64) << 22).to_string()
}

// A past search as the field held it: its text and each token's byte range and label.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub text: String,
    pub tokens: Vec<SavedToken>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedToken {
    pub start: usize,
    pub end: usize,
    pub label: String,
}

impl Saved {
    pub fn from_content(content: &InputContent) -> Self {
        let tokens = content.tokens().iter().map(|s| SavedToken { start: s.range().start, end: s.range().end, label: s.token().label().to_string() }).collect();
        Self { text: content.text().trim_end().to_owned(), tokens }
    }

    // Tokens that no longer fit the text (a hand-edited prefs file) are dropped, keeping their text.
    pub fn to_content(&self) -> InputContent {
        self.tokens.iter().fold(InputContent::new(self.text.clone()), |content, t| {
            let Some(text) = self.text.get(t.start..t.end) else { return content };
            let token = InlineToken::new(text.to_owned(), text.to_owned()).with_label(t.label.clone());
            content.clone().with_token(t.start..t.end, token).unwrap_or(content)
        })
    }

    // The search as words and token labels, in order: `from:` Rosa `in:` #clips dodge.
    pub fn parts(&self) -> Vec<(Option<&str>, &str)> {
        let mut parts = Vec::new();
        let mut at = 0;
        for t in &self.tokens {
            let Some(text) = self.text.get(t.start..t.end) else { continue };
            push_words(&mut parts, self.text.get(at..t.start).unwrap_or(""));
            parts.push((text.split_once(':').map(|(k, _)| k), t.label.as_str()));
            at = t.end;
        }
        push_words(&mut parts, self.text.get(at..).unwrap_or(""));
        parts
    }
}

fn push_words<'a>(parts: &mut Vec<(Option<&'a str>, &'a str)>, text: &'a str) {
    let text = text.trim();
    if !text.is_empty() {
        parts.push((None, text));
    }
}

// Newest first, each search once, at most `HISTORY_CAP`.
pub fn remember(history: &mut Vec<Saved>, search: Saved) {
    history.retain(|s| s.text != search.text);
    history.insert(0, search);
    history.truncate(HISTORY_CAP);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(n: &str) -> Option<String> {
        (n == "kiwi" || n == "42").then(|| "42".to_owned())
    }

    fn channel(n: &str) -> Option<String> {
        (n == "general" || n == "7").then(|| "7".to_owned())
    }

    #[test]
    fn filters_become_parameters() {
        let q = parse("rust from:@kiwi in:#general has:image pinned:true gpui", user, channel);
        assert_eq!(q.content, "rust gpui");
        assert_eq!(q.params, [("author_id".into(), "42".into()), ("channel_id".into(), "7".into()), ("has".into(), "image".into()), ("pinned".into(), "true".into())]);
        let q = parse("from:nobody has:spaceship", user, channel);
        assert_eq!(q.unresolved, ["from:nobody", "has:spaceship"]);
        assert_eq!(q.content, "");
    }

    #[test]
    fn tokens_parse_like_typed_filters() {
        let q = parse("from:42 mentions:42 in:7 has:forward authorType:bot dodge", user, channel);
        assert_eq!(q.content, "dodge");
        let names: Vec<&str> = q.params.iter().map(|(n, v)| if n == "has" { v.as_str() } else { n.as_str() }).collect();
        assert_eq!(names, ["author_id", "mentions", "channel_id", "snapshot", "author_type"]);
        assert_eq!(parse("authortype:robot", user, channel).unresolved, ["authortype:robot"]);
    }

    #[test]
    fn dates_bound_message_ids() {
        // 2015-01-02 is one day after Discord's epoch.
        let day = (86_400_000u64 << 22).to_string();
        let two = ((2 * 86_400_000u64) << 22).to_string();
        assert_eq!(parse("before:2015-01-02", user, channel).params, [("max_id".into(), day.clone())]);
        // After a day means from the start of the next one.
        assert_eq!(parse("after:2015-01-02", user, channel).params, [("min_id".into(), two.clone())]);
        assert_eq!(parse("during:2015-01-02", user, channel).params, [("min_id".into(), day), ("max_id".into(), two)]);
        assert_eq!(parse("before:yesterday", user, channel).unresolved, ["before:yesterday"]);
    }

    #[test]
    fn typing_finds_the_open_filter() {
        assert_eq!(typing("dodge from:ro", 0), Some((Key::From, "ro", 6)));
        assert_eq!(typing("in:#gen", 0), Some((Key::In, "gen", 0)));
        assert_eq!(typing("dodge ", 0), None);
        // Right after a token, its own `from:42` isn't a filter being typed.
        assert_eq!(typing("from:42", 7), None);
        assert_eq!(keys_matching("dodge fr", 0), [Key::From]);
        assert_eq!(keys_matching("dodge ", 0), []);
    }

    #[test]
    fn history_is_newest_first_once_and_capped() {
        let saved = |text: &str| Saved { text: text.into(), tokens: Vec::new() };
        let mut history = Vec::new();
        for i in 0..12 {
            remember(&mut history, saved(&format!("q{i}")));
        }
        assert_eq!(history.len(), HISTORY_CAP);
        assert_eq!(history[0].text, "q11");
        remember(&mut history, saved("q5"));
        assert_eq!(history.len(), HISTORY_CAP);
        assert_eq!((history[0].text.as_str(), history.iter().filter(|s| s.text == "q5").count()), ("q5", 1));
    }

    #[test]
    fn saved_searches_keep_their_tokens() {
        let token = InlineToken::new("from:42", "from:42").with_label("Rosa");
        let content = InputContent::new("from:42 dodge ").with_token(0..7, token).expect("token fits");
        let saved = Saved::from_content(&content);
        assert_eq!(saved.text, "from:42 dodge");
        assert_eq!(saved.parts(), [(Some("from"), "Rosa"), (None, "dodge")]);
        assert_eq!(saved.to_content().tokens().len(), 1);
        // A range that no longer fits the text drops the token, not the search.
        let stale = Saved { text: "dodge".into(), tokens: vec![SavedToken { start: 0, end: 40, label: "Rosa".into() }] };
        assert_eq!(stale.to_content().tokens().len(), 0);
    }
    #[test]
    fn written_filters_become_tokens() {
        let resolve = |key: Key, v: &str| match key {
            Key::From if v.eq_ignore_ascii_case("wren") => Some(("8".to_owned(), "Wren".to_owned())),
            _ => None,
        };
        let chipped = chip(&InputContent::new("from:_nobody from: Wren has:image"), resolve).expect("chips");
        assert_eq!(chipped.text().as_ref(), "from:_nobody from:8 has:image");
        let labels: Vec<&str> = chipped.tokens().iter().map(|t| t.token().label().as_ref()).collect();
        assert_eq!(labels, ["Wren", "image"]);
        // A person still being typed at the end waits for a space; a fixed value doesn't.
        assert!(chip(&InputContent::new("from:wren"), resolve).is_none());
        assert_eq!(chip(&InputContent::new("before:2026-11-01"), resolve).map(|c| c.tokens()[0].token().label().to_string()), Some("Nov 1, 2026".into()));
        // Tokens already there stay, shifted past the rewrite.
        let again = chip(&InputContent::new("from:Wren x"), resolve).expect("chips");
        let more = chip(&InputContent::new(format!("{} pinned:true", again.text())).with_token(0..6, again.tokens()[0].token().clone()).expect("fits"), resolve).expect("chips");
        assert_eq!((more.text().as_ref(), more.tokens().len()), ("from:8 x pinned:true", 2));
    }

    #[test]
    fn page_buttons_show_ends_and_neighbours() {
        let shown = |current, count| pages(current, count).into_iter().map(|p| p.map_or("…".to_owned(), |p| p.to_string())).collect::<Vec<_>>().join(" ");
        assert_eq!(shown(1, 36), "1 2 3 … 36");
        assert_eq!(shown(5, 36), "1 … 4 5 6 … 36");
        assert_eq!(shown(36, 36), "1 … 34 35 36");
        assert_eq!(shown(2, 5), "1 2 3 4 5");
        assert_eq!(page_count(880), 36);
        assert_eq!(page_count(1_000_000), 400);
        assert_eq!(offset(3), 50);
    }
    #[test]
    fn filters_popover_edits_keep_other_tokens() {
        let start = with_token(&InputContent::new("dodge"), "from:8", "Wren");
        assert_eq!(start.text().as_ref(), "from:8 dodge");
        let both = with_token(&start, "has:image", "image");
        assert_eq!(both.text().as_ref(), "from:8 has:image dodge");
        assert_eq!(both.tokens().len(), 2);
        let less = without_token(&both, "from:8");
        assert_eq!(less.text().as_ref(), "has:image dodge");
        assert_eq!(less.tokens()[0].range(), 0..9);
    }
}
