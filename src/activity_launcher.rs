// The activity launcher: search, what's running in this call first (with who's in it), your recent apps,
// then everything Discord's shelf offers. An app opens its detail in place, whose one button says what happens.
use gpui_kit::{
    AnyElement, Anchor, Context, Entity, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Point, ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Subscription, Window,
    component::{
        Icon,
        input::{InputEvent, InputState},
    },
    div, img, prelude::*, px, relative,
};

use crate::{
    Waffle,
    activities::Place,
    discord::{ShelfApp, User},
    part::Column,
    ui::prelude::*,
};

const WIDTH: f32 = 520.;
const HEIGHT: f32 = 600.;
// Discord's launcher grid: two cards a row, its Recents icons, and how far a card sinks when pressed.
const GRID_GAP: f32 = 12.;
const RECENT_ICON: f32 = 48.;
const GRID_SLACK: f32 = 4.;
const PRESS: f32 = 0.98;
const BLOCKED_OPACITY: f32 = 0.5;
const RECENT_SHOWN: usize = 8;
const RECENT_KEPT: usize = 12;
const LOADING_ROWS: usize = 5;
const FACES: usize = 3;

pub(crate) struct Launcher {
    at: Point<Pixels>,
    // Where a game started from it runs: the call, or the chat it was opened from.
    pub(crate) place: Place,
    search: Entity<InputState>,
    // The app whose detail is open in place of the list.
    detail: Option<String>,
    scroll: ScrollHandle,
    _search_events: Subscription,
}

// The shelf's apps by id, in Discord's order (empty while loading), and the server it's for (None: a DM call).
pub(crate) struct Shelf {
    guild_id: Option<String>,
    apps: Result<Vec<String>, String>,
}

// What starting an app here would do, as its button says it.
enum Outcome {
    Join,
    Start,
    // It can't run here, and why.
    Blocked(&'static str),
}

impl Waffle {
    // Opens the launcher over the Activities control, for the call; again closes it.
    pub(crate) fn toggle_launcher(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(place) = self.call_place() else { return };
        self.toggle_launcher_at(at, place, window, cx);
    }

    // The composer's launcher: games that run over this chat, no voice needed.
    pub(crate) fn toggle_chat_launcher(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(place) = self.chat_place() else { return };
        self.toggle_launcher_at(at, place, window, cx);
    }

    fn toggle_launcher_at(&mut self, at: Point<Pixels>, place: Place, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        if self.activities.launcher.take().is_some() {
            return cx.notify(); // lint-allow(global-notify): the launcher is drawn by the root, over the columns
        }
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search activities"));
        let events = cx.subscribe_in(&search, window, |this, _, event: &InputEvent, _, cx| match event {
            InputEvent::PressEnter { .. } => this.launch_top_hit(cx),
            _ => cx.notify(), // lint-allow(global-notify): the launcher is drawn by the root, over the columns
        });
        search.update(cx, |state, cx| state.focus(window, cx));
        let guild_id = place.guild_id.clone();
        self.activities.launcher = Some(Launcher { at, place, search, detail: None, scroll: ScrollHandle::new(), _search_events: events });
        if self.activities.shelf.as_ref().is_none_or(|shelf| shelf.guild_id != guild_id || shelf.apps.is_err()) {
            self.fetch_shelf(guild_id, cx);
        }
        cx.notify(); // lint-allow(global-notify): the launcher is drawn by the root, over the columns
    }

    // From the sidebar's voice panel: the call comes on screen with the launcher open over it.
    pub(crate) fn open_launcher_from_panel(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(channel) = self.call.as_ref().map(|c| c.channel.clone()) {
            self.show_call(channel, window, cx);
        }
        self.activities.launcher = None;
        self.toggle_launcher(at, window, cx);
    }

