// Game detection the way Discord's desktop client does it: Discord's list of detectable games, matched
// against the executable paths of running processes, and the game found published as your presence
// (a type-0 "Playing" activity with its `application_id`). As Discord's RunningGameStore (web client,
// 2026-11-21): apps on its non-games list and the streaming tools and Spotify it watches never count as
// games; games you turned detection off for are skipped; programs you added ("Add it!") count as games.
//
// The list is Discord's CDN copy, `/detectables/games.json` (unauthenticated, ~13 MB): each app has `executables`
// of `{ os: "win32" | "darwin" | "linux", name, is_launcher, arguments? }`. A name is a lowercase path
// suffix ("win64/cs2.exe", "factorio.app"); one starting with `>` names the process's own file and
// needs `arguments` in its command line (`>java` + "net.minecraft.client.main.Main"). Launchers never
// count. Verified against a live response (2026-11-19); the matching rules follow arRPC, an open
// reimplementation of the client's detection (not verified against Discord's own native module).
use std::{
    cell::{OnceCell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::Mutex,
    time::{Duration, SystemTime},
};

use futures::{StreamExt, channel::mpsc};
use gpui_kit::Context;
use serde::{Deserialize, Serialize};

use crate::{Waffle, discord::ActivityKind, query::Query};

// A game shows within a second of launching. Each scan lists processes (a few milliseconds) and
// matches only the ones new since the last.
// GAP: Discord's native module reports through a callback whose cadence isn't visible from its bundle.
const SCAN_EVERY: Duration = Duration::from_secs(1);
const CDN: &str = "https://cdn.discordapp.com";
// Discord's built-in observed apps (RunningGameStore): streaming tools and Spotify, never games.
const OBSERVED: [&str; 16] = [
    "obs/obs.exe", "obs32.exe", "obs64.exe", "obs.app", "xsplit.gamecaster.exe", "xsplit.core.exe", "gamecaster.exe", "bebo.exe",
    "streamlabs obs.exe", "streamlabs desktop.exe", "streamlabs desktop.app", "twitchstudio.exe", "twitch studio.app", "spotify.exe", "spotify.app", "obs",
];
// The list changes as Discord adds games; a day old is fresh enough.
const LIST_FRESH_FOR: Duration = Duration::from_secs(24 * 60 * 60);
// Offline with no cached list: try again this often.
const LIST_RETRY: Duration = Duration::from_secs(5 * 60);

/// The game running on this machine.
#[derive(Clone, Debug, PartialEq)]
pub struct Game {
    pub pid: u32,
    pub app_id: String,
    pub name: String,
    pub icon: Option<String>,
    // Unix milliseconds it was first seen running.
    pub started: i64,
}

impl Game {
    pub fn icon_url(&self) -> Option<String> {
        Some(crate::discord::app_icon_url(&self.app_id, self.icon.as_deref()?))
    }

    // An added program has no application: its path stands in for one.
    pub fn added(&self) -> bool {
        self.app_id.contains(['/', '\\'])
    }

    // Not verified: the activity's shape, taken from what arRPC sends Discord for a detected game.
    pub fn activity(&self) -> serde_json::Value {
        match self.added() {
            true => serde_json::json!({ "type": 0, "name": self.name, "timestamps": { "start": self.started } }),
            false => serde_json::json!({ "type": 0, "name": self.name, "application_id": self.app_id, "timestamps": { "start": self.started } }),
        }
    }
}

/// A game as a row shows it: its icon, and its name and running time on hover.
pub struct Playing {
    pub icon: String,
    pub name: String,
    // Unix milliseconds it started, when the presence says.
    pub started: Option<i64>,
}

impl Playing {
    // "Playing Hades II · 1:24:07", the time counted at the moment it's asked for.
    pub fn label(&self) -> String {
        let since = self.started.map(|start| crate::call::clock(((chrono::Utc::now().timestamp_millis() - start).max(0) / 1000) as u64));
        match since {
            Some(since) => format!("Playing {} · {since}", self.name),
            None => format!("Playing {}", self.name),
        }
    }
}

static RUNNING: Mutex<Option<Game>> = Mutex::new(None);
static RULES: Mutex<Option<Rules>> = Mutex::new(None);

/// What you set under Registered games, for the scan to follow.
#[derive(Clone, Debug, Default)]
pub struct Rules {
    // Application ids (or an added program's path) not to detect.
    pub off: HashSet<String>,
    pub added: Vec<Added>,
}

/// A program you added as a game ("Add it!"), by its executable's full path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Added {
    pub path: String,
    pub name: String,
}

