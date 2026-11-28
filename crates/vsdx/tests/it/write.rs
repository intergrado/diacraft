//! Saving: a new drawing is written whole, a drawing that was read is patched, and what an edit did not touch is the original's.
//!
//! Every input is built in code. The vectors are those.

use std::collections::BTreeMap;
use std::sync::Arc;

use diacraft_doc::{
    CellAddr, CellEdit, Connect, Document, Master, Origin, Owner, RawAttrs, RowKey, ShapeRef, ShapeType, SheetRef, TextPart, Value, WriteOrigin,
};
use diacraft_opc::xml;
use diacraft_opc::{Limits, Package};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};
use diacraft_testkit::package::{self, OD, PK, PageSpec, Pkg, VIS, VsdxBuilder, cell, shape, shape_with};
use diacraft_vsdx::{Kind, OriginalBytes, Variant, WriteError, WriteOptions, Written, read, write_with};

use crate::read_ok;

const NOW: i64 = 1_700_000_000;

fn options() -> WriteOptions {
    WriteOptions { now: Some(NOW), ..WriteOptions::default() }
}

fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let pkg = Package::read(bytes, &Limits::default()).unwrap();
    pkg.parts().map(|p| (p.to_owned(), pkg.part(p).unwrap().to_vec())).collect()
}

fn text_of(parts: &BTreeMap<String, Vec<u8>>, name: &str) -> String {
    String::from_utf8(parts.get(name).unwrap_or_else(|| panic!("no part {name}: {:?}", parts.keys())).clone()).unwrap()
}

/// A document with what the writer assigns (relationship ids) and what only the file knows (the times) cleared, to compare models.
fn same_model(a: &Document, b: &Document) -> Document {
    let mut doc = a.clone();
    doc.props = b.props.clone();
    // What a file holds of a page (its part's name, and what its root and its elements hold that no field does) is the file's, as the ids are.
    for (i, other) in b.pages.iter().enumerate() {
        if let Some(slot) = doc.pages.get_mut(i) {
            let page = Arc::make_mut(slot);
            page.rel_id = other.rel_id.clone();
            page.part_name = other.part_name.clone();
            page.ext = other.ext.clone();
        }
    }
    for (i, other) in b.masters.iter().enumerate() {
        if let Some(slot) = doc.masters.get_mut(i) {
            let master = Arc::make_mut(slot);
            master.rel_id = other.rel_id.clone();
            master.part_name = other.part_name.clone();
            master.ext = other.ext.clone();
        }
    }
    doc
}

fn assert_same_model(written: &Document, expected: &Document) {
    let want = same_model(expected, written);
    assert_eq!(written.title, want.title);
    assert_eq!(written.pages.len(), want.pages.len());
    for (w, e) in written.pages.iter().zip(want.pages.iter()) {
        assert_eq!(w, e, "page {}", e.id);
    }
    assert_eq!(written.masters.len(), want.masters.len());
    for (w, e) in written.masters.iter().zip(want.masters.iter()) {
        assert_eq!(w, e, "master {}", e.id);
    }
    assert_eq!(written.styles, want.styles);
    assert_eq!(written.sheet, want.sheet);
    assert_eq!(written.colors, want.colors);
    assert_eq!(written.fonts, want.fonts);
}

fn write_new(doc: &Document) -> Written {
    write_with(doc, None, &options()).unwrap_or_else(|e| panic!("{e}"))
}

fn save(doc: &Document, original: &[u8], baseline: Option<&Document>) -> Written {
    write_with(doc, Some(&OriginalBytes { bytes: original, baseline }), &options()).unwrap_or_else(|e| panic!("{e}"))
}

fn cell_at(doc: &mut Document, sheet: SheetRef, name: &str, value: &str) {
    doc.write_cell(&sheet, &CellAddr::direct(name), CellEdit::Value(Value::from_text(value)), WriteOrigin::Drawing).unwrap();
}

// ----------------------------------------------------------------------------------------------------------------- new drawings

