//! The whole layout: paragraphs measured, lines stacked in the block, and every line emitted.
//!
//! Each line is a box of height `L`, the baseline three quarters of the way down it;
//! the first baseline is `t - 0.75 L1` for top alignment, `b + T - 0.75 L1` for bottom and `(t + b) / 2 + T / 2 - 0.75 L1` for the
//! middle, where `t` and `b` are the top and bottom of the area inside the margins and `T` is the stack's height; and each next
//! baseline drops by a quarter of the box above and three quarters of its own, plus the space between paragraphs. The space after a
//! paragraph and before the next is added between them, and left out above the first and below the last. Nothing is
//! clipped or clamped: a stack taller than the block spills.

use diacraft_fonts::FontDb;
use kurbo::{Rect, Size};

use crate::breaking::{EPS, Tabs, break_lines, break_opportunities};
use crate::bullet::{FirstText, resolve};
use crate::emit::{Env, emit_line};
use crate::formats::{BlockStyle, ParaStyle, RunStyle, Sanitizer};
use crate::laid::{Background, Laid, Line, Note};
use crate::limits::{MAX_LENGTH, MAX_LINES, MAX_NOTES, MAX_RUNS};
use crate::model::{BlockFormat, ParaFormat, Run};
use crate::place::{DEFAULT_BULLET_GAP, ParaPlan, align, measure, plan_lines};
use crate::prepare::{Paragraph, prepare};
use crate::shape::{Shaping, shape_paragraph};
use crate::vertical::lay_out_down;

/// A paragraph measured, with the numbers its lines are placed by.
struct Planned {
    plan: ParaPlan,
    a_first: f64,
    a_rest: f64,
    gap: f64,
    /// Characters (cells) of the paragraph the line cap left unlaid.
    left_out: usize,
}

/// Measure one paragraph. `lines_left` bounds the lines it may make.
fn plan_paragraph<'r>(
    sh: &mut Shaping<'_, 'r>,
    runs: &'r [Run],
    para: &Paragraph,
    block: &BlockStyle,
    width: Option<f64>,
    lines_left: usize,
    san: &mut Sanitizer,
) -> Planned {
    let fmt: &'r ParaFormat = runs.get(para.format_run as usize).map_or(&*DEFAULT_PARA, |r| &*r.para);
    let style = ParaStyle::new(fmt, san);
    let shaped = shape_paragraph(sh, para, style.rtl);
    let first_cluster = shaped.clusters.first();
    let run = first_cluster.map_or(para.format_run, |c| c.run);
    let run_style = sh.styles.get(run as usize);
    let (first_size, font) = run_style.map_or((0.0, ""), |s| (s.size, s.font));
    let face = first_cluster.map_or_else(|| sh.entry_for(font), |c| c.face);
    let bullet = resolve(sh, fmt, &FirstText { run, size: first_size, face }, san);
    // The first line's text starts `TextPosAfterBullet` from the bullet's start, 0.25 inch when that is 0 or less, and never inside the bullet.
    let gap = bullet.as_ref().map_or(0.0, |b| {
        let tpab = san.len(fmt.text_pos_after_bullet, 0.0);
        (if tpab > 0.0 { tpab } else { DEFAULT_BULLET_GAP }).max(b.width)
    });
    let (m, a_first, a_rest, z) = measure(block, width, &style, gap);
    let allowed = break_opportunities(&shaped);
    let mut tab_adv = vec![0.0; shaped.clusters.len()];
    let tabs = Tabs { stops: &style.tabs, default: block.default_tab };
    let spans = break_lines(&shaped, &allowed, &m, &tabs, &mut tab_adv, lines_left.max(1));
    let lf_run = para.lf.map_or(para.format_run, |(r, _)| r);
    let empty_size = sh.styles.get(lf_run as usize).map_or(0.0, |s| s.size);
    let lines = plan_lines(para, &shaped, &tab_adv, &spans, &style, empty_size);
    // The line cap may have stopped before the paragraph's end: what it left is counted, so the caller is told.
    let next = spans.last().map_or(0, |s| s.next);
    let left_out = shaped.clusters.get(next).map_or(0, |c| para.cells.len().saturating_sub(c.cells.start));
    Planned { plan: ParaPlan { shaped, style, tab_adv, lines, bullet, z }, a_first, a_rest, gap, left_out }
}

