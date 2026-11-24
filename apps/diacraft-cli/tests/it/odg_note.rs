//! The save note as a field of the command line's answer (design decision 8): `notes` in the result of `file.save` for an `.odg`, empty for a drawing that a
//! LibreOffice save keeps whole and holding the note for one that holds behaviour it would drop. It is not among the `warnings`.

use diacraft_container::zip::{ArchiveWriter, Method};
use serde_json::json;

use crate::{TempDir, cli, json_lines, s, text};

const DRAWING: &str = "application/vnd.oasis.opendocument.graphics";
const NS: &str = concat!(
    r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
    r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
    r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" "#,
    r#"xmlns:diacraft="urn:app.diacraft:extension""#
);

/// A one-page drawing of two rectangles, A and B, each on a style of its own. With `formula`, A's style carries DiaCraft's data: a cell `Cost` whose formula reads B
/// (`Sheet.2!Width`), and B's carries its `ID`, as a save of DiaCraft writes them.
fn drawing(formula: bool) -> Vec<u8> {
    let cells = "&lt;Sheet&gt;&lt;Cell N=&quot;Cost&quot; V=&quot;1&quot; F=&quot;Sheet.2!Width&quot;/&gt;&lt;/Sheet&gt;";
    let (a_data, b_data) = if formula {
        (format!(r#"diacraft:version="1" diacraft:id="1" diacraft:cells="{cells}""#), r#"diacraft:version="1" diacraft:id="2""#.to_owned())
    } else {
        (String::new(), String::new())
    };
    let style = |name: &str, data: &str| {
        format!(
            r##"<style:style style:name="{name}" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000" draw:stroke="none" {data}/></style:style>"##
        )
    };
    let shape = |name: &str, style: &str, x: u32| {
        format!(
            "<draw:custom-shape draw:style-name=\"{style}\" draw:layer=\"layout\" draw:name=\"{name}\" svg:width=\"4cm\" svg:height=\"2cm\" svg:x=\"{x}cm\" svg:y=\"3cm\"><text:p/><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"non-primitive\" draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N\"/></draw:custom-shape>"
        )
    };
    let content = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-content {NS} office:version=\"1.3\"><office:automatic-styles>{}{}</office:automatic-styles><office:body><office:drawing><draw:page draw:name=\"p\" draw:master-page-name=\"Default\">{}{}</draw:page></office:drawing></office:body></office:document-content>",
        style("redA", &a_data),
        style("redB", &b_data),
        shape("A", "redA", 2),
        shape("B", "redB", 12)
    );
    let styles = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-styles {NS} office:version=\"1.3\"><office:styles/><office:automatic-styles><style:page-layout style:name=\"PM1\"><style:page-layout-properties fo:page-width=\"21cm\" fo:page-height=\"29.7cm\" style:print-orientation=\"portrait\"/></style:page-layout></office:automatic-styles><office:master-styles><draw:layer-set><draw:layer draw:name=\"layout\"/></draw:layer-set><style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"/></office:master-styles></office:document-styles>"
    );
    let manifest = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\"><manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"{DRAWING}\"/><manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/><manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/></manifest:manifest>"
    );
    let mut w = ArchiveWriter::new();
    w.add("mimetype", DRAWING.as_bytes(), Method::Stored).unwrap();
    w.add("content.xml", content.as_bytes(), Method::Deflated).unwrap();
    w.add("styles.xml", styles.as_bytes(), Method::Deflated).unwrap();
    w.add("META-INF/manifest.xml", manifest.as_bytes(), Method::Deflated).unwrap();
    w.finish().unwrap()
}

/// Opens `file`, moves A and saves, and gives the lines `run` printed.
fn open_move_save(file: &std::path::Path) -> Vec<serde_json::Value> {
    let open = json!({"path": s(file)}).to_string();
    let move_a = json!({"ids": [1], "dx": 1.0, "dy": 0.0}).to_string();
    let out = cli(&["run", "--cmd", "file.open", "--params", &open, "--cmd", "shape.move", "--params", &move_a, "--cmd", "file.save"], None);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 3, "{lines:?}");
    lines
}

