// AutoMod's notices: its row, what it flagged and the actions offered under it.
use super::*;

// AutoMod's notices look like messages (types 24 and 36 to 39): its shield, its name in the brand ink and
// Discord's OFFICIAL tag, then what happened.
pub(super) fn automod_row(message: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> AnyElement {
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
    let head = ui::row()
        .gap_2()
        .line_height(ui::LINE_LEAD)
        .child(div().text_size(ui::TEXT_MD).font_weight(FontWeight::MEDIUM).text_color(palette::accent_ink(cx)).child("AutoMod"))
        .child(ui::tag_with(Glyph::Checkmark, "OFFICIAL", cx))
        .child(div().relative().top_px().whitespace_nowrap().text_size(ui::TEXT_XS).text_color(palette::faint(cx)).child(chat::format_stamp(&message.timestamp)));
    let avatar = ui::row()
        .size(px(AVATAR))
        .flex_none()
        // lint-allow(raw-margin): optical, lines the shield's top with the name's cap height
        .mt_0p5()
        .rounded_full()
        .bg(palette::oklch(0.5, 0.13, 245.))
        .justify_center()
        .text_color(gpui_kit::white())
        .child(Icon::new(Glyph::Shield).size(ui::ICON_XL));
    let row = ui::row().items_stretch()
        .id(SharedString::from(format!("msg-{}", message.id)))
        .w_full()
        .px(px(chat::GUTTER))
        .py(px(chat::ROW_PAD))
        .gap(px(chat::GUTTER))
        .text_size(ui::TEXT_SM)
        .text_color(muted)
        .child(avatar)
        .child(ui::col().min_h_0().flex_1().child(head).children(body));
    chat::inset(row.into_any_element(), false)
}

// Not verified: the format of `content`. Discord passes it to `new Date()`, read here as RFC 3339, and
// prints it as 2-digit month and day, numeric year, hour and minute ("11/24/2026, 11:00 PM" in en-US).
pub(super) fn security_until(content: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(content).map(|t| t.with_timezone(&chrono::Local).format("%m/%d/%Y, %-I:%M %p").to_string()).unwrap_or_else(|_| content.to_owned())
}

pub(super) fn automod_field<'a>(message: &'a Message, name: &str) -> Option<&'a str> {
    message.embeds.first()?.fields.iter().find(|(n, _, _)| n == name).map(|(_, v, _)| v.as_str()).filter(|v| !v.is_empty())
}

// A blocked or flagged message: where, what was said with the keyword marked, which rule, and what a
// moderator can do about it.
pub(super) fn automod_action(message: &Message, ctx: &Rc<RowContext>, cx: &mut App) -> Vec<AnyElement> {
    // GAP: safety alerts (raids, mention raids, activity alerts) get only AutoMod's header; their cards aren't drawn yet.
    if message.embeds.first().is_some_and(|e| e.safety_alert) {
        return Vec::new();
    }
    let field = |name: &str| automod_field(message, name);
    let channel_id = field("channel_id").map(str::to_owned);
    let channel = channel_id.as_ref().and_then(|id| ctx.channel_names.get(id)).map_or("unknown-channel".to_owned(), |name| name.clone());
    // Not verified: the outcome's values; anything but "flagged" reads as blocked.
    let verb = if field("decision_outcome") == Some("flagged") { "has flagged a message in " } else { "has blocked a message in " };
    // GAP: app messages, modals, voice channel statuses, nicknames, quarantines and guests have their own headers.
    let header = sentence_line(Sentence::new(&message.id, cx).text(verb).strong(&format!("#{channel}")), None, cx);
    let flagged = message.embeds.first().and_then(|e| e.description.clone()).unwrap_or_default();
    let meta: Vec<String> = [("Keyword", field("keyword").map(str::to_owned)), ("Rule", field("rule_name").map(str::to_owned)), ("Timeout", field("timeout_duration").and_then(timeout_label)), ("Reason", field("decision_reason").map(str::to_owned))]
        .into_iter()
        .filter_map(|(label, value)| Some(format!("{label}: {}", value?)))
        .collect();
    let mut out = vec![header.into_any_element(), flagged_card(message, &flagged, field("keyword_matched_content"), ctx, cx)];
    if !meta.is_empty() {
        out.push(div().pt_1p5().text_size(ui::TEXT_XS).child(meta.join(" · ")).into_any_element());
    }
    out.push(automod_actions(message, channel_id, field("flagged_message_id").map(str::to_owned), ctx, cx));
    out
}

