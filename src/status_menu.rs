// Your status (auth.html `.au-accts`, profile.html "Custom status"): the menu your name in the panel opens
// (custom status, presence, then every signed-in account) and the "Set your status" dialog its first row opens.
use std::{cell::Cell, rc::Rc, time::Duration};

use chrono::{DateTime, Datelike, Local, NaiveDateTime, TimeDelta, Utc};
use gpui_kit::{
    AnyElement, App, Bounds, Context, Div, Entity, Focusable, FontWeight, MouseButton, MouseMoveEvent, Pixels, SharedString, Window,
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
    discord::{CustomStatus, Emoji, Profile, Status},
    icons::Glyph,
    layout::avatar_with_status,
    menu::{self, Entry, emoji_face},
    overlays::backdrop,
    palette,
    profile::{self, AVATAR, AVATAR_RING, Ink, name_block},
    ui::{self, Squircle},
};

// Not verified: Discord's limit, from memory.
const TEXT_MAX: usize = 128;
// The count turns amber this close to the limit.
const TEXT_WARN: usize = 110;
// Wider than a plain menu so account names fit beside the log-out button.
const MENU_WIDTH: f32 = 288.;
const DIALOG_WIDTH: f32 = 480.;
const FLYOUT_WIDTH: f32 = 200.;
const PRESENCES: [(Status, &str, &str); 4] = [
    (Status::Online, "Online", ""),
    (Status::Idle, "Idle", ""),
    (Status::Dnd, "Do not disturb", "Mutes notifications. Mentions still count up."),
    (Status::Offline, "Invisible", "You look offline but can use Waffle as normal"),
];
// What Discord offers for Idle, Do not disturb and Invisible, in minutes.
const DURATIONS: [(&str, Option<i64>); 6] = [("For 15 minutes", Some(15)), ("For 1 hour", Some(60)), ("For 8 hours", Some(480)), ("For 24 hours", Some(1440)), ("For 3 days", Some(4320)), ("Forever", None)];

// The durations card beside Idle, Do not disturb or Invisible, opened by hovering the row. A switch to
// another row waits out the tunnel's grace while the pointer heads for the open card.
#[derive(Default)]
pub(crate) struct Flyout {
    open: Option<Status>,
    pending: Option<Option<Status>>,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    tunnel: ui::Tunnel,
}

impl Flyout {
    #[cfg(debug_assertions)]
    pub(crate) fn opened(status: Status) -> Self {
        Flyout { open: Some(status), ..Flyout::default() }
    }
}

pub(crate) struct StatusEditor {
    pub(crate) emoji: Option<Emoji>,
    status: Status,
}

// When a custom status clears; the last choice is kept for the next one.
#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) enum ClearAfter {
    Never,
    HalfHour,
    Hour,
    FourHours,
    #[default]
    Today,
    ThisWeek,
}

impl ClearAfter {
    const ALL: [ClearAfter; 6] = [ClearAfter::Never, ClearAfter::HalfHour, ClearAfter::Hour, ClearAfter::FourHours, ClearAfter::Today, ClearAfter::ThisWeek];

    fn name(self) -> &'static str {
        match self {
            ClearAfter::Never => "Don't clear",
            ClearAfter::HalfHour => "30 minutes",
            ClearAfter::Hour => "1 hour",
            ClearAfter::FourHours => "4 hours",
            ClearAfter::Today => "Today",
            ClearAfter::ThisWeek => "This week",
        }
    }

    fn glyph(self) -> Glyph {
        match self {
            ClearAfter::Never => Glyph::Pin,
            ClearAfter::HalfHour => Glyph::Timer,
            ClearAfter::Hour => Glyph::Clock,
            ClearAfter::FourHours => Glyph::Recent,
            ClearAfter::Today => Glyph::Moon,
            ClearAfter::ThisWeek => Glyph::Calendar,
        }
    }

    fn hint(self) -> Option<&'static str> {
        match self {
            ClearAfter::Today => Some("Until midnight"),
            ClearAfter::ThisWeek => Some("Until Sunday night"),
            _ => None,
        }
    }

    // Today ends at midnight, this week at the midnight after Sunday.
    fn expiry(self, now: NaiveDateTime) -> Option<NaiveDateTime> {
        let minutes = match self {
            ClearAfter::Never => return None,
            ClearAfter::HalfHour => 30,
            ClearAfter::Hour => 60,
            ClearAfter::FourHours => 240,
            ClearAfter::Today => return now.date().succ_opt()?.and_hms_opt(0, 0, 0),
            ClearAfter::ThisWeek => {
                let days_left = 7 - i64::from(now.weekday().num_days_from_monday());
                return (now.date() + TimeDelta::days(days_left)).and_hms_opt(0, 0, 0);
            }
        };
        Some(now + TimeDelta::minutes(minutes))
    }
}

