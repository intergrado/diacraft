//! What a connector's route goes round: (the connector routing design, section 1.2), and the write that gluing makes.
//!
//! For a connector at the top level of a page, an **obstacle** is a shape of the same page's top level that is
//!
//! 1. **two-dimensional**, not a guide, not deleted from its master, with a box of finite numbers inside [`MAX_COORD`] that has an area;
//! 2. **placeable**: its effective `ObjType`, read by mask, has flag 1 and not 4, **or** holds none of flags 1, 2 and 4 and a connector has an end glued to it
//!    (the 0, "the application chooses, by what the shape is joined to", and the first choice of the Placement behavior list). A group whose `ObjType` holds 8 and not 1
//!    is no obstacle itself, glued or not, and **its members are taken by the same rule**, down to [`MAX_MEMBER_DEPTH`] levels (**design call**: a group of 9 is one obstacle);
//! 3. **on a layer that is shown**, or on no layer (**design call**: a shape the person cannot see bends no route; the hidden).
//!
//! Rule 4, **the exemption** (a shape whose box holds an end of the connector is no obstacle for it), is the router's and the connector's: it is given, which knows
//! the ends. An obstacle is **its box on the page**, level and upright: the box round the four corners of its frame ([`Reader::corners`]), so a turned shape is the box that holds it and an
//! ellipse or a triangle is its box (**design call**).
//!
//! **Routing through**: an obstacle whose effective `ShapePermeableX` is 1 does not block level legs, and one whose `ShapePermeableY` is 1 does not
//! block upright legs ([`ObstacleFacts::pass_x`], [`ObstacleFacts::pass_y`]).
//!
//! **Gluing writes `ObjType` 1** (owner 2026-11-25): a connector glued to a shape that reads none of the flags 1, 2 and 4 writes a local `ObjType` of the flags it
//! held and 1, in the same undo step as the glue ([`make_placeable`]); a shape that reads 1, 2 or 4 is not written, and a cell a `GUARD` protects is left as it is (the glue is made all
//! the same: a refusal would be worse than a shape that is no obstacle).
//!
//! **What it costs.** A shape is read by a fixed number of cells; a walk of a group's members is charged, one for each shape looked at, to the `budget` the caller gives and stops
//! when it is spent, saying so; a page's layers are read once for a call ([`HiddenLayers`]); the shapes a connector is glued to are found by one pass over the page's `Connect` rows
//! ([`glued_to`]).

use std::collections::{HashMap, HashSet};

use diacraft_doc::{CellAddr, CellEdit, CellWrite, Document, Owner, Shape, ShapeRef, ShapeType, SheetRef, Value, WriteOrigin, WriteOutcome};

use diacraft_route::{Step, Work};

use super::layers::{self, Layer};
use super::read::Reader;
use super::route::P;
use super::{MAX_COORD, Problem};

/// How many levels of groups below a top-level group are looked into for obstacles: a group that holds 8 and not 1 gives its members, and theirs, down to this many levels.
/// (Not [`super::MAX_GROUP_DEPTH`], which is how deep a person may nest groups.)
pub(crate) const MAX_MEMBER_DEPTH: usize = 8;

/// `ObjType` flag 1: placeable, an obstacle and a node for layout.
pub(crate) const PLACEABLE: u8 = 1;
/// Flag 2: routable, which must be one-dimensional.
pub(crate) const ROUTABLE: u8 = 2;
/// Flag 4: neither, which outranks 1 and 2.
pub(crate) const NEITHER: u8 = 4;
/// Flag 8: a group whose members are placeable or routable.
pub(crate) const MEMBERS: u8 = 8;

/// What an `ObjType` says of a shape, by mask: `4` outranks `1` and `2`, `8` combines with the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ObjKind {
    /// Flag 1 and not 4: an obstacle, and a node for layout.
    pub placeable: bool,
    /// Flag 2 and not 4: may be a connector (a one-dimensional shape).
    pub routable: bool,
    /// Flag 4: neither a node nor a connector.
    pub neither: bool,
    /// Flag 8: a group whose members are placeable or routable.
    pub members: bool,
    /// None of 1, 2 and 4: the application chooses, by what the shape is joined to.
    pub undecided: bool,
}

/// What the flags of an `ObjType` say.
pub(crate) const fn obj_kind(flags: u8) -> ObjKind {
    let neither = flags & NEITHER != 0;
    ObjKind {
        placeable: flags & PLACEABLE != 0 && !neither,
        routable: flags & ROUTABLE != 0 && !neither,
        neither,
        members: flags & MEMBERS != 0,
        undecided: flags & (PLACEABLE | ROUTABLE | NEITHER) == 0,
    }
}

/// The flags a cell's number holds: a whole number of a byte (`PtgByte`), rounded to the nearest, and 0 for anything else (not a number, below 0 or above 255).
pub(crate) fn flags_of(value: f64) -> u8 {
    let rounded = value.round();
    if rounded.is_finite() && (0.0..=255.0).contains(&rounded) { rounded as u8 } else { 0 }
}

/// A box on the page, level and upright, `x` right and `y` up: the box round the corners of a shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bounds {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

/// The box round four corners, or `None` when a number is not finite or lies beyond [`MAX_COORD`] or the box has no area.
fn bounds_of(corners: [P; 4]) -> Option<Bounds> {
    let mut b = Bounds { x0: f64::INFINITY, y0: f64::INFINITY, x1: f64::NEG_INFINITY, y1: f64::NEG_INFINITY };
    for (x, y) in corners {
        if !(x.is_finite() && y.is_finite() && x.abs() <= MAX_COORD && y.abs() <= MAX_COORD) {
            return None;
        }
        (b.x0, b.y0, b.x1, b.y1) = (b.x0.min(x), b.y0.min(y), b.x1.max(x), b.y1.max(y));
    }
    (b.x1 > b.x0 && b.y1 > b.y0).then_some(b)
}