#[test]
fn a_new_drawing_holds_exactly_the_parts_the_spec_lists_and_reads_back_with_no_warning() {
    let doc = Document::new_blank();
    let written = write_new(&doc);
    let p = parts(&written.bytes);
    let names: Vec<&str> = p.keys().map(String::as_str).collect();
    let mut want = vec![
        "/_rels/.rels",
        "/docProps/app.xml",
        "/docProps/core.xml",
        "/docProps/custom.xml",
        "/visio/_rels/document.xml.rels",
        "/visio/document.xml",
        "/visio/pages/_rels/pages.xml.rels",
        "/visio/pages/page1.xml",
        "/visio/pages/pages.xml",
        "/visio/windows.xml",
    ];
    want.sort_unstable();
    assert_eq!(names, want, "V15 lists eleven items; [Content_Types].xml is the eleventh and is not a part");
    assert!(written.read_back.warnings.is_empty(), "{:?}", written.read_back.warnings);
    assert!(written.warnings.is_empty(), "{:?}", written.warnings);
    // The package relationships: the document and three property relationships.
    let rels = Package::read(&written.bytes, &Limits::default()).unwrap().rels("/").unwrap();
    assert_eq!(rels.len(), 4);
    assert_eq!(rels.of_type(&format!("{VIS}document")).count(), 1);
    // The document's: pages and windows. The pages part: one page relationship.
    let pkg = Package::read(&written.bytes, &Limits::default()).unwrap();
    assert_eq!(pkg.rels("/visio/document.xml").unwrap().len(), 2);
    let page_rels = pkg.rels("/visio/pages/pages.xml").unwrap();
    assert_eq!(page_rels.len(), 1);
    assert_eq!(page_rels.of_type(&format!("{VIS}page")).count(), 1);
    // V2 of the document spec: new parts are in 2012/main. The root style, and `xml:space="preserve"`.
    let document = text_of(&p, "/visio/document.xml");
    assert!(document.contains("http://schemas.microsoft.com/office/visio/2012/main") && document.contains("xml:space=\"preserve\""), "{document}");
    assert!(document.contains("<StyleSheet ID=\"0\"") && document.contains("NameU=\"No Style\""), "{document}");
    assert_eq!(read_ok(&written.bytes).0.styles.len(), 1);
    // The writer's name. The build number's low 16 bits are above 2714.
    let app = text_of(&p, "/docProps/app.xml");
    assert!(app.contains("<Application>DiaCraft</Application>") && !app.contains("Microsoft"), "{app}");
    assert!(app.contains("<vt:lpstr>Page-1</vt:lpstr>"), "{app}");
    let custom = text_of(&p, "/docProps/custom.xml");
    let build: i64 = custom.split("<vt:i4>").nth(1).unwrap().split('<').next().unwrap().parse().unwrap();
    assert!(build & 0xFFFF > 2714, "{build}");
    assert_eq!(written.read_back.variant, Some(Variant { kind: Kind::Drawing, macro_enabled: false }));
    assert_same_model(&written.document, &doc);
}

#[test]
fn a_new_drawing_with_masters_shapes_text_and_connects_reads_back_as_the_model() {
    let nasty = "A & B <C> \"D\" 'E'\nF\tG  two  spaces  ";
    let master = MasterBuilder::new().name("Box").cell("Width", "1").shape(
        ShapeBuilder::new(4)
            .cell_u("Width", "2", "IN")
            .formula("Height", "Width*0.5", "1")
            .inh("LineWeight", Some("0.02"))
            .row_cell("Geometry", Some(0), RowKey::Ix(1), "X", "0")
            .row_formula("Geometry", Some(0), RowKey::Ix(2), "X", "Width*1", "2")
            .row_type("Geometry", Some(0), RowKey::Ix(1), "MoveTo")
            .row_type("Geometry", Some(0), RowKey::Ix(2), "LineTo")
            .row_cell("Connection", None, RowKey::Ix(0), "X", "1"),
    );
    let page = PageBuilder::new()
        .name("Main & <Page>")
        .cell_u("PageWidth", "11", "IN")
        .shape(ShapeBuilder::new(1).master(1).named("Box.1", "Box.1").cell("PinX", "3.50").formula("PinY", "Sheet.2!PinY", "4").text(nasty).set(
            CellAddr::in_row("User", None, RowKey::name("Mine"), "Value"),
            // (A text that says no unit is given `STR` by the writers, `string_units.rs`: the drawing a test expects back says it.)
            diacraft_doc::Cell::constant(Value::from_text("\"a\"")).with_unit("STR"),
        ))
        .shape(ShapeBuilder::new(2).group().cell("PinX", "1").child(ShapeBuilder::new(3).text_parts(vec![
            TextPart::Cp(0),
            TextPart::Chars(Arc::from("one")),
            TextPart::Pp(1),
            TextPart::Chars(Arc::from("two")),
            TextPart::Fld { ix: 0, text: Arc::from("3") },
        ])))
        .shape(ShapeBuilder::new(7).deleted());
    let mut doc = DocBuilder::new()
        .root(|r| r.cell("LineWeight", "0.01").cell("FillForegnd", "1"))
        .style(1, StyleBuilder::new().name("Normal").links(Some(0), Some(0), Some(0)))
        .master(1, master)
        .page(1, page)
        .page(2, PageBuilder::new().name("Second").back_page(1))
        .doc_cell("OutputFormat", "0")
        .build();
    doc.title = Arc::from("A title & more");
    let mut page = (**doc.pages.get(0).unwrap()).clone();
    page.connects.push(Connect {
        from_sheet: 1,
        from_cell: Some(Arc::from("BeginX")),
        from_part: Some(9),
        to_sheet: 2,
        to_cell: Some(Arc::from("PinX")),
        to_part: Some(3),
        raw: RawAttrs::new(),
        ext: Default::default(),
    });
    doc.pages.set(0, Arc::new(page)).unwrap();
    let written = write_new(&doc);
    assert!(written.read_back.warnings.is_empty(), "{:?}", written.read_back.warnings);
    assert_same_model(&written.document, &doc);
    let p = parts(&written.bytes);
    assert!(p.contains_key("/visio/masters/masters.xml") && p.contains_key("/visio/masters/master1.xml"), "{:?}", p.keys());
    assert!(p.contains_key("/visio/pages/page1.xml") && p.contains_key("/visio/pages/page2.xml"));
    // R22 of the masters spec: the page that uses master 1 has a `master` relationship, and the page that only has a background has a `page` one.
    let pkg = Package::read(&written.bytes, &Limits::default()).unwrap();
    let rels = pkg.rels("/visio/pages/page1.xml").unwrap();
    assert_eq!(rels.of_type(&format!("{VIS}master")).count(), 1, "{rels:?}");
    assert_eq!(pkg.rels("/visio/pages/page2.xml").unwrap().of_type(&format!("{VIS}page")).count(), 1);
    let app = text_of(&p, "/docProps/app.xml");
    assert!(
        app.contains("Main &amp; &lt;Page&gt;") && app.contains("<vt:lpstr>Second</vt:lpstr>") && app.contains("<vt:lpstr>Box</vt:lpstr>"),
        "{app}"
    );
    // A second save of what was saved changes nothing: the baseline is the file's own model.
    let again =
        write_with(&written.document, Some(&OriginalBytes { bytes: &written.bytes, baseline: Some(&written.document) }), &WriteOptions::default())
            .unwrap();
    let a = parts(&again.bytes);
    for (name, bytes) in &p {
        if name != "/docProps/app.xml" {
            assert_eq!(a.get(name), Some(bytes), "{name}");
        }
    }
}

