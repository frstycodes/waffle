// DM and group calls (design/dm-call-panel.html): the call is one floating window over the whole app. It
// rings in its corner, becomes the call in place, shows video when someone is on camera or sharing, and
// opens into the server voice stage (call.rs) when asked. A call that isn't ringing us is a Join pill in the
// DM header. The gateway's CALL_* events drive all of it.
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use gpui_kit::{
    Animation, AnimationExt, AnyElement, Context, Div, FontWeight, Hsla, ImageSource, RenderImage, SharedString, Stateful,
    component::{Icon, tooltip::Tooltip},
    div, prelude::*, px,
};

use crate::{
    Waffle,
    discord::{Channel, ChannelKind, VoiceState},
    icons::Glyph,
    palette::{self, Level, stage},
    sounds,
    ui::{self, RADIUS_LG, RADIUS_MD, Squircle, Tone},
};

// The window is portrait unless a shared screen needs landscape; the ringing card is a landscape strip.
const PORTRAIT: f32 = 248.;
const LANDSCAPE: f32 = 440.;
const RING: (f32, f32) = (340., 102.);
const HEAD: f32 = 52.;
const BAR: f32 = 52.;
const INSET: f32 = 8.;
// A face in a two-person call, and in a group's two-column grid; each over a name line.
const DUO_FACE: f32 = 72.;
const GROUP_FACE: f32 = 52.;
const NAME_LINE: f32 = 16.;
// The row under the faces for Ring again or Watch.
const NOTE: f32 = 40.;
// How long "Call ended" holds, then how long it takes to go.
const ENDED_HOLD: Duration = Duration::from_millis(1500);
const ENDED_FADE: Duration = Duration::from_millis(260);

// What a DM's call is to us, from the gateway's view of it and ours.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum DmCallState {
    // It rings us: Decline or Join.
    Incoming,
    // We're in and ringing them; nobody else has joined.
    Ringing,
    Live,
    // We're in, nobody else is, and it no longer rings anyone.
    Alone,
    // A call goes on here without us (we declined a group call, or joined late).
    Joinable,
}

pub(crate) fn call_state(ringing_me: bool, in_it: bool, others: usize, ringing: bool, active: bool) -> Option<DmCallState> {
    match (ringing_me, in_it) {
        (true, false) => Some(DmCallState::Incoming),
        (_, true) if others > 0 => Some(DmCallState::Live),
        (_, true) if ringing => Some(DmCallState::Ringing),
        (_, true) => Some(DmCallState::Alone),
        (false, false) if active => Some(DmCallState::Joinable),
        (false, false) => None,
    }
}

// The height the faces take: one column for a two-person call, two columns for a group.
fn faces_height(count: usize, duo: bool) -> f32 {
    let count = count.max(1) as f32;
    match duo {
        true => 14. + count * (DUO_FACE + 6. + NAME_LINE) + (count - 1.) * 16. + 10.,
        false => {
            let rows = (count / 2.).ceil();
            12. + rows * (GROUP_FACE + 4. + NAME_LINE) + (rows - 1.) * 14. + 8.
        }
    }
}

// One person under the window's head.
struct Face {
    avatar: String,
    name: String,
    speaking: bool,
    muted: bool,
    // Still being rung, or rung and never came.
    pending: bool,
    gone: bool,
}

// What fills the window between its head and its controls.
enum Body {
    Faces(Vec<Face>),
    Camera { frame: Arc<RenderImage>, mine: Option<Arc<RenderImage>> },
    Stream { name: String, frame: Option<Arc<RenderImage>>, camera: Option<Arc<RenderImage>> },
}

// Under the faces: one action that belongs to this moment of the call.
enum Note {
    RingAgain,
    Watch { streamer: String, name: String },
}

