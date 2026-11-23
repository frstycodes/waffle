// The design-system page's labels and keys, and the variants that replaced hand-built copies across the app:
// dialog header, section labels, media labels, list rows, cards, the toolbar, removable chips, icon-button
// tones and sizes, progress colours.
use super::*;

pub(super) fn badges(cx: &App) -> impl IntoElement {
    section("Badges and keys", "A count asks for you, so it takes the accent. Tags label what something is; roles carry their colour as a dot, never as text colour.", cx).child(
        board(cx)
            .child(
                row()
                    .gap_4()
                    .child(ui::count_badge(3))
                    .child(ui::count_badge(42))
                    .child(ui::count_badge(120))
                    .child(ui::unread_dot())
                    .child(ui::tag("App", cx))
                    .child(ui::tag_with(Glyph::Crown, "Host", cx))
                    .child(ui::tag("New", cx))
                    .child(ui::live_badge()),
            )
            .child(
                row()
                    .child(ui::role_chip("Moderators", palette::oklch(0.65, 0.2, 5.), cx))
                    .child(ui::role_chip("Raid team", palette::oklch(0.7, 0.14, 155.), cx))
                    .child(ui::role_chip("Night owls", palette::oklch(0.75, 0.14, 80.), cx))
                    .child(ui::role_chip("Members", palette::status_in(Status::Offline, cx), cx)),
            )
            .child(
                row()
                    .gap_4()
                    .child(ui::key_hint(ui::keys(["⌘", "K"], cx), "Quick switcher", cx))
                    .child(ui::key_hint(ui::keys(["=", "Backspace"], cx), "Mute", cx))
                    .child(ui::key_hint(ui::keys(["Esc"], cx), "Close", cx)),
            ),
    )
}

pub(super) fn kit_variants(cx: &App) -> impl IntoElement {
    let page = palette::surface(Level::Page, cx);
    let face = |name: &'static str| ui::monogram(name, ui::AVATAR_XL);
    section("Kit variants", "One component with options instead of a copy per screen: search for them with tools/ui_find.py.", cx).child(
        board(cx)
            .gap_6()
            .child(ui::dialog_header("ds-dialog-head", "Invite friends to Night Shift").lead(ui::server_tile(None, "Night Shift", ui::AVATAR_LG, cx)).subtitle("# general").on_close(|_, _, _| {}))
            .child(
                row()
                    .gap_6()
                    .child(ui::section_label("Online — 4", ui::Heading::Muted, cx))
                    .child(ui::section_label("Recent", ui::Heading::Faint, cx))
                    .child(ui::section_label("Channel name", ui::Heading::Strong, cx))
                    .child(div().relative().size_16().bg(palette::stream_ground()).child(div().absolute().bottom_1().right_1().child(ui::media_label().child("0:42")))),
            )
            .child(
                ui::col()
                    .w_80()
                    .child(ui::list_row("ds-row-1", "Mira", cx).lead(ui::badged(face("Mira"), ui::AVATAR_XL, ui::Shape::Round, page).status(Some(Status::Online))).subtitle("Playing Waffle Quest").height(ui::RowHeight::Tall))
                    .child(ui::list_row("ds-row-2", "Theo", cx).lead(face("Theo")).subtitle("Idle").trailing(ui::icon_button("ds-row-msg", Glyph::ChatRound, "Message", cx).small()))
                    .child(ui::list_row("ds-row-3", "Picked", cx).height(ui::RowHeight::Dense).control(|c| c.toggled(true, cx))),
            )
            .child(
                row()
                    .gap_4()
                    .child(ui::card("ds-card", cx).w_48().child("Start from a template"))
                    .child(ui::card("ds-card-on", cx).bordered(cx).toggled(true, cx).w_48().child("Chosen"))
                    .child(ui::chip("ds-chip-x", None, "Mira", false, cx).removable(|_, _, _| {}, cx)),
            )
            .child(
                row()
                    .gap_3()
                    .child(ui::toolbar("ds-toolbar", cx).child(ui::icon_button("ds-tb-1", Glyph::EmojiAdd, "React", cx).small()).child(ui::icon_button("ds-tb-2", Glyph::Reply, "Reply", cx).small()))
                    .child(div().p_2().bg(palette::stage::bg()).child(row().gap_2().child(ui::icon_button("ds-stage", Glyph::Settings, "Settings", cx).tone(Tone::Stage, cx).round()).child(ui::button("ds-stage-b", Tone::Stage, cx).child("Watch"))))
                    .child(ui::icon_button("ds-rail", Glyph::Plus, "Add a server", cx).tone(Tone::Secondary, cx).sized(44.))
                    .child(ui::icon_button("ds-copied", Glyph::Checkmark, "Copied", cx).succeeded(true, cx))
                    .child(div().w_40().child(ui::progress_bar(0.62, cx).fill(palette::positive_ink(cx)).track(palette::line(cx)).thin().joined()))
                    .child(ui::badged(ui::icon_button("ds-upload", Glyph::Plus, "Upload", cx).tone(Tone::Secondary, cx).large(), 40., ui::Shape::Control, page).mark(ui::Spot::TopRight, ui::badge_of(ui::unread_dot()).bare())),
            ),
    )
}
