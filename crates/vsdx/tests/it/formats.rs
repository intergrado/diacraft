//! The six formats, open and save: what each package holds, what a Save As between them changes, and what a save of a file with
//! nothing edited leaves alone.
//!
//! Every input is built in code. The VBA bytes are made up: nothing here runs, parses or needs real macro code.

use std::collections::BTreeMap;
use std::sync::Arc;

use diacraft_doc::{CellAddr, CellEdit, Document, Owner, ShapeRef, SheetRef, Value, WriteOrigin};
use diacraft_opc::{Limits, Package};
use diacraft_testkit::package::{MasterSpec, OD, PK, PageSpec, Pkg, VIS, VsdxBuilder, cell, shape};
use diacraft_vsdx::{Kind, OriginalBytes, Variant, WriteError, WriteOptions, Written, write_with};

use crate::read_ok;

const NOW: i64 = 1_700_000_000;
const EXTENSIONS: [&str; 6] = ["vsdx", "vsdm", "vssx", "vssm", "vstx", "vstm"];
const VBA_REL: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProject";
const SIG_REL: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProjectSignature";
const SIG_AGILE_REL: &str = "http://schemas.microsoft.com/office/2014/relationships/vbaProjectSignatureAgile";
const SIG_V3_REL: &str = "http://schemas.microsoft.com/office/2020/07/relationships/vbaProjectSignatureV3";
const VBA_BYTES: &[u8] = b"\xD0\xCF\x11\xE0 not code, never run";

fn variant(ext: &str) -> Variant {
    Variant::from_extension(ext).unwrap()
}

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

fn save(original: &[u8], doc: &Document, baseline: Option<&Document>, options: &WriteOptions) -> Result<Written, WriteError> {
    write_with(doc, Some(&OriginalBytes { bytes: original, baseline }), options)
}

fn edit_pin(doc: &mut Document) {
    let sheet = SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0));
    doc.write_cell(&sheet, &CellAddr::direct("PinX"), CellEdit::Value(Value::from_text("7")), WriteOrigin::Drawing).unwrap();
}

/// The `Default` and `Override` rows of a package, as sorted (key, content type) pairs.
type TypeRows = (Vec<(String, String)>, Vec<(String, String)>);

fn types_of(bytes: &[u8]) -> TypeRows {
    let pkg = Package::read(bytes, &Limits::default()).unwrap();
    let (mut d, mut o) = (pkg.content_types().defaults.clone(), pkg.content_types().overrides.clone());
    d.sort();
    o.sort();
    (d, o)
}

// ------------------------------------------------------------------------------------------------------------------------ the inputs

/// A package of the variant `ext`: drawings and templates have a page with a shape and a part; a stencil has one page row with no part
/// All have two masters, a thumbnail, an unknown part, a solution of another writer and an image.
fn package(ext: &str) -> Vec<u8> {
    let v = variant(ext);
    let mut builder = VsdxBuilder::new()
        .document_type(v.content_type())
        .master(MasterSpec::new(1, "Box").shapes(&shape(4, &cell("Width", "1"))).icon("AAABAA=="))
        .master(MasterSpec::new(2, "Two").shapes(&shape(4, &cell("Width", "2"))))
        .solution("Acme", r#"<SolutionXML Name="Acme"><x/></SolutionXML>"#);
    builder = if v.kind == Kind::Stencil {
        builder.page(PageSpec::new(0, "Page-1").without_part())
    } else {
        builder.page(PageSpec::new(1, "First").shapes(&shape(5, &cell("PinX", "1"))).rel("rId9", &format!("{OD}image"), "../media/image1.png"))
    };
    let extras = Pkg::new()
        .part_without_override("/foo/bar.dat", b"0123456789")
        .default_type("dat", "application/x-foo")
        .default_type("png", "image/png")
        .part_without_override("/visio/media/image1.png", b"\x89PNG.fake")
        .part_without_override("/docProps/thumbnail.emf", b"emf-bytes")
        .default_type("emf", "image/x-emf")
        .rel("/", "rId8", &format!("{PK}thumbnail"), "docProps/thumbnail.emf")
        .rel("/", "rId7", "urn:example:foo", "foo/bar.dat");
    builder.with(extras).build()
}

/// The VBA project of a macro-enabled package, the way a VSDX file holds it: the project by `Default` of `bin`, the
/// three signature parts by `Override`, and their relationships from the project's own relationships part.
fn macros() -> Pkg {
    Pkg::new()
        .default_type("bin", "application/vnd.ms-office.vbaProject")
        .part_without_override("/visio/vbaProject.bin", VBA_BYTES)
        .rel("/visio/document.xml", "rId50", VBA_REL, "vbaProject.bin")
        .part("/visio/vbaProjectSignature.bin", "application/vnd.ms-office.vbaProjectSignature", b"sig-1")
        .part("/visio/vbaProjectSignatureAgile.bin", "application/vnd.ms-office.vbaProjectSignatureAgile", b"sig-2")
        .part("/visio/vbaProjectSignatureV3.bin", "application/vnd.ms-office.vbaProjectSignatureV3", b"sig-3")
        .rel("/visio/vbaProject.bin", "rId1", SIG_REL, "vbaProjectSignature.bin")
        .rel("/visio/vbaProject.bin", "rId2", SIG_AGILE_REL, "vbaProjectSignatureAgile.bin")
        .rel("/visio/vbaProject.bin", "rId3", SIG_V3_REL, "vbaProjectSignatureV3.bin")
}

/// [`package`] with a macro project. The variant need not allow macros: a test of what a macro-free type does with one asks for it.
fn package_with_macros(ext: &str) -> Vec<u8> {
    let v = variant(ext);
    let mut builder = VsdxBuilder::new()
        .document_type(v.content_type())
        .master(MasterSpec::new(1, "Box").shapes(&shape(4, &cell("Width", "1"))))
        .page(PageSpec::new(1, "First").shapes(&shape(5, &cell("PinX", "1"))));
    builder = builder.with(macros());
    builder.build()
}

const MACRO_PARTS: [&str; 4] =
    ["/visio/vbaProject.bin", "/visio/vbaProjectSignature.bin", "/visio/vbaProjectSignatureAgile.bin", "/visio/vbaProjectSignatureV3.bin"];

// ------------------------------------------------------------------------------------------------------------------ save with nothing edited

#[test]
fn a_save_with_no_edit_changes_only_the_writers_name_in_every_one_of_the_six_formats() {
    for ext in EXTENSIONS {
        let original = package(ext);
        let (doc, report) = read_ok(&original);
        assert_eq!(report.variant, Some(variant(ext)), "{ext}");
        let written = save(&original, &doc, None, &options()).unwrap_or_else(|e| panic!("{ext}: {e}"));
        let (before, after) = (parts(&original), parts(&written.bytes));
        assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>(), "{ext}: the same parts, thumbnail included");
        let changed: Vec<&str> = after.iter().filter(|(k, v)| before.get(*k) != Some(*v)).map(|(k, _)| k.as_str()).collect();
        assert_eq!(changed, ["/docProps/app.xml"], "{ext}: R31, nothing else differs");
        assert_eq!(types_of(&original), types_of(&written.bytes), "{ext}: the content types are the same rows");
        assert_eq!(written.read_back.variant, Some(variant(ext)), "{ext}");
        assert!(
            written.warnings.is_empty() && written.read_back.warnings.is_empty(),
            "{ext}: {:?} {:?}",
            written.warnings,
            written.read_back.warnings
        );
    }
}

