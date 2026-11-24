//! The engine's `.odg` path: `file.open` sniffs an OpenDocument drawing, `file.save` and `file.save_as` splice an edit into its bytes and
//! write them, and the session keeps the file it wrote with the index the writer gave, so that a drawing saved twice is no edit the second time (Ruling
//!). The drawings are built by hand from the standard (`odg_build.rs`); what is read back is read by the `odg` crate's reader.

use diacraft_doc::Document;
use diacraft_engine::{EngineError, Session};
use serde_json::{Value, json};

use crate::odg_build::{
    Dir, Drawing, content_of, frame_of, glued, id_of, names_of, open_drawing, part_bytes, part_of, read_back, rect, rect_on, same_frame,
};
use crate::{check_session, run};

/// A drawing of four rectangles on page 0: A (holding "hello"), B and D on the style `gr1`, C on `gr2`.
pub fn four() -> Drawing {
    Drawing::new().page(
        &[
            rect("A", (2.0, 3.0, 4.0, 2.0), "hello"),
            rect("B", (12.0, 3.0, 4.0, 2.0), ""),
            rect_on("gr2", "C", (2.0, 8.0, 4.0, 2.0), ""),
            rect("D", (12.0, 8.0, 4.0, 2.0), ""),
        ]
        .concat(),
    )
}

/// Moves the shape `name` of page 0 by (`dx`, `dy`) inches.
pub fn shift(s: &mut Session, name: &str, dx: f64, dy: f64) {
    let id = id_of(s.document().unwrap(), 0, name);
    run(s, "shape.move", json!({ "ids": [id], "dx": dx, "dy": dy }));
}

/// `file.save`, which must succeed.
pub fn save(s: &mut Session) -> Value {
    run(s, "file.save", json!({}))
}

/// `file.save`, which must be refused: the error's message.
#[track_caller]
pub fn refused(s: &mut Session) -> String {
    match s.execute("file.save", &json!({})) {
        Err(EngineError::Failed { msg, .. }) => msg,
        other => panic!("the save was not refused as a failure: {other:?}"),
    }
}

/// What the shape at `at` on a page looks like to the window's controls (the fill, the line and the rest), without its `ID`.
pub fn look(s: &mut Session, page: usize, at: usize) -> Value {
    let doc = s.document().unwrap();
    let (page_id, id) = (doc.pages.get(page).unwrap().id, doc.pages.get(page).unwrap().shapes.get(at).unwrap().id);
    let mut shape = run(s, "shape.formatting", json!({ "ids": [id], "page": page_id }))["shapes"][0].clone();
    if let Some(object) = shape.as_object_mut() {
        object.remove("id");
    }
    shape
}

/// The text of the shape at `at` on a page, as an editor shows it.
pub fn text_at(s: &Session, page: usize, at: usize) -> String {
    let doc = s.document().unwrap();
    let (page_id, id) = (doc.pages.get(page).unwrap().id, doc.pages.get(page).unwrap().shapes.get(at).unwrap().id);
    diacraft_engine::draw::text_view(doc, page_id, id).map(|v| v.chars).unwrap_or_default()
}

/// The text of the shape `name`, as an editor shows it.
pub fn text(s: &Session, page: usize, name: &str) -> String {
    let doc = s.document().unwrap();
    text_at(s, page, doc.pages.get(page).unwrap().shapes.iter().position(|x| x.name_u.as_deref() == Some(name)).unwrap())
}

/// The frame of the shape at `at` on a page, as the engine reads it.
pub fn frame_at(doc: &Document, page: usize, at: usize) -> [f64; 5] {
    let p = doc.pages.get(page).unwrap();
    let f = diacraft_engine::draw::shape_frame(doc, p.id, p.shapes.get(at).unwrap().id).unwrap();
    [f.pin_x, f.pin_y, f.width, f.height, f.angle]
}

/// The glue of a page, as `(from, to)` by the places of the shapes in the drawing order, sorted.
pub fn glue(doc: &Document, page: usize) -> Vec<(usize, usize)> {
    let page = doc.pages.get(page).unwrap();
    let place = |id: u32| page.shapes.iter().position(|s| s.id == id).unwrap_or(usize::MAX);
    let mut rows: Vec<(usize, usize)> = page.connects.iter().map(|c| (place(c.from_sheet), place(c.to_sheet))).collect();
    rows.sort_unstable();
    rows
}

