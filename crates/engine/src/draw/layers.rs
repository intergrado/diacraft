//! Layers: a page's table of layers, the layers a shape is on, and the edits that add, change, assign and remove them.
//!
//! **What is kept in cells**. A page's layers are the rows of the `Layer` section of its own sheet, keyed by `IX` from 0,
//! and a shape's layers are the `IX` values joined by semicolons in its own `LayerMember` cell. A new layer writes all eleven cells:
//! `Visible` 1, `Print` 1, `Active` 0, `Lock` 0, `Snap` 1, `Glue` 1, `Color` 255 (none), `ColorTrans` 0, `Status` 0, and `Name` and `NameUniv` the name. A row that is read
//! from a file with `Status` 1 or 2 and no name is the place a removed layer left: it is no layer here, and keeps its number.
//!
//! **Project rules** (ours; the specs leave them open, and the check list lists them):
//!
//! * A shape's layers are read from its **own** `LayerMember` cell. A list that an instance would take from its master is not read, here or by the selections and the shape operations, which read through
//!   this module's reader (no page shape in the files does), so such a shape is on no layer until it is assigned one, and an assignment writes a cell of its own, which stops the master's list.
//! * A layer's name is its text with the white space at both ends taken off; it is not empty, holds no control character, is at most [`MAX_NAME_CHARS`]
//!   characters, and is **not another layer's of the page** (no table repeats one). It is not one of the five names the OpenDocument format reserves
//!   (`layout`, `background`, `backgroundobjects`, `controls` and `measurelines`), which a drawing in that format reads as no layer: such a layer would be
//!   lost on a save in it. `NameUniv` follows a rename when it was the name.
//! * A locked layer cannot be active. Setting `Active` on a locked layer is refused, and **locking an active layer makes
//!   it inactive** (reported in the answer): nothing the person set is lost that the rule allowed to stay.
//! * A shape added by a drop, a drawing tool, a connector or a text box with no layer of its own joins every active layer that is not locked (a locked layer takes no new members).
//!   With none, the shape gets no `LayerMember` cell.
//! * A shape's list is a set, written in rising order (a file's order means nothing). An assignment **replaces** the list, adds to it or takes from
//!   it. A layer that is locked takes no new member, and a shape that is on a locked layer is not edited (it cannot be selected): both are refused in words. Assigning a group
//!   keeps what its members hold unless `preserve_members` is false, which puts the list on every member at any depth too (the check box).
//! * The specifications say nothing of a group whose member is on a locked layer, so the edit is refused, which loses nothing. A
//!   command that edits a group through its members refuses a group with a member on a locked layer, in words that name the member, the group and the layer: a format of
//!   its shapes (the members formatted), a resize its members follow (any member at any depth: [`locked_member`]) and a list put on its members
//!   (`preserve_members` false). A move, a turn, a flip and the group's own text change no member's cells and run; a delete, a cut and an ungroup act on the group as one.
//! * A replace across a page, the drawing or a selected group passes over the shapes on locked layers ([`Locks`]) and counts them; Find
//!   still finds them.
//! * Removing a layer **closes the numbers**: the rows after it move up by one and every `LayerMember` of the page is written again (the vacant rows
//!   are what a reader keeps from a file, not what this writes; no `Status` is written). A shape that was on it alone is on no layer, or is deleted when the caller asks
//!   (the desktop's dialog deletes them; the command keeps them unless asked).
//! * What a layer table makes of a shape ([`Standing`]): **hidden** when it lists layers and every one is hidden; **locked** when
//!   any layer it lists is locked (the project rule); **off paper** when no listed layer prints; **no snap** and **no glue** when no listed layer allows it
//!   A list that names no row of the table is no list.
//!
//! **Costs.** A page holds at most [`MAX_LAYERS`] layers (a table past it is read as refused, in words), and every walk of the page's shapes is [`scan`]'s: iterative, no deeper
//! than the model allows and at most [`MAX_SHAPES_PER_PAGE`] shapes. Every list is cut at [`MAX_MEMBERS`] numbers.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{
    CellAddr, CellEdit, CellName, Document, EffectiveRow, MAX_SHAPE_DEPTH, MAX_SHAPES_PER_PAGE, Owner, Resolved, Resolver, RowKey, SectionKey, Shape,
    ShapeList, ShapeRef, SheetRef, Value,
};

use super::{Problem, delete_shapes, page_mut, require_page, write_cells};
use crate::formatting::Rgb;

/// The most layers a page holds, and the most a call reads: past it the page is refused in words.
pub const MAX_LAYERS: usize = 4_096;

/// The longest a layer's name is, in characters.
pub const MAX_NAME_CHARS: usize = 255;

/// The most numbers one list is read for.
pub const MAX_MEMBERS: usize = 1_024;

/// The five layer names the OpenDocument format reserves, which an `.odg` reads as no layer.
pub const RESERVED_NAMES: [&str; 5] = ["layout", "background", "backgroundobjects", "controls", "measurelines"];

/// One layer of a page, as `layer.list` gives it.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    /// The row's `IX`: the number a shape's list holds.
    pub ix: u32,
    pub name: String,
    pub name_univ: String,
    pub visible: bool,
    pub print: bool,
    pub active: bool,
    pub lock: bool,
    pub snap: bool,
    pub glue: bool,
    /// The colour laid over the shapes on this one layer; `None` for 255, none.
    pub colour: Option<(u8, u8, u8)>,
    /// `ColorTrans`: 0 opaque, 1 fully clear.
    pub transparency: f64,
    /// How many shapes are on it, the members of groups counted: [`page_layers`] counts them, [`read_layers`] leaves 0.
    pub shapes: usize,
}

/// What the layers make of one shape ([`page_standing`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Standing {
    /// Every layer it is on is hidden: it is not drawn, and not selected.
    pub hidden: bool,
    /// Some layer it is on is locked: it is not selected, moved or edited.
    pub locked: bool,
    /// No layer it is on prints.
    pub off_paper: bool,
    /// No layer it is on lets others snap to it.
    pub no_snap: bool,
    /// No layer it is on lets a connector glue to it.
    pub no_glue: bool,
}

impl Standing {
    /// Whether the shape is as a shape on no layer is: nothing in it differs.
    pub fn is_plain(&self) -> bool {
        *self == Standing::default()
    }
}

fn key() -> SectionKey {
    SectionKey::new("Layer", None)
}

fn too_many(n: usize) -> Problem {
    Problem::failed(format!("the page has {n} layers, and at most {MAX_LAYERS} are read or made"))
}

fn cell_of<'r, 'a>(cells: &'r [(CellName, Resolved<'a>)], name: &str) -> Option<&'r Resolved<'a>> {
    cells.iter().find(|(n, _)| n.as_str() == name).map(|(_, r)| r)
}

