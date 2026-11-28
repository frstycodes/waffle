//! Waffle installs itself on Windows (design/installers.html). The release's setup .exe is Waffle.exe under
//! another name: run as that, it shows this window, copies itself into place and opens the installed copy.
//! `--uninstall`, which Windows' Apps list runs, removes it again. `WAFFLE_DEMO=setup` shows the window
//! anywhere, with a pretend install.
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use gpui_kit::{App, Bounds, Context, FontWeight, SharedString, Task, Window, WindowBounds, WindowControlArea, WindowOptions, component::ActiveTheme, div, img, prelude::*, px, size};

use crate::{illustration::Illustration, palette::Level, ui::prelude::*};

#[cfg(target_os = "windows")]
mod windows;

// The installer window's size.
const WINDOW_WIDTH: f32 = 380.;
const WINDOW_HEIGHT: f32 = 440.;
const ART: f32 = 200.;
const TRACK: f32 = 220.;
// How often the window reads the copy's progress.
const POLL: Duration = Duration::from_millis(50);
// How long "Opening Waffle" stays up once the installed Waffle is starting.
const LINGER: Duration = Duration::from_millis(900);

// Runs the installer or the uninstaller when this launch is one; false for an ordinary launch.
pub fn run_instead() -> bool {
    #[cfg(target_os = "windows")]
    if std::env::args().any(|arg| arg == "--uninstall") {
        windows::uninstall();
        return true;
    }
    let demo = cfg!(debug_assertions) && std::env::var("WAFFLE_DEMO").as_deref() == Ok("setup");
    if !demo && !is_setup_exe() {
        return false;
    }
    gpui_kit::application().with_assets(crate::icons::Assets).run(|cx| {
        gpui_kit::init(cx);
        cx.text_system().add_fonts(crate::fonts::bundled()).expect("bundled fonts");
        cx.text_system().set_font_smoothing(false);
        open(cx);
    });
    true
}

// The release names the installer `waffle-windows-x86_64-setup.exe`; the installed copy is `Waffle.exe`.
fn is_setup_exe() -> bool {
    cfg!(target_os = "windows") && std::env::current_exe().ok().and_then(|exe| Some(exe.file_stem()?.to_string_lossy().to_lowercase().contains("setup"))).unwrap_or(false)
}

fn open(cx: &mut App) {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx))),
        titlebar: None,
        focus: true,
        is_resizable: false,
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        palette::sync(window, cx);
        window.set_window_title("Waffle Setup");
        cx.new(|cx| {
            let mut setup = Setup { phase: Phase::Installing, progress: Arc::default(), shown: (0, 0), work: None, ticker: None };
            setup.start(cx);
            setup
        })
    });
    if opened.is_err() {
        return cx.quit();
    }
    // Launched from a download or a browser, the installer comes to the front rather than opening behind it.
    cx.activate(true);
}

enum Phase {
    Installing,
    Opening,
    Failed(SharedString),
}

#[derive(Default)]
pub struct Progress {
    done: AtomicU64,
    total: AtomicU64,
}

pub struct Setup {
    phase: Phase,
    progress: Arc<Progress>,
    // The bytes last drawn, so the poll redraws only when the copy moved.
    shown: (u64, u64),
    work: Option<Task<()>>,
    ticker: Option<Task<()>>,
}

impl Setup {
    fn start(&mut self, cx: &mut Context<Self>) {
        self.phase = Phase::Installing;
        self.progress = Arc::default();
        self.shown = (0, 0);
        let progress = self.progress.clone();
        let work = cx.background_spawn(async move { install(&progress) });
        self.work = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            this.update(cx, |this, cx| this.finish(result, cx)).ok();
        }));
        self.ticker = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let Ok(true) = this.update(cx, |this, cx| this.tick(cx)) else { break };
            }
        }));
        cx.notify();
    }

    // Redraws when the copy moved; false once it's no longer installing.
    fn tick(&mut self, cx: &mut Context<Self>) -> bool {
        let now = (self.progress.done.load(Ordering::Relaxed), self.progress.total.load(Ordering::Relaxed));
        if now != self.shown {
            self.shown = now;
            cx.notify();
        }
        matches!(self.phase, Phase::Installing)
    }

    fn finish(&mut self, result: Result<Option<PathBuf>, String>, cx: &mut Context<Self>) {
        let installed = match result {
            Ok(installed) => installed,
            Err(reason) => {
                self.phase = Phase::Failed(reason.into());
                return cx.notify();
            }
        };
        self.phase = Phase::Opening;
        cx.notify();
        #[cfg(target_os = "windows")]
        if let Some(exe) = &installed {
            windows::launch(exe);
        }
        #[cfg(not(target_os = "windows"))]
        let _ = installed;
        self.work = Some(cx.spawn(async move |_, cx| {
            cx.background_executor().timer(LINGER).await;
            cx.update(|cx| cx.quit());
        }));
    }

    fn fraction(&self) -> f32 {
        match self.phase {
            Phase::Installing if self.shown.1 > 0 => self.shown.0 as f32 / self.shown.1 as f32,
            Phase::Installing | Phase::Failed(_) => 0.,
            Phase::Opening => 1.,
        }
    }
}

