// Text chat in a voice channel, around the call (docs/discord-voice-text-chat-re.md): new messages float
// in at the stage's top right while the chat is closed, the header's chat button counts them, and the
// sidebar's Open Chat opens it.
use std::time::{Duration, Instant};

use gpui_kit::{Animation, AnimationExt, AnyElement, Context, FontWeight, SharedString, Window, component::Icon, div, prelude::*, px};

use crate::{
    Waffle,
    discord::{Channel, Message},
    palette::{self, stage},
    ui::{self, RADIUS_MD, Squircle},
};

// Discord's toasts: three at most, one shared 10 s timer, 8px apart.
const CAP: usize = 3;
const LINGER: Duration = Duration::from_secs(10);
const EXIT: Duration = Duration::from_millis(200);
const GAP: f32 = 8.;
const WIDTH: f32 = 300.;
// Under the stage's 48px header band.
const TOP: f32 = 56.;
const RIGHT: f32 = 12.;
const AVATAR: f32 = 32.;
// The header's chat button stays lit this long after the unread count changes.
const HIGHLIGHT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub(crate) struct Preview {
    pub message: Message,
    pub mention: bool,
}

// The floating cards. `frozen` is the stack as it was when the pointer arrived: it stays on screen
// while hovered, whatever the timer or new messages do underneath.
#[derive(Default)]
pub(crate) struct CallPreviews {
    cards: Vec<Preview>,
    frozen: Option<Vec<Preview>>,
    leaving: Vec<(Preview, Instant)>,
    timer: u64,
}

impl CallPreviews {
    // Adds a card, dropping the oldest past three, and restarts the shared timer; returns the timer's
    // generation for `expire`.
    pub fn push(&mut self, preview: Preview, now: Instant) -> u64 {
        self.cards.push(preview);
        let over = self.cards.len().saturating_sub(CAP);
        let dropped: Vec<Preview> = self.cards.drain(..over).collect();
        self.leave(dropped, now);
        self.timer += 1;
        self.timer
    }

    // The shared timer fired: clears every card, unless a newer message restarted it.
    pub fn expire(&mut self, timer: u64, now: Instant) -> bool {
        if timer != self.timer {
            return false;
        }
        let gone = std::mem::take(&mut self.cards);
        self.leave(gone, now);
        true
    }

    pub fn freeze(&mut self, on: bool, now: Instant) {
        if on {
            self.frozen.get_or_insert_with(|| self.cards.clone());
            return;
        }
        let Some(shown) = self.frozen.take() else { return };
        let gone: Vec<Preview> = shown.into_iter().filter(|p| !self.cards.iter().any(|c| c.message.id == p.message.id)).collect();
        self.leave(gone, now);
    }

    pub fn visible(&self) -> &[Preview] {
        self.frozen.as_deref().unwrap_or(&self.cards)
    }

    fn leave(&mut self, gone: Vec<Preview>, now: Instant) {
        if self.frozen.is_none() {
            self.leaving.extend(gone.into_iter().map(|p| (p, now)));
        }
    }

    // Drops cards whose exit has played; whether any are still on their way out.
    pub fn prune(&mut self, now: Instant) -> bool {
        self.leaving.retain(|(_, at)| now.duration_since(*at) < EXIT);
        !self.leaving.is_empty()
    }
}

