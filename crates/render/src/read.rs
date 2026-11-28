//! Reading cells to draw them: the saved value, found by inheritance, and nothing recalculated.
//!
//! **Draw what was saved**: a cell's `V` stands, found by through the
//! document's [`Resolver`]. Two things need more than the saved text, and both are narrow.
//!
//! * The seven `Text Transform` cells and the two picture size cells that a file leaves out read a printed formula,
//!   `Width*0.5` and the like. [`Cx::num`] works those out itself: they are a cell's name and a factor.
//! * A value that is the marker `Themed` has no number to read: about a third of page shapes
//!   reach it for `FillPattern`. It is asked of the theme, by `calc`'s evaluator running the cell's own formula (`THEMEVAL()`) on the
//!   sheet that asked, and the answer is kept. When the theme cannot answer, the cell reads as missing and the
//!   caller's default stands, and the frame says so.
//! * A cell with a formula and **no value saved anywhere on the way** (`holds_unsaved_formula`: a style's `THEMEVAL()` with no `V` beside it, say) saved nothing to
//!   draw: its formula is worked out, on the sheet that asked and with the cells it reads worked out as well, and the answer is kept. Nothing
//!   else is evaluated: a cell with a saved value stands as saved. A result that is no value leaves the cell as it was read.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use diacraft_calc::{Budget, CellAddress, CellError, EvalCtx, EvalOptions, Evaluator, OperandMode, Value as Calc};
use diacraft_color::Rgba;
use diacraft_doc::lang::{self, Tag};
use diacraft_doc::{CellAddr, Document, Reading, Resolved, Resolver, RowKey, ShapeRef, SheetRef};

use crate::skips::{SkipKind, Skips};
use crate::worked::WorkedCache;

/// The steps `calc` may spend on themed cells in one frame. A `THEMEVAL()` is a handful of steps, and the answers are kept.
pub(crate) const THEME_STEPS: u64 = 4_000_000;

/// The steps `calc` may spend, **beside** [`THEME_STEPS`] and not out of them, on the cells a frame reads that hold a formula and no saved value. A
/// `THEMEVAL()` is 25 steps and each shape that follows a style of them has an answer of its own for each cell it reads: a page of 30,000 shapes that read seven is
/// five million, which the steps of the themed cells (and a formula that drew its steps from them) would have spent on the first 23,000, leaving the rest to the defaults.
/// The clock of a window's frame (`Limits::frame_time`) is what bounds the time; this bounds what an export that has no clock can be asked to work out.
pub const UNSAVED_STEPS: u64 = 32_000_000;

/// What a cell holds, read for drawing ([`read_cell`]).
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    /// No value anywhere on the way: the caller's default applies.
    Missing,
    /// A number.
    Num(f64),
    /// A colour.
    Color(Rgba),
    /// A text.
    Text(Arc<str>),
}

impl Drop for Cx<'_, '_> {
    /// Charges the cache of the document the steps this reading spent on cells with no saved value. A context that is borrowed (a panic that met it so) is not looked at:
    /// the steps it spent are not charged, which is the most that can be said of a frame that was lost.
    fn drop(&mut self) {
        if let Some(cache) = &self.kept
            && let Ok(own) = self.own.try_borrow()
        {
            cache.spend(self.allotted.saturating_sub(own.steps_left()));
        }
    }
}

/// A bound on a length that a file gives as `1.#INF`: past every page, short of overflowing arithmetic.
const BIG: f64 = 1.0e15;

/// The reading context of one frame: the resolver, the evaluator for themed values, and what the frame has to report.
///
/// **The three `RefCell`s are private, and each is borrowed only inside one method here, for a call that reaches no other of them.** A
/// reading (`val`) may borrow `themed`, `ev` and `skips` one after the other; nothing outside this file can hold one open while it
/// reads (a caller that held `skips` across a read panicked on the second borrow, and 11 of 12 real files drew a blank page). The
/// frame reports through [`Cx::note`] and [`Cx::note_n`], which borrow the list for the length of the count and no longer.
pub(crate) struct Cx<'a, 'c> {
    pub doc: &'a Document,
    pub res: Resolver<'a>,
    ev: RefCell<Evaluator<'a, 'c>>,
    themed: RefCell<HashMap<(SheetRef, CellAddr), Val>>,
    skips: RefCell<Skips>,
    /// The drawing's language (`lang::document_language`), read the first time a run asks for it.
    document_language: OnceCell<Tag>,
    /// The context the cells with a formula and no saved value are worked out in, with steps of its own ([`UNSAVED_STEPS`]) (`unsaved`). Read operands as worked out.
    own: RefCell<EvalCtx>,
    /// What the earlier frames of this document worked out, when the caller keeps them ([`WorkedCache`]): asked before a cell is worked out, and added to after.
    kept: Option<Arc<WorkedCache>>,
    /// The steps `own` was given: what it spent is this less what it has left, and is charged to the cache when the reading ends (`Drop`).
    allotted: u64,
}

