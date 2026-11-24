//! Graph layout: placing the shapes of a diagram by the connectors between them.
//!
//! [`layout`] takes a [`Graph`] (the boxes of the shapes and the connectors, each from one shape to another) and [`Settings`] (a [`Style`] and the spaces between
//! shapes) and gives the middle of each shape. It changes nothing and knows nothing of a document: the engine builds the graph from a page, writes the places as
//! cell writes, and routes the connectors again.
//!
//! **The styles** are the five of the Re-Layout Page gallery (of the UI specifications; the reference application publishes their names and no
//! algorithm, so every rule below is **ours**):
//!
//! * [`Style::Flowchart`], in four directions: layers along the flow, with the order inside them that crosses connectors least, set across so that connectors run
//!   straight where they can (`layered.rs`);
//! * [`Style::Hierarchy`], four directions and three alignments of a parent over its children (`tree.rs`);
//! * [`Style::CompactTree`], eight outlines: the shapes in a column or a row and each level set off from the one before (`tree.rs`);
//! * [`Style::Radial`]: a tree in rings about its root (`round.rs`);
//! * [`Style::Circular`]: every shape on one circle, joined shapes near each other (`round.rs`).
//!
//! **What every style does the same way.** The diagram is split into its separate parts (shapes joined, by connectors in either direction, are one part), each part is
//! laid out by itself, and the parts are set in rows (`pack.rs`), in the order of their places on the page now. Shapes keep their sizes. No two boxes overlap, at the
//! spaces asked for or more. The whole result is then moved so that its top-left corner is where the top-left corner of the diagram was. A connector between a
//! shape and itself, and one repeated, are ignored. Positions are decided from the order and the places the shapes have now, so a diagram laid out twice comes out the
//! same, and one that is already laid out changes little.
//!
//! **Bounds** (the diagram is hostile input). At most [`MAX_NODES`] shapes and [`MAX_EDGES`] connectors; a flowchart's stand-in points for connectors that cross layers
//! are counted before any is made and refused past [`MAX_DUMMIES`]; every pass charges a [`Work`] before it runs and the sum may not pass [`MAX_WORK`]. Every walk is
//! a loop over lists kept in a few vectors, so a chain of a hundred thousand shapes or a ring of them costs no stack. A refusal is an [`Error`] with the limit in words.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod forest;
mod frame;
#[cfg(kani)]
mod kani_proofs;
mod layered;
mod model;
mod org;
mod pack;
mod place;
mod round;
mod tree;

pub use model::{
    Align, Direction, Error, Graph, MAX_DUMMIES, MAX_EDGES, MAX_NODES, MAX_SIZE, MAX_WORK, Node, OrgSettings, Settings, Side, Spread, Style, Work,
};

use forest::{components, csr};
use frame::{Frame, Item};
use model::size;

/// The middle of each shape of `graph`, as `settings` ask, in the order of `graph.nodes`. See the module documentation.
pub fn layout(graph: &Graph, settings: &Settings) -> Result<Vec<(f64, f64)>, Error> {
    layout_with(graph, settings, &mut Work::new())
}

/// [`layout`], with the work charged to `work` (a test reads what it was).
pub fn layout_with(graph: &Graph, settings: &Settings, work: &mut Work) -> Result<Vec<(f64, f64)>, Error> {
    graph.check()?;
    settings.check()?;
    if graph.nodes.is_empty() {
        return Ok(Vec::new());
    }
    work.charge((graph.nodes.len() + graph.edges.len()) as u64)?;
    let edges = forest::clean_edges(&graph.edges);
    let mut places = match settings.style {
        Style::CompactTree { run, level } => tree::compact(&graph.nodes, &edges, run, level, settings.gaps(), work)?,
        style => {
            let (frame, gaps) = frame_of(style, settings);
            by_parts(graph, &edges, (frame, gaps), &Laying::Style(style), work)?
        }
    };
    anchor(graph, &mut places);
    if places.iter().any(|p| !(p.0.is_finite() && p.1.is_finite())) {
        return Err(Error::TooLarge("the diagram would be too large to place: lay out part of it".to_owned()));
    }
    Ok(places)
}