impl Waffle {
    // A message in the call's voice channel: a card, unless it's our own send from here.
    pub(crate) fn note_call_message(&mut self, message: &Message, own_send: bool, cx: &mut Context<Self>) {
        let in_call = self.call.as_ref().is_some_and(|c| c.guild_id.is_some() && c.channel.id == message.channel_id);
        if !in_call || own_send || message.pin_notice || message.call.is_some() {
            return;
        }
        if message.author.id != self.my_id() {
            self.call_unread_at = Some(Instant::now());
        }
        let preview = Preview { message: message.clone(), mention: self.mentions_me(message) };
        let timer = self.call_previews.push(preview, Instant::now());
        self.after_preview_exit(cx);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(LINGER).await;
            this.update(cx, |this, cx| {
                if this.call_previews.expire(timer, Instant::now()) {
                    this.after_preview_exit(cx);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    // Redraws once cards that just left have faded, to take them out.
    fn after_preview_exit(&mut self, cx: &mut Context<Self>) {
        if !self.call_previews.prune(Instant::now()) {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(EXIT).await;
            this.update(cx, |this, cx| {
                this.call_previews.prune(Instant::now());
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // Discord's Open Chat: in that call, the chat opens beside it; otherwise the channel opens on its own.
    pub(crate) fn open_call_chat(&mut self, channel: Channel, window: &mut Window, cx: &mut Context<Self>) {
        let in_call = self.call.as_ref().is_some_and(|c| c.channel.id == channel.id);
        if in_call {
            self.show_call(channel.clone(), window, cx);
        }
        if self.channel.as_ref().is_none_or(|c| c.id != channel.id) {
            self.open_channel(channel, window, cx);
        }
        if in_call {
            self.call_chat = true;
            self.call_view = true;
            self.composer.update(cx, |state, cx| state.focus(window, cx));
        }
        cx.notify();
    }

    // The Toggle Voice Channel Chat keybind: only in a server voice channel. Hotkeys come without a
    // window, and the main window's root is this view, so it runs once this update ends.
    pub(crate) fn toggle_call_chat_keybind(&mut self, cx: &mut Context<Self>) {
        if !self.call.as_ref().is_some_and(|c| c.guild_id.is_some()) {
            return;
        }
        let (this, main) = (cx.weak_entity(), self.main_window);
        cx.defer(move |cx| {
            main.update(cx, |_, window, cx| this.update(cx, |this, cx| this.toggle_call_chat(window, cx))).ok();
        });
    }

    // Mentions and unread messages in the server voice channel we're in; None in a DM call.
    pub(crate) fn call_chat_counts(&self) -> Option<(u32, u32)> {
        let call = self.call.as_ref().filter(|c| c.guild_id.is_some())?;
        let id = call.channel.id.as_str();
        let last_read = self.read_states.get(id).map_or(0, |r| r.last_read);
        let me = self.my_id();
        // GAP: counted from the loaded page (the newest 50), so a longer backlog shows as 50.
        let counted = self.feeds.get(id).map_or(0, |f| f.messages.iter().filter(|m| m.author.id != me && m.id.parse::<u64>().is_ok_and(|n| n > last_read)).count());
        let unread = if self.unread(id) { (counted as u32).max(1) } else { 0 };
        Some((self.mention_count(id), unread))
    }

    // Lit while there are mentions, and for a while after the unread count changes.
    pub(crate) fn call_chat_highlighted(&self) -> bool {
        let mentions = self.call_chat_counts().is_some_and(|(m, _)| m > 0);
        mentions || self.call_unread_at.is_some_and(|at| at.elapsed() < HIGHLIGHT)
    }

    pub(crate) fn call_previews_enabled(&self, channel_id: &str) -> bool {
        !self.prefs.call_previews_off.iter().any(|id| id == channel_id)
    }

    pub(crate) fn toggle_call_previews(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        let on = self.call_previews_enabled(channel_id);
        self.update_prefs(cx, |p| match on {
            true => p.call_previews_off.push(channel_id.to_owned()),
            false => p.call_previews_off.retain(|id| id != channel_id),
        });
    }

    // The stack at the stage's top right: leaving cards first (they're the oldest), then the rest.
    pub(crate) fn render_call_previews(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let call = self.call.as_ref().filter(|c| c.guild_id.is_some())?;
        if self.call_chat_shown() || !self.call_previews_enabled(&call.channel.id) {
            return None;
        }
        let shown = self.call_previews.visible();
        if shown.is_empty() && self.call_previews.leaving.is_empty() {
            return None;
        }
        let messages: Vec<Message> = self.call_previews.leaving.iter().map(|(p, _)| p.message.clone()).chain(shown.iter().map(|p| p.message.clone())).collect();
        let ctx = self.row_context_everywhere(&messages, WIDTH - AVATAR - 28., cx);
        let leaving = self.call_previews.leaving.iter().map(|(p, _)| {
            preview_card(p, &ctx, cx)
                .with_animation(SharedString::from(format!("call-preview-out-{}", p.message.id)), Animation::new(EXIT), |card, t| card.opacity(1. - t).top(px(-GAP * t)))
                .into_any_element()
        });
        let leaving: Vec<AnyElement> = leaving.collect();
        let cards = shown.iter().map(|p| {
            let (channel, id) = (p.message.channel_id.clone(), p.message.id.clone());
            preview_card(p, &ctx, cx)
                .cursor_pointer()
                .hover(|s| s.bg(palette::oklch(0.28, 0.005, 260.).opacity(0.96)))
                .on_click(cx.listener(move |this, _, window, cx| this.open_preview(&channel, id.clone(), window, cx)))
                .with_animation(SharedString::from(format!("call-preview-in-{}", p.message.id)), Animation::new(ui::DUR_PANEL), |card, t| card.opacity(t).left(px(12. * (1. - crate::motion::back_out(t)))))
                .into_any_element()
        });
        let stack = div()
            .id("call-previews")
            .occlude()
            .absolute()
            .top(px(TOP))
            .right(px(RIGHT))
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .gap(px(GAP))
            .children(leaving)
            .children(cards)
            // Occluding takes the hover from the stage, so the stack keeps itself frozen.
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.call_previews.freeze(*hovered, Instant::now());
                if !*hovered {
                    this.after_preview_exit(cx);
                }
                cx.notify();
            }));
        Some(stack.into_any_element())
    }

    // A card clicked: the chat opens at that message, flashed.
    fn open_preview(&mut self, channel_id: &str, message_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(channel) = self.call.as_ref().map(|c| c.channel.clone()).filter(|c| c.id == channel_id) else { return };
        self.call_previews.freeze(false, Instant::now());
        self.open_call_chat(channel, window, cx);
        self.jump_to(message_id, cx);
    }
}

// .preview: avatar, name, and the message clamped to two lines; dark in both themes, as the stage.
fn preview_card(preview: &Preview, ctx: &std::rc::Rc<crate::chat::RowContext>, cx: &mut Context<Waffle>) -> gpui_kit::Stateful<gpui_kit::Div> {
    let message = &preview.message;
    let mut shadows = vec![ui::shadow(gpui_kit::black().opacity(0.35), 0., 4., 16., 0., false)];
    shadows.push(match preview.mention {
        true => ui::shadow(palette::oklch(0.78, 0.15, 75.).opacity(0.5), 0., 0., 0., 1., true),
        false => ui::shadow(stage::wash(0.08), 0., 0., 0., 1., false),
    });
    // GAP: plain text, not the rich renderer: that inks mentions and links for the app theme, which
    // turns dark on this always-dark card in light mode. Discord renders rich content here.
    let text = crate::chat::plain_text(&message.content, ctx);
    let body = (!text.trim().is_empty()).then(|| text.into_any_element()).or_else(|| {
        let (glyph, label) = crate::forward::attachment_summary(message)?;
        Some(div().flex().items_center().gap(px(6.)).text_color(stage::muted()).child(Icon::new(glyph).size(px(16.))).child(label).into_any_element())
    });
    div()
        .id(SharedString::from(format!("call-preview-{}", message.id)))
        .relative()
        .w_full()
        .py(px(8.))
        .px(px(10.))
        .squircle(RADIUS_MD)
        .bg(palette::oklch(0.25, 0.005, 260.).opacity(0.92))
        .shadow(shadows)
        .flex()
        .gap(px(8.))
        .child(ui::avatar(message.author.avatar.clone(), AVATAR))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().truncate().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(stage::strong()).child(message.author.name.clone()))
                .children(body.map(|b| div().text_size(px(14.)).text_color(stage::text()).line_clamp(2).child(b))),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: &str) -> Preview {
        Preview { message: Message { id: id.into(), ..Default::default() }, mention: false }
    }

    fn ids(previews: &CallPreviews) -> Vec<String> {
        previews.visible().iter().map(|p| p.message.id.clone()).collect()
    }

    #[test]
    fn keeps_the_newest_three() {
        let (mut previews, now) = (CallPreviews::default(), Instant::now());
        for id in ["1", "2", "3", "4"] {
            previews.push(card(id), now);
        }
        assert_eq!(ids(&previews), ["2", "3", "4"]);
        assert!(previews.prune(now), "the dropped card fades out");
    }

    #[test]
    fn a_new_message_restarts_the_shared_timer() {
        let (mut previews, now) = (CallPreviews::default(), Instant::now());
        let first = previews.push(card("1"), now);
        let second = previews.push(card("2"), now);
        assert!(!previews.expire(first, now), "the first timer was replaced");
        assert_eq!(ids(&previews), ["1", "2"]);
        assert!(previews.expire(second, now));
        assert!(ids(&previews).is_empty(), "one timer clears every card");
    }

    #[test]
    fn hovering_freezes_the_stack() {
        let (mut previews, now) = (CallPreviews::default(), Instant::now());
        let timer = previews.push(card("1"), now);
        previews.freeze(true, now);
        previews.expire(timer, now);
        previews.push(card("2"), now);
        assert_eq!(ids(&previews), ["1"], "the stack shows what was there when hovered");
        previews.freeze(false, now);
        assert_eq!(ids(&previews), ["2"]);
        assert!(previews.prune(now), "the expired card leaves once let go");
        assert!(!previews.prune(now + EXIT));
    }
}
