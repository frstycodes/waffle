// Toasts: one line rising over the bottom of the window, and the kind that carries one way back (Rejoin).
use std::{rc::Rc, time::Duration};

use gpui_kit::{Animation, AnimationExt, AnyElement, Context, FontWeight, IntoElement, ParentElement, SharedString, Styled, div, prelude::*, px};

use crate::{
    Waffle, motion, palette,
    ui::{self, RADIUS_LG, Squircle},
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
            let button = ui::button("toast-action", ui::Tone::Secondary, cx).small().child(action.label.clone()).on_click(cx.listener(move |this, _, _, cx| {
                this.toast = None;
                this.toast_action = None;
                run(this, cx);
                cx.notify(); // lint-allow(global-notify): the toast is drawn by the root, over every column
            }));
            let toast = ui::toast_with(SharedString::from(format!("toast-{text}")), action.glyph, palette::muted(cx), text, action.detail.clone(), Some(button.into_any_element()), cx);
            return Some(ui::row().items_stretch().absolute().left_0().right_0().bottom_6().justify_center().child(div().occlude().child(toast)).into_any_element());
        }
        // GAP: the design leads a toast with its tone's icon and can carry one action; `show_toast`
        // carries only text, so neither shows yet.
        // lint-allow(custom-surface, custom-badge): ui::toast always leads with a glyph; a plain show_toast is one line of text.
        let toast = ui::row()
            .max_w(px(TOAST_MAX))
            .min_h_10()
            .py_2p5()
            .pl_3p5()
            .pr_2p5()
            .squircle(RADIUS_LG)
            .bg(palette::surface(palette::Level::Overlay, cx))
            .shadow(ui::shadow_pop(cx))
            .text_size(ui::TEXT_SM)
            .line_height(ui::LINE_SM)
            .font_weight(FontWeight::NORMAL)
            .text_color(palette::strong(cx))
            .child(text.clone());
        // Rises 8px into place from scale 0.97 with its fade (CSS `toast-in`, 240ms).
        // GAP: and from blur(4px); gpui has no filter blur on an element's own painting.
        let toast = toast.with_animation(SharedString::from(format!("toast-{text}")), Animation::new(Duration::from_millis(240)).with_easing(|t| motion::ease_out().sample(t)), |toast, t| {
            toast.translate_y(px(TOAST_RISE * (1. - t))).scale(0.97 + 0.03 * t).opacity(t)
        });
        Some(ui::row().items_stretch().absolute().left_0().right_0().bottom_6().justify_center().child(toast).into_any_element())
    }
}

// The toast's widest, and how far it rises as it comes in (CSS `toast-in`).
const TOAST_MAX: f32 = 380.;
const TOAST_RISE: f32 = 8.;
const TOAST_ACTION_HOLD: Duration = Duration::from_secs(6);

// The one action a toast can carry, with the glyph that leads it and a line of detail.
pub(crate) struct ToastAction {
    pub glyph: crate::icons::Glyph,
    pub detail: Option<SharedString>,
    pub label: SharedString,
    pub run: ToastRun,
}

pub(crate) type ToastRun = Rc<dyn Fn(&mut Waffle, &mut Context<Waffle>)>;
