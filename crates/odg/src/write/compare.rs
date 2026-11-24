//! What the model changed since it was read, without a byte of the file: the model as it is against the model as the file reads (the
//! baseline), page by page and shape by shape (the design's "Source trees and baseline").
//!
//! **The two models are matched by the element each shape came from, not by `ID`.** The index the caller holds maps a shape of the model as it
//! is to the ordinal of its element in the file (`ShapeIndex`), and reading the file again maps the baseline's shapes to the same ordinals; so a
//! second save, whose model keeps the `ID`s of the first while the file's elements were renumbered by a delete, matches each shape to the shape
//! of the file it is. A shape of the model that the index does not know is a new shape, which a later task writes (a refusal here); a shape of
//! the baseline that no shape of the model matches is deleted.
//!
//! **What is compared**, for each pair: everything but the pose cells ([`own::rest_same`]), and a difference is a change the writer does not
//! yet write and refuses, by the words of its family; then the pose, worked out on the page through every group around the shape (a group moved
//! moves its members); then, for a connector, its route (`Geometry` section 0). The page's own fields, the page's connections and the
//! document's other parts must be the same too. A shape that no cell of its own or of a group around it changed costs one comparison and no
//! arithmetic.
//!
//! **Order.** The shapes of a level (a page, or a group) are compared in the order of the model, which is drawing order: the same shapes in
//! another order is a reorder, found by comparing the two lists restricted to the shapes they share.

use std::collections::{HashMap, HashSet};

use diacraft_doc::{Connect, Document, Page, Shape, ShapeList, ShapeType};

use super::own::{self, Frame, Pose};
use super::route::{self, Route};
use super::stylemap::{CellVal, PAGE_CELLS, PageEdit, StyleEdit};
use super::{WriteError, WriteWork};
use crate::index::{PageShapes, PageSource, ShapeIndex};
use crate::limits::Limits;
use crate::read::Affine;
use crate::read::units::LenUnit;

/// What changed of a shape's frame.
#[derive(Clone, Debug)]
pub(crate) enum FrameChange {
    None,
    /// The frame moved by this much (inches, y up), the same size and turn.
    Translate {
        dx: f64,
        dy: f64,
    },
    /// The size or the turn changed: the pose now, and the turn the file's `rotate` holds (the model's, and minus it beside one mirror of the geometry).
    Reshape {
        pose: Pose,
        angle: f64,
    },
    /// The ends of a segment, on the page, inches, y up.
    Ends([(f64, f64); 2]),
}

/// A change of a shape that remains.
#[derive(Debug)]
pub(crate) struct ShapeDiff {
    pub ord: u32,
    /// The `ID` the shape has in the model as it is.
    pub id: u32,
    /// The shape is a segment (the baseline has its ends).
    pub one_d: bool,
    /// The shape is a connector (the baseline has its route cells).
    pub connector: bool,
    pub group: bool,
    pub frame: FrameChange,
    pub route: Option<Route>,
    /// The corners of the route the file holds, when `route` is a route the save does not write (a hop: `Route::Unreadable`): the index the save gives keeps them, so
    /// that the next save compares the model's route with the file's.
    pub file_route: Option<Vec<(f64, f64)>>,
    /// The style cells that changed, when the shape was restyled.
    pub style: Option<StyleEdit>,
    /// The paragraphs that changed, when the shape's text was edited.
    pub text: Option<super::text::TextEdit>,
    /// The outline, when it was edited: the sheet it is written from, and the modifiers a handle changed.
    pub geo: Option<own::GeoEdit>,
    /// What DiaCraft's extension carries of the shape and what this change does to it: present for every changed shape, since a change of the pose,
    /// the text or the names is also a change of the tier 2 that holds the old one.
    pub tier2: Option<own::Tier2Edit>,
    /// A shape of the file the model turned, to whose glue points connectors of the model are glued: which kinds of point ([`TurnedGlue`]). `None` for a
    /// shape whose turn is the file's, or that nothing is glued to.
    pub turned_glue: Option<TurnedGlue>,
    /// The `draw:layer` to write on the shape of the file: the layer the save declares that the model put the shape on. `None` keeps the
    /// layer the file named, its list riding in DiaCraft's data.
    pub layer: Option<String>,
}

/// The glue points of a turned shape of the file that connectors are glued to: LibreOffice places a `draw:glue-point` in the page's axes, from the box the shape
/// takes up on the page (`read::connectors::Place`), and the standard points of a polygon, a polyline or a path on the box of its outline, so a turn the file does not
/// say moves them elsewhere than the model's rows, which turn with the shape. The save refuses such a turn in words (`plan::apply_shape`), as the writer has no inverse of
/// that placement yet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TurnedGlue {
    /// A connector is glued to a row a `draw:glue-point` of the file made (a row with a held glue point).
    pub elements: bool,
    /// A connector is glued to one of the four standard rows.
    pub standard: bool,
}

/// The words of the refusal of such a turn.
pub(crate) const WORDS_TURNED_GLUE: &str = "a turn of a shape of the file whose connectors are glued to a glue point that LibreOffice places on the box the shape takes up on the page (a glue point the file gives as a draw:glue-point, or a standard one of a polygon, a polyline or a path): it would put the point elsewhere once the shape is turned";

/// A level whose shapes are in another order than they were: the parent's ordinal (0 for the page) and the shapes' ordinals, those the model
/// still has, in the order the baseline had them and in the order the model has.
#[derive(Debug)]
pub(crate) struct Level {
    pub parent: u32,
    pub was: Vec<u32>,
    pub now: Vec<u32>,
}

/// What one page changed.
#[derive(Debug)]
pub(crate) struct PageDiff {
    /// The page's place in the model.
    pub page_at: usize,
    /// The place of the page's entry in the index the caller holds, and so in the file as it was read; `None` for a page the model added, whose
    /// element the save makes: its `source` is then a place after those of the file's pages, which no scan holds.
    pub held_at: Option<usize>,
    pub source: PageSource,
    /// The page's height in inches (the flip) and the unit of its width.
    pub height: f64,
    /// The height the page had in the file, which differs from `height` when the page was resized: a shape that keeps its `PinY` then keeps its
    /// distance from the bottom, and its `svg:y` is written anew.
    pub height_was: f64,
    pub unit: LenUnit,
    /// The page's size, margins or orientation edited: what is written into its layout.
    pub size: Option<PageEdit>,
    /// How many shape elements the page holds in the file.
    pub elements: u32,
    pub shapes: Vec<ShapeDiff>,
    /// The ordinals of the baseline's modelled shapes that the model no longer has, in order.
    pub deleted: Vec<u32>,
    pub levels: Vec<Level>,
    /// The connectors (by ordinal) whose end lost its glue, and which end: true for the start.
    pub unglue: Vec<(u32, bool)>,
    /// Each shape of the model as it is, by `ID` and the ordinal of its element.
    pub survivors: Vec<(u32, u32)>,
    /// The shapes the model has put on the page that the file lacks, in the model's order, each with the shape it follows.
    pub new: Vec<NewShape>,
    /// The connections of the new shapes: a new connector's two ends.
    pub new_connects: Vec<Connect>,
    /// The layers of the page, which a new shape is written on (R8 `draw:layer`).
    pub layers: super::layers::PageLayers,
    /// The groups of the file that this save changed something under (a member's pose, a member taken away, the group's own) and whose frame, as the model has it, is not
    /// the box of their members: the file holds such a group as its members and the reader gives the box, so each is told in a note ([`crate::SaveNote::GroupFrameNotKept`]).
    /// Their `ID`s, in the order the page holds them. (A new group is judged where it is written.)
    pub unkept: Vec<u32>,
    /// The connectors glued to a group (to the group itself, and not to a member) as `(group, connector)`, sorted and with no pair twice: found for a page that has a
    /// group to tell in a note, none for the others.
    pub glued_to_groups: Vec<(u32, u32)>,
    /// The shapes a new connector is glued to whose model has exactly one flip (`FlipX` and `FlipY` differ), by `ID`: their standard glue points may be numbered the
    /// other way round (`write::shapes::glue_id`).
    pub one_flip: HashSet<u32>,
    /// For a page of the file with a layer set of its own (a `draw:layer-set` of its `draw:page` or master page), the layers of that set whose `Visible`, `Print` or `Lock` the
    /// model changed, with their new flags: written on the set's `draw:layer` elements as the document's set has them written (`layers::reflagged`).
    pub layer_flags: Vec<super::layers::NewLayer>,
    /// The page's comments as a save writes them, when they are not what the file's annotations read as (`comments.rs`).
    pub comments: Option<super::comments::CommentEdit>,
    /// The new shapes the page's comments are on, by `ID`: they keep their `ID`s in the file, as a shape another names does.
    pub tied: Vec<u32>,
}

/// What a new shape follows in the model's top level: nothing, a shape of the file by its ordinal, or an earlier new shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Anchor {
    First,
    Ord(u32),
    New(usize),
}

/// A shape the model has and the file lacks, and what it follows.
#[derive(Clone, Debug)]
pub(crate) struct NewShape {
    pub shape: std::sync::Arc<Shape>,
    /// What it follows in the top level. A member of a new group follows nothing: it is written inside the group's element, in the group's order.
    pub after: Anchor,
    /// The new group this shape is a member of, by its place in the list of new shapes: the group is written with its members inside it.
    pub parent: Option<usize>,
    /// The map from the shape's own frame to the page's: the identity for a shape of the top level, and the groups' around it for a member of a new group.
    pub outer: Affine,
}

/// The shapes of the model's page the file lacks, in order, each after the one before it in the model; a new group is followed by its members, deepest last, in
/// the pre-order the reader numbers them in. **A shape of the file inside a new group is refused in words**: moving an element of the file into another is not
/// written, and the shape would be written twice.
fn news_of(doc: &Page, held: &PageShapes) -> Result<Vec<NewShape>, WriteError> {
    let mut out: Vec<NewShape> = Vec::new();
    let mut previous = Anchor::First;
    for s in doc.shapes.iter() {
        match held.ordinal(s.id) {
            Some(o) => previous = Anchor::Ord(o),
            None => {
                let top = out.len();
                push_new(&mut out, s, previous, held)?;
                previous = Anchor::New(top);
            }
        }
    }
    Ok(out)
}