/// The frame a style is laid out in and the spaces across and along it.
fn frame_of(style: Style, settings: &Settings) -> (Frame, (f64, f64)) {
    let (gap_x, gap_y) = settings.gaps();
    match style {
        Style::Flowchart(dir) | Style::Hierarchy(dir, _) => {
            let frame = Frame::new(dir);
            (frame, frame.gaps(gap_x, gap_y))
        }
        _ => (Frame::down(), (gap_x, gap_y)),
    }
}

/// The middle of each shape of `graph` laid out as an org chart (`org.rs`): a tree in which each shape says how its reports are set out, as `settings` ask, in the order of
/// `graph.nodes`. The same guarantees as [`layout`]: no two boxes overlap, sizes are kept, the diagram's top-left corner stays, a ring or a repeat is harmless.
pub fn org_chart(graph: &Graph, settings: &OrgSettings<'_>) -> Result<Vec<(f64, f64)>, Error> {
    org_chart_with(graph, settings, &mut Work::new())
}

/// [`org_chart`], with the work charged to `work` (a test reads what it was).
pub fn org_chart_with(graph: &Graph, settings: &OrgSettings<'_>, work: &mut Work) -> Result<Vec<(f64, f64)>, Error> {
    graph.check()?;
    settings.check()?;
    if graph.nodes.is_empty() {
        return Ok(Vec::new());
    }
    work.charge((graph.nodes.len() + graph.edges.len()) as u64)?;
    let edges = forest::clean_edges(&graph.edges);
    let frame = Frame::new(settings.direction);
    let (gap_x, gap_y) = settings.gaps();
    let mut places = by_parts(graph, &edges, (frame, frame.gaps(gap_x, gap_y)), &Laying::Org(settings.spreads), work)?;
    anchor(graph, &mut places);
    if places.iter().any(|p| !(p.0.is_finite() && p.1.is_finite())) {
        return Err(Error::TooLarge("the diagram would be too large to place: lay out part of it".to_owned()));
    }
    Ok(places)
}

/// How a part of the diagram is laid out: in one of the five styles, or as an org chart (each shape's [`Spread`], by the shape's number in the whole diagram).
enum Laying<'a> {
    Style(Style),
    Org(&'a [Spread]),
}

/// One part of the diagram laid out as `laying` says: its shapes, its connectors (numbered in the part), the numbers the shapes have in the whole diagram and the spaces across and
/// along → each shape's middle.
fn lay_part(
    laying: &Laying<'_>,
    items: &[Item],
    edges: &[(u32, u32)],
    members: &[u32],
    gaps: (f64, f64),
    work: &mut Work,
) -> Result<Vec<(f64, f64)>, Error> {
    let style = match laying {
        Laying::Style(style) => *style,
        Laying::Org(spreads) => {
            let own: Vec<Spread> = members.iter().map(|&v| spreads.get(v as usize).copied().unwrap_or(Spread::Across)).collect();
            return org::org(items, edges, &own, gaps, work);
        }
    };
    match style {
        Style::Flowchart(_) => layered::flowchart(items, edges, gaps, work),
        Style::Hierarchy(_, align) => tree::hierarchy(items, edges, align, gaps, work),
        Style::Radial => round::radial(items, edges, gaps.0.max(gaps.1), work),
        Style::Circular => round::circular(items, edges, gaps.0.max(gaps.1), work),
        // The compact tree has no parts: it is laid out whole, by the caller.
        Style::CompactTree { .. } => Ok(vec![(0.0, 0.0); items.len()]),
    }
}

/// Lays out each separate part of the diagram by itself, sets the parts in rows, and gives the places on the page.
fn by_parts(graph: &Graph, edges: &[(u32, u32)], space: (Frame, (f64, f64)), laying: &Laying<'_>, work: &mut Work) -> Result<Vec<(f64, f64)>, Error> {
    let (frame, gaps) = space;
    let items: Vec<Item> = graph.nodes.iter().map(|n| frame.item(n)).collect();
    let parts = components(&items, edges);
    let at = match parts.as_slice() {
        [_] => {
            let all: Vec<u32> = (0..items.len() as u32).collect();
            lay_part(laying, &items, edges, &all, gaps, work)?
        }
        _ => lay_parts(&items, edges, &parts, (laying, gaps), work)?,
    };
    Ok(at.into_iter().map(|(c, m)| frame.to_page(c, m)).collect())
}

