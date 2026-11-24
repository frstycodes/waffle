// Stage channels (Discord type 13): what a stage moderator, speaker or audience member can do, as
// Discord's client does it (docs/discord-stage-re.md). The stage's look is design/explore-stage.html.
use gpui_kit::Context;
use crate::discord::Method;
use serde_json::{Value, json};

use crate::{
    Waffle,
    discord::{Guild, OnStage, Overwrite, VoiceState, perm},
    overwrites::set_tri,
    ui::Tri,
};

pub const STAGE: u8 = 13;
pub const TOPIC_MAX: usize = 120;
// Discord's start dialog ticks "Notify @everyone" for servers up to this size.
pub const NOTIFY_BY_DEFAULT_UP_TO: u64 = 50_000;

// Raised hands in the order they went up, oldest first, ties by user id: Discord's request list order,
// and the place in line Waffle numbers on each hand.
pub fn hand_queue<'a>(people: impl IntoIterator<Item = &'a VoiceState>) -> Vec<&'a VoiceState> {
    let mut queue: Vec<_> = people.into_iter().filter(|v| v.on_stage() == OnStage::HandRaised).collect();
    queue.sort_by(|a, b| (when(a), &a.user_id).cmp(&(when(b), &b.user_id)));
    queue
}

// Discord's timestamps are ISO 8601 with varying fraction digits, so compare them as instants.
fn when(v: &VoiceState) -> i64 {
    v.requested_at.as_deref().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).map_or(i64::MAX, |t| t.timestamp_micros())
}

// "1st", "2nd", "3rd", "11th", "22nd".
pub fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

