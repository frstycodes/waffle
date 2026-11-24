// A profile's pictures: the banner and the profile effect over the card.
use gpui_kit::{AnyElement, App, Div, Hsla, div, img, prelude::*, px};

use super::Ink;
use crate::{
    discord::{EffectLayer, FrameAnchor, FrameKind, Profile, ProfileFrame},
    palette, ui,
};

// Discord's banner heights (`--custom-user-profile-banner-height`): the popout's and side panel's, and the full
// profile's. With or without a picture the header keeps its height.
pub(crate) const BANNER: f32 = 105.;
pub(crate) const BANNER_FULL: f32 = 140.;

/// The banner `height` tall: the picture, cover-fit, else a strip of the theme or accent colour. An
/// animated banner holds its first frame unless `playing` (gpui animates only an image with an id).
pub(crate) fn banner(profile: &Profile, ink: Ink, radius: f32, playing: bool, height: f32, cx: &App) -> (AnyElement, f32) {
    let fill = |color: Hsla| ui::edge(div().w_full().h(px(height)).bg(color), ui::top(radius)).into_any_element();
    let el = match (&profile.banner, ink.border, profile.accent) {
        (Some(url), _, _) => {
            let picture = ui::media(url.clone(), ui::top(radius)).w_full().h(px(height)).object_fit(gpui_kit::ObjectFit::Cover);
            let picture = if playing { picture.id("profile-banner").into_any_element() } else { picture.into_any_element() };
            // A still animated banner says so, as Discord's GIF tag does.
            let tag = (!playing && is_animated(url)).then(|| ui::badge(gpui_kit::black().opacity(0.6), gpui_kit::white()).absolute().top_2().left_2().child("GIF"));
            div().relative().w_full().h(px(height)).child(picture).children(tag).into_any_element()
        }
        (None, Some((color, _)), _) => fill(color),
        (None, None, Some(accent)) => fill(palette::data_color(accent)),
        (None, None, None) => fill(palette::selected(cx)),
    };
    (el, height)
}

// Discord marks an animated asset's hash with `a_`; our fixtures are plain `.gif` files.
fn is_animated(url: &str) -> bool {
    url.contains("/a_") || url.split('?').next().is_some_and(|path| path.ends_with(".gif"))
}

pub(crate) fn full_avatar(url: &str) -> String {
    crate::discord::animated(url).replacen("size=128", "size=1024", 1)
}

/// The effect's layers over the whole card, scaled so its widest reaches the card's edges (`width`), played once
/// each time the card opens (a closed card drops the images' state). Drawn without a hitbox, so the card
/// under it keeps the pointer.
pub(crate) fn effect_overlay(layers: &[EffectLayer], width: f32) -> Option<Div> {
    let span = layers.iter().map(|l| l.x + l.width).fold(0., f32::max);
    if span <= 0. {
        return None;
    }
    let k = width / span;
    let images = layers.iter().enumerate().map(|(i, l)| img(l.src.clone()).play_once().id(("profile-effect", i)).absolute().left(px(l.x * k)).top(px(l.y * k)).w(px(l.width * k)).h(px(l.height * k)));
    Some(div().absolute().inset_0().overflow_hidden().children(images))
}

/// A profile frame's layers around a card `width` wide: `front` the ones over its content, else the ones behind
/// it, which show only where they reach past its edges. Each layer is drawn at the frame's full width and its
/// own picture's height, as Discord's are; staples overhang the top or bottom, rails fill the card's height
/// from their anchor, borders repeat down it. Drawn without a hitbox.
pub(crate) fn frame_layers(frame: &ProfileFrame, width: f32, front: bool) -> Option<Div> {
    let layers: Vec<_> = frame.layers.iter().filter(|l| l.front == front).collect();
    if layers.is_empty() || frame.inner_width <= 0. {
        return None;
    }
    let k = width / frame.inner_width;
    let side = frame.overflow_horizontal * k;
    // gpui sizes a picture by its own aspect ratio only from a definite width, not a percentage.
    let span = width + 2. * side;
    let pieces = layers.into_iter().enumerate().map(|(i, layer)| {
        let picture = || img(layer.src.clone()).w(px(span)).flex_none();
        match (layer.kind, layer.anchor) {
            (FrameKind::Staple, FrameAnchor::Top) => div().absolute().left_0().right_0().top(px(-frame.overflow_top * k)).child(picture()),
            (FrameKind::Staple, FrameAnchor::Bottom) => div().absolute().left_0().right_0().bottom(px(-frame.overflow_bottom * k)).child(picture()),
            (FrameKind::Staple, FrameAnchor::Center) | (FrameKind::Rail, _) => {
                let strip = ui::col().absolute().inset_0().overflow_hidden().child(picture());
                match layer.anchor {
                    FrameAnchor::Top => strip.justify_start(),
                    FrameAnchor::Center => strip.justify_center(),
                    FrameAnchor::Bottom => strip.justify_end(),
                }
            }
            (FrameKind::Border, _) => ui::col().absolute().inset_0().overflow_hidden().children((0..BORDER_TILES).map(|_| picture())),
        }
        .id(("profile-frame-layer", i))
    });
    Some(div().absolute().top_0().bottom_0().left(px(-side)).right(px(-side)).children(pieces))
}

// Enough copies of a border piece to run down the tallest card; the rest is cut at its foot.
const BORDER_TILES: usize = 24;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::WIDTH;

    #[test]
    fn effect_fills_the_card_width() {
        let layer = |x: f32, width: f32| EffectLayer { src: "a".into(), x, y: 0., width, height: 10., z: 0 };
        assert!(effect_overlay(&[], WIDTH).is_none());
        assert!(effect_overlay(&[layer(0., 0.)], WIDTH).is_none());
        assert!(effect_overlay(&[layer(0., 450.), layer(100., 50.)], WIDTH).is_some());
    }
}
