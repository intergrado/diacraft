//! **Random sequences of ShapeSheet commands, checked against a model** (design 11, "Property tests").
//!
//! A sequence is made of the commands of the ShapeSheet (`sheet.add_section`, `sheet.add_row`, `sheet.set_cell`, `sheet.set_cells`, `sheet.delete_row`, `sheet.delete_section`) on every kind of
//! sheet a drawing holds (a shape of a page, a page, a master's shape, a master's page, a style and the document), with undo, redo and a save in each of two formats between them, and
//! junk the commands must refuse. The drawing is generated too (one or two pages, one or two shapes on each, an instance of a master that reads a style or not).
//!
//! **What is checked** (each failure says the sequence, so that it repeats):
//!
//! 1. **A refusal changes nothing**: the drawing, the history and the revision are as they were; no command ends in an internal error.
//! 2. **A command that changed something is one undo step**, and one that changed nothing is none (the drawing, the history and the revision stay).
//! 3. **The model**: after each command the sections each sheet holds, the rows of each in order, the text of each cell a write made and the value of each constant, and of each cell that names
//!    a row of Scratch (**the formula follows its row** whatever the rows around it do, R29, and one whose row went keeps the value it had) are what the model says. The model knows the
//!    refusals the rules predict: a section there once, a row that exists, a name that is taken, a row of a master or a style that would move the rows that others read.
//! 4. **The drawing is settled**: a full recalculation (`recalc_all`, on the cached operands) finds no cell whose saved value differs from what its formula gives. An edit worked out every cell it
//!    reached, whichever sheet it was made on.
//! 5. **Undo and redo are exact**: each step back is the drawing (and the model) of the step before, each step forward the one after, at the end of the sequence and at any point in it.
//! 6. **Every writer saves it**: a `.vsdx` and a `.diacraft` file are written, read again, and each modelled sheet reads the same (`sheet.read`), the `.diacraft` drawing equal whole; the session
//!    may go on from the file it read.
//! 7. **Every `ref` that `sheet.read` gives maps back** to its cell: `sheet.cell` of it is that cell.
//!
//! The generator is proptest's, on a fixed seed so that a failure repeats; `cases` is explicit, since an environment variable never beats a `ProptestConfig` that names it. `chains_keep_the_model`
//! runs in nextest; `a_long_sweep_of_sequences` is the local sweep (`#[ignore]`), which runs many more (`DIACRAFT_SHEET_CASES`, `DIACRAFT_SHEET_SEED`).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_calc::{Budget, recalc_all};
use diacraft_doc::Document;
use diacraft_engine::{EngineError, Session};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};
use proptest::prelude::*;
use proptest::test_runner::{Config, RngAlgorithm, TestCaseError, TestRng, TestRunner};
use serde_json::{Value, json};

use crate::blank_session;
use crate::native_common::Dir;

// ---- the sheets of a generated drawing ---------------------------------------------------------------------------------------------------

/// A sheet of the drawing, as a command names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Which {
    Shape { page: u32, id: u32 },
    Page(u32),
    MasterShape,
    MasterPage,
    Style,
    Document,
}

impl Which {
    /// The keys that name the sheet to a command.
    fn keys(self) -> Value {
        match self {
            Which::Shape { page, id } => json!({"id": id, "page": page}),
            Which::Page(page) => json!({"page": page}),
            Which::MasterShape => json!({"master": 1, "id": 5}),
            Which::MasterPage => json!({"master": 1}),
            Which::Style => json!({"style": 1}),
            Which::Document => json!({"document": true}),
        }
    }

    /// The keys of a call on this sheet, with `extra` beside them.
    fn with(self, extra: Value) -> Value {
        let mut params = self.keys();
        if let (Some(map), Some(more)) = (params.as_object_mut(), extra.as_object()) {
            map.extend(more.clone());
        }
        params
    }

    /// Whether the rows of its sections are numbered by the sheets that read it: a master's own rows and a style's keep their numbers, and an insert that would move them is refused.
    fn given(self) -> bool {
        matches!(self, Which::MasterShape | Which::MasterPage | Which::Style)
    }

    /// How a formula on `from` names a cell of this sheet (`Sheet.11!`, `ThePage!`, `TheDoc!`, nothing for its own); none when the model does not write such a formula.
    fn named_from(self, from: Which) -> Option<String> {
        if self == from {
            return Some(String::new());
        }
        match (from, self) {
            (Which::Shape { page: a, .. }, Which::Shape { page: b, id }) if a == b => Some(format!("Sheet.{id}!")),
            (Which::Shape { page: a, .. }, Which::Page(b)) if a == b => Some("ThePage!".to_owned()),
            (Which::Shape { .. } | Which::Page(_), Which::Document) => Some("TheDoc!".to_owned()),
            _ => None,
        }
    }
}

/// The drawing a sequence starts from.
#[derive(Clone, Copy, Debug)]
struct Base {
    pages: u32,
    shapes: u32,
    /// An instance of the master, shape 20 on each page, that reads the style: what the master's and the style's edits reach.
    instance: bool,
}

impl Base {
    /// The sheets the model follows, in a fixed order.
    fn sheets(self) -> Vec<Which> {
        let mut all = Vec::new();
        for page in 1..=self.pages {
            all.push(Which::Page(page));
            for k in 0..self.shapes {
                all.push(Which::Shape { page, id: 10 + k });
            }
        }
        all.extend([Which::MasterShape, Which::MasterPage, Which::Style, Which::Document]);
        all
    }

    fn document(self) -> Document {
        let master = MasterBuilder::new().name("Box").shape(ShapeBuilder::new(5).cell_u("Width", "1", "IN").cell_u("Height", "1", "IN"));
        let mut doc = DocBuilder::new().master(1, master).style(1, StyleBuilder::new().name("S1"));
        for page in 1..=self.pages {
            let mut built = PageBuilder::new().name(&format!("Page-{page}"));
            for k in 0..self.shapes {
                built = built.shape(ShapeBuilder::new(10 + k).cell_u("Width", "2", "IN").cell_u("Height", "1", "IN"));
            }
            if self.instance {
                built = built.shape(ShapeBuilder::new(20).master(1).links(Some(1), Some(1), Some(1)));
            }
            doc = doc.page(page, built);
        }
        let mut doc = doc.build();
        // A drawing DiaCraft makes states the text cells its renderer assumes in its root style (`root_text`), and a Save As to a VSDX type writes a root that lacks them with the ones a reader
        // would read otherwise: the file, and so the sheet of a page after it is opened, would then hold rows the drawing did not.
        if let Some(at) = doc.styles.iter().position(|style| style.id == 0)
            && let Some(done) = doc.styles.get(at).and_then(|root| diacraft_doc::root_text::complete(root, diacraft_doc::root_text::Scope::Whole))
        {
            doc.styles.set(at, std::sync::Arc::new(done)).expect("the place is in the list");
        }
        doc
    }
}

// ---- the model -----------------------------------------------------------------------------------------------------------------------------