// Whether @everyone may raise a hand here: the switch atop the requests popover.
pub fn requests_allowed(guild: &Guild, overwrites: &[Overwrite]) -> bool {
    let everyone = guild.roles.iter().find(|r| r.id == guild.id).map_or(0, |r| r.permissions);
    match overwrites.iter().find(|o| o.id == guild.id && !o.member) {
        Some(o) if o.deny & perm::REQUEST_TO_SPEAK != 0 => false,
        Some(o) if o.allow & perm::REQUEST_TO_SPEAK != 0 => true,
        _ => everyone & perm::REQUEST_TO_SPEAK != 0,
    }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

impl Waffle {
    // Writing a topic starts the stage; Discord then puts whoever started it on stage, silently.
    pub(crate) fn start_stage(&mut self, guild_id: String, channel_id: String, topic: String, notify: bool, cx: &mut Context<Self>) {
        // Not verified: privacy_level 2 (guild only) is what Discord's start dialog always sends; it has no picker.
        let body = json!({ "channel_id": channel_id, "topic": topic, "privacy_level": 2, "send_start_notification": notify });
        self.rest(Method::POST, "/stage-instances".into(), Some(body), move |this, cx| this.set_my_suppress(&guild_id, &channel_id, false, true, cx), cx);
    }

    pub(crate) fn edit_stage(&mut self, channel_id: &str, topic: String, cx: &mut Context<Self>) {
        self.rest(Method::PATCH, format!("/stage-instances/{channel_id}"), Some(json!({ "topic": topic })), |_, _| {}, cx);
    }

    pub(crate) fn end_stage(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        self.rest(Method::DELETE, format!("/stage-instances/{channel_id}"), None, |this, cx| this.leave_voice(cx), cx);
    }

    // Raise or lower our hand. Raising while invited declines the invite instead, as Discord does.
    pub(crate) fn raise_hand(&mut self, guild_id: &str, channel_id: &str, raise: bool, cx: &mut Context<Self>) {
        if raise && self.my_stage_state(guild_id) == Some(OnStage::Invited) {
            return self.set_my_suppress(guild_id, channel_id, true, false, cx);
        }
        let body = json!({ "request_to_speak_timestamp": raise.then(now), "channel_id": channel_id });
        self.voice_state_patch(guild_id, "@me", body, cx);
    }

    // Up to the speakers (Speak on Stage, Accept) or down to the audience (Not Now, decline). `silent` is the
    // flag Discord sends when starting a stage puts its host on it.
    pub(crate) fn set_my_suppress(&mut self, guild_id: &str, channel_id: &str, suppress: bool, silent: bool, cx: &mut Context<Self>) {
        let mut body = json!({ "suppress": suppress, "request_to_speak_timestamp": Value::Null, "channel_id": channel_id });
        if silent {
            body["silent"] = json!(true);
        }
        self.voice_state_patch(guild_id, "@me", body, cx);
    }

    // Leave the Stage: back to the audience, camera and screen off.
    pub(crate) fn leave_the_stage(&mut self, guild_id: &str, channel_id: &str, cx: &mut Context<Self>) {
        let body = json!({ "suppress": true, "channel_id": channel_id, "self_video": false, "self_stream": false });
        self.voice_state_patch(guild_id, "@me", body, cx);
    }

    // Invite to Speak from the participant menu: they're asked, not moved.
    pub(crate) fn invite_to_speak(&mut self, guild_id: &str, channel_id: &str, user_id: &str, cx: &mut Context<Self>) {
        let body = json!({ "suppress": false, "request_to_speak_timestamp": now(), "channel_id": channel_id });
        self.voice_state_patch(guild_id, user_id, body, cx);
    }

    // The requests popover's Invite to Speak (`invite`) and Dismiss. Not verified: whether Discord clears the
    // request timestamp on Dismiss; its client relies on it.
    pub(crate) fn answer_request(&mut self, guild_id: &str, channel_id: &str, user_id: &str, invite: bool, cx: &mut Context<Self>) {
        self.voice_state_patch(guild_id, user_id, json!({ "suppress": !invite, "channel_id": channel_id }), cx);
    }

    pub(crate) fn move_to_audience(&mut self, guild_id: &str, channel_id: &str, user_id: &str, cx: &mut Context<Self>) {
        self.voice_state_patch(guild_id, user_id, json!({ "suppress": true, "channel_id": channel_id }), cx);
    }

    // Allow requests from @everyone: REQUEST_TO_SPEAK moves between the @everyone overwrite's allow and deny.
    pub(crate) fn allow_requests(&mut self, guild_id: &str, channel_id: &str, allow: bool, cx: &mut Context<Self>) {
        let Some(channel) = self.guilds.iter().find(|g| g.id == guild_id).and_then(|g| g.channel(channel_id)) else { return };
        let mut overwrites = channel.overwrites.clone();
        set_tri(&mut overwrites, guild_id, false, perm::REQUEST_TO_SPEAK, if allow { Tri::Allow } else { Tri::Deny });
        let Some(o) = overwrites.iter().find(|o| o.id == guild_id) else { return };
        let body = json!({ "type": 0, "allow": o.allow.to_string(), "deny": o.deny.to_string() });
        self.rest(Method::PUT, format!("/channels/{channel_id}/permissions/{guild_id}"), Some(body), |_, _| {}, cx);
    }

    fn voice_state_patch(&mut self, guild_id: &str, user: &str, body: Value, cx: &mut Context<Self>) {
        self.rest(Method::PATCH, format!("/guilds/{guild_id}/voice-states/{user}"), Some(body), |_, _| {}, cx);
    }

    pub(crate) fn my_stage_state(&self, guild_id: &str) -> Option<OnStage> {
        let me = self.me.as_ref()?;
        self.guilds.iter().find(|g| g.id == guild_id)?.voice_states.iter().find(|v| v.user_id == me.id).map(VoiceState::on_stage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hand(id: &str, at: Option<&str>, suppress: bool) -> VoiceState {
        VoiceState { user_id: id.into(), suppress, requested_at: at.map(Into::into), ..Default::default() }
    }

    #[test]
    fn stage_status_follows_discord() {
        assert_eq!(hand("a", None, true).on_stage(), OnStage::Audience);
        assert_eq!(hand("a", Some("2026-01-01T00:00:00+00:00"), true).on_stage(), OnStage::HandRaised);
        assert_eq!(hand("a", Some("2026-01-01T00:00:00+00:00"), false).on_stage(), OnStage::Invited);
        assert_eq!(hand("a", None, false).on_stage(), OnStage::Speaker);
    }

    #[test]
    fn hands_queue_oldest_first_by_instant() {
        let people = [
            hand("kai", Some("2026-01-01T00:00:09.5+00:00"), true),
            hand("jun", Some("2026-01-01T00:00:01.123456+00:00"), true),
            hand("lea", Some("2026-01-01T00:00:01.5+00:00"), true),
            hand("invited", Some("2026-01-01T00:00:00+00:00"), false),
            hand("rosa", None, true),
        ];
        let order: Vec<_> = hand_queue(&people).iter().map(|v| v.user_id.as_str()).collect();
        assert_eq!(order, ["jun", "lea", "kai"]);
    }

    #[test]
    fn ordinals() {
        let got: Vec<_> = [1, 2, 3, 4, 11, 12, 13, 21, 22, 101, 111].into_iter().map(ordinal).collect();
        assert_eq!(got, ["1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd", "101st", "111th"]);
    }
}
