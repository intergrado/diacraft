//! Setting cells of a ShapeSheet: what `sheet.set_cell` and `sheet.set_cells` do to a document, on the rules of the design of the ShapeSheet window (section 7.5) and
//! of the window's spec.
//!
//! * The text is cut of white space at its ends and of one leading `=`. **An empty text clears** the cell's own formula: the cell
//!   goes back to inheriting what it can inherit, with no value of its own (its old value was the result of the formula that went), or reads `No Formula` with the last value
//!   where nothing is inherited; a cell the sheet does not hold of its own is not changed. Any other text **must parse** (a fault is refused with its kind, its byte offset and
//!   words) and is written as typed, the recalculation working it out; a **number literal alone** is a value with no formula, and **text typed into a cell that
//!   expects text** with no leading `=` is a string.
//! * A bare number is read in the unit the cell shows (`3` in a cell showing `50 mm` is 3 mm), stored in the internal unit with the cell's `U`; a number
//!   typed with a unit keeps it for display; a cell that shows no unit and needs one takes the default unit of its kind (a length in inches, an angle in degrees), and a cell that
//!   shows none takes the number as it is. A literal whose unit is not of the cell's measure is a formula like any other, and calc fits it.
//! * A write goes to a cell the sheet holds or inherits, or that the catalogue lists at that place for that kind of sheet; any other is refused in words.
//! * A formula is at most [`MAX_FORMULA_BYTES`] bytes, a call at most [`MAX_WRITES`] writes, [`MAX_CALL_BYTES`] bytes of text and [`MAX_CALL_NODES`] parts of
//!   formulas in all: counts, held whole before anything is written.
//! * `force` writes past `GUARD` (`WriteOrigin::AutomationForce`); without it a guarded cell is refused in words (`WriteOrigin::Automation`).
//! * **The recalculation, by kind of sheet** (design 7.5, step 7): a shape of a page or a page's own sheet is [`settle`]d on its page with the shape as touched, so that what reads
//!   the cell is worked out and the connectors it moved are routed again; a master's cell on every page that holds an instance of the master, and the document's or a style's
//!   cell on every page, each as a whole page (`Changes::whole_page`, as `theme.apply` does), each within the evaluator's budget and **the call refused whole when one page passes it**.
//! * **One step, all or nothing.** The caller edits a copy of the document: an error leaves the drawing as it was, and a call that changed no cell is no step.
//!
//! **Costs.** The shapes a call names are found in one walk of each page that holds them ([`find_named`]), not a search for each write; the writes are
//! one [`Document::write_batch`]; the recalculation is one [`settle`] for each page touched, at the end.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use diacraft_calc::{Budget, reference_name};
use diacraft_doc::{
    Cell, CellAddr, CellClass, CellEdit, CellWrite, Document, FormulaSlot, Lookup, MAX_SHAPE_DEPTH, MAX_SHAPES_PER_PAGE, Owner, Resolved, Resolver,
    RowKey, SectionKey, ShapeList, ShapeRef, SheetKind, SheetRef, Value as Held, ValueKind, WriteOrigin, WriteOutcome, cell_info,
};
use diacraft_formula::{Expr, Formula, MAX_NODES, Number, ParseErrorKind, UnaryOp, parse_with_budget};
use diacraft_units::{Unit, parse_unit};

use super::scan::find_named;
use super::{Problem, settle};
use crate::sheetview::{self, CellView, Counts, Ctx, Place, Target, cut_text, fault_of};

/// The most bytes of one formula (no limit is published, and a file's formula may be up to 1 MiB and is kept).
pub const MAX_FORMULA_BYTES: usize = 65_536;

/// The most writes in one call.
pub const MAX_WRITES: usize = 4_096;

/// The most bytes of formula text in one call.
pub const MAX_CALL_BYTES: usize = 4 * 1024 * 1024;

/// The most parts (nodes of the trees of the formulas) in one call: a tree costs 72 to 128 bytes a node, so this holds a call's trees to about 32 MB.
pub const MAX_CALL_NODES: usize = 1 << 18;

/// The most cells (and results) an answer lists.
pub const MAX_LISTED: usize = 256;

/// The most shapes looked at to find the pages that hold an instance of a master; past it every page is worked out.
const MAX_INSTANCE_WALK: usize = 4_000_000;

/// One write asked for.
#[derive(Clone, Debug)]
pub struct Write {
    pub target: Target,
    /// The cell as a reference names it (`cell_address`), not yet spelled as the sheet spells a row's name.
    pub addr: CellAddr,
    pub text: String,
    pub force: bool,
}

/// What a write came to.
#[derive(Clone, Debug)]
pub struct WriteResult {
    pub place: Place,
    /// The cell as the sheet shows it after the call, with no text cut.
    pub cell: CellView,
    /// Whether this write changed the cell, judged against the cells the writes before it left.
    pub changed: bool,
}

/// A cell the recalculation changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Touched {
    pub place: Place,
    /// The reference a formula names it by; `None` where none can be spelled.
    pub cell: Option<String>,
}

/// The cells the call's recalculation and routing changed, other than the cells it wrote.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recalculated {
    /// How many, counted over the pages the call worked out.
    pub cells: usize,
    /// The first [`MAX_LISTED`] of them.
    pub listed: Vec<Touched>,
    /// There are more than the list holds, or the count could not be made whole.
    pub more: bool,
}