/// What the model knows of the `Value` cell of a row of User-defined Cells.
#[derive(Clone, Debug, PartialEq)]
enum Txt {
    /// A number typed alone (or `0`, as a row is made): a value with no formula, whose text is the number.
    Const(i64),
    /// A formula over other rows of User, as typed: its text is kept as it was typed, and the value is the evaluator's (a full recalculation checks it).
    Formula(String),
    /// A formula that names a row of Scratch by the tag the row holds: its text is spelled by the engine (the number moves with the row), its value is the tag, always.
    Reader(i64),
    /// An empty text was written: the cell reads `No Formula` and keeps the value it had, when the model knows it.
    Cleared(Option<i64>),
    /// A formula whose reference to a section that went the engine wrote as `REF()` (the formulas that name a section's rows read `#REF!` when it is deleted, named rows too): its text is
    /// whatever the engine wrote, and its value the one it had.
    Free,
}

/// A row of User-defined Cells.
#[derive(Clone, Debug, PartialEq)]
struct URow {
    name: String,
    txt: Txt,
    /// The rows of User the formula reads, by sheet and name, for the test that a formula does not read itself.
    reads: Vec<(Which, String)>,
}

/// The sections the model follows on one sheet; none when the sheet does not hold the section. A Scratch row holds its tag, or none while it is made and not yet tagged.
#[derive(Clone, Debug, Default, PartialEq)]
struct Held {
    user: Option<Vec<URow>>,
    scratch: Option<Vec<Option<i64>>>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Model {
    sheets: BTreeMap<Which, Held>,
    /// The next tag: a number no cell of the drawing holds.
    tags: i64,
}

impl Model {
    fn held(&self, w: Which) -> Held {
        self.sheets.get(&w).cloned().unwrap_or_default()
    }

    fn held_mut(&mut self, w: Which) -> &mut Held {
        self.sheets.entry(w).or_default()
    }

    fn user(&self, w: Which) -> &[URow] {
        self.sheets.get(&w).and_then(|h| h.user.as_deref()).unwrap_or(&[])
    }

    fn scratch(&self, w: Which) -> &[Option<i64>] {
        self.sheets.get(&w).and_then(|h| h.scratch.as_deref()).unwrap_or(&[])
    }

    fn fresh_tag(&mut self) -> i64 {
        self.tags += 1;
        1_000 + self.tags
    }

    fn row(&self, w: Which, name: &str) -> Option<&URow> {
        self.user(w).iter().find(|r| r.name.eq_ignore_ascii_case(name))
    }

    /// A section of User that goes: every formula that names a row of it is written again with `REF()` where the reference was.
    fn free_readers_of(&mut self, gone: Which) -> usize {
        let mut freed = 0;
        for held in self.sheets.values_mut() {
            for row in held.user.iter_mut().flatten() {
                if matches!(row.txt, Txt::Formula(_)) && row.reads.iter().any(|(w, _)| *w == gone) {
                    row.txt = Txt::Free;
                    freed += 1;
                }
            }
        }
        freed
    }

    /// Whether the cell `from` reads, through the rows it names and the rows they name, the cell `to`.
    fn reaches(&self, from: &(Which, String), to: &(Which, String)) -> bool {
        let mut seen: BTreeSet<(Which, String)> = BTreeSet::new();
        let mut stack = vec![(from.0, from.1.to_ascii_lowercase())];
        while let Some((w, name)) = stack.pop() {
            if (w, name.clone()) == (to.0, to.1.to_ascii_lowercase()) {
                return true;
            }
            if !seen.insert((w, name.clone())) {
                continue;
            }
            if let Some(row) = self.row(w, &name) {
                stack.extend(row.reads.iter().map(|(rw, rn)| (*rw, rn.to_ascii_lowercase())));
            }
        }
        false
    }
}

// ---- the sequences -------------------------------------------------------------------------------------------------------------------------

/// What a write puts in a cell, as a number picks it.
#[derive(Clone, Debug)]
enum Form {
    Const(i8),
    Clear,
    /// A formula over a row of User of a sheet the cell can name: the row alone, or plus or times a number.
    Cell {
        sheet: u8,
        row: u8,
        times: bool,
        by: u8,
        alone: bool,
    },
    /// A formula that names a row of Scratch.
    Tag {
        sheet: u8,
        row: u8,
    },
}

/// One step of a sequence. The numbers pick a sheet, a row, a place; the harness reads them against what the model holds, so that most steps are meaningful and some are refused.
#[derive(Clone, Debug)]
enum Op {
    AddSection {
        sheet: u8,
        scratch: bool,
    },
    DeleteSection {
        sheet: u8,
        scratch: bool,
    },
    AddRow {
        sheet: u8,
        scratch: bool,
        place: u8,
        name: u8,
        lower: bool,
        named: bool,
    },
    DeleteRow {
        sheet: u8,
        scratch: bool,
        row: u8,
    },
    Set {
        sheet: u8,
        row: u8,
        form: Form,
    },
    Batch {
        writes: Vec<(u8, u8, Form)>,
        bad_last: bool,
    },
    /// A step on the instance, whose sheet the model does not follow: it must keep the other rules.
    Instance {
        kind: u8,
    },
    Junk {
        kind: u8,
        sheet: u8,
    },
    Undo,
    Redo,
    Save {
        vsdx: bool,
        adopt: bool,
    },
}

fn form() -> impl Strategy<Value = Form> {
    prop_oneof![
        4 => any::<i8>().prop_map(Form::Const),
        1 => Just(Form::Clear),
        3 => (any::<u8>(), any::<u8>(), any::<bool>(), any::<u8>(), any::<bool>())
            .prop_map(|(sheet, row, times, by, alone)| Form::Cell { sheet, row, times, by, alone }),
        3 => (any::<u8>(), any::<u8>()).prop_map(|(sheet, row)| Form::Tag { sheet, row }),
    ]
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (any::<u8>(), any::<bool>()).prop_map(|(sheet, scratch)| Op::AddSection { sheet, scratch }),
        2 => (any::<u8>(), any::<bool>()).prop_map(|(sheet, scratch)| Op::DeleteSection { sheet, scratch }),
        8 => (any::<u8>(), any::<bool>(), any::<u8>(), any::<u8>(), any::<bool>(), any::<bool>())
            .prop_map(|(sheet, scratch, place, name, lower, named)| Op::AddRow { sheet, scratch, place, name, lower, named }),
        3 => (any::<u8>(), any::<bool>(), any::<u8>()).prop_map(|(sheet, scratch, row)| Op::DeleteRow { sheet, scratch, row }),
        12 => (any::<u8>(), any::<u8>(), form()).prop_map(|(sheet, row, form)| Op::Set { sheet, row, form }),
        3 => (prop::collection::vec((any::<u8>(), any::<u8>(), form()), 1..6), any::<bool>()).prop_map(|(writes, bad_last)| Op::Batch { writes, bad_last }),
        1 => any::<u8>().prop_map(|kind| Op::Instance { kind }),
        2 => (any::<u8>(), any::<u8>()).prop_map(|(kind, sheet)| Op::Junk { kind, sheet }),
        2 => Just(Op::Undo),
        1 => Just(Op::Redo),
        1 => (any::<bool>(), any::<bool>()).prop_map(|(vsdx, adopt)| Op::Save { vsdx, adopt }),
    ]
}

