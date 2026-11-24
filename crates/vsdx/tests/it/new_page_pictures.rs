//! **A page part a save makes new gets a row for every picture, embedded object and other part its shapes name** (a `ForeignData`'s
//! `Rel` is an `r:id` of the relationships of the part the shape is in). A copy of a page (`page.duplicate`) holds shapes whose `Rel` names a row of the part of the page it was copied from; the
//! part of the copy is new and had no rows, so before this every such shape lost its picture or its object when the drawing was saved. Both writers: the save into the file the drawing was read
//! from, and the model writer (`write_with(doc, None, ..)`). The rows lead to the parts the source's rows lead to (one part, two rows: nothing is copied), a picture that no part of the file holds
//! gets a part of its own made from the model's bytes, and the ids are unique in the part.
//!
//! Every input is built in code.

use std::collections::BTreeSet;
use std::sync::Arc;

use diacraft_doc::{Document, Page, Shape};
use diacraft_opc::TargetMode;
use diacraft_opc::name::{fold, is_rels_part, rels_source};
use diacraft_testkit::package::{MasterSpec, OD, PageSpec, Pkg, VIS, VsdxBuilder, cell, shape};
use diacraft_vsdx::{OriginalBytes, WriteOptions, Written, write_with};

use crate::model_writer::{CT_OLE, NOW, new_picture_shape, package, picture};
use crate::read_ok;

const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 1, 2, 3, 4];
const PNG_B: [u8; 8] = [0x89, b'P', b'N', b'G', 5, 6, 7, 8];
const EMF: [u8; 6] = [1, 0, 0, 0, 9, 9];
const OLE: [u8; 4] = [0xD0, 0xCF, 0x11, 0xE0];
const IMAGE1: &str = "/visio/media/image1.png";
const OBJECT1: &str = "/visio/embeddings/oleObject1.bin";

const PICTURE: &str = r#"<Shape ID="7" Type="Foreign"><Cell N="PinX" V="6"/><ForeignData ForeignType="Bitmap" CompressionType="PNG"><Rel r:id="rId1"/></ForeignData></Shape>"#;
const OBJECT: &str =
    r#"<Shape ID="8" Type="Foreign"><Cell N="PinX" V="3"/><ForeignData ForeignType="Object" ObjectType="2"><Rel r:id="rId2"/></ForeignData></Shape>"#;
const INSTANCE: &str = r#"<Shape ID="9" Master="1"><Cell N="PinX" V="4"/></Shape>"#;

fn image() -> String {
    format!("{OD}image")
}

fn ole() -> String {
    format!("{OD}oleObject")
}

/// Which writer.
#[derive(Clone, Copy, Debug)]
enum Path {
    Model,
    Splice,
}

const PATHS: [Path; 2] = [Path::Splice, Path::Model];

fn save(doc: &Document, original: &[u8], baseline: Option<&Document>, path: Path) -> Written {
    let opts = WriteOptions { now: Some(NOW), ..WriteOptions::default() };
    let result = match path {
        Path::Model => write_with(doc, None, &opts),
        Path::Splice => write_with(doc, Some(&OriginalBytes { bytes: original, baseline }), &opts),
    };
    result.unwrap_or_else(|e| panic!("{path:?}: the save refused: {e}"))
}

/// Page 1 holds a box (6), a picture (7, `rId1` to a PNG), an embedded object (8, `rId2` to an OLE part whose own row leads to its EMF picture) and an instance of master 1 (9, `rId3`); page 2
/// holds a box.
fn source() -> Vec<u8> {
    let plain = shape(6, &cell("PinX", "1"));
    VsdxBuilder::new()
        .master(MasterSpec::new(1, "M").shapes(&shape(1, &cell("Width", "1"))))
        .page(
            PageSpec::new(1, "P")
                .shapes(&format!("{plain}{PICTURE}{OBJECT}{INSTANCE}"))
                .rel("rId1", &image(), "../media/image1.png")
                .rel("rId2", &ole(), "../embeddings/oleObject1.bin")
                .rel("rId3", &format!("{VIS}master"), "../masters/master1.xml"),
        )
        .page(PageSpec::new(2, "Other").shapes(&shape(1, &cell("PinX", "1"))))
        .with(Pkg::new().part(IMAGE1, "image/png", PNG).part(OBJECT1, CT_OLE, OLE).part("/visio/media/image2.emf", "image/x-emf", EMF).rel(
            OBJECT1,
            "rId1",
            &image(),
            "../media/image2.emf",
        ))
        .build()
}

