//! What the `.odg` writer does not write, as the engine's save shows it: each change is a refused save with the writer's words, nothing is
//! written and the session is as it was; and a carriage return is never put into the model by a command, and refused in words when it came another way.
//! What no attribute of the file can say is not among them: DiaCraft's extension carries it, and the save is made and reads back.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Shape, Text, TextPart, Value as CellValue};
use diacraft_engine::{EngineError, Session};
use serde_json::{Value, json};

use crate::odg_build::{Dir, Drawing, id_of, open_drawing, rect};
use crate::odg_file::{four, refused, text};
use crate::{run, snapshot};

/// Puts a cell into the shape `name`'s own sheet directly in the model, as a command that wrote it would have.
fn put_cell(s: &mut Session, name: &str, cell_name: &str, cell: Cell) {
    s.edit("Test edit", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let at = page.shapes.iter().position(|x| x.name_u.as_deref() == Some(name)).unwrap();
        let shape = Arc::make_mut(page.shapes.get_mut(at).unwrap());
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new(cell_name), cell);
        Ok(())
    })
    .unwrap();
}

/// Changes the type of a row of the first `Geometry` section of the shape `name`.
fn set_row_type(s: &mut Session, name: &str, row: usize, kind: &str) {
    s.edit("Test edit", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let at = page.shapes.iter().position(|x| x.name_u.as_deref() == Some(name)).unwrap();
        let shape = Arc::make_mut(page.shapes.get_mut(at).unwrap());
        let section = Arc::make_mut(Arc::make_mut(&mut shape.sheet).sections.iter_mut().find(|x| x.name.as_str() == "Geometry").unwrap());
        Arc::make_mut(section.rows.get_mut(row).unwrap()).row_type = Some(Arc::from(kind));
        Ok(())
    })
    .unwrap();
}

/// Sets the text of the shape `name` to exactly `text`, whatever a command would have let in.
fn inject_text(s: &mut Session, name: &str, text: &str) {
    s.edit("Test edit", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let at = page.shapes.iter().position(|x| x.name_u.as_deref() == Some(name)).unwrap();
        Arc::make_mut(page.shapes.get_mut(at).unwrap()).text =
            Some(Arc::new(Text { parts: vec![TextPart::Chars(Arc::from(text))], ..Default::default() }));
        Ok(())
    })
    .unwrap();
}

/// Each change the writer does not write (part 1's refusals) ends the save as a refusal in the writer's words, with nothing written and the session as it
/// was. The person who takes the change back (the undo of a command) can save again.
#[test]
fn each_change_the_writer_does_not_write_is_refused_in_its_words_and_nothing_changes() {
    type Setup = Box<dyn Fn(&mut Session)>;
    let grouped = Drawing::new().page(&format!(
        "<draw:g draw:name=\"G\">{}</draw:g>{}",
        rect("M", (2.0, 3.0, 4.0, 2.0), ""),
        rect("A", (12.0, 3.0, 4.0, 2.0), "hello")
    ));
    let into_the_group = |s: &mut Session| {
        s.edit("Test edit", |doc| {
            let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
            let group = Arc::make_mut(page.shapes.get_mut(0).unwrap());
            let mut copy = Shape::clone(group.children.get(0).unwrap());
            copy.id = 99;
            group.children.push(Arc::new(copy));
            Ok(())
        })
        .unwrap();
    };
    let cases: Vec<(&str, Drawing, Setup, &str)> = vec![
        ("an arc in an outline", four(), Box::new(|s| set_row_type(s, "A", 2, "ArcTo")), "an outline whose rows are not the plain lines and curves"),
        (
            "a NURBS in an outline",
            four(),
            Box::new(|s| set_row_type(s, "A", 3, "NURBSTo")),
            "an outline whose rows are not the plain lines and curves",
        ),
        ("a new shape in a group", grouped, Box::new(into_the_group), "a new shape put in a group"),
    ];
    for (case, drawing, setup, words) in cases {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &drawing);
        setup(&mut s);
        let (file, held) = (dir.read("a.odg"), snapshot(&s));
        let said = refused(&mut s);
        assert!(said.contains("the drawing could not be saved: saving does not yet write this change:") && said.contains(words), "{case}: {said}");
        assert_eq!(dir.read("a.odg"), file, "{case}: nothing was written");
        assert_eq!(snapshot(&s), held, "{case}: the session is as it was");
        assert!(s.is_dirty(), "{case}: and still has the change");
        assert_eq!(dir.files(), ["a.odg"], "{case}: no file was left beside it");
    }
}

