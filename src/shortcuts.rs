// Discord's in-app keyboard shortcuts (docs/discord-settings-re.md §4). One table binds the keys, fills the
// Keyboard combos dialog (⌘/) and lists them under Settings › Keybinds, so the three can't drift. Every binding
// has no key context: gpui ranks those above a focused field's own keys, so each handler that only sometimes
// applies hands the key back with `cx.propagate()`.
use gpui_kit::{
    AnyElement, App, Context, Div, FontWeight, InteractiveElement, IntoElement, KeyBinding, MouseButton, ParentElement, SharedString, Stateful, StatefulInteractiveElement, Styled, Window,
    actions, component::Icon, div, point, px,
};

use crate::{
    RightPanel, Waffle,
    discord::{Channel, ChannelKind},
    feed_keys::platform_combo,
    icons::Glyph,
    message_menu::MessageAction,
    overlays::ComposerPicker,
    palette,
    settings::key_caps,
    ui::{self, Tone},
};

actions!(
    waffle,
    [
        ServerPrev, ServerNext, UnreadPrev, UnreadNext, MentionPrev, MentionNext, JumpToCall, ToggleLastServer, MarkServerRead, TogglePins,
        ToggleMembers, ToggleEmoji, ToggleGifs, ToggleStickers, ScrollUp, ScrollDown, JumpToUnread, UploadFile, CopyChannelLink, ToggleMute, ToggleDeafen, AnswerCall,
        DeclineCall, StartCall, GetHelp, SearchFresh, ToggleCombos,
    ]
);

#[derive(Clone, PartialEq, Debug, gpui_kit::Action)]
#[action(namespace = waffle, no_json)]
pub struct JumpToServer(usize);

const HELP_URL: &str = "https://support.discord.com";
pub(crate) const COMBOS_KEY: &str = "secondary+/";
const WIDE_DIALOG: f32 = 880.;
const NARROW_DIALOG: f32 = 480.;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Group {
    Messages,
    Navigation,
    VoiceAndVideo,
    Chat,
    Miscellaneous,
}

impl Group {
    // Discord's dialog order.
    pub(crate) const ALL: [Group; 5] = [Group::Messages, Group::Navigation, Group::VoiceAndVideo, Group::Chat, Group::Miscellaneous];

    pub(crate) fn title(self) -> &'static str {
        match self {
            Group::Messages => "Messages",
            Group::Navigation => "Navigation",
            Group::VoiceAndVideo => "Voice and video",
            Group::Chat => "Chat",
            Group::Miscellaneous => "Miscellaneous",
        }
    }

    fn note(self) -> Option<&'static str> {
        (self == Group::Messages).then_some("These shortcuts work when focused on a message.")
    }
}

// A combo in feed_keys' `secondary+shift+m` form, and the binding it makes, if this module makes it.
struct Key {
    combo: &'static str,
    bind: Option<fn(&str) -> KeyBinding>,
    shown: bool,
}

// Bound here and shown.
const fn on(combo: &'static str, bind: fn(&str) -> KeyBinding) -> Key {
    Key { combo, bind: Some(bind), shown: true }
}

// Bound here, but Discord doesn't show it.
const fn also(combo: &'static str, bind: fn(&str) -> KeyBinding) -> Key {
    Key { combo, bind: Some(bind), shown: false }
}

// Shown only: another module handles the key.
const fn listed(combo: &'static str) -> Key {
    Key { combo, bind: None, shown: true }
}

fn bind<A: gpui_kit::Action + Default>(combo: &str) -> KeyBinding {
    KeyBinding::new(&combo.replace('+', "-"), A::default(), None)
}

fn jump<const N: usize>(combo: &str) -> KeyBinding {
    KeyBinding::new(&combo.replace('+', "-"), JumpToServer(N), None)
}

struct Shortcut {
    group: Group,
    label: &'static str,
    keys: &'static [Key],
}

use Group::{Chat, Messages, Miscellaneous, Navigation, VoiceAndVideo};

