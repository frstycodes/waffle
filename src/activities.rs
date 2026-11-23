// Activities in a call: the launcher, the running instances on the stage, the frame we're in, and the answers
// to what its SDK asks the client (who's here, which channel, sign-in through Discord).
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use gpui_kit::{
    AnyElement, Context, relative, Entity, InteractiveElement, IntoElement, MouseButton, ParentElement, SharedString, StatefulInteractiveElement, Styled, Subscription, Window,
    component::Icon, div, prelude::*, px,
};
use serde_json::{Value, json};

use crate::{
    Waffle,
    activity_frame::{ActivityFrame, FrameEvent},
    discord::{self, ActivityInstance, OAuthRequest, ShelfApp, User},
    mascot,
    part::Column,
    ui::prelude::*,
};

// Room left between a focused game and the controls under it, so their tooltips (which open upward near the
// window's foot) land on the stage instead of behind the game's web view, which draws over everything.
pub(crate) const TIP_ROOM: f32 = 36.;

mod bridge;
mod invite;
mod views;
use bridge::{Consent, PendingAsk};
pub(crate) use invite::{ChatCards, activity_message_card};
pub(crate) use views::who_is_playing;
use views::{names_line, plain_names};

// A start this slow says so and offers a way out; this slow, it has failed.
const STILL_STARTING: Duration = Duration::from_secs(10);
const START_TIMEOUT: Duration = Duration::from_secs(45);
// A frame whose app hasn't connected by now didn't load.
const LOAD_TIMEOUT: Duration = Duration::from_secs(25);
#[derive(Default)]
pub(crate) struct Activities {
    // Every running activity we've heard of, in any channel.
    instances: Vec<ActivityInstance>,
    // Names and icons, from the launcher's shelf and from lookups of apps not on it.
    pub(crate) apps: HashMap<String, ShelfApp>,
    pub(crate) shelf: Option<crate::activity_launcher::Shelf>,
    pub(crate) launcher: Option<crate::activity_launcher::Launcher>,
    // The app we asked to start or join, and since when, until its instance names us.
    pending: Option<(String, Instant)>,
    // The launch interaction's nonce, to match its failure on the gateway.
    start_nonce: Option<String>,
    // The last start that failed, and why, shown on its card until tried again or dismissed.
    failed: Option<(String, String)>,
    // The game closed itself, with its own last words, kept on its card with Play again.
    closed: Option<(String, String)>,
    // Where the game we're in (or starting) runs; None until we start or join one.
    place: Option<Place>,
    // How much of a chat a game over it takes, top down (0..1), and the split's bounds while dragging it.
    chat_split: Option<f32>,
    splitting: bool,
    split_bounds: std::rc::Rc<std::cell::Cell<Option<gpui_kit::Bounds<gpui_kit::Pixels>>>>,
    // An activity to join once the call we're connecting to is up (joined from outside its call).
    join_on_connect: Option<String>,
    // The game's own window, when popped out of its slot.
    pub(crate) popout: Option<gpui_kit::AnyWindowHandle>,
    // The game we're in and when we got into it, for the voice panel's row; kept beside the frame so the panel's
    // height can be read without the app context.
    running: Option<(String, Instant)>,
    _ticker: Option<gpui_kit::Task<()>>,
    // What our presence says about the game we're in: its app and start, plus the lines the app sets itself.
    presence: Option<Value>,
    // Our game's frame never said hello: its card offers Reload in its place.
    unloaded: Option<String>,
    pub(crate) frame: Option<Entity<ActivityFrame>>,
    washes: invite::Washes,
    _frame_events: Option<Subscription>,
    consent: Option<Consent>,
    // Something the app asked to do that needs your say first.
    ask: Option<PendingAsk>,
}

// Where a game runs: a voice call's stage, or a text channel or DM where it plays over the chat.
#[derive(Clone)]
pub(crate) struct Place {
    pub channel: discord::Channel,
    pub guild_id: Option<String>,
    pub voice: bool,
}

