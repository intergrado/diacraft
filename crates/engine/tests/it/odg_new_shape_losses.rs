//! Nothing on a new shape is dropped without a word. The copy of the drawing a save hands the writer carries
//! what the writer writes, and **every other cell, section and part of the text a new shape holds is a kind of loss that a warning of the save names** (`odgfile/flatten.rs`,
//! "Nothing on a new shape is dropped without a word"). The first version of the copy carried a list and dropped the rest: a rectangle dropped, given a text and
//! formatted (bold, 40 points, red, left) was saved with `edited: true` and no warning, the file opened centred, plain and black, and the save after it said that
//! nothing had changed.
//!
//! Each case saves through `file.save`, reads the file back through the engine's own reader, and compares what the model draws with what the file draws, as the window
//! does: they **match, or a warning of that save names the difference**.

use diacraft_engine::Session;
use serde_json::{Value, json};

use crate::odg_build::{Dir, id_of, open_drawing};
use crate::odg_file::{assert_saves_again_as_no_edit, four, look, save};
use crate::run;

/// The page drawn at 40 pixels to the inch on white paper: its size and its pixels.
fn render(doc: &diacraft_doc::Document) -> (u32, u32, Vec<u8>) {
    let page = doc.pages.get(0).unwrap().id;
    let view = diacraft_render::View::fit_page(doc, page, 40.0).unwrap();
    let mut renderer = diacraft_render::Renderer::with_shared_fonts(diacraft_engine::measure::renderer_fonts());
    renderer.set_paper(Some(diacraft_color::Rgba::WHITE));
    let out = renderer.render(doc, page, &view);
    (out.width, out.height, out.rgba_premul)
}

/// How many pixels the model and the file drawn again differ in (by more than a few levels of a channel), and how many there are.
fn pixels_apart(model: &Session, file: &Session) -> usize {
    let (wa, ha, pa) = render(model.document().unwrap());
    let (wb, hb, pb) = render(file.document().unwrap());
    assert_eq!((wa, ha), (wb, hb), "the page is the size it was");
    pa.chunks(4).zip(pb.chunks(4)).filter(|(x, y)| x.iter().zip(y.iter()).any(|(p, q)| p.abs_diff(*q) > 24)).count()
}

/// The file the session saved, opened by another session.
fn reopened(dir: &Dir) -> Session {
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path("a.odg") }));
    again
}

fn warnings_of(saved: &Value) -> Vec<String> {
    saved["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str().map(str::to_owned)).collect()
}

fn position(s: &Session, id: u64) -> usize {
    s.document().unwrap().pages.get(0).unwrap().shapes.iter().position(|x| u64::from(x.id) == id).unwrap()
}

/// A rectangle dropped on the first page of `four()` and given `text`.
fn rectangle_with(s: &mut Session, text: &str) -> u64 {
    let made = run(s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }));
    let id = made["id"].as_u64().unwrap();
    run(s, "shape.set_text", json!({ "id": id, "text": text }));
    id
}

/// The reviewer's input: a rectangle dropped, given a text and formatted whole (bold, 40 points, red, left), saved. **Nothing of it is lost**: the writer
/// carries the `Character` and `Paragraph` rows and the text's markers in DiaCraft's own data on the shape's style, so the save says nothing, the file read again holds the
/// formatting the model holds, and the two drawings are the same picture.
#[test]
fn text_formatting_of_a_new_shape_is_carried_and_the_file_draws_what_the_model_draws() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let id = rectangle_with(&mut s, "Hello there");
    run(&mut s, "text.format", json!({ "ids": [id], "bold": true, "size": 40.0, "color": "#ff0000", "align": "left" }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true);
    assert_eq!(warnings_of(&saved), Vec::<String>::new(), "nothing is said: {saved}");
    let mut file = reopened(&dir);
    let at = position(&s, id);
    let (model_look, file_look) = (look(&mut s, 0, at), look(&mut file, 0, at));
    assert_eq!(model_look, file_look, "the file holds the formatting the model does");
    assert_eq!(pixels_apart(&s, &file), 0, "and the two drawings are the same picture");
    // The text itself is saved.
    assert_eq!(crate::odg_file::text_at(&file, 0, at), "Hello there");
    // The same drawing saved again is no edit and says nothing.
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    // The model is as it was: the formatting is still in it.
    let doc = s.document().unwrap();
    let shape = doc.pages.get(0).unwrap().shapes.get(at).unwrap();
    assert!(shape.sheet.sections.iter().any(|x| x.name.as_str() == "Character"), "the model keeps its Character rows");
}

