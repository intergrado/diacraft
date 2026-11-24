//! The package side of the engine's `.odg` path: signatures, the thumbnail, the formats a drawing is saved in, and the files that do not
//! open or changed on the disk.

// Only the test of a link makes a session of its own, and links are Unix's.
#[cfg(unix)]
use diacraft_engine::Session;
use diacraft_engine::{EngineError, OpenProblem};
use serde_json::{Value, json};

use crate::odg_build::{DRAWING, Dir, TEMPLATE, id_of, open_bytes_in, open_drawing, package, part_bytes, part_of};
use crate::odg_file::{assert_reopens_as_saved, assert_saves_again_as_no_edit, four, refused, save, shift};
use crate::{check_session, run, snapshot};

/// The signature of a signed drawing is kept as bytes by an edited save and no longer matches what it signed: said, as a signed
/// VSDX file's is. An unedited save says nothing.
#[test]
fn signed_odg_save_warns_and_keeps_signature_parts() {
    let dir = Dir::new();
    let signature = b"<?xml version=\"1.0\"?><signatures xmlns=\"urn:example:signatures\"><signature id=\"s1\">0123456789</signature></signatures>";
    let mut s = open_drawing(&dir, &four().file("META-INF/documentsignatures.xml", signature));
    let unedited = save(&mut s);
    assert_eq!(unedited["edited"], false);
    assert_eq!(unedited["warning_count"], 0, "an unedited save of a signed drawing says nothing: {unedited}");
    shift(&mut s, "A", 1.0, 0.0);
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true);
    let warnings: Vec<&str> = saved["warnings"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(warnings.iter().any(|w| w.contains("signed") && w.contains("signature no longer matches")), "{warnings:?}");
    let bytes = dir.read("a.odg");
    assert_eq!(part_bytes(&bytes, "META-INF/documentsignatures.xml").as_deref(), Some(&signature[..]), "the signature is kept byte for byte");
    assert!(part_of(&bytes, "META-INF/manifest.xml").contains("documentsignatures.xml"), "and still listed");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
}

/// A small PNG of one colour, for a thumbnail the file already has.
fn small_png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let pixels: Vec<u8> = rgba.iter().copied().cycle().take((width * height * 4) as usize).collect();
    encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
    out
}

/// A PNG decoded: its size and the pixels, row by row.
fn decode(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes.to_vec())).read_info().expect("a PNG");
    let mut pixels = vec![0u8; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).expect("a frame");
    assert_eq!(info.color_type, png::ColorType::Rgba, "the thumbnail is RGBA");
    (info.width, info.height, pixels)
}

/// An edited save writes a picture of the first page, rendered by the engine, as the package's thumbnail (at most 256 pixels on its long
/// side); an unedited save keeps the file's own, byte for byte.
#[test]
fn odg_thumbnail_is_regenerated_on_an_edited_save() {
    let dir = Dir::new();
    let old = small_png(2, 2, [250, 0, 250, 255]);
    let mut s = open_drawing(&dir, &four().file("Thumbnails/thumbnail.png", &old));
    // Nothing changed: the thumbnail is the file's own.
    assert_eq!(save(&mut s)["edited"], false);
    assert_eq!(part_bytes(&dir.read("a.odg"), "Thumbnails/thumbnail.png").as_deref(), Some(&old[..]));
    // Shape A (a blue rectangle at 2 to 6 cm from the left and 3 to 5 cm from the top) is moved 5 cm to the right.
    let a = id_of(s.document().unwrap(), 0, "A");
    run(&mut s, "shape.move", json!({ "ids": [a], "dx": 5.0 / 2.54, "dy": 0.0 }));
    assert_eq!(save(&mut s)["edited"], true);
    let made = part_bytes(&dir.read("a.odg"), "Thumbnails/thumbnail.png").expect("a thumbnail");
    assert_ne!(made, old, "regenerated");
    let (width, height, pixels) = decode(&made);
    assert!(
        width <= 256 && (200..=256).contains(&height) && width < height,
        "at most 256 pixels on the long side, the page's own shape: {width} x {height}"
    );
    // The page is 21 by 29.7 cm: a point at (x cm, y cm from the top) is at (x / 21 * width, y / 29.7 * height).
    let at = |x_cm: f64, y_cm: f64| {
        let (x, y) = ((x_cm / 21.0 * f64::from(width)) as usize, (y_cm / 29.7 * f64::from(height)) as usize);
        let i = (y * width as usize + x) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
    };
    let blue = [0x34, 0x65, 0xa4, 255];
    assert_eq!(at(7.4, 3.3), blue, "A is where it was moved to");
    assert_eq!(at(2.6, 3.3), [255, 255, 255, 255], "and not where it was");
    assert_eq!(at(12.6, 3.3), blue, "B, which was not moved, is where it was");
    // The next save of the same drawing keeps this thumbnail: it is no edit.
    assert_eq!(save(&mut s)["edited"], false);
    assert_eq!(part_bytes(&dir.read("a.odg"), "Thumbnails/thumbnail.png").as_deref(), Some(&made[..]));
}

