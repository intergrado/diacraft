//! One read of a page for the container commands: the top-level shapes by `ID`, the containers among them, and what a shape's ties and box are.
//!
//! **Only the shapes of the page's own list take part.** A container and its members are shapes of the page: a shape inside a group is not a member, and
//! a group is one shape that may be one.
//!
//! **The work of a command is one budget** ([`Work`], [`MAX_WORK`] steps), shared by every read of the page the command makes: a shape visited, a tie read, a box measured and a
//! container looked at for each eight a shape is tried against are steps, charged before the work is done. A budget that is spent is an error in words wherever it is met, and
//! never an answer: a box that could not be measured is not "no box", and a container that could not be looked at is not "no container" (the security scan's first finding). The
//! budget is the edit's (`Session::edit_if` enters [`EditWork`]), so the reads that `draw::move_each`, the command and the fitting make add up; a library caller outside an
//! edit has one for each read.

use std::cell::{Cell as Counter, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use diacraft_doc::{CellAddr, Document, FormulaSlot, MAX_SHAPES_PER_PAGE, Owner, RowKey, ShapeRef, ShapeType, SheetRef};

use super::settings::Settings;
use crate::draw::layers::{Layer, Locks};
use crate::draw::ties::{IN_CONTAINER, IN_LIST, LIST_MEMBERS, MAX_TIE_MARKS, MEMBERS, ON_EDGE, Ties};
use crate::draw::{Problem, Reader, units_per_inch};

/// The steps of work one container command may do: a shape visited, a tie read, a box measured are a step each (a box, a few). A page at its cap of shapes and a container that
/// holds all of them are within it; a refusal says so.
pub const MAX_WORK: usize = 8_000_000;

/// Why the ties of a shape are not read: its `Relationships` formula holds more than a list of calls, or names more shapes than are read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Odd {
    NotAList,
    TooMany,
}

thread_local! {
    /// The budget of the edit that is running on this thread, shared by every [`Work`] made while it runs.
    static EDIT: RefCell<Option<Rc<Counter<usize>>>> = const { RefCell::new(None) };
}

/// A budget of steps, charged before the work is done. Clones share what is left.
#[derive(Clone, Debug)]
pub(super) struct Work {
    left: Rc<Counter<usize>>,
}

impl Work {
    /// The budget of the edit that is running, or a new one of [`MAX_WORK`] steps.
    pub(super) fn new() -> Work {
        let shared = EDIT.with(|edit| edit.borrow().clone());
        Work { left: shared.unwrap_or_else(|| Rc::new(Counter::new(MAX_WORK))) }
    }

    /// Charges `n` steps. A budget that cannot pay is spent for good: every later charge is refused too.
    pub(super) fn charge(&self, n: usize) -> Result<(), Problem> {
        match self.left.get().checked_sub(n) {
            Some(left) => {
                self.left.set(left);
                Ok(())
            }
            None => {
                self.left.set(0);
                Err(Problem::failed(format!(
                    "the containers of the page need more than {MAX_WORK} steps of work in one command: use fewer shapes or fewer containers"
                )))
            }
        }
    }

    /// The steps left.
    #[cfg(test)]
    pub(super) fn left(&self) -> usize {
        self.left.get()
    }
}

/// For as long as it lives, the reads of containers that this thread's edit makes share one budget (`Session::edit_if` takes it around the command's closure). One taken while
/// another lives leaves the other's in place.
pub(crate) struct EditWork {
    entered: bool,
}

impl EditWork {
    pub(crate) fn enter() -> EditWork {
        EditWork::enter_with(MAX_WORK)
    }

    /// [`EditWork::enter`] with `steps` for the edit.
    pub(crate) fn enter_with(steps: usize) -> EditWork {
        let entered = EDIT.with(|edit| {
            let mut edit = edit.borrow_mut();
            if edit.is_some() {
                return false;
            }
            *edit = Some(Rc::new(Counter::new(steps)));
            true
        });
        EditWork { entered }
    }
}

impl Drop for EditWork {
    fn drop(&mut self) {
        if self.entered {
            EDIT.with(|edit| *edit.borrow_mut() = None);
        }
    }
}

// How many shapes the lists that keep each shape once have been asked about, for the tests that hold a command to a set and not a search of a list. Not kept outside tests.
#[cfg(test)]
thread_local! {
    pub(super) static PROBES: Counter<usize> = const { Counter::new(0) };
}

