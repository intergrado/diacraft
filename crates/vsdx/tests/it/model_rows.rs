//! **What a model save does with names and rows** when the model has been edited (items 1, 2, 3 and 6 of the review). `write_with(doc, None, ..)` writes the package from the model alone,
//! so a page that is added after another was taken out, or put before a page that has rows, must be given nothing of the other's, and a page that is kept must be written under the name it had.
//!
//! * **Names** (item 1, and the pictures of item 6): every name a part of the model goes by is reserved before the writer names a part of its own, and a part's rows are the rows of its own name. A
//!   page or master that must be renamed carries its rows to the new name, their targets taken to where they lead.
//! * **The rows between parts** (item 2): the rows of a page to the masters its shapes use and to its background page are made from the model, as the splice writer makes them (R22 of the masters
//!   spec, R6 of the pages spec).
//! * **DiaCraft's data next to another program's solutions** (item 3, and the reader's part of it): the data of the model is written where the file has its part, and the model handed back says what
//!   the file now says.
//!
//! Every input is built in code. The tests here fail on the writer of 59838e66.

use std::collections::BTreeSet;
use std::sync::Arc;

use diacraft_doc::{Document, Shape};
use diacraft_testkit::package::{CT_SOLUTION, CT_SOLUTIONS, MasterSpec, NS_2012, NS_R, OD, PageSpec, Pkg, VIS, VsdxBuilder, cell, shape, shape_with};
use diacraft_vsdx::{OriginalBytes, WriteOptions, write_with};

use crate::model_writer::{CT_OLE, DOC, model_write, new_picture_shape, package, parts, picture, text_of};
use crate::read_ok;

// ----------------------------------------------------------------------------------------------------------------------------- helpers

/// The rows from `source` in the package, as `(Id, type word after the last slash, the part it leads to)`.
fn rows_of(bytes: &[u8], source: &str) -> Vec<(String, String, String)> {
    package(bytes)
        .rels(source)
        .unwrap()
        .items
        .iter()
        .map(|r| (r.id.clone(), r.ty.rsplit('/').next().unwrap_or("").to_owned(), r.target.clone().unwrap_or_else(|| r.target_raw.clone())))
        .collect()
}

/// The part of the page called `name`, as the file written names it.
fn page_part(bytes: &[u8], name: &str) -> String {
    let (doc, _) = read_ok(bytes);
    let page = doc.pages.iter().find(|p| &*p.name == name).unwrap_or_else(|| panic!("no page {name}"));
    page.part_name.as_deref().unwrap_or_else(|| panic!("page {name} has no part")).to_owned()
}

fn master_part(bytes: &[u8], name: &str) -> String {
    let (doc, _) = read_ok(bytes);
    let master = doc.masters.iter().find(|m| m.name.as_deref() == Some(name)).unwrap_or_else(|| panic!("no master {name}"));
    master.part_name.as_deref().unwrap_or_else(|| panic!("master {name} has no part")).to_owned()
}

fn show(rows: &[(String, String, String)]) -> String {
    rows.iter().map(|(id, ty, to)| format!("{id} {ty} -> {to}")).collect::<Vec<_>>().join("; ")
}

/// A drawing of two pages whose first holds an embedded object and a row to a master, beside a master.
fn ole_page_package() -> Vec<u8> {
    let foreign = r#"<Shape ID="1" Type="Foreign"><ForeignData ForeignType="Object" ObjectType="2"><Rel r:id="rId2"/></ForeignData></Shape>"#;
    let first = PageSpec::new(1, "First").shapes(foreign).rel("rId2", &format!("{OD}oleObject"), "../embeddings/oleObject1.bin").rel(
        "rId7",
        &format!("{VIS}master"),
        "../masters/master1.xml",
    );
    let second = PageSpec::new(2, "Second").shapes(&shape(1, &cell("PinX", "1")));
    let extra = Pkg::new().part("/visio/embeddings/oleObject1.bin", CT_OLE, [1u8]);
    VsdxBuilder::new().master(MasterSpec::new(1, "Box").shapes(&shape(4, &cell("Width", "1")))).page(first).page(second).with(extra).build()
}

/// The page that `add_page` made, moved to the front.
fn add_page_at_the_front(doc: &mut Document, name: &str) {
    doc.add_page(Some(name)).unwrap();
    let last = doc.pages.len() - 1;
    let page = doc.pages.remove(last).unwrap();
    doc.pages.insert(0, page).unwrap();
}

fn with_shape(doc: &mut Document, page_at: usize, shape: Shape) {
    let page = Arc::make_mut(doc.pages.get_mut(page_at).unwrap());
    page.shapes.push(Arc::new(shape));
}

