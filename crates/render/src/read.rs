//! Reading cells to draw them: the saved value, found by inheritance, and nothing recalculated.
//!
//! **Draw what was saved**: a cell's `V` stands, found by through the
//! document's [`Resolver`]. Two things need more than the saved text, and both are narrow.
//!
//! * The seven `Text Transform` cells and the two picture size cells that a file leaves out read a printed formula,
//!   `Width*0.5` and the like. [`Cx::num`] works those out itself: they are a cell's name and a factor.
//! * A value that is the marker `Themed` has no number to read: about a third of page shapes
//!   reach it for `FillPattern`. It is asked of the theme, by `calc`'s evaluator running the cell's own formula (`THEMEVAL()`) on the
//!   sheet that asked, and the answer is kept. Nothing else is evaluated. When the theme cannot answer, the cell reads as missing and the
//!   caller's default stands, and the frame says so.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use diacraft_calc::{Budget, CellAddress, CellError, EvalCtx, EvalOptions, Evaluator, OperandMode, Value as Calc};
use diacraft_color::Rgba;
use diacraft_doc::{CellAddr, Document, Reading, Resolved, Resolver, RowKey, ShapeRef, SheetRef};

use crate::skips::{SkipKind, Skips};

/// The steps `calc` may spend on themed cells in one frame. A `THEMEVAL()` is a handful of steps, and the answers are kept.
pub(crate) const THEME_STEPS: u64 = 4_000_000;

/// What a cell holds, read for drawing.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Val {
    /// No value anywhere on the way: the caller's default applies.
    Missing,
    Num(f64),
    Color(Rgba),
    Text(Arc<str>),
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

impl<'a, 'c> Cx<'a, 'c> {
    pub(crate) fn new(doc: &'a Document, ctx: &'c mut EvalCtx) -> Cx<'a, 'c> {
        Cx {
            doc,
            res: Resolver::new(doc),
            ev: RefCell::new(Evaluator::new(doc, ctx)),
            themed: RefCell::new(HashMap::new()),
            skips: RefCell::new(Skips::default()),
        }
    }

    /// Notes one thing the frame left out or drew by a stand-in. `first` is built only for the first of its kind, and **after** the list is
    /// let go, so a `first` that reads cells (and meets a themed one, which notes) cannot find the list borrowed.
    pub(crate) fn note(&self, kind: SkipKind, first: impl FnOnce() -> String) {
        self.note_n(kind, 1, first);
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
        // A shape with no style link of a cell's family finds nothing for it, and reads the root style's value: "No Style" is the style
        // every object has (ASSUMED, no spec settles it: real shapes always link up to it).
        if found.value.cell.is_none()
            && matches!(at, SheetRef::Shape(_))
            && let Some(root) = self.res.resolve(&SheetRef::Style(0), addr)
            && root.value.cell.is_some()
        {
            return self.val_of(at, addr, &root, 0);
        }
        self.val_of(at, addr, &found, 0)
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
        match self.val_of(at, &addr, &base, depth + 1) {
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
