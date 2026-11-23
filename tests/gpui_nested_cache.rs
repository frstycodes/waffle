// Checks that a cached view replays when a cached view around it re-renders (part.rs relies on it: the
// app re-renders as the parent of whichever column changed, and the others must replay), and that a
// notified view re-renders alone, without the views around it (gpui-fast's retained views).
use gpui_kit::prelude::*;
use gpui_kit::{AnyView, Context, Entity, StyleRefinement, TestAppContext, Window, div};

struct Inner {
    renders: usize,
}

impl Render for Inner {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        div().size_full()
    }
}

struct Outer {
    renders: usize,
    inner: Entity<Inner>,
}

impl Render for Outer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        div().size_full().child(AnyView::from(self.inner.clone()).cached(StyleRefinement::default().size_full()))
    }
}

struct Root {
    outer: Entity<Outer>,
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(AnyView::from(self.outer.clone()).cached(StyleRefinement::default().size_full()))
    }
}

#[gpui_kit::test]
fn a_cached_view_replays_when_the_one_around_it_re_renders(cx: &mut TestAppContext) {
    let (root, cx) = cx.add_window_view(|_, cx| {
        let inner = cx.new(|_| Inner { renders: 0 });
        Root { outer: cx.new(|_| Outer { renders: 0, inner }) }
    });
    cx.run_until_parked();
    let (outer, inner) = root.read_with(cx, |root, cx| (root.outer.clone(), root.outer.read(cx).inner.clone()));
    let counts = |cx: &mut gpui_kit::VisualTestContext| (outer.read_with(cx, |o, _| o.renders), inner.read_with(cx, |i, _| i.renders));
    assert_eq!(counts(cx), (1, 1));

    outer.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert_eq!(counts(cx), (2, 1), "the outer view re-renders, the inner one replays");

    inner.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert_eq!(counts(cx), (2, 2), "a notified inner view re-renders alone");
}

// What the app's shell looks like when memory is released: something drawn before the app view (there,
// the GIF layer and overlays) changes how much it paints while the app view replays as a whole, and the
// app view re-renders afterwards around a column that replays. The column's recorded ranges were not
// updated by the whole-view replay, so replaying them indexed an older frame: wrong content, or
// "range end index out of range" in Scene::replay.
struct Painted {
    renders: usize,
}

impl Render for Painted {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        div().size_full().bg(gpui_kit::blue())
    }
}

struct PaintedOuter {
    renders: usize,
    inner: Entity<Painted>,
}

impl Render for PaintedOuter {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        div().size_full().bg(gpui_kit::green()).child(AnyView::from(self.inner.clone()).cached(StyleRefinement::default().size_full()))
    }
}

struct PaintedShell {
    before: usize,
    outer: Entity<PaintedOuter>,
}

impl Render for PaintedShell {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let before = (0..self.before).map(|_| div().size_2().bg(gpui_kit::red()));
        // Laid over the app view, as the GIF layer is, so the app view's bounds never change.
        div()
            .size_full()
            .child(div().absolute().size_full().children(before))
            .child(AnyView::from(self.outer.clone()).cached(StyleRefinement::default().size_full()))
    }
}

#[gpui_kit::test]
fn a_nested_view_re_renders_when_the_view_around_it_replayed_last_frame(cx: &mut TestAppContext) {
    let (shell, cx) = cx.add_window_view(|_, cx| {
        let inner = cx.new(|_| Painted { renders: 0 });
        PaintedShell { before: 64, outer: cx.new(|_| PaintedOuter { renders: 0, inner }) }
    });
    cx.run_until_parked();
    let (outer, inner) = shell.read_with(cx, |shell, cx| (shell.outer.clone(), shell.outer.read(cx).inner.clone()));
    let counts = |cx: &mut gpui_kit::VisualTestContext| (outer.read_with(cx, |o, _| o.renders), inner.read_with(cx, |i, _| i.renders));
    assert_eq!(counts(cx), (1, 1));

    // Less is painted before the app view, which replays as a whole: its column's ranges are now stale.
    shell.update(cx, |shell, cx| {
        shell.before = 0;
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(counts(cx), (1, 1), "the outer view replays whole");

    // The outer view re-renders; the whole-view replay shifted the column's ranges with it, so the
    // column replays them without indexing an older frame.
    outer.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert_eq!(counts(cx), (2, 1), "the column replays its shifted ranges");

    outer.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert_eq!(counts(cx), (3, 1), "and keeps replaying");
}

// A view nested in one that fades or scales (a popover animating in) inherits the opacity and transform
// its primitives are painted with. When the outer view re-renders with new values, the nested view
// must not replay what it painted under the old ones, or it stays at the animation's first frame.
struct Faded {
    opacity: f32,
    scale: f32,
    inner: Entity<Painted>,
}

impl Render for Faded {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().opacity(self.opacity).scale(self.scale).child(AnyView::from(self.inner.clone()).cached(StyleRefinement::default().size_full()))
    }
}

#[gpui_kit::test]
fn a_nested_view_re_renders_when_the_opacity_or_transform_around_it_changes(cx: &mut TestAppContext) {
    let (faded, cx) = cx.add_window_view(|_, cx| Faded { opacity: 0., scale: 0.9, inner: cx.new(|_| Painted { renders: 0 }) });
    cx.run_until_parked();
    let inner = faded.read_with(cx, |faded, _| faded.inner.clone());
    let renders = |cx: &mut gpui_kit::VisualTestContext| inner.read_with(cx, |i, _| i.renders);
    assert_eq!(renders(cx), 1);

    faded.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert_eq!(renders(cx), 1, "nothing it inherits changed: it replays");

    faded.update(cx, |faded, cx| {
        faded.opacity = 1.;
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(renders(cx), 2, "the opacity it inherits changed");

    faded.update(cx, |faded, cx| {
        faded.scale = 1.;
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(renders(cx), 3, "the transform it inherits changed");
}
