//! The shapes a layout, an auto-space or a move off the page breaks works on, read from a page with every count checked before anything is made of it.
//!
//! **What is in the scope** (ours: only "the selected part of the diagram, or the whole page when nothing is selected"):
//!
//! * **a shape** is a shape at the top of the page's list that is not a guide and not one-dimensional. A group is one shape. A shape named that is inside a group is
//!   not (the group is the shape that moves), and is counted in [`Scope::inside_groups`] so that the answer can say it;
//! * **a connector** is a one-dimensional shape at the top of the page that a `Connect` row glues an end of. One whose ends are both glued to shapes of the scope
//!   (or to a shape inside one) joins them in the graph. On a page, every connector of the page is routed again; on a selection, the connectors glued to the shapes
//!   selected are;
//! * **left where they are**: a shape on a locked layer (the rule, applied to the shapes a layout does not name: the layers' Lock "stops shapes on the
//!   layer from being selected, moved or edited"), and a shape that **moves with another** of the scope (a member of a container and a callout tied to a shape: the
//!   shape they follow is the one laid out, and they go along, `callouts::followers`). Neither is in the graph, and the locked ones are listed in the answer.
//!
//! **What it costs.** One pass over the page's top list (at most [`MAX_PAGE_SHAPES`] shapes) and one over its `Connect` rows (at most [`MAX_GLUE_ROWS`]), one walk of the
//! page for the shapes it must find by `ID` (a selection, the connectors of a selection, the shapes inside groups that a connector is glued to), and the callouts'
//! and containers' one walk. A page over a count is **refused in words** before any list of it is made.

use std::collections::{HashMap, HashSet};

use diacraft_doc::{Document, Owner, ShapeRef, ShapeType, SheetRef};

use super::super::callouts;
use super::super::layers;
use super::super::read::Reader;
use super::super::route::P;
use super::super::scan;
use super::super::{MAX_ROUTED_PER_EDIT, Problem};

/// The most shapes a layout lays out at once.
pub const MAX_NODES: usize = 20_000;

/// The most connectors a layout routes again: what one edit routes (`MAX_ROUTED_PER_EDIT`).
pub const MAX_CONNECTORS: usize = MAX_ROUTED_PER_EDIT;

/// The most shapes at the top of a page that are read to find the shapes, the connectors and the rest of it.
pub const MAX_PAGE_SHAPES: usize = 60_000;

/// The most `Connect` rows of a page that are read for the ends of its connectors.
pub const MAX_GLUE_ROWS: usize = 100_000;

/// The most shapes named in the answer as left where they are.
pub const MAX_LISTED: usize = 64;

/// A shape of the scope: where it is and how large, on the page, in drawing units.
#[derive(Clone, Debug)]
pub(super) struct Piece {
    pub id: u32,
    pub at: ShapeRef,
    /// The middle of the box that holds the shape's four corners, and the size of that box.
    pub centre: P,
    pub size: P,
}

impl Piece {
    /// The box as `(x0, y0, x1, y1)`.
    pub(super) fn bounds(&self) -> (f64, f64, f64, f64) {
        (self.centre.0 - self.size.0 / 2.0, self.centre.1 - self.size.1 / 2.0, self.centre.0 + self.size.0 / 2.0, self.centre.1 + self.size.1 / 2.0)
    }
}

/// A connector, and the shapes of the scope its two ends are glued to (the begin, then the end).
#[derive(Clone, Debug)]
pub(super) struct Wire {
    pub id: u32,
    pub ends: [Option<usize>; 2],
    /// The `ID`s of the shapes the two ends are glued to, as the `Connect` rows name them (a shape inside a group is not its group).
    pub raw: [Option<u32>; 2],
}

/// What a layout works on.
#[derive(Debug, Default)]
pub(super) struct Scope {
    /// The shapes laid out, in document order.
    pub nodes: Vec<Piece>,
    /// The connectors that join shapes of the scope or are glued to one: routed again after the shapes move.
    pub wires: Vec<Wire>,
    /// The shapes on a locked layer that are left where they are (the first [`MAX_LISTED`]).
    pub locked: Vec<u32>,
    /// How many shapes named are inside a group and so are left to it.
    pub inside_groups: usize,
    /// How many shapes move along with another of the scope and so are not laid out themselves.
    pub followers: usize,
}