/// A list of shapes that holds each once and keeps the order they came in: a set beside a vector, so that a shape is asked about in one step and never by a search of the list
/// (the security scan's second finding: `out.contains` in a loop is the square of the length).
#[derive(Debug, Default)]
pub(super) struct Unique {
    seen: HashSet<u32>,
    order: Vec<u32>,
}

impl Unique {
    pub(super) fn new() -> Unique {
        Unique::default()
    }

    /// Adds the shape unless it is there. → whether it was added.
    pub(super) fn push(&mut self, id: u32) -> bool {
        #[cfg(test)]
        PROBES.with(|c| c.set(c.get() + 1));
        let added = self.seen.insert(id);
        if added {
            self.order.push(id);
        }
        added
    }

    #[cfg(test)]
    pub(super) fn contains(&self, id: u32) -> bool {
        self.seen.contains(&id)
    }

    pub(super) fn len(&self) -> usize {
        self.order.len()
    }

    pub(super) fn into_vec(self) -> Vec<u32> {
        self.order
    }
}

/// A box on the page, sides parallel to its axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Rect {
    /// The middle of the box: in it whatever its sides, the sides of a box past half the largest number too (their sum would pass it).
    pub(crate) fn centre(&self) -> (f64, f64) {
        (self.x0.midpoint(self.x1), self.y0.midpoint(self.y1))
    }

    pub(crate) fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    pub(crate) fn height(&self) -> f64 {
        self.y1 - self.y0
    }

    pub(super) fn area(&self) -> f64 {
        self.width() * self.height()
    }

    /// Whether the point is in the box, its edges included.
    pub(crate) fn contains(&self, (x, y): (f64, f64)) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }

    pub(crate) fn union(self, other: Rect) -> Rect {
        Rect { x0: self.x0.min(other.x0), y0: self.y0.min(other.y0), x1: self.x1.max(other.x1), y1: self.y1.max(other.y1) }
    }

    /// Whether every side of `other` is within this box (a tolerance of a millionth, for the arithmetic of a recalculation).
    pub(crate) fn holds(&self, other: &Rect) -> bool {
        const EPS: f64 = 1.0e-6;
        other.x0 >= self.x0 - EPS && other.y0 >= self.y0 - EPS && other.x1 <= self.x1 + EPS && other.y1 <= self.y1 + EPS
    }
}

/// What a candidate container is, looked at once for the page read: its box, whether it is locked, whether it takes members, whether its ties are a list that can be
/// rewritten, and whether its own list names a locked layer (it is then left as it is).
#[derive(Clone, Copy, Debug)]
pub(super) struct Candidate {
    pub rect: Option<Rect>,
    pub locked: bool,
    pub takes: bool,
    pub usable: bool,
    pub on_locked_layer: bool,
}

/// What a page holds of containers, read once.
pub(super) struct Page<'a> {
    pub(super) doc: &'a Document,
    pub(super) reader: Reader<'a>,
    pub(super) id: u32,
    /// Drawing units to a paper inch.
    pub(super) k: f64,
    /// Where each top-level shape is in the page's list, by `ID` (the first of an `ID` the page repeats).
    index: HashMap<u32, usize>,
    /// The `ID`s of the shapes that are containers (typed `Container` or `List`, or holding members), in drawing order.
    pub(super) containers: Vec<u32>,
    container_set: HashSet<u32>,
    pub(super) work: Work,
    candidates: RefCell<HashMap<u32, Candidate>>,
    /// The page's locked layers, read when first asked, and what reading the shapes' lists has cost.
    locks: RefCell<Locks>,
    /// The containers on a locked layer that a command passed over, each once, in the order they were met: what its answer notes.
    passed: RefCell<Unique>,
    /// What each shape's own or inherited `User.msvStructureType` says, for the shapes that say `Container` or `List` (read in the walk of the page).
    typed: HashMap<u32, &'static str>,
    /// What a file's own cells make of the swimlanes of the page, found when first asked and kept for the read.
    pub(super) file: RefCell<super::lists::FileParts>,
}

