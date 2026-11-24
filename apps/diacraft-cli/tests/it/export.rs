//! `diacraft-cli export`: the binary, run over drawings built in code (`diacraft-testkit`), and the files it writes.

use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape};

use crate::{TempDir, cli, json_lines, s, text};

/// A 4 by 3 inch page with a red box, [1, 3] x [1, 2] in paper inches, and a label.
fn drawing() -> Vec<u8> {
    drawing_in("")
}

/// The same page with the label set in the face `font` (the face the file names, as a `Font` cell of the character row), or in no face named.
fn drawing_in(font: &str) -> Vec<u8> {
    let font_cell = if font.is_empty() { String::new() } else { format!(r#"<Cell N="Font" V="{font}"/>"#) };
    let sheet = r#"<Cell N="PageWidth" V="4"/><Cell N="PageHeight" V="3"/><Cell N="PageScale" V="1"/><Cell N="DrawingScale" V="1"/>"#;
    let boxed = shape(
        1,
        r##"<Cell N="PinX" V="1"/><Cell N="PinY" V="1"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="0"/><Cell N="LocPinY" V="0"/>
<Cell N="FillPattern" V="1"/><Cell N="FillForegnd" V="#FF0000"/><Cell N="LinePattern" V="0"/>
<Section N="Geometry" IX="0">
<Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="2"><Cell N="X" V="2"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="3"><Cell N="X" V="2"/><Cell N="Y" V="1"/></Row>
<Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="1"/></Row>
<Row T="LineTo" IX="5"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
</Section>"##,
    );
    let label = shape(
        2,
        &format!(
            r##"<Cell N="PinX" V="0.5"/><Cell N="PinY" V="2"/><Cell N="Width" V="3"/><Cell N="Height" V="0.6"/><Cell N="LocPinX" V="0"/><Cell N="LocPinY" V="0"/>
<Cell N="FillPattern" V="0"/><Cell N="LinePattern" V="0"/>
<Section N="Character"><Row IX="0">{font_cell}<Cell N="Size" V="0.3"/><Cell N="Color" V="#00AA00"/></Row></Section>
<Text><cp IX="0"/>Hello</Text>"##
        ),
    );
    VsdxBuilder::new().page(PageSpec::new(1, "First page").sheet(sheet).shapes(&(boxed + &label))).build()
}

fn be32(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

#[test]
fn an_svg_is_written_and_the_line_says_where_what_page_how_big_and_what_was_left_out() {
    let dir = TempDir::new("export-svg");
    let drawing = dir.write("d.vsdx", &drawing());
    let out = dir.path("p.svg");
    let run = cli(&["export", s(&drawing), "--page", "1", "--out", s(&out)], None);
    assert!(run.status.success(), "{}", text(&run.stderr));
    let lines = json_lines(&run.stdout);
    assert_eq!(lines.len(), 1);
    let r = &lines[0];
    assert_eq!(r["format"], "svg");
    assert_eq!(r["page"]["name"], "First page");
    assert_eq!((r["width"].as_u64(), r["height"].as_u64()), (Some(384), Some(288)));
    let file = std::fs::read_to_string(&out).unwrap();
    assert_eq!(r["bytes"].as_u64(), Some(file.len() as u64));
    assert!(file.contains("viewBox=\"0 0 384 288\""), "{file}");
    assert!(file.contains("fill=\"#ff0000\""), "the red box");
    assert!(file.contains(">Hello</text>"), "the label as text");
    assert!(file.contains("<title>First page</title>"));
    // Everything that was left out would be said, as the window says it; the page of the test leaves nothing out but a stand-in face at most.
    assert!(r["skipped"].is_array());
}

/// A `.diacraft` file is exported as a VSDX file is: told by its bytes, read by the one reader of the format, and drawn as the window draws it, so the picture is the one
/// the drawing it was saved from gives.
#[test]
fn a_diacraft_file_is_exported_as_the_drawing_it_was_saved_from() {
    let dir = TempDir::new("export-diacraft");
    let vsdx = dir.write("d.vsdx", &drawing());
    let native = dir.path("d.diacraft");
    let open = serde_json::json!({ "path": vsdx }).to_string();
    let save = serde_json::json!({ "path": native }).to_string();
    let made = cli(&["run", "--cmd", "file.open", "--params", &open, "--cmd", "file.save_as", "--params", &save], None);
    assert!(made.status.success(), "{}", text(&made.stderr));
    let bytes = std::fs::read(&native).unwrap();
    assert!(bytes.starts_with(b"PK") && bytes.windows(26).any(|w| w == b"application/vnd.diacraft.d"), "a .diacraft file");
    let export = |file: &str, name: &str| {
        let out = dir.path(name);
        let run = cli(&["export", file, "--out", s(&out), "--bundled-fonts"], None);
        assert!(run.status.success(), "{}", text(&run.stderr));
        (json_lines(&run.stdout).remove(0), std::fs::read_to_string(&out).unwrap())
    };
    let (line, svg) = export(s(&native), "n.svg");
    let (reference, expected) = export(s(&vsdx), "v.svg");
    assert_eq!(line["page"]["name"], "First page");
    assert!(svg.contains("fill=\"#ff0000\"") && svg.contains(">Hello</text>"), "{svg}");
    assert_eq!((line["width"].clone(), line["height"].clone()), (reference["width"].clone(), reference["height"].clone()));
    assert_eq!(svg, expected, "the same picture as the VSDX file it came from");
    // A file that is cut off after its first entry is refused in words, with no path.
    let cut = dir.write("cut.diacraft", &bytes[..120]);
    let run = cli(&["export", s(&cut), "--out", s(&dir.path("c.svg"))], None);
    assert!(!run.status.success());
    let stderr = text(&run.stderr);
    assert!(stderr.contains("cannot open the drawing") && stderr.contains("the ZIP archive is damaged"), "{stderr}");
    assert!(!stderr.contains("cut.diacraft"), "{stderr}");
}

/// `--bundled-fonts` reads nothing from the machine, and without it the faces are the machine's: a label set in Carlito and drawn as outlines
/// is Carlito's letters in the first and a stand-in's in the second, so the two files differ, and two runs with the option are the same file.
/// Where the machine has no Carlito (or the bundled faces have it) the two would not differ, and the test says `SKIPPED`.
#[test]
fn bundled_fonts_draws_in_the_bundled_faces_and_the_default_in_the_machines() {
    use diacraft_fonts::{FontDb, FontStyle, MatchKind};

    let plain = FontStyle { bold: false, italic: false };
    let has = |db: &FontDb| db.resolve("Carlito", plain).kind == MatchKind::Named;
    if !has(&FontDb::system_and_bundled()) || has(&FontDb::bundled()) {
        eprintln!("SKIPPED: this machine has no Carlito apart from the bundled faces, so the two runs would draw the same letters");
        return;
    }
    let dir = TempDir::new("export-fonts");
    let drawing = dir.write("d.vsdx", &drawing_in("Carlito"));
    let export = |name: &str, extra: &[&str]| {
        let out = dir.path(name);
        let mut args = vec!["export", s(&drawing), "--out", s(&out), "--text", "outlines"];
        args.extend_from_slice(extra);
        let run = cli(&args, None);
        assert!(run.status.success(), "{}", text(&run.stderr));
        std::fs::read(&out).unwrap()
    };
    let (machine, bundled, again) = (export("a.svg", &[]), export("b.svg", &["--bundled-fonts"]), export("c.svg", &["--bundled-fonts"]));
    assert_ne!(machine, bundled, "the machine's Carlito and the bundled stand-in draw other letters");
    assert_eq!(bundled, again, "the bundled faces are the same on every run");
}

#[test]
fn a_png_is_written_at_the_resolution_asked_for() {
    let dir = TempDir::new("export-png");
    let drawing = dir.write("d.vsdx", &drawing());
    let out = dir.path("p.png");
    let run = cli(&["export", s(&drawing), "--out", s(&out), "--dpi", "50"], None);
    assert!(run.status.success(), "{}", text(&run.stderr));
    let file = std::fs::read(&out).unwrap();
    assert_eq!(&file[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!((be32(&file, 16), be32(&file, 20)), (200, 150), "4 by 3 inches at 50 pixels to the inch");
    assert_eq!(json_lines(&run.stdout)[0]["width"], 200);
    // The default is 96.
    let out = dir.path("q.png");
    assert!(cli(&["export", s(&drawing), "--out", s(&out)], None).status.success());
    let file = std::fs::read(&out).unwrap();
    assert_eq!((be32(&file, 16), be32(&file, 20)), (384, 288));
}

#[test]
fn a_pdf_is_written_with_the_pages_size_in_points() {
    let dir = TempDir::new("export-pdf");
    let drawing = dir.write("d.vsdx", &drawing());
    let out = dir.path("p.pdf");
    let run = cli(&["export", s(&drawing), "--out", s(&out)], None);
    assert!(run.status.success(), "{}", text(&run.stderr));
    let file = std::fs::read(&out).unwrap();
    assert!(file.starts_with(b"%PDF-"));
    let r = &json_lines(&run.stdout)[0];
    assert_eq!((r["width"].as_u64(), r["height"].as_u64()), (Some(288), Some(216)));
    // The page's own size is in the file, whatever else is.
    assert!(text(&file).contains("/MediaBox"));
}

#[test]
fn a_file_that_is_not_written_leaves_the_old_one_as_it_was_and_names_no_path() {
    let dir = TempDir::new("export-fail");
    let drawing = dir.write("SECRETNAME.vsdx", &drawing());
    let out = dir.write("p.svg", b"the old picture");
    // A page the drawing does not have.
    let run = cli(&["export", s(&drawing), "--page", "2", "--out", s(&out)], None);
    assert_eq!(run.status.code(), Some(1));
    let err = text(&run.stderr);
    assert!(err.contains("the drawing has 1 page: there is no page 2"), "{err}");
    assert!(!err.contains("SECRETNAME") && !err.contains(s(&dir.path(""))), "no path in a message: {err}");
    assert_eq!(std::fs::read(&out).unwrap(), b"the old picture");
    assert!(run.stdout.is_empty());
}

#[test]
fn a_drawing_that_will_not_open_is_an_error_in_words_and_nothing_is_written() {
    let dir = TempDir::new("export-bad");
    let not_a_drawing = dir.write("x.vsdx", b"<html>not a zip</html>");
    let out = dir.path("p.svg");
    let run = cli(&["export", s(&not_a_drawing), "--out", s(&out)], None);
    assert_eq!(run.status.code(), Some(1));
    assert!(text(&run.stderr).contains("cannot open the drawing"), "{}", text(&run.stderr));
    assert!(!out.exists());
    let missing = dir.path("missing.vsdx");
    let run = cli(&["export", s(&missing), "--out", s(&out)], None);
    assert_eq!(run.status.code(), Some(1));
    assert!(text(&run.stderr).contains("cannot read the drawing: there is no such file"), "{}", text(&run.stderr));
    let run = cli(&["export", s(&dir.path("")), "--out", s(&out)], None);
    assert_eq!(run.status.code(), Some(1));
    assert!(text(&run.stderr).contains("not a file"), "{}", text(&run.stderr));
}

#[test]
fn the_output_is_never_the_drawing_and_a_folder_that_is_not_there_is_an_error() {
    let dir = TempDir::new("export-out");
    let original = drawing();
    let path = dir.write("d.vsdx", &original);
    // `.vsdx` is not a format it writes, so the drawing cannot be named at all; a link to it with a good extension is caught by its identity.
    let run = cli(&["export", s(&path), "--out", s(&path)], None);
    assert_eq!(run.status.code(), Some(2), "{}", text(&run.stderr));
    #[cfg(unix)]
    {
        let link = dir.path("link.svg");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        let run = cli(&["export", s(&path), "--out", s(&link)], None);
        assert_eq!(run.status.code(), Some(1));
        assert!(text(&run.stderr).contains("--out is the drawing itself"), "{}", text(&run.stderr));
        assert_eq!(std::fs::read(&path).unwrap(), original, "the drawing is as it was");
    }
    let nowhere = dir.path("no-such-folder").join("p.svg");
    let run = cli(&["export", s(&path), "--out", s(&nowhere)], None);
    assert_eq!(run.status.code(), Some(1));
    assert!(text(&run.stderr).contains("cannot write the file"), "{}", text(&run.stderr));
}

#[test]
fn a_mistake_in_the_line_is_status_two_and_runs_nothing() {
    let dir = TempDir::new("export-line");
    let drawing = dir.write("d.vsdx", &drawing());
    let out = dir.path("p.gif");
    for args in [
        vec!["export", s(&drawing)],
        vec!["export", s(&drawing), "--out", s(&out)],
        vec!["export", s(&drawing), "--out", "p.svg", "--page", "0"],
        vec!["export", s(&drawing), "--out", "p.svg", "--dpi", "96"],
    ] {
        let run = cli(&args, None);
        assert_eq!(run.status.code(), Some(2), "{args:?}: {}", text(&run.stderr));
    }
    assert!(!out.exists());
}

#[test]
fn a_page_too_large_for_the_format_is_an_error_and_not_a_file() {
    let dir = TempDir::new("export-huge");
    let sheet = r#"<Cell N="PageWidth" V="1000000000"/><Cell N="PageHeight" V="3"/><Cell N="PageScale" V="1"/><Cell N="DrawingScale" V="1"/>"#;
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "Huge").sheet(sheet)).build();
    let drawing = dir.write("d.vsdx", &bytes);
    for name in ["p.png", "p.svg", "p.pdf"] {
        let out = dir.path(name);
        let run = cli(&["export", s(&drawing), "--out", s(&out)], None);
        assert_eq!(run.status.code(), Some(1), "{name}: {}", text(&run.stderr));
        assert!(text(&run.stderr).contains("too large to export"), "{name}: {}", text(&run.stderr));
        assert!(!out.exists(), "{name}");
    }
}
