//! Line ends on new connectors and on edits of existing ones (add a
//! connector, restyle): `draw:marker-start` and `-end` with their widths and centres, the `draw:marker`
//! elements added to `office:styles` of `styles.xml` under DiaCraft's names, a marker the file holds reused by its name whatever it draws, and a kind the
//! specification gives no outline for refused in words.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Shape, Value};
use diacraft_odg::WriteError;

use crate::build::{DRAWING, package};
use crate::drawing::{Drawing, cm};
use crate::write_core::{Case, Open, Saver, compare, content_changes, echo_control, num_of, open, open_bytes, part, real, shape_mut};
use crate::write_new::{connector, push_shape, rectangle};

/// The page is A4 and `office:styles` is there, empty.
pub(crate) fn base() -> Drawing {
    Drawing::a4().page(&crate::write_core::custom("s1", "1cm"))
}

pub(crate) fn set(shape: &mut Shape, name: &str, value: f64) {
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new(name), Cell::constant(Value::from_number(value).unwrap()));
}

/// A new connector with a line end at its end (`end`: the number and the size class) and one at its begin, and the line's weight in points.
pub(crate) fn arrowed(id: u32, begin: Option<(f64, f64)>, end: Option<(f64, f64)>, weight_pt: f64) -> Shape {
    let mut c = connector(id, (1.0, 10.0, 8.0, 12.0));
    set(&mut c, "LineWeight", weight_pt / 72.0);
    if let Some((n, size)) = begin {
        set(&mut c, "BeginArrow", n);
        set(&mut c, "BeginArrowSize", size);
    }
    if let Some((n, size)) = end {
        set(&mut c, "EndArrow", n);
        set(&mut c, "EndArrowSize", size);
    }
    c
}

/// The length a style attribute holds, in points.
pub(crate) fn points(text: &str) -> f64 {
    let (number, unit) = text.split_at(text.find(|c: char| c.is_ascii_alphabetic()).unwrap());
    let n: f64 = number.parse().unwrap();
    n * match unit {
        "pt" => 1.0,
        "cm" => 72.0 / 2.54,
        "mm" => 72.0 / 25.4,
        "in" => 72.0,
        other => panic!("unit {other}"),
    }
}

/// The value of attribute `name` in the first style of `part` that holds it, or `None`.
pub(crate) fn attr(text: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let from = text.find(&key)? + key.len();
    Some(text[from..from + text[from..].find('"')?].to_owned())
}

/// The `draw:marker` elements of `styles.xml`.
pub(crate) fn markers(bytes: &[u8]) -> Vec<String> {
    let styles = part(bytes, "styles.xml");
    styles.split("<draw:marker ").skip(1).map(|m| m.split("/>").next().unwrap().to_owned()).collect()
}

/// A new connector whose end is number 13 at size class 2 and weight 0.75 pt: its style names `DiaCraft_20_end_20_13` with the width the spread
/// times the `S` gives (6.54 pt), and `styles.xml` holds the marker; read back, the connector has the end and the class.
fn end_arrow_case(save: &Saver) {
    let mut o = open(&base());
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, arrowed(id, None, Some((13.0, 2.0)), 0.75));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    assert_eq!(attr(&after, "draw:marker-end").as_deref(), Some("DiaCraft_20_end_20_13"), "{after}");
    let width = attr(&after, "draw:marker-end-width").unwrap_or_else(|| panic!("no width: {after}"));
    assert_eq!(width, "6.54pt", "Written in points, as the vectors give it");
    assert!(!after.contains("marker-end-center") && !after.contains("marker-start"), "a triangle is not centred, and there is no begin: {after}");
    // The marker, in `office:styles`, one for the number.
    let held = markers(&saved.bytes);
    assert_eq!(held.len(), 1, "{held:?}");
    assert!(held[0].contains("draw:name=\"DiaCraft_20_end_20_13\"") && held[0].contains("svg:viewBox=\"0 0 2000 3000\""), "{held:?}");
    assert!(held[0].contains("svg:d=\"M 1000 0 L 2000 3000 L 0 3000 Z\""), "{held:?}");
    // Only the parts the edit owns changed.
    let diff = compare(&o.bytes, &saved.bytes);
    let parts: Vec<&str> = diff.parts.iter().map(|p| p.part.as_str()).collect();
    assert!(parts.contains(&"content.xml") && parts.contains(&"styles.xml"), "{parts:?}");
    assert!(parts.iter().all(|p| ["content.xml", "styles.xml", "meta.xml", "META-INF/manifest.xml"].contains(p)), "{parts:?}");
    let styles = diff.parts.iter().find(|p| p.part == "styles.xml").unwrap();
    assert_eq!((styles.changed.len(), styles.counts.added, styles.counts.removed), (0, 1, 0), "only the marker: {diff:?}");
    let (changed, _, _) = content_changes(&diff);
    assert!(changed.is_empty(), "nothing of the file's own changed: {changed:?}");
    // Read back through the reader: the end and its class, and no begin.
    let back = open_bytes(saved.bytes.clone());
    let read = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap();
    assert_eq!((num_of(read, "EndArrow"), num_of(read, "EndArrowSize")), (13.0, 2.0));
    assert_eq!(read.sheet.cell("BeginArrow").and_then(|c| c.value.as_ref()?.number()).unwrap_or(0.0), 0.0);
    // The second and the third save are no edit.
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
}

