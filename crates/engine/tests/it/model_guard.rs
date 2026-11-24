//! The scan's finding 1: **what an edit unshares is charged by the edit.** A drawing of many shapes that hold one sheet is a sheet and a pointer for
//! each shape, and an open counts what a file holds; the edit of a shape makes the shape's own copy of what it changes, and the edit of every shape makes a copy for
//! each. `Session::edit_if` refuses an edit that would make the drawing hold more than an open takes, in words, and puts nothing of it into the document. Each case runs
//! in a child under `ulimit -v 3000000`, where the loop that unshares every shape of the drawing below is 12 GB and aborts the process without the guard.
#[cfg(target_os = "linux")]
use std::process::Command;
use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Page, Row, Section, Shape, ShapeList, Sheet, Value};
use diacraft_engine::{EngineError, Session};
use diacraft_sheetxml::model::{EDIT_MODEL_BYTES, held_bytes};

use crate::blank_session;

const SCENARIO: &str = "ENGINE_MODEL_GUARD_SCENARIO";

/// A line of `/proc/self/status`, in kibibytes.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|r| r.trim_start_matches(':').split_whitespace().next().and_then(|x| x.parse().ok()))
        .unwrap_or(0)
}

/// A drawing of one page of `shapes` shapes that all hold one `Arc` of a sheet of 8,192 sections of a cell each: 14 MB of model once a shape has a copy of its own, and
/// a few hundred bytes for a shape as it is held.
fn most_shared(shapes: usize) -> Document {
    let section = |k: usize| {
        let mut section = Section::new(&format!("s{k}"), None);
        section.cells.insert(CellName::new("c"), Cell::new());
        Arc::new(section)
    };
    let sheet = Arc::new(Sheet { sections: (0..diacraft_doc::MAX_SECTIONS_PER_SHEET).map(section).collect(), ..Sheet::default() });
    let mut doc = Document::empty();
    let mut page = Page::new(1, "P");
    let list: Vec<Arc<Shape>> = (0..shapes)
        .map(|i| {
            let mut shape = Shape::new(u32::try_from(i + 1).unwrap());
            shape.sheet = Arc::clone(&sheet);
            Arc::new(shape)
        })
        .collect();
    page.shapes = ShapeList::from_vec(list);
    doc.pages.push(Arc::new(page));
    doc
}

/// What the session holds of the model, counted whole: the drawing, every step of its history and what a save keeps, each allocation once.
fn held_now(s: &Session) -> u64 {
    s.model_held_exact().expect("a document")
}

fn v(text: &str) -> Cell {
    Cell::constant(Value::from_text(text))
}

/// A diagram's shape: seven cells and a path of five rows, each shape its own sheet and section (what a drawing that another program's file made is).
fn diagram(n: usize) -> Document {
    let mut doc = Document::empty();
    let mut page = Page::new(1, "P");
    let list: Vec<Arc<Shape>> = (0..n)
        .map(|i| {
            let mut sheet = Sheet::default();
            for (k, name) in ["PinX", "PinY", "Width", "Height", "LocPinX", "LocPinY", "Angle"].iter().enumerate() {
                sheet.cells.insert(CellName::new(name), v(&format!("{}", i + k)));
            }
            let mut section = Section::new("Geometry", Some(0));
            for r in 0..5u32 {
                let mut row = Row { ix: Some(r + 1), row_type: Some(Arc::from(if r == 0 { "MoveTo" } else { "LineTo" })), ..Row::default() };
                row.cells.insert(CellName::new("X"), v(&format!("{}", (i as u32 + r) % 7)));
                row.cells.insert(CellName::new("Y"), v(&format!("{r}")));
                section.rows.push(Arc::new(row));
            }
            sheet.sections.push(Arc::new(section));
            let mut shape = Shape::new(u32::try_from(i + 1).unwrap());
            shape.sheet = Arc::new(sheet);
            Arc::new(shape)
        })
        .collect();
    page.shapes = ShapeList::from_vec(list);
    doc.pages.push(Arc::new(page));
    doc
}