/// What a call did.
#[derive(Clone, Debug)]
pub struct Done {
    /// The drawing changed: some cell is not as it was. A call that did not leaves the document as it was and is no undo step.
    pub changed: bool,
    pub written: usize,
    pub unchanged: usize,
    /// The first [`MAX_LISTED`] writes' results, in the order they were asked.
    pub results: Vec<WriteResult>,
    pub recalculated: Recalculated,
}

// ---- the text of a write ---------------------------------------------------------------------------------------------------------------

/// A number `number` times `unit`'s factor in internal units, when that fits a number: `None` for a result that is not finite.
pub(crate) fn literal_in_unit(number: f64, unit: Unit) -> Option<f64> {
    let stored = unit.to_internal(number);
    stored.is_finite().then_some(stored)
}

/// What the text of a write is, once cut.
enum Typed {
    /// No text: the cell's own formula goes.
    Clear,
    /// A string typed into a cell that expects text.
    Text(String),
    /// A formula, as typed.
    Formula(Arc<Formula>),
    /// A number literal alone: its number, signed, the unit typed with it (`Unit::Unitless` for a bare number), and the formula it is, for a cell it is not for.
    Literal { number: f64, unit: Unit, formula: Arc<Formula> },
}

/// The number a literal formula is, with its unit: `3`, `3 in`, `-0.25 mm`, `25%`. Any other tree is `None`.
fn literal_of(expr: &Expr) -> Option<(f64, Unit)> {
    let plain = |e: &Expr| match e {
        Expr::Number(Number::Measured { value, unit, power: None }) => Some((*value, *unit)),
        _ => None,
    };
    match expr {
        Expr::Unary(op, inner) => {
            let (value, unit) = plain(inner)?;
            Some((if *op == UnaryOp::Minus { -value } else { value }, unit))
        }
        Expr::Percent(inner) => match plain(inner)? {
            (value, Unit::Unitless) => Some((value, Unit::Percent)),
            _ => None,
        },
        other => plain(other),
    }
}

/// How a parse that failed is said, for the text as it was given. `nodes_left` is what the call's budget of parts had left when the parse began.
fn fault_words(error: &diacraft_formula::ParseError, given: &str, front: usize, nodes_left: usize) -> String {
    if error.kind == ParseErrorKind::TooManyNodes && nodes_left < MAX_NODES {
        return format!("the formulas of one call have too many parts: at most {MAX_CALL_NODES} in all");
    }
    let fault = fault_of(error, given, front);
    format!("the formula has an error: {} ({}, byte {})", fault.words, fault.kind, fault.offset)
}

/// The text `given` as a write reads it for a cell of `kind`, with what is left of the call's budget of parts taken off by the tree it makes.
fn read_text(given: &str, kind: ValueKind, nodes_left: &mut usize) -> Result<Typed, Problem> {
    let (body, front) = cut_text(given);
    let inner = body.trim_start();
    let front = front.saturating_add(body.len().saturating_sub(inner.len()));
    if inner.is_empty() {
        return Ok(Typed::Clear);
    }
    let had_equals = given.trim().starts_with('=');
    let parse = |text: &str, nodes_left: &mut usize| -> Result<Arc<Formula>, Problem> {
        let formula = parse_with_budget(text, *nodes_left);
        match &formula.ast {
            Ok(_) => {
                *nodes_left = nodes_left.saturating_sub(formula.node_count());
                Ok(Arc::new(formula))
            }
            Err(e) => Err(Problem::bad(fault_words(e, given, front, *nodes_left))),
        }
    };
    if kind == ValueKind::Text && !had_equals {
        // A cell that expects text takes what is typed as a string. A text that is a string literal already is the formula as it stands: what a cell shows is what can be typed
        // back, and a formula copied from one cell into another is not quoted twice.
        let literal = parse_with_budget(inner, MAX_NODES);
        if matches!(literal.ast, Ok(Expr::Str(_))) {
            return parse(inner, nodes_left).map(Typed::Formula);
        }
        return Ok(Typed::Text(inner.to_owned()));
    }
    let formula = parse(inner, nodes_left)?;
    if let Ok(expr) = &formula.ast
        && let Some((number, unit)) = literal_of(expr)
    {
        return Ok(Typed::Literal { number, unit, formula });
    }
    Ok(Typed::Formula(formula))
}

/// A string as a formula spells it: in quotation marks, a quotation mark inside doubled.
fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\"\""))
}

// ---- the cell a write makes -------------------------------------------------------------------------------------------------------------

/// The unit attribute the cell shows its value in: the nearest cell with a value, else the one with the formula (as the sheet's cell view reads it).
fn shown_unit_attr(found: &Resolved<'_>) -> Option<Arc<str>> {
    found.value.cell.and_then(|c| c.unit.clone()).or_else(|| found.formula.cell.and_then(|c| c.unit.clone()))
}

/// The text a cell with no value of its own at all is given as its value, so that a formula that fails has a last good value to keep and its `E` is recorded
/// What the cell reads when found nowhere.
pub(super) fn default_text(kind: ValueKind) -> Option<&'static str> {
    match kind {
        ValueKind::Scalar | ValueKind::Length | ValueKind::Angle | ValueKind::Days | ValueKind::Font => Some("0"),
        ValueKind::Text => Some(""),
        ValueKind::Color => Some("#000000"),
        ValueKind::Point => None,
    }
}

