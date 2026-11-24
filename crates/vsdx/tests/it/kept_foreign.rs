//! Bug 2: **a `ForeignData` kept as an unknown element still names a row of its part, and a part a save makes new gives it that row**. On a shape whose `Type` is not
//! `Foreign` the reader does not read a `ForeignData`: it keeps it whole in the shape's kept markup, `Rel` and all. A copy of the page
//! (`page.duplicate`) is a page with no part, and the part a save makes for it got rows only for the `ForeignData`s that were read, so the kept one named a row
//! that was not there and its picture led nowhere. Now every relationship id of a shape's kept markup gets its row in such a part, the same row the source's part has, whatever the
//! shape's `Type` and whatever element holds it, through both writers: the save into the file the drawing was read from, and the model writer (`write_with(doc, None, ..)`).
//!
//! Every input is built in code.

use std::collections::BTreeSet;
use std::sync::Arc;

use diacraft_doc::{Document, Page, RawNode, Shape, ShapeList};
use diacraft_opc::TargetMode;
use diacraft_opc::name::{fold, is_rels_part, rels_source};
use diacraft_testkit::package::{OD, PageSpec, Pkg, VsdxBuilder, cell, shape};
use diacraft_vsdx::{Limits, OriginalBytes, WriteOptions, Written, compare_content, write_with};

use crate::model_writer::{NOW, package, page_cell};
use crate::read_ok;

const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 1, 2, 3, 4];
const PNG_B: [u8; 8] = [0x89, b'P', b'N', b'G', 5, 6, 7, 8];
const IMAGE1: &str = "/visio/media/image1.png";
const IMAGE2: &str = "/visio/media/image2.png";

/// A shape of `Type` `Shape` that holds a picture's `ForeignData`: kept as an unknown element.
const KEPT: &str = r#"<Shape ID="7" Type="Shape"><Cell N="PinX" V="6"/><ForeignData ForeignType="Bitmap" CompressionType="PNG"><Rel r:id="rId1"/></ForeignData></Shape>"#;
/// A group whose member, of no `Type`, holds one that names another row.
const GROUP: &str = r#"<Shape ID="3" Type="Group"><Cell N="PinX" V="3"/><Shapes><Shape ID="4"><Cell N="PinX" V="4"/><ForeignData ForeignType="Bitmap" CompressionType="PNG"><Rel r:id="rId2"/></ForeignData></Shape></Shapes></Shape>"#;
/// A picture that is read, naming the row the kept one names.
const READ: &str = r#"<Shape ID="8" Type="Foreign"><Cell N="PinX" V="8"/><ForeignData ForeignType="Bitmap" CompressionType="PNG"><Rel r:id="rId1"/></ForeignData></Shape>"#;

fn image() -> String {
    format!("{OD}image")
}

/// Which writer.
#[derive(Clone, Copy, Debug)]
enum Path {
    Splice,
    Model,
}

const PATHS: [Path; 2] = [Path::Splice, Path::Model];

fn save(doc: &Document, original: &[u8], baseline: Option<&Document>, path: Path) -> Written {
    let opts = WriteOptions { now: Some(NOW), ..WriteOptions::default() };
    let result = match path {
        Path::Splice => write_with(doc, Some(&OriginalBytes { bytes: original, baseline }), &opts),
        Path::Model => write_with(doc, None, &opts),
    };
    result.unwrap_or_else(|e| panic!("{path:?}: the save refused: {e}"))
}

