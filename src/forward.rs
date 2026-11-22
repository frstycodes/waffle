// Forwarding a message (design/flows-messaging.html#forward, laid out like Discord's "Forward To"): a dialog
// that picks up to five places and an optional note, then one forward per place, a type-1 reference to the
// original, with the note sent after it as a normal message.
use gpui_kit::{
    AnyElement, Context, Entity, Focusable, FontWeight, SharedString, Subscription, Window,
    component::{
        Icon,
        input::{Input, InputEvent, InputState as SearchState},
    },
    div,
    prelude::*,
    px,
};

use crate::{
    EmojiTarget, Waffle,
    discord::{Channel, ChannelKind, Guild, MediaKind, Message, perm},
    icons::Glyph,
    layout::avatar_with_status,
    overlays::backdrop,
    palette,
    ui::{self, Check, RADIUS_XS},
};

const MAX_PLACES: usize = 5;
const DIALOG_WIDTH: f32 = 480.;
// Five rows tall; past that the list scrolls.
const LIST_HEIGHT: f32 = 300.;
// Rows listed at once; search reaches the rest.
const LIST_CAP: usize = 50;
const LEAD: f32 = 40.;
const THUMB: f32 = 56.;

pub(crate) struct ForwardDialog {
    message: Message,
    // The original's server; history loaded over REST doesn't carry it on the message.
    guild_id: Option<String>,
    search: Entity<SearchState>,
    pub(crate) note: Entity<SearchState>,
    // (channel id, label) in the order picked.
    picked: Vec<(String, String)>,
    _subscriptions: [Subscription; 2],
}

// A place the message can go, as the dialog lists it.
struct Place<'a> {
    channel: &'a Channel,
    // None for a DM.
    guild: Option<&'a Guild>,
    // What the toast calls it: "#clips", or a person's name.
    label: String,
    // The server's name or "Direct message"; for a place you can't post in, why not.
    detail: SharedString,
    locked: bool,
}

impl Waffle {
    pub(crate) fn open_forward(&mut self, message: Message, window: &mut Window, cx: &mut Context<Self>) {
        let search = cx.new(|cx| SearchState::new(window, cx).placeholder("Search channels and people"));
        let note = cx.new(|cx| SearchState::new(window, cx).placeholder("Add a note (optional)"));
        let subscriptions = [
            cx.subscribe_in(&search, window, |_, _, _: &InputEvent, _, cx| cx.notify()),
            cx.subscribe_in(&note, window, |this, _, event: &InputEvent, _, cx| match event {
                InputEvent::PressEnter { .. } => this.send_forward(cx),
                _ => cx.notify(),
            }),
        ];
        search.update(cx, |state, cx| state.focus(window, cx));
        let guild_id = message.guild_id.clone().or_else(|| self.place_of(None, &message.channel_id).and_then(|(guild, _)| guild));
        self.forward_dialog = Some(ForwardDialog { message, guild_id, search, note, picked: Vec::new(), _subscriptions: subscriptions });
        cx.notify();
    }

    /// Names for the channels forwarded messages came from, for their source lines.
    pub(crate) fn forward_sources(&self, messages: &[Message]) -> Vec<(String, String)> {
        messages
            .iter()
            .filter_map(|m| m.forward.as_deref())
            .filter_map(|original| Some((original.channel_id.clone(), self.place_of(original.guild_id.as_deref(), &original.channel_id)?.1.name)))
            .collect()
    }