/// A game seen running here, for Registered games' list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Seen {
    // The application id, or an added program's path.
    pub id: String,
    pub name: String,
    // Unix milliseconds.
    pub last: i64,
}

pub fn set_rules(rules: Rules) {
    if let Ok(mut held) = RULES.lock() {
        *held = Some(rules);
    }
}

/// Programs with a window you could add as a game: (path, name), by name.
pub fn addable() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = processes().into_iter().filter(|(_, path)| looks_like_an_app(path)).map(|(_, path)| (program_name(&path), path)).map(|(name, path)| (path, name)).collect();
    out.sort_by_key(|(_, name)| name.to_lowercase());
    out.dedup_by(|a, b| a.1 == b.1);
    out
}

// An app bundle's own binary, or a program outside the system folders.
fn looks_like_an_app(path: &str) -> bool {
    let lower = path.to_lowercase().replace('\\', "/");
    match std::env::consts::OS {
        "macos" => lower.contains(".app/contents/macos/") && !lower.starts_with("/system/") && !lower.contains("/library/") && !lower.contains("helper"),
        _ => lower.ends_with(".exe") && !lower.contains("/windows/"),
    }
}

// "Factorio" from ".../factorio.app/Contents/MacOS/factorio" or "C:/Games/Factorio.exe".
pub fn program_name(path: &str) -> String {
    let path = path.replace('\\', "/");
    let bundle = path.split('/').find(|p| p.to_lowercase().ends_with(".app")).map(|b| b[..b.len() - 4].to_owned());
    bundle.unwrap_or_else(|| {
        let file = path.rsplit('/').next().unwrap_or(&path);
        file.strip_suffix(".exe").or_else(|| file.strip_suffix(".EXE")).unwrap_or(file).to_owned()
    })
}

/// The detected game's process, for Go Live's share bind to prefer over the frontmost app.
pub fn running_pid() -> Option<u32> {
    RUNNING.lock().ok()?.as_ref().map(|g| g.pid)
}

/// What the app keeps: the game detected here, and other people's games' icons by application id.
pub struct Games {
    pub mine: Option<Game>,
    // An application's icon URL, or None when it has no icon.
    icons: Rc<RefCell<Query<Option<String>>>>,
    watching: bool,
}

impl Default for Games {
    fn default() -> Self {
        Self { mine: None, icons: Rc::new(RefCell::new(Query::new("app-icons", Duration::from_secs(24 * 60 * 60)))), watching: false }
    }
}

impl Games {
    // Demo fixtures: an application's icon without asking Discord.
    pub fn seed_icon(&self, app_id: &str, url: &str) {
        self.icons.borrow_mut().seed(app_id, Some(url.to_owned()));
    }
}

