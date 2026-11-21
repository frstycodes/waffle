// Every setting, once: where it lives (page, then the group it sits in), what it's called, the words people
// search it by, and what draws it. New pages render straight from here; older pages draw their own rows
// (`Control::Custom`) and are listed so search can reach them. Each settings window is a scope with its own
// page enum and table (`SETTINGS` is the user's); `search` and `groups` work on any of them.
use super::Page;

// A settings window's pages: user settings here, channel and server settings in their own modules.
pub trait Scope: Copy + PartialEq + 'static {
    fn label(self) -> &'static str;
}

impl Scope for Page {
    fn label(self) -> &'static str {
        Page::label(self)
    }
}

pub struct Setting<P: Scope = Page> {
    pub id: &'static str,
    pub page: P,
    // The group title, which is also the page section a jump scrolls to.
    pub group: &'static str,
    pub title: &'static str,
    pub caption: Option<&'static str>,
    // Synonyms and the words people type for it ("dark" for Theme, "mic" for Input device).
    pub keywords: &'static [&'static str],
    pub control: Control,
    // The element id of the row to highlight after a jump, when it differs from `id`.
    pub row: Option<&'static str>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Control {
    // Drawn by its page's own code.
    Custom,
    // Managed on discord.com: the row opens it there.
    Link(&'static str),
    Switch(Toggle),
    Choice(Choice),
}

// An on/off setting: local ones live in settings.json, the rest on the Discord account.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Toggle {
    DesktopNotifications,
    DockBadge,
    Clock24,
    AutoDownloadUpdates,
    RenderEmbeds,
    RenderReactions,
    TtsCommand,
    DeveloperMode,
    ShareActivity,
    DmsFromServerMembers,
    FriendsAll,
    FriendsMutualFriends,
    FriendsMutualGuilds,
}

// One of several, drawn as radio rows filling its group.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    DmFilter,
}

impl Choice {
    // (stored value, title, detail), in the order shown.
    pub fn options(self) -> &'static [(u8, &'static str, Option<&'static str>)] {
        match self {
            Choice::DmFilter => &[
                (2, "Filter all direct messages", Some("Every DM is scanned for explicit images")),
                (1, "Filter DMs from non-friends", Some("Messages from friends aren't scanned")),
                (0, "Don't filter direct messages", None),
            ],
        }
    }
}

const DISCORD: &str = "https://discord.com/app";

const fn s(id: &'static str, page: Page, group: &'static str, title: &'static str, caption: Option<&'static str>, keywords: &'static [&'static str], control: Control) -> Setting {
    Setting { id, page, group, title, caption, keywords, control, row: None }
}

const fn on_row(mut setting: Setting, row: &'static str) -> Setting {
    setting.row = Some(row);
    setting
}

use Control::{Custom, Link, Switch};
use Page::*;