#[test]
fn every_variant_is_written_and_read_back_and_a_save_as_changes_only_the_document_type() {
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1).cell("PinX", "1"))).build();
    let base = write_new(&doc).bytes;
    let base_parts = parts(&base);
    for ext in ["vsdx", "vsdm", "vssx", "vssm", "vstx", "vstm"] {
        let variant = Variant::from_extension(ext).unwrap();
        // A new drawing written as this variant.
        let new = write_with(&doc, None, &WriteOptions { variant: Some(variant), ..options() }).unwrap();
        assert_eq!(new.read_back.variant, Some(variant), "{ext}");
        assert!(new.read_back.warnings.is_empty(), "{ext}: {:?}", new.read_back.warnings);
        // V9 of the package spec: a save as of a file read keeps every part and changes only the document type among the content types.
        let saved = write_with(
            &read_ok(&base).0,
            Some(&OriginalBytes { bytes: &base, baseline: None }),
            &WriteOptions { variant: Some(variant), ..WriteOptions::default() },
        )
        .unwrap();
        assert_eq!(saved.read_back.variant, Some(variant), "{ext}");
        let p = parts(&saved.bytes);
        for (name, bytes) in &base_parts {
            if name != "/docProps/app.xml" {
                assert_eq!(p.get(name), Some(bytes), "{ext}: {name}");
            }
        }
        let before = Package::read(&base, &Limits::default()).unwrap();
        let after = Package::read(&saved.bytes, &Limits::default()).unwrap();
        let mut a: Vec<_> = after.content_types().overrides.clone();
        let mut b: Vec<_> = before.content_types().overrides.clone();
        a.sort();
        b.sort();
        let differing: Vec<_> = a.iter().zip(&b).filter(|(x, y)| x != y).collect();
        assert_eq!(a.len(), b.len());
        assert_eq!(differing.len(), usize::from(ext != "vsdx"), "{ext}: {differing:?}");
        assert_eq!(after.content_types().defaults, before.content_types().defaults, "{ext}");
    }
}

// ----------------------------------------------------------------------------------------------------------- patching a file

/// A drawing with one page of two shapes, a master, an unknown part, an unused `Default`, a thumbnail and an unusual relationship id.
fn package_with_extras() -> Vec<u8> {
    let page_shapes = format!(
        "{}{}",
        shape_with(
            5,
            " x:mine=\"1\" xmlns:x=\"urn:example\" Master=\"1\"",
            &format!(
                "<x:note>hi</x:note>{}{}<?keep me?><!-- note -->{}",
                cell("PinX", "1"),
                r#"<Cell N="Width" V="1.50" F="Height*1.5" U="MM"/>"#,
                r#"<Cell N="LineColor" V="0" F="Inh"/>"#
            )
        ),
        shape(6, &cell("PinX", "2"))
    );
    VsdxBuilder::new()
        .page(PageSpec::new(1, "First").shapes(&page_shapes).rel("rId9", &format!("{OD}image"), "../media/image1.png"))
        .page(PageSpec::new(2, "Other").shapes(&shape(1, &cell("PinX", "9"))))
        .master(package::MasterSpec::new(1, "Box").shapes(&shape(4, &cell("Width", "1"))))
        .with(
            Pkg::new()
                .part_without_override("/foo/bar.dat", b"0123456789")
                .default_type("dat", "application/x-foo")
                .default_type("zzz", "application/x-z")
                .default_type("png", "image/png")
                .part_without_override("/visio/media/image1.png", b"\x89PNG.fake")
                .rel("/", "rId7", "urn:example:foo", "foo/bar.dat")
                .part_without_override("/docProps/thumbnail.emf", b"emf-bytes")
                .default_type("emf", "image/x-emf")
                .rel("/", "rId8", &format!("{PK}thumbnail"), "docProps/thumbnail.emf"),
        )
        .build()
}

fn page_cell(doc: &mut Document, page: u32, shape_at: u32, name: &str, value: &str) {
    cell_at(doc, SheetRef::Shape(ShapeRef::top(Owner::Page(page), shape_at)), name, value);
}

