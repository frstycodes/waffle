use super::*;

// Discord markdown outside a message's own content (a Components V2 text block), with its mentions, its
// body at `size`.
pub(crate) fn rich_text(id: &str, text: &str, size: f32, ctx: &RowContext, cx: &App) -> AnyElement {
    ui::col().w_full().children(text_blocks(id, &readable(text, ctx), size, ctx, cx)).into_any_element()
}

// A message body's markdown in order, its "-# " subtext lines small and muted between the rest.
pub(super) fn text_blocks(id: &str, text: &str, body: f32, ctx: &RowContext, cx: &App) -> Vec<AnyElement> {
    split_subtext(text)
        .into_iter()
        .enumerate()
        .map(|(i, (subtext, part))| {
            let size = if subtext { SUBTEXT } else { body };
            // Capped at the row's width so long lines wrap instead of running off the side.
            // A code block spans the message's width, as Discord's do; plain text wraps to its own.
            let block = div().min_w_0().max_w_full().when(part.contains("```"), |d| d.w_full());
            let block = block.when(subtext, |d| d.text_size(px(SUBTEXT)).text_color(palette::muted(cx)));
            block.child(markdown(SharedString::from(format!("{id}-{i}")), part, size, ctx)).into_any_element()
        })
        .collect()
}

// Mentions, role and channel links draw as badges (messages.css `.at`): accent ink on the soft fill.
pub(super) struct MentionBadges {
    this: WeakEntity<Waffle>,
    role_inks: std::sync::Arc<HashMap<String, Hsla>>,
    fill: Hsla,
    hover: Hsla,
    ink: Hsla,
}

#[derive(Clone)]
pub(super) struct Mention {
    url: String,
    label: String,
}

impl MarkdownPlugin for MentionBadges {
    fn name(&self) -> &str {
        "mention"
    }

    fn parse(&self, node: &markdown_ast::Node, _: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let markdown_ast::Node::Link(link) = node else { return None };
        if !link.url.starts_with("waffle://") {
            return None;
        }
        let label = node.to_string();
        Some(MarkdownNode::new("mention", Mention { url: link.url.clone(), label: label.clone() }).text(label))
    }

    fn render_inline(&self, node: &MarkdownNode, _: &InlineRenderContext, _: &mut Window, _: &mut App) -> Option<InlineElement> {
        let mention = node.data::<Mention>()?.clone();
        // Discord tints a role mention with the role's colour (not verified: the fill's alpha; 10% here, 20% on hover).
        let role = mention.url.strip_prefix("waffle://role/").and_then(|id| self.role_inks.get(id)).copied();
        let (fill, ink, hover) = role.map_or((self.fill, self.ink, self.hover), |ink| (ink.opacity(0.1), ink, ink.opacity(0.2)));
        let this = self.this.clone();
        let bounds = Rc::new(Cell::new(None));
        let measured = bounds.clone();
        let badge = mention_badge(mention.label.into(), fill, ink)
            .id(SharedString::from(format!("at-{}", mention.url)))
            .relative()
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .child(gpui_kit::canvas(move |b, _, _| measured.set(Some(b)), |_, _, _, _| {}).absolute().size_full())
            .on_click(move |event, window, cx| { cx.stop_propagation();
                let url = mention.url.clone();
                let beside = bounds.get().map_or(event.position(), crate::role_popout::beside);
                this.update(cx, |this, cx| this.open_mention(&url, event.position(), beside, window, cx)).ok();
            });
        Some(InlineElement::new(badge))
    }
}

// Inline marks: a mention's side padding, an option key's side padding and rounded edge, a server tag's height and gap.
const MENTION_PAD: f32 = 2.;
const OPTION_KEY_PAD: f32 = 7.;
const OPTION_EDGE: f32 = 5.;
const TAG_HEIGHT: f32 = 16.;

// A mention as messages draw it; the composer's mention chips wear the same.
// lint-allow(custom-component): ui::badge is a 20px block, and a mention must run inside a line of text at its size
pub(super) fn mention_badge(label: SharedString, fill: Hsla, ink: Hsla) -> Div {
    div().px(px(MENTION_PAD)).rounded_sm().bg(fill).text_color(ink).font_weight(FontWeight::MEDIUM).whitespace_nowrap().child(label)
}

