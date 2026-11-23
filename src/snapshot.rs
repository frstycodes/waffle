// The launch snapshot: the shell as you left it (you, your settings, your servers and folders, the last
// server's channels, your DMs, and where you were), so the next launch draws at once and is browsable before
// the gateway connects.
//
// Only what was true when it was saved and claims nothing about now. Its types are its own, with fields for
// none of the live state (presence, activities, custom status, voice, typing, calls, unread and mention
// counts, last message ids): what isn't a field can't be saved or read back. Everything live waits for READY.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::discord::{AccountSettings, Channel, ChannelKind, ChannelProps, Folder, Guild, MyAccount, NameStyle, Nameplate, Overwrite, Role, User};

// Bumped whenever the format changes: an older or newer file is a miss, never misread.
pub const VERSION: u32 = 1;

// Most DMs a launch needs: the list's first screens.
const MAX_DMS: usize = 40;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub account: String,
    pub me: Person,
    pub my_account: Account,
    pub settings: Settings,
    // Rail order; only the last server carries its channels.
    pub servers: Vec<Server>,
    pub folders: Vec<Fold>,
    pub dms: Vec<Dm>,
    pub place: Option<Place>,
}

// Someone as their name and face show them.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Person {
    pub id: String,
    pub name: String,
    pub username: String,
    pub avatar: String,
    pub bot: bool,
    pub decoration: Option<String>,
    pub nameplate: Option<Nameplate>,
    pub name_style: Option<NameStyle>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub username: String,
    pub global_name: Option<String>,
    pub banner: Option<String>,
    pub premium_type: u8,
}

