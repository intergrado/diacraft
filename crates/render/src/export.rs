//! Exporting a page: the parts every output shares, and the PNG.
//!
//! An export draws the same page the window does, through the same walk (`frame.rs`), onto another [`Target`]: this crate gives the raster
//! one and the PNG made of it; `diacraft-svg` and `diacraft-pdf` give the vector ones. Everything they have in common is here: what a page
//! maps to ([`PageLayout`]), what an export may spend ([`export_limits`]), what it gives back ([`Exported`]) and why it fails
//! ([`ExportError`]).
//!
//! **What an export leaves out** is what the window does: no grid, ruler, guide or selection. **What it draws by a stand-in**
//! is reported as the window reports it, in [`Exported::skipped`], so a caller that wants to warn its user has the same list.
//!
//! **Cost.** An export is an act the user asked for, so it may take longer than a frame of a window ([`export_limits`]: a minute for the
//! page, half of it for text); the counts still hold, and a page that reaches one is exported as far as it got, with the rest in `skipped`.

use std::fmt;
use std::time::Duration;

use diacraft_color::Rgba;
use diacraft_doc::Document;
use kurbo::Affine;
use vello_cpu::Pixmap;

use crate::limits::MAX_VIEW_PIXELS;
use crate::skips::{SkipKind, Skipped};
use crate::{Limits, PageId, Renderer, Target, View, page_paper_size};

/// The largest page an export writes, in output units (pixels, points) along a side. A page of 1,000,000 points is 13,888 inches: far past
/// any drawing, and short of the sizes where a coordinate loses its fractions in a 32-bit float.
pub const MAX_EXPORT_UNITS: f64 = 1.0e6;

/// The most bytes an export holds in memory before it gives up. A vector file of a page that fills this is not one anybody opens.
pub const MAX_EXPORT_BYTES: usize = 512 * 1024 * 1024;

/// Why a page was not exported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportError {
    /// The document has no page with this `ID`.
    NoSuchPage(PageId),
    /// The page has no size (`PageWidth` or `PageHeight` is missing, zero, negative or not a number), so there is nothing to export.
    NoPageSize,
    /// The page, at the size asked for, is past what an export writes. Says what and how big.
    TooLarge(String),
    /// A resolution that is not a positive number.
    BadResolution,
    /// The picture or the file could not be encoded. The words come from the encoder and hold no path.
    Encode(String),
    /// Drawing met a bug and recovered, so the output would be a page with parts missing and no way to say which: nothing is written.
    Bug(String),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::NoSuchPage(id) => write!(f, "the document has no page {id}"),
            ExportError::NoPageSize => write!(f, "the page has no size, so there is nothing to export"),
            ExportError::TooLarge(what) => write!(f, "too large to export: {what}"),
            ExportError::BadResolution => write!(f, "the resolution must be a positive number"),
            ExportError::Encode(why) => write!(f, "cannot write the file: {why}"),
            ExportError::Bug(why) => write!(f, "drawing the page failed: {why} (please report this bug)"),
        }
    }
}

impl std::error::Error for ExportError {}

/// How an export writes letters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextMode {
    /// As text where the target can (a face it may embed, letters that are one glyph each), else as outlines, which the walk counts as
    /// [`SkipKind::TextAsOutlines`].
    #[default]
    AsText,
    /// Always as outlines: the file looks the same wherever it is opened, and its letters cannot be selected or searched.
    AsOutlines,
}

/// A finished export.
#[derive(Clone, Debug, PartialEq)]
pub struct Exported {
    /// The file.
    pub bytes: Vec<u8>,
    /// The size in the format's own units: pixels for a PNG, CSS pixels for an SVG, points for a PDF. Rounded up.
    pub width: u32,
    pub height: u32,
    /// What was left out or drawn by a stand-in, as [`Rendered::skipped`](crate::Rendered::skipped) says it.
    pub skipped: Vec<Skipped>,
}

impl Exported {
    /// How many were left out or drawn by a stand-in, of one kind.
    pub fn count(&self, kind: SkipKind) -> usize {
        self.skipped.iter().filter(|s| s.kind == kind).map(|s| s.count).sum()
    }
}

/// What an export may spend: a minute for the page and half of it for laying text out, and the counts of a frame.
pub fn export_limits() -> Limits {
    Limits {
        frame_time: Some(Duration::from_secs(60)),
        text_time: Some(Duration::from_secs(30)),
        label_time: Some(Duration::from_secs(30)),
        ..Limits::default()
    }
}

/// Where a page goes on a target: paper inches (y up) to output units (y down), and the size in them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageLayout {
    pub to_px: Affine,
    pub width: f64,
    pub height: f64,
}

impl PageLayout {
    /// The layout of `page` at `units_per_inch` output units to a paper inch (96 for CSS pixels, 72 for points), its top-left corner at the
    /// origin. The size is the page's own, not rounded (a vector file may be any size).
    pub fn of(doc: &Document, page: PageId, units_per_inch: f64) -> Result<PageLayout, ExportError> {
        if doc.page(page).is_none() {
            return Err(ExportError::NoSuchPage(page));
        }
        if !(units_per_inch.is_finite() && units_per_inch > 0.0) {
            return Err(ExportError::BadResolution);
        }
        let (w, h) = page_paper_size(doc, page).ok_or(ExportError::NoPageSize)?;
        let (width, height) = (w * units_per_inch, h * units_per_inch);
        if !(width.is_finite() && height.is_finite() && width <= MAX_EXPORT_UNITS && height <= MAX_EXPORT_UNITS) {
            return Err(ExportError::TooLarge(format!("a page of {w:.1} by {h:.1} inches at {units_per_inch} to the inch")));
        }
        let to_px = Affine::new([units_per_inch, 0.0, 0.0, -units_per_inch, 0.0, height]);
        Ok(PageLayout { to_px, width, height })
    }

