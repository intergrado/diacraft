//! What a save does with the content types and with the property parts of a package.
//!
//! The sample-file round trip found four things in the writer, and each has its tests here, with the packages built in code:
//!
//! 1. `[Content_Types].xml` was rewritten in the writer's own layout though the rows were the file's: it is now the file's bytes when no row
//!    changes, and the file's layout when one does.
//! 2. An extended properties part hung under another properties relationship was not found, so it kept the old writer's name.
//! 3. Relationships parts with no content type got `application/octet-stream` by an `Override` each: they get the relationships type by the one
//!    `Default` every sample has.
//! 4. The names and ids of many new pages or masters were found by a scan for each: they are found by a walk up the numbers made once.

use std::sync::Arc;

use diacraft_doc::{CellAddr, CellEdit, Document, Master, Owner, ShapeRef, SheetRef, Value, WriteOrigin};
use diacraft_opc::name::is_rels_part;
use diacraft_opc::{Limits, Package};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use diacraft_testkit::package::{CT_DRAWING, CT_PAGE, CT_RELS, MasterSpec, OD, PK, PageSpec, Pkg, VsdxBuilder, cell, shape};
use diacraft_vsdx::{OriginalBytes, Variant, WriteOptions, Written, write_with};

use crate::read_ok;
use crate::round_trip::{flat, zipped};

const NOW: i64 = 1_700_000_000;
const NOW_TEXT: &str = "2023-11-14T22:13:20Z";
const NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const CT_OCTET: &str = "application/octet-stream";

fn options() -> WriteOptions {
    WriteOptions { now: Some(NOW), ..WriteOptions::default() }
}

fn save(original: &[u8], doc: &Document, options: &WriteOptions) -> Written {
    write_with(doc, Some(&OriginalBytes { bytes: original, baseline: None }), options).unwrap_or_else(|e| panic!("{e}"))
}

fn parts(bytes: &[u8]) -> std::collections::BTreeMap<String, Vec<u8>> {
    let pkg = Package::read(bytes, &Limits::default()).unwrap();
    pkg.parts().map(|p| (p.to_owned(), pkg.part(p).unwrap().to_vec())).collect()
}

fn text_of(parts: &std::collections::BTreeMap<String, Vec<u8>>, name: &str) -> String {
    String::from_utf8(parts.get(name).unwrap_or_else(|| panic!("no part {name}: {:?}", parts.keys())).clone()).unwrap()
}

/// The `Default` rows and the `Override` rows of a package, in the order of its stream.
type Rows = (Vec<(String, String)>, Vec<(String, String)>);

fn rows(bytes: &[u8]) -> Rows {
    let pkg = Package::read(bytes, &Limits::default()).unwrap();
    (pkg.content_types().defaults.clone(), pkg.content_types().overrides.clone())
}

/// `[Content_Types].xml` of a package, as the archive holds it.
fn stream(bytes: &[u8]) -> String {
    String::from_utf8(Package::read(bytes, &Limits::default()).unwrap().content_types_bytes().to_vec()).unwrap()
}

fn move_first_shape(doc: &mut Document) {
    let sheet = SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0));
    doc.write_cell(&sheet, &CellAddr::direct("PinX"), CellEdit::Value(Value::from_text("7")), WriteOrigin::Drawing).unwrap();
}

// ------------------------------------------------------------------------------------------------------------------------ the inputs

/// A drawing of two pages and a master, as a list of archive items with the content types stream first.
fn items() -> Vec<(String, Vec<u8>)> {
    flat(
        &VsdxBuilder::new()
            .page(PageSpec::new(1, "First").shapes(&shape(5, &cell("PinX", "1"))))
            .page(PageSpec::new(2, "Second"))
            .master(MasterSpec::new(1, "Box").shapes(&shape(4, &cell("Width", "1"))))
            .pkg(),
    )
}

/// The package of `items` with the content types stream replaced by `stream`.
fn with_stream(items: &[(String, Vec<u8>)], stream: &str) -> Vec<u8> {
    let mut items = items.to_vec();
    items[0].1 = stream.as_bytes().to_vec();
    zipped(&items)
}

/// The ways a writer may lay out `[Content_Types].xml`: how the reference app's own saves do (no line end, no white space), a tool's (single-quoted declaration,
/// CR LF, indented rows, a comment), a bare one (no declaration, tabs), one with a byte order mark, and one in a prefix.
#[derive(Clone, Copy, Debug)]
enum Layout {
    Packaging,
    Tool,
    Bare,
    Marked,
    Prefixed,
}

const LAYOUTS: [Layout; 5] = [Layout::Packaging, Layout::Tool, Layout::Bare, Layout::Marked, Layout::Prefixed];

/// The rows laid out as `layout` lays them out: the same text for any number of rows.
fn laid_out(rows: &Rows, layout: Layout) -> String {
    let (head, open, between, close, prefix) = match layout {
        Layout::Packaging => ("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>", format!("<Types xmlns=\"{NS}\">"), "", "</Types>", ""),
        Layout::Tool => (
            "<?xml version='1.0' encoding='UTF-8' standalone='yes'?>\r\n",
            format!("<Types xmlns=\"{NS}\">\r\n  <!-- written by a tool -->"),
            "\r\n  ",
            "\r\n</Types>\r\n",
            "",
        ),
        Layout::Bare => ("", format!("<Types xmlns=\"{NS}\">"), "\n\t", "\n</Types>", ""),
        Layout::Marked => ("\u{FEFF}<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n", format!("<Types xmlns=\"{NS}\">"), "", "</Types>", ""),
        Layout::Prefixed => {
            ("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>", format!("<ct:Types xmlns:ct=\"{NS}\">"), "", "</ct:Types>", "ct:")
        }
    };
    let mut out = format!("{head}{open}");
    for (extension, content_type) in &rows.0 {
        out.push_str(&format!("{between}<{prefix}Default Extension=\"{extension}\" ContentType=\"{content_type}\"/>"));
    }
    for (part, content_type) in &rows.1 {
        out.push_str(&format!("{between}<{prefix}Override PartName=\"{part}\" ContentType=\"{content_type}\"/>"));
    }
    out.push_str(close);
    out
}

/// The package of [`items`] with its stream in `layout`.
fn in_layout(layout: Layout) -> Vec<u8> {
    let items = items();
    let rows = rows(&zipped(&items));
    with_stream(&items, &laid_out(&rows, layout))
}

// ------------------------------------------------------------------------------------------------------------ 1. the stream, kept as it was

#[test]
fn a_save_that_changes_no_row_writes_the_stream_it_read_byte_for_byte_in_every_layout() {
    for layout in LAYOUTS {
        let original = in_layout(layout);
        let written_by_the_test = laid_out(&rows(&zipped(&items())), layout);
        assert_eq!(stream(&original), written_by_the_test, "{layout:?}: the input is what the test says it is");
        let (mut doc, _) = read_ok(&original);
        let unedited = save(&original, &doc, &options());
        assert_eq!(stream(&unedited.bytes), written_by_the_test, "{layout:?}: nothing edited");
        // A cell edited makes no part and takes none out: the rows are the file's.
        move_first_shape(&mut doc);
        let edited = save(&original, &doc, &options());
        assert_eq!(stream(&edited.bytes), written_by_the_test, "{layout:?}: a cell edited");
        assert!(
            parts(&edited.bytes).get("/visio/pages/page1.xml") != parts(&original).get("/visio/pages/page1.xml"),
            "{layout:?}: the edit was written"
        );
    }
}

#[test]
fn rows_a_save_carries_and_does_not_understand_are_kept_in_the_stream_unchanged() {
    // Duplicates (carried; the first wins on read), a row that lacks an attribute, a row for no part, an element that is no row, a comment.
    let items = items();
    let rows = rows(&zipped(&items));
    let plain = laid_out(&rows, Layout::Packaging);
    let extra = concat!(
        "<Override PartName=\"/gone.xml\"/><Foo a=\"b\"/><!-- c --><Default Extension=\"zzz\" ContentType=\"application/x-z\"/>",
        "<Default Extension=\"xml\" ContentType=\"text/xml\"/><Override PartName=\"/visio/pages/page1.xml\" ContentType=\"text/page\"/>",
    );
    let odd = plain.replace("</Types>", &format!("{extra}</Types>"));
    let original = with_stream(&items, &odd);
    let (mut doc, _) = read_ok(&original);
    assert_eq!(stream(&save(&original, &doc, &options()).bytes), odd, "unchanged");
    // A row added after them all, and every one of theirs is still there, in its place.
    doc.add_page(Some("Third")).unwrap();
    let added = save(&original, &doc, &options());
    let want = odd.replace("</Types>", &format!("<Override PartName=\"/visio/pages/page3.xml\" ContentType=\"{CT_PAGE}\"/></Types>"));
    assert_eq!(stream(&added.bytes), want);
}

