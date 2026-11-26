//! Following a renumber (as the window's spec R29 amends it): when rows are numbered again, **every formula that names one of them names the
//! same row**, and a formula that named a row that went reads `#REF!`.
//!
//! A reference names a row by its number (`Scratch.A3`, `Geometry1.X4`, `Connections.X2`, `Char.Size[2]`, `Sheet.20!Connections.X3`), so a row that moves under its number would leave
//! every such reference reading another row. [`follow`] finds them and writes the new number into the text:
//!
//! * **Which formulas.** Every local formula of the sheets a reference to the section can be on: the page the sheet is on, the document's sheet, and, for the document's own
//!   rows, the styles and masters too (`TheDoc!`); the formulas of another page only when they hold a `Pages[..]` qualifier, the one way a formula reaches another page's
//!   shapes. A formula is looked at only when its text holds the section's name (a substring test, no parse), so a drawing of 30,000 formulas that do not name the section
//!   costs a pass over their text.
//! * **Which references.** Each reference of such a formula is read **by the evaluator's own reading** ([`diacraft_calc::Locator`]: `Sheet.<id>!`, `ThePage!`, `TheDoc!`, a peer's
//!   name, a row's name in any letter case), so a reference is followed exactly when the evaluator would have read the row it names. A reference with a call before it
//!   (`LISTSHEETREF()!Width`) and a formula that builds a reference as text (`EVALTEXT`) are chosen when the formula runs and are not followed; a reference that names a row by a name
//!   (`Prop.Cost`) is not renumbered, since a name does not move.
//! * **What is written.** The bytes of the number and nothing else (`diacraft_formula::reference_spans` says where they are), or `REF()` for the whole reference of a row that
//!   went: the spacing, the strings and every other reference of a formula are kept as they were (a formula is kept verbatim). The formula is parsed again within the
//!   parser's limits and the call's budget, and a text that does not parse is an error and changes nothing.
//! * **`Connect` rows** of the page (`ToPart` 100 plus the point's `IX`, `ToCell` `Connections.X3`) follow a connection point that moves, and one that went is dropped.
//!
//! **Costs, as counts** ([`Work`]): the formulas looked at, the formulas read for their references, and the references located; the locator charges the evaluator's budget, and a
//! page too large to follow within it is an error and changes nothing. At most [`MAX_FOLLOWED_FORMULAS`] formulas are written and [`MAX_FOLLOWED_NODES`] parts of formulas parsed
//! again in one call.

use std::collections::{BTreeSet, HashMap};
use std::ops::Range;
use std::sync::Arc;

use diacraft_calc::{Budget, EvalCtx, EvalOptions, Locator, cell_address, reference_name};
use diacraft_doc::{
    Cell, CellAddr, Connect, Connects, Document, FormulaSlot, MAX_SHAPE_DEPTH, MAX_SHAPES_PER_PAGE, Owner, RowKey, Section, Shape, ShapeList,
    ShapeRef, Sheet, SheetRef, ix_to_row_number, section_spelling,
};
use diacraft_formula::{MAX_NODES, RefSpan, SegmentSpan, parse_with_budget, reference_spans};

use super::Problem;
use super::sheet_rows::sheet_mut;

/// The most formulas one call writes again.
pub const MAX_FOLLOWED_FORMULAS: usize = 200_000;

/// The most parts of formulas one call parses again: a tree costs 72 to 128 bytes a part, so this holds the trees of a call to about 130 MB.
pub const MAX_FOLLOWED_NODES: usize = 1 << 20;

/// The `ToPart` of a `Connect` row at a connection point is this plus the point's `IX`.
const TO_PART_BASE: u32 = 100;

/// What becomes of a row or a section that a call numbers again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Fate {
    /// It is taken out: a reference to it reads `#REF!`.
    Goes,
    /// It takes this `IX`.
    To(u32),
}

/// What a call numbers again, in one section of one sheet.
#[derive(Clone, Debug)]
pub(super) struct Remap {
    /// The sheet that holds the section.
    pub sheet: SheetRef,
    /// The section's name in a file (`Geometry`, `Scratch`, `Connection`).
    pub name: Arc<str>,
    /// The sections of that name that go or take another `IX`, by `IX` (`None` for the one section of any name but Geometry).
    pub sections: HashMap<Option<u32>, Fate>,
    /// The section whose rows are numbered again, and what becomes of each row by its `IX`.
    pub rows_of: Option<Option<u32>>,
    pub rows: HashMap<u32, Fate>,
}

