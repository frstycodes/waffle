// A profile's pictures: the banner and the profile effect over the card.
use gpui_kit::{AnyElement, App, Div, Hsla, div, img, prelude::*, px};

use super::{Ink, WIDTH};
use crate::{
    discord::{EffectLayer, Profile},
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
            if playing { picture.id("profile-banner").into_any_element() } else { picture.into_any_element() }
        }
        (None, Some((color, _)), _) => fill(color),
        (None, None, Some(accent)) => fill(palette::data_color(accent)),
        (None, None, None) => fill(palette::selected(cx)),
    };
    (el, height)
}

pub(crate) fn full_avatar(url: &str) -> String {
    crate::discord::animated(url).replacen("size=128", "size=1024", 1)
}

/// The effect's layers over the whole card, scaled so its widest reaches the card's edges, played once
/// each time the card opens (a closed card drops the images' state). Drawn without a hitbox, so the card
/// under it keeps the pointer.
pub(crate) fn effect_overlay(layers: &[EffectLayer]) -> Option<Div> {
    let span = layers.iter().map(|l| l.x + l.width).fold(0., f32::max);
    if span <= 0. {
        return None;
    }
    let k = WIDTH / span;
    let images = layers.iter().enumerate().map(|(i, l)| img(l.src.clone()).play_once().id(("profile-effect", i)).absolute().left(px(l.x * k)).top(px(l.y * k)).w(px(l.width * k)).h(px(l.height * k)));
    Some(div().absolute().inset_0().overflow_hidden().children(images))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_fills_the_card_width() {
        let layer = |x: f32, width: f32| EffectLayer { src: "a".into(), x, y: 0., width, height: 10., z: 0 };
        assert!(effect_overlay(&[]).is_none());
        assert!(effect_overlay(&[layer(0., 0.)]).is_none());
        assert!(effect_overlay(&[layer(0., 450.), layer(100., 50.)]).is_some());
    }
}
