// Faces (avatars, monograms, server and app tiles) and the marks that sit on their corners. A face takes
// marks through `badged`: one component with four corner slots that places each mark for the face's shape
// and cuts its ring out of the face, so callers never position or ring a mark by hand.
use gpui_kit::{AnyElement, ElementId, component::Icon, App, Div, FontWeight, Hsla, Img, IntoElement, ParentElement, RenderOnce, Styled, Window, div, img, linear_color_stop, linear_gradient, prelude::FluentBuilder as _, px};

use super::{
    RADIUS_LG, Squircle, count_badge,
    status_mark::{ringed_status, status_mark, status_metrics},
};
use crate::{
    discord::Mark,
    palette,
};

// A photo avatar.
pub fn avatar(url: impl Into<gpui_kit::SharedString>, size: f32) -> Img {
    img(url.into()).size(px(size)).flex_none().rounded_full()
}

// A DM's face at `size` (design: navigation.html's .gav). A pair puts the first member top left and the
// second bottom right at 0.69 of the size, the front one ringed in `ring`, the surface behind it.
pub fn dm_face(face: &crate::discord::DmFace, size: f32, ring: Hsla) -> AnyElement {
    use crate::discord::DmFace;
    match face {
        DmFace::Icon(url) | DmFace::One(url) => avatar(url.clone(), size).into_any_element(),
        DmFace::Letters(name) => monogram(name, size).into_any_element(),
        DmFace::Pair(back, front) => {
            let small = (size * PAIR_SCALE).round();
            let spread = if size >= 48. { 3. } else { 2. };
            div()
                .relative()
                .flex_none()
                .size(px(size))
                .child(div().absolute().top_0().left_0().child(avatar(back.clone(), small)))
                .child(div().absolute().right_0().bottom_0().size(px(small)).child(gap(ring, spread)).child(avatar(front.clone(), small)))
                .into_any_element()
        }
    }
}

const PAIR_SCALE: f32 = 0.69;

// An avatar without a photo: the name's initial on a gradient picked from the name. Below 24px it
// drops the initial.
pub fn monogram(name: &str, size: f32) -> Div {
    monogram_face(name_hue(name), size).rounded_full().when(size >= 24., |d| d.child(initial(name)))
}

// The monogram's gradient and type at `size`, on `hue`, with no shape or letter yet.
pub(super) fn monogram_face(hue: f32, size: f32) -> Div {
    div()
        .size(px(size))
        .flex_none()
        .bg(linear_gradient(135., linear_color_stop(palette::picture_oklch(0.53, 0.12, hue), 0.), linear_color_stop(palette::picture_oklch(0.41, 0.13, hue + 35.), 1.)))
        .flex()
        .items_center()
        .justify_center()
        .text_color(gpui_kit::white())
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(size * 0.38))
}

fn initial(name: &str) -> String {
    name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default()
}

// The design's avatar hue: a stable hash of the name onto the colour wheel.
pub fn name_hue(name: &str) -> f32 {
    name.chars().fold(7_u32, |h, c| (h * 31 + c as u32) % 360) as f32
}

// Paints `el` in the surface colour it stands in for; over the native sidebar, where that colour is
// clear, it cuts its shape out of whatever was painted under it instead, so the material shows through.
pub fn cut_out(el: Div, surface: Hsla) -> Div {
    match surface.a > 0. {
        true => el.bg(surface),
        false => el.backdrop_erase(),
    }
}

// The gap `spread` wide around a round mark or an overlapping avatar, separating it from what's under
// it. Goes first among the parent's children, so it paints before the mark.
pub fn gap(surface: Hsla, spread: f32) -> Div {
    cut_out(div().absolute().top(px(-spread)).left(px(-spread)).right(px(-spread)).bottom(px(-spread)).rounded_full(), surface)
}

// The rail's tile proportion (a 44px tile at RADIUS_LG), kept at every size a server tile is drawn.
const SERVER_TILE_ROUNDING: f32 = RADIUS_LG / 44.;

// A server's icon as a squircle tile at `size`, or its initials on the selected fill when it has none.
pub fn server_tile(icon: Option<&str>, name: &str, size: f32, cx: &App) -> AnyElement {
    let radius = size * SERVER_TILE_ROUNDING;
    if let Some(url) = icon {
        return img(url.to_owned()).size(px(size)).flex_none().squircle(radius).into_any_element();
    }
    div()
        .size(px(size))
        .flex_none()
        .squircle(radius)
        .bg(palette::selected(cx))
        .text_color(palette::strong(cx))
        .text_size(px((size * 0.38).round()))
        .font_weight(FontWeight::SEMIBOLD)
        .flex()
        .items_center()
        .justify_center()
        .child(crate::sidebar::initials(name))
        .into_any_element()
}

