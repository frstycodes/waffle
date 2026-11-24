use super::*;

#[derive(Clone, Debug, Default)]
pub struct SearchPage {
    pub messages: Vec<Message>,
    pub total: u32,
}

// Native GPUI ships no HTTP client (only its web build has one), so `img(url)` loads nothing
// until the app installs one. GET-only: it serves avatars, icons and attachments from the CDN.
#[derive(Default)]
pub struct Images(reqwest::Client);

use crate::downloads;

impl gpui_kit::http_client::HttpClient for Images {
    fn user_agent(&self) -> Option<&gpui_kit::http_client::http::HeaderValue> {
        None
    }

    fn proxy(&self) -> Option<&gpui_kit::http_client::Url> {
        None
    }

    fn send(
        &self,
        req: gpui_kit::http_client::Request<gpui_kit::http_client::AsyncBody>,
    ) -> futures::future::BoxFuture<'static, anyhow::Result<gpui_kit::http_client::Response<gpui_kit::http_client::AsyncBody>>> {
        let url = req.uri().to_string();
        if let Some(bytes) = downloads::get(&url).or_else(|| demo_media(&url)) {
            return Box::pin(async move { Ok(gpui_kit::http_client::Response::builder().status(200).body(bytes.to_vec().into())?) });
        }
        let get = self.0.get(&url);
        Box::pin(async move {
            // The disk tier, read on the loader's thread, never the UI's.
            if let Some(bytes) = downloads::get_or_read(&url) {
                return Ok(gpui_kit::http_client::Response::builder().status(200).body(bytes.to_vec().into())?);
            }
            // gpui drops this future when the image cache lets go of a load; the download stops with it.
            let mut fetch = AbortOnDrop(runtime().spawn(async move {
                let res = get.send().await?;
                Ok::<_, reqwest::Error>((res.status().as_u16(), res.bytes().await?))
            }));
            let (status, bytes) = (&mut fetch.0).await??;
            if cfg!(debug_assertions) && std::env::var_os("WAFFLE_IMAGE_DEBUG").is_some() && status >= 400 {
                eprintln!("image {status} {url}");
            }
            if status == 200 {
                downloads::put(url, bytes.clone());
            }
            Ok(gpui_kit::http_client::Response::builder().status(status).body(bytes.to_vec().into())?)
        })
    }
}

