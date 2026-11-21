// Slash commands, message components and app modals: the data and the wire. Rendering lives in the
// composer (picker, option chips), `components.rs` (what a message carries) and `app_modal.rs`.
//
// Shapes follow Discord's own web client (docs/discord-app-commands-re.md, read from its bundles); none
// has been checked against a live capture yet.

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
    // Untranslated names (the index's `name_default`): what the wire carries.
    pub path: Vec<String>,
    // The same parts as the index shows them in your language, where they differ.
    pub path_localized: Vec<Option<String>>,
    pub description: String,
    pub options: Vec<CommandOption>,
    // A command registered to one server: Discord echoes it in `data.guild_id`.
    pub guild_id: Option<String>,
    // The root command, normalized as Discord's client keeps it: echoed back in `data.application_command`.
    pub raw: Value,
}

impl AppCommand {
    pub fn name(&self) -> String {
        self.path.join(" ")
    }

    /// The path as you see it: each localized part, else the untranslated one.
    pub fn display_name(&self) -> String {
        let parts: Vec<&str> = self.path.iter().enumerate().map(|(i, part)| self.path_localized.get(i).and_then(Option::as_deref).unwrap_or(part)).collect();
        parts.join(" ")
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OptionKind {
    #[default]
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommandOption {
    pub kind: OptionKind,
    // Untranslated, as the wire carries it; `name_localized` is what you see.
    pub name: String,
    pub name_localized: Option<String>,
    pub description: String,
    pub required: bool,
    pub choices: Vec<Choice>,
    // Discord never sends both `choices` and `autocomplete` on one option.
    pub autocomplete: bool,
    // INTEGER and NUMBER bounds.
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
    // STRING bounds, in UTF-16 units as Discord's client counts them (`.length`).
    pub min_length: Option<u32>,
    pub max_length: Option<u32>,
    // CHANNEL options: the channel types allowed; empty allows every type.
    pub channel_types: Vec<u8>,
}

impl CommandOption {
    pub fn display_name(&self) -> &str {
        self.name_localized.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Choice {
    pub name: String,
    pub name_localized: Option<String>,
    pub value: Value,
}

impl Choice {
    pub fn display_name(&self) -> &str {
        self.name_localized.as_deref().unwrap_or(&self.name)
    }

    fn parse(c: &Value) -> Option<Self> {
        let (name, name_localized) = names(c)?;
        Some(Choice { name, name_localized, value: c["value"].clone() })
    }
}

/// `GET /guilds/{id}/application-command-index` (or the channel's, for a DM), merged with
/// `GET /users/@me/application-command-index` (user-installed apps).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommandIndex {
    pub apps: Vec<App>,
    pub commands: Vec<AppCommand>,
}

impl CommandIndex {
    /// Discord's merge: an app in both keeps the user index's commands, the context's extra ones appended.
    pub fn merge(context: CommandIndex, user: CommandIndex) -> CommandIndex {
        let mut apps = user.apps;
        let extra_apps: Vec<App> = context.apps.into_iter().filter(|a| !apps.iter().any(|u| u.id == a.id)).collect();
        apps.extend(extra_apps);
        let mut commands = user.commands;
        let extra: Vec<AppCommand> = context.commands.into_iter().filter(|c| !commands.iter().any(|u| u.id == c.id && u.path == c.path)).collect();
        commands.extend(extra);
        CommandIndex { apps, commands }
    }
}

// The untranslated name (`name_default`, else `name`) and the localized one where it differs.
fn names(v: &Value) -> Option<(String, Option<String>)> {
    let name = v["name_default"].as_str().or(v["name"].as_str())?.to_owned();
    let localized = v["name_localized"].as_str().or_else(|| v["name_default"].as_str().and(v["name"].as_str())).filter(|l| *l != name).map(str::to_owned);
    Some((name, localized))
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
        let (Some(id), Some(app), Some((name, localized))) = (c["id"].as_str(), c["application_id"].as_str(), names(c)) else { continue };
        let base = AppCommand {
            id: id.to_owned(),
            application_id: app.to_owned(),
            version: c["version"].as_str().unwrap_or_default().to_owned(),
            path: vec![name.clone()],
            path_localized: vec![localized.clone()],
            description: c["description"].as_str().unwrap_or_default().to_owned(),
            options: Vec::new(),
            guild_id: c["guild_id"].as_str().map(str::to_owned),
            raw: normalized(c, &name, localized.as_deref()),
        };
        flatten(base, &c["options"], &mut commands);
    }
    CommandIndex { apps, commands }
}

// Discord's index normalizer maps `name_default` to `name` and keeps the localized one as `name_localized`.
// Not verified: which other fields its normalized object carries once serialized into the body.
fn normalized(c: &Value, name: &str, localized: Option<&str>) -> Value {
    let mut raw = c.clone();
    raw["name"] = json!(name);
    if let Some(localized) = localized {
        raw["name_localized"] = json!(localized);
    }
    raw
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
        let Some((name, localized)) = names(sub) else { continue };
        let mut path = base.path.clone();
        path.push(name);
        let mut path_localized = base.path_localized.clone();
        path_localized.push(localized);
        let description = sub["description"].as_str().unwrap_or_default().to_owned();
        flatten(AppCommand { path, path_localized, description, ..base.clone() }, &sub["options"], out);
    }
}

fn parse_option(o: &Value) -> Option<CommandOption> {
    let (name, name_localized) = names(o)?;
    let length = |key: &str| o[key].as_u64().map(|v| v as u32);
    Some(CommandOption {
        kind: OptionKind::from_wire(o["type"].as_u64()?)?,
        name,
        name_localized,
        description: o["description"].as_str().unwrap_or_default().to_owned(),
        required: o["required"].as_bool().unwrap_or(false),
        choices: o["choices"].as_array().into_iter().flatten().filter_map(Choice::parse).collect(),
        autocomplete: o["autocomplete"].as_bool().unwrap_or(false),
        min_value: o["min_value"].as_f64(),
        max_value: o["max_value"].as_f64(),
        min_length: length("min_length"),
        max_length: length("max_length"),
        channel_types: channel_types(&o["channel_types"]),
    })
}

fn channel_types(v: &Value) -> Vec<u8> {
    v.as_array().into_iter().flatten().filter_map(|t| t.as_u64().map(|t| t as u8)).collect()
}

/// `data.options` for a command in its definition order (as Discord sends them, whatever order they
/// were typed in), nested under its subcommand path. `focused` marks the option an autocomplete
/// request (type 4) is asking about.
pub fn command_options(command: &AppCommand, values: &[(String, Value)], focused: Option<&str>) -> Value {
    let mut options: Vec<Value> = command
        .options
        .iter()
        .filter_map(|option| {
            let (name, value) = values.iter().find(|(name, _)| *name == option.name)?;
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

/// Type 2 runs a command; type 4 asks it for autocomplete choices while typing `focused`. Only type 2
/// carries `data.attachments` (empty until `attach_files`) and the composer's `analytics_location`.
pub fn command_body(command: &AppCommand, values: &[(String, Value)], focused: Option<&str>, origin: &Origin) -> Value {
    let mut data = json!({
        "version": command.version,
        "id": command.id,
        "name": command.path[0],
        "type": 1,
        "options": command_options(command, values, focused),
        "application_command": command.raw,
    });
    if let Some(guild_id) = &command.guild_id {
        data["guild_id"] = json!(guild_id);
    }
    if focused.is_some() {
        return envelope(4, &command.application_id, origin, data);
    }
    data["attachments"] = json!([]);
    let mut body = envelope(2, &command.application_id, origin, data);
    body["analytics_location"] = json!("slash_ui");
    body
}

/// A file for an ATTACHMENT option, already uploaded (`Client::stage_files`). The option's value is
/// the file's index in the list given to `attach_files`, as a number.
#[derive(Clone, Debug, PartialEq)]
pub struct CommandAttachment {
    pub filename: String,
    pub uploaded_filename: String,
    pub spoiler: bool,
}

/// Fills a type-2 body's `data.attachments` as Discord's client does: `id` is the index as a string.
pub fn attach_files(body: &mut Value, files: &[CommandAttachment]) {
    let files: Vec<Value> = files
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let mut a = json!({ "id": i.to_string(), "filename": f.filename, "uploaded_filename": f.uploaded_filename });
            if f.spoiler {
                a["is_spoiler"] = json!(true);
            }
            a
        })
        .collect();
    body["data"]["attachments"] = json!(files);
}

/// Type 3: a button press (`values` empty) or a select's picks. A select names its type twice, in
/// `component_type` and `type`, as Discord's client sends it.
pub fn component_body(application_id: &str, message_id: &str, message_flags: u64, component: &Component, values: &[String], origin: &Origin) -> Value {
    let (kind, custom_id) = match component {
        Component::Button(b) => (2, b.custom_id.clone().unwrap_or_default()),
        Component::Select(s) => (s.kind.wire(), s.custom_id.clone()),
        _ => (0, String::new()),
    };
    let mut data = json!({ "component_type": kind, "custom_id": custom_id });
    if matches!(component, Component::Select(_)) {
        data["type"] = json!(kind);
        data["values"] = json!(values);
    }
    let mut body = envelope(3, application_id, origin, data);
    body["message_id"] = json!(message_id);
    body["message_flags"] = json!(message_flags);
    body
}

/// Type 5: the modal's own tree echoed back with each field's value; `values` follows `Modal::fields`.
pub fn modal_body(modal: &Modal, values: &[FieldValue], origin: &Origin) -> Value {
    let mut values = values.iter();
    let components = submitted(&modal.components, &mut values);
    envelope(5, &modal.application.id, origin, json!({ "id": modal.id, "custom_id": modal.custom_id, "components": components }))
}

// Discord's shapes: a field never given a value goes out as `null` (a checkbox as `false`). A file
// upload always goes out unset: Waffle can't stage files in a modal yet.
fn submitted<'a>(components: &[ModalComponent], values: &mut impl Iterator<Item = &'a FieldValue>) -> Vec<Value> {
    components
        .iter()
        .map(|c| match c {
            ModalComponent::Row(children) => json!({ "type": 1, "components": submitted(children, values) }),
            ModalComponent::Label { component, .. } => {
                let inner = submitted(std::slice::from_ref(component.as_ref()), values);
                json!({ "type": 18, "component": inner[0] })
            }
            ModalComponent::Text(_) => json!({ "type": 10 }),
            ModalComponent::TextInput(t) => json!({ "type": 4, "custom_id": t.custom_id, "value": values.next().and_then(FieldValue::text) }),
            ModalComponent::Select(s) => json!({ "type": s.kind.wire(), "custom_id": s.custom_id, "values": values.next().and_then(FieldValue::picks) }),
            ModalComponent::FileUpload(f) => {
                values.next();
                json!({ "type": 19, "custom_id": f.custom_id, "values": null })
            }
            ModalComponent::RadioGroup(g) => json!({ "type": 21, "custom_id": g.custom_id, "value": values.next().and_then(FieldValue::picks).and_then(|p| p.first().cloned()) }),
            ModalComponent::CheckboxGroup(g) => json!({ "type": 22, "custom_id": g.custom_id, "values": values.next().and_then(FieldValue::picks) }),
            ModalComponent::Checkbox { custom_id, .. } => json!({ "type": 23, "custom_id": custom_id, "value": values.next() == Some(&FieldValue::Checked(true)) }),
        })
        .collect()
}

/// Who ran the command a message answers.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct InteractionMeta {
    // Discord's interaction id: `Event::InteractionCreate` pairs it with the nonce we sent.
    pub id: String,
    pub user: User,
    // Only for slash commands: a component or modal reply has none.
    pub command: Option<String>,
    // `interaction_metadata.ephemerality_reason`: why an ephemeral reply is only yours (Discord's enum).
    #[serde(default)]
    pub ephemerality_reason: Option<u64>,
}

pub fn parse_interaction(m: &Value) -> Option<InteractionMeta> {
    let meta = if m["interaction_metadata"].is_object() { &m["interaction_metadata"] } else { &m["interaction"] };
    if !meta["user"].is_object() {
        return None;
    }
    // Discord's "used" header reads the legacy `interaction.name`; `interaction_metadata.name` is the fallback.
    let command = m["interaction"]["name"].as_str().or(meta["name"].as_str()).filter(|_| meta["type"].as_u64().unwrap_or(2) == 2).map(str::to_owned);
    Some(InteractionMeta {
        id: meta["id"].as_str().unwrap_or_default().to_owned(),
        user: parse_user(&meta["user"]),
        command,
        ephemerality_reason: m["interaction_metadata"]["ephemerality_reason"].as_u64(),
    })
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Component {
    Row(Vec<Component>),
    Button(Button),
    Select(Select),
    Section {
        children: Vec<Component>,
        accessory: Option<Box<Component>>,
    },
    Text(String),
    Thumbnail {
        url: String,
        description: Option<String>,
        #[serde(default)]
        spoiler: bool,
        // The media's own size, when Discord sends it.
        #[serde(default)]
        width: Option<u32>,
        #[serde(default)]
        height: Option<u32>,
    },
    Gallery(Vec<GalleryItem>),
    File {
        url: String,
        name: Option<String>,
        // Bytes.
        #[serde(default)]
        size: Option<u64>,
        #[serde(default)]
        spoiler: bool,
    },
    Separator {
        divider: bool,
        large: bool,
    },
    Container {
        accent: Option<u32>,
        spoiler: bool,
        children: Vec<Component>,
    },
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
    // A Premium button's SKU.
    #[serde(default)]
    pub sku_id: Option<String>,
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
    // Discord's client defaults it to false when the app leaves it out.
    #[serde(default)]
    pub required: bool,
    // User, role, mentionable and channel selects: what starts picked.
    #[serde(default)]
    pub default_values: Vec<DefaultValue>,
    // Channel selects: the channel types listed; empty lists every type.
    #[serde(default)]
    pub channel_types: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DefaultValue {
    pub id: String,
    // "user", "role" or "channel".
    pub kind: String,
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
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

pub fn parse_components(v: &Value) -> Vec<Component> {
    v.as_array().into_iter().flatten().filter_map(parse_component).collect()
}

fn parse_component(c: &Value) -> Option<Component> {
    let text = |key: &str| c[key].as_str().map(str::to_owned);
    let size = |media: &Value, key: &str| media[key].as_u64().map(|v| v as u32);
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
            sku_id: text("sku_id"),
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
            required: c["required"].as_bool().unwrap_or(false),
            default_values: c["default_values"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|d| Some(DefaultValue { id: d["id"].as_str()?.to_owned(), kind: d["type"].as_str()?.to_owned() }))
                .collect(),
            channel_types: channel_types(&c["channel_types"]),
        }),
        // Discord drops a section with no text or no accessory.
        9 => {
            let children = parse_components(&c["components"]);
            let accessory = parse_component(&c["accessory"])?;
            if children.is_empty() {
                return None;
            }
            Component::Section { children, accessory: Some(Box::new(accessory)) }
        }
        10 => Component::Text(text("content").unwrap_or_default()),
        11 => Component::Thumbnail {
            url: c["media"]["url"].as_str()?.to_owned(),
            description: text("description"),
            spoiler: c["spoiler"].as_bool().unwrap_or(false),
            width: size(&c["media"], "width"),
            height: size(&c["media"], "height"),
        },
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
                        width: size(&i["media"], "width"),
                        height: size(&i["media"], "height"),
                    })
                })
                .collect(),
        ),
        13 => Component::File {
            url: c["file"]["url"].as_str()?.to_owned(),
            name: text("name"),
            size: c["size"].as_u64(),
            spoiler: c["spoiler"].as_bool().unwrap_or(false),
        },
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

