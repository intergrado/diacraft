//! The save note (design decision 8): an `.odg` save tells programs, in `notes`, when the drawing holds behaviour a LibreOffice save would drop
//! (cross-shape formulas, shape data; a master link is no trigger), and says nothing otherwise. It is not a warning: `warnings` is what a save did not write.

use diacraft_engine::Session;
use serde_json::{Value, json};

use crate::odg_build::{Dir, id_of, open_drawing, read_back};
use crate::odg_file::{assert_saves_again_as_no_edit, four, save};
use crate::run;

/// The notes of a save's answer.
fn notes(saved: &Value) -> &[Value] {
    saved["notes"].as_array().unwrap_or_else(|| panic!("a save of an .odg has `notes`: {saved}"))
}

/// Gives the shape `name` on the first page a section with one row named `row`, whose `Value` cell holds `formula`.
fn give(s: &mut Session, name: &str, section: &str, row: &str, formula: Option<&str>) {
    s.edit("Test edit", |doc| {
        let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let at = page.shapes.iter().position(|x| x.name_u.as_deref() == Some(name)).unwrap();
        let shape = std::sync::Arc::make_mut(page.shapes.get_mut(at).unwrap());
        let sheet = std::sync::Arc::make_mut(&mut shape.sheet);
        let mut made = diacraft_doc::Section::new(section, None);
        let mut r = diacraft_doc::Row { name: Some(std::sync::Arc::from(row)), ..diacraft_doc::Row::default() };
        let cell = match formula {
            Some(f) => diacraft_doc::Cell::with_formula(f, Some(diacraft_doc::Value::from_text("1"))),
            None => diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("1")),
        };
        r.cells.insert(diacraft_doc::CellName::new("Value"), cell);
        made.rows.push(std::sync::Arc::new(r));
        sheet.sections.push(std::sync::Arc::new(made));
        Ok(())
    })
    .unwrap();
}

/// A drawing of plain shapes, text, styles and connectors, edited in the ways that need nothing of DiaCraft's own markup (a text, a colour, a move, a connector,
/// a line style), with a shape of each built-in stencil dropped on it, holds nothing a LibreOffice save would drop: the save has no note, and a save of the file as it
/// is has none. (A dropped shape is linked to its master, which the file carries; that is no trigger, `note_is_absent_for_shapes_dropped...`.)
#[test]
fn note_absent_for_tier1_only_drawing() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let (a, b) = (id_of(s.document().unwrap(), 0, "A"), id_of(s.document().unwrap(), 0, "B"));
    // The Organization Chart Shapes hold Shape Data a LibreOffice save would drop: `orgchart_files.rs` has them.
    for (n, stencil) in diacraft_engine::stencil::stencils().iter().filter(|st| st.default_open && st.id != "orgchart").enumerate() {
        for (m, master) in stencil.masters().iter().enumerate() {
            run(&mut s, "shape.drop", json!({ "master": master.id, "stencil": stencil.id, "page": 0, "x": 1.0 + m as f64, "y": 4.0 + n as f64 }));
        }
    }
    run(&mut s, "shape.set_text", json!({ "id": a, "text": "hello" }));
    run(&mut s, "shape.format", json!({ "ids": [b], "fill": "#123456" }));
    run(&mut s, "shape.move", json!({ "ids": [b], "dx": 1.0, "dy": 0.5 }));
    run(&mut s, "connector.add", json!({ "from": { "id": a }, "to": { "id": b } }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true);
    assert_eq!(notes(&saved), &[] as &[Value], "no behaviour a LibreOffice save would drop: {saved}");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    let unedited = save(&mut s);
    assert_eq!((unedited["edited"].as_bool(), notes(&unedited).len()), (Some(false), 0), "{unedited}");
}

/// A shape dropped from a stencil is an instance of a master, which the file carries in DiaCraft's document file and a
/// LibreOffice save would drop, but it is **no trigger of the note**: a drop would show it in every session. One shape of each of the five built-in stencils saves with
/// no note, the file holds the five masters and the links all the same, and a session of its own that reads the file says nothing either; with a formula that reads
/// another shape beside them, the note counts that formula and has no field for the links.
#[test]
fn note_is_absent_for_shapes_dropped_from_the_stencils_and_for_the_file_that_holds_them() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    // The Organization Chart Shapes hold Shape Data (a position has its Name, Title and the rest), which a LibreOffice save would drop: they are a trigger of the note, and have a test of their own
    // (`orgchart_files.rs`).
    for (n, stencil) in diacraft_engine::stencil::stencils().iter().filter(|st| st.default_open && st.id != "orgchart").enumerate() {
        let master = stencil.masters().first().unwrap();
        run(&mut s, "shape.drop", json!({ "master": master.id, "stencil": stencil.id, "page": 0, "x": 1.0 + n as f64, "y": 5.0 }));
    }
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true);
    assert_eq!(notes(&saved), &[] as &[Value], "a master link is no trigger: {saved}");
    assert_eq!(read_back(&dir.read("a.odg")).0.masters.len(), 6, "the file carries the six masters all the same");
    assert_eq!(saved["warning_count"], 0, "{saved}");
    let again = save(&mut s);
    assert_eq!((again["edited"].as_bool(), notes(&again).len()), (Some(false), 0), "{again}");
    let b = id_of(s.document().unwrap(), 0, "B");
    give(&mut s, "A", "User", "Cost", Some(&format!("Sheet.{b}!Width*2")));
    let with = save(&mut s);
    let found = notes(&with);
    assert_eq!(found.len(), 1, "{with}");
    assert!(found[0].get("master_links").is_none(), "the note has no field for the links: {}", found[0]);
    assert!(!found[0]["words"].as_str().unwrap().contains("master"), "{}", found[0]);
    // A session of its own that reads the file says the same of it.
    let mut reopened = Session::new();
    run(&mut reopened, "file.open", json!({ "path": dir.path("a.odg") }));
    let read = save(&mut reopened);
    assert_eq!(notes(&read).len(), 1, "{read}");
    assert!(notes(&read)[0].get("master_links").is_none() && !notes(&read)[0]["words"].as_str().unwrap().contains("master"), "{read}");
}

