use super::*;

pub(super) async fn run(shared: Arc<Shared>, leave: oneshot::Receiver<()>) {
    let mut dispatches = shared.gateway.dispatches.subscribe();
    shared.send_voice_state(true);
    shared.emit(VoiceEvent::Stage(Stage::AwaitingEndpoint));
    let reason = tokio::select! {
        result = join_voice(&shared, &mut dispatches) => result.err().unwrap_or_else(|| "Voice connection closed".to_owned()),
        _ = leave => "Left".to_owned(),
    };
    // A superseded session's call already lives on in the next one: a leave or a Disconnected
    // from it would end that call.
    if shared.superseded.load(Ordering::Acquire) {
        return;
    }
    // 4006 is how the voice server drops us when another client of ours joins; its voice state can
    // reach us after the close, so a 4006 doesn't send the leave either.
    let elsewhere = shared.taken_elsewhere.load(Ordering::Acquire);
    if !elsewhere && !reason.ends_with("(4006)") {
        shared.send_voice_state(false);
    }
    let reason = if elsewhere { ANOTHER_LOCATION.to_owned() } else { reason };
    shared.emit(VoiceEvent::Stage(Stage::Disconnected));
    shared.emit(VoiceEvent::Disconnected(reason));
}

pub(super) async fn join_voice(shared: &Arc<Shared>, dispatches: &mut broadcast::Receiver<(String, Value)>) -> Result<(), String> {
    let mut target = await_target(shared, dispatches).await?;
    let mut backoff = Backoff::connection();
    let on_dispatch = |kind: &str, d: &Value| voice_dispatch(shared, kind, d);
    loop {
        let reason = match serve(shared, target.clone(), dispatches, on_dispatch, &mut backoff).await {
            Ended::Dispatch(reason) => reason,
            Ended::Lost(lost) => {
                debug_lost(&target, &lost);
                // Discord asks the main gateway to check its voice server whenever the connection drops.
                shared.gateway.send(5, Value::Null);
                if !lost.starts_with(NO_ROUTE) {
                    shared.emit(VoiceEvent::Stage(Stage::Disconnected));
                }
                match wait_out(backoff.fail(), dispatches, on_dispatch).await {
                    None => continue,
                    Some(reason) => reason,
                }
            }
        };
        // A fresh main-gateway session: our voice state went with the old one, so it goes out again.
        if shared.session_id.lock().is_ok_and(|id| id.is_none()) {
            shared.send_voice_state(true);
            target = await_target(shared, dispatches).await?;
            continue;
        }
        // Discord reallocates a call's voice server with a fresh VOICE_SERVER_UPDATE (docs: Voice, "Voice
        // Server Update"): the same session identifies on the new endpoint.
        let Some(server) = take_moved(shared) else { return Err(reason) };
        shared.emit(VoiceEvent::Stage(Stage::AwaitingEndpoint));
        target.server = server;
    }
}

// Waits for our voice server, as long as it takes, and the session to identify with on it.
pub(super) async fn await_target(shared: &Shared, dispatches: &mut broadcast::Receiver<(String, Value)>) -> Result<Target, String> {
    shared.emit(VoiceEvent::Stage(Stage::AwaitingEndpoint));
    let (session_id, server) = await_voice_server(shared, dispatches).await?;
    if let Ok(mut id) = shared.session_id.lock() {
        *id = Some(session_id.clone());
    }
    Ok(Target {
        server_id: shared.server_id().to_owned(),
        dave_channel: shared.channel_id.clone(),
        session_id,
        server,
        role: Role::Voice,
        streamer: String::new(),
    })
}

// Sits out a lost connection's backoff while following dispatches: None to try again, or why one
// ended it ("Gateway closed" once the gateway handle is gone).
pub(super) async fn wait_out(delay: Duration, dispatches: &mut broadcast::Receiver<(String, Value)>, on_dispatch: impl Fn(&str, &Value) -> Option<String>) -> Option<String> {
    let ended = async {
        loop {
            let Some((kind, d)) = next_dispatch(dispatches).await else { return "Gateway closed".to_owned() };
            if let Some(reason) = on_dispatch(&kind, &d) {
                return reason;
            }
        }
    };
    tokio::time::timeout(delay, ended).await.ok()
}

fn debug_lost(target: &Target, lost: &str) {
    if debug() {
        eprintln!("[{:?}] connection lost, identifying again: {lost}", target.role);
    }
}

pub(super) fn take_moved(shared: &Shared) -> Option<Server> {
    shared.moved_server.lock().ok().and_then(|mut s| s.take())
}

