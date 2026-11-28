// shadcn/ui's Skeleton: a loading placeholder in the shape of what's coming, with a calm shimmer.
use gpui_kit::{App, Div, ElementId, IntoElement, RenderOnce, StyleRefinement, Styled, Window, div, px};

use super::{RADIUS_SM, feedback};

// A loading placeholder: the selected fill with a shimmer that sweeps across, then rests. Callers size it
// (`.h_3().w(px(96.))`); `.radius` is its squircle corner, `Skeleton::circle(id, size)` a round one for an avatar.
// Keywords: skeleton, loading placeholder, shimmer, loading, avatar placeholder, shadcn
#[derive(IntoElement)]
pub struct Skeleton {
    base: Div,
    id: ElementId,
    radius: f32,
    circle: Option<f32>,
}

impl Skeleton {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self { base: div(), id: id.into(), radius: RADIUS_SM, circle: None }
    }

    pub fn circle(id: impl Into<ElementId>, size: f32) -> Self {
        Self { circle: Some(size), ..Self::new(id) }
    }

    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }
}

impl RenderOnce for Skeleton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        match self.circle {
            Some(size) => feedback::sweeping(self.base, self.id, size / 2., true, cx).size(px(size)).flex_none(),
            None => feedback::sweeping(self.base, self.id, self.radius, false, cx),
        }
    }
}

impl Styled for Skeleton {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}
