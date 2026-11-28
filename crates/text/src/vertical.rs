//! Vertical text: `TextDirection` 1.
//!
//! With `TextDirection` 1 the characters run down the block, columns stack from the right edge to the left, `VerticalAlign` 0, 1 and 2 seat
//! the text from the right edge, the middle or the left edge, and `HAlign` 0 starts a column at the area's top. That is the layout across,
//! turned a quarter clockwise: a line is a column, the start of a line is the top, the top of a line box is its right side, and the line a
//! text wraps against is the block's height. So the layout is **done across, in a block turned the other way up, and the result is turned**
//! (ours: no source says more than the rules above, and they are what a turn gives):
//!
//! * the block laid out across is as wide as the real one is tall, as tall as it is wide, and its margins turn with it: the real top margin
//!   is the start of a line, the bottom one its end, the right one the top of the stack and the left one its bottom;
//! * a point `(u, v)` of that layout (`u` along the line from the left, `v` up from the bottom) is the real point `(v, height - u)`.
//!   So the `x` of a baseline, of a line box's top and bottom is the `v` it had, and a line's `x` and `width` are lengths down the block;
//! * a letter that is not East Asian lies on its side: its run has `turned` set, its origin is where the turn puts it, and the renderer
//!   turns its outline a quarter clockwise about it. An East Asian character stands upright: it is centred across the column
//!   and in its cell down it, and the run it is in is not turned. A bullet is drawn like the letters;
//! * the lines through and under text are turned with the rest, so an underline lies down the baseline side of the column, which is its
//!   left (ours), **except in a run of Japanese or Korean, whose underline lies on the right of the characters**, as the Text
//!   dialog's page says (design call, which replaced for those two languages): see
//!   [`turn_decoration`].
//!
//! **Cost.** One pass over the glyphs after the layout across, and one more over the source characters, each once: a glyph is copied once, however the
//! way up changes along the line (`work::glyph_copies` counts it).

use diacraft_fonts::script::slot_of;
use diacraft_fonts::{Face, FontDb, FontSlot};
use kurbo::{Rect, Size};

use crate::laid::{Background, Decoration, GlyphRun, Laid, Line, PlacedGlyph};
use crate::layout::lay_out_across;
use crate::limits::{MAX_CHARS, MAX_LENGTH, MAX_RUNS};
use crate::model::{BlockFormat, Run};
use crate::work;

/// The face's own share above and below the baseline where the face says none: the ideographic box most East Asian faces use.
const DEFAULT_ASCENT: f64 = 0.88;
const DEFAULT_DESCENT: f64 = 0.12;

/// Does a character stand upright in vertical text? East Asian letters do (the scripts `AsianFont` is for), and so do the punctuation and the
/// full width forms that go with them.
fn upright(c: char) -> bool {
    slot_of(c) == FontSlot::Asian || matches!(c as u32, 0x3000..=0x303F | 0x3200..=0x33FF | 0xFF01..=0xFF60 | 0xFFE0..=0xFFE6)
}

/// Lay `runs` out down a block (`TextDirection` 1). `width` is the block's width and `block.height` its height, in paper inches, as for
/// text across.
pub(crate) fn lay_out_down(runs: &[Run], block: &BlockFormat, width: Option<f64>, db: &FontDb) -> Laid {
    let across = BlockFormat {
        left_margin: block.top_margin,
        right_margin: block.bottom_margin,
        top_margin: block.right_margin,
        bottom_margin: block.left_margin,
        text_direction: 0,
        height: width,
        ..block.clone()
    };
    let laid = lay_out_across(runs, &across, block.height, db, true);
    // How tall the block is: its height, or where its longest column ends when it has none.
    let height = match block.height.filter(|h| h.is_finite()) {
        Some(h) => h.clamp(-MAX_LENGTH, MAX_LENGTH),
        None => {
            let reach = laid.lines.iter().map(|l| l.x + l.width).fold(0.0_f64, f64::max);
            let margin = if block.bottom_margin.is_finite() { block.bottom_margin.clamp(-MAX_LENGTH, MAX_LENGTH) } else { 0.0 };
            reach + margin
        }
    };
    let source: Vec<char> = runs.iter().take(MAX_RUNS).flat_map(|r| r.text.chars()).take(MAX_CHARS).collect();
    turn(laid, &source, height)
}

/// The real point of a point of the layout across.
fn point(u: f64, v: f64, height: f64) -> (f64, f64) {
    (v, height - u)
}

