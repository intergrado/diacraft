//! Master matching, and bringing a master from one document into another (of the second M3 slice).
//!
//! **One place.** A master of an opened stencil dropped on a page, and a later paste from one drawing into another, both need to know
//! whether the target already holds the master, and if not to copy it in with what it depends on. That is [`bring_master`], and
//! [`find_same_master`] is the matching it starts with. Nothing else decides whether two masters are the same.
//!
//! # Matching (ours)
//!
//! `BaseID` names a master across drawings and `UniqueID` names it in one; a copy keeps both, and keeps its `UniqueID` unless the target holds
//! it already. A master `M` of the source is **the same** as a master `D` of the target when
//!
//! 1. `M`'s `UniqueID` is on no other master of its document and `D` has it, and their `BaseID`s are equal where both have one (the copy this
//!    code made earlier, or the same master from the same file); or else
//! 2. `M`'s `BaseID` is on no other master of its document and `D` has it (the same master by another drawing's `UniqueID`). Of several such
//!    `D` the one with `M`'s `NameU` is taken, else the first.
//!
//! An id that two masters of the source hold says nothing about either (and copies inside a stencil keep their `BaseID`), so a master
//! whose ids are both shared is matched by what it holds instead: a master of the target that has its `UniqueID` or `BaseID` and is, once
//! copied, exactly what would be added, is the one. The target's master is used as it is: whether the target's differs from the stencil's
//! is not looked at, since `BaseID` is the identity.
//!
//! # What is copied with a master
//!
//! A master's shapes refer to things outside it. For each, what is done (ours; the specs say only that a move "must
//! carry its styles and any theme, and map IDs"):
//!
//! * **Styles** (`LineStyle`, `FillStyle`, `TextStyle` of every shape and of the master's page sheet, and the links of the styles
//!   they reach): each style the master reaches is looked for in the target by name and definition (its sheet, with its own links mapped);
//!   if the target has it that is used, and if not it is added under a free `ID` (one above the highest). Only the copy's links are
//!   rewritten. The root style (`ID` 0) is the target's root whatever either holds. A link to a style the source lacks is dropped, and a
//!   cycle of styles is cut where it closes. Styles are never merged by name alone: V11 adds the style when the target's differs.
//! * **The colour table**: a colour cell (`FillForegnd`, `LineColor`, `Color` and the others of `COLOUR_CELLS`) that holds a plain whole number
//!   above 23 names a table entry. The entry is found in the target by its colour; if it is not there it is added under the same number when
//!   that is free, else under the lowest free number up to 253, and the cells of the copy are rewritten to it.
//! * **Face names**: a `Font`, `AsianFont` or `ComplexScriptFont` cell of a `Character` row names a face by its `NameU`; a face the target
//!   lacks is added to its table with the attributes it had.
//! * **A copy keeps no text of the stencil's ids**: the model keeps an `ID="05"`, a `LineStyle="03"`, a `Master` that is not the plain text of its number in
//!   `raw`, which wins on a write; the copy's master and styles have the `ID` the drawing gave them, a sheet whose link was rewritten keeps no text of the
//!   old one, and a master's shape names no master.
//! * **Themes** are not copied (until the writer can add a theme part): the writer cannot add a theme part, and a master has none of its own (a custom scheme rides in `User`
//!   rows of its shape, which are copied). Its themed cells read the target's theme.
//! * **Pictures, objects, embedded packages and any other reference to a part of the stencil's file** (a shape's `ForeignData` with a relationship,
//!   and anything the model keeps as the file had it that names a relationship, in the master, its page sheet, its shapes, its text and the styles it
//!   reaches): the master is **refused**, since a copy would name a relationship the saved drawing cannot write. A relationship
//!   attribute is found by its namespace's URI and not by its prefix (`refs.rs`).
//! * **Other masters named by `USE()`** are not followed; the answer says the master has such formulas.
//! * **Data connections and recordsets** of the stencil are not copied; the shape's own data rows are cells and are.
//! * **The master's part and relationship** are the target's to make: the copy has no `r:id`. What the master's own slot keeps beside its fields (the nodes of its part's `Shapes`
//!   and `Connects`, the prefixes the part's root declared, its ODF data) is copied, and walked and charged as the kept parts of its shapes are (`Survey::master_slot`).
//!
//! # Cost
//!
//! The copy walks the master once to see what it uses and, if a style or colour changes, once more to rewrite; a shape that nothing changes
//! is shared with the source (an `Arc`), not copied. Every node, cell and style visited is charged to a meter of [`MAX_BRING_NODES`], not
//! counted by calls, and a master past it is refused before anything is changed. No walk is recursive without the depth bound of the model
//! ([`MAX_SHAPE_DEPTH`]), and the style walk is iterative (a chain of tens of thousands of styles costs a vector, not a stack).

use std::collections::hash_map::RandomState;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::hash::{BuildHasher, Hash, Hasher};
use std::sync::Arc;

use diacraft_doc::{
    Cell, ColorEntry, Document, FaceName, FormulaSlot, Kept, Links, MAX_COLOR_ENTRIES, MAX_MASTERS, MAX_SHAPE_DEPTH, MAX_STYLES, Master, Passthrough,
    RawAttrs, RawNode, Row, Section, Shape, ShapeList, Sheet, Style, Text, TextPart, Value,
};

use super::refs;

/// The most shapes, cells, rows and styles one copy visits, all together. Real masters hold a few hundred; the largest the reader will take
/// holds millions, and this stops a copy of one from holding the engine.
pub const MAX_BRING_NODES: usize = 3_000_000;

/// The most face names a drawing's table holds after a copy.
pub const MAX_FACES: usize = 4_096;

