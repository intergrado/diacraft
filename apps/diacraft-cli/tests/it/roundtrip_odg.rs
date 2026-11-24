//! `diacraft-cli roundtrip` of an OpenDocument drawing: hand-built packages, written by the container's writer from the ODF standard,
//! copied through the package writer and compared with themselves. Nothing of a file's content may be in the line: the names a person made
//! up here are `Secret`.

use diacraft_container::zip::{ArchiveWriter, Method};
use serde_json::{Value, json};

use crate::{TempDir, cli, json_lines, s, text};

const DRAWING: &str = "application/vnd.oasis.opendocument.graphics";
const TEMPLATE: &str = "application/vnd.oasis.opendocument.graphics-template";
const OFFICE: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
const MANIFEST: &str = "urn:oasis:names:tc:opendocument:xmlns:manifest:1.0";

fn roundtrip(args: &[&str]) -> (Value, std::process::Output) {
    let mut line = vec!["roundtrip"];
    line.extend_from_slice(args);
    let out = cli(&line, None);
    let report = json_lines(&out.stdout).into_iter().next().unwrap_or(Value::Null);
    (report, out)
}

/// A drawing package: `mimetype` first and stored, the four XML parts deflated, LibreOffice's folder, a picture whose name holds words, a
/// thumbnail, a sub-document folder with a space, and a manifest that lists them all, its root row saying `media`. `manifest_extra` goes
/// inside a row (`encryption-data`).
fn package(media: &str, manifest_extra: &str) -> Vec<u8> {
    package_of(media, manifest_extra, "<Secret/>", "Pictures/Secret plan.png")
}

/// The package of [`package`], with `body` in `office:drawing` and the picture named `picture`.
fn package_of(media: &str, manifest_extra: &str, body: &str, picture: &str) -> Vec<u8> {
    let part = |root: &str, body: &str| {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:{root} xmlns:office=\"{OFFICE}\" office:version=\"1.3\">{body}</office:{root}>\n"
        )
    };
    let entries: Vec<(&str, Vec<u8>)> = vec![
        ("content.xml", part("document-content", &format!("<office:body><office:drawing>{body}</office:drawing></office:body>")).into_bytes()),
        ("styles.xml", part("document-styles", "<office:styles/>").into_bytes()),
        ("meta.xml", part("document-meta", "<office:meta/>").into_bytes()),
        ("settings.xml", part("document-settings", "<office:settings/>").into_bytes()),
        ("Configurations2/", Vec::new()),
        (picture, b"\x89PNG Secret".to_vec()),
        ("Object 1/content.xml", part("document-content", "").into_bytes()),
        ("Thumbnails/thumbnail.png", b"\x89PNG".to_vec()),
    ];
    let mut manifest =
        format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<manifest:manifest xmlns:manifest=\"{MANIFEST}\" manifest:version=\"1.3\">\n");
    manifest.push_str(&format!(" <manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"{media}\"/>\n"));
    for (name, _) in &entries {
        let inner = if *name == "content.xml" { manifest_extra } else { "" };
        manifest.push_str(&format!(
            " <manifest:file-entry manifest:full-path=\"{name}\" manifest:media-type=\"text/xml\">{inner}</manifest:file-entry>\n"
        ));
    }
    manifest.push_str(" <manifest:file-entry manifest:full-path=\"Object 1/\" manifest:media-type=\"\"/>\n</manifest:manifest>\n");
    let mut w = ArchiveWriter::new();
    w.add("mimetype", media.as_bytes(), Method::Stored).unwrap();
    for (name, data) in &entries {
        if name.ends_with('/') {
            w.add_directory(name).unwrap();
        } else if name.ends_with(".png") {
            w.add(name, data, Method::Stored).unwrap();
        } else {
            w.add(name, data, Method::Deflated).unwrap();
        }
    }
    w.add("META-INF/manifest.xml", manifest.as_bytes(), Method::Deflated).unwrap();
    w.finish().unwrap()
}