// The composer's tokens: mentions in the message badge, a slash command's option names as a quieter
// chip, and the blank end of each option's value. The hover fill marks one inside the selection.
pub(super) fn composer_tokens(cx: &App) -> impl Fn(&gpui_kit::component::input::InlineTokenContext, &mut Window, &mut App) -> Div + 'static {
    let (fill, selected, ink) = (palette::accent_soft(cx), palette::accent().opacity(0.26), palette::accent_ink(cx));
    let (key_fill, key_selected, key_ink) = (palette::selected(cx), palette::accent_soft(cx), palette::muted(cx));
    move |token, _, _| {
        let chip = token.token();
        if crate::command_picker::is_end_token(chip) {
            return div().w(px(crate::command_picker::END_CHIP)).h(token.line_height());
        }
        if crate::command_picker::is_option_token(chip) {
            let fill = if token.is_selected() { key_selected } else { key_fill };
            let name = chip.text().trim_end_matches(':').to_owned();
            return option_key(name.into(), token.line_height(), fill, key_ink);
        }
        mention_badge(chip.label().clone(), if token.is_selected() { selected } else { fill }, ink).h(token.line_height()).flex().items_center()
    }
}

// An option name in a typed slash command: the muted left segment of the option's box, which
// `Waffle::option_boxes` strokes around it and the value typed after it.
pub(super) fn option_key(label: SharedString, height: Pixels, fill: Hsla, ink: Hsla) -> Div {
    let key = ui::row().h_full().px(px(OPTION_KEY_PAD)).rounded_l(px(OPTION_EDGE)).bg(fill).text_color(ink).text_size(ui::TEXT_DETAIL).font_weight(FontWeight::SEMIBOLD).whitespace_nowrap().child(label);
    ui::row().items_stretch().h(height).pl(px(crate::command_picker::OPTION_GAP)).pr(px(OPTION_EDGE)).child(key)
}

// Discord markdown with mentions, channel links and code copy buttons; messages and embeds share it.
pub(super) fn markdown(id: SharedString, text: String, body: f32, ctx: &RowContext) -> TextView {
    let this = ctx.this.clone();
    let (copy_target, copied) = (ctx.this.clone(), ctx.copied.clone());
    let badges = MentionBadges { this: ctx.this.clone(), role_inks: ctx.role_inks.clone(), fill: ctx.mention_fill, hover: ctx.mention_hover, ink: ctx.mention_ink };
    let text = discord_underline(&discord_quote_rest(&text));
    let text = if ctx.underline_links { underline_links(&text) } else { text };
    TextView::markdown(id, discord_breaks(&text))
        // Constant: the plugin only changes how mentions draw, never what parses.
        .markdown_extensions(MarkdownExtensions::default().parser_revision(1).plugin(badges))
        // messages.css `.codeblock { margin-top: 6px }`: a block opening a message clears the name row.
        // lint-allow(raw-margin): the text view's code-block style takes the design's `.codeblock` top margin, it has no parent gap to carry it
        .style(gpui_kit::component::text::TextViewStyle { heading_base_font_size: px(body), ..Default::default() }.code_block(gpui_kit::StyleRefinement::default().mt_1p5()).heading_font_size(heading_size))
        .selectable(true)
        .code_block_actions(move |block, _, cx| copy_button(block.code().to_string(), copied.as_deref() == Some(block.code().as_ref()), copy_target.clone(), cx))
        .on_link_click(move |url, _, window, cx| {
            let url = url.to_string();
            if let Some(channel_id) = url.strip_prefix("waffle://channel/") {
                this.update(cx, |this, cx| {
                    let channel = this.guild().and_then(|g| g.channels.iter().find(|c| c.id == channel_id).cloned());
                    if let Some(channel) = channel {
                        this.open_channel(channel, window, cx);
                    }
                })
                .ok();
                return;
            }
            if !url.starts_with("waffle://") {
                this.update(cx, |this, cx| this.open_link(&url, window, cx)).ok();
            }
        })
}

