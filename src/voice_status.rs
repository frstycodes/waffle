// A voice channel's status (docs/discord-voice-channel-status-re.md): the line under its sidebar row and the
// call's title, and Discord's "What are we chatting about?" dialog that sets it, in Waffle's look
// (design/explore-voice-extras.html §1-3).
// GAP: shown as plain text with custom emoji as `:name:`; Discord's emphasis and spoiler markers show as typed.
use gpui_kit::{
    AnyElement, AppContext, Context, Div, Entity, Focusable, FontWeight, InteractiveElement, KeyDownEvent, ParentElement, SharedString, Stateful, StatefulInteractiveElement, Styled, Subscription, Window,
    component::{
        Icon,
        input::{Input, InputEvent, InputState as SearchState},
    },
    div, img,
    px,
};

use crate::{
    EmojiTarget, Waffle,
    discord::{Channel, ChannelKind, VIEW_CHANNEL, perm},
    icons::Glyph,
    menu::{Entry, item},
    overlays::backdrop,
    palette,
    ui::{self},
};

// The line heights of the status line under a channel and of the dialog's prompt.
const STATUS_LINE: f32 = 14.;
const PROMPT_LINE: f32 = 26.;

pub(crate) const ART: &str = "illustrations/voice-status.png";
// Discord's modal: Set Status is disabled past 500 characters, and the remaining count shows past 250.
const MAX_CHARS: usize = 500;
const SHOW_REMAINING_AFTER: usize = 250;
const WIDTH: f32 = 440.;
const HERO_HEIGHT: f32 = 210.;
const ART_WIDTH: f32 = 380.;
// The status line's indent: the row's 18px glyph and its 8px gap.
const LINE_INDENT: f32 = 26.;

pub(crate) struct VoiceStatusDialog {
    channel_id: String,
    // The status when the dialog opened, as Discord sends it: Set Status with the same text sends nothing.
    current: String,
    pub(crate) input: Entity<SearchState>,
    sending: bool,
    error: Option<SharedString>,
    _subscription: Subscription,
}

impl Waffle {
    // CHANNEL_INFO replaces a server's statuses, so a channel missing from it has none.
    pub(crate) fn on_voice_statuses(&mut self, guild_id: &str, statuses: Vec<(String, String)>, all: bool, cx: &mut Context<Self>) {
        if all && let Some(guild) = self.guilds.iter().find(|g| g.id == guild_id) {
            for channel in &guild.channels {
                self.voice_status.remove(&channel.id);
            }
        }
        for (channel, status) in statuses {
            self.set_voice_status(channel, status);
        }
        cx.notify(); // lint-allow(global-notify): a voice status shows on the channel list (nav) and the call stage (main)
    }

    fn set_voice_status(&mut self, channel_id: String, status: String) {
        match status.is_empty() {
            true => self.voice_status.remove(&channel_id),
            false => self.voice_status.insert(channel_id, status),
        };
    }

    pub(crate) fn voice_status_of(&self, channel_id: &str) -> Option<&str> {
        self.voice_status.get(channel_id).map(String::as_str)
    }

    fn connected_to(&self, channel_id: &str) -> bool {
        self.call.as_ref().is_some_and(|c| c.connected && c.channel.id == channel_id)
    }

    // Discord's full check: Set Voice Channel Status, Connect and View Channel, in a voice channel (stages have none).
    fn can_set_voice_status(&self, channel: &Channel) -> bool {
        guild_voice(channel) && [perm::SET_VOICE_CHANNEL_STATUS, perm::CONNECT, VIEW_CHANNEL].into_iter().all(|bit| self.can_in(&channel.id, bit))
    }

    // Only from inside the channel, as Discord: the sidebar, the call header and the menu all ask this.
    pub(crate) fn can_edit_voice_status(&self, channel: &Channel) -> bool {
        self.connected_to(&channel.id) && self.can_set_voice_status(channel)
    }

