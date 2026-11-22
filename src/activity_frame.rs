// An activity's frame: Discord's embedded app (https://<app>.discordsays.com) in a native webview laid over
// its tile, and the client's half of the Embedded App SDK's postMessage RPC. The webview loads a host page
// with an opaque origin ("null", which the SDK accepts) that frames the app and relays its messages here.
use std::{collections::HashSet, sync::Arc};

use futures::{StreamExt, channel::mpsc};
use gpui_kit::{Bounds, Context, EventEmitter, Image, ImageFormat, IntoElement, ObjectFit, ParentElement, Pixels, Render, Styled, StyledImage, Task, Window, canvas, div, img};
use serde_json::{Value, json};

// The SDK's opcodes.
const HANDSHAKE: u64 = 0;
const FRAME: u64 = 1;
const CLOSE: u64 = 2;
// RPC close code for a handshake from a frame we didn't open.
const CLOSE_UNSUPPORTED: u64 = 4000;
const ACTIVITY_HOST: &str = "discordsays.com";
// Some activities check for a browser they know; WKWebView's own agent names none.
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";

pub enum FrameEvent {
    // A command for the app to answer (AUTHORIZE, GET_CHANNEL …), answered with `ActivityFrame::reply`.
    Command { nonce: Value, cmd: String, args: Value },
    // The app closed itself (the SDK's close), with its own words.
    Closed { message: String },
}

pub struct ActivityFrame {
    pub app_id: String,
    webview: Option<wry::WebView>,
    frame_id: String,
    shown: bool,
    placed: Option<Bounds<Pixels>>,
    // The app's SDK has shaken hands: it loaded.
    pub connected: bool,
    // Events the app subscribed to; only those are dispatched to it.
    subscriptions: HashSet<String>,
    // The game as it last looked, drawn in its place while the live view is hidden under a menu or dialog.
    still: Option<Arc<Image>>,
    _snapshot: Option<Task<()>>,
    _inbox: Task<()>,
}

impl EventEmitter<FrameEvent> for ActivityFrame {}

impl ActivityFrame {
    // `params` are the instance's query parameters (instance_id, location_id, the proxy ticket …).
    pub fn new(app_id: String, params: Vec<(&'static str, String)>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let frame_id = format!("{:032x}", rand::random::<u128>());
        let base = format!("https://{app_id}.{ACTIVITY_HOST}/");
        let query = params.into_iter().chain([("frame_id", frame_id.clone()), ("platform", "desktop".into())]);
        let url = reqwest::Url::parse_with_params(&base, query).map(String::from).unwrap_or(base);
        let (tx, mut rx) = mpsc::unbounded::<String>();
        let webview = wry::WebViewBuilder::new()
            .with_html(host_page(&url))
            .with_user_agent(USER_AGENT)
            .with_background_color((0, 0, 0, 255))
            .with_visible(false)
            .with_ipc_handler(move |req| _ = tx.unbounded_send(req.into_body()))
            .build_as_child(window)
            .map_err(|e| eprintln!("activity frame: {e}"))
            .ok();
        let inbox = cx.spawn(async move |this, cx| {
            while let Some(text) = rx.next().await {
                if this.update(cx, |frame, cx| frame.receive(&text, cx)).is_err() {
                    break;
                }
            }
        });
        Self { app_id, webview, frame_id, connected: false, shown: false, placed: None, subscriptions: HashSet::new(), still: None, _snapshot: None, _inbox: inbox }
    }

    pub fn failed(&self) -> bool {
        self.webview.is_none()
    }

    // The webview draws over everything in the window, so it hides while anything else needs that spot. Hiding,
    // it first takes a still of the game to show in its place, and goes once the still is up; a menu opening
    // waits that long (a few frames) to show over the game.
    pub fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        if self.shown == shown {
            return;
        }
        self.shown = shown;
        if shown {
            self.still = None;
            self._snapshot = None;
            self.apply_visibility();
            return cx.notify();
        }
        let Some(still) = self.webview.as_ref().and_then(snapshot) else { return self.apply_visibility() };
        self._snapshot = Some(cx.spawn(async move |this, cx| {
            let still = still.await.ok().flatten();
            _ = this.update(cx, |frame, cx| {
                if frame.shown {
                    return;
                }
                frame.still = still.map(|jpeg| Arc::new(Image::from_bytes(ImageFormat::Jpeg, jpeg)));
                frame.apply_visibility();
                cx.notify();
            });
        }));
    }

