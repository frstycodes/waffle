// Desktop notifications and the Dock badge, following Discord's rules: nothing on Do Not Disturb, in quiet
// mode, or for the channel you're looking at; nothing at all from a muted server, category, channel or DM, or
// one set to Nothing; otherwise every message where the level is All messages, and only mentions (past the
// server's suppress rules) where it's Only @mentions. A thread takes its member settings, then its channel's.
use std::sync::{Mutex, OnceLock};

use futures::{StreamExt, channel::mpsc};
use gpui_kit::{Context, Window};

use crate::{
    Waffle,
    discord::{ChannelKind, Message, Relation, Status},
};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(not(target_os = "macos"))]
mod toast;
#[cfg(not(target_os = "macos"))]
use toast as platform;

// Waffle's AppUserModelID on Windows; the macOS bundle carries the same id in its Info.plist.
#[cfg(target_os = "windows")]
pub const APP_ID: &str = "com.frstycodes.waffle";

static CLICKS: OnceLock<Mutex<Option<mpsc::UnboundedSender<String>>>> = OnceLock::new();

#[derive(Debug, PartialEq, Eq)]
pub enum Notify {
    None,
    Message,
    Mention,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Level {
    All,
    Mentions,
    Nothing,
}

impl Level {
    // `message_notifications`, with 3 ("use the default") already resolved.
    fn from_setting(value: u8) -> Self {
        match value {
            0 => Level::All,
            1 => Level::Mentions,
            _ => Level::Nothing,
        }
    }

    // A thread member's flags: 2 all, 4 mentions, 8 nothing; none set follows the thread's channel.
    fn from_thread_flags(flags: u64) -> Option<Self> {
        [(2, Level::All), (4, Level::Mentions), (8, Level::Nothing)].into_iter().find(|(bit, _)| flags & bit != 0).map(|(_, level)| level)
    }
}

// How a message's place treats it, mutes and levels resolved thread → channel → category → server.
struct Rules {
    dm: bool,
    muted: bool,
    level: Level,
    // A voice channel's chat notifies every message only while we're in that call; otherwise mentions.
    all_here: bool,
    mentioned: bool,
}

fn decide(rules: &Rules) -> Notify {
    if rules.muted || rules.level == Level::Nothing {
        return Notify::None;
    }
    if rules.level == Level::All && rules.all_here {
        return Notify::Message;
    }
    if rules.mentioned { Notify::Mention } else { Notify::None }
}

impl Waffle {
    pub(crate) fn notify_kind(&self, message: &Message, focused_here: bool) -> Notify {
        let Some(me) = &self.me else { return Notify::None };
        // @silent: no sound and no desktop notification; it still counts as a mention.
        let quiet = self.my_status == Status::Dnd || self.account_settings.quiet_mode;
        if message.author.id == me.id || focused_here || quiet || message.silent() || self.hidden_author(&message.author.id).is_some() {
            return Notify::None;
        }
        decide(&self.rules_for(message))
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
        // DMs keep their overrides under "@me", which has no server default: All messages.
        let settings = self.settings_for(guild_id.as_deref().unwrap_or("@me"));
        let all = || self.guilds.iter().flat_map(|g| g.channels.iter().chain(g.threads.iter()));
        let here = all().find(|c| c.id == message.channel_id);
        let thread = here.filter(|c| c.kind == ChannelKind::Thread);
        let channel_id = thread.and_then(|t| t.parent_id.clone()).unwrap_or_else(|| message.channel_id.clone());
        let category = all().find(|c| c.id == channel_id).and_then(|c| c.parent_id.clone());
        let category_muted = category.as_ref().is_some_and(|p| settings.channel_muted(p));
        let opted_out = settings.opt_in() && !settings.opted_in(&channel_id) && !category.as_ref().is_some_and(|p| settings.opted_in(p));
        let mut muted = settings.is_muted() || settings.channel_muted(&message.channel_id) || settings.channel_muted(&channel_id) || category_muted || opted_out;
        // 3 means "inherit": channel → category → server.
        let override_of = |id: &String| settings.channels.get(id).map(|c| c.notifications).filter(|l| *l != 3);
        let mut level = Level::from_setting(override_of(&channel_id).or_else(|| category.as_ref().and_then(override_of)).unwrap_or(settings.notifications));
        let direct = message.mentions.iter().any(|u| u.id == me);
        let mut mentioned = direct || (message.mention_everyone && !settings.suppress_everyone) || (!settings.suppress_roles && self.mentions_role(message));
        if let Some(meta) = thread.and_then(|t| t.thread.as_ref()) {
            muted |= meta.muted;
            // GAP: Discord gives a thread you haven't joined nothing, but joins you to it when you're mentioned
            // there; Waffle doesn't see that join, so an unjoined thread notifies on mentions.
            let own = if meta.joined { Level::from_thread_flags(meta.flags) } else { Some(Level::Mentions) };
            if level != Level::Nothing {
                level = own.unwrap_or(level);
            }
            // Suppressing @everyone and roles is a server rule Discord doesn't apply inside threads.
            mentioned = direct || message.mention_everyone || self.mentions_role(message);
        }
        let in_voice = |id: &str| self.call.as_ref().is_some_and(|c| c.channel.id == id);
        let all_here = here.is_none_or(|c| c.kind != ChannelKind::Voice || in_voice(&c.id));
        Rules { dm: guild_id.is_none(), muted, level, all_here, mentioned }
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
        let guild_id = self.guild_id_of(message);
        let title = match &guild_id {
            None => message.author.name.clone(),
            Some(guild_id) => {
                let guild = self.guilds.iter().find(|g| &g.id == guild_id);
                let channel = guild.and_then(|g| g.channels.iter().chain(&g.threads).find(|c| c.id == message.channel_id)).map(|c| c.name.clone()).unwrap_or_default();
                format!("{} (#{channel}, {})", message.author.name, guild.map(|g| g.name.as_str()).unwrap_or_default())
            }
        };
        let body = if message.content.is_empty() { "Sent an attachment".to_owned() } else { message.content.clone() };
        let link = format!("https://discord.com/channels/{}/{}/{}", guild_id.as_deref().unwrap_or("@me"), message.channel_id, message.id);
        post(&title, &body, link);
    }

