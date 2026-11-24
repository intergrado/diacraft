//! The writer's core:
//! a save that changes only what an edit touched.
//!
//! Every edit test is a **case**, a function of a *saver*: it builds a drawing by hand, reads it, makes its edit in the model, saves through the
//! saver, and asserts what changed in the bytes (an element-level comparison with `diff_packages`, the text of the part itself, and the model the
//! saved bytes read back as). The tests run each case with the real writer, which must pass it, and with an **echo** saver that gives the original
//! bytes back, which must fail every one: a case that an echo passes checks nothing (`echo_control_fails_every_edit_case`).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use diacraft_doc::{Document, Shape, Value};
use diacraft_odg::package::Package;
use diacraft_odg::{Limits, OriginalPackage, PackageDiff, ShapeIndex, WriteError, WriteOptions, Written, diff_packages, read, write_with};

use crate::build::{DRAWING, package};
use crate::drawing::{Drawing, NS, close, cm};

/// The thumbnail the tests pass: a PNG's signature and some bytes.
pub const PNG: [u8; 12] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4];

/// A way of saving: the model, the file it was read from and the index.
pub type Saver = dyn Fn(&Document, &[u8], &ShapeIndex) -> Result<Written, WriteError>;

/// The writer, with a thumbnail.
pub fn real(doc: &Document, bytes: &[u8], index: &ShapeIndex) -> Result<Written, WriteError> {
    write_with(doc, Some(&OriginalPackage { bytes, index }), &WriteOptions { thumbnail: Some(PNG.to_vec()), ..WriteOptions::default() })
}

/// A writer that gives the original back: bytes in, bytes out.
pub fn echo(_doc: &Document, bytes: &[u8], index: &ShapeIndex) -> Result<Written, WriteError> {
    Ok(Written { bytes: bytes.to_vec(), index: index.clone(), notes: Vec::new(), edited: false, work: diacraft_odg::WriteWork::default() })
}

/// An echo that also says it edited (review): bytes in, bytes out, and `edited: true`. A case that checks only the flag passes it.
pub fn echo_says_edited(_doc: &Document, bytes: &[u8], index: &ShapeIndex) -> Result<Written, WriteError> {
    Ok(Written { bytes: bytes.to_vec(), index: index.clone(), notes: Vec::new(), edited: true, work: diacraft_odg::WriteWork::default() })
}

/// A drawing read: its bytes, the model and the index.
pub struct Open {
    pub bytes: Vec<u8>,
    pub doc: Document,
    pub index: ShapeIndex,
}

pub fn open_bytes(bytes: Vec<u8>) -> Open {
    let (doc, report) = read(&bytes, &Limits::default()).unwrap_or_else(|e| panic!("the drawing did not read: {} ({e})", e.class()));
    Open { bytes, doc, index: report.index }
}

pub fn open(d: &Drawing) -> Open {
    open_bytes(d.bytes())
}

/// A save that was made must give a file the reader reads under its default limits (a save never writes a file the reader refuses: `WriteWork::tree_room`). The memory
/// cases call this once they have let go of everything the save held, so that the check costs one read and not a model beside it (their children have no more than 3 GB
/// of address space), and print on a `READBACK` line what the file read as: its items and its pictures.
#[cfg(target_os = "linux")]
pub fn assert_reads_back(what: &str, bytes: &[u8]) {
    let (read, report) = diacraft_odg::read_partial(bytes, &Limits::default());
    println!(
        "\nREADBACK {what} class={} items={} media={} file={}",
        read.as_ref().err().map_or("read", |e| e.class()),
        report.work.items,
        report.work.media_bytes,
        bytes.len()
    );
    assert!(read.is_ok(), "{what}: the file the save made is refused by the reader: {:?}", read.err().map(|e| e.class()));
}

/// The bytes of one part of a package.
pub fn part(bytes: &[u8], name: &str) -> String {
    let p = Package::read(bytes, &Limits::default()).unwrap();
    String::from_utf8(p.part(name).unwrap_or_else(|| panic!("no part {name}")).to_vec()).unwrap()
}

/// The bytes of one part of a package, whatever they are.
pub fn part_bytes(bytes: &[u8], name: &str) -> Vec<u8> {
    let p = Package::read(bytes, &Limits::default()).unwrap();
    p.part(name).unwrap_or_else(|| panic!("no part {name}")).to_vec()
}

pub fn has_part(bytes: &[u8], name: &str) -> bool {
    Package::read(bytes, &Limits::default()).unwrap().part(name).is_some()
}