// Discord's labels and binds, in its order within each group. Left out: Create or join a server (bound in
// app_keys), Create a private group (no group DM flow yet), Create a thread, soundboard, Lofi with Wumpus.
const SHORTCUTS: &[Shortcut] = &[
    Shortcut { group: Messages, label: "Focus text area", keys: &[listed("escape")] },
    Shortcut {
        group: Navigation,
        label: "Navigate between servers",
        keys: &[on("secondary+alt+up", bind::<ServerPrev>), on("secondary+alt+down", bind::<ServerNext>), also("ctrl+shift+tab", bind::<ServerPrev>), also("ctrl+tab", bind::<ServerNext>)],
    },
    Shortcut { group: Navigation, label: "Navigate between channels", keys: &[listed("alt+up"), listed("alt+down")] },
    Shortcut { group: Navigation, label: "Navigate forward and backward in page history", keys: HISTORY },
    Shortcut { group: Navigation, label: "Navigate between unread channels", keys: &[on("alt+shift+up", bind::<UnreadPrev>), on("alt+shift+down", bind::<UnreadNext>)] },
    Shortcut {
        group: Navigation,
        label: "Navigate between unread channels with mentions",
        keys: &[on("secondary+alt+shift+up", bind::<MentionPrev>), on("secondary+alt+shift+down", bind::<MentionNext>)],
    },
    Shortcut { group: Navigation, label: "Navigate to current call", keys: &[on("secondary+alt+shift+v", bind::<JumpToCall>)] },
    Shortcut { group: Navigation, label: "Toggle between last server and DMs", keys: &[on("secondary+alt+right", bind::<ToggleLastServer>)] },
    Shortcut { group: Navigation, label: "Toggle QuickSwitcher", keys: &[listed("secondary+k")] },
    Shortcut {
        group: Navigation,
        label: "Jump to server 1 to 9",
        keys: &[
            on("secondary+1", jump::<1>),
            also("secondary+2", jump::<2>),
            also("secondary+3", jump::<3>),
            also("secondary+4", jump::<4>),
            also("secondary+5", jump::<5>),
            also("secondary+6", jump::<6>),
            also("secondary+7", jump::<7>),
            also("secondary+8", jump::<8>),
            also("secondary+9", jump::<9>),
        ],
    },
    Shortcut { group: VoiceAndVideo, label: "Toggle mute", keys: &[on("secondary+shift+m", bind::<ToggleMute>)] },
    Shortcut { group: VoiceAndVideo, label: "Toggle deafen", keys: &[on("secondary+shift+d", bind::<ToggleDeafen>)] },
    Shortcut { group: VoiceAndVideo, label: "Answer incoming call", keys: &[on("secondary+enter", bind::<AnswerCall>)] },
    Shortcut { group: VoiceAndVideo, label: "Decline incoming call", keys: &[on("escape", bind::<DeclineCall>)] },
    // Literal Ctrl on macOS too, as in Discord.
    Shortcut { group: VoiceAndVideo, label: "Start call in private message or group", keys: &[on("ctrl+'", bind::<StartCall>)] },
    Shortcut { group: Chat, label: "Mark server read", keys: &[on("shift+escape", bind::<MarkServerRead>)] },
    Shortcut { group: Chat, label: "Mark channel as read", keys: &[listed("escape")] },
    Shortcut { group: Chat, label: "Toggle pins popout", keys: &[on("secondary+p", bind::<TogglePins>)] },
    Shortcut { group: Chat, label: "Toggle inbox popout", keys: &[listed("secondary+i")] },
    Shortcut { group: Chat, label: "Mark top inbox channel read", keys: &[listed("secondary+shift+e")] },
    Shortcut { group: Chat, label: "Toggle channel member list", keys: &[on("secondary+u", bind::<ToggleMembers>)] },
    Shortcut { group: Chat, label: "Toggle emoji picker", keys: &[on("secondary+e", bind::<ToggleEmoji>)] },
    Shortcut { group: Chat, label: "Toggle GIF picker", keys: &[on("secondary+g", bind::<ToggleGifs>)] },
    Shortcut { group: Chat, label: "Toggle sticker picker", keys: &[on("secondary+s", bind::<ToggleStickers>)] },
    Shortcut { group: Chat, label: "Scroll chat up or down", keys: &[on("pageup", bind::<ScrollUp>), on("pagedown", bind::<ScrollDown>)] },
    Shortcut { group: Chat, label: "Jump to oldest unread message", keys: &[on("shift+pageup", bind::<JumpToUnread>)] },
    Shortcut { group: Chat, label: "Upload a file", keys: &[on("secondary+shift+u", bind::<UploadFile>)] },
    Shortcut { group: Chat, label: "Copy channel link", keys: &[on("secondary+shift+l", bind::<CopyChannelLink>)] },
    Shortcut { group: Miscellaneous, label: "Get help", keys: &[on("secondary+shift+h", bind::<GetHelp>)] },
    // ⌘F is app_keys' FocusSearch; ⇧⌘F is the same field, which never prefills.
    Shortcut { group: Miscellaneous, label: "Search", keys: &[listed("secondary+f"), also("secondary+shift+f", bind::<SearchFresh>)] },
    Shortcut { group: Miscellaneous, label: "Keyboard combos", keys: &[on(COMBOS_KEY, bind::<ToggleCombos>)] },
    Shortcut { group: Miscellaneous, label: "Zoom in, out, reset", keys: &[listed("secondary+="), listed("secondary+-"), listed("secondary+0")] },
];

