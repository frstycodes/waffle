// A profile's pictures: the banner and the profile effect over the card.
use gpui_kit::{AnyElement, App, Div, Hsla, div, img, prelude::*, px};

use super::{Ink, WIDTH};
use crate::{
    discord::{EffectLayer, Profile},
    palette, ui,
};

// Discord banners are 5:2; at that height a cover fit crops nothing.
const BANNER: f32 = WIDTH * 0.4;
const BANNER_PLAIN: f32 = 64.;

/// The banner with its height: the picture at 5:2, else a strip of the theme or accent colour. An
/// animated banner holds its first frame unless `playing` (gpui animates only an image with an id).
pub(crate) fn banner(profile: &Profile, ink: Ink, radius: f32, playing: bool, cx: &App) -> (AnyElement, f32) {
    match (&profile.banner, ink.border, profile.accent) {
        (Some(url), _, _) => {
            let picture = ui::media(url.clone(), ui::top(radius)).w_full().h(px(BANNER)).object_fit(gpui_kit::ObjectFit::Cover);
            (if playing { picture.id("profile-banner").into_any_element() } else { picture.into_any_element() }, BANNER)
        }
        (None, Some((color, _)), _) => (banner_fill(color, radius).into_any_element(), BANNER_PLAIN),
        (None, None, Some(accent)) => (banner_fill(palette::data_color(accent), radius).into_any_element(), BANNER_PLAIN),
        (None, None, None) => (banner_fill(palette::selected(cx), radius).into_any_element(), BANNER_PLAIN),
    }
}

fn banner_fill(color: Hsla, radius: f32) -> Div {
    ui::edge(div().w_full().h(px(BANNER_PLAIN)).bg(color), ui::top(radius))
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
