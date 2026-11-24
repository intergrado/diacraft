//! The check after a save (2026-11-22).
//!
//! **What it is.** After a save through a **model writer** (the VSDX writer, the `.odg` writer and the `.diacraft` writer, when the drawing came from a file of another format), the engine reads
//! the file it wrote back with the reader a person's Open uses, and compares what it read with the drawing that was saved, **class by class** ([`Class`]). The save result's `checked` field
//! says each class that differs, with the count of what differs in it (a program reads it), and the save card says the same in words ([`words`]) after the note's sentences about the data the file
//! carries. **A save is never held back by it**: the file is written, and the card says what differs. This is the 0.2 meaning of "ship safe: nothing is lost silently" for the `.odg` and for a
//! crossing between the formats; the Compatibility Checker **before** a save (Continue or Cancel) stays a 0.3 item.
//!
//! **What it does not check.** A save in place into the file the drawing was opened from (a splice) is not checked in 0.2: it keeps every part the edits did not touch, byte for byte, and
//! refuses a change it cannot write. DiaCraft's own carrier (the `DiaCraft/` entries of an `.odg`, the `visio/solutions` parts of a VSDX file) is not compared as content: what it carries is
//! compared where it comes back, in the model.
//!
//! **The same content is no loss.** A number in another unit, a colour in another spelling, `Width*1` for `Width`, a value a style sheet gives where the drawing held it on the shape, a style
//! link written as the values it gives (for a VSDX drawing through an `.odg`; for an `.odg`) are one flag, `as_values`, that the card says once: "written as values the file format
//! holds". **A formula that does not come back as a formula is a loss**, even when its value comes back, and the check counts it.
//!
//! **What it costs.** One read of the file written and one walk of both drawings, charged in steps before any table is built, against [`compare::budget`] (at least [`compare::MAX_STEPS`], and
//! more in proportion to what the drawings hold). A check that cannot finish (the reader
//! refuses the file, the walk is past its budget) says so, in words, with the reason: "the file was written; DiaCraft could not check it".
//!
//! * [`class`]: the classes, their stable ids and words.
//! * [`cells`]: when two cells are the same content, and the class a cell that differs is told in.
//! * [`kept`]: whether what a format keeps beside the cells came back.
//! * [`compare`]: the walk.

mod cells;
mod class;
mod comments;
mod compare;
mod kept;
mod meter;
#[cfg(test)]
mod tests;

use std::cell::RefCell;

use diacraft_doc::Document;
use serde_json::{Value, json};

pub use class::{ALL, Class};
/// For the Kani harnesses of the crate (`crate::kani_proofs`).
#[cfg(kani)]
pub(crate) use {cells::close, compare::Tally, meter::Meter};

/// One class that differs, and how many of its items do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Differs {
    /// The class.
    pub class: Class,
    /// The items of the class that differ, in the unit the class counts ([`Class::words`]).
    pub count: u64,
}

/// What the check found for a file it could read back: the classes that differ, and whether some content is written in a form the format holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Checked {
    /// The classes that differ, in the order of [`ALL`]. Empty when nothing does.
    pub classes: Vec<Differs>,
    /// Some content is written as values the file format holds (units, the form of a formula, a value taken from a style sheet, a style link): the same content, said once.
    pub as_values: bool,
}

/// The result of the check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The file was read back and compared.
    Checked(Checked),
    /// The file was written and the check could not finish: why, in words that hold no name from the file.
    Unchecked(String),
}

impl Outcome {
    /// Whether the check found nothing to say: it finished, nothing differs, and no content is written as values.
    pub fn is_clean(&self) -> bool {
        matches!(self, Outcome::Checked(checked) if checked.classes.is_empty() && !checked.as_values)
    }

    /// The result's `checked` field: `{done, classes: [{class, words, count}], as_values, reason}`. `done` is false for a check that could not finish, and then `reason` says why.
    pub fn to_json(&self) -> Value {
        match self {
            Outcome::Checked(checked) => json!({
                "done": true,
                "classes": checked.classes.iter().map(|d| json!({ "class": d.class.id(), "words": d.class.words(d.count), "count": d.count })).collect::<Vec<_>>(),
                "as_values": checked.as_values,
                "reason": Value::Null,
            }),
            Outcome::Unchecked(reason) => json!({ "done": false, "classes": [], "as_values": false, "reason": reason }),
        }
    }

    /// The outcome a result's `checked` field says, or `None` for a value that is not one (a save that was not checked has `null`). The window reads the field with this and words it with
    /// [`words`]; it never parses a warning.
    pub fn from_json(value: &Value) -> Option<Outcome> {
        let done = value.get("done")?.as_bool()?;
        if !done {
            return Some(Outcome::Unchecked(value.get("reason")?.as_str()?.to_owned()));
        }
        let mut classes = Vec::new();
        for entry in value.get("classes")?.as_array()? {
            let class = Class::of_id(entry.get("class")?.as_str()?)?;
            classes.push(Differs { class, count: entry.get("count")?.as_u64()? });
        }
        Some(Outcome::Checked(Checked { classes, as_values: value.get("as_values")?.as_bool()? }))
    }
}

/// The most characters of a reason the result keeps: the reader's refusals are a sentence and a number.
const REASON_CHARS: usize = 300;

