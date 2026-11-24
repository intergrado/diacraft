//! `diacraft-cli roundtrip`: the binary, run over packages built in code, and the one line it prints.

use std::collections::BTreeSet;

use diacraft_opc::{Limits, Package};
use diacraft_testkit::package::{MasterSpec, PageSpec, Pkg, VsdxBuilder, ZipItem, cell, esc, shape, zip};
use diacraft_vsdx::part_label;
use proptest::prelude::*;
use serde_json::{Value, json};

use crate::{TempDir, cli, json_lines, s, text};

const VBA_REL: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProject";

/// A drawing with two shapes, a master, and a part the model does not hold. The names are ones a person made up.
fn drawing(document_type: &str) -> Vec<u8> {
    let first = shape(5, &format!("{}{}{}", cell("PinX", "1"), cell("PinY", "2"), cell("Width", "1")));
    let second = shape(6, &cell("PinX", "4"));
    VsdxBuilder::new()
        .document_type(document_type)
        .page(PageSpec::new(1, "Secret page name").shapes(&(first + &second)))
        .master(MasterSpec::new(1, "Secret master name").shapes(&shape(4, &cell("Width", "1"))))
        .with(Pkg::new().part_without_override("/foo/Secret name.dat", b"0123456789").default_type("dat", "application/x-foo"))
        .build()
}

fn roundtrip(args: &[&str]) -> (Value, std::process::Output) {
    let mut line = vec!["roundtrip"];
    line.extend_from_slice(args);
    let out = cli(&line, None);
    let report = json_lines(&out.stdout).into_iter().next().unwrap_or(Value::Null);
    (report, out)
}