impl Waffle {
    // Starts the scan once per app; each change republishes your presence and redraws.
    pub(crate) fn watch_games(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.games.watching, true) {
            return;
        }
        let mut changes = watch();
        cx.spawn(async move |this, cx| {
            while let Some(game) = changes.next().await {
                let alive = this.update(cx, |this, cx| {
                    if let Some(game) = &game {
                        let seen = crate::games::Seen { id: game.app_id.clone(), name: game.name.clone(), last: chrono::Utc::now().timestamp_millis() };
                        this.update_prefs(cx, |p| match p.games_seen.iter_mut().find(|s| s.id == seen.id) {
                            Some(s) => *s = seen,
                            None => p.games_seen.push(seen),
                        });
                    }
                    this.games.mine = game;
                    this.publish_presence();
                    cx.notify(); // lint-allow(global-notify): your game shows in the user panel (nav), the member list and profile cards
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach(); // lint-allow(detached-task): ends when the window is gone, holding a WeakEntity
    }

    // Your game as others see it: only while Activity status sharing is on.
    pub(crate) fn shared_game(&self) -> Option<&Game> {
        self.games.mine.as_ref().filter(|_| self.account_settings.show_current_game)
    }

    // The game someone plays, with its icon: yours as detected here, anyone else's (or yours
    // elsewhere) by their presence's application.
    pub(crate) fn playing(&self, user_id: &str, cx: &mut Context<Self>) -> Option<Playing> {
        if self.me.as_ref().is_some_and(|me| me.id == user_id)
            && let Some(game) = self.shared_game()
        {
            return Some(Playing { icon: game.icon_url()?, name: game.name.clone(), started: Some(game.started) });
        }
        let rich = self.presences.get(user_id)?.rich.as_ref().filter(|r| matches!(r.kind, ActivityKind::Playing | ActivityKind::Competing))?;
        Some(Playing { icon: self.app_icon(rich.app_id.as_deref()?, cx)?, name: rich.name.clone(), started: rich.started })
    }

    // An application's icon: cached, else fetched; whatever showed it redraws when it lands.
    pub(crate) fn app_icon(&self, app_id: &str, cx: &mut Context<Self>) -> Option<String> {
        let cached = self.icons().borrow_mut().cached(app_id).flatten();
        let Some(client) = self.client.clone() else { return cached };
        if !self.icons().borrow_mut().should_fetch(app_id) {
            return cached;
        }
        let (icons, app_id) = (self.icons(), app_id.to_owned());
        cx.spawn(async move |this, cx| {
            let icon = client.app_icon(&app_id).await;
            // A failure (an unknown application included) counts as no icon until the day is up, so
            // rows redrawing don't ask again every frame.
            let url = icon.ok().flatten().map(|hash| crate::discord::app_icon_url(&app_id, &hash));
            icons.borrow_mut().store(&app_id, url);
            this.update(cx, |_, cx| cx.notify()).ok(); // lint-allow(global-notify): game icons show in the user panel (nav), the member list and profile cards
        })
        .detach(); // lint-allow(detached-task): a one-shot fetch that ends when the request returns, holding a WeakEntity
        cached
    }

    fn icons(&self) -> Rc<RefCell<Query<Option<String>>>> {
        self.games.icons.clone()
    }
}

// Scans on a thread of its own and sends the game whenever it changes (None: it closed).
fn watch() -> mpsc::UnboundedReceiver<Option<Game>> {
    let (send, receive) = mpsc::unbounded();
    std::thread::Builder::new()
        .name("games".into())
        .spawn(move || {
            let matcher = loop {
                if let Some(apps) = detectable() {
                    break Matcher::new(apps, os());
                }
                std::thread::sleep(LIST_RETRY);
            };
            let mut current: Option<Game> = None;
            let mut known: HashMap<u32, (String, Option<usize>)> = HashMap::new();
            loop {
                let rules = RULES.lock().ok().and_then(|r| r.clone()).unwrap_or_default();
                let found = scan(&matcher, &rules, &mut known, current.as_ref(), processes());
                if found.as_ref().map(|g| (&g.app_id, g.pid)) != current.as_ref().map(|g| (&g.app_id, g.pid)) {
                    current = found;
                    if let Ok(mut running) = RUNNING.lock() {
                        running.clone_from(&current);
                    }
                    if send.unbounded_send(current.clone()).is_err() {
                        return;
                    }
                }
                std::thread::sleep(SCAN_EVERY);
            }
        })
        .ok();
    receive
}

// The game still running keeps its place; otherwise the first match. Its start time carries over.
// `known` remembers each process's match, so only new processes are matched.
fn scan(matcher: &Matcher, rules: &Rules, known: &mut HashMap<u32, (String, Option<usize>)>, current: Option<&Game>, running: Vec<(u32, String)>) -> Option<Game> {
    let alive: HashSet<u32> = running.iter().map(|(pid, _)| *pid).collect();
    known.retain(|pid, _| alive.contains(pid));
    let mut found: Vec<Game> = Vec::new();
    for (pid, path) in running {
        let entry = known.entry(pid).or_insert_with(|| (path.clone(), None));
        if entry.0 != path {
            *entry = (path.clone(), None);
        }
        if entry.1.is_none() {
            entry.1 = Some(matcher.find(&path, || command_line(pid)).unwrap_or(usize::MAX));
        }
        let game = match entry.1 {
            Some(i) if i != usize::MAX => {
                let app = &matcher.apps[i];
                Game { pid, app_id: app.id.clone(), name: app.name.clone(), icon: app.icon_hash.clone(), started: 0 }
            }
            _ => match rules.added.iter().find(|a| a.path == path) {
                Some(added) => Game { pid, app_id: added.path.clone(), name: added.name.clone(), icon: None, started: 0 },
                None => continue,
            },
        };
        if !rules.off.contains(&game.app_id) {
            found.push(game);
        }
    }
    let pick = found.iter().position(|g| current.is_some_and(|c| c.app_id == g.app_id)).unwrap_or(0);
    let mut game = (pick < found.len()).then(|| found.swap_remove(pick))?;
    game.started = current.filter(|c| c.app_id == game.app_id).map_or_else(|| chrono::Utc::now().timestamp_millis(), |c| c.started);
    Some(game)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Detectable {
    id: String,
    name: String,
    icon_hash: Option<String>,
    executables: Vec<Executable>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Executable {
    os: String,
    name: String,
    is_launcher: bool,
    #[serde(default)]
    arguments: Option<String>,
}

// Discord's name for this OS in `executables`.
fn os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

// This OS's games from the cached copy while it's fresh, else from Discord (falling back to the stale
// copy offline). Only this OS's entries are kept, on disk and in memory. Games sharing an executable
// with a non-game or an observed app lose that executable, as Discord excludes them.
fn detectable() -> Option<Vec<Detectable>> {
    let games = detectable_games()?;
    let mut not_games: HashSet<String> = non_games().into_iter().collect();
    not_games.extend(OBSERVED.iter().map(|s| (*s).to_owned()));
    Some(without(games, &not_games))
}

fn without(games: Vec<Detectable>, not_games: &HashSet<String>) -> Vec<Detectable> {
    games
        .into_iter()
        .filter_map(|mut app| {
            app.executables.retain(|e| !not_games.contains(&e.name.trim_start_matches('>').to_lowercase()));
            (!app.executables.is_empty()).then_some(app)
        })
        .collect()
}

// Executable names on Discord's non-games list (browsers and the like), for this OS.
fn non_games() -> Vec<String> {
    #[derive(Deserialize)]
    struct NonGame {
        executables: Vec<NonGameExe>,
    }
    #[derive(Deserialize)]
    struct NonGameExe {
        os: String,
        name: String,
    }
    let fetched = crate::discord::runtime().block_on(async { reqwest::get(format!("{CDN}/detectables/non-games-v1.json")).await?.json::<Vec<NonGame>>().await });
    fetched.into_iter().flatten().flat_map(|g| g.executables).filter(|e| e.os == os()).map(|e| e.name.to_lowercase()).collect()
}

fn detectable_games() -> Option<Vec<Detectable>> {
    let file = crate::data_dir()?.join("cache").join(format!("detectable-{}.json", os()));
    let age = std::fs::metadata(&file).and_then(|m| m.modified()).ok().and_then(|t| SystemTime::now().duration_since(t).ok());
    let cached = || serde_json::from_slice::<Vec<Detectable>>(&std::fs::read(&file).ok()?).ok();
    if age.is_some_and(|age| age < LIST_FRESH_FOR)
        && let Some(apps) = cached()
    {
        return Some(apps);
    }
    let fetched = crate::discord::runtime().block_on(async { reqwest::get(format!("{CDN}/detectables/games.json")).await?.json::<Vec<Detectable>>().await });
    let Ok(apps) = fetched else { return cached() };
    let apps: Vec<Detectable> = apps.into_iter().filter_map(|app| only_on(app, os())).collect();
    if let Ok(text) = serde_json::to_vec(&apps) {
        let _ = std::fs::create_dir_all(file.parent()?);
        let _ = std::fs::write(&file, text);
    }
    Some(apps)
}

fn only_on(mut app: Detectable, os: &str) -> Option<Detectable> {
    app.executables.retain(|e| e.os == os && !e.is_launcher);
    (!app.executables.is_empty()).then_some(app)
}

// Executable names indexed for lookup by path suffix.
struct Matcher {
    apps: Vec<Detectable>,
    rules: HashMap<String, Vec<Rule>>,
}

struct Rule {
    app: usize,
    // `>name`: the process's own file only, never a folder above it.
    own_file: bool,
    arguments: Option<String>,
}

impl Matcher {
    fn new(apps: Vec<Detectable>, os: &str) -> Self {
        let mut rules: HashMap<String, Vec<Rule>> = HashMap::new();
        for (i, app) in apps.iter().enumerate() {
            for exe in app.executables.iter().filter(|e| e.os == os && !e.is_launcher) {
                let (name, own_file) = exe.name.strip_prefix('>').map_or((exe.name.as_str(), false), |n| (n, true));
                rules.entry(name.to_owned()).or_default().push(Rule { app: i, own_file, arguments: exe.arguments.clone() });
            }
        }
        Self { apps, rules }
    }

    // The app `path` belongs to. `command_line` is read only when a rule needs arguments; unknown
    // arguments never match.
    fn find(&self, path: &str, command_line: impl Fn() -> Option<String>) -> Option<usize> {
        let line = OnceCell::new();
        let has = |needle: &str| line.get_or_init(&command_line).as_deref().is_some_and(|l| l.contains(needle));
        candidates(path).into_iter().find_map(|(candidate, own_file)| {
            let rule = self.rules.get(&candidate)?.iter().find(|r| (own_file || !r.own_file) && r.arguments.as_deref().is_none_or(has))?;
            Some(rule.app)
        })
    }
}

// Every path suffix, the file itself first ("cs2.exe", "win64/cs2.exe", …), and for a macOS bundle the
// suffixes ending at its `.app` ("factorio.app", "applications/factorio.app"). True marks the file's own name.
fn candidates(path: &str) -> Vec<(String, bool)> {
    let path = path.to_lowercase().replace('\\', "/");
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let parts = &parts;
    let suffixes = |end: usize| (0..end).rev().map(move |start| parts[start..end].join("/"));
    let mut out: Vec<(String, bool)> = suffixes(parts.len()).enumerate().map(|(i, s)| (s, i == 0)).collect();
    for end in (1..parts.len()).filter(|&i| parts[i - 1].ends_with(".app")) {
        out.extend(suffixes(end).map(|s| (s, false)));
    }
    out
}

// (pid, executable path) of every process we may look at.
#[cfg(target_os = "macos")]
fn processes() -> Vec<(u32, String)> {
    use std::ffi::c_void;
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    // Room for processes started between the two calls.
    let mut pids = vec![0i32; count.max(0) as usize + 64];
    let count = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast::<c_void>(), (pids.len() * size_of::<i32>()) as i32) };
    pids.truncate(count.max(0) as usize);
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    pids.into_iter()
        .filter(|&pid| pid > 0)
        .filter_map(|pid| {
            let len = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast::<c_void>(), buf.len() as u32) };
            (len > 0).then(|| (pid as u32, String::from_utf8_lossy(&buf[..len as usize]).into_owned()))
        })
        .collect()
}

// KERN_PROCARGS2: argc, the executable path, padding, then argv, each NUL-terminated.
#[cfg(target_os = "macos")]
fn command_line(pid: u32) -> Option<String> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as i32];
    let mut size = 0usize;
    unsafe { libc::sysctl(mib.as_mut_ptr(), 3, std::ptr::null_mut(), &mut size, std::ptr::null_mut(), 0) };
    let mut buf = vec![0u8; size];
    if size < 4 || unsafe { libc::sysctl(mib.as_mut_ptr(), 3, buf.as_mut_ptr().cast(), &mut size, std::ptr::null_mut(), 0) } != 0 {
        return None;
    }
    let argc = i32::from_ne_bytes(buf[..4].try_into().ok()?) as usize;
    let args: Vec<String> = buf[4..size].split(|&b| b == 0).filter(|s| !s.is_empty()).skip(1).take(argc).map(|s| String::from_utf8_lossy(s).into_owned()).collect();
    Some(args.join(" "))
}

