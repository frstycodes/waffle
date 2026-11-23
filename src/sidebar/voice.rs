// People in a voice channel's rows, and the card a streamer's row opens.
use super::*;

// A stage lists only its speakers; everyone else folds into one row, as Discord's does.
pub(super) fn audience_row(count: usize, cx: &App) -> Div {
    ui::row()
        .h(px(VOICE_ROW))
        .flex_none()
        .pl(px(AUDIENCE_INDENT))
        .pr_2()
        .gap_2()
        .font_weight(FontWeight::NORMAL)
        .text_color(palette::muted(cx))
        .child(Icon::new(Glyph::Headphones).size(ui::ICON_MD).flex_none())
        .child(ui::line(format!("{count} in audience")).flex_1())
}

// A voice occupant's avatar; while they speak, a green ring sits 1px clear of it, as Discord's does, and
// while their soundboard sound plays it takes the accent, as their call tile does.
pub(super) fn speaking_avatar(avatar: &str, talking: bool, sounding: bool, cx: &App) -> Div {
    let ring = if sounding { Some(palette::accent()) } else { talking.then(|| palette::status_in(crate::discord::Status::Online, cx)) };
    // lint-allow(custom-mark): ui::speaking_ring cuts its gap in the surface colour; this one leaves the gap clear over the sidebar's material.
    let mark = ring.map(|color| div().absolute().top(px(-RING_CLEARANCE)).left(px(-RING_CLEARANCE)).size_7().rounded_full().border_2().border_color(color));
    div().relative().flex_none().size(px(VOICE_AVATAR)).child(ui::avatar(avatar.to_owned(), VOICE_AVATAR)).children(mark)
}

// What a voice occupant's row shows at its end, as Discord's: their mute and deafen (server ones red),
// then camera, then LIVE. Speaking shows as a ring on the avatar instead.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum VoiceMark {
    ServerMuted,
    ServerDeafened,
    Live,
    Camera,
    Muted,
    Deafened,
}

// Deafening yourself also mutes you, so a self-deafen shows both marks, as Discord's list does; a
// server deafen alone shows only its own, and a server mute and deafen together show both.
pub(super) fn voice_marks(state: &crate::discord::VoiceState) -> Vec<VoiceMark> {
    // The mic slot and the headphones slot fill on their own, so a server mute shows beside a server
    // deafen. `muted` and `deafened` include the server's flags, so only a self-deafen fills the mic slot.
    let mic = match (state.server_muted, state.muted || (state.deafened && !state.server_deafened)) {
        (true, _) => Some(VoiceMark::ServerMuted),
        (false, true) => Some(VoiceMark::Muted),
        _ => None,
    };
    let ears = match (state.server_deafened, state.deafened) {
        (true, _) => Some(VoiceMark::ServerDeafened),
        (false, true) => Some(VoiceMark::Deafened),
        _ => None,
    };
    let mut marks: Vec<VoiceMark> = mic.into_iter().chain(ears).collect();
    if state.camera {
        marks.push(VoiceMark::Camera);
    }
    if state.streaming {
        marks.push(VoiceMark::Live);
    }
    marks
}

pub(super) fn voice_mark_for(state: &crate::discord::VoiceState, cx: &App) -> Option<AnyElement> {
    let marks = voice_marks(state);
    if marks.is_empty() {
        return None;
    }
    let (faint, danger) = (palette::faint(cx), palette::danger_ink(cx));
    let mark = |icon: Icon, color: Hsla| ui::row().items_stretch().flex_none().text_color(color).child(icon.size(ui::ICON_MD)).into_any_element();
    let elements = marks.into_iter().map(|m| match m {
        VoiceMark::ServerMuted => mark(Nav::MicOffFill.icon(), danger),
        VoiceMark::ServerDeafened => mark(Nav::DeafenedFill.icon(), danger),
        VoiceMark::Live => ui::live_badge().h(px(LIVE_MARK_HEIGHT)).px_1p5().text_size(ui::TEXT_XXS).into_any_element(),
        VoiceMark::Camera => mark(Nav::CameraFill.icon(), faint),
        VoiceMark::Muted => mark(Nav::MicOffFill.icon(), faint),
        VoiceMark::Deafened => mark(Nav::DeafenedFill.icon(), faint),
    });
    Some(ui::row().flex_none().gap_1p5().children(elements).into_any_element())
}

pub(super) const STREAM_CARD_WIDTH: f32 = 260.;

