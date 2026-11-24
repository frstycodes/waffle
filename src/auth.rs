use std::time::Duration;

use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use futures::{SinkExt, StreamExt, channel::mpsc};
use rsa::{Oaep, RsaPrivateKey, pkcs8::EncodePublicKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_tungstenite::tungstenite::{Message as WsMessage, client::IntoClientRequest, http::HeaderValue};

use crate::{
    captcha,
    discord::{self, User},
};

// Discord's QR sign-in ("remote auth"). Undocumented; the op names below are what the official
// clients use as of v2 of the protocol. Not verified against a live server in this repo yet.
const REMOTE_AUTH: &str = "wss://remote-auth-gateway.discord.gg/?v=2";
// Release builds only: debug builds keep tokens in files (see `stored_token`).
#[cfg(not(debug_assertions))]
const KEYRING_SERVICE: &str = "waffle";
const KEYRING_USER: &str = "discord-token";

pub enum AuthEvent {
    Qr(String),
    Scanned(Box<User>),
    /// Discord wants a captcha; the URL is a local page that shows it in the browser.
    Captcha(String),
    SignedIn(String),
    Failed(String),
}

// Release builds keep each account's token in the Keychain, one entry per user id. Debug builds use
// files readable only by the user (0600): every rebuild changes the binary's signature, so the
// Keychain would ask for the password on each run. Testing convenience only: tokens sit on disk in
// plain text.
pub fn stored_token(user_id: &str) -> Option<String> {
    read(&slot(user_id)?)
}

pub fn store_token(user_id: &str, token: &str) {
    if let Some(slot) = slot(user_id) {
        write(&slot, token);
    }
}

pub fn forget_token(user_id: &str) {
    if let Some(slot) = slot(user_id) {
        delete(&slot);
    }
}

// Ids are snowflakes; anything else never names a Keychain entry or a file.
fn slot(user_id: &str) -> Option<String> {
    (!user_id.is_empty() && user_id.bytes().all(|b| b.is_ascii_digit())).then(|| format!("{KEYRING_USER}-{user_id}"))
}

#[cfg(not(debug_assertions))]
fn read(slot: &str) -> Option<String> {
    keyring::Entry::new(KEYRING_SERVICE, slot).ok()?.get_password().ok()
}

#[cfg(not(debug_assertions))]
fn write(slot: &str, token: &str) {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, slot) {
        let _ = entry.set_password(token);
    }
}

#[cfg(not(debug_assertions))]
fn delete(slot: &str) {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, slot) {
        let _ = entry.delete_credential();
    }
}

#[cfg(debug_assertions)]
fn token_file(slot: &str) -> Option<std::path::PathBuf> {
    Some(crate::data_dir()?.join(slot.replace(KEYRING_USER, "dev-token")))
}

#[cfg(debug_assertions)]
fn read(slot: &str) -> Option<String> {
    std::fs::read_to_string(token_file(slot)?).ok().map(|t| t.trim().to_owned()).filter(|t| !t.is_empty())
}

#[cfg(debug_assertions)]
fn write(slot: &str, token: &str) {
    use std::io::Write;
    let Some(path) = token_file(slot) else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    // Owner-only on Unix. On Windows the file sits in the user's own %APPDATA%, which other users can't read.
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    if let Ok(mut file) = options.open(&path) {
        let _ = file.write_all(token.as_bytes());
    }
}

#[cfg(debug_assertions)]
fn delete(slot: &str) {
    if let Some(path) = token_file(slot) {
        let _ = std::fs::remove_file(path);
    }
}

// Keeps offering fresh QR codes (each expires after ~2 minutes, or when the phone cancels)
// until one is approved. Dropping the receiver stops it.
pub fn sign_in() -> mpsc::UnboundedReceiver<AuthEvent> {
    let (tx, rx) = mpsc::unbounded();
    discord::runtime().spawn(async move {
        loop {
            let event = match qr_session(&tx).await {
                Ok(token) => AuthEvent::SignedIn(token),
                Err(Outcome::Expired) if !tx.is_closed() => continue,
                Err(Outcome::Expired) => return,
                Err(Outcome::Failed(error)) => {
                    eprintln!("auth: {error}");
                    AuthEvent::Failed(error)
                }
            };
            let _ = tx.unbounded_send(event);
            return;
        }
    });
    rx
}

enum Outcome {
    Expired,
    Failed(String),
}

