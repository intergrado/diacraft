//! The alt text of a drawing in an `.odg` file: `svg:title` and `svg:desc`, through the commands, the
//! files and the engine's own save, which splices into the file it opened. The writers and the reader are looked into in `crates/odg/tests/it/alt_text.rs`.

use serde_json::json;

use crate::alt_text_files::{first_page, ids, made, reopened, texts_at};
use crate::edit::drop_shape;
use crate::odg_build::{Dir, part_of};
use crate::{blank_session, run};

fn content_of(path: &str) -> String {
    part_of(&std::fs::read(path).unwrap(), "content.xml")
}

/// The save's own check read the file back and found no class that differs from the drawing (`file.save_as` says what it compared).
fn checked_clean(saved: &serde_json::Value) {
    let classes = saved["checked"]["classes"].as_array().unwrap_or_else(|| panic!("the save was checked: {saved}"));
    assert!(classes.is_empty(), "the file differs from the drawing: {}", saved["checked"]);
}

#[test]
fn a_new_drawings_title_and_description_are_in_the_file_before_the_shapes_text() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    run(&mut s, "shape.set_text", json!({"id": id, "text": "Note"}));
    run(&mut s, "shape.alt_text", json!({"id": id, "title": "Pump", "description": "Feeds <boiler> & tank"}));
    let dir = Dir::new();
    let path = dir.path("drawing.odg");
    checked_clean(&run(&mut s, "file.save_as", json!({ "path": path })));
    let content = content_of(&path);
    let title = content.find("<svg:title>Pump</svg:title>").expect("The title");
    let desc = content.find("<svg:desc>Feeds &lt;boiler&gt; &amp; tank</svg:desc>").expect("the description, escaped");
    let text = content.find("<text:p").expect("the text");
    assert!(title < desc && desc < text, "The title, the description, then the text: {content}");
    let mut back = reopened(&path);
    assert_eq!(texts_at(&mut back, 0), ("Pump".to_owned(), "Feeds <boiler> & tank".to_owned()));
}

#[test]
fn a_title_edited_in_another_program_is_the_title_here_and_a_save_keeps_the_markup() {
    let mut s = made();
    let dir = Dir::new();
    let path = dir.path("drawing.odg");
    checked_clean(&run(&mut s, "file.save_as", json!({ "path": path })));
    let before = content_of(&path);
    assert!(before.contains("<svg:title>Pump P-101</svg:title>"));
    // Another program changes the title and leaves the rest, DiaCraft's own data included.
    let edited = before.replace("<svg:title>Pump P-101</svg:title>", "<svg:title>Pump P-102</svg:title>");
    let parts: Vec<(String, Vec<u8>)> = {
        let bytes = std::fs::read(&path).unwrap();
        let package = diacraft_odg::package::Package::read(&bytes, &diacraft_odg::Limits::default()).unwrap();
        package
            .parts()
            .filter(|name| *name != "mimetype" && *name != "META-INF/manifest.xml")
            .map(|name| {
                let data = if name == "content.xml" { edited.clone().into_bytes() } else { package.part(name).unwrap().to_vec() };
                (name.to_owned(), data)
            })
            .collect()
    };
    let changed = dir.path("changed.odg");
    std::fs::write(&changed, crate::odg_build::package(crate::odg_build::DRAWING, parts)).unwrap();
    let mut open = reopened(&changed);
    assert_eq!(texts_at(&mut open, 0).0, "Pump P-102", "the file's title");
    // An edit of another shape is saved in the file, which keeps the title it holds.
    let all = ids(&open);
    let page = first_page(&open);
    run(&mut open, "shape.alt_text", json!({"id": all[3], "title": "New", "page": page}));
    run(&mut open, "file.save", json!({}));
    let after = content_of(&changed);
    assert!(after.contains("<svg:title>Pump P-102</svg:title>") && after.contains("<svg:title>New</svg:title>"), "{after}");
}

#[test]
fn a_title_cleared_in_an_opened_file_is_taken_out_of_the_markup() {
    let mut s = made();
    let dir = Dir::new();
    let path = dir.path("drawing.odg");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let mut open = reopened(&path);
    let all = ids(&open);
    let page = first_page(&open);
    run(&mut open, "shape.alt_text", json!({"id": all[0], "page": page, "title": ""}));
    run(&mut open, "file.save", json!({}));
    let content = content_of(&path);
    assert!(!content.contains("Pump P-101"), "No title is left: {content}");
    assert!(content.contains("<svg:desc>Feeds the boiler</svg:desc>"), "the description stays");
    let mut back = reopened(&path);
    assert_eq!(texts_at(&mut back, 0), (String::new(), "Feeds the boiler".to_owned()));
}

