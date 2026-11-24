//! Part names that a relationship spells in another case: OPC compares part names ASCII case-insensitively, so a
//! `Target` of `VISIO/Document.XML` reaches the part `/visio/document.xml`. **A lookup folds case, and what is written keeps the case each part arrived with**
//! A save of either writer keeps every name the package has, byte for byte, and a part it makes goes into the folder as the package spells it, not as a
//! relationship spelled it. The targets themselves are kept as written.
//!
//! The round-trip property (`round_trip.rs`) found it with seed `15556809671185670448`: a stencil whose package relationship said `visIo/document.xml`, saved
//! as a drawing, gave the page that had no part a new part `/visIo/pages/page2.xml`, a second spelling of the folder `/visio/pages/` beside the first.

use std::collections::BTreeSet;
use std::sync::Arc;

use diacraft_doc::{CellAddr, CellEdit, Document, Master, Owner, ShapeRef, SheetRef, Value, WriteOrigin};
use diacraft_opc::{Limits as OpcLimits, Package};
use diacraft_testkit::package::{MasterSpec, PageSpec, Pkg, VsdxBuilder, cell, shape};
use diacraft_vsdx::{Kind, Limits, OriginalBytes, Variant, WriteOptions, Written, compare_content, holds, holds_of, read, write_with};

use crate::read_ok;
use crate::round_trip::{check, flat, seed, zipped};

const NOW: i64 = 1_700_000_000;
const VBA_REL: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProject";
const VBA_TYPE: &str = "application/vnd.ms-office.vbaProject";
const MACRO_DOCUMENT: &str = "application/vnd.ms-visio.drawing.macroEnabled.main+xml";

/// The package `pkg` with each `(item, from, to)` made: the text `from` of the item (a ZIP item name, with no slash) replaced by `to`, once. The checksums are
/// made again, so the package is one an honest archive writer could have made.
fn respelled(pkg: &Pkg, edits: &[(&str, &str, &str)]) -> Vec<u8> {
    let mut items = flat(pkg);
    for (item, from, to) in edits {
        let (_, bytes) = items.iter_mut().find(|(name, _)| name == item).unwrap_or_else(|| panic!("no item {item}"));
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert_eq!(text.matches(from).count(), 1, "{item} says {from:?} once: {text}");
        *bytes = text.replacen(from, to, 1).into_bytes();
    }
    zipped(&items)
}

/// The package `pkg` with each item whose name starts with one of `renames`' first texts named with the second in its place (a ZIP item name, with no slash): the
/// archive spells those parts in another case than the relationships and the content types do.
fn renamed(pkg: &Pkg, renames: &[(&str, &str)]) -> Vec<u8> {
    let mut items = flat(pkg);
    for (name, _) in &mut items {
        if let Some((from, to)) = renames.iter().find(|(from, _)| name.starts_with(from)) {
            *name = format!("{to}{}", &name[from.len()..]);
        }
    }
    zipped(&items)
}

/// The names of the parts of a package, as the archive spells them.
fn names(bytes: &[u8]) -> BTreeSet<String> {
    let pkg = Package::read(bytes, &OpcLimits::default()).unwrap();
    pkg.parts().map(str::to_owned).collect()
}

/// The names `after` has that `before` does not, compared as written: a part the save kept under another spelling is one of them.
fn new_names(before: &[u8], after: &[u8]) -> Vec<String> {
    let old = names(before);
    names(after).into_iter().filter(|n| !old.contains(n)).collect()
}

/// The names `before` has that `after` does not, compared as written.
fn lost_names(before: &[u8], after: &[u8]) -> Vec<String> {
    let new = names(after);
    names(before).into_iter().filter(|n| !new.contains(n)).collect()
}

fn splice(doc: &Document, bytes: &[u8], variant: Option<Variant>) -> Written {
    let options = WriteOptions { now: Some(NOW), variant, ..WriteOptions::default() };
    write_with(doc, Some(&OriginalBytes { bytes, baseline: None }), &options).unwrap_or_else(|e| panic!("the save refused: {e} ({})", e.class()))
}

