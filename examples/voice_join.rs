// Live check for src/voice: joins a voice channel with the token Waffle stored in the Keychain,
// prints voice events, and leaves after the given number of seconds.
//   cargo run --example voice_join -- <guild_id> <channel_id> [seconds] [--go-live]
// `cargo test --example voice_join` runs the voice module's unit tests.

// The app is the real caller of the whole API; this harness only touches part of it.
#[path = "../src/voice/mod.rs"]
#[allow(dead_code)]
mod voice;

mod discord {
    use std::sync::OnceLock;

    use tokio::runtime::Runtime;

    pub fn runtime() -> &'static Runtime {
        static RUNTIME: OnceLock<Runtime> = OnceLock::new();
        RUNTIME.get_or_init(|| Runtime::new().expect("tokio runtime"))
    }
}

use std::time::Duration;

use futures::{SinkExt, StreamExt, channel::mpsc};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message as WsMessage;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [guild_id, channel_id, rest @ ..] = args.as_slice() else {
        return Err("usage: voice_join <guild_id> <channel_id> [seconds]".into());
    };
    let seconds: u64 = rest.iter().find_map(|s| s.parse().ok()).unwrap_or(30);
    let auto_go_live = rest.iter().any(|s| s == "--go-live");
    let token = keyring::Entry::new("waffle", "discord-token")
        .and_then(|e| e.get_password())
        .map_err(|_| "No token in Keychain (sign in with Waffle first)")?;

    let (gateway, outbound) = voice::GatewayHandle::new();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let main = main_gateway(token, gateway.clone(), outbound, channel_id.clone(), ready_tx);
    discord::runtime().spawn(async move {
        if let Err(e) = main.await {
            eprintln!("main gateway ended: {e}");
        }
    });
    let (user_id, mut live_users) = ready_rx.recv_timeout(Duration::from_secs(15)).map_err(|_| "Main gateway never sent READY")?;
    println!("signed in as {user_id}; live in this channel: {live_users:?}");

    let me = user_id.clone();
    let (session, mut events) = voice::join(user_id, Some(guild_id.clone()), channel_id.clone(), gateway, voice::VoiceSettings::default());
    println!("keys + Enter: m mute, u unmute, d deafen, e undeafen, p push-to-talk, t hold talk key, r release,");
    println!("              w <user_id> watch their Go Live, x stop watching, g go live (whole screen), s stop going live,");
    println!("              l list windows, ga <pid> go live with one app (its audio only), gw <window_id> go live with one window,");
    println!("              glo <480|720|1080|0> <15|30|60> go live with options, devices, cam <id>, cam off");
    let mut watching: Option<voice::StreamWatch> = None;
    let mut live: Option<voice::GoLive> = None;
    let mut frames = 0u64;
    let (keys_tx, mut keys) = mpsc::unbounded::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lines().map_while(Result::ok) {
            let _ = keys_tx.unbounded_send(line.trim().to_owned());
        }
    });
    discord::runtime().block_on(async {
        let deadline = tokio::time::sleep(Duration::from_secs(seconds));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                _ = &mut deadline => break,
                Some(key) = keys.next() => match key.as_str() {
                    "m" | "u" => session.set_flags(key == "m", false),
                    "d" | "e" => session.set_flags(key == "d", key == "d"),
                    "p" => session.set_input_mode(voice::InputMode::PushToTalk { release_delay_ms: 200 }),
                    "t" | "r" => session.set_talk_key(key == "t"),
                    "x" => watching = None,
                    "s" => live = None,
                    "g" => live = session.go_live().map_err(|e| println!("go live failed: {e}")).ok(),
                    "devices" => {
                        let show = |kind: &str, list: Vec<voice::Device>| {
                            list.iter().for_each(|d| println!("  {kind} {}{} — {}", d.id, if d.default { " (default)" } else { "" }, d.name));
                        };
                        show("input", voice::input_devices());
                        show("output", voice::output_devices());
                        show("camera", voice::cameras());
                    }
                    "cam off" => {
                        let _ = session.set_camera(None);
                    }
                    "l" => match voice::screens().and_then(|s| Ok((s, voice::shareable_windows()?))) {
                        Ok((screens, windows)) => {
                            screens.iter().for_each(|s| println!("  screen {} {} {}x{}", s.id, s.name, s.width, s.height));
                            windows.iter().for_each(|w| println!("  window {} pid {} {} — {}", w.id, w.pid, w.app_name, w.title));
                        }
                        Err(e) => println!("list failed: {e}"),
                    },
                    _ => {
                        let source = if let Some(pid) = key.strip_prefix("ga ") {
                            pid.trim().parse().ok().map(voice::ShareSource::App)
                        } else if let Some(id) = key.strip_prefix("gw ") {
                            id.trim().parse().ok().map(voice::ShareSource::Window)
                        } else {
                            None
                        };
                        if let Some(rest) = key.strip_prefix("glo ") {
                            let mut parts = rest.split_whitespace().filter_map(|p| p.parse().ok());
                            let options = voice::GoLiveOptions { resolution: parts.next().unwrap_or(720), fps: parts.next().unwrap_or(30), audio: true };
                            let main = voice::screens().ok().and_then(|s| s.first().map(|s| s.id)).unwrap_or_default();
                            live = session.go_live_with_options(voice::ShareSource::Screen(main), options, live.take()).map_err(|e| println!("go live failed: {e}")).ok();
                        } else if let Some(id) = key.strip_prefix("cam ") {
                            if let Err(e) = session.set_camera(Some(id.trim().to_owned())) {
                                println!("camera failed: {e}");
                            }
                        } else if let Some(source) = source {
                            live = session.go_live_with(source).map_err(|e| println!("go live failed: {e}")).ok();
                        } else if let Some(streamer) = key.strip_prefix("w ") {
                            match session.watch_stream(streamer.trim()) {
                                Ok(watch) => watching = Some(watch),
                                Err(e) => println!("watch failed: {e}"),
                            }
                        }
                    }
                },
                event = events.next() => match event {
                    Some(voice::VoiceEvent::Stage(stage)) => println!("stage {stage:?}"),
                    Some(voice::VoiceEvent::Stats(s)) => println!(
                        "stats ping {} ms (median {}), loss out {:?}%, {} {} {} dave={}",
                        s.ping_ms, s.median_ping_ms, s.outbound_loss_pct, s.endpoint, s.server_ip, s.encryption, s.dave
                    ),
                    Some(voice::VoiceEvent::InputLevel { .. }) => {}
                    Some(voice::VoiceEvent::Issue { user_id, code, active }) => println!("{user_id} issue {code} active={active}"),
                    Some(voice::VoiceEvent::StreamConnected { user_id }) => println!("{user_id} stream connected"),
                    Some(voice::VoiceEvent::Connected) => {
                        println!("connected");
                        if auto_go_live {
                            println!("going live");
                            live = session.go_live().map_err(|e| println!("go live failed: {e}")).ok();
                        }
                        if let Some(streamer) = live_users.pop() {
                            println!("auto-watching {streamer}");
                            watching = session.watch_stream(&streamer).map_err(|e| println!("watch failed: {e}")).ok();
                        }
                    }
                    Some(voice::VoiceEvent::Speaking { user_id, speaking }) => println!("{user_id} speaking={speaking}"),
                    Some(voice::VoiceEvent::VideoState { user_id, streaming, camera }) => {
                        println!("{user_id} streaming={streaming} camera={camera}");
                        if streaming && watching.is_none() && user_id != me {
                            println!("auto-watching {user_id}");
                            watching = session.watch_stream(&user_id).map_err(|e| println!("watch failed: {e}")).ok();
                        }
                    }
                    Some(voice::VoiceEvent::StreamFrame { user_id, source }) => {
                        let Some(frame) = session.take_frame(&user_id, source) else { continue };
                        frames += 1;
                        if frames == 1 {
                            save_ppm(&frame, "stream-frame.ppm");
                        }
                        if frames.is_multiple_of(150) {
                            save_ppm(&frame, "stream-frame-latest.ppm");
                        }
                        if frames == 1 || frames.is_multiple_of(60) {
                            println!("{source:?} frame #{frames} from {user_id}: {}x{}", frame.width, frame.height);
                        }
                    }
                    Some(voice::VoiceEvent::StreamEnded { user_id, reason }) => println!("stream from {user_id} ended: {reason}"),
                    Some(voice::VoiceEvent::Moved(channel)) => return println!("moved to {channel}"),
                    Some(voice::VoiceEvent::Disconnected(reason)) => return println!("disconnected: {reason}"),
                    None => return,
                },
            }
        }
        drop(watching);
        drop(live);
        session.leave();
        if let Some(voice::VoiceEvent::Disconnected(reason)) = events.next().await {
            println!("disconnected: {reason}");
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    });
    Ok(())
}