/// The comparison of two packages.
pub fn compare(a: &[u8], b: &[u8]) -> PackageDiff {
    diff_packages(a, b, &Limits::default()).unwrap()
}

/// The model of the page's top-level shape with `id`, to change.
pub fn shape_mut(doc: &mut Document, page: usize, id: u32) -> &mut Shape {
    let p = Arc::make_mut(doc.pages.get_mut(page).expect("no such page"));
    let at = p.shapes.iter().position(|s| s.id == id).expect("no such shape");
    Arc::make_mut(p.shapes.get_mut(at).expect("no such shape"))
}

/// Sets the number a cell of the shape holds (its formula stays).
pub fn set_num(shape: &mut Shape, cell: &str, value: f64) {
    let sheet = Arc::make_mut(&mut shape.sheet);
    let c = sheet.cells.get_mut(cell).unwrap_or_else(|| panic!("no cell {cell}"));
    c.value = Value::from_number(value);
}

pub fn num_of(shape: &Shape, cell: &str) -> f64 {
    shape.sheet.cell(cell).and_then(|c| c.value.as_ref()?.number()).unwrap_or(0.0)
}

/// Moves the shape on the page by (`dx`, `dy`) centimetres, y up, as a drag in the engine does: `PinX` and `PinY`.
pub fn move_by(doc: &mut Document, page: usize, id: u32, dx: f64, dy: f64) {
    let s = shape_mut(doc, page, id);
    let (x, y) = (num_of(s, "PinX"), num_of(s, "PinY"));
    set_num(s, "PinX", x + cm(dx));
    set_num(s, "PinY", y + cm(dy));
}

/// The `draw:custom-shape` of `name`, indented as a writer lays a page out, at `x` (a length as written).
pub fn custom(name: &str, x: &str) -> String {
    format!(
        "\n    <draw:custom-shape draw:name=\"{name}\" svg:x=\"{x}\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"2.5cm\">\n     <text:p/>\n     <draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"/>\n    </draw:custom-shape>"
    )
}

/// A drawing of one page of the custom shapes `s1` to `sn`, the `k`-th at `k` cm.
pub fn drawing_of(n: usize) -> Drawing {
    Drawing::a4().page(&(1..=n).map(|k| custom(&format!("s{k}"), &format!("{k}cm"))).collect::<String>())
}

/// A `meta.xml` as another writer's: a generator, a creation date, a title.
pub fn full_meta() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-meta {NS} office:version=\"1.3\"><office:meta><meta:generator>TestWriter/9$Linux</meta:generator><dc:title>A title</dc:title><meta:creation-date>2024-05-06T07:08:09</meta:creation-date></office:meta></office:document-meta>"
    )
}

/// What changed of `content.xml` between two packages: the paths and attributes of each changed element, and how many were added and removed.
pub fn content_changes(diff: &PackageDiff) -> (Vec<(String, Vec<String>)>, usize, usize) {
    match diff.parts.iter().find(|p| p.part == "content.xml") {
        Some(p) => (p.changed.iter().map(|c| (c.path.clone(), c.attributes.clone())).collect(), p.counts.added, p.counts.removed),
        None => (Vec::new(), 0, 0),
    }
}

/// The parts the save is allowed to change besides the edit: `meta.xml`, the manifest and the thumbnail.
pub fn only_bookkeeping_changed_beside_content(diff: &PackageDiff) {
    let mut parts: Vec<&str> = diff.parts.iter().map(|p| p.part.as_str()).filter(|p| *p != "content.xml").collect();
    parts.sort_unstable();
    assert!(parts.iter().all(|p| ["META-INF/manifest.xml", "meta.xml"].contains(p)), "{parts:?}");
    assert!(diff.bytes_only.iter().all(|p| p == "Thumbnails/thumbnail.png"), "{:?}", diff.bytes_only);
    assert!(diff.added.iter().all(|p| p == "Thumbnails/thumbnail.png"), "{:?}", diff.added);
    assert!(diff.removed.is_empty() && diff.unparsed.is_empty() && diff.methods_changed.is_empty() && !diff.order_changed, "{diff:?}");
}

// ----------------------------------------------------------------------------------------------------------------------------------------
// The cases.