fn base() -> impl Strategy<Value = Base> {
    (1_u32..=2, 1_u32..=2, any::<bool>()).prop_map(|(pages, shapes, instance)| Base { pages, shapes, instance })
}

/// Names a row of User may be given: the same name in two letter cases is one name.
const NAMES: [&str; 8] = ["Alpha", "Beta", "Gamma", "Delta", "Eps", "Zeta", "Eta", "Theta"];

/// Texts that are no formula, in any cell.
const BAD_TEXT: [&str; 7] = ["1+", "(", "((1)", "\"abc", ")", "User.A.Value+", "1,"];

/// Names that are no cell a command takes.
const BAD_CELL: [&str; 9] = ["", "Nope", "User.A", "Sheet.4!Width", "=Width", "User..Value", "Scratch.A0", "Width.Width", "Geometry1.X1.Y"];

// ---- what a run reached --------------------------------------------------------------------------------------------------------------------

/// What the sequences did, counted, so that the test is shown to have reached what it claims to check: a generator that never wrote a formula that names a row of Scratch would pass for ever.
#[derive(Clone, Copy)]
enum Event {
    SectionAdded,
    SectionDeleted,
    RowAdded,
    RowDeleted,
    Const,
    Formula,
    Reader,
    Cleared,
    Batch,
    MiddleRefused,
    FormulasFreed,
    Undo,
    Redo,
    SavedVsdx,
    SavedDiacraft,
    Adopted,
    InstanceStep,
    JunkRefused,
}

const EVENTS: [&str; 18] = [
    "sections added",
    "sections deleted",
    "rows added",
    "rows deleted",
    "constants written",
    "formulas over User written",
    "formulas that name a row of Scratch written",
    "cells cleared",
    "batches made",
    "inserts in the middle of a master's or a style's rows refused",
    "formulas written again with REF() by a section that went",
    "undos",
    "redos",
    ".vsdx files saved and read",
    ".diacraft files saved and read",
    "sessions that went on from the file they read",
    "steps on the instance that made an undo step",
    "junk inputs refused",
];

static COUNTS: [AtomicUsize; 18] = [const { AtomicUsize::new(0) }; 18];

fn note(event: Event) {
    COUNTS[event as usize].fetch_add(1, Ordering::Relaxed);
}

fn count(event: Event) -> usize {
    COUNTS[event as usize].load(Ordering::Relaxed)
}

/// The counts, one to a line.
fn tally() -> String {
    EVENTS.iter().zip(&COUNTS).map(|(name, n)| format!("{:>8}  {name}", n.load(Ordering::Relaxed))).collect::<Vec<_>>().join("\n")
}

// ---- the harness ---------------------------------------------------------------------------------------------------------------------------

/// What a write is, once planned: its text, what the model keeps of it, and the rows of User it reads.
type Planned = (String, Txt, Vec<(Which, String)>);

/// What a command may change, taken before and after: a refusal, and a call that changed nothing, must leave all of it as it was.
#[derive(Clone, Debug, PartialEq)]
struct State {
    doc: Option<Document>,
    undo: usize,
    redo: usize,
    revision: u64,
}

/// What a command answered, and whether it made an undo step.
struct Called {
    answer: Result<Value, EngineError>,
    stepped: bool,
}

struct Run {
    s: Session,
    base: Base,
    sheets: Vec<Which>,
    model: Model,
    /// The drawing and the model after each step the session made, and where the session is among them: undo and redo move along it.
    trail: Vec<(Document, Model)>,
    at: usize,
    /// The undo steps the session held when the trail began: the build of the drawing, or none for a drawing read from a file.
    undo_base: usize,
    dir: Dir,
    saved: usize,
    log: Vec<String>,
}

fn session_of(doc: Document) -> Session {
    let mut s = blank_session();
    s.edit("build", |d| {
        *d = doc;
        Ok(())
    })
    .unwrap();
    s
}

/// The key of a row as a reference spells it.
fn key_text(key: &Value) -> String {
    key.as_str().map_or_else(|| key.to_string(), str::to_owned)
}

/// The rows a `sheet.read` lists for `section`: each row's key and its cells by name.
fn rows_in(read: &Value, section: &str) -> Vec<(Value, BTreeMap<String, Value>)> {
    read["lines"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|l| l["kind"] == "row" && l["section"] == section)
        .map(|l| {
            let cells = l["cells"].as_array().into_iter().flatten().filter_map(|c| Some((c["name"].as_str()?.to_owned(), c.clone()))).collect();
            (l["row"]["key"].clone(), cells)
        })
        .collect()
}

impl Run {
    fn new(base: Base) -> Run {
        let doc = base.document();
        let model = Model::default();
        let s = session_of(doc.clone());
        let undo_base = s.history().map_or(0, |h| h.undo_len());
        Run {
            s,
            base,
            sheets: base.sheets(),
            model: model.clone(),
            trail: vec![(doc, model)],
            at: 0,
            undo_base,
            dir: Dir::new(),
            saved: 0,
            log: Vec::new(),
        }
    }

    fn state(&self) -> State {
        let history = self.s.history();
        State {
            doc: self.s.document().cloned(),
            undo: history.map_or(0, |h| h.undo_len()),
            redo: history.map_or(0, |h| h.redo_len()),
            revision: self.s.revision(),
        }
    }

    /// Runs a command and checks what every command keeps: no internal error, a refusal changes nothing, a call that made no step changed nothing, and a step is one.
    fn call(&mut self, id: &str, params: Value) -> Result<Called, String> {
        let before = self.state();
        let answer = self.s.execute(id, &params);
        if let Err(e @ EngineError::Internal { .. }) = &answer {
            return Err(format!("{id} {params}: {e}"));
        }
        let after = self.state();
        let stepped = after.undo > before.undo;
        match &answer {
            Err(e) if after != before => Err(format!("{id} {params} was refused ({e}) and changed the drawing, the history or the revision")),
            Ok(_) if !stepped && after != before => Err(format!("{id} {params} made no step and changed the drawing, the history or the revision")),
            Ok(_) if after.undo > before.undo + 1 => Err(format!("{id} {params} made {} steps", after.undo - before.undo)),
            _ => Ok(Called { answer, stepped }),
        }
    }

    /// Notes a step in the trail, with the model as it is now.
    fn commit(&mut self, called: &Called) {
        if called.stepped {
            self.trail.truncate(self.at + 1);
            if let Some(doc) = self.s.document() {
                self.trail.push((doc.clone(), self.model.clone()));
                self.at += 1;
            }
        }
    }

    fn query(&mut self, id: &str, params: Value) -> Result<Value, String> {
        self.s.execute(id, &params).map_err(|e| format!("{id} {params}: {e}"))
    }

