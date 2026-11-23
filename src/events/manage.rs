// The events the app holds: their lists and menus, RSVPs, Start, End and Cancel, and the calendar exports.
use super::*;

impl Waffle {
    pub(super) fn scheduled_event(&self, guild_id: &str, event_id: &str) -> Option<&ScheduledEvent> {
        self.guilds.iter().find(|g| g.id == guild_id)?.events.iter().find(|e| e.id == event_id)
    }

    pub(super) fn scheduled_event_mut(&mut self, guild_id: &str, event_id: &str) -> Option<&mut ScheduledEvent> {
        self.guilds.iter_mut().find(|g| g.id == guild_id)?.events.iter_mut().find(|e| e.id == event_id)
    }

    pub(super) fn event_guild(&self, guild_id: &str) -> Option<&Guild> {
        self.guilds.iter().find(|g| g.id == guild_id)
    }

    pub(super) fn may_manage(&self, event: &ScheduledEvent) -> bool {
        self.event_guild(&event.guild_id).is_some_and(|g| can_manage(g, self.my_id(), event))
    }

    // Where it happens, as words: the channel's name or the location.
    pub(super) fn event_place(&self, event: &ScheduledEvent) -> String {
        match event.entity_type {
            ScheduledEvent::EXTERNAL => event.location.clone().unwrap_or_default(),
            _ => event.channel_id.as_deref().and_then(|c| self.event_guild(&event.guild_id)?.channel(c)).map(|c| c.name.clone()).unwrap_or_default(),
        }
    }

    pub(super) fn event_creator(&self, event: &ScheduledEvent) -> Option<User> {
        let id = event.creator_id.as_deref()?;
        event.creator.clone().or_else(|| self.members.get(&event.guild_id)?.get(id).cloned())
    }

    // A GET whose body the caller wants; a failure comes back as the error text.
    pub(super) fn fetch_json(&mut self, path: String, then: impl FnOnce(&mut Waffle, Result<Value, String>, &mut Context<Waffle>) + 'static, cx: &mut Context<Self>) {
        self.call_json(Method::GET, path, None, then, cx);
    }

