//! The content check for VSDX packages (`diacraft_vsdx::compare_content`): the planted differences and the planted reformatting of the design's
//! section 4.3 on real packages, the echo control, and Review Focus 3: no attribute that holds text is number-typed, so `007` becomes `7` as a
//! difference. Every package is built in code from the specs (`diacraft-testkit`), and none is a file anyone made with another program.

use std::io::Write;

use diacraft_container::Work;
use diacraft_container::canon::{ContentDiff, Difference, Found};
use diacraft_container::zip::Archive;
use diacraft_testkit::package::{PageSpec, Pkg, VsdxBuilder, cell, cell_u, shape};
use diacraft_vsdx::{CANON_RULES, Limits, compare_content, compare_content_counted};

/// The folder of the format's parts, the names in it, and the namespaces, written in pieces.
const DIR: &str = concat!("vi", "sio");
const PAGE1: &str = concat!("/vi", "sio", "/pages/page1.xml");
const IMAGE1: &str = concat!("/vi", "sio", "/media/image1.png");
const EXTRA_A: &str = concat!("/vi", "sio", "/extra/a.bin");
const EXTRA_B: &str = concat!("/vi", "sio", "/extra/b.bin");
const REL_A: &str = concat!("vi", "sio", "/extra/a.bin");
const REL_B: &str = concat!("vi", "sio", "/extra/b.bin");
const EXTRA_P: &str = concat!("/vi", "sio", "/extra/p.dat");
const NS_2012_TEXT: &str = concat!("http://schemas.microsoft.com/office/vi", "sio", "/2012/main");
const NS_2011_TEXT: &str = concat!("http://schemas.microsoft.com/office/vi", "sio", "/2011/1/core");
const PNG: [u8; 12] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4];

/// A package of one page per entry of `pages`, each holding that XML as its shapes.
fn package(pages: &[&str]) -> Vec<u8> {
    let mut builder = VsdxBuilder::new();
    for (i, shapes) in pages.iter().enumerate() {
        builder = builder.page(PageSpec::new(u32::try_from(i + 1).unwrap(), &format!("Page-{}", i + 1)).shapes(shapes));
    }
    builder.build()
}

fn diff(a: &[u8], b: &[u8]) -> ContentDiff {
    let (ab, ba) = (compare_content(a, b, &Limits::default()).unwrap(), compare_content(b, a, &Limits::default()).unwrap());
    assert_eq!(ab.count, ba.count, "the count is symmetric");
    ab
}

/// The comparison of two one-page packages.
fn on_page(a: &str, b: &str) -> ContentDiff {
    diff(&package(&[a]), &package(&[b]))
}

fn only(d: &ContentDiff, found: Found, path: &str) {
    assert_eq!(d.count, 1, "{:?}", d.shown);
    assert_eq!(d.shown, [Difference { part: PAGE1.to_owned(), path: path.to_owned(), found }]);
}

fn two_cells(second: &str) -> String {
    shape(1, &format!("{}{}", cell("PinX", "1"), cell("PinY", second)))
}

// ------------------------------------------------------------------------------------------------------ planted differences fail

#[test]
fn a_changed_value_fails() {
    let d = on_page(&two_cells("2"), &two_cells("3"));
    only(&d, Found::Value, "/PageContents[0]/Shapes[0]/Shape[0]/Cell#1/@V");
}

#[test]
fn a_missing_attribute_fails() {
    let (a, b) = (shape(1, &cell_u("PinX", "1", "IN")), shape(1, &cell("PinX", "1")));
    only(&on_page(&a, &b), Found::Attribute, "/PageContents[0]/Shapes[0]/Shape[0]/Cell#0/@U");
}

#[test]
fn an_extra_element_fails() {
    let (a, b) = (shape(1, &cell("PinX", "1")), format!("{}{}", shape(1, &cell("PinX", "1")), shape(2, "")));
    only(&on_page(&a, &b), Found::Element, "/PageContents[0]/Shapes[0]/Shape[1]");
    // A cell is one too: an extra keyed child, found by its key.
    only(
        &on_page(&shape(1, &cell("PinX", "1")), &shape(1, &format!("{}{}", cell("PinX", "1"), cell("PinY", "1")))),
        Found::Element,
        "/PageContents[0]/Shapes[0]/Shape[0]/Cell#1",
    );
}

#[test]
fn two_ordered_siblings_swapped_fail() {
    // Two Shape elements: their order is the drawing's z-order.
    let (a, b) = (format!("{}{}", shape(1, ""), shape(2, "")), format!("{}{}", shape(2, ""), shape(1, "")));
    only(&on_page(&a, &b), Found::Order, "/PageContents[0]/Shapes[0]");
    // Two Row elements of a geometry section: the order of the path.
    let rows = |first: &str, second: &str| shape(1, &format!("<Section N=\"Geometry\" IX=\"0\">{first}{second}</Section>"));
    let (move_to, line_to) = ("<Row T=\"MoveTo\" IX=\"1\"><Cell N=\"X\" V=\"0\"/></Row>", "<Row T=\"LineTo\" IX=\"2\"><Cell N=\"X\" V=\"1\"/></Row>");
    only(&on_page(&rows(move_to, line_to), &rows(line_to, move_to)), Found::Order, "/PageContents[0]/Shapes[0]/Shape[0]/Section[0]");
}

#[test]
fn a_changed_picture_byte_fails() {
    let with = |bytes: &[u8]| {
        let pkg = Pkg::new().default_type("png", "image/png").part_without_override(IMAGE1, bytes);
        VsdxBuilder::new().with(pkg).build()
    };
    let mut changed = PNG;
    changed[10] = 9;
    let d = diff(&with(&PNG), &with(&changed));
    assert_eq!(d.count, 1, "{:?}", d.shown);
    assert_eq!(d.shown, [Difference { part: IMAGE1.to_owned(), path: String::new(), found: Found::Bytes }]);
    assert!(diff(&with(&PNG), &with(&PNG)).same());
}

#[test]
fn a_dropped_part_fails() {
    let with = |include: bool| {
        let mut pkg = Pkg::new().default_type("png", "image/png");
        if include {
            pkg = pkg.part_without_override(IMAGE1, PNG);
        }
        VsdxBuilder::new().with(pkg).build()
    };
    let d = diff(&with(true), &with(false));
    assert_eq!(d.shown, [Difference { part: IMAGE1.to_owned(), path: String::new(), found: Found::MissingPart }]);
    assert_eq!(compare_content(&with(false), &with(true), &Limits::default()).unwrap().shown.first().map(|x| x.found), Some(Found::ExtraPart));
}

#[test]
fn a_changed_instruction_fails() {
    // `NewValue` is an instruction a program may act on: it is compared.
    let with = |data: &str| shape(1, &format!("<?NewValue {data}?>{}", cell("PinX", "1")));
    only(&on_page(&with("V"), &with("W")), Found::Instruction, "/PageContents[0]/Shapes[0]/Shape[0]/#instruction[0]");
}

