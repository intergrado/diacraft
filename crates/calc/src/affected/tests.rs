//! The index of readers held to the walk it replaced. **`oracle.rs` is the walk of every formula of the page that each edit made before the
//! index; the index keeps its answer from one edit to the next, and these tests hold the two to the same answer.** Over generated pages (shapes and groups,
//! instances of masters with formulas that read beyond their sheets, qualified references by `ID` and by name, frame functions, text measures, path
//! functions, cells read through a call, cells that read a shape that is not there, `Connect` rows) and generated edits (a value, a formula, a formula
//! taken away, a cached result, a row, a shape added in a list or in a group, a shape taken out, a name, a master, a master's cell, a `Connect` row, a
//! page's name, and an undo that goes back to a document from several edits ago), the index of one drawing, kept through all of them, is compared at
//! each step with the walk:
//!
//! * the graph itself: every key and the readers listed under it, in the order, the readers of cells their text does not name and the readers that find no
//!   sheet, the memory the graph is charged for it (a charge for each entry, exactly);
//! * the cells that an edit reaches, in the order they are handed out, when the cells they change are decided by a rule that does not depend on either;
//! * the document after a commit: the same cells written with the same values, and the same report.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use diacraft_doc::{
    CellAddr, CellEdit, Connect, Connects, Document, Lookup, Owner, Row, RowKey, SectionKey, Shape, ShapeList, ShapeRef, SheetRef, Value as DocValue,
    WriteOrigin,
};
use diacraft_formula::parse;
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};
use proptest::prelude::*;

use super::{Affected, Reader, oracle};
use crate::changes::{Changes, changes_between, changes_between_with, each_cell};
use crate::commit::{Pending, apply, work_inherited, work_real};
use crate::eval::{Budget, EvalCtx, EvalOptions, Evaluator, OperandMode};
use crate::index::ReaderIndex;
use crate::rules::{COMMIT_ADDS_DIFFERING_INHERITED_CELLS, KEY_BYTES};
use crate::{CommitReport, commit_changed, commit_changed_with};

thread_local! {
    /// What the graphs the tests looked at held, summed: entries through another sheet, readers of cells no text names, readers that found no sheet, graphs looked at.
    static SEEN: std::cell::RefCell<(usize, usize, usize, usize)> = const { std::cell::RefCell::new((0, 0, 0, 0)) };
    /// The steps in which a section written whole reached cells that the edit's own changes did not, so that the comparison of the index with the walk for one is not empty.
    static WHOLE_REACHED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A small generator of numbers: the property tests give a seed, and a failing case is told by it.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n.max(1)).unwrap_or(1)).unwrap_or(0)
    }

    fn pick<'a, T>(&mut self, of: &'a [T]) -> &'a T {
        &of[self.below(of.len())]
    }
}

const MASTERS: u32 = 3;

/// The master that is a group of three shapes, the last of the `MASTERS`: the first two are drawn as it is.
const GROUP_MASTER: u32 = 10 + MASTERS - 1;

/// A formula for a cell of a shape of a page of `n` shapes: some read their own sheet only, some another by `ID` or by name, some carry a point between
/// frames, measure text, walk a path, read a cell by a call, name a cell that is only an address (`GETVAL`, `GETREF`) or name a shape that is
/// not there.
fn formula(rng: &mut Rng, n: u32) -> String {
    let mut id = || 1 + u32::try_from(rng.next() % u64::from(n + 3)).unwrap_or(1);
    let (a, b) = (id(), id());
    match rng.below(21) {
        0 => "Width*0.5".into(),
        1 => "User.A+PinX".into(),
        2 => "Geometry1.X2*2".into(),
        3 => format!("Sheet.{a}!PinX+1"),
        4 => format!("Sheet.{a}!User.A"),
        5 => format!("Sheet.{a}!User.a*2"),
        6 => format!("PAR(PNT(Sheet.{a}!Connections.X1,Sheet.{a}!Connections.Y1))"),
        7 => "LOC(PNT(1,2))".into(),
        8 => format!("LOCTOPAR(PNT(1,2),Sheet.{a}!PinX,Sheet.{b}!PinX)"),
        9 => "TEXTWIDTH(TheText)".into(),
        10 => format!("TEXTHEIGHT(Sheet.{a}!TheText)"),
        11 => format!("SEGMENTCOUNT(Sheet.{a}!Geometry1.Path)"),
        12 => "SHEETREF()!PinX*2".into(),
        13 => "LISTMEMBERCOUNT()".into(),
        14 => "Sheet.9999!PinX".into(),
        15 => format!("Sheet.{a}!Geometry1.X2+Sheet.{b}!Width"),
        16 => "ThePage!PageWidth".into(),
        17 => format!("IF(Sheet.{a}!Width>1,Width,Sheet.{b}!Height)"),
        // An address is no link: the cell it names does not work this one out (the plain `Width` and `PinX` do).
        18 => format!("GETVAL(Sheet.{a}!PinX)+Width"),
        19 => "GetRef(User.A)*2+PinX".into(),
        _ => format!("IF(Width>1,GETVAL(Sheet.{a}!Width),GETREF(User.a))+Sheet.{b}!Height"),
    }
}