/// A package with no thumbnail gets one on an edited save.
#[test]
fn an_edited_save_gives_a_package_with_no_thumbnail_one() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    assert!(part_bytes(&dir.read("a.odg"), "Thumbnails/thumbnail.png").is_none());
    shift(&mut s, "A", 1.0, 0.0);
    save(&mut s);
    let made = part_bytes(&dir.read("a.odg"), "Thumbnails/thumbnail.png").expect("a thumbnail");
    assert!(made.starts_with(&[0x89, b'P', b'N', b'G']));
    assert!(part_of(&dir.read("a.odg"), "META-INF/manifest.xml").contains("Thumbnails/thumbnail.png"));
}

/// A drawing is saved in its own format. A VSDX drawing keeps `.vsdx` (Save As offers the type the drawing has, and the engine refuses another in words,
/// writing nothing); an `.odg` is saved as an `.odg` and to no other type; a new drawing is saved as a VSDX file.
#[test]
fn save_as_vsdx_document_keeps_vsdx() {
    use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape};
    let dir = Dir::new();
    let vsdx = VsdxBuilder::new().page(PageSpec::new(1, "First").shapes(&shape(5, r#"<Cell N="Width" V="2.50"/>"#))).build();
    let mut s = open_bytes_in(&dir, "plan.vsdx", &vsdx);
    // A VSDX drawing saved as an .odg is a save across the formats; this one's first page has no size, which the package is made for, and it is refused in words
    // and nothing is written.
    let before = snapshot(&s);
    let e = s.execute("file.save_as", &json!({ "path": dir.path("plan.odg") })).unwrap_err();
    assert!(e.to_string().contains("first page") && e.to_string().contains("Nothing was written"), "{e}");
    assert_eq!(dir.files(), ["plan.vsdx"], "nothing was written");
    assert_eq!(snapshot(&s), before);
    // Saved as a VSDX file it is a VSDX file.
    let result = run(&mut s, "file.save_as", json!({ "path": dir.path("copy.vsdx") }));
    assert_eq!(result["variant"], "vsdx");
    assert!(result.get("parts").is_some(), "the result has the parts of a VSDX save: {result}");
    // A new drawing is saved as an .odg by the model writer, and as a VSDX file.
    let mut fresh = crate::blank_session();
    assert_eq!(run(&mut fresh, "file.save_as", json!({ "path": dir.path("new.odg") }))["variant"], "odg");
    let mut fresh = crate::blank_session();
    assert_eq!(run(&mut fresh, "file.save_as", json!({ "path": dir.path("new.vsdx") }))["variant"], "vsdx");
    assert_eq!(dir.files(), ["copy.vsdx", "new.odg", "new.vsdx", "plan.vsdx"]);
    // A name that is no type of either family is refused by `file.save_as` as a bad parameter, naming the types (the `.odg` among them), and by `file.save`
    // of a drawing whose file has such a name as a failure that says to use `file.save_as`; nothing is written.
    let e = fresh.execute("file.save_as", &json!({ "path": dir.path("new.txt") })).unwrap_err();
    assert!(matches!(&e, EngineError::BadParams { .. }), "{e:?}");
    assert!(
        e.to_string().contains(
            "the file name must end in .diacraft, .diacraft-stencil, .diacraft-template, .vsdx, .vsdm, .vssx, .vssm, .vstx, .vstm, .odg or .otg"
        ),
        "{e}"
    );
    let mut odd = open_bytes_in(&dir, "plan.dat", &vsdx);
    let e = odd.execute("file.save", &json!({})).unwrap_err();
    assert!(matches!(&e, EngineError::Failed { .. }), "{e:?}");
    assert!(e.to_string().contains("the drawing's file name must end in .vsdx, .vsdm, .vssx, .vssm, .vstx or .vstm: use file.save_as"), "{e}");
    assert_eq!(dir.files(), ["copy.vsdx", "new.odg", "new.vsdx", "plan.dat", "plan.vsdx"], "nothing was written");
}

#[test]
fn an_odg_is_saved_as_an_odg_and_save_as_another_odg_makes_it_the_drawings_file() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let original = dir.read("a.odg");
    shift(&mut s, "A", 1.0, 0.0);
    // A name that is no type is refused in words, and nothing is written.
    let before = snapshot(&s);
    let e = s.execute("file.save_as", &json!({ "path": dir.path("b.txt") })).unwrap_err();
    assert!(e.to_string().contains("the file name must end in"), "{e}");
    assert_eq!(dir.files(), ["a.odg"]);
    assert_eq!(snapshot(&s), before);
    // As another .odg: the new file is the drawing's own from then on, and the first is as it was.
    let result = run(&mut s, "file.save_as", json!({ "path": dir.path("b.odg") }));
    assert_eq!((result["variant"].as_str(), result["edited"].clone()), (Some("odg"), json!(true)));
    assert_eq!(dir.read("a.odg"), original, "the first file is as it was");
    assert_eq!(s.path().and_then(|p| p.file_name()).and_then(|n| n.to_str()), Some("b.odg"));
    assert_reopens_as_saved(&dir, &mut s, "b.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "b.odg");
    shift(&mut s, "B", 0.0, 1.0);
    assert_eq!(save(&mut s)["edited"], true);
    assert_eq!(dir.read("a.odg"), original, "and stays so");
    assert_reopens_as_saved(&dir, &mut s, "b.odg", 0);
    // `file.save` of a drawing whose file is not named .odg is refused in words, as is a path with no type.
    let mut odd = open_bytes_in(&dir, "odd.vsdx", &four().bytes());
    shift(&mut odd, "A", 1.0, 0.0);
    let words = refused(&mut odd);
    assert!(words.contains("file name must end in .odg"), "{words}");
    let e = s.execute("file.save_as", &json!({ "path": dir.path("noext") })).unwrap_err();
    assert!(
        e.to_string().contains("must end in .diacraft") && e.to_string().contains(".odg or .otg") && !e.to_string().contains(&dir.path("noext")),
        "{e}"
    );
}

/// A drawing template (`.otg`) is opened as a new drawing made from it (it was refused until then): the open document is replaced, and the new drawing has no file.
/// (`native_odg.rs` holds the rest of the `.otg` rows.)
#[test]
fn an_odg_template_opens_as_a_new_drawing_and_replaces_the_open_document() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let path = dir.write("t.otg", &four().bytes_of_type(TEMPLATE));
    let opened = run(&mut s, "file.open", json!({ "path": path }));
    assert_eq!(opened["file"]["new_from_template"], true, "{opened}");
    assert!(s.source().is_none() && !s.is_dirty());
}

/// Every way an `.odg` can fail to open leaves the open document as it was, and the message holds neither the path nor the file's text.
#[test]
fn an_odg_that_cannot_be_opened_leaves_the_open_document_as_it_was() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    shift(&mut s, "A", 1.0, 0.0);
    let before = snapshot(&s);
    let good = four().bytes();
    let broken = {
        let files = vec![
            ("content.xml".to_owned(), b"<office:document-content><unclosed></office:document-content>".to_vec()),
            ("styles.xml".to_owned(), four().styles_xml().into_bytes()),
        ];
        package(DRAWING, files)
    };
    let no_content = package(DRAWING, vec![("styles.xml".to_owned(), four().styles_xml().into_bytes())]);
    // The kind of trouble, as a typed field and as the first words of the message: the reader's own class follows.
    let cases: Vec<(&str, Vec<u8>, OpenProblem, &str)> = vec![
        ("html.odg", b"<html>not a zip</html>".to_vec(), OpenProblem::NotAPackage, "this is not a drawing file (not_zip)"),
        ("cut.odg", good[..good.len() / 2].to_vec(), OpenProblem::Damaged, "this file is damaged (zip_corrupt)"),
        ("empty.odg", Vec::new(), OpenProblem::NotAPackage, "this is not a drawing file (not_zip)"),
        ("broken.odg", broken, OpenProblem::Damaged, "this file is damaged (xml_malformed)"),
        ("no_content.odg", no_content, OpenProblem::Damaged, "this file is damaged (no_drawing_content)"),
    ];
    for (name, bytes, problem, words) in cases {
        let path = dir.write(name, &bytes);
        let e = s.execute("file.open", &json!({ "path": path })).unwrap_err();
        assert!(matches!(&e, EngineError::CannotOpen { problem: p, .. } if *p == problem), "{name}: {e:?}: wanted {problem:?}");
        assert!(e.to_string().contains(words), "{name}: {e}: wanted {words}");
        assert!(!e.to_string().contains(&path) && !e.to_string().contains("unclosed"), "{name}: the message holds the path or the file's text: {e}");
        assert_eq!(snapshot(&s), before, "{name}: a failed open changed the session");
        check_session(&s).unwrap();
    }
}