    // A clicked notification brings Waffle forward on the message it was about.
    pub(crate) fn open_clicked_notifications(window: &mut Window, cx: &mut Context<Self>) {
        let (sender, mut clicks) = mpsc::unbounded();
        let Ok(mut slot) = CLICKS.get_or_init(Default::default).lock() else { return };
        *slot = Some(sender);
        drop(slot);
        platform::install();
        cx.spawn_in(window, async move |this, cx| {
            while let Some(link) = clicks.next().await {
                let opened = this.update_in(cx, |this, window, cx| {
                    cx.activate(true);
                    window.activate_window();
                    this.open_link(&link, window, cx);
                });
                if opened.is_err() {
                    return;
                }
            }
        })
        // lint-allow(detached-task): ends when Waffle is dropped (the update fails) or the click channel closes.
        .detach();
    }

    // Discord's Dock badge: the number of unread mentions and DMs, or nothing.
    pub(crate) fn update_badge(&self) {
        let total: u32 = if self.prefs.dock_badge { self.read_states.values().map(|r| r.mentions).sum() } else { 0 };
        set_badge(if total == 0 { None } else { Some(total) });
    }
}

// `link` is the discord.com link a click opens.
pub(crate) fn post(title: &str, body: &str, link: String) {
    platform::post(title, body, link);
}

// Called from whichever thread the platform reports the click on.
fn clicked(link: String) {
    let Some(Ok(slot)) = CLICKS.get().map(Mutex::lock) else { return };
    if let Some(sender) = slot.as_ref() {
        let _ = sender.unbounded_send(link);
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

    fn rules(level: Level) -> Rules {
        Rules { dm: false, muted: false, level, all_here: true, mentioned: false }
    }

    #[test]
    fn follows_discord_levels() {
        assert_eq!(decide(&rules(Level::All)), Notify::Message);
        assert_eq!(decide(&rules(Level::Mentions)), Notify::None);
        assert_eq!(decide(&Rules { mentioned: true, ..rules(Level::Mentions) }), Notify::Mention);
        assert_eq!(decide(&Rules { mentioned: true, ..rules(Level::Nothing) }), Notify::None, "Nothing means nothing");
    }

    #[test]
    fn muted_places_stay_quiet_even_for_mentions() {
        assert_eq!(decide(&Rules { muted: true, ..rules(Level::All) }), Notify::None);
        assert_eq!(decide(&Rules { muted: true, mentioned: true, ..rules(Level::All) }), Notify::None);
        assert_eq!(decide(&Rules { dm: true, muted: true, ..rules(Level::All) }), Notify::None);
    }

    #[test]
    fn a_voice_chat_away_from_its_call_only_notifies_mentions() {
        assert_eq!(decide(&Rules { all_here: false, ..rules(Level::All) }), Notify::None);
        assert_eq!(decide(&Rules { all_here: false, mentioned: true, ..rules(Level::All) }), Notify::Mention);
    }

    #[test]
    fn reads_levels_and_thread_flags() {
        assert_eq!([0, 1, 2].map(Level::from_setting), [Level::All, Level::Mentions, Level::Nothing]);
        assert_eq!(Level::from_thread_flags(1 | 4), Some(Level::Mentions));
        assert_eq!(Level::from_thread_flags(8), Some(Level::Nothing));
        assert_eq!(Level::from_thread_flags(1), None);
    }
}
