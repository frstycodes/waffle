// What the picker lists: commands grouped by app, ranked and matched against the query.

use super::*;

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

pub(super) const BUILT_IN: &str = "Built-In";

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
pub(super) fn with_built_ins(mut rows: Vec<ListRow>, query: &str, filter: Option<usize>, tts_allowed: bool) -> Vec<ListRow> {
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

pub(super) fn plain_row(command: usize) -> ListRow {
    ListRow::Command { command, name: None, description: None }
}

// Apps with commands, by app frecency score (highest first), then name.
pub(super) fn app_order(index: &CommandIndex, ranks: &Ranks) -> Vec<usize> {
    let mut apps: Vec<usize> = (0..index.apps.len()).filter(|&a| index.commands.iter().any(|c| c.application_id == index.apps[a].id)).collect();
    apps.sort_by(|&a, &b| ranks.app[b].total_cmp(&ranks.app[a]).then_with(|| index.apps[a].name.to_lowercase().cmp(&index.apps[b].name.to_lowercase())));
    apps
}

// The rail: Recent when there is one, then each app in the browser's order.
pub(super) fn browser_sections(index: &CommandIndex, ranks: &Ranks) -> Vec<Section> {
    let recent = (!ranks.top.is_empty()).then_some(Section::Frecency);
    recent.into_iter().chain(app_order(index, ranks).into_iter().map(Section::App)).collect()
}

pub(super) fn matching(index: &CommandIndex, query: &str, ranks: &Ranks) -> Vec<ListRow> {
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
pub(super) fn keeps_trailing_space(c: &AppCommand, q: &str) -> bool {
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

pub(super) fn command_tier(c: &AppCommand, q: &str) -> Option<u8> {
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
pub(super) fn pickable(rows: &[ListRow]) -> Vec<usize> {
    rows.iter().enumerate().filter(|(_, r)| !matches!(r, ListRow::Header(_))).map(|(i, _)| i).collect()
}

pub(super) fn section_of(rows: &[ListRow], row: usize) -> Section {
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