/// Puts a new top-level shape in `out`, and for a group its members after it, each with the map of the groups around it: a stack of the members still to put, so no
/// input makes the walk recurse.
fn push_new(out: &mut Vec<NewShape>, shape: &std::sync::Arc<Shape>, after: Anchor, held: &PageShapes) -> Result<(), WriteError> {
    let starved = |held: usize| WriteError::Limit { what: "memory for the new shapes of a page", max: held as u64 };
    // The shapes to put, the last first: each with the group it is a member of, the map of the groups around it, and what it follows.
    let mut stack: Vec<(&std::sync::Arc<Shape>, Option<usize>, Affine, Anchor)> = vec![(shape, None, Affine::IDENTITY, after)];
    while let Some((s, parent, outer, after)) = stack.pop() {
        if parent.is_some() && held.ordinal(s.id).is_some() {
            return Err(WriteError::Unsupported(
                "a shape of the file put into a new group (moving an element of the file into another is not written)",
            ));
        }
        let at = out.len();
        out.try_reserve(1).map_err(|_| starved(at))?;
        out.push(NewShape { shape: std::sync::Arc::clone(s), after, parent, outer });
        if s.children.is_empty() {
            continue;
        }
        // The members are in the group's own frame: the map of the group takes them to the page.
        let own = own::local_to_parent(&s.sheet)
            .map(|m| own::compose(&outer, &m))
            .ok_or(WriteError::Unsupported("a new group whose frame is not numbers"))?;
        stack.try_reserve(s.children.len()).map_err(|_| starved(at))?;
        stack.extend(s.children.iter().rev().map(|m| (m, Some(at), own, Anchor::First)));
    }
    Ok(())
}

impl PageDiff {
    /// How much taller the page is than the file had it, inches (negative for a shorter one): the distance from the top a shape of the file keeps is as it was, and its
    /// y, up from the bottom, is `grew_by` more.
    pub(crate) fn grew_by(&self) -> f64 {
        self.height - self.height_was
    }

    /// Whether the page changed at all.
    pub(crate) fn any(&self) -> bool {
        !self.shapes.is_empty()
            || !self.deleted.is_empty()
            || !self.levels.is_empty()
            || !self.unglue.is_empty()
            || self.size.is_some()
            || !self.new.is_empty()
            || !self.layer_flags.is_empty()
            || self.comments.is_some()
    }
}

/// What the model changed.
#[derive(Debug, Default)]
pub(crate) struct Diff {
    /// One entry for each page of the model, in its order.
    pub pages: Vec<PageDiff>,
    /// What the model did to the pages: which it added, took away, moved and renamed.
    pub structure: super::structure::Structure,
    /// What the save does to DiaCraft's document file, when the model holds other document-level data than the file does.
    pub docfile: Option<crate::ext::docfile::Change>,
    /// The style sheets the model has that the file lacks, written as common styles of `styles.xml`, and the names shapes' parents are written with (`newstyles.rs`).
    pub styles: super::newstyles::Plan,
    /// The layers of the model's pages that no page of the file reads, which the save adds to the document's layer set of `styles.xml` (`layers::new_layers`).
    pub new_layers: Vec<super::layers::NewLayer>,
    /// The layers of the document's layer set whose `Visible`, `Print` or `Lock` the model changed, with their new flags: the save writes their `draw:display` and
    /// `draw:protected` (`layers::reflagged`).
    pub layer_flags: Vec<super::layers::NewLayer>,
}

impl Diff {
    pub(crate) fn any(&self) -> bool {
        self.pages.iter().any(PageDiff::any)
            || self.docfile.is_some()
            || self.structure.any()
            || !self.styles.is_empty()
            || !self.new_layers.is_empty()
            || !self.layer_flags.is_empty()
    }
}

/// A shape of one of the two models, with where it is.
struct Node<'a> {
    shape: &'a Shape,
    ord: u32,
    id: u32,
    /// The index in the list of nodes of the shape's group, when it has one.
    parent: Option<usize>,
    /// The map to the page of the group that holds it (the identity for a top-level shape), and the shape's own.
    outer: Affine,
    own: Option<Affine>,
}

/// The page's unit: the unit its width cell was read in.
fn page_unit(page: &Page) -> LenUnit {
    match page.sheet.cell("PageWidth").and_then(|c| c.unit.as_deref()) {
        Some("MM") => LenUnit::Mm,
        Some("IN") => LenUnit::In,
        Some("PT") => LenUnit::Pt,
        Some("P") => LenUnit::Pc,
        _ => LenUnit::Cm,
    }
}

/// Every shape of a level and below, in pre-order, each with its ordinal by `ordinal`. A shape the index does not know is `Err(id)`.
fn walk<'a>(
    shapes: &'a ShapeList,
    ordinal: &dyn Fn(u32) -> Option<u32>,
    skip_unknown: bool,
    limits: &Limits,
    work: &mut WriteWork,
) -> Result<Result<Vec<Node<'a>>, u32>, WriteError> {
    // The lists of the walk are as long as the shapes of the level and below: grown by `try_reserve`, an error and not an abort.
    let starved = |held: usize| WriteError::Limit { what: "memory for the walk of a page", max: held as u64 };
    let mut out: Vec<Node<'a>> = Vec::new();
    // The pending shapes, last first, with the node that holds them.
    let mut stack: Vec<(&'a Shape, Option<usize>)> = Vec::new();
    stack.try_reserve_exact(shapes.len()).map_err(|_| starved(shapes.len()))?;
    stack.extend(shapes.iter().map(|s| (&**s, None)));
    stack.reverse();
    while let Some((shape, parent)) = stack.pop() {
        work.charge_compared(limits)?;
        let Some(ord) = ordinal(shape.id) else {
            // A shape of the model's page the file lacks is a new shape (`news_of` takes it); one in a group is refused.
            if skip_unknown && parent.is_none() {
                continue;
            }
            return Ok(Err(shape.id));
        };
        let outer = parent.and_then(|p| out.get(p)).and_then(|n| n.own).unwrap_or(Affine::IDENTITY);
        let own = own::local_to_parent(&shape.sheet).map(|m| own::compose(&outer, &m));
        let at = out.len();
        out.try_reserve(1).map_err(|_| starved(at))?;
        out.push(Node { shape, ord, id: shape.id, parent, outer, own });
        let before = stack.len();
        stack.try_reserve(shape.children.len()).map_err(|_| starved(before))?;
        stack.extend(shape.children.iter().map(|c| (&**c, Some(at))));
        stack[before..].reverse();
    }
    Ok(Ok(out))
}

/// Whether the cells that make a shape's place are the same cells in both sheets: the same value, or number, of each.
fn pose_cells_same(a: &diacraft_doc::Sheet, b: &diacraft_doc::Sheet) -> bool {
    own::POSE_CELLS.iter().all(|name| match (a.cell(name), b.cell(name)) {
        (None, None) => true,
        (Some(x), Some(y)) => {
            x.formula == y.formula && (x.value == y.value || x.value.as_ref().and_then(|v| v.number()) == y.value.as_ref().and_then(|v| v.number()))
        }
        _ => false,
    })
}

/// The change of one shape that remains, or `None` when it is the same to a save.
struct Pair<'a> {
    base: &'a Node<'a>,
    doc: &'a Node<'a>,
    /// Where a shape's fill pattern is looked for when its own sheet holds none.
    fills: &'a Fills<'a>,
    /// A cell of the pose of a group around the shape changed, or a cell of its own pose did: the pose must be worked out.
    pose_may_differ: bool,
    /// How much the page's height grew (inches): the y of every shape on it is written against another height.
    dh: f64,
    /// The baseline shape's own style carries tier 2 in the file: what the extension holds follows an edit of the shape.
    in_file: bool,
    /// The baseline shape was read as a copy of another (its style carries an `ID` an earlier shape carries): it holds none of the data its style carries.
    copy: bool,
    /// The model lost a glue of this shape (a connector's end was unglued): the formulas of its end cells changed with it, and the connections say so.
    glue_off: bool,
    /// The tier-2 items of the baseline shape that tier 3 found stale when the file was read (`PageShapes::stale`).
    stale: &'a [crate::ext::fingerprint::Key],
    /// The native outputs of the baseline shape's markup that its tier-2 items rest on, as the file had them (`PageShapes::natives`).
    natives: Option<crate::ext::tier3::Natives>,
    /// The index the caller holds for the page: which routes a save kept ([`PageShapes::kept_route`]).
    held: &'a PageShapes,
}