// Discord's activity cue for an instance's update (its rules, read from its web build 2026-11-23): one starting
// in your voice channel or you getting into one launches; you out of yours ends; your game gaining or losing a
// player joins or leaves. Quiet for games elsewhere.
fn activity_cue(before: Option<usize>, after: usize, in_call: bool, was_in: bool, now_in: bool) -> Option<crate::sounds::Cue> {
    use crate::sounds::Cue;
    match (was_in, now_in) {
        (false, true) => Some(Cue::ActivityLaunch),
        (true, false) => Some(Cue::ActivityEnd),
        (true, true) if after > before.unwrap_or(after) => Some(Cue::ActivityUserJoin),
        (true, true) if after < before.unwrap_or(after) => Some(Cue::ActivityUserLeft),
        (false, false) if in_call && before.is_none() && after > 0 => Some(Cue::ActivityLaunch),
        _ => None,
    }
}

impl Activities {
    pub(crate) fn in_channel<'a>(&'a self, channel_id: &'a str) -> impl Iterator<Item = &'a ActivityInstance> + 'a {
        self.instances.iter().filter(move |i| i.channel_id == channel_id)
    }
}

impl Waffle {
    pub(crate) fn call_place(&self) -> Option<Place> {
        let call = self.call.as_ref()?;
        Some(Place { channel: call.channel.clone(), guild_id: call.guild_id.clone(), voice: true })
    }

    // The chat on screen, as a place a game can run over.
    pub(crate) fn chat_place(&self) -> Option<Place> {
        let channel = self.channel.clone()?;
        let guild_id = self.guilds.iter().find(|g| g.channels.iter().any(|c| c.id == channel.id)).map(|g| g.id.clone());
        Some(Place { channel, guild_id, voice: false })
    }

    // Where the game we're in (or starting) runs: its own place once there is one, else the call.
    pub(crate) fn game_place(&self) -> Option<Place> {
        self.activities.place.clone().or_else(|| self.call_place())
    }

    // The place an instance runs in, from what we know of its channel.
    fn instance_place(&self, instance: &ActivityInstance) -> Option<Place> {
        if let Some(call) = self.call_place().filter(|p| p.channel.id == instance.channel_id) {
            return Some(call);
        }
        let channel = match &instance.guild_id {
            Some(guild_id) => self.guilds.iter().find(|g| &g.id == guild_id)?.channel(&instance.channel_id).cloned()?,
            None => self.dms.iter().find(|d| d.id == instance.channel_id).cloned()?,
        };
        let voice = matches!(channel.kind, discord::ChannelKind::Voice);
        Some(Place { channel, guild_id: instance.guild_id.clone(), voice })
    }

    // The stage's activity cards: every activity running in the call's channel, and the one we're starting.
    pub(crate) fn activity_tiles(&self) -> Vec<String> {
        let Some(call) = &self.call else { return Vec::new() };
        let mut apps: Vec<String> = self.activities.in_channel(&call.channel.id).map(|i| i.application_id.clone()).collect();
        let held = [&self.activities.failed, &self.activities.closed].map(|held| held.as_ref().map(|(app_id, _)| app_id.as_str()));
        for app_id in [self.pending_app(), held[0], held[1]].into_iter().flatten() {
            if !apps.iter().any(|a| a == app_id) {
                apps.insert(0, app_id.to_owned());
            }
        }
        apps
    }

    fn pending_app(&self) -> Option<&str> {
        self.activities.pending.as_ref().map(|(app_id, _)| app_id.as_str())
    }

    // A start that went wrong stays on its card, with why and a way to try again.
    fn fail_activity(&mut self, app_id: &str, error: String, cx: &mut Context<Self>) {
        if self.pending_app() == Some(app_id) {
            self.activities.pending = None;
        }
        self.activities.failed = Some((app_id.to_owned(), error));
        self.redraw(Column::Main, cx);
    }

    pub(crate) fn in_activity(&self, app_id: &str, cx: &gpui_kit::App) -> bool {
        self.activities.frame.as_ref().is_some_and(|f| f.read(cx).app_id == app_id)
    }