#[test]
fn one_cell_edited_changes_one_attribute_and_every_other_part_is_the_originals() {
    let original = package_with_extras();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    page_cell(&mut doc, 1, 0, "PinX", "7");
    let written = save(&doc, &original, Some(&baseline));
    let before = parts(&original);
    let after = parts(&written.bytes);
    // The same set of parts but for the thumbnail: an edit removes it and its relationship.
    let mut gone: Vec<&String> = before.keys().filter(|k| !after.contains_key(*k)).collect();
    gone.sort();
    assert_eq!(gone, ["/docProps/thumbnail.emf"]);
    assert_eq!(after.keys().filter(|k| !before.contains_key(*k)).count(), 0);
    // Bytes level: everything else is the same bytes, but for the parts the edit touched.
    let mut changed: Vec<&str> = after.iter().filter(|(k, v)| before.get(*k) != Some(*v)).map(|(k, _)| k.as_str()).collect();
    changed.sort_unstable();
    assert_eq!(changed, ["/_rels/.rels", "/docProps/app.xml", "/docProps/core.xml", "/visio/pages/page1.xml"], "{:?}", written.rewritten);
    assert_eq!(after.get("/foo/bar.dat"), before.get("/foo/bar.dat"), "V1 of the preservation spec: the unknown part's 10 bytes");
    assert_eq!(after.get("/visio/pages/page2.xml"), before.get("/visio/pages/page2.xml"));
    assert_eq!(after.get("/visio/media/image1.png"), before.get("/visio/media/image1.png"));
    // Tree level: the page part differs at the one cell. Parse both and compare: the cell's `V`, nothing else.
    let old = xml::parse(&before["/visio/pages/page1.xml"]).unwrap();
    let new = xml::parse(&after["/visio/pages/page1.xml"]).unwrap();
    let mut old_text = format!("{old:?}");
    old_text = old_text.replace("(\"V\", \"1\")", "(\"V\", \"7\")");
    // The first `PinX` of shape 5 is the only cell with V=1 and N=PinX there.
    assert_eq!(old_text.matches("(\"V\", \"7\")").count(), 1);
    let new_text = format!("{new:?}");
    assert_eq!(new_text, old_text, "the edit is one attribute and the tree is otherwise the original's");
    let page = text_of(&after, "/visio/pages/page1.xml");
    assert!(page.contains("V=\"1.50\"") && page.contains("F=\"Height*1.5\"") && page.contains("U=\"MM\""), "{page}");
    assert!(
        page.contains("x:mine=\"1\"") && page.contains("<x:note>hi</x:note>") && page.contains("<?keep me?>") && page.contains("<!-- note -->"),
        "{page}"
    );
    // Regenerated level: the unused `Default`, the unknown relationship and its `Id` (of the preservation spec).
    let pkg = Package::read(&written.bytes, &Limits::default()).unwrap();
    assert!(pkg.content_types().defaults.iter().any(|(e, t)| e == "zzz" && t == "application/x-z"));
    assert!(pkg.content_types().defaults.iter().any(|(e, t)| e == "dat" && t == "application/x-foo"));
    assert!(pkg.content_types().defaults.iter().any(|(e, _)| e == "emf"), "the thumbnail's Default stays");
    let rels = pkg.rels("/").unwrap();
    assert_eq!(rels.get("rId7").map(|r| (r.ty.as_str(), r.target_raw.as_str())), Some(("urn:example:foo", "foo/bar.dat")));
    assert!(rels.items.iter().all(|r| !r.ty.ends_with("thumbnail")));
    assert_eq!(pkg.rels("/visio/pages/page1.xml").unwrap().get("rId9").map(|r| r.target_raw.as_str()), Some("../media/image1.png"));
    // The model reads back as the edited one.
    assert_same_model(&written.document, &doc);
    assert_eq!(first_cell_value(&written.document), "7");
}

fn first_cell_value(doc: &Document) -> String {
    let shape = doc.pages.get(0).unwrap().shapes.get(0).unwrap().clone();
    shape.sheet.cell("PinX").unwrap().value.as_ref().unwrap().text().to_owned()
}

#[test]
fn a_save_with_no_edit_keeps_every_part_but_the_writers_name_and_keeps_the_thumbnail() {
    let original = package_with_extras();
    let (doc, _) = read_ok(&original);
    let written = save(&doc, &original, None);
    let before = parts(&original);
    let after = parts(&written.bytes);
    assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>(), "the same parts, thumbnail included");
    let changed: Vec<&str> = after.iter().filter(|(k, v)| before.get(*k) != Some(*v)).map(|(k, _)| k.as_str()).collect();
    assert_eq!(changed, ["/docProps/app.xml"], "Nothing else differs");
    let app = text_of(&after, "/docProps/app.xml");
    assert!(app.contains("<Application>DiaCraft</Application>") && !app.contains("Test Writer"), "{app}");
    assert!(app.contains("<AppVersion>16.0000</AppVersion>"), "every other element is kept: {app}");
    // The content types and relationships are the same rows.
    let (a, b) = (Package::read(&original, &Limits::default()).unwrap(), Package::read(&written.bytes, &Limits::default()).unwrap());
    let mut x = a.content_types().overrides.clone();
    let mut y = b.content_types().overrides.clone();
    x.sort();
    y.sort();
    assert_eq!(x, y);
    assert_eq!(a.rels("/").unwrap().items, b.rels("/").unwrap().items);
    assert!(written.rewritten.iter().all(|p| p == "/docProps/app.xml"), "{:?}", written.rewritten);
}

