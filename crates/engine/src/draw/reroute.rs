//! Which connectors are routed again after an edit, and which keep the route they have.
//!
//! Our rule (it replaces for the connectors a command moves). A connector is *routed again*
//! (`route.rs`) when a command has moved one of its ends and one of its ends is glued, whether DiaCraft made it or a file did. A connector made
//! by DiaCraft (it has `User.DiaCraftRoute`) is also routed when the command is about it (`connector.add`, `connector.reconnect`), whatever moved.
//! The rows a reroute writes replace the connector's `Geometry` section 0 rows and nothing else of the connector but its label (`label.rs`).
//!
//! **When a route is fixed** (the connector's ends still follow what they are glued to, by their formulas; only the route is left):
//!
//! * the page's `DynamicsOff` is not 0, for a connector that follows its shape (while it is 1, edits on the page
//!   reroute nothing); a command about the connector itself still routes it;
//! * the connector's `ConFixedCode` is 1, 2 or 3: the setting [`Reroute`] gives it. 0 reroutes freely, and so do 4, 5 and
//!   6 (they mark whole-shape glue in files) and any other number (the last row). **2, never,** keeps the route it has and moves only the
//!   ends. **1, as needed, and 3, on crossover,** do the same, **except that a leg that would pass through a shape the connector is glued to makes a new
//!   route** (the second cause of 1, a placeable shape coming to cross a leg, is not built: nothing in the engine knows a shape as an obstacle
//!   but the two the connector stands on). A route made so keeps its setting: 3 is a setting and the mark of a hand edit alike;
//! * the connector is on a locked layer (a shape on a locked layer is not moved or edited, and the specifications say
//!   nothing of its route): it is routed by nothing, not even a forced reset, and its line jumps are not made again (`jumps.rs`);
//! * for a connector of a file, its effective route style is centre to centre (16), which does not route (a straight one
//!   (`RouteStyle` 2) and a curved one (`LineRouteExt` 2) are routed, straight and curved: `style.rs`). The connector's own
//!   `ShapeRouteStyle` and `ConLineRouteExt` win over the page's cells when they are not 0.
//!
//! A connector the command is about and names **forced** (the Connectors menu, Reset Connector: `settle_forced`) is routed whatever else holds.
//!
//! **What the crossing test costs** (the security scan's findings). The test of a fixed route set to 1 or 3 is a **try**, as a routing is
//! (`settle::route_after_within`): it counts against the most connectors an edit tries (a refit of a group's [`super::MAX_REROUTED`], a plain edit's
//! [`super::MAX_ROUTED_PER_EDIT`]), and a connector past them keeps its route and is said, as one not routed again is. A test reads at most [`MAX_CROSSING_ROWS`] rows
//! of the route; **a route of more is held not to cross, and keeps the route it has, unread** (no route a person or DiaCraft draws has that many, line jumps and all).
//!
//! **What the tests and the routings of a call share** (a move of the shape ten thousand connectors were glued to searched the page three times for each
//! connector it routed and passed over the page's `Connect` rows twice, 300 million shapes visited in 6.9 s): the glue of the ends of every connector the call tries
//! is read in **one pass over the page's `Connect` rows**, the shapes they are glued to are found with the rest of the edit's shapes ([`EndIndex`]): no test and no
//! routing searches the page. The connection points of a shape an end is glued to are **read once for the call**, and a shape past [`MAX_CROSSING_POINT_ROWS`] for the
//! call has none (an end glued to it then has no way of its own and leaves toward the other end, as an end glued to a point that faces no way does).

use std::collections::{BTreeSet, HashMap, HashSet};

use diacraft_doc::{CellAddr, CellEdit, Document, ShapeRef, SheetRef, Value};

use super::Problem;
use super::connectors::{EndFacts, TO_PART_BASE};
use super::glue::{WHOLE_SHAPE, exit_at_among};
use super::hand::{MAX_HAND_POINTS, base_route_within};
use super::read::{GluedPoints, Reader};
use super::route::{Dir, P};
use super::route_clear;
use super::scan::{Located, scan};
use super::style::{PathStyle, path_style};

/// How far an end may differ between two documents and still be where it was.
const SAME_PLACE: f64 = 1.0e-9;

/// The most rows of a fixed route the crossing test reads: 16 times the most points a hand edit gives a route ([`MAX_HAND_POINTS`]), room for line jumps
/// on its legs. A route of more rows is no route a person or DiaCraft drew: it is not read, and is held not to cross.
pub const MAX_CROSSING_ROWS: usize = 16 * MAX_HAND_POINTS;

