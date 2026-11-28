//! The seam between the walk of a page and what it draws on.
//!
//! The walk (`frame.rs`) reads a page's shapes and says what to draw in a few verbs: fill a path, stroke a path, clip, draw a picture, draw
//! text. A [`Target`] is what those verbs are said to. The raster target (`canvas.rs`, on `vello_cpu`) is one, and an export is another: the
//! SVG and PDF writers implement the same trait, so a page is read, placed, dashed, shaded and laid out once, in one place, and every output
//! sees the same page.
//!
//! **Units.** Every path the walk hands over is in **paper inches, y up**. [`Target::to_px`] takes those to the target's own units (pixels
//! for the raster, CSS pixels for SVG, points for PDF; y down) and [`Target::size`] is the target's size in them. Stroke widths, dashes and
//! gradient geometry are in paper inches too.
//!
//! **What a target must keep true.** `fill` and `stroke` may be called with any path the walk has made after [`Target::check`] or
//! [`Target::bring_in`] passed it (finite, within [`COORD_LIMIT_PX`] of the origin after `to_px`); a target never draws outside its size
//! without being clipped to it. `push_clip` and `pop_clip` nest; [`Target::mark`] and [`Target::rewind`] close the clips and layers a
//! drawing that was cut short (a panic the walk caught) left open.
//!
//! **Text.** A target that can write letters as text says so ([`Target::wants_text`]) and is offered each run ([`Target::text_run`]); it
//! answers `false` for a run it cannot write that way (the walk then draws the glyph outlines), so text is never dropped.

use std::cmp::Ordering;
use std::sync::Arc;

use diacraft_color::Rgba;
use diacraft_fonts::Face;
use diacraft_text::PlacedGlyph;
use kurbo::{Affine, BezPath, Point, Rect, Stroke};
use vello_cpu::Pixmap;

use crate::limits::{CLIP_MARGIN_PX, COORD_LIMIT_PX};

/// The side of a tile of the rasteriser, in pixels (`vello_common::tile::Tile::WIDTH`): a filter layer's pixmap is rounded out to whole tiles.
const TILE_PX: f64 = 4.0;

/// One colour of a gradient, `offset` in 0 to 1 along it. Colours blend as the straight bytes and alpha, not premultiplied, and
/// hold beyond the ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStop {
    pub offset: f64,
    pub color: Rgba,
}

/// Where a gradient runs, in its own space (the paint transform of [`Target::fill_with_paint_transform`] takes that to paper inches).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GradientKind {
    /// Along the line from `start` to `end`.
    Linear { start: Point, end: Point },
    /// Out from `center` to `radius`.
    Radial { center: Point, radius: f64 },
}

/// A gradient: its geometry and its colours.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub kind: GradientKind,
    pub stops: Vec<GradientStop>,
}

/// What a fill or a stroke is painted with.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    /// A colour; `Rgba` here is straight (not premultiplied) alpha.
    Solid(Rgba),
    /// A gradient whose geometry is in paper inches (or in its own space, when a paint transform comes with it).
    Gradient(Gradient),
}

/// Why a path was not handed over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// A coordinate is not a number, or lies past [`COORD_LIMIT_PX`] in pixels.
    OutOfRange,
}

/// The source text of a block and where each of its characters starts, for a target that writes letters as text.
#[derive(Clone, Copy, Debug)]
pub struct TextSource<'a> {
    /// The characters of the block's runs, in order, line feeds included: what [`PlacedGlyph::chars`] counts.
    pub text: &'a str,
    /// The byte offset in `text` of each character, and at the end `text.len()`: `chars + 1` numbers.
    pub byte_at: &'a [usize],
}

impl<'a> TextSource<'a> {
    /// The text of the characters `range` counts, or `None` when the range is not inside the block.
    pub fn slice(&self, range: &std::ops::Range<usize>) -> Option<&'a str> {
        let bytes = self.bytes(range)?;
        self.text.get(bytes)
    }

    /// The bytes of `text` the characters `range` count.
    pub fn bytes(&self, range: &std::ops::Range<usize>) -> Option<std::ops::Range<usize>> {
        if range.start > range.end {
            return None;
        }
        Some(*self.byte_at.get(range.start)?..*self.byte_at.get(range.end)?)
    }
}

