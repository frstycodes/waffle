// The stage's dialogs: start or edit the topic, end for everyone, the also-end prompt on disconnect, stage full.
// Copy is Discord's (docs/discord-stage-re.md).
use gpui_kit::{Context, Window};

use crate::Waffle;

impl Waffle {
    // "What will you be talking about?" when the stage has no instance yet, "Edit Stage" when it has one.
    pub(crate) fn open_stage_topic(&mut self, guild_id: String, channel_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let _ = (guild_id, channel_id, window, cx);
    }

    // "End Stage for Everyone": confirm, then end_stage.
    pub(crate) fn open_end_stage(&mut self, channel_id: String, cx: &mut Context<Self>) {
        let _ = (channel_id, cx);
    }

    // Disconnect from a stage: the last moderator on stage is asked "Do you want to also end the Stage?",
    // everyone else just leaves.
    pub(crate) fn disconnect_from_stage(&mut self, guild_id: String, channel_id: String, cx: &mut Context<Self>) {
        let _ = (guild_id, channel_id);
        self.leave_voice(cx);
    }

    // "Sorry, this Stage is full."
    pub(crate) fn open_stage_full(&mut self, cx: &mut Context<Self>) {
        let _ = cx;
    }
}
