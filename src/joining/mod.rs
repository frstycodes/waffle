// Joining a server the way Discord runs it (docs/discord-joining-re.md, design/explore-joining.html): the
// invite screen, onboarding questions, the rules gate in the composer, Channels & Roles and the Server Guide.
// This file holds what they share: Discord's data, the calls, the caches and the hooks the rest of the app
// calls. Each screen lives in its own file.
mod browse;
pub mod fixtures;
mod guide;
mod invite;
mod onboarding;
mod rows;
mod rules;

use std::collections::{HashMap, HashSet};

use gpui_kit::{AnyElement, Context, IntoElement, Window};
use reqwest::Method;
use serde_json::{Map, Value, json};

pub use browse::BrowseView;
pub use guide::GuideView;
pub use invite::InviteView;
pub use onboarding::OnboardingView;
pub use rules::RulesView;

use crate::{Waffle, channel_settings::Load, discord::Guild, server_settings::api::Call};

// Guild member flags (member `flags`).
pub mod member_flags {
    pub const COMPLETED_ONBOARDING: u64 = 1 << 1;
    pub const STARTED_ONBOARDING: u64 = 1 << 3;
    pub const STARTED_HOME_ACTIONS: u64 = 1 << 5;
    pub const COMPLETED_HOME_ACTIONS: u64 = 1 << 6;
}

// user_guild_settings `flags`, and `channel_overrides[].flags`.
pub mod settings_flags {
    pub const OPT_IN_CHANNELS_ON: u64 = 1 << 14;
    pub const OPT_IN_ENABLED: u64 = 1 << 12;
}

pub mod features {
    pub const COMMUNITY: &str = "COMMUNITY";
    pub const ONBOARDING: &str = "GUILD_ONBOARDING";
    pub const ONBOARDING_HAS_PROMPTS: &str = "GUILD_ONBOARDING_HAS_PROMPTS";
    pub const SERVER_GUIDE: &str = "GUILD_SERVER_GUIDE";
    pub const RULES_GATE: &str = "MEMBER_VERIFICATION_GATE_ENABLED";
    pub const MANUAL_APPROVAL: &str = "MEMBER_VERIFICATION_MANUAL_APPROVAL";
    pub const VERIFIED: &str = "VERIFIED";
    pub const PARTNERED: &str = "PARTNERED";
}

// A server page in the content column, opened from the rows atop the channel list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuildPage {
    Guide,
    ChannelsRoles,
}

#[derive(Default)]
pub struct Join {
    // Servers joined this session: their gates (onboarding, rules) apply on arrival.
    pub just_joined: HashSet<String>,
    pub onboarding: HashMap<String, Load<Onboarding>>,
    pub forms: HashMap<String, Load<Form>>,
    pub welcome: HashMap<String, Load<Welcome>>,
    // Server → the to-do channels done (GET new-member-actions, then each completion).
    pub done_actions: HashMap<String, HashSet<String>>,
    pub invite: Option<InviteView>,
    // Server → the nickname picked on its invite screen, set once the join goes through.
    pub invite_settings: HashMap<String, invite::Picked>,
    pub onboard: Option<OnboardingView>,
    // Onboarding closed halfway, by server: its picks and step, for Finish joining.
    pub paused_onboarding: HashMap<String, OnboardingView>,
    pub rules: Option<RulesView>,
    pub browse: BrowseView,
    pub guide: GuideView,
}

// The rules form (GET /guilds/{id}/member-verification). `fields` stays raw so Submit echoes it back.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Form {
    pub version: String,
    pub description: Option<String>,
    pub rules: Vec<String>,
    // Fields other than the rules make it an application (apply to join), which Waffle doesn't fill in yet.
    pub has_questions: bool,
    pub fields: Vec<Value>,
}

impl Form {
    pub fn parse(v: &Value) -> Self {
        let fields: Vec<Value> = v["form_fields"].as_array().cloned().unwrap_or_default();
        let terms = fields.iter().find(|f| f["field_type"] == "TERMS");
        Form {
            version: v["version"].as_str().unwrap_or_default().to_owned(),
            description: v["description"].as_str().filter(|d| !d.is_empty()).map(str::to_owned),
            rules: terms.map(|t| strings(&t["values"])).unwrap_or_default(),
            has_questions: fields.iter().any(|f| f["field_type"] != "TERMS"),
            fields,
        }
    }