#[test]
fn a_save_with_no_edit_keeps_the_macro_project_and_its_signatures_byte_for_byte_in_every_macro_type() {
    for ext in ["vsdm", "vssm", "vstm"] {
        let original = package_with_macros(ext);
        let (doc, _) = read_ok(&original);
        let written = save(&original, &doc, None, &options()).unwrap();
        let (before, after) = (parts(&original), parts(&written.bytes));
        for name in MACRO_PARTS {
            assert_eq!(before.get(name), after.get(name), "{ext}: {name}");
            assert!(before.contains_key(name), "{ext}: the test package holds {name}");
        }
        assert_eq!(
            before.get("/visio/_rels/vbaProject.bin.rels"),
            after.get("/visio/_rels/vbaProject.bin.rels"),
            "{ext}: E7, the project's relationships"
        );
        assert_eq!(types_of(&original), types_of(&written.bytes), "{ext}");
        assert!(written.warnings.is_empty(), "{ext}: {:?}", written.warnings);
    }
}

#[test]
fn an_edit_leaves_the_project_and_everything_else_the_edit_did_not_reach_alone_in_every_format() {
    for ext in ["vsdx", "vsdm", "vssx", "vssm", "vstx", "vstm"] {
        if variant(ext).kind == Kind::Stencil {
            continue; // a stencil has no shape on a page to move: its edit is the next test
        }
        let original = if variant(ext).macro_enabled { package_with_macros(ext) } else { package(ext) };
        let (mut doc, _) = read_ok(&original);
        let baseline = doc.clone();
        edit_pin(&mut doc);
        let written = save(&original, &doc, Some(&baseline), &options()).unwrap();
        let (before, after) = (parts(&original), parts(&written.bytes));
        for name in before.keys() {
            let reached = ["/visio/pages/page1.xml", "/docProps/app.xml", "/docProps/core.xml", "/_rels/.rels", "/docProps/thumbnail.emf"];
            if !reached.contains(&name.as_str()) {
                assert_eq!(before.get(name), after.get(name), "{ext}: {name}");
            }
        }
        assert_eq!(written.read_back.variant, Some(variant(ext)), "{ext}");
    }
}

#[test]
fn a_stencil_opens_with_its_masters_and_a_page_row_with_no_part_and_saves_the_same_row_and_no_new_part() {
    for ext in ["vssx", "vssm"] {
        let original = package(ext);
        let (doc, report) = read_ok(&original);
        assert_eq!(doc.masters.len(), 2, "{ext}");
        assert_eq!(report.variant.unwrap().kind, Kind::Stencil);
        // Vector 10 of the masters spec: the page row has no `Rel`, and the save adds no part.
        let written = save(&original, &doc, None, &options()).unwrap();
        assert_eq!(parts(&written.bytes).keys().collect::<Vec<_>>(), parts(&original).keys().collect::<Vec<_>>(), "{ext}");
        assert!(!parts(&written.bytes).contains_key("/visio/pages/page1.xml"), "{ext}");
        // A master edited: the master part and the rows around it change, and the page row still has no part.
        let mut edited = doc.clone();
        let mut master = (**edited.masters.iter().next().unwrap()).clone();
        master.name = Some(Arc::from("Renamed"));
        edited.masters.set(0, Arc::new(master)).unwrap();
        let again = save(&original, &edited, Some(&doc), &options()).unwrap();
        let after = parts(&again.bytes);
        assert!(!after.contains_key("/visio/pages/page1.xml"), "{ext}: still no page part");
        assert!(text_of(&after, "/visio/masters/masters.xml").contains("Renamed"), "{ext}");
        assert_eq!(after.get("/visio/pages/pages.xml"), parts(&original).get("/visio/pages/pages.xml"), "{ext}: the page row is untouched");
    }
}

// ------------------------------------------------------------------------------------------------------------------ Save As between formats