    /// The keys of the rows of `section` on `w`, as the engine spells them, in order.
    fn keys_of(&mut self, w: Which, section: &str) -> Result<Vec<Value>, String> {
        let read = self.query("sheet.read", w.with(json!({"sections": [section], "count": 2000})))?;
        Ok(rows_in(&read, section).into_iter().map(|(k, _)| k).collect())
    }

    // ---- the steps -----------------------------------------------------------------------------------------------------------------------

    fn exec(&mut self, op: &Op) -> Result<(), String> {
        match op {
            Op::AddSection { sheet, scratch } => {
                // Most sections go on a sheet that holds one already, so that sheets read each other's rows.
                let w = self.sheet_with(*sheet, |h| h.user.is_some() || h.scratch.is_some());
                self.add_section(w, *scratch)
            }
            Op::DeleteSection { sheet, scratch } => {
                let scratch = *scratch;
                let w = self.sheet_with(*sheet, |h| if scratch { h.scratch.is_some() } else { h.user.is_some() });
                self.delete_section(w, scratch)
            }
            Op::AddRow { sheet, scratch, place, name, lower, named } => {
                let name = (*named && !*scratch).then(|| {
                    let n = NAMES[usize::from(*name) % NAMES.len()];
                    if *lower { n.to_ascii_lowercase() } else { n.to_owned() }
                });
                let scratch = *scratch;
                let w = self.sheet_with(*sheet, |h| if scratch { h.scratch.is_some() } else { h.user.is_some() });
                self.add_row(w, scratch, *place, name)
            }
            Op::DeleteRow { sheet, scratch, row } => {
                let scratch = *scratch;
                let w = self.sheet_with(*sheet, |h| {
                    if scratch { h.scratch.as_ref().is_some_and(|r| !r.is_empty()) } else { h.user.as_ref().is_some_and(|r| !r.is_empty()) }
                });
                self.delete_row(w, scratch, *row)
            }
            Op::Set { sheet, row, form } => self.set(*sheet, *row, form),
            Op::Batch { writes, bad_last } => self.batch(writes, *bad_last),
            Op::Instance { kind } => self.instance(*kind),
            Op::Junk { kind, sheet } => self.junk(*kind, self.sheet(*sheet)),
            Op::Undo => self.walk(false),
            Op::Redo => self.walk(true),
            Op::Save { vsdx, adopt } => self.save(*vsdx, *adopt),
        }
    }

    fn sheet(&self, n: u8) -> Which {
        self.sheets[usize::from(n) % self.sheets.len()]
    }

    /// A sheet a number picks, among those the model holds something worth the step on in three of four (`holds` says which), else among all of them: a step that is refused for want of
    /// the section it needs is a step the sequence does not get on with, and most of a sequence's steps are meant to.
    fn sheet_with(&self, n: u8, holds: impl Fn(&Held) -> bool) -> Which {
        let preferred: Vec<Which> = self.sheets.iter().copied().filter(|w| holds(&self.model.held(*w))).collect();
        match preferred.get(usize::from(n >> 2) % preferred.len().max(1)) {
            Some(w) if !n.is_multiple_of(4) => *w,
            _ => self.sheet(n >> 2),
        }
    }

    fn add_section(&mut self, w: Which, scratch: bool) -> Result<(), String> {
        let name = if scratch { "Scratch" } else { "User" };
        let there = self.model.held(w).scratch.is_some() && scratch || self.model.held(w).user.is_some() && !scratch;
        let called = self.call("sheet.add_section", w.with(json!({"section": name})))?;
        match (there, &called.answer) {
            (true, Err(e)) if e.to_string().contains("is there once") => return Ok(()),
            (true, other) => return Err(format!("a second {name} section on {w:?}: {other:?}")),
            (false, Err(e)) => return Err(format!("{name} section on {w:?} refused: {e}")),
            (false, Ok(_)) => {}
        }
        note(Event::SectionAdded);
        if scratch {
            self.model.held_mut(w).scratch = Some(vec![None]);
        } else {
            let row = URow { name: "Row_1".to_owned(), txt: Txt::Const(0), reads: Vec::new() };
            self.model.held_mut(w).user = Some(vec![row]);
        }
        self.commit(&called);
        if scratch {
            let key = called.answer.ok().and_then(|a| a["rows"].get(0).cloned()).ok_or("the new Scratch section lists no row")?;
            self.tag(w, 0, &key)?;
        }
        Ok(())
    }

    /// Gives a row of Scratch that is made and has no tag its tag, in a step of its own.
    fn tag(&mut self, w: Which, at: usize, key: &Value) -> Result<(), String> {
        let tag = self.model.fresh_tag();
        let cell = format!("Scratch.A{}", key_text(key));
        let called = self.call("sheet.set_cell", w.with(json!({"cell": cell, "formula": tag.to_string()})))?;
        called.answer.as_ref().map_err(|e| format!("tagging {cell} on {w:?}: {e}"))?;
        if let Some(slot) = self.model.held_mut(w).scratch.as_mut().and_then(|rows| rows.get_mut(at)) {
            *slot = Some(tag);
        }
        self.commit(&called);
        Ok(())
    }

    fn delete_section(&mut self, w: Which, scratch: bool) -> Result<(), String> {
        let name = if scratch { "Scratch" } else { "User" };
        let there = if scratch { self.model.held(w).scratch.is_some() } else { self.model.held(w).user.is_some() };
        let called = self.call("sheet.delete_section", w.with(json!({"section": name})))?;
        match (there, &called.answer) {
            (false, Err(e)) if e.to_string().contains("has no section") => return Ok(()),
            (false, other) => return Err(format!("deleted a {name} section that {w:?} does not hold: {other:?}")),
            (true, Err(e)) => return Err(format!("deleting {name} on {w:?} refused: {e}")),
            (true, Ok(_)) => {}
        }
        note(Event::SectionDeleted);
        if scratch {
            self.model.held_mut(w).scratch = None;
        } else {
            self.model.held_mut(w).user = None;
            if self.model.free_readers_of(w) > 0 {
                note(Event::FormulasFreed);
            }
        }
        self.commit(&called);
        Ok(())
    }

