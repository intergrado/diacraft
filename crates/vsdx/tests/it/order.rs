//! The order of the entries of a saved package, and with it the bytes: a save of one model is the same file every time.
//!
//! The writer put the relationships parts it makes into the package in the order a hash map listed them, so two saves of one model, in one
//! process or in two, gave the entries in different orders and the lists of what a save wrote in different orders too (the order of the
//! zip's entries is why two runs of the old code never gave the same bytes). The rule is now:
//!
//! * a part the source package has keeps its place, in the order of the source's central directory, except that `[Content_Types].xml` is
//!   first and `_rels/.rels` second (which a writer may follow);
//! * a part the save makes goes after those, parts in the order the writer makes them, and then the relationships parts the save made or
//!   rewrote, by the place of the part each belongs to (the package's own first), and by the name where two places are the same.
//!
//! Every input is built in code. Standard `HashMap`s take a new random seed in every process and a new one for every map, and std has no
//! knob to set it, so "several hash seeds" here means several saves in this process and several processes of this test binary
//! (`run_child`, the pattern of `timing.rs`), each of which has seeds of its own.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use diacraft_doc::{Document, Master, Theme};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};
use diacraft_testkit::package::{self, MasterSpec, OD, PK, PageSpec, Pkg, VsdxBuilder};
use diacraft_vsdx::{OriginalBytes, Variant, WriteOptions, Written, write_with};

use crate::read_ok;

const NOW: i64 = 1_700_000_000;

fn options() -> WriteOptions {
    WriteOptions { now: Some(NOW), ..WriteOptions::default() }
}

fn write_new(doc: &Document) -> Written {
    write_with(doc, None, &options()).unwrap_or_else(|e| panic!("{e}"))
}

fn save(doc: &Document, source: &[u8], baseline: &Document, options: &WriteOptions) -> Written {
    write_with(doc, Some(&OriginalBytes { bytes: source, baseline: Some(baseline) }), options).unwrap_or_else(|e| panic!("{e}"))
}

// ------------------------------------------------------------------------------------------------------------------- the archive

