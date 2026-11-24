//! A flat OpenDocument drawing (`.fodg`) through the command line and the MCP server. It is **read** by every route (`run` and `mcp`
//! open it through `file.open`, `export` draws it, `roundtrip` and `compare` read it), `file.save` writes it back as a flat file in place (and `roundtrip` writes it again
//! unedited), and `file.save_as` of it is refused, to any name or type. The drawing is written by hand from the standard beside the package of the same two rectangles
//! (`odg_save::drawing`).

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use crate::odg_save::{DRAWING, NS, drawing};
use crate::{BIN, TempDir, cli, json_lines, s, text};

/// The package of `odg_save::drawing`, as one flat document: the same two red rectangles A and B on the master page `Default`, the one `office:automatic-styles` holding the
/// style `red` and the page layout, and `name` as the name of the page.
pub(crate) fn flat_drawing(page: &str) -> Vec<u8> {
    let shape = |name: &str, x: u32| {
        format!(
            "<draw:custom-shape draw:style-name=\"red\" draw:layer=\"layout\" draw:name=\"{name}\" svg:width=\"4cm\" svg:height=\"2cm\" svg:x=\"{x}cm\" svg:y=\"3cm\"><text:p/><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"non-primitive\" draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N\"/></draw:custom-shape>"
        )
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document {NS} office:mimetype=\"{DRAWING}\" office:version=\"1.3\"><office:automatic-styles><style:style style:name=\"red\" style:family=\"graphic\"><style:graphic-properties draw:fill=\"solid\" draw:fill-color=\"#ff0000\" draw:stroke=\"none\"/></style:style><style:page-layout style:name=\"PM1\"><style:page-layout-properties fo:page-width=\"21cm\" fo:page-height=\"29.7cm\" style:print-orientation=\"portrait\"/></style:page-layout></office:automatic-styles><office:master-styles><draw:layer-set><draw:layer draw:name=\"layout\"/></draw:layer-set><style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"/></office:master-styles><office:body><office:drawing><draw:page draw:name=\"{page}\" draw:master-page-name=\"Default\">{}{}</draw:page></office:drawing></office:body></office:document>",
        shape("A", 2),
        shape("B", 12)
    )
    .into_bytes()
}

/// What `diacraft-cli run` printed: one JSON line per step.
fn run_steps(steps: &[(&str, Value)]) -> std::process::Output {
    let owned: Vec<(String, String)> = steps.iter().map(|(command, params)| ((*command).to_owned(), params.to_string())).collect();
    let mut args = vec!["run"];
    for (command, params) in &owned {
        args.extend(["--cmd", command, "--params", params]);
    }
    cli(&args, None)
}

/// The same two files opened by `run` give the same document and the same words, and say which form each was.
#[test]
fn run_opens_a_fodg_as_it_opens_its_package_twin() {
    let dir = TempDir::new("fodg-run");
    let flat = dir.write("a.fodg", &flat_drawing("p"));
    let package = dir.write("a.odg", &drawing());
    let mut opened = Vec::new();
    for path in [&flat, &package] {
        let out = run_steps(&[("file.open", json!({"path": s(path)})), ("document.inspect", json!({}))]);
        assert!(out.status.success(), "{}", text(&out.stderr));
        let lines = json_lines(&out.stdout);
        assert_eq!(lines.len(), 2, "{lines:?}");
        opened.push((lines[0]["result"].clone(), lines[1]["result"].clone()));
    }
    assert_eq!(opened[0].0["file"]["form"], "flat");
    assert_eq!(opened[1].0["file"]["form"], "package");
    for key in ["variant", "kind", "new_from_template"] {
        assert_eq!(opened[0].0["file"][key], opened[1].0["file"][key], "file.{key}");
    }
    // A flat drawing says at the open what Save does with it: the one note, and nothing else is different.
    assert_eq!(opened[0].0["file"]["warning_count"], opened[1].0["file"]["warning_count"].as_u64().unwrap() + 1);
    assert!(opened[0].0["file"]["warnings"][0].as_str().unwrap().contains("flat OpenDocument file (.fodg)"), "{}", opened[0].0["file"]);
    assert_eq!(opened[0].0["pages"], opened[1].0["pages"]);
    assert_eq!(opened[0].1["pages"], opened[1].1["pages"]);
}

