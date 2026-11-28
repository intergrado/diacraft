//! What a page's scene must hold, worked out **apart from the scene**: by the walk of `obstacle.rs` for each top-level shape in page order (every shape it reaches, and which of
//! them are obstacles by the glue and `ObstacleFacts::check`, the first of an `ID` holding it), the glue by one pass over the `Connect` rows, and the legs of every connector by `base_route_within`. The scene's walk, its pointer compare and its updates share nothing with this but the readers they both call.

use std::collections::{BTreeMap, HashSet};

use diacraft_doc::{Document, Owner, ShapeRef, ShapeType, SheetRef};
use diacraft_route::{Point, Rect};

use super::super::Scene;
use crate::draw::hand::base_route_within;
use crate::draw::obstacle::{HiddenLayers, candidates_of, glued_to};
use crate::draw::read::Reader;

/// A connector's glue and legs, by its `ID`.
pub(super) type Wired = BTreeMap<u32, ([Option<u32>; 2], Vec<(Point, Point)>)>;

/// What the scene of a page holds.
#[derive(Debug, Default)]
pub(super) struct Expected {
    /// Each obstacle's box and its two pass flags.
    pub obstacles: BTreeMap<u32, (Rect, bool, bool)>,
    /// Each wired connector's glue and legs.
    pub wired: Wired,
}

const END_CELLS: [&str; 4] = ["BeginX", "BeginY", "EndX", "EndY"];

/// The legs of a route, as the router keeps them: level or upright, of some length, within the page.
fn kept(points: &[(f64, f64)]) -> Vec<(Point, Point)> {
    points.windows(2).map(|w| (Point::new(w[0].0, w[0].1), Point::new(w[1].0, w[1].1))).collect()
}

/// What the scene of page `page` of `doc` holds, with the caps of the scene (`shapes` shapes looked at in all).
pub(super) fn expected(doc: &Document, page: u32, shapes: usize) -> Expected {
    let mut out = Expected::default();
    let Some(p) = doc.page(page) else { return out };
    let reader = Reader::new(doc);
    let hidden = HiddenLayers::of(doc, Owner::Page(page));
    let glued = glued_to(doc, page);
    // The glue of each connector: the first row on either cell of an end.
    let mut from: BTreeMap<u32, [Option<u32>; 2]> = BTreeMap::new();
    for c in p.connects.iter() {
        let end = match c.from_cell.as_deref() {
            Some("BeginX" | "BeginY") => 0,
            Some("EndX" | "EndY") => 1,
            _ => continue,
        };
        if let Some(slot @ None) = from.entry(c.from_sheet).or_default().get_mut(end) {
            *slot = Some(c.to_sheet);
        }
    }
    let ends: HashSet<u32> =
        p.connects.iter().filter(|c| c.from_cell.as_deref().is_some_and(|n| END_CELLS.contains(&n))).map(|c| c.from_sheet).collect();
    let mut budget = shapes;
    let mut seen: HashSet<u32> = HashSet::new();
    let mut held: HashSet<u32> = HashSet::new();
    for (index, shape) in p.shapes.iter().enumerate() {
        if budget == 0 {
            break;
        }
        let at = ShapeRef { owner: Owner::Page(page), path: vec![u32::try_from(index).unwrap()] };
        let sheet = SheetRef::Shape(at.clone());
        // A repeated `ID` is the first shape's.
        if !seen.insert(shape.id) {
            continue;
        }
        // A connector: a one-dimensional shape that is ours or has a glued end.
        if !shape.del && shape.kind != ShapeType::Guide && reader.is_one_d(&sheet) {
            budget -= 1;
            if reader.is_routed(&sheet) || ends.contains(&shape.id) {
                let held = reader.resolver().rows_held(&sheet, &diacraft_doc::SectionKey::new("Geometry", Some(0))).unwrap_or(0);
                let legs = match base_route_within(&reader, &sheet, &reader.pose(&sheet), held) {
                    Ok(Some(points)) => kept(&points),
                    _ => Vec::new(),
                };
                out.wired.entry(shape.id).or_insert((from.get(&shape.id).copied().unwrap_or_default(), legs));
            }
            continue;
        }
        // Every shape the walk reaches, an obstacle or not: **the first holds its `ID`** (a shape that is no obstacle holds it too, as a glue or a layer may make it one without a read).
        let found = candidates_of(&reader, &at, &hidden, &mut budget);
        for facts in found.obstacles {
            if !held.insert(facts.id) || !facts.is_obstacle(glued.contains(&facts.id)) {
                continue;
            }
            out.obstacles.insert(
                facts.id,
                (Rect { x0: facts.bounds.x0, y0: facts.bounds.y0, x1: facts.bounds.x1, y1: facts.bounds.y1 }, facts.pass_x, facts.pass_y),
            );
        }
    }
    out
}

/// The scene holds what the oracle says: the obstacles, and the glue and the legs of the wired connectors that have legs.
pub(super) fn assert_matches(scene: &Scene, doc: &Document, shapes: usize, context: &str) {
    let want = expected(doc, scene.page(), shapes);
    let dump = scene.dump();
    let held: BTreeMap<u32, (Rect, bool, bool)> = dump.candidates.iter().filter_map(|(id, _, _, o)| o.map(|o| (*id, o))).collect();
    assert_eq!(held, want.obstacles, "{context}: the obstacles");
    assert_eq!(dump.obstacle_count, want.obstacles.len(), "{context}: the router holds the obstacles the scene says");
    let wired: Wired = dump.connectors.iter().filter(|c| c.wired).map(|c| (c.id, (c.glue, c.legs.clone()))).collect();
    assert_eq!(wired, want.wired, "{context}: the connectors, their glue and their legs");
}