/// A formula that reads another shape is behaviour a LibreOffice save would drop: the save has the note, counted, in words that are the probe's, and it is **not a
/// warning**; a save of the file as it was is no edit and has the note still (the drawing holds it), and taking the formula away takes the note away.
#[test]
fn note_present_with_cross_shape_formula() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let b = id_of(s.document().unwrap(), 0, "B");
    give(&mut s, "A", "User", "Cost", Some(&format!("Sheet.{b}!Width*2")));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true);
    let found = notes(&saved);
    assert_eq!(found.len(), 1, "{saved}");
    let note = &found[0];
    assert_eq!(note["class"], "libreoffice_would_drop");
    assert_eq!((note["cross_shape_formulas"].as_u64(), note["shape_data"].as_u64()), (Some(1), Some(1)), "{note}");
    let words = note["words"].as_str().unwrap();
    assert!(words.contains("1 shape with a formula that reads another shape") && words.contains("1 shape with data rows"), "{words}");
    assert!(
        words.contains("keeps the shapes, text, styles, pages and connectors, but drops DiaCraft data kept outside the shapes' styles, renumbers shape ids, glue points and style names, and recomputes connector routes"),
        "the probe's sentence: {words}"
    );
    assert!(words.contains("What is changed in LibreOffice does not update DiaCraft's data, and a shape pasted there copies it"), "{words}");
    assert_eq!(saved["warning_count"], 0, "the note is not a warning: {saved}");
    assert!(saved["warnings"].as_array().unwrap().is_empty(), "{saved}");
    // The file as it is: no edit, and the note is the drawing's, so it is still there.
    let again = save(&mut s);
    assert_eq!((again["edited"].as_bool(), notes(&again).len()), (Some(false), 1), "{again}");
    // Undone, the drawing holds none and the save says none.
    run(&mut s, "edit.undo", json!({}));
    let undone = save(&mut s);
    assert_eq!(notes(&undone), &[] as &[Value], "{undone}");
    // A file that holds the formula, opened again, gets the note with its first save, though that save is no edit.
    give(&mut s, "A", "User", "Cost", Some(&format!("Sheet.{b}!Width*2")));
    save(&mut s);
    let mut reopened = Session::new();
    run(&mut reopened, "file.open", json!({ "path": dir.path("a.odg") }));
    let third = save(&mut reopened);
    assert_eq!((third["edited"].as_bool(), notes(&third).len()), (Some(false), 1), "{third}");
}

/// A formula that reads another shape **by its name** (`B!Width`, `B.2!Width`) is behaviour a LibreOffice save would drop as much as `Sheet.2!Width` is (the review of
/// The note looked for `Sheet.<ID>` only), and one that names the shape it is in is not.
#[test]
fn note_counts_a_formula_that_reads_another_shape_by_its_name() {
    for (formula, cross) in [
        ("B!Width*2", true),
        ("IF(B!Width>1,1,2)", true),
        ("1+C.3!Height", true),
        ("A!Width", false),
        ("a.1!Width", false),
        ("ThePage!PageWidth", false),
        ("Width*2", false),
    ] {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &four());
        give(&mut s, "A", "User", "Cost", Some(formula));
        let saved = save(&mut s);
        let found = notes(&saved);
        let said = found.first().and_then(|n| n["cross_shape_formulas"].as_u64()).unwrap_or(0);
        assert_eq!(said, u64::from(cross), "{formula}: {saved}");
    }
}

