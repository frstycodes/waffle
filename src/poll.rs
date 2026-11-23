// Live polls (design/messages.html `.poll`, design/media.html #poll-creator): voting on a message's poll,
// its numbers, and "Create a poll", which the composer's + menu opens.
use chrono::{DateTime, Utc};
use gpui_kit::{
    AnyElement, Context, Entity, Focusable, FontWeight, MouseButton, SharedString, Window,
    component::{
        Icon,
        input::{Input, InputEvent, InputState},
    },
    div,
    prelude::*,
};
use reqwest::Method;
use serde_json::json;

use crate::{
    EmojiTarget, Waffle,
    discord::{Emoji, Message, Poll, PollAnswer, PollDraft, SendState},
    icons::Glyph,
    menu::{self, emoji_face},
    palette,
    ui::{self, Tone},
};

// Discord's limits for a poll.
const QUESTION_MAX: usize = 300;
const ANSWER_MAX: usize = 55;
const ANSWERS_MAX: usize = 10;
const ANSWERS_MIN: usize = 2;
// How long a poll runs, in hours, as Discord's creator offers it; 24 hours first selected.
const DURATIONS: [(u32, &str); 6] = [(1, "1 hour"), (4, "4 hours"), (8, "8 hours"), (24, "24 hours"), (72, "3 days"), (168, "1 week")];
const DEFAULT_DURATION: usize = 3;
const DIALOG_WIDTH: f32 = 480.;

pub(crate) struct PollCreator {
    question: Entity<InputState>,
    answers: Vec<AnswerRow>,
    duration: usize,
    multiselect: bool,
    problems: Problems,
    // "#general", or None in a DM.
    channel: Option<String>,
}

struct AnswerRow {
    input: Entity<InputState>,
    emoji: Option<Emoji>,
}

// What stops a poll from posting, named under the field it's about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Problems {
    question: Option<&'static str>,
    answers: Option<&'static str>,
}

impl Problems {
    fn any(self) -> bool {
        self != Problems::default()
    }
}

impl Poll {
    pub(crate) fn voted(&self) -> bool {
        self.answers.iter().any(|a| a.me)
    }

    // Not verified: Discord's "N votes" on a multiple-answer poll may count voters, not answers picked.
    pub(crate) fn total_votes(&self) -> u64 {
        self.answers.iter().map(|a| a.count).sum()
    }

    pub(crate) fn closed(&self, now: DateTime<Utc>) -> bool {
        self.finalized || ends_at(self).is_some_and(|end| end <= now)
    }

    pub(crate) fn prompt(&self, now: DateTime<Utc>) -> &'static str {
        match (self.closed(now), self.multiselect) {
            (true, _) => "Final results",
            (false, true) => "Select one or more answers",
            (false, false) => "Select one answer",
        }
    }

    // "141 votes · Ends in 3 hours", or "· Poll closed" once it has.
    pub(crate) fn foot(&self, now: DateTime<Utc>) -> String {
        let votes = votes_label(self.total_votes());
        if self.closed(now) {
            return format!("{votes} · Poll closed");
        }
        match ends_at(self) {
            Some(end) => format!("{votes} · Ends in {}", time_left(end - now)),
            None => votes,
        }
    }

    // Each answer's share of the votes as a whole percent, in answer order.
    pub(crate) fn shares(&self) -> Vec<u32> {
        let total = self.total_votes();
        self.answers.iter().map(|a| (a.count * 100 + total / 2).checked_div(total).unwrap_or(0) as u32).collect()
    }
}

fn ends_at(poll: &Poll) -> Option<DateTime<Utc>> {
    poll.expiry.as_deref().and_then(|t| DateTime::parse_from_rfc3339(t).ok()).map(|t| t.with_timezone(&Utc))
}

pub(crate) fn votes_label(count: u64) -> String {
    format!("{count} {}", if count == 1 { "vote" } else { "votes" })
}

