// Toasts: one line rising over the bottom of the window, and the kind that carries one way back (Rejoin).
use std::{rc::Rc, time::Duration};

use gpui_kit::{AnyElement, Context, IntoElement, ParentElement, SharedString, Styled, div, prelude::*};

use crate::{
    Waffle, palette,
    ui,
};

impl Waffle {
    pub(crate) fn show_toast(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let text = text.into();
        self.toast = Some(text.clone());
        self.toast_action = None;
        // Settings › Accessibility's minimum lengthens a toast, never shortens it.
        let shown = std::time::Duration::from_secs(u64::from(self.prefs.min_toast_secs.max(4)));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(shown).await;
            this.update(cx, |this, cx| {
                if this.toast.as_ref() == Some(&text) {
                    this.toast = None;
                    cx.notify(); // lint-allow(global-notify): the toast is drawn by the root, over every column
                }
            })
            .ok();
        })
        // lint-allow(detached-task): ends after the one timer, and holds only a WeakEntity.
        .detach();
        cx.notify(); // lint-allow(global-notify): the toast is drawn by the root, over every column
    }

    // A toast with a way back, held longer than a plain one (6s) so there's time to take it.
    pub(crate) fn show_toast_action(&mut self, text: impl Into<SharedString>, action: ToastAction, cx: &mut Context<Self>) {
        let text = text.into();
        self.toast = Some(text.clone());
        self.toast_action = Some(action);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TOAST_ACTION_HOLD).await;
            this.update(cx, |this, cx| {
                if this.toast.as_ref() == Some(&text) {
                    this.toast = None;
                    this.toast_action = None;
                    cx.notify(); // lint-allow(global-notify): the toast is drawn by the root, over every column
                }
            })
            .ok();
        })
        // lint-allow(detached-task): ends after the one timer, and holds only a WeakEntity.
        .detach();
        cx.notify(); // lint-allow(global-notify): the toast is drawn by the root, over every column
    }

    pub(crate) fn render_toast(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let text = self.toast.clone()?;
        if let Some(action) = &self.toast_action {
            let run = action.run.clone();
            let button = ui::Button::new("toast-action").variant(ui::ButtonVariant::Secondary).size(ui::ButtonSize::Sm).child(action.label.clone()).on_click(cx.listener(move |this, _, _, cx| {
                this.toast = None;
                this.toast_action = None;
                run(this, cx);
                cx.notify(); // lint-allow(global-notify): the toast is drawn by the root, over every column
            }));
            let toast = ui::Sonner::toast_with(SharedString::from(format!("toast-{text}")), action.glyph, palette::muted(cx), text, action.detail.clone(), Some(button.into_any_element()), cx);
            return Some(ui::row().items_stretch().absolute().left_0().right_0().bottom_6().justify_center().child(div().occlude().child(toast)).into_any_element());
        }
        // GAP: the design leads a toast with its tone's icon; `show_toast` carries only text.
        let toast = ui::Sonner::text(SharedString::from(format!("toast-{text}")), text, cx);
        Some(ui::row().items_stretch().absolute().left_0().right_0().bottom_6().justify_center().child(toast).into_any_element())
    }
}

const TOAST_ACTION_HOLD: Duration = Duration::from_secs(6);

// The one action a toast can carry, with the glyph that leads it and a line of detail.
pub(crate) struct ToastAction {
    pub glyph: crate::icons::Glyph,
    pub detail: Option<SharedString>,
    pub label: SharedString,
    pub run: ToastRun,
}

pub(crate) type ToastRun = Rc<dyn Fn(&mut Waffle, &mut Context<Waffle>)>;
