// What a "+n" hides: hovering the stack lists everything in it, and a list longer than SHOWN gets a Show all
// that unfolds the rest in place, scrolling past MAX_HEIGHT.
use gpui_kit::{AnyView, App, AppContext, Context, ElementId, FontWeight, IntoElement, ParentElement, Render, SharedString, Stateful, StatefulInteractiveElement, Styled, Window, div, prelude::*, px};

use super::{RADIUS_XS, avatar, media, surface};
use crate::palette;

const SHOWN: usize = 8;
const MAX_HEIGHT: f32 = 320.;

// One thing in the list: a person (round) or an item (a badge, a server; square), with its name.
#[derive(Clone, Debug)]
pub struct Entry {
    pub icon: String,
    pub round: bool,
    pub name: SharedString,
}

impl Entry {
    pub fn person(avatar: impl Into<String>, name: impl Into<SharedString>) -> Self {
        Entry { icon: avatar.into(), round: true, name: name.into() }
    }

    pub fn item(icon: impl Into<String>, name: impl Into<SharedString>) -> Self {
        Entry { icon: icon.into(), round: false, name: name.into() }
    }
}

// Shows the full list when `el` is hovered; the card itself stays up while the pointer is on it.
pub fn overflow_list<E: StatefulInteractiveElement>(el: E, title: impl Into<SharedString>, entries: Vec<Entry>) -> E {
    let title = title.into();
    el.hoverable_tooltip(move |_, cx| list_view(title.clone(), entries.clone(), cx))
}

fn list_view(title: SharedString, entries: Vec<Entry>, cx: &mut App) -> AnyView {
    cx.new(|_| OverflowList { title, entries, expanded: false }).into()
}

struct OverflowList {
    title: SharedString,
    entries: Vec<Entry>,
    expanded: bool,
}

impl Render for OverflowList {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::perf::probe("OverflowList");
        let long = self.entries.len() > SHOWN;
        let shown = shown(self.entries.len(), self.expanded);
        let rows = self.entries.iter().take(shown).enumerate().map(|(i, e)| row(i, e, cx));
        let show_all = (long && !self.expanded).then(|| {
            div()
                .id("overflow-show-all")
                .h(px(28.))
                .px(px(6.))
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(px(14.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::info_ink(cx))
                .hover(|s| s.underline())
                .child(format!("Show all {}", self.entries.len()))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.expanded = true;
                    cx.notify();
                }))
        });
        let list = div().id("overflow-rows").flex().flex_col().gap(px(2.)).max_h(px(MAX_HEIGHT)).overflow_y_scroll().children(rows);
        // The gap to the trigger is the tooltip feel's, kept by the framework (the pointer may cross it).
        div().child(
            surface(cx)
                .id("overflow-list")
                .occlude()
                .w(px(240.))
                .p(px(8.))
                .gap(px(4.))
                .child(div().px(px(6.)).text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::muted(cx)).child(format!("{} · {}", self.title, self.entries.len())))
                .child(list)
                .children(show_all),
        )
    }
}

fn row(i: usize, entry: &Entry, cx: &App) -> Stateful<gpui_kit::Div> {
    let icon = match entry.round {
        true => avatar(entry.icon.clone(), 20.).into_any_element(),
        false => media(entry.icon.clone(), super::all(RADIUS_XS)).size(px(20.)).flex_none().into_any_element(),
    };
    div()
        .id(ElementId::NamedInteger("overflow-row".into(), i as u64))
        .h(px(28.))
        .px(px(6.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .text_size(px(14.))
        .text_color(palette::strong(cx))
        .child(icon)
        .child(div().min_w_0().truncate().child(entry.name.clone()))
}

// Rows drawn: all of a short list, SHOWN of a long one until Show all.
fn shown(len: usize, expanded: bool) -> usize {
    if expanded { len } else { len.min(SHOWN) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_long_list_waits_for_show_all() {
        assert_eq!(super::shown(5, false), 5);
        assert_eq!(super::shown(8, false), 8);
        assert_eq!(super::shown(30, false), 8);
        assert_eq!(super::shown(30, true), 30);
    }
}