// Follows our channel's voice states while connected; ends the connection if we were moved out.
pub(super) fn voice_dispatch(shared: &Shared, kind: &str, d: &Value) -> Option<String> {
    if kind == "READY" {
        if let Ok(mut id) = shared.session_id.lock() {
            *id = None;
        }
        return Some(NEW_SESSION.into());
    }
    if kind == "VOICE_SERVER_UPDATE" {
        let for_us = d["guild_id"].as_str().or(d["channel_id"].as_str()) == Some(shared.server_id()) && d["channel_id"].as_str().is_none_or(|c| c == shared.channel_id);
        // A null endpoint means another update follows with the new server; wait for that one.
        let server = parse_server(d).filter(|_| for_us)?;
        if let Ok(mut moved) = shared.moved_server.lock() {
            *moved = Some(server);
        }
        return Some("Voice server moved".into());
    }
    if kind != "VOICE_STATE_UPDATE" {
        return None;
    }
    let user_id = d["user_id"].as_str()?;
    let in_channel = d["channel_id"].as_str() == Some(&shared.channel_id);
    if user_id == shared.user_id && taken_elsewhere(shared.session_id.lock().ok().and_then(|id| id.clone()).as_deref(), shared.guild_id.as_deref(), d) {
        shared.taken_elsewhere.store(true, Ordering::Release);
        return Some(ANOTHER_LOCATION.into());
    }
    if user_id == shared.user_id && !in_channel {
        let session = shared.session_id.lock().ok().and_then(|id| id.clone());
        match ours_left(session.as_deref(), d) {
            Left::Not => return None,
            Left::Removed => return Some("Removed from the voice channel".into()),
            Left::MovedTo(channel) => {
                shared.supersede();
                shared.emit(VoiceEvent::Moved(channel.to_owned()));
                return Some("Moved".into());
            }
        }
    }
    if user_id == shared.user_id && in_channel {
        note_server_mute(shared, d);
    }
    let streaming = in_channel && d["self_stream"].as_bool().unwrap_or(false);
    let camera = in_channel && d["self_video"].as_bool().unwrap_or(false);
    shared.emit(VoiceEvent::VideoState { user_id: user_id.to_owned(), streaming, camera });
    None
}

// A fresh main-gateway session (READY): voice rejoins under it, and Go Live streams end, as Discord's do.
pub const NEW_SESSION: &str = "New gateway session";
// A lost connection whose UDP discovery got no answer: the call keeps showing No Route meanwhile.
const NO_ROUTE: &str = "No route";

// Why a call ended when another client of ours joined voice in the same server and took it from this one.
pub const ANOTHER_LOCATION: &str = "Connected at another location";

// Our account in voice under another session in the same server (or, for DMs, in another call): one of
// our other clients took this call. Discord's own client keeps its call when another session joins voice
// in a different server, so each client can hold a call in its own server.
pub(super) fn taken_elsewhere(our_session: Option<&str>, our_guild: Option<&str>, d: &Value) -> bool {
    let theirs = d["session_id"].as_str();
    our_session.zip(theirs).is_some_and(|(ours, theirs)| ours != theirs) && d["channel_id"].is_string() && d["guild_id"].as_str() == our_guild
}

pub(super) enum Left<'a> {
    Not,
    Removed,
    MovedTo(&'a str),
}

// What a voice state of ours outside our channel means. Our own session in another channel is a move,
// and with no channel a disconnect. Another session of ours (another client) says nothing about ours
// here: one joining our server is caught first by `taken_elsewhere`. With either session unknown, it's
// taken as a removal.
pub(super) fn ours_left<'a>(our_session: Option<&str>, d: &'a Value) -> Left<'a> {
    let (theirs, channel) = (d["session_id"].as_str(), d["channel_id"].as_str());
    match (our_session.zip(theirs).map(|(a, b)| a == b), channel) {
        (Some(true), Some(channel)) => Left::MovedTo(channel),
        (Some(false), _) => Left::Not,
        _ => Left::Removed,
    }
}

#[derive(Clone)]
pub(super) struct Server {
    pub(super) token: String,
    pub(super) endpoint: String,
}

#[derive(Clone)]
pub(super) struct Target {
    pub(super) server_id: String,
    // DAVE's MLS group id: the voice channel, or a Go Live's `rtc_channel_id`.
    pub(super) dave_channel: String,
    pub(super) session_id: String,
    pub(super) server: Server,
    pub(super) role: Role,
    // Whose Go Live a Watch connection carries; empty otherwise.
    pub(super) streamer: String,
}

// STREAM_DELETE's reasons (Discord's client, 2026-11-21), plus `unavailable: true` as its own.
pub const STREAM_DELETE_UNAVAILABLE: &str = "unavailable";
pub const STREAM_DELETE_FULL: &str = "stream_full";
pub const STREAM_DELETE_UNAUTHORIZED: &str = "unauthorized";
pub const STREAM_DELETE_RATE_LIMITED: &str = "safety_guild_rate_limited";
pub const STREAM_DELETE_ENDED: &str = "stream_ended";

