// Discord's system messages (parity §1.7), drawn as design/explore-system-messages.html: one muted line with
// its icon in the avatar column for most types, a message-like row for AutoMod, the parent message for a
// thread's starter, and nothing for the types Discord draws nothing for. Copy, icons and rules come from
// docs/discord-system-messages-embeds-re.md.
use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

use gpui_kit::{AnyElement, App, Context, Div, FontWeight, HighlightStyle, Hsla, SharedString, Stateful, StyledText, component::Icon, div, prelude::*, px};

use crate::{
    Waffle,
    chat::{self, AVATAR, RowContext},
    discord::{Channel, ChannelKind, Message, SendState, Sticker, StickerFormat, User, perm},
    icons::Glyph,
    palette,
    ui::{self, RADIUS_MD, Squircle},
};

// Discord's USER_MESSAGE types: every other type is a system notice.
const USER_KINDS: [u8; 9] = [0, 19, 20, 23, 26, 41, 45, 47, 68];
const DISCORD_EPOCH: u64 = 1_420_070_400_000;
// Not verified: the bit. Discord's SystemChannelFlags has SUPPRESS_JOIN_NOTIFICATION_REPLIES as 1 << 3;
// the RE doc names the flag but not its value.
const SUPPRESS_JOIN_NOTIFICATION_REPLIES: u64 = 1 << 3;
// The run of joins that folds into one line.
const FOLD_AT: usize = 3;
const POLL_TITLE_CHARS: usize = 50;
// Space between a sentence's words; a piece glued to the one before it (a full stop after a name) pulls back by it.
const WORD_GAP: f32 = 4.;

// Discord's 13 welcome lines, around the joiner's name, picked by the message's timestamp.
const JOIN_LINES: [(&str, &str); 13] = [
    ("", " joined the party."),
    ("", " is here."),
    ("Welcome, ", ". We hope you brought pizza."),
    ("A wild ", " appeared."),
    ("", " just landed."),
    ("", " just slid into the server."),
    ("", " just showed up!"),
    ("Welcome ", ". Say hi!"),
    ("", " hopped into the server."),
    ("Everyone welcome ", "!"),
    ("Glad you're here, ", "."),
    ("Good to see you, ", "."),
    ("Yay you made it, ", "!"),
];

// "Wave to say hi!" sends one of these: Wumpus, Nelly, Clyde, Choco (APNG), Doggo, Peach.
const WAVE_STICKERS: [(&str, &str, StickerFormat); 6] = [
    ("749054660769218631", "Wave", StickerFormat::Lottie),
    ("751606379340365864", "Wave", StickerFormat::Lottie),
    ("754108890559283200", "Wave", StickerFormat::Lottie),
    ("781291131828699156", "Wave", StickerFormat::Apng),
    ("816087792291282944", "Sup", StickerFormat::Lottie),
    ("819128604311027752", "Scream", StickerFormat::Lottie),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    Message,
    Line,
    AutoMod,
    ThreadStarter,
    // Discord draws nothing for it.
    Hidden,
}

pub(crate) fn shape(message: &Message) -> Shape {
    if message.call.is_some() || message.pin_notice {
        return Shape::Line;
    }
    match message.kind {
        k if USER_KINDS.contains(&k) => Shape::Message,
        1 | 2 | 4 | 5 | 7..=12 | 14..=18 | 27..=31 | 46 => Shape::Line,
        24 | 36..=39 => Shape::AutoMod,
        21 => Shape::ThreadStarter,
        _ => Shape::Hidden,
    }
}

pub(crate) fn is_notice(message: &Message) -> bool {
    shape(message) != Shape::Message
}

// A snowflake's creation time in Unix milliseconds.
pub(crate) fn snowflake_ms(id: &str) -> u64 {
    id.parse::<u64>().map_or(0, |id| (id >> 22) + DISCORD_EPOCH)
}

fn join_copy(message_id: &str) -> (&'static str, &'static str) {
    JOIN_LINES[(snowflake_ms(message_id) % JOIN_LINES.len() as u64) as usize]
}