/// The number a cell of the shape at `at` holds.
pub fn number(doc: &Document, page: usize, at: usize, cell: &str) -> f64 {
    let shape = doc.pages.get(page).unwrap().shapes.get(at).unwrap();
    shape.sheet.cell(cell).and_then(|c| c.value.as_ref()?.number()).unwrap()
}

/// The drawing a session holds and the drawing the file it saved holds, compared the way a person would: the same shapes in the same order with the
/// same frames, looks and texts, and the same glue and ends of connectors.
pub fn assert_reopens_as_saved(dir: &Dir, s: &mut Session, name: &str, page: usize) {
    let bytes = dir.read(name);
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path(name) }));
    let (made, read) = (s.document().unwrap().clone(), again.document().unwrap().clone());
    let named = |d: &Document| names_of(d, page).into_iter().map(|n| (!n.is_empty()).then_some(n)).collect::<Vec<_>>();
    assert_eq!(named(&made).len(), named(&read).len(), "as many shapes");
    for (a, b) in named(&made).iter().zip(named(&read).iter()) {
        assert!(a.is_none() || a == b, "the shapes and their order: {:?} against {:?}", names_of(&made, page), names_of(&read, page));
    }
    for (at, shape) in made.pages.get(page).unwrap().shapes.iter().enumerate() {
        if shape.sheet.cell("BeginX").is_some() {
            for end in ["BeginX", "BeginY", "EndX", "EndY"] {
                let (a, b) = (number(&made, page, at, end), number(&read, page, at, end));
                assert!((a - b).abs() < 1e-3, "shape {at}: {end} is {a} in the model and {b} in the file");
            }
        } else {
            same_frame(frame_at(&made, page, at), frame_at(&read, page, at), &format!("shape {at}"));
            assert_eq!(look(s, page, at), look(&mut again, page, at), "shape {at}: its look");
            assert_eq!(text_at(s, page, at), text_at(&again, page, at), "shape {at}: its text");
        }
    }
    assert_eq!(glue(&made, page), glue(&read, page), "the glue");
    for cell in ["PageWidth", "PageHeight"] {
        let (a, b) = (number_of_page(&made, page, cell), number_of_page(&read, page, cell));
        assert!((a - b).abs() < 1e-6, "{cell} is {a} in the model and {b} in the file");
    }
    // The file reads with no warning: nothing in what was written is odd.
    let (_, report) = read_back(&bytes);
    assert_eq!(report.warnings, Vec::<String>::new(), "the saved file reads with no warning");
    check_session(&again).unwrap();
}

/// A second save of the same drawing through the session is no edit and writes the same bytes, and so is the third.
#[track_caller]
pub fn assert_saves_again_as_no_edit(dir: &Dir, s: &mut Session, name: &str) {
    let before = dir.read(name);
    let again = save(s);
    assert_eq!(again["edited"], false, "the second save of the same drawing is no edit: {again}");
    assert_eq!(dir.read(name), before, "and the file is the same bytes");
    assert_eq!(again["warning_count"], 0, "and says nothing: {again}");
    let third = save(s);
    assert_eq!(third["edited"], false, "nor is the third");
    assert_eq!(dir.read(name), before);
}