#[test]
fn a_page_added_adds_one_row_set_apart_as_the_others_are() {
    for layout in LAYOUTS {
        let original = in_layout(layout);
        let (mut doc, _) = read_ok(&original);
        doc.add_page(Some("Third")).unwrap();
        let written = save(&original, &doc, &options());
        let mut want = rows(&original);
        want.1.push(("/visio/pages/page3.xml".to_owned(), CT_PAGE.to_owned()));
        assert_eq!(stream(&written.bytes), laid_out(&want, layout), "{layout:?}: the declaration, the white space and the comment are the file's");
        assert_eq!(rows(&written.bytes), want, "{layout:?}");
    }
}

#[test]
fn a_page_taken_out_takes_its_row_and_the_white_space_before_it() {
    for layout in LAYOUTS {
        let original = in_layout(layout);
        let (mut doc, _) = read_ok(&original);
        doc.pages.remove(1).unwrap();
        let written = save(&original, &doc, &options());
        let mut want = rows(&original);
        want.1.retain(|(part, _)| part != "/visio/pages/page2.xml");
        assert_eq!(stream(&written.bytes), laid_out(&want, layout), "{layout:?}: no empty line is left where the row was");
    }
}

#[test]
fn a_save_as_another_type_changes_the_one_row_in_place_and_nothing_else() {
    let stencil = Variant::from_extension("vssx").unwrap();
    for layout in LAYOUTS {
        let original = in_layout(layout);
        let (doc, _) = read_ok(&original);
        let written = save(&original, &doc, &WriteOptions { variant: Some(stencil), ..options() });
        let want = stream(&original).replace(CT_DRAWING, stencil.content_type());
        assert_ne!(want, stream(&original));
        assert_eq!(stream(&written.bytes), want, "{layout:?}");
    }
}

// ------------------------------------------------------------------------------------------------------------ 2. the extended properties

/// An `app.xml` of the writer named, with the two pages of [`items`] in its page group.
fn app_xml(application: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><Application>{application}</Application><AppVersion>16.0000</AppVersion><HeadingPairs><vt:vector size=\"2\" baseType=\"variant\"><vt:variant><vt:lpstr>Pages</vt:lpstr></vt:variant><vt:variant><vt:i4>2</vt:i4></vt:variant></vt:vector></HeadingPairs><TitlesOfParts><vt:vector size=\"2\" baseType=\"lpstr\"><vt:lpstr>First</vt:lpstr><vt:lpstr>Second</vt:lpstr></vt:vector></TitlesOfParts></Properties>"
    )
}

/// The package relationships of a package made with the app part `app` and `edit` applied to the text of `_rels/.rels`.
fn with_package_rels(app: &str, edit: impl Fn(String) -> String) -> Vec<u8> {
    let pkg = VsdxBuilder::new()
        .app(app)
        .page(PageSpec::new(1, "First").shapes(&shape(5, &cell("PinX", "1"))))
        .page(PageSpec::new(2, "Second"))
        .master(MasterSpec::new(1, "Box").shapes(&shape(4, &cell("Width", "1"))))
        .pkg();
    let mut items = flat(&pkg);
    for (name, bytes) in &mut items {
        if name == "_rels/.rels" {
            *bytes = edit(String::from_utf8(bytes.clone()).unwrap()).into_bytes();
        }
    }
    zipped(&items)
}

/// The file whose extended properties hang under a core properties relationship, with its own core part under another.
fn app_under_a_core_relationship(application: &str) -> Vec<u8> {
    with_package_rels(&app_xml(application), |t| {
        let swapped = t.replace(&format!("{OD}extended-properties"), &format!("{PK}core-properties"));
        assert_ne!(swapped, t, "the relationship to the app part was there to change");
        swapped
    })
}

#[test]
fn the_app_part_is_found_by_its_content_type_under_a_core_properties_relationship() {
    let original = app_under_a_core_relationship("Another Drawing App");
    let (doc, report) = read_ok(&original);
    // The reader finds it the same way: the application is the app part's, the title the core part's, and neither part is taken for the other.
    assert_eq!(report.application.as_deref(), Some("Another Drawing App"));
    assert_eq!(report.warning_count("properties_unreadable"), 0, "{:?}", report.warnings);
    assert_eq!(&*doc.title, "Test drawing");
    let written = save(&original, &doc, &options());
    let (before, after) = (parts(&original), parts(&written.bytes));
    // The writer's name is ours and every other element is the file's.
    assert_eq!(text_of(&after, "/docProps/app.xml"), text_of(&before, "/docProps/app.xml").replace("Another Drawing App", "DiaCraft"));
    assert_eq!(after.get("/docProps/core.xml"), before.get("/docProps/core.xml"), "the core part is the one the core relationship also names");
    assert!(written.warnings.iter().all(|w| !w.contains("kept as it was")), "{:?}", written.warnings);
}

#[test]
fn an_edit_updates_the_app_part_found_that_way_and_the_core_part_found_the_ordinary_way() {
    let original = app_under_a_core_relationship("Another Drawing App");
    let (mut doc, _) = read_ok(&original);
    doc.add_page(Some("Third")).unwrap();
    let written = save(&original, &doc, &options());
    let after = parts(&written.bytes);
    let app = text_of(&after, "/docProps/app.xml");
    assert!(app.contains("<Application>DiaCraft</Application>"), "{app}");
    assert!(
        app.contains("<vt:lpstr>Third</vt:lpstr>") && app.contains("<vt:i4>3</vt:i4>") && app.contains("size=\"3\""),
        "the page list follows: {app}"
    );
    let core = text_of(&after, "/docProps/core.xml");
    assert!(core.contains(NOW_TEXT), "modified is set in the core part: {core}");
    assert!(!app.contains(NOW_TEXT), "and not in the app part: {app}");
    assert!(written.warnings.iter().all(|w| !w.contains("kept as it was")), "{:?}", written.warnings);
}

#[test]
fn a_core_properties_relationship_to_the_app_part_alone_is_not_taken_for_the_core_part() {
    // The only core-properties relationship names the app part, and the real core part has none: an edit sets no `modified` anywhere and the save
    // does not say it kept a part as it was (it used to try the app part as the core part, and say so).
    let original = with_package_rels(&app_xml("Another Drawing App"), |t| {
        let no_core = t.replace(&format!("<Relationship Id=\"rId2\" Type=\"{PK}core-properties\" Target=\"docProps/core.xml\"/>"), "");
        assert_ne!(no_core, t);
        no_core.replace(&format!("{OD}extended-properties"), &format!("{PK}core-properties"))
    });
    let (mut doc, _) = read_ok(&original);
    move_first_shape(&mut doc);
    let written = save(&original, &doc, &options());
    let (before, after) = (parts(&original), parts(&written.bytes));
    assert!(text_of(&after, "/docProps/app.xml").contains("<Application>DiaCraft</Application>"));
    assert_eq!(after.get("/docProps/core.xml"), before.get("/docProps/core.xml"), "no relationship reaches it: it is an unknown part");
    assert!(written.warnings.iter().all(|w| !w.contains("kept as it was")), "{:?}", written.warnings);
}

#[test]
fn the_app_part_is_found_under_a_relationship_of_any_type_and_with_no_content_type_of_its_own() {
    // Under a type of nobody's: the part says what it is.
    let odd = with_package_rels(&app_xml("Another Drawing App"), |t| t.replace(&format!("{OD}extended-properties"), "urn:example:something-else"));
    let (doc, _) = read_ok(&odd);
    let written = save(&odd, &doc, &options());
    assert!(text_of(&parts(&written.bytes), "/docProps/app.xml").contains("<Application>DiaCraft</Application>"));
    // The ordinary relationship to a part with no `Override`, typed by the `Default` of `xml`: the relationship says what it is (as before).
    let items = items();
    let mut types = rows(&zipped(&items));
    types.1.retain(|(part, _)| part != "/docProps/app.xml");
    let untyped = with_stream(&items, &laid_out(&types, Layout::Packaging));
    let (doc, _) = read_ok(&untyped);
    let written = save(&untyped, &doc, &options());
    let after = parts(&written.bytes);
    assert!(text_of(&after, "/docProps/app.xml").contains("<Application>DiaCraft</Application>"), "{:?}", text_of(&after, "/docProps/app.xml"));
    assert_eq!(stream(&written.bytes), stream(&untyped), "and no row is added for it: the Default of xml gives it a type");
}

