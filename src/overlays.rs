// Floating surfaces over the app: profile popout, status menu, emoji picker, quick switcher, and the
// actions the right-click menus run.
use std::time::Duration;

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    AnyElement, App, ClipboardItem, Context, Entity, FontWeight, HighlightStyle, KeyDownEvent, MouseButton, ScrollHandle, SharedString, StyledText, Window,
    component::{
        ActiveTheme, Icon,
        input::InputEvent,
    },
    div, img, point, prelude::*, px, relative,
};

use crate::{
    Channel, ChannelKind, Waffle, EmojiTarget, SearchState,
    discord::{Emoji, Message, User},
    icons::Glyph,
    illustration::Illustration,
    layout::avatar_with_status,
    palette,
    ui::{self, RADIUS_LG, RADIUS_MD, RADIUS_SM, RADIUS_XS, Squircle},
};

const PICKER_SIZE: (f32, f32) = (420., 440.);
// The composer's pickers share one frame, tall enough for the tabs over the GIF picker's.
const COMPOSER_PICKER_HEIGHT: f32 = PICKER_SIZE.1 + 100.;
const SWITCHER_WIDTH: f32 = 560.;

// The three pickers the composer opens in one spot, in tab order.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComposerPicker {
    Gifs,
    Stickers,
    Emoji,
}

impl ComposerPicker {
    const ALL: [ComposerPicker; 3] = [ComposerPicker::Gifs, ComposerPicker::Stickers, ComposerPicker::Emoji];
}

impl Waffle {
    pub(crate) fn on_emoji_search(&mut self, _: &Entity<SearchState>, _: &InputEvent, _: &mut Window, cx: &mut Context<Self>) {
        cx.notify();
    }