pub(super) fn flagged_card(message: &Message, text: &str, matched: Option<&str>, ctx: &Rc<RowContext>, cx: &App) -> AnyElement {
    let strong = palette::strong(cx);
    let mark = HighlightStyle { background_color: Some(palette::warning_ink(cx).opacity(0.26)), color: Some(strong), ..Default::default() };
    let highlights = matched.and_then(|m| keyword_range(text, m)).map(|r| vec![(r, mark)]).unwrap_or_default();
    let card = ui::row().items_stretch()
        .py_2p5()
        .px_3()
        .squircle(RADIUS_MD)
        .bg(strong.opacity(0.05))
        .gap_2p5()
        .child(ui::avatar(message.author.avatar.clone(), 24.).flex_none())
        .child(
            ui::col().min_h_0()
                .flex_1()
                .child(div().font_weight(FontWeight::SEMIBOLD).text_color(strong).child(ctx.name_of(&message.author)))
                .child(div().text_color(palette::text(cx)).child(StyledText::new(text.to_owned()).with_highlights(highlights))),
        );
    div().pt_1p5().child(card).into_any_element()
}

// Where the matched words sit in the flagged text, ignoring case.
pub(super) fn keyword_range(text: &str, matched: &str) -> Option<std::ops::Range<usize>> {
    let at = text.to_ascii_lowercase().find(&matched.to_ascii_lowercase())?;
    Some(at..at + matched.len())
}

// Not verified: the unit. Discord's `timeout_duration` field is read as seconds.
pub(super) fn timeout_label(seconds: &str) -> Option<String> {
    let seconds: u64 = seconds.parse().ok()?;
    let (n, unit) = match seconds {
        0..60 => (seconds, "sec"),
        60..3600 => (seconds / 60, "min"),
        3600..86400 => (seconds / 3600, "hour"),
        _ => (seconds / 86400, "day"),
    };
    Some(format!("{n} {unit}{}", if n == 1 { "" } else { "s" }))
}

pub(super) fn automod_actions(message: &Message, channel_id: Option<String>, flagged_id: Option<String>, ctx: &Rc<RowContext>, cx: &App) -> AnyElement {
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
    let report = {
        let (this, alert) = (ctx.this(), message.clone());
        ui::button(SharedString::from(format!("automod-report-{key}")), ui::Tone::Secondary, cx).small().child("Report Issues").on_click(move |_, _, cx| {
            cx.stop_propagation();
            let alert = alert.clone();
            this.update(cx, |this, cx| this.report_automod(alert, cx)).ok();
        })
    };
    // GAP: Discord hides Delete once `alert_actions_execution` says the message is gone; that field isn't read yet.
    let delete = target.filter(|(channel_id, _)| ctx.notices.moderated.contains(channel_id)).map(|_| {
        let (this, alert) = (ctx.this(), message.clone());
        ui::button(SharedString::from(format!("automod-delete-{key}")), ui::Tone::Secondary, cx).small().child("Delete User's Message").on_click(move |_, _, cx| {
            cx.stop_propagation();
            this.update(cx, |this, cx| this.automod_alert_action(&alert, AlertAction::DeleteUserMessage, cx)).ok();
        })
    });
    ui::row().items_stretch().pt_2().flex_wrap().gap_2().child(actions).children(jump).child(report).children(delete).into_any_element()
}

// Discord's alert actions on an AutoMod notice (POST /guilds/{id}/auto-moderation/alert-action), values from its client.
#[derive(Clone, Copy)]
pub(crate) enum AlertAction {
    DeleteUserMessage = 3,
    SubmitFeedback = 4,
}

impl Waffle {
    pub(crate) fn automod_alert_action(&mut self, alert: &Message, action: AlertAction, cx: &mut Context<Self>) {
        let Some(guild_id) = alert.guild_id.clone() else { return };
        let body = serde_json::json!({ "message_id": alert.id, "channel_id": alert.channel_id, "alert_action_type": action as u8 });
        self.rest(crate::discord::Method::POST, format!("/guilds/{guild_id}/auto-moderation/alert-action"), Some(body), |_, _| {}, cx);
    }

    // Report Issues: Discord's "Did we get it wrong?" confirm, then the feedback alert action.
    fn report_automod(&mut self, alert: Message, cx: &mut Context<Self>) {
        let body = "Let us know if this message shouldn\u{2019}t have been caught so we can improve AutoMod for the future. By doing this, you are also sending the flagged message to Discord.";
        self.confirm("Did we get it wrong?", body, "Yes", move |this, _, cx| this.automod_alert_action(&alert, AlertAction::SubmitFeedback, cx), cx);
        if let Some(dialog) = &mut self.dialog {
            dialog.danger = false;
        }
    }
}