/// A range of characters formatted leaves runs in the text of the new shape (the markers `Cp`), which the writer carries whole in DiaCraft's own data: the save
/// says nothing, and the file is the same picture.
#[test]
fn a_range_of_formatted_text_of_a_new_shape_is_carried_and_the_file_draws_what_the_model_draws() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let id = rectangle_with(&mut s, "Hello there");
    run(&mut s, "text.format", json!({ "ids": [id], "start": 2, "end": 5, "italic": true }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true);
    assert_eq!(warnings_of(&saved), Vec::<String>::new(), "{saved}");
    let file = reopened(&dir);
    assert_eq!(crate::odg_file::text_at(&file, 0, position(&s, id)), "Hello there", "the characters are all there");
    assert_eq!(pixels_apart(&s, &file), 0, "and the formatting of the range is the picture's");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// The text block of a new text box and of a new rectangle, moved: the block is a cell of the shape (`TxtPinX` and the rest), which the writer carries in DiaCraft's own data
/// So the save says nothing and the file draws the text where the model has it.
#[test]
fn a_moved_text_block_of_a_new_text_box_and_of_a_new_rectangle_is_carried() {
    for rectangle in [false, true] {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &four());
        let id = if rectangle {
            rectangle_with(&mut s, "Moved words")
        } else {
            run(&mut s, "text.box", json!({ "page": 0, "x": 4.0, "y": 2.0, "width": 2.0, "height": 0.5, "text": "Boxed words" }))["id"]
                .as_u64()
                .unwrap()
        };
        let first = save(&mut s);
        assert_eq!(first["warning_count"], 0, "rectangle={rectangle}: a text block that is the shape is no loss: {first}");
        let file = reopened(&dir);
        assert_eq!(pixels_apart(&s, &file), 0, "rectangle={rectangle}: and the file draws what the model draws");
        run(&mut s, "text.block_move", json!({ "id": id, "dx": 1.5, "dy": 1.0 }));
        let saved = save(&mut s);
        assert_eq!(warnings_of(&saved), Vec::<String>::new(), "rectangle={rectangle}: {saved}");
        let file = reopened(&dir);
        assert_eq!(pixels_apart(&s, &file), 0, "rectangle={rectangle}: the file draws the text where the model has it");
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    }
}

/// The ten shapes of the built-in stencils that the writer writes, each with a text: the file draws what the model draws, and the save says nothing. The decision and
/// the on-page reference hold their text in a block inset in the shape (the master's own), which the writer does not write: the shape keeps its link to the master
/// and the file carries the master, so a file read again has the inset (until a later change the save said the text block was not written, and the file drew the text across
/// the whole shape).
#[test]
fn each_writable_master_with_a_text_is_drawn_as_the_model_draws_it_and_the_save_says_nothing() {
    let masters = [
        ("basic", "rectangle"),
        ("basic", "square"),
        ("basic", "ellipse"),
        ("basic", "circle"),
        ("basic", "diamond"),
        ("basic", "hexagon"),
        ("basic", "octagon"),
        ("flowchart", "process"),
        ("flowchart", "decision"),
        ("flowchart", "on_page_reference"),
    ];
    for (stencil, master) in masters {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &four());
        let made = run(&mut s, "shape.drop", json!({ "master": master, "stencil": stencil, "page": 0, "x": 4.0, "y": 2.0 }));
        run(&mut s, "shape.set_text", json!({ "id": made["id"], "text": "Some words" }));
        let saved = save(&mut s);
        let said = warnings_of(&saved);
        let apart = pixels_apart(&s, &reopened(&dir));
        assert_eq!(apart, 0, "{stencil}/{master}: {apart} pixels apart: {said:?}");
        assert!(said.is_empty(), "{stencil}/{master}: {said:?}");
    }
}

