//! A one-dimensional shape whose geometry is not the plain line is written to an `.odg` as the polyline it draws, in the page's coordinates (`draw:path` when it has curves),
//! so that another program draws the zigzag of a Comm-link where DiaCraft does, and DiaCraft's own data carries the segment's frame (its ends, its pose and its geometry), so that a read
//! gives the same shape back. A save that returns Ok proves nothing of that: the file is opened again in a session of its own and compared with the session's.

use diacraft_doc::Document;
use diacraft_engine::Session;
use serde_json::json;

use crate::odg_build::{Dir, content_of, open_drawing};
use crate::odg_file::{assert_reopens_as_saved, assert_saves_again_as_no_edit, four, save};
use crate::odg_masters::assert_link_holds;
use crate::run;
use crate::stencils2_support::assert_reads_back_as_made;

/// The value of the attribute `name` of the first element that starts with `open` in `content`.
fn attr(content: &str, open: &str, name: &str) -> Option<String> {
    let from = content.find(open)?;
    let tag = content.get(from..)?.split('>').next()?;
    let key = format!(" {name}=\"");
    let at = tag.find(&key)? + key.len();
    Some(tag.get(at..)?.split('"').next()?.to_owned())
}

/// The inches a length attribute says (`3.25in`).
fn inches(text: &str) -> f64 {
    text.strip_suffix("in").unwrap_or_else(|| panic!("a length in inches: {text}")).parse().unwrap()
}

/// A new drawing with the master dropped at (`x`, `y`) on its first page: the session, the page's `ID` and the shape's.
fn dropped(stencil: &str, master: &str, (x, y): (f64, f64)) -> (Session, u32, u32) {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({}));
    let page = s.document().unwrap().pages.get(0).unwrap().id;
    let made = run(&mut s, "shape.drop", json!({ "master": master, "stencil": stencil, "page": page, "x": x, "y": y }));
    let id = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
    (s, page, id)
}

fn reopened(dir: &Dir, name: &str) -> Session {
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path(name) }));
    again
}

/// The number a cell of the first shape of the first page holds.
fn cell_of(doc: &Document, at: usize, name: &str) -> Option<f64> {
    doc.pages.get(0).unwrap().shapes.get(at).unwrap().sheet.cell(name).and_then(|c| c.value.as_ref()?.number())
}

/// The ends of the shape at `at` of the first page.
fn ends_of(doc: &Document, at: usize) -> [f64; 4] {
    ["BeginX", "BeginY", "EndX", "EndY"].map(|n| cell_of(doc, at, n).unwrap_or(f64::NAN))
}

#[track_caller]
fn assert_ends_near(a: [f64; 4], b: [f64; 4], what: &str) {
    for (x, y) in a.iter().zip(b.iter()) {
        assert!((x - y).abs() < 1e-3, "{what}: the ends are {a:?} in the session and {b:?} in the file");
    }
}

/// The points of a `draw:points`, as integers.
fn points_of(text: &str) -> Vec<(i64, i64)> {
    text.split_whitespace()
        .map(|pair| {
            let (x, y) = pair.split_once(',').unwrap();
            (x.parse().unwrap(), y.parse().unwrap())
        })
        .collect()
}

/// The rows of the first `Geometry` section of the shape at `at` of the first page **as the shape draws them** (its own rows over its master's: a Comm-link dropped holds the `X` of its rows
/// and its master the rest): each row's type and its `X` and `Y`.
fn rows_at(doc: &Document, at: usize) -> Vec<(String, f64, f64)> {
    let page = doc.pages.get(0).unwrap();
    let shape = diacraft_doc::ShapeRef::top(diacraft_doc::Owner::Page(page.id), u32::try_from(at).unwrap());
    let rows = diacraft_doc::Resolver::new(doc)
        .effective_rows(&diacraft_doc::SheetRef::Shape(shape), &diacraft_doc::SectionKey::new("Geometry", Some(0)))
        .expect("a geometry section");
    let number = |row: &diacraft_doc::EffectiveRow<'_>, name: &str| {
        row.cells
            .iter()
            .find(|(cell, _)| cell.as_str() == name)
            .and_then(|(_, resolved)| resolved.value.cell.and_then(|c| c.value.as_ref()).and_then(diacraft_doc::Value::number))
            .unwrap_or(f64::NAN)
    };
    rows.iter().map(|r| (r.row_type.as_deref().unwrap_or("").to_owned(), number(r, "X"), number(r, "Y"))).collect()
}

/// The zigzag (or the loop) of the shape at `at` is the session's own, row for row and exactly (to a billionth of an inch, where a path of the file is in hundredths of a millimetre, 4e-4 of
/// one), and it is a figure of more than a straight line's two points.
#[track_caller]
fn assert_same_rows(made: &Document, read: &Document, at: usize, what: &str) {
    let (a, b) = (rows_at(made, at), rows_at(read, at));
    assert!(a.len() >= 4, "{what}: a figure of its own and not a line: {a:?}");
    assert_eq!(a.len(), b.len(), "{what}: the rows: {a:?} against {b:?}");
    for (x, y) in a.iter().zip(b.iter()) {
        assert!(x.0 == y.0 && (x.1 - y.1).abs() < 1e-9 && (x.2 - y.2).abs() < 1e-9, "{what}: a row is {x:?} in the session and {y:?} in the file");
    }
}

/// A Comm-link dropped at (4, 3) of a new drawing has its ends 1.5 inch apart on the line y = 3 (the page is 8.5 inches high), and the zigzag of five legs 0.1 inch to each side: written as a
/// polyline in the page's coordinates, in hundredths of a millimetre from the corner of its own box (a view box of that box's size: 1.5 inch by 0.2).
const ZIGZAG: [(i64, i64); 6] = [(0, 254), (762, 0), (1524, 508), (2286, 0), (3048, 508), (3810, 254)];

/// A Comm-link of a new drawing saved as an `.odg` is a polyline of the zigzag's points and no line, and a session of its own reads the file as the session has the shape: the same ends, the same
/// link to the master, the same picture; and the file saves again as no edit.
#[test]
fn a_comm_link_saved_as_an_odg_is_a_polyline_of_the_zigzag_and_reads_back_the_same() {
    let dir = Dir::new();
    let (mut s, _page, _id) = dropped("network", "network.comm-link", (4.0, 3.0));
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("a.odg") }));
    assert_eq!(saved["warning_count"], 0, "{saved}");
    let content = content_of(&dir.read("a.odg"));
    assert!(content.contains("<draw:polyline "), "the zigzag is a polyline: {content}");
    assert!(!content.contains("<draw:line "), "and no straight line: {content}");
    let points = attr(&content, "<draw:polyline ", "draw:points").expect("the points");
    assert_eq!(points_of(&points), ZIGZAG, "the points of the zigzag, from the corner of its box");
    assert_eq!(attr(&content, "<draw:polyline ", "svg:viewBox").as_deref(), Some("0 0 3810 508"));
    let get = |name: &str| inches(&attr(&content, "<draw:polyline ", name).unwrap());
    assert!((get("svg:x") - 3.25).abs() < 1e-6 && (get("svg:y") - 5.4).abs() < 1e-6, "the box's corner on the page");
    assert!((get("svg:width") - 1.5).abs() < 1e-6 && (get("svg:height") - 0.2).abs() < 1e-6, "the box's size");
    let again = reopened(&dir, "a.odg");
    assert_eq!(again.document().unwrap().pages.get(0).unwrap().shapes.len(), 1);
    assert_ends_near(ends_of(s.document().unwrap(), 0), ends_of(again.document().unwrap(), 0), "a Comm-link");
    assert_reads_back_as_made(s.document().unwrap(), again.document().unwrap(), 0, "a Comm-link as an odg");
    assert_link_holds(s.document().unwrap(), again.document().unwrap(), 0, "a Comm-link as an odg");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    // The file read by a session of its own saves as no edit too, and the same bytes.
    let before = dir.read("a.odg");
    let mut later = reopened(&dir, "a.odg");
    let unedited = save(&mut later);
    assert_eq!(unedited["edited"], false, "{unedited}");
    assert_eq!(dir.read("a.odg"), before, "an unedited save is the same bytes");
}