// What the tests count of the work a copy does, for this thread: sheets compared with a style of the drawing, sheets fingerprinted, colour-table
// entries examined, and steps put on the stack of the walk of kept text. A test states the work as a count, which does not move with the load of the machine.
#[cfg(test)]
thread_local! {
    static SHEET_COMPARES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static SHEETS_FINGERPRINTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static COLOUR_ENTRIES_EXAMINED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static STEPS_PUSHED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The links a sheet's attributes may keep as text (`LineStyle="03"`), and that a rewritten link must not leave behind: raw wins on a write.
const LINK_ATTRS: [&str; 3] = ["LineStyle", "FillStyle", "TextStyle"];

/// The cells that hold a colour: a whole number above 23 in one of them is an entry of the colour table.
const COLOUR_CELLS: [&str; 11] = [
    "FillForegnd",
    "FillBkgnd",
    "ShdwForegnd",
    "ShdwBkgnd",
    "LineColor",
    "TextBkgnd",
    "Color",
    "GradientStopColor",
    "GlowColor",
    "BevelContourColor",
    "BevelDepthColor",
];

/// The cells of a `Character` row that name a face.
const FACE_CELLS: [&str; 3] = ["Font", "AsianFont", "ComplexScriptFont"];

/// A GUID as it is compared: trimmed, in capitals.
pub fn guid_key(text: &str) -> String {
    text.trim().to_ascii_uppercase()
}

fn same_guid(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// Whether a master's ids say which master it is among the masters of its own document: `UniqueID` is "different on each master" and
/// `BaseID` is kept by a copy, so each can be on more than one master of a hostile or a hand-copied file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    pub unique_is_unique: bool,
    pub base_is_unique: bool,
}

impl Identity {
    /// The identity of master `position` of `from`, counted in one pass over its masters. A caller that matches many masters of one document
    /// keeps the counts itself (`OpenedStencil::identity`).
    pub fn within(from: &Document, position: usize) -> Identity {
        let Some(master) = from.masters.get(position) else {
            return Identity { unique_is_unique: false, base_is_unique: false };
        };
        let (unique, base) = (master.unique_id.as_deref(), master.raw.get("BaseID"));
        let (mut unique_n, mut base_n) = (0_usize, 0_usize);
        for m in from.masters.iter() {
            unique_n += usize::from(matches!((unique, m.unique_id.as_deref()), (Some(a), Some(b)) if same_guid(a, b)));
            base_n += usize::from(matches!((base, m.raw.get("BaseID")), (Some(a), Some(b)) if same_guid(a, b)));
        }
        Identity { unique_is_unique: unique.is_some() && unique_n == 1, base_is_unique: base.is_some() && base_n == 1 }
    }
}

/// What a copy did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Brought {
    /// The master's `ID` in the target.
    pub id: u32,
    /// Whether it was copied in (and not found).
    pub added: bool,
    pub styles_added: usize,
    pub colours_added: usize,
    pub faces_added: usize,
    /// Things a person should know about the copy, in words.
    pub notes: Vec<String>,
}

/// Why a master was not brought in. The target is as it was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    NoSuchMaster,
    /// Its shapes hold this many pictures, objects, embedded packages or other references to a part of the stencil's package.
    Pictures(usize),
    /// Kept text declares more namespace prefixes than a copy reads (`refs::MAX_XMLNS_PER_ELEMENT`, `MAX_XMLNS_IN_SCOPE`).
    KeptTooComplex,
    /// More shapes, cells and styles than a copy visits.
    TooBig,
    /// The target already holds the most of this kind.
    Limit(&'static str),
    /// No colour number from 24 to 253 is free.
    ColourTableFull,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::NoSuchMaster => write!(f, "the stencil has no such master"),
            Refusal::Pictures(1) => {
                write!(
                    f,
                    "it holds a picture, an object or another part of the stencil's file, which a saved drawing cannot yet carry in from a stencil"
                )
            }
            Refusal::Pictures(n) => {
                write!(
                    f,
                    "it holds {n} pictures, objects or other parts of the stencil's file, which a saved drawing cannot yet carry in from a stencil"
                )
            }
            Refusal::TooBig => write!(f, "it is too large to copy (more than {MAX_BRING_NODES} shapes, rows, cells and styles)"),
            Refusal::Limit(what) => write!(f, "the drawing already holds as many {what} as it can"),
            Refusal::KeptTooComplex => write!(
                f,
                "it holds text of the stencil's file that declares more namespace prefixes than a copy reads ({} on one element, {} at once)",
                refs::MAX_XMLNS_PER_ELEMENT,
                refs::MAX_XMLNS_IN_SCOPE
            ),
            Refusal::ColourTableFull => write!(f, "the drawing's colour table has no number free for the master's colours"),
        }
    }
}

impl std::error::Error for Refusal {}

/// The `ID` of the master of `doc` that is the same as `source` (see the module documentation), if there is one.
pub fn find_same_master(doc: &Document, source: &Master, identity: Identity) -> Option<u32> {
    let (unique, base) = (source.unique_id.as_deref(), source.raw.get("BaseID"));
    if identity.unique_is_unique
        && let Some(u) = unique
    {
        let found = doc.masters.iter().find(|m| {
            m.unique_id.as_deref().is_some_and(|x| same_guid(x, u)) && !matches!((base, m.raw.get("BaseID")), (Some(a), Some(b)) if !same_guid(a, b))
        });
        if let Some(m) = found {
            return Some(m.id);
        }
    }
    if identity.base_is_unique
        && let Some(b) = base
    {
        let mut first = None;
        for m in doc.masters.iter() {
            if m.raw.get("BaseID").is_some_and(|x| same_guid(x, b)) {
                if source.name_u.is_some() && m.name_u == source.name_u {
                    return Some(m.id);
                }
                first.get_or_insert(m.id);
            }
        }
        return first;
    }
    None
}

/// Brings master `position` of `from` into `doc`: the master of `doc` that is the same, or a copy added with what it depends on (see the
/// module documentation). `doc` may be changed when this fails after it has begun to add; it fails before then for every reason but a
/// limit met while adding, which is checked first, so a caller that edits a copy (as `Session::edit` does) loses nothing.
pub fn bring_master(doc: &mut Document, from: &Document, position: usize, identity: Identity) -> Result<Brought, Refusal> {
    bring_with(doc, from, position, identity, MAX_BRING_NODES)
}

/// [`bring_master`] with a meter of `budget` nodes, which a test sets small.
pub(crate) fn bring_with(doc: &mut Document, from: &Document, position: usize, identity: Identity, budget: usize) -> Result<Brought, Refusal> {
    let source = from.masters.get(position).ok_or(Refusal::NoSuchMaster)?;
    if let Some(id) = find_same_master(doc, source, identity) {
        return Ok(Brought { id, ..Brought::default() });
    }
    let plan = plan(doc, from, source, budget)?;
    // A master that its ids do not tell (it has none, or each is on another master of its file too) is the one of the target that is exactly what
    // would be added, among those that share an id with it, or that have none as it has none and are called the same.
    let unidentified = !(identity.unique_is_unique || identity.base_is_unique);
    if unidentified && plan.styles.is_empty() && plan.colours.is_empty() && plan.faces.is_empty() {
        let (unique, base) = (source.unique_id.as_deref(), source.raw.get("BaseID"));
        let candidate = doc.masters.iter().find(|m| {
            let shares = matches!((unique, m.unique_id.as_deref()), (Some(a), Some(b)) if same_guid(a, b))
                || matches!((base, m.raw.get("BaseID")), (Some(a), Some(b)) if same_guid(a, b))
                || (unique.is_none() && base.is_none() && m.unique_id.is_none() && m.raw.get("BaseID").is_none() && m.name_u == source.name_u);
            shares && same_content(m, &plan.master)
        });
        if let Some(m) = candidate {
            return Ok(Brought { id: m.id, notes: plan.notes, ..Brought::default() });
        }
    }
    apply(doc, plan)
}

fn same_content(a: &Master, b: &Master) -> bool {
    a.name == b.name
        && a.name_u == b.name_u
        && a.pattern_flags == b.pattern_flags
        && a.icon == b.icon
        && a.sheet == b.sheet
        && a.shapes == b.shapes
        && a.connects == b.connects
        && a.raw == b.raw
        && a.passthrough == b.passthrough
}

// ---------------------------------------------------------------------------------------------------------------------------- the plan