#[test]
fn save_as_between_any_two_formats_changes_only_the_document_type_and_what_the_new_kind_needs() {
    for from in EXTENSIONS {
        for to in EXTENSIONS {
            let original = package(from);
            let (doc, _) = read_ok(&original);
            let written = save(&original, &doc, None, &WriteOptions { variant: Some(variant(to)), ..options() })
                .unwrap_or_else(|e| panic!("{from} to {to}: {e}"));
            assert_eq!(written.read_back.variant, Some(variant(to)), "{from} to {to}");
            let (before, after) = (parts(&original), parts(&written.bytes));
            let opening_stencil_to_other = variant(from).kind == Kind::Stencil && variant(to).kind != Kind::Stencil;
            let allowed_new = ["/visio/pages/page1.xml", "/visio/pages/_rels/pages.xml.rels"];
            let allowed_changed: &[&str] = if opening_stencil_to_other {
                &[
                    "/docProps/app.xml",
                    "/visio/pages/pages.xml",
                    "/visio/pages/_rels/pages.xml.rels",
                    "/docProps/core.xml",
                    "/_rels/.rels",
                    "/docProps/thumbnail.emf",
                ]
            } else {
                &["/docProps/app.xml"]
            };
            for (name, bytes) in &after {
                match before.get(name) {
                    Some(old) => assert!(old == bytes || allowed_changed.contains(&name.as_str()), "{from} to {to}: {name} changed"),
                    None => assert!(opening_stencil_to_other && allowed_new.contains(&name.as_str()), "{from} to {to}: {name} is new"),
                }
            }
            for name in before.keys() {
                assert!(
                    after.contains_key(name) || (opening_stencil_to_other && name == "/docProps/thumbnail.emf"),
                    "{from} to {to}: {name} is gone"
                );
            }
            // The only content type that moves among the rows the file had is the document's.
            let (_, old_rows) = types_of(&original);
            let (_, new_rows) = types_of(&written.bytes);
            let moved: Vec<_> = old_rows.iter().filter(|r| !new_rows.contains(r)).collect();
            assert_eq!(moved.len(), usize::from(from != to), "{from} to {to}: {moved:?}");
        }
    }
}

#[test]
fn a_stencil_saved_as_a_drawing_or_a_template_gets_a_part_for_its_page_so_that_no_page_is_left_with_no_part() {
    for to in ["vsdx", "vsdm", "vstx", "vstm"] {
        let original = package("vssx");
        let (doc, _) = read_ok(&original);
        let written = save(&original, &doc, None, &WriteOptions { variant: Some(variant(to)), ..options() }).unwrap();
        let p = parts(&written.bytes);
        assert!(p.contains_key("/visio/pages/page1.xml"), "{to}");
        assert!(text_of(&p, "/visio/pages/pages.xml").contains("<Rel"), "{to}: the page row names its part");
        let pkg = Package::read(&written.bytes, &Limits::default()).unwrap();
        assert_eq!(pkg.rels("/visio/pages/pages.xml").unwrap().of_type(&format!("{VIS}page")).count(), 1, "{to}");
        assert_eq!(pkg.content_type("/visio/pages/page1.xml"), Some("application/vnd.ms-visio.page+xml"), "{to}");
        let (back, report) = read_ok(&written.bytes);
        assert_eq!(back.pages.len(), 1);
        assert!(back.pages.get(0).unwrap().rel_id.is_some(), "{to}: the page has its relationship");
        assert!(report.warnings.is_empty(), "{to}: {:?}", report.warnings);
        // Saved again as a stencil, the file keeps the part it now has: a stencil may hold page parts (5 files do).
        let (back_doc, _) = read_ok(&written.bytes);
        let as_stencil = save(&written.bytes, &back_doc, Some(&back_doc), &WriteOptions { variant: Some(variant("vssx")), ..options() }).unwrap();
        assert!(parts(&as_stencil.bytes).contains_key("/visio/pages/page1.xml"), "{to}");
    }
}

#[test]
fn a_drawing_saved_as_a_stencil_keeps_its_pages_and_masters_and_says_what_the_stencil_holds() {
    let original = package("vsdx");
    let (doc, _) = read_ok(&original);
    let written = save(&original, &doc, None, &WriteOptions { variant: Some(variant("vssx")), ..options() }).unwrap();
    assert_eq!(written.read_back.variant, Some(variant("vssx")));
    assert_eq!(written.document.masters.len(), 2, "the masters are the stencil's shapes");
    assert_eq!(written.document.pages.len(), 1, "V9 of the package spec: every other part is kept, pages included");
    let note = written.warnings.iter().find(|w| w.contains("stencil")).unwrap_or_else(|| panic!("{:?}", written.warnings));
    assert!(note.contains("page"), "it says that the drawing's pages stay in the file: {note}");
    // A drawing with no master makes a stencil with nothing in it, and says so.
    let bare = VsdxBuilder::new().page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1")))).build();
    let (bare_doc, _) = read_ok(&bare);
    let empty = save(&bare, &bare_doc, None, &WriteOptions { variant: Some(variant("vssx")), ..options() }).unwrap();
    assert!(empty.warnings.iter().any(|w| w.contains("no masters")), "{:?}", empty.warnings);
    // A drawing saved as a template, or a template as a drawing, says nothing: it is the same package under another type.
    let as_template = save(&original, &doc, None, &WriteOptions { variant: Some(variant("vstx")), ..options() }).unwrap();
    assert!(as_template.warnings.is_empty(), "{:?}", as_template.warnings);
}

#[test]
fn a_new_drawing_is_written_as_any_of_the_six_and_a_stencil_of_its_masters_reads_back_with_them() {
    let doc = read_ok(&package("vsdx")).0;
    for ext in EXTENSIONS {
        let written = write_with(&doc, None, &WriteOptions { variant: Some(variant(ext)), ..options() });
        // A document with a thumbnail-less skeleton: the masters are written whatever the kind.
        let written = written.unwrap_or_else(|e| panic!("{ext}: {e}"));
        assert_eq!(written.read_back.variant, Some(variant(ext)), "{ext}");
        assert_eq!(written.document.masters.len(), 2, "{ext}");
        assert!(!parts(&written.bytes).keys().any(|k| k.contains("vbaProject")), "{ext}: a new file has no macro project, whatever its type");
    }
}