/// The cell a formula makes: the local cell, or a new one, with the formula; a value to stand until the recalculation (the cell's own, else the one it reads now, else the default of
/// its kind); and the unit the cell shows.
fn formula_cell(local: Option<&Cell>, found: &Resolved<'_>, formula: Arc<Formula>, value: Option<Held>) -> Cell {
    let mut cell = local.cloned().unwrap_or_default();
    cell.formula = FormulaSlot::Local(formula);
    cell.error = None;
    if let Some(value) = value {
        cell.value = Some(value);
    } else if cell.value.is_none() {
        cell.value = found.value.cell.and_then(|c| c.value.clone()).or_else(|| default_text(found.default.kind).map(Held::from_text));
    }
    if cell.unit.is_none() {
        cell.unit = shown_unit_attr(found);
    }
    cell
}

/// The cell a formula makes, or `None` when the cell already holds that formula text: the value and the error it holds are the cell's, and no recalculation is asked of a text that is
/// the cell's.
fn same_or_formula(local: Option<&Cell>, found: &Resolved<'_>, formula: Arc<Formula>) -> Option<Cell> {
    if matches!(local.map(|c| &c.formula), Some(FormulaSlot::Local(old)) if old.source == formula.source) {
        return None;
    }
    Some(formula_cell(local, found, formula, None))
}

/// The cell a number literal makes, or `None` when the literal is not for this cell and is a formula like any other.
fn literal_cell(local: Option<&Cell>, found: &Resolved<'_>, number: f64, typed: Unit) -> Option<Cell> {
    let attr = shown_unit_attr(found);
    let shown = attr.as_deref().and_then(parse_unit).filter(|u| *u != Unit::Unitless).or(match found.default.kind {
        ValueKind::Length => Some(Unit::Inch),
        ValueKind::Angle => Some(Unit::Degree),
        _ => None,
    });
    let (unit, written_unit) = if typed == Unit::Unitless {
        (shown.unwrap_or(Unit::Unitless), attr)
    } else {
        // A number with its own unit keeps it, in a cell that shows a unit of the same measure.
        if shown.map(|u| u.measure()) != Some(typed.measure()) {
            return None;
        }
        (typed, Some(Arc::from(typed.code())))
    };
    let number = if number == 0.0 { 0.0 } else { number };
    let stored = literal_in_unit(number, unit)?;
    let mut cell = local.cloned().unwrap_or_default();
    cell.formula = FormulaSlot::Absent;
    cell.value = Some(Held::from_number(stored)?);
    cell.error = None;
    cell.unit = written_unit;
    Some(cell)
}

/// The cell an empty text makes: `None` when the sheet holds no cell of its own there (nothing to clear).
fn clear_cell(local: Option<&Cell>, inheritable: bool) -> Option<Cell> {
    let local = local?;
    let mut cell = local.clone();
    cell.error = None;
    if inheritable {
        cell.formula = FormulaSlot::Inherit;
        cell.value = None;
        cell.unit = None;
    } else {
        cell.formula = FormulaSlot::NoFormula;
    }
    Some(cell)
}

/// The name of a kind of sheet, for words.
fn kind_word(kind: SheetKind) -> &'static str {
    match kind {
        SheetKind::Shape => "shape",
        SheetKind::Page => "page",
        SheetKind::Document => "document",
        SheetKind::Style => "style",
    }
}

/// Whether a section is there once the sheet's master and styles are merged in.
fn section_there(res: &Resolver<'_>, sheet: &SheetRef, key: &SectionKey) -> bool {
    res.section_layers(sheet, key).is_some_and(|layers| !layers.is_empty())
}

/// Whether the row is there: it holds a cell on the sheet or the sheets it inherits from, or is listed among the rows of its section.
fn row_there(res: &Resolver<'_>, sheet: &SheetRef, key: &SectionKey, row: &RowKey) -> bool {
    if res.row_cell_names(sheet, key, row).is_some_and(|names| !names.is_empty()) {
        return true;
    }
    res.effective_row_keys(sheet, key).is_some_and(|rows| rows.iter().any(|r| &r.key == row))
}

/// Whether the catalogue lists the cell at this place for this kind of sheet, and the place is there: a direct cell of the page-sheet class on a page or a master's page sheet, a
/// style flag on a style and the others on any sheet; a cell of a section of a section that is there; a cell of a row that is there.
fn listed_here(res: &Resolver<'_>, kind: SheetKind, sheet: &SheetRef, addr: &CellAddr) -> bool {
    match (&addr.section, &addr.row) {
        (None, _) => match cell_info(None, addr.cell.as_str()).class {
            CellClass::PageSheet => kind == SheetKind::Page,
            CellClass::StyleFlag(_) | CellClass::StyleLocal => kind == SheetKind::Style,
            _ => true,
        },
        (Some(section), None) => section_there(res, sheet, section),
        (Some(section), Some(row)) => row_there(res, sheet, section, row),
    }
}