#[test]
fn a_changed_content_type_fails() {
    let with = |kind: &str| VsdxBuilder::new().with(Pkg::new().part(EXTRA_A, kind, b"x")).build();
    let d = diff(&with("application/a"), &with("application/b"));
    assert_eq!((d.count, d.shown.first().map(|x| (x.part.as_str(), x.found))), (1, Some(("[Content_Types].xml", Found::Value))));
}

// ------------------------------------------------------------------------------------------------- planted reformatting passes

/// A package with its XML parts reformatted by `f`.
fn reformatted(builder: &VsdxBuilder, f: impl Fn(&str) -> String) -> Vec<u8> {
    builder
        .pkg()
        .map_parts(|name, bytes| {
            if name.ends_with(".xml") {
                *bytes = f(&String::from_utf8(std::mem::take(bytes)).unwrap()).into_bytes();
            }
        })
        .build()
}

fn busy_builder() -> VsdxBuilder {
    let shapes: String =
        (1..=4).map(|id| shape(id, &format!("{}{}{}", cell("PinX", &id.to_string()), cell_u("Width", "1.5", "IN"), cell("PinY", "2")))).collect();
    VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shapes)).page(PageSpec::new(2, "Q").shapes(&shape(1, &cell("LineWeight", "0.01"))))
}

#[test]
fn spacing_passes() {
    let builder = busy_builder();
    let plain = builder.build();
    // Spacing between elements, and inside a tag; an element with nothing in it keeps nothing in it (white space in an empty element is its text).
    let spaced =
        reformatted(&builder, |xml| xml.replace("><", ">\r\n   <").replace(">\r\n   </", "></").replace("/>", " />").replace("?>\n", "?>\r\n\r\n"));
    assert_ne!(plain, spaced);
    let d = diff(&plain, &spaced);
    assert!(d.same(), "{:?}", d.shown);
    // And with a comment between every pair of elements.
    let noted = reformatted(&builder, |xml| xml.replace("><", "><!-- a note --><"));
    assert!(diff(&plain, &noted).same());
}

#[test]
fn attribute_order_passes() {
    let a = on_page(&shape(1, &cell_u("PinX", "1", "IN")), &shape(1, "<Cell U=\"IN\" V=\"1\" N=\"PinX\"/>"));
    assert!(a.same(), "{:?}", a.shown);
    // And the attributes of a shape.
    assert!(on_page("<Shape ID=\"1\" Type=\"Group\" Name=\"x\"/>", "<Shape Name=\"x\" Type=\"Group\" ID=\"1\"/>").same());
}

#[test]
fn another_prefix_for_one_namespace_passes() {
    let page = |body: &str| format!("<?xml version=\"1.0\"?>{body}");
    let ns = NS_2012_TEXT;
    let plain = page(&format!("<PageContents xmlns=\"{ns}\"><Shapes><Shape ID=\"1\"><Cell N=\"PinX\" V=\"1\"/></Shape></Shapes></PageContents>"));
    let prefixed = page(&format!(
        "<v:PageContents xmlns:v=\"{ns}\"><v:Shapes><v:Shape ID=\"1\"><v:Cell N=\"PinX\" V=\"1\"/></v:Shape></v:Shapes></v:PageContents>"
    ));
    let build = |xml: &str| VsdxBuilder::new().page(PageSpec::new(1, "P").part(xml)).build();
    assert!(diff(&build(&plain), &build(&prefixed)).same());
    // The same names in another namespace are other names, and a part with none is not in the one that every sample uses.
    let other = plain.replace(ns, NS_2011_TEXT);
    assert!(!diff(&build(&plain), &build(&other)).same());
    assert!(!diff(&build(&plain), &build(&plain.replace(&format!(" xmlns=\"{ns}\""), ""))).same());
}

#[test]
fn keyed_siblings_reordered_pass() {
    let cells = |names: &[&str]| shape(1, &names.iter().map(|n| cell(n, "1")).collect::<String>());
    let (a, b) = (cells(&["PinX", "PinY", "Width", "Height"]), cells(&["Height", "PinX", "Width", "PinY"]));
    assert!(on_page(&a, &b).same());
    // A value changed after the reorder is one difference at the cell, found by its name (`Height` is the first of the four sorted by name).
    let changed = shape(1, &format!("{}{}{}{}", cell("Height", "2"), cell("PinX", "1"), cell("Width", "1"), cell("PinY", "1")));
    only(&on_page(&a, &changed), Found::Value, "/PageContents[0]/Shapes[0]/Shape[0]/Cell#0/@V");
    // The cells of a row are a set as well, and the rows are not.
    let rows = |first: &str| shape(1, &format!("<Section N=\"Geometry\" IX=\"0\"><Row T=\"LineTo\" IX=\"1\">{first}</Row></Section>"));
    assert!(on_page(&rows(&format!("{}{}", cell("X", "1"), cell("Y", "2"))), &rows(&format!("{}{}", cell("Y", "2"), cell("X", "1")))).same());
    // A cell that moved into another shape is two differences.
    assert!(!on_page(&format!("{}{}", shape(1, &cell("X", "1")), shape(2, "")), &format!("{}{}", shape(1, ""), shape(2, &cell("X", "1")))).same());
}

#[test]
fn rows_of_the_content_types_and_the_relationships_reordered_pass() {
    let a = Pkg::new()
        .part(EXTRA_A, "application/a", b"1")
        .part(EXTRA_B, "application/b", b"2")
        .rel("/", "rId7", "urn:t1", REL_A)
        .rel("/", "rId8", "urn:t2", REL_B);
    let b = Pkg::new()
        .part(EXTRA_B, "application/b", b"2")
        .part(EXTRA_A, "application/a", b"1")
        .rel("/", "rId8", "urn:t2", REL_B)
        .rel("/", "rId7", "urn:t1", REL_A);
    let (pa, pb) = (VsdxBuilder::new().with(a).build(), VsdxBuilder::new().with(b).build());
    assert_ne!(pa, pb);
    assert!(diff(&pa, &pb).same());
    // A relationship that points elsewhere, or has another id, and a row taken out, are not the same.
    let c = Pkg::new()
        .part(EXTRA_A, "application/a", b"1")
        .part(EXTRA_B, "application/b", b"2")
        .rel("/", "rId7", "urn:t1", REL_B)
        .rel("/", "rId8", "urn:t2", REL_B);
    assert!(!diff(&pa, &VsdxBuilder::new().with(c).build()).same());
}

// ------------------------------------------------------------------------------------------------------------- the zip's own layout

/// An archive of `entries` written the way `style` says: stored or deflated, with dates, extra fields, a data descriptor and an archive comment.
struct Style {
    deflate: bool,
    date: (u16, u16),
    extra: bool,
    descriptor: bool,
    comment: &'static [u8],
    reverse: bool,
}

