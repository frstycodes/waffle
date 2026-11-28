// The lines around an app's reply: who used which command, its status and failures, and the ephemeral footer.
use crate::ui;

use super::*;

// The line above an app's reply: "[avatar] Mira used /play", 14px with the command in link ink, joined to
// the avatar by the reply spine. A component's reply names no command.
pub(crate) fn used_command(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> Option<AnyElement> {
    let meta = message.interaction.as_ref()?;
    let command = meta.command.as_ref()?;
    // Discord names whoever used it, you included.
    let name = ctx.name_of(&meta.user);
    let strong = palette::strong(cx);
    Some(
        ui::row()
            .relative()
            .pl(px(chat::CONTENT_INSET))
            .child(chat::reply_spine(cx))
            .h(px(CAPTION_HEIGHT))
            .gap_1p5()
            .mb(px(chat::ACCESSORY_GAP)) // lint-allow(raw-margin): Discord's gap under the command line, as under a reply; the row's children have no parent gap
            .text_size(ui::TEXT_SM)
            .text_color(palette::muted(cx))
            .child(ui::Avatar::new(meta.user.avatar.clone(), 16.))
            .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).text_color(strong).child(name))
            .child(div().flex_none().child("used"))
            .child(ui::line(format!("/{command}")).font_weight(FontWeight::MEDIUM).text_color(palette::accent_ink(cx)))
            .into_any_element(),
    )
}

// The line under a command's stand-in or a deferred reply: Discord's "Sending command...", "{app} is
// thinking...", or why it never answered.
pub(crate) fn interaction_status(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> Option<AnyElement> {
    let waiting = |text: String| {
        ui::row()
            .gap_2()
            .italic()
            .text_color(palette::muted(cx))
            .child(ui::Spinner::new(SharedString::from(format!("thinking-{}", message.id)), palette::accent_ink(cx), 14.))
            .child(text)
            .into_any_element()
    };
    // Discord names the app by its bot's username.
    let app = Some(message.author.username.clone()).filter(|u| !u.is_empty()).unwrap_or_else(|| ctx.name_of(&message.author));
    Some(match interactions::command_status(message, interactions::now_ms())? {
        CommandStatus::Sending => waiting("Sending command...".into()),
        CommandStatus::Thinking => waiting(format!("{app} is thinking...")),
        CommandStatus::TimedOut => failure_line(interactions::DID_NOT_RESPOND.into(), cx),
        CommandStatus::Failed(why) => failure_line(why, cx),
    })
}

pub(crate) fn is_ephemeral(message: &Message) -> bool {
    message.flags & app_commands::EPHEMERAL != 0 || is_failed_stand_in(message)
}

// Our command's local stand-in, which the app never answered: only we ever saw it.
pub(crate) fn is_failed_stand_in(message: &Message) -> bool {
    message.state == SendState::Failed && interactions::is_stand_in(message)
}

// An ephemeral reply's tint: Discord's 5% wash of its brand, here the info ink.
pub(crate) fn ephemeral_tint(cx: &App) -> Hsla {
    palette::info_ink(cx).opacity(0.05)
}

// Discord's footer: "Only you can see this • Dismiss message", or with `ephemerality_reason` "Only you
// can see this message." over "{reason} • Dismiss". Dismissing drops it here, as Discord's does.
pub(crate) fn only_you(message: &Message, ctx: &Rc<RowContext>, cx: &App) -> AnyElement {
    let (this, channel_id, id) = (ctx.this(), message.channel_id.clone(), message.id.clone());
    let reason = message.interaction.as_ref().and_then(|m| m.ephemerality_reason).map(ephemerality_reason);
    let dismiss = ui::Button::new(SharedString::from(format!("dismiss-{}", message.id))).variant(ui::ButtonVariant::Link)
        .text_size(ui::TEXT_XS)
        .child(if reason.is_some() { "Dismiss" } else { "Dismiss message" })
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            this.update(cx, |this, cx| {
                this.remove_message(&channel_id, &id);
                cx.notify();
            })
            .ok();
        });
    let line = |lead: &str| ui::row().flex_wrap().gap_1().child(lead.to_owned()).child("•").child(dismiss);
    let text = match reason {
        Some(reason) => ui::col().child("Only you can see this message.").child(line(reason)),
        None => line("Only you can see this"),
    };
    ui::row()
        .mt(px(chat::ACCESSORY_GAP)) // lint-allow(raw-margin): optical gap under the component's body; the parent is a message row in another file
        .items_start()
        .gap_1p5()
        .text_size(ui::TEXT_XS)
        .text_color(palette::muted(cx))
        .child(ui::row().flex_none().h(px(SEEN_ICON_HEIGHT)).child(Icon::new(Glyph::Eye).size(ui::ICON_SM)))
        .child(text)
        .into_any_element()
}

// `interaction_metadata.ephemerality_reason` in Discord's words; an unknown one (0 included, as
// Discord's switch has no case for NONE) reads as its default.
pub(super) fn ephemerality_reason(reason: u64) -> &'static str {
    match reason {
        1 => "This feature is temporarily limited",
        2 => "This feature is temporarily limited for this server",
        3 => "This feature is temporarily limited for you",
        4 => "You are sending messages too quickly",
        5 => "You are being rate limited",
        6 => "You cannot message this user",
        7 => "You do not meet this server's verification level",
        8 => "You cannot unarchive this thread",
        9 => "You cannot join this thread",
        10 => "You cannot send messages in this channel",
        11 => "You cannot send attachments in this channel",
        12 => "You cannot send embeds in this channel",
        13 => "You cannot send stickers in this channel",
        14 => "This message was blocked by AutoMod. The message may also be viewed by the community owners.",
        15 => "This message contains a link blocked by Discord.",
        16 => "You cannot use this command in this channel",
        17 => "Only you can see this message during beta",
        18 => "You cannot use external apps in this channel",
        _ => "An unknown error occurred",
    }
}

// Discord's red failure line: an error icon and what went wrong. No retry.
pub(super) fn failure_line(text: String, cx: &App) -> AnyElement {
    ui::row()
        .mt(px(chat::ACCESSORY_GAP)) // lint-allow(raw-margin): optical gap under the component's body; the parent is a message row in another file
        .gap_1p5()
        .text_color(palette::danger_ink(cx))
        .child(Icon::new(Glyph::WarningCircle).size(ui::ICON_MD).flex_none())
        .child(div().min_w_0().child(text))
        .into_any_element()
}

// Under a row or section: the line for the failed interaction on one of its controls.
pub(super) fn failed_line<'a>(message: &Message, controls: impl IntoIterator<Item = &'a Component>, ctx: &RowContext, cx: &App) -> Option<AnyElement> {
    let failure = controls.into_iter().filter_map(custom_id).find_map(|id| ctx.components.calls.get(&(message.id.clone(), id)).and_then(|c| c.failure.clone()))?;
    Some(failure_line(failure, cx))
}
