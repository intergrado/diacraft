//! A Save As to a VSDX type writes DiaCraft's solution part with **no XML declaration**, whatever format the drawing came from, and the data in it comes back through the file and through a
//! `.diacraft` file and an `.odg` (the check of the files in the VSDX application: it logs an invalid parameter for a part that starts with a declaration, and drops it when it saves).
//!
//! The writers' own bytes are tested in `crates/vsdx/tests/it/solution_prolog.rs`; these go through a session, as a person saves: Save As from a drawing made new, from a `.diacraft` file, from an
//! `.odg`, and a save into a file written by a build before 0.2.4, whose part has the declaration. Every file is built in code.

use diacraft_engine::Session;
use diacraft_opc::{Limits, Package};
use diacraft_testkit::package::{PageSpec, VsdxBuilder};
use serde_json::{Value, json};

use crate::native_common::{Dir, pinned};
use crate::odg_build::{Drawing, rect};
use crate::run;

const SOLUTION: &str = "/visio/solutions/solution1.xml";

/// The solution part of the package, which must have one.
fn solution(bytes: &[u8]) -> String {
    let pkg = Package::read(bytes, &Limits::default()).unwrap();
    String::from_utf8(pkg.part(SOLUTION).unwrap_or_else(|| panic!("no solution part: {:?}", pkg.parts().collect::<Vec<_>>())).to_vec()).unwrap()
}

#[track_caller]
fn assert_starts_at_the_root(part: &str, what: &str) {
    assert!(part.starts_with("<SolutionXML") && !part.contains("<?xml"), "{what}: the part does not start at its root: {part}");
}

fn open(dir: &Dir, name: &str) -> Session {
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.path(name) }));
    s
}

fn save_as(s: &mut Session, dir: &Dir, name: &str) {
    run(s, "file.save_as", json!({ "path": dir.path(name) }));
}

/// The strengths and switches a session holds, which a file keeps in DiaCraft's data.
fn settings(s: &mut Session) -> Value {
    run(s, "snap.settings", json!({}))
}

/// Settings that are not the defaults, so that every format has data of DiaCraft's own to write.
fn change_settings(s: &mut Session) -> Value {
    run(s, "snap.set", json!({"strength_grid": 9, "strength_extensions": 100, "grid": false}));
    settings(s)
}

#[test]
fn a_save_as_from_a_new_drawing_writes_the_part_with_no_declaration() {
    let dir = Dir::new();
    let mut s = pinned();
    let want = change_settings(&mut s);
    save_as(&mut s, &dir, "a.vsdx");
    assert_starts_at_the_root(&solution(&dir.read("a.vsdx")), "a new drawing");
    assert_eq!(settings(&mut open(&dir, "a.vsdx")), want);
}

#[test]
fn a_save_as_from_a_diacraft_file_writes_the_part_with_no_declaration_and_the_data_comes_back() {
    let dir = Dir::new();
    let mut s = pinned();
    let want = change_settings(&mut s);
    save_as(&mut s, &dir, "a.diacraft");
    let mut t = open(&dir, "a.diacraft");
    assert_eq!(settings(&mut t), want, "the native file kept the data");
    save_as(&mut t, &dir, "b.vsdx");
    assert_starts_at_the_root(&solution(&dir.read("b.vsdx")), "from a .diacraft file");
    let mut u = open(&dir, "b.vsdx");
    assert_eq!(settings(&mut u), want, "the VSDX file kept it");
    // And back again, through a `.diacraft` file once more.
    save_as(&mut u, &dir, "c.diacraft");
    let mut v = open(&dir, "c.diacraft");
    save_as(&mut v, &dir, "d.vsdx");
    assert_starts_at_the_root(&solution(&dir.read("d.vsdx")), "a second trip");
    assert_eq!(settings(&mut open(&dir, "d.vsdx")), want);
}