    pub(crate) fn on_switcher_input(&mut self, _: &Entity<SearchState>, event: &InputEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            InputEvent::PressEnter { .. } => self.accept_switcher(window, cx),
            InputEvent::Change => {
                self.switcher = Some(0);
                cx.notify();
            }
            _ => {}
        }
    }

    pub(crate) fn copy_text(&self, text: impl Into<String>, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text.into()));
    }

    pub(crate) fn message_user(&mut self, user_id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.profile_card = None;
        if let Some(dm) = self.dms.iter().find(|d| d.recipient.as_ref().is_some_and(|r| r.id == user_id)).cloned() {
            self.guild_id = None;
            return self.open_channel(dm, window, cx);
        }
        let Some(client) = self.client.clone() else { return };
        cx.spawn_in(window, async move |this, cx| {
            let Ok(dm) = client.open_dm(&user_id).await else { return };
            this.update_in(cx, |this, window, cx| {
                if !this.dms.iter().any(|d| d.id == dm.id) {
                    this.dms.insert(0, dm.clone());
                }
                this.guild_id = None;
                this.open_channel(dm, window, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Puts an @name chip at the end of the composer, sent as a real mention.
    pub(crate) fn mention_user(&mut self, user: &User, window: &mut Window, cx: &mut Context<Self>) {
        let chip = gpui_kit::component::input::InlineToken::new(format!("<@{}>", user.id), format!("@{}", user.name));
        self.composer.update(cx, |state, cx| {
            let text = state.value();
            let end = text.len();
            state.set_selected_range(end..end, cx);
            if !text.is_empty() && !text.ends_with(' ') {
                state.insert(" ", window, cx);
            }
            crate::complete_mention(state, "", chip, window, cx);
            state.focus(window, cx);
        });
        self.has_draft = true;
        self.profile_card = None;
        cx.notify();
    }

    pub(crate) fn mark_channel_read(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        let Some(&last) = self.last_message.get(channel_id) else { return };
        let read = self.read_states.entry(channel_id.to_owned()).or_default();
        read.last_read = last;
        read.mentions = 0;
        if let Some(client) = self.client.clone() {
            let id = channel_id.to_owned();
            crate::discord::runtime().spawn(async move { client.ack(&id, &last.to_string()).await });
        }
        cx.notify();
    }

    pub(crate) fn mark_guild_read(&mut self, guild_id: &str, cx: &mut Context<Self>) {
        let Some(guild) = self.guilds.iter().find(|g| g.id == guild_id) else { return };
        let channels: Vec<String> = guild.channels.iter().filter(|c| self.unread(&c.id) || self.mention_count(&c.id) > 0).map(|c| c.id.clone()).collect();
        self.mark_channels_read(&channels, cx);
    }

    /// Acks every channel in `channels` at its newest message in one request.
    pub(crate) fn mark_channels_read(&mut self, channels: &[String], cx: &mut Context<Self>) {
        let unread: Vec<(String, u64)> = channels.iter().filter_map(|c| Some((c.clone(), *self.last_message.get(c)?))).collect();
        for (channel, last) in &unread {
            let read = self.read_states.entry(channel.clone()).or_default();
            read.last_read = *last;
            read.mentions = 0;
        }
        if let Some(client) = self.client.clone().filter(|_| !unread.is_empty()) {
            crate::discord::runtime().spawn(async move { client.ack_bulk(unread).await });
        }
        cx.notify();
    }

    /// Moves the read line to just before this message, as Discord's "Mark Unread", and tells Discord, so the
    /// channel stays unread on your other devices.
    pub(crate) fn mark_unread(&mut self, message: &Message, cx: &mut Context<Self>) {
        let Ok(id) = message.id.parse::<u64>() else { return };
        let loaded = self.feeds.get(&message.channel_id).map(|f| f.messages.clone()).unwrap_or_default();
        let (ack_id, mentions) = manual_ack(&loaded, id, |m| self.counts_as_mention(m));
        let read = self.read_states.entry(message.channel_id.clone()).or_default();
        read.last_read = id - 1;
        read.mentions = mentions;
        self.unread_marker = Some(id - 1);
        self.held_unread = Some(message.channel_id.clone());
        if let Some(client) = self.client.clone() {
            let channel_id = message.channel_id.clone();
            crate::discord::runtime().spawn(async move { client.ack_manual(&channel_id, ack_id, mentions).await });
        }
        cx.notify();
    }

    /// Clears a mention from the Inbox for good, without reading its channel; it comes back if Discord refuses.
    pub(crate) fn mark_mention_read(&mut self, message_id: String, cx: &mut Context<Self>) {
        let Some(Some(Ok(list))) = &mut self.inbox else { return };
        let Some(at) = list.iter().position(|m| m.id == message_id) else { return };
        let removed = list.remove(at);
        cx.notify();
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            let Err(error) = client.delete_mention(&message_id).await else { return };
            this.update(cx, |this, cx| {
                if let Some(Some(Ok(list))) = &mut this.inbox {
                    list.insert(at.min(list.len()), removed);
                }
                this.show_toast(format!("Couldn't mark that mention read: {error}"), cx);
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn message_link(&self, message: &Message) -> String {
        let place = message.guild_id.clone().or_else(|| self.guild_id.clone()).unwrap_or_else(|| "@me".into());
        format!("https://discord.com/channels/{place}/{}/{}", message.channel_id, message.id)
    }

    pub(crate) fn open_emoji_picker(&mut self, target: EmojiTarget, window: &mut Window, cx: &mut Context<Self>) {
        self.emoji_picker = Some(target);
        self.popover_at = window.mouse_position();
        self.emoji_search.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.focus(window, cx);
        });
        cx.notify();
    }

    fn pick_emoji(&mut self, emoji: Emoji, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.emoji_picker.take() else { return };
        match target {
            EmojiTarget::Reaction(message) => self.toggle_reaction(&message, emoji, cx),
            EmojiTarget::PollAnswer(ix) => self.set_poll_emoji(ix, emoji),
            EmojiTarget::CustomStatus => {
                if let Some(editor) = self.status_editor.as_mut() {
                    editor.emoji = Some(emoji);
                }
                cx.notify();
            }
            EmojiTarget::ForwardNote => {
                let shown = self.shown_emoji(&emoji);
                if let Some(dialog) = &self.forward_dialog {
                    dialog.note.update(cx, |state, cx| {
                        state.insert(shown, window, cx);
                        state.focus(window, cx);
                    });
                }
            }
            EmojiTarget::Composer => {
                let shown = self.shown_emoji(&emoji);
                // insert, not set_value: that would drop the draft's mention chips and put the caret first.
                self.composer.update(cx, |state, cx| {
                    let end = state.value().len();
                    state.set_selected_range(end..end, cx);
                    state.insert(shown, window, cx);
                    state.focus(window, cx);
                });
                self.has_draft = true;
            }
        }
        cx.notify();
    }

    // An emoji as a field shows it: itself, or `:name:` for a server's own, which `to_wire` turns back into
    // its `<:name:id>` when sent.
    fn shown_emoji(&mut self, emoji: &Emoji) -> String {
        let Some(id) = &emoji.id else { return emoji.name.clone() };
        let shown = format!(":{}:", emoji.name);
        let wire = format!("<{}:{}:{id}>", if emoji.animated { "a" } else { "" }, emoji.name);
        self.mention_wire.insert(shown.clone(), wire);
        shown
    }

    pub(crate) fn open_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.switcher = Some(0);
        self.switcher_input.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.focus(window, cx);
        });
        cx.notify();
    }

    // Every text channel and DM, best name matches first; empty query lists DMs then the open server.
    fn switcher_results(&self, cx: &Context<Self>) -> Vec<(Option<String>, Channel, SharedString)> {
        let query = self.switcher_input.read(cx).value().to_lowercase();
        let mut results: Vec<(Option<String>, Channel, SharedString)> = Vec::new();
        for dm in &self.dms {
            results.push((None, dm.clone(), "Direct Messages".into()));
        }
        for guild in &self.guilds {
            for channel in guild.channels.iter().filter(|c| c.kind == ChannelKind::Text && guild.can_view(self.my_id(), c)) {
                results.push((Some(guild.id.clone()), channel.clone(), guild.name.clone().into()));
            }
        }
        if query.is_empty() {
            results.retain(|(guild, _, _)| guild.is_none() || guild == &self.guild_id);
        } else {
            results.retain(|(_, c, _)| c.name.to_lowercase().contains(&query));
            results.sort_by_key(|(_, c, _)| !c.name.to_lowercase().starts_with(&query));
        }
        results.truncate(12);
        results
    }

    fn accept_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected) = self.switcher else { return };
        let Some((guild_id, channel, _)) = self.switcher_results(cx).into_iter().nth(selected) else { return };
        self.switcher = None;
        self.guild_id = guild_id;
        self.open_channel(channel, window, cx);
        self.composer.update(cx, |state, cx| state.focus(window, cx));
    }

    pub(crate) fn on_switcher_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected) = self.switcher else { return };
        let count = self.switcher_results(cx).len().max(1);
        match event.keystroke.key.as_str() {
            "down" => self.switcher = Some((selected + 1) % count),
            "up" => self.switcher = Some((selected + count - 1) % count),
            "escape" => self.switcher = None,
            "enter" => return self.accept_switcher(window, cx),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Closes whichever floating surface is open; true if one was.
    pub(crate) fn dismiss_overlay(&mut self, cx: &mut Context<Self>) -> bool {
        // The note's picker sits over the forward dialog: closing it leaves the dialog and its picks.
        if matches!(self.emoji_picker, Some(EmojiTarget::ForwardNote)) {
            self.emoji_picker = None;
            cx.notify();
            return true;
        }
        let text_viewer = self.text_files.borrow_mut().viewer.take().is_some();
        let open = text_viewer
            | self.profile_card.take().is_some()
            | std::mem::take(&mut self.status_menu)
            | self.status_editor.take().is_some()
            | self.poll_creator.take().is_some()
            | self.forward_dialog.take().is_some()
            | self.reactions_dialog.take().is_some()
            | self.emoji_picker.take().is_some()
            | self.switcher.take().is_some()
            | self.close_share_picker(cx)
            | self.pins.take().is_some()
            | std::mem::take(&mut self.inbox_open)
            | self.gif_picker.take().is_some()
            | self.dismiss_join_overlay()
            | self.dismiss_call_menus()
            | self.close_menu();
        if open {
            cx.notify();
        }
        open
    }

    pub(crate) fn render_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::new();
        out.extend(self.profile_overlay(window, cx));
        out.extend(self.text_viewer_overlay(window, cx));
        out.extend(self.status_menu_overlay(cx));
        out.extend(self.status_editor_overlay(window, cx));
        out.extend(self.poll_creator_overlay(window, cx));
        out.extend(self.forward_overlay(window, cx));
        out.extend(self.reactions_overlay(window, cx));
        out.extend(self.emoji_overlay(window, cx));
        out.extend(self.switcher_overlay(window, cx));
        out.extend(self.share_overlay(window, cx));
        out.extend(self.pins_overlay(cx));
        out.extend(self.inbox_overlay(window, cx));
        out.extend(self.composer_picker_overlay(window, cx));
        out.extend(self.join_overlays(window, cx));
        out
    }

    // The Inbox: your recent mentions across servers, each jumping to its message. It rises from the rail's
    // Inbox tile, showing the latest few, with the way to the full page at its foot.
    fn inbox_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.inbox_open {
            return None;
        }
        const PREVIEW: usize = 6;
        let more = self.inbox_tab == crate::inbox::InboxTab::Mentions && matches!(&self.inbox, Some(Some(Ok(list))) if list.len() > PREVIEW);
        let see_all = ui::button("inbox-see-all", ui::Tone::Secondary, cx)
            .w_full()
            .child(if more { "See all mentions" } else { "Open Inbox" })
            .on_click(cx.listener(|this, _, _, cx| this.open_inbox_page(cx)));
        Some(
            backdrop("inbox-backdrop", cx)
                .child(
                    crate::ui::surface(cx)
                        .id("inbox")
                        .absolute()
                        .left(px(crate::layout::RAIL_WIDTH + 4.))
                        .bottom(px(f32::from(self.user_panel_clearance()) + 4.))
                        .w(px(420.))
                        .max_h(px(560.))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(div().px_3().pt_2().pb_2().child(self.inbox_head(true, window, cx)))
                        .child(div().id("inbox-list").flex_shrink(1.).min_h_0().overflow_y_scroll().px_3().pb_3().child(self.inbox_body(Some(PREVIEW), cx)))
                        .child(div().px_3().pb_3().child(see_all)),
                )
                .into_any_element(),
        )
    }

    /// Your recent mentions as cards (at most `limit`), each jumping to its message on click.
    // A fetched list of messages as cards (mentions, saved), each opening its message.
    pub(crate) fn message_cards(&self, list: &Option<Option<Result<Vec<Message>, SharedString>>>, limit: Option<usize>, empty: (&'static str, &'static str), cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        match list {
            None | Some(None) => mention_skeletons(cx),
            Some(Some(Err(error))) => div().p_4().text_color(theme.danger).child(error.clone()).into_any_element(),
            Some(Some(Ok(list))) if list.is_empty() => crate::ui::empty_state("messages-empty", Illustration::InboxEmpty, empty.0, empty.1, cx).into_any_element(),
            Some(Some(Ok(list))) => {
                let shown: Vec<Message> = list.iter().take(limit.unwrap_or(usize::MAX)).cloned().collect();
                let ctx = self.row_context_everywhere(&shown, 380., cx);
                // The demo shows the first card as hovered, its actions up.
                let demo = cfg!(debug_assertions) && std::env::var("WAFFLE_DEMO").as_deref() == Ok("inbox-mark-read");
                div().flex().flex_col().gap_2().children(shown.iter().enumerate().map(|(i, m)| self.mention_card(m, &ctx, demo && i == 0, cx))).into_any_element()
            }
        }
    }

    // A mention, opening its message on click; on hover Jump and Discord's checkmark, Mark As Read, sit in a
    // slot of their own beside the author.
    fn mention_card(&self, m: &Message, ctx: &std::rc::Rc<crate::chat::RowContext>, revealed: bool, cx: &mut Context<Self>) -> gpui_kit::Stateful<gpui_kit::Div> {
        let place = match self.place_of(m.guild_id.as_deref(), &m.channel_id) {
            Some((Some(guild_id), channel)) => self.guilds.iter().find(|g| g.id == guild_id).map(|g| format!("#{} · {}", channel.name, g.name)).unwrap_or_default(),
            Some((None, channel)) => channel.name.clone(),
            None => String::new(),
        };
        let group = SharedString::from(format!("inbox-{}", m.id));
        let open = {
            let (guild_id, channel_id, message_id) = (m.guild_id.clone(), m.channel_id.clone(), m.id.clone());
            move |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| this.open_message(guild_id.clone(), &channel_id, message_id.clone(), window, cx)
        };
        let jump_open = open.clone();
        let jump = ui::button(SharedString::from(format!("inbox-jump-{}", m.id)), ui::Tone::Secondary, cx).small().child("Jump").on_click(cx.listener(move |this, _, window, cx| {
            cx.stop_propagation();
            jump_open(this, window, cx);
        }));
        let message_id = m.id.clone();
        let read = ui::icon_button(SharedString::from(format!("inbox-read-{}", m.id)), Glyph::Checkmark, "Mark As Read", cx).small().on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.mark_mention_read(message_id.clone(), cx);
        }));
        let actions = div().flex_none().my(px(-3.)).flex().items_center().gap(px(2.)).child(jump).child(read);
        let actions = if revealed { actions } else { actions.opacity(0.).group_hover(group.clone(), |s| s.opacity(1.)) };
        let author = div().flex().items_center().gap_2().child(ui::avatar(m.author.avatar.clone(), 24.)).child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::NORMAL).child(m.author.name.clone())).child(actions);
        div()
            .id(group.clone())
            .group(group)
            .p_3()
            .squircle(RADIUS_XS)
            .bg(palette::surface(palette::Level::Raised, cx))
            .cursor_pointer()
            .hover(|s| s.bg(palette::hover(cx)))
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_size(px(12.)).text_color(cx.theme().muted_foreground).child(place))
            .child(author)
            .child(div().text_size(px(14.)).line_clamp(3).children(crate::chat::content(m, ctx, cx)))
            .on_click(cx.listener(move |this, _, window, cx| open(this, window, cx)))
    }

    /// Where a composer popover's right edge sits from the window's: the composer's own right edge, which
    /// stops short of the members column while it shows.
    fn composer_popover_right(&self) -> f32 {
        let panel = self.panel_shown();
        16. + if panel { self.panel_width } else { 0. }
    }

    /// A composer popover's foot: 8px above the composer, which sits 8px off the window's.
    fn composer_popover_bottom(&self) -> f32 {
        self.composer_height.get() + 16.
    }

    // The composer's picker (media.html): GIFs, Stickers and Emoji tabs over one frame that stays put while
    // the tabs change what's in it. It rises from the composer at its right edge.
    fn composer_picker_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let content = match self.composer_picker()? {
            ComposerPicker::Gifs => self.gif_tab(self.gif_picker.as_ref()?, cx),
            ComposerPicker::Stickers => self.sticker_tab(window, cx),
            ComposerPicker::Emoji => self.emoji_content(window, cx),
        };
        let picker = picker_frame("composer-picker", COMPOSER_PICKER_HEIGHT, cx)
            .absolute()
            .right(px(self.composer_popover_right()))
            .bottom(px(self.composer_popover_bottom()))
            .child(div().pt(px(ui::SURFACE_INSET)).child(self.picker_tabs(window, cx)))
            .children(content);
        Some(backdrop("composer-picker-backdrop", cx).child(ui::rise(picker.transform_origin(ui::corner(true, true)), "composer-picker", Duration::ZERO)).into_any_element())
    }

    // GIFs, Stickers and Emoji over one frame at the same spot; Emoji is its own picker, so switching to or
    // from it swaps which one is open.
    fn picker_tabs(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let items = [ui::Tab::new("GIFs"), ui::Tab::new("Stickers"), ui::Tab::new("Emoji")];
        let selected = self.composer_picker().map_or(0, |tab| tab as usize);
        let this = cx.entity().downgrade();
        div().px(px(12.)).pt(px(4.)).child(ui::tabs("picker-tabs", &items, selected, move |i, window, cx| {
            this.update(cx, |this, cx| this.show_composer_picker(ComposerPicker::ALL[i], window, cx)).ok();
        }, window, cx))
    }

    /// Which of the composer's pickers is open, if any: what its tabs and buttons show as chosen.
    pub(crate) fn composer_picker(&self) -> Option<ComposerPicker> {
        if matches!(self.emoji_picker, Some(EmojiTarget::Composer)) {
            return Some(ComposerPicker::Emoji);
        }
        self.gif_picker.as_ref()?;
        Some(match self.picker_tab {
            crate::stickers::PickerTab::Gifs => ComposerPicker::Gifs,
            crate::stickers::PickerTab::Stickers => ComposerPicker::Stickers,
        })
    }

    /// A composer button or picker tab: opens that picker in the shared spot, or closes it if it's the one open.
    pub(crate) fn show_composer_picker(&mut self, picker: ComposerPicker, window: &mut Window, cx: &mut Context<Self>) {
        if self.composer_picker() == Some(picker) {
            self.gif_picker = None;
            self.emoji_picker = None;
            return cx.notify();
        }
        match picker {
            ComposerPicker::Gifs | ComposerPicker::Stickers => {
                self.emoji_picker = None;
                if self.gif_picker.is_none() {
                    self.open_gifs(window, cx);
                }
                let tab = if picker == ComposerPicker::Gifs { crate::stickers::PickerTab::Gifs } else { crate::stickers::PickerTab::Stickers };
                self.set_picker_tab(tab, window, cx);
            }
            ComposerPicker::Emoji => {
                self.gif_picker = None;
                self.open_emoji_picker(EmojiTarget::Composer, window, cx);
            }
        }
    }

    fn gif_tab(&self, results: &Option<Result<Vec<crate::discord::Gif>, SharedString>>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let column_width = (PICKER_SIZE.0 - 28.) / 2.;
        let body: AnyElement = match results {
            None => div().p_4().text_color(theme.muted_foreground).child("Loading GIFs…").into_any_element(),
            Some(Err(error)) => div().p_4().text_color(theme.danger).child(error.clone()).into_any_element(),
            Some(Ok(gifs)) => div()
                .flex()
                .flex_wrap()
                .gap_1()
                .children(gifs.iter().enumerate().map(|(i, g)| {
                    let url = g.url.clone();
                    let height = column_width * g.height / g.width.max(1.);
                    ui::pressable(div().id(SharedString::from(format!("gif-{i}"))), 0.97)
                        .w(px(column_width))
                        .h(px(height))
                        .squircle(RADIUS_MD)
                        .overflow_hidden()
                        .bg(theme.background)
                        .cursor_pointer()
                        .hover(|s| s.opacity(0.85))
                        .child(img(g.preview.clone()).size_full().object_fit(gpui_kit::ObjectFit::Cover).squircle(RADIUS_MD))
                        .on_click(cx.listener(move |this, _, window, cx| this.send_gif(url.clone(), window, cx)))
                }))
                .into_any_element(),
        };
        vec![
            div().p(px(10.)).child(ui::search_field("gif-search", &self.gif_search, cx)).into_any_element(),
            div().id("gif-scroll").flex_1().min_h_0().overflow_y_scrollbar().px_2().pb_2().child(body).into_any_element(),
        ]
    }

    // The pins popover under the header's pin button: newest first, each the message as search draws it.
    fn pins_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pins = self.pins.as_ref()?;
        let theme = cx.theme().clone();
        let body: AnyElement = match pins {
            None => div().p_4().text_color(theme.muted_foreground).child("Loading pins…").into_any_element(),
            Some(Err(error)) => div().p_4().text_color(theme.danger).child(error.clone()).into_any_element(),
            Some(Ok(list)) if list.is_empty() => crate::ui::empty_state("pins-empty", Illustration::PinsEmpty, "This channel doesn't have any pinned messages… yet.", "Pin a message from its menu to keep it here.", cx).into_any_element(),
            // A list, as the feed: it lays out only the cards in view, so scrolling doesn't redraw every pin.
            Some(Ok(list)) => {
                // Pins change from the fetch, an unpin and the demo; the list follows here, where they're drawn.
                if self.pins_list.item_count() != list.len() {
                    self.pins_list.reset(list.len());
                }
                let ctx = self.row_context(list, None, false, PIN_COLUMN, cx);
                let pins: std::rc::Rc<[Message]> = list.iter().cloned().collect();
                let rows = gpui_kit::list(self.pins_list.clone(), move |ix, _, cx| div().pb(px(8.)).child(pin_card(&pins[ix], &ctx, cx)).into_any_element())
                    .with_sizing_behavior(gpui_kit::ListSizingBehavior::Infer)
                    .max_h(px(500.))
                    .px_3();
                div().relative().child(rows).child(gpui_kit::component::scroll::Scrollbar::vertical(&self.pins_list)).into_any_element()
            }
        };
        let card = crate::ui::surface(cx)
            .id("pins")
            .w(px(420.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().px_4().py_3().flex().items_center().gap_2().child(Icon::new(Glyph::Pin).size(px(20.))).child(div().font_weight(FontWeight::SEMIBOLD).child("Pinned Messages")))
            .child(body);
        // Under the header's pin button: its top-right corner just right of the click, kept in the window.
        let at = point(self.popover_at.x + px(16.), px(crate::layout::HEADER_HEIGHT - 2.));
        Some(backdrop("pins-backdrop", cx).child(anchored_at(at, gpui_kit::Anchor::TopRight, ui::rise(card.transform_origin(ui::corner(true, false)), "pins", Duration::ZERO))).into_any_element())
    }


    // The emoji picker (messages.css `.picker`): search on top, categories down the left, the grid in
    // the middle and the hovered emoji named at the foot. Recent first, then the server's own, then Unicode.
    // For a reaction it opens above and left of the click, kept in the window; from the composer it's the
    // composer picker's Emoji tab.
    fn emoji_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if matches!(self.emoji_picker.as_ref()?, EmojiTarget::Composer) {
            return None;
        }
        let picker = picker_frame("emoji-picker", PICKER_SIZE.1, cx).children(self.emoji_content(window, cx));
        Some(backdrop("emoji-backdrop", cx).child(anchored_at(self.popover_at, gpui_kit::Anchor::BottomRight, ui::rise(picker.transform_origin(ui::corner(true, true)), "emoji-picker", Duration::ZERO))).into_any_element())
    }

    // Search, the category rail beside the grid, and the hovered emoji named at the foot.
    fn emoji_content(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let sections = self.emoji_sections(cx);
                let scroll = window.use_keyed_state("emoji-scroll-handle", cx, |_, _| ScrollHandle::new()).read(cx).clone();
        let hovered = window.use_keyed_state("emoji-hovered", cx, |_, _| None::<(Emoji, SharedString)>);
        let preview = hovered.read(cx).clone().or_else(|| sections.first().and_then(|s| Some((s.items.first()?.clone(), s.source.clone()))));
        let in_view = scroll.top_item();
        let rail = div()
            .id("emoji-rail")
            .w(px(48.))
            .flex_none()
            .px(px(6.))
            .pb(px(6.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(2.))
            .overflow_y_scroll()
            .border_r_1()
            .border_color(palette::line(cx))
            .children(sections.iter().enumerate().flat_map(|(i, section)| {
                let (selected, hover, strong) = (palette::selected(cx), palette::hover(cx), palette::strong(cx));
                let scroll = scroll.clone();
                // A line sets the server's own emoji apart from the Unicode groups after them.
                let after_server = i > 0 && matches!(sections[i - 1].lead, SectionLead::Server { .. }) && !matches!(section.lead, SectionLead::Server { .. });
                let line = after_server.then(|| div().w(px(20.)).h(px(1.)).my(px(4.)).flex_none().bg(palette::line(cx)).into_any_element());
                let tab = div()
                    .id(("emoji-cat", i))
                    .size(px(36.))
                    .flex_none()
                    .squircle(RADIUS_SM)
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .text_color(if i == in_view { strong } else { palette::muted(cx) })
                    .when(i == in_view, |d| d.bg(selected))
                    .when(i != in_view, |d| d.hover(move |s| s.bg(hover).text_color(strong)))
                    .child(section.lead.render(cx))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        scroll.scroll_to_top_of_item(i);
                        cx.notify();
                    }));
                line.into_iter().chain([tab.into_any_element()])
            }));
        let cell = |emoji: Emoji, source: SharedString, key: String| {
            let face = match emoji.url() {
                Some(url) => img(url).size(px(28.)).into_any_element(),
                None => div().text_size(px(26.)).line_height(px(26.)).child(emoji.name.clone()).into_any_element(),
            };
            let (picked, shown) = (emoji.clone(), (emoji, source));
            let hovered = hovered.clone();
            div()
                .id(SharedString::from(key))
                .w(relative(1. / 9.))
                .h(px(40.))
                .squircle(RADIUS_SM)
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|s| s.bg(palette::selected(cx)))
                .child(face)
                .on_hover(move |on: &bool, _, cx| {
                    if *on {
                        hovered.update(cx, |h, cx| {
                            *h = Some(shown.clone());
                            cx.notify();
                        });
                    }
                })
                .on_click(cx.listener(move |this, _, window, cx| this.pick_emoji(picked.clone(), window, cx)))
        };
        let grid = div()
            .id("emoji-scroll")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .px(px(8.))
            .pb(px(8.))
            .children(sections.into_iter().map(|section| {
                // GAP: the section label sticks to the top of the grid in the design; gpui has no sticky.
                div()
                    .flex()
                    .flex_col()
                    .mb(px(4.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .py(px(6.))
                            .px(px(4.))
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette::muted(cx))
                            .child(section.lead.small(cx))
                            .child(section.name.clone()),
                    )
                    .child(div().flex().flex_wrap().children(section.items.into_iter().enumerate().map(|(i, e)| cell(e, section.source.clone(), format!("emoji-{}-{i}", section.name)))))
            }));
        let foot = div()
            .min_h(px(52.))
            .flex_none()
            .py(px(6.))
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(10.))
            .border_t_1()
            .border_color(palette::line(cx))
            .children(preview.map(|(emoji, source)| {
                let face = match emoji.url() {
                    Some(url) => img(url).size(px(28.)).into_any_element(),
                    None => div().text_size(px(28.)).line_height(px(28.)).child(emoji.name.clone()).into_any_element(),
                };
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .min_w_0()
                    .child(div().w(px(32.)).flex_none().flex().justify_center().child(face))
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().truncate().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(crate::message_menu::shortcode(&emoji)))
                            .child(div().text_size(px(12.)).text_color(palette::muted(cx)).child(source)),
                    )
            }));
        // GAP: the design's skin-tone button sits beside search; Waffle has no skin tones yet.
        let head = div().p(px(10.)).child(ui::search_field("emoji-search", &self.emoji_search, cx));
        vec![
            head.into_any_element(),
            div().flex_1().min_h_0().flex().child(rail).child(div().flex_1().min_w_0().h_full().flex().relative().child(grid).child(gpui_kit::component::scroll::Scrollbar::vertical(&scroll))).into_any_element(),
            foot.into_any_element(),
        ]
    }

    // What the picker lists for the current search: recent (only before you search), the server's own, then Unicode by group.
    fn emoji_sections(&self, cx: &App) -> Vec<EmojiSection> {
        let query = self.emoji_search.read(cx).value().to_lowercase();
        let unicode: SharedString = "Unicode emoji".into();
        let mut sections = Vec::new();
        if query.is_empty() {
            let recent = self.prefs.frequent_emojis(18, &crate::message_menu::QUICK_PAD);
            sections.push(EmojiSection { lead: SectionLead::Glyph(Glyph::Recent), name: "Recently used".into(), source: unicode.clone(), items: recent });
        }
        if let Some(guild) = self.guild() {
            let custom: Vec<Emoji> = guild.emojis.iter().filter(|e| query.is_empty() || e.name.to_lowercase().contains(&query)).cloned().collect();
            if !custom.is_empty() {
                let name: SharedString = guild.name.clone().into();
                sections.push(EmojiSection { lead: SectionLead::Server { icon: guild.icon.clone(), name: guild.name.clone() }, name: name.clone(), source: name, items: custom });
            }
        }
        for group in emojis::Group::iter() {
            let items: Vec<Emoji> = group
                .emojis()
                .filter(|e| query.is_empty() || e.name().contains(&query) || e.shortcodes().any(|s| s.contains(&query)))
                .map(|e| Emoji::unicode(e.as_str()))
                .collect();
            if !items.is_empty() {
                let (name, glyph) = group_name(group);
                sections.push(EmojiSection { lead: SectionLead::Glyph(glyph), name: name.into(), source: unicode.clone(), items });
            }
        }
        sections
    }

    // The quick switcher (navigation.css `.switcher`): a focused field, results grouped by kind with
    // the match in bold, arrows to pick, Enter to go.
    fn switcher_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let selected = self.switcher?;
        let query = self.switcher_input.read(cx).value().to_string();
        let results = self.switcher_results(cx);
        let mut list: Vec<AnyElement> = Vec::new();
        let mut last_group = None;
        for (i, (guild_id, channel, place)) in results.into_iter().enumerate() {
            let group = if channel.recipient.is_some() || channel.kind == ChannelKind::Dm { "People" } else { "Channels" };
            if last_group != Some(group) {
                list.push(div().pt(px(10.)).px(px(10.)).pb(px(4.)).text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(palette::faint(cx)).child(group).into_any_element());
                last_group = Some(group);
            }
            list.push(self.switcher_row(i == selected, guild_id, channel, place, &query, cx).into_any_element());
        }
        let is_empty = list.is_empty();
        let field = div().flex_none().child(ui::search_field("switcher-search", &self.switcher_input, cx));
        let hint = |keys: Vec<AnyElement>, label: &'static str| div().flex().items_center().gap_1().children(keys).child(label);
        let key = |glyph: Glyph| crate::ui::kbd("", cx).child(Icon::new(glyph).size(px(12.))).into_any_element();
        let foot = div()
            .pt(px(10.))
            .px(px(10.))
            .pb(px(4.))
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x_4()
            .gap_y_2()
            .border_t_1()
            .border_color(palette::line(cx))
            .text_size(px(12.))
            .text_color(palette::muted(cx))
            .when(!is_empty, |f| f.child(hint(vec![key(Glyph::ChevronUp), key(Glyph::ChevronDown)], "Move")).child(hint(vec![crate::ui::kbd("Enter", cx).into_any_element()], "Open")))
            .child(hint(vec![crate::ui::kbd("Esc", cx).into_any_element()], "Close"));
        let body: AnyElement = if is_empty {
            crate::ui::empty_state("switcher-empty", crate::mascot::Mood::Waiting, format!("Nothing called “{query}”"), "Check the spelling, or search messages in a channel instead.", cx).into_any_element()
        } else {
            div().id("switcher-list").flex().flex_col().gap(px(2.)).max_h(px(380.)).overflow_y_scroll().children(list).into_any_element()
        };
        let body = div()
            .p(px(10.))
            .flex()
            .flex_col()
            .gap_2()
            .capture_key_down(cx.listener(Self::on_switcher_key))
            .child(field)
            .child(body)
            .child(foot);
        Some(
            backdrop("switcher-backdrop", cx)
                .bg(palette::scrim(cx))
                .child(div().absolute().top(px(120.)).left_0().right_0().flex().justify_center().child(ui::dialog("switcher", SWITCHER_WIDTH, body, window, cx)))
                .into_any_element(),
        )
    }

    // A channel's mark in a list of places: the person with their status, a group's faces, else the kind's glyph.
    pub(crate) fn channel_lead(&self, channel: &Channel, cx: &App) -> AnyElement {
        match (&channel.recipient, channel.kind) {
            (Some(user), _) => {
                let status = self.presences.get(&user.id).map(|p| p.mark());
                avatar_with_status(&user.avatar, 20., status, palette::surface(palette::Level::Overlay, cx))
            }
            (None, ChannelKind::Dm) => ui::dm_face(&channel.face(), 20., palette::surface(palette::Level::Overlay, cx)),
            (None, kind) => {
                let glyph = match kind {
                    ChannelKind::Dm => Glyph::User,
                    ChannelKind::Voice => Glyph::VolumeHigh,
                    _ if channel.private => Glyph::HashtagLock,
                    _ => Glyph::Hashtag,
                };
                div().text_color(palette::faint(cx)).child(Icon::new(glyph).size(px(18.))).into_any_element()
            }
        }
    }

    fn switcher_row(&self, active: bool, guild_id: Option<String>, channel: Channel, place: SharedString, query: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let lead = self.channel_lead(&channel, cx);
        let name: SharedString = channel.name.clone().into();
        let strong = palette::strong(cx);
        let mark = name.to_lowercase().find(&query.to_lowercase()).filter(|_| !query.is_empty()).map(|at| at..at + query.len()).filter(|r| name.is_char_boundary(r.start) && name.is_char_boundary(r.end));
        let label = StyledText::new(name.clone()).with_highlights(mark.map(|r| (r, HighlightStyle { color: Some(strong), font_weight: Some(FontWeight::SEMIBOLD), ..Default::default() })));
        let hover = palette::hover(cx);
        div()
            .id(SharedString::from(format!("switch-{}", channel.id)))
            .h(px(36.))
            .px(px(10.))
            .flex_none()
            .squircle(RADIUS_SM)
            .flex()
            .items_center()
            .gap(px(10.))
            .cursor_pointer()
            .text_color(if active { strong } else { palette::text(cx) })
            .when(active, |r| r.bg(palette::selected(cx)))
            .when(!active, |r| r.hover(move |s| s.bg(hover)))
            .child(lead)
            .child(div().min_w_0().truncate().child(label))
            .child(div().ml_auto().pl_3().flex_none().text_size(px(12.)).text_color(palette::faint(cx)).child(place))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.switcher = None;
                this.guild_id = guild_id.clone();
                this.open_channel(channel.clone(), window, cx);
            }))
    }
}

