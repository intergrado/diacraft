//! A measured line to what a renderer draws: glyph runs in visual order with every glyph's place, the lines through and under the
//! text, and the bullet.
//!
//! Clusters of one face, size, colour and style share a run, so a format change is a new run and nothing else is. A cluster's
//! glyphs keep the order the shaper gave them, which is already visual, and the clusters are put in visual order by the
//! bidirectional algorithm (rule L2). Decorations take their place and thickness from the face's own tables.

use std::ops::Range;

use crate::bidi::{Edge, edge_of, reset_edges, visual_order};
use crate::formats::{Decor, RunStyle};
use crate::laid::{Decoration, DecorationKind, GlyphRun, Line, PlacedGlyph};
use crate::model::Rgba;
use crate::place::{LinePlan, ParaPlan};
use crate::prepare::{Kind, Paragraph};
use crate::shape::{Cluster, FaceEntry};

/// The shear of a made-up italic, as the tangent of 12 degrees (the angle is this project's, no source's).
pub(crate) const SYNTHETIC_SLANT_DEGREES: f64 = 12.0;
/// The stroke that makes a face with no bold bolder, as a share of the size (the weight is this project's).
pub(crate) const SYNTHETIC_BOLD_EM: f64 = 0.03;
/// The lines a face doesn't recommend (a project rule): an underline's top below the baseline and a strikeout's above it,
/// and the thickness of both, as shares of the em.
const DEFAULT_UNDERLINE: (f64, f64) = (-0.1, 0.05);
const DEFAULT_STRIKE: (f64, f64) = (0.3, 0.05);
const DEFAULT_THICKNESS: f64 = 0.05;

/// What a line is emitted from.
pub(crate) struct Env<'a, 'r> {
    pub para: &'a ParaPlan,
    pub cells: &'a Paragraph,
    pub entries: &'a [FaceEntry],
    pub styles: &'a [RunStyle<'r>],
}

/// What makes two clusters one run: everything a renderer sets once for a run of glyphs.
#[derive(Clone, Copy, PartialEq)]
struct RunKey {
    face: usize,
    size: u64,
    x_scale: u64,
    y_off: u64,
    color: Rgba,
    opacity: u64,
    decor: Decor,
}

struct Open {
    key: RunKey,
    run: GlyphRun,
    x0: f64,
    y_off: f64,
    decor: Decor,
}

fn run_key(c: &Cluster, style: &RunStyle<'_>) -> RunKey {
    RunKey {
        face: c.face,
        size: c.size.to_bits(),
        x_scale: c.x_scale.to_bits(),
        y_off: c.y_off.to_bits(),
        color: style.color,
        opacity: style.opacity.to_bits(),
        decor: style.decor,
    }
}

fn glyph_run(entry: &FaceEntry, size: f64, x_scale: f64, color: Rgba, opacity: f64, glyphs: Vec<PlacedGlyph>) -> GlyphRun {
    GlyphRun {
        face: entry.face.clone(),
        size,
        x_scale,
        embolden: if entry.bold { size * SYNTHETIC_BOLD_EM } else { 0.0 },
        slant: if entry.italic { SYNTHETIC_SLANT_DEGREES.to_radians().tan() } else { 0.0 },
        color,
        opacity,
        turned: false,
        glyphs,
    }
}

/// One line through or under a run: a rectangle from `x0` to `x1` whose top edge is `top` and which is `thickness` tall.
fn bar(kind: DecorationKind, span: (f64, f64), top: f64, thickness: f64, run: &GlyphRun) -> Decoration {
    Decoration { kind, x0: span.0, x1: span.1, top, thickness, color: run.color, opacity: run.opacity, vertical: false }
}

/// The lines a run asks for, placed from its face's tables.
fn decorations(open: &Open, x1: f64, baseline: f64, out: &mut Vec<Decoration>) {
    let (face, size) = (&open.run.face, open.run.size);
    let y = baseline + open.y_off;
    let under = face.underline().map_or(DEFAULT_UNDERLINE, |s| (s.top, s.thickness));
    let strike = face.strikeout().map_or(DEFAULT_STRIKE, |s| (s.top, s.thickness));
    let (d, span, run) = (open.decor, (open.x0, x1), &open.run);
    // A double line beats the single one: two lines, not three, a thickness apart.
    if d.double_underline {
        for step in [0.0, 2.0] {
            out.push(bar(DecorationKind::DoubleUnderline, span, y + (under.0 - step * under.1) * size, under.1 * size, run));
        }
    } else if d.underline {
        out.push(bar(DecorationKind::Underline, span, y + under.0 * size, under.1 * size, run));
    }
    if d.overline {
        let t = face.underline().map_or(DEFAULT_THICKNESS, |s| s.thickness);
        out.push(bar(DecorationKind::Overline, span, y + (face.ascent_share().max(0.0) + t) * size, t * size, run));
    }
    if d.double_strike {
        for step in [-1.0, 1.0] {
            out.push(bar(DecorationKind::DoubleStrikethrough, span, y + (strike.0 - step * strike.1) * size, strike.1 * size, run));
        }
    } else if d.strike {
        out.push(bar(DecorationKind::Strikethrough, span, y + strike.0 * size, strike.1 * size, run));
    }
}

struct LineBuilder<'a, 'r> {
    env: &'a Env<'a, 'r>,
    plan: &'a LinePlan,
    baseline: f64,
    runs: Vec<GlyphRun>,
    decorations: Vec<Decoration>,
    open: Option<Open>,
}