// ------------------------------------------------------------------------------------------------------------------ macros

#[test]
fn a_macro_enabled_file_saved_as_a_macro_free_type_is_refused_until_the_caller_chooses_and_nothing_is_written() {
    for from in ["vsdm", "vssm", "vstm"] {
        let original = package_with_macros(from);
        let (doc, _) = read_ok(&original);
        for to in ["vsdx", "vssx", "vstx"] {
            let error = save(&original, &doc, None, &WriteOptions { variant: Some(variant(to)), ..options() }).unwrap_err();
            assert_eq!(error, WriteError::MacroProject, "{from} to {to}");
            assert_eq!(error.class(), "macro_project_dropped");
            let words = error.to_string();
            assert!(words.contains("macro") && words.contains("leave"), "the error says what the choice is: {words}");
        }
    }
}

#[test]
fn a_macro_project_chosen_to_be_dropped_goes_with_its_signatures_relationships_and_types_and_the_file_says_so() {
    for (from, to) in [("vsdm", "vsdx"), ("vssm", "vssx"), ("vstm", "vstx"), ("vsdm", "vssx"), ("vstm", "vsdx")] {
        let original = package_with_macros(from);
        let (doc, _) = read_ok(&original);
        let written = save(&original, &doc, None, &WriteOptions { variant: Some(variant(to)), drop_macros: true, ..options() }).unwrap();
        let after = parts(&written.bytes);
        assert!(!after.keys().any(|k| k.to_ascii_lowercase().contains("vbaproject")), "{from} to {to}: {:?}", after.keys());
        let pkg = Package::read(&written.bytes, &Limits::default()).unwrap();
        assert!(pkg.rels("/visio/document.xml").unwrap().items.iter().all(|r| r.ty != VBA_REL), "{from} to {to}: no relationship to a project");
        assert!(pkg.content_types().overrides.iter().all(|(_, t)| !t.contains("vbaProject")), "{from} to {to}: no type for a part that is gone");
        assert!(pkg.content_types().defaults.iter().any(|(e, _)| e == "bin"), "{from} to {to}: a Default row with no part stays");
        assert_eq!(written.read_back.variant, Some(variant(to)));
        assert!(written.read_back.warnings.is_empty(), "{:?}", written.read_back.warnings);
        // Said in words, with the parts.
        let said = written.warnings.iter().find(|w| w.contains("macro")).unwrap_or_else(|| panic!("{from} to {to}: {:?}", written.warnings));
        assert!(said.contains("vbaProject.bin") && said.contains("4 parts"), "{said}");
        assert_eq!(
            written.removed.iter().filter(|n| n.to_ascii_lowercase().contains("vbaproject")).count(),
            5,
            "four parts and the project's relationships: {:?}",
            written.removed
        );
        // Everything else the file had is the original's, byte for byte.
        let before = parts(&original);
        for (name, bytes) in &after {
            if !["/docProps/app.xml", "/docProps/core.xml", "/visio/_rels/document.xml.rels", "/docProps/thumbnail.emf"].contains(&name.as_str()) {
                assert_eq!(before.get(name), Some(bytes), "{from} to {to}: {name}");
            }
        }
    }
}

#[test]
fn dropping_macros_from_a_file_that_has_none_is_not_an_error_and_says_nothing() {
    let original = package("vsdm");
    let (doc, _) = read_ok(&original);
    let written = save(&original, &doc, None, &WriteOptions { variant: Some(variant("vsdx")), drop_macros: true, ..options() }).unwrap();
    assert!(written.warnings.is_empty(), "{:?}", written.warnings);
    assert_eq!(written.read_back.variant, Some(variant("vsdx")));
    // And a macro-free file saved as a macro-enabled one gets the type and no project.
    let plain = package("vsdx");
    let (plain_doc, _) = read_ok(&plain);
    let as_vsdm = save(&plain, &plain_doc, None, &WriteOptions { variant: Some(variant("vsdm")), ..options() }).unwrap();
    assert_eq!(as_vsdm.read_back.variant, Some(variant("vsdm")));
    assert!(!parts(&as_vsdm.bytes).keys().any(|k| k.contains("vbaProject")));
}

#[test]
fn the_choice_to_drop_macros_keeps_a_macro_type_a_macro_type_and_its_project() {
    let original = package_with_macros("vsdm");
    let (doc, _) = read_ok(&original);
    let written = save(&original, &doc, None, &WriteOptions { variant: Some(variant("vssm")), drop_macros: true, ..options() }).unwrap();
    for name in MACRO_PARTS {
        assert_eq!(parts(&written.bytes).get(name), parts(&original).get(name), "{name}: the type still allows macros, so nothing is dropped");
    }
    assert!(written.warnings.iter().all(|w| !w.contains("macro")), "{:?}", written.warnings);
}

#[test]
fn a_project_that_only_a_content_type_names_is_a_macro_project_too() {
    // A signature part with no relationship from anywhere (an orphan) is still VBA, and a macro-free type may not hold it.
    let original = VsdxBuilder::new()
        .document_type(variant("vsdm").content_type())
        .page(PageSpec::new(1, "A"))
        .with(Pkg::new().part("/visio/vbaProjectSignature.bin", "application/vnd.ms-office.vbaProjectSignature", b"orphan"))
        .build();
    let (doc, _) = read_ok(&original);
    let to_vsdx = WriteOptions { variant: Some(variant("vsdx")), ..options() };
    assert_eq!(save(&original, &doc, None, &to_vsdx).unwrap_err(), WriteError::MacroProject);
    let dropped = save(&original, &doc, None, &WriteOptions { drop_macros: true, ..to_vsdx }).unwrap();
    assert!(!parts(&dropped.bytes).contains_key("/visio/vbaProjectSignature.bin"));
}