    fn add_row(&mut self, w: Which, scratch: bool, place: u8, name: Option<String>) -> Result<(), String> {
        let held = self.model.held(w);
        let (len, taken): (usize, Vec<String>) = if scratch {
            (held.scratch.as_ref().map_or(0, Vec::len), Vec::new())
        } else {
            (held.user.as_ref().map_or(0, Vec::len), held.user.iter().flatten().map(|r| r.name.to_ascii_lowercase()).collect())
        };
        let there = if scratch { held.scratch.is_some() } else { held.user.is_some() };
        let section = if scratch { "Scratch" } else { "User" };
        // Where it goes: the end, after a row or before one.
        let keys = if there && len > 0 { self.keys_of(w, section)? } else { Vec::new() };
        let r = usize::from(place / 3) % len.max(1);
        let (at, extra) = match (place % 3, keys.get(r)) {
            (1, Some(key)) => (r + 1, json!({"after": key})),
            (2, Some(key)) => (r, json!({"before": key})),
            _ => (len, json!({})),
        };
        let mut params = w.with(json!({"section": section}));
        if let (Some(map), Some(more)) = (params.as_object_mut(), extra.as_object()) {
            map.extend(more.clone());
        }
        if let (Some(map), Some(n)) = (params.as_object_mut(), name.as_ref()) {
            map.insert("name".into(), json!(n));
        }
        let called = self.call("sheet.add_row", params)?;
        let duplicate = name.as_ref().is_some_and(|n| taken.contains(&n.to_ascii_lowercase()));
        let middle = at < len;
        match (&called.answer, there, duplicate) {
            (Err(e), false, _) if e.to_string().contains("has no section") => return Ok(()),
            (Err(e), true, true) if e.to_string().contains("already has a row") => return Ok(()),
            (Err(e), true, false) if scratch && w.given() && middle && e.to_string().contains("add the row at the end") => {
                note(Event::MiddleRefused);
                return Ok(());
            }
            (Err(e), ..) => return Err(format!("add_row {section} on {w:?} at {at} of {len} refused or wrongly: {e}")),
            (Ok(_), false, _) => return Err(format!("a row added to a {section} section that {w:?} does not hold")),
            (Ok(_), _, true) => return Err(format!("a row named {name:?} added where the name is taken")),
            (Ok(_), true, false) => {}
        }
        let answer = called.answer.as_ref().map_err(ToString::to_string)?;
        note(Event::RowAdded);
        if scratch {
            if let Some(rows) = self.model.held_mut(w).scratch.as_mut() {
                rows.insert(at, None);
            }
        } else {
            let made =
                name.unwrap_or_else(|| (1..).map(|n| format!("Row_{n}")).find(|n| !taken.contains(&n.to_ascii_lowercase())).unwrap_or_default());
            if answer["row"].as_str() != Some(made.as_str()) {
                return Err(format!("the new row of User on {w:?} is {}, the model expects {made}", answer["row"]));
            }
            if let Some(rows) = self.model.held_mut(w).user.as_mut() {
                rows.insert(at, URow { name: made, txt: Txt::Const(0), reads: Vec::new() });
            }
        }
        self.commit(&called);
        if scratch {
            let key = answer["row"].clone();
            self.tag(w, at, &key)?;
        }
        Ok(())
    }

    fn delete_row(&mut self, w: Which, scratch: bool, row: u8) -> Result<(), String> {
        let held = self.model.held(w);
        let len = if scratch { held.scratch.as_ref().map_or(0, Vec::len) } else { held.user.as_ref().map_or(0, Vec::len) };
        let section = if scratch { "Scratch" } else { "User" };
        let there = if scratch { held.scratch.is_some() } else { held.user.is_some() };
        let at = usize::from(row) % len.max(1);
        let key = if len > 0 { self.keys_of(w, section)?.get(at).cloned() } else { None };
        let Some(key) = key else {
            // Nothing to delete: a row nothing holds, or a section nothing holds, is refused.
            let called = self.call("sheet.delete_row", w.with(json!({"section": section, "row": "Nope"})))?;
            return called.answer.err().map(|_| ()).ok_or_else(|| format!("deleted a row `Nope` that {w:?} does not hold"));
        };
        let called = self.call("sheet.delete_row", w.with(json!({"section": section, "row": key})))?;
        if let Err(e) = &called.answer {
            return Err(format!("deleting row {key} of {section} on {w:?} refused: {e} (section there: {there})"));
        }
        note(Event::RowDeleted);
        if scratch {
            if let Some(rows) = self.model.held_mut(w).scratch.as_mut() {
                rows.remove(at);
            }
        } else if let Some(rows) = self.model.held_mut(w).user.as_mut() {
            rows.remove(at);
        }
        self.commit(&called);
        Ok(())
    }

    /// The sheets that hold at least one row of User.
    fn with_rows(&self) -> Vec<Which> {
        self.sheets.iter().copied().filter(|w| !self.model.user(*w).is_empty()).collect()
    }

    /// What a form comes to, written at the row `name` of User on `w`: its text, what the model keeps, and the rows it reads. None when the form needs a row nothing holds.
    fn plan(&mut self, w: Which, name: &str, form: &Form) -> Result<Option<Planned>, String> {
        let old = self.model.row(w, name).map(|r| r.txt.clone());
        Ok(match form {
            Form::Const(n) => Some((n.to_string(), Txt::Const(i64::from(*n)), Vec::new())),
            Form::Clear => {
                let value = match old {
                    Some(Txt::Const(n)) => Some(n),
                    Some(Txt::Reader(t)) => Some(t),
                    Some(Txt::Cleared(v)) => v,
                    _ => None,
                };
                Some((String::new(), Txt::Cleared(value), Vec::new()))
            }
            Form::Cell { sheet, row, times, by, alone } => {
                let reachable: Vec<Which> = self.with_rows().into_iter().filter(|t| t.named_from(w).is_some()).collect();
                let Some(target) = reachable.get(usize::from(*sheet) % reachable.len().max(1)).copied() else { return Ok(None) };
                let rows = self.model.user(target);
                let Some(read) = rows.get(usize::from(*row) % rows.len().max(1)).map(|r| r.name.clone()) else { return Ok(None) };
                if self.model.reaches(&(target, read.clone()), &(w, name.to_owned())) {
                    return Ok(None);
                }
                let prefix = target.named_from(w).unwrap_or_default();
                let mut text = format!("{prefix}User.{read}.Value");
                if !*alone {
                    text.push_str(&format!("{}{by}", if *times { "*" } else { "+" }));
                }
                Some((text.clone(), Txt::Formula(text), vec![(target, read)]))
            }
            Form::Tag { sheet, row } => {
                let reachable: Vec<Which> =
                    self.sheets.iter().copied().filter(|t| t.named_from(w).is_some() && self.model.scratch(*t).iter().any(Option::is_some)).collect();
                let Some(target) = reachable.get(usize::from(*sheet) % reachable.len().max(1)).copied() else { return Ok(None) };
                let rows: Vec<(usize, i64)> = self.model.scratch(target).iter().enumerate().filter_map(|(i, t)| t.map(|t| (i, t))).collect();
                let Some((at, tag)) = rows.get(usize::from(*row) % rows.len().max(1)).copied() else { return Ok(None) };
                let key = self.keys_of(target, "Scratch")?.get(at).cloned().ok_or("the model holds a Scratch row the sheet does not list")?;
                let prefix = target.named_from(w).unwrap_or_default();
                Some((format!("{prefix}Scratch.A{}", key_text(&key)), Txt::Reader(tag), Vec::new()))
            }
        })
    }

