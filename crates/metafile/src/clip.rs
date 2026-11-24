//! Clip regions: how a clip is kept and combined.
//!
//! A clip is the **intersection of parts**, each a path with a fill rule, because that is what a target can do: push a clip, and push
//! another inside it. The combine modes that are not an intersection are made from it where the parts allow: *exclude* is the intersection
//! with the complement of a part (a rectangle around everything, with the part cut out by the odd-crossings rule); *union* and *xor* of two
//! single parts are one part made of both paths. A combination that these cannot make, such as a union of two clips that are each an
//! intersection, is refused with `None` and the caller reports the record. In files the regions hold one rectangle each and the
//! modes are copy, intersect, and exclude once in a while, so the refusal is for hostile input.

use std::rc::Rc;

use crate::geom::{self, Op, P};
use crate::limits::{MAX_CLIP_PARTS, MAX_CLIP_POINTS};
use crate::model::{ClipPart, Path, Seg};
use crate::xf::Xf;

/// The coordinates past which nothing is kept: the rectangle that stands for "everywhere" in a complement, and the clamp for any
/// coordinate (so a 32-bit float holds it with room, and a target can take it).
pub(crate) const BOUND: f64 = 1.0e7;

/// One part, with its rectangle when it is an axis-aligned one (so that two rectangles intersect to a rectangle, not to two parts).
#[derive(Clone, Debug)]
pub(crate) struct RPart {
    pub part: ClipPart,
    pub rect: Option<[f64; 4]>,
}

/// A clip: parts that are all to hold. No parts is no clip.
#[derive(Debug, Default)]
pub(crate) struct ClipSet {
    pub parts: Vec<RPart>,
}

pub(crate) type Clip = Rc<ClipSet>;

pub(crate) fn unbounded() -> Clip {
    Rc::new(ClipSet::default())
}

/// A clip that lets nothing through.
pub(crate) fn nothing() -> Clip {
    Rc::new(ClipSet { parts: vec![RPart { part: ClipPart { path: Path::default(), even_odd: false }, rect: None }] })
}

impl ClipSet {
    pub fn is_unbounded(&self) -> bool {
        self.parts.is_empty()
    }

    /// Whether no point passes: some part has no area at all.
    pub fn is_nothing(&self) -> bool {
        self.parts.iter().any(|p| p.part.path.is_empty() || p.rect.is_some_and(|r| r[2] <= r[0] || r[3] <= r[1]))
    }

    /// How many path points the parts hold.
    pub fn points(&self) -> u64 {
        self.parts.iter().map(|p| path_points(&p.part.path)).sum()
    }
}

pub(crate) fn path_points(path: &Path) -> u64 {
    path.segs
        .iter()
        .map(|s| match s {
            Seg::Move(_) | Seg::Line(_) => 1,
            Seg::Cubic(..) => 3,
            Seg::Close => 0,
        })
        .sum()
}

/// A path from ops in frame coordinates, clamped to [`BOUND`]; `None` when a number is not finite.
pub(crate) fn path_of(ops: &[Op]) -> Option<Path> {
    let pt = |p: P| -> Option<[f32; 2]> { if p[0].is_finite() && p[1].is_finite() { Some([clamp(p[0]), clamp(p[1])]) } else { None } };
    let mut segs = Vec::with_capacity(ops.len());
    for op in ops {
        segs.push(match *op {
            Op::M(p) => Seg::Move(pt(p)?),
            Op::L(p) => Seg::Line(pt(p)?),
            Op::C(a, b, c) => Seg::Cubic(pt(a)?, pt(b)?, pt(c)?),
            Op::Z => Seg::Close,
        });
    }
    Some(Path { segs })
}

pub(crate) fn clamp(v: f64) -> f32 {
    v.clamp(-BOUND, BOUND) as f32
}

/// The part for ops in frame coordinates: a quadrilateral that is axis-aligned is also kept as a rectangle.
pub(crate) fn part_of(ops: &[Op], even_odd: bool) -> Option<RPart> {
    let path = path_of(ops)?;
    let rect = axis_aligned(ops);
    Some(RPart { part: ClipPart { path, even_odd }, rect })
}

/// A clip of one rectangle `[left, top, right, bottom]` in frame coordinates.
pub(crate) fn rect_part(r: [f64; 4]) -> Option<RPart> {
    let (l, t, rr, b) = (r[0].min(r[2]), r[1].min(r[3]), r[0].max(r[2]), r[1].max(r[3]));
    let mut part = part_of(&geom::rect(l, t, rr, b), false)?;
    part.rect = Some([l, t, rr, b]);
    Some(part)
}