pub(super) async fn await_voice_server(shared: &Shared, dispatches: &mut broadcast::Receiver<(String, Value)>) -> Result<(String, Server), String> {
    let (mut session_id, mut server) = (None, None);
    loop {
        let (kind, d) = next_dispatch(dispatches).await.ok_or("Gateway closed")?;
        let for_us = d["guild_id"].as_str().or(d["channel_id"].as_str()) == Some(shared.server_id());
        // A quick drag through several channels leaves the skipped channels' updates in flight: their
        // token identifies a session Discord has already moved on from (4006).
        let other_channel = d["channel_id"].as_str().is_some_and(|c| c != shared.channel_id);
        let ours = d["user_id"].as_str() == Some(&shared.user_id);
        match kind.as_str() {
            // Our voice state went with the old session: send it again from this one.
            "READY" => {
                (session_id, server) = (None, None);
                shared.send_voice_state(true);
            }
            "VOICE_STATE_UPDATE" if ours && for_us && d["channel_id"].is_null() && session_id.is_some() && d["session_id"].as_str() == session_id.as_deref() => {
                return Err("Removed from the voice channel".into());
            }
            "VOICE_STATE_UPDATE" if ours && for_us && !other_channel => {
                note_server_mute(shared, &d);
                session_id = d["session_id"].as_str().map(str::to_owned);
            }
            "VOICE_SERVER_UPDATE" if for_us && !other_channel => server = parse_server(&d).or(server),
            _ => {}
        }
        if let (Some(id), Some(server)) = (&session_id, &server) {
            return Ok((id.clone(), server.clone()));
        }
    }
}

fn note_server_mute(shared: &Shared, d: &Value) {
    let silenced = d["mute"].as_bool().unwrap_or(false) || d["suppress"].as_bool().unwrap_or(false);
    shared.server_muted.store(silenced, Ordering::Relaxed);
}

// A null endpoint means the server is being reallocated; another update follows.
pub(super) fn parse_server(d: &Value) -> Option<Server> {
    Some(Server { token: d["token"].as_str()?.to_owned(), endpoint: d["endpoint"].as_str()?.to_owned() })
}

// Main-gateway stream ops: 18 create (go live), 19 delete (stop streaming or watching),
// 20 watch, 22 set paused.
// How a stream task starts and stops: `after` holds it until the stream it replaces is gone, `stop` ends
// it, and `gone` tells whatever replaces it that Discord has deleted it.
pub(super) struct StreamEnds {
    pub(super) stop: oneshot::Receiver<()>,
    pub(super) after: Option<oneshot::Receiver<()>>,
    pub(super) gone: oneshot::Sender<()>,
}

pub(super) async fn stream(shared: Arc<Shared>, role: Role, streamer: String, key: String, session_id: String, ends: StreamEnds) {
    let StreamEnds { stop, after, gone } = ends;
    if let Some(after) = after {
        let _ = after.await;
    }
    // Subscribed only now, so the replaced stream's STREAM_DELETE isn't taken for ours.
    let mut dispatches = shared.gateway.dispatches.subscribe();
    if role.broadcasts() {
        let kind = if shared.guild_id.is_some() { "guild" } else { "call" };
        let create = json!({ "type": kind, "guild_id": shared.guild_id, "channel_id": shared.channel_id, "preferred_region": null });
        shared.gateway.send(18, create);
    } else {
        shared.gateway.send(20, json!({ "stream_key": key }));
    }
    let (reason, stopped) = tokio::select! {
        result = connect_stream(&shared, role, &streamer, &key, session_id, &mut dispatches) => (result.err().unwrap_or_else(|| "Stream connection closed".to_owned()), false),
        _ = stop => ("Stopped".to_owned(), true),
    };
    shared.gateway.send(19, json!({ "stream_key": key }));
    // A stream that ended on its own was already deleted; one we stopped is gone once Discord says so.
    if stopped && role.broadcasts() {
        let _ = tokio::time::timeout(DELETE_TIMEOUT, deleted(&key, &mut dispatches)).await;
    }
    let _ = gone.send(());
    shared.emit(VoiceEvent::StreamEnded { user_id: streamer.clone(), reason });
    // After the end, so the app still sees what ended it.
    let ended: &[u16] = if role.broadcasts() { &issues::BROADCAST_ISSUES } else { &issues::WATCH_ISSUES };
    for code in ended {
        shared.issue(&streamer, *code, false);
    }
}

// The next VOICE_* / STREAM_* dispatch; None once the gateway handle is gone. A receiver that fell
// behind skips what it missed instead of failing the join or stream waiting on it.
pub(super) async fn next_dispatch(dispatches: &mut broadcast::Receiver<(String, Value)>) -> Option<(String, Value)> {
    loop {
        match dispatches.recv().await {
            Ok(dispatch) => return Some(dispatch),
            Err(broadcast::error::RecvError::Lagged(missed)) => eprintln!("voice: missed {missed} gateway dispatches"),
            Err(broadcast::error::RecvError::Closed) => return None,
        }
    }
}