/// What a copy will add, worked out against the target and not yet written.
struct Plan {
    /// The master to add, with its `ID` and `UniqueID` still to be given.
    master: Master,
    styles: Vec<Style>,
    colours: Vec<ColorEntry>,
    faces: Vec<FaceName>,
    notes: Vec<String>,
}

/// Counts what a copy visits against what it may.
struct Meter {
    used: usize,
    budget: usize,
}

impl Meter {
    fn charge(&mut self, n: usize) -> Result<(), Refusal> {
        self.used = self.used.saturating_add(n);
        if self.used > self.budget { Err(Refusal::TooBig) } else { Ok(()) }
    }
}

/// What the master's shapes and the styles they reach use.
struct Survey {
    meter: Meter,
    links: BTreeSet<u32>,
    colours: BTreeSet<u32>,
    faces: BTreeSet<String>,
    pictures: usize,
    uses: usize,
    /// Whether a shape of the master carries a `Master` or `MasterShape` (a hostile master part), which the copy must not.
    strips_masters: bool,
}

/// The styles of a document by `ID`, the first of two with one `ID` being the one found.
type StylesById<'a> = HashMap<u32, &'a Arc<Style>>;

/// Where a style of the source goes in the target: its `ID` there, or `None` for no link.
type StyleMap = HashMap<u32, Option<u32>>;

/// A colour cell's whole number above 23 when it holds only that: no formula, no theme.
fn plain_index(cell: &Cell) -> Option<u32> {
    if !matches!(cell.formula, FormulaSlot::Absent) {
        return None;
    }
    let n = cell.value.as_ref()?.number()?;
    (n.is_finite() && n.fract() == 0.0 && (24.0..1.0e9).contains(&n)).then_some(n as u32)
}

/// Whether a formula calls `USE(`: not as the tail of a longer name.
fn calls_use(source: &str) -> bool {
    let b = source.as_bytes();
    b.windows(4).enumerate().any(|(i, w)| {
        w.eq_ignore_ascii_case(b"USE(") && !i.checked_sub(1).and_then(|p| b.get(p)).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
    })
}

impl Survey {
    fn cell(&mut self, section: Option<&str>, name: &str, cell: &Cell) -> Result<(), Refusal> {
        if COLOUR_CELLS.contains(&name)
            && let Some(ix) = plain_index(cell)
        {
            self.colours.insert(ix);
        }
        if section == Some("Character") && FACE_CELLS.contains(&name) {
            let face = cell.value.as_ref().map(|v| v.text().trim()).filter(|t| !t.is_empty() && t.parse::<f64>().is_err());
            if let Some(face) = face {
                self.faces.insert(face.to_owned());
            }
        }
        if let FormulaSlot::Local(f) = &cell.formula {
            self.meter.charge(f.source.len() / 32)?;
            if calls_use(&f.source) {
                self.uses += 1;
            }
        }
        Ok(())
    }