fn model(doc: &Document, variant: Variant, unchanged: bool) -> Written {
    let options = WriteOptions { now: Some(NOW), variant: Some(variant), content_unchanged: unchanged, ..WriteOptions::default() };
    write_with(doc, None, &options).unwrap_or_else(|e| panic!("the model writer refused: {e} ({})", e.class()))
}

fn drawing() -> Variant {
    Variant { kind: Kind::Drawing, macro_enabled: false }
}

/// A drawing with two pages and two masters, a part for each, an instance of the first master on the first page, a theme and a picture. The picture is typed by a
/// `Default` row, as the model writer types one (`model_writer.rs`: the choice is not in the model), so that a model save of it is the same content.
fn builder() -> VsdxBuilder {
    VsdxBuilder::new()
        .theme(br#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="T"><a:themeElements/></a:theme>"#)
        .master(MasterSpec::new(1, "Box").shapes(&shape(4, &cell("Width", "1"))))
        .master(MasterSpec::new(2, "Ring").shapes(&shape(4, &cell("Width", "2"))))
        .page(PageSpec::new(1, "First").shapes(r#"<Shape ID="1" Master="1"><Cell N="PinX" V="1"/></Shape>"#).rel(
            "rId1",
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image",
            "../media/image1.png",
        ))
        .page(PageSpec::new(2, "Second").shapes(&shape(1, &cell("PinX", "2"))))
        .with(Pkg::new().default_type("png", "image/png").part_without_override("/visio/media/image1.png", [1u8, 2, 3]))
}

/// Every relationship of [`builder`]'s package that names a part the model rebuilds, spelled in another case: the document part from the package, the
/// indexes and the theme from the document part, a page and a master from their indexes, and the picture from the page.
fn respelled_drawing() -> Vec<u8> {
    respelled(
        &builder().pkg(),
        &[
            ("_rels/.rels", "Target=\"visio/document.xml\"", "Target=\"VISIO/Document.XML\""),
            ("visio/_rels/document.xml.rels", "Target=\"pages/pages.xml\"", "Target=\"Pages/PAGES.xml\""),
            ("visio/_rels/document.xml.rels", "Target=\"masters/masters.xml\"", "Target=\"MASTERS/masters.XML\""),
            ("visio/_rels/document.xml.rels", "Target=\"theme/theme1.xml\"", "Target=\"Theme/Theme1.xml\""),
            ("visio/pages/_rels/pages.xml.rels", "Target=\"page1.xml\"", "Target=\"PAGE1.XML\""),
            ("visio/masters/_rels/masters.xml.rels", "Target=\"master1.xml\"", "Target=\"Master1.xml\""),
            ("visio/pages/_rels/page1.xml.rels", "Target=\"../media/image1.png\"", "Target=\"../MEDIA/Image1.PNG\""),
        ],
    )
}

/// A third master, with the shapes of the first: a master with no shapes has no part in a model save.
fn add_master(doc: &mut Document) {
    let shapes = doc.masters.get(0).unwrap().shapes.clone();
    doc.add_master(Master { id: 3, name: Some(Arc::from("Star")), name_u: Some(Arc::from("Star")), shapes, ..Master::default() }).unwrap();
}

fn page_cell(doc: &mut Document, page: u32, name: &str, value: &str) {
    let sheet = SheetRef::Shape(ShapeRef::top(Owner::Page(page), 0));
    doc.write_cell(&sheet, &CellAddr::direct(name), CellEdit::Value(Value::from_text(value)), WriteOrigin::Drawing).unwrap();
}

/// The text of the relationships part `name` of `bytes`.
fn rels_text(bytes: &[u8], name: &str) -> String {
    let pkg = Package::read(bytes, &OpcLimits::default()).unwrap();
    String::from_utf8(pkg.part(name).unwrap_or_else(|| panic!("no part {name}")).to_vec()).unwrap()
}

// ------------------------------------------------------------------------------------------------------------------ the case the property found

/// Seed `15556809671185670448` of `round_trip::a_save_of_anything_that_reads_keeps_what_it_had`, shrunk to the one change that matters (the seed also made the
/// solutions' relationships part unreadable, which every save keeps as its bytes): a stencil whose package relationship spells the document part's folder in other letter case, saved
/// as itself and as a macro-enabled drawing. The page that had no part gets one, in the folder the package has.
#[test]
fn seed_15556809671185670448_a_stencil_whose_document_relationship_is_spelled_in_another_case_saved_as_a_drawing() {
    let bytes = respelled(&seed("vssx"), &[("_rels/.rels", "Target=\"visio/document.xml\"", "Target=\"visIo/document.xml\"")]);
    for target in [None, Variant::from_extension("vsdm"), Variant::from_extension("vsdx")] {
        if let Err(message) = check(&bytes, target) {
            panic!("vssx to {}: {message}", target.map_or("itself", Variant::extension));
        }
    }
    let (doc, report) = read_ok(&bytes);
    assert_eq!(report.document_part, "/visio/document.xml", "the document part is named as the package has it, not as the relationship spells it");
    let written = splice(&doc, &bytes, Variant::from_extension("vsdx"));
    assert_eq!(new_names(&bytes, &written.bytes), ["/visio/pages/page2.xml"], "the new page part is in the folder the package has");
    assert_eq!(lost_names(&bytes, &written.bytes), Vec::<String>::new());
    // The relationship keeps its spelling: only the part the save made is named by the writer.
    assert!(rels_text(&written.bytes, "/_rels/.rels").contains("Target=\"visIo/document.xml\""));
    // The model writer makes the same part for the page, and the page's relationship to its background page (`BackPage`), which it makes from the model.
    let (doc, _) = read_ok(&bytes);
    let written = model(&doc, drawing(), true);
    assert_eq!(
        new_names(&bytes, &written.bytes),
        ["/visio/pages/_rels/page2.xml.rels", "/visio/pages/page2.xml"],
        "the model writer's new page part is in the folder the package has"
    );
    assert_eq!(lost_names(&bytes, &written.bytes), Vec::<String>::new());
}

/// The other way round: the archive spells the folder `VISIO/` and every relationship and row says `visio/`. The stencil's page that has no part gets one in the folder
/// as the archive spells it, and the round-trip check, which compares the names it knows as OPC compares them, takes that part for a page's.
#[test]
fn a_stencil_whose_archive_spells_the_folder_in_capitals_gets_its_new_page_parts_there() {
    let bytes = renamed(&seed("vssx"), &[("visio/", "VISIO/")]);
    for target in [None, Variant::from_extension("vsdm"), Variant::from_extension("vsdx")] {
        if let Err(message) = check(&bytes, target) {
            panic!("vssx to {}: {message}", target.map_or("itself", Variant::extension));
        }
    }
    let (doc, report) = read_ok(&bytes);
    assert_eq!(report.document_part, "/VISIO/document.xml");
    let written = splice(&doc, &bytes, Variant::from_extension("vsdx"));
    assert_eq!(new_names(&bytes, &written.bytes), ["/VISIO/pages/page2.xml"]);
    assert_eq!(lost_names(&bytes, &written.bytes), Vec::<String>::new());
    let written = model(&doc, drawing(), true);
    assert_eq!(new_names(&bytes, &written.bytes), ["/VISIO/pages/_rels/page2.xml.rels", "/VISIO/pages/page2.xml"]);
    assert_eq!(lost_names(&bytes, &written.bytes), Vec::<String>::new());
}

// -------------------------------------------------------------------------------------------------------------------------- a save into a file

#[test]
fn a_save_into_a_file_keeps_every_name_the_package_has_when_its_relationships_spell_them_in_another_case() {
    let bytes = respelled_drawing();
    let (doc, report) = read_ok(&bytes);
    assert_eq!(report.document_part, "/visio/document.xml");
    let parts: Vec<Option<&str>> = doc.pages.iter().map(|p| p.part_name.as_deref()).collect();
    assert_eq!(parts, [Some("/visio/pages/page1.xml"), Some("/visio/pages/page2.xml")], "the pages' parts are named as the package has them");
    let parts: Vec<Option<&str>> = doc.masters.iter().map(|m| m.part_name.as_deref()).collect();
    assert_eq!(parts, [Some("/visio/masters/master1.xml"), Some("/visio/masters/master2.xml")]);
    let themes: Vec<&str> = doc.themes.iter().map(|t| &*t.part).collect();
    assert_eq!(themes, ["/visio/theme/theme1.xml"]);

    // No edit, a cell of each page, and a Save As to a template: every name kept as it is spelled, nothing new.
    let mut edited = doc.clone();
    page_cell(&mut edited, 1, "PinX", "3");
    page_cell(&mut edited, 2, "PinX", "4");
    for (what, doc, variant) in
        [("no edit", &doc, None), ("two cells", &edited, None), ("a template", &doc, Some(Variant { kind: Kind::Template, macro_enabled: false }))]
    {
        let written = splice(doc, &bytes, variant);
        assert_eq!(new_names(&bytes, &written.bytes), Vec::<String>::new(), "{what}");
        assert_eq!(lost_names(&bytes, &written.bytes), Vec::<String>::new(), "{what}");
        assert_eq!(written.read_back.document_part, "/visio/document.xml", "{what}");
        let types = Package::read(&written.bytes, &OpcLimits::default()).unwrap();
        let overrides: Vec<&str> =
            types.content_types().overrides.iter().map(|(name, _)| name.as_str()).filter(|n| n.eq_ignore_ascii_case("/visio/document.xml")).collect();
        assert_eq!(overrides, ["/visio/document.xml"], "{what}: one row types the document part");
        for (rels, target) in [("/_rels/.rels", "VISIO/Document.XML"), ("/visio/pages/_rels/pages.xml.rels", "PAGE1.XML")] {
            assert!(rels_text(&written.bytes, rels).contains(&format!("Target=\"{target}\"")), "{what}: {rels} keeps {target}");
        }
    }
}

#[test]
fn a_page_and_a_master_added_in_a_save_into_a_file_go_into_the_folders_the_package_has() {
    let bytes = respelled_drawing();
    let (mut doc, _) = read_ok(&bytes);
    doc.add_page(Some("Third")).unwrap();
    add_master(&mut doc);
    let written = splice(&doc, &bytes, None);
    assert_eq!(new_names(&bytes, &written.bytes), ["/visio/masters/master3.xml", "/visio/pages/page3.xml"]);
    assert_eq!(lost_names(&bytes, &written.bytes), Vec::<String>::new());
    let pkg = Package::read(&written.bytes, &OpcLimits::default()).unwrap();
    for (rels, target) in [("/visio/pages/pages.xml", "page3.xml"), ("/visio/masters/masters.xml", "master3.xml")] {
        let rows = pkg.rels(rels).unwrap();
        let found: Vec<&str> = rows.items.iter().map(|r| r.target_raw.as_str()).filter(|t| *t == target).collect();
        assert_eq!(found, [target], "{rels} reaches the new part by a target in its own folder");
    }
}

/// What a save says it did names each part as the archive spells it: the relationships parts of the pages index and of a page, spelt `_RELS` in capitals, are rewritten
/// and taken out under those names (the writer finds a part's relationships part by its source's name, and spelt it its own way in the lists).
#[test]
fn the_lists_of_what_a_save_did_name_the_parts_as_the_archive_spells_them() {
    let bytes = renamed(
        &builder().pkg(),
        &[
            ("visio/pages/_rels/pages.xml.rels", "visio/pages/_RELS/PAGES.XML.RELS"),
            ("visio/pages/_rels/page1.xml.rels", "visio/pages/_RELS/Page1.xml.rels"),
        ],
    );
    let (mut doc, _) = read_ok(&bytes);
    doc.pages.remove(0).unwrap();
    doc.add_page(Some("Third")).unwrap();
    let written = splice(&doc, &bytes, None);
    assert!(written.rewritten.iter().any(|n| n == "/visio/pages/_RELS/PAGES.XML.RELS"), "{:?}", written.rewritten);
    assert!(written.removed.iter().any(|n| n == "/visio/pages/_RELS/Page1.xml.rels"), "{:?}", written.removed);
    assert_eq!(new_names(&bytes, &written.bytes), ["/visio/pages/page3.xml"]);
    let lost = lost_names(&bytes, &written.bytes);
    assert_eq!(
        lost,
        ["/visio/media/image1.png", "/visio/pages/_RELS/Page1.xml.rels", "/visio/pages/page1.xml"],
        "the page taken out and what only it reached"
    );
    let listed: BTreeSet<&str> = written.removed.iter().map(String::as_str).collect();
    assert_eq!(listed, lost.iter().map(String::as_str).collect(), "the list of what was taken out names the parts as they were spelt");
}

/// The `Override` of the document part, spelt in capitals, is the row a Save As changes, in place, and no second row is added for the part (one per part name); the
/// model writer types the part with one row too.
#[test]
fn a_content_types_row_spelt_in_another_case_is_the_row_a_save_as_changes() {
    let bytes = respelled(&builder().pkg(), &[("[Content_Types].xml", "PartName=\"/visio/document.xml\"", "PartName=\"/VISIO/DOCUMENT.XML\"")]);
    let (doc, _) = read_ok(&bytes);
    let template = Variant { kind: Kind::Template, macro_enabled: false };
    let rows = |bytes: &[u8]| -> Vec<(String, String)> {
        let pkg = Package::read(bytes, &OpcLimits::default()).unwrap();
        pkg.content_types().overrides.iter().filter(|(name, _)| name.eq_ignore_ascii_case("/visio/document.xml")).cloned().collect()
    };
    let written = splice(&doc, &bytes, Some(template));
    assert_eq!(rows(&written.bytes), [("/VISIO/DOCUMENT.XML".to_owned(), template.content_type().to_owned())]);
    let written = model(&doc, template, true);
    assert_eq!(rows(&written.bytes).len(), 1, "{:?}", rows(&written.bytes));
}

// ------------------------------------------------------------------------------------------------------------------------- the model writer

#[test]
fn a_model_save_keeps_every_name_the_package_has_when_its_relationships_spell_them_in_another_case() {
    let bytes = respelled_drawing();
    let (doc, _) = read_ok(&bytes);
    for unchanged in [true, false] {
        let written = model(&doc, drawing(), unchanged);
        // An edited save leaves the thumbnail out, and this package has none: the names are the package's, all of them, as they are spelled.
        assert_eq!(new_names(&bytes, &written.bytes), Vec::<String>::new(), "content unchanged: {unchanged}");
        assert_eq!(lost_names(&bytes, &written.bytes), Vec::<String>::new(), "content unchanged: {unchanged}");
        assert_eq!(written.read_back.document_part, "/visio/document.xml");
        for (rels, target) in [
            ("/_rels/.rels", "VISIO/Document.XML"),
            ("/visio/_rels/document.xml.rels", "Pages/PAGES.xml"),
            ("/visio/_rels/document.xml.rels", "MASTERS/masters.XML"),
            ("/visio/_rels/document.xml.rels", "Theme/Theme1.xml"),
            ("/visio/pages/_rels/pages.xml.rels", "PAGE1.XML"),
            ("/visio/masters/_rels/masters.xml.rels", "Master1.xml"),
            ("/visio/pages/_rels/page1.xml.rels", "../MEDIA/Image1.PNG"),
        ] {
            assert!(
                rels_text(&written.bytes, rels).contains(&format!("Target=\"{target}\"")),
                "{rels} keeps {target}: {}",
                rels_text(&written.bytes, rels)
            );
        }
        if unchanged {
            let d = compare_content(&bytes, &written.bytes, &Limits::default()).unwrap();
            assert!(d.same(), "{:?}", d.shown.iter().take(8).map(|x| format!("{} {} {:?}", x.part, x.path, x.found)).collect::<Vec<_>>());
        }
    }
    // The control: the package with every target as the package spells the parts is written under the same names, and is the same content.
    let plain = builder().build();
    let written = model(&read_ok(&plain).0, drawing(), true);
    assert_eq!((new_names(&plain, &written.bytes), lost_names(&plain, &written.bytes)), (Vec::new(), Vec::new()));
    assert!(compare_content(&plain, &written.bytes, &Limits::default()).unwrap().same());
}

#[test]
fn a_page_and_a_master_added_in_a_model_save_go_into_the_folders_the_package_has() {
    let bytes = respelled_drawing();
    let (mut doc, _) = read_ok(&bytes);
    doc.add_page(Some("Third")).unwrap();
    add_master(&mut doc);
    let written = model(&doc, drawing(), false);
    assert_eq!(new_names(&bytes, &written.bytes), ["/visio/masters/master3.xml", "/visio/pages/page3.xml"]);
    assert_eq!(lost_names(&bytes, &written.bytes), Vec::<String>::new());
}

/// **What the model writer cannot know**: the name of a part the model rebuilds and holds no name for. The pages index here is reached by a relationship spelt
/// `Pages/PAGES.xml`, and its rows to the pages are all rows the writer makes again (so no held row has the index as its source): the model has no name for it, and a
/// model save writes it under the name the relationship gives, **in the folder as the package's pages spell it**, and a page added goes into that folder too, not into a
/// second spelling of it. A save into the file keeps the index's name. A field of the model for the names of the parts it rebuilds would keep it in a model save too.
#[test]
fn an_index_the_model_holds_no_name_for_is_written_in_the_folder_the_package_has() {
    let bytes = respelled(&builder().pkg(), &[("visio/_rels/document.xml.rels", "Target=\"pages/pages.xml\"", "Target=\"Pages/PAGES.xml\"")]);
    let (mut doc, _) = read_ok(&bytes);
    let written = model(&doc, drawing(), true);
    assert_eq!(new_names(&bytes, &written.bytes), ["/visio/pages/PAGES.xml", "/visio/pages/_rels/PAGES.xml.rels"]);
    assert_eq!(lost_names(&bytes, &written.bytes), ["/visio/pages/_rels/pages.xml.rels", "/visio/pages/pages.xml"]);
    // The one difference is the `PartName` of the index's `Override`, which names the part as it is written: a row is a value, compared as written.
    let d = compare_content(&bytes, &written.bytes, &Limits::default()).unwrap();
    let shown: Vec<String> = d.shown.iter().map(|x| format!("{} {} {:?}", x.part, x.path, x.found)).collect();
    assert_eq!(shown, ["[Content_Types].xml /Types[0]/Override#11/@PartName Value"]);
    doc.add_page(Some("Third")).unwrap();
    let written = model(&doc, drawing(), false);
    assert_eq!(new_names(&bytes, &written.bytes), ["/visio/pages/PAGES.xml", "/visio/pages/_rels/PAGES.xml.rels", "/visio/pages/page3.xml"]);
    let written = splice(&doc, &bytes, None);
    assert_eq!(new_names(&bytes, &written.bytes), ["/visio/pages/page3.xml"]);
    assert_eq!(lost_names(&bytes, &written.bytes), Vec::<String>::new());
}

// ------------------------------------------------------------------------------------------------------------------------- what a package holds

/// The count of what a package holds names the macro project as the package has it, so that it is the model's count of the same package
/// (`holds_of`), whatever case the document's relationship spells the project in.
#[test]
fn the_holds_of_a_package_name_a_project_spelled_in_another_case_as_the_package_has_it() {
    let pkg = builder()
        .document_type(MACRO_DOCUMENT)
        .with(Pkg::new().default_type("bin", VBA_TYPE).part_without_override("/visio/vbaProject.bin", b"not code").rel(
            "/visio/document.xml",
            "rId50",
            VBA_REL,
            "VBAPROJECT.BIN",
        ))
        .pkg();
    let bytes = pkg.build();
    let found = holds(&bytes, &Limits::default()).unwrap();
    assert_eq!(found.macro_parts, ["/visio/vbaProject.bin"]);
    let (doc, _) = read(&bytes, &Limits::default()).unwrap();
    assert_eq!(holds_of(&doc).macro_parts, found.macro_parts, "the package's count and the model's agree");
}