/// Move: the shape moved 1 cm right and 0.5 cm down: only its `svg:x` and `svg:y` change, in their places, and `meta.xml` and the thumbnail.
fn move_changes_only_that_shapes_svg_x_and_svg_y(save: &Saver) {
    let mut d = drawing_of(3);
    d.meta = Some(full_meta());
    let mut o = open(&d);
    // Shape 2 is at svg:x="2cm" svg:y="3cm", 4 cm by 2.5 cm.
    move_by(&mut o.doc, 0, 2, 1.0, -0.5);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(saved.edited);
    let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    let expected = before.replacen("draw:name=\"s2\" svg:x=\"2cm\" svg:y=\"3cm\"", "draw:name=\"s2\" svg:x=\"3cm\" svg:y=\"3.5cm\"", 1);
    assert_ne!(expected, before);
    assert_eq!(after, expected, "the same bytes but for the two values, in their places and their quotes");
    let diff = compare(&o.bytes, &saved.bytes);
    let (changed, added, removed) = content_changes(&diff);
    assert_eq!((added, removed), (0, 0));
    assert_eq!(changed.len(), 1, "{changed:?}");
    assert!(changed[0].0.ends_with("draw:custom-shape[2]"), "{changed:?}");
    assert_eq!(changed[0].1, ["svg:x", "svg:y"]);
    // `meta.xml` has the generator of DiaCraft and keeps the creation date and the title; the thumbnail is the picture passed.
    only_bookkeeping_changed_beside_content(&diff);
    let (meta_before, meta_after) = (part(&o.bytes, "meta.xml"), part(&saved.bytes, "meta.xml"));
    let (from, to) = (meta_after.find("<meta:generator>").unwrap() + "<meta:generator>".len(), meta_after.find("</meta:generator>").unwrap());
    assert!(meta_after[from..to].starts_with("DiaCraft/"), "{meta_after}");
    assert_eq!(format!("{}TestWriter/9$Linux{}", &meta_after[..from], &meta_after[to..]), meta_before, "only the generator's text changed");
    assert_eq!(&saved.bytes[..38], &o.bytes[..38], "mimetype is still first and stored");
    assert!(Package::read(&saved.bytes, &Limits::default()).unwrap().mimetype_regular());
    assert_eq!(Package::read(&saved.bytes, &Limits::default()).unwrap().part("Thumbnails/thumbnail.png"), Some(&PNG[..]));
    assert!(part(&saved.bytes, "META-INF/manifest.xml").contains("Thumbnails/thumbnail.png"));
    // The model the saved bytes read as: shape 2 is where it was moved to.
    let back = open_bytes(saved.bytes.clone());
    close(num_of(back.doc.pages.get(0).unwrap().shapes.get(1).unwrap(), "PinX"), cm(5.0));
    close(num_of(back.doc.pages.get(0).unwrap().shapes.get(1).unwrap(), "PinY"), cm(29.7 - 4.25 - 0.5));
}

/// R9 resize and rotate: only the attributes the table owns.
fn resize_and_rotate_change_only_owned_attributes(save: &Saver) {
    let owned = ["svg:x", "svg:y", "svg:width", "svg:height", "draw:transform"];
    // Resize: 6 cm by 3 cm about the same centre.
    let mut o = open(&drawing_of(3));
    {
        let s = shape_mut(&mut o.doc, 0, 2);
        for (cell, v) in [("Width", cm(6.0)), ("Height", cm(3.0)), ("LocPinX", cm(3.0)), ("LocPinY", cm(1.5))] {
            set_num(s, cell, v);
        }
    }
    let resized = save(&o.doc, &o.bytes, &o.index).unwrap();
    let diff = compare(&o.bytes, &resized.bytes);
    let (changed, added, removed) = content_changes(&diff);
    assert_eq!((added, removed, changed.len()), (0, 0, 1), "{changed:?}");
    assert!(changed[0].1.iter().all(|a| owned.contains(&a.as_str())), "{changed:?}");
    let (before, after) = (part(&o.bytes, "content.xml"), part(&resized.bytes, "content.xml"));
    let expected = before.replacen(
        "svg:x=\"2cm\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"2.5cm\"",
        "svg:x=\"1cm\" svg:y=\"2.75cm\" svg:width=\"6cm\" svg:height=\"3cm\"",
        1,
    );
    assert_eq!(after, expected);
    // Rotate: 30 degrees about the pin. `svg:x` and `svg:y` go, `draw:transform` comes, and everything else is as it was.
    let mut o = open(&drawing_of(3));
    let angle = 0.5235987755982988_f64;
    let (pin_x, pin_y) = {
        let s = shape_mut(&mut o.doc, 0, 2);
        // A shape read with no turn has no `Angle` cell: the cell is made.
        Arc::make_mut(&mut s.sheet)
            .cells
            .insert(diacraft_doc::CellName::new("Angle"), diacraft_doc::Cell::constant(Value::from_number(angle).unwrap()));
        (num_of(s, "PinX"), num_of(s, "PinY"))
    };
    let turned = save(&o.doc, &o.bytes, &o.index).unwrap();
    let diff = compare(&o.bytes, &turned.bytes);
    let (changed, _, _) = content_changes(&diff);
    assert_eq!(changed.len(), 1, "{changed:?}");
    assert!(changed[0].1.iter().all(|a| owned.contains(&a.as_str())), "{changed:?}");
    let after = part(&turned.bytes, "content.xml");
    let line = after.lines().find(|l| l.contains("draw:name=\"s2\"")).unwrap();
    assert!(!line.contains("svg:x=") && !line.contains("svg:y="), "{line}");
    assert!(
        line.contains("svg:width=\"4cm\" svg:height=\"2.5cm\"") && line.contains("draw:transform=\"rotate (0.5235987755982988) translate ("),
        "{line}"
    );
    // Read back: the same pin, turned 30 degrees.
    let back = open_bytes(turned.bytes.clone());
    let s = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap();
    close(num_of(s, "PinX"), pin_x);
    close(num_of(s, "PinY"), pin_y);
    close(num_of(s, "Angle"), angle);
    close(num_of(s, "Width"), cm(4.0));
}