/// The two ends, the size classes and the weights: each width is the spread times `S`, and the reader gives the class back. A disc is centred on the vertex.
fn two_ends_case(save: &Saver) {
    for (begin, end, weight) in [
        ((4.0, 0.0), (4.0, 1.0), 0.25),
        ((10.0, 3.0), (13.0, 4.0), 1.0),
        ((11.0, 5.0), (39.0, 2.0), 6.0),
        ((2.0, 2.0), (5.0, 2.0), 0.75),
        ((6.0, 1.0), (42.0, 0.0), 2.0),
    ] {
        let mut o = open(&base());
        let id = o.index.next_id(0).unwrap();
        push_shape(&mut o.doc, arrowed(id, Some(begin), Some(end), weight));
        let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{begin:?} {end:?}: {e}"));
        let after = part(&saved.bytes, "content.xml");
        let centred = |n: f64| matches!(n as u32, 10 | 11 | 42);
        assert_eq!(attr(&after, "draw:marker-start-center").is_some(), centred(begin.0), "{after}");
        assert_eq!(attr(&after, "draw:marker-end-center").is_some(), centred(end.0), "{after}");
        if centred(begin.0) {
            assert_eq!(attr(&after, "draw:marker-start-center").as_deref(), Some("true"));
        }
        let back = open_bytes(saved.bytes.clone());
        let read = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap();
        assert_eq!((num_of(read, "BeginArrow"), num_of(read, "BeginArrowSize")), (begin.0, begin.1), "{after}");
        assert_eq!((num_of(read, "EndArrow"), num_of(read, "EndArrowSize")), (end.0, end.1), "{after}");
        // The markers held are the numbers used, each once.
        let mut wanted = vec![begin.0 as u32, end.0 as u32];
        wanted.sort_unstable();
        wanted.dedup();
        assert_eq!(markers(&saved.bytes).len(), wanted.len());
    }
}

/// Reuse: two connectors with one end share one marker; a marker the file holds under the name with the form this save writes is reused, and the styles part
/// is not touched.
fn reuse_case(save: &Saver) {
    let mut o = open(&base());
    let (a, b) = (o.index.next_id(0).unwrap(), o.index.next_id(0).unwrap() + 1);
    push_shape(&mut o.doc, arrowed(a, None, Some((4.0, 2.0)), 0.75));
    push_shape(&mut o.doc, arrowed(b, Some((4.0, 2.0)), Some((4.0, 3.0)), 0.75));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(markers(&saved.bytes).len(), 1, "{:?}", markers(&saved.bytes));
    // Saved again with another shape that has the same end: the marker is held, and `styles.xml` is byte for byte what the first save made.
    let mut doc = o.doc.clone();
    push_shape(&mut doc, arrowed(b + 1, None, Some((4.0, 1.0)), 0.75));
    let second = save(&doc, &saved.bytes, &saved.index).unwrap();
    assert!(second.edited);
    assert_eq!(part(&second.bytes, "styles.xml"), part(&saved.bytes, "styles.xml"), "the marker is reused");
    assert_eq!(markers(&second.bytes).len(), 1);
}

