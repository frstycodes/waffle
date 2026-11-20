// Slash commands, message components and app modals: the data and the wire. Rendering lives in the
// composer (picker, option chips), `components.rs` (what a message carries) and `app_modal.rs`.
//
// Not verified: every user-account shape here (the command index, `POST /interactions` bodies and the
// INTERACTION_* dispatches) follows what Discord's client is documented to send by third parties; none
// has been checked against a capture from Discord's own client yet.

use serde_json::{json, Value};

use crate::discord::{parse_user, User};

pub const EPHEMERAL: u64 = 1 << 6;
pub const LOADING: u64 = 1 << 7;
pub const COMPONENTS_V2: u64 = 1 << 15;

/// An app that has commands where the index was fetched.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct App {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
}

/// One runnable chat-input command. Subcommands are flattened: `/music play` is its own entry whose
/// `path` is `["music", "play"]`, so the picker lists what you can actually run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AppCommand {
    pub id: String,
    pub application_id: String,
    pub version: String,
    pub path: Vec<String>,
    pub description: String,
    pub options: Vec<CommandOption>,
    // The command as the index sent it: Discord's client echoes it back in `data.application_command`.
    pub raw: Value,
}

impl AppCommand {
    pub fn name(&self) -> String {
        self.path.join(" ")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionKind {
    String,
    Integer,
    Boolean,
    User,
    Channel,
    Role,
    Mentionable,
    Number,
    Attachment,
}

impl OptionKind {
    fn from_wire(kind: u64) -> Option<Self> {
        Some(match kind {
            3 => Self::String,
            4 => Self::Integer,
            5 => Self::Boolean,
            6 => Self::User,
            7 => Self::Channel,
            8 => Self::Role,
            9 => Self::Mentionable,
            10 => Self::Number,
            11 => Self::Attachment,
            _ => return None,
        })
    }

    fn wire(self) -> u64 {
        match self {
            Self::String => 3,
            Self::Integer => 4,
            Self::Boolean => 5,
            Self::User => 6,
            Self::Channel => 7,
            Self::Role => 8,
            Self::Mentionable => 9,
            Self::Number => 10,
            Self::Attachment => 11,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CommandOption {
    pub kind: OptionKind,
    pub name: String,
    pub description: String,
    pub required: bool,
    pub choices: Vec<Choice>,
    // Discord never sends both `choices` and `autocomplete` on one option.
    pub autocomplete: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub name: String,
    pub value: Value,
}

/// `GET /guilds/{id}/application-command-index` (or the channel's, for a DM).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommandIndex {
    pub apps: Vec<App>,
    pub commands: Vec<AppCommand>,
}

pub fn parse_index(v: &Value) -> CommandIndex {
    let apps = v["applications"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let id = a["id"].as_str()?.to_owned();
            let icon = a["icon"].as_str().map(|hash| format!("https://cdn.discordapp.com/app-icons/{id}/{hash}.png?size=64"));
            Some(App { name: a["name"].as_str().unwrap_or("App").to_owned(), icon, id })
        })
        .collect();
    let mut commands = Vec::new();
    // Type 1 is chat input; 2 and 3 are the user and message context menus.
    for c in v["application_commands"].as_array().into_iter().flatten().filter(|c| c["type"].as_u64().unwrap_or(1) == 1) {
        let (Some(id), Some(app), Some(name)) = (c["id"].as_str(), c["application_id"].as_str(), c["name"].as_str()) else { continue };
        let base = AppCommand {
            id: id.to_owned(),
            application_id: app.to_owned(),
            version: c["version"].as_str().unwrap_or_default().to_owned(),
            path: vec![name.to_owned()],
            description: c["description"].as_str().unwrap_or_default().to_owned(),
            options: Vec::new(),
            raw: c.clone(),
        };
        flatten(base, &c["options"], &mut commands);
    }
    CommandIndex { apps, commands }
}

// Option type 1 is a subcommand, 2 a group of them.
fn flatten(base: AppCommand, options: &Value, out: &mut Vec<AppCommand>) {
    let options = options.as_array().map(Vec::as_slice).unwrap_or_default();
    let subs: Vec<&Value> = options.iter().filter(|o| matches!(o["type"].as_u64(), Some(1 | 2))).collect();
    if subs.is_empty() {
        out.push(AppCommand { options: options.iter().filter_map(parse_option).collect(), ..base });
        return;
    }
    for sub in subs {
        let mut path = base.path.clone();
        path.push(sub["name"].as_str().unwrap_or_default().to_owned());
        let description = sub["description"].as_str().unwrap_or_default().to_owned();
        flatten(AppCommand { path, description, ..base.clone() }, &sub["options"], out);
    }
}

fn parse_option(o: &Value) -> Option<CommandOption> {
    Some(CommandOption {
        kind: OptionKind::from_wire(o["type"].as_u64()?)?,
        name: o["name"].as_str()?.to_owned(),
        description: o["description"].as_str().unwrap_or_default().to_owned(),
        required: o["required"].as_bool().unwrap_or(false),
        choices: o["choices"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| Some(Choice { name: c["name"].as_str()?.to_owned(), value: c["value"].clone() }))
            .collect(),
        autocomplete: o["autocomplete"].as_bool().unwrap_or(false),
    })
}

/// `data.options` for a command, nesting the values under its subcommand path. `focused` marks the
/// option an autocomplete request (type 4) is asking about.
pub fn command_options(command: &AppCommand, values: &[(String, Value)], focused: Option<&str>) -> Value {
    let mut options: Vec<Value> = values
        .iter()
        .filter_map(|(name, value)| {
            let option = command.options.iter().find(|o| &o.name == name)?;
            let mut o = json!({ "type": option.kind.wire(), "name": name, "value": value });
            if focused == Some(name.as_str()) {
                o["focused"] = json!(true);
            }
            Some(o)
        })
        .collect();
    // Innermost first: `/a b c` wraps the values in c (type 1), then b (type 2).
    for (depth, name) in command.path.iter().enumerate().skip(1).rev() {
        let kind = if depth == command.path.len() - 1 { 1 } else { 2 };
        options = vec![json!({ "type": kind, "name": name, "options": options })];
    }
    json!(options)
}

/// Where an interaction happens, and the gateway session that will get its INTERACTION_* events.
pub struct Origin<'a> {
    pub channel_id: &'a str,
    pub guild_id: Option<&'a str>,
    pub session_id: &'a str,
    pub nonce: &'a str,
}

fn envelope(kind: u64, application_id: &str, origin: &Origin, data: Value) -> Value {
    let mut body = json!({
        "type": kind,
        "application_id": application_id,
        "channel_id": origin.channel_id,
        "session_id": origin.session_id,
        "nonce": origin.nonce,
        "data": data,
    });
    if let Some(guild_id) = origin.guild_id {
        body["guild_id"] = json!(guild_id);
    }
    body
}

/// Type 2 runs a command; type 4 asks it for autocomplete choices while typing `focused`.
pub fn command_body(command: &AppCommand, values: &[(String, Value)], focused: Option<&str>, origin: &Origin) -> Value {
    let data = json!({
        "version": command.version,
        "id": command.id,
        "name": command.path[0],
        "type": 1,
        "options": command_options(command, values, focused),
        "application_command": command.raw,
        "attachments": [],
    });
    envelope(if focused.is_some() { 4 } else { 2 }, &command.application_id, origin, data)
}

/// Type 3: a button press (`values` empty) or a select's picks.
pub fn component_body(application_id: &str, message_id: &str, message_flags: u64, component: &Component, values: &[String], origin: &Origin) -> Value {
    let (kind, custom_id) = match component {
        Component::Button(b) => (2, b.custom_id.clone().unwrap_or_default()),
        Component::Select(s) => (s.kind.wire(), s.custom_id.clone()),
        _ => (0, String::new()),
    };
    let mut data = json!({ "component_type": kind, "custom_id": custom_id });
    if matches!(component, Component::Select(_)) {
        data["values"] = json!(values);
    }
    let mut body = envelope(3, application_id, origin, data);
    body["message_id"] = json!(message_id);
    body["message_flags"] = json!(message_flags);
    body
}

/// Type 5: a modal's text fields, each in its own action row as Discord's client sends them.
pub fn modal_body(modal: &Modal, values: &[String], origin: &Origin) -> Value {
    let rows: Vec<Value> = modal
        .fields
        .iter()
        .zip(values)
        .map(|(f, v)| json!({ "type": 1, "components": [{ "type": 4, "custom_id": f.custom_id, "value": v }] }))
        .collect();
    envelope(5, &modal.application.id, origin, json!({ "id": modal.id, "custom_id": modal.custom_id, "components": rows }))
}

/// Who ran the command a message answers.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct InteractionMeta {
    // Discord's interaction id: `Event::InteractionCreate` pairs it with the nonce we sent.
    pub id: String,
    pub user: User,
    // Only for slash commands: a component or modal reply has none.
    pub command: Option<String>,
}

pub fn parse_interaction(m: &Value) -> Option<InteractionMeta> {
    let meta = if m["interaction_metadata"].is_object() { &m["interaction_metadata"] } else { &m["interaction"] };
    if !meta["user"].is_object() {
        return None;
    }
    // Not verified: newer payloads name the command in `interaction_metadata.name`; the legacy
    // `interaction.name` is the fallback.
    let command = meta["name"].as_str().or(m["interaction"]["name"].as_str()).filter(|_| meta["type"].as_u64().unwrap_or(2) == 2).map(str::to_owned);
    Some(InteractionMeta { id: meta["id"].as_str().unwrap_or_default().to_owned(), user: parse_user(&meta["user"]), command })
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Component {
    Row(Vec<Component>),
    Button(Button),
    Select(Select),
    Section { children: Vec<Component>, accessory: Option<Box<Component>> },
    Text(String),
    Thumbnail { url: String, description: Option<String> },
    Gallery(Vec<GalleryItem>),
    File { url: String, name: Option<String> },
    Separator { divider: bool, large: bool },
    Container { accent: Option<u32>, spoiler: bool, children: Vec<Component> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ButtonStyle {
    Primary,
    Secondary,
    Success,
    Danger,
    Link,
    Premium,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Button {
    pub style: ButtonStyle,
    pub label: Option<String>,
    // A Unicode character, or a custom emoji's CDN url.
    pub emoji: Option<String>,
    pub custom_id: Option<String>,
    pub url: Option<String>,
    pub disabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SelectKind {
    String,
    User,
    Role,
    Mentionable,
    Channel,
}

impl SelectKind {
    fn wire(self) -> u64 {
        match self {
            Self::String => 3,
            Self::User => 5,
            Self::Role => 6,
            Self::Mentionable => 7,
            Self::Channel => 8,
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Select {
    pub kind: SelectKind,
    pub custom_id: String,
    pub placeholder: Option<String>,
    pub options: Vec<SelectOption>,
    pub min: u32,
    pub max: u32,
    pub disabled: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SelectOption {
    pub label: String,
    pub value: String,
    pub description: Option<String>,
    pub emoji: Option<String>,
    pub default: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GalleryItem {
    pub url: String,
    pub description: Option<String>,
    pub spoiler: bool,
}

pub fn parse_components(v: &Value) -> Vec<Component> {
    v.as_array().into_iter().flatten().filter_map(parse_component).collect()
}

fn parse_component(c: &Value) -> Option<Component> {
    let text = |key: &str| c[key].as_str().map(str::to_owned);
    Some(match c["type"].as_u64()? {
        1 => Component::Row(parse_components(&c["components"])),
        2 => Component::Button(Button {
            style: match c["style"].as_u64().unwrap_or(2) {
                1 => ButtonStyle::Primary,
                3 => ButtonStyle::Success,
                4 => ButtonStyle::Danger,
                5 => ButtonStyle::Link,
                6 => ButtonStyle::Premium,
                _ => ButtonStyle::Secondary,
            },
            label: text("label"),
            emoji: emoji(&c["emoji"]),
            custom_id: text("custom_id"),
            url: text("url"),
            disabled: c["disabled"].as_bool().unwrap_or(false),
        }),
        kind @ (3 | 5 | 6 | 7 | 8) => Component::Select(Select {
            kind: match kind {
                5 => SelectKind::User,
                6 => SelectKind::Role,
                7 => SelectKind::Mentionable,
                8 => SelectKind::Channel,
                _ => SelectKind::String,
            },
            custom_id: text("custom_id")?,
            placeholder: text("placeholder"),
            options: c["options"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|o| {
                    Some(SelectOption {
                        label: o["label"].as_str()?.to_owned(),
                        value: o["value"].as_str()?.to_owned(),
                        description: o["description"].as_str().map(str::to_owned),
                        emoji: emoji(&o["emoji"]),
                        default: o["default"].as_bool().unwrap_or(false),
                    })
                })
                .collect(),
            min: c["min_values"].as_u64().unwrap_or(1) as u32,
            max: c["max_values"].as_u64().unwrap_or(1) as u32,
            disabled: c["disabled"].as_bool().unwrap_or(false),
        }),
        9 => Component::Section { children: parse_components(&c["components"]), accessory: parse_component(&c["accessory"]).map(Box::new) },
        10 => Component::Text(text("content").unwrap_or_default()),
        11 => Component::Thumbnail { url: c["media"]["url"].as_str()?.to_owned(), description: text("description") },
        12 => Component::Gallery(
            c["items"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|i| {
                    Some(GalleryItem {
                        url: i["media"]["url"].as_str()?.to_owned(),
                        description: i["description"].as_str().map(str::to_owned),
                        spoiler: i["spoiler"].as_bool().unwrap_or(false),
                    })
                })
                .collect(),
        ),
        13 => Component::File { url: c["file"]["url"].as_str()?.to_owned(), name: text("name") },
        14 => Component::Separator { divider: c["divider"].as_bool().unwrap_or(true), large: c["spacing"].as_u64() == Some(2) },
        17 => Component::Container {
            accent: c["accent_color"].as_u64().map(|v| v as u32),
            spoiler: c["spoiler"].as_bool().unwrap_or(false),
            children: parse_components(&c["components"]),
        },
        _ => return None,
    })
}

fn emoji(e: &Value) -> Option<String> {
    match e["id"].as_str() {
        Some(id) => Some(format!("https://cdn.discordapp.com/emojis/{id}.{}?size=48", if e["animated"].as_bool() == Some(true) { "gif" } else { "png" })),
        None => e["name"].as_str().map(str::to_owned),
    }
}

/// INTERACTION_MODAL_CREATE: a form an app asked us to fill in.
#[derive(Clone, Debug, PartialEq)]
pub struct Modal {
    // The interaction id the modal answers; the submit's `data.id`.
    pub id: String,
    pub nonce: Option<String>,
    pub channel_id: String,
    pub application: App,
    pub title: String,
    pub custom_id: String,
    pub fields: Vec<TextInput>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextInput {
    pub custom_id: String,
    pub label: String,
    // Style 2 is the multi-line paragraph.
    pub paragraph: bool,
    pub placeholder: Option<String>,
    pub value: Option<String>,
    pub required: bool,
    pub min: Option<u32>,
    pub max: Option<u32>,
}

pub fn parse_modal(d: &Value) -> Option<Modal> {
    let app = &d["application"];
    let app_id = app["id"].as_str().or(d["application_id"].as_str())?.to_owned();
    let mut fields = Vec::new();
    for row in d["components"].as_array().into_iter().flatten() {
        // Rows (type 1) wrap one input each; newer modals use labels (type 18) with `component`.
        let inputs = row["components"].as_array().cloned().unwrap_or_else(|| vec![row["component"].clone()]);
        for i in inputs.iter().filter(|i| i["type"] == 4) {
            fields.push(TextInput {
                custom_id: i["custom_id"].as_str().unwrap_or_default().to_owned(),
                label: i["label"].as_str().or(row["label"].as_str()).unwrap_or_default().to_owned(),
                paragraph: i["style"] == 2,
                placeholder: i["placeholder"].as_str().map(str::to_owned),
                value: i["value"].as_str().map(str::to_owned),
                required: i["required"].as_bool().unwrap_or(true),
                min: i["min_length"].as_u64().map(|v| v as u32),
                max: i["max_length"].as_u64().map(|v| v as u32),
            });
        }
    }
    Some(Modal {
        id: d["id"].as_str()?.to_owned(),
        nonce: d["nonce"].as_str().map(str::to_owned),
        channel_id: d["channel_id"].as_str().unwrap_or_default().to_owned(),
        application: App {
            icon: app["icon"].as_str().map(|hash| format!("https://cdn.discordapp.com/app-icons/{app_id}/{hash}.png?size=64")),
            name: app["name"].as_str().unwrap_or("App").to_owned(),
            id: app_id,
        },
        title: d["title"].as_str().unwrap_or_default().to_owned(),
        custom_id: d["custom_id"].as_str().unwrap_or_default().to_owned(),
        fields,
    })
}

/// APPLICATION_COMMAND_AUTOCOMPLETE_RESPONSE's choices.
pub fn parse_choices(d: &Value) -> Vec<Choice> {
    d["choices"].as_array().into_iter().flatten().filter_map(|c| Some(Choice { name: c["name"].as_str()?.to_owned(), value: c["value"].clone() })).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> CommandIndex {
        parse_index(&json!({
            "applications": [{ "id": "9", "name": "Wavebot", "icon": "abc" }],
            "application_commands": [
                { "id": "1", "application_id": "9", "version": "v1", "type": 1, "name": "play", "description": "Play a song",
                  "options": [{ "type": 3, "name": "query", "description": "Song", "required": true, "autocomplete": true },
                              { "type": 4, "name": "volume", "description": "0-100", "choices": [{ "name": "Loud", "value": 100 }] }] },
                { "id": "2", "application_id": "9", "version": "v2", "type": 1, "name": "music", "description": "Music",
                  "options": [{ "type": 2, "name": "queue", "description": "Queue", "options": [
                      { "type": 1, "name": "clear", "description": "Clear the queue", "options": [{ "type": 5, "name": "all", "description": "All" }] }] }] },
                { "id": "3", "application_id": "9", "version": "v3", "type": 3, "name": "Report", "description": "" }
            ]
        }))
    }

    #[test]
    fn index_flattens_subcommands_and_skips_context_menus() {
        let index = index();
        assert_eq!(index.apps[0].icon.as_deref(), Some("https://cdn.discordapp.com/app-icons/9/abc.png?size=64"));
        let names: Vec<String> = index.commands.iter().map(AppCommand::name).collect();
        assert_eq!(names, ["play", "music queue clear"]);
        let play = &index.commands[0];
        assert!(play.options[0].required && play.options[0].autocomplete);
        assert_eq!(play.options[1].choices[0].value, json!(100));
        assert_eq!(index.commands[1].description, "Clear the queue");
    }

    #[test]
    fn command_body_nests_options_under_subcommands() {
        let index = index();
        let origin = Origin { channel_id: "c", guild_id: Some("g"), session_id: "s", nonce: "n" };
        let body = command_body(&index.commands[1], &[("all".into(), json!(true))], None, &origin);
        assert_eq!(body["type"], 2);
        assert_eq!(body["guild_id"], "g");
        assert_eq!(body["data"]["name"], "music");
        assert_eq!(
            body["data"]["options"],
            json!([{ "type": 2, "name": "queue", "options": [{ "type": 1, "name": "clear", "options": [{ "type": 5, "name": "all", "value": true }] }] }])
        );
        let auto = command_body(&index.commands[0], &[("query".into(), json!("midn"))], Some("query"), &origin);
        assert_eq!(auto["type"], 4);
        assert_eq!(auto["data"]["options"][0]["focused"], true);
    }

    #[test]
    fn components_parse_rows_and_v2_layouts() {
        let parsed = parse_components(&json!([
            { "type": 1, "components": [
                { "type": 2, "style": 1, "label": "Vote", "custom_id": "v" },
                { "type": 2, "style": 5, "label": "Rules", "url": "https://x.test" }] },
            { "type": 17, "accent_color": 15706673, "components": [
                { "type": 9, "components": [{ "type": 10, "content": "**Now playing**" }], "accessory": { "type": 11, "media": { "url": "https://img.test/a.png" } } },
                { "type": 14, "spacing": 2 },
                { "type": 12, "items": [{ "media": { "url": "https://img.test/b.png" } }] }] },
            { "type": 99 }
        ]));
        assert_eq!(parsed.len(), 2);
        let Component::Row(row) = &parsed[0] else { panic!("row") };
        assert!(matches!(&row[1], Component::Button(Button { style: ButtonStyle::Link, url: Some(_), .. })));
        let Component::Container { accent, children, .. } = &parsed[1] else { panic!("container") };
        assert_eq!(*accent, Some(15706673));
        assert!(matches!(&children[0], Component::Section { accessory: Some(_), .. }));
        assert_eq!(children[1], Component::Separator { divider: true, large: true });
    }

    #[test]
    fn modal_round_trips_its_fields() {
        let modal = parse_modal(&json!({
            "id": "i1", "nonce": "n", "channel_id": "c", "title": "Suggest", "custom_id": "suggest",
            "application": { "id": "9", "name": "Quizzy" },
            "components": [{ "type": 1, "components": [{ "type": 4, "custom_id": "q", "label": "Question", "style": 1, "max_length": 200 }] },
                           { "type": 1, "components": [{ "type": 4, "custom_id": "why", "label": "Why", "style": 2, "required": false }] }]
        }))
        .unwrap();
        assert_eq!(modal.fields.len(), 2);
        assert!(modal.fields[1].paragraph && !modal.fields[1].required);
        let origin = Origin { channel_id: "c", guild_id: None, session_id: "s", nonce: "n2" };
        let body = modal_body(&modal, &["Doom or Myst?".into(), String::new()], &origin);
        assert_eq!(body["type"], 5);
        assert_eq!(body["application_id"], "9");
        assert_eq!(body["data"]["components"][0]["components"][0], json!({ "type": 4, "custom_id": "q", "value": "Doom or Myst?" }));
    }

    #[test]
    fn interaction_meta_names_the_command() {
        let meta = parse_interaction(&json!({ "interaction_metadata": { "type": 2, "name": "play", "user": { "id": "u", "username": "mira" } } })).unwrap();
        assert_eq!(meta.command.as_deref(), Some("play"));
        let button = parse_interaction(&json!({ "interaction_metadata": { "type": 3, "user": { "id": "u", "username": "mira" } } })).unwrap();
        assert_eq!(button.command, None);
    }
}