/// INTERACTION_MODAL_CREATE: a form an app asked us to fill in, kept as the tree it came in so the
/// submit can echo it back.
#[derive(Clone, Debug, PartialEq)]
pub struct Modal {
    // The interaction id the modal answers; the submit's `data.id`.
    pub id: String,
    pub nonce: Option<String>,
    pub channel_id: String,
    pub application: App,
    pub title: String,
    pub custom_id: String,
    pub components: Vec<ModalComponent>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ModalComponent {
    // Type 1: the legacy wrapper, one text input each.
    Row(Vec<ModalComponent>),
    // Type 18: a title and description over one field.
    Label { label: String, description: Option<String>, component: Box<ModalComponent> },
    TextInput(TextInput),
    Select(Select),
    // Type 10: markdown between fields.
    Text(String),
    FileUpload(FileUpload),
    RadioGroup(ChoiceGroup),
    CheckboxGroup(ChoiceGroup),
    Checkbox { custom_id: String, default: bool },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextInput {
    pub custom_id: String,
    // The legacy label on the input itself; a type-18 label carries it instead.
    pub label: String,
    // Style 2 is the multi-line paragraph.
    pub paragraph: bool,
    pub placeholder: Option<String>,
    pub value: Option<String>,
    // Discord's client defaults it to false when the app leaves it out.
    pub required: bool,
    // In UTF-16 units, as Discord's client counts them.
    pub min: Option<u32>,
    pub max: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileUpload {
    pub custom_id: String,
    pub required: bool,
    pub min: Option<u32>,
    pub max: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChoiceGroup {
    pub custom_id: String,
    pub options: Vec<SelectOption>,
    pub required: bool,
    pub min: Option<u32>,
    pub max: Option<u32>,
}

/// What a modal field holds. `Unset` is a field never given a value, which the submit sends as null.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldValue {
    Unset,
    Text(String),
    Picks(Vec<String>),
    Checked(bool),
}

impl FieldValue {
    fn text(&self) -> Option<String> {
        match self {
            FieldValue::Text(t) => Some(t.clone()),
            _ => None,
        }
    }

    fn picks(&self) -> Option<Vec<String>> {
        match self {
            FieldValue::Picks(p) => Some(p.clone()),
            _ => None,
        }
    }
}

/// One thing a modal draws, in order: a text block, or a field with what labels it.
#[derive(Clone, Copy, Debug)]
pub enum Block<'a> {
    Text(&'a str),
    Field(Field<'a>),
}

#[derive(Clone, Copy, Debug)]
pub struct Field<'a> {
    pub label: &'a str,
    pub description: Option<&'a str>,
    pub component: &'a ModalComponent,
}

impl Field<'_> {
    pub fn required(&self) -> bool {
        match self.component {
            ModalComponent::TextInput(t) => t.required,
            ModalComponent::Select(s) => s.required,
            ModalComponent::FileUpload(f) => f.required,
            ModalComponent::RadioGroup(g) | ModalComponent::CheckboxGroup(g) => g.required,
            _ => false,
        }
    }

    /// What the field starts with: the app's value or defaults, else unset.
    /// Not verified: that Discord's client seeds its field state from these prefills.
    pub fn initial(&self) -> FieldValue {
        let defaults = |options: &[SelectOption]| options.iter().filter(|o| o.default).map(|o| o.value.clone()).collect::<Vec<_>>();
        let picks = |picks: Vec<String>| if picks.is_empty() { FieldValue::Unset } else { FieldValue::Picks(picks) };
        match self.component {
            ModalComponent::TextInput(t) => t.value.clone().map_or(FieldValue::Unset, FieldValue::Text),
            ModalComponent::Select(s) => picks(defaults(&s.options)),
            ModalComponent::RadioGroup(g) | ModalComponent::CheckboxGroup(g) => picks(defaults(&g.options)),
            ModalComponent::Checkbox { default, .. } => FieldValue::Checked(*default),
            _ => FieldValue::Unset,
        }
    }
}

impl Modal {
    /// Text blocks and fields depth-first, the order `modal_body` reads values in.
    pub fn blocks(&self) -> Vec<Block<'_>> {
        let mut out = Vec::new();
        collect_blocks(&self.components, None, &mut out);
        out
    }

    pub fn fields(&self) -> Vec<Field<'_>> {
        self.blocks()
            .into_iter()
            .filter_map(|b| match b {
                Block::Field(f) => Some(f),
                Block::Text(_) => None,
            })
            .collect()
    }
}

