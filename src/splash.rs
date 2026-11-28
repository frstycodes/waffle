// Waffle's launch window (design/names.html): a small Graphite card on the app's own surface with the waffle loader, the
// name, one line naming the real loading stage and a bar for how far along it is, shown while the hidden
// main window signs in and loads. `Waffle` advances it and closes it, then brings the main window forward.
use gpui_kit::{
    App, AppContext, Bounds, Context, FontWeight, IntoElement, ParentElement, Render, SharedString, Styled, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions,
    component::ActiveTheme, div, px, size,
};

use crate::{
    mascot,
    palette::{self, Level},
    ui::{self, Squircle, shadow},
};

// Signing in, loading your servers, opening Waffle.
pub const STAGES: usize = 3;

const MARK: f32 = 56.;
const TRACK: f32 = 120.;
// The splash window's size.
const WINDOW_WIDTH: f32 = 220.;
const WINDOW_HEIGHT: f32 = 248.;

pub struct Splash {
    stage: SharedString,
    progress: f32,
}

impl Splash {
    pub fn set(&mut self, step: usize, stage: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.stage = stage.into();
        self.progress = (step + 1) as f32 / STAGES as f32;
        cx.notify();
    }
}

pub fn open(stage: impl Into<SharedString>, cx: &mut App) -> Option<WindowHandle<Splash>> {
    let stage = stage.into();
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx))),
        titlebar: None,
        kind: WindowKind::PopUp,
        focus: true,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };
    cx.open_window(options, |window, cx| {
        palette::sync(window, cx);
        cx.new(|_| Splash { stage, progress: 1. / STAGES as f32 })
    })
    .ok()
}

impl Render for Splash {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::perf::probe("Splash");
        let bg = palette::surface(Level::Raised, cx);
        // Opened without gpui-kit's root view, so the app font is set here.
        // GAP: the design's 0 18 40 drop shadow falls outside this borderless window's bounds.
        div().size_full().font_family(cx.theme().font_family.clone()).child(
            // lint-allow(custom-surface): ui::PopoverContent::new is the overlay fill with a drop shadow; the splash is a raised fill with an inset hairline in a borderless window.
            ui::col()
                .size_full()
                .px_4()
                .squircle(ui::RADIUS_MD)
                .bg(bg)
                // One inset hairline: a border on a squircle in a transparent window doubled up at the corners.
                .shadow(vec![shadow(palette::line_strong(cx), 0., 0., 0., 1., true)])
                .items_center()
                .justify_center()
                .gap_2p5()
                .child(mascot::loader("splash-loader", MARK, palette::logo_ink(cx), cx))
                .child(div().pt_1().text_size(ui::TEXT_LG).line_height(ui::LINE_SM).font_weight(FontWeight::BOLD).text_color(palette::strong(cx)).child("Waffle"))
                .child(ui::line(self.stage.clone()).max_w_full().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(palette::muted(cx)))
                .child(div().w(px(TRACK)).child(ui::Progress::new(self.progress).track(palette::track(cx)))),
        )
    }
}