#[test]
fn other_relationships_of_the_document_keep_their_ids_when_the_project_goes() {
    let original = package_with_macros("vsdm");
    let (doc, _) = read_ok(&original);
    let before = Package::read(&original, &Limits::default()).unwrap().rels("/visio/document.xml").unwrap();
    let written = save(&original, &doc, None, &WriteOptions { variant: Some(variant("vsdx")), drop_macros: true, ..options() }).unwrap();
    let after = Package::read(&written.bytes, &Limits::default()).unwrap().rels("/visio/document.xml").unwrap();
    let kept: Vec<_> = before.items.iter().filter(|r| r.ty != VBA_REL).collect();
    assert_eq!(after.items.len(), kept.len());
    for row in kept {
        assert_eq!(after.get(&row.id).map(|r| (&r.ty, &r.target_raw)), Some((&row.ty, &row.target_raw)), "{}: an Id is never renumbered", row.id);
    }
}

// ------------------------------------------------------------------------------------------------------------------ a copy of a template

#[test]
fn a_copy_made_from_a_template_is_stamped_new_and_a_save_of_a_file_that_is_one_is_not() {
    let original = package("vstx");
    let (doc, _) = read_ok(&original);
    // Saved as a drawing, as the engine does it for a copy of a template: the creation and modification times are those of the copy.
    let copy = save(&original, &doc, None, &WriteOptions { variant: Some(variant("vsdx")), new_copy: true, ..options() }).unwrap();
    let core = text_of(&parts(&copy.bytes), "/docProps/core.xml");
    assert!(core.contains("2023-11-14T22:13:20Z"), "{core}");
    assert_eq!(core.matches("2023-11-14T22:13:20Z").count(), 2, "created and modified: {core}");
    assert_eq!(copy.read_back.variant, Some(variant("vsdx")));
    // The same save without the flag leaves the file's times, here none.
    let plain = save(&original, &doc, None, &WriteOptions { variant: Some(variant("vsdx")), ..options() }).unwrap();
    assert!(!text_of(&parts(&plain.bytes), "/docProps/core.xml").contains("2023-11-14"));
}

#[test]
fn a_copy_of_a_template_with_times_of_its_own_gets_the_new_ones_and_keeps_every_other_property() {
    let core = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>Plan</dc:title><dc:creator>An author</dc:creator><dcterms:created xsi:type="dcterms:W3CDTF">2001-02-03T04:05:06Z</dcterms:created><dcterms:modified xsi:type="dcterms:W3CDTF">2002-02-03T04:05:06Z</dcterms:modified></cp:coreProperties>"#;
    let original = VsdxBuilder::new().document_type(variant("vstx").content_type()).core(core).page(PageSpec::new(1, "A")).build();
    let (doc, _) = read_ok(&original);
    let copy = save(&original, &doc, None, &WriteOptions { variant: Some(variant("vsdx")), new_copy: true, ..options() }).unwrap();
    let text = text_of(&parts(&copy.bytes), "/docProps/core.xml");
    assert!(!text.contains("2001-02-03") && !text.contains("2002-02-03"), "{text}");
    assert_eq!(text.matches("2023-11-14T22:13:20Z").count(), 2, "{text}");
    assert!(text.contains("<dc:creator>An author</dc:creator>") && text.contains("<dc:title>Plan</dc:title>"), "{text}");
}

// ------------------------------------------------------------------------------------------------------------------ odd packages

#[test]
fn names_and_rows_a_real_package_may_have_survive_a_save_with_nothing_edited() {
    use diacraft_testkit::package::ZipItem;
    // A name with a letter beyond ASCII and a space (percent-encoded in the archive), a name in capitals, a folder item, a part with
    // no content type, a `Default` and an `Override` for parts that are not there, and a relationship to a part that is not there.
    let original = VsdxBuilder::new()
        .page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1"))))
        .with(
            Pkg::new()
                .part("/visio/media/image\u{e9} 1.png", "image/png", b"\x89PNG one")
                .part("/Visio/EXTRA/CAPS.DAT", "application/x-caps", b"capitals")
                .part_without_override("/visio/loose.zzz", b"no type")
                .default_type("qqq", "application/x-never-used")
                .rel("/", "rId77", "urn:example:gone", "visio/not-there.xml")
                .item(ZipItem::stored("visio/pages/", b"")),
        )
        .build();
    let (doc, report) = read_ok(&original);
    let written = save(&original, &doc, None, &options()).unwrap();
    let (before, after) = (parts(&original), parts(&written.bytes));
    assert!(before.contains_key("/visio/media/image\u{e9} 1.png"), "{:?}", before.keys());
    for (name, bytes) in before.iter().filter(|(n, _)| *n != "/docProps/app.xml") {
        assert_eq!(after.get(name), Some(bytes), "{name}");
    }
    assert_eq!(after.len(), before.len(), "The folder item is no part and is not written");
    let pkg = Package::read(&written.bytes, &Limits::default()).unwrap();
    assert!(pkg.content_types().defaults.iter().any(|(e, _)| e == "qqq"), "A Default for no part stays");
    assert_eq!(pkg.rels("/").unwrap().get("rId77").map(|r| r.target_raw.as_str()), Some("visio/not-there.xml"), "a relationship to nowhere stays");
    // The part with no type gets one so that the output is legal, and the save says so.
    assert_eq!(pkg.content_type("/visio/loose.zzz"), Some("application/octet-stream"));
    assert!(written.warnings.iter().any(|w| w.contains("content type")), "{:?}", written.warnings);
    assert!(report.warnings.len() <= 3, "{:?}", report.warnings);
}