#[test]
fn the_page_names_of_app_xml_follow_the_pages_and_only_those() {
    let app = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><Application>Another Drawing App</Application><HeadingPairs><vt:vector size=\"4\" baseType=\"variant\"><vt:variant><vt:lpstr>Seiten</vt:lpstr></vt:variant><vt:variant><vt:i4>1</vt:i4></vt:variant><vt:variant><vt:lpstr>Masters</vt:lpstr></vt:variant><vt:variant><vt:i4>1</vt:i4></vt:variant></vt:vector></HeadingPairs><TitlesOfParts><vt:vector size=\"2\" baseType=\"lpstr\"><vt:lpstr>A</vt:lpstr><vt:lpstr>Box</vt:lpstr></vt:vector></TitlesOfParts></Properties>";
    let original = VsdxBuilder::new()
        .app(app)
        .page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1"))))
        .master(package::MasterSpec::new(1, "Box").shapes(&shape(4, &cell("Width", "1"))))
        .build();
    let (mut doc, _) = read_ok(&original);
    doc.rename_page(1, "B").unwrap();
    let written = save(&doc, &original, None);
    let after = text_of(&parts(&written.bytes), "/docProps/app.xml");
    assert!(after.contains("<vt:lpstr>B</vt:lpstr>") && !after.contains("<vt:lpstr>A</vt:lpstr>"), "{after}");
    assert!(after.contains("<vt:lpstr>Seiten</vt:lpstr>") && after.contains("<vt:lpstr>Box</vt:lpstr>"), "the label is left as it is: {after}");
    assert!(after.contains("<Application>DiaCraft</Application>"));
    assert_eq!(read_ok(&written.bytes).0.pages.get(0).unwrap().name.as_ref(), "B");
}

#[test]
fn shapes_added_taken_out_retexted_and_connected_in_a_page_that_was_read() {
    let original = package_with_extras();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    let mut page = (**doc.pages.get(0).unwrap()).clone();
    // A new shape after the others, with a section and a row; shape 6 taken out; shape 5's text made.
    let added = ShapeBuilder::new(8)
        .cell("PinX", "4")
        .formula("PinY", "PinX*2", "8")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "X", "0")
        .text("hello <world>");
    let mut tmp = DocBuilder::new().page(1, PageBuilder::new().shape(added)).build();
    let new_shape = tmp.pages.get_mut(0).unwrap().shapes.get(0).unwrap().clone();
    let mut shapes: Vec<_> = page.shapes.iter().cloned().collect();
    shapes.remove(1);
    shapes.push(new_shape);
    page.shapes = shapes.into_iter().collect();
    page.connects.push(Connect {
        from_sheet: 8,
        from_cell: Some(Arc::from("BeginX")),
        from_part: Some(9),
        to_sheet: 5,
        to_cell: Some(Arc::from("PinX")),
        to_part: Some(3),
        raw: RawAttrs::new(),
        ext: Default::default(),
    });
    doc.pages.set(0, Arc::new(page)).unwrap();
    {
        let first = doc.pages.get(0).unwrap().shapes.get(0).unwrap().clone();
        let mut s = (*first).clone();
        s.text = Some(Arc::new(diacraft_doc::Text { parts: vec![TextPart::Chars(Arc::from("typed"))], ..Default::default() }));
        let mut page = (**doc.pages.get(0).unwrap()).clone();
        page.shapes.set(0, Arc::new(s)).unwrap();
        doc.pages.set(0, Arc::new(page)).unwrap();
    }
    let written = save(&doc, &original, Some(&baseline));
    assert_same_model(&written.document, &doc);
    let after = parts(&written.bytes);
    let page_xml = text_of(&after, "/visio/pages/page1.xml");
    assert!(page_xml.contains("x:mine=\"1\"") && page_xml.contains("<x:note>hi</x:note>") && page_xml.contains("<?keep me?>"), "{page_xml}");
    assert!(page_xml.contains("hello &lt;world&gt;") && page_xml.contains("<Connects>"), "{page_xml}");
    // Only the page part and the housekeeping parts changed.
    let before = parts(&original);
    let mut changed: Vec<&str> = after.iter().filter(|(k, v)| before.get(*k) != Some(*v)).map(|(k, _)| k.as_str()).collect();
    changed.sort_unstable();
    assert_eq!(changed, ["/_rels/.rels", "/docProps/app.xml", "/docProps/core.xml", "/visio/pages/page1.xml"]);
}