// "today at 17:20", "tomorrow at 09:00", further out the weekday.
fn when(at: NaiveDateTime, now: NaiveDateTime) -> String {
    let day = match (at.date() - now.date()).num_days() {
        0 => "today".to_owned(),
        1 => "tomorrow".to_owned(),
        _ => format!("on {}", at.format("%A")),
    };
    format!("{day} at {}", at.format("%H:%M"))
}

fn local(at: DateTime<Utc>) -> NaiveDateTime {
    at.with_timezone(&Local).naive_local()
}

impl Waffle {
    pub(crate) fn toggle_status_menu(&mut self, cx: &mut Context<Self>) {
        self.status_menu = !self.status_menu;
        self.status_flyout = Flyout::default();
        cx.notify(); // lint-allow(global-notify): the menu is an overlay, but the user panel under it also lights while it's open
    }

    // Hovering a row opens its durations (`target`), or closes them for a row without any.
    fn hover_status_row(&mut self, target: Option<Status>, at: gpui_kit::Point<Pixels>, cx: &mut Context<Self>) {
        let flyout = &mut self.status_flyout;
        if flyout.open == target {
            flyout.pending = None;
            return;
        }
        if flyout.open.is_some() && flyout.tunnel.heading_to(at, flyout.bounds.get()) {
            flyout.pending = Some(target);
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(ui::TUNNEL_GRACE).await;
                this.update(cx, |this, cx| {
                    if this.status_flyout.pending == Some(target) {
                        this.status_flyout.pending = None;
                        this.status_flyout.open = target;
                        this.redraw_overlays(cx);
                    }
                })
                .ok();
            })
            // lint-allow(detached-task): ends itself after the one grace timer, and holds only a WeakEntity.
            .detach();
            return;
        }
        flyout.pending = None;
        flyout.open = target;
        self.redraw_overlays(cx);
    }

    fn row_hover(&self, target: Option<Status>, cx: &mut Context<Self>) -> impl Fn(&bool, &mut Window, &mut App) + 'static {
        cx.listener(move |this, hovered: &bool, window, cx| {
            if *hovered {
                this.hover_status_row(target, window.mouse_position(), cx);
            }
        })
    }

    // `minutes` None keeps it until changed.
    // GAP: the fall back to Online runs only while Waffle is open; Discord's own clients keep the expiry in
    // the protobuf settings, which Waffle doesn't write (not verified).
    pub(crate) fn set_status(&mut self, status: Status, minutes: Option<i64>, cx: &mut Context<Self>) {
        self.my_status = status;
        self.status_menu = false;
        let until = minutes.map(|m| Local::now() + TimeDelta::minutes(m));
        self.status_until = until;
        if let Some(client) = self.client.clone() {
            let (custom, game, activity) = (self.custom_status.clone(), self.shared_game().cloned(), self.activity_presence());
            crate::discord::runtime().spawn(async move { client.set_status(status, custom.as_ref(), game.as_ref(), activity).await });
        }
        if let Some(until) = until {
            self.run_at(until, move |this, cx| {
                if this.status_until == Some(until) {
                    this.set_status(Status::Online, None, cx);
                }
            }, cx);
        }
        cx.notify(); // lint-allow(global-notify): your presence draws in the user panel, the member list and the status menu
    }

    pub(crate) fn set_custom_status(&mut self, custom: Option<CustomStatus>, cx: &mut Context<Self>) {
        self.status_menu = false;
        let before = self.custom_status.clone();
        let setting = custom.as_ref().map(CustomStatus::setting);
        self.rest_optimistic(
            crate::discord::Method::PATCH,
            "/users/@me/settings".into(),
            Some(serde_json::json!({ "custom_status": setting })),
            move |this| {
                this.custom_status = custom;
                this.publish_presence();
            },
            move |this| {
                this.custom_status = before;
                this.publish_presence();
            },
            cx,
        );
        self.expire_custom_status_later(cx);
    }

    // Discord clears the saved setting itself; this takes it off the panel and our presence on time.
    pub(crate) fn expire_custom_status_later(&self, cx: &mut Context<Self>) {
        let Some(at) = self.custom_status.as_ref().and_then(|c| c.expires_at) else { return };
        self.run_at(at.with_timezone(&Local), move |this, cx| {
            if this.custom_status.as_ref().and_then(|c| c.expires_at) == Some(at) {
                this.custom_status = None;
                this.publish_presence();
                cx.notify(); // lint-allow(global-notify): your custom status draws in the user panel and the member list
            }
        }, cx);
    }

    pub(crate) fn publish_presence(&self) {
        if let Some(client) = &self.client {
            client.send_presence(self.my_status, self.custom_status.as_ref(), self.shared_game(), self.activity_presence());
        }
    }

    fn run_at(&self, at: DateTime<Local>, then: impl FnOnce(&mut Waffle, &mut Context<Waffle>) + 'static, cx: &mut Context<Self>) {
        let wait = (at - Local::now()).to_std().unwrap_or_default();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            this.update(cx, |this, cx| then(this, cx)).ok();
        })
        // lint-allow(detached-task): ends itself once the timer fires and `then` has run, and holds only a WeakEntity.
        .detach();
    }

    /// The panel's second line: your custom status with its emoji, else the presence word.
    pub(crate) fn status_line(&self) -> String {
        let Some(custom) = &self.custom_status else { return self.my_status.label().to_owned() };
        let emoji = custom.emoji.as_ref().filter(|e| e.id.is_none()).map(|e| e.name.as_str());
        [emoji, Some(custom.text.as_str()).filter(|t| !t.is_empty())].into_iter().flatten().collect::<Vec<_>>().join(" ")
    }

    pub(crate) fn open_status_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.status_menu = false;
        let current = self.custom_status.clone();
        let text = current.as_ref().map(|c| c.text.clone()).unwrap_or_default();
        self.status_input.update(cx, |state, cx| {
            state.set_value(text, window, cx);
            state.focus(window, cx);
        });
        self.status_editor = Some(StatusEditor { emoji: current.and_then(|c| c.emoji), status: self.my_status });
        if let Some(me) = self.me.clone() {
            self.fetch_profile(me, None, cx);
        }
        cx.notify(); // lint-allow(global-notify): the editor is an overlay, but closing the menu also unlights the user panel
    }

    pub(crate) fn on_status_input(&mut self, _: &Entity<SearchState>, event: &InputEvent, _: &mut Window, cx: &mut Context<Self>) {
        match event {
            InputEvent::PressEnter { .. } => self.save_status(cx),
            InputEvent::Change => self.redraw_overlays(cx),
            _ => {}
        }
    }

    fn save_status(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.status_editor.take() else { return };
        let text: String = self.status_input.read(cx).value().trim().chars().take(TEXT_MAX).collect();
        let expires_at = self.clear_after.expiry(Local::now().naive_local()).and_then(|t| t.and_local_timezone(Local).earliest()).map(|t| t.with_timezone(&Utc));
        let custom = (!text.is_empty() || editor.emoji.is_some()).then_some(CustomStatus { text, emoji: editor.emoji, expires_at });
        if editor.status != self.my_status {
            self.set_status(editor.status, None, cx);
        }
        self.set_custom_status(custom, cx);
    }

    fn close_status_editor(&mut self, cx: &mut Context<Self>) {
        self.status_editor = None;
        self.redraw_overlays(cx);
    }

    fn clear_after_entries(&self) -> Vec<Entry> {
        ClearAfter::ALL
            .into_iter()
            .map(|choice| {
                // A radio row keeps its menu open; one choice is all this list takes.
                let row = menu::item(choice.name(), move |this, _, cx| {
                    this.clear_after = choice;
                    this.menu = None;
                    cx.notify(); // lint-allow(global-notify): closes the menu, which the root draws over the columns
                })
                .icon(choice.glyph())
                .radio(self.clear_after == choice);
                match choice.hint() {
                    Some(hint) => row.hint(hint),
                    None => row,
                }
                .entry()
            })
            .collect()
    }

    pub(crate) fn status_menu_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.status_menu {
            return None;
        }
        let add = menu::item("Add an account", |_, _, _| {}).icon(Glyph::Plus);
        let add = menu::menu_item_row("add-account".into(), &add, false, true, cx).on_hover(self.row_hover(None, cx)).on_click(cx.listener(|this, _, window, cx| this.add_account(window, cx)));
        let card = ui::PopoverContent::new(cx)
            .w(px(MENU_WIDTH))
            .id("status-menu")
            .occlude()
            .absolute()
            .left_4()
            .bottom(px(self.panel_stack_height() + 16.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, _| this.status_flyout.tunnel.track(e.position)))
            .child(self.custom_status_row(cx))
            .children(PRESENCES.iter().map(|&(status, label, _)| self.presence_row(status, label, cx)))
            .child(menu::separator())
            .child(div().pt_1p5().pb_0p5().px_2p5().text_size(ui::TEXT_XS).font_weight(FontWeight::SEMIBOLD).text_color(palette::muted(cx)).child("Accounts"))
            .children(self.prefs.accounts.iter().map(|account| self.account_row(account, cx)))
            .when(self.can_add_account(), |card| card.child(menu::separator()).child(add));
        #[cfg(debug_assertions)]
        let card = card.child(menu::separator()).child(self.design_system_row(cx));
        Some(backdrop("status-backdrop", cx).child(ui::rise(card.transform_origin(ui::corner(false, true)), "status-menu", Duration::ZERO)).into_any_element())
    }

    // "Set a custom status", or the one you have: its emoji and text, when it clears, and a clear button.
    fn custom_status_row(&self, cx: &mut Context<Self>) -> ui::Control {
        let row = match &self.custom_status {
            Some(custom) => {
                let label = if custom.text.is_empty() { "Custom status".to_owned() } else { custom.text.clone() };
                let clears = custom.expires_at.map_or_else(|| "Doesn't clear".to_owned(), |at| format!("Clears {}", when(local(at), Local::now().naive_local())));
                let row = menu::item(label, |_, _, _| {}).hint(clears);
                match &custom.emoji {
                    Some(emoji) => row.emoji(emoji.clone()),
                    None => row.icon(Glyph::Emoji),
                }
            }
            None => menu::item("Set a custom status", |_, _, _| {}).icon(Glyph::Emoji),
        };
        let clear = self.custom_status.is_some().then(|| {
            ui::Button::new("status-custom-clear").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconXs).icon(Glyph::X).tooltip_text("Clear custom status").on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.set_custom_status(None, cx);
            }))
        });
        menu::menu_item_row("status-custom".into(), &row, false, true, cx).children(clear).on_hover(self.row_hover(None, cx)).on_click(cx.listener(|this, _, window, cx| this.open_status_editor(window, cx)))
    }

    // One click sets a presence until changed; hovering Idle, Do not disturb or Invisible opens its durations
    // beside the row.
    fn presence_row(&self, status: Status, label: &'static str, cx: &mut Context<Self>) -> ui::Control {
        let current = self.my_status == status;
        let timed = status != Status::Online;
        let open = timed && self.status_flyout.open == Some(status);
        let row = menu::item(label, |_, _, _| {}).radio(current).status(status);
        let row = match self.status_until.filter(|_| current) {
            Some(until) => row.hint(format!("Until {}", when(until.naive_local(), Local::now().naive_local()))),
            None => row,
        };
        let chevron = timed.then(|| div().flex_none().text_color(palette::muted(cx)).child(Icon::new(Glyph::ChevronRight).size(ui::ICON_MD)));
        let flyout = open.then(|| {
            // lint-allow(raw-margin): sits past its row's right edge by the card's inset and a gap, which left_full can't add to.
            div().absolute().top(px(-ui::SURFACE_INSET)).left_full().ml(px(ui::SURFACE_INSET + 4.)).child(ui::Popover::floating("status-duration-float", self.duration_flyout(status, cx)).with_priority(1))
        });
        menu::menu_item_row(SharedString::from(format!("status-{label}")), &row, open, true, cx)
            .relative()
            .children(chevron)
            .children(flyout)
            .on_hover(self.row_hover(timed.then_some(status), cx))
            .on_click(cx.listener(move |this, _, _, cx| this.set_status(status, None, cx)))
    }

    fn duration_flyout(&self, status: Status, cx: &mut Context<Self>) -> impl IntoElement {
        let record = self.status_flyout.bounds.clone();
        let rows = DURATIONS.iter().map(|&(label, minutes)| {
            let row = menu::item(label, |_, _, _| {});
            menu::menu_item_row(SharedString::from(format!("status-for-{label}")), &row, false, false, cx).on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.set_status(status, minutes, cx);
            }))
        });
        let card = ui::PopoverContent::new(cx)
            .id("status-durations")
            .occlude()
            .w(px(FLYOUT_WIDTH))
            .relative()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_hover(cx.listener(|this, hovered: &bool, _, _| {
                if *hovered {
                    this.status_flyout.pending = None;
                }
            }))
            .child(gpui_kit::canvas(move |bounds, _, _| record.set(bounds), |_, _, _, _| {}).absolute().inset_0())
            .children(rows);
        ui::rise(card, "status-durations", Duration::ZERO)
    }

    // One signed-in account: its avatar and status, the connected one filled; the others switch on click
    // and carry a log-out button.
    fn account_row(&self, account: &crate::accounts::Account, cx: &mut Context<Self>) -> ui::Control {
        let current = self.me.as_ref().is_some_and(|me| me.id == account.id);
        let status = if current { self.my_status } else { account.status };
        let ring = if current { palette::selected(cx) } else { palette::surface(palette::Level::Overlay, cx) };
        let name = ui::col().min_h_0()
            .flex_1()
            .line_height(ui::LINE_DETAIL)
            .child(ui::line(account.name.clone()).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)))
            .child(ui::line(status.label()).text_size(ui::TEXT_XS).text_color(palette::muted(cx)));
        let (switch_id, out_id) = (account.id.clone(), account.id.clone());
        let log_out = (!current).then(|| {
            ui::Button::new(SharedString::from(format!("log-out-{}", account.id))).variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconXs).icon(Glyph::DoorExit).tooltip_text(format!("Log out of {}", account.name)).on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.remove_account(out_id.clone(), window, cx);
            }))
        });
        ui::DropdownMenuItem::new(SharedString::from(format!("account-{}", account.id)), cx)
            .toggled(current, cx)
            .min_h_12()
            .pl_2()
            .pr_1p5()
            .child(avatar_with_status(&account.avatar, 32., Some(status.into()), ring))
            .child(name)
            .children(log_out)
            .on_hover(self.row_hover(None, cx))
            .on_click(cx.listener(move |this, _, window, cx| this.switch_account(switch_id.clone(), window, cx)))
    }

    pub(crate) fn status_editor_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let editor = self.status_editor.as_ref()?;
        let head = ui::row()
            .items_start()
            .gap_3()
            .child(
                ui::col().min_h_0()
                    .flex_1()
                    .gap_1()
                    .child(div().text_size(ui::TEXT_LG).line_height(ui::LINE_LG).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child("Set your status"))
                    .child(div().text_size(ui::TEXT_SM).text_color(palette::muted(cx)).child("Shown on your profile and in member lists.")),
            )
            .child(ui::Button::new("status-close").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).icon(Glyph::X).tooltip_text("Close").on_click(cx.listener(|this, _, _, cx| this.close_status_editor(cx))));
        let clear_after = ui::Select::new("status-clear-after", Some(self.clear_after.glyph()), self.clear_after.name(), false, cx)
            .on_click(cx.listener(|this, _, window, cx| this.open_menu(window.mouse_position(), this.clear_after_entries(), cx)));
        let footer = ui::row()
            .gap_2()
            .child(ui::row().items_stretch().flex_1().child(ui::Button::new("status-clear-all").variant(ui::ButtonVariant::Ghost).child("Clear status").on_click(cx.listener(|this, _, _, cx| {
                this.status_editor = None;
                this.set_custom_status(None, cx);
            }))))
            .child(ui::Button::new("status-cancel").variant(ui::ButtonVariant::Secondary).child("Cancel").on_click(cx.listener(|this, _, _, cx| this.close_status_editor(cx))))
            .child(ui::Button::new("status-save").child("Save status").on_click(cx.listener(|this, _, _, cx| this.save_status(cx))));
        let body = ui::DialogContent::new()
            .child(head)
            .child(self.status_preview(editor, cx))
            .child(ui::Field::new("Custom status", cx).child(self.status_text_field(editor, cx)))
            .child(ui::Field::new("Clear after", cx).child(clear_after))
            .child(ui::Field::new("Status", cx).child(ui::col().children(PRESENCES.iter().map(|&(status, label, detail)| presence_choice(status, label, detail, editor.status == status, cx)))))
            .child(footer);
        let backdrop = div().id("status-editor-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_status_editor(cx)));
        Some(ui::Dialog::modal(backdrop, ui::Dialog::new("status-editor", DIALOG_WIDTH, body, window, cx), cx))
    }

    // Your card as others will see it, the bubble and presence following the dialog as you edit.
    fn status_preview(&self, editor: &StatusEditor, cx: &mut Context<Self>) -> AnyElement {
        let Some(me) = &self.me else { return div().into_any_element() };
        let profile = self.my_profile();
        let ink = Ink::new(profile.theme, cx);
        let text = self.status_input.read(cx).value().trim().to_owned();
        let face = ui::row().gap_1p5().text_color(ink.text);
        let face = if text.is_empty() && editor.emoji.is_none() {
            face.text_color(ink.muted).child(div().italic().child("What's on your mind?"))
        } else {
            face.children(editor.emoji.as_ref().map(|e| div().flex_none().child(emoji_face(e, 18.)))).when(!text.is_empty(), |f| f.child(div().min_w_0().line_clamp(2).child(text)))
        };
        let radius = profile::inner_radius(ink);
        let (banner, banner_height) = profile::banner(&profile, ink, radius, false, profile::BANNER, cx);
        let decoration = me.decoration.clone().or(profile.decoration.clone());
        let avatar = div().p(px(AVATAR_RING)).rounded_full().bg(ink.top).child(crate::layout::decorated_avatar(&me.avatar, decoration.as_deref(), AVATAR, Some(editor.status.into()), ink.top));
        let inset = 16.;
        let header = ui::col()
            .relative()
            .child(banner)
            // lint-allow(raw-margin): the avatar row pulls up over the banner's foot, which padding can't do.
            .child(ui::row().items_stretch().px(px(inset - AVATAR_RING)).mt(px(-(AVATAR / 2. + AVATAR_RING))).mb_2().child(avatar))
            .child(ui::row().items_stretch().absolute().top(px(banner_height - 20.)).left(px(inset + AVATAR + AVATAR_RING + 6.)).right_3().child(ui::thought_bubble("status-bubble", face, ink.top, palette::line(cx))));
        let (outer, inner) = profile::themed_card(ink, f32::MAX, cx);
        outer.child(inner.child(header).child(div().px(px(inset)).pb(px(inset)).child(name_block(me, &profile, ink, None, None, None)))).into_any_element()
    }

    fn my_profile(&self) -> Profile {
        let Some(me) = &self.me else { return Profile::default() };
        self.profiles.peek(&profile::profile_key(&me.id, None)).unwrap_or_default()
    }

    // The emoji button, the text, its count against the limit and a clear button once there's text.
    fn status_text_field(&self, editor: &StatusEditor, cx: &mut Context<Self>) -> AnyElement {
        let length = self.status_input.read(cx).value().chars().count();
        let emoji = ui::row()
            .id("status-emoji")
            .size_6()
            .justify_center()
            .cursor_pointer()
            .text_color(palette::muted(cx))
            .child(match &editor.emoji {
                Some(emoji) => emoji_face(emoji, 20.),
                None => Icon::new(Glyph::Emoji).size(ui::ICON_XL).into_any_element(),
            })
            .on_click(cx.listener(|this, _, window, cx| this.open_emoji_picker(EmojiTarget::CustomStatus, window, cx)));
        let count = div().text_size(ui::TEXT_XS).text_color(if length >= TEXT_WARN { palette::warning_ink(cx) } else { palette::faint(cx) }).child(format!("{length}/{TEXT_MAX}"));
        let focus = self.status_input.read(cx).focus_handle(cx);
        let field = ui::Input::new("status-text", Input::new(&self.status_input).appearance(false), &focus).leading(emoji).trailing(count);
        if length == 0 {
            return field.into_any_element();
        }
        field.trailing(ui::Button::new("status-clear-text").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconXs).icon(Glyph::XCircle).tooltip_text("Clear text").on_click(cx.listener(|this, _, window, cx| {
            this.status_input.update(cx, |state, cx| state.set_value("", window, cx));
            this.redraw_overlays(cx);
        })))
        .into_any_element()
    }
}

