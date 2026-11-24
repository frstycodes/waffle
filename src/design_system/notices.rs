// Inline notices and the scope variants: strips with an action at the row's end, a text-only toast, labelled and
// bare rules, a numbered mark, a thought bubble, a static panel, scoped controls (a themed card, the call stage), the stage's own
// surface, list rows with a stamp and a body, and the media track.
use super::*;

pub(super) fn notices(cx: &App) -> impl IntoElement {
    let stage = palette::stage::bg();
    section("Notices and scopes", "Compact strips, one-line toasts and rules from data, and the same controls in a scope's own colours.", cx).child(
        board(cx)
            .gap_6()
            .child(
                ui::col()
                    .gap_2()
                    .child(ui::strip(ui::StripFill::Raised, cx).child(Icon::new(Glyph::Megaphone).size(ui::ICON_XL).text_color(palette::muted(cx))).child(div().flex_1().child("Follow to get this channel's updates.")).child(ui::button("ds-strip-follow", Tone::Primary, cx).child("Follow")))
                    .child(ui::strip(ui::StripFill::Selected, cx).child(Icon::new(Glyph::Megaphone).size(ui::ICON_MD).text_color(palette::muted(cx))).child(div().flex_1().child("Share with your followers!")).child(ui::icon_button("ds-strip-x", Glyph::X, "Dismiss", cx).small()))
                    .child(ui::strip(ui::StripFill::Accent, cx).child(Icon::new(Glyph::CheckCircle).size(ui::ICON_LG).text_color(palette::accent_ink(cx))).child(div().flex_1().child("Next: Say hi")).child(ui::button("ds-strip-go", Tone::Secondary, cx).small().child("Go"))),
            )
            .child(
                row()
                    .gap_6()
                    .child(ui::toast_text("ds-toast-text", "Copied link", cx))
                    .child(ui::number_mark(1, cx))
                    .child(ui::number_mark(12, cx))
                    .child(ui::role_chip("Rustacean", palette::data_color(0xE67E22), cx).child(ui::chip_remove("ds-role-remove", palette::muted(cx), palette::strong(cx), ui::REMOVE_GLYPH_SM, |_, _, _| {})))
                    .child(div().pt_4().pl_3().w_48().child(ui::thought_bubble("ds-thought", "🍯 drizzling honey on everything", palette::surface(palette::Level::Raised, cx), palette::line(cx))))
                    .child(ui::panel(cx).w_64().p_3().child("A static panel, no pointer")),
            )
            .child(ui::divider_with("Monday, March 3, 2025", palette::faint(cx), palette::line(cx)))
            .child(ui::divider_with("NEW", palette::alert(), palette::alert()))
            .child(
                row()
                    .gap_4()
                    .child(ui::col().w_32().gap_2().child(ui::rule(palette::line_strong(cx))).child(ui::rule(palette::alert())))
                    .child(div().p_2().bg(stage).child(ui::surface_in(stage, palette::stage::wash(0.14), cx).w_48().child(ui::card("ds-stage-card", cx).flat().on_stage().child("On the stage"))))
                    .child(ui::button("ds-scoped", Tone::Ghost, cx).scoped(palette::accent_soft(cx), palette::accent_soft(cx), palette::accent_ink(cx), palette::strong(cx)).child("Scoped colours")),
            )
            .child(
                ui::col()
                    .w_96()
                    .child(ui::list_row("ds-row-stamp", "Mira", cx).height(ui::RowHeight::Fit).top().lead(ui::monogram("Mira", ui::AVATAR_SM)).stamp("12:04").body(div().text_color(palette::text(cx)).child("Two lines of the message sit under the name, as long as they need.")))
                    .child(ui::list_row("ds-row-ink", "Playing now", cx).height(ui::RowHeight::Dense).title_ink(palette::accent_ink(cx))),
            )
            .child(row().gap_8().child(div().w_64().child(ui::media_track(0.4, 12., false))).child(div().h_24().child(ui::media_track(0.65, 12., true)))),
    )
}