/// The rectangle `ops` is, when it is one with its edges along the axes: four corners (and one that closes it), nothing else.
fn axis_aligned(ops: &[Op]) -> Option<[f64; 4]> {
    let mut pts: Vec<P> = Vec::with_capacity(5);
    for op in ops {
        match *op {
            Op::M(p) if pts.is_empty() => pts.push(p),
            Op::L(p) if !pts.is_empty() => pts.push(p),
            Op::Z => {}
            _ => return None,
        }
        if pts.len() > 5 {
            return None;
        }
    }
    if pts.len() == 5 && pts.first() == pts.last() {
        pts.pop();
    }
    let [p0, p1, p2, p3] = pts.as_slice() else {
        return None;
    };
    let horizontal_first = p0[1] == p1[1] && p1[0] == p2[0] && p2[1] == p3[1] && p3[0] == p0[0];
    let vertical_first = p0[0] == p1[0] && p1[1] == p2[1] && p2[0] == p3[0] && p3[1] == p0[1];
    if !(horizontal_first || vertical_first) {
        return None;
    }
    let xs = [p0[0], p1[0], p2[0], p3[0]];
    let ys = [p0[1], p1[1], p2[1], p3[1]];
    let lo = |v: &[f64; 4]| v.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = |v: &[f64; 4]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    Some([lo(&xs), lo(&ys), hi(&xs), hi(&ys)])
}

pub(crate) fn single(part: RPart) -> Clip {
    Rc::new(ClipSet { parts: vec![part] })
}

/// Why two clips were not combined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClipError {
    /// The parts cannot make this combination (a union of clips that are each an intersection).
    Cannot,
    /// The result would hold more parts or points than a clip may (`MAX_CLIP_PARTS`, `MAX_CLIP_POINTS`): the caps.
    TooBig,
}

/// A combination's answer, with a clip past the caps noted on the budget as a cap reached.
pub(crate) fn capped<T>(cv: &mut crate::canvas::Canvas, answer: Result<T, ClipError>) -> Result<T, ClipError> {
    if answer.as_ref().err() == Some(&ClipError::TooBig) {
        cv.budget.note_cap("clip size");
    }
    answer
}

/// The clip and the meta region together: what every item of a device context is cut by.
pub(crate) fn effective_of(clip: &Clip, meta: &Clip) -> Result<Clip, ClipError> {
    if meta.is_unbounded() {
        Ok(clip.clone())
    } else if clip.is_unbounded() {
        Ok(meta.clone())
    } else {
        intersect(clip, meta)
    }
}

/// Whether a clip of this many parts and points is within the caps.
fn fits(parts: usize, points: u64) -> bool {
    parts <= MAX_CLIP_PARTS && points <= MAX_CLIP_POINTS
}

/// A copy of a clip's parts, when it is within the caps. Every clip the crate makes is, so this is the guard on what comes in from outside
/// a combination (a region read from a record).
fn copy_of(set: &ClipSet) -> Result<Clip, ClipError> {
    if fits(set.parts.len(), set.points()) { Ok(Rc::new(ClipSet { parts: set.parts.clone() })) } else { Err(ClipError::TooBig) }
}

/// The points in both: rectangles fold into one rectangle, other parts are kept. The size of the result is worked out before anything is
/// copied, and a result past the caps is refused: a clip made by intersecting path after path would otherwise gain a part for each and copy
/// them all each time.
pub(crate) fn intersect(a: &ClipSet, b: &ClipSet) -> Result<Clip, ClipError> {
    if a.is_unbounded() {
        return copy_of(b);
    }
    if b.is_unbounded() {
        return copy_of(a);
    }
    let mut rect: Option<[f64; 4]> = None;
    let (mut kept, mut points) = (0usize, 0u64);
    for p in a.parts.iter().chain(b.parts.iter()) {
        match p.rect {
            Some(r) => {
                rect = Some(match rect {
                    None => r,
                    Some(q) => [q[0].max(r[0]), q[1].max(r[1]), q[2].min(r[2]), q[3].min(r[3])],
                })
            }
            None => {
                kept += 1;
                points = points.saturating_add(path_points(&p.part.path));
            }
        }
    }
    // The folded rectangle is one more part of five points.
    if !fits(kept + usize::from(rect.is_some()), points + if rect.is_some() { 5 } else { 0 }) {
        return Err(ClipError::TooBig);
    }
    let mut parts: Vec<RPart> = a.parts.iter().chain(b.parts.iter()).filter(|p| p.rect.is_none()).cloned().collect();
    crate::work::clip_parts(parts.len());
    if let Some(r) = rect {
        // A rectangle with no area clips everything.
        let part = if r[2] <= r[0] || r[3] <= r[1] {
            Some(RPart { part: ClipPart { path: Path::default(), even_odd: false }, rect: None })
        } else {
            rect_part(r)
        };
        if let Some(part) = part {
            parts.insert(0, part);
        }
    }
    Ok(Rc::new(ClipSet { parts }))
}