/// Turn a layout across into one down a block `height` tall.
fn turn(laid: Laid, source: &[char], height: f64) -> Laid {
    let lines = laid.lines.into_iter().map(|line| turn_line(line, source, height)).collect();
    let background = laid.background.map(|b| {
        let r = b.rect;
        let (x0, y1) = point(r.x0, r.y0, height);
        let (x1, y0) = point(r.x1, r.y1, height);
        Background { rect: Rect::new(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)), ..b }
    });
    Laid { lines, size: Size::new(laid.size.height, laid.size.width), background, ..laid }
}

fn turn_line(line: Line, source: &[char], height: f64) -> Line {
    // The centre line of the column: an upright letter is centred on it.
    let centre = (line.top + line.bottom) / 2.0;
    let mut runs: Vec<GlyphRun> = Vec::with_capacity(line.runs.len());
    for run in &line.runs {
        turn_run(run, source, centre, height, &mut runs);
    }
    let bullet = line.bullet.as_ref().map(|b| {
        let mut turned = empty_like(b, true);
        turned.glyphs = b.glyphs.iter().map(|g| placed(g, g.y, height - g.x)).collect();
        turned
    });
    let decorations = line.decorations.iter().map(|d| turn_decoration(d, centre, height)).collect();
    Line { vertical: true, runs, bullet, decorations, ..line }
}

/// A line through or under the text, turned: it keeps its place along the column (`x0` and `x1` are lengths down the block) and lies across it where it
/// did across, which is the left for an underline. **An underline of a Japanese or Korean run lies on the right of the characters** (
/// for those two languages): its inner edge is at the right edge of the characters' cells, which are centred on the column's centre line
/// and as wide as the run's size, and its thickness is the one it had.
fn turn_decoration(d: &Decoration, centre: f64, height: f64) -> Decoration {
    let top = match d.right_of {
        // `top` is the right edge of a turned line, which lies `thickness` to its left.
        Some(right) => centre + right.em / 2.0 + right.outward + d.thickness,
        None => d.top,
    };
    Decoration { vertical: true, x0: height - d.x1, x1: height - d.x0, top, right_of: None, ..*d }
}

/// A run's glyphs, in runs of the same way up: the letters on their sides together and the upright letters together.
fn turn_run(run: &GlyphRun, source: &[char], centre: f64, height: f64, out: &mut Vec<GlyphRun>) {
    let (ascent, descent) = shares(&run.face);
    let mut open: Option<GlyphRun> = None;
    for g in &run.glyphs {
        let stands = source.get(g.chars.start).copied().is_some_and(upright);
        if open.as_ref().is_none_or(|o| o.turned == stands) {
            if let Some(done) = open.take() {
                out.push(done);
            }
            open = Some(empty_like(run, !stands));
        }
        let moved = if stands {
            // Centred across the column on the advance, and down it in the cell the advance makes, where the em box (above the baseline by the
            // ascent, below it by the descent) centres.
            let middle = height - (g.x + g.advance / 2.0);
            placed(g, centre - g.advance / 2.0, middle - (ascent - descent) / 2.0 * run.size)
        } else {
            let (x, y) = point(g.x, g.y, height);
            placed(g, x, y)
        };
        if let Some(o) = open.as_mut() {
            o.glyphs.push(moved);
        }
    }
    if let Some(done) = open {
        out.push(done);
    }
}

/// The glyph `g` at another place: its glyph, advance and characters are the same.
fn placed(g: &PlacedGlyph, x: f64, y: f64) -> PlacedGlyph {
    work::glyph_copy(1);
    PlacedGlyph { gid: g.gid, x, y, advance: g.advance, chars: g.chars.clone() }
}

/// A run with the face, size, colour and everything else of `run` and no glyphs, turned or not. **Built field by field**, not as `GlyphRun { .. , ..run.clone() }`:
/// that copies the run's glyphs to throw them away, and a text that changes the way up at every character (a letter and a Han character alternating)
/// makes a run for each, so it copies the glyphs of the run once for each glyph of it, which is the square of the text.
fn empty_like(run: &GlyphRun, turned: bool) -> GlyphRun {
    GlyphRun {
        face: run.face.clone(),
        size: run.size,
        x_scale: run.x_scale,
        embolden: run.embolden,
        slant: run.slant,
        color: run.color,
        opacity: run.opacity,
        turned,
        glyphs: Vec::new(),
    }
}

/// The face's ascent and descent as shares of the em, or the ideographic box.
fn shares(face: &Face) -> (f64, f64) {
    let (a, d) = (face.ascent_share(), face.descent_share());
    if a.is_finite() && d.is_finite() && a + d > 0.0 { (a, d) } else { (DEFAULT_ASCENT, DEFAULT_DESCENT) }
}