// layout.rs `history_step`.
const HISTORY: &[Key] = if cfg!(target_os = "macos") { &[listed("secondary+["), listed("secondary+]")] } else { &[listed("alt+left"), listed("alt+right")] };

pub fn init(cx: &mut App) {
    cx.bind_keys(SHORTCUTS.iter().flat_map(|s| s.keys).filter_map(|k| Some((k.bind?)(k.combo))));
}

// A row of the dialog or the settings list: the label and the combos it shows.
pub(crate) struct Row {
    pub label: &'static str,
    pub keys: Vec<&'static str>,
}

// Messages lead with feed_keys' own table, each action's first key as its menu shows it.
pub(crate) fn rows(group: Group) -> Vec<Row> {
    let messages = (group == Group::Messages).then(|| MessageAction::ALL.map(|a| Row { label: a.label(), keys: vec![a.keys()[0]] })).into_iter().flatten();
    let table = SHORTCUTS.iter().filter(|s| s.group == group).map(|s| Row { label: s.label, keys: s.keys.iter().filter(|k| k.shown).map(|k| k.combo).collect() });
    messages.chain(table).collect()
}

// Combos read "or", each in this platform's order and caps.
pub(crate) fn caps(keys: &[&str], cx: &App) -> Div {
    let muted = palette::muted(cx);
    div().flex().items_center().gap_1().flex_none().children(keys.iter().enumerate().flat_map(|(i, combo)| {
        let or = (i > 0).then(|| div().px_0p5().text_size(ui::TEXT_XS).text_color(muted).child("or").into_any_element());
        or.into_iter().chain([key_caps(&platform_combo(combo), cx).into_any_element()])
    }))
}

pub(crate) fn row(id: impl Into<SharedString>, row: &Row, line: bool, cx: &App) -> Stateful<Div> {
    ui::settings_row(id, line, cx).min_h_10().py_1p5().child(div().flex_1().min_w_0().text_color(palette::strong(cx)).child(row.label)).child(caps(&row.keys, cx))
}

// A group's heading inside a card (the settings list, where every group shares one card).
pub(crate) fn group_heading(group: Group, cx: &App) -> Div {
    div().px_3().pt_3p5().pb_1().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(group.title())
}

