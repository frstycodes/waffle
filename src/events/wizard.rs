// The create and edit wizard: its state, its steps and the calls that save it.
use super::*;

// The wizard
impl Waffle {
    pub(crate) fn open_event_wizard(&mut self, guild_id: String, editing: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        let event = editing.as_deref().and_then(|id| self.scheduled_event(&guild_id, id)).cloned();
        let local = |iso: &str| parse_time(iso).map(|t| t.with_timezone(&Local));
        let start = event.as_ref().and_then(|e| local(&e.start)).unwrap_or_else(|| default_start(Local::now()));
        let end = event.as_ref().and_then(|e| e.end.as_deref()).and_then(local).unwrap_or(start + Duration::hours(1));
        let input = |value: String, placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            let state = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
            state.update(cx, |s, cx| s.set_value(value, window, cx));
            // lint-allow(detached-task, global-notify): the subscription ends when the input, held in the wizard, drops; the wizard is drawn by the Overlays view, which re-renders only with the app
            cx.subscribe(&state, |_, _, _: &InputEvent, cx| cx.notify()).detach();
            state
        };
        let location = input(event.as_ref().and_then(|e| e.location.clone()).unwrap_or_default(), "Add a location, link, or something.", window, cx);
        let topic = input(event.as_ref().map(|e| e.name.clone()).unwrap_or_default(), "What's your event?", window, cx);
        let (start_date, start_time) = (date_state(start, window, cx), time_state(start, window, cx));
        let (end_date, end_time) = (date_state(end, window, cx), time_state(end, window, cx));
        let description = cx.new(|cx| TextareaState::new(window, cx).placeholder("Tell people a little more about your event. Markdown, new lines, and links are supported."));
        description.update(cx, |s, cx| s.set_value(event.as_ref().and_then(|e| e.description.clone()).unwrap_or_default(), window, cx));
        // lint-allow(detached-task, global-notify): the subscription ends when the input, held in the wizard, drops; the wizard is drawn by the Overlays view, which re-renders only with the app
        cx.subscribe(&description, |_, _, _: &InputEvent, cx| cx.notify()).detach();
        let first_channel = self.event_guild(&guild_id).and_then(|g| event_channels(g, self.my_id(), ScheduledEvent::VOICE).first().map(|c| c.0.clone()));
        let entity_type = event.as_ref().map_or(ScheduledEvent::VOICE, |e| e.entity_type);
        self.events.wizard = Some(Wizard {
            guild_id,
            editing,
            started: event.as_ref().is_some_and(|e| e.status == ScheduledEvent::ACTIVE),
            step: Step::Location,
            entity_type,
            channel_id: event.as_ref().map_or(first_channel, |e| e.channel_id.clone()),
            frequency: event.as_ref().and_then(|e| e.recurrence.as_ref()).map_or(Frequency::Once, Frequency::of_rule),
            location,
            topic,
            start_date,
            start_time,
            end_date,
            end_time,
            description,
            cover: Cover::Kept,
            occurrence: None,
            link: None,
            busy: false,
        });
        cx.notify(); // lint-allow(global-notify): the events dialogs are drawn by the Overlays view, which re-renders only with the app
    }

    // "Edit this Event": the wizard on one occurrence's times, saved as its exception.
    // Not verified: Discord's single-occurrence modal (`462166`) wasn't fetched; the exception carries only times,
    // so only times are offered here.
    pub(super) fn open_occurrence_editor(&mut self, guild_id: String, event_id: String, recurrence_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, occurrence)) = self.find_occurrence(&guild_id, &event_id, &recurrence_id) else { return };
        self.open_event_wizard(guild_id, Some(event_id), window, cx);
        let Some(w) = self.events.wizard.as_mut() else { return };
        w.step = Step::Info;
        let (start, end) = (occurrence.start.with_timezone(&Local), occurrence.end.unwrap_or(occurrence.start + Duration::hours(1)).with_timezone(&Local));
        let pickers = (w.start_date.clone(), w.start_time.clone(), w.end_date.clone(), w.end_time.clone());
        w.occurrence = Some(occurrence);
        pickers.0.update(cx, |s, cx| s.set_date(start.date_naive(), window, cx));
        pickers.1.update(cx, |s, cx| s.set_time(start.time(), window, cx));
        pickers.2.update(cx, |s, cx| s.set_date(end.date_naive(), window, cx));
        pickers.3.update(cx, |s, cx| s.set_time(end.time(), window, cx));
    }

    // Pick a cover, held as a data URI until the save goes out.
    pub(super) fn pick_cover(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            // Read off the UI thread: a cover can be several megabytes.
            let read = cx.background_executor().spawn(async move {
                let bytes = std::fs::read(&path).ok()?;
                Some((crate::server_settings::api::image_data(crate::server_settings::view::mime_of(&path), &bytes), path))
            });
            let Some((data, path)) = read.await else { return };
            this.update(cx, |this, cx| this.set_wizard(cx, |w| w.cover = Cover::Picked { path, data })).ok();
        })
        .detach(); // lint-allow(detached-task): ends itself once its request or file prompt resolves, and holds only a weak handle
    }

    // Demo scenes jump straight to a step with a topic typed.
    #[cfg(debug_assertions)]
    pub(crate) fn fill_event_wizard(&mut self, topic: &str, entity_type: u8, location: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(wizard) = self.events.wizard.as_mut() else { return };
        wizard.step = Step::Info;
        wizard.entity_type = entity_type;
        wizard.frequency = Frequency::Weekly;
        let (topic_state, location_state) = (wizard.topic.clone(), wizard.location.clone());
        topic_state.update(cx, |s, cx| s.set_value(topic.to_owned(), window, cx));
        location_state.update(cx, |s, cx| s.set_value(location.to_owned(), window, cx));
    }

    pub(super) fn wizard_times(&self, cx: &Context<Self>) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
        let Some(w) = self.events.wizard.as_ref() else { return (None, None) };
        (picked_instant(&w.start_date, &w.start_time, cx), picked_instant(&w.end_date, &w.end_time, cx))
    }

    // Discord's validation (`688218`), in its words.
    pub(super) fn wizard_problems(&self, cx: &Context<Self>) -> Problems {
        let Some(w) = self.events.wizard.as_ref() else { return Problems::default() };
        let external = w.entity_type == ScheduledEvent::EXTERNAL;
        let location = w.location.read(cx).value().trim().to_owned();
        let topic = w.topic.read(cx).value().trim().to_owned();
        let (start, end) = self.wizard_times(cx);
        let now = Utc::now();
        let location = match external {
            true if location.is_empty() => Some("Either a location or channel must be specified."),
            true if location.chars().count() > LOCATION_MAX => Some("Locations can be at most 100 characters."),
            false if w.channel_id.is_none() => Some("Either a location or channel must be specified."),
            _ => None,
        };
        let topic = match topic.chars().count() {
            0 => Some("Topic must be specified."),
            n if n > TOPIC_MAX => Some("Topics can be at most 100 characters."),
            _ => None,
        };
        let horizon = if w.frequency == Frequency::Once { Duration::days(365) } else { Duration::days(365 * 4) };
        let start_problem = match start {
            None => Some("Events must have a start date and time."),
            Some(s) if w.editing.is_none() && s <= now => Some("The start time must be in the future."),
            Some(s) if s > now + horizon => Some("The start time is too far in the future."),
            _ => None,
        };
        let end = match (external, start, end) {
            (false, _, _) => None,
            (true, _, None) => Some("End date and time are required."),
            (true, Some(s), Some(e)) if e <= s => Some("The end time must be after the start time."),
            (true, _, Some(e)) if e <= now => Some("The end time must be in the future."),
            _ => None,
        };
        let description = w.description.read(cx).value().chars().count() > DESCRIPTION_MAX;
        Problems { location, topic, start: start_problem, end, description }
    }

    pub(super) fn wizard_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let problems = self.wizard_problems(cx);
        let Some(w) = self.events.wizard.as_mut() else { return };
        let next = match (w.step, forward) {
            (Step::Location, true) => Step::Info,
            (Step::Info, true) => Step::Review,
            (Step::Info, false) => Step::Location,
            (Step::Review, false) => Step::Info,
            (step, _) => step,
        };
        if forward && !problems.step_ok(w.step) {
            return;
        }
        w.step = next;
        cx.notify(); // lint-allow(global-notify): the events dialogs are drawn by the Overlays view, which re-renders only with the app
    }

    pub(super) fn wizard_submit(&mut self, cx: &mut Context<Self>) {
        if self.events.wizard.as_ref().is_some_and(|w| w.occurrence.is_some()) {
            return self.send_occurrence(cx);
        }
        let editing_series = self.events.wizard.as_ref().and_then(|w| w.editing.as_deref().map(|id| (w.guild_id.clone(), id.to_owned())));
        let series = editing_series.and_then(|(g, id)| self.scheduled_event(&g, &id).cloned()).filter(|e| e.recurrence.is_some());
        let schedule_changed = series.is_some_and(|e| {
            let (start, end) = self.wizard_times(cx);
            start.map(|s| s.to_rfc3339()) != parse_time(&e.start).map(|s| s.to_rfc3339()) || end.map(|s| s.to_rfc3339()) != e.end.as_deref().and_then(parse_time).map(|s| s.to_rfc3339())
        });
        if !schedule_changed {
            return self.send_event(cx);
        }
        self.confirm("Edit all Events in this Series", "Are you sure you want to edit this event? Editing this event will also update all events in this series.", "Save Event", |this, _, cx| this.send_event(cx), cx);
        self.set_dialog_cancel("Never Mind");
    }

    pub(super) fn send_event(&mut self, cx: &mut Context<Self>) {
        if !self.wizard_problems(cx).step_ok(Step::Review) {
            return;
        }
        let (start, end) = self.wizard_times(cx);
        let Some(w) = self.events.wizard.as_ref() else { return };
        let Some(start) = start else { return };
        let external = w.entity_type == ScheduledEvent::EXTERNAL;
        let description = w.description.read(cx).value().trim().to_owned();
        let mut body = json!({
            "name": w.topic.read(cx).value().trim(),
            "description": description,
            "privacy_level": PRIVATE,
            "scheduled_start_time": start.to_rfc3339(),
            "scheduled_end_time": if external { end.map(|e| e.to_rfc3339()) } else { None },
            "entity_type": w.entity_type,
            "channel_id": if external { None } else { w.channel_id.clone() },
            "entity_metadata": if external { json!({ "location": w.location.read(cx).value().trim() }) } else { Value::Null },
            "recurrence_rule": w.frequency.rule(start, start.with_timezone(&Local).date_naive()),
        });
        if let Some(image) = cover_field(&w.cover) {
            body["image"] = image;
        }
        let (method, path) = match &w.editing {
            Some(id) => (Method::PATCH, format!("/guilds/{}/scheduled-events/{id}", w.guild_id)),
            None => (Method::POST, format!("/guilds/{}/scheduled-events", w.guild_id)),
        };
        let editing = w.editing.is_some();
        let Some(client) = self.client.clone() else { return };
        if let Some(w) = self.events.wizard.as_mut() {
            w.busy = true;
        }
        cx.notify(); // lint-allow(global-notify): the events dialogs are drawn by the Overlays view, which re-renders only with the app
        cx.spawn(async move |this, cx| {
            let result = client.call(method, path, Some(body)).await;
            this.update(cx, |this, cx| {
                if let Some(w) = this.events.wizard.as_mut() {
                    w.busy = false;
                }
                match result.map(|v| discord::parse_scheduled_event(&v)) {
                    Ok(Some(event)) => {
                        let link = event_link(&event);
                        this.on_event(discord::Event::ScheduledEvent(event), cx);
                        match (editing, this.events.wizard.as_mut()) {
                            (false, Some(w)) => {
                                w.step = Step::Success;
                                w.link = Some(link);
                            }
                            _ => this.events.wizard = None,
                        }
                    }
                    Ok(None) => this.events.wizard = None,
                    Err(error) => this.show_toast(error, cx),
                }
                cx.notify(); // lint-allow(global-notify): the events dialogs are drawn by the Overlays view, which re-renders only with the app
            })
            .ok();
        })
        .detach(); // lint-allow(detached-task): ends itself once its request or file prompt resolves, and holds only a weak handle
    }

    // One occurrence's new times, as its exception; the wizard closes once it's sent.
    pub(super) fn send_occurrence(&mut self, cx: &mut Context<Self>) {
        if !self.wizard_problems(cx).step_ok(Step::Review) {
            return;
        }
        let (start, end) = self.wizard_times(cx);
        let Some(w) = self.events.wizard.take() else { return };
        let (Some(start), Some(occurrence)) = (start, w.occurrence) else { return };
        let Some(event) = w.editing.and_then(|id| self.scheduled_event(&w.guild_id, &id).cloned()) else { return };
        let end = if event.entity_type == ScheduledEvent::EXTERNAL { end } else { occurrence.end.map(|e| start + (e - occurrence.start)) };
        let (method, path, body) = exception_request(&event, &occurrence, start, end, false);
        self.send_exception(&event, occurrence.recurrence_id.clone(), (method, path, Some(body)), cx);
    }

    pub(super) fn set_wizard(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Wizard)) {
        if let Some(w) = self.events.wizard.as_mut() {
            change(w);
        }
        cx.notify(); // lint-allow(global-notify): the events dialogs are drawn by the Overlays view, which re-renders only with the app
    }

    pub(super) fn open_frequency_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (start, _) = self.wizard_times(cx);
        let day = start.map_or_else(|| Local::now().date_naive(), |s| s.with_timezone(&Local).date_naive());
        let at = window.mouse_position();
        self.open_live_menu(
            at,
            move |this| {
                let chosen = this.events.wizard.as_ref().map(|w| w.frequency);
                Frequency::options(day)
                    .into_iter()
                    .map(|f| item(f.label(day), move |this, _, cx| this.set_wizard(cx, |w| w.frequency = f)).radio(chosen == Some(f)).entry())
                    .collect()
            },
            cx,
        );
    }

    pub(super) fn open_event_channel_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(w) = self.events.wizard.as_ref() else { return };
        let Some(guild) = self.event_guild(&w.guild_id) else { return };
        let (chosen, glyph) = (w.channel_id.clone(), if w.entity_type == ScheduledEvent::VOICE { Some(Glyph::VolumeHigh) } else { None });
        let entries: Vec<Entry> = event_channels(guild, self.my_id(), w.entity_type)
            .into_iter()
            .map(|(id, name)| {
                let on = chosen.as_ref() == Some(&id);
                let row = item(name, move |this, _, cx| this.set_wizard(cx, |w| w.channel_id = Some(id.clone()))).radio(on);
                match glyph {
                    Some(glyph) => row.icon(glyph).entry(),
                    None => row.entry(),
                }
            })
            .collect();
        self.open_menu(window.mouse_position(), entries, cx);
    }
}

// The channels an event of `entity_type` can be held in: voice or stage channels you can see and join.
pub(super) fn event_channels(guild: &Guild, me: &str, entity_type: u8) -> Vec<(String, String)> {
    let stage = entity_type == ScheduledEvent::STAGE;
    guild
        .channels
        .iter()
        .filter(|c| c.kind == discord::ChannelKind::Voice && is_stage(c) == stage)
        .filter(|c| guild.channel_permissions(me, c) & (discord::VIEW_CHANNEL | perm::CONNECT) == discord::VIEW_CHANNEL | perm::CONNECT)
        .map(|c| (c.id.clone(), c.name.clone()))
        .collect()
}

pub(super) fn is_stage(channel: &discord::Channel) -> bool {
    channel.raw_type == STAGE_TYPE
}
