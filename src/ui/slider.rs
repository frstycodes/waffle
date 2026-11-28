// shadcn/ui's Slider: a 0 to 1 value, dragged or stepped by arrow keys.
use gpui_kit::{App, ElementId, IntoElement, RenderOnce, Window};

use super::feedback;

type OnValueChange = Box<dyn Fn(f32, &mut Window, &mut App) + 'static>;

// A slider whose thumb grows on hover and narrows while held; `.on_value_change` hears every move. `Slider::track(value)`
// is its resting look alone, for screens that run their own drag.
// Keywords: slider, range, volume, seek, scrub, drag value, shadcn
#[derive(IntoElement)]
pub struct Slider {
    id: Option<ElementId>,
    value: f32,
    on_value_change: Option<OnValueChange>,
}

impl Slider {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self { id: Some(id.into()), value: 0., on_value_change: None }
    }

    pub fn track(value: f32) -> Self {
        Self { id: None, value, on_value_change: None }
    }

    pub fn value(mut self, value: f32) -> Self {
        self.value = value;
        self
    }

    pub fn on_value_change(mut self, change: impl Fn(f32, &mut Window, &mut App) + 'static) -> Self {
        self.on_value_change = Some(Box::new(change));
        self
    }
}

impl RenderOnce for Slider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        match self.id {
            Some(id) => {
                let change = self.on_value_change.unwrap_or_else(|| Box::new(|_, _, _| {}));
                feedback::slider_live(id, self.value, change, window, cx).into_any_element()
            }
            None => feedback::slider_track(self.value, cx).into_any_element(),
        }
    }
}