/// The cell, if the sheet holds it, inherits it, or lists it here; else the words that refuse it.
fn ensure_there<'a>(res: &Resolver<'a>, kind: SheetKind, sheet: &SheetRef, addr: &CellAddr) -> Result<Resolved<'a>, Problem> {
    let label = reference_name(addr).unwrap_or_else(|| addr.cell.as_str().to_owned());
    let Some(found) = res.resolve(sheet, addr) else {
        return Err(Problem::failed(format!("the sheet has no cell {label}: the row or the section it is in is deleted here")));
    };
    if found.formula.cell.is_some() || found.value.cell.is_some() || (found.listed && listed_here(res, kind, sheet, addr)) {
        return Ok(found);
    }
    let words = match (&addr.section, &addr.row) {
        (Some(section), Some(row)) if !row_there(res, sheet, section, row) => {
            if section_there(res, sheet, section) {
                "add the row first (sheet.add_row)".to_owned()
            } else {
                "add the row first (sheet.add_row), after its section (the section is not there either: sheet.add_section)".to_owned()
            }
        }
        (Some(_), Some(_)) => "the catalogue lists no such cell for the section".to_owned(),
        (Some(section), None) if !section_there(res, sheet, section) => "add the section first (sheet.add_section)".to_owned(),
        (Some(_), None) => "the catalogue lists no such cell for the section".to_owned(),
        (None, _) if found.listed => format!("a {} sheet does not hold it", kind_word(kind)),
        (None, _) => "no batch of the catalogue lists it, and the sheet does not hold it".to_owned(),
    };
    Err(Problem::failed(format!("the sheet has no cell {label}: {words}")))
}

// ---- finding the sheets ------------------------------------------------------------------------------------------------------------------

/// A sheet a write names, found.
struct Found {
    place: Place,
    sheet: SheetRef,
}

/// The first shape with each `ID` of `wanted` in the shapes of a master, in one walk (a group before its members): the walk of [`find_named`], for a master's part.
fn find_in_master(doc: &Document, master: u32, wanted: &HashSet<u32>) -> Result<HashMap<u32, ShapeRef>, Problem> {
    let mut found: HashMap<u32, ShapeRef> = HashMap::new();
    let top = &doc.master(master).ok_or_else(|| Problem::bad(format!("the drawing has no master {master}")))?.shapes;
    let mut stack: Vec<(&ShapeList, usize)> = vec![(top, 0)];
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
            return Err(Problem::failed(format!("a master holds at most {MAX_SHAPES_PER_PAGE} shapes")));
        }
        let depth = stack.len();
        path.truncate(depth.saturating_sub(1));
        path.push(u32::try_from(at).unwrap_or(u32::MAX));
        if wanted.contains(&shape.id) && !found.contains_key(&shape.id) {
            found.insert(shape.id, ShapeRef { owner: Owner::Master(master), path: path.clone() });
            if found.len() >= wanted.len() {
                break;
            }
        }
        if depth < MAX_SHAPE_DEPTH && !shape.children.is_empty() {
            stack.push((&shape.children, 0));
        }
    }
    Ok(found)
}

/// Whether the shape at `at` is there (not deleted by `Del`).
fn is_live(doc: &Document, at: &ShapeRef) -> bool {
    doc.shape(at).is_some_and(|shape| !shape.del)
}

/// The sheets the targets name, in the words of [`Target::resolve`] and with the shapes found in one walk of each page (and one of each master) however many there are. The
/// refusal says which target it was.
fn find_sheets(doc: &Document, targets: &[&Target], batch: bool) -> Result<Vec<Found>, Problem> {
    let said = |n: usize, words: String| Problem::bad(if batch { format!("write {}: {words}", n + 1) } else { words });
    let mut out: Vec<Option<Found>> = (0..targets.len()).map(|_| None).collect();
    // The shapes by the page they are named on, by the master, and the ones named by their `ID` alone.
    let mut by_page: BTreeMap<u32, HashSet<u32>> = BTreeMap::new();
    let mut by_master: BTreeMap<u32, HashSet<u32>> = BTreeMap::new();
    let mut loose: HashSet<u32> = HashSet::new();
    for (n, target) in targets.iter().enumerate() {
        match target {
            Target::Shape { id, page: Some(page), master: None } => {
                doc.page(*page).ok_or_else(|| said(n, format!("the drawing has no page {page}")))?;
                by_page.entry(*page).or_default().insert(*id);
            }
            Target::Shape { id, master: Some(master), .. } => {
                doc.master(*master).ok_or_else(|| said(n, format!("the drawing has no master {master}")))?;
                by_master.entry(*master).or_default().insert(*id);
            }
            Target::Shape { id, page: None, master: None } => {
                loose.insert(*id);
            }
            other => {
                let place = other.resolve(doc).map_err(|words| said(n, words))?;
                let sheet = place.sheet(doc).ok_or_else(|| said(n, "the sheet is not in the drawing".to_owned()))?;
                if let Some(slot) = out.get_mut(n) {
                    *slot = Some(Found { place, sheet });
                }
            }
        }
    }
    let mut on_page: HashMap<(u32, u32), ShapeRef> = HashMap::new();
    for (page, ids) in &by_page {
        for (id, at) in find_named(doc, *page, ids)? {
            if is_live(doc, &at) {
                on_page.insert((*page, id), at);
            }
        }
    }
    let mut in_master: HashMap<(u32, u32), ShapeRef> = HashMap::new();
    for (master, ids) in &by_master {
        for (id, at) in find_in_master(doc, *master, ids)? {
            if is_live(doc, &at) {
                in_master.insert((*master, id), at);
            }
        }
    }
    // The first page that holds each shape named by its `ID` alone: the pages in order, each walked once for what is still wanted.
    let mut homes: HashMap<u32, (u32, ShapeRef)> = HashMap::new();
    let mut wanted = loose;
    for page in doc.pages.iter() {
        if wanted.is_empty() {
            break;
        }
        for (id, at) in find_named(doc, page.id, &wanted)? {
            if is_live(doc, &at) {
                wanted.remove(&id);
                homes.insert(id, (page.id, at));
            }
        }
    }
    for (n, target) in targets.iter().enumerate() {
        let found = match target {
            Target::Shape { id, page: Some(page), master: None } => on_page
                .get(&(*page, *id))
                .map(|at| Found { place: Place::Shape { owner: Owner::Page(*page), id: *id }, sheet: SheetRef::Shape(at.clone()) })
                .ok_or_else(|| said(n, format!("page {page} has no shape {id}")))?,
            Target::Shape { id, master: Some(master), .. } => in_master
                .get(&(*master, *id))
                .map(|at| Found { place: Place::Shape { owner: Owner::Master(*master), id: *id }, sheet: SheetRef::Shape(at.clone()) })
                .ok_or_else(|| said(n, format!("master {master} has no shape {id}")))?,
            Target::Shape { id, page: None, master: None } => homes
                .get(id)
                .map(|(page, at)| Found { place: Place::Shape { owner: Owner::Page(*page), id: *id }, sheet: SheetRef::Shape(at.clone()) })
                .ok_or_else(|| said(n, format!("no page has a shape {id}")))?,
            _ => match out.get_mut(n).and_then(Option::take) {
                Some(found) => found,
                None => return Err(said(n, "the sheet is not in the drawing".to_owned())),
            },
        };
        if let Some(slot) = out.get_mut(n) {
            *slot = Some(found);
        }
    }
    Ok(out.into_iter().flatten().collect())
}

