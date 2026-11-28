// Display: Badge and tag, Count badge, Role chip, Keycap, Divider, Progress, Mic meter, Skeleton, Icon tile.
use super::*;

fn variant(title: &'static str, lede_text: &'static str, content: Div, cx: &App) -> Frame {
    (Kind::Variants, section(title, lede_text, cx).child(board(cx).child(content)).into_any_element())
}

pub(super) fn badge_frames(cx: &App) -> Vec<Frame> {
    vec![variant("Badge / Kind", "Tags label what something is: accent ink on the accent tint.", row().gap_4().child(ui::Badge::new("App").variant(ui::BadgeVariant::Accent)).child(ui::Badge::new("Host").variant(ui::BadgeVariant::Accent).icon(Glyph::Crown)).child(ui::Badge::new("New").variant(ui::BadgeVariant::Accent)).child(ui::Badge::live()), cx)]
}

pub(super) fn count_badge_frames(cx: &App) -> Vec<Frame> {
    let marks = row().gap_4().child(ui::Badge::count(3)).child(ui::Badge::count(42)).child(ui::Badge::count(120)).child(ui::Badge::dot()).child(ui::number_mark(1, cx)).child(ui::number_mark(12, cx));
    vec![variant("Count badge / Kind", "A count asks for you, so it takes the alert red. Without a number, an unread dot. Number marks number steps.", marks, cx)]
}

pub(super) fn role_toggle_frames(cx: &App) -> Vec<Frame> {
    let chips = row()
        .child(ui::role_chip("Moderators", palette::oklch(0.65, 0.2, 5.), cx))
        .child(ui::role_chip("Raid team", palette::oklch(0.7, 0.14, 155.), cx))
        .child(ui::role_chip("Night owls", palette::oklch(0.75, 0.14, 80.), cx))
        .child(ui::role_chip("Members", palette::status_in(Status::Offline, cx), cx))
        .child(ui::role_chip("Rustacean", palette::data_color(0xE67E22), cx).child(ui::chip_remove("ds-role-remove", palette::muted(cx), palette::strong(cx), ui::REMOVE_GLYPH_SM, |_, _, _| {})));
    vec![variant("Role chip / Kind", "The role's colour is a dot, never text colour; the capsule is tinted ~14% light, 20% dark.", chips, cx)]
}

pub(super) fn keycap_frames(cx: &App) -> Vec<Frame> {
    let keys = row()
        .gap_6()
        .child(ui::key_hint(ui::KbdGroup::new(["⌘", "K"]), "Quick switcher", cx))
        .child(ui::key_hint(ui::KbdGroup::new(["Esc"]), "Close", cx))
        .child(ui::Kbd::glyph(Glyph::ChevronUp))
        .child(ui::row().p_2().bg(palette::danger()).child(ui::Kbd::new("⌫").translucent(palette::on_accent())));
    vec![variant("Keycap / Kind", "A raised cap with a deeper bottom edge, so it reads as a key you could press; translucent on a coloured fill.", keys, cx)]
}

pub(super) fn divider_frames(cx: &App) -> Vec<Frame> {
    let content = ui::col()
        .gap_4()
        .child(ui::Separator::new().label("Today"))
        .child(ui::Separator::new().w_full())
        .child(ui::Separator::new().label("Monday, March 3, 2025").ink(palette::faint(cx)).color(palette::line(cx)))
        .child(ui::Separator::new().label("NEW").ink(palette::alert()).color(palette::alert()))
        .child(row().gap_6().child(ui::section_label("Online — 4", ui::Heading::Muted, cx)).child(ui::section_label("Recent", ui::Heading::Faint, cx)).child(ui::section_label("Channel name", ui::Heading::Strong, cx)))
        .child(ui::row().gap_4().h_8().child(note("Seam", cx)).child(ui::Separator::new().orientation(ui::Orientation::Vertical)).child(note("between a toolbar's groups", cx)));
    vec![variant("Divider / Kind", "A label on a divider names what follows; the NEW divider takes the alert red.", content, cx)]
}

