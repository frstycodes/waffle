// A stage channel's view (design/explore-stage.html): the topic over the speakers' cards and the audience's
// faces, raised hands numbered in the order they went up, and a foot bar saying where you stand. Discord's
// behaviour and copy (docs/discord-stage-re.md); dark in both themes, like the call.
use std::time::Duration;

use gpui_kit::{
    AnyElement, Context, Div, FontWeight, Global, MouseButton, MouseDownEvent, SharedString, Stateful, Window,
    component::{Icon, tooltip::Tooltip},
    div, prelude::*, px, relative,
};

use crate::{
    Waffle,
    call::{CONTROLS_HEIGHT, Control, DOCK_PAD, FOCUS_HEAD, STAGE_PAD, control, control_group, stage_button, stage_icon_button, tabular},
    discord::{Channel, Guild, OnStage, VoiceState},
    icons::Glyph,
    menu::item,
    palette::{self, stage},
    stage::{STAGE, hand_queue, ordinal, requests_allowed},
    ui::{self, RADIUS_LG, RADIUS_MD, RADIUS_SM, Squircle},
};

// The foot bar: 12px over its 48px controls, 16px under.
const FOOT_HEIGHT: f32 = 12. + CONTROLS_HEIGHT + 16.;
const FACE: f32 = 48.;
const FACE_CELL: f32 = 88.;
const REQUESTS_WIDTH: f32 = 340.;
const INVITE_WIDTH: f32 = 440.;

// The stage whose requests popover is open, by channel id.
#[derive(Default)]
pub(crate) struct StageRequests(pub(crate) Option<String>);

impl Global for StageRequests {}

// Who's where on the stage, read once per frame.
struct Room<'a> {
    guild: &'a Guild,
    channel: &'a Channel,
    topic: Option<&'a str>,
    moderator: bool,
    // We may raise a hand: @everyone may, or we moderate.
    can_ask: bool,
    me: OnStage,
    speakers: usize,
    // Raised hands first, numbered by their place in line, then everyone else in the order they joined.
    audience: Vec<(&'a VoiceState, Option<usize>)>,
    hands: usize,
}

impl Room<'_> {
    fn my_place(&self, me: &str) -> Option<usize> {
        self.audience.iter().find(|(v, _)| v.user_id == me).and_then(|(_, place)| *place)
    }
}

impl Waffle {
    pub(crate) fn render_stage(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(room) = self.room() else { return self.stage_root(cx).into_any_element() };
        let (width, height) = self.stage_size(window);
        let requests_open = room.moderator && room.topic.is_some() && self.requests_open(&room.channel.id, cx);
        let body = match room.topic {
            Some(_) => self.stage_people(&room, width - STAGE_PAD * 2., cx),
            None => self.stage_waiting(&room, cx).into_any_element(),
        };
        self.stage_root(cx)
            .flex()
            .flex_col()
            .child(self.stage_head(self.stage_header(&room, cx)))
            .child(body)
            .child(self.stage_foot(&room, cx))
            .when(requests_open, |d| d.child(self.requests_popover(&room, height, window, cx)))
            .when(room.me == OnStage::Invited, |d| d.child(self.invite_card(&room, width, cx)))
            .into_any_element()
    }

    pub(crate) fn call_is_stage(&self) -> bool {
        self.call.as_ref().is_some_and(|c| c.channel.raw_type == STAGE)
    }

    fn stage_call(&self) -> Option<(&Guild, &Channel)> {
        let call = self.call.as_ref().filter(|_| self.call_is_stage())?;
        let guild = self.guilds.iter().find(|g| Some(&g.id) == call.guild_id.as_ref())?;
        Some((guild, guild.channel(&call.channel.id)?))
    }