    fn set(&mut self, sheet: u8, row: u8, form: &Form) -> Result<(), String> {
        let candidates = self.with_rows();
        let Some(w) = candidates.get(usize::from(sheet) % candidates.len().max(1)).copied() else {
            return self.set_nothing();
        };
        let rows = self.model.user(w);
        let Some(name) = rows.get(usize::from(row) % rows.len().max(1)).map(|r| r.name.clone()) else { return Ok(()) };
        let Some((text, txt, reads)) = self.plan(w, &name, form)? else { return Ok(()) };
        let old = self.model.row(w, &name).map(|r| r.txt.clone());
        let called = self.call("sheet.set_cell", w.with(json!({"cell": format!("User.{name}.Value"), "formula": text})))?;
        let answer = called.answer.as_ref().map_err(|e| format!("writing {text:?} to User.{name}.Value on {w:?}: {e}"))?;
        let changed = old.as_ref() != Some(&txt);
        if answer["changed"] != json!(changed) || called.stepped != changed {
            return Err(format!(
                "writing {text:?} to User.{name}.Value on {w:?} over {old:?}: changed {}, a step {}; the model expects {changed}",
                answer["changed"], called.stepped
            ));
        }
        note(match &txt {
            Txt::Const(_) => Event::Const,
            Txt::Formula(_) => Event::Formula,
            Txt::Reader(_) => Event::Reader,
            Txt::Cleared(_) | Txt::Free => Event::Cleared,
        });
        if let Some(row) = self.model.held_mut(w).user.as_mut().and_then(|rows| rows.iter_mut().find(|r| r.name == name)) {
            row.txt = txt;
            row.reads = reads;
        }
        self.commit(&called);
        Ok(())
    }

    /// A write when no sheet holds a row of User: the cell is not there, and the command says so.
    fn set_nothing(&mut self) -> Result<(), String> {
        let w = Which::Document;
        let called = self.call("sheet.set_cell", w.with(json!({"cell": "User.Alpha.Value", "formula": "1"})))?;
        match called.answer {
            Err(e) if e.to_string().contains("the sheet has no cell") => Ok(()),
            other => Err(format!("a write to a cell no sheet holds: {other:?}")),
        }
    }

    /// `sheet.set_cells`: writes planned one after the other on a copy of the model, the call all or nothing; the bad last write makes the whole call refused.
    fn batch(&mut self, writes: &[(u8, u8, Form)], bad_last: bool) -> Result<(), String> {
        let start = self.model.clone();
        let mut items: Vec<Value> = Vec::new();
        // The cells a write of this call made: a clear of one of them keeps the value the cell stood at before the call's recalculation, which no model knows (a formula typed and cleared in
        // one call is never worked out, so `No Formula` keeps the value that stood in the cell: the call recalculates once, at the end).
        let mut written_here: BTreeSet<(Which, String)> = BTreeSet::new();
        for (sheet, row, form) in writes {
            let candidates = self.with_rows();
            let Some(w) = candidates.get(usize::from(*sheet) % candidates.len().max(1)).copied() else { break };
            let rows = self.model.user(w);
            let Some(name) = rows.get(usize::from(*row) % rows.len().max(1)).map(|r| r.name.clone()) else { break };
            let Some((text, mut txt, reads)) = self.plan(w, &name, form)? else { continue };
            if matches!(txt, Txt::Cleared(_)) && written_here.contains(&(w, name.clone())) {
                txt = Txt::Cleared(None);
            }
            written_here.insert((w, name.clone()));
            let mut item = w.with(json!({"cell": format!("User.{name}.Value"), "formula": text}));
            if let Some(map) = item.as_object_mut() {
                map.insert("force".into(), json!(false));
            }
            items.push(item);
            if let Some(row) = self.model.held_mut(w).user.as_mut().and_then(|rows| rows.iter_mut().find(|r| r.name == name)) {
                row.txt = txt;
                row.reads = reads;
            }
        }
        if bad_last {
            items.push(Which::Document.with(json!({"cell": "User.Alpha.Value", "formula": BAD_TEXT[items.len() % BAD_TEXT.len()]})));
        }
        if items.is_empty() {
            self.model = start;
            return Ok(());
        }
        let called = self.call("sheet.set_cells", json!({"writes": items}))?;
        if bad_last {
            self.model = start;
            return match called.answer {
                Err(e) if e.to_string().contains("write ") => Ok(()),
                other => Err(format!("a batch with a bad last write: {other:?}")),
            };
        }
        let answer = called.answer.as_ref().map_err(|e| format!("a batch of {} writes refused: {e}", items.len()))?;
        let changed = self.model != start;
        if called.stepped != changed {
            return Err(format!("a batch ({}) made a step: {}; the model expects {changed}: {answer}", items.len(), called.stepped));
        }
        note(Event::Batch);
        self.commit(&called);
        Ok(())
    }

    /// A step on the instance of the master, which reads what the master and the style give: the model does not follow it, and it must keep the other rules.
    fn instance(&mut self, kind: u8) -> Result<(), String> {
        if !self.base.instance {
            return Ok(());
        }
        let w = Which::Shape { page: 1, id: 20 };
        let (id, params) = match kind % 10 {
            0 => ("sheet.add_section", w.with(json!({"section": "Scratch"}))),
            1 => ("sheet.add_row", w.with(json!({"section": "Scratch"}))),
            2 => ("sheet.add_row", w.with(json!({"section": "Scratch", "before": 1}))),
            3 => ("sheet.delete_row", w.with(json!({"section": "Scratch", "row": 1}))),
            4 => ("sheet.set_cell", w.with(json!({"cell": "Width", "formula": "4"}))),
            5 => ("sheet.set_cell", w.with(json!({"cell": "Width", "formula": ""}))),
            6 => ("sheet.delete_section", w.with(json!({"section": "Scratch"}))),
            7 => ("sheet.add_section", w.with(json!({"section": "User"}))),
            8 => ("sheet.add_row", w.with(json!({"section": "User"}))),
            _ => ("sheet.set_cell", w.with(json!({"cell": "User.Row_1.Value", "formula": "7"}))),
        };
        let called = self.call(id, params)?;
        if called.stepped {
            note(Event::InstanceStep);
        }
        self.commit(&called);
        Ok(())
    }

    /// Input every command refuses, in words, and that changes nothing.
    fn junk(&mut self, kind: u8, w: Which) -> Result<(), String> {
        let k = usize::from(kind);
        let (id, params) = match k % 4 {
            0 => ("sheet.set_cell", w.with(json!({"cell": "User.Alpha.Value", "formula": BAD_TEXT[k / 4 % BAD_TEXT.len()]}))),
            1 => ("sheet.set_cell", w.with(json!({"cell": BAD_CELL[k / 4 % BAD_CELL.len()], "formula": "1"}))),
            2 => match k / 4 % 5 {
                0 => ("sheet.set_cell", w.with(json!({"cell": "User.Alpha.Value", "formula": "1", "bogus": 1}))),
                1 => ("sheet.set_cell", json!({"cell": "User.Alpha.Value", "formula": "1"})),
                2 => ("sheet.set_cells", json!({"writes": []})),
                3 => ("sheet.add_row", w.with(json!({"section": "Nope"}))),
                _ => ("sheet.delete_section", w.with(json!({"section": "Shape Transform"}))),
            },
            _ => match k / 4 % 4 {
                0 => ("sheet.add_row", w.with(json!({"section": "User", "name": "9 bad name"}))),
                1 => ("sheet.add_row", w.with(json!({"section": "User", "after": 4_294_967_296_u64}))),
                2 => ("sheet.add_section", w.with(json!({"section": "Character"}))),
                _ => ("sheet.set_cell", w.with(json!({"cell": "User.Alpha.Value", "formula": "x".repeat(65_537)}))),
            },
        };
        let called = self.call(id, params.clone())?;
        match called.answer {
            Err(e) if !e.to_string().is_empty() => {
                note(Event::JunkRefused);
                Ok(())
            }
            other => Err(format!("{id} {params} was not refused: {other:?}")),
        }
    }