// Discord keeps every line break and a line's leading spaces; markdown joins single breaks into a
// space and drops indentation. Each break becomes a hard break and leading spaces non-breaking ones,
// outside fenced code, which already keeps both.
pub(crate) fn discord_breaks(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 8);
    let mut in_fence = false;
    let mut lines = text.split('\n').peekable();
    while let Some(line) = lines.next() {
        let fence = line.trim_start().starts_with("```");
        if in_fence || fence {
            out.push_str(line);
        } else {
            let indent = line.len() - line.trim_start_matches(' ').len();
            out.extend(std::iter::repeat_n('\u{a0}', indent));
            out.push_str(&line[indent..]);
        }
        if fence {
            in_fence = !in_fence;
        }
        let Some(next) = lines.peek() else { break };
        let breaks = !in_fence && !fence && !line.trim().is_empty() && !next.trim().is_empty() && !next.trim_start().starts_with("```");
        out.push_str(if breaks { "  \n" } else { "\n" });
    }
    out
}

// Discord's heading sizes against the body text: # 1.5×, ## 1.25×, ### the body's own size.
pub(super) fn heading_size(level: u8, body: Pixels) -> Pixels {
    match level {
        1 => body * 1.5,
        2 => body * 1.25,
        _ => body,
    }
}

// Runs of body and of "-# " subtext lines, in order (true marks subtext, its marker dropped); fenced
// code stays body.
pub(super) fn split_subtext(text: &str) -> Vec<(bool, String)> {
    let mut runs: Vec<(bool, String)> = Vec::new();
    let mut in_fence = false;
    for line in text.split('\n') {
        let subtext = line.strip_prefix("-# ").filter(|_| !in_fence);
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
        let (is_sub, line) = subtext.map_or((false, line), |rest| (true, rest));
        match runs.last_mut() {
            Some((kind, run)) if *kind == is_sub => {
                run.push('\n');
                run.push_str(line);
            }
            _ => runs.push((is_sub, line.to_owned())),
        }
    }
    runs.retain(|(_, run)| !run.trim().is_empty());
    runs
}

// Always underline links: each web link wrapped in `<u>`, since the markdown view never underlines a link
// itself. A bare address becomes `<url>`, which autolinks after the tag where a bare one might not.
// Mentions and emoji, links too, are left alone.
pub(super) fn underline_links(markdown: &str) -> String {
    outside_code(markdown, |text| {
        let mut out = String::with_capacity(text.len() + 16);
        let mut rest = text;
        while let Some((start, end, bare)) = next_link(rest) {
            out.push_str(&rest[..start]);
            let link = &rest[start..end];
            match bare {
                true => out.push_str(&format!("<u><{link}></u>")),
                false => out.push_str(&format!("<u>{link}</u>")),
            }
            rest = &rest[end..];
        }
        out.push_str(rest);
        out
    })
}

// Discord's `__text__` underlines; CommonMark would make it bold.
pub(super) fn discord_underline(text: &str) -> String {
    outside_code(text, |part| {
        let mut out = String::with_capacity(part.len());
        let mut rest = part;
        while let Some(start) = rest.find("__") {
            let inner = &rest[start + 2..];
            match inner.find("__").filter(|&end| !inner[..end].trim().is_empty() && !inner[..end].contains('\n')) {
                Some(end) => {
                    out.push_str(&rest[..start]);
                    out.push_str(&format!("<u>{}</u>", &inner[..end]));
                    rest = &inner[end + 2..];
                }
                None => {
                    out.push_str(&rest[..start + 2]);
                    rest = inner;
                }
            }
        }
        out.push_str(rest);
        out
    })
}

// Discord's `>>> `: everything from that line to the end of the message is one quote.
pub(super) fn discord_quote_rest(text: &str) -> String {
    let mut fenced = false;
    let mut out: Vec<String> = Vec::new();
    let mut lines = text.split('\n');
    while let Some(line) = lines.next() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
        }
        if let Some(first) = line.strip_prefix(">>> ").filter(|_| !fenced) {
            out.push(format!("> {first}"));
            out.extend(lines.map(|l| format!("> {l}")));
            break;
        }
        out.push(line.to_owned());
    }
    out.join("\n")
}