/// A new line or connector with a text (it was a loss the save told, since the file was not given the text; a Data Flow is a connector whose label is
/// what it is for): the text is written as the `text:p` the element holds, read back, and the save says nothing.
#[test]
fn the_text_of_a_new_connector_is_saved_and_reads_back() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let a = id_of(s.document().unwrap(), 0, "A");
    let b = id_of(s.document().unwrap(), 0, "B");
    let made = run(&mut s, "connector.add", json!({ "from": { "id": a }, "to": { "id": b } }));
    run(&mut s, "shape.set_text", json!({ "id": made["id"], "text": "label" }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true);
    assert_eq!(warnings_of(&saved), Vec::<String>::new(), "{saved}");
    assert!(crate::odg_build::content_of(&dir.read("a.odg")).contains("<text:p>label</text:p>"), "the connector holds its text");
    let again = reopened(&dir);
    let last = |d: &diacraft_doc::Document| d.pages.get(0).unwrap().shapes.len() - 1;
    let at = last(again.document().unwrap());
    let doc = again.document().unwrap();
    let words =
        diacraft_engine::draw::text_view(doc, doc.pages.get(0).unwrap().id, doc.pages.get(0).unwrap().shapes.get(at).unwrap().id).map(|v| v.chars);
    assert_eq!(words.as_deref(), Some("label"));
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// Each other kind of what a new shape holds is carried, and none is said: shape data (a `Property` row, a `User` row), the layer, a cell, a section and the
/// background of the text. The file read again holds each as the model has it.
#[test]
fn each_other_kind_a_new_shape_holds_is_carried_and_not_said() {
    let cases: [(&str, &str); 6] = [
        ("a Property row", "Property"),
        ("a User row", "User"),
        ("a layer", "LayerMember"),
        ("an unknown cell", "LockMoveX"),
        ("an unknown section", "Actions"),
        ("the background of the text", "TextBkgnd"),
    ];
    for (what, name) in cases {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &four());
        let made = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }));
        let id = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
        s.edit("Test edit", |doc| {
            let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
            let at = page.shapes.iter().position(|x| x.id == id).unwrap();
            let shape = std::sync::Arc::make_mut(page.shapes.get_mut(at).unwrap());
            let sheet = std::sync::Arc::make_mut(&mut shape.sheet);
            match name {
                "Property" | "User" | "Actions" => {
                    let mut section = diacraft_doc::Section::new(name, None);
                    let row = diacraft_doc::Row { name: Some(std::sync::Arc::from("Row_1")), ..diacraft_doc::Row::default() };
                    section.rows.push(std::sync::Arc::new(row));
                    sheet.sections.push(std::sync::Arc::new(section));
                }
                cell => {
                    sheet.cells.insert(diacraft_doc::CellName::new(cell), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("1")));
                }
            }
            Ok(())
        })
        .unwrap();
        let saved = save(&mut s);
        assert_eq!(saved["edited"], true, "{what}");
        assert_eq!(warnings_of(&saved), Vec::<String>::new(), "{what}: {saved}");
        let file = reopened(&dir);
        let at = position(&s, u64::from(id));
        let read = file.document().unwrap().pages.get(0).unwrap().shapes.get(at).unwrap().clone();
        let found = read.sheet.cell(name).is_some() || read.sheet.sections.iter().any(|x| x.name.as_str() == name);
        assert!(
            found,
            "{what}: the file read again holds it: cells {:?}",
            read.sheet.cells.keys().map(|k| k.as_str().to_owned()).collect::<Vec<_>>()
        );
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    }
}