#[test]
fn an_unreadable_property_part_is_kept_as_it_was_and_the_save_says_so() {
    // The reader opens a file whose app.xml is not XML (a warning), so the save must not refuse it; the part stays, and so does what it says.
    let bad = "<Properties><Application>Old writer";
    let original = VsdxBuilder::new().app(bad).page(PageSpec::new(1, "A")).build();
    let (doc, report) = read_ok(&original);
    assert_eq!(report.warning_count("properties_unreadable"), 1);
    let written = save(&original, &doc, None, &options()).unwrap();
    assert_eq!(parts(&written.bytes).get("/docProps/app.xml").map(Vec::as_slice), Some(bad.as_bytes()));
    assert!(written.warnings.iter().any(|w| w.contains("docProps/app.xml") && w.contains("kept as it was")), "{:?}", written.warnings);
    // The same with an edit and a template copy: core.xml too.
    let core_bad = "<cp:coreProperties";
    let both = VsdxBuilder::new().core(core_bad).app(bad).document_type(variant("vstx").content_type()).page(PageSpec::new(1, "A")).build();
    let (doc, _) = read_ok(&both);
    let copy = save(&both, &doc, None, &WriteOptions { variant: Some(variant("vsdx")), new_copy: true, ..options() }).unwrap();
    let after = parts(&copy.bytes);
    assert_eq!(after.get("/docProps/core.xml").map(Vec::as_slice), Some(core_bad.as_bytes()));
    assert_eq!(copy.warnings.iter().filter(|w| w.contains("kept as it was")).count(), 2, "{:?}", copy.warnings);
}

#[test]
fn an_index_that_is_not_the_one_it_was_reached_as_is_kept_as_bytes_by_a_save_with_nothing_edited() {
    // The reader reads a masters index whose root is not `Masters` as unknown, so its relationships are never read: the save of a file
    // like that must not read them either, and the part stays.
    let masters = r#"<Foreign xmlns="urn:example"/>"#;
    let pkg = VsdxBuilder::new().page(PageSpec::new(1, "A")).master(MasterSpec::new(1, "M")).pkg();
    let mut items = crate::round_trip::flat(&pkg);
    for (name, bytes) in &mut items {
        match name.as_str() {
            "visio/masters/masters.xml" => *bytes = masters.as_bytes().to_vec(),
            "visio/masters/_rels/masters.xml.rels" => *bytes = b"<Relationships".to_vec(),
            _ => {}
        }
    }
    let original = crate::round_trip::zipped(&items);
    let (doc, _) = read_ok(&original);
    let written = save(&original, &doc, None, &options()).unwrap();
    assert_eq!(parts(&written.bytes).get("/visio/masters/masters.xml").map(Vec::as_slice), Some(masters.as_bytes()));
}

// ------------------------------------------------------------------------------------------------------------------ hostile cost

/// A macro-enabled package with `n` parts of a VBA type, each reached by a relationship of the project's type from the document.
fn many_vba_parts(n: usize) -> Vec<u8> {
    let mut pkg = Pkg::new();
    for i in 0..n {
        pkg = pkg
            .part(&format!("/visio/v{i}.bin"), "application/vnd.ms-office.vbaProjectSignature", b"s")
            .rel("/visio/document.xml", &format!("rId{}", 100 + i), VBA_REL, &format!("v{i}.bin"))
            // Each is a project with relationships of its own, one to the next: a signature relationship that the drop must follow.
            .rel(&format!("/visio/v{i}.bin"), "rId1", SIG_REL, &format!("v{}.bin", (i + 1) % n));
    }
    VsdxBuilder::new().document_type(variant("vsdm").content_type()).page(PageSpec::new(1, "A")).with(pkg).build()
}

#[test]
fn dropping_a_macro_project_costs_what_a_save_costs_however_many_parts_and_relationships_are_vba() {
    let n = 20_000;
    let original = many_vba_parts(n);
    let (doc, _) = read_ok(&original);
    // The control: the same file saved as a type that keeps them. Counted, not timed (the ratio of two timings failed under load): the rows of the
    // relationships, the parts and the content types the removal visits (`diacraft_vsdx::work`), which a save that keeps the project does not visit.
    diacraft_vsdx::work::reset();
    let kept = save(&original, &doc, None, &WriteOptions { variant: Some(variant("vssm")), ..options() }).unwrap();
    let kept_visits = diacraft_vsdx::work::rels_visited() + diacraft_vsdx::work::parts_visited() + diacraft_vsdx::work::overrides_visited();
    assert!(kept.macros_dropped.is_empty());
    assert_eq!(kept_visits, 0, "a save that keeps the project removes nothing");
    diacraft_vsdx::work::reset();
    let dropped = save(&original, &doc, None, &WriteOptions { variant: Some(variant("vsdx")), drop_macros: true, ..options() }).unwrap();
    let (rels, parts_named, overrides) =
        (diacraft_vsdx::work::rels_visited(), diacraft_vsdx::work::parts_visited(), diacraft_vsdx::work::overrides_visited());
    eprintln!(
        "[work] {n} VBA parts and relationships: dropped by visiting {rels} relationship rows, {parts_named} parts and {overrides} content types"
    );
    // One pass over each list, however many parts there are: each row, part and content type is visited once, and a few more for the rows and
    // types that are not the project's (a pass for each part would be 20,000 times the list).
    let limit = u64::try_from(n + 64).unwrap();
    assert!(
        rels <= limit && parts_named <= limit && overrides <= limit,
        "{rels} rows, {parts_named} parts and {overrides} content types visited for {n} parts"
    );
    assert_eq!(dropped.macros_dropped.len(), n);
    let after = parts(&dropped.bytes);
    assert!(!after.keys().any(|k| k.contains("/visio/v") || k.contains("_rels/v")), "every part of the project is gone, relationships parts too");
    assert_eq!(dropped.warnings.iter().filter(|w| w.contains("macro")).count(), 1, "one warning, with the names cut short and the count");
    assert!(dropped.warnings.iter().all(|w| w.len() < 600), "{:?}", dropped.warnings);
}