pub(super) struct AbortOnDrop<T>(pub(super) tokio::task::JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

// Made-up media a WAFFLE_DEMO scene serves in place of the network (debug builds only).
pub static DEMO_MEDIA: OnceLock<fn(&str) -> Option<bytes::Bytes>> = OnceLock::new();

pub fn demo_media(url: &str) -> Option<bytes::Bytes> {
    if !cfg!(debug_assertions) {
        return None;
    }
    DEMO_MEDIA.get()?(url)
}

// Discord's HTTP failures (docs.discord.com, Opcodes and Status Codes, HTTP): 429 waits out
// `retry_after` and tries again; a 5xx is retried once, only for methods that are safe to repeat, so a
// message never sends twice; 401 means the token is no longer accepted. Every other failure shows
// Discord's own message (its JSON `message`, field errors included).
pub async fn send(req: transport::Request) -> Result<Value, String> {
    let idempotent = req.try_clone().and_then(|r| r.build().ok()).is_some_and(|r| matches!(*r.method(), transport::Method::GET | transport::Method::PUT | transport::Method::DELETE | transport::Method::HEAD));
    let mut req = req;
    let mut retries = 0;
    loop {
        // None for a streamed body (uploads): those aren't retried.
        let again = req.try_clone();
        let res = req.send().await.map_err(|e| e.to_string())?;
        let status = res.status();
        let url = res.uri().path().to_owned();
        let header_wait = res.headers().get(transport::header::RETRY_AFTER).and_then(|v| v.to_str().ok()?.parse::<f64>().ok());
        let body: Value = res.json().await.unwrap_or(Value::Null);
        if status.is_success() {
            return Ok(body);
        }
        if let (Some(wait), Some(next)) = (retry_wait(status.as_u16(), &body, header_wait, idempotent, retries), again) {
            eprintln!("http: {status} {url}, retrying in {wait:?}");
            tokio::time::sleep(wait).await;
            req = next;
            retries += 1;
            continue;
        }
        eprintln!("http: {status} {url} (code {}): {body}", body["code"]);
        return Err(error_text(status.as_u16(), &body));
    }
}

// Longer than this and the caller hears "rate limited" instead of a frozen action.
pub(super) const MAX_RETRY_WAIT: f64 = 10.;

pub(super) fn retry_wait(status: u16, body: &Value, header_wait: Option<f64>, idempotent: bool, retries: u32) -> Option<Duration> {
    match status {
        429 if retries < 2 => {
            // Discord's client reads the header first and waits 5s when neither says.
            let secs = header_wait.or(body["retry_after"].as_f64()).unwrap_or(5.);
            (secs <= MAX_RETRY_WAIT).then(|| Duration::from_secs_f64(secs.max(0.)))
        }
        // The gateway errors Discord's client retries; it leaves 500 and 503 to the caller.
        502 | 504 if idempotent && retries == 0 => Some(Duration::from_secs(1)),
        _ => None,
    }
}

// Discord's client tries a command index this many times before it gives up.
pub(super) const INDEX_TRIES: u32 = 3;

// A command index GET: 202 (Discord still building it) waits 5 s, 429 waits `retry_after`.
pub(super) async fn fetch_index(req: transport::Request) -> Result<Value, String> {
    let mut req = req;
    let mut tries = 1;
    loop {
        let again = req.try_clone();
        let res = req.send().await.map_err(|e| e.to_string())?;
        let status = res.status().as_u16();
        let header_wait = res.headers().get(transport::header::RETRY_AFTER).and_then(|v| v.to_str().ok()?.parse::<f64>().ok());
        let body: Value = res.json().await.unwrap_or(Value::Null);
        if let (Some(wait), Some(next)) = (index_wait(status, &body, header_wait).filter(|_| tries < INDEX_TRIES), again) {
            tokio::time::sleep(wait).await;
            req = next;
            tries += 1;
            continue;
        }
        return match status {
            // Not Discord's wording: its client shows no text for an index that never finished building.
            202 => Err("Commands are still loading. Try again in a moment.".into()),
            200..=299 => Ok(body),
            _ => Err(error_text(status, &body)),
        };
    }
}

pub(super) fn index_wait(status: u16, body: &Value, header_wait: Option<f64>) -> Option<Duration> {
    match status {
        202 => Some(Duration::from_secs(5)),
        429 => Some(Duration::from_secs_f64(body["retry_after"].as_f64().or(header_wait).unwrap_or(1.).max(0.))),
        _ => None,
    }
}

/// Why `POST /interactions` was refused, as Discord's client reads it (`web#964363`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InteractError {
    pub status: Option<u16>,
    // Discord's error code (`body.code`), its `errorCode`.
    pub code: Option<u64>,
    // The server's words, shown on a command's stand-in.
    pub message: Option<String>,
    // The command index is out of date: refetch it before the next command.
    pub stale_index: bool,
    // Seconds, on a 429.
    pub retry_after: Option<f64>,
}

pub(super) const INVALID_FORM_BODY: u64 = 50035;
// Not verified: Discord's UNKNOWN_INTEGRATION is believed to be API code 10005.
pub(super) const UNKNOWN_INTEGRATION: u64 = 10005;

pub(super) async fn send_interaction(req: transport::Request) -> Result<(), InteractError> {
    let res = req.send().await.map_err(|_| InteractError::default())?;
    let status = res.status().as_u16();
    let body: Value = res.json().await.unwrap_or(Value::Null);
    if (200..300).contains(&status) {
        return Ok(());
    }
    eprintln!("http: {status} /interactions (code {}): {body}", body["code"]);
    Err(interact_error(status, &body))
}

