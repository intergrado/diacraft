//! Moving items by a transform: a picture put inside another, and a picture whose header gave no frame moved to its own
//! extent.

use std::sync::Arc;

use crate::canvas::Canvas;
use crate::clip::{self, Clip, ClipSet, RPart};
use crate::model::{self, Drawn, Item, LinearPaint, Paint, Path, Picture, Placement, RadialPaint, Seg, Stroke};
use crate::xf::Xf;

fn pt(xf: &Xf, p: [f32; 2]) -> [f32; 2] {
    let q = xf.apply([f64::from(p[0]), f64::from(p[1])]);
    [clip::clamp(q[0]), clip::clamp(q[1])]
}

pub(crate) fn path(xf: &Xf, p: &Path) -> Path {
    let segs = p
        .segs
        .iter()
        .map(|s| match *s {
            Seg::Move(a) => Seg::Move(pt(xf, a)),
            Seg::Line(a) => Seg::Line(pt(xf, a)),
            Seg::Cubic(a, b, c) => Seg::Cubic(pt(xf, a), pt(xf, b), pt(xf, c)),
            Seg::Close => Seg::Close,
        })
        .collect();
    Path { segs }
}

/// A linear gradient moved by `xf`. Its stripes of one colour stay stripes, but their direction is not the image of the old direction under
/// a map that is not a similarity, so the new start and end are found from the gradient's own slope: the parameter at a point `q` is
/// (`M`⁻¹`q` − `a`)·`d` / |`d`|², a function whose slope is `w`, and the new end is `w` / |`w`|² from the new start.
fn linear(xf: &Xf, l: &LinearPaint) -> LinearPaint {
    let stops = l.stops.clone();
    let d = [l.end[0] - l.start[0], l.end[1] - l.start[1]];
    let len2 = d[0] * d[0] + d[1] * d[1];
    let start = xf.apply(l.start);
    let (Some(inv), true) = (xf.inverse(), len2 > 0.0) else {
        return LinearPaint { start, end: xf.apply(l.end), stops };
    };
    // w = M⁻ᵀ · d / |d|²
    let w = [(inv.a * d[0] + inv.b * d[1]) / len2, (inv.c * d[0] + inv.d * d[1]) / len2];
    let w2 = w[0] * w[0] + w[1] * w[1];
    if w2 > 0.0 && w2.is_finite() {
        LinearPaint { start, end: [start[0] + w[0] / w2, start[1] + w[1] / w2], stops }
    } else {
        LinearPaint { start, end: xf.apply(l.end), stops }
    }
}

fn radial(xf: &Xf, r: &RadialPaint) -> RadialPaint {
    let right = xf.apply_vector([r.basis[0], r.basis[1]]);
    let up = xf.apply_vector([r.basis[2], r.basis[3]]);
    RadialPaint { center: xf.apply(r.center), basis: [right[0], right[1], up[0], up[1]], stops: r.stops.clone() }
}

fn paint(xf: &Xf, p: &Paint) -> Paint {
    match p {
        Paint::Solid(c) => Paint::Solid(*c),
        Paint::Linear(l) => Paint::Linear(Arc::new(linear(xf, l))),
        Paint::Radial(r) => Paint::Radial(Arc::new(radial(xf, r))),
    }
}

fn stroke(xf: &Xf, s: &Stroke) -> Stroke {
    let k = xf.mean_scale();
    Stroke {
        paint: paint(xf, &s.paint),
        width: s.width * k,
        cap: s.cap,
        join: s.join,
        miter_limit: s.miter_limit,
        dashes: s.dashes.iter().map(|d| d * k).collect(),
        dash_offset: s.dash_offset * k,
    }
}

/// One item moved by `xf` (its clip index is left as it is).
pub(crate) fn item(xf: &Xf, it: &Item) -> Item {
    let drawn = match &it.drawn {
        Drawn::Path(p) => Drawn::Path(Box::new(model::PathItem {
            path: path(xf, &p.path),
            fill: p.fill.as_ref().map(|f| model::Fill { paint: paint(xf, &f.paint), even_odd: f.even_odd }),
            stroke: p.stroke.as_ref().map(|s| stroke(xf, s)),
        })),
        Drawn::Bitmap(b) => {
            let mut moved = (**b).clone();
            moved.quad = [xf.apply(b.quad[0]), xf.apply(b.quad[1]), xf.apply(b.quad[2])];
            Drawn::Bitmap(Box::new(moved))
        }
        Drawn::Text(t) => {
            let mut moved = (**t).clone();
            let right = xf.apply_vector([t.basis[0], t.basis[1]]);
            let up = xf.apply_vector([t.basis[2], t.basis[3]]);
            moved.basis = [right[0], right[1], up[0], up[1]];
            moved.placement = match &t.placement {
                Placement::Origins(o) => Placement::Origins(o.iter().map(|p| xf.apply(*p)).collect()),
                Placement::Flow { origin, h_align } => Placement::Flow { origin: xf.apply(*origin), h_align: *h_align },
            };
            Drawn::Text(Box::new(moved))
        }
    };
    Item { clip: it.clip, drawn }
}

