//! Every master of the five built-in stencils is dropped into an `.odg` drawing, saved, read back as the session had it and saved again as no edit
//! (the twenty masters of were refused by a gate until a later change). A save that only returns Ok proves nothing about the shape: the file is opened
//! again and the shape is compared with the session's by what it draws and where it connects.

use diacraft_doc::Document;
use diacraft_engine::draw::connection_points;
use diacraft_engine::{EngineError, Session};
use diacraft_render::{Renderer, View};
use serde_json::{Value, json};

use crate::odg_build::{Dir, id_of, open_drawing};
use crate::odg_file::{assert_reopens_as_saved, assert_saves_again_as_no_edit, four, save};
use crate::odg_masters::assert_link_holds;
use crate::run;

/// The first page, drawn at 50 pixels to the inch on white paper: its size and its premultiplied pixels. Nothing may be left out of the frame.
fn drawn(doc: &Document) -> (u32, u32, Vec<u8>) {
    let page = doc.pages.get(0).unwrap().id;
    let view = View::fit_page(doc, page, 50.0).unwrap();
    let mut renderer = Renderer::new();
    renderer.set_paper(Some(diacraft_color::Rgba::WHITE));
    let frame = renderer.render(doc, page, &view);
    assert!(frame.skipped.is_empty(), "everything was drawn: {:?}", frame.skipped);
    (frame.width, frame.height, frame.rgba_premul)
}

/// The most the pixels of two drawings may differ by, in one channel: a path read back at the precision the file holds is the same path.
const PIXEL_SLACK: i32 = 2;

/// Asserts that the drawing `read` (a file read again) shows what `made` (the session's own) shows, and that the shape at `at` has the connection points it
/// had: as many, at the same places, the same way out.
#[track_caller]
fn assert_reads_back_as_made(made: &Document, read: &Document, at: usize, what: &str) {
    assert_reads_back_within(made, read, at, what, PIXEL_SLACK);
}

/// [`assert_reads_back_as_made`] with `slack` for the most a channel of a pixel may differ by. A route written as a path is in hundredths of a millimetre, which moves a
/// hairline by a hundredth of a pixel and its anti-aliasing by a few levels.
#[track_caller]
fn assert_reads_back_within(made: &Document, read: &Document, at: usize, what: &str, slack: i32) {
    let (a, b) = (drawn(made), drawn(read));
    assert_eq!((a.0, a.1, a.2.len()), (b.0, b.1, b.2.len()), "{what}: the picture's size");
    let worst = a.2.iter().zip(b.2.iter()).map(|(x, y)| (i32::from(*x) - i32::from(*y)).abs()).max().unwrap_or(0);
    assert!(worst <= slack, "{what}: the file draws the shape differently from the session (the worst channel differs by {worst})");
    assert!(a.2.iter().any(|v| *v != 255), "{what}: the picture is not blank");
    let page = |d: &Document| d.pages.get(0).unwrap().id;
    let id = |d: &Document| d.pages.get(0).unwrap().shapes.get(at).unwrap().id;
    let (pa, pb) = (connection_points(made, page(made), id(made)), connection_points(read, page(read), id(read)));
    // A connector a person made (a Data Flow, or what `connector.add` makes) has none, and the file says so, though the reader gives every shape the four standard rows
    // Answered the question of the divergence every connector saved had, which read back with four.
    assert_eq!(pa.len(), pb.len(), "{what}: the connection points, {pa:?} against {pb:?}");
    for (x, y) in pa.iter().zip(pb.iter()) {
        let near = |p: f64, q: f64| (p - q).abs() < 1e-3;
        assert!(
            x.index == y.index && near(x.x, y.x) && near(x.y, y.y) && near(x.dir_x, y.dir_x) && near(x.dir_y, y.dir_y),
            "{what}: a connection point is {x:?} in the session and {y:?} in the file"
        );
    }
}

/// The session `dir`'s `a.odg` is read into.
fn reopened(dir: &Dir) -> Session {
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path("a.odg") }));
    again
}

/// All fifty masters: dropped on their own into a drawing of four rectangles, saved (the save says nothing, but for an outline written as a rectangle for other
/// programs), read back and compared, and saved again as no edit.
#[test]
fn each_of_the_fifty_masters_is_saved_reads_back_as_the_session_had_it_and_saves_again_as_no_edit() {
    let mut count = 0;
    let mut approximated = Vec::new();
    for stencil in diacraft_engine::stencil::stencils() {
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
    // The masters whose outline has an arc, a part that is not filled or another row that a path of lines and curves cannot say are written as a rectangle for
    // other programs and carried as tier 2 (which the pictures above show is read back exactly): thirteen of Basic Flowchart's and two of the new
    // ones, Gane-Sarson's Process (rounded corners) and Data Store (an open path). The other eighteen of the twenty new masters are tier 1.
    assert_eq!(
        approximated,
        [
            "flowchart/terminator",
            "flowchart/data",
            "flowchart/document",
            "flowchart/multiple_documents",
            "flowchart/predefined_process",
            "flowchart/manual_operation",
            "flowchart/preparation",
            "flowchart/database",
            "flowchart/internal_storage",
            "flowchart/delay",
            "flowchart/display",
            "flowchart/summing_junction",
            "flowchart/or",
            "gane-sarson/gane-sarson.process",
            "gane-sarson/gane-sarson.data-store",
        ],
        "the masters written as a rectangle for other programs"
    );
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
    let before = number(s.document().unwrap(), 0, 4, "EndArrow");
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
    for stencil in diacraft_engine::stencil::stencils() {
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
