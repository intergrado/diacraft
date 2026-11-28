//! Data given to a group of a flat `.fodg` drawing (a User-defined Cell, Shape Data, a Connection Points row of its own, alt text) was **dropped without a word**. A package keeps a
//! group's data in DiaCraft's document file (`group` entries, found by the group's `draw:name`); a flat file has no document file, and the save compared the group's data with nothing and
//! wrote none of it, so `file.save` answered `edited: false`, left the file as it was, and marked the session clean, with the data held in the session alone. A group with a name was dropped as
//! one with none (the package saves the named one and refuses the unnamed one in words). A flat drawing now refuses it in words, as the specification asks of what a flat file cannot keep (the
//! change of what a package keeps in its document file is refused, never dropped), and so it does for a new group, whose data the first save dropped while it said it was an edit.
//! The drawings are written by hand from the standard (`odg_build.rs`); the data is given by the commands.

use diacraft_engine::{EngineError, Session};
use serde_json::json;

use crate::odg_build::{Dir, Drawing, read_back, rect};
use crate::run;

/// What each of the commands gives a group: a `User` section, Shape Data, one more Connection Points row, and a title.
const KINDS: [&str; 4] = ["user", "data", "glue_row", "alt"];

/// A page of a rectangle and a group of two rectangles, the group with `name` (an attribute, or none) and `inside` before its members (a title and a description, or nothing).
fn drawing(name: &str, inside: &str) -> Drawing {
    let [a, b, c] = [("A", 1.0), ("B", 4.0), ("C", 8.0)].map(|(name, x)| rect(name, (x, 1.0, 2.0, 1.0), ""));
    let attr = if name.is_empty() { String::new() } else { format!(" draw:name=\"{name}\"") };
    Drawing::new().page(&format!("{c}<draw:g{attr}>{inside}{a}{b}</draw:g>"))
}

/// The title and the description a group's markup gives (`svg:title` and `svg:desc`: the rows of its alt text).
const TITLED: &str = "<svg:title>Pump group</svg:title><svg:desc>Two boxes</svg:desc>";

/// A page of a rectangle and a group with its own style and a glue point of its own (what the markup gives a group, which needs no entry): the fuzz target's seed.
fn glued() -> Drawing {
    Drawing::new().page(concat!(
        r#"<draw:rect draw:style-name="gr1" draw:name="R" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2cm"/>"#,
        r#"<draw:g draw:style-name="gr1"><draw:glue-point draw:id="4" svg:x="1cm" svg:y="1cm"/>"#,
        r#"<draw:rect draw:style-name="gr1" svg:x="1cm" svg:y="5cm" svg:width="2cm" svg:height="2cm"/>"#,
        r#"<draw:rect draw:style-name="gr1" svg:x="4cm" svg:y="5cm" svg:width="2cm" svg:height="2cm"/></draw:g>"#
    ))
}

/// `drawing` opened as the file `name` (a package for `.odg`, one flat document for `.fodg`): the session, and the bytes of the file.
fn open(dir: &Dir, name: &str, drawing: &Drawing) -> (Session, Vec<u8>) {
    let bytes = if name.ends_with(".fodg") { drawing.flat_bytes() } else { drawing.bytes() };
    let path = dir.write(name, &bytes);
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    (s, bytes)
}

/// The `ID` of the first group of the first page.
fn group_id(s: &Session) -> u32 {
    s.document().unwrap().pages.get(0).unwrap().shapes.iter().find(|x| !x.children.is_empty()).map(|x| x.id).expect("the group")
}

/// Gives the shape `id` what `kind` says, through its command.
fn give(s: &mut Session, id: u32, kind: &str) {
    let (command, params) = match kind {
        "user" => ("sheet.add_section", json!({ "id": id, "section": "User" })),
        "data" => ("shapedata.define", json!({ "id": id, "fields": [{ "label": "Cost", "type": "number", "value": "3" }] })),
        "glue_row" => ("sheet.add_row", json!({ "id": id, "section": "Connections" })),
        // A group made in DiaCraft holds no Connection Points section: the section is what it is given.
        "glue_section" => ("sheet.add_section", json!({ "id": id, "section": "Connections" })),
        "alt" => ("shape.alt_text", json!({ "id": id, "title": "Other pump" })),
        other => panic!("no such kind {other}"),
    };
    run(s, command, params);
}

/// The message of a `file.save` that was refused.
#[track_caller]
fn refused(s: &mut Session) -> String {
    match s.execute("file.save", &json!({})) {
        Err(EngineError::Failed { msg, .. }) => msg,
        other => panic!("the save was not refused: {other:?}"),
    }
}