/// A shape of a page of `n` shapes: its cells, sometimes members, or the instance of a master.
fn shape(rng: &mut Rng, ids: &mut std::ops::RangeInclusive<u32>, n: u32, depth: usize) -> Option<ShapeBuilder> {
    let id = ids.next()?;
    let mut s = ShapeBuilder::new(id).cell("PinX", &id.to_string()).cell("PinY", "2").cell("Width", "1").cell("Height", "0.5");
    match rng.below(10) {
        // An instance of the master that is a group: its members stand for the master's (`MasterShape`), and the formulas they inherit read the other member.
        0 if depth + 2 < 3 && rng.below(2) == 0 => {
            s = s.group().master(GROUP_MASTER);
            for stands_for in [2, 3] {
                if let Some(member) = ids.next() {
                    s = s.child(ShapeBuilder::new(member).master_shape(stands_for).cell("PinX", &member.to_string()));
                }
            }
        }
        0 if depth < 3 => {
            s = s.group();
            for _ in 0..1 + rng.below(3) {
                if let Some(child) = shape(rng, ids, n, depth + 1) {
                    s = s.child(child);
                }
            }
        }
        1..=3 => s = s.master(10 + u32::try_from(rng.below(MASTERS as usize)).unwrap_or(0)),
        _ => {}
    }
    for name in ["LocPinX", "LocPinY", "TxtPinX", "TxtPinY", "Angle"] {
        if rng.below(3) == 0 {
            let f = formula(rng, n);
            s = s.formula(name, &f, "1");
        }
    }
    if rng.below(4) == 0 {
        s = s.inh("LocPinX", None);
    }
    for row in ["A", "B"] {
        if rng.below(3) == 0 {
            let f = formula(rng, n);
            s = s.row_formula("User", None, RowKey::name(row), "Value", &f, "1");
        }
    }
    if rng.below(3) == 0 {
        let f = formula(rng, n);
        s = s.row_cell("Geometry", Some(1), RowKey::Ix(1), "X", "1").row_formula("Geometry", Some(1), RowKey::Ix(2), "X", &f, "2");
    }
    if rng.below(8) == 0 {
        s = s.named(&format!("Box{id}"), &format!("Box{id}"));
    }
    Some(s)
}

/// A page of about `n` shapes, with three masters, a few `Connect` rows, and a second page.
fn generate(rng: &mut Rng, n: u32) -> Document {
    let mut docs = DocBuilder::new();
    for m in 0..MASTERS {
        let mut top = ShapeBuilder::new(1).cell("Width", "1").cell("Height", "1");
        for name in ["LocPinX", "LocPinY", "TxtPinX"] {
            if rng.below(2) == 0 {
                let f =
                    *rng.pick(&["Width*0.5", "Sheet.1!Width*0.5", "PAR(PNT(Width,Height))", "TEXTWIDTH(TheText)", "LISTMEMBERCOUNT()", "Height*2"]);
                top = top.formula(name, f, "1");
            }
        }
        if 10 + m == GROUP_MASTER {
            // A group of `1` with two members, `2` and `3`, that name each other: an instance of it holds two members that stand for them.
            let member = |id: u32, other: u32| {
                ShapeBuilder::new(id).cell("Width", "1").formula("LocPinX", &format!("Sheet.{other}!Width*0.5"), "1").formula(
                    "Angle",
                    &format!("PAR(PNT(Sheet.{other}!Connections.X1,0))"),
                    "0",
                )
            };
            let group = ShapeBuilder::new(1).group().cell("Width", "2").child(member(2, 3)).child(member(3, 2));
            docs = docs.master(GROUP_MASTER, MasterBuilder::new().name("Group").shape(group));
            continue;
        }
        docs = docs.master(10 + m, MasterBuilder::new().name(&format!("M{m}")).shape(top));
    }
    let mut page = PageBuilder::new();
    let mut ids = 1..=n;
    while let Some(top) = shape(rng, &mut ids, n, 0) {
        page = page.shape(top);
    }
    let mut doc = docs.page(1, page).page(2, PageBuilder::new().name("Other")).build();
    let mut connects = Vec::new();
    for _ in 0..rng.below(5) {
        let from = 1 + u32::try_from(rng.below(n as usize)).unwrap_or(0);
        let to = 1 + u32::try_from(rng.below(n as usize)).unwrap_or(0);
        let cell: Arc<str> = Arc::from(*rng.pick(&["BeginX", "EndY", "EndX"]));
        connects.push(Connect { from_sheet: from, from_cell: Some(cell), to_sheet: to, to_cell: Some(Arc::from("PinX")), ..Connect::default() });
    }
    if let Some(first) = doc.pages.get_mut(0) {
        Arc::make_mut(first).connects = Connects::from_vec(connects);
    }
    doc
}

/// Every shape of page 1, a group before its members.
fn shapes_of(doc: &Document) -> Vec<ShapeRef> {
    fn walk(list: &ShapeList, path: &mut Vec<u32>, out: &mut Vec<ShapeRef>) {
        for (i, shape) in list.iter().enumerate() {
            path.push(u32::try_from(i).unwrap_or(0));
            out.push(ShapeRef { owner: Owner::Page(1), path: path.clone() });
            walk(&shape.children, path, out);
            path.pop();
        }
    }
    let mut out = Vec::new();
    if let Some(page) = doc.page(1) {
        walk(&page.shapes, &mut Vec::new(), &mut out);
    }
    out
}

fn shape_in<'a>(list: &'a mut ShapeList, path: &[u32]) -> Option<&'a mut Shape> {
    let (first, rest) = path.split_first()?;
    let shape = Arc::make_mut(list.get_mut(usize::try_from(*first).ok()?)?);
    if rest.is_empty() { Some(shape) } else { shape_in(&mut shape.children, rest) }
}

/// The list the shape at `path` is in, and its place in it (`None` for a shape of the page's top list: `path` of one number).
fn list_of<'a>(doc: &'a mut Document, path: &[u32]) -> Option<(&'a mut ShapeList, usize)> {
    let (last, up) = path.split_last()?;
    let page = Arc::make_mut(doc.pages.get_mut(0)?);
    let list = if up.is_empty() { &mut page.shapes } else { &mut shape_in(&mut page.shapes, up)?.children };
    Some((list, usize::try_from(*last).ok()?))
}

/// A cell of a shape, by `each_cell`, picked at random.
fn some_cell(rng: &mut Rng, doc: &Document, at: &ShapeRef) -> Option<CellAddr> {
    let shape = doc.shape(at)?;
    let mut cells: Vec<CellAddr> = Vec::new();
    each_cell(&shape.sheet, |addr, _| cells.push(addr));
    (!cells.is_empty()).then(|| cells[rng.below(cells.len())].clone())
}