/// The state `calc`'s evaluator borrows for a frame.
pub(crate) fn eval_ctx() -> EvalCtx {
    eval_ctx_steps(THEME_STEPS)
}

/// As [`eval_ctx`] with `steps` to spend: a share of a parallel walk has a part of the frame's (`share.rs`).
pub(crate) fn eval_ctx_steps(steps: u64) -> EvalCtx {
    // `Cached`: a referenced cell reads as saved, which is all a themed cell's formula does (it names no cell).
    let opts = EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() };
    EvalCtx::new(opts, Budget::with_steps(steps))
}

/// The context of the cells with no saved value, with `steps` to spend: `Live`, so that a cell such a formula reads is worked out as well (a cell it reads may have no saved
/// value either).
fn unsaved_ctx(steps: u64) -> EvalCtx {
    EvalCtx::new(EvalOptions { operands: OperandMode::Live, ..EvalOptions::default() }, Budget::with_steps(steps))
}

impl<'a, 'c> Cx<'a, 'c> {
    pub(crate) fn new(doc: &'a Document, ctx: &'c mut EvalCtx) -> Cx<'a, 'c> {
        Cx {
            doc,
            res: Resolver::new(doc),
            ev: RefCell::new(Evaluator::new(doc, ctx)),
            themed: RefCell::new(HashMap::new()),
            skips: RefCell::new(Skips::default()),
            document_language: OnceCell::new(),
            own: RefCell::new(unsaved_ctx(UNSAVED_STEPS)),
            kept: None,
            allotted: UNSAVED_STEPS,
        }
    }

    /// This reading with the answers the frames of the document worked out kept in `cache`, **if the cache is for this very document** ([`WorkedCache::is_for`]): the cache
    /// of another document is not used, and a cell is then worked out as it was before there was one.
    ///
    /// The steps for the cells with no saved value are then **the document's**: this reading has no more than the cache has left, and charges the cache what it spends when
    /// it ends, so that the frames of a document spend [`MAX_STEPS_PER_DOCUMENT`] in all and not that much each.
    pub(crate) fn with_kept(mut self, cache: Option<&Arc<WorkedCache>>) -> Cx<'a, 'c> {
        self.kept = cache.filter(|c| c.is_for(self.doc)).cloned();
        let steps = self.allotted;
        self.with_unsaved_steps(steps)
    }

    /// This reading with `steps` for the cells it works out that have no saved value (a share of a parallel walk has a part of the frame's, `share.rs`), **or the cache's
    /// if it has fewer left**.
    pub(crate) fn with_unsaved_steps(mut self, steps: u64) -> Cx<'a, 'c> {
        self.allotted = self.kept.as_ref().map_or(steps, |cache| steps.min(cache.steps_left()));
        *self.own.borrow_mut() = unsaved_ctx(self.allotted);
        self
    }

    /// Whether the steps for the cells with no saved value ran out in this reading.
    pub(crate) fn unsaved_exhausted(&self) -> bool {
        self.own.borrow().stats.budget_exhausted
    }

    /// Notes one thing the frame left out or drew by a stand-in. `first` is built only for the first of its kind, and **after** the list is
    /// let go, so a `first` that reads cells (and meets a themed one, which notes) cannot find the list borrowed.
    pub(crate) fn note(&self, kind: SkipKind, first: impl FnOnce() -> String) {
        self.note_n(kind, 1, first);
    }

