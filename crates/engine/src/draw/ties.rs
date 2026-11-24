//! A shape's `Relationships` cell, read and written: **the one reader and writer of the ties of callouts, containers and lists**.
//!
//! The cell is a `SUM` of `DEPENDSON` calls, each a code and the shapes after it by `Sheet.<id>!SheetRef()`. Code 1 is a container's members and 4 the containers a shape is in, 7 to 10
//! a shape on a container's left, right, top or bottom edge, 3 the callouts tied to a shape and 6 the shape a callout points at, and 2, 5 and 11 a list's. **A call that the code asking does not
//! own is kept as it is, in its place** ("a writer must not sort it"), and a target that is not a `Sheet.<id>` of a shape (a lost one, `REF()`, a name) is kept as the text it was.
//!
//! **Two readings of a formula.** [`Ties::from_expr`] walks the formula for its `DEPENDSON` calls wherever they are (a callout's reading, which never refuses a shape for a formula it
//! does not understand); [`Ties::is_plain`] says whether the formula is **nothing but** a list of those calls (a `SUM` of them, or one), which a container's writer asks before it rewrites a cell
//! A formula with anything else in it would lose it. [`Ties::check_rewritable`] refuses, in words, a formula that names more than [`MAX_TIE_MARKS`] shapes (the rest was not read,
//! and a rewrite would drop it).
//!
//! **Cost.** Asking whether a shape is named under a code and adding one cost a step each after the first (an index of the shapes named, made once, on the first question), and a shape is
//! taken out of a call by one pass over its marks ([`Ties::retain_sheets`], [`Ties::remove_many`], [`Ties::forget_many`]), whatever their number.

use std::cell::OnceCell;
use std::collections::HashSet;

use diacraft_doc::{Cell, CellAddr, CellEdit, FormulaSlot, Shape, SheetRef, Value};
use diacraft_formula::{Expr, Qualifier};

use super::read::Reader;

/// The code of a container's members.
pub(crate) const MEMBERS: u32 = 1;
/// The code of a list's members, in their order (a writer must not sort it).
pub(crate) const LIST_MEMBERS: u32 = 2;
/// The code of the callouts tied to a shape.
pub(crate) const CALLOUTS: u32 = 3;
/// The code of the containers a shape is in.
pub(crate) const IN_CONTAINER: u32 = 4;
/// The code of the list that holds a shape.
pub(crate) const IN_LIST: u32 = 5;
/// The code of the shape a callout points at.
pub(crate) const TARGET: u32 = 6;
/// The codes of a shape that sits on a container's left, right, top and bottom edge.
pub(crate) const ON_EDGE: [u32; 4] = [7, 8, 9, 10];

/// Most steps of the walk of one `Relationships` formula.
const MAX_TIE_STEPS: usize = 4_096;

/// The most shapes a `Relationships` formula is read as naming, in all its calls. A formula that names more is read as far as that and is **not rewritten** (a rewrite would drop the rest), so a command
/// that would change it is refused in words. The parser holds a formula to 131,072 nodes, so a formula names about 60,000 at most.
pub const MAX_TIE_MARKS: usize = 20_000;

// The reads of the cells of a shape's ties by this thread, and the marks of a call looked at while one is changed, for the tests that hold a move, a copy or a delete to a cost of its own.
#[cfg(test)]
thread_local! {
    pub(crate) static TIES_READ: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static MARKS_SCANNED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// What a `DEPENDSON` call names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Mark {
    /// `Sheet.<id>!SheetRef()`.
    Sheet(u32),
    /// `REF()`: a shape that was lost.
    Lost,
    /// Anything else, as the formula printed it.
    Other(String),
}