const WITH_MARKER: &str = r#"<draw:marker draw:name="DiaCraft_20_end_20_4" draw:display-name="DiaCraft end 4" svg:viewBox="0 0 200 200" svg:d="M 100 0 L 200 200 L 0 200 Z"/>"#;
const OTHER_MARKER: &str = r#"<draw:marker draw:name="DiaCraft_20_end_20_4" svg:viewBox="0 0 20 30" svg:d="m10 0-10 30h20z"/>"#;

fn reuse_from_the_file_case(save: &Saver) {
    // The file holds the marker with the form the save writes: reused, and `styles.xml` is as it was.
    let mut o = open(&base().common(WITH_MARKER));
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, arrowed(id, None, Some((4.0, 2.0)), 0.75));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(part(&saved.bytes, "styles.xml"), part(&o.bytes, "styles.xml"), "nothing was added to `office:styles`");
    assert!(part(&saved.bytes, "content.xml").contains("draw:marker-end=\"DiaCraft_20_end_20_4\""));
    // The file holds a marker of that name that draws another form (a path and a box of another program's, a twin): it is DiaCraft's by its name alone (Ruling
    //), and is used as it is. Nothing is added to `office:styles`, and the end names it with the width the save works out.
    for held in [
        OTHER_MARKER.to_owned(),
        WITH_MARKER.replace("L 200 200", "L 100 100"),
        WITH_MARKER.replace("0 0 200 200", "0 0 200 300"),
        format!("{WITH_MARKER}{WITH_MARKER}"),
    ] {
        let mut o = open(&base().common(&held));
        let id = o.index.next_id(0).unwrap();
        push_shape(&mut o.doc, arrowed(id, None, Some((4.0, 2.0)), 0.75));
        let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{held}: {e}"));
        assert_eq!(part(&saved.bytes, "styles.xml"), part(&o.bytes, "styles.xml"), "{held}: nothing was added to `office:styles`");
        let after = part(&saved.bytes, "content.xml");
        assert!(after.contains("draw:marker-end=\"DiaCraft_20_end_20_4\"") && after.contains("draw:marker-end-width=\"6.54pt\""), "{held}: {after}");
    }
}

/// The two markers as LibreOffice writes them back after it saves our file: our names and display names, the path rewritten (`M100 0l100 200h-200z`) and the
/// disc's curves rounded to whole numbers.
const LO_4: &str =
    r#"<draw:marker draw:name="DiaCraft_20_end_20_4" draw:display-name="DiaCraft end 4" svg:viewBox="0 0 200 200" svg:d="M100 0l100 200h-200z"/>"#;
const LO_10: &str = r#"<draw:marker draw:name="DiaCraft_20_end_20_10" draw:display-name="DiaCraft end 10" svg:viewBox="0 0 150 150" svg:d="M75 0c41 0 75 34 75 75s-34 75-75 75-75-34-75-75 34-75 75-75z"/>"#;