    fn room(&self) -> Option<Room<'_>> {
        let (guild, channel) = self.stage_call()?;
        let me = self.me.as_ref()?.id.as_str();
        let people: Vec<&VoiceState> = guild.voice_states.iter().filter(|v| v.channel_id.as_deref() == Some(channel.id.as_str())).collect();
        let queue = hand_queue(people.iter().copied());
        let rest = people.iter().copied().filter(|v| matches!(v.on_stage(), OnStage::Audience | OnStage::Invited));
        let audience: Vec<_> = queue.iter().enumerate().map(|(i, v)| (*v, Some(i + 1))).chain(rest.map(|v| (v, None))).collect();
        let moderator = guild.is_stage_moderator(me, channel);
        Some(Room {
            guild,
            channel,
            topic: guild.stage(&channel.id).map(|s| s.topic.as_str()),
            moderator,
            can_ask: moderator || requests_allowed(guild, &channel.overwrites),
            me: self.my_stage_state(&guild.id).unwrap_or(OnStage::Audience),
            speakers: people.iter().filter(|v| v.on_stage() == OnStage::Speaker).count(),
            hands: queue.len(),
            audience,
        })
    }

    // Whether someone on this stage's call moderates it, for the mark on their card.
    pub(crate) fn stage_moderator(&self, user_id: &str) -> bool {
        let Some((guild, channel)) = self.stage_call() else { return false };
        if self.me.as_ref().is_some_and(|m| m.id == user_id) {
            return guild.is_stage_moderator(user_id, channel);
        }
        // GAP: someone the member list hasn't loaded yet reads as holding no roles.
        let roles = self.member_roles.get(&guild.id).and_then(|r| r.get(user_id)).map_or(&[][..], Vec::as_slice);
        guild.is_stage_moderator_with(user_id, roles, channel)
    }

    fn requests_open(&self, channel_id: &str, cx: &Context<Self>) -> bool {
        cx.try_global::<StageRequests>().is_some_and(|r| r.0.as_deref() == Some(channel_id))
    }

    // The topic as the title, then who's speaking and listening; moderators can edit the topic and open the
    // raised hands.
    fn stage_header(&self, room: &Room, cx: &mut Context<Self>) -> Div {
        let (title, line) = match room.topic {
            Some(topic) => (topic.to_owned(), format!("{} · {} speaking · {} in audience", room.channel.name, room.speakers, room.audience.len())),
            None => (room.channel.name.clone(), "Stage Channel".to_owned()),
        };
        let live = room.topic.is_some();
        let manage = live && room.moderator;
        let (guild_id, channel_id) = (room.guild.id.clone(), room.channel.id.clone());
        let edit = stage_icon_button("stage-edit", Glyph::Pen, "Edit Stage", false).occlude().on_click(cx.listener(move |this, _, window, cx| this.open_stage_topic(guild_id.clone(), channel_id.clone(), window, cx)));
        let open = self.requests_open(&room.channel.id, cx);
        let channel_id = room.channel.id.clone();
        let requests = div()
            .relative()
            .flex_none()
            .child(stage_icon_button("stage-requests-toggle", Glyph::Hand, if open { "Hide Requests" } else { "Show Requests" }, open).occlude().on_click(cx.listener(move |_, _, _, cx| {
                cx.set_global(StageRequests((!open).then(|| channel_id.clone())));
                cx.notify();
            })))
            .when(room.hands > 0, |d| d.child(ui::count_badge(room.hands as u32).absolute().top(px(-3.)).right(px(-5.))));
        div()
            .h(px(40.))
            .flex()
            .items_center()
            .gap(px(10.))
            .child(Icon::new(Glyph::Stage).size(px(20.)).text_color(stage::muted()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(div().truncate().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).text_color(stage::strong()).child(title))
                    .child(div().truncate().text_size(px(12.)).text_color(stage::muted()).font_features(tabular()).child(line)),
            )
            .when(live, |d| d.child(ui::live_badge()))
            .when(manage, |d| d.child(edit).child(requests))
            .child(div().w(px(1.)).h(px(20.)).mx(px(2.)).flex_none().bg(stage::wash(0.14)))
            .child(self.chat_toggle(cx))
    }

    fn stage_people(&self, room: &Room, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let hands = match room.hands {
            0 => String::new(),
            1 => " · 1 hand raised".to_owned(),
            n => format!(" · {n} hands raised"),
        };
        let faces: Vec<AnyElement> = room.audience.iter().map(|(state, place)| self.audience_face(state, *place, cx).into_any_element()).collect();
        div()
            .id("stage-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(STAGE_PAD))
            .pt(px(4.))
            .pb(px(STAGE_PAD))
            .flex()
            .flex_col()
            .gap(px(20.))
            .when(room.speakers > 0, |d| d.child(section(Glyph::Mic, format!("Speakers · {}", room.speakers)).child(self.speaker_grid(width, cx))))
            .when(!faces.is_empty(), |d| d.child(section(Glyph::Headphones, format!("Audience · {}{hands}", room.audience.len())).child(div().flex().flex_wrap().gap_x(px(8.)).gap_y(px(12.)).children(faces))))
            .into_any_element()
    }

    // One listener: their face, a raised hand carrying its place in line, their name and why they're asked for.
    fn audience_face(&self, state: &VoiceState, place: Option<usize>, cx: &mut Context<Self>) -> Stateful<Div> {
        let id = state.user_id.clone();
        let (name, avatar) = match &state.user {
            Some(user) => (user.name.clone(), user.avatar.clone()),
            None => (self.user_name(&id), self.user_avatar(&id)),
        };
        let why = match (place, state.on_stage()) {
            (Some(n), _) => Some(format!("{} in line", ordinal(n))),
            (None, OnStage::Invited) => Some("Invited".to_owned()),
            _ => None,
        };
        div()
            .id(SharedString::from(format!("stage-face-{id}")))
            .w(px(FACE_CELL))
            .px(px(4.))
            .py(px(8.))
            .squircle(RADIUS_MD)
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .hover(|s| s.bg(stage::wash(0.05)))
            .child(div().relative().size(px(FACE)).child(ui::avatar(avatar, FACE)).children(place.map(hand_badge)))
            .child(div().max_w(relative(1.)).truncate().text_size(px(12.)).text_color(stage::text()).child(name))
            .children(why.map(|why| div().text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::accent()).font_features(tabular()).child(why)))
            .on_mouse_down(MouseButton::Right, cx.listener(move |this, e: &MouseDownEvent, _, cx| this.voice_user_menu(&id, e.position, cx)))
    }

    fn stage_waiting(&self, room: &Room, cx: &mut Context<Self>) -> Div {
        let (title, body) = match room.moderator {
            true => ("Start the Stage", "Pick a topic to start the conversation."),
            false => ("Waiting for the Stage to start", "What will you be talking about today?"),
        };
        let (guild_id, channel_id) = (room.guild.id.clone(), room.channel.id.clone());
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .child(ui::icon_tile(Glyph::Stage, stage::wash(0.08), stage::strong()).size(px(56.)).mb(px(4.)))
            .child(div().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).text_color(stage::strong()).child(title))
            .child(div().text_size(px(13.)).text_color(stage::muted()).child(body))
            .when(room.moderator, |d| {
                d.child(ui::button("stage-start", ui::Tone::Primary, cx).mt(px(8.)).child("Start Stage").on_click(cx.listener(move |this, _, window, cx| this.open_stage_topic(guild_id.clone(), channel_id.clone(), window, cx))))
            })
    }

    // Where you stand, your mic (and for speakers camera and screen), the stage's own action, and the way out.
    fn stage_foot(&self, room: &Room, cx: &mut Context<Self>) -> Div {
        let live = room.topic.is_some();
        let speaker = room.me == OnStage::Speaker;
        let me = self.me.as_ref().map(|m| m.id.as_str()).unwrap_or_default();
        let say = live.then(|| say(room, room.my_place(me), self.muted || self.deafened));
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(STAGE_PAD))
            .pt(px(12.))
            .pb(px(16.))
            .child(div().flex_1().min_w_0().children(say))
            .child(control_group().child(self.mic_control(cx)).when(speaker, |g| g.child(self.camera_control(cx)).child(self.share_control(cx))))
            .when(live, |d| d.child(control_group().children(self.stage_actions(room, cx))))
            .child(self.disconnect_control(room, cx))
    }

    fn stage_actions(&self, room: &Room, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let (guild_id, channel_id) = (room.guild.id.clone(), room.channel.id.clone());
        if room.me == OnStage::Speaker {
            let leave = control("stage-leave-stage", Glyph::ArrowLineDown, "Leave the Stage", Control::Rest, true).on_click(cx.listener(move |this, _, _, cx| this.leave_the_stage(&guild_id, &channel_id, cx)));
            return vec![leave.into_any_element()];
        }
        let suppressed = matches!(room.me, OnStage::Audience | OnStage::HandRaised);
        let speak = (room.moderator && suppressed).then(|| {
            let (guild_id, channel_id) = (guild_id.clone(), channel_id.clone());
            speak_on_stage().on_click(cx.listener(move |this, _, _, cx| this.set_my_suppress(&guild_id, &channel_id, false, false, cx))).into_any_element()
        });
        let raised = room.me == OnStage::HandRaised;
        let tip = match (room.can_ask, raised) {
            (false, _) => "Raise Hand Disabled",
            (true, true) => "Cancel Request",
            (true, false) => "Request to Speak",
        };
        let hand = control("stage-hand", Glyph::Hand, tip, if raised { Control::Active } else { Control::Rest }, room.can_ask)
            .when(room.can_ask, |b| b.on_click(cx.listener(move |this, _, _, cx| this.raise_hand(&guild_id, &channel_id, !raised, cx))));
        speak.into_iter().chain([hand.into_any_element()]).collect()
    }

    // The red button leaves quietly; a moderator's chevron beside it ends the stage for everyone.
    fn disconnect_control(&self, room: &Room, cx: &mut Context<Self>) -> AnyElement {
        let (guild_id, channel_id) = (room.guild.id.clone(), room.channel.id.clone());
        let can_end = room.moderator && room.topic.is_some();
        let leave = ui::pressable(div().id("stage-disconnect"), 0.92)
            .w(px(if can_end { 48. } else { 56. }))
            .h(px(CONTROLS_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .bg(palette::danger())
            .text_color(gpui_kit::white())
            .cursor_pointer()
            .hover(|s| s.bg(palette::danger_hover()))
            .child(Icon::new(Glyph::CallEnd).size(px(20.)))
            .tooltip(|window, cx| Tooltip::new("Disconnect Quietly").build(window, cx))
            .on_click(cx.listener(move |this, _, _, cx| this.disconnect_from_stage(guild_id.clone(), channel_id.clone(), cx)));
        let shell = div().h(px(CONTROLS_HEIGHT)).flex_none().flex().squircle(10. + DOCK_PAD).overflow_hidden();
        if !can_end {
            return shell.child(leave).into_any_element();
        }
        let channel_id = room.channel.id.clone();
        let more = ui::pressable(div().id("stage-disconnect-more"), 0.92)
            .w(px(22.))
            .h(px(CONTROLS_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .bg(palette::danger())
            .text_color(gpui_kit::white())
            .cursor_pointer()
            .hover(|s| s.bg(palette::danger_hover()))
            .child(Icon::new(Glyph::ChevronUpBold).size(px(12.)))
            .tooltip(|window, cx| Tooltip::new("More ways to leave").build(window, cx))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    let channel_id = channel_id.clone();
                    let end = item("End Stage for Everyone", move |this, _, cx| this.open_end_stage(channel_id.clone(), cx)).icon(Glyph::Stage).danger().entry();
                    this.open_menu(e.position, vec![end], cx);
                }),
            );
        shell.gap(px(1.)).child(leave).child(more).into_any_element()
    }

    // Requests to Speak: whether @everyone may ask, then each raised hand in line, then anyone invited and
    // not yet up.
    fn requests_popover(&self, room: &Room, stage_height: f32, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let allowed = requests_allowed(room.guild, &room.channel.overwrites);
        let (guild_id, channel_id) = (room.guild.id.clone(), room.channel.id.clone());
        let allow = div()
            .id("stage-allow-requests")
            .min_h(px(34.))
            .px(px(10.))
            .py(px(8.))
            .squircle(RADIUS_SM)
            .flex()
            .items_center()
            .gap(px(12.))
            .cursor_pointer()
            .hover(|s| s.bg(stage::wash(0.05)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_size(px(14.)).font_weight(FontWeight::MEDIUM).text_color(stage::strong()).child("Allow requests from @everyone"))
                    .child(div().text_size(px(12.)).text_color(stage::muted()).child("Turn off to stop new hands going up.")),
            )
            .child(ui::switch_live("stage-allow-switch", allowed, window, cx))
            .on_click(cx.listener(move |this, _, _, cx| this.allow_requests(&guild_id, &channel_id, !allowed, cx)));
        let asks: Vec<(&VoiceState, Option<usize>)> = room.audience.iter().filter(|(v, place)| place.is_some() || v.on_stage() == OnStage::Invited).copied().collect();
        let list: AnyElement = match asks.is_empty() {
            true => requests_empty().into_any_element(),
            false => {
                let rows: Vec<AnyElement> = asks.into_iter().map(|(state, place)| self.request_row(room, state, place, cx).into_any_element()).collect();
                div().id("stage-requests-list").flex_1().min_h_0().overflow_y_scroll().flex().flex_col().gap(px(2.)).children(rows).into_any_element()
            }
        };
        let card = div()
            .id("stage-requests")
            .occlude()
            .absolute()
            .top(px(FOCUS_HEAD + 4.))
            .right(px(12.))
            .w(px(REQUESTS_WIDTH))
            .max_h(px((stage_height - FOCUS_HEAD - FOOT_HEIGHT - 8.).max(160.)))
            .p(px(6.))
            .squircle(RADIUS_LG)
            .bg(stage::overlay())
            .shadow(ui::shadow_pop_on(stage::wash(0.14), cx))
            .flex()
            .flex_col()
            .gap(px(2.))
            .text_color(stage::text())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .pt(px(8.))
                    .px(px(10.))
                    .pb(px(4.))
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(stage::strong())
                    .child(div().flex_1().child("Requests to Speak"))
                    .when(room.hands > 0, |d| d.child(ui::count_badge(room.hands as u32))),
            )
            .child(allow)
            .child(div().h(px(1.)).mx(px(10.)).my(px(4.)).flex_none().bg(stage::wash(0.08)))
            .child(list);
        // It hangs from the header's hand button, so it grows from its top right.
        ui::rise(card.transform_origin(gpui_kit::point(relative(1.), relative(0.))), "stage-requests", Duration::ZERO).into_any_element()
    }

    fn request_row(&self, room: &Room, state: &VoiceState, place: Option<usize>, cx: &mut Context<Self>) -> Stateful<Div> {
        let id = state.user_id.clone();
        let (name, avatar) = match &state.user {
            Some(user) => (user.name.clone(), user.avatar.clone()),
            None => (self.user_name(&id), self.user_avatar(&id)),
        };
        let invited = place.is_none();
        let detail = match invited {
            true => "Invited, waiting for them to accept".to_owned(),
            false => format!("Raised {}", raised_ago(state).to_lowercase()),
        };
        let (guild_id, channel_id) = (room.guild.id.clone(), room.channel.id.clone());
        let (dismiss_guild, dismiss_channel, dismiss_user) = (guild_id.clone(), channel_id.clone(), id.clone());
        let user = id.clone();
        let invite = ui::button(SharedString::from(format!("stage-invite-{id}")), ui::Tone::Primary, cx)
            .small()
            .disabled(invited)
            .child(if invited { "Invite Sent" } else { "Invite to Speak" })
            .when(!invited, |b| b.on_click(cx.listener(move |this, _, _, cx| this.answer_request(&guild_id, &channel_id, &user, true, cx))));
        div()
            .id(SharedString::from(format!("stage-request-{id}")))
            .min_h(px(48.))
            .pl(px(10.))
            .pr(px(6.))
            .py(px(4.))
            .squircle(RADIUS_SM)
            .flex()
            .items_center()
            .gap(px(10.))
            .hover(|s| s.bg(stage::wash(0.05)))
            .child(div().w(px(16.)).flex_none().flex().justify_center().text_size(px(13.)).font_weight(FontWeight::BOLD).text_color(palette::accent()).font_features(tabular()).children(place.map(|n| n.to_string())))
            .child(ui::avatar(avatar, 32.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(div().truncate().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(stage::strong()).child(name))
                    .child(div().truncate().text_size(px(12.)).text_color(stage::muted()).font_features(tabular()).child(detail)),
            )
            .child(invite)
            .child(
                stage_icon_button(SharedString::from(format!("stage-dismiss-{id}")), Glyph::X, "Dismiss", false)
                    .on_click(cx.listener(move |this, _, _, cx| this.answer_request(&dismiss_guild, &dismiss_channel, &dismiss_user, false, cx))),
            )
    }

    // A card over the stage rather than Discord's modal: only Not Now or Accept answers it.
    fn invite_card(&self, room: &Room, stage_width: f32, cx: &mut Context<Self>) -> AnyElement {
        let place = room.topic.map(str::to_owned).unwrap_or_else(|| room.channel.name.clone());
        let width = (stage_width - 24.).min(INVITE_WIDTH);
        let (guild_id, channel_id) = (room.guild.id.clone(), room.channel.id.clone());
        let (accept_guild, accept_channel) = (guild_id.clone(), channel_id.clone());
        let card = div()
            .id("stage-invite")
            .occlude()
            .absolute()
            .left(px((stage_width - width) / 2.))
            .bottom(px(FOOT_HEIGHT + 8.))
            .w(px(width))
            .py(px(12.))
            .pl(px(14.))
            .pr(px(12.))
            .squircle(RADIUS_LG)
            .bg(stage::overlay())
            .shadow(ui::shadow_pop_on(stage::wash(0.14), cx))
            .flex()
            .items_center()
            .gap(px(12.))
            .child(ui::icon_tile(Glyph::Stage, stage::wash(0.08), stage::strong()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(stage::strong()).child("You have been invited to speak."))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .text_size(px(12.))
                            .text_color(stage::muted())
                            .child(div().flex_none().child("Join the speakers in"))
                            .child(Icon::new(Glyph::Stage).size(px(12.)))
                            .child(div().min_w_0().truncate().child(place)),
                    ),
            )
            .child(stage_button("stage-invite-later", "Not Now").on_click(cx.listener(move |this, _, _, cx| this.set_my_suppress(&guild_id, &channel_id, true, false, cx))))
            .child(ui::button("stage-invite-accept", ui::Tone::Primary, cx).small().child("Accept").on_click(cx.listener(move |this, _, _, cx| this.set_my_suppress(&accept_guild, &accept_channel, false, false, cx))));
        ui::rise(card.transform_origin(gpui_kit::point(relative(0.5), relative(1.))), "stage-invite", Duration::ZERO).into_any_element()
    }
}

