// todo("windows"): remove
#![cfg_attr(windows, allow(dead_code))]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AtlasTextureId, AtlasTile, Background, Bounds, ContentMask, Corners, DevicePixels, Edges, Hsla,
    Pixels, Point, Radians, ScaledPixels, Size, bounds_tree::BoundsTree, point, size,
};
use std::{
    fmt::Debug,
    iter::Peekable,
    ops::{Add, Range, Sub},
    slice,
};

#[allow(non_camel_case_types, unused)]
#[expect(missing_docs)]
pub type PathVertex_ScaledPixels = PathVertex<ScaledPixels>;

#[expect(missing_docs)]
pub type DrawOrder = u32;

/// A boolean stored as a `u32` so that GPU-facing structs contain no
/// compiler-inserted padding bytes, which would be undefined behavior to
/// reinterpret as `&[u8]` when writing instance buffers. Guaranteed to be
/// `0` or `1` by construction; shaders read it as a `u32`/`uint`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
pub struct PaddedBool32(u32);

impl From<bool> for PaddedBool32 {
    fn from(value: bool) -> Self {
        PaddedBool32(value as u32)
    }
}

#[derive(Default)]
#[expect(missing_docs)]
pub struct Scene {
    pub(crate) paint_operations: Vec<PaintOperation>,
    primitive_bounds: BoundsTree<ScaledPixels>,
    layer_stack: Vec<DrawOrder>,
    pub shadows: Vec<Shadow>,
    pub quads: Vec<Quad>,
    pub paths: Vec<Path<ScaledPixels>>,
    pub underlines: Vec<Underline>,
    pub monochrome_sprites: Vec<MonochromeSprite>,
    pub subpixel_sprites: Vec<SubpixelSprite>,
    pub polychrome_sprites: Vec<PolychromeSprite>,
    pub surfaces: Vec<PaintSurface>,
    pub backdrop_blurs: Vec<BackdropBlur>,
}

#[expect(missing_docs)]
impl Scene {
    pub fn clear(&mut self) {
        self.paint_operations.clear();
        self.primitive_bounds.clear();
        self.layer_stack.clear();
        self.paths.clear();
        self.shadows.clear();
        self.quads.clear();
        self.underlines.clear();
        self.monochrome_sprites.clear();
        self.subpixel_sprites.clear();
        self.polychrome_sprites.clear();
        self.surfaces.clear();
        self.backdrop_blurs.clear();
    }

    pub fn len(&self) -> usize {
        self.paint_operations.len()
    }

    pub fn push_layer(&mut self, bounds: Bounds<ScaledPixels>) {
        let order = self.primitive_bounds.insert(bounds);
        self.layer_stack.push(order);
        self.paint_operations
            .push(PaintOperation::StartLayer(bounds));
    }

    pub fn pop_layer(&mut self) {
        self.layer_stack.pop();
        self.paint_operations.push(PaintOperation::EndLayer);
    }

    pub fn insert_primitive(&mut self, primitive: impl Into<Primitive>) {
        let mut primitive = primitive.into();
        let clipped_bounds = primitive
            .transformed_bounds()
            .intersect(&primitive.content_mask().bounds);

        if clipped_bounds.is_empty() {
            return;
        }

        let order = self
            .layer_stack
            .last()
            .copied()
            .unwrap_or_else(|| self.primitive_bounds.insert(clipped_bounds));
        match &mut primitive {
            Primitive::Shadow(shadow) => {
                shadow.order = order;
                self.shadows.push(*shadow);
            }
            Primitive::Quad(quad) => {
                quad.order = order;
                self.quads.push(*quad);
            }
            Primitive::Path(path) => {
                path.order = order;
                path.id = PathId(self.paths.len());
                self.paths.push(path.clone());
            }
            Primitive::Underline(underline) => {
                underline.order = order;
                self.underlines.push(*underline);
            }
            Primitive::MonochromeSprite(sprite) => {
                sprite.order = order;
                self.monochrome_sprites.push(*sprite);
            }
            Primitive::SubpixelSprite(sprite) => {
                sprite.order = order;
                self.subpixel_sprites.push(*sprite);
            }
            Primitive::PolychromeSprite(sprite) => {
                sprite.order = order;
                self.polychrome_sprites.push(*sprite);
            }
            Primitive::Surface(surface) => {
                surface.order = order;
                self.surfaces.push(surface.clone());
            }
            Primitive::BackdropBlur(blur) => {
                blur.order = order;
                self.backdrop_blurs.push(*blur);
            }
        }
        self.paint_operations
            .push(PaintOperation::Primitive(primitive));
    }

    pub fn replay(&mut self, range: Range<usize>, prev_scene: &Scene) {
        for operation in &prev_scene.paint_operations[range] {
            match operation {
                PaintOperation::Primitive(primitive) => self.insert_primitive(primitive.clone()),
                PaintOperation::StartLayer(bounds) => self.push_layer(*bounds),
                PaintOperation::EndLayer => self.pop_layer(),
            }
        }
    }

    pub fn finish(&mut self) {
        self.shadows.sort_by_key(|shadow| shadow.order);
        self.quads.sort_by_key(|quad| quad.order);
        self.paths.sort_by_key(|path| path.order);
        self.underlines.sort_by_key(|underline| underline.order);
        self.monochrome_sprites
            .sort_by_key(|sprite| (sprite.order, sprite.tile.texture_id.index));
        self.subpixel_sprites
            .sort_by_key(|sprite| (sprite.order, sprite.tile.texture_id.index));
        self.polychrome_sprites
            .sort_by_key(|sprite| (sprite.order, sprite.tile.texture_id.index));
        self.surfaces.sort_by_key(|surface| surface.order);
        self.backdrop_blurs.sort_by_key(|blur| blur.order);
    }

    #[cfg_attr(
        all(
            any(target_os = "linux", target_os = "freebsd"),
            not(any(feature = "x11", feature = "wayland"))
        ),
        allow(dead_code)
    )]
    pub fn batches(&self) -> impl Iterator<Item = PrimitiveBatch> + '_ {
        BatchIterator {
            shadows_start: 0,
            shadows_iter: self.shadows.iter().peekable(),
            quads_start: 0,
            quads_iter: self.quads.iter().peekable(),
            paths_start: 0,
            paths_iter: self.paths.iter().peekable(),
            underlines_start: 0,
            underlines_iter: self.underlines.iter().peekable(),
            monochrome_sprites_start: 0,
            monochrome_sprites_iter: self.monochrome_sprites.iter().peekable(),
            subpixel_sprites_start: 0,
            subpixel_sprites_iter: self.subpixel_sprites.iter().peekable(),
            polychrome_sprites_start: 0,
            polychrome_sprites_iter: self.polychrome_sprites.iter().peekable(),
            surfaces_start: 0,
            surfaces_iter: self.surfaces.iter().peekable(),
            backdrop_blurs_start: 0,
            backdrop_blurs_iter: self.backdrop_blurs.iter().peekable(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Default)]