/// One page of `n` places for one shape of `rows` rows of a path: the shape is one allocation, held in each place (what a duplicate makes).
fn path_duplicated(n: usize, rows: usize) -> Document {
    let mut section = Section::new("Geometry", Some(0));
    for r in 0..rows {
        let mut row =
            Row { ix: Some(u32::try_from(r + 1).unwrap()), row_type: Some(Arc::from(if r == 0 { "MoveTo" } else { "LineTo" })), ..Row::default() };
        row.cells.insert(CellName::new("X"), v(&format!("{}", r % 7)));
        row.cells.insert(CellName::new("Y"), v(&format!("{}", r % 5)));
        section.rows.push(Arc::new(row));
    }
    let mut sheet = Sheet::default();
    sheet.sections.push(Arc::new(section));
    let mut shape = Shape::new(1);
    shape.sheet = Arc::new(sheet);
    let shape = Arc::new(shape);
    let mut doc = Document::empty();
    let mut page = Page::new(1, "P");
    page.shapes = ShapeList::from_vec(vec![shape; n]);
    doc.pages.push(Arc::new(page));
    doc
}

/// One page of `n` places for one shape that keeps a tree of unknown XML (what a file's unknown elements are kept as for the save): 20,000 elements of two attributes, 60,000 items,
/// and the shape one allocation held in each place.
fn kept_duplicated(n: usize) -> Document {
    use diacraft_doc::{Passthrough, Placed, RawAttrs, RawElement, RawNode};
    let leaf = |i: usize| {
        RawNode::Element(RawElement {
            name: Arc::from("x:c"),
            attrs: RawAttrs::from_pairs([("a", "1"), ("b", format!("{i}").as_str())]),
            children: Vec::new(),
        })
    };
    let tree = RawNode::Element(RawElement {
        name: Arc::from("x:k"),
        attrs: RawAttrs::from_pairs([("xmlns:x", "urn:kept")]),
        children: (0..20_000).map(leaf).collect(),
    });
    let mut shape = Shape::new(1);
    shape.passthrough = Passthrough::from_nodes(vec![Placed { after: 0, node: tree }]);
    let shape = Arc::new(shape);
    let mut doc = Document::empty();
    let mut page = Page::new(1, "P");
    page.shapes = ShapeList::from_vec(vec![shape; n]);
    doc.pages.push(Arc::new(page));
    doc
}

/// The move of every shape: one cell of each (a select-all and a nudge). Each shape and each sheet is copied.
fn nudge_all(s: &mut Session, x: usize) -> Result<(), EngineError> {
    s.edit("nudge", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        for i in 0..page.shapes.len() {
            let shape = Arc::make_mut(page.shapes.get_mut(i).unwrap());
            Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("PinX"), v(&format!("{x}")));
        }
        Ok(())
    })
}

/// The edit of shape 0 that puts a value of 8 MiB, of its own, in a cell: the string is 8 MiB that the step before it holds the other of.
fn put_a_big_value(s: &mut Session, i: usize) -> Result<(), EngineError> {
    s.edit("put a value", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
        let mut text = format!("{i:08}");
        text.push_str(&"x".repeat((8 << 20) - text.len()));
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("Comment"), v(&text));
        Ok(())
    })
}

/// The edit of shape `i`: it changes every section of its sheet, which makes a copy of the sheet and of each section.
fn unshare(s: &mut Session, i: usize) -> Result<(), EngineError> {
    s.edit("unshare a shape", |doc| {
        let Some(page) = doc.pages.get_mut(0) else { return Ok(()) };
        let Some(shape) = Arc::make_mut(page).shapes.get_mut(i) else { return Ok(()) };
        let sheet = Arc::make_mut(&mut Arc::make_mut(shape).sheet);
        for section in &mut sheet.sections {
            Arc::make_mut(section).del = !section.del;
        }
        Ok(())
    })
}

