// Desktop notifications and the Dock badge, following Discord's rules: nothing on Do Not Disturb or
// for the channel you're looking at; DMs notify unless muted; in servers the channel's level wins
// over the server's, muted channels and servers notify only on mentions, and suppressed
// @everyone / role mentions don't count.
use crate::{
    Waffle,
    discord::{Message, Relation, Status},
};

#[derive(Debug, PartialEq, Eq)]
pub enum Notify {
    None,
    Message,
    Mention,
}

// How a message's place treats it, with mutes and levels resolved thread → channel → category → server.
struct Rules {
    dm: bool,
    muted: bool,
    mentioned: bool,
    level: u8,
}

// `level`: 0 all messages, 1 only mentions, 2 nothing (already resolved from channel/category/server).
fn decide(dm: bool, level: u8, mentioned: bool, muted: bool) -> Notify {
    if dm {
        return if muted { Notify::None } else { Notify::Message };
    }
    match (level, mentioned, muted) {
        (2, _, _) => Notify::None,
        (_, true, _) => Notify::Mention,
        (0, false, false) => Notify::Message,
        _ => Notify::None,
    }
}

impl Waffle {
    pub(crate) fn notify_kind(&self, message: &Message, focused_here: bool) -> Notify {
        let Some(me) = &self.me else { return Notify::None };
        // @silent: no sound and no desktop notification; it still counts as a mention.
        if message.author.id == me.id || focused_here || self.my_status == Status::Dnd || message.silent() {
            return Notify::None;
        }
        let rules = self.rules_for(message);
        decide(rules.dm, rules.level, rules.mentioned, rules.muted)
    }

    /// Whether a message adds to its channel's mention count, as Discord's read state counts it: it mentions
    /// you past the server's suppress rules, or it's in an unmuted DM. Yours and blocked users' never do.
    pub(crate) fn counts_as_mention(&self, message: &Message) -> bool {
        let blocked = self.relationships.iter().any(|r| r.kind == Relation::Blocked && r.user.id == message.author.id);
        if message.author.id == self.my_id() || blocked {
            return false;
        }
        let rules = self.rules_for(message);
        if rules.dm { !rules.muted } else { rules.mentioned }
    }

    fn rules_for(&self, message: &Message) -> Rules {
        let me = self.my_id();
        let guild_id = self.guild_id_of(message);
        let settings = self.settings_for(guild_id.as_deref().unwrap_or("@me"));
        // A thread takes its channel's mute and level; the channel its category's.
        let all = || self.guilds.iter().flat_map(|g| g.channels.iter().chain(g.threads.iter()));
        let mut channel_id = message.channel_id.clone();
        if let Some(parent) = all().find(|c| c.id == channel_id && c.kind == crate::discord::ChannelKind::Thread).and_then(|t| t.parent_id.clone()) {
            channel_id = parent;
        }
        let parent = all().find(|c| c.id == channel_id).and_then(|c| c.parent_id.clone());
        let muted = settings.is_muted()
            || settings.channel_muted(&message.channel_id)
            || settings.channel_muted(&channel_id)
            || parent.as_ref().is_some_and(|p| settings.channel_muted(p));
        let direct = message.mentions.iter().any(|u| u.id == me);
        let everyone = message.mention_everyone && !settings.suppress_everyone;
        let role = !settings.suppress_roles && self.mentions_role(message);
        // 3 means "inherit": channel → category → server.
        let channel_level = settings.channels.get(&channel_id).map(|c| c.notifications).filter(|l| *l != 3);
        let category_level = parent.as_ref().and_then(|p| settings.channels.get(p)).map(|c| c.notifications).filter(|l| *l != 3);
        Rules { dm: guild_id.is_none(), muted, mentioned: direct || everyone || role, level: channel_level.or(category_level).unwrap_or(settings.notifications) }
    }

    /// The message's server, or None in a DM. History from REST comes without `guild_id`, so a missing one
    /// is looked up by channel.
    pub(crate) fn guild_id_of(&self, message: &Message) -> Option<String> {
        message.guild_id.clone().or_else(|| self.place_of(None, &message.channel_id).and_then(|(guild, _)| guild))
    }

    pub(crate) fn notify(&self, message: &Message) {
        if !self.prefs.desktop_notifications {
            return;
        }
        let place = match &message.guild_id {
            None => message.author.name.clone(),
            Some(guild_id) => {
                let guild = self.guilds.iter().find(|g| &g.id == guild_id);
                let channel = guild.and_then(|g| g.channels.iter().find(|c| c.id == message.channel_id)).map(|c| c.name.clone()).unwrap_or_default();
                format!("{} (#{channel}, {})", message.author.name, guild.map(|g| g.name.as_str()).unwrap_or_default())
            }
        };
        let body = if message.content.is_empty() { "Sent an attachment".to_owned() } else { message.content.clone() };
        // Not verified: how macOS attributes notifications from an unbundled binary (a .app bundle
        // shows its own name and icon).
        std::thread::spawn(move || {
            let _ = notify_rust::Notification::new().summary(&place).body(&body).show();
        });
    }

    // Discord's Dock badge: the number of unread mentions and DMs, or nothing.
    pub(crate) fn update_badge(&self) {
        let total: u32 = if self.prefs.dock_badge { self.read_states.values().map(|r| r.mentions).sum() } else { 0 };
        set_badge(if total == 0 { None } else { Some(total) });
    }
}

#[cfg(target_os = "macos")]
fn set_badge(count: Option<u32>) {
    use objc2::{
        msg_send,
        runtime::{AnyClass, AnyObject},
    };
    let label = count.map(|n| if n > 99 { "99+".to_owned() } else { n.to_string() }).unwrap_or_default();
    // SAFETY: plain AppKit messages on the main thread; nil checks below.
    unsafe {
        let (Some(app_class), Some(string_class)) = (AnyClass::get(c"NSApplication"), AnyClass::get(c"NSString")) else { return };
        let app: *mut AnyObject = msg_send![app_class, sharedApplication];
        if app.is_null() {
            return;
        }
        let tile: *mut AnyObject = msg_send![app, dockTile];
        let text = std::ffi::CString::new(label).unwrap_or_default();
        let ns: *mut AnyObject = msg_send![string_class, stringWithUTF8String: text.as_ptr()];
        let _: () = msg_send![tile, setBadgeLabel: ns];
    }
}

// ponytail: the taskbar overlay badge on Windows (ITaskbarList3) is the upgrade.
#[cfg(not(target_os = "macos"))]
fn set_badge(_: Option<u32>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_discord_levels() {
        assert_eq!(decide(true, 1, false, false), Notify::Message);
        assert_eq!(decide(true, 0, false, true), Notify::None);
        assert_eq!(decide(false, 0, false, false), Notify::Message);
        assert_eq!(decide(false, 0, false, true), Notify::None, "muted servers stay quiet");
        assert_eq!(decide(false, 0, true, true), Notify::Mention, "but mentions still come through");
        assert_eq!(decide(false, 1, false, false), Notify::None);
        assert_eq!(decide(false, 1, true, false), Notify::Mention);
        assert_eq!(decide(false, 2, true, false), Notify::None, "Nothing means nothing");
    }
}
