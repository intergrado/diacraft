//! A flat document is saved as a package is, with what added to the writer (on the merge of main):
//! the same edit made to a package and to the flat document of the same pieces, and saved by each
//! form's own save, writes **the same line-end markers and the same paragraphs**. The oracle is the package writer's own output for the same model, which those
//! tasks measured against the specification's vectors.

use std::sync::Arc;

use diacraft_doc::TextPart;
use diacraft_odg::{SaveNote, Written};

use crate::drawing::Drawing;
use crate::flat::Flat;
use crate::flat_write::{save, second_save_is_no_edit, text_of};
use crate::write_core::{Open, num_of, open, open_bytes, part};
use crate::write_line_ends::{FOREIGN_ARROW, MARKER_T23, MARKER_T44, TWO_STYLES, two_connectors};
use crate::write_markers::{arrowed, attr, base, markers};
use crate::write_new::push_shape;
use crate::write_runs::{chars, chars_of, set_parts, set_row, shape, the_shape};

/// The `draw:marker` elements of a flat document, as `markers` gives those of a package's `styles.xml`.
fn flat_markers(bytes: &[u8]) -> Vec<String> {
    text_of(bytes).split("<draw:marker ").skip(1).map(|m| m.split("/>").next().unwrap_or_default().to_owned()).collect()
}

/// A saved package and a saved flat document of one drawing: the same edit, each saved by its form's save, and each saved again as no edit.
fn pair(d: &Drawing, edit: &dyn Fn(&mut Open)) -> (Open, Written, Open, Written) {
    let (mut package, mut flat) = (open(d), open_bytes(Flat::of(d).bytes()));
    edit(&mut package);
    edit(&mut flat);
    let p = save(&package.doc, &package.bytes, &package.index).unwrap_or_else(|e| panic!("the package: {e} ({})", e.class()));
    let f = save(&flat.doc, &flat.bytes, &flat.index).unwrap_or_else(|e| panic!("the flat document: {e} ({})", e.class()));
    assert!(p.edited && f.edited);
    second_save_is_no_edit(&package.doc, &p);
    second_save_is_no_edit(&flat.doc, &f);
    (package, p, flat, f)
}

/// The line ends a new connector is given, read back from a saved file: the number and the size class of its begin and of its end.
fn read_ends(bytes: &[u8]) -> [f64; 4] {
    let back = open_bytes(bytes.to_vec());
    let s = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap();
    [num_of(s, "BeginArrow"), num_of(s, "BeginArrowSize"), num_of(s, "EndArrow"), num_of(s, "EndArrowSize")]
}