/// The first group of the first page of the file read again.
fn group_read(bytes: &[u8]) -> diacraft_doc::Shape {
    let (doc, _) = read_back(bytes);
    doc.pages.get(0).unwrap().shapes.iter().find(|x| !x.children.is_empty()).map(|x| (**x).clone()).expect("the group in the file")
}

/// The rows a section of a shape's sheet holds (0 when it has none).
fn rows(shape: &diacraft_doc::Shape, section: &str) -> usize {
    shape.sheet.section(&diacraft_doc::SectionKey::new(section, None)).map_or(0, |s| s.rows.len())
}

/// **The vector, in a flat file**: each kind of data, given to a group with a name and to one with none, is refused in words that name the group, with nothing written and the session as it
/// was, and the way out the words give holds: the data taken back, the save is no edit. It answered `edited: false` for every one, wrote nothing and marked the session clean.
#[test]
fn data_given_to_a_group_of_a_flat_drawing_is_refused_in_words_and_never_dropped() {
    for (name, who) in [("", "the group with no name numbered 1 on the page \"page1\""), ("Pump", "the group \"Pump\" on the page \"page1\"")] {
        for kind in KINDS {
            let what = format!("{who}: {kind}");
            let dir = Dir::new();
            let (mut s, bytes) = open(&dir, "a.fodg", &drawing(name, ""));
            let id = group_id(&s);
            give(&mut s, id, kind);
            assert!(s.is_dirty(), "{what}: the data is a change of the drawing");
            let said = refused(&mut s);
            assert!(said.contains(&format!("{who} has data of its own")), "{what}: {said}");
            assert!(said.contains("a flat file keeps none of it") && said.ends_with("Nothing was written."), "{what}: {said}");
            assert!(
                !said.contains("Undo") && !said.contains("edit.undo"),
                "{what}: the cause is the group's data, which a later change does not take back: {said}"
            );
            assert!(
                said.chars().count() <= 300 + "`file.save` failed: ".len(),
                "{what}: the window shows the first 300 characters, {}: {said}",
                said.chars().count()
            );
            assert_eq!(dir.read("a.fodg"), bytes, "{what}: nothing was written");
            assert!(s.is_dirty(), "{what}: the session still holds the change, and is not marked clean");
            // The data taken back: the save is then no edit, and the file is the same bytes.
            run(&mut s, "edit.undo", json!({}));
            let saved = run(&mut s, "file.save", json!({}));
            assert_eq!(saved["edited"], false, "{what}: {saved}");
            assert_eq!(dir.read("a.fodg"), bytes, "{what}");
        }
    }
}

/// The package keeps what the flat file cannot: a group with a name has its data in an entry of the document file and the drawing opened again has it; a group with no name is refused in
/// words (it has no key to find an entry by). This is what the flat file's refusal is the twin of.
#[test]
fn the_package_keeps_the_data_of_a_named_group_and_refuses_that_of_an_unnamed_one() {
    for kind in KINDS {
        let dir = Dir::new();
        let (mut s, bytes) = open(&dir, "a.odg", &drawing("Pump", ""));
        let id = group_id(&s);
        give(&mut s, id, kind);
        let saved = run(&mut s, "file.save", json!({}));
        assert_eq!(saved["edited"], true, "{kind}: {saved}");
        let back = group_read(&dir.read("a.odg"));
        match kind {
            "user" | "alt" => assert!(rows(&back, "User") > 0, "{kind}: the User row comes back"),
            "data" => assert!(rows(&back, "Property") > 0, "{kind}: Shape Data comes back"),
            _ => assert_eq!(rows(&back, "Connection"), 5, "{kind}: the standard four and the row given"),
        }
        assert_ne!(dir.read("a.odg"), bytes, "{kind}: the save wrote the file");
        let (mut s, bytes) = open(&dir, "b.odg", &drawing("", ""));
        let id = group_id(&s);
        give(&mut s, id, kind);
        let said = refused(&mut s);
        // The engine's words: the group first, then what a package keeps it under.
        assert!(said.contains("the group with no name numbered 1") && said.contains("has data of its own or a link to a master"), "{kind}: {said}");
        assert_eq!(dir.read("b.odg"), bytes, "{kind}: nothing written");
    }
}