fn number_of(cells: &[(CellName, Resolved<'_>)], name: &str) -> Option<f64> {
    cell_of(cells, name)?.value.cell?.value.as_ref()?.number().filter(|n| n.is_finite())
}

fn text_of(cells: &[(CellName, Resolved<'_>)], name: &str) -> Option<String> {
    Some(cell_of(cells, name)?.value.cell?.value.as_ref()?.text().to_owned())
}

/// A flag: a cell that holds a number is set when it is not 0, and a missing cell reads `default`.
fn flag_of(cells: &[(CellName, Resolved<'_>)], name: &str, default: bool) -> bool {
    number_of(cells, name).map_or(default, |n| n != 0.0)
}

/// The colour a `Color` cell gives: `#RRGGBB`, or an index of the document's colour table, and none for 255 (the form is tested first, since `#0000ff` is 255 as a value).
fn colour_of(doc: &Document, cells: &[(CellName, Resolved<'_>)]) -> Option<(u8, u8, u8)> {
    let value = cell_of(cells, "Color")?.value.cell?.value.as_ref()?;
    if value.text().starts_with('#') {
        return Rgb::parse(value.text()).map(|c| (c.0, c.1, c.2));
    }
    let n = value.number().filter(|n| n.is_finite())?;
    if n == 255.0 {
        return None;
    }
    let index = if (0.0..1.0e9).contains(&n) { n.round() as i64 } else { 0 };
    let c = doc.color(index).or_else(|| doc.color(0))?;
    Some((c.r, c.g, c.b))
}

fn row_ix(row: &EffectiveRow<'_>) -> Option<u32> {
    match row.key {
        RowKey::Ix(ix) => Some(ix),
        RowKey::Name(_) => None,
    }
}

/// The layers of a page, in row order: a vacant row (`Status` 1 or 2 with no name) is no layer. `shapes` is left 0.
pub fn read_layers(doc: &Document, page: u32) -> Result<Vec<Layer>, Problem> {
    require_page(doc, page)?;
    let res = Resolver::new(doc);
    let sheet = SheetRef::Page(page);
    let rows = match res.effective_rows_within(&sheet, &key(), MAX_LAYERS) {
        None => return Ok(Vec::new()),
        Some(Err(n)) => return Err(too_many(n)),
        Some(Ok(rows)) => rows,
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let Some(ix) = row_ix(row) else { continue };
        let cells = &row.cells;
        let name = text_of(cells, "Name").unwrap_or_default();
        let status = number_of(cells, "Status").unwrap_or(0.0);
        if (status == 1.0 || status == 2.0) && name.is_empty() {
            continue;
        }
        let name_univ = text_of(cells, "NameUniv").unwrap_or_default();
        out.push(Layer {
            ix,
            name,
            name_univ,
            visible: flag_of(cells, "Visible", true),
            print: flag_of(cells, "Print", true),
            active: flag_of(cells, "Active", false),
            lock: flag_of(cells, "Lock", false),
            snap: flag_of(cells, "Snap", true),
            glue: flag_of(cells, "Glue", true),
            colour: colour_of(doc, cells),
            transparency: number_of(cells, "ColorTrans").unwrap_or(0.0).clamp(0.0, 1.0),
            shapes: 0,
        });
    }
    Ok(out)
}

/// The longest `LayerMember` text that is read, in bytes, checked **before any of it is copied or parsed**: a shape on more layers than this holds has a list a drawing does not make (a
/// thousand layers is about 5 KB), and it is read as on no layer and counted (`overlong`). A list that sits on a master is no instance's, so no instance's reading ever looks at it.
pub const MAX_MEMBER_BYTES: usize = 16 * 1024;

/// The most bytes of `LayerMember` text that one walk of a page reads over all its shapes: past it the call is refused in words, so that a page of shapes on very many layers costs what
/// is read of them and no more than this in all.
pub const MAX_PAGE_MEMBER_BYTES: usize = 8 * 1024 * 1024;

/// The most entries of paths a walk that keeps them holds (a path is a step for each level a shape is down): a page of a million shapes each in a chain of groups is no walk to keep.
const MAX_PATH_STEPS: usize = 16 * 1024 * 1024;

/// What reading the lists of a walk has cost so far: the one budget every reader of a `LayerMember` text charges, whatever asks (the layers' commands, the window's pointer, a
/// selection by layer).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemberBudget {
    /// Bytes of `LayerMember` text parsed, charged against [`MAX_PAGE_MEMBER_BYTES`].
    pub read: usize,
    /// Lists longer than [`MAX_MEMBER_BYTES`], each read as empty.
    pub overlong: usize,
    /// Pieces of text parsed (every piece counted, a number or not), at most [`MAX_MEMBERS`] of each list.
    pub pieces: u64,
    /// Probes of the set a list is made a set by: one for each number a piece gave.
    pub probes: u64,
}

fn over_budget() -> Problem {
    Problem::failed(format!(
        "the shapes list more layer memberships than the layers are read for: at most {MAX_PAGE_MEMBER_BYTES} bytes of layer lists are read in one call, and the page's come to more"
    ))
}

/// The numbers a `LayerMember` text lists, **bounded and charged**: a text longer than [`MAX_MEMBER_BYTES`] is read as empty (its length is looked at, nothing else), a
/// shorter one is split and parsed after its bytes are charged to `budget` (refused in words past [`MAX_PAGE_MEMBER_BYTES`]), at most [`MAX_MEMBERS`] pieces of it, **every piece
/// counted** whether it is a number or not. A piece that is not digits is dropped, each number is kept once, and the list is in rising order. One set probe for each piece.
pub fn read_members(text: &str, budget: &mut MemberBudget) -> Result<Vec<u32>, Problem> {
    if text.len() > MAX_MEMBER_BYTES {
        budget.overlong = budget.overlong.saturating_add(1);
        return Ok(Vec::new());
    }
    budget.read = budget.read.saturating_add(text.len());
    if budget.read > MAX_PAGE_MEMBER_BYTES {
        return Err(over_budget());
    }
    let mut set: BTreeSet<u32> = BTreeSet::new();
    for piece in text.split(';').take(MAX_MEMBERS) {
        budget.pieces = budget.pieces.saturating_add(1);
        if let Ok(n) = piece.trim().parse::<u32>() {
            budget.probes = budget.probes.saturating_add(1);
            set.insert(n);
        }
    }
    Ok(set.into_iter().collect())
}

/// The layers `shape` is on: its own `LayerMember` cell, whatever rows the page has, read as [`read_members`] reads it, **charged to `budget`** (the caller's walk).
pub fn shape_layers_charged(shape: &Shape, budget: &mut MemberBudget) -> Result<Vec<u32>, Problem> {
    match shape.sheet.cell("LayerMember").and_then(|c| c.value.as_ref()) {
        Some(v) => read_members(v.text(), budget),
        None => Ok(Vec::new()),
    }
}

/// [`shape_layers_charged`] for one shape on its own: a text over [`MAX_MEMBER_BYTES`] is read as on no layer.
pub fn shape_layers(shape: &Shape) -> Vec<u32> {
    shape_layers_charged(shape, &mut MemberBudget::default()).unwrap_or_default()
}

/// The text of a list: the numbers in rising order, joined by semicolons.
fn member_text(set: &BTreeSet<u32>) -> String {
    set.iter().map(u32::to_string).collect::<Vec<_>>().join(";")
}

/// What one walk of a page found: the shapes that hold a `LayerMember` cell, with their lists, in document order, and what reading the lists cost.
struct Walk {
    /// `(path, id, list)` of each shape with a `LayerMember` cell; the path is empty unless the walk was asked to keep it.
    listed: Vec<(Vec<u32>, u32, Vec<u32>)>,
    cost: MemberBudget,
}

/// One walk of the page's shapes, a group before its members, for the shapes with a `LayerMember` cell: every list read through [`read_members`] and charged to one budget for the
/// walk, which refuses the page in words when it is spent. The paths are kept only when `keep_paths` says (a removal needs them), each charged against [`MAX_PATH_STEPS`].
fn walk_members(doc: &Document, page: u32, keep_paths: bool) -> Result<Walk, Problem> {
    let top = &doc.page(page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?.shapes;
    let mut out = Walk { listed: Vec::new(), cost: MemberBudget::default() };
    let mut stack: Vec<(&ShapeList, usize)> = vec![(top, 0)];
    let mut path: Vec<u32> = Vec::new();
    let mut count = 0usize;
    let mut kept_steps = 0usize;
    while let Some(frame) = stack.last_mut() {
        let (list, at) = *frame;
        let Some(shape) = list.get(at) else {
            stack.pop();
            continue;
        };
        frame.1 = at.saturating_add(1);
        count = count.saturating_add(1);
        #[cfg(test)]
        super::NODES_WALKED.with(|c| c.set(c.get() + 1));
        if count > MAX_SHAPES_PER_PAGE {
            return Err(Problem::failed(format!("a page holds at most {MAX_SHAPES_PER_PAGE} shapes")));
        }
        let depth = stack.len();
        if keep_paths {
            path.truncate(depth.saturating_sub(1));
            path.push(u32::try_from(at).unwrap_or(u32::MAX));
        }
        if shape.sheet.cell("LayerMember").is_some() && !shape.del {
            let list = shape_layers_charged(shape, &mut out.cost)?;
            let kept = if keep_paths {
                kept_steps = kept_steps.saturating_add(path.len());
                if kept_steps > MAX_PATH_STEPS {
                    return Err(Problem::failed("the page's shapes are nested too deeply for the layers' lists to be written again in one call"));
                }
                path.clone()
            } else {
                Vec::new()
            };
            out.listed.push((kept, shape.id, list));
        }
        if depth < MAX_SHAPE_DEPTH && !shape.children.is_empty() {
            stack.push((&shape.children, 0));
        }
    }
    Ok(out)
}

/// The layers of a page with the number of shapes on each (a member of a group counted): one walk of the page.
pub fn page_layers(doc: &Document, page: u32) -> Result<Vec<Layer>, Problem> {
    let mut layers = read_layers(doc, page)?;
    if layers.is_empty() {
        return Ok(layers);
    }
    let walk = walk_members(doc, page, false)?;
    let mut counts: HashMap<u32, usize> = HashMap::new();
    for (_, _, list) in &walk.listed {
        for ix in list {
            *counts.entry(*ix).or_insert(0) += 1;
        }
    }
    for layer in &mut layers {
        layer.shapes = counts.get(&layer.ix).copied().unwrap_or(0);
    }
    Ok(layers)
}

/// The standing of one list against the table: the rule the window's pointer and the selections by type and region share.
pub fn standing_of(list: &[u32], table: &HashMap<u32, &Layer>) -> Standing {
    let rows: Vec<&Layer> = list.iter().filter_map(|ix| table.get(ix).copied()).collect();
    if rows.is_empty() {
        return Standing::default();
    }
    Standing {
        hidden: rows.iter().all(|l| !l.visible),
        locked: rows.iter().any(|l| l.lock),
        off_paper: rows.iter().all(|l| !l.print),
        no_snap: rows.iter().all(|l| !l.snap),
        no_glue: rows.iter().all(|l| !l.glue),
    }
}

/// What the page's layers make of its shapes: for each shape that is not as a shape on no layer is, by `ID` (the first shape of an `ID`; members of groups included). A page whose
/// layers are all plain (visible, printing, unlocked, snapping and gluing) gives an empty map without a walk of its shapes.
pub fn page_standing(doc: &Document, page: u32) -> Result<HashMap<u32, Standing>, Problem> {
    let layers = read_layers(doc, page)?;
    if layers.iter().all(|l| l.visible && l.print && !l.lock && l.snap && l.glue) {
        return Ok(HashMap::new());
    }
    let table: HashMap<u32, &Layer> = layers.iter().map(|l| (l.ix, l)).collect();
    let walk = walk_members(doc, page, false)?;
    let mut out = HashMap::new();
    for (_, id, list) in &walk.listed {
        let standing = standing_of(list, &table);
        if !standing.is_plain() {
            out.entry(*id).or_insert(standing);
        }
    }
    Ok(out)
}

/// The standing of the shape `id` of the page (the first with that `ID`): plain when it is on no layer.
pub fn shape_standing(doc: &Document, page: u32, id: u32) -> Result<Standing, Problem> {
    Ok(page_standing(doc, page)?.get(&id).copied().unwrap_or_default())
}

/// Whether the page has a layer that is locked. A page that is not there, and a table of more than [`MAX_LAYERS`] rows (which the layer commands refuse in words, and the
/// window reads as having no standing), have none.
pub fn has_locked_layer(doc: &Document, page: u32) -> bool {
    doc.page(page).is_some() && read_layers(doc, page).is_ok_and(|layers| layers.iter().any(|l| l.lock))
}

/// The first of the shapes `ids` of page `page` (each looked for as `Document::find_shape` finds it) whose own list names a locked layer, with the
/// first locked layer it names (a shape on any locked layer is not selected, moved or edited). Each shape goes by its own list, as the window's pointer does: a
/// group on no locked layer is not barred by a member that is on one, nor a member by its group's list. An `ID` the page does not hold is passed over (the command says so).
///
/// **What it costs.** A page whose table locks nothing costs the read of its table and no walk; else one walk of the page finds every shape named, and their lists are read
/// through [`read_members`] on one budget ([`MAX_PAGE_MEMBER_BYTES`], refused in words when spent).
pub fn first_locked(doc: &Document, page: u32, ids: &[u32]) -> Result<Option<(u32, Layer)>, Problem> {
    if doc.page(page).is_none() || ids.is_empty() {
        return Ok(None);
    }
    let Ok(table) = read_layers(doc, page) else { return Ok(None) };
    let locked: HashMap<u32, &Layer> = table.iter().filter(|l| l.lock).map(|l| (l.ix, l)).collect();
    if locked.is_empty() {
        return Ok(None);
    }
    let ids = super::scan::distinct(ids);
    let wanted: HashSet<u32> = ids.iter().copied().collect();
    let found = super::scan::scan(doc, page, &wanted)?.found;
    let mut budget = MemberBudget::default();
    for id in ids {
        let Some(shape) = found.get(&id).and_then(|at| doc.shape(at)) else { continue };
        if let Some(layer) = shape_layers_charged(shape, &mut budget)?.iter().find_map(|ix| locked.get(ix)) {
            return Ok(Some((id, (*layer).clone())));
        }
    }
    Ok(None)
}

/// **For a walk**: the locked layers of each page a walk meets, read once for the page, and the shapes it meets judged each by its **own**
/// `LayerMember`, every list read on one [`MemberBudget`] for the walk ([`MAX_PAGE_MEMBER_BYTES`], refused in words when spent). A page none of whose layers is
/// locked, and a table of more than [`MAX_LAYERS`] rows (read as having no standing, as [`has_locked_layer`] reads it), cost the read of the table and no list.
#[derive(Debug, Default)]
pub struct Locks {
    /// The locked layers of each page met, by their `IX`.
    pages: HashMap<u32, HashMap<u32, Layer>>,
    budget: MemberBudget,
}

impl Locks {
    /// The first locked layer that `shape`, a shape of page `page`, names in its own list; `None` when it names none.
    pub fn of(&mut self, doc: &Document, page: u32, shape: &Shape) -> Result<Option<Layer>, Problem> {
        let locked = self.pages.entry(page).or_insert_with(|| {
            read_layers(doc, page).map(|layers| layers.into_iter().filter(|l| l.lock).map(|l| (l.ix, l)).collect()).unwrap_or_default()
        });
        if locked.is_empty() || shape.sheet.cell("LayerMember").is_none() {
            return Ok(None);
        }
        Ok(shape_layers_charged(shape, &mut self.budget)?.iter().find_map(|ix| locked.get(ix)).cloned())
    }

    /// What reading the lists has cost so far.
    pub fn cost(&self) -> MemberBudget {
        self.budget
    }
}

/// **A group edited through its members.** The first member, at any depth, of the shapes `ids` of page `page` that hold members, whose own list names a locked
/// layer, as `(group, member, layer)`: `group` is the shape named, `member` the shape inside it. A command that formats, retexts or resizes a group through its members, or
/// writes their lists, refuses it (a shape on a locked layer is not edited; the specifications say nothing of a group whose member is on one, and a refusal loses
/// nothing). A shape named that holds no members, an `ID` the page does not hold, and a page none of whose layers is locked give `None`, the last with no walk.
///
/// **What it costs.** The read of the page's table, then one walk of the page to find the shapes named ([`super::scan::scan`]) and a walk of each group named, iterative and no
/// deeper than the model allows, every member counted against [`MAX_SHAPES_PER_PAGE`] for the call and every list read on one budget.
pub fn locked_member(doc: &Document, page: u32, ids: &[u32]) -> Result<Option<(u32, u32, Layer)>, Problem> {
    if ids.is_empty() || !has_locked_layer(doc, page) {
        return Ok(None);
    }
    let ids = super::scan::distinct(ids);
    let wanted: HashSet<u32> = ids.iter().copied().collect();
    let found = super::scan::scan(doc, page, &wanted)?.found;
    let mut locks = Locks::default();
    let mut seen = 0usize;
    for id in ids {
        let Some(group) = found.get(&id).and_then(|at| doc.shape(at)) else { continue };
        let mut stack: Vec<(&ShapeList, usize)> = vec![(&group.children, 1)];
        while let Some((list, depth)) = stack.pop() {
            for member in list.iter() {
                seen = seen.saturating_add(1);
                if seen > MAX_SHAPES_PER_PAGE {
                    return Err(Problem::failed(format!("a page holds at most {MAX_SHAPES_PER_PAGE} shapes")));
                }
                if member.del {
                    continue;
                }
                if let Some(layer) = locks.of(doc, page, member)? {
                    return Ok(Some((id, member.id, layer)));
                }
                if depth < MAX_SHAPE_DEPTH && !member.children.is_empty() {
                    stack.push((&member.children, depth + 1));
                }
            }
        }
    }
    Ok(None)
}

/// Refuses in words, with [`locked_member_words`], a command that `what`s (`formatted`, `resized`, ...) the shapes `ids` of page `page` through their members, when a member
/// of one of them is on a locked layer ([`locked_member`]).
pub fn refuse_locked_member(doc: &Document, page: u32, ids: &[u32], what: &str) -> Result<(), Problem> {
    match locked_member(doc, page, ids)? {
        Some((group, member, layer)) => Err(Problem::bad(locked_member_words(group, member, &layer, what))),
        None => Ok(()),
    }
}

/// The words of a refusal to `what` (`formatted`, `resized`, ...) the group `group` through its member `member`, which is on the locked layer `layer`.
pub fn locked_member_words(group: u32, member: u32, layer: &Layer, what: &str) -> String {
    format!(
        "shape {member} in group {group} is on layer {} (\"{}\"), which is locked: a group is not {what} through a member on a locked layer, since a shape on a locked layer is not \
         edited; unlock the layer first",
        layer.ix,
        crate::text::shown(&layer.name, 64)
    )
}

/// The specifications say nothing of a group whose member is on a locked layer, and a refusal loses nothing: refuses
/// in words a command that would take whole (`deleted`, `cut`) or take apart (`ungrouped`) a group among the shapes `ids` of page `page` that holds a member, at any depth, on a
/// locked layer ([`locked_member`]). A delete or a cut takes the member with the group, and an ungroup rewrites the cells that tie it to the group (its frame, and its formulas that
/// name the group), and a shape on a locked layer is not moved, deleted or edited. A copy of such a group is made, its member on the same layer.
pub fn refuse_locked_in_group(doc: &Document, page: u32, ids: &[u32], what: &str) -> Result<(), Problem> {
    match locked_member(doc, page, ids)? {
        Some((group, member, layer)) => Err(Problem::bad(whole_group_words(group, member, &layer, what))),
        None => Ok(()),
    }
}

/// The words of [`refuse_locked_in_group`]'s refusal to `what` the group `group`, whose member `member` is on the locked layer `layer`.
pub fn whole_group_words(group: u32, member: u32, layer: &Layer, what: &str) -> String {
    format!(
        "shape {member} in group {group} is on layer {} (\"{}\"), which is locked: a group that holds a shape on a locked layer is not {what}, since that shape is not moved, \
         deleted or edited; unlock the layer first",
        layer.ix,
        crate::text::shown(&layer.name, 64)
    )
}

/// Refuses in words a command that would `what` (`moved`, `deleted`, `cut`) a shape on a locked layer because it goes with the shapes the
/// command names: a member of a container, or a callout tied to a shape (`followers`, pairs of an `ID` and its shape, each judged by its own list). A shape on
/// a locked layer is not moved or edited; the specifications say nothing of one that a container holds, so the command is refused, which loses nothing.
pub fn refuse_locked_followers<'a>(
    doc: &Document,
    page: u32,
    followers: impl IntoIterator<Item = (u32, &'a Shape)>,
    what: &str,
) -> Result<(), Problem> {
    let mut locks = Locks::default();
    for (id, shape) in followers {
        if let Some(layer) = locks.of(doc, page, shape)? {
            return Err(Problem::bad(follower_words(id, &layer, what)));
        }
    }
    Ok(())
}

/// The words of [`refuse_locked_followers`]'s refusal, for the shape `id` on the locked layer `layer`.
pub fn follower_words(id: u32, layer: &Layer, what: &str) -> String {
    format!(
        "shape {id} is on layer {} (\"{}\"), which is locked: it would be {what} with the shapes named, as a member of a container or a callout tied to one, and a shape on a \
         locked layer is not moved or edited; unlock the layer first",
        layer.ix,
        crate::text::shown(&layer.name, 64)
    )
}

/// The words of a refusal of an edit of shape `id`, which is on the locked layer `layer`.
pub fn locked_words(id: u32, layer: &Layer) -> String {
    format!(
        "shape {id} is on layer {} (\"{}\"), which is locked: a shape on a locked layer is not selected, moved or edited; unlock the layer first",
        layer.ix,
        crate::text::shown(&layer.name, 64)
    )
}

/// The layer that bars new glue to `shape`, a shape of page `page` (the Glue box says whether a connector may be glued to the layer's
/// shapes; a shape is glued to when any layer its own list names allows it, and a shape on no layer always is): the first layer its list names, when none of them allows glue,
/// else `None`. Glue a file holds is kept as the file has it: this is asked only when an end is glued anew. A table of more than [`MAX_LAYERS`] rows bars nothing (the window
/// reads it as having no standing, and the layer commands refuse it in words).
pub fn glue_barred(doc: &Document, page: u32, shape: &Shape) -> Option<Layer> {
    let table = read_layers(doc, page).ok()?;
    if table.iter().all(|l| l.glue) {
        return None;
    }
    let by_ix: HashMap<u32, &Layer> = table.iter().map(|l| (l.ix, l)).collect();
    let rows: Vec<&Layer> = shape_layers(shape).iter().filter_map(|ix| by_ix.get(ix).copied()).collect();
    if rows.iter().any(|l| l.glue) {
        return None;
    }
    rows.first().map(|l| (*l).clone())
}

/// The layer that bars glue to a shape added to page `page` with no list of its own: it joins every active layer that is not locked, and is
/// glued to only when one of them allows glue. `None` when it is glued to: no layer is active, or one that is allows glue.
pub fn new_shape_glue_barred(doc: &Document, page: u32) -> Option<Layer> {
    let table = read_layers(doc, page).ok()?;
    let joined: Vec<&Layer> = table.iter().filter(|l| l.active && !l.lock).collect();
    if joined.iter().any(|l| l.glue) {
        return None;
    }
    joined.first().map(|l| (*l).clone())
}

/// The words of a refusal to glue a connector to shape `id`, whose layers allow no glue; `layer` is the first of them.
pub fn glue_words(id: u32, layer: &Layer) -> String {
    format!(
        "shape {id} is on layer {} (\"{}\"), whose Glue box is off: a connector is not glued to a shape whose layers allow no glue; tick Glue for the layer first",
        layer.ix,
        crate::text::shown(&layer.name, 64)
    )
}

// ---------- names ----------

/// The name `text` gives a layer after a change checks, against the layers `others` of the page.
fn checked_name(text: &str, others: &[Layer], except: Option<u32>) -> Result<String, Problem> {
    let name = text.trim();
    if name.is_empty() {
        return Err(Problem::bad("a layer's name is empty"));
    }
    if name.chars().any(char::is_control) {
        return Err(Problem::bad("a layer's name holds a control character"));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(Problem::bad(format!("a layer's name is too long: at most {MAX_NAME_CHARS} characters")));
    }
    if RESERVED_NAMES.contains(&name) {
        return Err(Problem::bad(format!(
            "\"{name}\" is a layer name the OpenDocument format reserves for its own layers, which a drawing saved in it would lose: choose another"
        )));
    }
    if others.iter().any(|l| l.name == name && Some(l.ix) != except) {
        return Err(Problem::bad(format!("the page already has a layer named \"{}\"", crate::text::shown(name, 64))));
    }
    Ok(name.to_owned())
}

/// A layer asked for by its number or its name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Which {
    Ix(u32),
    Name(String),
}

/// The layer `which` names among `layers`.
pub fn find<'a>(layers: &'a [Layer], which: &Which) -> Result<&'a Layer, Problem> {
    let found = match which {
        Which::Ix(ix) => layers.iter().find(|l| l.ix == *ix),
        Which::Name(name) => layers.iter().find(|l| l.name == *name),
    };
    found.ok_or_else(|| match which {
        Which::Ix(ix) => Problem::bad(format!("the page has no layer {ix}")),
        Which::Name(name) => Problem::bad(format!("the page has no layer named \"{}\"", crate::text::shown(name, 64))),
    })
}

// ---------- writing ----------

fn cell_addr(ix: u32, cell: &str) -> CellAddr {
    CellAddr::in_row("Layer", None, RowKey::Ix(ix), cell)
}

fn put(writes: &mut Vec<(SheetRef, CellAddr, CellEdit)>, page: u32, ix: u32, cell: &str, text: &str) {
    writes.push((SheetRef::Page(page), cell_addr(ix, cell), CellEdit::Value(Value::from_text(text))));
}

fn flag_text(on: bool) -> &'static str {
    if on { "1" } else { "0" }
}

/// What a new layer is made with: `None` leaves the value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    pub name: Option<String>,
    pub visible: Option<bool>,
    pub print: Option<bool>,
    pub active: Option<bool>,
    pub lock: Option<bool>,
    pub snap: Option<bool>,
    pub glue: Option<bool>,
    /// `Some(None)` is no colour (255).
    pub colour: Option<Option<(u8, u8, u8)>>,
    pub transparency: Option<f64>,
}

impl Settings {
    /// Whether the settings change nothing.
    pub fn is_empty(&self) -> bool {
        *self == Settings::default()
    }
}

fn colour_text(colour: Option<(u8, u8, u8)>) -> String {
    match colour {
        Some((r, g, b)) => format!("#{r:02x}{g:02x}{b:02x}"),
        None => "255".to_owned(),
    }
}

fn transparency_text(t: f64) -> Result<String, Problem> {
    if !t.is_finite() || !(0.0..=1.0).contains(&t) {
        return Err(Problem::bad("`transparency` must be a number from 0 (opaque) to 1 (clear)"));
    }
    Ok(crate::stencil::number_text(t))
}

/// The layer a page's table has no number for yet: one past the highest row, which is 0 for a page with none.
fn next_ix(doc: &Document, page: u32) -> Result<u32, Problem> {
    let sheet = &doc.page(page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?.sheet;
    let highest = sheet.section(&key()).and_then(|s| s.rows.iter().filter_map(|r| r.ix).max());
    highest.map_or(Ok(0), |h| h.checked_add(1).ok_or_else(|| Problem::failed("no layer numbers are left on the page")))
}

/// Adds a layer to the end of the page's table → its `IX`.
pub fn add_layer(doc: &mut Document, page: u32, settings: &Settings) -> Result<u32, Problem> {
    let layers = read_layers(doc, page)?;
    if layers.len() >= MAX_LAYERS {
        return Err(Problem::failed(format!("a page holds at most {MAX_LAYERS} layers")));
    }
    let name = checked_name(settings.name.as_deref().unwrap_or(""), &layers, None)?;
    let (lock, active) = (settings.lock.unwrap_or(false), settings.active.unwrap_or(false));
    if lock && active {
        return Err(Problem::bad("a locked layer cannot be active"));
    }
    let transparency = settings.transparency.map_or(Ok("0".to_owned()), transparency_text)?;
    let ix = next_ix(doc, page)?;
    let mut writes = Vec::with_capacity(11);
    for (cell, text) in [
        ("Name", name.clone()),
        ("NameUniv", name),
        ("Visible", flag_text(settings.visible.unwrap_or(true)).to_owned()),
        ("Print", flag_text(settings.print.unwrap_or(true)).to_owned()),
        ("Active", flag_text(active).to_owned()),
        ("Lock", flag_text(lock).to_owned()),
        ("Snap", flag_text(settings.snap.unwrap_or(true)).to_owned()),
        ("Glue", flag_text(settings.glue.unwrap_or(true)).to_owned()),
        ("Color", colour_text(settings.colour.unwrap_or(None))),
        ("ColorTrans", transparency),
        ("Status", "0".to_owned()),
    ] {
        put(&mut writes, page, ix, cell, &text);
    }
    write_cells(doc, writes)?;
    Ok(ix)
}

/// What [`set_layer`] changed besides what it was asked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SetOutcome {
    /// The layer was locked while active, so it was made inactive.
    pub deactivated: bool,
}

/// Changes the layer `ix` of the page. A rename keeps the shapes on it.
pub fn set_layer(doc: &mut Document, page: u32, ix: u32, settings: &Settings) -> Result<SetOutcome, Problem> {
    let layers = read_layers(doc, page)?;
    let layer = layers.iter().find(|l| l.ix == ix).ok_or_else(|| Problem::bad(format!("the page has no layer {ix}")))?;
    let mut writes = Vec::new();
    let mut outcome = SetOutcome::default();
    if let Some(text) = &settings.name {
        let name = checked_name(text, &layers, Some(ix))?;
        put(&mut writes, page, ix, "Name", &name);
        // The universal name follows a rename when it was the name.
        if layer.name_univ == layer.name || layer.name_univ.is_empty() {
            put(&mut writes, page, ix, "NameUniv", &name);
        }
    }
    let (lock, active) = (settings.lock.unwrap_or(layer.lock), settings.active.unwrap_or(layer.active));
    if lock && settings.active == Some(true) {
        return Err(Problem::bad("a locked layer cannot be active"));
    }
    let active = if lock && active {
        outcome.deactivated = settings.lock == Some(true);
        false
    } else {
        active
    };
    if let Some(on) = settings.visible {
        put(&mut writes, page, ix, "Visible", flag_text(on));
    }
    if let Some(on) = settings.print {
        put(&mut writes, page, ix, "Print", flag_text(on));
    }
    if settings.lock.is_some() {
        put(&mut writes, page, ix, "Lock", flag_text(lock));
    }
    if settings.active.is_some() || outcome.deactivated {
        put(&mut writes, page, ix, "Active", flag_text(active));
    }
    if let Some(on) = settings.snap {
        put(&mut writes, page, ix, "Snap", flag_text(on));
    }
    if let Some(on) = settings.glue {
        put(&mut writes, page, ix, "Glue", flag_text(on));
    }
    if let Some(colour) = settings.colour {
        put(&mut writes, page, ix, "Color", &colour_text(colour));
    }
    if let Some(t) = settings.transparency {
        put(&mut writes, page, ix, "ColorTrans", &transparency_text(t)?);
    }
    if writes.is_empty() {
        return Err(Problem::bad("there is nothing to change: give a name or a setting"));
    }
    write_cells(doc, writes)?;
    Ok(outcome)
}

// ---------- assigning ----------

/// How an assignment treats the list a shape has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The list becomes these layers (the Layer dialog's ticks).
    Replace,
    /// These layers are added to the list.
    Add,
    /// These layers are taken from the list.
    Remove,
}

/// What [`assign_layers`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Assigned {
    /// How many shapes were named (a group's members that were changed with it are not counted).
    pub shapes: usize,
    /// How many `LayerMember` cells were written.
    pub written: usize,
}

/// The paths of the shapes at `at` and every member of it, at any depth, the shape first.
fn subtree_paths(doc: &Document, at: &ShapeRef) -> Vec<Vec<u32>> {
    let mut out: Vec<Vec<u32>> = vec![at.path.clone()];
    let Some(shape) = doc.shape(at) else { return out };
    let mut stack: Vec<(&ShapeList, Vec<u32>, usize)> = vec![(&shape.children, at.path.clone(), 1)];
    while let Some((list, base, depth)) = stack.pop() {
        for (i, member) in list.iter().enumerate() {
            let Ok(step) = u32::try_from(i) else { continue };
            let mut path = base.clone();
            path.push(step);
            out.push(path.clone());
            if depth < MAX_SHAPE_DEPTH && !member.children.is_empty() {
                stack.push((&member.children, path, depth + 1));
            }
        }
    }
    out
}

fn shape_at<'d>(doc: &'d Document, page: u32, path: &[u32]) -> Option<&'d Shape> {
    doc.shape(&ShapeRef { owner: Owner::Page(page), path: path.to_vec() }).map(|v| &**v)
}

