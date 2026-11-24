//! Letters as PDF text, in an embedded subset of the face they were laid out in.
//!
//! **Where the font allows.** A run is written as text when its face may be embedded (its `OS/2` `fsType` does not forbid it: `Face::may_embed`)
//! and the writer can read it, and when every letter has a glyph in it (no glyph 0, the missing-glyph box). The glyphs are placed one by one
//! at the places the layout gave them, so ligatures, joined and right-to-left scripts and marks come out as laid out, and each carries the
//! characters it stands for, so the text can be selected, searched and copied. Two kinds of run are written as outlines instead (the walk
//! does that when this answers `false`, and counts them as `TextAsOutlines`): a run in a face that may not or cannot be embedded, and one
//! whose glyphs do not stand on one baseline (raised marks); and a run with a made-up bold or slant, which the walk does not offer.
//!
//! **Where it sits.** As in `diacraft-svg`: the text's own frame is the block's turned to run along x with y down and its unit the point, so
//! the font size and the places of the glyphs are written in points, and `FontScale` is a horizontal stretch in the frame's matrix.

use craft_pdf::write::backend as krilla;
use diacraft_render::TextRun;
use krilla::color::rgb;
use krilla::geom::Point;
use krilla::paint::{Fill, FillRule};
use krilla::text::{GlyphId, KrillaGlyph};
use kurbo::Affine;

use crate::target::PdfTarget;

/// The most glyphs one run may hold to be written as text; the walk budgets glyphs far lower, so this is a second wall.
const MAX_RUN_GLYPHS: usize = 100_000;

/// The largest font size in points a run is written at (Krilla's sizes are 32-bit, and a page is 14,400 points in most readers).
const MAX_FONT_SIZE: f64 = 100_000.0;

/// Writes `run` as text; `false` and nothing written when it cannot be.
pub(crate) fn write_run(target: &mut PdfTarget<'_>, run: &TextRun<'_>) -> bool {
    if run.glyphs.is_empty() || run.glyphs.len() > MAX_RUN_GLYPHS {
        return false;
    }
    if !target.visible() || run.color.a == 0 {
        // Nothing can be seen, so nothing is written, and nothing is left to draw.
        return true;
    }
    if !target.charge(crate::target::BYTES_PER_GLYPH * run.glyphs.len() as u64) {
        return true;
    }
    let k = target.scale();
    let font_size = run.size * k;
    if !(k.is_finite()
        && k > 0.0
        && run.x_scale.is_finite()
        && run.x_scale > 0.0
        && font_size.is_finite()
        && font_size > 0.0
        && font_size <= MAX_FONT_SIZE)
    {
        return false;
    }
    let first_y = run.glyphs.first().map_or(0.0, |g| g.y);
    let mut xs = Vec::with_capacity(run.glyphs.len());
    for g in run.glyphs.iter() {
        let on_baseline = (g.y - first_y).abs() < 1.0e-9;
        if g.gid == 0 || !on_baseline || !g.x.is_finite() || !g.advance.is_finite() || run.source.bytes(&g.chars).is_none() {
            return false;
        }
        xs.push(g.x * k / run.x_scale);
    }
    // The element's frame: x right in points over the stretch, y down in points; the block's frame is x right and y up in inches.
    let to_local = Affine::new([run.x_scale / k, 0.0, 0.0, -1.0 / k, 0.0, 0.0]);
    let m = target.to_px_matrix() * run.to_paper * to_local;
    if !m.as_coeffs().iter().all(|c| c.is_finite() && c.abs() < 1.0e9) || !xs.iter().all(|x| x.abs() < 1.0e9) {
        return false;
    }
    let Some(font) = target.font_of(run.face) else {
        return false;
    };
    // Each glyph's advance is the distance to the next one's origin, in ems, so the writer's pen stands exactly where the layout put each.
    let mut glyphs = Vec::with_capacity(run.glyphs.len());
    for (i, g) in run.glyphs.iter().enumerate() {
        let next = xs.get(i + 1).copied().unwrap_or(xs.get(i).copied().unwrap_or(0.0) + g.advance * k / run.x_scale);
        let advance = ((next - xs.get(i).copied().unwrap_or(0.0)) / font_size) as f32;
        let Some(range) = run.source.bytes(&g.chars) else {
            return false;
        };
        glyphs.push(KrillaGlyph::new(GlyphId::new(g.gid), advance, 0.0, 0.0, 0.0, range, None));
    }
    let start = Point::from_xy(xs.first().copied().unwrap_or(0.0) as f32, (-first_y * k) as f32);
    let c = run.color;
    let surface = target.surface();
    surface.push_transform(&krilla::geom::Transform::from_row(
        m.as_coeffs()[0] as f32,
        m.as_coeffs()[1] as f32,
        m.as_coeffs()[2] as f32,
        m.as_coeffs()[3] as f32,
        m.as_coeffs()[4] as f32,
        m.as_coeffs()[5] as f32,
    ));
    surface.set_stroke(None);
    surface.set_fill(Some(Fill {
        paint: rgb::Color::new(c.r, c.g, c.b).into(),
        opacity: krilla::num::NormalizedF32::new(f32::from(c.a) / 255.0).unwrap_or(krilla::num::NormalizedF32::ONE),
        rule: FillRule::NonZero,
    }));
    surface.draw_glyphs(start, &glyphs, font, run.source.text, font_size as f32, false);
    surface.pop();
    true
}