/// R9 delete: the element goes with the white space before it, and nothing else changes.
fn delete_removes_the_element_and_nothing_else(save: &Saver) {
    let mut o = open(&drawing_of(3));
    let p = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
    p.shapes.remove(1);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    let expected = before.replacen(&custom("s2", "2cm"), "", 1);
    assert_ne!(expected, before);
    assert_eq!(after, expected);
    let diff = compare(&o.bytes, &saved.bytes);
    let (changed, added, removed) = content_changes(&diff);
    assert_eq!((changed.len(), added, removed), (0, 0, 1), "{diff:?}");
    assert_eq!(diff.parts.iter().find(|p| p.part == "content.xml").unwrap().removed[0].path.rsplit('/').next(), Some("draw:custom-shape[2]"));
    // The index: shape 3's element is the second now, and shape 2 has none.
    assert_eq!((saved.index.ordinal(0, 1), saved.index.ordinal(0, 2), saved.index.ordinal(0, 3)), (Some(1), None, Some(2)));
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(back.doc.pages.get(0).unwrap().shapes.len(), 2);
}

/// R4 reorder: the element moves.
fn reorder_moves_the_element(save: &Saver) {
    let mut o = open(&drawing_of(3));
    let p = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
    let first = p.shapes.remove(0).unwrap();
    p.shapes.push(first);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    // `s1` is the last of the three now, with the white space before it; the other two keep their places.
    let expected =
        before.replacen(&custom("s1", "1cm"), "", 1).replacen(&custom("s3", "3cm"), &format!("{}{}", custom("s3", "3cm"), custom("s1", "1cm")), 1);
    assert_eq!(after, expected);
    let diff = compare(&o.bytes, &saved.bytes);
    let part_diff = diff.parts.iter().find(|p| p.part == "content.xml").unwrap();
    assert_eq!(part_diff.counts.moved, 1, "{part_diff:?}");
    assert_eq!((saved.index.ordinal(0, 1), saved.index.ordinal(0, 2), saved.index.ordinal(0, 3)), (Some(3), Some(1), Some(2)));
    let back = open_bytes(saved.bytes.clone());
    let names: Vec<_> = back.doc.pages.get(0).unwrap().shapes.iter().map(|s| s.name.as_deref().unwrap_or("").to_owned()).collect();
    assert_eq!(names, ["s2", "s3", "s1"]);
}

