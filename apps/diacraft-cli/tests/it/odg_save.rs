//! The command line and the MCP server over an `.odg`: `run` opens one, edits it and saves it through the engine's commands, a change the
//! writer does not write is a refused save in the writer's own words, and `export` draws one.
//!
//! The drawing is written by hand from the ODF standard and put together by the container's writer, as `odg_render.rs` does.

use diacraft_container::zip::{ArchiveWriter, Method};
use serde_json::{Value, json};
use std::io::Write;
use std::process::{Command, Stdio};

use crate::{BIN, TempDir, cli, json_lines, s, text};

pub(crate) const DRAWING: &str = "application/vnd.oasis.opendocument.graphics";
pub(crate) const NS: &str = concat!(
    r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
    r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
    r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0""#
);

/// A one-page A4 drawing of two rectangles, A (red, at 2 cm and 3 cm from the top left, 4 by 2) and B (red, at 12 and 3), on the style `red`.
pub(crate) fn drawing() -> Vec<u8> {
    let shape = |name: &str, x: u32| {
        format!(
            "<draw:custom-shape draw:style-name=\"red\" draw:layer=\"layout\" draw:name=\"{name}\" svg:width=\"4cm\" svg:height=\"2cm\" svg:x=\"{x}cm\" svg:y=\"3cm\"><text:p/><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"non-primitive\" draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N\"/></draw:custom-shape>"
        )
    };
    let content = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-content {NS} office:version=\"1.3\"><office:automatic-styles><style:style style:name=\"red\" style:family=\"graphic\"><style:graphic-properties draw:fill=\"solid\" draw:fill-color=\"#ff0000\" draw:stroke=\"none\"/></style:style></office:automatic-styles><office:body><office:drawing><draw:page draw:name=\"p\" draw:master-page-name=\"Default\">{}{}</draw:page></office:drawing></office:body></office:document-content>",
        shape("A", 2),
        shape("B", 12)
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

/// The commands that make a curved connector on `drawing()`: an ellipse dropped, a connector from A to it and the connector made curved. Each is `(command, params)`. (The `.odg` save
/// refused it, in words, until a later change: the engine draws it as a quadratic spline, and the writer writes each piece of it as the cubic it is.)
fn curved_connector() -> Vec<(&'static str, String)> {
    vec![
        ("shape.drop", json!({"master": "ellipse", "page": 0, "x": 4.0, "y": 2.0}).to_string()),
        ("connector.add", json!({"from": {"id": 1}, "to": {"id": 3}}).to_string()),
        ("connector.style", json!({"style": "curved", "ids": [4]}).to_string()),
    ]
}

/// The route of the connector the file holds: its `svg:d`.
fn route_of(bytes: &[u8]) -> String {
    let package = diacraft_odg::package::Package::read(bytes, &diacraft_odg::Limits::default()).unwrap();
    let content = String::from_utf8(package.part("content.xml").unwrap().to_vec()).unwrap();
    let at = content.find("<draw:connector ").expect("a connector");
    let tag = &content[at..content[at..].find('>').map(|n| at + n).unwrap()];
    tag.split("svg:d=\"").nth(1).and_then(|r| r.split('"').next()).expect("the route is a path").to_owned()
}

/// What `diacraft_odg` reads of the file: the `x` of each shape in centimetres (a shape's `PinX` less half its width, in inches, from the left).
fn lefts(bytes: &[u8]) -> Vec<f64> {
    let (doc, _) = diacraft_odg::read(bytes, &diacraft_odg::Limits::default()).unwrap();
    doc.pages
        .get(0)
        .unwrap()
        .shapes
        .iter()
        .map(|shape| {
            let number = |name: &str| shape.sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap();
            (number("PinX") - number("Width") / 2.0) * 2.54
        })
        .collect()
}

#[test]
fn run_opens_an_odg_moves_a_shape_and_saves_it_as_an_odg() {
    let dir = TempDir::new("odg-run");
    let path = dir.write("a.odg", &drawing());
    let before = std::fs::read(&path).unwrap();
    assert_eq!(lefts(&before).iter().map(|x| x.round()).collect::<Vec<_>>(), [2.0, 12.0]);
    let move_a = json!({"ids": [1], "dx": 1.0, "dy": 0.0}).to_string();
    let open = json!({"path": s(&path)}).to_string();
    let out = cli(&["run", "--cmd", "file.open", "--params", &open, "--cmd", "shape.move", "--params", &move_a, "--cmd", "file.save"], None);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert_eq!(lines[0]["result"]["file"]["variant"], "odg", "{}", lines[0]);
    assert_eq!(lines[2]["result"]["variant"], "odg");
    assert_eq!(lines[2]["result"]["edited"], true);
    let after = std::fs::read(&path).unwrap();
    assert_ne!(after, before);
    let moved = lefts(&after);
    assert!((moved[0] - (2.0 + 2.54)).abs() < 1e-6 && (moved[1] - 12.0).abs() < 1e-6, "A moved an inch and B did not: {moved:?}");
}

#[test]
fn run_saves_a_curved_connector_in_an_odg_as_a_path_and_says_nothing_against_it() {
    let dir = TempDir::new("odg-curved");
    let path = dir.write("a.odg", &drawing());
    let before = std::fs::read(&path).unwrap();
    let open = json!({"path": s(&path)}).to_string();
    let mut args = vec!["run", "--cmd", "file.open", "--params", &open];
    let steps = curved_connector();
    for (command, params) in &steps {
        args.extend(["--cmd", command, "--params", params]);
    }
    args.extend(["--cmd", "file.save"]);
    let out = cli(&args, None);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 5, "{lines:?}");
    assert!(lines[3]["result"]["warnings"].is_null(), "making a curved connector warns of nothing: {}", lines[3]);
    let after = std::fs::read(&path).unwrap();
    assert_ne!(after, before, "the file was written");
    assert!(route_of(&after).contains('c'), "the curve is written as cubics: {}", route_of(&after));
}

