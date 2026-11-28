//! Undo groups, which only a running command can open (so these tests run local commands that open
//! them), and what a panic or an error inside one leaves behind.
//!
//! These are crate-internal tests because `Session::grouped` is not public: the integration tests reach
//! groups through `command.batch`.

use diacraft_doc::Document;
use std::sync::Arc;

use serde_json::{Value, json};

use super::Session;
use crate::cmd::{self, CommandSpec};
use crate::error::{EngineError, Result, failed};
use crate::guard;

fn spec(id: &'static str, run: crate::Run) -> CommandSpec {
    CommandSpec { id, label: "Test", menu: &[], shortcut: None, params: "none", enabled: cmd::always, run, journal: true }
}

/// A session with a document of two pages made by two edits.
fn session() -> Session {
    let mut s = Session::new();
    for (id, params) in [("file.new", json!({})), ("page.rename", json!({"id": 1, "name": "Baseline"})), ("page.add", json!({"name": "Second"}))] {
        s.execute(id, &params).unwrap();
    }
    s
}

/// Everything a failed group must leave as it was.
#[derive(Debug, PartialEq)]
struct Snap {
    document: Option<Document>,
    undo_steps: usize,
    redo_steps: usize,
    revision: u64,
    journal: Vec<(String, Value)>,
}

fn snap(s: &Session) -> Snap {
    Snap {
        document: s.document().cloned(),
        undo_steps: s.history().map_or(0, |h| h.undo_len()),
        redo_steps: s.history().map_or(0, |h| h.redo_len()),
        revision: s.revision(),
        journal: s.journal().to_vec(),
    }
}

fn add_page(s: &mut Session, label: &str, name: &str) -> Result<u32> {
    s.edit(label, |d| d.add_page(Some(name)).map_err(|e| failed("test", e.to_string())))
}

// ---------- what a group records ----------

fn two_pages_in_a_group(s: &mut Session, _: &Value) -> Result<Value> {
    s.grouped("Two pages", |s| {
        add_page(s, "Add a", "A")?;
        add_page(s, "Add b", "B")?;
        Ok(json!({}))
    })
}

#[test]
fn grouped_edits_are_one_undo_step_named_for_the_group() {
    let mut s = session();
    let before = s.document().cloned().unwrap();
    let steps = s.history().unwrap().undo_len();

    s.run_spec(&spec("test.two", two_pages_in_a_group), &json!({})).unwrap();

    assert_eq!(s.document().map(|d| d.pages.len()), Some(4));
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Two pages"));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.document(), Some(&before));
}

fn empty_group(s: &mut Session, _: &Value) -> Result<Value> {
    s.grouped("Nothing", |_| Ok(json!({})))
}

#[test]
fn a_group_that_edits_nothing_records_nothing() {
    let mut s = session();
    let before = snap(&s);
    s.run_spec(&spec("test.empty", empty_group), &json!({})).unwrap();
    assert_eq!(Snap { journal: vec![], ..snap(&s) }, Snap { journal: vec![], ..before }, "no step, no revision");
}

// ---------- when a group fails ----------

/// A group that edits and fails; the command goes on and succeeds, having caught the failure.
fn failed_group_then_carry_on(s: &mut Session, _: &Value) -> Result<Value> {
    let outcome = s.grouped("Doomed", |s| {
        add_page(s, "Add", "Gone")?;
        // A command run inside the group is part of it: it is not journaled of its own.
        s.execute("page.add", &json!({"name": "Gone too"}))?;
        Err::<Value, _>(failed("test", "changed my mind"))
    });
    Ok(json!({ "group_said": outcome.unwrap_err().to_string(), "pages_now": s.document().map(|d| d.pages.len()) }))
}

#[test]
fn a_failed_group_restores_the_document_and_the_revision_and_journals_nothing_of_its_own() {
    let mut s = session();
    let before = snap(&s);

    let result = s.run_spec(&spec("test.carry_on", failed_group_then_carry_on), &json!({})).unwrap();

    assert_eq!(result["group_said"], "`test` failed: changed my mind");
    assert_eq!(result["pages_now"], 2, "inside the command, right after the group failed, the pages are back");
    let after = snap(&s);
    assert_eq!(
        (&after.document, after.undo_steps, after.redo_steps, after.revision),
        (&before.document, before.undo_steps, before.redo_steps, before.revision)
    );
    // The command succeeded and is journaled; the page.add it ran inside the group that was rolled back is not.
    assert_eq!(after.journal.len(), before.journal.len() + 1);
    assert_eq!(after.journal.last().map(|e| e.0.as_str()), Some("test.carry_on"));
    // And a group can be opened again: the failed one left nothing open.
    s.run_spec(&spec("test.two", two_pages_in_a_group), &json!({})).unwrap();
}

fn undo_in_a_group(s: &mut Session, _: &Value) -> Result<Value> {
    let said = s.grouped("Outer", |s| {
        let mut said = vec![];
        for id in ["edit.undo", "edit.redo", "file.new"] {
            said.push(s.execute(id, &json!({})).unwrap_err().to_string());
        }
        // A group inside a group.
        said.push(s.grouped("Inner", |_| Ok(json!({}))).unwrap_err().to_string());
        Ok(json!(said))
    })?;
    Ok(said)
}

#[test]
fn groups_do_not_nest_and_refuse_undo_redo_and_new() {
    let mut s = session();
    s.execute("edit.undo", &json!({})).unwrap();
    let before = snap(&s);
    let said = s.run_spec(&spec("test.undo_in_group", undo_in_a_group), &json!({})).unwrap();
    let said: Vec<&str> = said.as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    // edit.redo is enabled here (a step was undone), so all three reach the group's refusal.
    for (said, id) in said.iter().zip(["edit.undo", "edit.redo", "file.new"]) {
        assert!(said.contains(id) && said.contains("inside a batch or other undo group"), "{id}: {said}");
    }
    assert!(said[3].contains("undo groups cannot nest"), "{}", said[3]);
    assert_eq!(snap(&s).document, before.document);
    assert_eq!(snap(&s).redo_steps, 1, "refused, not done");
}

fn group_outside_a_command_is_refused(s: &mut Session, _: &Value) -> Result<Value> {
    Ok(json!(s.grouped("Fine name", |_| Ok(json!({}))).is_ok()))
}

#[test]
fn a_group_is_refused_outside_a_running_command_and_without_a_document() {
    // Direct call: no command is running, so no group may open (its edits could be journaled one by one
    // and a panic would have nothing to put it back).
    let mut s = session();
    let before = snap(&s);
    let mut ran = false;
    let e = s
        .grouped("Direct", |_| {
            ran = true;
            Ok(())
        })
        .unwrap_err();
    assert!(!ran);
    assert!(matches!(&e, EngineError::Internal { msg, .. } if msg.contains("running command")), "{e:?}");
    assert_eq!(snap(&s), before);

    // From a command, but with no document.
    let mut none = Session::new();
    let result = none.run_spec(&spec("test.no_doc", group_outside_a_command_is_refused), &json!({})).unwrap();
    assert_eq!(result, json!(false));
}

fn group_with_a_bad_name(s: &mut Session, _: &Value) -> Result<Value> {
    let mut ran = false;
    let e = s
        .grouped("two\nlines", |_| {
            ran = true;
            Ok(json!({}))
        })
        .unwrap_err();
    Ok(json!({ "ran": ran, "error": e.to_string() }))
}

#[test]
fn a_group_refuses_a_name_that_breaks_the_name_rule_before_running() {
    let mut s = session();
    let r = s.run_spec(&spec("test.bad_name", group_with_a_bad_name), &json!({})).unwrap();
    assert_eq!(r["ran"], false);
    let text = r["error"].as_str().unwrap();
    assert!(text.contains("must not contain control characters") && !text.chars().any(char::is_control), "{text:?}");
}

// ---------- when a group panics ----------

fn panic_in_a_group(s: &mut Session, _: &Value) -> Result<Value> {
    s.grouped("Doomed", |s| {
        add_page(s, "Doomed", "Doomed")?;
        panic!("boom in a group")
    })
}

#[test]
fn a_panic_inside_a_group_the_command_does_not_catch_puts_the_session_back() {
    let mut s = session();
    let before = snap(&s);
    let e = s.run_spec(&spec("test.group_panic", panic_in_a_group), &json!({})).unwrap_err();
    assert!(matches!(&e, EngineError::Internal { msg, .. } if msg.contains("boom in a group")), "{e:?}");
    assert_eq!(s.panics_caught(), 1);
    assert_eq!(snap(&s), before);
    assert!(!s.in_group());
    // Nothing the group refuses is refused now.
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("file.new", &json!({})).unwrap();
}