// The shield on a moderator's name chip.
pub(crate) fn moderator_mark(user_id: &str) -> Stateful<Div> {
    div()
        .id(SharedString::from(format!("stage-moderator-{user_id}")))
        .flex_none()
        .child(Icon::new(Glyph::Shield).size(px(14.)).text_color(gpui_kit::white()))
        .tooltip(|window, cx| Tooltip::new("Stage Moderator").build(window, cx))
}

// A section's label over its people, sentence case, counts in tabular figures.
fn section(glyph: Glyph, label: String) -> Div {
    div().flex().flex_col().gap(px(10.)).child(
        div().flex().items_center().gap(px(6.)).text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(stage::muted()).font_features(tabular()).child(Icon::new(glyph).size(px(14.))).child(label),
    )
}

// The raised hand on a face, carrying its place in line.
fn hand_badge(place: usize) -> Div {
    div()
        .absolute()
        .top(px(-4.))
        .right(px(-9.))
        .h(px(22.))
        .min_w(px(22.))
        .pl(px(5.))
        .pr(px(6.))
        .rounded_full()
        .flex()
        .items_center()
        .gap(px(2.))
        .bg(palette::accent())
        .border_2()
        .border_color(stage::bg())
        .text_color(palette::on_accent())
        .text_size(px(12.))
        .font_weight(FontWeight::BOLD)
        .font_features(tabular())
        .child(Icon::new(Glyph::Hand).size(px(12.)))
        .child(place.to_string())
}

