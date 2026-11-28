//! The SVG writer: a [`Target`] that writes what the walk of a page says as SVG elements.
//!
//! Every path is written in output units (CSS pixels, y down) with the page's transform applied, so an element needs no transform of its
//! own; a gradient, a picture and a text run carry the one matrix they need. A clip is a `clipPath` with a `<g>` around what it clips, a
//! blurred shadow is a `<g>` with a `feGaussianBlur` filter whose region is the shadow's own box, and a picture is a PNG in a data URI.
//! Strokes are uniform in scale (the page transform is a scale and a flip), so a width or a dash in paper inches is the same number times
//! the scale.

use std::fmt::Write as _;
use std::sync::Arc;

use diacraft_color::Rgba;
use diacraft_render::export::{ExportError, MAX_EXPORT_BYTES, TextMode, pixmap_png};
use diacraft_render::{Gradient, GradientKind, Paint, Pixmap, ShapeLabel, Target};
use kurbo::{Affine, BezPath, Cap, Join, PathEl, Rect, Stroke};

use crate::num::{base64, coord, escape, hex, matrix, num, opacity};

/// A group opened and not yet closed.
enum Open {
    Clip,
    /// A shape's group: `written` says whether a `<g>` was opened for it (a shape with nothing to say of itself has none, and is closed as nothing).
    Shape {
        written: bool,
    },
    /// A blurred layer: what was in the body when it opened (the layer's own content is written in a fresh body), its deviation in
    /// output units, and the box of what has been drawn in it.
    Blur {
        outer: String,
        sigma: f64,
        bbox: Option<Rect>,
    },
}

pub(crate) struct SvgTarget {
    to_px: Affine,
    size: (f64, f64),
    /// Output units to a paper inch.
    scale: f64,
    defs: String,
    body: String,
    next_id: u32,
    open: Vec<Open>,
    pub(crate) text: TextMode,
    /// Bytes written so far, the held ones in `Open::Blur` included.
    bytes: usize,
    overflow: bool,
    /// The first picture that could not be encoded: the file would lack it, so the export fails and says so.
    failed: Option<ExportError>,
}

impl SvgTarget {
    pub(crate) fn new(to_px: Affine, size: (f64, f64), text: TextMode) -> SvgTarget {
        let scale = to_px.determinant().abs().sqrt();
        SvgTarget {
            to_px,
            size,
            scale,
            defs: String::new(),
            body: String::new(),
            next_id: 1,
            open: Vec::new(),
            text,
            bytes: 0,
            overflow: false,
            failed: None,
        }
    }

    /// Output units to a paper inch.
    pub(crate) fn scale(&self) -> f64 {
        self.scale
    }

    pub(crate) fn to_px_matrix(&self) -> Affine {
        self.to_px
    }

    /// Appends to the body, unless the file is already past its cap.
    pub(crate) fn put(&mut self, text: &str) {
        self.bytes = self.bytes.saturating_add(text.len());
        if self.bytes > MAX_EXPORT_BYTES {
            self.overflow = true;
            return;
        }
        self.body.push_str(text);
    }

    fn put_def(&mut self, text: &str) {
        self.bytes = self.bytes.saturating_add(text.len());
        if self.bytes > MAX_EXPORT_BYTES {
            self.overflow = true;
            return;
        }
        self.defs.push_str(text);
    }