/// Why a shape is no obstacle, in the words `shape.behavior` gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NotObstacle {
    /// Deleted from its master (`Del`).
    Deleted,
    /// One-dimensional: a connector or a line.
    OneD,
    /// A guide.
    Guide,
    /// A box that is not finite, lies beyond the page's reach or has no area.
    NoBox,
    /// `ObjType` holds 4: neither.
    Neither,
    /// A group that holds 8 and not 1: its members are the obstacles, not the group.
    GroupMembers,
    /// `ObjType` holds 2 and not 1: routable, and no node.
    Routable,
    /// `ObjType` holds none of 1, 2 and 4 and no connector is glued to the shape.
    NotPlaced,
    /// Every layer the shape is on is hidden.
    Hidden,
}

impl NotObstacle {
    /// The reason, for the person and for the program that asks.
    pub(crate) fn words(self) -> &'static str {
        match self {
            NotObstacle::Deleted => "it is deleted from its master",
            NotObstacle::OneD => "it is a connector or a line, which a route does not go round",
            NotObstacle::Guide => "it is a guide, an aid and no shape",
            NotObstacle::NoBox => "its box is empty or lies beyond the page's reach",
            NotObstacle::Neither => "its placement behavior is Do not lay out and route around",
            NotObstacle::GroupMembers => {
                "it is a group whose members are placed and not the group (ObjType 8 without 1): its members are the obstacles"
            }
            NotObstacle::Routable => "it is marked routable and not placeable (ObjType 2)",
            NotObstacle::NotPlaced => "its placement behavior is Let the application decide and no connector is glued to it",
            NotObstacle::Hidden => "every layer it is on is hidden",
        }
    }
}

/// What the router needs of a shape that may be an obstacle: the candidate the scene keeps, whose placeability depends on whether a connector is glued to it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ObstacleFacts {
    pub id: u32,
    /// Its box on the page.
    pub bounds: Bounds,
    /// The flags of its effective `ObjType`.
    pub flags: u8,
    pub kind: ObjKind,
    /// `ShapePermeableX` is 1: level legs go through it.
    pub pass_x: bool,
    /// `ShapePermeableY` is 1: upright legs go through it.
    pub pass_y: bool,
    /// Every layer it is on is hidden (a shape on no layer is not).
    pub hidden: bool,
}

impl ObstacleFacts {
    /// Whether the shape is an obstacle when a connector is glued to it (`glued`) or none is: rules 2 and 3, or the reason it is none.
    pub(crate) fn check(&self, glued: bool) -> Result<(), NotObstacle> {
        let kind = self.kind;
        if kind.neither {
            Err(NotObstacle::Neither)
        } else if kind.members && !kind.placeable {
            Err(NotObstacle::GroupMembers)
        } else if !(kind.placeable || kind.undecided && glued) {
            Err(if kind.routable { NotObstacle::Routable } else { NotObstacle::NotPlaced })
        } else if self.hidden {
            Err(NotObstacle::Hidden)
        } else {
            Ok(())
        }
    }

    /// Whether the shape is an obstacle when `glued` says whether a connector is glued to it.
    pub(crate) fn is_obstacle(&self, glued: bool) -> bool {
        self.check(glued).is_ok()
    }
}

/// The visibility of a page's layers, read once for a call: the hidden (a shape that lists layers is hidden when every one it lists is).
#[derive(Debug, Default)]
pub(crate) struct HiddenLayers {
    /// Each layer's `IX` and whether it is shown.
    shown: HashMap<u32, bool>,
    any_hidden: bool,
}

impl HiddenLayers {
    /// The layers of a page (a master's, a page that is not there and a table of more than [`layers::MAX_LAYERS`] rows have none: they hide nothing).
    pub(crate) fn of(doc: &Document, owner: Owner) -> HiddenLayers {
        let Owner::Page(page) = owner else { return HiddenLayers::default() };
        let table: Vec<Layer> = layers::read_layers(doc, page).unwrap_or_default();
        HiddenLayers { any_hidden: table.iter().any(|l| !l.visible), shown: table.iter().map(|l| (l.ix, l.visible)).collect() }
    }

    /// Whether the layers the shape is on (its own `LayerMember`) are all hidden.
    pub(crate) fn hides(&self, shape: &Shape) -> bool {
        if !self.any_hidden {
            return false;
        }
        let mut on_one = false;
        for ix in layers::shape_layers(shape) {
            match self.shown.get(&ix) {
                Some(true) => return false,
                Some(false) => on_one = true,
                // A list that names no row of the table names no layer.
                None => {}
            }
        }
        on_one
    }
}

/// Reads shape `at`, **whatever is glued to it**: its box, flags, pass cells and layers; or why it can never be an obstacle (a connector or a line, a guide, a deleted shape, a box that
/// is no box). [`ObstacleFacts::check`] then says whether it is one now.
pub(crate) fn read_candidate(reader: &Reader<'_>, at: &ShapeRef, layers: &HiddenLayers) -> Result<ObstacleFacts, NotObstacle> {
    let shape = reader.doc().shape(at).ok_or(NotObstacle::NoBox)?;
    if shape.del {
        return Err(NotObstacle::Deleted);
    }
    if shape.kind == ShapeType::Guide {
        return Err(NotObstacle::Guide);
    }
    let sheet = SheetRef::Shape(at.clone());
    if reader.is_one_d(&sheet) {
        return Err(NotObstacle::OneD);
    }
    let bounds = bounds_of(reader.corners(at)).ok_or(NotObstacle::NoBox)?;
    let flags = flags_of(reader.number(&sheet, "ObjType"));
    Ok(ObstacleFacts {
        id: shape.id,
        bounds,
        flags,
        kind: obj_kind(flags),
        pass_x: reader.number(&sheet, "ShapePermeableX") != 0.0,
        pass_y: reader.number(&sheet, "ShapePermeableY") != 0.0,
        hidden: layers.hides(shape),
    })
}