/// A shape's `Relationships`: each `DEPENDSON` call, in formula order, as its code and what it names.
#[derive(Clone, Debug)]
pub(crate) struct Ties {
    pub(crate) calls: Vec<(u32, Vec<Mark>)>,
    /// The formula named more than [`MAX_TIE_MARKS`] shapes and the rest was not read.
    pub(crate) truncated: bool,
    /// The formula is nothing but a list of calls.
    plain: bool,
    /// The (code, shape) pairs named, made on the first question and kept in step by the changes.
    index: OnceCell<HashSet<(u32, u32)>>,
    /// The calls (by place) that a change left with no shape: [`Ties::drop_emptied`] takes them out. A call the file held empty is not among them.
    emptied: HashSet<usize>,
}

impl Default for Ties {
    fn default() -> Ties {
        Ties { calls: Vec::new(), truncated: false, plain: true, index: OnceCell::new(), emptied: HashSet::new() }
    }
}

impl PartialEq for Ties {
    fn eq(&self, other: &Ties) -> bool {
        self.calls == other.calls && self.truncated == other.truncated
    }
}

impl Eq for Ties {}

fn mark_of(e: &Expr) -> Mark {
    match e {
        Expr::Call(call) if call.is("SHEETREF") => {
            if let [Qualifier::Path(segments)] = call.qualifiers.as_slice()
                && let [sheet, id] = segments.as_slice()
                && sheet.name.eq_ignore_ascii_case("sheet")
                && sheet.index.is_none()
                && id.index.is_none()
                && let Ok(id) = id.name.parse::<u32>()
            {
                return Mark::Sheet(id);
            }
            Mark::Other(diacraft_formula::print(e))
        }
        Expr::Call(call) if call.is("REF") && call.args.is_empty() => Mark::Lost,
        other => Mark::Other(diacraft_formula::print(other)),
    }
}

/// The code a `DEPENDSON` call opens with: a whole number from 0 to 999,999.
fn code_of(call: &diacraft_formula::Call) -> Option<u32> {
    let code = match call.args.first() {
        Some(Some(Expr::Number(n))) => n.quantity().map(|q| q.stored),
        _ => None,
    }?;
    whole_code(code)
}

/// `code` as the code of a call: a whole number from 0 to 999,999, and no other number.
pub(crate) fn whole_code(code: f64) -> Option<u32> {
    ((0.0..1.0e6).contains(&code) && code.fract() == 0.0).then_some(code as u32)
}

/// Whether the formula is nothing but a list of `DEPENDSON` calls: one, or a `SUM` of them.
fn is_plain_list(root: &Expr) -> bool {
    let call = |e: &Expr| matches!(e, Expr::Call(c) if c.is("DEPENDSON") && c.qualifiers.is_empty() && code_of(c).is_some() && c.args.iter().skip(1).all(Option::is_some));
    match root {
        Expr::Call(c) if c.is("SUM") && c.qualifiers.is_empty() => c.args.iter().all(|a| a.as_ref().is_some_and(call)),
        other => call(other),
    }
}

impl Ties {
    /// The calls of the formula `root`, walked and not evaluated.
    pub(crate) fn from_expr(root: &Expr) -> Ties {
        #[cfg(test)]
        TIES_READ.with(|c| c.set(c.get().saturating_add(1)));
        let mut calls = Vec::new();
        let mut kept = 0_usize;
        let mut truncated = false;
        let mut stack: Vec<&Expr> = vec![root];
        let mut steps = 0_usize;
        while let Some(e) = stack.pop() {
            steps += 1;
            if steps > MAX_TIE_STEPS {
                break;
            }
            match e {
                Expr::Call(call) if call.is("DEPENDSON") && call.qualifiers.is_empty() => {
                    if let Some(code) = code_of(call) {
                        let mut marks = Vec::new();
                        for arg in call.args.iter().skip(1).flatten() {
                            if kept >= MAX_TIE_MARKS {
                                truncated = true;
                                break;
                            }
                            kept += 1;
                            marks.push(mark_of(arg));
                        }
                        calls.push((code, marks));
                    }
                }
                Expr::Call(call) => stack.extend(call.args.iter().rev().flatten()),
                Expr::Unary(_, x) | Expr::Percent(x) => stack.push(x),
                Expr::Binary(_, a, b) => {
                    stack.push(b);
                    stack.push(a);
                }
                _ => {}
            }
        }
        Ties { calls, truncated, plain: is_plain_list(root), index: OnceCell::new(), emptied: HashSet::new() }
    }