static DEFAULT_PARA: std::sync::LazyLock<ParaFormat> = std::sync::LazyLock::new(ParaFormat::default);

/// How wide the block is when it is as wide as its widest line.
fn natural_extent(planned: &[Planned]) -> f64 {
    let mut widest = 0.0f64;
    for p in planned {
        for l in &p.plan.lines {
            let first = l.span.start == 0;
            let a = if first { p.a_first } else { p.a_rest };
            let pushed = if first && p.plan.bullet.is_some() { p.gap } else { 0.0 };
            widest = widest.max(a + pushed + l.width);
        }
    }
    widest
}

/// The baselines of every line, top to bottom, and the stack's height `T`.
fn stack(planned: &[Planned], block: &BlockStyle) -> (Vec<f64>, f64) {
    let mut heights: Vec<(f64, f64)> = Vec::new();
    for (pi, p) in planned.iter().enumerate() {
        for (li, l) in p.plan.lines.iter().enumerate() {
            let gap = match (li, pi.checked_sub(1).and_then(|i| planned.get(i))) {
                (0, Some(prev)) => prev.plan.style.sp_after + p.plan.style.sp_before,
                _ => 0.0,
            };
            heights.push((l.height, gap));
        }
    }
    let total: f64 = heights.iter().map(|(h, g)| h + g).sum();
    let height = block.height.unwrap_or(total + block.top + block.bottom);
    let (t, b) = (height - block.top, block.bottom);
    let l1 = heights.first().map_or(0.0, |h| h.0);
    let first = match block.valign {
        2 => b + total - 0.75 * l1,
        1 => (t + b) / 2.0 + total / 2.0 - 0.75 * l1,
        _ => t - 0.75 * l1,
    };
    let mut out = Vec::with_capacity(heights.len());
    let mut y = first;
    let mut prev_h = 0.0;
    for (i, (h, gap)) in heights.iter().enumerate() {
        if i > 0 {
            y -= 0.25 * prev_h + 0.75 * h + gap;
        }
        out.push(y);
        prev_h = *h;
    }
    (out, total)
}

/// The notes a layout ends with.
fn notes(sh: Shaping<'_, '_>, san: &Sanitizer, cut: Cuts) -> Vec<Note> {
    let mut out = sh.notes;
    if sh.standins_omitted > 0 {
        out.push(Note::StandInsOmitted { count: sh.standins_omitted });
    }
    if cut.chars > 0 {
        out.push(Note::Truncated { chars: cut.chars });
    }
    if cut.runs > 0 {
        out.push(Note::RunsCut { runs: cut.runs });
    }
    if cut.lines_chars > 0 {
        out.push(Note::LinesCut { chars: cut.lines_chars });
    }
    if sh.marks_cut {
        out.push(Note::MarksCut);
    }
    if sh.budget_used {
        out.push(Note::ShapingBudgetUsed);
    }
    if sh.search_cut {
        out.push(Note::FaceSearchCut);
    }
    if sh.missing_glyphs > 0 {
        out.push(Note::MissingGlyphs { clusters: sh.missing_glyphs });
    }
    // A face that failed, once each, whether its shaper could not be used or a later call into its tables panicked.
    let mut failed: Vec<(u64, String)> = Vec::new();
    for entry in &sh.entries {
        let id = entry.face.id();
        if (entry.face.is_broken() || sh.shaping_failed.contains(&id)) && failed.len() < MAX_NOTES && !failed.iter().any(|(i, _)| *i == id) {
            failed.push((id, entry.face.family().to_owned()));
        }
    }
    out.extend(failed.into_iter().map(|(_, family)| Note::FaceFailed { family }));
    if sh.bullet_cut {
        out.push(Note::BulletStringCut);
    }
    if san.bad {
        out.push(Note::BadNumber);
    }
    out
}

/// What a layout left out, by what cut it.
#[derive(Clone, Copy, Debug, Default)]
struct Cuts {
    chars: usize,
    runs: usize,
    lines_chars: usize,
}

