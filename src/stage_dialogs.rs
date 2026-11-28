// The stage's dialogs: start or edit the topic, end for everyone, the also-end prompt on disconnect, stage full.
// Copy is Discord's (docs/discord-stage-re.md).
use std::collections::HashMap;

use gpui_kit::{
    AnyElement, Context, InteractiveElement, KeyDownEvent, MouseButton, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window,
    component::input::{Input, InputEvent},
    div,
};

use crate::{
    Waffle,
    discord::{Guild, OnStage},
    icons::Glyph,
    palette,
    stage::{NOTIFY_BY_DEFAULT_UP_TO, TOPIC_MAX},
    ui::{self},
};

const PLACEHOLDER: &str = "The debate is over: diet is better than regular";

pub(crate) struct StageTopic {
    guild_id: String,
    guild_name: String,
    channel_id: String,
    editing: bool,
    notify: bool,
}

impl Waffle {
    // "What will you be talking about?" when the stage has no instance yet, "Edit Stage" when it has one.
    pub(crate) fn open_stage_topic(&mut self, guild_id: String, channel_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(guild) = self.guilds.iter().find(|g| g.id == guild_id) else { return };
        let topic = guild.stage(&channel_id).map(|s| s.topic.clone());
        // Not verified: the member list's group counts cover everyone; on large servers Discord sends online
        // groups only, so this is at least the online count.
        let known_members: u64 = self.member_groups.get(&guild_id).map_or(0, |g| g.values().map(|&n| u64::from(n)).sum());
        let form = StageTopic { guild_name: guild.name.clone(), guild_id, channel_id, editing: topic.is_some(), notify: known_members <= NOTIFY_BY_DEFAULT_UP_TO };
        self.menu = None;
        self.dialog_input.update(cx, |state, cx| {
            state.set_value(topic.unwrap_or_default(), window, cx);
            state.set_placeholder(PLACEHOLDER, window, cx);
            state.focus(window, cx);
        });
        self.stage_topic = Some(form);
        cx.notify(); // lint-allow(global-notify): the topic dialog is an overlay, which only follows the app
    }

    // "End Stage for Everyone": confirm, then end_stage.
    pub(crate) fn open_end_stage(&mut self, channel_id: String, cx: &mut Context<Self>) {
        self.confirm("Are you sure you want to end the Stage for everyone?", "The Stage will end immediately.", "End Stage", move |this, _, cx| this.end_stage(&channel_id, cx), cx);
    }

    // Disconnect from a stage: the last moderator on stage is asked "Do you want to also end the Stage?",
    // everyone else just leaves.
    pub(crate) fn disconnect_from_stage(&mut self, guild_id: String, channel_id: String, cx: &mut Context<Self>) {
        let Some(me) = self.me.as_ref() else { return self.leave_voice(cx) };
        let roles = self.member_roles.get(&guild_id);
        let asks = self.guilds.iter().find(|g| g.id == guild_id).is_some_and(|g| last_moderator_speaker(g, &channel_id, &me.id, roles));
        if !asks {
            return self.leave_voice(cx);
        }
        self.confirm(
            "Do you want to also end the Stage?",
            "A Stage with no speakers will automatically end after a few minutes.",
            "Disconnect and End",
            move |this, _, cx| this.end_stage(&channel_id, cx),
            cx,
        );
        self.set_dialog_instead("No, just disconnect quietly", |this, _, cx| this.leave_voice(cx));
    }

    // "Sorry, this Stage is full."
    pub(crate) fn open_stage_full(&mut self, cx: &mut Context<Self>) {
        self.notice("Sorry, this Stage is full.", "This Stage is at maximum capacity.", "Okay", cx);
    }

    fn close_stage_topic(&mut self, cx: &mut Context<Self>) {
        self.stage_topic = None;
        cx.notify(); // lint-allow(global-notify): the topic dialog is an overlay, which only follows the app
    }