/// Puts the shapes `ids` on, adds them to, or takes them from the `layers` (numbers of the page's rows), by the layer assignment rule.
pub fn assign_layers(doc: &mut Document, page: u32, ids: &[u32], layers: &[u32], mode: Mode, preserve_members: bool) -> Result<Assigned, Problem> {
    let table = read_layers(doc, page)?;
    for ix in layers {
        let layer = table.iter().find(|l| l.ix == *ix).ok_or_else(|| Problem::bad(format!("the page has no layer {ix}")))?;
        if layer.lock && mode != Mode::Remove {
            return Err(Problem::bad(format!(
                "layer {ix} (\"{}\") is locked, and a locked layer takes no new shapes",
                crate::text::shown(&layer.name, 64)
            )));
        }
    }
    let (_, found) = super::scan::named(doc, page, ids)?;
    // A member named with its group is the group's to change.
    let outer = super::scan::outermost(found);
    let standing = page_standing(doc, page)?;
    for (id, _) in &outer {
        if standing.get(id).is_some_and(|s| s.locked) {
            return Err(Problem::bad(format!(
                "shape {id} is on a locked layer, and a shape on a locked layer is not edited: unlock the layer first"
            )));
        }
    }
    // A list put on the members too writes the list of a member on a locked layer, which is not edited.
    if !preserve_members {
        let named: Vec<u32> = outer.iter().map(|(id, _)| *id).collect();
        refuse_locked_member(doc, page, &named, "assigned to layers")?;
    }
    let wanted: BTreeSet<u32> = layers.iter().copied().collect();
    let mut writes: Vec<(SheetRef, CellAddr, CellEdit)> = Vec::new();
    // Every list the call reads, the members of groups too, is charged to one budget.
    let mut budget = MemberBudget::default();
    for (_, at) in &outer {
        let paths = if preserve_members { vec![at.path.clone()] } else { subtree_paths(doc, at) };
        if paths.len() > MAX_SHAPES_PER_PAGE {
            return Err(Problem::failed(format!("a page holds at most {MAX_SHAPES_PER_PAGE} shapes")));
        }
        for path in paths {
            let Some(shape) = shape_at(doc, page, &path) else { continue };
            let held: BTreeSet<u32> = shape_layers_charged(shape, &mut budget)?.into_iter().collect();
            let next: BTreeSet<u32> = match mode {
                Mode::Replace => wanted.clone(),
                Mode::Add => held.union(&wanted).copied().collect(),
                Mode::Remove => held.difference(&wanted).copied().collect(),
            };
            let had_cell = shape.sheet.cell("LayerMember").is_some();
            let unchanged = if had_cell { next == held && shape_text_is_canonical(shape, &next) } else { next.is_empty() && shape.master.is_none() };
            if unchanged {
                continue;
            }
            writes.push((
                SheetRef::Shape(ShapeRef { owner: Owner::Page(page), path }),
                CellAddr::direct("LayerMember"),
                CellEdit::Value(Value::from_text(&member_text(&next))),
            ));
        }
    }
    let written = writes.len();
    write_cells(doc, writes)?;
    Ok(Assigned { shapes: outer.len(), written })
}