    /// Notes a text block drawn by `used` in place of the face `requested` that the machine lacks ([`SkipKind::FaceStandIn`]).
    pub(crate) fn note_face(&self, requested: &str, used: &str) {
        self.skips.borrow_mut().note_face(requested, used);
    }

    /// Notes a text block with more missing faces than its layout names ([`SkipKind::FaceStandIn`]).
    pub(crate) fn note_faces_unnamed(&self) {
        self.skips.borrow_mut().note_faces_unnamed();
    }

    /// Notes `n` of a kind. See [`Cx::note`].
    pub(crate) fn note_n(&self, kind: SkipKind, n: usize, first: impl FnOnce() -> String) {
        if n == 0 {
            return;
        }
        let is_first = self.skips.borrow_mut().count(kind, n);
        if is_first {
            let sample = first();
            self.skips.borrow_mut().set_first(kind, sample);
        }
    }

    /// Panics while holding one of the cells open, as a bug that did so would: the test of `fault.rs` that a caught panic frees them.
    #[cfg(test)]
    #[allow(clippy::panic)]
    pub(crate) fn panic_holding(&self, held: crate::fault::Holding) -> ! {
        use crate::fault::Holding;
        match held {
            Holding::Nothing => panic!("a test panic holding nothing"),
            Holding::Skips => {
                let _held = self.skips.borrow_mut();
                panic!("a test panic holding the list of notes")
            }
            Holding::Themed => {
                let _held = self.themed.borrow_mut();
                panic!("a test panic holding the kept theme answers")
            }
            Holding::Eval => {
                let _held = self.ev.borrow_mut();
                panic!("a test panic holding the evaluator")
            }
        }
    }

    /// What the frame noted, leaving the list empty.
    pub(crate) fn take_skips(&self) -> Skips {
        self.skips.take()
    }

    /// What the cell at `addr` of `at` holds, found by inheritance.
    pub(crate) fn val(&self, at: &SheetRef, addr: &CellAddr) -> Val {
        let Some(found) = self.res.resolve(at, addr) else {
            // A row or section deleted on the way (`Del`): nothing in it is inherited either.
            return Val::Missing;
        };
        // A formula with no value saved anywhere on the way is worked out first: it is not a cell that finds nothing, and so it does not read the root style's value.
        if found.holds_unsaved_formula() {
            return self.unsaved(at, addr, &found);
        }
        self.val_found(at, addr, &found)
    }

    /// The drawing's language, which a run with no `LangID` anywhere is in.
    pub(crate) fn document_language(&self) -> &Tag {
        self.document_language.get_or_init(|| lang::document_language(self.doc, None))
    }

    /// What the cell found holds, as the file saved it: the value of the nearest cell that has one, the root style's for a shape that finds none.
    fn val_found(&self, at: &SheetRef, addr: &CellAddr, found: &Resolved<'_>) -> Val {
        // A shape with no style link of a cell's family finds nothing for it, and reads the root style's value: "No Style" is the style
        // every object has (ASSUMED, no spec settles it: real shapes always link up to it). **The one rule** (`Resolver::root_standin`) the readers
        // of the edit commands read a formula that gives no value by as well.
        if let Some(root) = self.res.root_standin(at, addr, found) {
            return self.val_of(at, addr, &root, 0);
        }
        self.val_of(at, addr, found, 0)
    }