struct EmojiSection {
    lead: SectionLead,
    name: SharedString,
    // What the foot says an emoji from here is.
    source: SharedString,
    items: Vec<Emoji>,
}

// Recent and the Unicode groups are marked by an icon, a server's own emoji by its icon.
enum SectionLead {
    Glyph(Glyph),
    Server { icon: Option<String>, name: String },
}

impl SectionLead {
    fn render(&self, cx: &App) -> AnyElement {
        match self {
            SectionLead::Glyph(glyph) => Icon::new(*glyph).size(px(18.)).into_any_element(),
            SectionLead::Server { icon, name } => ui::server_tile(icon.as_deref(), name, 24., cx),
        }
    }

    fn small(&self, cx: &App) -> AnyElement {
        match self {
            SectionLead::Glyph(glyph) => Icon::new(*glyph).size(px(14.)).into_any_element(),
            SectionLead::Server { icon, name } => ui::server_tile(icon.as_deref(), name, 16., cx),
        }
    }
}

// What someone's up to, as the popout's status line says it.
// A full-window catcher: clicking outside the floating surface closes it.
/// A popover placed with one corner at `at` (in window coordinates), shifted to stay 8px inside the window.
// The mention cards' shape while they load: the place line, avatar and name, two lines of text.
fn mention_skeletons(cx: &App) -> AnyElement {
    let card = |i: usize, widths: (f32, f32, f32)| {
        div()
            .p_3()
            .squircle(RADIUS_XS)
            .bg(palette::surface(palette::Level::Raised, cx))
            .flex()
            .flex_col()
            .gap_2()
            .child(ui::skeleton(("mention-skel-place", i), RADIUS_XS, cx).h(px(10.)).w(px(widths.0)))
            .child(div().flex().items_center().gap_2().child(ui::skeleton_circle(("mention-skel-avatar", i), 24., cx)).child(ui::skeleton(("mention-skel-name", i), RADIUS_XS, cx).h(px(12.)).w(px(widths.1))))
            .child(ui::skeleton(("mention-skel-text", i), RADIUS_XS, cx).h(px(12.)).w(gpui_kit::relative(widths.2)))
    };
    div().flex().flex_col().gap_2().child(card(0, (140., 90., 0.85))).child(card(1, (110., 70., 0.6))).child(card(2, (160., 100., 0.75))).into_any_element()
}

