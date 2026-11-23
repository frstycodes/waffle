// An activity's frame: Discord's embedded app (https://<app>.discordsays.com) in a native webview over its
// tile, and the client's half of the Embedded App SDK's postMessage RPC. The webview loads a host page with an
// opaque origin ("null", which the SDK accepts) that frames the app and relays its messages here.
//
// On macOS the webview sits in a gpui-fast composition surface: above the app, below its deferred draws, so
// tooltips, menus, popovers and dialogs draw over the game. Elsewhere it covers the window, and hides while
// something else needs its spot.
use std::collections::HashSet;

use futures::{StreamExt, channel::mpsc};
use gpui_kit::{App, Bounds, Context, EventEmitter, IntoElement, ParentElement, Pixels, Render, Styled, Task, Window, canvas, div};
use serde_json::{Value, json};

// The SDK's opcodes.
const HANDSHAKE: u64 = 0;
const FRAME: u64 = 1;
const CLOSE: u64 = 2;
// RPC close code for a handshake from a frame we didn't open.
const CLOSE_UNSUPPORTED: u64 = 4000;
const ACTIVITY_HOST: &str = "discordsays.com";
// How long the slot must hold its size before the page is laid out at it.
#[cfg(target_os = "macos")]
const RESIZE_SETTLE: std::time::Duration = std::time::Duration::from_millis(150);
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
    #[cfg(target_os = "macos")]
    composed: Option<compose::Composed>,
    // Waits for a resize to pause before the page takes the new size.
    #[cfg(target_os = "macos")]
    _settle: Option<Task<()>>,
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
        #[cfg(target_os = "macos")]
        let composed = webview.as_ref().and_then(|webview| compose::attach(webview, window, cx));
        let inbox = cx.spawn(async move |this, cx| {
            while let Some(text) = rx.next().await {
                if this.update(cx, |frame, cx| frame.receive(&text, cx)).is_err() {
                    break;
                }
            }
        });
        Self {
            app_id,
            webview,
            frame_id,
            connected: false,
            shown: false,
            placed: None,
            subscriptions: HashSet::new(),
            #[cfg(target_os = "macos")]
            composed,
            #[cfg(target_os = "macos")]
            _settle: None,
            _inbox: inbox,
        }
    }

    pub fn failed(&self) -> bool {
        self.webview.is_none()
    }

    // Whether gpui draws its overlays over the game; without it the webview covers them, and must hide.
    pub fn composed(&self) -> bool {
        #[cfg(target_os = "macos")]
        return self.composed.is_some();
        #[cfg(not(target_os = "macos"))]
        false
    }

    pub fn set_shown(&mut self, shown: bool) {
        if self.shown != shown {
            self.shown = shown;
            self.apply_visibility();
        }
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
    pub fn move_to(&mut self, window: &Window, cx: &mut App) {
        self.placed = None;
        let Some(webview) = &self.webview else { return };
        #[cfg(target_os = "macos")]
        if let Some(composed) = self.composed.take() {
            composed.release(cx);
        }
        move_webview(webview, window);
        #[cfg(target_os = "macos")]
        {
            self.composed = compose::attach(webview, window, cx);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = cx;
    }

    fn place(&mut self, bounds: Bounds<Pixels>, scale: f32, cx: &mut Context<Self>) {
        if self.placed == Some(bounds) {
            return;
        }
        self.placed = Some(bounds);
        let Some(webview) = &self.webview else { return };
        #[cfg(target_os = "macos")]
        if let Some(composed) = &self.composed {
            // While its slot changes size (the sidebar dragged), the page keeps its size and is scaled to fit:
            // laying the game out again every frame is what made the drag stutter. It takes the new size once
            // the slot holds still.
            if composed.place(webview, bounds, scale) {
                self._settle = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(RESIZE_SETTLE).await;
                    _ = this.update(cx, |frame, _| frame.settle());
                }));
            }
            return self.apply_visibility();
        }
        let _ = (scale, cx);
        let (origin, size) = (bounds.origin, bounds.size);
        _ = webview.set_bounds(wry::Rect {
            position: wry::dpi::LogicalPosition::new(f32::from(origin.x) as f64, f32::from(origin.y) as f64).into(),
            size: wry::dpi::LogicalSize::new(f32::from(size.width) as f64, f32::from(size.height) as f64).into(),
        });
        self.apply_visibility();
    }

    #[cfg(target_os = "macos")]
    fn settle(&mut self) {
        if let (Some(composed), Some(webview), Some(bounds)) = (&self.composed, &self.webview, self.placed) {
            composed.resize(webview, bounds.size);
        }
    }

    fn apply_visibility(&self) {
        let visible = self.shown && self.placed.is_some();
        let Some(webview) = &self.webview else { return };
        #[cfg(target_os = "macos")]
        if let Some(composed) = &self.composed {
            composed.set_visible(visible);
        }
        _ = webview.set_visible(visible);
    }
}

#[cfg(target_os = "macos")]
impl Drop for ActivityFrame {
    // The webview goes with the frame; its surface is taken out of the window's composition after it.
    fn drop(&mut self) {
        self.webview = None;
        if let Some(composed) = self.composed.take() {
            composed.release_later();
        }
    }
}