// Per viewer and per message, so everyone waves with a different sticker.
fn wave_sticker(me: &str, message_id: &str) -> Sticker {
    let (id, name, format) = WAVE_STICKERS[((snowflake_ms(me) + snowflake_ms(message_id)) % WAVE_STICKERS.len() as u64) as usize];
    Sticker { id: id.to_owned(), name: name.to_owned(), format, tags: String::new() }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fold {
    None,
    // The run's first join: draws the folded line for `len` joins.
    Head(usize),
    // Folded into the line above: draws nothing.
    Tail,
}

// Waffle's addition: three or more people joining in a row, nothing between them and on one day, fold into one line.
pub(crate) fn join_fold(messages: &[Message], ix: usize) -> Fold {
    let human_join = |m: &Message| m.kind == 7 && !m.author.bot;
    let linked = |a: usize, b: usize| human_join(&messages[a]) && human_join(&messages[b]) && chat::day(&messages[a].timestamp) == chat::day(&messages[b].timestamp);
    if !human_join(&messages[ix]) {
        return Fold::None;
    }
    let mut start = ix;
    while start > 0 && linked(start - 1, start) {
        start -= 1;
    }
    let mut end = ix;
    while end + 1 < messages.len() && linked(end, end + 1) {
        end += 1;
    }
    match (end - start + 1 >= FOLD_AT, start == ix) {
        (false, _) => Fold::None,
        (true, true) => Fold::Head(end - start + 1),
        (true, false) => Fold::Tail,
    }
}

// What rows need to know about the open channel and server beyond the messages themselves.
#[derive(Default)]
pub(crate) struct Facts {
    guild_name: Option<String>,
    group: bool,
    thread: bool,
    forum_post: bool,
    greet: bool,
    server_settings: Option<String>,
    // Threads named by thread-created notices that we know, so their names open them.
    threads: HashMap<String, Channel>,
    // Channels named by AutoMod notices where we may delete the flagged message.
    moderated: HashSet<String>,
    open_runs: HashSet<String>,
}

impl Waffle {
    pub(crate) fn notice_facts(&self, messages: &[Message], channel: Option<&Channel>) -> Facts {
        let Some(channel) = channel else { return Facts::default() };
        let guild = self.guild().filter(|_| channel.kind != ChannelKind::Dm);
        let suppressed = guild.is_some_and(|g| g.system_channel_flags & SUPPRESS_JOIN_NOTIFICATION_REPLIES != 0);
        let locked = channel.thread.as_ref().is_some_and(|t| t.archived && t.locked) && !self.can_in(&channel.id, perm::MANAGE_THREADS);
        let known = |id: &String| guild.and_then(|g| g.threads.iter().chain(&g.channels).find(|t| &t.id == id)).cloned();
        Facts {
            guild_name: guild.map(|g| g.name.clone()),
            group: channel.kind == ChannelKind::Dm,
            thread: channel.kind == ChannelKind::Thread,
            forum_post: self.thread_parent(channel).is_some_and(|p| p.kind == ChannelKind::Forum),
            greet: guild.is_some() && !suppressed && !locked && self.can_in(&channel.id, perm::SEND_MESSAGES),
            server_settings: guild.filter(|g| self.can_open_server_settings(&g.id)).map(|g| g.id.clone()),
            threads: messages.iter().filter(|m| m.kind == 18).filter_map(|m| known(m.reference_channel.as_ref()?)).map(|t| (t.id.clone(), t)).collect(),
            moderated: messages.iter().filter(|m| m.kind == 24).filter_map(|m| automod_field(m, "channel_id")).filter(|c| self.can_in(c, perm::MANAGE_MESSAGES)).map(str::to_owned).collect(),
            open_runs: self.open_join_runs.clone(),
        }
    }

    fn toggle_join_run(&mut self, first_id: String, cx: &mut Context<Self>) {
        if !self.open_join_runs.remove(&first_id) {
            self.open_join_runs.insert(first_id);
        }
        cx.notify();
    }

    /// Discord's "Wave to say hi!": a wave sticker in reply to a join, shown at once like a sent sticker.
    pub(crate) fn wave(&mut self, join: Message, cx: &mut Context<Self>) {
        let (Some(client), Some(me)) = (self.client.clone(), self.me.clone()) else { return };
        let Some(guild_id) = join.guild_id.clone().or_else(|| self.guild_id.clone()) else { return };
        let sticker = wave_sticker(&me.id, &join.id);
        let nonce = crate::nonce();
        let pending = Message {
            id: format!("pending-{nonce}"),
            channel_id: join.channel_id.clone(),
            guild_id: Some(guild_id.clone()),
            author: me,
            timestamp: chrono::Utc::now().to_rfc3339(),
            reply_to: Some(join.id.clone()),
            reply: Some(Box::new(join.clone())),
            nonce: Some(nonce.clone()),
            state: SendState::Sending,
            stickers: vec![sticker.clone()],
            ..Default::default()
        };
        self.push_local(pending, cx);
        cx.spawn(async move |this, cx| {
            let result = client.greet(&join.channel_id, &guild_id, &join.id, &sticker.id).await;
            this.update(cx, |this, cx| this.settle_greet(&join.channel_id, &nonce, result, cx)).ok();
        })
        .detach();
    }

    // The greet endpoint takes no nonce, so the gateway's copy can't replace the pending one: when it
    // landed first, the pending copy just goes.
    fn settle_greet(&mut self, channel_id: &str, nonce: &str, result: Result<Message, String>, cx: &mut Context<Self>) {
        let echoed = result.as_ref().is_ok_and(|sent| self.feeds.get(channel_id).is_some_and(|f| f.messages.iter().any(|m| m.id == sent.id)));
        if !echoed {
            return self.settle(channel_id, nonce, result, cx);
        }
        self.remove_message(channel_id, &format!("pending-{nonce}"));
        cx.notify();
    }
}

pub(crate) fn row(messages: &[Message], ix: usize, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    let message = &messages[ix];
    if let Some(call) = &message.call {
        return chat::call_line(message, call, ctx, cx);
    }
    if message.pin_notice {
        return chat::pin_line(message, ctx, cx);
    }
    match shape(message) {
        Shape::Message | Shape::Hidden => nothing(),
        Shape::AutoMod => automod_row(message, ctx, cx),
        Shape::ThreadStarter => starter_row(message, ctx, cx),
        Shape::Line => match join_fold(messages, ix) {
            Fold::Tail => nothing(),
            Fold::Head(len) => fold_line(&messages[ix..ix + len], ctx, cx),
            Fold::None => line(message, ctx, cx),
        },
    }
}

fn nothing() -> AnyElement {
    div().into_any_element()
}

/// A notice's sentence: muted words that wrap one by one, with names and links set in among them.
pub(crate) struct Sentence {
    key: String,
    parts: Vec<AnyElement>,
    trailing_space: bool,
    strong: Hsla,
}

impl Sentence {
    // `key` keeps the ids of its names and links apart from other sentences in the same row.
    pub(crate) fn new(key: &str, cx: &App) -> Self {
        Sentence { key: key.to_owned(), parts: Vec::new(), trailing_space: false, strong: palette::strong(cx) }
    }

    pub(crate) fn text(self, text: &str) -> Self {
        self.words(text, |w| w)
    }

    pub(crate) fn strong(self, text: &str) -> Self {
        let strong = self.strong;
        self.words(text, move |w| w.font_weight(FontWeight::SEMIBOLD).text_color(strong))
    }

    // Their name, opening their profile.
    pub(crate) fn name(self, user: &User, ctx: &Rc<RowContext>) -> Self {
        let id = self.id("name");
        let name = chat::profile_trigger(id, user, ctx).font_weight(FontWeight::SEMIBOLD).text_color(self.strong).hover(|s| s.underline()).child(ctx.name_of(user));
        self.part(name)
    }

    pub(crate) fn link(&self, label: impl Into<SharedString>) -> Stateful<Div> {
        let id = self.id("link");
        div().id(SharedString::from(id)).cursor_pointer().font_weight(FontWeight::SEMIBOLD).text_color(self.strong).hover(|s| s.underline()).child(label.into())
    }

    pub(crate) fn part(mut self, part: impl IntoElement) -> Self {
        let spaced = self.trailing_space;
        self.push(part.into_any_element(), spaced);
        self.trailing_space = false;
        self
    }

    fn id(&self, what: &str) -> String {
        format!("{}-{what}-{}", self.key, self.parts.len())
    }

    fn words(mut self, text: &str, style: impl Fn(Div) -> Div) -> Self {
        let mut spaced = self.trailing_space || text.starts_with(char::is_whitespace);
        for word in text.split_whitespace() {
            self.push(style(div().child(word.to_owned())).into_any_element(), spaced);
            spaced = true;
        }
        self.trailing_space = text.ends_with(char::is_whitespace);
        self
    }

    fn push(&mut self, part: AnyElement, spaced: bool) {
        let glued = !spaced && !self.parts.is_empty();
        self.parts.push(if glued { div().ml(px(-WORD_GAP)).child(part).into_any_element() } else { part });
    }
}

/// The one-line notice every system line shares: its icon in the avatar column, the sentence and its time,
/// then anything that hangs under it (a button, a card).
pub(crate) fn notice_line(id: &str, glyph: Glyph, ink: Hsla, sentence: Sentence, stamp: Option<String>, under: Vec<AnyElement>, cx: &App) -> AnyElement {
    let row = div()
        .id(SharedString::from(format!("msg-{id}")))
        .w_full()
        .px_4()
        .mt_0p5()
        .py_1()
        .flex()
        .gap_4()
        .text_size(ui::TEXT_SM)
        .child(div().w(px(AVATAR)).h(px(22.)).flex_none().flex().items_center().justify_center().child(Icon::new(glyph).size(ui::ICON_LG).text_color(ink)))
        .child(div().flex_1().min_w_0().flex().flex_col().child(sentence_line(sentence, stamp, cx)).children(under));
    chat::inset(row.into_any_element(), false)
}

fn sentence_line(sentence: Sentence, stamp: Option<String>, cx: &App) -> Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x(px(WORD_GAP))
        .line_height(px(22.))
        .text_color(palette::muted(cx))
        .children(sentence.parts)
        .children(stamp.map(|s| div().ml_0p5().whitespace_nowrap().text_size(ui::TEXT_XS).text_color(palette::faint(cx)).child(s)))
}

