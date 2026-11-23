// Windows: closing the window keeps Waffle running in the notification area, like Discord. The
// tray icon brings the window back (click or double-click); its menu offers Open and Quit. Calls,
// Go Live and notifications carry on while the window is hidden.
use std::{cell::RefCell, sync::OnceLock, time::Duration};

use gpui_kit::{App, Window};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tray_icon::{
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};
use windows::{
    Win32::{
        Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, WAIT_OBJECT_0},
        System::Threading::{CreateEventW, SetEvent, WaitForSingleObject},
        UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow, IsIconic, IsWindowVisible, SW_HIDE, SW_MINIMIZE, SW_RESTORE, SW_SHOW, SetForegroundWindow, ShowWindow},
    },
    core::HSTRING,
};

const ICON_PX: u32 = 32;
// Tray and menu events arrive on channels; checked this often on the main thread.
const POLL: Duration = Duration::from_millis(120);

// The running Waffle's activation event, as a raw handle (HANDLE isn't Sync).
static ACTIVATE: OnceLock<usize> = OnceLock::new();

thread_local! {
    // The tray icon lives as long as the app; it must stay on the UI thread that created it.
    static TRAY: RefCell<Option<TrayIcon>> = const { RefCell::new(None) };
}

pub fn install(window: &mut Window, cx: &mut App) {
    let Some(hwnd) = hwnd(window) else { return };
    crate::presence::watch(hwnd);
    watch_activation(hwnd, cx);
    // Registered before the tray is built: with no close handler gpui lets Windows destroy the window,
    // and closing the last window quits. The close button, Alt+F4 and the taskbar's Close all ask
    // here; Waffle hides to the tray, or minimizes when there is no tray icon to bring it back from.
    window.on_window_should_close(cx, move |_, _| {
        let to = if TRAY.with(|t| t.borrow().is_some()) { SW_HIDE } else { SW_MINIMIZE };
        // SAFETY: `hwnd` is this window's handle, valid while the window exists (it isn't closed here).
        unsafe {
            let _ = ShowWindow(hwnd, to);
        }
        false
    });

    let open = MenuItem::new("Open Waffle", true, None);
    let quit = MenuItem::new("Quit Waffle", true, None);
    let menu = Menu::new();
    if menu.append_items(&[&open, &PredefinedMenuItem::separator(), &quit]).is_err() {
        return;
    }
    let mut builder = TrayIconBuilder::new().with_tooltip("Waffle").with_menu(Box::new(menu)).with_menu_on_left_click(false);
    if let Some(icon) = icon(crate::settings::Prefs::load().app_icon.bytes()) {
        builder = builder.with_icon(icon);
    }
    let tray = match builder.build() {
        Ok(tray) => tray,
        Err(e) => return eprintln!("tray: {e}"),
    };
    TRAY.with(|t| *t.borrow_mut() = Some(tray));
    // Dropping the icon removes it; left to process exit, a dead icon stays until hovered.
    cx.on_app_quit(|_| {
        TRAY.with(|t| t.borrow_mut().take());
        async {}
    })
    // lint-allow(detached-task): an app-quit subscription, with no view to drop; it lives as long as the process.
    .detach();

    let (open_id, quit_id) = (open.id().clone(), quit.id().clone());
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(POLL).await;
            while let Ok(event) = TrayIconEvent::receiver().try_recv() {
                let activate = matches!(
                    event,
                    TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. }
                );
                if activate {
                    show(hwnd);
                }
            }
            while let Ok(event) = MenuEvent::receiver().try_recv() {
                if event.id == open_id {
                    show(hwnd);
                } else if event.id == quit_id {
                    TRAY.with(|t| t.borrow_mut().take());
                    cx.update(|cx| cx.quit());
                    return;
                }
            }
        }
    })
    // lint-allow(detached-task): polls the tray for the life of the process and ends on Quit; there is no view to drop.
    .detach();
}

// One Waffle per executable: a second launch (the taskbar pin while Waffle sits in the tray) signals
// the running one to show its window, then exits. Keyed by the exe path, so a dev build runs beside
// the installed one. Returns false when another Waffle took the hand-off. The event dies with the
// process, and gpui's restart relaunches only after exit, so a restart is never turned away.
pub fn claim_single_instance() -> bool {
    let exe = std::env::current_exe().map(|p| p.to_string_lossy().to_lowercase().replace('\\', "/")).unwrap_or_default();
    let name = HSTRING::from(format!("Local\\Waffle.Activate.{exe}"));
    // SAFETY: plain Win32 calls; the handle is kept for the life of the process.
    unsafe {
        let Ok(event) = CreateEventW(None, false, false, &name) else { return true };
        if GetLastError() != ERROR_ALREADY_EXISTS {
            ACTIVATE.get_or_init(|| event.0 as usize);
            return true;
        }
        // The running Waffle may only take the foreground if the launching process allows it.
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        let _ = SetEvent(event);
    }
    false
}

fn watch_activation(hwnd: HWND, cx: &mut App) {
    let Some(&event) = ACTIVATE.get() else { return };
    cx.spawn(async move |cx| {
        // Until the splash brings the main window forward it loads hidden behind it, and showing it
        // early would put a half-loaded window on screen; the splash is already in view then.
        let mut loaded = false;
        loop {
            cx.background_executor().timer(POLL).await;
            // SAFETY: our own window, and the event handle stays open for the life of the process.
            unsafe {
                loaded |= IsWindowVisible(hwnd).as_bool();
                if WaitForSingleObject(HANDLE(event as *mut _), 0) == WAIT_OBJECT_0 && loaded {
                    show(hwnd);
                }
            }
        }
    })
    // lint-allow(detached-task): polls for a second launch for the life of the process; there is no view to drop.
    .detach();
}

fn show(hwnd: HWND) {
    // SAFETY: plain Win32 calls on our own top-level window.
    unsafe {
        let _ = ShowWindow(hwnd, if IsIconic(hwnd).as_bool() { SW_RESTORE } else { SW_SHOW });
        let _ = SetForegroundWindow(hwnd);
    }
}

fn hwnd(window: &Window) -> Option<HWND> {
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(HWND(handle.hwnd.get() as *mut std::ffi::c_void)),
        _ => None,
    }
}

// Shows `png` (an app icon, see app_icon.rs) in the tray, when the icon is picked in Settings.
pub fn set_icon(png: &[u8]) {
    let Some(icon) = icon(png) else { return };
    TRAY.with(|t| {
        if let Some(tray) = t.borrow().as_ref() {
            let _ = tray.set_icon(Some(icon));
        }
    });
}

// An app icon's 512px PNG scaled down for the notification area.
fn icon(png: &[u8]) -> Option<Icon> {
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?.to_rgba8();
    let small = image::imageops::resize(&image, ICON_PX, ICON_PX, image::imageops::FilterType::Lanczos3);
    Icon::from_rgba(small.into_raw(), ICON_PX, ICON_PX).ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_app_icon_makes_a_tray_icon() {
        for (path, png) in crate::app_icon::BRAND_ICONS {
            assert!(super::icon(png).is_some(), "{path}");
        }
    }
}