pub(crate) fn anchored_at(at: gpui_kit::Point<gpui_kit::Pixels>, corner: gpui_kit::Anchor, popover: impl IntoElement) -> AnyElement {
    gpui_kit::anchored().position(at).anchor(corner).snap_to_window_with_margin(px(8.)).child(popover).into_any_element()
}

// A picker's floating card: the overlay surface, rounded and shadowed, its content laid out top to bottom.
fn picker_frame(id: &'static str, height: f32, cx: &App) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .w(px(PICKER_SIZE.0))
        .h(px(height))
        .squircle(RADIUS_LG)
        .bg(palette::surface(palette::Level::Overlay, cx))
        .shadow(ui::shadow_pop(cx))
        .flex()
        .flex_col()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

pub(crate) fn backdrop(id: &'static str, cx: &mut Context<Waffle>) -> gpui_kit::Stateful<gpui_kit::Div> {
    div().id(id).absolute().inset_0().occlude().on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _, _, cx| {
            this.dismiss_overlay(cx);
        }),
    )
}

// A Unicode group's name, and the icon that marks it in the picker's rail.
fn group_name(group: emojis::Group) -> (&'static str, Glyph) {
    use emojis::Group::*;
    match group {
        SmileysAndEmotion => ("Smileys & Emotion", Glyph::Emoji),
        PeopleAndBody => ("People", Glyph::HandWaving),
        AnimalsAndNature => ("Nature", Glyph::Leaf),
        FoodAndDrink => ("Food & Drink", Glyph::Hamburger),
        TravelAndPlaces => ("Travel & Places", Glyph::Airplane),
        Activities => ("Activities", Glyph::SoccerBall),
        Objects => ("Objects", Glyph::Lightbulb),
        Symbols => ("Symbols", Glyph::Heart),
        Flags => ("Flags", Glyph::Flag),
    }
}