    // Discord's menu: Clear from outside the channel with Manage Channels, else Set from inside it.
    pub(crate) fn voice_status_entry(&self, channel: &Channel) -> Option<Entry> {
        let has_status = self.voice_status_of(&channel.id).is_some();
        if guild_voice(channel) && !self.connected_to(&channel.id) && has_status && self.can_in(&channel.id, perm::MANAGE_CHANNELS) {
            let id = channel.id.clone();
            return Some(item("Clear channel status", move |this, _, cx| this.clear_voice_status(id.clone(), cx)).icon(Glyph::XCircle).entry());
        }
        let open = channel.clone();
        self.can_edit_voice_status(channel).then(|| item("Set channel status", move |this, window, cx| this.open_voice_status(&open, window, cx)).icon(Glyph::Pen).entry())
    }

    pub(crate) fn open_voice_status(&mut self, channel: &Channel, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.voice_status_of(&channel.id).unwrap_or_default().to_owned();
        let shown = self.shown_status(&current);
        let placeholder = format!("Status for {}", channel.name);
        let input = cx.new(|cx| SearchState::new(window, cx).placeholder(placeholder).default_value(shown));
        let subscription = cx.subscribe_in(&input, window, |this, _, event: &InputEvent, _, cx| match event {
            InputEvent::PressEnter { .. } => this.submit_voice_status(cx),
            _ => cx.notify(), // lint-allow(global-notify): the status dialog is an overlay, which only follows the app
        });
        input.update(cx, |state, cx| state.focus(window, cx));
        self.menu = None;
        self.voice_status_dialog = Some(VoiceStatusDialog { channel_id: channel.id.clone(), current, input, sending: false, error: None, _subscription: subscription });
        cx.notify(); // lint-allow(global-notify): the status dialog is an overlay, which only follows the app
    }

    fn close_voice_status(&mut self, cx: &mut Context<Self>) {
        self.voice_status_dialog = None;
        cx.notify(); // lint-allow(global-notify): the status dialog is an overlay, which only follows the app
    }

    // The field's text as Discord takes it: picked custom emoji back to `<:name:id>`.
    fn status_wire(&self, text: &str) -> String {
        let mut emoji: Vec<_> = self.mention_wire.iter().filter(|(shown, _)| shown.starts_with(':')).collect();
        emoji.sort_by_key(|(shown, _)| std::cmp::Reverse(shown.len()));
        emoji.into_iter().fold(text.to_owned(), |text, (shown, wire)| text.replace(shown.as_str(), wire))
    }

    // A status as the field shows it: custom emoji as `:name:`, remembered so sending puts them back.
    fn shown_status(&mut self, status: &str) -> String {
        for (shown, wire) in custom_emoji(status) {
            self.mention_wire.insert(shown, wire);
        }
        plain_status(status)
    }