/// Whether a shape's own or inherited `User.msvStructureType` says `Container` or `List`.
pub(super) fn typed_container(reader: &Reader<'_>, sheet: &SheetRef) -> Option<&'static str> {
    let addr = CellAddr::in_row("User", None, RowKey::name("msvStructureType"), "Value");
    let found = reader.resolver().resolve(sheet, &addr)?;
    // A cell that holds a formula and no saved value is what the formula works out to, as the picture reads it.
    let text = reader.worked().text_of(sheet, &addr, &found)?.trim().to_owned();
    match text.as_str() {
        "Container" => Some("Container"),
        "List" => Some("List"),
        _ => None,
    }
}

impl<'a> Page<'a> {
    /// Reads page `page`: one walk of its list, charged. A page with more shapes than the model allows is refused.
    pub(super) fn read(doc: &'a Document, page: u32) -> Result<Page<'a>, Problem> {
        let p = doc.page(page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
        if p.shapes.len() > MAX_SHAPES_PER_PAGE {
            return Err(Problem::failed(format!("a page holds at most {MAX_SHAPES_PER_PAGE} shapes")));
        }
        let reader = Reader::new(doc);
        let work = Work::new();
        let mut index: HashMap<u32, usize> = HashMap::with_capacity(p.shapes.len());
        let mut containers: Vec<u32> = Vec::new();
        let mut typed: HashMap<u32, &'static str> = HashMap::new();
        for (at, shape) in p.shapes.iter().enumerate() {
            work.charge(1)?;
            index.entry(shape.id).or_insert(at);
            if shape.kind == ShapeType::Guide || index.get(&shape.id) != Some(&at) {
                continue;
            }
            // A shape that has neither a master nor a user row of its own cannot be one: no read of its cells.
            let may = shape.master.is_some() || shape.sheet.sections.iter().any(|s| s.name.as_str() == "User");
            let own_ties = shape.sheet.cell("Relationships").is_some();
            if !may && !own_ties {
                continue;
            }
            work.charge(4)?;
            let sheet = SheetRef::Shape(ShapeRef { owner: Owner::Page(page), path: vec![u32::try_from(at).unwrap_or(u32::MAX)] });
            let kind = typed_container(&reader, &sheet);
            if let Some(kind) = kind {
                typed.insert(shape.id, kind);
            }
            let is = kind.is_some() || (own_ties && Self::reads_members(&reader, &sheet));
            if is {
                containers.push(shape.id);
            }
        }
        let container_set = containers.iter().copied().collect();
        Ok(Page {
            doc,
            reader,
            id: page,
            k: units_per_inch(doc, page),
            index,
            containers,
            container_set,
            work,
            candidates: RefCell::new(HashMap::new()),
            locks: RefCell::new(Locks::default()),
            passed: RefCell::new(Unique::new()),
            typed,
            file: RefCell::new(super::lists::FileParts::default()),
        })
    }

    /// What the shape's `User.msvStructureType` says, `Container` or `List`, read in the walk of the page: `None` for any other value, or none.
    pub(super) fn typed(&self, id: u32) -> Option<&'static str> {
        self.typed.get(&id).copied()
    }

    fn reads_members(reader: &Reader<'_>, sheet: &SheetRef) -> bool {
        let found = reader.resolver().resolve(sheet, &CellAddr::direct("Relationships"));
        Ties::of_cell(found.and_then(|f| f.formula.cell)).lists_members()
    }

    /// Whether the page has a top-level shape with this `ID`.
    pub(super) fn has(&self, id: u32) -> bool {
        self.index.contains_key(&id)
    }

    /// The place of the shape in the page's list.
    pub(super) fn at(&self, id: u32) -> Option<usize> {
        self.index.get(&id).copied()
    }

    pub(super) fn shape_ref(&self, id: u32) -> Option<ShapeRef> {
        let at = self.at(id)?;
        Some(ShapeRef { owner: Owner::Page(self.id), path: vec![u32::try_from(at).ok()?] })
    }

    pub(super) fn sheet(&self, id: u32) -> Option<SheetRef> {
        self.shape_ref(id).map(SheetRef::Shape)
    }

    /// Whether the shape is a container (typed `Container` or `List`, or holding members).
    pub(super) fn is_container(&self, id: u32) -> bool {
        self.container_set.contains(&id)
    }

    /// Whether the shape is a container that takes new members: typed `Container` (a list holds the members it is given as a sequence, by `swimlane.insert`, and takes none by a drop), and
    /// not a phase (a phase is a band across the lanes, and a shape dropped over it goes to the lane under it).
    pub(super) fn takes_members(&self, id: u32) -> Result<bool, Problem> {
        self.work.charge(2)?;
        Ok(self.typed(id) == Some("Container") && super::lists::role(self, id)? != super::lists::Role::Phase)
    }

    /// The ties of a shape: `Err(Odd)` inside when its `Relationships` formula is not a plain list of calls. The formula's length is charged before it is read.
    fn read_ties(&self, id: u32) -> Result<Result<Ties, Odd>, Problem> {
        self.work.charge(2)?;
        let Some(sheet) = self.sheet(id) else { return Ok(Ok(Ties::default())) };
        let found = self.reader.resolver().resolve(&sheet, &CellAddr::direct("Relationships"));
        let cell = found.and_then(|f| f.formula.cell);
        if let Some(FormulaSlot::Local(formula)) = cell.map(|c| &c.formula) {
            self.work.charge(formula.source.len() / 16)?;
        }
        let ties = Ties::of_cell(cell);
        self.work.charge(ties.named() / 4)?;
        Ok(if !ties.is_plain() {
            Err(Odd::NotAList)
        } else if ties.truncated {
            Err(Odd::TooMany)
        } else {
            Ok(ties)
        })
    }

    /// The ties of a shape on the page: none when it has none; refused in words when its `Relationships` formula is not a plain list of calls (for a command that would
    /// rewrite them).
    pub(super) fn ties(&self, id: u32) -> Result<Ties, Problem> {
        self.read_ties(id)?.map_err(|odd| match odd {
            Odd::NotAList => Problem::failed(format!(
                "the `Relationships` cell of shape {id} is not a list of ties (a SUM of DEPENDSON calls), and a container command does not rewrite a formula it does not understand"
            )),
            Odd::TooMany => Problem::failed(format!(
                "the `Relationships` cell of shape {id} names more than {MAX_TIE_MARKS} shapes, which is more than DiaCraft reads or rewrites: take some away first"
            )),
        })
    }

    /// Whether the shape's ties can be read and rewritten. An automatic step (a move's membership, a fit) skips a shape that has none it understands, and says nothing.
    pub(super) fn usable(&self, id: u32) -> Result<bool, Problem> {
        Ok(self.read_ties(id)?.is_ok())
    }

    /// The ties of a shape when they can be read and rewritten, `None` when they cannot: what a walk of membership reads, which names nobody for such a shape.
    pub(super) fn plain_ties(&self, id: u32) -> Result<Option<Ties>, Problem> {
        Ok(self.read_ties(id)?.ok())
    }

    /// Whether the shape sits on an edge of the container (codes 7 to 10): a shape whose ties are not a list does not.
    pub(super) fn attached(&self, member: u32, container: u32) -> Result<bool, Problem> {
        Ok(self.read_ties(member)?.is_ok_and(|t| t.ids_on_edge(container)))
    }

    /// The containers a shape is in, among the shapes of the page: its direct ties of kind 4, and the edges it sits on (7 to 10). A shape whose ties are not a list is in none.
    pub(super) fn containers_of(&self, id: u32) -> Result<Vec<u32>, Problem> {
        let Ok(ties) = self.read_ties(id)? else { return Ok(Vec::new()) };
        let mut out = Unique::new();
        for code in std::iter::once(IN_CONTAINER).chain(ON_EDGE) {
            for c in ties.ids(code) {
                if c != id && self.has(c) {
                    out.push(c);
                }
            }
        }
        Ok(out.into_vec())
    }

    /// The members of a container that are shapes of the page, in the order the container lists them. A container whose ties are not a list holds none.
    pub(super) fn members_of(&self, id: u32) -> Result<Vec<u32>, Problem> {
        let Ok(ties) = self.read_ties(id)? else { return Ok(Vec::new()) };
        Ok(ties.ids(MEMBERS).into_iter().filter(|m| *m != id && self.has(*m)).collect())
    }

    /// What a shape holds as a whole: the shapes of a container's list of members (code 1) and then those of a list's (code 2), each once, in the order the shape names them.
    pub(super) fn parts_of(&self, id: u32) -> Result<Vec<u32>, Problem> {
        let Ok(ties) = self.read_ties(id)? else { return Ok(Vec::new()) };
        // Each is named once: a shape that a container and a list both name is one member (a set and not a search of the list; the probes `Unique` counts are for the listings of a container's members).
        let mut seen: HashSet<u32> = HashSet::new();
        let mut out: Vec<u32> = Vec::new();
        for code in [MEMBERS, LIST_MEMBERS] {
            for m in ties.ids(code) {
                if m != id && self.has(m) && seen.insert(m) {
                    out.push(m);
                }
            }
        }
        Ok(out)
    }

    /// The members of a list (code 2), in the order the list names them: the shapes of the page. A shape whose ties are not a list holds none.
    pub(super) fn list_members_of(&self, id: u32) -> Result<Vec<u32>, Problem> {
        let Ok(ties) = self.read_ties(id)? else { return Ok(Vec::new()) };
        Ok(ties.ids(LIST_MEMBERS).into_iter().filter(|m| *m != id && self.has(*m)).collect())
    }

    /// The list that holds a shape (code 5): the first one the shape names that is a shape of the page.
    pub(super) fn list_of(&self, id: u32) -> Result<Option<u32>, Problem> {
        let Ok(ties) = self.read_ties(id)? else { return Ok(None) };
        Ok(ties.ids(IN_LIST).into_iter().find(|l| *l != id && self.has(*l)))
    }

    /// The box a shape's box is on the page. `None` for a shape with no box or one that is not finite; an error when the budget cannot pay for measuring it.
    pub(super) fn bounds(&self, id: u32) -> Result<Option<Rect>, Problem> {
        self.work.charge(8)?;
        let Some(at) = self.shape_ref(id) else { return Ok(None) };
        let sheet = SheetRef::Shape(at.clone());
        let points: Vec<(f64, f64)> = if self.reader.is_one_d(&sheet) {
            let n = |name: &str| self.reader.number(&sheet, name);
            vec![self.reader.parent_to_page(&at, (n("BeginX"), n("BeginY"))), self.reader.parent_to_page(&at, (n("EndX"), n("EndY")))]
        } else {
            self.reader.corners(&at).to_vec()
        };
        if !points.iter().all(|(x, y)| x.is_finite() && y.is_finite()) {
            return Ok(None);
        }
        let Some(first) = points.first().copied() else { return Ok(None) };
        let mut r = Rect { x0: first.0, y0: first.1, x1: first.0, y1: first.1 };
        for (x, y) in points {
            r = r.union(Rect { x0: x, y0: y, x1: x, y1: y });
        }
        Ok(Some(r))
    }

    /// What the container `id` is as a place to put a shape in: looked at once, charged once, and remembered for the rest of the read.
    pub(super) fn candidate(&self, id: u32) -> Result<Candidate, Problem> {
        if let Some(known) = self.candidates.borrow().get(&id) {
            return Ok(*known);
        }
        let found = Candidate {
            rect: self.bounds(id)?,
            locked: Settings::read(self, id)?.locked,
            takes: self.takes_members(id)?,
            usable: self.usable(id)?,
            on_locked_layer: self.locked_layer(id)?.is_some(),
        };
        self.candidates.borrow_mut().insert(id, found);
        Ok(found)
    }

    /// The first locked layer the shape `id`'s own list names, `None` when it names none: the page's table is read once for the page read, and each list read is
    /// charged a step and its bytes to one budget ([`Locks`]).
    pub(super) fn locked_layer(&self, id: u32) -> Result<Option<Layer>, Problem> {
        let Some(at) = self.shape_ref(id) else { return Ok(None) };
        let Some(shape) = self.doc.shape(&at) else { return Ok(None) };
        self.work.charge(1)?;
        self.locks.borrow_mut().of(self.doc, self.id, shape)
    }

    /// Whether the shape `id` is on a locked layer ([`Page::locked_layer`]).
    pub(super) fn on_locked_layer(&self, id: u32) -> Result<bool, Problem> {
        Ok(self.locked_layer(id)?.is_some())
    }

    /// Notes that a command passed over the container `id`, which is on a locked layer.
    pub(super) fn pass_over(&self, id: u32) {
        self.passed.borrow_mut().push(id);
    }

    /// The containers on a locked layer that the command passed over in this read, each once, in the order they were met.
    pub(super) fn passed_over(&self) -> Vec<u32> {
        std::mem::take(&mut *self.passed.borrow_mut()).into_vec()
    }

    /// The shapes inside the containers `ids`, nested ones too: every shape a container or a list holds, and what those hold, each once, in the order they are found. A container among
    /// `ids` is not listed again, and a shape that holds one of its own containers (a cycle in a file) stops the walk there.
    pub(super) fn contents(&self, ids: &[u32]) -> Result<Vec<u32>, Problem> {
        let mut seen: HashSet<u32> = ids.iter().copied().collect();
        let mut out = Unique::new();
        let mut queue: Vec<u32> = ids.iter().rev().copied().collect();
        while let Some(next) = queue.pop() {
            for member in self.parts_of(next)? {
                self.work.charge(1)?;
                if seen.insert(member) {
                    out.push(member);
                    queue.push(member);
                }
            }
        }
        Ok(out.into_vec())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::sync::Arc;

    use diacraft_doc::{Cell, CellName, Shape, Sheet, Value};
    use diacraft_testkit::fixtures::blank_document;

    use super::*;

    /// A document whose first page holds `n` rectangles side by side.
    fn page_of(n: u32) -> Document {
        let mut doc = blank_document();
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        for id in 1..=n {
            let mut shape = Shape::new(id);
            shape.kind = ShapeType::Shape;
            let mut sheet = Sheet::default();
            for (name, v) in
                [("PinX", f64::from(id)), ("PinY", 1.0), ("Width", 0.5), ("Height", 0.5), ("LocPinX", 0.25), ("LocPinY", 0.25), ("Angle", 0.0)]
            {
                sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text(&v.to_string())).with_unit("IN"));
            }
            shape.sheet = Arc::new(sheet);
            page.shapes.push(Arc::new(shape));
        }
        doc
    }

    #[test]
    fn a_spent_budget_is_an_error_and_never_no_box() {
        let doc = page_of(3);
        let _edit = EditWork::enter_with(200);
        let page = Page::read(&doc, 1).unwrap();
        // A read of the page costs a step for each shape: the rest is spent on measuring.
        let mut asked = 0;
        loop {
            match page.bounds(2) {
                Ok(Some(_)) => asked += 1,
                Ok(None) => panic!("a spent budget was read as no box after {asked} boxes"),
                Err(e) => {
                    assert!(e.message.contains("steps of work in one command"), "{}", e.message);
                    break;
                }
            }
            assert!(asked < 100, "the budget never ran out");
        }
        assert!(page.bounds(2).is_err(), "and it stays spent");
        assert_eq!(page.work.left(), 0);
    }

    #[test]
    fn the_reads_of_one_edit_share_one_budget_and_a_read_outside_an_edit_has_its_own() {
        let doc = page_of(10);
        {
            let _edit = EditWork::enter_with(25);
            Page::read(&doc, 1).unwrap();
            Page::read(&doc, 1).unwrap();
            let e = Page::read(&doc, 1).err().expect("the third read of ten shapes is more than 25 steps");
            assert!(e.message.contains("steps of work in one command"), "{}", e.message);
        }
        Page::read(&doc, 1).unwrap();
        Page::read(&doc, 1).unwrap();
    }

    #[test]
    fn a_shape_asked_about_is_one_probe_of_a_set_and_not_a_search_of_the_list() {
        let mut list = Unique::new();
        PROBES.with(|c| c.set(0));
        for round in 0..3_u32 {
            for id in 0..20_000_u32 {
                list.push(id + round);
            }
        }
        assert_eq!(list.len(), 20_002);
        assert!(list.contains(20_001));
        assert_eq!(PROBES.with(Counter::get), 60_000, "one step for each shape asked about, however long the list");
    }

    /// The counterexample, replayed: a box whose sides are past half the largest number had an infinity for its middle (the sum of two sides overflowed), so the middle
    /// was not in the box, a shape there was in no container, and a member's edge was moved by an infinity.
    #[test]
    fn a_box_at_the_far_end_of_the_numbers_holds_its_own_middle() {
        for (lo, hi) in [(f64::MAX, f64::MAX), (f64::MAX / 2.0 * 1.5, f64::MAX), (-f64::MAX, -f64::MAX), (-f64::MAX, f64::MAX), (1.0, 3.0)] {
            let r = Rect { x0: lo, y0: lo, x1: hi, y1: hi };
            let (cx, cy) = r.centre();
            assert!(cx.is_finite() && cy.is_finite() && r.contains((cx, cy)), "{lo} to {hi}: the middle {cx} is in the box");
        }
        assert_eq!(Rect { x0: 1.0, y0: -2.0, x1: 3.0, y1: 4.0 }.centre(), (2.0, 1.0), "a box of ordinary sides has its middle where it was");
    }
}