/// A file that LibreOffice saved holds our markers in its own form; a line end of number 4 or 10 added to it, or changed on a connector of it, is written, and
/// its `styles.xml` is as it was (the review: the save refused it as "a marker that draws another form").
#[test]
fn line_ends_in_a_file_another_program_saved_are_written_and_the_markers_it_holds_are_used_as_they_are() {
    let held = format!("{LO_4}{LO_10}");
    // New connectors with ends 4 and 10, at the begin and at the end.
    let mut o = open(&base().common(&held));
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, arrowed(id, Some((10.0, 1.0)), Some((4.0, 2.0)), 0.75));
    push_shape(&mut o.doc, arrowed(id + 1, Some((4.0, 2.0)), Some((10.0, 2.0)), 0.75));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(part(&saved.bytes, "styles.xml"), part(&o.bytes, "styles.xml"), "the file's markers stand, and none is added");
    let back = open_bytes(saved.bytes.clone());
    let shapes = &back.doc.pages.get(0).unwrap().shapes;
    let read = |at: usize| {
        let s = shapes.get(at).unwrap();
        (num_of(s, "BeginArrow"), num_of(s, "BeginArrowSize"), num_of(s, "EndArrow"), num_of(s, "EndArrowSize"))
    };
    assert_eq!(read(1), (10.0, 1.0, 4.0, 2.0));
    assert_eq!(read(2), (4.0, 2.0, 10.0, 2.0));
    let again = real(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes, "saved again, it is no edit");
    // An existing connector of that file: its end changed from 4 to 10, and its weight changed.
    let mut o = open(&Drawing::a4().common(&held).auto(OWN_STYLES).page(&connectors(2)));
    end_of(&mut o, 0, 10.0, 2.0);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(part(&saved.bytes, "styles.xml"), part(&o.bytes, "styles.xml"));
    let id = o.doc.pages.get(0).unwrap().shapes.get(1).unwrap().id;
    set(shape_mut(&mut o.doc, 0, id), "LineWeight", 3.0 / 72.0);
    let saved = real(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(part(&saved.bytes, "styles.xml"), part(&o.bytes, "styles.xml"));
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains("draw:marker-end=\"DiaCraft_20_end_20_10\""), "{after}");
}

const CASES: [Case; 4] = [
    ("a_new_connector_with_an_end_writes_the_marker_and_its_width", end_arrow_case),
    ("both_ends_each_size_and_weight_are_written_and_read_back", two_ends_case),
    ("one_marker_serves_every_end_of_its_number", reuse_case),
    ("a_marker_the_file_holds_is_reused_as_it_is", reuse_from_the_file_case),
];

#[test]
fn a_new_connector_with_an_end_writes_the_marker_and_its_width() {
    end_arrow_case(&real);
}

#[test]
fn both_ends_each_size_and_weight_are_written_and_read_back() {
    two_ends_case(&real);
}

#[test]
fn one_marker_serves_every_end_of_its_number() {
    reuse_case(&real);
}

#[test]
fn a_marker_the_file_holds_is_reused_as_it_is() {
    reuse_from_the_file_case(&real);
}

#[test]
fn echo_control_fails_every_marker_case() {
    echo_control(&CASES);
}

/// A number that is no line end and a size outside 0 to 6 have no art: they are not written as a marker, their cells are tier 2, the file holds no marker
/// for them, and the cells read back as the model has them. (The custom end, 254, has number 4's marker and a carried cell: `write_line_ends`, V44.) Every number from 1 to 45 and every size from 0 to 6 is written as
/// a marker: there is no kind left that the writer has no art for.
#[test]
fn a_line_end_the_writer_has_no_art_for_is_carried_as_tier_2_and_no_marker_is_written() {
    let o = open(&base());
    let id = o.index.next_id(0).unwrap();
    for (n, size) in [(46.0, 2.0), (255.0, 2.0), (-1.0, 2.0), (4.5, 2.0), (4.0, 7.0), (4.0, 100.0), (4.0, 101.0), (4.0, 1.5), (14.0, 7.0)] {
        for end in [true, false] {
            let mut doc = o.doc.clone();
            let shape = if end { arrowed(id, None, Some((n, size)), 0.75) } else { arrowed(id, Some((n, size)), None, 0.75) };
            push_shape(&mut doc, shape);
            let saved = real(&doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{n} {size}: {e}"));
            let content = part(&saved.bytes, "content.xml");
            assert!(!content.contains("draw:marker-end=") && !content.contains("draw:marker-start="), "{n} {size}: no marker is written: {content}");
            let back = open_bytes(saved.bytes);
            let read = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap().clone();
            let cell = if end { "EndArrow" } else { "BeginArrow" };
            let value = read.sheet.cell(cell).and_then(|c| c.value.as_ref()).and_then(diacraft_doc::Value::number);
            assert_eq!(value, Some(n), "{n} {size} {cell}: the cell reads back");
        }
    }
    for n in 1..=45 {
        for size in 0..=6 {
            let mut doc = o.doc.clone();
            push_shape(&mut doc, arrowed(id, None, Some((f64::from(n), f64::from(size))), 0.75));
            real(&doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{n} at size {size}: {e}"));
        }
    }
}

/// A package with no `styles.xml`, and a `styles.xml` with no `office:styles`: refused in words.
#[test]
fn a_package_with_no_place_for_the_marker_is_refused_in_words() {
    let mut d = base();
    d.no_styles = true;
    let mut o = open(&d);
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, arrowed(id, None, Some((4.0, 2.0)), 0.75));
    assert!(matches!(real(&o.doc, &o.bytes, &o.index), Err(WriteError::Unsupported(what)) if what.contains("styles.xml")));
    // `styles.xml` with no `office:styles`.
    let d = base();
    let files: Vec<(String, Vec<u8>)> =
        d.files()
            .into_iter()
            .map(|(n, b)| {
                if n == "styles.xml" {
                    (n, String::from_utf8(b).unwrap().replace("<office:styles></office:styles>", "").into_bytes())
                } else {
                    (n, b)
                }
            })
            .collect();
    let mut o = crate::write_core::open_bytes(package(DRAWING, files));
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, arrowed(id, None, Some((4.0, 2.0)), 0.75));
    assert!(matches!(real(&o.doc, &o.bytes, &o.index), Err(WriteError::Unsupported(what)) if what.contains("office:styles")));
}