fn u16_at(bytes: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes([bytes[at], bytes[at + 1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> usize {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize
}

/// The names of the entries of a ZIP, read from its headers and not through the package reader: the central directory's order, which the local
/// headers must keep (each at the offset the directory gives, in the same order, with the same name).
fn entry_names(bytes: &[u8]) -> Vec<String> {
    let end = bytes.len() - 22;
    assert_eq!(&bytes[end..end + 4], b"PK\x05\x06", "an end record with no archive comment");
    let (count, mut at) = (u16_at(bytes, end + 10), u32_at(bytes, end + 16));
    let (mut names, mut offsets) = (Vec::new(), Vec::new());
    for _ in 0..count {
        assert_eq!(&bytes[at..at + 4], b"PK\x01\x02");
        let (name, extra, comment) = (u16_at(bytes, at + 28), u16_at(bytes, at + 30), u16_at(bytes, at + 32));
        offsets.push(u32_at(bytes, at + 42));
        names.push(String::from_utf8(bytes[at + 46..at + 46 + name].to_vec()).unwrap());
        at += 46 + name + extra + comment;
    }
    for (name, &offset) in names.iter().zip(&offsets) {
        assert_eq!(&bytes[offset..offset + 4], b"PK\x03\x04");
        let local = u16_at(bytes, offset + 26);
        assert_eq!(&bytes[offset + 30..offset + 30 + local], name.as_bytes(), "the local header names the entry the directory does");
    }
    assert!(offsets.windows(2).all(|w| w[0] < w[1]), "the local headers are in the order of the directory");
    names
}

fn slashed(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| format!("/{n}")).collect()
}

// ---------------------------------------------------------------------------------------------------------------------- fixtures

/// A shape that is an instance of master `id`, so that the page it is on has a relationships part to the master.
fn instance(id: u32) -> Arc<diacraft_doc::Shape> {
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(9).master(id).cell("PinX", "1"))).build();
    doc.pages.get(0).unwrap().shapes.get(0).unwrap().clone()
}

/// A drawing of `pages` pages that each hold an instance of master 2.
fn drawing(pages: u32) -> Document {
    let mut b = DocBuilder::new().master(2, MasterBuilder::new().name("M").shape(ShapeBuilder::new(1).cell("Width", "1")));
    for id in 1..=pages {
        b = b.page(id, PageBuilder::new().shape(ShapeBuilder::new(1).master(2)));
    }
    b.build()
}

const THEME: &[u8] =
    br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="T"/>"#;

fn theme(part: &str) -> Arc<Theme> {
    Arc::new(Theme { part: Arc::from(part), bytes: Arc::from(THEME), ..Theme::default() })
}

/// The package of a drawing of two pages and a master, with the entries in an order no rule of ours would give: the builder's, reversed (so
/// `[Content_Types].xml` is last, and `_rels/.rels` nearly).
fn reversed_source() -> Vec<u8> {
    let builder = VsdxBuilder::new().master(MasterSpec::new(2, "M")).page(PageSpec::new(1, "A")).page(PageSpec::new(2, "B"));
    let mut items = builder.pkg().items();
    items.reverse();
    package::zip(&items)
}

/// What a source of that order gives, before any edit: `[Content_Types].xml` and `_rels/.rels` first, then every other entry as the source has it.
fn source_order_with_the_two_first(source: &[u8]) -> Vec<String> {
    let names = entry_names(source);
    let (first, second) = ("[Content_Types].xml".to_owned(), "_rels/.rels".to_owned());
    let mut want = vec![first.clone(), second.clone()];
    want.extend(names.iter().filter(|n| **n != first && **n != second).cloned());
    want
}

// ----------------------------------------------------------------------------------------------------------------------- the order

/// The parts of a new drawing of `pages` pages that each use a master, in the order the writer gives them: the skeleton's, the parts the pages and
/// the master add (masters index and master, then the pages), then the relationships parts a save made, by the place of the part each is the
/// relationships of: the masters index, then page 1, 2, 3 and on in that order (a plain sort by name puts `page10` before `page2`).
fn new_drawing_entries(pages: u32) -> Vec<String> {
    let mut want: Vec<String> = [
        "[Content_Types].xml",
        "_rels/.rels",
        "docProps/app.xml",
        "docProps/core.xml",
        "docProps/custom.xml",
        "visio/document.xml",
        "visio/_rels/document.xml.rels",
        "visio/pages/pages.xml",
        "visio/pages/_rels/pages.xml.rels",
        "visio/windows.xml",
        "visio/masters/masters.xml",
        "visio/masters/master1.xml",
    ]
    .iter()
    .map(|n| (*n).to_owned())
    .collect();
    want.extend((1..=pages).map(|n| format!("visio/pages/page{n}.xml")));
    want.push("visio/masters/_rels/masters.xml.rels".to_owned());
    want.extend((1..=pages).map(|n| format!("visio/pages/_rels/page{n}.xml.rels")));
    want
}

#[test]
fn a_new_drawing_with_many_relationships_parts_has_its_entries_in_the_defined_order_on_every_save() {
    // Twelve pages that use a master make thirteen relationships parts that are new: 13! orders, of which the old code gave one by chance.
    let doc = drawing(12);
    let want = new_drawing_entries(12);
    let first = write_new(&doc);
    assert_eq!(entry_names(&first.bytes), want);
    for round in 0..25 {
        let again = write_new(&doc);
        assert_eq!(entry_names(&again.bytes), want, "save {round}");
        // The lists of what the save did are in the same order too, and so are the bytes (the zip's date is fixed, and `now` is given).
        assert_eq!((&again.added, &again.rewritten, &again.removed), (&first.added, &first.rewritten, &first.removed), "save {round}");
        assert!(again.bytes == first.bytes, "save {round}: the same model gives the same bytes");
    }
    // Added: the new parts in the order they are made, the relationships parts last, by the place of the part they belong to. Rewritten: the parts
    // the skeleton had and the save changed, the relationships parts among them in the same order (the document's, then the pages index's).
    let mut added = slashed(&["visio/masters/masters.xml", "visio/masters/master1.xml"]);
    added.extend((1..=12).map(|n| format!("/visio/pages/page{n}.xml")));
    added.push("/visio/masters/_rels/masters.xml.rels".to_owned());
    added.extend((1..=12).map(|n| format!("/visio/pages/_rels/page{n}.xml.rels")));
    assert_eq!(first.added, added);
    assert_eq!(
        first.rewritten,
        slashed(&["visio/document.xml", "visio/pages/pages.xml", "visio/_rels/document.xml.rels", "visio/pages/_rels/pages.xml.rels"])
    );
    assert!(first.removed.is_empty());
}

#[test]
fn nine_hundred_relationships_parts_are_in_the_order_of_their_pages() {
    // The relationships parts a save made are ordered by the place of their pages (page 900 after
    // page 899, not after page 90).
    let doc = drawing(900);
    let want = new_drawing_entries(900);
    let first = write_new(&doc);
    assert_eq!(entry_names(&first.bytes), want);
    assert_eq!(first.added.len(), 2 + 900 + 1 + 900);
    let again = write_new(&doc);
    assert!(again.bytes == first.bytes && again.added == first.added && again.rewritten == first.rewritten);
}

#[test]
fn the_content_types_come_first_and_the_package_relationships_second_in_every_kind_of_save() {
    for (what, written) in scenarios() {
        let names = entry_names(&written.bytes);
        assert_eq!(&names[..2], ["[Content_Types].xml", "_rels/.rels"], "{what}");
    }
}

#[test]
fn a_save_with_no_edit_keeps_the_order_of_the_source_but_for_the_two_the_convention_puts_first() {
    let source = reversed_source();
    let names = entry_names(&source);
    let mut sorted = names.clone();
    sorted.sort();
    assert_ne!(names, sorted, "the source is in no order of ours");
    assert_eq!(names.last().map(String::as_str), Some("[Content_Types].xml"), "and has the content types last");
    let (doc, _) = read_ok(&source);
    let written = save(&doc, &source, &doc, &options());
    let want = source_order_with_the_two_first(&source);
    assert_eq!(entry_names(&written.bytes), want);
    assert_ne!(want, names, "the two moved: that is the one exception, and it is the convention's");
    // The same on the next saves, and the same bytes.
    for round in 0..10 {
        let again = save(&doc, &source, &doc, &options());
        assert!(again.bytes == written.bytes && entry_names(&again.bytes) == want, "save {round}");
    }
}

/// A read document with three pages added, each holding an instance of the master the file has.
fn three_pages_added(source: &[u8]) -> (Document, Document) {
    let (baseline, _) = read_ok(source);
    let mut doc = baseline.clone();
    for _ in 0..3 {
        let id = doc.add_page(None).unwrap();
        let at = doc.pages.iter().position(|p| p.id == id).unwrap();
        let mut page = (**doc.pages.get(at).unwrap()).clone();
        page.shapes = std::iter::once(instance(2)).collect();
        doc.pages.set(at, Arc::new(page)).unwrap();
    }
    (baseline, doc)
}

#[test]
fn an_edit_keeps_every_entry_of_the_source_in_its_place_and_puts_the_new_ones_after_them_in_the_defined_order() {
    let source = reversed_source();
    let (baseline, doc) = three_pages_added(&source);
    let mut want = source_order_with_the_two_first(&source);
    // Three pages: the parts first, then the relationships parts they have, by the place of the page each belongs to.
    want.extend(["visio/pages/page3.xml", "visio/pages/page4.xml", "visio/pages/page5.xml"].map(str::to_owned));
    want.extend(["visio/pages/_rels/page3.xml.rels", "visio/pages/_rels/page4.xml.rels", "visio/pages/_rels/page5.xml.rels"].map(str::to_owned));
    let first = save(&doc, &source, &baseline, &options());
    assert_eq!(entry_names(&first.bytes), want);
    assert_eq!(
        first.added,
        slashed(&["visio/pages/page3.xml", "visio/pages/page4.xml", "visio/pages/page5.xml"])
            .into_iter()
            .chain(slashed(&["visio/pages/_rels/page3.xml.rels", "visio/pages/_rels/page4.xml.rels", "visio/pages/_rels/page5.xml.rels"]))
            .collect::<Vec<_>>()
    );
    for round in 0..25 {
        let again = save(&doc, &source, &baseline, &options());
        assert_eq!(entry_names(&again.bytes), want, "save {round}");
        assert_eq!((&again.added, &again.rewritten, &again.removed), (&first.added, &first.rewritten, &first.removed), "save {round}");
        assert!(again.bytes == first.bytes, "save {round}");
    }
    // The relationships parts the source had and the save changed are where the source had them, and are listed as rewritten in the order of the
    // parts they belong to (the package's own are not changed here): the document's, then the pages index's.
    assert!(
        first.rewritten.iter().position(|n| n == "/visio/_rels/document.xml.rels")
            < first.rewritten.iter().position(|n| n == "/visio/pages/_rels/pages.xml.rels")
    );
}

#[test]
fn rewritten_relationships_parts_are_listed_in_the_order_of_the_parts_they_belong_to_and_not_by_name() {
    // The pages index and the masters index each gain a row, so both of their relationships parts are rewritten. The source has the pages index
    // before the masters index (the other way round from their names), and the list follows the source's order.
    let source = VsdxBuilder::new().master(MasterSpec::new(2, "M")).page(PageSpec::new(1, "A")).page(PageSpec::new(2, "B")).build();
    let names = entry_names(&source);
    let place = |name: &str| names.iter().position(|n| n == name).unwrap();
    assert!(place("visio/pages/pages.xml") < place("visio/masters/masters.xml"));
    let (baseline, _) = read_ok(&source);
    let mut doc = baseline.clone();
    doc.add_page(None).unwrap();
    doc.add_master(Master { id: 7, name: Some(Arc::from("Seven")), ..Master::default() }).unwrap();
    let first = save(&doc, &source, &baseline, &options());
    let rels = |w: &Written| -> Vec<String> { w.rewritten.iter().filter(|n| n.ends_with(".rels")).cloned().collect() };
    assert_eq!(rels(&first), slashed(&["visio/pages/_rels/pages.xml.rels", "visio/masters/_rels/masters.xml.rels"]));
    for round in 0..25 {
        let again = save(&doc, &source, &baseline, &options());
        assert_eq!(rels(&again), rels(&first), "save {round}");
        assert_eq!(entry_names(&again.bytes), entry_names(&first.bytes), "save {round}");
    }
}

// ------------------------------------------------------------------------------------------------------------------- scenarios

fn macro_source() -> Vec<u8> {
    let pkg = Pkg::new().part("/visio/vbaProject.bin", "application/vnd.ms-office.vbaProject", b"VBA-BYTES").rel(
        "/visio/document.xml",
        "rId50",
        "http://schemas.microsoft.com/office/2006/relationships/vbaProject",
        "vbaProject.bin",
    );
    VsdxBuilder::new().document_type("application/vnd.ms-visio.drawing.macroEnabled.main+xml").page(PageSpec::new(1, "A")).with(pkg).build()
}

/// Two pages, the first with a picture only it reaches, a master, and a thumbnail.
fn extras_source() -> Vec<u8> {
    VsdxBuilder::new()
        .page(PageSpec::new(1, "First").rel("rId9", &format!("{OD}image"), "../media/image1.png"))
        .page(PageSpec::new(2, "Other"))
        .master(MasterSpec::new(1, "Box"))
        .with(
            Pkg::new()
                .default_type("png", "image/png")
                .part_without_override("/visio/media/image1.png", b"\x89PNG.fake")
                .part_without_override("/docProps/thumbnail.emf", b"emf-bytes")
                .default_type("emf", "image/x-emf")
                .rel("/", "rId8", &format!("{PK}thumbnail"), "docProps/thumbnail.emf"),
        )
        .build()
}

/// How many saves [`scenarios`] makes.
const SCENARIOS: usize = 8;

/// Saves of every kind that put parts into a package or take them out: a new drawing with many pages, one with themes, a new master, an edit of a
/// file whose entries are in an odd order and a save of it with no edit, a page taken out with its picture, the data of DiaCraft set in a file, and
/// a drawing with its macros taken out.
fn scenarios() -> Vec<(&'static str, Written)> {
    let mut out = Vec::new();
    out.push(("a new drawing of twelve pages", write_new(&drawing(12))));
    let mut themed = drawing(3);
    for n in 1..=3 {
        themed.themes.push(theme(&format!("/visio/theme/theme{n}.xml")));
    }
    out.push(("a new drawing of three pages and three themes", write_new(&themed)));
    let source = reversed_source();
    let (baseline, doc) = three_pages_added(&source);
    out.push(("three pages added to a file with its entries reversed", save(&doc, &source, &baseline, &options())));
    out.push(("no edit of a file with its entries reversed", save(&baseline, &source, &baseline, &options())));
    let mut with_theme = doc.clone();
    with_theme.themes.push(theme("/visio/theme/theme1.xml"));
    with_theme.add_master(Master { id: 7, name: Some(Arc::from("Seven")), ..Master::default() }).unwrap();
    out.push(("a theme and a master added to a file with its entries reversed", save(&with_theme, &source, &baseline, &options())));
    let macros = macro_source();
    let (read, _) = read_ok(&macros);
    let dropped = WriteOptions { variant: Variant::from_extension("vsdx"), drop_macros: true, ..options() };
    out.push(("a macro project left out", save(&read, &macros, &read, &dropped)));
    let extras = extras_source();
    let (baseline, _) = read_ok(&extras);
    let mut doc = baseline.clone();
    doc.pages.remove(0).unwrap();
    doc.add_page(Some("Third")).unwrap();
    out.push(("a page taken out with its picture and one added", save(&doc, &extras, &baseline, &options())));
    let plain = VsdxBuilder::new().page(PageSpec::new(1, "A")).build();
    let (baseline, _) = read_ok(&plain);
    let mut doc = baseline.clone();
    assert!(doc.set_diacraft_child("snap", Some(&[("alignment_box", "1"), ("glue", "0")])));
    out.push(("the DiaCraft data set in a file that had none", save(&doc, &plain, &baseline, &options())));
    out
}

/// A hash of a save that two processes can compare: the bytes and every list, with a hasher whose keys are fixed.
fn digest(written: &Written) -> u64 {
    let mut hasher = DefaultHasher::new();
    written.bytes.hash(&mut hasher);
    (&written.rewritten, &written.added, &written.removed, &written.warnings, written.kept, &written.macros_dropped).hash(&mut hasher);
    hasher.finish()
}

fn digests() -> Vec<(&'static str, u64)> {
    scenarios().iter().map(|(what, written)| (*what, digest(written))).collect()
}

#[test]
fn every_kind_of_save_gives_the_same_bytes_and_lists_in_one_process() {
    let first = digests();
    assert_eq!(first.len(), SCENARIOS);
    for round in 0..20 {
        assert_eq!(digests(), first, "round {round}");
    }
}

/// The name of the variable that tells the test binary, run as a child, to print its digests.
const CHILD: &str = "DIACRAFT_VSDX_ORDER_CHILD";

/// The digests of a child of this test binary: a process of its own, with hash seeds of its own.
fn run_child() -> Vec<(String, u64)> {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let out = std::process::Command::new(exe)
        .args(["--exact", "order::child", "--ignored", "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .output()
        .expect("run the test binary");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "the child failed: {stdout}\n{}", String::from_utf8_lossy(&out.stderr));
    // The first line a test prints is on the line of its own name (`test order::child ... DIGEST ...`).
    let lines: Vec<(String, u64)> = stdout
        .lines()
        .filter_map(|l| l.find("DIGEST ").map(|at| &l[at + "DIGEST ".len()..]))
        .filter_map(|rest| {
            let (hex, what) = rest.split_once(' ')?;
            Some((what.to_owned(), u64::from_str_radix(hex, 16).ok()?))
        })
        .collect();
    assert_eq!(lines.len(), SCENARIOS, "the digests the child printed: {stdout}");
    lines
}

/// Run as a child of [`every_kind_of_save_gives_the_same_bytes_and_lists_in_every_process`]. `ignored`, so that a plain run of the suite does not
/// run it as a test of its own; with the variable unset it does nothing.
#[test]
#[ignore = "run as a child of the test that compares processes"]
fn child() {
    if std::env::var(CHILD).is_err() {
        return;
    }
    for (what, hash) in digests() {
        println!("DIGEST {hash:016x} {what}");
    }
}

#[test]
fn every_kind_of_save_gives_the_same_bytes_and_lists_in_every_process() {
    let here: Vec<(String, u64)> = digests().into_iter().map(|(what, hash)| (what.to_owned(), hash)).collect();
    // Six processes, each with its own hash seeds. The saves that put relationships parts into the package gave a different order in almost every one.
    for process in 0..6 {
        assert_eq!(run_child(), here, "process {process}");
    }
}