/// Letters in one face, size and colour, to write as text if the target can.
#[derive(Clone, Copy, Debug)]
pub struct TextRun<'a> {
    pub face: &'a Face,
    /// The em in the block's frame (paper inches).
    pub size: f64,
    /// `FontScale`: the glyphs are this much wider than the face makes them, at the same height. Never 0 or negative.
    pub x_scale: f64,
    /// Red, green and blue, and the alpha with the run's opacity folded in.
    pub color: Rgba,
    /// In visual order, left to right, in the block's frame: x right, y up (`diacraft_text`'s `laid.rs`).
    pub glyphs: &'a [PlacedGlyph],
    pub source: TextSource<'a>,
    /// The block's frame to paper inches.
    pub to_paper: Affine,
}

/// What a shape says of itself to a reader who cannot see it: its alt text, as `diacraft_doc::alt_text::read` finds it (its own, or its master shape's), and its name.
/// The texts are as the document holds them and may be empty or white space, and a target that writes them leaves out what holds nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapeLabel<'a> {
    /// The alt text's title.
    pub title: &'a str,
    /// The alt text's description.
    pub description: &'a str,
    /// The shape's `Name`, else its `NameU`, else empty.
    pub name: &'a str,
}

/// What a page is drawn on. See the module documentation.
pub trait Target {
    /// Paper inches (y up) to the target's units (y down).
    fn to_px(&self) -> Affine;

    /// The target's width and height in its own units.
    fn size(&self) -> (f64, f64);

    /// Fills `path` (paper inches), by the odd-crossings rule when `even_odd`.
    fn fill(&mut self, path: &BezPath, paint: &Paint, even_odd: bool);

    /// Fills `path` with `paint`, whose own geometry is moved by `paint_transform` before the view's.
    fn fill_with_paint_transform(&mut self, path: &BezPath, paint: &Paint, even_odd: bool, paint_transform: Affine);

    /// Strokes `path` (paper inches) with `stroke`, whose width and dashes are paper inches.
    fn stroke(&mut self, path: &BezPath, stroke: &Stroke, paint: &Paint);

    /// The narrowest stroke this target draws, in its own units; 0 for none. A target that rasterises says its pixel
    /// (1): a stroke narrower than that comes out faint, not thin, so the walk hands it one that width, at the line's full colour. A writer
    /// of SVG or PDF says 0: the reader rasterises, and the file keeps the line's true width.
    fn min_stroke(&self) -> f64 {
        0.0
    }

    /// Draws what `draw` draws through a Gaussian blur of `sigma` paper inches (a standard deviation). Only called when
    /// [`Target::can_blur`].
    fn blurred<F: FnOnce(&mut Self)>(&mut self, sigma: f64, draw: F)
    where
        Self: Sized;

    /// Can this target blur? One that cannot gets a hard-edged shadow, and the walk says so.
    fn can_blur(&self) -> bool {
        true
    }

    /// Clips what is drawn until [`Target::pop_clip`] to `path` (paper inches).
    fn push_clip(&mut self, path: &BezPath);

    fn pop_clip(&mut self);

    /// Draws `image` (premultiplied RGBA, y down, its first row at the top) through `place`, which takes its pixel space to paper inches.
    /// The image is stretched, not tiled, and what is drawn is cut by the clip in force.
    fn image(&mut self, image: &Arc<Pixmap>, place: Affine);

    /// A mark to [`Target::rewind`] to: how many clips and layers are in force now.
    fn mark(&self) -> usize;

    /// Pops the clips and layers pushed since `mark`, and puts back whatever state a drawing cut short could have left changed.
    fn rewind(&mut self, mark: usize);

    /// How many steps the target holds for later, for one that writes them down (`record.rs`) and is held to a count; 0 for the others.
    #[doc(hidden)]
    fn recorded(&self) -> usize {
        0
    }

    /// Does this target write what each shape says of itself ([`ShapeLabel`])? If not, the walk neither reads it nor calls [`Target::begin_shape`] and [`Target::end_shape`]: a raster, a recording
    /// and a PDF page have no use for it.
    fn wants_labels(&self) -> bool {
        false
    }