/// The most-shared drawing there is, in a session, and an edit loop that unshares every shape of it: refused, in words, once the drawing would hold more than an
/// open takes; the drawing is then as the last edit that passed left it.
fn scenario_edit_loop() {
    let mut s = blank_session();
    s.edit("install", |doc| {
        *doc = most_shared(900);
        Ok(())
    })
    .expect("a drawing that holds one sheet is small as it is held");
    let (mut passed, mut refused) = (0usize, None);
    for i in 0..900 {
        if i % 10 == 0 {
            println!(
                "GUARD edit_loop at {i}: kept {:?} MiB, {} steps, VmRSS {} MiB",
                s.model_held().map(|b| b >> 20),
                s.history().unwrap().undo_len(),
                status_kib("VmRSS") >> 10
            );
        }
        match unshare(&mut s, i) {
            Ok(()) => passed += 1,
            Err(e) => {
                refused = Some((i, e));
                break;
            }
        }
    }
    let (at, error) = refused.expect("an edit of every one of 900 shapes is 12 GB: some edit must be refused");
    println!("GUARD edit_loop: {passed} edits passed, edit {at} refused: {error}, VmPeak {} MiB", status_kib("VmPeak") >> 10);
    assert_eq!(passed, at);
    assert!((60..=130).contains(&passed), "{passed} edits of 14 MB are the 1.5 GiB an edit may leave");
    let EngineError::Failed { msg, .. } = &error else { panic!("not a Failed: {error:?}") };
    assert!(msg.contains("hold more than 1536 MiB") && msg.contains("Nothing was changed"), "{msg}");
    // What the refused edit would have made is not in the document: the shapes after the last that passed still share the one sheet, and the document holds
    // what it did.
    let doc = s.document().expect("the document");
    let shapes = &doc.pages.get(0).expect("the page").shapes;
    assert!(Arc::ptr_eq(&shapes.get(at).unwrap().sheet, &shapes.get(899).unwrap().sheet), "the refused edit left its shape as it was");
    assert!(held_bytes(doc) <= EDIT_MODEL_BYTES + (EDIT_MODEL_BYTES >> 3), "{} MiB", held_bytes(doc) >> 20);
    // An edit that makes the drawing no larger is still made: a name on a shape that has its own copy.
    s.edit("name a shape", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        Arc::make_mut(page.shapes.get_mut(0).unwrap()).name = Some(Arc::from("kept"));
        Ok(())
    })
    .expect("an edit that unshares nothing is not refused");
    // And so is the edit of a shape that shares the sheet and changes nothing of it.
    s.edit("name another shape", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        Arc::make_mut(page.shapes.get_mut(899).unwrap()).name = Some(Arc::from("kept"));
        Ok(())
    })
    .expect("a shape that keeps the shared sheet");
    println!("GUARD edit_loop: peak {} MiB", status_kib("VmPeak") >> 10);
}