// The next web link in `text`: its byte range and whether it's a bare address. A masked `[label](url)`
// and an `<url>` keep their own syntax; a bare one ends at whitespace, less trailing punctuation.
pub(super) fn next_link(text: &str) -> Option<(usize, usize, bool)> {
    let mut from = 0;
    loop {
        let at = from + text[from..].find("http")?;
        let after = &text[at..];
        from = at + 4;
        if !(after.starts_with("https://") || after.starts_with("http://")) {
            continue;
        }
        let before = &text[..at];
        if let Some(label) = before.strip_suffix("](").or_else(|| before.strip_suffix("](<")) {
            let (Some(open), Some(close)) = (label.rfind('['), after.find(')')) else { continue };
            let close = at + close + 1;
            if label[..open].ends_with('!') {
                from = close;
                continue;
            }
            return Some((open, close, false));
        }
        if let Some(end) = after.find('>').filter(|_| before.ends_with('<')) {
            return Some((at - 1, at + end + 1, false));
        }
        if !before.chars().next_back().is_none_or(|c| c.is_whitespace() || "*_~(".contains(c)) {
            continue;
        }
        let word = &after[..after.find(|c: char| c.is_whitespace() || c == '<').unwrap_or(after.len())];
        let url = word.trim_end_matches(['.', ',', '!', '?', ')', ';', ':', '*', '_', '~', '|', '\'', '"']);
        return Some((at, at + url.len(), true));
    }
}

// `f` applied to the text outside code: a run of backticks opens a span or block that the next run of the
// same length closes, as in markdown; a run never closed is plain text.
pub(crate) fn outside_code(text: &str, mut f: impl FnMut(&str) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let (mut plain, mut at) = (0, 0);
    while let Some(open) = text[at..].find('`').map(|i| at + i) {
        let len = backtick_run(&text[open..]);
        match closing_run(text, open + len, len) {
            Some(end) => {
                out.push_str(&f(&text[plain..open]));
                out.push_str(&text[open..end]);
                (plain, at) = (end, end);
            }
            None => at = open + len,
        }
    }
    out.push_str(&f(&text[plain..]));
    out
}

pub(super) fn backtick_run(text: &str) -> usize {
    text.bytes().take_while(|&b| b == b'`').count()
}

// Where the next run of exactly `len` backticks at or after `from` ends.
pub(super) fn closing_run(text: &str, mut from: usize, len: usize) -> Option<usize> {
    while let Some(open) = text[from..].find('`').map(|i| from + i) {
        let run = backtick_run(&text[open..]);
        if run == len {
            return Some(open + run);
        }
        from = open + run;
    }
    None
}

// `||secret||` → a solid bar of the same length until revealed (gpui's markdown can't paint a cover).
pub(super) fn hide_spoilers(text: &str, revealed: bool) -> String {
    let parts: Vec<&str> = text.split("||").collect();
    // An odd number of `||` leaves the last one unmatched: it stays literal text.
    let paired = if parts.len().is_multiple_of(2) { parts.len() - 1 } else { parts.len() };
    let mut out = String::with_capacity(text.len());
    for (i, part) in parts.iter().enumerate() {
        match (i % 2 == 1 && i < paired, revealed) {
            (true, true) => out.push_str(part),
            (true, false) => {
                out.push('`');
                out.extend(std::iter::repeat_n('█', part.chars().count().clamp(1, 40)));
                out.push('`');
            }
            (false, _) if i >= paired => {
                out.push_str("||");
                out.push_str(part);
            }
            (false, _) => out.push_str(part),
        }
    }
    out
}