/// `notes` is an array in the result of the save: empty for a drawing a LibreOffice save keeps whole, and holding the note, with its class and counts, for one that holds
/// a formula that reads another shape. It is not a warning.
#[test]
fn run_gives_the_save_note_as_the_notes_field_of_the_save() {
    let dir = TempDir::new("odg-note");
    let lines = open_move_save(&dir.write("plain.odg", &drawing(false)));
    assert_eq!(lines[2]["result"]["notes"], json!([]), "nothing a LibreOffice save would drop: {}", lines[2]);
    let lines = open_move_save(&dir.write("held.odg", &drawing(true)));
    let result = &lines[2]["result"];
    let notes = result["notes"].as_array().unwrap_or_else(|| panic!("`notes` is an array: {result}"));
    assert_eq!(notes.len(), 1, "{result}");
    assert_eq!(notes[0]["class"], "libreoffice_would_drop");
    assert_eq!(notes[0]["cross_shape_formulas"], 1, "{}", notes[0]);
    assert!(notes[0]["words"].as_str().unwrap().contains("a LibreOffice save would drop"), "{}", notes[0]);
    assert_eq!(result["warning_count"], 0, "it is not a warning: {result}");
}

/// **The open's answer names what the drawing does not hold**, first among `file.warnings`: the reader's head line, then a line with the count of each class
/// (here a dimension line, which the drawing keeps only as a placeholder); `warning_count` counts each line.
#[test]
fn run_gives_what_the_drawing_does_not_hold_first_among_the_open_s_warnings() {
    let dir = TempDir::new("odg-notkept");
    let bytes = drawing(false);
    // The drawing with a dimension line beside its rectangles.
    let package = diacraft_odg::Package::read(&bytes, &diacraft_odg::Limits::default()).unwrap();
    let content = String::from_utf8(package.part("content.xml").unwrap().to_vec()).unwrap().replacen(
        "</draw:page>",
        r#"<draw:measure svg:x1="1cm" svg:y1="8cm" svg:x2="6cm" svg:y2="8cm"/></draw:page>"#,
        1,
    );
    let styles = package.part("styles.xml").unwrap().to_vec();
    let manifest = package.part("META-INF/manifest.xml").unwrap().to_vec();
    let mut w = ArchiveWriter::new();
    w.add("mimetype", DRAWING.as_bytes(), Method::Stored).unwrap();
    w.add("content.xml", content.as_bytes(), Method::Deflated).unwrap();
    w.add("styles.xml", &styles, Method::Deflated).unwrap();
    w.add("META-INF/manifest.xml", &manifest, Method::Deflated).unwrap();
    let file = dir.write("measured.odg", &w.finish().unwrap());
    let open = json!({"path": s(&file)}).to_string();
    let out = cli(&["run", "--cmd", "file.open", "--params", &open], None);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let lines = json_lines(&out.stdout);
    let file = &lines[0]["result"]["file"];
    assert_eq!(file["warnings"][0], diacraft_odg::NOT_KEPT_HEAD, "{file}");
    assert_eq!(
        file["warnings"][1],
        "1 shape of a kind DiaCraft keeps only as a placeholder (a dimension line, a callout, a form control or a 3-D scene), which a copy writes as a plain shape",
        "{file}"
    );
    assert!(file["warning_count"].as_u64().unwrap() >= 2, "{file}");
    // A drawing that holds all of its file is told nothing of the kind.
    let plain = dir.write("plain.odg", &drawing(false));
    let open = json!({"path": s(&plain)}).to_string();
    let out = cli(&["run", "--cmd", "file.open", "--params", &open], None);
    let lines = json_lines(&out.stdout);
    assert!(!lines[0]["result"]["file"]["warnings"].as_array().unwrap().iter().any(|w| w == diacraft_odg::NOT_KEPT_HEAD), "{}", lines[0]);
}
