// Whether Waffle is in front of the user, and which parts of its window can actually be seen.
//
// On Windows a thread listens for the system's own events: our window minimized or restored, hidden
// (closed to the tray) or shown, and the foreground window changing. Each one is handled at once.
// While the window is on screen the thread also samples twice a second, because nothing reports a
// window covering ours: its visible region is worked out by taking the client area and cutting away
// every window stacked above it. While minimized or in the tray it doesn't poll at all, only waits. Windows that only look like they cover us are skipped: cloaked ones (other
// virtual desktops, suspended UWP apps) and click-through overlays (WS_EX_TRANSPARENT, such as game
// and recording overlays), which are usually see-through.
//
// The UI uses it to pause animation nobody can see (`may_animate`, `is_visible`) and to release
// memory while in the background (`background`). Elsewhere the window always counts as in front.
use gpui_kit::{Bounds, Pixels};

#[cfg(target_os = "windows")]
pub use windows_impl::watch;

use std::sync::Mutex;

use futures::channel::mpsc;

// Woken on every change (see `changes`).
static LISTENERS: Mutex<Vec<mpsc::UnboundedSender<()>>> = Mutex::new(Vec::new());

/// A stream that yields whenever focus, visibility or the visible region changes.
pub fn changes() -> mpsc::UnboundedReceiver<()> {
    let (send, receive) = mpsc::unbounded();
    if let Ok(mut listeners) = LISTENERS.lock() {
        listeners.push(send);
    }
    receive
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn announce() {
    if let Ok(mut listeners) = LISTENERS.lock() {
        listeners.retain(|l| l.unbounded_send(()).is_ok());
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::sync::{
        Mutex,
        atomic::{AtomicIsize, AtomicU64, AtomicUsize, Ordering},
    };

    use windows::Win32::{
        Foundation::{HWND, POINT, RECT},
        Graphics::{
            Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute},
            Gdi::{ClientToScreen, CombineRgn, CreateRectRgn, DeleteObject, GetRegionData, HRGN, NULLREGION, RGN_DIFF, RGNDATA, RGNDATAHEADER},
        },
        UI::{
            Accessibility::{HWINEVENTHOOK, SetWinEventHook},
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{
                EVENT_OBJECT_HIDE, EVENT_OBJECT_SHOW, EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART, GA_ROOTOWNER,
                GW_HWNDPREV, GWL_EXSTYLE, GetAncestor, GetClientRect, GetForegroundWindow, GetMessageW, GetWindow, GetWindowLongPtrW, GetWindowRect,
                IsIconic, IsWindowVisible, KillTimer, MSG, OBJID_WINDOW, SetTimer, WINEVENT_OUTOFCONTEXT, WM_TIMER, WS_EX_TRANSPARENT,
            },
        },
    };

    use super::Sample;

    // How often to look for windows covering ours while it's on screen.
    const EVERY_MS: u32 = 500;

    pub(super) static STATE: Mutex<Option<Sample>> = Mutex::new(None);
    // Bumped whenever the sample changes, so the UI knows to redraw.
    pub(super) static GENERATION: AtomicU64 = AtomicU64::new(0);

    // The watched window, and the polling timer while one runs (0: none).
    static WINDOW: AtomicIsize = AtomicIsize::new(0);
    static TIMER: AtomicUsize = AtomicUsize::new(0);

    // Watches `hwnd`, the main window, for the rest of the app's life. The hooks deliver events to
    // this thread's message loop, so everything here runs on that one thread.
    pub fn watch(hwnd: HWND) {
        WINDOW.store(hwnd.0 as isize, Ordering::Relaxed);
        let _ = std::thread::Builder::new().name("presence".into()).spawn(|| {
            let pid = std::process::id();
            // SAFETY: plain hook registrations and a standard message loop on this thread.
            unsafe {
                let _ = SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(on_event), 0, 0, WINEVENT_OUTOFCONTEXT);
                let _ = SetWinEventHook(EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND, None, Some(on_event), pid, 0, WINEVENT_OUTOFCONTEXT);
                let _ = SetWinEventHook(EVENT_OBJECT_SHOW, EVENT_OBJECT_HIDE, None, Some(on_event), pid, 0, WINEVENT_OUTOFCONTEXT);
                resample();
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    if msg.message == WM_TIMER {
                        resample();
                    }
                }
            }
        });
    }

    unsafe extern "system" fn on_event(_: HWINEVENTHOOK, event: u32, hwnd: HWND, object: i32, _: i32, _: u32, _: u32) {
        let ours = hwnd.0 as isize == WINDOW.load(Ordering::Relaxed) && object == OBJID_WINDOW.0;
        if event == EVENT_SYSTEM_FOREGROUND || ours {
            resample();
        }
    }

    // Takes a fresh sample, tells listeners if anything changed, and polls only while on screen.
    fn resample() {
        let hwnd = HWND(WINDOW.load(Ordering::Relaxed) as *mut std::ffi::c_void);
        let sample = sample(hwnd);
        let shown = sample.shown;
        let changed = STATE.lock().is_ok_and(|mut state| {
            let changed = state.as_ref() != Some(&sample);
            *state = Some(sample);
            changed
        });
        if changed {
            GENERATION.fetch_add(1, Ordering::Relaxed);
            super::announce();
        }
        let timer = TIMER.load(Ordering::Relaxed);
        // SAFETY: thread timers on this thread's message queue.
        unsafe {
            if shown && timer == 0 {
                TIMER.store(SetTimer(None, 0, EVERY_MS, None), Ordering::Relaxed);
            } else if !shown && timer != 0 {
                let _ = KillTimer(None, timer);
                TIMER.store(0, Ordering::Relaxed);
            }
        }
    }

    fn sample(hwnd: HWND) -> Sample {
        // SAFETY: read-only window queries; every region created here is deleted before returning.
        unsafe {
            let shown = IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool();
            let active = shown && GetForegroundWindow() == hwnd;
            let visible = if shown { visible_rects(hwnd) } else { Vec::new() };
            Sample { active, shown, visible }
        }
    }

    // The client area's visible parts, in logical pixels from its top-left corner.
    unsafe fn visible_rects(hwnd: HWND) -> Vec<[f32; 4]> {
        let mut client = RECT::default();
        if unsafe { GetClientRect(hwnd, &mut client) }.is_err() {
            return Vec::new();
        }
        let mut origin = POINT::default();
        let _ = unsafe { ClientToScreen(hwnd, &mut origin) };
        let region = unsafe { CreateRectRgn(origin.x, origin.y, origin.x + client.right, origin.y + client.bottom) };
        let mut above = unsafe { GetWindow(hwnd, GW_HWNDPREV) };
        while let Ok(window) = above {
            if covers(window, hwnd)
                && let Some(rect) = unsafe { frame(window) }
            {
                let cut = unsafe { CreateRectRgn(rect.left, rect.top, rect.right, rect.bottom) };
                let kind = unsafe { CombineRgn(Some(region), Some(region), Some(cut), RGN_DIFF) };
                let _ = unsafe { DeleteObject(cut.into()) };
                if kind == NULLREGION {
                    break;
                }
            }
            above = unsafe { GetWindow(window, GW_HWNDPREV) };
        }
        let scale = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.;
        let rects = unsafe { region_rects(region) }
            .into_iter()
            .map(|r| [(r.left - origin.x) as f32 / scale, (r.top - origin.y) as f32 / scale, (r.right - origin.x) as f32 / scale, (r.bottom - origin.y) as f32 / scale])
            .collect();
        let _ = unsafe { DeleteObject(region.into()) };
        rects
    }

    // A window above ours that hides what's under it.
    fn covers(window: HWND, ours: HWND) -> bool {
        // SAFETY: read-only queries on a window handle from the z-order walk.
        unsafe {
            if !IsWindowVisible(window).as_bool() || IsIconic(window).as_bool() {
                return false;
            }
            // Our own menus, tooltips and popups aren't in the way of anything.
            if GetAncestor(window, GA_ROOTOWNER) == ours {
                return false;
            }
            if GetWindowLongPtrW(window, GWL_EXSTYLE) as u32 & WS_EX_TRANSPARENT.0 != 0 {
                return false;
            }
            let mut cloaked = 0u32;
            let cloaked = DwmGetWindowAttribute(window, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, size_of::<u32>() as u32).is_ok() && cloaked != 0;
            !cloaked
        }
    }

    // The window as drawn: DWM's frame bounds leave out the invisible resize border around it.
    unsafe fn frame(window: HWND) -> Option<RECT> {
        let mut rect = RECT::default();
        let dwm = unsafe { DwmGetWindowAttribute(window, DWMWA_EXTENDED_FRAME_BOUNDS, &mut rect as *mut RECT as *mut _, size_of::<RECT>() as u32) };
        if dwm.is_err() {
            unsafe { GetWindowRect(window, &mut rect) }.ok()?;
        }
        (rect.right > rect.left && rect.bottom > rect.top).then_some(rect)
    }

    unsafe fn region_rects(region: HRGN) -> Vec<RECT> {
        let size = unsafe { GetRegionData(region, 0, None) } as usize;
        if size < size_of::<RGNDATAHEADER>() {
            return Vec::new();
        }
        // u64s keep the buffer aligned for the header.
        let mut buffer = vec![0u64; size.div_ceil(8)];
        let data = buffer.as_mut_ptr() as *mut RGNDATA;
        if unsafe { GetRegionData(region, size as u32, Some(data)) } == 0 {
            return Vec::new();
        }
        let count = unsafe { (*data).rdh.nCount } as usize;
        let rects = unsafe { (*data).Buffer.as_ptr() as *const RECT };
        (0..count).map(|i| unsafe { *rects.add(i) }).collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Sample {
    // The window has keyboard focus.
    active: bool,
    // Not hidden (in the tray) or minimized.
    shown: bool,
    // The parts of the client area no other window covers: [left, top, right, bottom], logical pixels.
    visible: Vec<[f32; 4]>,
}

#[cfg(target_os = "windows")]
fn current() -> Option<Sample> {
    windows_impl::STATE.lock().ok()?.clone()
}

#[cfg(not(target_os = "windows"))]
fn current() -> Option<Sample> {
    None
}

/// Changes whenever focus, visibility or the visible region does.
pub fn generation() -> u64 {
    #[cfg(target_os = "windows")]
    return windows_impl::GENERATION.load(std::sync::atomic::Ordering::Relaxed);
    #[cfg(not(target_os = "windows"))]
    0
}

/// Animations (GIFs) play only while the window is focused, as the user asked.
pub fn may_animate() -> bool {
    current().is_none_or(|s| s.active)
}

/// Whether any of `bounds` (window coordinates) is on screen, not covered by another window.
pub fn is_visible(bounds: Bounds<Pixels>) -> bool {
    current().is_none_or(|s| s.shown && overlaps(&s.visible, bounds))
}

fn overlaps(visible: &[[f32; 4]], bounds: Bounds<Pixels>) -> bool {
    let (left, top) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
    let (right, bottom) = (left + f32::from(bounds.size.width), top + f32::from(bounds.size.height));
    visible.iter().any(|[l, t, r, b]| left < *r && *l < right && top < *b && *t < bottom)
}

/// Nobody is using it: unfocused, hidden, minimized, or completely covered.
pub fn background() -> bool {
    current().is_some_and(|s| !s.active || !s.shown || s.visible.is_empty())
}

/// Minimized or closed to the tray.
pub fn hidden() -> bool {
    current().is_some_and(|s| !s.shown)
}

/// Nothing of the window can be seen: hidden (in the tray), minimized, or completely covered.
pub fn out_of_sight() -> bool {
    current().is_some_and(|s| !s.shown || s.visible.is_empty())
}

#[cfg(test)]
mod tests {
    use gpui_kit::{point, px, size};

    use super::*;

    #[test]
    fn region_overlap() {
        const CELL: f32 = 20.;
        const SMALL: f32 = 10.;
        // Visible: a strip across the top and a block on the right; the rest is covered.
        let visible = [[0., 0., 100., 50.], [80., 50., 100., 200.]];
        let at = |x: f32, y: f32| overlaps(&visible, Bounds { origin: point(px(x), px(y)), size: size(px(CELL), px(CELL)) });
        assert!(at(10., 10.));
        assert!(at(90., 40.), "partly visible counts");
        assert!(!at(10., 60.), "under the covering window");
        assert!(at(70., 120.), "touches the visible block");
        assert!(!overlaps(&[], Bounds { origin: Default::default(), size: size(px(SMALL), px(SMALL)) }), "fully covered");
    }
}