pub(super) struct Streamer<'a> {
    pub(super) id: &'a str,
    pub(super) name: &'a str,
    pub(super) avatar: &'a str,
}

// The row and its card share one hover key, so moving from one onto the other keeps the card open.
pub(super) fn stream_hover_key(user_id: &str) -> String {
    format!("stream-{user_id}")
}

// "Theo watching", "Theo and Ivy watching", "Theo, Ivy and Sam watching", "Theo, Ivy and 2 others watching".
pub(super) fn viewer_line(names: &[String]) -> Option<String> {
    let who = match names {
        [] => return None,
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [a, b, c] => format!("{a}, {b} and {c}"),
        [a, b, rest @ ..] => format!("{a}, {b} and {} others", rest.len()),
    };
    Some(format!("{who} watching"))
}

// The shared app and how long it's been live, "Blender · 24 min"; either half may be missing.
pub(super) fn stream_meta(app: Option<&str>, live: Option<Duration>) -> Option<String> {
    let parts: Vec<String> = app.map(str::to_owned).into_iter().chain(live.map(live_for)).collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

pub(super) fn live_for(live: Duration) -> String {
    let minutes = live.as_secs() / 60;
    match (minutes / 60, minutes % 60) {
        (0, 0) => "Just started".into(),
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} hr"),
        (h, m) => format!("{h} hr {m} min"),
    }
}

// Unlike Discord, joining leaves the view where it is; the stage opens from a second click.
#[derive(Debug, PartialEq)]
pub(super) enum VoiceRowClick {
    Join,
    OpenStage,
}

pub(super) fn voice_row_click(in_call: Option<&str>, clicked: &str) -> VoiceRowClick {
    match in_call == Some(clicked) {
        true => VoiceRowClick::OpenStage,
        false => VoiceRowClick::Join,
    }
}

