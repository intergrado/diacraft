//! DiaCraft's solution part is written with **no XML declaration**, by every writer (a check of the files in the VSDX application, 2026-11-27: it logs an invalid parameter for a solution part that
//! starts with one, and drops the part when it saves; the solution parts of the files it writes itself have none).
//!
//! Each test reads the bytes a writer made: the splice into a file (a part made new, and a part patched), a drawing made new, and the model writer (a part made new, a part the file has,
//! and the data of the other format). A part of another program is the file's bytes, whatever it starts with, and the reader takes a part with the declaration or without it, because the
//! files of the builds before this one have it.
//!
//! **A part of the builds before this one, which has the declaration, is written without it by any save that writes the file again** (an edit of anything, or a Save As), even when DiaCraft's
//! data did not change, and by cutting the declaration and nothing else; a save with no edit leaves it as it is.
//!
//! Every input is built in code.

use std::collections::BTreeMap;

use diacraft_doc::Document;
use diacraft_opc::{Limits as OpcLimits, Package};
use diacraft_testkit::package::{CT_SOLUTION, CT_SOLUTIONS, NS_2012, NS_R, PageSpec, Pkg, VIS, VsdxBuilder, ZipItem, cell, shape, zip};
use diacraft_vsdx::{DiacraftPart, Kind, OriginalBytes, Variant, WriteOptions, Written, write_with};

use crate::model_writer::{DOC, macro_drawing, model_write, model_write_as, page_cell, parts};
use crate::read_ok;

const SOLUTION: &str = "/visio/solutions/solution1.xml";
const INDEX: &str = "/visio/solutions/solutions.xml";
/// What the builds before this one wrote at the start of the part.
const OLD_DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n";
const DATA: &str = r#"<dc:Data xmlns:dc="urn:app.diacraft:extension" dc:version="1"><dc:snap alignment_box="1"/></dc:Data>"#;

fn options() -> WriteOptions {
    WriteOptions { now: Some(1_700_000_000), ..WriteOptions::default() }
}

fn drawing() -> Vec<u8> {
    VsdxBuilder::new().page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1")))).build()
}

/// A drawing whose DiaCraft part is the one of the builds before this one: the declaration, and the data alone.
fn old_drawing() -> Vec<u8> {
    VsdxBuilder::new()
        .page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1"))))
        .solution("DiaCraft", &format!("{OLD_DECLARATION}<SolutionXML Name=\"DiaCraft\">{DATA}</SolutionXML>"))
        .build()
}

/// A drawing whose DiaCraft part holds a note of another reader's beside the data, after the declaration.
fn old_drawing_with_a_note() -> Vec<u8> {
    VsdxBuilder::new()
        .page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1"))))
        .solution("DiaCraft", &format!("{OLD_DECLARATION}<SolutionXML Name=\"DiaCraft\">{DATA}<Note>another reader's</Note></SolutionXML>"))
        .build()
}

fn splice(doc: &Document, original: &[u8], baseline: &Document) -> Written {
    write_with(doc, Some(&OriginalBytes { bytes: original, baseline: Some(baseline) }), &options()).unwrap_or_else(|e| panic!("{e}"))
}

fn part_bytes(bytes: &[u8], name: &str) -> Vec<u8> {
    parts(bytes).remove(name).unwrap_or_else(|| panic!("no part {name}"))
}

/// The part starts at its root: no declaration, no byte order mark and no instruction anywhere in it.
#[track_caller]
fn assert_no_declaration(bytes: &[u8], what: &str) {
    let text = String::from_utf8_lossy(bytes);
    assert!(text.starts_with("<SolutionXML"), "{what}: the part does not start at its root: {text}");
    assert!(!text.contains("<?xml"), "{what}: the part holds a declaration: {text}");
}

fn with_snap(doc: &mut Document, attr: &str) {
    assert!(doc.set_diacraft_child("snap", Some(&[(attr, "1")])));
}

// ------------------------------------------------------------------------------------------------------------------------------ a part made new