impl Waffle {
    pub(crate) fn on_dm_call(&mut self, channel_id: String, ringing: Vec<String>, cx: &mut Context<Self>) {
        let rings_me = ringing.iter().any(|id| id == self.my_id());
        let in_it = self.call.as_ref().is_some_and(|c| c.channel.id == channel_id);
        // Nobody left to ring: they answered, declined, or it rang out.
        if ringing.is_empty() && self.ringing_out.as_ref() == Some(&channel_id) {
            self.stop_ringing_out();
        }
        let was_ringing_me = self.ringing_in.as_ref() == Some(&channel_id);
        if rings_me && !in_it && !was_ringing_me {
            self.ringing_in = Some(channel_id.clone());
            sounds::ring(sounds::Cue::CallRinging);
            if !self.window_active {
                let caller = self.dm_caller(&channel_id);
                // Not verified: whether macOS shows an unbundled binary's notification; the main window doesn't take focus.
                std::thread::spawn(move || {
                    let _ = notify_rust::Notification::new().summary(&caller).body("Incoming call").show();
                });
            }
        } else if was_ringing_me && (!rings_me || in_it) {
            self.stop_ringing_in();
        }
        self.dm_calls.insert(channel_id, ringing);
        cx.notify();
    }

    pub(crate) fn on_dm_call_ended(&mut self, channel_id: &str) {
        self.dm_calls.remove(channel_id);
        self.dm_voice.retain(|v| v.channel_id.as_deref() != Some(channel_id));
        if self.ringing_in.as_deref() == Some(channel_id) {
            self.stop_ringing_in();
        }
    }

    pub(crate) fn stop_ringing_in(&mut self) {
        if self.ringing_in.take().is_some() {
            sounds::stop_ring();
        }
    }

    // Joins a DM's call, ringing the others when nobody is in it yet. `video` turns the camera on once connected.
    pub(crate) fn start_dm_call(&mut self, channel: Channel, video: bool, cx: &mut Context<Self>) {
        if self.call.as_ref().is_some_and(|c| c.channel.id == channel.id) {
            return;
        }
        if self.ringing_in.as_ref() == Some(&channel.id) {
            self.stop_ringing_in();
        }
        if self.dm_others(&channel.id).is_empty() {
            self.ring_on_connect = Some(channel.id.clone());
        }
        self.join_voice_in(None, channel, cx);
        self.camera_on_connect = video;
    }

    // Join from the ringing card, the header pill or a call line. We stay where we are: the call starts in
    // its corner, and the DM is one click away from the window.
    pub(crate) fn answer_dm_call(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        let Some(dm) = self.dms.iter().find(|d| d.id == channel_id).cloned() else { return };
        self.start_dm_call(dm, false, cx);
    }

    // Stops it ringing for us only; a group call goes on.
    pub(crate) fn decline_dm_call(&mut self, channel_id: String, cx: &mut Context<Self>) {
        self.stop_ringing_in();
        let (Some(client), me) = (self.client.clone(), self.my_id().to_owned()) else { return };
        // Not verified: the decline endpoint and body, taken from Discord's client traffic as documented by third parties.
        crate::discord::runtime().spawn(async move {
            let _ = client.call(reqwest::Method::POST, format!("/channels/{channel_id}/call/stop-ringing"), Some(serde_json::json!({ "recipients": [me] }))).await;
        });
        cx.notify();
    }

    // Rings everyone in the DM who isn't in its call, with our ringtone until one joins.
    pub(crate) fn ring_dm(&mut self, channel_id: String, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        self.ring_out(channel_id.clone(), cx);
        crate::discord::runtime().spawn(async move {
            let _ = client.call(reqwest::Method::POST, format!("/channels/{channel_id}/call/ring"), Some(serde_json::json!({ "recipients": null }))).await;
        });
    }

    // Everyone else in the DM's call.
    pub(crate) fn dm_others(&self, channel_id: &str) -> Vec<&VoiceState> {
        let me = self.my_id();
        self.dm_voice.iter().filter(|v| v.channel_id.as_deref() == Some(channel_id) && v.user_id != me).collect()
    }

    fn dm_caller(&self, channel_id: &str) -> String {
        match self.dm_others(channel_id).first() {
            Some(v) => v.user.as_ref().map_or_else(|| self.user_name(&v.user_id), |u| u.name.clone()),
            None => self.dms.iter().find(|d| d.id == channel_id).map(|d| d.name.clone()).unwrap_or_else(|| "Someone".into()),
        }
    }