// ---- planning --------------------------------------------------------------------------------------------------------------------------

/// One write, planned against the document as the call found it.
struct Planned {
    place: Place,
    sheet: SheetRef,
    addr: CellAddr,
    force: bool,
    /// The cell to put there; `None` when the write changes nothing.
    make: Option<Cell>,
}

/// The cell the sheet holds of its own at `addr`.
fn local_cell(res: &Resolver<'_>, sheet: &SheetRef, addr: &CellAddr) -> Option<Cell> {
    match res.sheet(sheet)?.lookup(addr) {
        Lookup::Found(cell) => Some(cell.clone()),
        _ => None,
    }
}

/// Plans every write, refusing the call at the first one that cannot be made, with nothing written.
fn plan(doc: &Document, writes: &[Write], batch: bool) -> Result<Vec<Planned>, Problem> {
    if writes.len() > MAX_WRITES {
        return Err(Problem::bad(format!("`writes` holds at most {MAX_WRITES} writes")));
    }
    let mut bytes = 0_usize;
    for (n, write) in writes.iter().enumerate() {
        if write.text.len() > MAX_FORMULA_BYTES {
            let words = format!("a formula is at most {MAX_FORMULA_BYTES} bytes long, and this one has {}", write.text.len());
            return Err(Problem::bad(if batch { format!("write {}: {words}", n + 1) } else { words }));
        }
        bytes = bytes.saturating_add(write.text.len());
    }
    if bytes > MAX_CALL_BYTES {
        return Err(Problem::bad(format!("the formulas of one call are at most {MAX_CALL_BYTES} bytes in all")));
    }
    let targets: Vec<&Target> = writes.iter().map(|w| &w.target).collect();
    let sheets = find_sheets(doc, &targets, batch)?;
    let res = Resolver::new(doc);
    // The cell each earlier write of the call left, so that a write is judged against it.
    let mut left_by: HashMap<(SheetRef, CellAddr), Option<Cell>> = HashMap::new();
    let mut nodes_left = MAX_CALL_NODES;
    let mut planned = Vec::with_capacity(writes.len());
    for (n, (write, found_sheet)) in writes.iter().zip(sheets).enumerate() {
        let said = |problem: Problem| {
            if batch { Problem { message: format!("write {}: {}", n + 1, problem.message), ..problem } } else { problem }
        };
        let Found { place, sheet } = found_sheet;
        let addr = sheetview::respell(&res, &sheet, write.addr.clone());
        let found = ensure_there(&res, SheetKind::of(&sheet), &sheet, &addr).map_err(said)?;
        let key = (sheet.clone(), addr.clone());
        let was = match left_by.get(&key) {
            Some(left) => left.clone(),
            None => local_cell(&res, &sheet, &addr),
        };
        let typed = read_text(&write.text, found.default.kind, &mut nodes_left).map_err(said)?;
        let made = match typed {
            Typed::Clear => clear_cell(was.as_ref(), res.inherits(&sheet, &addr)),
            Typed::Text(text) => {
                let formula = Arc::new(parse_with_budget(&quoted(&text), MAX_NODES));
                nodes_left = nodes_left.saturating_sub(formula.node_count());
                Some(formula_cell(was.as_ref(), &found, formula, Some(Held::from_text(&text))))
            }
            Typed::Formula(formula) => same_or_formula(was.as_ref(), &found, formula),
            Typed::Literal { number, unit, formula } => match literal_cell(was.as_ref(), &found, number, unit) {
                Some(cell) => Some(cell),
                // A literal that is not for this cell is a formula like any other.
                None => same_or_formula(was.as_ref(), &found, formula),
            },
        };
        let make = made.filter(|cell| was.as_ref() != Some(cell));
        let left = make.clone().or_else(|| was.clone());
        left_by.insert(key, left.clone());
        planned.push(Planned { place, sheet, addr, force: write.force, make });
    }
    Ok(planned)
}