#[test]
fn an_unchanged_round_trip_says_every_part_is_the_same_but_the_writers_name_and_the_model_reads_back_equal() {
    let dir = TempDir::new("rt");
    let input = dir.write("in.vsdx", &drawing("application/vnd.ms-visio.drawing.main+xml"));
    let output = dir.path("out.vsdx");
    let (report, out) = roundtrip(&[s(&input), "--out", s(&output)]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(report["opened"], true);
    assert_eq!(report["saved"], true);
    assert_eq!(report["variant"], "vsdx");
    assert_eq!(report["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(report["parts"]["changed"], json!(["/docProps/app.xml"]), "{report}");
    assert_eq!(report["parts"]["added_count"], 0);
    assert_eq!(report["parts"]["removed_count"], 0);
    assert_eq!(report["parts"]["content_types"]["equal"], true);
    assert_eq!(report["parts"]["relationships"]["equal"], true);
    assert_eq!(report["model"]["equal"], true, "{report}");
    assert_eq!(report["changed"]["shape_count"], 0);
    assert!(report.get("moved").is_none());
    // The names a person made up are not in the line.
    let all = text(&out.stdout);
    assert!(!all.contains("Secret"), "{all}");
}

#[test]
fn a_move_changes_the_cells_of_the_one_shape_and_nothing_else_and_the_report_names_them() {
    let dir = TempDir::new("rt");
    let input = dir.write("in.vsdx", &drawing("application/vnd.ms-visio.drawing.main+xml"));
    let output = dir.path("out.vsdx");
    let (report, out) = roundtrip(&[s(&input), "--out", s(&output), "--move", "0.5,-0.25"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(report["moved"]["page"], 1);
    assert_eq!(report["moved"]["shape"], 5);
    assert_eq!(report["changed"]["shape_count"], 1, "{report}");
    let shape = &report["changed"]["shapes"][0];
    assert_eq!((shape["page"].clone(), shape["shape"].clone()), (json!(1), json!(5)));
    let mut cells: Vec<&str> = shape["cells"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
    cells.sort_unstable();
    assert_eq!(cells, ["PinX", "PinY"], "{report}");
    let changed: Vec<&str> = report["parts"]["changed"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
    assert!(changed.contains(&"/visio/pages/page1.xml") && changed.contains(&"/docProps/app.xml"), "{changed:?}");
    assert!(!changed.contains(&"/visio/masters/master1.xml"), "{changed:?}");
    assert_eq!(report["model"]["equal"], false, "the model differs by the move, as it should");
}

#[test]
fn a_template_round_trips_as_the_file_itself_and_a_save_as_another_type_says_both() {
    let dir = TempDir::new("rt");
    let input = dir.write("in.vstx", &drawing("application/vnd.ms-visio.template.main+xml"));
    let output = dir.path("out.vstx");
    let (report, _) = roundtrip(&[s(&input), "--out", s(&output)]);
    assert_eq!(report["variant"], "vstx");
    assert_eq!(report["save"]["variant"], "vstx");
    assert_eq!(report["parts"]["changed"], json!(["/docProps/app.xml"]));
    let other = dir.path("out.vsdx");
    let (report, _) = roundtrip(&[s(&input), "--out", s(&other)]);
    assert_eq!(report["save"]["variant"], "vsdx");
    assert_eq!(report["parts"]["content_types"]["override_rows_differing"], 2, "the document's type, once on each side: {report}");
}

#[test]
fn a_macro_enabled_file_saved_as_a_macro_free_type_is_a_report_with_the_error_and_no_file() {
    let dir = TempDir::new("rt");
    let bytes = VsdxBuilder::new()
        .document_type("application/vnd.ms-visio.drawing.macroEnabled.main+xml")
        .page(PageSpec::new(1, "A"))
        .with(Pkg::new().default_type("bin", "application/vnd.ms-office.vbaProject").part_without_override("/visio/vbaProject.bin", b"vba").rel(
            "/visio/document.xml",
            "rId50",
            VBA_REL,
            "vbaProject.bin",
        ))
        .build();
    let input = dir.write("in.vsdm", &bytes);
    let same = dir.path("out.vsdm");
    let (report, _) = roundtrip(&[s(&input), "--out", s(&same)]);
    assert_eq!(report["parts"]["identical"].as_u64().unwrap() + 1, report["parts"]["in"].as_u64().unwrap(), "{report}");
    let plain = dir.path("out.vsdx");
    let (report, out) = roundtrip(&[s(&input), "--out", s(&plain)]);
    assert!(out.status.success());
    assert_eq!(report["saved"], false);
    assert_eq!(report["save_error"], json!({ "error": "macros_not_allowed" }), "{report}");
    assert!(!plain.exists());
}

#[test]
fn a_file_that_will_not_open_is_a_report_with_a_class_and_the_status_is_zero() {
    let dir = TempDir::new("rt");
    let input = dir.write("junk.vsdx", b"this is not a package");
    let (report, out) = roundtrip(&[s(&input), "--out", s(&dir.path("out.vsdx"))]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(report["opened"], false);
    assert!(report["refusal"]["class"].as_str().is_some_and(|c| !c.is_empty()), "{report}");
    assert!(!dir.path("out.vsdx").exists());
}

#[test]
fn the_input_is_never_written_and_a_mistake_in_the_line_is_status_two() {
    let dir = TempDir::new("rt");
    let bytes = drawing("application/vnd.ms-visio.drawing.main+xml");
    let input = dir.write("in.vsdx", &bytes);
    let out = cli(&["roundtrip", s(&input), "--out", s(&input)], None);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("never writes its input"), "{}", text(&out.stderr));
    assert_eq!(std::fs::read(&input).unwrap(), bytes);
    for bad in [
        vec!["roundtrip"],
        vec!["roundtrip", "a.vsdx"],
        vec!["roundtrip", "a.vsdx", "--out", "b.vsdx", "--move", "1"],
        vec!["roundtrip", "a", "b", "--out", "c"],
    ] {
        let out = cli(&bad, None);
        assert_eq!(out.status.code(), Some(2), "{bad:?}: {}", text(&out.stderr));
    }
}

/// The package `bytes` with the part `from` named `to` (a name that holds words, in characters a part name may hold unescaped) and holding `with`,
/// in the relationships and the content types too.
fn renamed(bytes: &[u8], from: &str, to: &str, with: &[u8]) -> Vec<u8> {
    let pkg = Package::read(bytes, &Limits::default()).unwrap();
    let mut xml =
        String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">");
    for (extension, content_type) in &pkg.content_types().defaults {
        xml.push_str(&format!("<Default Extension=\"{extension}\" ContentType=\"{content_type}\"/>"));
    }
    for (part, content_type) in &pkg.content_types().overrides {
        let part = if part == from { to } else { part };
        xml.push_str(&format!("<Override PartName=\"{}\" ContentType=\"{content_type}\"/>", esc(part)));
    }
    xml.push_str("</Types>");
    let mut items = vec![ZipItem::deflated("[Content_Types].xml", xml.as_bytes())];
    for name in pkg.parts() {
        let content = pkg.part(name).unwrap();
        if name == from {
            items.push(ZipItem::deflated(to.trim_start_matches('/'), with));
        } else if name.ends_with(".rels") {
            let text = String::from_utf8(content.to_vec()).unwrap().replace(from.trim_start_matches('/'), &esc(to.trim_start_matches('/')));
            items.push(ZipItem::deflated(name.trim_start_matches('/'), text.as_bytes()));
        } else {
            items.push(ZipItem::deflated(name.trim_start_matches('/'), content));
        }
    }
    zip(&items)
}

#[test]
fn a_part_name_that_holds_words_is_never_in_the_report_whatever_the_save_says() {
    let dir = TempDir::new("rt");
    let words = "/docProps/Secret(story)~.xml";
    let built = drawing("application/vnd.ms-visio.drawing.main+xml");
    // The properties part is named by a person and is not XML: the file opens, and the save keeps the part and says so.
    let input = dir.write("in.vsdx", &renamed(&built, "/docProps/app.xml", words, b"<Properties><Application>Secret story"));
    let (report, out) = roundtrip(&[s(&input), "--out", s(&dir.path("out.vsdx")), "--move", "1,0"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(report["saved"], true, "{report}");
    let said: Vec<&str> = report["save"]["warnings"].as_array().unwrap().iter().map(|w| w.as_str().unwrap()).collect();
    assert_eq!(said, ["property_part_kept"], "the warning is there, by its class and not its words");
    let all = text(&out.stdout);
    assert!(!all.contains("Secret") && !all.contains("story"), "{all}");
    // The part with the words is among those the report counts, and not named.
    let parts = &report["parts"];
    assert_eq!(parts["identical"].as_u64().unwrap() + parts["changed_count"].as_u64().unwrap(), parts["in"].as_u64().unwrap(), "{parts}");
}

// ------------------------------------------------------------------------------------------------ names that hold one another, words, marks

/// Marks that make a part name one a person could have written (`part_label` calls such a part by its role), and that a properties part may be
/// named with and still be found by its relationship. Among them are the marks `escape_debug` changes (`'`, `"`, a zero width space) and the one
/// JSON escapes (`"`).
const MARKS: [char; 16] = ['~', '(', ')', ' ', '\'', '"', '&', ';', '=', '+', ',', '@', '$', '!', '\u{e9}', '\u{200b}'];

/// Every mark the reader let a part name hold when this was written, for parts the model does not hold (no relationship leads to them).
const MORE_MARKS: [char; 13] = ['%', '#', '[', ']', '{', '}', '^', '`', '|', '<', '>', '*', '?'];

/// The words the tool has of its own where a name could be: the roles `part_label` gives a part, the classes of warnings and of errors.
const OWN_WORDS: [&str; 22] = [
    "a part",
    "a property part",
    "other",
    "stencil_keeps_pages",
    "stencil_without_masters",
    "macro_project_dropped",
    "page_or_master_removed",
    "signature_no_longer_holds",
    "page_name_list_not_found",
    "property_part_kept",
    "parts_without_content_type",
    "unknown_command",
    "disabled",
    "bad_params",
    "failed",
    "cannot_open",
    "changed_on_disk",
    "macros_not_allowed",
    "internal",
    "not_found",
    "damaged",
    "too_big",
];

/// The package of a drawing with `extra` parts the model does not hold (each with an `Override` for its type when `typed`), and with its properties
/// part named `app` and holding XML or not.
fn named(extra: &[String], typed: bool, app: Option<(&str, bool)>) -> Vec<u8> {
    let mut pkg = Pkg::new();
    for (i, name) in extra.iter().enumerate() {
        let bytes = format!("part {i}");
        pkg = if typed { pkg.part(name, "application/x-foo", bytes) } else { pkg.part_without_override(name, bytes) };
    }
    let built = VsdxBuilder::new().page(PageSpec::new(1, "A")).with(pkg).build();
    let Some((name, xml)) = app else { return built };
    let original = Package::read(&built, &Limits::default()).unwrap().part("/docProps/app.xml").unwrap().to_vec();
    renamed(&built, "/docProps/app.xml", name, if xml { &original } else { b"<Properties><Application>x" })
}

/// The report of a round trip of `bytes` from a file with the extension `from` to one with `to`.
fn report_of(bytes: &[u8], from: &str, to: &str, shift: bool) -> Value {
    let dir = TempDir::new("rtn");
    let input = dir.write(&format!("in.{from}"), bytes);
    let output = dir.path(&format!("out.{to}"));
    let mut args = vec![s(&input), "--out", s(&output)];
    if shift {
        args.extend(["--move", "1,0"]);
    }
    let (report, out) = roundtrip(&args);
    assert!(out.status.success(), "{}", text(&out.stderr));
    report
}

/// A name a person could have written: `part_label` calls such a part by its role.
fn assert_hidden(names: &[String]) {
    for name in names {
        assert_ne!(part_label("", name).as_ref(), name, "{name:?} is package structure, and is told");
    }
}

/// Every string value and object key, both raw and JSON-escaped, without the hash. Only these text
/// fields can carry part-name text: booleans, counts and null are protocol values, not name fragments.
fn told(report: &Value) -> String {
    fn push_text(text: &str, into: &mut String) {
        into.push_str(text);
        into.push('\n');
        into.push_str(&serde_json::to_string(text).unwrap());
        into.push('\n');
    }
    fn strings(value: &Value, into: &mut String) {
        match value {
            Value::String(text) => push_text(text, into),
            Value::Array(items) => items.iter().for_each(|v| strings(v, into)),
            Value::Object(map) => {
                for (key, v) in map {
                    push_text(key, into);
                    strings(v, into);
                }
            }
            _ => {}
        }
    }
    let mut report = report.clone();
    if let Some(map) = report.as_object_mut() {
        map.remove("sha256");
    }
    let mut all = String::new();
    strings(&report, &mut all);
    all
}

/// Every run of three characters of the names, in the forms a message gives a name (as it is, escaped with `escape_debug`, without the leading
/// slash, and as JSON writes it): any piece of three characters or more of a name holds one of them.
fn fragments(names: &[String]) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for name in names {
        let json = serde_json::to_string(name).unwrap();
        let json = json.strip_prefix('"').and_then(|j| j.strip_suffix('"')).unwrap().to_owned();
        for form in [name.clone(), name.escape_debug().to_string(), name.trim_start_matches('/').to_owned(), json] {
            let chars: Vec<char> = form.chars().collect();
            found.extend(chars.windows(3).map(|w| w.iter().collect::<String>()));
        }
    }
    found
}

/// The pieces of three characters of the hidden names that are in the report `subject` and in no word of `control`, the report of the same package
/// without those parts (so that the report's own words and names of the structure are not taken for a piece of a name).
fn leaked(hidden: &[String], subject: &Value, control: &Value) -> Vec<String> {
    let (said, own) = (told(subject), told(control) + "\n" + &OWN_WORDS.join("\n"));
    fragments(hidden).into_iter().filter(|piece| !own.contains(piece.as_str()) && said.contains(piece.as_str())).collect()
}

fn assert_no_piece_of(hidden: &[String], subject: &Value, control: &Value) {
    let found = leaked(hidden, subject, control);
    assert!(found.is_empty(), "pieces of part names are in the report: {found:?}\n{subject}");
}

#[test]
fn non_string_report_scalars_are_not_part_name_fragments() {
    let names = ["/fal_~", "/false~", "/tru_~", "/nul_~", "/123~"].map(str::to_owned);
    let subject = json!({"equal": false, "count": 123, "optional": null});
    let control = json!({"equal": true, "count": 0, "optional": 0});
    assert_no_piece_of(&names, &subject, &control);
}

#[test]
fn boolean_words_leaked_as_text_or_keys_are_still_detected() {
    let names = ["/fal_~", "/false~"].map(str::to_owned);
    let control = json!({"equal": true});
    for piece in ["fal", "false"] {
        let subject = json!({"equal": true, "detail": piece});
        assert!(leaked(&names, &subject, &control).iter().any(|found| found == "fal"), "string value {piece:?} must be detected");
        let mut subject = json!({"equal": true});
        subject.as_object_mut().unwrap().insert(piece.to_owned(), Value::Bool(false));
        assert!(leaked(&names, &subject, &control).iter().any(|found| found == "fal"), "object key {piece:?} must be detected");
    }
}

#[test]
fn raw_and_escaped_part_names_are_detected_in_values_and_keys() {
    let name = "/qz\"\u{200b}vx".to_owned();
    let names = [name.clone()];
    for form in [name.clone(), name.escape_debug().to_string(), name.trim_start_matches('/').to_owned(), serde_json::to_string(&name).unwrap()] {
        let value = json!({"detail": form});
        assert!(!leaked(&names, &value, &json!({})).is_empty(), "a part name in a string is detected: {form:?}");
        let mut key = json!({});
        key.as_object_mut().unwrap().insert(form.clone(), Value::Null);
        assert!(!leaked(&names, &key, &json!({})).is_empty(), "a part name in a key is detected: {form:?}");
    }
}

#[test]
fn an_untyped_part_name_containing_fal_is_not_confused_with_a_false_report_flag() {
    // The minimized property-test failure: unknown untyped parts change a boolean content-types
    // comparison to false, while the control report contains only true. No name text was leaked.
    let names = ["/aAA~", "/fal_~/aAA~", "/fal_~/aAA~atEV"].map(str::to_owned);
    assert_hidden(&names);
    let subject = report_of(&named(&names, false, None), "vsdx", "vsdx", true);
    let control = report_of(&named(&[], false, None), "vsdx", "vsdx", true);
    assert_eq!(subject["parts"]["content_types"]["equal"], false);
    assert_eq!(control["parts"]["content_types"]["equal"], true);
    assert_no_piece_of(&names, &subject, &control);
}

fn warnings_of(report: &Value) -> Vec<&str> {
    report["save"]["warnings"].as_array().unwrap().iter().map(|w| w.as_str().unwrap()).collect()
}

fn listed<'a>(report: &'a Value, which: &str) -> Vec<&'a str> {
    report["parts"][which].as_array().unwrap().iter().map(|w| w.as_str().unwrap()).collect()
}

#[test]
fn part_names_that_hold_one_another_leave_no_piece_in_the_report() {
    // `/a`, `/a/b` and `/a/bcd` with a mark, so that they are names a person could have written: the first is in the second, and the second in the
    // third. (A package cannot hold `/a` and `/a/b`: one part's name is not the folder of another's. So the second is `/b/a~`, which holds `/a~`.)
    let family: Vec<String> = ["/a~", "/b/a~", "/b/a~cd"].iter().map(|n| (*n).to_owned()).collect();
    assert_hidden(&family);
    for (i, app) in family.iter().enumerate() {
        let others: Vec<String> = family.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, n)| n.clone()).collect();
        for (typed, xml, shift) in [(false, true, false), (true, false, true), (false, false, true)] {
            let subject = report_of(&named(&others, typed, Some((app, xml))), "vsdx", "vsdx", shift);
            let control = report_of(&named(&[], typed, None), "vsdx", "vsdx", shift);
            assert_eq!(subject["saved"], true, "{subject}");
            assert_no_piece_of(&family, &subject, &control);
            // The ways a name could have come into the report are in use: the properties part is changed (named by its role) or kept with a
            // warning, and the parts with no type are counted.
            assert_eq!(listed(&subject, "changed").contains(&"a part"), xml, "{subject}");
            assert_eq!(warnings_of(&subject).contains(&"property_part_kept"), !xml, "{subject}");
            assert_eq!(warnings_of(&subject).contains(&"parts_without_content_type"), !typed, "{subject}");
        }
    }
    // All three as parts the model does not hold.
    let (subject, control) =
        (report_of(&named(&family, false, None), "vsdx", "vsdx", true), report_of(&named(&[], false, None), "vsdx", "vsdx", true));
    assert_eq!(warnings_of(&subject), ["parts_without_content_type"], "{subject}");
    assert_no_piece_of(&family, &subject, &control);
}

#[test]
fn part_names_that_are_words_of_the_reports_own_text_change_nothing_in_it() {
    let words: Vec<String> = [
        "/part~",
        "/parts had no content type~",
        "/a part~",
        "/was kept as it was~",
        "/property_part_kept~",
        "/changed~",
        "/identical~",
        "/warnings~",
        "/saved~",
        "/stencil~",
    ]
    .iter()
    .map(|n| (*n).to_owned())
    .collect();
    assert_hidden(&words);
    for (xml, shift) in [(true, false), (false, true)] {
        let subject = report_of(&named(&words[1..], false, Some((&words[0], xml))), "vsdx", "vsdx", shift);
        let control = report_of(&named(&[], false, None), "vsdx", "vsdx", shift);
        assert_no_piece_of(&words, &subject, &control);
        // The words are the report's own: the same keys, and the warnings are the classes and nothing of the names.
        let keys = |v: &Value| v.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
        assert_eq!(keys(&subject), keys(&control));
        let expected: Vec<&str> = if xml { vec!["parts_without_content_type"] } else { vec!["property_part_kept", "parts_without_content_type"] };
        assert_eq!(warnings_of(&subject), expected, "{subject}");
        assert_eq!(listed(&subject, "added"), listed(&control, "added"));
        assert_eq!(listed(&subject, "removed"), listed(&control, "removed"));
    }
}

#[test]
fn part_names_with_marks_that_escape_debug_or_json_changes_leave_no_piece() {
    let names: Vec<String> = ["/q'z", "/q\"z", "/q\u{200b}z", "/w\"/q\"z", "/w\"/q\"z\u{200b}x"].iter().map(|n| (*n).to_owned()).collect();
    assert_hidden(&names);
    for name in &names {
        assert_ne!(name.escape_debug().to_string(), *name, "{name:?}: escape_debug leaves it as it is");
    }
    for (i, app) in names.iter().enumerate() {
        let others: Vec<String> = names.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, n)| n.clone()).collect();
        for xml in [true, false] {
            let subject = report_of(&named(&others, false, Some((app, xml))), "vsdx", "vsdx", true);
            let control = report_of(&named(&[], false, None), "vsdx", "vsdx", true);
            assert_eq!(subject["saved"], true, "{subject}");
            assert_no_piece_of(&names, &subject, &control);
            assert_eq!(listed(&subject, "changed").contains(&"a part"), xml, "{subject}");
        }
    }
}

#[test]
fn every_mark_a_part_name_may_hold_is_as_safe() {
    let names: Vec<String> = MARKS.iter().chain(MORE_MARKS.iter()).map(|c| format!("/qz{c}zq")).collect();
    assert_hidden(&names);
    for typed in [false, true] {
        let subject = report_of(&named(&names, typed, None), "vsdx", "vsdx", false);
        let control = report_of(&named(&[], typed, None), "vsdx", "vsdx", false);
        assert_eq!(subject["opened"], true, "{subject}");
        assert_eq!(subject["parts"]["in"].as_u64().unwrap(), control["parts"]["in"].as_u64().unwrap() + names.len() as u64, "{subject}");
        assert_no_piece_of(&names, &subject, &control);
    }
}

#[test]
fn macro_parts_named_one_inside_another_are_a_class_in_the_refusal_and_not_in_the_report_when_kept() {
    let (vba, more) = ("/visio/vba~", "/visio/vba~x");
    let bytes = VsdxBuilder::new()
        .document_type("application/vnd.ms-visio.drawing.macroEnabled.main+xml")
        .page(PageSpec::new(1, "A"))
        .with(Pkg::new().part(vba, "application/vnd.ms-office.vbaProject", b"vba").part(more, "application/vnd.ms-office.vbaProject", b"more").rel(
            "/visio/document.xml",
            "rId50",
            VBA_REL,
            "vba~",
        ))
        .build();
    let names = [vba.to_owned(), more.to_owned()];
    assert_hidden(&names);
    let control = report_of(
        &VsdxBuilder::new().document_type("application/vnd.ms-visio.drawing.macroEnabled.main+xml").page(PageSpec::new(1, "A")).build(),
        "vsdm",
        "vsdx",
        false,
    );
    let refused = report_of(&bytes, "vsdm", "vsdx", false);
    assert_eq!(refused["save_error"], json!({ "error": "macros_not_allowed" }), "{refused}");
    assert_no_piece_of(&names, &refused, &control);
    let kept = report_of(&bytes, "vsdm", "vsdm", false);
    assert_eq!(kept["saved"], true, "{kept}");
    assert_no_piece_of(&names, &kept, &report_of(&bytes_without(&bytes, &names), "vsdm", "vsdm", false));
}

/// The package `bytes` without the parts `names`: a control for a report.
fn bytes_without(bytes: &[u8], names: &[String]) -> Vec<u8> {
    let pkg = Package::read(bytes, &Limits::default()).unwrap();
    let mut items = vec![];
    for name in pkg.parts() {
        if !names.iter().any(|n| n == name) && name != "/[Content_Types].xml" {
            items.push(ZipItem::deflated(name.trim_start_matches('/'), pkg.part(name).unwrap()));
        }
    }
    let types = pkg.content_types();
    let mut xml = String::from("<?xml version=\"1.0\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">");
    for (extension, content_type) in &types.defaults {
        xml.push_str(&format!("<Default Extension=\"{}\" ContentType=\"{}\"/>", esc(extension), esc(content_type)));
    }
    for (part, content_type) in types.overrides.iter().filter(|(part, _)| !names.contains(part)) {
        xml.push_str(&format!("<Override PartName=\"{}\" ContentType=\"{}\"/>", esc(part), esc(content_type)));
    }
    xml.push_str("</Types>");
    items.insert(0, ZipItem::deflated("[Content_Types].xml", xml.as_bytes()));
    zip(&items)
}

#[test]
fn a_file_that_will_not_open_is_told_by_its_class_alone_whatever_its_parts_are_called() {
    let dir = TempDir::new("rt");
    // A package with parts named one inside another and no content types: refused, and the report says by what class and nothing else.
    let bytes = zip(&[ZipItem::deflated("a~/b~cd", b"x"), ZipItem::deflated("a~/b~", b"y"), ZipItem::deflated("a~", b"z")]);
    let input = dir.write("in.vsdx", &bytes);
    let (report, out) = roundtrip(&[s(&input), "--out", s(&dir.path("out.vsdx"))]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(report["opened"], false);
    let refusal = report["refusal"].as_object().unwrap();
    assert_eq!(refusal.keys().collect::<Vec<_>>(), ["class"], "{report}");
    let names = ["/a~/b~cd".to_owned(), "/a~/b~".to_owned(), "/a~".to_owned()];
    assert_no_piece_of(&names, &report, &json!({}));
}

/// Marks, tokens of letters around them, and three names that hold one another by construction: a name, the same in a folder, and the same with
/// more letters. (A package cannot hold a name and a name in a folder of that name, so the folder is another one, and holds a `_` that no first
/// segment of the first name does.)
fn family() -> impl Strategy<Value = Vec<String>> {
    let token = "[A-Za-z]{3,6}";
    let mark = proptest::sample::select(MARKS.to_vec());
    (token, mark.clone(), token, mark, token, any::<bool>()).prop_map(|(a, m, b, n, c, reversed)| {
        let (first, second) = (format!("/{a}{m}"), format!("/{b}_{n}/{a}{m}"));
        let third = format!("{second}{c}");
        if reversed { vec![third, second, first] } else { vec![first, second, third] }
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, max_shrink_iters: 32, failure_persistence: None, ..ProptestConfig::default() })]

    /// No piece of three characters or more that only the part names have is in the line, whatever the names hold of one another, of marks and of
    /// letters, whichever of them names the properties part (holding XML or not) and whether the parts have a type.
    #[test]
    fn no_piece_of_a_part_name_is_in_any_line_of_the_report(
        names in family(),
        typed in any::<bool>(),
        app in proptest::option::of((0usize..3, any::<bool>())),
        shift in any::<bool>(),
    ) {
        for name in &names {
            let label = part_label("", name);
            prop_assert_ne!(&*label, name.as_str());
        }
        let (named_app, extra): (Option<(&str, bool)>, Vec<String>) = match app {
            Some((i, xml)) => (
                names.get(i).map(|n| (n.as_str(), xml)),
                names.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, n)| n.clone()).collect(),
            ),
            None => (None, names.clone()),
        };
        let subject = report_of(&named(&extra, typed, named_app), "vsdx", "vsdx", shift);
        let control = report_of(&named(&[], typed, None), "vsdx", "vsdx", shift);
        let found = leaked(&names, &subject, &control);
        prop_assert!(found.is_empty(), "pieces of part names are in the report: {:?}\n{}", found, subject);
    }
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn a_round_trip_of_a_drawing_of_80000_shapes_holds_the_model_it_read_and_is_refused_in_words_where_the_second_read_would_not_fit_and_does_not_abort()
{
    // `roundtrip` reads the file, reads it again to open it in a session, and reads what it saved: three reads in one process, the first model kept. The second was given the whole
    // budget and the process, 3.4 GB, aborted under `ulimit -v 3000000`; it is given what is left, and refused in words.
    let dir = TempDir::new("rt-heavy");
    let input = dir.write("in.vsdx", &diacraft_testkit::package::typical_drawing(20, 4_000, None));
    let output = dir.path("out.vsdx");
    let script = format!("ulimit -v 3000000 && exec \"{}\" roundtrip \"{}\" --out \"{}\"", crate::BIN, input.display(), output.display());
    let out = std::process::Command::new("/bin/sh").arg("-c").arg(script).env_remove("RUST_LOG").output().unwrap();
    assert!(out.status.success(), "the round trip aborted under `ulimit -v 3000000`: {:?}\n{}", out.status, text(&out.stderr));
    let report = json_lines(&out.stdout).into_iter().next().unwrap_or(Value::Null);
    assert_eq!(report["opened"], false, "{report}");
    assert_eq!(report["refusal"]["class"], "engine", "{report}");
    assert_eq!(report["refusal"]["problem"], "out_of_memory", "refused for the memory the model that was read holds: {report}");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn a_round_trip_whose_read_of_what_it_saved_would_not_fit_beside_the_model_and_the_session_says_it_does_not_read_back_and_does_not_abort() {
    // 32,000 shapes: the first read, kept, and the session's read of the same file fit (1.0 GB of model each), and the read of what was saved, with the budget less the two, does not:
    // refused for memory and told as a model that does not read back, where it was given the whole budget and the process aborted.
    let dir = TempDir::new("rt-third");
    let input = dir.write("in.vsdx", &diacraft_testkit::package::typical_drawing(8, 4_000, None));
    let output = dir.path("out.vsdx");
    let script = format!("ulimit -v 3000000 && exec \"{}\" roundtrip \"{}\" --out \"{}\"", crate::BIN, input.display(), output.display());
    let out = std::process::Command::new("/bin/sh").arg("-c").arg(script).env_remove("RUST_LOG").output().unwrap();
    assert!(out.status.success(), "the round trip aborted under `ulimit -v 3000000`: {:?}\n{}", out.status, text(&out.stderr));
    let report = json_lines(&out.stdout).into_iter().next().unwrap_or(Value::Null);
    assert_eq!((report["opened"].clone(), report["saved"].clone()), (json!(true), json!(true)), "the first two reads and the save fit: {report}");
    assert_eq!(report["model"]["reads_back"], false, "{report}");
    assert_eq!(report["model"]["class"], "too_much_memory", "refused for the memory held beside it: {report}");
}