    pub fn reply(&self, nonce: Value, cmd: &str, result: Result<Value, (u32, String)>) {
        let payload = match result {
            Ok(data) => json!({ "cmd": cmd, "evt": null, "nonce": nonce, "data": data }),
            Err((code, message)) => json!({ "cmd": cmd, "evt": "ERROR", "nonce": nonce, "data": { "code": code, "message": message } }),
        };
        self.send(json!([FRAME, payload]));
    }

    pub fn dispatch(&self, evt: &str, data: Value) {
        if self.subscriptions.contains(evt) {
            self.send(json!([FRAME, { "cmd": "DISPATCH", "evt": evt, "nonce": null, "data": data }]));
        }
    }

    fn receive(&mut self, text: &str, cx: &mut Context<Self>) {
        let Ok(Value::Array(message)) = serde_json::from_str::<Value>(text) else { return };
        let (Some(op), Some(payload)) = (message.first().and_then(Value::as_u64), message.get(1)) else { return };
        match op {
            HANDSHAKE => self.handshake(payload),
            FRAME => self.command(payload, cx),
            CLOSE => cx.emit(FrameEvent::Closed { message: payload["message"].as_str().unwrap_or_default().to_owned() }),
            _ => {}
        }
    }

    fn handshake(&mut self, payload: &Value) {
        if payload["frame_id"].as_str() != Some(self.frame_id.as_str()) || payload["client_id"].as_str() != Some(self.app_id.as_str()) {
            return self.send(json!([CLOSE, { "code": CLOSE_UNSUPPORTED, "message": "Unrecognized frame" }]));
        }
        self.connected = true;
        let config = json!({ "cdn_host": "cdn.discordapp.com", "api_endpoint": "//discord.com/api", "environment": "production" });
        self.send(json!([FRAME, { "cmd": "DISPATCH", "evt": "READY", "nonce": null, "data": { "v": 1, "config": config } }]));
    }

    fn command(&mut self, payload: &Value, cx: &mut Context<Self>) {
        let cmd = payload["cmd"].as_str().unwrap_or_default().to_owned();
        let nonce = payload["nonce"].clone();
        let evt = payload["evt"].as_str().map(str::to_owned);
        match (cmd.as_str(), evt) {
            ("SUBSCRIBE", Some(evt)) => {
                self.subscriptions.insert(evt.clone());
                self.reply(nonce, &cmd, Ok(json!({ "evt": evt })));
            }
            ("UNSUBSCRIBE", Some(evt)) => {
                self.subscriptions.remove(&evt);
                self.reply(nonce, &cmd, Ok(json!({ "evt": evt })));
            }
            _ => cx.emit(FrameEvent::Command { nonce, cmd, args: payload["args"].clone() }),
        }
    }

    // The message is JSON, which is also a JavaScript literal.
    fn send(&self, message: Value) {
        if let Some(webview) = &self.webview {
            _ = webview.evaluate_script(&format!("window.__waffle({message})"));
        }
    }

    // Moves the webview into another window (a pop-out, and back); it places itself again on that window's next paint.
    pub fn move_to(&mut self, window: &Window) {
        self.placed = None;
        let Some(webview) = &self.webview else { return };
        move_webview(webview, window);
    }

    fn place(&mut self, bounds: Bounds<Pixels>) {
        if self.placed == Some(bounds) {
            return;
        }
        self.placed = Some(bounds);
        if let Some(webview) = &self.webview {
            let (origin, size) = (bounds.origin, bounds.size);
            _ = webview.set_bounds(wry::Rect {
                position: wry::dpi::LogicalPosition::new(f32::from(origin.x) as f64, f32::from(origin.y) as f64).into(),
                size: wry::dpi::LogicalSize::new(f32::from(size.width) as f64, f32::from(size.height) as f64).into(),
            });
        }
        self.apply_visibility();
    }

    fn apply_visibility(&self) {
        if let Some(webview) = &self.webview {
            _ = webview.set_visible(self.shown && self.placed.is_some());
        }
    }
}

// The frame takes the bounds its slot paints at; the webview follows them.
impl Render for ActivityFrame {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.weak_entity();
        let still = self.still.clone().filter(|_| !self.shown);
        div()
            .relative()
            .size_full()
            .children(still.map(|image| img(image).absolute().inset_0().size_full().object_fit(ObjectFit::Fill)))
            .child(
                canvas(|_, _, _| (), move |bounds, _, _, cx| {
                    _ = this.update(cx, |frame, _| frame.place(bounds));
                })
                .absolute()
                .inset_0()
                .size_full(),
            )
    }
}