#[test]
fn a_custom_properties_part_under_another_relationship_is_edited_in_place_and_not_replaced() {
    // A file may hang `docProps/custom.xml` under any properties relationship. A save that asks for `RecalcDocument` used to find none, make a new
    // part of that name, and so replace the one the file had with its user's properties in it.
    let custom = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/custom-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><property fmtid=\"{D5CDD505-2E9C-101B-9397-08002B2CF9AE}\" pid=\"2\" name=\"Mine\"><vt:lpwstr>kept</vt:lpwstr></property></Properties>";
    let pkg = VsdxBuilder::new()
        .page(PageSpec::new(1, "First").shapes(&shape(5, &cell("PinX", "1"))))
        .with(Pkg::new().part("/docProps/custom.xml", "application/vnd.openxmlformats-officedocument.custom-properties+xml", custom).rel(
            "/",
            "rId9",
            "urn:example:the-custom-ones",
            "docProps/custom.xml",
        ))
        .build();
    let (doc, _) = read_ok(&pkg);
    let written = save(&pkg, &doc, &WriteOptions { recalc_document: true, ..options() });
    let after = text_of(&parts(&written.bytes), "/docProps/custom.xml");
    assert!(after.contains("name=\"Mine\"") && after.contains("<vt:lpwstr>kept</vt:lpwstr>"), "the user's property is kept: {after}");
    assert!(
        after.contains("name=\"RecalcDocument\"") && after.matches("RecalcDocument").count() == 1,
        "and the request is in the same part: {after}"
    );
    assert_eq!(
        Package::read(&written.bytes, &Limits::default()).unwrap().rels("/").unwrap().len(),
        4,
        "no relationship was added for a part that was there"
    );
}

// ------------------------------------------------------------------------------------------------------------ 3. relationships parts with no type

/// The package of [`items`] with the stream written by `edit` from its rows (in the packaging layout).
fn with_rows(edit: impl FnOnce(&mut Rows)) -> Vec<u8> {
    let items = items();
    let mut types = rows(&zipped(&items));
    edit(&mut types);
    with_stream(&items, &laid_out(&types, Layout::Packaging))
}

fn rels_parts(bytes: &[u8]) -> Vec<String> {
    parts(bytes).into_keys().filter(|name| is_rels_part(name)).collect()
}

/// The `Default` row of `rels` is not in the stream; the relationships parts are in the package.
fn no_default_for_rels() -> Vec<u8> {
    with_rows(|types| types.0.retain(|(extension, _)| extension != "rels"))
}

#[test]
fn relationships_parts_with_no_type_get_the_relationships_type_by_one_default_and_not_an_override_each() {
    let original = no_default_for_rels();
    let names = rels_parts(&original);
    assert!(names.len() >= 4, "{names:?}");
    assert!(names.iter().all(|n| Package::read(&original, &Limits::default()).unwrap().content_type(n).is_none()), "they have no type to begin with");
    let (doc, _) = read_ok(&original);
    let written = save(&original, &doc, &options());
    let (before, after) = (rows(&original), rows(&written.bytes));
    let mut want = before.clone();
    want.0.push(("rels".to_owned(), CT_RELS.to_owned()));
    assert_eq!(after, want, "one row added, a Default, and the rows of the file as they were");
    assert!(after.1.iter().all(|(_, ct)| ct != CT_OCTET), "{:?}", after.1);
    let pkg = Package::read(&written.bytes, &Limits::default()).unwrap();
    for name in &names {
        assert_eq!(pkg.content_type(name), Some(CT_RELS), "{name}");
    }
    // The save says so, in words and counts.
    let told: Vec<&String> = written.warnings.iter().filter(|w| w.contains("parts had no content type")).collect();
    assert_eq!(told.len(), 1, "{:?}", written.warnings);
    assert!(told[0].starts_with(&format!("{} parts had no content type", names.len())) && told[0].contains("the relationships type"), "{}", told[0]);
    assert!(!told[0].contains(CT_OCTET), "{}", told[0]);
    // The layout of the stream is the file's: the row is the one more.
    assert_eq!(stream(&written.bytes), laid_out(&want, Layout::Packaging));
}

#[test]
fn a_part_of_another_kind_with_no_type_still_gets_octet_stream_by_an_override_and_the_relationships_parts_the_default() {
    let items = items();
    let mut loose = items.clone();
    loose.push(("visio/loose.zzz".to_owned(), b"no type".to_vec()));
    let mut types = rows(&zipped(&items));
    types.0.retain(|(extension, _)| extension != "rels");
    let original = with_stream(&loose, &laid_out(&types, Layout::Packaging));
    let (doc, _) = read_ok(&original);
    let written = save(&original, &doc, &options());
    let after = rows(&written.bytes);
    assert_eq!(after.1.iter().filter(|(_, ct)| ct == CT_OCTET).collect::<Vec<_>>(), [&("/visio/loose.zzz".to_owned(), CT_OCTET.to_owned())]);
    assert_eq!(after.1.len(), types.1.len() + 1, "one Override more, for the one part that is not a relationships part");
    assert_eq!(after.0.iter().filter(|(e, _)| e == "rels").count(), 1);
    let n = rels_parts(&original).len() + 1;
    let told = written.warnings.iter().find(|w| w.contains("parts had no content type")).unwrap();
    assert!(told.starts_with(&format!("{n} parts had no content type: {} relationships parts", n - 1)) && told.ends_with(CT_OCTET), "{told}");
}

#[test]
fn relationships_parts_each_typed_by_an_override_get_no_default_and_no_row_at_all() {
    // Some writers type each relationships part by an `Override` and have no `Default` for `rels`. Every part has a type, so the stream is the file's.
    let items = items();
    let original = {
        let mut types = rows(&zipped(&items));
        types.0.retain(|(extension, _)| extension != "rels");
        for name in rels_parts(&zipped(&items)) {
            types.1.push((name, CT_RELS.to_owned()));
        }
        with_stream(&items, &laid_out(&types, Layout::Tool))
    };
    let (mut doc, _) = read_ok(&original);
    let unedited = save(&original, &doc, &options());
    assert_eq!(stream(&unedited.bytes), stream(&original));
    assert!(unedited.warnings.is_empty(), "{:?}", unedited.warnings);
    move_first_shape(&mut doc);
    assert_eq!(stream(&save(&original, &doc, &options()).bytes), stream(&original), "and so is it after an edit that makes no part");
}

#[test]
fn a_relationships_part_made_by_a_save_is_typed_by_the_default_and_the_file_s_own_untyped_ones_are_told() {
    // Page 2 gets an instance of the master, so the save makes `page2.xml.rels`; the file had no type for any relationships part.
    let original = no_default_for_rels();
    let (mut doc, _) = read_ok(&original);
    let instance = DocBuilder::new()
        .page(9, PageBuilder::new().shape(ShapeBuilder::new(9).master(1).cell("PinX", "1")))
        .build()
        .pages
        .get(0)
        .unwrap()
        .shapes
        .get(0)
        .unwrap()
        .clone();
    let mut page = (**doc.pages.get(1).unwrap()).clone();
    page.shapes = std::iter::once(instance).collect();
    doc.pages.set(1, Arc::new(page)).unwrap();
    let written = save(&original, &doc, &options());
    assert!(parts(&written.bytes).contains_key("/visio/pages/_rels/page2.xml.rels"), "{:?}", written.added);
    let pkg = Package::read(&written.bytes, &Limits::default()).unwrap();
    for name in rels_parts(&written.bytes) {
        assert_eq!(pkg.content_type(&name), Some(CT_RELS), "{name}");
    }
    let after = rows(&written.bytes);
    assert_eq!(after.0.iter().filter(|(e, _)| e == "rels").count(), 1);
    assert!(after.1.iter().all(|(_, ct)| ct != CT_OCTET), "{:?}", after.1);
    // Only the file's own parts are told: the one the save made is the writer's.
    let n = rels_parts(&original).len();
    let told = written.warnings.iter().find(|w| w.contains("parts had no content type")).unwrap();
    assert!(told.starts_with(&format!("{n} parts had no content type")), "{told}");
}