impl Waffle {
    pub(crate) fn listen_shortcuts(&self, root: Stateful<Div>, cx: &mut Context<Self>) -> Stateful<Div> {
        root.on_action(cx.listener(|this, _: &ServerPrev, window, cx| this.step_server(-1, window, cx)))
            .on_action(cx.listener(|this, _: &ServerNext, window, cx| this.step_server(1, window, cx)))
            .on_action(cx.listener(|this, _: &UnreadPrev, window, cx| this.step_unread(-1, false, window, cx)))
            .on_action(cx.listener(|this, _: &UnreadNext, window, cx| this.step_unread(1, false, window, cx)))
            .on_action(cx.listener(|this, _: &MentionPrev, window, cx| this.step_unread(-1, true, window, cx)))
            .on_action(cx.listener(|this, _: &MentionNext, window, cx| this.step_unread(1, true, window, cx)))
            .on_action(cx.listener(Self::jump_to_call))
            .on_action(cx.listener(Self::toggle_last_server))
            .on_action(cx.listener(Self::jump_to_server))
            .on_action(cx.listener(Self::mark_server_read))
            .on_action(cx.listener(Self::toggle_pins))
            .on_action(cx.listener(Self::toggle_members))
            .on_action(cx.listener(|this, _: &ToggleEmoji, window, cx| this.toggle_picker(ComposerPicker::Emoji, window, cx)))
            .on_action(cx.listener(|this, _: &ToggleGifs, window, cx| this.toggle_picker(ComposerPicker::Gifs, window, cx)))
            .on_action(cx.listener(|this, _: &ToggleStickers, window, cx| this.toggle_picker(ComposerPicker::Stickers, window, cx)))
            .on_action(cx.listener(|this, _: &ScrollUp, _, cx| this.scroll_chat(-1., cx)))
            .on_action(cx.listener(|this, _: &ScrollDown, _, cx| this.scroll_chat(1., cx)))
            .on_action(cx.listener(Self::jump_to_unread))
            .on_action(cx.listener(Self::upload_file))
            .on_action(cx.listener(Self::copy_channel_link))
            .on_action(cx.listener(|this, _: &ToggleMute, _, cx| this.toggle_mute(cx)))
            .on_action(cx.listener(|this, _: &ToggleDeafen, _, cx| this.toggle_deafen(cx)))
            .on_action(cx.listener(Self::answer_call))
            .on_action(cx.listener(Self::decline_call))
            .on_action(cx.listener(Self::start_call_here))
            .on_action(cx.listener(|_, _: &GetHelp, _, cx| cx.open_url(HELP_URL)))
            .on_action(cx.listener(|this, _: &SearchFresh, window, cx| this.focus_search(&crate::app_keys::FocusSearch, window, cx)))
            .on_action(cx.listener(|this, _: &ToggleCombos, _, cx| this.set_keyboard_combos(!this.keyboard_combos, cx)))
    }

    pub(crate) fn set_keyboard_combos(&mut self, open: bool, cx: &mut Context<Self>) {
        self.keyboard_combos = open;
        cx.notify();
    }

    // Something over the app that a shortcut mustn't reach under.
    fn covered(&self) -> bool {
        self.settings_open() || self.dialog.is_some() || self.keyboard_combos || self.viewer.is_some()
    }

    // The channel whose chat fills the main column, if nothing covers it.
    fn chat_shown(&self) -> Option<Channel> {
        if self.covered() || self.call_view || self.inbox_page {
            return None;
        }
        self.channel.clone().filter(|c| c.kind != ChannelKind::Forum)
    }

    // Servers in the rail's order, the DMs' home first.
    fn rail_places(&self) -> Vec<Option<String>> {
        let guilds = self.rail_entries().into_iter().flat_map(|entry| match entry {
            crate::RailEntry::Guild(i) => vec![i],
            crate::RailEntry::Folder(_, indices) => indices,
        });
        std::iter::once(None).chain(guilds.map(|i| Some(self.guilds[i].id.clone()))).collect()
    }

    fn step_server(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        if self.covered() {
            return;
        }
        let places = self.rail_places();
        let current = places.iter().position(|p| *p == self.guild_id).unwrap_or(0);
        let next = wrap(current, delta, places.len());
        self.open_guild(places[next].clone(), window, cx);
    }