fn collect_blocks<'a>(components: &'a [ModalComponent], label: Option<(&'a str, Option<&'a str>)>, out: &mut Vec<Block<'a>>) {
    for c in components {
        match c {
            ModalComponent::Row(children) => collect_blocks(children, None, out),
            ModalComponent::Label { label, description, component } => collect_blocks(std::slice::from_ref(component.as_ref()), Some((label, description.as_deref())), out),
            ModalComponent::Text(text) => out.push(Block::Text(text)),
            ModalComponent::TextInput(t) => {
                let (label, description) = label.unwrap_or((&t.label, None));
                out.push(Block::Field(Field { label, description, component: c }));
            }
            _ => {
                let (label, description) = label.unwrap_or(("", None));
                out.push(Block::Field(Field { label, description, component: c }));
            }
        }
    }
}

pub fn parse_modal(d: &Value) -> Option<Modal> {
    let app = &d["application"];
    let app_id = app["id"].as_str().or(d["application_id"].as_str())?.to_owned();
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
        components: d["components"].as_array().into_iter().flatten().filter_map(parse_modal_component).collect(),
    })
}

fn parse_modal_component(c: &Value) -> Option<ModalComponent> {
    let text = |key: &str| c[key].as_str().map(str::to_owned);
    let custom_id = || text("custom_id").unwrap_or_default();
    let count = |key: &str| c[key].as_u64().map(|v| v as u32);
    let required = c["required"].as_bool().unwrap_or(false);
    // Not verified: radio and checkbox group options are believed to share a select option's shape.
    let group = || ChoiceGroup {
        custom_id: custom_id(),
        options: c["options"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|o| {
                Some(SelectOption {
                    label: o["label"].as_str()?.to_owned(),
                    value: o["value"].as_str()?.to_owned(),
                    description: o["description"].as_str().map(str::to_owned),
                    emoji: None,
                    default: o["default"].as_bool().unwrap_or(false),
                })
            })
            .collect(),
        required,
        min: count("min_values"),
        max: count("max_values"),
    };
    Some(match c["type"].as_u64()? {
        1 => ModalComponent::Row(c["components"].as_array().into_iter().flatten().filter_map(parse_modal_component).collect()),
        18 => ModalComponent::Label { label: text("label").unwrap_or_default(), description: text("description"), component: Box::new(parse_modal_component(&c["component"])?) },
        4 => ModalComponent::TextInput(TextInput {
            custom_id: custom_id(),
            label: text("label").unwrap_or_default(),
            paragraph: c["style"] == 2,
            placeholder: text("placeholder"),
            value: text("value"),
            required,
            min: count("min_length"),
            max: count("max_length"),
        }),
        3 | 5 | 6 | 7 | 8 => match parse_component(c)? {
            Component::Select(s) => ModalComponent::Select(s),
            _ => return None,
        },
        10 => ModalComponent::Text(text("content").unwrap_or_default()),
        19 => ModalComponent::FileUpload(FileUpload { custom_id: custom_id(), required, min: count("min_values"), max: count("max_values") }),
        21 => ModalComponent::RadioGroup(group()),
        22 => ModalComponent::CheckboxGroup(group()),
        23 => ModalComponent::Checkbox { custom_id: custom_id(), default: c["default"].as_bool().unwrap_or(false) },
        _ => return None,
    })
}