pub static SETTINGS: &[Setting] = &[
    // My account
    on_row(s("display-name", Account, "Account info", "Display name", None, &["name", "nickname", "rename"], Custom), "acct-name"),
    on_row(s("username", Account, "Account info", "Username", Some("Your unique @name; Discord asks for your password to change it"), &["handle", "@", "tag", "rename"], Custom), "acct-username"),
    on_row(s("avatar", Account, "Account info", "Avatar", None, &["profile picture", "pfp", "photo", "image"], Custom), "change-avatar"),
    on_row(s("banner", Account, "Account info", "Banner", Some("Needs Nitro; without it, Discord sets it"), &["cover", "header image", "background"], Custom), "change-banner"),
    on_row(s("user-id", Account, "Account info", "User ID", Some("Copy it to share or report"), &["id", "copy id", "snowflake"], Custom), "acct-id"),
    s("profile", Account, "On discord.com", "Bio, pronouns and profile effects", Some("Decorations, effects, nameplates and name styles"), &["bio", "about me", "pronouns", "decoration", "effect", "nameplate", "nitro"], Link(DISCORD)),
    s("password", Account, "On discord.com", "Password", None, &["change password", "security", "login"], Link(DISCORD)),
    s("two-factor", Account, "On discord.com", "Two-factor authentication", Some("Authenticator app, security keys and backup codes"), &["2fa", "mfa", "authenticator", "security key", "passkey", "backup codes"], Link(DISCORD)),
    s("email-phone", Account, "On discord.com", "Email and phone", None, &["email", "phone", "contact", "verify"], Link(DISCORD)),
    s("devices", Account, "On discord.com", "Devices", Some("Where you're signed in, and logging out of them"), &["sessions", "log out everywhere", "signed in"], Link(DISCORD)),
    s("authorized-apps", Account, "On discord.com", "Authorized apps", None, &["oauth", "apps", "integrations", "permissions"], Link(DISCORD)),
    s("connections", Account, "On discord.com", "Connections", Some("Linked accounts such as Spotify, Steam and GitHub"), &["spotify", "steam", "xbox", "playstation", "github", "twitch", "youtube", "linked accounts"], Link(DISCORD)),
    s("family-center", Account, "On discord.com", "Family Center", None, &["parent", "teen", "guardian", "family"], Link(DISCORD)),
    s("billing", Account, "On discord.com", "Nitro and billing", Some("Subscriptions, boosts, gifts and payment methods"), &["nitro", "subscription", "payment", "gift", "boost", "billing", "card"], Link(DISCORD)),
    s("data", Account, "On discord.com", "Request your data", Some("A copy of what Discord stores about you"), &["privacy", "export", "download data", "gdpr", "data and privacy"], Link(DISCORD)),
    s("delete-account", Account, "On discord.com", "Disable or delete account", None, &["delete", "remove account", "deactivate", "close account"], Link(DISCORD)),
    // Content & social
    s("dm-server-members", ContentSocial, "Direct messages", "Allow DMs from server members", Some("Applies to servers you join from now on"), &["dm", "direct messages", "privacy", "strangers", "message requests"], Switch(Toggle::DmsFromServerMembers)),
    s("dm-filter", ContentSocial, "Message filtering", "Filter direct messages", Some("Discord blurs explicit images in DMs it scans"), &["spam", "explicit", "nsfw", "safe", "sensitive media", "filter"], Control::Choice(Choice::DmFilter)),
    s("friends-all", ContentSocial, "Who can send you a friend request", "Everyone", None, &["friend request", "who can add me", "add friend"], Switch(Toggle::FriendsAll)),
    s("friends-mutual-friends", ContentSocial, "Who can send you a friend request", "Friends of friends", None, &["friend request", "mutual friends"], Switch(Toggle::FriendsMutualFriends)),
    s("friends-mutual-guilds", ContentSocial, "Who can send you a friend request", "Server members", None, &["friend request", "mutual servers"], Switch(Toggle::FriendsMutualGuilds)),
    // Appearance
    s("theme", Appearance, "Theme", "Theme", Some("Light, dark or midnight"), &["dark", "light", "midnight", "mode", "colour", "color", "night"], Custom),
    s("native-sidebar", Appearance, "Sidebar", "Sidebar material", Some("Liquid Glass behind the sidebar"), &["liquid glass", "glass", "translucent", "transparent", "vibrancy", "blur", "wallpaper"], Custom),
    s("ui-font", Appearance, "Text", "Interface font", Some("Menus, messages and everything else"), &["font", "typeface", "text", "figtree", "type"], Custom),
    s("mono-font", Appearance, "Text", "Code font", Some("Code blocks and inline code"), &["font", "monospace", "mono", "code", "roboto mono", "typeface"], Custom),
    s("reduce-motion", Appearance, "Motion", "Reduce motion", Some("Fades instead of springs and slides"), &["animation", "motion", "accessibility", "springs"], Custom),
    s("app-icon", Appearance, "App icon", "App icon", Some("How Waffle looks in your Dock"), &["dock icon", "logo", "icon"], Custom),
    // Voice and video
    s("input-device", VoiceVideo, "Voice", "Input device", None, &["mic", "microphone", "input"], Custom),
    s("output-device", VoiceVideo, "Voice", "Output device", None, &["speaker", "headphones", "audio", "output"], Custom),
    s("input-volume", VoiceVideo, "Voice", "Input volume", None, &["mic volume", "gain", "loudness"], Custom),
    s("output-volume", VoiceVideo, "Voice", "Output volume", None, &["speaker volume", "volume", "loudness"], Custom),
    s("mic-test", VoiceVideo, "Voice", "Mic test", Some("Hear yourself back"), &["test", "loopback", "hear myself"], Custom),
    s("input-profile", VoiceVideo, "Input profile", "Input profile", Some("Voice isolation, studio or custom"), &["voice isolation", "studio", "custom", "preset"], Custom),
    s("input-mode", VoiceVideo, "Input mode", "Input mode", Some("Voice activity or push to talk"), &["push to talk", "ptt", "voice activity", "vad", "open mic"], Custom),
    on_row(s("input-sensitivity", VoiceVideo, "Input mode", "Input sensitivity", None, &["threshold", "gate", "sensitivity"], Custom), "input-sensitivity"),
    on_row(s("noise-suppression", VoiceVideo, "Voice processing", "Noise suppression", None, &["krisp", "background noise", "deepfilter", "noise"], Custom), "noise"),
    on_row(s("echo-cancellation", VoiceVideo, "Voice processing", "Echo cancellation", None, &["echo", "feedback"], Custom), "row-set-echo"),
    s("camera", VoiceVideo, "Camera", "Camera", None, &["webcam", "video", "preview"], Custom),
    s("voice-sounds", VoiceVideo, "Sounds", "Voice sounds", Some("Join, leave, mute and deafen cues"), &["join sound", "leave sound", "cues", "sound effects"], Custom),
    on_row(s("reset-voice", VoiceVideo, "Advanced", "Reset voice and video settings", None, &["defaults", "reset"], Custom), "reset"),
    // Chat
    s("render-embeds", Chat, "Display", "Show embeds and link previews", None, &["links", "preview", "unfurl", "embeds"], Switch(Toggle::RenderEmbeds)),
    s("render-reactions", Chat, "Display", "Show emoji reactions", None, &["reactions", "emoji"], Switch(Toggle::RenderReactions)),
    s("tts-command", Chat, "Display", "Allow playback and usage of /tts command", Some("Using the /tts command speaks your message out loud. Turn this off if you prefer to see /tts messages as text only."), &["tts", "text to speech", "speak", "read aloud", "voice"], Switch(Toggle::TtsCommand)),
    s("clock-24", Chat, "Display", "Use a 24-hour clock", Some("For message times"), &["time", "timestamp", "am pm", "clock", "24h"], Switch(Toggle::Clock24)),
    // Notifications
    s("desktop-notifications", Notifications, "Desktop", "Desktop notifications", Some("For DMs, mentions and what your server settings ask for"), &["alerts", "banner", "popup", "toast", "notify"], Switch(Toggle::DesktopNotifications)),
    s("dock-badge", Notifications, "Desktop", "Unread badge on the Dock icon", Some("Counts unread mentions and DMs"), &["badge", "count", "dock", "mentions", "unread"], Switch(Toggle::DockBadge)),
    s("message-sounds", Notifications, "Sounds", "Message and mention sounds", None, &["sound", "ding", "ping", "audio"], Custom),
    // Keybinds
    s("keybinds", Keybinds, "Your keybinds", "Your keybinds", Some("Shortcuts that work while another app is in front"), &["shortcut", "hotkey", "global", "push to talk key"], Custom),
    s("default-keybinds", Keybinds, "Default keybinds", "Default keybinds", None, &["shortcut", "hotkey", "keyboard"], Custom),
    // Advanced
    s("developer-mode", Advanced, "Developer", "Developer mode", Some("Adds Copy ID to the menus of servers, channels, people and messages"), &["copy id", "developer", "debug", "ids"], Switch(Toggle::DeveloperMode)),
    // Listed while Developer mode is on.
    s("crash-logs", Advanced, "Crash logs", "Crash logs", Some("Save a crash report to attach to a bug"), &["crash", "log", "report", "bug", "debug", "panic"], Custom),
    // Updates
    // One row holds the version, the status and its action (check, download, or restart to update).
    on_row(s("check-for-updates", Updates, "Waffle", "Check for updates", Some("The version you're on, and Restart to update once one is downloaded"), &["update", "version", "upgrade", "release", "about", "latest", "restart to update", "install", "restart"], Custom), "update-status"),
    s("whats-new", Updates, "Waffle", "What's new", Some("Release notes on GitHub"), &["release notes", "changelog", "release", "version", "update"], Custom),
    s("last-checked", Updates, "Waffle", "Last checked", None, &["update", "check", "version"], Custom),
    s("auto-download-updates", Updates, "Automatic updates", "Download updates automatically", Some("Off, Waffle still checks and tells you, then downloads when you ask"), &["update", "upgrade", "auto update", "background", "download", "release"], Switch(Toggle::AutoDownloadUpdates)),
    // Activity privacy
    s("share-activity", ActivityPrivacy, "Activity status", "Share your activity status", Some("What you're playing or listening to, as Discord's apps detect it"), &["game", "playing", "spotify", "rich presence", "status", "activity"], Switch(Toggle::ShareActivity)),
];