fn classify(pair: &Pair<'_>, work: &mut WriteWork) -> Result<Option<ShapeDiff>, WriteError> {
    let (b, d) = (pair.base.shape, pair.doc.shape);
    let one_d = b.sheet.cell("BeginX").is_some();
    let connector = b.sheet.cell("ShapeRouteStyle").is_some();
    let palette = |index: i64| pair.fills.palette(index);
    let (style, text, geo, delta) = match own::shape_change(b, d, connector, &palette, &mut work.cells_compared, &mut work.memory_room) {
        own::ShapeChange::Same => (None, None, None, None),
        own::ShapeChange::Edit { style, text, geo, delta } => (style, text, geo, delta),
        own::ShapeChange::Refuse(words) => return Err(WriteError::Unsupported(words)),
        own::ShapeChange::Memory => {
            return Err(WriteError::Limit {
                what: "bytes of memory a save holds (the model and the file read again beside it)",
                max: work.memory_cap,
            });
        }
    };
    // A fill colour set, or a gradient turned off, states `draw:fill` as the model's fill pattern says (class F4).
    let style = style.map(|mut edit| {
        own::decide_fill(&mut edit, &d.sheet, |cell| pair.fills.number(&d.sheet, cell, &mut work.cells_compared));
        // The numbers a line end's attributes are worked out from, the shape's own or its line style's.
        own::decide_line_end(&mut edit, &d.sheet, |cell| pair.fills.line_number(&d.sheet, cell, &mut work.cells_compared));
        edit
    });
    // The formula of a connector's end cell is the glue's: it changes when the glue goes, and then the connection says so. A formula set on an end with the glue as
    // it was is not written (the file would rebuild the end from its glue), so it is refused in words and not dropped.
    if connector && !pair.glue_off && !own::pose_extras_same(&b.sheet, &d.sheet, &own::END_CELLS) {
        return Err(WriteError::Unsupported(WORDS_END_FORMULA));
    }
    let group = d.kind == ShapeType::Group;
    // A group's own text (R45 writes it as one more member of the group: not written yet) is refused in words, and never dropped (review).
    if group && (text.is_some() || super::groups::holds_text(d)) {
        return Err(WriteError::Unsupported(WORDS_GROUP_TEXT_OF_THE_FILE));
    }
    // A group's angle or flip that differs from the file's is not written (R45 keeps it in DiaCraft's document file and writes the members turned: not written yet), and
    // the members' poses it changes are written without it, which reads back as an angle of 0: refused in words (review).
    if group {
        let (was, now) = (super::groups::turn_of_sheet(&b.sheet), super::groups::turn_of_sheet(&d.sheet));
        if was.iter().zip(&now).any(|(x, y)| (x - y).abs() > 1e-9) {
            return Err(WriteError::Unsupported(super::groups::WORDS_GROUP_TURN));
        }
    }
    // A group's own data (its cells beyond what the markup says) belongs in DiaCraft's document file, keyed by the page and the group's `draw:name` and number
    // The file's entries are made from the model, so a change to it is the file's, and no edit of the group's element. **Its name
    // is the key**, and a change to it is not written (the entry is found by it, and the element's `draw:name` is not regenerated for a group): refused in words.
    if group && delta.as_ref().is_some_and(|d| d.fields) {
        return Err(WriteError::Unsupported(WORDS_GROUP_NAME));
    }
    // A group's frame cells are what the reader rebuilds from the box of its members: no entry of the document file carries them (`ext::docfile::GROUP_FRAME`), so
    // a change of one that is more than a move or a resize (the place of the pin in the box given as a number, a formula set on a frame cell) would be written nowhere
    // and read back as the box's centre, with the save saying nothing: refused in words, as the turn of a group is.
    // (A turn cell the model gained at 0 is the turn the file's group has: it is no pin, and is not refused here.)
    if group && delta.as_ref().is_some_and(|d| super::groups::frame_cell_edited(&b.sheet, d)) {
        return Err(WriteError::Unsupported(WORDS_GROUP_PIN));
    }
    let delta = if group { None } else { delta };
    let frame = if group || !pair.pose_may_differ { FrameChange::None } else { frame_change(pair, one_d)? };
    // The route is written in the page's coordinates (`svg:d`), so a connector whose ends moved (it was moved itself, or its glue was) has it written again
    // even when its own route, in its frame, is as it was (a Data Flow moved after its first save was drawn where it had been).
    // **Rows that spell the route another way are no new route** (the milestone-1 rerun's W1): a connector no edit touched has its rows rebuilt in the
    // engine's form by a later rebuild of line jumps (a hop taken away, a point on a leg dropped), and writing that would draw it elsewhere (its `draw:line-skew` goes
    // with a route written anew). The ends being as they were, its rows are compared by the corners of the route on the page ([`route::corners`]).
    // **A change of the page's height is no edit of a connector**: it keeps `PinY`, so every shape and every connector end moves down the
    // page together and the route cells, which are measured from the page's bottom, stay as they are. The connector keeps its own path through it, shifted by what the page
    // grew (`plan.rs`, `apply_shape`: the file's path is a fixed distance from the page's top), so a route whose corners are the file's is no route to write. **And it is not
    // named again**: a connector whose rows and ends are as they were is routed by nothing, whatever the page's height did; a route the save kept for a hop
    // was said in the save that kept it, and a path with two subpaths, which reads as a route with a second `MoveTo`, is no hop and was never to be said.
    let rows_same = own::route_same(&b.sheet, &d.sheet, &mut work.cells_compared);
    let ends_moved = ends_moved(pair, &frame);
    // **The route the file holds is what the model's is compared with, and that is not always the baseline's**: a save that kept the file's path (a hop, said) left the
    // model as the baseline and the file as it was, so a route that lost its hop is the baseline's by its corners and not the file's.
    let mut file_route = None;
    let route = if !connector || (rows_same && !ends_moved) {
        None
    } else if ends_moved {
        let route = doc_route(pair);
        if matches!(route, Route::Unreadable) {
            file_route = file_route_of(pair, &mut work.cells_compared);
        }
        Some(route)
    } else {
        let held = file_route_of(pair, &mut work.cells_compared);
        let route = spelled_route(pair, held.as_deref(), &mut work.cells_compared);
        if matches!(route, Some(Route::Unreadable)) {
            file_route = held;
        }
        route
    };
    if matches!(frame, FrameChange::None) && route.is_none() && style.is_none() && text.is_none() && geo.is_none() && delta.is_none() {
        return Ok(None);
    }
    // Only a shape that carries tier 2 in the file, or whose edit adds some, has the extension looked at: the edit of any other costs what it did.
    // A copy has no tier 2 of its own: an edit that adds some gives it one, and an edit of its style (which names the style of the shape it is a copy of) gives it
    // a style that does not repeat the other shape's `ID` and data.
    let copy_edit = pair.copy && (delta.is_some() || style.is_some() || text.is_some());
    let tier2 = (delta.is_some() || pair.in_file || copy_edit).then(|| own::Tier2Edit {
        id: pair.doc.id,
        copy: pair.copy,
        model: std::sync::Arc::new(d.clone()),
        delta: delta.unwrap_or_default(),
        stale: pair.stale.to_vec(),
        outline: own::OutlineWrite::of(geo.as_ref()),
        natives: pair.natives.clone(),
    });
    Ok(Some(ShapeDiff {
        ord: pair.doc.ord,
        id: pair.doc.id,
        one_d,
        connector,
        group,
        frame,
        route,
        file_route,
        style,
        text,
        geo,
        tier2,
        turned_glue: None,
        layer: None,
    }))
}

/// The words of the refusal of a text on a group of the file.
pub(crate) const WORDS_GROUP_TEXT_OF_THE_FILE: &str = "a text on a group of the file (a group's text is written as one more member of the group, which is not written yet: take the text off the group, or put it on a shape beside it)";

/// The words of the refusal of a change of a group's frame cells that is neither a move nor a resize.
pub(crate) const WORDS_GROUP_PIN: &str = "a change of a group's pin that is neither a move nor a resize (the place of its pin in its box set as a number, or a formula on one of its frame cells): a group's frame is the box of its members, which the file holds in the members' places and has no place for a pin of another place";

/// The words of the refusal of a change to a group's name or its master link.
pub(crate) const WORDS_GROUP_NAME: &str = "a change to a group's name or master link (the name is the key DiaCraft's document file finds the group's data by, and a group's element has no style to carry a link)";

/// The words of the refusal of a formula set on the end of a connector whose glue is as it was.
pub(crate) const WORDS_END_FORMULA: &str = "a formula or another attribute set on the end of a connector whose glue is as it was (the ends of a connector are rebuilt from its glue; a formula of their own is not written yet)";

/// The most style sheets a fill cell is looked for through for one shape.
const MOST_STYLE_STEPS: usize = 64;

/// Where a fill cell (`FillPattern`, `FillGradientEnabled`) of a shape that holds none of its own is found: the drawing's style sheets by `ID`, indexed the first
/// time one is asked for (a restyle of a colour on a shape whose styles state no fill), with each sheet looked at counted in the steps the comparison is held to.
pub(crate) struct Fills<'a> {
    document: &'a Document,
    by_id: std::cell::OnceCell<HashMap<u32, usize>>,
}

