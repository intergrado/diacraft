//! The rest of the review's fixes to the engine's `.odg` path: the way out of a refusal and the warnings at the time of the action
//! And the behaviours the review's mutants showed no test held (the masters a drop adds, the file's stamp after a save, the stencils an open puts back).

use diacraft_engine::{EngineError, Session};
use serde_json::{Value, json};

use crate::odg_build::{Dir, id_of, open_drawing};
use crate::odg_file::{assert_reopens_as_saved, assert_saves_again_as_no_edit, four, refused, save};
use crate::run;

fn warnings(answer: &Value) -> Vec<String> {
    answer.get("warnings").and_then(Value::as_array).map(|w| w.iter().filter_map(|t| t.as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

/// **Every refusal says the way out, and the way out works**: an `.odg` is saved only as an `.odg`, so a change the writer cannot write
/// traps the work unless the person is told to take it back. Each refusal the engine makes of an `.odg` save, whoever makes it (the writer, the flattening, the guard of a
/// glue), tells the change and says to undo it, and the undo is followed by a save.
#[test]
fn each_refusal_of_an_odg_save_says_to_undo_the_change_and_the_undo_saves() {
    type Change = Box<dyn Fn(&mut Session)>;
    // (A dashed line, a triangle, the rounded corners of a shape of the file, a master of one of the new stencils and a curved new connector are no longer among them:
    // the extension carries a dash and a rounding no attribute says and the connection points of a triangle, the twenty masters of are written, a turned
    // new shape is written too, and a curved connector is written as cubics. What is left is a change of the outline of a shape of the file to
    // rows the writer cannot say as a path.)
    let cases: [(&str, Change); 1] = [(
        "an arc in the outline of a shape of the file",
        Box::new(|s| {
            s.edit("Test edit", |doc| {
                let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
                let at = page.shapes.iter().position(|x| x.name_u.as_deref() == Some("A")).unwrap();
                let shape = std::sync::Arc::make_mut(page.shapes.get_mut(at).unwrap());
                let sections = &mut std::sync::Arc::make_mut(&mut shape.sheet).sections;
                let section = std::sync::Arc::make_mut(sections.iter_mut().find(|x| x.name.as_str() == "Geometry").unwrap());
                std::sync::Arc::make_mut(section.rows.get_mut(2).unwrap()).row_type = Some(std::sync::Arc::from("ArcTo"));
                Ok(())
            })
            .unwrap();
        }),
    )];
    for (what, change) in cases {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &four());
        // The change is one undo step of its own, or two: a connector made and then given a style, and the style is the step to take back.
        change(&mut s);
        let said = refused(&mut s);
        assert!(said.contains("could not be saved") && said.contains("Undo the change (edit.undo) to save"), "{what}: {said}");
        assert!(said.chars().count() <= 601, "{what}: {said}");
        // Undo until the drawing saves: the refusals above are each one command from a drawing that does (the style after the connector).
        let mut undone = 0;
        while s.execute("file.save", &json!({})).is_err() {
            run(&mut s, "edit.undo", json!({}));
            undone += 1;
            assert!(undone <= 2, "{what}: still refused after two undos");
        }
        assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    }
}

/// **A shape put back by an undo, with a connector of the file glued to it, is refused with the way out that works** (the review, parked item 3):
/// deleting both ends of a connector and saving, then undoing the delete, leaves a connector glued to shapes the file no longer holds. The refusal used to say to
/// undo the change, which goes round (the undo is the change). It names a redo, which gives the drawing the file has, and the redo saves.
#[test]
fn a_shape_an_undo_put_back_is_refused_with_a_redo_as_the_way_out_and_the_redo_saves() {
    let dir = Dir::new();
    let ided = |text: String, id: &str| text.replacen("<draw:custom-shape ", &format!("<draw:custom-shape xml:id=\"{id}\" draw:id=\"{id}\" "), 1);
    let shapes = [
        ided(crate::odg_build::rect("A", (2.0, 3.0, 4.0, 2.0), ""), "id1"),
        ided(crate::odg_build::rect("B", (12.0, 3.0, 4.0, 2.0), ""), "id2"),
        crate::odg_build::glued("K", "id1", "id2", (6.0, 4.0, 12.0, 4.0)),
    ]
    .concat();
    let mut s = open_drawing(&dir, &crate::odg_build::Drawing::new().page(&shapes));
    let (a, b) = (id_of(s.document().unwrap(), 0, "A"), id_of(s.document().unwrap(), 0, "B"));
    run(&mut s, "shape.delete", json!({ "ids": [a, b] }));
    assert_eq!(crate::odg_file::save(&mut s)["edited"], true);
    run(&mut s, "edit.undo", json!({}));
    let said = refused(&mut s);
    assert!(said.contains("put back by an undo"), "{said}");
    assert!(said.contains("Redo the change you took back (edit.redo) to save") && !said.contains("Undo the change (edit.undo)"), "{said}");
    assert!(said.chars().count() <= 601, "{said}");
    // The way out works: the redo is the drawing the file holds.
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(crate::odg_file::save(&mut s)["edited"], false);
    // The other way out the words name: the shapes deleted again.
    run(&mut s, "edit.undo", json!({}));
    assert!(s.execute("file.save", &json!({})).is_err());
    run(&mut s, "shape.delete", json!({ "ids": [a, b] }));
    assert_eq!(crate::odg_file::save(&mut s)["edited"], false);
}

/// **Nothing warns at the time it is made any more** (warned of a curved connector while the save refused it; writes it as cubics): a
/// shape the writer writes (every master of the built-in stencils), a straight connector and a curved one on an `.odg`, and a drawing that is not an `.odg`, get no
/// warning, and the drawing saves.
#[test]
fn a_curved_connector_does_not_warn_when_it_is_made_and_is_saved() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let rect = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }));
    assert!(warnings(&rect).is_empty(), "a rectangle is written: {rect}");
    let triangle = run(&mut s, "shape.drop", json!({ "master": "triangle", "stencil": "basic", "page": 0, "x": 6.0, "y": 2.0 }));
    assert!(warnings(&triangle).is_empty(), "a triangle is written: {triangle}");
    let arrow = run(&mut s, "shape.drop", json!({ "master": "arrows.right", "stencil": "arrows", "page": 0, "x": 8.0, "y": 2.0 }));
    assert!(warnings(&arrow).is_empty(), "an arrow is written: {arrow}");
    let a = id_of(s.document().unwrap(), 0, "A");
    let b = id_of(s.document().unwrap(), 0, "B");
    let straight = run(&mut s, "connector.add", json!({ "from": { "id": a }, "to": { "id": b } }));
    assert!(warnings(&straight).is_empty(), "{straight}");
    let set = run(&mut s, "connector.style", json!({ "style": "curved" }));
    assert!(warnings(&set).is_empty(), "the style says nothing: {set}");
    let curved = run(&mut s, "connector.add", json!({ "from": { "id": a }, "to": { "id": b } }));
    assert!(warnings(&curved).is_empty(), "a curved connector is written as cubics: {curved}");
    let own = u32::try_from(straight["id"].as_u64().unwrap()).unwrap();
    let curve_one = run(&mut s, "connector.style", json!({ "style": "curved", "ids": [own] }));
    assert_eq!(curve_one["styled"], 1);
    assert!(warnings(&curve_one).is_empty(), "one connector made curved: {curve_one}");
    assert_eq!(save(&mut s)["edited"], true, "and the drawing saves");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// **The page's connector style is saved** (before it, the page's own cells were not written and the save was refused for them): `connector.style` with no `ids`