#[test]
fn open_edit_save_odg_round_trips_the_model() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let doc = s.document().unwrap().clone();
    let (a, b, c, d) = (id_of(&doc, 0, "A"), id_of(&doc, 0, "B"), id_of(&doc, 0, "C"), id_of(&doc, 0, "D"));
    let before = dir.read("a.odg");
    // Each way of editing a drawing that the first milestone names: move, resize, turn, restyle, retext, delete, reorder, and add a basic shape and a
    // connector.
    run(&mut s, "shape.move", json!({ "ids": [a], "dx": 0.5, "dy": -0.25 }));
    let fb = frame_of(&doc, 0, "B");
    run(&mut s, "shape.resize", json!({ "id": b, "x": fb[0] - fb[2] / 2.0, "y": fb[1] - fb[3] / 2.0, "width": fb[2] * 1.5, "height": fb[3] * 0.75 }));
    run(&mut s, "shape.rotate", json!({ "ids": [c], "angle": 30 }));
    run(&mut s, "shape.format", json!({ "ids": [b], "fill": "#00ff00" }));
    run(&mut s, "shape.set_text", json!({ "id": a, "text": "J", "start": 0, "end": 1 }));
    run(&mut s, "shape.delete", json!({ "ids": [d] }));
    run(&mut s, "shape.order", json!({ "ids": [a], "to": "front" }));
    assert!(s.is_dirty());
    let saved = save(&mut s);
    assert_eq!(saved["variant"], "odg");
    assert_eq!(saved["edited"], true);
    assert!(!s.is_dirty(), "a saved drawing is clean");
    assert_ne!(dir.read("a.odg"), before);
    assert_eq!(names_of(s.document().unwrap(), 0), ["B", "C", "A"]);
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_eq!(text(&s, 0, "A"), "Jello");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    // Then a basic shape and a connector (the writer does not put a new shape and a new order of the page in one save).
    let page = doc.pages.get(0).unwrap().id;
    let ellipse = run(&mut s, "shape.drop", json!({ "master": "ellipse", "page": page, "x": 4.0, "y": 2.0 }));
    let new = u32::try_from(ellipse["id"].as_u64().unwrap()).unwrap();
    run(&mut s, "connector.add", json!({ "from": { "id": new }, "to": { "id": c } }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true);
    assert_eq!(names_of(s.document().unwrap(), 0), ["B", "C", "A", &format!("Ellipse.{new}"), &format!("Connector.{}", new + 1)]);
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    check_session(&s).unwrap();
}

/// What one case of `each_kind_of_edit_saves_as_the_edit_and_saves_again_as_no_edit` does to a session.
pub type Edit = Box<dyn Fn(&mut Session, &Document)>;

/// The kinds of edit: what each does to a session that has opened `four()`.
pub fn edit_cases() -> Vec<(&'static str, Edit)> {
    let id = |doc: &Document, name: &str| id_of(doc, 0, name);
    vec![
        ("a move", Box::new(move |s, d| drop(run(s, "shape.move", json!({ "ids": [id(d, "A")], "dx": 1.0, "dy": 0.5 }))))),
        (
            "a resize",
            Box::new(move |s, d| {
                let f = frame_of(d, 0, "B");
                run(
                    s,
                    "shape.resize",
                    json!({ "id": id(d, "B"), "x": f[0] - f[2] / 2.0, "y": f[1] - f[3] / 2.0, "width": f[2] * 2.0, "height": f[3] }),
                );
            }),
        ),
        ("a turn", Box::new(move |s, d| drop(run(s, "shape.rotate", json!({ "ids": [id(d, "C")], "angle": 45 }))))),
        (
            "a colour of a style three shapes share",
            Box::new(move |s, d| drop(run(s, "shape.format", json!({ "ids": [id(d, "D")], "fill": "#ff8800" })))),
        ),
        ("a line of a style one shape names", Box::new(move |s, d| drop(run(s, "shape.format", json!({ "ids": [id(d, "C")], "weight": 3.0 }))))),
        ("a retext", Box::new(move |s, d| drop(run(s, "shape.set_text", json!({ "id": id(d, "A"), "text": "x", "start": 5, "end": 5 }))))),
        ("a delete", Box::new(move |s, d| drop(run(s, "shape.delete", json!({ "ids": [id(d, "D")] }))))),
        ("a reorder", Box::new(move |s, d| drop(run(s, "shape.order", json!({ "ids": [id(d, "A")], "to": "front" }))))),
        ("a new rectangle", Box::new(|s, _| drop(run(s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }))))),
        (
            "a new ellipse with a text",
            Box::new(|s, _| {
                let made = run(s, "shape.drop", json!({ "master": "ellipse", "page": 0, "x": 4.0, "y": 2.0 }));
                run(s, "shape.set_text", json!({ "id": made["id"], "text": "round" }));
            }),
        ),
        ("a text box", Box::new(|s, _| drop(run(s, "text.box", json!({ "page": 0, "x": 4.0, "y": 2.0, "text": "a label" }))))),
        (
            "a connector between two shapes of the file",
            Box::new(move |s, d| drop(run(s, "connector.add", json!({ "from": { "id": id(d, "A") }, "to": { "id": id(d, "B") } })))),
        ),
        ("a duplicate", Box::new(move |s, d| drop(run(s, "shape.duplicate", json!({ "ids": [id(d, "A")] }))))),
        (
            "a copy and a paste",
            Box::new(move |s, d| {
                run(s, "edit.copy", json!({ "ids": [id(d, "C")] }));
                run(s, "edit.paste", json!({}));
            }),
        ),
        ("a page made taller", Box::new(|s, _| drop(run(s, "page.size", json!({ "width": 8.27, "height": 14.0 }))))),
    ]
}