    /// Elements kept as the file had them (what the model has no field for) that name a part of the package by a relationship: a copy would
    /// carry a reference to a part the drawing does not have. Counted as pictures are. The namespace is judged by its URI (`refs.rs`).
    /// **`owners` are the attributes of the typed elements the nodes are kept in**, the outermost first (a shape's `raw`, then a foreign object's
    /// attributes): their `xmlns:` declarations are in scope for the nodes, and stay so for the whole walk.
    fn kept_nodes<'a>(&mut self, owners: &[&'a RawAttrs], nodes: impl Iterator<Item = &'a RawNode>) -> Result<(), Refusal> {
        let mut nodes = nodes.peekable();
        if nodes.peek().is_none() {
            return Ok(());
        }
        let mut scope = refs::Scope::new();
        for attrs in owners {
            self.meter.charge(attrs.len())?;
            scope.enter_outer(attrs).map_err(|_| Refusal::KeptTooComplex)?;
        }
        self.walk(&mut scope, nodes)
    }

    /// Walks kept nodes in `scope`, which holds the declarations of the elements they are kept in. **Iterative, with one scope for the whole walk**:
    /// an element's declarations are pushed when it is entered and taken back when it is left, so an element of K declarations and N children costs
    /// K and N and not K times N (S7), and the declarations are charged to the meter and capped. **A node is charged when it is put on the walk's
    /// stack**, so a wide element is refused by the meter before its children are stacked and not after. The scope is as it was when the walk ends.
    fn walk<'a>(&mut self, scope: &mut refs::Scope<'a>, nodes: impl Iterator<Item = &'a RawNode>) -> Result<(), Refusal> {
        /// What the walk still has to do: enter a node, or leave the element whose bindings are cut back to a mark.
        enum Step<'a> {
            Enter(&'a RawNode),
            Leave(usize),
        }
        let mut work: Vec<Step<'a>> = Vec::new();
        for node in nodes {
            self.meter.charge(1)?;
            #[cfg(test)]
            STEPS_PUSHED.with(|c| c.set(c.get().saturating_add(1)));
            work.push(Step::Enter(node));
        }
        while let Some(step) = work.pop() {
            let node = match step {
                Step::Leave(mark) => {
                    scope.cut_back(mark);
                    continue;
                }
                Step::Enter(node) => node,
            };
            // The node was charged when it was stacked: an element's attributes are charged now, and its children before they are stacked.
            if let RawNode::Element(element) = node {
                self.meter.charge(element.attrs.len())?;
                let mark = scope.mark();
                scope.enter(&element.attrs).map_err(|_| Refusal::KeptTooComplex)?;
                if scope.any_relationship(&element.attrs) {
                    self.pictures += 1;
                }
                if !element.children.is_empty() {
                    self.meter.charge(element.children.len())?;
                    work.push(Step::Leave(mark));
                    #[cfg(test)]
                    STEPS_PUSHED.with(|c| c.set(c.get().saturating_add(element.children.len())));
                    work.extend(element.children.iter().map(Step::Enter));
                } else {
                    scope.cut_back(mark);
                }
            }
        }
        Ok(())
    }

    /// The elements, comments and instructions a typed element keeps besides what the model has a field for, with `owners` as for [`Survey::kept_nodes`].
    fn kept_parts<'a>(&mut self, kept: &'a Passthrough, owners: &[&'a RawAttrs]) -> Result<(), Refusal> {
        if kept.is_empty() {
            return Ok(());
        }
        self.kept_nodes(owners, kept.iter().map(|p| &p.node))
    }

    /// Kept elements of a typed element of a sheet, walked in the sheet's `scope`.
    fn kept_parts_in<'a>(&mut self, scope: &mut refs::Scope<'a>, kept: &'a Passthrough) -> Result<(), Refusal> {
        if kept.is_empty() {
            return Ok(());
        }
        self.walk(scope, kept.iter().map(|p| &p.node))
    }

    /// **What the copy keeps of the master's own slot** (`Master::leave_package` has taken out what names a place in the stencil's package): the nodes its `Shapes` and its `Connects` hold beside
    /// the shapes and the rows, the prefixes the root of its part declared, whether its row had a `PageSheet`, its ODF data and unknown `.diacraft` content (the security
    /// review). Each entry is charged and its attributes and nodes walked as every other kept part is, before the copy keeps them: the meter bounds them, and a relationship among them is
    /// counted in `pictures` (the caller takes them out, `refs::leave_package_for_another`, and says so). The prefixes of the root are in scope for every entry, as they are in the part.
    fn master_slot(&mut self, master: &Master) -> Result<(), Refusal> {
        let Some(slot) = master.ext.get() else { return Ok(()) };
        let root = slot.vsdx.0.iter().find(|(key, _)| &**key == "MasterContents").map(|(_, kept)| &kept.raw);
        for (key, kept) in &slot.vsdx.0 {
            self.meter.charge(1)?;
            // The root's own attributes are its own, and not an outer element's.
            self.kept_entry(if &**key == "MasterContents" { None } else { root }, kept)?;
        }
        self.kept_entry(None, &slot.odf)?;
        self.kept_entry(None, &slot.native)
    }

    /// One entry of a slot: its attributes and its nodes, with the declarations of `outer` (the root of the part it is in) in scope.
    fn kept_entry<'a>(&mut self, outer: Option<&'a RawAttrs>, kept: &'a Kept) -> Result<(), Refusal> {
        if kept.raw.is_empty() && kept.kept.is_empty() {
            return Ok(());
        }
        let mut scope = refs::Scope::new();
        if let Some(outer) = outer {
            self.meter.charge(outer.len())?;
            scope.enter_outer(outer).map_err(|_| Refusal::KeptTooComplex)?;
        }
        self.enter_attrs(&mut scope, &kept.raw)?;
        self.kept_parts_in(&mut scope, &kept.kept)
    }

    /// Attributes kept as the file had them: a relationship among them is a reference to a part, as an element's is.
    fn kept_attrs(&mut self, attrs: &RawAttrs) -> Result<(), Refusal> {
        if attrs.is_empty() {
            return Ok(());
        }
        self.meter.charge(attrs.len())?;
        if refs::attrs_refer(attrs).map_err(|_| Refusal::KeptTooComplex)? {
            self.pictures += 1;
        }
        Ok(())
    }

    /// Puts the declarations of an element of a sheet in the sheet's `scope`, and counts a relationship among its own attributes, judged with the
    /// declarations of every element above it in the sheet. The caller takes them back (`Scope::cut_back`) when it leaves the element.
    fn enter_attrs<'a>(&mut self, scope: &mut refs::Scope<'a>, attrs: &'a RawAttrs) -> Result<(), Refusal> {
        if attrs.is_empty() {
            return Ok(());
        }
        self.meter.charge(attrs.len())?;
        // Its own attributes are judged with its own declarations, which the copy gives back with them, and those of the elements above it, which
        // may only add a relationship binding (`Scope::enter_outer`). For what is kept below it, its declarations are an outer element's too.
        let mark = scope.mark();
        scope.enter(attrs).map_err(|_| Refusal::KeptTooComplex)?;
        if scope.any_relationship(attrs) {
            self.pictures += 1;
        }
        scope.cut_back(mark);
        scope.enter_outer(attrs).map_err(|_| Refusal::KeptTooComplex)?;
        Ok(())
    }

    /// Everything a cell keeps that the model has no field for, with the declarations of the cell in scope for it and taken back after.
    fn kept_by_cell<'a>(&mut self, scope: &mut refs::Scope<'a>, cell: &'a Cell) -> Result<(), Refusal> {
        if cell.raw.is_empty() && cell.extra.is_none() {
            return Ok(());
        }
        let mark = scope.mark();
        self.enter_attrs(scope, &cell.raw)?;
        if let Some(extra) = &cell.extra {
            self.kept_parts_in(scope, &extra.passthrough)?;
        }
        scope.cut_back(mark);
        Ok(())
    }

    /// `outer` is the attributes of the element the sheet is the sheet of (a shape's, a style's, a master's). **One scope serves the whole sheet**: the
    /// declarations of that element, then of the sheet, a section, a row and a cell, each in nesting order and taken back when its element is left, so
    /// that a binding to the relationships namespace on any element above a kept node in the same sheet counts, and one on a sibling is not seen.
    /// **These are outer declarations (`Scope::enter_outer`): they only add**, since the writer need not give them back at the element, and
    /// one that binds a prefix to another namespace does not lift the closed default (`r:id` under an `r` rebound there is still a relationship). The
    /// shapes above the sheet's (a group) are not followed.
    fn sheet<'a>(&mut self, sheet: &'a Sheet, outer: &'a RawAttrs, with_links: bool) -> Result<(), Refusal> {
        if with_links {
            self.links.extend([sheet.links.line, sheet.links.fill, sheet.links.text].into_iter().flatten());
        }
        self.meter.charge(1 + sheet.cells.len())?;
        let mut scope = refs::Scope::new();
        self.meter.charge(outer.len())?;
        scope.enter_outer(outer).map_err(|_| Refusal::KeptTooComplex)?;
        self.enter_attrs(&mut scope, &sheet.raw)?;
        self.kept_parts_in(&mut scope, &sheet.passthrough)?;
        for (name, cell) in &sheet.cells {
            self.cell(None, name.as_str(), cell)?;
            self.kept_by_cell(&mut scope, cell)?;
        }
        for section in &sheet.sections {
            let mark = scope.mark();
            self.section(&mut scope, section)?;
            scope.cut_back(mark);
        }
        Ok(())
    }

    fn section<'a>(&mut self, scope: &mut refs::Scope<'a>, section: &'a Section) -> Result<(), Refusal> {
        self.meter.charge(1 + section.cells.len())?;
        self.enter_attrs(scope, &section.raw)?;
        self.kept_parts_in(scope, &section.passthrough)?;
        let section_name = section.name.as_str();
        for (name, cell) in &section.cells {
            self.cell(Some(section_name), name.as_str(), cell)?;
            self.kept_by_cell(scope, cell)?;
        }
        for row in &section.rows {
            self.meter.charge(1 + row.cells.len())?;
            let mark = scope.mark();
            self.enter_attrs(scope, &row.raw)?;
            self.kept_parts_in(scope, &row.passthrough)?;
            for (name, cell) in &row.cells {
                self.cell(Some(section_name), name.as_str(), cell)?;
                self.kept_by_cell(scope, cell)?;
            }
            scope.cut_back(mark);
        }
        Ok(())
    }

    /// The text of a shape keeps elements the model has no field for (`TextPart::Raw`).
    fn kept_by_text<'a>(&mut self, text: &'a Text, owner: &'a RawAttrs) -> Result<(), Refusal> {
        // By reference: the nodes are walked where they are, and a large one is charged as it is walked and not copied first.
        self.kept_nodes(&[owner], text.parts.iter().filter_map(|part| if let TextPart::Raw(node) = part { Some(node) } else { None }))
    }

    fn shapes<'a>(&mut self, list: &'a ShapeList) -> Result<(), Refusal> {
        let mut work: Vec<(&'a ShapeList, usize)> = vec![(list, 1)];
        while let Some((level, depth)) = work.pop() {
            for shape in level.iter() {
                self.meter.charge(1)?;
                self.sheet(&shape.sheet, &shape.raw, true)?;
                self.kept_parts(&shape.passthrough, &[&shape.raw])?;
                self.kept_attrs(&shape.raw)?;
                if let Some(text) = &shape.text {
                    self.kept_by_text(text, &shape.raw)?;
                }
                if let Some(foreign) = &shape.foreign {
                    self.kept_parts(&foreign.passthrough, &[&shape.raw, &foreign.attrs])?;
                    self.kept_attrs(&foreign.attrs)?;
                    if foreign.rel_id.is_some() || foreign.picture.is_some() {
                        self.pictures += 1;
                    }
                }
                if shape.master.is_some()
                    || shape.master_shape.is_some()
                    || shape.raw.get("Master").is_some()
                    || shape.raw.get("MasterShape").is_some()
                {
                    self.strips_masters = true;
                }
                if depth < MAX_SHAPE_DEPTH && !shape.children.is_empty() {
                    work.push((&shape.children, depth + 1));
                }
            }
        }
        Ok(())
    }
}