/// An edit of the drawing, picked at random. `history` is the documents of the steps before, which an undo goes back to.
fn edit(rng: &mut Rng, doc: &mut Document, n: u32, history: &[Document]) {
    let shapes = shapes_of(doc);
    let at = (!shapes.is_empty()).then(|| shapes[rng.below(shapes.len())].clone());
    match rng.below(24) {
        // A value in a cell the shape holds: a move, a fill; on a cell with a formula it takes the formula away.
        0..=3 => {
            if let Some(at) = at
                && let Some(addr) = some_cell(rng, doc, &at)
            {
                let _ = doc.write_cell(&SheetRef::Shape(at), &addr, CellEdit::Value(DocValue::from_text("7")), WriteOrigin::Drawing);
            }
        }
        // A formula in a cell, one it has or a new one.
        4..=6 => {
            if let Some(at) = at {
                let addr = match rng.below(3) {
                    0 => some_cell(rng, doc, &at),
                    1 => Some(CellAddr::direct(rng.pick(&["LocPinX", "Angle", "TxtPinY", "FlipX"]))),
                    _ => Some(CellAddr::in_row("User", None, RowKey::name(rng.pick(&["A", "B", "Fresh"])), "Value")),
                };
                let text = formula(rng, n);
                if let Some(addr) = addr {
                    let edit = CellEdit::Formula { formula: Arc::new(parse(&text)), value: Some(DocValue::from_text("3")) };
                    let _ = doc.write_cell(&SheetRef::Shape(at), &addr, edit, WriteOrigin::Drawing);
                }
            }
        }
        7 => {
            if let Some(at) = at
                && let Some(addr) = some_cell(rng, doc, &at)
            {
                let _ = doc.write_cell(&SheetRef::Shape(at), &addr, CellEdit::ClearFormula, WriteOrigin::Drawing);
            }
        }
        // A result of an evaluation: the cell's value, not its formula.
        8 | 9 => {
            if let Some(at) = at
                && let Some(addr) = some_cell(rng, doc, &at)
            {
                let edit = CellEdit::Cached { value: DocValue::from_text("5"), error: None, unit: None };
                let _ = doc.write_cell(&SheetRef::Shape(at), &addr, edit, WriteOrigin::Evaluation);
            }
        }
        // A shape added: at the end of the page or among the others, or in a group.
        10 | 11 => {
            let id = n + 10 + u32::try_from(rng.below(1000)).unwrap_or(0);
            let mut ids = id..=id;
            let donor =
                DocBuilder::new().page(1, PageBuilder::new().shape(shape(rng, &mut ids, n, 3).unwrap_or_else(|| ShapeBuilder::new(id)))).build();
            let Some(new) = donor.page(1).and_then(|p| p.shapes.get(0)).cloned() else { return };
            let path: Vec<u32> = match (&at, rng.below(3)) {
                (Some(at), 0) => at.path.clone(),
                (_, 1) => vec![0],
                _ => vec![u32::try_from(shapes.len()).unwrap_or(0)],
            };
            if let Some((list, place)) = list_of(doc, &path) {
                let _ = list.insert(place.min(list.len()), new);
            }
        }
        // A shape taken out.
        12 => {
            if let Some(at) = at
                && let Some((list, place)) = list_of(doc, &at.path)
            {
                list.remove(place);
            }
        }
        // A name, a master.
        13 | 14 => {
            if let Some(at) = at
                && let Some(page) = doc.pages.get_mut(0)
                && let Some(s) = shape_in(&mut Arc::make_mut(page).shapes, &at.path)
            {
                if rng.below(2) == 0 {
                    s.name_u = Some(Arc::from(format!("N{}", rng.below(5))));
                } else {
                    s.master = rng.pick(&[None, Some(10), Some(11), Some(12), Some(99)]).to_owned();
                }
            }
        }
        // A cell of a master.
        15 => {
            let master = 10 + u32::try_from(rng.below(MASTERS as usize)).unwrap_or(0);
            let text = formula(rng, 1);
            let sheet = SheetRef::Shape(ShapeRef { owner: Owner::Master(master), path: vec![0] });
            let edit = CellEdit::Formula { formula: Arc::new(parse(&text)), value: Some(DocValue::from_text("3")) };
            let _ = doc.write_cell(&sheet, &CellAddr::direct(rng.pick(&["LocPinX", "LocPinY", "Angle"])), edit, WriteOrigin::Drawing);
        }
        // A `Connect` row, or none.
        16 => {
            if let Some(page) = doc.pages.get_mut(0) {
                let page = Arc::make_mut(page);
                if rng.below(3) == 0 {
                    page.connects = Connects::new();
                } else {
                    let (from, to) = (1 + u32::try_from(rng.below(n as usize)).unwrap_or(0), 1 + u32::try_from(rng.below(n as usize)).unwrap_or(0));
                    page.connects.push(Connect {
                        from_sheet: from,
                        from_cell: Some(Arc::from("EndX")),
                        to_sheet: to,
                        to_cell: Some(Arc::from("PinX")),
                        ..Connect::default()
                    });
                }
            }
        }
        // The name of a page.
        17 => {
            let _ = doc.rename_page(if rng.below(2) == 0 { 1 } else { 2 }, &format!("Name{}", rng.below(4)));
        }
        // An undo, or a jump to an earlier document: the index is for the one before.
        18 => {
            if !history.is_empty() {
                *doc = history[rng.below(history.len())].clone();
            }
        }
        // A shape given the `ID` of another: two shapes of a list may hold one `ID`, which a comparison by `ID` cannot tell apart.
        19 => {
            if let Some(at) = at
                && let Some(other) = doc.shape(&shapes[rng.below(shapes.len())]).map(|s| s.id)
                && let Some(page) = doc.pages.get_mut(0)
                && let Some(s) = shape_in(&mut Arc::make_mut(page).shapes, &at.path)
            {
                s.id = other;
            }
        }
        // A copy of a shape beside it: the same `ID` twice in one list.
        20 => {
            if let Some(at) = at
                && let Some(copy) = doc.shape(&at).cloned()
                && let Some((list, place)) = list_of(doc, &at.path)
            {
                let _ = list.insert(place + 1, copy);
            }
        }
        // A row with no key in the first section of a sheet (compared by its place among the rows of no key), or the sheet's line style link changed, which
        // no comparison of the sheet can attribute to a cell: it gives up, and the shape is new in every cell.
        21 => {
            let keyless = rng.below(2) == 0;
            if let Some(at) = at
                && let Some(page) = doc.pages.get_mut(0)
                && let Some(s) = shape_in(&mut Arc::make_mut(page).shapes, &at.path)
            {
                let sheet = Arc::make_mut(&mut s.sheet);
                if keyless {
                    if let Some(section) = sheet.sections.get_mut(0) {
                        Arc::make_mut(section).rows.push(Arc::new(Row::default()));
                    }
                } else {
                    sheet.links.line = if sheet.links.line.is_some() { None } else { Some(0) };
                }
            }
        }
        // The same sheet in another allocation: no change in content, and none for a comparison by pointer to skip.
        22 => {
            if let Some(at) = at
                && let Some(page) = doc.pages.get_mut(0)
                && let Some(s) = shape_in(&mut Arc::make_mut(page).shapes, &at.path)
            {
                s.sheet = Arc::new((*s.sheet).clone());
            }
        }
        // The same value again: another allocation, the same content.
        _ => {
            if let Some(at) = at
                && let Some(addr) = some_cell(rng, doc, &at)
                && let Some(Lookup::Found(cell)) = doc.shape(&at).map(|s| s.sheet.lookup(&addr))
                && let Some(value) = cell.value.clone()
            {
                let _ = doc.write_cell(&SheetRef::Shape(at), &addr, CellEdit::Cached { value, error: None, unit: None }, WriteOrigin::Evaluation);
            }
        }
    }
}