    fn jump_to_server(&mut self, action: &JumpToServer, window: &mut Window, cx: &mut Context<Self>) {
        if self.covered() {
            return;
        }
        let Some(place) = self.rail_places().into_iter().nth(action.0) else { return };
        self.open_guild(place, window, cx);
    }

    // Not verified: whether Discord's unread steps cross servers; here they run through every place in the
    // rail's order, DMs first, wrapping at the ends. Muted channels count too.
    fn step_unread(&mut self, delta: isize, mentions: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.covered() {
            return;
        }
        let me = self.my_id().to_owned();
        let mut order: Vec<(Option<String>, Channel)> = self.dms.iter().map(|c| (None, c.clone())).collect();
        for guild in self.rail_places().into_iter().flatten().filter_map(|id| self.guilds.iter().find(|g| g.id == id)) {
            let text = crate::sidebar::channel_groups(guild, &me).into_iter().flat_map(|(_, c)| c).filter(|c| c.kind == ChannelKind::Text);
            order.extend(text.map(|c| (Some(guild.id.clone()), c)));
        }
        let wanted = |c: &Channel| if mentions { self.mention_count(&c.id) > 0 } else { self.unread(&c.id) };
        let current = self.channel.as_ref().and_then(|c| order.iter().position(|(_, o)| o.id == c.id));
        let Some(next) = next_match(current, delta, order.len(), |i| wanted(&order[i].1)) else { return };
        let (guild_id, channel) = order.swap_remove(next);
        self.guild_id = guild_id;
        self.open_channel(channel, window, cx);
    }

    fn jump_to_call(&mut self, _: &JumpToCall, window: &mut Window, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_ref().filter(|_| !self.covered()) else { return };
        let (channel, in_server) = (call.channel.clone(), call.guild_id.is_some());
        if in_server {
            return self.show_call(channel, window, cx);
        }
        self.open_dm_stage(channel, window, cx);
    }

    fn toggle_last_server(&mut self, _: &ToggleLastServer, window: &mut Window, cx: &mut Context<Self>) {
        if self.covered() {
            return;
        }
        if self.guild_id.is_some() {
            return self.open_guild(None, window, cx);
        }
        let Some(last) = self.history.last_guild() else { return };
        self.open_guild(Some(last), window, cx);
    }

    fn mark_server_read(&mut self, _: &MarkServerRead, _: &mut Window, cx: &mut Context<Self>) {
        let Some(guild_id) = self.guild_id.clone().filter(|_| !self.covered()) else { return cx.propagate() };
        self.mark_guild_read(&guild_id, cx);
    }

    fn toggle_pins(&mut self, _: &TogglePins, _: &mut Window, cx: &mut Context<Self>) {
        if self.chat_shown().is_none() {
            return;
        }
        if self.pins.take().is_some() {
            return cx.notify();
        }
        self.popover_at = point(px(self.pins_button_x.get()), px(0.));
        self.open_pins(cx);
    }

    fn toggle_members(&mut self, _: &ToggleMembers, _: &mut Window, cx: &mut Context<Self>) {
        if self.chat_shown().is_none() || self.guild_id.is_none() {
            return;
        }
        self.right_panel = if matches!(self.right_panel, RightPanel::Members) { RightPanel::Hidden } else { RightPanel::Members };
        cx.notify();
    }

    fn toggle_picker(&mut self, picker: ComposerPicker, window: &mut Window, cx: &mut Context<Self>) {
        if self.chat_shown().is_none() {
            return;
        }
        self.show_composer_picker(picker, window, cx);
    }

    // A page of the feed, unless the composer holds lines of its own to page through.
    fn scroll_chat(&mut self, direction: f32, cx: &mut Context<Self>) {
        let composer_pages = self.composer.read(cx).value().contains('\n');
        if self.chat_shown().is_none() || composer_pages {
            return cx.propagate();
        }
        let page = f32::from(self.list.viewport_bounds().size.height) * 0.8;
        self.list.scroll_by(px(page * direction));
        cx.notify();
    }

