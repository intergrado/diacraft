//! The PDF writer: a [`Target`] that writes what the walk of a page says as PDF drawing operators, through Krilla (`craft-pdf`'s writer).
//!
//! Every path is written in points with the page's transform applied (y down: Krilla puts the origin at the page's top-left corner and
//! flips for the file), so a path needs no matrix of its own; a gradient, a picture and a text run carry the one matrix they need. A clip is
//! a clipping path around what it clips. **Blurs are not written** (PDF has no blur): the walk draws a blurred shadow hard-edged and says so
//! ([`Target::can_blur`] is false).

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use craft_pdf::write::backend as krilla;
use diacraft_color::Rgba;
use diacraft_render::export::{ExportError, TextMode, pixmap_png};
use diacraft_render::{Face, Gradient, GradientKind, Paint, Pixmap, Target, TextRun};
use krilla::color::rgb;
use krilla::geom::{Path, PathBuilder, Size, Transform};
use krilla::image::Image;
use krilla::num::NormalizedF32;
use krilla::paint::{Fill, FillRule, LineCap, LineJoin, LinearGradient, RadialGradient, SpreadMethod, Stop, Stroke, StrokeDash};
use krilla::surface::Surface;
use krilla::text::Font;
use kurbo::{Affine, BezPath, Cap, Join, PathEl, Rect};

/// The largest magnitude a coordinate or a matrix entry may have in a content stream. A PDF/A file may hold only integers up to
/// 2,147,483,647, and the writer panics on a whole number at the end of the range; art past this is left out.
const MAX_COORD: f64 = 1.0e9;

/// What a path element is counted as costing in a page's content stream, in bytes: a curve writes six numbers and an operator, a line two.
const BYTES_PER_ELEMENT: u64 = 32;

/// What painting costs besides its path: the colour, the line's width, cap and join, the operator.
const BYTES_PER_PAINT: u64 = 96;

/// What a glyph costs, in bytes. (Public to `text.rs`.)
pub(crate) const BYTES_PER_GLYPH: u64 = 48;

/// A straight-sided path of more elements than this is cut to the sheet's printable part before it is written (see
/// [`PdfTarget::with_print_limits`]): a polyline of thousands of points across thirty tiles would otherwise be written whole on each.
const CUT_ABOVE_ELEMENTS: usize = 64;

/// A clip pushed and not yet popped.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Clip {
    /// On the surface: a `pop` takes it off.
    Pushed,
    /// A clip that leaves nothing (an empty path): nothing is drawn until it is popped, and nothing was pushed.
    Nothing,
}

pub(crate) struct PdfTarget<'a> {
    surface: Surface<'a>,
    to_px: Affine,
    size: (f64, f64),
    /// Points to a paper inch.
    scale: f64,
    clips: Vec<Clip>,
    /// How many of `clips` are [`Clip::Nothing`]: while there is one, nothing is drawn.
    nothing: usize,
    text: TextMode,
    /// The fonts written so far, by face id: `None` for a face that may not be embedded or could not be read for it.
    fonts: HashMap<u64, Option<Font>>,
    /// The first thing that could not be written: the file would lack it, so the export fails and says so.
    pub(crate) failed: Option<ExportError>,
    /// A print's sheet: what ink may land on, in paper inches, and what the page's content may cost in all (`None`: an export, which is
    /// bounded by its one page).
    print: Option<PrintLimits>,
    /// What the content written so far is counted as costing, in bytes.
    written: u64,
}

/// What a sheet of a print is held to.
#[derive(Clone, Copy)]
struct PrintLimits {
    /// The sheet's printable part, in paper inches: what is written is cut to it.
    clip: Rect,
    /// The bytes the sheet may cost: what the print may still spend.
    budget: u64,
    /// The budget is spent: nothing more is written, and the sheet fails.
    spent: bool,
}

fn xf(a: Affine) -> Transform {
    let c = a.as_coeffs();
    Transform::from_row(c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32, c[4] as f32, c[5] as f32)
}

fn unit(v: f32) -> NormalizedF32 {
    NormalizedF32::new(if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 }).unwrap_or(NormalizedF32::ONE)
}

fn rgb_of(c: Rgba) -> krilla::color::Color {
    rgb::Color::new(c.r, c.g, c.b).into()
}

fn opacity_of(c: Rgba) -> NormalizedF32 {
    unit(f32::from(c.a) / 255.0)
}

fn in_range(values: &[f64]) -> bool {
    values.iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD)
}