    /// What a cell holds that has a formula and no saved value: **the formula is worked out**, on the sheet that asked and with the cells it reads
    /// worked out as well (a cell it reads may have no saved value either), and the answer is kept per cell. A viewer draws what a file saved, and a cell
    /// with no `V` anywhere on the way saved nothing: the formula found by inheritance is all that says what it holds, and a default drawn in its place would
    /// be a value nobody wrote. Written for the `THEMEVAL()` of a style (a file that leaves the value out beside it is drawn themed, as the same formula with the
    /// marker beside it is), and the same for any formula. A result that is no value (an error, a function this evaluator lacks, a kind a drawing does not read) leaves
    /// the cell as it was read before, so nothing a cell held is lost to a formula that cannot be worked out.
    ///
    /// **The cost.** The answer is asked for, in order, of the frame's own table, of the cache of the document ([`WorkedCache`]) and of the formula;
    /// one that was worked out is put in the cache for the frames that follow, so a page is worked out once for as long as the document is the same. The formula is worked
    /// out in a context with steps of its own ([`UNSAVED_STEPS`]), and **when those run out the cell is read as it was before, the frame says so
    /// ([`SkipKind::UnsavedUnanswered`]) and nothing is kept of it**: a later frame, with steps of its own, works it out.
    fn unsaved(&self, at: &SheetRef, addr: &CellAddr, found: &Resolved<'_>) -> Val {
        let key = (at.clone(), addr.clone());
        // Each borrow ends in the statement that takes it: none is open while the next is taken, or while `val_found` runs.
        let known = self.themed.borrow().get(&key).cloned();
        if let Some(known) = known {
            return known;
        }
        if let Some(kept) = self.kept.as_ref().and_then(|cache| cache.get(&key)) {
            self.themed.borrow_mut().insert(key, kept.clone());
            return kept;
        }
        let asked = CellAddress::new(at.clone(), addr.clone());
        let answer = {
            let mut own = self.own.borrow_mut();
            self.ev.borrow_mut().evaluate_worked_out_in(&mut own, &asked)
        };
        if self.unsaved_exhausted() {
            // The steps ran out on the way: what was worked out is not the cell's answer, so none is kept (the frame reads the cell as it was, and the next works it out).
            self.note(SkipKind::UnsavedUnanswered, || format!("the cell {} of a sheet", addr.cell.as_str()));
            let val = self.val_found(at, addr, found);
            self.themed.borrow_mut().insert(key, val.clone());
            return val;
        }
        let val = match answer {
            Some(Calc::Number(q)) if q.stored.is_finite() => Val::Num(q.stored),
            Some(Calc::Bool(b)) => Val::Num(if b { 1.0 } else { 0.0 }),
            Some(Calc::Color(c)) => Val::Color(Rgba::rgb(c.r, c.g, c.b)),
            Some(Calc::Text(t)) => Val::Text(t),
            // The theme answered that it holds no value: a stop past its entry's list, as for a marker.
            Some(Calc::Error(CellError::Na)) => Val::Missing,
            _ => self.val_found(at, addr, found),
        };
        if let Some(cache) = &self.kept {
            cache.put(key.clone(), val.clone());
        }
        self.themed.borrow_mut().insert(key, val.clone());
        val
    }

    fn val_of(&self, at: &SheetRef, addr: &CellAddr, found: &Resolved<'_>, depth: u8) -> Val {
        let value = found.value.cell.and_then(|c| c.value.as_ref());
        let Some(value) = value else {
            return self.default_of(at, found, depth);
        };
        match value.reading() {
            Reading::Number(n) => Val::Num(n),
            Reading::Empty => self.default_of(at, found, depth),
            Reading::Infinite { negative } => Val::Num(if negative { -BIG } else { BIG }),
            Reading::Themed => self.themed(at, addr),
            Reading::Text => {
                let text = value.text();
                // A colour cell's `V` is `#RRGGBB` (`#` and six hex digits in
                // every colour cell of the census). `Rgba::from_hex` takes 3, 6 or 8 bare hex digits as well, so a face called `Bed` or `Decade`, or a
                // layer called `Abc`, read as a colour.
                if text.starts_with('#')
                    && let Some(rgba) = Rgba::from_hex(text)
                {
                    Val::Color(rgba)
                } else if text.eq_ignore_ascii_case("true") {
                    Val::Num(1.0)
                } else if text.eq_ignore_ascii_case("false") {
                    Val::Num(0.0)
                } else {
                    Val::Text(Arc::from(text))
                }
            }
        }
    }

    /// A cell with no value on the way: the printed formula of the few cells that read one (`Name*factor`), or missing.
    fn default_of(&self, at: &SheetRef, found: &Resolved<'_>, depth: u8) -> Val {
        let Some(formula) = found.default.formula else {
            return Val::Missing;
        };
        // The catalogue's formulas are a cell and a factor; they name each other three deep at most. The depth bound is for a catalogue that
        // changes.
        if depth > 4 {
            return Val::Missing;
        }
        let Some((name, factor)) = formula.split_once('*') else {
            return Val::Missing;
        };
        let Ok(factor) = factor.trim().parse::<f64>() else {
            return Val::Missing;
        };
        let addr = CellAddr::direct(name.trim());
        let Some(base) = self.res.resolve(at, &addr) else {
            return Val::Missing;
        };
        let read = if base.holds_unsaved_formula() { self.unsaved(at, &addr, &base) } else { self.val_of(at, &addr, &base, depth + 1) };
        match read {
            Val::Num(n) => Val::Num(n * factor),
            _ => Val::Missing,
        }
    }

