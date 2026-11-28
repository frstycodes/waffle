use super::*;

pub(super) struct Outbound {
    pub(super) commands: mpsc::UnboundedReceiver<Value>,
    pub(super) voice: crate::voice::GatewayHandle,
    pub(super) voice_out: mpsc::UnboundedReceiver<String>,
    pub(super) subscriptions: pacing::SharedSubscriptions,
}

// A drop RESUMEs the session (its id, the URL READY named and the last sequence) so Discord replays
// what was missed and keeps everything bound to it, a voice connection included: a fresh IDENTIFY
// ends the old session, and the voice server closes with 4014. Only an invalid session, or one that
// fails to come back in a few tries, identifies anew.
pub(super) async fn run_gateway(token: String, tx: mpsc::UnboundedSender<Event>, mut outbound: Outbound) {
    load_build_number().await;
    let mut resume: Option<Resume> = None;
    let mut fails = 0u32;
    loop {
        let mut up = false;
        let reason = match session(&token, &tx, &mut outbound, &mut resume, &mut up).await {
            Ok(()) => return,
            Err(GatewayError::AuthFailed) => {
                let _ = tx.unbounded_send(Event::AuthFailed);
                return;
            }
            Err(GatewayError::Fatal(reason)) => {
                let _ = tx.unbounded_send(Event::Disconnected(reason));
                return;
            }
            Err(GatewayError::Dropped(reason)) => reason,
        };
        if tx.unbounded_send(Event::Disconnected(reason)).is_err() {
            return;
        }
        // Discord's gateway backoff: it restarts once a session is up (READY or RESUMED), and a session
        // that can't be resumed in a few tries is given up for a fresh IDENTIFY.
        fails = if up { 1 } else { fails + 1 };
        if fails > pacing::RESUME_FAILS {
            resume = None;
        }
        tokio::time::sleep(pacing::reconnect_wait(fails, rand::random::<f64>())).await;
    }
}

pub(super) struct Resume {
    pub(super) session_id: String,
    pub(super) url: String,
    pub(super) seq: Option<u64>,
}

impl Resume {
    pub(super) fn payload(&self, token: &str) -> String {
        json!({ "op": 6, "d": { "token": token, "session_id": self.session_id, "seq": self.seq } }).to_string()
    }
}

// What each gateway close code means for us, from Discord's close event code table (docs.discord.com,
// Opcodes and Status Codes; 4015/4016 from docs.discord.food). Anything else, a network drop included,
// resumes.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Close {
    Resume,
    // The session is gone: identify anew.
    Identify,
    // 4004: the token was refused.
    AuthFailed,
    // Reconnecting can't help (bot-only shard and intent codes, wrong API version): stop and say so.
    Fatal,
}

pub(super) fn close_kind(code: u16) -> Close {
    match code {
        4004 => Close::AuthFailed,
        // 4007 invalid seq and 4009 session timed out invalidate the session; 4015 too many sessions,
        // 4016 connection request canceled.
        4007 | 4009 | 4015 | 4016 => Close::Identify,
        4010..=4014 => Close::Fatal,
        _ => Close::Resume,
    }
}

pub(super) fn gateway_close_text(code: u16, reason: &str) -> String {
    let what = match code {
        4000 => "Unknown error",
        4001 => "Unknown opcode",
        4002 => "Decode error",
        4003 => "Not authenticated",
        4004 => "Authentication failed",
        4005 => "Already authenticated",
        4007 => "Invalid sequence",
        4008 => "Rate limited",
        4009 => "Session timed out",
        4010 => "Invalid shard",
        4011 => "Sharding required",
        4012 => "Invalid API version",
        4013 => "Invalid intents",
        4014 => "Disallowed intents",
        4015 => "Too many sessions",
        4016 => "Connection request canceled",
        _ if reason.is_empty() => "Connection closed",
        _ => reason,
    };
    format!("{what} ({code})")
}

pub(super) enum GatewayError {
    AuthFailed,
    Dropped(String),
    Fatal(String),
}