    fn jump_to_unread(&mut self, _: &JumpToUnread, _: &mut Window, cx: &mut Context<Self>) {
        let Some(channel) = self.chat_shown() else { return };
        let Some(read) = self.unread_marker else { return };
        let me = self.my_id();
        let first = self.feeds.get(&channel.id).and_then(|f| f.messages.iter().find(|m| crate::order(&m.id) > read && m.author.id != me)).map(|m| m.id.clone());
        let Some(first) = first else { return };
        self.jump_to(first, cx);
    }

    fn upload_file(&mut self, _: &UploadFile, window: &mut Window, cx: &mut Context<Self>) {
        if self.chat_shown().is_some() {
            self.upload(window, cx);
        }
    }

    fn copy_channel_link(&mut self, _: &CopyChannelLink, _: &mut Window, cx: &mut Context<Self>) {
        let Some(channel) = self.chat_shown() else { return };
        let place = self.guild_id.clone().unwrap_or_else(|| "@me".into());
        self.copy_text(format!("https://discord.com/channels/{place}/{}", channel.id), cx);
        self.show_toast("Link copied", cx);
    }

    fn answer_call(&mut self, _: &AnswerCall, _: &mut Window, cx: &mut Context<Self>) {
        let Some(channel_id) = self.ringing_in.clone() else { return cx.propagate() };
        self.answer_dm_call(&channel_id, cx);
    }

    // Esc closes the combos dialog first, then declines a ringing call; otherwise it goes on to its usual work.
    fn decline_call(&mut self, _: &DeclineCall, _: &mut Window, cx: &mut Context<Self>) {
        if self.keyboard_combos {
            return self.set_keyboard_combos(false, cx);
        }
        let Some(channel_id) = self.ringing_in.clone() else { return cx.propagate() };
        self.decline_dm_call(channel_id, cx);
    }

    fn start_call_here(&mut self, _: &StartCall, _: &mut Window, cx: &mut Context<Self>) {
        let Some(channel) = self.chat_shown().filter(|c| c.kind == ChannelKind::Dm) else { return };
        self.start_dm_call(channel, false, cx);
    }

