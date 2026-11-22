// A listen-only keyboard hook: reports the keys held, system-wide, each time they change. macOS uses a
// CGEventTap (needs Input Monitoring permission), Windows a WH_KEYBOARD_LL hook; both pass every key
// on untouched.
use crate::hotkeys;
use futures::channel::mpsc::UnboundedSender;
use std::sync::Mutex;

struct Tracker {
    // Keycodes held, in the order pressed.
    codes: Vec<u16>,
    sent: Vec<&'static str>,
    tx: UnboundedSender<Vec<&'static str>>,
}

static TRACKER: Mutex<Option<Tracker>> = Mutex::new(None);

pub fn start(tx: UnboundedSender<Vec<&'static str>>) {
    let Ok(mut tracker) = TRACKER.lock() else { return };
    let first = tracker.is_none();
    *tracker = Some(Tracker { codes: Vec::new(), sent: Vec::new(), tx });
    if first {
        let _ = std::thread::Builder::new().name("key-hook".into()).spawn(platform::listen);
    }
}

// The hook is in: an empty list tells Hotkeys it's listening.
fn listening() {
    if let Ok(Some(tracker)) = TRACKER.lock().as_deref() {
        let _ = tracker.tx.unbounded_send(Vec::new());
    }
}

fn key(code: u16, pressed: bool) {
    if hotkeys::key_for_code(code).is_none() {
        return;
    }
    let Ok(mut tracker) = TRACKER.lock() else { return };
    let Some(t) = tracker.as_mut() else { return };
    if pressed && t.codes.contains(&code) {
        return;
    }
    // A release can go missing (macOS secure input hides keys from taps), so a press first drops keys
    // the system says are up.
    t.codes.retain(|&c| c != code && (!pressed || platform::is_down(c)));
    if pressed {
        t.codes.push(code);
    }
    let mut held: Vec<&'static str> = Vec::new();
    for name in t.codes.iter().filter_map(|&c| hotkeys::key_for_code(c)) {
        if !held.contains(&name) {
            held.push(name);
        }
    }
    if held != t.sent {
        t.sent = held.clone();
        let _ = t.tx.unbounded_send(held);
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::c_void;
    use std::ptr::null_mut;
    use std::sync::atomic::{AtomicPtr, Ordering};
    use std::time::Duration;

    type Ref = *mut c_void;
    type Callback = extern "C" fn(Ref, u32, Ref, Ref) -> Ref;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGPreflightListenEventAccess() -> bool;
        fn CGRequestListenEventAccess() -> bool;
        fn CGEventTapCreate(tap: u32, place: u32, options: u32, events: u64, callback: Callback, info: Ref) -> Ref;
        fn CGEventTapEnable(tap: Ref, enable: bool);
        fn CGEventGetIntegerValueField(event: Ref, field: u32) -> i64;
        fn CGEventGetFlags(event: Ref) -> u64;
        fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFRunLoopCommonModes: Ref;
        fn CFMachPortCreateRunLoopSource(allocator: Ref, port: Ref, order: isize) -> Ref;
        fn CFRunLoopGetCurrent() -> Ref;
        fn CFRunLoopAddSource(run_loop: Ref, source: Ref, mode: Ref);
        fn CFRunLoopRun();
    }

    const SESSION_TAP: u32 = 1;
    const HEAD_INSERT: u32 = 0;
    const LISTEN_ONLY: u32 = 1;
    const KEY_DOWN: u32 = 10;
    const KEY_UP: u32 = 11;
    const FLAGS_CHANGED: u32 = 12;
    const DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
    const DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
    const KEYCODE_FIELD: u32 = 9;
    const HID_STATE: i32 = 1;

    static TAP: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

    // Asks for Input Monitoring once, then waits until it's granted. Without it the tap is still made, but
    // macOS strips key presses from it and it hears only modifiers, so a made tap doesn't mean listening.
    pub fn listen() {
        if !unsafe { CGPreflightListenEventAccess() } {
            unsafe { CGRequestListenEventAccess() };
        }
        while !unsafe { CGPreflightListenEventAccess() } {
            std::thread::sleep(Duration::from_secs(2));
        }
        let mask = (1 << KEY_DOWN) | (1 << KEY_UP) | (1 << FLAGS_CHANGED);
        let tap = loop {
            let tap = unsafe { CGEventTapCreate(SESSION_TAP, HEAD_INSERT, LISTEN_ONLY, mask, on_event, null_mut()) };
            if !tap.is_null() {
                break tap;
            }
            std::thread::sleep(Duration::from_secs(2));
        };
        TAP.store(tap, Ordering::Relaxed);
        unsafe {
            let source = CFMachPortCreateRunLoopSource(null_mut(), tap, 0);
            CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
        }
        super::listening();
        unsafe { CFRunLoopRun() };
    }

    pub fn is_down(code: u16) -> bool {
        unsafe { CGEventSourceKeyState(HID_STATE, code) }
    }

    extern "C" fn on_event(_proxy: Ref, kind: u32, event: Ref, _info: Ref) -> Ref {
        if matches!(kind, DISABLED_BY_TIMEOUT | DISABLED_BY_USER_INPUT) {
            unsafe { CGEventTapEnable(TAP.load(Ordering::Relaxed), true) };
            return event;
        }
        let code = unsafe { CGEventGetIntegerValueField(event, KEYCODE_FIELD) } as u16;
        match kind {
            KEY_DOWN | KEY_UP => super::key(code, kind == KEY_DOWN),
            FLAGS_CHANGED => {
                if let Some(mask) = side_mask(code) {
                    super::key(code, unsafe { CGEventGetFlags(event) } & mask != 0);
                }
            }
            _ => {}
        }
        event
    }

    // The device-dependent flag for each modifier key (IOKit's NX_DEVICE*KEYMASK), which tells the
    // left and right keys apart.
    fn side_mask(code: u16) -> Option<u64> {
        Some(match code {
            0x3B => 0x1,
            0x38 => 0x2,
            0x3C => 0x4,
            0x37 => 0x8,
            0x36 => 0x10,
            0x3A => 0x20,
            0x3D => 0x40,
            0x3E => 0x2000,
            _ => return None,
        })
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, MSG, SetTimer, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN, WM_TIMER,
    };

    // Windows removes a low-level hook, silently and for good, the first time it answers later than
    // LowLevelHooksTimeout; a game pinning the CPU can starve this thread that long. So the thread runs
    // at top priority, and the hook is put back in every so often in case it was dropped anyway.
    const REHOOK_MS: u32 = 30_000;

    // The hook runs on this thread, so it pumps messages for as long as the app runs.
    pub fn listen() {
        let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL) };
        let Some(mut hook) = install() else { return };
        super::listening();
        unsafe { SetTimer(None, 0, REHOOK_MS, None) };
        let mut msg = MSG::default();
        while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
            if msg.message == WM_TIMER
                && let Some(fresh) = install()
            {
                let _ = unsafe { UnhookWindowsHookEx(hook) };
                hook = fresh;
            }
        }
    }

    fn install() -> Option<HHOOK> {
        let module = unsafe { GetModuleHandleW(None) }.ok().map(Into::into);
        unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(on_key), module, 0) }.ok()
    }

    pub fn is_down(code: u16) -> bool {
        (unsafe { GetAsyncKeyState(i32::from(code)) } as u16) & 0x8000 != 0
    }

    // Windows drops a hook that takes too long, so this only records the key.
    unsafe extern "system" fn on_key(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 {
            let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
            super::key(info.vkCode as u16, matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN));
        }
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }
}