/// The formula and the value of a cell of the shape `name` on the first page, as text.
fn cell_text(s: &Session, name: &str, cell: &str) -> Option<(Option<String>, Option<String>)> {
    let doc = s.document()?;
    let shape = doc.pages.get(0)?.shapes.iter().find(|x| x.name_u.as_deref() == Some(name))?;
    let c = shape.sheet.cell(cell)?;
    Some((c.formula.text().map(str::to_owned), c.value.as_ref().map(|v| v.text().to_owned())))
}

/// A dash, a gradient, a hatch and a formula in a style cell are what no attribute of the file says: the style's own attributes keep what they
/// say, DiaCraft's extension carries the cell, and the save is made, reads back as the model holds it, and saves again as no edit.
#[test]
fn a_change_no_attribute_of_the_file_can_say_is_carried_by_the_extension_saved_and_read_back() {
    type Setup = Box<dyn Fn(&mut Session)>;
    let a = |s: &Session| id_of(s.document().unwrap(), 0, "A");
    let gradient = |s: &mut Session| put_cell(s, "A", "FillGradientEnabled", Cell::constant(CellValue::from_text("1")));
    let hatch = |s: &mut Session| put_cell(s, "A", "FillPattern", Cell::constant(CellValue::from_text("5")));
    let formula = |s: &mut Session| put_cell(s, "A", "LineWeight", Cell::with_formula("0.1+0.1", Some(CellValue::from_text("0.2"))));
    // What each case is called, how it is made, the cell the file must hold, and its formula and value.
    type Case<'a> = (&'a str, Setup, &'a str, (Option<&'a str>, Option<&'a str>));
    let cases: Vec<Case<'_>> = vec![
        ("a dashed line", Box::new(move |s| drop(run(s, "shape.format", json!({ "ids": [a(s)], "pattern": 3 })))), "LinePattern", (None, Some("3"))),
        ("a gradient", Box::new(gradient), "FillGradientEnabled", (None, Some("1"))),
        ("a hatch", Box::new(hatch), "FillPattern", (None, Some("5"))),
        ("a formula in a style cell", Box::new(formula), "LineWeight", (Some("0.1+0.1"), Some("0.2"))),
    ];
    for (case, setup, cell, (formula, value)) in cases {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &four());
        setup(&mut s);
        let saved = crate::odg_file::save(&mut s);
        assert_eq!(saved["edited"], true, "{case}: {saved}");
        let mut again = Session::new();
        run(&mut again, "file.open", json!({ "path": dir.path("a.odg") }));
        let (f, v) = cell_text(&again, "A", cell).unwrap_or_else(|| panic!("{case}: the file has no {cell}"));
        assert_eq!((f.as_deref(), v.as_deref()), (formula, value), "{case}: what the file says of {cell}");
        crate::odg_file::assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    }
}

/// A new shape on a page whose shapes carry `draw:z-index` is saved with the attribute (the save refused it before a change), reads back, and saves again as no
/// edit.
#[test]
fn a_new_shape_on_a_page_whose_shapes_carry_z_index_is_saved_with_the_attribute() {
    let dir = Dir::new();
    let page = Drawing::new().page(
        &[rect("A", (2.0, 3.0, 4.0, 2.0), "hello"), rect("B", (12.0, 3.0, 4.0, 2.0), "")]
            .concat()
            .replace(" draw:name=\"A\"", " draw:z-index=\"1\" draw:name=\"A\"")
            .replace(" draw:name=\"B\"", " draw:z-index=\"2\" draw:name=\"B\""),
    );
    let mut s = open_drawing(&dir, &page);
    run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }));
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["edited"], true, "{saved}");
    let content = crate::odg_build::content_of(&dir.read("a.odg"));
    assert!(content.matches("draw:z-index=").count() == 3, "the two of the file and the new shape's: {content}");
    crate::odg_file::assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_eq!(run(&mut s, "file.save", json!({}))["edited"], false);
}