pub(super) async fn deleted(key: &str, dispatches: &mut broadcast::Receiver<(String, Value)>) {
    while let Some((kind, d)) = next_dispatch(dispatches).await {
        if kind == "STREAM_DELETE" && d["stream_key"].as_str() == Some(key) {
            return;
        }
    }
}

pub(super) async fn connect_stream(
    shared: &Arc<Shared>,
    role: Role,
    streamer: &str,
    key: &str,
    session_id: String,
    dispatches: &mut broadcast::Receiver<(String, Value)>,
) -> Result<(), String> {
    let (rtc, server) = tokio::time::timeout(SERVER_TIMEOUT, await_stream_server(key, dispatches))
        .await
        .map_err(|_| "Stream server did not respond".to_owned())??;
    match role.broadcasts() {
        true => shared.gateway.send(22, json!({ "stream_key": key, "paused": false })),
        false => shared.emit(VoiceEvent::StreamCreated { user_id: streamer.to_owned() }),
    }
    let mut target = Target { server_id: rtc.server_id, dave_channel: rtc.channel_id, session_id, server, role, streamer: streamer.to_owned() };
    let moved = Mutex::new(None);
    let on_dispatch = |kind: &str, d: &Value| stream_dispatch(key, kind, d, &moved);
    let mut backoff = Backoff::connection();
    // As the voice connection: a lost stream connection identifies again, forever, until STREAM_DELETE.
    loop {
        let reason = match serve(shared, target.clone(), dispatches, on_dispatch, &mut backoff).await {
            Ended::Dispatch(reason) => reason,
            Ended::Lost(lost) => {
                debug_lost(&target, &lost);
                shared.gateway.send(21, json!({ "stream_key": key }));
                match wait_out(backoff.fail(), dispatches, on_dispatch).await {
                    None => continue,
                    Some(reason) => reason,
                }
            }
        };
        let Some(server) = moved.lock().ok().and_then(|mut m| m.take()) else { return Err(reason) };
        target.server = server;
    }
}

// What ends a stream's connection: its delete, a new server for it (`moved`), or a fresh main session.
pub(super) fn stream_dispatch(key: &str, kind: &str, d: &Value, moved: &Mutex<Option<Server>>) -> Option<String> {
    if kind == "READY" {
        return Some(NEW_SESSION.into());
    }
    if d["stream_key"].as_str() != Some(key) {
        return None;
    }
    match kind {
        "STREAM_DELETE" => Some(delete_reason(d)),
        "STREAM_SERVER_UPDATE" => {
            let server = parse_server(d)?;
            *moved.lock().ok()? = Some(server);
            Some("Stream server moved".into())
        }
        _ => None,
    }
}

pub(super) fn delete_reason(d: &Value) -> String {
    match d["unavailable"].as_bool() {
        Some(true) => STREAM_DELETE_UNAVAILABLE.to_owned(),
        _ => d["reason"].as_str().unwrap_or(STREAM_DELETE_ENDED).to_owned(),
    }
}

#[derive(Clone)]
pub(super) struct RtcIds {
    pub(super) server_id: String,
    pub(super) channel_id: String,
}

pub(super) async fn await_stream_server(key: &str, dispatches: &mut broadcast::Receiver<(String, Value)>) -> Result<(RtcIds, Server), String> {
    let (mut rtc, mut server) = (None, None);
    loop {
        let (kind, d) = next_dispatch(dispatches).await.ok_or("Gateway closed")?;
        if kind == "READY" {
            return Err(NEW_SESSION.into());
        }
        if d["stream_key"].as_str() != Some(key) {
            continue;
        }
        match kind.as_str() {
            "STREAM_CREATE" => {
                if let (Some(server_id), Some(channel_id)) = (d["rtc_server_id"].as_str(), d["rtc_channel_id"].as_str()) {
                    rtc = Some(RtcIds { server_id: server_id.to_owned(), channel_id: channel_id.to_owned() });
                }
            }
            "STREAM_SERVER_UPDATE" => server = parse_server(&d).or(server),
            "STREAM_DELETE" => return Err(d["reason"].as_str().unwrap_or("Stream unavailable").to_owned()),
            _ => {}
        }
        if let (Some(rtc), Some(server)) = (&rtc, &server) {
            return Ok((rtc.clone(), server.clone()));
        }
    }
}

// Round-trip times from heartbeat acks.
#[derive(Default)]
pub(super) struct Pings(pub(super) VecDeque<u32>);

impl Pings {
    pub(super) fn record(&mut self, sent_ms: i64) {
        let rtt = (chrono::Utc::now().timestamp_millis() - sent_ms).clamp(0, u32::MAX as i64) as u32;
        self.0.push_back(rtt);
        if self.0.len() > PINGS_KEPT {
            self.0.pop_front();
        }
    }