    pub(crate) fn on_activity_instance(&mut self, instance: ActivityInstance, cx: &mut Context<Self>) {
        let same = |i: &ActivityInstance| i.application_id == instance.application_id && i.location_id == instance.location_id;
        let before = self.activities.instances.iter().find(|i| same(i)).map(|i| i.participants.len());
        let was_in = self.in_activity(&instance.application_id, cx) && self.game_place().is_some_and(|p| p.channel.id == instance.channel_id);
        self.activities.instances.retain(|i| !same(i));
        self.fetch_app(&instance.application_id, cx);
        // Our gateway session in it means this client joined it: the frame opens wherever it runs.
        // So does one we asked to start or join, under any session of ours: after a restart Discord can keep our
        // old session in the game, and the join only confirms it.
        let asked = self.pending_app() == Some(instance.application_id.as_str());
        let me_in = instance.participants.iter().any(|p| self.me.as_ref().is_some_and(|m| m.id == p.user_id) && (p.session_id == self.gateway_session || asked));
        if !instance.participants.is_empty() {
            self.activities.instances.push(instance.clone());
        }
        let framed = was_in;
        let in_call = self.call.as_ref().is_some_and(|c| c.channel.id == instance.channel_id);
        if let Some(cue) = activity_cue(before, instance.participants.len(), in_call, was_in, me_in) {
            crate::sounds::play(cue);
        }
        match (me_in, framed) {
            (true, false) if self.activities.frame.is_none() => {
                self.activities.place = self.instance_place(&instance);
                self.open_frame(instance.clone(), cx);
            }
            (false, true) => self.close_frame(cx),
            _ => {}
        }
        if let Some(frame) = &self.activities.frame
            && framed
        {
            let participants = self.sdk_participants(&instance);
            frame.read(cx).dispatch("ACTIVITY_INSTANCE_PARTICIPANTS_UPDATE", json!({ "participants": participants }));
        }
        // The channel list and chat cards show every channel's activities.
        self.redraw(Column::Nav, cx);
        self.redraw(Column::Main, cx);
    }

    // An app's name and icon for a card, when it isn't on the shelf we have.
    fn fetch_app(&mut self, app_id: &str, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        if self.activities.apps.contains_key(app_id) {
            return;
        }
        let id = app_id.to_owned();
        self.activities.apps.insert(id.clone(), ShelfApp::bare(id.clone(), "Activity".into(), String::new(), None));
        cx.spawn(async move |this, cx| {
            let Ok(app) = client.call(reqwest::Method::GET, format!("/applications/{id}/rpc"), None).await else { return };
            let icon = app["icon"].as_str().map(|icon| discord::app_icon_url(&id, icon));
            let name = app["name"].as_str().unwrap_or("Activity").to_owned();
            let description = app["description"].as_str().unwrap_or_default().to_owned();
            let assets = client.app_assets(&id).await.unwrap_or_default();
            let art = |name: &str| assets.iter().find(|(n, _)| n == name).map(|(_, asset)| discord::app_asset_url(&id, asset));
            let (cover, background) = (art("embedded_cover").or_else(|| art("embedded_background")), art("embedded_background").or_else(|| art("embedded_cover")));
            _ = this.update(cx, |this, cx| {
                this.activities.apps.insert(id.clone(), ShelfApp { cover, background, ..ShelfApp::bare(id, name, description, icon) });
                this.redraw(Column::Nav, cx);
                this.redraw(Column::Main, cx);
            });
        })
        .detach(); // lint-allow(detached-task): a one-shot fetch that ends when the request returns, holding a WeakEntity
    }

    pub(crate) fn app_name(&self, app_id: &str) -> String {
        self.activities.apps.get(app_id).map(|a| a.name.clone()).unwrap_or_else(|| "Activity".into())
    }

    // Starts `app_id` in the call, or joins it if it's already running there. One activity at a time: leaving
    // the one we're in for another asks first.
    pub(crate) fn launch_activity(&mut self, app_id: String, cx: &mut Context<Self>) {
        let Some(place) = self.activities.launcher.as_ref().map(|l| l.place.clone()).or_else(|| self.call_place()) else { return };
        self.launch_activity_at(app_id, place, cx);
    }

    pub(crate) fn launch_activity_at(&mut self, app_id: String, place: Place, cx: &mut Context<Self>) {
        self.activities.launcher = None;
        if self.in_activity(&app_id, cx) {
            self.call_focus = Some(activity_key(&app_id));
            return cx.notify(); // lint-allow(global-notify): closes the root-drawn launcher and refocuses the stage
        }
        let Some(current) = self.activities.frame.as_ref().map(|f| f.read(cx).app_id.clone()) else { return self.start_or_join(app_id, place, cx) };
        let (from, to) = (self.app_name(&current), self.app_name(&app_id));
        let body = format!("You'll leave {from}. Anyone still playing it carries on without you.");
        self.confirm(format!("Switch to {to}?"), body, "Switch", move |this, _, cx| this.start_or_join(app_id.clone(), place.clone(), cx), cx);
    }