    fn submit_voice_status(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.voice_status_dialog.as_ref().filter(|d| !d.sending) else { return };
        let text = dialog.input.read(cx).value().to_string();
        if text.chars().count() > MAX_CHARS {
            return;
        }
        let status = self.status_wire(text.trim());
        if status == dialog.current {
            return self.close_voice_status(cx);
        }
        let channel_id = dialog.channel_id.clone();
        if let Some(dialog) = self.voice_status_dialog.as_mut() {
            dialog.sending = true;
            dialog.error = None;
        }
        cx.notify(); // lint-allow(global-notify): the status dialog is an overlay, which only follows the app
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            let result = client.set_voice_channel_status(&channel_id, &status).await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.set_voice_status(channel_id, status);
                        this.voice_status_dialog = None;
                    }
                    Err(error) => {
                        if let Some(dialog) = this.voice_status_dialog.as_mut() {
                            dialog.sending = false;
                            dialog.error = Some(error.into());
                        }
                    }
                }
                cx.notify(); // lint-allow(global-notify): the status dialog is an overlay, which only follows the app
            })
            .ok();
        })
        .detach(); // lint-allow(detached-task): a one-shot request that ends when it returns, holding a WeakEntity
    }

    // The menu's Clear Channel Status: an empty status, with Discord's refusal as a toast.
    pub(crate) fn clear_voice_status(&mut self, channel_id: String, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            let result = client.set_voice_channel_status(&channel_id, "").await;
            this.update(cx, |this, cx| match result {
                Ok(()) => {
                    this.set_voice_status(channel_id, String::new());
                    cx.notify(); // lint-allow(global-notify): a voice status shows on the channel list (nav) and the call stage (main)
                }
                Err(error) => this.show_toast(error, cx),
            })
            .ok();
        })
        .detach(); // lint-allow(detached-task): a one-shot request that ends when it returns, holding a WeakEntity
    }

    // The emoji picker's pick, at the caret.
    pub(crate) fn insert_voice_status_emoji(&mut self, shown: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = &self.voice_status_dialog else { return };
        dialog.input.update(cx, |state, cx| {
            state.insert(shown, window, cx);
            state.focus(window, cx);
        });
    }

    // The sidebar row's second line: the status, or, for someone in the channel who may set one, the way to.
    // Clicking it opens the dialog only for them; for everyone else it's inert text.
    pub(crate) fn voice_row_status(&self, channel: &Channel, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let status = self.voice_status_of(&channel.id).filter(|_| guild_voice(channel));
        let editable = self.can_edit_voice_status(channel);
        let line = ui::row().id(SharedString::from(format!("voice-status-{}", channel.id))).pl(px(LINE_INDENT)).gap_1p5().text_size(ui::TEXT_XS).line_height(ui::LINE_XS);
        let line = match status {
            Some(status) => line.text_color(palette::muted(cx)).font_weight(FontWeight::NORMAL).child(ui::line(plain_status(status))),
            None if editable => {
                let strong = palette::strong(cx);
                line.text_color(palette::text(cx)).font_weight(FontWeight::MEDIUM).hover(move |s| s.text_color(strong)).child("Set a channel status").child(Icon::new(Glyph::Pen).size(ui::ICON_SM))
            }
            None => return None,
        };
        if !editable {
            return Some(line);
        }
        let open = channel.clone();
        Some(line.cursor_pointer().on_click(cx.listener(move |this, _, window, cx| {
            cx.stop_propagation();
            this.open_voice_status(&open, window, cx);
        })))
    }

    // Under the call's title: the status while it has one, opening the dialog for those who may change it.
    pub(crate) fn call_status_line(&self, channel: &Channel, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let status = self.voice_status_of(&channel.id).filter(|_| guild_voice(channel))?;
        let line = ui::line(plain_status(status)).id("call-status").text_size(ui::TEXT_XS).line_height(px(STATUS_LINE)).font_weight(FontWeight::NORMAL).text_color(palette::stage::muted());
        if !self.can_edit_voice_status(channel) {
            return Some(line);
        }
        let (open, strong) = (channel.clone(), palette::stage::text());
        Some(
            line.occlude()
                .cursor_pointer()
                .hover(move |s| s.text_color(strong))
                .tooltip(ui::Tooltip::text("Set a channel status"))
                .on_click(cx.listener(move |this, _, window, cx| this.open_voice_status(&open, window, cx))),
        )
    }

    pub(crate) fn voice_status_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.voice_status_dialog.as_ref()?;
        let (strong, muted) = (palette::strong(cx), palette::muted(cx));
        let typed = dialog.input.read(cx).value().chars().count();
        let over = typed > MAX_CHARS;
        // The card clips to a rectangle, so the hero's tint takes the dialog's top corners itself.
        let hero = ui::edge(div(), ui::top(ui::RADIUS_XL))
            .relative()
            .h(px(HERO_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            // Fades to the same tint at zero alpha by the hero's bottom edge: fading to transparent black
            // greys the middle, and a diagonal leaves the lower corner tinted, both read as a hard edge.
            .bg(gpui_kit::linear_gradient(180., gpui_kit::linear_color_stop(palette::accent_soft(cx), 0.), gpui_kit::linear_color_stop(palette::accent_soft(cx).opacity(0.), 1.)))
            .child(img(ART).w(px(ART_WIDTH)).h(px(ART_WIDTH * 640. / 1024.)))
            .child(div().absolute().top_3().right_3().child(ui::Button::new("voice-status-close").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).icon(Glyph::X).tooltip_text("Close").on_click(cx.listener(|this, _, _, cx| this.close_voice_status(cx)))));
        let focus = dialog.input.read(cx).focus_handle(cx);
        let emoji = ui::Button::new("voice-status-emoji").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconXs).icon(Glyph::Emoji).tooltip_text("Add emoji").on_click(cx.listener(|this, _, window, cx| this.open_emoji_picker(EmojiTarget::VoiceStatus, window, cx)));
        let field = ui::Input::new("voice-status-field", Input::new(&dialog.input).appearance(false), &focus).trailing(emoji).error(over || dialog.error.is_some());
        let remaining = (typed > SHOW_REMAINING_AFTER).then(|| {
            div().flex_none().font_features(crate::call::tabular()).text_color(if over { palette::danger_ink(cx) } else { palette::faint(cx) }).child((MAX_CHARS as i64 - typed as i64).to_string())
        });
        let meta = ui::row()
            .min_h_4()
            .items_start()
            .justify_between()
            .gap_2()
            .text_size(ui::TEXT_XS)
            .child(div().min_w_0().children(dialog.error.clone().map(|e| ui::FieldError::new(e, cx))))
            .children(remaining);
        let actions = ui::row().items_stretch()
            .gap_2()
            .child(ui::Button::new("voice-status-cancel").variant(ui::ButtonVariant::Secondary).flex_1().child("Cancel").on_click(cx.listener(|this, _, _, cx| this.close_voice_status(cx))))
            .child(
                ui::Button::new("voice-status-submit")
                    .flex_1()
                    .disabled(over)
                    .loading(dialog.sending.then_some("Set status"))
                    .child("Set status")
                    .on_click(cx.listener(|this, _, _, cx| this.submit_voice_status(cx))),
            );
        let body = ui::col()
            .pt_4()
            .px_6()
            .pb_6()
            .gap_2()
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if e.keystroke.key == "escape" {
                    this.close_voice_status(cx);
                }
            }))
            .child(div().text_center().text_size(ui::TEXT_LG).line_height(px(PROMPT_LINE)).font_weight(FontWeight::BOLD).text_color(strong).child("What are we chatting about?"))
            .child(div().pb_3().text_center().text_color(muted).child("Let others know what you're up to in the voice channel!"))
            .child(div().font_weight(FontWeight::SEMIBOLD).text_color(strong).child("Status"))
            .child(field)
            .child(meta)
            .child(div().pt_1().child(actions));
        let card = ui::col().child(hero).child(body);
        let card = div().id("voice-status").occlude().child(ui::Dialog::new("voice-status-dialog", WIDTH, card, window, cx));
        Some(ui::Dialog::modal(backdrop("voice-status-backdrop", cx), card, cx))
    }
}