    // The ⌘/ dialog: every group in Discord's order, in two columns when the window has room.
    pub(crate) fn render_keyboard_combos(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.keyboard_combos {
            return None;
        }
        let viewport = window.viewport_size();
        let wide = f32::from(viewport.width) >= WIDE_DIALOG + 64.;
        let head = div()
            .flex()
            .items_center()
            .gap_3()
            .child(div().text_size(ui::TEXT_LG).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child("Keyboard combos"))
            .child(key_caps(&platform_combo(COMBOS_KEY), cx))
            .child(div().flex_1())
            .child(ui::icon_button("combos-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.set_keyboard_combos(false, cx))));
        let blurb = div().text_color(palette::muted(cx)).child("Master these to be the greatest Waffle Waffle Revolution player");
        let app: &App = cx;
        let column = |groups: &[Group]| div().flex_1().min_w_0().flex().flex_col().gap_5().children(groups.iter().map(|&g| dialog_group(g, app)));
        let columns = match wide {
            true => div().flex().items_start().gap_6().child(column(&Group::ALL[..2])).child(column(&Group::ALL[2..])),
            false => column(&Group::ALL),
        };
        // Room under the cap for the head and blurb; the groups scroll in the rest.
        let list = div().id("combos-list").max_h(px(f32::from(viewport.height) * 0.92 - 140.)).overflow_y_scroll().child(columns);
        let body = ui::dialog_body().child(head).child(blurb).child(list);
        let width = if wide { WIDE_DIALOG } else { NARROW_DIALOG };
        let backdrop = div().id("combos-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.set_keyboard_combos(false, cx)));
        Some(ui::modal(backdrop, ui::dialog("keyboard-combos", width, body, window, cx), cx))
    }

    // Settings › Keybinds: the same list, one card, and the way to the dialog.
    pub(crate) fn default_keybinds(&self, cx: &mut Context<Self>) -> Div {
        let mut card = ui::settings_card(cx);
        for group in Group::ALL {
            card = card.child(group_heading(group, cx));
            card = card.children(rows(group).iter().enumerate().map(|(i, r)| row(format!("default-{}-{i}", group.title()), r, i > 0, cx)));
        }
        let open = ui::button("open-combos", Tone::Secondary, cx)
            .small()
            .child(Icon::new(Glyph::Keyboard).size(ui::ICON_MD))
            .child("Keyboard combos")
            .child(key_caps(&platform_combo(COMBOS_KEY), cx))
            .on_click(cx.listener(|this, _, _, cx| this.set_keyboard_combos(true, cx)));
        ui::settings_group("Default keybinds", cx).child(card).child(div().flex().child(open))
    }
}

fn dialog_group(group: Group, cx: &App) -> Div {
    let rows = rows(group);
    let note = group.note().map(|n| div().px_1().text_size(ui::TEXT_XS).text_color(palette::muted(cx)).child(n));
    div()
        .flex()
        .flex_col()
        .gap_1p5()
        .child(div().px_1().text_size(ui::TEXT_SM).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(group.title()))
        .children(note)
        .child(ui::settings_card(cx).children(rows.iter().enumerate().map(|(i, r)| row(format!("combo-{}-{i}", group.title()), r, i > 0, cx))))
}

fn wrap(index: usize, delta: isize, len: usize) -> usize {
    (index as isize + delta).rem_euclid(len.max(1) as isize) as usize
}

// The nearest index past `current` in `delta`'s direction that matches, wrapping round; never `current` itself.
fn next_match(current: Option<usize>, delta: isize, len: usize, matches: impl Fn(usize) -> bool) -> Option<usize> {
    let start = current.unwrap_or(if delta > 0 { len.saturating_sub(1) } else { 0 });
    (1..=len).map(|step| wrap(start, delta * step as isize, len)).filter(|&i| Some(i) != current).find(|&i| matches(i))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_combo_binds_and_none_repeats() {
        let mut seen = HashSet::new();
        for key in SHORTCUTS.iter().flat_map(|s| s.keys) {
            let Some(bind) = key.bind else { continue };
            // KeyBinding::new panics on a combo it can't parse.
            let binding = bind(key.combo);
            assert_eq!(binding.keystrokes().len(), 1, "{}", key.combo);
            assert!(seen.insert(key.combo), "{} is bound twice", key.combo);
        }
    }

    #[test]
    fn shown_combos_are_keystrokes() {
        for group in Group::ALL {
            for row in rows(group) {
                assert!(!row.keys.is_empty(), "{} shows no keys", row.label);
                for combo in row.keys {
                    let gpui = if combo == "plus" { "+".to_owned() } else { combo.replace('+', "-") };
                    assert!(gpui_kit::Keystroke::parse(&gpui).is_ok(), "{combo}");
                }
            }
        }
    }

    #[test]
    fn messages_come_from_the_feeds_table() {
        let labels: Vec<&str> = rows(Group::Messages).iter().map(|r| r.label).collect();
        assert_eq!(labels.len(), MessageAction::ALL.len() + 1);
        assert_eq!(labels[0], MessageAction::Edit.label());
        assert_eq!(rows(Group::Messages)[0].keys, vec![MessageAction::Edit.keys()[0]]);
    }

    #[test]
    fn steps_wrap_and_skip_the_current_place() {
        assert_eq!(wrap(0, -1, 3), 2);
        assert_eq!(wrap(2, 1, 3), 0);
        let unread = [false, true, false, true];
        assert_eq!(next_match(Some(1), 1, 4, |i| unread[i]), Some(3));
        assert_eq!(next_match(Some(3), 1, 4, |i| unread[i]), Some(1));
        assert_eq!(next_match(Some(1), -1, 4, |i| unread[i]), Some(3));
        assert_eq!(next_match(None, 1, 4, |i| unread[i]), Some(1));
        assert_eq!(next_match(Some(1), 1, 4, |i| i == 1), None);
        assert_eq!(next_match(None, 1, 0, |_| true), None);
    }
}