/// The left of each shape of a flat drawing, in centimetres, as the `odg` crate reads it (a shape's `PinX` less half its width, in inches).
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

/// `run` opens a flat drawing, moves a shape and saves it back as a flat file in place (the second save is no edit, the bytes the same).
/// A Save As of it to a `.fodg` is refused in words, with nothing written, and to an `.odg` or a VSDX type it is written, the flat file left as it was.
#[test]
fn run_saves_a_fodg_back_as_a_flat_file_and_saves_it_as_the_other_types() {
    let dir = TempDir::new("fodg-run-save");
    let path = dir.write("a.fodg", &flat_drawing("p"));
    let before = std::fs::read(&path).unwrap();
    assert_eq!(lefts(&before).iter().map(|x| x.round()).collect::<Vec<_>>(), [2.0, 12.0]);
    let open = json!({"path": s(&path)});
    let moved = json!({"ids": [1], "dx": 1.0, "dy": 0.0});
    let out = run_steps(&[("file.open", open.clone()), ("shape.move", moved.clone()), ("file.save", json!({})), ("file.save", json!({}))]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 4, "{lines:?}");
    assert_eq!((lines[2]["result"]["form"].clone(), lines[2]["result"]["edited"].clone()), (json!("flat"), json!(true)), "{}", lines[2]);
    assert_eq!((lines[3]["result"]["form"].clone(), lines[3]["result"]["edited"].clone()), (json!("flat"), json!(false)), "{}", lines[3]);
    let after = std::fs::read(&path).unwrap();
    assert_ne!(after, before);
    assert_eq!(diacraft_odg::Form::of(&after), diacraft_odg::Form::Flat);
    let moved_to = lefts(&after);
    assert!((moved_to[0] - (2.0 + 2.54)).abs() < 1e-6 && (moved_to[1] - 12.0).abs() < 1e-6, "A moved an inch and B did not: {moved_to:?}");
    // Nothing edited: Save is the file's own bytes.
    let unedited = run_steps(&[("file.open", open.clone()), ("file.save", json!({}))]);
    assert!(unedited.status.success(), "{}", text(&unedited.stderr));
    assert_eq!(std::fs::read(&path).unwrap(), after, "an unedited save is the file's bytes");
    // Save As to a `.fodg` is refused in words, and nothing is written.
    let on_disk = std::fs::read(&path).unwrap();
    let out = run_steps(&[("file.open", open.clone()), ("shape.move", moved.clone()), ("file.save_as", json!({"path": s(&dir.path("b.fodg"))}))]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("step 2 (file.save_as)"), "{stderr}");
    for part in ["flat", ".fodg", "not written by Save As", "Nothing was written", "as an .odg"] {
        assert!(stderr.contains(part), "a .fodg says {part}: {stderr}");
    }
    assert_eq!(std::fs::read(&path).unwrap(), on_disk, "the file is as it was");
    assert!(!dir.path("b.fodg").exists(), "b.fodg was not written");
    assert_eq!(json_lines(&out.stdout).len(), 2, "the open and the move printed their lines");
    // Save As to an `.odg`, a `.diacraft` file and a VSDX type writes the drawing, with the move, and leaves the flat file as it was.
    for (what, target, kind) in [("an .odg", "b.odg", "odg"), ("a .diacraft file", "b.diacraft", "diacraft"), ("a .vsdx", "b.vsdx", "vsdx")] {
        let out = run_steps(&[("file.open", open.clone()), ("shape.move", moved.clone()), ("file.save_as", json!({"path": s(&dir.path(target))}))]);
        assert!(out.status.success(), "{what}: {}", text(&out.stderr));
        let lines = json_lines(&out.stdout);
        assert_eq!(lines.len(), 3, "{what}: {lines:?}");
        assert_eq!(lines[2]["result"]["variant"], kind, "{what}: {}", lines[2]);
        assert!(dir.path(target).exists(), "{what}: {target} was written");
        assert_eq!(std::fs::read(&path).unwrap(), on_disk, "{what}: the flat file is as it was");
    }
    assert_eq!(diacraft_odg::Form::of(&std::fs::read(dir.path("b.odg")).unwrap()), diacraft_odg::Form::Package, "an .odg is a package");
    let (doc, _) = diacraft_odg::read(&std::fs::read(dir.path("b.odg")).unwrap(), &diacraft_odg::Limits::default()).unwrap();
    assert_eq!(doc.pages.get(0).map(|p| p.shapes.len()), Some(2), "the drawing is in the package");
}