/// A new connector with line ends is saved with DiaCraft's markers in `office:styles`, named by number and band class, drawn as the
/// package's are, and the style names them with the widths the package writes. Without an `office:styles` the flat document is given one at its place; with one,
/// the markers go into it.
#[test]
fn a_new_line_end_is_written_into_a_flat_document_as_the_package_writes_it() {
    type End = Option<(f64, f64)>;
    let cases: [(&str, End, End, f64); 6] = [
        ("a hollow triangle, thin", None, Some((16.0, 2.0)), 0.75),
        ("the same on a heavy line", None, Some((16.0, 2.0)), 5.0),
        ("a crossbar and the spine", None, Some((24.0, 2.0)), 0.75),
        ("a begin and an end", Some((13.0, 3.0)), Some((14.0, 2.0)), 0.75),
        ("a number with no band class, the largest size class", None, Some((2.0, 6.0)), 0.75),
        ("the last number", None, Some((45.0, 1.0)), 2.0),
    ];
    for with_styles in [false, true] {
        for (what, begin, end, weight) in &cases {
            let mut d = base();
            if with_styles {
                d = d.common(r#"<style:style style:name="Standard" style:family="graphic"/>"#);
            }
            let edit = |o: &mut Open| {
                let id = o.index.next_id(0).unwrap();
                push_shape(&mut o.doc, arrowed(id, *begin, *end, *weight));
            };
            let (_, p, _, f) = pair(&d, &edit);
            let who = format!("{what} (office:styles in the file: {with_styles})");
            let (wanted, got) = (markers(&p.bytes), flat_markers(&f.bytes));
            assert!(!wanted.is_empty(), "{who}: the package writes a marker");
            assert_eq!(wanted, got, "{who}: the markers of the flat document are the package's");
            let (content, flat) = (part(&p.bytes, "content.xml"), text_of(&f.bytes));
            for name in ["draw:marker-end", "draw:marker-end-width", "draw:marker-start", "draw:marker-start-width", "draw:marker-end-center"] {
                assert_eq!(attr(&flat, name), attr(&content, name), "{who}: {name}");
            }
            assert!(flat.contains("<office:styles>"), "{who}: the markers have their container");
            assert!(flat.find("<office:styles>").unwrap() < flat.find("<office:automatic-styles>").unwrap(), "{who}: in the order");
            let asked = [begin.map_or(0.0, |b| b.0), begin.map_or(0.0, |b| b.1), end.map_or(0.0, |e| e.0), end.map_or(0.0, |e| e.1)];
            assert_eq!(read_ends(&f.bytes), read_ends(&p.bytes), "{who}: read back as the package's");
            if begin.is_none() {
                assert_eq!(&read_ends(&f.bytes)[2..], &asked[2..], "{who}: the end reads back as it was asked");
            } else {
                assert_eq!(read_ends(&f.bytes), asked, "{who}: both ends read back as they were asked");
            }
        }
    }
}

/// Two lines with markers `t23` and `t44`; the second line is deleted and the markers of DiaCraft's name that no style names go: `t44` is taken
/// out of `office:styles` of the flat document as it is of `styles.xml`, with the white space that was its own, and another producer's marker is not.
#[test]
fn a_marker_no_style_names_after_the_save_is_taken_out_of_a_flat_document_and_another_producers_is_not() {
    let common = format!("{FOREIGN_ARROW}{MARKER_T23}{MARKER_T44}");
    let d = Drawing::a4().common(&common).auto(TWO_STYLES).page(&two_connectors());
    let (_, p, _, f) = pair(&d, &|o| {
        Arc::make_mut(o.doc.pages.get_mut(0).unwrap()).shapes.remove(1);
    });
    let names = |markers: Vec<String>| -> Vec<String> { markers.iter().map(|m| attr(m, "draw:name").unwrap()).collect() };
    assert_eq!(names(markers(&p.bytes)), ["Arrow", "DiaCraft_20_end_20_14_20_t23"], "the package");
    assert_eq!(names(flat_markers(&f.bytes)), ["Arrow", "DiaCraft_20_end_20_14_20_t23"], "the flat document");
    let flat = text_of(&f.bytes);
    assert!(flat.contains(FOREIGN_ARROW) && flat.contains(MARKER_T23), "the others are as they were");
    assert!(flat.contains("draw:marker-end=\"DiaCraft_20_end_20_14_20_t23\"") && !flat.contains("t44"), "{flat}");
    // A save that touches a line and orphans nothing leaves a marker no style names where it is.
    let mut o = open_bytes(Flat::of(&Drawing::a4().common(&format!("{MARKER_T23}{MARKER_T44}")).auto(TWO_STYLES).page(&two_connectors())).bytes());
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    crate::write_core::set_num(crate::write_core::shape_mut(&mut o.doc, 0, id), "LinePattern", 0.0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(saved.edited && flat_markers(&saved.bytes).len() == 2, "{}", text_of(&saved.bytes));
    // A flat document whose `office:styles` is made for a marker and whose marker is then orphaned by a later save, in two steps: the second save takes it out.
    let mut o = open_bytes(Flat::of(&base()).bytes());
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, arrowed(id, None, Some((14.0, 2.0)), 0.75));
    let first = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(flat_markers(&first.bytes).len(), 1);
    let mut again = open_bytes(first.bytes.clone());
    let n = again.doc.pages.get(0).unwrap().shapes.len();
    Arc::make_mut(again.doc.pages.get_mut(0).unwrap()).shapes.remove(n - 1);
    let second = save(&again.doc, &again.bytes, &again.index).unwrap();
    assert!(flat_markers(&second.bytes).is_empty(), "the last line that named it is gone, and so is the marker: {}", text_of(&second.bytes));
    second_save_is_no_edit(&again.doc, &second);
}

const T1: &str = r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"#;
const AUTHOR: &str = r#"<text:author-name text:fixed="true">Ann</text:author-name>"#;

/// What is between the start tag of the first custom shape and its geometry: the paragraphs the writer wrote.
fn paragraphs_of(text: &str) -> String {
    let from = text.find("<draw:custom-shape").and_then(|k| text[k..].find('>').map(|e| k + e + 1)).unwrap_or_else(|| panic!("no shape: {text}"));
    let to = text.find("<draw:enhanced-geometry").unwrap_or_else(|| panic!("no geometry: {text}"));
    text[from..to].to_owned()
}

/// One retext of a paragraph that holds zero-width elements, saved as a package and as a flat document: the paragraphs written are the same bytes, they read back as
/// the model's characters, the check of the bytes made looked at the same paragraphs, the notes are the same, and `expected` is what both hold.
fn zero_case(what: &str, auto: &str, inner: &str, edit: &dyn Fn(&mut Open), expected: &str) -> Vec<SaveNote> {
    let d = Drawing::a4().auto(auto).page(&shape("x", "1cm", inner));
    let (package, p, flat, f) = pair(&d, edit);
    let (wanted, got) = (paragraphs_of(&part(&p.bytes, "content.xml")), paragraphs_of(&text_of(&f.bytes)));
    assert_eq!(wanted, expected, "{what}: the package writes what the vector says");
    assert_eq!(got, wanted, "{what}: the flat document's paragraphs are the package's");
    assert_eq!(f.work.checked_zero, p.work.checked_zero, "{what}: the same paragraphs were read back for their elements");
    assert_eq!(format!("{:?}", f.notes), format!("{:?}", p.notes), "{what}: the same notes");
    let back = open_bytes(f.bytes.clone());
    assert_eq!(chars_of(the_shape(&back.doc)), chars_of(the_shape(&flat.doc)), "{what}: the file reads as the model's characters");
    assert_eq!(chars_of(the_shape(&back.doc)), chars_of(the_shape(&package.doc)), "{what}: and the package's");
    f.notes
}

fn bold(o: &mut Open, ix: u32) {
    set_row(o, "Character", ix, &[("Style", 1.0)]);
}

/// A bookmark, a reference mark, a soft page break and a field stay in a paragraph written whole, at the offset the model's text gives them, in a
/// flat document as in a package; a field whose characters changed is written as plain text with one note.
#[test]
fn zero_width_elements_stay_in_a_paragraph_a_flat_document_writes_whole_as_they_do_in_a_package() {
    // A mark between two runs stands at the top level, before the bold run.
    zero_case(
        "V14",
        "",
        r#"<text:p>Alpha <text:bookmark text:name="B1"/>beta gamma</text:p>"#,
        &|o| {
            bold(o, 1);
            set_parts(o, vec![chars("Alpha "), TextPart::Cp(1), chars("beta"), TextPart::Cp(0), chars(" gamma")]);
        },
        r#"<text:p>Alpha <text:bookmark text:name="B1"/><text:span text:style-name="T1">beta</text:span> gamma</text:p>"#,
    );
    // A mark inside a run stays inside its span.
    zero_case(
        "V15",
        T1,
        r#"<text:p><text:span text:style-name="T1">gam<text:bookmark text:name="B2"/>ma</text:span> delta</text:p>"#,
        &|o| {
            set_row(o, "Character", 2, &[("Size", 0.1666667), ("Style", 2.0)]);
            set_parts(o, vec![TextPart::Cp(0), chars("gamma"), TextPart::Cp(2), chars(" delta")]);
        },
        r#"<text:p><text:span text:style-name="T1">gam<text:bookmark text:name="B2"/>ma</text:span><text:span text:style-name="T2"> delta</text:span></text:p>"#,
    );
    // A mark in the text an edit removed goes to the end of the common prefix.
    zero_case(
        "V16",
        "",
        r#"<text:p>alpha beta<text:reference-mark text:name="M1"/> gamma</text:p>"#,
        &|o| {
            bold(o, 1);
            set_parts(o, vec![chars("alpha "), TextPart::Cp(1), chars("gamma")]);
        },
        r#"<text:p>alpha <text:reference-mark text:name="M1"/><text:span text:style-name="T1">gamma</text:span></text:p>"#,
    );
    // A range whose start's paragraph goes has its start put at the start of the next paragraph kept.
    zero_case(
        "V17",
        "",
        r#"<text:p>start <text:bookmark-start text:name="R2"/>here</text:p><text:p>end<text:bookmark-end text:name="R2"/> there</text:p>"#,
        &|o| set_parts(o, vec![chars("end there")]),
        r#"<text:p><text:bookmark-start text:name="R2"/>end<text:bookmark-end text:name="R2"/> there</text:p>"#,
    );
    // A plain space after a mark is `text:s`.
    zero_case(
        "V18",
        "",
        r#"<text:p>a <text:bookmark text:name="B3"/><text:s/>b</text:p>"#,
        &|o| {
            bold(o, 1);
            set_parts(o, vec![chars("a  "), TextPart::Cp(1), chars("b")]);
        },
        r#"<text:p>a <text:bookmark text:name="B3"/><text:s/><text:span text:style-name="T1">b</text:span></text:p>"#,
    );
    // A kept field keeps its bytes; a field whose characters changed is plain text, with one note that names it.
    let notes = zero_case(
        "V19, kept",
        "",
        &format!("<text:p>by {AUTHOR} today</text:p>"),
        &|o| {
            bold(o, 1);
            set_parts(o, vec![chars("by Ann "), TextPart::Cp(1), chars("today")]);
        },
        &format!(r#"<text:p>by {AUTHOR}<text:s/><text:span text:style-name="T1">today</text:span></text:p>"#),
    );
    assert!(notes.is_empty(), "{notes:?}");
    let notes = zero_case(
        "V19, changed",
        "",
        &format!("<text:p>by {AUTHOR} today</text:p>"),
        &|o| set_parts(o, vec![chars("by Bob today")]),
        "<text:p>by Bob today</text:p>",
    );
    assert!(matches!(notes.as_slice(), [SaveNote::FieldWritten { element: "author-name", .. }]), "{notes:?}");
    // A range that collapses keeps its order.
    zero_case(
        "V20",
        "",
        r#"<text:p>one <text:bookmark-start text:name="R1"/>two<text:bookmark-end text:name="R1"/> four</text:p>"#,
        &|o| {
            bold(o, 1);
            set_parts(o, vec![chars("one "), TextPart::Cp(1), chars("four")]);
        },
        r#"<text:p>one <text:bookmark-start text:name="R1"/><text:bookmark-end text:name="R1"/><text:span text:style-name="T1">four</text:span></text:p>"#,
    );
    // The prefix is taken before the suffix.
    zero_case(
        "V21",
        "",
        r#"<text:p>one two<text:bookmark text:name="B4"/> three</text:p>"#,
        &|o| {
            bold(o, 1);
            set_parts(o, vec![chars("one "), TextPart::Cp(1), chars("three")]);
        },
        r#"<text:p>one <text:span text:style-name="T1">t<text:bookmark text:name="B4"/>hree</text:span></text:p>"#,
    );
    // R10 items 1 and 5: a soft page break between paragraphs stays with the paragraph after it.
    let soft = "<text:soft-page-break/>";
    zero_case(
        "a soft page break",
        "",
        &format!("<text:p>one</text:p>{soft}<text:p>two</text:p>"),
        &|o| {
            bold(o, 1);
            set_parts(o, vec![chars("one\n"), TextPart::Cp(1), chars("two")]);
        },
        &format!(r#"<text:p>one</text:p>{soft}<text:p><text:span text:style-name="T1">two</text:span></text:p>"#),
    );
    zero_case(
        "a paragraph put in before a break",
        "",
        &format!("{soft}<text:p>one</text:p>"),
        &|o| set_parts(o, vec![chars("new\none")]),
        &format!("<text:p>new</text:p>{soft}<text:p>one</text:p>"),
    );
}
