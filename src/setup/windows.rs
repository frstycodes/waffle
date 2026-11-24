use std::{
    fs::File,
    io::{self, Read, Write},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::Ordering,
};

use windows::{
    Win32::{
        System::{
            Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile},
            Registry::{HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW},
        },
        UI::Shell::{FOLDERID_Desktop, FOLDERID_Programs, FOLDERID_RoamingAppData, FOLDERID_UserProgramFiles, IShellLinkW, KNOWN_FOLDER_FLAG, SHGetKnownFolderPath, ShellLink},
    },
    core::{GUID, HSTRING, Interface, PCWSTR},
};

use super::Progress;

// The key Inno Setup registered Waffle under, so installing over an old install replaces its Apps entry
// instead of adding a second one.
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{62BFCB0C-BB1C-47AE-A8E4-C16EEE4043D0}_is1";
const COPY_CHUNK: usize = 1 << 20;
// Keeps cmd's window from flashing up during uninstall.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// Copies this exe to %LOCALAPPDATA%\Programs\Waffle\Waffle.exe, adds the Start menu entry and the Apps
// entry, and returns the installed exe. The message on failure is shown to the user as is.
pub fn install(progress: &Progress) -> Result<PathBuf, String> {
    let source = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = install_dir().ok_or("Windows didn't say where apps for your account go.")?;
    let target = dir.join("Waffle.exe");
    let need = std::fs::metadata(&source).map(|m| m.len()).unwrap_or_default();
    let explain = |e: io::Error| match e.kind() {
        io::ErrorKind::StorageFull => format!("There isn't enough space on this drive. Free up {}\u{a0}MB, then try again.", need.div_ceil(1_000_000)),
        io::ErrorKind::PermissionDenied => format!("Windows wouldn't let Setup write to {}. Close anything using it, then try again.", dir.display()),
        _ => e.to_string(),
    };
    std::fs::create_dir_all(&dir).map_err(explain)?;
    // A running Waffle keeps its exe open, but Windows still lets it be renamed; the updater clears the
    // `.old` copy on Waffle's next start.
    if target.exists() {
        let aside = dir.join("Waffle.exe.old");
        let _ = std::fs::remove_file(&aside);
        std::fs::rename(&target, &aside).map_err(explain)?;
    }
    copy(&source, &target, progress).map_err(explain)?;
    // What Inno Setup left behind; its uninstaller would no longer match this install.
    for leftover in ["unins000.exe", "unins000.dat"] {
        let _ = std::fs::remove_file(dir.join(leftover));
    }
    let start_menu = known_folder(&FOLDERID_Programs).ok_or("Windows didn't say where the Start menu is.")?;
    // SAFETY: COM on this thread only, for the shortcut below.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        shortcut(&start_menu.join("Waffle.lnk"), &target).map_err(|e| e.message())?;
    }
    register(&target, &dir, need);
    Ok(target)
}

pub fn launch(exe: &Path) {
    if let Err(e) = Command::new(exe).current_dir(exe.parent().unwrap_or(exe)).spawn() {
        eprintln!("[setup] couldn't open {}: {e}", exe.display());
    }
}

// Windows' Apps list runs this (UninstallString). It removes the shortcuts and the Apps entry, then the
// install folder once this process has exited. Your data in %APPDATA% stays, as it did with Inno Setup.
pub fn uninstall() {
    let Some(dir) = std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.to_path_buf())) else { return };
    // Only the folder Setup installs into is ever deleted, never wherever a stray copy ran from.
    let Some(installed) = install_dir() else { return };
    if !same_path(&dir, &installed) {
        return;
    }
    let pinned = known_folder(&FOLDERID_RoamingAppData).map(|data| data.join(r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar"));
    for folder in [known_folder(&FOLDERID_Programs), known_folder(&FOLDERID_Desktop), pinned].into_iter().flatten() {
        let _ = std::fs::remove_file(folder.join("Waffle.lnk"));
    }
    // SAFETY: a plain registry call on our own key.
    unsafe {
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(UNINSTALL_KEY));
    }
    // The running exe can't delete its own folder: cmd waits a moment for this process to exit, then does.
    let removal = format!("/c ping -n 3 127.0.0.1 >nul & rmdir /s /q \"{}\"", dir.display());
    let _ = Command::new("cmd").raw_arg(removal).creation_flags(CREATE_NO_WINDOW).spawn();
}

// %LOCALAPPDATA%\Programs\Waffle: per user, so no admin prompt, and the updater can replace Waffle.exe in place.
fn install_dir() -> Option<PathBuf> {
    known_folder(&FOLDERID_UserProgramFiles).map(|programs| programs.join("Waffle"))
}

fn copy(from: &Path, to: &Path, progress: &Progress) -> io::Result<()> {
    let mut source = File::open(from)?;
    progress.total.store(source.metadata()?.len(), Ordering::Relaxed);
    let mut target = File::create(to)?;
    let mut buffer = vec![0; COPY_CHUNK];
    let mut done = 0;
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        target.write_all(&buffer[..read])?;
        done += read as u64;
        progress.done.store(done, Ordering::Relaxed);
    }
    target.sync_all()
}

unsafe fn shortcut(link: &Path, target: &Path) -> windows::core::Result<()> {
    // SAFETY: the caller initialised COM on this thread.
    unsafe {
        let shell: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        shell.SetPath(&HSTRING::from(target.to_string_lossy().as_ref()))?;
        if let Some(dir) = target.parent() {
            shell.SetWorkingDirectory(&HSTRING::from(dir.to_string_lossy().as_ref()))?;
        }
        let file: IPersistFile = shell.cast()?;
        file.Save(&HSTRING::from(link.to_string_lossy().as_ref()), true)
    }
}

// The Apps list entry. GAP: DisplayVersion is the version installed here; a self-update doesn't rewrite it.
fn register(exe: &Path, dir: &Path, bytes: u64) {
    let exe = exe.to_string_lossy();
    let text = [
        ("DisplayName", "Waffle".to_string()),
        ("DisplayVersion", env!("CARGO_PKG_VERSION").to_string()),
        ("Publisher", "frstycodes".to_string()),
        ("DisplayIcon", format!("{exe},0")),
        ("InstallLocation", dir.to_string_lossy().into_owned()),
        ("UninstallString", format!("\"{exe}\" --uninstall")),
        ("QuietUninstallString", format!("\"{exe}\" --uninstall")),
    ];
    let numbers = [("NoModify", 1), ("NoRepair", 1), ("EstimatedSize", (bytes / 1024) as u32)];
    // SAFETY: plain registry calls on a key this function opens and closes.
    unsafe {
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(UNINSTALL_KEY));
        let mut key = HKEY::default();
        if RegCreateKeyExW(HKEY_CURRENT_USER, &HSTRING::from(UNINSTALL_KEY), None, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut key, None).is_err() {
            return;
        }
        for (name, value) in text {
            let wide: Vec<u8> = value.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
            let _ = RegSetValueExW(key, &HSTRING::from(name), None, REG_SZ, Some(&wide));
        }
        for (name, value) in numbers {
            let _ = RegSetValueExW(key, &HSTRING::from(name), None, REG_DWORD, Some(&value.to_le_bytes()));
        }
        let _ = RegCloseKey(key);
    }
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

fn same_path(a: &Path, b: &Path) -> bool {
    let normal = |p: &Path| p.to_string_lossy().trim_end_matches('\\').to_lowercase();
    normal(a) == normal(b)
}