#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
pub(crate) enum PrimitiveKind {
    // First, so a blur sharing a draw order with other primitives samples the backdrop before
    // they are drawn over it.
    BackdropBlur,
    Shadow,
    #[default]
    Quad,
    Path,
    Underline,
    MonochromeSprite,
    SubpixelSprite,
    PolychromeSprite,
    Surface,
}

pub(crate) enum PaintOperation {
    Primitive(Primitive),
    StartLayer(Bounds<ScaledPixels>),
    EndLayer,
}

#[derive(Clone)]
#[expect(missing_docs)]
pub enum Primitive {
    Shadow(Shadow),
    Quad(Quad),
    Path(Path<ScaledPixels>),
    Underline(Underline),
    MonochromeSprite(MonochromeSprite),
    SubpixelSprite(SubpixelSprite),
    PolychromeSprite(PolychromeSprite),
    Surface(PaintSurface),
    BackdropBlur(BackdropBlur),
}

#[expect(missing_docs)]
impl Primitive {
    pub fn bounds(&self) -> &Bounds<ScaledPixels> {
        match self {
            Primitive::Shadow(shadow) => &shadow.bounds,
            Primitive::Quad(quad) => &quad.bounds,
            Primitive::Path(path) => &path.bounds,
            Primitive::Underline(underline) => &underline.bounds,
            Primitive::MonochromeSprite(sprite) => &sprite.bounds,
            Primitive::SubpixelSprite(sprite) => &sprite.bounds,
            Primitive::PolychromeSprite(sprite) => &sprite.bounds,
            Primitive::Surface(surface) => &surface.bounds,
            Primitive::BackdropBlur(blur) => &blur.bounds,
        }
    }

    pub fn content_mask(&self) -> &ContentMask<ScaledPixels> {
        match self {
            Primitive::Shadow(shadow) => &shadow.content_mask,
            Primitive::Quad(quad) => &quad.content_mask,
            Primitive::Path(path) => &path.content_mask,
            Primitive::Underline(underline) => &underline.content_mask,
            Primitive::MonochromeSprite(sprite) => &sprite.content_mask,
            Primitive::SubpixelSprite(sprite) => &sprite.content_mask,
            Primitive::PolychromeSprite(sprite) => &sprite.content_mask,
            Primitive::Surface(surface) => &surface.content_mask,
            Primitive::BackdropBlur(blur) => &blur.content_mask,
        }
    }

    /// The bounds this primitive covers once its transformation is applied.
    pub fn transformed_bounds(&self) -> Bounds<ScaledPixels> {
        let transformation = match self {
            Primitive::Shadow(shadow) => shadow.transformation,
            Primitive::Quad(quad) => quad.transformation,
            Primitive::Underline(underline) => underline.transformation,
            Primitive::MonochromeSprite(sprite) => sprite.transformation,
            Primitive::SubpixelSprite(sprite) => sprite.transformation,
            Primitive::PolychromeSprite(sprite) => sprite.transformation,
            // Paths are transformed on the CPU and backdrop blurs before they're painted (see
            // `BackdropFilter::transformed`), so their bounds are already in window space.
            // GAP: video surfaces are not transformed.
            Primitive::Path(_) | Primitive::Surface(_) | Primitive::BackdropBlur(_) => TransformationMatrix::unit(),
        };
        transformation.transform_bounds(*self.bounds())
    }
}

#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
struct BatchIterator<'a> {
    shadows_start: usize,
    shadows_iter: Peekable<slice::Iter<'a, Shadow>>,
    quads_start: usize,
    quads_iter: Peekable<slice::Iter<'a, Quad>>,
    paths_start: usize,
    paths_iter: Peekable<slice::Iter<'a, Path<ScaledPixels>>>,
    underlines_start: usize,
    underlines_iter: Peekable<slice::Iter<'a, Underline>>,
    monochrome_sprites_start: usize,
    monochrome_sprites_iter: Peekable<slice::Iter<'a, MonochromeSprite>>,
    subpixel_sprites_start: usize,
    subpixel_sprites_iter: Peekable<slice::Iter<'a, SubpixelSprite>>,
    polychrome_sprites_start: usize,
    polychrome_sprites_iter: Peekable<slice::Iter<'a, PolychromeSprite>>,
    surfaces_start: usize,
    surfaces_iter: Peekable<slice::Iter<'a, PaintSurface>>,
    backdrop_blurs_start: usize,
    backdrop_blurs_iter: Peekable<slice::Iter<'a, BackdropBlur>>,
}

impl<'a> Iterator for BatchIterator<'a> {
    type Item = PrimitiveBatch;

