// Pieces the app had rebuilt by hand in many places, now one component each: the dialog header, section
// labels, labels over media, list rows and the floating toolbar.
use gpui_kit::{
    AnyElement, App, ClickEvent, Div, ElementId, FontWeight, InteractiveElement, Interactivity, IntoElement, ParentElement, RenderOnce, SharedString, Stateful, StatefulInteractiveElement, StyleRefinement,
    Styled, Window, div, prelude::FluentBuilder as _, px,
};

use super::{Control, LINE_LG, RADIUS_SM, Squircle, TEXT_LG, TEXT_SM, TEXT_XS, badge, col, icon_button, line, menu_row, row, shadow_pop};
use crate::{icons::Glyph, palette};

type OnClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

// A dialog's header: its title, with an optional lead before it (an app's tile) and a muted line under it,
// and the close button at the end.
// Keywords: modal title, dialog title, close button, header, heading, x
#[derive(IntoElement)]
pub struct DialogHeader {
    id: ElementId,
    title: SharedString,
    subtitle: Option<AnyElement>,
    lead: Option<AnyElement>,
    on_close: Option<OnClick>,
}

// A dialog's title row with its close button; see `DialogHeader`.
// Keywords: modal title, dialog title, close button, header, heading, x
pub fn dialog_header(id: impl Into<ElementId>, title: impl Into<SharedString>) -> DialogHeader {
    DialogHeader { id: id.into(), title: title.into(), subtitle: None, lead: None, on_close: None }
}

impl DialogHeader {
    // The muted line under the title: a channel's name, what the dialog is for.
    pub fn subtitle(mut self, subtitle: impl IntoElement) -> Self {
        self.subtitle = Some(subtitle.into_any_element());
        self
    }

    // Before the title: an app's or server's tile.
    pub fn lead(mut self, lead: impl IntoElement) -> Self {
        self.lead = Some(lead.into_any_element());
        self
    }

    pub fn on_close(mut self, on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Box::new(on_close));
        self
    }
}

impl RenderOnce for DialogHeader {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let (strong, muted) = (palette::strong(cx), palette::muted(cx));
        let close = self.on_close.map(|on_close| icon_button(ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "close".into()), Glyph::X, "Close", cx).on_click(on_close));
        row()
            .items_start()
            .gap_3()
            .children(self.lead.map(|lead| div().flex_none().child(lead)))
            .child(
                col()
                    .flex_1()
                    .gap_0p5()
                    .child(div().text_size(TEXT_LG).line_height(LINE_LG).font_weight(FontWeight::SEMIBOLD).text_color(strong).child(self.title))
                    .children(self.subtitle.map(|s| div().text_size(TEXT_SM).text_color(muted).child(s))),
            )
            .children(close)
    }
}

// How much a section label stands out: the usual muted small caps-weight label, a fainter one in dense
// lists, or a strong one that heads a form group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heading {
    Muted,
    Faint,
    Strong,
}

// A small heading over a group of things (a list section, a form group, a picker's category); the caller
// can add a lead glyph or a trailing count as children.
// Keywords: section heading, group label, caption, subheading, eyebrow, category title, form label
pub fn section_label(text: impl Into<SharedString>, kind: Heading, cx: &App) -> Div {
    let label = row().gap_1p5().font_weight(FontWeight::SEMIBOLD);
    let label = match kind {
        Heading::Muted => label.text_size(TEXT_XS).text_color(palette::muted(cx)),
        Heading::Faint => label.text_size(TEXT_XS).text_color(palette::faint(cx)),
        Heading::Strong => label.text_size(TEXT_SM).text_color(palette::strong(cx)),
    };
    label.child(text.into())
}

// A label over an image or video (a duration, GIF, a muted mark): white on the media scrim, whatever the theme.
// Keywords: duration, gif label, video badge, overlay label, on media, scrim label
pub fn media_label() -> Div {
    badge(palette::media_scrim().opacity(0.72), gpui_kit::white()).gap_1()
}

// How tall a list row is: dense (36, a picker), default (48, a member) or tall (60, a friend with a status line).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RowHeight {
    Dense,
    #[default]
    Default,
    Tall,
}

// A row for a person or thing in a list: a lead (avatar, tile, icon), its name with a muted line under it,
// and trailing actions; it fills on hover. Keeps the builder chain (`.on_click`, `.toggled`, layout).
// Keywords: person row, member row, friend row, user row, list item, avatar name subtitle, result row
#[derive(IntoElement)]
pub struct ListRow {
    control: Control,
    title: SharedString,
    lead: Option<AnyElement>,
    subtitle: Option<AnyElement>,
    trailing: Vec<AnyElement>,
    height: RowHeight,
}

// A list row for a person or thing: lead, name, muted line, trailing actions; see `ListRow`.
// Keywords: person row, member row, friend row, user row, list item, avatar name subtitle, result row
pub fn list_row(id: impl Into<ElementId>, title: impl Into<SharedString>, cx: &App) -> ListRow {
    ListRow { control: menu_row(id, cx), title: title.into(), lead: None, subtitle: None, trailing: Vec::new(), height: RowHeight::Default }
}

impl ListRow {
    pub fn lead(mut self, lead: impl IntoElement) -> Self {
        self.lead = Some(lead.into_any_element());
        self
    }

    pub fn subtitle(mut self, subtitle: impl IntoElement) -> Self {
        self.subtitle = Some(subtitle.into_any_element());
        self
    }

    // After the text: actions, a count, a time.
    pub fn trailing(mut self, trailing: impl IntoElement) -> Self {
        self.trailing.push(trailing.into_any_element());
        self
    }

    pub fn height(mut self, height: RowHeight) -> Self {
        self.height = height;
        self
    }

    // The row's own control, for its options (`.toggled`, `.highlighted`, `.danger`).
    pub fn control(mut self, f: impl FnOnce(Control) -> Control) -> Self {
        self.control = f(self.control);
        self
    }
}

impl Styled for ListRow {
    fn style(&mut self) -> &mut StyleRefinement {
        self.control.style()
    }
}

impl InteractiveElement for ListRow {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.control.interactivity()
    }
}

impl StatefulInteractiveElement for ListRow {}

impl RenderOnce for ListRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let (strong, muted) = (palette::strong(cx), palette::muted(cx));
        let row_height = match self.height {
            RowHeight::Dense => 36.,
            RowHeight::Default => 48.,
            RowHeight::Tall => 60.,
        };
        self.control
            .h(px(row_height))
            .gap_3()
            .children(self.lead.map(|lead| div().flex_none().child(lead)))
            .child(
                col()
                    .flex_1()
                    .child(line(self.title).font_weight(FontWeight::MEDIUM).text_color(strong))
                    .children(self.subtitle.map(|s| div().min_w_0().truncate().text_size(TEXT_XS).text_color(muted).child(s))),
            )
            .when(!self.trailing.is_empty(), |r| r.child(row().flex_none().gap_1().children(self.trailing)))
    }
}

// A small floating bar of icon buttons (a message's hover actions, an image's tools): the overlay surface,
// tight around its buttons. It occludes, so what's under it never takes its hover.
// Keywords: hover actions, action bar, floating toolbar, icon bar, message actions
pub fn toolbar(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .occlude()
        .flex()
        .items_center()
        .gap_0p5()
        .p_0p5()
        .squircle(RADIUS_SM)
        .bg(palette::surface(palette::Level::Overlay, cx))
        .shadow(shadow_pop(cx))
}