    // The topic field's events while the dialog is open: Enter submits, and the topic stops at Discord's 120.
    pub(crate) fn stage_topic_input(&mut self, event: &InputEvent, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(event, InputEvent::PressEnter { .. }) {
            return self.submit_stage_topic(window, cx);
        }
        let value = self.dialog_input.read(cx).value().to_string();
        if value.chars().count() <= TOPIC_MAX {
            return;
        }
        let clipped: String = value.chars().take(TOPIC_MAX).collect();
        self.dialog_input.update(cx, |state, cx| state.set_value(clipped, window, cx));
    }

    fn submit_stage_topic(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let topic = self.dialog_input.read(cx).value().trim().to_owned();
        if topic.is_empty() {
            return;
        }
        let Some(form) = self.stage_topic.take() else { return };
        cx.notify(); // lint-allow(global-notify): the topic dialog is an overlay, which only follows the app
        if form.editing {
            return self.edit_stage(&form.channel_id, topic, cx);
        }
        // Discord's start joins the stage first when we aren't in it: the host goes on stage once there.
        let joined = self.call.as_ref().is_some_and(|c| c.channel.id == form.channel_id);
        let channel = self.guilds.iter().find(|g| g.id == form.guild_id).and_then(|g| g.channel(&form.channel_id)).cloned();
        if let Some(channel) = channel.filter(|_| !joined) {
            self.join_voice_in(Some(form.guild_id.clone()), channel, cx);
        }
        self.start_stage(form.guild_id, form.channel_id, topic, form.notify, cx);
    }

    pub(crate) fn render_stage_topic(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.stage_topic.as_ref()?;
        let (strong, muted) = (palette::strong(cx), palette::muted(cx));
        let (title, subtitle, submit) = match form.editing {
            true => ("Edit Stage", "Change the topic", "Save Changes"),
            false => ("What will you be talking about?", "Write a topic to start your Stage.", "Start Stage"),
        };
        let (empty, length) = {
            let value = self.dialog_input.read(cx).value();
            (value.trim().is_empty(), value.chars().count())
        };
        let count = div().flex_none().text_size(ui::TEXT_XS).text_color(palette::faint(cx)).child(format!("{length}/{TOPIC_MAX}"));
        let focus = gpui_kit::Focusable::focus_handle(self.dialog_input.read(cx), cx);
        let topic = ui::Field::new("Stage Topic", cx).child(ui::Input::new("stage-topic-field", Input::new(&self.dialog_input).appearance(false), &focus).trailing(count));
        let head = ui::row()
            .items_start()
            .gap_2()
            .child(
                ui::col().min_h_0()
                    .flex_1()
                    .gap_1()
                    .child(div().text_size(ui::TEXT_LG).line_height(ui::LINE_LG).font_weight(gpui_kit::FontWeight::SEMIBOLD).text_color(strong).child(title))
                    .child(div().text_size(ui::TEXT_SM).line_height(ui::LINE_SM).text_color(muted).child(subtitle)),
            )
            .child(ui::Button::new("stage-topic-close").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).icon(Glyph::X).tooltip_text("Close").on_click(cx.listener(|this, _, _, cx| this.close_stage_topic(cx))));
        let notify = (!form.editing).then(|| {
            let detail: SharedString = format!("Everyone in {} gets a notification that the Stage started.", form.guild_name).into();
            let check = ui::Checkbox::new("stage-topic-notify-check").checked(if form.notify { ui::Check::On } else { ui::Check::Off });
            ui::pick_row("stage-topic-notify", check, "Notify @everyone", Some(detail), cx).on_click(cx.listener(|this, _, _, cx| {
                if let Some(form) = this.stage_topic.as_mut() {
                    form.notify = !form.notify;
                }
                cx.notify(); // lint-allow(global-notify): the topic dialog is an overlay, which only follows the app
            }))
        });
        let actions = ui::row().items_stretch()
            .gap_2()
            .child(ui::Button::new("stage-topic-cancel").variant(ui::ButtonVariant::Secondary).flex_1().child("Cancel").on_click(cx.listener(|this, _, _, cx| this.close_stage_topic(cx))))
            .child(ui::Button::new("stage-topic-submit").flex_1().child(submit).disabled(empty).on_click(cx.listener(|this, _, window, cx| this.submit_stage_topic(window, cx))));
        let body = ui::DialogContent::new()
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if e.keystroke.key == "escape" {
                    this.close_stage_topic(cx);
                }
            }))
            .child(head)
            .child(topic)
            .children(notify)
            .child(actions);
        let backdrop = div().id("stage-topic-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_stage_topic(cx)));
        Some(ui::Dialog::modal(backdrop, ui::Dialog::new("stage-topic", 420., body, window, cx), cx))
    }
}