/// Shape `at` as an obstacle when `glued` says a connector is glued to it: its facts, or `None` when it is none (for the shape alone: a group's
/// members are [`obstacles_of`]'s).
#[allow(dead_code, reason = "read by the router's caller, which is not built yet")]
pub(crate) fn read_obstacle(reader: &Reader<'_>, at: &ShapeRef, glued: bool) -> Option<ObstacleFacts> {
    let layers = HiddenLayers::of(reader.doc(), at.owner);
    read_candidate(reader, at, &layers).ok().filter(|facts| facts.is_obstacle(glued))
}

/// What [`obstacles_of`] and [`candidates_below`] found.
#[derive(Debug, Default)]
pub(crate) struct Found {
    /// The shapes, in document order.
    pub obstacles: Vec<ObstacleFacts>,
    /// The read stopped because `budget` was spent: there are shapes it did not look at.
    pub cut: bool,
    /// The read stopped because the work it was given ([`candidates_below`]) was refused a step: there are shapes it did not look at, and **what it found is not what a read would find**. A cut
    /// of `budget` is a rule of the page and this is not: the caller's work ran out.
    pub refused: bool,
}

/// The obstacles of a top-level shape `top`: the shape itself, or, for a group that holds 8 and not 1, its members by the same rule down to [`MAX_MEMBER_DEPTH`] levels (a group of 9
/// is one obstacle and its members are not looked at). `glued` says whether a connector is glued to a shape by its `ID`. Each shape looked at takes one from `budget`; when it is spent the
/// read stops and says so.
///
/// **What it holds.** The groups being read are an explicit stack of the group and the next member to look at, at most [`MAX_MEMBER_DEPTH`] of them whatever a group holds: a group of
/// 200,000 members read with a budget of 50 makes 50 references to members, and not 200,000 first.
#[allow(dead_code, reason = "the oracle of the kept scene's tests and the router's caller, which is not built yet")]
pub(crate) fn obstacles_of(reader: &Reader<'_>, top: &ShapeRef, layers: &HiddenLayers, glued: &dyn Fn(u32) -> bool, budget: &mut usize) -> Found {
    walk(reader, top, None, layers, &|facts| facts.is_obstacle(glued(facts.id)), budget, None)
}

/// Every two-dimensional shape [`obstacles_of`] looks at, **whether or not it is an obstacle now**: the shapes of the same walk that have a box, in document order, each with the
/// facts that decide it ([`ObstacleFacts::check`]). The kept scene holds these, so that a connector glued to a shape or a layer shown changes what is an obstacle with no read of the
/// shape (`route_scene.rs`). The budget and the groups are [`obstacles_of`]'s; this is for a top-level shape the caller has read already.
/// `facts` are [`read_candidate`]'s for `top`: the shape is not read again, and still takes one from `budget`. **Each member is charged to `work` before it is read** (one step, the shape's own
/// look having been charged by the caller who read it), and a step that is refused stops the walk with [`Found::refused`]: a walk given one step reads one member, however many the group holds.
pub(crate) fn candidates_below(
    reader: &Reader<'_>,
    top: &ShapeRef,
    facts: ObstacleFacts,
    layers: &HiddenLayers,
    budget: &mut usize,
    work: &mut Work,
) -> Found {
    walk(reader, top, Some(facts), layers, &|_| true, budget, Some(work))
}

/// What [`candidates_below`] keeps for the top-level shape `top`, read here with the rest (no facts given, no work charged): **every** two-dimensional shape of the walk, an obstacle or not, in
/// document order. The oracle of the kept scene's tests holds the scene to it, and to the rule that the first such shape holds an `ID`, obstacle or not.
#[cfg(test)]
pub(crate) fn candidates_of(reader: &Reader<'_>, top: &ShapeRef, layers: &HiddenLayers, budget: &mut usize) -> Found {
    walk(reader, top, None, layers, &|_| true, budget, None)
}

/// The walk of [`obstacles_of`] and [`candidates_below`]: the shapes it reaches that `keep` takes. `known` is the facts of `top` where the caller has them. With a `work`, every shape that is
/// read (every one but a `top` that is `known`) is charged one step of it first, after the budget has said there is room for it, so that a refusal reads nothing more.
fn walk(
    reader: &Reader<'_>,
    top: &ShapeRef,
    mut known: Option<ObstacleFacts>,
    layers: &HiddenLayers,
    keep: &dyn Fn(&ObstacleFacts) -> bool,
    budget: &mut usize,
    mut work: Option<&mut Work>,
) -> Found {
    let mut found = Found::default();
    let mut open: Vec<(ShapeRef, usize)> = Vec::new();
    let mut next = Some(top.clone());
    loop {
        if let Some(at) = next.take() {
            if *budget == 0 {
                found.cut = true;
                break;
            }
            if known.is_none() && work.as_deref_mut().is_some_and(|work| !work.charge_for(Step::Visit, 1)) {
                found.refused = true;
                break;
            }
            *budget -= 1;
            let read = match known.take() {
                Some(facts) => Ok(facts),
                None => read_candidate(reader, &at, layers),
            };
            if let Ok(facts) = read {
                if facts.kind.members && !facts.kind.placeable {
                    // The members of a group at level 0 are level 1, and so on: those at level `MAX_MEMBER_DEPTH` are read, theirs are not.
                    if open.len() < MAX_MEMBER_DEPTH {
                        open.push((at, 0));
                    }
                } else if keep(&facts) {
                    found.obstacles.push(facts);
                }
            }
        }
        // The next member of the innermost group that has one, or the end of the walk.
        let Some((group, cursor)) = open.last_mut() else { break };
        let member = *cursor;
        *cursor = cursor.saturating_add(1);
        match member_at(reader, group, member) {
            Some(at) => next = Some(at),
            None => {
                open.pop();
            }
        }
    }
    found
}