/// A save, then another through the new index: delete shape 3, save, move shape 5, save again.
fn second_save_maps_shapes_through_the_new_index(save: &Saver) {
    let mut o = open(&drawing_of(5));
    Arc::make_mut(o.doc.pages.get_mut(0).unwrap()).shapes.remove(2);
    let first = save(&o.doc, &o.bytes, &o.index).unwrap();
    // The model is the same, the file is the first save's, and shape 5's element is the fourth of four.
    assert_eq!((first.index.ordinal(0, 4), first.index.ordinal(0, 5)), (Some(3), Some(4)));
    move_by(&mut o.doc, 0, 5, 2.0, 0.0);
    let second = save(&o.doc, &first.bytes, &first.index).unwrap();
    let diff = compare(&first.bytes, &second.bytes);
    let (changed, added, removed) = content_changes(&diff);
    assert_eq!((changed.len(), added, removed), (1, 0, 0), "{changed:?}");
    assert!(changed[0].0.ends_with("draw:custom-shape[4]"), "{changed:?}");
    let (before, after) = (part(&first.bytes, "content.xml"), part(&second.bytes, "content.xml"));
    assert_eq!(after, before.replacen("draw:name=\"s5\" svg:x=\"5cm\"", "draw:name=\"s5\" svg:x=\"7cm\"", 1));
    // A third save through the second's index, with shape 1 deleted: the elements shift again and the right one moves.
    Arc::make_mut(o.doc.pages.get_mut(0).unwrap()).shapes.remove(0);
    let third = save(&o.doc, &second.bytes, &second.index).unwrap();
    assert_eq!((third.index.ordinal(0, 2), third.index.ordinal(0, 4), third.index.ordinal(0, 5)), (Some(1), Some(2), Some(3)));
    move_by(&mut o.doc, 0, 4, 0.0, -1.0);
    let fourth = save(&o.doc, &third.bytes, &third.index).unwrap();
    let (changed, _, _) = content_changes(&compare(&third.bytes, &fourth.bytes));
    assert!(changed.len() == 1 && changed[0].0.ends_with("draw:custom-shape[2]"), "{changed:?}");
    let back = open_bytes(fourth.bytes.clone());
    let names: Vec<_> = back.doc.pages.get(0).unwrap().shapes.iter().map(|s| s.name.as_deref().unwrap_or("").to_owned()).collect();
    assert_eq!(names, ["s2", "s4", "s5"]);
}

/// Review Focus 3: a file of `office:version="1.2"` keeps its version through an edited save.
fn edited_save_keeps_office_version_1_2(save: &Saver) {
    let mut d = drawing_of(2);
    d.version = "1.2".to_owned();
    d.meta = Some(full_meta().replace("office:version=\"1.3\"", "office:version=\"1.2\""));
    d.settings = Some(crate::drawing::settings(""));
    let mut o = open(&d);
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(saved.edited);
    // The edit is in the file: the shape moved 1 cm right (an echo that only says `edited` fails here).
    let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    assert!(before.contains("draw:name=\"s1\" svg:x=\"1cm\"") && after.contains("draw:name=\"s1\" svg:x=\"2cm\""), "{after}");
    for name in ["content.xml", "styles.xml", "meta.xml"] {
        let text = part(&saved.bytes, name);
        assert!(text.contains("office:version=\"1.2\""), "{name}: {text}");
        assert!(!text.contains("office:version=\"1.3\"") && !text.contains("office:version=\"1.4\""), "{name}");
    }
    assert!(
        part(&saved.bytes, "settings.xml").contains("office:version=\"1.3\"")
            || part(&saved.bytes, "settings.xml").contains("office:version=\"1.2\"")
    );
    assert_eq!(part(&saved.bytes, "settings.xml"), part(&o.bytes, "settings.xml"), "a part no edit reached is the file's own");
    assert_eq!(part(&saved.bytes, "styles.xml"), part(&o.bytes, "styles.xml"));
    assert!(part(&saved.bytes, "META-INF/manifest.xml").contains("manifest:version=\"1.3\""));
    assert_eq!(read(&saved.bytes, &Limits::default()).unwrap().1.office_version.as_deref(), Some("1.2"));
}

/// A length is written in the unit of its attribute, as the shortest decimal that reads back to the cell's value.
fn lengths_round_trip_exactly_in_their_unit(save: &Saver) {
    let shapes = [
        ("a", "2.5cm", 1.0, "3.5cm"),
        ("b", "25.4mm", 1.0, "35.4mm"),
        ("c", "1.25in", 0.5 * 2.54, "1.75in"),
        ("d", "10.5pt", 0.5 / 72.0 * 2.54, "11pt"),
    ];
    let body: String = shapes.iter().map(|(n, x, ..)| custom(n, x)).collect();
    let mut o = open(&Drawing::a4().page(&body));
    for (at, (_, _, dx_cm, _)) in shapes.iter().enumerate() {
        move_by(&mut o.doc, 0, u32::try_from(at + 1).unwrap(), *dx_cm, 0.0);
    }
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    for (name, _, _, now) in &shapes {
        assert!(after.contains(&format!("draw:name=\"{name}\" svg:x=\"{now}\" svg:y=\"3cm\"")), "{name}: {after}");
    }
    // Read back, each shape is where the model put it, to the bit the unit's arithmetic gives.
    let back = open_bytes(saved.bytes.clone());
    for (at, (_, _, _, now)) in shapes.iter().enumerate() {
        let want = o.doc.pages.get(0).unwrap().shapes.get(at).unwrap();
        close(num_of(back.doc.pages.get(0).unwrap().shapes.get(at).unwrap(), "PinX"), num_of(want, "PinX"));
        assert!(!now.is_empty());
    }
}