#[test]
fn an_instances_text_from_its_master_is_the_text_of_its_shape_in_the_file() {
    let mut s = crate::alt_text_files::with_master_and_instance();
    let dir = Dir::new();
    let path = dir.path("drawing.odg");
    checked_clean(&run(&mut s, "file.save_as", json!({ "path": path })));
    let content = content_of(&path);
    assert!(content.contains("<svg:title>Valve</svg:title><svg:desc>A gate valve</svg:desc>"), "{content}");
    let mut back = reopened(&path);
    assert_eq!(texts_at(&mut back, 0), ("Valve".to_owned(), "A gate valve".to_owned()));
    // The instance still holds none of the rows, and its master is still the master's.
    let page = first_page(&back);
    let shape_id = ids(&back)[0];
    let d = back.document().unwrap();
    let at = d.find_shape(diacraft_doc::Owner::Page(page), shape_id).unwrap();
    let shape = d.shape(&at).unwrap();
    assert!(shape.sheet.section(&diacraft_doc::SectionKey::new("User", None)).is_none(), "no copy of the master's rows");
    assert_eq!(shape.master, Some(5));
}

/// A drawing of one group of two boxes, and the group's id.
fn a_group() -> (crate::Session, u32) {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 3.0);
    let b = drop_shape(&mut s, "rectangle", 5.0, 3.0);
    let group = run(&mut s, "shape.group", json!({"ids": [a, b]}))["id"].as_u64().unwrap();
    (s, u32::try_from(group).unwrap())
}

#[test]
fn a_group_with_a_title_is_written_with_it_and_reads_it_back() {
    let dir = Dir::new();
    // What the save's check says of a group of two boxes is what it says of it with a title and a description: they add no difference.
    let (mut plain, _) = a_group();
    let said_of_plain = run(&mut plain, "file.save_as", json!({ "path": dir.path("plain.odg") }));
    let (mut s, group) = a_group();
    run(&mut s, "shape.alt_text", json!({"id": group, "title": "Pair", "description": "Two boxes"}));
    let path = dir.path("drawing.odg");
    let said = run(&mut s, "file.save_as", json!({ "path": path }));
    assert_eq!(said["checked"], said_of_plain["checked"], "a title adds no difference");
    assert!(content_of(&path).contains("<svg:title>Pair</svg:title>"));
    let mut back = reopened(&path);
    assert_eq!(texts_at(&mut back, 0), ("Pair".to_owned(), "Two boxes".to_owned()));
}

#[test]
fn many_titled_shapes_save_and_read_back_in_an_odg_file() {
    let mut s = blank_session();
    s.edit("test: many shapes", |d| {
        let at = d.pages.iter().position(|p| p.id == 1).unwrap();
        let page = std::sync::Arc::make_mut(d.pages.get_mut(at).unwrap());
        for i in 0..2_000u32 {
            let mut shape = diacraft_doc::Shape::new(100 + i);
            let sheet = std::sync::Arc::make_mut(&mut shape.sheet);
            for (name, value) in [("PinX", 1.0 + f64::from(i % 40) * 0.2), ("PinY", 1.0 + f64::from(i / 40) * 0.1), ("Width", 0.1), ("Height", 0.1)] {
                sheet.cells.insert(name.into(), diacraft_doc::Cell::constant(diacraft_doc::Value::from_number(value).unwrap()));
            }
            let mut user = diacraft_doc::Section::new("User", None);
            user.rows.push(std::sync::Arc::new(diacraft_doc::alt_text::standard_row("visAltText", &format!("Shape {i}"))));
            sheet.sections.push(std::sync::Arc::new(user));
            page.shapes.push(std::sync::Arc::new(shape));
        }
        Ok(())
    })
    .unwrap();
    let dir = Dir::new();
    let path = dir.path("drawing.odg");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let mut back = reopened(&path);
    assert_eq!(texts_at(&mut back, 0).0, "Shape 0");
    assert_eq!(texts_at(&mut back, 1_999).0, "Shape 1999");
    assert_eq!(content_of(&path).matches("<svg:title>").count(), 2_000);
}