    pub(super) fn stats(&self) -> (u32, u32, Vec<u32>) {
        let last = self.0.back().copied().unwrap_or(0);
        (last, median(self.0.iter().copied()), self.0.iter().copied().collect())
    }
}

pub(super) fn median(values: impl Iterator<Item = u32>) -> u32 {
    let mut sorted: Vec<u32> = values.collect();
    sorted.sort_unstable();
    match sorted.len() {
        0 => 0,
        n if n % 2 == 1 => sorted[n / 2],
        n => (sorted[n / 2 - 1] + sorted[n / 2]) / 2,
    }
}

// The server's outbound-loss reports over the last LOSS_WINDOW stats ticks, averaged.
#[derive(Default)]
pub(super) struct LossWindow(pub(super) VecDeque<f32>);

impl LossWindow {
    pub(super) fn push(&mut self, report: Option<f32>) -> Option<f32> {
        if let Some(loss) = report {
            self.0.push_back(loss);
            if self.0.len() > LOSS_WINDOW {
                self.0.pop_front();
            }
        }
        (!self.0.is_empty()).then(|| self.0.iter().sum::<f32>() / self.0.len() as f32)
    }
}

// Why a connection ended: a dispatch `on_dispatch` ended it on, or the connection was lost past resuming.
pub(super) enum Ended {
    Dispatch(String),
    Lost(String),
}

impl From<String> for Ended {
    fn from(lost: String) -> Self {
        Ended::Lost(lost)
    }
}

impl From<&str> for Ended {
    fn from(lost: &str) -> Self {
        Ended::Lost(lost.to_owned())
    }
}

// One voice-gateway connection: the channel's own (audio + cameras) or a Go Live stream's. It resumes
// what it can; `backoff` restarts once media is up.
pub(super) async fn serve(
    shared: &Arc<Shared>,
    target: Target,
    dispatches: &mut broadcast::Receiver<(String, Value)>,
    on_dispatch: impl Fn(&str, &Value) -> Option<String>,
    backoff: &mut Backoff,
) -> Ended {
    match connection(shared, target, dispatches, on_dispatch, backoff).await {
        Ok(never) => match never {},
        Err(ended) => ended,
    }
}