// A presence to pick in the dialog: its mark, name and what it does, the radio at the end.
fn presence_choice(status: Status, label: &'static str, detail: &'static str, on: bool, cx: &mut Context<Waffle>) -> gpui_kit::Stateful<Div> {
    let row = ui::row()
        .id(SharedString::from(format!("status-choice-{label}")))
        .min_h_11()
        .px_2p5()
        .py_1p5()
        .squircle(ui::RADIUS_MD)
        .gap_3()
        .when(on, |r| r.bg(palette::selected(cx)))
        .child(ui::status_mark(status, 14.))
        .child(
            ui::col().min_h_0()
                .flex_1()
                .child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(label))
                .when(!detail.is_empty(), |d| d.child(div().text_size(ui::TEXT_XS).text_color(palette::muted(cx)).child(detail))),
        )
        .child(ui::RadioGroupItem::mark(on))
        .on_click(cx.listener(move |this, _, _, cx| {
            if let Some(editor) = this.status_editor.as_mut() {
                editor.status = status;
            }
            cx.notify();
        }));
    ui::settings_choice(row, cx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: u32, h: u32, m: u32) -> NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 11, day).and_then(|d| d.and_hms_opt(h, m, 0)).expect("valid time")
    }

    #[test]
    fn clear_after_expiry_and_when() {
        // 2026-11-19 is a Thursday.
        let now = at(19, 16, 50);
        assert_eq!(ClearAfter::HalfHour.expiry(now), Some(at(19, 17, 20)));
        assert_eq!(ClearAfter::Today.expiry(now), Some(at(20, 0, 0)));
        assert_eq!(ClearAfter::ThisWeek.expiry(now), Some(at(23, 0, 0)));
        assert_eq!(ClearAfter::ThisWeek.expiry(at(22, 23, 0)), Some(at(23, 0, 0)));
        assert_eq!(ClearAfter::Never.expiry(now), None);
        assert_eq!(when(at(19, 17, 20), now), "today at 17:20");
        assert_eq!(when(at(20, 2, 0), now), "tomorrow at 02:00");
        assert_eq!(when(at(22, 9, 5), now), "on Sunday at 09:05");
    }
}