#[test]
fn a_stencil_of_the_most_pages_saved_as_a_drawing_gets_its_page_parts_in_work_linear_in_the_pages() {
    let n = diacraft_doc::MAX_PAGES;
    let original = (1..=n)
        .fold(VsdxBuilder::new().document_type(variant("vssx").content_type()), |b, i| b.page(PageSpec::new(i as u32, "P").without_part()))
        .build();
    let (doc, _) = read_ok(&original);
    assert_eq!(doc.pages.len(), n);
    // Counted, not timed (the ratio of two timings failed under load): the names tried for the new parts and the rows looked at to find a free
    // `rId`. The names are found in one walk up the numbers and the ids in use are collected once, so the work is the pages plus what was there:
    // the claim is linear, and a save that keeps the stencil does none of it. (Each part used to try the names from the first and each
    // relationship to build the set of the ids in use again: 500,500 tries and as many rows for the 1,000 pages the model takes at most.)
    diacraft_vsdx::work::reset();
    save(&original, &doc, None, &WriteOptions { variant: Some(variant("vssx")), ..options() }).unwrap();
    assert_eq!((diacraft_vsdx::work::name_probes(), diacraft_vsdx::work::id_scans()), (0, 0), "a save that keeps the stencil makes no part");
    diacraft_vsdx::work::reset();
    let made = save(&original, &doc, None, &WriteOptions { variant: Some(variant("vsdx")), ..options() }).unwrap();
    let (probes, scans) = (diacraft_vsdx::work::name_probes(), diacraft_vsdx::work::id_scans());
    eprintln!("[work] a stencil of {n} pages with no part: given {n} parts by {probes} names tried and {scans} rows looked at");
    let limit = u64::try_from(n + 64).unwrap();
    assert!(probes >= u64::try_from(n).unwrap(), "a name is tried for each part: {probes}");
    assert!(probes <= limit && scans <= limit, "{probes} names tried and {scans} rows looked at for {n} pages");
    assert_eq!(made.added.iter().filter(|a| a.starts_with("/visio/pages/page")).count(), n);
    assert_eq!(made.document.pages.iter().filter(|p| p.rel_id.is_some()).count(), n);
}

#[test]
fn a_stencil_with_page_parts_already_saved_as_a_drawing_walks_the_names_and_rows_it_has_once() {
    // 400 of the 1,000 pages have parts (`page1.xml` to `page400.xml`, `rId1` to `rId400`) and 600 have none. The new parts take `page401.xml`
    // on and the new relationships `rId401` on: found by walking past the 400 names and 400 rows once, in all 1,000 tries and 400 rows, and not
    // once for each of the 600 (about 420,000 of each). Every page ends with a part of its own and a relationship of its own.
    let (n, had) = (diacraft_doc::MAX_PAGES, 400usize);
    let original = (1..=n)
        .fold(VsdxBuilder::new().document_type(variant("vssx").content_type()), |b, i| {
            let page = PageSpec::new(i as u32, "P");
            b.page(if i <= had { page } else { page.without_part() })
        })
        .build();
    let (doc, _) = read_ok(&original);
    assert_eq!(doc.pages.iter().filter(|p| p.rel_id.is_some()).count(), had);
    diacraft_vsdx::work::reset();
    let made = save(&original, &doc, None, &WriteOptions { variant: Some(variant("vsdx")), ..options() }).unwrap();
    let (probes, scans) = (diacraft_vsdx::work::name_probes(), diacraft_vsdx::work::id_scans());
    eprintln!("[work] a stencil of {n} pages, {had} with a part: given {} parts by {probes} names tried and {scans} rows looked at", n - had);
    let limit = u64::try_from(n + 64).unwrap();
    assert!(probes <= limit && scans <= limit, "{probes} names tried and {scans} rows looked at for {n} pages, {had} with parts");
    assert_eq!(made.added.iter().filter(|a| a.starts_with("/visio/pages/page")).count(), n - had);
    let ids: std::collections::BTreeSet<&str> = made.document.pages.iter().filter_map(|p| p.rel_id.as_deref()).collect();
    assert_eq!(ids.len(), n, "every page has a relationship of its own");
    let after = parts(&made.bytes);
    for i in 1..=n {
        assert!(after.contains_key(&format!("/visio/pages/page{i}.xml")), "page{i}.xml is there");
    }
    assert!(ids.contains("rId401") && ids.contains("rId1000") && !ids.contains("rId1001"), "the new ids follow the old");
}

// ------------------------------------------------------------------------------------------------------------------ names that hold words

/// A part name that holds a person's words, in characters a part name may hold unescaped: not package structure (`part_label` calls it by its role).
const WORDS: &str = "Secret(story)~";

/// The package `bytes` with the part `from` named `to`, in the content types and the relationships too.
fn renamed(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
    let pkg = Package::read(bytes, &Limits::default()).unwrap();
    let mut items: Vec<(String, Vec<u8>)> = vec![("[Content_Types].xml".to_owned(), Vec::new())];
    items.extend(pkg.parts().map(|name| (name.trim_start_matches('/').to_owned(), pkg.part(name).unwrap().to_vec())));
    // The content types stream is not a part: take it from the archive as the reader sees it.
    let types = std::str::from_utf8(&raw_types(bytes)).unwrap().to_owned();
    if let Some(first) = items.first_mut() {
        first.1 = types.replace(from, to).into_bytes();
    }
    for (name, content) in &mut items {
        if name.as_str() == from.trim_start_matches('/') {
            *name = to.trim_start_matches('/').to_owned();
        } else if name.ends_with(".rels") {
            let text = String::from_utf8(content.clone()).unwrap();
            *content = text.replace(from.trim_start_matches('/'), to.trim_start_matches('/')).into_bytes();
        }
    }
    crate::round_trip::zipped(&items)
}