pub(super) fn interact_error(status: u16, body: &Value) -> InteractError {
    let code = body["code"].as_u64();
    let retry_after = body["retry_after"].as_f64();
    if !(400..500).contains(&status) || body.is_null() {
        return InteractError { status: Some(status), code, retry_after, ..Default::default() };
    }
    if code == Some(INVALID_FORM_BODY) && body["errors"].is_object() {
        let first = first_form_error(&body["errors"]);
        let stale = first.is_some_and(|e| matches!(e["code"].as_str(), Some("INTERACTION_APPLICATION_COMMAND_INVALID_VERSION" | "INTERACTION_APPLICATION_COMMAND_INVALID")));
        return InteractError { status: Some(status), code: None, message: first.and_then(|e| e["message"].as_str()).map(str::to_owned), stale_index: stale, retry_after };
    }
    let message = body["message"].as_str().map(str::to_owned);
    if code == Some(UNKNOWN_INTEGRATION) {
        return InteractError { status: Some(status), code: None, message, stale_index: true, retry_after };
    }
    InteractError { status: Some(status), code, message, stale_index: false, retry_after }
}

// The first `_errors[0]` anywhere in Discord's nested form errors, depth first.
pub(super) fn first_form_error(errors: &Value) -> Option<&Value> {
    if let Some(first) = errors["_errors"].get(0) {
        return Some(first);
    }
    errors.as_object()?.iter().filter(|(key, _)| *key != "_errors").find_map(|(_, inner)| inner.is_object().then(|| first_form_error(inner)).flatten())
}

pub(super) const SESSION_EXPIRED: &str = "Your session has expired. Sign in again.";

// A failed call as Discord answered it: the HTTP status (0 when no answer came), its JSON error code, and
// the text `error_text` words it as. For callers that word errors themselves (Add Friend).
#[derive(Clone, Debug, PartialEq)]
pub struct ApiError {
    pub status: u16,
    pub code: Option<u64>,
    pub text: String,
}

impl From<String> for ApiError {
    fn from(text: String) -> Self {
        ApiError { status: 0, code: None, text }
    }
}

impl From<ApiError> for String {
    fn from(error: ApiError) -> Self {
        error.text
    }
}

pub(super) fn error_text(status: u16, body: &Value) -> String {
    match status {
        401 => SESSION_EXPIRED.to_owned(),
        _ if let Some(why) = body["code"].as_u64().and_then(launch_error) => why.to_owned(),
        // 40002 (verify your account), 50035 (invalid form) and the rest read as Discord words them.
        _ => api_error(body).unwrap_or_else(|| transport::StatusCode::from_u16(status).map_or_else(|_| status.to_string(), |s| s.to_string())),
    }
}

