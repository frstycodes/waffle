// Discord's own links in a message, previewed the way Discord does: a message link quotes the message it
// points at, an invite shows its server's card, and a channel link reads as the channel's chip (in
// `chat::readable`). A card fetches when its row first renders, which the chat list does only for rows in
// view, and the last fetch shows at once afterwards (`query.rs`).
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    f32::consts::TAU,
    rc::Rc,
    time::Duration,
};

use gpui_kit::{Animation, AnimationExt, AnyElement, App, Context, FontWeight, SharedString, Stateful, WeakEntity, Window, component::Icon, div, img, prelude::*, px};

use crate::{
    Waffle,
    discord::{Invite, MediaKind, Message, Status},
    icons::Glyph,
    palette::{self, Level},
    query::Query,
    ui::{self, RADIUS_LG, RADIUS_MD, RADIUS_SM, Squircle, Tone},
};

// Previews for the first few Discord links in a message; any more stay plain links.
const MAX_PREVIEWS: usize = 3;
const MESSAGE_WIDTH: f32 = 432.;
const INVITE_WIDTH: f32 = 400.;
const INVITE_ICON: f32 = 48.;
const THUMB: f32 = 64.;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscordLink {
    // `guild` is "@me" for a DM.
    Message { guild: String, channel: String, message: String },
    Channel { guild: String, channel: String },
    Invite(String),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Preview {
    Message(Box<Message>),
    Invite(Invite),
    // Deleted, expired, or in a channel you can't read: a message link previews nothing, an invite shows
    // the invalid card.
    Unavailable,
}

pub type Previews = Rc<RefCell<Query<Preview>>>;

pub fn previews() -> Previews {
    Rc::new(RefCell::new(Query::new("link-previews", Duration::from_secs(10 * 60))))
}

// `https://discord.com/channels/…`, the ptb, canary and old discordapp hosts, and both invite forms.
pub fn parse(url: &str) -> Option<DiscordLink> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let (host, path) = rest.split_once('/')?;
    let host = host.strip_prefix("www.").unwrap_or(host).to_ascii_lowercase();
    let path = path.split(['?', '#']).next()?.trim_end_matches('/');
    let parts: Vec<&str> = path.split('/').collect();
    let app = matches!(host.as_str(), "discord.com" | "ptb.discord.com" | "canary.discord.com" | "discordapp.com" | "ptb.discordapp.com" | "canary.discordapp.com");
    match (host.as_str(), parts.as_slice()) {
        ("discord.gg", [code]) => invite(code),
        (_, ["invite", code]) if app => invite(code),
        (_, ["channels", guild, channel, rest @ ..]) if app && (*guild == "@me" || snowflake(guild)) && snowflake(channel) => match rest {
            [] => Some(DiscordLink::Channel { guild: guild.to_string(), channel: channel.to_string() }),
            [message] if snowflake(message) => Some(DiscordLink::Message { guild: guild.to_string(), channel: channel.to_string(), message: message.to_string() }),
            _ => None,
        },
        _ => None,
    }
}