fn options() -> EvalOptions {
    EvalOptions { operands: OperandMode::Live, ..EvalOptions::default() }
}

/// Whether the cell a walk reached is one that changes, decided by the seed and the cell: neither walk has a say in it.
fn changes_to(seed: u64, reader: &Reader) -> bool {
    let mut h = DefaultHasher::new();
    seed.hash(&mut h);
    reader.hash(&mut h);
    !h.finish().is_multiple_of(3)
}

/// The cells the index hands out for `changes`, in order, when each one that `changes_to` says changes passes the change on.
fn reached_by_index(index: &mut ReaderIndex, doc: &Document, changes: &Changes, seed: u64) -> Vec<Reader> {
    let mut ctx = EvalCtx::new(options(), Budget::default());
    let mut ev = Evaluator::new(doc, &mut ctx);
    let mut work = Affected::new(&mut ev, doc, 1, changes, index);
    let mut out = Vec::new();
    while let Some(reader) = work.next() {
        if changes_to(seed, &reader) {
            work.changed(&mut ev, &reader);
        }
        out.push(reader);
    }
    out
}

/// The same by the walk of the whole page.
fn reached_by_walk(doc: &Document, changes: &Changes, seed: u64) -> Vec<Reader> {
    let mut ctx = EvalCtx::new(options(), Budget::default());
    let mut ev = Evaluator::new(doc, &mut ctx);
    let mut work = oracle::Affected::new(&mut ev, doc, 1, changes);
    let mut out = Vec::new();
    while let Some(reader) = work.next() {
        if changes_to(seed, &reader) {
            work.changed(&mut ev, &reader);
        }
        out.push(reader);
    }
    out
}

/// What the index says of the page, with the graph a walk makes of it, entry for entry, and the memory the entries are charged.
fn graph_matches_the_walk(index: &mut ReaderIndex, doc: &Document) -> Result<(), String> {
    let mut ctx = EvalCtx::new(options(), Budget::default());
    let mut ev = Evaluator::new(doc, &mut ctx);
    let kept = index.sync(&mut ev, doc, 1).ok_or("no graph")?;
    let mut walked = oracle::Graph::default();
    let mut ctx2 = EvalCtx::new(options(), Budget::default());
    let mut ev2 = Evaluator::new(doc, &mut ctx2);
    oracle::list_cross_readers(&mut ev2, doc, 1, &mut walked);
    let as_readers = |entries: &[crate::index::Entry]| -> Vec<Reader> { entries.iter().map(|e| e.reader.clone()).collect() };
    if kept.cross.len() != walked.cross.len() {
        return Err(format!("{} keys kept, {} walked", kept.cross.len(), walked.cross.len()));
    }
    for (key, readers) in &walked.cross {
        match kept.cross.get(key) {
            Some(entries) if &as_readers(entries) == readers => {}
            other => return Err(format!("readers of {key:?}: kept {:?}, walked {readers:?}", other.map(|e| as_readers(e)))),
        }
    }
    if as_readers(&kept.dynamic) != walked.dynamic {
        return Err(format!("dynamic: kept {:?}, walked {:?}", as_readers(&kept.dynamic), walked.dynamic));
    }
    let named = |entries: &[crate::index::Entry]| -> Vec<(Reader, Option<u32>)> { entries.iter().map(|e| (e.reader.clone(), e.named)).collect() };
    if named(&kept.dangling) != walked.dangling {
        return Err(format!("dangling: kept {:?}, walked {:?}", named(&kept.dangling), walked.dangling));
    }
    // Each entry is charged for, once: a cell read through another sheet or a frame, and a reference that finds no sheet.
    let entries: usize = kept.cross.values().map(Vec::len).sum::<usize>() + kept.dangling.len();
    SEEN.with(|s| {
        let mut s = s.borrow_mut();
        s.0 += kept.cross.values().map(Vec::len).sum::<usize>();
        s.1 += kept.dynamic.len();
        s.2 += kept.dangling.len();
        s.3 += 1;
    });
    let held = kept.bytes;
    if held != 2 * KEY_BYTES * u64::try_from(entries).unwrap_or(u64::MAX) {
        return Err(format!("{entries} entries are charged {held}"));
    }
    Ok(())
}