/// Whether the shape's `LayerMember` text is what a list of `set` is written as.
fn shape_text_is_canonical(shape: &Shape, set: &BTreeSet<u32>) -> bool {
    shape.sheet.cell("LayerMember").and_then(|c| c.value.as_ref()).is_some_and(|v| v.text() == member_text(set))
}

/// A shape just added joins every active layer that is not locked. Nothing is written when there is none.
pub fn join_active(doc: &mut Document, page: u32, id: u32) -> Result<(), Problem> {
    let layers = read_layers(doc, page)?;
    let active: BTreeSet<u32> = layers.iter().filter(|l| l.active && !l.lock).map(|l| l.ix).collect();
    if active.is_empty() {
        return Ok(());
    }
    let at = super::require_shape(doc, page, id)?;
    if doc.shape(&at).is_some_and(|s| s.sheet.cell("LayerMember").is_some()) {
        return Ok(());
    }
    write_cells(doc, vec![(SheetRef::Shape(at), CellAddr::direct("LayerMember"), CellEdit::Value(Value::from_text(&member_text(&active))))])
}

/// [`join_active`] for the shapes just added at `refs` (places on the page the caller has), in one write: what a command that adds many shapes at once asks, where [`join_active`]
/// for each walked the page from its top to find each one. A place that is no shape is passed over.
pub fn join_active_at(doc: &mut Document, page: u32, refs: &[ShapeRef]) -> Result<(), Problem> {
    let layers = read_layers(doc, page)?;
    let active: BTreeSet<u32> = layers.iter().filter(|l| l.active && !l.lock).map(|l| l.ix).collect();
    if active.is_empty() {
        return Ok(());
    }
    let text = member_text(&active);
    let writes = refs
        .iter()
        .filter(|at| doc.shape(at).is_some_and(|s| s.sheet.cell("LayerMember").is_none()))
        .map(|at| (SheetRef::Shape(at.clone()), CellAddr::direct("LayerMember"), CellEdit::Value(Value::from_text(&text))))
        .collect();
    write_cells(doc, writes)
}