impl<'a> PdfTarget<'a> {
    pub(crate) fn new(surface: Surface<'a>, to_px: Affine, size: (f64, f64), text: TextMode) -> PdfTarget<'a> {
        let scale = to_px.determinant().abs().sqrt();
        PdfTarget { surface, to_px, size, scale, clips: Vec::new(), nothing: 0, text, fonts: HashMap::new(), failed: None, print: None, written: 0 }
    }

    /// Makes this the target of a print's sheet. **What lies outside `clip` (paper inches) is not written**: a path whose box (and stroke) misses
    /// it is left out, a straight-sided path of many elements is cut to it, and a picture that misses it is left out. And **what is written is
    /// counted** (a path costs by its elements, a picture by its bytes, a glyph by a constant): once it passes `budget` bytes nothing more is
    /// written and [`PdfTarget::finish`] fails, so a drawing of many sheets cannot fill the memory before the print is refused.
    pub(crate) fn with_print_limits(mut self, clip: Rect, budget: u64) -> PdfTarget<'a> {
        self.print = Some(PrintLimits { clip, budget, spent: false });
        self
    }

    /// What has been written, counted as [`PdfTarget::with_print_limits`] says.
    pub(crate) fn written(&self) -> u64 {
        self.written
    }

    /// Counts `bytes` more written. `false`: the budget is spent (now or before), and nothing is to be written.
    pub(crate) fn charge(&mut self, bytes: u64) -> bool {
        self.written = self.written.saturating_add(bytes);
        let Some(limits) = &mut self.print else { return true };
        if self.written > limits.budget {
            limits.spent = true;
        }
        if limits.spent {
            self.failed.get_or_insert(ExportError::TooLarge("the drawing on the sheet passed the bytes a print may hold".to_owned()));
        }
        !limits.spent
    }