/// The styles the survey's links reach in `from`, dependencies before the styles that are based on them, and the styles by `ID` (the first of
/// two with one `ID` is the one found, as everywhere). Iterative. A link to a style that is not there ends where it is; a cycle is cut where it
/// closes. The sheets of the styles are surveyed on the way (their colours and faces).
fn style_closure<'a>(from: &'a Document, roots: &BTreeSet<u32>, sv: &mut Survey) -> Result<(StylesById<'a>, Vec<u32>), Refusal> {
    if roots.iter().all(|r| *r == 0) {
        return Ok((HashMap::new(), Vec::new()));
    }
    sv.meter.charge(from.styles.len())?;
    let mut by_id: StylesById<'a> = HashMap::with_capacity(from.styles.len());
    for style in from.styles.iter() {
        by_id.entry(style.id).or_insert(style);
    }
    let mut seen: HashSet<u32> = HashSet::new();
    let mut order: Vec<u32> = Vec::new();
    for &root in roots {
        if root == 0 || !seen.insert(root) {
            continue;
        }
        let mut stack: Vec<(u32, u8)> = vec![(root, 0)];
        while let Some(&(id, step)) = stack.last() {
            let Some(style) = by_id.get(&id) else {
                stack.pop();
                continue;
            };
            if step == 0 {
                sv.meter.charge(1)?;
                sv.sheet(&style.sheet, &style.raw, false)?;
                sv.kept_attrs(&style.raw)?;
                sv.kept_parts(&style.passthrough, &[&style.raw])?;
            }
            if step < 3 {
                if let Some(top) = stack.last_mut() {
                    top.1 = step + 1;
                }
                let links = style.sheet.links;
                let dependency = match step {
                    0 => links.line,
                    1 => links.fill,
                    _ => links.text,
                };
                if let Some(d) = dependency
                    && d != 0
                    && seen.insert(d)
                {
                    stack.push((d, 0));
                }
                continue;
            }
            stack.pop();
            order.push(id);
        }
    }
    Ok((by_id, order))
}

/// What a copy maps: the style of the source to the style of the target (`None`: no link), and the colour numbers that change.
struct Maps {
    styles: StyleMap,
    colours: HashMap<u32, u32>,
    /// Whether any colour number changes.
    recolours: bool,
}

impl Maps {
    /// The link as the copy has it.
    fn link(&self, link: Option<u32>) -> Option<u32> {
        match link {
            None => None,
            Some(0) => Some(0),
            Some(id) => self.styles.get(&id).copied().flatten(),
        }
    }

    fn links(&self, links: Links) -> Links {
        Links { line: self.link(links.line), fill: self.link(links.fill), text: self.link(links.text) }
    }

    /// Whether the copy is the source's: no link and no colour changes.
    fn is_identity(&self, roots: &BTreeSet<u32>) -> bool {
        !self.recolours && roots.iter().all(|r| self.link(Some(*r)) == Some(*r))
    }
}

/// A colour as the tables compare it: trimmed, in capitals.
fn rgb_key(rgb: &str) -> String {
    rgb.trim().to_ascii_uppercase()
}

/// The colour numbers of `used` in the target: found by colour, else the same number if it is free, else the lowest free. **Both tables are
/// indexed once** (a table holds up to 4,096 entries and `used` up to as many numbers as the meter lets cells be visited, so a scan of a
/// table for each number would be a million times four thousand compares): the work is the two tables' lengths and the numbers used.
fn plan_colours(doc: &Document, from: &Document, used: &BTreeSet<u32>, meter: &mut Meter) -> Result<(HashMap<u32, u32>, Vec<ColorEntry>), Refusal> {
    let examined = from.colors.len().saturating_add(doc.colors.len());
    meter.charge(examined.saturating_add(used.len()))?;
    #[cfg(test)]
    COLOUR_ENTRIES_EXAMINED.with(|c| c.set(c.get().saturating_add(examined)));
    let mut source: HashMap<u32, &ColorEntry> = HashMap::with_capacity(from.colors.len());
    for entry in from.colors.iter() {
        source.entry(entry.ix).or_insert(entry);
    }
    let mut by_rgb: HashMap<String, u32> = HashMap::with_capacity(doc.colors.len());
    for entry in doc.colors.iter() {
        by_rgb.entry(rgb_key(&entry.rgb)).or_insert(entry.ix);
    }
    let mut map: HashMap<u32, u32> = HashMap::new();
    let mut added: Vec<ColorEntry> = Vec::new();
    let mut taken: HashSet<u32> = doc.colors.iter().map(|e| e.ix).collect();
    for &ix in used {
        let Some(wanted) = source.get(&ix) else { continue };
        let key = rgb_key(&wanted.rgb);
        if let Some(known) = by_rgb.get(&key) {
            map.insert(ix, *known);
            continue;
        }
        if doc.colors.len().saturating_add(added.len()) >= MAX_COLOR_ENTRIES {
            return Err(Refusal::Limit("colour table entries"));
        }
        let at = if taken.contains(&ix) { (24..=253).find(|n| !taken.contains(n)).ok_or(Refusal::ColourTableFull)? } else { ix };
        taken.insert(at);
        map.insert(ix, at);
        by_rgb.insert(key, at);
        added.push(ColorEntry { ix: at, rgb: Arc::clone(&wanted.rgb) });
    }
    Ok((map, added))
}

/// The faces of `names` that the target lacks and the source has, with the attributes they had.
fn plan_faces(doc: &Document, from: &Document, names: &BTreeSet<String>) -> Result<Vec<FaceName>, Refusal> {
    let have: HashSet<&str> = doc.fonts.iter().map(|f| &*f.name_u).collect();
    let source: HashMap<&str, &FaceName> = from.fonts.iter().map(|f| (&*f.name_u, f)).collect();
    let mut added = Vec::new();
    for name in names {
        if have.contains(name.as_str()) {
            continue;
        }
        if let Some(face) = source.get(name.as_str()) {
            added.push((*face).clone());
        }
    }
    // The cap is on what the copy adds: a drawing that already holds more faces than that can still take a master that names none of them.
    if !added.is_empty() && doc.fonts.len().saturating_add(added.len()) > MAX_FACES {
        return Err(Refusal::Limit("face names"));
    }
    Ok(added)
}

