// Window material behind the sidebar: AppKit's sidebar glass on macOS 27, where the Appearance setting
// turns it on. Off, in full screen and everywhere else (Windows included), the sidebar is solid.
use std::cell::Cell;

use gpui_kit::WindowBackgroundAppearance;

thread_local! {
    // Whether this frame draws over the native material; set by `sync_sidebar_material` at the top of
    // each frame, before anything reads it.
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
}

// What the system calls its sidebar material, or None where Waffle can't show one (the setting is hidden).
pub fn setting_name() -> Option<&'static str> {
    if !supported() {
        return None;
    }
    Some("Liquid Glass sidebar")
}

pub fn window_background() -> WindowBackgroundAppearance {
    match supported() {
        true => WindowBackgroundAppearance::Transparent,
        false => WindowBackgroundAppearance::Opaque,
    }
}

// Whether this frame's sidebar is unpainted over the native material.
pub fn active() -> bool {
    ACTIVE.get()
}

// Called first thing each frame: decides whether the material shows (supported, turned on, not in full
// screen) and keeps the native side in step.
pub fn sync_sidebar_material(window: &mut gpui_kit::Window, width: f32, dark: bool, enabled: bool) {
    if !supported() {
        return;
    }
    let on = enabled && !window.is_fullscreen();
    ACTIVE.set(on);
    // `width` is in layout units; AppKit's sidebar is sized in window points, which zoom scales.
    apply(window, on, width * window.zoom(), dark);
}

pub fn sidebar_fill(cx: &gpui_kit::App) -> gpui_kit::Hsla {
    behind_sidebar(crate::palette::surface(crate::palette::Level::Sidebar, cx))
}

// A colour that stands in for the surface behind something on the sidebar (a cutout ring, an edge wash):
// over the native material, nothing.
pub fn behind_sidebar(opaque: gpui_kit::Hsla) -> gpui_kit::Hsla {
    match active() {
        true => gpui_kit::transparent_black(),
        false => opaque,
    }
}

// A fill lifted off the sidebar (server tiles): over the native material, the card wash; on the solid
// sidebar, the opaque step it always had.
pub fn lifted(opaque: gpui_kit::Hsla, cx: &gpui_kit::App) -> gpui_kit::Hsla {
    match active() {
        true => card(cx),
        false => opaque,
    }
}

// The sidebar's cards (the user and voice panel, the search button): a white wash in both modes, so the
// surface under them carries through and they read lighter in both themes.
pub fn card(cx: &gpui_kit::App) -> gpui_kit::Hsla {
    use gpui_kit::component::ActiveTheme as _;
    gpui_kit::white().opacity(if cx.theme().is_dark() { 0.08 } else { 0.55 })
}

#[cfg(not(target_os = "macos"))]
fn supported() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
fn apply(_: &mut gpui_kit::Window, _: bool, _: f32, _: bool) {}

// AppKit's sidebar glass arrived in macOS 27; earlier, the sidebar item's background isn't the look.
#[cfg(target_os = "macos")]
fn supported() -> bool {
    objc2_foundation::NSProcessInfo::processInfo().operatingSystemVersion().majorVersion >= 27
}