async fn qr_session(tx: &mpsc::UnboundedSender<AuthEvent>) -> Result<String, Outcome> {
    let failed = |e: &dyn std::fmt::Display| Outcome::Failed(e.to_string());
    let key = tokio::task::spawn_blocking(|| RsaPrivateKey::new(&mut rand::thread_rng(), 2048))
        .await
        .map_err(|e| failed(&e))?
        .map_err(|e| failed(&e))?;
    let public_key = key.to_public_key().to_public_key_der().map_err(|e| failed(&e))?;

    let mut request = REMOTE_AUTH.into_client_request().map_err(|e| failed(&e))?;
    // The gateway refuses (403) a connection without Discord's own origin; it takes Waffle's user agent.
    request.headers_mut().insert("Origin", HeaderValue::from_static("https://discord.com"));
    if let Ok(agent) = HeaderValue::from_str(&crate::discord::identity().user_agent) {
        request.headers_mut().insert("User-Agent", agent);
    }
    let (ws, _) = tokio_tungstenite::connect_async(request).await.map_err(|e| Outcome::Failed(format!("QR gateway: {}", describe_ws_error(&e))))?;
    let (mut sink, mut stream) = ws.split();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(3600));
    heartbeat.tick().await;
    let send = |payload: Value| WsMessage::Text(payload.to_string());

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                sink.send(send(json!({ "op": "heartbeat" }))).await.map_err(|e| failed(&e))?;
            }
            frame = stream.next() => {
                let text = match frame {
                    Some(Ok(WsMessage::Text(text))) => text,
                    Some(Ok(WsMessage::Close(_))) | None => return Err(Outcome::Expired),
                    Some(Ok(_)) => continue,
                    Some(Err(e)) => return Err(failed(&e)),
                };
                let msg: Value = serde_json::from_str(&text).map_err(|e| failed(&e))?;
                match msg["op"].as_str().unwrap_or_default() {
                    "hello" => {
                        let ms = msg["heartbeat_interval"].as_u64().unwrap_or(41_250);
                        heartbeat = tokio::time::interval(Duration::from_millis(ms));
                        heartbeat.tick().await;
                        let init = json!({ "op": "init", "encoded_public_key": STANDARD.encode(public_key.as_bytes()) });
                        sink.send(send(init)).await.map_err(|e| failed(&e))?;
                    }
                    "nonce_proof" => {
                        let nonce = decrypt(&key, &msg["encrypted_nonce"])?;
                        let proof = URL_SAFE_NO_PAD.encode(Sha256::digest(&nonce));
                        sink.send(send(json!({ "op": "nonce_proof", "proof": proof }))).await.map_err(|e| failed(&e))?;
                    }
                    "pending_remote_init" => {
                        let fingerprint = msg["fingerprint"].as_str().unwrap_or_default();
                        let _ = tx.unbounded_send(AuthEvent::Qr(format!("https://discord.com/ra/{fingerprint}")));
                    }
                    "pending_ticket" | "pending_finish" => {
                        let payload = String::from_utf8(decrypt(&key, &msg["encrypted_user_payload"])?).map_err(|e| failed(&e))?;
                        if let Some(user) = parse_user_payload(&payload) {
                            let _ = tx.unbounded_send(AuthEvent::Scanned(Box::new(user)));
                        }
                    }
                    "pending_login" => {
                        // The exchange can wait on a captcha for minutes. The gateway session must stay alive
                        // meanwhile: one that misses its heartbeats is closed, and its ticket stops working.
                        let ticket = msg["ticket"].as_str().unwrap_or_default().to_owned();
                        let exchange = exchange_ticket(tx, &key, &ticket);
                        tokio::pin!(exchange);
                        let mut open = true;
                        loop {
                            tokio::select! {
                                result = &mut exchange => return result,
                                _ = heartbeat.tick(), if open => {
                                    // A failed beat means the socket is gone; the exchange may still succeed.
                                    open = sink.send(send(json!({ "op": "heartbeat" }))).await.is_ok();
                                }
                                frame = stream.next(), if open => match frame {
                                    Some(Ok(WsMessage::Close(close))) => {
                                        eprintln!("auth: QR gateway closed during the ticket exchange: {close:?}");
                                        open = false;
                                    }
                                    Some(Err(e)) => {
                                        eprintln!("auth: QR gateway error during the ticket exchange: {e}");
                                        open = false;
                                    }
                                    None => open = false,
                                    Some(Ok(_)) => {}
                                },
                            }
                        }
                    }
                    // Protocol v1 sends the token directly.
                    "finish" => return String::from_utf8(decrypt(&key, &msg["encrypted_token"])?).map_err(|e| failed(&e)),
                    "cancel" => return Err(Outcome::Expired),
                    _ => {}
                }
            }
        }
    }
}

// The ticket exchange goes where Discord's web client sends it, as that client: one cookie jar for
// every attempt (a solved captcha is bound to the session that was asked), and the fingerprint,
// locale and timezone the web client sends.
const LOGIN_API: &str = "https://discord.com/api/v9";