/// An OLE frame with its folder, its replacement image and another object and picture that stay.
fn object_drawing() -> Drawing {
    let mut d = Drawing::a4().page(concat!(
        r#"<draw:frame draw:name="ole" svg:x="2cm" svg:y="2cm" svg:width="5cm" svg:height="3cm"><draw:object xlink:href="./Object 1"/><draw:image xlink:href="./ObjectReplacements/Object 1"/></draw:frame>"#,
        r#"<draw:frame draw:name="other" svg:x="2cm" svg:y="8cm" svg:width="5cm" svg:height="3cm"><draw:object xlink:href="./Object 2"/><draw:image xlink:href="./ObjectReplacements/Object 2"/></draw:frame>"#,
    ));
    d.meta = Some(full_meta());
    for (name, bytes) in [
        ("Object 1/content.xml", &b"<office:document-content/>"[..]),
        ("Object 1/styles.xml", b"<office:document-styles/>"),
        ("ObjectReplacements/Object 1", &PNG[..]),
        ("Object 2/content.xml", b"<office:document-content/>"),
        ("ObjectReplacements/Object 2", &PNG[..]),
        ("Pictures/keep.png", &PNG[..]),
    ] {
        d.files.push((name.to_owned(), bytes.to_vec()));
    }
    d
}

/// The entries and manifest rows of a deleted frame go with it.
fn delete_object_removes_its_object_parts_and_manifest_rows(save: &Saver) {
    let mut o = open_bytes(object_with_manifest());
    Arc::make_mut(o.doc.pages.get_mut(0).unwrap()).shapes.remove(0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let diff = compare(&o.bytes, &saved.bytes);
    let mut removed = diff.removed.clone();
    removed.sort_unstable();
    assert_eq!(removed, ["Object 1/content.xml", "Object 1/styles.xml", "ObjectReplacements/Object 1"], "{diff:?}");
    let manifest = part(&saved.bytes, "META-INF/manifest.xml");
    assert!(!manifest.contains("Object 1") && !manifest.contains("ObjectReplacements/Object 1"), "{manifest}");
    let before = part(&o.bytes, "META-INF/manifest.xml");
    for kept in ["Object 2/content.xml", "ObjectReplacements/Object 2", "Pictures/keep.png", "Object 2/"] {
        assert!(manifest.contains(&format!("manifest:full-path=\"{kept}\"")), "{kept}: {manifest}");
    }
    // Only those rows are gone: the manifest is the file's with the three rows, and the folder's row, cut out, and the thumbnail's row added.
    let kept: Vec<&str> = before.lines().filter(|l| !l.contains("Object 1")).collect();
    let after_kept: Vec<&str> = manifest.lines().filter(|l| !l.contains("Thumbnails")).collect();
    assert_eq!(kept, after_kept);
    assert!(has_part(&saved.bytes, "Object 2/content.xml") && has_part(&saved.bytes, "Pictures/keep.png"));
    assert_eq!(part(&saved.bytes, "Object 2/content.xml"), "<office:document-content/>");
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(back.doc.pages.get(0).unwrap().shapes.len(), 1);
}

/// `object_drawing`'s package with a manifest that lists each entry, and the folder rows of the two objects.
fn object_with_manifest() -> Vec<u8> {
    object_with_manifest_under("")
}

/// [`object_with_manifest`] with `doctype` (a document type declaration, or nothing) between the XML declaration and the root.
pub fn object_with_manifest_under(doctype: &str) -> Vec<u8> {
    let d = object_drawing();
    let mut files = d.files();
    let mut rows: Vec<(String, String)> = vec![("/".to_owned(), DRAWING.to_owned())];
    for (name, _) in &files {
        let media = if name.ends_with(".png") || name.starts_with("ObjectReplacements/") { "image/png" } else { "text/xml" };
        rows.push((name.clone(), media.to_owned()));
    }
    rows.insert(1, ("Object 1/".to_owned(), "application/vnd.oasis.opendocument.graphics".to_owned()));
    rows.insert(2, ("Object 2/".to_owned(), "application/vnd.oasis.opendocument.graphics".to_owned()));
    let mut text = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{}<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\">\n",
        if doctype.is_empty() { String::new() } else { format!("{doctype}\n") }
    );
    for (path, media) in &rows {
        if path == "/" {
            text.push_str(&format!(" <manifest:file-entry manifest:full-path=\"/\" manifest:version=\"1.3\" manifest:media-type=\"{media}\"/>\n"));
        } else {
            text.push_str(&format!(" <manifest:file-entry manifest:full-path=\"{path}\" manifest:media-type=\"{media}\"/>\n"));
        }
    }
    text.push_str("</manifest:manifest>\n");
    files.push(("META-INF/manifest.xml".to_owned(), text.into_bytes()));
    crate::build::write(Some(DRAWING), &files)
}

