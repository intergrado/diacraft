//! A `.diacraft` file is recognised by its bytes before either other reader looks (the design's 1.2): `file.open` asks `diacraft_native::sniff` first,
//! then `diacraft_odg::sniff`, then the VSDX reader, and neither of the others can claim a `.diacraft` file.

use diacraft_engine::Session;
use serde_json::json;

use crate::native_common::{Dir, made, vsdx_drawing};
use crate::{blank_session, run};

/// The bytes of a `.diacraft` file a session wrote.
fn diacraft_bytes() -> Vec<u8> {
    let dir = Dir::new();
    let mut s = blank_session();
    made(&mut s);
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    dir.read("a.diacraft")
}

#[test]
fn a_diacraft_file_is_sniffed_first() {
    let bytes = diacraft_bytes();
    assert_eq!(diacraft_native::sniff(&bytes), Some(diacraft_native::Kind::Drawing));
    let dir = Dir::new();
    // Whatever the file is called: the bytes decide, and the extension gets a warning.
    let path = dir.write("x.vsdx", &bytes);
    let mut s = Session::new();
    let result = run(&mut s, "file.open", json!({ "path": path }));
    assert_eq!(result["file"]["variant"], "diacraft", "{result}");
    assert_eq!(result["file"]["kind"], "drawing");
    let warnings = result["file"]["warnings"].as_array().unwrap();
    assert!(
        warnings.iter().any(|w| w.as_str().is_some_and(|w| w.contains(".vsdx") && w.contains("DiaCraft"))),
        "the extension is told: {warnings:?}"
    );
    assert!(result["file"]["warning_count"].as_u64().unwrap() >= 1);
    assert_eq!(result["pages"].as_array().unwrap().len(), 2, "the drawing is the file's");
    // A name that is right says nothing.
    let right = dir.write("y.diacraft", &bytes);
    let mut t = Session::new();
    let result = run(&mut t, "file.open", json!({ "path": right }));
    assert_eq!(result["file"]["warning_count"], 0, "{result}");
    assert_eq!(result["file"]["warnings"], json!([]));
}

#[test]
fn the_odg_and_vsdx_readers_refuse_a_diacraft_file() {
    let bytes = diacraft_bytes();
    assert!(diacraft_odg::sniff(&bytes).is_none(), "no ODF type and no ODF manifest");
    let refused = diacraft_vsdx::read(&bytes, &diacraft_vsdx::Limits::default()).expect_err("the VSDX reader takes no .diacraft file");
    assert_eq!(refused.kind(), diacraft_vsdx::ErrorKind::NotAPackage, "a non-OPC class, not damage: {}", refused.class());
    // And the other way: a VSDX file is no `.diacraft` file, and goes to the VSDX reader as before.
    let vsdx = vsdx_drawing();
    assert!(diacraft_native::sniff(&vsdx).is_none());
    let dir = Dir::new();
    let mut s = Session::new();
    let result = run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &vsdx) }));
    assert_eq!(result["file"]["variant"], "vsdx");
}

#[test]
fn a_damaged_diacraft_file_is_refused_in_words_and_leaves_the_drawing_open() {
    let mut bytes = diacraft_bytes();
    let dir = Dir::new();
    let mut s = blank_session();
    run(&mut s, "page.add", json!({ "name": "Kept" }));
    let before = crate::snapshot(&s);
    // The first entry is intact, so the file is a `.diacraft` file; the rest of it is cut off.
    bytes.truncate(bytes.len() / 2);
    let path = dir.write("cut.diacraft", &bytes);
    let error = s.execute("file.open", &json!({ "path": path })).unwrap_err();
    let words = error.to_string();
    assert!(words.contains("damaged") || words.contains("cannot") || words.contains("not a drawing file"), "{words}");
    assert!(!words.contains(&path), "no path in the words: {words}");
    assert_eq!(crate::snapshot(&s), before, "a file that cannot be opened leaves the open drawing as it is");
}

#[test]
fn a_file_of_a_major_version_this_build_does_not_know_is_refused_naming_it() {
    use diacraft_container::zip::{Archive, ArchiveWriter, Method};
    let bytes = diacraft_bytes();
    // The same package with its manifest's version made 2.0.
    let mut archive = Archive::read(&bytes, &diacraft_container::Limits::default()).unwrap();
    let names: Vec<String> = archive.entries().iter().map(|e| e.name.clone()).collect();
    let mut w = ArchiveWriter::new();
    for (k, name) in names.iter().enumerate() {
        let data = archive.inflate(k, &mut diacraft_container::Work::default()).unwrap();
        let data = if name == "manifest.xml" {
            let text = String::from_utf8(data).unwrap();
            assert!(text.contains("format=\"1.1\""), "{text}");
            text.replace("format=\"1.1\"", "format=\"2.0\"").into_bytes()
        } else {
            data
        };
        w.add(name, &data, if name == "mimetype" { Method::Stored } else { Method::Deflated }).unwrap();
    }
    let dir = Dir::new();
    let path = dir.write("new.diacraft", &w.finish().unwrap());
    let error = Session::new().execute("file.open", &json!({ "path": path })).unwrap_err();
    assert!(error.to_string().contains("made by a newer DiaCraft (format 2.0)"), "{error}");
}
