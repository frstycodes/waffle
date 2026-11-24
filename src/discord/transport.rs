// How Waffle reaches Discord: the one file that names the HTTP and websocket library. The rest of the app
// goes through `Client` (client.rs) or the functions here, so swapping the library is this file and
// Cargo.toml, as long as the replacement keeps reqwest's builder shape.
//
// wreq makes the TLS and HTTP/2 handshakes Chrome's (BoringSSL), matching the Chrome identity.rs claims.
// CDN images, fonts and the updater stay on reqwest: they carry no account.
use std::sync::OnceLock;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Role};
pub use wreq::{Body, Method, RequestBuilder as Request, StatusCode, header, multipart};
use wreq_util::{Emulation, Platform, Profile};

use super::identity;

// Keep in step with identity.rs's CHROME_MAJOR, so the handshake and the user agent name the same Chrome.
const PROFILE: Profile = Profile::Chrome149;

// One client for the run, so connections and Discord's cookies carry over between requests, as a tab's do.
fn http() -> &'static wreq::Client {
    static HTTP: OnceLock<wreq::Client> = OnceLock::new();
    HTTP.get_or_init(|| {
        // identity.rs sets every header itself, so the profile's own (a page load's, not a fetch's) stay off.
        let emulation = Emulation::builder().profile(PROFILE).platform(Platform::Windows).headers(false).build();
        wreq::Client::builder().emulation(emulation).cookie_store(true).build().expect("the Chrome profile builds a client")
    })
}

// A request with the web client's headers; the caller adds Authorization where Discord wants it.
pub fn request(method: Method, url: impl AsRef<str>) -> Request {
    identity().headers(http().request(method, url.as_ref()))
}

// A request without Discord's headers, for URLs that aren't Discord's API (signed upload URLs, the login page).
pub fn plain(method: Method, url: impl AsRef<str>) -> Request {
    http().request(method, url.as_ref())
}

// A websocket opened the way the web client opens one from discord.com: the gateway, voice servers and QR
// sign-in. The stream is tungstenite's, so the code on top doesn't know which library opened it.
// Over HTTP/1.1, as Chrome opens them, with Chrome's header case and order: Cloudflare refuses (403) a
// handshake that carries a browser's Origin in wreq's lowercase order.
pub async fn websocket(url: &str) -> Result<WebSocketStream<impl AsyncRead + AsyncWrite + Unpin + Send + 'static>, String> {
    let socket = http()
        .websocket(url)
        .version(wreq::Version::HTTP_11)
        .orig_headers(chrome_upgrade_order())
        .header(wreq::header::PRAGMA, "no-cache")
        .header(wreq::header::CACHE_CONTROL, "no-cache")
        .header(wreq::header::USER_AGENT, &identity().user_agent)
        .header(wreq::header::ORIGIN, "https://discord.com")
        .header(wreq::header::ACCEPT_ENCODING, "gzip, deflate, br, zstd")
        .header(wreq::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .into_websocket()
        .await
        .map_err(|e| e.to_string())?;
    // Nothing has been read past the handshake yet, so the raw stream hands over without losing a frame.
    Ok(WebSocketStream::from_raw_socket(socket.into_inner(), Role::Client, None).await)
}

// The order Chrome writes an upgrade's headers in. Not offered: permessage-deflate, which Chrome sends and
// tungstenite can't decode.
fn chrome_upgrade_order() -> wreq::header::OrigHeaderMap {
    let mut order = wreq::header::OrigHeaderMap::new();
    for name in ["Host", "Connection", "Pragma", "Cache-Control", "User-Agent", "Upgrade", "Origin", "Sec-WebSocket-Version", "Accept-Encoding", "Accept-Language", "Sec-WebSocket-Key"] {
        order.insert(name);
    }
    order
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;

    use super::*;

    // Live, no token: Discord answers the web client's signed-out calls and the gateway says hello.
    #[test]
    #[ignore = "reaches discord.com"]
    fn live_discord_takes_the_chrome_client() {
        crate::discord::runtime().block_on(async {
            let res = request(Method::GET, "https://discord.com/api/v9/experiments").send().await.expect("request");
            assert!(res.status().is_success(), "{}", res.status());
            let mut ws = websocket("wss://gateway.discord.gg/?v=10&encoding=json").await.expect("websocket");
            let hello = ws.next().await.expect("a frame").expect("readable").into_text().expect("text");
            assert!(hello.contains("\"op\":10"), "{hello}");
            let mut qr = websocket("wss://remote-auth-gateway.discord.gg/?v=2").await.expect("QR sign-in websocket");
            let hello = qr.next().await.expect("a frame").expect("readable").into_text().expect("text");
            assert!(hello.contains("\"op\":\"hello\""), "{hello}");
        });
    }
}