/// A copy of page `from` put after it, as `page.duplicate` makes one: everything of the page under a new `ID` and name, and nothing of the place the page had in its package. → the copy's `ID`.
fn duplicate(doc: &mut Document, from: u32) -> u32 {
    let at = doc.pages.iter().position(|p| p.id == from).unwrap();
    let id = doc.pages.iter().map(|p| p.id).max().unwrap() + 1;
    let mut copy = Page::clone(doc.pages.get(at).unwrap());
    copy.id = id;
    copy.name = Arc::from(format!("{}-2", copy.name).as_str());
    copy.name_u = Some(Arc::clone(&copy.name));
    copy.leave_package();
    doc.pages.insert(at + 1, Arc::new(copy)).unwrap();
    id
}

fn remove_page(doc: &mut Document, id: u32) {
    let at = doc.pages.iter().position(|p| p.id == id).unwrap();
    doc.pages.remove(at).unwrap();
}

/// Every row of the file that leads to a part the file does not have: `(source, Id, target)`.
fn dangling(bytes: &[u8]) -> Vec<(String, String, String)> {
    let pkg = package(bytes);
    let names: BTreeSet<String> = pkg.parts().map(fold).collect();
    let mut out = Vec::new();
    for rels in pkg.parts().filter(|n| is_rels_part(n)).map(str::to_owned).collect::<Vec<_>>() {
        let Some(source) = rels_source(&rels) else { continue };
        for row in pkg.rels(&source).unwrap().items {
            if row.mode != TargetMode::External
                && let Some(target) = row.target.clone()
                && !names.contains(&fold(&target))
            {
                out.push((source.clone(), row.id.clone(), target));
            }
        }
    }
    out
}

/// The parts of the file that hold exactly `bytes`.
fn holding(bytes: &[u8], content: &[u8]) -> Vec<String> {
    let pkg = package(bytes);
    pkg.parts().filter(|n| pkg.part(n) == Some(content)).map(str::to_owned).collect()
}

/// The `ForeignData` of shape `id` of page `page` of the file read back, and the row its `Rel` names in the relationships of the page's part: `(type, the part it leads to)`.
fn shown(bytes: &[u8], page: u32, id: u32, case: &str) -> (Vec<u8>, String, Option<String>) {
    let (back, report) = read_ok(bytes);
    assert_eq!(report.warning_count("picture_unresolved"), 0, "{case}: {:?}", report.warnings);
    let p = back.page(page).unwrap_or_else(|| panic!("{case}: there is no page {page}"));
    let part = p.part_name.as_deref().unwrap_or_else(|| panic!("{case}: page {page} has no part"));
    let foreign =
        p.shapes.iter().find(|s| s.id == id).and_then(|s| s.foreign.clone()).unwrap_or_else(|| panic!("{case}: page {page} has no shape {id}"));
    let bytes_shown =
        foreign.picture.as_ref().map(|m| m.bytes.to_vec()).unwrap_or_else(|| panic!("{case}: shape {id} of page {page} shows no picture"));
    let rels = package(bytes).rels(part).unwrap();
    let ids: Vec<String> = rels.items.iter().map(|r| r.id.to_ascii_lowercase()).collect();
    assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), ids.len(), "{case}: two rows of {part} have one id: {ids:?}");
    let row =
        rels.get(foreign.rel_id.as_deref().unwrap()).unwrap_or_else(|| panic!("{case}: the Rel of shape {id} of page {page} names no row of {part}"));
    (bytes_shown, row.ty.clone(), row.target.clone())
}