    /// The theme's answer for a cell whose value is the marker. Kept per cell.
    fn themed(&self, at: &SheetRef, addr: &CellAddr) -> Val {
        let key = (at.clone(), addr.clone());
        // Each borrow ends in the statement that takes it: none is open while the next is taken, or while `note` runs.
        let known = self.themed.borrow().get(&key).cloned();
        if let Some(known) = known {
            return known;
        }
        let asked = CellAddress::new(at.clone(), addr.clone());
        let answer = self.ev.borrow_mut().evaluate(&asked);
        let val = match answer {
            Calc::Number(q) if q.stored.is_finite() => Val::Num(q.stored),
            Calc::Bool(b) => Val::Num(if b { 1.0 } else { 0.0 }),
            Calc::Color(c) => Val::Color(Rgba::rgb(c.r, c.g, c.b)),
            Calc::Text(t) => Val::Text(t),
            // The theme answered that it holds no value: a stop past its entry's list. The row has no
            // colour, which the stop reader drops, and nothing is left unanswered.
            Calc::Error(CellError::Na) => Val::Missing,
            _ => {
                self.note(SkipKind::ThemeUnanswered, || format!("the cell {} of a sheet", addr.cell.as_str()));
                self.theme_fallback(addr)
            }
        };
        self.themed.borrow_mut().insert(key, val.clone());
        val
    }

    /// What a marker reads as when the theme cannot answer:
    /// the root style's cell of the same name, for every cell, the colour cells included. A marker stands for a
    /// `THEMEVAL` call that names no property, and with no theme to answer that call gives the root's cell;
    /// MS-VSDX's no-theme column of colours is for a call that names a property, which `calc` answers when
    /// it runs the cell's formula, and which is never read from here. `calc` answers where it can: where no theme is selected (the root's cell
    /// too), and where the file's theme part holds the entry. What is left here is a file with scheme cells above 0 and no theme part at all,
    /// a part or entry the reader could not read, a custom scheme (65535), and the properties that are not worked out (bevel, glow and the like).
    fn theme_fallback(&self, addr: &CellAddr) -> Val {
        // The text colour of any `Character` row reads the root's first row, as `calc` does (its `root_cell` for the text colour).
        let in_character = addr.section.as_ref().is_some_and(|s| s.name.as_str() == "Character");
        let root_row;
        let addr = if in_character && addr.cell.as_str() == "Color" {
            root_row = CellAddr::in_row("Character", None, RowKey::Ix(0), "Color");
            &root_row
        } else {
            addr
        };
        let Some(root) = self.res.resolve(&SheetRef::Style(0), addr) else {
            return Val::Missing;
        };
        let Some(value) = root.value.cell.and_then(|c| c.value.as_ref()) else {
            return Val::Missing;
        };
        match value.reading() {
            Reading::Number(n) => Val::Num(n),
            // A colour, `#RRGGBB`: a root cell that is a face name, and that happens to be made of hex digits, is not one.
            Reading::Text if value.text().starts_with('#') => Rgba::from_hex(value.text()).map_or(Val::Missing, Val::Color),
            _ => Val::Missing,
        }
    }

    // ---- typed readings ----------------------------------------------------------------------------------------------------

    /// The number a cell directly under the sheet holds, 0 when missing.
    pub(crate) fn num(&self, at: &SheetRef, name: &str) -> f64 {
        self.num_or(at, name, 0.0)
    }

    /// As [`Cx::num`], with `default` for a missing cell or one that is not a number.
    pub(crate) fn num_or(&self, at: &SheetRef, name: &str, default: f64) -> f64 {
        self.num_at(at, &CellAddr::direct(name), default)
    }

    pub(crate) fn num_at(&self, at: &SheetRef, addr: &CellAddr, default: f64) -> f64 {
        match self.val(at, addr) {
            Val::Num(n) if n.is_finite() => n,
            Val::Num(_) | Val::Missing | Val::Color(_) | Val::Text(_) => default,
        }
    }