/// `office:styles` written as an empty element is opened for the marker, and the declared prefixes of the file are the ones the marker is written under.
#[test]
fn the_marker_goes_into_an_empty_office_styles_and_under_the_prefixes_the_part_binds() {
    let d = base();
    let files: Vec<(String, Vec<u8>)> = d
        .files()
        .into_iter()
        .map(|(n, b)| {
            if n == "styles.xml" {
                (n, String::from_utf8(b).unwrap().replace("<office:styles></office:styles>", "<office:styles/>").into_bytes())
            } else {
                (n, b)
            }
        })
        .collect();
    let mut o = crate::write_core::open_bytes(package(DRAWING, files));
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, arrowed(id, None, Some((4.0, 2.0)), 0.75));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let styles = part(&saved.bytes, "styles.xml");
    assert!(styles.contains("<office:styles><draw:marker draw:name=\"DiaCraft_20_end_20_4\""), "{styles}");
    assert!(styles.contains("</draw:marker>") || styles.contains("Z\"/></office:styles>"), "{styles}");
    assert_eq!(markers(&saved.bytes).len(), 1);
    // A part that binds `draw` and `svg` to other prefixes is written under those.
    let renamed: Vec<(String, Vec<u8>)> = d
        .files()
        .into_iter()
        .map(|(n, b)| {
            if n == "styles.xml" {
                let s = String::from_utf8(b).unwrap().replace("xmlns:draw=", "xmlns:dw=").replace("xmlns:svg=", "xmlns:sv=");
                (n, s.replace("draw:layer-set", "dw:layer-set").replace("draw:layer ", "dw:layer ").into_bytes())
            } else {
                (n, b)
            }
        })
        .collect();
    let mut o = crate::write_core::open_bytes(package(DRAWING, renamed));
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, arrowed(id, None, Some((4.0, 2.0)), 0.75));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let styles = part(&saved.bytes, "styles.xml");
    assert!(styles.contains("<dw:marker dw:name=\"DiaCraft_20_end_20_4\"") && styles.contains(" sv:viewBox="), "{styles}");
    let back = open_bytes(saved.bytes);
    assert_eq!(num_of(back.doc.pages.get(0).unwrap().shapes.get(1).unwrap(), "EndArrow"), 4.0);
}

const FILE_STYLES: &str = r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:marker-end="Arrow" draw:marker-end-width="0.3cm" svg:stroke-width="0.05cm" draw:stroke="solid"/></style:style>"#;
const FOREIGN: &str = r#"<draw:marker draw:name="Arrow" svg:viewBox="0 0 20 30" svg:d="m10 0-10 30h20z"/>"#;