/// The command catches the panic itself, as a caller of `catch_panic` around a call would, and goes on:
/// the group must not be left open, nor its half-made edit in the document.
fn catch_a_group_panic_and_carry_on(s: &mut Session, _: &Value) -> Result<Value> {
    let (title, revision, undo_steps) = (s.document().map(|d| d.title.clone()), s.revision(), s.history().map(|h| h.undo_len()));
    let caught = guard::catch_panic(|| {
        s.grouped::<Value>("Doomed", |s| {
            s.edit("Half", |d| {
                d.title = "half edited".into();
                Ok(())
            })?;
            // A command run inside the group raised the depth; the panic skips its way back down.
            s.depth += 3;
            panic!("boom in a group")
        })
    });
    Ok(json!({
        "caught": caught.is_err(),
        "in_group": s.in_group(),
        "title_unchanged": s.document().map(|d| d.title.clone()) == title,
        "revision_unchanged": s.revision() == revision,
        "undo_steps_unchanged": s.history().map(|h| h.undo_len()) == undo_steps,
        "depth_back": s.depth == 1,
        // A new group can be opened and works.
        "next_group_ok": s.grouped("Next", |s| add_page(s, "Add", "After")).is_ok(),
    }))
}

#[test]
fn a_panic_in_a_group_that_the_command_catches_leaves_no_group_and_no_half_edit() {
    let mut s = session();
    let r = s.run_spec(&spec("test.catch", catch_a_group_panic_and_carry_on), &json!({})).unwrap();
    assert_eq!(
        r,
        json!({"caught": true, "in_group": false, "title_unchanged": true, "revision_unchanged": true, "undo_steps_unchanged": true, "depth_back": true, "next_group_ok": true})
    );
}

// ---------- the page reads kept for the snap ----------

/// An edit, then a read of the page's, then an error: the revision is put back, and the revision the edit reached is reached again by the next edit.
fn read_then_fail(s: &mut Session, _: &Value) -> Result<Value> {
    add_page(s, "Edit", "Mid")?;
    // A read asked for while the command runs is given, and **not kept**: the revision it was taken at is one the failure takes back.
    let read = s.snap_page(1);
    assert!(read.is_some(), "the page is read for the command");
    assert!(s.snap_reads().is_empty(), "and not kept");
    Err(failed("test", "the command fails after its edit"))
}

#[test]
fn a_page_read_while_a_command_runs_is_not_kept_so_a_failed_command_leaves_no_read_of_what_it_edited() {
    let mut s = session();
    let outside = s.snap_page(1).unwrap();
    assert!(Arc::ptr_eq(&s.kept_snap_page(1).unwrap(), &outside), "a read asked for between commands is kept for the revision");
    let revision = s.revision();
    s.run_spec(&spec("test.read_then_fail", read_then_fail), &json!({})).unwrap_err();
    assert_eq!(s.revision(), revision, "the failure put the revision back");
    // The edit that comes next reaches the revision the failed command's own edit reached; the read taken in the failed command would have been found for it.
    add_page(&mut s, "Another", "Other").unwrap();
    assert!(s.kept_snap_page(1).is_none(), "nothing kept for the revision of another document");
    assert!(s.snap_reads().is_empty());
}

/// An edit through the session's own `edit` moves the revision and so drops the kept read; a call that changes nothing (`edit_if` saying so) moves nothing and keeps it.
#[test]
fn an_edit_drops_the_kept_read_and_one_that_changes_nothing_keeps_it() {
    let mut s = session();
    let first = s.snap_page(1).unwrap();
    s.edit_if("Nothing", |_| Ok(((), false))).unwrap();
    assert!(Arc::ptr_eq(&s.kept_snap_page(1).unwrap(), &first), "no change, no new revision, the same read");
    add_page(&mut s, "Edit", "Third").unwrap();
    assert!(s.kept_snap_page(1).is_none(), "the edit moved the revision");
    let second = s.snap_page(1).unwrap();
    assert!(!Arc::ptr_eq(&first, &second));
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.kept_snap_page(1).is_none(), "undo moved the revision");
}

// ------------------------------------------------------------------------------------------------------------------------------------- the guard of an edit

mod guard_of_an_edit {
    use std::sync::Arc;

    use diacraft_doc::{Cell, CellName, Document, Page, Section, Shape, ShapeList, Sheet, Value};
    use diacraft_sheetxml::model::{allocations_walked, held_bytes_of, whole_walks};
    use serde_json::json;

    use super::super::Session;
    use super::spec;
    use crate::error::{EngineError, Result};
    use crate::history::History;

    /// A drawing of `n` shapes that each have a sheet of their own, of ten sections, **and nothing in a shape equal to another's** (a reader that shares what is equal would make one
    /// of equal ones, and these stand for a drawing a reader that shares nothing makes: the model of the file a save wrote is then as large as the drawing).
    fn unshared(n: usize) -> Document {
        let mut doc = Document::empty();
        let mut page = Page::new(1, "P");
        let shapes: Vec<Arc<Shape>> = (0..n).map(|i| Arc::new(shape(u32::try_from(i + 1).unwrap()))).collect();
        page.shapes = ShapeList::from_vec(shapes);
        doc.pages.push(Arc::new(page));
        doc
    }

    fn shape(id: u32) -> Shape {
        let mut sheet = Sheet::default();
        sheet.cells.insert(CellName::new("c"), Cell::constant(Value::from_text(&format!("a{id}"))));
        for k in 0..10 {
            let mut section = Section::new(&format!("s{k}"), None);
            section.cells.insert(CellName::new("c"), Cell::constant(Value::from_text(&format!("{id}.{k}"))));
            sheet.sections.push(Arc::new(section));
        }
        let mut shape = Shape::new(id);
        shape.sheet = Arc::new(sheet);
        shape
    }

    /// A session that holds `doc`, counted whole as it is opened.
    fn opened(doc: Document) -> Session {
        let mut s = Session::new();
        s.replace_document("file.new", doc).expect("a document");
        s
    }

    /// The edit of shape `i`: its cell `c` is `text`, which makes a copy of the page's chunk of shapes, of the shape and of its sheet.
    fn write(s: &mut Session, i: usize, text: &str) -> Result<()> {
        s.edit("write", |doc| {
            let Some(page) = doc.pages.get_mut(0) else { return Ok(()) };
            let Some(shape) = Arc::make_mut(page).shapes.get_mut(i) else { return Ok(()) };
            Arc::make_mut(&mut Arc::make_mut(shape).sheet).cells.insert(CellName::new("c"), Cell::constant(Value::from_text(text)));
            Ok(())
        })
    }

    /// The edit of shape `i` that makes its sheet and sections its own: a copy of each.
    fn unshare(s: &mut Session, i: usize) -> Result<()> {
        s.edit("unshare", |doc| {
            let Some(page) = doc.pages.get_mut(0) else { return Ok(()) };
            let Some(shape) = Arc::make_mut(page).shapes.get_mut(i) else { return Ok(()) };
            for section in &mut Arc::make_mut(&mut Arc::make_mut(shape).sheet).sections {
                Arc::make_mut(section).del = !section.del;
            }
            Ok(())
        })
    }

    fn remove_a_shape(s: &mut Session, i: usize) -> Result<()> {
        s.edit("remove", |doc| {
            if let Some(page) = doc.pages.get_mut(0) {
                let page = Arc::make_mut(page);
                if page.shapes.len() > 3 {
                    page.shapes.remove(i % page.shapes.len());
                }
            }
            Ok(())
        })
    }

    fn duplicate_a_shape(s: &mut Session, i: usize) -> Result<()> {
        s.edit("duplicate", |doc| {
            if let Some(page) = doc.pages.get_mut(0) {
                let page = Arc::make_mut(page);
                if let Some(shape) = page.shapes.get(i % page.shapes.len().max(1)).cloned() {
                    page.shapes.push(shape);
                }
            }
            Ok(())
        })
    }

    thread_local! {
        /// How many times a group of three writes has run on this thread: the letter it writes.
        static ROUND: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
    }

    /// Three writes of 50,000 characters or more, each of a letter the group before it did not write, so that the group is a change however often it runs: a group that leaves the
    /// drawing as it was is taken back whole and ends no redo step, which is not what this group is here to show.
    fn three_writes_in_a_group(s: &mut Session, _: &serde_json::Value) -> Result<serde_json::Value> {
        let letter = ROUND.with(|round| {
            let n = round.get();
            round.set(n.wrapping_add(1));
            char::from(b'a' + n % 26).to_string()
        });
        s.grouped("Three writes", |s| {
            for k in 0..3 {
                write(s, 0, &letter.repeat(50_000 + k))?;
            }
            Ok(json!({}))
        })
    }

    fn one_write(s: &mut Session, _: &serde_json::Value) -> Result<serde_json::Value> {
        write(s, 5, "cccc")?;
        Ok(json!({}))
    }

