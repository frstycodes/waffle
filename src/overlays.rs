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
    Channel, ChannelKind, Waffle, part::Column, EmojiTarget, SearchState,
    discord::{Emoji, Message, User},
    icons::Glyph,
    illustration::Illustration,
    layout::avatar_with_status,
    palette,
    ui::{self, RADIUS_MD, RADIUS_SM, RADIUS_XS, Squircle},
};

mod emoji;
mod read;
mod switcher;

const PICKER_SIZE: (f32, f32) = (420., 440.);
// The composer's pickers share one frame, tall enough for the tabs over the GIF picker's.
const COMPOSER_PICKER_HEIGHT: f32 = PICKER_SIZE.1 + 100.;
const SWITCHER_WIDTH: f32 = 560.;
// Where the quick switcher hangs from the window's top, and how tall its results list grows before it scrolls.
const SWITCHER_TOP: f32 = 120.;
const SWITCHER_LIST_HEIGHT: f32 = 380.;
// The Inbox and the pins popover share a width; each caps its height and scrolls inside.
const LIST_POPOVER_WIDTH: f32 = 420.;
const INBOX_MAX_HEIGHT: f32 = 560.;
const PINS_MAX_HEIGHT: f32 = 500.;
// The pins popover opens this far right of the click, so its corner sits by the header's pin button.
const PINS_NUDGE: f32 = 16.;
// How far a mention card's actions reach past the author row, so their buttons don't stretch it.
const ACTION_BLEED: f32 = 3.;
// The emoji picker's glyph size, and the foot that names the hovered emoji.
const EMOJI_GLYPH: f32 = 26.;
const EMOJI_FOOT_HEIGHT: f32 = 52.;

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
        // lint-allow(detached-task): ends when the DM request returns, and holds only a WeakEntity.
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
        cx.notify(); // lint-allow(global-notify): the draft shows in the chat column and the closed profile card in the overlay layer
    }

    pub(crate) fn message_link(&self, message: &Message) -> String {
        let place = message.guild_id.clone().or_else(|| self.guild_id.clone()).unwrap_or_else(|| "@me".into());
        format!("https://discord.com/channels/{place}/{}/{}", message.channel_id, message.id)
    }

    /// Closes whichever floating surface is open; true if one was.
    pub(crate) fn dismiss_overlay(&mut self, cx: &mut Context<Self>) -> bool {
        // The note's picker sits over the forward dialog: closing it leaves the dialog and its picks.
        if matches!(self.emoji_picker, Some(EmojiTarget::ForwardNote | EmojiTarget::VoiceStatus)) {
            self.emoji_picker = None;
            self.redraw_overlays(cx);
            return true;
        }
        // A profile's popover (role picker, note, reply) closes alone, back to its card.
        if self.close_card_pop(cx) {
            return true;
        }
        // The full-size avatar closes alone, back to the popout it opened from.
        if let Some(card) = self.profile_card.as_mut().filter(|c| c.avatar_full) {
            card.avatar_full = false;
            self.redraw_overlays(cx);
            return true;
        }
        let text_viewer = self.text_files.borrow_mut().viewer.take().is_some();
        let open = text_viewer
            | self.role_popout.close()
            | self.profile_card.take().is_some()
            | std::mem::take(&mut self.status_menu)
            | self.status_editor.take().is_some()
            | self.poll_creator.take().is_some()
            | self.take_app_modal()
            | self.close_events()
            | self.forward_dialog.take().is_some()
            | self.voice_status_dialog.take().is_some()
            | self.dismiss_announcement_dialog()
            | self.report_dialog.take().is_some()
            | self.group_dialog.take().is_some()
            | self.reactions_dialog.take().is_some()
            | self.emoji_picker.take().is_some()
            | self.switcher.take().is_some()
            | self.close_share_picker(cx)
            | self.pins.take().is_some()
            | std::mem::take(&mut self.inbox_open)
            | self.gif_picker.take().is_some()
            | self.soundboard.close_picker()
            | self.dismiss_join_overlay()
            | self.dismiss_call_menus()
            | self.close_menu();
        if open {
            cx.notify(); // lint-allow(global-notify): closes whichever of many surfaces was open, some drawn in the columns
        }
        open
    }

    pub(crate) fn render_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::new();
        // Under the profile, which its rows open.
        out.extend(self.role_popout_overlay(window, cx));
        out.extend(self.profile_overlay(window, cx));
        out.extend(self.text_viewer_overlay(window, cx));
        out.extend(self.status_menu_overlay(cx));
        out.extend(self.status_editor_overlay(window, cx));
        out.extend(self.poll_creator_overlay(window, cx));
        out.extend(self.app_modal_overlay(window, cx));
        out.extend(self.events_overlay(window, cx));
        out.extend(self.forward_overlay(window, cx));
        out.extend(self.voice_status_overlay(window, cx));
        out.extend(self.announcement_overlay(window, cx));
        out.extend(self.report_overlay(window, cx));
        out.extend(self.group_overlay(window, cx));
        out.extend(self.reactions_overlay(window, cx));
        out.extend(self.emoji_overlay(window, cx));
        out.extend(self.switcher_overlay(window, cx));
        out.extend(self.share_overlay(window, cx));
        out.extend(self.pins_overlay(cx));
        out.extend(self.inbox_overlay(window, cx));
        out.extend(self.composer_picker_overlay(window, cx));
        out.extend(self.soundboard_overlay(window, cx));
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
                        .w(px(LIST_POPOVER_WIDTH))
                        .max_h(px(INBOX_MAX_HEIGHT.min(crate::ui::room_above(window, f32::from(self.user_panel_clearance()) + 4.))))
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
            Some(Some(Ok(list))) => {
                let shown: Vec<Message> = list.iter().filter(|m| self.hidden_author(&m.author.id).is_none()).take(limit.unwrap_or(usize::MAX)).cloned().collect();
                if shown.is_empty() {
                    return crate::ui::empty_state("messages-empty", Illustration::InboxEmpty, empty.0, empty.1, cx).into_any_element();
                }
                let ctx = self.row_context_everywhere(&shown, 380., cx);
                // The demo shows the first card as hovered, its actions up.
                let demo = cfg!(debug_assertions) && std::env::var("WAFFLE_DEMO").as_deref() == Ok("inbox-mark-read");
                ui::col().gap_2().children(shown.iter().enumerate().map(|(i, m)| self.mention_card(m, &ctx, demo && i == 0, cx))).into_any_element()
            }
        }
    }

    // A mention, opening its message on click; on hover Jump and Discord's checkmark, Mark As Read, sit in a
    // slot of their own beside the author.
    fn mention_card(&self, m: &Message, ctx: &std::rc::Rc<crate::chat::RowContext>, revealed: bool, cx: &mut Context<Self>) -> ui::Control {
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
        // lint-allow(raw-margin): the 30px buttons reach past the 24px author row so they don't stretch it, which padding can't do.
        let actions = ui::row().flex_none().my(px(-ACTION_BLEED)).gap_0p5().child(jump).child(read);
        let actions = if revealed { actions } else { actions.opacity(0.).group_hover(group.clone(), |s| s.opacity(1.)) };
        let author = ui::row().gap_2().child(ui::avatar(m.author.avatar.clone(), 24.)).child(ui::line(m.author.name.clone()).flex_1().font_weight(FontWeight::NORMAL)).child(actions);
        ui::card(group.clone(), cx)
            .group(group)
            .stacked()
            .squircle(RADIUS_XS)
            .gap_1()
            .child(div().text_size(ui::TEXT_XS).text_color(cx.theme().muted_foreground).child(place))
            .child(author)
            .child(div().text_size(ui::TEXT_SM).line_clamp(3).children(crate::chat::content(m, ctx, cx)))
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
        let picker = picker_frame("composer-picker", COMPOSER_PICKER_HEIGHT.min(ui::room_above(window, self.composer_popover_bottom())), cx)
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
        div().px_3().pt_1().child(ui::tabs("picker-tabs", &items, selected, move |i, window, cx| {
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
            // The composer's buttons show which picker is open.
            self.redraw(Column::Main, cx);
            return self.redraw_overlays(cx);
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
            Some(Ok(gifs)) => ui::row().items_stretch()
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
            div().p_2p5().child(ui::search_field("gif-search", &self.gif_search, cx)).into_any_element(),
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
                let rows = gpui_kit::list(self.pins_list.clone(), move |ix, _, cx| div().pb_2().child(pin_card(&pins[ix], &ctx, cx)).into_any_element())
                    .with_sizing_behavior(gpui_kit::ListSizingBehavior::Infer)
                    .max_h(px(PINS_MAX_HEIGHT))
                    .px_3();
                div().relative().child(rows).child(gpui_kit::component::scroll::Scrollbar::vertical(&self.pins_list)).into_any_element()
            }
        };
        let card = crate::ui::surface(cx)
            .id("pins")
            .w(px(LIST_POPOVER_WIDTH))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(ui::row().px_4().py_3().gap_2().child(Icon::new(Glyph::Pin).size(ui::ICON_XL)).child(div().font_weight(FontWeight::SEMIBOLD).child("Pinned Messages")))
            .child(body);
        // Under the header's pin button: its top-right corner just right of the click, kept in the window.
        let at = point(self.popover_at.x + px(PINS_NUDGE), px(crate::layout::HEADER_HEIGHT - 2.));
        Some(backdrop("pins-backdrop", cx).child(ui::floating_at("pins-float", at, gpui_kit::Anchor::TopRight, ui::rise(card.transform_origin(ui::corner(true, false)), "pins", Duration::ZERO))).into_any_element())
    }

}

