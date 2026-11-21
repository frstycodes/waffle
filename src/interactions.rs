// An interaction's life after we send it, as Discord's client runs it: a command's stand-in ("Sending
// command...", then "{app} is thinking..."), the clicked control's spinner, and what's left when the
// app never answers. Commands and components have no client timer: Discord's gateway sends
// INTERACTION_FAILURE when the app misses its 3 s. Only a modal submit times out here.

use std::{sync::Arc, time::Duration};

use gpui_kit::gpui::Context;
use serde_json::Value;

use crate::{
    app_commands::{self, App, InteractionMeta},
    discord::{InteractError, Message, SendState, User},
    Waffle,
};

// Discord's client fails a modal submit with no answer after this long.
const MODAL_TIMEOUT: Duration = Duration::from_secs(10);
// A deferred reply (LOADING) still loading this long after it was sent reads "did not respond".
const DEFERRED_TIMEOUT_MS: u64 = 15 * 60 * 1000;
// A command stand-in failing later than this after it was sent reads "did not respond".
const FAILED_LATE_MS: u64 = 3000;
// Discord's ReasonCodes.TIMEOUT.
const REASON_TIMEOUT: u64 = 2;

pub(crate) const INTERACTION_FAILED: &str = "This interaction failed";
pub(crate) const SLOW_DOWN: &str = "Slow down! You're clicking too fast.";
pub(crate) const DID_NOT_RESPOND: &str = "The application did not respond";
pub(crate) const MODAL_FAILED: &str = "Something went wrong. Try again.";

pub(crate) struct PendingInteraction {
    pub channel_id: String,
    pub app: App,
    // Discord's id once INTERACTION_CREATE names it; the app's reply carries it.
    pub id: Option<String>,
    pub kind: InteractionKind,
    // What to show once it failed: Discord's text for a component, `MODAL_FAILED` for a form.
    pub failure: Option<String>,
}

impl PendingInteraction {
    pub fn failed(&self) -> bool {
        self.failure.is_some()
    }
}

#[derive(Clone, PartialEq)]
pub(crate) enum InteractionKind {
    // A slash command, standing in the feed as a local message until the reply replaces it.
    Command,
    // A button or select on `message_id`: that control shows the spinner, the message's others disable.
    Component { message_id: String, custom_id: String },
    Modal,
}

/// What a command's status line says (Discord's `fK`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CommandStatus {
    Sending,
    Thinking,
    TimedOut,
    Failed(String),
}