    /// A flag: any value but 0 is set, truth. A missing cell is `default`.
    pub(crate) fn flag(&self, at: &SheetRef, name: &str, default: bool) -> bool {
        self.flag_at(at, &CellAddr::direct(name), default)
    }

    pub(crate) fn flag_at(&self, at: &SheetRef, addr: &CellAddr, default: bool) -> bool {
        match self.val(at, addr) {
            Val::Num(n) => n != 0.0,
            _ => default,
        }
    }

    pub(crate) fn text_at(&self, at: &SheetRef, addr: &CellAddr) -> Option<Arc<str>> {
        match self.val(at, addr) {
            Val::Text(t) => Some(t),
            Val::Num(n) => Some(Arc::from(n.to_string())),
            Val::Color(c) => Some(Arc::from(c.hex())),
            Val::Missing => None,
        }
    }

    /// A colour cell: `#RRGGBB`, or an index of the fixed palette or the document's colour table.
    /// `None` when the cell holds nothing that reads as a colour.
    pub(crate) fn colour(&self, at: &SheetRef, name: &str) -> Option<Rgba> {
        self.colour_at(at, &CellAddr::direct(name))
    }

    pub(crate) fn colour_at(&self, at: &SheetRef, addr: &CellAddr) -> Option<Rgba> {
        match self.val(at, addr) {
            Val::Color(c) => Some(c),
            Val::Num(n) => Some(self.indexed_colour(n)),
            Val::Text(_) | Val::Missing => None,
        }
    }

    /// The colour of the document's table at index `n`. An index that names nothing is colour 0.
    pub(crate) fn indexed_colour(&self, n: f64) -> Rgba {
        self.doc.color(to_index(n)).or_else(|| self.doc.color(0)).unwrap_or(Rgba::BLACK)
    }
}

/// A colour index from a number: whole numbers only, anything else is 0.
fn to_index(n: f64) -> i64 {
    if n.is_finite() && (0.0..1.0e9).contains(&n) { n.round() as i64 } else { 0 }
}

/// The sheet of a shape.
pub(crate) fn sheet_of(shape: &ShapeRef) -> SheetRef {
    SheetRef::Shape(shape.clone())
}

/// What the cell at `addr` of `sheet` reads as in a frame's picture, read as the renderer reads a cell it draws from: its saved value, found by inheritance, and for a cell that holds a
/// formula and no value saved anywhere on the way what the formula works out to, **with the steps of a frame** ([`UNSAVED_STEPS`]). A caller that reads cells outside a frame uses it to
/// read one the way the picture does (the tests that hold the readers of the edit commands to the picture do), where it needs no table of answers kept from one cell to the next: each call
/// starts from nothing.
pub fn read_cell(doc: &Document, sheet: &SheetRef, addr: &CellAddr) -> Val {
    read_cell_within(doc, sheet, addr, UNSAVED_STEPS)
}

