//! The model writer's classes: each is a vector built in code from the class's rule and construct, since no sample file crosses.
//! A class that is content is fixed and pinned here; one that is not is pinned as it is, with the reason beside it. F3, F4 and F7 were pinned as differences the
//! check counted; makes them the same content, and each now says so (the check's own tests are `canon_implied_rows.rs`).

use std::sync::Arc;

use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape};

use crate::model_writer::{diff, model_write, show, text_of, visit};
use crate::read_ok;

const PAGES: &str = concat!("/vi", "sio", "/pages/pages.xml");

/// A drawing of one page named `P` whose row in the pages index is `row` (the builder's `ID="1" Name="P" NameU="P"` replaced by it).
fn with_page_row(row: &str) -> Vec<u8> {
    VsdxBuilder::new()
        .page(PageSpec::new(1, "P").shapes(&shape(1, r#"<Cell N="PinX" V="1"/>"#)))
        .pkg()
        .map_parts(|name, bytes| {
            if name == PAGES {
                let text = String::from_utf8(bytes.clone()).unwrap();
                assert_eq!(text.matches(r#"ID="1" Name="P" NameU="P""#).count(), 1);
                *bytes = text.replacen(r#"ID="1" Name="P" NameU="P""#, row, 1).into_bytes();
            }
        })
        .build()
}

/// The `Page` element of the pages index of a package, as text.
fn page_row(bytes: &[u8]) -> String {
    let pages = text_of(bytes, PAGES);
    let start = pages.find("<Page ").unwrap();
    let end = start + pages[start..].find('>').unwrap();
    pages[start..=end].to_owned()
}

// ------------------------------------------------------------------------------------------------------------ F6: a page row with no Name

#[test]
fn a_page_row_with_no_name_is_written_with_none_while_its_name_is_the_one_read() {
    for row in [r#"ID="1" NameU="P""#, r#"ID="1""#] {
        let source = with_page_row(row);
        // The visit is the same content, and so is an edited save: the name is the one the row gave.
        let written = visit(row, &source);
        assert!(!page_row(&written.bytes).contains(" Name="), "{row}: {}", page_row(&written.bytes));
        let (doc, _) = read_ok(&source);
        let edited = model_write(&doc, false);
        assert!(!page_row(&edited.bytes).contains(" Name="), "{row}: {}", page_row(&edited.bytes));
        // It reads back as the model it was written from.
        let (back, _) = read_ok(&written.bytes);
        assert_eq!(back.pages.iter().next().unwrap().name, doc.pages.iter().next().unwrap().name);
    }
}

#[test]
fn a_page_renamed_since_is_written_with_its_name_and_a_row_that_had_one_keeps_it() {
    let source = with_page_row(r#"ID="1" NameU="P""#);
    let (mut doc, _) = read_ok(&source);
    if let Some(page) = doc.pages.get_mut(0) {
        Arc::make_mut(page).name = Arc::from("Q");
    }
    let written = model_write(&doc, false);
    assert!(page_row(&written.bytes).contains(r#"Name="Q""#), "{}", page_row(&written.bytes));
    let (back, _) = read_ok(&written.bytes);
    assert_eq!(&*back.pages.iter().next().unwrap().name, "Q");
    // The control: a row with a `Name`, even one equal to its `NameU`, keeps it.
    let named = with_page_row(r#"ID="1" Name="P" NameU="P""#);
    let written = visit("named", &named);
    assert!(page_row(&written.bytes).contains(r#"Name="P""#), "{}", page_row(&written.bytes));
    let d = diff(&named, &written.bytes);
    assert!(d.same(), "{}", show(&d));
}

// ------------------------------------------------------------------------------------------------------------ F8: a window, and a `Rel` spelt in another namespace

const WINDOWS: &str = concat!("/vi", "sio", "/windows.xml");
const DOCUMENT: &str = concat!("/vi", "sio", "/document.xml");
const CT_WINDOWS: &str = concat!("application/vnd.ms-vi", "sio", ".windows+xml");

/// A drawing of page 1 with a `windows.xml` of two windows: one on page 1, one on page 7, which the file does not have.
fn with_windows() -> Vec<u8> {
    let windows = concat!(
        r#"<?xml version="1.0" encoding="utf-8"?><Windows xmlns="http://schemas.microsoft.com/office/vi"#,
        r#"sio/2012/main"><Window ID="0" WindowType="Drawing" Page="1"/><Window ID="1" WindowType="Drawing" Page="7"/></Windows>"#,
    );
    VsdxBuilder::new()
        .page(PageSpec::new(1, "P").shapes(&shape(1, r#"<Cell N="PinX" V="1"/>"#)))
        .with(diacraft_testkit::package::Pkg::new().part(WINDOWS, CT_WINDOWS, windows).rel(
            DOCUMENT,
            "rId30",
            &format!("{}windows", diacraft_testkit::package::VIS),
            "windows.xml",
        ))
        .build()
}

#[test]
fn a_window_on_a_page_the_file_does_not_have_stays_while_the_drawing_is_as_it_was_read() {
    let source = with_windows();
    let written = visit("windows", &source);
    assert_eq!(text_of(&written.bytes, WINDOWS).matches("<Window ").count(), 2);
    // After an edit the model cannot tell a page deleted since from one never there: the window on page 7 goes, the other stays.
    let (doc, _) = read_ok(&source);
    let edited = model_write(&doc, false);
    let windows = text_of(&edited.bytes, WINDOWS);
    assert_eq!(windows.matches("<Window ").count(), 1, "{windows}");
    assert!(windows.contains(r#"Page="1""#), "{windows}");
}

/// The package relationships namespace, which a file may bind `r` to on a part's root.
const PACKAGE_RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const OFFICE_RELS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PNG: [u8; 12] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4];

/// A page part whose root binds `r` to the package relationships namespace: shape 1's `Rel` is spelt `r:id` under that binding (no relationship id of this
/// format, so the reader keeps it as it is), and shape 2's declares `r` again as the relationships namespace and names a picture.
fn with_foreign_r() -> Vec<u8> {
    let root = concat!(r#"<PageContents xmlns="http://schemas.microsoft.com/office/vi"#, r#"sio/2012/main""#);
    let part = format!(
        concat!(
            r#"<?xml version="1.0"?>{root} xmlns:r="{pk}"><Shapes>"#,
            r#"<Shape ID="1" Type="Foreign"><Cell N="PinX" V="1"/><ForeignData ForeignType="Bitmap"><Rel r:id="rId9"/></ForeignData></Shape>"#,
            r#"<Shape ID="2" Type="Foreign"><Cell N="PinX" V="2"/><ForeignData ForeignType="Bitmap" CompressionType="PNG"><Rel xmlns:r="{od}" r:id="rId1"/></ForeignData></Shape>"#,
            r#"</Shapes></PageContents>"#,
        ),
        root = root,
        pk = PACKAGE_RELS,
        od = OFFICE_RELS,
    );
    let media = concat!("/vi", "sio", "/media/image1.png");
    VsdxBuilder::new()
        .page(PageSpec::new(1, "P").part(&part).rel("rId1", &format!("{}image", diacraft_testkit::package::OD), "../media/image1.png"))
        .with(diacraft_testkit::package::Pkg::new().part_without_override(media, PNG).default_type("png", "image/png"))
        .build()
}

#[test]
fn a_rel_spelt_under_a_root_that_binds_r_elsewhere_keeps_its_namespace_and_the_model_reads_back_the_same() {
    let source = with_foreign_r();
    let (doc, _) = read_ok(&source);
    let shapes = |d: &diacraft_doc::Document| d.pages.iter().next().unwrap().shapes.clone();
    let foreign = |d: &diacraft_doc::Document, at: usize| shapes(d).get(at).unwrap().foreign.clone().unwrap();
    assert_eq!(foreign(&doc, 0).rel_id, None, "the reader takes no relationship id from the other namespace");
    assert_eq!(foreign(&doc, 1).rel_id.as_deref(), Some("rId1"));
    let written = visit("r bound elsewhere", &source);
    let (back, _) = read_ok(&written.bytes);
    assert_eq!(foreign(&back, 0), foreign(&doc, 0), "the Rel took the relationships namespace in the written part");
    assert_eq!(foreign(&back, 1).rel_id.as_deref(), Some("rId1"));
    assert!(foreign(&back, 1).picture.is_some());
    // A `Rel` the writer makes in such a part, with nothing kept beside it, declares `r` for itself and is found again.
    let mut bare = doc.clone();
    if let Some(page) = bare.pages.get_mut(0)
        && let Some(shape) = Arc::make_mut(page).shapes.get_mut(1)
        && let Some(data) = Arc::make_mut(shape).foreign.as_mut()
    {
        Arc::make_mut(data).ext = Default::default();
    }
    let rewritten = model_write(&bare, false);
    let (again, _) = read_ok(&rewritten.bytes);
    assert_eq!(foreign(&again, 1).rel_id.as_deref(), Some("rId1"), "{}", text_of(&rewritten.bytes, concat!("/vi", "sio", "/pages/page1.xml")));
    assert!(foreign(&again, 1).picture.is_some());
    assert_eq!(foreign(&again, 0).rel_id, None);
}

// ------------------------------------------------------------------------------------------------------------ F5: property parts the package names and does not hold

/// A drawing whose package relationships name `docProps/core.xml`, `app.xml` and `custom.xml` and that holds none of them.
fn with_absent_properties() -> Vec<u8> {
    use diacraft_testkit::package::{CT_DRAWING, CT_PAGE, CT_PAGES, NS_2012, NS_R, OD, PK, Pkg, VIS};
    let dir = concat!("vi", "sio");
    let document = format!("<?xml version=\"1.0\"?><VisioDocument xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><DocumentSheet/></VisioDocument>");
    let pages = format!(
        "<?xml version=\"1.0\"?><Pages xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Page ID=\"0\" Name=\"P\" NameU=\"P\"><PageSheet/><Rel r:id=\"rId1\"/></Page></Pages>"
    );
    let page = format!(
        "<?xml version=\"1.0\"?><PageContents xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Shapes><Shape ID=\"1\"><Cell N=\"PinX\" V=\"1\"/></Shape></Shapes></PageContents>"
    );
    Pkg::new()
        .part(&format!("/{dir}/document.xml"), CT_DRAWING, document)
        .rel("/", "rId1", &format!("{VIS}document"), &format!("{dir}/document.xml"))
        .rel("/", "rId2", &format!("{PK}core-properties"), "docProps/core.xml")
        .rel("/", "rId3", &format!("{OD}extended-properties"), "docProps/app.xml")
        .rel("/", "rId4", &format!("{OD}custom-properties"), "docProps/custom.xml")
        .part(&format!("/{dir}/pages/pages.xml"), CT_PAGES, pages)
        .rel(&format!("/{dir}/document.xml"), "rId1", &format!("{VIS}pages"), "pages/pages.xml")
        .part(&format!("/{dir}/pages/page1.xml"), CT_PAGE, page)
        .rel(&format!("/{dir}/pages/pages.xml"), "rId1", &format!("{VIS}page"), "page1.xml")
        .build()
}

#[test]
fn a_property_part_the_package_names_and_does_not_hold_is_not_made_by_a_save_of_the_drawing_as_it_was_read() {
    let source = with_absent_properties();
    let written = visit("absent properties", &source);
    let names = crate::model_writer::parts(&written.bytes);
    for part in ["/docProps/core.xml", "/docProps/app.xml", "/docProps/custom.xml"] {
        assert!(!names.contains_key(part), "{part} was made: {:?}", names.keys().collect::<Vec<_>>());
    }
    // The rows stay as the file had them.
    let rels = String::from_utf8(names.get("/_rels/.rels").unwrap().clone()).unwrap();
    for target in ["docProps/core.xml", "docProps/app.xml", "docProps/custom.xml"] {
        assert!(rels.contains(target), "{rels}");
    }
    // An edited save writes its bookkeeping, and makes the core and app parts for it.
    let (doc, _) = read_ok(&source);
    let edited = model_write(&doc, false);
    let names = crate::model_writer::parts(&edited.bytes);
    assert!(names.contains_key("/docProps/core.xml") && names.contains_key("/docProps/app.xml"), "{:?}", names.keys().collect::<Vec<_>>());
    // The control: a package that names no property part gets none either way.
    let (plain, _) = read_ok(&with_absent_properties_unnamed());
    let names = crate::model_writer::parts(&model_write(&plain, true).bytes);
    assert!(!names.contains_key("/docProps/core.xml"), "{:?}", names.keys().collect::<Vec<_>>());
}

/// [`with_absent_properties`] with no row to a property part.
fn with_absent_properties_unnamed() -> Vec<u8> {
    use diacraft_testkit::package::{CT_DRAWING, CT_PAGE, CT_PAGES, NS_2012, NS_R, Pkg, VIS};
    let dir = concat!("vi", "sio");
    let document = format!("<?xml version=\"1.0\"?><VisioDocument xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><DocumentSheet/></VisioDocument>");
    let pages = format!(
        "<?xml version=\"1.0\"?><Pages xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Page ID=\"0\" Name=\"P\" NameU=\"P\"><PageSheet/><Rel r:id=\"rId1\"/></Page></Pages>"
    );
    let page = format!(
        "<?xml version=\"1.0\"?><PageContents xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Shapes><Shape ID=\"1\"><Cell N=\"PinX\" V=\"1\"/></Shape></Shapes></PageContents>"
    );
    Pkg::new()
        .part(&format!("/{dir}/document.xml"), CT_DRAWING, document)
        .rel("/", "rId1", &format!("{VIS}document"), &format!("{dir}/document.xml"))
        .part(&format!("/{dir}/pages/pages.xml"), CT_PAGES, pages)
        .rel(&format!("/{dir}/document.xml"), "rId1", &format!("{VIS}pages"), "pages/pages.xml")
        .part(&format!("/{dir}/pages/page1.xml"), CT_PAGE, page)
        .rel(&format!("/{dir}/pages/pages.xml"), "rId1", &format!("{VIS}page"), "page1.xml")
        .build()
}

// ------------------------------------------------------------------------------------------------------------ F4 and F7: not content, pinned

/// A handmade drawing of one page with the shape `shape`, the `extra` parts and rows added by `more`.
fn handmade(shape: &str, more: impl FnOnce(diacraft_testkit::package::Pkg) -> diacraft_testkit::package::Pkg) -> Vec<u8> {
    use diacraft_testkit::package::{CT_DRAWING, CT_PAGE, CT_PAGES, NS_2012, NS_R, Pkg, VIS};
    let dir = concat!("vi", "sio");
    let document = format!("<?xml version=\"1.0\"?><VisioDocument xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><DocumentSheet/></VisioDocument>");
    let pages = format!(
        "<?xml version=\"1.0\"?><Pages xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Page ID=\"0\" Name=\"P\" NameU=\"P\"><PageSheet/><Rel r:id=\"rId1\"/></Page></Pages>"
    );
    let page = format!("<?xml version=\"1.0\"?><PageContents xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Shapes>{shape}</Shapes></PageContents>");
    more(
        Pkg::new()
            .part(&format!("/{dir}/document.xml"), CT_DRAWING, document)
            .rel("/", "rId1", &format!("{VIS}document"), &format!("{dir}/document.xml"))
            .part(&format!("/{dir}/pages/pages.xml"), CT_PAGES, pages)
            .rel(&format!("/{dir}/document.xml"), "rId1", &format!("{VIS}pages"), "pages/pages.xml")
            .part(&format!("/{dir}/pages/page1.xml"), CT_PAGE, page)
            .rel(&format!("/{dir}/pages/pages.xml"), "rId1", &format!("{VIS}page"), "page1.xml"),
    )
    .build()
}

const BOX: &str = r#"<Shape ID="1"><Cell N="PinX" V="1"/></Shape>"#;

/// The differences of a visit (the package read and written from the model with its bookkeeping kept), as (part, path, kind).
fn visit_differences(source: &[u8]) -> Vec<(String, String, diacraft_container::canon::Found)> {
    diff(source, &visit_written(source)).shown.into_iter().map(|d| (d.part, d.path, d.found)).collect()
}

/// The package a visit writes.
fn visit_written(source: &[u8]) -> Vec<u8> {
    let (doc, report) = read_ok(source);
    let variant = report.variant.unwrap_or(diacraft_vsdx::Variant { kind: diacraft_vsdx::Kind::Drawing, macro_enabled: false });
    crate::model_writer::model_write_as(&doc, true, variant).bytes
}

#[test]
fn a_property_part_typed_as_the_drawing_is_written_with_its_own_type_and_is_the_same_content() {
    // Not content (makes it so for the check): the part's role is its relationship's, which the reader reads it by; the type the writer gives a
    // part it makes from fields is the type of what it is, and a file with two parts typed as the drawing names two main parts. The type is rewritten, and the check
    // sees no difference.
    use diacraft_testkit::package::{CT_DRAWING, OD};
    let app = r#"<?xml version="1.0"?><Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Company>C</Company></Properties>"#;
    let source =
        handmade(BOX, |p| p.part("/docProps/app.xml", CT_DRAWING, app).rel("/", "rId2", &format!("{OD}extended-properties"), "docProps/app.xml"));
    let written = visit_written(&source);
    let package = diacraft_opc::Package::read(&written, &diacraft_opc::Limits::default()).unwrap();
    assert_eq!(
        package.content_type("/docProps/app.xml"),
        Some("application/vnd.openxmlformats-officedocument.extended-properties+xml"),
        "the type is rewritten"
    );
    let found = visit_differences(&source);
    assert!(found.is_empty(), "{found:?}");
    let (doc, _) = read_ok(&source);
    assert_eq!(doc.props.company.as_deref(), Some("C"), "the part is read for what its relationship says it is");
}

#[test]
fn a_part_with_no_content_type_is_given_one_and_its_bytes_are_kept_and_is_the_same_content() {
    // Not content (makes it so for the check): every part of a package has a content type (ECMA-376 Part 2, 10.1.2.2), and a part a file left
    // without one is given one. Its bytes are the same, and the check sees no difference.
    let source = handmade(BOX, |p| p.part_without_override("/extra/thing.dat", b"0123"));
    let written = visit_written(&source);
    let package = diacraft_opc::Package::read(&written, &diacraft_opc::Limits::default()).unwrap();
    assert!(package.content_type("/extra/thing.dat").is_some(), "a row is added");
    assert_eq!(package.part("/extra/thing.dat"), Some(&b"0123"[..]));
    let found = visit_differences(&source);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_row_with_no_ix_is_written_with_the_ix_it_reads_as_and_reads_back_the_same() {
    // Not content (makes it so for the check): MS-VSDX says an absent `IX` is the count of the rows before it,
    // which is what the reader gives the row and the writer writes; the model holds no trace of the spelling. Pinned: the writer writes the `IX` attributes, the check
    // sees no difference, and the model reads back the same.
    let geometry = r#"<Section N="Geometry" IX="0"><Row T="MoveTo"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo"><Cell N="X" V="1"/><Cell N="Y" V="0"/></Row></Section>"#;
    let source = handmade(&format!(r#"<Shape ID="1"><Cell N="PinX" V="1"/>{geometry}</Shape>"#), |p| p);
    let found = visit_differences(&source);
    assert!(found.is_empty(), "{found:?}");
    let (doc, _) = read_ok(&source);
    let written = visit_written(&source);
    let page = text_of(&written, concat!("/vi", "sio", "/pages/page1.xml"));
    assert_eq!(page.matches("IX=\"").count(), 3, "the section's and the two rows': {page}");
    let (back, _) = read_ok(&written);
    let rows = |d: &diacraft_doc::Document| -> Vec<Option<u32>> {
        let shape = d.pages.iter().next().unwrap().shapes.iter().next().unwrap().clone();
        shape.sheet.sections.iter().flat_map(|s| s.rows.iter().map(|r| r.ix).collect::<Vec<_>>()).collect()
    };
    assert_eq!(rows(&doc), vec![Some(0), Some(1)]);
    assert_eq!(rows(&back), rows(&doc));
}

// ------------------------------------------------------------------------------------------------------------ F3: a row R22 implies, made where the file lacked it

#[test]
fn a_page_whose_shapes_use_a_master_and_that_has_no_row_to_it_is_given_one_by_every_save_and_is_the_same_content() {
    // Not content (the concern 5 keeps it so; makes it so for the check): gives a page a relationship to
    // the part of each master its shapes use, which says what the shapes' `Master` says already; the writer makes a missing one, also in a save of the drawing as it
    // was read, so that a master the writer names anew is reached. Pinned: the writer makes the page's relationships part with the one row, and the check sees no
    // difference.
    use diacraft_testkit::package::MasterSpec;
    let master_shape = r#"<Shape ID="5" Type="Shape"><Cell N="Width" V="1"/></Shape>"#;
    let instance = r#"<Shape ID="1" Master="2" Type="Shape"><Cell N="PinX" V="1"/></Shape>"#;
    let source = VsdxBuilder::new().master(MasterSpec::new(2, "Box").shapes(master_shape)).page(PageSpec::new(1, "P").shapes(instance)).build();
    let found = visit_differences(&source);
    let rels = concat!("/vi", "sio", "/pages/_rels/page1.xml.rels");
    assert!(found.is_empty(), "{found:?}");
    let written = visit_written(&source);
    let made = text_of(&written, rels);
    assert_eq!(made.matches("<Relationship ").count(), 1, "{made}");
    assert!(made.contains("/master\"") && made.contains("master1.xml"), "{made}");
}