    /// Undo or redo one step, along the trail: the drawing and the model are those of the step. Nothing to undo at the start of the trail, nor to redo at its end.
    fn walk(&mut self, forward: bool) -> Result<(), String> {
        let can = if forward { self.at + 1 < self.trail.len() } else { self.at > 0 };
        if !can {
            return Ok(());
        }
        let id = if forward { "edit.redo" } else { "edit.undo" };
        self.query(id, json!({}))?;
        note(if forward { Event::Redo } else { Event::Undo });
        self.at = if forward { self.at + 1 } else { self.at - 1 };
        let (doc, model) = self.trail[self.at].clone();
        if self.s.document() != Some(&doc) {
            return Err(format!("{id} did not give the drawing of step {} of the trail", self.at));
        }
        self.model = model;
        Ok(())
    }

    // ---- what is checked after each step -----------------------------------------------------------------------------------------------

    /// The history is in step with the trail: one undo step for each step of it that was made, and the rest to redo.
    fn check_history(&self) -> Result<(), String> {
        let state = self.state();
        let (undo, redo) = (self.undo_base + self.at, self.trail.len() - 1 - self.at);
        if (state.undo, state.redo) == (undo, redo) {
            return Ok(());
        }
        Err(format!("the history holds {} undo and {} redo steps, the trail {undo} and {redo}", state.undo, state.redo))
    }

    /// The sections each sheet holds, their rows and their cells are what the model says.
    fn verify(&mut self) -> Result<(), String> {
        for w in self.sheets.clone() {
            self.verify_sheet(w)?;
        }
        Ok(())
    }

    fn verify_sheet(&mut self, w: Which) -> Result<(), String> {
        let held = self.model.held(w);
        let sections = self.query("sheet.sections", w.keys())?;
        let present: Vec<&str> = sections["present"].as_array().into_iter().flatten().filter_map(|p| p["section"].as_str()).collect();
        // A section with no rows is held and not shown: the window has nothing to list in it, and `add_row` still finds it.
        let wanted = [("User", held.user.as_ref().is_some_and(|r| !r.is_empty())), ("Scratch", held.scratch.as_ref().is_some_and(|r| !r.is_empty()))];
        for (name, there) in wanted {
            if present.contains(&name) != there {
                return Err(format!("{w:?} shows the sections {present:?}; the model says {name} is shown: {there}"));
            }
        }
        let names: Vec<&str> = wanted.iter().filter(|(_, there)| *there).map(|(n, _)| *n).collect();
        if names.is_empty() {
            return Ok(());
        }
        let read = self.query("sheet.read", w.with(json!({"sections": names, "count": 2000})))?;
        if let Some(rows) = &held.user {
            verify_user(w, rows, &read)?;
        }
        if let Some(tags) = &held.scratch {
            verify_scratch(w, tags, &read)?;
        }
        Ok(())
    }

    /// A full recalculation finds every cell as its formula gives it from the values that stand: the edits worked out whatever they reached.
    fn settled(&self) -> Result<(), String> {
        let Some(doc) = self.s.document() else { return Err("no document".to_owned()) };
        let report = recalc_all(doc, &Budget::default());
        if report.differed.is_empty() && report.differed_unlisted == 0 && !report.budget_exhausted {
            return Ok(());
        }
        let first: Vec<_> = report.differed.iter().take(3).collect();
        Err(format!(
            "a full recalculation differs from the drawing in {} cells, the first {first:?}",
            report.differed.len() as u64 + report.differed_unlisted
        ))
    }