impl<'a> Fills<'a> {
    pub(crate) fn new(document: &'a Document) -> Fills<'a> {
        Fills { document, by_id: std::cell::OnceCell::new() }
    }

    /// The colour an index of the fixed palette or of the document's table names, as `#rrggbb`.
    pub(crate) fn palette(&self, index: i64) -> Option<String> {
        self.document.color(index).map(|c| format!("#{}", c.hex().to_ascii_lowercase()))
    }

    /// The number the model draws a shape with in the fill cell `cell` when its own sheet holds none: the first of its fill style sheet, that sheet's parent and
    /// so on that holds one, ending at the root (style 0). `None` when no sheet in the chain does, or the chain is longer than [`MOST_STYLE_STEPS`] (a cycle).
    /// The index costs one step for each style sheet, once, and each sheet looked at one.
    pub(crate) fn number(&self, sheet: &diacraft_doc::Sheet, cell: &str, steps: &mut u64) -> Option<f64> {
        self.chain_number(sheet, cell, steps, |links| links.fill)
    }

    /// [`Fills::number`] for a cell of the line family (a line end's, the weight): found through the `LineStyle` link and the sheets above it.
    pub(crate) fn line_number(&self, sheet: &diacraft_doc::Sheet, cell: &str, steps: &mut u64) -> Option<f64> {
        self.chain_number(sheet, cell, steps, |links| links.line)
    }

    fn chain_number(
        &self,
        sheet: &diacraft_doc::Sheet,
        cell: &str,
        steps: &mut u64,
        link_of: fn(&diacraft_doc::Links) -> Option<u32>,
    ) -> Option<f64> {
        let index = self.by_id.get_or_init(|| {
            *steps = steps.saturating_add(self.document.styles.len() as u64);
            self.document.styles.iter().enumerate().map(|(at, style)| (style.id, at)).collect()
        });
        let number = |s: &diacraft_doc::Sheet| s.cell(cell).and_then(|c| c.value.as_ref()).and_then(diacraft_doc::Value::number);
        let mut link = link_of(&sheet.links).unwrap_or(0);
        for _ in 0..MOST_STYLE_STEPS {
            *steps = steps.saturating_add(1);
            let style = index.get(&link).and_then(|at| self.document.styles.get(*at))?;
            if let Some(n) = number(&style.sheet) {
                return Some(n);
            }
            if link == 0 {
                return None;
            }
            link = link_of(&style.sheet.links).unwrap_or(0);
        }
        None
    }
}

/// The route a connector has in the model: its `Geometry` section 0 on the page, or [`Route::Unreadable`].
fn doc_route(pair: &Pair<'_>) -> Route {
    route::of(&pair.doc.shape.sheet, &pair.doc.outer).map_or(Route::Unreadable, Route::Path)
}

/// Whether the ends of a connector are elsewhere on the page than the baseline had them: its own cells, or those of a group around it, moved them. **A change of the
/// page's height moves none** (the ends are in the model's measure, up from the page's bottom, and a height edit keeps `PinY`): the frame is `Ends` for it all the same, to
/// have the ends written against the new height, and this is what tells the two apart.
fn ends_moved(pair: &Pair<'_>, frame: &FrameChange) -> bool {
    let FrameChange::Ends(now) = frame else { return false };
    own::ends(&pair.base.shape.sheet, &pair.base.outer).is_none_or(|was| !own::same_ends(&was, now))
}

/// The corners of the route the **file** holds of the connector, **in the page as this save leaves it**: those the index kept when a save kept the file's path
/// ([`PageShapes::kept_route`]), else the baseline's (the file as it was read, or the model as a save wrote its route). `None` for a route that has none (a curve, a route
/// that is no path). The rows looked at are counted in `steps`.
///
/// **The corners are the same whatever the page's height does**: the file's path is a fixed distance from the page's top, and a save that changes the height shifts every
/// path it keeps by the change (`plan.rs`, `apply_shape`), so the corners, which are measured from the page's bottom, are where they were. (Before
/// the path stayed where it was, and the corners were raised by the page's growth, the blocker.) The page's width needs no care
/// either: `svg:x` is from the left edge.
fn file_route_of(pair: &Pair<'_>, steps: &mut u64) -> Option<Vec<(f64, f64)>> {
    pair.held.kept_route(pair.base.id).or_else(|| route::corners(&pair.base.shape.sheet, &pair.base.outer, steps))
}

/// What a connector whose ends are as they were makes of its route, when its rows differ between the models (a connector whose rows are the baseline's is no concern of this:
/// its path is the file's, whatever the page's height did, [`classify`]): **nothing** when the corners of its route on the page are the same as those of the route the file
/// holds, `file` (the rows are the engine's spelling of the file's route, or the route without a hop it had), unless the model's route has a hop now, which the `.odg` has no
/// place for: that is said, and the file's own path is kept ([`Route::Unreadable`]). A route whose corners differ, or of which either side has none (a curve, a route
/// that is no path), is the model's route as it stands. The rows looked at are counted in `steps`.
fn spelled_route(pair: &Pair<'_>, file: Option<&[(f64, f64)]>, steps: &mut u64) -> Option<Route> {
    let route = doc_route(pair);
    match (file, route::corners(&pair.doc.shape.sheet, &pair.doc.outer, steps)) {
        (Some(was), Some(now)) if route::same_corners(was, &now) => matches!(route, Route::Unreadable).then_some(route),
        _ => Some(route),
    }
}

/// The change of a shape's frame between the two models.
fn frame_change(pair: &Pair<'_>, one_d: bool) -> Result<FrameChange, WriteError> {
    let words = WriteError::Unsupported("a position, size or turn cell that is not a number");
    let (b, d) = (&pair.base.shape.sheet, &pair.doc.shape.sheet);
    if one_d {
        let (old, new) = (own::ends(b, &pair.base.outer).ok_or(words.clone())?, own::ends(d, &pair.doc.outer).ok_or(words)?);
        return Ok(if own::same_ends(&old, &new) && pair.dh == 0.0 { FrameChange::None } else { FrameChange::Ends(new) });
    }
    let (old, new) = (own::pose_2d(b, &pair.base.outer).ok_or(words.clone())?, own::pose_2d(d, &pair.doc.outer).ok_or(words)?);
    // A page that grew by `dh` puts every shape that keeps its `PinY` lower by `dh`, down the page: the shift the model sees is `dh` less.
    Ok(match own::frame(&old, &new) {
        Frame::Same if pair.dh == 0.0 => FrameChange::None,
        Frame::Same => FrameChange::Translate { dx: 0.0, dy: -pair.dh },
        Frame::Translate { dx, dy } => FrameChange::Translate { dx, dy: dy - pair.dh },
        // The turn written is the turn of the frame, **not counting the flips the file holds**: they are the mirrors of the shape's geometry
        // (`draw:mirror-horizontal`, `draw:mirror-vertical`), which stay as the file has them and mirror the shape inside its frame before the transform
        // turns it. A shape flipped both ways has a linear part that is itself a turn of half a turn more than its
        // `Angle`, and writing that turn beside the mirrors turned the shape half a turn too far (the milestone-1 class F5).
        Frame::Reshape(pose) => {
            // A change of the flips is not written (the mirrors are not edited by this save): the flips are the ones the baseline holds.
            let flips = own::flips_of(&pair.base.shape.sheet).unwrap_or((1.0, 1.0));
            if own::flips_of(&pair.doc.shape.sheet).unwrap_or((1.0, 1.0)) != flips {
                return Err(WriteError::Unsupported("a flip, a scale or a shear of a shape's frame"));
            }
            match own::turn_of(&own::unflipped(&pose, flips)) {
                // The turn the file holds beside one mirror is minus the model's (`read::turn_beside_mirrors`).
                Some(angle) => FrameChange::Reshape { pose, angle: crate::read::turn_beside_mirrors(angle, flips.0 == -1.0, flips.1 == -1.0) },
                None => return Err(WriteError::Unsupported("a flip, a scale or a shear of a shape's frame")),
            }
        }
    })
}

/// The document's fields besides its pages that a save does not write. **In a package, what DiaCraft's document file carries is not among them** (masters, themes,
/// the document's sheet, the colour table and DiaCraft's own data: `ext::docfile`); in a flat drawing (`flat`), which has no such file, the first four are
/// (a change of them is refused in words and never dropped), and DiaCraft's own data is the flat extension's, which the caller compares. The font table is
/// in neither: a drawing read from an `.odg` has an empty one
/// (the file's own faces are declarations the styles name, not a table), and the engine adds a name to it when a run is given a font; the font a run names is
/// written as a `style:font-face` declaration beside the text style that names it, so a name in the table changes nothing that is written, and
/// a save that refused it refused what it could write.
fn document_same(doc: &Document, base: &Document, flat: bool, file: &[crate::ext::docfile::NativeStyle], steps: &mut u64, model_path: bool) -> bool {
    doc.version == base.version
        && doc.title == base.title
        && props_same(&doc.props, &base.props)
        // In a package a style sheet added last is carried (`newstyles.rs`); in a flat drawing, which has no document file for what it holds, the styles are
        // compared whole.
        && (if flat { super::newstyles::styles_same(doc, base) } else { super::newstyles::plan(doc, base, file, steps, model_path).is_ok() })
        // In a package the Snap & Glue words of `DocumentSettings` are carried by DiaCraft's document file (`ext::docfile`), and are no part of what is compared;
        // a flat drawing has no such file, so its whole `passthrough` is.
        && (if flat { doc.passthrough == base.passthrough } else { doc.same_but_snap_words(base) })
        // A flat drawing has no document file (a package entry): what that file carries has no place in it, and a change of it is refused. DiaCraft's own data
        // (`diacraft`) is no part of this either way: the document file carries it in a package, and a flat drawing's extension is the save's to write.
        && (!flat
            || (doc.masters == base.masters
                && doc.themes == base.themes
                && doc.colors == base.colors
                && own::sheet_same(&doc.sheet, &base.sheet, steps)))
}

/// **What a refusal of a change of the document's own fields says it is** (the one sentence for all of them hid that 1,404 refusals were a core property written as an empty element, not
/// the masters the sentence named): for a package, the first of the document's own fields that [`document_same`] compares and that differs. Every sentence keeps the words "the document's own fields", which
/// callers and tests look for, and is no longer than it must be: the banner of a flat file cuts a long cause and with it the way out. **A flat drawing keeps the sentence that was said of all of them** (its
/// banner holds the way out after it, and what a flat file's change is, its masters, is what the sentence names), and so does a difference none of the fields below is (the Snap & Glue words and the styles
/// are compared as `document_same` does).
fn document_difference(doc: &Document, base: &Document, flat: bool) -> &'static str {
    if flat {
        "a change of the document's own fields, styles or masters"
    } else if doc.version != base.version {
        "a change of the document's own fields (its version)"
    } else if doc.title != base.title {
        "a change of the document's own fields (its title)"
    } else if !props_same(&doc.props, &base.props) {
        "a change of the document's own fields (a property)"
    } else if !doc.same_but_snap_words(base) {
        "a change of the document's own fields (the elements of its root)"
    } else {
        "a change of the document's own fields, styles or masters"
    }
}

/// **Whether the document's properties are those of the file, as far as an `.odg` holds them**: every field but the version of the program that wrote the file
/// (`app_version`, a VSDX file's `AppVersion`), which is the producer's, as the generator is. No `.odg` holds another program's version: the model writer leaves it out and the reader
/// of an `.odg` gives none, so a drawing of a VSDX file saved as an `.odg` keeps the version its VSDX file said while the file the save wrote reads with none, and comparing it refused the next
/// save of that drawing, with no edit, as "a change of the document's own fields (a property)". No save writes it and no edit sets it, so leaving it out of the comparison loses nothing a save
/// could have kept. **Every other field is compared**: the destructuring names them all, so that a field added to [`diacraft_doc::DocProps`] is compared until it is ruled out by name.
fn props_same(doc: &diacraft_doc::DocProps, base: &diacraft_doc::DocProps) -> bool {
    let diacraft_doc::DocProps {
        subject,
        creator,
        last_modified_by,
        keywords,
        description,
        category,
        company,
        manager,
        hyperlink_base,
        app_version: _,
        created,
        modified,
        last_printed,
        language,
        custom,
    } = doc;
    *subject == base.subject
        && *creator == base.creator
        && *last_modified_by == base.last_modified_by
        && *keywords == base.keywords
        && *description == base.description
        && *category == base.category
        && *company == base.company
        && *manager == base.manager
        && *hyperlink_base == base.hyperlink_base
        && *created == base.created
        && *modified == base.modified
        && *last_printed == base.last_printed
        && *language == base.language
        && *custom == base.custom
}

/// The page's fields besides its shapes and its connections.
#[cfg(test)]
fn page_same(doc: &Page, base: &Page, steps: &mut u64) -> bool {
    page_fields_same(doc, base, &[], steps)
}

/// The page's fields a save does not write besides what DiaCraft's document file carries: its name, background, back page, relationship, attributes and
/// unknown nodes. In a package the page's `ID`, its `NameU` and its sheet are carried (the file holds what the markup does not say; a page's size, margins and
/// orientation are written into its layout by [`page_edit`]); a flat drawing has no such file, so they are compared too (`flat`).
fn page_fields_carried(doc: &Page, base: &Page, flat: bool, steps: &mut u64) -> bool {
    if flat {
        return page_fields_same(doc, base, &PAGE_CELLS, steps);
    }
    // The name is not compared: a page under another name is a rename, which the save writes (`structure`).
    same_page_markup(doc, base)
}