/// The parts laid out one by one and set in rows: each part's boxes begin at the corner it is given.
fn lay_parts(
    items: &[Item],
    edges: &[(u32, u32)],
    parts: &[Vec<u32>],
    how: (&Laying<'_>, (f64, f64)),
    work: &mut Work,
) -> Result<Vec<(f64, f64)>, Error> {
    let (laying, gaps) = how;
    let n = items.len();
    let mut part_of = vec![0_u32; n];
    let mut local_of = vec![0_u32; n];
    for (p, members) in parts.iter().enumerate() {
        for (i, &v) in members.iter().enumerate() {
            part_of[v as usize] = p as u32;
            local_of[v as usize] = i as u32;
        }
    }
    let ids: Vec<(u32, u32)> = edges.iter().enumerate().map(|(i, e)| (part_of[e.0 as usize], i as u32)).collect();
    let by_part = csr(parts.len(), &ids);
    let mut laid: Vec<Vec<(f64, f64)>> = Vec::with_capacity(parts.len());
    let mut sizes: Vec<(f64, f64)> = Vec::with_capacity(parts.len());
    let mut corners: Vec<(f64, f64)> = Vec::with_capacity(parts.len());
    for (p, members) in parts.iter().enumerate() {
        let sub_items: Vec<Item> = members.iter().map(|&v| items[v as usize]).collect();
        let sub_edges: Vec<(u32, u32)> =
            by_part.row(p).iter().map(|&e| (local_of[edges[e as usize].0 as usize], local_of[edges[e as usize].1 as usize])).collect();
        let at = lay_part(laying, &sub_items, &sub_edges, members, gaps, work)?;
        let (lo, hi) = extent(&sub_items, &at);
        sizes.push((hi.0 - lo.0, hi.1 - lo.1));
        corners.push(lo);
        laid.push(at);
    }
    let origins = pack::rows(&sizes, gaps.0, gaps.1);
    let mut out = vec![(0.0_f64, 0.0_f64); n];
    for (p, members) in parts.iter().enumerate() {
        let (lo, to) = (corners[p], origins[p]);
        for (&v, &(c, m)) in members.iter().zip(&laid[p]) {
            out[v as usize] = (c - lo.0 + to.0, m - lo.1 + to.1);
        }
    }
    Ok(out)
}

/// The corners of the box that holds the shapes of a part, `(across, along)` least and most.
fn extent(items: &[Item], at: &[(f64, f64)]) -> ((f64, f64), (f64, f64)) {
    let mut lo = (f64::INFINITY, f64::INFINITY);
    let mut hi = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (item, &(c, m)) in items.iter().zip(at) {
        lo = (lo.0.min(c - item.cs / 2.0), lo.1.min(m - item.ms / 2.0));
        hi = (hi.0.max(c + item.cs / 2.0), hi.1.max(m + item.ms / 2.0));
    }
    if lo.0.is_finite() && lo.1.is_finite() { (lo, hi) } else { ((0.0, 0.0), (0.0, 0.0)) }
}

/// The top-left corner of the boxes `(x, y, width, height)`.
fn top_left(boxes: impl Iterator<Item = (f64, f64, f64, f64)>) -> (f64, f64) {
    let (mut left, mut top) = (f64::INFINITY, f64::NEG_INFINITY);
    for (x, y, w, h) in boxes {
        left = left.min(x - w / 2.0);
        top = top.max(y + h / 2.0);
    }
    (left, top)
}

/// Moves the places so that the top-left corner of the boxes is where it is in `graph`.
fn anchor(graph: &Graph, places: &mut [(f64, f64)]) {
    let old = top_left(graph.nodes.iter().map(|n| (n.x, n.y, size(n.width), size(n.height))));
    let new = top_left(graph.nodes.iter().zip(places.iter()).map(|(n, p)| (p.0, p.1, size(n.width), size(n.height))));
    let (dx, dy) = (old.0 - new.0, old.1 - new.1);
    if dx.is_finite() && dy.is_finite() {
        for p in places.iter_mut() {
            *p = (p.0 + dx, p.1 + dy);
        }
    }
}