// ---------- removing ----------

/// What [`delete_layers`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Deleted {
    /// The numbers the removed layers had.
    pub removed: Vec<u32>,
    /// The shapes deleted with them.
    pub shapes_deleted: usize,
    /// The shapes whose list was written again.
    pub lists_written: usize,
    /// What reading the lists of the page cost (one walk), and **how many lookups the removal made**: one probe of a set for each number of each list, and one search of the sorted
    /// removed numbers for each number that stays. Never a scan of the table for a number.
    pub cost: MemberBudget,
    pub lookups: u64,
}

/// Removes the layers `which` (numbers of the page's rows) and closes the numbers. With `delete_shapes` the shapes that are on none but the removed layers go too.
///
/// **What it costs.** One walk of the page, every list charged to the walk's budget ([`MAX_PAGE_MEMBER_BYTES`]); the table's numbers are put in a set once, the removed numbers in a
/// sorted list once, and each number of a list is one probe of the set and one binary search: the work is the pieces read times a logarithm, not the pieces times the layers.
pub fn delete_layers(doc: &mut Document, page: u32, which: &[u32], delete_shapes_too: bool) -> Result<Deleted, Problem> {
    let table = read_layers(doc, page)?;
    let known: HashSet<u32> = table.iter().map(|l| l.ix).collect();
    let gone: BTreeSet<u32> = which.iter().copied().collect();
    for ix in &gone {
        if !known.contains(ix) {
            return Err(Problem::bad(format!("the page has no layer {ix}")));
        }
    }
    if gone.is_empty() {
        return Ok(Deleted::default());
    }
    let removed: Vec<u32> = gone.iter().copied().collect();
    let mut lookups = 0u64;
    // The shapes only on the removed layers, taken before any list changes.
    let walk = walk_members(doc, page, true)?;
    let doomed: Vec<u32> = if delete_shapes_too {
        walk.listed
            .iter()
            .filter(|(_, _, list)| {
                lookups = lookups.saturating_add(list.len() as u64);
                let mut any = false;
                for ix in list {
                    if gone.contains(ix) {
                        any = true;
                    } else if known.contains(ix) {
                        return false;
                    }
                }
                any
            })
            .map(|(_, id, _)| *id)
            .collect()
    } else {
        Vec::new()
    };
    // The numbers that stay move up by the removed numbers below them: a search of the sorted removed numbers.
    let renumber = |ix: u32| -> Option<u32> {
        match removed.binary_search(&ix) {
            Ok(_) => None,
            Err(below) => ix.checked_sub(u32::try_from(below).unwrap_or(u32::MAX)),
        }
    };
    // The page's own rows: the removed ones go and the rest close up.
    {
        let page_part = page_mut(doc, page).ok_or_else(|| Problem::failed("the page went away"))?;
        let sheet = Arc::make_mut(&mut page_part.sheet);
        let section = sheet.section_mut(&key())?;
        section.rows.retain(|row| row.ix.is_none_or(|ix| !gone.contains(&ix)));
        for row in section.rows.iter_mut() {
            if let Some(ix) = row.ix
                && let Some(moved) = renumber(ix)
                && moved != ix
            {
                Arc::make_mut(row).ix = Some(moved);
            }
        }
    }
    // Every list that changes is written again, in the numbers that stay.
    let mut writes: Vec<(SheetRef, CellAddr, CellEdit)> = Vec::new();
    for (path, _, list) in &walk.listed {
        lookups = lookups.saturating_add(list.len() as u64);
        let next: BTreeSet<u32> = list.iter().filter_map(|ix| renumber(*ix)).collect();
        if next.iter().copied().eq(list.iter().copied()) {
            continue;
        }
        writes.push((
            SheetRef::Shape(ShapeRef { owner: Owner::Page(page), path: path.clone() }),
            CellAddr::direct("LayerMember"),
            CellEdit::Value(Value::from_text(&member_text(&next))),
        ));
    }
    let lists_written = writes.len();
    write_cells(doc, writes)?;
    let doomed_set: HashSet<u32> = doomed.iter().copied().collect();
    let shapes_deleted = if doomed_set.is_empty() { 0 } else { delete_shapes(doc, page, &doomed)? };
    Ok(Deleted { removed, shapes_deleted, lists_written, cost: walk.cost, lookups })
}