// ----------------------------------------------------------------------------------------------- item 1: the rows go with the name

#[test]
fn a_page_added_after_the_first_was_taken_out_takes_nothing_of_it() {
    let (mut doc, _) = read_ok(&ole_page_package());
    doc.pages.remove(0).unwrap();
    doc.add_page(Some("Added")).unwrap();
    let written = model_write(&doc, false);
    let added = page_part(&written.bytes, "Added");
    let rows = rows_of(&written.bytes, &added);
    assert!(rows.is_empty(), "the new page holds rows of the page taken out: {}", show(&rows));
    let p = parts(&written.bytes);
    assert!(!p.contains_key("/visio/embeddings/oleObject1.bin"), "the embedded object only the page taken out reached: {:?}", p.keys());
    // The page that stays is as it was.
    assert_eq!(page_part(&written.bytes, "Second"), "/visio/pages/page2.xml");
}

#[test]
fn a_page_added_at_the_front_leaves_the_page_that_was_first_its_part_and_its_rows() {
    let (mut doc, _) = read_ok(&ole_page_package());
    add_page_at_the_front(&mut doc, "Added");
    let written = model_write(&doc, false);
    let first = page_part(&written.bytes, "First");
    assert_eq!(first, "/visio/pages/page1.xml", "a page is written under the name it had");
    let rows = rows_of(&written.bytes, &first);
    let ole = rows.iter().find(|r| r.0 == "rId2").unwrap_or_else(|| panic!("the object the shape names has no row: {}", show(&rows)));
    assert!(parts(&written.bytes).contains_key(&ole.2), "the row of the object leads to a part: {}", show(&rows));
    let added = page_part(&written.bytes, "Added");
    assert_ne!(added, first);
    assert!(rows_of(&written.bytes, &added).is_empty(), "{}", show(&rows_of(&written.bytes, &added)));
}