/// `export` draws a flat drawing: by what the file is, whatever it is called, and the picture is the package twin's.
#[test]
fn export_draws_a_fodg_as_it_draws_its_package_twin() {
    let dir = TempDir::new("fodg-export");
    let flat = dir.write("a.fodg", &flat_drawing("p"));
    let package = dir.write("a.odg", &drawing());
    let renamed = dir.write("a.dat", &flat_drawing("p"));
    for ext in ["svg", "pdf", "png"] {
        let (from_flat, from_package, from_renamed) =
            (dir.path(&format!("flat.{ext}")), dir.path(&format!("package.{ext}")), dir.path(&format!("renamed.{ext}")));
        for (input, out) in [(&flat, &from_flat), (&package, &from_package), (&renamed, &from_renamed)] {
            let run = cli(&["export", s(input), "--page", "1", "--out", s(out)], None);
            assert!(run.status.success(), "{ext}: {}", text(&run.stderr));
        }
        let bytes = |p: &std::path::Path| std::fs::read(p).unwrap();
        if ext != "pdf" {
            // A PDF carries its own creation time, so only the picture and the vector file are compared byte for byte.
            assert_eq!(bytes(&from_flat), bytes(&from_package), "{ext}: the picture of the flat file is its package twin's");
        }
        assert_eq!(bytes(&from_flat), bytes(&from_renamed), "{ext}: the name of the file decides nothing");
        assert!(!bytes(&from_flat).is_empty());
    }
    let svg = std::fs::read_to_string(dir.path("flat.svg")).unwrap();
    assert!(svg.contains("ff0000") || svg.contains("rgb(255,0,0)"), "the red of the shapes is drawn: {svg}");
}

/// A hostile flat file is refused by `export` in words (the kind of trouble, no text of the file), and nothing is written.
#[test]
fn export_refuses_a_hostile_fodg_in_words() {
    let dir = TempDir::new("fodg-export-hostile");
    let secret = "SECRET-WORDS";
    let good = String::from_utf8(flat_drawing("p")).unwrap();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("a DOCTYPE", good.replacen("?>\n", &format!("?>\n<!DOCTYPE office:document [<!ENTITY a \"{secret}\">]>\n"), 1).into_bytes()),
        ("a file cut off", good.get(..good.find("<draw:page").unwrap_or(40) + 30).unwrap().as_bytes().to_vec()),
        ("another kind of document", good.replace(DRAWING, "application/vnd.oasis.opendocument.text").into_bytes()),
        (
            "nesting past the depth cap",
            good.replacen("<draw:page ", &format!("{}<draw:page ", "<draw:g>".repeat(5_000)), 1)
                .replacen("</draw:page>", &format!("</draw:page>{}", "</draw:g>".repeat(5_000)), 1)
                .into_bytes(),
        ),
    ];
    for (what, bytes) in cases {
        let path = dir.write("bad.fodg", &bytes);
        let out = dir.path("bad.png");
        let run = cli(&["export", s(&path), "--out", s(&out)], None);
        assert_eq!(run.status.code(), Some(1), "{what}");
        let stderr = text(&run.stderr);
        assert!(stderr.contains("cannot open the drawing") && !stderr.contains(secret) && !stderr.contains(s(&path)), "{what}: {stderr}");
        assert!(!out.exists(), "{what}: nothing was written");
    }
}

/// One JSON-RPC conversation with `diacraft-cli mcp --headless` made of `command_run` calls: the replies to them, in order.
fn mcp_calls(calls: &[(&str, Value)]) -> Vec<Value> {
    let tools: Vec<(&str, Value)> = calls.iter().map(|(command, params)| ("command_run", json!({"id": command, "params": params}))).collect();
    mcp_tools(&tools)
}