fn connectors(count: usize) -> String {
    (1..=count)
        .map(|k| {
            format!(
                "<draw:connector draw:name=\"c{k}\" draw:style-name=\"gr1\" draw:layer=\"layout\" svg:x1=\"1cm\" svg:y1=\"{k}cm\" svg:x2=\"6cm\" svg:y2=\"{k}cm\" svg:d=\"M 0 0 L 5000 0\" svg:viewBox=\"0 0 5000 1\"><text:p/></draw:connector>"
            )
        })
        .collect()
}

fn existing(count: usize) -> Open {
    open(&Drawing::a4().common(FOREIGN).auto(FILE_STYLES).page(&connectors(count)))
}

/// The cells of the ends of an existing connector set (`EndArrow`, `EndArrowSize`), and nothing else touched.
fn end_of(o: &mut Open, which: usize, n: f64, size: f64) {
    let id = o.doc.pages.get(0).unwrap().shapes.get(which).unwrap().id;
    let s = shape_mut(&mut o.doc, 0, id);
    set(s, "EndArrow", n);
    set(s, "EndArrowSize", size);
}

/// An edit of an arrow cell of an existing shape goes through the restyle rules: a style two connectors name is cloned for the one edited, the other keeps
/// its marker and the style, the foreign marker of the file stays, and DiaCraft's is added.
#[test]
fn an_arrow_edit_of_an_existing_connector_clones_the_shared_style_and_leaves_the_other_alone() {
    let mut o = existing(2);
    // The reader gives the foreign marker's stand-in.
    assert_eq!(num_of(o.doc.pages.get(0).unwrap().shapes.get(0).unwrap(), "EndArrow"), 4.0);
    end_of(&mut o, 0, 13.0, 3.0);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    assert!(after.contains(FILE_STYLES), "gr1 is as it was, for the other connector: {after}");
    assert!(after.contains("draw:name=\"c2\" draw:style-name=\"gr1\""), "{after}");
    assert!(after.contains("draw:name=\"c1\" draw:style-name=\"gr2\""), "the first has the clone: {after}");
    let clone = after.split("style:name=\"gr2\"").nth(1).unwrap().split("</style:style>").next().unwrap();
    assert!(clone.contains("draw:marker-end=\"DiaCraft_20_end_20_13\""), "{clone}");
    // Class 3 at 0.05 cm (1.417 pt): 2 x (3.24 + 1.417).
    let width = attr(clone, "draw:marker-end-width").unwrap();
    assert!((points(&width) - 2.0 * (3.24 + 0.05 / 2.54 * 72.0)).abs() < 0.01, "{width}");
    assert_ne!(before, after);
    let styles = part(&saved.bytes, "styles.xml");
    assert!(styles.contains(FOREIGN) && styles.contains("DiaCraft_20_end_20_13"), "{styles}");
    let back = open_bytes(saved.bytes.clone());
    let read = back.doc.pages.get(0).unwrap().shapes.get(0).unwrap();
    assert_eq!((num_of(read, "EndArrow"), num_of(read, "EndArrowSize")), (13.0, 3.0));
    assert_eq!(num_of(back.doc.pages.get(0).unwrap().shapes.get(1).unwrap(), "EndArrow"), 4.0, "the other is as it was");
    // A size alone is the width again.
    let mut o = existing(1);
    let mut sized = o.doc.clone();
    {
        let id = sized.pages.get(0).unwrap().shapes.get(0).unwrap().id;
        let s = shape_mut(&mut sized, 0, id);
        set(s, "EndArrowSize", 4.0);
    }
    o.doc = sized;
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    let width = attr(&after, "draw:marker-end-width").unwrap();
    assert!((points(&width) - 2.0 * (3.96 + 0.05 / 2.54 * 72.0)).abs() < 0.01, "{width}");
    // The style the only connector names is edited where it stands, and its orphan is not left: the foreign marker is `Arrow`'s, a common element.
    assert!(!after.contains("style:name=\"gr2\""), "{after}");
}

/// A weight alone leaves a foreign marker and its width as the file has them (kept while the arrow and size cells are unchanged).
#[test]
fn a_weight_edit_leaves_the_foreign_marker_of_the_file_as_it_is() {
    let mut o = existing(1);
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    set(shape_mut(&mut o.doc, 0, id), "LineWeight", cm(0.1));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains("draw:marker-end=\"Arrow\"") && after.contains("draw:marker-end-width=\"0.3cm\""), "{after}");
    assert_eq!(part(&saved.bytes, "styles.xml"), part(&o.bytes, "styles.xml"));
}