// The synced settings that change how things draw.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub developer_mode: bool,
    pub render_embeds: bool,
    pub render_reactions: bool,
    pub message_display_compact: bool,
    pub inline_attachment_media: bool,
    pub inline_embed_media: bool,
    pub gif_auto_play: bool,
    pub animate_emoji: bool,
    pub animate_stickers: u8,
    pub convert_emoticons: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub owner_id: Option<String>,
    // Ours, with the server's roles, so the channel list shows what we may see.
    pub my_roles: Vec<String>,
    pub roles: Vec<ServerRole>,
    pub features: Vec<String>,
    pub channels: Vec<Chan>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ServerRole {
    pub id: String,
    pub name: String,
    pub color: u32,
    pub position: i64,
    pub permissions: u64,
    pub managed: bool,
    pub icon: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chan {
    pub id: String,
    pub name: String,
    pub kind: ChannelKind,
    pub raw_type: u8,
    pub parent_id: Option<String>,
    pub position: i64,
    pub topic: Option<String>,
    pub private: bool,
    pub overwrites: Vec<Overwrite>,
    pub props: ChannelProps,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dm {
    pub id: String,
    pub name: String,
    pub kind: ChannelKind,
    pub raw_type: u8,
    pub recipient: Option<Person>,
    pub recipients: Vec<Person>,
    pub icon: Option<String>,
    pub owner_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Fold {
    pub id: String,
    pub name: Option<String>,
    pub color: Option<u32>,
    pub guild_ids: Vec<String>,
}

// Where you were: a server's channel or a DM (`guild_id` None), and how far up you had scrolled.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub guild_id: Option<String>,
    pub channel_id: String,
    // The message at the top of the feed and how far into it, while scrolled up from the newest.
    pub scroll: Option<(String, f32)>,
}

// What the app hands over to be saved.
pub struct Shell<'a> {
    pub me: &'a User,
    pub my_account: &'a MyAccount,
    pub settings: &'a AccountSettings,
    pub guilds: &'a [Guild],
    pub folders: &'a [Folder],
    pub dms: &'a [Channel],
    pub place: Option<Place>,
}

impl Snapshot {
    pub fn capture(shell: Shell) -> Self {
        let open_guild = shell.place.as_ref().and_then(|p| p.guild_id.clone());
        let s = shell.settings;
        Self {
            version: VERSION,
            account: shell.me.id.clone(),
            me: Person::from(shell.me),
            my_account: Account { username: shell.my_account.username.clone(), global_name: shell.my_account.global_name.clone(), banner: shell.my_account.banner.clone(), premium_type: shell.my_account.premium_type },
            settings: Settings {
                developer_mode: s.developer_mode,
                render_embeds: s.render_embeds,
                render_reactions: s.render_reactions,
                message_display_compact: s.message_display_compact,
                inline_attachment_media: s.inline_attachment_media,
                inline_embed_media: s.inline_embed_media,
                gif_auto_play: s.gif_auto_play,
                animate_emoji: s.animate_emoji,
                animate_stickers: s.animate_stickers,
                convert_emoticons: s.convert_emoticons,
            },
            servers: shell.guilds.iter().map(|g| Server::from_guild(g, open_guild.as_deref() == Some(g.id.as_str()))).collect(),
            folders: shell.folders.iter().map(|f| Fold { id: f.id.clone(), name: f.name.clone(), color: f.color, guild_ids: f.guild_ids.clone() }).collect(),
            dms: shell.dms.iter().take(MAX_DMS).map(Dm::from).collect(),
            place: shell.place,
        }
    }

    pub fn my_account(&self) -> MyAccount {
        let a = &self.my_account;
        MyAccount { username: a.username.clone(), global_name: a.global_name.clone(), banner: a.banner.clone(), premium: a.premium_type > 0, premium_type: a.premium_type }
    }

    pub fn account_settings(&self) -> AccountSettings {
        let s = &self.settings;
        AccountSettings {
            developer_mode: s.developer_mode,
            render_embeds: s.render_embeds,
            render_reactions: s.render_reactions,
            message_display_compact: s.message_display_compact,
            inline_attachment_media: s.inline_attachment_media,
            inline_embed_media: s.inline_embed_media,
            gif_auto_play: s.gif_auto_play,
            animate_emoji: s.animate_emoji,
            animate_stickers: s.animate_stickers,
            convert_emoticons: s.convert_emoticons,
            ..AccountSettings::default()
        }
    }

    pub fn guilds(&self) -> Vec<Guild> {
        self.servers.iter().map(Server::guild).collect()
    }

    pub fn folders(&self) -> Vec<Folder> {
        self.folders.iter().map(|f| Folder { id: f.id.clone(), name: f.name.clone(), color: f.color, guild_ids: f.guild_ids.clone() }).collect()
    }

    pub fn dms(&self) -> Vec<Channel> {
        self.dms.iter().map(Dm::channel).collect()
    }

    pub fn to_bytes(&self) -> Option<Vec<u8>> {
        serde_json::to_vec(self).ok()
    }

    // A file of another version, another account's, or one that doesn't parse is a miss.
    pub fn from_bytes(bytes: &[u8], account: &str) -> Option<Self> {
        #[derive(Deserialize)]
        struct Version {
            version: u32,
        }
        (serde_json::from_slice::<Version>(bytes).ok()?.version == VERSION).then_some(())?;
        let snapshot: Self = serde_json::from_slice(bytes).ok()?;
        (snapshot.account == account).then_some(snapshot)
    }
}

pub fn path(account: &str) -> Option<PathBuf> {
    Some(crate::store::account_dir(account)?.join("snapshot.json"))
}

// Off the UI thread only.
pub fn read(file: &Path, account: &str) -> Option<Snapshot> {
    Snapshot::from_bytes(&std::fs::read(file).ok()?, account)
}

impl From<&User> for Person {
    fn from(u: &User) -> Self {
        Self {
            id: u.id.clone(),
            name: u.name.clone(),
            username: u.username.clone(),
            avatar: u.avatar.clone(),
            bot: u.bot,
            decoration: u.decoration.clone(),
            nameplate: u.nameplate.as_deref().cloned(),
            name_style: u.name_style.clone(),
        }
    }
}

impl Person {
    pub fn user(&self) -> User {
        User {
            id: self.id.clone(),
            name: self.name.clone(),
            username: self.username.clone(),
            avatar: self.avatar.clone(),
            bot: self.bot,
            decoration: self.decoration.clone(),
            nameplate: self.nameplate.clone().map(Box::new),
            name_style: self.name_style.clone(),
            ..User::default()
        }
    }
}

impl Server {
    fn from_guild(g: &Guild, with_channels: bool) -> Self {
        let role = |r: &Role| ServerRole { id: r.id.clone(), name: r.name.clone(), color: r.color, position: r.position, permissions: r.permissions, managed: r.managed, icon: r.icon.clone() };
        Self {
            id: g.id.clone(),
            name: g.name.clone(),
            icon: g.icon.clone(),
            owner_id: g.owner_id.clone(),
            my_roles: g.my_roles.clone(),
            roles: if with_channels { g.roles.iter().map(role).collect() } else { Vec::new() },
            features: g.features.clone(),
            channels: if with_channels { g.channels.iter().map(Chan::from).collect() } else { Vec::new() },
        }
    }

    fn guild(&self) -> Guild {
        let role = |r: &ServerRole| Role { id: r.id.clone(), name: r.name.clone(), color: r.color, position: r.position, permissions: r.permissions, managed: r.managed, icon: r.icon.clone() };
        Guild {
            id: self.id.clone(),
            name: self.name.clone(),
            icon: self.icon.clone(),
            owner_id: self.owner_id.clone(),
            my_roles: self.my_roles.clone(),
            roles: self.roles.iter().map(role).collect(),
            features: self.features.clone(),
            channels: self.channels.iter().map(Chan::channel).collect(),
            ..Guild::default()
        }
    }
}

impl From<&Channel> for Chan {
    fn from(c: &Channel) -> Self {
        Self {
            id: c.id.clone(),
            name: c.name.clone(),
            kind: c.kind,
            raw_type: c.raw_type,
            parent_id: c.parent_id.clone(),
            position: c.position,
            topic: c.topic.clone(),
            private: c.private,
            overwrites: c.overwrites.clone(),
            props: (*c.props).clone(),
        }
    }
}

impl Chan {
    fn channel(&self) -> Channel {
        Channel {
            id: self.id.clone(),
            name: self.name.clone(),
            kind: self.kind,
            parent_id: self.parent_id.clone(),
            position: self.position,
            topic: self.topic.clone(),
            last_message_id: None,
            private: self.private,
            recipient: None,
            recipients: Vec::new(),
            icon: None,
            thread: None,
            raw_type: self.raw_type,
            overwrites: self.overwrites.clone(),
            props: Box::new(self.props.clone()),
        }
    }
}

impl From<&Channel> for Dm {
    fn from(c: &Channel) -> Self {
        Self {
            id: c.id.clone(),
            name: c.name.clone(),
            kind: c.kind,
            raw_type: c.raw_type,
            recipient: c.recipient.as_ref().map(Person::from),
            recipients: c.recipients.iter().map(Person::from).collect(),
            icon: c.icon.clone(),
            owner_id: c.props.owner_id.clone(),
        }
    }
}

impl Dm {
    fn channel(&self) -> Channel {
        Channel {
            id: self.id.clone(),
            name: self.name.clone(),
            kind: self.kind,
            parent_id: None,
            position: 0,
            topic: None,
            last_message_id: None,
            private: false,
            recipient: self.recipient.as_ref().map(Person::user),
            recipients: self.recipients.iter().map(Person::user).collect(),
            icon: self.icon.clone(),
            thread: None,
            raw_type: self.raw_type,
            overwrites: Vec::new(),
            props: Box::new(ChannelProps { owner_id: self.owner_id.clone(), ..ChannelProps::default() }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(id: &str) -> Channel {
        Chan { id: id.into(), name: "general".into(), kind: ChannelKind::Text, raw_type: 0, parent_id: None, position: 0, topic: None, private: false, overwrites: Vec::new(), props: ChannelProps::default() }.channel()
    }

    fn sample() -> Snapshot {
        let me = User { id: "1".into(), name: "me".into(), avatar: "a".into(), ..Default::default() };
        let channel = Channel { last_message_id: Some("99".into()), ..text("10") };
        let guild = Guild { id: "5".into(), name: "Café".into(), channels: vec![channel], roles: vec![Role { id: "5".into(), name: "@everyone".into(), color: 0, position: 0, permissions: 1024, managed: false, icon: None }], ..Default::default() };
        let dm = Channel { recipient: Some(User { id: "2".into(), name: "friend".into(), ..Default::default() }), last_message_id: Some("98".into()), ..text("20") };
        Snapshot::capture(Shell {
            me: &me,
            my_account: &MyAccount::default(),
            settings: &AccountSettings::default(),
            guilds: &[guild],
            folders: &[Folder { id: "f".into(), name: None, color: None, guild_ids: vec!["5".into()] }],
            dms: &[dm],
            place: Some(Place { guild_id: Some("5".into()), channel_id: "10".into(), scroll: Some(("90".into(), 12.)) }),
        })
    }

    fn keys(value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    out.push(k.clone());
                    keys(v, out);
                }
            }
            serde_json::Value::Array(items) => items.iter().for_each(|v| keys(v, out)),
            _ => {}
        }
    }

    #[test]
    fn carries_no_live_fields() {
        let json = serde_json::to_value(sample()).unwrap();
        let mut found = Vec::new();
        keys(&json, &mut found);
        let live = ["status", "presence", "presences", "activity", "activities", "custom_status", "custom", "rich", "streaming", "voice_states", "voice", "speaking", "typing", "unread", "mentions", "mention_count", "read_state", "read_states", "last_message_id", "last_read", "stage_instances", "events", "call", "calls", "mobile_only"];
        for key in &found {
            assert!(!live.contains(&key.as_str()), "the snapshot has a live field: {key}");
        }
        let back = Snapshot::from_bytes(&sample().to_bytes().unwrap(), "1").unwrap();
        assert!(back.guilds()[0].voice_states.is_empty());
        assert!(back.guilds()[0].channels.iter().all(|c| c.last_message_id.is_none()), "no unread can be worked out from it");
        assert!(back.dms().iter().all(|c| c.last_message_id.is_none()));
    }

    #[test]
    fn round_trips() {
        let s = sample();
        let back = Snapshot::from_bytes(&s.to_bytes().unwrap(), "1").unwrap();
        assert_eq!(back.to_bytes(), s.to_bytes());
        assert_eq!(back.guilds()[0].channels[0].name, "general");
        assert_eq!(back.place.unwrap().scroll, Some(("90".into(), 12.)));
    }

    #[test]
    fn corrupt_old_or_foreign_files_are_misses() {
        assert!(Snapshot::from_bytes(b"{not json", "1").is_none());
        assert!(Snapshot::from_bytes(b"", "1").is_none());
        let mut old = sample();
        old.version = VERSION + 1;
        assert!(Snapshot::from_bytes(&old.to_bytes().unwrap(), "1").is_none());
        assert!(Snapshot::from_bytes(&sample().to_bytes().unwrap(), "2").is_none(), "another account's");
        let bytes = sample().to_bytes().unwrap();
        assert!(Snapshot::from_bytes(&bytes[..bytes.len() / 2], "1").is_none(), "cut short");
        let dir = crate::store::temp_root("snapshot");
        assert!(read(&dir.join("snapshot.json"), "1").is_none(), "missing");
    }
}