/// The paths that make something of every shape's sheet without an edit: the writer of a `.vsdx` and the export of a page as SVG and PDF. Each ends in an
/// answer in words (a file, or a refusal) and none runs the process out of address space. (The `.odg` writer splices an edit into the bytes of the file the drawing
/// came from and writes no other drawing; a copy, a paste, a duplicate and a drop of a stencil are edits, and pass the guard of `Session::edit_if`.)
fn scenario_save_vsdx(scenario: &str) {
    let mut s = blank_session();
    s.edit("install", |doc| {
        *doc = most_shared(300);
        // A page with a size, so that there is something to export.
        let page = Arc::make_mut(doc.pages.get_mut(0).expect("the page"));
        let sheet = Arc::make_mut(&mut page.sheet);
        for name in ["PageWidth", "PageHeight"] {
            sheet.cells.insert(CellName::new(name), Cell::constant(diacraft_doc::Value::from_text("10")));
        }
        Ok(())
    })
    .expect("a drawing that holds one sheet is small as it is held");
    let dir = std::env::temp_dir().join(format!("diacraft-guard-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder");
    let (command, file) = match scenario {
        "save_vsdx" => ("file.save_as", "out.vsdx"),
        "export_svg" => ("file.export", "out.svg"),
        _ => ("file.export", "out.pdf"),
    };
    let out = s.execute(command, &serde_json::json!({ "path": dir.join(file).to_string_lossy() }));
    println!("GUARD {scenario}: {:?}, VmPeak {} MiB", out.as_ref().err().map(ToString::to_string), status_kib("VmPeak") >> 10);
    let _ = std::fs::remove_dir_all(&dir);
    if scenario == "save_vsdx" {
        // 300 shapes that each hold 33,000 items of sheet are 10 million items in a part: the writer's own reader refuses it, and the save says so and keeps nothing.
        let message = out.expect_err("a part of 10 million items is more than the reader takes").to_string();
        assert!(message.contains("could not be saved") && message.contains("items"), "{message}");
        assert!(status_kib("VmPeak") >> 10 < 2_500, "the save of the most-shared drawing took {} MiB", status_kib("VmPeak") >> 10);
    } else {
        // What is drawn is what each shape has, and the shared sheet is read by each and copied by none.
        out.expect("a page of shapes that share a sheet is exported");
        assert!(status_kib("VmPeak") >> 10 < 1_000, "the export took {} MiB", status_kib("VmPeak") >> 10);
    }
}

/// What an undo and a redo do to the count the guard keeps: nothing. The drawing and its history are the same documents after them, and the count is of all of them, so no
/// place that gives the session another document has to remember to reset it, and the edit after them is counted from what the session holds.
fn scenario_undo_redo() {
    let mut s = blank_session();
    s.edit("install", |doc| {
        *doc = most_shared(900);
        Ok(())
    })
    .expect("a drawing that holds one sheet is small as it is held");
    // Unshare shapes until the drawing and its history are within a few sheets of the figure.
    let mut next = 0usize;
    while held_now(&s) + (EDIT_MODEL_BYTES >> 5) < EDIT_MODEL_BYTES && unshare(&mut s, next).is_ok() {
        next += 1;
        let (counted, fresh) = (s.model_held().expect("counted"), held_now(&s));
        assert!(
            counted >= fresh && counted - fresh < 1 << 20,
            "the count is never under what is held, and is over it by what the history let go of for its own bytes: {counted} against {fresh}"
        );
    }
    let near = held_now(&s);
    assert!(near <= EDIT_MODEL_BYTES && near + (EDIT_MODEL_BYTES >> 4) > EDIT_MODEL_BYTES, "within a sheet or two of the figure: {} MiB", near >> 20);
    // An undo and a redo leave the count as it is, and it is the count of the documents.
    let kept = s.model_held();
    for _ in 0..5 {
        s.execute("edit.undo", &serde_json::json!({})).expect("undo");
        assert_eq!((s.model_held(), held_now(&s)), (kept, near));
    }
    for _ in 0..5 {
        s.execute("edit.redo", &serde_json::json!({})).expect("redo");
        assert_eq!((s.model_held(), held_now(&s)), (kept, near));
    }
    // The next edits are counted from what the session holds: the history makes room for them, and an edit the drawing alone cannot take is refused in words.
    let steps = s.history().expect("a history").undo_len();
    let mut refused = None;
    for i in next..900 {
        if let Err(e) = unshare(&mut s, i) {
            refused = Some(e);
            break;
        }
        assert!(held_now(&s) <= EDIT_MODEL_BYTES, "past the figure: {} MiB", held_now(&s) >> 20);
    }
    let refused = refused.expect("the drawing alone is at the figure before every shape is unshared");
    assert!(
        matches!(&refused, EngineError::Failed { msg, .. } if msg.contains("hold more than 1536 MiB") && msg.contains("Nothing was changed")),
        "{refused:?}"
    );
    let after = s.model_held().expect("counted");
    assert!(after >= held_now(&s) && after - held_now(&s) < 1 << 20, "never under what is held: {after} against {}", held_now(&s));
    // An undo after the refusal and a redo give the drawing back, and the count is the same.
    s.execute("edit.undo", &serde_json::json!({})).expect("undo");
    s.execute("edit.redo", &serde_json::json!({})).expect("redo");
    assert_eq!(s.model_held(), Some(after));
    println!(
        "GUARD undo_redo: {next} sheets unshared, {} MiB, {steps} steps, held {} MiB, {} steps",
        near >> 20,
        after >> 20,
        s.history().unwrap().undo_len()
    );
}

/// **The history is held beside the drawing** (the review's first finding): the shapes of a drawing that were unshared to a gigabyte, and edited again,
/// are two copies of each sheet, one in the drawing and one in the step before the edit. The guard of the drawing alone let that grow to the address space and the process
/// aborted at the 85th edit; the count of the session's documents lets the oldest steps go before it refuses.
fn scenario_reedit() {
    let mut s = blank_session();
    s.edit("install", |doc| {
        *doc = most_shared(900);
        Ok(())
    })
    .expect("a drawing that holds one sheet is small as it is held");
    let mut k = 0usize;
    while held_bytes(s.document().expect("the document")) < (1 << 30) {
        unshare(&mut s, k).expect("an unshare under the figure");
        k += 1;
    }
    let mut refused = 0usize;
    for i in 0..85 {
        match unshare(&mut s, i % k) {
            Ok(()) => {}
            Err(EngineError::Failed { msg, .. }) => {
                assert!(msg.contains("hold more than 1536 MiB") && msg.contains("Nothing was changed"), "{msg}");
                refused += 1;
            }
            Err(other) => panic!("{other:?}"),
        }
    }
    let held = held_now(&s);
    println!(
        "GUARD reedit: {k} shapes unshared, 85 re-edits, {refused} refused, held {} MiB, {} steps, VmPeak {} MiB",
        held >> 20,
        s.history().unwrap().undo_len(),
        status_kib("VmPeak") >> 10
    );
    assert!(held <= EDIT_MODEL_BYTES, "the session holds {} MiB", held >> 20);
    assert!(s.history().unwrap().undo_len() < k + 85, "the history let steps go to make room");
}

/// A select-all and a nudge of a drawing of 100,000 shapes that each have a sheet of their own: each is a copy of every shape and sheet, and the step before it holds the
/// other. The process aborted at the 12th edit.
fn scenario_nudge() {
    let mut s = blank_session();
    s.edit("install", |doc| {
        *doc = diagram(100_000);
        Ok(())
    })
    .expect("a drawing");
    let one = held_now(&s);
    let mut passed = 0usize;
    for x in 0..30 {
        match nudge_all(&mut s, x) {
            Ok(()) => passed += 1,
            Err(EngineError::Failed { msg, .. }) => assert!(msg.contains("Nothing was changed"), "{msg}"),
            Err(other) => panic!("{other:?}"),
        }
    }
    let held = held_now(&s);
    println!(
        "GUARD nudge: drawing {} MiB, {passed} of 30 passed, held {} MiB, {} steps, VmPeak {} MiB",
        one >> 20,
        held >> 20,
        s.history().unwrap().undo_len(),
        status_kib("VmPeak") >> 10
    );
    assert_eq!(passed, 30, "each nudge makes the drawing no larger, and the steps make room");
    assert!(held <= EDIT_MODEL_BYTES, "the session holds {} MiB", held >> 20);
    assert!(s.history().unwrap().undo_len() <= 4, "a drawing of {} MiB keeps few steps", one >> 20);
}

/// 400 edits that each put a value of 8 MiB, of its own, in a cell: 3 GiB that the steps hold if none is let go of. The characters of a value an edit makes are counted.
fn scenario_values() {
    let mut s = blank_session();
    s.edit("install", |doc| {
        *doc = diagram(10);
        Ok(())
    })
    .expect("a drawing");
    for i in 0..400 {
        put_a_big_value(&mut s, i).unwrap_or_else(|e| panic!("edit {i}: {e}"));
    }
    let held = held_now(&s);
    println!(
        "GUARD values: 400 edits of 8 MiB, held {} MiB, {} steps, VmPeak {} MiB",
        held >> 20,
        s.history().unwrap().undo_len(),
        status_kib("VmPeak") >> 10
    );
    assert!(held <= EDIT_MODEL_BYTES, "the session holds {} MiB", held >> 20);
    let steps = s.history().unwrap().undo_len();
    // The history is held to its own bytes as well (192 MiB of steps in a direction), and the steps of 8 MiB that fit are 23.
    assert!((10..=200).contains(&steps), "the steps that fit beside the drawing: {steps}");
}

/// **The save of a model that shares as a `.vsdx`** (the review's second finding): the writer made the tree of every shape, shared or not, with no cap, and
/// the read-back that refuses a part past the reader's caps came after the tree of ten thousand shapes of a thousand rows (100 million items) was made, and the process aborted.
/// The writer is held to the caps of the read-back while it makes each part, and refuses in words.
fn scenario_save_vsdx_duplicates(scenario: &str) {
    let mut s = blank_session();
    s.edit("install", |doc| {
        *doc = match scenario {
            "save_vsdx_900" => most_shared(900),
            "save_vsdx_kept" => kept_duplicated(10_000),
            _ => path_duplicated(10_000, 1_000),
        };
        let page = Arc::make_mut(doc.pages.get_mut(0).expect("the page"));
        let sheet = Arc::make_mut(&mut page.sheet);
        for name in ["PageWidth", "PageHeight"] {
            sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text("10")));
        }
        Ok(())
    })
    .expect("a drawing that holds its shapes once is small as it is held");
    let dir = std::env::temp_dir().join(format!("diacraft-guard-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder");
    let out = s.execute("file.save_as", &serde_json::json!({ "path": dir.join("out.vsdx").to_string_lossy() }));
    let wrote = dir.join("out.vsdx").exists();
    let _ = std::fs::remove_dir_all(&dir);
    let message = out.expect_err("a part of 100 million items is more than the reader takes").to_string();
    println!("GUARD {scenario}: {message}, VmPeak {} MiB", status_kib("VmPeak") >> 10);
    assert!(message.contains("could not be saved") && message.contains("items in one page or master part"), "{message}");
    assert!(!wrote, "a file that does not read back is not kept");
    assert!(status_kib("VmPeak") >> 10 < 2_500, "the save took {} MiB", status_kib("VmPeak") >> 10);
}