/// The most rows of `Connection` sections the crossing tests and the routings of one call read, over the shapes the ends are glued to, each shape once (and as many
/// for the placing of the ends calc cannot work out, `glue.rs`): 256 shapes of the [`crate::snap::MAX_CONNECTION_ROWS`] rows a snap reads of one, or 16,384 rectangles of
/// four points, or one shape of 65,536 points.
pub const MAX_CROSSING_POINT_ROWS: usize = 256 * crate::snap::MAX_CONNECTION_ROWS;

/// The cells of the two ends of a connector.
const END_CELLS: [&str; 4] = ["BeginX", "BeginY", "EndX", "EndY"];

/// Whether the page's `DynamicsOff` is set.
pub(crate) fn dynamics_off(reader: &Reader<'_>, page: u32) -> bool {
    reader.number(&SheetRef::Page(page), "DynamicsOff") != 0.0
}

/// When a connector may take a new path: what the Connector tab's Reroute list sets, in `ConFixedCode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reroute {
    /// 0: whenever the application sees a need; the dynamic connector's default.
    Freely,
    /// 1: when a leg would pass through a shape it is glued to, or a placeable shape comes to cross it.
    AsNeeded,
    /// 2: not at all, even when a leg passes through a shape it is glued to.
    Never,
    /// 3: only when a leg would pass through a shape it is glued to. A path changed by hand gives a connector this value.
    OnCrossover,
}

impl Reroute {
    /// The four, in the order of their codes.
    pub const ALL: [Reroute; 4] = [Reroute::Freely, Reroute::AsNeeded, Reroute::Never, Reroute::OnCrossover];

    /// The `ConFixedCode` that says it.
    pub const fn code(self) -> u8 {
        match self {
            Reroute::Freely => 0,
            Reroute::AsNeeded => 1,
            Reroute::Never => 2,
            Reroute::OnCrossover => 3,
        }
    }

    /// The word a command takes for it.
    pub const fn word(self) -> &'static str {
        match self {
            Reroute::Freely => "freely",
            Reroute::AsNeeded => "as_needed",
            Reroute::Never => "never",
            Reroute::OnCrossover => "on_crossover",
        }
    }

    /// The setting a word names (`freely`, `as_needed`, `never` or `on_crossover`, in any case, with `-` or a space for `_`).
    pub fn from_word(word: &str) -> Option<Reroute> {
        let word = word.trim().to_ascii_lowercase().replace(['-', ' '], "_");
        Reroute::ALL.into_iter().find(|r| r.word() == word)
    }

    /// The setting a `ConFixedCode` says: 1, 2 and 3 are theirs, and every other number (0, the 4, 5 and 6 that files hold for whole-shape glue, anything else, a
    /// number that is not one) reads as 0.
    pub fn from_code(code: f64) -> Reroute {
        if !code.is_finite() {
            Reroute::Freely
        } else if (code - 1.0).abs() < 0.5 {
            Reroute::AsNeeded
        } else if (code - 2.0).abs() < 0.5 {
            Reroute::Never
        } else if (code - 3.0).abs() < 0.5 {
            Reroute::OnCrossover
        } else {
            Reroute::Freely
        }
    }
}

/// The reroute setting of connector `id` on a page. `None` for a page or a shape that is not there and for a shape that is not a connector.
pub fn connector_reroute(doc: &Document, page: u32, id: u32) -> Option<Reroute> {
    let reader = Reader::new(doc);
    let sheet = SheetRef::Shape(reader.find(page, id)?);
    reader.is_one_d(&sheet).then(|| setting(&reader, &sheet))
}

/// The setting of a connector's own `ConFixedCode`.
fn setting(reader: &Reader<'_>, sheet: &SheetRef) -> Reroute {
    Reroute::from_code(reader.number(sheet, "ConFixedCode"))
}

