// People you blocked or ignored: their messages fold in the feed, and they don't ping you.
use std::ops::Range;

use gpui_kit::Context;

use crate::{
    Waffle,
    discord::{Message, Relation},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Hidden {
    Blocked,
    Ignored,
}

// Consecutive messages from one hidden author, folded into one row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Run {
    pub start: usize,
    pub len: usize,
    pub hidden: Hidden,
    // Opened with Show.
    pub shown: bool,
}

impl Waffle {
    pub(crate) fn hidden_author(&self, user_id: &str) -> Option<Hidden> {
        if self.relationships.iter().any(|r| r.kind == Relation::Blocked && r.user.id == user_id) {
            return Some(Hidden::Blocked);
        }
        self.ignored.contains(user_id).then_some(Hidden::Ignored)
    }

    /// The feed's runs, each marked shown once Show opened it.
    pub(crate) fn folds(&self, messages: &[Message]) -> Vec<Run> {
        let mut runs = runs(messages, |id| self.hidden_author(id));
        for run in &mut runs {
            run.shown = self.shown_folds.contains(&messages[run.start].id);
        }
        runs
    }

    pub(crate) fn toggle_fold(&mut self, first_id: String, cx: &mut Context<Self>) {
        if !self.shown_folds.remove(&first_id) {
            self.shown_folds.insert(first_id);
        }
        self.redraw(crate::part::Column::Main, cx);
    }

    /// gpui's list keeps each row's measured height, so rows whose fold changed since the last frame (a block,
    /// an ignore, Show, a message joining a run) are measured again, in the old layout and the new.
    pub(crate) fn refold(&self, channel_id: &str, messages: &[Message], folds: &[Run]) {
        let now: Vec<(String, usize, bool)> = folds.iter().map(|run| (messages[run.start].id.clone(), run.len, run.shown)).collect();
        let mut drawn = self.fold_layout.borrow_mut();
        if drawn.channel_id == channel_id && drawn.folds == now {
            return;
        }
        let count = self.list.item_count();
        let was: Vec<Range<usize>> = match drawn.channel_id == channel_id {
            true => drawn.folds.iter().filter_map(|(id, len, _)| messages.iter().position(|m| &m.id == id).map(|at| at..at + len)).collect(),
            false => Vec::new(),
        };
        for range in was.into_iter().chain(folds.iter().map(|run| run.start..run.start + run.len)) {
            self.list.remeasure_items(range.start.min(count)..range.end.min(count));
        }
        *drawn = FoldLayout { channel_id: channel_id.to_owned(), folds: now };
    }
}

// A channel and its folds as last drawn: each run's first message id, length and whether it's shown.
#[derive(Default)]
pub(crate) struct FoldLayout {
    channel_id: String,
    folds: Vec<(String, usize, bool)>,
}

/// The feed's runs, by author alone: a day boundary inside one doesn't split it. `hidden` is asked once
/// per change of author.
pub(crate) fn runs(messages: &[Message], hidden: impl Fn(&str) -> Option<Hidden>) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut previous: Option<(&str, Option<Hidden>)> = None;
    for (ix, author) in messages.iter().map(|m| m.author.id.as_str()).enumerate() {
        let same = previous.is_some_and(|(p, _)| p == author);
        let kind = if same { previous.and_then(|(_, k)| k) } else { hidden(author) };
        previous = Some((author, kind));
        let Some(kind) = kind else { continue };
        match runs.last_mut() {
            Some(run) if same => run.len += 1,
            _ => runs.push(Run { start: ix, len: 1, hidden: kind, shown: false }),
        }
    }
    runs
}

/// The run holding message `ix`, if any.
pub(crate) fn run_at(runs: &[Run], ix: usize) -> Option<&Run> {
    let run = runs.get(runs.partition_point(|r| r.start + r.len <= ix))?;
    (run.start <= ix).then_some(run)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(authors: &[(&str, &str)]) -> Vec<Message> {
        authors.iter().map(|(id, day)| Message { author: crate::discord::User { id: (*id).into(), ..Default::default() }, timestamp: format!("2026-11-{day}T23:59:00Z"), ..Default::default() }).collect()
    }

    fn hidden(id: &str) -> Option<Hidden> {
        match id {
            "vex" => Some(Hidden::Blocked),
            "dune" => Some(Hidden::Ignored),
            _ => None,
        }
    }

    #[test]
    fn folds_runs_per_author_across_days() {
        let found = runs(&feed(&[("rosa", "20"), ("vex", "20"), ("vex", "20"), ("vex", "21"), ("kai", "21"), ("dune", "21"), ("rosa", "21")]), hidden);
        assert_eq!(found, vec![Run { start: 1, len: 3, hidden: Hidden::Blocked, shown: false }, Run { start: 5, len: 1, hidden: Hidden::Ignored, shown: false }]);
        assert_eq!(run_at(&found, 3), Some(&found[0]));
        assert_eq!(run_at(&found, 4), None);
        assert_eq!(run_at(&found, 5), Some(&found[1]));
        assert_eq!(run_at(&found, 6), None);
    }

    #[test]
    fn another_author_splits_a_run() {
        let found = runs(&feed(&[("vex", "20"), ("dune", "20"), ("vex", "20"), ("kai", "20"), ("vex", "20")]), hidden);
        assert_eq!(found.iter().map(|r| (r.start, r.len)).collect::<Vec<_>>(), vec![(0, 1), (1, 1), (2, 1), (4, 1)]);
    }

    #[test]
    fn single_message_and_nothing_hidden() {
        assert_eq!(runs(&feed(&[("vex", "20")]), hidden), vec![Run { start: 0, len: 1, hidden: Hidden::Blocked, shown: false }]);
        assert!(runs(&feed(&[("rosa", "20"), ("kai", "20")]), hidden).is_empty());
        assert!(runs(&[], hidden).is_empty());
    }
}