// ------------------------------------------------------------------------------------------------------------ 4. many new parts

#[test]
fn six_hundred_new_pages_among_four_hundred_name_and_id_their_parts_in_work_linear_in_the_pages() {
    let (had, new) = (400usize, 600usize);
    let original = (1..=had).fold(VsdxBuilder::new(), |b, i| b.page(PageSpec::new(u32::try_from(i).unwrap(), "P"))).build();
    let (mut doc, _) = read_ok(&original);
    assert_eq!(doc.pages.len(), had);
    for _ in 0..new {
        doc.add_page(None).unwrap();
    }
    let types = rows(&original);
    let rows_there = types.0.len() + types.1.len();
    // Counted, not timed: the names tried for the new parts, the rows looked at to find a free `rId`, and the rows of the content types looked at
    // to find the `Override` of a part. A control that makes no part does none of it.
    diacraft_vsdx::work::reset();
    save(&original, &read_ok(&original).0, &options());
    assert_eq!(
        (diacraft_vsdx::work::name_probes(), diacraft_vsdx::work::id_scans(), diacraft_vsdx::work::type_scans()),
        (0, 0, 0),
        "a save that makes no part looks for no name, no id and no type"
    );
    diacraft_vsdx::work::reset();
    let written = save(&original, &doc, &options());
    let (probes, scans, type_rows) = (diacraft_vsdx::work::name_probes(), diacraft_vsdx::work::id_scans(), diacraft_vsdx::work::type_scans());
    eprintln!(
        "[work] {new} pages added to {had}: {probes} names tried, {scans} relationship rows and {type_rows} content type rows looked at ({rows_there} rows there)"
    );
    // One walk up the names: the 400 that are there and the 600 that are free. One collection of the 400 ids in use. One index of the types.
    let (limit, ids_limit, types_limit) =
        (u64::try_from(had + new + 64).unwrap(), u64::try_from(had + 64).unwrap(), u64::try_from(rows_there + 64).unwrap());
    assert!(probes >= u64::try_from(new).unwrap(), "a name is tried for each part: {probes}");
    assert!(probes <= limit && scans <= ids_limit && type_rows <= types_limit, "{probes} names tried, {scans} rows, {type_rows} types");
    // And the result is what a page at a time gave: the names and ids follow the old ones.
    let after = parts(&written.bytes);
    for i in 1..=had + new {
        assert!(after.contains_key(&format!("/visio/pages/page{i}.xml")), "page{i}.xml");
    }
    let ids: std::collections::BTreeSet<&str> = written.document.pages.iter().filter_map(|p| p.rel_id.as_deref()).collect();
    assert_eq!(ids.len(), had + new, "every page has a relationship of its own");
    assert!(ids.contains("rId401") && ids.contains("rId1000") && !ids.contains("rId1001"));
    let pkg = Package::read(&written.bytes, &Limits::default()).unwrap();
    assert!((had + 1..=had + new).all(|i| pkg.content_type(&format!("/visio/pages/page{i}.xml")) == Some(CT_PAGE)));
    assert_eq!(rows(&written.bytes).1.len(), types.1.len() + new, "one Override for each new page");
}

#[test]
fn three_thousand_new_masters_among_five_hundred_name_and_id_their_parts_in_work_linear_in_the_masters() {
    let (had, new) = (500usize, 3_000usize);
    let original =
        (1..=had).fold(VsdxBuilder::new().page(PageSpec::new(1, "A")), |b, i| b.master(MasterSpec::new(u32::try_from(i).unwrap(), "M"))).build();
    let (mut doc, _) = read_ok(&original);
    assert_eq!(doc.masters.len(), had);
    for i in 0..new {
        let id = u32::try_from(had + 1 + i).unwrap();
        doc.add_master(Master { id, name: Some(Arc::from("N")), name_u: Some(Arc::from("N")), ..Master::default() }).unwrap();
    }
    let types = rows(&original);
    let rows_there = types.0.len() + types.1.len();
    diacraft_vsdx::work::reset();
    let written = save(&original, &doc, &options());
    let (probes, scans, type_rows) = (diacraft_vsdx::work::name_probes(), diacraft_vsdx::work::id_scans(), diacraft_vsdx::work::type_scans());
    eprintln!(
        "[work] {new} masters added to {had}: {probes} names tried, {scans} relationship rows and {type_rows} content type rows looked at ({rows_there} rows there)"
    );
    let (limit, ids_limit, types_limit) =
        (u64::try_from(had + new + 64).unwrap(), u64::try_from(had + 64).unwrap(), u64::try_from(rows_there + 64).unwrap());
    assert!(probes >= u64::try_from(new).unwrap(), "a name is tried for each part: {probes}");
    assert!(probes <= limit && scans <= ids_limit && type_rows <= types_limit, "{probes} names tried, {scans} rows, {type_rows} types");
    let after = parts(&written.bytes);
    for i in 1..=had + new {
        assert!(after.contains_key(&format!("/visio/masters/master{i}.xml")), "master{i}.xml");
    }
    let ids: std::collections::BTreeSet<&str> = written.document.masters.iter().filter_map(|m| m.rel_id.as_deref()).collect();
    assert_eq!(ids.len(), had + new, "every master has a relationship of its own");
    assert!(ids.contains("rId501") && ids.contains("rId3500") && !ids.contains("rId3501"));
    assert_eq!(rows(&written.bytes).1.len(), types.1.len() + new, "one Override for each new master");
}

#[test]
fn new_parts_take_the_least_free_numbers_past_names_that_are_in_the_way() {
    // `page2.xml` and `page4.xml` are parts of the file that no relationship reaches (unknown parts, kept as they are): three new pages take the
    // least numbers no part has, as a page at a time did, and the parts in the way are not touched.
    let original = VsdxBuilder::new()
        .page(PageSpec::new(1, "A"))
        .with(Pkg::new().part("/visio/pages/page2.xml", "application/x-other", b"mine").part(
            "/visio/pages/page4.xml",
            "application/x-other",
            b"mine too",
        ))
        .build();
    let (mut doc, _) = read_ok(&original);
    for _ in 0..3 {
        doc.add_page(None).unwrap();
    }
    let written = save(&original, &doc, &options());
    let (before, after) = (parts(&original), parts(&written.bytes));
    let mut added: Vec<&String> = after.keys().filter(|k| k.starts_with("/visio/pages/page") && !before.contains_key(*k)).collect();
    added.sort();
    assert_eq!(added, ["/visio/pages/page3.xml", "/visio/pages/page5.xml", "/visio/pages/page6.xml"]);
    assert_eq!(after.get("/visio/pages/page2.xml"), before.get("/visio/pages/page2.xml"));
    assert_eq!(after.get("/visio/pages/page4.xml"), before.get("/visio/pages/page4.xml"));
    let ids: Vec<&str> = written.document.pages.iter().filter_map(|p| p.rel_id.as_deref()).collect();
    assert_eq!(ids, ["rId1", "rId2", "rId3", "rId4"]);
}