/// Reads the scope: the shapes `ids` of page `page`, or every shape of the page when `ids` is `None`.
pub(super) fn read(doc: &Document, page: u32, ids: Option<&[u32]>) -> Result<Scope, Problem> {
    let p = doc.page(page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
    if p.connects.len() > MAX_GLUE_ROWS {
        return Err(Problem::failed(format!(
            "the page has {} glued ends, more than the {MAX_GLUE_ROWS} a layout reads: lay out part of the diagram",
            p.connects.len()
        )));
    }
    let (tops, inside_groups) = match ids {
        None => (top_of_page(doc, page)?, 0),
        Some(ids) => named_tops(doc, page, ids)?,
    };
    let ends = glue_ends(doc, page);
    let mut scope = Scope { inside_groups, ..Scope::default() };
    let (candidates, wires) = classify(doc, tops, &ends)?;
    let standing = layers::page_standing(doc, page)?;
    let (unlocked, locked): (Vec<Piece>, Vec<Piece>) = candidates.into_iter().partition(|c| !standing.get(&c.id).is_some_and(|s| s.locked));
    scope.locked = locked.iter().take(MAX_LISTED).map(|c| c.id).collect();
    scope.nodes = without_followers(doc, unlocked, &mut scope.followers)?;
    scope.wires = wires_of(doc, page, ids.is_none(), &scope.nodes, &ends, wires)?;
    if scope.wires.len() > MAX_CONNECTORS {
        return Err(too_many_connectors());
    }
    Ok(scope)
}

/// What [`classify`] found: the shapes to lay out, with their boxes, and the connectors with their places.
type Classified = (Vec<Piece>, Vec<(u32, ShapeRef)>);

/// The shapes of `tops` told apart: those that can be laid out, with their boxes, and the connectors (one-dimensional shapes with a glued end). A guide is neither. **The counts are
/// checked as each is found**, so a page of too many is refused before the list of them is any longer than the limit.
fn classify(doc: &Document, tops: Vec<(u32, ShapeRef)>, ends: &HashMap<u32, [Option<u32>; 2]>) -> Result<Classified, Problem> {
    let reader = Reader::new(doc);
    let mut candidates: Vec<Piece> = Vec::new();
    let mut wires: Vec<(u32, ShapeRef)> = Vec::new();
    for (id, at) in tops {
        let Some(shape) = doc.shape(&at) else { continue };
        if shape.kind == ShapeType::Guide {
            continue;
        }
        if reader.is_one_d(&SheetRef::Shape(at.clone())) {
            if ends.contains_key(&id) {
                wires.push((id, at));
                if wires.len() > MAX_CONNECTORS {
                    return Err(too_many_connectors());
                }
            }
            continue;
        }
        if let Some(piece) = piece_of(&reader, id, at) {
            candidates.push(piece);
            if candidates.len() > MAX_NODES {
                return Err(too_many_shapes());
            }
        }
    }
    Ok((candidates, wires))
}

/// The refusal of a page with more shapes to lay out than a layout places.
fn too_many_shapes() -> Problem {
    Problem::failed(format!("the page has more than {MAX_NODES} shapes to lay out, the most a layout places at once: select part of the diagram"))
}

/// The refusal of a page with more connectors to route again than one step routes.
fn too_many_connectors() -> Problem {
    Problem::failed(format!(
        "the page has more than {MAX_CONNECTORS} connectors to route again, the most one step routes: select part of the diagram"
    ))
}

/// Every shape at the top of the page, with its place: refused in words past [`MAX_PAGE_SHAPES`].
fn top_of_page(doc: &Document, page: u32) -> Result<Vec<(u32, ShapeRef)>, Problem> {
    let p = doc.page(page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
    if p.shapes.len() > MAX_PAGE_SHAPES {
        return Err(Problem::failed(format!(
            "the page has {} shapes at its top, more than the {MAX_PAGE_SHAPES} a layout reads: select part of the diagram",
            p.shapes.len()
        )));
    }
    Ok(p.shapes
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id, ShapeRef { owner: Owner::Page(page), path: vec![u32::try_from(i).unwrap_or(u32::MAX)] }))
        .collect())
}

/// The shapes named that are at the top of the page, and how many named are inside a group.
fn named_tops(doc: &Document, page: u32, ids: &[u32]) -> Result<(Vec<(u32, ShapeRef)>, usize), Problem> {
    let (_, found) = scan::named(doc, page, ids)?;
    let inside = found.iter().filter(|(_, at)| at.path.len() != 1).count();
    Ok((found.into_iter().filter(|(_, at)| at.path.len() == 1).collect(), inside))
}

/// The shapes each connector is glued to, by the connector's `ID`: one pass over the `Connect` rows. A row names the end's x cell (`BeginX`, `EndX`).
pub(super) fn glue_ends(doc: &Document, page: u32) -> HashMap<u32, [Option<u32>; 2]> {
    let mut ends: HashMap<u32, [Option<u32>; 2]> = HashMap::new();
    let Some(p) = doc.page(page) else { return ends };
    for c in p.connects.iter() {
        let which = match c.from_cell.as_deref() {
            Some("BeginX") => 0,
            Some("EndX") => 1,
            _ => continue,
        };
        let entry = ends.entry(c.from_sheet).or_insert([None, None]);
        if let Some(slot) = entry.get_mut(which) {
            *slot = Some(c.to_sheet);
        }
    }
    ends
}