fn under_row(child: impl IntoElement) -> AnyElement {
    div().mt_2().flex().flex_wrap().gap_2().child(child).into_any_element()
}

fn line(message: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    let (muted, positive, danger) = (palette::muted(cx), palette::positive_ink(cx), palette::danger_ink(cx));
    let facts = &ctx.notices;
    let author = &message.author;
    let s = Sentence::new(&message.id, cx).name(author, ctx);
    let mut under = Vec::new();
    let (glyph, ink, sentence) = match message.kind {
        1 => {
            let target = message.mentions.first().unwrap_or(author);
            (Glyph::ArrowRight, positive, s.text(" added ").name(target, ctx).text(if facts.thread { " to the thread." } else { " to the group." }))
        }
        2 => {
            let target = message.mentions.first().filter(|t| t.id != author.id);
            let sentence = match (facts.thread, target) {
                (true, target) => s.text(" removed ").name(target.unwrap_or(author), ctx).text(" from the thread."),
                (false, Some(target)) => s.text(" removed ").name(target, ctx).text(" from the group."),
                (false, None) => s.text(" left the group."),
            };
            (Glyph::ArrowLeft, danger, sentence)
        }
        // GAP: Discord ends the group lines with an Edit Group link; Waffle has no group editor yet.
        4 if facts.group && message.content.is_empty() => (Glyph::Pen, muted, s.text(" removed the custom group name.")),
        4 if facts.group => (Glyph::Pen, muted, s.text(" changed the group name: ").strong(&message.content).text(".")),
        4 if facts.forum_post => (Glyph::Pen, muted, s.text(" changed the post title: ").strong(&message.content)),
        4 => (Glyph::Pen, muted, s.text(" changed the channel name: ").strong(&message.content)),
        5 => (Glyph::Pen, muted, s.text(" changed the group icon.")),
        7 => return join_line(message, ctx, cx),
        8..=11 => (Glyph::Gem, palette::boost_ink(cx), boost_sentence(message, s, facts)),
        12 => (Glyph::ArrowRight, positive, s.text(" has added ").strong(&message.content).text(" to this channel. Its most important updates will show up here.")),
        14..=17 => return discovery_line(message, ctx, cx),
        18 => (Glyph::Thread, muted, thread_sentence(message, s, ctx)),
        27 => (Glyph::Stage, positive, s.text(" started ").strong(&message.content)),
        28 => (Glyph::Stage, muted, s.text(" ended ").strong(&message.content)),
        29 => (Glyph::Mic, muted, s.text(" is now a speaker.")),
        // GAP: Discord offers Invite to Speak here to members who can mute others while the request is
        // pending; approving it needs the stage's request state, which Waffle doesn't track yet.
        30 => (Glyph::HandWaving, muted, s.text(" requested to speak.")),
        31 => (Glyph::Stage, muted, s.text(" changed the Stage topic: ").strong(&message.content)),
        46 => {
            under = chat::poll_results(message, ctx, cx);
            (Glyph::Poll, muted, poll_sentence(message, s, ctx))
        }
        _ => return nothing(),
    };
    notice_line(&message.id, glyph, ink, sentence, Some(chat::format_stamp(&message.timestamp)), under, cx)
}