// ------------------------------------------------------------------------------------------------------------ rows missing, at random

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig { cases: 300, failure_persistence: None, ..proptest::prelude::ProptestConfig::default() })]

    /// Any `Default` and `Override` of the stream may be missing, in any layout: the save finds the parts with no type and gives each one, carries
    /// every row the file had, adds nothing for a part that has a type, and writes the file's bytes when no part lacked one.
    #[test]
    fn a_stream_with_rows_missing_is_completed_and_no_row_the_file_had_is_lost(seed in proptest::prelude::any::<u64>()) {
        let mut g = crate::fuzz::Gen(seed | 1);
        let items = items();
        let mut types = rows(&zipped(&items));
        types.0.retain(|_| g.below(3) != 0);
        types.1.retain(|_| g.below(3) != 0);
        let layout = LAYOUTS[g.below(LAYOUTS.len())];
        let original = with_stream(&items, &laid_out(&types, layout));
        let Ok((mut doc, _)) = diacraft_vsdx::read(&original, &diacraft_vsdx::Limits::default()) else { return Ok(()) };
        let before = Package::read(&original, &Limits::default()).unwrap();
        let untyped: Vec<String> = before.parts().filter(|p| before.content_type(p).is_none()).map(str::to_owned).collect();
        for edit in [false, true] {
            if edit {
                doc.add_page(None).unwrap();
            }
            let written = save(&original, &doc, &options());
            let after = Package::read(&written.bytes, &Limits::default()).unwrap();
            let (old, new) = (rows(&original), rows(&written.bytes));
            for row in &old.0 {
                proptest::prop_assert!(new.0.contains(row), "{layout:?}: the Default {row:?} is gone");
            }
            for row in &old.1 {
                proptest::prop_assert!(new.1.contains(row), "{layout:?}: the Override {row:?} is gone");
            }
            for name in after.parts() {
                proptest::prop_assert!(after.content_type(name).is_some(), "{layout:?}: {name} has no type");
                if is_rels_part(name) && untyped.iter().any(|u| u == name) {
                    proptest::prop_assert_eq!(after.content_type(name), Some(CT_RELS), "{:?}: {}", layout, name);
                }
            }
            proptest::prop_assert!(
                !new.1.iter().any(|(part, ct)| ct == CT_OCTET && is_rels_part(part)),
                "{layout:?}: a relationships part is given octet-stream"
            );
            if untyped.is_empty() && !edit {
                proptest::prop_assert_eq!(stream(&written.bytes), stream(&original), "{:?}: every part had a type", layout);
            }
            let added = (new.0.len() + new.1.len()).saturating_sub(old.0.len() + old.1.len());
            // One Default for all the relationships parts, an Override for each of the others, and the new page's own.
            let others = untyped.iter().filter(|u| !is_rels_part(u)).count();
            let room = others + usize::from(untyped.iter().any(|u| is_rels_part(u))) + usize::from(edit) + 1;
            proptest::prop_assert!(added <= room, "{layout:?}: {added} rows added for {} parts with no type", untyped.len());
        }
    }
}

#[test]
fn a_stream_in_utf_16_is_kept_as_it_was_and_when_a_row_changes_it_is_written_in_utf_8_without_losing_one() {
    // R5 lets the stream be UTF-16. Nothing edited: the file's bytes. A page added: the tree is written, in UTF-8 with no declaration (UTF-8 is what
    // a stream with none is), and every row stays.
    let items = items();
    let types = rows(&zipped(&items));
    let text = laid_out(&types, Layout::Packaging).replace("encoding=\"UTF-8\"", "encoding=\"UTF-16\"");
    let mut utf16 = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    let original = {
        let mut items = items.clone();
        items[0].1 = utf16.clone();
        zipped(&items)
    };
    let (mut doc, _) = read_ok(&original);
    assert_eq!(rows(&original), types);
    assert_eq!(Package::read(&save(&original, &doc, &options()).bytes, &Limits::default()).unwrap().content_types_bytes(), utf16.as_slice());
    doc.add_page(Some("Third")).unwrap();
    let written = save(&original, &doc, &options());
    let mut want = types.clone();
    want.1.push(("/visio/pages/page3.xml".to_owned(), CT_PAGE.to_owned()));
    assert_eq!(rows(&written.bytes), want);
}

/// The package of [`items`] with a stream in the layout of a tool that writes single quotes everywhere, CR LF, an indented row to a line and a comment.
fn single_quoted_package() -> (Vec<u8>, String, Rows) {
    let items = items();
    let types = rows(&zipped(&items));
    let single = laid_out(&types, Layout::Tool).replace('"', "'");
    (with_stream(&items, &single), single, types)
}

/// `single` with one more `Override` row, laid out as the others.
fn with_row(single: &str, part: &str, content_type: &str) -> String {
    single.replace("\r\n</Types>\r\n", &format!("\r\n  <Override PartName='{part}' ContentType='{content_type}'/>\r\n</Types>\r\n"))
}

#[test]
fn a_stream_in_single_quotes_keeps_every_row_it_had_byte_for_byte_when_a_row_is_added_taken_out_or_changed() {
    let (original, single, types) = single_quoted_package();
    let (mut doc, _) = read_ok(&original);
    assert_eq!(rows(&original), types);
    assert_eq!(stream(&save(&original, &doc, &options()).bytes), single, "no row changed: the file's bytes, single quotes and all");
    // A row added: the file's bytes and the one row, in the file's quotes and set apart as the others are.
    doc.add_page(Some("Third")).unwrap();
    let added = save(&original, &doc, &options());
    assert_eq!(stream(&added.bytes), with_row(&single, "/visio/pages/page3.xml", CT_PAGE));
    // A row taken out, with the white space before it.
    let (mut doc, _) = read_ok(&original);
    doc.pages.remove(1).unwrap();
    let removed = save(&original, &doc, &options());
    let line = "\r\n  <Override PartName='/visio/pages/page2.xml' ContentType='application/vnd.ms-visio.page+xml'/>";
    assert!(single.contains(line));
    assert_eq!(stream(&removed.bytes), single.replace(line, ""));
    // A row changed: the one row is written anew, in the same quotes and with its attributes in their order.
    let (doc, _) = read_ok(&original);
    let stencil = Variant::from_extension("vssx").unwrap();
    let as_stencil = save(&original, &doc, &WriteOptions { variant: Some(stencil), ..options() });
    assert_eq!(stream(&as_stencil.bytes), single.replace(CT_DRAWING, stencil.content_type()));
    // A new relationships `Default` goes after the last `Default`, in the same quotes.
    let without_default =
        single.replace("\r\n  <Default Extension='rels' ContentType='application/vnd.openxmlformats-package.relationships+xml'/>", "");
    assert_ne!(without_default, single);
    let bare = with_stream(&items(), &without_default);
    let (doc, _) = read_ok(&bare);
    let typed = save(&bare, &doc, &options());
    let want = without_default.replace(
        "<Default Extension='xml' ContentType='application/xml'/>",
        &format!("<Default Extension='xml' ContentType='application/xml'/>\r\n  <Default Extension='rels' ContentType='{CT_RELS}'/>"),
    );
    assert_eq!(stream(&typed.bytes), want);
}

/// `value` as an attribute value between `quote`s.
fn escaped(value: &str, quote: char) -> String {
    let value = value.replace('&', "&amp;").replace('<', "&lt;");
    if quote == '\'' { value.replace('\'', "&apos;") } else { value.replace('"', "&quot;") }
}

#[test]
fn a_new_row_whose_part_name_holds_quotes_and_ampersands_is_written_in_the_files_quotes_and_reads_back() {
    // The pages are in a folder called `it's&co`, so the part name of a new page holds an apostrophe and an ampersand: the row is written between the
    // file's quotes, with what needs escaping there escaped, and reads back as the same name.
    for quote in ['\'', '"'] {
        let mut items = items();
        for (name, bytes) in &mut items {
            if let Some(rest) = name.strip_prefix("visio/pages/") {
                *name = format!("visio/it's&co/{rest}");
            }
            if name == "visio/_rels/document.xml.rels" {
                let text = String::from_utf8(bytes.clone()).unwrap().replace("Target=\"pages/pages.xml\"", "Target=\"it's&amp;co/pages.xml\"");
                *bytes = text.into_bytes();
            }
        }
        let mut types = rows(&zipped(&self::items()));
        for (part, _) in &mut types.1 {
            *part = part.replace("/visio/pages/", "/visio/it's&co/");
        }
        let row = |part: &str, ct: &str| {
            format!("\r\n  <Override PartName={quote}{}{quote} ContentType={quote}{}{quote}/>", escaped(part, quote), escaped(ct, quote))
        };
        let mut text = format!("<Types xmlns={quote}{NS}{quote}>");
        for (extension, ct) in &types.0 {
            text.push_str(&format!("\r\n  <Default Extension={quote}{extension}{quote} ContentType={quote}{}{quote}/>", escaped(ct, quote)));
        }
        for (part, ct) in &types.1 {
            text.push_str(&row(part, ct));
        }
        text.push_str("\r\n</Types>\r\n");
        let original = with_stream(&items, &text);
        assert_eq!(rows(&original), types, "{quote}: the input is what the test says it is");
        let (mut doc, _) = read_ok(&original);
        doc.add_page(Some("Third")).unwrap();
        let written = save(&original, &doc, &options());
        let want = text.replace("\r\n</Types>\r\n", &format!("{}\r\n</Types>\r\n", row("/visio/it's&co/page3.xml", CT_PAGE)));
        assert_eq!(stream(&written.bytes), want, "{quote}");
        assert!(rows(&written.bytes).1.contains(&("/visio/it's&co/page3.xml".to_owned(), CT_PAGE.to_owned())), "{quote}");
    }
}

// ------------------------------------------------------------------------------------------------------------ themes