/// The words of the card for a check, one string for each part that applies, in order: **the classes that differ**, each with its count, then **the values group**, then, for a check that could not
/// finish, **why**. Empty when nothing is to be said (a check that found nothing, or none made): with nothing to say there is no card.
pub fn words(outcome: &Outcome) -> Vec<String> {
    let mut out = Vec::new();
    match outcome {
        Outcome::Checked(checked) => {
            if !checked.classes.is_empty() {
                let n = checked.classes.len();
                out.push(format!(
                    "DiaCraft read the file back and compared it with the drawing, and the file differs in {n} way{}. The drawing in DiaCraft is as it was.",
                    if n == 1 { "" } else { "s" }
                ));
                for differs in &checked.classes {
                    out.push(format!("{}: {}", differs.class.label(), differs.class.words(differs.count)));
                }
            }
            if checked.as_values {
                out.push(
                    "Written as values the file format holds: units, the form of a formula (Width*1 for Width), values taken from style sheets, links to style sheets, and the forms of an earlier version of the file format in the form its current version takes. It is the same content."
                        .to_owned(),
                );
            }
        }
        Outcome::Unchecked(reason) => {
            out.push(format!("The file was written; DiaCraft could not check it: {reason}"));
        }
    }
    out
}

/// A function that changes the drawing read back, which a test plants.
type Tamper = Box<dyn FnMut(&mut Document)>;

thread_local! {
    /// A test's way to plant a loss: a function that changes the drawing read back before it is compared (never set outside a test).
    static TAMPER: RefCell<Option<Tamper>> = const { RefCell::new(None) };
}

/// Runs `run` with `tamper` applied to the drawing every check in this thread reads back, so that a test can plant a loss in any class and see the save name it. Test support: it is
/// `#[doc(hidden)]`, nothing in the engine sets it, and it works in the calling thread only.
#[doc(hidden)]
pub fn with_tamper<R>(tamper: impl FnMut(&mut Document) + 'static, run: impl FnOnce() -> R) -> R {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            TAMPER.with(|slot| *slot.borrow_mut() = None);
        }
    }
    TAMPER.with(|slot| *slot.borrow_mut() = Some(Box::new(tamper)));
    let _reset = Reset;
    run()
}

/// The drawing read back, changed as the test in this thread asked (a clone: the drawings share what they hold), or `None` when no test asked.
fn tampered(after: &Document) -> Option<Document> {
    TAMPER.with(|slot| {
        let mut slot = slot.borrow_mut();
        let tamper = slot.as_mut()?;
        let mut copy = after.clone();
        tamper(&mut copy);
        Some(copy)
    })
}

/// Compares `before`, the drawing a model writer was given, with `after`, what its file reads back as, within the steps a comparison of drawings of their size may take ([`compare::budget`]).
pub fn compare(before: &Document, after: &Document, macros_left_out: bool) -> Outcome {
    compare_within(before, after, macros_left_out, compare::budget(before, after))
}

/// [`compare`] with the most steps it may take given.
pub(crate) fn compare_within(before: &Document, after: &Document, macros_left_out: bool, max_steps: u64) -> Outcome {
    let changed = tampered(after);
    let after = changed.as_ref().unwrap_or(after);
    let mut walk = compare::Compare::new(before, after, macros_left_out, max_steps);
    if walk.run().is_err() {
        return Outcome::Unchecked("the drawing is too big to compare with the file in one step".to_owned());
    }
    let tally = walk.tally;
    let classes = ALL.into_iter().map(|class| Differs { class, count: tally.count(class) }).filter(|d| d.count > 0).collect();
    Outcome::Checked(Checked { classes, as_values: tally.as_values })
}

/// Why a file could not be read back, in words that name nothing from the file: the class of the reader's refusal, and for want of room or memory, that.
fn unread(class: &str) -> Outcome {
    let room = ["large", "memory", "limit", "exceeded", "too_many", "complex"].iter().any(|word| class.contains(word));
    let words = if room {
        format!("there was not enough room or memory to read the file back ({class})")
    } else {
        format!("the file does not read back ({class})")
    };
    Outcome::Unchecked(crate::shown(&words, REASON_CHARS))
}

/// The first places that differ between `before` and `after`, in words that hold the names of cells and their values: **for a test or a person finding why a card says what it says**, and never shown to
/// a user or put in a result (the words may hold what a drawing holds). At most 64.
#[doc(hidden)]
pub fn explain(before: &Document, after: &Document) -> Vec<String> {
    let mut walk = compare::Compare::new(before, after, false, compare::budget(before, after));
    walk.trace_on = true;
    let _ = walk.run();
    walk.trace
}

/// The check of an `.odg` the `.odg` writer made from `before`: the file read back as an Open reads it, and compared. **The read is the save's own**, made beside the drawing the save holds: the save
/// refused before it began what would not fit a second copy of the drawing beside the first, and the writer's own check of the file made one such copy already, which has been let go.
pub(crate) fn check_odg(before: &Document, bytes: &[u8], macros_left_out: bool) -> Outcome {
    let limits = diacraft_odg::Limits { threads: 1, ..diacraft_odg::Limits::default() };
    match crate::odgfile::read_odg(bytes, &limits) {
        Ok((after, _)) => compare(before, &after, macros_left_out),
        Err(e) => unread(e.class()),
    }
}

/// The check of a `.diacraft` file written from `before`, a drawing that came from a file of another format. The memory the read may use is what the process's budget leaves beside what it holds.
pub(crate) fn check_native(before: &Document, bytes: &[u8], macros_left_out: bool) -> Outcome {
    let limits = crate::nativefile::open::limits_with(crate::nativefile::open::READ_BUDGET.saturating_sub(crate::memory::held()));
    match crate::nativefile::read_native(bytes, &limits) {
        Ok(read) => compare(before, &read.doc, macros_left_out),
        Err(e) => unread(e.class()),
    }
}