/// What the writer writes of a new shape's fill and line reaches the file (the review's mutants that dropped the line cap, the transparencies, the margins and the
/// vertical alignment from the copy were caught only by the count of steps): each is set on a new shape, and the file read again holds it.
#[test]
fn the_fill_the_line_the_margins_and_the_alignment_of_a_new_shape_reach_the_file() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let made = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }));
    let id = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
    let cells: [(&str, &str, Option<&str>); 8] = [
        ("LineCap", "1", None),
        ("FillForegndTrans", "0.5", None),
        ("LineColorTrans", "0.25", None),
        ("TopMargin", "0.1", Some("IN")),
        ("BottomMargin", "0.2", Some("IN")),
        ("LeftMargin", "0.3", Some("IN")),
        ("RightMargin", "0.4", Some("IN")),
        ("VerticalAlign", "2", None),
    ];
    s.edit("Test edit", |doc| {
        let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let at = page.shapes.iter().position(|x| x.id == id).unwrap();
        let shape = std::sync::Arc::make_mut(page.shapes.get_mut(at).unwrap());
        let sheet = std::sync::Arc::make_mut(&mut shape.sheet);
        for (name, value, unit) in cells {
            let mut cell = diacraft_doc::Cell::constant(diacraft_doc::Value::from_text(value));
            cell.unit = unit.map(std::sync::Arc::from);
            sheet.cells.insert(diacraft_doc::CellName::new(name), cell);
        }
        Ok(())
    })
    .unwrap();
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true);
    assert!(warnings_of(&saved).is_empty(), "all of them are written: {saved}");
    let file = reopened(&dir);
    let at = position(&s, u64::from(id));
    let shape = file.document().unwrap().pages.get(0).unwrap().shapes.get(at).unwrap().clone();
    let number = |name: &str| shape.sheet.cell(name).and_then(|c| c.value.as_ref()?.number());
    assert_eq!(number("LineCap"), Some(1.0), "the line cap");
    assert_eq!(number("FillForegndTrans"), Some(0.5), "the transparency of the fill");
    assert_eq!(number("LineColorTrans"), Some(0.25), "the transparency of the line");
    for (name, inches) in [("TopMargin", 0.1), ("BottomMargin", 0.2), ("LeftMargin", 0.3), ("RightMargin", 0.4)] {
        assert!(number(name).is_some_and(|n| (n - inches).abs() < 1e-3), "{name}: {:?}", number(name));
    }
    assert_eq!(number("VerticalAlign"), Some(2.0), "the vertical alignment");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// A new shape that is flipped or turned is saved the way round the drawing has it (it was refused by the writer in words, "written with a later task"): the
/// file draws the shape's box where the drawing has it, so that the file is not given a shape the right way round that the drawing has the other way (the review's mutant
/// that dropped the flips from the copy): the corners of the box, in the order of the shape's own frame, are the same in a session that opens the file, and so are the flips.
#[test]
fn a_flipped_or_turned_new_shape_is_saved_the_way_round_the_drawing_has_it() {
    for (command, params) in [
        ("shape.flip", json!({ "axis": "horizontal" })),
        ("shape.flip", json!({ "axis": "vertical" })),
        ("shape.rotate", json!({ "angle": 45 })),
        ("shape.rotate", json!({ "angle": 90 })),
        ("shape.rotate", json!({ "angle": -30 })),
    ] {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &four());
        // A right triangle is not symmetrical: flipped or turned the wrong way, it is not the shape the drawing has.
        let made = run(&mut s, "shape.drop", json!({ "master": "right_triangle", "page": 0, "x": 4.0, "y": 2.0 }));
        let mut params = params;
        params["ids"] = json!([made["id"]]);
        run(&mut s, command, params.clone());
        let saved = save(&mut s);
        assert_eq!(saved["edited"], true, "{command} {params}");
        assert!(warnings_of(&saved).is_empty(), "{command} {params}: {saved}");
        let file = reopened(&dir);
        let at = position(&s, made["id"].as_u64().unwrap());
        let corners = |s: &Session| {
            let doc = s.document().unwrap();
            let page = doc.pages.get(0).unwrap();
            diacraft_engine::draw::shape_corners(doc, page.id, page.shapes.get(at).unwrap().id).unwrap()
        };
        let (mine, theirs) = (corners(&s), corners(&file));
        for (a, b) in mine.iter().zip(theirs.iter()) {
            assert!(
                (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6,
                "{command} {params}: the box is at {mine:?} in the drawing and {theirs:?} in the file"
            );
        }
        let flip = |s: &Session, cell: &str| {
            let doc = s.document().unwrap();
            doc.pages.get(0).unwrap().shapes.get(at).unwrap().sheet.cell(cell).and_then(|c| c.value.as_ref()?.number()).unwrap_or(0.0)
        };
        for cell in ["FlipX", "FlipY"] {
            assert_eq!(flip(&s, cell), flip(&file, cell), "{command} {params}: {cell}");
        }
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    }
}
