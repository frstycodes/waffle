// What a game looks like around it: its card on the stage, the bar over it, the corner window and pop-out away from
// its place, over a chat, as a line in the channel list, and as a live card in chats.
use super::*;

pub(super) const FACES_SHOWN: usize = 4;
// The floating window an activity keeps playing in away from the call: a 16:9 picture under its bar.
pub(super) const PIP_WIDTH: f32 = 400.;
pub(super) const PIP_BAR: f32 = 40.;
// The bar over a focused activity: the app, who's in it, and its actions.
pub(super) const STAGE_BAR: f32 = 48.;
pub(super) const POPOUT_WIDTH: f32 = 960.;
// A game over a chat starts at this share of it, top down, and the split stays within these.
pub(super) const CHAT_SPLIT: f32 = 0.6;
pub(super) const SPLIT_MIN: f32 = 0.25;
pub(super) const SPLIT_MAX: f32 = 0.85;
pub(super) const SPLIT_HANDLE: f32 = 10.;
// The channel list's activity line matches the voice rows under it: their height, and indented past the channel glyph.
pub(super) const LIST_ROW: f32 = 30.;
pub(super) const LIST_INDENT: f32 = 32.;
pub(super) const LIST_FACES: usize = 3;
// A popped-out game's window: the game, edge to edge.
pub(super) struct ActivityPopout {
    frame: Entity<ActivityFrame>,
}

impl gpui_kit::Render for ActivityPopout {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(gpui_kit::black()).child(self.frame.clone())
    }
}

// A round button on the stage (Full screen in the activity's bar).
pub(super) const STAGE_ROUND: f32 = 28.;

// "Mira, Theo and 3 others", for who's playing.
pub(super) fn names_line(users: &[User]) -> String {
    let names: Vec<&str> = users.iter().map(|u| u.name.as_str()).collect();
    match names.as_slice() {
        [] => String::new(),
        [one] => format!("{one} is"),
        [a, b] => format!("{a} and {b} are"),
        [a, b, rest @ ..] => format!("{a}, {b} and {} other{} are", rest.len(), if rest.len() == 1 { "" } else { "s" }),
    }
}

// "Mira and Theo are playing", for a card or the launcher.
pub(crate) fn who_is_playing(players: &[User]) -> String {
    match players.is_empty() {
        true => "Nobody's playing yet".into(),
        false => format!("{} playing", names_line(players)),
    }
}