/// A title a group of the flat file has (`svg:title`, `svg:desc`: rows of its alt text) is kept when nobody touches it, and a title changed in the session is refused as any other data of the
/// group is.
#[test]
fn a_title_a_flat_group_has_is_kept_untouched_and_a_title_changed_is_refused() {
    for name in ["", "Pump"] {
        let dir = Dir::new();
        let (mut s, bytes) = open(&dir, "a.fodg", &drawing(name, TITLED));
        let id = group_id(&s);
        // Moved with its title: saved, and the title is the file's.
        run(&mut s, "shape.move", json!({ "ids": [id], "dx": 1.0, "dy": 0.5 }));
        assert_eq!(run(&mut s, "file.save", json!({}))["edited"], true, "{name:?}: a move of a titled group");
        let moved = dir.read("a.fodg");
        assert!(String::from_utf8_lossy(&moved).contains("<svg:title>Pump group</svg:title>"), "{name:?}: the title stays in the file");
        assert_ne!(moved, bytes, "{name:?}");
        // The title changed: refused, and the file is as the move left it.
        give(&mut s, id, "alt");
        let said = refused(&mut s);
        assert!(said.contains("has data of its own") && said.ends_with("Nothing was written."), "{name:?}: {said}");
        assert_eq!(dir.read("a.fodg"), moved, "{name:?}");
    }
}

/// What the markup gives a group is no data of its own: a group with a style and a glue point of its own, a titled group, a group with a name and one with none are moved, resized, moved again
/// and saved after each (every save an edit, written, and the file read again has the group where the session has it), and a save with nothing changed is no edit.
#[test]
fn a_group_of_a_flat_drawing_that_holds_only_what_its_markup_gives_is_moved_resized_and_saved() {
    let cases: [(&str, Drawing); 5] = [
        ("style and glue point", glued()),
        ("titled, no name", drawing("", TITLED)),
        ("titled, named", drawing("Pump", TITLED)),
        ("plain, no name", drawing("", "")),
        ("plain, named", drawing("Pump", "")),
    ];
    for (what, drawing) in cases {
        let dir = Dir::new();
        let (mut s, _) = open(&dir, "a.fodg", &drawing);
        let id = group_id(&s);
        let number = |s: &Session, cell: &str| {
            let doc = s.document().unwrap();
            let shape = doc.pages.get(0).and_then(|p| p.shapes.iter().find(|x| x.id == id)).expect("the group");
            shape.sheet.cell(cell).and_then(|c| c.value.as_ref()?.number()).expect("a number")
        };
        run(&mut s, "shape.move", json!({ "ids": [id], "dx": 12.0 / 7.0, "dy": 0.0 }));
        let first = s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("{what}: the save after the move: {e}"));
        assert_eq!(first["edited"], true, "{what}: {first}");
        let (w, h) = (number(&s, "Width"), number(&s, "Height"));
        let (x, y) = (number(&s, "PinX") - w / 2.0, number(&s, "PinY") - h / 2.0);
        run(&mut s, "shape.resize", json!({ "id": id, "x": x + 0.3, "y": y + 0.7, "width": w * 1.37, "height": h * 0.83 }));
        let second = s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("{what}: the save after the resize: {e}"));
        assert_eq!(second["edited"], true, "{what}: {second}");
        let back = group_read(&dir.read("a.fodg"));
        for cell in ["Width", "Height", "PinX", "PinY"] {
            let now = back.sheet.cell(cell).and_then(|c| c.value.as_ref()?.number()).expect("a number");
            assert!((number(&s, cell) - now).abs() < 1e-9, "{what}: {cell}: the session has {}, the file read again {now}", number(&s, cell));
        }
        run(&mut s, "shape.move", json!({ "ids": [id], "dx": -0.123_456_789, "dy": 0.987_654_321 }));
        let third = s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("{what}: the save after the second move: {e}"));
        assert_eq!(third["edited"], true, "{what}: {third}");
        let bytes = dir.read("a.fodg");
        let last = s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("{what}: the save with nothing changed: {e}"));
        assert_eq!(last["edited"], false, "{what}: {last}");
        assert_eq!(dir.read("a.fodg"), bytes, "{what}");
    }
}

/// Data on a member of the group, which has a style of its own, is saved in a flat file as in a package: the refusal is for the group's own data alone.
#[test]
fn data_given_to_a_member_of_a_group_of_a_flat_drawing_is_saved() {
    for name in ["", "Pump"] {
        let dir = Dir::new();
        let (mut s, bytes) = open(&dir, "a.fodg", &drawing(name, ""));
        let member = s
            .document()
            .unwrap()
            .pages
            .get(0)
            .unwrap()
            .shapes
            .iter()
            .find(|x| !x.children.is_empty())
            .and_then(|g| g.children.get(0))
            .map(|m| m.id)
            .expect("a member");
        give(&mut s, member, "user");
        let saved = s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("{name:?}: {e}"));
        assert_eq!(saved["edited"], true, "{name:?}: {saved}");
        assert_ne!(dir.read("a.fodg"), bytes, "{name:?}");
        assert_eq!(run(&mut s, "file.save", json!({}))["edited"], false, "{name:?}: and again");
    }
}