    fn id(&mut self, prefix: &str) -> String {
        let id = format!("{prefix}{}", self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// The path data of `path` (paper inches) in output units.
    fn path_data(&self, path: &BezPath) -> String {
        let mut d = String::new();
        let at = |d: &mut String, letter: char, points: &[kurbo::Point]| {
            d.push(letter);
            for (i, p) in points.iter().enumerate() {
                let q = self.to_px * *p;
                if i > 0 {
                    d.push(' ');
                }
                let _ = write!(d, "{} {}", coord(q.x), coord(q.y)); // Writing to a String does not fail.
            }
        };
        for el in path.elements() {
            match *el {
                PathEl::MoveTo(p) => at(&mut d, 'M', &[p]),
                PathEl::LineTo(p) => at(&mut d, 'L', &[p]),
                PathEl::QuadTo(a, p) => at(&mut d, 'Q', &[a, p]),
                PathEl::CurveTo(a, b, p) => at(&mut d, 'C', &[a, b, p]),
                PathEl::ClosePath => d.push('Z'),
            }
        }
        d
    }

    /// Grows the box of the blurred layer being drawn, if there is one, by `path` (paper inches) and half a stroke width (output units).
    fn grow_blur_box(&mut self, path: &BezPath, half_width: f64) {
        let to_px = self.to_px;
        if let Some(Open::Blur { bbox, .. }) = self.open.iter_mut().rev().find(|o| matches!(o, Open::Blur { .. })) {
            let b = to_px.transform_rect_bbox(path.control_box()).inflate(half_width, half_width);
            *bbox = Some(bbox.map_or(b, |old| old.union(b)));
        }
    }

    /// The attributes that paint with `paint` as `fill` or `stroke` (`attr`), or `None` for a colour with no ink. A gradient is written to
    /// the definitions; `to_space` takes the gradient's own space to paper inches.
    fn paint_attrs(&mut self, attr: &str, paint: &Paint, to_space: Affine) -> Option<String> {
        match paint {
            Paint::Solid(c) => solid_attrs(attr, *c),
            Paint::Gradient(g) => {
                let id = self.gradient(g, self.to_px * to_space);
                Some(format!("{attr}=\"url(#{id})\""))
            }
        }
    }

    /// Defines a gradient whose space reaches output units through `to_px`, and gives its id.
    fn gradient(&mut self, g: &Gradient, to_px: Affine) -> String {
        let id = self.id("g");
        let c = to_px.as_coeffs();
        let mut def = String::new();
        let transform = matrix(c);
        match g.kind {
            GradientKind::Linear { start, end } => {
                let _ = write!(
                    def,
                    "<linearGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" gradientTransform=\"{transform}\">",
                    num(start.x, 6),
                    num(start.y, 6),
                    num(end.x, 6),
                    num(end.y, 6)
                );
            }
            GradientKind::Radial { center, radius } => {
                let _ = write!(
                    def,
                    "<radialGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" cx=\"{x}\" cy=\"{y}\" fx=\"{x}\" fy=\"{y}\" r=\"{}\" gradientTransform=\"{transform}\">",
                    num(radius, 6),
                    x = num(center.x, 6),
                    y = num(center.y, 6)
                );
            }
        }
        for stop in &g.stops {
            let _ = write!(def, "<stop offset=\"{}\" stop-color=\"{}\"", num(stop.offset.clamp(0.0, 1.0), 4), hex(stop.color));
            if let Some(o) = opacity(stop.color.a) {
                let _ = write!(def, " stop-opacity=\"{o}\"");
            }
            def.push_str("/>");
        }
        def.push_str(match g.kind {
            GradientKind::Linear { .. } => "</linearGradient>\n",
            GradientKind::Radial { .. } => "</radialGradient>\n",
        });
        self.put_def(&def);
        id
    }

    fn fill_with(&mut self, path: &BezPath, paint: &Paint, even_odd: bool, paint_transform: Affine) {
        let Some(attrs) = self.paint_attrs("fill", paint, paint_transform) else {
            return;
        };
        let d = self.path_data(path);
        if d.is_empty() {
            return;
        }
        self.grow_blur_box(path, 0.0);
        let rule = if even_odd { " fill-rule=\"evenodd\"" } else { "" };
        self.put(&format!("<path d=\"{d}\" {attrs}{rule}/>\n"));
    }

    /// Closes the innermost group.
    fn close_top(&mut self) {
        match self.open.pop() {
            Some(Open::Clip) => self.put("</g>\n"),
            Some(Open::Shape { written: true }) => self.put("</g>\n"),
            Some(Open::Shape { written: false }) => {}
            Some(Open::Blur { outer, sigma, bbox }) => {
                let inner = std::mem::replace(&mut self.body, outer);
                // Bytes held for the layer were counted when written; the layer's content is put back in the body whole.
                self.bytes = self.bytes.saturating_sub(inner.len());
                let Some(b) = bbox else {
                    return;
                };
                let id = self.id("b");
                // The filter's region is the shadow's own box and three deviations around it, where the blur has faded.
                let r = b.inflate(3.0 * sigma + 2.0, 3.0 * sigma + 2.0);
                self.put_def(&format!(
                    "<filter id=\"{id}\" filterUnits=\"userSpaceOnUse\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" color-interpolation-filters=\"sRGB\"><feGaussianBlur stdDeviation=\"{}\"/></filter>\n",
                    coord(r.x0),
                    coord(r.y0),
                    coord(r.width()),
                    coord(r.height()),
                    num(sigma, 4)
                ));
                self.put(&format!("<g filter=\"url(#{id})\">\n"));
                self.put(&inner);
                self.put("</g>\n");
            }
            None => {}
        }
    }

    /// The file, or why there is none.
    pub(crate) fn finish(mut self, title: Option<&str>, description: Option<&str>) -> Result<String, ExportError> {
        self.rewind(0);
        if let Some(e) = self.failed.take() {
            return Err(e);
        }
        if self.overflow {
            return Err(ExportError::TooLarge(format!("the file would pass {MAX_EXPORT_BYTES} bytes")));
        }
        let (w, h) = self.size;
        let mut out = String::with_capacity(self.defs.len() + self.body.len() + 512);
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        let _ = writeln!(
            out,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" version=\"1.1\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">",
            w = coord(w),
            h = coord(h)
        );
        // A title or a description with nothing but white space is never written (SVG 2, 5.7).
        if let Some(title) = title.filter(|t| !is_blank(t)) {
            let _ = writeln!(out, "<title>{}</title>", escape(title));
        }
        if let Some(description) = description.filter(|d| !is_blank(d)) {
            let _ = writeln!(out, "<desc>{}</desc>", escape(description));
        }
        if !self.defs.is_empty() {
            out.push_str("<defs>\n");
            out.push_str(&self.defs);
            out.push_str("</defs>\n");
        }
        out.push_str(&self.body);
        out.push_str("</svg>\n");
        Ok(out)
    }
}

/// Whether a text holds nothing but white space (Unicode's `White_Space`).
fn is_blank(text: &str) -> bool {
    text.chars().all(char::is_whitespace)
}

/// Owner, 2026-11-26: a shape's group in the file carries the SVG standard's own title and description. The group's `<title>` is the alt text's title and its `<desc>` the
/// description, written as they are; **a shape with no alt text at all gets its name as the `<title>`** (so that every shape has an accessible name, and a file read by a program that knows no
/// alt text lists the shapes by their names). This departs, which writes no group and no name for a shape with no alt text: to flip it, set this to `false`.
const NAME_FALLBACK: bool = true;

/// The most characters of a shape's name written as its title.
const MAX_NAME_CHARS: usize = 1_024;

fn solid_attrs(attr: &str, c: Rgba) -> Option<String> {
    if c.a == 0 {
        return None;
    }
    let mut s = format!("{attr}=\"{}\"", hex(c));
    if let Some(o) = opacity(c.a) {
        let _ = write!(s, " {attr}-opacity=\"{o}\"");
    }
    Some(s)
}

impl Target for SvgTarget {
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

    fn stroke(&mut self, path: &BezPath, stroke: &Stroke, paint: &Paint) {
        let Some(attrs) = self.paint_attrs("stroke", paint, Affine::IDENTITY) else {
            return;
        };
        let d = self.path_data(path);
        if d.is_empty() {
            return;
        }
        let k = self.scale;
        self.grow_blur_box(path, stroke.width * k / 2.0 * stroke.miter_limit.clamp(1.0, 10.0));
        let cap = match stroke.start_cap {
            Cap::Butt => "butt",
            Cap::Square => "square",
            Cap::Round => "round",
        };
        let join = match stroke.join {
            Join::Bevel => "bevel",
            Join::Miter => "miter",
            Join::Round => "round",
        };
        let mut s = format!(
            "<path d=\"{d}\" fill=\"none\" {attrs} stroke-width=\"{}\" stroke-linecap=\"{cap}\" stroke-linejoin=\"{join}\"",
            num(stroke.width * k, 4)
        );
        if stroke.join == Join::Miter {
            let _ = write!(s, " stroke-miterlimit=\"{}\"", num(stroke.miter_limit.max(1.0), 3));
        }
        if !stroke.dash_pattern.is_empty() {
            let dashes: Vec<String> = stroke.dash_pattern.iter().map(|d| num(d * k, 4)).collect();
            let _ = write!(s, " stroke-dasharray=\"{}\"", dashes.join(" "));
            if stroke.dash_offset != 0.0 {
                let _ = write!(s, " stroke-dashoffset=\"{}\"", num(stroke.dash_offset * k, 4));
            }
        }
        s.push_str("/>\n");
        self.put(&s);
    }

    fn blurred<F: FnOnce(&mut Self)>(&mut self, sigma: f64, draw: F) {
        let depth = self.open.len();
        let outer = std::mem::take(&mut self.body);
        self.open.push(Open::Blur { outer, sigma: sigma * self.scale, bbox: None });
        draw(self);
        self.rewind(depth);
    }

    fn push_clip(&mut self, path: &BezPath) {
        let id = self.id("c");
        let d = self.path_data(path);
        // Clipped by the odd-crossings rule, as the fills the clips are made of are drawn.
        self.put_def(&format!("<clipPath id=\"{id}\"><path d=\"{d}\" clip-rule=\"evenodd\"/></clipPath>\n"));
        self.put(&format!("<g clip-path=\"url(#{id})\">\n"));
        self.open.push(Open::Clip);
    }

    fn pop_clip(&mut self) {
        self.close_top();
    }

    fn image(&mut self, image: &Arc<Pixmap>, place: Affine) {
        let png = match pixmap_png(image) {
            Ok(png) => png,
            Err(e) => {
                self.failed.get_or_insert(e);
                return;
            }
        };
        let m = self.to_px * place;
        if !m.as_coeffs().iter().all(|c| c.is_finite()) {
            return;
        }
        self.put(&format!(
            "<image width=\"{}\" height=\"{}\" transform=\"{}\" preserveAspectRatio=\"none\" xlink:href=\"data:image/png;base64,{}\"/>\n",
            image.width(),
            image.height(),
            matrix(m.as_coeffs()),
            base64(&png)
        ));
    }

    fn mark(&self) -> usize {
        self.open.len()
    }

    fn rewind(&mut self, mark: usize) {
        while self.open.len() > mark {
            self.close_top();
        }
    }

    fn wants_labels(&self) -> bool {
        true
    }

    fn begin_shape(&mut self, label: &ShapeLabel<'_>) {
        let title = (!is_blank(label.title)).then_some(label.title);
        let description = (!is_blank(label.description)).then_some(label.description);
        let name = || {
            let end = label.name.char_indices().nth(MAX_NAME_CHARS).map_or(label.name.len(), |(at, _)| at);
            label.name.get(..end).filter(|n| !is_blank(n))
        };
        let title = match (title, description) {
            (None, None) if NAME_FALLBACK => name(),
            (title, _) => title,
        };
        if title.is_none() && description.is_none() {
            self.open.push(Open::Shape { written: false });
            return;
        }
        let mut head = String::from("<g>\n");
        if let Some(title) = title {
            let _ = writeln!(head, "<title>{}</title>", escape(title));
        }
        if let Some(description) = description {
            let _ = writeln!(head, "<desc>{}</desc>", escape(description));
        }
        self.put(&head);
        self.open.push(Open::Shape { written: true });
    }

    fn end_shape(&mut self) {
        // The innermost shape's group: a clip or a layer a drawing cut short left above it is closed first.
        let Some(at) = self.open.iter().rposition(|o| matches!(o, Open::Shape { .. })) else {
            return;
        };
        self.rewind(at + 1);
        self.close_top();
    }

    fn wants_text(&self) -> bool {
        self.text == TextMode::AsText
    }

    fn text_run(&mut self, run: &diacraft_render::TextRun<'_>) -> bool {
        crate::text::write_run(self, run)
    }
}