/// The same Comm-link dropped into a drawing of a file and saved into it: the polyline is added, and the file reads back as the session has it.
#[test]
fn a_comm_link_added_to_a_file_is_a_polyline_of_the_zigzag() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let page = s.document().unwrap().pages.get(0).unwrap().id;
    run(&mut s, "shape.drop", json!({ "master": "network.comm-link", "stencil": "network", "page": page, "x": 4.0, "y": 3.0 }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let content = content_of(&dir.read("a.odg"));
    assert!(content.contains("<draw:polyline "), "the zigzag is a polyline: {content}");
    assert!(!content.contains("<draw:line "), "and no straight line");
    let points = attr(&content, "<draw:polyline ", "draw:points").expect("the points");
    assert_eq!(points_of(&points).len(), 6, "{points}");
    let again = reopened(&dir, "a.odg");
    let n = again.document().unwrap().pages.get(0).unwrap().shapes.len();
    assert_eq!(n, 5, "the four rectangles and the Comm-link");
    assert_ends_near(ends_of(s.document().unwrap(), 4), ends_of(again.document().unwrap(), 4), "a Comm-link added to a file");
    assert_reads_back_as_made(s.document().unwrap(), again.document().unwrap(), 4, "a Comm-link added to a file");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    let before = dir.read("a.odg");
    let mut later = reopened(&dir, "a.odg");
    let unedited = save(&mut later);
    assert_eq!(unedited["edited"], false, "{unedited}");
    assert_eq!(dir.read("a.odg"), before, "an unedited save is the same bytes");
}

/// The Self Message of the UML sequence stencil is the other figure of its kind: a loop out across the line, with the arrow at its end.
#[test]
fn a_self_message_saved_as_an_odg_is_a_polyline_with_its_arrow() {
    let dir = Dir::new();
    let (mut s, _page, _id) = dropped("uml-sequence", "uml-sequence.self-message", (4.0, 3.0));
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.odg") }));
    let content = content_of(&dir.read("a.odg"));
    assert!(content.contains("<draw:polyline "), "{content}");
    assert!(!content.contains("<draw:line "), "{content}");
    assert!(content.contains("draw:marker-end"), "the arrow at the end of the loop: {content}");
    assert!(!content.contains("draw:marker-start"), "and none at its start: {content}");
    let points = points_of(&attr(&content, "<draw:polyline ", "draw:points").unwrap());
    // The loop: up 0.4 inch from the begin, across 0.6, down to the end.
    assert_eq!(points.len(), 4, "{points:?}");
    let (w, h) = (points[3].0 - points[0].0, (points[0].1 - points[1].1).abs());
    assert_eq!((w, h), (1524, 1016), "0.6 inch across, 0.4 inch out");
    let again = reopened(&dir, "a.odg");
    assert_ends_near(ends_of(s.document().unwrap(), 0), ends_of(again.document().unwrap(), 0), "a Self Message");
    assert_reads_back_as_made(s.document().unwrap(), again.document().unwrap(), 0, "a Self Message as an odg");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// A Comm-link whose ends are moved (it is long, and turned) is a polyline whose points follow the line, and the file reads back as the session has it.
#[test]
fn a_comm_link_with_free_ends_off_the_level_is_a_polyline_along_its_line() {
    let dir = Dir::new();
    let (mut s, page, id) = dropped("network", "network.comm-link", (4.0, 3.0));
    run(&mut s, "connector.reconnect", json!({ "page": page, "id": id, "end": "begin", "target": { "x": 2.0, "y": 2.0 } }));
    run(&mut s, "connector.reconnect", json!({ "page": page, "id": id, "end": "end", "target": { "x": 6.0, "y": 5.0 } }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.odg") }));
    let content = content_of(&dir.read("a.odg"));
    assert!(content.contains("<draw:polyline "), "{content}");
    let points = points_of(&attr(&content, "<draw:polyline ", "draw:points").unwrap());
    assert_eq!(points.len(), 6, "{points:?}");
    let again = reopened(&dir, "a.odg");
    assert_ends_near(ends_of(s.document().unwrap(), 0), ends_of(again.document().unwrap(), 0), "a turned Comm-link");
    assert_reads_back_as_made(s.document().unwrap(), again.document().unwrap(), 0, "a turned Comm-link as an odg");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// The built-in masters that are one-dimensional: the two whose geometry is their own (`Fixed1D`) are polylines in an `.odg` (connectors of the type `line` when they are glued);
/// every other (a connector) is a connector. The list is the answer to "which built-in masters are one-dimensional with a geometry that is not a plain line".
#[test]
fn every_builtin_one_dimensional_master_is_written_as_what_it_draws() {
    let mut fixed: Vec<String> = Vec::new();
    for stencil in diacraft_engine::stencil::stencils() {
        for master in stencil.masters().iter().filter(|m| m.is_connector()) {
            let dir = Dir::new();
            let (mut s, _page, _id) = dropped(stencil.id, master.id, (4.0, 3.0));
            let saved = s.execute("file.save_as", &json!({ "path": dir.path("a.odg") }));
            assert!(saved.is_ok(), "{}/{}: {saved:?}", stencil.id, master.id);
            let content = content_of(&dir.read("a.odg"));
            if master.is_fixed_1d() {
                fixed.push(master.id.to_owned());
                assert!(content.contains("<draw:polyline "), "{}/{}: a polyline: {content}", stencil.id, master.id);
                assert!(!content.contains("<draw:line ") && !content.contains("<draw:connector "), "{}/{}: {content}", stencil.id, master.id);
            } else {
                assert!(content.contains("<draw:connector "), "{}/{}: a connector: {content}", stencil.id, master.id);
                assert!(!content.contains("<draw:polyline "), "{}/{}: {content}", stencil.id, master.id);
            }
            let again = reopened(&dir, "a.odg");
            assert_reads_back_as_made(s.document().unwrap(), again.document().unwrap(), 0, &format!("{}/{} as an odg", stencil.id, master.id));
            if master.is_fixed_1d() {
                // Glued between two rectangles: a connector of the type `line` glued to both, which reads back with the connections and the picture the session has.
                let dir = Dir::new();
                let (mut s, _page, [_, _, link]) = glued_figure(stencil.id, master.id);
                run(&mut s, "file.save_as", json!({ "path": dir.path("a.odg") }));
                let content = content_of(&dir.read("a.odg"));
                let what = format!("{}/{} glued", stencil.id, master.id);
                assert!(
                    content.contains("<draw:connector ") && !content.contains("<draw:polyline ") && !content.contains("<draw:line "),
                    "{what}: {content}"
                );
                assert_eq!(attr(&content, "<draw:connector ", "draw:type").as_deref(), Some("line"), "{what}");
                assert!(
                    attr(&content, "<draw:connector ", "draw:start-shape").is_some()
                        && attr(&content, "<draw:connector ", "draw:end-shape").is_some(),
                    "{what}: glued to both shapes: {content}"
                );
                let again = reopened(&dir, "a.odg");
                assert_eq!(glue_of(&again), glue_of(&s), "{}/{}: the connections", stencil.id, master.id);
                assert_eq!(glue_of(&s).len(), 2, "{}/{}: glued at both ends", stencil.id, master.id);
                let at = s.document().unwrap().pages.get(0).unwrap().shapes.iter().position(|x| x.id == link).unwrap();
                assert_reads_back_as_made(s.document().unwrap(), again.document().unwrap(), at, &format!("{what} as an odg"));
                assert_same_rows(s.document().unwrap(), again.document().unwrap(), at, &what);
            }
        }
    }
    fixed.sort();
    assert_eq!(fixed, ["network.comm-link", "uml-sequence.self-message"], "the built-in masters whose geometry is their own");
}

/// A session on a file that holds a Comm-link written as a polyline (a new drawing saved as `a.odg` in `dir`): the session, the page's `ID` and the shape's.
fn session_on_a_file_with_a_comm_link(dir: &Dir) -> (Session, u32, u32) {
    let (mut made, _page, _id) = dropped("network", "network.comm-link", (4.0, 3.0));
    run(&mut made, "file.save_as", json!({ "path": dir.path("a.odg") }));
    let s = reopened(dir, "a.odg");
    let (page, id) = {
        let doc = s.document().unwrap();
        let page = doc.pages.get(0).unwrap();
        (page.id, page.shapes.get(0).unwrap().id)
    };
    (s, page, id)
}

/// What `shape.formatting` says of the shape.
fn looks(s: &mut Session, page: u32, id: u32) -> serde_json::Value {
    run(s, "shape.formatting", json!({ "ids": [id], "page": page }))["shapes"][0].clone()
}

/// A Comm-link of a file, restyled, is saved into the file: the polyline stays what it was, and the file reads back with the new style and the same zigzag.
#[test]
fn a_comm_link_of_a_file_restyled_and_saved_stays_a_polyline_with_the_new_style() {
    let dir = Dir::new();
    let (mut s, page, id) = session_on_a_file_with_a_comm_link(&dir);
    let before = content_of(&dir.read("a.odg"));
    run(&mut s, "shape.format", json!({ "ids": [id], "page": page, "line": "#ff0000", "pattern": 7 }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let content = content_of(&dir.read("a.odg"));
    assert!(content.contains("<draw:polyline ") && !content.contains("<draw:line "), "{content}");
    assert_eq!(
        attr(&content, "<draw:polyline ", "draw:points"),
        attr(&before, "<draw:polyline ", "draw:points"),
        "the zigzag is as it was: only the style changed"
    );
    let mut again = reopened(&dir, "a.odg");
    let (was, now) = (looks(&mut s, page, id), looks(&mut again, page, id));
    assert_eq!(was, now, "the look the session has is the look the file has");
    assert_eq!(now["pattern"], 7, "{now}");
    assert_ends_near(ends_of(s.document().unwrap(), 0), ends_of(again.document().unwrap(), 0), "a restyled Comm-link");
    assert_reads_back_as_made(s.document().unwrap(), again.document().unwrap(), 0, "a restyled Comm-link of a file");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// A Comm-link of a file, moved, is saved into the file: the polyline's box and points are where the shape is, and the file reads back as the session has it.
#[test]
fn a_comm_link_of_a_file_moved_and_saved_is_a_polyline_where_it_moved() {
    let dir = Dir::new();
    let (mut s, page, id) = session_on_a_file_with_a_comm_link(&dir);
    let before = content_of(&dir.read("a.odg"));
    run(&mut s, "shape.move", json!({ "ids": [id], "page": page, "dx": 1.0, "dy": 0.5 }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let content = content_of(&dir.read("a.odg"));
    assert!(content.contains("<draw:polyline ") && !content.contains("<draw:line "), "{content}");
    let get = |text: &str, name: &str| inches(&attr(text, "<draw:polyline ", name).unwrap());
    assert!((get(&content, "svg:x") - get(&before, "svg:x") - 1.0).abs() < 1e-6, "one inch to the right");
    assert!((get(&content, "svg:y") - get(&before, "svg:y") + 0.5).abs() < 1e-6, "half an inch up the page (y grows down it)");
    assert_eq!(attr(&content, "<draw:polyline ", "draw:points"), attr(&before, "<draw:polyline ", "draw:points"), "the same zigzag");
    let again = reopened(&dir, "a.odg");
    assert_ends_near(ends_of(s.document().unwrap(), 0), ends_of(again.document().unwrap(), 0), "a moved Comm-link");
    assert_reads_back_as_made(s.document().unwrap(), again.document().unwrap(), 0, "a moved Comm-link of a file");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// A Comm-link of a file whose ends are put elsewhere (it is longer and turned) is saved into the file: its polyline is the zigzag along the new line.
#[test]
fn a_comm_link_of_a_file_stretched_and_turned_is_a_polyline_along_its_new_line() {
    let dir = Dir::new();
    let (mut s, page, id) = session_on_a_file_with_a_comm_link(&dir);
    run(&mut s, "connector.reconnect", json!({ "page": page, "id": id, "end": "end", "target": { "x": 6.0, "y": 5.0 } }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let content = content_of(&dir.read("a.odg"));
    assert!(content.contains("<draw:polyline ") && !content.contains("<draw:line "), "{content}");
    let again = reopened(&dir, "a.odg");
    assert_ends_near(ends_of(s.document().unwrap(), 0), ends_of(again.document().unwrap(), 0), "a stretched Comm-link");
    assert_reads_back_as_made(s.document().unwrap(), again.document().unwrap(), 0, "a stretched Comm-link of a file");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// A Comm-link of a file, given a text and deleted, is saved: the text is the polyline's, and the delete takes the element out.
#[test]
fn a_comm_link_of_a_file_given_a_text_and_then_deleted_is_saved() {
    let dir = Dir::new();
    let (mut s, page, id) = session_on_a_file_with_a_comm_link(&dir);
    run(&mut s, "shape.set_text", json!({ "id": id, "page": page, "text": "link" }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let content = content_of(&dir.read("a.odg"));
    assert!(content.contains("<draw:polyline ") && content.contains(">link<"), "{content}");
    let again = reopened(&dir, "a.odg");
    assert_reads_back_as_made(s.document().unwrap(), again.document().unwrap(), 0, "a Comm-link with a text");
    run(&mut s, "shape.delete", json!({ "ids": [id], "page": page }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    assert!(!content_of(&dir.read("a.odg")).contains("<draw:polyline "), "the polyline went with the shape");
}

/// The file `from` of `dir` with `change` made to its `content.xml`, written as `to`: what another program that edits the polyline leaves.
fn changed_copy(dir: &Dir, from: &str, to: &str, change: impl Fn(&str) -> String) {
    let bytes = dir.read(from);
    let package = diacraft_odg::Package::read(&bytes, &diacraft_odg::Limits::default()).unwrap();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for name in package.parts().filter(|n| *n != "mimetype" && *n != "META-INF/manifest.xml") {
        let data = package.part(name).unwrap().to_vec();
        let data = if name == "content.xml" { change(&String::from_utf8(data).unwrap()).into_bytes() } else { data };
        files.push((name.to_owned(), data));
    }
    for folder in package.folders() {
        files.push((format!("{}/", folder.trim_end_matches('/')), Vec::new()));
    }
    dir.write(to, &crate::odg_build::package(package.media_type().unwrap(), files));
}

/// The first shape of the first page of the file `name` of `dir` read, and whether it still holds a segment's frame (the ends of a segment).
fn read_first(dir: &Dir, name: &str) -> (Session, diacraft_doc::Shape) {
    let s = reopened(dir, name);
    let shape = (**s.document().unwrap().pages.get(0).unwrap().shapes.get(0).unwrap()).clone();
    (s, shape)
}

/// A polyline that another program has not edited is the segment it was written as, whatever the units it states its box in (LibreOffice writes the lengths in the unit of the document): its
/// frame is what DiaCraft's data carries.
#[test]
fn a_polyline_left_as_it_was_by_another_program_is_still_the_segment() {
    let dir = Dir::new();
    let (mut s, _page, _id) = dropped("network", "network.comm-link", (4.0, 3.0));
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.odg") }));
    // The same box in centimetres, to a hundredth of a millimetre.
    changed_copy(&dir, "a.odg", "b.odg", |c| {
        let (x, y) = (attr(c, "<draw:polyline ", "svg:x").unwrap(), attr(c, "<draw:polyline ", "svg:y").unwrap());
        let (width, height) = (attr(c, "<draw:polyline ", "svg:width").unwrap(), attr(c, "<draw:polyline ", "svg:height").unwrap());
        let cm = |text: &str| format!("{}cm", inches(text) * 2.54);
        c.replace(&format!("svg:x=\"{x}\""), &format!("svg:x=\"{}\"", cm(&x)))
            .replace(&format!("svg:y=\"{y}\""), &format!("svg:y=\"{}\"", cm(&y)))
            .replace(&format!("svg:width=\"{width}\""), &format!("svg:width=\"{}\"", cm(&width)))
            .replace(&format!("svg:height=\"{height}\""), &format!("svg:height=\"{}\"", cm(&height)))
    });
    let (_, shape) = read_first(&dir, "b.odg");
    assert!(shape.sheet.cell("BeginX").is_some() && shape.sheet.cell("EndX").is_some(), "the segment's frame is still carried");
    assert!(shape.master.is_some(), "and its master");
    // As LibreOffice writes a polyline it only saved (measured on 26.8.2.2): the points on its own grid, each a unit of 0.01 mm off at the far edge, and the view box a unit wider.
    changed_copy(&dir, "a.odg", "c.odg", |c| {
        c.replace("0 0 3810 508", "0 0 3811 509")
            .replace("0,254 762,0 1524,508 2286,0 3048,508 3810,254", "0,255 762,0 1524,509 2287,0 3049,509 3811,255")
    });
    let (mut later, shape) = read_first(&dir, "c.odg");
    assert!(shape.sheet.cell("BeginX").is_some() && shape.master.is_some(), "a polyline another program saved is still the segment");
    let before = dir.read("c.odg");
    assert_eq!(save(&mut later)["edited"], false, "and an unedited save of it is no edit");
    assert_eq!(dir.read("c.odg"), before);
}

/// A polyline another program moved, edited in its points, or turned, is the polyline it says: tier 3 takes the segment's frame and geometry out together, the master with them, and
/// the shape is the 2-D shape the reader makes of the element.
#[test]
fn a_polyline_another_program_changed_is_the_polyline_the_file_says() {
    let dir = Dir::new();
    let (mut s, _page, _id) = dropped("network", "network.comm-link", (4.0, 3.0));
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.odg") }));
    let content = content_of(&dir.read("a.odg"));
    let x = attr(&content, "<draw:polyline ", "svg:x").unwrap();
    let points = attr(&content, "<draw:polyline ", "draw:points").unwrap();
    type Change = Box<dyn Fn(&str) -> String>;
    let changes: [(&str, Change); 3] = [
        ("moved", Box::new(move |c: &str| c.replace(&format!("svg:x=\"{x}\""), "svg:x=\"4.25in\""))),
        ("edited", Box::new(move |c: &str| c.replace(&points, &points.replacen("0,254", "0,300", 1)))),
        ("turned", Box::new(|c: &str| c.replacen("<draw:polyline ", "<draw:polyline draw:transform=\"rotate (0.25)\" ", 1))),
    ];
    for (what, change) in changes {
        let name = format!("{what}.odg");
        changed_copy(&dir, "a.odg", &name, change);
        let (mut again, shape) = read_first(&dir, &name);
        assert!(shape.sheet.cell("BeginX").is_none(), "{what}: no segment: the markup's polyline stands");
        assert!(shape.master.is_none(), "{what}: and no master: the shape is not the instance of a line any more");
        assert!(shape.sheet.sections.iter().any(|s| s.name.as_str() == "Geometry"), "{what}: it draws its polyline");
        // The file as it is saves again as no edit.
        let before = dir.read(&name);
        let unedited = save(&mut again);
        assert_eq!(unedited["edited"], false, "{what}: {unedited}");
        assert_eq!(dir.read(&name), before, "{what}: the same bytes");
    }
}

/// A Comm-link of a file saved as `.diacraft` and from it as `.odg` again is the same polyline: the model path writes a shape it read from a polyline as the polyline it draws.
#[test]
fn a_comm_link_of_an_odg_saved_as_diacraft_and_as_odg_again_is_the_same_polyline() {
    let dir = Dir::new();
    let (mut made, _page, _id) = dropped("network", "network.comm-link", (4.0, 3.0));
    run(&mut made, "file.save_as", json!({ "path": dir.path("a.odg") }));
    let mut from_odg = reopened(&dir, "a.odg");
    run(&mut from_odg, "file.save_as", json!({ "path": dir.path("b.diacraft") }));
    let mut from_native = reopened(&dir, "b.diacraft");
    run(&mut from_native, "file.save_as", json!({ "path": dir.path("c.odg") }));
    let (first, second) = (content_of(&dir.read("a.odg")), content_of(&dir.read("c.odg")));
    for what in ["svg:x", "svg:y", "svg:width", "svg:height", "svg:viewBox", "draw:points"] {
        assert_eq!(attr(&first, "<draw:polyline ", what), attr(&second, "<draw:polyline ", what), "{what}");
    }
    assert!(!second.contains("<draw:line "), "{second}");
    let again = reopened(&dir, "c.odg");
    assert_ends_near(ends_of(made.document().unwrap(), 0), ends_of(again.document().unwrap(), 0), "a Comm-link through .diacraft");
    assert_reads_back_as_made(made.document().unwrap(), again.document().unwrap(), 0, "a Comm-link through .diacraft");
}

/// A new drawing with two rectangles and a Comm-link glued from the first to the second: the session, the page's `ID`, and the `ID`s of the first rectangle, the second and the link.
fn glued_comm_link() -> (Session, u32, [u32; 3]) {
    glued_figure("network", "network.comm-link")
}

/// [`glued_comm_link`] for any one-dimensional master of a built-in stencil.
fn glued_figure(stencil: &str, master: &str) -> (Session, u32, [u32; 3]) {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({}));
    let page = s.document().unwrap().pages.get(0).unwrap().id;
    let id = |made: serde_json::Value| u32::try_from(made["id"].as_u64().unwrap()).unwrap();
    let a = id(run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": page, "x": 2.0, "y": 4.0 })));
    let b = id(run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": page, "x": 6.0, "y": 5.0 })));
    let link = id(run(&mut s, "shape.drop", json!({ "master": master, "stencil": stencil, "page": page, "x": 4.0, "y": 8.0 })));
    run(&mut s, "connector.reconnect", json!({ "page": page, "id": link, "end": "begin", "target": { "id": a } }));
    run(&mut s, "connector.reconnect", json!({ "page": page, "id": link, "end": "end", "target": { "id": b } }));
    (s, page, [a, b, link])
}

/// The connections of the first page, as (from, to) pairs of `ID`s, in the order the page holds them.
fn glue_of(s: &Session) -> Vec<(u32, u32)> {
    s.document().unwrap().pages.get(0).unwrap().connects.iter().map(|c| (c.from_sheet, c.to_sheet)).collect()
}

/// A Comm-link glued between two shapes is a **connector** glued to both (another program keeps no glue of a polyline's, and LibreOffice left the end where it was when it
/// moved a rectangle), of the type `line`, with the zigzag's points as its path; DiaCraft's data carries the zigzag, so the file reads back with the connections the session has, the ends where
/// the shapes put them, the zigzag's own rows, the same picture; the end follows the shape it is glued to when that moves, and the file saves again as no edit.
#[test]
fn a_glued_comm_link_is_a_connector_and_stays_glued_through_a_save_and_a_reopen() {
    let dir = Dir::new();
    let (mut s, page, [a, b, link]) = glued_comm_link();
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("a.odg") }));
    assert_eq!(saved["warning_count"], 0, "{saved}");
    let content = content_of(&dir.read("a.odg"));
    assert!(content.contains("<draw:connector "), "glued: a connector: {content}");
    assert!(!content.contains("<draw:polyline ") && !content.contains("<draw:line "), "{content}");
    assert!(!content.contains("diacraft:connects="), "the glue is the connector's own, not DiaCraft's data: {content}");
    assert_eq!(attr(&content, "<draw:connector ", "draw:type").as_deref(), Some("line"), "a straight line between the glue points: {content}");
    let ids: Vec<String> =
        ["draw:start-shape", "draw:end-shape"].iter().map(|n| attr(&content, "<draw:connector ", n).expect("a shape glued to")).collect();
    assert_ne!(ids[0], ids[1], "the two rectangles");
    for name in ["draw:start-glue-point", "draw:end-glue-point"] {
        assert!(attr(&content, "<draw:connector ", name).is_some(), "{name}: {content}");
    }
    let path = attr(&content, "<draw:connector ", "svg:d").expect("the zigzag's points as the path");
    assert_eq!(path.matches('l').count(), 5, "five legs of the zigzag: {path}");
    let mut again = reopened(&dir, "a.odg");
    assert_eq!(glue_of(&again), glue_of(&s), "the connections of the page");
    assert_eq!(glue_of(&s).len(), 2);
    let at = s.document().unwrap().pages.get(0).unwrap().shapes.iter().position(|x| x.id == link).unwrap();
    assert_ends_near(ends_of(s.document().unwrap(), at), ends_of(again.document().unwrap(), at), "a glued Comm-link");
    assert_reads_back_as_made(s.document().unwrap(), again.document().unwrap(), at, "a glued Comm-link as an odg");
    assert_same_rows(s.document().unwrap(), again.document().unwrap(), at, "a glued Comm-link as an odg");
    // The end follows the shape it is glued to: the same move in both sessions gives the same ends, and the zigzag is the figure's still.
    run(&mut s, "shape.move", json!({ "ids": [b], "page": page, "dx": 0.0, "dy": -1.5 }));
    run(&mut again, "shape.move", json!({ "ids": [b], "page": page, "dx": 0.0, "dy": -1.5 }));
    assert_ends_near(
        ends_of(s.document().unwrap(), at),
        ends_of(again.document().unwrap(), at),
        "a glued Comm-link after the shape it is glued to moved",
    );
    assert_eq!(glue_of(&again), glue_of(&s), "and it is glued still");
    let _ = a;
    // The first file saves again as no edit, by a session of its own too.
    run(&mut s, "shape.move", json!({ "ids": [b], "page": page, "dx": 0.0, "dy": 1.5 }));
    let before = dir.read("a.odg");
    let mut later = reopened(&dir, "a.odg");
    let unedited = save(&mut later);
    assert_eq!(unedited["edited"], false, "{unedited}");
    assert_eq!(dir.read("a.odg"), before, "an unedited save is the same bytes");
}

/// A Comm-link glued at one end only is a connector too (the glued end follows its shape in another program; the free end stays where it is), and one glued at no end is the polyline.
#[test]
fn a_comm_link_glued_at_one_end_is_a_connector_and_one_glued_at_none_is_a_polyline() {
    let dir = Dir::new();
    let id = |made: serde_json::Value| u32::try_from(made["id"].as_u64().unwrap()).unwrap();
    // (A session that saved an `.odg` is based on it: each form is made in a new session.)
    let drawing = || {
        let mut s = Session::new();
        run(&mut s, "file.new", json!({}));
        let page = s.document().unwrap().pages.get(0).unwrap().id;
        let a = id(run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": page, "x": 2.0, "y": 4.0 })));
        let link = id(run(&mut s, "shape.drop", json!({ "master": "network.comm-link", "stencil": "network", "page": page, "x": 5.0, "y": 8.0 })));
        (s, page, a, link)
    };
    let (mut s, _page, _a, _link) = drawing();
    run(&mut s, "file.save_as", json!({ "path": dir.path("none.odg") }));
    let none = content_of(&dir.read("none.odg"));
    assert!(none.contains("<draw:polyline ") && !none.contains("<draw:connector "), "glued at no end: the polyline: {none}");
    let (mut s, page, a, link) = drawing();
    run(&mut s, "connector.reconnect", json!({ "page": page, "id": link, "end": "begin", "target": { "id": a } }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("one.odg") }));
    let one = content_of(&dir.read("one.odg"));
    assert!(one.contains("<draw:connector ") && !one.contains("<draw:polyline "), "glued at one end: a connector: {one}");
    assert_eq!(attr(&one, "<draw:connector ", "draw:type").as_deref(), Some("line"));
    assert!(attr(&one, "<draw:connector ", "draw:start-shape").is_some(), "{one}");
    assert!(attr(&one, "<draw:connector ", "draw:end-shape").is_none(), "the free end names no shape: {one}");
    let again = reopened(&dir, "one.odg");
    assert_eq!(glue_of(&again), glue_of(&s));
    assert_eq!(glue_of(&s).len(), 1);
    let at = s.document().unwrap().pages.get(0).unwrap().shapes.iter().position(|x| x.id == link).unwrap();
    assert_ends_near(ends_of(s.document().unwrap(), at), ends_of(again.document().unwrap(), at), "a Comm-link glued at one end");
    assert_same_rows(s.document().unwrap(), again.document().unwrap(), at, "a Comm-link glued at one end");
}

/// A glued Comm-link saved as `.diacraft` and `.vsdx`, and from either as `.odg`, is the same connector: the model path writes the shape it read from a native file or a VSDX file as a connector
/// glued to its shapes, with the zigzag in its own data.
#[test]
fn a_glued_comm_link_saved_as_diacraft_or_vsdx_and_then_as_odg_is_the_same_connector() {
    let dir = Dir::new();
    let (mut made, _page, [_, _, link]) = glued_comm_link();
    run(&mut made, "file.save_as", json!({ "path": dir.path("direct.odg") }));
    let direct = content_of(&dir.read("direct.odg"));
    for via in ["b.diacraft", "b.vsdx"] {
        run(&mut made, "file.save_as", json!({ "path": dir.path(via) }));
        let mut from = Session::new();
        run(&mut from, "file.open", json!({ "path": dir.path(via) }));
        let out = format!("from-{via}.odg");
        run(&mut from, "file.save_as", json!({ "path": dir.path(&out) }));
        let content = content_of(&dir.read(&out));
        assert!(content.contains("<draw:connector ") && !content.contains("<draw:polyline "), "{via}: a connector: {content}");
        assert_eq!(attr(&content, "<draw:connector ", "draw:type").as_deref(), Some("line"), "{via}");
        for name in ["draw:start-shape", "draw:end-shape", "draw:start-glue-point", "draw:end-glue-point", "svg:d"] {
            assert!(attr(&content, "<draw:connector ", name).is_some(), "{via}: {name}: {content}");
        }
        // The path of a model that came through the file is the path of the session's own (the points are in hundredths of a millimetre, from the same rows to a hundredth).
        let (was, now) = (attr(&direct, "<draw:connector ", "svg:d").unwrap(), attr(&content, "<draw:connector ", "svg:d").unwrap());
        assert_eq!(was.matches('l').count(), now.matches('l').count(), "{via}: the zigzag's legs: {was} against {now}");
        let again = reopened(&dir, &out);
        assert_eq!(glue_of(&again), glue_of(&made), "{via}: the connections");
        let at = made.document().unwrap().pages.get(0).unwrap().shapes.iter().position(|x| x.id == link).unwrap();
        assert_same_rows(made.document().unwrap(), again.document().unwrap(), at, via);
    }
}

/// A glued Comm-link of an `.odg` (a connector) saved as `.diacraft` and as `.odg` again is the same connector: the model path writes a shape it read from a connector as the connector it
/// draws, with the same type, the same glue, the same path and the zigzag's rows.
#[test]
fn a_glued_comm_link_of_an_odg_saved_as_diacraft_and_as_odg_again_is_the_same_connector() {
    let dir = Dir::new();
    let (mut made, _page, [_, _, link]) = glued_comm_link();
    run(&mut made, "file.save_as", json!({ "path": dir.path("a.odg") }));
    let mut from_odg = reopened(&dir, "a.odg");
    run(&mut from_odg, "file.save_as", json!({ "path": dir.path("b.diacraft") }));
    let mut from_native = reopened(&dir, "b.diacraft");
    run(&mut from_native, "file.save_as", json!({ "path": dir.path("c.odg") }));
    let (first, second) = (content_of(&dir.read("a.odg")), content_of(&dir.read("c.odg")));
    assert!(!second.contains("<draw:polyline ") && !second.contains("<draw:line "), "{second}");
    for what in ["draw:type", "draw:start-glue-point", "draw:end-glue-point", "svg:d", "svg:viewBox", "svg:x1", "svg:y1", "svg:x2", "svg:y2"] {
        assert!(attr(&first, "<draw:connector ", what).is_some(), "{what}: {first}");
        assert_eq!(attr(&first, "<draw:connector ", what), attr(&second, "<draw:connector ", what), "{what}");
    }
    let again = reopened(&dir, "c.odg");
    assert_eq!(glue_of(&again), glue_of(&made));
    let at = made.document().unwrap().pages.get(0).unwrap().shapes.iter().position(|x| x.id == link).unwrap();
    assert_same_rows(made.document().unwrap(), again.document().unwrap(), at, "a glued Comm-link through .diacraft");
    assert_reads_back_as_made(made.document().unwrap(), again.document().unwrap(), at, "a glued Comm-link through .diacraft");
}

/// A Comm-link the file holds as a polyline (it was saved glued at no end), glued in the session to the two rectangles beside it and saved as `.diacraft` and as `.odg` from there, is a connector
/// glued to them: the model path writes a shape by what it is now, and not as the element it was read from.
#[test]
fn a_polyline_comm_link_of_an_odg_glued_in_the_session_is_a_connector_when_saved_through_diacraft() {
    let dir = Dir::new();
    let mut made = Session::new();
    run(&mut made, "file.new", json!({}));
    let page = made.document().unwrap().pages.get(0).unwrap().id;
    run(&mut made, "shape.drop", json!({ "master": "rectangle", "page": page, "x": 2.0, "y": 4.0 }));
    run(&mut made, "shape.drop", json!({ "master": "rectangle", "page": page, "x": 6.0, "y": 5.0 }));
    run(&mut made, "shape.drop", json!({ "master": "network.comm-link", "stencil": "network", "page": page, "x": 4.0, "y": 8.0 }));
    run(&mut made, "file.save_as", json!({ "path": dir.path("a.odg") }));
    assert!(content_of(&dir.read("a.odg")).contains("<draw:polyline "), "unglued: the polyline");
    let mut s = reopened(&dir, "a.odg");
    let (page, a, b, link) = {
        let page = s.document().unwrap().pages.get(0).unwrap();
        (page.id, page.shapes.get(0).unwrap().id, page.shapes.get(1).unwrap().id, page.shapes.get(2).unwrap().id)
    };
    run(&mut s, "connector.reconnect", json!({ "page": page, "id": link, "end": "begin", "target": { "id": a } }));
    run(&mut s, "connector.reconnect", json!({ "page": page, "id": link, "end": "end", "target": { "id": b } }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("b.diacraft") }));
    let mut native = Session::new();
    run(&mut native, "file.open", json!({ "path": dir.path("b.diacraft") }));
    run(&mut native, "file.save_as", json!({ "path": dir.path("c.odg") }));
    let content = content_of(&dir.read("c.odg"));
    assert!(content.contains("<draw:connector ") && !content.contains("<draw:polyline "), "glued now: a connector: {content}");
    assert_eq!(attr(&content, "<draw:connector ", "draw:type").as_deref(), Some("line"), "{content}");
    assert!(
        attr(&content, "<draw:connector ", "draw:start-shape").is_some() && attr(&content, "<draw:connector ", "draw:end-shape").is_some(),
        "{content}"
    );
    let again = reopened(&dir, "c.odg");
    assert_eq!(glue_of(&again), glue_of(&s), "the connections");
    assert_eq!(glue_of(&s).len(), 2);
    assert_same_rows(s.document().unwrap(), again.document().unwrap(), 2, "a Comm-link glued in the session");
}

/// A Comm-link glued between two rectangles that are grouped with it is a connector of the type `line` inside the group's `draw:g`, glued to the rectangles, the zigzag's points on the page by the
/// group's frame as every point of the group is: the file reads back with the glue the session has, and draws what the session draws.
#[test]
fn a_glued_comm_link_inside_a_group_is_a_connector_in_the_group() {
    let dir = Dir::new();
    let (mut s, page, [a, b, link]) = glued_comm_link();
    run(&mut s, "shape.group", json!({ "page": page, "ids": [a, b, link] }));
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("a.odg") }));
    assert_eq!(saved["warning_count"], 0, "{saved}");
    let content = content_of(&dir.read("a.odg"));
    let group = content.find("<draw:g ").expect("the group");
    let connector = content.find("<draw:connector ").expect("the connector");
    assert!(group < connector, "the connector is in the group: {content}");
    assert!(!content.contains("<draw:polyline "), "{content}");
    assert_eq!(attr(&content, "<draw:connector ", "draw:type").as_deref(), Some("line"), "{content}");
    assert!(
        attr(&content, "<draw:connector ", "draw:start-shape").is_some() && attr(&content, "<draw:connector ", "draw:end-shape").is_some(),
        "{content}"
    );
    let path = attr(&content, "<draw:connector ", "svg:d").expect("the path");
    assert_eq!(path.matches('l').count(), 5, "the zigzag's five legs: {path}");
    let again = reopened(&dir, "a.odg");
    assert_eq!(glue_of(&again), glue_of(&s), "the connections of the page");
    assert_reads_back_as_made(s.document().unwrap(), again.document().unwrap(), 0, "a glued Comm-link inside a group");
}

/// Two new shapes and a Comm-link glued between them, added to a file: a connector of the type `line` glued to both, the zigzag in DiaCraft's data. A Comm-link glued to a shape the file holds is
/// a connector as it was.
#[test]
fn a_glued_comm_link_added_to_a_file_is_a_connector_whether_its_shapes_are_new_or_the_files() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let page = s.document().unwrap().pages.get(0).unwrap().id;
    let id = |made: serde_json::Value| u32::try_from(made["id"].as_u64().unwrap()).unwrap();
    let a = id(run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": page, "x": 2.0, "y": 1.0 })));
    let b = id(run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": page, "x": 6.0, "y": 2.0 })));
    let link = id(run(&mut s, "shape.drop", json!({ "master": "network.comm-link", "stencil": "network", "page": page, "x": 4.0, "y": 8.0 })));
    run(&mut s, "connector.reconnect", json!({ "page": page, "id": link, "end": "begin", "target": { "id": a } }));
    run(&mut s, "connector.reconnect", json!({ "page": page, "id": link, "end": "end", "target": { "id": b } }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let content = content_of(&dir.read("a.odg"));
    assert!(content.contains("<draw:connector ") && !content.contains("<draw:polyline "), "between new shapes: a connector: {content}");
    assert_eq!(attr(&content, "<draw:connector ", "draw:type").as_deref(), Some("line"), "{content}");
    assert!(!content.contains("diacraft:connects="), "{content}");
    let again = reopened(&dir, "a.odg");
    assert_eq!(glue_of(&again), glue_of(&s), "the connections of the page");
    let at = s.document().unwrap().pages.get(0).unwrap().shapes.iter().position(|x| x.id == link).unwrap();
    assert_reads_back_as_made(s.document().unwrap(), again.document().unwrap(), at, "a glued Comm-link added to a file");
    assert_same_rows(s.document().unwrap(), again.document().unwrap(), at, "a glued Comm-link added to a file");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    // Glued to the file's own shapes: a connector, with its glue in the element, the type `line` and the zigzag's rows carried.
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let page = s.document().unwrap().pages.get(0).unwrap().id;
    let doc = s.document().unwrap().clone();
    let (first, second) = (crate::odg_build::id_of(&doc, 0, "A"), crate::odg_build::id_of(&doc, 0, "B"));
    let link = id(run(&mut s, "shape.drop", json!({ "master": "network.comm-link", "stencil": "network", "page": page, "x": 4.0, "y": 8.0 })));
    run(&mut s, "connector.reconnect", json!({ "page": page, "id": link, "end": "begin", "target": { "id": first } }));
    run(&mut s, "connector.reconnect", json!({ "page": page, "id": link, "end": "end", "target": { "id": second } }));
    save(&mut s);
    let content = content_of(&dir.read("a.odg"));
    assert!(content.contains("<draw:connector ") && !content.contains("<draw:polyline "), "glued to the file's shapes: a connector: {content}");
    assert_eq!(attr(&content, "<draw:connector ", "draw:type").as_deref(), Some("line"), "{content}");
    let again = reopened(&dir, "a.odg");
    assert_eq!(glue_of(&again), glue_of(&s), "the connections of the page");
    let at = s.document().unwrap().pages.get(0).unwrap().shapes.iter().position(|x| x.id == link).unwrap();
    assert_same_rows(s.document().unwrap(), again.document().unwrap(), at, "a Comm-link glued to the file's shapes");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// A glued Comm-link of a file (a connector) is saved after an edit: the shape it is glued to moved (the connector follows, glued still, its zigzag the figure's), the glue taken off one end
/// (the other stays), and the shape the other end is glued to deleted. Each saves, and the file reads back with the connections the session has and none that dangles.
#[test]
fn a_glued_comm_link_of_a_file_follows_its_shape_and_loses_its_glue_with_a_save() {
    let dir = Dir::new();
    let (mut made, _page, [_, _, _]) = glued_comm_link();
    run(&mut made, "file.save_as", json!({ "path": dir.path("a.odg") }));
    let mut s = reopened(&dir, "a.odg");
    let (page, shapes) = {
        let doc = s.document().unwrap();
        let page = doc.pages.get(0).unwrap();
        (page.id, page.shapes.iter().map(|x| x.id).collect::<Vec<_>>())
    };
    let (a, b, link) = (shapes[0], shapes[1], shapes[2]);
    // The shape it is glued to moves.
    let before = attr(&content_of(&dir.read("a.odg")), "<draw:connector ", "svg:y2").unwrap();
    run(&mut s, "shape.move", json!({ "ids": [b], "page": page, "dx": 1.0, "dy": -1.5 }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let content = content_of(&dir.read("a.odg"));
    assert!(content.contains("<draw:connector ") && !content.contains("<draw:polyline "), "{content}");
    assert_ne!(attr(&content, "<draw:connector ", "svg:y2").unwrap(), before, "the connector followed the shape");
    assert_eq!(attr(&content, "<draw:connector ", "draw:type").as_deref(), Some("line"), "{content}");
    let again = reopened(&dir, "a.odg");
    assert_eq!(glue_of(&again), glue_of(&s));
    assert_ends_near(ends_of(s.document().unwrap(), 2), ends_of(again.document().unwrap(), 2), "a followed Comm-link");
    assert_reads_back_as_made(s.document().unwrap(), again.document().unwrap(), 2, "a followed Comm-link");
    assert_same_rows(s.document().unwrap(), again.document().unwrap(), 2, "a followed Comm-link");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    // The glue is taken off its begin: the end is put at a free place.
    run(&mut s, "connector.reconnect", json!({ "page": page, "id": link, "end": "begin", "target": { "x": 1.0, "y": 8.0 } }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let again = reopened(&dir, "a.odg");
    assert_eq!(glue_of(&again), glue_of(&s), "the glue the session has");
    assert_eq!(glue_of(&s).len(), 1, "one end is glued still");
    assert_ends_near(ends_of(s.document().unwrap(), 2), ends_of(again.document().unwrap(), 2), "a Comm-link unglued at its begin");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    // The shape its end is glued to is deleted: the glue goes, and the file has no connection that dangles.
    run(&mut s, "shape.delete", json!({ "ids": [b], "page": page }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let again = reopened(&dir, "a.odg");
    assert_eq!(glue_of(&again), glue_of(&s));
    assert!(glue_of(&again).is_empty(), "{:?}", glue_of(&again));
    let _ = a;
}

/// An end glued anew on a Comm-link of a file that holds it as a polyline (it was saved glued at no end) is refused in words (the element has no glue to write: Save As writes it as a connector),
/// and the file stays as it was.
#[test]
fn an_end_glued_anew_on_a_comm_link_the_file_holds_as_a_polyline_is_refused_in_words() {
    let dir = Dir::new();
    let (mut made, page, [_, _, link]) = glued_comm_link();
    run(&mut made, "connector.reconnect", json!({ "page": page, "id": link, "end": "begin", "target": { "x": 4.0, "y": 8.0 } }));
    run(&mut made, "connector.reconnect", json!({ "page": page, "id": link, "end": "end", "target": { "x": 6.0, "y": 8.0 } }));
    run(&mut made, "file.save_as", json!({ "path": dir.path("a.odg") }));
    assert!(content_of(&dir.read("a.odg")).contains("<draw:polyline "), "glued at no end: the polyline");
    let mut s = reopened(&dir, "a.odg");
    let (page, b, link) = {
        let doc = s.document().unwrap();
        let page = doc.pages.get(0).unwrap();
        (page.id, page.shapes.get(1).unwrap().id, page.shapes.get(2).unwrap().id)
    };
    run(&mut s, "connector.reconnect", json!({ "page": page, "id": link, "end": "begin", "target": { "id": b } }));
    let before = dir.read("a.odg");
    let words = crate::odg_file::refused(&mut s);
    assert!(words.contains("glued") || words.contains("glue"), "{words}");
    assert_eq!(dir.read("a.odg"), before, "nothing was written");
}

/// A Comm-link whose ends are one point has a geometry with no box (every point of it on one line level with an axis, which a view box cannot map): it is written as it was, a line, and
/// the save is made and reads back.
#[test]
fn a_comm_link_of_no_length_is_written_as_it_was() {
    let dir = Dir::new();
    let (mut s, page, id) = dropped("network", "network.comm-link", (4.0, 3.0));
    run(&mut s, "connector.reconnect", json!({ "page": page, "id": id, "end": "begin", "target": { "x": 5.0, "y": 3.0 } }));
    run(&mut s, "connector.reconnect", json!({ "page": page, "id": id, "end": "end", "target": { "x": 5.0, "y": 3.0 } }));
    let saved = s.execute("file.save_as", &json!({ "path": dir.path("a.odg") }));
    assert!(saved.is_ok(), "{saved:?}");
    let content = content_of(&dir.read("a.odg"));
    assert!(content.contains("<draw:line ") && !content.contains("<draw:polyline "), "{content}");
    let again = reopened(&dir, "a.odg");
    assert_ends_near(ends_of(s.document().unwrap(), 0), ends_of(again.document().unwrap(), 0), "a Comm-link of no length");
}

/// Not a check: writes the files a look, to the folder `POLY1D_OUT` (run it with `--ignored`). Each is a page of the figures it names.
#[test]
#[ignore = "writes files for a look with LibreOffice"]
fn files_for_a_look_with_another_program() {
    let out = std::path::PathBuf::from(std::env::var("POLY1D_OUT").expect("POLY1D_OUT: the folder to write to"));
    std::fs::create_dir_all(&out).unwrap();
    let to = |s: &mut Session, name: &str| {
        let path = out.join(name);
        run(s, "file.save_as", json!({ "path": path.to_str().unwrap() }));
    };
    // The Comm-link and the Self Message level, and a Comm-link turned and long, each on its own page's place.
    let mut s = Session::new();
    run(&mut s, "file.new", json!({}));
    let page = s.document().unwrap().pages.get(0).unwrap().id;
    for (master, stencil, x, y) in [("network.comm-link", "network", 3.0, 7.0), ("uml-sequence.self-message", "uml-sequence", 8.0, 7.0)] {
        run(&mut s, "shape.drop", json!({ "master": master, "stencil": stencil, "page": page, "x": x, "y": y }));
    }
    let turned = run(&mut s, "shape.drop", json!({ "master": "network.comm-link", "stencil": "network", "page": page, "x": 4.0, "y": 3.0 }))["id"]
        .as_u64()
        .unwrap();
    run(&mut s, "connector.reconnect", json!({ "page": page, "id": turned, "end": "begin", "target": { "x": 2.0, "y": 2.0 } }));
    run(&mut s, "connector.reconnect", json!({ "page": page, "id": turned, "end": "end", "target": { "x": 6.0, "y": 4.5 } }));
    to(&mut s, "unglued.odg");
    // A Comm-link glued between two rectangles (a page of its own): a connector of the type `line` between them.
    let (mut s, _page, _ids) = glued_comm_link();
    to(&mut s, "glued.odg");
    // A Self Message glued at both ends, to the same two rectangles.
    let (mut s, _page, _ids) = glued_figure("uml-sequence", "uml-sequence.self-message");
    to(&mut s, "glued_self.odg");
    // A Comm-link glued at its begin only.
    let mut s = Session::new();
    run(&mut s, "file.new", json!({}));
    let page = s.document().unwrap().pages.get(0).unwrap().id;
    let first = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": page, "x": 2.0, "y": 4.0 }))["id"].as_u64().unwrap();
    let link = run(&mut s, "shape.drop", json!({ "master": "network.comm-link", "stencil": "network", "page": page, "x": 6.0, "y": 8.0 }))["id"]
        .as_u64()
        .unwrap();
    run(&mut s, "connector.reconnect", json!({ "page": page, "id": link, "end": "begin", "target": { "id": first } }));
    to(&mut s, "one_end.odg");
}

/// Not a check: what a read of the file `POLY1D_IN` makes of its first page (run it with `--ignored`): for each shape, whether it holds the ends of a segment, its master, its geometry's row
/// types, and the page's connections.
#[test]
#[ignore = "reports on a file for a look with LibreOffice"]
fn report_on_a_file() {
    let path = std::env::var("POLY1D_IN").expect("POLY1D_IN: the file to read");
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    let doc = s.document().unwrap();
    let page = doc.pages.get(0).unwrap();
    for shape in page.shapes.iter() {
        let rows: Vec<String> = shape
            .sheet
            .sections
            .iter()
            .filter(|x| x.name.as_str() == "Geometry")
            .flat_map(|x| x.rows.iter().map(|r| r.row_type.as_deref().unwrap_or("").to_owned()))
            .collect();
        eprintln!(
            "shape {}: segment {} master {:?} width {:?} rows {:?}",
            shape.id,
            shape.sheet.cell("BeginX").is_some(),
            shape.master,
            shape.sheet.cell("Width").and_then(|c| c.value.as_ref()?.number()),
            rows
        );
    }
    eprintln!("connects: {:?}", page.connects.iter().map(|c| (c.from_sheet, c.to_sheet)).collect::<Vec<_>>());
}

/// A connector the engine routes (`connector.add`), between two new shapes and drawn free, is no figure of its own: it keeps its route cell and its right-angled path, and is written as it was, a
/// connector of the standard type (no `draw:type`), not the `line` of the Comm-link.
#[test]
fn a_routed_connector_keeps_the_standard_type_and_its_path() {
    let dir = Dir::new();
    let mut s = Session::new();
    run(&mut s, "file.new", json!({}));
    let page = s.document().unwrap().pages.get(0).unwrap().id;
    let id = |made: serde_json::Value| u32::try_from(made["id"].as_u64().unwrap()).unwrap();
    let a = id(run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": page, "x": 2.0, "y": 4.0 })));
    let b = id(run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": page, "x": 6.0, "y": 5.0 })));
    run(&mut s, "connector.add", json!({ "page": page, "from": { "id": a }, "to": { "id": b } }));
    run(&mut s, "connector.add", json!({ "page": page, "from": { "x": 1.0, "y": 1.0 }, "to": { "x": 3.0, "y": 2.0 } }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.odg") }));
    let content = content_of(&dir.read("a.odg"));
    let tags: Vec<&str> = content.split("<draw:connector ").skip(1).filter_map(|piece| piece.split('>').next()).collect();
    assert_eq!(tags.len(), 2, "two connectors: {content}");
    assert!(!content.contains("<draw:polyline "), "{content}");
    for tag in tags {
        assert!(!tag.contains("draw:type="), "the standard type, which is no attribute: {tag}");
        assert!(tag.contains("svg:d=\"M ") && tag.contains(" h ") && tag.contains(" v "), "the right-angled route as the path: {tag}");
    }
}
