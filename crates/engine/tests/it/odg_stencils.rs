//! Every master of the five built-in stencils is dropped into an `.odg` drawing, saved, read back as the session had it and saved again as no edit
//! (the twenty masters of were refused by a gate until a later change). A save that only returns Ok proves nothing about the shape: the file is opened
//! again and the shape is compared with the session's by what it draws and where it connects.

use diacraft_doc::Document;
#[allow(unused_imports)] // `Session` was used by the helpers that moved to the support file; a test added here may use it
use diacraft_engine::{EngineError, Session};
use serde_json::{Value, json};

use crate::odg_build::{Dir, content_of, id_of, open_drawing};
use crate::odg_file::{assert_reopens_as_saved, assert_saves_again_as_no_edit, four, save};
use crate::odg_masters::assert_link_holds;
use crate::run;
// The helpers moved to the support file of the second batch's per-stencil checks, which runs the same checks on the stencils it adds. Every one that
// this file's tests could use is imported, so a test added here keeps the names it had.
#[allow(unused_imports)]
use crate::stencils2_support::{PIXEL_SLACK, assert_reads_back_as_made, assert_reads_back_within, drawn, reopened};

/// All fifty masters: dropped on their own into a drawing of four rectangles, saved (the save says nothing, but for an outline written as a rectangle for other
/// programs), read back and compared, and saved again as no edit.
#[test]
fn each_of_the_fifty_masters_is_saved_reads_back_as_the_session_had_it_and_saves_again_as_no_edit() {
    let mut count = 0;
    let mut approximated = Vec::new();
    for stencil in diacraft_engine::stencil::stencils().iter().take(7) {
        for master in stencil.masters() {
            count += 1;
            let key = format!("{}/{}", stencil.id, master.id);
            let dir = Dir::new();
            let mut s = open_drawing(&dir, &four());
            let dropped = run(&mut s, "shape.drop", json!({ "master": master.id, "stencil": stencil.id, "page": 0, "x": 4.0, "y": 2.0 }));
            assert!(dropped.get("warnings").is_none(), "{key}: a drop into an .odg warns of nothing: {dropped}");
            let saved = match s.execute("file.save", &json!({})) {
                Ok(saved) => saved,
                Err(EngineError::Failed { msg, .. }) => panic!("{key}: the save was refused: {msg}"),
                Err(other) => panic!("{key}: {other:?}"),
            };
            assert_eq!(saved["edited"], true, "{key}");
            let said: Vec<&str> = saved["warnings"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
            if !said.is_empty() {
                // A position's text shows two fields, which a save in place writes as the text they show, and says so (the fields stay Shape Data in DiaCraft's data).
                let fields = key.starts_with("orgchart/") && said.len() == 1 && said[0].contains("new fields written as the text they show");
                assert!(fields || (said.len() == 1 && said[0].contains("outline that is written as a rectangle")), "{key}: {said:?}");
                if !fields {
                    approximated.push(key.clone());
                }
            }
            let again = reopened(&dir);
            assert_eq!(again.document().unwrap().pages.get(0).unwrap().shapes.len(), 5, "{key}: the four rectangles and the shape");
            assert_reads_back_as_made(s.document().unwrap(), again.document().unwrap(), 4, &key);
            // The shape keeps its link to its master, the file carries the master, and the shape's own sections are all it resolves.
            assert_link_holds(s.document().unwrap(), again.document().unwrap(), 4, &key);
            assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
            assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
        }
    }
    assert_eq!(count, 58, "fifty-eight masters in seven stencils");
    // Every master's outline is written as the path it draws: the arcs of the rounded and curved ones as `A` and `W`, and a part that is not filled or not stroked in a set
    // of its own. Before it, thirteen of Basic Flowchart's masters and two of Gane-Sarson's (Process, with rounded corners, and Data Store, an open path) were written as a rectangle
    // for other programs, with a note, and carried exactly by DiaCraft's extension.
    assert!(approximated.is_empty(), "the masters written as a rectangle for other programs: {approximated:?}");
}

/// Gane-Sarson's Process (rounded corners, four arcs) and Data Store (one open path with no fill) are written as the outlines they draw: no note says a rectangle
/// stands in, the path of Data Store is its open three sides in a set with `F` (the right side left open), and the path of Process is its four sides and four arcs, each counterclockwise as seen, closed.
#[test]
fn the_gane_sarson_process_and_data_store_are_their_outlines_for_other_programs() {
    for (master, check) in [("gane-sarson.process", true), ("gane-sarson.data-store", false)] {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &four());
        run(&mut s, "shape.drop", json!({ "master": master, "stencil": "gane-sarson", "page": 0, "x": 4.0, "y": 2.0 }));
        let saved = save(&mut s);
        assert_eq!(saved["warnings"].as_array().unwrap().len(), 0, "{master}: no note that an outline stands in as a rectangle: {saved}");
        let content = content_of(&dir.read("a.odg"));
        let path = content.split("draw:enhanced-path=\"").last().unwrap().split('"').next().unwrap().to_owned();
        assert_ne!(path, "M 0 0 L 21600 0 21600 21600 0 21600 Z N", "{master}: the outline is not the stand-in rectangle");
        if check {
            // The shape is 1.6 by 1 inch with corners of 0.12 inch: the bottom edge from (1620, 21600) to (19980, 21600), then an arc up to the right edge (counterclockwise as seen), and so round.
            assert!(path.starts_with("M 1620 21600 L 19980 21600 A "), "{path}");
            assert!(path.ends_with(" Z N"), "{path}");
            assert_eq!(path.matches(" A ").count(), 4, "four corners, each an arc: {path}");
            assert_eq!(path.matches(" W ").count(), 0, "{path}");
        } else {
            // 1.8 by 0.8 inch: from the top right corner round the left side, open, and no fill.
            assert_eq!(path, "M 21600 0 L 0 0 0 21600 21600 21600 F N", "{master}");
        }
    }
}

