//! APPID: `diacraft-cli roundtrip --via diacraft` and `compare --content` over files that a build made before the id moved to `diacraft.app` wrote (DiaCraft's namespace is
//! `urn:io.github.one_bit.diacraft:extension` in them): the file goes through a `.diacraft` file and back, the output is in the current namespace and names the old one nowhere, and
//! **the content check names nothing about the namespace**: what it says of a file of before is what it says of the same file with its namespace put in the current form (a file made
//! from a new drawing has a few differences of its own in two parts, which the build before the move gave as well; they are the same either way).
//!
//! The four files are the ones `main` at 3f696318 wrote (`diacraft_testkit::old_namespace`), not files this code built by hand.

use diacraft_container::zip::{Archive, ArchiveWriter, Method};
use diacraft_container::{Limits, Work};
use diacraft_testkit::old_namespace::{DRAWING_ODG, DRAWING_VSDX, ODG_AS_VSDX, OLD_NAMESPACE, VSDX_AS_ODG, file, parts};
use serde_json::Value;

use crate::{TempDir, cli, json_lines, s, text};

const CURRENT: &str = "urn:app.diacraft:extension";

/// The file `name` with the old namespace put in the current form in every part that holds it, written by the container's writer.
fn current(name: &str) -> Vec<u8> {
    let mut out = ArchiveWriter::new();
    for (entry, bytes) in parts(name) {
        let (method, bytes) = match String::from_utf8(bytes) {
            Ok(text) if entry == "mimetype" => (Method::Stored, text.into_bytes()),
            Ok(text) => (Method::Deflated, text.replace(OLD_NAMESPACE, CURRENT).into_bytes()),
            Err(bytes) => (Method::Deflated, bytes.into_bytes()),
        };
        out.add(&entry, &bytes, method).unwrap();
    }
    out.finish().unwrap()
}

/// Whether any part of the archive in `bytes` names the old namespace.
fn names_the_old_namespace(bytes: &[u8]) -> Vec<String> {
    let mut archive = Archive::read(bytes, &Limits::default()).unwrap();
    let names: Vec<String> = archive.entries().iter().map(|e| e.name.clone()).collect();
    names
        .into_iter()
        .enumerate()
        .filter(|(at, _)| String::from_utf8_lossy(&archive.inflate(*at, &mut Work::default()).unwrap()).contains("one_bit"))
        .map(|(_, name)| name)
        .collect()
}

/// The line `roundtrip --via diacraft` printed for `input`, with the file it wrote.
fn visit(dir: &TempDir, input: &[u8], extension: &str, tag: &str) -> (Value, Vec<u8>) {
    let (input_path, output) = (dir.write(&format!("{tag}-in.{extension}"), input), dir.path(&format!("{tag}-out.{extension}")));
    let out = cli(&["roundtrip", s(&input_path), "--out", s(&output), "--via", "diacraft", "--bundled-fonts"], None);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let line = json_lines(&out.stdout).into_iter().next().unwrap();
    assert_eq!(line["refusal"], Value::Null, "the trip finished: {line}");
    (line, std::fs::read(&output).unwrap())
}

/// What `compare --content` printed for the two files, as its `content` field.
fn compare(dir: &TempDir, first: (&[u8], &str), second: (&[u8], &str), tag: &str) -> Value {
    let (a, b) = (dir.write(&format!("{tag}-a.{}", first.1), first.0), dir.write(&format!("{tag}-b.{}", second.1), second.0));
    let out = cli(&["compare", "--content", s(&a), s(&b)], None);
    assert!(out.status.success(), "{}", text(&out.stderr));
    json_lines(&out.stdout).into_iter().next().unwrap()["content"].clone()
}

#[test]
fn the_trip_through_a_diacraft_file_of_a_file_of_the_old_namespace_says_what_it_says_of_the_same_file_in_the_current_one() {
    let dir = TempDir::new("old-namespace");
    for (name, extension) in [(DRAWING_VSDX, "vsdx"), (ODG_AS_VSDX, "vsdx"), (DRAWING_ODG, "odg"), (VSDX_AS_ODG, "odg")] {
        let (old_line, old_out) = visit(&dir, &file(name), extension, "old");
        let (new_line, new_out) = visit(&dir, &current(name), extension, "new");
        assert_eq!(old_line["opened"], true, "{name}: {old_line}");
        assert!(
            names_the_old_namespace(&old_out).is_empty(),
            "{name}: the output names the old namespace in {:?}",
            names_the_old_namespace(&old_out)
        );
        // The content check: what it found, and where, is the same whichever namespace the input was in.
        assert_eq!(old_line["content"], new_line["content"], "{name}: the namespace costs the content check nothing");
        assert_eq!(old_line["model"], new_line["model"], "{name}: nor the check of the model");
        assert_eq!(old_line["content"]["compared"], true, "{name}: {old_line}");
        // `compare --content` of the file against what came back: the same, with the source first.
        let (against_old, against_new) = (
            compare(&dir, (&file(name), extension), (&old_out, extension), "c-old"),
            compare(&dir, (&current(name), extension), (&new_out, extension), "c-new"),
        );
        assert_eq!(against_old, against_new, "{name}");
    }
}

#[test]
fn a_vsdx_file_of_the_old_namespace_comes_back_the_same_content_and_names_the_old_namespace_nowhere() {
    let dir = TempDir::new("old-namespace-vsdx");
    let (line, out) = visit(&dir, &file(DRAWING_VSDX), "vsdx", "plain");
    assert_eq!(line["content"]["same"], true, "{line}");
    assert_eq!(line["content"]["count"], 0, "{line}");
    assert_eq!(line["model"]["same"], true, "{line}");
    assert!(names_the_old_namespace(&out).is_empty());
    // Both ways round, and the old file against the current form of itself: nothing named.
    for (a, b) in [(file(DRAWING_VSDX), current(DRAWING_VSDX)), (current(DRAWING_VSDX), file(DRAWING_VSDX))] {
        let content = compare(&dir, (&a, "vsdx"), (&b, "vsdx"), "forms");
        assert_eq!((content["same"].clone(), content["count"].clone()), (Value::Bool(true), Value::from(0)), "{content}");
    }
}

#[test]
fn a_planted_difference_beside_the_namespace_is_still_named() {
    let dir = TempDir::new("old-namespace-planted");
    let mut changed = ArchiveWriter::new();
    for (entry, bytes) in parts(DRAWING_VSDX) {
        let text = String::from_utf8(bytes).ok();
        let bytes = match (&text, entry.ends_with("solution1.xml")) {
            (Some(t), true) => t.replacen("strength_grid=\"40\"", "strength_grid=\"41\"", 1).into_bytes(),
            (Some(t), false) => t.clone().into_bytes(),
            (None, _) => Vec::new(),
        };
        changed.add(&entry, &bytes, Method::Deflated).unwrap();
    }
    let content = compare(&dir, (&file(DRAWING_VSDX), "vsdx"), (&changed.finish().unwrap(), "vsdx"), "planted");
    assert_eq!(content["same"], false, "{content}");
    assert_eq!(content["count"], 1, "{content}");
}