/// Page `page` of the file shows the picture and the object of page 1 of [`source`], through rows to the very parts page 1's rows reach, and names the master its instance uses.
fn assert_shows_both(bytes: &[u8], page: u32, case: &str) {
    assert_eq!(shown(bytes, page, 7, case), (PNG.to_vec(), image(), Some(IMAGE1.to_owned())), "{case}: the picture of page {page}");
    assert_eq!(shown(bytes, page, 8, case), (EMF.to_vec(), ole(), Some(OBJECT1.to_owned())), "{case}: the object of page {page}");
    let (back, _) = read_ok(bytes);
    let part = back.page(page).unwrap().part_name.clone().unwrap();
    let rels = package(bytes).rels(&part).unwrap();
    assert!(
        rels.items.iter().any(|r| r.ty == format!("{VIS}master") && r.target.as_deref() == Some("/visio/masters/master1.xml")),
        "{case}: page {page} names no master: {:?}",
        rels.items
    );
}

/// The picture, the object and its picture are each one part of the file: the rows of two pages lead to one part, and nothing was copied.
fn assert_one_of_each(bytes: &[u8], case: &str) {
    assert_eq!(holding(bytes, &PNG), [IMAGE1], "{case}: the picture");
    assert_eq!(holding(bytes, &OLE), [OBJECT1], "{case}: the object");
    assert_eq!(holding(bytes, &EMF), ["/visio/media/image2.emf"], "{case}: the object's picture");
    let bad = dangling(bytes);
    assert!(bad.is_empty(), "{case}: rows lead to parts the file lacks: {bad:?}");
}

// ------------------------------------------------------------------------------------------------------------------------------ the cases

#[test]
fn a_copied_page_shows_the_picture_and_the_object_of_its_source_through_rows_to_the_same_parts_after_either_save() {
    let bytes = source();
    for path in PATHS {
        let (mut doc, _) = read_ok(&bytes);
        let copy = duplicate(&mut doc, 1);
        let written = save(&doc, &bytes, None, path);
        let case = format!("{path:?}");
        assert_shows_both(&written.bytes, 1, &case);
        assert_shows_both(&written.bytes, copy, &case);
        assert_one_of_each(&written.bytes, &case);
        // The ids of the shapes are the ones the model holds: the file says what the model says, so the next save has nothing to write for them.
        let (back, _) = read_ok(&written.bytes);
        let ids =
            |p: &Page| p.shapes.iter().filter_map(|s| s.foreign.as_ref().and_then(|f| f.rel_id.as_deref().map(str::to_owned))).collect::<Vec<_>>();
        assert_eq!(ids(back.page(copy).unwrap()), ["rId1", "rId2"], "{case}");
        assert_eq!(back.page(copy).unwrap().shapes, doc.page(copy).unwrap().shapes, "{case}: the copy's shapes read back as the model holds them");
    }
}

#[test]
fn a_copy_whose_source_page_is_taken_out_in_the_same_save_or_a_later_one_keeps_the_picture_and_the_object() {
    let bytes = source();
    for path in PATHS {
        // The same save.
        let (mut doc, _) = read_ok(&bytes);
        let copy = duplicate(&mut doc, 1);
        remove_page(&mut doc, 1);
        let written = save(&doc, &bytes, None, path);
        let case = format!("{path:?}, the source taken out in the same save");
        assert_shows_both(&written.bytes, copy, &case);
        assert_one_of_each(&written.bytes, &case);
        // A later save, into the file the first wrote, with its read-back as the baseline (what the engine does after any save).
        let (mut doc, _) = read_ok(&bytes);
        let copy = duplicate(&mut doc, 1);
        let first = save(&doc, &bytes, None, path);
        remove_page(&mut doc, 1);
        let second = save(&doc, &first.bytes, Some(&first.document), Path::Splice);
        let case = format!("{path:?}, the source taken out in a later save");
        assert_shows_both(&second.bytes, copy, &case);
        assert_one_of_each(&second.bytes, &case);
    }
}