/// The nth shape of the page at `page` in `doc`, by its place in the drawing order.
fn id_at(doc: &Document, page: usize, at: usize) -> u32 {
    doc.pages.get(page).unwrap().shapes.get(at).unwrap().id
}

/// The number a shape's cell holds, as the engine reads it.
fn number(doc: &Document, page: usize, at: usize, cell: &str) -> f64 {
    doc.pages
        .get(page)
        .unwrap()
        .shapes
        .get(at)
        .unwrap()
        .sheet
        .cell(cell)
        .and_then(|c| c.value.as_ref().and_then(diacraft_doc::Value::number))
        .unwrap_or(f64::NAN)
}

/// A Data Flow is a connector with an arrowhead at its end and a label. Glued between two shapes and given a label, it is saved, and read back it holds
/// its text, its end arrow and its glue.
#[test]
fn a_data_flow_between_two_shapes_with_a_label_keeps_its_arrow_its_label_and_its_glue() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let flow = run(&mut s, "shape.drop", json!({ "master": "gane-sarson.data-flow", "stencil": "gane-sarson", "page": 0, "x": 8.0, "y": 2.0 }));
    let flow_id = flow["id"].as_u64().unwrap();
    let doc = s.document().unwrap().clone();
    let (a, b) = (id_of(&doc, 0, "A"), id_of(&doc, 0, "B"));
    run(&mut s, "connector.reconnect", json!({ "page": 0, "id": flow_id, "end": "begin", "target": { "id": a } }));
    run(&mut s, "connector.reconnect", json!({ "page": 0, "id": flow_id, "end": "end", "target": { "id": b } }));
    assert_eq!(s.document().unwrap().pages.get(0).unwrap().connects.len(), 2, "the flow is glued at both ends");
    run(&mut s, "shape.set_text", json!({ "page": 0, "id": flow_id, "text": "Order data" }));
    // The instance inherits its end arrow from its master (a connector master keeps its look): it is read through the master.
    let before = {
        let doc = s.document().unwrap();
        let at = doc.find_shape(diacraft_doc::Owner::Page(doc.pages.get(0).unwrap().id), id_at(doc, 0, 4)).unwrap();
        let found =
            diacraft_doc::Resolver::new(doc).resolve(&diacraft_doc::SheetRef::Shape(at), &diacraft_doc::CellAddr::direct("EndArrow")).unwrap();
        found.value.cell.and_then(|c| c.value.as_ref().and_then(diacraft_doc::Value::number)).unwrap_or(f64::NAN)
    };
    assert!(before > 0.0, "the master's end arrow is on the shape: {before}");
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let again = reopened(&dir);
    let (made, read) = (s.document().unwrap(), again.document().unwrap());
    assert_eq!(number(read, 0, 4, "EndArrow"), before, "the end arrow");
    let text = |d: &Document, at: usize| {
        diacraft_engine::draw::text_view(d, d.pages.get(0).unwrap().id, id_at(d, 0, at)).map(|v| v.chars).unwrap_or_default()
    };
    assert_eq!(text(made, 4), "Order data");
    assert_eq!(text(read, 4), "Order data", "the label");
    for end in ["BeginX", "BeginY", "EndX", "EndY"] {
        assert!((number(made, 0, 4, end) - number(read, 0, 4, end)).abs() < 1e-3, "{end}");
    }
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// Each master, edited before its first save (a fill) and after it (moved and, but for the Data Flow, resized: a shape of the file that carries tier 2 is
/// edited), is saved each time, read back as the session has it, and saved again as no edit.
#[test]
fn each_of_the_fifty_masters_is_edited_before_and_after_its_first_save_and_still_reads_back_as_the_session_has_it() {
    for stencil in diacraft_engine::stencil::stencils().iter().take(7) {
        for master in stencil.masters() {
            let key = format!("{}/{}", stencil.id, master.id);
            let dir = Dir::new();
            let mut s = open_drawing(&dir, &four());
            let dropped = run(&mut s, "shape.drop", json!({ "master": master.id, "stencil": stencil.id, "page": 0, "x": 4.0, "y": 2.0 }));
            let id = dropped["id"].as_u64().unwrap();
            run(&mut s, "shape.format", json!({ "ids": [id], "fill": "#ffcc00" }));
            save(&mut s);
            let first = reopened(&dir);
            assert_reads_back_as_made(s.document().unwrap(), first.document().unwrap(), 4, &format!("{key} before the edit"));
            run(&mut s, "shape.move", json!({ "ids": [id], "dx": 1.5, "dy": 0.5 }));
            if master.id != "gane-sarson.data-flow" {
                let doc = s.document().unwrap().clone();
                let frame = diacraft_engine::draw::shape_frame(&doc, doc.pages.get(0).unwrap().id, u32::try_from(id).unwrap()).unwrap();
                let (w, h) = (frame.width * 1.25, frame.height * 0.75);
                run(&mut s, "shape.resize", json!({ "id": id, "x": frame.pin_x - w / 2.0, "y": frame.pin_y - h / 2.0, "width": w, "height": h }));
            }
            let saved = save(&mut s);
            assert_eq!(saved["edited"], true, "{key}: the edit is saved");
            let second = reopened(&dir);
            assert_reads_back_as_made(s.document().unwrap(), second.document().unwrap(), 4, &format!("{key} after the edit"));
            assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
        }
    }
}