    /// The size in whole output units, rounded up, and at least 1.
    pub fn whole(&self) -> (u32, u32) {
        (self.width.ceil().max(1.0) as u32, self.height.ceil().max(1.0) as u32)
    }
}

/// Draws `page` onto `target` with the limits of an export and on `paper` (a colour, or none for a transparent page), and puts the
/// renderer's own limits and paper back. The list is what was left out or drawn by a stand-in; a bug in drawing is an error here, since a
/// vector file cannot say which part is missing.
pub fn draw_page<T: Target>(
    renderer: &mut Renderer,
    doc: &Document,
    page: PageId,
    paper: Option<Rgba>,
    target: &mut T,
) -> Result<Vec<Skipped>, ExportError> {
    let (limits, kept_paper) = (renderer.limits(), renderer.paper());
    renderer.set_limits(export_limits());
    renderer.set_paper(paper);
    let skipped = renderer.draw_onto(doc, page, target);
    renderer.set_limits(limits);
    renderer.set_paper(kept_paper);
    if let Some(bug) = skipped.iter().find(|s| s.kind == SkipKind::Recovered) {
        return Err(ExportError::Bug(bug.first.clone()));
    }
    if skipped.iter().any(|s| s.kind == SkipKind::NoSuchPage) {
        return Err(ExportError::NoSuchPage(page));
    }
    Ok(skipped)
}

/// Premultiplied RGBA8 to straight (the PNG's): each channel divided by the alpha, rounded.
pub fn unpremultiply(premul: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(premul.len());
    for px in premul.as_chunks::<4>().0 {
        let [r, g, b, a] = *px;
        if a == 0 || a == 255 {
            out.extend_from_slice(&[r, g, b, a]);
        } else {
            let un = |c: u8| ((u32::from(c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8;
            out.extend_from_slice(&[un(r), un(g), un(b), a]);
        }
    }
    out
}

/// A PNG of straight RGBA8 pixels (`width * height * 4` bytes), at `px_per_inch` if given (written as the file's physical size).
pub fn encode_png(width: u32, height: u32, straight_rgba: &[u8], px_per_inch: Option<f64>) -> Result<Vec<u8>, ExportError> {
    let wanted = u64::from(width).saturating_mul(u64::from(height)).saturating_mul(4);
    if width == 0 || height == 0 || wanted != straight_rgba.len() as u64 {
        return Err(ExportError::Encode("the pixels do not fill the picture".to_owned()));
    }
    let fail = |e: png::EncodingError| ExportError::Encode(e.to_string());
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    if let Some(ppi) = px_per_inch.filter(|p| p.is_finite() && *p > 0.0 && *p < 1.0e6) {
        // The file's physical size, in pixels to the metre: 39.3701 inches make one.
        let per_metre = (ppi * 39.3701).round() as u32;
        encoder.set_pixel_dims(Some(png::PixelDimensions { xppu: per_metre, yppu: per_metre, unit: png::Unit::Meter }));
    }
    let mut writer = encoder.write_header().map_err(fail)?;
    writer.write_image_data(straight_rgba).map_err(fail)?;
    writer.finish().map_err(fail)?;
    Ok(bytes)
}

/// A PNG of a decoded picture, for a vector file that carries one.
pub fn pixmap_png(image: &Pixmap) -> Result<Vec<u8>, ExportError> {
    encode_png(u32::from(image.width()), u32::from(image.height()), &unpremultiply(image.data_as_u8_slice()), None)
}

/// A PNG of `page` at `px_per_inch` pixels to the inch, on white paper ([`Renderer::set_paper`] changes the paper; none gives a transparent
/// page). The size is the page's, rounded up.
pub fn export_png(renderer: &mut Renderer, doc: &Document, page: PageId, px_per_inch: f64) -> Result<Exported, ExportError> {
    if doc.page(page).is_none() {
        return Err(ExportError::NoSuchPage(page));
    }
    if !(px_per_inch.is_finite() && px_per_inch > 0.0) {
        return Err(ExportError::BadResolution);
    }
    let (w, h) = page_paper_size(doc, page).ok_or(ExportError::NoPageSize)?;
    let view = View::fit_page(doc, page, px_per_inch).ok_or_else(|| {
        ExportError::TooLarge(format!(
            "a page of {w:.1} by {h:.1} inches at {px_per_inch} pixels to the inch is past {} pixels a side",
            crate::limits::MAX_VIEW_SIDE
        ))
    })?;
    if u64::from(view.width) * u64::from(view.height) > MAX_VIEW_PIXELS {
        return Err(ExportError::TooLarge(format!("{} by {} pixels is past {MAX_VIEW_PIXELS} pixels", view.width, view.height)));
    }
    let before = renderer.limits();
    renderer.set_limits(export_limits());
    let rendered = renderer.render(doc, page, &view);
    renderer.set_limits(before);
    if let Some(bug) = rendered.skipped.iter().find(|s| s.kind == SkipKind::Recovered) {
        return Err(ExportError::Bug(bug.first.clone()));
    }
    if rendered.count(SkipKind::ViewRefused) > 0 {
        return Err(ExportError::TooLarge(format!("{} by {} pixels", view.width, view.height)));
    }
    let bytes = encode_png(rendered.width, rendered.height, &unpremultiply(&rendered.rgba_premul), Some(px_per_inch))?;
    Ok(Exported { bytes, width: rendered.width, height: rendered.height, skipped: rendered.skipped })
}