#[test]
fn a_page_added_and_a_page_taken_out_with_what_only_it_reached() {
    let original = package_with_extras();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    // The only page that uses `image1.png` is taken out; the unknown part stays; a page is added.
    doc.pages.remove(0).unwrap();
    let id = doc.add_page(Some("Third")).unwrap();
    let written = save(&doc, &original, Some(&baseline));
    assert_same_model(&written.document, &doc);
    let after = parts(&written.bytes);
    assert!(!after.contains_key("/visio/pages/page1.xml") && !after.contains_key("/visio/pages/_rels/page1.xml.rels"));
    assert!(!after.contains_key("/visio/media/image1.png"), "a picture only the removed page reached goes with it: {:?}", after.keys());
    assert!(after.contains_key("/foo/bar.dat"), "an unknown part stays");
    assert!(after.contains_key("/visio/pages/page2.xml"), "the page that stays");
    assert!(after.contains_key("/visio/pages/page3.xml"), "{:?}", after.keys());
    assert_eq!(id, 3);
    let pkg = Package::read(&written.bytes, &Limits::default()).unwrap();
    assert_eq!(pkg.rels("/visio/pages/pages.xml").unwrap().len(), 2);
    assert!(pkg.content_type("/visio/pages/page3.xml").is_some_and(|t| t.ends_with("page+xml")));
    assert!(pkg.content_type("/visio/pages/page1.xml").is_none() || !after.contains_key("/visio/pages/page1.xml"));
    assert!(pkg.warnings().is_empty(), "{:?}", pkg.warnings());
}

#[test]
fn a_master_added_to_a_file_that_has_masters_and_one_that_has_none() {
    // With masters already.
    let original = package_with_extras();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    doc.add_master(Master { id: 2, name: Some(Arc::from("Circle")), name_u: Some(Arc::from("Circle")), ..Master::default() }).unwrap();
    {
        let mut m = (**doc.masters.get(1).unwrap()).clone();
        m.shapes = DocBuilder::new()
            .master(2, MasterBuilder::new().shape(ShapeBuilder::new(4).cell("Width", "1")))
            .build()
            .masters
            .get(0)
            .unwrap()
            .shapes
            .clone();
        doc.masters.set(1, Arc::new(m)).unwrap();
    }
    // An instance of it on page 2: the page's relationships gain a `master` one (V8 of the masters spec).
    let inst = DocBuilder::new()
        .page(2, PageBuilder::new().shape(ShapeBuilder::new(9).master(2).cell("PinX", "1")))
        .build()
        .pages
        .get(0)
        .unwrap()
        .shapes
        .get(0)
        .unwrap()
        .clone();
    {
        let mut page = (**doc.pages.get(1).unwrap()).clone();
        let mut list: Vec<_> = page.shapes.iter().cloned().collect();
        list.push(inst);
        page.shapes = list.into_iter().collect();
        doc.pages.set(1, Arc::new(page)).unwrap();
    }
    let written = save(&doc, &original, Some(&baseline));
    assert_same_model(&written.document, &doc);
    let pkg = Package::read(&written.bytes, &Limits::default()).unwrap();
    assert!(pkg.part("/visio/masters/master2.xml").is_some(), "{:?}", pkg.parts().collect::<Vec<_>>());
    let rels = pkg.rels("/visio/pages/page2.xml").unwrap();
    assert_eq!(rels.of_type(&format!("{VIS}master")).count(), 1, "{rels:?}");
    assert!(pkg.warnings().is_empty());

    // With none: the index, its relationship and its type are made.
    let bare = VsdxBuilder::new().without_masters_part().page(PageSpec::new(1, "A")).build();
    let (mut doc, _) = read_ok(&bare);
    doc.add_master(Master { id: 1, name: Some(Arc::from("M")), ..Master::default() }).unwrap();
    let written = save(&doc, &bare, None);
    assert_eq!(written.document.masters.len(), 1);
    let pkg = Package::read(&written.bytes, &Limits::default()).unwrap();
    assert!(pkg.part("/visio/masters/masters.xml").is_some() && pkg.part("/visio/masters/master1.xml").is_some());
}

#[test]
fn a_package_with_a_macro_project_is_not_saved_to_a_type_that_cannot_hold_it() {
    let vba = |ty: &str| {
        let pkg = Pkg::new().part("/visio/vbaProject.bin", "application/vnd.ms-office.vbaProject", b"VBA-BYTES").rel(
            "/visio/document.xml",
            "rId50",
            "http://schemas.microsoft.com/office/2006/relationships/vbaProject",
            "vbaProject.bin",
        );
        VsdxBuilder::new().document_type(ty).page(PageSpec::new(1, "A")).with(pkg).build()
    };
    let original = vba("application/vnd.ms-visio.drawing.macroEnabled.main+xml");
    let (doc, _) = read_ok(&original);
    let to_vsdx = write_with(
        &doc,
        Some(&OriginalBytes { bytes: &original, baseline: None }),
        &WriteOptions { variant: Some(Variant::from_extension("vsdx").unwrap()), ..options() },
    );
    assert_eq!(to_vsdx.unwrap_err(), WriteError::MacroProject, "an error, and nothing is written");
    // Within the macro types the project is kept as it is (V7 of the preservation spec).
    let mut edited = doc.clone();
    page_cell_new_shape(&mut edited);
    let written = save(&edited, &original, Some(&doc));
    assert_eq!(parts(&written.bytes).get("/visio/vbaProject.bin").map(Vec::as_slice), Some(&b"VBA-BYTES"[..]));
    let as_vssm = write_with(
        &doc,
        Some(&OriginalBytes { bytes: &original, baseline: None }),
        &WriteOptions { variant: Variant::from_extension("vssm"), ..options() },
    )
    .unwrap();
    assert_eq!(parts(&as_vssm.bytes).get("/visio/vbaProject.bin").map(Vec::as_slice), Some(&b"VBA-BYTES"[..]));
}