    fn three_small_writes_in_a_group(s: &mut Session, _: &serde_json::Value) -> Result<serde_json::Value> {
        s.grouped("Three small writes", |s| {
            for k in 0..3 {
                write(s, k + 1, "dddd")?;
            }
            Ok(json!({}))
        })
    }

    fn a_group_that_fails(s: &mut Session, _: &serde_json::Value) -> Result<serde_json::Value> {
        let before = s.model_held();
        let outcome = s.grouped("Fails", |s| {
            write(s, 1, &"f".repeat(50_000))?;
            write(s, 2, &"f".repeat(50_000))?;
            Err::<(), _>(crate::error::failed("test", "the group goes wrong"))
        });
        assert!(outcome.is_err());
        // The count is the one before the group, or more if the group let go of undo steps; it is never less than what the documents hold.
        assert!(s.model_held() >= before && s.model_held() >= s.model_held_exact());
        Ok(json!({}))
    }

    /// The count that is kept is never less than the count of the documents the session holds, whole, and is it when nothing it met was doubtful.
    fn held_as_whole(s: &Session, context: &str) {
        assert_eq!(s.published_held(), s.model_held().unwrap_or(0), "what the session told the process is its count {context}");
        let (kept, exact, drifted) = (s.model_held().expect("held"), s.model_held_exact().expect("held"), s.model_held_drifted().expect("held"));
        assert!(kept >= exact, "a count under what is held: {kept} against {exact} {context}");
        if !drifted {
            assert_eq!(kept, exact, "no allocation was met twice, so the count is exact {context}");
        }
    }

    /// A small deterministic generator, for sequences that are the same on every run.
    fn next(state: &mut u64) -> u64 {
        *state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        *state >> 33
    }

    #[test]
    fn the_count_kept_is_the_count_of_the_documents_the_session_holds_through_edits_undo_redo_groups_and_the_limit_of_steps() {
        let group = spec("test.three_writes", three_writes_in_a_group);
        let mut s = opened(unshared(30));
        s.state.as_mut().unwrap().history = History::with_limit(8);
        held_as_whole(&s, "as opened");
        let mut random = 7u64;
        let mut ops = String::new();
        // Edits that write, unshare, undo, redo and group (the limit of steps lets the oldest go): every allocation the edits make is one the walk meets once, and the count is exact.
        for step in 0..300 {
            let r = next(&mut random);
            let i = usize::try_from(r % 30).unwrap();
            let did = match r % 10 {
                0..=3 => {
                    write(&mut s, i, &format!("v{}", r % 1000)).unwrap();
                    "write"
                }
                4 | 5 => {
                    unshare(&mut s, i).unwrap();
                    "unshare"
                }
                6 => {
                    let _ = s.execute("edit.undo", &json!({}));
                    "undo"
                }
                7 => {
                    let _ = s.execute("edit.redo", &json!({}));
                    "redo"
                }
                8 => {
                    // A group ends the redo steps, which the command's snapshot holds until the command ends: the count is then marked as one that may be more than what is held.
                    let redo_held = s.history().unwrap().redo_len() > 0;
                    s.run_spec(&group, &json!({})).unwrap();
                    if redo_held {
                        assert_eq!(s.model_held_drifted(), Some(true), "the redo steps that the command held were not taken from the count");
                        s.recount();
                    }
                    "group"
                }
                _ => {
                    // An edit that changes nothing leaves the count as it was.
                    let before = s.model_held();
                    s.edit_if("nothing", |_| Ok(((), false))).unwrap();
                    assert_eq!(s.model_held(), before);
                    "nothing"
                }
            };
            ops.push_str(&format!("{did} "));
            held_as_whole(&s, &format!("after step {step}: {ops}"));
            assert_eq!(s.model_held_drifted(), Some(false), "nothing of this kind is met twice, after step {step}: {ops}");
            assert!(s.history().unwrap().undo_len() <= 8);
        }
        // Shapes that are taken out and put in twice are the same allocation in two places: the count may be more than what is held, and is never less.
        let mut s = opened(unshared(30));
        s.state.as_mut().unwrap().history = History::with_limit(8);
        for step in 0..300 {
            let r = next(&mut random);
            match r % 8 {
                0 | 1 => write(&mut s, usize::try_from(r % 30).unwrap(), &format!("v{}", r % 1000)).unwrap(),
                2 => remove_a_shape(&mut s, usize::try_from(r % 30).unwrap()).unwrap(),
                3 | 4 => duplicate_a_shape(&mut s, usize::try_from(r % 30).unwrap()).unwrap(),
                5 => {
                    let _ = s.execute("edit.undo", &json!({}));
                }
                6 => {
                    let _ = s.execute("edit.redo", &json!({}));
                }
                _ => unshare(&mut s, usize::try_from(r % 30).unwrap()).unwrap(),
            }
            held_as_whole(&s, &format!("after step {step}"));
        }
    }

    #[test]
    fn a_group_lets_go_of_the_documents_its_edits_replace_and_a_failed_one_puts_the_count_back() {
        let group = spec("test.three_writes", three_writes_in_a_group);
        let mut s = opened(unshared(10));
        let base = s.model_held().unwrap();
        s.run_spec(&group, &json!({})).unwrap();
        held_as_whole(&s, "after the group");
        assert_eq!(s.model_held_drifted(), Some(false));
        // The three values are 150 KB, and the drawing holds the last of them, and the document before the group (the step it recorded) holds none: the edits between were let go of.
        let held = s.model_held().unwrap();
        assert!(held >= base + 50_000 && held < base + 100_000, "{held} against {base}: one value of the three and a copy of the page and the shape");
        assert_eq!(s.history().unwrap().undo_len(), 1);
        let fails = spec("test.group_fails", a_group_that_fails);
        s.run_spec(&fails, &json!({})).unwrap();
        assert_eq!(s.model_held_exact(), Some(held), "the group that failed is put back with the document");
    }

    /// A group that writes a cell and writes its old text back.
    fn there_and_back_in_a_group(s: &mut Session, _: &serde_json::Value) -> Result<serde_json::Value> {
        s.grouped("There and back", |s| {
            write(s, 1, &"t".repeat(50_000))?;
            write(s, 1, "first")?;
            Ok(json!({}))
        })
    }

    /// A group whose edits leave the drawing as it was is taken back whole: the drawing is the very document it was, no step is recorded and the redo steps stay, the
    /// revision is the one it was, and the count is the one the group began with, exactly (no undo step was let go of, so nothing is in doubt), also with a history that has let go of its
    /// oldest steps already (a limit of 8 steps, and 12 edits).
    #[test]
    fn a_group_that_leaves_the_drawing_as_it_was_is_taken_back_with_the_count_it_began_with() {
        let group = spec("test.there_and_back", there_and_back_in_a_group);
        for steps in [1_usize, 12] {
            let mut s = opened(unshared(10));
            s.state.as_mut().unwrap().history = History::with_limit(8);
            for k in 0..steps {
                write(&mut s, 1, if k == steps - 1 { "first" } else { "other" }).unwrap();
            }
            write(&mut s, 2, "second").unwrap();
            s.execute("edit.undo", &json!({})).unwrap();
            let (document, undo, redo, revision, held) = (
                Arc::clone(&s.state.as_ref().unwrap().doc),
                s.history().unwrap().undo_len(),
                s.history().unwrap().redo_len(),
                s.revision(),
                s.model_held(),
            );
            assert_eq!(redo, 1, "a step to redo, which a group that changes something ends");
            s.run_spec(&group, &json!({})).unwrap();
            let state = s.state.as_ref().unwrap();
            assert!(Arc::ptr_eq(&state.doc, &document), "{steps}: the very document it was");
            assert_eq!(
                (s.history().unwrap().undo_len(), s.history().unwrap().redo_len(), s.revision()),
                (undo, redo, revision),
                "{steps}: no step, the redo step stays, no revision"
            );
            assert_eq!((s.model_held(), s.model_held_drifted()), (held, Some(false)), "{steps}: the count it began with, in no doubt");
            held_as_whole(&s, "after a group taken back");
            s.execute("edit.redo", &json!({})).unwrap();
        }
    }

