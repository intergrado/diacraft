//! Connectors and copies in the engine's `.odg` save: `connector.add` writes neither route cell the writer reads to tell
//! a connector from a line, and a copy of a shape the reader read carries what a new shape cannot.

use serde_json::{Value, json};

use crate::odg_build::{Dir, Drawing, content_of, id_of, open_drawing, read_back, rect_with_extras};
use crate::odg_file::{assert_reopens_as_saved, assert_saves_again_as_no_edit, four, save};
use crate::run;

/// The text of the first element called `tag` (from its `<` to its end tag), or the empty text.
fn element(content: &str, tag: &str) -> String {
    let start = content.find(&format!("<{tag} ")).unwrap_or(content.len());
    let end = content[start..].find(&format!("</{tag}>")).map_or(content.len(), |at| start + at + tag.len() + 3);
    content[start..end].to_owned()
}

/// The value of an attribute in `text`.
fn attribute(text: &str, name: &str) -> Option<String> {
    let at = text.find(&format!(" {name}=\""))? + name.len() + 3;
    Some(text[at..].split('"').next()?.to_owned())
}

/// The first parked item of the fix round's re-review: `connector.add` writes neither `ShapeRouteStyle` nor `ConLineRouteExt`, so the
/// writer took every connector of the engine's for a `draw:line` and refused it when glued. The copy the save hands the writer gives a connector the route
/// cells, and the model's connector (what `.vsdx` writes) is unchanged.
#[test]
fn connector_add_between_two_new_shapes_saves_as_a_glued_draw_connector_and_saves_again_as_no_edit() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let one = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 3.0, "y": 2.0 }));
    let two = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 6.0, "y": 2.0 }));
    run(&mut s, "connector.add", json!({ "from": { "id": one["id"] }, "to": { "id": two["id"] } }));
    let doc = s.document().unwrap().clone();
    let held = doc.pages.get(0).unwrap().shapes.iter().find(|x| x.sheet.cell("BeginX").is_some()).unwrap().clone();
    assert!(held.sheet.cell("ShapeRouteStyle").is_none() && held.sheet.cell("ConLineRouteExt").is_none(), "connector.add is unchanged for .vsdx");
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true);
    let content = content_of(&dir.read("a.odg"));
    assert!(!content.contains("<draw:line "), "no line: {content}");
    let connector = element(&content, "draw:connector");
    let (start, end) = (attribute(&connector, "draw:start-shape").unwrap(), attribute(&connector, "draw:end-shape").unwrap());
    assert_ne!(start, end);
    for id in [&start, &end] {
        assert!(
            content.contains(&format!("xml:id=\"{id}\"")) && content.contains(&format!("draw:id=\"{id}\"")),
            "{id} names a shape of the file: {content}"
        );
    }
    assert!(attribute(&connector, "draw:start-glue-point").is_some() && attribute(&connector, "draw:end-glue-point").is_some(), "{connector}");
    // It reads back as a connector glued to the two shapes, by the rows that glue a connector's begin and end.
    let (read, _) = read_back(&dir.read("a.odg"));
    let page = read.pages.get(0).unwrap();
    let glued: Vec<(String, String)> = page
        .connects
        .iter()
        .map(|c| {
            let to = page.shapes.iter().find(|x| x.id == c.to_sheet).and_then(|x| x.name_u.as_deref().map(str::to_owned)).unwrap_or_default();
            (c.from_cell.as_deref().unwrap_or("").to_owned(), to)
        })
        .collect();
    assert_eq!(glued.len(), 2, "{glued:?}");
    assert!(glued.iter().all(|(_, to)| to.starts_with("Rectangle.")) && glued[0].1 != glued[1].1, "{glued:?}");
    assert!(glued.iter().any(|(from, _)| from.starts_with("Begin")) && glued.iter().any(|(from, _)| from.starts_with("End")), "{glued:?}");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    // The arrowhead at its end is written: the connector's style names DiaCraft's marker for the number the connector resolves to, `styles.xml`
    // holds the marker, and nothing is said of a line end. Read back, the connector has the end.
    assert!(!saved["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|w| w.contains("a line end"))), "{saved}");
    let file = dir.read("a.odg");
    let content = content_of(&file);
    assert!(content.contains("draw:marker-end=\"DiaCraft_20_end_20_4\"") && content.contains("draw:marker-end-width="), "{content}");
    assert!(!content.contains("draw:marker-start"), "no begin: {content}");
    assert!(crate::odg_build::part_of(&file, "styles.xml").contains("<draw:marker draw:name=\"DiaCraft_20_end_20_4\""), "the marker is held");
    let shape = read.pages.get(0).unwrap().shapes.iter().find(|x| x.sheet.cell("BeginX").is_some()).unwrap();
    assert_eq!(shape.sheet.cell("EndArrow").and_then(|c| c.value.as_ref()?.number()), Some(4.0));
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// A connector from a shape of the file to a new one, in each style a connector draws in; and one with free ends. A right-angle or a straight one is a
/// `draw:connector` whose type is the style it has, and saves again as no edit. **A curved one is written as the cubics it is** (the engine draws it as a
/// quadratic B-spline, `NURBSTo` rows of degree 2, and the writer writes each piece raised to a cubic, `route.rs`; it was refused in words before), and its rows are carried.
#[test]
fn connectors_of_each_style_and_with_free_ends_save_as_draw_connectors_and_a_curved_one_is_written_as_cubics() {
    for (style, kind) in [("right_angle", None), ("straight", Some("line"))] {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &four());
        let a = id_of(s.document().unwrap(), 0, "A");
        let made = run(&mut s, "shape.drop", json!({ "master": "ellipse", "page": 0, "x": 4.0, "y": 2.0 }));
        let c = run(&mut s, "connector.add", json!({ "from": { "id": a }, "to": { "id": made["id"] } }));
        run(&mut s, "connector.style", json!({ "style": style, "ids": [c["id"]] }));
        save(&mut s);
        let connector = element(&content_of(&dir.read("a.odg")), "draw:connector");
        assert_eq!(attribute(&connector, "draw:type").as_deref(), kind, "{style}: {connector}");
        assert!(connector.contains("draw:start-shape=") && connector.contains("draw:end-shape="), "{style}: {connector}");
        assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    }
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let a = id_of(s.document().unwrap(), 0, "A");
    let made = run(&mut s, "shape.drop", json!({ "master": "ellipse", "page": 0, "x": 4.0, "y": 2.0 }));
    let c = run(&mut s, "connector.add", json!({ "from": { "id": a }, "to": { "id": made["id"] } }));
    run(&mut s, "connector.style", json!({ "style": "curved", "ids": [c["id"]] }));
    save(&mut s);
    let curved = element(&content_of(&dir.read("a.odg")), "draw:connector");
    let path = attribute(&curved, "svg:d").expect("the route is a path");
    assert!(path.contains('c'), "a curve is written as cubics: {path}");
    assert!(curved.contains("draw:start-shape=") && curved.contains("draw:end-shape="), "{curved}");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    // A connector between two places of the page glues to nothing, and is still a connector.
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    run(&mut s, "connector.add", json!({ "from": { "x": 1.0, "y": 1.0 }, "to": { "x": 3.0, "y": 1.5 } }));
    save(&mut s);
    let connector = element(&content_of(&dir.read("a.odg")), "draw:connector");
    assert!(!connector.is_empty() && !connector.contains("draw:start-shape="), "{connector}");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// The other parked item: a new shape copied from a shape the reader read carries attributes and elements of the file's own that a new
/// shape cannot, and the save says what it left out instead of dropping it in silence, and does not refuse (copying a shape is common). **No id of the
/// file's is copied** to the new shape.
#[test]
fn duplicating_a_read_shape_with_extras_saves_with_a_note_and_without_them_and_copies_no_id() {
    let dir = Dir::new();
    let extra = rect_with_extras("S", (2.0, 3.0, 4.0, 2.0)).replacen("<draw:custom-shape ", "<draw:custom-shape xml:id=\"id7\" draw:id=\"id7\" ", 1);
    let mut s = open_drawing(&dir, &Drawing::new().page(&extra));
    let original = s.document().unwrap().pages.get(0).unwrap().shapes.get(0).unwrap().clone();
    // What the reader keeps of the shape's own markup is in the `odf` slot, not in the VSDX `raw` and `passthrough`.
    let kept = |shape: &diacraft_doc::Shape| shape.ext.get().map(|d| d.odf.kept.clone()).unwrap_or_default();
    assert_eq!(kept(&original).raw.get("ext:note"), Some("kept"), "the reader keeps the attribute of the file's own");
    assert!(kept(&original).kept.iter().count() >= 2, "and the child element");
    run(&mut s, "shape.duplicate", json!({ "ids": [original.id] }));
    let copy = s.document().unwrap().pages.get(0).unwrap().shapes.get(1).unwrap().clone();
    assert_eq!(kept(&copy).raw.get("ext:note"), Some("kept"), "the copy holds what the original holds");
    assert!(kept(&copy).raw.get("xml:id").is_none() && kept(&copy).raw.get("draw:id").is_none(), "but no id: {:?}", kept(&copy).raw);
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true);
    let notes: Vec<&str> = saved["warnings"].as_array().unwrap().iter().filter_map(Value::as_str).filter(|w| w.contains("held")).collect();
    assert_eq!(notes.len(), 1, "{saved}");
    assert!(notes[0].contains("1 attribute") && notes[0].contains("1 element") && notes[0].contains("does not write"), "{}", notes[0]);
    let content = content_of(&dir.read("a.odg"));
    assert_eq!(content.matches("ext:note").count(), 1, "only the original has the attribute: {content}");
    assert_eq!(content.matches("<ext:data ").count(), 1, "and the element: {content}");
    assert_eq!(content.matches("id7").count(), 2, "and the id (as `xml:id` and `draw:id`, once each): {content}");
    assert_eq!(content.matches("<draw:custom-shape ").count(), 2);
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    // Said once: the same drawing saved again says nothing and is no edit.
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// A copy of a shape that holds nothing of the kind has no note: the attributes and the geometry the writer writes itself are not "left out".
#[test]
fn duplicating_a_plain_shape_says_nothing() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let a = id_of(s.document().unwrap(), 0, "A");
    run(&mut s, "shape.duplicate", json!({ "ids": [a] }));
    let saved = save(&mut s);
    assert_eq!(saved["warning_count"], 0, "{saved}");
}

/// A connector added in a later save, between a shape that an earlier save wrote as new and a shape of the file: the earlier shape is held flattened and
/// compared as saved, so both ends of the connector name shapes of the file, and the file saves again as no edit.
#[test]
fn a_connector_added_after_a_save_between_a_saved_new_shape_and_a_shape_of_the_file_is_glued_at_both_ends() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let new = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }));
    save(&mut s);
    let a = id_of(s.document().unwrap(), 0, "A");
    run(&mut s, "connector.add", json!({ "from": { "id": new["id"] }, "to": { "id": a } }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let content = content_of(&dir.read("a.odg"));
    let connector = element(&content, "draw:connector");
    let (start, end) = (attribute(&connector, "draw:start-shape").unwrap(), attribute(&connector, "draw:end-shape").unwrap());
    assert_ne!(start, end, "{connector}");
    for id in [&start, &end] {
        assert!(content.contains(&format!("draw:id=\"{id}\"")), "{id} names a shape of the file: {content}");
    }
    let (read, _) = read_back(&dir.read("a.odg"));
    assert_eq!(read.pages.get(0).unwrap().connects.len(), 2);
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}