/// One JSON-RPC conversation with `diacraft-cli mcp --headless`: each call is a tool and its arguments, and the replies come in order.
fn mcp_tools(calls: &[(&str, Value)]) -> Vec<Value> {
    let request = |id: u64, method: &str, params: Value| format!("{}\n", json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
    let mut input = vec![
        request(1, "initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}})),
        format!("{}\n", json!({"jsonrpc": "2.0", "method": "notifications/initialized"})),
    ];
    for (n, (tool, arguments)) in calls.iter().enumerate() {
        input.push(request(2 + n as u64, "tools/call", json!({"name": tool, "arguments": arguments})));
    }
    let mut child = Command::new(BIN)
        .args(["mcp", "--headless"])
        .env_remove("RUST_LOG")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input.concat().as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let replies = json_lines(&out.stdout);
    assert_eq!(replies.len(), calls.len() + 1, "{replies:?}");
    replies.into_iter().skip(1).collect()
}

/// The MCP server opens a flat drawing as the engine does, says it is flat, saves it back as a flat file in place, writes a Save As of it to an `.odg`, and a Save As of it to a
/// `.fodg` is an `isError` result holding the words, with nothing written.
#[test]
fn mcp_saves_a_fodg_in_place_writes_a_save_as_and_shows_the_refusal_of_a_fodg_in_words() {
    let dir = TempDir::new("fodg-mcp");
    let path = dir.write("a.fodg", &flat_drawing("p"));
    let before = std::fs::read(&path).unwrap();
    let replies = mcp_calls(&[
        ("file.open", json!({"path": s(&path)})),
        ("shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0})),
        ("file.save", json!({})),
        ("file.save_as", json!({"path": s(&dir.path("b.odg"))})),
        ("file.save_as", json!({"path": s(&dir.path("b.fodg"))})),
    ]);
    let opened: Value = serde_json::from_str(replies[0]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!((opened["file"]["variant"].clone(), opened["file"]["form"].clone()), (json!("odg"), json!("flat")), "{opened}");
    assert_eq!(opened["pages"].as_array().map(Vec::len), Some(1));
    assert_ne!(replies[0]["result"]["isError"], true, "{}", replies[0]);
    let saved = &replies[2]["result"];
    assert_ne!(saved["isError"], true, "{saved}");
    let saved: Value = serde_json::from_str(saved["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!((saved["form"].clone(), saved["edited"].clone()), (json!("flat"), json!(true)), "{saved}");
    let after = std::fs::read(&path).unwrap();
    assert_ne!(after, before);
    assert_eq!(diacraft_odg::Form::of(&after), diacraft_odg::Form::Flat);
    let written = &replies[3]["result"];
    assert_ne!(written["isError"], true, "file.save_as to .odg: {written}");
    let written: Value = serde_json::from_str(written["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!((written["variant"].clone(), written["form"].clone()), (json!("odg"), json!("package")), "{written}");
    let refused = &replies[4]["result"];
    assert_eq!(refused["isError"], true, "file.save_as to .fodg: {refused}");
    let words = refused["content"][0]["text"].as_str().unwrap();
    assert!(words.contains("not written by Save As") && words.contains("flat") && words.contains("as an .odg"), "{words}");
    assert!(!words.contains(s(&path)), "{words}");
    assert_eq!(std::fs::read(&path).unwrap(), after, "the Save As wrote nothing over the flat file");
    assert!(dir.path("b.odg").exists() && !dir.path("b.fodg").exists());
}

/// A drawing that is not flat is not written as a flat file either: Save As to a `.fodg` is an `isError` result that says it is not offered.
#[test]
fn mcp_refuses_a_save_as_fodg_of_an_odg_in_words() {
    let dir = TempDir::new("fodg-mcp-package");
    let path = dir.write("a.odg", &drawing());
    let replies = mcp_calls(&[("file.open", json!({"path": s(&path)})), ("file.save_as", json!({"path": s(&dir.path("b.fodg"))}))]);
    let refused = &replies[1]["result"];
    assert_eq!(refused["isError"], true, "{refused}");
    let words = refused["content"][0]["text"].as_str().unwrap();
    assert!(
        words.contains("flat") && words.contains("not written by Save As") && words.contains("as an .odg") && words.contains("Nothing was written"),
        "{words}"
    );
    assert!(!dir.path("b.fodg").exists());
}

fn roundtrip(args: &[&str]) -> (Value, std::process::Output) {
    let mut line = vec!["roundtrip"];
    line.extend_from_slice(args);
    let out = cli(&line, None);
    let report = json_lines(&out.stdout).into_iter().next().unwrap_or(Value::Null);
    (report, out)
}

/// `roundtrip` of a flat drawing reads it with the `odg` crate's reader (not the `not_zip` of a package reader given a document) and writes it again through the flat
/// writer: an unedited save is the file's bytes, so `--out`, which must be a `.fodg`, is the same file and the line says so; no word of the file is in it.
#[test]
fn roundtrip_reads_a_fodg_and_writes_it_again_as_the_same_flat_file() {
    let dir = TempDir::new("fodg-roundtrip");
    let flat = dir.write("a.fodg", &flat_drawing("Secret page"));
    let out = dir.path("out.fodg");
    let (report, run) = roundtrip(&[s(&flat), "--out", s(&out)]);
    assert!(run.status.success(), "{}", text(&run.stderr));
    assert_eq!(report["format"], "odg", "{report}");
    assert_eq!(report["form"], "flat", "{report}");
    assert_eq!(report["opened"], true, "{report}");
    assert_eq!(report["variant"], "odg", "{report}");
    assert_eq!(report["read"]["reads_back"], true, "{report}");
    assert_eq!(report["read"]["pages"], 1, "{report}");
    assert_eq!(report["read"]["warning_count"], 0, "{report}");
    assert_eq!(report["saved"], true, "{report}");
    assert_eq!(report["file_identical"], true, "{report}");
    assert_eq!(report["out_bytes"], std::fs::metadata(&flat).unwrap().len(), "{report}");
    assert_eq!(report["saved_read"]["pages"], 1, "{report}");
    assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(&flat).unwrap(), "the unedited save is the file's bytes");
    assert!(!report.to_string().contains("Secret"), "no word of the file: {report}");
    assert_eq!(report["bytes"], std::fs::metadata(&flat).unwrap().len());
    // `--out` must be a flat file's name: an `.odg` is no way round it, and nothing is written.
    let (to_odg, _) = roundtrip(&[s(&flat), "--out", s(&dir.path("out.odg"))]);
    assert_eq!(to_odg["saved"], false, "{to_odg}");
    assert_eq!(to_odg["save_error"], json!({ "error": "other_type", "want": "fodg" }), "{to_odg}");
    assert!(!dir.path("out.odg").exists());
    // The package twin is copied as before.
    let package = dir.write("a.odg", &drawing());
    let (twin, _) = roundtrip(&[s(&package), "--out", s(&dir.path("twin.odg"))]);
    assert_eq!((twin["opened"].clone(), twin["saved"].clone()), (json!(true), json!(true)), "{twin}");
    assert!(twin.get("form").is_none_or(|f| f == "package"), "{twin}");
    // `--move` cannot be honoured, as for a package: it is said, and the unedited round trip is made all the same.
    let again = dir.path("again.fodg");
    let (moved, _) = roundtrip(&[s(&flat), "--out", s(&again), "--move", "1,0"]);
    assert_eq!(moved["moved"], json!({ "error": "not_supported" }), "{moved}");
    assert_eq!((moved["saved"].clone(), moved["file_identical"].clone()), (json!(true), json!(true)), "{moved}");
    assert!(report.get("moved").is_none(), "no move was asked for: {report}");
}

/// What a flat drawing holds that a person should be told of is in the line as counts and classes: a macro (a linked Basic library), a picture it
/// links and does not hold, and the class of the warning that says so, never its words, and no name or path of the file.
#[test]
fn roundtrip_says_what_a_flat_drawing_holds_by_counts_and_classes() {
    let dir = TempDir::new("fodg-roundtrip-holds");
    let good = String::from_utf8(flat_drawing("p")).unwrap();
    let scripts = "<office:scripts><office:script script:language=\"ooo:Basic\" xmlns:script=\"urn:oasis:names:tc:opendocument:xmlns:script:1.0\"><ooo:libraries xmlns:ooo=\"http://openoffice.org/2004/office\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"><ooo:library-linked ooo:name=\"Secret\" xlink:href=\"Secret.xlb\" ooo:readonly=\"false\"/></ooo:libraries></office:script></office:scripts>";
    let picture = "<draw:frame draw:name=\"F\" svg:width=\"2cm\" svg:height=\"2cm\" svg:x=\"1cm\" svg:y=\"1cm\"><draw:image xmlns:xlink=\"http://www.w3.org/1999/xlink\" xlink:href=\"Secret/plan.png\" xlink:type=\"simple\"/></draw:frame>";
    let holding = good.replacen("<office:automatic-styles>", &format!("{scripts}<office:automatic-styles>"), 1).replacen(
        "</draw:page>",
        &format!("{picture}</draw:page>"),
        1,
    );
    let path = dir.write("holds.fodg", holding.as_bytes());
    let (report, run) = roundtrip(&[s(&path), "--out", s(&dir.path("out.fodg"))]);
    assert!(run.status.success(), "{}", text(&run.stderr));
    assert_eq!(report["opened"], true, "{report}");
    assert_eq!(report["read"]["has_macros"], true, "{report}");
    assert_eq!(report["read"]["linked_pictures"], 1, "{report}");
    // The linked picture, and what the drawing does not hold: a flat drawing's `office:scripts` is read for its macros alone, so the open names it, a head
    // line and a line for its class.
    assert_eq!(report["read"]["warning_count"], 3, "{report}");
    assert_eq!(report["read"]["warnings"], json!(["linked_picture_not_read", "not_kept"]), "{report}");
    assert!(!report.to_string().contains("Secret"), "{report}");
    // The same file beside a plain one: `compare` tells each side by the same counts.
    let plain = dir.write("plain.fodg", &flat_drawing("p"));
    let out = cli(&["compare", s(&plain), s(&path)], None);
    let line = json_lines(&out.stdout).into_iter().next().unwrap();
    assert_eq!((line["first"]["read"]["has_macros"].clone(), line["second"]["read"]["has_macros"].clone()), (json!(false), json!(true)), "{line}");
    assert_eq!(line["second"]["read"]["linked_pictures"], 1, "{line}");
    assert_eq!(line["second"]["read"]["pages"], 1, "{line}");
}

/// A flat file that cannot be read is a report with the class of the refusal and exit status 0, as a package that cannot be read is.
#[test]
fn roundtrip_and_compare_say_the_class_of_a_hostile_fodg_and_nothing_of_it() {
    let dir = TempDir::new("fodg-roundtrip-hostile");
    let secret = "SECRET-WORDS";
    let good = String::from_utf8(flat_drawing("p")).unwrap();
    let doctype = good.replacen("?>\n", &format!("?>\n<!DOCTYPE office:document [<!ENTITY a \"{secret}\">]>\n"), 1);
    let cut = good.get(..good.find("<draw:page").unwrap_or(40) + 30).unwrap().to_owned();
    let deep = good.replacen("<draw:page ", &format!("{}<draw:page ", "<draw:g>".repeat(5_000)), 1).replacen(
        "</draw:page>",
        &format!("</draw:page>{}", "</draw:g>".repeat(5_000)),
        1,
    );
    let other_kind = good.replace(DRAWING, "application/vnd.oasis.opendocument.text");
    for (what, bytes, class) in [
        ("a DOCTYPE", doctype, "xml_entity_refused"),
        ("a file cut off", cut, "xml_malformed"),
        ("nesting past the depth cap", deep, "xml_too_complex"),
        ("another kind of document", other_kind, "not_a_drawing"),
    ] {
        let path = dir.write("bad.fodg", bytes.as_bytes());
        let (report, run) = roundtrip(&[s(&path), "--out", s(&dir.path("out.fodg"))]);
        assert!(run.status.success(), "{what}: {}", text(&run.stderr));
        assert_eq!(report["opened"], false, "{what}: {report}");
        assert_eq!(report["form"], "flat", "{what}: {report}");
        assert_eq!(report["refusal"]["class"], class, "{what}: {report}");
        assert!(!report.to_string().contains(secret), "{what}: {report}");
        assert!(!dir.path("out.fodg").exists());
        let good_path = dir.write("good.fodg", &flat_drawing("p"));
        let compared = cli(&["compare", s(&good_path), s(&path)], None);
        assert!(compared.status.success(), "{what}: {}", text(&compared.stderr));
        let line = json_lines(&compared.stdout).into_iter().next().unwrap();
        assert_eq!((line["first"]["opened"].clone(), line["second"]["opened"].clone()), (json!(true), json!(false)), "{what}: {line}");
        assert_eq!(line["second"]["refusal"]["class"], class, "{what}: {line}");
        assert!(line.get("parts").is_none(), "{what}: nothing is compared when a side does not open: {line}");
        assert!(!line.to_string().contains(secret), "{what}: {line}");
    }
}

/// `compare` reads a flat drawing and a package: each side says what it is, and the comparison that is of two packages' parts is said not to be made.
#[test]
fn compare_reads_a_fodg_and_says_the_parts_are_not_compared() {
    let dir = TempDir::new("fodg-compare");
    let flat = dir.write("a.fodg", &flat_drawing("p"));
    let other = dir.write("b.fodg", &flat_drawing("q"));
    let package = dir.write("a.odg", &drawing());
    for (first, second) in [(&flat, &other), (&flat, &package), (&package, &flat)] {
        let out = cli(&["compare", s(first), s(second)], None);
        assert!(out.status.success(), "{}", text(&out.stderr));
        let line = json_lines(&out.stdout).into_iter().next().unwrap();
        for side in ["first", "second"] {
            assert_eq!(line[side]["opened"], true, "{line}");
            assert_eq!(line[side]["variant"], "odg", "{line}");
        }
        let forms = (line["first"]["form"].clone(), line["second"]["form"].clone());
        assert!(forms.0 == "flat" || forms.1 == "flat", "{line}");
        assert_eq!(line["parts"]["comparable"], false, "{line}");
        assert_eq!(line["parts"]["class"], "flat_not_compared", "{line}");
    }
    // Two packages are compared as before.
    let twin = dir.write("c.odg", &drawing());
    let out = cli(&["compare", s(&package), s(&twin)], None);
    let line = json_lines(&out.stdout).into_iter().next().unwrap();
    assert_eq!(line["parts_identical"], true, "{line}");
}

/// Item 5 had it disabled: MCP's `command_list` says `file.save_as` is enabled for a drawing opened from a flat file, with no reason, as it is
/// for a package, and lists it in `enabled_only` beside `file.save`.
#[test]
fn mcp_lists_file_save_as_as_enabled_for_a_flat_drawing_and_for_a_package() {
    let dir = TempDir::new("fodg-mcp-list");
    let flat = dir.write("a.fodg", &flat_drawing("p"));
    let package = dir.write("a.odg", &drawing());
    let ids = |reply: &Value| -> Vec<String> {
        let list: Value = serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        list.as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap().to_owned()).collect()
    };
    for (what, path) in [("a flat drawing", &flat), ("a package", &package)] {
        let replies = mcp_tools(&[
            ("command_run", json!({"id": "file.open", "params": {"path": s(path)}})),
            ("command_list", json!({"filter": "file.save"})),
            ("command_list", json!({"filter": "file.save", "enabled_only": true})),
        ]);
        let all: Value = serde_json::from_str(replies[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        let save_as = all.as_array().unwrap().iter().find(|c| c["id"] == "file.save_as").unwrap();
        assert_eq!(save_as["enabled"], true, "{what}: {save_as}");
        let usable = ids(&replies[2]);
        assert!(usable.contains(&"file.save".to_owned()), "{what}: {usable:?}");
        assert!(usable.contains(&"file.save_as".to_owned()), "{what}: {usable:?}");
        assert!(save_as.get("disabled_reason").is_none(), "{what}: {save_as}");
    }
}