/// A clip of a picture, moved by `xf` into the clip state of another.
fn clip_set(xf: &Xf, c: &model::Clip) -> Clip {
    let parts = c
        .parts
        .iter()
        .map(|p| {
            let moved = path(xf, &p.path);
            let rect = axis_aligned_rect(&moved);
            RPart { part: model::ClipPart { path: moved, even_odd: p.even_odd }, rect }
        })
        .collect();
    std::rc::Rc::new(ClipSet { parts })
}

fn axis_aligned_rect(p: &Path) -> Option<[f64; 4]> {
    clip::part_of(&clip::path_ops(p), false).and_then(|part| part.rect)
}

/// What an item costs to move: the points of its path, and a unit for anything else.
fn cost(it: &Item) -> u64 {
    match &it.drawn {
        Drawn::Path(p) => clip::path_points(&p.path).saturating_add(p.stroke.as_ref().map_or(0, |s| s.dashes.len() as u64)).saturating_add(1),
        _ => 1,
    }
}

/// Puts `inner` into `outer`, moved by `xf` and cut by `bound` (the outer clip at the draw) and by `extra` (the part of the plane the
/// inner picture's draw covers). Items keep their order. The inner picture's report is taken in. Returns false when a cap ended it.
///
/// Every copy is charged first: a clip of the inner picture by the points it holds, an item by the points of its path. An inner
/// clip that makes a clip past the caps with `bound` takes its items out, and the cap is noted.
pub(crate) fn merge(outer: &mut Canvas, inner: Picture, xf: &Xf, bound: &Clip, extra: Option<&Clip>) -> bool {
    outer.report.absorb(inner.report);
    // Each of the inner picture's clips becomes the outer clip it makes with `bound`, named once.
    let base = match extra {
        Some(e) => clip::capped(outer, clip::intersect(bound, e)),
        None => Ok(bound.clone()),
    };
    let Ok(base) = base else { return false };
    let mut mapped: Vec<Option<Clip>> = Vec::with_capacity(inner.clips.len());
    for c in &inner.clips {
        if c.parts.is_empty() {
            mapped.push(Some(base.clone()));
            continue;
        }
        let points: u64 = c.parts.iter().map(|p| clip::path_points(&p.path)).sum();
        if !outer.budget.work(points.saturating_add(base.points()).saturating_add(1)) {
            return false;
        }
        let made = clip::intersect(&base, &clip_set(xf, c));
        mapped.push(clip::capped(outer, made).ok());
    }
    for it in &inner.items {
        if !outer.budget.alive() || !outer.budget.work(cost(it)) {
            return false;
        }
        let Some(clip) = usize::try_from(it.clip).ok().and_then(|i| mapped.get(i)).and_then(Clone::clone) else { continue };
        let moved = item(xf, it);
        let points = match &moved.drawn {
            Drawn::Path(p) => clip::path_points(&p.path),
            _ => 0,
        };
        outer.emit(&clip, moved.drawn, points);
    }
    true
}

/// A picture whose header gave no frame: the frame becomes the extent of what it draws, and the items move to put its corner at the origin
/// (a WMF with no window takes the box of what it draws).
pub(crate) fn fit_to_extent(pic: &mut Picture) {
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    let mut grow = |p: [f64; 2]| {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    };
    for it in &pic.items {
        match &it.drawn {
            Drawn::Path(p) => {
                for s in &p.path.segs {
                    match *s {
                        Seg::Move(a) | Seg::Line(a) => grow([f64::from(a[0]), f64::from(a[1])]),
                        Seg::Cubic(a, b, c) => {
                            for q in [a, b, c] {
                                grow([f64::from(q[0]), f64::from(q[1])]);
                            }
                        }
                        Seg::Close => {}
                    }
                }
            }
            Drawn::Bitmap(b) => {
                let [ul, ur, ll] = b.quad;
                for q in [ul, ur, ll, [ur[0] + ll[0] - ul[0], ur[1] + ll[1] - ul[1]]] {
                    grow(q);
                }
            }
            Drawn::Text(t) => match &t.placement {
                Placement::Origins(o) => o.iter().for_each(|p| grow(*p)),
                Placement::Flow { origin, .. } => grow(*origin),
            },
        }
    }
    if !(lo[0].is_finite() && hi[0].is_finite() && lo[1].is_finite() && hi[1].is_finite()) {
        return;
    }
    let shift = Xf::translate(-lo[0], -lo[1]);
    for it in &mut pic.items {
        *it = item(&shift, it);
    }
    for c in &mut pic.clips {
        for part in &mut c.parts {
            part.path = path(&shift, &part.path);
        }
    }
    pic.width = (hi[0] - lo[0]).max(1.0);
    pic.height = (hi[1] - lo[1]).max(1.0);
}
