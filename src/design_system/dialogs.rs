// Dialogs page: the card, its header and footer. The flow that opens one is in flows.rs.
use super::*;

const WIDTHS: [f32; 3] = [360., 440., 560.];

impl DesignSystem {
    pub(super) fn dialogs(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let card = |i: usize, width: f32, window: &mut Window, cx: &mut Context<Self>| {
            let body = ui::DialogContent::new()
                .child(ui::dialog_header(("ds-dialog-head", i), "Invite friends to Night Shift").lead(ui::server_tile(None, "Night Shift", ui::AVATAR_LG, cx)).subtitle("# general").on_close(|_, _, _| {}))
                .child(div().text_color(palette::muted(cx)).child("Friends who accept will join with the Members role."))
                .child(row().justify_end().child(ui::Button::new(("ds-dialog-cancel", i)).variant(ui::ButtonVariant::Secondary).child("Cancel")).child(ui::Button::new(("ds-dialog-send", i)).child("Send invite")));
            ui::col().gap_1().child(div().text_size(ui::TEXT_XS).text_color(palette::faint(cx)).child(format!("{width}px"))).child(ui::Dialog::new(("ds-dialog", i), width, body, window, cx))
        };
        section("Dialog", "A card on the raised surface: a header with the thing's name and a close button, a body, a footer with one primary action at its end. Widths come in three sizes.", cx)
            .child(board(cx).child(row().gap_6().items_start().children(WIDTHS.into_iter().enumerate().map(|(i, w)| card(i, w, window, cx)).collect::<Vec<_>>())))
    }
}