fn theme_part(name: &str, bytes: &str) -> Arc<diacraft_doc::Theme> {
    Arc::new(diacraft_doc::Theme { part: Arc::from(name), bytes: Arc::from(bytes.as_bytes()), ..diacraft_doc::Theme::default() })
}

const THEME_A: &str = "<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"A\"/>";
const THEME_B: &str = "<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"B\"/>";
const CT_THEME: &str = "application/vnd.openxmlformats-officedocument.theme+xml";

#[test]
fn a_theme_added_to_a_single_quoted_crlf_file_adds_its_row_and_keeps_every_other_rows_bytes() {
    let (original, single, _) = single_quoted_package();
    let (mut doc, _) = read_ok(&original);
    doc.themes.push(theme_part("/visio/theme/theme1.xml", THEME_A));
    let written = save(&original, &doc, &options());
    assert!(parts(&written.bytes).contains_key("/visio/theme/theme1.xml"));
    assert_eq!(stream(&written.bytes), with_row(&single, "/visio/theme/theme1.xml", CT_THEME), "the file's bytes and the theme's row");
    // Many themes added are named in one walk, each with its row.
    let (mut doc, _) = read_ok(&original);
    for i in 0..5 {
        doc.themes.push(theme_part(&format!("/visio/theme/new{i}.xml"), THEME_A));
    }
    let many = save(&original, &doc, &options());
    let after = stream(&many.bytes);
    assert!(after.starts_with(&single[..single.len() - "\r\n</Types>\r\n".len()]), "every row of the file is first, as it was");
    assert_eq!(rows(&many.bytes).1.len(), rows(&original).1.len() + 5);
}

#[test]
fn a_theme_rewritten_leaves_the_stream_as_it_was_and_a_theme_taken_out_takes_its_row_and_the_white_space_before_it() {
    // A file with a theme part of its own, in the single-quoted CR LF layout.
    let items = flat(&VsdxBuilder::new().theme(THEME_A.as_bytes()).page(PageSpec::new(1, "First").shapes(&shape(5, &cell("PinX", "1")))).pkg());
    let types = rows(&zipped(&items));
    assert!(types.1.iter().any(|(p, c)| p == "/visio/theme/theme1.xml" && c == CT_THEME));
    let single = laid_out(&types, Layout::Tool).replace('"', "'");
    let original = with_stream(&items, &single);
    // Rewritten: the bytes of the part change, the stream does not.
    let (mut doc, _) = read_ok(&original);
    doc.themes.set(0, theme_part("/visio/theme/theme1.xml", THEME_B)).unwrap();
    let rewritten = save(&original, &doc, &options());
    assert_eq!(text_of(&parts(&rewritten.bytes), "/visio/theme/theme1.xml"), THEME_B);
    assert_eq!(stream(&rewritten.bytes), single);
    // Taken out: the part, its relationship and its row go, and the rest of the stream is the file's bytes.
    let (mut doc, _) = read_ok(&original);
    doc.themes.remove(0).unwrap();
    let removed = save(&original, &doc, &options());
    assert!(!parts(&removed.bytes).contains_key("/visio/theme/theme1.xml"));
    let line = format!("\r\n  <Override PartName='/visio/theme/theme1.xml' ContentType='{CT_THEME}'/>");
    assert!(single.contains(&line));
    assert_eq!(stream(&removed.bytes), single.replace(&line, ""));
}

#[test]
fn a_stream_whose_children_cannot_be_found_in_its_bytes_is_written_from_the_tree_with_the_files_declaration_and_what_follows_the_root() {
    // A root written empty has no content to find, so a row added makes it a root with rows (a stream like that holds no part's type), written from
    // the tree: but the declaration, its line end and what follows the root are the file's own bytes, not a declaration of the writer's.
    let items = items();
    let head = "<?xml version='1.0' encoding='UTF-8'?>\r\n";
    let original = with_stream(&items, &format!("{head}<Types xmlns='{NS}'/>\r\n"));
    let (mut doc, _) = read_ok(&original);
    doc.add_page(Some("Third")).unwrap();
    let written = save(&original, &doc, &options());
    let after = rows(&written.bytes);
    assert!(after.1.iter().any(|(p, c)| p == "/visio/pages/page3.xml" && c == CT_PAGE), "{after:?}");
    let pkg = Package::read(&written.bytes, &Limits::default()).unwrap();
    assert!(pkg.parts().all(|p| pkg.content_type(p).is_some()), "every part has a type");
    let text = stream(&written.bytes);
    assert!(text.starts_with(&format!("{head}<Types xmlns=\"{NS}\">")), "{text}");
    assert!(text.ends_with("</Types>\r\n") && !text.contains("standalone"), "{text}");
}

#[test]
fn the_reader_finds_a_property_part_by_its_content_type_as_the_writer_does() {
    // The only core-properties relationship names the app part: the reader reads it as the app part (the application is its own) and finds no core
    // part, and does not say the part it took for core is unreadable (it used to).
    let only = with_package_rels(&app_xml("Another Drawing App"), |t| {
        let no_core = t.replace(&format!("<Relationship Id=\"rId2\" Type=\"{PK}core-properties\" Target=\"docProps/core.xml\"/>"), "");
        no_core.replace(&format!("{OD}extended-properties"), &format!("{PK}core-properties"))
    });
    let (doc, report) = read_ok(&only);
    assert_eq!(report.application.as_deref(), Some("Another Drawing App"));
    assert_eq!(report.warning_count("properties_unreadable"), 0, "{:?}", report.warnings);
    assert_eq!(&*doc.title, "", "no core part is reached, so no title");
    // The ordinary file reads as it did: both parts by their own relationships.
    let ordinary = with_package_rels(&app_xml("Another Drawing App"), |t| t);
    let (doc, report) = read_ok(&ordinary);
    assert_eq!((report.application.as_deref(), &*doc.title), (Some("Another Drawing App"), "Test drawing"));
    // A part with no content type of its own (typed by the `Default` of `xml`) is still read through the relationship of its kind.
    let items = items();
    let mut types = rows(&zipped(&items));
    types.1.retain(|(part, _)| part != "/docProps/app.xml" && part != "/docProps/core.xml");
    let untyped = with_stream(&items, &laid_out(&types, Layout::Packaging));
    let (doc, report) = read_ok(&untyped);
    assert_eq!(&*doc.title, "Test drawing");
    assert!(report.application.is_some());
}

// ------------------------------------------------------------------------------------------------------------: crafted shapes