fn boost_sentence(message: &Message, s: Sentence, facts: &Facts) -> Sentence {
    let count = message.content.trim().parse::<u64>().ok().filter(|n| *n > 0).unwrap_or(1);
    let s = match count {
        1 => s.text(" just boosted the server!"),
        n => s.text(" just boosted the server ").strong(&n.to_string()).text(" times!"),
    };
    let tier = message.kind - 8;
    match (&facts.guild_name, tier) {
        (Some(guild), 1..) => s.text(&format!(" {guild} has achieved ")).strong(&format!("Level {tier}!")),
        _ => s,
    }
}

fn thread_sentence(message: &Message, s: Sentence, ctx: &Rc<RowContext>) -> Sentence {
    let thread = message.reference_channel.as_ref().and_then(|id| ctx.notices.threads.get(id)).cloned();
    let name = thread.as_ref().map_or_else(|| message.content.clone(), |t| t.name.clone());
    let s = s.text(" started a thread: ");
    // GAP: a thread this client hasn't seen yet shows its name without opening; Discord loads it first.
    let s = match thread {
        Some(thread) => {
            let this = ctx.this();
            let link = s.link(name).on_click(move |_, window, cx| {
                cx.stop_propagation();
                this.update(cx, |this, cx| this.open_channel(thread.clone(), window, cx)).ok();
            });
            s.part(link)
        }
        None => s.strong(&name),
    };
    let (this, parent) = (ctx.this(), message.channel_id.clone());
    let threads = s.link("threads").on_click(move |event, _, cx| {
        cx.stop_propagation();
        let at = event.position();
        this.update(cx, |this, cx| this.open_threads_popout(parent.clone(), at, cx)).ok();
    });
    s.text(". See all ").part(threads).text(".")
}