/// `[Content_Types].xml` of an archive, as text bytes.
fn raw_types(bytes: &[u8]) -> Vec<u8> {
    let pkg = Package::read(bytes, &Limits::default()).unwrap();
    let (defaults, overrides) = (pkg.content_types().defaults.clone(), pkg.content_types().overrides.clone());
    let mut xml =
        String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">");
    for (extension, content_type) in defaults {
        xml.push_str(&format!("<Default Extension=\"{extension}\" ContentType=\"{content_type}\"/>"));
    }
    for (part, content_type) in overrides {
        xml.push_str(&format!("<Override PartName=\"{part}\" ContentType=\"{content_type}\"/>"));
    }
    xml.push_str("</Types>");
    xml.into_bytes()
}

/// Every text a save tells: the warnings, and the words of the error.
fn told(result: &Result<Written, WriteError>) -> String {
    match result {
        Ok(w) => w.warnings.join("\n"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn a_part_name_that_holds_words_is_never_in_a_warning_about_a_property_part() {
    let app = format!("/docProps/{WORDS}app.xml");
    let core = format!("/docProps/{WORDS}core.xml");
    let built = VsdxBuilder::new()
        .document_type(variant("vstx").content_type())
        .app("<Properties><Application>")
        .core("<cp:coreProperties")
        .page(PageSpec::new(1, "A"))
        .build();
    let original = renamed(&renamed(&built, "/docProps/app.xml", &app), "/docProps/core.xml", &core);
    let (doc, report) = read_ok(&original);
    assert_eq!(report.warning_count("properties_unreadable"), 2, "the file opens, with a warning for each");
    assert!(parts(&original).contains_key(&app), "the test names the part with the words: {:?}", parts(&original).keys());
    let result = save(&original, &doc, None, &WriteOptions { variant: Some(variant("vsdx")), new_copy: true, ..options() });
    let said = told(&result);
    assert_eq!(result.as_ref().unwrap().warnings.iter().filter(|w| w.contains("a property part was kept as it was")).count(), 2, "{said}");
    assert!(!said.contains("Secret") && !said.contains("story"), "{said}");
    // The in-process lists of the parts written keep the names (they are for the caller that has the file); the words of a warning do not.
    assert!(!format!("{:?}", result.unwrap().warnings).contains("Secret"));
}

#[test]
fn a_part_name_that_holds_words_is_never_in_the_warning_about_dropped_macros() {
    let project = format!("/visio/{WORDS}.bin");
    let signature = format!("/visio/{WORDS}sig.bin");
    let pkg = Pkg::new()
        .default_type("bin", "application/vnd.ms-office.vbaProject")
        .part_without_override(&project, VBA_BYTES)
        .rel("/visio/document.xml", "rId50", VBA_REL, &format!("{WORDS}.bin"))
        .part(&signature, "application/vnd.ms-office.vbaProjectSignature", b"sig")
        .rel(&project, "rId1", SIG_REL_FOR_WORDS, &format!("{WORDS}sig.bin"));
    let original = VsdxBuilder::new().document_type(variant("vsdm").content_type()).page(PageSpec::new(1, "A")).with(pkg).build();
    let (doc, _) = read_ok(&original);
    let result = save(&original, &doc, None, &WriteOptions { variant: Some(variant("vsdx")), drop_macros: true, ..options() });
    let said = told(&result);
    let written = result.unwrap();
    assert_eq!(written.macros_dropped.len(), 2, "both parts are dropped: {said}");
    assert!(said.contains("macro project was left out") && said.contains("2 parts removed"), "the count is told: {said}");
    assert!(!said.contains("Secret") && !said.contains("story"), "{said}");
    // One part of each kind: the name that is package structure is told, the other is counted.
    let mixed = Pkg::new()
        .default_type("bin", "application/vnd.ms-office.vbaProject")
        .part_without_override("/visio/vbaProject.bin", VBA_BYTES)
        .rel("/visio/document.xml", "rId50", VBA_REL, "vbaProject.bin")
        .part(&signature, "application/vnd.ms-office.vbaProjectSignature", b"sig");
    let original = VsdxBuilder::new().document_type(variant("vsdm").content_type()).page(PageSpec::new(1, "A")).with(mixed).build();
    let (doc, _) = read_ok(&original);
    let said = told(&save(&original, &doc, None, &WriteOptions { variant: Some(variant("vsdx")), drop_macros: true, ..options() }));
    assert!(said.contains("/visio/vbaProject.bin and 1 more") && !said.contains("Secret"), "{said}");
}

const SIG_REL_FOR_WORDS: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProjectSignature";

#[test]
fn a_part_name_that_holds_words_is_never_in_the_words_of_a_write_error() {
    use diacraft_opc::xml::XmlWriteError;
    let error = WriteError::Xml { part: format!("/visio/pages/{WORDS}.xml"), error: XmlWriteError::BadName("x".to_owned()) };
    assert!(!error.to_string().contains("Secret"), "{error}");
    assert!(error.to_string().contains("a part cannot be written"), "{error}");
    // A name that is package structure is told.
    let plain = WriteError::Xml { part: "/visio/pages/page1.xml".to_owned(), error: XmlWriteError::BadName("x".to_owned()) };
    assert!(plain.to_string().contains("/visio/pages/page1.xml"), "{plain}");
}
