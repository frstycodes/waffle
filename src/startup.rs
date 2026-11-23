//! Opening the main window, and the timers that give memory back while the app is idle or in the background.

use crate::*;

// The main window's opening size and the smallest it can be resized to.
const WINDOW_WIDTH: f32 = 1280.;
const WINDOW_HEIGHT: f32 = 820.;
const MIN_WIDTH: f32 = 940.;
const MIN_HEIGHT: f32 = 520.;

// An update downloaded in the background last run goes in before anything signs in, then this run
// relaunches as the new build. A failed install opens Waffle as usual.
pub(crate) fn install_then_open(staged: updater::Staged, signed_in: bool, cx: &mut gpui_kit::App) {
    let splash = splash::open(format!("Installing {}…", staged.version), cx);
    cx.spawn(async move |cx| {
        match cx.background_spawn(async move { updater::install(&staged) }).await {
            // Opens the new build once this process is gone, so the two never overlap.
            Ok(()) => return cx.update(|cx| cx.restart()),
            Err(e) => eprintln!("[updater] install failed: {e}"),
        }
        cx.update(|cx| {
            if signed_in {
                return open_main(splash, cx);
            }
            // Signing in happens in the main window, so the splash steps aside, after the main
            // window exists: on Windows gpui quits when its last window closes, and closing the
            // splash first left none open for a moment, so signed-out starts quit at once.
            open_main(None, cx);
            if let Some(splash) = splash {
                splash.update(cx, |_, window, _| window.remove_window()).ok();
            }
        });
    })
    // lint-allow(detached-task): runs once to install the update, then the app restarts or opens the window
    .detach();
}

pub(crate) fn open_main(splash: Option<gpui_kit::WindowHandle<splash::Splash>>, cx: &mut gpui_kit::App) {
    let options = WindowOptions {
        show: splash.is_none(),
        focus: splash.is_none(),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx))),
        window_min_size: Some(size(px(MIN_WIDTH), px(MIN_HEIGHT))),
        titlebar: Some(TitlebarOptions { title: Some("Waffle".into()), appears_transparent: true, traffic_light_position: Some(layout::TRAFFIC_LIGHTS) }),
        window_background: material::window_background(),
        // Every drag area moves the window and zooms on double click itself; AppKit's own titlebar
        // handling covers only its 28pt strip and changed between macOS 26 and 27.
        app_owns_titlebar_drag: true,
        ..Default::default()
    };
    let handle = gpui_kit::open_window(options, cx, |window, cx| {
        palette::sync(window, cx);
        #[cfg(target_os = "windows")]
        tray::install(window, cx);
        #[cfg(target_os = "macos")]
        keep_running_in_dock(window, cx);
        cx.new(|cx| shell::Shell::new(window, splash, cx))
    })
    .expect("open window");
    if material::setting_name().is_some() {
        // gpui-component's root paints the theme background over the whole window, hiding the native
        // material; Root's own style is applied last, so it can clear that.
        handle
            .0
            .downcast::<gpui_kit::base::Root>()
            .expect("kit root")
            .update(cx, |root, _, _| {
                use gpui_kit::{Refineable as _, prelude::*};
                root.style().refine(&gpui_kit::StyleRefinement::default().bg(gpui_kit::transparent_black()));
            })
            .expect("clear root background");
    }
    cx.activate(true);
    background_watch(cx);
    idle_sweep(cx);
}

// In view, images are only swept when a column redraws, so what was browsed before the window went
// still would stay decoded. Sweeps on a timer and hands freed heap pages back when it let go of any.
const IDLE_SWEEP: Duration = Duration::from_secs(15);

fn idle_sweep(cx: &mut gpui_kit::App) {
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(IDLE_SWEEP).await;
            if !cx.update(images::sweep_all) {
                continue;
            }
            // After the deferred image drops have run and the GPU has let go of the textures.
            cx.background_executor().timer(Duration::from_millis(100)).await;
            cx.background_executor().spawn(async { memory::compact() }).await;
        }
    })
    // lint-allow(detached-task): an app-lifetime sweep owned by the process, not a view
    .detach();
}

// Background mode, unless a video is playing: minimized or closed to the tray, the app gives memory
// back at once (frees decoded images and their GPU textures, then pages everything out). Unfocused or
// fully covered, it waits `BACKGROUND_AFTER` first: fully covered it does the same, merely unfocused it
// only compacts its heaps, since paging out a window still in view would make the next click hitch.
const BACKGROUND_AFTER: Duration = Duration::from_secs(5);
// While a video plays in the background nothing announces it stopping, so check this often.
const VIDEO_CHECK: Duration = Duration::from_secs(5);