/// What a fingerprint of a sheet is made of, how many nodes it visited and how many bytes of text it hashed.
struct Fingerprint<H: Hasher> {
    h: H,
    nodes: usize,
    bytes: usize,
}

/// A fingerprint of a sheet, and what it cost to take, which is also what a comparison of two sheets that hash alike costs.
struct Print {
    hash: u64,
    nodes: usize,
    bytes: usize,
}

impl Print {
    /// The cost in the meter's units: a node is one, and 32 bytes of text are one (as the survey counts a formula's source).
    fn cost(&self) -> usize {
        self.nodes.saturating_add(self.bytes / 32)
    }
}

impl<H: Hasher> Fingerprint<H> {
    /// Hashes a piece of text, and counts its bytes.
    fn text(&mut self, text: &str) {
        text.hash(&mut self.h);
        self.bytes = self.bytes.saturating_add(text.len());
    }

    /// Hashes a piece of text that may be there, and counts its bytes.
    fn maybe(&mut self, text: Option<&str>) {
        text.hash(&mut self.h);
        self.bytes = self.bytes.saturating_add(text.map_or(0, str::len));
    }

    fn attrs(&mut self, raw: &RawAttrs) {
        for (name, value) in raw.iter() {
            self.text(name);
            self.text(value);
            self.nodes += 1;
        }
    }

    /// The kept elements, text, comments and instructions, in order, each by what it holds: iterative, and counted.
    fn kept(&mut self, kept: &Passthrough) {
        let mut work: Vec<&RawNode> = Vec::new();
        for placed in kept.iter() {
            placed.after.hash(&mut self.h);
            work.push(&placed.node);
            while let Some(node) = work.pop() {
                self.nodes += 1;
                match node {
                    RawNode::Element(element) => {
                        0_u8.hash(&mut self.h);
                        self.text(&element.name);
                        self.attrs(&element.attrs);
                        element.children.len().hash(&mut self.h);
                        work.extend(element.children.iter().rev());
                    }
                    RawNode::Text(text) => {
                        1_u8.hash(&mut self.h);
                        self.text(text);
                    }
                    RawNode::Comment(text) => {
                        2_u8.hash(&mut self.h);
                        self.text(text);
                    }
                    RawNode::Pi { target, data } => {
                        3_u8.hash(&mut self.h);
                        self.text(target);
                        self.text(data);
                    }
                }
            }
        }
    }

    fn cell(&mut self, name: &str, cell: &Cell) {
        self.text(name);
        self.maybe(cell.formula.text());
        self.maybe(cell.value.as_ref().map(Value::text));
        self.maybe(cell.unit.as_deref());
        self.maybe(cell.error.as_deref());
        self.attrs(&cell.raw);
        if let Some(extra) = &cell.extra {
            self.maybe(extra.text.as_deref());
            self.kept(&extra.passthrough);
        }
        self.nodes += 1;
    }
}

/// A hash of **everything `Sheet == Sheet` compares**, and how many nodes it visited and bytes of text it hashed: the links, the sheet's kept attributes and elements, its cells
/// (each by formula, value, unit, error, kept attributes and kept content), its sections and rows with their keys, flags, names, kept attributes and
/// elements, and their cells. Two sheets that are equal hash alike, and two that differ in anything the comparison looks at land together only by
/// chance, so a bucket holds the one or two sheets that are the same and not every sheet of a name (a hash that left out the kept attributes put 45,000
/// styles that differed only there in one bucket, 9e8 comparisons).
fn fingerprint(random: &RandomState, sheet: &Sheet) -> Print {
    #[cfg(test)]
    SHEETS_FINGERPRINTED.with(|c| c.set(c.get().saturating_add(1)));
    let mut f = Fingerprint { h: random.build_hasher(), nodes: 1, bytes: 0 };
    (sheet.links.line, sheet.links.fill, sheet.links.text).hash(&mut f.h);
    f.attrs(&sheet.raw);
    f.kept(&sheet.passthrough);
    for (name, cell) in &sheet.cells {
        f.cell(name.as_str(), cell);
    }
    for section in &sheet.sections {
        f.text(section.name.as_str());
        (section.ix, section.del).hash(&mut f.h);
        f.attrs(&section.raw);
        f.kept(&section.passthrough);
        f.nodes += 1;
        for (name, cell) in &section.cells {
            f.cell(name.as_str(), cell);
        }
        for row in &section.rows {
            (row.ix, row.del).hash(&mut f.h);
            f.maybe(row.name.as_deref());
            f.maybe(row.local_name.as_deref());
            f.maybe(row.row_type.as_deref());
            f.attrs(&row.raw);
            f.kept(&row.passthrough);
            f.nodes += 1;
            for (name, cell) in &row.cells {
                f.cell(name.as_str(), cell);
            }
        }
    }
    Print { hash: f.h.finish(), nodes: f.nodes, bytes: f.bytes }
}

/// A style's names: what two styles must share to be the same one.
type StyleName<'a> = (Option<&'a str>, Option<&'a str>);