async fn exchange_ticket(tx: &mpsc::UnboundedSender<AuthEvent>, key: &RsaPrivateKey, ticket: &str) -> Result<String, Outcome> {
    let failed = |e: String| Outcome::Failed(format!("Ticket exchange: {e}"));
    let client = reqwest::Client::builder().cookie_store(true).user_agent(&crate::discord::identity().user_agent).build().map_err(|e| failed(e.to_string()))?;
    let fingerprint = web_fingerprint(&client).await;
    let mut solved: Option<(captcha::Challenge, String)> = None;
    // Discord may ask for a captcha, and occasionally asks again after one is solved.
    for attempt in 1..=3 {
        let mut req = crate::discord::identity().headers(client.post(format!("{LOGIN_API}/users/@me/remote-auth/login")).header("Origin", "https://discord.com").header("Referer", "https://discord.com/login").json(&json!({ "ticket": ticket })));
        if let Some(fingerprint) = &fingerprint {
            req = req.header("X-Fingerprint", fingerprint);
        }
        if let Some((challenge, answer)) = &solved {
            for (name, value) in challenge.headers(answer) {
                req = req.header(name, value);
            }
        }
        let res = req.send().await.map_err(|e| failed(e.to_string()))?;
        let status = res.status();
        let body: Value = res.json().await.unwrap_or(Value::Null);
        if status.is_success() {
            let token = decrypt(key, &body["encrypted_token"])?;
            return String::from_utf8(token).map_err(|e| Outcome::Failed(e.to_string()));
        }
        let Some(challenge) = captcha::Challenge::from_response(&body) else {
            eprintln!("http: {status} remote-auth/login (attempt {attempt}, after captcha: {}): {body}", solved.is_some());
            return Err(failed(body["message"].as_str().map(str::to_owned).unwrap_or_else(|| status.to_string())));
        };
        eprintln!("auth: remote-auth/login asked for a captcha (attempt {attempt})");
        let pending = captcha::serve(&challenge).await.map_err(failed)?;
        let _ = tx.unbounded_send(AuthEvent::Captcha(pending.url.clone()));
        let answer = pending.solved().await.map_err(Outcome::Failed)?;
        solved = Some((challenge, answer));
    }
    Err(failed("Discord kept asking for a captcha".into()))
}

// The fingerprint Discord hands a signed-out web client (and the cookies set with it, kept by `client`).
async fn web_fingerprint(client: &reqwest::Client) -> Option<String> {
    let res = client.get(format!("{LOGIN_API}/experiments")).header("Origin", "https://discord.com").send().await.ok()?;
    let body: Value = res.json().await.ok()?;
    body["fingerprint"].as_str().map(str::to_owned)
}

// A refused upgrade carries the server's reason in the response body; show it instead of just the status.
fn describe_ws_error(e: &tokio_tungstenite::tungstenite::Error) -> String {
    use tokio_tungstenite::tungstenite::Error;
    match e {
        Error::Http(res) => {
            let body = res.body().as_deref().map(String::from_utf8_lossy).unwrap_or_default();
            let body: String = body.trim().chars().take(200).collect();
            if body.is_empty() { res.status().to_string() } else { format!("{} ({body})", res.status()) }
        }
        e => e.to_string(),
    }
}

fn decrypt(key: &RsaPrivateKey, encoded: &Value) -> Result<Vec<u8>, Outcome> {
    let failed = |e: &dyn std::fmt::Display| Outcome::Failed(e.to_string());
    let bytes = STANDARD.decode(encoded.as_str().unwrap_or_default()).map_err(|e| failed(&e))?;
    key.decrypt(Oaep::new::<Sha256>(), &bytes).map_err(|e| failed(&e))
}

// "id:discriminator:avatar_hash:username"
fn parse_user_payload(payload: &str) -> Option<User> {
    let mut parts = payload.splitn(4, ':');
    let (id, _discriminator, avatar, name) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    Some(User { id: id.to_owned(), name: name.to_owned(), avatar: discord::avatar_url(id, Some(avatar)), ..Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::RsaPublicKey;

    #[test]
    fn only_snowflakes_name_slots() {
        assert_eq!(slot("80351110224678912").as_deref(), Some("discord-token-80351110224678912"));
        assert!(slot("").is_none());
        assert!(slot("../dev-token").is_none());
    }

    #[test]
    fn parses_scanned_user() {
        let user = parse_user_payload("9:0:abc:ferris").unwrap();
        assert_eq!(user.name, "ferris");
        assert!(user.avatar.ends_with("/avatars/9/abc.png?size=128"));
        assert!(parse_user_payload("9:0").is_none());
    }

    #[test]
    fn decrypts_oaep_sha256() {
        let key = RsaPrivateKey::new(&mut rand::thread_rng(), 1024).unwrap();
        let sealed = RsaPublicKey::from(&key).encrypt(&mut rand::thread_rng(), Oaep::new::<Sha256>(), b"nonce").unwrap();
        let plain = decrypt(&key, &Value::String(STANDARD.encode(sealed))).ok().unwrap();
        assert_eq!(plain, b"nonce");
    }
}