/// `n` characters of `alphabet` in a fixed pseudo-random order, so that a gap of them does not compress to nothing.
fn blanks(n: usize, alphabet: &[char]) -> String {
    let mut x = 0x2545_F491_4F6C_DD1Du64;
    (0..n)
        .map(|_| {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            alphabet[(x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as usize % alphabet.len()]
        })
        .collect()
}

/// `items` and `count` parts with a name nobody types (`junk/p0.zzz` on), so that a plain save gives each an `Override`.
fn with_untyped_parts(mut items: Vec<(String, Vec<u8>)>, count: usize) -> Vec<(String, Vec<u8>)> {
    for i in 0..count {
        items.push((format!("junk/p{i}.zzz"), b"x".to_vec()));
    }
    items
}

#[test]
fn a_long_gap_before_the_last_row_is_cut_to_a_line_end_or_nothing_and_not_repeated_for_each_row_added() {
    let items = items();
    let plain = laid_out(&rows(&zipped(&items)), Layout::Packaging);
    let at = plain.rfind("<Override").unwrap();
    let n = 300;
    // 1 MB of spaces and tabs; the same with a CR LF in it; the same with an LF.
    for (gap, cut) in [
        (blanks(1_000_000, &[' ', '\t']), ""),
        (format!("{}\r\n{}", blanks(500_000, &[' ', '\t']), blanks(500_000, &[' ', '\t'])), "\r\n"),
        (format!("{}\n{}", blanks(500_000, &[' ', '\t']), blanks(500_000, &[' ', '\t'])), "\n"),
    ] {
        let spaced = format!("{}{gap}{}", &plain[..at], &plain[at..]);
        let original = with_stream(&with_untyped_parts(items.clone(), n), &spaced);
        let (doc, _) = read_ok(&original);
        let written = save(&original, &doc, &options());
        let out = stream(&written.bytes);
        let name = cut.escape_debug().to_string();
        assert!(out.len() < spaced.len() + n * 200, "{name}: {} bytes from {}", out.len(), spaced.len());
        let kept = &spaced[..spaced.len() - "</Types>".len()];
        assert!(out.starts_with(kept), "{name}: the file's bytes first");
        assert_eq!(rows(&written.bytes).1.len(), rows(&original).1.len() + n, "{name}");
        // Each new row is set apart from the one before it by the cut gap, and the first of them is too.
        let row = "<Override PartName=\"/junk/p";
        assert_eq!(out.matches(&format!("{cut}{row}")).count(), n, "{name}");
        if cut.is_empty() {
            assert_eq!(out[kept.len()..].matches("/><Override").count(), n - 1, "{name}: the new rows follow one another");
        }
    }
}

/// **This test is a guard for the code as it is and would have passed on `1a4f644`**: that version searched for the last row and inserted past the
/// comments for each new row, and counted none of it, so no count could tell it from this one (the evidence for the old cost is the CLI timings of the
/// review: 1.6 s, 5.3 s and 29.5 s for 300, 3,000 and 20,000 rows). What it holds is that the anchor's walk back and the index are counted and are one
/// pass each: a change that searched again for each row would show here as 3,000 times the comments.
#[test]
fn many_comments_after_the_last_row_cost_one_pass_and_not_a_pass_for_each_row_added() {
    let items = items();
    let plain = laid_out(&rows(&zipped(&items)), Layout::Packaging);
    let (comments, n) = (100_000usize, 3_000usize);
    let close = plain.rfind("</Types>").unwrap();
    let trail = "<!---->".repeat(comments);
    let crafted = format!("{}{trail}{}", &plain[..close], &plain[close..]);
    let original = with_stream(&with_untyped_parts(items, n), &crafted);
    let (doc, _) = read_ok(&original);
    // Counted, not timed: the children of the stream looked at to find where the new rows go and to index the rows, which is a pass each, and not a
    // pass past the comments for each of the rows (a hundred thousand children for each of 3,000 rows).
    diacraft_vsdx::work::reset();
    let written = save(&original, &doc, &options());
    let scans = diacraft_vsdx::work::type_scans();
    eprintln!("[work] {n} rows added before {comments} comments: {scans} children looked at");
    let children = u64::try_from(comments + 64).unwrap();
    assert!(scans >= children - 64, "the stream was looked at: {scans}");
    assert!(scans <= 3 * children, "{scans} children looked at for {n} rows after {comments} comments");
    // The rows are in place: after the last row, before the comments, which are the file's.
    let out = stream(&written.bytes);
    assert!(out.starts_with(&plain[..close]) && out.ends_with(&format!("{trail}</Types>")), "the file's bytes around the new rows");
    assert_eq!(rows(&written.bytes).1.len(), rows(&original).1.len() + n);
}

#[test]
fn a_default_for_rels_with_no_content_type_is_not_a_default_and_the_save_adds_one() {
    // `<Default Extension="rels"/>` has no type to give: the reader skips it (and says so), so the relationships parts have none, and the save must
    // not take the row for a `Default` of `rels` (it used to, and failed with "the part has no content type").
    let items = items();
    let plain = laid_out(&rows(&zipped(&items)), Layout::Packaging);
    let rels_row = format!("<Default Extension=\"rels\" ContentType=\"{CT_RELS}\"/>");
    assert!(plain.contains(&rels_row));
    let holed = plain.replace(&rels_row, "<Default Extension=\"rels\"/>");
    let original = with_stream(&items, &holed);
    let (doc, _) = read_ok(&original);
    let before = Package::read(&original, &Limits::default()).unwrap();
    assert!(rels_parts(&original).iter().all(|n| before.content_type(n).is_none()), "the relationships parts have no type to begin with");
    let written = save(&original, &doc, &options());
    // The new `Default` follows the last `Default` and the file's row stays where it was.
    let want = holed.replace(
        "<Default Extension=\"xml\" ContentType=\"application/xml\"/>",
        &format!("<Default Extension=\"xml\" ContentType=\"application/xml\"/>{rels_row}"),
    );
    assert_ne!(want, holed);
    assert_eq!(stream(&written.bytes), want);
    let pkg = Package::read(&written.bytes, &Limits::default()).unwrap();
    for name in rels_parts(&written.bytes) {
        assert_eq!(pkg.content_type(&name), Some(CT_RELS), "{name}");
    }
    assert!(written.warnings.iter().any(|w| w.contains("parts had no content type")), "{:?}", written.warnings);
}

#[test]
fn an_override_whose_part_name_is_percent_encoded_is_found_when_its_part_is_taken_out_and_when_its_type_is_changed() {
    // The reader of the stream decodes the names of the rows (`pag%C3%A91.xml` is `pagé1.xml`); the writer matched them as written, so a page taken
    // out left its row, and a Save As added a second row for the document part and the first one, the old type, won.
    let encoded = |from: &str, to: &str, items: &mut Vec<(String, Vec<u8>)>, types: &mut Rows| {
        for (name, bytes) in items.iter_mut() {
            if name == from.trim_start_matches('/') {
                *name = to.trim_start_matches('/').to_owned();
            } else if name.ends_with(".rels") {
                let text = String::from_utf8(bytes.clone()).unwrap();
                let (a, b) = (from.trim_start_matches('/').rsplit('/').next().unwrap(), to.trim_start_matches('/').rsplit('/').next().unwrap());
                *bytes = text
                    .replace(&format!("\"{a}\""), &format!("\"{b}\""))
                    .replace(&format!("\"{}\"", from.trim_start_matches('/')), &format!("\"{}\"", to.trim_start_matches('/')))
                    .into_bytes();
            }
        }
        for (part, _) in &mut types.1 {
            if part == from {
                *part = to.to_owned();
            }
        }
    };
    // A page taken out.
    let (mut renamed, mut types) = (items(), rows(&zipped(&items())));
    encoded("/visio/pages/page1.xml", "/visio/pages/pag%C3%A91.xml", &mut renamed, &mut types);
    let original = with_stream(&renamed, &laid_out(&types, Layout::Packaging));
    assert!(Package::read(&original, &Limits::default()).unwrap().part("/visio/pages/pag\u{e9}1.xml").is_some());
    let (mut doc, _) = read_ok(&original);
    doc.pages.remove(0).unwrap();
    let written = save(&original, &doc, &options());
    assert!(Package::read(&written.bytes, &Limits::default()).unwrap().part("/visio/pages/pag\u{e9}1.xml").is_none());
    assert!(!stream(&written.bytes).contains("pag%C3%A91"), "the row of the part taken out is gone: {}", stream(&written.bytes));
    assert_eq!(rows(&written.bytes).1.len(), types.1.len() - 1);
    // The document part's type changed by a Save As.
    let (mut renamed, mut types) = (items(), rows(&zipped(&items())));
    encoded("/visio/document.xml", "/visio/d%C3%B4c.xml", &mut renamed, &mut types);
    for (name, bytes) in &mut renamed {
        if name == "visio/_rels/document.xml.rels" {
            *name = "visio/_rels/d%C3%B4c.xml.rels".to_owned();
        }
        if name == "_rels/.rels" {
            *bytes = String::from_utf8(bytes.clone()).unwrap().replace("visio/document.xml", "visio/d%C3%B4c.xml").into_bytes();
        }
    }
    let original = with_stream(&renamed, &laid_out(&types, Layout::Packaging));
    let (doc, report) = read_ok(&original);
    assert_eq!(report.variant, Some(Variant::from_extension("vsdx").unwrap()), "the document part is found and typed");
    let stencil = Variant::from_extension("vssx").unwrap();
    let written = save(&original, &doc, &WriteOptions { variant: Some(stencil), ..options() });
    assert_eq!(rows(&written.bytes).1.len(), types.1.len(), "no second row for the document part: {}", stream(&written.bytes));
    assert_eq!(Package::read(&written.bytes, &Limits::default()).unwrap().content_type("/visio/d\u{f4}c.xml"), Some(stencil.content_type()));
}

#[test]
fn a_thousand_pages_with_pictures_mostly_taken_out_cost_a_pass_over_each_list_and_not_a_pass_for_each_page() {
    let (n, keep) = (1000usize, 200usize);
    let mut builder = VsdxBuilder::new();
    let mut pictures = Pkg::new();
    for i in 1..=n {
        builder = builder.page(PageSpec::new(u32::try_from(i).unwrap(), "P").rel("rId9", &format!("{OD}image"), &format!("../media/image{i}.png")));
        pictures = pictures.part(&format!("/visio/media/image{i}.png"), "image/png", b"png");
    }
    let original = builder.with(pictures).build();
    let (mut doc, _) = read_ok(&original);
    for _ in keep..n {
        doc.pages.remove(keep).unwrap();
    }
    let types = rows(&original);
    let rows_there = types.0.len() + types.1.len();
    diacraft_vsdx::work::reset();
    let written = save(&original, &doc, &options());
    let (rels, parts_named, overrides) =
        (diacraft_vsdx::work::rels_visited(), diacraft_vsdx::work::parts_visited(), diacraft_vsdx::work::overrides_visited());
    eprintln!(
        "[work] {} pages with pictures taken out of {n}: {rels} relationship rows, {parts_named} parts and {overrides} content type rows visited",
        n - keep
    );
    // The index's relationships in one pass; the pages in one pass of the stream and the pictures in a second; each part named once.
    let (gone, limit) = (n - keep, u64::try_from(n + 64).unwrap());
    assert!(rels <= limit, "{rels} relationship rows visited");
    assert!(parts_named <= u64::try_from(2 * gone + 64).unwrap(), "{parts_named} parts named");
    assert!(overrides <= u64::try_from(2 * rows_there + 64).unwrap(), "{overrides} content type rows visited for {rows_there} there");
    // And the result is the same as taking them out one by one: the pages and their pictures go, the others stay.
    let after = parts(&written.bytes);
    assert!(
        !after.contains_key(&format!("/visio/pages/page{}.xml", keep + 1)) && !after.contains_key(&format!("/visio/media/image{}.png", keep + 1))
    );
    assert!(after.contains_key(&format!("/visio/pages/page{keep}.xml")) && after.contains_key(&format!("/visio/media/image{keep}.png")));
    assert_eq!(after.keys().filter(|k| k.starts_with("/visio/media/")).count(), keep);
    assert_eq!(rows(&written.bytes).1.len(), types.1.len() - 2 * gone, "a row less for each page and each picture");
}

#[test]
fn a_property_relationship_used_twice_is_warned_of_with_the_rule_that_chooses() {
    // Two core-properties relationships (the odd file's): the reader warns, and says which is used, which is not always the first.
    let original = app_under_a_core_relationship("Another Drawing App");
    let (_, report) = read_ok(&original);
    let warning = report.warnings.iter().find(|w| w.contains("more than one relationship")).unwrap_or_else(|| panic!("{:?}", report.warnings));
    assert!(warning.contains("property part") && warning.contains("content type"), "{warning}");
    assert!(!warning.ends_with("the first is used"), "{warning}");
}

#[test]
fn a_stream_that_the_files_bytes_could_not_hold_is_written_as_its_rows_alone_and_the_save_says_the_comments_went() {
    // 450,000 comments (3.15 MB) after the last row, and 20,000 parts that need a row each (1.5 MB more): the file's bytes and the rows would pass the
    // 4 MiB the reader takes for the stream, so the stream is written as its rows (the writer before a change wrote every stream so, and saved this
    // file): the declaration and root tag of the file, the rows in order, no comment.
    let items = items();
    let types = rows(&zipped(&items));
    let plain = laid_out(&types, Layout::Packaging);
    let close = plain.rfind("</Types>").unwrap();
    let crafted = format!("{}{}{}", &plain[..close], "<!---->".repeat(450_000), &plain[close..]);
    let n = 20_000;
    let original = with_stream(&with_untyped_parts(items, n), &crafted);
    let (doc, _) = read_ok(&original);
    let written = save(&original, &doc, &options());
    let mut want = types;
    want.1.extend((0..n).map(|i| (format!("/junk/p{i}.zzz"), CT_OCTET.to_owned())));
    let out = stream(&written.bytes);
    assert_eq!(out, laid_out(&want, Layout::Packaging), "the file's declaration and root, then its rows and the new ones, and nothing between them");
    assert!(out.len() < 4 * 1024 * 1024 && !out.contains("<!--"));
    assert!(written.warnings.iter().any(|w| w.contains("comments") && w.contains("white space")), "{:?}", written.warnings);
    // The same file with the room the comments took: the file's bytes, no warning.
    let roomy = with_stream(&with_untyped_parts(self::items(), n), &format!("{}{}{}", &plain[..close], "<!---->".repeat(1000), &plain[close..]));
    let (doc, _) = read_ok(&roomy);
    let kept = save(&roomy, &doc, &options());
    assert!(stream(&kept.bytes).contains("<!---->") && kept.warnings.iter().all(|w| !w.contains("comments")), "{:?}", kept.warnings);
}

#[test]
fn rows_that_would_pass_the_size_a_package_allows_for_the_stream_even_alone_are_a_limit_and_not_a_part_that_cannot_be_read_back() {
    // 20,000 parts with names of 250 characters need 6 MB of rows by themselves.
    let mut items = items();
    for i in 0..20_000 {
        items.push((format!("junk/{i:05}{}.zzz", "n".repeat(250)), b"x".to_vec()));
    }
    let original = with_stream(&items, &laid_out(&rows(&zipped(&self::items())), Layout::Packaging));
    let (doc, _) = read_ok(&original);
    let error = write_with(&doc, Some(&OriginalBytes { bytes: &original, baseline: None }), &options()).map(|w| w.bytes.len()).unwrap_err();
    assert_eq!(error.class(), "write_limit", "{error}");
    // The refusal is the one for the rows alone, after the file's bytes and the rows alone were both tried: the message says which.
    assert!(error.to_string().contains("rows of the content types alone"), "{error}");
}

#[test]
fn forty_five_thousand_short_named_parts_in_a_compact_stream_are_saved_with_the_files_bytes_and_their_rows() {
    // The rows of 45,000 parts come to 3.5 MB, under the 4 MiB the stream may have, and the file's own bytes are small: the writer before a change saved
    // this file, and an estimate of what the rows add that ran high refused it. The save is the file's bytes and the rows, with no comment lost.
    let items = items();
    let types = rows(&zipped(&items));
    let plain = laid_out(&types, Layout::Packaging);
    let n = 45_000;
    let original = with_stream(&with_untyped_parts(items, n), &plain);
    let (doc, _) = read_ok(&original);
    let written = save(&original, &doc, &options());
    let mut want = types;
    want.1.extend((0..n).map(|i| (format!("/junk/p{i}.zzz"), CT_OCTET.to_owned())));
    let out = stream(&written.bytes);
    assert_eq!(out, laid_out(&want, Layout::Packaging));
    assert!(out.len() > 3_000_000 && out.len() < 4 * 1024 * 1024, "{} bytes", out.len());
    assert!(written.warnings.iter().all(|w| !w.contains("comments")), "{:?}", written.warnings);
}

#[test]
fn comments_before_the_root_or_after_it_go_with_the_rows_alone_form_and_the_save_is_made() {
    // 400,000 comments (2.8 MB) before `<Types>` or after `</Types>`, and 20,000 parts that need a row each: the file's bytes and the rows would pass 4 MiB.
    // The rows alone keep the byte order mark, the declaration and the line end after it, the root's tag and the rows; nothing before or after.
    let n = 20_000;
    for (layout, line_end, before) in
        [(Layout::Packaging, "\r\n", true), (Layout::Packaging, "", false), (Layout::Marked, "", true), (Layout::Marked, "", false)]
    {
        let items = items();
        let types = rows(&zipped(&items));
        let with_line_end = |text: String| if line_end.is_empty() { text } else { text.replacen("?>", &format!("?>{line_end}"), 1) };
        let head = with_line_end(laid_out(&types, layout));
        let root = head.find("<Types").unwrap();
        let comments = "<!---->".repeat(400_000);
        let crafted = if before { format!("{}{comments}{}", &head[..root], &head[root..]) } else { format!("{head}{comments}") };
        let original = with_stream(&with_untyped_parts(items, n), &crafted);
        let (doc, _) = read_ok(&original);
        let written = save(&original, &doc, &options());
        let mut want = types;
        want.1.extend((0..n).map(|i| (format!("/junk/p{i}.zzz"), CT_OCTET.to_owned())));
        let name = format!("{layout:?}, {line_end:?}, comments {}", if before { "before the root" } else { "after the root" });
        assert_eq!(stream(&written.bytes), with_line_end(laid_out(&want, layout)), "{name}");
        assert!(written.warnings.iter().any(|w| w.contains("comments") && w.contains("white space")), "{name}: {:?}", written.warnings);
    }
}