fn rezip(package: &[u8], style: &Style) -> Vec<u8> {
    let limits = diacraft_container::Limits::default();
    let mut archive = Archive::read(package, &limits).unwrap();
    let mut work = Work::default();
    let mut entries = Vec::new();
    for index in 0..archive.entries().len() {
        let data = archive.inflate(index, &mut work).unwrap();
        entries.push((archive.entries()[index].name.clone(), data));
    }
    if style.reverse {
        entries.reverse();
    }
    let (mut out, mut central) = (Vec::new(), Vec::new());
    for (name, data) in &entries {
        let (method, body) = if style.deflate {
            let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
            encoder.write_all(data).unwrap();
            (8u16, encoder.finish().unwrap())
        } else {
            (0u16, data.clone())
        };
        let crc = crc32fast::hash(data);
        let flags: u16 = if style.descriptor { 8 } else { 0 };
        let extra: Vec<u8> = if style.extra { vec![0xFE, 0xCA, 4, 0, 1, 2, 3, 4] } else { Vec::new() };
        let offset = u32::try_from(out.len()).unwrap();
        // The local header: with a descriptor the sizes are zero, as writers that stream them leave them.
        let (lc, ls, lu) = if style.descriptor { (0, 0, 0) } else { (crc, u32::try_from(body.len()).unwrap(), u32::try_from(data.len()).unwrap()) };
        for (value, width) in
            [(0x0403_4B50u64, 4), (20, 2), (u64::from(flags), 2), (u64::from(method), 2), (u64::from(style.date.0), 2), (u64::from(style.date.1), 2)]
        {
            out.extend_from_slice(&value.to_le_bytes()[..width]);
        }
        for value in [lc, ls, lu] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&u16::try_from(name.len()).unwrap().to_le_bytes());
        out.extend_from_slice(&u16::try_from(extra.len()).unwrap().to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&extra);
        out.extend_from_slice(&body);
        if style.descriptor {
            for value in [0x0807_4B50u32, crc, u32::try_from(body.len()).unwrap(), u32::try_from(data.len()).unwrap()] {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
        for (value, width) in [
            (0x0201_4B50u64, 4),
            (45, 2),
            (20, 2),
            (u64::from(flags), 2),
            (u64::from(method), 2),
            (u64::from(style.date.0), 2),
            (u64::from(style.date.1), 2),
        ] {
            central.extend_from_slice(&value.to_le_bytes()[..width]);
        }
        for value in [crc, u32::try_from(body.len()).unwrap(), u32::try_from(data.len()).unwrap()] {
            central.extend_from_slice(&value.to_le_bytes());
        }
        for value in [name.len(), extra.len(), 0, 0, 0] {
            central.extend_from_slice(&u16::try_from(value).unwrap().to_le_bytes());
        }
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
        central.extend_from_slice(&extra);
    }
    let (start, count) = (u32::try_from(out.len()).unwrap(), u16::try_from(entries.len()).unwrap());
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&u32::try_from(central.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&u16::try_from(style.comment.len()).unwrap().to_le_bytes());
    out.extend_from_slice(style.comment);
    out
}

#[test]
fn recompression_dates_extra_fields_and_comment_pass() {
    let plain = busy_builder().with(Pkg::new().default_type("png", "image/png").part_without_override(IMAGE1, PNG)).build();
    let styles = [
        Style { deflate: false, date: (0, 0x21), extra: false, descriptor: false, comment: b"", reverse: false },
        Style { deflate: true, date: (0x6000, 0x5A21), extra: true, descriptor: false, comment: b"a comment on the archive", reverse: false },
        Style { deflate: true, date: (0x1234, 0x4321), extra: true, descriptor: true, comment: b"", reverse: true },
        Style { deflate: false, date: (1, 0x21), extra: true, descriptor: true, comment: b"x", reverse: true },
    ];
    for (i, style) in styles.iter().enumerate() {
        let again = rezip(&plain, style);
        assert!(diff(&plain, &again).same(), "style {i}");
        assert!(diff(&again, &rezip(&again, &styles[(i + 1) % styles.len()])).same(), "style {i}, again");
    }
    // The archive that was written reads back as what it was: a change in a picture's bytes is still found in a stored or a deflated entry.
    let mut changed_png = PNG;
    changed_png[11] = 0;
    let changed = busy_builder().with(Pkg::new().default_type("png", "image/png").part_without_override(IMAGE1, changed_png)).build();
    let d = diff(&rezip(&plain, &styles[1]), &rezip(&changed, &styles[0]));
    assert_eq!((d.count, d.shown.first().map(|x| x.found)), (1, Some(Found::Bytes)));
}

// -------------------------------------------------------------------------------------------------------------------------- text

fn text_page(text_xml: &str) -> String {
    shape(1, &format!("<Text>{text_xml}</Text>"))
}

#[test]
fn comments_are_ignored() {
    let (a, b) = (shape(1, &format!("<Text><cp IX=\"0\"/>a{}b</Text>", "<!-- x -->")), shape(1, "<Text><cp IX=\"0\"/>ab</Text>"));
    assert!(on_page(&a, &b).same());
    let c = shape(1, "<Text><cp IX=\"0\"/>a<!-- x -->c</Text>");
    assert!(!on_page(&a, &c).same());
}

#[test]
fn text_is_compared_after_cdata_references_and_joining() {
    let a = text_page("<cp IX=\"0\"/>a&amp;b");
    assert!(on_page(&a, &text_page("<cp IX=\"0\"/><![CDATA[a&b]]>")).same());
    assert!(on_page(&a, &text_page("<cp IX=\"0\"/>a<![CDATA[&]]>b")).same());
    only(&on_page(&a, &text_page("<cp IX=\"0\"/>a&amp;c")), Found::Text, "/PageContents[0]/Shapes[0]/Shape[0]/Text[0]/#text[1]");
}

#[test]
fn blank_text_is_ignored_only_between_children_of_an_element_with_no_text() {
    // Between shapes, indented: nothing.
    assert!(on_page(&format!("\n  {}\n  {}\n", shape(1, ""), shape(2, "")), &format!("{}{}", shape(1, ""), shape(2, ""))).same());
    // In a shape's text it is the text, and a space beside a marker is a space: spacing and text are told apart by the element.
    only(&on_page(&text_page("<cp IX=\"0\"/> "), &text_page("<cp IX=\"0\"/>")), Found::Text, "/PageContents[0]/Shapes[0]/Shape[0]/Text[0]/#text[1]");
    assert!(!on_page(&text_page("<cp IX=\"0\"/>\n<pp IX=\"0\"/>"), &text_page("<cp IX=\"0\"/><pp IX=\"0\"/>")).same());
    assert!(!on_page(&text_page(" "), &text_page("")).same());
    // The same in a part whose root says `xml:space`, as every part of the builder's does: spacing between its elements is still no content.
    let d = on_page(&text_page("a<cp IX=\"0\"/>"), &text_page("a<cp IX=\"0\"/> "));
    assert_eq!(d.count, 1);
}

// ------------------------------------------------------------------------------------------------------------ echo control

fn big_page(salt: Option<(usize, usize)>) -> String {
    (0..12)
        .map(|shape_id| {
            let cells: String = (0..16)
                .map(|c| {
                    let v = if salt == Some((shape_id, c)) { "SALT".to_owned() } else { format!("{}.5", c) };
                    format!("<Cell N=\"Cell{c:02}\" V=\"{v}\" U=\"IN\"/>")
                })
                .collect();
            shape(u32::try_from(shape_id + 1).unwrap(), &cells)
        })
        .collect()
}

#[test]
fn a_file_against_itself_is_the_same() {
    let package = package(&[&big_page(None), &big_page(None), &big_page(None)]);
    let d = diff(&package, &package);
    assert!(d.same() && d.shown.is_empty());
    // And against a copy that is written another way.
    let style = Style { deflate: true, date: (7, 0x21), extra: true, descriptor: true, comment: b"c", reverse: true };
    assert!(diff(&package, &rezip(&package, &style)).same());
}

#[test]
fn a_file_against_a_salted_copy_fails_at_the_salt_only() {
    let base = package(&[&big_page(None), &big_page(None), &big_page(None)]);
    for (page, shape_at, cell_at) in [(0usize, 0usize, 0usize), (1, 5, 7), (2, 11, 15), (1, 0, 15)] {
        let mut pages = [big_page(None), big_page(None), big_page(None)];
        pages[page] = big_page(Some((shape_at, cell_at)));
        let salted = package(&[&pages[0], &pages[1], &pages[2]]);
        let d = diff(&base, &salted);
        assert_eq!(d.count, 1, "{:?}", d.shown);
        let (part, path, found) = d.shown.first().map(|x| (x.part.clone(), x.path.clone(), x.found)).unwrap();
        assert_eq!(part, format!("/{DIR}/pages/page{}.xml", page + 1));
        assert_eq!(path, format!("/PageContents[0]/Shapes[0]/Shape[{shape_at}]/Cell#{cell_at}/@V"));
        assert_eq!(found, Found::Value);
    }
}

// ------------------------------------------------------------------------------------------------------------ Review Focus 3

#[test]
fn verbatim_attributes_are_not_number_typed() {
    // Only an attribute that a model writer is shown to respell may be number-typed (NP8). The model keeps every VSDX value as the file gave it,
    // so none is: if `V` were listed, `007` changed to `7` would pass as the same content.
    assert!(CANON_RULES.numbers.is_empty());
    for attribute in ["V", "F", "U", "E", "IX", "ID", "N"] {
        assert!(!CANON_RULES.numbers.iter().any(|n| n.local == attribute), "{attribute}");
    }
    // The other two lists are the ones the module documents: no paragraphs, and `Text` as the mixed content.
    assert!(CANON_RULES.paragraphs.is_empty());
    assert!(CANON_RULES.exact_text.iter().all(|n| n.local == "Text"));
}

#[test]
fn zero_padded_value_change_is_a_difference() {
    // A cell with `V` of `007` against `7`, through the whole check on a real package.
    let with = |v: &str| shape(1, &format!("<Cell N=\"X\" V=\"{v}\"/>"));
    only(&on_page(&with("007"), &with("7")), Found::Value, "/PageContents[0]/Shapes[0]/Shape[0]/Cell#0/@V");
    // Every spelling of a number is a difference in every attribute that holds text.
    let template = |attribute: &str, value: &str| {
        let mut parts = [("ID", "1"), ("N", "X"), ("V", "2"), ("U", "IN"), ("F", "1+1"), ("E", "#REF!"), ("IX", "1")];
        for (name, v) in &mut parts {
            if *name == attribute {
                *v = value;
            }
        }
        let get = |name: &str| parts.iter().find(|(n, _)| *n == name).map(|(_, v)| *v).unwrap();
        format!(
            "<Shape ID=\"{}\"><Cell N=\"{}\" V=\"{}\" U=\"{}\" F=\"{}\" E=\"{}\"/><Section N=\"Geometry\" IX=\"{}\"/></Shape>",
            get("ID"),
            get("N"),
            get("V"),
            get("U"),
            get("F"),
            get("E"),
            get("IX")
        )
    };
    for (attribute, a, b) in [
        ("V", "007", "7"),
        ("V", "1.50", "1.5"),
        ("V", "1E0", "1"),
        ("V", "+1", "1"),
        ("V", "1e0", "1.0"),
        ("V", "0", "-0"),
        ("U", "007", "7"),
        ("F", "007", "7"),
        ("F", "1+1", "1+1 "),
        ("E", "007", "7"),
        ("IX", "01", "1"),
        ("IX", "1", "+1"),
        ("ID", "007", "7"),
        ("ID", "7", "7.0"),
        ("N", "007", "7"),
    ] {
        let d = on_page(&template(attribute, a), &template(attribute, b));
        assert!(!d.same(), "{attribute}: {a} against {b} passed");
    }
    // And the one that is not a spelling: a value that is the same passes.
    assert!(on_page(&template("V", "007"), &template("V", "007")).same());
}

// -------------------------------------------------------------------------------------------------------------------- part names

#[test]
fn vsdx_part_names_fold_ascii_case_only() {
    let with =
        |name: &str| VsdxBuilder::new().with(Pkg::new().default_type("bin", "application/octet-stream").part_without_override(name, b"x")).build();
    // OPC part names are the same when ASCII case is folded.
    assert!(diff(&with("/X/Page1.bin"), &with("/x/page1.bin")).same());
    // Other letters do not fold: the two names, written as UTF-8 percent-escapes, are two parts.
    let d = diff(&with("/x/%C3%89.bin"), &with("/x/%C3%A9.bin"));
    assert_eq!(d.count, 2, "{:?}", d.shown);
    let found: Vec<Found> = d.shown.iter().map(|x| x.found).collect();
    assert!(found.contains(&Found::MissingPart) && found.contains(&Found::ExtraPart), "{found:?}");
    // The same name in a different case is the same part, and its bytes are still compared.
    assert!(
        !diff(
            &with("/X/Page1.bin"),
            &VsdxBuilder::new().with(Pkg::new().default_type("bin", "application/octet-stream").part_without_override("/x/page1.bin", b"y")).build()
        )
        .same()
    );
}

#[test]
fn difference_paths_show_no_names_a_person_chose() {
    // A page named `Secret`, a cell named `User.Secret`, a row of the properties section, and the text of a shape: all differ, none is in a path.
    let page = |name: &str, user: &str, text: &str| {
        let shapes = shape(
            1,
            &format!(
                "{}<Section N=\"Property\"><Row N=\"{name}\">{}</Row></Section><Text>{text}</Text>",
                cell(&format!("User.{name}"), user),
                cell("Value", user)
            ),
        );
        VsdxBuilder::new().page(PageSpec::new(1, name).shapes(&shapes)).build()
    };
    let d = diff(&page("Secret", "1", "Secret text"), &page("Public", "2", "Public text"));
    assert!(d.count >= 4, "{:?}", d.shown);
    for found in &d.shown {
        for word in ["Secret", "Public", "User"] {
            assert!(!found.path.contains(word) && !found.part.contains(word), "{found:?}");
        }
    }
}

// ------------------------------------------------------------------------------------------------------------- what is XML

#[test]
fn a_picture_of_an_xml_type_is_bytes_and_other_xml_is_xml() {
    let with = |kind: &str, bytes: &str| VsdxBuilder::new().with(Pkg::new().part(EXTRA_P, kind, bytes)).build();
    // An SVG picture is a picture: reformatting it is another picture.
    assert!(!diff(&with("image/svg+xml", "<svg/>"), &with("image/svg+xml", "<svg />")).same());
    // A part of an XML content type, or of one with `+xml`, is XML, and a parameter after the type does not hide it.
    for kind in ["application/xml", "text/xml", "application/vnd.example.thing+xml", "application/xml; charset=utf-8"] {
        assert!(diff(&with(kind, "<a/>"), &with(kind, "<a  />")).same(), "{kind}");
    }
    // Any other type is bytes.
    assert!(!diff(&with("application/octet-stream", "<a/>"), &with("application/octet-stream", "<a  />")).same());
    // A part of an XML type that does not parse is bytes, and the same bytes are the same.
    assert!(!diff(&with("application/xml", "<a>"), &with("application/xml", "<a >")).same());
    assert!(diff(&with("application/xml", "<a>"), &with("application/xml", "<a>")).same());
}

// ------------------------------------------------------------------------------------------------------------------ the cost

#[test]
fn comparing_counts_its_work_and_reads_each_package_once() {
    let a = package(&[&big_page(None), &big_page(None)]);
    let b = package(&[&big_page(None), &big_page(Some((3, 3)))]);
    let mut work = Work::default();
    let d = compare_content_counted(&a, &b, &Limits::default(), &mut work).unwrap();
    assert_eq!(d.count, 1);
    // The content types, the relationships and the pages of each, each part parsed when its bytes differ: the page whose bytes are the same is not.
    assert!(work.canon_steps > 0 && work.xml_events > 0);
    assert!(work.canon_steps <= 40 * 2 * 12 * 16, "{}", work.canon_steps);
}

#[test]
fn a_package_that_cannot_be_read_is_an_error_with_a_class() {
    let good = package(&[&big_page(None)]);
    assert_eq!(compare_content(b"not a package", &good, &Limits::default()).unwrap_err().class(), "not_zip");
    assert_eq!(compare_content(&good, &good[..good.len() / 2], &Limits::default()).unwrap_err().class(), "zip_corrupt");
}

// -------------------------------------------------------------------------------------------- each list in each place it applies

#[test]
fn cells_are_a_set_and_text_is_exact_in_every_namespace_a_part_can_be_written_in() {
    use diacraft_testkit::package::Namespace;
    for ns in [Namespace::Main2012, Namespace::Core2011, Namespace::None] {
        let with = |shapes: &str| VsdxBuilder::new().namespace(ns).page(PageSpec::new(1, "P").shapes(shapes)).build();
        let cells = |names: &[&str]| shape(1, &names.iter().map(|n| cell(n, "1")).collect::<String>());
        // Cells in another order: the same. A cell changed: one difference, at its place by name.
        assert!(diff(&with(&cells(&["PinX", "PinY", "Width"])), &with(&cells(&["Width", "PinX", "PinY"]))).same(), "{ns:?}");
        let changed = shape(1, &format!("{}{}{}", cell("Width", "2"), cell("PinX", "1"), cell("PinY", "1")));
        let d = diff(&with(&cells(&["PinX", "PinY", "Width"])), &with(&changed));
        assert_eq!(d.count, 1, "{ns:?}: {:?}", d.shown);
        assert_eq!(
            d.shown.first().map(|x| (x.path.as_str(), x.found)),
            Some(("/PageContents[0]/Shapes[0]/Shape[0]/Cell#2/@V", Found::Value)),
            "{ns:?}"
        );
        // Text beside a marker: a space is a space.
        let (a, b) = (with(&text_page("<cp IX=\"0\"/> ")), with(&text_page("<cp IX=\"0\"/>")));
        assert_eq!(diff(&a, &b).count, 1, "{ns:?}");
        // And spacing between shapes is spacing.
        let (a, b) = (with(&format!("{}{}", shape(1, ""), shape(2, ""))), with(&format!("\n {} \n {}\n", shape(1, ""), shape(2, ""))));
        assert!(diff(&a, &b).same(), "{ns:?}");
    }
}

#[test]
fn default_rows_of_the_content_types_reordered_pass_and_a_changed_one_does_not() {
    let with = |first: (&str, &str), second: (&str, &str)| {
        let pkg = Pkg::new().default_type(first.0, first.1).default_type(second.0, second.1);
        VsdxBuilder::new().with(pkg).build()
    };
    let (png, jpg) = (("png", "image/png"), ("jpg", "image/jpeg"));
    assert!(diff(&with(png, jpg), &with(jpg, png)).same());
    let d = diff(&with(png, jpg), &with(png, ("jpg", "image/jpg")));
    assert_eq!((d.count, d.shown.first().map(|x| (x.part.as_str(), x.found))), (1, Some(("[Content_Types].xml", Found::Value))));
    // A row taken out is one difference, whichever side it is on.
    assert_eq!(diff(&with(png, jpg), &VsdxBuilder::new().with(Pkg::new().default_type("png", "image/png")).build()).count, 1);
}

#[test]
fn a_relationships_part_with_no_content_type_is_xml_by_its_name() {
    use diacraft_testkit::package::{ZipItem, zip};
    // A package whose content types give no type to `rels`: the part is XML by its name, so spacing in it is spacing.
    let types = "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/></Types>";
    let rels = |body: &str| {
        let xml = format!("<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{body}</Relationships>");
        zip(&[ZipItem::stored("[Content_Types].xml", types.as_bytes()), ZipItem::stored("_rels/.rels", xml.as_bytes())])
    };
    let (a, b) = (
        rels("<Relationship Id=\"rId1\" Type=\"urn:t\" Target=\"a.xml\"/>"),
        rels("\n  <Relationship Target=\"a.xml\" Type=\"urn:t\" Id=\"rId1\"></Relationship>\n"),
    );
    assert!(diff(&a, &b).same());
    assert!(!diff(&a, &rels("<Relationship Id=\"rId1\" Type=\"urn:t\" Target=\"b.xml\"/>")).same());
}

#[test]
fn a_part_named_in_another_case_is_the_same_part_and_the_row_that_names_it_is_a_value() {
    // The set of parts folds case, and so does a row's key, as OPC's reader folds it; the row's `PartName` is a value compared as written:
    // so with a row the change of case is one difference, in the value of the row, and the part itself is found in both packages.
    let with = |name: &str| VsdxBuilder::new().with(Pkg::new().part(name, "application/vnd.example.thing+xml", b"<a/>")).build();
    let d = diff(&with(&format!("/{DIR}/extra/Thing.xml")), &with(&format!("/{DIR}/extra/thing.xml")));
    assert_eq!(d.count, 1, "{:?}", d.shown);
    assert!(d.shown.iter().all(|x| x.part == "[Content_Types].xml" && x.found == Found::Value), "{:?}", d.shown);
}

// ------------------------------------------------------------------------------------ content type rows as the OPC reader folds them

/// A package of a stored part and these content types rows.
fn rows_package(rows: &str) -> Vec<u8> {
    use diacraft_testkit::package::{ZipItem, zip};
    let types = format!("<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">{rows}</Types>");
    zip(&[ZipItem::stored("[Content_Types].xml", types.as_bytes()), ZipItem::stored(&format!("{DIR}/pages/page1.xml"), b"<a/>")])
}

fn overrides(first: (&str, &str), second: (&str, &str)) -> Vec<u8> {
    let row = |(name, kind): (&str, &str)| format!("<Override PartName=\"{name}\" ContentType=\"{kind}\"/>");
    rows_package(&format!("{}{}", row(first), row(second)))
}

#[test]
fn overrides_the_opc_reader_folds_to_one_part_are_compared_in_the_order_written() {
    // OPC's reader takes the first row of a part name that is the same when ASCII case is folded and `%` escapes are decoded; so two rows
    // for one part in another order are another type for the part, and the check must not call them the same.
    let (lower_name, upper_name) = (format!("/{DIR}/pages/page1.xml"), format!("/{}/PAGES/PAGE1.XML", DIR.to_uppercase()));
    let (page, master) = ((lower_name.as_str(), "application/page+xml"), (upper_name.as_str(), "application/master+xml"));
    let (a, b) = (overrides(page, master), overrides(master, page));
    assert!(!diff(&a, &b).same(), "the rows in another order give the part another type");
    assert!(diff(&a, &a).same());
    // A percent escape of a digit is the same name once decoded.
    let escaped_name = format!("/{DIR}/pages/page%31.xml");
    let escaped = (escaped_name.as_str(), "application/master+xml");
    assert!(!diff(&overrides(page, escaped), &overrides(escaped, page)).same());
    // Rows for parts that are not the same part are a set.
    let other = ("/other.xml", "application/x+xml");
    assert!(diff(&overrides(page, other), &overrides(other, page)).same());
}

#[test]
fn defaults_the_opc_reader_folds_to_one_extension_are_compared_in_the_order_written() {
    let row = |(extension, kind): (&str, &str)| format!("<Default Extension=\"{extension}\" ContentType=\"{kind}\"/>");
    let (lower, upper, png) = (("xml", "application/xml"), ("XML", "text/xml"), ("png", "image/png"));
    let package = |a: (&str, &str), b: (&str, &str)| rows_package(&format!("{}{}", row(a), row(b)));
    assert!(!diff(&package(lower, upper), &package(upper, lower)).same());
    assert!(diff(&package(lower, png), &package(png, lower)).same());
}

// ---------------------------------------------------------------------------------------- prefixes inside the values of some attributes

const NS_PAGE: &str = NS_2012_TEXT;
const NS_MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";

/// A one-page package whose page part is `xml`.
fn page_part(xml: &str) -> Vec<u8> {
    VsdxBuilder::new().page(PageSpec::new(1, "P").part(xml)).build()
}

/// A page part whose root says `ignorable` and binds `v14` as `bound` says (the whole attribute and declaration text).
fn ignorable_page(attrs: &str) -> Vec<u8> {
    page_part(&format!("<PageContents xmlns=\"{NS_PAGE}\" xmlns:mc=\"{NS_MC}\"{attrs}><Shapes/></PageContents>"))
}

#[test]
fn a_prefix_in_mc_ignorable_is_its_namespace() {
    let bound = |uri: &str| ignorable_page(&format!(" xmlns:v14=\"{uri}\" mc:Ignorable=\"v14\""));
    let a = bound("urn:a");
    // The same namespace under another prefix passes; another namespace under the same prefix, and a prefix with no declaration, do not.
    assert!(diff(&a, &ignorable_page(" xmlns:w=\"urn:a\" mc:Ignorable=\"w\"")).same());
    assert!(!diff(&a, &bound("urn:b")).same(), "v14 rebound");
    assert!(!diff(&a, &ignorable_page(" mc:Ignorable=\"v14\"")).same(), "the declaration dropped");
    // A list: each prefix is its own, in the order written.
    let two = |x: &str, y: &str| ignorable_page(&format!(" xmlns:a=\"urn:a\" xmlns:b=\"urn:b\" xmlns:c=\"urn:c\" mc:Ignorable=\"{x} {y}\""));
    assert!(diff(&two("a", "b"), &two("a", "b")).same());
    assert!(diff(&two("a", "b"), &ignorable_page(" xmlns:a=\"urn:a\" xmlns:b=\"urn:b\" mc:Ignorable=\"a   b\"")).same());
    assert!(!diff(&two("a", "b"), &two("a", "c")).same());
    assert!(!diff(&two("a", "b"), &two("b", "a")).same());
    // A prefix that nothing declares, on both sides, is the same text: nothing is said of it.
    assert!(diff(&ignorable_page(" mc:Ignorable=\"zz\""), &ignorable_page(" mc:Ignorable=\"zz\"")).same());
    assert!(!diff(&ignorable_page(" mc:Ignorable=\"zz\""), &ignorable_page(" mc:Ignorable=\"yy\"")).same());
    // The attribute is read by name: another attribute of the same text is text.
    assert!(diff(&ignorable_page(" xmlns:v14=\"urn:a\" Mine=\"v14\""), &ignorable_page(" xmlns:v14=\"urn:b\" Mine=\"v14\"")).same());
}

#[test]
fn a_prefix_in_the_requires_of_an_mc_choice_is_its_namespace() {
    let choice = |uri: &str, prefix: &str| {
        page_part(&format!(
            "<PageContents xmlns=\"{NS_PAGE}\" xmlns:mc=\"{NS_MC}\"><mc:AlternateContent><mc:Choice Requires=\"{prefix}\" xmlns:{prefix}=\"{uri}\"><Shapes/></mc:Choice><mc:Fallback/></mc:AlternateContent></PageContents>"
        ))
    };
    let a = choice("urn:a", "x");
    assert!(diff(&a, &choice("urn:a", "y")).same(), "another prefix, the same namespace");
    assert!(!diff(&a, &choice("urn:b", "x")).same(), "x rebound");
    // The attribute is `Requires` of `Choice` only.
    let other = |uri: &str| page_part(&format!("<PageContents xmlns=\"{NS_PAGE}\" xmlns:x=\"{uri}\"><Shapes Requires=\"x\"/></PageContents>"));
    assert!(diff(&other("urn:a"), &other("urn:b")).same(), "a Requires on another element is text");
}

#[test]
fn a_prefix_in_an_xsi_type_is_its_namespace() {
    // The element is `dc:title`, whose prefix stays bound, so that what differs is only what the type's prefix names.
    let core = |declaration: &str, kind: &str| {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"{declaration} xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:title xsi:type=\"{kind}\">2026-11-17T00:00:00Z</dc:title></cp:coreProperties>"
        )
    };
    let package = |xml: &str| VsdxBuilder::new().core(xml).build();
    let declared = |uri: &str| format!(" xmlns:dcterms=\"{uri}\"");
    let w3c = declared("http://purl.org/dc/terms/");
    let a = package(&core(&w3c, "dcterms:W3CDTF"));
    assert!(diff(&a, &package(&core(&w3c, "dcterms:W3CDTF"))).same());
    assert!(diff(&a, &package(&core(&declared("http://purl.org/dc/terms/").replace("dcterms", "terms"), "terms:W3CDTF"))).same(), "another prefix");
    assert!(!diff(&a, &package(&core(&declared("urn:other"), "dcterms:W3CDTF"))).same(), "dcterms rebound");
    assert!(!diff(&a, &package(&core("", "dcterms:W3CDTF"))).same(), "dcterms with no declaration");
    assert!(!diff(&a, &package(&core(&w3c, "dcterms:Other"))).same());
    // No prefix: the default namespace's name, and not the dcterms one.
    assert!(!diff(&a, &package(&core(&w3c, "W3CDTF"))).same());
    // Unbound on both sides it is the same text, and another text is not.
    assert!(diff(&package(&core("", "dcterms:W3CDTF")), &package(&core("", "dcterms:W3CDTF"))).same());
    assert!(!diff(&package(&core("", "dcterms:W3CDTF")), &package(&core("", "terms:W3CDTF"))).same());
}

// ------------------------------------------------------------------------------------------------------------ the budget of a comparison

#[test]
fn a_package_past_the_budget_is_refused_in_words_before_it_is_inflated() {
    use diacraft_testkit::package::{ZipItem, zip};
    // A package whose one part inflates to 200 MiB (a file of 400 KB): more than the half of the budget that each package may take.
    let big = zip(&[
        ZipItem::stored("[Content_Types].xml", b"<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"/>"),
        ZipItem::zeros_under_the_ratio_cap("big.bin", 200 << 20),
    ]);
    let small = package(&[&shape(1, "")]);
    let mut work = Work::default();
    let err = compare_content_counted(&big, &small, &Limits::default(), &mut work).unwrap_err();
    assert_eq!(err.class(), "package_too_large", "{err}");
    assert!(err.to_string().contains("larger than"), "{err}");
}

#[test]
fn a_refusal_of_a_part_names_the_part() {
    // A page of more items than the budget holds (666,667 cells of three items each), the same page with a cell changed.
    let page = |last: &str| {
        let cells = "<Cell N=\"A\" V=\"1\"/>".repeat(666_667);
        page_part(&format!(
            "<PageContents xmlns=\"{NS_PAGE}\"><Shapes><Shape ID=\"1\">{cells}<Cell N=\"B\" V=\"{last}\"/></Shape></Shapes></PageContents>"
        ))
    };
    let err = compare_content(&page("1"), &page("2"), &Limits::default()).unwrap_err();
    assert_eq!(err.class(), "xml_too_complex");
    assert!(err.to_string().contains(&format!("/{DIR}/pages/page1.xml")), "{err}");
}

// ------------------------------------------------------------------------------------------------------ what a whole comparison holds

/// The peak resident memory of this process so far, in bytes.
fn peak_rss() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
        .map_or(0, |kb| kb * 1024)
}

/// A package at the per-package budget (192 MiB inflated): a page part and a part of zeros that fills what the page leaves.
fn package_at_the_budget(page: &str) -> Vec<u8> {
    use diacraft_testkit::package::{ZipItem, zip};
    let types = "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"bin\" ContentType=\"application/octet-stream\"/></Types>";
    let room = (192u64 << 20) - types.len() as u64 - page.len() as u64 - 1024;
    zip(&[
        ZipItem::stored("[Content_Types].xml", types.as_bytes()),
        ZipItem::deflated(&format!("{DIR}/pages/page1.xml"), page.as_bytes()),
        ZipItem::zeros_under_the_ratio_cap("filler.bin", room),
    ])
}

#[test]
#[ignore = "about 1.5 GB; run with --ignored, alone, and read the peak"]
fn a_comparison_of_two_packages_at_the_budget_with_a_part_at_the_item_budget_stays_under_2_gib() {
    // Two packages of 192 MiB inflated each, in each a page of 1,998,000 items (666,000 cells) and a filler of zeros: the most the budgets
    // allow, in the one shape that costs the most memory for its bytes (items).
    let page = |last: &str| {
        let cells = "<Cell N=\"A\" V=\"1\"/>".repeat(666_000);
        format!("<PageContents xmlns=\"{NS_PAGE}\"><Shapes><Shape ID=\"1\">{cells}<Cell N=\"B\" V=\"{last}\"/></Shape></Shapes></PageContents>")
    };
    let (a, b) = (package_at_the_budget(&page("1")), package_at_the_budget(&page("2")));
    let before = peak_rss();
    let d = compare_content(&a, &b, &Limits::default()).unwrap();
    assert_eq!(d.count, 1, "{:?}", d.shown);
    let peak = peak_rss();
    eprintln!(
        "two packages at the budget, a page at the item budget: peak resident {} MB (before {} MB, the two files of {} KB)",
        peak >> 20,
        before >> 20,
        a.len() >> 10
    );
    assert!(peak < 2 << 30, "{} MB", peak >> 20);
}

#[test]
#[ignore = "about 1 GB; run with --ignored, alone, and read the peak"]
fn a_comparison_of_two_packages_at_the_budget_with_a_part_of_text_stays_under_2_gib() {
    // Two packages of 192 MiB inflated each, in each a page that is one text node of 180 MiB: the most bytes of text the budgets allow.
    use diacraft_testkit::package::{ZipItem, zip};
    let package = |last: &str| {
        let types = "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/></Types>";
        // Pseudo-random letters, so that the text does not deflate past the reader's ratio.
        let text: String = (0u64..180 << 20).map(|i| char::from(b'a' + ((i.wrapping_mul(2_654_435_761) >> 11) % 26) as u8)).collect();
        let page = format!("<PageContents xmlns=\"{NS_PAGE}\"><Text>{text}{last}</Text></PageContents>");
        zip(&[ZipItem::stored("[Content_Types].xml", types.as_bytes()), ZipItem::deflated(&format!("{DIR}/pages/page1.xml"), page.as_bytes())])
    };
    let (a, b) = (package("x"), package("y"));
    let before = peak_rss();
    let d = compare_content(&a, &b, &Limits::default()).unwrap();
    assert_eq!(d.count, 1, "{:?}", d.shown);
    let peak = peak_rss();
    eprintln!("two packages at the budget, a page of text: peak resident {} MB (before {} MB)", peak >> 20, before >> 20);
    assert!(peak < 2 << 30, "{} MB", peak >> 20);
}

// ------------------------------------------------------------------------------------------------------ how a content type says XML

/// A package of one stored part `name` of `bytes`, with a content type `kind` for it by an `Override` (none when `kind` is empty), and a
/// `Default` for `xml` only.
fn typed_part(name: &str, kind: &str, bytes: &str) -> Vec<u8> {
    use diacraft_testkit::package::{ZipItem, zip};
    let row = if kind.is_empty() { String::new() } else { format!("<Override PartName=\"/{name}\" ContentType=\"{kind}\"/>") };
    let types = format!(
        "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/>{row}</Types>"
    );
    zip(&[ZipItem::stored("[Content_Types].xml", types.as_bytes()), ZipItem::stored(name, bytes.as_bytes())])
}

#[test]
fn a_content_type_says_xml_whatever_its_case_and_the_space_around_it() {
    // Spacing in a part is spacing only when the part is XML; so each way of writing an XML type must be found.
    let spaced = |name: &str, kind: &str| (typed_part(name, kind, "<a x=\"1\" y=\"2\"/>"), typed_part(name, kind, "<a y=\"2\"   x=\"1\"></a>"));
    for kind in [
        "application/xml ",
        " application/xml",
        "Application/XML",
        "APPLICATION/VND.EXAMPLE+XML",
        "text/xml;charset=utf-8",
        "text/xml ; charset=utf-8",
    ] {
        let (a, b) = spaced("extra.dat", kind);
        assert!(diff(&a, &b).same(), "{kind:?}");
    }
    // A type that is not XML, and one that only ends like it in the middle of a word, are bytes.
    for kind in ["application/octet-stream", "application/xmlish", "image/svg+xml", "IMAGE/SVG+XML", " image/png "] {
        let (a, b) = spaced("extra.dat", kind);
        assert!(!diff(&a, &b).same(), "{kind:?}");
    }
}

#[test]
fn a_part_with_no_type_is_xml_when_its_name_ends_in_rels_in_any_case() {
    let rels = |name: &str, spaced: bool| {
        let body = if spaced {
            "<Relationships  >\n <Relationship Id=\"rId1\" Type=\"urn:t\" Target=\"a.xml\" />\n</Relationships>"
        } else {
            "<Relationships><Relationship Id=\"rId1\" Type=\"urn:t\" Target=\"a.xml\"/></Relationships>"
        };
        typed_part(name, "", body)
    };
    for name in ["_rels/.rels", "_rels/.RELS", "x/_rels/a.xml.Rels"] {
        assert!(diff(&rels(name, false), &rels(name, true)).same(), "{name}");
    }
    // Another name is no type: bytes.
    assert!(!diff(&rels("_rels/.dat", false), &rels("_rels/.dat", true)).same());
}

#[test]
fn a_package_under_the_budget_is_read_and_compared() {
    use diacraft_testkit::package::{ZipItem, zip};
    // A part of 150 MiB inflated in each package (of 192 that each may hold), identical but for a small part that differs.
    let package = |small: &[u8]| {
        let types = "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"bin\" ContentType=\"application/octet-stream\"/></Types>";
        zip(&[
            ZipItem::stored("[Content_Types].xml", types.as_bytes()),
            ZipItem::zeros_under_the_ratio_cap("big.bin", 150 << 20),
            ZipItem::stored("small.bin", small),
        ])
    };
    let d = compare_content(&package(b"1"), &package(b"2"), &Limits::default()).unwrap();
    assert_eq!(d.count, 1, "{:?}", d.shown);
    assert_eq!(d.shown.first().map(|x| (x.part.as_str(), x.found)), Some(("/small.bin", Found::Bytes)));
}

#[test]
fn a_prefix_in_mc_must_understand_and_a_name_in_mc_process_content_are_their_namespaces() {
    let page = |attr: &str, value: &str, uri: &str| ignorable_page(&format!(" xmlns:v14=\"{uri}\" mc:{attr}=\"{value}\""));
    for (attr, value, other) in [("MustUnderstand", "v14", "w"), ("ProcessContent", "v14:Shape", "w:Shape")] {
        let a = page(attr, value, "urn:a");
        let renamed = ignorable_page(&format!(" xmlns:w=\"urn:a\" mc:{attr}=\"{other}\""));
        assert!(diff(&a, &renamed).same(), "{attr}: another prefix for the namespace");
        assert!(!diff(&a, &page(attr, value, "urn:b")).same(), "{attr}: the prefix declared again");
        assert!(!diff(&a, &ignorable_page(&format!(" mc:{attr}=\"{value}\""))).same(), "{attr}: the declaration dropped");
    }
    // A name in `ProcessContent` is the name in its namespace: another local name is another name.
    assert!(!diff(&page("ProcessContent", "v14:Shape", "urn:a"), &page("ProcessContent", "v14:Other", "urn:a")).same());
}

#[test]
fn a_long_namespace_and_many_prefixes_in_mc_ignorable_are_refused_in_words_and_not_built() {
    // Each of the attributes the VSDX list reads through the namespaces in scope, with a prefix bound to a URI of 10,000 bytes used 10,000 times
    // (a value of 20 KB; 100 MB expanded): the comparison of the packages is refused, naming the page part.
    let uri = "u".repeat(10_000);
    let prefixes = vec!["p"; 10_000].join(" ");
    let names = vec!["p:t"; 10_000].join(" ");
    let page = |attrs: &str, extra: &str| {
        page_part(&format!(
            "<PageContents xmlns=\"{NS_PAGE}\" xmlns:mc=\"{NS_MC}\" xmlns:p=\"{uri}\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"{attrs}{extra}><Shapes/></PageContents>"
        ))
    };
    for attrs in [
        format!(" mc:Ignorable=\"{prefixes}\""),
        format!(" mc:MustUnderstand=\"{prefixes}\""),
        format!(" mc:ProcessContent=\"{names}\""),
        format!(" xsi:type=\"{names}\""),
    ] {
        let err = compare_content(&page(&attrs, ""), &page(&attrs, " Other=\"1\""), &Limits::default()).unwrap_err();
        assert_eq!(err.class(), "xml_too_complex", "{}", &attrs[..20]);
        assert!(err.to_string().contains(&format!("/{DIR}/pages/page1.xml")), "{err}");
    }
    // `Requires` of a `Choice`.
    let choice = |extra: &str| {
        page_part(&format!(
            "<PageContents xmlns=\"{NS_PAGE}\" xmlns:mc=\"{NS_MC}\" xmlns:p=\"{uri}\"><mc:AlternateContent><mc:Choice Requires=\"{prefixes}\"{extra}><Shapes/></mc:Choice></mc:AlternateContent></PageContents>"
        ))
    };
    let err = compare_content(&choice(""), &choice(" Other=\"1\""), &Limits::default()).unwrap_err();
    assert_eq!(err.class(), "xml_too_complex");
    // A package with the value as it is in a real part, one prefix once, is compared.
    let real = |extra: &str| {
        page_part(&format!(
            "<PageContents xmlns=\"{NS_PAGE}\" xmlns:mc=\"{NS_MC}\" xmlns:v14=\"urn:a\" mc:Ignorable=\"v14\"{extra}><Shapes/></PageContents>"
        ))
    };
    assert_eq!(compare_content(&real(""), &real(" Other=\"1\""), &Limits::default()).unwrap().count, 1);
}