pub(super) fn icon_tile_frames(cx: &App) -> Vec<Frame> {
    let tiles = row()
        .gap_4()
        .child(ui::ItemMedia::icon(Glyph::Hashtag, palette::selected(cx), palette::strong(cx)))
        .child(ui::ItemMedia::icon(Glyph::Stream, palette::accent(), palette::on_accent()))
        .child(ui::ItemMedia::icon(Glyph::Trash, palette::danger_tint().opacity(1.25), palette::danger_ink(cx)));
    vec![variant("Icon tile / Fill", "Leads rows that need more presence than a bare icon.", tiles, cx)]
}

impl DesignSystem {
    pub(super) fn progress_frames(&self, _window: &mut Window, cx: &mut Context<Self>) -> Vec<Frame> {
        let strong = |text: &'static str, cx: &App| div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(text);
        let content = div()
            .grid()
            .grid_cols(3)
            .gap_6()
            .child(stack().child(ui::row().items_stretch().justify_between().child(strong("clip.mp4", cx)).child(div().text_color(palette::faint(cx)).child("12.4 / 31 MB"))).child(ui::Progress::new(0.4).animated("p-clip")))
            .child(stack().child(strong("Connecting to Lounge…", cx)).child(ui::Progress::indeterminate("p-connect").into_any_element()))
            .child(row().child(ui::ring(0.72, cx)).child(div().text_color(palette::muted(cx)).child("18 of 25 uploaded")))
            .child(stack().child(strong("Thin", cx)).child(div().w_40().child(ui::Progress::new(0.62).fill(palette::positive_ink(cx)).track(palette::line(cx)).thin())));
        vec![variant("Progress / Kind", "Progress shows a count when the work is countable. Waits look alive: the wave, or a calm shimmer that sweeps then rests.", content, cx)]
    }

    pub(super) fn meter_frames(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Frame> {
        let level = self.meter_level();
        // lint-allow(endless-animation): drawn only on the design-system page, which exists to show it moving.
        window.request_animation_frame();
        vec![variant("Mic meter / Level", "Equal bars that light up to the level, turning amber then red near the top. Here it follows a random level every 90ms.", ui::col().w_80().child(ui::meter(level, 24, cx)), cx)]
    }

    // The page's meter script: every 90ms the level decays and takes a random kick.
    pub(super) fn meter_level(&mut self) -> f32 {
        let step = self.started.elapsed().as_millis() / 90;
        if step != self.meter.0 {
            let noise = ((step.wrapping_mul(2654435761) % 1000) as f32) / 1000.;
            self.meter = (step, (self.meter.1 * 0.82 + noise * 0.35).clamp(0., 1.));
        }
        self.meter.1
    }

    pub(super) fn skeleton_frames(&self, cx: &mut Context<Self>) -> Vec<Frame> {
        let skeleton_row = |id: &'static str, widths: (f32, f32), block: bool, _cx: &App| {
            ui::row().gap_2().child(ui::Skeleton::circle((id, 0usize), 40.)).child(
                stack()
                    .flex_1()
                    .child(ui::Skeleton::new((id, 1usize)).radius(ui::RADIUS_XS).h_3().w(px(widths.0)))
                    .child(ui::Skeleton::new((id, 2usize)).radius(ui::RADIUS_XS).h_3().w(relative(widths.1)))
                    .when(block, |d| d.child(ui::Skeleton::new((id, 3usize)).radius(ui::RADIUS_LG).w(px(SKELETON_BLOCK.0)).h(px(SKELETON_BLOCK.1)))),
            )
        };
        vec![variant("Skeleton / Shape", "Line, block and circle share one sweep that rests between passes.", ui::col().gap_3().child(skeleton_row("sk-a", (120., 0.72), false, cx)).child(skeleton_row("sk-b", (90., 0.48), true, cx)), cx)]
    }
}