/// The `.odg` writer of a drawing that shares (the second finding of says to check it): one shape of a thousand rows in ten thousand places, saved as an
/// `.odg`, ends as a save or in words and does not abort.
fn scenario_save_odg_duplicates() {
    let mut s = blank_session();
    s.edit("install", |doc| {
        *doc = path_duplicated(10_000, 1_000);
        let page = Arc::make_mut(doc.pages.get_mut(0).expect("the page"));
        let sheet = Arc::make_mut(&mut page.sheet);
        for name in ["PageWidth", "PageHeight"] {
            sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text("10")));
        }
        Ok(())
    })
    .expect("a drawing that holds its shapes once is small as it is held");
    let dir = std::env::temp_dir().join(format!("diacraft-guard-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder");
    let out = s.execute("file.save_as", &serde_json::json!({ "path": dir.join("out.odg").to_string_lossy() }));
    let _ = std::fs::remove_dir_all(&dir);
    println!(
        "GUARD save_odg_duplicates: {:?}, VmPeak {} MiB",
        out.as_ref().map(|_| "saved").map_err(ToString::to_string),
        status_kib("VmPeak") >> 10
    );
    assert!(status_kib("VmPeak") >> 10 < 2_500, "the save took {} MiB", status_kib("VmPeak") >> 10);
}