/// Whether the model's page has the page's own fields the file's page has but for its name, its `ID` and its cells: whether it is that page, as far as what the file holds
/// of a page's element says (its background, the page it names as its background, its relationship, the attributes of its element and the nodes it keeps).
pub(crate) fn same_page_markup(doc: &Page, base: &Page) -> bool {
    doc.background == base.background
        && doc.back_page == base.back_page
        && doc.rel_id == base.rel_id
        && doc.raw == base.raw
        && own::passthrough_same(&doc.passthrough, &base.passthrough)
        && own::page_kept_same(doc, base)
}

/// The page's fields besides its shapes and connections, and but for the cells named in `skip` (a save of a package compares the page's fields by
/// [`page_fields_carried`] and its sheet by what the document file carries; a flat drawing's save compares them all).
fn page_fields_same(doc: &Page, base: &Page, skip: &[&str], steps: &mut u64) -> bool {
    doc.id == base.id
        && doc.name == base.name
        && doc.name_u == base.name_u
        && doc.background == base.background
        && doc.back_page == base.back_page
        && doc.rel_id == base.rel_id
        && (if skip.is_empty() { own::sheet_same(&doc.sheet, &base.sheet, steps) } else { own::sheet_same_but(&doc.sheet, &base.sheet, skip, steps) })
        && doc.raw == base.raw
        && own::passthrough_same(&doc.passthrough, &base.passthrough)
        && own::page_kept_same(doc, base)
}

/// The model's changes since it was read: `doc` against `base`, matched through `held` (the index the caller holds) and `fresh` (the index
/// reading the file again gives). The document's other fields, each page's, and the pages in the same order are compared first.
pub(crate) fn compare(
    doc: &Document,
    base: &Document,
    held: &ShapeIndex,
    fresh: &ShapeIndex,
    limits: &Limits,
    work: &mut WriteWork,
) -> Result<Diff, WriteError> {
    compare_with(doc, base, held, fresh, false, limits, work)
}

/// [`compare`], where `flat` says the drawing is a flat one: its document's own DiaCraft data is the save's to write (the flat extension), a
/// change of it is no refusal and no part of the difference, and the caller compares it; and it has no document file, so what that file carries is compared and a
/// change of it is refused ([`document_same`], [`page_fields_carried`]).
pub(crate) fn compare_with(
    doc: &Document,
    base: &Document,
    held: &ShapeIndex,
    fresh: &ShapeIndex,
    flat: bool,
    limits: &Limits,
    work: &mut WriteWork,
) -> Result<Diff, WriteError> {
    if !document_same(doc, base, flat, &work.file_styles, &mut work.cells_compared, work.model_path) {
        // A change of the style sheets that is not a sheet added last says what is wrong with it.
        if !flat && let Err(words) = super::newstyles::plan(doc, base, &work.file_styles, &mut work.cells_compared, work.model_path) {
            return Err(WriteError::Unsupported(words));
        }
        return Err(WriteError::Unsupported(document_difference(doc, base, flat)));
    }
    // The two indexes are of the file: as many entries as the file has pages.
    if held.pages() != base.pages.len() || fresh.pages() != base.pages.len() {
        return Err(WriteError::Mismatch("the index does not hold the pages of the drawing"));
    }
    let structure = super::structure::of(doc, base, held, fresh, flat, limits, work)?;
    let mut diff = Diff { structure, ..Diff::default() };
    // The layers the model's pages have that no page of the file reads (a layer added in DiaCraft): the save declares them in the document's layer set, so that another
    // program sees them. A flat drawing carries no page's layers, and is given none.
    // Only a page whose rows are not the very rows its markup gives it can hold such a layer: a save that changed no page's layers looks at none of them (the cost).
    if !flat {
        let key = diacraft_doc::SectionKey::new("Layer", None);
        let mut changed: Vec<&diacraft_doc::Section> = Vec::new();
        // The sections of the pages that read the document's layer set (a page of the file with no set of its own, and a page the save adds), in the model's order, each with
        // the section the page had when the drawing was read (a row is changed when it is not what the page had).
        let mut reading: Vec<(&diacraft_doc::Section, Option<&diacraft_doc::Section>)> = Vec::new();
        for (at, page) in doc.pages.iter().enumerate() {
            let Some(section) = page.sheet.section(&key) else { continue };
            let matched = diff.structure.matched.get(at).copied().flatten();
            let markup = match matched {
                Some(h) => work.file_layers.get(h).cloned().flatten(),
                None => work.new_page_layers.clone(),
            };
            if matched.is_none_or(|h| !fresh.page(h).is_some_and(|p| p.own_layers())) {
                reading.push((section, matched.and_then(|h| read_layers(base, held, h))));
            }
            // One step for each row compared; no copy is made.
            work.charge_steps(section.rows.len())?;
            if !markup.is_some_and(|m| std::sync::Arc::ptr_eq(&m, section) || *m == **section) {
                changed.push(section);
            }
        }
        if !changed.is_empty() {
            let mut declared: HashSet<String> = HashSet::new();
            super::layers::names_of(work.new_page_layers.clone().as_deref(), &mut declared, work)?;
            for section in work.file_layers.clone().iter().flatten() {
                super::layers::names_of(Some(section), &mut declared, work)?;
            }
            diff.new_layers = super::layers::new_layers(&changed, &declared, work)?;
            // The layers the document's set declares whose flags the model changed: their `draw:display` and `draw:protected` are written, the row that changed
            // deciding. The model writer's package was made with the layer set the model asks for (`model/layer_set.rs` decided it, and no row of a drawing with no
            // file is a changed one): its splice writes none again.
            if !work.model_path {
                diff.layer_flags = super::layers::reflagged(&reading, work.new_page_layers.clone().as_deref(), work)?;
            }
        }
    }
    let fills = Fills::new(doc);
    // The comments of the model and of the file, by the page each is on, once.
    let (doc_comments, base_comments) = (super::comments::ByPage::of(&doc.comments, work)?, super::comments::ByPage::of(&base.comments, work)?);
    let mut added = 0usize;
    for (at, d) in doc.pages.iter().enumerate() {
        let Some(h) = diff.structure.matched.get(at).copied().flatten() else {
            // A page the file lacks: compared with a page of its own with no shapes, so that every shape of it is a new shape, written with the page.
            let blank = Page { sheet: std::sync::Arc::clone(&d.sheet), ..Page::new(d.id, &d.name) };
            let source = PageSource::Page(diff.structure.elements.saturating_add(added));
            added += 1;
            let none = PageShapes::new(source);
            // The layers the reader gives a shape on a page the save adds are the document's, with those the save adds to it.
            let markup = work.new_page_layers.clone();
            let mut page = compare_page(&fills, at, (d, &blank), (&none, &none), &none, (None, markup, &diff.new_layers), limits, work)?;
            page.held_at = None;
            let new_ids: HashSet<u32> = page.new.iter().map(|n| n.shape.id).collect();
            let ties = super::comments::Ties { held: &none, fresh: &none, new: &new_ids, deleted: &[], restructured: true };
            (page.comments, page.tied) = super::comments::edit_for(&doc_comments, d.id, &base_comments, None, &ties, work)?;
            diff.pages.push(page);
            continue;
        };
        let Some(b) = base.pages.get(h) else { return Err(WriteError::Mismatch("the index does not hold the pages of the drawing")) };
        // The page's `ID` in the file as it was read: its comments in the baseline are on it.
        let base_page_id = b.id;
        let (Some(held_page), Some(fresh_page)) = (held.page(h), fresh.page(h)) else {
            return Err(WriteError::Mismatch("the index does not hold the pages of the drawing"));
        };
        // A page a save made or changed is compared with the model as that save had it, not with the file read again, which gives
        // back more than the save wrote of a new shape and the master page a resized page was moved to.
        let saved = held_page.saved();
        let b: &Page = saved.map_or(&**b, |p| &**p);
        if !page_fields_carried(d, b, flat, &mut work.cells_compared) {
            return Err(WriteError::Unsupported("a change of a page's own fields, size or layers"));
        }
        let size = page_edit(d, b, &mut work.cells_compared)?;
        if held_page.source != fresh_page.source {
            return Err(WriteError::Mismatch("the index does not say where the pages came from"));
        }
        // The index of a file holds as many shape elements, and kept ones, as reading the file again finds.
        if held_page.elements() != fresh_page.elements() || held_page.kept().len() != fresh_page.kept().len() {
            return Err(WriteError::Mismatch("the index is not the file's: it holds other shape elements than the file does"));
        }
        // A background page's size is its layout's, which the page that names its master page shares: the plan of the pages refuses an edit of it that
        // the page does not make with it (`pages::plan`).
        // The shapes of the saved page are the model's, by the `ID`s the held index knows; the file's own numbering is the same elements.
        let indexes = if saved.is_some() { (held_page, held_page) } else { (held_page, fresh_page) };
        // The layers the markup of the page gives it (its own layer set, else the document's, with those the save adds to it), which a shape's `draw:layer` is resolved against.
        let markup = work.file_layers.get(h).cloned().flatten();
        let added: &[super::layers::NewLayer] = if fresh_page.own_layers() { &[] } else { &diff.new_layers };
        // A page with a layer set of its own has the flags the model changed written in that set, as the document's set has: its own rows against what it had.
        let own_flags = match (fresh_page.own_layers() && !flat, d.sheet.section(&diacraft_doc::SectionKey::new("Layer", None))) {
            (true, Some(section)) if !markup.as_ref().is_some_and(|m| std::sync::Arc::ptr_eq(m, section) || **m == **section) => {
                super::layers::reflagged(&[(&**section, read_layers(base, held, h))], markup.as_deref(), work)?
            }
            _ => Vec::new(),
        };
        let mut page = compare_page(&fills, at, (d, b), indexes, fresh_page, (size, markup, added), limits, work)?;
        page.held_at = Some(h);
        page.layer_flags = own_flags;
        let new_ids: HashSet<u32> = page.new.iter().map(|n| n.shape.id).collect();
        let restructured = !page.new.is_empty() || !page.deleted.is_empty() || !page.levels.is_empty();
        let ties = super::comments::Ties { held: held_page, fresh: fresh_page, new: &new_ids, deleted: &page.deleted, restructured };
        (page.comments, page.tied) = super::comments::edit_for(&doc_comments, d.id, &base_comments, Some(base_page_id), &ties, work)?;
        diff.pages.push(page);
    }
    Ok(diff)
}

