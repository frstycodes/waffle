// Opening where you left off. At launch the shell is drawn from the snapshot (`snapshot.rs`) and the open
// channel's messages from the message cache, before the gateway connects; READY then replaces it all in
// place, keeping the open channel, its feed and its scroll, so nothing flickers. You can browse and type
// before READY; sending waits for it (`when_live`).
//
// Nothing live is drawn until READY confirms it: `live` gates your own status mark and every presence that
// would otherwise fall back to Offline, and the snapshot has no field for the rest.
use std::{sync::Arc, time::Duration};

use futures::{StreamExt, channel::oneshot};
use gpui_kit::{Context, Task, Window, px};

use crate::{
    Feed, Status, Waffle, cache, discord,
    snapshot::{self, Place, Shell, Snapshot},
    store,
    tiered::Read,
};

// How long a change waits before the snapshot is written again.
const SAVE_EVERY: Duration = Duration::from_secs(10);
// How long quitting waits for the last write.
const QUIT_FLUSH: Duration = Duration::from_millis(500);

pub struct Launch {
    // READY has arrived this session: presence and status may be drawn, and sends go out.
    pub live: bool,
    waiters: Vec<oneshot::Sender<()>>,
    // The bytes last written, so an unchanged shell isn't written again.
    written: Option<Vec<u8>>,
    // The open channel's newest message when its feed was last cached.
    cached_feed: Option<(String, String)>,
    pub messages: cache::Messages,
    _saver: Option<Task<()>>,
}

impl Launch {
    pub fn new() -> Self {
        Self { live: false, waiters: Vec::new(), written: None, cached_feed: None, messages: cache::tiers(), _saver: None }
    }
}

// Which account the launch belongs to: the saved one, unless DISCORD_TOKEN names someone we can't know yet.
fn launch_account(prefs: &crate::settings::Prefs) -> Option<String> {
    let from_env = std::env::var("DISCORD_TOKEN").is_ok_and(|t| !t.trim().is_empty());
    (!from_env).then(|| prefs.account.clone()).flatten().filter(|id| store::valid_id(id))
}