/// **The figure is the process's**: two sessions in one process, each with a drawing of 100,000 shapes (986 MiB as it is counted, a gigabyte and a half as it
/// is held), are 2.8 GB with the third each edit makes, and a process of 3,000,000 KiB aborts. Each session held to its own 1.5 GiB passed; the second is held to what the first leaves.
fn scenario_two_sessions() {
    let mut first = blank_session();
    first
        .edit("install", |doc| {
            *doc = diagram(100_000);
            Ok(())
        })
        .expect("the first drawing");
    let mut second = blank_session();
    let refused = second
        .edit("install", |doc| {
            *doc = diagram(100_000);
            Ok(())
        })
        .expect_err("what the first session holds leaves the second no room for as much");
    let EngineError::Failed { msg, .. } = &refused else { panic!("not a Failed: {refused:?}") };
    println!("GUARD two_sessions: the second is refused: {msg}, VmPeak {} MiB", status_kib("VmPeak") >> 10);
    assert!(msg.contains("hold more than") && msg.contains("Nothing was changed"), "{msg}");
    // A drawing that fits beside it is made and edited, and the first is edited as before.
    second
        .edit("install", |doc| {
            *doc = diagram(20_000);
            Ok(())
        })
        .expect("a smaller drawing fits beside the first");
    for x in 0..5 {
        nudge_all(&mut second, x).expect("an edit of the small drawing");
    }
    first
        .edit("move one", |doc| {
            let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
            let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
            Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("PinX"), v("1"));
            Ok(())
        })
        .expect("an edit of one shape of the first drawing");
    let (a, b) = (first.model_held().unwrap() >> 20, second.model_held().unwrap() >> 20);
    println!(
        "GUARD two_sessions: held {a} MiB and {b} MiB, process {} MiB, VmPeak {} MiB",
        diacraft_engine::memory::held() >> 20,
        status_kib("VmPeak") >> 10
    );
    assert!(a + b <= EDIT_MODEL_BYTES >> 20, "the two sessions hold {} MiB", a + b);
}