// The largest whole unit left, rounded up under an hour so the last minute still reads "1 minute".
fn time_left(left: chrono::Duration) -> String {
    let minutes = (left.num_seconds().max(0) + 59) / 60;
    let (n, unit) = match minutes {
        m if m < 60 => (m.max(1), "minute"),
        m if m < 24 * 60 => (m / 60, "hour"),
        m => (m / (24 * 60), "day"),
    };
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

/// Your vote as `picks` (none: removed), counted at once: answers you leave lose your vote.
pub(crate) fn cast(poll: &mut Poll, picks: &[u32]) {
    for answer in &mut poll.answers {
        let on = picks.contains(&answer.id);
        if answer.me != on {
            answer.count = if on { answer.count + 1 } else { answer.count.saturating_sub(1) };
            answer.me = on;
        }
    }
}

/// A vote from the gateway. Your own arrives after `cast` already counted it, so it only lands once.
pub(crate) fn apply_vote(poll: &mut Poll, answer_id: u32, added: bool, mine: bool) {
    let Some(answer) = poll.answers.iter_mut().find(|a| a.id == answer_id) else { return };
    if mine && answer.me == added {
        return;
    }
    answer.count = if added { answer.count + 1 } else { answer.count.saturating_sub(1) };
    if mine {
        answer.me = added;
    }
}

/// A failed poll's draft again, for Resend: its run length from when it was sent until it would end.
pub(crate) fn draft_of(message: &Message) -> Option<PollDraft> {
    let poll = message.poll.as_ref()?;
    let sent = DateTime::parse_from_rfc3339(&message.timestamp).ok()?.with_timezone(&Utc);
    let hours = ends_at(poll).map_or(DURATIONS[DEFAULT_DURATION].0, |end| ((end - sent).num_minutes() as f64 / 60.).round() as u32);
    Some(PollDraft { question: poll.question.clone(), answers: poll.answers.iter().map(|a| (a.text.clone(), a.emoji.clone())).collect(), hours, multiselect: poll.multiselect })
}

// How the pending message shows the poll until Discord answers. Not verified: Discord numbers answers from 1.
fn preview(draft: &PollDraft, now: DateTime<Utc>) -> Poll {
    Poll {
        question: draft.question.clone(),
        answers: draft.answers.iter().zip(1..).map(|((text, emoji), id)| PollAnswer { id, text: text.clone(), emoji: emoji.clone(), count: 0, me: false }).collect(),
        expiry: Some((now + chrono::Duration::hours(draft.hours.into())).to_rfc3339()),
        multiselect: draft.multiselect,
        finalized: false,
    }
}

// `answers` are the filled-in ones, trimmed.
fn check(question: &str, answers: &[&str]) -> Problems {
    let question = if question.is_empty() {
        Some("Add a question to post the poll")
    } else if question.chars().count() > QUESTION_MAX {
        Some("Keep the question to 300 characters")
    } else {
        None
    };
    let answers = if answers.len() < ANSWERS_MIN {
        Some("Fill in at least two answers")
    } else if answers.iter().any(|a| a.chars().count() > ANSWER_MAX) {
        Some("Keep each answer to 55 characters")
    } else {
        None
    };
    Problems { question, answers }
}

// "3 of 10", and why Add answer is off at the limit.
fn answer_count(rows: usize) -> String {
    if rows >= ANSWERS_MAX {
        return format!("{ANSWERS_MAX} of {ANSWERS_MAX}, the most a poll can have");
    }
    format!("{rows} of {ANSWERS_MAX}")
}

// Voting on a message's poll.
impl Waffle {
    // A pick before Vote: one answer replaces another, unless the poll takes several.
    pub(crate) fn pick_poll_answer(&mut self, message_id: String, answer_id: u32, multiselect: bool, cx: &mut Context<Self>) {
        let picks = self.poll_picks.entry(message_id).or_default();
        match picks.iter().position(|&id| id == answer_id) {
            Some(ix) => {
                picks.remove(ix);
            }
            None if multiselect => picks.push(answer_id),
            None => *picks = vec![answer_id],
        }
        cx.notify(); // lint-allow(global-notify): message components, polls and pending commands draw in the feed and the thread panel
    }

    pub(crate) fn toggle_poll_results(&mut self, message_id: String, cx: &mut Context<Self>) {
        if !self.poll_peek.remove(&message_id) {
            self.poll_peek.insert(message_id);
        }
        cx.notify(); // lint-allow(global-notify): message components, polls and pending commands draw in the feed and the thread panel
    }

    /// Votes for `picks`, or removes your vote when empty; shown at once, put back if Discord refuses.
    pub(crate) fn vote(&mut self, channel_id: String, message_id: String, picks: Vec<u32>, cx: &mut Context<Self>) {
        let Some(before) = self.message_mut(&channel_id, &message_id).and_then(|m| m.poll.clone()) else { return };
        self.poll_picks.remove(&message_id);
        self.poll_peek.remove(&message_id);
        let path = format!("/channels/{channel_id}/polls/{message_id}/answers/@me");
        let body = json!({ "answer_ids": picks.iter().map(u32::to_string).collect::<Vec<_>>() });
        let (c1, i1) = (channel_id.clone(), message_id.clone());
        self.rest_optimistic(
            Method::PUT,
            path,
            Some(body),
            move |this| {
                if let Some(poll) = this.message_mut(&c1, &i1).and_then(|m| m.poll.as_mut()) {
                    cast(poll, &picks);
                }
            },
            move |this| {
                if let Some(m) = this.message_mut(&channel_id, &message_id) {
                    m.poll = Some(before);
                }
            },
            cx,
        );
    }
}

// The creator.
impl Waffle {
    pub(crate) fn open_poll_creator(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(channel) = self.channel.as_ref() else { return };
        let label = self.guild_id.is_some().then(|| format!("#{}", channel.name));
        let question = self.poll_input("What do you want to ask?", window, cx);
        let answers = (0..ANSWERS_MIN).map(|_| AnswerRow { input: self.poll_input("Type an answer", window, cx), emoji: None }).collect();
        question.update(cx, |state, cx| state.focus(window, cx));
        self.menu = None;
        self.poll_creator = Some(PollCreator { question, answers, duration: DEFAULT_DURATION, multiselect: false, problems: Problems::default(), channel: label });
        cx.notify(); // lint-allow(global-notify): the poll creator is drawn by the root's overlays, over every column
    }

    fn poll_input(&self, placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        cx.subscribe_in(&input, window, Self::on_poll_input).detach(); // lint-allow(detached-task): the subscription ends when the input or Waffle is dropped
        input
    }

    // Enter moves to the next answer, adding one after the last; once Post has named a problem, it clears
    // as soon as it's fixed.
    fn on_poll_input(&mut self, input: &Entity<InputState>, event: &InputEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(creator) = self.poll_creator.as_ref() else { return };
        match event {
            InputEvent::PressEnter { .. } => {
                let next = match creator.answers.iter().position(|a| &a.input == input) {
                    Some(ix) => ix + 1,
                    None => 0,
                };
                self.focus_poll_answer(next, window, cx);
            }
            InputEvent::Change => {
                if creator.problems.any() {
                    let problems = self.poll_problems(cx);
                    if let Some(creator) = self.poll_creator.as_mut() {
                        creator.problems = problems;
                    }
                }
                cx.notify(); // lint-allow(global-notify): the poll creator is drawn by the root's overlays, over every column
            }
            _ => {}
        }
    }

    // Focuses answer `ix`, adding it when it's one past the last and there's room.
    fn focus_poll_answer(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(count) = self.poll_creator.as_ref().map(|c| c.answers.len()) else { return };
        if ix == count && count < ANSWERS_MAX {
            let input = self.poll_input("Type an answer", window, cx);
            if let Some(creator) = self.poll_creator.as_mut() {
                creator.answers.push(AnswerRow { input, emoji: None });
            }
        }
        let Some(row) = self.poll_creator.as_ref().and_then(|c| c.answers.get(ix)) else { return };
        row.input.clone().update(cx, |state, cx| state.focus(window, cx));
        cx.notify(); // lint-allow(global-notify): the poll creator is drawn by the root's overlays, over every column
    }

    // Demo scenes fill the creator as if typed; the last row keeps focus.
    #[cfg(debug_assertions)]
    pub(crate) fn fill_poll_creator(&mut self, question: &str, answers: &[(&str, Option<&str>)], window: &mut Window, cx: &mut Context<Self>) {
        for (ix, (text, emoji)) in answers.iter().enumerate() {
            self.focus_poll_answer(ix, window, cx);
            let Some(row) = self.poll_creator.as_mut().and_then(|c| c.answers.get_mut(ix)) else { return };
            row.emoji = emoji.map(Emoji::unicode);
            row.input.clone().update(cx, |state, cx| state.set_value(text.to_string(), window, cx));
        }
        if let Some(creator) = self.poll_creator.as_ref() {
            creator.question.clone().update(cx, |state, cx| state.set_value(question.to_owned(), window, cx));
        }
    }

    fn remove_poll_answer(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(creator) = self.poll_creator.as_mut().filter(|c| c.answers.len() > ANSWERS_MIN) else { return };
        creator.answers.remove(ix);
        cx.notify(); // lint-allow(global-notify): the poll creator is drawn by the root's overlays, over every column
    }

    pub(crate) fn set_poll_emoji(&mut self, ix: usize, emoji: Emoji) {
        if let Some(row) = self.poll_creator.as_mut().and_then(|c| c.answers.get_mut(ix)) {
            row.emoji = Some(emoji);
        }
    }

    fn close_poll_creator(&mut self, cx: &mut Context<Self>) {
        self.poll_creator = None;
        cx.notify(); // lint-allow(global-notify): the poll creator is drawn by the root's overlays, over every column
    }

    // The draft as typed: the question and the filled-in answers, trimmed.
    fn poll_draft(&self, cx: &Context<Self>) -> Option<PollDraft> {
        let creator = self.poll_creator.as_ref()?;
        let answers = creator.answers.iter().map(|a| (a.input.read(cx).value().trim().to_owned(), a.emoji.clone())).filter(|(text, _)| !text.is_empty()).collect();
        Some(PollDraft { question: creator.question.read(cx).value().trim().to_owned(), answers, hours: DURATIONS[creator.duration].0, multiselect: creator.multiselect })
    }

    fn poll_problems(&self, cx: &Context<Self>) -> Problems {
        let Some(draft) = self.poll_draft(cx) else { return Problems::default() };
        check(&draft.question, &draft.answers.iter().map(|(text, _)| text.as_str()).collect::<Vec<_>>())
    }

    fn post_poll(&mut self, cx: &mut Context<Self>) {
        let problems = self.poll_problems(cx);
        if problems.any() {
            if let Some(creator) = self.poll_creator.as_mut() {
                creator.problems = problems;
            }
            return cx.notify(); // lint-allow(global-notify): the poll creator is drawn by the root's overlays, over every column
        }
        let Some(draft) = self.poll_draft(cx) else { return };
        self.poll_creator = None;
        self.send_poll(draft, cx);
    }

    // Shown at once as a pending message, settled when Discord answers, as text and stickers are.
    fn send_poll(&mut self, draft: PollDraft, cx: &mut Context<Self>) {
        let (Some(client), Some(channel), Some(me)) = (self.client.clone(), self.channel.clone(), self.me.clone()) else { return };
        let nonce = crate::nonce();
        let now = Utc::now();
        let pending = Message {
            id: format!("pending-{nonce}"),
            channel_id: channel.id.clone(),
            guild_id: self.guild_id.clone(),
            author: me,
            timestamp: now.to_rfc3339(),
            nonce: Some(nonce.clone()),
            state: SendState::Sending,
            poll: Some(preview(&draft, now)),
            ..Default::default()
        };
        self.push_local(pending, cx);
        cx.spawn(async move |this, cx| {
            let result = client.send_poll(&channel.id, &draft, &nonce).await;
            this.update(cx, |this, cx| this.settle(&channel.id, &nonce, result, cx)).ok();
        })
        .detach(); // lint-allow(detached-task): one send: ends when the request returns, or when Waffle is dropped
    }

    fn poll_duration_entries(&self) -> Vec<menu::Entry> {
        let current = self.poll_creator.as_ref().map_or(DEFAULT_DURATION, |c| c.duration);
        DURATIONS
            .iter()
            .enumerate()
            .map(|(i, &(_, label))| {
                // A radio row keeps its menu open; one choice is all this list takes.
                menu::item(label, move |this, _, cx| {
                    if let Some(creator) = this.poll_creator.as_mut() {
                        creator.duration = i;
                    }
                    this.menu = None;
                    cx.notify(); // lint-allow(global-notify): the poll creator is drawn by the root's overlays, over every column
                })
                .radio(i == current)
                .entry()
            })
            .collect()
    }

    pub(crate) fn poll_creator_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let creator = self.poll_creator.as_ref()?;
        let subtitle = creator.channel.as_ref().map_or("Posts to this conversation".to_owned(), |c| format!("Posts to {c}"));
        let head = ui::row()
            .items_start()
            .gap_3()
            .child(
                ui::col()
                    .flex_1()
                    .gap_1()
                    .child(div().text_size(ui::TEXT_LG).line_height(ui::LINE_LG).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child("Create a poll"))
                    .child(div().text_size(ui::TEXT_SM).text_color(palette::muted(cx)).child(subtitle)),
            )
            .child(ui::icon_button("poll-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.close_poll_creator(cx))));
        let length = creator.question.read(cx).value().chars().count();
        let question_focus = creator.question.read(cx).focus_handle(cx);
        let question = ui::field("Question", cx)
            .child(ui::text_field("poll-question", Input::new(&creator.question).appearance(false), &question_focus).error(creator.problems.question.is_some()))
            .child(under_field(creator.problems.question, Some(format!("{length} / {QUESTION_MAX}")), length > QUESTION_MAX, cx));
        let full = creator.answers.len() >= ANSWERS_MAX;
        let add = ui::row()
            .gap_2()
            .child(
                ui::button("poll-add", Tone::Ghost, cx)
                    .small()
                    .disabled(full)
                    .child(Icon::new(Glyph::Plus).size(ui::ICON_MD))
                    .child("Add answer")
                    .on_click(cx.listener(|this, _, window, cx| {
                        let next = this.poll_creator.as_ref().map_or(0, |c| c.answers.len());
                        this.focus_poll_answer(next, window, cx);
                    })),
            )
            .child(div().text_size(ui::TEXT_XS).text_color(palette::faint(cx)).child(answer_count(creator.answers.len())));
        let rows: Vec<AnyElement> = creator.answers.iter().enumerate().map(|(i, row)| answer_row(i, row, creator.answers.len() > ANSWERS_MIN, cx).into_any_element()).collect();
        let answers = ui::field("Answers", cx).children(rows).children(creator.problems.answers.map(|p| ui::field_error(p, cx))).child(add);
        let duration = ui::field("Runs for", cx).child(
            ui::select("poll-duration", Some(Glyph::Timer), DURATIONS[creator.duration].1, false, cx)
                .on_click(cx.listener(|this, _, window, cx| this.open_menu(window.mouse_position(), this.poll_duration_entries(), cx))),
        );
        let multiple = ui::toggle_row("poll-multiselect", "Allow multiple answers", None, creator.multiselect, window, cx).on_click(cx.listener(|this, _, _, cx| {
            if let Some(creator) = this.poll_creator.as_mut() {
                creator.multiselect = !creator.multiselect;
            }
            cx.notify(); // lint-allow(global-notify): the poll creator is drawn by the root's overlays, over every column
        }));
        let footer = ui::row().items_stretch()
            .justify_end()
            .gap_2()
            .child(ui::button("poll-cancel", Tone::Secondary, cx).child("Cancel").on_click(cx.listener(|this, _, _, cx| this.close_poll_creator(cx))))
            .child(ui::button("poll-post", Tone::Primary, cx).child("Post poll").on_click(cx.listener(|this, _, _, cx| this.post_poll(cx))));
        let body = ui::dialog_body().child(head).child(question).child(answers).child(duration).child(multiple).child(footer);
        let backdrop = div().id("poll-creator-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_poll_creator(cx)));
        Some(ui::modal(backdrop, ui::dialog("poll-creator", DIALOG_WIDTH, body, window, cx), cx))
    }
}

// One answer: its emoji button inside the field, the text, and the way to remove it.
fn answer_row(i: usize, row: &AnswerRow, removable: bool, cx: &mut Context<Waffle>) -> impl IntoElement {
    let emoji = ui::row()
        .id(SharedString::from(format!("poll-emoji-{i}")))
        .size_6()
        .flex_none()
        .justify_center()
        .cursor_pointer()
        .text_color(palette::muted(cx))
        .child(match &row.emoji {
            Some(emoji) => emoji_face(emoji, 20.),
            None => Icon::new(Glyph::Emoji).size(ui::ICON_XL).into_any_element(),
        })
        .on_click(cx.listener(move |this, _, window, cx| this.open_emoji_picker(EmojiTarget::PollAnswer(i), window, cx)));
    let focus = row.input.read(cx).focus_handle(cx);
    let field = ui::text_field(SharedString::from(format!("poll-answer-{i}")), Input::new(&row.input).appearance(false), &focus).leading(emoji);
    let length = row.input.read(cx).value().chars().count();
    ui::row()
        .gap_1()
        .child(div().flex_1().min_w_0().child(field.error(length > ANSWER_MAX)))
        .child(ui::icon_button(SharedString::from(format!("poll-remove-{i}")), Glyph::Trash, "Remove answer", cx).disabled(!removable).on_click(cx.listener(move |this, _, _, cx| this.remove_poll_answer(i, cx))))
}

// The line under a field: what's wrong (if anything) at the start, the running count at the end.
fn under_field(problem: Option<&'static str>, count: Option<String>, over: bool, cx: &mut Context<Waffle>) -> impl IntoElement {
    ui::row()
        .gap_2()
        .text_size(ui::TEXT_XS)
        .child(div().flex_1().min_w_0().children(problem.map(|p| ui::field_error(p, cx))))
        .children(count.map(|c| div().flex_none().text_color(if over { palette::danger_ink(cx) } else { palette::faint(cx) }).child(c)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn poll(counts: &[(u64, bool)]) -> Poll {
        let answers = counts.iter().zip(1..).map(|(&(count, me), id)| PollAnswer { id, text: format!("a{id}"), emoji: None, count, me }).collect();
        Poll { question: "Map?".into(), answers, expiry: Some("2026-11-20T21:00:00Z".into()), multiselect: false, finalized: false }
    }

    fn at(t: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(t).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn shares_round_each_answer_and_survive_no_votes() {
        assert_eq!(poll(&[(68, false), (50, false), (24, false)]).shares(), vec![48, 35, 17]);
        assert_eq!(poll(&[(0, false), (0, false)]).shares(), vec![0, 0]);
    }

    #[test]
    fn foot_counts_votes_and_time_left() {
        assert_eq!(poll(&[(1, false), (0, false)]).foot(at("2026-11-20T17:40:00Z")), "1 vote · Ends in 3 hours");
        assert_eq!(poll(&[(140, false), (1, false)]).foot(at("2026-11-20T20:59:30Z")), "141 votes · Ends in 1 minute");
        assert_eq!(poll(&[(2, false)]).foot(at("2026-11-18T20:00:00Z")), "2 votes · Ends in 2 days");
        assert_eq!(poll(&[(2, false)]).foot(at("2026-11-20T21:00:00Z")), "2 votes · Poll closed");
        let finalized = Poll { finalized: true, ..poll(&[(0, false)]) };
        assert_eq!(finalized.foot(at("2026-11-20T12:00:00Z")), "0 votes · Poll closed");
    }

    #[test]
    fn casting_moves_your_vote_and_the_gateway_echo_lands_once() {
        let mut p = poll(&[(3, true), (1, false)]);
        cast(&mut p, &[2]);
        assert_eq!(p.answers.iter().map(|a| (a.count, a.me)).collect::<Vec<_>>(), vec![(2, false), (2, true)]);
        apply_vote(&mut p, 2, true, true);
        assert_eq!(p.answers[1].count, 2);
        apply_vote(&mut p, 1, true, false);
        assert_eq!(p.answers[0].count, 3);
        cast(&mut p, &[]);
        assert!(!p.voted());
        assert_eq!(p.total_votes(), 4);
    }

    #[test]
    fn check_names_whats_missing() {
        assert_eq!(check("", &["a"]), Problems { question: Some("Add a question to post the poll"), answers: Some("Fill in at least two answers") });
        let long = "x".repeat(ANSWER_MAX + 1);
        assert_eq!(check("Map?", &["a", &long]).answers, Some("Keep each answer to 55 characters"));
        assert!(!check("Map?", &["a", "b"]).any());
        assert_eq!(answer_count(3), "3 of 10");
        assert_eq!(answer_count(10), "10 of 10, the most a poll can have");
    }

    #[test]
    fn resend_keeps_the_run_length() {
        let message = Message { timestamp: "2026-11-20T20:00:00Z".into(), poll: Some(poll(&[(0, false), (0, false)])), ..Default::default() };
        let draft = draft_of(&message).unwrap();
        assert_eq!((draft.hours, draft.answers.len()), (1, 2));
        assert_eq!(preview(&draft, at("2026-11-20T20:00:00Z")).expiry.as_deref(), Some("2026-11-20T21:00:00+00:00"));
    }
}