/// Sets the reroute setting of connectors `ids` to `mode`, written locally (so that a master's value is overridden) as one edit, and routes nothing (the path stays until a
/// change of the glued shapes asks for another). A connector that has the setting already (a 4, 5 or 6 reads as freely) is not written. → how many
/// connectors changed. A shape that is not a connector is an error and nothing changes.
pub fn set_reroute(doc: &mut Document, page: u32, ids: &[u32], mode: Reroute) -> Result<usize, Problem> {
    let (_, found) = super::scan::named(doc, page, ids)?;
    let mut writes = Vec::new();
    {
        let reader = Reader::new(doc);
        for (id, at) in &found {
            let sheet = SheetRef::Shape(at.clone());
            if !reader.is_one_d(&sheet) {
                return Err(Problem::bad(format!("shape {id} is not a connector")));
            }
            if setting(&reader, &sheet) != mode {
                let code = Value::from_text(&mode.code().to_string());
                writes.push((sheet, CellAddr::direct("ConFixedCode"), CellEdit::Value(code)));
            }
        }
    }
    let changed = writes.len();
    super::write_cells(doc, writes)?;
    Ok(changed)
}

/// Whether the connector's own `ConFixedCode` keeps the route it has (1, 2 and 3: see the module documentation).
pub(crate) fn con_fixed(reader: &Reader<'_>, sheet: &SheetRef) -> bool {
    setting(reader, sheet) != Reroute::Freely
}