    // PUT /guilds/{id}/requests/@me: the form back with the rules agreed to.
    pub fn agreed_body(&self) -> Value {
        let fields: Vec<Value> = self
            .fields
            .iter()
            .map(|f| {
                let mut f = f.clone();
                if f["field_type"] == "TERMS" {
                    f["response"] = json!(true);
                }
                f
            })
            .collect();
        json!({ "version": self.version, "form_fields": fields })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptKind {
    MultipleChoice,
    Dropdown,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OptionEmoji {
    pub id: Option<String>,
    pub name: Option<String>,
    pub animated: bool,
}

impl OptionEmoji {
    fn parse(v: &Value) -> Option<Self> {
        let emoji = OptionEmoji { id: v["id"].as_str().map(str::to_owned), name: v["name"].as_str().filter(|n| !n.is_empty()).map(str::to_owned), animated: v["animated"].as_bool().unwrap_or(false) };
        (emoji.id.is_some() || emoji.name.is_some()).then_some(emoji)
    }

    // A custom emoji's image; unicode ones render as text from `name`.
    pub fn url(&self) -> Option<String> {
        let id = self.id.as_ref()?;
        Some(format!("{}/emojis/{id}.{}?size=48", crate::discord::CDN, if self.animated { "gif" } else { "png" }))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PromptOption {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub emoji: Option<OptionEmoji>,
    pub role_ids: Vec<String>,
    pub channel_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Prompt {
    pub id: String,
    pub title: String,
    pub kind: PromptKind,
    pub single_select: bool,
    pub required: bool,
    pub in_onboarding: bool,
    pub options: Vec<PromptOption>,
}

// GET /guilds/{id}/onboarding.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Onboarding {
    pub enabled: bool,
    pub prompts: Vec<Prompt>,
    pub default_channel_ids: Vec<String>,
    // The option ids you picked so far.
    pub responses: Vec<String>,
    pub prompts_seen: Vec<String>,
}

impl Onboarding {
    pub fn parse(v: &Value) -> Self {
        let prompts = v["prompts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| !p["disabled"].as_bool().unwrap_or(false))
            .filter_map(|p| {
                Some(Prompt {
                    id: p["id"].as_str()?.to_owned(),
                    title: p["title"].as_str().unwrap_or_default().to_owned(),
                    kind: if p["type"].as_u64() == Some(1) { PromptKind::Dropdown } else { PromptKind::MultipleChoice },
                    single_select: p["single_select"].as_bool().unwrap_or(false),
                    required: p["required"].as_bool().unwrap_or(false),
                    in_onboarding: p["in_onboarding"].as_bool().unwrap_or(false),
                    options: p["options"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|o| {
                            Some(PromptOption {
                                id: o["id"].as_str()?.to_owned(),
                                title: o["title"].as_str().unwrap_or_default().to_owned(),
                                description: o["description"].as_str().filter(|d| !d.is_empty()).map(str::to_owned),
                                emoji: OptionEmoji::parse(&o["emoji"]),
                                role_ids: strings(&o["role_ids"]),
                                channel_ids: strings(&o["channel_ids"]),
                            })
                        })
                        .collect(),
                })
            })
            .collect();
        Onboarding {
            enabled: v["enabled"].as_bool().unwrap_or(false),
            prompts,
            default_channel_ids: strings(&v["default_channel_ids"]),
            responses: strings(&v["responses"]),
            prompts_seen: v["onboarding_prompts_seen"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default(),
        }
    }

    // The questions shown full screen right after joining.
    pub fn onboarding_prompts(&self) -> impl Iterator<Item = &Prompt> {
        self.prompts.iter().filter(|p| p.in_onboarding)
    }

    // Body for POST (finishing the full-screen flow) or PUT (Channels & Roles) onboarding-responses: the
    // picked option ids among `prompts`, and every prompt and option shown marked seen now.
    pub fn responses_body<'a>(prompts: impl IntoIterator<Item = &'a Prompt>, picked: &HashSet<String>, now_ms: i64) -> Value {
        let (mut responses, mut prompts_seen, mut options_seen) = (Vec::new(), Map::new(), Map::new());
        for prompt in prompts {
            prompts_seen.insert(prompt.id.clone(), json!(now_ms));
            for option in &prompt.options {
                options_seen.insert(option.id.clone(), json!(now_ms));
                if picked.contains(&option.id) {
                    responses.push(json!(option.id));
                }
            }
        }
        json!({ "onboarding_responses": responses, "onboarding_prompts_seen": prompts_seen, "onboarding_responses_seen": options_seen })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    View,
    Chat,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemberAction {
    pub channel_id: String,
    pub kind: ActionKind,
    pub title: String,
    pub description: Option<String>,
    pub emoji: Option<OptionEmoji>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Resource {
    pub channel_id: String,
    pub title: String,
    pub description: Option<String>,
    pub emoji: Option<OptionEmoji>,
}

// GET /guilds/{id}/new-member-welcome: the Server Guide.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Welcome {
    pub enabled: bool,
    pub message: Option<String>,
    pub author_ids: Vec<String>,
    pub actions: Vec<MemberAction>,
    pub resources: Vec<Resource>,
}

impl Welcome {
    pub fn parse(v: &Value) -> Self {
        let message = &v["welcome_message"];
        Welcome {
            enabled: v["enabled"].as_bool().unwrap_or(false),
            // Discord only shows a welcome of 7 characters or more that names its authors.
            message: message["message"].as_str().filter(|m| m.chars().count() >= 7 && message["author_ids"].as_array().is_some_and(|a| !a.is_empty())).map(str::to_owned),
            author_ids: strings(&message["author_ids"]),
            actions: v["new_member_actions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| {
                    Some(MemberAction {
                        channel_id: a["channel_id"].as_str()?.to_owned(),
                        kind: if a["action_type"].as_u64() == Some(1) { ActionKind::Chat } else { ActionKind::View },
                        title: a["title"].as_str().unwrap_or_default().to_owned(),
                        description: a["description"].as_str().filter(|d| !d.is_empty()).map(str::to_owned),
                        emoji: OptionEmoji::parse(&a["emoji"]),
                    })
                })
                .collect(),
            resources: v["resource_channels"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|r| {
                    Some(Resource {
                        channel_id: r["channel_id"].as_str()?.to_owned(),
                        title: r["title"].as_str().unwrap_or_default().to_owned(),
                        description: r["description"].as_str().filter(|d| !d.is_empty()).map(str::to_owned),
                        emoji: OptionEmoji::parse(&r["emoji"]),
                    })
                })
                .collect(),
        }
    }
}

// GET /guilds/{id}/new-member-actions: `{channel_actions: {id: {completed}}}`.
pub fn parse_done_actions(v: &Value) -> HashSet<String> {
    v["channel_actions"].as_object().into_iter().flatten().filter(|(_, a)| a["completed"].as_bool() == Some(true)).map(|(id, _)| id.clone()).collect()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_owned)).collect()
}

// Every REST call the joining screens make. Routes from Discord's own client (docs/discord-joining-re.md).
pub mod api {
    use super::*;

    pub fn invite(code: &str) -> Call {
        Call::new(Method::GET, format!("/invites/{code}?with_counts=true"), None)
    }
    pub fn form(guild_id: &str) -> Call {
        Call::new(Method::GET, format!("/guilds/{guild_id}/member-verification?with_guild=false"), None)
    }
    pub fn agree(guild_id: &str, form: &Form) -> Call {
        Call::new(Method::PUT, format!("/guilds/{guild_id}/requests/@me"), Some(form.agreed_body()))
    }
    pub fn onboarding(guild_id: &str) -> Call {
        Call::new(Method::GET, format!("/guilds/{guild_id}/onboarding"), None)
    }
    // POST finishes the full-screen flow; PUT saves from Channels & Roles.
    pub fn answer(guild_id: &str, finish: bool, body: Value) -> Call {
        Call::new(if finish { Method::POST } else { Method::PUT }, format!("/guilds/{guild_id}/onboarding-responses"), Some(body))
    }
    pub fn welcome(guild_id: &str) -> Call {
        Call::new(Method::GET, format!("/guilds/{guild_id}/new-member-welcome"), None)
    }
    pub fn done_actions(guild_id: &str) -> Call {
        Call::new(Method::GET, format!("/guilds/{guild_id}/new-member-actions"), None)
    }
    pub fn complete_action(guild_id: &str, channel_id: &str) -> Call {
        Call::new(Method::POST, format!("/guilds/{guild_id}/new-member-action/{channel_id}"), None)
    }
    // Not verified: the call Discord makes for the invite screen's nickname; this is the documented one.
    pub fn nickname(guild_id: &str, nick: &str) -> Call {
        Call::new(Method::PATCH, format!("/guilds/{guild_id}/members/@me"), Some(json!({ "nick": nick })))
    }
}

impl Waffle {
    // A server we joined, now in our list: its gates start here (onboarding or the rules bar).
    pub(crate) fn on_joined(&mut self, guild_id: &str, cx: &mut Context<Self>) {
        if !self.join.just_joined.remove(guild_id) {
            return;
        }
        if let Some(guild) = self.guilds.iter_mut().find(|g| g.id == guild_id) {
            let pending = guild.has_feature(features::RULES_GATE);
            guild.me = crate::discord::Membership { pending, joined_at: Some(chrono::Utc::now().to_rfc3339()), ..guild.me.clone() };
        }
        self.start_onboarding_if_needed(guild_id.to_owned(), cx);
    }

    pub(crate) fn fetch_onboarding(&mut self, guild_id: &str, then: impl FnOnce(&mut Waffle, &mut Context<Waffle>) + 'static, cx: &mut Context<Self>) {
        let id = guild_id.to_owned();
        self.join.onboarding.insert(id.clone(), Load::Loading);
        self.server_call(api::onboarding(guild_id), move |this, result, cx| {
            this.join.onboarding.insert(id, result.map(|v| Onboarding::parse(&v)).map_or_else(Load::Failed, Load::Ready));
            then(this, cx);
        }, cx);
    }

    pub(crate) fn fetch_form(&mut self, guild_id: &str, cx: &mut Context<Self>) {
        let id = guild_id.to_owned();
        if matches!(self.join.forms.get(&id), Some(Load::Ready(_) | Load::Loading)) {
            return;
        }
        self.join.forms.insert(id.clone(), Load::Loading);
        self.server_call(api::form(guild_id), move |this, result, _| {
            this.join.forms.insert(id, result.map(|v| Form::parse(&v)).map_or_else(Load::Failed, Load::Ready));
        }, cx);
    }

    pub(crate) fn fetch_welcome(&mut self, guild_id: &str, cx: &mut Context<Self>) {
        let id = guild_id.to_owned();
        if matches!(self.join.welcome.get(&id), Some(Load::Ready(_) | Load::Loading)) {
            return;
        }
        self.join.welcome.insert(id.clone(), Load::Loading);
        let started = self.guilds.iter().find(|g| g.id == id).is_some_and(|g| g.me.flags & member_flags::STARTED_HOME_ACTIONS != 0);
        let key = id.clone();
        self.server_call(api::welcome(guild_id), move |this, result, _| {
            this.join.welcome.insert(key, result.map(|v| Welcome::parse(&v)).map_or_else(Load::Failed, Load::Ready));
        }, cx);
        if started {
            let key = id.clone();
            self.server_call(api::done_actions(&id), move |this, result, _| {
                if let Ok(v) = result {
                    // Merge: a to-do ticked while this was in flight stays ticked.
                    this.join.done_actions.entry(key).or_default().extend(parse_done_actions(&v));
                }
            }, cx);
        }
    }

    pub(crate) fn open_guild_page(&mut self, page: GuildPage, cx: &mut Context<Self>) {
        self.guild_page = Some(page);
        self.inbox_page = false;
        cx.notify();
    }

    pub(crate) fn render_guild_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(guild) = self.guild().cloned() else { return gpui_kit::div().into_any_element() };
        match self.guild_page {
            Some(GuildPage::ChannelsRoles) => self.render_channels_roles(&guild, window, cx),
            _ => self.render_guide(&guild, window, cx),
        }
    }

    pub(crate) fn join_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::new();
        out.extend(self.rules_overlay(window, cx));
        out.extend(self.onboarding_overlay(window, cx));
        out.extend(self.invite_overlay(window, cx));
        out
    }

    // Escape: the rules dialog and the invite screen close; onboarding keeps its own Close button.
    pub(crate) fn dismiss_join_overlay(&mut self) -> bool {
        self.join.rules.take().is_some() | self.join.invite.take().is_some()
    }

    // Whether this server is one we should treat as "new member" (7 days, to-dos not all done).
    pub(crate) fn is_new_member(guild: &Guild) -> bool {
        let recent = guild.me.joined_at.as_deref().and_then(|j| chrono::DateTime::parse_from_rfc3339(j).ok()).is_some_and(|j| chrono::Utc::now().signed_duration_since(j).num_days() < 7);
        recent && guild.me.flags & member_flags::COMPLETED_HOME_ACTIONS == 0
    }
}

pub fn demo(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    fixtures::install(waffle);
    match scene.split('-').next().unwrap_or_default() {
        "invite" => invite::demo(waffle, scene, window, cx),
        "onboarding" => onboarding::demo(waffle, scene, window, cx),
        "rules" => rules::demo(waffle, scene, window, cx),
        "roles" => browse::demo(waffle, scene, window, cx),
        _ => guide::demo(waffle, scene, window, cx),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_agrees_to_rules_and_keeps_other_fields() {
        let form = Form::parse(&json!({ "version": "2024", "form_fields": [
            { "field_type": "TERMS", "label": "Read and agree", "values": ["Be kind", "No spam"], "required": true },
            { "field_type": "TEXT_INPUT", "label": "Why join?", "required": false }
        ]}));
        assert_eq!(form.rules, vec!["Be kind", "No spam"]);
        assert!(form.has_questions);
        let body = form.agreed_body();
        assert_eq!(body["version"], "2024");
        assert_eq!(body["form_fields"][0]["response"], true);
        assert!(body["form_fields"][1].get("response").is_none());
    }

    #[test]
    fn onboarding_parses_and_answers() {
        let ob = Onboarding::parse(&json!({ "enabled": true, "default_channel_ids": ["c1"], "responses": ["o2"],
            "onboarding_prompts_seen": { "p1": 1 },
            "prompts": [
                { "id": "p1", "title": "Games?", "type": 0, "single_select": false, "required": true, "in_onboarding": true,
                  "options": [{ "id": "o1", "title": "Rift", "emoji": { "name": "⚔️" }, "role_ids": ["r1"], "channel_ids": ["c2"] }, { "id": "o2", "title": "Hades", "emoji": { "id": null, "name": "" } }] },
                { "id": "p2", "title": "Region", "type": 1, "single_select": true, "in_onboarding": false, "options": [] },
                { "id": "p3", "title": "Off", "disabled": true, "options": [] }
            ]}));
        assert_eq!(ob.prompts.len(), 2);
        assert_eq!(ob.onboarding_prompts().count(), 1);
        assert_eq!(ob.prompts[1].kind, PromptKind::Dropdown);
        assert_eq!(ob.prompts[0].options[0].emoji.as_ref().and_then(|e| e.name.clone()).as_deref(), Some("⚔️"));
        assert_eq!(ob.prompts[0].options[1].emoji, None);
        assert_eq!(ob.prompts_seen, vec!["p1"]);
        let picked: HashSet<String> = ["o1".to_owned()].into();
        let body = Onboarding::responses_body(ob.onboarding_prompts(), &picked, 5);
        assert_eq!(body["onboarding_responses"], json!(["o1"]));
        assert_eq!(body["onboarding_prompts_seen"]["p1"], 5);
        assert_eq!(body["onboarding_responses_seen"]["o2"], 5);
    }

    #[test]
    fn welcome_needs_authors_and_seven_characters() {
        let w = Welcome::parse(&json!({ "enabled": true, "welcome_message": { "author_ids": ["u1"], "message": "Hi all" },
            "new_member_actions": [{ "channel_id": "c1", "action_type": 1, "title": "Say hi", "emoji": { "name": "👋" } }],
            "resource_channels": [{ "channel_id": "c2", "title": "FAQ" }] }));
        assert_eq!(w.message, None);
        assert_eq!(w.actions[0].kind, ActionKind::Chat);
        assert_eq!(w.resources[0].title, "FAQ");
        let done = parse_done_actions(&json!({ "channel_actions": { "c1": { "completed": true }, "c2": { "completed": false } } }));
        assert_eq!(done, ["c1".to_owned()].into());
    }
}