/// The shape as the layout sees it: the box that holds its four corners.
fn piece_of(reader: &Reader<'_>, id: u32, at: ShapeRef) -> Option<Piece> {
    let corners = reader.corners(&at);
    if corners.iter().any(|(x, y)| !(x.is_finite() && y.is_finite())) {
        return None;
    }
    let (mut lo, mut hi) = ((f64::INFINITY, f64::INFINITY), (f64::NEG_INFINITY, f64::NEG_INFINITY));
    for (x, y) in corners {
        lo = (lo.0.min(x), lo.1.min(y));
        hi = (hi.0.max(x), hi.1.max(y));
    }
    Some(Piece { id, at, centre: ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0), size: (hi.0 - lo.0, hi.1 - lo.1) })
}

/// The shapes that do not move along with another of them: each shape says which shapes go with it (`callouts::follow_of`: its callouts, and its members if it is a container), and what
/// those name that is also a shape of the list is dropped from it. `followers` counts them. A walk of the page is not made.
fn without_followers(doc: &Document, nodes: Vec<Piece>, followers: &mut usize) -> Result<Vec<Piece>, Problem> {
    let mut follow: HashSet<u32> = HashSet::new();
    for node in &nodes {
        if let Some(shape) = doc.shape(&node.at) {
            follow.extend(callouts::follow_of(shape)?.into_iter().filter(|id| *id != node.id));
        }
    }
    if follow.is_empty() {
        return Ok(nodes);
    }
    let (kept, gone): (Vec<Piece>, Vec<Piece>) = nodes.into_iter().partition(|n| !follow.contains(&n.id));
    *followers = gone.len();
    Ok(kept)
}

/// The connectors of the scope, with the index of the shape each end is glued to (or of the group that holds it). On a page it is every connector of the top of the page; on
/// a selection, those with an end at a shape of the scope, which are found with one walk of the page.
fn wires_of(
    doc: &Document,
    page: u32,
    whole_page: bool,
    nodes: &[Piece],
    ends: &HashMap<u32, [Option<u32>; 2]>,
    on_top: Vec<(u32, ShapeRef)>,
) -> Result<Vec<Wire>, Problem> {
    let index: HashMap<u32, usize> = nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
    let inside = inside_groups(doc, page, nodes, ends, &index)?;
    let to_index = |target: u32| index.get(&target).or_else(|| inside.get(&target)).copied();
    let mut wires: Vec<Wire> = Vec::new();
    let mut seen: HashSet<u32> = HashSet::new();
    let mut take = |id: u32, wire_ends: [Option<u32>; 2]| {
        if seen.insert(id) {
            wires.push(Wire { id, ends: [wire_ends[0].and_then(to_index), wire_ends[1].and_then(to_index)], raw: wire_ends });
        }
    };
    if whole_page {
        for (id, _) in &on_top {
            take(*id, ends.get(id).copied().unwrap_or([None, None]));
        }
    } else {
        // A selection: every connector with an end at a shape of the scope, wherever it is in the page's list.
        let mut found: Vec<(u32, [Option<u32>; 2])> = ends.iter().map(|(id, e)| (*id, *e)).collect();
        found.sort_unstable_by_key(|(id, _)| *id);
        for (id, e) in found {
            if e.iter().flatten().any(|t| to_index(*t).is_some()) {
                take(id, e);
            }
        }
    }
    Ok(wires)
}

/// The shapes inside groups that connectors of `ends` are glued to, and the index (in `nodes`) of the group that holds each: found with one walk of the page, for the
/// targets that are not shapes of the scope themselves. A target inside a shape of the scope is carried by it.
fn inside_groups(
    doc: &Document,
    page: u32,
    nodes: &[Piece],
    ends: &HashMap<u32, [Option<u32>; 2]>,
    index: &HashMap<u32, usize>,
) -> Result<HashMap<u32, usize>, Problem> {
    let groups: HashMap<u32, usize> =
        nodes.iter().enumerate().filter(|(_, n)| doc.shape(&n.at).is_some_and(|s| !s.children.is_empty())).map(|(i, n)| (n.id, i)).collect();
    if groups.is_empty() {
        return Ok(HashMap::new());
    }
    let unknown: HashSet<u32> = ends.values().flatten().flatten().copied().filter(|t| !index.contains_key(t)).collect();
    if unknown.is_empty() {
        return Ok(HashMap::new());
    }
    let found = scan::scan(doc, page, &unknown)?.found;
    let top_ids: Vec<Option<u32>> = doc.page(page).map(|p| p.shapes.iter().map(|s| Some(s.id)).collect()).unwrap_or_default();
    let mut out = HashMap::new();
    for (target, at) in found {
        let Some(top) = at.path.first().and_then(|i| top_ids.get(usize::try_from(*i).ok()?)).copied().flatten() else { continue };
        if let Some(&node) = groups.get(&top) {
            out.insert(target, node);
        }
    }
    Ok(out)
}
