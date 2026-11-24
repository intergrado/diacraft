//! `diacraft-cli roundtrip --via diacraft`, `compare --content`, and the commands the trip is made of: the binary, run over packages built in code, and the one line each
//! prints. The names a person made up here are `Secret`, and none may be in a line.
//!
//! **A VSDX file makes the whole trip** (wired the engine's save of a `.diacraft` drawing as a VSDX type to the model writer), and the tests that run it compare the content
//! of what came back. **An `.odg` makes the whole trip too** (a drawing opened from an `.odg` is saved as `.diacraft` wired the `.odg` model writer,
//! which lifted the refusal of the last leg), and `an_odg_drawing_makes_the_whole_trip_and_the_content_check_says_what_it_found` holds the line to what the content check finds.

use std::path::Path;
use std::process::Output;

use diacraft_testkit::package::{MasterSpec, PageSpec, Pkg, VsdxBuilder, cell, shape, typical_shape};
use serde_json::{Value, json};

use crate::{BIN, TempDir, cli, json_lines, s, text};

/// A drawing of one page of three shapes (a box with geometry, a text and formula cells each: `typical_shape`), a master and a part the model does not hold. The names a person made up
/// are `Secret`.
fn drawing() -> Vec<u8> {
    let sheet = r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/><Cell N="PageScale" V="1"/><Cell N="DrawingScale" V="1"/>"#;
    let shapes: String = (1..=3).map(typical_shape).collect();
    VsdxBuilder::new()
        .page(PageSpec::new(1, "Secret page name").sheet(sheet).shapes(&shapes))
        .master(MasterSpec::new(1, "Secret master name").shapes(&shape(4, &cell("Width", "1"))))
        .with(Pkg::new().part_without_override("/foo/Secret name.dat", b"0123456789").default_type("dat", "application/x-foo"))
        .build()
}