// No hook on other systems yet; keybinds stay inert there.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    pub fn listen() {}

    pub fn is_down(_code: u16) -> bool {
        false
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use crate::hotkeys::{Action, Hotkeys, Keybind};
    use std::ffi::c_void;
    use std::time::{Duration, Instant};

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceCreate(state: i32) -> *mut c_void;
        fn CGEventCreateKeyboardEvent(source: *mut c_void, key: u16, down: bool) -> *mut c_void;
        fn CGEventPost(tap: u32, event: *mut c_void);
    }

    // Presses real (synthetic) keys, which reach whatever app is in front: put a harmless one there.
    // Needs Input Monitoring and Accessibility for the terminal running it (not a sandboxed agent shell,
    // whose taps hear nothing): cargo test -- --ignored hears_chords
    #[test]
    #[ignore]
    fn hears_chords_from_other_apps() {
        let (mut hotkeys, mut rx) = Hotkeys::start();
        let bind = |keys: &str, action| Keybind { action, keys: Some(keys.into()) };
        hotkeys.bind(&[bind("=+backspace", Action::PushToTalk), bind("[+\\", Action::ToggleMute)]);
        let mut fired = Vec::new();
        let mut drain = |hotkeys: &mut Hotkeys| {
            let until = Instant::now() + Duration::from_millis(300);
            while Instant::now() < until {
                match rx.try_recv() {
                    Ok(held) => fired.extend(hotkeys.update(held)),
                    _ => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        };
        drain(&mut hotkeys);
        assert!(hotkeys.listening, "no tap: is Input Monitoring granted?");
        let source = unsafe { CGEventSourceCreate(1) };
        // = (0x18), Backspace (0x33), [ (0x21), \ (0x2A): press both, release both.
        for (a, b) in [(0x18, 0x33), (0x21, 0x2A)] {
            for (key, down) in [(a, true), (b, true), (b, false), (a, false)] {
                unsafe { CGEventPost(0, CGEventCreateKeyboardEvent(source, key, down)) };
                std::thread::sleep(Duration::from_millis(30));
            }
        }
        drain(&mut hotkeys);
        assert_eq!(fired, [(Action::PushToTalk, true), (Action::PushToTalk, false), (Action::ToggleMute, true), (Action::ToggleMute, false)]);
    }
}