/// [`commit_changed`] with the walk of the whole page: the loop of `commit_changed_with` over [`oracle::Affected`].
fn commit_by_walk(doc: &mut Document, changes: &Changes) -> CommitReport {
    if changes.whole_page || doc.page(1).is_none() {
        return commit_changed(doc, &Budget::default(), EvalOptions::default(), 1, changes);
    }
    let mut report = CommitReport::default();
    let mut pending: Vec<Pending> = Vec::new();
    let mut ctx = EvalCtx::new(options(), Budget::default());
    if !changes.is_empty() {
        let snapshot: &Document = doc;
        let mut ev = Evaluator::new(snapshot, &mut ctx);
        let mut work = oracle::Affected::new(&mut ev, snapshot, 1, changes);
        while let Some((sheet, addr)) = work.next() {
            if ev.ctx.stats.budget_exhausted {
                break;
            }
            let held = ev.res.sheet(&sheet).map(|own| ev.res.lookup(own, &addr));
            let wrote = match held {
                Some(Lookup::Found(_)) => work_real(&mut ev, snapshot, &sheet, addr.clone(), &mut report, &mut pending, false),
                Some(Lookup::Missing) if COMMIT_ADDS_DIFFERING_INHERITED_CELLS => {
                    work_inherited(&mut ev, snapshot, &sheet, addr.clone(), &mut report, &mut pending, false)
                }
                _ => false,
            };
            if wrote {
                work.changed(&mut ev, &(sheet, addr));
            }
        }
    }
    report.stats = ctx.stats;
    report.budget_exhausted = ctx.stats.budget_exhausted;
    report.memory_exhausted = ctx.stats.memory_exhausted;
    apply(doc, pending, &mut report);
    report
}

/// One generated drawing, edited `steps` times, the index kept through all of it. `Err` says what differed.
fn run(seed: u64, shapes: u32, steps: usize) -> Result<(), String> {
    let mut rng = Rng(seed | 1);
    let mut doc = generate(&mut rng, shapes);
    let mut index = ReaderIndex::new();
    let mut history: Vec<Document> = vec![doc.clone()];
    // The first commit of an unedited page, which makes the index.
    graph_matches_the_walk(&mut index, &doc).map_err(|why| format!("the first graph: {why}"))?;
    for step in 0..steps {
        let before = doc.clone();
        edit(&mut rng, &mut doc, shapes, &history);
        let changes = changes_between(&before, &doc, 1);
        // The comparison in step, from what the index remembers, says what the comparison by `ID` says: the same cells, rows, shapes and `ID`s, in the same order.
        let in_step = changes_between_with(&mut index, &before, &doc, 1);
        // The end cells of the connectors whose `Connect` rows changed are listed in the order a hash set gives them, which differs from one call to the next
        // (`connect_changes`, as it always did): then the cells are compared as a list sorted, and as a list in order when no `Connect` row changed.
        let sorted = |c: &Changes| {
            let mut c = c.clone();
            c.cells.sort_by_key(|(sheet, addr)| format!("{sheet:?}{addr:?}"));
            c
        };
        let connects_changed = before.page(1).map(|p| &p.connects) != doc.page(1).map(|p| &p.connects);
        let alike = if connects_changed { sorted(&in_step) == sorted(&changes) } else { in_step == changes };
        if !alike {
            return Err(format!("step {step}: the changes in step differ: {in_step:?}, by ID {changes:?}"));
        }
        let seed_of_step = seed ^ (step as u64) << 8;
        // The graph is looked at on some steps, so that the index is brought up to date by a commit on the others, from what the last commit and this edit changed.
        match rng.below(3) {
            0 => graph_matches_the_walk(&mut index, &doc).map_err(|why| format!("step {step}, the graph: {why}"))?,
            1 => {
                let kept = reached_by_index(&mut index, &doc, &changes, seed_of_step);
                let walked = reached_by_walk(&doc, &changes, seed_of_step);
                if kept != walked {
                    return Err(format!("step {step}, the cells reached: kept {kept:?}, walked {walked:?}"));
                }
            }
            _ => {}
        }
        // A route written whole (`Changes::sections`): the first section of one shape, chosen by the seed of the step, wakes the readers of any cell of it, in the index as in the walk.
        let places = shapes_of(&doc);
        if let Some(at) = places.get(usize::try_from(seed_of_step % (places.len().max(1) as u64)).unwrap_or(0)) {
            let mut whole = changes.clone();
            whole.sections.push((SheetRef::Shape(at.clone()), SectionKey::new("Geometry", Some(0))));
            let kept = reached_by_index(&mut index, &doc, &whole, seed_of_step);
            let walked = reached_by_walk(&doc, &whole, seed_of_step);
            if kept != walked {
                return Err(format!("step {step}, a section written whole at {at:?}: kept {kept:?}, walked {walked:?}"));
            }
            if kept.len() > reached_by_walk(&doc, &changes, seed_of_step).len() {
                WHOLE_REACHED.with(|n| n.set(n.get() + 1));
            }
        }
        let (mut with_index, mut with_walk) = (doc.clone(), doc.clone());
        let a = commit_changed_with(&mut index, &mut with_index, &Budget::default(), EvalOptions::default(), 1, &changes);
        let b = commit_by_walk(&mut with_walk, &changes);
        if with_index != with_walk {
            return Err(format!("step {step}: the documents differ after the commit ({a:?}, {b:?})"));
        }
        let counts = |r: &CommitReport| (r.written, r.added, r.unchanged, r.skipped, r.refused, r.budget_exhausted, r.memory_exhausted);
        if counts(&a) != counts(&b) {
            return Err(format!("step {step}: the reports differ: {a:?}, {b:?}"));
        }
        doc = with_index;
        history.push(doc.clone());
        if rng.below(12) == 0 {
            index = ReaderIndex::new();
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 1_000, max_shrink_iters: 0, ..ProptestConfig::default() })]

    /// The index of a drawing, kept through generated edits, is the walk of the page each edit made before it: the graph, the cells an edit reaches and the
    /// document after the commit (see the module documentation).
    #[test]
    fn the_index_kept_through_edits_is_the_walk_of_the_whole_page(seed in any::<u64>(), shapes in 2u32..28, steps in 1usize..14) {
        if let Err(why) = run(seed, shapes, steps) {
            prop_assert!(false, "seed {seed}, {shapes} shapes, {steps} steps: {why}");
        }
    }
}

