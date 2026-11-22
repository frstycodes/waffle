// Minimize, maximize and close for Windows: 46 wide and the header line's full height, flush right, so
// their glyphs centre on the header and the window's top right corner is Close when maximized.
// The title bar is drawn by the app (`appears_transparent`), and unlike macOS, Windows then shows no
// caption buttons of its own. Each button is tagged with its WindowControlArea, so Windows does the
// hit-testing: clicks, Snap Layouts on maximize hover, and the close button all behave like native
// ones. Nothing is drawn on other platforms.
use gpui_kit::{AnyElement, App, Hsla, Window, WindowControlArea, component::Icon, div, prelude::*, px};

use crate::{icons::Glyph, layout::HEADER_HEIGHT, palette, sidebar::Nav, ui};

const BUTTON_WIDTH: f32 = 46.;
/// Room the controls take at the right of the header line.
pub const WIDTH: f32 = width(cfg!(target_os = "windows"));

const fn width(windows: bool) -> f32 {
    if windows { BUTTON_WIDTH * 3. } else { 0. }
}

pub fn render(window: &Window, cx: &App) -> Option<AnyElement> {
    if !cfg!(target_os = "windows") {
        return None;
    }
    let (muted, strong, hover) = (palette::muted(cx), palette::strong(cx), palette::hover(cx));
    let button = |id: &'static str, icon: Icon, area: WindowControlArea, hover_bg: Hsla, hover_fg: Hsla| {
        div()
            .id(id)
            .w(px(BUTTON_WIDTH))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .text_color(muted)
            .hover(move |s| s.bg(hover_bg).text_color(hover_fg))
            .active(move |s| s.bg(hover_bg.opacity(0.85)).text_color(hover_fg))
            // gpui answers Windows' hit test with the first registered control area under the
            // cursor, and the title bar's Drag area (the whole window on the login screen) is
            // registered first. Blocking the mouse hides those from the hit test under the button.
            .occlude()
            .window_control_area(area)
            .child(icon.size(ui::ICON_MD))
    };
    let maximize = if window.is_maximized() { Nav::Restore } else { Nav::Maximize };
    Some(
        div()
            .absolute()
            .top_0()
            .right_0()
            .h(px(HEADER_HEIGHT))
            .flex()
            .child(button("window-minimize", Nav::Minimize.icon(), WindowControlArea::Min, hover, strong))
            .child(button("window-maximize", maximize.icon(), WindowControlArea::Max, hover, strong))
            .child(button("window-close", Icon::new(Glyph::X), WindowControlArea::Close, palette::window_close_hover(), gpui_kit::white()))
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn caption_room_is_three_buttons_on_windows_only() {
        assert_eq!(super::width(true), 138.);
        assert_eq!(super::width(false), 0.);
    }
}