#[cfg(target_os = "windows")]
fn processes() -> Vec<(u32, String)> {
    use windows::{
        Win32::{
            Foundation::CloseHandle,
            System::{
                ProcessStatus::EnumProcesses,
                Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW},
            },
        },
        core::PWSTR,
    };
    let mut pids = vec![0u32; 4096];
    let mut bytes = 0u32;
    if unsafe { EnumProcesses(pids.as_mut_ptr(), (pids.len() * size_of::<u32>()) as u32, &mut bytes) }.is_err() {
        return Vec::new();
    }
    pids.truncate(bytes as usize / size_of::<u32>());
    let mut buf = vec![0u16; 1024];
    pids.into_iter()
        .filter(|&pid| pid > 4)
        .filter_map(|pid| {
            let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
            let mut len = buf.len() as u32;
            let named = unsafe { QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) };
            let _ = unsafe { CloseHandle(process) };
            named.ok().map(|_| (pid, String::from_utf16_lossy(&buf[..len as usize])))
        })
        .collect()
}

// Another process's command line, for the `>` rules with arguments (Minecraft Java, Source mods), from
// NtQueryInformationProcess's ProcessCommandLineInformation (Windows 8.1 and later): a UNICODE_STRING
// whose buffer follows it in the block it fills.
#[cfg(target_os = "windows")]
fn command_line(pid: u32) -> Option<String> {
    use std::ffi::c_void;
    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtQueryInformationProcess(process: HANDLE, class: u32, info: *mut c_void, length: u32, returned: *mut u32) -> i32;
    }
    #[repr(C)]
    struct UnicodeString {
        length: u16,
        maximum_length: u16,
        buffer: *const u16,
    }
    const PROCESS_COMMAND_LINE_INFORMATION: u32 = 60;
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut needed = 0u32;
    // Asked with no room first, it answers how much it needs.
    unsafe { NtQueryInformationProcess(process, PROCESS_COMMAND_LINE_INFORMATION, std::ptr::null_mut(), 0, &mut needed) };
    let line = (needed as usize >= size_of::<UnicodeString>()).then(|| {
        // u64 words keep the block aligned for the pointer inside it.
        let mut block = vec![0u64; (needed as usize).div_ceil(8)];
        let status = unsafe { NtQueryInformationProcess(process, PROCESS_COMMAND_LINE_INFORMATION, block.as_mut_ptr().cast(), needed, &mut needed) };
        if status < 0 {
            return None;
        }
        let text = unsafe { &*block.as_ptr().cast::<UnicodeString>() };
        if text.buffer.is_null() {
            return None;
        }
        // The buffer points into `block`, which lives until this closure returns.
        let chars = unsafe { std::slice::from_raw_parts(text.buffer, usize::from(text.length) / 2) };
        Some(String::from_utf16_lossy(chars))
    });
    let _ = unsafe { CloseHandle(process) };
    line.flatten()
}