/// The property above is not empty: over generated drawings and edits the index goes down every path it has (it is made, kept as it was, put right from what
/// the edits replaced, given up for a change it does not follow, a sheet's formulas classified again, a sheet whose values alone were replaced, the sheets that
/// read a sheet whose rows changed put right), and the graphs hold entries of every kind.
#[test]
fn the_generated_edits_go_down_every_path_of_the_index() {
    use crate::index::tally;
    for seed in 0..400_u64 {
        run(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1, 2 + (seed % 24) as u32, 12).unwrap();
    }
    let counts = [
        ("made", tally::get(&tally::MADE)),
        ("kept as it was", tally::get(&tally::SAME_PAGE)),
        ("put right", tally::get(&tally::PUT_RIGHT)),
        ("given up", tally::get(&tally::GIVEN_UP)),
        ("classified again", tally::get(&tally::RECLASSIFIED)),
        ("values only", tally::get(&tally::VALUES_ONLY)),
        ("rows changed", tally::get(&tally::ROWS_CHANGED)),
        ("compared in step", tally::get(&tally::IN_STEP)),
        ("compared by ID", tally::get(&tally::BY_ID)),
        ("compared in step with a comparison of sheets giving up", tally::get(&tally::GAVE_UP_IN_STEP)),
    ];
    for (what, n) in counts {
        assert!(n > 0, "no graph was {what}: {counts:?}");
    }
    let (cross, dynamic, dangling, graphs) = SEEN.with(|s| *s.borrow());
    assert!(cross > 0 && dynamic > 0 && dangling > 0 && graphs > 0, "{cross} {dynamic} {dangling} {graphs}");
    let whole = WHOLE_REACHED.with(std::cell::Cell::get);
    assert!(whole > 0, "no section written whole reached a cell that the edit's changes did not");
    eprintln!("paths {counts:?}; entries {cross} through other sheets, {dynamic} dynamic, {dangling} dangling, in {graphs} graphs");
}

/// A page of three shapes, the second with a `Geometry` section of one row.
fn small_page() -> Document {
    let rows = ShapeBuilder::new(2).cell("PinX", "2").row_cell("Geometry", Some(1), RowKey::Ix(1), "X", "1");
    DocBuilder::new()
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(1).cell("PinX", "1")).shape(rows).shape(ShapeBuilder::new(3).cell("PinX", "3")))
        .build()
}

/// A sheet that another allocation holds the same content of is no change, and the comparison by `ID` says so; so does the comparison in step, **also when a row of the
/// sheet has no key** (compared by its place among the rows of no key; comparing the sheets gave up on it before): the comparison by `ID` skips two lists
/// that are equal without looking at their sheets, and the comparison in step must say the same of a shape that is equal in content though it is not the same allocation.
#[test]
fn an_equal_copy_of_a_sheet_with_a_row_that_has_no_key_is_no_change_by_id_and_in_step() {
    let mut keyless = small_page();
    let page = Arc::make_mut(keyless.pages.get_mut(0).unwrap());
    let shape = Arc::make_mut(page.shapes.get_mut(1).unwrap());
    let section = Arc::make_mut(Arc::make_mut(&mut shape.sheet).sections.get_mut(0).unwrap());
    section.rows.push(Arc::new(Row::default()));
    let mut copy = keyless.clone();
    let page = Arc::make_mut(copy.pages.get_mut(0).unwrap());
    let shape = Arc::make_mut(page.shapes.get_mut(1).unwrap());
    shape.sheet = Arc::new((*shape.sheet).clone());
    assert!(!Arc::ptr_eq(
        &keyless.shape(&ShapeRef { owner: Owner::Page(1), path: vec![1] }).unwrap().sheet,
        &copy.shape(&ShapeRef { owner: Owner::Page(1), path: vec![1] }).unwrap().sheet
    ));
    let by_id = changes_between(&keyless, &copy, 1);
    assert!(by_id.is_empty(), "equal lists say nothing: {by_id:?}");
    let in_step = changes_between_with(&mut ReaderIndex::new(), &keyless, &copy, 1);
    assert_eq!(in_step, by_id);
    // With another shape different as well the lists are not equal, and the two comparisons say the same of both shapes.
    let mut other = copy.clone();
    let before = other.clone();
    let sheet = SheetRef::Shape(ShapeRef { owner: Owner::Page(1), path: vec![2] });
    other.write_cell(&sheet, &CellAddr::direct("PinX"), CellEdit::Value(DocValue::from_text("9")), WriteOrigin::Drawing).unwrap();
    let by_id = changes_between(&keyless, &other, 1);
    let in_step = changes_between_with(&mut ReaderIndex::new(), &keyless, &other, 1);
    assert_eq!(in_step, by_id);
    assert!(!by_id.fresh.is_empty() || !by_id.cells.is_empty(), "{by_id:?}");
    assert_eq!(changes_between_with(&mut ReaderIndex::new(), &before, &other, 1), changes_between(&before, &other, 1));
}