#[test]
fn a_save_into_a_file_makes_the_part_with_no_declaration() {
    let original = drawing();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    with_snap(&mut doc, "glue");
    let written = splice(&doc, &original, &baseline);
    assert_no_declaration(&part_bytes(&written.bytes, SOLUTION), "splice");
    let (again, report) = read_ok(&written.bytes);
    assert_eq!(again.diacraft, doc.diacraft);
    assert!(matches!(report.diacraft, Some(DiacraftPart::Loaded { .. })), "{:?}", report.diacraft);
}

#[test]
fn a_drawing_made_new_gets_the_part_with_no_declaration() {
    let mut doc = Document::new_blank();
    with_snap(&mut doc, "dynamic_grid");
    let written = write_with(&doc, None, &options()).unwrap_or_else(|e| panic!("{e}"));
    assert_no_declaration(&part_bytes(&written.bytes, SOLUTION), "new drawing");
    assert_eq!(read_ok(&written.bytes).0.diacraft, doc.diacraft);
}

#[test]
fn the_model_writer_makes_the_part_with_no_declaration() {
    let (mut doc, _) = read_ok(&drawing());
    with_snap(&mut doc, "guides");
    let written = model_write(&doc, false);
    assert_no_declaration(&part_bytes(&written.bytes, SOLUTION), "model writer");
    assert_eq!(read_ok(&written.bytes).0.diacraft, doc.diacraft);
}

#[test]
fn the_data_of_the_other_format_rides_in_a_part_with_no_declaration() {
    // The seed of the carrier's tests: a model of data that VSDX cannot say, written by the model writer.
    let seed = crate::carry::carried_seed();
    assert_no_declaration(&part_bytes(&seed, SOLUTION), "model writer, carried data");
    let (doc, report) = read_ok(&seed);
    assert_eq!((report.carry.stale, report.carry.gone, report.carry.left_out), (0, 0, 0), "{:?}", report.carry);
    // A save into that file, after an edit of DiaCraft's own data, makes the part again with its items.
    let baseline = doc.clone();
    let mut doc = doc;
    with_snap(&mut doc, "glue");
    let saved = splice(&doc, &seed, &baseline);
    assert_no_declaration(&part_bytes(&saved.bytes, SOLUTION), "splice, carried data");
    let (back, report) = read_ok(&saved.bytes);
    assert_eq!((report.carry.stale, report.carry.gone, report.carry.left_out), (0, 0, 0), "{:?}", report.carry);
    assert_eq!(back.diacraft, doc.diacraft);
}

// ------------------------------------------------------------------------------------------------------------------------------ a part the file has

