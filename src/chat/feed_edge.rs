// Where the feed meets the composer: the feed stops at the composer's top edge, so nothing scrolls under it, and its
// last FEED_FADE px fade into the page so the cut isn't a hard line. The typing line sits on that fade.
use gpui_kit::{App, Div, IntoElement, ParentElement, Styled, div, linear_color_stop, linear_gradient, px};

use crate::{Waffle, palette::{self, Level}};

// How tall the fade is.
const FEED_FADE: f32 = 32.;
// The typing line's room above the composer (the line sits 28px up, 24px tall).
const TYPING_LINE: f32 = 24.;
// What the feed's foot keeps clear, so its last message rests above the typing line and the fade.
pub(crate) const FEED_FOOT: f32 = FEED_FADE + TYPING_LINE;

impl Waffle {
    // `feed` in a box that ends at the composer's top, with the fade along its foot. The composer itself is laid
    // absolutely at the foot of the same box's parent, as before.
    pub(crate) fn feed_above_composer(&self, feed: impl IntoElement, cx: &App) -> Div {
        let page = palette::surface(Level::Page, cx);
        let fade = div().absolute().left_0().right_0().bottom_0().h(px(FEED_FADE)).bg(linear_gradient(180., linear_color_stop(page.opacity(0.), 0.), linear_color_stop(page, 1.)));
        let above = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            // lint-allow(raw-px): the composer's measured height
            .bottom(px(self.composer_height.get()))
            .child(feed)
            .child(fade);
        div().relative().flex_1().min_h_0().child(above)
    }
}