/// A form control moves with its `office:forms` entry: the control's frame is patched and the entry is as it was.
fn move_form_control_keeps_its_forms_entry(save: &Saver) {
    let forms = r#"<office:forms form:automatic-focus="false"><form:form form:name="f"><form:button form:id="control1" form:label="Go"/></form:form></office:forms>"#;
    let control = r#"<draw:control draw:control="control1" draw:layer="controls" svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"/>"#;
    let mut d = Drawing::a4().page(&format!("{forms}{control}"));
    d.ns.push_str(r#" xmlns:form="urn:oasis:names:tc:opendocument:xmlns:form:1.0""#);
    let mut o = open(&d);
    move_by(&mut o.doc, 0, 1, 2.0, 0.0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    assert!(after.contains(forms), "the entry's bytes are as they were");
    assert!(after.contains(r#"draw:control="control1""#));
    assert_eq!(after, before.replacen("svg:x=\"1cm\" svg:y=\"1cm\"", "svg:x=\"3cm\" svg:y=\"1cm\"", 1));
    let (changed, added, removed) = content_changes(&compare(&o.bytes, &saved.bytes));
    assert_eq!((changed.len(), added, removed), (1, 0, 0));
    assert_eq!(changed[0].1, ["svg:x"]);
}

pub type Case = (&'static str, fn(&Saver));

/// Every edit case, by the name of the test the plan gives it.
pub const CASES: &[Case] = &[
    ("move_changes_only_that_shapes_svg_x_and_svg_y", move_changes_only_that_shapes_svg_x_and_svg_y),
    ("resize_and_rotate_change_only_owned_attributes", resize_and_rotate_change_only_owned_attributes),
    ("delete_removes_the_element_and_nothing_else", delete_removes_the_element_and_nothing_else),
    ("reorder_moves_the_element", reorder_moves_the_element),
    ("second_save_maps_shapes_through_the_new_index", second_save_maps_shapes_through_the_new_index),
    ("edited_save_keeps_office_version_1_2", edited_save_keeps_office_version_1_2),
    ("lengths_round_trip_exactly_in_their_unit", lengths_round_trip_exactly_in_their_unit),
    ("delete_object_removes_its_object_parts_and_manifest_rows", delete_object_removes_its_object_parts_and_manifest_rows),
    ("move_form_control_keeps_its_forms_entry", move_form_control_keeps_its_forms_entry),
];

#[test]
fn move_changes_only_that_shapes_svg_x_and_svg_y_test() {
    move_changes_only_that_shapes_svg_x_and_svg_y(&real);
}

#[test]
fn resize_and_rotate_change_only_owned_attributes_test() {
    resize_and_rotate_change_only_owned_attributes(&real);
}

#[test]
fn delete_removes_the_element_and_nothing_else_test() {
    delete_removes_the_element_and_nothing_else(&real);
}

#[test]
fn reorder_moves_the_element_test() {
    reorder_moves_the_element(&real);
}

#[test]
fn second_save_maps_shapes_through_the_new_index_test() {
    second_save_maps_shapes_through_the_new_index(&real);
}

#[test]
fn edited_save_keeps_office_version_1_2_test() {
    edited_save_keeps_office_version_1_2(&real);
}

#[test]
fn lengths_round_trip_exactly_in_their_unit_test() {
    lengths_round_trip_exactly_in_their_unit(&real);
}

#[test]
fn delete_object_removes_its_object_parts_and_manifest_rows_test() {
    delete_object_removes_its_object_parts_and_manifest_rows(&real);
}

#[test]
fn move_form_control_keeps_its_forms_entry_test() {
    move_form_control_keeps_its_forms_entry(&real);
}

/// The salted-edit control (plan): an echo writer, bytes in and bytes out, fails every edit case. A case it passed would check
/// nothing.
#[test]
fn echo_control_fails_every_edit_case() {
    for (name, case) in CASES {
        let outcome = catch_unwind(AssertUnwindSafe(|| case(&echo)));
        let Err(payload) = outcome else { panic!("the echo writer passed the case {name}: the case checks nothing") };
        let said = payload.downcast_ref::<String>().cloned().or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned())).unwrap_or_default();
        println!("ECHO FAILS {name}: {}", said.lines().next().unwrap_or_default().chars().take(160).collect::<String>());
    }
    // So does an echo that says `edited: true`: no case is satisfied by the flag alone.
    for (name, case) in CASES {
        let outcome = catch_unwind(AssertUnwindSafe(|| case(&echo_says_edited)));
        assert!(outcome.is_err(), "an echo that says it edited passed the case {name}: the case checks only the flag");
    }
    // And the real writer passes each of them (the tests above run them one by one; this is the same list).
    for (name, case) in CASES {
        let outcome = catch_unwind(AssertUnwindSafe(|| case(&real)));
        assert!(outcome.is_ok(), "the real writer failed the case {name}");
    }
}

/// An unedited save is the original: every part, and the file.
#[test]
fn unedited_save_is_byte_identical() {
    let mut vectors: Vec<(String, Vec<u8>)> = Vec::new();
    for version in ["1.2", "1.3", "1.4"] {
        vectors.push((format!("libreoffice {version}"), crate::read_families::libreoffice(version)));
    }
    for (version, doctype) in [("1.0", true), ("1.1", true), ("1.2", false)] {
        vectors.push((format!("openoffice {version}"), crate::read_families::openoffice(version, doctype)));
    }
    vectors.push(("hand-built".to_owned(), drawing_of(4).bytes()));
    vectors.push(("objects".to_owned(), object_with_manifest()));
    let mut with_meta = drawing_of(2);
    with_meta.meta = Some(full_meta());
    with_meta.settings = Some(crate::drawing::settings(""));
    with_meta.files.push(("Thumbnails/thumbnail.png".to_owned(), PNG.to_vec()));
    with_meta.files.push(("Configurations2/".to_owned(), Vec::new()));
    vectors.push(("meta, settings, a thumbnail and Configurations2/".to_owned(), with_meta.bytes()));
    let mut w3c = drawing_of(2);
    w3c.ns = w3c
        .ns
        .replace("urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0", "http://www.w3.org/2000/svg")
        .replace("urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0", "http://www.w3.org/1999/XSL/Format");
    vectors.push(("W3C namespaces".to_owned(), w3c.bytes()));
    for (what, bytes) in vectors {
        let o = open_bytes(bytes);
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{what}: {e}"));
        assert!(!saved.edited, "{what}");
        assert!(saved.notes.is_empty(), "{what}");
        assert_eq!(saved.bytes, o.bytes, "{what}: the file");
        let diff = compare(&o.bytes, &saved.bytes);
        assert!(diff.is_empty(), "{what}: {diff:?}");
        assert_eq!(saved.index, o.index, "{what}");
        // Every part, by name.
        let (a, b) = (Package::read(&o.bytes, &Limits::default()).unwrap(), Package::read(&saved.bytes, &Limits::default()).unwrap());
        for name in a.parts() {
            assert_eq!(a.part(name), b.part(name), "{what}: {name}");
        }
    }
    let _ = package;
}

/// The echo control of a list of cases: each runs with the echo writer, which must fail it (the first line of how it fails is printed, for
/// the report), then with an echo that says it edited, which must fail it too, then with the real writer, which must pass it.
pub fn echo_control(cases: &[Case]) {
    for (name, case) in cases {
        let Err(payload) = catch_unwind(AssertUnwindSafe(|| case(&echo))) else {
            panic!("the echo writer passed the case {name}: the case checks nothing")
        };
        let said = payload.downcast_ref::<String>().cloned().or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned())).unwrap_or_default();
        println!("ECHO FAILS {name}: {}", said.lines().next().unwrap_or_default().chars().take(150).collect::<String>());
        assert!(catch_unwind(AssertUnwindSafe(|| case(&echo_says_edited))).is_err(), "an echo that says it edited passed the case {name}");
        assert!(catch_unwind(AssertUnwindSafe(|| case(&real))).is_ok(), "the real writer failed the case {name}");
    }
}

/// Sets the number a cell of a new shape holds, adding the cell when it has none.
pub fn set_num_new(shape: &mut Shape, cell: &str, value: f64) {
    let sheet = Arc::make_mut(&mut shape.sheet);
    sheet.cells.insert(diacraft_doc::CellName::new(cell), diacraft_doc::Cell::constant(Value::from_number(value).expect("a number")));
}