    /// A shape that is drawn begins: what is drawn until the matching [`Target::end_shape`], its own geometry and its members', belongs to it. Calls nest, as a group's members are inside the group, and
    /// a drawing cut short is closed by [`Target::rewind`] with the clips and layers. Only called when [`Target::wants_labels`].
    fn begin_shape(&mut self, _label: &ShapeLabel<'_>) {}

    /// The shape the last [`Target::begin_shape`] opened ends.
    fn end_shape(&mut self) {}

    /// Does this target write letters as text? If not, the walk does not build the source text.
    fn wants_text(&self) -> bool {
        false
    }

    /// Writes `run` as text and says `true`, or says `false` and writes nothing, when it cannot (a face that cannot be embedded, letters
    /// that do not map to glyphs one by one, ...): the walk then draws the glyph outlines.
    fn text_run(&mut self, _run: &TextRun<'_>) -> bool {
        false
    }

    /// The target as a rectangle in its own units.
    fn pixel_rect(&self) -> Rect {
        let (w, h) = self.size();
        Rect::new(0.0, 0.0, w, h)
    }

    /// The most pixels the filter layer of a blur can hold: `paper` (a box in paper inches) in the target's units, cut to the target, with
    /// `spread_px` and the rasteriser's rounding to whole tiles added on every side. The raster layer's pixmap is that size and is kept until
    /// the frame ends; an SVG filter costs its viewer much the same, and a target that cannot blur ([`Target::can_blur`]) is never asked to.
    /// The walk counts it against `MAX_BLUR_PIXELS_PER_FRAME` for every target that blurs.
    fn blur_layer_pixels(&self, paper: Rect, spread_px: f64) -> u64 {
        let px = self.to_px().transform_rect_bbox(paper).intersect(self.pixel_rect());
        let (w, h) = (px.width(), px.height());
        if !(w.is_finite() && h.is_finite()) || w <= 0.0 || h <= 0.0 {
            return 0;
        }
        let pad = spread_px.max(0.0) + 2.0 * TILE_PX;
        let pixels = (w + 2.0 * pad) * (h + 2.0 * pad);
        if pixels.is_finite() { pixels as u64 } else { u64::MAX }
    }

    /// The part of paper the target shows, in paper inches, or `None` when the transform cannot be inverted.
    fn paper_view(&self) -> Option<Rect> {
        let to_px = self.to_px();
        if to_px.determinant().abs().partial_cmp(&1.0e-18) != Some(Ordering::Greater) {
            return None;
        }
        let back = to_px.inverse();
        let r = back.transform_rect_bbox(self.pixel_rect());
        (r.x0.is_finite() && r.y0.is_finite() && r.x1.is_finite() && r.y1.is_finite()).then_some(r)
    }

    /// Whether the bounding box of `path` (in paper inches) is a finite one inside the coordinates a target takes, and whether any of it
    /// can reach the target, with `margin_px` to spare.
    fn check(&self, path: &BezPath, margin_px: f64) -> Result<bool, Refused> {
        if path.is_empty() {
            return Ok(false);
        }
        let paper = path.control_box();
        let px = self.to_px().transform_rect_bbox(paper);
        if !(px.x0.is_finite() && px.y0.is_finite() && px.x1.is_finite() && px.y1.is_finite()) {
            return Err(Refused::OutOfRange);
        }
        if px.x0.abs() > COORD_LIMIT_PX || px.x1.abs() > COORD_LIMIT_PX || px.y0.abs() > COORD_LIMIT_PX || px.y1.abs() > COORD_LIMIT_PX {
            return Err(Refused::OutOfRange);
        }
        let view = self.pixel_rect().inflate(margin_px, margin_px);
        Ok(px.x1 >= view.x0 && px.x0 <= view.x1 && px.y1 >= view.y0 && px.y0 <= view.y1)
    }

    /// As [`Target::check`], but hands the path back: a path of straight segments that reaches past [`COORD_LIMIT_PX`] is cut to a
    /// rectangle around the view (see `clip.rs`) instead of refused. The flag says it was cut. `Ok(None)` is a path that is nowhere near the
    /// view.
    fn bring_in(&self, path: BezPath, stroke: bool, margin_px: f64) -> Result<Option<(BezPath, bool)>, Refused> {
        match self.check(&path, margin_px) {
            Ok(true) => return Ok(Some((path, false))),
            Ok(false) => return Ok(None),
            Err(Refused::OutOfRange) => {}
        }
        // Only a path whose every number is finite can be cut; one that is not a number is refused.
        if !path.is_finite() {
            return Err(Refused::OutOfRange);
        }
        let ppi = self.to_px().determinant().abs().sqrt().max(1.0e-12);
        let view = self.paper_view().ok_or(Refused::OutOfRange)?;
        let reach = (margin_px.max(0.0) + CLIP_MARGIN_PX) / ppi;
        let rect = view.inflate(reach, reach);
        let cut = if stroke { crate::clip::polyline(&path, rect) } else { crate::clip::polygon(&path, rect) };
        match cut {
            Some(p) if p.is_empty() => Ok(None),
            Some(p) => Ok(Some((p, true))),
            None => Err(Refused::OutOfRange),
        }
    }
}