/// changes the page's own cells, which DiaCraft's document file carries. The answer says nothing of the page, for straight, right-angled and curved (which warned until
///); the drawing saves, reads back and saves again as no edit; a style on connectors by `ids`, a style that changes nothing and a drawing that is not an `.odg` say
/// nothing.
#[test]
fn setting_the_pages_connector_style_on_an_odg_is_saved_in_the_document_file() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let set = run(&mut s, "connector.style", json!({ "style": "straight" }));
    assert_eq!(set["default_changed"], true);
    assert!(warnings(&set).is_empty(), "the page's cells are saved: {set}");
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    assert!(dir.read("a.odg").windows(21).any(|w| w == b"DiaCraft/document.xml"), "the page's cells are in the document file");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    // Right-angled, from straight: the page's default changes again, and is saved.
    let back = run(&mut s, "connector.style", json!({ "style": "right_angle" }));
    assert!(warnings(&back).is_empty(), "{back}");
    assert_eq!(save(&mut s)["edited"], true);
    // Curved: no word (the writer writes a curved connector).
    let curved = run(&mut s, "connector.style", json!({ "style": "curved" }));
    assert!(warnings(&curved).is_empty(), "{curved}");
    // No change, no word; connectors by `ids` are not the page's.
    assert!(warnings(&run(&mut s, "connector.style", json!({ "style": "curved" }))).is_empty());
    let (a, b) = (id_of(s.document().unwrap(), 0, "A"), id_of(s.document().unwrap(), 0, "B"));
    let made = run(&mut s, "connector.add", json!({ "from": { "id": a }, "to": { "id": b } }));
    let own = run(&mut s, "connector.style", json!({ "style": "straight", "ids": [made["id"]] }));
    assert_eq!(own["default_changed"], false);
    assert!(warnings(&own).is_empty(), "{own}");
    // A drawing that is not an `.odg` writes the page's cells, and is told nothing.
    let mut blank = Session::new();
    run(&mut blank, "file.new", json!({}));
    let style = run(&mut blank, "connector.style", json!({ "style": "straight" }));
    assert!(warnings(&style).is_empty(), "{style}");
}