    fn fetch_shelf(&mut self, guild_id: Option<String>, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        self.activities.shelf = Some(Shelf { guild_id: guild_id.clone(), apps: Ok(Vec::new()) });
        cx.spawn(async move |this, cx| {
            let shelf = client.activity_shelf(guild_id.as_deref()).await;
            _ = this.update(cx, |this, cx| {
                let ids = shelf.map(|apps| {
                    let ids = apps.iter().map(|a| a.id.clone()).collect();
                    this.activities.apps.extend(apps.into_iter().map(|a| (a.id.clone(), a)));
                    ids
                });
                this.activities.shelf = Some(Shelf { guild_id, apps: ids });
                cx.notify(); // lint-allow(global-notify): the launcher is drawn by the root, over the columns
            });
        })
        .detach(); // lint-allow(detached-task): a one-shot fetch that ends when the request returns, holding a WeakEntity
    }

    // Starting one puts it first among your recent apps.
    pub(crate) fn remember_activity(&mut self, app_id: &str) {
        let recent = &mut self.prefs.recent_activities;
        recent.retain(|id| id != app_id);
        recent.insert(0, app_id.to_owned());
        recent.truncate(RECENT_KEPT);
        self.prefs.save();
    }

    fn launcher_query(&self, cx: &gpui_kit::App) -> String {
        self.activities.launcher.as_ref().map(|l| l.search.read(cx).value().trim().to_lowercase()).unwrap_or_default()
    }

    // The search's matches, in shelf order, those that can run here first.
    fn launcher_matches(&self, query: &str) -> Vec<&ShelfApp> {
        let ids = match self.activities.shelf.as_ref().map(|s| &s.apps) {
            Some(Ok(ids)) => ids.as_slice(),
            _ => &[],
        };
        let mut hits: Vec<&ShelfApp> = ids.iter().filter_map(|id| self.activities.apps.get(id)).filter(|a| a.name.to_lowercase().contains(query) || a.description.to_lowercase().contains(query)).collect();
        hits.sort_by_key(|a| !a.desktop);
        hits
    }

    // Enter in the search starts (or joins) the top hit.
    fn launch_top_hit(&mut self, cx: &mut Context<Self>) {
        let query = self.launcher_query(cx);
        let top = self.launcher_matches(&query).into_iter().find(|a| a.desktop).map(|a| a.id.clone());
        if let Some(app_id) = top {
            self.start_from_launcher(app_id, cx);
        }
    }

    fn start_from_launcher(&mut self, app_id: String, cx: &mut Context<Self>) {
        self.remember_activity(&app_id);
        self.launch_activity(app_id, cx);
    }

    fn open_detail(&mut self, app_id: Option<String>, cx: &mut Context<Self>) {
        if let Some(launcher) = &mut self.activities.launcher {
            launcher.detail = app_id;
        }
        cx.notify(); // lint-allow(global-notify): the launcher is drawn by the root, over the columns
    }

    fn outcome(&self, app: &ShelfApp) -> Outcome {
        if !app.desktop {
            return Outcome::Blocked("Only on phones for now");
        }
        if let Some(why) = self.activities.launcher.as_ref().and_then(|l| self.activities_blocked(&l.place)) {
            return Outcome::Blocked(why);
        }
        match self.running_here(&app.id) {
            Some(_) => Outcome::Join,
            None => Outcome::Start,
        }
    }

    // Who's playing `app_id` where the launcher starts games, when it's running there.
    fn running_here(&self, app_id: &str) -> Option<Vec<User>> {
        let channel_id = self.activities.launcher.as_ref()?.place.channel.id.clone();
        self.activities.in_channel(&channel_id).any(|i| i.application_id == app_id).then(|| self.players_in(&channel_id, app_id))
    }