#[test]
fn a_page_added_before_a_background_page_leaves_the_row_to_it_where_it_leads() {
    let pkg = VsdxBuilder::new()
        // (The background page has content: a page that is empty and one that is added are one part, and that is no case.)
        .page(PageSpec::new(1, "Back").attrs(r#" Background="1""#).shapes(&shape(1, &cell("PinX", "1"))))
        .page(PageSpec::new(2, "Front").attrs(r#" BackPage="1""#).rel("rId8", &format!("{VIS}page"), "page1.xml"))
        .build();
    let (mut doc, _) = read_ok(&pkg);
    add_page_at_the_front(&mut doc, "Added");
    let written = model_write(&doc, false);
    let (back, front, added) = (page_part(&written.bytes, "Back"), page_part(&written.bytes, "Front"), page_part(&written.bytes, "Added"));
    let rows = rows_of(&written.bytes, &front);
    let pages: Vec<&(String, String, String)> = rows.iter().filter(|r| r.1 == "page").collect();
    assert_eq!(pages.len(), 1, "{}", show(&rows));
    assert_eq!(pages[0].2, back, "the background page's row leads to the background page, not to the page added: {}", show(&rows));
    assert!(rows_of(&written.bytes, &added).is_empty());
}

#[test]
fn a_master_brought_in_after_another_was_taken_out_takes_nothing_of_it() {
    let pkg = VsdxBuilder::new()
        .page(PageSpec::new(1, "P"))
        .master(MasterSpec::new(1, "Old").shapes(&shape(4, &cell("Width", "1"))).rel("rId1", &format!("{OD}image"), "../media/old.png"))
        .master(MasterSpec::new(2, "Keep").shapes(&shape(5, &cell("Width", "2"))))
        .with(Pkg::new().part("/visio/media/old.png", "image/png", [0x89u8, b'P', b'N', b'G', 1]))
        .build();
    let (mut doc, _) = read_ok(&pkg);
    let mut brought = (**doc.masters.get(1).unwrap()).clone();
    doc.masters.remove(0).unwrap();
    brought.id = 9;
    brought.name = Some(Arc::from("New"));
    brought.name_u = Some(Arc::from("New"));
    brought.rel_id = None;
    brought.part_name = None;
    brought.unique_id = None;
    doc.add_master(brought).unwrap();
    let written = model_write(&doc, false);
    let new = master_part(&written.bytes, "New");
    assert!(rows_of(&written.bytes, &new).is_empty(), "{}", show(&rows_of(&written.bytes, &new)));
    assert!(!parts(&written.bytes).contains_key("/visio/media/old.png"), "the picture only the master taken out reached");
    assert_eq!(master_part(&written.bytes, "Keep"), "/visio/masters/master2.xml");
}

#[test]
fn a_held_row_to_a_part_that_is_not_there_is_not_taken_over_by_a_page_that_is_added() {
    let extra = Pkg::new().rel(DOC, "rId77", "urn:example:thing", "pages/page2.xml");
    let pkg = VsdxBuilder::new().page(PageSpec::new(1, "P")).with(extra).build();
    let (mut doc, _) = read_ok(&pkg);
    doc.add_page(Some("Added")).unwrap();
    let written = model_write(&doc, false);
    let row = package(&written.bytes).rels(DOC).unwrap().get("rId77").map(|r| r.target_raw.clone());
    assert_eq!(row.as_deref(), Some("pages/page2.xml"), "the row is as it was");
    assert!(!parts(&written.bytes).contains_key("/visio/pages/page2.xml"), "the page added took the name a held row leads to");
    assert_ne!(page_part(&written.bytes, "Added"), "/visio/pages/page2.xml");
}

#[test]
fn a_page_that_must_be_renamed_carries_its_rows_to_where_they_lead() {
    // Two pages name one part, which is two folders down; an edit to the second makes it a part of its own, and the part is named in the pages' folder: the row of the object says
    // `../../embeddings/o.bin` from where the part was, and must say what leads to the same object from where it is.
    let shared = format!("<PageContents xmlns=\"{NS_2012}\"><Shapes><Shape ID=\"1\"/></Shapes></PageContents>");
    let pages = format!(
        "<Pages xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Page ID=\"1\" Name=\"A\"><PageSheet/><Rel r:id=\"rId1\"/></Page><Page ID=\"2\" Name=\"B\"><PageSheet/><Rel r:id=\"rId2\"/></Page></Pages>"
    );
    let pkg = Pkg::new()
        .part("/visio/pages/pages.xml", diacraft_testkit::package::CT_PAGES, pages)
        .rel(DOC, "rId50", &format!("{VIS}pages"), "pages/pages.xml")
        .part("/visio/pages/deep/one.xml", diacraft_testkit::package::CT_PAGE, shared)
        .rel("/visio/pages/pages.xml", "rId1", &format!("{VIS}page"), "deep/one.xml")
        .rel("/visio/pages/pages.xml", "rId2", &format!("{VIS}page"), "deep/one.xml")
        .rel("/visio/pages/deep/one.xml", "rId1", &format!("{OD}oleObject"), "../../embeddings/o.bin")
        .part("/visio/embeddings/o.bin", CT_OLE, [3u8]);
    let bytes = VsdxBuilder::new().without_pages_part().with(pkg).build();
    let (mut doc, _) = read_ok(&bytes);
    crate::model_writer::page_cell(&mut doc, 2, 0, "PinX", "5");
    let written = model_write(&doc, false);
    let (a, b) = (page_part(&written.bytes, "A"), page_part(&written.bytes, "B"));
    assert_ne!(a, b, "the pages have parts of their own once one of them differs");
    for part in [&a, &b] {
        let rows = rows_of(&written.bytes, part);
        let ole = rows.iter().find(|r| r.0 == "rId1").unwrap_or_else(|| panic!("{part} lost the row of the object: {}", show(&rows)));
        assert_eq!(ole.2, "/visio/embeddings/o.bin", "{part}: {}", show(&rows));
    }
    assert!(parts(&written.bytes).contains_key("/visio/embeddings/o.bin"));
}

// ------------------------------------------------------------------------------------------------------ item 6: a picture keeps its name

#[test]
fn a_new_picture_on_an_earlier_page_does_not_take_the_name_of_a_picture_a_later_page_has() {
    let foreign = r#"<Shape ID="1" Type="Foreign"><ForeignData ForeignType="Bitmap"><Rel r:id="rId1"/></ForeignData></Shape>"#;
    let png = [0x89u8, b'P', b'N', b'G', 1, 2, 3, 4];
    let pkg = VsdxBuilder::new()
        .page(PageSpec::new(1, "A"))
        .page(PageSpec::new(2, "B").shapes(foreign).rel("rId1", &format!("{OD}image"), "../media/image1.png"))
        .with(Pkg::new().part("/visio/media/image1.png", "image/png", png))
        .build();
    let (mut doc, _) = read_ok(&pkg);
    let new = picture("not a part name of this format", &[0x89, b'P', b'N', b'G', 9, 9, 9, 9]);
    with_shape(&mut doc, 0, new_picture_shape(1, &new));
    let written = model_write(&doc, false);
    let p = parts(&written.bytes);
    assert_eq!(
        p.get("/visio/media/image1.png").map(Vec::as_slice),
        Some(&png[..]),
        "the picture that was there is under the name it had: {:?}",
        p.keys()
    );
    let rows = rows_of(&written.bytes, &page_part(&written.bytes, "B"));
    assert_eq!(rows.iter().find(|r| r.0 == "rId1").map(|r| r.2.as_str()), Some("/visio/media/image1.png"));
    let images: BTreeSet<&String> = p.keys().filter(|k| k.starts_with("/visio/media/")).collect();
    assert_eq!(images.len(), 2, "{images:?}");
}

// -------------------------------------------------------------------------------------------- item 2: the rows between parts are rebuilt

fn page_with_master_package(shapes: &str, rows: &[(&str, &str, &str)]) -> Vec<u8> {
    let mut page = PageSpec::new(1, "P").shapes(shapes);
    for (id, ty, target) in rows {
        page = page.rel(id, ty, target);
    }
    VsdxBuilder::new().master(MasterSpec::new(1, "Box").shapes(&shape(4, &cell("Width", "1")))).page(page).build()
}

#[test]
fn the_first_instance_of_a_master_on_a_page_gives_the_page_its_row() {
    let (mut doc, _) = read_ok(&page_with_master_package(&shape(1, &cell("PinX", "1")), &[]));
    with_shape(&mut doc, 0, Shape { id: 9, master: Some(1), ..Shape::default() });
    let written = model_write(&doc, false);
    let rows = rows_of(&written.bytes, "/visio/pages/page1.xml");
    let masters: Vec<&(String, String, String)> = rows.iter().filter(|r| r.1 == "master").collect();
    assert_eq!(masters.len(), 1, "{}", show(&rows));
    assert_eq!(masters[0].2, "/visio/masters/master1.xml");
}

#[test]
fn the_last_instance_of_a_master_taken_out_takes_the_row_with_it() {
    let bytes =
        page_with_master_package(&shape_with(5, " Master=\"1\"", &cell("PinX", "1")), &[("rId5", &format!("{VIS}master"), "../masters/master1.xml")]);
    let (mut doc, _) = read_ok(&bytes);
    // As it is, the row stays.
    let kept = model_write(&doc, false);
    assert_eq!(rows_of(&kept.bytes, "/visio/pages/page1.xml").iter().filter(|r| r.1 == "master").count(), 1);
    Arc::make_mut(doc.pages.get_mut(0).unwrap()).shapes = Default::default();
    let written = model_write(&doc, false);
    let rows = rows_of(&written.bytes, "/visio/pages/page1.xml");
    assert!(rows.iter().all(|r| r.1 != "master"), "{}", show(&rows));
}

#[test]
fn a_row_to_a_master_taken_out_is_not_written() {
    let bytes =
        page_with_master_package(&shape_with(5, " Master=\"1\"", &cell("PinX", "1")), &[("rId5", &format!("{VIS}master"), "../masters/master1.xml")]);
    let (mut doc, _) = read_ok(&bytes);
    doc.masters.remove(0).unwrap();
    let written = model_write(&doc, false);
    let rows = rows_of(&written.bytes, "/visio/pages/page1.xml");
    assert!(rows.iter().all(|r| r.1 != "master"), "a row leads to a master that is not in the file: {}", show(&rows));
}

fn background_package() -> Vec<u8> {
    VsdxBuilder::new()
        .page(PageSpec::new(1, "Back").attrs(r#" Background="1""#))
        .page(PageSpec::new(2, "Other").attrs(r#" Background="1""#))
        .page(PageSpec::new(3, "Front").attrs(r#" BackPage="1""#).rel("rId8", &format!("{VIS}page"), "page1.xml"))
        .build()
}

fn background_rows(bytes: &[u8]) -> Vec<String> {
    rows_of(bytes, &page_part(bytes, "Front")).iter().filter(|r| r.1 == "page").map(|r| r.2.clone()).collect()
}

#[test]
fn a_background_page_taken_out_leaves_no_row_to_a_part_that_is_not_written() {
    let (mut doc, _) = read_ok(&background_package());
    doc.pages.remove(0).unwrap();
    let written = model_write(&doc, false);
    let p = parts(&written.bytes);
    for target in background_rows(&written.bytes) {
        assert!(p.contains_key(&target), "the page has a row to {target}, which is not in the file");
    }
}

#[test]
fn a_background_page_that_is_cleared_or_changed_leaves_its_row_the_page_that_is_now_the_background() {
    let (mut doc, _) = read_ok(&background_package());
    assert_eq!(background_rows(&model_write(&doc, false).bytes), ["/visio/pages/page1.xml"]);
    // Changed to the other background page.
    Arc::make_mut(doc.pages.get_mut(2).unwrap()).back_page = Some(2);
    assert_eq!(background_rows(&model_write(&doc, false).bytes), ["/visio/pages/page2.xml"]);
    // Cleared.
    Arc::make_mut(doc.pages.get_mut(2).unwrap()).back_page = None;
    assert!(background_rows(&model_write(&doc, false).bytes).is_empty());
}

#[test]
fn a_row_to_a_page_that_is_no_background_page_is_the_files_own_and_stays() {
    // R6 of the pages spec: a page can have such a row and no `BackPage`, and nothing says why. It is kept.
    let pkg = VsdxBuilder::new().page(PageSpec::new(1, "One")).page(PageSpec::new(2, "Two").rel("rId8", &format!("{VIS}page"), "page1.xml")).build();
    let (doc, _) = read_ok(&pkg);
    let written = model_write(&doc, false);
    let rows = rows_of(&written.bytes, &page_part(&written.bytes, "Two"));
    assert_eq!(rows.iter().filter(|r| r.1 == "page").map(|r| r.2.as_str()).collect::<Vec<_>>(), ["/visio/pages/page1.xml"], "{}", show(&rows));
}

// -------------------------------------------------------------: the rows a page had, and the rows its shapes use, are kept

/// The rows of `source`, sorted, for a comparison that the order of the rows does not move (OPC gives it no meaning).
fn sorted_rows(bytes: &[u8], source: &str) -> Vec<(String, String, String)> {
    let mut rows = rows_of(bytes, source);
    rows.sort();
    rows
}

/// A drawing whose pages have rows the model does not explain: "Front" has one to a master no shape on it uses and one to a background page though it has no `BackPage` (R6 of the pages
/// spec found five files with such a row), and the whole relationships part of "Only" is one row to a master no shape on it uses.
fn unexplained_rows_package() -> Vec<u8> {
    VsdxBuilder::new()
        .master(MasterSpec::new(1, "Box").shapes(&shape(4, &cell("Width", "1"))))
        .page(PageSpec::new(1, "Back").attrs(r#" Background="1""#).shapes(&shape(1, &cell("PinX", "9"))))
        .page(PageSpec::new(2, "Front").shapes(&shape(1, &cell("PinX", "1"))).rel("rId5", &format!("{VIS}master"), "../masters/master1.xml").rel(
            "rId6",
            &format!("{VIS}page"),
            "page1.xml",
        ))
        .page(PageSpec::new(3, "Only").shapes(&shape(1, &cell("PinX", "2"))).rel("rId9", &format!("{VIS}master"), "../masters/master1.xml"))
        .build()
}

#[test]
fn every_row_a_page_had_is_written_by_a_save_of_the_drawing_as_it_was_read() {
    // The gate's sample-file report: rows to masters were not written in 7 files, and a relationships part of such rows was left out in 2, on a visit that edited nothing. While the drawing is
    // as it was read, a row the model does not explain is the file's own.
    let bytes = unexplained_rows_package();
    let (doc, _) = read_ok(&bytes);
    let written = crate::model_writer::visit("rows the model does not explain", &bytes);
    for page in ["Back", "Front", "Only"] {
        let (before, after) = (sorted_rows(&bytes, &page_part(&bytes, page)), sorted_rows(&written.bytes, &page_part(&written.bytes, page)));
        assert_eq!(after, before, "{page}: the rows written are not the rows the page had: {}", show(&after));
    }
    assert_eq!(sorted_rows(&written.bytes, "/visio/pages/page3.xml").len(), 1, "the relationships part of a page of one such row is written");
    // After an edit they are made from the model: no shape uses the master, and the page has no background page (R22 of the masters spec).
    let edited = model_write(&doc, false);
    let front = rows_of(&edited.bytes, &page_part(&edited.bytes, "Front"));
    assert!(front.iter().all(|r| r.1 != "master" && r.1 != "page"), "{}", show(&front));
    assert!(rows_of(&edited.bytes, &page_part(&edited.bytes, "Only")).is_empty());
}

#[test]
fn a_row_the_page_uses_is_written_by_every_save() {
    // A master a shape on the page is an instance of, at the top and in a group (nested ones count), its row spelt as a path from the root of the package; a picture and an embedded
    // object the shapes show. Each is kept, edited or not.
    let shapes = format!(
        "{}{}{}{}",
        r#"<Shape ID="1" Master="1"><Cell N="PinX" V="1"/></Shape>"#,
        r#"<Shape ID="2" Type="Group"><Shapes><Shape ID="3" Master="2"><Cell N="PinX" V="2"/></Shape></Shapes></Shape>"#,
        r#"<Shape ID="4" Type="Foreign"><ForeignData ForeignType="Bitmap"><Rel r:id="rId2"/></ForeignData></Shape>"#,
        r#"<Shape ID="5" Type="Foreign"><ForeignData ForeignType="Object" ObjectType="2"><Rel r:id="rId3"/></ForeignData></Shape>"#,
    );
    let bytes = VsdxBuilder::new()
        .master(MasterSpec::new(1, "One").shapes(&shape(4, &cell("Width", "1"))))
        .master(MasterSpec::new(2, "Two").shapes(&shape(5, &cell("Width", "2"))))
        .page(
            PageSpec::new(1, "P")
                .shapes(&shapes)
                .rel("rId1", &format!("{VIS}master"), "/visio/masters/master1.xml")
                .rel("rId2", &format!("{OD}image"), "../media/m.png")
                .rel("rId3", &format!("{OD}oleObject"), "../embeddings/o.bin")
                .rel("rId4", &format!("{VIS}master"), "../masters/master2.xml"),
        )
        .with(Pkg::new().part("/visio/media/m.png", "image/png", [0x89u8, b'P', b'N', b'G', 1]).part("/visio/embeddings/o.bin", CT_OLE, [7u8]))
        .build();
    let (doc, _) = read_ok(&bytes);
    let before = sorted_rows(&bytes, "/visio/pages/page1.xml");
    assert_eq!(before.len(), 4);
    for unchanged in [true, false] {
        let written = model_write(&doc, unchanged);
        let after = sorted_rows(&written.bytes, &page_part(&written.bytes, "P"));
        assert_eq!(after, before, "as read {unchanged}: {}", show(&after));
        let p = parts(&written.bytes);
        for (_, _, target) in &after {
            assert!(p.contains_key(target), "as read {unchanged}: a row leads to {target}, which is not in the file");
        }
    }
}

#[test]
fn a_row_of_the_master_or_page_kind_to_a_part_that_is_neither_is_the_files_own() {
    // Rows of those kinds whose part is held (it is no master of the index and no page): they lead to a part the file has, and no edit can have made them stale.
    let bytes = VsdxBuilder::new()
        .page(PageSpec::new(1, "P").shapes(&shape(1, &cell("PinX", "1"))).rel("rId5", &format!("{VIS}master"), "../extra/thing.xml").rel(
            "rId6",
            &format!("{VIS}page"),
            "../extra/other.xml",
        ))
        .with(Pkg::new().part("/visio/extra/thing.xml", "application/xml", "<thing/>").part("/visio/extra/other.xml", "application/xml", "<other/>"))
        .build();
    let (doc, _) = read_ok(&bytes);
    let before = sorted_rows(&bytes, "/visio/pages/page1.xml");
    for unchanged in [true, false] {
        let written = model_write(&doc, unchanged);
        assert_eq!(sorted_rows(&written.bytes, "/visio/pages/page1.xml"), before, "as read {unchanged}");
        let p = parts(&written.bytes);
        assert!(p.contains_key("/visio/extra/thing.xml") && p.contains_key("/visio/extra/other.xml"), "{:?}", p.keys());
    }
}

// ----------------------------------------------------------------------------------- item 3: DiaCraft's data next to another's solution

/// A solutions index of DiaCraft's solution and another program's, and the part of each.
fn two_solutions(diacraft: &str) -> Vec<u8> {
    let index = format!(
        "<Solutions xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Solution Name=\"DiaCraft\"><Rel r:id=\"rId1\"/></Solution><Solution Name=\"OtherApp\"><Rel r:id=\"rId2\"/></Solution></Solutions>"
    );
    let extra = Pkg::new()
        .part("/visio/solutions/solutions.xml", CT_SOLUTIONS, index)
        .rel(DOC, "rId60", &format!("{VIS}solutions"), "solutions/solutions.xml")
        .part("/visio/solutions/solution1.xml", CT_SOLUTION, diacraft)
        .rel("/visio/solutions/solutions.xml", "rId1", &format!("{VIS}solution"), "solution1.xml")
        .part("/visio/solutions/solution2.xml", CT_SOLUTION, "<SolutionXML Name=\"OtherApp\"><Data x=\"1\"/></SolutionXML>")
        .rel("/visio/solutions/solutions.xml", "rId2", &format!("{VIS}solution"), "solution2.xml");
    VsdxBuilder::new().page(PageSpec::new(1, "P")).with(extra).build()
}

const DIACRAFT_DATA: &str = r#"<dc:Data xmlns:dc="urn:app.diacraft:extension" dc:version="1"><dc:snap alignment_box="1"/></dc:Data>"#;

fn solution_edit_is_written(diacraft_part: &str, whole: bool) {
    let original = two_solutions(diacraft_part);
    let (mut doc, _) = read_ok(&original);
    assert!(doc.set_diacraft_child("snap", Some(&[("alignment_box", "0"), ("guides", "0")])));
    let written = model_write(&doc, false);
    let p = parts(&written.bytes);
    let ours = text_of(&written.bytes, "/visio/solutions/solution1.xml");
    assert!(ours.contains(r#"<dc:snap alignment_box="0" guides="0"/>"#), "the data the model holds was not written: {ours}");
    if !whole {
        assert!(ours.contains("<Note>another reader's</Note>"), "what else the part holds is as it was: {ours}");
    }
    // Another program's solution is the file's, byte for byte.
    assert_eq!(p.get("/visio/solutions/solution2.xml"), parts(&original).get("/visio/solutions/solution2.xml"));
    let pkg = package(&written.bytes);
    assert_eq!(pkg.content_type("/visio/solutions/solution1.xml"), Some("application/vnd.ms-visio.solution+xml"));
    // The model handed back is what the file says, and a save into the file finds nothing to write.
    let (reread, _) = read_ok(&written.bytes);
    assert_eq!(reread.diacraft, doc.diacraft);
    assert_eq!(written.document.diacraft, reread.diacraft);
    let again = write_with(
        &doc,
        Some(&OriginalBytes { bytes: &written.bytes, baseline: Some(&written.document) }),
        &WriteOptions { now: Some(1_700_000_000), ..WriteOptions::default() },
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(text_of(&again.bytes, "/visio/solutions/solution1.xml").contains(r#"<dc:snap alignment_box="0" guides="0"/>"#));
}

#[test]
fn the_data_of_the_model_is_written_into_the_part_another_programs_solutions_leave_held() {
    // The part holds a note of another reader's beside the data: it is held as the bytes it is, and the data in it is replaced.
    solution_edit_is_written(&format!("<SolutionXML Name=\"DiaCraft\">{DIACRAFT_DATA}<Note>another reader's</Note></SolutionXML>"), false);
}

#[test]
fn the_data_of_the_model_is_written_from_the_model_when_the_part_holds_nothing_else() {
    // The part holds nothing but the data: the model rebuilds it, and the part is not held beside the model's data.
    let original = two_solutions(&format!("<SolutionXML Name=\"DiaCraft\">{DIACRAFT_DATA}</SolutionXML>"));
    let (doc, _) = read_ok(&original);
    assert!(
        doc.parts.iter().all(|p| !p.name.ends_with("solution1.xml")),
        "the part is held as bytes beside the data: {:?}",
        doc.parts.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
    assert!(doc.parts.iter().any(|p| p.name.ends_with("solution2.xml")), "another program's part is held");
    solution_edit_is_written(&format!("<SolutionXML Name=\"DiaCraft\">{DIACRAFT_DATA}</SolutionXML>"), true);
}

#[test]
fn data_set_in_the_model_of_a_file_whose_solutions_are_another_programs_gets_a_row_and_a_part() {
    let index = format!("<Solutions xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Solution Name=\"OtherApp\"><Rel r:id=\"rId2\"/></Solution></Solutions>");
    let extra = Pkg::new()
        .part("/visio/solutions/solutions.xml", CT_SOLUTIONS, index)
        .rel(DOC, "rId60", &format!("{VIS}solutions"), "solutions/solutions.xml")
        .part("/visio/solutions/solution2.xml", CT_SOLUTION, "<SolutionXML Name=\"OtherApp\"><Data x=\"1\"/></SolutionXML>")
        .rel("/visio/solutions/solutions.xml", "rId2", &format!("{VIS}solution"), "solution2.xml");
    let original = VsdxBuilder::new().page(PageSpec::new(1, "P")).with(extra).build();
    let (mut doc, _) = read_ok(&original);
    assert!(doc.diacraft.is_none());
    assert!(doc.set_diacraft_child("snap", Some(&[("alignment_box", "0")])));
    let written = model_write(&doc, false);
    let (reread, report) = read_ok(&written.bytes);
    assert_eq!(reread.diacraft, doc.diacraft, "{:?}", report.diacraft);
    let p = parts(&written.bytes);
    assert_eq!(p.get("/visio/solutions/solution2.xml"), parts(&original).get("/visio/solutions/solution2.xml"));
    assert!(text_of(&written.bytes, "/visio/solutions/solutions.xml").contains("OtherApp"));
    assert_eq!(written.document.diacraft, doc.diacraft);
}

// ----------------------------------------------------------------------------------------------- any edits of the pages and masters

/// A drawing of a background page, a page of masters in use, an embedded object and a row of two others, and two masters: every kind of row between parts the writer makes or keeps.
fn edited_seed() -> Vec<u8> {
    let uses = r#"<Shape ID="1" Master="1"><Cell N="PinX" V="1"/></Shape><Shape ID="2" Type="Foreign"><ForeignData ForeignType="Object" ObjectType="2"><Rel r:id="rId2"/></ForeignData></Shape>"#;
    let extra = Pkg::new().part("/visio/embeddings/o.bin", CT_OLE, [1u8]).part("/visio/media/m.png", "image/png", [0x89u8, b'P', b'N', b'G', 1]);
    VsdxBuilder::new()
        .master(MasterSpec::new(1, "One").shapes(&shape(4, &cell("Width", "1"))).rel("rId1", &format!("{OD}image"), "../media/m.png"))
        .master(MasterSpec::new(2, "Two").shapes(&shape(5, &cell("Width", "2"))))
        .page(PageSpec::new(1, "Back").attrs(r#" Background="1""#).shapes(&shape(1, &cell("PinX", "9"))))
        .page(
            PageSpec::new(2, "Uses")
                .attrs(r#" BackPage="1""#)
                .shapes(uses)
                .rel("rId2", &format!("{OD}oleObject"), "../embeddings/o.bin")
                .rel("rId3", &format!("{VIS}page"), "page1.xml")
                .rel("rId4", &format!("{VIS}master"), "../masters/master1.xml"),
        )
        .page(PageSpec::new(3, "Plain").shapes(&shape(1, &cell("PinX", "2"))))
        .with(extra)
        .build()
}

fn apply(doc: &mut Document, op: u8, at: usize) {
    let pages = doc.pages.len();
    let masters = doc.masters.len();
    match op % 8 {
        0 => {
            let _ = doc.add_page(Some("Added"));
        }
        1 if pages > 0 => {
            doc.pages.remove(at % pages);
        }
        2 if pages > 1 => {
            if let Some(page) = doc.pages.remove(at % pages) {
                let to = (at / 2) % pages.max(1);
                let _ = doc.pages.insert(to.min(doc.pages.len()), page);
            }
        }
        3 if pages > 0 => {
            let target = doc.pages.get((at + 1) % pages).map(|p| p.id);
            Arc::make_mut(doc.pages.get_mut(at % pages).unwrap()).back_page = target;
        }
        4 if pages > 0 => Arc::make_mut(doc.pages.get_mut(at % pages).unwrap()).back_page = None,
        5 if masters > 0 => {
            doc.masters.remove(at % masters);
        }
        6 if masters > 0 => {
            let mut copy = (**doc.masters.get(at % masters).unwrap()).clone();
            copy.id = copy.id.saturating_add(10 + at as u32);
            copy.rel_id = None;
            copy.part_name = None;
            copy.unique_id = None;
            let _ = doc.add_master(copy);
        }
        _ if pages > 0 => with_shape(doc, at % pages, Shape { id: 7_000 + at as u32, master: Some(1 + (at % 3) as u32), ..Shape::default() }),
        _ => {}
    }
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig { cases: 200, failure_persistence: None, ..proptest::prelude::ProptestConfig::default() })]

    /// **Any run of edits to the pages and masters is written, and no relationship of the package it makes leads to a part that is not in it** (the model writer never panics, never gives a page the
    /// rows of another, and never leaves a row to a page, master or picture it did not write), and the package reads.
    #[test]
    fn edits_of_the_pages_and_masters_leave_no_row_that_leads_nowhere(ops in proptest::collection::vec((0u8..16, 0usize..8), 0..14)) {
        let (mut doc, _) = read_ok(&edited_seed());
        for (op, at) in &ops {
            apply(&mut doc, *op, *at);
        }
        let Ok(written) = crate::model_writer::try_write(&doc, &crate::model_writer::options(false)) else { return Ok(()) };
        let pkg = package(&written.bytes);
        let names: BTreeSet<String> = pkg.parts().map(str::to_owned).collect();
        let sources = std::iter::once("/".to_owned()).chain(names.iter().filter(|n| !n.ends_with(".rels")).cloned());
        for source in sources {
            for row in pkg.rels(&source).unwrap().items.iter().filter(|r| r.mode != diacraft_opc::TargetMode::External) {
                let leads = row.target.clone().unwrap_or_default();
                proptest::prop_assert!(names.contains(&leads), "{source} has a row {} to {leads}, which the package does not have; ops {ops:?}", row.id);
            }
        }
        proptest::prop_assert!(read_ok(&written.bytes).0.pages.len() == doc.pages.len(), "{ops:?}");
    }
}