/// A master link set on a group is refused in words in a flat drawing, as it is in a package (a group's element has no style to carry the link).
#[test]
fn a_master_link_on_a_group_is_refused_in_a_flat_drawing_as_in_a_package() {
    for file in ["a.odg", "a.fodg"] {
        let dir = Dir::new();
        let (mut s, bytes) = open(&dir, file, &drawing("Pump", ""));
        let id = group_id(&s);
        s.edit("Test setup: a master link", |doc: &mut diacraft_doc::Document| {
            let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
            let at = page.shapes.iter().position(|x| x.id == id).unwrap();
            std::sync::Arc::make_mut(page.shapes.get_mut(at).unwrap()).master = Some(7);
            Ok(())
        })
        .unwrap();
        let said = refused(&mut s);
        assert!(said.contains("a change to a group's name or master link"), "{file}: {said}");
        assert_eq!(dir.read(file), bytes, "{file}: nothing written");
    }
}

/// Two shapes drawn in the session and grouped (they have no master, which a flat drawing has no place for), saved in a flat drawing: the group of no data is written, saved again with no edit,
/// and reads back with its members; the same group given data is refused in words that name it, where its data was dropped by the first save, which said it was an edit.
#[test]
fn a_new_group_of_a_flat_drawing_is_saved_when_it_holds_no_data_and_refused_when_it_holds_some() {
    let grouped = |s: &mut Session| {
        let a = run(s, "shape.draw", json!({ "page": 0, "tool": "rectangle", "points": [[6, 6], [8, 7]] }));
        let b = run(s, "shape.draw", json!({ "page": 0, "tool": "rectangle", "points": [[9, 6], [11, 7]] }));
        let group = run(s, "shape.group", json!({ "page": 0, "ids": [a["id"], b["id"]] }));
        u32::try_from(group["id"].as_u64().unwrap()).unwrap()
    };
    // No data: saved, no edit the second time, and the file read again has a second group of two members.
    let dir = Dir::new();
    let (mut s, bytes) = open(&dir, "a.fodg", &drawing("Pump", ""));
    grouped(&mut s);
    let saved = s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("a new group with no data: {e}"));
    assert_eq!(saved["edited"], true, "{saved}");
    assert_ne!(dir.read("a.fodg"), bytes);
    let (doc, _) = read_back(&dir.read("a.fodg"));
    let groups: Vec<usize> = doc.pages.get(0).unwrap().shapes.iter().filter(|x| !x.children.is_empty()).map(|x| x.children.len()).collect();
    assert_eq!(groups, [2, 2], "the group of the file and the new one, each of two members");
    let written = dir.read("a.fodg");
    let again = s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("the second save of a new group: {e}"));
    assert_eq!((again["edited"].clone(), dir.read("a.fodg") == written), (json!(false), true), "{again}");
    // With data: refused, naming the group, and the first save writes nothing.
    for kind in ["user", "data", "glue_section", "alt"] {
        let dir = Dir::new();
        let (mut s, bytes) = open(&dir, "a.fodg", &drawing("Pump", ""));
        let id = grouped(&mut s);
        give(&mut s, id, kind);
        let said = refused(&mut s);
        assert!(said.contains("has data of its own") && said.contains("a flat file keeps none of it"), "{kind}: {said}");
        assert_eq!(dir.read("a.fodg"), bytes, "{kind}: nothing written");
        assert!(s.is_dirty(), "{kind}");
    }
}

/// The words are bounded whatever the file calls its page: a page name as long as the file made it is cut in the message, a line feed in it is escaped, and the way out is whole.
#[test]
fn the_page_name_in_the_refusal_of_a_flat_group_is_cut_and_the_way_out_is_whole() {
    let long = "p".repeat(5_000);
    let dir = Dir::new();
    let text = String::from_utf8(drawing("Pump", "").flat_bytes()).unwrap().replace("draw:name=\"page1\"", &format!("draw:name=\"{long}&#10;[2J\""));
    assert!(text.contains(&long), "the page is called by the long name");
    let path = dir.write("a.fodg", text.as_bytes());
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    let id = group_id(&s);
    give(&mut s, id, "user");
    let said = refused(&mut s);
    assert!(said.chars().count() < 400 && !said.contains('\n') && !said.contains(&long), "{} characters: {said}", said.chars().count());
    assert!(said.ends_with("Nothing was written."), "{said}");
}