/// How a new region meets the clip (EMF `RGN_*` 1 to 5 and EMF+ `CombineMode` 0 to 5, in one set).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// The new region replaces the clip.
    Replace,
    /// The points in both.
    And,
    /// The points in either.
    Or,
    /// The points in one and not both.
    Xor,
    /// The clip, less the region.
    Exclude,
    /// The region, less the clip.
    Complement,
}

/// The complement of one part: everything, with the part cut out by the odd-crossings rule.
fn complement(part: &RPart) -> Option<RPart> {
    let mut ops = geom::rect(-BOUND, -BOUND, BOUND, BOUND);
    ops.extend(path_ops(&part.part.path));
    // The cut-out is by the odd-crossings rule whatever rule the part had: for the parts that arise (one ring, or a rectangle) the rules agree.
    part_of(&ops, true)
}

/// The ops of a path.
pub(crate) fn path_ops(path: &Path) -> Vec<Op> {
    let wide = |p: [f32; 2]| [f64::from(p[0]), f64::from(p[1])];
    path.segs
        .iter()
        .map(|s| match *s {
            Seg::Move(p) => Op::M(wide(p)),
            Seg::Line(p) => Op::L(wide(p)),
            Seg::Cubic(a, b, c) => Op::C(wide(a), wide(b), wide(c)),
            Seg::Close => Op::Z,
        })
        .collect()
}

/// `old` combined with `new` by `mode`. The sizes are checked before anything is built: a combination of parts that would pass the caps is
/// `TooBig`, and one these parts cannot make is `Cannot`.
pub(crate) fn combine(old: &ClipSet, new: &ClipSet, mode: Mode) -> Result<Clip, ClipError> {
    let one = |set: &ClipSet| single_part(set).ok_or(ClipError::Cannot);
    let cut_of = |part: &RPart| complement(part).ok_or(ClipError::Cannot);
    match mode {
        Mode::Replace => copy_of(new),
        Mode::And => intersect(old, new),
        Mode::Exclude => match one(new)? {
            // Less everything: nothing is left.
            None => Ok(nothing()),
            Some(cut) => intersect(old, &ClipSet { parts: vec![cut_of(&cut)?] }),
        },
        Mode::Complement => match one(old)? {
            None => Ok(nothing()),
            Some(cut) => intersect(new, &ClipSet { parts: vec![cut_of(&cut)?] }),
        },
        Mode::Or => {
            // Everywhere, and anything, is everywhere.
            if old.is_unbounded() || new.is_unbounded() {
                return Ok(unbounded());
            }
            if !fits(1, old.points().saturating_add(new.points())) {
                return Err(ClipError::TooBig);
            }
            let (Some(a), Some(b)) = (one(old)?, one(new)?) else { return Ok(unbounded()) };
            // Both rings turned the same way, so that the non-zero rule takes their union.
            let mut ops = path_ops(&oriented(&a.part.path));
            ops.extend(path_ops(&oriented(&b.part.path)));
            part_of(&ops, false).map(single).ok_or(ClipError::Cannot)
        }
        Mode::Xor => {
            if !fits(1, old.points().saturating_add(new.points()).saturating_add(5)) {
                return Err(ClipError::TooBig);
            }
            match (one(old)?, one(new)?) {
                (None, None) => Ok(nothing()),
                (None, Some(only)) | (Some(only), None) => Ok(single(cut_of(&only)?)),
                (Some(a), Some(b)) => {
                    let mut ops = path_ops(&a.part.path);
                    ops.extend(path_ops(&b.part.path));
                    part_of(&ops, true).map(single).ok_or(ClipError::Cannot)
                }
            }
        }
    }
}