#[test]
fn a_copy_saved_and_then_edited_saves_again_with_its_picture_and_its_object() {
    let bytes = source();
    for path in PATHS {
        let (mut doc, _) = read_ok(&bytes);
        let copy = duplicate(&mut doc, 1);
        let first = save(&doc, &bytes, None, path);
        // The box of the copy (its first shape) is moved: its part is patched, and its pictures are as the file has them.
        crate::model_writer::page_cell(&mut doc, copy, 0, "PinX", "2.5");
        let second = save(&doc, &first.bytes, Some(&first.document), Path::Splice);
        let case = format!("{path:?} then an edited save");
        assert_shows_both(&second.bytes, copy, &case);
        assert_shows_both(&second.bytes, 1, &case);
        assert_one_of_each(&second.bytes, &case);
    }
}

#[test]
fn two_shapes_of_a_new_page_that_name_one_id_for_two_pictures_each_show_their_own() {
    // Page 1's `rId1` is one picture and page 2's `rId1` another; a new page holds a shape from each.
    let first = r#"<Shape ID="7" Type="Foreign"><ForeignData ForeignType="Bitmap" CompressionType="PNG"><Rel r:id="rId1"/></ForeignData></Shape>"#;
    let bytes = VsdxBuilder::new()
        .page(PageSpec::new(1, "A").shapes(first).rel("rId1", &image(), "../media/image1.png"))
        .page(PageSpec::new(2, "B").shapes(first).rel("rId1", &image(), "../media/image3.png"))
        .with(Pkg::new().part(IMAGE1, "image/png", PNG).part("/visio/media/image3.png", "image/png", PNG_B))
        .build();
    for path in PATHS {
        let (mut doc, _) = read_ok(&bytes);
        let shape_of = |doc: &Document, page: u32| doc.page(page).unwrap().shapes.get(0).cloned().unwrap();
        let (a, b) = (shape_of(&doc, 1), shape_of(&doc, 2));
        let id = doc.add_page(Some("Both")).unwrap();
        let at = doc.pages.iter().position(|p| p.id == id).unwrap();
        {
            let page = Arc::make_mut(doc.pages.get_mut(at).unwrap());
            page.shapes.push(a);
            let mut other = Shape::clone(&b);
            other.id = 17;
            page.shapes.push(Arc::new(other));
        }
        let written = save(&doc, &bytes, None, path);
        let case = format!("{path:?}");
        let (seven, eight) = (shown(&written.bytes, id, 7, &case), shown(&written.bytes, id, 17, &case));
        assert_eq!(seven, (PNG.to_vec(), image(), Some(IMAGE1.to_owned())), "{case}");
        assert_eq!(eight, (PNG_B.to_vec(), image(), Some("/visio/media/image3.png".to_owned())), "{case}");
        assert!(dangling(&written.bytes).is_empty(), "{case}");
        assert_eq!(holding(&written.bytes, &PNG_B), ["/visio/media/image3.png"], "{case}: the second picture is not copied");
    }
}