    pub(crate) fn dm_call_state(&self, channel_id: &str) -> Option<DmCallState> {
        let me = self.my_id();
        let ringing = self.dm_calls.get(channel_id).is_some_and(|r| r.iter().any(|id| id != me)) || self.ringing_out.as_deref() == Some(channel_id);
        let others = self.dm_others(channel_id).len();
        call_state(self.ringing_in.as_deref() == Some(channel_id), self.call.as_ref().is_some_and(|c| c.channel.id == channel_id), others, ringing, others > 0)
    }

    // Who the call still rings (or rang, once it's given up), us left out.
    fn dm_rung(&self, channel: &Channel) -> Vec<String> {
        let me = self.my_id();
        match channel.recipient.as_ref() {
            Some(r) => vec![r.id.clone()],
            None => self.dm_calls.get(&channel.id).cloned().unwrap_or_default().into_iter().filter(|id| id != me).collect(),
        }
    }

    // Leaving a DM call: the window reads "Call ended" for a moment, then goes.
    pub(crate) fn show_call_ended(&mut self, name: String, secs: u64, cx: &mut Context<Self>) {
        let at = Instant::now();
        self.dm_call_ended = Some((name, secs, at));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ENDED_HOLD + ENDED_FADE).await;
            this.update(cx, |this, cx| {
                if this.dm_call_ended.as_ref().is_some_and(|(_, _, shown)| *shown == at) {
                    this.dm_call_ended = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    // Opens the DM's call as the server voice stage, with the DM under it for Back to chat.
    pub(crate) fn open_dm_stage(&mut self, channel: Channel, window: &mut gpui_kit::Window, cx: &mut Context<Self>) {
        if self.channel.as_ref().is_none_or(|c| c.id != channel.id) {
            self.guild_id = None;
            self.open_channel(channel, window, cx);
        }
        self.call_view = true;
        cx.notify();
    }

    // The stage's way back to the DM's chat; the call goes on in its window.
    pub(crate) fn back_to_chat(&mut self, window: &mut gpui_kit::Window, cx: &mut Context<Self>) {
        if self.call_fullscreen {
            self.set_call_fullscreen(false, window, cx);
        }
        self.call_view = false;
        cx.notify();
    }

    // The floating call window: ringing, the call while we're off its stage, or its last word.
    pub(crate) fn call_window(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if let Some(channel_id) = self.ringing_in.as_deref() {
            return self.ring_window(channel_id, cx);
        }
        let call = self.call.as_ref().filter(|c| c.guild_id.is_none());
        if call.is_none()
            && let Some((name, secs, _)) = &self.dm_call_ended
        {
            return Some(self.ended_window(name, *secs, cx));
        }
        let call = call.filter(|_| !self.call_view)?;
        let channel = self.dms.iter().find(|d| d.id == call.channel.id).unwrap_or(&call.channel).clone();
        Some(self.live_window(&channel, cx))
    }

    // The window's surface, carried by the pointer like the stream picture-in-picture.
    fn window_frame(&self, id: &'static str, size: (f32, f32), cx: &mut Context<Self>) -> Stateful<Div> {
        self.floating_drag(div().id(id), cx)
            // Floats over the app: what's under it must not take the hover or the click.
            .occlude()
            .absolute()
            .w(px(size.0))
            .h(px(size.1))
            .squircle(RADIUS_LG)
            .overflow_hidden()
            .bg(stage::bg())
            .text_color(stage::strong())
            .shadow(ui::shadow_pop(cx))
            .flex()
            .flex_col()
    }

    // Landscape: who's calling on the left, Join over Decline on the right. Join is voice only; the camera
    // is one click away once we're in.
    fn ring_window(&self, channel_id: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dm = self.dms.iter().find(|d| d.id == channel_id)?;
        let face = match self.dm_others(channel_id).first() {
            Some(v) => ui::avatar(v.user.as_ref().map_or_else(|| self.user_avatar(&v.user_id), |u| u.avatar.clone()), 52.).into_any_element(),
            None => ui::dm_face(&dm.face(), 52., stage::bg()),
        };
        let line = if dm.recipient.is_some() { "Incoming call".to_owned() } else { format!("Calling {}", dm.name) };
        let (join_id, decline_id) = (channel_id.to_owned(), channel_id.to_owned());
        let card = self
            .window_frame("call-ringing", RING, cx)
            .flex_row()
            .items_center()
            .gap_3()
            .p_3p5()
            .child(
                div()
                    .size(px(52.))
                    .flex_none()
                    .rounded_full()
                    .child(face)
                    .with_animation("ringing-pulse", Animation::new(Duration::from_millis(1400)).repeat(), |d, t| d.opacity(0.6 + 0.4 * (t * std::f32::consts::TAU).cos().abs())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(div().truncate().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(self.dm_caller(channel_id)))
                    .child(div().truncate().text_size(ui::TEXT_XS).font_weight(FontWeight::SEMIBOLD).text_color(palette::accent()).child(line)),
            )
            .child(
                div()
                    .w(px(112.))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .child(
                        ui::button("ringing-join", Tone::Primary, cx)
                            .small()
                            .w_full()
                            .child(Icon::new(Glyph::Phone).size(ui::ICON_MD))
                            .child("Join")
                            .on_click(cx.listener(move |this, _, _, cx| this.answer_dm_call(&join_id, cx))),
                    )
                    .child(
                        ui::button("ringing-decline", Tone::Danger, cx)
                            .small()
                            .w_full()
                            .child(Icon::new(Glyph::CallEnd).size(ui::ICON_MD))
                            .child("Decline")
                            .on_click(cx.listener(move |this, _, _, cx| this.decline_dm_call(decline_id.clone(), cx))),
                    ),
            );
        Some(self.place_floating(card, RING))
    }

    fn ended_window(&self, name: &str, secs: u64, cx: &mut Context<Self>) -> AnyElement {
        let size = (PORTRAIT, HEAD);
        let head = self.window_frame("call-ended", size, cx).child(window_head("Call ended".into(), format!("{name} · lasted {}", lasted(secs as i64))));
        let hold = ENDED_HOLD.as_secs_f32() / (ENDED_HOLD + ENDED_FADE).as_secs_f32();
        div()
            .absolute()
            .inset_0()
            .child(self.place_floating(head, size))
            .with_animation("call-ended-out", Animation::new(ENDED_HOLD + ENDED_FADE), move |d, t| d.opacity(if t < hold { 1. } else { 1. - (t - hold) / (1. - hold) }))
            .into_any_element()
    }

    fn live_window(&self, channel: &Channel, cx: &mut Context<Self>) -> AnyElement {
        let call = self.call.as_ref().expect("in a DM call");
        let state = self.dm_call_state(&channel.id).unwrap_or(DmCallState::Alone);
        let group = channel.recipient.is_none();
        let others = self.dm_others(&channel.id);
        let clock = crate::call::clock(call.joined.elapsed().as_secs());
        let (title, sub) = match state {
            DmCallState::Ringing => (format!("Calling {}…", channel.name), format!("Ringing · {clock}")),
            DmCallState::Alone => (format!("{} didn't pick up", channel.name), "No one else is here".to_owned()),
            _ if group => (channel.name.clone(), format!("{} in call · {clock}", others.len() + 1)),
            _ => (channel.name.clone(), format!("In call · {clock}")),
        };
        let (body, note) = self.window_body(channel, state, &others);
        let (width, middle) = match &body {
            Body::Stream { .. } => (LANDSCAPE, (LANDSCAPE - 2. * INSET) * 9. / 16.),
            Body::Camera { .. } => (PORTRAIT, (PORTRAIT - 2. * INSET) * 4. / 3.),
            Body::Faces(faces) => (PORTRAIT, faces_height(faces.len(), !group)),
        };
        let size = (width, HEAD + middle + if note.is_some() { NOTE } else { 0. } + BAR);
        let stage_channel = channel.clone();
        let head = window_head(title, sub).child(
            window_icon_button("call-window-stage", Glyph::Fullscreen, "Open stage")
                .on_click(cx.listener(move |this, _, window, cx| this.open_dm_stage(stage_channel.clone(), window, cx))),
        );
        let middle = div().h(px(middle)).flex_none().child(match body {
            Body::Faces(faces) => faces_grid(faces, !group),
            Body::Camera { frame, mine } => framed(div().size_full().child(video(frame, RADIUS_MD)).children(mine.map(|f| inset(video(f, 9.), 0.32, 3. / 4.)))).into_any_element(),
            Body::Stream { name, frame, camera } => framed(
                div()
                    .size_full()
                    .bg(gpui_kit::black())
                    .child(match frame {
                        Some(frame) => ui::media(ImageSource::Render(frame), ui::all(RADIUS_MD)).absolute().inset_0().size_full().object_fit(gpui_kit::ObjectFit::Contain).into_any_element(),
                        None => crate::call::stream_fill(RADIUS_MD).absolute().inset_0().flex().items_center().justify_center().text_size(ui::TEXT_XS).text_color(gpui_kit::white().opacity(0.7)).child(format!("Joining {name}'s stream…")).into_any_element(),
                    })
                    .children(camera.map(|f| inset(video(f, 9.), 0.28, 9. / 16.))),
            )
            .into_any_element(),
        });
        let note = note.map(|note| {
            let row = div().h(px(NOTE)).flex_none().flex().items_center().justify_center();
            match note {
                Note::RingAgain => {
                    let id = channel.id.clone();
                    row.child(stage_wash_button("call-window-ring", Glyph::Phone, "Ring again".into()).on_click(cx.listener(move |this, _, _, cx| this.ring_dm(id.clone(), cx))))
                }
                Note::Watch { streamer, name } => row.child(
                    stage_wash_button("call-window-watch", Glyph::ShareScreen, format!("Watch {name}'s screen").into()).on_click(cx.listener(move |this, _, _, cx| this.watch_stream(streamer.clone(), cx))),
                ),
            }
        });
        let frame = self.window_frame("call-window", size, cx).child(head).child(middle).children(note).child(self.window_controls(cx));
        self.place_floating(frame, size)
    }

    // A watched stream outranks a camera; with neither, the faces. Someone sharing that we aren't watching
    // gets a Watch under the faces.
    fn window_body(&self, channel: &Channel, state: DmCallState, others: &[&VoiceState]) -> (Body, Option<Note>) {
        let call = self.call.as_ref().expect("in a DM call");
        let me = self.my_id();
        if let Some(watching) = call.focused() {
            let body = Body::Stream { name: self.user_name(&watching.streamer_id), frame: watching.frame.clone(), camera: call.cameras.get(&watching.streamer_id).cloned() };
            return (body, None);
        }
        if let Some(frame) = others.iter().find_map(|v| call.cameras.get(&v.user_id)) {
            return (Body::Camera { frame: frame.clone(), mine: call.cameras.get(me).cloned() }, None);
        }
        let first = |name: String| name.split_whitespace().next().map(str::to_owned).unwrap_or(name);
        let mut faces: Vec<Face> = others
            .iter()
            .map(|v| Face {
                avatar: v.user.as_ref().map_or_else(|| self.user_avatar(&v.user_id), |u| u.avatar.clone()),
                name: first(v.user.as_ref().map_or_else(|| self.user_name(&v.user_id), |u| u.name.clone())),
                speaking: self.speaking(&v.user_id),
                muted: v.muted || v.deafened,
                pending: false,
                gone: false,
            })
            .collect();
        faces.push(Face { avatar: self.user_avatar(me), name: "You".into(), speaking: self.speaking(me), muted: self.muted || self.deafened, pending: false, gone: false });
        if matches!(state, DmCallState::Ringing | DmCallState::Alone) {
            let rung = self.dm_rung(channel).into_iter().filter(|id| others.iter().all(|v| &v.user_id != id));
            faces.extend(rung.map(|id| Face {
                avatar: self.user_avatar(&id),
                name: if state == DmCallState::Alone { "No answer".into() } else { "Ringing…".into() },
                speaking: false,
                muted: false,
                pending: state == DmCallState::Ringing,
                gone: state == DmCallState::Alone,
            }));
        }
        let note = match state {
            DmCallState::Alone => Some(Note::RingAgain),
            _ => others.iter().find(|v| v.streaming).map(|v| Note::Watch { streamer: v.user_id.clone(), name: first(self.user_name(&v.user_id)) }),
        };
        (Body::Faces(faces), note)
    }

    // Only what's reached for mid-call: mute, camera, share, and leave. The rest lives on the stage.
    fn window_controls(&self, cx: &mut Context<Self>) -> Div {
        let call = self.call.as_ref().expect("in a DM call");
        let muted = self.muted || self.deafened;
        let sharing = call.sharing.is_some();
        let mic = window_control("call-window-mic", if muted { Glyph::MicOffFill } else { Glyph::MicFill }, if muted { "Unmute" } else { "Mute" }, if muted { Look::Off } else { Look::Rest }, true)
            .on_click(cx.listener(|this, _, _, cx| this.toggle_mute(cx)));
        let camera = window_control(
            "call-window-camera",
            if call.camera_on { Glyph::VideoFill } else { Glyph::VideoOffFill },
            match (call.connected, call.camera_on) {
                (false, _) => "Available once you're connected",
                (true, true) => "Turn off camera",
                (true, false) => "Turn on camera",
            },
            if call.camera_on { Look::On } else { Look::Rest },
            call.connected,
        )
        .when(call.connected, |b| b.on_click(cx.listener(|this, _, _, cx| this.toggle_camera(cx))));
        let share = window_control("call-window-share", Glyph::ShareScreenFill, if sharing { "Stop sharing" } else { "Share your screen" }, if sharing { Look::Active } else { Look::Rest }, call.connected)
            .when(call.connected, |b| b.on_click(cx.listener(|this, _, _, cx| this.toggle_screen_share(cx))));
        let leave = ui::pressable(div().id("call-window-leave"), 0.92)
            .w_11()
            .h_9()
            .flex_none()
            .squircle(12.)
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .bg(palette::danger())
            .hover(|s| s.bg(palette::danger_hover()))
            .text_color(gpui_kit::white())
            .child(Icon::new(Glyph::CallEnd).size(px(19.)))
            .tooltip(|window, cx| Tooltip::new("Leave call").build(window, cx))
            .on_click(cx.listener(|this, _, _, cx| this.leave_voice(cx)));
        let group = div().p_0p5().flex().gap_0p5().squircle(12.).bg(stage::wash(0.06)).child(mic).child(camera).child(share);
        div().h(px(BAR)).flex_none().pb_1().flex().items_center().justify_center().gap_1p5().child(group).child(leave)
    }

    // A call going on in this DM that isn't ringing us: who's in, a camera mark if anyone's on video, and Join.
    pub(crate) fn dm_join_pill(&self, channel: &Channel, cx: &mut Context<Self>) -> Option<AnyElement> {
        if channel.kind != ChannelKind::Dm || self.dm_call_state(&channel.id)? != DmCallState::Joinable {
            return None;
        }
        let others = self.dm_others(&channel.id);
        let video = others.iter().any(|v| v.camera || v.streaming);
        let faces: Vec<String> = others.iter().take(3).map(|v| v.user.as_ref().map_or_else(|| self.user_avatar(&v.user_id), |u| u.avatar.clone())).collect();
        let id = channel.id.clone();
        let names = others.iter().map(|v| v.user.as_ref().map_or_else(|| self.user_name(&v.user_id), |u| u.name.clone())).collect::<Vec<_>>().join(", ");
        Some(
            div()
                .id("dm-join-pill")
                .occlude()
                .h_8()
                .pl_1p5()
                .pr(px(3.))
                .mr_1()
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .squircle(RADIUS_MD)
                .bg(palette::hover(cx))
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::strong(cx))
                .child(ui::avatar_stack(faces, 22., palette::surface(Level::Page, cx)))
                .child("In a call")
                .when(video, |d| d.child(Icon::new(Glyph::Video).size(ui::ICON_SM).text_color(palette::muted(cx))))
                .child(
                    ui::button("dm-join", Tone::Primary, cx)
                        .small()
                        .child(Icon::new(Glyph::Phone).size(ui::ICON_SM))
                        .child("Join")
                        .on_click(cx.listener(move |this, _, _, cx| this.answer_dm_call(&id, cx))),
                )
                .tooltip(move |window, cx| Tooltip::new(format!("In the call: {names}")).build(window, cx))
                .into_any_element(),
        )
    }

    // The DM row's second line while a call goes on there.
    pub(crate) fn dm_call_line(&self, channel_id: &str, cx: &mut Context<Self>) -> Option<(Glyph, &'static str, Hsla)> {
        if self.ringing_in.as_deref() == Some(channel_id) {
            return Some((Glyph::Phone, "Calling you…", palette::accent_ink(cx)));
        }
        if self.call.as_ref().is_some_and(|c| c.channel.id == channel_id) {
            return Some((Glyph::Phone, "In call", palette::positive_ink(cx)));
        }
        (!self.dm_others(channel_id).is_empty()).then(|| (Glyph::Phone, "Call in progress", palette::positive_ink(cx)))
    }
}

// The window's head: title over the timer; it carries the window when dragged.
fn window_head(title: String, sub: String) -> Div {
    div()
        .h(px(HEAD))
        .flex_none()
        .pl_3()
        .pr_1p5()
        .flex()
        .items_center()
        .gap_1p5()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().truncate().text_size(ui::TEXT_SM).font_weight(FontWeight::SEMIBOLD).child(title))
                .child(div().truncate().text_size(ui::TEXT_XS).text_color(stage::muted()).font_features(crate::call::tabular()).child(sub)),
        )
}

fn window_icon_button(id: &'static str, glyph: Glyph, tip: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .size_8()
        .flex_none()
        .squircle(11.)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(stage::muted())
        .hover(|s| s.bg(stage::wash(0.08)).text_color(stage::strong()))
        .child(Icon::new(glyph).size(ui::ICON_LG))
        .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
}

fn stage_wash_button(id: &'static str, glyph: Glyph, label: SharedString) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(30.))
        .px_3()
        .flex()
        .items_center()
        .gap_1p5()
        .squircle(11.)
        .cursor_pointer()
        .bg(stage::wash(0.1))
        .hover(|s| s.bg(stage::wash(0.16)))
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(Icon::new(glyph).size(ui::ICON_SM))
        .child(label)
}

#[derive(Clone, Copy)]
enum Look {
    Rest,
    Off,
    On,
    Active,
}

// GAP: a compact twin of call.rs's stage `control`; fold into it once the regrouped stage bar (main session's
// work in call.rs) lands with a size option.
fn window_control(id: &'static str, glyph: Glyph, tip: &'static str, look: Look, enabled: bool) -> ui::Pressable {
    let (fill, hover, ink) = match look {
        Look::Rest => (gpui_kit::transparent_black(), stage::wash(0.1), gpui_kit::white()),
        Look::Off => (stage::off(false), stage::off(true), stage::off_ink()),
        Look::On => (stage::on(false), stage::on(true), stage::on_ink()),
        Look::Active => (palette::accent(), palette::accent_hover(), palette::on_accent()),
    };
    ui::pressable(div().id(id), if enabled { 0.92 } else { 1. })
        .w_9()
        .h_8()
        .flex_none()
        .squircle(10.)
        .flex()
        .items_center()
        .justify_center()
        .bg(fill)
        .text_color(ink)
        .child(Icon::new(glyph).size(px(17.)))
        .when(enabled, |b| b.cursor_pointer().hover(move |s| s.bg(hover)))
        .when(!enabled, |b| b.opacity(0.4).cursor(gpui_kit::CursorStyle::OperationNotAllowed))
        .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
}

// A two-person call stacks its two faces; a group lays them out two to a row.
fn faces_grid(faces: Vec<Face>, duo: bool) -> AnyElement {
    let size = if duo { DUO_FACE } else { GROUP_FACE };
    let cells = faces.into_iter().enumerate().map(|(i, f)| {
        // Speaking keeps presence green, as the stage's tiles do: a 2px ring 2px off the face.
        let ring = f.speaking.then(|| div().absolute().top_neg_1().left_neg_1().size(px(size + 8.)).rounded_full().border_2().border_color(stage::online()));
        let avatar = div().relative().size(px(size)).flex_none().rounded_full().child(ui::avatar(f.avatar, size)).children(ring);
        let avatar = match (f.pending, f.gone) {
            (true, _) => avatar
                .with_animation(SharedString::from(format!("call-window-ring-{i}")), Animation::new(Duration::from_millis(1400)).repeat(), |d, t| d.opacity(0.45 + 0.4 * (t * std::f32::consts::TAU).cos().abs()))
                .into_any_element(),
            (_, true) => avatar.opacity(0.35).into_any_element(),
            _ => avatar.into_any_element(),
        };
        let name = div()
            .h(px(NAME_LINE))
            .max_w_full()
            .flex()
            .items_center()
            .gap_1()
            .text_size(px(if duo { 13. } else { 11. }))
            .text_color(stage::muted())
            .when(f.muted, |d| d.child(Icon::new(Glyph::MicOff).size(ui::ICON_XS).text_color(stage::off_ink())))
            .child(div().min_w_0().truncate().child(f.name));
        div().min_w_0().flex().flex_col().items_center().gap(px(if duo { 6. } else { 4. })).child(avatar).child(name)
    });
    match duo {
        true => div().size_full().pt_3p5().pb_2p5().px_3().flex().flex_col().items_center().gap_4().children(cells).into_any_element(),
        false => div().size_full().pt_3().pb_2().px_3().grid().grid_cols(2).gap_x_2().gap_y_3p5().children(cells).into_any_element(),
    }
}

// Video inside the window, inset from its sides with its own corners.
fn framed(content: Div) -> Div {
    div().size_full().px(px(INSET)).child(content.relative().squircle(RADIUS_MD).overflow_hidden())
}

// gpui clips children to the box, not its corners, so the picture carries its own radius.
fn video(frame: Arc<RenderImage>, radius: f32) -> AnyElement {
    ui::media(ImageSource::Render(frame), ui::all(radius)).absolute().inset_0().size_full().object_fit(gpui_kit::ObjectFit::Cover).into_any_element()
}

// A small picture in the corner: our own camera over theirs, or the sharer's camera over their screen.
fn inset(picture: AnyElement, width: f32, aspect: f32) -> Div {
    div()
        .absolute()
        .right_2()
        .bottom_2()
        .w(gpui_kit::relative(width))
        .h(gpui_kit::relative(width * aspect))
        .squircle(9.)
        .overflow_hidden()
        .border_1()
        .border_color(stage::wash(0.14))
        .child(picture)
}

// "lasted 12 minutes": Discord's wording for a finished call.
pub(crate) fn lasted(secs: i64) -> String {
    match secs {
        ..60 => "a few seconds".into(),
        60..120 => "a minute".into(),
        120..3600 => format!("{} minutes", secs / 60),
        3600..7200 => "an hour".into(),
        _ => format!("{} hours", secs / 3600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_states() {
        assert_eq!(call_state(true, false, 1, false, true), Some(DmCallState::Incoming));
        assert_eq!(call_state(false, true, 0, true, false), Some(DmCallState::Ringing));
        assert_eq!(call_state(false, true, 1, true, true), Some(DmCallState::Live), "someone joined while others still ring");
        assert_eq!(call_state(false, true, 0, false, false), Some(DmCallState::Alone));
        assert_eq!(call_state(false, false, 2, false, true), Some(DmCallState::Joinable), "declined a group call that goes on");
        assert_eq!(call_state(false, false, 0, false, false), None);
        assert_eq!(call_state(true, true, 1, true, true), Some(DmCallState::Live), "answered on this device");
    }

    #[test]
    fn the_window_stays_portrait_for_faces() {
        // Two people stack in one column; a group of five takes three rows of two.
        assert_eq!(faces_height(2, true), 14. + 2. * 94. + 16. + 10.);
        assert_eq!(faces_height(5, false), 12. + 3. * 72. + 2. * 14. + 8.);
        assert!(HEAD + faces_height(2, true) + BAR > PORTRAIT);
        assert!(HEAD + faces_height(3, false) + BAR > PORTRAIT);
    }

    #[test]
    fn lasted_reads_like_discord() {
        assert_eq!(lasted(5), "a few seconds");
        assert_eq!(lasted(90), "a minute");
        assert_eq!(lasted(12 * 60 + 30), "12 minutes");
        assert_eq!(lasted(3700), "an hour");
        assert_eq!(lasted(3 * 3600), "3 hours");
    }
}