    pub(crate) fn render_launcher(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let launcher = self.activities.launcher.as_ref()?;
        let body = match launcher.detail.as_ref().and_then(|id| self.activities.apps.get(id)) {
            Some(app) => self.launcher_detail(app, cx),
            None => self.launcher_list(launcher, cx),
        };
        let surface = ui::surface(cx).w(px(WIDTH)).h(px(HEIGHT)).flex().flex_col().child(body);
        let backdrop = div().id("launcher-backdrop").absolute().inset_0().occlude().on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.activities.launcher = None;
                cx.notify(); // lint-allow(global-notify): the launcher is drawn by the root, over the columns
            }),
        );
        // From the call bar it rises to the right of the control; from the composer's end, to its left.
        let (at, anchor) = (launcher.at, if launcher.place.voice { Anchor::BottomLeft } else { Anchor::BottomRight });
        Some(div().absolute().inset_0().child(backdrop).child(ui::floating_at("launcher", at, anchor, ui::rise(surface, "launcher-rise", std::time::Duration::ZERO))).into_any_element())
    }

    fn launcher_list(&self, launcher: &Launcher, cx: &mut Context<Self>) -> AnyElement {
        let query = self.launcher_query(cx);
        let mut rows: Vec<AnyElement> = Vec::new();
        if let Some(app_id) = self.activities.frame.as_ref().map(|f| f.read(cx).app_id.clone()) {
            rows.push(
                ui::menu_row("launcher-leave", cx)
                    .danger(cx)
                    .child(Icon::new(Glyph::PhoneDisconnect).size(ICON_LG))
                    .child(ui::line(format!("Leave {}", self.app_name(&app_id))).flex_1())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.activities.launcher = None;
                        this.leave_with_rejoin(cx);
                    }))
                    .into_any_element(),
            );
            if cfg!(target_os = "macos") && self.activities.popout.is_none() {
                rows.push(ui::menu_row("launcher-popout", cx).child(Icon::new(Glyph::Open).size(ICON_LG)).child(ui::line("Pop out").flex_1()).on_click(cx.listener(|this, _, _, cx| {
                    this.activities.launcher = None;
                    this.pop_out(cx);
                })).into_any_element());
            }
        }
        match self.activities.shelf.as_ref().map(|s| &s.apps) {
            Some(Err(error)) => rows.push(self.shelf_failed(error, cx)),
            Some(Ok(ids)) if ids.is_empty() => rows.extend((0..LOADING_ROWS).map(|i| loading_row(i, cx))),
            Some(Ok(_)) if !query.is_empty() => {
                let hits = self.launcher_matches(&query);
                if hits.is_empty() {
                    rows.push(self.no_matches(&query, cx));
                }
                rows.push(self.card_grid(hits, cx));
            }
            // As Discord's: what's running here, your recent apps as a row of icons, then every activity as a card.
            Some(Ok(ids)) => {
                let running: Vec<&ShelfApp> = ids.iter().filter_map(|id| self.activities.apps.get(id)).filter(|a| self.running_here(&a.id).is_some()).collect();
                let recent: Vec<&ShelfApp> = self.prefs.recent_activities.iter().filter_map(|id| self.activities.apps.get(id)).filter(|a| ids.contains(&a.id)).take(RECENT_SHOWN).collect();
                if !running.is_empty() {
                    rows.push(section_label(format!("Running in {}", launcher.place.channel.name), cx));
                    rows.push(self.card_grid(running, cx));
                }
                if !recent.is_empty() {
                    rows.push(section_label("Recents", cx));
                    rows.push(self.recent_icons(recent, cx));
                }
                rows.push(section_label("Activities", cx));
                rows.push(self.card_grid(self.launcher_matches(""), cx));
            }
            None => {}
        }
        let list = ui::col().id("launcher-list").p(px(SURFACE_INSET)).gap_0p5().children(rows);
        ui::col()
            .size_full()
            .child(ui::row().flex_none().px_3().pt_3().pb_2().child(div().flex_1().child(ui::search_field("launcher-search", &launcher.search, cx))))
            .child(ui::scroll_area(list, &launcher.scroll))
            .into_any_element()
    }

    fn card_grid(&self, apps: Vec<&ShelfApp>, cx: &mut Context<Self>) -> AnyElement {
        // Rows of two, built as rows: wrapping inside the scroll area laid them one a row.
        let rows: Vec<AnyElement> = apps.chunks(2).map(|pair| ui::row().items_stretch().gap(px(GRID_GAP)).children(pair.iter().map(|app| self.launcher_card(app, cx))).into_any_element()).collect();
        ui::col().gap(px(GRID_GAP)).px(px(GRID_GAP / 2.)).pb_2().children(rows).into_any_element()
    }

    // Recents, as Discord's: the apps' icons in a row, each opening its detail.
    fn recent_icons(&self, apps: Vec<&ShelfApp>, cx: &mut Context<Self>) -> AnyElement {
        let icons = apps.into_iter().map(|app| {
            let (id, name) = (app.id.clone(), SharedString::from(app.name.clone()));
            ui::pressable(div().id(SharedString::from(format!("recent-{}", app.id))), PRESS)
                .flex_none()
                .cursor_pointer()
                .child(ui::server_tile(app.icon.as_deref(), &app.name, RECENT_ICON, cx))
                .tooltip(move |window, cx| ui::tip(name.clone(), window, cx))
                .on_click(cx.listener(move |this, _, _, cx| this.open_detail(Some(id.clone()), cx)))
        });
        ui::row().flex_wrap().gap(px(GRID_GAP)).px(px(GRID_GAP / 2.)).pb_2().children(icons).into_any_element()
    }

    // An activity as Discord's card: its art over its icon, name and one line (who's in it here, else what it
    // is). One that can't run here is dimmed, with why on hover.
    fn launcher_card(&self, app: &ShelfApp, cx: &mut Context<Self>) -> AnyElement {
        let id = app.id.clone();
        let outcome = self.outcome(app);
        let players = self.running_here(&app.id);
        let line: SharedString = match (&outcome, &players) {
            (Outcome::Blocked(why), _) => (*why).into(),
            (_, Some(players)) => crate::activities::who_is_playing(players).into(),
            _ if !app.description.is_empty() => app.description.clone().into(),
            _ => players_count(app.max_players).into(),
        };
        let faces: Vec<ui::Entry> = players.iter().flatten().map(|u| ui::Entry::person(u.avatar.clone(), u.name.clone())).collect();
        let blocked = match outcome {
            Outcome::Blocked(why) => Some(why),
            _ => None,
        };
        // Two a row: the launcher's width less its inset, the grid's side padding and the gap between the two, with
        // a little slack for the surface's edge so the second never wraps under the first.
        let width = ((WIDTH - SURFACE_INSET * 2. - GRID_GAP * 2.) / 2. - GRID_SLACK).floor();
        let art: AnyElement = match &app.cover {
            Some(url) => img(url.clone()).size_full().object_fit(gpui_kit::ObjectFit::Cover).into_any_element(),
            None => ui::row().size_full().justify_center().bg(palette::selected(cx)).child(Icon::new(Glyph::Image).size(ICON_XL).text_color(palette::faint(cx))).into_any_element(),
        };
        let marks = ui::row()
            .absolute()
            .top_2()
            .right_2()
            .gap_1()
            .when(app.new, |d| d.child(ui::tag("New", cx)))
            .when(app.age_gate, |d| d.child(ui::tag("18+", cx)));
        ui::pressable(div().id(SharedString::from(format!("launch-{}", app.id))), PRESS)
            .w(px(width))
            .flex_none()
            .flex()
            .flex_col()
            .squircle(RADIUS_MD)
            .overflow_hidden()
            .bg(palette::hover(cx))
            .cursor_pointer()
            .hover(|s| s.bg(palette::press(cx)))
            .when(blocked.is_some(), |d| d.opacity(BLOCKED_OPACITY))
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(width * 9. / 16.))
                    .flex_none()
                    .child(art)
                    .child(marks)
                    .when(!faces.is_empty(), |d| d.child(div().absolute().left_2().bottom_2().child(ui::capped_stack(&faces, FACES, AVATAR_SM, palette::surface(palette::Level::Raised, cx), palette::selected(cx), palette::text(cx))))),
            )
            .child(
                ui::row()
                    .p_2p5()
                    .gap_2p5()
                    .child(ui::server_tile(app.icon.as_deref(), &app.name, AVATAR_XL, cx))
                    .child(ui::col().flex_1().child(ui::line(app.name.clone()).font_weight(gpui_kit::FontWeight::SEMIBOLD).text_color(palette::strong(cx))).child(ui::line(line).text_size(TEXT_XS).text_color(palette::muted(cx)))),
            )
            .when_some(blocked, |d, why| d.tooltip(move |window, cx| ui::tip(why, window, cx)))
            .on_click(cx.listener(move |this, _, _, cx| this.open_detail(Some(id.clone()), cx)))
            .into_any_element()
    }

    // The app in place of the list: its art, what it is, how many can play, who's in it here, then one
    // button named for what it does.
    fn launcher_detail(&self, app: &ShelfApp, cx: &mut Context<Self>) -> AnyElement {
        let id = app.id.clone();
        let outcome = self.outcome(app);
        let players = self.running_here(&app.id);
        let facts = [Some(players_count(app.max_players)), Some(if app.desktop { "Desktop and phone".to_owned() } else { "Phone only".to_owned() }), app.age_gate.then(|| "18 and over".to_owned())]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
        let note = match (&outcome, &players) {
            (Outcome::Blocked(why), _) => Some(ui::callout(ui::Notice::Info, Glyph::Info, *why, blocked_fix(why), cx)),
            (_, Some(players)) => Some(ui::callout(ui::Notice::Info, Glyph::Users, crate::activities::who_is_playing(players), "You'll join their game.", cx)),
            _ if app.age_gate => Some(ui::callout(ui::Notice::Warning, Glyph::Warning, "For 18 and over", "Discord asks for your age the first time you play.", cx)),
            _ => None,
        };
        let label = match outcome {
            Outcome::Join => format!("Join {}", app.name),
            Outcome::Start => format!("Start {}", app.name),
            Outcome::Blocked(_) => format!("Start {}", app.name),
        };
        let blocked = matches!(outcome, Outcome::Blocked(_));
        let art = app.cover.clone().map(|url| img(url).w_full().h(px(WIDTH * 9. / 16. - SURFACE_INSET * 4.)).flex_none().squircle(RADIUS_MD).object_fit(gpui_kit::ObjectFit::Cover));
        ui::col()
            .size_full()
            .child(
                ui::row()
                    .flex_none()
                    .px_2()
                    .pt_2()
                    .gap_1()
                    .child(ui::icon_button("launcher-back", Glyph::ChevronLeft, "Back", cx).on_click(cx.listener(|this, _, _, cx| this.open_detail(None, cx))))
                    .child(ui::line(app.name.clone()).flex_1().text_size(TEXT_MD).font_weight(gpui_kit::FontWeight::SEMIBOLD).text_color(palette::strong(cx))),
            )
            .child(
                div().id("launcher-detail").flex_1().min_h_0().overflow_y_scroll().child(
                    ui::col()
                        .p_3()
                        .gap_3()
                        .children(art)
                        .child(ui::row().gap_3().child(ui::server_tile(app.icon.as_deref(), &app.name, AVATAR_XL, cx)).child(ui::col().flex_1().gap_0p5().child(ui::line(app.name.clone()).font_weight(gpui_kit::FontWeight::SEMIBOLD).text_color(palette::strong(cx))).child(ui::line(facts).text_size(TEXT_XS).text_color(palette::muted(cx)))))
                        .when(!app.description.is_empty(), |d| d.child(div().text_size(TEXT_SM).line_height(relative(LEADING_BODY)).text_color(palette::text(cx)).child(app.description.clone()))),
                ),
            )
            .child(
                ui::col()
                    .flex_none()
                    .p_3()
                    .gap_2()
                    .children(note)
                    .child(ui::button("launcher-go", Tone::Primary, cx).w_full().disabled(blocked).child(label).on_click(cx.listener(move |this, _, _, cx| {
                        if !blocked {
                            this.start_from_launcher(id.clone(), cx);
                        }
                    }))),
            )
            .into_any_element()
    }

    fn no_matches(&self, query: &str, cx: &mut Context<Self>) -> AnyElement {
        let clear = ui::button("launcher-clear", Tone::Secondary, cx).small().child("Browse everything").on_click(cx.listener(|this, _, window, cx| {
            if let Some(launcher) = &this.activities.launcher {
                launcher.search.update(cx, |state, cx| state.set_value("", window, cx));
            }
            this.redraw(Column::Main, cx);
        }));
        ui::col()
            .items_center()
            .gap_2()
            .py_6()
            .px_4()
            .text_center()
            .child(div().text_size(TEXT_SM).font_weight(gpui_kit::FontWeight::MEDIUM).text_color(palette::strong(cx)).child(format!("Nothing matches “{query}”")))
            .child(div().text_size(TEXT_XS).text_color(palette::muted(cx)).child("Try a shorter name, or browse everything."))
            .child(clear)
            .into_any_element()
    }

    fn shelf_failed(&self, error: &str, cx: &mut Context<Self>) -> AnyElement {
        let guild_id = self.activities.launcher.as_ref().and_then(|l| l.place.guild_id.clone());
        let retry = ui::button("launcher-retry", Tone::Secondary, cx).small().child("Try again").on_click(cx.listener(move |this, _, _, cx| this.fetch_shelf(guild_id.clone(), cx)));
        ui::col()
            .items_center()
            .gap_2()
            .py_6()
            .px_4()
            .text_center()
            .child(div().text_size(TEXT_SM).font_weight(gpui_kit::FontWeight::MEDIUM).text_color(palette::strong(cx)).child("Couldn't load activities"))
            .child(div().text_size(TEXT_XS).text_color(palette::muted(cx)).child(format!("Discord didn't answer ({error}). Your call is fine.")))
            .child(retry)
            .into_any_element()
    }
}