/// An encrypted `.odg` is not a drawing this opens, and one with a part past a limit is too big to open: each by the kind of trouble it is, in words, with the
/// open document as it was.
#[test]
fn an_encrypted_odg_and_one_past_a_limit_are_refused_by_their_kind_in_words() {
    use diacraft_testkit::package::{ZipItem, zip};
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let before = snapshot(&s);
    let manifest = |rows: &str| {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\"><manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"{DRAWING}\"/>{rows}</manifest:manifest>"
        )
    };
    let encrypted = zip(&[
        ZipItem::stored("mimetype", DRAWING.as_bytes()),
        ZipItem::deflated("content.xml", b"x"),
        ZipItem::deflated(
            "META-INF/manifest.xml",
            manifest(
                "<manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"><manifest:encryption-data manifest:checksum-type=\"SHA1\" manifest:checksum=\"AA==\"/></manifest:file-entry>",
            )
            .as_bytes(),
        ),
    ]);
    let too_big = zip(&[
        ZipItem::stored("mimetype", DRAWING.as_bytes()),
        ZipItem::zeros_under_the_ratio_cap("content.xml", 300 * 1024 * 1024),
        ZipItem::deflated(
            "META-INF/manifest.xml",
            manifest("<manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/>").as_bytes(),
        ),
    ]);
    // A package whose directory the sniff reads whole, and whose content nests past the XML depth: the odg reader, not the other, refuses it.
    let nested = package(DRAWING, vec![("content.xml".to_owned(), format!("{}{}", "<a>".repeat(5_000), "</a>".repeat(5_000)).into_bytes())]);
    let cases = [
        ("encrypted.odg", encrypted, OpenProblem::NotAPackage, "this is not a drawing file (encrypted_package)"),
        ("too_big.odg", too_big, OpenProblem::TooBig, "this file is too big to open safely"),
        ("too_deep.odg", nested, OpenProblem::TooBig, "this file is too big to open safely (xml_too_complex)"),
    ];
    for (name, bytes, problem, words) in cases {
        let path = dir.write(name, &bytes);
        let e = s.execute("file.open", &json!({ "path": path })).unwrap_err();
        assert!(matches!(&e, EngineError::CannotOpen { problem: p, .. } if *p == problem), "{name}: {e:?}: wanted {problem:?}");
        assert!(e.to_string().contains(words), "{name}: {e}: wanted {words}");
        assert_eq!(snapshot(&s), before, "{name}: a failed open changed the session");
    }
}