    /// The path as it is to be written on a print's sheet: `None` when none of it can show (it, with the stroke's reach, misses the sheet's
    /// printable part), cut to the part when it is straight-sided and long, else as it is. An export writes every path as it is.
    fn fitted<'p>(&self, path: &'p BezPath, stroke: Option<&kurbo::Stroke>) -> Option<Cow<'p, BezPath>> {
        let Some(limits) = &self.print else { return Some(Cow::Borrowed(path)) };
        // A stroke reaches past its path by half its width, and a mitre by up to its limit times that: ten times the width is more than any line's.
        let reach = stroke.map_or(0.0, |s| s.width * s.miter_limit.clamp(1.0, 10.0) + 0.05);
        let area = limits.clip.inflate(reach, reach);
        let bounds = path.control_box();
        if !(bounds.x0.is_finite() && bounds.y0.is_finite() && bounds.x1.is_finite() && bounds.y1.is_finite()) {
            return Some(Cow::Borrowed(path));
        }
        if bounds.x1 < area.x0 || bounds.x0 > area.x1 || bounds.y1 < area.y0 || bounds.y0 > area.y1 {
            return None;
        }
        let dashed = stroke.is_some_and(|s| !s.dash_pattern.is_empty());
        if path.elements().len() > CUT_ABOVE_ELEMENTS && !dashed {
            // A path with a curve in it, which the cut does not take, is written as it is, and counted.
            if let Some(cut) = diacraft_render::print::cut_to(path, area, stroke.is_some()) {
                return if cut.is_empty() { None } else { Some(Cow::Owned(cut)) };
            }
        }
        Some(Cow::Borrowed(path))
    }

    pub(crate) fn scale(&self) -> f64 {
        self.scale
    }

    pub(crate) fn to_px_matrix(&self) -> Affine {
        self.to_px
    }

    pub(crate) fn surface(&mut self) -> &mut Surface<'a> {
        &mut self.surface
    }

    /// Whether anything can be seen: not inside a clip that leaves nothing, and not after a print's budget is spent.
    pub(crate) fn visible(&self) -> bool {
        self.nothing == 0 && !self.print.is_some_and(|p| p.spent)
    }

    /// The font to write a face in: made once, and `None` for a face that cannot be embedded (its licence says no, or the writer could not
    /// read it). The walk then draws that face's letters as outlines.
    pub(crate) fn font_of(&mut self, face: &Face) -> Option<Font> {
        if let Some(known) = self.fonts.get(&face.id()) {
            return known.clone();
        }
        let font = if face.may_embed() { craft_pdf::write::font(face.data(), face.index(), None).ok() } else { None };
        self.fonts.insert(face.id(), font.clone());
        font
    }

    /// `path` (paper inches) as a Krilla path in points; `None` when it is empty or reaches past [`MAX_COORD`].
    fn path_of(&self, path: &BezPath) -> Option<Path> {
        let mut b = PathBuilder::new();
        let at = |p: kurbo::Point| {
            let q = self.to_px * p;
            (in_range(&[q.x, q.y])).then_some((q.x as f32, q.y as f32))
        };
        for el in path.elements() {
            match *el {
                PathEl::MoveTo(p) => {
                    let (x, y) = at(p)?;
                    b.move_to(x, y);
                }
                PathEl::LineTo(p) => {
                    let (x, y) = at(p)?;
                    b.line_to(x, y);
                }
                PathEl::QuadTo(a, p) => {
                    let ((ax, ay), (x, y)) = (at(a)?, at(p)?);
                    b.quad_to(ax, ay, x, y);
                }
                PathEl::CurveTo(a, c, p) => {
                    let ((ax, ay), (cx, cy), (x, y)) = (at(a)?, at(c)?, at(p)?);
                    b.cubic_to(ax, ay, cx, cy, x, y);
                }
                PathEl::ClosePath => b.close(),
            }
        }
        b.finish()
    }

    /// A paint for Krilla and the opacity that goes with it, or `None` for a colour with no ink. `to_space` takes a gradient's own space
    /// to paper inches.
    fn paint_of(&self, paint: &Paint, to_space: Affine) -> Option<(krilla::paint::Paint, NormalizedF32)> {
        match paint {
            Paint::Solid(c) => (c.a != 0).then(|| (rgb_of(*c).into(), opacity_of(*c))),
            Paint::Gradient(g) => {
                let m = self.to_px * to_space;
                if !in_range(&m.as_coeffs()) {
                    return None;
                }
                Some((gradient_of(g, m)?, NormalizedF32::ONE))
            }
        }
    }

    fn fill_with(&mut self, path: &BezPath, paint: &Paint, even_odd: bool, paint_transform: Affine) {
        if !self.visible() {
            return;
        }
        let Some(path) = self.fitted(path, None) else { return };
        if !self.charge(BYTES_PER_PAINT + BYTES_PER_ELEMENT * path.elements().len() as u64) {
            return;
        }
        let (Some((paint, opacity)), Some(path)) = (self.paint_of(paint, paint_transform), self.path_of(&path)) else {
            return;
        };
        let rule = if even_odd { FillRule::EvenOdd } else { FillRule::NonZero };
        self.surface.set_stroke(None);
        self.surface.set_fill(Some(Fill { paint, opacity, rule }));
        self.surface.draw_path(&path);
    }

    /// Closes what the walk left open and finishes the page's surface; the first failure, if there was one.
    pub(crate) fn finish(mut self) -> Result<(), ExportError> {
        self.rewind(0);
        self.surface.finish();
        match self.failed {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

/// A gradient as Krilla draws it, through `to_px` (its space to points).
fn gradient_of(g: &Gradient, to_px: Affine) -> Option<krilla::paint::Paint> {
    let mut stops = Vec::with_capacity(g.stops.len());
    let mut last = 0.0f32;
    for s in &g.stops {
        // Offsets rise: a stop is never before the one that came first.
        last = (s.offset as f32).clamp(0.0, 1.0).max(last);
        stops.push(Stop { offset: unit(last), color: rgb_of(s.color), opacity: opacity_of(s.color) });
    }
    if stops.len() < 2 {
        return None;
    }
    let transform = xf(to_px);
    Some(match g.kind {
        GradientKind::Linear { start, end } => {
            let ok = in_range(&[start.x, start.y, end.x, end.y]);
            ok.then(|| {
                LinearGradient {
                    x1: start.x as f32,
                    y1: start.y as f32,
                    x2: end.x as f32,
                    y2: end.y as f32,
                    transform,
                    spread_method: SpreadMethod::Pad,
                    stops,
                    anti_alias: true,
                }
                .into()
            })?
        }
        GradientKind::Radial { center, radius } => {
            let ok = in_range(&[center.x, center.y, radius]) && radius > 0.0;
            ok.then(|| {
                RadialGradient {
                    fx: center.x as f32,
                    fy: center.y as f32,
                    fr: 0.0,
                    cx: center.x as f32,
                    cy: center.y as f32,
                    cr: radius as f32,
                    transform,
                    spread_method: SpreadMethod::Pad,
                    stops,
                    anti_alias: true,
                }
                .into()
            })?
        }
    })
}

impl Target for PdfTarget<'_> {
    fn to_px(&self) -> Affine {
        self.to_px
    }

    fn size(&self) -> (f64, f64) {
        self.size
    }

    fn fill(&mut self, path: &BezPath, paint: &Paint, even_odd: bool) {
        self.fill_with(path, paint, even_odd, Affine::IDENTITY);
    }

    fn fill_with_paint_transform(&mut self, path: &BezPath, paint: &Paint, even_odd: bool, paint_transform: Affine) {
        self.fill_with(path, paint, even_odd, paint_transform);
    }

    fn stroke(&mut self, path: &BezPath, stroke: &kurbo::Stroke, paint: &Paint) {
        if !self.visible() {
            return;
        }
        let Some(path) = self.fitted(path, Some(stroke)) else { return };
        if !self.charge(BYTES_PER_PAINT + BYTES_PER_ELEMENT * path.elements().len() as u64) {
            return;
        }
        let (Some((paint, opacity)), Some(path)) = (self.paint_of(paint, Affine::IDENTITY), self.path_of(&path)) else {
            return;
        };
        let k = self.scale;
        let width = stroke.width * k;
        if !(width.is_finite() && width > 0.0 && width <= MAX_COORD) {
            return;
        }
        let dash = dash_of(stroke, k);
        let stroke = Stroke {
            paint,
            width: width as f32,
            miter_limit: stroke.miter_limit.clamp(1.0, 1.0e4) as f32,
            line_cap: match stroke.start_cap {
                Cap::Butt => LineCap::Butt,
                Cap::Square => LineCap::Square,
                Cap::Round => LineCap::Round,
            },
            line_join: match stroke.join {
                Join::Bevel => LineJoin::Bevel,
                Join::Miter => LineJoin::Miter,
                Join::Round => LineJoin::Round,
            },
            opacity,
            dash,
        };
        self.surface.set_fill(None);
        self.surface.set_stroke(Some(stroke));
        self.surface.draw_path(&path);
    }

    fn blurred<F: FnOnce(&mut Self)>(&mut self, _sigma: f64, draw: F) {
        // Not called: the walk asks `can_blur` first. If it is, the shadow is drawn sharp.
        draw(self);
    }

    fn can_blur(&self) -> bool {
        false
    }

    fn push_clip(&mut self, path: &BezPath) {
        // A print's sheet writes nothing once its budget is spent, a clip no more than a fill: the clip is pushed as one that leaves nothing, so
        // that its pop balances it and nothing reaches the surface for it. (Inside a clip that leaves nothing nothing can show, and the clip is
        // not written either.)
        if self.print.is_some() && !(self.visible() && self.charge(BYTES_PER_PAINT + BYTES_PER_ELEMENT * path.elements().len() as u64)) {
            self.nothing += 1;
            self.clips.push(Clip::Nothing);
            return;
        }
        match self.path_of(path) {
            Some(p) => {
                // Clipped by the odd-crossings rule, as the fills the clips are made of are drawn.
                self.surface.push_clip_path(&p, &FillRule::EvenOdd);
                self.clips.push(Clip::Pushed);
            }
            None => {
                self.nothing += 1;
                self.clips.push(Clip::Nothing);
            }
        }
    }

    fn pop_clip(&mut self) {
        match self.clips.pop() {
            Some(Clip::Pushed) => self.surface.pop(),
            Some(Clip::Nothing) => self.nothing = self.nothing.saturating_sub(1),
            None => {}
        }
    }

    fn image(&mut self, image: &Arc<Pixmap>, place: Affine) {
        if !self.visible() {
            return;
        }
        let png = match pixmap_png(image) {
            Ok(png) => png,
            Err(e) => {
                self.failed.get_or_insert(e);
                return;
            }
        };
        // A picture's bytes are written into the file whole each time it is drawn, so they are counted each time.
        if !self.charge(BYTES_PER_PAINT + png.len() as u64) {
            return;
        }
        let m = self.to_px * place;
        let (Some(size), true) = (Size::from_wh(f32::from(image.width()), f32::from(image.height())), in_range(&m.as_coeffs())) else {
            return;
        };
        match Image::from_png(png.into(), true) {
            Ok(img) => {
                self.surface.push_transform(&xf(m));
                self.surface.draw_image(img, size);
                self.surface.pop();
            }
            Err(why) => {
                self.failed.get_or_insert(ExportError::Encode(format!("a picture could not be written: {why}")));
            }
        }
    }

    fn mark(&self) -> usize {
        self.clips.len()
    }

    fn rewind(&mut self, mark: usize) {
        while self.clips.len() > mark {
            self.pop_clip();
        }
    }

    fn wants_text(&self) -> bool {
        self.text == TextMode::AsText
    }

    fn text_run(&mut self, run: &TextRun<'_>) -> bool {
        crate::text::write_run(self, run)
    }
}

/// The dash of a stroke in points, if it has a usable one: the entries are finite and not negative and they add up to something. An odd
/// number of entries is repeated, as SVG reads it (so every line has the same dash whatever reads it).
fn dash_of(stroke: &kurbo::Stroke, k: f64) -> Option<StrokeDash> {
    let mut array: Vec<f32> = stroke.dash_pattern.iter().map(|d| (d * k) as f32).collect();
    if array.is_empty() || array.len() > 64 {
        return None;
    }
    if array.len() % 2 == 1 {
        array.extend_from_within(..);
    }
    let sum: f32 = array.iter().sum();
    let usable = array.iter().all(|d| d.is_finite() && *d >= 0.0) && sum.is_finite() && sum > 0.0;
    let offset = (stroke.dash_offset * k) as f32;
    usable.then_some(StrokeDash { array, offset: if offset.is_finite() { offset } else { 0.0 } })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use craft_pdf::write::{Metadata, Writer};
    use diacraft_color::Rgba;
    use kurbo::Shape as _;

    use super::*;

    /// Writes `n` clipped fills on a page of a print whose budget is `budget` bytes, as plain operators, and gives the file and how the target
    /// ended.
    fn clipped_fills(n: usize, budget: u64) -> (Vec<u8>, Result<(), ExportError>) {
        let settings = krilla::SerializeSettings { compress_content_streams: false, ..Default::default() };
        let mut writer = Writer::new(settings, Metadata { creator: "test".to_owned(), producer: "test".to_owned(), ..Metadata::default() });
        let mut page = writer.document_mut().start_page_with(krilla::page::PageSettings::new(Size::from_wh(612.0, 792.0).unwrap()));
        let ended = {
            let mut target = PdfTarget::new(page.surface(), Affine::scale(72.0), (612.0, 792.0), TextMode::AsOutlines)
                .with_print_limits(Rect::new(0.0, 0.0, 8.5, 11.0), budget);
            for i in 0..n {
                // Each shape is clipped to its own box, as a picture is to its frame.
                let x = (i % 80) as f64 * 0.1;
                let area = Rect::new(x, 1.0, x + 0.09, 2.0).to_path(0.1);
                target.push_clip(&area);
                target.fill(&area, &Paint::Solid(Rgba::rgb(255, 0, 0)), false);
                target.pop_clip();
            }
            assert!(target.clips.is_empty(), "every clip pushed was popped");
            target.finish()
        };
        page.finish();
        (writer.finish().expect("the file is written"), ended)
    }

    /// How many times the operator `op` stands in the page's content: the graphics state operators `q` and `Q`, which a clip is bracketed by.
    fn lines(file: &[u8], op: &str) -> usize {
        let start = file.windows(6).position(|w| w == b"stream").expect("a content stream");
        let end = file.windows(9).rposition(|w| w == b"endstream").expect("its end");
        file[start..end].split(|b| b.is_ascii_whitespace()).filter(|token| *token == op.as_bytes()).count()
    }

    #[test]
    fn once_the_budget_is_spent_nothing_more_reaches_the_page_not_even_a_clip_and_the_states_balance() {
        // A clip and a fill cost 256 bytes each of the count, so 20,000 is spent after about 40 shapes; 100 and 5,000 shapes then write the same bytes.
        let (few, few_ended) = clipped_fills(100, 20_000);
        let (many, many_ended) = clipped_fills(5_000, 20_000);
        assert!(matches!(few_ended, Err(ExportError::TooLarge(_))) && matches!(many_ended, Err(ExportError::TooLarge(_))), "the budget was passed");
        assert_eq!(
            many.len(),
            few.len(),
            "5,000 shapes wrote {} bytes against {} for 100: what came after the budget was spent reached the page",
            many.len(),
            few.len()
        );
        // The graphics states balance, in the file that stopped and the one that did not need to: every `q` has its `Q`.
        for file in [&few, &many] {
            let (open, close) = (lines(file, "q"), lines(file, "Q"));
            assert!(open > 0, "some clips were written before the budget ran out");
            assert_eq!(open, close, "q and Q: {open} and {close}");
        }
        // And with room for all of them nothing is stopped, and every one is written.
        let (all, ended) = clipped_fills(100, 100_000_000);
        assert!(ended.is_ok());
        assert!(all.len() > 2 * few.len(), "the 100 shapes whole are {} bytes against {} for the part the small budget took", all.len(), few.len());
        assert_eq!(lines(&all, "q"), lines(&all, "Q"));
        assert!(lines(&all, "q") >= 100, "a clip for each shape: {}", lines(&all, "q"));
    }
}