// A file on its way to Discord. `name` is what the message shows; `description` and `spoiler` ride on the
// message's attachment entry.
#[derive(Clone, Debug)]
pub struct Upload {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub description: Option<String>,
    pub spoiler: bool,
    pub sent: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

/// The most you can upload per file here: the higher of what your Nitro and the server's boost level
/// allow (docs.discord.food, Uploading Files: 10 MiB default; Nitro 500, Classic and Basic 50; boost
/// level 2 50, level 3 100).
pub fn upload_limit(premium_type: u8, guild_tier: u8) -> u64 {
    let user = match premium_type {
        2 => 500,
        1 | 3 => 50,
        _ => 10,
    };
    let guild = match guild_tier {
        3 => 100,
        2 => 50,
        _ => 10,
    };
    user.max(guild) << 20
}

/// The longest message text Discord takes: 4000 characters with Nitro, 2000 otherwise.
// Not verified: whether Discord counts code points or UTF-16 units; code points are counted here.
pub fn message_limit(premium_type: u8) -> usize {
    if premium_type == 2 { 4000 } else { 2000 }
}

// Discord caps a message at 10 attachments.
pub const MAX_FILES: usize = 10;

// `POST /channels/{id}/attachments`: one slot per file, its index as the id the response echoes.
pub(super) fn attachments_request(files: &[Upload]) -> Value {
    let files: Vec<Value> = files.iter().enumerate().map(|(i, f)| json!({ "id": i.to_string(), "filename": f.name, "file_size": f.size })).collect();
    json!({ "files": files })
}

// (upload_url, upload_filename) per file, in the request's order.
pub(super) fn parse_upload_slots(body: &Value, count: usize) -> Result<Vec<(String, String)>, String> {
    let mut slots: Vec<(usize, String, String)> = body["attachments"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(i, a)| {
            let at = a["id"].as_str().and_then(|id| id.parse().ok()).or(a["id"].as_u64().map(|id| id as usize)).unwrap_or(i);
            Some((at, a["upload_url"].as_str()?.to_owned(), a["upload_filename"].as_str()?.to_owned()))
        })
        .collect();
    slots.sort_by_key(|s| s.0);
    if slots.len() != count {
        return Err("Discord didn't return an upload URL for every file".into());
    }
    Ok(slots.into_iter().map(|(_, url, name)| (url, name)).collect())
}

// Create Message's body; `attachments` are files already uploaded.
pub(super) fn message_body(content: &str, nonce: &str, reply_to: Option<&str>, ping_author: bool, attachments: &[SentAttachment]) -> Value {
    let mut body = json!({ "content": content, "nonce": nonce });
    if let Some(id) = reply_to {
        body["message_reference"] = json!({ "message_id": id });
        body["allowed_mentions"] = json!({ "parse": ["users", "roles", "everyone"], "replied_user": ping_author });
    }
    if !attachments.is_empty() {
        let list: Vec<Value> = attachments.iter().enumerate().map(|(i, a)| a.entry(i)).collect();
        body["attachments"] = json!(list);
    }
    body
}

// An uploaded file as Create Message names it.
pub struct SentAttachment {
    pub filename: String,
    // The upload slot's `upload_filename`.
    pub uploaded_filename: String,
    pub description: Option<String>,
    pub spoiler: bool,
}

impl SentAttachment {
    // Discord's own serializer: the index as the id, `description` only when set (an empty one goes as
    // is), `is_spoiler` only when true.
    pub(super) fn entry(&self, index: usize) -> Value {
        let mut entry = json!({ "id": index.to_string(), "filename": self.filename, "uploaded_filename": self.uploaded_filename });
        if let Some(description) = &self.description {
            entry["description"] = json!(description);
        }
        if self.spoiler {
            entry["is_spoiler"] = json!(true);
        }
        entry
    }
}

// Edit Message's body for Modify Attachment after sending: every attachment the message keeps, by id,
// with the edited one carrying its alt text and spoiler (sent true or false).
// Not verified: that leaving an attachment out deletes it, which is why all of them go.
pub fn attachments_patch(ids: &[String], edited: &str, description: &str, spoiler: bool) -> Value {
    let list: Vec<Value> = ids
        .iter()
        .map(|id| match id == edited {
            true => json!({ "id": id, "description": description, "is_spoiler": spoiler }),
            false => json!({ "id": id }),
        })
        .collect();
    json!({ "attachments": list })
}

// Not verified: this is the body as the API reference describes a forward (a type-1 reference and no
// content); it hasn't been checked against a request from Discord's own client.
// Discord's play body: the sound and its emoji, and the sound's server unless it's one of Discord's own.
pub(super) fn soundboard_play_body(sound: &SoundboardSound) -> Value {
    let emoji = sound.emoji.as_ref();
    let mut body = json!({ "sound_id": sound.id, "emoji_id": emoji.and_then(|e| e.id.clone()), "emoji_name": emoji.map(|e| e.name.clone()).filter(|n| !n.is_empty()) });
    if !sound.is_default() {
        body["source_guild_id"] = json!(sound.guild_id);
    }
    body
}

pub(super) fn forward_body(original: &Message, guild_id: Option<&str>, nonce: &str) -> Value {
    let mut reference = json!({ "type": 1, "message_id": original.id, "channel_id": original.channel_id });
    if let Some(guild_id) = guild_id {
        reference["guild_id"] = json!(guild_id);
    }
    json!({ "nonce": nonce, "message_reference": reference })
}

// The PUT to a signed storage URL: no Authorization (the URL is the credential), streamed in chunks so
// `sent` follows the bytes out. Content-Length is set so the body isn't sent chunked.
pub(super) async fn put_file(url: String, path: PathBuf, sent: std::sync::Arc<std::sync::atomic::AtomicU64>) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    let bytes = bytes::Bytes::from(tokio::fs::read(&path).await.map_err(|e| e.to_string())?);
    let len = bytes.len();
    sent.store(0, Ordering::Relaxed);
    let chunks = (0..len).step_by(64 << 10).map(move |at| bytes.slice(at..(at + (64 << 10)).min(len)));
    let stream = futures::stream::iter(chunks).map(move |chunk| {
        sent.fetch_add(chunk.len() as u64, Ordering::Relaxed);
        Ok::<_, std::io::Error>(chunk)
    });
    let res = transport::plain(transport::Method::PUT, url).header(transport::header::CONTENT_LENGTH, len).body(transport::Body::wrap_stream(stream)).send().await.map_err(|e| e.to_string())?;
    if res.status().is_success() {
        return Ok(());
    }
    let status = res.status();
    Err(format!("Upload failed: {status}"))
}