fn poll_sentence(message: &Message, s: Sentence, ctx: &Rc<RowContext>) -> Sentence {
    let question = message.embeds.iter().find_map(|e| e.poll.as_ref()).map(|p| p.question.as_str()).unwrap_or_default();
    let (this, target) = (ctx.this(), message.reply_to.clone());
    let title = s.link(shorten(question, POLL_TITLE_CHARS)).on_click(move |_, _, cx| {
        cx.stop_propagation();
        let Some(id) = target.clone() else { return };
        this.update(cx, |this, cx| this.jump_to(id, cx)).ok();
    });
    s.text("'s poll ").part(title).text(" has closed.")
}

// Not verified: the ellipsis. Discord truncates the question to 50 characters; whether it marks the cut wasn't read.
fn shorten(text: &str, chars: usize) -> String {
    match text.char_indices().nth(chars) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_owned(),
    }
}

fn discovery_line(message: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    let guild = ctx.notices.guild_name.clone().unwrap_or_else(|| "this server".to_owned());
    let s = Sentence::new(&message.id, cx);
    let (glyph, ink, sentence) = match message.kind {
        14 => {
            let s = s.strong(&guild).text(" has been removed from Server Discovery because it no longer passes all the requirements. Check ");
            let settings = match ctx.notices.server_settings.clone() {
                Some(guild_id) => {
                    let this = ctx.this();
                    let link = s.link("Server Settings").text_color(palette::accent_ink(cx)).on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        this.update(cx, |this, cx| this.open_server_settings(guild_id.clone(), window, cx)).ok();
                    });
                    s.part(link)
                }
                None => s.strong("Server Settings"),
            };
            (Glyph::XCircle, palette::danger_ink(cx), settings.text(" for more details."))
        }
        15 => (Glyph::CheckCircle, palette::positive_ink(cx), s.text("This server is eligible for Server Discovery again and has been automatically relisted!")),
        16 => (
            Glyph::WarningCircle,
            palette::muted(cx),
            s.strong(&guild).text(" has failed Discovery activity requirements for 1 week. If it fails for 4 weeks in a row, it will be automatically removed from Discovery."),
        ),
        _ => (
            Glyph::WarningCircle,
            palette::muted(cx),
            s.strong(&guild).text(" has failed Discovery activity requirements for 3 weeks in a row. If it fails for 1 more week, it will be removed from Discovery."),
        ),
    };
    notice_line(&message.id, glyph, ink, sentence, Some(chat::format_stamp(&message.timestamp)), Vec::new(), cx)
}

fn join_line(message: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    let positive = palette::positive_ink(cx);
    let stamp = Some(chat::format_stamp(&message.timestamp));
    if message.author.bot {
        return notice_line(&message.id, Glyph::ArrowRight, positive, bot_join_sentence(message, ctx, cx), stamp, Vec::new(), cx);
    }
    let under = wave_button(message, ctx, cx).map(under_row).into_iter().collect();
    notice_line(&message.id, Glyph::ArrowRight, positive, join_sentence(message, ctx, cx), stamp, under, cx)
}

fn join_sentence(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> Sentence {
    let (before, after) = join_copy(&message.id);
    Sentence::new(&message.id, cx).text(before).name(&message.author, ctx).text(after)
}

fn bot_join_sentence(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> Sentence {
    let s = Sentence::new(&message.id, cx);
    let app = |s: Sentence| {
        let name = chat::profile_trigger(format!("app-{}", message.id), &message.author, ctx).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).hover(|s| s.underline()).child(ctx.name_of(&message.author));
        let badge = ui::badge(palette::accent_soft(cx), palette::accent_ink(cx)).h(px(18.)).px_1p5().gap(px(3.)).child(Icon::new(Glyph::Checkmark).size(px(11.))).child("APP");
        s.part(div().flex().items_center().gap(px(WORD_GAP)).child(name).child(badge))
    };
    match &message.actor {
        Some(actor) => app(s.name(actor, ctx).text(" has added ")).text(" to the server."),
        None => app(s).text(" was added to the server."),
    }
}

fn wave_button(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> Option<AnyElement> {
    if !ctx.notices.greet || message.author.bot {
        return None;
    }
    let (this, join) = (ctx.this(), message.clone());
    let button = ui::button(SharedString::from(format!("wave-{}", message.id)), ui::Tone::Secondary, cx)
        .small()
        .child(Icon::new(Glyph::Sticker).size(ui::ICON_MD))
        .child("Wave to say hi!")
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            this.update(cx, |this, cx| this.wave(join.clone(), cx)).ok();
        });
    Some(button.into_any_element())
}