impl Render for Setup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::perf::probe("Setup");
        let failed = matches!(self.phase, Phase::Failed(_));
        let (title, detail): (SharedString, SharedString) = match &self.phase {
            Phase::Installing => ("Installing Waffle".into(), megabytes(self.shown).into()),
            Phase::Opening => ("Opening Waffle".into(), "Pour yourself something warm.".into()),
            Phase::Failed(reason) => ("Couldn't install Waffle".into(), reason.clone()),
        };
        // Opened without gpui-kit's root view, so the app font is set here.
        div().size_full().font_family(cx.theme().font_family.clone()).child(
            ui::col()
                .size_full()
                .bg(palette::surface(Level::Page, cx))
                .items_center()
                // lint-allow(titlebar-clearance): the setup window opens with titlebar: None, so no caption buttons or traffic lights share its top; its own close button ends this row.
                .child(ui::row().id("setup-drag").w_full().justify_end().window_control_area(WindowControlArea::Drag).child(ui::Button::new("setup-close").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).icon(Glyph::X).tooltip_text("Close").on_click(|_, _, cx| cx.quit())))
                .child(img(Illustration::InstallerWaffle.path()).size(px(ART)).flex_none().opacity(if failed { 0.45 } else { 1. }))
                .child(
                    ui::col()
                        .w_full()
                        .px_8()
                        .pt_4()
                        .gap_1p5()
                        .items_center()
                        .child(ui::line(title).text_size(ui::TEXT_LG).line_height(ui::LINE_LG).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)))
                        .child(div().w_full().text_center().text_size(ui::TEXT_SM).line_height(ui::LINE_SM).text_color(palette::muted(cx)).child(detail))
                        .children((!failed).then(|| div().pt_3().w(px(TRACK)).child(ui::Progress::new(self.fraction()).track(palette::track(cx)))))
                        .children(failed.then(|| {
                            ui::row()
                                .pt_3()
                                .gap_2()
                                .child(ui::Button::new("setup-quit").variant(ui::ButtonVariant::Secondary).child("Close").on_click(|_, _, cx| cx.quit()))
                                .child(ui::Button::new("setup-retry").child("Try again").on_click(cx.listener(|this, _, _, cx| this.start(cx))))
                        })),
                ),
        )
    }
}

// "18.3 of 42.6 MB", or nothing before the size is known.
fn megabytes((done, total): (u64, u64)) -> String {
    if total == 0 {
        return String::new();
    }
    let mb = |bytes: u64| bytes as f64 / 1_000_000.;
    format!("{:.1} of {:.1} MB", mb(done), mb(total))
}

#[cfg(target_os = "windows")]
fn install(progress: &Progress) -> Result<Option<PathBuf>, String> {
    windows::install(progress).map(Some)
}

// WAFFLE_DEMO=setup: an eight-second pretend copy, so the window can be seen on any system.
#[cfg(not(target_os = "windows"))]
fn install(progress: &Progress) -> Result<Option<PathBuf>, String> {
    const TOTAL: u64 = 42_600_000;
    progress.total.store(TOTAL, Ordering::Relaxed);
    for step in 1..=40 {
        std::thread::sleep(Duration::from_millis(200));
        progress.done.store(TOTAL * step / 40, Ordering::Relaxed);
    }
    if std::env::var_os("WAFFLE_SETUP_FAIL").is_some() {
        return Err("There isn't enough space on this drive. Free up 43\u{a0}MB, then try again.".into());
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    #[test]
    fn megabytes_reads_like_the_design() {
        assert_eq!(super::megabytes((18_300_000, 42_600_000)), "18.3 of 42.6 MB");
        assert_eq!(super::megabytes((0, 0)), "");
    }
}