    pub(super) fn call_json(&mut self, method: Method, path: String, body: Option<Value>, then: impl FnOnce(&mut Waffle, Result<Value, String>, &mut Context<Waffle>) + 'static, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            let result = client.call(method, path, body).await;
            this.update(cx, |this, cx| {
                then(this, result, cx);
                cx.notify(); // lint-allow(global-notify): the callback can change the events dialogs, the nav's Events row and its live bar
            })
            .ok();
        })
        .detach(); // lint-allow(detached-task): ends itself once its request resolves, and holds only a weak handle
    }

    pub(crate) fn open_events(&mut self, guild_id: String, cx: &mut Context<Self>) {
        self.menu = None;
        self.events.list = Some(guild_id.clone());
        self.fetch_event_counts(&guild_id, cx);
        cx.notify(); // lint-allow(global-notify): the events dialogs are drawn by the Overlays view, which re-renders only with the app
    }

    // On open, as Discord's list does: each event's Interested count, then which of them you marked.
    pub(super) fn fetch_event_counts(&mut self, guild_id: &str, cx: &mut Context<Self>) {
        let ids: Vec<String> = self.event_guild(guild_id).map(|g| g.events.iter().filter(|e| e.upcoming()).map(|e| e.id.clone()).collect()).unwrap_or_default();
        for id in ids {
            let (g, e) = (guild_id.to_owned(), id.clone());
            self.fetch_json(
                format!("/guilds/{guild_id}/scheduled-events/{id}/users/counts"),
                move |this, result, _| {
                    let Some(count) = result.ok().and_then(|v| v["guild_scheduled_event_count"].as_u64()) else { return };
                    if let Some(event) = this.scheduled_event_mut(&g, &e) {
                        event.interested = count as u32;
                    }
                },
                cx,
            );
        }
        let g = guild_id.to_owned();
        // Not verified: the response is a list of `{guild_scheduled_event_id, guild_scheduled_event_exception_id,
        // response, …}` for this server, the exception id set on single-occurrence answers.
        self.fetch_json(
            format!("/users/@me/scheduled-events?guild_ids={guild_id}"),
            move |this, result, _| {
                let Ok(list) = result else { return };
                let (series, single) = rsvps_of(&list);
                this.events.rsvps.extend(single);
                let Some(guild) = this.guilds.iter_mut().find(|x| x.id == g) else { return };
                for event in &mut guild.events {
                    event.me_interested = series.contains(&event.id);
                }
            },
            cx,
        );
    }

    // Whether you're Interested in one occurrence: its own answer, else the series'.
    pub(super) fn occurrence_interested(&self, event: &ScheduledEvent, recurrence_id: &str) -> bool {
        self.events.rsvps.get(&(event.id.clone(), recurrence_id.to_owned())).copied().unwrap_or(event.me_interested)
    }

    // One occurrence's Interested (`929267.QC`): differing from the series is an answer on the occurrence
    // (`UNINTERESTED` opts out of a series you follow); matching it again drops that answer.
    pub(super) fn set_occurrence_interested(&mut self, guild_id: String, event_id: String, recurrence_id: String, on: bool, cx: &mut Context<Self>) {
        let Some(series) = self.scheduled_event(&guild_id, &event_id).map(|e| e.me_interested) else { return };
        let path = format!("/guilds/{guild_id}/scheduled-events/{event_id}/{recurrence_id}/users/@me");
        let (method, body) = if on == series { (Method::DELETE, None) } else { (Method::PUT, Some(json!({ "response": u8::from(on) }))) };
        let key = (event_id, recurrence_id);
        let before = self.events.rsvps.get(&key).copied();
        let undo = key.clone();
        self.rest_optimistic(
            method,
            path,
            body,
            move |this| {
                match on == series {
                    true => this.events.rsvps.remove(&key),
                    false => this.events.rsvps.insert(key, on),
                };
            },
            move |this| {
                match before {
                    Some(was) => this.events.rsvps.insert(undo, was),
                    None => this.events.rsvps.remove(&undo),
                };
            },
            cx,
        );
    }

    // A series' Interested asks which: all of it or the next occurrence.
    pub(super) fn open_series_interested(&mut self, guild_id: String, event_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let at = window.mouse_position();
        self.open_live_menu(at, move |this| this.interested_entries(&guild_id, &event_id, None), cx);
    }

    // "All events in this series" / "Only this event", ticked as they stand. `recurrence_id` picks the
    // occurrence; none means the next one.
    pub(super) fn interested_entries(&self, guild_id: &str, event_id: &str, recurrence_id: Option<&str>) -> Vec<Entry> {
        let Some(event) = self.scheduled_event(guild_id, event_id) else { return Vec::new() };
        let Some(occurrence) = recurrence_id.map(str::to_owned).or_else(|| next_occurrence(event, Utc::now()).map(|o| o.recurrence_id)) else { return Vec::new() };
        let (series, single) = (event.me_interested, self.occurrence_interested(event, &occurrence));
        let (g, e) = (guild_id.to_owned(), event_id.to_owned());
        let (g2, e2) = (g.clone(), e.clone());
        vec![
            item("All events in this series", move |this, _, cx| this.toggle_interested(g.clone(), e.clone(), cx)).checked(series).entry(),
            item("Only this event", move |this, _, cx| this.set_occurrence_interested(g2.clone(), e2.clone(), occurrence.clone(), !single, cx)).checked(single).entry(),
        ]
    }

    pub(crate) fn open_event_details(&mut self, guild_id: String, event_id: String, cx: &mut Context<Self>) {
        self.menu = None;
        self.events.details = Some(Details { guild_id, event_id, tab: 0, users: None });
        cx.notify(); // lint-allow(global-notify): the events dialogs are drawn by the Overlays view, which re-renders only with the app
    }

    pub(super) fn set_details_tab(&mut self, tab: usize, cx: &mut Context<Self>) {
        let Some(details) = self.events.details.as_mut() else { return };
        details.tab = tab;
        let load = tab == 1 && details.users.is_none();
        let path = format!("/guilds/{}/scheduled-events/{}/users?limit=100&with_member=true", details.guild_id, details.event_id);
        let (guild_id, event_id) = (details.guild_id.clone(), details.event_id.clone());
        cx.notify(); // lint-allow(global-notify): the events dialogs are drawn by the Overlays view, which re-renders only with the app
        if !load {
            return;
        }
        self.fetch_json(
            path,
            move |this, result, _| {
                let Some(details) = this.events.details.as_mut().filter(|d| d.event_id == event_id) else { return };
                let users = result.map(|list| list.as_array().into_iter().flatten().map(|r| discord::parse_member_user(&r["member"], &r["user"], Some(&guild_id))).collect());
                details.users = Some(users);
            },
            cx,
        );
    }

    pub(super) fn set_interested(&mut self, guild_id: &str, event_id: &str, on: bool) {
        let Some(event) = self.scheduled_event_mut(guild_id, event_id).filter(|e| e.me_interested != on) else { return };
        event.me_interested = on;
        event.interested = if on { event.interested + 1 } else { event.interested.saturating_sub(1) };
    }

    pub(super) fn toggle_interested(&mut self, guild_id: String, event_id: String, cx: &mut Context<Self>) {
        let Some(on) = self.scheduled_event(&guild_id, &event_id).map(|e| e.me_interested) else { return };
        let path = format!("/guilds/{guild_id}/scheduled-events/{event_id}/users/@me");
        let (method, body) = if on { (Method::DELETE, None) } else { (Method::PUT, Some(json!({ "response": 1 }))) };
        let (g, e) = (guild_id.clone(), event_id.clone());
        self.rest_optimistic(method, path, body, move |this| this.set_interested(&guild_id, &event_id, !on), move |this| this.set_interested(&g, &e, on), cx);
    }

    pub(super) fn event_menu(&self, guild_id: &str, event_id: &str) -> Vec<Entry> {
        let Some(event) = self.scheduled_event(guild_id, event_id) else { return Vec::new() };
        let manage = self.may_manage(event);
        let (active, canceled) = (event.status == ScheduledEvent::ACTIVE, event.status == ScheduledEvent::CANCELED);
        let ids = || (guild_id.to_owned(), event_id.to_owned());
        let mut out = Vec::new();
        if !active {
            let calendar = |label: &'static str, glyph: Glyph, kind: Calendar| {
                let (g, e) = ids();
                item(label, move |this, _, cx| this.add_to_calendar(&g, &e, kind, cx)).icon(glyph).entry()
            };
            out.push(submenu("Add to Calendar", vec![calendar("Google", Glyph::Open, Calendar::Google), calendar("Download ICS", Glyph::Download, Calendar::Ics), calendar("Outlook", Glyph::Open, Calendar::Outlook)]).icon(Glyph::CalendarAdd).entry());
        }
        if manage && !active && !canceled {
            let (g, e) = ids();
            out.push(item("Start Event", move |this, _, cx| this.open_start_event(g.clone(), e.clone(), cx)).icon(Glyph::Play).entry());
        }
        // A series' items split into this (the next occurrence) and all of it.
        let next = event.recurrence.as_ref().and_then(|_| next_occurrence(event, Utc::now())).map(|o| o.recurrence_id);
        if manage && !canceled {
            let (g, e) = ids();
            let all = item(if next.is_some() { "Edit all Events in this Series" } else { "Edit Event" }, move |this, window, cx| this.open_event_wizard(g.clone(), Some(e.clone()), window, cx));
            out.push(match next.clone() {
                Some(occurrence) => {
                    let (g, e) = ids();
                    let this_one = item("Edit this Event", move |this, window, cx| this.open_occurrence_editor(g.clone(), e.clone(), occurrence.clone(), window, cx)).entry();
                    submenu("Edit Event", vec![this_one, all.entry()]).icon(Glyph::Pen).entry()
                }
                None => all.icon(Glyph::Pen).entry(),
            });
        }
        let link = event_link(event);
        let copy_all = item(if next.is_some() { "Copy Link for all Events" } else { "Copy Event Link" }, move |this, _, cx| this.copy_event_link(link.clone(), cx));
        out.push(match next.clone() {
            Some(occurrence) => {
                let link = occurrence_link(event, &occurrence);
                let only = item("Copy Link for Only this Event", move |this, _, cx| this.copy_event_link(link.clone(), cx)).entry();
                submenu("Copy Event Link", vec![only, copy_all.entry()]).icon(Glyph::Link).entry()
            }
            None => copy_all.icon(Glyph::Link).entry(),
        });
        if manage && !active && !canceled {
            let (g, e) = ids();
            out.push(Entry::Separator);
            let all = item(if next.is_some() { "Cancel all Events in this Series" } else { "Cancel Event" }, move |this, _, cx| this.confirm_cancel_event(g.clone(), e.clone(), cx)).danger();
            out.push(match next {
                Some(occurrence) => {
                    let (g, e) = ids();
                    let this_one = item("Cancel this Event", move |this, _, cx| this.confirm_cancel_occurrence(g.clone(), e.clone(), occurrence.clone(), cx)).danger().entry();
                    submenu("Cancel Event", vec![this_one, all.entry()]).danger().icon(Glyph::CalendarX).entry()
                }
                None => all.icon(Glyph::CalendarX).entry(),
            });
        }
        out
    }

    // An occurrence row's menu (`924037` on a recurrence): edit or cancel this one, Restore a canceled one,
    // its link, and its Interested.
    pub(super) fn occurrence_menu(&self, guild_id: &str, event_id: &str, recurrence_id: &str) -> Vec<Entry> {
        let Some(event) = self.scheduled_event(guild_id, event_id) else { return Vec::new() };
        let Some(occurrence) = occurrences(event, Utc::now(), usize::MAX).into_iter().find(|o| o.recurrence_id == recurrence_id) else { return Vec::new() };
        let manage = self.may_manage(event) && event.status != ScheduledEvent::CANCELED;
        let ids = || (guild_id.to_owned(), event_id.to_owned(), recurrence_id.to_owned());
        let mut out = Vec::new();
        if manage && !occurrence.canceled {
            let (g, e, r) = ids();
            out.push(item("Edit this Event", move |this, window, cx| this.open_occurrence_editor(g.clone(), e.clone(), r.clone(), window, cx)).icon(Glyph::Pen).entry());
        }
        if manage && occurrence.canceled {
            let (g, e, r) = ids();
            out.push(item("Restore Event", move |this, _, cx| this.restore_occurrence(&g, &e, &r, cx)).icon(Glyph::Calendar).entry());
        }
        let link = occurrence_link(event, recurrence_id);
        out.push(item("Copy Event Link", move |this, _, cx| this.copy_event_link(link.clone(), cx)).icon(Glyph::Link).entry());
        if !occurrence.canceled {
            let label = if self.occurrence_interested(event, recurrence_id) { "Mark as Not Interested" } else { "Mark as Interested" };
            out.push(submenu(label, self.interested_entries(guild_id, event_id, Some(recurrence_id))).icon(Glyph::Bell).entry());
        }
        if manage && !occurrence.canceled {
            let (g, e, r) = ids();
            out.push(Entry::Separator);
            out.push(item("Cancel this Event", move |this, _, cx| this.confirm_cancel_occurrence(g.clone(), e.clone(), r.clone(), cx)).danger().icon(Glyph::CalendarX).entry());
        }
        out
    }

    pub(super) fn open_occurrence_menu(&mut self, guild_id: String, event_id: String, recurrence_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let at = window.mouse_position();
        self.open_live_menu(at, move |this| this.occurrence_menu(&guild_id, &event_id, &recurrence_id), cx);
    }

    pub(super) fn find_occurrence(&self, guild_id: &str, event_id: &str, recurrence_id: &str) -> Option<(ScheduledEvent, Occurrence)> {
        let event = self.scheduled_event(guild_id, event_id)?;
        let occurrence = occurrences(event, Utc::now(), usize::MAX).into_iter().find(|o| o.recurrence_id == recurrence_id)?;
        Some((event.clone(), occurrence))
    }

    // Writes an occurrence's exception at once, putting the old one back if Discord refuses.
    pub(super) fn send_exception(&mut self, event: &ScheduledEvent, recurrence_id: String, (method, path, body): (Method, String, Option<Value>), cx: &mut Context<Self>) {
        let after = exception_from(&recurrence_id, body.as_ref());
        let before = event.exceptions.iter().find(|x| x.id == recurrence_id).cloned();
        let (g, e) = (event.guild_id.clone(), event.id.clone());
        let (g2, e2, r2) = (g.clone(), e.clone(), recurrence_id.clone());
        let swap = |this: &mut Waffle, g: &str, e: &str, id: &str, exception: Option<EventException>| {
            let Some(event) = this.scheduled_event_mut(g, e) else { return };
            event.exceptions.retain(|x| x.id != id);
            event.exceptions.extend(exception);
        };
        self.rest_optimistic(method, path, body, move |this| swap(this, &g, &e, &recurrence_id, after), move |this| swap(this, &g2, &e2, &r2, before), cx);
    }

    pub(super) fn confirm_cancel_occurrence(&mut self, guild_id: String, event_id: String, recurrence_id: String, cx: &mut Context<Self>) {
        self.confirm_delete(
            "Cancel Event?",
            "Are you sure you want to cancel this event?",
            "Cancel Event",
            move |this, _, cx| {
                let Some((event, occurrence)) = this.find_occurrence(&guild_id, &event_id, &recurrence_id) else { return };
                let (method, path, body) = exception_request(&event, &occurrence, occurrence.start, occurrence.end, true);
                this.send_exception(&event, recurrence_id.clone(), (method, path, Some(body)), cx);
            },
            cx,
        );
        self.set_dialog_cancel("Never Mind");
    }

    pub(super) fn restore_occurrence(&mut self, guild_id: &str, event_id: &str, recurrence_id: &str, cx: &mut Context<Self>) {
        let Some((event, occurrence)) = self.find_occurrence(guild_id, event_id, recurrence_id) else { return };
        let request = restore_request(&event, &occurrence);
        self.send_exception(&event, recurrence_id.to_owned(), request, cx);
    }

    // The server menu's "Create Event" (`499817`), after Create Channel and Create Category.
    pub(crate) fn create_event_entry(&self, guild_id: &str) -> Option<Entry> {
        let guild = self.event_guild(guild_id).filter(|g| can_create(g, self.my_id()))?;
        let gid = guild.id.clone();
        Some(item("Create Event", move |this, window, cx| this.open_event_wizard(gid.clone(), None, window, cx)).icon(Glyph::CalendarAdd).entry())
    }

    pub(super) fn open_event_menu(&mut self, guild_id: String, event_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let at = window.mouse_position();
        self.open_live_menu(at, move |this| this.event_menu(&guild_id, &event_id), cx);
    }

    pub(super) fn copy_event_link(&mut self, link: String, cx: &mut Context<Self>) {
        self.copy_text(link, cx);
        self.show_toast("Link Copied!", cx);
    }

    pub(super) fn add_to_calendar(&mut self, guild_id: &str, event_id: &str, kind: Calendar, cx: &mut Context<Self>) {
        let Some(event) = self.scheduled_event(guild_id, event_id).cloned() else { return };
        let place = self.event_place(&event);
        if let Some(url) = calendar_url(kind, &event, &place) {
            return cx.open_url(&url);
        }
        let Some(text) = ics(&event, &place, Utc::now()) else { return };
        // Windows sets USERPROFILE, not HOME.
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
        let Some(path) = home.map(|h| std::path::PathBuf::from(h).join("Downloads").join(ics_file_name(&event.name))) else { return };
        let name = ics_file_name(&event.name);
        // Written off the UI thread, then the toast says how it went.
        cx.spawn(async move |this, cx| {
            let written = cx.background_executor().spawn(async move { std::fs::write(&path, text) }).await;
            this.update(cx, |this, cx| match written {
                Ok(()) => this.show_toast(format!("Saved {name} to Downloads"), cx),
                Err(e) => this.show_toast(format!("Couldn't save the event: {e}"), cx),
            })
            .ok();
        })
        .detach(); // lint-allow(detached-task): ends itself once the file is written, and holds only a weak handle
    }

    pub(super) fn open_start_event(&mut self, guild_id: String, event_id: String, cx: &mut Context<Self>) {
        self.menu = None;
        self.events.start = Some(Start { guild_id, event_id, notify: true });
        cx.notify(); // lint-allow(global-notify): the events dialogs are drawn by the Overlays view, which re-renders only with the app
    }

    // Starting (`163765`): a stage event opens its stage, made first when it's gone; the rest switch to live.
    // Then you join the event's channel.
    pub(super) fn start_event(&mut self, cx: &mut Context<Self>) {
        let Some(start) = self.events.start.take() else { return };
        let Some(event) = self.scheduled_event(&start.guild_id, &start.event_id).cloned() else { return };
        let (g, e) = (event.guild_id.clone(), event.id.clone());
        if event.entity_type != ScheduledEvent::STAGE {
            let path = format!("/guilds/{g}/scheduled-events/{e}");
            return self.rest(Method::PATCH, path, Some(json!({ "status": ScheduledEvent::ACTIVE })), move |this, cx| this.event_started(&g, &e, cx), cx);
        }
        let stage = event.channel_id.as_deref().and_then(|c| self.event_guild(&g)?.channel(c)).cloned();
        match stage {
            Some(stage) => self.open_stage(&event, &stage.id, start.notify, cx),
            None => self.create_stage(event, start.notify, cx),
        }
    }

    pub(super) fn open_stage(&mut self, event: &ScheduledEvent, channel_id: &str, notify: bool, cx: &mut Context<Self>) {
        let body = json!({ "channel_id": channel_id, "topic": event.name, "privacy_level": PRIVATE, "guild_scheduled_event_id": event.id, "send_start_notification": notify });
        let (g, e) = (event.guild_id.clone(), event.id.clone());
        self.rest(Method::POST, "/stage-instances".into(), Some(body), move |this, cx| this.event_started(&g, &e, cx), cx);
    }

    // The stage is named after the event (`name.substring(0,100)`).
    // Not verified: whether Discord also points the event at the new stage; here only the stage instance names it.
    pub(super) fn create_stage(&mut self, event: ScheduledEvent, notify: bool, cx: &mut Context<Self>) {
        let name: String = event.name.chars().take(100).collect();
        let path = format!("/guilds/{}/channels", event.guild_id);
        self.call_json(
            Method::POST,
            path,
            Some(json!({ "name": name, "type": STAGE_TYPE })),
            move |this, result, cx| {
                let stage = match result.map(|v| discord::parse_channel(&v, &event.guild_id)) {
                    Ok(Some(stage)) => stage,
                    Ok(None) => return,
                    Err(error) => return this.show_toast(error, cx),
                };
                if let Some(guild) = this.guilds.iter_mut().find(|g| g.id == event.guild_id) {
                    if guild.channel(&stage.id).is_none() {
                        guild.channels.push(stage.clone());
                    }
                    if let Some(listed) = guild.events.iter_mut().find(|e| e.id == event.id) {
                        listed.channel_id = Some(stage.id.clone());
                    }
                }
                this.open_stage(&event, &stage.id, notify, cx);
            },
            cx,
        );
    }

    pub(super) fn event_started(&mut self, guild_id: &str, event_id: &str, cx: &mut Context<Self>) {
        let Some(event) = self.scheduled_event_mut(guild_id, event_id) else { return };
        event.status = ScheduledEvent::ACTIVE;
        let channel_id = event.channel_id.clone();
        let Some(channel) = channel_id.and_then(|c| self.event_guild(guild_id)?.channel(&c).cloned()) else { return };
        self.join_voice_in(Some(guild_id.to_owned()), channel, cx);
    }

    pub(super) fn confirm_end_event(&mut self, guild_id: String, event_id: String, cx: &mut Context<Self>) {
        self.confirm("End Event", "Are you sure you want to end the event?", "Yes, end the event", move |this, _, cx| {
            let (g, e) = (guild_id.clone(), event_id.clone());
            let path = format!("/guilds/{guild_id}/scheduled-events/{event_id}");
            this.rest(
                Method::PATCH,
                path,
                Some(json!({ "status": ScheduledEvent::COMPLETED })),
                move |this, _| {
                    if let Some(event) = this.scheduled_event_mut(&g, &e) {
                        event.status = ScheduledEvent::COMPLETED;
                    }
                },
                cx,
            );
        }, cx);
    }

    // Discord's Cancel Event deletes the event rather than setting status 4.
    pub(super) fn confirm_cancel_event(&mut self, guild_id: String, event_id: String, cx: &mut Context<Self>) {
        let series = self.scheduled_event(&guild_id, &event_id).is_some_and(|e| e.recurrence.is_some());
        let (body, button) = match series {
            true => ("Are you sure you want to cancel this event? Canceling this event will also cancel all events in this series.", "Cancel all Events"),
            false => ("Are you sure you want to cancel this event?", "Cancel Event"),
        };
        self.confirm_delete("Cancel Event?", body, button, move |this, _, cx| this.cancel_event(&guild_id, &event_id, cx), cx);
        self.set_dialog_cancel("Never Mind");
    }

    pub(super) fn cancel_event(&mut self, guild_id: &str, event_id: &str, cx: &mut Context<Self>) {
        let Some(event) = self.scheduled_event(guild_id, event_id).cloned() else { return };
        if self.events.details.as_ref().is_some_and(|d| d.event_id == event_id) {
            self.events.details = None;
        }
        let (g, e) = (guild_id.to_owned(), event_id.to_owned());
        let path = format!("/guilds/{guild_id}/scheduled-events/{event_id}");
        self.rest_optimistic(
            Method::DELETE,
            path,
            None,
            move |this| {
                if let Some(guild) = this.guilds.iter_mut().find(|x| x.id == g) {
                    guild.events.retain(|x| x.id != e);
                }
            },
            move |this| {
                if let Some(guild) = this.guilds.iter_mut().find(|x| x.id == event.guild_id) {
                    guild.events.push(event);
                }
            },
            cx,
        );
    }

    // Escape and outside clicks close the topmost layer only.
    pub(crate) fn close_events(&mut self) -> bool {
        let layers = &mut self.events;
        layers.start.take().is_some() || layers.wizard.take().is_some() || layers.details.take().is_some() || layers.list.take().is_some()
    }

    pub(super) fn close_event_layer(&mut self, cx: &mut Context<Self>) {
        self.close_events();
        cx.notify(); // lint-allow(global-notify): the events dialogs are drawn by the Overlays view, which re-renders only with the app
    }

    // Every layer closes, for a jump elsewhere; your occurrence answers stay.
    pub(super) fn leave_events(&mut self) {
        let rsvps = std::mem::take(&mut self.events.rsvps);
        self.events = Events { rsvps, ..Events::default() };
    }

    // The empty list's "Server Settings > Roles".
    pub(super) fn open_roles_settings(&mut self, guild_id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_events();
        self.open_server_settings(guild_id, window, cx);
        self.open_server_page(crate::server_settings::access::Page::Roles, cx);
    }
}
