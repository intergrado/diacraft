//! Where a paragraph's lines start across the block and how tall each is.
//!
//! A line's near limit `a` is the area's near edge plus `IndLeft`, plus `IndFirst` on a paragraph's first line, and its far limit
//! `z` is the area's far edge less `IndRight`; a place outside the block moves to the nearest point on its edge, and an `IndRight`
//! too large for the room gives way to `IndLeft`. Text aligned to the start begins at `a`, aligned to the end finishes at `z`, centred is
//! centred between them, all by the line's width without its trailing spaces. Justified text stretches the gaps of every line but a
//! paragraph's last. A bullet sits at `a` on the first line and pushes that line's text in by `TextPosAfterBullet`.
//!
//! **A right-to-left paragraph is the mirror image** (design call, which replaced the rule
//! that alignment 0 and 2 keep their sides). The near edge of a left-to-right paragraph is the block's left and the far one its right; the near edge of a
//! right-to-left one is the right and the far one the left. So `IndLeft`, the start indent, is on the right and `IndRight`, the stop indent, on the left, the
//! first-line indent counts from the start indent, a bullet stands at the right, and tab stops are measured from the right edge. Everything is measured, broken
//! into lines and placed as for a left-to-right paragraph, in a frame whose origin is the near edge and whose `x` runs away from it, and only
//! then turned about the block's width into the block's own frame ([`align`]): alignment 0 is the start (the near side) in either direction, and 2 the end
//! (the far side), which is what puts a line aligned 0 against the right. A paragraph with any other `Flags` than 1 is not touched.

use std::ops::Range;

use crate::breaking::{EPS, LineSpan, Measure};
use crate::bullet::BulletMark;
use crate::formats::{Align, BlockStyle, ParaStyle};
use crate::prepare::{Kind, Paragraph};
use crate::shape::Shaped;

/// How far from the bullet's start the text starts when `TextPosAfterBullet` is 0 or less, in inches.
pub(crate) const DEFAULT_BULLET_GAP: f64 = 0.25;

/// One line, measured, before the stack decides where it sits.
#[derive(Clone, Debug)]
pub(crate) struct LinePlan {
    pub span: LineSpan,
    /// The height of the box the line makes, from its largest size and `SpLine`.
    pub height: f64,
    /// The width without trailing spaces, with a hyphen if the line ends at a soft hyphen.
    pub width: f64,
    pub chars: Range<usize>,
    /// The left edge of the line's text, in the block's frame, and the stretch of each of its spaces: filled by [`align`].
    pub x: f64,
    pub extra_per_space: f64,
    pub overflow: bool,
    /// The left edge of the bullet (the first line of a bulleted paragraph).
    pub bullet_x: Option<f64>,
}

/// A paragraph measured: its clusters and widths, its lines, and what it needs to be placed.
#[derive(Debug)]
pub(crate) struct ParaPlan {
    pub shaped: Shaped,
    pub style: ParaStyle,
    pub tab_adv: Vec<f64>,
    pub lines: Vec<LinePlan>,
    pub bullet: Option<BulletMark>,
    /// The far limit of the block's room for this paragraph, from its near edge, or `None` when the block is as wide as its widest line.
    pub z: Option<f64>,
    /// The block's width, which a right-to-left paragraph's places are turned about, or `None` when the block is as wide as its widest line.
    pub width: Option<f64>,
}

/// Clamp `v` to `lo..=hi` without panicking when the range is empty (a block of negative width): `hi` wins.
pub(crate) fn clamp_to(v: f64, lo: f64, hi: f64) -> f64 {
    v.max(lo).min(hi)
}

/// Where this paragraph's lines start and how much room they have, as distances from its near edge (the block's left, or its right for a right-to-left
/// paragraph): the measure for breaking, where the first and later lines start and the far limit. `gap` is how far the first line's text is pushed in by a
/// bullet. The measure's places, which tab stops are measured by, are from the text area's near edge, inside the margin.
pub(crate) fn measure(block: &BlockStyle, width: Option<f64>, style: &ParaStyle, gap: f64) -> (Measure, f64, f64, Option<f64>) {
    let hi = width.unwrap_or(f64::INFINITY);
    let (near, far) = if style.rtl { (block.right, block.left) } else { (block.left, block.right) };
    let a_first = clamp_to(near + style.ind_left + style.ind_first, 0.0, hi);
    let a_rest = clamp_to(near + style.ind_left, 0.0, hi);
    let z = width.map(|w| clamp_to(w - far - style.ind_right, 0.0, hi));
    let room = |a: f64, pushed: f64| z.map_or(f64::INFINITY, |z| z.max(a) - a - pushed);
    let m = Measure { first_x: a_first + gap - near, first_avail: room(a_first, gap), rest_x: a_rest - near, rest_avail: room(a_rest, 0.0) };
    (m, a_first, a_rest, z)
}