// What someone's up to, as the popout's status line says it.
// A full-window catcher: clicking outside the floating surface closes it.
/// A popover placed with one corner at `at` (in window coordinates), shifted to stay 8px inside the window.
// The mention cards' shape while they load: the place line, avatar and name, two lines of text.
fn mention_skeletons(cx: &App) -> AnyElement {
    let card = |i: usize, widths: (f32, f32, f32)| {
        ui::col()
            .p_3()
            .squircle(RADIUS_XS)
            .bg(palette::surface(palette::Level::Raised, cx))
            .gap_2()
            .child(ui::skeleton(("mention-skel-place", i), RADIUS_XS, cx).h_2p5().w(px(widths.0)))
            .child(ui::row().gap_2().child(ui::skeleton_circle(("mention-skel-avatar", i), 24., cx)).child(ui::skeleton(("mention-skel-name", i), RADIUS_XS, cx).h_3().w(px(widths.1))))
            .child(ui::skeleton(("mention-skel-text", i), RADIUS_XS, cx).h_3().w(gpui_kit::relative(widths.2)))
    };
    ui::col().gap_2().child(card(0, (140., 90., 0.85))).child(card(1, (110., 70., 0.6))).child(card(2, (160., 100., 0.75))).into_any_element()
}

// A picker's floating card: the overlay surface, rounded and shadowed, its content laid out top to bottom.
fn picker_frame(id: &'static str, height: f32, cx: &App) -> gpui_kit::Stateful<gpui_kit::Div> {
    ui::surface(cx)
        .id(id)
        .p_0()
        .gap_0()
        .w(px(PICKER_SIZE.0))
        .h(px(height))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

pub(crate) use crate::ui::rich_line;

pub(crate) fn backdrop(id: &'static str, cx: &mut Context<Waffle>) -> gpui_kit::Stateful<gpui_kit::Div> {
    div().id(id).absolute().inset_0().occlude().on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _, _, cx| {
            this.dismiss_overlay(cx);
        }),
    )
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
    ui::row()
        .id(group.clone())
        .group(group.clone())
        .items_start()
        .gap_2()
        .p_3()
        .squircle(RADIUS_MD)
        .bg(palette::surface(palette::Level::Page, cx))
        .border_1()
        .border_color(palette::line(cx))
        .child(crate::chat::search_hit(m, ctx, cx).flex_1().min_w_0())
        // lint-allow(raw-margin): the actions pull into the card's corner past its padding, which padding can't do.
        .child(ui::row().items_stretch().flex_none().mt_neg_1().mr_neg_1().gap_1().opacity(0.).group_hover(group, |s| s.opacity(1.)).child(jump).child(unpin))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::read::{manual_ack, previous_millisecond};

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
