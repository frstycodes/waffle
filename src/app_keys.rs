// App shortcuts: ⌘R / ⌘Q on macOS, Ctrl+R / Ctrl+Q on Windows and Linux; ⌘= / ⌘- / ⌘0 zoom like Discord; ⌘F / Ctrl+F in a channel or DM
// (the "Chat" key context, so Settings can bind its own); ⌘I toggles the Inbox and ⌘⇧E reads its top unread
// channel, as Discord's. Alt+F4 on Windows stays the window's close, which hides Waffle to the tray like
// Discord (tray.rs).
use gpui_kit::{App, KeyBinding, actions};

use crate::Waffle;

actions!(waffle, [Quit, Restart, FocusSearch, ZoomIn, ZoomOut, ResetZoom, ToggleInbox, MarkTopUnreadRead]);

// Discord's zoom levels.
const ZOOM_STEPS: [f32; 11] = [0.5, 0.67, 0.75, 0.8, 0.9, 1., 1.1, 1.25, 1.5, 1.75, 2.];

pub fn init(cx: &mut App) {
    // `secondary` is ⌘ on macOS and Ctrl elsewhere.
    cx.bind_keys([
        KeyBinding::new("secondary-q", Quit, None),
        KeyBinding::new("secondary-r", Restart, None),
        KeyBinding::new("secondary-f", FocusSearch, Some("Chat")),
        KeyBinding::new("secondary-=", ZoomIn, None),
        KeyBinding::new("secondary-+", ZoomIn, None),
        KeyBinding::new("secondary--", ZoomOut, None),
        KeyBinding::new("secondary-0", ResetZoom, None),
        KeyBinding::new("secondary-i", ToggleInbox, None),
        KeyBinding::new("secondary-shift-e", MarkTopUnreadRead, None),
    ]);
    cx.on_action(|_: &ToggleInbox, cx| update_main(cx, Waffle::toggle_inbox));
    cx.on_action(|_: &MarkTopUnreadRead, cx| update_main(cx, Waffle::mark_top_unread_read));
    cx.on_action(|_: &Quit, cx| cx.quit());
    // In a call it asks first, then leaves voice (restart.rs).
    cx.on_action(|_: &Restart, cx| crate::restart::restart_app(cx));
    cx.on_action(|_: &ZoomIn, cx| set_zoom(cx, |z| ZOOM_STEPS.into_iter().find(|s| *s > z + 0.001)));
    cx.on_action(|_: &ZoomOut, cx| set_zoom(cx, |z| ZOOM_STEPS.into_iter().rev().find(|s| *s < z - 0.001)));
    cx.on_action(|_: &ResetZoom, cx| set_zoom(cx, |_| Some(1.)));
}

fn update_main(cx: &mut App, act: fn(&mut Waffle, &mut gpui_kit::Context<Waffle>)) {
    let Some(main) = cx.try_global::<crate::restart::Main>().and_then(|m| m.0.upgrade()) else { return };
    main.update(cx, act);
}

// Every window follows the saved zoom; `next` None means already at the end of the range.
fn set_zoom(cx: &mut App, next: impl Fn(f32) -> Option<f32>) {
    let Some(main) = cx.try_global::<crate::restart::Main>().and_then(|m| m.0.upgrade()) else { return };
    let Some(zoom) = next(main.read(cx).prefs.zoom) else { return };
    main.update(cx, |this, cx| {
        this.prefs.zoom = zoom;
        this.prefs.save();
        this.show_toast(format!("Zoom {}%", (zoom * 100.).round()), cx);
    });
    // The shortcut's window is checked out of the app while its key event dispatches, so updating it
    // here fails; the zoom lands once dispatch returns.
    cx.defer(move |cx| {
        for window in cx.windows() {
            window.update(cx, |_, window, cx| crate::layout::apply_zoom(window, zoom, cx)).ok();
        }
    });
}