// Discord asks a moderator who leaves from the stage whether to end it, unless another speaker is a moderator.
// `roles` are the server's members' roles as we know them; a speaker whose roles we haven't seen counts as
// @everyone.
pub(crate) fn last_moderator_speaker(guild: &Guild, channel_id: &str, me: &str, roles: Option<&HashMap<String, Vec<String>>>) -> bool {
    let Some(channel) = guild.channel(channel_id) else { return false };
    let speakers: Vec<&str> = guild.voice_states.iter().filter(|v| v.channel_id.as_deref() == Some(channel_id) && v.on_stage() == OnStage::Speaker).map(|v| v.user_id.as_str()).collect();
    if guild.stage(channel_id).is_none() || !speakers.contains(&me) || !guild.is_stage_moderator(me, channel) {
        return false;
    }
    let moderator = |user: &str| guild.is_stage_moderator_with(user, roles.and_then(|r| r.get(user)).map_or(&[][..], Vec::as_slice), channel);
    !speakers.into_iter().filter(|&u| u != me).any(moderator)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::discord::{Role, StageInstance, VoiceState, parse_channel, perm};

    fn guild(speakers: &[&str], live: bool) -> Guild {
        let role = |id: &str, permissions: u64| Role { id: id.into(), name: id.into(), color: 0, position: 0, permissions, managed: false, icon: None };
        let on_stage = |user: &str, suppress: bool| VoiceState { user_id: user.into(), channel_id: Some("s".into()), suppress, ..Default::default() };
        Guild {
            id: "g".into(),
            name: "g".into(),
            icon: None,
            channels: vec![parse_channel(&json!({ "id": "s", "type": 13 }), "g").unwrap()],
            roles: vec![role("g", perm::CONNECT), role("mods", perm::STAGE_MODERATOR)],
            voice_states: speakers.iter().map(|u| on_stage(u, false)).chain([on_stage("listener", true)]).collect(),
            stage_instances: live.then(|| StageInstance { channel_id: "s".into(), topic: "t".into() }).into_iter().collect(),
            emojis: Vec::new(),
            stickers: Vec::new(),
            owner_id: Some("owner".into()),
            default_notifications: 0,
            threads: Vec::new(),
            my_roles: vec!["mods".into()],
            premium_tier: 0,
            afk_channel_id: None,
            ..Default::default()
        }
    }

    #[test]
    fn asks_to_end_only_the_last_moderator_on_stage() {
        let roles: HashMap<String, Vec<String>> = [("mod2".to_owned(), vec!["mods".to_owned()])].into();
        let asks = |g: &Guild| last_moderator_speaker(g, "s", "me", Some(&roles));
        assert!(asks(&guild(&["me", "guest"], true)));
        // Another moderator is speaking, by role or as the owner.
        assert!(!asks(&guild(&["me", "mod2"], true)));
        assert!(!asks(&guild(&["me", "owner"], true)));
        // We're in the audience, the stage isn't live, or we aren't a moderator.
        assert!(!asks(&guild(&["guest"], true)));
        assert!(!asks(&guild(&["me"], false)));
        assert!(!asks(&Guild { my_roles: Vec::new(), ..guild(&["me"], true) }));
    }
}