// Search's hit column less the pin card's Jump and Unpin slot.
const PIN_COLUMN: f32 = crate::search_results::HIT_COLUMN - 100.;

// A pin, with Jump and Unpin at its top right on hover, as Discord's, in a slot of their own so the head never
// runs under them. Unpin asks first unless Shift is held.
fn pin_card(m: &Message, ctx: &std::rc::Rc<crate::chat::RowContext>, cx: &mut App) -> AnyElement {
    let group = SharedString::from(format!("pin-{}", m.id));
    let (this, id) = (ctx.this(), m.id.clone());
    let jump = ui::button(SharedString::from(format!("pin-jump-{}", m.id)), ui::Tone::Secondary, cx).small().child("Jump").on_click(move |_, _, cx| {
        this.update(cx, |this, cx| {
            this.pins = None;
            this.jump_to(id.clone(), cx);
        })
        .ok();
    });
    let (this, pinned) = (ctx.this(), m.clone());
    let unpin = ui::icon_button(SharedString::from(format!("pin-unpin-{}", m.id)), Glyph::X, "Unpin", cx).small().on_click(move |_, window, cx| {
        this.update(cx, |this, cx| this.run_message_action(crate::message_menu::MessageAction::Pin, &pinned, false, window, cx)).ok();
    });
    div()
        .id(group.clone())
        .group(group.clone())
        .flex()
        .items_start()
        .gap(px(8.))
        .p(px(12.))
        .squircle(RADIUS_MD)
        .bg(palette::surface(palette::Level::Page, cx))
        .border_1()
        .border_color(palette::line(cx))
        .child(crate::chat::search_hit(m, ctx, cx).flex_1().min_w_0())
        .child(div().flex_none().mt(px(-4.)).mr(px(-4.)).flex().gap(px(4.)).opacity(0.).group_hover(group, |s| s.opacity(1.)).child(jump).child(unpin))
        .into_any_element()
}

