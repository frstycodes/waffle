use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicIsize, Ordering},
};

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile, STGM_READWRITE},
        UI::{
            HiDpi::GetDpiForWindow,
            Shell::{FOLDERID_Desktop, FOLDERID_Programs, FOLDERID_RoamingAppData, IShellLinkW, KNOWN_FOLDER_FLAG, SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify, SHGetKnownFolderPath, ShellLink},
            WindowsAndMessaging::{CreateIconFromResourceEx, DestroyIcon, HICON, ICON_BIG, ICON_SMALL, LR_DEFAULTCOLOR, SendMessageW, WM_SETICON},
        },
    },
    core::{GUID, HSTRING, Interface, PCWSTR},
};

// The main window, once `attach` has run.
static WINDOW: AtomicIsize = AtomicIsize::new(0);
// Written beside Waffle.exe; the installer's [UninstallDelete] removes it.
const ICO_NAME: &str = "Waffle-icon.ico";
// PNG-in-ICO resources need this version (Vista and later).
const ICON_RESOURCE_VERSION: u32 = 0x0003_0000;

pub fn attach(hwnd: HWND, png: &[u8]) {
    WINDOW.store(hwnd.0 as isize, Ordering::Relaxed);
    set_window_icon(png);
}

// The title bar, Alt+Tab and an unpinned taskbar button. The exe's built-in icon stays the class icon.
pub fn set_window_icon(png: &[u8]) {
    let hwnd = HWND(WINDOW.load(Ordering::Relaxed) as *mut std::ffi::c_void);
    if hwnd.is_invalid() {
        return;
    }
    // SAFETY: `hwnd` is the main window, alive for the app's life; each icon replaced by WM_SETICON was
    // one this function created, so destroying it is ours to do.
    unsafe {
        let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.;
        for (which, base) in [(ICON_BIG, 32.), (ICON_SMALL, 16.)] {
            let px = (base * scale).round() as u32;
            let Some(sized) = super::png_at(png, px) else { continue };
            let Ok(icon) = CreateIconFromResourceEx(&sized, true, ICON_RESOURCE_VERSION, px as i32, px as i32, LR_DEFAULTCOLOR) else { continue };
            let old = SendMessageW(hwnd, WM_SETICON, Some(WPARAM(which as usize)), Some(LPARAM(icon.0 as isize)));
            if old.0 != 0 {
                let _ = DestroyIcon(HICON(old.0 as *mut std::ffi::c_void));
            }
        }
    }
}

// Start menu, desktop and taskbar-pin shortcuts, which show while Waffle is closed. `None` points them
// back at the exe's own icon. Explorer may keep a cached pin icon until it restarts.
pub fn set_shortcut_icons(png: Option<&'static [u8]>) {
    let Some(exe) = std::env::current_exe().ok() else { return };
    let _ = std::thread::Builder::new().name("shortcut-icons".into()).spawn(move || {
        let ico = exe.with_file_name(ICO_NAME);
        let icon = match png {
            Some(png) => {
                let Some(bytes) = super::ico(png) else { return };
                if std::fs::write(&ico, bytes).is_err() {
                    return;
                }
                ico
            }
            // Nothing was ever customised: leave the shortcuts alone.
            None if !ico.exists() => return,
            None => {
                let _ = std::fs::remove_file(&ico);
                exe
            }
        };
        // SAFETY: COM on this thread only; the link objects live and die inside `retarget`.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            for link in shortcuts() {
                let _ = retarget(&link, &icon);
            }
            SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
        }
    });
}

fn shortcuts() -> Vec<PathBuf> {
    let pinned = known_folder(&FOLDERID_RoamingAppData).map(|data| data.join(r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar"));
    [known_folder(&FOLDERID_Programs), known_folder(&FOLDERID_Desktop), pinned].into_iter().flatten().map(|dir| dir.join("Waffle.lnk")).filter(|link| link.exists()).collect()
}

fn known_folder(id: &GUID) -> Option<PathBuf> {
    // SAFETY: the returned string is freed with CoTaskMemFree, as SHGetKnownFolderPath requires.
    unsafe {
        let path = SHGetKnownFolderPath(id, KNOWN_FOLDER_FLAG(0), None).ok()?;
        let text = path.to_string().ok();
        CoTaskMemFree(Some(path.0 as *const std::ffi::c_void));
        text.map(PathBuf::from)
    }
}

unsafe fn retarget(link: &Path, icon: &Path) -> windows::core::Result<()> {
    // SAFETY: the caller initialised COM on this thread.
    unsafe {
        let shell: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let file: IPersistFile = shell.cast()?;
        file.Load(&HSTRING::from(link.to_string_lossy().as_ref()), STGM_READWRITE)?;
        shell.SetIconLocation(&HSTRING::from(icon.to_string_lossy().as_ref()), 0)?;
        file.Save(PCWSTR::null(), true)
    }
}