impl Waffle {
    /// Sends `body` (from `app_commands::command_body` and friends, built with `nonce`) and tracks it.
    pub(crate) fn interact(&mut self, nonce: String, app: App, kind: InteractionKind, body: Value, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let Some(channel_id) = body["channel_id"].as_str().map(str::to_owned) else { return };
        if let (InteractionKind::Command, Some(me)) = (&kind, self.me.clone()) {
            let command = body["data"]["options"].as_array().map(|o| command_path(&body["data"]["name"], o));
            self.push_local(stand_in(&channel_id, &nonce, &app, me, command), cx);
        }
        let modal = kind == InteractionKind::Modal;
        self.interactions.insert(nonce.clone(), PendingInteraction { channel_id, app, id: None, kind, failure: None });
        let sent = nonce.clone();
        cx.spawn(async move |this, cx| {
            loop {
                match client.interact(body.clone()).await {
                    Ok(()) => return,
                    // Discord's client resends a rate-limited modal submit after `retry_after`.
                    Err(e) if modal && e.status == Some(429) => cx.background_executor().timer(Duration::from_secs_f64(e.retry_after.unwrap_or(1.).max(0.))).await,
                    Err(e) => {
                        this.update(cx, |this, cx| this.interaction_refused(&sent, e, cx)).ok();
                        return;
                    }
                }
            }
        })
        .detach();
        if modal {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(MODAL_TIMEOUT).await;
                this.update(cx, |this, cx| {
                    if this.interactions.get(&nonce).is_some_and(|p| !p.failed()) {
                        this.fail_interaction(&nonce, Refusal::default(), cx);
                    }
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }

    // Discord writes the interaction's id into the stand-in, which turns "Sending command..." into
    // "{app} is thinking...".
    pub(crate) fn on_interaction_create(&mut self, nonce: String, id: String, cx: &mut Context<Self>) {
        let Some(p) = self.interactions.get_mut(&nonce) else { return };
        p.id = Some(id.clone());
        let channel_id = p.channel_id.clone();
        if let Some(meta) = self.stand_in_mut(&channel_id, &nonce).and_then(|m| m.interaction.as_mut()) {
            meta.id = id;
        }
        cx.notify();
    }

    // A command's reply arrives as its own message, so a success only ends a component's spinner or a
    // form; a command's stand-in waits for the reply (or the modal it opened) to replace it.
    pub(crate) fn on_interaction_success(&mut self, nonce: String, cx: &mut Context<Self>) {
        if self.interactions.get(&nonce).is_some_and(|p| p.kind != InteractionKind::Command) {
            self.interactions.remove(&nonce);
        }
        cx.notify();
    }

    /// INTERACTION_FAILURE from the gateway.
    pub(crate) fn interaction_failed(&mut self, nonce: &str, reason_code: Option<u64>, cx: &mut Context<Self>) {
        self.fail_interaction(nonce, Refusal { reason_code, ..Default::default() }, cx);
    }

    // `POST /interactions` refused it. An out-of-date command index is refetched on next use.
    fn interaction_refused(&mut self, nonce: &str, error: InteractError, cx: &mut Context<Self>) {
        if error.stale_index {
            self.command_picker.mark_indexes_stale();
        }
        let refusal = Refusal { status: error.status, code: error.code, message: error.message, reason_code: None };
        self.fail_interaction(nonce, refusal, cx);
    }

    fn fail_interaction(&mut self, nonce: &str, refusal: Refusal, cx: &mut Context<Self>) {
        let Some(p) = self.interactions.get_mut(nonce) else { return };
        match p.kind {
            // Discord drops a failed command; its stand-in keeps the reason.
            InteractionKind::Command => {
                let reason = refusal.message.or_else(|| reason_text(refusal.reason_code, &p.app.name));
                let channel_id = p.channel_id.clone();
                self.interactions.remove(nonce);
                if let Some(m) = self.stand_in_mut(&channel_id, nonce) {
                    m.state = SendState::Failed;
                    m.flags &= !app_commands::LOADING;
                    m.interaction_error = reason;
                }
            }
            InteractionKind::Component { .. } => p.failure = Some(component_failure(&refusal, &p.app.name)),
            InteractionKind::Modal => p.failure = Some(MODAL_FAILED.to_owned()),
        }
        cx.notify();
    }

    fn stand_in_mut(&mut self, channel_id: &str, nonce: &str) -> Option<&mut Message> {
        Arc::make_mut(&mut self.feeds.get_mut(channel_id)?.messages).iter_mut().find(|m| m.nonce.as_deref() == Some(nonce) && m.state != SendState::Sent)
    }

    /// The nonce of our command whose stand-in `reply` replaces: matched by Discord's interaction id.
    pub(crate) fn interaction_reply_nonce(&mut self, reply: &Message) -> Option<String> {
        let id = &reply.interaction.as_ref()?.id;
        let nonce = self.interactions.iter().find(|(_, p)| p.id.as_ref() == Some(id)).map(|(n, _)| n.clone())?;
        self.interactions.remove(&nonce);
        Some(nonce)
    }

    /// A modal opened in answer to our interaction (matched by its nonce, else its interaction id), which
    /// Discord counts as done: a command's stand-in goes, a control's spinner stops.
    pub(crate) fn answered_by_modal(&mut self, nonce: Option<&str>, interaction_id: &str, cx: &mut Context<Self>) {
        let by_id = || self.interactions.iter().find(|(_, p)| p.id.as_deref() == Some(interaction_id)).map(|(n, _)| n.clone());
        let Some(nonce) = nonce.filter(|n| self.interactions.contains_key(*n)).map(str::to_owned).or_else(by_id) else { return };
        let Some(p) = self.interactions.remove(&nonce) else { return };
        if p.kind == InteractionKind::Command {
            self.remove_message(&p.channel_id, &format!("pending-{nonce}"));
        }
        cx.notify();
    }

    /// Discord clears failed component interactions (and their red lines) on a channel switch.
    pub(crate) fn clear_failed_interactions(&mut self) {
        self.interactions.retain(|_, p| !(p.failed() && matches!(p.kind, InteractionKind::Component { .. })));
    }

    /// One interaction per message: while one is in flight, every control on the message is disabled
    /// and no other goes out (Discord's `canQueueInteraction`).
    pub(crate) fn message_busy(&self, message_id: &str) -> bool {
        self.interactions.values().any(|p| !p.failed() && matches!(&p.kind, InteractionKind::Component { message_id: m, .. } if m == message_id))
    }

    /// A new interaction on a message replaces the failed ones before it: only the latest one's line shows.
    pub(crate) fn forget_failed_on(&mut self, message_id: &str) {
        self.interactions.retain(|_, p| !(p.failed() && matches!(&p.kind, InteractionKind::Component { message_id: m, .. } if m == message_id)));
    }

    /// A deferred reply turns into "did not respond" 15 minutes after it was sent; this redraws it then.
    pub(crate) fn watch_deferred(&self, message_id: &str, flags: u64, cx: &mut Context<Self>) {
        if flags & app_commands::LOADING == 0 {
            return;
        }
        let Some(sent) = snowflake_ms(message_id) else { return };
        let wait = (sent + DEFERRED_TIMEOUT_MS).saturating_sub(now_ms());
        if wait == 0 {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(wait + 1000)).await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
    }
}

// What Discord's client knows about a failure: the HTTP status and code, the server's words, and the
// gateway's reason code.
#[derive(Default)]
struct Refusal {
    status: Option<u16>,
    code: Option<u64>,
    message: Option<String>,
    reason_code: Option<u64>,
}

// Discord's ReasonCodes text. Not verified: the activity-launch codes, which only activities raise, are
// left to the generic line.
fn reason_text(reason_code: Option<u64>, app: &str) -> Option<String> {
    match reason_code? {
        REASON_TIMEOUT if !app.is_empty() => Some(format!("{app} didn't respond in time")),
        REASON_TIMEOUT => Some("The application didn't respond in time".to_owned()),
        _ => None,
    }
}

// The line under a failed component (Discord's `zv`): a 429 first, then the reason, else the generic.
// Not verified: whether a 429 reaches Discord's check as the body's code or only as the HTTP status.
fn component_failure(refusal: &Refusal, app: &str) -> String {
    if refusal.status == Some(429) || refusal.code == Some(429) {
        return SLOW_DOWN.to_owned();
    }
    reason_text(refusal.reason_code, app).unwrap_or_else(|| INTERACTION_FAILED.to_owned())
}

/// The status line under a command's stand-in or an app's deferred reply, at `now_ms`; None when
/// there's none to show.
pub(crate) fn command_status(message: &Message, now_ms: u64) -> Option<CommandStatus> {
    let meta = message.interaction.as_ref()?;
    let stand_in = is_stand_in(message);
    // A stand-in's id is the nonce we sent, a snowflake of when it went out.
    let sent = if stand_in { message.nonce.as_deref().and_then(snowflake_ms) } else { snowflake_ms(&message.id) };
    let loading = message.flags & app_commands::LOADING != 0;
    let expired = sent.is_some_and(|t| t + DEFERRED_TIMEOUT_MS < now_ms);
    let failed_late = message.state == SendState::Failed && sent.is_some_and(|t| t + FAILED_LATE_MS < now_ms);
    if stand_in && message.state == SendState::Sending {
        return Some(if meta.id.is_empty() { CommandStatus::Sending } else { CommandStatus::Thinking });
    }
    if loading {
        return Some(if expired { CommandStatus::TimedOut } else { CommandStatus::Thinking });
    }
    if failed_late {
        return Some(CommandStatus::TimedOut);
    }
    (stand_in && message.state == SendState::Failed).then(|| CommandStatus::Failed(message.interaction_error.clone().unwrap_or_else(|| INTERACTION_FAILED.to_owned())))
}

pub(crate) fn is_stand_in(message: &Message) -> bool {
    message.interaction.is_some() && message.id.starts_with("pending-")
}

// When a snowflake id was made, in Unix milliseconds.
fn snowflake_ms(id: &str) -> Option<u64> {
    id.parse::<u64>().ok().map(|id| (id >> 22) + 1_420_070_400_000)
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

// `/music queue clear` back out of the nested options a body carries.
fn command_path(name: &Value, options: &[Value]) -> String {
    let mut path = vec![name.as_str().unwrap_or_default().to_owned()];
    let mut level = options;
    while let Some(sub) = level.first().filter(|o| matches!(o["type"].as_u64(), Some(1 | 2))) {
        path.push(sub["name"].as_str().unwrap_or_default().to_owned());
        level = sub["options"].as_array().map(Vec::as_slice).unwrap_or_default();
    }
    path.join(" ")
}

// Discord's local CHAT_INPUT_COMMAND message: the app's bot as author, no content, our nonce as its id.
fn stand_in(channel_id: &str, nonce: &str, app: &App, me: User, command: Option<String>) -> Message {
    Message {
        id: format!("pending-{nonce}"),
        channel_id: channel_id.to_owned(),
        author: User { id: app.id.clone(), name: app.name.clone(), username: app.name.clone(), avatar: app.icon.clone().unwrap_or_default(), bot: true, ..Default::default() },
        timestamp: chrono::Utc::now().to_rfc3339(),
        nonce: Some(nonce.to_owned()),
        state: SendState::Sending,
        interaction: Some(InteractionMeta { id: String::new(), user: me, command, ephemerality_reason: None }),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn command_path_reads_nested_subcommands() {
        let options = json!([{ "type": 2, "name": "queue", "options": [{ "type": 1, "name": "clear", "options": [{ "type": 5, "name": "all", "value": true }] }] }]);
        assert_eq!(command_path(&json!("music"), options.as_array().unwrap()), "music queue clear");
        assert_eq!(command_path(&json!("play"), json!([{ "type": 3, "name": "q", "value": "x" }]).as_array().unwrap()), "play");
    }

    fn snowflake_at(ms: u64) -> String {
        ((ms - 1_420_070_400_000) << 22).to_string()
    }

    #[test]
    fn command_status_follows_discords_states() {
        let sent = 1_700_000_000_000;
        let nonce = snowflake_at(sent);
        let mut m = stand_in("c", &nonce, &App { id: "9".into(), name: "Wavebot".into(), icon: None }, User::default(), Some("play".into()));
        assert_eq!(command_status(&m, sent + 10), Some(CommandStatus::Sending));
        m.interaction.as_mut().unwrap().id = "i1".into();
        assert_eq!(command_status(&m, sent + 10), Some(CommandStatus::Thinking));
        m.state = SendState::Failed;
        m.interaction_error = Some("Wavebot didn't respond in time".into());
        assert_eq!(command_status(&m, sent + 2000), Some(CommandStatus::Failed("Wavebot didn't respond in time".into())));
        assert_eq!(command_status(&m, sent + 3001), Some(CommandStatus::TimedOut), "a failure older than 3 s reads as no response");
        m.interaction_error = None;
        assert_eq!(command_status(&m, sent + 10), Some(CommandStatus::Failed(INTERACTION_FAILED.into())));

        let deferred = Message { id: snowflake_at(sent), flags: app_commands::LOADING, interaction: m.interaction.clone(), ..Default::default() };
        assert_eq!(command_status(&deferred, sent + DEFERRED_TIMEOUT_MS), Some(CommandStatus::Thinking));
        assert_eq!(command_status(&deferred, sent + DEFERRED_TIMEOUT_MS + 1), Some(CommandStatus::TimedOut));
        let answered = Message { flags: 0, ..deferred };
        assert_eq!(command_status(&answered, sent + DEFERRED_TIMEOUT_MS + 1), None);
    }

    #[test]
    fn component_failures_use_discords_copy() {
        assert_eq!(component_failure(&Refusal { status: Some(429), ..Default::default() }, "Pollster"), SLOW_DOWN);
        assert_eq!(component_failure(&Refusal { reason_code: Some(REASON_TIMEOUT), ..Default::default() }, "Pollster"), "Pollster didn't respond in time");
        assert_eq!(component_failure(&Refusal { reason_code: Some(REASON_TIMEOUT), ..Default::default() }, ""), "The application didn't respond in time");
        assert_eq!(component_failure(&Refusal { status: Some(400), message: Some("Bad".into()), ..Default::default() }, "Pollster"), INTERACTION_FAILED);
    }
}