// How many `Connect` rows the crossing tests passed over to find the glue of the connectors they test, for the test that holds a call to one pass. Not kept outside tests.
#[cfg(test)]
thread_local! {
    pub(crate) static CONNECT_ROWS_READ: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn note_connect_read() {
    CONNECT_ROWS_READ.with(|c| c.set(c.get().saturating_add(1)));
}

#[cfg(not(test))]
pub(crate) fn note_connect_read() {}

/// Where an end of a connector is glued: the shape and the `ToPart` of its `Connect` row.
#[derive(Clone, Copy, Debug)]
struct Glued {
    to: u32,
    part: Option<u32>,
}

/// What the crossing tests and the routings of one call share (for the tests, for the routings): where each end of the connectors the call
/// tries is glued, from **one pass over the page's `Connect` rows** (the first row on either cell of an end, as `end_connect` read it for each end, which was a pass of
/// its own: two for each connector routed), the shapes those rows name found with the rest of the edit's shapes (`Located`, never a search of the page each), and the
/// connection points of each glued shape **read once for the call** ([`GluedPoints`], within [`MAX_CROSSING_POINT_ROWS`] rows for all of them).
#[derive(Debug)]
pub(crate) struct EndIndex {
    ends: HashMap<u32, [Option<Glued>; 2]>,
    points: GluedPoints,
}

impl EndIndex {
    /// The glue of connectors `ids` of page `page`, and the shapes it names looked up in `located` (one walk of the page for those not looked up yet).
    pub(crate) fn new(doc: &Document, page: u32, ids: &HashSet<u32>, located: &mut Located) -> Result<EndIndex, Problem> {
        let mut ends: HashMap<u32, [Option<Glued>; 2]> = HashMap::new();
        if let Some(p) = doc.page(page).filter(|_| !ids.is_empty()) {
            for c in p.connects.iter() {
                note_connect_read();
                if !ids.contains(&c.from_sheet) {
                    continue;
                }
                let end = match c.from_cell.as_deref() {
                    Some("BeginX" | "BeginY") => 0,
                    Some("EndX" | "EndY") => 1,
                    _ => continue,
                };
                if let Some(slot @ None) = ends.entry(c.from_sheet).or_default().get_mut(end) {
                    *slot = Some(Glued { to: c.to_sheet, part: c.to_part });
                }
            }
        }
        let shapes: Vec<u32> = ends.values().flatten().flatten().map(|glued| glued.to).collect();
        located.find(doc, page, shapes)?;
        Ok(EndIndex { ends, points: GluedPoints::new(MAX_CROSSING_POINT_ROWS) })
    }

    /// Whether the route of connector `id` as it is now passes through a shape the connector is glued to: a leg goes into the box of the shape at an
    /// end, the legs on the boxes themselves held as [`route_clear::crosses`] holds them. A connector with no path of lines has no leg to cross, and one whose route has
    /// more than [`MAX_CROSSING_ROWS`] rows is held not to cross, unread.
    pub(crate) fn crosses(&mut self, reader: &Reader<'_>, located: &Located, id: u32) -> bool {
        let Some(at) = located.get(&id) else { return false };
        let sheet = SheetRef::Shape(at.clone());
        let Ok(Some(points)) = base_route_within(reader, &sheet, &reader.pose(&sheet), MAX_CROSSING_ROWS) else { return false };
        let (begin, end) =
            ((reader.number(&sheet, "BeginX"), reader.number(&sheet, "BeginY")), (reader.number(&sheet, "EndX"), reader.number(&sheet, "EndY")));
        let (from, to) = self.end_facts(reader, located, at, id, begin, end);
        route_clear::crosses(&points, begin, from.dir, end, to.dir, from.quad.as_ref(), to.quad.as_ref())
    }

    /// What a route needs to know of the two ends of connector `id` (at `connector`), whose begin is at `begin` and end at `end`: the way each leaves and the box of the
    /// shape it is glued to (rule 7), from the glue read and the points read once.
    pub(crate) fn end_facts(
        &mut self,
        reader: &Reader<'_>,
        located: &Located,
        connector: &ShapeRef,
        id: u32,
        begin: P,
        end: P,
    ) -> (EndFacts, EndFacts) {
        let [first, last] = self.ends.get(&id).copied().unwrap_or_default();
        (self.facts(reader, located, connector, first, begin, end), self.facts(reader, located, connector, last, end, begin))
    }

    /// What the test or the route needs of an end at `at`, the other end being at `other`: the direction of the connection point it is glued to, through the shape's
    /// turns, flips and groups and snapped to an axis; an end glued to the whole of a shape (`ToPart` 3) leaves by the point of the shape it is at
    /// ([`exit_at_among`]); and the box of the shape it is glued to, in the frame of the connector's parent.
    fn facts(&mut self, reader: &Reader<'_>, located: &Located, connector: &ShapeRef, glued: Option<Glued>, at: P, other: P) -> EndFacts {
        let none = EndFacts { dir: None, quad: None };
        let Some(glued) = glued else { return none };
        let Some(target) = located.get(&glued.to) else { return none };
        let dir = match glued.part {
            Some(part) if part >= TO_PART_BASE => {
                let index = part - TO_PART_BASE;
                self.points.points(reader, glued.to, target).iter().find(|p| p.index == index).and_then(|p| Dir::from_vector(p.dir_x, p.dir_y))
            }
            Some(WHOLE_SHAPE) => exit_at_among(reader, target, self.points.points(reader, glued.to, target), at, other),
            _ => None,
        };
        let quad = reader.corners(target).map(|corner| reader.point_from_page(connector, corner));
        EndFacts { dir, quad: Some(quad) }
    }
}

/// Whether the connector of a file keeps the route it has (centre to centre).
pub(crate) fn not_routable(reader: &Reader<'_>, page: u32, sheet: &SheetRef) -> bool {
    path_style(reader, page, sheet) == PathStyle::CentreToCentre
}

/// Whether a command moved an end of connector `id`: its end cells differ between `old` (the document as the command found it, whose shapes are where
/// `old_paths` says) and now, or it was not there before.
fn ends_moved(reader: &Reader<'_>, sheet: &SheetRef, old: &Reader<'_>, old_paths: &HashMap<u32, ShapeRef>, id: u32) -> bool {
    let Some(old_at) = old_paths.get(&id) else { return true };
    let old_sheet = SheetRef::Shape(old_at.clone());
    END_CELLS.iter().any(|name| (reader.number(sheet, name) - old.number(&old_sheet, name)).abs() > SAME_PLACE)
}

/// The connectors that have a `Connect` row on one of their end cells: the ones with a glued end.
fn glued_connectors(doc: &Document, page: u32) -> BTreeSet<u32> {
    doc.page(page)
        .map(|p| p.connects.iter().filter(|c| c.from_cell.as_deref().is_some_and(|name| END_CELLS.contains(&name))).map(|c| c.from_sheet).collect())
        .unwrap_or_default()
}

/// A connector [`connectors_to_route`] hands on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ToRoute {
    pub id: u32,
    /// A fixed route set to reroute as needed or on crossover: it is routed again only if a leg of it crosses a shape the connector is glued to
    /// ([`EndIndex::crosses`]), and then it keeps its setting.
    pub check: bool,
}