    /// The ties a cell holds in a formula of its own: none for a cell with none, or one that did not parse.
    pub(crate) fn of_cell(cell: Option<&Cell>) -> Ties {
        let Some(FormulaSlot::Local(formula)) = cell.map(|c| &c.formula) else { return Ties::default() };
        match &formula.ast {
            Ok(root) => Ties::from_expr(root),
            Err(_) => Ties { plain: false, ..Ties::default() },
        }
    }

    /// Whether the formula is nothing but a list of calls: what a container's writer asks before it rewrites a cell.
    pub(crate) fn is_plain(&self) -> bool {
        self.plain
    }

    /// Whether a command may rewrite these ties: the formula is nothing but a list of calls and was read to its end.
    pub(crate) fn is_rewritable(&self) -> bool {
        self.plain && !self.truncated
    }

    /// Whether the formula is a plain list of calls that holds a container's members (an empty one says the shape is a container too).
    pub(crate) fn lists_members(&self) -> bool {
        self.plain && self.has_code(MEMBERS)
    }

    /// Whether the formula is a plain list of calls that holds shapes as a whole: a container's members (code 1) or a list's (code 2). What goes along when the shape is moved, copied or deleted.
    pub(crate) fn holds_parts(&self) -> bool {
        self.plain && (self.has_code(MEMBERS) || self.has_code(LIST_MEMBERS))
    }

    /// Whether these ties are all of the formula's: refused in words when it named more shapes than were read, as what was not read could be a callout to follow, copy or delete, and a rewrite would drop it.
    pub(crate) fn check_rewritable(&self, id: u32) -> Result<(), super::Problem> {
        if self.truncated {
            return Err(super::Problem::failed(format!(
                "the Relationships of shape {id} names more than {MAX_TIE_MARKS} shapes, which is more than DiaCraft reads or rewrites: take some away first"
            )));
        }
        Ok(())
    }

    /// Keeps under `code` the shapes that `keep` says to, and everything that is not a shape of this page (a lost one, a name): one pass over the marks. → whether any went.
    pub(crate) fn retain_sheets(&mut self, code: u32, keep: impl Fn(u32) -> bool) -> bool {
        let mut removed = false;
        for (place, (c, marks)) in self.calls.iter_mut().enumerate() {
            if *c == code {
                #[cfg(test)]
                MARKS_SCANNED.with(|n| n.set(n.get().saturating_add(marks.len())));
                let before = marks.len();
                marks.retain(|m| !matches!(m, Mark::Sheet(id) if !keep(*id)));
                removed |= marks.len() != before;
                if marks.is_empty() && before > 0 {
                    self.emptied.insert(place);
                }
            }
        }
        if removed {
            self.index = OnceCell::new();
        }
        removed
    }

    /// The ties a shape holds in a `Relationships` formula of its own: `None` for a shape with none (a master's is not read, so a walk of the shapes inside a group costs a lookup each).
    pub(crate) fn of_shape(shape: &Shape) -> Option<Ties> {
        let cell = shape.sheet.cell("Relationships")?;
        let FormulaSlot::Local(formula) = &cell.formula else { return None };
        let Ok(root) = &formula.ast else { return None };
        Some(Ties::from_expr(root))
    }

    /// The ties of a sheet as inheritance finds them: its own formula, else its master's.
    pub(crate) fn of_sheet(reader: &Reader<'_>, sheet: &SheetRef) -> Ties {
        let Some(found) = reader.resolver().resolve(sheet, &CellAddr::direct("Relationships")) else { return Ties::default() };
        let Some(FormulaSlot::Local(formula)) = found.formula.cell.map(|c| &c.formula) else { return Ties::default() };
        let Ok(root) = &formula.ast else { return Ties::default() };
        Ties::from_expr(root)
    }

