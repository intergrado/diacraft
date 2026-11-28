//! The text a cell holds says `STR`, and a text field has a picture.
//!
//! A program that reads the `U` of a cell takes a text with none for a number it cannot read:
//! an org chart showed `0 0` for every name and title, and a text field with an empty `Format` shows nothing, since an empty picture gives empty text
//! Every test here reads **the bytes a save wrote**, in both writers: Save As writes the model (`file.save_as`) and Save into the file splices (`file.save`).
//!
//! `problems` is the check: every cell of a text kind that says no `STR`, every `Value` of Shape Data, a user-defined cell or a field that holds a text with no unit, and every field row whose
//! `Format` is empty.

use std::collections::BTreeSet;

use diacraft_engine::Session;
use diacraft_opc::xml::{El, Node, parse};
use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape};
use serde_json::json;

use crate::edit::id_of;
use crate::native_common::Dir;
use crate::run;

/// The cells of a text kind in the data sections: a string whatever it reads as, so that each says `STR`.
fn is_text_cell(section: &str, name: &str) -> bool {
    matches!((section, name), ("Property", "Label" | "Prompt" | "Format" | "SortKey") | ("Field", "Format") | ("User", "Prompt"))
}

fn is_number(v: &str) -> bool {
    !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'+' | b'e' | b'E')) && v.parse::<f64>().is_ok()
}

fn walk(el: &El, section: &str, row: &str, part: &str, out: &mut Vec<String>) {
    let (mut section, mut row) = (section.to_owned(), row.to_owned());
    if el.is("Section") {
        section = el.attr("N").unwrap_or("").to_owned();
    }
    if el.is("Row") {
        row = el.attr("N").or_else(|| el.attr("IX")).unwrap_or("").to_owned();
    }
    if el.is("Cell") && matches!(section.as_str(), "Property" | "User" | "Field") {
        let (name, v, u) = (el.attr("N").unwrap_or(""), el.attr("V"), el.attr("U"));
        let at = format!("{part} {section}/{row}/{name} V={v:?} U={u:?} F={:?}", el.attr("F"));
        if is_text_cell(&section, name) && v.is_some() && u != Some("STR") {
            out.push(format!("a text cell with no STR: {at}"));
        }
        if name == "Value" && u.is_none() && v.is_some_and(|v| !v.is_empty() && !is_number(v) && !v.starts_with('#')) {
            out.push(format!("a text value with no unit: {at}"));
        }
        if section == "Field" && name == "Format" && v.is_none_or(|v| v.trim().is_empty()) {
            out.push(format!("a field with an empty Format: {at}"));
        }
    }
    for child in &el.children {
        if let Node::El(child) = child {
            walk(child, &section, &row, part, out);
        }
    }
}

/// What is wrong with the text cells of the pages and masters of a VSDX file.
fn problems(bytes: &[u8]) -> Vec<String> {
    let package = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).expect("a package");
    let mut out = Vec::new();
    let names: Vec<String> = package.parts().map(str::to_owned).collect();
    for name in names {
        if name.ends_with(".xml") && (name.contains("/pages/") || name.contains("/masters/")) {
            let root = parse(package.part(&name).expect("the part")).expect("xml");
            walk(&root, "", "", &name, &mut out);
        }
    }
    out
}

/// The text of a part whose name ends with `suffix`.
fn part(bytes: &[u8], suffix: &str) -> String {
    let package = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).expect("a package");
    let name = package.parts().find(|p| p.ends_with(suffix)).unwrap_or_else(|| panic!("no part ends with {suffix}")).to_owned();
    String::from_utf8(package.part(&name).expect("the part").to_vec()).expect("text")
}

fn save_as(s: &mut Session, dir: &Dir, name: &str) -> Vec<u8> {
    let path = dir.path(name);
    run(s, "file.save_as", json!({ "path": path }));
    std::fs::read(&path).expect("the file")
}

fn opened(path: &str) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

fn first_page(s: &Session) -> u32 {
    s.document().unwrap().pages.iter().find(|p| !p.background).unwrap().id
}

#[track_caller]
fn assert_clean(what: &str, bytes: &[u8]) {
    let found = problems(bytes);
    assert!(found.is_empty(), "{what}:\n{}", found.join("\n"));
}

