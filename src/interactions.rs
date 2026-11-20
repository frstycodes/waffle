// An interaction's life after we send it: a "<App> is thinking…" stand-in for a command, the
// clicked button's spinner for a component, and what's left when the app never answers.

use std::{sync::Arc, time::Duration};

use gpui_kit::gpui::Context;
use serde_json::Value;

use crate::{
    app_commands::{self, App, InteractionMeta},
    discord::{Message, SendState, User},
    Waffle,
};

// Discord gives an app 3 seconds to acknowledge; past this with no INTERACTION_CREATE it never will.
const NO_ANSWER: Duration = Duration::from_secs(10);

pub(crate) struct PendingInteraction {
    pub channel_id: String,
    pub app: App,
    // Discord's id once INTERACTION_CREATE names it; the app's reply carries it.
    pub id: Option<String>,
    pub kind: InteractionKind,
    // The `POST /interactions` body, resent by Try again with a fresh nonce.
    pub body: Value,
    pub failed: bool,
}

#[derive(Clone, PartialEq)]
pub(crate) enum InteractionKind {
    // A slash command, standing in the feed as a local message until the reply replaces it.
    Command,
    // A button or select on `message_id`: that control shows the spinner.
    Component { message_id: String, custom_id: String },
    Modal,
}

impl Waffle {
    /// Sends `body` (from `app_commands::command_body` and friends, built with `nonce`) and tracks it.
    pub(crate) fn interact(&mut self, nonce: String, app: App, kind: InteractionKind, body: Value, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let Some(channel_id) = body["channel_id"].as_str().map(str::to_owned) else { return };
        if let (InteractionKind::Command, Some(me)) = (&kind, self.me.clone()) {
            let command = body["data"]["options"].as_array().map(|o| command_path(&body["data"]["name"], o));
            self.push_local(thinking(&channel_id, &nonce, &app, me, command), cx);
        }
        self.interactions.insert(nonce.clone(), PendingInteraction { channel_id, app, id: None, kind, body: body.clone(), failed: false });
        cx.spawn(async move |this, cx| {
            let sent = client.interact(body).await;
            if sent.is_err() {
                this.update(cx, |this, cx| this.interaction_failed(&nonce, cx)).ok();
                return;
            }
            cx.background_executor().timer(NO_ANSWER).await;
            this.update(cx, |this, cx| {
                if this.interactions.get(&nonce).is_some_and(|p| p.id.is_none()) {
                    this.interaction_failed(&nonce, cx);
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn on_interaction_create(&mut self, nonce: String, id: String) {
        if let Some(p) = self.interactions.get_mut(&nonce) {
            p.id = Some(id);
        }
    }

    // A command's reply arrives as its own message, so a success only ends a component's spinner;
    // a command's stand-in waits for the reply (or the modal it opened) to replace it.
    pub(crate) fn on_interaction_success(&mut self, nonce: String, cx: &mut Context<Self>) {
        if self.interactions.get(&nonce).is_some_and(|p| p.kind != InteractionKind::Command) {
            self.interactions.remove(&nonce);
        }
        cx.notify();
    }

    pub(crate) fn interaction_failed(&mut self, nonce: &str, cx: &mut Context<Self>) {
        let Some(p) = self.interactions.get_mut(nonce) else { return };
        p.failed = true;
        let channel_id = p.channel_id.clone();
        if let Some(m) = self.feeds.get_mut(&channel_id).and_then(|f| Arc::make_mut(&mut f.messages).iter_mut().find(|m| m.nonce.as_deref() == Some(nonce))) {
            m.state = SendState::Failed;
            m.flags &= !app_commands::LOADING;
        }
        cx.notify();
    }

    /// The nonce of our command whose stand-in `reply` replaces: matched by Discord's interaction id.
    pub(crate) fn interaction_reply_nonce(&mut self, reply: &Message) -> Option<String> {
        let id = &reply.interaction.as_ref()?.id;
        let nonce = self.interactions.iter().find(|(_, p)| p.id.as_ref() == Some(id)).map(|(n, _)| n.clone())?;
        self.interactions.remove(&nonce);
        Some(nonce)
    }

    /// A modal opened in answer to our command: its stand-in has nothing left to wait for.
    pub(crate) fn drop_stand_in(&mut self, interaction_id: &str, cx: &mut Context<Self>) {
        let Some(nonce) = self.interactions.iter().find(|(_, p)| p.id.as_deref() == Some(interaction_id)).map(|(n, _)| n.clone()) else { return };
        let Some(p) = self.interactions.remove(&nonce) else { return };
        let id = format!("pending-{nonce}");
        self.remove_message(&p.channel_id, &id);
        cx.notify();
    }

    /// Try again on a failed command or component: the same body under a new nonce.
    pub(crate) fn retry_interaction(&mut self, nonce: &str, cx: &mut Context<Self>) {
        let Some(mut p) = self.interactions.remove(nonce) else { return };
        if p.kind == InteractionKind::Command {
            self.remove_message(&p.channel_id, &format!("pending-{nonce}"));
        }
        let fresh = crate::nonce();
        p.body["nonce"] = fresh.clone().into();
        self.interact(fresh, p.app, p.kind, p.body, cx);
    }

    /// The failed or spinning interaction on a message's control, for drawing it.
    pub(crate) fn component_interaction(&self, message_id: &str, custom_id: &str) -> Option<(&str, &PendingInteraction)> {
        self.interactions
            .iter()
            .find(|(_, p)| matches!(&p.kind, InteractionKind::Component { message_id: m, custom_id: c } if m == message_id && c == custom_id))
            .map(|(n, p)| (n.as_str(), p))
    }
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

fn thinking(channel_id: &str, nonce: &str, app: &App, me: User, command: Option<String>) -> Message {
    Message {
        id: format!("pending-{nonce}"),
        channel_id: channel_id.to_owned(),
        author: User { id: app.id.clone(), name: app.name.clone(), avatar: app.icon.clone().unwrap_or_default(), bot: true, ..Default::default() },
        timestamp: chrono::Utc::now().to_rfc3339(),
        nonce: Some(nonce.to_owned()),
        state: SendState::Sending,
        interaction: Some(InteractionMeta { id: String::new(), user: me, command }),
        flags: app_commands::LOADING,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn command_path_reads_nested_subcommands() {
        let options = json!([{ "type": 2, "name": "queue", "options": [{ "type": 1, "name": "clear", "options": [{ "type": 5, "name": "all", "value": true }] }] }]);
        assert_eq!(super::command_path(&json!("music"), options.as_array().unwrap()), "music queue clear");
        assert_eq!(super::command_path(&json!("play"), json!([{ "type": 3, "name": "q", "value": "x" }]).as_array().unwrap()), "play");
    }
}