impl Waffle {
    // Once per app: listens for disk reads landing, saves the snapshot as it changes and on quit.
    pub(crate) fn start_store(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        store::sweep_unowned();
        let mut landed = store::on_landed();
        cx.spawn(async move |this, cx| {
            while landed.next().await.is_some() {
                // A burst of reads is one redraw.
                while landed.try_recv().is_ok() {}
                if this.update(cx, |this, cx| this.on_disk_landed(cx)).is_err() {
                    break;
                }
            }
        })
        .detach(); // lint-allow(detached-task): holds a WeakEntity and ends when the app's entity is gone or the store's channel closes
        self.launch._saver = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(SAVE_EVERY).await;
                if this.update(cx, |this, _| this.save_snapshot()).is_err() {
                    break;
                }
            }
        }));
        cx.on_app_quit(|this, _| {
            this.save_snapshot();
            let (done, wait) = std::sync::mpsc::channel();
            store::io(move || {
                let _ = done.send(());
            });
            let _ = wait.recv_timeout(QUIT_FLUSH);
            async {}
        })
        .detach(); // lint-allow(detached-task): the quit hook lives as long as the app, which is what it's for
        self.launch_from_snapshot(window, cx);
    }

    // Reads the account's snapshot and its open channel's messages off the UI thread, then draws them.
    pub(crate) fn launch_from_snapshot(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let account = launch_account(&self.prefs);
        store::set_account(account.as_deref());
        let Some(account) = account else { return };
        let reader = self.launch.messages.disk_reader();
        let read = cx.background_executor().spawn(async move {
            let snapshot = snapshot::read(&snapshot::path(&account)?, &account)?;
            let place = snapshot.place.as_ref().map(|p| p.channel_id.clone());
            let messages = reader.zip(place).and_then(|(reader, id)| Some((id.clone(), reader.read(&id)?)));
            Some((snapshot, messages))
        });
        cx.spawn_in(window, async move |this, cx| {
            let Some((snapshot, messages)) = read.await else { return };
            this.update_in(cx, |this, window, cx| this.apply_snapshot(snapshot, messages, window, cx)).ok();
        })
        .detach(); // lint-allow(detached-task): one read, applied once through a WeakEntity, then done
    }

    fn apply_snapshot(&mut self, snapshot: Snapshot, messages: Option<(String, Read<Vec<discord::Message>>)>, window: &mut Window, cx: &mut Context<Self>) {
        // READY got here first, or the session ended: the live state wins.
        if self.me.is_some() || self.launch.live || !matches!(self.status, Status::Connecting) {
            return;
        }
        self.me = Some(snapshot.me.user());
        self.my_account = snapshot.my_account();
        self.account_settings = snapshot.account_settings();
        self.developer_mode = snapshot.settings.developer_mode;
        self.guilds = snapshot.guilds();
        self.folders = snapshot.folders();
        self.dms = snapshot.dms();
        if let Some((id, read)) = messages {
            let feed = Feed { messages: Arc::new(read.0.clone()), loaded: true, has_more: true, ..Default::default() };
            self.launch.messages.insert_read(&id, read);
            self.feeds.insert(id, feed);
        }
        if let Some(place) = snapshot.place {
            self.open_place(place, window, cx);
        }
        self.finish_splash(cx);
        cx.notify(); // lint-allow(global-notify): the whole shell appears at once, every column changes
    }

    fn open_place(&mut self, place: Place, window: &mut Window, cx: &mut Context<Self>) {
        let channel = match &place.guild_id {
            Some(id) => self.guilds.iter().find(|g| &g.id == id).and_then(|g| g.channels.iter().find(|c| c.id == place.channel_id)).cloned(),
            None => self.dms.iter().find(|d| d.id == place.channel_id).cloned(),
        };
        let Some(channel) = channel else { return };
        if let Some((message, offset)) = place.scroll {
            self.scroll_anchors.insert(channel.id.clone(), (message, px(offset)));
        }
        self.guild_id = place.guild_id;
        self.open_channel(channel, window, cx);
        // Launch lands here; there's nothing to go back to.
        self.history = crate::home::History::default();
    }

    // READY: the live state has replaced the snapshot's. The open channel takes READY's copy of itself.
    pub(crate) fn reconcile_ready(&mut self, cx: &mut Context<Self>) {
        self.launch.live = true;
        // A demo's account is a fixture: nothing of it is kept.
        let demo = cfg!(debug_assertions) && std::env::var_os("WAFFLE_DEMO").is_some();
        store::set_account(self.me.as_ref().filter(|_| !demo).map(|me| me.id.as_str()));
        if let Some(open) = self.channel.as_ref().map(|c| c.id.clone()) {
            let fresh = self.guilds.iter().flat_map(|g| &g.channels).chain(&self.dms).find(|c| c.id == open).cloned();
            if let Some(fresh) = fresh {
                self.channel = Some(fresh);
            }
        }
        for waiter in self.launch.waiters.drain(..) {
            let _ = waiter.send(());
        }
        self.pin_neighbours();
        self.save_snapshot();
        cx.notify(); // lint-allow(global-notify): READY changes every column
    }

    // Resolves once READY has arrived; at once if it has.
    pub(crate) fn when_live(&mut self) -> impl Future<Output = ()> + use<> {
        let (send, receive) = oneshot::channel();
        match self.launch.live {
            true => drop(send.send(())),
            false => self.launch.waiters.push(send),
        }
        async move {
            let _ = receive.await;
        }
    }

    pub(crate) fn live(&self) -> bool {
        self.launch.live
    }

    // Your own status, once READY has said what it is.
    pub(crate) fn my_mark(&self) -> Option<discord::Status> {
        self.live().then_some(self.my_status)
    }

    // Someone's presence: what the gateway sent, Offline when it sent none, and nothing at all before READY,
    // when Offline would be a claim we can't make.
    pub(crate) fn presence_of(&self, user_id: &str) -> Option<discord::Presence> {
        self.live().then(|| self.presences.get(user_id).cloned().unwrap_or_default())
    }

    // Opening a channel: its cached messages if memory has them (a disk read lands later if not), and the
    // cache pins it and its neighbours.
    pub(crate) fn warm_feed(&mut self, channel_id: &str) {
        if !self.feeds.contains_key(channel_id)
            && let Some(messages) = cache::load(&mut self.launch.messages, channel_id)
        {
            self.feeds.insert(channel_id.to_owned(), Feed { messages: Arc::new(messages), loaded: true, has_more: true, ..Default::default() });
        }
        self.pin_neighbours();
    }

    fn pin_neighbours(&mut self) {
        let Some(open) = self.channel.as_ref().map(|c| c.id.clone()) else { return };
        let siblings: Vec<String> = match self.guild() {
            Some(guild) => guild.channels.iter().filter(|c| c.kind == discord::ChannelKind::Text).map(|c| c.id.clone()).collect(),
            None => self.dms.iter().map(|c| c.id.clone()).collect(),
        };
        let at = siblings.iter().position(|id| *id == open);
        let near = at.map(|i| siblings[i.saturating_sub(1)..(i + 2).min(siblings.len())].to_vec()).unwrap_or_default();
        self.launch.messages.pin(near.into_iter().chain([open]));
    }

    // Disk reads have landed: profiles, icons and previews read them on their next draw; a channel whose
    // feed is still empty takes its cached messages.
    fn on_disk_landed(&mut self, cx: &mut Context<Self>) {
        self.profiles.drain();
        if let Some(card) = self.profile_card.as_mut().filter(|c| c.profile.is_none()) {
            card.profile = self.profiles.peek(&crate::profile::profile_key(&card.user.id, card.guild_id.as_deref()));
        }
        for id in self.launch.messages.drain_keys() {
            let Some(messages) = self.launch.messages.peek(&id) else { continue };
            let feed = self.feeds.entry(id.clone()).or_default();
            if feed.loaded {
                continue;
            }
            (feed.messages, feed.loaded, feed.has_more) = (Arc::new(messages), true, true);
            let count = feed.messages.len();
            if self.channel.as_ref().is_some_and(|c| c.id == id) {
                self.list.reset(count);
                if let Some(anchor) = self.scroll_anchors.get(&id).cloned() {
                    self.scroll_to_anchor(anchor);
                }
            }
        }
        cx.notify(); // lint-allow(global-notify): disk reads land for profiles, icons, previews and feeds across every column
    }

    // Writes the shell if it changed since the last write, and caches the open feed if it grew. Only once
    // READY has confirmed it: before that there's nothing newer than what's on disk.
    pub(crate) fn save_snapshot(&mut self) {
        if !self.live() {
            return;
        }
        // Only the account the caches are keyed to (never a demo's, never one READY hasn't named).
        let Some(me) = self.me.as_ref().filter(|me| store::account().as_deref() == Some(me.id.as_str())) else { return };
        let place = self.channel.as_ref().map(|c| Place { guild_id: self.guild_id.clone(), channel_id: c.id.clone(), scroll: self.scroll_anchor().map(|(m, o)| (m, f32::from(o))) });
        let snapshot = Snapshot::capture(Shell { me, my_account: &self.my_account, settings: &self.account_settings, guilds: &self.guilds, folders: &self.folders, dms: &self.dms, place });
        if let Some(open) = self.channel.as_ref().map(|c| c.id.clone())
            && let Some(feed) = self.feeds.get(&open).filter(|f| f.loaded)
            && let Some(newest) = feed.messages.last().map(|m| m.id.clone())
            && self.launch.cached_feed.as_ref() != Some(&(open.clone(), newest.clone()))
        {
            cache::save(&mut self.launch.messages, &open, &feed.messages);
            self.launch.cached_feed = Some((open, newest));
        }
        let (Some(bytes), Some(path)) = (snapshot.to_bytes(), snapshot::path(&snapshot.account)) else { return };
        if self.launch.written.as_ref() == Some(&bytes) {
            return;
        }
        self.launch.written = Some(bytes.clone());
        store::io(move || {
            if let Err(e) = store::write_atomic(&path, &bytes) {
                eprintln!("snapshot: {e}");
            }
        });
    }
}