#[test]
fn a_save_into_a_file_whose_part_has_the_declaration_writes_it_without() {
    let original = old_drawing();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    with_snap(&mut doc, "guides");
    let written = splice(&doc, &original, &baseline);
    let bytes = part_bytes(&written.bytes, SOLUTION);
    assert_no_declaration(&bytes, "splice, a part with the declaration");
    assert!(String::from_utf8_lossy(&bytes).contains(r#"guides="1""#));
    assert_eq!(read_ok(&written.bytes).0.diacraft, doc.diacraft);
    // The part that has none is written with none the next time too: a save into it does not put a declaration back.
    let (mut second, _) = read_ok(&written.bytes);
    let baseline = second.clone();
    with_snap(&mut second, "snap_handles");
    let again = splice(&second, &written.bytes, &baseline);
    assert_no_declaration(&part_bytes(&again.bytes, SOLUTION), "splice, a part with none");
    assert_eq!(read_ok(&again.bytes).0.diacraft, second.diacraft);
}

#[test]
fn a_part_a_file_has_with_no_declaration_gets_none_when_it_is_patched() {
    let original = VsdxBuilder::new()
        .page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1"))))
        .solution("DiaCraft", &format!("<SolutionXML Name=\"DiaCraft\">{DATA}</SolutionXML>"))
        .build();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    with_snap(&mut doc, "guides");
    let written = splice(&doc, &original, &baseline);
    assert_no_declaration(&part_bytes(&written.bytes, SOLUTION), "splice, a part with none");
}

#[test]
fn a_part_that_holds_a_note_and_has_the_declaration_is_patched_without_it() {
    let original = old_drawing_with_a_note();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    with_snap(&mut doc, "guides");
    let written = splice(&doc, &original, &baseline);
    let bytes = part_bytes(&written.bytes, SOLUTION);
    assert_no_declaration(&bytes, "splice, a part with a note");
    assert!(String::from_utf8_lossy(&bytes).contains("<Note>another reader's</Note>"));
    assert_eq!(read_ok(&written.bytes).0.diacraft, doc.diacraft);
}

#[test]
fn the_model_writer_writes_a_part_the_file_has_with_the_declaration_without_it() {
    let (mut doc, _) = read_ok(&old_drawing());
    with_snap(&mut doc, "guides");
    let written = model_write(&doc, false);
    let bytes = part_bytes(&written.bytes, SOLUTION);
    assert_no_declaration(&bytes, "model writer, a part with the declaration");
    assert!(String::from_utf8_lossy(&bytes).contains(r#"guides="1""#));
    assert_eq!(read_ok(&written.bytes).0.diacraft, doc.diacraft);
}

#[test]
fn the_model_writer_writes_a_part_that_holds_a_note_and_has_the_declaration_without_it() {
    let (mut doc, _) = read_ok(&old_drawing_with_a_note());
    with_snap(&mut doc, "guides");
    let written = model_write(&doc, false);
    let bytes = part_bytes(&written.bytes, SOLUTION);
    assert_no_declaration(&bytes, "model writer, a part with a note");
    assert!(String::from_utf8_lossy(&bytes).contains("<Note>another reader's</Note>"));
    assert_eq!(read_ok(&written.bytes).0.diacraft, doc.diacraft);
}

#[test]
fn what_else_the_prolog_of_a_patched_part_holds_stays() {
    // A byte order mark and a comment before the root are another writer's: only the declaration goes.
    let part = format!("\u{feff}{OLD_DECLARATION}<!-- mine --><SolutionXML Name=\"DiaCraft\">{DATA}<Note>kept</Note></SolutionXML>");
    let original = VsdxBuilder::new().page(PageSpec::new(1, "A")).solution("DiaCraft", &part).build();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    with_snap(&mut doc, "guides");
    let written = splice(&doc, &original, &baseline);
    let text = String::from_utf8(part_bytes(&written.bytes, SOLUTION)).unwrap();
    assert!(text.starts_with("\u{feff}<!-- mine --><SolutionXML"), "{text}");
    assert!(!text.contains("<?xml"), "{text}");
}

// ------------------------------------------------------------------------------------------------------------------------------ what is not changed

/// The name of the part of the `n`th solution of a package.
fn solution_part(n: usize) -> String {
    SOLUTION.replace("solution1", &format!("solution{n}"))
}

/// A drawing whose solutions index lists the solutions `rows` (a name and the text of its part), the first in the part `solution_part(1)`, the second in `solution_part(2)`.
fn solutions_package(rows: &[(&str, &str)]) -> Vec<u8> {
    let mut list = String::new();
    let mut extra = Pkg::new().rel(DOC, "rId60", &format!("{VIS}solutions"), "solutions/solutions.xml");
    for (at, (name, text)) in rows.iter().enumerate() {
        let n = at + 1;
        list.push_str(&format!("<Solution Name=\"{name}\"><Rel r:id=\"rId{n}\"/></Solution>"));
        extra =
            extra.part(&solution_part(n), CT_SOLUTION, *text).rel(INDEX, &format!("rId{n}"), &format!("{VIS}solution"), &format!("solution{n}.xml"));
    }
    let index = format!("<Solutions xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\">{list}</Solutions>");
    VsdxBuilder::new().page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1")))).with(extra.part(INDEX, CT_SOLUTIONS, index)).build()
}

/// A drawing whose one solution is another program's, in the part `other`.
fn two_solutions(other: &str) -> Vec<u8> {
    solutions_package(&[("OtherApp", other)])
}

#[test]
fn a_part_of_another_program_is_written_as_it_was_read_by_both_writers() {
    for other in [
        format!("{OLD_DECLARATION}<SolutionXML Name=\"OtherApp\"><Mine a='1'/></SolutionXML>"),
        "<?xml version='1.0'?>\n<SolutionXML Name=\"OtherApp\"><Mine/></SolutionXML>".to_owned(),
        "<SolutionXML Name=\"OtherApp\"><Mine/></SolutionXML>".to_owned(),
    ] {
        let original = two_solutions(&other);
        let (mut doc, _) = read_ok(&original);
        let baseline = doc.clone();
        with_snap(&mut doc, "glue");
        // The splice: the other's part, then DiaCraft's own part beside it.
        let spliced = splice(&doc, &original, &baseline);
        assert_eq!(part_bytes(&spliced.bytes, SOLUTION), other.as_bytes(), "splice: {other}");
        assert_no_declaration(&part_bytes(&spliced.bytes, &solution_part(2)), "splice, beside another program's");
        // The model writer.
        let model = model_write(&doc, false);
        assert_eq!(part_bytes(&model.bytes, SOLUTION), other.as_bytes(), "model writer: {other}");
        let ours: Vec<String> = parts(&model.bytes)
            .into_iter()
            .filter(|(n, b)| n.contains("/solutions/solution") && n != INDEX && b != other.as_bytes())
            .map(|(n, _)| n)
            .collect();
        assert_eq!(ours.len(), 1, "DiaCraft's own part is the only other: {ours:?}");
        assert_no_declaration(&part_bytes(&model.bytes, &ours[0]), "model writer, beside another program's");
        assert_eq!(read_ok(&model.bytes).0.diacraft, doc.diacraft);
    }
}

/// A drawing whose index lists DiaCraft's solution (part 1, the text `ours`) and another program's (part 2, the text `other`).
fn both_solutions(ours: &str, other: &str) -> Vec<u8> {
    solutions_package(&[("DiaCraft", ours), ("OtherApp", other)])
}

#[test]
fn beside_another_programs_part_the_one_of_a_build_before_this_one_is_written_without_the_declaration_and_the_other_is_not_touched() {
    let other = format!("{OLD_DECLARATION}<SolutionXML Name=\"OtherApp\"><Mine a='1'/></SolutionXML>");
    for ours in [
        format!("{OLD_DECLARATION}<SolutionXML Name=\"DiaCraft\">{DATA}</SolutionXML>"),
        format!("{OLD_DECLARATION}<SolutionXML Name=\"DiaCraft\">{DATA}<Note>another reader's</Note></SolutionXML>"),
    ] {
        let original = both_solutions(&ours, &other);
        let (mut doc, _) = read_ok(&original);
        let baseline = doc.clone();
        with_snap(&mut doc, "guides");
        let spliced = splice(&doc, &original, &baseline);
        let model = model_write(&doc, false);
        for (writer, bytes) in [("splice", &spliced.bytes), ("model writer", &model.bytes)] {
            assert_no_declaration(&part_bytes(bytes, SOLUTION), writer);
            assert_eq!(part_bytes(bytes, &solution_part(2)), other.as_bytes(), "{writer}: the other program's part is the file's bytes");
            let (again, _) = read_ok(bytes);
            assert_eq!(again.diacraft, doc.diacraft, "{writer}");
        }
    }
}

#[test]
fn a_save_with_no_edit_leaves_a_part_with_the_declaration_as_it_is() {
    let original = old_drawing();
    let (doc, _) = read_ok(&original);
    let baseline = doc.clone();
    let written = splice(&doc, &original, &baseline);
    assert_eq!(part_bytes(&written.bytes, SOLUTION), part_bytes(&original, SOLUTION), "an unedited save writes what it read");
}

// ------------------------------------------------------------------------------------------------------------------------------ a part of the builds before this one

/// What a save that writes the file again makes of a part the builds before this one wrote: the part, with its declaration and the line end after it cut, and nothing else changed.
fn without_the_old_declaration(part: &[u8]) -> Vec<u8> {
    part.strip_prefix(OLD_DECLARATION.as_bytes()).unwrap_or_else(|| panic!("the part has no declaration: {}", String::from_utf8_lossy(part))).to_vec()
}

#[test]
fn an_edit_of_a_shape_writes_the_part_of_a_build_before_this_one_without_the_declaration_though_the_data_is_the_same() {
    for (what, original) in [("the data alone", old_drawing()), ("with a note beside it", old_drawing_with_a_note())] {
        let (mut doc, _) = read_ok(&original);
        let baseline = doc.clone();
        page_cell(&mut doc, 1, 0, "PinX", "2");
        assert_eq!(doc.diacraft, baseline.diacraft, "{what}: the edit is not of DiaCraft's data");
        let written = splice(&doc, &original, &baseline);
        let after = part_bytes(&written.bytes, SOLUTION);
        assert_no_declaration(&after, what);
        assert_eq!(
            after,
            without_the_old_declaration(&part_bytes(&original, SOLUTION)),
            "{what}: the declaration is cut, and nothing else of the part changes"
        );
        assert_eq!(read_ok(&written.bytes).0.diacraft, doc.diacraft, "{what}");
        // A second save, of another edit, has nothing left to cut and writes the part as it is.
        let (mut again, _) = read_ok(&written.bytes);
        let baseline = again.clone();
        page_cell(&mut again, 1, 0, "PinX", "3");
        assert_eq!(part_bytes(&splice(&again, &written.bytes, &baseline).bytes, SOLUTION), after, "{what}");
    }
}

#[test]
fn a_save_as_writes_the_part_of_a_build_before_this_one_without_the_declaration_though_nothing_was_edited() {
    let original = old_drawing();
    let (doc, _) = read_ok(&original);
    let baseline = doc.clone();
    let named = WriteOptions { variant: Some(Variant { kind: Kind::Drawing, macro_enabled: false }), ..options() };
    let written = write_with(&doc, Some(&OriginalBytes { bytes: &original, baseline: Some(&baseline) }), &named).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(part_bytes(&written.bytes, SOLUTION), without_the_old_declaration(&part_bytes(&original, SOLUTION)));
    assert_eq!(read_ok(&written.bytes).0.diacraft, doc.diacraft);
}

#[test]
fn what_stands_before_the_root_of_a_part_of_a_build_before_this_one_stays_when_a_save_cuts_its_declaration() {
    let part = format!("\u{feff}{OLD_DECLARATION}<!-- mine --><SolutionXML Name=\"DiaCraft\">{DATA}</SolutionXML>");
    let original = VsdxBuilder::new().page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1")))).solution("DiaCraft", &part).build();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    page_cell(&mut doc, 1, 0, "PinX", "2");
    let written = splice(&doc, &original, &baseline);
    let text = String::from_utf8(part_bytes(&written.bytes, SOLUTION)).unwrap();
    assert_eq!(text, format!("\u{feff}<!-- mine --><SolutionXML Name=\"DiaCraft\">{DATA}</SolutionXML>"));
}

#[test]
fn beside_another_programs_part_an_edit_cuts_the_declaration_of_diacraft_s_part_only() {
    let other = format!("{OLD_DECLARATION}<SolutionXML Name=\"OtherApp\"><Mine a='1'/></SolutionXML>");
    let ours = format!("{OLD_DECLARATION}<SolutionXML Name=\"DiaCraft\">{DATA}</SolutionXML>");
    let original = both_solutions(&ours, &other);
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    page_cell(&mut doc, 1, 0, "PinX", "2");
    let written = splice(&doc, &original, &baseline);
    assert_eq!(part_bytes(&written.bytes, SOLUTION), without_the_old_declaration(ours.as_bytes()));
    assert_eq!(part_bytes(&written.bytes, &solution_part(2)), other.as_bytes(), "the other program's part is the file's bytes");
}

#[test]
fn a_part_that_is_not_diacraft_s_or_does_not_read_is_left_as_it_is_by_an_edit() {
    for (what, text) in [
        ("another name", format!("{OLD_DECLARATION}<SolutionXML Name=\"OtherApp\">{DATA}</SolutionXML>")),
        ("another root", format!("{OLD_DECLARATION}<Something Name=\"DiaCraft\">{DATA}</Something>")),
        ("not XML", format!("{OLD_DECLARATION}<SolutionXML Name=\"DiaCraft\"><open>")),
        ("a declaration that never closes", "<?xml version=\"1.0\"".to_owned()),
    ] {
        let original = solutions_package(&[("DiaCraft", &text)]);
        let (mut doc, _) = read_ok(&original);
        let baseline = doc.clone();
        page_cell(&mut doc, 1, 0, "PinX", "2");
        let written = splice(&doc, &original, &baseline);
        assert_eq!(part_bytes(&written.bytes, SOLUTION), text.as_bytes(), "{what}");
    }
}

// ------------------------------------------------------------------------------------------------------------------------------ the reader

#[test]
fn the_reader_reads_the_part_with_the_declaration_and_without_it() {
    let with = read_ok(&old_drawing());
    let without = read_ok(
        &VsdxBuilder::new()
            .page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1"))))
            .solution("DiaCraft", &format!("<SolutionXML Name=\"DiaCraft\">{DATA}</SolutionXML>"))
            .build(),
    );
    assert!(with.0.diacraft.is_some(), "{:?}", with.1.diacraft);
    assert_eq!(with.0.diacraft, without.0.diacraft);
    assert_eq!(with.1.diacraft, without.1.diacraft);
    assert!(matches!(with.1.diacraft, Some(DiacraftPart::Loaded { children: 1, .. })), "{:?}", with.1.diacraft);
    // With a byte order mark and the declaration in single quotes, as another writer may have put them.
    let odd = VsdxBuilder::new()
        .page(PageSpec::new(1, "A"))
        .solution("DiaCraft", &format!("\u{feff}<?xml version='1.0' encoding='utf-8'?>\n<SolutionXML Name=\"DiaCraft\">{DATA}</SolutionXML>"))
        .build();
    assert_eq!(read_ok(&odd).0.diacraft, without.0.diacraft);
}

// ------------------------------------------------------------------------------------------------------------------------------ a Save As keeps the data

fn items_of(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let pkg = Package::read(bytes, &OpcLimits::default()).unwrap();
    let mut out = vec![("[Content_Types].xml".to_owned(), pkg.content_types_bytes().to_vec())];
    out.extend(pkg.parts().map(|p| (p.trim_start_matches('/').to_owned(), pkg.part(p).unwrap().to_vec())));
    out
}

#[test]
fn the_data_comes_back_through_a_file_of_the_builds_before_this_one() {
    // The seed with its solution part written the way 0.2.3 wrote it: the declaration first.
    let seed = crate::carry::carried_seed();
    let items: Vec<ZipItem> = items_of(&seed)
        .into_iter()
        .map(|(name, mut bytes)| {
            if format!("/{name}") == SOLUTION {
                // From the root on, whatever the writer put before it, then the declaration of the builds before this one.
                let root = String::from_utf8_lossy(&bytes).find("<SolutionXML").unwrap();
                let mut old = OLD_DECLARATION.as_bytes().to_vec();
                old.extend_from_slice(&bytes[root..]);
                bytes = old;
            }
            ZipItem::deflated(&name, &bytes)
        })
        .collect();
    let old = zip(&items);
    assert!(part_bytes(&old, SOLUTION).starts_with(b"<?xml"));
    let (from_old, old_report) = read_ok(&old);
    let (from_new, new_report) = read_ok(&seed);
    assert_eq!((old_report.carry.stale, old_report.carry.gone, old_report.carry.left_out), (0, 0, 0), "{:?}", old_report.carry);
    assert_eq!(old_report.carry, new_report.carry, "{:?} / {:?} / {:?}", old_report.diacraft, new_report.diacraft, old_report.warnings);
    assert_eq!(from_old.diacraft, from_new.diacraft);
    // Written from the model alone, the old file's data is in a part with no declaration, and is the same.
    let saved: BTreeMap<String, Vec<u8>> = parts(&model_write_as(&from_old, false, macro_drawing()).bytes);
    assert_no_declaration(saved.get(SOLUTION).unwrap(), "model writer, from a file with the declaration");
    assert_eq!(saved.get(SOLUTION), parts(&seed).get(SOLUTION), "the part is what the model writer made of the same data");
}