    pub(crate) fn start_or_join(&mut self, app_id: String, place: Place, cx: &mut Context<Self>) {
        // A second press while the first is on its way would send the launch twice.
        if self.pending_app() == Some(app_id.as_str()) {
            return;
        }
        self.activities.closed = None;
        if let Some(why) = self.activities_blocked(&place) {
            if !place.voice {
                return self.show_toast(format!("Couldn't start {}: {why}.", self.app_name(&app_id)), cx);
            }
            self.call_focus = Some(activity_key(&app_id));
            return self.fail_activity(&app_id, format!("{why}."), cx);
        }
        let Some(client) = self.client.clone() else { return };
        let (channel_id, guild_id) = (place.channel.id.clone(), place.guild_id.clone());
        let running = self.activities.in_channel(&channel_id).any(|i| i.application_id == app_id);
        self.leave_activity(cx);
        self.activities.place = Some(place);
        self.activities.failed = None;
        self.activities.pending = Some((app_id.clone(), Instant::now()));
        self.call_focus = Some(activity_key(&app_id));
        self.watch_start(app_id.clone(), cx);
        // Already listed under an older session of ours (a restart): no update will come, so open it now.
        let me = self.me.as_ref().map(|m| m.id.clone());
        let listed = self.activities.in_channel(&channel_id).find(|i| i.application_id == app_id && i.participants.iter().any(|p| Some(&p.user_id) == me.as_ref())).cloned();
        if let Some(instance) = listed {
            self.activities.place = self.instance_place(&instance);
            self.open_frame(instance, cx);
        }
        let session = self.gateway_session.clone();
        let nonce = crate::nonce();
        self.activities.start_nonce = Some(nonce.clone());
        cx.spawn(async move |this, cx| {
            let result = match running {
                true => client.join_activity(&app_id, &channel_id, guild_id.as_deref(), &session).await,
                false => client.start_activity(&app_id, &channel_id, guild_id.as_deref(), &session, &nonce).await,
            };
            let Err(error) = result else { return };
            _ = this.update(cx, |this, cx| this.fail_activity(&app_id, error, cx));
        })
        .detach(); // lint-allow(detached-task): a one-shot request that ends when it returns, holding a WeakEntity
        cx.notify(); // lint-allow(global-notify): closes the root-drawn launcher
    }