impl<P: Scope> Setting<P> {
    pub fn row_id(&self) -> &'static str {
        self.row.unwrap_or(self.id)
    }
}

// The groups a page shows, in registry order.
pub fn groups<P: Scope>(table: &'static [Setting<P>], page: P) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for setting in table.iter().filter(|s| s.page == page) {
        if !out.contains(&setting.group) {
            out.push(setting.group);
        }
    }
    out
}

// Settings matching every word of `query`, best first: the exact title, then an exact synonym ("mic" is the
// input device, not Mic test), then a title starting with it; per word, a title word beats a synonym, which
// beats the group or page name, which beats the caption. Ties keep registry order (page order, then on the page).
pub fn search<P: Scope>(table: &'static [Setting<P>], query: &str) -> Vec<&'static Setting<P>> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let phrase = words.join(" ");
    let mut hits: Vec<(u32, usize, &'static Setting<P>)> = table
        .iter()
        .enumerate()
        .filter_map(|(i, setting)| {
            let title = setting.title.to_lowercase();
            let mut total = match () {
                _ if title == phrase => 400,
                _ if setting.keywords.iter().any(|k| *k == phrase) => 300,
                _ if title.starts_with(&phrase) => 250,
                _ => 0,
            };
            for word in &words {
                total += word_score(setting, word)?;
            }
            Some((total, i, setting))
        })
        .collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    hits.into_iter().map(|(_, _, s)| s).collect()
}

fn word_score<P: Scope>(setting: &Setting<P>, word: &str) -> Option<u32> {
    let starts = |text: &str| text.to_lowercase().split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(word));
    if starts(setting.title) {
        return Some(60);
    }
    if setting.keywords.iter().any(|k| starts(k)) {
        return Some(45);
    }
    if starts(setting.group) || starts(Scope::label(setting.page)) {
        return Some(25);
    }
    setting.caption.filter(|c| c.to_lowercase().contains(word)).map(|_| 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Page; 10] = [Account, ContentSocial, Appearance, VoiceVideo, Chat, Notifications, Keybinds, Advanced, Updates, ActivityPrivacy];

    #[test]
    fn ids_and_rows_are_unique() {
        let mut ids: Vec<&str> = SETTINGS.iter().map(|s| s.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), SETTINGS.len(), "duplicate setting id");
        let mut rows: Vec<&str> = SETTINGS.iter().map(|s| s.row_id()).collect();
        rows.sort_unstable();
        rows.dedup();
        assert_eq!(rows.len(), SETTINGS.len(), "two settings highlight the same row");
    }

    #[test]
    fn update_words_find_the_updates_page() {
        for word in ["update", "version", "upgrade", "release", "restart to update"] {
            assert_eq!(search(SETTINGS, word).first().map(|s| s.page), Some(Updates), "{word}");
        }
        assert_eq!(search(SETTINGS, "auto update").first().map(|s| s.id), Some("auto-download-updates"));
    }

    #[test]
    fn every_page_has_settings() {
        for page in ALL {
            assert!(!groups(SETTINGS, page).is_empty(), "{page:?} lists nothing");
        }
    }

    #[test]
    fn a_page_keeps_each_group_together() {
        for page in ALL {
            let order: Vec<&str> = SETTINGS.iter().filter(|s| s.page == page).map(|s| s.group).collect();
            let mut seen: Vec<&str> = Vec::new();
            for (i, group) in order.iter().enumerate() {
                assert!(i == 0 || order[i - 1] == *group || !seen.contains(group), "{page:?}: {group} is split");
                seen.push(group);
            }
        }
    }

    #[test]
    fn synonyms_find_their_setting_first() {
        let first = |q: &str| search(SETTINGS, q).first().map(|s| s.id);
        assert_eq!(first("dark"), Some("theme"));
        assert_eq!(first("mic"), Some("input-device"));
        assert_eq!(first("ptt"), Some("input-mode"));
        assert_eq!(first("2fa"), Some("two-factor"));
        let copy_id: Vec<&str> = search(SETTINGS, "copy id").iter().take(2).map(|s| s.id).collect();
        assert!(copy_id.contains(&"user-id") && copy_id.contains(&"developer-mode"), "{copy_id:?}");
        assert_eq!(first("spotify").map(|id| id == "connections" || id == "share-activity"), Some(true));
    }

    #[test]
    fn a_title_match_beats_a_caption_match() {
        let hits: Vec<&str> = search(SETTINGS, "noise").iter().map(|s| s.id).collect();
        assert_eq!(hits.first(), Some(&"noise-suppression"));
        assert!(hits.contains(&"input-profile") || hits.len() == 1);
    }

    #[test]
    fn every_word_must_match() {
        assert!(search(SETTINGS, "friend request").iter().all(|s| s.group.starts_with("Who can")));
        assert!(search(SETTINGS, "theme xyzzy").is_empty());
        assert!(search(SETTINGS, "   ").is_empty());
    }

    #[test]
    fn exact_title_ranks_above_prefix() {
        let hits: Vec<&str> = search(SETTINGS, "camera").iter().map(|s| s.id).collect();
        assert_eq!(hits.first(), Some(&"camera"));
    }
}