// How many references to members were made, for the test that holds the read of a huge group to its budget. Not kept outside tests.
#[cfg(test)]
thread_local! {
    pub(crate) static MEMBER_REFS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Member number `member` of the group at `group`, if it has one.
fn member_at(reader: &Reader<'_>, group: &ShapeRef, member: usize) -> Option<ShapeRef> {
    reader.doc().shape(group)?.children.get(member)?;
    let mut path = group.path.clone();
    path.push(u32::try_from(member).ok()?);
    #[cfg(test)]
    MEMBER_REFS.with(|c| c.set(c.get() + 1));
    Some(ShapeRef { owner: group.owner, path })
}

/// The cells that are the ends of a connector.
const END_CELLS: [&str; 4] = ["BeginX", "BeginY", "EndX", "EndY"];

/// The shapes of page `page` that a connector has an end glued to: one pass over the page's `Connect` rows (the row of an end cell, from any shape: a connector is a shape with a
/// glued end), the shape glued to itself left out.
pub(crate) fn glued_to(doc: &Document, page: u32) -> HashSet<u32> {
    doc.page(page)
        .map(|p| {
            p.connects
                .iter()
                .filter(|c| c.to_sheet != c.from_sheet && c.from_cell.as_deref().is_some_and(|cell| END_CELLS.contains(&cell)))
                .map(|c| c.to_sheet)
                .collect()
        })
        .unwrap_or_default()
}

/// Owner, 2026-11-25. Makes shape `target` of page `page` placeable because a connector is glued to it: a two-dimensional shape that reads none of the flags 1, 2 and 4
/// gets a local `ObjType` of the flags it holds (a group's 8 among them) and 1. A shape that holds one of them, a connector or line, a guide and a deleted shape are not written, and nor is
/// one whose `ObjType` a `GUARD` protects. → whether a cell was written. The edit is the caller's: one undo step with the glue.
pub(crate) fn make_placeable(doc: &mut Document, page: u32, target: u32) -> Result<bool, Problem> {
    let Some(at) = doc.find_shape(Owner::Page(page), target) else { return Ok(false) };
    Ok(make_all_placeable(doc, &[at])? > 0)
}

/// [`make_placeable`] for the shapes at `targets` (found already, each once): one read of the document and one batch of writes. → how many cells were written.
pub(crate) fn make_all_placeable(doc: &mut Document, targets: &[ShapeRef]) -> Result<usize, Problem> {
    let mut writes: Vec<(SheetRef, CellWrite)> = Vec::new();
    {
        let reader = Reader::new(doc);
        for at in targets {
            let sheet = SheetRef::Shape(at.clone());
            let Some(shape) = doc.shape(at) else { continue };
            if shape.del || shape.kind == ShapeType::Guide || reader.is_one_d(&sheet) {
                continue;
            }
            let flags = flags_of(reader.number(&sheet, "ObjType"));
            if obj_kind(flags).undecided {
                let edit = CellEdit::Value(Value::from_text(&(flags | PLACEABLE).to_string()));
                writes.push((sheet, CellWrite { addr: CellAddr::direct("ObjType"), edit, origin: WriteOrigin::Drawing }));
            }
        }
    }
    if writes.is_empty() {
        return Ok(0);
    }
    // A cell a `GUARD` protects is kept (`KeptGuarded`): the glue is made all the same.
    let outcomes = doc.write_batch(writes)?;
    Ok(outcomes.iter().filter(|outcome| **outcome == WriteOutcome::Written).count())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::sync::Arc;

    use diacraft_doc::{Cell, CellName, Connect, Connects, ShapeList, Sheet};

    use super::super::layers::{Settings, add_layer, set_layer};
    use super::super::page_mut;
    use super::*;

    fn number(sheet: &mut Sheet, name: &str, value: f64) {
        sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_number(value).unwrap()));
    }

    /// A shape whose box is `w` by `h` with its lower left corner at (`x`, `y`) in its parent's frame, with the cells `cells` as well.
    fn boxed(id: u32, (x, y, w, h): (f64, f64, f64, f64), cells: &[(&str, f64)]) -> Shape {
        let mut sheet = Sheet::default();
        for (name, value) in [("PinX", x + w / 2.0), ("PinY", y + h / 2.0), ("Width", w), ("Height", h), ("LocPinX", w / 2.0), ("LocPinY", h / 2.0)] {
            number(&mut sheet, name, value);
        }
        for (name, value) in cells {
            number(&mut sheet, name, *value);
        }
        Shape { sheet: Arc::new(sheet), ..Shape::new(id) }
    }

    fn group(id: u32, (x, y, w, h): (f64, f64, f64, f64), objtype: f64, members: Vec<Shape>) -> Shape {
        // A group's frame has its local pin at its lower left corner, so that a member's own place is its place inside the group.
        let mut g = boxed(id, (x, y, w, h), &[("ObjType", objtype), ("LocPinX", 0.0), ("LocPinY", 0.0), ("PinX", x), ("PinY", y)]);
        g.children = ShapeList::from_vec(members.into_iter().map(Arc::new).collect());
        g
    }

    fn doc_of(shapes: Vec<Shape>) -> Document {
        let mut doc = Document::new_blank();
        let page = page_mut(&mut doc, 1).unwrap();
        for shape in shapes {
            page.shapes.push(Arc::new(shape));
        }
        doc
    }

    fn at(index: u32) -> ShapeRef {
        ShapeRef { owner: Owner::Page(1), path: vec![index] }
    }

    fn found(doc: &Document, top: u32, glued: &[u32]) -> Vec<u32> {
        let reader = Reader::new(doc);
        let layers = HiddenLayers::of(doc, Owner::Page(1));
        let glued: HashSet<u32> = glued.iter().copied().collect();
        let mut budget = 1_000;
        obstacles_of(&reader, &at(top), &layers, &|id| glued.contains(&id), &mut budget).obstacles.iter().map(|f| f.id).collect()
    }

    #[test]
    fn every_byte_is_read_by_mask_and_four_outranks_one_and_two() {
        for flags in 0..=255u8 {
            let kind = obj_kind(flags);
            assert_eq!(kind.placeable, flags & 1 != 0 && flags & 4 == 0, "{flags}: placeable");
            assert_eq!(kind.routable, flags & 2 != 0 && flags & 4 == 0, "{flags}: routable");
            assert_eq!(kind.neither, flags & 4 != 0, "{flags}: neither");
            assert_eq!(kind.members, flags & 8 != 0, "{flags}: members");
            assert_eq!(kind.undecided, flags & 7 == 0, "{flags}: undecided");
        }
        // The cases: 10 is a connector with the group flag noted, 6 is none.
        assert!(obj_kind(10).routable && obj_kind(10).members && !obj_kind(10).placeable);
        assert!(!obj_kind(6).routable && obj_kind(6).neither);
        // The flags no list names (16, 32, ...) do not disturb the four.
        assert_eq!(obj_kind(17), ObjKind { placeable: true, routable: false, neither: false, members: false, undecided: false });
        assert_eq!(obj_kind(16), ObjKind { placeable: false, routable: false, neither: false, members: false, undecided: true });
    }

    #[test]
    fn a_number_is_flags_only_when_it_is_a_whole_number_of_a_byte() {
        for (value, want) in [
            (0.0, 0),
            (1.0, 1),
            (9.0, 9),
            (255.0, 255),
            (1.0000000001, 1),
            (0.9999999999, 1),
            (255.4, 255),
            (256.0, 0),
            (-1.0, 0),
            (f64::NAN, 0),
            (f64::INFINITY, 0),
            (f64::NEG_INFINITY, 0),
            (1.0e300, 0),
        ] {
            assert_eq!(flags_of(value), want, "{value}");
        }
    }

    #[test]
    fn a_group_of_8_gives_its_members_and_a_group_of_9_is_one() {
        let members =
            |first: u32| vec![boxed(first, (1.0, 1.0, 1.0, 1.0), &[("ObjType", 1.0)]), boxed(first + 1, (3.0, 1.0, 1.0, 1.0), &[("ObjType", 1.0)])];
        let doc = doc_of(vec![
            group(10, (0.0, 0.0, 6.0, 3.0), 8.0, members(11)),
            group(20, (10.0, 0.0, 6.0, 3.0), 9.0, members(21)),
            group(30, (20.0, 0.0, 6.0, 3.0), 12.0, members(31)),
            group(40, (30.0, 0.0, 6.0, 3.0), 4.0, members(41)),
        ]);
        assert_eq!(found(&doc, 0, &[]), [11, 12], "8: the members, in order, and not the group");
        assert_eq!(found(&doc, 1, &[]), [20], "9: the group is one obstacle, its members are not looked at");
        assert_eq!(found(&doc, 2, &[]), [31, 32], "12 holds 8 and not 1: its members");
        assert_eq!(found(&doc, 3, &[]), Vec::<u32>::new(), "4 without 8: neither, and its members are no business of a route");
        // A member of a group is its box on the page: the group's place and the member's.
        let reader = Reader::new(&doc);
        let mut budget = 10;
        let facts = obstacles_of(&reader, &at(1), &HiddenLayers::default(), &|_| false, &mut budget).obstacles;
        assert_eq!(facts[0].bounds, Bounds { x0: 10.0, y0: 0.0, x1: 16.0, y1: 3.0 }, "a group is its own box");
        let mut budget = 10;
        let facts = obstacles_of(&reader, &at(0), &HiddenLayers::default(), &|_| false, &mut budget).obstacles;
        assert_eq!(facts[1].bounds, Bounds { x0: 3.0, y0: 1.0, x1: 4.0, y1: 2.0 }, "a member is its box through the group's frame");
    }

    #[test]
    fn a_group_of_8_that_is_glued_is_still_no_obstacle_itself() {
        let doc = doc_of(vec![group(10, (0.0, 0.0, 6.0, 3.0), 8.0, vec![boxed(11, (1.0, 1.0, 1.0, 1.0), &[("ObjType", 1.0)])])]);
        assert_eq!(found(&doc, 0, &[10]), [11], "Glued or not");
        let facts = read_candidate(&Reader::new(&doc), &at(0), &HiddenLayers::default()).unwrap();
        assert_eq!(facts.check(true), Err(NotObstacle::GroupMembers));
    }

    #[test]
    fn members_deeper_than_the_depth_are_not_looked_at() {
        // A chain of groups that hold 8, the last holding a placeable box: the box is `groups` levels below the top-level shape.
        let chain = |groups: usize| {
            let mut inner = vec![boxed(100, (1.0, 1.0, 1.0, 1.0), &[("ObjType", 1.0)])];
            for level in (0..groups).rev() {
                inner = vec![group(u32::try_from(level).unwrap() + 1, (0.0, 0.0, 10.0, 10.0), 8.0, inner)];
            }
            doc_of(inner)
        };
        // The top group is level 0 and its member level 1: a box at level 8 is taken, one at level 9 is not.
        assert_eq!(found(&chain(8), 0, &[]), [100], "eight groups: the box is at level 8");
        assert_eq!(found(&chain(9), 0, &[]), Vec::<u32>::new(), "nine groups: the box is past the depth");
        assert_eq!(MAX_MEMBER_DEPTH, 8);
    }

    #[test]
    fn a_shape_that_reads_zero_is_an_obstacle_only_when_a_connector_is_glued_to_it() {
        let doc = doc_of(vec![
            boxed(1, (0.0, 0.0, 1.0, 1.0), &[]),
            boxed(2, (0.0, 0.0, 1.0, 1.0), &[("ObjType", 1.0)]),
            boxed(3, (0.0, 0.0, 1.0, 1.0), &[("ObjType", 4.0)]),
            boxed(4, (0.0, 0.0, 1.0, 1.0), &[("ObjType", 2.0)]),
            boxed(5, (0.0, 0.0, 1.0, 1.0), &[("ObjType", 5.0)]),
            boxed(6, (0.0, 0.0, 1.0, 1.0), &[("ObjType", 16.0)]),
            boxed(7, (0.0, 0.0, 1.0, 1.0), &[("ObjType", 3.0)]),
        ]);
        let reader = Reader::new(&doc);
        let check = |index: u32, glued: bool| {
            let facts = read_candidate(&reader, &at(index), &HiddenLayers::default()).unwrap();
            assert_eq!(read_obstacle(&reader, &at(index), glued).is_some(), facts.check(glued).is_ok());
            facts.check(glued)
        };
        assert_eq!((check(0, false), check(0, true)), (Err(NotObstacle::NotPlaced), Ok(())), "0: by the glue");
        assert_eq!((check(1, false), check(1, true)), (Ok(()), Ok(())), "1: always");
        assert_eq!((check(2, false), check(2, true)), (Err(NotObstacle::Neither), Err(NotObstacle::Neither)), "4: never");
        assert_eq!((check(3, false), check(3, true)), (Err(NotObstacle::Routable), Err(NotObstacle::Routable)), "2: routable, no node");
        assert_eq!(
            (check(4, true), check(5, false), check(5, true)),
            (Err(NotObstacle::Neither), Err(NotObstacle::NotPlaced), Ok(())),
            "5 is 4; 16 is none of the three"
        );
        assert_eq!((check(6, false), check(6, true)), (Ok(()), Ok(())), "3 holds 1 and 2: placeable");
        // The words are the answer of `shape.behavior`.
        assert_eq!(NotObstacle::NotPlaced.words(), "its placement behavior is Let the application decide and no connector is glued to it");
    }

    #[test]
    fn the_pass_cells_are_read_one_for_each_direction() {
        let doc = doc_of(vec![
            boxed(1, (0.0, 0.0, 1.0, 1.0), &[("ObjType", 1.0), ("ShapePermeableX", 1.0)]),
            boxed(2, (0.0, 0.0, 1.0, 1.0), &[("ObjType", 1.0), ("ShapePermeableY", 1.0)]),
        ]);
        let reader = Reader::new(&doc);
        let x = read_obstacle(&reader, &at(0), false).unwrap();
        let y = read_obstacle(&reader, &at(1), false).unwrap();
        assert_eq!((x.pass_x, x.pass_y, y.pass_x, y.pass_y), (true, false, false, true), "Neither flag is read as both");
    }

    #[test]
    fn a_guide_and_a_one_d_shape_are_no_obstacle() {
        let mut guide = boxed(1, (0.0, 0.0, 1.0, 1.0), &[("ObjType", 1.0)]);
        guide.kind = ShapeType::Guide;
        let line = boxed(2, (0.0, 0.0, 1.0, 1.0), &[("ObjType", 1.0), ("BeginX", 0.0), ("BeginY", 0.0), ("EndX", 1.0), ("EndY", 1.0)]);
        let doc = doc_of(vec![guide, line, boxed(3, (0.0, 0.0, 1.0, 1.0), &[("ObjType", 1.0)])]);
        let reader = Reader::new(&doc);
        let why = |index: u32| read_candidate(&reader, &at(index), &HiddenLayers::default()).err();
        assert_eq!([why(0), why(1), why(2)], [Some(NotObstacle::Guide), Some(NotObstacle::OneD), None]);
        // Glued or not, placeable or not: they never are.
        assert!(read_obstacle(&reader, &at(0), true).is_none() && read_obstacle(&reader, &at(1), true).is_none());
    }

    #[test]
    fn a_deleted_shape_and_a_shape_with_no_box_are_no_obstacle() {
        let mut deleted = boxed(1, (0.0, 0.0, 1.0, 1.0), &[("ObjType", 1.0)]);
        deleted.del = true;
        let doc = doc_of(vec![
            deleted,
            boxed(2, (0.0, 0.0, 0.0, 1.0), &[("ObjType", 1.0)]),
            boxed(3, (0.0, 0.0, 1.0e300, 1.0), &[("ObjType", 1.0)]),
            boxed(4, (2.0e6, 0.0, 1.0, 1.0), &[("ObjType", 1.0)]),
            boxed(5, (0.0, 0.0, 1.0, 1.0), &[("ObjType", 1.0)]),
        ]);
        let reader = Reader::new(&doc);
        let why = |index: u32| read_candidate(&reader, &at(index), &HiddenLayers::default()).err();
        assert_eq!(
            [why(0), why(1), why(2), why(3), why(4)],
            [Some(NotObstacle::Deleted), Some(NotObstacle::NoBox), Some(NotObstacle::NoBox), Some(NotObstacle::NoBox), None]
        );
    }

    #[test]
    fn a_turned_shape_is_the_box_that_holds_it() {
        let doc = doc_of(vec![boxed(1, (4.0, 4.0, 2.0, 1.0), &[("ObjType", 1.0), ("Angle", std::f64::consts::FRAC_PI_4)])]);
        let b = read_obstacle(&Reader::new(&doc), &at(0), false).unwrap().bounds;
        let half = 1.5 * std::f64::consts::FRAC_1_SQRT_2;
        for (got, want) in [(b.x0, 5.0 - half), (b.y0, 4.5 - half), (b.x1, 5.0 + half), (b.y1, 4.5 + half)] {
            assert!((got - want).abs() < 1.0e-9, "{b:?}");
        }
    }

    #[test]
    fn a_hidden_layer_hides_an_obstacle() {
        let mut doc = doc_of(vec![
            boxed(1, (0.0, 0.0, 1.0, 1.0), &[("ObjType", 1.0)]),
            boxed(2, (0.0, 0.0, 1.0, 1.0), &[("ObjType", 1.0)]),
            boxed(3, (0.0, 0.0, 1.0, 1.0), &[("ObjType", 1.0)]),
            boxed(4, (0.0, 0.0, 1.0, 1.0), &[("ObjType", 1.0)]),
        ]);
        for name in ["Shown", "Hidden"] {
            add_layer(&mut doc, 1, &Settings { name: Some(name.to_owned()), ..Settings::default() }).unwrap();
        }
        set_layer(&mut doc, 1, 1, &Settings { visible: Some(false), ..Settings::default() }).unwrap();
        let mut members = |index: usize, list: &str| {
            let page = page_mut(&mut doc, 1).unwrap();
            let shape = Arc::make_mut(page.shapes.get_mut(index).unwrap());
            Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("LayerMember"), Cell::constant(Value::from_text(list)));
        };
        members(0, "1"); // hidden alone
        members(1, "0;1"); // one of its layers is shown
        members(2, "7"); // names no layer
        let reader = Reader::new(&doc);
        let layers = HiddenLayers::of(&doc, Owner::Page(1));
        let hidden = |index: u32| read_candidate(&reader, &at(index), &layers).unwrap().hidden;
        assert_eq!([hidden(0), hidden(1), hidden(2), hidden(3)], [true, false, false, false], "Hidden when it lists layers and every one is");
        assert_eq!(read_candidate(&reader, &at(0), &layers).unwrap().check(false), Err(NotObstacle::Hidden));
        // And it is the rule `standing_of` states, for every list of the table.
        let table = layers::read_layers(&doc, 1).unwrap();
        let by_ix: HashMap<u32, &Layer> = table.iter().map(|l| (l.ix, l)).collect();
        for list in [vec![], vec![0], vec![1], vec![0, 1], vec![1, 9], vec![9]] {
            let mut shape = Shape::new(1);
            let text = list.iter().map(u32::to_string).collect::<Vec<_>>().join(";");
            let mut sheet = Sheet::default();
            sheet.cells.insert(CellName::new("LayerMember"), Cell::constant(Value::from_text(&text)));
            shape.sheet = Arc::new(sheet);
            assert_eq!(layers.hides(&shape), layers::standing_of(&list, &by_ix).hidden, "{list:?}");
        }
    }

    /// With a work, each member is charged one step **before** it is read: the steps given are the members read, a step refused stops the walk, and the budget's cut and the work's refusal are
    /// told apart.
    #[test]
    fn the_walk_of_the_members_charges_each_to_the_work_before_it_reads_it() {
        let members: Vec<Shape> = (0..50).map(|i| boxed(100 + i, (f64::from(i), 0.0, 1.0, 1.0), &[("ObjType", 1.0)])).collect();
        let doc = doc_of(vec![group(1, (0.0, 0.0, 60.0, 1.0), 8.0, members)]);
        let reader = Reader::new(&doc);
        let layers = HiddenLayers::default();
        let top = read_candidate(&reader, &at(0), &layers).unwrap();
        // Four steps for the members: four read, and the fifth refused before it is read.
        let (mut budget, mut work) = (100, Work::new(4));
        MEMBER_REFS.with(|c| c.set(0));
        let found = candidates_below(&reader, &at(0), top, &layers, &mut budget, &mut work);
        assert_eq!(
            (found.obstacles.len(), found.refused, found.cut, budget, work.spent()),
            (4, true, false, 95, 4),
            "the group's look and four members' were taken from the budget"
        );
        assert!(MEMBER_REFS.with(std::cell::Cell::get) <= 6, "no reference to a member past the one that was refused");
        // No step at all: no member read, and it is the work's refusal.
        let (mut budget, mut work) = (100, Work::new(0));
        let found = candidates_below(&reader, &at(0), top, &layers, &mut budget, &mut work);
        assert_eq!((found.obstacles.len(), found.refused, found.cut, budget), (0, true, false, 99));
        // A work that was spent already refuses the first member too.
        let (mut budget, mut work) = (100, Work::new(1));
        assert!(work.charge(1) && !work.charge(1));
        assert!(candidates_below(&reader, &at(0), top, &layers, &mut budget, &mut work).refused);
        // A budget that is spent first is a cut, and costs the steps of the members it let through.
        let (mut budget, mut work) = (11, Work::new(1_000));
        let found = candidates_below(&reader, &at(0), top, &layers, &mut budget, &mut work);
        assert_eq!((found.obstacles.len(), found.cut, found.refused, budget, work.spent()), (10, true, false, 0, 10));
        // Exactly the steps the members need: neither.
        let (mut budget, mut work) = (51, Work::new(50));
        let found = candidates_below(&reader, &at(0), top, &layers, &mut budget, &mut work);
        assert_eq!((found.obstacles.len(), found.cut, found.refused, work.spent()), (50, false, false, 50));
        // The same walk with no work to charge (`obstacles_of`) is never refused.
        let mut budget = 11;
        assert!(!obstacles_of(&reader, &at(0), &layers, &|_| true, &mut budget).refused);
    }

    #[test]
    fn the_walk_of_the_members_is_charged_and_stops_with_its_budget() {
        let members: Vec<Shape> = (0..50).map(|i| boxed(100 + i, (f64::from(i), 0.0, 1.0, 1.0), &[("ObjType", 1.0)])).collect();
        let doc = doc_of(vec![group(1, (0.0, 0.0, 60.0, 1.0), 8.0, members)]);
        let reader = Reader::new(&doc);
        let mut budget = 11;
        let read = obstacles_of(&reader, &at(0), &HiddenLayers::default(), &|_| false, &mut budget);
        assert_eq!((read.obstacles.len(), read.cut, budget), (10, true, 0), "the group and ten members were looked at, and the rest was not");
        let mut budget = 51;
        let read = obstacles_of(&reader, &at(0), &HiddenLayers::default(), &|_| false, &mut budget);
        assert_eq!((read.obstacles.len(), read.cut, budget), (50, false, 0), "exactly enough");
        let mut budget = 0;
        assert!(obstacles_of(&reader, &at(0), &HiddenLayers::default(), &|_| false, &mut budget).cut);
    }

    /// A group of 200,000 members read with a budget of 50 looks at 50 shapes and makes no more references to members than that (the groups open at a time, each with the next
    /// member to look at, are at most [`MAX_MEMBER_DEPTH`], whatever a group holds).
    #[test]
    fn a_group_of_200000_members_is_read_to_its_budget_and_no_references_are_made_past_it() {
        let members: Vec<Shape> = (0..200_000).map(|i| boxed(i + 10, (0.0, 0.0, 1.0, 1.0), &[("ObjType", 1.0)])).collect();
        let doc = doc_of(vec![group(1, (0.0, 0.0, 1.0, 1.0), 8.0, members)]);
        let reader = Reader::new(&doc);
        let mut budget = 50;
        MEMBER_REFS.with(|c| c.set(0));
        let read = obstacles_of(&reader, &at(0), &HiddenLayers::default(), &|_| false, &mut budget);
        assert_eq!((read.obstacles.len(), read.cut), (49, true));
        let made = MEMBER_REFS.with(std::cell::Cell::get);
        assert!(made <= 60, "{made} references to members were made for a read of 50 shapes");
    }

    #[test]
    fn the_shapes_a_connector_end_is_glued_to_are_found_in_one_pass_and_a_row_of_another_cell_is_none() {
        let mut doc = doc_of(vec![]);
        let row = |from: u32, cell: &str, to: u32| Connect { from_sheet: from, from_cell: Some(Arc::from(cell)), to_sheet: to, ..Connect::default() };
        page_mut(&mut doc, 1).unwrap().connects = Connects::from_vec(vec![
            row(10, "BeginX", 1),
            row(10, "EndX", 2),
            row(11, "EndY", 2),
            row(12, "PinX", 3),
            row(13, "BeginX", 13),
            Connect { from_sheet: 14, to_sheet: 4, ..Connect::default() },
        ]);
        assert_eq!(glued_to(&doc, 1), HashSet::from([1, 2]));
        assert!(glued_to(&doc, 9).is_empty(), "a page that is not there");
    }

    #[test]
    fn gluing_makes_a_shape_that_reads_none_of_1_2_and_4_placeable_and_leaves_the_others() {
        let mut doc = doc_of(vec![
            boxed(1, (0.0, 0.0, 1.0, 1.0), &[]),
            boxed(2, (0.0, 0.0, 1.0, 1.0), &[("ObjType", 8.0)]),
            boxed(3, (0.0, 0.0, 1.0, 1.0), &[("ObjType", 1.0)]),
            boxed(4, (0.0, 0.0, 1.0, 1.0), &[("ObjType", 4.0)]),
            boxed(5, (0.0, 0.0, 1.0, 1.0), &[("ObjType", 2.0)]),
            boxed(6, (0.0, 0.0, 1.0, 1.0), &[("ObjType", 16.0)]),
            boxed(7, (0.0, 0.0, 1.0, 1.0), &[("BeginX", 0.0), ("BeginY", 0.0), ("EndX", 1.0), ("EndY", 1.0)]),
        ]);
        let mut guide = boxed(8, (0.0, 0.0, 1.0, 1.0), &[]);
        guide.kind = ShapeType::Guide;
        page_mut(&mut doc, 1).unwrap().shapes.push(Arc::new(guide));
        let written: Vec<bool> = (1..=9).map(|id| make_placeable(&mut doc, 1, id).unwrap()).collect();
        assert_eq!(
            written,
            [true, true, false, false, false, true, false, false, false],
            "a shape that holds 1, 2 or 4, a line, a guide and a shape that is not there are left"
        );
        let own = |id: u32| {
            let shape = doc.shape(&doc.find_shape(Owner::Page(1), id).unwrap()).unwrap();
            shape.sheet.cell("ObjType").and_then(|c| c.value.as_ref()?.number())
        };
        assert_eq!(
            [own(1), own(2), own(3), own(4), own(5), own(6), own(7)],
            [Some(1.0), Some(9.0), Some(1.0), Some(4.0), Some(2.0), Some(17.0), None]
        );
        assert!(!make_placeable(&mut doc, 1, 1).unwrap(), "once is enough: the shape reads 1 now");
    }

    /// The shapes a Re-Layout glues are found already (one walk of the page by the caller): making them placeable is one read of the document and one batch of writes, whatever their
    /// number, and no search for a shape by its `ID` (10,000 searches of a page of 10,000 shapes are 50 million visits).
    #[test]
    fn many_shapes_are_made_placeable_in_one_batch_with_no_search() {
        let mut doc = Document::new_blank();
        page_mut(&mut doc, 1).unwrap().shapes = ShapeList::from_vec((1..=10_000).map(|id| Arc::new(Shape::new(id))).collect());
        let targets: Vec<ShapeRef> = (0..10_000).map(at).collect();
        diacraft_doc::work::reset();
        assert_eq!(make_all_placeable(&mut doc, &targets).unwrap(), 10_000);
        assert_eq!(diacraft_doc::work::finds_visited(), 0, "no search for a shape");
        assert!(
            diacraft_doc::work::resolver_builds() < 100,
            "the lookups are built once for the batch, not for each shape: {}",
            diacraft_doc::work::resolver_builds()
        );
        // Written once: every shape reads 1 now, and a second call has nothing to write.
        assert_eq!(make_all_placeable(&mut doc, &targets).unwrap(), 0);
        let reader = Reader::new(&doc);
        assert!(targets.iter().all(|at| flags_of(reader.number(&SheetRef::Shape(at.clone()), "ObjType")) == 1));
    }

    #[test]
    fn a_guarded_object_type_is_left_as_it_is_and_the_call_is_no_error() {
        let mut doc = doc_of(vec![boxed(1, (0.0, 0.0, 1.0, 1.0), &[])]);
        let guarded = Cell::with_formula("GUARD(0)", Value::from_number(0.0));
        let page = page_mut(&mut doc, 1).unwrap();
        Arc::make_mut(&mut Arc::make_mut(page.shapes.get_mut(0).unwrap()).sheet).cells.insert(CellName::new("ObjType"), guarded);
        let before = doc.clone();
        assert!(!make_placeable(&mut doc, 1, 1).unwrap());
        assert_eq!(doc, before);
    }
}