#[test]
fn a_picture_from_another_format_on_a_new_page_is_given_a_part_of_its_own_and_two_pages_that_show_it_share_it() {
    // No relationship: the picture is the shape's own, as an `.odg` frame with an image is.
    let media = picture("Pictures/100000000000000A0000000A.png", &PNG_B);
    let add = |doc: &mut Document, name: &str| {
        let id = doc.add_page(Some(name)).unwrap();
        let at = doc.pages.iter().position(|p| p.id == id).unwrap();
        Arc::make_mut(doc.pages.get_mut(at).unwrap()).shapes.push(Arc::new(new_picture_shape(1, &media)));
        id
    };
    // Into a file that was read, and as a drawing that was never saved.
    let read = source();
    let (mut from_file, _) = read_ok(&read);
    let fresh_doc = Document::new_blank();
    let mut fresh = fresh_doc.clone();
    for (case, doc, original) in [("into a file", &mut from_file, Some(&read[..])), ("a new drawing", &mut fresh, None)] {
        let (one, two) = (add(doc, "One"), add(doc, "Two"));
        let opts = WriteOptions { now: Some(NOW), ..WriteOptions::default() };
        let written = match original {
            Some(bytes) => write_with(doc, Some(&OriginalBytes { bytes, baseline: None }), &opts),
            None => write_with(doc, None, &opts),
        }
        .unwrap_or_else(|e| panic!("{case}: the save refused: {e}"));
        for page in [one, two] {
            let (bytes_shown, ty, target) = shown(&written.bytes, page, 1, case);
            assert_eq!((bytes_shown, ty), (PNG_B.to_vec(), image()), "{case}: page {page}");
            let target = target.unwrap();
            assert!(target.starts_with("/visio/media/") && target.ends_with(".png"), "{case}: {target}");
            assert_eq!(package(&written.bytes).content_type(&target), Some("image/png"), "{case}");
        }
        assert_eq!(holding(&written.bytes, &PNG_B).len(), 1, "{case}: one part for the picture two pages show");
        assert!(dangling(&written.bytes).is_empty(), "{case}");
        let (back, _) = read_ok(&written.bytes);
        let attrs = back.page(one).unwrap().shapes.get(0).unwrap().foreign.clone().unwrap();
        assert_eq!(attrs.attr("CompressionType"), Some("PNG"), "{case}");
    }
}

// ------------------------------------------------------------------------------------------------------------------------------ the costs