    // A frame that hasn't heard from its app by LOAD_TIMEOUT didn't load: its card takes over with Reload.
    fn watch_load(&mut self, app_id: String, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(LOAD_TIMEOUT).await;
            _ = this.update(cx, |this, cx| {
                let stuck = this.activities.frame.as_ref().is_some_and(|f| f.read(cx).app_id == app_id && !f.read(cx).connected);
                if stuck {
                    this.activities.unloaded = Some(app_id);
                    cx.notify(); // lint-allow(global-notify): the root decides the frame's visibility, the stage draws the card
                }
            });
        })
        .detach(); // lint-allow(detached-task): ends after its one timer fires, holding a WeakEntity
    }

    // Opens the game again in the same instance, keeping your place in it.
    fn reload_activity(&mut self, cx: &mut Context<Self>) {
        let Some(app_id) = self.activities.unloaded.take() else { return };
        let instance = self.game_instance(&app_id);
        let place = self.activities.place.clone();
        self.close_frame(cx);
        self.activities.place = place;
        match instance {
            Some(instance) => {
                self.activities.pending = Some((app_id, Instant::now()));
                self.open_frame(instance, cx);
            }
            None => self.fail_activity(&app_id, "That activity isn't around any more.".into(), cx),
        }
    }

    // The game ended itself: we leave its instance, and its card keeps what it said, with Play again.
    fn activity_closed(&mut self, message: String, cx: &mut Context<Self>) {
        let Some(app_id) = self.activities.frame.as_ref().map(|f| f.read(cx).app_id.clone()) else { return };
        self.leave_activity(cx);
        self.activities.closed = Some((app_id.clone(), message));
        self.call_focus = Some(activity_key(&app_id));
        self.redraw(Column::Main, cx);
    }

    // Joins `app_id` in `channel`'s call: straight in when we're there, else into the call first and the
    // game as soon as it connects.
    pub(crate) fn join_activity_in(&mut self, channel: discord::Channel, app_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let here = self.call.as_ref().is_some_and(|c| c.channel.id == channel.id && c.connected);
        self.show_call(channel, window, cx);
        match here {
            true => self.launch_activity(app_id, cx),
            false => self.activities.join_on_connect = Some(app_id),
        }
    }

    pub(crate) fn activity_on_connect(&mut self, cx: &mut Context<Self>) {
        if let Some(app_id) = self.activities.join_on_connect.take() {
            self.launch_activity(app_id, cx);
        }
    }

    // Where someone is playing `app_id`, when it's a voice channel of a server we're in: for Join on their card.
    pub(crate) fn activity_place(&self, user_id: &str, app_id: &str) -> Option<discord::Channel> {
        let instance = self.activities.instances.iter().find(|i| i.application_id == app_id && i.participants.iter().any(|p| p.user_id == user_id))?;
        let guild = self.guilds.iter().find(|g| Some(&g.id) == instance.guild_id.as_ref())?;
        guild.channel(&instance.channel_id).cloned()
    }

    // The game you're in, for your presence: what Discord's client says for an embedded app.
    pub(crate) fn activity_presence(&self) -> Option<Value> {
        self.activities.presence.clone()
    }

    // Someone in a game here, for their row's mark when their presence doesn't say (it can lag the game).
    pub(crate) fn activity_mark(&self, user_id: &str, channel_id: &str) -> Option<crate::games::Playing> {
        let instance = self.activities.in_channel(channel_id).find(|i| i.participants.iter().any(|p| p.user_id == user_id))?;
        let app = self.activities.apps.get(&instance.application_id)?;
        Some(crate::games::Playing { icon: app.icon.clone()?, name: app.name.clone(), started: None })
    }

    pub(crate) fn on_interaction_failed(&mut self, nonce: &str, reason: u64, cx: &mut Context<Self>) {
        if self.activities.start_nonce.as_deref() != Some(nonce) {
            return;
        }
        self.activities.start_nonce = None;
        if let Some(app_id) = self.pending_app().map(str::to_owned) {
            self.fail_activity(&app_id, discord::launch_failure(reason).to_owned(), cx);
        }
    }

    // Why activities can't start in this call at all, when they can't: the AFK channel, or Use Activities off.
    pub(crate) fn activities_blocked(&self, place: &Place) -> Option<&'static str> {
        let guild = self.guilds.iter().find(|g| Some(&g.id) == place.guild_id.as_ref())?;
        if guild.afk_channel_id.as_ref() == Some(&place.channel.id) {
            return Some("You can't start activities in the AFK channel");
        }
        (self.place_permissions(place) & discord::perm::USE_EMBEDDED_ACTIVITIES == 0).then_some("A moderator turned off Use Activities here")
    }

    // The instance of `app_id` where our game runs.
    fn game_instance(&self, app_id: &str) -> Option<ActivityInstance> {
        let place = self.game_place()?;
        self.activities.in_channel(&place.channel.id).find(|i| i.application_id == app_id).cloned()
    }

    // A start that hasn't come up: its card says so at STILL_STARTING, and it fails at START_TIMEOUT.
    fn watch_start(&mut self, app_id: String, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            for wait in [STILL_STARTING, START_TIMEOUT - STILL_STARTING] {
                cx.background_executor().timer(wait).await;
                let still = this.update(cx, |this, cx| {
                    let still = this.pending_app() == Some(app_id.as_str());
                    if still {
                        this.redraw(Column::Main, cx);
                    }
                    still
                });
                if !still.unwrap_or(false) {
                    return;
                }
            }
            _ = this.update(cx, |this, cx| this.fail_activity(&app_id, "It didn't start in time".into(), cx));
        })
        .detach(); // lint-allow(detached-task): ends when the activity starts or the wait times out, holding a WeakEntity
    }

    fn cancel_start(&mut self, cx: &mut Context<Self>) {
        self.activities.pending = None;
        self.activities.failed = None;
        self.activities.closed = None;
        self.call_focus = None;
        self.redraw(Column::Main, cx);
    }

    pub(crate) fn join_activity(&mut self, app_id: String, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.launch_activity(app_id, cx);
    }

    // Leaving voice ends your part in a game: with others in it, ask once, naming them. Alone, just leave.
    pub(crate) fn leave_call_asking(&mut self, cx: &mut Context<Self>) {
        let Some((app, others)) = self.game_with_others(cx) else { return self.leave_voice(cx) };
        let channel = self.call.as_ref().map(|c| c.channel.name.clone()).unwrap_or_default();
        let body = format!("You're playing {app} with {}. Leaving {channel} takes you out of the game too.", plain_names(&others));
        self.confirm("Leave the call?", body, "Leave call", |this, _, cx| this.leave_voice(cx), cx);
    }

    // Moving to another voice channel leaves the game the same way, so it asks the same.
    pub(crate) fn switch_call_asking(&mut self, to: discord::Channel, cx: &mut Context<Self>, go: impl Fn(&mut Waffle, &mut Context<Waffle>) + 'static) {
        let moving = self.call.as_ref().is_some_and(|c| c.channel.id != to.id);
        let Some((app, others)) = self.game_with_others(cx).filter(|_| moving) else { return go(self, cx) };
        let body = format!("You're playing {app} with {}. Moving to {} takes you out of the game.", plain_names(&others), to.name);
        self.confirm(format!("Move to {}?", to.name), body, "Move", move |this, _, cx| go(this, cx), cx);
    }

    // The game we're in and who else is in it, when someone is.
    fn game_with_others(&self, cx: &gpui_kit::App) -> Option<(String, Vec<User>)> {
        self.game_place().filter(|p| p.voice)?;
        let app_id = self.activities.frame.as_ref()?.read(cx).app_id.clone();
        let me = self.me.as_ref().map(|m| m.id.clone());
        let others: Vec<User> = self.players(&app_id).into_iter().filter(|u| Some(&u.id) != me.as_ref()).collect();
        (!others.is_empty()).then(|| (self.app_name(&app_id), others))
    }

    // Everyone in `app_id`'s game where ours runs (or the call), you included.
    pub(crate) fn players(&self, app_id: &str) -> Vec<User> {
        self.game_place().map(|p| self.players_in(&p.channel.id, app_id)).unwrap_or_default()
    }

    pub(crate) fn players_in(&self, channel_id: &str, app_id: &str) -> Vec<User> {
        self.activities.in_channel(channel_id).find(|i| i.application_id == app_id).map(|i| i.participants.iter().map(|p| self.player(&p.user_id)).collect()).unwrap_or_default()
    }

    // Leaving by choice: the toast holds Rejoin for a few seconds, naming who's still playing.
    pub(crate) fn leave_with_rejoin(&mut self, cx: &mut Context<Self>) {
        let Some(app_id) = self.activities.frame.as_ref().map(|f| f.read(cx).app_id.clone()) else { return };
        let me = self.me.as_ref().map(|m| m.id.clone());
        let others: Vec<User> = self.players(&app_id).into_iter().filter(|u| Some(&u.id) != me.as_ref()).collect();
        self.leave_activity(cx);
        let detail = (!others.is_empty()).then(|| SharedString::from(format!("{} still playing", names_line(&others))));
        let rejoin = app_id.clone();
        let action = crate::toast::ToastAction { glyph: Glyph::ActivitiesFill, detail, label: "Rejoin".into(), run: std::rc::Rc::new(move |this, cx| this.launch_activity(rejoin.clone(), cx)) };
        self.show_toast_action(format!("You left {}", self.app_name(&app_id)), action, cx);
    }

    // Leaves the activity we're in, if any: Discord's leave, then the frame closes at once.
    pub(crate) fn leave_activity(&mut self, cx: &mut Context<Self>) {
        let Some(app_id) = self.activities.frame.as_ref().map(|f| f.read(cx).app_id.clone()) else { return };
        crate::sounds::play(crate::sounds::Cue::ActivityEnd);
        if let (Some(client), Some(instance)) = (self.client.clone(), self.game_instance(&app_id)) {
            let session = self.gateway_session.clone();
            cx.spawn(async move |_, _| _ = client.leave_activity(&instance, &session).await).detach(); // lint-allow(detached-task): a one-shot leave request that ends when it returns, holding no entity
        }
        self.close_frame(cx);
    }

    // Our instance came up: a proxy ticket, then the frame. Built on the next turn, where the window is free.
    fn open_frame(&mut self, instance: ActivityInstance, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let window = self.main_window;
        cx.spawn(async move |this, cx| {
            let ticket = client.activity_proxy_ticket(&instance.application_id, &instance.channel_id).await;
            _ = window.update(cx, |_, window, cx| {
                _ = this.update(cx, |this, cx| {
                    // Opened already (Join found us listed, then Discord's update came): one frame only.
                    if this.activities.frame.is_some() {
                        return;
                    }
                    let ticket = match ticket {
                        Ok(ticket) => ticket,
                        Err(error) => return this.fail_activity(&instance.application_id, error, cx),
                    };
                    let params = frame_params(&instance, ticket);
                    let app_id = instance.application_id.clone();
                    let frame = cx.new(|cx| ActivityFrame::new(app_id.clone(), params, window, cx));
                    if frame.read(cx).failed() {
                        return this.fail_activity(&app_id, "Waffle couldn't open a web view for it".into(), cx);
                    }
                    this.activities._frame_events = Some(cx.subscribe(&frame, |this, frame, event: &FrameEvent, cx| match event {
                        FrameEvent::Command { nonce, cmd, args } => this.answer_activity(frame, nonce.clone(), cmd, args, cx),
                        FrameEvent::Closed { message } => this.activity_closed(message.clone(), cx),
                    }));
                    this.activities.frame = Some(frame);
                    this.activities.running = Some((app_id.clone(), Instant::now()));
                    this.tick_activity_row(cx);
                    this.activities.presence = Some(json!({ "type": 0, "name": this.app_name(&app_id), "application_id": app_id, "timestamps": { "start": chrono::Utc::now().timestamp_millis() } }));
                    this.publish_presence();
                    this.activities.unloaded = None;
                    this.activities.closed = None;
                    this.watch_load(app_id.clone(), cx);
                    this.activities.pending = None;
                    this.activities.start_nonce = None;
                    this.call_focus = Some(activity_key(&app_id));
                    cx.notify(); // lint-allow(global-notify): the frame's visibility is decided by the root render
                });
            });
        })
        .detach(); // lint-allow(detached-task): a one-shot start that ends when the request returns, holding a WeakEntity
    }

    // What the app draws beside or over a game, lifted over its webview while one is open: the webview sits on
    // the app's base layer and trails it a frame on a resize, so only the stage itself stays under it.
    pub(crate) fn above_game(&self, element: impl IntoElement) -> AnyElement {
        match self.activities.frame.is_some() {
            // lint-allow(raw-floating): drawn in its own place; its elements take the pointer themselves
            true => gpui_kit::deferred(element).into_any_element(),
            false => element.into_any_element(),
        }
    }

    // The demo scenes' game: `url` playing on the call's stage, as an activity of our own would.
    pub(crate) fn demo_activity(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_ref() else { return };
        let me = discord::ActivityParticipant { user_id: "4194304".into(), session_id: "demo".into() };
        self.activities.instances.push(ActivityInstance {
            application_id: "demo".into(),
            launch_id: "demo".into(),
            instance_id: "demo".into(),
            location_id: "demo".into(),
            channel_id: call.channel.id.clone(),
            guild_id: call.guild_id.clone(),
            participants: vec![me],
        });
        self.activities.frame = Some(cx.new(|cx| ActivityFrame::demo(url, window, cx)));
        self.activities.running = Some(("demo".into(), Instant::now()));
        self.call_focus = Some(activity_key("demo"));
    }

    fn close_frame(&mut self, cx: &mut Context<Self>) {
        let Some(frame) = self.activities.frame.take() else { return };
        let app_id = frame.read(cx).app_id.clone();
        self.activities._frame_events = None;
        self.activities.running = None;
        self.activities._ticker = None;
        self.activities.place = None;
        if let Some(popout) = self.activities.popout.take() {
            cx.defer(move |cx| _ = popout.update(cx, |_, window, _| window.remove_window()));
        }
        self.activities.consent = None;
        self.activities.ask = None;
        if self.activities.presence.take().is_some() {
            self.publish_presence();
        }
        if self.call_focus.as_deref() == Some(activity_key(&app_id).as_str()) {
            self.call_focus = None;
        }
        cx.notify(); // lint-allow(global-notify): the stage and the root both drew the frame
    }

    // When the call ends, the activity goes with it.
    pub(crate) fn end_activities(&mut self, cx: &mut Context<Self>) {
        self.activities.pending = None;
        self.activities.failed = None;
        self.activities.closed = None;
        self.activities.unloaded = None;
        self.activities.launcher = None;
        self.close_frame(cx);
    }

    // The game shows while the call is on screen with its card drawn. Composed, menus, dialogs and settings
    // draw over it; otherwise the webview covers them, so it hides while one is up.
    pub(crate) fn sync_activity_frame(&self, cx: &mut Context<Self>) {
        let Some(frame) = &self.activities.frame else { return };
        let (app_id, composed) = frame.read_with(cx, |frame, _| (frame.app_id.clone(), frame.composed()));
        let held = self.drag.is_some();
        frame.update(cx, |frame, _| frame.set_held(held));
        let covered = !composed && (self.menu.is_some()
            || self.dialog.is_some()
            || self.device_menu.is_some()
            || self.connection_info
            || self.settings_open()
            || self.activities.launcher.is_some()
            || self.activities.consent.is_some()
            || self.activities.ask.is_some()
            || self.activities.unloaded.is_some()
            || self.viewer.is_some());
        if self.activities.popout.is_some() {
            return frame.update(cx, |frame, _| frame.set_shown(true));
        }
        let Some(place) = self.game_place() else { return };
        let on_screen = self.game_on_screen(&place);
        let focused = self.call_focus.is_none() || self.call_focus.as_deref() == Some(activity_key(&app_id).as_str());
        let on_stage = on_screen && (!place.voice || focused);
        // Away from its place it plays in the floating window, which a ring takes the place of.
        let in_pip = !on_screen && self.ringing_in.is_none();
        frame.update(cx, |frame, _| frame.set_shown((on_stage || in_pip) && !covered));
    }

    fn call_permissions(&self) -> u64 {
        self.game_place().map(|p| self.place_permissions(&p)).unwrap_or(0)
    }

    pub(crate) fn place_permissions(&self, place: &Place) -> u64 {
        let Some(me) = &self.me else { return 0 };
        let guild = place.guild_id.as_ref().and_then(|id| self.guilds.iter().find(|g| &g.id == id));
        // A DM has no permissions to check: everything an activity asks is allowed there.
        guild.map(|g| g.channel_permissions(&me.id, &place.channel)).unwrap_or(u64::MAX)
    }

    // Whether the game's place is what's on screen, so the game shows there and not in the corner.
    pub(crate) fn game_on_screen(&self, place: &Place) -> bool {
        match place.voice {
            true => self.call_view,
            false => !self.call_view && !self.inbox_page && self.channel.as_ref().is_some_and(|c| c.id == place.channel.id),
        }
    }

}