// The folded run: the first two names, then "N others", which opens each join's own line in place. No wave
// button on the fold itself: a wave goes to one person.
fn fold_line(run: &[Message], ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    let first = &run[0];
    let others = run.len() - 2;
    let open = ctx.notices.open_runs.contains(&first.id);
    let s = Sentence::new(&first.id, cx).name(&first.author, ctx).text(", ").name(&run[1].author, ctx).text(" and ");
    let (this, id) = (ctx.this(), first.id.clone());
    let toggle = s.link(if others == 1 { "1 other".to_owned() } else { format!("{others} others") }).text_color(palette::accent_ink(cx)).on_click(move |_, _, cx| {
        cx.stop_propagation();
        this.update(cx, |this, cx| this.toggle_join_run(id.clone(), cx)).ok();
    });
    let sentence = s.part(toggle).text(" joined the server.");
    let under = match open {
        false => Vec::new(),
        true => vec![div().mt_1().flex().flex_col().gap_0p5().children(run[2..].iter().map(|m| folded_join(m, ctx, cx))).into_any_element()],
    };
    notice_line(&first.id, Glyph::ArrowRight, palette::positive_ink(cx), sentence, Some(chat::format_stamp(&first.timestamp)), under, cx)
}

fn folded_join(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> AnyElement {
    let wave = wave_button(message, ctx, cx).map(|b| div().mt_1().mb_1().child(b));
    div().flex().flex_col().child(sentence_line(join_sentence(message, ctx, cx), Some(chat::clock(&message.timestamp)), cx)).children(wave).into_any_element()
}

// A thread's first item: the message it was started from, whole, with Jump to go back to it.
fn starter_row(message: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    let Some(parent) = message.reply.as_deref() else {
        let copy = if message.reference_deleted { "Original message was deleted" } else { "Sorry, we couldn't load the first message in this thread" };
        return notice_line(&message.id, Glyph::Thread, palette::muted(cx), Sentence::new(&message.id, cx).text(copy), None, Vec::new(), cx);
    };
    let (this, guild_id, channel_id, id) = (ctx.this(), parent.guild_id.clone().or_else(|| message.guild_id.clone()), parent.channel_id.clone(), parent.id.clone());
    let jump = ui::button(SharedString::from(format!("starter-jump-{}", message.id)), ui::Tone::Secondary, cx).small().child("Jump").on_click(move |_, window, cx| {
        cx.stop_propagation();
        this.update(cx, |this, cx| this.open_message(guild_id.clone(), &channel_id, id.clone(), window, cx)).ok();
    });
    let row = div()
        .id(SharedString::from(format!("msg-{}", message.id)))
        .relative()
        .w_full()
        .px_4()
        .mt_3()
        .pt_1()
        .pb_0p5()
        .child(chat::starter_message(parent, ctx, cx))
        .child(div().absolute().top_1().right_3().child(jump));
    chat::inset(row.into_any_element(), false)
}

// AutoMod's notices look like messages (types 24 and 36 to 39): its shield, its name and a SYSTEM tag, then
// what happened.
fn automod_row(message: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
    let muted = palette::muted(cx);
    let s = Sentence::new(&message.id, cx);
    let body: Vec<AnyElement> = match message.kind {
        24 => automod_action(message, ctx, cx),
        36 => vec![sentence_line(s.name(&message.author, ctx).text(&format!(" enabled security actions until {}.", security_until(&message.content))), None, cx).into_any_element()],
        37 => vec![sentence_line(s.name(&message.author, ctx).text(" disabled security actions."), None, cx).into_any_element()],
        38 => {
            let guild = ctx.notices.guild_name.clone().unwrap_or_default();
            vec![sentence_line(s.name(&message.author, ctx).text(&format!(" reported a raid in {guild}.")), None, cx).into_any_element()]
        }
        _ => vec![sentence_line(s.name(&message.author, ctx).text(" resolved an Activity Alert."), None, cx).into_any_element()],
    };
    let head = div()
        .flex()
        .items_center()
        .gap_2()
        .line_height(px(22.))
        .child(div().text_size(ui::TEXT_MD).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child("AutoMod"))
        .child(ui::badge(palette::accent_soft(cx), palette::accent_ink(cx)).h(px(18.)).px_1p5().child("SYSTEM"))
        .child(div().relative().top_px().whitespace_nowrap().text_size(ui::TEXT_XS).text_color(palette::faint(cx)).child(chat::format_stamp(&message.timestamp)));
    let avatar = div()
        .size(px(AVATAR))
        .flex_none()
        .mt_0p5()
        .rounded_full()
        .bg(palette::oklch(0.5, 0.13, 245.))
        .flex()
        .items_center()
        .justify_center()
        .text_color(gpui_kit::white())
        .child(Icon::new(Glyph::Shield).size(ui::ICON_XL));
    let row = div()
        .id(SharedString::from(format!("msg-{}", message.id)))
        .w_full()
        .px_4()
        .mt_3()
        .pt_1()
        .pb_0p5()
        .flex()
        .gap_4()
        .text_size(ui::TEXT_SM)
        .text_color(muted)
        .child(avatar)
        .child(div().flex_1().min_w_0().flex().flex_col().child(head).children(body));
    chat::inset(row.into_any_element(), false)
}

// Not verified: the format of `content`. Discord passes it to `new Date()`; read here as RFC 3339.
fn security_until(content: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(content).map(|t| t.with_timezone(&chrono::Local).format("%b %-d, %Y, %-I:%M %p").to_string()).unwrap_or_else(|_| content.to_owned())
}

fn automod_field<'a>(message: &'a Message, name: &str) -> Option<&'a str> {
    message.embeds.first()?.fields.iter().find(|(n, _, _)| n == name).map(|(_, v, _)| v.as_str()).filter(|v| !v.is_empty())
}