    /// The shapes named under `code`, in order: the lost ones and the ones that are not `Sheet.<id>` are no shapes here.
    pub(crate) fn targets(&self, code: u32) -> Vec<u32> {
        self.calls
            .iter()
            .filter(|(c, _)| *c == code)
            .flat_map(|(_, marks)| marks.iter())
            .filter_map(|m| if let Mark::Sheet(id) = m { Some(*id) } else { None })
            .collect()
    }

    /// The shapes named under `code`, in order, each once.
    pub(crate) fn ids(&self, code: u32) -> Vec<u32> {
        let mut seen: HashSet<u32> = HashSet::new();
        self.targets(code).into_iter().filter(|id| seen.insert(*id)).collect()
    }

    /// Every shape named, under any code, each once.
    pub(crate) fn all_ids(&self) -> Vec<u32> {
        let mut seen: HashSet<u32> = HashSet::new();
        self.calls
            .iter()
            .flat_map(|(_, marks)| marks.iter())
            .filter_map(|m| if let Mark::Sheet(id) = m { Some(*id) } else { None })
            .filter(|id| seen.insert(*id))
            .collect()
    }

    pub(crate) fn count(&self, code: u32) -> usize {
        self.calls.iter().filter(|(c, _)| *c == code).map(|(_, marks)| marks.len()).sum()
    }

    /// How many marks the ties hold in all.
    pub(crate) fn named(&self) -> usize {
        self.calls.iter().map(|(_, marks)| marks.len()).sum()
    }

    /// Whether some call has this code, even one that names nobody (an empty container still says it is one).
    pub(crate) fn has_code(&self, code: u32) -> bool {
        self.calls.iter().any(|(c, _)| *c == code)
    }

    fn index(&self) -> &HashSet<(u32, u32)> {
        self.index.get_or_init(|| {
            self.calls
                .iter()
                .flat_map(|(code, marks)| marks.iter().filter_map(move |m| if let Mark::Sheet(id) = m { Some((*code, *id)) } else { None }))
                .collect()
        })
    }

    /// Whether `id` is named under `code`: a step, after the index is made.
    pub(crate) fn contains(&self, code: u32, id: u32) -> bool {
        self.index().contains(&(code, id))
    }

    /// Whether the shape holds a tie that puts it on an edge of the container `id`.
    pub(crate) fn ids_on_edge(&self, id: u32) -> bool {
        ON_EDGE.iter().any(|code| self.contains(*code, id))
    }

    /// Names `id` under `code`, in the first call of that code or a new one; nothing when it is named there already. → whether it was added.
    pub(crate) fn add(&mut self, code: u32, id: u32) -> bool {
        if self.contains(code, id) {
            return false;
        }
        match self.calls.iter_mut().find(|(c, _)| *c == code) {
            Some((_, marks)) => marks.push(Mark::Sheet(id)),
            None => self.calls.push((code, vec![Mark::Sheet(id)])),
        }
        if let Some(index) = self.index.get_mut() {
            index.insert((code, id));
        }
        true
    }

    /// Makes sure a call with this code is there, naming nobody when it has no one (a container with no members).
    pub(crate) fn ensure(&mut self, code: u32) {
        if !self.has_code(code) {
            self.calls.push((code, Vec::new()));
        }
    }