/// A command that was refused by the writer is undone, and the drawing saves: the refusal leaves the way back.
#[test]
fn a_refused_command_is_undone_and_the_drawing_saves_again() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    // (An arc in an outline: a dash is no longer refused, carries it.)
    set_row_type(&mut s, "A", 2, "ArcTo");
    let said = refused(&mut s);
    assert!(said.contains("an outline whose rows are not the plain lines and curves"), "{said}");
    run(&mut s, "edit.undo", json!({}));
    assert!(!s.is_dirty(), "the drawing is the one the file holds again");
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["edited"], false);
}

/// Found by the random walks (`odg_random.rs`): a connector the file holds, glued to a shape the file does not hold. An undo put a shape back after a save that deleted it, with
/// the glue of a connector of the file. The writer wrote the glue of the connectors it makes and refused a glue made anew between shapes of the file, and passed this one by:
/// the save was made and the file lacked a glue the drawing had. The save wrote nothing then, in words; **it writes the shape and the glue**, and the file
/// reads back with the connector glued to the shape again.
#[test]
fn a_connector_of_the_file_glued_to_a_shape_the_file_does_not_hold_is_saved_with_its_glue() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let made = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }));
    let a = id_of(s.document().unwrap(), 0, "A");
    run(&mut s, "file.save", json!({}));
    run(&mut s, "connector.add", json!({ "from": { "id": made["id"] }, "to": { "id": a } }));
    assert_eq!(run(&mut s, "file.save", json!({}))["edited"], true);
    run(&mut s, "shape.delete", json!({ "ids": [made["id"]] }));
    assert_eq!(run(&mut s, "file.save", json!({}))["edited"], true);
    // The delete is taken back: the shape is not in the file, and the connector that is has its glue to it again.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(run(&mut s, "file.save", json!({}))["edited"], true, "the shape and the glue are written");
    crate::odg_file::assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    // Taking back the connector too leaves a shape and nothing glued: it saves, and reads back as it is.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(run(&mut s, "file.save", json!({}))["edited"], true);
    crate::odg_file::assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
}

/// A carriage return is not put into the model by any command that takes text: a new line is a line feed.
#[test]
fn the_text_commands_never_put_a_carriage_return_into_the_model() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let a = id_of(s.document().unwrap(), 0, "A");
    let before = snapshot(&s);
    let tries: Vec<(&str, Value)> = vec![
        ("shape.set_text", json!({ "id": a, "text": "one\r\ntwo" })),
        ("shape.set_text", json!({ "id": a, "text": "x\ry", "start": 1, "end": 2 })),
        ("shape.set_text", json!({ "id": a, "text": "\r", "start": 0, "end": 0 })),
        ("text.box", json!({ "page": 0, "x": 4.0, "y": 2.0, "text": "q\rr" })),
    ];
    for (command, params) in tries {
        let e = s.execute(command, &params).unwrap_err();
        assert!(matches!(&e, EngineError::BadParams { .. }) && e.to_string().contains("control characters"), "{command} {params}: {e:?}");
        assert_eq!(snapshot(&s), before, "{command}: the model is as it was");
    }
    // A line feed is what a new line is, and the text with one is the text.
    run(&mut s, "shape.set_text", json!({ "id": a, "text": "one\ntwo" }));
    assert_eq!(text(&s, 0, "A"), "one\ntwo");
}

/// A carriage return that arrived some other way than a command (a model built by other code) is refused by the save in words, for a shape of the file
/// and for a new one.
#[test]
fn a_carriage_return_in_the_model_refuses_the_save_in_words() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    inject_text(&mut s, "B", "line one\rline two");
    let (file, held) = (dir.read("a.odg"), snapshot(&s));
    let said = refused(&mut s);
    assert!(said.contains("a text with a carriage return") && said.contains("a new line is a line feed"), "{said}");
    assert_eq!((dir.read("a.odg"), snapshot(&s)), (file, held));
    // In a new shape too.
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let made = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }));
    inject_text(&mut s, made["name"].as_str().unwrap(), "a\r\nb");
    let said = refused(&mut s);
    assert!(said.contains("a text with a carriage return"), "{said}");
    assert!(s.is_dirty());
}