/// APPLICATION_COMMAND_AUTOCOMPLETE_RESPONSE's choices.
pub fn parse_choices(d: &Value) -> Vec<Choice> {
    d["choices"].as_array().into_iter().flatten().filter_map(Choice::parse).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> CommandIndex {
        parse_index(&json!({
            "applications": [{ "id": "9", "name": "Wavebot", "icon": "abc" }],
            "application_commands": [
                { "id": "1", "application_id": "9", "version": "v1", "type": 1, "name": "jouer", "name_default": "play", "description": "Play a song", "guild_id": "g9",
                  "options": [{ "type": 3, "name": "query", "description": "Song", "required": true, "autocomplete": true, "min_length": 2, "max_length": 100 },
                              { "type": 4, "name": "volume", "description": "0-100", "min_value": 0, "max_value": 100, "choices": [{ "name": "Loud", "value": 100 }] },
                              { "type": 7, "name": "room", "description": "Where", "channel_types": [2, 13] }] },
                { "id": "2", "application_id": "9", "version": "v2", "type": 1, "name": "music", "description": "Music",
                  "options": [{ "type": 2, "name": "queue", "description": "Queue", "options": [
                      { "type": 1, "name": "clear", "description": "Clear the queue", "options": [{ "type": 5, "name": "all", "description": "All" }] }] }] },
                { "id": "3", "application_id": "9", "version": "v3", "type": 3, "name": "Report", "description": "" }
            ]
        }))
    }

    fn origin() -> Origin<'static> {
        Origin { channel_id: "c", guild_id: Some("g"), session_id: "s", nonce: "n" }
    }

    #[test]
    fn index_flattens_subcommands_and_skips_context_menus() {
        let index = index();
        assert_eq!(index.apps[0].icon.as_deref(), Some("https://cdn.discordapp.com/app-icons/9/abc.png?size=64"));
        let names: Vec<String> = index.commands.iter().map(AppCommand::name).collect();
        assert_eq!(names, ["play", "music queue clear"]);
        let play = &index.commands[0];
        assert_eq!(play.display_name(), "jouer");
        assert_eq!(play.guild_id.as_deref(), Some("g9"));
        assert_eq!(play.raw["name"], "play");
        assert!(play.options[0].required && play.options[0].autocomplete);
        assert_eq!((play.options[0].min_length, play.options[0].max_length), (Some(2), Some(100)));
        assert_eq!((play.options[1].min_value, play.options[1].max_value), (Some(0.), Some(100.)));
        assert_eq!(play.options[1].choices[0].value, json!(100));
        assert_eq!(play.options[2].channel_types, [2, 13]);
        assert_eq!(index.commands[1].description, "Clear the queue");
        assert_eq!(index.commands[1].display_name(), "music queue clear");
    }

    #[test]
    fn names_prefer_name_default_and_keep_the_localized_one() {
        assert_eq!(names(&json!({ "name": "jouer", "name_default": "play" })), Some(("play".into(), Some("jouer".into()))));
        assert_eq!(names(&json!({ "name": "play", "name_localized": "jouer" })), Some(("play".into(), Some("jouer".into()))));
        assert_eq!(names(&json!({ "name": "play", "name_default": "play" })), Some(("play".into(), None)));
    }

    #[test]
    fn merge_keeps_the_user_index_on_overlap() {
        let app = |id: &str| App { id: id.into(), name: id.into(), icon: None };
        let command = |id: &str, app: &str| AppCommand { id: id.into(), application_id: app.into(), path: vec![id.into()], ..Default::default() };
        let context = CommandIndex { apps: vec![app("a"), app("b")], commands: vec![command("1", "a"), command("2", "b"), command("3", "a")] };
        let user = CommandIndex { apps: vec![app("a")], commands: vec![AppCommand { version: "user".into(), ..command("1", "a") }] };
        let merged = CommandIndex::merge(context, user);
        assert_eq!(merged.apps.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(merged.commands.iter().map(|c| (c.id.as_str(), c.version.as_str())).collect::<Vec<_>>(), [("1", "user"), ("2", ""), ("3", "")]);
    }

    #[test]
    fn command_body_matches_discords() {
        let index = index();
        let body = command_body(&index.commands[1], &[("all".into(), json!(true))], None, &origin());
        assert_eq!(body["type"], 2);
        assert_eq!(body["guild_id"], "g");
        assert_eq!(body["analytics_location"], "slash_ui");
        assert_eq!(body["data"]["name"], "music");
        assert_eq!(body["data"]["attachments"], json!([]));
        assert!(body["data"].get("guild_id").is_none());
        assert_eq!(
            body["data"]["options"],
            json!([{ "type": 2, "name": "queue", "options": [{ "type": 1, "name": "clear", "options": [{ "type": 5, "name": "all", "value": true }] }] }])
        );
        // Typed volume first, query second: sent in definition order, under the untranslated name.
        let play = command_body(&index.commands[0], &[("volume".into(), json!(5)), ("query".into(), json!("x"))], None, &origin());
        assert_eq!(play["data"]["name"], "play");
        assert_eq!(play["data"]["guild_id"], "g9");
        assert_eq!(play["data"]["options"], json!([{ "type": 3, "name": "query", "value": "x" }, { "type": 4, "name": "volume", "value": 5 }]));
        let auto = command_body(&index.commands[0], &[("query".into(), json!("midn"))], Some("query"), &origin());
        assert_eq!(auto["type"], 4);
        assert_eq!(auto["data"]["options"][0]["focused"], true);
        assert!(auto["data"].get("attachments").is_none() && auto.get("analytics_location").is_none());
    }

    #[test]
    fn attach_files_indexes_the_uploads() {
        let mut body = command_body(&index().commands[1], &[], None, &origin());
        attach_files(&mut body, &[CommandAttachment { filename: "a.png".into(), uploaded_filename: "up/a.png".into(), spoiler: true }]);
        assert_eq!(body["data"]["attachments"], json!([{ "id": "0", "filename": "a.png", "uploaded_filename": "up/a.png", "is_spoiler": true }]));
    }

    #[test]
    fn select_body_names_its_type_twice() {
        let select = parse_components(&json!([{ "type": 5, "custom_id": "who" }])).remove(0);
        let body = component_body("9", "m", 0, &select, &["u1".into()], &origin());
        assert_eq!(body["data"], json!({ "component_type": 5, "custom_id": "who", "type": 5, "values": ["u1"] }));
        let button = Component::Button(Button { style: ButtonStyle::Primary, label: None, emoji: None, custom_id: Some("b".into()), url: None, disabled: false, sku_id: None });
        assert_eq!(component_body("9", "m", 0, &button, &[], &origin())["data"], json!({ "component_type": 2, "custom_id": "b" }));
    }

    #[test]
    fn components_parse_rows_and_v2_layouts() {
        let parsed = parse_components(&json!([
            { "type": 1, "components": [
                { "type": 2, "style": 1, "label": "Vote", "custom_id": "v" },
                { "type": 2, "style": 6, "sku_id": "77" },
                { "type": 2, "style": 5, "label": "Rules", "url": "https://x.test" }] },
            { "type": 17, "accent_color": 15706673, "components": [
                { "type": 9, "components": [{ "type": 10, "content": "**Now playing**" }], "accessory": { "type": 11, "spoiler": true, "media": { "url": "https://img.test/a.png", "width": 300, "height": 200 } } },
                { "type": 9, "components": [{ "type": 10, "content": "no accessory" }] },
                { "type": 14, "spacing": 2 },
                { "type": 12, "items": [{ "media": { "url": "https://img.test/b.png", "width": 640, "height": 480 } }] },
                { "type": 13, "file": { "url": "attachment://a.txt" }, "size": 12, "spoiler": true }] },
            { "type": 6, "custom_id": "r", "required": true, "default_values": [{ "id": "1", "type": "role" }] },
            { "type": 99 }
        ]));
        assert_eq!(parsed.len(), 3);
        let Component::Row(row) = &parsed[0] else { panic!("row") };
        assert!(matches!(&row[1], Component::Button(Button { style: ButtonStyle::Premium, sku_id: Some(_), .. })));
        assert!(matches!(&row[2], Component::Button(Button { style: ButtonStyle::Link, url: Some(_), .. })));
        let Component::Container { accent, children, .. } = &parsed[1] else { panic!("container") };
        assert_eq!(*accent, Some(15706673));
        let Component::Section { accessory: Some(accessory), .. } = &children[0] else { panic!("section") };
        assert!(matches!(accessory.as_ref(), Component::Thumbnail { spoiler: true, width: Some(300), height: Some(200), .. }));
        assert_eq!(children[1], Component::Separator { divider: true, large: true });
        assert!(matches!(&children[2], Component::Gallery(items) if items[0].width == Some(640)));
        assert!(matches!(&children[3], Component::File { size: Some(12), spoiler: true, .. }));
        let Component::Select(roles) = &parsed[2] else { panic!("select") };
        assert!(roles.required);
        assert_eq!(roles.default_values, [DefaultValue { id: "1".into(), kind: "role".into() }]);
    }

    #[test]
    fn modal_echoes_its_own_tree() {
        let modal = parse_modal(&json!({
            "id": "i1", "nonce": "n", "channel_id": "c", "title": "Suggest", "custom_id": "suggest",
            "application": { "id": "9", "name": "Quizzy" },
            "components": [
                { "type": 1, "components": [{ "type": 4, "custom_id": "q", "label": "Question", "style": 1, "max_length": 200, "required": true }] },
                { "type": 10, "content": "Pick carefully" },
                { "type": 18, "label": "Why", "description": "Optional", "component": { "type": 4, "custom_id": "why", "style": 2 } },
                { "type": 18, "label": "Kind", "component": { "type": 3, "custom_id": "kind", "options": [{ "label": "A", "value": "a", "default": true }] } },
                { "type": 18, "label": "Agree", "component": { "type": 23, "custom_id": "ok" } }
            ]
        }))
        .unwrap();
        let fields = modal.fields();
        assert_eq!(fields.iter().map(|f| f.label).collect::<Vec<_>>(), ["Question", "Why", "Kind", "Agree"]);
        assert_eq!(fields[1].description, Some("Optional"));
        assert!(fields[0].required() && !fields[1].required());
        assert_eq!(fields[2].initial(), FieldValue::Picks(vec!["a".into()]));
        assert!(matches!(modal.blocks()[1], Block::Text("Pick carefully")));
        let body = modal_body(&modal, &[FieldValue::Text("Doom or Myst?".into()), FieldValue::Unset, FieldValue::Picks(vec!["a".into()]), FieldValue::Checked(true)], &origin());
        assert_eq!(body["type"], 5);
        assert_eq!(body["application_id"], "9");
        assert_eq!(
            body["data"]["components"],
            json!([
                { "type": 1, "components": [{ "type": 4, "custom_id": "q", "value": "Doom or Myst?" }] },
                { "type": 10 },
                { "type": 18, "component": { "type": 4, "custom_id": "why", "value": null } },
                { "type": 18, "component": { "type": 3, "custom_id": "kind", "values": ["a"] } },
                { "type": 18, "component": { "type": 23, "custom_id": "ok", "value": true } }
            ])
        );
    }

    #[test]
    fn interaction_meta_names_the_command() {
        let meta = parse_interaction(&json!({ "interaction_metadata": { "type": 2, "name": "play", "user": { "id": "u", "username": "mira" }, "ephemerality_reason": 3 } })).unwrap();
        assert_eq!(meta.command.as_deref(), Some("play"));
        assert_eq!(meta.ephemerality_reason, Some(3));
        let legacy = parse_interaction(&json!({ "interaction_metadata": { "type": 2, "name": "music", "user": { "id": "u", "username": "mira" } }, "interaction": { "name": "music queue" } })).unwrap();
        assert_eq!(legacy.command.as_deref(), Some("music queue"));
        let button = parse_interaction(&json!({ "interaction_metadata": { "type": 3, "user": { "id": "u", "username": "mira" } } })).unwrap();
        assert_eq!(button.command, None);
    }
}