/// An end taken away writes the empty name, so that the style draws no end whatever its parent does, and no width or centre.
#[test]
fn an_end_set_to_none_writes_the_empty_marker_name() {
    let mut o = existing(1);
    end_of(&mut o, 0, 0.0, 2.0);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    assert_eq!(attr(&after, "draw:marker-end").as_deref(), Some(""), "{after}");
    assert!(!after.contains("marker-end-width"), "{after}");
    let back = open_bytes(saved.bytes);
    assert_eq!(num_of(back.doc.pages.get(0).unwrap().shapes.get(0).unwrap(), "EndArrow"), 0.0);
}

/// Many ends of many numbers in one save: each number's marker is written once, however many shapes name it, in the order of the numbers.
#[test]
fn many_new_connectors_share_the_markers_of_their_numbers() {
    let mut o = open(&base());
    let first = o.index.next_id(0).unwrap();
    for (k, id) in (0..90u32).zip(first..) {
        let n = [2.0, 4.0, 5.0, 6.0, 10.0, 11.0, 13.0, 39.0, 42.0][(k % 9) as usize];
        push_shape(&mut o.doc, arrowed(id, Some((n, f64::from(k % 6))), Some((4.0, 2.0)), 0.75));
    }
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let held = markers(&saved.bytes);
    assert_eq!(held.len(), 9, "nine numbers, nine markers: {held:?}");
    let names: Vec<&str> = held.iter().map(|m| attr(m, "draw:name").unwrap().leak() as &str).collect();
    let numbers: Vec<u32> = names.iter().map(|n| n.trim_start_matches("DiaCraft_20_end_20_").parse().unwrap()).collect();
    assert!(numbers.windows(2).all(|w| w[0] < w[1]), "in the order of the numbers: {numbers:?}");
    let back = open_bytes(saved.bytes);
    assert_eq!(back.doc.pages.get(0).unwrap().shapes.len(), 91);
    // A rectangle (no end) with a line end cell the writer does not write for a 2-D shape is not asked: no marker for it.
    let mut o = open(&base());
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, rectangle(id, "R", (2.0, 3.0, 4.0, 2.0)));
    assert_eq!(markers(&real(&o.doc, &o.bytes, &o.index).unwrap().bytes).len(), 0);
}

const OWN_STYLES: &str = r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:marker-start="DiaCraft_20_end_20_10" draw:marker-start-width="3.825pt" draw:marker-start-center="true" draw:marker-end="DiaCraft_20_end_20_4" draw:marker-end-width="6.54pt" svg:stroke-width="0.75pt" draw:stroke="solid"/></style:style>"#;