/// The masters a drop adds to the document are not the file's, and the copy leaves them out **whether or not a shape was flattened** (the review's mutant M3 kept
/// them on the path where nothing is to be flattened, and the writer refused a changed list of masters): a shape dropped, saved and deleted, then saved again
/// with nothing flattened and a master in the document.
#[test]
fn the_masters_a_drop_added_are_left_out_of_the_copy_when_nothing_is_flattened() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let made = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }));
    save(&mut s);
    run(&mut s, "shape.delete", json!({ "ids": [made["id"]] }));
    assert!(!s.document().unwrap().masters.is_empty(), "the master stays in the document when its instance is gone");
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "the delete is written: {saved}");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// The file the engine wrote is the file it holds, as it looked when written (the review's mutant M4: a save that kept no stamp let a change made on the disk
/// after the save go unnoticed, and the next save replaced it): after a save, the file is changed by something else, the drawing is edited, and the save is
/// refused as `ChangedOnDisk`, and made with `force`.
#[test]
fn a_file_changed_on_the_disk_after_a_save_is_not_replaced_without_force() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    crate::odg_file::shift(&mut s, "A", 0.5, 0.0);
    save(&mut s);
    // Something else writes the file: another drawing, of another size.
    let other = four().page("").bytes();
    assert_ne!(other.len(), dir.read("a.odg").len());
    dir.write("a.odg", &other);
    crate::odg_file::shift(&mut s, "A", 0.5, 0.0);
    match s.execute("file.save", &json!({})) {
        Err(EngineError::ChangedOnDisk { .. }) => {}
        other => panic!("the file that changed after the save was replaced, or refused another way: {other:?}"),
    }
    assert_eq!(dir.read("a.odg"), other, "nothing was written");
    let saved = run(&mut s, "file.save", json!({ "force": true }));
    assert_eq!(saved["edited"], true);
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
}

/// **An open puts the built-in stencils back** (the review's mutant M5: `open_odg` did not, so a drawing opened after a template had that template's stencils, in
/// its order): the Basic Flowchart template opens the flowchart stencil first, and an `.odg` opened after it has them as a drawing has them, Basic Shapes first.
#[test]
fn opening_an_odg_after_a_template_has_the_built_in_stencils_as_a_drawing_has_them() {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({ "template": "Basic Flowchart" }));
    let after_template: Vec<&str> = s.stencils().open_builtins().to_vec();
    let default: Vec<&str> = diacraft_engine::stencil::stencils().iter().map(|st| st.id).collect();
    assert_ne!(after_template, default, "the template opens them in an order of its own: {after_template:?}");
    let dir = Dir::new();
    let path = dir.write("a.odg", &four().bytes());
    run(&mut s, "file.open", json!({ "path": path }));
    assert_eq!(s.stencils().open_builtins(), default, "as a drawing has them");
}