// An app's icon as a server tile, or without one its initial on the monogram gradient, hued by `id` so an
// app keeps its colour whatever it's called.
pub fn app_tile(icon: Option<&str>, name: &str, id: &str, size: f32, cx: &App) -> AnyElement {
    if icon.is_some() {
        return server_tile(icon, name, size, cx);
    }
    monogram_face(name_hue(id), size).squircle(size * SERVER_TILE_ROUNDING).child(initial(name)).into_any_element()
}

// A round mark holding a glyph: white on a `Solid` fill (a server's voice or stream badge), or muted on the
// face's own surface (a channel's kind on a server tile).
// Keywords: voice badge, stream badge, glyph dot, channel kind, round icon
pub fn icon_mark(icon: Icon, fill: MarkFill, size: f32, cx: &App) -> Div {
    let mark = div().size(px(size)).flex_none().rounded_full().flex().items_center().justify_center();
    match fill {
        MarkFill::Solid(fill) => mark.bg(fill).text_color(gpui_kit::white()).child(icon.size(px((size * 0.6).round()))),
        MarkFill::Surface => mark.text_color(palette::muted(cx)).child(icon.size(px((size * 0.66).round()))),
    }
}

// What an icon mark is filled with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MarkFill {
    Solid(Hsla),
    Surface,
}

// Where on a face a mark sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spot {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

// A face's outline, which decides where its corner marks land: on a round face a mark sits on the curve,
// tucked in; on a tile it hangs past the corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Round,
    Tile,
    // A control (an icon button, an upload tile): the mark hangs just past its corner.
    Control,
}

impl Shape {
    // How far a mark's outer edge sits inside the face's edge (negative: past it), and the ring cut around it.
    fn placement(self, size: f32) -> (f32, f32) {
        match self {
            Shape::Round => (size * 0.04 - 2., 2.),
            Shape::Tile => (-4., 3.),
            Shape::Control => (-3., 2.),
        }
    }
}

// One mark for a face's corner: any element, ringed by the face in its surface colour. `scale` grows it in
// (a count arriving with a folder's trip); while it's scaling it goes without its cut-out, which would
// otherwise leave a hole the size of the full mark.
pub struct Badge {
    el: Content,
    radius: Option<f32>,
    scale: f32,
    // Drawn without the cut ring: over media or a busy surface where a ring in its colour would show as a halo.
    bare: bool,
}

// A corner mark from any element (a count, an icon mark, a status), round unless given a `radius`.
// Keywords: custom mark, corner element, any badge
pub fn badge_of(el: impl IntoElement) -> Badge {
    Badge { el: Content::El(el.into_any_element()), radius: None, scale: 1., bare: false }
}

// What a mark draws: an element, or presence, which needs the theme and the face's surface and so is
// drawn when the face renders. Presence on a round face places and rings itself, from Discord's table.
enum Content {
    El(AnyElement),
    Status(Mark, f32),
    RoundStatus(Mark),
    Icon(Box<Icon>, MarkFill),
}

// The phone's corners, as a share of its width.
const PHONE_ROUNDING: f32 = 0.1875;

// An icon mark's share of its face (20px on the rail's 44px tile).
const ICON_MARK_SCALE: f32 = 0.45;

impl Badge {
    // Presence `dot` wide; `Badged::status` picks the dot for the face's size.
    pub fn status(status: impl Into<Mark>, dot: f32) -> Self {
        let status = status.into();
        let radius = (status == Mark::Mobile).then_some(dot * PHONE_ROUNDING);
        Badge { el: Content::Status(status, dot), radius, scale: 1., bare: false }
    }

    // No cut ring: the mark sits straight on the face.
    pub fn bare(mut self) -> Self {
        self.bare = true;
        self
    }

    pub fn scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }
}

