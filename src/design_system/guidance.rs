// Guidance: rendered do and don't pairs for the rules that are easiest to break, and Waffle's own recipes built
// only from tokens and components, ending in one realistic screen.
use super::*;

// A titled box holding one side of a pair; the Don't title takes the danger ink and nothing else changes.
fn example(title: &'static str, dont: bool, content: impl IntoElement, cx: &App) -> Div {
    let ink = if dont { palette::danger_ink(cx) } else { palette::positive_ink(cx) };
    ui::col().flex_1().min_w_0().gap_2().child(div().text_size(ui::TEXT_XS).font_weight(FontWeight::SEMIBOLD).text_color(ink).child(title)).child(board(cx).child(content))
}

fn pair(rule: &'static str, doing: impl IntoElement, dont: impl IntoElement, cx: &App) -> Frame {
    let body = ui::col().gap_3().child(row().items_start().gap_4().child(example("Do", false, doing, cx)).child(example("Don't", true, dont, cx))).child(note(rule, cx));
    (Kind::Variants, section("Do and Don't", "", cx).child(body).into_any_element())
}

pub(super) fn do_dont_frames(cx: &App) -> Vec<Frame> {
    let button = |id: &'static str, variant: ui::ButtonVariant, label: &'static str| ui::Button::new(id).variant(variant).child(label);
    let page = palette::surface(Level::Page, cx);
    vec![
        pair(
            "One primary action per group.",
            row().child(button("dd-do-1", ui::ButtonVariant::Default, "Join voice")).child(button("dd-do-2", ui::ButtonVariant::Secondary, "Invite friends")),
            row().child(button("dd-dont-1", ui::ButtonVariant::Default, "Join voice")).child(button("dd-dont-2", ui::ButtonVariant::Default, "Invite friends")),
            cx,
        ),
        pair(
            "An icon button always has a tooltip: hover the left one.",
            ui::Button::new("dd-do-icon").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).icon(Glyph::Pin).tooltip_text("Pin message"),
            ui::Button::new("dd-dont-icon").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).icon(Glyph::Pin),
            cx,
        ),
        pair(
            "Destructive only for the step that destroys.",
            row().child(button("dd-do-cancel", ui::ButtonVariant::Secondary, "Cancel")).child(button("dd-do-delete", ui::ButtonVariant::Destructive, "Delete server")),
            row().child(button("dd-dont-save", ui::ButtonVariant::Destructive, "Save changes")).child(button("dd-dont-cancel", ui::ButtonVariant::Secondary, "Cancel")),
            cx,
        ),
        pair(
            "Status is never colour alone: each state has its own shape.",
            row().gap_4().child(ui::status_mark(Status::Online, 14.)).child(ui::status_mark(Status::Idle, 14.)).child(ui::status_mark(Status::Dnd, 14.)).child(ui::status_mark(Status::Offline, 14.)),
            row().gap_4().children([palette::status(Status::Online), palette::status(Status::Idle), palette::status(Status::Dnd), palette::status(Status::Offline)].map(|c| div().size(px(14.)).rounded_full().bg(c))),
            cx,
        ),
        pair(
            "One accent per surface: the accent marks the one thing to do.",
            row().gap_3().child(ui::Badge::new("New").variant(ui::BadgeVariant::Accent)).child(ui::Badge::new("Beta").variant(ui::BadgeVariant::Secondary)),
            row().gap_3().child(ui::Badge::new("New")).child(ui::Badge::new("Beta")).child(ui::Badge::new("Live").variant(ui::BadgeVariant::Default)),
            cx,
        ),
        pair(
            "Corners come from the radius ladder, never a number of your own.",
            row().gap_3().child(div().size_12().bg(page).border_1().border_color(palette::line_strong(cx)).squircle(ui::RADIUS_MD)),
            row().gap_3().child(div().size_12().bg(page).border_1().border_color(palette::line_strong(cx)).rounded(px(6.))),
            cx,
        ),
    ]
}

impl DesignSystem {
    pub(super) fn recipe_frames(&mut self, cx: &mut Context<Self>) -> Vec<Frame> {
        let setting = |id: &'static str, title: &'static str, detail: &'static str, on: bool, line: bool, cx: &App| {
            ui::Item::setting(id, line, cx).child(ui::settings_text(title, Some(detail), cx)).child(ui::Switch::new(gpui_kit::SharedString::from(format!("{id}-switch"))).checked(on))
        };
        let rows = ui::Card::new(cx)
            .child(setting("rc-noise", "Noise suppression", "Quiet the room behind your voice.", true, false, cx))
            .child(setting("rc-echo", "Echo cancellation", "Stop your speakers feeding back.", true, true, cx))
            .child(setting("rc-gain", "Automatic gain", "Level your microphone for you.", false, true, cx));
        let recipe = |title: &'static str, lede_text: &'static str, content: Div| (Kind::Variants, section(title, lede_text, cx).child(board(cx).child(content)).into_any_element());
        let screen = ui::col()
            .w_full()
            .max_w(px(560.))
            .gap_4()
            .child(div().text_size(ui::TEXT_LG).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child("Voice and video"))
            .child(ui::Alert::new(ui::Notice::Info, Glyph::Info, "Your microphone is in use", "Changes apply the next time you join a call.", cx))
            .child(rows)
            .child(row().justify_end().gap_2().child(ui::Button::new("rc-reset").variant(ui::ButtonVariant::Secondary).child("Reset")).child(ui::Button::new("rc-save").child("Save changes")));
        vec![
            recipe("Recipes / Settings group", "A settings group is a Card of Items, each a title, a detail and one control at its end.", ui::col().w_full().max_w(px(560.)).child(ui::Card::new(cx).child(setting("rc-a", "Show on/off indicators", "A tick or a cross in the switch's knob.", false, false, cx)).child(setting("rc-b", "Reduce motion", "Springs become short fades.", true, true, cx)))),
            recipe("Recipes / Example screen", "One realistic screen, built only from tokens and components: an Alert, a Card of Items with Switches, and a primary action with its quiet partner.", screen),
        ]
    }
}