/// Page 1 holds a box (6), a shape of `Type` `Shape` with a kept picture (7, `rId1`), a group whose member keeps one (4, `rId2`), and a picture that is read (8, `rId1`); page 2 a box.
fn source() -> Vec<u8> {
    VsdxBuilder::new()
        .page(
            PageSpec::new(1, "P")
                .shapes(&format!("{}{KEPT}{GROUP}{READ}", shape(6, &cell("PinX", "1"))))
                .rel("rId1", &image(), "../media/image1.png")
                .rel("rId2", &image(), "../media/image2.png"),
        )
        .page(PageSpec::new(2, "Other").shapes(&shape(1, &cell("PinX", "1"))))
        // The pictures are typed by a `Default` row, as the model writer types them (`model_writer.rs`: which of the two a file chose is not in the model), so that the files of the two writers
        // can be held to the same content.
        .with(Pkg::new().default_type("png", "image/png").part_without_override(IMAGE1, PNG).part_without_override(IMAGE2, PNG_B))
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

/// Every `r:id` written in a page part that names no row of that part's relationships: `(part, id)`. The pages' parts are the ones under `pages/` that are not the index.
fn unnamed(bytes: &[u8]) -> Vec<(String, String)> {
    let pkg = package(bytes);
    let mut out = Vec::new();
    for part in pkg.parts().filter(|n| n.starts_with("/visio/pages/page") && n.ends_with(".xml")).map(str::to_owned).collect::<Vec<_>>() {
        let text = String::from_utf8(pkg.part(&part).unwrap().to_vec()).unwrap();
        let rels = pkg.rels(&part).ok();
        for (at, _) in text.match_indices(r#"r:id=""#) {
            let rest = &text[at + 6..];
            let id = &rest[..rest.find('"').unwrap()];
            if rels.as_ref().is_none_or(|r| r.get(id).is_none()) {
                out.push((part.clone(), id.to_owned()));
            }
        }
    }
    out
}

/// The parts of the file that hold exactly `content`.
fn holding(bytes: &[u8], content: &[u8]) -> Vec<String> {
    let pkg = package(bytes);
    pkg.parts().filter(|n| pkg.part(n) == Some(content)).map(str::to_owned).collect()
}

/// The kept `ForeignData` of the shape at `path` (places among members) of a list, and the id its `Rel` names.
fn kept_rel(list: &ShapeList, path: &[u32]) -> Option<String> {
    let (first, rest) = path.split_first()?;
    let s: &Shape = list.iter().find(|s| s.id == *first)?;
    if !rest.is_empty() {
        return kept_rel(&s.children, rest);
    }
    assert!(s.foreign.is_none(), "a ForeignData on a shape of Type {:?} is not read", s.kind);
    s.passthrough.iter().find_map(|p| match &p.node {
        RawNode::Element(e) if &*e.name == "ForeignData" => e.children.iter().find_map(|c| match c {
            RawNode::Element(rel) if &*rel.name == "Rel" => rel.attrs.get("r:id").map(str::to_owned),
            _ => None,
        }),
        _ => None,
    })
}

/// Page `page` of the file shows, through rows of its own part, the kept pictures of page 1 of [`source`], to the very parts page 1's rows reach, and the picture that is read.
fn assert_rows(bytes: &[u8], page: u32, case: &str) {
    let (back, report) = read_ok(bytes);
    assert_eq!(report.warning_count("picture_unresolved"), 0, "{case}: {:?}", report.warnings);
    let p = back.page(page).unwrap_or_else(|| panic!("{case}: no page {page}"));
    let part = p.part_name.clone().unwrap_or_else(|| panic!("{case}: page {page} has no part"));
    let rels = package(bytes).rels(&part).unwrap();
    for (path, target) in [(&[7][..], IMAGE1), (&[3, 4][..], IMAGE2)] {
        let id = kept_rel(&p.shapes, path).unwrap_or_else(|| panic!("{case}: shape {path:?} of page {page} keeps no ForeignData with a Rel"));
        let row = rels.get(&id).unwrap_or_else(|| panic!("{case}: the kept Rel {id} of shape {path:?} of page {page} names no row of {part}"));
        assert_eq!((row.ty.as_str(), row.target.as_deref()), (image().as_str(), Some(target)), "{case}: shape {path:?} of page {page}");
    }
    let read = p.shapes.iter().find(|s| s.id == 8).and_then(|s| s.foreign.clone()).unwrap();
    assert_eq!(read.picture.as_ref().map(|m| m.bytes.to_vec()), Some(PNG.to_vec()), "{case}: the picture that is read, on page {page}");
    let ids: Vec<String> = rels.items.iter().map(|r| r.id.to_ascii_lowercase()).collect();
    assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), ids.len(), "{case}: two rows of {part} have one id: {ids:?}");
}

