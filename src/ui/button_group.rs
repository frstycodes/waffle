// shadcn/ui's ButtonGroup: related buttons joined into one control, a hairline seam between them. Build it from
// `.item(..)` so the seam's corners are the group's job, never each button's: `ButtonGroup::new().item(main).item(chevron)`.
use gpui_kit::{AnyElement, App, Div, IntoElement, ParentElement, RenderOnce, SharedString, InteractiveElement as _, StyleRefinement, Styled, Window, px};

use super::{HAIRLINE, RADIUS_MD, row};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    First,
    Middle,
    Last,
    Only,
}

type Rounded = Box<dyn FnOnce(Place, f32, f32) -> AnyElement>;

// A group of buttons as one pill: round on the outside (`.outer`, 14px), tight where they meet (`.seam`, 4px), 1px
// apart. `.group(name)` names it for `group_hover`, so pointing at either half can lift both, as Discord's mic and
// its chevron do. `.child(..)` adds a part that keeps its own corners (the stage's caret).
// Keywords: button group, split button, joined buttons, segmented actions, main action and options chevron, mic and caret, shadcn
#[derive(IntoElement)]
pub struct ButtonGroup {
    base: Div,
    group: Option<SharedString>,
    outer: f32,
    seam: Option<f32>,
    items: Vec<Rounded>,
}

impl ButtonGroup {
    pub fn new() -> Self {
        Self { base: row().items_stretch().gap(HAIRLINE), group: None, outer: RADIUS_MD, seam: Some(4.), items: Vec::new() }
    }

    pub fn group(mut self, name: impl Into<SharedString>) -> Self {
        self.group = Some(name.into());
        self
    }

    pub fn outer(mut self, radius: f32) -> Self {
        self.outer = radius;
        self
    }

    // The corner radius where two parts meet; `None` leaves every part's corners as they are.
    pub fn seam(mut self, radius: impl Into<Option<f32>>) -> Self {
        self.seam = radius.into();
        self
    }

    // A button of the group: its corners are set for its place.
    pub fn item<E: Styled + IntoElement + 'static>(mut self, el: E) -> Self {
        self.items.push(Box::new(move |place, outer, seam| {
            let (o, s) = (px(outer), px(seam));
            let el = match place {
                Place::Only => el.rounded(o),
                Place::First => el.rounded_l(o).rounded_r(s),
                Place::Last => el.rounded_r(o).rounded_l(s),
                Place::Middle => el.rounded(s),
            };
            el.into_any_element()
        }));
        self
    }
}

impl Default for ButtonGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderOnce for ButtonGroup {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let count = self.items.len();
        let seam = self.seam.unwrap_or(0.);
        let base = match self.group {
            Some(name) => self.base.group(name),
            None => self.base,
        };
        base.children(self.items.into_iter().enumerate().map(|(i, round)| {
            let place = match (i, count) {
                (_, 1) => Place::Only,
                (0, _) => Place::First,
                (i, n) if i + 1 == n => Place::Last,
                _ => Place::Middle,
            };
            round(place, self.outer, seam)
        }))
    }
}

impl Styled for ButtonGroup {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl ParentElement for ButtonGroup {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.base.extend(elements);
    }
}
