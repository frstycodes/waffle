// A profile's pictures: the banner and the profile effect over the card.
use std::time::Duration;

use gpui_kit::{AnyElement, App, Corners, Div, Hsla, Pixels, div, img, prelude::*, px};

use super::Ink;
use crate::{
    discord::{EffectLayer, FrameAnchor, FrameKind, FrameLayer, Profile, ProfileFrame},
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
            let tag = (!playing && is_animated(url)).then(|| ui::Badge::empty().colors(gpui_kit::black().opacity(0.6), gpui_kit::white()).absolute().top_2().left_2().child("GIF"));
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

/// The effect's layers over the whole card, scaled so its widest reaches the card's edges (`width`), each shown
/// while its timeline says, `since` the card opened, and clipped to the card's `corners`. A layer starting over
/// plays its picture again. Drawn without a hitbox, so the card under it keeps the pointer. Also returns when
/// it next changes.
pub(crate) fn effect_overlay(layers: &[EffectLayer], width: f32, corners: Corners<Pixels>, since: Duration, hovered: bool) -> Option<(Div, Option<Duration>)> {
    let span = layers.iter().map(|l| l.x + l.width).fold(0., f32::max);
    if span <= 0. {
        return None;
    }
    let k = width / span;
    let t = since.as_secs_f32() * 1000. - EFFECT_INTRO_DELAY;
    let images = layers.iter().enumerate().filter_map(|(i, l)| {
        let iteration = effect_iteration(l, t)?;
        let picture = img(l.src.clone()).play_once().id(("profile-effect", ((i as u64) << 32) | iteration)).absolute().left(px(l.x * k)).top(px(l.y * k)).w(px(l.width * k)).h(px(l.height * k));
        Some(picture)
    });
    let next = layers.iter().filter_map(|l| effect_next_change(l, t)).fold(None, |soonest: Option<f32>, ms| Some(soonest.map_or(ms, |s| s.min(ms))));
    let overlay = ui::edge(div().absolute().inset_0().overflow_hidden(), corners).when(hovered, |d| d.opacity(EFFECT_HOVERED_OPACITY)).children(images);
    Some((overlay, next.map(|ms| Duration::from_secs_f32(ms.max(1.) / 1000.))))
}

// Discord holds the intro this long after the card opens, and dims the effect while the card is hovered.
const EFFECT_INTRO_DELAY: f32 = 500.;
const EFFECT_HOVERED_OPACITY: f32 = 0.35;

// Which showing of the layer is on at `t` ms into the intro, if any. A layer without a duration stays on.
fn effect_iteration(l: &EffectLayer, t: f32) -> Option<u64> {
    let local = t - l.start;
    if local < 0. {
        return None;
    }
    if l.duration <= 0. {
        return Some(0);
    }
    if !l.looping {
        return (local < l.duration).then_some(0);
    }
    let period = l.duration + l.loop_delay.max(0.);
    ((local % period) < l.duration).then_some((local / period) as u64)
}

// Milliseconds from `t` until the layer next shows or hides; None once it never will again.
fn effect_next_change(l: &EffectLayer, t: f32) -> Option<f32> {
    let local = t - l.start;
    if local < 0. {
        return Some(-local);
    }
    if l.duration <= 0. {
        return None;
    }
    if !l.looping {
        return (local < l.duration).then_some(l.duration - local);
    }
    let period = l.duration + l.loop_delay.max(0.);
    let at = local % period;
    Some(if at < l.duration { l.duration - at } else { period - at })
}

/// A profile frame's layers around a card `width` wide: `front` the ones over its content, else the ones behind
/// it, which show only where they reach past its edges, of those `keep` lets through. Each layer is drawn at the frame's full width and its
/// own picture's height, as Discord's are; staples overhang the top or bottom, rails fill the card's height
/// from their anchor, borders repeat down it. Drawn without a hitbox.
pub(crate) fn frame_layers(frame: &ProfileFrame, width: f32, front: bool, keep: impl Fn(&FrameLayer) -> bool) -> Option<Div> {
    let layers: Vec<_> = frame.layers.iter().filter(|l| l.front == front && keep(l)).collect();
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
        let layer = |x: f32, width: f32| EffectLayer { src: "a".into(), x, y: 0., width, height: 10., z: 0, start: 0., duration: 0., looping: false, loop_delay: 0. };
        let at = |layers: &[EffectLayer]| effect_overlay(layers, WIDTH, ui::all(8.), Duration::ZERO, false).is_some();
        assert!(!at(&[]));
        assert!(!at(&[layer(0., 0.)]));
        assert!(at(&[layer(0., 450.), layer(100., 50.)]));
    }

    #[test]
    fn effect_layers_follow_their_timeline() {
        let intro = EffectLayer { src: "a".into(), x: 0., y: 0., width: 1., height: 1., z: 0, start: 0., duration: 1000., looping: false, loop_delay: 0. };
        let looped = EffectLayer { start: 2000., duration: 500., looping: true, loop_delay: 1500., ..intro.clone() };
        assert_eq!((effect_iteration(&intro, -1.), effect_iteration(&intro, 0.), effect_iteration(&intro, 1000.)), (None, Some(0), None));
        assert_eq!(effect_next_change(&intro, 400.), Some(600.));
        assert_eq!(effect_next_change(&intro, 1200.), None);
        assert_eq!((effect_iteration(&looped, 1999.), effect_iteration(&looped, 2100.), effect_iteration(&looped, 2600.)), (None, Some(0), None));
        assert_eq!(effect_iteration(&looped, 4100.), Some(1));
        assert_eq!(effect_next_change(&looped, 2600.), Some(1400.));
    }
}