impl Remap {
    pub(super) fn new(sheet: SheetRef, name: &str) -> Remap {
        Remap { sheet, name: Arc::from(name), sections: HashMap::new(), rows_of: None, rows: HashMap::new() }
    }

    /// Whether nothing is numbered again.
    pub(super) fn is_empty(&self) -> bool {
        self.sections.is_empty() && self.rows.is_empty()
    }

    /// How a reference to the cell `addr` of `sheet`, spelled as `span` says in `source`, is written now: `None` for a reference that is as it was.
    fn edit(&self, sheet: &SheetRef, addr: &CellAddr, span: &RefSpan, source: &str) -> Option<(Range<usize>, String)> {
        if *sheet != self.sheet {
            return None;
        }
        let key = addr.section.as_ref()?;
        if key.name.as_str() != self.name.as_ref() {
            return None;
        }
        let geometry = self.name.as_ref() == "Geometry";
        match self.sections.get(&key.ix) {
            Some(Fate::Goes) => return Some((span.start..span.end, "REF()".to_owned())),
            Some(Fate::To(ix)) => return renumbered(source, span.segments.first()?, ix.checked_add(1)?, false),
            None => {}
        }
        if self.rows_of == Some(key.ix)
            && let Some(RowKey::Ix(row)) = addr.row
        {
            return match self.rows.get(&row)? {
                Fate::Goes => Some((span.start..span.end, "REF()".to_owned())),
                Fate::To(ix) => {
                    // The number a reference spells for the new `IX`: a Geometry row's is the `IX`, any other's one more.
                    let number = ix_to_row_number(geometry, *ix)?;
                    renumbered(source, span.segments.get(1)?, number, true)
                }
            };
        }
        None
    }
}

/// The edit that puts `number` where a segment of a path holds one: the digits in its `[index]`, the digits that end its name (`X4`, `Geometry3`), or, for a row's name with
/// none (`Scratch.A` is row 1), the number added after the name. `row` is false for a section's name, which always ends in its number.
fn renumbered(source: &str, segment: &SegmentSpan, number: u32, row: bool) -> Option<(Range<usize>, String)> {
    if let Some(digits) = &segment.index {
        return Some((digits.clone(), number.to_string()));
    }
    let name = source.get(segment.name.clone())?;
    let end = if name.len() >= 2 && name.starts_with('\'') && name.ends_with('\'') { segment.name.end.checked_sub(1)? } else { segment.name.end };
    let inner = source.get(segment.name.start..end)?;
    let digits = inner.bytes().rev().take_while(u8::is_ascii_digit).count();
    if digits == 0 && !row {
        return None;
    }
    Some((end.checked_sub(digits)?..end, number.to_string()))
}

/// What a call's following did, as counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Work {
    /// The formulas looked at: every local formula of the sheets scanned.
    pub visited: u64,
    /// The formulas whose text holds the section's name, read for where their references are.
    pub parsed: u64,
    /// The references located.
    pub located: u64,
}

/// What [`follow`] changed.
#[derive(Clone, Debug, Default)]
pub(super) struct Followed {
    /// The formulas written again.
    pub formulas: usize,
    /// The `Connect` rows that follow a point, or are dropped with it.
    pub connects: usize,
    /// The pages a formula was written on.
    pub pages: BTreeSet<u32>,
    pub work: Work,
}

/// Whether `text` holds `needle`, whatever the letter case of either's ASCII letters.
fn holds_ignoring_case(text: &str, needle: &str) -> bool {
    let (text, needle) = (text.as_bytes(), needle.as_bytes());
    needle.is_empty() || text.windows(needle.len()).any(|window| window.eq_ignore_ascii_case(needle))
}