/// The `Layer` section the page `h` of the file had when the drawing was read, DiaCraft's data included: the model a save made or changed the page with, when one did (Ruling
///), else the baseline's page.
fn read_layers<'b>(base: &'b Document, held: &'b ShapeIndex, h: usize) -> Option<&'b diacraft_doc::Section> {
    let page: &Page = match held.page(h).and_then(|p| p.saved()) {
        Some(saved) => saved,
        None => base.pages.get(h)?,
    };
    page.sheet.section(&diacraft_doc::SectionKey::new("Layer", None)).map(|s| &**s)
}

/// The nodes of the model as it is and of the baseline, and the ordinals the baseline's modelled shapes have.
struct Nodes<'a> {
    doc: Vec<Node<'a>>,
    base: Vec<Node<'a>>,
    /// The baseline node of each ordinal.
    by_ord: HashMap<u32, usize>,
}

fn nodes_of<'a>(
    doc: &'a Page,
    base: &'a Page,
    indexes: (&PageShapes, &PageShapes),
    limits: &Limits,
    work: &mut WriteWork,
) -> Result<Nodes<'a>, WriteError> {
    let (held, fresh) = indexes;
    let doc_nodes = match walk(&doc.shapes, &|id| held.ordinal(id), true, limits, work)? {
        Ok(nodes) => nodes,
        Err(_) => {
            return Err(WriteError::Unsupported("a new shape put in a group of the file (a member added to a group the file holds is not written)"));
        }
    };
    let base_nodes = match walk(&base.shapes, &|id| fresh.ordinal(id), false, limits, work)? {
        Ok(nodes) => nodes,
        Err(_) => return Err(WriteError::Mismatch("the file holds a shape its index does not")),
    };
    let by_ord: HashMap<u32, usize> = base_nodes.iter().enumerate().map(|(i, n)| (n.ord, i)).collect();
    if by_ord.len() != base_nodes.len() {
        return Err(WriteError::Mismatch("two shapes of the file have one element"));
    }
    let doc_ords: HashSet<u32> = doc_nodes.iter().map(|n| n.ord).collect();
    if doc_ords.len() != doc_nodes.len() {
        return Err(WriteError::Mismatch("two shapes of the drawing have one element"));
    }
    Ok(Nodes { doc: doc_nodes, base: base_nodes, by_ord })
}