fn page_cell_new_shape(doc: &mut Document) {
    let mut page = (**doc.pages.get(0).unwrap()).clone();
    let mut list: Vec<_> = page.shapes.iter().cloned().collect();
    list.push(
        DocBuilder::new()
            .page(1, PageBuilder::new().shape(ShapeBuilder::new(3).cell("PinX", "1")))
            .build()
            .pages
            .get(0)
            .unwrap()
            .shapes
            .get(0)
            .unwrap()
            .clone(),
    );
    page.shapes = list.into_iter().collect();
    doc.pages.set(0, Arc::new(page)).unwrap();
}

#[test]
fn an_edit_the_writer_cannot_write_yet_is_an_error_and_writes_nothing() {
    let original = VsdxBuilder::new().page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1")))).build();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    // A change to `Data1` of a shape.
    let mut s = (**doc.pages.get(0).unwrap().shapes.get(0).unwrap()).clone();
    s.data1 = Some(Arc::from("changed"));
    let mut page = (**doc.pages.get(0).unwrap()).clone();
    page.shapes.set(0, Arc::new(s)).unwrap();
    doc.pages.set(0, Arc::new(page)).unwrap();
    let error = write_with(&doc, Some(&OriginalBytes { bytes: &original, baseline: Some(&baseline) }), &options()).unwrap_err();
    assert!(matches!(error, WriteError::Unsupported(_)), "{error}");
    assert_eq!(error.class(), "write_unsupported");
    // A document that is not the file's: the baseline says a shape the file lacks.
    let other = VsdxBuilder::new().page(PageSpec::new(1, "A")).build();
    let (mut wrong, _) = read_ok(&other);
    page_cell_new_shape(&mut wrong);
    let error = write_with(&doc, Some(&OriginalBytes { bytes: &original, baseline: Some(&wrong) }), &options()).unwrap_err();
    assert!(matches!(error, WriteError::Mismatch(_) | WriteError::Unsupported(_)), "{error}");
}

#[test]
fn a_character_xml_cannot_hold_is_an_error_and_not_a_damaged_part() {
    let original = VsdxBuilder::new().page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1")))).build();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    page_cell(&mut doc, 1, 0, "PinX", "a\u{1}b");
    let error = write_with(&doc, Some(&OriginalBytes { bytes: &original, baseline: Some(&baseline) }), &options()).unwrap_err();
    assert_eq!(error.class(), "xml_not_writable", "{error}");
    let mut new = Document::new_blank();
    new.title = Arc::from("tab\u{0}");
    assert_eq!(write_with(&new, None, &options()).unwrap_err().class(), "xml_not_writable");
}

#[test]
fn recalc_document_is_added_once_and_found_by_name() {
    let original = package_with_extras();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    page_cell(&mut doc, 1, 0, "PinX", "7");
    let ask = WriteOptions { recalc_document: true, ..options() };
    let first = write_with(&doc, Some(&OriginalBytes { bytes: &original, baseline: Some(&baseline) }), &ask).unwrap();
    let custom = text_of(&parts(&first.bytes), "/docProps/custom.xml");
    assert_eq!(custom.matches("name=\"RecalcDocument\"").count(), 1, "{custom}");
    assert!(custom.contains("<vt:bool>true</vt:bool>"));
    // A second save with the request adds nothing (V9 of the preservation spec).
    let second = write_with(&first.document, Some(&OriginalBytes { bytes: &first.bytes, baseline: Some(&first.document) }), &ask).unwrap();
    assert_eq!(text_of(&parts(&second.bytes), "/docProps/custom.xml").matches("RecalcDocument").count(), 1);
    // And without the request nothing is added.
    let none = save(&doc, &original, Some(&baseline));
    assert!(
        !parts(&none.bytes).contains_key("/docProps/custom.xml"),
        "a file with no custom properties gets none unless a recalculation is asked for"
    );
}

#[test]
fn the_written_package_has_agreeing_headers_and_only_stored_or_deflated_entries() {
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1).cell("PinX", "1"))).build();
    let zip = write_new(&doc).bytes;
    let end = zip.len() - 22;
    assert_eq!(u32::from_le_bytes(zip[end..end + 4].try_into().unwrap()), 0x0605_4B50);
    let count = u16::from_le_bytes(zip[end + 10..end + 12].try_into().unwrap()) as usize;
    let mut at = u32::from_le_bytes(zip[end + 16..end + 20].try_into().unwrap()) as usize;
    for _ in 0..count {
        let method = u16::from_le_bytes(zip[at + 10..at + 12].try_into().unwrap());
        assert!(method == 0 || method == 8);
        let local = u32::from_le_bytes(zip[at + 42..at + 46].try_into().unwrap()) as usize;
        // CRC, both sizes and the method agree between the two headers.
        assert_eq!(zip[at + 10..at + 12], zip[local + 8..local + 10]);
        assert_eq!(zip[at + 16..at + 28], zip[local + 14..local + 26]);
        let name_len = u16::from_le_bytes(zip[at + 28..at + 30].try_into().unwrap()) as usize;
        at += 46 + name_len;
    }
}