/// The connectors to route again after a command, and those to route again if their route crosses a shape they are glued to, in the order of their `ID`s. `reach`
/// is the shapes the command changed and everything inside them; `touched` and `extra` are the shapes it wrote and the connectors it is about (see
/// [`super::settle`]), and `located` the shapes `settle` looked up. `before` is the document as the command found it. A candidate not looked up yet is in one more
/// walk of the page (`Located::find`), and the connectors of a file whose old ends are to be compared in one walk of the page before, only when there is one: not
/// one search each, as a guide that carries a thousand shapes makes a thousand subjects. The crossing tests are not made here: they are tries, made within the
/// routing's limit.
#[allow(clippy::too_many_arguments)]
pub(crate) fn connectors_to_route(
    doc: &Document,
    before: &Document,
    page: u32,
    reach: &BTreeSet<u32>,
    touched: &[u32],
    extra: &[u32],
    forced: &[u32],
    located: &mut Located,
) -> Result<Vec<ToRoute>, Problem> {
    let Some(p) = doc.page(page) else { return Ok(Vec::new()) };
    let subjects: BTreeSet<u32> = touched.iter().chain(extra).chain(forced).copied().collect();
    let forced: BTreeSet<u32> = forced.iter().copied().collect();
    // A connector follows a shape it is glued to that the command changed, and a connector inside a shape the command changed (a group that is
    // turned or flipped: its frame changes, and an end glued to a shape outside it is somewhere else in that frame).
    let followers: BTreeSet<u32> =
        p.connects.iter().filter(|c| reach.contains(&c.to_sheet) || reach.contains(&c.from_sheet)).map(|c| c.from_sheet).collect();
    let candidates: BTreeSet<u32> = subjects.union(&followers).copied().collect();
    located.find(doc, page, candidates.iter().copied())?;
    let path_of = |id: &u32| located.get(id).cloned();
    let reader = Reader::new(doc);
    let glued = glued_connectors(doc, page);
    let off = dynamics_off(&reader, page);
    // Routed for what they are; a connector of a file that is glued and may be routed, if one of its ends moved (`compared`).
    let mut routed: Vec<u32> = Vec::new();
    let mut compared: Vec<(u32, SheetRef)> = Vec::new();
    // The connectors routed again if a leg would cross a shape they are glued to: they keep the setting that said so.
    let mut checked: Vec<u32> = Vec::new();
    // The locked layers of the page, read once, and each connector judged by its own list.
    let mut locks = super::layers::Locks::default();
    for id in &candidates {
        let Some(at) = path_of(id) else { continue };
        // **A connector on a locked layer keeps the route it has** (a shape on a locked layer is not moved or edited, and its route is rows of
        // its own): its glued ends follow the shapes they are glued to by their formulas, as a fixed route's do, and nothing asks it to be routed, not even a command about it
        // (the registry refuses a command that names it, `cmd/locked.rs`).
        if let Some(shape) = doc.shape(&at)
            && locks.of(doc, page, shape)?.is_some()
        {
            continue;
        }
        let sheet = SheetRef::Shape(at);
        if !reader.is_one_d(&sheet) {
            continue;
        }
        if forced.contains(id) {
            routed.push(*id);
            continue;
        }
        let subject = subjects.contains(id);
        // While the page's dynamics are off a connector follows its shape and nothing more; one the command is about is routed.
        if off && !subject {
            continue;
        }
        let ours = reader.is_routed(&sheet);
        // A fixed route is left, but a connector of ours that the command is about (a new one) has to have a route at all, and one set to reroute as needed
        // or on crossover has a new route when a leg of the one it has would pass through a shape it is glued to: it is tested.
        if con_fixed(&reader, &sheet) {
            if ours && subject {
                routed.push(*id);
            } else if matches!(setting(&reader, &sheet), Reroute::AsNeeded | Reroute::OnCrossover) && glued.contains(id) {
                checked.push(*id);
            }
            continue;
        }
        if ours {
            routed.push(*id);
        } else if glued.contains(id) && !not_routable(&reader, page, &sheet) {
            compared.push((*id, sheet));
        }
    }
    if !compared.is_empty() {
        // The page as the command found it: a page the command added has none of them.
        let wanted: HashSet<u32> = compared.iter().map(|(id, _)| *id).collect();
        let old_paths = if before.page(page).is_some() { scan(before, page, &wanted)?.found } else { HashMap::new() };
        let old = Reader::new(before);
        routed.extend(compared.into_iter().filter(|(id, sheet)| ends_moved(&reader, sheet, &old, &old_paths, *id)).map(|(id, _)| id));
    }
    let mut out: Vec<ToRoute> = routed.into_iter().map(|id| ToRoute { id, check: false }).collect();
    out.extend(checked.into_iter().map(|id| ToRoute { id, check: true }));
    out.sort_unstable_by_key(|next| next.id);
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// 0 freely, 1 as needed, 2 never, 3 on crossover; 4, 5, 6 and anything else read as 0.
    #[test]
    fn a_code_reads_as_its_setting_and_every_other_number_as_freely() {
        for (code, want) in [(0.0, Reroute::Freely), (1.0, Reroute::AsNeeded), (2.0, Reroute::Never), (3.0, Reroute::OnCrossover)] {
            assert_eq!(Reroute::from_code(code), want, "{code}");
            assert_eq!(Reroute::from_code(code + 0.4), want, "{code} and a rounding");
        }
        // (A number half way between two codes is no code's: it reads as freely.)
        for code in [4.0, 5.0, 6.0, 7.0, 255.0, -1.0, 1.5, 2.5, 1.0e300, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(Reroute::from_code(code), Reroute::Freely, "{code}");
        }
    }

    #[test]
    fn a_setting_goes_to_its_code_and_its_word_and_back() {
        for mode in Reroute::ALL {
            assert_eq!(Reroute::from_code(f64::from(mode.code())), mode);
            assert_eq!(Reroute::from_word(mode.word()), Some(mode));
        }
        assert_eq!(Reroute::from_word(" On-Crossover "), Some(Reroute::OnCrossover));
        assert_eq!(Reroute::from_word("AS NEEDED"), Some(Reroute::AsNeeded));
        for word in ["", "sometimes", "0", "free", "reroute freely"] {
            assert_eq!(Reroute::from_word(word), None, "{word:?}");
        }
        let codes: Vec<u8> = Reroute::ALL.iter().map(|r| r.code()).collect();
        assert_eq!(codes, [0, 1, 2, 3]);
    }

    /// The glue of the connectors to test is read in **one pass over the page's `Connect` rows**, whatever their number (a routing reads each end by a pass
    /// of its own), the first row on either cell of an end is the one that counts, as for the routing, and the shapes the rows name are found in one walk of the page.
    #[test]
    fn the_glue_of_the_connectors_tested_is_one_pass_over_the_connect_rows_and_one_walk_of_the_page() {
        use std::sync::Arc;

        use diacraft_doc::{Connect, Connects};
        use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

        let wires = 500_u32;
        let mut page = PageBuilder::new();
        for id in 1..=wires {
            page = page.shape(
                ShapeBuilder::new(id).cell_u("BeginX", "0", "IN").cell_u("BeginY", "0", "IN").cell_u("EndX", "1", "IN").cell_u("EndY", "1", "IN"),
            );
        }
        let mut doc = DocBuilder::new().page(1, page.shape(ShapeBuilder::new(9_001)).shape(ShapeBuilder::new(9_002))).build();
        let glue = |from: u32, cell: &str, to: u32, part: u32| Connect {
            from_sheet: from,
            from_cell: Some(Arc::from(cell)),
            to_sheet: to,
            to_part: Some(part),
            ..Connect::default()
        };
        let mut rows = Vec::new();
        for id in 1..=wires {
            rows.push(glue(id, "BeginX", 9_001, 100));
            // A second row on the same end: the first one counts.
            rows.push(glue(id, "BeginY", 9_002, 101));
            rows.push(glue(id, "EndY", 9_002, 3));
        }
        super::super::page_mut(&mut doc, 1).unwrap().connects = Connects::from_vec(rows);
        let tested: HashSet<u32> = (1..=wires / 2).collect();
        CONNECT_ROWS_READ.with(|c| c.set(0));
        super::super::scan::NODES_WALKED.with(|c| c.set(0));
        let mut located = Located::default();
        let crossings = EndIndex::new(&doc, 1, &tested, &mut located).unwrap();
        assert_eq!(CONNECT_ROWS_READ.with(|c| c.get()), 3 * wires as usize, "one pass over the rows");
        assert_eq!(super::super::scan::NODES_WALKED.with(|c| c.get()), wires as usize + 2, "one walk of the page");
        assert_eq!(crossings.ends.len(), tested.len());
        for id in &tested {
            let [begin, end] = crossings.ends.get(id).copied().unwrap();
            assert_eq!(begin.map(|g| (g.to, g.part)), Some((9_001, Some(100))), "connector {id}");
            assert_eq!(end.map(|g| (g.to, g.part)), Some((9_002, Some(3))), "connector {id}");
        }
        assert!(located.get(&9_001).is_some() && located.get(&9_002).is_some());
        // Nothing to test: no pass at all.
        CONNECT_ROWS_READ.with(|c| c.set(0));
        let none = EndIndex::new(&doc, 1, &HashSet::new(), &mut Located::default()).unwrap();
        assert!(none.ends.is_empty());
        assert_eq!(CONNECT_ROWS_READ.with(|c| c.get()), 0);
    }
}