/// Shape data (a `Property` row, or a `User` row that is not the route marker) is the second kind; each kind is counted by shape.
#[test]
fn note_counts_shape_data_and_cross_shape_formulas_by_shape() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let c = id_of(s.document().unwrap(), 0, "C");
    give(&mut s, "A", "Property", "Cost", None);
    give(&mut s, "B", "User", "Tag", None);
    give(&mut s, "D", "User", "Link", Some(&format!("Sheet.{c}!PinX")));
    let saved = save(&mut s);
    let found = notes(&saved);
    assert_eq!(found.len(), 1, "{saved}");
    assert_eq!((found[0]["cross_shape_formulas"].as_u64(), found[0]["shape_data"].as_u64()), (Some(1), Some(3)), "{}", found[0]);
    assert!(found[0]["words"].as_str().unwrap().contains("1 shape with a formula that reads another shape, 3 shapes with data rows"), "{}", found[0]);
}

/// A connector the engine makes holds the `User` row that marks its route: it is not shape data, and a drawing of connectors has no note.
#[test]
fn the_route_marker_of_a_connector_is_no_shape_data() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let (a, b, c) = (id_of(s.document().unwrap(), 0, "A"), id_of(s.document().unwrap(), 0, "B"), id_of(s.document().unwrap(), 0, "C"));
    run(&mut s, "connector.add", json!({ "from": { "id": a }, "to": { "id": b } }));
    run(&mut s, "connector.add", json!({ "from": { "id": b }, "to": { "id": c } }));
    let saved = save(&mut s);
    assert_eq!(notes(&saved), &[] as &[Value], "{saved}");
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path("a.odg") }));
    assert_eq!(notes(&save(&mut again)), &[] as &[Value], "and read back, the connectors' marker is still no data");
}

/// The shape data of a shape as the VSDX reader builds it, read from a drawing that holds `<Section N="Property">` (the model keeps shape data in a section called
/// `Property`: `calc` maps the formula prefix `Prop` to it): the review's finding that the save note and the loss audit matched `Prop`, which no model holds.
fn shape_data_as_the_vsdx_reader_builds_it(dir: &Dir) -> std::sync::Arc<diacraft_doc::Section> {
    use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape};
    let data = r#"<Section N="Property"><Row N="Cost"><Cell N="Value" V="3"/><Cell N="Label" V="Cost"/></Row></Section>"#;
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&shape(1, data))).build();
    let mut vsdx = Session::new();
    run(&mut vsdx, "file.open", json!({ "path": dir.write("data.vsdx", &bytes) }));
    let doc = vsdx.document().unwrap();
    let section =
        doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.sections.iter().find(|s| !s.rows.is_empty()).cloned().expect("the shape data");
    assert_eq!(section.name.as_str(), "Property", "the section the VSDX reader builds shape data in");
    section
}

/// Puts a section on the shape `name` of the first page (or on the shape `id` when `name` is empty).
fn put_section(s: &mut Session, name: &str, id: u32, section: &std::sync::Arc<diacraft_doc::Section>) {
    s.edit("Test setup: shape data", |doc| {
        let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let at = page.shapes.iter().position(|x| if name.is_empty() { x.id == id } else { x.name_u.as_deref() == Some(name) }).unwrap();
        let shape = std::sync::Arc::make_mut(page.shapes.get_mut(at).unwrap());
        std::sync::Arc::make_mut(&mut shape.sheet).sections.push(std::sync::Arc::clone(section));
        Ok(())
    })
    .unwrap();
}

/// The review (the spec checker's finding): shape data built as the VSDX reader builds it, in a `Property` section, is shape data to the save note, which matched
/// `Prop`; the same on the shape of a drawing that is saved again.
#[test]
fn note_counts_the_shape_data_a_vsdx_reader_builds() {
    let dir = Dir::new();
    let data = shape_data_as_the_vsdx_reader_builds_it(&dir);
    let mut s = open_drawing(&dir, &four());
    put_section(&mut s, "A", 0, &data);
    let saved = save(&mut s);
    let found = notes(&saved);
    assert_eq!(found.len(), 1, "{saved}");
    assert_eq!((found[0]["cross_shape_formulas"].as_u64(), found[0]["shape_data"].as_u64()), (Some(0), Some(1)), "{}", found[0]);
    assert!(found[0]["words"].as_str().unwrap().contains("1 shape with data rows"), "{}", found[0]);
}

/// The shape data a VSDX reader builds is carried by a new shape that holds it (a drop given the section), and the save says nothing of it (the writer carries a
/// `Property` or `User` section in DiaCraft's own data on the shape's style; the loss audit used to say it in the words of shape data).
#[test]
fn the_shape_data_a_vsdx_reader_builds_is_carried_by_a_new_shape_and_not_said() {
    let dir = Dir::new();
    let data = shape_data_as_the_vsdx_reader_builds_it(&dir);
    let mut s = open_drawing(&dir, &four());
    let made = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }));
    put_section(&mut s, "", u32::try_from(made["id"].as_u64().unwrap()).unwrap(), &data);
    let saved = save(&mut s);
    let said: Vec<&str> = saved["warnings"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(!said.iter().any(|w| w.starts_with("shape data") || w.starts_with("cells and sections")), "nothing of it is said: {said:?}");
}