impl Waffle {
    // .preview, the hover card on someone live: the stream's thumbnail under LIVE, with a way in
    // over it on hover, then what they share and how many watch. It grows toward its row from 96%
    // with a fade, passing 100% before it settles.
    pub(super) fn stream_card(&self, streamer: Streamer, channel: &Channel, stream_key: &str, connected: bool, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Only once the bytes are in, so the fade below shows the picture rather than an empty box.
        let preview = self.stream_preview(streamer.id).filter(|url| matches!(window.use_asset::<gpui_kit::ImgResourceLoader>(&gpui_kit::Resource::Uri(url.clone().into()), cx), Some(Ok(_))));
        let app = self.presences.get(streamer.id).and_then(|p| p.activity.as_ref()).filter(|a| matches!(a.kind, ActivityKind::Playing | ActivityKind::Streaming)).map(|a| a.text.as_str());
        let viewer_ids: Vec<&String> = self.stream_viewers.get(stream_key).into_iter().flatten().filter(|id| *id != streamer.id).collect();
        let viewer_names: Vec<String> = viewer_ids.iter().map(|id| self.user_name(id)).collect();
        let watching = (!viewer_ids.is_empty()).then(|| format!("{} watching", viewer_ids.len()));
        let live = stream_meta(None, self.stream_live_since.get(stream_key).map(|at| at.elapsed()));
        let detail: Vec<String> = app.map(|_| streamer.name.to_owned()).into_iter().chain(watching).chain(live).collect();
        let who: Option<SharedString> = viewer_line(&viewer_names).map(Into::into);
        let hover_key = stream_hover_key(streamer.id);
        let (watch_id, watch_channel) = (streamer.id.to_owned(), channel.clone());
        let thumb_group = SharedString::from(format!("stream-thumb-{}", streamer.id));
        let thumb = div()
            .id(SharedString::from(format!("stream-card-thumb-{}", streamer.id)))
            .group(thumb_group.clone())
            .relative()
            .w_full()
            .aspect_ratio(16. / 9.)
            .cursor_pointer()
            .child(crate::call::stream_fill(ui::RADIUS_MD).absolute().inset_0())
            .children(preview.map(|url| {
                ui::media(url, ui::all(ui::RADIUS_MD))
                    .absolute()
                    .size_full()
                    .object_fit(gpui_kit::ObjectFit::Cover)
                    .with_animation(SharedString::from(format!("stream-preview-{}", streamer.id)), Animation::new(ui::DUR_POP), |el, t| el.opacity(t))
            }))
            .child(ui::live_badge().absolute().top_2().left_2())
            .child(
                // lint-allow(custom-mark): ui::media_label is a label over media; this is a hover scrim over the thumbnail holding the watch button.
                ui::row()
                    .absolute()
                    .inset_0()
                    .squircle(ui::RADIUS_MD)
                    .bg(gpui_kit::black().opacity(0.45))
                    .justify_center()
                    .opacity(0.)
                    .group_hover(thumb_group, |s| s.opacity(1.))
                    .child(crate::call::watch_button(if connected { "Watch stream" } else { "Join and watch" })),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.stream_card_hover = None;
                match connected {
                    true => this.watch_stream(watch_id.clone(), cx),
                    // Not in that call yet: join it, then watch once connected.
                    false => this.watch_on_connect = Some(watch_id.clone()),
                }
                this.show_call(watch_channel.clone(), window, cx);
            }));
        let cap = ui::row()
            .id(SharedString::from(format!("stream-card-cap-{}", streamer.id)))
            .px_1p5()
            .pt_0p5()
            .pb_1()
            .gap_2()
            .child(ui::avatar(streamer.avatar.to_owned(), 24.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(ui::line(app.unwrap_or(streamer.name).to_owned()).text_size(ui::TEXT_SM).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)))
                    .child(ui::line(detail.join(" · ")).text_size(ui::TEXT_XS).text_color(palette::faint(cx))),
            )
            .when_some(who, |d, who| d.tooltip(move |window, cx| ui::tip(who.clone(), window, cx)));
        let record = self.stream_card_bounds.clone();
        ui::surface(cx)
            .id(SharedString::from(format!("stream-card-{}", streamer.id)))
            // Blocks the rows under it; its own hover keeps it open once the pointer leaves the row.
            .occlude()
            .w(px(STREAM_CARD_WIDTH))
            .child(gpui_kit::canvas(move |bounds, _, _| record.set(bounds), |_, _, _, _| {}).absolute().inset_0())
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                this.stream_card_hover = hovered.then(|| hover_key.clone());
                this.redraw(crate::part::Column::Nav, cx);
            }))
            .gap_1p5()
            .text_size(ui::TEXT_SM)
            .child(thumb)
            .child(cap)
            .transform_origin(point(relative(0.), relative(0.)))
            .with_animation(SharedString::from(format!("stream-card-open-{}", streamer.id)), Animation::new(Duration::from_millis(180)).with_easing(motion::back_out), |card, t| {
                card.scale(0.96 + 0.04 * t).opacity((t / 0.6).clamp(0., 1.))
            })
    }

    // A voice channel takes people dropped anywhere on it, its row or the people under it. Each part
    // claims the target while the pointer is on it and lets go only of its own claim, so moving
    // between parts of one channel never drops it.
    pub(super) fn voice_drop_zone(&self, zone: Stateful<Div>, part: SharedString, channel: &Channel, cx: &mut Context<Self>) -> Stateful<Div> {
        let (target, name, drop_target) = (channel.id.clone(), channel.name.clone(), channel.clone());
        zone.on_drag_move::<crate::drag::VoiceDrag>(cx.listener(move |this, e: &gpui_kit::DragMoveEvent<crate::drag::VoiceDrag>, _, cx| {
            let claim = Some((target.clone(), part.clone()));
            let inside = e.bounds.contains(&e.event.position) && this.can_move_voice(&e.drag(cx).guild_id, &e.drag(cx).user_id, &target);
            match (inside, this.drag_motion.voice_target == claim) {
                (true, false) => {
                    let joining = this.me.as_ref().is_some_and(|me| me.id == e.drag(cx).user_id);
                    let home = this.guilds.iter().flat_map(|g| &g.voice_states).any(|v| v.user_id == e.drag(cx).user_id && v.channel_id.as_ref() == Some(&target));
                    this.drag_motion.voice_target = claim;
                    this.drag_motion.set_tag((!home).then(|| format!("{} {name}", if joining { "Join" } else { "Move to" }).into()));
                }
                (false, true) => {
                    this.drag_motion.voice_target = None;
                    this.drag_motion.set_tag(None);
                }
                _ => return,
            }
            this.redraw(crate::part::Column::Nav, cx);
        }))
        .on_drop(cx.listener(move |this, drag: &crate::drag::VoiceDrag, _, cx| this.drop_voice_user(drag, drop_target.clone(), cx)))
    }

    // Held over this voice channel by someone from elsewhere: it lights up and opens a slot.
    pub(super) fn welcomes_voice_drop(&self, channel: &Channel, guild: &Guild) -> bool {
        let targeted = self.drag_motion.voice_target.as_ref().is_some_and(|(id, _)| *id == channel.id);
        targeted && !guild.voice_states.iter().any(|v| v.channel_id.as_ref() == Some(&channel.id) && self.drag_motion.row(&format!("voice-{}", v.user_id)) == RowDrag::Hole)
    }

    // Who a carried voice channel takes along, as its ghost shows them.
    pub(super) fn voice_people(&self, channel: &Channel, guild: &Guild) -> Vec<(String, SharedString)> {
        guild
            .voice_states
            .iter()
            .filter(|v| v.channel_id.as_ref() == Some(&channel.id))
            .map(|v| {
                let avatar = v.user.as_ref().map(|u| u.avatar.clone()).unwrap_or_else(|| crate::discord::avatar_url(&v.user_id, None));
                let name = v.user.as_ref().map(|u| u.name.clone()).unwrap_or_else(|| self.user_name(&v.user_id));
                (avatar, name.into())
            })
            .collect()
    }

    // Leaving a streaming row toward its card holds the card open for the tunnel's grace.
    pub(super) fn hover_stream_row(&mut self, key: &str, hovered: bool, at: gpui_kit::Point<gpui_kit::Pixels>, cx: &mut Context<Self>) {
        if !hovered && self.stream_tunnel.heading_to(at, self.stream_card_bounds.get()) {
            self.stream_hold = Some(key.to_owned());
            let key = key.to_owned();
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(ui::TUNNEL_GRACE).await;
                this.update(cx, |this, cx| {
                    if this.stream_hold.as_deref() == Some(key.as_str()) {
                        this.stream_hold = None;
                        this.set_hovered(&key, false, cx);
                    }
                })
                .ok();
            })
            // lint-allow(detached-task): ends itself once the one grace timer fires, and holds only a WeakEntity.
            .detach();
            return;
        }
        self.stream_hold = None;
        self.set_hovered(key, hovered, cx);
    }

    // Who is in a voice channel: 30px rows under it, the mini wave for whoever speaks, then one mark.
    pub(super) fn voice_rows(&self, channel: &Channel, guild: &Guild, in_call: bool, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let speaking = self.call.as_ref().filter(|_| in_call).map(|c| c.speaking.clone()).unwrap_or_default();
        let watching: Vec<String> = self.call.as_ref().map(|c| c.watching.iter().map(|w| w.streamer_id.clone()).collect()).unwrap_or_default();
        let connected = self.call.as_ref().is_some_and(|c| c.connected && in_call);
        let can_drag_people = self.can_in(&channel.id, crate::discord::perm::MOVE_MEMBERS);
        let (mut present, audience): (Vec<&crate::discord::VoiceState>, Vec<_>) = guild
            .voice_states
            .iter()
            .filter(|v| v.channel_id.as_ref() == Some(&channel.id))
            .partition(|v| channel.raw_type != STAGE || v.on_stage() == OnStage::Speaker);
        // Whoever is streaming leads the list; the rest keep their order (a stable sort).
        present.sort_by_key(|v| !v.streaming);
        let avatar_of = |v: &crate::discord::VoiceState| v.user.as_ref().map(|u| u.avatar.clone()).unwrap_or_else(|| crate::discord::avatar_url(&v.user_id, None));
        if self.hidden_voice_names.contains(&channel.id) && !present.is_empty() {
            let sidebar = crate::material::sidebar_fill(cx);
            return vec![ui::row().h(px(VOICE_ROW)).pl_8().child(ui::avatar_stack(present.iter().map(|v| avatar_of(v)), VOICE_AVATAR, sidebar)).into_any_element()];
        }
        let (hover, selected, text) = (palette::hover(cx), palette::selected(cx), palette::text(cx));
        let mut rows = self.activity_rows(channel, cx);
        let me = self.me.as_ref().map(|m| m.id.clone());
        let elsewhere = |user_id: &str| me.as_deref() == Some(user_id) && !in_call;
        for state in present {
            let name = state.user.as_ref().map(|u| u.name.clone()).unwrap_or_else(|| self.user_name(&state.user_id));
            let avatar = avatar_of(state);
            let talking = speaking.contains(&state.user_id);
            let is_watched = watching.contains(&state.user_id);
            let hover_key = stream_hover_key(&state.user_id);
            // Open while the row or the card itself is hovered, so the card's button can be reached.
            let card_open = self.hovered.as_deref() == Some(hover_key.as_str()) || self.stream_card_hover.as_deref() == Some(hover_key.as_str());
            let stream_card = (state.streaming && card_open).then(|| {
                let stream_key = format!("guild:{}:{}:{}", guild.id, channel.id, state.user_id);
                self.stream_card(Streamer { id: &state.user_id, name: &name, avatar: &avatar }, channel, &stream_key, connected, window, cx).into_any_element()
            });
            let stream_hover = cx.listener(move |this, hovered: &bool, window, cx| this.hover_stream_row(&hover_key, *hovered, window.mouse_position(), cx));
            let end = voice_mark_for(state, cx);
            let game = self.activity_mark(&state.user_id, &channel.id).or_else(|| self.playing(&state.user_id, cx)).map(|game| {
                div()
                    .id(SharedString::from(format!("voice-game-{}", state.user_id)))
                    .flex_none()
                    .child(img(game.icon.clone()).size_4().squircle(4.))
                    .tooltip(move |window, cx| ui::tip(game.label(), window, cx))
            });
            let key = SharedString::from(format!("voice-{}", state.user_id));
            let carried = self.drag_motion.row(&key);
            let row = div().id(key.clone()).h(px(VOICE_ROW)).flex_none().squircle(ui::RADIUS_SM).children(self.drag_motion.spot(&key));
            let row = self.voice_drop_zone(row, key.clone(), channel, cx);
            if carried == RowDrag::Hole {
                rows.push(hole(row, cx).into_any_element());
                continue;
            }
            let row = match can_drag_people || self.me.as_ref().is_some_and(|m| m.id == state.user_id) {
                true => {
                    let drag = crate::drag::VoiceDrag { user_id: state.user_id.clone(), guild_id: guild.id.clone(), name: name.clone().into(), avatar: avatar.clone() };
                    let look = crate::drag::Look::Person { avatar: drag.avatar.clone(), name: drag.name.clone() };
                    row.on_drag(drag, crate::drag::pick_up(cx.weak_entity(), key, look, point(px(self.channels_width - 16.), px(VOICE_ROW)), 1.05, 0., false))
                }
                false => row,
            };
            rows.push(
                row.when(carried == RowDrag::Hidden, |r| r.invisible())
                    .pl_8()
                    .pr_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .font_weight(FontWeight::NORMAL)
                    .text_color(if talking { palette::strong(cx) } else { palette::muted(cx) })
                    .hover(move |s| s.bg(hover).text_color(text))
                    // You, in this call from another client (not this app): avatar and name faded, as Discord does.
                    .child(speaking_avatar(&avatar, talking, self.soundboard_playing(&state.user_id), cx).when(elsewhere(&state.user_id), |a| a.opacity(0.45)))
                    .child(ui::line(name.clone()).flex_1().when(elsewhere(&state.user_id), |n| n.opacity(0.45)))
                    .children(game.map(|g| g.when(elsewhere(&state.user_id), |g| g.opacity(0.45))))
                    .children(end)
                    .on_mouse_down(gpui_kit::MouseButton::Right, {
                        let user_id = state.user_id.clone();
                        cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.voice_user_menu(&user_id, e.position, cx))
                    })
                    .when(state.streaming, |r| r.on_hover(stream_hover).on_mouse_move(cx.listener(|this, e: &gpui_kit::MouseMoveEvent, _, _| this.stream_tunnel.track(e.position))).children(stream_card.map(|card| div().absolute().top_0().right_1().child(ui::floating("stream-card-float", card)))))
                    .when(is_watched, |r| r.bg(selected))
                    // A click opens their profile beside the channel list, as Discord's; a stream is watched from
                    // the preview its row shows on hover.
                    .cursor_pointer()
                    .on_click({
                        let (user_id, x) = (state.user_id.clone(), crate::layout::RAIL_WIDTH + self.channels_width + 8.);
                        cx.listener(move |this, e: &gpui_kit::ClickEvent, _, cx| this.open_voice_profile(&user_id, point(px(x), e.position().y - px(PROFILE_LIFT)), cx))
                    })
                    .into_any_element(),
            );
        }
        if !audience.is_empty() {
            rows.push(audience_row(audience.len(), cx).into_any_element());
        }
        // .vd-slot: where the person held over this channel will sit.
        if self.welcomes_voice_drop(channel, guild) {
            rows.push(div().h(px(VOICE_ROW)).pl_8().flex_none().child(div().size_full().squircle(ui::RADIUS_SM).border(px(THICK_LINE)).border_color(palette::accent()).bg(palette::accent_soft(cx))).into_any_element());
        }
        rows
    }
}