fn invite(code: &str) -> Option<DiscordLink> {
    (!code.is_empty() && code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')).then(|| DiscordLink::Invite(code.to_owned()))
}

fn snowflake(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

// The links in a message that get a card, in order, each once. A link wrapped in `<…>` is one its
// author chose not to preview, as in Discord; masked `[text](url)` links aren't previewed either.
pub fn previewed(content: &str) -> Vec<DiscordLink> {
    let mut out = Vec::new();
    for word in content.split_whitespace() {
        if !word.starts_with("http") {
            continue;
        }
        let url = word.trim_end_matches(['.', ',', '!', '?', ')', ';', ':', '*', '_', '~', '|']);
        let Some(link) = parse(url) else { continue };
        if matches!(link, DiscordLink::Channel { .. }) || out.contains(&link) {
            continue;
        }
        out.push(link);
        if out.len() == MAX_PREVIEWS {
            break;
        }
    }
    out
}

// Bare channel links to a channel we know, as the chip `<#id>` makes. A link inside `<…>` or a masked
// `[text](url)` keeps its own text.
pub fn channel_chips(text: &str, names: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("http") {
        let (before, tail) = rest.split_at(start);
        out.push_str(before);
        let end = tail.find(char::is_whitespace).unwrap_or(tail.len());
        let word = &tail[..end];
        let url = word.trim_end_matches(['.', ',', '!', '?', ')', ';', ':', '*', '_', '~', '|']);
        let wrapped = before.ends_with(['<', '(']);
        let chip = match parse(url) {
            Some(DiscordLink::Channel { channel, .. }) if !wrapped => names.get(&channel).map(|name| format!("[#{name}](waffle://channel/{channel})")),
            _ => None,
        };
        match chip {
            Some(chip) => {
                out.push_str(&chip);
                out.push_str(&word[url.len()..]);
            }
            None => out.push_str(word),
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

impl DiscordLink {
    // Its cache key; a channel link has nothing to fetch.
    pub fn key(&self) -> Option<String> {
        match self {
            Self::Message { channel, message, .. } => Some(format!("m-{channel}-{message}")),
            Self::Invite(code) => Some(format!("i-{code}")),
            Self::Channel { .. } => None,
        }
    }
}

impl Waffle {
    // Fetches a preview unless a fresh one is cached; the card that asked redraws when it lands.
    pub(crate) fn load_link_preview(&mut self, link: DiscordLink, cx: &mut Context<Self>) {
        let Some(key) = link.key() else { return };
        let Some(client) = self.client.clone() else { return };
        if !self.link_previews.borrow_mut().should_fetch(&key) {
            return;
        }
        cx.spawn(async move |this, cx| {
            let fetched = match &link {
                DiscordLink::Message { channel, message, .. } => client.linked_message(channel, message).await.map(|m| m.map_or(Preview::Unavailable, |m| Preview::Message(Box::new(m)))),
                DiscordLink::Invite(code) => client.invite(code).await.map(|i| i.map_or(Preview::Unavailable, Preview::Invite)),
                DiscordLink::Channel { .. } => return,
            };
            this.update(cx, |this, cx| {
                let mut previews = this.link_previews.borrow_mut();
                match fetched {
                    Ok(preview) => previews.store(&key, preview),
                    Err(_) => previews.failed(&key),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // A clicked link: a Discord one opens here, anything else in the browser.
    pub(crate) fn open_link(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(link) = parse(url) else { return cx.open_url(url) };
        match link {
            DiscordLink::Message { guild, channel, message } => self.open_linked(guild, channel, Some(message), window, cx),
            DiscordLink::Channel { guild, channel } => self.open_linked(guild, channel, None, window, cx),
            DiscordLink::Invite(code) => self.open_invite(code, cx),
        }
    }

    // The channel a link points at, scrolled to and flashing its message when it names one.
    fn open_linked(&mut self, guild: String, channel_id: String, message_id: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let here = self.channel.as_ref().is_some_and(|c| c.id == channel_id);
        if !here {
            let guild_id = (guild != "@me").then_some(guild);
            let channel = match &guild_id {
                Some(g) => self.guilds.iter().find(|x| &x.id == g).and_then(|x| x.channels.iter().chain(&x.threads).find(|c| c.id == channel_id)).cloned(),
                None => self.dms.iter().find(|c| c.id == channel_id).cloned(),
            };
            let Some(channel) = channel else { return self.show_toast("You don't have access to that channel", cx) };
            if self.guild_id != guild_id {
                self.open_guild(guild_id, window, cx);
            }
            self.open_channel(channel, window, cx);
        }
        if let Some(message_id) = message_id {
            self.jump_to(message_id, cx);
        }
    }

    // Joins the invite's server and opens it. A captcha Discord asks for shows in the browser, as at sign-in.
    pub(crate) fn join_invite(&mut self, code: String, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let (captcha_url, mut captcha_urls) = futures::channel::mpsc::unbounded::<String>();
        cx.spawn(async move |_, cx| {
            while let Some(url) = futures::StreamExt::next(&mut captcha_urls).await {
                cx.update(|cx: &mut App| cx.open_url(&url));
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let joined = client.join_invite(&code, captcha_url).await;
            this.update(cx, |this, cx| match joined {
                Ok(guild_id) => {
                    this.join.just_joined.insert(guild_id.clone());
                    this.apply_invite_settings(&guild_id, cx);
                    this.open_joined_guild(guild_id, cx)
                }
                Err(error) => this.show_toast(error, cx),
            })
            .ok();
        })
        .detach();
    }

    // A server we just joined opens once its GUILD_CREATE lands; one we were already in opens now.
    pub(crate) fn open_joined_guild(&mut self, guild_id: String, cx: &mut Context<Self>) {
        if !self.guilds.iter().any(|g| g.id == guild_id) {
            self.joining = Some(guild_id);
            return;
        }
        self.on_joined(&guild_id, cx);
        let (this, main) = (cx.weak_entity(), self.main_window);
        // The main window's root is this view, so it can only be borrowed once this update ends.
        cx.defer(move |cx| {
            main.update(cx, |_, window, cx| this.update(cx, |this, cx| this.open_guild(Some(guild_id), window, cx))).ok();
        });
    }
}

// What a card needs from the row drawing it.
pub struct CardContext<'a> {
    pub this: WeakEntity<Waffle>,
    pub previews: &'a Previews,
    pub joined: &'a HashSet<String>,
    pub channel_names: &'a HashMap<String, String>,
    // The quoted message's text as one plain line, mentions and emoji resolved.
    pub quote: &'a dyn Fn(&str) -> String,
    pub stamp: fn(&str) -> String,
}

// A card for each previewed link in `content`: the preview, its skeleton while the first fetch runs, or
// nothing for a message you can't see.
pub fn cards(owner: &str, content: &str, ctx: &CardContext, cx: &mut App) -> Vec<AnyElement> {
    previewed(content)
        .into_iter()
        .enumerate()
        .filter_map(|(i, link)| {
            let key = link.key()?;
            let id = format!("link-{owner}-{i}");
            let preview = ctx.previews.borrow_mut().cached(&key);
            let this = ctx.this.clone();
            let fetch = link.clone();
            // Every render asks; a fresh or running fetch makes it a no-op, a stale one refreshes underneath.
            cx.defer(move |cx| {
                this.update(cx, |this, cx| this.load_link_preview(fetch, cx)).ok();
            });
            match (link, preview) {
                (DiscordLink::Message { .. }, None) => Some(skeleton(id, false, cx)),
                (DiscordLink::Invite(_), None) => Some(skeleton(id, true, cx)),
                (DiscordLink::Message { guild, channel, .. }, Some(Preview::Message(message))) => Some(message_card(id, &guild, &channel, &message, ctx, cx)),
                (DiscordLink::Invite(code), Some(Preview::Invite(invite))) => Some(invite_card(id, &code, &invite, ctx, cx)),
                (DiscordLink::Invite(_), Some(_)) => Some(invalid_invite(id, cx)),
                _ => None,
            }
        })
        .collect()
}

// One surface step up from the feed, like the embed cards.
fn card(id: String, width: f32, cx: &App) -> Stateful<gpui_kit::Div> {
    div()
        .id(SharedString::from(id))
        .mt_2()
        .w_full()
        .max_w(px(width))
        .py_3()
        .px_3p5()
        .flex()
        .squircle(RADIUS_LG)
        .bg(palette::surface(Level::Sidebar, cx))
        .border_1()
        .border_color(palette::line(cx))
}

fn message_card(id: String, guild: &str, channel: &str, message: &Message, ctx: &CardContext, cx: &App) -> AnyElement {
    let (strong, muted, faint) = (palette::strong(cx), palette::muted(cx), palette::faint(cx));
    let place = match ctx.channel_names.get(channel) {
        Some(name) => div().flex().items_center().gap_1().min_w_0().child(Icon::new(Glyph::Hashtag).size(ui::ICON_SM).flex_none()).child(div().min_w_0().truncate().child(name.clone())),
        None => div().child(if guild == "@me" { "Direct message" } else { "Message" }),
    };
    let quote = (ctx.quote)(&message.content);
    let (has_quote, bare) = (!quote.trim().is_empty(), quote.trim().is_empty() && message.media.is_empty());
    let thumb = message.media.first().map(|m| match &m.kind {
        MediaKind::Image => m.url.clone(),
        MediaKind::Video { poster } | MediaKind::Gif { poster } => poster.clone(),
    });
    let text = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_1()
        .text_size(ui::TEXT_SM)
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .min_w_0()
                .child(ui::avatar(message.author.avatar.clone(), 20.).flex_none())
                .child(div().min_w_0().truncate().font_weight(FontWeight::MEDIUM).text_color(strong).child(message.author.name.clone()))
                .child(div().flex_none().text_size(ui::TEXT_XS).text_color(faint).child((ctx.stamp)(&message.timestamp))),
        )
        .when(has_quote, |t| t.child(div().text_color(muted).line_clamp(3).text_ellipsis().child(quote)))
        .when(bare, |t| t.child(div().text_color(faint).child("Attachment")))
        .child(
            div()
                .mt_1()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .text_size(ui::TEXT_XS)
                .text_color(faint)
                .child(place)
                .child(div().flex_none().font_weight(FontWeight::MEDIUM).text_color(palette::accent_ink(cx)).child("Jump")),
        );
    let (this, guild, channel, target) = (ctx.this.clone(), guild.to_owned(), channel.to_owned(), message.id.clone());
    let hover = palette::hover(cx);
    card(id, MESSAGE_WIDTH, cx)
        .items_start()
        .gap_3p5()
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .child(text)
        .children(thumb.map(|url| img(url).size(px(THUMB)).flex_none().squircle(RADIUS_SM).object_fit(gpui_kit::ObjectFit::Cover)))
        .on_click(move |_, window, cx| { cx.stop_propagation();
            let (guild, channel, target) = (guild.clone(), channel.clone(), target.clone());
            this.update(cx, |this, cx| this.open_linked(guild, channel, Some(target), window, cx)).ok();
        })
        .into_any_element()
}

fn invite_card(id: String, code: &str, invite: &Invite, ctx: &CardContext, cx: &App) -> AnyElement {
    let joined = ctx.joined.contains(&invite.guild_id);
    let count = |n: Option<u64>, label: &'static str, dot: gpui_kit::Hsla| {
        n.map(|n| div().flex().items_center().gap_1p5().child(div().size_2().rounded_full().bg(dot)).child(format!("{} {label}", grouped(n))))
    };
    let counts = div()
        .flex()
        .flex_wrap()
        .gap_x_3()
        .text_size(ui::TEXT_XS)
        .text_color(palette::muted(cx))
        .children(count(invite.online, "Online", palette::status_in(Status::Online, cx)))
        .children(count(invite.members, "Members", palette::faint(cx)));
    let icon = match &invite.icon {
        Some(url) => img(url.clone()).size(px(INVITE_ICON)).flex_none().squircle(RADIUS_MD).into_any_element(),
        None => initials_tile(&invite.name, cx),
    };
    let (this, guild_id, code) = (ctx.this.clone(), invite.guild_id.clone(), code.to_owned());
    let action = match joined {
        true => ui::button(SharedString::from(format!("{id}-open")), Tone::Secondary, cx).child("Joined").on_click(move |_, window, cx| { cx.stop_propagation();
            let guild_id = guild_id.clone();
            this.update(cx, |this, cx| this.open_guild(Some(guild_id), window, cx)).ok();
        }),
        false => ui::button(SharedString::from(format!("{id}-join")), Tone::Primary, cx).child("Join").on_click(move |_, _, cx| { cx.stop_propagation();
            let code = code.clone();
            this.update(cx, |this, cx| this.join_invite(code, cx)).ok();
        }),
    };
    let name = div().min_w_0().truncate().text_size(ui::TEXT_MD).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(invite.name.clone());
    let row = div().child(icon).child(div().flex_1().min_w_0().flex().flex_col().gap_0p5().child(name).child(counts)).child(div().flex_none().child(action));
    invite_frame(id, row, cx)
}

fn invalid_invite(id: String, cx: &App) -> AnyElement {
    let tile = div().size(px(INVITE_ICON)).flex_none().squircle(RADIUS_MD).bg(palette::selected(cx)).flex().items_center().justify_center().text_color(palette::faint(cx)).child(Icon::new(Glyph::Prohibit).size(px(22.)));
    let row = div().child(tile).child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_size(ui::TEXT_MD).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child("Invalid invite"))
                .child(div().text_size(ui::TEXT_XS).text_color(palette::muted(cx)).child("It expired or was revoked. Ask for a new one.")),
    );
    invite_frame(id, row, cx)
}

// The invite card: its caption over one row of icon, name and action.
fn invite_frame(id: String, row: gpui_kit::Div, cx: &App) -> AnyElement {
    card(id, INVITE_WIDTH, cx)
        .flex_col()
        .gap_2p5()
        .child(div().text_size(ui::TEXT_XS).text_color(palette::faint(cx)).child("Invite to join a server"))
        .child(row.flex().items_center().gap_3())
        .into_any_element()
}

fn initials_tile(name: &str, cx: &App) -> AnyElement {
    let initials: String = name.split_whitespace().filter_map(|w| w.chars().next()).take(3).collect();
    div().size(px(INVITE_ICON)).flex_none().squircle(RADIUS_MD).bg(palette::selected(cx)).flex().items_center().justify_center().text_size(ui::TEXT_MD).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(initials).into_any_element()
}

// The card's real layout in bars, pulsing like the feed's skeleton.
fn skeleton(id: String, invite: bool, cx: &App) -> AnyElement {
    let bar = palette::selected(cx);
    let line = |w: f32, h: f32| div().w(px(w)).max_w_full().h(px(h)).rounded_full().bg(bar);
    let body = match invite {
        true => card(id.clone(), INVITE_WIDTH, cx).flex_col().gap_2p5().child(line(140., 10.)).child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(div().size(px(INVITE_ICON)).flex_none().squircle(RADIUS_MD).bg(bar))
                .child(div().flex_1().flex().flex_col().gap_2().child(line(160., 14.)).child(line(180., 10.)))
                .child(div().w_16().h_9().flex_none().squircle(RADIUS_SM).bg(bar)),
        ),
        false => card(id.clone(), MESSAGE_WIDTH, cx).flex_col().gap_2().child(div().flex().items_center().gap_2().child(div().size_5().rounded_full().bg(bar)).child(line(96., 12.)).child(line(72., 10.))).child(line(320., 12.)).child(line(220., 12.)).child(line(80., 10.)),
    };
    body.with_animation(SharedString::from(format!("{id}-pulse")), Animation::new(Duration::from_millis(1100)).repeat(), |el, t| el.opacity(0.6 + 0.4 * (t * TAU).cos().abs())).into_any_element()
}

// "8,311", as Discord writes member counts.
pub(crate) fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(guild: &str) -> Option<DiscordLink> {
        Some(DiscordLink::Message { guild: guild.into(), channel: "22".into(), message: "333".into() })
    }

    #[test]
    fn message_links_on_every_host() {
        for host in ["discord.com", "ptb.discord.com", "canary.discord.com", "discordapp.com", "www.discord.com", "Discord.com"] {
            assert_eq!(parse(&format!("https://{host}/channels/1/22/333")), message("1"), "{host}");
        }
        assert_eq!(parse("https://discord.com/channels/@me/22/333"), message("@me"));
        assert_eq!(parse("https://discord.com/channels/1/22/333/"), message("1"));
        assert_eq!(parse("https://discord.com/channels/1/22/333?query=1#x"), message("1"));
        assert_eq!(parse("http://discord.com/channels/1/22/333"), message("1"));
    }

    #[test]
    fn channel_and_invite_links() {
        assert_eq!(parse("https://discord.com/channels/1/22"), Some(DiscordLink::Channel { guild: "1".into(), channel: "22".into() }));
        for url in ["https://discord.gg/waffle-hall", "https://discord.com/invite/waffle-hall", "https://canary.discord.com/invite/waffle-hall/", "https://discord.gg/waffle-hall?event=4"] {
            assert_eq!(parse(url), Some(DiscordLink::Invite("waffle-hall".into())), "{url}");
        }
    }

    #[test]
    fn other_links_are_not_discords() {
        for url in [
            "https://discord.com/channels/1/22/333/4",
            "https://discord.com/channels/abc/22",
            "https://discord.com/channels/1",
            "https://discord.com/invite/",
            "https://discord.gg/a/b",
            "https://discord.gg/bad%20code",
            "https://notdiscord.com/channels/1/22/333",
            "https://discord.com.evil.io/channels/1/22/333",
            "https://discord.com/events/1/2",
            "discord.com/channels/1/22/333",
        ] {
            assert_eq!(parse(url), None, "{url}");
        }
    }

    #[test]
    fn previews_the_first_few_once_each() {
        let text = "see https://discord.com/channels/1/22/333, and https://discord.com/channels/1/22/333 again; join https://discord.gg/a! \
            not <https://discord.gg/b> or https://discord.com/channels/1/22 then https://discord.gg/c https://discord.gg/d";
        let links = previewed(text);
        assert_eq!(links, vec![message("1").unwrap(), DiscordLink::Invite("a".into()), DiscordLink::Invite("c".into())]);
    }

    #[test]
    fn known_channel_links_become_chips() {
        let names = HashMap::from([("22".to_string(), "general".to_string())]);
        assert_eq!(channel_chips("over in https://discord.com/channels/1/22.", &names), "over in [#general](waffle://channel/22).");
        let kept = "https://discord.com/channels/1/99 <https://discord.com/channels/1/22> [here](https://discord.com/channels/1/22) https://discord.com/channels/1/22/333 http";
        assert_eq!(channel_chips(kept, &names), kept, "unknown, suppressed, masked and message links stay as written");
    }

    #[test]
    fn keys_fit_the_cache() {
        assert_eq!(message("1").unwrap().key().as_deref(), Some("m-22-333"));
        assert_eq!(DiscordLink::Invite("waffle-hall".into()).key().as_deref(), Some("i-waffle-hall"));
        assert_eq!(DiscordLink::Channel { guild: "1".into(), channel: "2".into() }.key(), None);
    }

    #[test]
    fn unavailable_is_cached_and_not_refetched() {
        let mut q: Query<Preview> = Query::at(None, Duration::from_secs(60));
        assert!(q.should_fetch("m-22-333"));
        q.store("m-22-333", Preview::Unavailable);
        assert!(matches!(q.cached("m-22-333"), Some(Preview::Unavailable)));
        assert!(!q.should_fetch("m-22-333"), "a forbidden or deleted message isn't asked for again while fresh");
        assert!(q.should_fetch("i-gone"));
        q.failed("i-gone");
        assert!(q.cached("i-gone").is_none(), "a network failure caches nothing");
        assert!(q.should_fetch("i-gone"), "and is tried again");
    }

    #[test]
    fn counts_read_like_discord() {
        assert_eq!([grouped(0), grouped(999), grouped(1204), grouped(8_311_000)], ["0", "999", "1,204", "8,311,000"]);
    }
}