// What the foot bar says about you, its title in the accent when you're waiting on someone.
fn say(room: &Room, place: Option<usize>, muted: bool) -> Div {
    let (title, detail) = match (room.me, place) {
        (OnStage::Speaker, _) => ("You're a speaker".to_owned(), if muted { "You're muted. Unmute when it's your turn." } else { "Everyone on the Stage hears you." }),
        (OnStage::Invited, _) => ("Invited to speak".to_owned(), "Accept to join the speakers."),
        (OnStage::HandRaised, Some(n)) => (format!("Hand raised, {} in line", ordinal(n)), "Moderators can see your request. Press the hand again to lower it."),
        _ if !room.can_ask => ("You're in the audience".to_owned(), "Moderators turned off requests to speak."),
        _ => ("You're in the audience".to_owned(), "Your mic will be muted unless you are invited to speak."),
    };
    let asks = matches!(room.me, OnStage::Invited | OnStage::HandRaised);
    div()
        .text_size(px(12.))
        .text_color(stage::muted())
        .font_features(tabular())
        .child(div().truncate().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(if asks { palette::accent() } else { stage::strong() }).child(title))
        .child(div().truncate().child(detail))
}

// .cbtn.wide: a control with its words, for the action that changes where you stand.
fn speak_on_stage() -> ui::Pressable {
    ui::pressable(div().id("stage-speak"), 0.92)
        .h(px(CONTROLS_HEIGHT - DOCK_PAD * 2.))
        .px(px(14.))
        .flex_none()
        .squircle(10.)
        .flex()
        .items_center()
        .gap(px(8.))
        .text_color(gpui_kit::white())
        .text_size(px(14.))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(|s| s.bg(stage::wash(0.08)))
        .child(Icon::new(Glyph::MicFill).size(px(20.)))
        .child("Speak on Stage")
}

fn requests_empty() -> Div {
    div()
        .pt(px(20.))
        .px(px(12.))
        .pb(px(24.))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(2.))
        .text_size(px(13.))
        .text_color(stage::muted())
        .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(stage::strong()).child("No Requests"))
        .child("Requests to speak will show up here.")
}

// How long ago a hand went up, in the feed's steps.
fn raised_ago(state: &VoiceState) -> String {
    let at = state.requested_at.as_deref().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok());
    let ms = at.map_or(0, |t| (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_milliseconds());
    crate::panel::ago(ms)
}
