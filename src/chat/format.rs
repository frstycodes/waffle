use super::*;

/// Discord's wire syntax made readable: mentions become links the transcript styles and routes,
/// custom emoji become inline images at Discord's 1.375em (animated ones play).
pub(super) fn readable(content: &str, ctx: &RowContext) -> String {
    readable_with(content, ctx, true)
}

// `inline_emoji` false keeps custom emoji as `:name:`, for one-line plain text such as reply quotes.
pub(super) fn readable_with(content: &str, ctx: &RowContext, inline_emoji: bool) -> String {
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let Some(end) = tail.find('>') else {
            out.push_str(tail);
            return out;
        };
        let token = &tail[1..end];
        let replacement = if let Some(id) = token.strip_prefix("@&") {
            Some(ctx.role_names.get(id).map_or_else(|| "@unknown-role".to_owned(), |n| format!("[@{n}](waffle://role/{id})")))
        } else if let Some(id) = token.strip_prefix("@!").or_else(|| token.strip_prefix('@')) {
            Some(format!("[@{}](waffle://user/{id})", ctx.names.get(id).map_or("unknown-user", String::as_str)))
        } else if let Some(id) = token.strip_prefix('#') {
            ctx.channel_names.get(id).map(|n| format!("[#{n}](waffle://channel/{id})"))
        } else if let Some(stamp) = token.strip_prefix("t:") {
            discord_timestamp(stamp)
        } else if let Some(name) = token.strip_prefix("a:").or_else(|| token.strip_prefix(':')) {
            match (inline_emoji, emoji_image(token, ctx.media.emoji)) {
                (true, Some(html)) => Some(html),
                _ => name.split(':').next().map(|n| format!(":{n}:")),
            }
        } else {
            None
        };
        match replacement {
            Some(r) => out.push_str(&r),
            None => out.push_str(&tail[..=end]),
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    let out = crate::link_preview::channel_chips(&out, &ctx.channel_names);
    // ponytail: plain word match, like the tokens above it also matches inside code spans.
    out.replace("@everyone", "[@everyone](waffle://everyone)").replace("@here", "[@here](waffle://here)")
}

// A message's text without markdown, mentions named: for surfaces that set their own ink.
pub(crate) fn plain_text(content: &str, ctx: &RowContext) -> String {
    plain(&readable_with(content, ctx, false))
}

// One line of plain text for the reply quote: markdown markers and link targets dropped.
pub(super) fn plain(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    let (mut skipping_target, mut after_bracket) = (false, false);
    for c in markdown.chars() {
        match c {
            '(' if after_bracket => skipping_target = true,
            ')' if skipping_target => skipping_target = false,
            _ if skipping_target => {}
            '*' | '_' | '`' | '~' | '[' | ']' => {}
            '\n' => out.push(' '),
            _ => out.push(c),
        }
        after_bracket = c == ']';
    }
    out
}

// `<t:1731590400:R>` and friends, rendered in the reader's time zone like Discord does.
pub(super) fn discord_timestamp(stamp: &str) -> Option<String> {
    let (secs, style) = stamp.split_once(':').unwrap_or((stamp, "f"));
    let time = DateTime::from_timestamp(secs.parse().ok()?, 0)?.with_timezone(&Local);
    Some(format!("`{}`", match style {
        "t" => time.format("%H:%M").to_string(),
        "T" => time.format("%H:%M:%S").to_string(),
        "d" => time.format("%d/%m/%Y").to_string(),
        "D" => time.format("%-d %B %Y").to_string(),
        "F" => time.format("%A, %-d %B %Y %H:%M").to_string(),
        "R" => relative(Local::now().signed_duration_since(time)),
        _ => time.format("%-d %B %Y %H:%M").to_string(),
    }))
}

pub(crate) fn relative(delta: chrono::TimeDelta) -> String {
    let (secs, future) = (delta.num_seconds().abs(), delta.num_seconds() < 0);
    let (n, unit) = match secs {
        s if s < 60 => (s, "second"),
        s if s < 3600 => (s / 60, "minute"),
        s if s < 86_400 => (s / 3600, "hour"),
        s if s < 2_592_000 => (s / 86_400, "day"),
        s if s < 31_536_000 => (s / 2_592_000, "month"),
        s => (s / 31_536_000, "year"),
    };
    let unit = if n == 1 { unit.to_string() } else { format!("{unit}s") };
    if future { format!("in {n} {unit}") } else { format!("{n} {unit} ago") }
}

// Custom emoji as inline images and nothing else changed: markdown outside a message (a profile bio).
pub(crate) fn inline_emoji(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        let tail = &rest[start..];
        let Some(end) = tail.find('>') else { break };
        out.push_str(&rest[..start]);
        match emoji_image(&tail[1..end], true) {
            Some(html) => out.push_str(&html),
            None => out.push_str(&tail[..=end]),
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

// `a:name:id` or `:name:id` as an inline image the markdown view lays out in the text flow. Its
// images carry element ids, so animated emoji play unless `animate` is off. 22px is Discord's 1.375em at 16px text.
pub(super) fn emoji_image(token: &str, animate: bool) -> Option<String> {
    let animated = animate && token.starts_with("a:");
    let mut parts = token.trim_start_matches("a:").trim_start_matches(':').split(':');
    let (name, id) = (parts.next()?, parts.next()?);
    if parts.next().is_some() || name.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let url = Emoji { id: Some(id.to_owned()), name: name.to_owned(), animated }.url()?;
    Some(format!(r#"<img src="{url}" alt=":{name}:" width="22" height="22" />"#))
}

pub(super) fn custom_emoji(content: &str) -> Vec<Emoji> {
    let mut out = Vec::new();
    let mut rest = content;
    while let Some(start) = rest.find('<') {
        let tail = &rest[start..];
        let Some(end) = tail.find('>') else { break };
        let token = &tail[1..end];
        let animated = token.starts_with("a:");
        let parts: Vec<&str> = token.trim_start_matches("a:").trim_start_matches(':').split(':').collect();
        if (token.starts_with(':') || animated) && parts.len() == 2 {
            out.push(Emoji { id: Some(parts[1].to_owned()), name: parts[0].to_owned(), animated });
        }
        rest = &tail[end + 1..];
    }
    out
}

pub(super) fn strip_emoji(content: &str) -> String {
    let mut out = String::new();
    let mut rest = content;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let Some(end) = tail.find('>') else { break };
        let token = &tail[1..end];
        if !(token.starts_with(':') || token.starts_with("a:")) {
            out.push_str(&tail[..=end]);
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

// Who is typing, names emphasised: "Mira is typing…", "Mira and Theo are typing…", then "Several people".
pub(super) fn typing_parts(names: &[&str]) -> Vec<(String, bool)> {
    let name = |n: &str| (n.to_owned(), true);
    let word = |w: &str| (w.to_owned(), false);
    match names {
        [] => Vec::new(),
        [one] => vec![name(one), word(" is typing…")],
        [a, b] => vec![name(a), word(" and "), name(b), word(" are typing…")],
        [a, b, c] => vec![name(a), word(", "), name(b), word(" and "), name(c), word(" are typing…")],
        _ => vec![word("Several people are typing…")],
    }
}

// A strip at the top of the composer: replying, editing, or a send waiting out slowmode.
pub(crate) fn bar_row(cx: &App) -> Div {
    ui::row()
        .min_h_10()
        .py_1()
        .pl_3p5()
        .pr_1p5()
        .gap_1p5()
        .border_b_1()
        .border_color(palette::line(cx))
        .text_size(ui::TEXT_SM)
        .text_color(palette::muted(cx))
}

// A reply or queued bar's label: it takes the bar's room and truncates. It carries bold runs, which ui::line's plain text can't.
pub(crate) fn bar_label(label: StyledText) -> Div {
    // lint-allow(raw-truncate): ui::line takes plain text, and this label carries bold runs
    div().min_w_0().truncate().child(label).flex_1()
}

// A keycap and what it does, for the edit bar.
// A suggestion's face: an avatar, a channel's #, an emoji.
pub(crate) fn suggest_lead(lead: &crate::SuggestLead, cx: &App) -> AnyElement {
    match lead {
        crate::SuggestLead::Avatar(url) => ui::avatar(url.clone(), 28.).into_any_element(),
        crate::SuggestLead::Channel => ui::row().size_7().flex_none().rounded_full().bg(palette::selected(cx)).text_color(palette::muted(cx)).justify_center().child(Icon::new(Glyph::Hashtag).size(ui::ICON_MD)).into_any_element(),
        crate::SuggestLead::Emoji(emoji) => ui::row().size_7().flex_none().justify_center().child(crate::menu::emoji_face(emoji, 22.)).into_any_element(),
        crate::SuggestLead::Command(glyph) => ui::row().size_7().flex_none().justify_center().text_color(palette::muted(cx)).child(Icon::new(*glyph).size(ui::ICON_XL)).into_any_element(),
    }
}

// A placeholder name bar's width and the bars' corner.
const SKELETON_NAME: f32 = 120.;
const SKELETON_RADIUS: f32 = 6.;
// The unread rule's stub under the NEW tag: how far it runs and where it sits down the tag.
const UNREAD_STUB: f32 = 9.;
const UNREAD_STUB_TOP: f32 = 7.5;

// Placeholders shaped like the real transcript.
pub(super) fn skeleton(cx: &App) -> AnyElement {
    let row = |(i, w): (usize, f32)| {
        ui::row()
            .items_stretch()
            .gap_4()
            .child(ui::skeleton_circle(("skeleton-avatar", i), AVATAR, cx))
            .child(
                ui::col().min_h_0()
                    .flex_1()
                    .gap_2()
                    .pt_1()
                    .child(ui::skeleton(("skeleton-name", i), SKELETON_RADIUS, cx).w(px(SKELETON_NAME)).h_3())
                    .child(ui::skeleton(("skeleton-line", i), SKELETON_RADIUS, cx).w(px(w)).h_3()),
            )
    };
    ui::col()
        .size_full()
        .px_6()
        .pb_4()
        .justify_end()
        .gap_6()
        .children([360., 220., 420., 280.].into_iter().enumerate().map(row))
        .into_any_element()
}

// One accent line where unread starts, ending in a New badge.
// Discord's unread divider (design/unread-line.html): a red rule into a NEW tag whose point runs back along
// it. The tag is one shape (`icons::NEW_TAG`), NEW centred on its body, clear of the point.
pub(super) fn unread_line() -> AnyElement {
    let alert = palette::alert();
    let tag = div()
        .relative()
        .flex_none()
        .w_10()
        .h_4()
        // The rule runs on under the tag to its body, so it meets the sharp tip at full thickness.
        // lint-allow(custom-divider): ui::inset_rule runs along an edge in the neutral line colour; this stub carries the alert rule on under the NEW tag
        .child(div().absolute().left_neg_px().w(px(UNREAD_STUB)).top(px(UNREAD_STUB_TOP)).h(ui::HAIRLINE).bg(alert))
        .child(gpui_kit::svg().path(crate::icons::NEW_TAG).absolute().inset_0().size_full().text_color(alert))
        .child(ui::row().absolute().top_0().bottom_0().left_2().right_0().justify_center().text_color(gpui_kit::white()).text_size(ui::TEXT_XXS).font_weight(FontWeight::BOLD).child("NEW"));
    let row = ui::row().w_full().px_4().child(rule(alert)).child(tag);
    div().py_2().child(row).into_any_element()
}

// One rule running to its neighbour: a day divider's line, or the unread line in alert red.
fn rule(color: Hsla) -> Div {
    // lint-allow(custom-divider): ui::divider takes a 'static label in the neutral line colour, and these two take a formatted date or the alert colour
    div().flex_1().h(ui::HAIRLINE).bg(color)
}

// Between dates, inside the feed's gutters, never edge to edge.
// lint-allow(custom-component): ui::divider takes a 'static label, and a day's label is formatted from the message's date
pub(super) fn day_divider(timestamp: &str, cx: &App) -> AnyElement {
    let line = || rule(palette::line(cx));
    let row = ui::row()
        .w_full()
        .px_4()
        .gap_2p5()
        .whitespace_nowrap()
        .text_size(ui::TEXT_XS)
        .font_weight(FontWeight::MEDIUM)
        .text_color(palette::faint(cx))
        .child(line())
        .child(day(timestamp).map(|d| d.format("%A, %B %-d, %Y").to_string()).unwrap_or_default())
        .child(line());
    div().pt_4().pb_2().child(row).into_any_element()
}

pub(super) fn empty_state(id: &'static str, art: impl Into<ui::Art>, title: &'static str, detail: impl Into<SharedString>, cx: &App) -> AnyElement {
    ui::row().size_full().justify_center().child(ui::empty_state(id, art, title, detail, cx)).into_any_element()
}

pub(super) fn continues(previous: &Message, message: &Message) -> bool {
    let (Ok(a), Ok(b)) = (DateTime::parse_from_rfc3339(&previous.timestamp), DateTime::parse_from_rfc3339(&message.timestamp)) else {
        return false;
    };
    // An app's answer to a command keeps its own header and "used /command" line.
    let answers_command = message.interaction.as_ref().is_some_and(|i| i.command.is_some());
    previous.author.id == message.author.id && !answers_command && (b - a).num_seconds() < GROUP_GAP_SECS
}

pub(crate) fn local(timestamp: &str) -> Option<DateTime<Local>> {
    Some(DateTime::parse_from_rfc3339(timestamp).ok()?.with_timezone(&Local))
}

pub(crate) fn day(timestamp: &str) -> Option<NaiveDate> {
    Some(local(timestamp)?.date_naive())
}

// Settings › Chat › Use a 24-hour clock. A global, not a parameter: the formatters sit deep under
// rendering code that has no prefs to hand.
pub(super) static CLOCK_24: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn set_clock_24(on: bool) {
    CLOCK_24.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub(super) fn time_format(gutter: bool) -> &'static str {
    match (CLOCK_24.load(std::sync::atomic::Ordering::Relaxed), gutter) {
        (true, _) => "%H:%M",
        (false, false) => "%-I:%M %p",
        (false, true) => "%-I:%M",
    }
}

// "Today at 9:41 PM", "Yesterday at 11:58 PM", the weekday within the week, then the date.
pub(crate) fn format_stamp(timestamp: &str) -> String {
    let Some(time) = local(timestamp) else { return String::new() };
    let clock = time.format(time_format(false));
    match (Local::now().date_naive() - time.date_naive()).num_days() {
        0 => format!("Today at {clock}"),
        1 => format!("Yesterday at {clock}"),
        2..=6 => format!("{} at {clock}", time.format("%A")),
        _ => format!("{} at {clock}", time.format("%B %-d, %Y")),
    }
}

// "9:41 PM", or "21:41".
// Discord's bar text: "21 new messages since 4:08 PM", "50+ new messages since…" when not all are loaded,
// and " on November 3, 2026" after the time when the first unread is from another day.
pub(super) fn unread_label(count: usize, estimated: bool, since: &str) -> String {
    let messages = match (estimated, count) {
        (true, n) => format!("{n}+ new messages"),
        (false, 1) => "1 new message".to_owned(),
        (false, n) => format!("{n} new messages"),
    };
    let today = chrono::Local::now().date_naive();
    match day(since).filter(|d| *d != today) {
        Some(d) => format!("{messages} since {} on {}", clock(since), d.format("%B %-d, %Y")),
        None => format!("{messages} since {}", clock(since)),
    }
}

pub(crate) fn clock(timestamp: &str) -> String {
    local(timestamp).map(|t| t.format(time_format(false)).to_string()).unwrap_or_default()
}

// The continuation gutter's "9:41": the half of the day is the header's to say.
pub(super) fn gutter_clock(timestamp: &str) -> String {
    local(timestamp).map(|t| t.format(time_format(true)).to_string()).unwrap_or_default()
}

pub(crate) fn format_size(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.0} KB", b as f64 / (1 << 10) as f64),
        b => format!("{b} bytes"),
    }
}

// A breadcrumb back up the hierarchy: muted until hovered.
pub(super) fn crumb(id: &'static str, cx: &App) -> Stateful<Div> {
    let (hover, strong) = (palette::hover(cx), palette::strong(cx));
    // lint-allow(custom-button): ui::button has no breadcrumb look, muted ink that strengthens on hover and takes the surrounding type
    ui::row()
        .id(id)
        .occlude()
        .h_8()
        .px_1p5()
        .gap_1p5()
        .squircle(RADIUS_SM)
        .cursor_pointer()
        .whitespace_nowrap()
        .font_weight(FontWeight::NORMAL)
        .text_color(palette::muted(cx))
        .hover(move |s| s.bg(hover).text_color(strong))
}