/// No row leads nowhere, no id of a page part names no row, and each picture is one part.
fn assert_whole(bytes: &[u8], case: &str) {
    assert!(dangling(bytes).is_empty(), "{case}: rows lead to parts the file lacks: {:?}", dangling(bytes));
    assert!(unnamed(bytes).is_empty(), "{case}: ids name rows that are not there: {:?}", unnamed(bytes));
    assert_eq!(holding(bytes, &PNG), [IMAGE1], "{case}: the first picture");
    assert_eq!(holding(bytes, &PNG_B), [IMAGE2], "{case}: the second picture");
}

#[test]
fn a_copied_page_gives_the_kept_foreign_data_of_a_shape_of_any_type_the_rows_its_source_has_after_either_save() {
    let bytes = source();
    for path in PATHS {
        let (mut doc, report) = read_ok(&bytes);
        assert_eq!(report.warning_count("picture_unresolved"), 0, "{:?}", report.warnings);
        let copy = duplicate(&mut doc, 1);
        let written = save(&doc, &bytes, None, path);
        let case = format!("{path:?}");
        assert_rows(&written.bytes, 1, &case);
        assert_rows(&written.bytes, copy, &case);
        assert_whole(&written.bytes, &case);
        // The file says what the model says: the copy's shapes read back as the model holds them, the kept markup with the ids it had.
        let (back, _) = read_ok(&written.bytes);
        assert_eq!(back.page(copy).unwrap().shapes, doc.page(copy).unwrap().shapes, "{case}: the copy's shapes read back as the model holds them");
        assert_eq!(kept_rel(&back.page(copy).unwrap().shapes, &[7]).as_deref(), Some("rId1"), "{case}");
    }
}

#[test]
fn a_copy_saved_reopened_and_edited_is_saved_again_with_its_rows_by_either_writer() {
    let bytes = source();
    for path in PATHS {
        let (mut doc, _) = read_ok(&bytes);
        let copy = duplicate(&mut doc, 1);
        let first = save(&doc, &bytes, None, path);
        // Reopened (the read of the file is the baseline, as the engine has it after any save) and the copy's box moved: its part is patched, and the rows stay.
        let (back, _) = read_ok(&first.bytes);
        let mut edited = back.clone();
        // Shape 6 is the first of the copy's shapes.
        page_cell(&mut edited, copy, 0, "PinX", "2.5");
        for again in PATHS {
            let second = save(&edited, &first.bytes, Some(&back), again);
            let case = format!("first {path:?}, then {again:?}");
            assert_rows(&second.bytes, 1, &case);
            assert_rows(&second.bytes, copy, &case);
            assert_whole(&second.bytes, &case);
            // The content check: the edit is all that differs from the first save.
            let (read_again, _) = read_ok(&second.bytes);
            assert_eq!(
                read_again
                    .page(copy)
                    .unwrap()
                    .shapes
                    .iter()
                    .find(|s| s.id == 6)
                    .unwrap()
                    .sheet
                    .cell("PinX")
                    .unwrap()
                    .value
                    .as_ref()
                    .map(|v| v.text()),
                Some("2.5"),
                "{case}"
            );
        }
        // The two writers write the same content for the copy.
        let other = match path {
            Path::Splice => Path::Model,
            Path::Model => Path::Splice,
        };
        let one = save(&edited, &first.bytes, Some(&back), path);
        let two = save(&edited, &first.bytes, Some(&back), other);
        let diff = compare_content(&one.bytes, &two.bytes, &Limits::default()).unwrap();
        assert!(diff.same(), "first {path:?}: the content of the two writers' files differs: {:?}", diff.shown);
    }
}