/// A drawing whose one page has no shape on it.
fn empty_drawing() -> Vec<u8> {
    VsdxBuilder::new().page(PageSpec::new(1, "Secret page name").sheet(r#"<Cell N="PageWidth" V="4"/><Cell N="PageHeight" V="3"/>"#)).build()
}

/// The line of a command and what the process did. The line is `Null` when nothing was printed.
fn line_of(args: &[&str]) -> (Value, Output) {
    let out = cli(args, None);
    let report = json_lines(&out.stdout).into_iter().next().unwrap_or(Value::Null);
    (report, out)
}

/// `roundtrip --via diacraft` of `input` to `output`, with `extra` options.
fn visit(input: &Path, output: &Path, extra: &[&str]) -> (Value, Output) {
    let mut args = vec!["roundtrip", s(input), "--out", s(output), "--via", "diacraft"];
    args.extend_from_slice(extra);
    line_of(&args)
}

/// What the command printed, to put in the message of an assertion.
fn shown(report: &Value) -> String {
    serde_json::to_string_pretty(report).unwrap()
}

/// The parts the bookkeeping of an edited save writes (design 4.4) and the package structure around them: nothing a salted edit changes is a person's content in any of them.
const BOOKKEEPING: [&str; 8] = [
    "/docProps/core.xml",
    "/docProps/app.xml",
    "[Content_Types].xml",
    "/_rels/.rels",
    "/visio/_rels/document.xml.rels",
    "/visio/pages/_rels/pages.xml.rels",
    "/visio/pages/pages.xml",
    "/visio/document.xml",
];

/// The parts a new shape's master may bring in.
fn is_a_master_part(part: &str) -> bool {
    part.starts_with("/visio/masters/")
}

/// Asserts what a salted edit of `kind` may change in a VSDX package, as the content check tells it: the page the edit acts on, and only inside the shape it touched (the first of the
/// page, by its place), plus the bookkeeping parts; a new shape also brings its master. **The edit must be seen:** at least one difference in the page's part.
fn assert_only_the_shape_and_the_bookkeeping(content: &Value, kind: &str, report: &Value) {
    let differences = content["differences"].as_array().unwrap_or_else(|| panic!("no differences list: {}", shown(report)));
    assert_eq!(content["count"].as_u64(), Some(differences.len() as u64), "few enough differences to list them all: {}", shown(report));
    let mut at_the_page = 0;
    for difference in differences {
        let part = difference["part"].as_str().unwrap();
        let path = difference["path"].as_str().unwrap();
        if part == "/visio/pages/page1.xml" {
            at_the_page += 1;
            // The first shape of the page is the first `Shape` of its `Shapes`; a new shape is the last (the third place, after the three that were there).
            let first = path.contains("Shape[0]") || path.contains("Shape#0");
            let appended = kind == "add";
            assert!(first || appended, "{kind}: a difference outside the first shape, at {path}: {}", shown(report));
        } else {
            assert!(
                BOOKKEEPING.contains(&part) || (kind == "add" && is_a_master_part(part)) || part.ends_with(".rels"),
                "{kind}: a difference in {part}, which is neither the page nor bookkeeping: {}",
                shown(report)
            );
        }
    }
    assert!(at_the_page > 0, "{kind}: the edit is not in the page's part: {}", shown(report));
}

// ------------------------------------------------------------------------------------------------------------------------ the visit

/// The plain visit: an unedited VSDX package goes to `.diacraft` and back with the same content, and the line says so in the one shape every line has. (This test and the five of
/// `salted` were red until the engine's save of a `.diacraft` drawing as a VSDX type was wired to the model writer, for that reason alone: the engine's refusal was in the line.)
#[test]
fn roundtrip_via_diacraft_reports_the_same_content_for_a_built_vsdx() {
    let dir = TempDir::new("via");
    let input = dir.write("in.vsdx", &drawing());
    let output = dir.path("out.vsdx");
    let (report, out) = visit(&input, &output, &[]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(report["via"], "diacraft");
    assert_eq!(report["format"], "vsdx");
    assert_eq!(report["variant"], "vsdx");
    assert_eq!(report["opened"], true);
    assert_eq!(report["edit"], Value::Null);
    assert_eq!(report["input"]["bytes"], std::fs::metadata(&input).unwrap().len());
    assert_eq!(report["input"]["sha256"].as_str().unwrap().len(), 64);
    assert!(report["diacraft"]["bytes"].as_u64().unwrap() > 0, "the `.diacraft` file was made: {}", shown(&report));
    // The names a person made up are not in the line.
    assert!(!text(&out.stdout).contains("Secret"), "{}", text(&out.stdout));
    // The trip was made to its end: no leg was refused, and the check was made.
    assert_eq!(report["refusal"], Value::Null, "a leg of the trip was refused: {}", shown(&report));
    assert_eq!(report["output"]["bytes"], std::fs::metadata(&output).unwrap().len(), "{}", shown(&report));
    assert_eq!(report["content"]["compared"], true, "{}", shown(&report));
    assert_eq!(report["content"]["same"], true, "{}", shown(&report));
    assert_eq!(report["content"]["count"], 0);
    assert_eq!(report["content"]["differences"], json!([]));
    assert_eq!(report["model"]["same"], true, "{}", shown(&report));
}

/// One test of the plan's `each_salted_edit_differs_only_at_its_shape_and_the_bookkeeping` for each kind of edit (NP13), the edit is made, the file is saved as
/// `.diacraft` and opened again, saved back as a VSDX file, and the content check finds differences only in the page's part, at the shape the edit touched, and in the bookkeeping.
fn salted(kind: &str) {
    let dir = TempDir::new("salt");
    let input = dir.write("in.vsdx", &drawing());
    let output = dir.path("out.vsdx");
    let (report, out) = visit(&input, &output, &["--edit", kind]);
    assert!(out.status.success(), "{kind}: {}", text(&out.stderr));
    // The edit was made, on the first shape of the first page (a new shape is told by its own `ID`).
    assert_eq!(report["edit"]["kind"], kind, "{}", shown(&report));
    assert_eq!(report["edit"]["page"], 1, "{}", shown(&report));
    assert!(report["edit"]["error"].is_null(), "{kind}: the edit was not made: {}", shown(&report));
    if kind == "add" {
        assert!(report["edit"]["shape"].as_u64().is_some_and(|id| id > 3), "the new shape has an ID of its own: {}", shown(&report));
    } else {
        assert_eq!(report["edit"]["shape"], 1, "{}", shown(&report));
    }
    assert!(!text(&out.stdout).contains("Secret"), "{}", text(&out.stdout));
    assert_eq!(report["refusal"], Value::Null, "{kind}: a leg of the trip was refused: {}", shown(&report));
    assert_eq!(report["content"]["compared"], true, "{kind}: {}", shown(&report));
    assert_eq!(report["content"]["same"], false, "{kind}: the edit is a difference: {}", shown(&report));
    assert_only_the_shape_and_the_bookkeeping(&report["content"], kind, &report);
    // The model read back holds the edit: it is the model of the edited drawing.
    assert_eq!(report["model"]["same"], true, "{kind}: {}", shown(&report));
}

mod each_salted_edit_differs_only_at_its_shape_and_the_bookkeeping {
    #[test]
    fn cell() {
        super::salted("cell");
    }

    #[test]
    fn r#move() {
        super::salted("move");
    }

    #[test]
    fn retext() {
        super::salted("retext");
    }

    #[test]
    fn delete() {
        super::salted("delete");
    }

    #[test]
    fn add() {
        super::salted("add");
    }
}

/// **A check of the check above, green today**: what a salted edit changes in a VSDX package is told by the helper as it should be, on a file the engine writes now (a VSDX file saved over
/// itself splices into the bytes it was read from, and changes what the edit changed): each kind of edit, made by its own command, then the content check of the input and the file written.
/// The assertions the red tests make are shown to hold for an output that exists, and to fail for one that was not edited.
#[test]
fn the_helper_that_judges_a_salted_edit_holds_for_what_the_splice_writer_makes() {
    let dir = TempDir::new("salt-splice");
    let input = dir.write("in.vsdx", &drawing());
    let edits: [(&str, &str, Value); 5] = [
        ("cell", "shape.format", json!({ "ids": [1], "page": 1, "weight": 1.23 })),
        ("move", "shape.move", json!({ "ids": [1], "page": 1, "dx": 0.125, "dy": 0.125 })),
        ("retext", "shape.set_text", json!({ "id": 1, "page": 1, "text": "salt" })),
        ("delete", "shape.delete", json!({ "ids": [1], "page": 1 })),
        ("add", "shape.drop", json!({ "master": "rectangle", "page": 1, "x": 4.25, "y": 5.5 })),
    ];
    for (kind, command, params) in edits {
        let output = dir.path(&format!("{kind}.vsdx"));
        let (open, edit, save) = (json!({ "path": s(&input) }).to_string(), params.to_string(), json!({ "path": s(&output) }).to_string());
        let run = cli(
            &["run", "--cmd", "file.open", "--params", &open, "--cmd", command, "--params", &edit, "--cmd", "file.save_as", "--params", &save],
            None,
        );
        assert!(run.status.success(), "{kind}: {}", text(&run.stderr));
        let (report, out) = line_of(&["compare", "--content", s(&input), s(&output)]);
        assert!(out.status.success(), "{kind}: {}", text(&out.stderr));
        assert_eq!(report["content"]["same"], false, "{kind}: {}", shown(&report));
        assert_only_the_shape_and_the_bookkeeping(&report["content"], kind, &report);
    }
    // A file that was not edited is the same as itself, and the helper's demand that the edit be seen fails for it.
    let (report, _) = line_of(&["compare", "--content", s(&input), s(&input)]);
    assert_eq!(report["content"]["same"], true);
    let seen = std::panic::catch_unwind(|| assert_only_the_shape_and_the_bookkeeping(&report["content"], "cell", &report));
    assert!(seen.is_err(), "no difference in the page's part is not an edit that was seen");
}

// ------------------------------------------------------------------------------------------------------------------------ a leg that is refused

/// A file that does not open is a report with the leg, the engine's kind and its words, and never says same: the content and the model were not compared.
#[test]
fn an_input_that_does_not_open_is_refused_at_the_open_leg_and_nothing_says_same() {
    let dir = TempDir::new("via-bad");
    let input = dir.write("in.vsdx", b"not a package at all");
    let output = dir.path("out.vsdx");
    let (report, out) = visit(&input, &output, &[]);
    assert!(out.status.success(), "a refusal is a report: {}", text(&out.stderr));
    assert_eq!(report["opened"], false);
    assert_eq!(report["refusal"]["leg"], "open", "{}", shown(&report));
    assert_eq!(report["refusal"]["error"], "cannot_open");
    assert!(report["refusal"]["words"].as_str().is_some_and(|w| !w.is_empty()), "the engine's words: {}", shown(&report));
    assert_never_same(&report);
    assert!(!output.exists(), "nothing was written");
}

/// What a line that did not finish may say: the check was not made, and `same` is neither true nor false for the content and the model.
fn assert_never_same(report: &Value) {
    assert_eq!(report["content"]["compared"], false, "{}", shown(report));
    assert_eq!(report["content"]["same"], Value::Null, "{}", shown(report));
    assert_eq!(report["content"]["count"], Value::Null, "{}", shown(report));
    assert_eq!(report["content"]["differences"], json!([]), "{}", shown(report));
    assert_eq!(report["model"]["same"], Value::Null, "{}", shown(report));
    assert!(report["refusal"]["leg"].is_string(), "a line that did not finish says which leg: {}", shown(report));
    assert!(!text_of(report).contains("\"same\":true"), "{}", shown(report));
}

fn text_of(report: &Value) -> String {
    serde_json::to_string(report).unwrap()
}

/// An edit that has no shape to act on is refused at the edit leg, in the line's own words, and `--edit add` on the same file is not (it needs only a page).
#[test]
fn an_edit_with_no_shape_to_act_on_is_refused_at_the_edit_leg_and_an_add_is_not() {
    let dir = TempDir::new("via-empty");
    let input = dir.write("in.vsdx", &empty_drawing());
    for kind in ["cell", "move", "retext", "delete"] {
        let (report, out) = visit(&input, &dir.path("out.vsdx"), &["--edit", kind]);
        assert!(out.status.success(), "{kind}: {}", text(&out.stderr));
        assert_eq!(report["opened"], true);
        assert_eq!(report["edit"], json!({ "kind": kind, "error": "no_shape" }), "{}", shown(&report));
        assert_eq!(report["refusal"]["leg"], "edit", "{kind}: {}", shown(&report));
        assert_eq!(report["refusal"]["words"], "the file has no shape on any page to edit");
        assert_eq!(report["diacraft"], Value::Null, "the trip stopped at the edit: nothing was saved as .diacraft");
        assert_never_same(&report);
    }
    let (report, _) = visit(&input, &dir.path("out.vsdx"), &["--edit", "add"]);
    assert_eq!(report["edit"]["kind"], "add", "{}", shown(&report));
    assert!(report["edit"]["error"].is_null(), "a new shape needs a page and no shape: {}", shown(&report));
    assert_ne!(report["refusal"]["leg"], "edit", "{}", shown(&report));
    assert!(report["diacraft"]["bytes"].as_u64().is_some_and(|b| b > 0), "the trip went on to the `.diacraft` file: {}", shown(&report));
}

/// The command line's own mistakes are refused before anything is made: an output that names no type of the input's format, and a file that is a `.diacraft` already.
#[test]
fn a_name_of_another_format_and_a_diacraft_input_are_refused_in_words() {
    let dir = TempDir::new("via-names");
    let input = dir.write("in.vsdx", &drawing());
    let run = cli(&["roundtrip", s(&input), "--out", s(&dir.path("out.odg")), "--via", "diacraft"], None);
    assert_eq!(run.status.code(), Some(1));
    assert!(text(&run.stderr).contains("--out must end in .vsdx"), "{}", text(&run.stderr));
    assert!(run.stdout.is_empty());
    let run = cli(&["roundtrip", s(&input), "--out", s(&dir.path("out.txt")), "--via", "diacraft"], None);
    assert_eq!(run.status.code(), Some(1), "{}", text(&run.stderr));
    // The file itself is never the output.
    let run = cli(&["roundtrip", s(&input), "--out", s(&input), "--via", "diacraft"], None);
    assert_eq!(run.status.code(), Some(1));
    assert!(text(&run.stderr).contains("never writes its input"), "{}", text(&run.stderr));
    assert_eq!(std::fs::read(&input).unwrap(), drawing(), "the input was not written");
    // A `.diacraft` file has already made the trip.
    let native = dir.path("n.diacraft");
    let (open, save) = (json!({ "path": s(&input) }).to_string(), json!({ "path": s(&native) }).to_string());
    assert!(cli(&["run", "--cmd", "file.open", "--params", &open, "--cmd", "file.save_as", "--params", &save], None).status.success());
    let run = cli(&["roundtrip", s(&native), "--out", s(&dir.path("o.vsdx")), "--via", "diacraft"], None);
    assert_eq!(run.status.code(), Some(1));
    assert!(text(&run.stderr).contains("a .diacraft file already"), "{}", text(&run.stderr));
}

/// **An `.odg` makes the whole trip** (which wired the `.odg` model writer and so lifted Call 18's refusal, as had lifted the first leg's): it is opened and saved as
/// `.diacraft`, the `.diacraft` file is opened in a new session and saved back as an `.odg`, and the line holds the content check of the input and the output. **The check says what it finds, and
/// never same for a trip that changed the content**: the hand-built file of another program (version 1.3 on every part, no settings part, its own style names) differs from what the model writer
/// gives back only where the writer's own rules say (its own version, an empty settings part, its own names for the automatic styles, its own generator),
/// counts those as nothing, so the drawing is the same content. An edit made first is told, and is a difference.
#[test]
fn an_odg_drawing_makes_the_whole_trip_and_the_content_check_says_what_it_found() {
    let dir = TempDir::new("via-odg");
    let input = dir.write("in.odg", &crate::odg_save::drawing());
    let output = dir.path("out.odg");
    let (report, out) = visit(&input, &output, &[]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(report["format"], "odg");
    assert_eq!(report["opened"], true, "{}", shown(&report));
    assert_eq!(report["variant"], "odg", "{}", shown(&report));
    assert_eq!(report["refusal"], Value::Null, "the trip finished: {}", shown(&report));
    assert!(report["diacraft"]["bytes"].as_u64().is_some_and(|n| n > 0), "the `.diacraft` file was made: {}", shown(&report));
    assert!(report["output"]["bytes"].as_u64().is_some_and(|n| n > 0) && output.exists(), "the `.odg` was written back: {}", shown(&report));
    assert_eq!(report["content"]["compared"], true, "{}", shown(&report));
    assert_eq!(
        report["content"]["same"],
        true,
        "a hand-built file of another program is the content the model writer gives back: {}",
        shown(&report)
    );
    assert_eq!(report["content"]["count"], 0, "{}", shown(&report));
    assert_eq!(report["content"]["differences"], json!([]), "{}", shown(&report));
    // The output is an `.odg` that opens, with the drawing's two shapes.
    let mut session = diacraft_engine::Session::new();
    session.execute("file.open", &json!({ "path": s(&output) })).unwrap();
    assert_eq!(session.document().unwrap().pages.iter().next().map(|p| p.shapes.len()), Some(2));
    // An edit made first is told, and the trip finishes with it.
    let (edited, _) = visit(&input, &output, &["--move", "1,1"]);
    assert_eq!(edited["edit"]["kind"], "move", "{}", shown(&edited));
    assert_eq!(edited["refusal"], Value::Null, "{}", shown(&edited));
    assert_eq!(edited["content"]["compared"], true, "{}", shown(&edited));
    assert_eq!(edited["content"]["same"], false, "{}", shown(&edited));
    let differences = edited["content"]["differences"].as_array().unwrap();
    assert!(
        !differences.is_empty() && differences.iter().all(|d| d["part"].is_string() && d["path"].is_string() && d["found"].is_string()),
        "{}",
        shown(&edited)
    );
}

/// A drawing of two pages that share the automatic style `gr1`, the second with two shapes that carry ids and a connector glued to both (a drawing whose second page the model writer
/// writes with styles of its own, which carry DiaCraft's own markup), written by hand from the ODF standard. The file holds its layer set (LibreOffice's five): one that the writer adds where the
/// file had none still counts.
fn two_pages_odg() -> Vec<u8> {
    use crate::odg_save::{DRAWING, NS};
    use diacraft_container::zip::{ArchiveWriter, Method};
    let rect = |name: &str, id: &str, x: u32| {
        let ids = if id.is_empty() { String::new() } else { format!("xml:id=\"{id}\" draw:id=\"{id}\" ") };
        format!(
            "<draw:custom-shape {ids}draw:style-name=\"gr1\" draw:text-style-name=\"P1\" draw:name=\"{name}\" svg:width=\"4cm\" svg:height=\"2cm\" svg:x=\"{x}cm\" svg:y=\"3cm\"><text:p>t</text:p><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"non-primitive\" draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N\"/></draw:custom-shape>"
        )
    };
    let connector = "<draw:connector draw:style-name=\"gr1\" draw:text-style-name=\"P1\" draw:name=\"K\" draw:start-shape=\"id1\" draw:start-glue-point=\"3\" draw:end-shape=\"id2\" draw:end-glue-point=\"1\" svg:x1=\"6cm\" svg:y1=\"4cm\" svg:x2=\"12cm\" svg:y2=\"4cm\" svg:d=\"M 6000 4000 L 12000 4000\" svg:viewBox=\"0 0 12001 4001\"><text:p/></draw:connector>";
    let content = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-content {NS} office:version=\"1.3\"><office:automatic-styles><style:style style:name=\"gr1\" style:family=\"graphic\"><style:graphic-properties draw:fill=\"solid\" draw:fill-color=\"#3465a4\" draw:stroke=\"solid\" svg:stroke-width=\"0.05cm\" svg:stroke-color=\"#000000\"/></style:style><style:style style:name=\"P1\" style:family=\"paragraph\"/></office:automatic-styles><office:body><office:drawing><draw:page draw:name=\"page1\" draw:master-page-name=\"Default\">{}</draw:page><draw:page draw:name=\"page2\" draw:master-page-name=\"Default\">{}{}{connector}</draw:page></office:drawing></office:body></office:document-content>",
        rect("A", "", 2),
        rect("S", "id1", 2),
        rect("T", "id2", 12)
    );
    let styles = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-styles {NS} office:version=\"1.3\"><office:styles/><office:automatic-styles><style:page-layout style:name=\"PM1\"><style:page-layout-properties fo:page-width=\"29.7cm\" fo:page-height=\"21cm\"/></style:page-layout></office:automatic-styles><office:master-styles><draw:layer-set><draw:layer draw:name=\"layout\"/><draw:layer draw:name=\"background\"/><draw:layer draw:name=\"backgroundobjects\"/><draw:layer draw:name=\"controls\"/><draw:layer draw:name=\"measurelines\"/></draw:layer-set><style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"/></office:master-styles></office:document-styles>"
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

/// **A trip whose only difference is DiaCraft's own carrier is the same content** (the second round): the writer gives the shapes of the second page styles of their
/// own, copies of the style the pages share, and writes DiaCraft's attributes on them; the attributes count as nothing when only the written file has them, and the copies of a style count as
/// one, so the file the trip wrote is the content of the one it started from, and the line says so.
#[test]
fn an_odg_trip_whose_only_difference_is_diacrafts_carrier_is_the_same_content() {
    let dir = TempDir::new("via-carrier");
    let input = dir.write("in.odg", &two_pages_odg());
    let output = dir.path("out.odg");
    let (report, out) = visit(&input, &output, &[]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(report["refusal"], Value::Null, "the trip finished: {}", shown(&report));
    // The premise: the file that came back does hold the carrier, so that the check had something to forgive.
    let written = std::fs::read(&output).unwrap();
    let package = diacraft_odg::package::Package::read(&written, &diacraft_odg::Limits::default()).unwrap();
    let content = String::from_utf8(package.part("content.xml").unwrap().to_vec()).unwrap();
    let styles = String::from_utf8(package.part("styles.xml").unwrap().to_vec()).unwrap();
    assert!(
        content.contains("diacraft:") || styles.contains("diacraft:") || package.entries().any(|e| e.name.starts_with("DiaCraft/")),
        "the file holds DiaCraft's own carrier"
    );
    assert_eq!(report["content"]["compared"], true, "{}", shown(&report));
    assert_eq!(report["content"]["same"], true, "{}", shown(&report));
    assert_eq!(report["content"]["count"], 0, "{}", shown(&report));
    // The same through `compare --content`, with the source first.
    let (line, status) = line_of(&["compare", "--content", s(&input), s(&output)]);
    assert!(status.status.success());
    assert_eq!(line["content"]["same"], true, "{}", shown(&line));
}

// ------------------------------------------------------------------------------------------------------------------------ the scratch folder

/// Runs the visit with `TMPDIR` set to `tmp`, as the child process sees the system's temporary folder.
fn visit_in(tmp: &Path, input: &Path, output: &Path, extra: &[&str]) -> Output {
    let mut command = std::process::Command::new(BIN);
    command.args(["roundtrip", s(input), "--out", s(output), "--via", "diacraft"]).args(extra).env("TMPDIR", tmp).env_remove("RUST_LOG");
    command.output().unwrap()
}

/// The scratch folder of a run is removed when the run ends, whatever ended it: a trip to its end, a leg the engine refused, an edit that could not be made, a file that did not open.
#[test]
fn the_scratch_folder_is_removed_whatever_ended_the_run() {
    let dir = TempDir::new("via-scratch");
    let tmp = dir.path("tmp");
    std::fs::create_dir(&tmp).unwrap();
    let good = dir.write("good.vsdx", &drawing());
    let empty = dir.write("empty.vsdx", &empty_drawing());
    let bad = dir.write("bad.vsdx", b"nope");
    for (input, extra) in [(&good, &[][..]), (&good, &["--edit", "delete"]), (&empty, &["--edit", "cell"]), (&bad, &[])] {
        let run = visit_in(&tmp, input, &dir.path("o.vsdx"), extra);
        assert!(run.status.success(), "{}", text(&run.stderr));
        let left: Vec<_> = std::fs::read_dir(&tmp).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert!(left.is_empty(), "the scratch folder was left behind: {left:?}");
    }
}

/// A temporary folder that is not there is a failure in words, with nothing written; the `--out` is not made.
#[test]
fn a_temporary_folder_that_is_not_there_is_refused_in_words() {
    let dir = TempDir::new("via-notmp");
    let input = dir.write("in.vsdx", &drawing());
    let output = dir.path("o.vsdx");
    let run = visit_in(&dir.path("missing"), &input, &output, &[]);
    assert_eq!(run.status.code(), Some(1));
    assert!(text(&run.stderr).contains("cannot make a scratch folder"), "{}", text(&run.stderr));
    assert!(!output.exists());
}

// ------------------------------------------------------------------------------------------------------------------------ compare --content

/// A file against itself is the same content, in the one shape of a content line.
#[test]
fn compare_content_of_a_file_with_itself_is_the_same() {
    let dir = TempDir::new("cc-self");
    let file = dir.write("a.vsdx", &drawing());
    let (report, out) = line_of(&["compare", "--content", s(&file), s(&file)]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(report["format"], "vsdx");
    assert_eq!(report["first"], report["second"]);
    assert_eq!(report["first"]["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(report["content"], json!({ "compared": true, "same": true, "count": 0, "differences": [], "class": null }), "{}", shown(&report));
    assert!(!text(&out.stdout).contains("Secret"), "{}", text(&out.stdout));
}

/// A planted difference is found where it was planted, by part, path and kind, and counted exactly; the names of the file are not in the line.
#[test]
fn compare_content_names_a_planted_difference_by_part_path_and_kind() {
    let dir = TempDir::new("cc-diff");
    let a = dir.write("a.vsdx", &drawing());
    let edited = dir.path("b.vsdx");
    let (open, edit, save) = (
        json!({ "path": s(&a) }).to_string(),
        json!({ "ids": [1], "page": 1, "weight": 1.23 }).to_string(),
        json!({ "path": s(&edited) }).to_string(),
    );
    let run = cli(
        &["run", "--cmd", "file.open", "--params", &open, "--cmd", "shape.format", "--params", &edit, "--cmd", "file.save_as", "--params", &save],
        None,
    );
    assert!(run.status.success(), "{}", text(&run.stderr));
    let (report, out) = line_of(&["compare", "--content", s(&a), s(&edited)]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let content = &report["content"];
    assert_eq!(content["compared"], true);
    assert_eq!(content["same"], false);
    let differences = content["differences"].as_array().unwrap();
    assert_eq!(content["count"].as_u64(), Some(differences.len() as u64));
    assert!(differences.iter().any(|d| d["part"] == "/visio/pages/page1.xml" && d["found"] == "value"), "{}", shown(&report));
    for difference in differences {
        assert!(difference["path"].is_string() && difference["found"].is_string() && difference["part"].is_string(), "{difference}");
    }
    assert!(!text(&out.stdout).contains("Secret"), "{}", text(&out.stdout));
    // The count is symmetric.
    let (back, _) = line_of(&["compare", "--content", s(&edited), s(&a)]);
    assert_eq!(back["content"]["count"], content["count"]);
}

/// A part whose name a person chose is told by its role, and never by its name: a changed part of the file that holds a name.
#[test]
fn compare_content_tells_a_part_a_person_named_by_its_role() {
    let dir = TempDir::new("cc-names");
    let first = drawing();
    let second = VsdxBuilder::new()
        .page(PageSpec::new(1, "Secret page name").sheet(r#"<Cell N="PageWidth" V="8.5"/>"#).shapes(&(1..=3).map(typical_shape).collect::<String>()))
        .master(MasterSpec::new(1, "Secret master name").shapes(&shape(4, &cell("Width", "1"))))
        .with(Pkg::new().part_without_override("/foo/Secret name.dat", b"changed!!!").default_type("dat", "application/x-foo"))
        .build();
    let (a, b) = (dir.write("a.vsdx", &first), dir.write("b.vsdx", &second));
    let (report, out) = line_of(&["compare", "--content", s(&a), s(&b)]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(
        report["content"]["differences"].as_array().unwrap().iter().any(|d| d["part"] == "a part" && d["found"] == "bytes"),
        "{}",
        shown(&report)
    );
    assert!(!text(&out.stdout).contains("Secret"), "{}", text(&out.stdout));
}

/// Two files of two formats are refused in words, with the exit status 1 and nothing on standard output: the check compares two files of one format.
#[test]
fn compare_content_of_two_formats_is_refused_in_words() {
    let dir = TempDir::new("cc-formats");
    let vsdx = dir.write("a.vsdx", &drawing());
    let odg = dir.write("b.odg", &crate::odg_save::drawing());
    for (a, b) in [(&vsdx, &odg), (&odg, &vsdx)] {
        let run = cli(&["compare", "--content", s(a), s(b)], None);
        assert_eq!(run.status.code(), Some(1), "{}", text(&run.stderr));
        assert!(run.stdout.is_empty(), "no line, so nothing says same: {}", text(&run.stdout));
        let stderr = text(&run.stderr);
        assert!(
            stderr.contains("different formats") && stderr.contains("vsdx") && stderr.contains("odg") && stderr.contains("one format"),
            "{stderr}"
        );
    }
    // The same two `.odg` files are compared by the lists of their format, and are the same.
    let (report, out) = line_of(&["compare", "--content", s(&odg), s(&odg)]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(report["format"], "odg");
    assert_eq!(report["content"]["same"], true, "{}", shown(&report));
}

/// A `.diacraft` file has no content check of its own: it is refused in words, and so is a file that cannot be read from disk.
#[test]
fn compare_content_refuses_a_diacraft_file_and_a_missing_file_in_words() {
    let dir = TempDir::new("cc-native");
    let vsdx = dir.write("a.vsdx", &drawing());
    let native = dir.path("n.diacraft");
    let (open, save) = (json!({ "path": s(&vsdx) }).to_string(), json!({ "path": s(&native) }).to_string());
    assert!(cli(&["run", "--cmd", "file.open", "--params", &open, "--cmd", "file.save_as", "--params", &save], None).status.success());
    let run = cli(&["compare", "--content", s(&native), s(&native)], None);
    assert_eq!(run.status.code(), Some(1));
    assert!(text(&run.stderr).contains("a .diacraft file has no content check of its own"), "{}", text(&run.stderr));
    assert!(run.stdout.is_empty());
    let run = cli(&["compare", "--content", s(&vsdx), s(&dir.path("missing.vsdx"))], None);
    assert_eq!(run.status.code(), Some(1));
    assert!(text(&run.stderr).contains("cannot read the file"), "{}", text(&run.stderr));
}

/// A file that is not a package is a report with the class of the refusal and the exit status 0, and says neither same nor different.
#[test]
fn compare_content_of_a_file_that_is_not_a_package_is_a_report_with_the_class() {
    let dir = TempDir::new("cc-bad");
    let good = dir.write("a.vsdx", &drawing());
    let bad = dir.write("b.vsdx", b"not a package");
    let (report, out) = line_of(&["compare", "--content", s(&good), s(&bad)]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(report["content"]["compared"], false, "{}", shown(&report));
    assert_eq!(report["content"]["same"], Value::Null);
    assert!(report["content"]["class"].as_str().is_some_and(|c| !c.is_empty()), "{}", shown(&report));
}

// ------------------------------------------------------------------------------------------------------------------------ export and run

/// `export` reads a `.diacraft` as `file.open` does (the plan's `export_reads_a_diacraft`): a picture of the page of a `.diacraft` made from a VSDX package is the one exported from the
/// package, for every format and for a page that is not the first.
#[test]
fn export_reads_a_diacraft() {
    let dir = TempDir::new("export-native");
    let sheet = r#"<Cell N="PageWidth" V="4"/><Cell N="PageHeight" V="3"/><Cell N="PageScale" V="1"/><Cell N="DrawingScale" V="1"/>"#;
    let boxed = shape(
        1,
        r##"<Cell N="PinX" V="1"/><Cell N="PinY" V="1"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="0"/><Cell N="LocPinY" V="0"/>
<Cell N="FillPattern" V="1"/><Cell N="FillForegnd" V="#00FF00"/><Cell N="LinePattern" V="0"/>
<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="3"><Cell N="X" V="2"/><Cell N="Y" V="1"/></Row><Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="1"/></Row><Row T="LineTo" IX="5"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>"##,
    );
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "First").sheet(sheet)).page(PageSpec::new(2, "Second").sheet(sheet).shapes(&boxed)).build();
    let vsdx = dir.write("d.vsdx", &bytes);
    let native = dir.path("d.diacraft");
    let (open, save) = (json!({ "path": s(&vsdx) }).to_string(), json!({ "path": s(&native) }).to_string());
    assert!(cli(&["run", "--cmd", "file.open", "--params", &open, "--cmd", "file.save_as", "--params", &save], None).status.success());
    for extension in ["svg", "png", "pdf"] {
        let from = |file: &Path, name: &str| {
            let out = dir.path(name);
            let run = cli(&["export", s(file), "--page", "2", "--out", s(&out), "--bundled-fonts"], None);
            assert!(run.status.success(), "{extension}: {}", text(&run.stderr));
            (json_lines(&run.stdout).remove(0), std::fs::read(&out).unwrap())
        };
        let (line, picture) = from(&native, &format!("n.{extension}"));
        let (reference, expected) = from(&vsdx, &format!("v.{extension}"));
        assert_eq!(line["page"]["name"], "Second", "{extension}: the page asked for");
        assert_eq!((line["width"].clone(), line["height"].clone()), (reference["width"].clone(), reference["height"].clone()), "{extension}");
        if extension == "pdf" {
            // A PDF names the file's own producer and date: its pages are what is compared.
            assert!(picture.starts_with(b"%PDF") && expected.starts_with(b"%PDF"));
        } else {
            assert!(picture == expected, "{extension}: the same picture as the VSDX file it came from");
        }
    }
    assert!(std::fs::read_to_string(dir.path("n.svg")).unwrap().contains("#00ff00"), "the box of the second page is drawn");
}

/// `run --cmd file.open` and `file.save_as` work with `.diacraft` (the plan's `run_opens_and_saves_a_diacraft`): a VSDX file saved as `.diacraft`, the `.diacraft` opened in a run of its
/// own, its status the format's, and saved again as another `.diacraft` that is the same bytes (a second save of what has not changed).
#[test]
fn run_opens_and_saves_a_diacraft() {
    let dir = TempDir::new("run-native");
    let vsdx = dir.write("d.vsdx", &drawing());
    let (first, second) = (dir.path("one.diacraft"), dir.path("two.diacraft"));
    let (open, save) = (json!({ "path": s(&vsdx) }).to_string(), json!({ "path": s(&first) }).to_string());
    let run = cli(&["run", "--cmd", "file.open", "--params", &open, "--cmd", "file.save_as", "--params", &save, "--cmd", "file.status"], None);
    assert!(run.status.success(), "{}", text(&run.stderr));
    let lines = json_lines(&run.stdout);
    assert_eq!(lines[1]["result"]["variant"], "diacraft", "{lines:?}");
    assert!(lines[1]["result"]["format"].as_str().is_some_and(|v| v.starts_with("1.")), "the version of the format: {lines:?}");
    assert_eq!(lines[2]["result"]["format"], "diacraft", "the working file is the `.diacraft` it was saved as: {lines:?}");
    assert_eq!(lines[2]["result"]["kind"], "drawing");
    let written = std::fs::read(&first).unwrap();
    assert_eq!(lines[1]["result"]["bytes"], written.len());
    assert!(written.starts_with(b"PK") && written.windows(26).any(|w| w == b"application/vnd.diacraft.d"), "a `.diacraft` package");
    // Opened in a run of its own, and saved as another `.diacraft`.
    let (open, save) = (json!({ "path": s(&first) }).to_string(), json!({ "path": s(&second) }).to_string());
    let run = cli(&["run", "--cmd", "file.open", "--params", &open, "--cmd", "file.status", "--cmd", "file.save_as", "--params", &save], None);
    assert!(run.status.success(), "{}", text(&run.stderr));
    let lines = json_lines(&run.stdout);
    assert_eq!(lines[1]["result"]["format"], "diacraft", "{lines:?}");
    assert_eq!(lines[1]["result"]["dirty"], false);
    assert_eq!(lines[2]["result"]["variant"], "diacraft");
    assert_eq!(std::fs::read(&second).unwrap(), written, "a drawing opened and saved with nothing changed is the same file");
}

/// A stencil and a template make the trip as what they are: opened as the file itself (a template is not a new drawing here), told by the type they came in, and sent to the `.diacraft`
/// type of their own kind; the output is saved in the type `--out` names. (`native_extension`'s own test is where the type of the intermediate file is seen.)
#[test]
fn a_stencil_and_a_template_are_visited_as_the_files_they_are() {
    let dir = TempDir::new("via-kinds");
    for (name, content_type, variant) in
        [("in.vssx", "application/vnd.ms-visio.stencil.main+xml", "vssx"), ("in.vstx", "application/vnd.ms-visio.template.main+xml", "vstx")]
    {
        let shapes: String = (1..=2).map(typical_shape).collect();
        let bytes = VsdxBuilder::new().document_type(content_type).page(PageSpec::new(1, "Secret page name").shapes(&shapes)).build();
        let input = dir.write(name, &bytes);
        let output = dir.path(&format!("out.{variant}"));
        let (report, out) = visit(&input, &output, &[]);
        assert!(out.status.success(), "{name}: {}", text(&out.stderr));
        assert_eq!(report["opened"], true, "{}", shown(&report));
        assert_eq!(report["variant"], variant, "{}", shown(&report));
        assert!(report["diacraft"]["bytes"].as_u64().is_some_and(|b| b > 0), "{name}: the `.diacraft` file was made: {}", shown(&report));
        assert!(!text(&out.stdout).contains("Secret"), "{}", text(&out.stdout));
        // The trip is made to its end, in the type the file came in, and the content is the same.
        assert_eq!(report["refusal"], Value::Null, "{name}: {}", shown(&report));
        assert_eq!(report["content"]["same"], true, "{name}: {}", shown(&report));
        assert_eq!(report["model"]["same"], true, "{name}: {}", shown(&report));
        assert!(output.exists(), "{name}");
    }
}

/// **The line is an instrument, so it is one shape**: the same keys, at every level a reader looks at, whatever ended the trip (a file that did not open, an edit that could not be made, a
/// leg the engine refused, a trip made to its end), and the same keys inside `content`, `model`, `input` and a refusal. A harness that reads `content.same` finds it.
#[test]
fn the_line_has_one_shape_whatever_ended_the_trip() {
    let dir = TempDir::new("via-shape");
    let good = dir.write("good.vsdx", &drawing());
    let empty = dir.write("empty.vsdx", &empty_drawing());
    let bad = dir.write("bad.vsdx", b"nope");
    let output = dir.path("out.vsdx");
    let keys = |value: &Value| -> Vec<String> { value.as_object().unwrap().keys().cloned().collect() };
    let top = ["content", "diacraft", "edit", "format", "input", "model", "opened", "output", "refusal", "variant", "via"];
    let mut refusals = 0;
    for (input, extra) in [(&bad, &[][..]), (&empty, &["--edit", "move"]), (&good, &[]), (&good, &["--edit", "add"]), (&good, &["--move", "1,1"])] {
        let (report, out) = visit(input, &output, extra);
        assert!(out.status.success(), "{}", text(&out.stderr));
        assert_eq!(keys(&report), top, "{}", shown(&report));
        assert_eq!(keys(&report["input"]), ["bytes", "sha256"], "{}", shown(&report));
        assert_eq!(keys(&report["content"]), ["class", "compared", "count", "differences", "same"], "{}", shown(&report));
        assert_eq!(report["via"], "diacraft");
        if report["content"]["compared"] == true {
            assert!(report["content"]["same"].is_boolean() && report["content"]["count"].is_u64(), "{}", shown(&report));
            assert_eq!(keys(&report["model"])[0], "differ", "{}", shown(&report));
            assert!(report["refusal"].is_null() && report["output"]["bytes"].is_u64(), "{}", shown(&report));
        } else {
            refusals += 1;
            assert_eq!(keys(&report["refusal"]), ["error", "leg", "problem", "words"], "{}", shown(&report));
            assert_never_same(&report);
        }
    }
    assert!(refusals >= 2, "the refusals of the open and edit legs are among the lines");
}