/// The styles of the closure in the target: each found by name and definition, else added under the next free `ID`. `order` has the
/// dependencies first, so a style's links are mapped when its turn comes. **The drawing's styles of a name are hashed by definition once, the
/// first time a style of that name is looked for, and charged to the meter**, so each style of the closure is compared with the one or two of
/// the drawing's that hash as it does and not with every style of its name (65,536 styles of one name against a chain of 20,000 would be a
/// billion comparisons of sheets).
fn plan_styles(
    doc: &Document,
    by_id: &StylesById<'_>,
    order: &[u32],
    roots: &BTreeSet<u32>,
    colours: &HashMap<u32, u32>,
    meter: &mut Meter,
) -> Result<(StyleMap, Vec<Style>), Refusal> {
    let mut map: StyleMap = HashMap::new();
    for root in roots {
        map.insert(*root, None);
    }
    if order.is_empty() {
        return Ok((map, Vec::new()));
    }
    meter.charge(doc.styles.len())?;
    let mut by_name: HashMap<StyleName<'_>, Vec<&Arc<Style>>> = HashMap::new();
    for style in doc.styles.iter() {
        by_name.entry((style.name_u.as_deref(), style.name.as_deref())).or_default().push(style);
    }
    let random = RandomState::new();
    let mut hashed: HashMap<StyleName<'_>, HashMap<u64, Vec<&Arc<Style>>>> = HashMap::new();
    let recolours = colours.iter().any(|(a, b)| a != b);
    let mut next = doc.styles.iter().map(|s| s.id).max().map_or(Some(1), |h| h.checked_add(1)).ok_or(Refusal::Limit("style ids"))?.max(1);
    let mut added: Vec<Style> = Vec::new();
    for &id in order {
        let Some(style) = by_id.get(&id) else { continue };
        meter.charge(1)?;
        let links = Links {
            line: link_in(&map, style.sheet.links.line),
            fill: link_in(&map, style.sheet.links.fill),
            text: link_in(&map, style.sheet.links.text),
        };
        let sheet = remap_sheet(&style.sheet, links, recolours.then_some(colours), meter)?;
        let name: StyleName<'_> = (style.name_u.as_deref(), style.name.as_deref());
        if let std::collections::hash_map::Entry::Vacant(slot) = hashed.entry(name) {
            let mut group: HashMap<u64, Vec<&Arc<Style>>> = HashMap::new();
            for candidate in by_name.get(&name).into_iter().flatten() {
                let print = fingerprint(&random, &candidate.sheet);
                meter.charge(print.cost())?;
                group.entry(print.hash).or_default().push(candidate);
            }
            slot.insert(group);
        }
        let print = fingerprint(&random, &sheet);
        meter.charge(print.cost())?;
        // Each comparison is charged what it can cost, the nodes of the sheet compared and its text (a formula's source, a kept attribute's value) at a
        // unit for 32 bytes: a bucket of sheets that hash alike is told apart by `==`, which is a pass over both, so a bucket cannot be made large and
        // free, nor its sheets long and free.
        let mut found = None;
        for candidate in hashed.get(&name).and_then(|group| group.get(&print.hash)).into_iter().flatten() {
            meter.charge(print.cost())?;
            #[cfg(test)]
            SHEET_COMPARES.with(|c| c.set(c.get().saturating_add(1)));
            if *candidate.sheet == *sheet {
                found = Some(candidate.id);
                break;
            }
        }
        let mapped = match found {
            Some(existing) => existing,
            None => {
                if doc.styles.len().saturating_add(added.len()) >= MAX_STYLES {
                    return Err(Refusal::Limit("styles"));
                }
                let new_id = next;
                next = next.checked_add(1).ok_or(Refusal::Limit("style ids"))?;
                // The copy's `ID` is the one it is given: the text of the stencil's (`ID="03"`) is not kept, or a save would write it.
                added.push(Style {
                    id: new_id,
                    name: style.name.clone(),
                    name_u: style.name_u.clone(),
                    sheet,
                    raw: without(&style.raw, &["ID"]),
                    passthrough: style.passthrough.clone(),
                    // A style's slot is left with the stencil: what its file held beside the style sheet is that file's. The master's own slot is brought in, and charged to
                    // the meter before it is kept (`Survey::master_slot`).
                    ext: Default::default(),
                });
                new_id
            }
        };
        map.insert(id, Some(mapped));
    }
    Ok((map, added))
}

/// The attributes of `raw` but those called `names`.
fn without(raw: &RawAttrs, names: &[&str]) -> RawAttrs {
    if raw.iter().all(|(name, _)| !names.contains(&name)) {
        return raw.clone();
    }
    RawAttrs::from_pairs(raw.iter().filter(|(name, _)| !names.contains(name)))
}

fn link_in(map: &StyleMap, link: Option<u32>) -> Option<u32> {
    match link {
        None => None,
        Some(0) => Some(0),
        Some(id) => map.get(&id).copied().flatten(),
    }
}

/// The cell with its colour number changed, if it is a colour cell that holds a number the map changes.
fn recolour(name: &str, cell: &Cell, map: &HashMap<u32, u32>) -> Option<Cell> {
    if !COLOUR_CELLS.contains(&name) {
        return None;
    }
    let ix = plain_index(cell)?;
    let new = *map.get(&ix)?;
    if new == ix {
        return None;
    }
    let mut changed = cell.clone();
    changed.value = Some(Value::from_text(&new.to_string()));
    Some(changed)
}

fn recolour_section(section: &Arc<Section>, map: &HashMap<u32, u32>, meter: &mut Meter) -> Result<Option<Section>, Refusal> {
    let mut out: Option<Section> = None;
    meter.charge(section.cells.len())?;
    for (name, cell) in &section.cells {
        if let Some(c) = recolour(name.as_str(), cell, map) {
            out.get_or_insert_with(|| (**section).clone()).cells.insert(name.clone(), c);
        }
    }
    for (at, row) in section.rows.iter().enumerate() {
        meter.charge(1 + row.cells.len())?;
        let mut changed: Option<Row> = None;
        for (name, cell) in &row.cells {
            if let Some(c) = recolour(name.as_str(), cell, map) {
                changed.get_or_insert_with(|| (**row).clone()).cells.insert(name.clone(), c);
            }
        }
        if let Some(row) = changed
            && let Some(slot) = out.get_or_insert_with(|| (**section).clone()).rows.get_mut(at)
        {
            *slot = Arc::new(row);
        }
    }
    Ok(out)
}

/// The sheet with its links `links` and the colour numbers of `map` changed; the very sheet when that changes nothing.
fn remap_sheet(sheet: &Arc<Sheet>, links: Links, map: Option<&HashMap<u32, u32>>, meter: &mut Meter) -> Result<Arc<Sheet>, Refusal> {
    let mut out: Option<Sheet> = None;
    if links != sheet.links {
        let copy = out.get_or_insert_with(|| (**sheet).clone());
        copy.links = links;
        // The old link's text is in `raw` when the file wrote it as `03`: raw wins on a write, so a link rewritten here would revert to it.
        copy.raw = without(&copy.raw, &LINK_ATTRS);
    }
    if let Some(map) = map {
        meter.charge(sheet.cells.len())?;
        for (name, cell) in &sheet.cells {
            if let Some(c) = recolour(name.as_str(), cell, map) {
                out.get_or_insert_with(|| (**sheet).clone()).cells.insert(name.clone(), c);
            }
        }
        for (at, section) in sheet.sections.iter().enumerate() {
            meter.charge(1)?;
            if let Some(changed) = recolour_section(section, map, meter)?
                && let Some(slot) = out.get_or_insert_with(|| (**sheet).clone()).sections.get_mut(at)
            {
                *slot = Arc::new(changed);
            }
        }
    }
    Ok(out.map_or_else(|| Arc::clone(sheet), Arc::new))
}

/// The shapes of `list` as the copy has them: each shape that a link or a colour changes is a new `Arc`, every other is shared.
fn remap_list(list: &ShapeList, maps: &Maps, meter: &mut Meter, depth: usize) -> Result<Option<ShapeList>, Refusal> {
    if depth > MAX_SHAPE_DEPTH {
        return Ok(None);
    }
    let mut changed = false;
    let mut out: Vec<Arc<Shape>> = Vec::with_capacity(list.len());
    for shape in list.iter() {
        meter.charge(1)?;
        let sheet = remap_sheet(&shape.sheet, maps.links(shape.sheet.links), maps.recolours.then_some(&maps.colours), meter)?;
        let children = if shape.children.is_empty() { None } else { remap_list(&shape.children, maps, meter, depth + 1)? };
        // A shape of a master part names no master (and the reader leaves `Master` and `MasterShape` in `raw` there): a copy that kept them would
        // be written naming the drawing's masters, whichever they are.
        let names_a_master =
            shape.master.is_some() || shape.master_shape.is_some() || shape.raw.get("Master").is_some() || shape.raw.get("MasterShape").is_some();
        if Arc::ptr_eq(&sheet, &shape.sheet) && children.is_none() && !names_a_master {
            out.push(Arc::clone(shape));
            continue;
        }
        changed = true;
        let mut copy = Shape::clone(shape);
        copy.master = None;
        copy.master_shape = None;
        copy.raw = without(&copy.raw, &["Master", "MasterShape"]);
        copy.sheet = sheet;
        if let Some(children) = children {
            copy.children = children;
        }
        out.push(Arc::new(copy));
    }
    Ok(changed.then(|| ShapeList::from_vec(out)))
}

