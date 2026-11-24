//! Saving, the harder inputs: a part that carries a prefix, a byte order mark and the older namespace; two pages that share a part; a
//! page of a hundred thousand shapes; groups nested to the cap; and generated edits, each written and read back as the model.

use std::sync::Arc;
use std::time::Instant;

use diacraft_doc::{CellAddr, CellEdit, Document, Owner, ShapeRef, SheetRef, Value, WriteOrigin};
use diacraft_opc::{Limits, Package};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use diacraft_testkit::package::{Namespace, PageSpec, Pkg, VsdxBuilder, cell, shape};
use diacraft_vsdx::{OriginalBytes, WriteError, WriteOptions, Written, read, write_with};
use proptest::prelude::*;

use crate::read_ok;

fn save(doc: &Document, original: &[u8], baseline: &Document) -> Written {
    write_with(doc, Some(&OriginalBytes { bytes: original, baseline: Some(baseline) }), &WriteOptions::default()).unwrap_or_else(|e| panic!("{e}"))
}

fn part(bytes: &[u8], name: &str) -> String {
    let pkg = Package::read(bytes, &Limits::default()).unwrap();
    String::from_utf8(pkg.part(name).unwrap_or_else(|| panic!("no part {name}")).to_vec()).unwrap()
}

fn set(doc: &mut Document, page: u32, at: u32, name: &str, value: &str) {
    let sheet = SheetRef::Shape(ShapeRef::top(Owner::Page(page), at));
    doc.write_cell(&sheet, &CellAddr::direct(name), CellEdit::Value(Value::from_text(value)), WriteOrigin::Drawing).unwrap();
}

fn new_shape(id: u32) -> Arc<diacraft_doc::Shape> {
    DocBuilder::new()
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(id).cell("PinX", "1").text("t")))
        .build()
        .pages
        .get(0)
        .unwrap()
        .shapes
        .get(0)
        .unwrap()
        .clone()
}

fn push_shape(doc: &mut Document, page: usize, shape: Arc<diacraft_doc::Shape>) {
    let mut p = (**doc.pages.get(page).unwrap()).clone();
    p.shapes.push(shape);
    doc.pages.set(page, Arc::new(p)).unwrap();
}