// "Mira and Theo", for a sentence.
pub(super) fn plain_names(users: &[User]) -> String {
    let names: Vec<&str> = users.iter().map(|u| u.name.as_str()).collect();
    match names.as_slice() {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

// The bar's line: who's playing with you.
pub(super) fn players_line(players: &[User], me: Option<&str>) -> String {
    let others: Vec<User> = players.iter().filter(|u| Some(u.id.as_str()) != me).cloned().collect();
    match others.is_empty() {
        true => "Just you so far".into(),
        false => format!("{} playing with you", names_line(&others)),
    }
}

// What an activity's card shows when the game itself isn't in it.
pub(super) enum CardState {
    // Running, and we're not in it: who's playing, and Join.
    Running,
    // We asked for it; `slow` once it's taken STILL_STARTING.
    Starting { slow: bool },
    // It didn't come up, and why.
    Failed(String),
    // We're in it, but its app never connected.
    DidntLoad,
    // It closed itself, with its words.
    Closed(String),
}

// Discord's tile sizes: past these widths the faces, line and name step up (56 / 32 / 24 faces).
pub(super) const TILE_WIDE: f32 = 400.;
// The voice panel's row for the game you're in.
pub(super) const PANEL_ROW: f32 = 44.;
// The voice panel's gap between its rows (its gap_2).
pub(super) const PANEL_ROW_GAP: f32 = 8.;
// A focused game's shape: Discord letterboxes activities at 16:9.
pub(super) const GAME_ASPECT: f32 = 16. / 9.;
pub(super) const TILE_MEDIUM: f32 = 300.;
pub(super) const FACE_WIDE: f32 = 56.;
// How dark, and how soft, the game's art sits behind the tile's words.
pub(super) const ART_DIM: f32 = 0.6;
pub(super) const ART_BLUR: f32 = 12.;

// "MERIBASSAI is playing", or "MERIBASSAI and 2 others are playing", as Discord's tile says it.
pub(super) fn playing_line(others: &[User]) -> String {
    match others {
        [] => String::new(),
        [one] => format!("{} is playing", one.name),
        [first, rest @ ..] => format!("{} and {} other{} are playing", first.name, rest.len(), if rest.len() == 1 { "" } else { "s" }),
    }
}

// Under this width a card drops the app's icon, the line under its name and the faces, keeping its name and buttons.
pub(super) const SMALL_CARD: f32 = 200.;


impl Waffle {
    // The focused activity's own bar, kept apart from the call's Disconnect: the app and who's in it, then
    // Invite, Full screen and a labelled Leave.
    pub(super) fn activity_bar(&self, app_id: &str, cx: &mut Context<Self>) -> AnyElement {
        let app = self.activities.apps.get(app_id);
        let name = self.app_name(app_id);
        let players = self.players(app_id);
        let faces: Vec<ui::Entry> = players.iter().map(|u| ui::Entry::person(u.avatar.clone(), u.name.clone())).collect();
        let place = self.game_place();
        let invite = place.as_ref().filter(|p| p.voice).and_then(|p| Some((p.guild_id.clone()?, p.channel.id.clone()))).filter(|_| place.as_ref().is_some_and(|p| self.place_permissions(p) & discord::perm::CREATE_INSTANT_INVITE != 0));
        let on_stage = place.as_ref().is_some_and(|p| p.voice);
        let fullscreen = self.call_fullscreen;
        ui::row()
            .h(px(STAGE_BAR))
            .flex_none()
            .px_3()
            .gap_2()
            // The stage's own colour: the bar belongs to the call around the game, not a band of its own.
            .bg(palette::stage::bg())
            .text_color(palette::stage::text())
            .child(ui::server_tile(app.and_then(|a| a.icon.as_deref()), &name, AVATAR_MD, cx))
            .child(
                ui::row()
                    .flex_1()
                    .gap_2()
                    .child(ui::line(name).flex_none().max_w(relative(0.5)).text_size(TEXT_SM).font_weight(gpui_kit::FontWeight::SEMIBOLD).text_color(palette::stage::strong()))
                    .child(ui::capped_stack(&faces, FACES_SHOWN, AVATAR_SM, palette::stage::bg(), palette::stage::wash(0.2), palette::stage::strong()).flex_none())
                    .child(ui::line(players_line(&players, self.me.as_ref().map(|m| m.id.as_str()))).flex_1().text_size(TEXT_XS).text_color(palette::stage::muted())),
            )
            .when_some(invite, |d, (guild, channel)| {
                d.child(ui::round_button("activity-bar-invite", Glyph::GroupPlus, "Invite", STAGE_ROUND).flex_none().on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.show_invite(guild.clone(), channel.clone(), window, cx);
                })))
            })
            .when(on_stage, |d| d.child(ui::round_button("activity-bar-fullscreen", if fullscreen { Glyph::FullscreenExit } else { Glyph::Fullscreen }, if fullscreen { "Exit full screen" } else { "Full screen" }, STAGE_ROUND).flex_none().on_click(cx.listener(
                move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.set_call_fullscreen(!fullscreen, window, cx);
                },
            ))))
            .when(cfg!(target_os = "macos"), |d| {
                d.child(ui::round_button("activity-bar-popout", Glyph::Open, "Pop out", STAGE_ROUND).flex_none().on_click(cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.pop_out(cx);
                })))
            })
            .child(ui::button("activity-bar-leave", Tone::Danger, cx).small().flex_none().child("Leave activity").on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.leave_with_rejoin(cx);
            })))
            .into_any_element()
    }

    // The running game's icon for the call bar's Activities control: a small tile in its corner.
    pub(crate) fn running_app_icon(&self, cx: &gpui_kit::App) -> Option<AnyElement> {
        let app_id = self.activities.frame.as_ref()?.read(cx).app_id.clone();
        let app = self.activities.apps.get(&app_id);
        let tile = ui::server_tile(app.and_then(|a| a.icon.as_deref()), &self.app_name(&app_id), AVATAR_XS, cx);
        Some(div().absolute().right_0p5().bottom_0p5().child(tile).into_any_element())
    }

    // Back from its window into its slot.
    pub(super) fn pop_in(&mut self, cx: &mut Context<Self>) {
        if self.activities.popout.take().is_none() {
            return;
        }
        if let Some(frame) = self.activities.frame.clone() {
            let main = self.main_window;
            cx.defer(move |cx| _ = main.update(cx, |_, window, cx| frame.update(cx, |frame, cx| frame.move_to(window, cx))));
        }
        cx.notify(); // lint-allow(global-notify): the stage's slot and the root's visibility both change
    }

    // The slot a popped-out game leaves: where it went, and the way back.
    pub(super) fn popped_out_slot(&self, app_id: &str, cx: &mut Context<Self>) -> AnyElement {
        let back = ui::button("activity-pop-in", Tone::Primary, cx).small().flex_none().child("Bring it back").on_click(cx.listener(|this, _, _, cx| {
            if let Some(popout) = this.activities.popout {
                cx.defer(move |cx| _ = popout.update(cx, |_, window, _| window.remove_window()));
            }
            this.pop_in(cx);
        }));
        ui::col()
            .size_full()
            .gap_2()
            .items_center()
            .justify_center()
            .child(ui::line(self.app_name(app_id)).text_size(TEXT_SM).font_weight(gpui_kit::FontWeight::SEMIBOLD).text_color(palette::stage::strong()))
            .child(div().text_size(TEXT_XS).text_color(palette::stage::muted()).child("In a pop-out"))
            .child(back)
            .into_any_element()
    }

    // A game in this chat plays over its top, the conversation kept under it; the line between them drags.
    pub(crate) fn chat_with_activity(&mut self, chat: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        let frame = self.activities.frame.clone().filter(|_| self.activities.popout.is_none());
        let (Some(frame), Some(place)) = (frame, self.game_place().filter(|p| !p.voice)) else { return chat };
        if !self.game_on_screen(&place) {
            return chat;
        }
        let app_id = frame.read(cx).app_id.clone();
        let split = self.activities.chat_split.unwrap_or(CHAT_SPLIT);
        let bounds = self.activities.split_bounds.clone();
        let handle = div()
            .id("activity-split")
            .h(px(SPLIT_HANDLE))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .cursor_row_resize()
            .bg(palette::surface(palette::Level::Raised, cx))
            .child(div().w_10().h_1().rounded_full().bg(palette::line_strong(cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.activities.splitting = true;
                    this.redraw(Column::Main, cx);
                }),
            );
        let game = if self.activities.unloaded.as_deref() == Some(app_id.as_str()) {
            self.activity_tile(&app_id, None, cx)
        } else {
            div().size_full().bg(gpui_kit::black()).child(frame).into_any_element()
        };
        ui::col()
            .id("activity-over-chat")
            .relative()
            .size_full()
            .child(gpui_kit::canvas(move |b, _, _| bounds.set(Some(b)), |_, _, _, _| ()).absolute().inset_0())
            .child(ui::col().h(relative(split)).flex_none().child(self.activity_bar(&app_id, cx)).child(div().flex_1().min_h_0().child(game)))
            .child(handle)
            .child(div().flex_1().min_h_0().flex().child(chat))
            // AppKit keeps sending a drag to the view it began in, so the split follows over the game too.
            .on_mouse_move(cx.listener(|this, e: &gpui_kit::MouseMoveEvent, _, cx| {
                let Some(b) = this.activities.split_bounds.get().filter(|_| this.activities.splitting && e.pressed_button == Some(MouseButton::Left)) else { return };
                let at = f32::from(e.position.y - b.origin.y) / f32::from(b.size.height).max(1.);
                this.activities.chat_split = Some(at.clamp(SPLIT_MIN, SPLIT_MAX));
                this.redraw(Column::Main, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.activities.splitting = false;
                    this.redraw(Column::Main, cx);
                }),
            )
            .into_any_element()
    }

    // Away from the call, the activity keeps playing in a floating window: carried by its bar (the game takes
    // every pointer event over its picture), with the way back to the call and out of the activity.
    pub(crate) fn activity_pip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let frame = self.activities.frame.clone().filter(|_| self.activities.popout.is_none())?;
        let place = self.game_place()?;
        if self.game_on_screen(&place) {
            return None;
        }
        let app_id = frame.read(cx).app_id.clone();
        let app = self.activities.apps.get(&app_id);
        let name = self.app_name(&app_id);
        let bar = ui::row()
            .h(px(PIP_BAR))
            .flex_none()
            .pl_2p5()
            .pr_1()
            .gap_2()
            .child(ui::server_tile(app.and_then(|a| a.icon.as_deref()), &name, AVATAR_SM, cx))
            .child(ui::line(name).flex_1().text_size(TEXT_SM).font_weight(gpui_kit::FontWeight::MEDIUM).text_color(palette::strong(cx)))
            .child(ui::icon_button("activity-pip-back", Glyph::Fullscreen, if place.voice { "Back to call" } else { "Back to chat" }, cx).small().on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                match place.voice {
                    true => this.show_call(place.channel.clone(), window, cx),
                    false => this.open_channel(place.channel.clone(), window, cx),
                }
            })))
            .child(ui::icon_button("activity-pip-leave", Glyph::PhoneDisconnect, "Leave activity", cx).small().on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.leave_with_rejoin(cx);
            })));
        let size = (PIP_WIDTH, PIP_WIDTH * 9. / 16. + PIP_BAR);
        let player = self
            .floating_drag(div().id("activity-pip"), cx)
            .occlude()
            .absolute()
            .w(px(size.0))
            .h(px(size.1))
            .flex()
            .flex_col()
            .squircle(RADIUS_MD)
            .overflow_hidden()
            .bg(palette::surface(palette::Level::Raised, cx))
            .shadow(ui::shadow_pop(cx))
            .child(bar)
            .child(div().flex_1().min_h_0().bg(gpui_kit::black()).child(frame));
        Some(self.place_floating(player, size))
    }

    // An activity's card on the stage: the frame itself when we're in it, else what's running there, who's
    // playing, and Join.
    pub(crate) fn activity_tile(&self, app_id: &str, card: Option<f32>, cx: &mut Context<Self>) -> AnyElement {
        let id = app_id.to_owned();
        let base = div().id(SharedString::from(activity_key(app_id))).relative().overflow_hidden().map(|d| match card {
            Some(w) => d.w(px(w)).h(px(w * 9. / 16.)).flex_none().squircle(RADIUS_LG),
            None => d.size_full(),
        });
        let unloaded = self.activities.unloaded.as_deref() == Some(app_id);
        let on_stage = self.game_place().is_some_and(|p| p.voice);
        let popped = self.activities.popout.is_some() && self.in_activity(app_id, cx);
        if popped {
            return base.bg(palette::stage::dock()).text_color(palette::stage::text()).child(self.popped_out_slot(app_id, cx)).into_any_element();
        }
        if let Some(frame) = self.activities.frame.as_ref().filter(|f| f.read(cx).app_id == app_id && !unloaded && on_stage) {
            let frame = frame.clone();
            return match card {
                Some(_) => base.bg(gpui_kit::black()).child(frame).into_any_element(),
                // Focused, as Discord's: the game at 16:9 in the middle of the stage's black, nothing over it; the
                // call's header names it and the call bar's Activities control holds Leave and Pop out.
                None => base.bg(gpui_kit::black()).child(ui::sized(move |size, _, _| {
                    let (w, h) = (f32::from(size.width), f32::from(size.height));
                    let (w, h) = (w.min(h * GAME_ASPECT), h.min(w / GAME_ASPECT));
                    div().size_full().flex().items_center().justify_center().child(div().w(px(w)).h(px(h)).flex_none().child(frame)).into_any_element()
                })).into_any_element(),
            };
        }
        let app = self.activities.apps.get(app_id);
        let name = app.map(|a| a.name.clone()).unwrap_or_else(|| "Activity".into());
        let held = |held: &Option<(String, String)>| held.as_ref().filter(|(id, _)| id == app_id).map(|(_, text)| text.clone());
        let state = match &self.activities.pending {
            _ if unloaded => CardState::DidntLoad,
            Some((pending, since)) if pending == app_id => CardState::Starting { slow: since.elapsed() >= STILL_STARTING },
            _ => match (held(&self.activities.failed), held(&self.activities.closed)) {
                (Some(why), _) => CardState::Failed(why),
                (_, Some(words)) => CardState::Closed(words),
                _ => CardState::Running,
            },
        };
        // Discord's tile: the game's art, dimmed and blurred, under who's playing, the game's name and one white
        // button. Its three sizes follow the tile's width, as Discord's do (wide over 400, medium over 300).
        let width = card.unwrap_or(f32::INFINITY);
        let (face, title, big) = match width {
            w if w > TILE_WIDE => (FACE_WIDE, TEXT_XL, true),
            w if w > TILE_MEDIUM => (AVATAR_LG, TEXT_LG, false),
            _ => (AVATAR_MD, TEXT_MD, false),
        };
        let small = width < SMALL_CARD;
        let me = self.me.as_ref().map(|m| m.id.clone());
        let others: Vec<User> = self.call.as_ref().map(|c| self.players_in(&c.channel.id, app_id)).unwrap_or_default().into_iter().filter(|u| Some(&u.id) != me.as_ref()).collect();
        let faces: Vec<ui::Entry> = others.iter().map(|u| ui::Entry::person(u.avatar.clone(), u.name.clone())).collect();
        let lead = match &state {
            CardState::Starting { .. } => Some(mascot::loader(SharedString::from(format!("activity-loading-{app_id}")), face.max(AVATAR_XL), palette::logo(), cx)),
            CardState::Failed(_) | CardState::DidntLoad => Some(Icon::new(Glyph::Warning).size(ICON_XL).text_color(palette::stage::strong()).into_any_element()),
            CardState::Running | CardState::Closed(_) if !faces.is_empty() => Some(ui::capped_stack(&faces, FACES_SHOWN, face, gpui_kit::black(), palette::stage::wash(0.2), palette::stage::strong()).into_any_element()),
            CardState::Running | CardState::Closed(_) => Some(ui::server_tile(app.and_then(|a| a.icon.as_deref()), &name, face, cx)),
        };
        let line = match &state {
            CardState::Running if others.is_empty() => None,
            CardState::Running => Some(playing_line(&others)),
            CardState::Starting { slow: false } => Some("Starting…".to_owned()),
            CardState::Starting { slow: true } => Some("Still starting. It's taking longer than usual.".to_owned()),
            CardState::Failed(why) => Some(format!("Couldn't start it: {why}")),
            CardState::DidntLoad => Some("It didn't load. Reloading keeps your place in the game.".to_owned()),
            CardState::Closed(words) if words.is_empty() => Some("The game closed.".to_owned()),
            CardState::Closed(words) => Some(format!("The game closed: “{words}”")),
        };
        let button = |label: &'static str, tone: Tone, cx: &mut Context<Self>| {
            ui::button(SharedString::from(format!("activity-{label}-{app_id}")), tone, cx).when(!big, |b| b.small()).flex_none().child(label)
        };
        let actions = match &state {
            CardState::Running => vec![button("Join Activity", Tone::Overlay, cx).on_click(cx.listener(move |this, _, _, cx| this.join_activity(id.clone(), cx)))],
            CardState::Starting { slow: false } => Vec::new(),
            CardState::Starting { slow: true } => vec![button("Cancel", Tone::OnMedia, cx).on_click(cx.listener(|this, _, _, cx| this.cancel_start(cx)))],
            CardState::Failed(_) => vec![
                button("Dismiss", Tone::OnMedia, cx).on_click(cx.listener(|this, _, _, cx| this.cancel_start(cx))),
                button("Try again", Tone::Overlay, cx).on_click(cx.listener(move |this, _, _, cx| this.join_activity(id.clone(), cx))),
            ],
            CardState::DidntLoad => vec![
                button("Leave", Tone::OnMedia, cx).on_click(cx.listener(|this, _, _, cx| {
                    this.activities.unloaded = None;
                    this.leave_activity(cx);
                })),
                button("Reload", Tone::Overlay, cx).on_click(cx.listener(|this, _, _, cx| this.reload_activity(cx))),
            ],
            CardState::Closed(_) => vec![
                button("Dismiss", Tone::OnMedia, cx).on_click(cx.listener(|this, _, _, cx| this.cancel_start(cx))),
                button("Play again", Tone::Overlay, cx).on_click(cx.listener(move |this, _, _, cx| this.join_activity(id.clone(), cx))),
            ],
        };
        let art = app.and_then(|a| a.background.clone()).map(|url| gpui_kit::img(url).absolute().inset_0().size_full().object_fit(gpui_kit::ObjectFit::Cover));
        base.bg(gpui_kit::black())
            .text_color(palette::stage::strong())
            .children(art)
            // Discord dims the art to 60% black; a blur keeps it a backdrop, not a picture to read.
            .child(div().absolute().inset_0().bg(gpui_kit::black().opacity(ART_DIM)).backdrop_blur(px(ART_BLUR)))
            .child(
                ui::col()
                    .relative()
                    .size_full()
                    .p_3()
                    .gap_1p5()
                    .items_center()
                    .justify_center()
                    .text_center()
                    .when_some(lead.filter(|_| !small), |d, lead| d.child(div().mb_1().child(lead)))
                    .when_some(line.filter(|_| !small), |d, line| d.child(div().max_w_full().text_size(TEXT_SM).line_height(relative(LEADING_BODY)).text_color(palette::stage::text()).child(line)))
                    .child(ui::line(name).max_w_full().text_size(if small { TEXT_SM } else { title }).font_weight(gpui_kit::FontWeight::BOLD))
                    .when(!actions.is_empty(), |d| d.child(ui::row().flex_none().mt_1p5().gap_2().children(actions))),
            )
            .into_any_element()
    }

    // The channel list's line for each activity running in a voice channel, above the people in it: the app, and
    // who's playing. In that call it joins the activity; from outside, it opens the call, where Join waits.
    pub(crate) fn activity_rows(&self, channel: &discord::Channel, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let in_call = self.call.as_ref().is_some_and(|c| c.channel.id == channel.id);
        let sidebar = crate::material::sidebar_fill(cx);
        self.activities
            .in_channel(&channel.id)
            .map(|instance| {
                let app_id = instance.application_id.clone();
                let app = self.activities.apps.get(&app_id);
                let name = self.app_name(&app_id);
                let faces: Vec<ui::Entry> = instance.participants.iter().map(|p| self.player(&p.user_id)).map(|u| ui::Entry::person(u.avatar, u.name)).collect();
                let playing = self.in_activity(&app_id, cx);
                let target = channel.clone();
                ui::row()
                    .id(SharedString::from(format!("list-{}-{}", activity_key(&app_id), channel.id)))
                    .h(px(LIST_ROW))
                    .flex_none()
                    .pl(px(LIST_INDENT))
                    .pr_2()
                    .gap_2()
                    .squircle(RADIUS_SM)
                    .cursor_pointer()
                    .text_color(if playing { palette::accent_ink(cx) } else { palette::text(cx) })
                    .hover(|s| s.bg(palette::hover(cx)))
                    .child(ui::server_tile(app.and_then(|a| a.icon.as_deref()), &name, AVATAR_SM, cx))
                    .child(ui::line(name).flex_1().text_size(TEXT_SM).font_weight(gpui_kit::FontWeight::MEDIUM))
                    .child(ui::capped_stack(&faces, LIST_FACES, AVATAR_XS, sidebar, palette::selected(cx), palette::text(cx)).flex_none())
                    .tooltip(move |window, cx| ui::tip(if playing { "Open activity" } else if in_call { "Join activity" } else { "Join the call and the activity" }, window, cx))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.join_activity_in(target.clone(), app_id.clone(), window, cx);
                    }))
                    .into_any_element()
            })
            .collect()
    }

    // The game in a window of its own, sized like a game, carrying its name; closing it brings the game back.
    #[cfg(target_os = "macos")]
    pub(crate) fn pop_out(&mut self, cx: &mut Context<Self>) {
        let Some(frame) = self.activities.frame.clone() else { return };
        if self.activities.popout.is_some() {
            return;
        }
        let title = self.app_name(&frame.read(cx).app_id);
        let this = cx.weak_entity();
        let options = gpui_kit::WindowOptions {
            window_bounds: Some(gpui_kit::WindowBounds::Windowed(gpui_kit::Bounds::centered(None, gpui_kit::size(px(POPOUT_WIDTH), px(POPOUT_WIDTH * 9. / 16.)), cx))),
            titlebar: Some(gpui_kit::TitlebarOptions { title: Some(title.into()), ..Default::default() }),
            focus: true,
            ..Default::default()
        };
        let opened = cx.open_window(options, |window, cx| {
            frame.update(cx, |frame, cx| frame.move_to(window, cx));
            window.on_window_should_close(cx, move |_, cx| {
                _ = this.update(cx, |this, cx| this.pop_in(cx));
                true
            });
            cx.new(|_| ActivityPopout { frame: frame.clone() })
        });
        if let Ok(handle) = opened {
            self.activities.popout = Some(handle.into());
            cx.notify(); // lint-allow(global-notify): the stage's slot and the root's visibility both change
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn pop_out(&mut self, _: &mut Context<Self>) {}

    pub(crate) fn player(&self, user_id: &str) -> User {
        self.participants().into_iter().find(|s| s.user_id == user_id).and_then(|s| s.user).unwrap_or_else(|| User {
            id: user_id.to_owned(),
            name: self.user_name(user_id),
            avatar: discord::avatar_url(user_id, None),
            ..Default::default()
        })
    }

    // Someone in an activity in a voice channel you can reach: where, and Join (the call and the game together).
    pub(crate) fn activity_join_button(&self, user_id: &str, rich: &discord::RichPresence, cx: &mut Context<Self>) -> Option<AnyElement> {
        let channel = self.activity_place(user_id, rich.app_id.as_deref()?)?;
        let app_id = rich.app_id.clone()?;
        let label = format!("Join in {}", channel.name);
        Some(ui::button("profile-activity-join", ui::Tone::Primary, cx).small().w_full().child(label).on_click(cx.listener(move |this, _, window, cx| this.join_activity_in(channel.clone(), app_id.clone(), window, cx))).into_any_element())
    }

    // The call header's title while a game is focused: the room, then the game, as Discord's header.
    pub(crate) fn activity_title(&self, channel: &str, cx: &gpui_kit::App) -> String {
        let focused = self.activities.frame.as_ref().map(|f| f.read(cx).app_id.clone()).filter(|id| self.call_focus.as_deref() == Some(activity_key(id).as_str()));
        match focused {
            Some(app_id) => format!("{channel} • {}", self.app_name(&app_id)),
            None => channel.to_owned(),
        }
    }

    // The composer's Activities button: the launcher for games that run over this chat.
    pub(crate) fn composer_activities_button(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        ui::icon_button("composer-activities", Glyph::Activities, "Activities", cx).toggled(self.activities.launcher.is_some(), cx).on_click(cx.listener(|this, _, window, cx| {
            let at = window.mouse_position();
            this.toggle_chat_launcher(at, window, cx);
        }))
    }

    // The voice panel's row for the game you're in, as Discord's: its icon and name, how long you've played, and
    // Leave Activity. Only for a game in this call; one over a chat has its own bar.
    pub(crate) fn activity_panel_row(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (app_id, since) = self.activities.running.clone().filter(|_| self.game_place().is_some_and(|p| p.voice))?;
        let app = self.activities.apps.get(&app_id);
        let name = self.app_name(&app_id);
        let elapsed = format!("{} elapsed", crate::call::clock(since.elapsed().as_secs()));
        Some(
            ui::row()
                .h(px(PANEL_ROW))
                .flex_none()
                .gap_2p5()
                .child(ui::server_tile(app.and_then(|a| a.icon.as_deref()), &name, AVATAR_XL, cx))
                .child(ui::col().flex_1().child(ui::line(name).font_weight(gpui_kit::FontWeight::SEMIBOLD).text_color(palette::strong(cx))).child(ui::line(elapsed).text_size(TEXT_XS).text_color(palette::muted(cx))))
                .child(ui::icon_button("panel-leave-activity", Glyph::DoorExit, "Leave Activity", cx).flex_none().on_click(cx.listener(|this, _, _, cx| this.leave_with_rejoin(cx))))
                .into_any_element(),
        )
    }

    // How much taller the voice panel stands while it carries that row.
    pub(crate) fn activity_panel_height(&self) -> f32 {
        let in_call = self.activities.running.is_some() && self.activities.place.as_ref().is_none_or(|p| p.voice);
        if in_call { PANEL_ROW + PANEL_ROW_GAP } else { 0. }
    }

    // The row's clock: once a second, the sidebar alone redraws.
    pub(super) fn tick_activity_row(&mut self, cx: &mut Context<Self>) {
        self.activities._ticker = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(std::time::Duration::from_secs(1)).await;
                if this.update(cx, |this, cx| this.redraw(Column::Nav, cx)).is_err() {
                    return;
                }
            }
        }));
    }

    // The call bar's red button, as Discord's: while you're in a game here it leaves the game; else the call.
    pub(crate) fn red_button_look(&self) -> (Glyph, &'static str) {
        match self.activity_panel_height() > 0. {
            true => (Glyph::DoorExit, "Leave Activity"),
            false => (Glyph::CallEnd, "Leave call"),
        }
    }

    pub(crate) fn press_red_button(&mut self, cx: &mut Context<Self>) {
        match self.activity_panel_height() > 0. {
            true => self.leave_with_rejoin(cx),
            false => self.leave_call_asking(cx),
        }
    }

    // Invite, in the call's header while a game runs there: who you invite joins the call, and the game from it.
    pub(crate) fn activity_invite_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let place = self.game_place().filter(|p| p.voice && self.activities.running.is_some())?;
        let guild = place.guild_id.clone().filter(|_| self.place_permissions(&place) & discord::perm::CREATE_INSTANT_INVITE != 0)?;
        let channel = place.channel.id.clone();
        Some(
            ui::round_button("activity-invite", Glyph::GroupPlus, "Invite to activity", STAGE_ROUND)
                .flex_none()
                .occlude()
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.show_invite(guild.clone(), channel.clone(), window, cx);
                }))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tile_names_who_is_playing() {
        let user = |name: &str| User { id: name.into(), name: name.into(), ..Default::default() };
        assert_eq!(playing_line(&[user("MERIBASSAI")]), "MERIBASSAI is playing");
        assert_eq!(playing_line(&[user("Mira"), user("Theo")]), "Mira and 1 other are playing");
        assert_eq!(playing_line(&[user("Mira"), user("Theo"), user("Jun")]), "Mira and 2 others are playing");
    }

    #[test]
    fn who_is_playing_reads_right() {
        let user = |name: &str| User { id: name.into(), name: name.into(), ..Default::default() };
        let (mira, theo, jun, ana) = (user("Mira"), user("Theo"), user("Jun"), user("Ana"));
        assert_eq!(who_is_playing(&[]), "Nobody's playing yet");
        assert_eq!(who_is_playing(std::slice::from_ref(&mira)), "Mira is playing");
        assert_eq!(who_is_playing(&[mira.clone(), theo.clone()]), "Mira and Theo are playing");
        assert_eq!(who_is_playing(&[mira.clone(), theo.clone(), jun.clone(), ana.clone()]), "Mira, Theo and 2 others are playing");
        assert_eq!(plain_names(&[mira.clone(), theo.clone(), jun.clone()]), "Mira, Theo and Jun");
        assert_eq!(players_line(std::slice::from_ref(&mira), Some("Mira")), "Just you so far");
        assert_eq!(players_line(&[mira, theo], Some("Mira")), "Theo is playing with you");
        assert_eq!(discord::launch_failure(13), "A moderator turned off Use Activities here.");
        assert_eq!(discord::launch_failure(99), "It didn't start. Try again in a moment.");
    }
}