/// Through `file.save`, for each kind of edit: after the save the same drawing saved again is no edit, byte for byte, and what the file reads
/// back as is what the session holds. A new shape and a page that was split are among them, since they are the ones the writer's own reading of the file
/// would have shown as changed.
#[test]
fn each_kind_of_edit_saves_as_the_edit_and_saves_again_as_no_edit() {
    for (case, edit) in edit_cases() {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &four());
        let doc = s.document().unwrap().clone();
        edit(&mut s, &doc);
        let saved = s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("{case}: {e}"));
        assert_eq!(saved["edited"], true, "{case}");
        assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
        check_session(&s).unwrap();
    }
}

/// The echo control: a save that gave the original file back, whatever was edited, fails the reading of what was saved in every case above. (The
/// bytes of the file are put back as they were after the real save, and the comparison with the session must not pass.)
#[test]
fn echo_control_fails_every_edit_case() {
    for (case, edit) in edit_cases() {
        let dir = Dir::new();
        let original = four().bytes();
        let mut s = open_drawing(&dir, &four());
        let doc = s.document().unwrap().clone();
        edit(&mut s, &doc);
        save(&mut s);
        dir.write("a.odg", &original);
        let held = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| assert_reopens_as_saved(&dir, &mut s, "a.odg", 0)));
        assert!(held.is_err(), "{case}: a save that wrote the original passed the comparison with what the session holds");
    }
}