/// The formula cells of one sheet, each with where it is: `visit(section, row key, cell name, cell)`.
fn each_formula<'a>(sheet: &'a Sheet, mut visit: impl FnMut(Option<&'a Section>, Option<RowKey>, &'a str, &'a Cell)) {
    for (name, cell) in &sheet.cells {
        if matches!(cell.formula, FormulaSlot::Local(_)) {
            visit(None, None, name.as_str(), cell);
        }
    }
    for held in &sheet.sections {
        let section: &Section = held;
        if section.del {
            continue;
        }
        for (name, cell) in &section.cells {
            if matches!(cell.formula, FormulaSlot::Local(_)) {
                visit(Some(section), None, name.as_str(), cell);
            }
        }
        for row in &section.rows {
            if row.del || !row.cells.values().any(|c| matches!(c.formula, FormulaSlot::Local(_))) {
                continue;
            }
            let Some(key) = row.key(section.name.as_str()) else { continue };
            for (name, cell) in &row.cells {
                if matches!(cell.formula, FormulaSlot::Local(_)) {
                    visit(Some(section), Some(key.clone()), name.as_str(), cell);
                }
            }
        }
    }
}

/// The shapes of a part, each with its path, outermost first, to the depth the model holds: a group before its members.
fn each_shape<'a>(top: &'a ShapeList, mut visit: impl FnMut(&[u32], &'a Shape)) {
    let mut stack: Vec<(&'a ShapeList, usize)> = vec![(top, 0)];
    let mut path: Vec<u32> = Vec::new();
    let mut walked = 0_usize;
    while let Some(frame) = stack.last_mut() {
        let (list, at) = *frame;
        let Some(shape) = list.get(at) else {
            stack.pop();
            continue;
        };
        frame.1 = at.saturating_add(1);
        walked = walked.saturating_add(1);
        if walked > MAX_SHAPES_PER_PAGE {
            return;
        }
        path.truncate(stack.len().saturating_sub(1));
        path.push(u32::try_from(at).unwrap_or(u32::MAX));
        if !shape.del {
            visit(&path, shape);
        }
        if stack.len() < MAX_SHAPE_DEPTH && !shape.children.is_empty() {
            stack.push((&shape.children, 0));
        }
    }
}

/// What the scan of a document keeps.
struct Scan<'a, 'c> {
    remap: &'a Remap,
    /// A formula is read only when its text holds one of these: the section's name as a reference spells it and as a file names it.
    needles: Vec<&'a str>,
    locator: Locator<'a, 'c>,
    edits: Vec<(SheetRef, CellAddr, Cell)>,
    pages: BTreeSet<u32>,
    work: Work,
    nodes: usize,
}

impl<'a> Scan<'a, '_> {
    /// Looks at every local formula of `sheet`, which `at` names. `pages_only`: only a formula that holds a `Pages[..]` qualifier is read.
    fn sheet(&mut self, sheet: &Sheet, at: &dyn Fn() -> SheetRef, pages_only: bool) -> Result<(), Problem> {
        let mut found: Vec<(Option<&Section>, Option<RowKey>, &str, &Cell)> = Vec::new();
        let needles = &self.needles;
        let mut seen = 0_u64;
        each_formula(sheet, |section, row, name, cell| {
            seen += 1;
            let FormulaSlot::Local(formula) = &cell.formula else { return };
            let source: &str = &formula.source;
            if formula.is_parsed() && needles.iter().any(|needle| source.contains(needle)) && (!pages_only || holds_ignoring_case(source, "pages[")) {
                found.push((section, row, name, cell));
            }
        });
        self.work.visited = self.work.visited.saturating_add(seen);
        for (section, row, name, cell) in found {
            let addr = match (section, row) {
                (None, _) => CellAddr::direct(name),
                (Some(section), None) => CellAddr::in_section(section.name.as_str(), section.ix, name),
                (Some(section), Some(row)) => CellAddr::in_row(section.name.as_str(), section.ix, row, name),
            };
            self.formula(at(), addr, cell)?;
        }
        Ok(())
    }

    /// Reads one formula, and keeps the cell it becomes when a reference of it is written again.
    fn formula(&mut self, sheet: SheetRef, addr: CellAddr, cell: &Cell) -> Result<(), Problem> {
        let FormulaSlot::Local(formula) = &cell.formula else { return Ok(()) };
        let Ok(spans) = reference_spans(&formula.source) else { return Ok(()) };
        self.work.parsed = self.work.parsed.saturating_add(1);
        let mut edits: Vec<(Range<usize>, String)> = Vec::new();
        for span in &spans {
            let located = self.locator.locate(&sheet, &span.reference);
            self.work.located = self.work.located.saturating_add(1);
            match located {
                Some((target, found)) => edits.extend(self.remap.edit(&target, &found, span, &formula.source)),
                None if self.locator.exhausted() => return Err(too_large()),
                None => {}
            }
        }
        if edits.is_empty() {
            return Ok(());
        }
        edits.sort_by_key(|(range, _)| (range.start, range.end));
        let mut text = String::with_capacity(formula.source.len() + 8);
        let mut copied = 0_usize;
        for (range, new) in &edits {
            if range.start < copied {
                // Two edits over the same bytes: a formula this reading does not understand. It is left as it is, and said.
                return Err(Problem::failed("a formula names a row in a way that cannot be followed: the rows were not numbered again"));
            }
            text.push_str(formula.source.get(copied..range.start).ok_or_else(unwritable)?);
            text.push_str(new);
            copied = range.end;
        }
        text.push_str(formula.source.get(copied..).ok_or_else(unwritable)?);
        let written = parse_with_budget(&text, MAX_NODES);
        if !written.is_parsed() {
            return Err(unwritable());
        }
        self.nodes = self.nodes.saturating_add(written.node_count());
        if self.nodes > MAX_FOLLOWED_NODES || self.edits.len() >= MAX_FOLLOWED_FORMULAS {
            return Err(Problem::failed(format!(
                "numbering the rows again would write more than {MAX_FOLLOWED_FORMULAS} formulas or {MAX_FOLLOWED_NODES} parts of formulas: nothing was changed"
            )));
        }
        let mut cell = cell.clone();
        cell.formula = FormulaSlot::Local(Arc::new(written));
        if let SheetRef::Shape(ShapeRef { owner: Owner::Page(page), .. }) | SheetRef::Page(page) = &sheet {
            self.pages.insert(*page);
        }
        self.edits.push((sheet, addr, cell));
        Ok(())
    }
}