// GAP: no process scan on Linux yet.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn processes() -> Vec<(u32, String)> {
    Vec::new()
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn command_line(_pid: u32) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, exes: &[(&str, &str, bool, Option<&str>)]) -> Detectable {
        let executables = exes.iter().map(|&(os, name, is_launcher, arguments)| Executable { os: os.into(), name: name.into(), is_launcher, arguments: arguments.map(Into::into) }).collect();
        Detectable { id: id.into(), name: id.into(), icon_hash: None, executables }
    }

    fn matcher(os: &str) -> Matcher {
        let apps = vec![
            app("cs2", &[("win32", "win64/cs2.exe", false, None)]),
            app("factorio", &[("darwin", "factorio.app", false, None), ("win32", "factorio.exe", false, None)]),
            app("wow", &[("darwin", "_retail_/world of warcraft.app", false, None)]),
            app("minecraft", &[("darwin", ">java", false, Some("net.minecraft.client.main.Main")), ("win32", ">javaw.exe", false, Some("net.minecraft.client.main.Main"))]),
            app("league", &[("win32", "leagueclientux.exe", true, None), ("win32", "league of legends.exe", false, None)]),
        ];
        Matcher::new(apps.into_iter().filter_map(|a| only_on(a, os)).collect(), os)
    }

    fn found(m: &Matcher, path: &str, line: Option<&str>) -> Option<String> {
        m.find(path, || line.map(Into::into)).map(|i| m.apps[i].id.clone())
    }

    #[test]
    fn matches_a_path_suffix_on_windows() {
        let m = matcher("win32");
        assert_eq!(found(&m, r"C:\Steam\steamapps\common\Counter-Strike Global Offensive\game\bin\win64\cs2.exe", None).as_deref(), Some("cs2"));
        assert_eq!(found(&m, r"C:\Games\cs2.exe", None), None, "the folder is part of the name");
        assert_eq!(found(&m, r"D:\Factorio\bin\x64\Factorio.exe", None).as_deref(), Some("factorio"), "case doesn't matter");
        assert_eq!(found(&m, r"C:\Riot Games\League of Legends\Game\League of Legends.exe", None).as_deref(), Some("league"));
        assert_eq!(found(&m, r"C:\Riot Games\League of Legends\LeagueClientUx.exe", None), None, "launchers never count");
    }

    #[test]
    fn matches_a_macos_bundle() {
        let m = matcher("darwin");
        assert_eq!(found(&m, "/Applications/factorio.app/Contents/MacOS/factorio", None).as_deref(), Some("factorio"));
        assert_eq!(found(&m, "/Applications/World of Warcraft/_retail_/World of Warcraft.app/Contents/MacOS/World of Warcraft", None).as_deref(), Some("wow"));
        assert_eq!(found(&m, "/Applications/World of Warcraft.app/Contents/MacOS/World of Warcraft", None), None, "the folder is part of the name");
        assert_eq!(found(&m, "/usr/local/bin/factorio.exe", None), None, "another OS's executables don't count");
    }

    #[test]
    fn own_file_rules_need_their_arguments() {
        let m = matcher("darwin");
        let java = "/Library/Java/JavaVirtualMachines/jdk-21/Contents/Home/bin/java";
        assert_eq!(found(&m, java, Some("-Xmx2G net.minecraft.client.main.Main --version 1.21")).as_deref(), Some("minecraft"));
        assert_eq!(found(&m, java, Some("-jar language-server.jar")), None, "any other java program");
        assert_eq!(found(&m, java, None), None, "unreadable arguments never match");
        assert_eq!(found(&m, "/opt/java/bin/javafx", Some("net.minecraft.client.main.Main")), None);
        assert_eq!(found(&m, "/opt/java/bin/java/helper", Some("net.minecraft.client.main.Main")), None, "`>` names the file, never a folder");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn scans_this_process() {
        let (me, exe) = (std::process::id(), std::env::current_exe().expect("own path"));
        let name = exe.file_name().and_then(|n| n.to_str()).expect("file name");
        assert!(processes().iter().any(|(pid, path)| *pid == me && path.ends_with(name)), "the test binary sees itself");
        assert!(command_line(me).is_some_and(|line| line.contains(name)), "argv[0] leads the command line");
    }

    #[test]
    fn labels_the_game_with_its_running_time() {
        let game = |started| Playing { icon: String::new(), name: "Hades II".into(), started };
        assert_eq!(game(None).label(), "Playing Hades II");
        let label = game(Some(chrono::Utc::now().timestamp_millis() - 3_725_000)).label();
        assert!(label.starts_with("Playing Hades II · 1:02:0"), "{label}");
    }

    #[test]
    fn non_games_and_observed_apps_never_count() {
        let apps = vec![app("chrome-game", &[("win32", "chrome.exe", false, None)]), app("both", &[("win32", "obs64.exe", false, None), ("win32", "both.exe", false, None)])];
        let kept = without(apps, &["chrome.exe".to_owned(), "obs64.exe".to_owned()].into_iter().collect());
        assert_eq!(kept.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["both"]);
        assert_eq!(kept[0].executables.len(), 1);
    }

    #[test]
    fn rules_skip_games_turned_off_and_add_programs() {
        let m = matcher("win32");
        let path = r"D:\Factorio\bin\x64\Factorio.exe".to_owned();
        let running = || vec![(7, path.clone()), (8, r"C:\\Tools\\mygame.exe".to_owned())];
        let found = |rules: &Rules| scan(&m, rules, &mut HashMap::new(), None, running()).map(|g| (g.pid, g.app_id));
        let added = Added { path: r"C:\\Tools\\mygame.exe".into(), name: "mygame".into() };
        let rules = Rules { off: HashSet::from(["factorio".to_owned()]), added: vec![added.clone()] };
        assert_eq!(found(&rules), Some((8, added.path.clone())));
        assert_eq!(found(&Rules::default()), Some((7, "factorio".to_owned())));
        assert_eq!(found(&Rules { off: HashSet::from(["factorio".to_owned()]), added: Vec::new() }), None);
        assert_eq!(program_name("/Applications/Factorio.app/Contents/MacOS/factorio"), "Factorio");
        assert_eq!(program_name(r"C:\Games\Celeste.exe"), "Celeste");
    }

    #[test]
    fn reads_the_detectable_shape() {
        let body = r#"[{ "id": "356875221078245376", "name": "Overwatch", "icon_hash": "a60b", "aliases": [], "hook": true,
            "executables": [{ "is_launcher": false, "name": "overwatch.exe", "os": "win32" }] }]"#;
        let apps: Vec<Detectable> = serde_json::from_str(body).expect("parses");
        assert_eq!((apps[0].icon_hash.as_deref(), apps[0].executables[0].name.as_str()), (Some("a60b"), "overwatch.exe"));
    }
}