#[test]
fn a_prefixed_part_with_a_byte_order_mark_and_the_older_namespace_is_patched_in_its_own_style() {
    // A page part in the older namespace, every element prefixed `v:`, with a BOM, a declaration, CR LF line ends and a comment after the root.
    let page = "\u{FEFF}<?xml version='1.0' encoding='utf-8'?>\r\n<v:PageContents xmlns:v='http://schemas.microsoft.com/office/visio/2011/1/core' xmlns:r='http://schemas.openxmlformats.org/officeDocument/2006/relationships' xml:space='preserve'>\r\n<v:Shapes>\r\n<v:Shape ID='1'><v:Cell N='PinX' V='1'/></v:Shape>\r\n</v:Shapes>\r\n</v:PageContents>\r\n<!-- after -->";
    let original = VsdxBuilder::new().namespace(Namespace::Core2011).page(PageSpec::new(1, "A").part(page)).build();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    set(&mut doc, 1, 0, "PinX", "9");
    push_shape(&mut doc, 0, new_shape(7));
    let written = save(&doc, &original, &baseline);
    let out = part(&written.bytes, "/visio/pages/page1.xml");
    assert!(out.starts_with("\u{FEFF}<?xml version='1.0' encoding='utf-8'?>\r\n<v:PageContents"), "the prolog is the original's: {out:?}");
    assert!(out.ends_with("</v:PageContents>\r\n<!-- after -->"), "the epilog is the original's: {out:?}");
    assert!(out.contains("<v:Cell N=\"PinX\" V=\"9\"/>"), "{out}");
    assert!(
        out.contains("<v:Shape ID=\"7\"><v:Cell N=\"PinX\" V=\"1\"/><v:Text>t</v:Text></v:Shape>"),
        "a new shape is written in the part's prefix: {out}"
    );
    // A parser reads CR LF between elements as LF (XML 1.0 section 2.11), so the white space of a part that is written again is LF; what the
    // part says outside its root (the declaration, the comment after it) is copied as it was.
    assert!(out.contains("\n</v:Shapes>"), "the white space around the shapes is the original's: {out:?}");
    // It reads back, in the same namespace, with the new shape.
    let (back, report) = read(&written.bytes, &diacraft_vsdx::Limits::default()).unwrap();
    assert_eq!(back.pages.get(0).unwrap().shapes.len(), 2);
    assert_eq!(report.namespaces.core_2011, 4, "{:?}", report.namespaces);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn two_pages_that_share_a_part_cannot_be_edited_in_one() {
    use diacraft_testkit::package::{CT_DRAWING, CT_PAGE, CT_PAGES, NS_2012, NS_R, VIS};
    // Two rows of the pages index with relationships to one part, built by hand.
    let root = |name: &str, inner: &str| format!("<{name} xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\" xml:space=\"preserve\">{inner}</{name}>");
    let pages = root("Pages", "<Page ID=\"1\" Name=\"A\"><Rel r:id=\"rId1\"/></Page><Page ID=\"2\" Name=\"B\"><Rel r:id=\"rId2\"/></Page>");
    let page = root("PageContents", &format!("<Shapes>{}</Shapes>", shape(1, &cell("PinX", "1"))));
    let original = Pkg::new()
        .default_type("rels", "application/vnd.openxmlformats-package.relationships+xml")
        .default_type("xml", "application/xml")
        .part("/visio/document.xml", CT_DRAWING, root("VisioDocument", "<StyleSheets><StyleSheet ID=\"0\" NameU=\"No Style\"/></StyleSheets>"))
        .rel("/", "rId1", &format!("{VIS}document"), "visio/document.xml")
        .part("/visio/pages/pages.xml", CT_PAGES, pages)
        .rel("/visio/document.xml", "rId1", &format!("{VIS}pages"), "pages/pages.xml")
        .part("/visio/pages/page1.xml", CT_PAGE, page)
        .rel("/visio/pages/pages.xml", "rId1", &format!("{VIS}page"), "page1.xml")
        .rel("/visio/pages/pages.xml", "rId2", &format!("{VIS}page"), "page1.xml")
        .build();
    let (doc, _) = read_ok(&original);
    assert_eq!(doc.pages.len(), 2);
    // Nothing edited: saved as it is.
    let same = save(&doc, &original, &doc);
    assert_eq!(part(&same.bytes, "/visio/pages/page1.xml"), part(&original, "/visio/pages/page1.xml"));
    // An edit to one of the two would have to be an edit to the other: refused, and nothing is written.
    let mut edited = doc.clone();
    set(&mut edited, 1, 0, "PinX", "2");
    let error = write_with(&edited, Some(&OriginalBytes { bytes: &original, baseline: Some(&doc) }), &WriteOptions::default()).unwrap_err();
    assert!(matches!(error, WriteError::Unsupported(_)), "{error}");
}

#[test]
fn a_page_of_a_hundred_thousand_shapes_is_patched_in_time_and_the_rest_is_kept() {
    let mut xml = String::new();
    for id in 1..=100_000u32 {
        xml.push_str(&shape(id, &format!("{}{}", cell("PinX", &id.to_string()), cell("PinY", "1"))));
    }
    let original =
        VsdxBuilder::new().page(PageSpec::new(1, "Big").shapes(&xml)).page(PageSpec::new(2, "Small").shapes(&shape(1, &cell("PinX", "1")))).build();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    set(&mut doc, 1, 54_321, "PinX", "-1");
    push_shape(&mut doc, 0, new_shape(100_001));
    let started = Instant::now();
    let written = save(&doc, &original, &baseline);
    let took = started.elapsed();
    // A hang guard and no measure of cost (the save takes about a second at load 50): what was written is asserted below.
    assert!(took.as_secs_f64() < 60.0, "a save of a page of 100,000 shapes took {took:?}");
    println!("save of a page of 100,000 shapes, one cell and one shape: {took:?}");
    let out = part(&written.bytes, "/visio/pages/page1.xml");
    assert_eq!(out.matches("<Shape ").count(), 100_001);
    assert!(out.contains("V=\"-1\""));
    assert_eq!(part(&written.bytes, "/visio/pages/page2.xml"), part(&original, "/visio/pages/page2.xml"));
    assert_eq!(written.document.pages.get(0).unwrap().shapes.len(), 100_001);
}

#[test]
fn groups_nested_as_deep_as_xml_allows_are_written_and_deeper_ones_are_refused_not_overflowed() {
    // A group costs two levels of XML (`Shape` and `Shapes`) and the reader's cap on XML depth is 200, so about 98 levels of groups read. The model
    // allows 200, which no file can hold: the writer's own recursion is bounded by that, and what is deeper than a file can hold is refused by the
    // check that reads the file just made, and never returned.
    let nested = |levels: u32| {
        let mut leaf = ShapeBuilder::new(5000).cell("PinX", "1");
        for id in (1..=levels).rev() {
            leaf = ShapeBuilder::new(id).group().child(leaf);
        }
        DocBuilder::new().page(1, PageBuilder::new().shape(leaf)).build()
    };
    let handle = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let written = write_with(&nested(95), None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(written.read_back.stats.max_shape_depth, 96);
            let error = write_with(&nested(120), None, &WriteOptions::default()).unwrap_err();
            assert!(matches!(error, WriteError::SelfCheck(_)), "{error}");
            let error = write_with(&nested(250), None, &WriteOptions::default()).unwrap_err();
            assert!(matches!(error, WriteError::Limit(_)), "{error}");
        })
        .unwrap();
    handle.join().unwrap();
}

