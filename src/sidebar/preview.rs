// A server's hover card on the rail, as Discord's: its name, then who's talking in voice and who's live.
use gpui_kit::{Context, FontWeight, IntoElement, ParentElement, Render, SharedString, Styled, Window, component::Icon, div, prelude::FluentBuilder as _, px};

use crate::ui::prelude::*;

// Six faces at most per row, the last a "+n" when they don't all fit; the card stays a glance, not a roster.
const PREVIEW_FACES: usize = 6;
const PREVIEW_WIDTH: f32 = 280.;

#[derive(Clone)]
pub(super) struct ServerPreview {
    pub(super) name: SharedString,
    pub(super) voice: Vec<ui::Entry>,
    pub(super) live: Vec<ui::Entry>,
    pub(super) muted: bool,
}

impl ServerPreview {
    pub(super) fn has_activity(&self) -> bool {
        !self.voice.is_empty() || !self.live.is_empty()
    }
}

impl Render for ServerPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::perf::probe("ServerPreview");
        let surface = palette::surface(palette::Level::Overlay, cx);
        let muted = palette::muted(cx);
        let (wash, strong) = (palette::selected(cx), palette::strong(cx));
        let row = |key: &'static str, title: &'static str, glyph: Glyph, people: &[ui::Entry]| {
            ui::row().gap_2().child(Icon::new(glyph).size(ui::ICON_MD).text_color(muted)).child(ui::AvatarGroup::listed(key, title, people.to_vec(), PREVIEW_FACES, ui::AVATAR_MD, surface, wash, strong))
        };
        // Anchored flush to the tile; plain tooltips get their 12px gap from Tooltip's own margin.
        div().pl_3().child(
            ui::PopoverContent::new(cx)
                .max_w(px(PREVIEW_WIDTH))
                .p_3()
                .gap_2()
                .child(ui::line(self.name.clone()).text_size(ui::TEXT_SM).font_weight(FontWeight::SEMIBOLD).text_color(strong))
                .when(!self.voice.is_empty(), |c| c.child(row("server-preview-voice", "In voice", Glyph::VolumeHigh, &self.voice)))
                .when(!self.live.is_empty(), |c| c.child(row("server-preview-live", "Live", Glyph::ShareScreen, &self.live)))
                .when(self.muted, |c| c.child(div().text_size(ui::TEXT_XS).text_color(muted).child("Muted"))),
        )
    }
}