/// Page 1 holds `n` embedded objects, each naming a row of its own (`rId1` to `rIdn`) to one object, whose own row leads to the EMF it is shown as; page 2 holds a box.
fn objects_page(n: u32) -> Vec<u8> {
    let shapes: String = (1..=n)
        .map(|i| {
            format!(r#"<Shape ID="{i}" Type="Foreign"><ForeignData ForeignType="Object" ObjectType="2"><Rel r:id="rId{i}"/></ForeignData></Shape>"#)
        })
        .collect();
    let page = (1..=n).fold(PageSpec::new(1, "P").shapes(&shapes), |page, i| page.rel(&format!("rId{i}"), &ole(), "../embeddings/oleObject1.bin"));
    VsdxBuilder::new()
        .page(page)
        .page(PageSpec::new(2, "Other").shapes(&box_shape()))
        .with(Pkg::new().part(OBJECT1, CT_OLE, OLE).part("/visio/media/image2.emf", "image/x-emf", EMF).rel(
            OBJECT1,
            "rId1",
            &image(),
            "../media/image2.emf",
        ))
        .build()
}

fn box_shape() -> String {
    shape(1, &cell("PinX", "1"))
}

/// Item 2 of the security scan: **the row a shape of a copy names is found in a map of its source part's rows by `Id`**, made once for each part, and not by a walk of the
/// rows for each shape (`n` shapes naming `n` rows were `n * n / 2` rows looked at). Both writers, counted in the rows each looked at (`work::rels_visited` for the save into a file, whose
/// relationships parts are trees, and `work::model_rows_walked` for the model writer).
#[test]
fn a_copy_of_a_page_whose_n_objects_name_n_rows_looks_at_rows_in_proportion_to_n() {
    let n = 2_000u32;
    let bytes = objects_page(n);
    let (mut doc, _) = read_ok(&bytes);
    let copy = duplicate(&mut doc, 1);
    diacraft_vsdx::work::reset();
    let spliced = save(&doc, &bytes, None, Path::Splice);
    let looked = diacraft_vsdx::work::rels_visited();
    eprintln!("[splice] a copy of {n} objects naming {n} rows: {looked} rows looked at");
    diacraft_vsdx::work::reset();
    let modelled = save(&doc, &bytes, None, Path::Model);
    let walked = diacraft_vsdx::work::model_rows_walked();
    eprintln!("[model] a copy of {n} objects naming {n} rows: {walked} rows walked");
    assert!(looked <= 8 * u64::from(n), "splice: {looked} rows looked at for {n} shapes");
    assert!(walked <= 12 * u64::from(n), "model: {walked} rows walked for {n} shapes");
    assert_eq!(shown(&spliced.bytes, copy, n, "Splice"), (EMF.to_vec(), ole(), Some(OBJECT1.to_owned())));
    assert_eq!(shown(&modelled.bytes, copy, n, "Model"), (EMF.to_vec(), ole(), Some(OBJECT1.to_owned())));
}

/// `k` pages, each with an object naming `rId1` to an object part of its own, whose own relationships are `r` rows (the first to its EMF); and a page that is not taken out.
fn gone_pages(k: u32, r: u32) -> Vec<u8> {
    let object_shape = r#"<Shape ID="1" Type="Foreign"><ForeignData ForeignType="Object" ObjectType="2"><Rel r:id="rId1"/></ForeignData></Shape>"#;
    let mut builder = VsdxBuilder::new();
    let mut parts = Pkg::new();
    for p in 1..=k {
        builder =
            builder.page(PageSpec::new(p, &format!("P{p}")).shapes(object_shape).rel("rId1", &ole(), &format!("../embeddings/oleObject{p}.bin")));
        let object = format!("/visio/embeddings/oleObject{p}.bin");
        parts = parts.part(&object, CT_OLE, OLE).part(&format!("/visio/media/fallback{p}.emf"), "image/x-emf", EMF).rel(
            &object,
            "rId1",
            &image(),
            &format!("../media/fallback{p}.emf"),
        );
        for j in 2..=r {
            parts = parts.rel(&object, &format!("rId{j}"), "urn:example:more", &format!("../media/more{p}x{j}.bin"));
        }
    }
    builder.page(PageSpec::new(k + 1, "Kept").shapes(&box_shape())).with(parts).build()
}

/// Item 1 of the security scan: **the model writer's search for the row of a shape whose page is gone** (a copy whose source was taken out before the save) is answered once
/// for each id and picture, looks at a bounded number of the rows of that id, and asks a map of what each held part reaches, made once; it does not walk every row of the id and every row of
/// its target for each shape (`m` shapes, `k` rows of one id and `r` rows of each target were `m * k * r` rows). Counted in `work::model_rows_walked`.
/// [`gone_pages`] read, its `k` pages taken out, and a page added of `m` shapes, each showing a picture of its own that no row of a gone page reaches, and naming `rId1` when `named`: every one
/// of them is asked about every gone row of `rId1`. → the model and the page's `ID`.
fn gone_doc(bytes: &[u8], k: u32, m: u32, named: bool) -> (Document, u32) {
    let (mut doc, _) = read_ok(bytes);
    let id = doc.add_page(Some("New")).unwrap();
    let at = doc.pages.iter().position(|p| p.id == id).unwrap();
    {
        let page = Arc::make_mut(doc.pages.get_mut(at).unwrap());
        for j in 1..=m {
            let media = Arc::new(diacraft_doc::Media {
                part: Arc::from(format!("/visio/media/other{j}.emf").as_str()),
                content_type: Arc::from("image/x-emf"),
                bytes: Arc::from(&[1u8, 0, 0, 0, 7, (j % 251) as u8, (j / 251) as u8][..]),
            });
            let attrs = diacraft_doc::RawAttrs::from_pairs([("ForeignType", "Object")]);
            let rel_id = named.then(|| Arc::from("rId1"));
            let foreign = diacraft_doc::ForeignData { attrs, rel_id, picture: Some(media), ..diacraft_doc::ForeignData::default() };
            page.shapes.push(Arc::new(Shape { id: j, kind: diacraft_doc::ShapeType::Foreign, foreign: Some(Arc::new(foreign)), ..Shape::default() }));
        }
    }
    for p in 1..=k {
        remove_page(&mut doc, p);
    }
    (doc, id)
}

#[test]
fn shapes_whose_page_is_gone_are_matched_to_the_rows_of_gone_pages_in_work_that_does_not_multiply() {
    let (k, r, m) = (200u32, 50u32, 300u32);
    let bytes = gone_pages(k, r);
    let (doc, id) = gone_doc(&bytes, k, m, true);
    diacraft_vsdx::work::reset();
    let written = save(&doc, &bytes, None, Path::Model);
    let walked = diacraft_vsdx::work::model_rows_walked();
    let bound = 80 * u64::from(m) + 4 * u64::from(k) * u64::from(r);
    eprintln!("[model] {m} shapes, {k} gone rows of one id, {r} rows of each target: {walked} rows walked (bound {bound})");
    assert!(walked <= bound, "{walked} rows walked, against {bound}");
    // Each shape shows its own picture, through a row of its own.
    let (back, report) = read_ok(&written.bytes);
    assert_eq!(report.warning_count("picture_unresolved"), 0, "{:?}", report.warnings);
    assert_eq!(back.page(id).unwrap().shapes.iter().filter(|s| s.foreign.as_ref().is_some_and(|f| f.picture.is_some())).count(), m as usize);
}

/// The two cases above, large, timed beside a yardstick. **The copy**: the same save of a copy of a page of as many boxes, which name no rows; a copy of a page of objects costs a small multiple
/// of it at most (the writing and the check of its 50,000 rows; 25,000 rows is near the most one relationships part of 4 MiB holds). **The shapes whose page is gone**: the same drawing whose
/// shapes name no relationship, so that no gone row is asked about.
#[test]
#[ignore = "timing: run with --ignored, on a machine that is not busy"]
fn timed_beside_a_yardstick() {
    use std::time::Instant;
    let time = |label: &str, work: &dyn Fn()| -> f64 {
        let start = Instant::now();
        work();
        let secs = start.elapsed().as_secs_f64();
        println!("{label}: {secs:.3} s");
        secs
    };
    let n = 25_000u32;
    let objects = objects_page(n);
    let boxes = {
        let shapes: String = (1..=n).map(|i| shape(i, &cell("PinX", "1"))).collect();
        VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shapes)).page(PageSpec::new(2, "Other").shapes(&box_shape())).build()
    };
    let copied = |bytes: &[u8]| {
        let (mut doc, _) = read_ok(bytes);
        duplicate(&mut doc, 1);
        doc
    };
    let (with, without) = (copied(&objects), copied(&boxes));
    for path in PATHS {
        let a = time(&format!("{path:?}: save a copy of {n} objects naming {n} rows"), &|| drop(save(&with, &objects, None, path)));
        let b = time(&format!("{path:?}: save a copy of {n} boxes (the yardstick)"), &|| drop(save(&without, &boxes, None, path)));
        assert!(a <= 8.0 * b + 0.5, "{path:?}: {a} against {b}");
    }
    let (k, r, m) = (500u32, 100u32, 2_000u32);
    let bytes = gone_pages(k, r);
    let ((named, _), (plain, _)) = (gone_doc(&bytes, k, m, true), gone_doc(&bytes, k, m, false));
    let a = time(&format!("Model: {m} shapes naming rId1, {k} gone rows of it, {r} rows of each target"), &|| {
        drop(save(&named, &bytes, None, Path::Model))
    });
    let b = time(&format!("Model: the same {m} shapes naming no row (the yardstick)"), &|| drop(save(&plain, &bytes, None, Path::Model)));
    assert!(a <= 4.0 * b + 0.5, "{a} against {b}");
}