/// The source characters a line holds, spaces at its end included.
fn source_range(para: &Paragraph, shaped: &Shaped, span: &LineSpan) -> Range<usize> {
    let cl = &shaped.clusters;
    let src = |cluster: usize, last: bool| {
        let c = cl.get(cluster)?;
        let cell = para.cells.get(if last { c.cells.end.checked_sub(1)? } else { c.cells.start })?;
        Some(cell.src as usize)
    };
    let start = src(span.start, false);
    let end = span.full_end.checked_sub(1).filter(|_| span.full_end > span.start).and_then(|i| src(i, true)).map(|s| s + 1);
    match (start, end) {
        (Some(s), Some(e)) => s..e.max(s),
        _ => {
            let at = start.or_else(|| para.lf.map(|(_, s)| s as usize)).or_else(|| para.cells.last().map(|c| c.src as usize + 1)).unwrap_or(0);
            at..at
        }
    }
}

/// The largest size on a line: of its drawn clusters, else of any cluster on it, else the size of the run that sizes an empty one.
fn line_size(shaped: &Shaped, span: &LineSpan, empty_size: f64) -> f64 {
    let max_of = |range: Range<usize>| {
        shaped.clusters.get(range).unwrap_or(&[]).iter().map(|c| c.line_size).fold(None, |m: Option<f64>, s| Some(m.map_or(s, |m| m.max(s))))
    };
    max_of(span.start..span.end).or_else(|| max_of(span.start..span.full_end)).unwrap_or(empty_size)
}

/// A line's width: its clusters' advances, tabs as measured, and the hyphen a soft hyphen draws if the line ends there.
fn line_width(shaped: &Shaped, tab_adv: &[f64], span: &LineSpan) -> f64 {
    let mut w = 0.0;
    for (i, c) in shaped.clusters.iter().enumerate().take(span.end).skip(span.start) {
        w += if c.kind == Kind::Tab { tab_adv.get(i).copied().unwrap_or(0.0) } else { c.adv };
    }
    if span.hyphen
        && let Some(c) = span.end.checked_sub(1).and_then(|i| shaped.clusters.get(i))
    {
        w += c.hyphen;
    }
    w
}

/// Measure every line of a paragraph.
pub(crate) fn plan_lines(
    para: &Paragraph,
    shaped: &Shaped,
    tab_adv: &[f64],
    spans: &[LineSpan],
    style: &ParaStyle,
    empty_size: f64,
) -> Vec<LinePlan> {
    spans
        .iter()
        .map(|span| {
            let size = line_size(shaped, span, empty_size).max(0.0);
            LinePlan {
                span: *span,
                height: style.line_height(size),
                width: line_width(shaped, tab_adv, span),
                chars: source_range(para, shaped, span),
                x: 0.0,
                extra_per_space: 0.0,
                overflow: false,
                bullet_x: None,
            }
        })
        .collect()
}

/// Set where every line of a paragraph starts, once the far limit is known. `a_first` and `a_rest` are the near limits, `gap` the first line's bullet push, `z`
/// the far limit (all from the paragraph's near edge, as [`measure`] gives them), and `edge` the width of the block a right-to-left paragraph is turned about.
pub(crate) fn align(para: &mut ParaPlan, a_first: f64, a_rest: f64, gap: f64, z: f64, edge: f64) {
    let n = para.lines.len();
    let has_bullet = para.bullet.is_some();
    let bullet_width = para.bullet.as_ref().map_or(0.0, |b| b.width);
    let rtl = para.style.rtl;
    // From the near edge, alignment 0 puts a line against the near side and 2 against the far one, in either direction: that is what puts a right-to-left
    // line aligned 0 against the right. The places below are all from the near edge.
    let align = para.style.align;
    for (i, line) in para.lines.iter_mut().enumerate() {
        let first = line.span.start == 0;
        let a = if first { a_first } else { a_rest };
        let a_text = if first && has_bullet { a + gap } else { a };
        let z = z.max(a_text);
        let room = z - a_text;
        let spaces = para.shaped.clusters.get(line.span.start..line.span.end).unwrap_or(&[]).iter().filter(|c| c.space).count();
        line.overflow = line.width > room + EPS;
        let start = match align {
            Align::Left => a_text,
            Align::Centre => (a_text + z) / 2.0 - line.width / 2.0,
            Align::Right => z - line.width,
            Align::Justify | Align::Distribute => {
                let last = i + 1 == n;
                if (align == Align::Distribute || !last) && spaces > 0 && room > line.width + EPS {
                    line.extra_per_space = (room - line.width) / spaces as f64;
                    line.width = room;
                }
                a_text
            }
        };
        // Turned about the block's width, a right-to-left line's near edge is its right one: its left edge is `edge` less where it starts and how wide it is.
        // The bullet, which draws from its left edge, is turned the same way.
        let (x, bullet_x) = if rtl { (edge - start - line.width, edge - a - bullet_width) } else { (start, a) };
        line.x = x;
        line.bullet_x = (first && has_bullet).then_some(bullet_x);
    }
}