#[test]
fn an_id_of_kept_markup_taken_in_the_new_part_by_another_row_is_written_with_one_of_its_own() {
    // A copy whose picture that is read, put first, names `rId1` but shows the second picture, and whose kept shape names `rId1` for the first: the part gives `rId1` to the first shape
    // walked, and the kept markup gets an id of its own, written into it.
    let bytes = source();
    for path in PATHS {
        let (mut doc, _) = read_ok(&bytes);
        let copy = duplicate(&mut doc, 1);
        {
            let at = doc.pages.iter().position(|p| p.id == copy).unwrap();
            let page = Arc::make_mut(doc.pages.get_mut(at).unwrap());
            // The picture is given another allocation, attributes the source does not have and the second picture, so it is not found on the source page and is given a row to its picture.
            let at = (0..page.shapes.len()).find(|&i| page.shapes.get(i).is_some_and(|s| s.id == 8)).unwrap();
            let moved = page.shapes.remove(at).unwrap();
            let mut moved = Shape::clone(&moved);
            let mut foreign = diacraft_doc::ForeignData::clone(moved.foreign.as_ref().unwrap());
            let mut media = diacraft_doc::Media::clone(foreign.picture.as_ref().unwrap());
            media.bytes = Arc::from(&PNG_B[..]);
            media.part = Arc::from(IMAGE2);
            foreign.picture = Some(Arc::new(media));
            foreign.attrs = diacraft_doc::RawAttrs::from_pairs([("ForeignType", "Bitmap"), ("CompressionType", "PNG"), ("Marked", "1")]);
            moved.foreign = Some(Arc::new(foreign));
            page.shapes.insert(0, Arc::new(moved)).unwrap();
        }
        let written = save(&doc, &bytes, None, path);
        let case = format!("{path:?}");
        assert!(dangling(&written.bytes).is_empty() && unnamed(&written.bytes).is_empty(), "{case}");
        let (back, _) = read_ok(&written.bytes);
        let p = back.page(copy).unwrap();
        let rels = package(&written.bytes).rels(p.part_name.as_deref().unwrap()).unwrap();
        let read = p.shapes.iter().find(|s| s.id == 8).and_then(|s| s.foreign.clone()).unwrap();
        assert_eq!(read.rel_id.as_deref(), Some("rId1"), "{case}: the first shape walked keeps its id");
        assert_eq!(read.picture.as_ref().map(|m| m.bytes.to_vec()), Some(PNG_B.to_vec()), "{case}: and shows its own picture");
        let kept = kept_rel(&p.shapes, &[7]).unwrap();
        assert_ne!(kept, "rId1", "{case}: the kept markup is given another id");
        assert_eq!(rels.get(&kept).and_then(|r| r.target.clone()).as_deref(), Some(IMAGE1), "{case}: which leads to its own picture");
    }
}

// ------------------------------------------------------------------------------------------------------------------------------ crafted inputs, counted

/// The relationships namespace, which an `r:id` is in.
const NS_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// One crafted case and its yardstick: the attributes and the content of the kept `ForeignData` of shape 7, and the same markup made plain (the same size, no declaration and no attribute
/// that may name a relationship), with the number of declarations the case's markup makes.
struct Crafted {
    attrs: String,
    inner: String,
    plain_attrs: String,
    plain_inner: String,
    declarations: u64,
}