#[test]
fn a_save_as_from_an_odg_writes_the_part_with_no_declaration_and_the_data_comes_back() {
    let dir = Dir::new();
    let odg = Drawing::new().page(&format!("{}{}", rect("R", (2.0, 2.0, 4.0, 2.0), "hello"), rect("S", (8.0, 2.0, 4.0, 2.0), ""))).bytes();
    dir.write("a.odg", &odg);
    let mut s = open(&dir, "a.odg");
    let want = change_settings(&mut s);
    save_as(&mut s, &dir, "b.vsdx");
    assert_starts_at_the_root(&solution(&dir.read("b.vsdx")), "from an .odg");
    let mut t = open(&dir, "b.vsdx");
    assert_eq!(settings(&mut t), want);
    // The `.odg` of the VSDX file's drawing and back to a VSDX file keeps both the shapes and the data.
    save_as(&mut t, &dir, "c.odg");
    let mut u = open(&dir, "c.odg");
    assert_eq!(settings(&mut u), want, "the .odg kept the data");
    save_as(&mut u, &dir, "d.vsdx");
    assert_starts_at_the_root(&solution(&dir.read("d.vsdx")), "an .odg trip");
    assert_eq!(settings(&mut open(&dir, "d.vsdx")), want);
}

/// The part of a build before this one: the declaration, and one setting that is not a default.
const OLD_PART: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n",
    r#"<SolutionXML Name="DiaCraft"><dc:Data xmlns:dc="urn:app.diacraft:extension" dc:version="1"><dc:snap strength_grid="40"/></dc:Data></SolutionXML>"#
);

/// A file of a build before this one: its part has the declaration.
fn old_file(dir: &Dir, name: &str) {
    dir.write(name, &VsdxBuilder::new().page(PageSpec::new(1, "A")).solution("DiaCraft", OLD_PART).build());
}

#[test]
fn a_save_after_an_edit_of_something_else_takes_the_declaration_out_of_the_part_of_a_build_before_this_one() {
    let dir = Dir::new();
    old_file(&dir, "old.vsdx");
    let mut s = open(&dir, "old.vsdx");
    assert_eq!(settings(&mut s)["strength_grid"], json!(40), "a part with the declaration is read");
    // The edit is of the page's name, and DiaCraft's data is as it was.
    run(&mut s, "page.rename", json!({"id": 1, "name": "Renamed"}));
    run(&mut s, "file.save", json!({}));
    let part = solution(&dir.read("old.vsdx"));
    assert_starts_at_the_root(&part, "a save into the file");
    assert_eq!(part, OLD_PART.split_once("\r\n").unwrap().1, "the declaration is cut, and nothing else of the part");
    assert_eq!(settings(&mut open(&dir, "old.vsdx"))["strength_grid"], json!(40), "the data is kept");
    // A change of the data writes the part again, with none, and the part is as it was.
    run(&mut s, "snap.set", json!({"strength_ruler": 1}));
    run(&mut s, "file.save", json!({}));
    let part = solution(&dir.read("old.vsdx"));
    assert_starts_at_the_root(&part, "a save of the data");
    assert!(part.contains(r#"strength_grid="40""#) && part.contains(r#"strength_ruler="1""#), "{part}");
    let r = settings(&mut open(&dir, "old.vsdx"));
    assert_eq!((&r["strength_grid"], &r["strength_ruler"]), (&json!(40), &json!(1)));
}

#[test]
fn a_save_as_of_a_file_of_a_build_before_this_one_writes_the_part_without_the_declaration_and_leaves_the_file_alone() {
    let dir = Dir::new();
    old_file(&dir, "old.vsdx");
    let mut s = open(&dir, "old.vsdx");
    save_as(&mut s, &dir, "copy.vsdx");
    assert_starts_at_the_root(&solution(&dir.read("copy.vsdx")), "a copy");
    assert_eq!(settings(&mut open(&dir, "copy.vsdx"))["strength_grid"], json!(40), "the copy has the data");
    assert_eq!(solution(&dir.read("old.vsdx")), OLD_PART, "the file the copy was made from is as it was");
    // The same drawing saved as another VSDX type.
    let mut t = open(&dir, "old.vsdx");
    save_as(&mut t, &dir, "copy.vsdm");
    assert_starts_at_the_root(&solution(&dir.read("copy.vsdm")), "a copy of another type");
}

#[test]
fn a_save_with_no_edit_leaves_the_part_of_a_build_before_this_one_as_it_is() {
    let dir = Dir::new();
    old_file(&dir, "old.vsdx");
    let mut s = open(&dir, "old.vsdx");
    run(&mut s, "file.save", json!({}));
    assert_eq!(solution(&dir.read("old.vsdx")), OLD_PART, "an unedited save writes what it read");
}
