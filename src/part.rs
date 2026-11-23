// The window's columns (the sidebar, the main column, the side panel), each a cached view of its own, so a
// list scrolling or an animation running in one column rebuilds and lays out that column alone: gpui
// notifies the view being drawn, and replays the cached views beside it (see the patch in gpui's view.rs).
//
// A part re-renders whenever the app notifies, so it never shows older state than the rest of the window.
// Each loads its images through a cache stamped by its own renders: a shared cache stamped by the app's
// would evict a replaying column's images while they are still on screen.
use gpui_kit::{AnyElement, AnyView, Context, Entity, IntoElement, Render, StyleRefinement, Subscription, WeakEntity, Window, div, image_cache, prelude::*};

use crate::{Waffle, images::Images};

type RenderFn = fn(&mut Waffle, &mut Window, &mut Context<Waffle>) -> AnyElement;

pub struct Part {
    waffle: WeakEntity<Waffle>,
    render: RenderFn,
    // Which column, for the render counters (perf.rs).
    name: &'static str,
    images: Entity<Images>,
    _observe: Subscription,
}

impl Render for Part {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::perf::probe(self.name);
        self.images.update(cx, |images, cx| images.next_frame(cx));
        let render = self.render;
        let content = self.waffle.update(cx, |waffle, cx| render(waffle, window, cx)).unwrap_or_else(|_| div().into_any_element());
        image_cache(self.images.clone()).size_full().child(content)
    }
}

// The app's parts, made on first draw.
#[derive(Default)]
pub struct Parts {
    nav: Option<Entity<Part>>,
    main: Option<Entity<Part>>,
    panel: Option<Entity<Part>>,
    overlays: Option<Entity<Overlays>>,
}

#[derive(Clone, Copy)]
pub enum Column {
    Nav,
    Main,
    Panel,
}

impl Waffle {
    // `column` as a cached view laid out by `style`; its own content can't size it.
    pub fn part(&mut self, column: Column, style: StyleRefinement, cx: &mut Context<Self>) -> AnyElement {
        let (slot, render, name): (&mut Option<Entity<Part>>, RenderFn, &'static str) = match column {
            Column::Nav => (&mut self.parts.nav, |w, window, cx| w.render_nav(window, cx).into_any_element(), "Nav column"),
            Column::Main => (&mut self.parts.main, Waffle::render_main, "Main column"),
            Column::Panel => (&mut self.parts.panel, |w, window, cx| w.render_panel(window, cx).unwrap_or_else(|| div().into_any_element()), "Panel column"),
        };
        let waffle = cx.entity();
        let part = slot.get_or_insert_with(|| {
            // lint-allow(render-work): made once, on the column's first draw (get_or_insert_with), then kept.
            cx.new(|cx: &mut Context<Part>| Part {
                waffle: waffle.downgrade(),
                render,
                name,
                images: crate::images::new_cache(cx),
                // lint-allow(render-work, global-notify): subscribed once with the part, which notifies itself, following the app.
                _observe: cx.observe(&waffle, |_, _, cx| cx.notify()),
            })
        });
        AnyView::from(part.clone()).cached(style).into_any_element()
    }

    // Re-renders `column` alone, for state only it draws (a hover): notifying the app re-renders every
    // column, since each part observes it.
    pub fn redraw(&mut self, column: Column, cx: &mut Context<Self>) {
        let part = match column {
            Column::Nav => &self.parts.nav,
            Column::Main => &self.parts.main,
            Column::Panel => &self.parts.panel,
        };
        match part.clone() {
            Some(part) => part.update(cx, |_, cx| cx.notify()), // lint-allow(global-notify): notifies the part alone
            // Not drawn yet: nothing to keep, so the app's redraw draws it.
            None => cx.notify(), // lint-allow(global-notify): the part doesn't exist yet
        }
    }

    // Re-renders the popovers and pickers (`Overlays`) alone, for state only they draw. The menu and the
    // dialogs are built by the root, so they still take the app's redraw.
    pub fn redraw_overlays(&mut self, cx: &mut Context<Self>) {
        match self.parts.overlays.clone() {
            Some(overlays) => overlays.update(cx, |_, cx| cx.notify()), // lint-allow(global-notify): notifies the overlay layer alone
            // Not drawn yet: nothing to keep, so the app's redraw draws it.
            None => cx.notify(), // lint-allow(global-notify): the overlay layer doesn't exist yet
        }
    }

    // The popovers (overlays.rs) as a view of their own, so a list scrolling or a hover style in one
    // notifies this view rather than the app.
    pub fn overlays(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let waffle = cx.entity().downgrade();
        // lint-allow(render-work): made once, on the first draw (get_or_insert_with), then kept.
        self.parts.overlays.get_or_insert_with(|| cx.new(|_| Overlays { waffle })).clone().into_any_element()
    }
}

// Not cached: it renders whenever the app does, so it's never staler than the app, and on its own when
// something inside it notifies.
pub struct Overlays {
    waffle: WeakEntity<Waffle>,
}

impl Render for Overlays {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::perf::probe("Overlays");
        let overlays = self.waffle.update(cx, |waffle, cx| waffle.render_overlays(window, cx)).unwrap_or_default();
        // After the app's render set the rest of this layout's covering.
        if !overlays.is_empty() {
            crate::gif::set_covered(true);
        }
        div().absolute().inset_0().children(overlays)
    }
}