// ---- the pages a call recalculates ------------------------------------------------------------------------------------------------------

/// The pages that hold an instance of the master, found by a walk bounded by [`MAX_INSTANCE_WALK`] shapes: every page past it.
pub(super) fn pages_using_master(doc: &Document, master: u32) -> Vec<u32> {
    let mut pages = Vec::new();
    let mut walked = 0_usize;
    for page in doc.pages.iter() {
        let mut stack: Vec<(&ShapeList, usize)> = vec![(&page.shapes, 0)];
        let mut uses = false;
        while let Some(frame) = stack.last_mut() {
            let (list, at) = *frame;
            let Some(shape) = list.get(at) else {
                stack.pop();
                continue;
            };
            frame.1 = at.saturating_add(1);
            walked = walked.saturating_add(1);
            if walked > MAX_INSTANCE_WALK {
                return doc.pages.iter().map(|p| p.id).collect();
            }
            if shape.master == Some(master) {
                uses = true;
                break;
            }
            if stack.len() < MAX_SHAPE_DEPTH && !shape.children.is_empty() {
                stack.push((&shape.children, 0));
            }
        }
        if uses {
            pages.push(page.id);
        }
    }
    pages
}

/// The pages a call works out, each with the shapes of it the call wrote.
fn pages_of(doc: &Document, planned: &[&Planned]) -> BTreeMap<u32, Vec<u32>> {
    let mut pages: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    let mut masters: HashSet<u32> = HashSet::new();
    let mut every = false;
    for p in planned {
        match &p.place {
            Place::Shape { owner: Owner::Page(page), id } => pages.entry(*page).or_default().push(*id),
            Place::Page(page) => {
                pages.entry(*page).or_default();
            }
            Place::Shape { owner: Owner::Master(master), .. } | Place::Master(master) => {
                masters.insert(*master);
            }
            Place::Style(_) | Place::Document => every = true,
        }
    }
    if every {
        for page in doc.pages.iter() {
            pages.entry(page.id).or_default();
        }
    } else {
        for master in masters {
            for page in pages_using_master(doc, master) {
                pages.entry(page).or_default();
            }
        }
    }
    pages
}

/// The words of a recalculation that could not be finished.
pub(super) fn too_large(problem: Problem, page: u32) -> Problem {
    if !problem.bad_params && problem.message.contains("too large to recalculate") {
        Problem::failed(format!(
            "the page {page} is too large to work out again after this edit, with the pages the edit reaches (they share {} steps of the evaluator): nothing was changed",
            Budget::DEFAULT_STEPS
        ))
    } else {
        problem
    }
}

/// The pages whose content is another allocation in `now` than in `before`: the pages an edit wrote, or a recalculation did. The lists of pages are compared chunk by chunk, so a
/// drawing of thousands of pages costs what the pages that differ do; a list of another length (no cell write adds a page) names every page.
pub(super) fn changed_pages(before: &Document, now: &Document) -> Vec<u32> {
    if before.pages.len() != now.pages.len() {
        return now.pages.iter().map(|p| p.id).collect();
    }
    let mut out: Vec<u32> = Vec::new();
    for (was, is) in before.pages.chunk_arcs().iter().zip(now.pages.chunk_arcs()) {
        if Arc::ptr_eq(was, is) {
            continue;
        }
        out.extend(was.iter().zip(is.iter()).filter(|(a, b)| !Arc::ptr_eq(a, b)).map(|(_, b)| b.id));
    }
    out
}

/// The document as it began (`before`), with everything but the pages' own sheets and shapes taken from the document as it is (so that a style, a master or the document's sheet a call wrote
/// is not a change a comparison of a page with it finds): what a page of the document is compared with to find the cells that were written on it.
pub(super) fn comparison_base(before: &Document, doc: &Document) -> Document {
    let mut base = before.clone();
    base.styles = doc.styles.clone();
    base.themes = doc.themes.clone();
    base.colors = Arc::clone(&doc.colors);
    base.fonts = Arc::clone(&doc.fonts);
    base.sheet = Arc::clone(&doc.sheet);
    base.masters = doc.masters.clone();
    base
}

/// What the call changed on the pages it worked out, other than the cells it wrote: the pages of the document as it began, against the document as it is ([`comparison_base`]).
fn recalculated(
    before: &Document,
    doc: &Document,
    pages: &[u32],
    written: &HashSet<(SheetRef, CellAddr)>,
) -> (Recalculated, Vec<(SheetRef, CellAddr)>) {
    let base = comparison_base(before, doc);
    let mut out = Recalculated::default();
    let mut all: Vec<(SheetRef, CellAddr)> = Vec::new();
    for page in pages {
        let changes = crate::calc_index::changes_between(&base, doc, *page);
        if changes.whole_page {
            out.more = true;
            continue;
        }
        for (sheet, addr) in changes.cells {
            if written.contains(&(sheet.clone(), addr.clone())) {
                continue;
            }
            out.cells = out.cells.saturating_add(1);
            if out.listed.len() < MAX_LISTED {
                if let Some(place) = Place::of(doc, &sheet) {
                    out.listed.push(Touched { place, cell: reference_name(&addr) });
                }
            } else {
                out.more = true;
            }
            all.push((sheet, addr));
        }
    }
    (out, all)
}