#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn model_guard_child() {
    let scenario = std::env::var(SCENARIO).unwrap_or_default();
    match scenario.as_str() {
        "edit_loop" => scenario_edit_loop(),
        "save_vsdx" | "export_svg" | "export_pdf" => scenario_save_vsdx(&scenario),
        "undo_redo" => scenario_undo_redo(),
        "reedit" => scenario_reedit(),
        "nudge" => scenario_nudge(),
        "values" => scenario_values(),
        "save_vsdx_duplicates" | "save_vsdx_900" | "save_vsdx_kept" => scenario_save_vsdx_duplicates(&scenario),
        "save_odg_duplicates" => scenario_save_odg_duplicates(),
        "two_sessions" => scenario_two_sessions(),
        other => panic!("no scenario {other}"),
    }
    println!("guard: no abort");
}

#[cfg(target_os = "linux")]
fn run(scenario: &str) -> String {
    let exe = std::env::current_exe().unwrap();
    let script = "ulimit -v 3000000 && exec \"$0\" --exact model_guard::model_guard_child --ignored --nocapture --test-threads=1";
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && text.contains("guard: no abort"), "{scenario}: {text}");
    eprintln!("{}", text.lines().filter(|l| l.contains("GUARD")).collect::<Vec<_>>().join("\n"));
    text
}

#[test]
#[cfg(target_os = "linux")]
fn the_save_of_the_most_shared_drawing_as_a_vsdx_is_refused_in_words_by_the_writer_and_does_not_run_out_of_address_space() {
    run("save_vsdx");
}

#[test]
#[cfg(target_os = "linux")]
fn the_count_of_an_edit_is_of_the_drawing_it_starts_from_and_an_undo_and_a_redo_give_the_guard_no_stale_count() {
    run("undo_redo");
}

#[test]
#[cfg(target_os = "linux")]
fn the_export_of_the_most_shared_drawing_as_svg_copies_no_sheet() {
    run("export_svg");
}

#[test]
#[cfg(target_os = "linux")]
fn the_export_of_the_most_shared_drawing_as_pdf_copies_no_sheet() {
    run("export_pdf");
}

#[test]
#[cfg(target_os = "linux")]
fn an_edit_loop_that_unshares_every_shape_of_the_most_shared_drawing_is_refused_in_words_before_it_passes_what_an_open_takes() {
    run("edit_loop");
}

#[test]
#[cfg(target_os = "linux")]
fn the_edit_of_shapes_that_were_unshared_to_a_gigabyte_is_counted_with_the_history_and_the_steps_make_room() {
    run("reedit");
}

#[test]
#[cfg(target_os = "linux")]
fn a_select_all_and_a_nudge_of_a_hundred_thousand_shapes_keeps_the_steps_that_fit_and_does_not_run_out_of_address_space() {
    run("nudge");
}

#[test]
#[cfg(target_os = "linux")]
fn four_hundred_edits_that_each_put_eight_mebibytes_in_a_cell_are_counted_by_their_characters_and_do_not_run_out_of_address_space() {
    run("values");
}

#[test]
#[cfg(target_os = "linux")]
fn the_save_as_a_vsdx_of_one_shape_of_a_thousand_rows_in_ten_thousand_places_is_refused_in_words_while_its_parts_are_made() {
    run("save_vsdx_duplicates");
}

#[test]
#[cfg(target_os = "linux")]
fn the_save_as_a_vsdx_of_the_most_shared_drawing_of_nine_hundred_shapes_is_refused_in_words_while_its_parts_are_made() {
    run("save_vsdx_900");
}

#[test]
#[cfg(target_os = "linux")]
fn the_save_as_an_odg_of_one_shape_of_a_thousand_rows_in_ten_thousand_places_ends_as_a_save_or_in_words_and_does_not_abort() {
    run("save_odg_duplicates");
}

#[test]
#[cfg(target_os = "linux")]
fn the_save_as_a_vsdx_of_one_shape_that_keeps_a_large_tree_of_unknown_xml_in_ten_thousand_places_is_refused_in_words_before_the_kept_trees_are_made()
{
    run("save_vsdx_kept");
}

#[test]
#[cfg(target_os = "linux")]
fn two_sessions_with_a_drawing_each_are_held_to_one_figure_and_the_second_is_refused_in_words_and_neither_runs_out_of_address_space() {
    run("two_sessions");
}