// A user's server tag (Discord's guild tag): its badge and text, as it follows their name.
pub(crate) fn guild_tag(tag: &crate::discord::GuildTag, cx: &App) -> Div {
    // Discord's chip: 16px tall, 4px in, its 14px badge 2px before 12px semibold text in the default ink.
    ui::badge(palette::selected(cx), palette::text(cx)).h(px(TAG_HEIGHT)).px_1().gap_0p5().text_size(ui::TEXT_XS).font_weight(gpui_kit::FontWeight::SEMIBOLD).children(tag.badge.clone().map(|b| img(b).size_3p5())).child(tag.text.clone())
}

// A display name in its Discord style's colours, one run per character, each colour stepped in lightness
// until it reads on `ground` as Discord's client does (3:1, 2.5:1 for the multi-colour fills). GAP: gpui
// text has no gradient fill, stroke or glow, so gradients and prisms step per character and neon, toon
// and pop draw in their main colour; the style's font isn't drawn yet.
pub(crate) fn name_runs(name: &str, style: &NameStyle, ground: Hsla) -> Vec<(std::ops::Range<usize>, HighlightStyle)> {
    let colors = match style.colors.is_empty() {
        true => default_name_colors(style.effect),
        false => style.colors.as_slice(),
    };
    let ratio = if matches!(style.effect, NameEffect::Gradient | NameEffect::Glow | NameEffect::Prism) { 2.5 } else { 3. };
    let inks: Vec<Hsla> = colors.iter().map(|c| readable_on(palette::data_color(*c), ground, ratio)).collect();
    if inks.is_empty() {
        return Vec::new();
    }
    let count = name.chars().count().max(1) as f32;
    let mut letter = 0;
    name.char_indices()
        .enumerate()
        .map(|(i, (at, ch))| {
            let along = (i as f32 + 0.5) / count;
            let ink = match style.effect {
                NameEffect::Gummy => inks[letter % inks.len()],
                // Stops spread over 10%..90%, as Discord lays them.
                NameEffect::Gradient | NameEffect::Glow => along_stops(&inks, ((along - 0.1) / 0.8).clamp(0., 1.)),
                // One cycle through every colour and back to the first.
                NameEffect::Prism => along_stops(&[inks.as_slice(), &inks[..1]].concat(), along),
                NameEffect::Solid | NameEffect::Neon | NameEffect::Toon | NameEffect::Pop => inks[0],
            };
            letter += usize::from(!ch.is_whitespace());
            (at..at + ch.len_utf8(), HighlightStyle { color: Some(ink), ..Default::default() })
        })
        .collect()
}

// A style sent without colours draws its effect's defaults (Discord's web client, 2026-11-19); solid's is
// the theme's own text colour.
pub(super) fn default_name_colors(effect: NameEffect) -> &'static [u32] {
    match effect {
        NameEffect::Solid => &[],
        NameEffect::Gradient | NameEffect::Glow => &[0x2AAEA6, 0xFFC490],
        NameEffect::Gummy => &[0xE9A9D5, 0xA9DDF0, 0xB9A7EE, 0xB9EACB],
        NameEffect::Prism => &[0xFE9242, 0x35EC7E, 0xFFE047, 0xFF4CD2, 0x5865F2],
        NameEffect::Toon => &[0xF42098],
        NameEffect::Neon => &[0x691DED],
        NameEffect::Pop => &[0x0FCF86],
    }
}

// The colour at `t` (0..1) along evenly spaced stops.
pub(super) fn along_stops(stops: &[Hsla], t: f32) -> Hsla {
    let span = t * (stops.len() - 1) as f32;
    let from = (span.floor() as usize).min(stops.len() - 1);
    ui::mix(stops[from], stops[(from + 1).min(stops.len() - 1)], span - from as f32)
}

// Discord's clamp: 0.05 lighter (on a dark ground) or darker per step, at most ten steps.
pub(super) fn readable_on(mut ink: Hsla, ground: Hsla, ratio: f32) -> Hsla {
    let lighten = palette::luminance(ground) <= 0.5;
    for _ in 0..10 {
        if palette::contrast(ink, ground) >= ratio || (lighten && ink.l >= 0.95) || (!lighten && ink.l <= 0.05) {
            break;
        }
        ink.l += if lighten { 0.05 } else { -0.05 };
    }
    ink
}