// Discord's refusal in its own words. "Invalid Form Body" says nothing alone, so the first field's
// message follows it: "Invalid Form Body: name: Must be between 1 and 100 in length."
/// A REST error that means the token itself was refused (HTTP 401).
pub fn is_unauthorized(error: &str) -> bool {
    error == SESSION_EXPIRED
}

pub(super) fn api_error(body: &Value) -> Option<String> {
    fn first_field(errors: &Value, path: &str) -> Option<String> {
        if let Some(message) = errors["_errors"][0]["message"].as_str() {
            return Some(format!("{path}: {message}"));
        }
        errors.as_object()?.iter().find_map(|(key, inner)| first_field(inner, &if path.is_empty() { key.clone() } else { format!("{path}.{key}") }))
    }
    let message = body["message"].as_str()?;
    Some(match first_field(&body["errors"], "") {
        Some(detail) => format!("{message}: {detail}"),
        None => message.to_owned(),
    })
}

// `send` for lookups where 403 and 404 are answers ("not there for you"), not failures to retry.
pub(super) async fn send_or_gone(req: transport::Request) -> Result<Option<Value>, String> {
    let res = req.send().await.map_err(|e| e.to_string())?;
    let status = res.status();
    if matches!(status.as_u16(), 403 | 404) {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(status.to_string());
    }
    res.json().await.map(Some).map_err(|e| e.to_string())
}

// A server invite's card: `GET /invites/{code}?with_counts=true`.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
// Cached previews predate some fields.
#[serde(default)]
pub struct Invite {
    pub guild_id: String,
    pub name: String,
    pub icon: Option<String>,
    pub online: Option<u64>,
    pub members: Option<u64>,
    pub features: Vec<String>,
    pub splash: Option<String>,
    // The invite lets you pick a server nickname before joining (Discord's `is_nickname_changeable`).
    pub nickname_changeable: bool,
}

// A server's invite splash on the CDN, wide enough for a full-window background.
pub fn splash_url(guild_id: &str, hash: &str) -> String {
    format!("{CDN}/splashes/{guild_id}/{hash}.jpg?size=2048")
}

// The server a joined invite was to; a group DM invite has none.
pub(super) fn joined_guild(body: &Value) -> Option<String> {
    body["guild"]["id"].as_str().or(body["guild_id"].as_str()).map(str::to_owned)
}

// Group DM invites carry no guild; they get the invalid card, as there is no server to show.
pub fn parse_invite(v: &Value) -> Option<Invite> {
    let guild = &v["guild"];
    let id = guild["id"].as_str()?;
    Some(Invite {
        guild_id: id.to_owned(),
        name: guild["name"].as_str().unwrap_or_default().to_owned(),
        icon: guild["icon"].as_str().map(|hash| format!("{CDN}/icons/{id}/{hash}.png?size=96")),
        online: v["approximate_presence_count"].as_u64(),
        members: v["approximate_member_count"].as_u64(),
        features: strings(&guild["features"]),
        splash: guild["splash"].as_str().map(|hash| splash_url(id, hash)),
        nickname_changeable: v["is_nickname_changeable"].as_bool().unwrap_or(false),
    })
}

// What the app and the voice engine send through the gateway, and where voice dispatches go.
