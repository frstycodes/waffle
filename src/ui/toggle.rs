// shadcn/ui's Toggle: a two-state button, pressed taking the accent tint and a half-accent edge. Filters,
// options and any "on or off" pill; a chip that takes its label away is `.removable`.
use gpui_kit::{
    AnyElement, App, Div, ElementId, InteractiveElement, Interactivity, IntoElement, ParentElement, RenderOnce, Stateful, StatefulInteractiveElement, StyleRefinement, Styled, Window, div,
    component::Icon, prelude::FluentBuilder, px,
};

use super::control;
use crate::icons::Glyph;

type Remove = Box<dyn Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static>;

// A filter or option that is on or off; pressed takes the accent tint. Add the label with `.child`.
// Keywords: toggle, chip, pill, filter, token, pressed, on off, option, tag filter, shadcn
#[derive(IntoElement)]
pub struct Toggle {
    id: ElementId,
    base: Stateful<Div>,
    children: Vec<AnyElement>,
    pressed: bool,
    disabled: bool,
    glyph: Option<Glyph>,
    lead: Option<AnyElement>,
    remove: Option<Remove>,
}

impl Toggle {
    pub fn new(id: impl Into<ElementId>) -> Self {
        let id = id.into();
        Self { base: div().id(id.clone()), id, children: Vec::new(), pressed: false, disabled: false, glyph: None, lead: None, remove: None }
    }

    pub fn pressed(mut self, pressed: bool) -> Self {
        self.pressed = pressed;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    // Leads the label with a glyph.
    pub fn icon(mut self, glyph: Glyph) -> Self {
        self.glyph = Some(glyph);
        self
    }

    // Leads the label with any element (a forum tag's emoji).
    pub fn lead(mut self, lead: impl IntoElement) -> Self {
        self.lead = Some(lead.into_any_element());
        self
    }

    // A small × after the label calls `remove`, without the click reaching the toggle.
    pub fn removable(mut self, remove: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.remove = Some(Box::new(remove));
        self
    }

}

impl RenderOnce for Toggle {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let control = control::toggle_control(self.id, self.base, self.pressed, cx)
            .disabled(self.disabled)
            .when_some(self.glyph, |c, glyph| c.child(Icon::new(glyph).size(px(16.))))
            .children(self.lead)
            .children(self.children);
        match self.remove {
            Some(remove) => control.removable(remove, cx).render(window, cx).into_any_element(),
            None => control.render(window, cx).into_any_element(),
        }
    }
}

impl Styled for Toggle {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Toggle {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Toggle {}

impl ParentElement for Toggle {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}