pub(crate) fn activity_key(app_id: &str) -> String {
    format!("activity:{app_id}")
}

// The query the activity's frame opens with, as Discord's client builds it.
fn frame_params(instance: &ActivityInstance, ticket: String) -> Vec<(&'static str, String)> {
    let mut params = vec![
        ("instance_id", instance.instance_id.clone()),
        ("location_id", instance.location_id.clone()),
        ("launch_id", instance.launch_id.clone()),
        ("channel_id", instance.channel_id.clone()),
        ("discord_proxy_ticket", ticket),
    ];
    if let Some(guild_id) = &instance.guild_id {
        params.push(("guild_id", guild_id.clone()));
    }
    params
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_cues_follow_discord() {
        use crate::sounds::Cue;
        assert_eq!(activity_cue(None, 1, true, false, false), Some(Cue::ActivityLaunch), "someone starts one in your call");
        assert_eq!(activity_cue(None, 1, false, false, false), None, "a game in another channel is quiet");
        assert_eq!(activity_cue(Some(1), 2, true, false, true), Some(Cue::ActivityLaunch), "you get into one");
        assert_eq!(activity_cue(Some(2), 3, true, true, true), Some(Cue::ActivityUserJoin));
        assert_eq!(activity_cue(Some(3), 2, true, true, true), Some(Cue::ActivityUserLeft));
        assert_eq!(activity_cue(Some(2), 1, true, true, false), Some(Cue::ActivityEnd));
        assert_eq!(activity_cue(Some(2), 3, true, false, false), None, "others joining a game you're not in");
    }

}