    /// Puts the shapes `ids` under `code`, in this order, in the first call of the code (a new call when there is none), and takes the other calls of the code out. A mark of the code
    /// that is no shape of the page (a lost target, a name) is kept, after them: nothing a file said is dropped. A list's order is its members' order. → whether the formula changed.
    pub(crate) fn set_order(&mut self, code: u32, ids: &[u32]) -> bool {
        let current: Vec<&Mark> = self.calls.iter().filter(|(c, _)| *c == code).flat_map(|(_, marks)| marks.iter()).collect();
        let mut wanted: Vec<Mark> = ids.iter().map(|id| Mark::Sheet(*id)).collect();
        wanted.extend(current.iter().filter(|m| !matches!(m, Mark::Sheet(_))).map(|m| (*m).clone()));
        if current.iter().copied().eq(wanted.iter()) {
            return false;
        }
        let mut placed = false;
        self.calls.retain_mut(|(c, marks)| {
            if *c != code {
                return true;
            }
            if placed {
                return false;
            }
            placed = true;
            *marks = std::mem::take(&mut wanted);
            true
        });
        if !placed {
            self.calls.push((code, wanted));
        }
        self.index = OnceCell::new();
        self.emptied.clear();
        true
    }

    /// Takes `id` out from under `code`. → whether it was there.
    pub(crate) fn remove(&mut self, code: u32, id: u32) -> bool {
        self.retain_sheets(code, |named| named != id)
    }

    /// Takes many shapes out from under `code`, in one pass over the marks. → how many marks went.
    pub(crate) fn remove_many(&mut self, code: u32, ids: &HashSet<u32>) -> usize {
        let gone = std::cell::Cell::new(0_usize);
        self.retain_sheets(code, |id| {
            let keep = !ids.contains(&id);
            gone.set(gone.get() + usize::from(!keep));
            keep
        });
        gone.get()
    }

    /// Takes many shapes out from under every code, in one pass over the marks. → whether any went.
    pub(crate) fn forget_many(&mut self, ids: &HashSet<u32>) -> bool {
        let codes: Vec<u32> = self.calls.iter().map(|(c, _)| *c).collect::<HashSet<u32>>().into_iter().collect();
        let mut any = false;
        for code in codes {
            any |= self.retain_sheets(code, |id| !ids.contains(&id));
        }
        any
    }

    /// Takes every shape out from under `code`, keeping the empty call.
    pub(crate) fn clear(&mut self, code: u32) {
        for (c, marks) in &mut self.calls {
            if *c == code {
                marks.clear();
            }
        }
        self.index = OnceCell::new();
    }

    /// Takes out the calls that a change left with no shape, except those of the codes in `keep` (a container's list of members stays, empty: it says the shape is a container). A call the file held with no
    /// shape stays, and so does one that a shape was put back into.
    pub(crate) fn drop_emptied(&mut self, keep: &[u32]) {
        if self.emptied.is_empty() {
            return;
        }
        let mut place = 0_usize;
        let emptied = std::mem::take(&mut self.emptied);
        self.calls.retain(|(code, marks)| {
            let here = place;
            place += 1;
            !(emptied.contains(&here) && marks.is_empty() && !keep.contains(code))
        });
        self.emptied = HashSet::new();
        self.index = OnceCell::new();
    }

    /// The formula: a `SUM` of the calls, as the files write it.
    pub(crate) fn print(&self) -> String {
        let calls: Vec<String> = self
            .calls
            .iter()
            .map(|(code, marks)| {
                let mut call = format!("DEPENDSON({code}");
                for mark in marks {
                    call.push(',');
                    match mark {
                        Mark::Sheet(id) => call.push_str(&format!("Sheet.{id}!SheetRef()")),
                        Mark::Lost => call.push_str("REF()"),
                        Mark::Other(text) => call.push_str(text),
                    }
                }
                call.push(')');
                call
            })
            .collect();
        format!("SUM({})", calls.join(","))
    }