/// The one part of a clip: `Some(None)` for no parts, `Some(Some(part))` for one, and `None` for more.
fn single_part(set: &ClipSet) -> Option<Option<RPart>> {
    match set.parts.as_slice() {
        [] => Some(None),
        [one] => Some(Some(one.clone())),
        _ => None,
    }
}

/// The path with each figure turned to run positively (clockwise on a y-down screen), so that figures that overlap add up under the
/// non-zero rule.
fn oriented(path: &Path) -> Path {
    let mut out: Vec<Seg> = Vec::with_capacity(path.segs.len());
    let mut figure: Vec<Seg> = Vec::new();
    let flush = |figure: &mut Vec<Seg>, out: &mut Vec<Seg>| {
        if signed_area(figure) < 0.0 {
            out.extend(reversed(figure));
        } else {
            out.append(figure);
        }
        figure.clear();
    };
    for seg in &path.segs {
        if matches!(seg, Seg::Move(_)) {
            flush(&mut figure, &mut out);
        }
        figure.push(*seg);
    }
    flush(&mut figure, &mut out);
    Path { segs: out }
}

/// The area a figure's control polygon encloses, signed (positive when clockwise with y down).
fn signed_area(figure: &[Seg]) -> f64 {
    let mut pts: Vec<[f64; 2]> = Vec::new();
    for s in figure {
        match *s {
            Seg::Move(p) | Seg::Line(p) => pts.push([f64::from(p[0]), f64::from(p[1])]),
            Seg::Cubic(a, b, c) => {
                for p in [a, b, c] {
                    pts.push([f64::from(p[0]), f64::from(p[1])]);
                }
            }
            Seg::Close => {}
        }
    }
    let mut area = 0.0;
    for (i, p) in pts.iter().enumerate() {
        let q = pts.get((i + 1) % pts.len().max(1)).unwrap_or(p);
        area += p[0] * q[1] - q[0] * p[1];
    }
    area / 2.0
}

/// One figure run backwards.
fn reversed(figure: &[Seg]) -> Vec<Seg> {
    let closed = figure.last() == Some(&Seg::Close);
    // The vertices in order, with the curve between each pair where there is one.
    let mut points: Vec<[f32; 2]> = Vec::new();
    let mut between: Vec<Option<([f32; 2], [f32; 2])>> = Vec::new();
    for s in figure {
        match *s {
            Seg::Move(p) => points.push(p),
            Seg::Line(p) => {
                between.push(None);
                points.push(p);
            }
            Seg::Cubic(a, b, c) => {
                between.push(Some((a, b)));
                points.push(c);
            }
            Seg::Close => {}
        }
    }
    let mut out = Vec::with_capacity(figure.len());
    let Some(last) = points.last() else {
        return Vec::new();
    };
    out.push(Seg::Move(*last));
    for i in (0..points.len().saturating_sub(1)).rev() {
        let Some(&to) = points.get(i) else { break };
        match between.get(i).copied().flatten() {
            Some((a, b)) => out.push(Seg::Cubic(b, a, to)),
            None => out.push(Seg::Line(to)),
        }
    }
    if closed {
        out.push(Seg::Close);
    }
    out
}

/// A region of rectangles in logical units, moved by `xf` into frame coordinates: a part for each rectangle that is not a rectangle once
/// moved, or all of them together as a rectangle's union when there is more than one (their paths add up under the non-zero rule: the
/// rectangles of a region do not overlap, and are turned the same way).
pub(crate) fn region_of_rects(rects: &[[f64; 4]], xf: &Xf) -> Option<Clip> {
    if rects.is_empty() {
        return Some(nothing());
    }
    // A region of more rectangles than a clip may hold the points of is refused before a path is made of it.
    if rects.len().saturating_mul(5) as u64 > MAX_CLIP_POINTS {
        return None;
    }
    if let [one] = rects {
        let ops = geom::transformed(&geom::rect(one[0], one[1], one[2], one[3]), xf);
        return Some(single(part_of(&ops, false)?));
    }
    let mut ops = Vec::with_capacity(rects.len() * 5);
    for r in rects {
        ops.extend(geom::transformed(&geom::rect(r[0], r[1], r[2], r[3]), xf));
    }
    let path = oriented(&path_of(&ops)?);
    Some(single(RPart { part: ClipPart { path, even_odd: false }, rect: None }))
}