impl<'a, 'r> LineBuilder<'a, 'r> {
    fn close(&mut self, x1: f64) {
        if let Some(open) = self.open.take() {
            if open.decor != Decor::default() {
                decorations(&open, x1, self.baseline, &mut self.decorations);
            }
            if !open.run.glyphs.is_empty() {
                self.runs.push(open.run);
            }
        }
    }

    /// The run to add a cluster's glyphs to: the open one if the cluster has its key, else a new one.
    fn run_for(&mut self, c: &Cluster, pen: f64) -> Option<&mut Open> {
        let entry = self.env.entries.get(c.face)?;
        let style = self.env.styles.get(c.run as usize)?;
        let key = run_key(c, style);
        if self.open.as_ref().is_none_or(|o| o.key != key) {
            self.close(pen);
            let run = glyph_run(entry, c.size, c.x_scale, style.color, style.opacity, Vec::new());
            self.open = Some(Open { key, run, x0: pen, y_off: c.y_off, decor: style.decor });
        }
        self.open.as_mut()
    }

    /// The source characters a cluster draws.
    fn source(&self, c: &Cluster) -> Range<usize> {
        let cells = &self.env.cells.cells;
        let first = cells.get(c.cells.start).map_or(0, |x| x.src as usize);
        let last = c.cells.end.checked_sub(1).and_then(|i| cells.get(i)).map_or(first, |x| x.src as usize);
        first..last + 1
    }

    /// Add a text cluster's glyphs and move the pen past them.
    fn text(&mut self, c: &Cluster, pen: f64) -> f64 {
        let (src, baseline, extra) = (self.source(c), self.baseline, self.plan.extra_per_space);
        // The glyphs live in the paragraph, not in `self`, so borrowing them doesn't hold `self` while a run is opened.
        let env = self.env;
        let glyphs = env.para.shaped.glyphs.get(c.glyphs.clone()).unwrap_or(&[]);
        let mut x = pen;
        if let Some(open) = self.run_for(c, pen) {
            for g in glyphs {
                open.run.glyphs.push(PlacedGlyph { gid: g.gid, x: x + g.dx, y: baseline + c.y_off + g.dy, advance: g.adv, chars: src.clone() });
                x += g.adv;
            }
        } else {
            x += glyphs.iter().map(|g| g.adv).sum::<f64>();
        }
        x += c.letterspace;
        if c.space {
            x += extra;
        }
        x
    }

    /// A tab: no glyph, but a decoration runs across it when the text on both sides has one.
    fn tab(&mut self, c: &Cluster, pen: f64, adv: f64) -> f64 {
        if c.size > 0.0 {
            self.run_for(c, pen);
        }
        pen + adv
    }

    /// The hyphen a line cut at a soft hyphen ends with, in the cluster's own face.
    fn hyphen(&mut self, c: &Cluster, pen: f64) -> f64 {
        let src = self.source(c);
        let (baseline, gid) = (self.baseline, self.env.entries.get(c.face).map_or(0, |e| e.face.glyph_for('-')));
        if let Some(open) = self.run_for(c, pen) {
            open.run.glyphs.push(PlacedGlyph { gid, x: pen, y: baseline + c.y_off, advance: c.hyphen, chars: src });
        }
        pen + c.hyphen
    }
}

/// The bullet of a paragraph's first line, or `None`.
fn bullet_run(env: &Env<'_, '_>, plan: &LinePlan, baseline: f64) -> Option<GlyphRun> {
    let mark = env.para.bullet.as_ref()?;
    let entry = env.entries.get(mark.face)?;
    let mut x = plan.bullet_x?;
    let mut glyphs = Vec::with_capacity(mark.glyphs.len());
    for (gid, adv) in &mark.glyphs {
        glyphs.push(PlacedGlyph { gid: *gid, x, y: baseline, advance: *adv, chars: 0..0 });
        x += adv;
    }
    Some(glyph_run(entry, mark.size, 1.0, mark.color, mark.opacity, glyphs))
}

/// Emit one line of a paragraph with its baseline at `baseline`.
pub(crate) fn emit_line(env: &Env<'_, '_>, plan: &LinePlan, baseline: f64) -> Line {
    let span = &plan.span;
    let slice = env.para.shaped.clusters.get(span.start..span.end).unwrap_or(&[]);
    let mut levels: Vec<u8> = slice.iter().map(|c| c.level).collect();
    if levels.iter().any(|l| *l != 0) {
        let edges: Vec<Edge> =
            slice.iter().map(|c| edge_of(env.cells.cells.get(c.cells.clone()).unwrap_or(&[]).iter().map(|cell| cell.ch))).collect();
        reset_edges(&mut levels, &edges, u8::from(env.para.style.rtl));
    }
    let mut b = LineBuilder { env, plan, baseline, runs: Vec::new(), decorations: Vec::new(), open: None };
    let mut pen = plan.x;
    for idx in visual_order(&levels) {
        let Some(c) = slice.get(idx) else { continue };
        pen = match c.kind {
            Kind::Tab => b.tab(c, pen, env.para.tab_adv.get(span.start + idx).copied().unwrap_or(0.0)),
            Kind::SoftHyphen if span.hyphen && span.start + idx + 1 == span.end => b.hyphen(c, pen),
            Kind::Text if c.size > 0.0 => b.text(c, pen),
            _ => pen,
        };
    }
    b.close(pen);
    Line {
        paragraph: 0,
        chars: plan.chars.clone(),
        baseline,
        top: baseline + 0.75 * plan.height,
        bottom: baseline - 0.25 * plan.height,
        height: plan.height,
        x: plan.x,
        width: plan.width,
        rtl: env.para.style.rtl,
        vertical: false,
        runs: b.runs,
        decorations: b.decorations,
        bullet: bullet_run(env, plan, baseline),
    }
}