// Runs on presence changes rather than a timer (see presence::changes): redraws so GIFs stop or start
// (see gif.rs), and releases memory as above. While minimized or in the tray it only waits.
fn background_watch(cx: &mut gpui_kit::App) {
    let mut changes = presence::changes();
    cx.spawn(async move |cx| {
        // When the app went to the background, and the presence state memory was last released in:
        // each new out-of-sight state (covered, then minimized) releases again.
        let (mut since, mut released_in, mut compacted): (Option<Instant>, Option<u64>, bool) = (None, None, false);
        loop {
            let video = cx.update(|cx| video::is_playing(cx));
            let state = presence::generation();
            let mut wait = None;
            if !presence::background() {
                (since, released_in, compacted) = (None, None, false);
            } else if video {
                since = None;
                wait = Some(VIDEO_CHECK);
            } else {
                let at = *since.get_or_insert_with(Instant::now);
                let left = BACKGROUND_AFTER.saturating_sub(at.elapsed());
                if presence::out_of_sight() && (presence::hidden() || left.is_zero()) {
                    if released_in != Some(state) {
                        released_in = Some(state);
                        release_memory(cx).await;
                    }
                } else if !left.is_zero() {
                    wait = Some(left);
                } else if !std::mem::replace(&mut compacted, true) {
                    cx.background_executor().spawn(async { memory::compact() }).await;
                }
            }
            let changed = match wait {
                None => match changes.next().await {
                    Some(()) => true,
                    None => return,
                },
                Some(wait) => {
                    let timer = cx.background_executor().timer(wait);
                    futures::select_biased! {
                        change = changes.next() => match change {
                            Some(()) => true,
                            None => return,
                        },
                        _ = futures::FutureExt::fuse(timer) => false,
                    }
                }
            };
            // Redraw so GIFs stop or start, but not for going out of sight: that would reload what was just freed.
            if changed && !presence::out_of_sight() {
                cx.update(|cx| cx.refresh_windows());
            }
        }
    })
    // lint-allow(detached-task): an app-lifetime watcher owned by the process; it returns when the presence channel closes
    .detach();
}

// Frees decoded images (RAM and GPU), lets GIF players notice and drop their frames, then pages the
// rest out. Downloaded image files stay cached; paging takes them out of RAM too.
async fn release_memory(cx: &mut gpui_kit::AsyncApp) {
    cx.update(release_caches);
    cx.background_executor().timer(Duration::from_millis(100)).await;
    cx.background_executor()
        .spawn(async {
            let before = memory::process().map_or(0, |p| p.private_working_set);
            memory::trim();
            let after = memory::process().map_or(0, |p| p.private_working_set);
            if std::env::var_os("WAFFLE_MEMORY").is_some() {
                eprintln!("background: gave back memory, {} MB -> {} MB", before >> 20, after >> 20);
            }
        })
        .await;
}

// Decoded images and their GPU textures; GIF players see them go and drop their frames. Whatever is
// on screen decodes again from the download cache when next drawn.
fn release_caches(cx: &mut gpui_kit::App) {
    images::clear_all(cx);
}

// macOS: closing the window hides Waffle and keeps it signed in; the Dock icon or ⌘Tab brings it
// back and ⌘Q quits. Minimized, hidden or fully covered, it frees its caches and hands freed heap
// pages back (there is no working set to empty, the system compresses idle pages itself), unless a
// video is playing. Shown again, it redraws from scratch: the cached app view still points at the
// freed images' atlas tiles.
#[cfg(target_os = "macos")]
fn keep_running_in_dock(window: &mut gpui_kit::Window, cx: &mut gpui_kit::App) {
    window.on_window_should_close(cx, |_, cx| {
        cx.hide();
        false
    });
    let released = std::rc::Rc::new(std::cell::Cell::new(false));
    window
        .observe_window_visibility(move |visibility, window, cx| {
            if visibility.is_visible() {
                if released.replace(false) {
                    window.refresh();
                }
                return;
            }
            if video::is_playing(cx) || released.replace(true) {
                return;
            }
            release_caches(cx);
            cx.spawn(async move |cx| {
                // After the deferred image drops have run and the GPU has let go of the textures.
                cx.background_executor().timer(Duration::from_millis(100)).await;
                cx.background_executor().spawn(async { memory::compact() }).await;
            })
            // lint-allow(detached-task): one 100ms wait then a compaction, then the task ends
            .detach();
        })
        // lint-allow(detached-task): the Dock observer lives as long as the window and ends with it
        .detach();
}

