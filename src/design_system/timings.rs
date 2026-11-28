// Motion page: the six timings, playable.
use super::*;

impl DesignSystem {
    pub(super) fn motion(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cards: [(&str, &str, u64, Easing); 6] = [
            ("Press", "120ms · ease-out · scale 0.97", 120, motion::ease_out()),
            ("Hover", "150ms · ease-out · colour only", 150, motion::ease_out()),
            ("Pop", "200ms · ease-out · from 0.95", 200, motion::ease_out()),
            ("Spring", "250ms · touch-driven, ~0.3 bounce", 250, motion::ease_spring()),
            ("Panel", "260ms · drawer curve", 260, Easing::cubic_bezier(0.32, 0.72, 0., 1.).expect("drawer curve")),
            ("Move", "on-screen movement", 400, motion::ease_move()),
        ];
        let cards: Vec<_> = cards.into_iter().enumerate().map(|(i, (name, detail, ms, curve))| {
            let at = transition(gpui_kit::ElementId::from(("motion-dot", i)), if self.played[i] { 1. } else { 0. }, Transition::new(Duration::from_millis(ms)).easing(curve), window, cx);
            board(cx)
                .id(("motion", i))
                .p_4()
                .gap_3()
                .cursor_pointer()
                .child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(name))
                .child(note(detail, cx))
                .child(
                    div().relative().h_10().squircle(ui::RADIUS_MD).bg(palette::surface(Level::Page, cx)).child(
                        // lint-allow(custom-mark): ui::badged places marks on faces; this is a motion demo's track and moving block
                        div().absolute().top_2().left_2().right_8().h_6().child(div().absolute().top_0().left(relative(at)).size_6().squircle(8.).bg(palette::accent())),
                    ),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.played[i] = !this.played[i];
                    cx.notify();
                }))
        }).collect();
        section("Motion", "Quick and a little springy. Things you touch overshoot slightly and settle inside 250ms; nothing that happens a hundred times a day animates more than a fade. Click a card to play it.", cx)
            .child(div().grid().grid_cols(4).gap_4().children(cards))
    }
}