// A blocked or flagged message: where, what was said with the keyword marked, which rule, and what a
// moderator can do about it.
fn automod_action(message: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> Vec<AnyElement> {
    // GAP: safety alerts (raids, mention raids, activity alerts) get only AutoMod's header; their cards aren't drawn yet.
    if message.embeds.first().is_some_and(|e| e.safety_alert) {
        return Vec::new();
    }
    let field = |name: &str| automod_field(message, name);
    let channel_id = field("channel_id").map(str::to_owned);
    let channel = channel_id.as_ref().and_then(|id| ctx.channel_names.get(id)).map_or("unknown-channel".to_owned(), |name| name.clone());
    // Not verified: the outcome's values; anything but "flagged" reads as blocked.
    let verb = if field("decision_outcome") == Some("flagged") { "has flagged a message in" } else { "has blocked a message in" };
    // GAP: app messages, modals, voice channel statuses, nicknames, quarantines and guests have their own headers.
    let header = sentence_line(Sentence::new(&message.id, cx).text(verb).strong(&format!("#{channel}")), None, cx);
    let flagged = message.embeds.first().and_then(|e| e.description.clone()).unwrap_or_default();
    let meta: Vec<String> = [("Keyword", field("keyword").map(str::to_owned)), ("Rule", field("rule_name").map(str::to_owned)), ("Timeout", field("timeout_duration").and_then(timeout_label)), ("Reason", field("decision_reason").map(str::to_owned))]
        .into_iter()
        .filter_map(|(label, value)| Some(format!("{label}: {}", value?)))
        .collect();
    let mut out = vec![header.into_any_element(), flagged_card(message, &flagged, field("keyword_matched_content"), ctx, cx)];
    if !meta.is_empty() {
        out.push(div().mt_1p5().text_size(ui::TEXT_XS).child(meta.join(" · ")).into_any_element());
    }
    out.push(automod_actions(message, channel_id, field("flagged_message_id").map(str::to_owned), ctx, cx));
    out
}

fn flagged_card(message: &Message, text: &str, matched: Option<&str>, ctx: &Rc<RowContext>, cx: &App) -> AnyElement {
    let strong = palette::strong(cx);
    let mark = HighlightStyle { background_color: Some(palette::warning_ink(cx).opacity(0.26)), color: Some(strong), ..Default::default() };
    let highlights = matched.and_then(|m| keyword_range(text, m)).map(|r| vec![(r, mark)]).unwrap_or_default();
    div()
        .mt_1p5()
        .py_2p5()
        .px_3()
        .squircle(RADIUS_MD)
        .bg(strong.opacity(0.05))
        .flex()
        .gap_2p5()
        .child(ui::avatar(message.author.avatar.clone(), 24.).flex_none())
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().font_weight(FontWeight::SEMIBOLD).text_color(strong).child(ctx.name_of(&message.author)))
                .child(div().text_color(palette::text(cx)).child(StyledText::new(text.to_owned()).with_highlights(highlights))),
        )
        .into_any_element()
}

// Where the matched words sit in the flagged text, ignoring case.
fn keyword_range(text: &str, matched: &str) -> Option<std::ops::Range<usize>> {
    let at = text.to_ascii_lowercase().find(&matched.to_ascii_lowercase())?;
    Some(at..at + matched.len())
}

// Not verified: the unit. Discord's `timeout_duration` field is read as seconds.
fn timeout_label(seconds: &str) -> Option<String> {
    let seconds: u64 = seconds.parse().ok()?;
    let (n, unit) = match seconds {
        0..60 => (seconds, "sec"),
        60..3600 => (seconds / 60, "min"),
        3600..86400 => (seconds / 3600, "hour"),
        _ => (seconds / 86400, "day"),
    };
    Some(format!("{n} {unit}{}", if n == 1 { "" } else { "s" }))
}