// What to do about a reason an app can't run here.
fn blocked_fix(why: &str) -> &'static str {
    match why {
        "Only on phones for now" => "Its maker hasn't released it for desktop. People on phones in this call can start it.",
        w if w.contains("AFK") => "Move to another voice channel to play.",
        _ => "Ask a moderator to turn on Use Activities for this channel.",
    }
}

fn players_count(max: Option<u32>) -> String {
    match max {
        Some(1) => "Single player".into(),
        Some(n) => format!("Up to {n} players"),
        None => "Any number can play".into(),
    }
}

fn section_label(text: impl Into<SharedString>, cx: &gpui_kit::App) -> AnyElement {
    div().px_2p5().pt_2().pb_1().text_size(TEXT_XS).font_weight(gpui_kit::FontWeight::SEMIBOLD).text_color(palette::muted(cx)).child(text.into()).into_any_element()
}

// A row's shape while the shelf loads.
fn loading_row(i: usize, cx: &gpui_kit::App) -> AnyElement {
    ui::row()
        .px_2p5()
        .py_1p5()
        .gap_2p5()
        .child(ui::skeleton(SharedString::from(format!("launcher-skel-icon-{i}")), RADIUS_MD, cx).size(px(AVATAR_XL)).flex_none())
        .child(ui::col().flex_1().gap_1p5().child(ui::skeleton(SharedString::from(format!("launcher-skel-a-{i}")), RADIUS_XS, cx).h_3().w(relative(0.5))).child(ui::skeleton(SharedString::from(format!("launcher-skel-b-{i}")), RADIUS_XS, cx).h_2p5().w(relative(0.8))))
        .into_any_element()
}
