// The window's content: the app, drawn as a cached view, with the GIF layer above it (see gif.rs).
//
// gpui redraws a changed view together with every view above it. With the app cached here, a redraw
// caused by the GIF layer replays the app's last drawing instead of rebuilding it; the app renders
// again only when it changes itself.
use gpui_kit::{Context, Entity, Render, StyleRefinement, Window, div, prelude::*};

use crate::{Waffle, gif::GifLayer};

pub struct Shell {
    app: Entity<Waffle>,
    gifs: Entity<GifLayer>,
}

impl Shell {
    pub fn new(window: &mut Window, splash: Option<gpui_kit::WindowHandle<crate::splash::Splash>>, cx: &mut Context<Self>) -> Self {
        let app = cx.new(|cx| Waffle::new(window, splash, cx));
        let gifs = cx.new(GifLayer::new);
        Self { app, gifs }
    }
}

impl Render for Shell {
    // Out of sight (minimized, in the tray or fully covered) and no video playing, nothing is drawn at
    // all: incoming events keep updating the app's data, but nothing is laid out, decoded or touched
    // until the window can be seen again, so memory given back in the background stays given back.
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::perf::probe("Shell");
        if crate::presence::out_of_sight() && !crate::video::is_playing(cx) && !crate::gif::forced() {
            return div().into_any_element();
        }
        div()
            .relative()
            .size_full()
            .child(self.app.clone().cached(StyleRefinement::default().size_full()))
            .child(self.gifs.clone())
            .into_any_element()
    }
}
