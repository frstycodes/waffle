// shadcn/ui's Progress: the done part, a 4px gap, then what's left; or a calm sweep for work that can't be counted.
use gpui_kit::{App, ElementId, Hsla, IntoElement, RenderOnce, Styled, Window, base::{Transition, transition}, px};

use super::{DUR_PANEL, feedback};
use crate::{motion, palette};

// Progress in the accent, 6px tall. `.fill` and `.track` take another scope's colours (an upload's positive fill,
// a poll's share, a splash's track), `.thin()` is 4px, `.animated(id)` grows the done part
// to each new value over the panel timing rather than jumping, and `Progress::indeterminate(id)` travels the track.
// Keywords: progress, progress bar, loading bar, upload, meter, share, percentage, track, indeterminate, shadcn
#[derive(IntoElement)]
pub struct Progress {
    value: f32,
    fill: Option<Hsla>,
    track: Option<Hsla>,
    thin: bool,
    animated: Option<ElementId>,
    indeterminate: Option<ElementId>,
}

impl Progress {
    // `value` is 0 to 1.
    pub fn new(value: f32) -> Self {
        Self { value, fill: None, track: None, thin: false, animated: None, indeterminate: None }
    }

    pub fn indeterminate(id: impl Into<ElementId>) -> Self {
        Self { indeterminate: Some(id.into()), ..Self::new(0.) }
    }

    pub fn fill(mut self, fill: Hsla) -> Self {
        self.fill = Some(fill);
        self
    }

    pub fn track(mut self, track: Hsla) -> Self {
        self.track = Some(track);
        self
    }

    pub fn thin(mut self) -> Self {
        self.thin = true;
        self
    }

    pub fn animated(mut self, id: impl Into<ElementId>) -> Self {
        self.animated = Some(id.into());
        self
    }
}

impl RenderOnce for Progress {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        if let Some(id) = self.indeterminate {
            return feedback::progress_indeterminate(id, cx).into_any_element();
        }
        let value = self.value.clamp(0., 1.);
        let value = match self.animated {
            Some(id) => transition(id, value, Transition::new(DUR_PANEL).easing(motion::ease_out()), window, cx),
            None => value,
        };
        let height = if self.thin { 4. } else { 6. };
        let fill = self.fill.unwrap_or_else(palette::accent);
        let track = self.track.unwrap_or_else(|| palette::line_strong(cx));
        feedback::track_in(value, height, height / 2., 2., fill, track).gap(px(4.)).into_any_element()
    }
}