fn automod_actions(message: &Message, channel_id: Option<String>, flagged_id: Option<String>, ctx: &Rc<RowContext>, cx: &App) -> AnyElement {
    let key = &message.id;
    let (this, author) = (ctx.this(), message.author.clone());
    let actions = ui::button(SharedString::from(format!("automod-actions-{key}")), ui::Tone::Secondary, cx).small().child(Icon::new(Glyph::Shield).size(ui::ICON_MD)).child("Actions").on_click(move |event, _, cx| {
        cx.stop_propagation();
        let at = event.position();
        this.update(cx, |this, cx| this.open_author_menu(author.clone(), at, cx)).ok();
    });
    let target = channel_id.zip(flagged_id);
    let jump = target.clone().map(|(channel_id, id)| {
        let (this, guild_id) = (ctx.this(), message.guild_id.clone());
        ui::button(SharedString::from(format!("automod-jump-{key}")), ui::Tone::Secondary, cx).small().child("Jump to Surrounding").on_click(move |_, window, cx| {
            cx.stop_propagation();
            this.update(cx, |this, cx| this.open_message(guild_id.clone(), &channel_id, id.clone(), window, cx)).ok();
        })
    });
    // GAP: Discord hides Delete once `alert_actions_execution` says the message is gone; that field isn't read yet.
    let delete = target.filter(|(channel_id, _)| ctx.notices.moderated.contains(channel_id)).map(|(channel_id, id)| {
        let this = ctx.this();
        ui::button(SharedString::from(format!("automod-delete-{key}")), ui::Tone::Secondary, cx).small().child("Delete User's Message").on_click(move |_, _, cx| {
            cx.stop_propagation();
            let flagged = Message { id: id.clone(), channel_id: channel_id.clone(), ..Default::default() };
            this.update(cx, |this, cx| this.delete(flagged, cx)).ok();
        })
    });
    div().mt_2().flex().flex_wrap().gap_2().child(actions).children(jump).children(delete).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(id: &str, kind: u8, bot: bool, timestamp: &str) -> Message {
        let mut author = User { id: format!("u{id}"), ..Default::default() };
        author.bot = bot;
        Message { id: id.into(), kind, author, timestamp: timestamp.into(), ..Default::default() }
    }

    #[test]
    fn join_lines_follow_the_snowflake_timestamp() {
        // (1305842396315832330 >> 22) + 1420070400000 = 1731407488660, which is 5 mod 13.
        assert_eq!(snowflake_ms("1305842396315832330"), 1_731_407_488_660);
        assert_eq!(join_copy("1305842396315832330"), ("", " just slid into the server."));
        assert_eq!(snowflake_ms("not a snowflake"), 0);
    }

    #[test]
    fn three_joins_in_a_row_fold_and_two_do_not() {
        let at = "2026-11-20T12:00:00Z";
        let join = |id: &str| message(id, 7, false, at);
        let two = [join("1"), join("2"), message("3", 0, false, at)];
        assert_eq!((0..3).map(|i| join_fold(&two, i)).collect::<Vec<_>>(), [Fold::None, Fold::None, Fold::None]);
        let three = [join("1"), join("2"), join("3")];
        assert_eq!((0..3).map(|i| join_fold(&three, i)).collect::<Vec<_>>(), [Fold::Head(3), Fold::Tail, Fold::Tail]);
        let broken = [join("1"), join("2"), message("3", 0, false, at), join("4"), join("5")];
        assert!((0..5).all(|i| join_fold(&broken, i) == Fold::None));
        let bot = [join("1"), join("2"), message("3", 7, true, at), join("4")];
        assert!((0..4).all(|i| join_fold(&bot, i) == Fold::None));
    }

    #[test]
    fn unknown_system_types_draw_nothing() {
        let kind = |kind: u8| shape(&message("1", kind, false, ""));
        assert_eq!(kind(0), Shape::Message);
        assert_eq!(kind(19), Shape::Message);
        assert_eq!(kind(7), Shape::Line);
        assert_eq!(kind(24), Shape::AutoMod);
        assert_eq!(kind(21), Shape::ThreadStarter);
        for hidden in [13, 22, 25, 40, 44, 55, 63, 99] {
            assert_eq!(kind(hidden), Shape::Hidden, "type {hidden}");
            assert!(is_notice(&message("1", hidden, false, "")));
        }
    }

    #[test]
    fn poll_titles_and_keywords() {
        assert_eq!(shorten("short", 50), "short");
        assert_eq!(shorten(&"a".repeat(60), 50), format!("{}…", "a".repeat(50)));
        assert_eq!(keyword_range("free nitro at Discord-Gift.ru hurry", "discord-gift"), Some(14..26));
        assert_eq!(timeout_label("600").as_deref(), Some("10 mins"));
        assert_eq!(timeout_label("3600").as_deref(), Some("1 hour"));
    }
}