/// Two shapes of a list that hold one `ID` are told apart by their order (a file may keep a deleted shape beside a live one of its `ID`, and a page worked out whole
/// gave every instance on it the inherited cells the edit never reached), by `ID` and in step alike, whatever the index remembers of the page before; and when the `ID`s are in
/// order again the comparison in step is as it was.
#[test]
fn two_shapes_of_a_list_with_one_id_are_told_apart_by_their_order_by_id_and_in_step() {
    let mut index = ReaderIndex::new();
    let clean = small_page();
    // The index has seen the page with its `ID`s in order, and an edit of a value is compared in step.
    let mut edited = clean.clone();
    let sheet = SheetRef::Shape(ShapeRef { owner: Owner::Page(1), path: vec![0] });
    edited.write_cell(&sheet, &CellAddr::direct("PinX"), CellEdit::Value(DocValue::from_text("4")), WriteOrigin::Drawing).unwrap();
    let first = changes_between_with(&mut index, &clean, &edited, 1);
    assert_eq!(first, changes_between(&clean, &edited, 1));
    assert_eq!(first.cells.len(), 1);
    // A shape takes another's `ID`: two of one list hold one `ID`.
    let mut twice = edited.clone();
    Arc::make_mut(Arc::make_mut(twice.pages.get_mut(0).unwrap()).shapes.get_mut(2).unwrap()).id = 1;
    let moved = SheetRef::Shape(ShapeRef { owner: Owner::Page(1), path: vec![1] });
    let mut twice_edited = twice.clone();
    twice_edited.write_cell(&moved, &CellAddr::direct("PinX"), CellEdit::Value(DocValue::from_text("5")), WriteOrigin::Drawing).unwrap();
    for (before, after) in [(&edited, &twice), (&twice, &twice_edited), (&twice_edited, &edited)] {
        let by_id = changes_between(before, after, 1);
        let in_step = changes_between_with(&mut index, before, after, 1);
        assert_eq!(in_step, by_id);
    }
    let found = changes_between_with(&mut index, &twice, &twice_edited, 1);
    assert!(!found.whole_page, "a page with two shapes of one `ID` is compared shape by shape: {found:?}");
    assert_eq!(found.cells, vec![(moved.clone(), CellAddr::direct("PinX"))], "the one cell the edit wrote");
    // Back to `ID`s in order: an edit is compared in step again and finds the one cell.
    let mut again = edited.clone();
    let sheet = SheetRef::Shape(ShapeRef { owner: Owner::Page(1), path: vec![2] });
    again.write_cell(&sheet, &CellAddr::direct("PinX"), CellEdit::Value(DocValue::from_text("6")), WriteOrigin::Drawing).unwrap();
    let found = changes_between_with(&mut index, &edited, &again, 1);
    assert!(!found.whole_page);
    assert_eq!(found, changes_between(&edited, &again, 1));
    assert_eq!(found.cells, vec![(sheet, CellAddr::direct("PinX"))]);
}

/// A page of `n` shapes, each with a cell that reads a cell of the shape before it, and the document after a write to the pin of the first.
fn chain_page(n: u32) -> (Document, Document) {
    let mut page = PageBuilder::new();
    for id in 1..=n {
        let mut shape = ShapeBuilder::new(id).cell("PinX", "1");
        if id > 1 {
            // The second reads the pin of the first, and each after it the cell of the one before: a change to the pin goes down the whole chain.
            let reads = if id == 2 { "PinX".to_string() } else { "User.C".to_string() };
            shape = shape.row_formula("User", None, RowKey::name("C"), "Value", &format!("Sheet.{}!{reads}+1", id - 1), "2");
        }
        page = page.shape(shape);
    }
    let before = DocBuilder::new().page(1, page).build();
    let mut after = before.clone();
    let sheet = SheetRef::Shape(ShapeRef { owner: Owner::Page(1), path: vec![0] });
    after.write_cell(&sheet, &CellAddr::direct("PinX"), CellEdit::Value(DocValue::from_text("5")), WriteOrigin::Drawing).unwrap();
    (before, after)
}

/// A graph that is kept is charged to every commit that uses it, as the walk was charged when it made it: a commit whose budget cannot hold the graph ends as out of memory
/// and leaves none, and the commit after it, with the memory it needs, makes the graph again and does what a commit with no index does.
#[test]
fn a_graph_that_is_kept_is_charged_to_the_commit_that_uses_it_and_a_budget_too_small_leaves_none() {
    let (before, after) = chain_page(30);
    let changes = changes_between(&before, &after, 1);
    let mut index = ReaderIndex::new();
    let mut first = after.clone();
    let made = commit_changed_with(&mut index, &mut first, &Budget::default(), EvalOptions::default(), 1, &changes);
    assert!(!made.budget_exhausted);
    let held = index.held_bytes();
    // Thirty shapes, each reading the one before: 29 readers, each charged for what it was before the index.
    assert_eq!(held, 2 * KEY_BYTES * 29);
    // The memory of the commit cannot hold the graph that is kept (nor the cells it queues): it ends as out of memory, and no graph is kept.
    let mut tight = after.clone();
    let small = Budget { bytes: held - 1, ..Budget::default() };
    let ended = commit_changed_with(&mut index, &mut tight, &small, EvalOptions::default(), 1, &changes);
    assert!(ended.memory_exhausted && ended.budget_exhausted, "{ended:?}");
    assert_eq!(index.held_bytes(), 0, "a graph that could not be charged is not kept");
    assert_eq!(tight, after, "nothing was written");
    // With the memory it needs the graph is made again, and the commit is the one that has no index.
    let mut again = after.clone();
    let report = commit_changed_with(&mut index, &mut again, &Budget::default(), EvalOptions::default(), 1, &changes);
    let mut plain = after.clone();
    let expected = commit_changed(&mut plain, &Budget::default(), EvalOptions::default(), 1, &changes);
    assert_eq!(again, plain);
    assert_eq!((report.written, report.unchanged, report.skipped), (expected.written, expected.unchanged, expected.skipped));
    assert!(report.written >= 29, "{report:?}");
    assert_eq!(index.held_bytes(), held);
}

/// A budget of steps that runs out while the graph is made leaves none (a graph that is partly made would hide readers from the commits after it), and the commits after it are
/// right.
#[test]
fn a_budget_of_steps_that_runs_out_while_the_graph_is_made_leaves_none() {
    let (before, after) = chain_page(60);
    let changes = changes_between(&before, &after, 1);
    let mut index = ReaderIndex::new();
    let mut short = after.clone();
    let ended = commit_changed_with(&mut index, &mut short, &Budget::with_steps(40), EvalOptions::default(), 1, &changes);
    assert!(ended.budget_exhausted && !ended.memory_exhausted, "{ended:?}");
    assert_eq!(index.held_bytes(), 0);
    let mut full = after.clone();
    let report = commit_changed_with(&mut index, &mut full, &Budget::default(), EvalOptions::default(), 1, &changes);
    let mut plain = after.clone();
    commit_changed(&mut plain, &Budget::default(), EvalOptions::default(), 1, &changes);
    assert_eq!(full, plain);
    assert!(report.written >= 59, "{report:?}");
}