fn unwritable() -> Problem {
    Problem::failed("a formula that names a row could not be written again with its new number: nothing was changed")
}

fn too_large() -> Problem {
    Problem::failed("the drawing is too large to find the formulas that name the rows within the budget of the evaluator: nothing was changed")
}

impl Scan<'_, '_> {
    /// The shapes of a part, each with the sheet it holds.
    fn shapes(&mut self, top: &ShapeList, owner: Owner, pages_only: bool) -> Result<(), Problem> {
        let mut failed: Option<Problem> = None;
        each_shape(top, |path, shape| {
            if failed.is_none()
                && let Err(e) = self.sheet(&shape.sheet, &|| SheetRef::Shape(ShapeRef { owner, path: path.to_vec() }), pages_only)
            {
                failed = Some(e);
            }
        });
        failed.map_or(Ok(()), Err)
    }

    /// Every sheet a reference to the section can be on: the pages (the page of the section whole, the others for `Pages[..]` only), the document's sheet, and for the
    /// document's own rows the styles and the masters.
    fn document(&mut self, reading: &Document) -> Result<(), Problem> {
        let (page, document) = match &self.remap.sheet {
            SheetRef::Shape(ShapeRef { owner: Owner::Page(page), .. }) | SheetRef::Page(page) => (Some(*page), false),
            other => (None, matches!(other, SheetRef::Document)),
        };
        for p in reading.pages.iter() {
            let id = p.id;
            let pages_only = !document && Some(id) != page;
            self.sheet(&p.sheet, &|| SheetRef::Page(id), pages_only)?;
            self.shapes(&p.shapes, Owner::Page(id), pages_only)?;
        }
        self.sheet(&reading.sheet, &|| SheetRef::Document, false)?;
        if document {
            for style in reading.styles.iter() {
                let id = style.id;
                self.sheet(&style.sheet, &|| SheetRef::Style(id), false)?;
            }
            for master in reading.masters.iter() {
                let id = master.id;
                self.sheet(&master.sheet, &|| SheetRef::Master(id), false)?;
                self.shapes(&master.shapes, Owner::Master(id), false)?;
            }
        }
        Ok(())
    }
}