/// An element that names 50,000 relationship ids, each by a prefix it declares for the relationships namespace (`xmlns:pN` and `pN:id`).
fn many_ids_as_attributes() -> Crafted {
    let attrs: String = (0..50_000).map(|n| format!(r#" xmlns:p{n}="{NS_REL}" p{n}:id="rId{}""#, n + 2)).collect();
    let plain_attrs: String = (0..50_000).map(|n| format!(r#" xmlnsxp{n}="{NS_REL}" p{n}xid="rId{}""#, n + 2)).collect();
    Crafted { attrs, inner: String::new(), plain_attrs, plain_inner: String::new(), declarations: 50_000 }
}

/// An element whose 50,000 children each name a relationship id, before the `ForeignData`'s own `Rel`.
fn many_ids_as_children() -> Crafted {
    let inner: String = format!("<Extra>{}</Extra>", (0..50_000).map(|n| format!(r#"<Rel r:id="rId{}"/>"#, n + 2)).collect::<String>());
    let plain_inner: String = format!("<Extra>{}</Extra>", (0..50_000).map(|n| format!(r#"<Rel rxid="rId{}"/>"#, n + 2)).collect::<String>());
    Crafted { attrs: String::new(), inner, plain_attrs: String::new(), plain_inner, declarations: 0 }
}

/// 10,000 children, each declaring a prefix and naming an id in it, under 150 levels that each declare one more.
fn wide_under_deep_declarations() -> Crafted {
    let (depth, width) = (150, 10_000);
    let nest = |declare: &str, children: &str| {
        let open: String = (0..depth).map(|k| format!(r#"<L {declare}d{k}="urn:d{k}">"#)).collect();
        format!("{open}{children}{}", "</L>".repeat(depth))
    };
    let children: String = (0..width).map(|n| format!(r#"<C xmlns:c="urn:c" c:id="v{n}"/>"#)).collect();
    let plain: String = (0..width).map(|n| format!(r#"<C xmlnsxc="urn:c" cxid="v{n}"/>"#)).collect();
    Crafted {
        attrs: String::new(),
        inner: nest("xmlns:", &children),
        plain_attrs: String::new(),
        plain_inner: nest("xmlnsx", &plain),
        declarations: (depth + width) as u64,
    }
}

/// A page whose shape 7, of `Type` `Shape`, keeps a `ForeignData` with `attrs`, and `inner` before its own `Rel`, of `rId1`.
fn crafted_source(attrs: &str, inner: &str) -> Vec<u8> {
    let kept = format!(
        r#"<Shape ID="7" Type="Shape"><Cell N="PinX" V="6"/><ForeignData ForeignType="Bitmap"{attrs}>{inner}<Rel r:id="rId1"/></ForeignData></Shape>"#
    );
    VsdxBuilder::new()
        .page(PageSpec::new(1, "P").shapes(&format!("{}{kept}", shape(6, &cell("PinX", "1")))).rel("rId1", &image(), "../media/image1.png"))
        .with(Pkg::new().default_type("png", "image/png").part_without_override(IMAGE1, PNG))
        .build()
}

/// The source read, its page copied and saved by `path`: the steps the walks of kept markup took and the entries they copied, and the file.
fn kept_work(bytes: &[u8], path: Path) -> (u64, u64, Written) {
    let (mut doc, _) = read_ok(bytes);
    duplicate(&mut doc, 1);
    let (steps, copied) = (diacraft_vsdx::work::kept_steps(), diacraft_vsdx::work::kept_entries_copied());
    let written = save(&doc, bytes, None, path);
    (diacraft_vsdx::work::kept_steps() - steps, diacraft_vsdx::work::kept_entries_copied() - copied, written)
}

#[test]
fn kept_markup_that_names_many_ids_or_declares_deep_and_wide_costs_a_walk_of_its_size_and_copies_only_its_declarations() {
    for (case, crafted) in [
        ("50,000 ids as attributes", many_ids_as_attributes()),
        ("50,000 ids as children", many_ids_as_children()),
        ("10,000 children under 150 declarations", wide_under_deep_declarations()),
    ] {
        let (bytes, plain) = (crafted_source(&crafted.attrs, &crafted.inner), crafted_source(&crafted.plain_attrs, &crafted.plain_inner));
        assert_eq!(
            crafted.attrs.len() + crafted.inner.len(),
            crafted.plain_attrs.len() + crafted.plain_inner.len(),
            "{case}: the yardstick is markup of the same size"
        );
        for path in PATHS {
            let (steps, copied, written) = kept_work(&bytes, path);
            let (yard_steps, yard_copied, _) = kept_work(&plain, path);
            eprintln!("{case}, {path:?}: {steps} steps and {copied} entries copied, beside {yard_steps} and {yard_copied} for plain markup");
            // The walk is of the markup's size: a step for each element, attribute and declaration, and a bounded few for each id it looks up.
            assert!(steps <= 2 * yard_steps + 20_000, "{case}, {path:?}: {steps} steps beside {yard_steps}");
            // **What is copied is the declarations, once each**: not the attributes of an element for each id it names, nor the declarations in force for each child.
            assert!(copied <= crafted.declarations + 200, "{case}, {path:?}: {copied} entries copied for {} declarations", crafted.declarations);
            assert!(yard_copied <= 200, "{case}, {path:?}: {yard_copied}");
            // The save is a file whose copy names the row of `rId1`.
            let (back, _) = read_ok(&written.bytes);
            let copy = back.pages.get(1).unwrap();
            let rels = package(&written.bytes).rels(copy.part_name.as_deref().unwrap()).unwrap();
            assert_eq!(rels.get("rId1").and_then(|r| r.target.clone()).as_deref(), Some(IMAGE1), "{case}, {path:?}");
        }
    }
}

const SCENARIO: &str = "DIACRAFT_KEPT_SCENARIO";

#[test]
#[ignore = "run by the test below under `ulimit -v`; alone it needs a scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("the scenario");
    let crafted = match scenario.as_str() {
        "attributes" => many_ids_as_attributes(),
        "children" => many_ids_as_children(),
        _ => wide_under_deep_declarations(),
    };
    let bytes = crafted_source(&crafted.attrs, &crafted.inner);
    for path in PATHS {
        let (mut doc, _) = read_ok(&bytes);
        duplicate(&mut doc, 1);
        let opts = WriteOptions { now: Some(NOW), ..WriteOptions::default() };
        let outcome = match path {
            Path::Splice => write_with(&doc, Some(&OriginalBytes { bytes: &bytes, baseline: None }), &opts),
            Path::Model => write_with(&doc, None, &opts),
        };
        match outcome {
            Ok(written) => println!("OUTCOME {path:?} written {} bytes", written.bytes.len()),
            Err(e) => println!("OUTCOME {path:?} refused:{} {e}", e.class()),
        }
    }
    let peak = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|t| t.lines().find_map(|l| l.strip_prefix("VmPeak:").map(|v| v.trim().to_owned())))
        .unwrap_or_default();
    println!("VMPEAK {peak}");
    println!("kept: no abort");
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

#[test]
#[cfg(target_os = "linux")]
fn the_crafted_kept_markup_is_saved_by_either_writer_under_the_address_limit_and_does_not_abort() {
    let exe = std::env::current_exe().unwrap();
    let script = "ulimit -v 3000000 && exec \"$0\" --exact kept_foreign::child --ignored --nocapture --test-threads=1";
    for scenario in ["attributes", "children", "declarations"] {
        let out = std::process::Command::new("/bin/sh").arg("-c").arg(script).arg(&exe).env(SCENARIO, scenario).output().unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success() && text.contains("kept: no abort"), "{scenario}: the child did not end well: {text}");
        assert!(text.contains("OUTCOME Splice written") && text.contains("OUTCOME Model written"), "{scenario}: both saves end as a file: {text}");
        eprintln!("{scenario}: {}", text.lines().filter(|l| l.contains("OUTCOME") || l.contains("VMPEAK")).collect::<Vec<_>>().join(" | "));
    }
}