#[test]
fn a_saved_drawing_reads_back_through_the_reader_and_its_first_save_is_stable() {
    // The new drawing, written, read, and written again from what was read, gives the same page and document parts.
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1).cell("PinX", "1.50").text("x"))).build();
    let first = write_new(&doc);
    let (read_doc, _) = read(&first.bytes, &diacraft_vsdx::Limits::default()).unwrap();
    // The returned model is the file's, but for the unknown elements of the document, the font table and the root style, which are the caller's (the
    // skeleton's `DocumentSettings`, the default face it lists for a drawing that names none, and the root of a drawing that came from no file, to which
    // the writer adds the cells a reader would take as another value: the file holds them and the baseline of the next save does not, so that they are no edit).
    let mut expected = first.document.clone();
    assert!(expected.passthrough.is_empty() && read_doc.passthrough.len() == 1);
    assert!(expected.fonts.is_empty() && read_doc.fonts.len() == 1);
    assert!(
        read_doc.style(0).is_some_and(|root| root.sheet.cell("DefaultTabStop").is_some())
            && expected.style(0).is_some_and(|root| root.sheet.cell("DefaultTabStop").is_none()),
        "the file's root holds the cells added to it and the model given back the caller's"
    );
    expected.passthrough = read_doc.passthrough.clone();
    expected.fonts = read_doc.fonts.clone();
    expected.styles = read_doc.styles.clone();
    assert_eq!(read_doc, expected);
    let second = save(&read_doc, &first.bytes, None);
    assert_eq!(parts(&first.bytes).get("/visio/pages/page1.xml"), parts(&second.bytes).get("/visio/pages/page1.xml"));
    assert_eq!(parts(&first.bytes).get("/visio/document.xml"), parts(&second.bytes).get("/visio/document.xml"));
    let _ = Origin::Local;
}

/// Shapes are matched by `ID`, and a shape that has the `ID` of one the edit took out, but is another thing (another `Type`, another
/// master), is one taken out and one put in: it is not that shape patched, which would also have to change what the writer cannot write (a
/// picture, `Data1`). The `ID`s of the engine do not repeat now; a model from another source may.
#[test]
fn a_shape_that_has_the_id_of_a_removed_one_but_another_type_or_master_is_one_removed_and_one_added() {
    let picture = r#"<Shape ID="2" Type="Foreign"><Cell N="PinX" V="3"/><Data1>held</Data1></Shape>"#;
    let plain_with_data = r#"<Shape ID="2" Type="Shape" Master="3"><Cell N="PinX" V="3"/><Data1>held</Data1></Shape>"#;
    let master = package::MasterSpec::new(3, "M").shapes(&shape(1, &cell("Width", "2")));
    let master_4 = package::MasterSpec::new(4, "N").shapes(&shape(1, &cell("Width", "3")));
    // What the file holds, the new shape's type, and its master.
    let cases: [(&str, ShapeType, Option<u32>); 2] = [(picture, ShapeType::Shape, None), (plain_with_data, ShapeType::Shape, Some(4))];
    for (held, kind, master_id) in cases {
        let original = VsdxBuilder::new()
            .master(master.clone())
            .master(master_4.clone())
            .page(PageSpec::new(1, "A").shapes(&format!("{}{held}", shape(1, &cell("PinX", "1")))))
            .build();
        let (mut doc, _) = read_ok(&original);
        let baseline = doc.clone();
        let mut new = diacraft_doc::Shape::new(2);
        new.kind = kind.clone();
        new.master = master_id;
        let mut page = (**doc.pages.get(0).unwrap()).clone();
        page.shapes.set(1, Arc::new(new)).unwrap();
        doc.pages.set(0, Arc::new(page)).unwrap();
        let written = write_with(&doc, Some(&OriginalBytes { bytes: &original, baseline: Some(&baseline) }), &options())
            .unwrap_or_else(|e| panic!("{kind:?} {master_id:?}: {e}"));
        let page_xml = text_of(&parts(&written.bytes), "/visio/pages/page1.xml");
        assert_eq!(page_xml.matches("<Shape ID=\"2\"").count(), 1, "{page_xml}");
        assert!(!page_xml.contains("Data1") && !page_xml.contains("held"), "the old shape is gone: {page_xml}");
        let (again, _) = read_ok(&written.bytes);
        let shape_2 = again.pages.get(0).unwrap().shapes.get(1).unwrap().clone();
        assert_eq!((shape_2.id, shape_2.master, shape_2.kind.clone()), (2, master_id, kind));
    }
}

/// The same `Type` and master with other contents is still the same shape, patched: only a different kind of thing is a replacement.
#[test]
fn a_shape_with_the_same_id_type_and_master_is_patched_in_place() {
    let original = VsdxBuilder::new().page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1")))).build();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    page_cell(&mut doc, 1, 0, "PinX", "4");
    let written = save(&doc, &original, Some(&baseline));
    let before = text_of(&parts(&original), "/visio/pages/page1.xml");
    let after = text_of(&parts(&written.bytes), "/visio/pages/page1.xml");
    assert_eq!(after, before.replace(r#"V="1""#, r#"V="4""#));
}
