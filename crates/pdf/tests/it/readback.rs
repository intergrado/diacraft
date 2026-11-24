//! Reading a PDF back with the shared reader and interpreter, which know nothing of the writer: what was painted, in device points (y down,
//! the origin at the page's top-left corner), with what colour, and what text each glyph stands for.

use std::sync::Arc;

use craft_pdf::interpret::font::Glyph;
use craft_pdf::interpret::{
    BlendMode, ClipPath, Context, Device, GlyphDrawMode, Image, InterpreterCache, InterpreterSettings, Paint, PathDrawMode, SoftMask, TransformExt,
};
use kurbo::{Affine, BezPath, Point, Rect, Shape as _};

/// One path the page painted.
#[derive(Clone, Debug)]
pub struct PaintedPath {
    /// Its box in device points.
    pub bbox: Rect,
    /// Its colour as straight RGBA bytes when it was painted with a colour; `None` when with a pattern (a gradient).
    pub rgba: Option<[u8; 4]>,
    pub stroked: bool,
    /// For a stroke, the width in points and the dash array.
    pub width: f32,
    pub dashes: Vec<f32>,
    pub even_odd: bool,
}

/// One glyph the page painted.
#[derive(Clone, Debug)]
pub struct PaintedGlyph {
    /// The characters the file says it stands for.
    pub text: String,
    pub gid: u32,
    /// Where its origin is, in device points.
    pub origin: Point,
    /// How many device points a unit of its outline (font units) is, along x and along y.
    pub scale: (f64, f64),
    pub rgba: [u8; 4],
    /// Whether the font is embedded (its program is in the file).
    pub embedded: bool,
    /// Whether the outline has any elements (a space has none).
    pub drawn: bool,
}

#[derive(Default, Debug)]
pub struct Seen {
    pub paths: Vec<PaintedPath>,
    pub glyphs: Vec<PaintedGlyph>,
    pub images: Vec<Affine>,
    /// The boxes of the clips pushed, in device points, in order.
    pub clips: Vec<Rect>,
    pub open_clips: i32,
    pub pages: usize,
    pub size: (f64, f64),
}

impl Seen {
    /// The paths painted with this colour.
    pub fn with_colour(&self, rgb: [u8; 3]) -> Vec<&PaintedPath> {
        self.paths.iter().filter(|p| p.rgba.is_some_and(|c| c[..3] == rgb && c[3] > 0)).collect()
    }

    /// The characters of every glyph, in order.
    pub fn text(&self) -> String {
        self.glyphs.iter().map(|g| g.text.as_str()).collect()
    }
}

impl Device<'_> for Seen {
    fn set_soft_mask(&mut self, _: Option<SoftMask<'_>>) {}
    fn set_blend_mode(&mut self, _: BlendMode) {}
    fn draw_path(&mut self, path: &BezPath, transform: Affine, paint: &Paint<'_>, mode: &PathDrawMode) {
        let rgba = match paint {
            Paint::Color(c) => Some(c.to_rgba().to_rgba8()),
            Paint::Pattern(_) => None,
        };
        let bbox = (transform * path.clone()).bounding_box();
        match mode {
            PathDrawMode::Fill(rule) => self.paths.push(PaintedPath {
                bbox,
                rgba,
                stroked: false,
                width: 0.0,
                dashes: Vec::new(),
                even_odd: format!("{rule:?}").contains("EvenOdd"),
            }),
            PathDrawMode::Stroke(props) => self.paths.push(PaintedPath {
                bbox,
                rgba,
                stroked: true,
                width: props.line_width,
                dashes: props.dash_array.to_vec(),
                even_odd: false,
            }),
        }
    }
    fn push_clip_path(&mut self, clip: &ClipPath) {
        self.open_clips += 1;
        self.clips.push(clip.path.bounding_box());
    }
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'_>>, _: BlendMode) {}
    fn draw_glyph(&mut self, glyph: &Glyph<'_>, transform: Affine, glyph_transform: Affine, paint: &Paint<'_>, mode: &GlyphDrawMode) {
        if !matches!(mode, GlyphDrawMode::Fill) {
            return;
        }
        let Glyph::Outline(outline) = glyph else {
            return;
        };
        let rgba = match paint {
            Paint::Color(c) => c.to_rgba().to_rgba8(),
            Paint::Pattern(_) => [0, 0, 0, 0],
        };
        let text = match glyph.as_unicode() {
            Some(craft_pdf::interpret::hayro_cmap::BfString::Char(c)) => c.to_string(),
            Some(craft_pdf::interpret::hayro_cmap::BfString::String(s)) => s,
            None => String::new(),
        };
        let m = transform * glyph_transform;
        let c = m.as_coeffs();
        self.glyphs.push(PaintedGlyph {
            text,
            gid: outline.glyph_id().to_u32(),
            origin: m * Point::new(0.0, 0.0),
            scale: (c[0].hypot(c[1]), c[2].hypot(c[3])),
            rgba,
            embedded: outline.font_data().is_some(),
            drawn: !outline.outline().elements().is_empty(),
        });
    }
    fn draw_image(&mut self, _: Image<'_, '_>, transform: Affine) {
        self.images.push(transform);
    }
    fn pop_clip_path(&mut self) {
        self.open_clips -= 1;
    }
    fn pop_transparency_group(&mut self) {}
}

/// Reads page 1 of `bytes`.
pub fn read(bytes: &[u8]) -> Seen {
    read_page(bytes, 0)
}

/// Reads page `n` (from 0) of `bytes`.
pub fn read_page(bytes: &[u8], n: usize) -> Seen {
    let pdf = craft_pdf::open(Arc::new(bytes.to_vec()), None).expect("the PDF opens");
    let pages = pdf.pages();
    let page = &pages[n];
    let (w, h) = page.render_dimensions();
    let cache = InterpreterCache::new();
    let mut context = Context::new(
        page.initial_transform(true).to_kurbo(),
        Rect::new(0.0, 0.0, f64::from(w), f64::from(h)),
        &cache,
        page.xref(),
        InterpreterSettings::default(),
    );
    let mut seen = Seen { pages: pages.len(), size: (f64::from(w), f64::from(h)), ..Seen::default() };
    craft_pdf::interpret::interpret_page(page, &mut context, &mut seen);
    seen
}