/// The page cells that differ between the model's page and the file's, with their values: `None` when the page's size, margins and orientation are
/// as they were. A cell that holds a formula is refused (its formula is tier 2).
pub(crate) fn page_edit(doc: &Page, base: &Page, steps: &mut u64) -> Result<Option<PageEdit>, WriteError> {
    let mut changes: Vec<(&'static str, Option<CellVal>)> = Vec::new();
    for name in PAGE_CELLS {
        *steps = steps.saturating_add(1);
        if own::one_cell_same(&doc.sheet, &base.sheet, name) {
            continue;
        }
        let value = doc.sheet.cell(name).map(CellVal::of).transpose().map_err(WriteError::Unsupported)?;
        changes.push((name, value));
    }
    if changes.is_empty() {
        return Ok(None);
    }
    let number = |p: &Page| p.sheet.cell("PageHeight").and_then(|c| c.value.as_ref()?.number());
    let (height_was, height_now) = (number(base), number(doc));
    let words = WriteError::Unsupported("a page with no height");
    super::stylemap::page_ops(&changes).map_err(WriteError::Unsupported)?;
    Ok(Some(PageEdit { changes, height_was: height_was.ok_or(words.clone())?, height_now: height_now.ok_or(words)? }))
}

#[allow(clippy::too_many_arguments)]
fn compare_page(
    fills: &Fills<'_>,
    at: usize,
    (doc, base): (&Page, &Page),
    indexes: (&PageShapes, &PageShapes),
    tier2_in_file: &PageShapes,
    (size, markup, added): (Option<PageEdit>, Option<std::sync::Arc<diacraft_doc::Section>>, &[super::layers::NewLayer]),
    limits: &Limits,
    work: &mut WriteWork,
) -> Result<PageDiff, WriteError> {
    let height_was =
        base.sheet.cell("PageHeight").and_then(|c| c.value.as_ref()?.number()).ok_or(WriteError::Unsupported("a page with no height"))?;
    let height = size.as_ref().map_or(height_was, |s| s.height_now);
    let mut page = PageDiff {
        page_at: at,
        held_at: None,
        source: indexes.1.source,
        height,
        height_was,
        size,
        unit: page_unit(base),
        elements: indexes.1.elements(),
        shapes: Vec::new(),
        deleted: Vec::new(),
        levels: Vec::new(),
        unglue: Vec::new(),
        survivors: Vec::new(),
        new: Vec::new(),
        new_connects: Vec::new(),
        layers: super::layers::PageLayers::default(),
        unkept: Vec::new(),
        glued_to_groups: Vec::new(),
        one_flip: HashSet::new(),
        layer_flags: Vec::new(),
        comments: None,
        tied: Vec::new(),
    };
    let nodes = nodes_of(doc, base, indexes, limits, work)?;
    // What the page lost and gained besides its shapes' own cells, before the shapes are compared: the shapes the model deleted, the levels it reordered, the new
    // shapes, and the glue that went (a connector's end formulas change with it).
    page.deleted = deleted(&nodes);
    page.levels = levels(&nodes);
    page.new = news_of(doc, indexes.0)?;
    // The layers a new shape is written on are looked at only for a page that gains a shape.
    if !page.new.is_empty() {
        page.layers = super::layers::PageLayers::of(markup.as_deref(), &doc.sheet, added, work)?;
    }
    let new_ids: HashSet<u32> = page.new.iter().map(|n| n.shape.id).collect();
    (page.unglue, page.new_connects) = unglued(doc, base, &nodes, indexes, &page.deleted, &new_ids)?;
    // The shapes new connectors are glued to that have one flip: one look at each shape of the page, when a new connector is glued to any.
    if !page.new_connects.is_empty() {
        let targets: HashSet<u32> = page.new_connects.iter().map(|c| c.to_sheet).collect();
        let one_flip = |sheet: &diacraft_doc::Sheet| own::flips_of(sheet).is_some_and(|(x, y)| x != y);
        work.order_steps = work.order_steps.saturating_add((nodes.doc.len() + page.new.len()) as u64);
        let of_file = nodes.doc.iter().map(|n| (n.id, &*n.shape.sheet));
        let of_new = page.new.iter().map(|n| (n.shape.id, &*n.shape.sheet));
        page.one_flip = of_file.chain(of_new).filter(|(id, sheet)| targets.contains(id) && one_flip(sheet)).map(|(id, _)| id).collect();
    }
    let glue_off: HashSet<u32> = page.unglue.iter().map(|(ord, _)| *ord).collect();
    // The shapes read as copies, by `ID`, indexed once (each is one look, charged as the page's other lookups are).
    work.order_steps = work.order_steps.saturating_add(tier2_in_file.copies().len() as u64);
    let copies: HashSet<u32> = tier2_in_file.copies().iter().copied().collect();
    let mut changed_own = vec![false; nodes.doc.len()];
    let mut changed_above = vec![false; nodes.doc.len()];
    // The rows each shape has connectors glued to, by `ID`: made from the page's connections the first time a turned shape asks.
    let mut glued_rows: Option<HashMap<u32, Vec<u32>>> = None;
    // The page's layers, made for a page that gains a shape, and else when a shape of the file is put on a layer the save declares.
    let mut layers_made = !page.new.is_empty();
    for (i, d) in nodes.doc.iter().enumerate() {
        page.survivors.push((d.id, d.ord));
        let Some(&bi) = nodes.by_ord.get(&d.ord) else { return Err(WriteError::Mismatch("a shape of the drawing is not in the file")) };
        let b = nodes.base.get(bi).ok_or(WriteError::Mismatch("a shape of the drawing is not in the file"))?;
        let (parent_ord, base_parent_ord) =
            (d.parent.and_then(|p| nodes.doc.get(p)).map(|n| n.ord), b.parent.and_then(|p| nodes.base.get(p)).map(|n| n.ord));
        if parent_ord != base_parent_ord {
            return Err(WriteError::Unsupported(
                "a shape moved into or out of a group (a shape of the file grouped, or taken out of a group: moving an element of the file into another is not written)",
            ));
        }
        changed_own[i] = !pose_cells_same(&b.shape.sheet, &d.shape.sheet);
        changed_above[i] = d.parent.is_some_and(|p| changed_above.get(p).copied().unwrap_or(false) || changed_own.get(p).copied().unwrap_or(false));
        let dh = page.grew_by();
        let in_file = tier2_in_file.has_tier2(b.id);
        let copy = copies.contains(&b.id);
        let pair = Pair {
            base: b,
            doc: d,
            fills,
            pose_may_differ: changed_own[i] || changed_above[i] || dh != 0.0,
            dh,
            in_file,
            copy,
            glue_off: glue_off.contains(&d.ord),
            stale: tier2_in_file.stale(b.id),
            natives: tier2_in_file.natives(b.id),
            held: indexes.0,
        };
        if let Some(mut change) = classify(&pair, work)? {
            if matches!(change.frame, FrameChange::Reshape { .. }) && !change.group {
                change.turned_glue = turned_glue(doc, &pair, &mut glued_rows, work);
            }
            // **Plan **: a shape of the file that the model put on a layer the save itself declares gets its `draw:layer` written, so that another
            // program shows it on that layer; every other shape of the file keeps the layer it named. A group (`draw:g`) has no `draw:layer`.
            if !added.is_empty() && !change.group && !own::one_cell_same(&b.shape.sheet, &d.shape.sheet, "LayerMember") {
                if !layers_made {
                    page.layers = super::layers::PageLayers::of(markup.as_deref(), &doc.sheet, added, work)?;
                    layers_made = true;
                }
                let layer = page.layers.of_shape(&d.shape.sheet);
                work.order_steps = work.order_steps.saturating_add(added.len() as u64);
                if added.iter().any(|l| l.name == layer.name) {
                    change.layer = Some(layer.name);
                }
            }
            page.shapes.push(change);
        }
    }
    // The groups of the file the edit changed something under, whose frame is not the box of their members; and what is glued to the groups a note is made of.
    page.unkept = unkept_in_file(&nodes, &changed_own, &page.deleted, &mut work.group_boxes);
    if !page.unkept.is_empty() || page.new.iter().any(|n| !n.shape.children.is_empty()) {
        page.glued_to_groups = glued_to_groups(doc, &nodes, &page.new);
    }
    Ok(page)
}

/// What connectors are glued to of a shape of the file the model turned ([`TurnedGlue`]): `None` when its turn is the file's, or no connector is glued to a row of it. The
/// page's connections are put in `glued` by the shape they glue to once, on the first ask (one pass, counted), and each ask is then one look.
fn turned_glue(doc: &Page, pair: &Pair<'_>, glued: &mut Option<HashMap<u32, Vec<u32>>>, work: &mut WriteWork) -> Option<TurnedGlue> {
    let (b, d) = (&pair.base.shape.sheet, &pair.doc.shape.sheet);
    let (old, new) = (own::pose_2d(b, &pair.base.outer)?, own::pose_2d(d, &pair.doc.outer)?);
    if old.lin.iter().zip(&new.lin).all(|(x, y)| (x - y).abs() <= 1e-9) {
        return None;
    }
    let map = glued.get_or_insert_with(|| {
        work.order_steps = work.order_steps.saturating_add(doc.connects.len() as u64);
        let mut map: HashMap<u32, Vec<u32>> = HashMap::new();
        for c in &doc.connects {
            if let Some(row) = c.to_part.and_then(|p| p.checked_sub(100)) {
                map.entry(c.to_sheet).or_default().push(row);
            }
        }
        map
    });
    let rows = map.get(&pair.doc.id)?;
    let held = |row: u32| {
        b.sections
            .iter()
            .filter(|s| s.name.as_str() == "Connection")
            .flat_map(|s| s.rows.iter())
            .any(|r| r.ix == Some(row) && r.ext.get().and_then(|e| e.odf.held.as_ref()).is_some_and(|h| h.group == diacraft_doc::HeldGroup::Glue))
    };
    work.order_steps = work.order_steps.saturating_add(rows.len() as u64);
    let glue = TurnedGlue { elements: rows.iter().any(|r| held(*r)), standard: rows.iter().any(|r| *r < 4) };
    (glue != TurnedGlue::default()).then_some(glue)
}

/// The `ID`s of the groups of the file whose frame is not the box of their members, **among the groups that this save changed something under**: a member (at any depth)
/// whose pose cells differ from the file's, a member the model took away, or the group's own pose cells. The file holds a group as its members and the reader gives the box
/// of them, so a group the model holds to a size (`LockCalcWH`, a margin) has a frame the file does not keep, which a member that moved or turned makes a different box from
/// the one the last save wrote (the frame is then off by what the member moved). A group that nothing under it changed is not looked at: a note said so when it was written,
/// and a page edited elsewhere says nothing of it. One pass over the page's nodes, last to first (a member comes after its group), each once, **counted in `steps`**; none
/// when nothing changed a pose or took a shape away. The boxes are the leaves' (a member group counts by its own members' box, carried by its own place, and not by its own
/// frame), as for a new group ([`super::groups::spans_inside`]); a group whose frame or whose members' are not numbers is no group to tell of (the comparison refuses what it
/// cannot write).
fn unkept_in_file(nodes: &Nodes<'_>, changed_own: &[bool], deleted: &[u32], steps: &mut u64) -> Vec<u32> {
    use super::groups::{Span, extent, frame_is_box};
    let n = nodes.doc.len();
    if n == 0 || (deleted.is_empty() && !changed_own.iter().any(|c| *c)) {
        return Vec::new();
    }
    // Something at or under a node changed: its own pose, or a shape the model took away from it.
    let mut below = vec![false; n];
    if !deleted.is_empty() {
        let doc_at: HashMap<u32, usize> = nodes.doc.iter().enumerate().map(|(i, node)| (node.ord, i)).collect();
        for ord in deleted {
            let parent = nodes.by_ord.get(ord).and_then(|b| nodes.base.get(*b)).and_then(|b| b.parent).and_then(|p| nodes.base.get(p)).map(|p| p.ord);
            if let Some(at) = parent.and_then(|o| doc_at.get(&o)).copied()
                && let Some(slot) = below.get_mut(at)
            {
                *slot = true;
            }
        }
    }
    let mut inside = vec![Span::Empty; n];
    for (i, node) in nodes.doc.iter().enumerate().rev() {
        *steps = steps.saturating_add(1);
        let Some(parent) = node.parent else { continue };
        let span = if node.shape.children.is_empty() {
            extent(node.shape).map_or(Span::Unreadable, Span::Box)
        } else {
            inside.get(i).copied().unwrap_or(Span::Empty).carried(own::local_to_parent(&node.shape.sheet))
        };
        if let Some(slot) = inside.get_mut(parent) {
            *slot = slot.join(span);
        }
        if (changed_own.get(i).copied().unwrap_or(false) || below.get(i).copied().unwrap_or(false))
            && let Some(slot) = below.get_mut(parent)
        {
            *slot = true;
        }
    }
    let mut out = Vec::new();
    for (i, node) in nodes.doc.iter().enumerate() {
        if node.shape.children.is_empty() || !(changed_own.get(i).copied().unwrap_or(false) || below.get(i).copied().unwrap_or(false)) {
            continue;
        }
        // A one-dimensional group has ends and no frame to compare: it is no group the reader rebuilds.
        if node.shape.sheet.cell("BeginX").is_some() {
            continue;
        }
        let (Some(Span::Box(b)), Some(pose)) = (inside.get(i).copied(), own::pose_2d(&node.shape.sheet, &Affine::IDENTITY)) else { continue };
        if !frame_is_box(&node.shape.sheet, pose.size, b) {
            out.push(node.id);
        }
    }
    out
}

/// The connectors glued to a group of the page, to the group itself, as `(group, connector)` sorted and with no pair twice (a connector glued by both ends is one): for the
/// groups of the file in `nodes` and the new groups in `new`. One pass over the page's connections.
fn glued_to_groups(doc: &Page, nodes: &Nodes<'_>, new: &[NewShape]) -> Vec<(u32, u32)> {
    let groups: HashSet<u32> = nodes
        .doc
        .iter()
        .filter(|n| !n.shape.children.is_empty())
        .map(|n| n.id)
        .chain(new.iter().filter(|n| !n.shape.children.is_empty()).map(|n| n.shape.id))
        .collect();
    let mut out: Vec<(u32, u32)> = doc.connects.iter().filter(|c| groups.contains(&c.to_sheet)).map(|c| (c.to_sheet, c.from_sheet)).collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The ordinals of the baseline's modelled shapes the model no longer has.
fn deleted(nodes: &Nodes<'_>) -> Vec<u32> {
    let doc: HashSet<u32> = nodes.doc.iter().map(|n| n.ord).collect();
    let mut out: Vec<u32> = nodes.base.iter().map(|n| n.ord).filter(|o| !doc.contains(o)).collect();
    out.sort_unstable();
    out
}

/// The levels whose shapes the model has in another order than the baseline had them.
fn levels(nodes: &Nodes<'_>) -> Vec<Level> {
    let group = |list: &[Node<'_>]| {
        let mut by_parent: HashMap<u32, Vec<u32>> = HashMap::new();
        for n in list {
            let parent = n.parent.and_then(|p| list.get(p)).map_or(0, |p| p.ord);
            by_parent.entry(parent).or_default().push(n.ord);
        }
        by_parent
    };
    let (was, now) = (group(&nodes.base), group(&nodes.doc));
    let mut out = Vec::new();
    for (parent, current) in &now {
        let Some(before) = was.get(parent) else { continue };
        let kept: HashSet<u32> = current.iter().copied().collect();
        let before: Vec<u32> = before.iter().copied().filter(|o| kept.contains(o)).collect();
        if before != *current {
            out.push(Level { parent: *parent, was: before, now: current.clone() });
        }
    }
    out.sort_by_key(|l| l.parent);
    out
}

/// What the comparison of a page's connections gives: the connectors whose end lost its glue (and which end), and the new shapes' own connections.
type Unglued = (Vec<(u32, bool)>, Vec<Connect>);

/// The connections the model lost: each row the baseline has and the model does not, with the model's ends in the baseline's `ID`s. A
/// connection the model gained or changed is not written by this task.
fn unglued(
    doc: &Page,
    base: &Page,
    nodes: &Nodes<'_>,
    indexes: (&PageShapes, &PageShapes),
    deleted: &[u32],
    new_ids: &HashSet<u32>,
) -> Result<Unglued, WriteError> {
    // The connections that name a new shape are the new shapes' own; the others are compared with the file's. **A connection from a shape the file holds
    // to a new shape is neither**: the glue of a new connector is written with the connector, and a glue made anew on a connector of the file is not
    // written yet, so a save that let it pass would be made and the file would lack a glue the model has (the review: an undo that
    // put back a shape a save had deleted, with the glue of a connector the file holds).
    if doc.connects.iter().any(|c| !new_ids.contains(&c.from_sheet) && new_ids.contains(&c.to_sheet)) {
        return Err(WriteError::Unsupported(
            "a connector of the file glued to a new shape (a glue made anew on a connector of the file is written by a later version)",
        ));
    }
    let touches_new = |c: &Connect| new_ids.contains(&c.from_sheet) || new_ids.contains(&c.to_sheet);
    let new_connects: Vec<Connect> = doc.connects.iter().filter(|c| touches_new(c)).cloned().collect();
    let mine: Vec<&Connect> = doc.connects.iter().filter(|c| !touches_new(c)).collect();
    if mine.len() == base.connects.len() && mine.iter().zip(base.connects.iter()).all(|(a, b)| *a == b) {
        return Ok((Vec::new(), new_connects));
    }
    let (held, fresh) = indexes;
    let fresh_id: HashMap<u32, u32> = fresh.ordinal_map().iter().map(|(id, ord)| (*ord, *id)).collect();
    // A row of the model, in the `ID`s of the baseline.
    let to_base = |id: u32| held.ordinal(id).and_then(|ord| fresh_id.get(&ord).copied());
    let key = |c: &Connect| (c.from_sheet, c.from_cell.clone(), c.from_part, c.to_sheet, c.to_cell.clone(), c.to_part);
    let mut now = HashSet::new();
    for c in mine.iter().copied() {
        let (Some(from), Some(to)) = (to_base(c.from_sheet), to_base(c.to_sheet)) else {
            return Err(WriteError::Unsupported("a connection to a shape the file does not hold"));
        };
        now.insert(key(&Connect { from_sheet: from, to_sheet: to, ..c.clone() }));
    }
    let before: HashSet<_> = base.connects.iter().map(&key).collect();
    if now.iter().any(|k| !before.contains(k)) {
        return Err(WriteError::Unsupported("a connector glued or glued anew (a connection is written by a later task)"));
    }
    let dead: HashSet<u32> = deleted.iter().copied().collect();
    let by_id: HashMap<u32, u32> = nodes.base.iter().map(|n| (n.id, n.ord)).collect();
    let mut out = Vec::new();
    for c in base.connects.iter().filter(|c| !now.contains(&key(c))) {
        let Some(&ord) = by_id.get(&c.from_sheet) else { continue };
        if dead.contains(&ord) {
            continue;
        }
        match c.from_cell.as_deref() {
            Some("BeginX" | "BeginY") => out.push((ord, true)),
            Some("EndX" | "EndY") => out.push((ord, false)),
            _ => return Err(WriteError::Unsupported("a connection of a kind the writer does not know")),
        }
    }
    out.sort_unstable();
    out.dedup();
    Ok((out, new_connects))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use diacraft_doc::{Cell, CellName, Document, Sheet, Value};

    use super::*;

    fn sheet(cells: &[(&str, Cell)]) -> Sheet {
        let mut sheet = Sheet::default();
        for (name, cell) in cells {
            sheet.cells.insert(CellName::new(name), cell.clone());
        }
        sheet
    }

    fn number(n: f64) -> Cell {
        Cell::constant(Value::from_number(n).unwrap_or_else(|| Value::from_text("0")))
    }

    #[test]
    fn the_page_unit_is_the_unit_its_width_cell_was_read_in() {
        let page = |unit: Option<&str>| {
            let mut page = Page::new(0, "p");
            let cell = Cell { unit: unit.map(Arc::from), ..Cell::new() };
            Arc::make_mut(&mut page.sheet).cells.insert(CellName::new("PageWidth"), cell);
            page
        };
        let units =
            [(Some("MM"), LenUnit::Mm), (Some("IN"), LenUnit::In), (Some("PT"), LenUnit::Pt), (Some("P"), LenUnit::Pc), (Some("CM"), LenUnit::Cm)];
        for (unit, expected) in units {
            assert_eq!(page_unit(&page(unit)), expected, "{unit:?}");
        }
        assert_eq!(page_unit(&page(None)), LenUnit::Cm);
        assert_eq!(page_unit(&Page::new(0, "no width")), LenUnit::Cm, "a page with no width cell");
    }

    #[test]
    fn a_recalculation_of_a_page_s_or_the_document_s_sheet_is_no_edit_and_a_changed_formula_or_constant_is() {
        // L10: a cell with a formula is the same whatever the calculation made of it (its cached value); one without is the same by value.
        let formula = |f: &str, v: &str| Cell::read(Some(f), Some(v), None, None, &mut 1_000);
        let page = |cell: Cell| {
            let mut page = Page::new(0, "p");
            Arc::make_mut(&mut page.sheet).cells.insert(CellName::new("PageScale"), cell);
            page
        };
        let (mut steps, base) = (0u64, page(formula("Width*2", "2")));
        assert!(page_same(&page(formula("Width*2", "9")), &base, &mut steps), "another cached value of the same formula");
        assert!(steps > 0, "what is compared is counted");
        assert!(!page_same(&page(formula("Width*3", "2")), &base, &mut steps), "another formula");
        assert!(!page_same(&page(number(2.0)), &base, &mut steps), "a formula taken away");
        assert!(page_same(&page(number(2.0)), &page(number(2.0)), &mut steps));
        assert!(!page_same(&page(number(3.0)), &page(number(2.0)), &mut steps), "a constant is the same only by its value");
        assert!(page_same(&page(Cell::constant(Value::from_text("2.0"))), &page(number(2.0)), &mut steps), "the same number written another way");
        // The document's sheet the same way.
        let doc = |cell: Cell| {
            let mut doc = Document::empty();
            Arc::make_mut(&mut doc.sheet).cells.insert(CellName::new("DocLangID"), cell);
            doc
        };
        assert!(own::sheet_same(&doc(formula("1+1", "2")).sheet, &doc(formula("1+1", "3")).sheet, &mut steps));
        assert!(!own::sheet_same(&doc(formula("1+2", "2")).sheet, &doc(formula("1+1", "2")).sheet, &mut steps));
        assert!(!own::sheet_same(&doc(number(2.0)).sheet, &doc(number(3.0)).sheet, &mut steps));
        // In a package the document's own cells are carried by DiaCraft's document file: no refusal of a save.
        assert!(document_same(&doc(number(2.0)), &doc(number(3.0)), false, &[], &mut steps, false));
        // A flat drawing has no document file: its cells are compared, a change of them is refused.
        assert!(document_same(&doc(formula("1+1", "2")), &doc(formula("1+1", "3")), true, &[], &mut steps, false));
        assert!(!document_same(&doc(formula("1+2", "2")), &doc(formula("1+1", "2")), true, &[], &mut steps, false));
        assert!(!document_same(&doc(number(2.0)), &doc(number(3.0)), true, &[], &mut steps, false));
    }

    #[test]
    fn a_pose_is_the_same_by_value_or_by_number_and_by_every_cell() {
        let pose = |x: Cell, y: Cell| sheet(&[("PinX", x), ("PinY", y)]);
        let a = pose(number(1.0), number(2.0));
        assert!(pose_cells_same(&a, &a.clone()), "a sheet is the same as itself");
        assert!(pose_cells_same(&Sheet::default(), &Sheet::default()), "no pose cell in either");
        // The same number written another way is the same pose; another number is not, in either cell.
        let text = |t: &str| Cell::constant(Value::from_text(t));
        assert!(pose_cells_same(&a, &pose(text("1.0"), number(2.0))));
        assert!(pose_cells_same(&a, &pose(number(1.0), text("2.00"))));
        assert!(!pose_cells_same(&a, &pose(number(1.5), number(2.0))));
        assert!(!pose_cells_same(&a, &pose(number(1.0), number(2.5))));
        // A cell in one and not in the other, either way round.
        assert!(!pose_cells_same(&a, &sheet(&[("PinX", number(1.0))])));
        assert!(!pose_cells_same(&sheet(&[("PinX", number(1.0))]), &a));
        // A cell that is not a pose cell is no part of the comparison.
        assert!(pose_cells_same(&sheet(&[("Fill", number(1.0))]), &sheet(&[("Fill", number(2.0))])));
        // The same value under another formula is another pose.
        let formula = |f: &str| Cell::read(Some(f), Some("1"), None, None, &mut 1_000);
        assert!(pose_cells_same(&sheet(&[("PinX", formula("Width*0.5"))]), &sheet(&[("PinX", formula("Width*0.5"))])));
        assert!(!pose_cells_same(&sheet(&[("PinX", formula("Width*0.5"))]), &sheet(&[("PinX", formula("Width*0.25"))])));
    }

    /// A style sheet `id` holding `cells`, whose line link is `line` and fill link `fill`.
    fn style(id: u32, line: Option<u32>, fill: Option<u32>, cells: &[(&str, f64)]) -> Arc<diacraft_doc::Style> {
        let cells: Vec<(&str, Cell)> = cells.iter().map(|(n, v)| (*n, number(*v))).collect();
        let mut sheet = sheet(&cells);
        sheet.links.line = line;
        sheet.links.fill = fill;
        Arc::new(diacraft_doc::Style { id, sheet: Arc::new(sheet), ..diacraft_doc::Style::default() })
    }

    #[test]
    fn a_line_cell_is_found_through_the_line_links_and_a_fill_cell_through_the_fill_links() {
        let mut document = Document::empty();
        for s in [
            style(0, None, None, &[("LineWeight", 0.01), ("FillPattern", 1.0)]),
            style(5, Some(6), Some(7), &[]),
            style(6, Some(0), Some(0), &[("LineWeight", 0.03)]),
            style(7, Some(0), Some(0), &[("FillPattern", 2.0)]),
            style(8, Some(9), Some(9), &[]),
            style(9, Some(8), Some(8), &[]),
        ] {
            document.styles.push(s);
        }
        let fills = Fills::new(&document);
        let mut on = sheet(&[]);
        on.links.line = Some(5);
        on.links.fill = Some(5);
        let mut steps = 0u64;
        // The weight is the line chain's (5, then 6), the pattern the fill chain's (5, then 7): the two chains are not one.
        assert_eq!(fills.line_number(&on, "LineWeight", &mut steps), Some(0.03));
        assert_eq!(fills.number(&on, "FillPattern", &mut steps), Some(2.0));
        assert_eq!(fills.line_number(&on, "FillPattern", &mut steps), Some(1.0), "the root holds it, at the end of the line chain");
        assert_eq!(fills.number(&on, "LineWeight", &mut steps), Some(0.01), "and the weight at the end of the fill chain");
        // No sheet in the chain holds it; the root is where a chain ends; a link to no sheet ends it.
        assert_eq!(fills.line_number(&on, "BeginArrow", &mut steps), None);
        let mut unlinked = sheet(&[]);
        unlinked.links.line = Some(99);
        assert_eq!(fills.line_number(&unlinked, "LineWeight", &mut steps), None);
        // No link is the root's.
        assert_eq!(fills.line_number(&sheet(&[]), "LineWeight", &mut steps), Some(0.01));
        // A chain that goes round is followed to the cap, and no further.
        let mut round = sheet(&[]);
        round.links.line = Some(8);
        let mut counted = 0u64;
        assert_eq!(fills.line_number(&round, "LineWeight", &mut counted), None);
        assert_eq!(counted, MOST_STYLE_STEPS as u64);
    }

    #[test]
    fn a_palette_index_is_the_colour_the_document_names_by_it() {
        let document = Document::empty();
        let fills = Fills::new(&document);
        let named = fills.palette(0);
        assert!(named.is_some(), "the fixed palette names index 0");
        // The schema's colour is `#` and six hexadecimal digits (`Rgba::hex` gives the six, in capitals, and no hash): the palette hands the writer what it writes.
        let colour = document.color(0).unwrap();
        assert_eq!(named, Some(format!("#{:02x}{:02x}{:02x}", colour.r, colour.g, colour.b)));
        assert!(named.as_deref().is_some_and(|n| n.len() == 7 && n.starts_with('#')));
        assert_eq!(fills.palette(1_000_000), None, "an index no table holds");
    }
}