/// Writes every formula that names a row of `remap` as it is named now, and puts the `Connect` rows right, on `doc`. The rows themselves are not touched here: the caller
/// numbers them again after, so that a formula that is in a row that moves is found where it is.
pub(super) fn follow(doc: &mut Document, remap: &Remap) -> Result<Followed, Problem> {
    if remap.is_empty() {
        return Ok(Followed::default());
    }
    let mut followed = Followed::default();
    let edits = {
        let reading: &Document = doc;
        let mut ctx = EvalCtx::new(EvalOptions::default(), Budget::default());
        let mut needles: Vec<&str> = vec![remap.name.as_ref()];
        needles.extend(section_spelling(remap.name.as_ref()));
        let mut scan = Scan {
            remap,
            needles,
            locator: Locator::new(reading, &mut ctx),
            edits: Vec::new(),
            pages: BTreeSet::new(),
            work: Work::default(),
            nodes: 0,
        };
        scan.document(reading)?;
        followed.work = scan.work;
        followed.pages = std::mem::take(&mut scan.pages);
        std::mem::take(&mut scan.edits)
    };
    followed.formulas = edits.len();
    write_formulas(doc, edits)?;
    followed.connects = follow_connects(doc, remap)?;
    Ok(followed)
}

/// Puts the cells in the sheets they are of, a sheet at a time.
fn write_formulas(doc: &mut Document, edits: Vec<(SheetRef, CellAddr, Cell)>) -> Result<(), Problem> {
    let mut by_sheet: HashMap<SheetRef, Vec<(CellAddr, Cell)>> = HashMap::new();
    for (sheet, addr, cell) in edits {
        by_sheet.entry(sheet).or_default().push((addr, cell));
    }
    for (sheet, cells) in by_sheet {
        let target = sheet_mut(doc, &sheet).ok_or_else(|| Problem::failed("a sheet that holds a formula to write again is not there"))?;
        for (addr, cell) in cells {
            target.set_cell(&addr, cell).map_err(|e| Problem::failed(e.to_string()))?;
        }
    }
    Ok(())
}