pub(super) async fn session(token: &str, tx: &mpsc::UnboundedSender<Event>, outbound: &mut Outbound, resume: &mut Option<Resume>, up: &mut bool) -> Result<(), GatewayError> {
    let dropped = |e: &dyn std::fmt::Display| GatewayError::Dropped(e.to_string());
    let url = resume.as_ref().map(|r| format!("{}/?v=10&encoding=json", r.url.trim_end_matches('/'))).unwrap_or_else(|| GATEWAY.to_owned());
    let ws = transport::websocket(&url).await.map_err(|e| {
        eprintln!("gateway: connect failed: {e}");
        dropped(&e)
    })?;
    let (mut sink, mut stream) = ws.split();
    let mut seq: Option<u64> = resume.as_ref().and_then(|r| r.seq);
    let mut heartbeat = tokio::time::interval(Duration::from_secs(3600));
    heartbeat.tick().await;
    // A beat with no ACK before the next one means a zombie socket (typically after sleep or a network
    // change): Discord's guidance is to drop it and reconnect rather than wait for TCP to notice.
    let mut acked = true;
    let mut greeted = false;
    let mut presence = pacing::Presence::default();

    loop {
        let presence_wake = presence.wake();
        tokio::select! {
            _ = tokio::time::sleep_until(presence_wake.unwrap_or_else(tokio::time::Instant::now)), if presence_wake.is_some() => {
                if let Some(update) = presence.release(tokio::time::Instant::now()) {
                    sink.send(WsMessage::Text(update.to_string())).await.map_err(|e| dropped(&e))?;
                }
            }
            _ = heartbeat.tick() => {
                if !acked {
                    return Err(GatewayError::Dropped("No heartbeat ACK".into()));
                }
                acked = false;
                let beat = json!({ "op": 1, "d": seq }).to_string();
                sink.send(WsMessage::Text(beat)).await.map_err(|e| dropped(&e))?;
            }
            // Held until IDENTIFY or RESUME is out: anything earlier is answered with a 4003 close.
            Some(command) = outbound.commands.next(), if greeted => {
                let command = match command["op"].as_u64() {
                    Some(3) => presence.offer(command, tokio::time::Instant::now()),
                    _ => Some(command),
                };
                if let Some(command) = command {
                    sink.send(WsMessage::Text(command.to_string())).await.map_err(|e| dropped(&e))?;
                }
            }
            Some(text) = outbound.voice_out.next(), if greeted => {
                sink.send(WsMessage::Text(text)).await.map_err(|e| dropped(&e))?;
            }
            frame = stream.next() => {
                let frame = match frame {
                    None => return Err(GatewayError::Dropped("Connection closed".into())),
                    Some(frame) => frame.map_err(|e| dropped(&e))?,
                };
                let text = match frame {
                    WsMessage::Text(text) => text,
                    WsMessage::Close(None) => return Err(GatewayError::Dropped("Connection closed".into())),
                    WsMessage::Close(Some(close)) => {
                        let code = u16::from(close.code);
                        let text = gateway_close_text(code, &close.reason);
                        eprintln!("gateway: closed {text}");
                        return Err(match close_kind(code) {
                            Close::AuthFailed => GatewayError::AuthFailed,
                            Close::Fatal => GatewayError::Fatal(text),
                            Close::Identify => {
                                *resume = None;
                                GatewayError::Dropped(text)
                            }
                            Close::Resume => GatewayError::Dropped(text),
                        });
                    }
                    _ => continue,
                };
                let payload: Value = serde_json::from_str(&text).map_err(|e| dropped(&e))?;
                if let Some(s) = payload["s"].as_u64() {
                    seq = Some(s);
                    if let Some(resume) = resume.as_mut() {
                        resume.seq = seq;
                    }
                }
                match payload["op"].as_u64() {
                    Some(10) => {
                        let ms = payload["d"]["heartbeat_interval"].as_u64().unwrap_or(41_250);
                        // The first beat waits a random share of the interval (Discord's jitter), then every interval.
                        let period = Duration::from_millis(ms);
                        let first = period.mul_f64(rand::random::<f64>());
                        heartbeat = tokio::time::interval_at(tokio::time::Instant::now() + first, period);
                        // After a sleep, beats missed while asleep would fire back to back and fail the ACK check.
                        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                        let hello = resume.as_ref().map(|r| r.payload(token)).unwrap_or_else(|| identify(token));
                        sink.send(WsMessage::Text(hello)).await.map_err(|e| dropped(&e))?;
                        greeted = true;
                    }
                    Some(11) => acked = true,
                    // The gateway asking for a beat now.
                    Some(1) => sink.send(WsMessage::Text(json!({ "op": 1, "d": seq }).to_string())).await.map_err(|e| dropped(&e))?,
                    Some(7) => return Err(GatewayError::Dropped("Gateway asked to reconnect".into())),
                    // Invalid session: `d` says whether it can still be resumed.
                    Some(9) => {
                        if payload["d"].as_bool() != Some(true) {
                            *resume = None;
                        }
                        return Err(GatewayError::Dropped("Session expired".into()));
                    }
                    Some(0) => {
                        let kind = payload["t"].as_str().unwrap_or("");
                        match kind {
                            "READY" => {
                                let d = &payload["d"];
                                // No resume URL: resume through the regular gateway.
                                let url = d["resume_gateway_url"].as_str().unwrap_or("wss://gateway.discord.gg").to_owned();
                                *resume = d["session_id"].as_str().map(|id| Resume { session_id: id.to_owned(), url, seq });
                                *up = true;
                                // A fresh session holds no subscriptions.
                                if let Ok(mut subscriptions) = outbound.subscriptions.lock() {
                                    subscriptions.clear();
                                }
                            }
                            "RESUMED" => {
                                *up = true;
                                _ = tx.unbounded_send(Event::Resumed);
                            }
                            _ => {}
                        }
                        outbound.voice.dispatch(kind, &payload["d"]);
                        let Some(event) = dispatch(kind, &payload["d"]) else { continue };
                        if tx.unbounded_send(event).is_err() {
                            return Ok(());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

pub(super) fn status_wire(status: Status) -> &'static str {
    match status {
        Status::Online => "online",
        Status::Idle => "idle",
        Status::Dnd => "dnd",
        Status::Offline => "invisible",
    }
}

// capabilities 0 asks for the legacy READY shape: full guild objects and user objects in DMs.
pub(super) fn identify(token: &str) -> String {
    json!({
        "op": 2,
        "d": {
            "token": token,
            "capabilities": 0,
            "properties": identity().properties(),
            "presence": { "status": "online", "since": 0, "activities": [], "afk": false },
            "compress": false,
        }
    })
    .to_string()
}

pub(super) fn dispatch(kind: &str, d: &Value) -> Option<Event> {
    let s = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    match kind {
        "READY" => Some(Event::Ready(Box::new(parse_ready(d)))),
        "USER_SETTINGS_UPDATE" => Some(Event::UserSettings(d.clone())),
        // Not verified: the payload shape `{settings: {type, proto}, partial}`, inferred from Discord's store handler.
        "USER_SETTINGS_PROTO_UPDATE" if d["settings"]["type"].as_u64() == Some(1) => {
            Some(Event::SettingsProto { proto: s(&d["settings"]["proto"]), partial: d["partial"].as_bool().unwrap_or(false) })
        }
        "GUILD_CREATE" => parse_guild(d).map(Event::GuildCreate),
        "THREAD_CREATE" if d["newly_created"].as_bool() == Some(true) => {
            let guild_id = d["guild_id"].as_str()?.to_owned();
            let channel = parse_channel(d, &guild_id)?;
            let created = Event::ForumPostCreated { forum_id: s(&d["parent_id"]), post_id: channel.id.clone(), owner_id: s(&d["owner_id"]) };
            Some(Event::Batch(vec![Event::ChannelUpsert { guild_id, channel }, created]))
        }
        // Not verified: `threads[].thread_id` and `threads[].count`; only the request's shape was read.
        "FORUM_UNREADS" => Some(Event::ForumUnreads {
            forum_id: s(&d["channel_id"]),
            counts: d["threads"].as_array().into_iter().flatten().filter_map(|t| Some((t["thread_id"].as_str()?.to_owned(), t["count"].as_u64().unwrap_or(0) as u32))).collect(),
        }),
        "CHANNEL_CREATE" | "CHANNEL_UPDATE" if d["guild_id"].is_null() && matches!(d["type"].as_u64(), Some(1 | 3)) => parse_dm(d, &Value::Null).map(Event::DmUpsert),
        "CHANNEL_DELETE" if d["guild_id"].is_null() => Some(Event::DmDelete(d["id"].as_str()?.to_owned())),
        "CHANNEL_RECIPIENT_ADD" | "CHANNEL_RECIPIENT_REMOVE" => Some(Event::RecipientChange { channel_id: d["channel_id"].as_str()?.to_owned(), user: parse_user(&d["user"]), added: kind == "CHANNEL_RECIPIENT_ADD" }),
        "CHANNEL_CREATE" | "CHANNEL_UPDATE" | "THREAD_CREATE" | "THREAD_UPDATE" => {
            let guild_id = d["guild_id"].as_str()?.to_owned();
            parse_channel(d, &guild_id).map(|channel| Event::ChannelUpsert { guild_id, channel })
        }
        "CHANNEL_DELETE" | "THREAD_DELETE" => Some(Event::ChannelDelete { guild_id: d["guild_id"].as_str()?.to_owned(), id: d["id"].as_str()?.to_owned() }),
        "GUILD_SCHEDULED_EVENT_CREATE" | "GUILD_SCHEDULED_EVENT_UPDATE" => parse_scheduled_event(d).map(Event::ScheduledEvent),
        // Not verified: the exception payload carries `guild_id` and `event_id` beside the exception's own fields.
        "GUILD_SCHEDULED_EVENT_EXCEPTION_CREATE" | "GUILD_SCHEDULED_EVENT_EXCEPTION_UPDATE" | "GUILD_SCHEDULED_EVENT_EXCEPTION_DELETE" => {
            let exception = parse_event_exception(d)?;
            Some(Event::ScheduledEventException {
                guild_id: d["guild_id"].as_str()?.to_owned(),
                event_id: d["event_id"].as_str()?.to_owned(),
                id: exception.id.clone(),
                exception: (kind != "GUILD_SCHEDULED_EVENT_EXCEPTION_DELETE").then_some(exception),
            })
        }
        "GUILD_SCHEDULED_EVENT_DELETE" => Some(Event::ScheduledEventDelete { guild_id: d["guild_id"].as_str()?.to_owned(), id: d["id"].as_str()?.to_owned() }),
        "GUILD_SCHEDULED_EVENT_USER_ADD" | "GUILD_SCHEDULED_EVENT_USER_REMOVE" => Some(Event::ScheduledEventUser {
            guild_id: s(&d["guild_id"]),
            event_id: s(&d["guild_scheduled_event_id"]),
            user_id: s(&d["user_id"]),
            added: kind == "GUILD_SCHEDULED_EVENT_USER_ADD",
        }),
        "GUILD_MEMBER_UPDATE" => Some(Event::MemberUpdate {
            guild_id: d["guild_id"].as_str()?.to_owned(),
            user: parse_member_user(d, &d["user"], d["guild_id"].as_str()),
            roles: strings(&d["roles"]),
            timed_out_until: timeout_end(d),
            membership: Membership::parse(d),
        }),
        "RELATIONSHIP_ADD" | "RELATIONSHIP_UPDATE" => {
            let kind = match d["type"].as_u64()? {
                1 => Relation::Friend,
                2 => Relation::Blocked,
                3 => Relation::Incoming,
                4 => Relation::Outgoing,
                _ => return None,
            };
            let user = if d["user"].is_object() { parse_user(&d["user"]) } else { User { id: d["id"].as_str()?.to_owned(), ..Default::default() } };
            Some(Event::RelationshipChange { user_id: d["id"].as_str()?.to_owned(), relationship: Some(Relationship { user, kind, since: d["since"].as_str().map(str::to_owned) }) })
        }
        "RELATIONSHIP_REMOVE" => Some(Event::RelationshipChange { user_id: d["id"].as_str()?.to_owned(), relationship: None }),
        "THREAD_MEMBER_UPDATE" => Some(Event::ThreadJoined { id: d["id"].as_str()?.to_owned(), joined: true }),
        "THREAD_LIST_SYNC" => {
            let guild_id = d["guild_id"].as_str()?.to_owned();
            // Sent with many threads at once; each becomes an upsert below via Batch.
            let threads: Vec<Event> = d["threads"].as_array().into_iter().flatten().filter_map(|t| parse_channel(t, &guild_id)).map(|channel| Event::ChannelUpsert { guild_id: guild_id.clone(), channel }).collect();
            Some(Event::Batch(threads))
        }
        "USER_GUILD_SETTINGS_UPDATE" => Some(Event::GuildSettings(parse_guild_settings(d))),
        "MESSAGE_CREATE" => parse_message(d).map(|m| Event::MessageCreate(Box::new(m))),
        "INTERACTION_CREATE" => Some(Event::InteractionCreate { nonce: d["nonce"].as_str()?.to_owned(), id: s(&d["id"]) }),
        "INTERACTION_SUCCESS" => Some(Event::InteractionSuccess { nonce: d["nonce"].as_str()?.to_owned() }),
        "INTERACTION_FAILURE" => Some(Event::InteractionFailure { nonce: d["nonce"].as_str()?.to_owned(), reason_code: d["reason_code"].as_u64() }),
        "INTERACTION_MODAL_CREATE" => crate::app_commands::parse_modal(d).map(Event::ModalCreate),
        "APPLICATION_COMMAND_AUTOCOMPLETE_RESPONSE" => Some(Event::Autocomplete { nonce: d["nonce"].as_str()?.to_owned(), choices: crate::app_commands::parse_choices(d) }),
        "MESSAGE_UPDATE" => Some(Event::MessageUpdate(d.clone())),
        "MESSAGE_DELETE" => Some(Event::MessageDelete { channel_id: s(&d["channel_id"]), id: s(&d["id"]) }),
        "MESSAGE_REACTION_ADD" | "MESSAGE_REACTION_REMOVE" => Some(Event::Reaction {
            channel_id: s(&d["channel_id"]),
            message_id: s(&d["message_id"]),
            user_id: s(&d["user_id"]),
            emoji: parse_emoji(&d["emoji"]),
            added: kind == "MESSAGE_REACTION_ADD",
        }),
        "MESSAGE_POLL_VOTE_ADD" | "MESSAGE_POLL_VOTE_REMOVE" => Some(Event::PollVote {
            channel_id: s(&d["channel_id"]),
            message_id: s(&d["message_id"]),
            user_id: s(&d["user_id"]),
            answer_id: d["answer_id"].as_u64()? as u32,
            added: kind == "MESSAGE_POLL_VOTE_ADD",
        }),
        "TYPING_START" => Some(Event::Typing {
            channel_id: s(&d["channel_id"]),
            user_id: s(&d["user_id"]),
            name: d["member"]["user"].is_object().then(|| parse_member_user(&d["member"], &d["member"]["user"], d["guild_id"].as_str()).name),
        }),
        "PRESENCE_UPDATE" => Some(Event::Presence { user_id: s(&d["user"]["id"]), presence: parse_presence(d) }),
        "GUILD_MEMBER_LIST_UPDATE" => Some(Event::MemberList {
            guild_id: s(&d["guild_id"]),
            // Group counts arrive here; the group items inside the list ops carry only ids.
            groups: d["groups"].as_array().into_iter().flatten().map(|g| (s(&g["id"]), g["count"].as_u64().unwrap_or(0) as u32)).collect(),
            ops: d["ops"].as_array().into_iter().flatten().filter_map(|op| parse_list_op(op, d["guild_id"].as_str())).collect(),
        }),
        "GUILD_MEMBERS_CHUNK" => Some(Event::MembersChunk {
            guild_id: s(&d["guild_id"]),
            members: d["members"].as_array().into_iter().flatten().map(|m| (parse_member_user(m, &m["user"], d["guild_id"].as_str()), strings(&m["roles"]))).collect(),
            facts: d["members"].as_array().into_iter().flatten().filter_map(MemberFacts::parse).collect(),
        }),
        "MESSAGE_ACK" => Some(Event::Ack {
            channel_id: s(&d["channel_id"]),
            message_id: snowflake(&d["message_id"]),
            manual: d["manual"].as_bool().unwrap_or(false).then(|| d["mention_count"].as_u64().unwrap_or(0) as u32),
        }),
        "VOICE_STATE_UPDATE" => Some(Event::VoiceState(parse_voice_state(d), d["session_id"].as_str().map(str::to_owned))),
        "STAGE_INSTANCE_CREATE" | "STAGE_INSTANCE_UPDATE" => Some(Event::StageInstance { guild_id: s(&d["guild_id"]), channel_id: s(&d["channel_id"]), instance: parse_stage_instance(d) }),
        "STAGE_INSTANCE_DELETE" => Some(Event::StageInstance { guild_id: s(&d["guild_id"]), channel_id: s(&d["channel_id"]), instance: None }),
        "CHANNEL_INFO" => {
            let channels = || d["channels"].as_array().into_iter().flatten();
            Some(Event::Batch(vec![
                Event::VoiceStartTimes { guild_id: s(&d["guild_id"]), times: channels().map(|c| (s(&c["id"]), c["voice_start_time"].as_u64())).collect(), all: true },
                Event::VoiceStatuses { guild_id: s(&d["guild_id"]), statuses: channels().map(|c| (s(&c["id"]), s(&c["status"]))).collect(), all: true },
            ]))
        }
        "VOICE_CHANNEL_STATUS_UPDATE" => Some(Event::VoiceStatuses { guild_id: s(&d["guild_id"]), statuses: vec![(s(&d["id"]), s(&d["status"]))], all: false }),
        "VOICE_CHANNEL_START_TIME_UPDATE" => Some(Event::VoiceStartTimes { guild_id: s(&d["guild_id"]), times: vec![(s(&d["id"]), d["voice_start_time"].as_u64())], all: false }),
        "STREAM_CREATE" | "STREAM_UPDATE" => Some(Event::StreamUpdate { key: s(&d["stream_key"]), viewers: strings(&d["viewer_ids"]), paused: d["paused"].as_bool().unwrap_or(false) }),
        "STREAM_DELETE" => Some(Event::StreamDelete { key: s(&d["stream_key"]) }),
        "VOICE_SERVER_UPDATE" => Some(Event::VoiceServer(d.clone())),
        "CALL_CREATE" | "CALL_UPDATE" => Some(Event::Call { channel_id: s(&d["channel_id"]), ringing: strings(&d["ringing"]) }),
        "CALL_DELETE" => Some(Event::CallEnded { channel_id: s(&d["channel_id"]) }),
        "EMBEDDED_ACTIVITY_UPDATE_V2" => parse_activity_instance(d).map(Event::ActivityInstance),
        // Activities already running when we connected.
        "READY_SUPPLEMENTAL" => Some(Event::Batch(
            d["guilds"].as_array().into_iter().flatten().flat_map(|g| g["activity_instances"].as_array().into_iter().flatten()).filter_map(parse_activity_instance).map(Event::ActivityInstance).collect(),
        )),
        "VOICE_CHANNEL_EFFECT_SEND" => parse_voice_effect(d).map(Event::VoiceEffect),
        // Not verified: the reply's shape, `{ guild_id, soundboard_sounds }`, is the public API's.
        "SOUNDBOARD_SOUNDS" | "GUILD_SOUNDBOARD_SOUNDS_UPDATE" => Some(Event::SoundboardSounds {
            guild_id: id_string(&d["guild_id"])?,
            sounds: d["soundboard_sounds"].as_array().into_iter().flatten().filter_map(SoundboardSound::parse).collect(),
            all: true,
        }),
        "GUILD_SOUNDBOARD_SOUND_CREATE" | "GUILD_SOUNDBOARD_SOUND_UPDATE" => {
            let sound = SoundboardSound::parse(d)?;
            Some(Event::SoundboardSounds { guild_id: sound.guild_id.clone(), sounds: vec![sound], all: false })
        }
        "GUILD_SOUNDBOARD_SOUND_DELETE" => Some(Event::SoundboardSoundDelete { guild_id: id_string(&d["guild_id"])?, sound_id: id_string(&d["sound_id"])? }),
        _ => None,
    }
}