#[test]
fn a_hand_built_odg_package_round_trips_with_every_part_byte_identical() {
    let dir = TempDir::new("rt-odg");
    let bytes = package(DRAWING, "");
    let input = dir.write("in.odg", &bytes);
    let output = dir.path("out.odg");
    let (report, out) = roundtrip(&[s(&input), "--out", s(&output)]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(
        (report["format"].clone(), report["variant"].clone(), report["opened"].clone()),
        (json!("odg"), json!("odg"), json!(true)),
        "{report}"
    );
    assert_eq!(report["saved"], true, "{report}");
    assert_eq!(report["parts_identical"], true, "{report}");
    assert_eq!(report["file_identical"], true, "the package writer copies every entry as it is: {report}");
    assert_eq!(report["bytes_dropped"], json!({ "count": 0, "reasons": {} }), "{report}");
    assert_eq!((report["headers_rewritten"].clone(), report["rows_reordered"].clone()), (json!(0), json!(false)), "{report}");
    assert_eq!(std::fs::read(&output).unwrap(), bytes);
    assert_eq!(report["parts"]["identical"], 9, "{report}");
    assert_eq!(report["parts"]["order_changed"], false);
    assert_eq!(report["element_diff"], json!({ "parts_changed": 0, "parts": [] }));
    assert_eq!(report["read"]["files"], 9);
    assert_eq!(report["read"]["folders"], 1);
    assert_eq!(report["read"]["mimetype_regular"], true);
    assert_eq!(report["read"]["warnings"], json!([]), "{report}");
    assert_eq!(report["saved_read"]["files"], 9);
    assert_eq!(report["sha256"].as_str().unwrap().len(), 64);
    let all = text(&out.stdout);
    assert!(!all.contains("Secret"), "{all}");
}

#[test]
fn a_template_round_trips_as_a_template_and_another_type_is_not_written() {
    let dir = TempDir::new("rt-odg");
    let input = dir.write("in.otg", &package(TEMPLATE, ""));
    let (report, _) = roundtrip(&[s(&input), "--out", s(&dir.path("out.otg"))]);
    assert_eq!((report["variant"].clone(), report["parts_identical"].clone()), (json!("otg"), json!(true)), "{report}");
    // Before the drawing's reader exists a drawing is saved only as itself, and a move cannot be made.
    let (report, out) = roundtrip(&[s(&input), "--out", s(&dir.path("out.odg"))]);
    assert!(out.status.success());
    assert_eq!(report["save_error"], json!({ "error": "other_type", "want": "otg" }), "{report}");
    assert!(!dir.path("out.odg").exists());
    let drawing = dir.write("d.odg", &package(DRAWING, ""));
    let (report, _) = roundtrip(&[s(&drawing), "--out", s(&dir.path("d2.odg")), "--move", "1,0"]);
    assert_eq!(report["moved"], json!({ "error": "not_supported" }));
    assert_eq!(report["parts_identical"], true, "the unedited round trip is still made: {report}");
}

#[test]
fn an_odg_that_will_not_open_is_a_report_with_its_class_and_no_file() {
    let dir = TempDir::new("rt-odg");
    for (name, bytes, class) in [
        ("locked.odg", package(DRAWING, "<manifest:encryption-data/>"), "encrypted_package"),
        ("slides.odg", package("application/vnd.oasis.opendocument.presentation", ""), "not_a_drawing"),
        ("junk.odg", b"Secret words, not a package".to_vec(), "not_zip"),
    ] {
        let input = dir.write(name, &bytes);
        let output = dir.path(&format!("out-{name}"));
        let (report, out) = roundtrip(&[s(&input), "--out", s(&output)]);
        assert!(out.status.success(), "{}", text(&out.stderr));
        assert_eq!(report["opened"], false, "{report}");
        assert_eq!(report["refusal"], json!({ "class": class }), "{name}: {report}");
        assert!(!output.exists());
        assert!(!text(&out.stdout).contains("Secret"));
    }
    // A package that says it is a drawing, whatever its name, goes the same way.
    let input = dir.write("drawing.bin", &package(DRAWING, ""));
    let (report, _) = roundtrip(&[s(&input), "--out", s(&dir.path("out.odg"))]);
    assert_eq!(report["format"], "odg", "{report}");
}

/// The CRC-32 of ZIP (APPNOTE 4.4.7), bit by bit.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// A stored entry written by hand from the PKWARE APPNOTE, for what the container's writer never makes: flag bit 3, local sizes and a
/// ZIP64 record in the local header, and bytes after the data.
#[derive(Clone)]
struct Raw {
    name: String,
    data: Vec<u8>,
    flags: u16,
    /// The local header's checksum and sizes, when they are not the row's.
    local_sizes: Option<(u32, u32, u32)>,
    local_extra: Vec<u8>,
    after: Vec<u8>,
}

impl Raw {
    fn stored(name: &str, data: &[u8]) -> Raw {
        Raw { name: name.to_owned(), data: data.to_vec(), flags: 0, local_sizes: None, local_extra: Vec::new(), after: Vec::new() }
    }
}

/// A data descriptor with its signature (APPNOTE 4.3.9): sizes of 8 bytes when `wide`, else 4.
fn descriptor(wide: bool, crc: u32, size: u32) -> Vec<u8> {
    let mut out = [0x0807_4B50u32.to_le_bytes(), crc.to_le_bytes()].concat();
    if wide {
        out.extend_from_slice(&[u64::from(size).to_le_bytes(), u64::from(size).to_le_bytes()].concat());
    } else {
        out.extend_from_slice(&[size.to_le_bytes(), size.to_le_bytes()].concat());
    }
    out
}

/// The archive of `entries` in order, every one stored, closed by a plain end record.
fn assemble(entries: &[Raw]) -> Vec<u8> {
    let (mut out, mut rows) = (Vec::new(), Vec::new());
    for e in entries {
        let offset = out.len() as u32;
        let (crc, len, name) = (crc32(&e.data), e.data.len() as u32, e.name.as_bytes());
        let (local_crc, local_compressed, local_size) = e.local_sizes.unwrap_or((crc, len, len));
        let external: u32 = if e.name.ends_with('/') { 0x10 } else { 0 };
        out.extend_from_slice(&0x0403_4B50u32.to_le_bytes());
        for field in [20u16, e.flags, 0, 0, 0x21] {
            out.extend_from_slice(&field.to_le_bytes());
        }
        for field in [local_crc, local_compressed, local_size] {
            out.extend_from_slice(&field.to_le_bytes());
        }
        out.extend_from_slice(&[(name.len() as u16).to_le_bytes(), (e.local_extra.len() as u16).to_le_bytes()].concat());
        out.extend_from_slice(&[name, &e.local_extra, &e.data, &e.after].concat());
        rows.extend_from_slice(&0x0201_4B50u32.to_le_bytes());
        for field in [20u16, 20, e.flags, 0, 0, 0x21] {
            rows.extend_from_slice(&field.to_le_bytes());
        }
        for field in [crc, len, len] {
            rows.extend_from_slice(&field.to_le_bytes());
        }
        for field in [name.len() as u16, 0, 0, 0, 0] {
            rows.extend_from_slice(&field.to_le_bytes());
        }
        rows.extend_from_slice(&[external.to_le_bytes(), offset.to_le_bytes()].concat());
        rows.extend_from_slice(name);
    }
    let (start, count) = (out.len() as u32, entries.len() as u16);
    out.extend_from_slice(&rows);
    out.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
    for field in [0u16, 0, count, count] {
        out.extend_from_slice(&field.to_le_bytes());
    }
    out.extend_from_slice(&[(rows.len() as u32).to_le_bytes(), start.to_le_bytes()].concat());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// A drawing package of stored entries, `mimetype` first, written by hand, with `extra` after it and a manifest that lists every entry.
fn raw_package(extra: Vec<Raw>) -> Vec<u8> {
    let part =
        |root: &str| format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:{root} xmlns:office=\"{OFFICE}\" office:version=\"1.3\"/>\n");
    let mut entries = vec![Raw::stored("mimetype", DRAWING.as_bytes())];
    entries.extend(extra);
    for (name, root) in [("content.xml", "document-content"), ("styles.xml", "document-styles"), ("meta.xml", "document-meta")] {
        entries.push(Raw::stored(name, part(root).as_bytes()));
    }
    let mut manifest = format!("<manifest:manifest xmlns:manifest=\"{MANIFEST}\" manifest:version=\"1.3\">");
    manifest.push_str(&format!("<manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"{DRAWING}\"/>"));
    for e in entries.iter().skip(1) {
        manifest.push_str(&format!("<manifest:file-entry manifest:full-path=\"{}\" manifest:media-type=\"\"/>", e.name));
    }
    manifest.push_str("</manifest:manifest>");
    entries.push(Raw::stored("META-INF/manifest.xml", manifest.as_bytes()));
    assemble(&entries)
}

/// How many entries a streaming reader reads, from local header to local header, never the directory (APPNOTE 4.3.7 to 4.3.9): each
/// stored entry as long as its local header says (or the ZIP64 record it leaves its sizes to: the uncompressed size, then the compressed),
/// then the descriptor flag bit 3 announces, with sizes of 8 bytes when the local header holds a ZIP64 record and of 4 when it does not,
/// stating what was read. `Err` with the entry, counted from 1, where it breaks.
fn stream_walk(bytes: &[u8]) -> Result<usize, usize> {
    let u16_at = |at: usize| bytes.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |at: usize| bytes.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let u64_at = |at: usize| bytes.get(at..at + 8).map(|b| u64::from_le_bytes(b.try_into().unwrap()));
    let (mut at, mut count) = (0usize, 0usize);
    loop {
        let broke = Err(count + 1);
        match bytes.get(at..at + 4) {
            Some(b"PK\x03\x04") => {}
            Some(b"PK\x01\x02") => return Ok(count),
            _ => return broke,
        }
        let (Some(flags), Some(compressed), Some(name_len), Some(extra_len)) = (u16_at(at + 6), u32_at(at + 18), u16_at(at + 26), u16_at(at + 28))
        else {
            return broke;
        };
        let extra_at = at + 30 + usize::from(name_len);
        let zip64 = (u16_at(extra_at), u16_at(extra_at + 2)) == (Some(1), Some(16)) && extra_len >= 20;
        let length = match (compressed, zip64) {
            (0xFFFF_FFFF, true) => u64_at(extra_at + 12).unwrap_or(u64::MAX),
            (0xFFFF_FFFF, false) => return broke,
            (n, _) => u64::from(n),
        } as usize;
        let data = extra_at + usize::from(extra_len);
        let Some(content) = data.checked_add(length).and_then(|end| bytes.get(data..end)) else { return broke };
        at = data + length;
        if flags & 0x0008 != 0 {
            at += if u32_at(at) == Some(0x0807_4B50) { 4 } else { 0 };
            let stated = if zip64 {
                (u32_at(at), u64_at(at + 4), u64_at(at + 12))
            } else {
                (u32_at(at), u32_at(at + 4).map(u64::from), u32_at(at + 8).map(u64::from))
            };
            if stated != (Some(crc32(content)), Some(length as u64), Some(length as u64)) {
                return broke;
            }
            at += if zip64 { 20 } else { 12 };
        }
        count += 1;
    }
}

#[test]
fn the_harness_vector_round_trips_byte_identical_and_streams() {
    // A folder entry with flag bit 3, local sizes 0xFFFFFFFF, a ZIP64 record (0, 0) in its local header and a 24-byte
    // descriptor. The copy keeps the descriptor's form, so it is the file, and a streaming reader reads every entry of it.
    let zip64 = [&1u16.to_le_bytes()[..], &16u16.to_le_bytes(), &[0u8; 16]].concat();
    let folder = |after: Vec<u8>| Raw {
        flags: 0x0008,
        local_sizes: Some((0, 0xFFFF_FFFF, 0xFFFF_FFFF)),
        local_extra: zip64.clone(),
        after,
        ..Raw::stored("Configurations2/", b"")
    };
    let bytes = raw_package(vec![folder(descriptor(true, 0, 0))]);
    assert_eq!(stream_walk(&bytes), Ok(6), "the file streams");
    let dir = TempDir::new("rt-odg");
    let input = dir.write("in.odg", &bytes);
    let output = dir.path("out.odg");
    let (report, out) = roundtrip(&[s(&input), "--out", s(&output)]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(report["file_identical"], true, "{report}");
    assert_eq!(report["bytes_dropped"], json!({ "count": 0, "reasons": {} }), "{report}");
    assert_eq!((report["headers_rewritten"].clone(), report["rows_reordered"].clone()), (json!(0), json!(false)), "{report}");
    assert_eq!(report["parts_identical"], true, "{report}");
    assert_eq!(report["parts"]["headers_changed_count"], 0, "{report}");
    assert_eq!(report["read"]["folders"], 1, "{report}");
    let copy = std::fs::read(&output).unwrap();
    assert_eq!(copy, bytes);
    assert_eq!(stream_walk(&copy), Ok(6), "a streaming reader reads the copy whole");
    // The walk is the one that caught the fault: the same file with the short descriptor the copy used to write breaks it at the folder's
    // next entry.
    assert_eq!(stream_walk(&raw_package(vec![folder(descriptor(false, 0, 0))])), Err(2));
}

#[test]
fn bytes_no_header_announces_are_dropped_and_the_report_says_how_many_and_why() {
    // As one producer writes: a 16-byte descriptor after each of four entries whose flags announce none. A streaming
    // reader breaks at the first; the copy is a well-formed archive without them, every part the same, and the line says why the file
    // is not the same file.
    let names = ["Pictures/Secret 1.png", "Pictures/Secret 2.png", "Thumbnails/thumbnail.png", "settings.xml"];
    let extra: Vec<Raw> = names
        .iter()
        .map(|name| {
            let data = format!("bytes of {name}").into_bytes();
            Raw { after: descriptor(false, crc32(&data), data.len() as u32), ..Raw::stored(name, &data) }
        })
        .collect();
    let bytes = raw_package(extra);
    assert_eq!(stream_walk(&bytes), Err(3), "the descriptor after the second entry");
    let dir = TempDir::new("rt-odg");
    let input = dir.write("in.odg", &bytes);
    let output = dir.path("out.odg");
    let (report, out) = roundtrip(&[s(&input), "--out", s(&output)]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(report["file_identical"], false, "{report}");
    assert_eq!(report["bytes_dropped"], json!({ "count": 64, "reasons": { "unannounced": 64 } }), "{report}");
    assert_eq!((report["headers_rewritten"].clone(), report["rows_reordered"].clone()), (json!(0), json!(false)), "{report}");
    assert_eq!(report["parts_identical"], true, "{report}");
    assert_eq!(report["parts"]["identical"], 9, "{report}");
    assert_eq!(report["parts"]["headers_changed_count"], 0, "{report}");
    assert_eq!(report["out_bytes"].as_u64().unwrap() + 64, bytes.len() as u64);
    assert_eq!(report["saved_read"]["files"], report["read"]["files"]);
    let copy = std::fs::read(&output).unwrap();
    assert_eq!(stream_walk(&copy), Ok(9), "the copy streams");
    assert!(!text(&out.stdout).contains("Secret"), "{}", text(&out.stdout));
    // A round trip of the copy is the same file.
    let again = dir.path("again.odg");
    let (report, _) = roundtrip(&[s(&output), "--out", s(&again)]);
    assert_eq!((report["file_identical"].clone(), report["bytes_dropped"]["count"].clone()), (json!(true), json!(0)), "{report}");
}

fn compare(first: &str, second: &str) -> (Value, std::process::Output) {
    let out = cli(&["compare", first, second], None);
    let report = json_lines(&out.stdout).into_iter().next().unwrap_or(Value::Null);
    (report, out)
}

/// A page holding `shapes`, in the drawing namespace.
fn page(shapes: &str) -> String {
    format!(
        "<draw:page xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" \
         xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\">{shapes}</draw:page>"
    )
}

#[test]
fn compare_names_a_planted_one_attribute_change() {
    // The harness plants one change and sees exactly it, by its path and attribute.
    let dir = TempDir::new("cmp-odg");
    let rects = |x: &str| page(&format!("<draw:rect svg:x=\"1cm\"/><draw:rect svg:x=\"{x}\"/>"));
    let first = dir.write("a.odg", &package_of(DRAWING, "", &rects("2cm"), "Pictures/image.png"));
    let second = dir.write("b.odg", &package_of(DRAWING, "", &rects("2.5cm"), "Pictures/image.png"));
    let (report, out) = compare(s(&first), s(&second));
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!((report["first"]["opened"].clone(), report["second"]["opened"].clone()), (json!(true), json!(true)), "{report}");
    assert_eq!(report["parts_identical"], false);
    assert_eq!(report["parts"]["identical"], 8, "every part but content.xml: {report}");
    let diff = &report["element_diff"];
    assert_eq!(diff["parts_changed"], 1, "{report}");
    let part = &diff["parts"][0];
    assert_eq!(part["part"], "content.xml");
    assert_eq!(part["counts"], json!({ "added": 0, "removed": 0, "changed": 1, "moved": 0, "whitespace": 0 }));
    assert_eq!(part["changed"][0]["path"], "/office:document-content/office:body/office:drawing/draw:page/draw:rect[2]");
    assert_eq!(part["changed"][0]["attributes"], json!(["svg:x"]));
    // The same file twice: nothing differs.
    let (report, _) = compare(s(&first), s(&first));
    assert_eq!((report["parts_identical"].clone(), report["element_diff"]["parts_changed"].clone()), (json!(true), json!(0)));
    assert_eq!(report["first"]["sha256"], report["second"]["sha256"]);
}

#[test]
fn compare_shows_no_name_a_person_chose() {
    // An element no recorded vocabulary holds, a picture's name, and a known element under a prefix of the file's own choosing.
    let dir = TempDir::new("cmp-odg");
    let first = dir.write("a.odg", &package_of(DRAWING, "", &page(""), "Pictures/MyPlan.png"));
    let planted = page("<draw:MyPassword/><d:rect xmlns:d=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\"/>");
    let second = dir.write("b.odg", &package_of(DRAWING, "", &planted, "Pictures/MyPlan.png"));
    let (report, out) = compare(s(&first), s(&second));
    let all = text(&out.stdout);
    assert!(!all.contains("MyPassword") && !all.contains("MyPlan"), "{all}");
    let added = &report["element_diff"]["parts"][0]["added"];
    let page_path = "/office:document-content/office:body/office:drawing/draw:page";
    assert_eq!(added, &json!([format!("{page_path}/(unlisted)"), format!("{page_path}/draw:rect")]), "{report}");
    // A picture that differs is told by its role.
    let other = dir.write("c.odg", &package_of(DRAWING, "", &page(""), "Pictures/MyOtherPlan.png"));
    let (report, out) = compare(s(&first), s(&other));
    assert!(!text(&out.stdout).contains("Plan"), "{}", text(&out.stdout));
    assert_eq!(report["parts"]["added"], json!(["picture"]));
    assert_eq!(report["parts"]["removed"], json!(["picture"]));
}

#[test]
fn compare_reports_a_file_that_will_not_open_and_refuses_a_wrong_line() {
    let dir = TempDir::new("cmp-odg");
    let good = dir.write("a.odg", &package(DRAWING, ""));
    let junk = dir.write("b.odg", b"Secret words, not a package");
    let (report, out) = compare(s(&good), s(&junk));
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(report["first"]["opened"], true);
    assert_eq!(report["second"]["refusal"], json!({ "class": "not_zip" }), "{report}");
    assert!(report.get("element_diff").is_none() && !text(&out.stdout).contains("Secret"));
    for bad in [vec!["compare"], vec!["compare", "a.odg"], vec!["compare", "a.odg", "b.odg", "c.odg"], vec!["compare", "a.odg", "--out", "b.odg"]] {
        assert_eq!(cli(&bad, None).status.code(), Some(2), "{bad:?}");
    }
    // A file that cannot be read from disk is a failure, named on standard error.
    let out = cli(&["compare", s(&good), s(&dir.path("missing.odg"))], None);
    assert_eq!(out.status.code(), Some(1));
}