// Users already in Go Live in `channel_id`, from READY's guild voice states (legacy READY shape).
fn streamers(ready: &Value, channel_id: &str) -> Vec<String> {
    let states = ready["guilds"].as_array().into_iter().flatten().flat_map(|g| g["voice_states"].as_array().into_iter().flatten());
    states
        .filter(|v| v["channel_id"].as_str() == Some(channel_id) && v["self_stream"].as_bool() == Some(true))
        .filter_map(|v| v["user_id"].as_str().map(str::to_owned))
        .collect()
}

// A decoded frame as a viewable image (`open stream-frame.ppm`).
fn save_ppm(frame: &voice::VideoFrame, path: &str) {
    let mut ppm = format!("P6 {} {} 255\n", frame.width, frame.height).into_bytes();
    ppm.extend(frame.bgra.as_chunks::<4>().0.iter().flat_map(|px| [px[2], px[1], px[0]]));
    if std::fs::write(path, ppm).is_ok() {
        println!("saved frame to {path}");
    }
}

// The smallest main gateway that satisfies the hook voice needs: write `outbound`, feed `dispatch`.
async fn main_gateway(
    token: String,
    gateway: voice::GatewayHandle,
    mut outbound: mpsc::UnboundedReceiver<String>,
    channel_id: String,
    ready: std::sync::mpsc::Sender<(String, Vec<String>)>,
) -> Result<(), String> {
    let (ws, _) = tokio_tungstenite::connect_async("wss://gateway.discord.gg/?v=10&encoding=json").await.map_err(|e| e.to_string())?;
    let (mut sink, mut stream) = ws.split();
    let mut seq: Option<u64> = None;
    let mut heartbeat = tokio::time::interval(Duration::from_secs(3600));
    heartbeat.tick().await;
    loop {
        tokio::select! {
            _ = heartbeat.tick() => sink.send(WsMessage::Text(json!({ "op": 1, "d": seq }).to_string())).await.map_err(|e| e.to_string())?,
            Some(text) = outbound.next() => sink.send(WsMessage::Text(text)).await.map_err(|e| e.to_string())?,
            frame = stream.next() => {
                let text = match frame {
                    Some(Ok(WsMessage::Text(text))) => text,
                    Some(Ok(WsMessage::Close(close))) => return Err(format!("closed: {close:?}")),
                    Some(Ok(_)) => continue,
                    Some(Err(e)) => return Err(e.to_string()),
                    None => return Err("stream ended".into()),
                };
                let payload: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
                seq = payload["s"].as_u64().or(seq);
                match payload["op"].as_u64() {
                    Some(op @ (7 | 9)) => return Err(format!("gateway op {op}: {}", payload["d"])),
                    Some(10) => {
                        let ms = payload["d"]["heartbeat_interval"].as_u64().unwrap_or(41_250);
                        heartbeat = tokio::time::interval(Duration::from_millis(ms));
                        heartbeat.tick().await;
                        let identify = json!({ "op": 2, "d": {
                            "token": token,
                            "capabilities": 0,
                            "properties": { "os": std::env::consts::OS, "browser": "Waffle", "device": "" },
                            "compress": false,
                        }});
                        sink.send(WsMessage::Text(identify.to_string())).await.map_err(|e| e.to_string())?;
                    }
                    Some(0) => {
                        let kind = payload["t"].as_str().unwrap_or_default();
                        if kind == "READY" {
                            let me = payload["d"]["user"]["id"].as_str().unwrap_or_default().to_owned();
                            if std::env::var_os("WAFFLE_VOICE_DEBUG").is_some() {
                                let guilds = payload["d"]["guilds"].as_array().map_or(0, Vec::len);
                                let keys: Vec<&String> = payload["d"]["guilds"][0].as_object().map(|g| g.keys().collect()).unwrap_or_default();
                                eprintln!("READY: {guilds} guilds; first guild keys: {keys:?}");
                                let states = payload["d"]["guilds"].as_array().into_iter().flatten().flat_map(|g| g["voice_states"].as_array().into_iter().flatten());
                                for state in states.filter(|v| v["channel_id"].as_str() == Some(&channel_id)) {
                                    eprintln!("READY voice state: {:.300}", state.to_string());
                                }
                            }
                            let _ = ready.send((me, streamers(&payload["d"], &channel_id)));
                        }
                        gateway.dispatch(kind, &payload["d"]);
                    }
                    _ => {}
                }
            }
        }
    }
}
