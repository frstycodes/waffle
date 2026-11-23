// What the events feature draws: the list, an event's card and details, the wizard and the Start dialog.
use super::*;

// The events list's tallest scroll, a card's and the wizard's cover heights, an event's title size, the
// small people and repeat glyphs, the Interested list's tallest scroll and the stepper's line weight.
const LIST_HEIGHT: f32 = 560.;
const COVER_HEIGHT: f32 = 140.;
const WIZARD_COVER_HEIGHT: f32 = 120.;
const TITLE_SIZE: f32 = 18.;
const COUNT_ICON: f32 = 13.;
const INTERESTED_HEIGHT: f32 = 360.;
const STEP_LINE: f32 = 1.5;

// Rendering
impl Waffle {
    pub(crate) fn events_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::new();
        out.extend(self.events_list_dialog(window, cx));
        out.extend(self.event_details_dialog(window, cx));
        out.extend(self.event_wizard_dialog(window, cx));
        out.extend(self.start_event_dialog(window, cx));
        out
    }

    // The sidebar's top: a live event's bar, then the Events row (shown when there's something to see or
    // you could add one).
    pub(crate) fn event_rows(&self, guild: &Guild, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let me = self.my_id();
        let count = upcoming_count(guild, me);
        let mut out: Vec<AnyElement> = Vec::new();
        if let Some(live) = guild.events.iter().find(|e| e.status == ScheduledEvent::ACTIVE) {
            out.push(self.live_bar(guild, live, cx));
        }
        if count == 0 && !can_create(guild, me) {
            return out;
        }
        let gid = guild.id.clone();
        let state = crate::sidebar::RowState { selected: self.events.list.as_deref() == Some(guild.id.as_str()), unread: false, muted: false, strong: false };
        let row = crate::sidebar::channel_row(SharedString::from(format!("upcoming-events-{}", guild.id)), "events-row".into(), state, cx)
            .child(Icon::new(Glyph::Calendar).size(ui::ICON_XL).flex_none())
            .child(ui::line(events_title(count)).flex_1())
            .on_click(cx.listener(move |this, _, _, cx| this.open_events(gid.clone(), cx)));
        out.push(row.into_any_element());
        out
    }

    pub(super) fn live_bar(&self, guild: &Guild, event: &ScheduledEvent, cx: &mut Context<Self>) -> AnyElement {
        let channel = event.channel_id.as_deref().and_then(|c| guild.channel(c)).cloned();
        let listening = channel.as_ref().map(|c| guild.voice_states.iter().filter(|v| v.channel_id.as_deref() == Some(c.id.as_str())).count());
        let line = match (&channel, listening) {
            (Some(c), Some(n)) if is_stage(c) => format!("{} · {n} listening", c.name),
            (Some(c), Some(n)) => format!("{} · {n} in voice", c.name),
            _ => self.event_place(event),
        };
        let lead = match &channel {
            Some(c) => crate::sidebar::channel_glyph(c),
            None => Icon::new(Glyph::MapPoint),
        };
        let (g, e) = (event.guild_id.clone(), event.id.clone());
        let join = channel.clone().map(|c| {
            let label = if is_stage(&c) { "Join stage" } else { "Join" };
            ui::button("events-live-join", Tone::Primary, cx).small().h_7().child(label).on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.join_voice(c.clone(), cx);
            }))
        });
        ui::col()
            .id("events-live-bar")
            // lint-allow(raw-margin): spaces the bar from the channel rows under it, whose list (sidebar.rs) owns no gap
            .mb_2()
            .p_2p5()
            .gap_1p5()
            .squircle(ui::RADIUS_MD)
            .bg(palette::accent_soft(cx))
            .text_color(palette::text(cx))
            .cursor_pointer()
            .child(
                ui::row()
                    .gap_2()
                    .child(ui::live_badge())
                    .child(ui::line(event.name.clone()).flex_1().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx))),
            )
            .child(ui::row().gap_1().text_size(ui::TEXT_XS).text_color(palette::muted(cx)).child(lead.size_3p5()).child(ui::line(line)))
            .children(join)
            .on_click(cx.listener(move |this, _, _, cx| this.open_event_details(g.clone(), e.clone(), cx)))
            .into_any_element()
    }

    pub(super) fn events_list_dialog(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let guild_id = self.events.list.as_deref()?;
        let guild = self.event_guild(guild_id)?;
        let me = self.my_id();
        let events = listed(guild, me);
        let title = events_title(upcoming_count(guild, me));
        let gid = guild_id.to_owned();
        let create = can_create(guild, me).then(|| {
            // lint-allow(custom-divider): ui::divider is a horizontal rule; this is the vertical one beside Create Event
            ui::row().gap_3().child(div().w(ui::HAIRLINE).h_5().bg(palette::line(cx))).child(
                ui::button("events-create", Tone::Primary, cx).small().child(Icon::new(Glyph::Plus).size(ui::ICON_MD)).child("Create Event").on_click(cx.listener(move |this, _, window, cx| this.open_event_wizard(gid.clone(), None, window, cx))),
            )
        });
        let head = ui::row()
            .gap_2p5()
            .child(div().text_color(palette::muted(cx)).child(Icon::new(Glyph::Calendar).size(ui::ICON_XL)))
            .child(ui::line(title).flex_1().text_size(ui::TEXT_LG).line_height(ui::LINE_LG).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)))
            .children(create)
            .child(ui::icon_button("events-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.close_event_layer(cx))));
        let now = Local::now();
        let body: AnyElement = match events.is_empty() {
            true => self.events_empty(guild, cx),
            // lint-allow(unvirtualized-list): one server's scheduled events, at most 100
            false => ui::col()
                .id("events-list")
                .max_h(px(LIST_HEIGHT))
                .overflow_y_scroll()
                .gap_3()
                .children(events.into_iter().map(|e| self.event_card(e, &now, true, cx)))
                .into_any_element(),
        };
        let backdrop = div().id("events-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_event_layer(cx)));
        Some(ui::modal(backdrop, ui::dialog("events", LIST_WIDTH, ui::dialog_body().child(head).child(body), window, cx), cx))
    }

    pub(super) fn events_empty(&self, guild: &Guild, cx: &mut Context<Self>) -> AnyElement {
        let gid = guild.id.clone();
        let roles = guild.can(self.my_id(), perm::MANAGE_ROLES).then(|| {
            let link = div()
                .id("events-roles-link")
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::accent_ink(cx))
                .cursor_pointer()
                .hover(|s| s.underline())
                .child("Server Settings > Roles")
                .on_click(cx.listener(move |this, _, window, cx| this.open_roles_settings(gid.clone(), window, cx)));
            ui::row().items_stretch().max_w_80().text_center().flex_wrap().justify_center().gap_x_1().text_size(ui::TEXT_XS).text_color(palette::faint(cx)).child("You can give other people permission to create events in").child(link).child(".")
        });
        ui::empty_state("events-empty", Mood::Idle, "There are no upcoming events.", "Schedule an event for any planned activity in your server.", cx).children(roles).into_any_element()
    }

    // One card (`.ev2`): when, who and how many; the name and two lines of description; where, and the actions.
    // `actions` off draws the wizard's preview.
    pub(super) fn event_card(&self, event: &ScheduledEvent, now: &DateTime<Local>, actions: bool, cx: &mut Context<Self>) -> AnyElement {
        let when = when(event, now);
        let ink = phase_ink(when.phase, cx);
        let (strong, muted) = (palette::strong(cx), palette::muted(cx));
        let canceled = when.phase == Phase::Canceled;
        let cover = cover_source(event).map(|source| div().pb_1p5().child(ui::media(source, ui::all(ui::RADIUS_MD)).w_full().h(px(COVER_HEIGHT))));
        let head = ui::row()
            .items_start()
            .gap_2()
            .child(div().pt_px().text_color(ink).child(Icon::new(if canceled { Glyph::CalendarX } else { Glyph::Calendar }).size(ui::ICON_LG)))
            .child(when_column(&when, repeat_label(event), ink, cx))
            .child(self.who(event, cx));
        let card = ui::col()
            .id(SharedString::from(format!("event-card-{}", event.id)))
            .gap_1p5()
            .p_4()
            .flex_none()
            .squircle(ui::RADIUS_LG)
            .bg(palette::surface(palette::Level::Page, cx))
            .border_1()
            .border_color(palette::line(cx))
            .children(cover)
            .child(head)
            .child(div().pt_0p5().text_size(px(TITLE_SIZE)).line_height(ui::LINE_LEAD).font_weight(FontWeight::SEMIBOLD).text_color(if canceled { muted } else { strong }).child(event.name.clone()))
            .children(event.description.as_deref().map(|d| description_view(&format!("event-card-md-{}", event.id), d, cx).max_h_10().overflow_hidden()))
            .child(self.card_foot(event, actions, cx));
        if !actions {
            return card.into_any_element();
        }
        let (g, e) = (event.guild_id.clone(), event.id.clone());
        let hover = palette::line_strong(cx);
        card.cursor_pointer().hover(move |s| s.border_color(hover)).on_click(cx.listener(move |this, _, _, cx| this.open_event_details(g.clone(), e.clone(), cx))).into_any_element()
    }

    // The creator's avatar and the Interested count, with the tooltip naming both.
    pub(super) fn who(&self, event: &ScheduledEvent, cx: &mut Context<Self>) -> AnyElement {
        let creator = self.event_creator(event);
        let tip = match &creator {
            Some(user) => format!("Created by {} · {} people are interested", user.name, event.interested),
            None => format!("{} people are interested", event.interested),
        };
        ui::row()
            .id(SharedString::from(format!("event-who-{}", event.id)))
            .flex_none()
            .h_5()
            .gap_1()
            .text_size(ui::TEXT_XS)
            .text_color(palette::muted(cx))
            .children(creator.map(|u| div().pr_1().child(ui::avatar(u.avatar, 20.))))
            .child(Icon::new(Glyph::Users).size(px(COUNT_ICON)))
            .child(event.interested.to_string())
            .tooltip(move |window, cx| ui::tip(tip.clone(), window, cx))
            .into_any_element()
    }

    pub(super) fn card_foot(&self, event: &ScheduledEvent, actions: bool, cx: &mut Context<Self>) -> AnyElement {
        div()
            .pt_1p5()
            .child(
                ui::row()
                    .pt_3()
                    .border_t_1()
                    .border_color(palette::line(cx))
                    .gap_2()
                    .child(self.event_location(event, actions, cx))
                    .when(actions, |d| d.children(self.event_actions(event, true, cx))),
            )
            .into_any_element()
    }

    // The channel (opens it) or the place (a link opens in the browser).
    pub(super) fn event_location(&self, event: &ScheduledEvent, live: bool, cx: &mut Context<Self>) -> AnyElement {
        let channel = event.channel_id.as_deref().and_then(|c| self.event_guild(&event.guild_id)?.channel(c)).cloned();
        let icon = match &channel {
            Some(c) => crate::sidebar::channel_glyph(c),
            None => Icon::new(Glyph::MapPoint),
        };
        let place = self.event_place(event);
        let link = place.starts_with("http://") || place.starts_with("https://");
        let text = palette::text(cx);
        ui::row()
            .id(SharedString::from(format!("event-place-{}", event.id)))
            .flex_1()
            .gap_1p5()
            .text_size(ui::TEXT_SM)
            .text_color(text)
            .child(div().flex_none().text_color(palette::muted(cx)).child(icon.size(ui::ICON_LG)))
            .child(ui::line(place.clone()))
            .when(live && (channel.is_some() || link), |d| {
                d.cursor_pointer().hover(|s| s.underline()).on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    match channel.clone() {
                        Some(c) => {
                            this.leave_events();
                            this.open_channel(c, window, cx);
                        }
                        None => cx.open_url(&place),
                    }
                }))
            })
            .into_any_element()
    }

    // The footer's buttons (`428978.Zq`): more, then Interested, Start, Join or End Event as the state allows.
    pub(super) fn event_actions(&self, event: &ScheduledEvent, more: bool, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let manage = self.may_manage(event);
        let (active, canceled) = (event.status == ScheduledEvent::ACTIVE, event.status == ScheduledEvent::CANCELED);
        let external = event.entity_type == ScheduledEvent::EXTERNAL;
        let key = |part: &str| SharedString::from(format!("event-{part}-{}-{}", event.id, more));
        let ids = || (event.guild_id.clone(), event.id.clone());
        let mut out: Vec<AnyElement> = Vec::new();
        if more {
            let (g, e) = ids();
            out.push(
                ui::icon_button(key("more"), Glyph::MoreVertical, "More event options", cx)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.open_event_menu(g.clone(), e.clone(), window, cx);
                    }))
                    .into_any_element(),
            );
        }
        if canceled {
            return out;
        }
        if !active {
            let (g, e) = ids();
            let series = event.recurrence.is_some();
            let next = series.then(|| next_occurrence(event, Utc::now())).flatten();
            let on = next.as_ref().map_or(event.me_interested, |o| self.occurrence_interested(event, &o.recurrence_id));
            out.push(
                ui::button(key("interested"), Tone::Secondary, cx)
                    .small()
                    .toggled(on, cx)
                    .child(Icon::new(if on { Glyph::BellRinging } else { Glyph::Bell }).size(ui::ICON_MD))
                    .child("Interested")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        match series {
                            true => this.open_series_interested(g.clone(), e.clone(), window, cx),
                            false => this.toggle_interested(g.clone(), e.clone(), cx),
                        }
                    }))
                    .into_any_element(),
            );
        }
        if !active && manage && within_start_window(event, Utc::now()) {
            let (g, e) = ids();
            out.push(
                ui::button(key("start"), Tone::Primary, cx)
                    .small()
                    .child("Start")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.open_start_event(g.clone(), e.clone(), cx);
                    }))
                    .into_any_element(),
            );
        }
        let channel = event.channel_id.as_deref().and_then(|c| self.event_guild(&event.guild_id)?.channel(c)).cloned();
        if let Some(channel) = channel.filter(|_| active && !external) {
            let joined = self.call.as_ref().is_some_and(|c| c.channel.id == channel.id);
            let label = match (joined, is_stage(&channel)) {
                (true, _) => "Go To Channel",
                (false, true) => "Listen In",
                (false, false) => "Join",
            };
            out.push(
                ui::button(key("join"), Tone::Primary, cx)
                    .small()
                    .child(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.leave_events();
                        match joined {
                            true => this.open_channel(channel.clone(), window, cx),
                            false => this.join_voice(channel.clone(), cx),
                        }
                    }))
                    .into_any_element(),
            );
        }
        if active && external && manage {
            let (g, e) = ids();
            out.push(
                ui::button(key("end"), Tone::Secondary, cx)
                    .small()
                    .child("End Event")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.confirm_end_event(g.clone(), e.clone(), cx);
                    }))
                    .into_any_element(),
            );
        }
        out
    }

    pub(super) fn event_details_dialog(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let details = self.events.details.as_ref()?;
        let event = self.scheduled_event(&details.guild_id, &details.event_id)?;
        let cover = cover_source(event).map(|source| ui::media(source, ui::all(ui::RADIUS_LG)).w_full().h_40());
        let head = ui::row()
            .items_start()
            .gap_3()
            .child(div().flex_1().min_w_0().text_size(ui::TEXT_LG).line_height(ui::LINE_LG).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(event.name.clone()))
            .child(ui::icon_button("event-details-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.close_event_layer(cx))));
        let items = [ui::Tab::new("Event Info"), ui::Tab::new(format!("{} Interested", event.interested))];
        let this = cx.entity().downgrade();
        let tabs = ui::tabs("event-details-tabs", &items, details.tab, move |i, _, cx| {
            this.update(cx, |this, cx| this.set_details_tab(i, cx)).ok();
        }, window, cx);
        let body = match details.tab {
            0 => self.event_info(event, cx),
            _ => self.interested_list(event, details.users.as_ref(), cx),
        };
        let foot = ui::row().justify_end().gap_2().children(self.event_actions(event, true, cx));
        let body = ui::dialog_body().children(cover).child(head).child(tabs).child(body).child(foot);
        let backdrop = div().id("event-details-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_event_layer(cx)));
        Some(ui::modal(backdrop, ui::dialog("event-details", DETAILS_WIDTH, body, window, cx), cx))
    }

    pub(super) fn event_info(&self, event: &ScheduledEvent, cx: &mut Context<Self>) -> AnyElement {
        let now = Local::now();
        let when = when(event, &now);
        let ink = phase_ink(when.phase, cx);
        let (start, end) = (parse_time(&event.start).map(|t| t.with_timezone(&Local)), event.end.as_deref().and_then(parse_time).map(|t| t.with_timezone(&Local)));
        let dates = start.map(|s| range_line(&s, end.as_ref(), &now)).unwrap_or_default();
        let location = self.event_location(event, true, cx);
        let fact = |icon: AnyElement, text: String| ui::row().gap_2p5().child(ui::row().items_stretch().flex_none().w_5().justify_center().text_color(palette::muted(cx)).child(icon)).child(div().min_w_0().child(text));
        let creator = self.event_creator(event);
        ui::col()
            .gap_3()
            .child(ui::row().items_start().gap_2().child(div().pt_px().text_color(ink).child(Icon::new(Glyph::Calendar).size(ui::ICON_LG))).child(when_column(&when, repeat_label(event), ink, cx)))
            .children(event.description.as_deref().map(|d| description_view(&format!("event-info-md-{}", event.id), d, cx)))
            .child(
                ui::col()
                    .gap_2()
                    .text_size(ui::TEXT_SM)
                    .text_color(palette::text(cx))
                    .child(fact(Icon::new(Glyph::Clock).size(ui::ICON_LG).into_any_element(), dates))
                    .child(ui::row().items_stretch().child(location))
                    .children(creator.map(|u| fact(ui::avatar(u.avatar.clone(), 20.).into_any_element(), format!("Created by {}", u.name))))
                    .child(fact(Icon::new(Glyph::Users).size(ui::ICON_LG).into_any_element(), format!("{} people are interested", event.interested))),
            )
            .children(self.occurrence_rows(event, cx))
            .into_any_element()
    }

    // A series' next occurrences, each with its menu; canceled ones stay, marked, so they can be restored.
    pub(super) fn occurrence_rows(&self, event: &ScheduledEvent, cx: &mut Context<Self>) -> Option<AnyElement> {
        let list = occurrences(event, Utc::now(), OCCURRENCES_SHOWN);
        if list.is_empty() {
            return None;
        }
        let now = Local::now();
        let rows = list.into_iter().map(|o| {
            let (g, e, r) = (event.guild_id.clone(), event.id.clone(), o.recurrence_id.clone());
            let start = o.start.with_timezone(&Local);
            let interested = !o.canceled && self.occurrence_interested(event, &o.recurrence_id);
            let more = ui::icon_button(SharedString::from(format!("event-occurrence-more-{}", o.recurrence_id)), Glyph::MoreVertical, "More event options", cx).on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open_occurrence_menu(g.clone(), e.clone(), r.clone(), window, cx);
            }));
            ui::row()
                .gap_2p5()
                .py_1()
                .child(div().flex_none().text_color(if o.canceled { palette::danger_ink(cx) } else { palette::muted(cx) }).child(Icon::new(if o.canceled { Glyph::CalendarX } else { Glyph::Calendar }).size(ui::ICON_MD)))
                .child(ui::line(date_line(&start, &now)).flex_1().text_color(if o.canceled { palette::muted(cx) } else { palette::text(cx) }).when(o.canceled, |d| d.line_through()))
                .when(o.canceled, |d| d.child(div().flex_none().text_size(ui::TEXT_XS).font_weight(FontWeight::SEMIBOLD).text_color(palette::danger_ink(cx)).child("Canceled")))
                .when(interested, |d| d.child(div().flex_none().text_color(palette::accent_ink(cx)).child(Icon::new(Glyph::BellRinging).size(ui::ICON_SM))))
                .child(more)
        });
        let head = div().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(palette::muted(cx)).child("Events in this series");
        Some(ui::col().gap_0p5().pt_1().child(head).children(rows).into_any_element())
    }

    pub(super) fn interested_list(&self, event: &ScheduledEvent, users: Option<&Result<Vec<User>, String>>, cx: &mut Context<Self>) -> AnyElement {
        let note = |text: String| div().py_6().text_center().text_color(palette::muted(cx)).child(text).into_any_element();
        let users = match users {
            None => return ui::col().gap_2().children((0..3).map(|i| ui::skeleton(SharedString::from(format!("event-user-skeleton-{i}")), ui::RADIUS_SM, cx).h_9())).into_any_element(),
            Some(Err(_)) => return note("We are having problems looking up who is interested in this event. Please try again later.".into()),
            Some(Ok(users)) if users.is_empty() => return note("No one's interested in this event yet.".into()),
            Some(Ok(users)) => users,
        };
        let more = (event.interested as usize).saturating_sub(users.len());
        let rows = users.iter().map(|u| {
            ui::row()
                .gap_2p5()
                .px_2()
                .py_1p5()
                .child(ui::avatar(u.avatar.clone(), 32.))
                .child(ui::line(u.name.clone()).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)))
        });
        ui::col()
            .id("event-interested")
            .max_h(px(INTERESTED_HEIGHT))
            .overflow_y_scroll()
            .gap_0p5()
            .children(rows)
            .when(more > 0, |d| d.child(div().px_2().pt_2().text_size(ui::TEXT_XS).text_color(palette::muted(cx)).child(format!("{more} more people are interested."))))
            .into_any_element()
    }

    pub(super) fn start_event_dialog(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let start = self.events.start.as_ref()?;
        let event = self.scheduled_event(&start.guild_id, &start.event_id)?;
        let notify = (event.entity_type == ScheduledEvent::STAGE).then(|| {
            let mark = ui::check(if start.notify { ui::Check::On } else { ui::Check::Off }, cx);
            ui::pick_row("event-start-notify", mark, "Notify @everyone", None, cx).on_click(cx.listener(|this, _, _, cx| {
                if let Some(start) = this.events.start.as_mut() {
                    start.notify = !start.notify;
                }
                cx.notify(); // lint-allow(global-notify): the Start dialog is drawn by the Overlays view, which re-renders only with the app
            }))
        });
        let foot = ui::row().items_stretch()
            .justify_end()
            .gap_2()
            .child(ui::button("event-start-cancel", Tone::Secondary, cx).child("Cancel").on_click(cx.listener(|this, _, _, cx| this.close_event_layer(cx))))
            .child(ui::button("event-start-go", Tone::Primary, cx).child("Start Event").on_click(cx.listener(|this, _, _, cx| this.start_event(cx))));
        let body = ui::dialog_body()
            .child(div().text_size(ui::TEXT_LG).line_height(ui::LINE_LG).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(event.name.clone()))
            .child(div().text_color(palette::muted(cx)).child("You are about to start this Private event"))
            .children(notify)
            .child(foot);
        let backdrop = div().id("event-start-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_event_layer(cx)));
        Some(ui::modal(backdrop, ui::dialog("event-start", 420., body, window, cx), cx))
    }

    pub(super) fn event_wizard_dialog(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let w = self.events.wizard.as_ref()?;
        let (title, subtitle) = match (w.step, &w.occurrence) {
            (_, Some(_)) => ("Edit this Event", Some("Only this event in the series changes.")),
            (Step::Location, None) => ("Where is your event?", Some("So no one gets lost on where to go.")),
            (Step::Info, None) => ("What's your event about?", Some("Fill out the details of your event.")),
            (Step::Review, None) => ("Here's a preview of your event.", None),
            (Step::Success, None) => ("All set. Now share your event!", None),
        };
        let head = ui::row()
            .items_start()
            .gap_3()
            .child(
                ui::col()
                    .flex_1()
                    .gap_0p5()
                    .child(div().text_size(ui::TEXT_LG).line_height(ui::LINE_LG).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(title))
                    .children(subtitle.map(|s| div().text_size(ui::TEXT_SM).text_color(palette::muted(cx)).child(s))),
            )
            .child(ui::icon_button("event-wizard-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.close_event_layer(cx))));
        let problems = self.wizard_problems(cx);
        let step = match (w.step, w.occurrence.is_some()) {
            (_, true) => self.wizard_times_fields(w, &problems, window, cx),
            (Step::Location, false) => self.wizard_location(w, &problems, cx),
            (Step::Info, false) => self.wizard_info(w, &problems, window, cx),
            (Step::Review, false) => self.wizard_review(w, cx),
            (Step::Success, false) => wizard_success(w, cx),
        };
        let ok = problems.step_ok(if w.occurrence.is_some() { Step::Review } else { w.step });
        let body = ui::dialog_body()
            .child(head)
            .when(w.step != Step::Success && w.occurrence.is_none(), |d| d.child(stepper(w.step, cx)))
            .child(ui::dialog_scroll("event-wizard-fields", [step], window, cx))
            .when(w.step != Step::Success, |d| d.child(wizard_foot(w, ok, cx)));
        let backdrop = div().id("event-wizard-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_event_layer(cx)));
        Some(ui::modal(backdrop, ui::dialog("event-wizard", WIZARD_WIDTH, body, window, cx), cx))
    }

    pub(super) fn wizard_location(&self, w: &Wizard, problems: &Problems, cx: &mut Context<Self>) -> AnyElement {
        let guild = self.event_guild(&w.guild_id);
        let community = guild.is_some_and(|g| g.features.iter().any(|f| f == "COMMUNITY"));
        let me = self.my_id();
        let options = [
            (ScheduledEvent::STAGE, crate::sidebar::Nav::Stage.icon(), "Stage Channel", "Great for larger community audio events."),
            (ScheduledEvent::VOICE, Icon::new(Glyph::VolumeHigh), "Voice Channel", "Hang out with voice, video, screenshare, and Go Live."),
            (ScheduledEvent::EXTERNAL, Icon::new(Glyph::MapPoint), "Somewhere Else", "Text channel, external link, or in-person location."),
        ];
        let rows: Vec<AnyElement> = options.into_iter().filter(|(kind, ..)| *kind != ScheduledEvent::STAGE || community).map(|(kind, icon, name, hint)| {
            let empty = kind != ScheduledEvent::EXTERNAL && guild.is_none_or(|g| event_channels(g, me, kind).is_empty());
            let detail = if empty { "There are no available channels." } else { hint };
            let on = w.entity_type == kind;
            ui::pick_row_icon(SharedString::from(format!("event-kind-{kind}")), ui::radio(on, cx), icon, name, Some(detail.into()), cx)
                .disabled(empty || w.started)
                .on_click(cx.listener(move |this, _, _, cx| {
                    let first = this.events.wizard.as_ref().and_then(|w| this.event_guild(&w.guild_id)).and_then(|g| event_channels(g, this.my_id(), kind).first().map(|c| c.0.clone()));
                    this.set_wizard(cx, |w| {
                        if w.entity_type != kind {
                            w.channel_id = first;
                        }
                        w.entity_type = kind;
                    });
                }))
                .into_any_element()
        }).collect();
        let place: AnyElement = match w.entity_type {
            ScheduledEvent::EXTERNAL => {
                let focus = w.location.read(cx).focus_handle(cx);
                ui::field("Enter a location", cx).child(ui::text_field("event-location", Input::new(&w.location).appearance(false), &focus)).into_any_element()
            }
            kind => {
                let name = w.channel_id.as_deref().and_then(|c| guild?.channel(c)).map_or("Select a channel".to_owned(), |c| c.name.clone());
                let glyph = (kind == ScheduledEvent::VOICE).then_some(Glyph::VolumeHigh);
                ui::field("Select a channel", cx)
                    .child(ui::select("event-channel", glyph, name, false, cx).on_click(cx.listener(|this, _, window, cx| this.open_event_channel_menu(window, cx))))
                    .child(ui::field_hint("Events in private channels will only be visible to people who can access that channel.", None, cx))
                    .into_any_element()
            }
        };
        ui::col()
            .gap_3()
            .when(w.started, |d| d.child(ui::callout(ui::Notice::Info, Glyph::Info, "Location", "You cannot edit the location type of an event after it starts.", cx)))
            .child(ui::col().gap_0p5().children(rows))
            .child(place)
            .when(w.step == Step::Location && w.entity_type == ScheduledEvent::EXTERNAL, |d| d.children(problems.location.filter(|_| !w.location.read(cx).value().is_empty()).map(|p| ui::field_error(p, cx))))
            .into_any_element()
    }

    pub(super) fn wizard_info(&self, w: &Wizard, problems: &Problems, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let typed_topic = !w.topic.read(cx).value().trim().is_empty();
        let focus = w.topic.read(cx).focus_handle(cx);
        let topic_field = ui::text_field("event-topic", Input::new(&w.topic).appearance(false), &focus).error(problems.topic.is_some() && typed_topic);
        let topic = ui::field("Event Topic", cx).child(topic_field).children(problems.topic.filter(|_| typed_topic).map(|p| ui::field_error(p, cx)));
        let (start_at, _) = self.wizard_times(cx);
        let day = start_at.map_or_else(|| Local::now().date_naive(), |s| s.with_timezone(&Local).date_naive());
        let frequency = ui::field("Event Frequency", cx).child(ui::select("event-frequency", Some(Glyph::Repeat), w.frequency.label(day), false, cx).on_click(cx.listener(|this, _, window, cx| this.open_frequency_menu(window, cx))));
        let focus = w.description.read(cx).focus_handle(cx);
        let length = w.description.read(cx).value().chars().count();
        let description = ui::field("Description", cx)
            .child(ui::text_area("event-description", Textarea::new(&w.description).appearance(false).px_0().text_size(ui::TEXT_SM), &focus).error(length > DESCRIPTION_MAX))
            .child(ui::field_hint("", Some(format!("{length} / {DESCRIPTION_MAX}").into()), cx));
        ui::col()
            .gap_3p5()
            .child(topic)
            .child(self.wizard_times_fields(w, problems, window, cx))
            .child(frequency)
            .child(description)
            .child(self.wizard_cover(w, cx))
            .into_any_element()
    }

    // Start Date and Start Time, then End Date and End Time for somewhere else, each pair's error under it.
    pub(super) fn wizard_times_fields(&self, w: &Wizard, problems: &Problems, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let pair = |a: AnyElement, b: AnyElement| ui::row().items_stretch().gap_3().child(div().flex_1().min_w_0().child(a)).child(div().flex_1().min_w_0().child(b));
        let start = pair(
            ui::field("Start Date", cx).child(ui::date_field("event-start-date", &w.start_date, problems.start.is_some(), window, cx)).into_any_element(),
            ui::field("Start Time", cx).child(ui::time_field("event-start-time", &w.start_time, problems.start.is_some(), window, cx)).into_any_element(),
        );
        let end = (w.entity_type == ScheduledEvent::EXTERNAL).then(|| {
            pair(
                ui::field("End Date", cx).child(ui::date_field("event-end-date", &w.end_date, problems.end.is_some(), window, cx)).into_any_element(),
                ui::field("End Time", cx).child(ui::time_field("event-end-time", &w.end_time, problems.end.is_some(), window, cx)).into_any_element(),
            )
        });
        ui::col()
            .gap_3p5()
            .child(start)
            .children(problems.start.map(|p| ui::field_error(p, cx)))
            .children(end)
            .children(problems.end.map(|p| ui::field_error(p, cx)))
            .into_any_element()
    }

    // "Cover Image": Upload Cover Image, or the picked or current cover with Change Image and Remove Image.
    pub(super) fn wizard_cover(&self, w: &Wizard, cx: &mut Context<Self>) -> AnyElement {
        let kept = w.editing.as_deref().and_then(|id| self.scheduled_event(&w.guild_id, id)).and_then(|e| e.image.clone());
        let preview = ScheduledEvent { id: w.editing.clone().unwrap_or_default(), image: wizard_image(&w.cover, kept), ..Default::default() };
        let hint = ui::field_hint("We recommend an image that's at least 800px wide and 320px tall.", None, cx);
        let field = ui::field("Cover Image", cx);
        let Some(source) = cover_source(&preview) else {
            let upload = ui::button("event-cover-upload", Tone::Secondary, cx).small().child(Icon::new(Glyph::Upload).size(ui::ICON_MD)).child("Upload Cover Image").on_click(cx.listener(|this, _, _, cx| this.pick_cover(cx)));
            return field.child(ui::row().items_stretch().child(upload)).child(hint).into_any_element();
        };
        let change = ui::button("event-cover-change", Tone::Secondary, cx).small().child("Change Image").on_click(cx.listener(|this, _, _, cx| this.pick_cover(cx)));
        let remove = ui::button("event-cover-remove", Tone::Ghost, cx).small().child("Remove Image").on_click(cx.listener(|this, _, _, cx| this.set_wizard(cx, |w| w.cover = Cover::Removed)));
        field
            .child(ui::media(source, ui::all(ui::RADIUS_MD)).w_full().h(px(WIZARD_COVER_HEIGHT)))
            .child(ui::row().items_stretch().gap_2().child(change).child(remove))
            .child(hint)
            .into_any_element()
    }

    pub(super) fn wizard_review(&self, w: &Wizard, cx: &mut Context<Self>) -> AnyElement {
        let (start, end) = self.wizard_times(cx);
        let external = w.entity_type == ScheduledEvent::EXTERNAL;
        let kept = w.editing.as_deref().and_then(|id| self.scheduled_event(&w.guild_id, id)).and_then(|e| e.image.clone());
        let preview = ScheduledEvent {
            id: w.editing.clone().unwrap_or_else(|| "preview".into()),
            image: wizard_image(&w.cover, kept),
            guild_id: w.guild_id.clone(),
            channel_id: if external { None } else { w.channel_id.clone() },
            creator_id: self.me.as_ref().map(|m| m.id.clone()),
            creator: self.me.clone(),
            name: w.topic.read(cx).value().trim().to_owned(),
            description: Some(w.description.read(cx).value().trim().to_owned()).filter(|d| !d.is_empty()),
            start: start.map(|s| s.to_rfc3339()).unwrap_or_default(),
            end: if external { end.map(|e| e.to_rfc3339()) } else { None },
            status: ScheduledEvent::SCHEDULED,
            entity_type: w.entity_type,
            location: external.then(|| w.location.read(cx).value().trim().to_owned()),
            recurrence: start.and_then(|s| w.frequency.rule(s, s.with_timezone(&Local).date_naive())),
            ..Default::default()
        };
        let channel = self.event_place(&preview);
        let note = match external {
            true => div().child("This event will auto start when it's time."),
            false => ui::row().items_stretch().flex_wrap().gap_1().child("When it's time, visit").child(div().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(format!("#{channel}"))).child("to start the event!"),
        };
        ui::col().gap_3().child(self.event_card(&preview, &Local::now(), false, cx)).child(note.text_size(ui::TEXT_SM).text_color(palette::muted(cx))).into_any_element()
    }
}

// A description as Discord renders it: markdown, line breaks kept, custom emoji as images (the profile bio's way).
pub(super) fn description_view(id: &str, text: &str, cx: &App) -> gpui_kit::Div {
    let view = TextView::markdown(SharedString::from(id.to_owned()), crate::chat::inline_emoji(&crate::chat::discord_breaks(text)));
    div().min_w_0().text_size(ui::TEXT_SM).line_height(ui::LINE_SM).text_color(palette::text(cx)).child(view)
}

// The status line's column: the coloured lead and the rest on one line, the repeat under it.
pub(super) fn when_column(when: &When, repeat: Option<String>, ink: Hsla, cx: &gpui_kit::App) -> AnyElement {
    let muted = palette::muted(cx);
    let line = ui::row().items_stretch()
        .flex_wrap()
        .gap_1()
        .children(when.lead.clone().map(|lead| div().font_weight(FontWeight::SEMIBOLD).text_color(ink).child(lead)))
        .when(when.lead.is_some() && when.rest.is_some(), |d| d.child("·"))
        .children(when.rest.clone());
    ui::col()
        .flex_1()
        .text_size(ui::TEXT_SM)
        .line_height(ui::LINE_SM)
        .text_color(muted)
        .child(line)
        .children(repeat.map(|r| ui::row().gap_1().text_size(ui::TEXT_XS).line_height(ui::LINE_DETAIL).child(Icon::new(Glyph::Repeat).size(px(COUNT_ICON))).child(r)))
        .into_any_element()
}

// `.cm-stepper`: Location, Event Info, Review; done steps take a tick, the current one the accent.
// lint-allow(custom-component): ui::tabs switches between views; this is a numbered progress of steps done, current and next
pub(super) fn stepper(step: Step, cx: &gpui_kit::App) -> AnyElement {
    let at = match step {
        Step::Location => 0,
        Step::Info => 1,
        _ => 2,
    };
    let items = ["Location", "Event Info", "Review"].into_iter().enumerate().map(|(i, label)| {
        let (done, current) = (i < at, i == at);
        let mark = ui::row()
            .size_5()
            .flex_none()
            .rounded_full()
            .justify_center()
            .text_size(ui::TEXT_XXS)
            .map(|d| match (done, current) {
                (true, _) => d.bg(palette::selected(cx)).text_color(palette::strong(cx)).child(Icon::new(Glyph::Checkmark).size(ui::ICON_XS)),
                (_, true) => d.bg(palette::accent()).text_color(palette::on_accent()).child((i + 1).to_string()),
                _ => d.border(px(STEP_LINE)).border_color(palette::line_strong(cx)).child((i + 1).to_string()),
            });
        let ink = if current { palette::strong(cx) } else if done { palette::muted(cx) } else { palette::faint(cx) };
        ui::row()
            .gap_1p5()
            .text_size(ui::TEXT_XS)
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(ink)
            // lint-allow(custom-divider): ui::divider is a full-width hairline; this is the short rule joining two steps
            .when(i > 0, |d| d.child(div().w_5().h(px(STEP_LINE)).bg(palette::line_strong(cx))))
            .child(mark)
            .child(label)
    });
    ui::row().gap_2().children(items).into_any_element()
}

pub(super) fn wizard_foot(w: &Wizard, ok: bool, cx: &mut Context<Waffle>) -> AnyElement {
    let primary = match (w.step, &w.editing) {
        _ if w.occurrence.is_some() => "Save Event",
        (Step::Review, Some(_)) => "Save Event",
        (Step::Review, None) => "Create Event",
        _ => "Next",
    };
    let occurrence = w.occurrence.is_some();
    let review = w.step == Step::Review || occurrence;
    let busy = w.busy.then_some(if w.editing.is_some() { "Saving…" } else { "Creating…" });
    ui::row()
        .gap_2()
        .when(w.step != Step::Location && !occurrence, |d| d.child(ui::button("event-wizard-back", Tone::Secondary, cx).small().child("Back").on_click(cx.listener(|this, _, _, cx| this.wizard_step(false, cx)))))
        .child(div().flex_1())
        .child(ui::button("event-wizard-cancel", Tone::Secondary, cx).child("Cancel").on_click(cx.listener(|this, _, _, cx| this.close_event_layer(cx))))
        .child(ui::button("event-wizard-next", Tone::Primary, cx).disabled(!ok || w.busy).loading(busy).child(primary).on_click(cx.listener(move |this, _, _, cx| match review {
            true => this.wizard_submit(cx),
            false => this.wizard_step(true, cx),
        })))
        .into_any_element()
}

pub(super) fn wizard_success(w: &Wizard, cx: &mut Context<Waffle>) -> AnyElement {
    let link = w.link.clone().unwrap_or_default();
    let field = ui::field_frame(cx)
        .pr_1()
        .child(ui::line(link.clone()).flex_1().text_size(ui::TEXT_SM))
        .child(ui::button("event-link-copy", Tone::Primary, cx).small().child("Copy").on_click(cx.listener(move |this, _, _, cx| this.copy_event_link(link.clone(), cx))));
    ui::col()
        .gap_3()
        .child(div().text_color(palette::muted(cx)).child("Copy the event link below to invite people to your event. This also acts as a server invite link."))
        .child(field)
        .into_any_element()
}