// Discord's Mark Unread: it acks the newest loaded message before the one marked (or the millisecond
// before it) and counts the loaded messages after that ack which `counts` as mentions.
fn manual_ack(loaded: &[Message], marked: u64, counts: impl Fn(&Message) -> bool) -> (u64, u32) {
    let ack = loaded.iter().map(|m| crate::order(&m.id)).filter(|id| *id < marked).max().unwrap_or_else(|| previous_millisecond(marked));
    let mentions = loaded.iter().filter(|m| crate::order(&m.id) > ack && counts(m)).count();
    (ack, mentions as u32)
}

// The lowest snowflake one millisecond before `id`: the timestamp sits above bit 22.
fn previous_millisecond(id: u64) -> u64 {
    ((id >> 22).saturating_sub(1)) << 22
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(id: u64, mentions_me: bool) -> Message {
        Message { id: id.to_string(), content: if mentions_me { "@me".into() } else { String::new() }, ..Default::default() }
    }

    #[test]
    fn previous_millisecond_drops_a_millisecond_and_the_low_bits() {
        let id = (1_000 << 22) | 0x3F_FFFF;
        assert_eq!(previous_millisecond(id), 999 << 22);
        assert_eq!(previous_millisecond(5), 0, "an id in the epoch's first millisecond can't go lower");
    }

    #[test]
    fn mark_unread_acks_the_message_before_and_counts_mentions_after_it() {
        let mentions = |m: &Message| !m.content.is_empty();
        let loaded = [message(10 << 22, true), message(20 << 22, false), message(30 << 22, true), message(40 << 22, true)];
        assert_eq!(manual_ack(&loaded, 30 << 22, mentions), (20 << 22, 2), "the newest older message, and the marked one counts");
        assert_eq!(manual_ack(&loaded, 20 << 22, mentions), (10 << 22, 2));
        assert_eq!(manual_ack(&loaded, 10 << 22, mentions), (9 << 22, 3), "nothing older loaded: the millisecond before");
        assert_eq!(manual_ack(&[], 10 << 22, mentions), (9 << 22, 0));
    }
}