// The page the webview loads: the activity in a sandboxed iframe (no top navigation, no popups), and a relay
// between its postMessages and the host. Only this page can reach `window.ipc`; the frame is cross-origin.
fn host_page(url: &str) -> String {
    let src = Value::String(url.to_owned());
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><style>html,body{{margin:0;height:100%;overflow:hidden;background:#000}}iframe{{display:block;border:0;width:100%;height:100%}}</style></head><body>
<iframe id="f" allow="autoplay; encrypted-media; fullscreen; clipboard-write" sandbox="allow-scripts allow-same-origin allow-forms allow-pointer-lock allow-modals"></iframe>
<script>
const f = document.getElementById('f');
window.__waffle = (m) => f.contentWindow.postMessage(m, '*');
addEventListener('message', (e) => {{ if (e.source === f.contentWindow) window.ipc.postMessage(JSON.stringify(e.data)); }});
f.src = {src};
</script></body></html>"#
    )
}

#[cfg(target_os = "macos")]
fn move_webview(webview: &wry::WebView, window: &Window) {
    use objc2::{msg_send, runtime::AnyObject};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use wry::WebViewExtMacOS;
    let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return };
    let view = handle.ns_view.as_ptr() as *mut AnyObject;
    // SAFETY: gpui's live content view on the main thread; its window outlives this call.
    let ns_window: *mut AnyObject = unsafe { msg_send![view, window] };
    if !ns_window.is_null() {
        _ = webview.reparent(ns_window.cast());
    }
}

// GAP: Windows has no pop-out yet; the button that offers it shows on macOS only.
#[cfg(not(target_os = "macos"))]
fn move_webview(_: &wry::WebView, _: &Window) {}

// The webview's current picture as JPEG, from WKWebView's own snapshot. None where there's no snapshot to take.
#[cfg(target_os = "macos")]
fn snapshot(webview: &wry::WebView) -> Option<futures::channel::oneshot::Receiver<Option<Vec<u8>>>> {
    use std::cell::RefCell;

    use block2::RcBlock;
    use objc2::{class, msg_send, rc::Retained, runtime::AnyObject};
    use wry::WebViewExtMacOS;

    // NSBitmapImageFileTypeJPEG.
    const JPEG: usize = 3;
    let (tx, rx) = futures::channel::oneshot::channel();
    let tx = RefCell::new(Some(tx));
    let done = RcBlock::new(move |image: *mut AnyObject, _error: *mut AnyObject| {
        let Some(tx) = tx.borrow_mut().take() else { return };
        if image.is_null() {
            _ = tx.send(None);
            return;
        }
        // SAFETY: WebKit calls this on the main thread with a live NSImage; every object here is autoreleased
        // or owned by `image`, and the bytes are copied out before returning.
        let jpeg = unsafe {
            let tiff: *mut AnyObject = msg_send![image, TIFFRepresentation];
            let rep: *mut AnyObject = msg_send![class!(NSBitmapImageRep), imageRepWithData: tiff];
            let options: *mut AnyObject = msg_send![class!(NSDictionary), dictionary];
            let data: *mut AnyObject = if rep.is_null() { std::ptr::null_mut() } else { msg_send![rep, representationUsingType: JPEG, properties: options] };
            (!data.is_null()).then(|| {
                let len: usize = msg_send![data, length];
                let bytes: *const u8 = msg_send![data, bytes];
                std::slice::from_raw_parts(bytes, len).to_vec()
            })
        };
        _ = tx.send(jpeg);
    });
    let view = webview.webview();
    let view: &AnyObject = Retained::as_ref(&view);
    let nil: *mut AnyObject = std::ptr::null_mut();
    // SAFETY: a live WKWebView on the main thread; a nil configuration snapshots the whole visible view.
    unsafe {
        let _: () = msg_send![view, takeSnapshotWithConfiguration: nil, completionHandler: &*done];
    }
    Some(rx)
}

#[cfg(not(target_os = "macos"))]
fn snapshot(_: &wry::WebView) -> Option<futures::channel::oneshot::Receiver<Option<Vec<u8>>>> {
    None
}