    #[test]
    fn a_save_keeps_the_count_exact_with_the_model_it_reads_the_file_as_held_beside_the_drawing() {
        let mut s = opened(unshared(20));
        write(&mut s, 0, "x").unwrap();
        let before = s.model_held().unwrap();
        let dir = std::env::temp_dir().join(format!("diacraft-held-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        s.execute("file.save_as", &json!({ "path": dir.join("a.vsdx").to_string_lossy() })).expect("a drawing of 20 shapes is saved");
        let _ = std::fs::remove_dir_all(&dir);
        held_as_whole(&s, "after a save");
        assert!(
            s.model_held().unwrap() > before,
            "the model the file reads as is held beside the drawing: {} against {before}",
            s.model_held().unwrap()
        );
        write(&mut s, 1, "y").unwrap();
        held_as_whole(&s, "after an edit after a save");
    }

    /// The edit of shape `i` that adds `n` cells to its sheet: structure that nothing takes away.
    fn add_cells(s: &mut Session, i: usize, n: usize) -> Result<()> {
        s.edit("add cells", |doc| {
            let Some(page) = doc.pages.get_mut(0) else { return Ok(()) };
            let Some(shape) = Arc::make_mut(page).shapes.get_mut(i) else { return Ok(()) };
            let sheet = Arc::make_mut(&mut Arc::make_mut(shape).sheet);
            for k in 0..n {
                sheet.cells.insert(CellName::new(&format!("n{k}")), Cell::new());
            }
            Ok(())
        })
    }

    #[test]
    fn a_document_that_is_a_step_and_the_saved_one_is_let_go_of_once_when_the_edit_is_judged_past_the_figure() {
        // The opened drawing is the saved one (`clean`) and, after one edit, the oldest undo step: the same document in two of the places the judgement lists. Let go of twice it would free
        // what it does not, and the edit that adds cells beside what it takes away would be judged as no larger.
        let mut s = opened(unshared(500));
        s.model_most = s.model_held().unwrap() / 2;
        write(&mut s, 0, "bbbb").expect("an edit that makes the drawing no larger");
        assert!(add_cells(&mut s, 3, 200).is_err(), "200 cells more than the drawing had, past the figure");
        write(&mut s, 1, "cccc").expect("and an edit that makes it no larger is made");
    }

    fn two_writes_after_an_undo(s: &mut Session, _: &serde_json::Value) -> Result<serde_json::Value> {
        write(s, 5, "dddd")?;
        write(s, 6, "eeee")?;
        Ok(json!({}))
    }

    #[test]
    fn an_edit_after_one_that_ended_the_redo_steps_in_the_same_command_is_judged_with_the_redo_steps_the_command_still_holds() {
        let mut s = opened(unshared(500));
        s.model_most = s.model_held().unwrap() / 2;
        write(&mut s, 0, "bbbb").unwrap();
        write(&mut s, 1, "cccc").unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.history().unwrap().redo_len(), 1);
        // The first of the two edits ends the redo step, which the snapshot of the command still holds; the second must be judged with it, and is made.
        s.run_spec(&spec("test.two_writes", two_writes_after_an_undo), &json!({})).expect("two edits that make the drawing no larger");
        held_as_whole(&s, "after the command");
    }

    /// The edit that puts shape `i` in front of the others of its page (a z-order change): it is the same allocation in another place, and the shapes after it shift by one.
    fn to_the_front(s: &mut Session, i: usize) -> Result<()> {
        s.edit("order", |doc| {
            let page = Arc::make_mut(doc.pages.get_mut(0).expect("the page"));
            if let Some(shape) = page.shapes.remove(i) {
                page.shapes.push(shape);
            }
            Ok(())
        })
    }

    fn delete_one(s: &mut Session, i: usize) -> Result<()> {
        s.edit("delete", |doc| {
            Arc::make_mut(doc.pages.get_mut(0).expect("the page")).shapes.remove(i);
            Ok(())
        })
    }

    /// The edit that makes a copy of shape `i`: a shape of its own that holds the sheet the first holds.
    fn duplicate_one(s: &mut Session, i: usize, id: u32) -> Result<()> {
        s.edit("duplicate", |doc| {
            let page = Arc::make_mut(doc.pages.get_mut(0).expect("the page"));
            let mut copy = Shape::clone(page.shapes.get(i).expect("a shape"));
            copy.id = id;
            page.shapes.push(Arc::new(copy));
            Ok(())
        })
    }

    /// The edit that takes shapes `i` and the one after it into a group (a shape of its own that holds them as members).
    fn group_two(s: &mut Session, i: usize, id: u32) -> Result<()> {
        s.edit("group", |doc| {
            let page = Arc::make_mut(doc.pages.get_mut(0).expect("the page"));
            let (a, b) = (page.shapes.remove(i).expect("a shape"), page.shapes.remove(i).expect("the next"));
            let mut group = Shape::new(id);
            group.children = ShapeList::from_vec(vec![a, b]);
            page.shapes.push(Arc::new(group));
            Ok(())
        })
    }

    /// What a count of the whole of the session's documents looks at: the allocations of one walk of them all.
    fn whole(s: &Session) -> u64 {
        let before = allocations_walked();
        s.model_held_exact().expect("held");
        allocations_walked() - before
    }

    #[test]
    fn an_edit_that_puts_shapes_in_other_places_costs_what_it_changed_and_no_walk_of_the_drawing() {
        let mut s = opened(unshared(3_000));
        let all = whole(&s);
        assert!(all > 30_000, "the allocations of the drawing: {all}");
        type Op = fn(&mut Session) -> Result<()>;
        let ops: [(&str, Op); 4] = [
            ("order to the front", |s| to_the_front(s, 10)),
            ("delete", |s| delete_one(s, 20)),
            ("duplicate", |s| duplicate_one(s, 30, 90_001)),
            ("group", |s| group_two(s, 40, 90_002)),
        ];
        for (name, op) in ops {
            let (walks, touched) = (whole_walks(), allocations_walked());
            op(&mut s).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(whole_walks(), walks, "{name}: no walk of the drawing");
            let walked = allocations_walked() - touched;
            // The shapes that shifted (a list of 3,000 in chunks of 64), each looked at once, and none of their sheets.
            assert!(walked < all / 5, "{name}: what was looked at, {walked}, is not the drawing, {all}");
            held_as_whole(&s, name);
        }
        // Past the figure, the edits that make the drawing no larger are made as cheaply, and the two that add a shape are refused, in words, as cheaply.
        s.model_most = s.model_held().unwrap() / 2;
        let (walks, touched) = (whole_walks(), allocations_walked());
        to_the_front(&mut s, 11).expect("the same shapes in another order");
        delete_one(&mut s, 21).expect("a shape the fewer");
        let refused = [duplicate_one(&mut s, 31, 90_003), group_two(&mut s, 41, 90_004)];
        assert!(refused.iter().all(Result::is_err), "a shape more, past the figure: {refused:?}");
        assert_eq!(whole_walks(), walks, "no walk of the drawing, for an edit made or refused");
        // Each is judged against the steps the history holds and the saved drawing, which share what they do not hold at the places that shifted: a few times the shapes that moved, and not the
        // drawing for each (the whole of it is {all}).
        assert!(allocations_walked() - touched < 2 * all, "four edits looked at {} allocations", allocations_walked() - touched);
    }

    /// The edit that replaces shape `i` by a copy of it (a shape of its own that holds the sheet the first held).
    fn replace_by_a_copy(s: &mut Session, i: usize, id: u32) -> Result<()> {
        s.edit("replace", |doc| {
            let page = Arc::make_mut(doc.pages.get_mut(0).expect("the page"));
            let mut copy = Shape::clone(page.shapes.get(i).expect("a shape"));
            copy.id = id;
            page.shapes.remove(i);
            page.shapes.push(Arc::new(copy));
            Ok(())
        })
    }

    #[test]
    fn what_a_copy_shares_is_the_doubt_of_the_count_and_the_count_is_made_whole_only_when_the_doubt_could_be_what_passes_the_figure() {
        let mut s = opened(unshared(500));
        let held = s.model_held().unwrap();
        // A figure a little over what is held. Each copy of a shape holds the sheet that is held already, which the count takes as new: it is over what is held by thirty sheets, and
        // the figure is passed by the count and not by the drawing.
        s.model_most = held + 100_000;
        let walks = whole_walks();
        for k in 0..30 {
            let before = whole_walks();
            duplicate_one(&mut s, 3, 90_000 + k).unwrap_or_else(|e| panic!("copy {k}: {e}"));
            if whole_walks() > before {
                assert_eq!(s.model_held_drifted(), Some(false), "a count made whole has no doubt left (copy {k})");
            }
        }
        let made_whole = whole_walks() - walks;
        assert!(
            (1..=8).contains(&made_whole),
            "the count was made whole {made_whole} times in thirty copies, when it passed the figure and the doubt could be the whole of it"
        );
        held_as_whole(&s, "after thirty copies");
        // Far past the figure, the doubt of a copy is not the whole of what passes it, and nothing is walked: the copy is refused for the shape it adds, with no count of the drawing.
        s.model_most = held / 2;
        let walks = whole_walks();
        assert!(duplicate_one(&mut s, 4, 91_000).is_err());
        // An edit that takes a shape away and puts a copy of it in its place makes the drawing no larger, though the copy's sheet is counted as new beside the one it shares.
        replace_by_a_copy(&mut s, 5, 91_001).expect("the sheet it shares is the doubt, and is not taken for growth");
        assert_eq!(whole_walks(), walks, "no walk of the drawing");
    }

    #[test]
    fn an_edit_that_counted_a_great_deal_that_may_be_held_already_is_judged_by_the_two_drawings_counted_whole() {
        // A shape that holds a sheet of 4,000 sections (about 10 MB), and the edit that replaces it by a copy of itself: the copy's sheet is the one held, and is counted as new beside it.
        let mut doc = unshared(20);
        let mut sheet = Sheet::default();
        for k in 0..4_000 {
            let mut section = Section::new(&format!("h{k}"), None);
            section.cells.insert(CellName::new("c"), Cell::new());
            sheet.sections.push(Arc::new(section));
        }
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        Arc::make_mut(page.shapes.get_mut(7).unwrap()).sheet = Arc::new(sheet);
        let mut s = opened(doc);
        s.model_most = s.model_held().unwrap() / 2;
        let walks = whole_walks();
        replace_by_a_copy(&mut s, 7, 92_000).expect("the drawing is no larger");
        assert_eq!(
            whole_walks() - walks,
            2,
            "what it counted that may be held already was more than the doubt that is taken on trust: the two drawings are counted whole"
        );
    }

    #[test]
    fn the_document_that_was_opened_is_held_by_the_model_a_save_compares_with_and_is_not_freed_with_the_step_that_is_it() {
        // Before a save the model the file reads as is the document that was opened, and after the first edit that document is the oldest undo step as well: letting go of the step
        // frees nothing, and the count says so.
        let mut s = Session::new();
        s.replace_document_from("file.open", unshared(300), crate::file::OpenFile::new(std::path::Path::new("a.vsdx"), Vec::new())).unwrap();
        s.model_most = s.model_held().unwrap() + 60_000;
        for k in 0..12 {
            write(&mut s, k, &format!("v{k}")).unwrap_or_else(|e| panic!("edit {k}: {e}"));
            held_as_whole(&s, &format!("after edit {k}"));
        }
    }

    #[test]
    fn the_session_tells_the_process_what_it_holds_and_the_guard_keeps_its_drawing_within_the_figure_less_what_another_session_holds() {
        let base = crate::memory::held();
        let mut b = opened(unshared(500));
        let held = b.model_held().unwrap();
        assert_eq!(crate::memory::held() - base, held, "a session with no file holds what it counts, in the ledger of the process");
        // A figure that leaves room for 200 cells more, beside nothing else.
        b.model_most = held + held / 2;
        add_cells(&mut b, 2, 200).expect("room for them");
        let after_one = b.model_held().unwrap();
        assert_eq!(crate::memory::held() - base, after_one, "the hold moves with each edit");
        b.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(crate::memory::held() - base, b.model_held().unwrap(), "and with an undo, which leaves the count as it was");
        // Another session in the process holds a drawing of the same size: what is left of the figure for the first is what that one does not hold, and 200 cells more are refused.
        let a = opened(unshared(500));
        let others = a.model_held().unwrap();
        assert_eq!(crate::memory::held() - base, b.model_held().unwrap() + others);
        let refused = add_cells(&mut b, 3, 200).expect_err("the other session holds what the figure left");
        assert!(refused.to_string().contains("hold more than"), "{refused}");
        // When the other session is gone its hold is given back, and the edit is made.
        drop(a);
        assert_eq!(crate::memory::held() - base, b.model_held().unwrap(), "a dropped session gives its hold back");
        add_cells(&mut b, 3, 200).expect("room for them again");
    }

    #[test]
    fn the_model_a_save_keeps_is_in_the_figure_of_the_session_and_not_in_the_ledger_of_the_process() {
        // After a save the count has the saved model beside the drawing (a second model, which the process's ledger holds in the file the save wrote), and what the session
        // publishes is what the drawing and its steps have grown by: a read beside it is refused only where it was refused before the count.
        let mut s = opened(unshared(300));
        write(&mut s, 0, "bbbb").unwrap();
        let dir = std::env::temp_dir().join(format!("diacraft-held-p200-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let before = s.model_held().unwrap();
        s.execute("file.save_as", &json!({ "path": dir.join("a.vsdx").to_string_lossy() })).expect("a drawing of 300 shapes is saved");
        let _ = std::fs::remove_dir_all(&dir);
        let state = s.state.as_ref().unwrap();
        let (extra, opened) = (state.baseline_extra, state.opened);
        assert!(extra > before / 2, "the saved model is a second model beside the drawing: {extra} against {before}");
        assert_eq!(
            s.hold.bytes(),
            s.published_held().saturating_sub(opened).saturating_sub(extra),
            "the hold is the count less the open's and the saved model"
        );
        // A drawing that no file made holds its whole count in the ledger, the saved model apart: the drawing and its steps, as they were before the save and a step more.
        assert!(
            s.hold.bytes() + before / 50 >= before && s.hold.bytes() < before + before / 50,
            "the drawing and its steps, without the saved model: {} against {before}",
            s.hold.bytes()
        );
        // The figure of the session's own edits has the saved model besides, and a drawing that fits is edited.
        s.model_most = before + 40_000;
        add_cells(&mut s, 2, 50).expect("the figure is what the drawing and its steps may hold");
    }

    #[test]
    fn the_figure_of_a_session_opened_from_a_file_is_not_less_what_its_own_file_holds() {
        // The file's bytes and the model it read are held in the ledger of the process for as long as the drawing is open; they are the drawing's own, and what the figure is less is what
        // the others hold. A figure with room for a few cells is not taken from by 8 MB of the session's own file.
        let mut s = Session::new();
        let file = crate::file::OpenFile::new(std::path::Path::new("a.vsdx"), vec![0u8; 5_000_000]).with_model_bytes(3_000_000);
        s.replace_document_from("file.open", unshared(500), file).unwrap();
        assert!(s.held_memory().drawing >= 8_000_000, "the file holds its bytes and the model: {}", s.held_memory().drawing);
        s.model_most = s.model_held().unwrap() + 40_000;
        add_cells(&mut s, 2, 50).expect("a few cells fit the figure, and the session's own file is not taken from it");
    }

    /// A history of the two newest steps of `s` that holds what a cap of 130 bytes lets two steps of 60 hold, and a count made whole beside it.
    fn with_a_history_at_a_cap(s: &mut Session) {
        let steps: Vec<Arc<Document>> = s.history().unwrap().undo_documents().cloned().collect();
        let mut history = History::with_limits(1_000, 130);
        for doc in &steps[steps.len() - 2..] {
            history.record(Arc::from("edit"), Arc::clone(doc), 60);
        }
        s.state.as_mut().unwrap().history = history;
        s.recount();
        assert_eq!(s.model_held_drifted(), Some(false), "a count made whole has no doubt");
    }

    #[test]
    fn a_step_the_history_lets_go_of_for_its_own_bytes_is_taken_from_the_count_and_the_next_edit_past_the_figure_makes_no_recount() {
        let mut s = opened(unshared(300));
        for k in 0..3 {
            write(&mut s, k, "bbbb").unwrap();
        }
        with_a_history_at_a_cap(&mut s);
        // An edit whose step is charged more than the cap (the charge is the step's own, a few kilobytes) lets the two older steps go: what they free is taken from the count, which
        // is the count of what is held and is not marked as one that may be more.
        write(&mut s, 5, "cccc").unwrap();
        assert_eq!(s.history().unwrap().undo_len(), 1, "the cap let the older steps go");
        assert_eq!(s.model_held_drifted(), Some(false), "the count took from itself what the steps freed");
        held_as_whole(&s, "after the history cut its steps");
        // The next edit past the figure is judged by the count and not by a walk of the drawing.
        let walks = whole_walks();
        s.model_most = s.model_held().unwrap() / 2;
        write(&mut s, 6, "dddd").expect("the same size, past the figure");
        assert_eq!(whole_walks(), walks, "no recount: the count was exact");
        held_as_whole(&s, "after the edit past the figure");
    }

    #[test]
    fn a_step_the_history_lets_go_of_as_an_undo_puts_one_in_the_other_direction_is_taken_from_the_count() {
        let mut s = opened(unshared(300));
        for k in 0..3 {
            write(&mut s, k, "bbbb").unwrap();
        }
        with_a_history_at_a_cap(&mut s);
        // The first undo puts one step in the redo direction, which keeps the newest whatever it holds: nothing goes.
        s.step_history("edit.undo", true).unwrap();
        assert_eq!(s.model_held_drifted(), Some(false));
        // The second puts another beside it, and the two hold more than the cap: the farthest goes, and the count takes what it frees.
        let walks = whole_walks();
        s.step_history("edit.undo", true).unwrap();
        assert_eq!((s.history().unwrap().undo_len(), s.history().unwrap().redo_len()), (0, 1), "the cap let a redo step go");
        assert_eq!(s.model_held_drifted(), Some(false), "on an undo, the count is exact");
        assert_eq!(whole_walks(), walks, "and it was not made again");
        held_as_whole(&s, "after an undo that cut a step");
    }

    #[test]
    fn the_steps_the_history_cuts_for_an_edit_a_command_makes_are_let_go_of_by_its_snapshot_and_taken_from_the_count() {
        // A command keeps a snapshot of the history, which holds the steps the history cuts as the command's edit records its own: the snapshot lets them go as it does for what a plan lets
        // go of, so that what they free is freed and the count says so.
        let mut s = opened(unshared(300));
        for k in 0..3 {
            write(&mut s, k, "bbbb").unwrap();
        }
        with_a_history_at_a_cap(&mut s);
        let walks = whole_walks();
        s.run_spec(&spec("test.one_write", one_write), &json!({})).unwrap();
        assert_eq!(s.history().unwrap().undo_len(), 1, "the cap let the older steps go");
        assert_eq!(s.model_held_drifted(), Some(false), "the snapshot let them go too, and the count took what they freed");
        assert_eq!(whole_walks(), walks, "no recount");
        held_as_whole(&s, "after a command whose edit made the history cut steps");
    }

    #[test]
    fn a_step_the_history_lets_go_of_inside_a_command_is_held_by_the_commands_snapshot_and_the_count_says_it_may_be_more() {
        // A command keeps a snapshot of the history to put back if it fails, so a step the history cuts while it runs is freed when it ends and not now.
        let mut s = opened(unshared(300));
        for k in 0..3 {
            write(&mut s, k, "bbbb").unwrap();
        }
        with_a_history_at_a_cap(&mut s);
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!((s.history().unwrap().undo_len(), s.history().unwrap().redo_len()), (0, 1), "the cap let a redo step go");
        assert_eq!(s.model_held_drifted(), Some(true), "the snapshot held the step the count took");
    }

    #[test]
    fn an_edit_after_a_save_does_not_walk_the_model_the_save_kept_and_an_undo_that_finds_no_step_says_why() {
        let mut s = opened(unshared(1_500));
        let dir = std::env::temp_dir().join(format!("diacraft-held-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write(&mut s, 0, "bbbb").unwrap();
        s.execute("file.save_as", &json!({ "path": dir.join("a.vsdx").to_string_lossy() })).expect("a drawing of 1,500 shapes is saved");
        let _ = std::fs::remove_dir_all(&dir);
        // The model the file reads as is held beside the drawing, and shares nothing with it: the drawing and it are past the figure, and the drawing alone is not.
        // A figure that the drawing and its steps are within and that the saved model beside them passes: the drawing is not refused a shape because it was saved.
        let held = s.model_held().unwrap();
        s.model_most = held * 3 / 4;
        duplicate_one(&mut s, 3, 93_000)
            .expect("a shape more: the figure is what the drawing and its steps may hold, and the saved model is held beside it");
        // The figure is what the drawing and its steps may hold: a figure of nothing leaves the drawing and its steps past it.
        s.model_most = 0;
        let all = whole(&s);
        let (walks, touched) = (whole_walks(), allocations_walked());
        for k in 0..5 {
            write(&mut s, 10 + k, "cccc").unwrap_or_else(|e| panic!("edit {k}: {e}"));
        }
        assert_eq!(whole_walks(), walks, "no walk of the drawing");
        let walked = allocations_walked() - touched;
        assert!(
            walked < all / 4,
            "five edits after a save look at {walked} of the {all} allocations of what is held: the model the save kept is not walked"
        );
        held_as_whole(&s, "after the edits");
        // The steps were let go of to keep the drawing within its memory, and an undo that finds none says so.
        assert!(s.history().unwrap().drawing_was_cut());
        while s.history().unwrap().undo_len() > 0 {
            s.execute("edit.undo", &json!({})).unwrap();
        }
        let refused = s.execute("edit.undo", &json!({})).expect_err("no step is left");
        assert!(refused.to_string().contains("the older steps were let go to keep the drawing within its memory"), "{refused}");
    }

    #[test]
    fn the_redo_steps_an_edit_ends_are_let_go_of_by_the_count() {
        let mut s = opened(unshared(10));
        for k in 0..5 {
            write(&mut s, k, &"r".repeat(30_000)).unwrap();
        }
        let top = s.model_held().unwrap();
        for _ in 0..3 {
            s.execute("edit.undo", &json!({})).unwrap();
        }
        assert_eq!(s.model_held(), Some(top), "an undo moves a document and holds the same");
        write(&mut s, 7, "short").unwrap();
        held_as_whole(&s, "after an edit that ended three redo steps");
        assert_eq!(s.history().unwrap().redo_len(), 0);
        assert!(
            s.model_held().unwrap() < top,
            "the strings of the three steps that are gone are let go of: {} against {top}",
            s.model_held().unwrap()
        );
    }

    #[test]
    fn past_the_figure_the_oldest_undo_steps_make_room_and_an_edit_that_cannot_be_made_to_fit_is_refused_with_nothing_changed() {
        let mut s = opened(unshared(10));
        // Each edit writes a value of 100 KB: a new string, held by the step before it until that step is let go of.
        write(&mut s, 0, &"a".repeat(100_000)).unwrap();
        let base = s.model_held().unwrap();
        s.model_most = base + 450_000;
        for k in 0..30 {
            write(&mut s, 0, &format!("{}{k}", "b".repeat(100_000))).unwrap_or_else(|e| panic!("edit {k}: {e}"));
            assert!(s.model_held().unwrap() <= s.model_most, "past the figure after edit {k}: {}", s.model_held().unwrap());
            held_as_whole(&s, &format!("after edit {k}"));
        }
        let steps = s.history().unwrap().undo_len();
        assert!((2..=5).contains(&steps), "the undo steps that fit beside the drawing: {steps}");
        // An edit that makes the drawing alone more than the figure: no step to let go of makes room, and it is refused in words.
        let (document, revision, held) = (s.document().cloned(), s.revision(), s.model_held());
        let refused = write(&mut s, 1, &"c".repeat(1_000_000)).expect_err("a value of 1 MB when the figure is 450 KB over the drawing");
        let EngineError::Failed { msg, .. } = &refused else { panic!("{refused:?}") };
        assert!(msg.contains("hold more than") && msg.contains("Nothing was changed"), "{msg}");
        assert_eq!((s.document().cloned(), s.revision(), s.model_held(), s.history().unwrap().undo_len()), (document, revision, held, steps));
        // The drawing is past the figure now (another program's file made it so): an edit that makes it no larger is made, with the undo steps let go of that it needs.
        s.model_most = s.model_held_exact().unwrap() / 2;
        write(&mut s, 0, &format!("{}!", "d".repeat(100_000))).expect("a value of the length of the one it replaces");
        assert!(s.history().unwrap().undo_len() <= 1, "no step beside a drawing that is past the figure: {}", s.history().unwrap().undo_len());
        held_as_whole(&s, "past the figure");
        // And one that makes it larger is refused.
        let long = write(&mut s, 0, &"e".repeat(20_000_000)).expect_err("characters past the one slack of an edit make the drawing larger");
        let words = long.to_string();
        assert!(
            words.contains("the text of the cells") && words.contains("a shorter text") && !words.contains("share a sheet"),
            "worded for the characters: {words}"
        );
        let wide = add_cells(&mut s, 2, 3_000).expect_err("cells that nothing takes away, past the figure");
        let words = wide.to_string();
        assert!(words.contains("of shapes, sheets and sections") && words.contains("share a sheet"), "worded for the structure: {words}");
        write(&mut s, 0, &"e".repeat(100_100)).expect("a few more characters than the value it replaces are not a larger drawing");
    }

    /// A drawing read from a VSDX file (its stamp is pending, and keeps the document it was read as), edited ten times with room for eight steps, so that **no step holds the document it was
    /// read as any more**, and saved: the document the stamp keeps is held by nothing else (the baseline is the model the save read back). `resolved` asks for the stamp, which lets go of it.
    fn read_edited_and_saved(n: usize, resolved: bool) -> Session {
        let path = std::path::Path::new("a.vsdx");
        let mut s = Session::new();
        s.replace_document_from("file.open", unshared(n), crate::file::OpenFile::new(path, Vec::new()).with_pending_content_stamp())
            .expect("a document");
        s.state.as_mut().expect("a state").history = History::with_limit(8);
        for k in 0..10 {
            write(&mut s, k % 3, &format!("v{k}")).expect("an edit");
        }
        assert_eq!(s.history().unwrap().undo_len(), 8, "the step that held the document the drawing was read as is gone");
        let saved = s.save_inputs().expect("inputs").doc;
        s.mark_saved(&saved, crate::file::OpenFile::new(path, Vec::new()), Document::clone(&saved));
        if resolved {
            let _ = s.content_stamp();
            assert!(!s.content_stamp_pending());
        }
        s
    }

    #[test]
    fn the_document_a_pending_stamp_keeps_is_counted_by_the_session_once_nothing_else_holds_it() {
        // (The blocker of the re-review: a kept document is memory, and was held by no count.)
        let kept = read_edited_and_saved(200, false);
        assert!(kept.content_stamp_pending());
        let freed = read_edited_and_saved(200, true);
        let (with, without) = (kept.model_held_exact().unwrap(), freed.model_held_exact().unwrap());
        assert!(with > without, "the document the stamp keeps is in the count: {with} against {without} once the stamp has let go of it");
        held_as_whole(&kept, "with the stamp pending, after the save");
        assert_eq!(kept.model_held(), Some(with), "the count kept is the count of the documents whole");
        // Counted by the allocations it holds **that nothing else does**: the document shares the shapes the edits did not copy with the drawing, and those are counted once.
        let drawing = kept.document().expect("a document");
        let alone = held_bytes_of([drawing], u64::MAX);
        let read = kept
            .state
            .as_ref()
            .and_then(|state| state.source.as_ref())
            .and_then(|file| file.content_stamp_cell().read_model())
            .expect("the document kept");
        let both = held_bytes_of([drawing, &*read], u64::MAX);
        assert!(
            both < alone + held_bytes_of([&*read], u64::MAX),
            "the shapes the edits did not copy are counted once: {both} against {alone} + the document alone"
        );
        // The process is told what the session holds, **the document the stamp keeps included** (it is not the model a save keeps, which the file's own hold stands for): the count less what
        // it was when the file was opened and less that model.
        let state = kept.state.as_ref().expect("a state");
        let model_a_save_keeps = state.baseline_extra;
        assert_eq!(kept.hold.bytes(), with.saturating_sub(state.opened).saturating_sub(model_a_save_keeps));
        let freed_extra = freed.state.as_ref().expect("a state").baseline_extra;
        assert_eq!(
            model_a_save_keeps, freed_extra,
            "the model a save keeps is the same with the stamp pending and made: the kept document is not part of it"
        );
    }

    #[test]
    fn a_stamp_that_has_been_made_lets_go_of_the_document_and_the_next_edit_makes_the_count_whole_again() {
        let mut s = read_edited_and_saved(200, false);
        held_as_whole(&s, "with the document kept");
        let with = s.model_held_exact().unwrap();
        let _ = s.content_stamp();
        assert!(!s.content_stamp_pending());
        let without = s.model_held_exact().unwrap();
        assert!(without < with, "the document is let go of when the stamp is made: {without} against {with}");
        // The count has not been told yet, and says so: it may be more than what is held.
        assert_eq!(s.model_held_drifted(), Some(true));
        assert!(s.model_held().unwrap() >= without);
        write(&mut s, 5, "after").expect("an edit");
        held_as_whole(&s, "after the stamp let go");
        // Past the figure the count is made whole before an edit is judged by it: the let-go document is not counted against it.
        s.model_most = without + 1_000_000;
        write(&mut s, 6, "after again").expect("an edit that fits once the document the stamp kept is let go of");
        held_as_whole(&s, "past the figure");
    }

    #[test]
    fn edits_undo_and_groups_after_a_save_do_not_take_from_the_count_what_the_kept_document_still_holds() {
        let group = spec("test.three_writes", three_writes_in_a_group);
        let mut s = read_edited_and_saved(200, false);
        let mut random = 11u64;
        for k in 0..30 {
            match next(&mut random) % 4 {
                0 => write(&mut s, usize::try_from(next(&mut random) % 200).unwrap(), &format!("w{k}")).expect("an edit"),
                1 => {
                    let _ = s.execute("edit.undo", &json!({}));
                }
                2 => {
                    let _ = s.execute("edit.redo", &json!({}));
                }
                _ => {
                    s.run_spec(&group, &json!({})).expect("a group");
                }
            }
            held_as_whole(&s, &format!("after step {k}"));
        }
        assert!(s.content_stamp_pending(), "none of it made the stamp");
        assert!(s.model_held_exact().unwrap() > held_bytes_of([s.document().unwrap()], u64::MAX), "the kept document is still held, and counted");
    }

    #[test]
    fn an_edit_refused_for_the_document_a_pending_stamp_keeps_is_judged_again_once_the_stamp_has_let_go_of_it() {
        // A drawing is not refused an edit because it was read from a foreign file and saved: the document the stamp keeps is memory the session counts, and **a refusal it may be the cause of
        // makes the stamp** (which lets go of it) and judges the edit again. The tightest figure at which a drawing whose stamp is made takes the edit is found by search; the same drawing with
        // its stamp pending, at that figure, takes it too, and has made the stamp.
        let accepts = |most: u64| {
            let mut s = read_edited_and_saved(200, true);
            s.model_most = most;
            add_cells(&mut s, 3, 3_000).is_ok()
        };
        let (mut low, mut high) = (0u64, read_edited_and_saved(200, true).model_held_exact().unwrap().saturating_mul(2));
        assert!(accepts(high) && !accepts(low), "the search starts from a figure that takes the edit and one that does not");
        while high - low > 1 {
            let middle = low + (high - low) / 2;
            if accepts(middle) {
                high = middle;
            } else {
                low = middle;
            }
        }
        let tight = high;
        let mut kept = read_edited_and_saved(200, false);
        assert!(kept.content_stamp_pending());
        kept.model_most = tight;
        add_cells(&mut kept, 3, 3_000).expect("the edit is made: what the stamp kept is let go of, and the drawing fits");
        assert!(!kept.content_stamp_pending(), "the refusal made the stamp, which let go of the document");
        assert!(kept.content_stamp().is_some(), "and the stamp is the content's as it was read");
        held_as_whole(&kept, "after the edit judged again");
        // Under that figure the drawing does not fit with or without the document: refused in words, as the drawing alone is, and nothing is changed.
        let mut tighter = read_edited_and_saved(200, false);
        tighter.model_most = low;
        let (document, revision) = (tighter.document().cloned(), tighter.revision());
        let refused = add_cells(&mut tighter, 3, 3_000).expect_err("the drawing alone does not fit");
        assert!(refused.to_string().contains("hold more than") && refused.to_string().contains("Nothing was changed"), "{refused}");
        assert_eq!((tighter.document().cloned(), tighter.revision()), (document, revision));
        // An edit that is made without the figure being in question does not make the stamp.
        let mut easy = read_edited_and_saved(200, false);
        add_cells(&mut easy, 3, 3_000).expect("an edit");
        assert!(easy.content_stamp_pending(), "no refusal, no stamp");
    }

    #[test]
    fn an_edit_past_the_figure_does_not_count_the_drawing_whole_and_neither_do_an_undo_a_redo_and_an_edit_after_them() {
        let walks = whole_walks();
        let mut s = opened(unshared(2_000));
        assert_eq!(whole_walks(), walks + 1, "the drawing is counted whole once, as it is opened");
        let held = s.model_held().unwrap();
        // The figure is half of what the drawing holds: it is past it before the first edit.
        s.model_most = held / 2;
        let walks = whole_walks();
        for k in 0..50 {
            write(&mut s, (k * 37) % 2_000, "bbbb").unwrap_or_else(|e| panic!("edit {k}: {e}"));
        }
        // The steps are let go of as the figure is passed, so there is one to undo and to redo.
        assert_eq!(s.history().unwrap().undo_len(), 1);
        s.execute("edit.undo", &json!({})).expect("an undo");
        s.execute("edit.redo", &json!({})).expect("a redo");
        s.execute("edit.undo", &json!({})).expect("an undo");
        write(&mut s, 5, "cccc").expect("the first edit after a redo");
        s.run_spec(&spec("test.three_small_writes", three_small_writes_in_a_group), &json!({})).expect("a group");
        assert_eq!(whole_walks(), walks, "no edit, undo, redo or group of a drawing past the figure counted it whole");
        assert!(s.history().unwrap().undo_len() <= 2, "past the figure the steps are let go of: {}", s.history().unwrap().undo_len());
        held_as_whole(&s, "after them");
        // The same below the figure.
        let mut s = opened(unshared(2_000));
        let walks = whole_walks();
        for k in 0..50 {
            write(&mut s, (k * 37) % 2_000, "bbbb").unwrap();
        }
        assert_eq!(whole_walks(), walks);
    }
}

/// (b), as the blocker of the re-review changed it: **a save does not make the stamp, and the stamp does not depend on the baseline the save replaces**. The stamp pending
/// keeps the document the drawing was read as; the baseline is replaced by the model the save read its file back as, and the stamp made after that is still the stamp of the content as it
/// was read, and a drawing that was edited reads as edited. This calls `mark_saved` itself, as the save of a VSDX file does after it has written.
#[test]
fn mark_saved_makes_no_pass_and_the_pending_stamp_is_still_that_of_the_document_it_was_read_as() {
    let dir = std::env::temp_dir().join(format!("diacraft-session-stamp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.vsdx");
    let mut first = session();
    first.execute("file.save_as", &json!({ "path": path.to_str().unwrap() })).unwrap();
    let mut s = Session::new();
    s.execute("file.open", &json!({ "path": path.to_str().unwrap() })).unwrap();
    assert!(s.content_stamp_pending(), "a named VSDX file arms the stamp");
    let original = s.state.as_ref().and_then(|state| state.baseline.clone()).unwrap();
    s.execute("page.add", &json!({"name": "Edited"})).unwrap();
    let edited = s.save_inputs().unwrap().doc;
    assert!(!Arc::ptr_eq(&original, &edited));
    // What the save does after it has written: the saved file, and the model it reads as, in the baseline's place.
    s.mark_saved(&edited, crate::file::OpenFile::new(&path, Vec::new()), Document::clone(&edited));
    assert!(s.content_stamp_pending(), "mark_saved does not make the stamp");
    assert_eq!(s.stamp_work().passes, 0, "and costs no pass");
    assert!(
        !Arc::ptr_eq(s.state.as_ref().and_then(|state| state.baseline.as_ref()).unwrap(), &original),
        "the baseline is the model the save read back"
    );
    let stamp_of = |doc: &Document| crate::nativefile::stamp_of(doc, 0).map(|made| made.0);
    let (before, after) = (stamp_of(&original), stamp_of(&edited));
    assert_ne!(before, after, "the two contents have two stamps");
    assert_eq!(s.content_stamp(), before, "the stamp is that of the content as it was read, and not of the content as it was saved");
    assert_eq!(s.stamp_work().passes, 1);
    assert_eq!(s.content_unchanged(), Some(false));
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------- a window setting turned is a change of the drawing and no undo step ----------

/// A new drawing: nothing is edited, so it is clean.
fn fresh() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s
}

/// Turns the grid off in `doc`, as `window.settings.set` does.
fn grid_off(doc: &mut Document) -> Result<((), bool)> {
    let changed =
        crate::view::set(doc, &diacraft_doc::WindowFlags { grid: Some(false), ..Default::default() }).map_err(|words| failed("test", words))?;
    Ok(((), changed))
}

#[test]
fn a_view_edit_marks_the_drawing_unsaved_and_makes_no_step_and_moves_no_revision() {
    let mut s = fresh();
    assert!(!s.is_dirty());
    let (revision, steps) = (s.revision(), s.history().unwrap().undo_len());
    let before = s.document().cloned().unwrap();
    let clean = s.save_inputs().unwrap().doc;
    s.edit_view("test", grid_off).unwrap();
    assert!(s.is_dirty(), "the drawing is changed");
    assert_eq!(s.history().unwrap().undo_len(), steps, "no undo step");
    assert_eq!(s.history().unwrap().redo_len(), 0);
    assert_eq!(s.revision(), revision, "no revision moves");
    assert!(!Arc::ptr_eq(&clean, &s.save_inputs().unwrap().doc));
    // Nothing of the drawing was copied: every page is the very page the drawing held, so nothing is calculated or drawn again for it.
    let after = s.document().unwrap();
    for (was, now) in before.pages.iter().zip(after.pages.iter()) {
        assert!(Arc::ptr_eq(was, now), "page {} is shared", was.id);
    }
    assert!(!crate::view::view_settings(after).grid);
    // A value the drawing shows already changes nothing: the document is the very one.
    let held = s.save_inputs().unwrap().doc;
    s.edit_view("test", grid_off).unwrap();
    assert!(Arc::ptr_eq(&held, &s.save_inputs().unwrap().doc), "a setting turned to what it is is no change");
}

#[test]
fn a_view_edit_that_fails_leaves_the_session_as_it_was() {
    let mut s = fresh();
    let before = snap(&s);
    let failure = s.edit_view("test", |doc| {
        grid_off(doc)?;
        Err::<((), bool), _>(failed("test", "refused after the copy was changed"))
    });
    assert!(failure.is_err());
    assert_eq!(snap(&s), before, "the change was made on a copy");
    assert!(!s.is_dirty());
    assert!(Session::new().edit_view("test", grid_off).unwrap_err().to_string().contains("no document open"));
}

/// A Snap & Glue option is no undo step either, but it decides which targets the snap reads, so **the revision moves** and a read kept for the old option is not found.
#[test]
fn a_snap_setting_edit_is_no_step_marks_the_drawing_unsaved_and_moves_the_revision_once() {
    let mut s = fresh();
    let (revision, steps) = (s.revision(), s.history().unwrap().undo_len());
    let kept = s.snap_page(1).unwrap();
    assert!(Arc::ptr_eq(&s.kept_snap_page(1).unwrap(), &kept));
    let dynamic_off = |doc: &mut Document| -> Result<((), bool)> {
        let mut now = crate::snap::SnapSettings::of(doc);
        let changed = now.dynamic_grid;
        now.dynamic_grid = false;
        now.apply(doc);
        Ok(((), changed))
    };
    s.edit_snap_setting("test", dynamic_off).unwrap();
    assert!(s.is_dirty(), "the drawing is changed");
    assert_eq!(s.history().unwrap().undo_len(), steps, "no undo step");
    assert_eq!(s.revision(), revision + 1, "the revision moves, once");
    assert!(s.kept_snap_page(1).is_none(), "the read kept for the old options is not found");
    assert!(!crate::snap::SnapSettings::of(s.document().unwrap()).dynamic_grid);
    // What `f` says changed nothing leaves the document and the revision as they were.
    let held = s.save_inputs().unwrap().doc;
    s.edit_snap_setting("test", dynamic_off).unwrap();
    assert!(Arc::ptr_eq(&held, &s.save_inputs().unwrap().doc));
    assert_eq!(s.revision(), revision + 1, "no change, no revision");
    // A change that fails leaves the session as it was, the revision with it.
    let before = snap(&s);
    let failure = s.edit_snap_setting("test", |doc| {
        let mut now = crate::snap::SnapSettings::of(doc);
        now.snap = false;
        now.apply(doc);
        Err::<((), bool), _>(failed("test", "refused after the copy was changed"))
    });
    assert!(failure.is_err());
    assert_eq!(snap(&s), before, "the change was made on a copy");
    assert!(Session::new().edit_snap_setting("test", dynamic_off).unwrap_err().to_string().contains("no document open"));
}

fn view_then_edit_in_a_group(s: &mut Session, _: &Value) -> Result<Value> {
    s.grouped("Both", |s| {
        s.edit_view("test", grid_off)?;
        add_page(s, "Add", "Added")?;
        Ok(json!({}))
    })
}

fn view_alone_in_a_group(s: &mut Session, _: &Value) -> Result<Value> {
    s.grouped("View only", |s| {
        s.edit_view("test", grid_off)?;
        Ok(json!({}))
    })
}

fn view_then_fail_in_a_group(s: &mut Session, _: &Value) -> Result<Value> {
    s.grouped("Fails", |s| {
        s.edit_view("test", grid_off)?;
        Err(failed("test", "the group fails"))
    })
}

#[test]
fn a_view_edit_in_a_group_is_no_step_of_the_group_and_goes_with_a_group_that_fails() {
    // A group of a setting alone records no step, and the drawing is changed.
    let mut s = fresh();
    assert!(!s.is_dirty());
    let steps = s.history().unwrap().undo_len();
    s.run_spec(&spec("test.view_alone", view_alone_in_a_group), &json!({})).unwrap();
    assert_eq!(s.history().unwrap().undo_len(), steps);
    assert!(s.is_dirty());
    assert!(!crate::view::view_settings(s.document().unwrap()).grid);
    // A group of a setting and an edit records the one step, and its undo takes the edit back and leaves the setting.
    let mut s = session();
    let steps = s.history().unwrap().undo_len();
    s.run_spec(&spec("test.view_and_edit", view_then_edit_in_a_group), &json!({})).unwrap();
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.document().unwrap().pages.len(), 2, "the page the group added is taken back");
    assert!(!crate::view::view_settings(s.document().unwrap()).grid, "the setting stays");
    // A group that fails takes the setting back with the rest.
    let mut s = fresh();
    let before = snap(&s);
    assert!(s.run_spec(&spec("test.view_fails", view_then_fail_in_a_group), &json!({})).is_err());
    assert_eq!(snap(&s), before);
    assert!(!s.is_dirty());
}