// AppKit's own sidebar: an NSSplitViewController whose sidebar item is pinned to the rail and channel
// list's width, under GPUI's view, so the system draws the sidebar background (its glass, tinted by the
// wallpaper and not the windows behind). Claude's desktop app does the same. Made on first use and taken
// out of the window while off: even hidden, it sends clicks on full screen's traffic lights to the content
// under them. One main window, so one split view.
#[cfg(target_os = "macos")]
fn apply(window: &mut gpui_kit::Window, on: bool, width: f32, dark: bool) {
    use objc2::{
        msg_send,
        runtime::{AnyClass, AnyObject},
    };
    use objc2_foundation::NSRect;
    use raw_window_handle::RawWindowHandle;
    #[derive(Clone, Copy)]
    struct Native {
        sidebar: *mut AnyObject,
        view: *mut AnyObject,
        content: *mut AnyObject,
        width: f32,
        dark: bool,
        on: bool,
    }
    thread_local! {
        static NATIVE: Cell<Option<Native>> = const { Cell::new(None) };
    }
    if let Some(mut native) = NATIVE.get() {
        if native.width != width {
            pin(native.sidebar, width);
            // Resizing the sidebar item lays out the title bar, which puts the traffic lights back where
            // AppKit keeps them; gpui only moves them again on its next window event, so they jumped
            // while the list was dragged. Lay out now and move them back in the same pass.
            // SAFETY: the split view made below, on the main thread.
            unsafe {
                let ns_window: *mut AnyObject = msg_send![native.content, window];
                let _: () = msg_send![ns_window, layoutIfNeeded];
            }
            window.set_traffic_light_position(crate::layout::traffic_lights(window.zoom()));
        }
        if native.dark != dark {
            match_theme(window, dark);
        }
        if native.on != on {
            attach(native.view, native.content, on);
        }
        (native.width, native.dark, native.on) = (width, dark, on);
        NATIVE.set(Some(native));
        return;
    }
    match_theme(window, dark);
    let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window) else { return };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return };
    let ns_view = handle.ns_view.as_ptr() as *mut AnyObject;
    let class = |name: &std::ffi::CStr| AnyClass::get(name).expect("AppKit class");
    // SAFETY: AppKit calls on the main thread, where GPUI renders. The controller is never released: it
    // lives as long as the window, and holds its view while that is out of the window.
    unsafe {
        let ns_window: *mut AnyObject = msg_send![ns_view, window];
        // GPUI's transparent background keeps its layer clear over the sidebar; an opaque window with the
        // system window colour behind it keeps other windows from showing through, leaving only the
        // wallpaper tint AppKit gives that colour.
        let backing: *mut AnyObject = msg_send![class(c"NSColor"), windowBackgroundColor];
        let _: () = msg_send![ns_window, setBackgroundColor: backing];
        let _: () = msg_send![ns_window, setOpaque: true];
        let content: *mut AnyObject = msg_send![ns_window, contentView];
        let bounds: NSRect = msg_send![content, bounds];
        let pane = |frame: NSRect| -> *mut AnyObject {
            let controller: *mut AnyObject = msg_send![class(c"NSViewController"), new];
            let view: *mut AnyObject = msg_send![class(c"NSView"), alloc];
            let view: *mut AnyObject = msg_send![view, initWithFrame: frame];
            let _: () = msg_send![controller, setView: view];
            controller
        };
        let split: *mut AnyObject = msg_send![class(c"NSSplitViewController"), new];
        let sidebar: *mut AnyObject = msg_send![class(c"NSSplitViewItem"), sidebarWithViewController: pane(bounds)];
        let _: () = msg_send![sidebar, setCanCollapse: false];
        let rest: *mut AnyObject = msg_send![class(c"NSSplitViewItem"), splitViewItemWithViewController: pane(bounds)];
        let _: () = msg_send![split, addSplitViewItem: sidebar];
        let _: () = msg_send![split, addSplitViewItem: rest];
        pin(sidebar, width);
        let view: *mut AnyObject = msg_send![split, view];
        let _: () = msg_send![view, setFrame: bounds];
        // NSViewWidthSizable | NSViewHeightSizable
        let _: () = msg_send![view, setAutoresizingMask: 2usize | 16usize];
        attach(view, content, on);
        NATIVE.set(Some(Native { sidebar, view, content, width, dark, on }));
    }
}

// Puts the split view under GPUI's view, sized to the window, or takes it out. The controller keeps the
// view alive while it's out.
#[cfg(target_os = "macos")]
fn attach(view: *mut objc2::runtime::AnyObject, content: *mut objc2::runtime::AnyObject, on: bool) {
    use objc2::{msg_send, runtime::AnyObject};
    use objc2_foundation::NSRect;
    // SAFETY: the split view and the window's content view, on the main thread.
    unsafe {
        if !on {
            let _: () = msg_send![view, removeFromSuperview];
            return;
        }
        let bounds: NSRect = msg_send![content, bounds];
        let _: () = msg_send![view, setFrame: bounds];
        let nil: *mut AnyObject = std::ptr::null_mut();
        let _: () = msg_send![content, addSubview: view, positioned: -1isize, relativeTo: nil];
    }
}

// AppKit draws the sidebar and the window's backing for the window's appearance, which follows the system
// unless set: a light theme on a dark system would put dark ink on a dark sidebar.
#[cfg(target_os = "macos")]
fn match_theme(window: &gpui_kit::Window, dark: bool) {
    use objc2::{
        msg_send,
        runtime::{AnyClass, AnyObject},
    };
    use objc2_foundation::NSString;
    use raw_window_handle::RawWindowHandle;
    let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window) else { return };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return };
    let ns_view = handle.ns_view.as_ptr() as *mut AnyObject;
    let name = NSString::from_str(if dark { "NSAppearanceNameDarkAqua" } else { "NSAppearanceNameAqua" });
    // SAFETY: AppKit calls on the main thread, where GPUI renders.
    unsafe {
        let ns_window: *mut AnyObject = msg_send![ns_view, window];
        let class = AnyClass::get(c"NSAppearance").expect("NSAppearance");
        let appearance: *mut AnyObject = msg_send![class, appearanceNamed: &*name];
        let _: () = msg_send![ns_window, setAppearance: appearance];
    }
}

#[cfg(target_os = "macos")]
fn pin(item: *mut objc2::runtime::AnyObject, width: f32) {
    use objc2::msg_send;
    // SAFETY: a live NSSplitViewItem, on the main thread.
    unsafe {
        let _: () = msg_send![item, setMinimumThickness: width as f64];
        let _: () = msg_send![item, setMaximumThickness: width as f64];
    }
}

#[cfg(not(target_os = "macos"))]
pub fn system_reduce_motion() -> bool {
    false
}

#[cfg(target_os = "macos")]
pub fn system_reduce_motion() -> bool {
    use objc2::{
        msg_send,
        runtime::{AnyClass, AnyObject},
    };
    // SAFETY: a read-only NSWorkspace property.
    unsafe {
        let class = AnyClass::get(c"NSWorkspace").expect("NSWorkspace");
        let workspace: *mut AnyObject = msg_send![class, sharedWorkspace];
        msg_send![workspace, accessibilityDisplayShouldReduceMotion]
    }
}