// Only a server voice channel has a status; a stage (raw type 13) has none.
fn guild_voice(channel: &Channel) -> bool {
    channel.kind == ChannelKind::Voice && channel.raw_type != 13
}

// Each custom emoji in a status as (`:name:`, `<:name:id>`).
fn custom_emoji(status: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut rest = status;
    while let Some(start) = rest.find('<') {
        let tail = &rest[start..];
        let Some(end) = tail.find('>') else { break };
        let token = &tail[1..end];
        let name = token.strip_prefix("a:").or_else(|| token.strip_prefix(':')).and_then(|t| t.split_once(':')).filter(|(_, id)| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()));
        if let Some((name, _)) = name {
            found.push((format!(":{name}:"), tail[..=end].to_owned()));
        }
        rest = &tail[end + 1..];
    }
    found
}

// A status as one line of text: custom emoji as `:name:`.
pub(crate) fn plain_status(status: &str) -> String {
    custom_emoji(status).into_iter().fold(status.to_owned(), |text, (shown, wire)| text.replace(&wire, &shown))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_emoji_read_as_names() {
        assert_eq!(plain_status("lofi <:waffle:42> and <a:dance:7>"), "lofi :waffle: and :dance:");
        assert_eq!(plain_status("a <b> c <:x:> d"), "a <b> c <:x:> d", "not emoji: left as typed");
        assert_eq!(custom_emoji("<a:dance:7>"), vec![(":dance:".to_owned(), "<a:dance:7>".to_owned())]);
    }
}