// A face with marks on its corners (presence, a count, a voice or stream badge, a channel's kind): each
// placed for the face's `Shape` and ringed in `surface`, the colour the face sits on, so it reads cut out of
// the face, and two marks never touch. `ui::badged(ui::avatar(url, 40.), 40., Shape::Round, page)
// .status(mark).mark(Spot::TopRight, badge_of(...))`.
// Keywords: avatar, tile, server icon, corner, badge, dot, count, notification, status, presence, voice, stream, overlay, cut-out, ring
#[derive(IntoElement)]
pub struct Badged {
    face: AnyElement,
    size: f32,
    shape: Shape,
    surface: Hsla,
    marks: Vec<(Spot, Badge)>,
    // Keys the presence mark's morph from one status to the next; without it a change of status is a swap.
    morph: Option<ElementId>,
}

// A face (avatar, monogram, server or app tile) ready to take corner marks; see `Badged`.
// Keywords: avatar, tile, server icon, corner, badge, dot, count, notification, status, presence, voice, overlay
pub fn badged(face: impl IntoElement, size: f32, shape: Shape, surface: Hsla) -> Badged {
    Badged { face: face.into_any_element(), size, shape, surface, marks: Vec::new(), morph: None }
}

impl Badged {
    // A mark at `spot`; a later mark at the same spot replaces the earlier one.
    pub fn mark(mut self, spot: Spot, badge: Badge) -> Self {
        self.marks.retain(|(s, _)| *s != spot);
        self.marks.push((spot, badge));
        self
    }

    // Presence at the bottom-right, sized for the face: Discord's avatar table on a round face, 30% of a
    // tile (9 to 16px).
    pub fn status(self, status: Option<impl Into<Mark>>) -> Self {
        let Some(status) = status.map(Into::into) else { return self };
        let badge = match self.shape {
            Shape::Round => Badge { el: Content::RoundStatus(status), radius: None, scale: 1., bare: false },
            Shape::Tile | Shape::Control => Badge::status(status, (self.size * 0.3).clamp(9., 16.).round()),
        };
        self.mark(Spot::BottomRight, badge)
    }

    // Morphs the presence mark between statuses as Discord does, keyed by `id` (the person's or the row's).
    pub fn morph(mut self, id: impl Into<ElementId>) -> Self {
        self.morph = Some(id.into());
        self
    }

    // An icon mark at `spot`, sized for the face.
    pub fn icon(self, spot: Spot, icon: Icon, fill: MarkFill) -> Self {
        self.mark(spot, Badge { el: Content::Icon(Box::new(icon), fill), radius: None, scale: 1., bare: false })
    }

    // A count at the bottom-right, red as counts are; nothing for zero.
    pub fn count(self, count: u32) -> Self {
        self.count_scaled(count, 1.)
    }

    // The same, grown in to `scale`.
    pub fn count_scaled(self, count: u32, scale: f32) -> Self {
        match count > 0 && scale > 0.01 {
            true => self.mark(Spot::BottomRight, badge_of(count_badge(count)).scale(scale)),
            false => self,
        }
    }
}

impl RenderOnce for Badged {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (inset, ring) = self.shape.placement(self.size);
        let at = inset - ring;
        let (surface, size, morph) = (self.surface, self.size, self.morph);
        let marks: Vec<AnyElement> = self.marks.into_iter().map(|(spot, badge)| {
            let el = match badge.el {
                Content::El(el) => el,
                Content::Status(status, dot) => status_mark(status, dot).into_any_element(),
                Content::RoundStatus(status) => return ringed_status(status, status_metrics(size), surface, morph.clone(), window, cx).into_any_element(),
                Content::Icon(icon, fill) => icon_mark(*icon, fill, (size * ICON_MARK_SCALE).round(), cx).into_any_element(),
            };
            let ring = if badge.bare { 0. } else { ring };
            let at = if badge.bare { inset } else { at };
            let holder = div().absolute().p(px(ring)).flex().items_center().justify_center();
            // The ring follows the mark's own corner, grown by the ring's width.
            let holder = match badge.radius {
                Some(radius) => holder.rounded(px(radius + ring)),
                None => holder.rounded_full(),
            };
            let holder = match spot {
                Spot::TopLeft => holder.top(px(at)).left(px(at)),
                Spot::TopRight => holder.top(px(at)).right(px(at)),
                Spot::BottomLeft => holder.bottom(px(at)).left(px(at)),
                Spot::BottomRight => holder.bottom(px(at)).right(px(at)),
            };
            let holder = if badge.bare { holder } else if badge.scale >= 0.999 { cut_out(holder, surface) } else { holder.scale(badge.scale.max(0.)).opacity(badge.scale.clamp(0., 1.)) };
            holder.child(el).into_any_element()
        }).collect();
        div().relative().flex_none().size(px(self.size)).child(self.face).children(marks)
    }
}