/// The one rectangle of a text background, hugging the lines, or none if there is no text to hug.
fn background(lines: &[Line], block: &BlockStyle, has_text: bool) -> Option<Background> {
    let (color, trans) = block.background?;
    if !has_text {
        return None;
    }
    let first = lines.first()?;
    let last = lines.last()?;
    let x0 = lines.iter().map(|l| l.bullet.as_ref().and_then(|b| b.glyphs.first()).map_or(l.x, |g| g.x.min(l.x))).fold(f64::INFINITY, f64::min);
    let x1 = lines.iter().map(|l| l.x + l.width).fold(f64::NEG_INFINITY, f64::max);
    Some(Background { rect: Rect::new(x0, last.bottom, x1.max(x0), first.top), color, opacity: 1.0 - trans })
}

/// Lay out `runs` in a block of the given format. `width` is the block's width in paper inches (`TxtWidth` times `PageScale` over
/// `DrawingScale`), or `None` to lay the text out with no wrapping and align it within its own widest line. A width of zero or less is
/// laid out one cluster to a line, and a width that isn't a number is read as `None`. With `TextDirection` 1 the characters
/// run down the block: see [`crate::vertical`].
pub fn lay_out(runs: &[Run], block: &BlockFormat, width: Option<f64>, db: &FontDb) -> Laid {
    if block.text_direction == 1 { lay_out_down(runs, block, width, db) } else { lay_out_across(runs, block, width, db) }
}

/// The layout across the block, for text of any direction: `width` is how far a line may run (see [`lay_out`]). Vertical text is laid out by this
/// in a block turned the other way up.
pub(crate) fn lay_out_across(runs: &[Run], block: &BlockFormat, width: Option<f64>, db: &FontDb) -> Laid {
    let mut san = Sanitizer::default();
    let bs = BlockStyle::new(block, &mut san);
    if width.is_some_and(|w| !w.is_finite()) {
        san.bad = true;
    }
    let width = width.filter(|w| w.is_finite()).map(|w| w.clamp(-MAX_LENGTH, MAX_LENGTH));
    let prepared = prepare(runs);
    let kept = runs.get(..runs.len().min(MAX_RUNS)).unwrap_or(runs);
    let styles: Vec<RunStyle<'_>> = kept.iter().map(|r| RunStyle::new(&r.char, &mut san)).collect();
    let mut sh = Shaping::new(db, &styles);
    let mut planned: Vec<Planned> = Vec::new();
    let mut lines_total = 0usize;
    let mut cut = Cuts { chars: prepared.dropped, runs: prepared.runs_dropped, lines_chars: 0 };
    for para in &prepared.paragraphs {
        if lines_total >= MAX_LINES {
            cut.lines_chars += para.cells.len() + 1;
            continue;
        }
        let p = plan_paragraph(&mut sh, kept, para, &bs, width, MAX_LINES - lines_total, &mut san);
        lines_total += p.plan.lines.len();
        cut.lines_chars += p.left_out;
        planned.push(p);
    }
    let natural = natural_extent(&planned);
    for p in &mut planned {
        let z = p.plan.z.unwrap_or(natural);
        let (a_first, a_rest, gap) = (p.a_first, p.a_rest, p.gap);
        align(&mut p.plan, a_first, a_rest, gap, z);
    }
    let (baselines, total) = stack(&planned, &bs);
    let mut lines = Vec::with_capacity(baselines.len());
    let mut at = baselines.iter();
    for (pi, (p, para)) in planned.iter().zip(&prepared.paragraphs).enumerate() {
        let env = Env { para: &p.plan, cells: para, entries: &sh.entries, styles: &styles };
        for plan in &p.plan.lines {
            let mut line = emit_line(&env, plan, at.next().copied().unwrap_or(0.0));
            line.paragraph = pi;
            lines.push(line);
        }
    }
    let height = bs.height.unwrap_or(total + bs.top + bs.bottom);
    let overflow = total > height - bs.top - bs.bottom + EPS || planned.iter().any(|p| p.plan.lines.iter().any(|l| l.overflow));
    let has_text = planned.iter().any(|p| p.plan.shaped.clusters.iter().any(|c| c.kind == crate::prepare::Kind::Text));
    let size = Size::new(lines.iter().map(|l| l.width).fold(0.0, f64::max), total);
    let background = background(&lines, &bs, has_text);
    drop(planned);
    Laid { lines, size, overflow, background, notes: notes(sh, &san, cut) }
}