/// The org chart template with three positions dropped and their names and titles filled in.
fn org_chart() -> (Session, Vec<u32>) {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({"template": "organization_chart"}));
    let mut ids = Vec::new();
    for (i, (master, name, title)) in
        [("executive", "Ann Lee", "Chief Executive"), ("manager", "Bo Chan", "Head of Sales"), ("position", "Cy Dunn", "Analyst")]
            .into_iter()
            .enumerate()
    {
        let id = id_of(&run(
            &mut s,
            "shape.drop",
            json!({"master": format!("orgchart.{master}"), "page": 1, "x": 2.0 + 2.5 * i as f64, "y": 6.0 - i as f64}),
        ));
        run(&mut s, "shapedata.set", json!({"id": id, "row": "Name", "value": name}));
        run(&mut s, "shapedata.set", json!({"id": id, "row": "Title", "value": title}));
        ids.push(id);
    }
    (s, ids)
}

#[test]
fn an_org_chart_says_str_on_every_text_cell_in_the_model_writer() {
    let dir = Dir::new();
    let (mut s, _) = org_chart();
    let bytes = save_as(&mut s, &dir, "org.vsdx");
    assert_clean("Save As of the org chart", &bytes);
    // The cells of the instances that inherit their formula, which showed `0 0`: the cached text says `STR` itself.
    let page = part(&bytes, "/pages/page1.xml");
    for shown in ["Ann Lee", "Chief Executive", "Bo Chan", "Head of Sales", "Cy Dunn", "Analyst"] {
        assert!(page.contains(&format!(r#"<Cell N="Value" V="{shown}" U="STR" F="Inh"/>"#)), "{shown} in a field row of the page:\n{page}");
        assert!(page.contains(&format!(r#"<Cell N="Value" V="{shown}" U="STR"/>"#)), "{shown} in a Shape Data row of the page:\n{page}");
    }
    // The master's field rows have a picture for a text, and say it is a string.
    let master = part(&bytes, "/masters/master1.xml");
    assert_eq!(master.matches(r#"<Cell N="Format" V="@" U="STR"/>"#).count(), 2, "the name's field and the title's:\n{master}");
    // The Shape Data rows say what each cell is: a label, a prompt and a sort key are strings, and a row with no picture holds the empty string.
    assert!(master.contains(r#"<Cell N="Label" V="Name" U="STR"/>"#) && master.contains(r#"<Cell N="SortKey" V="0001" U="STR"/>"#), "{master}");
    assert!(master.contains(r#"<Cell N="Format" V="" U="STR"/>"#), "{master}");
}

#[test]
fn an_org_chart_edited_in_its_file_says_str_in_the_save_into_the_file() {
    let dir = Dir::new();
    let (mut s, ids) = org_chart();
    save_as(&mut s, &dir, "org.vsdx");
    // The file is opened, a name is changed, a shape is dropped and named, and the file is saved into: the splice writes the cells the edit made.
    let mut back = opened(&dir.path("org.vsdx"));
    run(&mut back, "shapedata.set", json!({"id": ids[1], "row": "Name", "value": "Bo Chang"}));
    run(&mut back, "shapedata.set", json!({"id": ids[2], "row": "E_mail", "value": "cy@example.org"}));
    let fresh = id_of(&run(&mut back, "shape.drop", json!({"master": "orgchart.assistant", "page": 1, "x": 9.0, "y": 3.0})));
    run(&mut back, "shapedata.set", json!({"id": fresh, "row": "Name", "value": "Di Eng"}));
    run(&mut back, "shapedata.set", json!({"id": fresh, "row": "Title", "value": "Assistant to the Head"}));
    run(&mut back, "file.save", json!({}));
    let bytes = std::fs::read(dir.path("org.vsdx")).unwrap();
    assert_clean("a save into the org chart file", &bytes);
    let page = part(&bytes, "/pages/page1.xml");
    for shown in ["Bo Chang", "Di Eng", "Assistant to the Head"] {
        assert!(page.contains(&format!(r#"V="{shown}" U="STR""#)), "{shown} says STR in the page the splice wrote:\n{page}");
    }
    assert!(page.contains(r#"V="cy@example.org" U="STR""#), "{page}");
    // What the file says reads back as what the session shows.
    let page_id = first_page(&back);
    for (id, name) in [(ids[1], "Bo Chang"), (fresh, "Di Eng")] {
        let data = run(&mut opened(&dir.path("org.vsdx")), "shapedata.list", json!({"id": id, "page": page_id}));
        let value = data["fields"].as_array().unwrap().iter().find(|f| f["row"] == "Name").map(|f| f["value"].clone());
        assert_eq!(value, Some(json!(name)), "{data}");
    }
}

#[test]
fn an_unedited_save_is_the_file_it_read_and_a_files_own_cells_keep_what_they_had() {
    // A file as another program may write it: text cells with no unit, an inherited one, a field with an empty `Format`. None is touched by an edit elsewhere.
    let own = r#"<Section N="Property"><Row N="Name"><Cell N="Value" V="Ann"/><Cell N="Label" V="Name"/><Cell N="Prompt" V=""/><Cell N="Type" V="0"/><Cell N="Format" V=""/></Row></Section>
<Section N="User"><Row N="Note"><Cell N="Value" V="kept" F="Inh"/></Row></Section>
<Section N="Field"><Row IX="0"><Cell N="Value" V="Ann" F="Prop.Name"/><Cell N="Format" V=""/><Cell N="Type" V="0"/></Row></Section>"#;
    let cells = r#"<Cell N="PinX" V="2" U="IN"/><Cell N="PinY" V="2" U="IN"/><Cell N="Width" V="1" U="IN"/><Cell N="Height" V="1" U="IN"/>"#;
    let page =
        PageSpec::new(1, "Page-1").shapes(&format!("{}{}", shape(1, &format!("{cells}{own}")), shape(2, &cells.replace("V=\"2\"", "V=\"5\""))));
    let bytes = VsdxBuilder::new().page(page).build();
    let dir = Dir::new();
    let path = dir.write("theirs.vsdx", &bytes);
    let mut s = opened(&path);
    let before = part(&bytes, "/pages/page1.xml");
    let block = |text: &str| text[text.find("<Section N=\"Property\"").unwrap()..text.find("</Shape>").unwrap()].to_owned();
    // A save with no edit is the file as it is: the page the other program wrote, byte for byte (the properties name the writer, which is all a save of the file as it is changes).
    let unedited = dir.write("unedited.vsdx", &bytes);
    run(&mut opened(&unedited), "file.save", json!({}));
    assert_eq!(part(&std::fs::read(&unedited).unwrap(), "/pages/page1.xml"), before, "an unedited save writes the page it read");
    // An edit elsewhere (the other shape moves) and a save into the file: the cells it did not touch are as they were, byte for byte.
    run(&mut s, "shape.move", json!({"ids": [2], "dx": 1.0, "dy": 0.5}));
    run(&mut s, "file.save", json!({}));
    let saved = std::fs::read(&path).unwrap();
    assert_eq!(block(&part(&saved, "/pages/page1.xml")), block(&before), "a file's own cells keep what they had");
    // The save named DiaCraft as the writer of the file, so a save of it with no edit gives the text of its `User` and `Field` rows the unit they lack and
    // changes no other cell: the `Property` row's cells are as the other program wrote them.
    let mut again = opened(&path);
    run(&mut again, "file.save", json!({}));
    let resaved = part(&std::fs::read(&path).unwrap(), "/pages/page1.xml");
    let property = |text: &str| text[text.find("<Section N=\"Property\"").unwrap()..text.find("</Section>").unwrap()].to_owned();
    assert_eq!(property(&resaved), property(&before), "the row of Shape Data keeps what it had");
    assert!(resaved.contains("V=\"kept\"") && resaved.matches("U=\"STR\"").count() == 2, "the user row and the field row say STR now:\n{resaved}");
    // The cell that is edited says what it holds.
    let mut edit = opened(&path);
    run(&mut edit, "shapedata.set", json!({"id": 1, "row": "Name", "value": "Bea"}));
    run(&mut edit, "file.save", json!({}));
    let edited = part(&std::fs::read(&path).unwrap(), "/pages/page1.xml");
    assert!(edited.contains(r#"<Cell N="Value" V="Bea" U="STR"/>"#), "{edited}");
    assert!(edited.contains(r#"<Cell N="Label" V="Name"/>"#), "the label nobody edited is as the file had it:\n{edited}");
}

/// A drawing with one box, every kind of field inserted into it. → the session, the box.
fn fields_everywhere() -> (Session, u32) {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({}));
    let id = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 4.0, "y": 5.0})));
    run(
        &mut s,
        "shapedata.define",
        json!({"id": id, "fields": [
            {"name": "Owner", "label": "Owner", "type": "string", "value": "Pat"},
            {"name": "Size", "label": "Size", "type": "number", "value": "12.5"},
            {"name": "Level", "label": "Level", "type": "fixed_list", "format": "Low;High", "value": "High"},
            {"name": "Due", "label": "Due", "type": "date", "value": "2026-03-04"},
        ]}),
    );
    (s, id)
}

/// Every field there is to insert: (the params of `text.insert_field`, the picture it is made with).
fn every_field() -> Vec<(serde_json::Value, &'static str)> {
    vec![
        (json!({"category": "date_time", "field": "current"}), "{<211>}"),
        (json!({"category": "date_time", "field": "creation"}), "{<211>}"),
        (json!({"category": "document", "field": "creator"}), "@"),
        (json!({"category": "document", "field": "title"}), "@"),
        (json!({"category": "geometry", "field": "width"}), "esc(7)"),
        (json!({"category": "geometry", "field": "angle"}), "esc(12)"),
        (json!({"category": "object", "field": "name"}), "@"),
        (json!({"category": "object", "field": "id"}), "esc(0)"),
        (json!({"category": "page", "field": "name"}), "@"),
        (json!({"category": "page", "field": "page_number"}), "esc(0)"),
        (json!({"category": "page", "field": "page_count"}), "esc(0)"),
        (json!({"category": "shape_data", "field": "Owner"}), "@"),
        (json!({"category": "shape_data", "field": "Level"}), "@"),
        (json!({"category": "shape_data", "field": "Size"}), "esc(0)"),
        (json!({"category": "shape_data", "field": "Due"}), "{<211>}"),
        (json!({"category": "custom", "formula": "\"plain\""}), "@"),
        (json!({"category": "custom", "formula": "Width*2"}), "esc(0)"),
        (json!({"category": "custom", "formula": "NOW()"}), "{<211>}"),
        (json!({"category": "user_cell", "field": "Note"}), "@"),
    ]
}

fn insert_every_field(s: &mut Session, id: u32) {
    for said in every_field_inserted(s, id) {
        assert!(!said["format"].as_str().unwrap_or_default().is_empty(), "no field is made with an empty format: {said}");
    }
}

/// Inserts every field of [`every_field`]. → what each insertion said: its params as `field` and the `format` it was made with.
fn every_field_inserted(s: &mut Session, id: u32) -> Vec<serde_json::Value> {
    // A user cell of text for the last field to show.
    run(s, "sheet.add_section", json!({"id": id, "section": "User-defined Cells"}));
    run(s, "sheet.add_row", json!({"id": id, "section": "User", "name": "Note"}));
    run(s, "sheet.set_cell", json!({"id": id, "cell": "User.Note", "formula": "\"remember\""}));
    let mut done = Vec::new();
    for (params, _) in every_field() {
        let mut p = params.clone();
        p["id"] = json!(id);
        let said = s.execute("text.insert_field", &p).unwrap_or_else(|e| panic!("{params}: {e}"));
        done.push(json!({"field": params, "format": said["format"]}));
    }
    done
}

/// The `Format` cells of the `Field` section of the first shape that has one, in row order: (the picture, the unit).
fn field_formats(page_xml: &str) -> Vec<(String, Option<String>)> {
    let root = parse(page_xml.as_bytes()).expect("xml");
    fn find<'a>(el: &'a El, out: &mut Vec<&'a El>) {
        if el.is("Section") && el.attr("N") == Some("Field") {
            out.push(el);
        }
        for c in &el.children {
            if let Node::El(child) = c {
                find(child, out);
            }
        }
    }
    let mut sections = Vec::new();
    find(&root, &mut sections);
    let section = sections.first().expect("a Field section");
    section
        .elements()
        .filter(|r| r.is("Row"))
        .filter_map(|r| r.elements().find(|c| c.attr("N") == Some("Format")))
        .map(|c| (c.attr("V").unwrap_or("").to_owned(), c.attr("U").map(str::to_owned)))
        .collect()
}

#[test]
fn a_field_of_each_type_is_written_with_its_picture_and_says_str_in_the_model_writer() {
    let dir = Dir::new();
    let (mut s, id) = fields_everywhere();
    insert_every_field(&mut s, id);
    let bytes = save_as(&mut s, &dir, "fields.vsdx");
    assert_clean("Save As of a shape with a field of every type", &bytes);
    let got = field_formats(&part(&bytes, "/pages/page1.xml"));
    let want: Vec<(String, Option<String>)> = every_field().iter().map(|(_, picture)| ((*picture).to_owned(), Some("STR".to_owned()))).collect();
    assert_eq!(got, want, "the picture and the unit of each field row, in the order they were inserted");
}

#[test]
fn and_in_the_save_into_the_file() {
    let dir = Dir::new();
    let (mut s, id) = fields_everywhere();
    save_as(&mut s, &dir, "into.vsdx");
    // The fields are put in after the file is opened, so that the splice writes the new rows.
    let mut back = opened(&dir.path("into.vsdx"));
    insert_every_field(&mut back, id);
    run(&mut back, "file.save", json!({}));
    let bytes = std::fs::read(dir.path("into.vsdx")).unwrap();
    assert_clean("a save into a file that took fields of every type", &bytes);
    let got = field_formats(&part(&bytes, "/pages/page1.xml"));
    let want: Vec<(String, Option<String>)> = every_field().iter().map(|(_, picture)| ((*picture).to_owned(), Some("STR".to_owned()))).collect();
    assert_eq!(got, want);
}

#[test]
fn a_blank_format_is_not_a_format_and_a_given_one_is_kept() {
    let (mut s, id) = fields_everywhere();
    let blank = run(&mut s, "text.insert_field", json!({"id": id, "category": "document", "field": "creator", "format": "  "}));
    assert_eq!(blank["format"], "@", "a blank format is none given: {blank}");
    let given = run(&mut s, "text.insert_field", json!({"id": id, "category": "document", "field": "title", "format": "@+"}));
    assert_eq!(given["format"], "@+", "{given}");
    let choices = run(&mut s, "text.field_choices", json!({"id": id}));
    let listed: Vec<(String, String)> = choices["categories"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|c| c["fields"].as_array().unwrap().iter())
        .map(|f| (f["id"].as_str().unwrap().to_owned(), f["format"].as_str().unwrap().to_owned()))
        .collect();
    assert!(listed.iter().all(|(_, format)| !format.is_empty()), "the dialog lists no field with no picture: {listed:?}");
}

/// The cells of the stencils' masters and the shapes dropped from them: every master of the stencil `id` dropped on a new page, and saved.
fn dropped_from(stencil: &str, masters: &[&str]) -> Vec<u8> {
    let dir = Dir::new();
    let mut s = Session::new();
    run(&mut s, "file.new", json!({}));
    let _ = s.execute("stencil.open", &json!({"id": stencil}));
    for (i, master) in masters.iter().enumerate() {
        let (x, y) = (1.5 + (i % 5) as f64 * 1.8, 8.0 - (i / 5) as f64 * 1.6);
        run(&mut s, "shape.drop", json!({"master": master, "page": 1, "x": x, "y": y}));
    }
    save_as(&mut s, &dir, "stencil.vsdx")
}

#[test]
fn the_network_c4_and_uml_stencils_label_their_shapes_in_text_cells_that_say_str() {
    for (stencil, masters, label) in [
        ("network", vec!["network.router", "network.switch", "network.firewall", "network.cloud"], "Manufacturer"),
        ("c4", vec!["c4.person", "c4.container", "c4.component", "c4.boundary"], "Description"),
        ("uml-class", vec!["uml-class.class", "uml-class.interface", "uml-class.enumeration", "uml-class.package", "uml-class.object"], "Abstract"),
    ] {
        let bytes = dropped_from(stencil, &masters);
        assert_clean(&format!("the {stencil} stencil's masters"), &bytes);
        let first = part(&bytes, "/masters/master1.xml");
        assert!(first.contains(&format!(r#"<Cell N="Label" V="{label}" U="STR"/>"#)), "{stencil}: a label says it is a string:\n{first}");
        // The masters that show a text through a field give it a picture, which says it is a string too.
        if stencil != "uml-class" {
            assert!(first.contains(r#"<Cell N="Format" V="@" U="STR"/>"#), "{stencil}: {first}");
        }
    }
}

#[test]
fn every_master_of_every_shipped_stencil_says_str_on_its_text_cells() {
    let mut seen = BTreeSet::new();
    for stencil in diacraft_engine::stencil::stencils() {
        let masters: Vec<&str> = stencil.masters().iter().filter(|m| !m.is_hidden()).map(|m| m.id).collect();
        let dir = Dir::new();
        let mut s = Session::new();
        run(&mut s, "file.new", json!({}));
        let _ = s.execute("stencil.open", &json!({"id": stencil.id}));
        for (i, master) in masters.iter().enumerate() {
            let (x, y) = (1.0 + (i % 6) as f64 * 1.4, 9.0 - (i / 6 % 8) as f64 * 1.1);
            // A part of a structure (a swimlane) is refused alone, and the master is still in the file when the drop copied it in first.
            let _ = s.execute("shape.drop", &json!({"master": master, "page": 1, "x": x, "y": y}));
        }
        let bytes = save_as(&mut s, &dir, "all.vsdx");
        let found = problems(&bytes);
        assert!(found.is_empty(), "the {} stencil:\n{}", stencil.id, found.join("\n"));
        seen.insert(stencil.id);
    }
    assert!(seen.len() >= 8, "the sweep looked at the stencils there are: {seen:?}");
}

/// The cells of a text kind by the catalogue (every section, not only the data ones) that hold a value and say no `STR`.
fn text_kind_problems(bytes: &[u8]) -> Vec<String> {
    fn go(el: &El, section: Option<&str>, row: &str, part: &str, out: &mut Vec<String>) {
        let mut section = section;
        let mut row = row.to_owned();
        if el.is("Section") {
            section = el.attr("N");
        }
        if el.is("Row") {
            row = el.attr("N").or_else(|| el.attr("IX")).unwrap_or("").to_owned();
        }
        if el.is("Cell") && !part.ends_with("document.xml") {
            let name = el.attr("N").unwrap_or("");
            let kind = diacraft_doc::cell_info(section, name).kind;
            if kind == diacraft_doc::ValueKind::Text && el.attr("V").is_some() && el.attr("U") != Some("STR") {
                out.push(format!("{part} {section:?}/{row}/{name} V={:?} U={:?} F={:?}", el.attr("V"), el.attr("U"), el.attr("F")));
            }
        }
        for child in &el.children {
            if let Node::El(child) = child {
                go(child, section, &row, part, out);
            }
        }
    }
    let package = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).expect("a package");
    let mut out = Vec::new();
    let names: Vec<String> = package.parts().map(str::to_owned).collect();
    for name in names {
        if name.ends_with(".xml") && (name.contains("/pages/") || name.contains("/masters/")) {
            go(&parse(package.part(&name).expect("the part")).expect("xml"), None, "", &name, &mut out);
        }
    }
    out
}

#[test]
fn a_drawing_made_with_the_commands_that_write_text_cells_says_str_on_each() {
    let dir = Dir::new();
    let mut s = Session::new();
    run(&mut s, "file.new", json!({}));
    let a = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 5.0})));
    let b = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 6.0, "y": 5.0})));
    run(
        &mut s,
        "hyperlink.set",
        json!({"id": a, "links": [{"description": "Spec", "address": "https://example.org/spec", "sub_address": "top", "extra_info": "x", "frame": "_top"}]}),
    );
    run(&mut s, "layer.add", json!({"name": "Notes"}));
    run(&mut s, "shape.alt_text", json!({"id": b, "title": "Box B", "description": "The second box"}));
    run(
        &mut s,
        "shapedata.define",
        json!({"id": b, "fields": [{"name": "Cost", "label": "Cost", "type": "number", "prompt": "In euros", "sort_key": "a", "value": "3"}]}),
    );
    run(&mut s, "text.format", json!({"ids": [a], "bullet": 1}));
    run(&mut s, "container.add", json!({"ids": [a, b]}));
    run(&mut s, "callout.insert", json!({"ids": [a]}));
    run(&mut s, "swimlane.create", json!({}));
    let bytes = save_as(&mut s, &dir, "commands.vsdx");
    let found = text_kind_problems(&bytes);
    assert!(found.is_empty(), "Save As:\n{}", found.join("\n"));
    assert_clean("Save As", &bytes);
    // The check is not empty: the cells it looks at are there, and say what they are.
    let page = part(&bytes, "/pages/page1.xml");
    for cell in [
        r#"<Cell N="Address" V="https://example.org/spec" U="STR""#,
        r#"<Cell N="Description" V="Spec" U="STR""#,
        r#"<Cell N="Prompt" V="In euros" U="STR""#,
        r#"<Cell N="SortKey" V="a" U="STR""#,
        r#"<Cell N="Label" V="Cost" U="STR""#,
    ] {
        assert!(page.contains(cell), "{cell} in the page:\n{page}");
    }
    // A layer's name is in the page's own sheet, which the pages part holds.
    let pages = part(&bytes, "/pages/pages.xml");
    assert!(pages.contains(r#"<Cell N="Name" V="Notes" U="STR""#), "{pages}");
    // And the same commands after the file is opened: the splice writes them.
    let mut back = Session::new();
    run(&mut back, "file.open", json!({"path": dir.path("commands.vsdx")}));
    let c = id_of(&run(&mut back, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 9.0, "y": 7.0})));
    run(&mut back, "hyperlink.set", json!({"id": c, "links": [{"description": "Home", "address": "https://example.org"}]}));
    run(
        &mut back,
        "shapedata.define",
        json!({"id": c, "fields": [{"name": "Owner", "label": "Owner", "type": "string", "prompt": "Who", "value": "Pat"}]}),
    );
    run(&mut back, "layer.add", json!({"name": "More"}));
    run(&mut back, "file.save", json!({}));
    let saved = std::fs::read(dir.path("commands.vsdx")).unwrap();
    let found = text_kind_problems(&saved);
    assert!(found.is_empty(), "a save into the file:\n{}", found.join("\n"));
    assert_clean("a save into the file", &saved);
}

/// Not a check: writes the files the LibreOffice comparison opens (`FIX_LO_DIR` names the folder), so that what LibreOffice Draw shows of their text can be put beside what an earlier build's
/// files showed: `cargo test -p diacraft-engine --test it write_the_files_for_libreoffice -- --ignored` with the variable set.
#[test]
#[ignore = "writes files for a black-box comparison; set FIX_LO_DIR"]
fn write_the_files_for_libreoffice() {
    let Ok(out) = std::env::var("FIX_LO_DIR") else { return };
    let dir = std::path::PathBuf::from(out);
    std::fs::create_dir_all(&dir).unwrap();
    let keep = |name: &str, bytes: &[u8]| std::fs::write(dir.join(name), bytes).unwrap();
    let scratch = Dir::new();
    let (mut org, _) = org_chart();
    keep("org.vsdx", &save_as(&mut org, &scratch, "org.vsdx"));
    let (mut fields, id) = fields_everywhere();
    every_field_inserted(&mut fields, id);
    keep("fields.vsdx", &save_as(&mut fields, &scratch, "fields.vsdx"));
    keep("network.vsdx", &dropped_from("network", &["network.router", "network.switch", "network.firewall", "network.cloud"]));
    keep("c4.vsdx", &dropped_from("c4", &["c4.person", "c4.container", "c4.component", "c4.boundary"]));
    keep(
        "uml.vsdx",
        &dropped_from("uml-class", &["uml-class.class", "uml-class.interface", "uml-class.enumeration", "uml-class.package", "uml-class.object"]),
    );
}