/// The line's weight changed on a connector whose style names markers of DiaCraft's own: each width is written again for the new weight (the weight feeds the
/// width), so that the size class read back is the class the model has; a style two connectors name is cloned for the one edited and the other keeps its widths.
#[test]
fn a_weight_edit_writes_the_widths_of_the_markers_of_the_style_again_and_the_class_reads_back() {
    let markers = format!(
        "{WITH_MARKER}{}",
        r#"<draw:marker draw:name="DiaCraft_20_end_20_10" draw:display-name="DiaCraft end 10" svg:viewBox="0 0 150 150" svg:d="M 75 0 C 116.42 0 150 33.58 150 75 C 150 116.42 116.42 150 75 150 C 33.58 150 0 116.42 0 75 C 0 33.58 33.58 0 75 0 Z"/>"#
    );
    let mut o = open(&Drawing::a4().common(&markers).auto(OWN_STYLES).page(&connectors(2)));
    let first = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap();
    // The reader finds the class of each end from the width and the weight.
    assert_eq!((num_of(first, "BeginArrow"), num_of(first, "BeginArrowSize")), (10.0, 1.0), "3.825 = 1.5 x (1.8 + 0.75): class 1");
    assert_eq!((num_of(first, "EndArrow"), num_of(first, "EndArrowSize")), (4.0, 2.0));
    let id = first.id;
    set(shape_mut(&mut o.doc, 0, id), "LineWeight", 3.0 / 72.0);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    let clone = after.split("style:name=\"gr2\"").nth(1).unwrap().split("</style:style>").next().unwrap();
    // Class 1 begin: 1.5 x (1.8 + 3) = 7.2 pt; class 2 end: 2 x (2.52 + 3) = 11.04 pt; the marker names and the centre are as they were.
    assert!(clone.contains("draw:marker-end-width=\"11.04pt\"") && clone.contains("draw:marker-start-width=\"7.2pt\""), "{clone}");
    assert!(clone.contains("draw:marker-end=\"DiaCraft_20_end_20_4\"") && clone.contains("draw:marker-start-center=\"true\""), "{clone}");
    assert!(after.contains(OWN_STYLES), "the style the other connector names is as it was: {after}");
    let back = open_bytes(saved.bytes.clone());
    let read = back.doc.pages.get(0).unwrap().shapes.get(0).unwrap();
    assert_eq!((num_of(read, "BeginArrow"), num_of(read, "BeginArrowSize")), (10.0, 1.0));
    assert_eq!((num_of(read, "EndArrow"), num_of(read, "EndArrowSize")), (4.0, 2.0));
    close_to(num_of(read, "LineWeight"), 3.0 / 72.0);
    // An edit of one end's own cells leaves the other end's width as it is when the weight is not changed.
    let mut o = open(&Drawing::a4().common(&markers).auto(OWN_STYLES).page(&connectors(1)));
    end_of(&mut o, 0, 4.0, 3.0);
    let after = part(&real(&o.doc, &o.bytes, &o.index).unwrap().bytes, "content.xml");
    assert!(after.contains("draw:marker-start-width=\"3.825pt\""), "{after}");
}

fn close_to(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} is not {b}");
}

/// The weight and a line end of the same connector changed in one save: the width of the end that changed is the new mark's at the new weight, and the width
/// the weight alone would have written for the mark the file named before is not written after it.
#[test]
fn a_weight_and_a_line_end_changed_together_write_the_width_of_the_new_end() {
    let markers = format!(
        "{WITH_MARKER}{}",
        r#"<draw:marker draw:name="DiaCraft_20_end_20_10" draw:display-name="DiaCraft end 10" svg:viewBox="0 0 150 150" svg:d="M 75 0 C 116.42 0 150 33.58 150 75 C 150 116.42 116.42 150 75 150 C 33.58 150 0 116.42 0 75 C 0 33.58 33.58 0 75 0 Z"/>"#
    );
    let mut o = open(&Drawing::a4().common(&markers).auto(OWN_STYLES).page(&connectors(1)));
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    {
        let shape = shape_mut(&mut o.doc, 0, id);
        set(shape, "LineWeight", 3.0 / 72.0);
        set(shape, "BeginArrow", 4.0);
        set(shape, "BeginArrowSize", 3.0);
    }
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    // Number 4: 2 x (3.24 + 3) = 12.48 pt, a triangle that is not centred; and not number 10's 1.5 x (3.24 + 3) = 9.36 pt that the file named before.
    assert!(after.contains("draw:marker-start=\"DiaCraft_20_end_20_4\"") && after.contains("draw:marker-start-width=\"12.48pt\""), "{after}");
    assert!(!after.contains("9.36pt") && !after.contains("draw:marker-start-center"), "{after}");
    // The end the edit did not name is written for the new weight: number 4, class 2: 2 x (2.52 + 3) = 11.04 pt.
    assert!(after.contains("draw:marker-end-width=\"11.04pt\""), "{after}");
    let back = open_bytes(saved.bytes);
    let read = back.doc.pages.get(0).unwrap().shapes.get(0).unwrap();
    assert_eq!((num_of(read, "BeginArrow"), num_of(read, "BeginArrowSize")), (4.0, 3.0));
    assert_eq!((num_of(read, "EndArrow"), num_of(read, "EndArrowSize")), (4.0, 2.0));
}