// The frame takes the bounds its slot prepaints at; the webview follows them. Placed at prepaint, as
// gpui-webview does: a native surface positioned then stacks with the view that hosts it.
impl Render for ActivityFrame {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.weak_entity();
        div().relative().size_full().child(
            canvas(
                move |bounds, window, cx| {
                    let scale = window.scale_factor();
                    _ = this.update(cx, |frame, cx| frame.place(bounds, scale, cx));
                },
                |_, _, _, _| {},
            )
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

#[cfg(target_os = "macos")]
mod compose {
    use anyhow::{Context as _, anyhow, ensure};
    use std::cell::Cell;

    use gpui_kit::{AnyWindowHandle, App, AsyncApp, Bounds, Pixels, Size, Window, WindowCompositionSurface};
    use objc2::{msg_send, runtime::AnyObject};
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use wry::WebViewExtMacOS;

    pub(super) struct Composed {
        surface: WindowCompositionSurface,
        // The surface's NSView, holding the WKWebView.
        container: usize,
        // The size the page is laid out at; the container scales it to the slot until they match.
        laid_out: Cell<Option<Size<Pixels>>>,
        window: AnyWindowHandle,
        cx: AsyncApp,
    }

    // Moves the WKWebView into a native surface of `window`'s composition. None (logged) where the window
    // can't compose: the webview stays on the window, over everything.
    pub(super) fn attach(webview: &wry::WebView, window: &Window, cx: &App) -> Option<Composed> {
        let attached = (|| {
            let composition = window.enable_window_composition()?;
            let surface = composition.create_native_surface()?;
            let added = (|| {
                let handle = surface.platform_surface()?.platform_handle()?;
                let parent = *handle.downcast::<usize>().map_err(|_| anyhow!("the surface is not an AppKit view"))?;
                ensure!(parent != 0, "the surface has no AppKit view");
                let view = webview.webview();
                // SAFETY: the surface's NSView and Wry's WKWebView are live, on the main thread.
                unsafe {
                    let _: () = msg_send![parent as *mut AnyObject, addSubview: &*view];
                    // Wry makes the webview follow its parent's size, which would lay the page out again on
                    // every frame of a resize; it takes its size from `resize` alone.
                    let _: () = msg_send![&*view, setAutoresizingMask: 0usize];
                    let _: () = msg_send![parent as *mut AnyObject, setAutoresizesSubviews: false];
                }
                Ok(parent)
            })();
            match added {
                Ok(container) => anyhow::Ok((surface, container)),
                Err(error) => {
                    composition.remove_surface(surface.id())?;
                    Err(error)
                }
            }
        })();
        match attached {
            Ok((surface, container)) => Some(Composed { surface, container, laid_out: Cell::new(None), window: window.window_handle(), cx: cx.to_async() }),
            Err(error) => {
                eprintln!("activity frame: no composition, the game covers overlays: {error:#}");
                None
            }
        }
    }

    impl Composed {
        // The surface takes the slot's window bounds. The first time, the page is laid out at its size; after
        // that a new size only scales the page, and true when it did: `resize` lays it out once it settles.
        pub(super) fn place(&self, webview: &wry::WebView, bounds: Bounds<Pixels>, scale: f32) -> bool {
            let Ok(surface) = self.surface.platform_surface() else { return false };
            _ = surface.set_bounds(bounds.to_device_pixels(scale));
            match self.laid_out.get() {
                None => {
                    self.resize(webview, bounds.size);
                    false
                }
                Some(laid_out) => {
                    // The container's frame is the slot; its bounds the page's size, which AppKit scales to it.
                    self.set_container_bounds(laid_out);
                    laid_out != bounds.size
                }
            }
        }

        pub(super) fn resize(&self, webview: &wry::WebView, size: Size<Pixels>) {
            self.laid_out.set(Some(size));
            let view = webview.webview();
            // SAFETY: Wry's live WKWebView, on the main thread; its frame is local to the surface's view.
            unsafe {
                let _: () = msg_send![&*view, setFrame: NSRect::new(NSPoint::new(0., 0.), ns_size(size))];
            }
            self.set_container_bounds(size);
        }

        fn set_container_bounds(&self, size: Size<Pixels>) {
            // SAFETY: the surface's live NSView, on the main thread.
            unsafe {
                let _: () = msg_send![self.container as *mut AnyObject, setBoundsSize: ns_size(size)];
            }
        }

        pub(super) fn set_visible(&self, visible: bool) {
            if let Ok(surface) = self.surface.platform_surface() {
                _ = surface.set_visible(visible);
            }
        }

        // Takes the surface out of its window's composition, which draws again so the pointer reaches what
        // was under it.
        pub(super) fn release(self, cx: &mut App) {
            let id = self.surface.id();
            _ = self.window.update(cx, |_, window, _| remove(window, id));
        }

        // `release` from where the app may be borrowed (a drop): on the next turn of the main thread.
        pub(super) fn release_later(self) {
            let (id, window) = (self.surface.id(), self.window);
            self.cx
                .spawn(async move |cx| _ = window.update(cx, |_, window, _| remove(window, id)))
                .detach();
        }
    }

    fn ns_size(size: Size<Pixels>) -> NSSize {
        NSSize::new(f32::from(size.width) as f64, f32::from(size.height) as f64)
    }

    fn remove(window: &mut Window, id: gpui_kit::CompositionSurfaceId) {
        let removed = window.enable_window_composition().and_then(|c| c.remove_surface(id)).context("removing the activity's surface");
        if let Err(error) = removed {
            eprintln!("activity frame: {error:#}");
        }
        window.refresh();
    }
}