/// Two pages on one master page, the second made taller: the master page and its layout are copied for it, the first page is as it was, and the second
/// saved again is no edit.
#[test]
fn a_page_that_shares_its_master_page_is_split_when_it_is_resized_and_saves_again_as_no_edit() {
    let dir = Dir::new();
    let drawing = Drawing::new().page(&rect("A", (2.0, 3.0, 4.0, 2.0), "")).page(&rect("B", (2.0, 3.0, 4.0, 2.0), ""));
    let mut s = open_drawing(&dir, &drawing);
    let pages: Vec<u32> = s.document().unwrap().pages.iter().map(|p| p.id).collect();
    run(&mut s, "page.size", json!({ "page": pages[1], "width": 9.0, "height": 15.0 }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true);
    let bytes = dir.read("a.odg");
    let (content, styles) = (content_of(&bytes), part_of(&bytes, "styles.xml"));
    assert!(content.contains("draw:master-page-name=\"Default_2\"") && content.contains("draw:master-page-name=\"Default\""), "{content}");
    assert!(styles.contains("style:name=\"PM2\"") && styles.contains("style:name=\"PM1\""), "{styles}");
    let (read, _) = read_back(&bytes);
    let height = |d: &Document, at: usize| number_of_page(d, at, "PageHeight");
    assert!(
        (height(&read, 1) - 15.0).abs() < 1e-6 && (height(&read, 0) - 29.7 / 2.54).abs() < 1e-6,
        "the second page is 15 inches, the first as it was"
    );
    for page in 0..2 {
        assert_reopens_as_saved(&dir, &mut s, "a.odg", page);
    }
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

fn number_of_page(doc: &Document, at: usize, cell: &str) -> f64 {
    doc.pages.get(at).unwrap().sheet.cell(cell).and_then(|c| c.value.as_ref()?.number()).unwrap()
}

/// `shape.drop` makes an instance of a master that holds only its pin, which the writer refuses for a new shape. The save hands it a flattened
/// copy: the shape is written as a native element with the cells it resolves to and the link to its master (the master is written into DiaCraft's document file),
/// and the drawing in the session is not changed.
/// Then the file is read back, the drawing saved again is no edit, and an edit of the dropped shape afterwards is written as the edit it is.
#[test]
fn a_shape_dropped_from_a_stencil_is_saved_read_back_and_saved_again_with_no_edit() {
    for (stencil, master) in [
        ("basic", "rectangle"),
        ("basic", "ellipse"),
        ("basic", "diamond"),
        ("basic", "hexagon"),
        ("flowchart", "process"),
        ("flowchart", "decision"),
    ] {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &four());
        let made = run(&mut s, "shape.drop", json!({ "master": master, "stencil": stencil, "page": 0, "x": 4.0, "y": 2.0 }));
        let name = made["name"].as_str().unwrap().to_owned();
        let saved = save(&mut s);
        assert_eq!(saved["edited"], true, "{master}");
        // The model is as it was: an instance of a master the document holds, with its pin alone.
        let doc = s.document().unwrap().clone();
        let shape = doc.pages.get(0).unwrap().shapes.iter().find(|x| x.name_u.as_deref() == Some(&name)).unwrap().clone();
        assert!(shape.master.is_some() && !doc.masters.is_empty(), "{master}: the engine's model is unchanged");
        assert_eq!(shape.sheet.cells.keys().map(|k| k.as_str()).collect::<Vec<_>>(), ["PinX", "PinY"], "{master}");
        // The file holds a native shape with the link to its master, and the master is in the document file.
        let content = content_of(&dir.read("a.odg"));
        assert!(content.contains(&format!("draw:name=\"{name}\"")), "{master}: {content}");
        let link = format!("diacraft:master=\"{}\"", shape.master.unwrap());
        assert!(content.contains(&link), "{master}: the link to the master is written ({link}): {content}");
        for carried in ["diacraft:cells=", "diacraft:gone=", "diacraft:s-", "diacraft:name=", "diacraft:text="] {
            assert!(
                !content.contains(carried),
                "{master}: {carried}: nothing else of DiaCraft's is written for a shape the file says in full: {content}"
            );
        }
        let document = part_bytes(&dir.read("a.odg"), "DiaCraft/document.xml").unwrap_or_else(|| panic!("{master}: no document file"));
        assert!(String::from_utf8_lossy(&document).contains("master"), "{master}: the master is carried");
        assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
        // An edit of the dropped shape after the save is the edit it is: a move is written as a move.
        run(&mut s, "shape.move", json!({ "ids": [made["id"]], "dx": 1.0, "dy": 0.0 }));
        let moved = save(&mut s);
        assert_eq!(moved["edited"], true, "{master}");
        assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
        // And its colour, which the master and the style sheet gave it, is the colour the person chooses.
        run(&mut s, "shape.format", json!({ "ids": [made["id"]], "fill": "#123456" }));
        assert_eq!(save(&mut s)["edited"], true);
        assert!(content_of(&dir.read("a.odg")).contains("#123456"), "{master}");
        assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    }
}

/// A master whose connection points are not the four side middles (the triangle's) is saved: DiaCraft's extension carries its connection points, and
/// the file reads back as the model has it and saves again as no edit.
#[test]
fn a_stencil_shape_whose_connection_points_are_not_the_side_middles_is_saved_and_read_back() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    run(&mut s, "shape.drop", json!({ "master": "triangle", "stencil": "basic", "page": 0, "x": 4.0, "y": 2.0 }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// A connector of the file, glued at both ends to shapes that carry ids: the engine routes it again when a shape it is glued to is moved, the writer writes
/// its new route and ends, and the drawing saved again is no edit.
#[test]
fn a_connector_of_the_file_follows_a_shape_that_is_moved_and_saves_again_as_no_edit() {
    let dir = Dir::new();
    let ided = |text: String, id: &str| text.replacen("<draw:custom-shape ", &format!("<draw:custom-shape xml:id=\"{id}\" draw:id=\"{id}\" "), 1);
    let shapes = [
        ided(rect("A", (2.0, 3.0, 4.0, 2.0), ""), "id1"),
        ided(rect("B", (12.0, 3.0, 4.0, 2.0), ""), "id2"),
        glued("K", "id1", "id2", (6.0, 4.0, 12.0, 4.0)),
    ]
    .concat();
    let mut s = open_drawing(&dir, &Drawing::new().page(&shapes));
    let page = s.document().unwrap().pages.get(0).unwrap().clone();
    assert_eq!(page.connects.len(), 2, "the connector is glued at both ends: {:?}", page.connects);
    let before = dir.read("a.odg");
    crate::odg_file::shift(&mut s, "A", 0.0, -1.0);
    assert_eq!(save(&mut s)["edited"], true);
    assert_ne!(dir.read("a.odg"), before);
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    let moved = s.document().unwrap().clone();
    let ends = |d: &Document| -> Vec<f64> { ["BeginX", "BeginY", "EndX", "EndY"].iter().map(|c| number(d, 0, 2, c)).collect() };
    let first = read_back(&before).0;
    assert_ne!(ends(&moved), ends(&first), "the connector's ends moved with the shape it is glued to");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// Every master of every built-in stencil, dropped on its own and saved: all fifty-six are written (the six of the Organization Chart Shapes, among them) (ten of the original thirty natively, twenty by DiaCraft's
/// extension, and the twenty of: a gate that refused them is gone), saved, read back and saved again with no edit; and the drop
/// warns of nothing. (`odg_stencils.rs` compares each with the session's by what it draws and where it connects.)
#[test]
fn each_master_of_the_built_in_stencils_is_saved_and_the_drop_of_none_warns() {
    let mut count = 0;
    let mut written = Vec::new();
    for stencil in diacraft_engine::stencil::stencils() {
        for master in stencil.masters() {
            count += 1;
            let dir = Dir::new();
            let mut s = open_drawing(&dir, &four());
            let dropped = run(&mut s, "shape.drop", json!({ "master": master.id, "stencil": stencil.id, "page": 0, "x": 4.0, "y": 2.0 }));
            let key = format!("{}/{}", stencil.id, master.id);
            assert!(dropped.get("warnings").is_none(), "{key}: the drop of a shape the save writes warns of nothing: {dropped}");
            match s.execute("file.save", &json!({})) {
                Ok(saved) => {
                    assert_eq!(saved["edited"], true, "{key}");
                    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
                    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
                    written.push(key);
                }
                Err(other) => panic!("{key}: {other:?}"),
            }
        }
    }
    assert_eq!((count, written.len()), (58, 58), "fifty-eight masters, fifty-eight written");
    assert_eq!(written.iter().filter(|key| key.starts_with("orgchart/")).count(), 6, "{written:?}");
    assert_eq!(written.iter().filter(|key| key.starts_with("basic/")).count(), 12, "{written:?}");
    assert_eq!(written.iter().filter(|key| key.starts_with("flowchart/")).count(), 18, "{written:?}");
    assert_eq!(written.iter().filter(|key| key.starts_with("decorative/")).count(), 8, "{written:?}");
    assert_eq!(written.iter().filter(|key| key.starts_with("arrows/")).count(), 8, "{written:?}");
    assert_eq!(written.iter().filter(|key| key.starts_with("gane-sarson/")).count(), 4, "{written:?}");
}

/// What the engine drops and draws is saved as the file's own markup and the links to the masters of the shapes, and nothing else of DiaCraft's extension:
/// a shape whose cells are what the reader rebuilds from the markup carries no cell, section or removal, and a document file holds the masters it links
/// to. (A master whose connection points or outline are not the plain ones is the other thing: the extension carries them.) **The connector carries one removal and its
/// outline**: `connector.add` gives it no connection points, and the reader gives every shape four; and its outline holds `NoFill` 1, which the reader gives a
/// segment's outline nowhere.
#[test]
fn what_the_engine_drops_and_draws_is_saved_with_the_links_to_its_masters_and_nothing_else_in_diacrafts_namespace() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let first = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }));
    let second = run(&mut s, "shape.drop", json!({ "master": "ellipse", "stencil": "basic", "page": 0, "x": 8.0, "y": 2.0 }));
    run(&mut s, "connector.add", json!({ "from": { "id": first["id"] }, "to": { "id": second["id"] } }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let content = content_of(&dir.read("a.odg"));
    // What the connector's glue names keeps its `ID`, and nothing else is carried: no cell, no section but the connector's outline, and no removal but its connection points.
    for carried in ["diacraft:cells=", "diacraft:name=", "diacraft:text="] {
        assert!(!content.contains(carried), "{carried}: {content}");
    }
    assert_eq!(
        (content.matches("diacraft:s-").count(), content.matches("diacraft:s-Geometry-0=").count()),
        (1, 1),
        "the connector's outline, for its NoFill, and no other section: {content}"
    );
    assert_eq!(
        (content.matches("diacraft:gone=").count(), content.matches("diacraft:gone=\"s.Connection._\"").count()),
        (1, 1),
        "the connector has no connection points, and says so: {content}"
    );
    assert_eq!(content.matches("diacraft:master=\"").count(), 2, "each of the two shapes names its master: {content}");
    let document = String::from_utf8(part_bytes(&dir.read("a.odg"), "DiaCraft/document.xml").expect("the masters are in the document file")).unwrap();
    assert_eq!(document.matches("<diacraft:master").count(), 2, "and the document file holds the two masters: {document}");
}