/// `file.save` takes no parameter, so a VSDX drawing whose file is called `.odg` is refused as a failure with a way out (`file.save_as`), and not as a bad
/// parameter (`file.save_as` is the command with a name to be bad).
#[test]
fn a_vsdx_drawing_called_odg_is_refused_by_file_save_as_a_failure_with_a_way_out_and_by_save_as_as_a_bad_name() {
    use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape};
    let dir = Dir::new();
    let vsdx = VsdxBuilder::new().page(PageSpec::new(1, "First").shapes(&shape(5, r#"<Cell N="Width" V="2.50"/>"#))).build();
    let mut s = crate::odg_build::open_bytes_in(&dir, "v.odg", &vsdx);
    match s.execute("file.save", &json!({})) {
        Err(EngineError::Failed { msg, .. }) => assert!(msg.contains("use file.save_as") && msg.contains(".vsdx"), "{msg}"),
        other => panic!("file.save of a VSDX drawing called v.odg was not a failure: {other:?}"),
    }
    // `file.save_as` names a type: a VSDX drawing saved as an `.odg` is a save across the formats, written, or refused by what the drawing holds in words (this one has a
    // first page with no size), and a name that is no type is a bad parameter.
    match s.execute("file.save_as", &json!({ "path": dir.path("w.odg") })) {
        Err(EngineError::Failed { msg, .. }) => assert!(msg.contains("first page") && msg.contains("Nothing was written"), "{msg}"),
        other => panic!("file.save_as of a drawing with a first page of no size as an .odg was not refused in words: {other:?}"),
    }
    match s.execute("file.save_as", &json!({ "path": dir.path("w.txt") })) {
        Err(EngineError::BadParams { .. }) => {}
        other => panic!("file.save_as to a name of no type was not a bad parameter: {other:?}"),
    }
    assert_eq!(dir.files(), ["v.odg"], "nothing was written");
}

/// The type a drawing is saved as is the type it was read as or last saved as (what Save As offers and adds to a name typed with none), by what the package is and
/// not by what the file is called: an `.odg` called `a.dat`, a VSDX file called `v.odg`, a stencil, a drawing saved as a template, and a drawing that has no file.
#[test]
fn the_type_of_the_file_a_drawing_has_is_the_type_it_was_read_as_or_last_saved_as() {
    let dir = Dir::new();
    let mut s = Session::new();
    assert!(s.source().is_none());
    run(&mut s, "file.new", json!({}));
    assert_eq!(s.source().and_then(|f| f.extension()), None, "a new drawing has no file of a type");
    run(&mut s, "file.save_as", json!({ "path": dir.path("made.vssx") }));
    assert_eq!(s.source().and_then(|f| f.extension()), Some("vssx"), "saved as a stencil");
    run(&mut s, "file.save_as", json!({ "path": dir.path("again.vstx") }));
    assert_eq!(s.source().and_then(|f| f.extension()), Some("vstx"), "and then as a template");
    // Read by what the package is, whatever the file is called.
    let stencil = std::fs::read(dir.path("made.vssx")).unwrap();
    for (name, bytes, wanted) in [("v.odg", &stencil, "vssx"), ("a.dat", &four().bytes(), "odg"), ("b.vsdx", &four().bytes(), "odg")] {
        let mut other = Session::new();
        let path = dir.write(name, bytes);
        run(&mut other, "file.open", json!({ "path": path }));
        assert_eq!(other.source().and_then(|f| f.extension()), Some(wanted), "{name}");
    }
    // An `.odg` saved as an `.odg` is still one.
    let mut odg = open_drawing(&dir, &four());
    crate::odg_file::shift(&mut odg, "A", 0.5, 0.0);
    save(&mut odg);
    assert_eq!(odg.source().and_then(|f| f.extension()), Some("odg"));
}