#[test]
fn a_shape_with_thousands_of_cells_and_a_section_of_thousands_of_rows_round_trips() {
    let mut s = ShapeBuilder::new(1);
    for i in 0..3000 {
        s = s.cell(&format!("User.Cell{i:05}"), &i.to_string());
    }
    for i in 0..20_000u32 {
        s = s.row_cell("Geometry", Some(0), diacraft_doc::RowKey::Ix(i + 1), "X", &i.to_string());
    }
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(s)).build();
    let written = write_with(&doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(written.document.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cells.len(), 3000);
    // Edit one row in the middle: patched through the row table, not a scan of 20,000.
    let mut edited = written.document.clone();
    let sheet = SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0));
    let addr = CellAddr::in_row("Geometry", Some(0), diacraft_doc::RowKey::Ix(10_000), "X");
    edited.write_cell(&sheet, &addr, CellEdit::Value(Value::from_text("-5")), WriteOrigin::Drawing).unwrap();
    let again = save(&edited, &written.bytes, &written.document);
    assert_eq!(part(&again.bytes, "/visio/pages/page1.xml").matches("V=\"-5\"").count(), 1);
}

// ----------------------------------------------------------------------------------------------------------------- generated

fn arb_text() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            Just('&'),
            Just('<'),
            Just('>'),
            Just('"'),
            Just('\''),
            Just('\n'),
            Just('\t'),
            Just(' '),
            Just('1'),
            Just('.'),
            Just('#'),
            any::<char>().prop_filter("xml char", |c| matches!(*c, '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}'))
        ],
        0..10,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Any values, formulas, names and texts: a new drawing writes and reads back as the model; the same drawing edited and written
    /// again reads back as the edited model, and a cell nobody wrote keeps the text it had.
    #[test]
    fn generated_drawings_and_edits_round_trip(
        values in proptest::collection::vec(arb_text(), 1..6),
        formulas in proptest::collection::vec(arb_text(), 1..4),
        title in "[A-Za-z0-9 &<>]{0,12}",
        extra in 0usize..4,
        change in arb_text(),
    ) {
        let mut shape = ShapeBuilder::new(1).named(&values[0], &values[0]);
        for (i, v) in values.iter().enumerate() {
            shape = shape.cell(&format!("User.V{i}"), v);
        }
        for (i, f) in formulas.iter().enumerate() {
            shape = shape.formula(&format!("User.F{i}"), f, "1");
        }
        shape = shape.text(&format!("t{}", values[0]));
        let mut doc = DocBuilder::new().page(1, PageBuilder::new().name("P").shape(shape)).build();
        doc.title = Arc::from(title.as_str());
        let written = write_with(&doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{e}"));
        prop_assert_eq!(written.document.pages.get(0).unwrap().shapes.get(0).unwrap(), doc.pages.get(0).unwrap().shapes.get(0).unwrap());
        prop_assert_eq!(&written.document.title, &doc.title);
        // Edit: a cell, and some more shapes.
        let mut edited = written.document.clone();
        let sheet = SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0));
        edited.write_cell(&sheet, &CellAddr::direct("User.V0"), CellEdit::Value(Value::from_text(&change)), WriteOrigin::Drawing).unwrap();
        for i in 0..extra {
            push_shape(&mut edited, 0, new_shape(10 + i as u32));
        }
        let again = save(&edited, &written.bytes, &written.document);
        let page = again.document.pages.get(0).unwrap();
        prop_assert_eq!(page.shapes.len(), 1 + extra);
        let first = page.shapes.get(0).unwrap();
        prop_assert_eq!(first.sheet.cells.get("User.V0").unwrap().value.as_ref().unwrap().text(), change.as_str());
        if values.len() > 1 {
            // The cells nobody wrote are as they were, to the character.
            prop_assert_eq!(first.sheet.cells.get("User.V1"), written.document.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cells.get("User.V1"));
        }
    }
}