/// The `Connect` rows of the page of a shape whose connection points are numbered again: a row glued to a point that moves follows it (`ToPart` and `ToCell`), and one glued to
/// a point that went, or to a section that went, is dropped (the end that was glued reads `#REF!`). → how many rows changed.
fn follow_connects(doc: &mut Document, remap: &Remap) -> Result<usize, Problem> {
    let SheetRef::Shape(at) = &remap.sheet else { return Ok(0) };
    let Owner::Page(page) = at.owner else { return Ok(0) };
    if remap.name.as_ref() != "Connection" {
        return Ok(0);
    }
    let Some(id) = doc.shape(at).map(|shape| shape.id) else { return Ok(0) };
    let Some(held) = doc.page(page) else { return Ok(0) };
    let whole = matches!(remap.sections.get(&None), Some(Fate::Goes));
    let fate_of = |ix: u32| -> Option<Fate> {
        if whole {
            return Some(Fate::Goes);
        }
        if remap.rows_of == Some(None) { remap.rows.get(&ix).copied() } else { None }
    };
    let mut changed = 0_usize;
    let mut list: Vec<Connect> = Vec::with_capacity(held.connects.len());
    for connect in held.connects.iter() {
        let mut connect = connect.clone();
        if connect.to_sheet != id {
            list.push(connect);
            continue;
        }
        // The point it is glued to: by its `ToPart` (100 plus the `IX`), else by the reference in its `ToCell`.
        let by_cell = connect
            .to_cell
            .as_deref()
            .and_then(|text| cell_address(text).ok())
            .filter(|a| a.section.as_ref().is_some_and(|k| k.name.as_str() == "Connection"));
        let old_ix = match (connect.to_part, &by_cell) {
            (Some(part), _) if part >= TO_PART_BASE => Some(part - TO_PART_BASE),
            (_, Some(addr)) => match &addr.row {
                Some(RowKey::Ix(ix)) => Some(*ix),
                _ => None,
            },
            _ => None,
        };
        match old_ix.and_then(&fate_of) {
            None => list.push(connect),
            Some(Fate::Goes) => changed += 1,
            Some(Fate::To(new)) => {
                connect.to_part = connect.to_part.map(|_| TO_PART_BASE.saturating_add(new));
                if let Some(mut addr) = by_cell {
                    addr.row = Some(RowKey::Ix(new));
                    if let Some(text) = reference_name(&addr) {
                        connect.to_cell = Some(Arc::from(text));
                    }
                }
                changed += 1;
                list.push(connect);
            }
        }
    }
    if changed > 0 {
        let page = super::page_mut(doc, page).ok_or_else(|| Problem::failed("the page went away"))?;
        page.connects = Connects::from_vec(list);
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn span(source: &str, at: usize) -> RefSpan {
        reference_spans(source).unwrap().remove(at)
    }

    fn remap_rows(name: &str, geometry: bool, rows: &[(u32, Fate)]) -> Remap {
        let mut remap = Remap::new(SheetRef::Document, name);
        remap.rows_of = Some(geometry.then_some(0));
        remap.rows = rows.iter().copied().collect();
        remap
    }

    fn apply(source: &str, edit: (Range<usize>, String)) -> String {
        format!("{}{}{}", &source[..edit.0.start], edit.1, &source[edit.0.end..])
    }

    #[test]
    fn a_row_number_is_written_where_the_reference_spells_it_whatever_the_spelling() {
        let remap = remap_rows("Scratch", false, &[(2, Fate::To(3)), (0, Fate::To(1))]);
        let at = |source: &str, row: u32| {
            let addr = CellAddr::in_row("Scratch", None, RowKey::Ix(row), "A");
            remap.edit(&SheetRef::Document, &addr, &span(source, 0), source).map(|e| apply(source, e))
        };
        // The digits that end a name, the digits of an index, the number a name with none leaves out (row 1), and a quoted name.
        assert_eq!(at("Scratch.A3", 2), Some("Scratch.A4".to_owned()));
        assert_eq!(at("Scratch.A[3]", 2), Some("Scratch.A[4]".to_owned()));
        assert_eq!(at("Scratch.A", 0), Some("Scratch.A2".to_owned()));
        assert_eq!(at("Scratch.'A3'", 2), Some("Scratch.'A4'".to_owned()));
        // A row that stays is not touched, and nor is another section's.
        assert_eq!(at("Scratch.A9", 8), None);
        let other = CellAddr::in_row("User", None, RowKey::Ix(2), "A");
        assert_eq!(remap.edit(&SheetRef::Document, &other, &span("User.A3", 0), "User.A3"), None);
        assert_eq!(
            remap.edit(&SheetRef::Style(1), &CellAddr::in_row("Scratch", None, RowKey::Ix(2), "A"), &span("Scratch.A3", 0), "Scratch.A3"),
            None
        );
    }

    #[test]
    fn a_geometry_row_is_numbered_by_its_ix_and_a_section_by_its_ix_plus_one() {
        let mut remap = remap_rows("Geometry", true, &[(4, Fate::To(5)), (3, Fate::Goes)]);
        remap.sections.insert(Some(2), Fate::To(1));
        remap.rows_of = Some(Some(0));
        let row = |source: &str, section: u32, row: u32| {
            let addr = CellAddr::in_row("Geometry", Some(section), RowKey::Ix(row), "X");
            remap.edit(&SheetRef::Document, &addr, &span(source, 0), source).map(|e| apply(source, e))
        };
        assert_eq!(row("Geometry1.X4", 0, 4), Some("Geometry1.X5".to_owned()));
        assert_eq!(row("Sheet.2!Geometry1.X3", 0, 3), Some("REF()".to_owned()), "the qualifier goes with the reference that is no more");
        // A row of another section of the same name is not the one that moves; a section that takes another number is written with it.
        assert_eq!(row("Geometry2.X4", 1, 4), None);
        assert_eq!(row("Geometry3.X2", 2, 2), Some("Geometry2.X2".to_owned()));
    }

    #[test]
    fn a_text_that_holds_a_name_in_any_letter_case_is_found_without_a_parse() {
        assert!(holds_ignoring_case("1+Pages[Site]!A", "pages["));
        assert!(holds_ignoring_case("PAGES[x]", "pages["));
        assert!(!holds_ignoring_case("Page[1]", "pages["));
        assert!(holds_ignoring_case("anything", ""));
    }
}