    fn next(&mut self) -> Option<Self::Item> {
        let mut orders_and_kinds = [
            (
                self.shadows_iter.peek().map(|s| s.order),
                PrimitiveKind::Shadow,
            ),
            (self.quads_iter.peek().map(|q| q.order), PrimitiveKind::Quad),
            (self.paths_iter.peek().map(|q| q.order), PrimitiveKind::Path),
            (
                self.underlines_iter.peek().map(|u| u.order),
                PrimitiveKind::Underline,
            ),
            (
                self.monochrome_sprites_iter.peek().map(|s| s.order),
                PrimitiveKind::MonochromeSprite,
            ),
            (
                self.subpixel_sprites_iter.peek().map(|s| s.order),
                PrimitiveKind::SubpixelSprite,
            ),
            (
                self.polychrome_sprites_iter.peek().map(|s| s.order),
                PrimitiveKind::PolychromeSprite,
            ),
            (
                self.surfaces_iter.peek().map(|s| s.order),
                PrimitiveKind::Surface,
            ),
            (
                self.backdrop_blurs_iter.peek().map(|b| b.order),
                PrimitiveKind::BackdropBlur,
            ),
        ];
        orders_and_kinds.sort_by_key(|(order, kind)| (order.unwrap_or(u32::MAX), *kind));

        let first = orders_and_kinds[0];
        let second = orders_and_kinds[1];
        let (batch_kind, max_order_and_kind) = if first.0.is_some() {
            (first.1, (second.0.unwrap_or(u32::MAX), second.1))
        } else {
            return None;
        };

        match batch_kind {
            PrimitiveKind::Shadow => {
                let shadows_start = self.shadows_start;
                let mut shadows_end = shadows_start + 1;
                self.shadows_iter.next();
                while self
                    .shadows_iter
                    .next_if(|shadow| (shadow.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    shadows_end += 1;
                }
                self.shadows_start = shadows_end;
                Some(PrimitiveBatch::Shadows(shadows_start..shadows_end))
            }
            PrimitiveKind::Quad => {
                let quads_start = self.quads_start;
                let mut quads_end = quads_start + 1;
                self.quads_iter.next();
                while self
                    .quads_iter
                    .next_if(|quad| (quad.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    quads_end += 1;
                }
                self.quads_start = quads_end;
                Some(PrimitiveBatch::Quads(quads_start..quads_end))
            }
            PrimitiveKind::Path => {
                let paths_start = self.paths_start;
                let mut paths_end = paths_start + 1;
                self.paths_iter.next();
                while self
                    .paths_iter
                    .next_if(|path| (path.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    paths_end += 1;
                }
                self.paths_start = paths_end;
                Some(PrimitiveBatch::Paths(paths_start..paths_end))
            }
            PrimitiveKind::Underline => {
                let underlines_start = self.underlines_start;
                let mut underlines_end = underlines_start + 1;
                self.underlines_iter.next();
                while self
                    .underlines_iter
                    .next_if(|underline| (underline.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    underlines_end += 1;
                }
                self.underlines_start = underlines_end;
                Some(PrimitiveBatch::Underlines(underlines_start..underlines_end))
            }
            PrimitiveKind::MonochromeSprite => {
                let texture_id = self.monochrome_sprites_iter.peek().unwrap().tile.texture_id;
                let sprites_start = self.monochrome_sprites_start;
                let mut sprites_end = sprites_start + 1;
                self.monochrome_sprites_iter.next();
                while self
                    .monochrome_sprites_iter
                    .next_if(|sprite| {
                        (sprite.order, batch_kind) < max_order_and_kind
                            && sprite.tile.texture_id == texture_id
                    })
                    .is_some()
                {
                    sprites_end += 1;
                }
                self.monochrome_sprites_start = sprites_end;
                Some(PrimitiveBatch::MonochromeSprites {
                    texture_id,
                    range: sprites_start..sprites_end,
                })
            }
            PrimitiveKind::SubpixelSprite => {
                let texture_id = self.subpixel_sprites_iter.peek().unwrap().tile.texture_id;
                let sprites_start = self.subpixel_sprites_start;
                let mut sprites_end = sprites_start + 1;
                self.subpixel_sprites_iter.next();
                while self
                    .subpixel_sprites_iter
                    .next_if(|sprite| {
                        (sprite.order, batch_kind) < max_order_and_kind
                            && sprite.tile.texture_id == texture_id
                    })
                    .is_some()
                {
                    sprites_end += 1;
                }
                self.subpixel_sprites_start = sprites_end;
                Some(PrimitiveBatch::SubpixelSprites {
                    texture_id,
                    range: sprites_start..sprites_end,
                })
            }
            PrimitiveKind::PolychromeSprite => {
                let texture_id = self.polychrome_sprites_iter.peek().unwrap().tile.texture_id;
                let sprites_start = self.polychrome_sprites_start;
                let mut sprites_end = sprites_start + 1;
                self.polychrome_sprites_iter.next();
                while self
                    .polychrome_sprites_iter
                    .next_if(|sprite| {
                        (sprite.order, batch_kind) < max_order_and_kind
                            && sprite.tile.texture_id == texture_id
                    })
                    .is_some()
                {
                    sprites_end += 1;
                }
                self.polychrome_sprites_start = sprites_end;
                Some(PrimitiveBatch::PolychromeSprites {
                    texture_id,
                    range: sprites_start..sprites_end,
                })
            }
            PrimitiveKind::Surface => {
                let surfaces_start = self.surfaces_start;
                let mut surfaces_end = surfaces_start + 1;
                self.surfaces_iter.next();
                while self
                    .surfaces_iter
                    .next_if(|surface| (surface.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    surfaces_end += 1;
                }
                self.surfaces_start = surfaces_end;
                Some(PrimitiveBatch::Surfaces(surfaces_start..surfaces_end))
            }
            PrimitiveKind::BackdropBlur => {
                let blurs_start = self.backdrop_blurs_start;
                let mut blurs_end = blurs_start + 1;
                self.backdrop_blurs_iter.next();
                while self
                    .backdrop_blurs_iter
                    .next_if(|blur| (blur.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    blurs_end += 1;
                }
                self.backdrop_blurs_start = blurs_end;
                Some(PrimitiveBatch::BackdropBlurs(blurs_start..blurs_end))
            }
        }
    }
}

#[derive(Debug)]
#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
#[allow(missing_docs)]
pub enum PrimitiveBatch {
    Shadows(Range<usize>),
    Quads(Range<usize>),
    Paths(Range<usize>),
    Underlines(Range<usize>),
    MonochromeSprites {
        texture_id: AtlasTextureId,
        range: Range<usize>,
    },
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    SubpixelSprites {
        texture_id: AtlasTextureId,
        range: Range<usize>,
    },
    PolychromeSprites {
        texture_id: AtlasTextureId,
        range: Range<usize>,
    },
    Surfaces(Range<usize>),
    BackdropBlurs(Range<usize>),
}

impl PrimitiveBatch {
    #[expect(missing_docs)]
    pub fn label(&self) -> String {
        match self {
            Self::Shadows(range) => format!("shadows ({})", range.len()),
            Self::Quads(range) => format!("quads ({})", range.len()),
            Self::Paths(range) => format!("paths ({})", range.len()),
            Self::Underlines(range) => format!("underlines ({})", range.len()),
            Self::MonochromeSprites { texture_id, range } => {
                format!(
                    "monochrome sprites ({}) on atlas {}",
                    range.len(),
                    texture_id.index
                )
            }
            Self::SubpixelSprites { texture_id, range } => {
                format!(
                    "subpixel sprites ({}) on atlas {}",
                    range.len(),
                    texture_id.index
                )
            }
            Self::PolychromeSprites { texture_id, range } => {
                format!(
                    "polychrome sprites ({}) on atlas {}",
                    range.len(),
                    texture_id.index
                )
            }
            Self::Surfaces(range) => format!("surfaces ({})", range.len()),
            Self::BackdropBlurs(range) => format!("backdrop blurs ({})", range.len()),
        }
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
#[expect(missing_docs)]
pub struct Quad {
    pub order: DrawOrder,
    pub border_style: BorderStyle,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub background: Background,
    pub border_color: Hsla,
    pub corner_radii: Corners<ScaledPixels>,
    pub border_widths: Edges<ScaledPixels>,
    pub transformation: TransformationMatrix,
    /// See [`CornerShape::exponent`].
    pub corner_exponent: f32,
    pub pad: u32, // align to 8 bytes
}

impl Default for Quad {
    fn default() -> Self {
        Self {
            order: DrawOrder::default(),
            border_style: BorderStyle::default(),
            bounds: Bounds::default(),
            content_mask: ContentMask::default(),
            background: Background::default(),
            border_color: Hsla::default(),
            corner_radii: Corners::default(),
            border_widths: Edges::default(),
            transformation: TransformationMatrix::unit(),
            corner_exponent: CornerShape::Round.exponent(),
            pad: 0,
        }
    }
}

impl From<Quad> for Primitive {
    fn from(quad: Quad) -> Self {
        Primitive::Quad(quad)
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
#[expect(missing_docs)]
pub struct Underline {
    pub order: DrawOrder,
    pub pad: u32, // align to 8 bytes
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub thickness: ScaledPixels,
    pub wavy: PaddedBool32,
    pub transformation: TransformationMatrix,
}

impl From<Underline> for Primitive {
    fn from(underline: Underline) -> Self {
        Primitive::Underline(underline)
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
#[expect(missing_docs)]
pub struct Shadow {
    pub order: DrawOrder,
    pub blur_radius: ScaledPixels,
    pub bounds: Bounds<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub element_bounds: Bounds<ScaledPixels>,
    pub element_corner_radii: Corners<ScaledPixels>,
    /// 0 = drop shadow (rendered outside the element), 1 = inset shadow (rendered inside).
    pub inset: u32,
    pub pad: u32, // align to 8 bytes
    pub transformation: TransformationMatrix,
    /// See [`CornerShape::exponent`].
    pub corner_exponent: f32,
    pub pad2: u32, // align to 8 bytes
}

impl From<Shadow> for Primitive {
    fn from(shadow: Shadow) -> Self {
        Primitive::Shadow(shadow)
    }
}

/// The style of a border.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[repr(C)]
pub enum BorderStyle {
    /// A solid border.
    #[default]
    Solid = 0,
    /// A dashed border.
    Dashed = 1,
}

/// The shape of an element's corners within the area its corner radii give them, as with the CSS
/// `corner-shape` property.
///
/// Every shape is a superellipse `|x|^n + |y|^n = 1` with `n = 2^|K|` for the CSS parameter `K`;
/// negative `K` gives the concave mirror image of the convex curve.
#[derive(Default, Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum CornerShape {
    /// A quarter circle or ellipse, `superellipse(1)`.
    #[default]
    Round,
    /// A rounded square, `superellipse(2)`.
    Squircle,
    /// A straight diagonal cut, `superellipse(0)`.
    Bevel,
    /// A concave quarter ellipse, `superellipse(-1)`.
    Scoop,
    /// A superellipse with the CSS parameter `K`, as in `superellipse(K)`.
    Superellipse(f32),
}

impl CornerShape {
    /// Beyond this `|K|` the curve is within a fraction of a pixel of a right angle at UI sizes,
    /// and larger values only risk float overflow in the shaders.
    const MAX_PARAMETER: f32 = 10.;

    /// The CSS `superellipse()` parameter `K` of this shape.
    pub fn parameter(self) -> f32 {
        match self {
            CornerShape::Round => 1.,
            CornerShape::Squircle => 2.,
            CornerShape::Bevel => 0.,
            CornerShape::Scoop => -1.,
            CornerShape::Superellipse(k) => k,
        }
    }

    /// The superellipse exponent `2^|K|` the shaders draw, negated for concave shapes.
    /// Exactly `2.0` for [`CornerShape::Round`], which the shaders draw with circular arcs.
    pub fn exponent(self) -> f32 {
        let parameter = self
            .parameter()
            .clamp(-Self::MAX_PARAMETER, Self::MAX_PARAMETER);
        let exponent = parameter.abs().exp2();
        if parameter < 0. { -exponent } else { exponent }
    }
}

/// A data type representing a 2 dimensional transformation that can be applied to an element.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct TransformationMatrix {
    /// 2x2 matrix containing rotation and scale,
    /// stored row-major
    pub rotation_scale: [[f32; 2]; 2],
    /// translation vector
    pub translation: [f32; 2],
}

impl Eq for TransformationMatrix {}

impl TransformationMatrix {
    /// The unit matrix, has no effect.
    pub fn unit() -> Self {
        Self {
            rotation_scale: [[1.0, 0.0], [0.0, 1.0]],
            translation: [0.0, 0.0],
        }
    }

    /// Move the origin by a given point
    pub fn translate(self, point: Point<ScaledPixels>) -> Self {
        self.compose(Self {
            rotation_scale: [[1.0, 0.0], [0.0, 1.0]],
            translation: [point.x.0, point.y.0],
        })
    }

    /// Clockwise rotation in radians around the origin
    pub fn rotate(self, angle: Radians) -> Self {
        self.compose(Self {
            rotation_scale: [
                [angle.0.cos(), -angle.0.sin()],
                [angle.0.sin(), angle.0.cos()],
            ],
            translation: [0.0, 0.0],
        })
    }

    /// Scale around the origin
    pub fn scale(self, size: Size<f32>) -> Self {
        self.compose(Self {
            rotation_scale: [[size.width, 0.0], [0.0, size.height]],
            translation: [0.0, 0.0],
        })
    }

    /// Perform matrix multiplication with another transformation
    /// to produce a new transformation that is the result of
    /// applying both transformations: first, `other`, then `self`.
    #[inline]
    pub fn compose(self, other: TransformationMatrix) -> TransformationMatrix {
        if other == Self::unit() {
            return self;
        }
        // Perform matrix multiplication
        TransformationMatrix {
            rotation_scale: [
                [
                    self.rotation_scale[0][0] * other.rotation_scale[0][0]
                        + self.rotation_scale[0][1] * other.rotation_scale[1][0],
                    self.rotation_scale[0][0] * other.rotation_scale[0][1]
                        + self.rotation_scale[0][1] * other.rotation_scale[1][1],
                ],
                [
                    self.rotation_scale[1][0] * other.rotation_scale[0][0]
                        + self.rotation_scale[1][1] * other.rotation_scale[1][0],
                    self.rotation_scale[1][0] * other.rotation_scale[0][1]
                        + self.rotation_scale[1][1] * other.rotation_scale[1][1],
                ],
            ],
            translation: [
                self.translation[0]
                    + self.rotation_scale[0][0] * other.translation[0]
                    + self.rotation_scale[0][1] * other.translation[1],
                self.translation[1]
                    + self.rotation_scale[1][0] * other.translation[0]
                    + self.rotation_scale[1][1] * other.translation[1],
            ],
        }
    }

    /// Apply transformation to a point, mainly useful for debugging
    pub fn apply(&self, point: Point<Pixels>) -> Point<Pixels> {
        let input = [point.x.0, point.y.0];
        let mut output = self.translation;
        for (i, output_cell) in output.iter_mut().enumerate() {
            for (k, input_cell) in input.iter().enumerate() {
                *output_cell += self.rotation_scale[i][k] * *input_cell;
            }
        }
        Point::new(output[0].into(), output[1].into())
    }
}

impl Default for TransformationMatrix {
    fn default() -> Self {
        Self::unit()
    }
}

impl TransformationMatrix {
    /// Whether this transformation leaves everything where it is.
    #[inline]
    pub fn is_unit(&self) -> bool {
        *self == Self::unit()
    }

    /// Each axis's scale when this transformation only scales (possibly mirroring) and
    /// translates, keeping rectangles axis-aligned; `None` when it rotates or skews.
    pub fn axis_aligned_scale(&self) -> Option<Size<f32>> {
        let [[a, b], [c, d]] = self.rotation_scale;
        (b == 0. && c == 0.).then(|| size(a.abs(), d.abs()))
    }

    /// The transformation that undoes this one. Its entries are not finite when this
    /// transformation collapses the plane (a zero scale), so nothing maps back into bounds.
    pub(crate) fn inverse(&self) -> Self {
        let [[a, b], [c, d]] = self.rotation_scale;
        let inverse_determinant = 1. / (a * d - b * c);
        let rotation_scale = [
            [d * inverse_determinant, -b * inverse_determinant],
            [-c * inverse_determinant, a * inverse_determinant],
        ];
        let [x, y] = self.translation;
        Self {
            rotation_scale,
            translation: [
                -(rotation_scale[0][0] * x + rotation_scale[0][1] * y),
                -(rotation_scale[1][0] * x + rotation_scale[1][1] * y),
            ],
        }
    }

    /// Apply this transformation to a point in scaled pixels.
    pub fn apply_scaled(&self, point: Point<ScaledPixels>) -> Point<ScaledPixels> {
        let transformed = self.apply(Point::new(point.x.0.into(), point.y.0.into()));
        Point::new(ScaledPixels(transformed.x.0), ScaledPixels(transformed.y.0))
    }

    /// The smallest axis-aligned bounds containing `bounds` once transformed.
    pub fn transform_bounds(&self, bounds: Bounds<ScaledPixels>) -> Bounds<ScaledPixels> {
        if self.is_unit() {
            return bounds;
        }
        let corners = [
            bounds.origin,
            bounds.top_right(),
            bounds.bottom_right(),
            bounds.bottom_left(),
        ]
        .map(|corner| self.apply_scaled(corner));
        let (mut min, mut max) = (corners[0], corners[0]);
        for corner in &corners[1..] {
            min = min.min(corner);
            max = max.max(corner);
        }
        Bounds::from_corners(min, max)
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
#[expect(missing_docs)]
pub struct MonochromeSprite {
    pub order: DrawOrder,
    pub pad: u32,
    pub tile: AtlasTile,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub transformation: TransformationMatrix,
}

impl From<MonochromeSprite> for Primitive {
    fn from(sprite: MonochromeSprite) -> Self {
        Primitive::MonochromeSprite(sprite)
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
#[expect(missing_docs)]
pub struct SubpixelSprite {
    pub order: DrawOrder,
    pub pad: u32, // align to 8 bytes
    pub tile: AtlasTile,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub transformation: TransformationMatrix,
}

impl From<SubpixelSprite> for Primitive {
    fn from(sprite: SubpixelSprite) -> Self {
        Primitive::SubpixelSprite(sprite)
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
#[expect(missing_docs)]
pub struct PolychromeSprite {
    pub order: DrawOrder,
    pub pad: u32,
    pub tile: AtlasTile,
    pub grayscale: PaddedBool32,
    pub opacity: f32,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
    pub transformation: TransformationMatrix,
    /// See [`CornerShape::exponent`].
    pub corner_exponent: f32,
    pub pad2: u32, // align to 8 bytes
}

impl From<PolychromeSprite> for Primitive {
    fn from(sprite: PolychromeSprite) -> Self {
        Primitive::PolychromeSprite(sprite)
    }
}

#[derive(Clone, Debug)]
#[allow(missing_docs)]
pub struct PaintSurface {
    pub order: DrawOrder,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub image_buffer: objc2_core_foundation::CFRetained<objc2_core_video::CVPixelBuffer>,
}

impl From<PaintSurface> for Primitive {
    fn from(surface: PaintSurface) -> Self {
        Primitive::Surface(surface)
    }
}

/// Blurs and saturates what was drawn under `bounds` before it, like CSS `backdrop-filter`, and
/// composites the result back inside the rounded `bounds`.
#[derive(Debug, Copy, Clone)]
#[expect(missing_docs)]
pub struct BackdropBlur {
    pub order: DrawOrder,
    /// Standard deviation of the gaussian, like CSS `blur()`.
    pub blur_radius: ScaledPixels,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
    /// Like CSS `saturate()`: 0 is grayscale and 1 leaves colors unchanged.
    pub saturation: f32,
    pub opacity: f32,
    /// See [`crate::BackdropFilter::fade`].
    pub fade: [ScaledPixels; 4],
    /// See [`crate::BackdropFilter::clear_top`].
    pub clear_top: ScaledPixels,
    /// The element's corner shape, as [`CornerShape::exponent`], so the blur fills a squircle exactly.
    pub corner_exponent: f32,
}

impl From<BackdropBlur> for Primitive {
    fn from(blur: BackdropBlur) -> Self {
        Primitive::BackdropBlur(blur)
    }
}

/// The most downsample passes a backdrop blur takes. Past a blur radius of about 400 device
/// pixels, blurs stop widening.
const MAX_BACKDROP_BLUR_LEVELS: usize = 8;

/// Wider tap offsets reach further per pass but leave gaps that show as banding.
const MAX_BACKDROP_BLUR_TAP_OFFSET: f32 = 2.0;

/// The variance one downsample and its upsample add, in squared texels of the finer level, is
/// `TAP_VARIANCE * offset² + FILTER_VARIANCE`: the taps' spread plus bilinear filtering and
/// decimation. Measured by running the shaders' kernels over an impulse.
const BACKDROP_BLUR_TAP_VARIANCE: f32 = 11. / 6.;
const BACKDROP_BLUR_FILTER_VARIANCE: f32 = 0.85;

impl BackdropBlur {
    /// Plans the dual Kawase passes approximating this blur on a target of `viewport_size`,
    /// or `None` when nothing of it is visible.
    pub fn plan(&self, viewport_size: Size<DevicePixels>) -> Option<BackdropBlurPlan> {
        let visible = self.bounds.intersect(&self.content_mask.bounds);
        let region = Bounds::from_corners(
            point(
                DevicePixels(visible.left().0.floor() as i32),
                DevicePixels(visible.top().0.floor() as i32),
            ),
            point(
                DevicePixels(visible.right().0.ceil() as i32),
                DevicePixels(visible.bottom().0.ceil() as i32),
            ),
        )
        .intersect(&Bounds::new(
            point(DevicePixels(0), DevicePixels(0)),
            viewport_size,
        ));
        if region.is_empty() {
            return None;
        }

        let (level_count, tap_offset) = backdrop_blur_levels(self.blur_radius.0);
        Some(BackdropBlurPlan {
            region,
            level_count,
            tap_offset,
        })
    }
}

/// The fewest levels whose tap offset stays within [`MAX_BACKDROP_BLUR_TAP_OFFSET`]: each further
/// level is coarser, so the result shifts more with where content sits on its grid. Level `k`
/// adds `4^(k - 1)` times the variance of level 1, so `level_count` levels add
/// `(4^level_count - 1) / 3` times it.
fn backdrop_blur_levels(blur_radius: f32) -> (usize, f32) {
    if blur_radius <= 0. {
        return (0, 0.);
    }
    let variance = blur_radius * blur_radius;
    (1..=MAX_BACKDROP_BLUR_LEVELS)
        .find_map(|level_count| {
            let level_variance = 3. * variance / ((1u32 << (2 * level_count)) - 1) as f32;
            let offset_squared =
                (level_variance - BACKDROP_BLUR_FILTER_VARIANCE) / BACKDROP_BLUR_TAP_VARIANCE;
            (offset_squared <= MAX_BACKDROP_BLUR_TAP_OFFSET.powi(2))
                .then(|| (level_count, offset_squared.max(0.).sqrt()))
        })
        .unwrap_or((MAX_BACKDROP_BLUR_LEVELS, MAX_BACKDROP_BLUR_TAP_OFFSET))
}

/// How a renderer draws one [`BackdropBlur`]. It copies `region` out of the target as level 0,
/// downsamples it `level_count` times, upsamples back to level 1, and composites level 1 into
/// the target with a final upsample. With no levels it composites level 0, only saturating.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BackdropBlurPlan {
    /// The device pixels of the target that the blur reads.
    pub region: Bounds<DevicePixels>,
    /// How many times the region is halved.
    pub level_count: usize,
    /// Distance of the diagonal downsample taps, in texels of the level they read. Upsample
    /// taps sit at half this distance.
    pub tap_offset: f32,
}

impl BackdropBlurPlan {
    /// The size of `level`, where level 0 is the region itself.
    pub fn level_size(&self, level: usize) -> Size<DevicePixels> {
        let divisor = 1 << level;
        let halve = |length: DevicePixels| DevicePixels((length.0 + divisor - 1) / divisor);
        size(
            halve(self.region.size.width),
            halve(self.region.size.height),
        )
    }

    /// The level the composite pass samples.
    pub fn composite_source_level(&self) -> usize {
        self.level_count.min(1)
    }

    /// Renders `target_level` from the level next to it: a downsample from the level above, or
    /// an upsample from the level below. Each level sits at the origin of its texture.
    pub fn level_pass(
        &self,
        source_level: usize,
        target_level: usize,
        source_texture_size: Size<DevicePixels>,
        target_texture_size: Size<DevicePixels>,
    ) -> BackdropBlurPass {
        let target_region = self.level_size(target_level);
        let is_downsample = target_level > source_level;
        BackdropBlurPass {
            bounds: Bounds::default(),
            clip_bounds: Bounds::new(
                point(ScaledPixels(0.), ScaledPixels(0.)),
                size(
                    ScaledPixels(target_region.width.0 as f32),
                    ScaledPixels(target_region.height.0 as f32),
                ),
            ),
            corner_radii: Corners::default(),
            target_size: extent(target_texture_size),
            region_origin: [0., 0.],
            source_size: extent(self.level_size(source_level)),
            source_texture_size: extent(source_texture_size),
            coordinate_scale: if is_downsample { 2. } else { 0.5 },
            tap_offset: if is_downsample {
                self.tap_offset
            } else {
                self.tap_offset / 2.
            },
            saturation: 1.,
            opacity: 1.,
            fade: [0.; 4],
            corner_exponent: 2.,
            clear_top: 0.,
            corner_padding: [0.; 2],
        }
    }

    /// Composites the blurred backdrop into the target inside `blur`'s rounded bounds, by
    /// upsampling level 1, or by copying level 0 back when the plan has no levels.
    pub fn composite_pass(
        &self,
        blur: &BackdropBlur,
        source_texture_size: Size<DevicePixels>,
        target_size: Size<DevicePixels>,
    ) -> BackdropBlurPass {
        let source_level = self.composite_source_level();
        let is_upsample = source_level == 1;
        BackdropBlurPass {
            bounds: blur.bounds,
            clip_bounds: blur.bounds.intersect(&blur.content_mask.bounds),
            corner_radii: blur.corner_radii,
            target_size: extent(target_size),
            region_origin: [self.region.origin.x.0 as f32, self.region.origin.y.0 as f32],
            source_size: extent(self.level_size(source_level)),
            source_texture_size: extent(source_texture_size),
            coordinate_scale: if is_upsample { 0.5 } else { 1. },
            tap_offset: if is_upsample {
                self.tap_offset / 2.
            } else {
                0.
            },
            saturation: blur.saturation,
            opacity: blur.opacity,
            fade: blur.fade.map(|length| length.0),
            corner_exponent: blur.corner_exponent,
            clear_top: blur.clear_top.0,
            corner_padding: [0.; 2],
        }
    }
}

/// Rounding level textures up lets an element that grows over a few frames reallocate rarely.
const BACKDROP_BLUR_TEXTURE_GRANULARITY: u32 = 64;

/// Drawn frames without a blur after which a renderer frees its blur level textures. Frames, not
/// time, so it runs the same on every platform; an idle window that draws nothing keeps them until
/// its next few frames.
const BACKDROP_BLUR_RELEASE_AFTER_FRAMES: u32 = 30;

/// Frames with blurs over which the largest size each level needed is measured, before a level
/// allocated well above it is shrunk.
const BACKDROP_BLUR_SHRINK_WINDOW_FRAMES: u32 = 120;

/// Decides how big a renderer's backdrop blur level textures are, and when to shrink or free them,
/// so a full-window blur doesn't hold tens of megabytes for the window's life. Each renderer keeps
/// one with its level textures (`textures[level]`, level 0 first), asks it for each level's size
/// as it draws a blur, then calls [`Self::end_frame`] once the frame is drawn.
#[derive(Debug, Default)]
pub struct BackdropBlurTextureBudget {
    /// Whether a blur asked for textures this frame.
    used_this_frame: bool,
    frames_without_blurs: u32,
    frames_in_window: u32,
    /// Per level, the largest size needed this window.
    window_peak: Vec<Size<DevicePixels>>,
}

impl BackdropBlurTextureBudget {
    /// The size to (re)allocate `level` at so it holds `needed`, or `None` when `current`, its
    /// allocated size, already does. Grows, never shrinks: [`Self::end_frame`] shrinks.
    pub fn allocation_size(
        &mut self,
        level: usize,
        needed: Size<DevicePixels>,
        current: Option<Size<DevicePixels>>,
    ) -> Option<Size<DevicePixels>> {
        self.used_this_frame = true;
        if self.window_peak.len() <= level {
            self.window_peak.resize(level + 1, Size::default());
        }
        let peak = &mut self.window_peak[level];
        *peak = size(peak.width.max(needed.width), peak.height.max(needed.height));

        if current
            .is_some_and(|current| current.width >= needed.width && current.height >= needed.height)
        {
            return None;
        }
        let current = current.unwrap_or_default();
        let needed = backdrop_blur_allocation(needed);
        Some(size(
            needed.width.max(current.width),
            needed.height.max(current.height),
        ))
    }

    /// Call once a frame is drawn, with the sizes of the level textures allocated, level 0 first.
    /// Returns how many of them to keep: the renderer frees the rest, and the next blur that needs
    /// them allocates them again at the size it needs. Everything goes after a stretch of frames
    /// without blurs; a level, and those after it, goes when the blurs of a whole window of
    /// frames needed under a quarter of its area.
    pub fn end_frame(&mut self, allocated: &[Size<DevicePixels>]) -> usize {
        let used = std::mem::take(&mut self.used_this_frame);
        if !used {
            self.frames_without_blurs += 1;
            if self.frames_without_blurs >= BACKDROP_BLUR_RELEASE_AFTER_FRAMES {
                *self = Self::default();
                return 0;
            }
            return allocated.len();
        }

        self.frames_without_blurs = 0;
        self.frames_in_window += 1;
        if self.frames_in_window < BACKDROP_BLUR_SHRINK_WINDOW_FRAMES {
            return allocated.len();
        }
        let area = |size: Size<DevicePixels>| size.width.0 as i64 * size.height.0 as i64;
        let keep = allocated
            .iter()
            .enumerate()
            .position(|(level, &allocated)| {
                let peak = self.window_peak.get(level).copied().unwrap_or_default();
                // A level no blur needed this window has a zero peak.
                area(peak) == 0 || area(allocated) > 4 * area(backdrop_blur_allocation(peak))
            })
            .unwrap_or(allocated.len());
        self.frames_in_window = 0;
        self.window_peak.clear();
        keep
    }
}

/// `needed` rounded up to whole granules, at least one.
fn backdrop_blur_allocation(needed: Size<DevicePixels>) -> Size<DevicePixels> {
    let round_up = |length: DevicePixels| {
        DevicePixels(
            (length.0.max(1) as u32).next_multiple_of(BACKDROP_BLUR_TEXTURE_GRANULARITY) as i32,
        )
    };
    size(round_up(needed.width), round_up(needed.height))
}

fn extent(size: Size<DevicePixels>) -> [f32; 2] {
    [size.width.0 as f32, size.height.0 as f32]
}

/// The shader inputs of one backdrop blur pass, which samples a source texture around each
/// target pixel. A level pass covers its whole target level; the composite covers the visible
/// part of the element.
#[derive(Clone, Debug, PartialEq)]
#[repr(C)]
pub struct BackdropBlurPass {
    /// The element's bounds, for the composite's rounded-corner mask.
    pub bounds: Bounds<ScaledPixels>,
    /// The target pixels drawn.
    pub clip_bounds: Bounds<ScaledPixels>,
    /// The element's corner radii, for the composite's rounded-corner mask.
    pub corner_radii: Corners<ScaledPixels>,
    /// The size of the target texture.
    pub target_size: [f32; 2],
    /// Where the source's texel (0, 0) sits on the target, in target pixels.
    pub region_origin: [f32; 2],
    /// The source texels holding the region; samples mirror at its edges.
    pub source_size: [f32; 2],
    /// The size of the source texture.
    pub source_texture_size: [f32; 2],
    /// Source texels per target pixel.
    pub coordinate_scale: f32,
    /// The distance of the kernel's taps, in source texels.
    pub tap_offset: f32,
    /// See [`BackdropBlur::saturation`].
    pub saturation: f32,
    /// The composite's opacity.
    pub opacity: f32,
    /// How far in from each edge (top, right, bottom, left) the composite fades in, in target
    /// pixels. On a 16-byte boundary for the shaders' vec4.
    pub fade: [f32; 4],
    /// The composite's corner shape, as [`CornerShape::exponent`]; 2 is circular.
    pub corner_exponent: f32,
    /// How far down from the top the content clears to transparent, in target pixels; 0 for none.
    /// Takes a slot that was padding, so the pass keeps its size.
    pub clear_top: f32,
    /// Pads the pass to a multiple of 16 bytes, which uniform and structured buffers need.
    pub corner_padding: [f32; 2],
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[expect(missing_docs)]
pub struct PathId(pub usize);

/// A line made up of a series of vertices and control points.
#[derive(Clone, Debug)]
#[expect(missing_docs)]
pub struct Path<P: Clone + Debug + Default + PartialEq> {
    pub id: PathId,
    pub order: DrawOrder,
    pub bounds: Bounds<P>,
    pub content_mask: ContentMask<P>,
    pub vertices: Vec<PathVertex<P>>,
    pub color: Background,
    start: Point<P>,
    current: Point<P>,
    contour_count: usize,
}

impl Path<Pixels> {
    /// Create a new path with the given starting point.
    pub fn new(start: Point<Pixels>) -> Self {
        Self {
            id: PathId(0),
            order: DrawOrder::default(),
            vertices: Vec::new(),
            start,
            current: start,
            bounds: Bounds {
                origin: start,
                size: Default::default(),
            },
            content_mask: Default::default(),
            color: Default::default(),
            contour_count: 0,
        }
    }

    /// Scale this path by the given factor.
    pub fn scale(&self, factor: f32) -> Path<ScaledPixels> {
        Path {
            id: self.id,
            order: self.order,
            bounds: self.bounds.scale(factor),
            content_mask: self.content_mask.scale(factor),
            vertices: self
                .vertices
                .iter()
                .map(|vertex| vertex.scale(factor))
                .collect(),
            start: self.start.map(|start| start.scale(factor)),
            current: self.current.scale(factor),
            contour_count: self.contour_count,
            color: self.color,
        }
    }

    /// Move the start, current point to the given point.
    pub fn move_to(&mut self, to: Point<Pixels>) {
        self.contour_count += 1;
        self.start = to;
        self.current = to;
    }

    /// Draw a straight line from the current point to the given point.
    pub fn line_to(&mut self, to: Point<Pixels>) {
        self.contour_count += 1;
        if self.contour_count > 1 {
            self.push_triangle(
                (self.start, self.current, to),
                (point(0., 1.), point(0., 1.), point(0., 1.)),
            );
        }
        self.current = to;
    }

    /// Draw a curve from the current point to the given point, using the given control point.
    pub fn curve_to(&mut self, to: Point<Pixels>, ctrl: Point<Pixels>) {
        self.contour_count += 1;
        if self.contour_count > 1 {
            self.push_triangle(
                (self.start, self.current, to),
                (point(0., 1.), point(0., 1.), point(0., 1.)),
            );
        }

        self.push_triangle(
            (self.current, ctrl, to),
            (point(0., 0.), point(0.5, 0.), point(1., 1.)),
        );
        self.current = to;
    }

    /// Push a triangle to the Path.
    pub fn push_triangle(
        &mut self,
        xy: (Point<Pixels>, Point<Pixels>, Point<Pixels>),
        st: (Point<f32>, Point<f32>, Point<f32>),
    ) {
        self.bounds = self
            .bounds
            .union(&Bounds {
                origin: xy.0,
                size: Default::default(),
            })
            .union(&Bounds {
                origin: xy.1,
                size: Default::default(),
            })
            .union(&Bounds {
                origin: xy.2,
                size: Default::default(),
            });

        self.vertices.push(PathVertex {
            xy_position: xy.0,
            st_position: st.0,
            content_mask: Default::default(),
        });
        self.vertices.push(PathVertex {
            xy_position: xy.1,
            st_position: st.1,
            content_mask: Default::default(),
        });
        self.vertices.push(PathVertex {
            xy_position: xy.2,
            st_position: st.2,
            content_mask: Default::default(),
        });
    }
}

impl<T> Path<T>
where
    T: Clone + Debug + Default + PartialEq + PartialOrd + Add<T, Output = T> + Sub<Output = T>,
{
    #[allow(unused)]
    #[expect(missing_docs)]
    pub fn clipped_bounds(&self) -> Bounds<T> {
        self.bounds.intersect(&self.content_mask.bounds)
    }
}

impl From<Path<ScaledPixels>> for Primitive {
    fn from(path: Path<ScaledPixels>) -> Self {
        Primitive::Path(path)
    }
}

#[derive(Clone, Debug)]
#[repr(C)]
#[expect(missing_docs)]
pub struct PathVertex<P: Clone + Debug + Default + PartialEq> {
    pub xy_position: Point<P>,
    pub st_position: Point<f32>,
    pub content_mask: ContentMask<P>,
}

#[expect(missing_docs)]
impl PathVertex<Pixels> {
    pub fn scale(&self, factor: f32) -> PathVertex<ScaledPixels> {
        PathVertex {
            xy_position: self.xy_position.scale(factor),
            st_position: self.st_position,
            content_mask: self.content_mask.scale(factor),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scaled_bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<ScaledPixels> {
        Bounds::new(
            point(ScaledPixels(x), ScaledPixels(y)),
            size(ScaledPixels(width), ScaledPixels(height)),
        )
    }

    fn backdrop_blur(
        blur_radius: f32,
        bounds: Bounds<ScaledPixels>,
        content_mask: Bounds<ScaledPixels>,
    ) -> BackdropBlur {
        BackdropBlur {
            order: 0,
            blur_radius: ScaledPixels(blur_radius),
            bounds,
            content_mask: ContentMask {
                bounds: content_mask,
            },
            corner_radii: Corners::default(),
            saturation: 1.,
            opacity: 1.,
            fade: [ScaledPixels(0.); 4],
            clear_top: ScaledPixels(0.),
            corner_exponent: 2.,
        }
    }

    /// Bilinear sampling with the shaders' mirroring and clamping, along one axis. The kernels'
    /// spread along x is independent of y, so one axis measures the blur's standard deviation.
    fn sample(level: &[f32], position: f32) -> f32 {
        let size = level.len() as f32;
        let mirrored = size - (size - position.abs()).abs();
        let texel = mirrored.clamp(0.5, (size - 0.5).max(0.5)) - 0.5;
        let index = texel.floor() as usize;
        let fraction = texel - index as f32;
        let next = (index + 1).min(level.len() - 1);
        level[index] * (1. - fraction) + level[next] * fraction
    }

    fn downsample(level: &[f32], offset: f32) -> Vec<f32> {
        (0..level.len().div_ceil(2))
            .map(|texel| {
                let center = 2. * (texel as f32 + 0.5);
                (2. * sample(level, center)
                    + sample(level, center - offset)
                    + sample(level, center + offset))
                    / 4.
            })
            .collect()
    }

    fn upsample(level: &[f32], offset: f32, len: usize) -> Vec<f32> {
        (0..len)
            .map(|texel| {
                let center = (texel as f32 + 0.5) / 2.;
                (sample(level, center - 2. * offset)
                    + sample(level, center + 2. * offset)
                    + 2. * sample(level, center)
                    + 4. * sample(level, center - offset)
                    + 4. * sample(level, center + offset))
                    / 12.
            })
            .collect()
    }

    fn blurred_standard_deviation(level_count: usize, tap_offset: f32, impulse: usize) -> f32 {
        let mut region = vec![0.; 4096];
        region[impulse] = 1.;
        let mut levels = vec![region];
        for _ in 0..level_count {
            levels.push(downsample(levels.last().unwrap(), tap_offset));
        }
        let mut blurred = levels.pop().unwrap();
        while let Some(finer) = levels.pop() {
            blurred = upsample(&blurred, tap_offset / 2., finer.len());
        }

        let total: f32 = blurred.iter().sum();
        let position = |texel: usize| texel as f32 + 0.5;
        let mean = blurred
            .iter()
            .enumerate()
            .map(|(i, v)| position(i) * v)
            .sum::<f32>()
            / total;
        let variance = blurred
            .iter()
            .enumerate()
            .map(|(i, v)| (position(i) - mean).powi(2) * v)
            .sum::<f32>()
            / total;
        variance.sqrt()
    }

    #[test]
    fn backdrop_blur_kernels_match_the_requested_radius() {
        for blur_radius in [1.5, 2., 4., 6., 10., 16., 20., 32., 56., 90.] {
            let (level_count, tap_offset) = backdrop_blur_levels(blur_radius);
            assert!(tap_offset <= MAX_BACKDROP_BLUR_TAP_OFFSET);
            // Downsampling makes the result depend on where the content sits within a texel of
            // the coarsest level, so average over every position.
            let phases = 1 << level_count;
            let standard_deviation = (0..phases)
                .map(|phase| blurred_standard_deviation(level_count, tap_offset, 2048 + phase))
                .sum::<f32>()
                / phases as f32;
            assert!(
                (standard_deviation - blur_radius).abs() < 0.05 * blur_radius,
                "blur radius {blur_radius} blurred by {standard_deviation} with {level_count} levels"
            );
        }
    }

    #[test]
    fn backdrop_blur_plan_reads_only_the_visible_pixels() {
        let viewport = size(DevicePixels(800), DevicePixels(600));
        let blur = backdrop_blur(
            10.,
            scaled_bounds(-20.5, 100.25, 300., 200.),
            scaled_bounds(0., 0., 800., 250.),
        );
        let plan = blur.plan(viewport).unwrap();
        assert_eq!(
            plan.region,
            Bounds::new(
                point(DevicePixels(0), DevicePixels(100)),
                size(DevicePixels(280), DevicePixels(150)),
            )
        );
        assert_eq!(
            plan.level_size(1),
            size(DevicePixels(140), DevicePixels(75))
        );
        assert_eq!(plan.level_size(2), size(DevicePixels(70), DevicePixels(38)));

        let clipped_out = backdrop_blur(
            10.,
            scaled_bounds(900., 0., 100., 100.),
            scaled_bounds(0., 0., 2000., 2000.),
        );
        assert_eq!(clipped_out.plan(viewport), None);
    }

    #[test]
    fn backdrop_saturation_alone_takes_no_blur_levels() {
        let blur = backdrop_blur(
            0.,
            scaled_bounds(0., 0., 100., 100.),
            scaled_bounds(0., 0., 100., 100.),
        );
        let plan = blur
            .plan(size(DevicePixels(100), DevicePixels(100)))
            .unwrap();
        assert_eq!(plan.level_count, 0);
        assert_eq!(plan.composite_source_level(), 0);
    }
}
