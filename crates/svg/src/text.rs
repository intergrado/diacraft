//! Letters as an SVG `<text>` element, where they map to glyphs one for one.
//!
//! **Where the font allows.** An SVG file cannot carry a font the way a PDF does, so `<text>` names the face (`font-family`, its weight and
//! slant) and a viewer that has it draws the face we laid the text out in; one that has not draws another, and the letters keep the places
//! the layout gave them, since every one has its own x. Two kinds of run are not written so, and are drawn as outlines (the walk does that
//! when this answers `false`, and counts them as `TextAsOutlines`):
//!
//! * a run where a letter and a glyph are not one for one: a ligature, a letter with its marks, a joined script, text that runs right to left
//!   (its glyphs are in visual order, so a `<text>` would be read backwards), or a letter the face has no glyph for;
//! * a run with a made-up bold or slant, which the element has no way to say (the walk filters those out before it asks).
//!
//! **Where it sits.** The element's own frame is the block's: its transform is the page's, the block's to paper, and a flip, so that the
//! block's y (up) is the element's (down), and a scale to take the numbers back to output units, in which the font size and the x of each
//! glyph are written. `FontScale` is a horizontal stretch in that transform.

use diacraft_render::TextRun;
use kurbo::Affine;

use crate::num::{coord, escape, hex, matrix, num, opacity};
use crate::target::SvgTarget;

/// The most characters one run may hold to be written as text; the walk budgets glyphs far lower, so this is a second wall.
const MAX_RUN_CHARS: usize = 100_000;

/// The generic family to fall back on when the viewer lacks the face: a guess from the face's own name.
fn generic(family: &str) -> &'static str {
    let k = family.to_ascii_lowercase();
    if ["mono", "courier", "hack", "consolas", "cousine", "typewriter"].iter().any(|m| k.contains(m)) {
        "monospace"
    } else if (k.contains("serif") && !k.contains("sans"))
        || ["times", "tinos", "georgia", "cambria", "caladea", "garamond", "palatino"].iter().any(|m| k.contains(m))
    {
        "serif"
    } else {
        "sans-serif"
    }
}

/// Writes `run` as a `<text>` element; `false` and nothing written when its letters are not one glyph each.
pub(crate) fn write_run(target: &mut SvgTarget, run: &TextRun<'_>) -> bool {
    let family = run.face.family();
    if family.is_empty() || run.glyphs.is_empty() || run.glyphs.len() > MAX_RUN_CHARS {
        return false;
    }
    let color = run.color;
    // One glyph for one letter, in the order of the text.
    let mut letters = String::with_capacity(run.glyphs.len());
    let mut previous_end = 0usize;
    for (i, g) in run.glyphs.iter().enumerate() {
        let one = g.chars.end.checked_sub(g.chars.start) == Some(1);
        let in_order = i == 0 || g.chars.start >= previous_end;
        let Some(text) = run.source.slice(&g.chars) else {
            return false;
        };
        let mut chars = text.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            return false;
        };
        if g.gid == 0 || !one || !in_order || !g.x.is_finite() || !g.y.is_finite() {
            return false;
        }
        previous_end = g.chars.end;
        // A tab or a line break stands as a space: the glyph's own x places what follows.
        letters.push(if c.is_control() { ' ' } else { c });
    }
    if letters.chars().all(|c| c == ' ') {
        // Nothing to see, and nothing to select that a reader would miss.
        return true;
    }
    let k = target.scale();
    if !(k.is_finite() && k > 0.0 && run.x_scale.is_finite() && run.x_scale > 0.0) {
        return false;
    }
    // The element's frame: x right in output units over the stretch, y down in output units; the block's frame is x right and y up in
    // inches. `to_local` takes the element's frame to the block's.
    let to_local = Affine::new([run.x_scale / k, 0.0, 0.0, -1.0 / k, 0.0, 0.0]);
    let m = target.to_px_matrix() * run.to_paper * to_local;
    let coeffs = m.as_coeffs();
    if !coeffs.iter().all(|c| c.is_finite() && c.abs() < 1.0e9) {
        return false;
    }
    let xs: Vec<String> = run.glyphs.iter().map(|g| coord(g.x * k / run.x_scale)).collect();
    let first_y = run.glyphs.first().map_or(0.0, |g| g.y);
    let same_baseline = run.glyphs.iter().all(|g| (g.y - first_y).abs() < 1.0e-9);
    let ys = if same_baseline { coord(-first_y * k) } else { run.glyphs.iter().map(|g| coord(-g.y * k)).collect::<Vec<_>>().join(" ") };
    let mut s = format!(
        "<text transform=\"{}\" font-family=\"{}, {}\" font-size=\"{}\"",
        matrix(coeffs),
        escape(&family.replace(['\'', '"'], "")),
        generic(family),
        num(run.size * k, 4)
    );
    if run.face.weight() != 400 {
        s.push_str(&format!(" font-weight=\"{}\"", run.face.weight()));
    }
    if run.face.is_italic() {
        s.push_str(" font-style=\"italic\"");
    }
    s.push_str(&format!(" fill=\"{}\"", hex(color)));
    if let Some(o) = opacity(color.a) {
        s.push_str(&format!(" fill-opacity=\"{o}\""));
    }
    s.push_str(&format!(" x=\"{}\" y=\"{ys}\" xml:space=\"preserve\">{}</text>\n", xs.join(" "), escape(&letters)));
    target.put(&s);
    true
}