/// Works out what bringing `source` into `doc` adds, and the master to add, without changing `doc`.
fn plan(doc: &Document, from: &Document, source: &Master, budget: usize) -> Result<Plan, Refusal> {
    if doc.masters.len() >= MAX_MASTERS {
        return Err(Refusal::Limit("masters"));
    }
    let mut sv = Survey {
        meter: Meter { used: 0, budget },
        links: BTreeSet::new(),
        colours: BTreeSet::new(),
        faces: BTreeSet::new(),
        pictures: 0,
        uses: 0,
        strips_masters: false,
    };
    sv.sheet(&source.sheet, &source.raw, true)?;
    sv.kept_attrs(&source.raw)?;
    sv.kept_parts(&source.passthrough, &[&source.raw])?;
    // The `Connect` rows the master holds keep what they have no field for too.
    for connect in source.connects.iter() {
        sv.meter.charge(1)?;
        sv.kept_attrs(&connect.raw)?;
    }
    sv.shapes(&source.shapes)?;
    // What the stencil's file holds beside the master is that file's and not this drawing's: the name of the master's part in the package it was read from, the `Rel` of its row and the
    // attributes of its part's root but the prefixes it declared. The drawing's package names its own parts, and two masters from two stencils would otherwise name one. What is
    // the master's own stays: the nodes its `Shapes` and `Connects` hold beside the shapes, and whether its row had a `PageSheet`. **What stays is charged and walked before it is kept**
    // (`Survey::master_slot`): it is the master's content as its shapes are. **A reference among it to a part of the stencil's package is taken out, and the answer says so**
    // (`refs::leave_package_for_another`, the second security review): the master's part in the drawing has no row for it, and the rest of what the master keeps is worth keeping.
    let mut master = source.clone();
    master.leave_package();
    let pictures = sv.pictures;
    sv.master_slot(&master)?;
    let in_slot = sv.pictures.saturating_sub(pictures);
    sv.pictures = pictures;
    let stripped = if in_slot > 0 { refs::leave_package_for_another(&mut master) } else { 0 };
    if sv.pictures > 0 {
        return Err(Refusal::Pictures(sv.pictures));
    }
    let roots = sv.links.clone();
    let (by_id, order) = style_closure(from, &roots, &mut sv)?;
    // The styles the master reaches are looked at too: a relationship kept in one of them is a reference to a part the drawing will not have.
    if sv.pictures > 0 {
        return Err(Refusal::Pictures(sv.pictures));
    }
    let (colour_map, colours) = plan_colours(doc, from, &sv.colours, &mut sv.meter)?;
    let faces = plan_faces(doc, from, &sv.faces)?;
    let (style_map, styles) = plan_styles(doc, &by_id, &order, &roots, &colour_map, &mut sv.meter)?;
    let recolours = colour_map.iter().any(|(a, b)| a != b);
    let maps = Maps { styles: style_map, colours: colour_map, recolours };
    // The copy's `ID` is the one the drawing gives it: the text of the stencil's (`ID="05"`) is in `raw`, which wins on a write, so it is not kept.
    master.raw = without(&source.raw, &["ID"]);
    if !maps.is_identity(&roots) || sv.strips_masters {
        master.sheet = remap_sheet(&source.sheet, maps.links(source.sheet.links), maps.recolours.then_some(&maps.colours), &mut sv.meter)?;
        if let Some(shapes) = remap_list(&source.shapes, &maps, &mut sv.meter, 1)? {
            master.shapes = shapes;
        }
    }
    let mut notes = Vec::new();
    if stripped > 0 {
        notes.push(left_out_words(stripped, "the stencil's"));
    }
    if sv.uses > 0 {
        notes.push("formulas of this master call USE() to name other masters, which are not copied".to_owned());
    }
    Ok(Plan { master, styles, colours, faces, notes })
}

/// The words that say a master's references to parts of the package it came from were left out (`refs::leave_package_for_another`): `whose` is the package's owner ("the stencil's").
pub(crate) fn left_out_words(n: usize, whose: &str) -> String {
    format!(
        "{n} reference(s) to parts of {whose} file in what the master keeps beside its shapes were left out: the drawing does not have those parts, so they would lead nowhere"
    )
}

/// The `UniqueID` of the copy: the source's if the target holds none like it, else one of ours that it does not.
pub(super) fn unique_id_for(doc: &Document, source: Option<&str>) -> Option<Arc<str>> {
    let source = source?;
    let taken: HashSet<String> = doc.masters.iter().filter_map(|m| m.unique_id.as_deref()).map(guid_key).collect();
    if !taken.contains(&guid_key(source)) {
        return Some(Arc::from(source));
    }
    (1_u64..).map(|n| format!("{{D1AC0003-0000-4000-8000-{n:012X}}}")).find(|g| !taken.contains(&guid_key(g))).map(|g| Arc::from(g.as_str()))
}

/// Writes the plan into `doc`.
fn apply(doc: &mut Document, plan: Plan) -> Result<Brought, Refusal> {
    let Plan { mut master, styles, colours, faces, notes } = plan;
    let id = doc.masters.iter().map(|m| m.id).max().map_or(Some(1), |h| h.checked_add(1)).ok_or(Refusal::Limit("master ids"))?.max(1);
    master.id = id;
    let plan_unique = master.unique_id.clone();
    master.unique_id = unique_id_for(doc, master.unique_id.as_deref());
    let mut notes = notes;
    if let (Some(was), Some(now)) = (plan_unique.as_deref(), master.unique_id.as_deref())
        && was != now
    {
        // A `UniqueID` is renewed when the target would hold it twice, and the copy says so.
        notes.push(format!(
            "the drawing already has a master with the UniqueID {}, so the copy has {}",
            crate::text::shown(was, 64),
            crate::text::shown(now, 64)
        ));
    }
    let brought = Brought { id, added: true, styles_added: styles.len(), colours_added: colours.len(), faces_added: faces.len(), notes };
    for style in styles {
        doc.add_style(style).map_err(|_| Refusal::Limit("styles"))?;
    }
    if !colours.is_empty() {
        Arc::make_mut(&mut doc.colors).extend(colours);
    }
    if !faces.is_empty() {
        Arc::make_mut(&mut doc.fonts).extend(faces);
    }
    doc.add_master(master).map_err(|_| Refusal::Limit("masters"))?;
    Ok(brought)
}

#[cfg(test)]
mod tests;