async fn connection(
    shared: &Arc<Shared>,
    target: Target,
    dispatches: &mut broadcast::Receiver<(String, Value)>,
    on_dispatch: impl Fn(&str, &Value) -> Option<String>,
    backoff: &mut Backoff,
) -> Result<std::convert::Infallible, Ended> {
    let is_voice = target.role == Role::Voice;
    let stage = |stage| {
        if is_voice {
            shared.emit(VoiceEvent::Stage(stage));
        }
    };
    let host = target.server.endpoint.trim_start_matches("wss://").trim_end_matches('/').to_owned();
    let url = format!("wss://{host}/?v=8");
    let ws = crate::discord::transport::websocket(&url).await?;
    let (mut sink, mut stream) = ws.split();
    let (out, mut out_rx) = mpsc::unbounded::<WsMessage>();

    let my_id: u64 = shared.user_id.parse().map_err(|_| "Invalid user id".to_owned())?;
    let channel: u64 = target.dave_channel.parse().map_err(|_| "Invalid channel id".to_owned())?;
    let mut dave = dave::Dave::new(my_id, channel, out.clone())?;
    let mut media: Option<media::Media> = None;
    let users = media::Users::default();
    let counters = Arc::new(media::Counters::default());
    let mut ready: Option<(Ssrcs, std::net::UdpSocket)> = None;
    let mut seq_ack: i64 = -1;
    // Until HELLO (op 8) sets the real beat, this is how long the server gets to send it (Discord's 20 s).
    let mut heartbeat = tokio::time::interval(HELLO_TIMEOUT);
    heartbeat.tick().await;
    let mut stats_tick = tokio::time::interval(STATS_EVERY);
    let mut pings = Pings::default();
    let mut loss = LossWindow::default();
    let mut stats = Stats { endpoint: host.clone(), ..Stats::default() };
    let mut resumes = 0;
    let mut senders = wants::VideoSsrcs::new();
    let mut socket_backoff = Backoff::socket();
    let mut acked = false;
    let mut last_ack = Instant::now();

    let identify = json!({ "op": 0, "d": {
        "server_id": target.server_id,
        "channel_id": target.dave_channel,
        "user_id": shared.user_id,
        "session_id": target.session_id,
        "token": target.server.token,
        "video": true,
        // Every Go Live connection, a viewer's too, describes a screen stream.
        "streams": [{ "type": if is_voice { "video" } else { "screen" }, "rid": "100", "quality": 100 }],
        "max_dave_protocol_version": davey::DAVE_PROTOCOL_VERSION,
    }});
    sink.send(WsMessage::Text(identify.to_string())).await.map_err(|e| e.to_string())?;
    stage(Stage::Authenticating);

    loop {
        // An error and the close code it came with, if any.
        let polled: Result<Option<WsMessage>, (String, Option<u16>)> = tokio::select! {
            _ = heartbeat.tick() => match acked {
                // A beat still unacked when the next is due: the socket is dead (Discord's 4800).
                false => Err(("Heartbeat timeout".to_owned(), None)),
                true => {
                    acked = false;
                    sink.send(beat(seq_ack)).await.map(|()| None).map_err(|e| (e.to_string(), None))
                }
            },
            _ = stats_tick.tick(), if is_voice && media.is_some() => {
                (stats.ping_ms, stats.median_ping_ms, stats.last_pings) = pings.stats();
                stats.outbound_loss_pct = loss.push(counters.outbound_loss());
                stats.dave = dave.e2ee().load(Ordering::Relaxed);
                let playout = shared.mixer.playout_stats();
                stats.playout_delay_ms = playout.iter().map(|(_, p)| p.target_ms).max();
                if debug() {
                    for (key, p) in &playout {
                        eprintln!(
                            "[playout {key:x}] target {} ms, buffered {} ms, underruns {}, concealed {}, fec {}, late {}, dropped {}",
                            p.target_ms, p.buffered_ms, p.underruns, p.concealed, p.fec, p.late, p.dropped
                        );
                    }
                }
                shared.emit(VoiceEvent::Stats(stats.clone()));
                Ok(None)
            }
            Some(msg) = out_rx.next() => sink.send(msg).await.map(|()| None).map_err(|e| (e.to_string(), None)),
            () = shared.wants_changed.notified(), if is_voice && media.is_some() => {
                sink.send(sink_wants(shared, &target, &senders)).await.map(|()| None).map_err(|e| (e.to_string(), None))
            }
            Some((kind, d)) = next_dispatch(dispatches) => match on_dispatch(&kind, &d) {
                Some(reason) => return Err(Ended::Dispatch(reason)),
                None => Ok(None),
            },
            frame = stream.next() => match frame {
                None => Err(("Voice connection lost".to_owned(), None)),
                Some(Err(e)) => Err((e.to_string(), None)),
                Some(Ok(WsMessage::Close(close))) => Err((close_reason(close.as_ref()), close.map(|c| u16::from(c.code)))),
                Some(Ok(frame)) => Ok(Some(frame)),
            },
        };
        let frame = match polled {
            Ok(Some(frame)) => frame,
            Ok(None) => continue,
            Err((lost, code)) => {
                stage(Stage::Authenticating);
                let resume = WsMessage::Text(resume_payload(&target, seq_ack).to_string());
                (sink, stream) = loop {
                    if reconnect(code, resumes, media.is_some(), last_ack.elapsed()) == Reconnect::Identify {
                        return Err(Ended::Lost(lost));
                    }
                    resumes += 1;
                    tokio::time::sleep(socket_backoff.fail()).await;
                    let Ok(mut ws) = crate::discord::transport::websocket(&url).await else { continue };
                    if ws.send(resume.clone()).await.is_ok() {
                        break ws.split();
                    }
                };
                acked = true;
                continue;
            }
        };

        let payload = match frame {
            WsMessage::Binary(bytes) => {
                let Some((seq, op, body)) = dave::parse_binary(&bytes) else { continue };
                if debug() {
                    eprintln!("[{:?}] voice binary op {op} ({} bytes)", target.role, body.len());
                }
                seq_ack = seq as i64;
                dave.handle_binary(op, body);
                continue;
            }
            WsMessage::Text(text) => serde_json::from_str::<Value>(&text).map_err(|e| e.to_string())?,
            _ => continue,
        };
        if let Some(seq) = payload["seq"].as_i64() {
            seq_ack = seq;
        }
        let d = &payload["d"];
        if debug() && payload["op"] != 6 {
            eprintln!("[{:?}] voice op {}: {:.300}", target.role, payload["op"], d.to_string());
        }
        match payload["op"].as_u64() {
            Some(8) => {
                let ms = d["heartbeat_interval"].as_f64().unwrap_or(13_750.0) as u64;
                heartbeat = tokio::time::interval(Duration::from_millis(ms));
                heartbeat.tick().await;
                acked = true;
            }
            // The server asking for a beat now.
            Some(3) => {
                acked = false;
                sink.send(beat(seq_ack)).await.map_err(|e| e.to_string())?;
            }
            Some(9) => {
                resumes = 0;
                socket_backoff.succeed();
                if media.is_some() {
                    stage(Stage::Connected);
                }
            }
            Some(6) => {
                acked = true;
                last_ack = Instant::now();
                if let Some(sent) = d["t"].as_i64() {
                    pings.record(sent);
                    // Discord rates a connection bad past 500 ms or 10% loss of what it sends (2006).
                    if matches!(target.role, Role::Broadcast(..)) {
                        let ping = pings.0.back().copied().unwrap_or(0);
                        let bad = ping > 500 || counters.outbound_loss().is_some_and(|loss| loss > issues::LOSS_LINE);
                        shared.issue(&shared.user_id, issues::STREAM_SEND_NETWORK_QUALITY, bad);
                    }
                    if debug() {
                        eprintln!("[{:?}] heartbeat ack after {} ms", target.role, pings.0.back().copied().unwrap_or(0));
                    }
                }
            }
            Some(2) => {
                socket_backoff.succeed();
                stage(Stage::RtcConnecting);
                let ssrc = d["ssrc"].as_u64().ok_or("Voice ready without ssrc")? as u32;
                let ip = d["ip"].as_str().ok_or("Voice ready without ip")?.to_owned();
                let port = d["port"].as_u64().ok_or("Voice ready without port")? as u16;
                stats.server_ip = format!("{ip}:{port}");
                let modes: Vec<&str> = d["modes"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                let mode = rtp::MODES.into_iter().find(|m| modes.contains(m)).ok_or("No supported encryption mode")?;
                stage(Stage::IceChecking);
                let discovered = tokio::task::spawn_blocking(move || media::discover(&ip, port, ssrc)).await.map_err(|e| e.to_string())?;
                let (socket, (my_ip, my_port)) = match discovered {
                    Ok(found) => found,
                    // Discord tells the server once (op 32) and keeps showing No Route while it retries.
                    Err(lost) => {
                        stage(Stage::NoRoute);
                        if backoff.is_fresh() {
                            let _ = sink.send(WsMessage::Text(json!({ "op": 32, "d": {} }).to_string())).await;
                        }
                        return Err(Ended::Lost(format!("{NO_ROUTE}: {lost}")));
                    }
                };
                let video = &d["streams"][0];
                let ssrcs = Ssrcs {
                    audio: ssrc,
                    video: video["ssrc"].as_u64().unwrap_or(0) as u32,
                    rtx: video["rtx_ssrc"].as_u64().unwrap_or(0) as u32,
                };
                ready = Some((ssrcs, socket));
                sink.send(WsMessage::Text(select_protocol(&my_ip, my_port, mode).to_string())).await.map_err(|e| e.to_string())?;
                stage(Stage::DtlsConnecting);
            }
            Some(4) => {
                let (ssrcs, socket) = ready.take().ok_or("Session description before ready")?;
                let key: Vec<u8> = d["secret_key"].as_array().into_iter().flatten().filter_map(|b| b.as_u64().map(|b| b as u8)).collect();
                let mode = d["mode"].as_str().unwrap_or_default();
                stats.encryption = mode.to_owned();
                let cipher = rtp::Cipher::new(mode, &key).ok_or("Server picked an unsupported mode")?;
                dave.set_protocol_version(d["dave_protocol_version"].as_u64().unwrap_or(0) as u16);
                if let Role::Broadcast(_, options) = target.role {
                    sink.send(WsMessage::Text(announce_video(&ssrcs, VideoKind::Screen(options), true).to_string())).await.map_err(|e| e.to_string())?;
                }
                media = Some(media::Media::start(
                    media::Link {
                        socket,
                        ssrc: ssrcs.audio,
                        video_ssrc: ssrcs.video,
                        rtx_ssrc: ssrcs.rtx,
                        cipher,
                        dave: dave.session(),
                        e2ee: dave.e2ee(),
                        shared: shared.clone(),
                        role: target.role,
                        my_id: shared.user_id.clone(),
                        events: shared.events.clone(),
                        ws: out.clone(),
                        nonce: Default::default(),
                        keyframe: AtomicBool::new(true),
                        counters: counters.clone(),
                        connection: media::connection_id(),
                    },
                    users.clone(),
                )?);
                sink.send(sink_wants(shared, &target, &senders)).await.map_err(|e| e.to_string())?;
                backoff.succeed();
                if is_voice {
                    stage(Stage::Connected);
                    shared.emit(VoiceEvent::Connected);
                } else if target.role == Role::Watch {
                    shared.emit(VoiceEvent::StreamConnected { user_id: target.streamer.clone() });
                }
            }
            Some(5) => {
                if let (Some(user), Some(ssrc)) = (d["user_id"].as_str(), d["ssrc"].as_u64()) {
                    media::map_ssrc(&users, ssrc as u32, user);
                }
            }
            Some(12) => {
                if let Some(user) = d["user_id"].as_str() {
                    video_ssrcs(d).for_each(|ssrc| media::map_ssrc(&users, ssrc, user));
                    senders.insert(user.to_owned(), wants::video_ssrcs(d));
                    // A camera that just came on gets its overrides (focused, disabled) too.
                    if is_voice && media.is_some() {
                        sink.send(sink_wants(shared, &target, &senders)).await.map_err(|e| e.to_string())?;
                    }
                }
            }
            Some(11) => dave.users_joined(d["user_ids"].as_array().into_iter().flatten().filter_map(Value::as_str)),
            Some(13) => {
                if let Some(user) = d["user_id"].as_str() {
                    senders.remove(user);
                    dave.user_left(user);
                    if let Some(media) = &media {
                        media.user_left(user);
                    }
                }
            }
            Some(op @ (21 | 22 | 24)) => dave.handle_json(op, d),
            _ => {}
        }
    }
}

// Op 15: the video this connection asks for. A Go Live connection carries one picture, wanted whole.
fn sink_wants(shared: &Shared, target: &Target, senders: &wants::VideoSsrcs) -> WsMessage {
    let d = match target.role {
        Role::Voice => shared.video_wants.lock().map(|w| wants::payload(&w, senders)).unwrap_or_else(|_| json!({ "any": 100 })),
        _ => json!({ "any": 100 }),
    };
    WsMessage::Text(json!({ "op": 15, "d": d }).to_string())
}

fn beat(seq_ack: i64) -> WsMessage {
    WsMessage::Text(json!({ "op": 3, "d": { "t": chrono::Utc::now().timestamp_millis(), "seq_ack": seq_ack } }).to_string())
}

pub(super) fn resume_payload(target: &Target, seq_ack: i64) -> Value {
    json!({ "op": 7, "d": {
        "server_id": target.server_id,
        "channel_id": target.dave_channel,
        "session_id": target.session_id,
        "token": target.server.token,
        "seq_ack": seq_ack,
    }})
}

pub(super) fn close_reason(close: Option<&tungstenite::protocol::CloseFrame>) -> String {
    match close {
        None => "Voice connection lost".to_owned(),
        Some(c) => {
            let code = u16::from(c.code);
            let reason = close_label(code).map(str::to_owned).unwrap_or_else(|| if c.reason.is_empty() { "Voice connection lost".to_owned() } else { c.reason.to_string() });
            format!("{reason} ({code})")
        }
    }
}

// WAFFLE_VOICE_DEBUG=1 logs voice-gateway traffic and unroutable UDP packets to stderr.
pub(crate) fn debug() -> bool {
    static DEBUG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DEBUG.get_or_init(|| std::env::var_os("WAFFLE_VOICE_DEBUG").is_some())
}

pub(super) struct Ssrcs {
    pub(super) audio: u32,
    pub(super) video: u32,
    pub(super) rtx: u32,
}

// What our outgoing video is, for op 12.
#[derive(Clone, Copy)]
pub(super) enum VideoKind {
    Camera,
    Screen(GoLiveOptions),
}

// Op 12 tells the SFU and viewers which SSRC carries our video and its limits. Shape follows
// what official clients send — Not verified.
pub(super) fn announce_video(ssrcs: &Ssrcs, kind: VideoKind, active: bool) -> Value {
    let (kind, (width, height), fps) = match kind {
        VideoKind::Camera => ("video", media::CAMERA_SIZE, media::CAMERA_FPS),
        VideoKind::Screen(options) => ("screen", media::go_live_size(options.resolution), options.fps),
    };
    json!({ "op": 12, "d": {
        "audio_ssrc": ssrcs.audio,
        "video_ssrc": if active { ssrcs.video } else { 0 },
        "rtx_ssrc": if active { ssrcs.rtx } else { 0 },
        "streams": [{
            "type": kind, "rid": "100", "ssrc": ssrcs.video, "rtx_ssrc": ssrcs.rtx, "active": active, "quality": 100,
            "max_bitrate": media::video_bitrate(width, height, fps), "max_framerate": fps,
            "max_resolution": { "type": "fixed", "width": width, "height": height },
        }],
    }})
}

// We decode only H.264, so it is the only video codec offered.
pub(super) fn select_protocol(address: &str, port: u16, mode: &str) -> Value {
    json!({ "op": 1, "d": {
        "protocol": "udp",
        "data": { "address": address, "port": port, "mode": mode },
        "codecs": [
            { "name": "opus", "type": "audio", "priority": 1000, "payload_type": rtp::OPUS_PAYLOAD_TYPE },
            { "name": "H264", "type": "video", "priority": 1000, "payload_type": rtp::H264_PAYLOAD_TYPE,
              "rtx_payload_type": rtp::H264_RTX_PAYLOAD_TYPE, "encode": true, "decode": true },
        ],
    }})
}

// Op 12 names a user's audio, video, retransmission and per-stream SSRCs; 0 means "none".
pub(super) fn video_ssrcs(d: &Value) -> impl Iterator<Item = u32> + '_ {
    let streams = d["streams"].as_array().into_iter().flatten().flat_map(|s| [&s["ssrc"], &s["rtx_ssrc"]]);
    [&d["audio_ssrc"], &d["video_ssrc"], &d["rtx_ssrc"]]
        .into_iter()
        .chain(streams)
        .filter_map(Value::as_u64)
        .filter(|&ssrc| ssrc != 0)
        .map(|ssrc| ssrc as u32)
}