/// Whether a changed cell moves or reshapes its shape: the pose and the size, the ends, and the sections that give its outline and its connection points.
pub(super) fn moves_shape(addr: &CellAddr) -> bool {
    match &addr.section {
        None => matches!(
            addr.cell.as_str(),
            "PinX" | "PinY" | "Width" | "Height" | "LocPinX" | "LocPinY" | "Angle" | "FlipX" | "FlipY" | "BeginX" | "BeginY" | "EndX" | "EndY"
        ),
        Some(section) => matches!(section.name.as_str(), "Geometry" | "Connection"),
    }
}

// ---- the call ------------------------------------------------------------------------------------------------------------------------------

/// Makes the writes on `doc` (a copy the caller drops on an error), recalculates what they changed and says what came of them.
///
/// **All or nothing**: every write is planned (parsed, found, checked) before one is made, and an error at any stage leaves nothing the caller keeps. `batch` makes the words of a
/// refusal name the write (`write 2: ...`).
pub fn apply(doc: &mut Document, writes: &[Write], batch: bool) -> Result<Done, Problem> {
    // The pages the call works out share one budget of the evaluator's steps.
    let _shared = crate::calc_index::SharedSteps::enter(Budget::DEFAULT_STEPS);
    if batch && writes.is_empty() {
        return Err(Problem::bad("`writes` must hold at least one write"));
    }
    let before = doc.clone();
    let planned = plan(&before, writes, batch)?;
    let made: Vec<usize> = planned.iter().enumerate().filter(|(_, p)| p.make.is_some()).map(|(i, _)| i).collect();
    let batch_writes: Vec<(SheetRef, CellWrite)> = made
        .iter()
        .filter_map(|i| planned.get(*i))
        .filter_map(|p| {
            let cell = p.make.clone()?;
            let origin = if p.force { WriteOrigin::AutomationForce } else { WriteOrigin::Automation };
            Some((p.sheet.clone(), CellWrite { addr: p.addr.clone(), edit: CellEdit::Cell(cell), origin }))
        })
        .collect();
    let outcomes = doc.write_batch(batch_writes).map_err(|e| Problem::failed(format!("the write was refused: {e}")))?;
    for (outcome, index) in outcomes.iter().zip(&made) {
        if *outcome == WriteOutcome::KeptGuarded
            && let Some(p) = planned.get(*index)
        {
            let label = reference_name(&p.addr).unwrap_or_else(|| p.addr.cell.as_str().to_owned());
            let words = format!("the cell {label} is guarded by GUARD(): send force: true to replace it");
            return Err(Problem::failed(if batch { format!("write {}: {words}", index + 1) } else { words }));
        }
    }
    // The cells as they were when the call began, against what the writes left: a call that put every cell back is no change.
    let differ = |doc: &Document| {
        let (was, now) = (Resolver::new(&before), Resolver::new(doc));
        made.iter().filter_map(|i| planned.get(*i)).any(|p| local_cell(&was, &p.sheet, &p.addr) != local_cell(&now, &p.sheet, &p.addr))
    };
    let net_changed = differ(doc);
    let mut written: HashSet<(SheetRef, CellAddr)> = HashSet::new();
    for p in planned.iter().filter(|p| p.make.is_some()) {
        written.insert((p.sheet.clone(), p.addr.clone()));
    }
    let mut recalculated_out = Recalculated::default();
    if net_changed {
        let touched: Vec<&Planned> = planned.iter().filter(|p| p.make.is_some()).collect();
        // A call that wrote only cells of pages has nothing on the sheets no page works out to compare.
        if touched.iter().any(|p| !matches!(p.place, Place::Shape { owner: Owner::Page(_), .. } | Place::Page(_))) {
            let on_no_page: Vec<(SheetRef, CellAddr)> = planned
                .iter()
                .filter(|p| p.make.is_some() && !matches!(p.place, Place::Shape { owner: Owner::Page(_), .. } | Place::Page(_)))
                .map(|p| (p.sheet.clone(), p.addr.clone()))
                .collect();
            super::unpaged::work_out_written(doc, &before, &on_no_page)?;
        }
        let pages = pages_of(doc, &touched);
        // A write to the document's sheet or a style works every page out whole, the cells of one that name another included.
        let _every = touched.iter().any(|p| matches!(p.place, Place::Style(_) | Place::Document)).then(crate::calc_index::EveryPage::enter);
        for (page, shapes) in &pages {
            let mut shapes = shapes.clone();
            shapes.sort_unstable();
            shapes.dedup();
            settle(doc, &before, *page, &shapes, &[]).map_err(|problem| too_large(problem, *page))?;
        }
        // The pages the recalculation reached through a formula that names a page (`Pages[..]`) are others than those the call settled: the cells that read what the
        // call wrote were worked out there in the same commit, and are listed with the others. The connectors glued to the shapes of those pages that moved were routed again by the
        // settle step that reached them (`settle::commit`), as they are after any command.
        let reached: Vec<u32> = changed_pages(&before, doc).into_iter().filter(|page| !pages.contains_key(page)).collect();
        let mut page_ids: Vec<u32> = pages.keys().copied().collect();
        page_ids.extend(reached.iter().copied());
        // A call that worked a whole page out (a master, a style or the document) names no shape it moved, and the connectors glued to the shapes whose pose, size or outline cells
        // changed were routed again by the settle step of each page, as they are after any edit that recalculates: the cells the routes wrote are listed here too.
        recalculated_out = recalculated(&before, doc, &page_ids, &written).0;
    }
    // What each of the first writes reads as now.
    let res = Resolver::new(doc);
    let mut eval = sheetview::read_eval_ctx();
    let mut results = Vec::new();
    for p in planned.iter().take(MAX_LISTED) {
        let ctx = Ctx::new(&res, &p.sheet, &mut eval);
        let name = p.addr.cell.as_str().to_owned();
        let mut counts = Counts::default();
        if let Some(cell) = sheetview::view(&ctx, &p.addr, &name, None, None, &mut counts) {
            let changed = p.make.is_some();
            results.push(WriteResult { place: p.place.clone(), cell, changed });
        }
    }
    let written_count = planned.iter().filter(|p| p.make.is_some()).count();
    // A write that stood a value in a cell until the recalculation may be followed by one that puts the formula back, and the recalculation then gives the value back as it was
    // The cells written are judged again once the pages are worked out, and a call that left them as they were is no change, whatever it did between.
    let changed = net_changed && differ(doc);
    Ok(Done { changed, written: written_count, unchanged: planned.len().saturating_sub(written_count), results, recalculated: recalculated_out })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn literal(text: &str) -> Option<(f64, Unit)> {
        let formula = diacraft_formula::parse(text);
        literal_of(formula.ast.as_ref().unwrap())
    }

    #[test]
    fn a_number_alone_is_a_literal_with_its_sign_and_unit_and_nothing_else_is() {
        assert_eq!(literal("3"), Some((3.0, Unit::Unitless)));
        assert_eq!(literal("3 in"), Some((3.0, Unit::Inch)));
        assert_eq!(literal("-0.25 mm"), Some((-0.25, Unit::Millimetre)));
        assert_eq!(literal("+2"), Some((2.0, Unit::Unitless)));
        assert_eq!(literal("25%"), Some((25.0, Unit::Percent)));
        for not in ["1+2", "Width", "3 IN^2", "--3", "TRUE", "\"3\"", "8FT 4IN", "3 in * 2", "3%%", "-25%", "PI()"] {
            assert_eq!(literal(not), None, "{not}");
        }
    }

    #[test]
    fn a_literal_fits_the_internal_unit_or_is_none() {
        assert_eq!(literal_in_unit(2.54, Unit::Centimetre), Some(1.0));
        assert_eq!(literal_in_unit(3.0, Unit::Unitless), Some(3.0));
        assert_eq!(literal_in_unit(25.0, Unit::Percent), Some(0.25));
        assert_eq!(literal_in_unit(f64::INFINITY, Unit::Inch), None);
        assert_eq!(literal_in_unit(f64::NAN, Unit::Inch), None);
        assert_eq!(literal_in_unit(1e308, Unit::Mile), None, "a number that does not fit is not finite");
    }

    #[test]
    fn a_string_is_quoted_with_its_quotation_marks_doubled() {
        assert_eq!(quoted("Unit cost"), "\"Unit cost\"");
        assert_eq!(quoted("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(quoted(""), "\"\"");
    }

    #[test]
    fn a_text_is_read_by_the_kind_of_cell_it_is_typed_in() {
        let mut nodes = MAX_CALL_NODES;
        assert!(matches!(read_text("  ", ValueKind::Scalar, &mut nodes), Ok(Typed::Clear)));
        assert!(matches!(read_text(" = ", ValueKind::Scalar, &mut nodes), Ok(Typed::Clear)));
        assert!(matches!(read_text("Unit cost", ValueKind::Text, &mut nodes), Ok(Typed::Text(t)) if t == "Unit cost"));
        assert!(matches!(read_text("=Prop.Cost", ValueKind::Text, &mut nodes), Ok(Typed::Formula(_))));
        assert!(matches!(read_text("\"x\"", ValueKind::Text, &mut nodes), Ok(Typed::Formula(_))), "a string literal is a formula as it stands");
        assert!(matches!(read_text("3 in", ValueKind::Length, &mut nodes), Ok(Typed::Literal { .. })));
        assert!(matches!(read_text("Width*2", ValueKind::Length, &mut nodes), Ok(Typed::Formula(_))));
        let e = read_text("1+", ValueKind::Scalar, &mut nodes).err().unwrap();
        assert!(e.bad_params && e.message.contains("unexpected_end") && e.message.contains("byte 2"), "{}", e.message);
        // The parts a call may hold are counted, and a text past them is refused in words.
        let mut few = 2_usize;
        let e = read_text("1+2+3+4", ValueKind::Scalar, &mut few).err().unwrap();
        assert!(e.message.contains("too many parts"), "{}", e.message);
    }

    #[test]
    fn a_change_of_a_cell_moves_a_shape_when_it_is_the_pose_the_size_the_ends_or_the_outline() {
        assert!(moves_shape(&CellAddr::direct("PinX")));
        assert!(moves_shape(&CellAddr::in_row("Geometry", Some(0), RowKey::Ix(1), "X")));
        assert!(moves_shape(&CellAddr::in_row("Connection", None, RowKey::Ix(0), "X")));
        assert!(!moves_shape(&CellAddr::direct("FillForegnd")));
        assert!(!moves_shape(&CellAddr::in_row("Scratch", None, RowKey::Ix(0), "A")));
    }
}