    // DMs and server text and voice channels you can see (a voice channel's text chat takes messages), the
    // most recently active first; ones you can't post in stay listed, locked, so they don't seem to be missing.
    // GAP: a DM whose person doesn't accept messages from you isn't known until a send fails, and a DM row says
    // "Direct message" where Discord shows the username: `User` keeps only the display name.
    fn forward_places(&self, query: &str) -> Vec<Place<'_>> {
        let me = self.my_id();
        let dms = self.dms.iter().map(|c| Place { channel: c, guild: None, label: c.name.clone(), detail: "Direct message".into(), locked: false });
        let channels = self
            .guilds
            .iter()
            .flat_map(|g| g.channels.iter().filter(move |c| matches!(c.kind, ChannelKind::Text | ChannelKind::Voice) && g.can_view(me, c)).map(move |c| (g, c)))
            .map(|(g, c)| {
                let locked = !self.can_in(&c.id, perm::SEND_MESSAGES);
                Place { channel: c, guild: Some(g), label: format!("#{}", c.name), detail: if locked { "Read only".into() } else { g.name.clone().into() }, locked }
            });
        let query = query.to_lowercase();
        let mut places: Vec<Place> = dms.chain(channels).filter(|p| p.channel.name.to_lowercase().contains(&query)).collect();
        places.sort_by_key(|p| std::cmp::Reverse(p.channel.last_message_id.as_deref().and_then(|id| id.parse::<u64>().ok()).unwrap_or(0)));
        places.truncate(LIST_CAP);
        places
    }

    pub(crate) fn toggle_forward_place(&mut self, id: String, label: String, cx: &mut Context<Self>) {
        let Some(dialog) = self.forward_dialog.as_mut() else { return };
        match dialog.picked.iter().position(|(picked, _)| *picked == id) {
            Some(at) => _ = dialog.picked.remove(at),
            None if dialog.picked.len() < MAX_PLACES => dialog.picked.push((id, label)),
            None => {}
        }
        cx.notify();
    }

    fn close_forward(&mut self, cx: &mut Context<Self>) {
        self.forward_dialog = None;
        cx.notify();
    }

    // Closes at once; each place's outcome comes back as one toast when all have gone.
    // GAP: a note that fails after its forward went counts the place as failed; the toast can't say which part.
    fn send_forward(&mut self, cx: &mut Context<Self>) {
        let (Some(client), Some(dialog)) = (self.client.clone(), self.forward_dialog.as_ref()) else { return };
        if dialog.picked.is_empty() {
            return;
        }
        let Some(dialog) = self.forward_dialog.take() else { return };
        let note = self.to_wire(dialog.note.read(cx).value().trim());
        let (message, guild_id, places) = (dialog.message, dialog.guild_id, dialog.picked);
        cx.spawn(async move |this, cx| {
            let mut failed = Vec::new();
            for (channel_id, label) in &places {
                let mut sent = client.forward_message(channel_id, &message, guild_id.as_deref(), &crate::nonce()).await.map(drop);
                if sent.is_ok() && !note.is_empty() {
                    sent = client.send_message(channel_id, &note, &crate::nonce(), None, false).await.map(drop);
                }
                if sent.is_err() {
                    failed.push(label.clone());
                }
            }
            let labels: Vec<String> = places.into_iter().map(|(_, label)| label).collect();
            this.update(cx, |this, cx| this.show_toast(forward_outcome(&labels, &failed), cx)).ok();
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn forward_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.forward_dialog.as_ref()?;
        let query = dialog.search.read(cx).value().trim().to_owned();
        let full = dialog.picked.len() >= MAX_PLACES;
        let head = div()
            .flex()
            .items_start()
            .gap_3()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(div().text_size(px(20.)).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child("Forward message"))
                    .child(div().text_size(px(14.)).text_color(palette::muted(cx)).child(format!("{} · {}", self.forward_subtitle(&dialog.message), picked_count(dialog.picked.len())))),
            )
            .child(ui::icon_button("forward-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.close_forward(cx))));
        let places = self.forward_places(&query);
        let list: AnyElement = if places.is_empty() {
            ui::empty_state("forward-empty", crate::mascot::Mood::Waiting, format!("Nothing called “{query}”"), "Search channels, DMs and group DMs by name.", cx).into_any_element()
        } else {
            let rows: Vec<AnyElement> = places
                .into_iter()
                .map(|place| {
                    let on = dialog.picked.iter().any(|(id, _)| *id == place.channel.id);
                    self.place_row(place, on, full && !on, window, cx)
                })
                .collect();
            div().id("forward-places").flex().flex_col().gap(px(2.)).max_h(px(LIST_HEIGHT)).overflow_y_scroll().children(rows).into_any_element()
        };
        let body = ui::dialog_body()
            .child(head)
            .child(ui::search_field("forward-search", &dialog.search, cx))
            .child(list)
            .children(self.forward_preview(&dialog.message, cx))
            .child(self.note_row(dialog, cx));
        Some(ui::modal(backdrop("forward-backdrop", cx), ui::dialog("forward-dialog", DIALOG_WIDTH, body, window, cx), cx))
    }

    // "Rosa's message from #general"; a DM's needs no place.
    fn forward_subtitle(&self, message: &Message) -> String {
        let author = &message.author.name;
        match self.place_of(message.guild_id.as_deref(), &message.channel_id) {
            Some((Some(_), channel)) => format!("{author}'s message from #{}", channel.name),
            _ => format!("{author}'s message"),
        }
    }

    // The place's mark, name and where it is, then its check, or a lock where you can't post.
    fn place_row(&self, place: Place, on: bool, full: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let id = place.channel.id.clone();
        let text = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(div().truncate().child(place.channel.name.clone()))
            .child(div().truncate().text_size(px(12.)).text_color(palette::muted(cx)).child(place.detail.clone()));
        let mark = match place.locked {
            true => div().text_color(palette::faint(cx)).child(Icon::new(Glyph::HashtagLock).size(px(18.))).into_any_element(),
            false => ui::check_live(SharedString::from(format!("forward-check-{id}")), if on { Check::On } else { Check::Off }, window, cx).into_any_element(),
        };
        let lead = self.place_lead(&place, cx);
        let row = ui::menu_row(SharedString::from(format!("forward-place-{id}")), cx).disabled(place.locked || full).child(lead).child(text).child(mark);
        if place.locked || full {
            return row.into_any_element();
        }
        let label = place.label;
        row.on_click(cx.listener(move |this, _, _, cx| this.toggle_forward_place(id.clone(), label.clone(), cx))).into_any_element()
    }

    // A server channel: the server's icon with the channel's kind on a badge at its corner. A DM: the person
    // with their presence, or a group's faces.
    fn place_lead(&self, place: &Place, cx: &Context<Self>) -> AnyElement {
        let surface = palette::surface(palette::Level::Raised, cx);
        let Some(guild) = place.guild else {
            let channel = place.channel;
            return match &channel.recipient {
                Some(user) => avatar_with_status(&user.avatar, LEAD, self.presences.get(&user.id).map(|p| p.mark()), surface),
                None => ui::dm_face(&channel.face(), LEAD, surface),
            };
        };
        let glyph = match place.channel.kind {
            ChannelKind::Voice => Glyph::VolumeHigh,
            _ if place.channel.private || place.locked => Glyph::HashtagLock,
            _ => Glyph::Hashtag,
        };
        ui::server_tile_badged(guild.icon.as_deref(), &guild.name, LEAD, glyph, surface, cx).into_any_element()
    }

    // What goes out: the text, two lines at most, or what it carries, with the first picture at the right.
    fn forward_preview(&self, message: &Message, cx: &mut Context<Self>) -> Option<AnyElement> {
        let shown = message.forward.as_deref().unwrap_or(message);
        let ctx = self.row_context_everywhere(std::slice::from_ref(shown), DIALOG_WIDTH, cx);
        let text = crate::chat::content(shown, &ctx, cx).map(|t| div().line_clamp(2).child(t).into_any_element());
        let text = text.or_else(|| {
            let (glyph, label) = attachment_summary(shown)?;
            Some(div().flex().items_center().gap(px(6.)).text_color(palette::muted(cx)).child(Icon::new(glyph).size(px(16.))).child(label).into_any_element())
        })?;
        let thumb = shown.media.first().map(|m| ui::media(crate::discord::thumb_url(m, THUMB * 2.), ui::all(RADIUS_XS)).size(px(THUMB)).flex_none().object_fit(gpui_kit::ObjectFit::Cover));
        Some(
            div()
                .flex()
                .items_center()
                .gap_3()
                .pl(px(12.))
                .border_l(px(2.))
                .border_color(palette::line_strong(cx))
                .text_size(px(14.))
                .text_color(palette::text(cx))
                .child(div().flex_1().min_w_0().child(text))
                .children(thumb)
                .into_any_element(),
        )
    }

    // The note, with its emoji button inside, and Send beside it.
    fn note_row(&self, dialog: &ForwardDialog, cx: &mut Context<Self>) -> AnyElement {
        let focus = dialog.note.read(cx).focus_handle(cx);
        let emoji = ui::icon_button("forward-note-emoji", Glyph::Emoji, "Add emoji", cx).small().on_click(cx.listener(|this, _, window, cx| this.open_emoji_picker(EmojiTarget::ForwardNote, window, cx)));
        let note = ui::text_field("forward-note", Input::new(&dialog.note).appearance(false), &focus).trailing(emoji);
        // The field's height, so the two read as one row.
        let send = ui::button("forward-send", ui::Tone::Primary, cx)
            .h(px(40.))
            .disabled(dialog.picked.is_empty())
            .child(Icon::new(Glyph::PaperPlane).size(px(18.)))
            .child("Send")
            .on_click(cx.listener(|this, _, _, cx| this.send_forward(cx)));
        div().flex().items_center().gap_2().child(div().flex_1().min_w_0().child(note)).child(send).into_any_element()
    }
}

fn picked_count(picked: usize) -> String {
    match picked {
        0 => format!("Pick up to {MAX_PLACES} places"),
        n => format!("{n} of {MAX_PLACES} picked"),
    }
}

// "1 image", "2 files", "Sticker": what a message without text carries.
fn attachment_summary(message: &Message) -> Option<(Glyph, String)> {
    let count = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let videos = message.media.iter().filter(|m| matches!(m.kind, MediaKind::Video { .. })).count();
    match (message.media.len(), message.files.len()) {
        (0, 0) if !message.stickers.is_empty() => Some((Glyph::Sticker, "Sticker".into())),
        (0, 0) => None,
        (0, files) => Some((Glyph::File, count(files, "file", "files"))),
        (media, _) if videos == media => Some((Glyph::Image, count(media, "video", "videos"))),
        (media, _) => Some((Glyph::Image, count(media, "image", "images"))),
    }
}

fn forward_outcome(sent_to: &[String], failed: &[String]) -> String {
    match (failed, sent_to) {
        ([], [one]) => format!("Forwarded to {one}"),
        ([], _) => format!("Forwarded to {} places", sent_to.len()),
        _ => format!("Couldn't forward to {}", failed.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discord::{File, Media};

    #[test]
    fn forward_outcome_names_the_place_or_counts() {
        let places = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        assert_eq!(forward_outcome(&places(&["#clips"]), &[]), "Forwarded to #clips");
        assert_eq!(forward_outcome(&places(&["#clips", "Mira"]), &[]), "Forwarded to 2 places");
        assert_eq!(forward_outcome(&places(&["#clips", "Mira"]), &places(&["Mira"])), "Couldn't forward to Mira");
        assert_eq!(picked_count(0), "Pick up to 5 places");
        assert_eq!(picked_count(2), "2 of 5 picked");
    }

    #[test]
    fn attachments_read_as_counts() {
        let image = Media { url: "a.png".into(), width: 1., height: 1., kind: MediaKind::Image, description: None };
        let file = File { name: "a.zip".into(), url: "a.zip".into(), size: 1 };
        let summary = |m: Message| attachment_summary(&m).map(|(_, label)| label);
        assert_eq!(summary(Message { media: vec![image.clone()], ..Default::default() }).as_deref(), Some("1 image"));
        assert_eq!(summary(Message { files: vec![file.clone(), file], ..Default::default() }).as_deref(), Some("2 files"));
        assert_eq!(summary(Message::default()), None);
    }
}