/// The numbers of the layers no shape of the page is on.
pub fn unreferenced(doc: &Document, page: u32) -> Result<Vec<u32>, Problem> {
    Ok(page_layers(doc, page)?.into_iter().filter(|l| l.shapes == 0).map(|l| l.ix).collect())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// A document whose first page holds `n` shapes, each on the layers `0` and `1` of a table of two, with the shapes' lists written as the commands write them.
    fn page_of(n: u32) -> Document {
        let mut doc = Document::new_blank();
        let page = doc.pages.get(0).map(|p| p.id).unwrap();
        for name in ["A", "B"] {
            add_layer(&mut doc, page, &Settings { name: Some(name.to_owned()), ..Settings::default() }).unwrap();
        }
        let p = page_mut(&mut doc, page).unwrap();
        for id in 1..=n {
            let mut shape = Shape::new(id);
            let mut sheet = diacraft_doc::Sheet::default();
            sheet.cells.insert(CellName::new("LayerMember"), diacraft_doc::Cell::constant(Value::from_text("0;1")));
            shape.sheet = Arc::new(sheet);
            p.shapes.push(Arc::new(shape));
        }
        doc
    }

    fn walked() -> usize {
        super::super::NODES_WALKED.with(std::cell::Cell::get)
    }

    fn reset() {
        super::super::NODES_WALKED.with(|c| c.set(0));
    }

    /// The work of a call is counted in the nodes of the page it walks, and each call walks the page a fixed number of times, whatever the layers say: a page of 3,000 shapes is
    /// 3,000 nodes for a count of the layers' shapes, for the standing of the shapes, and for the removal of a layer, and two walks for an assignment of half of them.
    #[test]
    fn each_call_walks_the_page_a_fixed_number_of_times() {
        let mut doc = page_of(3_000);
        reset();
        let layers = page_layers(&doc, 1).unwrap();
        assert_eq!((layers.len(), layers[0].shapes, layers[1].shapes), (2, 3_000, 3_000));
        assert_eq!(walked(), 3_000, "the count of the shapes on the layers is one walk");
        // A table of plain layers needs no walk to say nothing is hidden or locked.
        reset();
        assert!(page_standing(&doc, 1).unwrap().is_empty());
        assert_eq!(walked(), 0);
        set_layer(&mut doc, 1, 0, &Settings { lock: Some(true), ..Settings::default() }).unwrap();
        reset();
        assert_eq!(page_standing(&doc, 1).unwrap().len(), 3_000, "every shape is on the locked layer");
        assert_eq!(walked(), 3_000);
        // Removing the other layer writes every list again, from one walk.
        set_layer(&mut doc, 1, 0, &Settings { lock: Some(false), ..Settings::default() }).unwrap();
        reset();
        let done = delete_layers(&mut doc, 1, &[1], false).unwrap();
        assert_eq!((done.lists_written, walked()), (3_000, 3_000));
        // An assignment of 1,500 shapes finds them in one walk (`scan`), and the standing of a page of plain layers costs none.
        let ids: Vec<u32> = (1..=1_500).collect();
        reset();
        let assigned = assign_layers(&mut doc, 1, &ids, &[], Mode::Replace, true).unwrap();
        assert_eq!(assigned.written, 1_500);
        assert_eq!(walked(), 3_000, "one walk to find the shapes; the layers are plain, so no walk for the standing");
    }

    /// A list is bounded and charged: a text over [`MAX_MEMBER_BYTES`] is read as empty and counted, one pieces' worth at most [`MAX_MEMBERS`], a number once, in rising order, one probe
    /// for each piece that is a number, and the walk's budget refuses the page in words when it is spent.
    #[test]
    fn a_list_is_bounded_charged_and_a_set() {
        let mut budget = MemberBudget::default();
        let big = (0..100_000u32).map(|n| (n % 7).to_string()).collect::<Vec<_>>().join(";");
        assert!(big.len() > MAX_MEMBER_BYTES);
        assert!(read_members(&big, &mut budget).unwrap().is_empty(), "a list over the byte cap is read as on no layer");
        assert_eq!((budget.overlong, budget.read, budget.pieces), (1, 0, 0), "and nothing of it was parsed or charged");
        let spread = (0..1_500u32).map(|n| n.to_string()).collect::<Vec<_>>().join(";");
        assert!(spread.len() < MAX_MEMBER_BYTES);
        let listed = read_members(&spread, &mut budget).unwrap();
        assert_eq!(
            (listed.len(), budget.pieces, budget.probes),
            (MAX_MEMBERS, MAX_MEMBERS as u64, MAX_MEMBERS as u64),
            "every piece is counted, at most 1,024 of them"
        );
        let mut one = MemberBudget::default();
        assert_eq!(
            read_members("x;;-1;4294967296; 3 ;3;1;1", &mut one).unwrap(),
            [1, 3],
            "pieces that are no number are dropped, a number is kept once, in rising order"
        );
        assert_eq!((one.pieces, one.probes), (8, 4), "eight pieces, four of them numbers: one probe each");
        // The walk's budget: each text is charged before it is parsed, and the call is refused past the figure.
        let mut spent = MemberBudget { read: MAX_PAGE_MEMBER_BYTES - 2, ..MemberBudget::default() };
        let refused = read_members("0;1", &mut spent).unwrap_err();
        assert!(refused.message.contains("layer lists") && refused.message.contains("at most"), "{}", refused.message);
    }

    /// 76,000 shapes on a thousand layers, each list holding every number twice, is a page the layers are not read for: the walk is refused in words as soon as the bytes of the lists
    /// come to the budget, after a small part of the page and not after all of it. (Parsing every list with a scan of the list for each number is minutes of work.)
    #[test]
    fn a_page_of_shapes_on_a_thousand_layers_each_with_repeats_is_refused_in_words_early() {
        let mut doc = Document::new_blank();
        let page = doc.pages.get(0).map(|p| p.id).unwrap();
        let mut writes = Vec::new();
        for k in 0..1_000u32 {
            for (cell, text) in [("Name", format!("L{k}")), ("Visible", "0".to_owned())] {
                writes.push((SheetRef::Page(page), CellAddr::in_row("Layer", None, RowKey::Ix(k), cell), CellEdit::Value(Value::from_text(&text))));
            }
        }
        write_cells(&mut doc, writes).unwrap();
        // One text, shared by every cell: 2,000 pieces and about 7.8 KB.
        let list: String = (0..1_000u32).chain(0..1_000u32).map(|n| n.to_string()).collect::<Vec<_>>().join(";");
        assert!(list.len() < 16 * 1024);
        let value = Value::from_text(&list);
        let p = page_mut(&mut doc, page).unwrap();
        for id in 1..=76_000u32 {
            let mut shape = Shape::new(id);
            let mut sheet = diacraft_doc::Sheet::default();
            sheet.cells.insert(CellName::new("LayerMember"), diacraft_doc::Cell::constant(value.clone()));
            shape.sheet = Arc::new(sheet);
            p.shapes.push(Arc::new(shape));
        }
        for call in ["page_layers", "page_standing", "delete_layers"] {
            reset();
            let refused = match call {
                "page_layers" => page_layers(&doc, page).map(|_| ()),
                "page_standing" => page_standing(&doc, page).map(|_| ()),
                _ => delete_layers(&mut doc, page, &[0], false).map(|_| ()),
            };
            let words = refused.expect_err(call).message;
            assert!(words.contains("layer memberships") && words.contains("at most"), "{call}: {words}");
            let share = 8 * 1024 * 1024 / list.len() + 2;
            assert!(walked() <= share, "{call}: the walk stopped after {} of 76,000 shapes (the budget holds about {share})", walked());
        }
    }

    /// 10,000 layers on a page of 76,000 shapes is a table the layers are not read for: a removal of 5,000 of them is refused in words before the page is walked at all.
    #[test]
    fn removing_half_of_ten_thousand_layers_is_refused_in_words_before_a_walk() {
        let mut doc = Document::new_blank();
        let page = doc.pages.get(0).map(|p| p.id).unwrap();
        {
            let p = page_mut(&mut doc, page).unwrap();
            let sheet = Arc::make_mut(&mut p.sheet);
            let mut section = diacraft_doc::Section::new("Layer", None);
            for k in 0..10_000u32 {
                let mut row = diacraft_doc::Row { ix: Some(k), ..diacraft_doc::Row::default() };
                row.cells.insert(CellName::new("Name"), diacraft_doc::Cell::constant(Value::from_text(&format!("L{k}"))));
                section.rows.push(Arc::new(row));
            }
            sheet.sections.push(Arc::new(section));
            for id in 1..=76_000u32 {
                let mut shape = Shape::new(id);
                let mut cells = diacraft_doc::Sheet::default();
                cells.cells.insert(CellName::new("LayerMember"), diacraft_doc::Cell::constant(Value::from_text("0;5000")));
                shape.sheet = Arc::new(cells);
                p.shapes.push(Arc::new(shape));
            }
        }
        reset();
        let which: Vec<u32> = (0..10_000).step_by(2).collect();
        let words = delete_layers(&mut doc, page, &which, false).unwrap_err().message;
        assert!(words.contains("10000 layers") && words.contains("at most 4096"), "{words}");
        assert_eq!(walked(), 0, "refused before the page was walked");
    }

    /// The most layers a page holds, 4,096, on a page of 76,000 shapes each on ten of them: removing half of them is one walk, **every number of every list is one probe of a set and
    /// one search of the removed numbers**, and the lists come out renumbered: the work is the numbers read, not the numbers times the layers.
    #[test]
    fn removing_half_of_the_most_layers_from_a_page_of_many_shapes_costs_a_probe_for_each_number() {
        let mut doc = Document::new_blank();
        let page = doc.pages.get(0).map(|p| p.id).unwrap();
        let mut writes = Vec::new();
        for k in 0..MAX_LAYERS as u32 {
            writes.push((
                SheetRef::Page(page),
                CellAddr::in_row("Layer", None, RowKey::Ix(k), "Name"),
                CellEdit::Value(Value::from_text(&format!("L{k}"))),
            ));
        }
        write_cells(&mut doc, writes).unwrap();
        let shapes = 76_000u32;
        {
            let p = page_mut(&mut doc, page).unwrap();
            // Ten numbers each, spread over the table, the same text for every shape.
            let value = Value::from_text("1;3;5;1000;2001;2002;3000;4000;4001;4095");
            for id in 1..=shapes {
                let mut shape = Shape::new(id);
                let mut cells = diacraft_doc::Sheet::default();
                cells.cells.insert(CellName::new("LayerMember"), diacraft_doc::Cell::constant(value.clone()));
                shape.sheet = Arc::new(cells);
                p.shapes.push(Arc::new(shape));
            }
        }
        let which: Vec<u32> = (0..MAX_LAYERS as u32).filter(|k| k % 2 == 0).collect();
        reset();
        let done = delete_layers(&mut doc, page, &which, false).unwrap();
        assert_eq!(walked(), shapes as usize, "one walk of the page");
        let numbers = u64::from(shapes) * 10;
        assert_eq!((done.cost.pieces, done.cost.probes), (numbers, numbers), "every piece read once, one set probe for each");
        assert_eq!(done.lookups, numbers, "one search of the removed numbers for each number: not a scan of the 4,096 rows for each");
        assert_eq!(done.lists_written, shapes as usize);
        // The numbers that stayed, odd ones, moved up by the removed even ones below them: 1 to 0, 3 to 1, 5 to 2, 1000 is removed, 2001 to 1000, 4095 to 2047.
        let at = doc.find_shape(Owner::Page(page), 1).unwrap();
        let text = doc.shape(&at).unwrap().sheet.cell("LayerMember").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned());
        assert_eq!(text.as_deref(), Some("0;1;2;1000;2000;2047"), "the shape list, renumbered");
        assert_eq!(read_layers(&doc, page).unwrap().len(), MAX_LAYERS / 2);
    }
}