/// A connector made with free ends (what `connector.add` does with two places), saved and then moved by itself: its route (`svg:d`, in the page's coordinates)
/// is written where it went and not left where it was. The Data Flow master is a connector of the same kind.
#[test]
fn a_connector_moved_after_its_first_save_is_drawn_where_it_was_moved() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let made = run(&mut s, "connector.add", json!({ "from": { "x": 3.25, "y": 2.0 }, "to": { "x": 4.75, "y": 2.0 } }));
    save(&mut s);
    run(&mut s, "shape.move", json!({ "ids": [made["id"]], "dx": 1.5, "dy": 0.5 }));
    save(&mut s);
    let again = reopened(&dir);
    assert_reads_back_as_made(s.document().unwrap(), again.document().unwrap(), 4, "the moved connector");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// A labeled Data Flow glued between two shapes, saved, and then rerouted by a move of the shape it is glued to: the flow is a shape of the file edited (the engine
/// writes its ends and its label's place again), which the save carries and does not refuse; read back it holds its label and its arrow, and the save after is no edit.
#[test]
fn a_labeled_data_flow_rerouted_after_its_first_save_is_carried_and_keeps_its_label() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let flow = run(&mut s, "shape.drop", json!({ "master": "gane-sarson.data-flow", "stencil": "gane-sarson", "page": 0, "x": 8.0, "y": 2.0 }));
    let flow_id = flow["id"].as_u64().unwrap();
    let doc = s.document().unwrap().clone();
    let (a, b) = (id_of(&doc, 0, "A"), id_of(&doc, 0, "B"));
    run(&mut s, "connector.reconnect", json!({ "page": 0, "id": flow_id, "end": "begin", "target": { "id": a } }));
    run(&mut s, "connector.reconnect", json!({ "page": 0, "id": flow_id, "end": "end", "target": { "id": b } }));
    run(&mut s, "shape.set_text", json!({ "page": 0, "id": flow_id, "text": "Order data" }));
    save(&mut s);
    // The shape it is glued to moves: the engine reroutes the flow.
    run(&mut s, "shape.move", json!({ "ids": [b], "dx": 0.0, "dy": -1.0 }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let again = reopened(&dir);
    let (made, read) = (s.document().unwrap(), again.document().unwrap());
    let text = |d: &Document| {
        d.pages
            .get(0)
            .unwrap()
            .shapes
            .iter()
            .filter_map(|x| diacraft_engine::draw::text_view(d, d.pages.get(0).unwrap().id, x.id).map(|v| v.chars))
            .find(|t| t == "Order data")
    };
    assert_eq!(text(made).as_deref(), Some("Order data"));
    assert_eq!(text(read).as_deref(), Some("Order data"), "the label after the reroute");
    for end in ["BeginX", "BeginY", "EndX", "EndY"] {
        assert!((number(made, 0, 4, end) - number(read, 0, 4, end)).abs() < 1e-3, "{end}");
    }
    assert_eq!(number(read, 0, 4, "EndArrow"), 4.0);
    assert_reads_back_within(made, read, 4, "the rerouted data flow", 16);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}