/// A graph past the figure a session may keep is let go of at the end of a commit, and one at it is kept.
#[test]
fn a_graph_past_the_most_that_is_kept_is_let_go_of() {
    let (before, after) = chain_page(20);
    let changes = changes_between(&before, &after, 1);
    let mut index = ReaderIndex::new();
    commit_changed_with(&mut index, &mut after.clone(), &Budget::default(), EvalOptions::default(), 1, &changes);
    let held = index.held_bytes();
    assert!(held > 0 && held <= crate::index::MAX_KEPT_BYTES);
    index.finish_within(held);
    assert_eq!(index.held_bytes(), held, "a graph at the figure is kept");
    index.finish_within(held - 1);
    assert_eq!(index.held_bytes(), 0, "a graph past it is not");
}

/// `n` shapes whose `Width` reads their own `PinX` and whose `Height` reads the `Width` of the shape before, each holding one `Geometry` section of `rows` rows of
/// constants between them (one allocation, as the copies of a shape hold it: a paste, a file's thousand copies of one connector), and a copy with every `PinX` written.
fn shapes_sharing_a_section(n: u32, rows: u32) -> (Document, Document) {
    let mut page = PageBuilder::new();
    for id in 1..=n {
        let mut shape = ShapeBuilder::new(id).cell_u("PinX", "1", "IN").formula("Width", "PinX*2", "2");
        if id > 1 {
            shape = shape.formula("Height", &format!("Sheet.{}!Width", id - 1), "2");
        }
        page = page.shape(shape);
    }
    let mut before = DocBuilder::new().page(1, page).build();
    let mut section = diacraft_doc::Section::new("Geometry", Some(0));
    for k in 1..=rows {
        let mut row = Row { ix: Some(k), row_type: Some(Arc::from(if k == 1 { "MoveTo" } else { "LineTo" })), ..Row::default() };
        for (name, v) in [("X", k), ("Y", k % 2)] {
            row.cells.insert(diacraft_doc::CellName::new(name), diacraft_doc::Cell::constant(DocValue::from_text(&v.to_string())));
        }
        section.rows.push(Arc::new(row));
    }
    let section = Arc::new(section);
    let list = &mut Arc::make_mut(before.pages.get_mut(0).unwrap()).shapes;
    for at in 0..list.len() {
        let shape = list.get_mut(at).map(Arc::make_mut).unwrap();
        Arc::make_mut(&mut shape.sheet).sections.push(Arc::clone(&section));
    }
    let mut after = before.clone();
    for at in 0..n {
        let sheet = SheetRef::Shape(ShapeRef { owner: Owner::Page(1), path: vec![at] });
        after.write_cell(&sheet, &CellAddr::direct("PinX"), CellEdit::Value(DocValue::from_text("5")), WriteOrigin::Drawing).unwrap();
    }
    (before, after)
}

/// A section the copies of a shape share is walked **once for a commit**, however many copies hold it: by the index of the page when it is made, by the
/// comparison of the versions of a sheet when it is put right (a section behind the same pointer in both is not walked at all), and by the cells of a sheet that read its
/// own cells. The commits are the commits with no index, cell for cell (ten thousand connectors sharing a route of 4,000 rows were 160 million cells for a move).
#[test]
fn a_section_the_copies_of_a_shape_share_is_walked_once_for_a_commit_and_the_commits_are_the_same() {
    let (n, rows) = (300_u32, 2_000_u32);
    let (before, after) = shapes_sharing_a_section(n, rows);
    let changes = changes_between(&before, &after, 1);
    let mut index = ReaderIndex::new();
    // The first commit makes the index; the second, of the same change on the next version, puts it right from the sheets that were replaced.
    let mut first = after.clone();
    crate::work::reset();
    let made = commit_changed_with(&mut index, &mut first, &Budget::default(), EvalOptions::default(), 1, &changes);
    let walked_making = crate::work::cells_walked();
    let mut again = first.clone();
    let next = written_pins(&again, n, "6");
    let changes_next = changes_between(&again, &next, 1);
    again = next;
    crate::work::reset();
    let put_right = commit_changed_with(&mut index, &mut again, &Budget::default(), EvalOptions::default(), 1, &changes_next);
    let walked_putting_right = crate::work::cells_walked();
    // Each sheet's own cells, a few times, and the shared section's rows of two cells once or twice for the commit: not the section for each of the 300.
    let most = u64::from(n) * 16 + 4 * u64::from(rows) * 2;
    assert!(walked_making <= most, "making the index walked {walked_making} cells, at most {most}");
    assert!(walked_putting_right <= most, "putting it right walked {walked_putting_right} cells, at most {most}");
    // The same commits as with no index.
    let mut plain = after.clone();
    let expected = commit_changed(&mut plain, &Budget::default(), EvalOptions::default(), 1, &changes);
    assert_eq!(first, plain);
    assert_eq!((made.written, made.unchanged, made.skipped), (expected.written, expected.unchanged, expected.skipped));
    assert!(made.written >= u64::from(n), "{made:?}");
    let mut plain_next = first.clone();
    let next = written_pins(&plain_next, n, "6");
    let changes_plain = changes_between(&plain_next, &next, 1);
    plain_next = next;
    let expected_next = commit_changed(&mut plain_next, &Budget::default(), EvalOptions::default(), 1, &changes_plain);
    assert_eq!(again, plain_next);
    assert_eq!((put_right.written, put_right.unchanged), (expected_next.written, expected_next.unchanged));
}

/// `doc` with the `PinX` of its first `n` top-level shapes written as `value`.
fn written_pins(doc: &Document, n: u32, value: &str) -> Document {
    let mut out = doc.clone();
    for at in 0..n {
        let sheet = SheetRef::Shape(ShapeRef { owner: Owner::Page(1), path: vec![at] });
        out.write_cell(&sheet, &CellAddr::direct("PinX"), CellEdit::Value(DocValue::from_text(value)), WriteOrigin::Drawing).unwrap();
    }
    out
}