/// [`read_cell`] with `steps` for the cells that hold a formula and no saved value: past them the cell reads as it did before there were any (a formula that is not worked out reads as a
/// cell that holds none, the root style's value for a shape's cell that it holds: `Resolver::root_standin`).
pub fn read_cell_within(doc: &Document, sheet: &SheetRef, addr: &CellAddr, steps: u64) -> Val {
    let mut ctx = eval_ctx();
    Cx::new(doc, &mut ctx).with_unsaved_steps(steps).val(sheet, addr)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use diacraft_doc::{Cell, Owner};
    use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

    use super::*;

    /// `n` shapes that each hold `RGB(1,2,3)` as a formula and no value in their fill.
    fn unsaved_fills(n: u32) -> Document {
        let mut page = PageBuilder::new();
        for id in 1..=n {
            page = page.shape(ShapeBuilder::new(id).set(CellAddr::direct("FillForegnd"), Cell::with_formula("RGB(1,2,3)", None)));
        }
        DocBuilder::new().page(1, page).build()
    }

    fn fill_of(at: u32) -> (SheetRef, CellAddr) {
        (SheetRef::Shape(ShapeRef::top(Owner::Page(1), at)), CellAddr::direct("FillForegnd"))
    }

    #[test]
    fn the_steps_for_cells_with_no_saved_value_running_out_leave_the_rest_unanswered_and_keep_nothing_of_them() {
        let doc = Arc::new(unsaved_fills(4));
        // What one cell costs, counted.
        let one = {
            let mut ctx = eval_ctx();
            let cx = Cx::new(&doc, &mut ctx);
            let (sheet, addr) = fill_of(0);
            assert_eq!(cx.colour_at(&sheet, &addr), Some(Rgba::rgb(1, 2, 3)));
            cx.own.borrow().stats.steps
        };
        assert!(one > 0, "a cell takes steps");
        // Steps for two cells and a half: the first two are answered and kept, the third runs out on the way, and so does the fourth, at once.
        let cache = WorkedCache::new(Arc::clone(&doc));
        let mut ctx = eval_ctx();
        let cx = Cx::new(&doc, &mut ctx).with_kept(Some(&cache)).with_unsaved_steps(one * 2 + one / 2);
        let read: Vec<Option<Rgba>> = (0..4)
            .map(|at| {
                let (sheet, addr) = fill_of(at);
                cx.colour_at(&sheet, &addr)
            })
            .collect();
        assert_eq!(read[..2], [Some(Rgba::rgb(1, 2, 3)), Some(Rgba::rgb(1, 2, 3))]);
        assert_eq!(read[2..], [None, None], "the cells past the steps are read as they were before: no value, and so no colour");
        assert!(cx.unsaved_exhausted());
        assert_eq!(cx.take_skips().into_list().iter().filter(|s| s.kind == SkipKind::UnsavedUnanswered).map(|s| s.count).sum::<usize>(), 2);
        assert_eq!(cache.answers(), 2, "only what was worked out is kept");
        // A frame of the same document with steps of its own finds those two in the cache and works the other two out.
        let mut ctx = eval_ctx();
        let cx = Cx::new(&doc, &mut ctx).with_kept(Some(&cache)).with_unsaved_steps(one * 2 + one / 2);
        for at in 0..4 {
            let (sheet, addr) = fill_of(at);
            assert_eq!(cx.colour_at(&sheet, &addr), Some(Rgba::rgb(1, 2, 3)), "cell {at}");
        }
        assert!(!cx.unsaved_exhausted());
        assert_eq!(cache.answers(), 4);
    }

    #[test]
    fn a_formula_that_gives_no_value_reads_as_a_cell_that_holds_none_which_is_the_root_styles_value_for_a_shape_and_the_answer_when_it_works_out() {
        // The one rule, `Resolver::root_standin`, which the commands' reader (`Worked`) reads a failed formula by as well.
        let weight = CellAddr::direct("LineWeight");
        let doc_of = |formula: &str| {
            let shape = ShapeBuilder::new(1).set(weight.clone(), Cell::with_formula(formula, None));
            DocBuilder::new().root(|r| r.cell("LineWeight", "0.0625")).page(1, PageBuilder::new().shape(shape)).build()
        };
        let sheet = SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0));
        assert_eq!(read_cell(&doc_of("1/0"), &sheet, &weight), Val::Num(0.0625), "an error");
        assert_eq!(read_cell(&doc_of("NOSUCHFUNCTION(1)"), &sheet, &weight), Val::Num(0.0625), "a function the evaluator lacks");
        assert_eq!(read_cell(&doc_of("0.5+0.5"), &sheet, &weight), Val::Num(1.0), "a formula that works out is its answer");
        assert_eq!(read_cell_within(&doc_of("0.5+0.5"), &sheet, &weight, 0), Val::Num(0.0625), "steps that ran out read by the same rule");
    }

    #[test]
    fn a_cache_of_another_document_is_not_used() {
        let (doc, other) = (Arc::new(unsaved_fills(2)), Arc::new(unsaved_fills(2)));
        let cache = WorkedCache::new(Arc::clone(&other));
        let mut ctx = eval_ctx();
        let cx = Cx::new(&doc, &mut ctx).with_kept(Some(&cache));
        let (sheet, addr) = fill_of(0);
        assert_eq!(cx.colour_at(&sheet, &addr), Some(Rgba::rgb(1, 2, 3)));
        assert_eq!(cache.answers(), 0, "the cache of the other document was neither read nor written");
    }
}