    /// The write that makes the shape's `Relationships` these ties.
    pub(crate) fn write(&self, sheet: SheetRef) -> (SheetRef, CellAddr, CellEdit) {
        (sheet, CellAddr::direct("Relationships"), CellEdit::Cell(Cell::with_formula(&self.print(), Some(Value::from_text("0")))))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn of(formula: &str) -> Ties {
        let parsed = diacraft_formula::parse(formula);
        Ties::from_expr(parsed.ast.as_ref().unwrap())
    }

    #[test]
    fn a_list_is_put_in_the_order_given_in_its_first_call_and_the_other_calls_of_its_code_and_every_other_code_stay_as_they_were() {
        let mut ties =
            of("SUM(DEPENDSON(2,Sheet.5!SheetRef(),Sheet.6!SheetRef()),DEPENDSON(4,Sheet.3!SheetRef()),DEPENDSON(2,Sheet.7!SheetRef(),REF()))");
        assert!(ties.set_order(LIST_MEMBERS, &[7, 5, 6, 8]));
        assert_eq!(
            ties.print(),
            "SUM(DEPENDSON(2,Sheet.7!SheetRef(),Sheet.5!SheetRef(),Sheet.6!SheetRef(),Sheet.8!SheetRef(),REF()),DEPENDSON(4,Sheet.3!SheetRef()))"
        );
        assert!(!ties.set_order(LIST_MEMBERS, &[7, 5, 6, 8]), "the same order is no change");
        assert!(ties.contains(LIST_MEMBERS, 8) && !ties.contains(LIST_MEMBERS, 9), "the index is made again");
        // No call of the code yet: a new one is added, in the end.
        let mut none = of("SUM(DEPENDSON(4,Sheet.3!SheetRef()))");
        assert!(none.set_order(LIST_MEMBERS, &[1, 2]));
        assert_eq!(none.print(), "SUM(DEPENDSON(4,Sheet.3!SheetRef()),DEPENDSON(2,Sheet.1!SheetRef(),Sheet.2!SheetRef()))");
        // An empty order leaves the call empty: it still says the shape is a list.
        assert!(none.set_order(LIST_MEMBERS, &[]));
        assert_eq!(none.print(), "SUM(DEPENDSON(4,Sheet.3!SheetRef()),DEPENDSON(2))");
    }

    #[test]
    fn the_ties_of_a_file_are_read_and_written_back_as_they_were_and_a_lost_target_is_no_member() {
        let text = "SUM(DEPENDSON(1,Sheet.5!SheetRef(),REF(),Sheet.6!SheetRef()),DEPENDSON(4,Sheet.3!SheetRef()),DEPENDSON(6,Sheet.9!SheetRef()))";
        let ties = of(text);
        assert_eq!(ties.ids(MEMBERS), [5, 6]);
        assert_eq!(ties.ids(IN_CONTAINER), [3]);
        assert_eq!(ties.ids(TARGET), [9]);
        assert_eq!(ties.count(MEMBERS), 3, "a lost target is a mark, and no shape");
        assert_eq!(ties.print(), text, "a call this code does not own is kept in its place");
        assert!(ties.is_rewritable() && ties.lists_members());
        assert_eq!(of("SUM(DEPENDSON(1))").print(), "SUM(DEPENDSON(1))");
        assert!(of("SUM(DEPENDSON(1))").lists_members(), "an empty container still says it is one");
        assert_eq!(of("DEPENDSON(1,Sheet.2!SheetRef())").ids(MEMBERS), [2]);
    }

    #[test]
    fn a_shape_taken_out_and_put_back_is_named_once_and_the_edges_are_read_with_the_containers() {
        let mut ties = of("SUM(DEPENDSON(1,Sheet.5!SheetRef()),DEPENDSON(3),DEPENDSON(6,Sheet.5!SheetRef()),DEPENDSON(8,Sheet.2!SheetRef()))");
        assert!(ties.ids_on_edge(2) && !ties.ids_on_edge(5));
        assert!(ties.remove(MEMBERS, 5));
        assert!(!ties.remove(MEMBERS, 5));
        assert!(ties.add(MEMBERS, 5), "put back");
        assert!(!ties.add(MEMBERS, 5), "named once");
        ties.drop_emptied(&[MEMBERS]);
        assert_eq!(ties.print(), "SUM(DEPENDSON(1,Sheet.5!SheetRef()),DEPENDSON(3),DEPENDSON(6,Sheet.5!SheetRef()),DEPENDSON(8,Sheet.2!SheetRef()))");
        assert!(ties.forget_many(&[5].into_iter().collect()));
        ties.drop_emptied(&[MEMBERS]);
        assert_eq!(
            ties.print(),
            "SUM(DEPENDSON(1),DEPENDSON(3),DEPENDSON(8,Sheet.2!SheetRef()))",
            "a call a change emptied goes, the list of members and an empty call the file held stay"
        );
    }

    #[test]
    fn a_plain_list_of_calls_is_plain_and_anything_else_in_the_formula_is_not() {
        assert!(of("SUM(DEPENDSON(1,Sheet.5!SheetRef(),REF()),DEPENDSON(4))").is_plain());
        assert!(of("DEPENDSON(1,Sheet.2!SheetRef())").is_plain());
        assert!(Ties::default().is_plain(), "no cell, no ties, nothing to lose");
        for text in [
            "Width*2",
            "IF(1,DEPENDSON(1),0)",
            "SUM(DEPENDSON(1),2)",
            "SUM(DEPENDSON(x,Sheet.1!SheetRef()))",
            "SUM(DEPENDSON(1.5))",
            "SUM(DEPENDSON(1,,Sheet.1!SheetRef()))",
        ] {
            assert!(!of(text).is_plain(), "{text}");
        }
        // The callout's reading finds the calls all the same.
        assert_eq!(of("IF(1,DEPENDSON(3,Sheet.8!SheetRef()),0)").targets(3), [8]);
    }

    #[test]
    fn a_shape_is_named_once_by_the_index_and_taken_out_in_one_pass() {
        let mut ties = Ties::default();
        for id in 1..=20_000_u32 {
            assert!(ties.add(MEMBERS, id));
        }
        assert!(!ties.add(MEMBERS, 7), "a shape is named once");
        assert!(ties.contains(MEMBERS, 20_000) && !ties.contains(IN_CONTAINER, 1));
        MARKS_SCANNED.with(|c| c.set(0));
        let gone: HashSet<u32> = (1..=15_000_u32).collect();
        assert_eq!(ties.remove_many(MEMBERS, &gone), 15_000);
        assert_eq!(MARKS_SCANNED.with(std::cell::Cell::get), 20_000, "one pass over the marks, whatever the number taken out");
        assert!(!ties.contains(MEMBERS, 15_000) && ties.contains(MEMBERS, 15_001));
        assert_eq!(ties.ids(MEMBERS).len(), 5_000);
        assert!(ties.add(MEMBERS, 1), "a shape taken out can be put back");
    }

    #[test]
    fn forgetting_a_shape_takes_it_from_every_code_and_drop_empty_calls_leaves_the_members_call() {
        let mut ties = of("SUM(DEPENDSON(1,Sheet.5!SheetRef()),DEPENDSON(3),DEPENDSON(4,Sheet.5!SheetRef()),DEPENDSON(6,Sheet.5!SheetRef()))");
        assert!(ties.forget_many(&[5].into_iter().collect()));
        ties.drop_emptied(&[MEMBERS]);
        assert_eq!(
            ties.print(),
            "SUM(DEPENDSON(1),DEPENDSON(3))",
            "a container's list of members and an empty call the file held stay, a call a change emptied goes"
        );
        assert!(!ties.forget_many(&[5].into_iter().collect()));
    }

    #[test]
    fn a_formula_that_names_more_than_the_most_is_read_that_far_and_refused_in_words() {
        let list = (2..20_100_u32).map(|id| format!("Sheet.{id}!SheetRef()")).collect::<Vec<_>>().join(",");
        let ties = of(&format!("SUM(DEPENDSON(1,{list}))"));
        assert!(ties.truncated && ties.named() == MAX_TIE_MARKS);
        let e = ties.check_rewritable(9).unwrap_err();
        assert!(e.message.contains("names more than 20000 shapes"), "{}", e.message);
    }
}