    /// Every `ref` a read gives maps back to its cell: the cells of the sections the model follows, and the first of the others.
    fn refs_map_back(&mut self, w: Which) -> Result<(), String> {
        let read = self.query("sheet.read", w.with(json!({"count": 2000})))?;
        let mut checked = 0_usize;
        for line in read["lines"].as_array().into_iter().flatten() {
            let ours = matches!(line["section"].as_str(), Some("User" | "Scratch"));
            for cell in line["cells"].as_array().into_iter().flatten() {
                let Some(reference) = cell["ref"].as_str() else { continue };
                if !ours && checked >= 12 {
                    continue;
                }
                checked += 1;
                let again = self.query("sheet.cell", w.with(json!({"cell": reference})))?;
                for key in ["ref", "name", "formula", "value", "unit", "error", "origin"] {
                    if cell["truncated"] != json!(true) && again[key] != cell[key] {
                        return Err(format!(
                            "the ref {reference} of {w:?} reads {} in sheet.read and {} in sheet.cell, for {key}",
                            cell[key], again[key]
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    // ---- saving --------------------------------------------------------------------------------------------------------------------------

    /// What each modelled sheet reads as, for a comparison of the drawing with the one a file gives back.
    fn reads(s: &mut Session, sheets: &[Which]) -> Result<Vec<Value>, String> {
        sheets.iter().map(|w| s.execute("sheet.read", &w.with(json!({"count": 2000}))).map_err(|e| format!("sheet.read of {w:?}: {e}"))).collect()
    }

    /// Saves the drawing as a `.vsdx` or a `.diacraft` file, reads it in a session of its own and compares each sheet; the session may go on from what the file gave.
    fn save(&mut self, vsdx: bool, adopt: bool) -> Result<(), String> {
        self.saved += 1;
        let ext = if vsdx { "vsdx" } else { "diacraft" };
        let path = self.dir.path(&format!("sheets-{}.{ext}", self.saved));
        self.query("file.save_as", json!({"path": path}))?;
        let mut reopened = Session::new();
        reopened.execute("file.open", &json!({"path": path})).map_err(|e| format!("the {ext} file just saved does not open: {e}"))?;
        let sheets = self.sheets.clone();
        let (mine, theirs) = (Run::reads(&mut self.s, &sheets)?, Run::reads(&mut reopened, &sheets)?);
        for ((w, a), b) in sheets.iter().zip(&mine).zip(&theirs) {
            if a != b {
                return Err(format!("{w:?} reads differently after a save as {ext} and an open:\n  before {a}\n  after  {b}"));
            }
        }
        if !vsdx && self.s.document() != reopened.document() {
            return Err("the drawing read back from a .diacraft file is not the drawing that was saved".to_owned());
        }
        note(if vsdx { Event::SavedVsdx } else { Event::SavedDiacraft });
        if adopt {
            note(Event::Adopted);
            self.undo_base = reopened.history().map_or(0, |h| h.undo_len());
            self.s = reopened;
            if let Some(doc) = self.s.document() {
                self.trail = vec![(doc.clone(), self.model.clone())];
                self.at = 0;
            }
        }
        Ok(())
    }

    /// The end of a sequence: every ref maps back, both writers save the drawing, undo takes every step back along the trail and redo puts them again.
    fn finish(&mut self) -> Result<(), String> {
        for w in self.sheets.clone() {
            self.refs_map_back(w)?;
        }
        self.save(true, false)?;
        self.save(false, false)?;
        let end = self.at;
        while self.at > 0 {
            self.walk(false)?;
            self.verify()?;
        }
        while self.at < end {
            self.walk(true)?;
            self.verify()?;
        }
        self.check_history()
    }
}

/// The rows of User a `sheet.read` lists are the model's, with the cells the model knows.
fn verify_user(w: Which, rows: &[URow], read: &Value) -> Result<(), String> {
    let listed = rows_in(read, "User");
    let names: Vec<String> = listed.iter().map(|(k, _)| key_text(k)).collect();
    let want: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    if names != want {
        return Err(format!("the rows of User on {w:?} are {names:?}; the model says {want:?}"));
    }
    for (row, (_, cells)) in rows.iter().zip(&listed) {
        let Some(cell) = cells.get("Value") else { return Err(format!("the row {} of User on {w:?} lists no Value", row.name)) };
        let at = format!("User.{}.Value on {w:?} (the model: {:?})", row.name, row.txt);
        let (formula, value) = (cell["formula"].as_str().unwrap_or(""), cell["value"].as_f64());
        let wrong = match &row.txt {
            Txt::Const(n) => formula != n.to_string() || value != Some(*n as f64),
            Txt::Formula(text) => formula != text,
            Txt::Reader(tag) => value != Some(*tag as f64),
            Txt::Cleared(kept) => formula != "No Formula" || kept.is_some_and(|n| value != Some(n as f64)),
            Txt::Free => false,
        };
        if wrong {
            return Err(format!("{at} reads formula {formula:?} and value {}", cell["value"]));
        }
    }
    Ok(())
}

/// The rows of Scratch a `sheet.read` lists hold the tags of the model, in order.
fn verify_scratch(w: Which, tags: &[Option<i64>], read: &Value) -> Result<(), String> {
    let listed = rows_in(read, "Scratch");
    let held: Vec<Option<i64>> = listed.iter().map(|(_, cells)| cells.get("A").and_then(|c| c["value"].as_f64()).map(|v| v as i64)).collect();
    let agree = held.len() == tags.len() && held.iter().zip(tags).all(|(h, t)| t.is_none() || h == t);
    if agree {
        return Ok(());
    }
    Err(format!("the rows of Scratch on {w:?} hold {held:?}; the model says {tags:?}"))
}

// ---- running a sequence ----------------------------------------------------------------------------------------------------------------

/// Runs one sequence on one drawing and says what failed, with the steps taken.
fn run_case(base: Base, ops: &[Op]) -> Result<(), String> {
    let mut run = Run::new(base);
    for (n, op) in ops.iter().enumerate() {
        run.log.push(format!("{op:?}"));
        let checked = run.exec(op).and_then(|()| run.check_history()).and_then(|()| run.verify()).and_then(|()| run.settled());
        if let Err(why) = checked {
            return Err(format!("{base:?}, step {n} of {}: {why}\nsteps so far:\n  {}", ops.len(), run.log.join("\n  ")));
        }
    }
    run.finish().map_err(|why| format!("{base:?}, at the end of {} steps: {why}\nsteps:\n  {}", ops.len(), run.log.join("\n  ")))
}

/// A section whose rows were all deleted is held and not shown (the window has nothing to list in it): it is still there after a save and an open in either format, so that a row can
/// be added to it as it could before the save. The model of the sequences holds it so, and a sequence that saved and went on from the file read finds out when it is not.
#[test]
fn a_section_with_no_rows_is_still_there_after_a_save_and_an_open_in_each_format() {
    for ext in ["vsdx", "diacraft"] {
        let mut run = Run::new(Base { pages: 1, shapes: 1, instance: false });
        let w = Which::Shape { page: 1, id: 10 };
        run.add_section(w, false).unwrap();
        run.delete_row(w, false, 0).unwrap();
        assert_eq!(run.model.held(w).user, Some(Vec::new()), "the model holds the empty section");
        let path = run.dir.path(&format!("empty.{ext}"));
        run.query("file.save_as", json!({"path": path})).unwrap();
        let mut reopened = Session::new();
        reopened.execute("file.open", &json!({"path": path})).unwrap();
        let added = reopened.execute("sheet.add_row", &w.with(json!({"section": "User", "name": "Alpha"})));
        assert!(added.is_ok(), "{ext}: a row added to the empty section of the file that was read: {added:?}");
    }
}

/// Runs `cases` sequences from the seed, and gives the first that fails, shrunk.
fn sweep(cases: u32, seed: u64) -> Result<(), String> {
    let mut seed_bytes = [0_u8; 32];
    seed_bytes[..8].copy_from_slice(&seed.to_le_bytes());
    let config = Config { cases, failure_persistence: None, max_shrink_iters: 400, ..Config::default() };
    let mut runner = TestRunner::new_with_rng(config, TestRng::from_seed(RngAlgorithm::ChaCha, &seed_bytes));
    let strategy = (base(), prop::collection::vec(op(), 8..=48));
    runner.run(&strategy, |(base, ops)| run_case(base, &ops).map_err(TestCaseError::fail)).map_err(|e| e.to_string())
}

#[test]
fn chains_keep_the_model() {
    if let Err(why) = sweep(64, 0x5EED_0001) {
        panic!("{why}");
    }
    // The sequences reached what the test claims to check (a count of 0 would be a generator that never gets there), failed or not.
    let reached = [
        Event::SectionAdded,
        Event::RowAdded,
        Event::RowDeleted,
        Event::Const,
        Event::Formula,
        Event::Reader,
        Event::Cleared,
        Event::Batch,
        Event::Undo,
        Event::Redo,
        Event::SavedVsdx,
        Event::SavedDiacraft,
        Event::JunkRefused,
        Event::InstanceStep,
    ];
    for event in reached {
        assert!(count(event) > 0, "no sequence reached {}:\n{}", EVENTS[event as usize], tally());
    }
}

/// The local sweep: many more sequences, on seeds to choose (`DIACRAFT_SHEET_CASES`, `DIACRAFT_SHEET_SEED`). Run alone, with `--ignored --nocapture --test-threads=1`.
#[test]
#[ignore = "a sweep of thousands of sequences, run by hand: DIACRAFT_SHEET_CASES and DIACRAFT_SHEET_SEED choose its size and seed"]
fn a_long_sweep_of_sequences() {
    let cases = std::env::var("DIACRAFT_SHEET_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(2_000);
    let seed = std::env::var("DIACRAFT_SHEET_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let outcome = sweep(cases, seed);
    eprintln!("what {cases} sequences from seed {seed} reached:\n{}", tally());
    if let Err(why) = outcome {
        panic!("{why}");
    }
}