/// A drawing opened through a link is written to the file the link led to when it was opened, and the link stays a link (gaps A5), for an `.odg` as for the
/// other types.
#[cfg(unix)]
#[test]
fn an_odg_opened_through_a_link_is_saved_to_the_file_the_link_led_to_and_the_link_stays_a_link() {
    let dir = Dir::new();
    dir.write("real.odg", &four().bytes());
    std::os::unix::fs::symlink(dir.path("real.odg"), dir.path("link.odg")).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": dir.path("link.odg") }));
    assert_eq!(s.source().unwrap().path(), std::path::Path::new(&dir.path("link.odg")));
    shift(&mut s, "A", 1.0, 0.0);
    assert_eq!(save(&mut s)["edited"], true);
    assert!(std::fs::symlink_metadata(dir.path("link.odg")).unwrap().file_type().is_symlink(), "the link is still a link");
    assert_ne!(dir.read("real.odg"), four().bytes(), "the file it led to was written");
    let file = s.source().unwrap();
    // The target is the link resolved in full: on macOS the temporary folder is itself under a link (`/var` is `/private/var`), so the folder of
    // the target is the resolved one, not the folder the test was given.
    let real = std::fs::canonicalize(dir.path("real.odg")).unwrap();
    assert_eq!((file.path(), file.target()), (std::path::Path::new(&dir.path("link.odg")), real.as_path()));
    assert_saves_again_as_no_edit(&dir, &mut s, "real.odg");
}

/// A file that something else changed on the disk is not replaced by a save, unless `force` says so, for an `.odg` as for any file.
#[test]
fn a_changed_odg_on_the_disk_is_not_replaced_without_force() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    shift(&mut s, "A", 1.0, 0.0);
    let mut other = four().bytes();
    other.extend_from_slice(b"some bytes after the end of the archive");
    dir.write("a.odg", &other);
    let e = s.execute("file.save", &json!({})).unwrap_err();
    assert!(matches!(e, EngineError::ChangedOnDisk { .. }), "{e:?}");
    assert_eq!(dir.read("a.odg"), other, "nothing was written");
    let forced = run(&mut s, "file.save", json!({ "force": true }));
    assert_eq!(forced["edited"], true);
    assert_ne!(dir.read("a.odg"), other);
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
}