#[test]
fn run_saves_an_odg_as_a_vsdx_type_across_the_formats_and_a_name_of_no_type_is_refused_in_words() {
    let dir = TempDir::new("odg-type");
    let path = dir.write("a.odg", &drawing());
    let open = json!({"path": s(&path)}).to_string();
    // A save across the formats is written, and the result says what the file carries.
    let target = json!({"path": s(&dir.path("a.vsdx"))}).to_string();
    let out = cli(&["run", "--cmd", "file.open", "--params", &open, "--cmd", "file.save_as", "--params", &target], None);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(dir.path("a.vsdx").exists());
    let lines = json_lines(&out.stdout);
    assert!(lines[1]["result"]["carried"]["classes"].is_array(), "{lines:?}");
    // A name that is no type is still refused, in words, and nothing is written.
    let target = json!({"path": s(&dir.path("a.txt"))}).to_string();
    let out = cli(&["run", "--cmd", "file.open", "--params", &open, "--cmd", "file.save_as", "--params", &target], None);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("the file name must end in"), "{}", text(&out.stderr));
    assert!(!dir.path("a.txt").exists());
}

/// The MCP server answers the same: the curved connector's commands say nothing against it, and the save is made.
#[test]
fn mcp_saves_a_curved_connector_in_an_odg_as_a_path() {
    let dir = TempDir::new("odg-mcp");
    let path = dir.write("a.odg", &drawing());
    let request = |id: u64, method: &str, params: Value| format!("{}\n", json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
    let call = |id: u64, tool: &str, arguments: Value| request(id, "tools/call", json!({"name": tool, "arguments": arguments}));
    let mut input = vec![
        request(1, "initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}})),
        format!("{}\n", json!({"jsonrpc": "2.0", "method": "notifications/initialized"})),
        call(2, "command_run", json!({"id": "file.open", "params": {"path": s(&path)}})),
    ];
    for (n, (command, params)) in curved_connector().iter().enumerate() {
        let params: Value = serde_json::from_str(params).unwrap();
        input.push(call(3 + n as u64, "command_run", json!({"id": command, "params": params})));
    }
    input.push(call(6, "command_run", json!({"id": "file.save"})));
    let input = input.concat();
    let mut child = Command::new(BIN)
        .args(["mcp", "--headless"])
        .env_remove("RUST_LOG")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let replies = json_lines(&out.stdout);
    assert_eq!(replies.len(), 6, "{replies:?}");
    let styled: Value = serde_json::from_str(replies[4]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(styled["warnings"].is_null(), "{styled}");
    let saved = &replies[5]["result"];
    assert!(saved["isError"].is_null() || saved["isError"] == false, "{saved}");
    assert!(route_of(&std::fs::read(&path).unwrap()).contains('c'));
}

/// `export` draws an `.odg` as it draws a VSDX file: the package's own type decides which reader reads it.
#[test]
fn export_draws_an_odg() {
    let dir = TempDir::new("odg-export");
    let path = dir.write("a.odg", &drawing());
    let out = dir.path("a.svg");
    let run = cli(&["export", s(&path), "--page", "1", "--out", s(&out)], None);
    assert!(run.status.success(), "{}", text(&run.stderr));
    let svg = std::fs::read_to_string(&out).unwrap();
    assert!(svg.starts_with("<?xml") || svg.starts_with("<svg"), "{}", &svg[..svg.len().min(80)]);
    assert!(svg.contains("#ff0000") || svg.contains("ff0000") || svg.contains("rgb(255,0,0)"), "the red of the shapes is drawn: {svg}");
    let line = json_lines(&run.stdout);
    assert_eq!(line[0]["format"], "svg");
    assert!(line[0]["width"].as_f64().unwrap() > 0.0);
    // A page the drawing does not have is said in words.
    let bad = cli(&["export", s(&path), "--page", "2", "--out", s(&dir.path("b.svg"))], None);
    assert_eq!(bad.status.code(), Some(1));
    assert!(text(&bad.stderr).contains("the drawing has 1 page: there is no page 2"), "{}", text(&bad.stderr));
}

/// A one-page drawing of one rectangle, `A`, whose style sets no fill and no line: its look is what its style sheets give it, and so the theme's once one is applied.
fn plain_drawing() -> Vec<u8> {
    let content = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-content {NS} office:version=\"1.3\"><office:automatic-styles><style:style style:name=\"plain\" style:family=\"graphic\"><style:graphic-properties/></style:style></office:automatic-styles><office:body><office:drawing><draw:page draw:name=\"p\" draw:master-page-name=\"Default\"><draw:custom-shape draw:style-name=\"plain\" draw:layer=\"layout\" draw:name=\"A\" svg:width=\"4cm\" svg:height=\"2cm\" svg:x=\"2cm\" svg:y=\"3cm\"><text:p/><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"non-primitive\" draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N\"/></draw:custom-shape></draw:page></office:drawing></office:body></office:document-content>"
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

/// The review: `export` opens an `.odg` the way `file.open` does, so that a theme the document file carries is read and a shape that
/// follows it is drawn in the colour the window draws it in (the export read the file with no theme parsed, and a themed colour is a call that answers nothing without one).
#[test]
fn export_draws_a_themed_odg_in_the_colour_the_window_draws_it() {
    let dir = TempDir::new("odg-themed-export");
    let path = dir.write("a.odg", &plain_drawing());
    let mut engine = diacraft_engine::Session::new();
    engine.execute("file.open", &json!({ "path": path })).unwrap();
    engine.execute("theme.apply", &json!({ "theme": "fernwood" })).unwrap();
    engine.execute("file.save", &json!({})).unwrap();
    let doc = engine.document().unwrap();
    let (page, id) = (doc.pages.get(0).unwrap().id, doc.pages.get(0).unwrap().shapes.get(0).unwrap().id);
    let window = engine.execute("shape.formatting", &json!({ "ids": [id], "page": page })).unwrap();
    let fill = window["shapes"][0]["fill"].as_str().unwrap().to_ascii_lowercase();
    assert!(fill.starts_with('#') && fill != "#ffffff", "the theme gives the shape a colour: {window}");
    let out = dir.path("a.svg");
    let run = cli(&["export", s(&path), "--page", "1", "--out", s(&out)], None);
    assert!(run.status.success(), "{}", text(&run.stderr));
    let svg = std::fs::read_to_string(&out).unwrap().to_ascii_lowercase();
    assert!(svg.contains(&fill), "the export draws the window's colour {fill}: {svg}");
}
