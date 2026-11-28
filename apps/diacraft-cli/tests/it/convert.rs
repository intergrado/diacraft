//! `diacraft-cli convert FILE --out PATH`: the binary, run over drawings built in code and over drawings the engine wrote, and the files and the one line it writes.
//!
//! **A convert is a Save As.** A drawing type goes through `file.open` and `file.save_as` in the registry, so every refusal, every warning and the way the file is written (whole beside
//! the target, then renamed over it) is the engine's own, and the tests below pin the engine's words where a refusal is the point. A picture type goes through `export`'s path and its
//! options. The names a person made up in the files here are `Secret`, and no message may repeat one, nor a path (a picture's line names its page, as `export`'s does).

use std::path::Path;
use std::process::Output;

use diacraft_engine::Session;
use diacraft_testkit::package::{MasterSpec, PageSpec, Pkg, VsdxBuilder, ZipItem, shape, typical_drawing, typical_shape, zip};
use serde_json::{Value, json};

use crate::{BIN, TempDir, cli, json_lines, s, text};

const STENCIL: &str = "application/vnd.ms-visio.stencil.main+xml";
const TEMPLATE: &str = "application/vnd.ms-visio.template.main+xml";
const MACRO_DRAWING: &str = "application/vnd.ms-visio.drawing.macroEnabled.main+xml";
const VBA_REL: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProject";

/// Every type `convert` writes, as the help names it.
const DRAWING_TYPES: [&str; 11] = ["diacraft", "diacraft-stencil", "diacraft-template", "vsdx", "vsdm", "vssx", "vssm", "vstx", "vstm", "odg", "otg"];
const PICTURE_TYPES: [&str; 3] = ["png", "svg", "pdf"];

/// A drawing of one page of three shapes (a box with geometry, a text and formula cells each: `typical_shape`) and a master. The names a person made up are `Secret`.
fn vsdx() -> Vec<u8> {
    let sheet = r#"<Cell N="PageWidth" V="4"/><Cell N="PageHeight" V="3"/><Cell N="PageScale" V="1"/><Cell N="DrawingScale" V="1"/>"#;
    let shapes: String = (1..=3).map(typical_shape).collect();
    VsdxBuilder::new()
        .page(PageSpec::new(1, "Secret page name").sheet(sheet).shapes(&shapes))
        .page(PageSpec::new(2, "Second page").sheet(sheet).shapes(&shape(10, "")))
        .master(MasterSpec::new(1, "Secret master name").shapes(&shape(4, r#"<Cell N="Width" V="1"/>"#)))
        .build()
}

/// A stencil of one master.
fn stencil() -> Vec<u8> {
    VsdxBuilder::new().document_type(STENCIL).master(MasterSpec::new(1, "Secret master name").shapes(&shape(4, r#"<Cell N="Width" V="1"/>"#))).build()
}

/// A template of one page.
fn template() -> Vec<u8> {
    let shapes: String = (1..=2).map(typical_shape).collect();
    VsdxBuilder::new().document_type(TEMPLATE).page(PageSpec::new(1, "Secret page name").shapes(&shapes)).build()
}

/// What `convert` printed (the one line, or `Null` when it printed none) and how the process ended.
fn convert(input: &Path, out: &Path, extra: &[&str]) -> (Value, Output) {
    let mut args = vec!["convert", s(input), "--out", s(out)];
    args.extend_from_slice(extra);
    let run = cli(&args, None);
    let line = json_lines(&run.stdout).into_iter().next().unwrap_or(Value::Null);
    (line, run)
}

/// `convert` that is to succeed: its line.
fn converted(input: &Path, out: &Path, extra: &[&str]) -> Value {
    let (line, run) = convert(input, out, extra);
    assert!(run.status.success(), "{}", text(&run.stderr));
    assert_eq!(json_lines(&run.stdout).len(), 1, "one line and nothing else: {}", text(&run.stdout));
    line
}

/// The names of the files in the folder, sorted: what a failed or a finished run left.
fn files_in(dir: &TempDir) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir.path("")).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    names
}

/// The line of `compare --content FIRST SECOND`.
fn content_of(first: &Path, second: &Path) -> Value {
    let run = cli(&["compare", "--content", s(first), s(second)], None);
    assert!(run.status.success(), "{}", text(&run.stderr));
    json_lines(&run.stdout).remove(0)["content"].clone()
}

/// The type `file.open` says a file is (its `variant`), in a session of this process.
fn variant_of(path: &Path) -> Value {
    let mut session = Session::new();
    let opened = session.execute("file.open", &json!({ "path": path, "edit_template": true })).unwrap();
    opened["file"]["variant"].clone()
}

/// A drawing the engine wrote: a new drawing with a rectangle, an ellipse and a second page, saved as `name` (by its extension), through `run`.
fn made_by_the_engine(dir: &TempDir, name: &str) -> std::path::PathBuf {
    let path = dir.path(name);
    let steps = [
        ("file.new", json!({})),
        ("shape.drop", json!({ "master": "rectangle", "page": 1, "x": 2.0, "y": 3.0 })),
        ("shape.drop", json!({ "master": "ellipse", "page": 1, "x": 5.0, "y": 3.0 })),
        ("page.add", json!({ "name": "Second" })),
        ("file.save_as", json!({ "path": path })),
    ];
    let owned: Vec<(String, String)> = steps.iter().map(|(command, params)| ((*command).to_owned(), params.to_string())).collect();
    let mut args = vec!["run"];
    for (command, params) in &owned {
        args.extend(["--cmd", command, "--params", params]);
    }
    let run = cli(&args, None);
    assert!(run.status.success(), "{}", text(&run.stderr));
    path
}

// ------------------------------------------------------------------------------------------------------------------------ the drawing types

/// The brief's content check: a VSDX package goes to `.diacraft` and back, and `compare --content` of the file and what came back finds the same content.
#[test]
fn a_vsdx_drawing_goes_to_diacraft_and_back_with_the_same_content() {
    let dir = TempDir::new("conv-trip");
    let input = dir.write("in.vsdx", &vsdx());
    let native = dir.path("a.diacraft");
    let back = dir.path("back.vsdx");
    let first = converted(&input, &native, &[]);
    assert_eq!((first["in"].clone(), first["format"].clone(), first["out"].clone()), (json!("vsdx"), json!("diacraft"), json!(s(&native))));
    assert_eq!(first["bytes"].as_u64(), Some(std::fs::metadata(&native).unwrap().len()), "{first}");
    let bytes = std::fs::read(&native).unwrap();
    assert!(bytes.starts_with(b"PK") && bytes.windows(26).any(|w| w == b"application/vnd.diacraft.d"), "a .diacraft file");
    let second = converted(&native, &back, &[]);
    assert_eq!((second["in"].clone(), second["format"].clone()), (json!("diacraft"), json!("vsdx")));
    assert_eq!(second["bytes"].as_u64(), Some(std::fs::metadata(&back).unwrap().len()), "{second}");
    let content = content_of(&input, &back);
    assert_eq!((content["compared"].clone(), content["same"].clone(), content["count"].clone()), (json!(true), json!(true), json!(0)), "{content}");
    assert_eq!(files_in(&dir), ["a.diacraft", "back.vsdx", "in.vsdx"], "no temporary file is left beside what was written");
    // The names a person made up are not in a line.
    assert!(!first.to_string().contains("Secret") && !second.to_string().contains("Secret"));
}

/// The same trip for a drawing the engine wrote, which is a `.diacraft` drawing first and a VSDX file from its model.
#[test]
fn a_drawing_the_engine_wrote_goes_through_diacraft_and_back_with_the_same_content() {
    let dir = TempDir::new("conv-engine");
    let written = made_by_the_engine(&dir, "x.vsdx");
    let native = dir.path("y.diacraft");
    let back = dir.path("z.vsdx");
    converted(&written, &native, &[]);
    converted(&native, &back, &[]);
    let content = content_of(&written, &back);
    assert_eq!((content["compared"].clone(), content["same"].clone()), (json!(true), json!(true)), "{content}");
    // A `.diacraft` drawing the engine wrote goes to a VSDX type from its model, and to `.diacraft` again.
    let native_written = made_by_the_engine(&dir, "n.diacraft");
    let line = converted(&native_written, &dir.path("n.vsdx"), &[]);
    assert_eq!((line["in"].clone(), line["format"].clone()), (json!("diacraft"), json!("vsdx")));
    let again = converted(&native_written, &dir.path("n2.diacraft"), &[]);
    assert_eq!((again["in"].clone(), again["format"].clone()), (json!("diacraft"), json!("diacraft")));
}

/// An `.odg` is saved as `.diacraft` by the engine, and the file holds the model the `.odg` read as: opened again, it is an equal model.
#[test]
fn an_odg_goes_to_diacraft_and_reopens_as_an_equal_model() {
    let dir = TempDir::new("conv-odg-native");
    let input = dir.write("in.odg", &crate::odg_save::drawing());
    let native = dir.path("a.diacraft");
    let line = converted(&input, &native, &[]);
    assert_eq!((line["in"].clone(), line["format"].clone()), (json!("odg"), json!("diacraft")));
    let (mut from_odg, mut from_native) = (Session::new(), Session::new());
    from_odg.execute("file.open", &json!({ "path": input })).unwrap();
    from_native.execute("file.open", &json!({ "path": native })).unwrap();
    assert!(from_odg.document().unwrap() == from_native.document().unwrap(), "the model the `.diacraft` file reads as is the `.odg`'s");
    assert_eq!(files_in(&dir), ["a.diacraft", "in.odg"]);
}

/// A VSDX package is written as each of the six VSDX types, and the type of the file is the one its name says: the package's own (its content type).
#[test]
fn a_vsdx_drawing_goes_to_each_of_the_vsdx_types_and_the_type_is_the_names() {
    let dir = TempDir::new("conv-types");
    let input = dir.write("in.vsdx", &vsdx());
    for extension in ["vsdm", "vstx", "vssx", "vssm", "vstm", "vsdx"] {
        let out = dir.path(&format!("out.{extension}"));
        let line = converted(&input, &out, &[]);
        assert_eq!((line["in"].clone(), line["format"].clone()), (json!("vsdx"), json!(extension)), "{line}");
        assert_eq!(line["bytes"].as_u64(), Some(std::fs::metadata(&out).unwrap().len()), "{extension}: {line}");
        assert_eq!(variant_of(&out), json!(extension), "the package is the type its name says");
        // A VSDX save counts its parts, and says nothing of what a `.diacraft` save copies.
        let parts = &line["parts"];
        assert!(["kept", "rewritten", "added", "removed"].iter().all(|k| parts[k].is_u64()), "{extension}: {line}");
        assert!(parts["kept"].as_u64().unwrap() > 0, "an edited save keeps the parts it did not touch: {line}");
        assert_eq!(line["copied"], Value::Null);
    }
    // A stencil keeps its pages and says so, by the class word `roundtrip` tells it by and in words; the warnings are the first 20 as the engine gives them.
    let line = converted(&input, &dir.path("s.vssx"), &[]);
    let told = told_warnings(&line);
    assert_eq!(told.len(), 1, "{line}");
    assert_eq!(told[0].0, "stencil_keeps_pages", "{line}");
    assert!(told[0].1.contains("keeps its pages") && !told[0].1.contains("Secret"), "words of the class, and nothing of the file: {}", told[0].1);
    assert_eq!(line["warning_count"], 1);
    assert_eq!(line["macros_dropped"], 0);
}

/// A template is converted as the file itself and not as a new drawing made from it, and a stencil keeps its kind in `.diacraft`.
#[test]
fn a_template_and_a_stencil_are_converted_as_the_files_they_are() {
    let dir = TempDir::new("conv-kinds");
    let template = dir.write("t.vstx", &template());
    let line = converted(&template, &dir.path("d.vsdx"), &[]);
    assert_eq!((line["in"].clone(), line["format"].clone()), (json!("vstx"), json!("vsdx")));
    let mut session = Session::new();
    session.execute("file.open", &json!({ "path": dir.path("d.vsdx") })).unwrap();
    assert_eq!(session.document().unwrap().pages.len(), 1, "the template's page is in the file: the file itself, not a new drawing");
    let line = converted(&template, &dir.path("t.diacraft-template"), &[]);
    assert_eq!((line["in"].clone(), line["format"].clone()), (json!("vstx"), json!("diacraft-template")));
    assert_eq!(variant_of(&dir.path("t.diacraft-template")), json!("diacraft-template"));
    let stencil = dir.write("s.vssx", &stencil());
    let line = converted(&stencil, &dir.path("s.diacraft-stencil"), &[]);
    assert_eq!((line["in"].clone(), line["format"].clone()), (json!("vssx"), json!("diacraft-stencil")));
    let back = converted(&dir.path("s.diacraft-stencil"), &dir.path("again.vssx"), &[]);
    assert_eq!((back["in"].clone(), back["format"].clone()), (json!("diacraft-stencil"), json!("vssx")));
    assert_eq!(variant_of(&dir.path("again.vssx")), json!("vssx"));
}

/// An `.odg` is saved as an `.odg` (a splice into the file's own bytes), and the content of the copy is the file's.
#[test]
fn an_odg_goes_to_an_odg_with_the_same_content() {
    let dir = TempDir::new("conv-odg");
    let input = dir.write("in.odg", &crate::odg_save::drawing());
    let out = dir.path("out.odg");
    let line = converted(&input, &out, &[]);
    assert_eq!((line["in"].clone(), line["format"].clone()), (json!("odg"), json!("odg")));
    assert_eq!(line["bytes"].as_u64(), Some(std::fs::metadata(&out).unwrap().len()));
    let content = content_of(&input, &out);
    assert_eq!((content["compared"].clone(), content["same"].clone()), (json!(true), json!(true)), "{content}");
}

/// The type is the extension of `--out`, in any case, and the line says it in lower case.
#[test]
fn the_type_comes_from_the_extension_in_any_case() {
    let dir = TempDir::new("conv-case");
    let input = dir.write("in.vsdx", &vsdx());
    let line = converted(&input, &dir.path("A.DiaCraft"), &[]);
    assert_eq!(line["format"], "diacraft");
    let line = converted(&input, &dir.path("B.VSDM"), &[]);
    assert_eq!(line["format"], "vsdm");
    assert_eq!(variant_of(&dir.path("B.VSDM")), json!("vsdm"));
    let line = converted(&input, &dir.path("C.PNG"), &[]);
    assert_eq!(line["format"], "png");
    // The type of the input is not its name's: a file's bytes decide, as for `file.open`.
    let named_wrong = dir.write("looks.png", &vsdx());
    let line = converted(&named_wrong, &dir.path("D.vssx"), &[]);
    assert_eq!(line["in"], "vsdx");
}

/// A file that was there is replaced, whole, and a failed run leaves no temporary file.
#[test]
fn an_existing_file_is_replaced_whole() {
    let dir = TempDir::new("conv-replace");
    let input = dir.write("in.vsdx", &vsdx());
    let out = dir.write("out.diacraft", b"the old file");
    converted(&input, &out, &[]);
    let bytes = std::fs::read(&out).unwrap();
    assert!(bytes.starts_with(b"PK"), "the old file was replaced");
    assert_eq!(files_in(&dir), ["in.vsdx", "out.diacraft"]);
}

// ------------------------------------------------------------------------------------------------------------------------ the picture types

/// One of each picture type: the bytes are `export`'s, with its options, and the line is `export`'s with the types of the input and the output.
#[test]
fn a_picture_is_written_as_export_writes_it_with_its_options() {
    let dir = TempDir::new("conv-pictures");
    let input = dir.write("in.vsdx", &vsdx());
    for (extension, magic) in [("png", &b"\x89PNG\r\n\x1a\n"[..]), ("svg", &b"<?xml"[..]), ("pdf", &b"%PDF-"[..])] {
        let out = dir.path(&format!("p.{extension}"));
        let line = converted(&input, &out, &["--bundled-fonts"]);
        let file = std::fs::read(&out).unwrap();
        assert!(file.starts_with(magic), "{extension}");
        assert_eq!((line["in"].clone(), line["format"].clone(), line["out"].clone()), (json!("vsdx"), json!(extension), json!(s(&out))));
        assert_eq!(line["bytes"].as_u64(), Some(file.len() as u64));
        assert_eq!(line["page"]["name"], "Secret page name");
        assert!(line["width"].is_u64() && line["height"].is_u64() && line["skipped"].is_array(), "{line}");
        // The same page, as `export` writes it.
        let reference = dir.path(&format!("e.{extension}"));
        let exported = cli(&["export", s(&input), "--out", s(&reference), "--bundled-fonts"], None);
        assert!(exported.status.success(), "{}", text(&exported.stderr));
        let export_line = json_lines(&exported.stdout).remove(0);
        for key in ["width", "height", "page", "skipped"] {
            assert_eq!(line[key], export_line[key], "{extension} {key}");
        }
        assert_eq!(file, std::fs::read(&reference).unwrap(), "{extension}: the file export writes");
    }
    // Its options: a page by its place, a resolution, letters as outlines, no paper.
    let line = converted(&input, &dir.path("two.png"), &["--page", "2", "--dpi", "50"]);
    assert_eq!((line["page"]["name"].clone(), line["width"].clone(), line["height"].clone()), (json!("Second page"), json!(200), json!(150)));
    let outlines = converted(&input, &dir.path("o.svg"), &["--text", "outlines", "--transparent", "--bundled-fonts"]);
    let plain = converted(&input, &dir.path("t.svg"), &["--bundled-fonts"]);
    assert!(outlines["bytes"].as_u64() != plain["bytes"].as_u64(), "outlines and a clear page are other bytes");
    let svg = std::fs::read_to_string(dir.path("o.svg")).unwrap();
    assert!(!svg.contains("<text"), "every letter is outlines: {svg}");
}

/// A picture of a drawing the engine wrote, and of an `.odg` and a `.diacraft` file: the input is read as `file.open` reads it.
#[test]
fn a_picture_of_an_odg_and_of_a_diacraft_file() {
    let dir = TempDir::new("conv-pictures-in");
    let odg = dir.write("in.odg", &crate::odg_save::drawing());
    let line = converted(&odg, &dir.path("o.svg"), &["--bundled-fonts"]);
    assert_eq!((line["in"].clone(), line["format"].clone()), (json!("odg"), json!("svg")));
    let native = made_by_the_engine(&dir, "n.diacraft");
    let line = converted(&native, &dir.path("n.png"), &["--bundled-fonts"]);
    assert_eq!((line["in"].clone(), line["format"].clone()), (json!("diacraft"), json!("png")));
    let stencil = dir.write("s.vssx", &stencil());
    // A stencil has no page: a page the drawing lacks, in export's words.
    let (_, run) = convert(&stencil, &dir.path("s.png"), &[]);
    assert_eq!(run.status.code(), Some(1));
    assert!(text(&run.stderr).contains("there is no page 1"), "{}", text(&run.stderr));
}

/// `--page` past the drawing's pages is an error in export's words, and writes nothing.
#[test]
fn a_page_the_drawing_lacks_is_an_error_and_writes_nothing() {
    let dir = TempDir::new("conv-page");
    let input = dir.write("SECRETNAME.vsdx", &vsdx());
    let out = dir.write("p.svg", b"the old picture");
    let (line, run) = convert(&input, &out, &["--page", "3"]);
    assert_eq!(run.status.code(), Some(1));
    let err = text(&run.stderr);
    assert!(err.contains("the drawing has 2 pages: there is no page 3"), "{err}");
    assert!(!err.contains("SECRETNAME") && !err.contains(s(&dir.path(""))), "no path in a message: {err}");
    assert_eq!((line, std::fs::read(&out).unwrap()), (Value::Null, b"the old picture".to_vec()));
}

// ------------------------------------------------------------------------------------------------------------------------ mistakes in the line

/// Each is status 2, says what is wrong in words, runs nothing and writes nothing.
#[test]
fn a_mistake_in_the_line_is_status_two_and_nothing_is_written() {
    let dir = TempDir::new("conv-line");
    let input = dir.write("in.vsdx", &vsdx());
    let out = |name: &str| s(&dir.path(name)).to_owned();
    let cases: Vec<(Vec<String>, &str)> = vec![
        (vec!["convert".into()], "nothing to convert: give the drawing's file"),
        (vec!["convert".into(), s(&input).into()], "nowhere to write: give --out PATH"),
        (vec!["convert".into(), s(&input).into(), "--out".into(), out("a.txt")], "--out must end in"),
        (vec!["convert".into(), s(&input).into(), "--out".into(), out("a")], "--out must end in"),
        (vec!["convert".into(), s(&input).into(), "--out".into(), out("a.fotg")], "--out must end in"),
        (vec!["convert".into(), s(&input).into(), "--out".into(), out("a.fodg")], "--out must end in"),
        (vec!["convert".into(), s(&input).into(), "--out".into(), out("a.vsd")], "--out must end in"),
        (vec!["convert".into(), s(&input).into(), "--out".into(), out("a.vsdx"), "--dpi".into(), "96".into()], "--dpi is for a picture"),
        (vec!["convert".into(), s(&input).into(), "--out".into(), out("a.diacraft"), "--page".into(), "1".into()], "--page is for a picture"),
        (vec!["convert".into(), s(&input).into(), "--out".into(), out("a.odg"), "--text".into(), "outlines".into()], "--text is for a picture"),
        (vec!["convert".into(), s(&input).into(), "--out".into(), out("a.vssx"), "--transparent".into()], "--transparent is for a picture"),
        (vec!["convert".into(), s(&input).into(), "--out".into(), out("a.vstx"), "--bundled-fonts".into()], "--bundled-fonts is for a picture"),
        (vec!["convert".into(), s(&input).into(), "--out".into(), out("a.svg"), "--dpi".into(), "96".into()], "--dpi is for a PNG"),
        (vec!["convert".into(), s(&input).into(), "--out".into(), out("a.png"), "--text".into(), "text".into()], "--text is for an SVG or a PDF"),
        (vec!["convert".into(), s(&input).into(), "--out".into(), out("a.png"), "--page".into(), "0".into()], "--page must be a whole number"),
        (vec!["convert".into(), s(&input).into(), "--out".into(), out("a.png"), "--dpi".into(), "9000".into()], "--dpi must be a number"),
        (vec!["convert".into(), s(&input).into(), "--out".into(), out("a.vsdx"), "--out".into(), out("b.vsdx")], "--out was given twice"),
        (vec!["convert".into(), s(&input).into(), "other.vsdx".into(), "--out".into(), out("a.vsdx")], "takes one drawing"),
        (vec!["convert".into(), s(&input).into(), "--out".into(), out("a.vsdx"), "--unknown".into()], "unknown option `--unknown` for `convert`"),
        (vec!["convert".into(), s(&input).into(), "--out".into()], "--out needs a value"),
    ];
    for (args, words) in cases {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let run = cli(&refs, None);
        assert_eq!(run.status.code(), Some(2), "{args:?}: {}", text(&run.stderr));
        assert!(text(&run.stderr).contains(words), "{args:?}: {}", text(&run.stderr));
        assert!(run.stdout.is_empty(), "{args:?}");
    }
    assert_eq!(files_in(&dir), ["in.vsdx"], "nothing was written");
    // The words of an unknown type name every type that is written.
    let run = cli(&["convert", s(&input), "--out", &out("a.txt")], None);
    let words = text(&run.stderr);
    for extension in DRAWING_TYPES.iter().chain(&PICTURE_TYPES) {
        assert!(words.contains(&format!(".{extension}")), ".{extension}: {words}");
    }
}

// ------------------------------------------------------------------------------------------------------------------------ the engine's refusals

/// A refusal of the engine's Save As passes through in its own words: status 1, nothing on standard output, no file written, an existing file left as it was, and the input untouched.
fn refused(input: &Path, out_name: &str, words: &str) {
    let dir = Path::new(input).parent().unwrap().to_owned();
    let out = dir.join(out_name);
    let before = std::fs::read(input).unwrap();
    let old = std::fs::read(&out).ok();
    let mut listing: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    listing.sort();
    let (line, run) = convert(input, &out, &[]);
    assert_eq!(run.status.code(), Some(1), "{out_name}: {}", text(&run.stderr));
    assert!(text(&run.stderr).contains(words), "{out_name}: the engine's words `{words}` in: {}", text(&run.stderr));
    assert_eq!(line, Value::Null, "nothing on standard output");
    assert_eq!(std::fs::read(&out).ok(), old, "{out_name}: a file that was there is as it was, and none is made");
    assert_eq!(std::fs::read(input).unwrap(), before, "the input is as it was");
    let mut after: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    after.sort();
    assert_eq!(after, listing, "no temporary file is left");
}

/// The `.odg` half of the native format (which lifted Call 18's refusal): a `.diacraft` drawing is converted to an `.odg` (a drawing) and an `.otg` (a template) from the
/// model, and back to `.diacraft`; the `.odg` of the trip back is the content of the first, bar the shapes' own markup until a later change (`compare --content`); an existing file at
/// `--out` is replaced whole.
#[test]
fn a_diacraft_drawing_goes_to_an_odg_and_an_otg_and_back() {
    let dir = TempDir::new("conv-native-odg");
    let native = made_by_the_engine(&dir, "n.diacraft");
    dir.write("old.odg", b"the old file");
    let line = converted(&native, &dir.path("old.odg"), &[]);
    assert_eq!((line["in"].clone(), line["format"].clone(), line["variant"].clone()), (json!("diacraft"), json!("odg"), json!("odg")), "{line}");
    assert!(line["bytes"].as_u64().is_some_and(|n| n > 0) && line["parts"].is_null() && line["copied"].is_null(), "{line}");
    assert_eq!(variant_of(&dir.path("old.odg")), json!("odg"), "the file that was there is replaced whole");
    let template = converted(&native, &dir.path("n.otg"), &[]);
    assert_eq!((template["format"].clone(), template["variant"].clone()), (json!("otg"), json!("otg")), "{template}");
    assert_eq!(variant_of(&dir.path("n.otg")), json!("otg"));
    // Back: the `.odg` is a `.diacraft` drawing that reads as the same picture, and the `.odg` made of it is the content of the first.
    let back = converted(&dir.path("old.odg"), &dir.path("b.diacraft"), &[]);
    assert_eq!((back["in"].clone(), back["format"].clone()), (json!("odg"), json!("diacraft")), "{back}");
    converted(&dir.path("b.diacraft"), &dir.path("again.odg"), &[]);
    let content = content_of(&dir.path("old.odg"), &dir.path("again.odg"));
    assert_eq!(content["compared"], json!(true), "{content}");
    let differences = content["differences"].as_array().unwrap();
    assert!(
        content["same"] == json!(true) || differences.iter().all(|d| d["part"] == json!("content.xml")),
        "only the shapes' own markup differs until the shapes are written back as read: {content}"
    );
    // A template (an `.otg`) is converted as the file itself, to `.odg` too.
    let from_template = converted(&dir.path("n.otg"), &dir.path("t.odg"), &[]);
    assert_eq!((from_template["in"].clone(), from_template["format"].clone()), (json!("otg"), json!("odg")), "{from_template}");
    assert_eq!(files_in(&dir), ["again.odg", "b.diacraft", "n.diacraft", "n.otg", "old.odg", "t.odg"]);
}

#[test]
fn the_crossings_are_written_and_what_the_engine_does_not_write_is_refused_in_its_words() {
    let dir = TempDir::new("conv-refuse-across");
    // **A save across is written**: a VSDX drawing as an `.odg` and an `.otg`, an `.odg` as a VSDX type, and the line says what the file carries.
    let input = dir.write("in.vsdx", &vsdx());
    for (out, format) in [("x.odg", "odg"), ("x.otg", "otg")] {
        let line = converted(&input, &dir.path(out), &[]);
        assert_eq!((line["in"].clone(), line["format"].clone()), (json!("vsdx"), json!(format)), "{line}");
        assert!(line["carried"]["classes"].is_array() && line["carried"]["macros"] == json!(false), "the line has what the file carries: {line}");
    }
    let odg = dir.write("in.odg", &crate::odg_save::drawing());
    let line = converted(&odg, &dir.path("x.vsdx"), &[]);
    assert_eq!((line["in"].clone(), line["format"].clone()), (json!("odg"), json!("vsdx")), "{line}");
    assert!(line["carried"]["classes"].is_array(), "{line}");
    refused(&dir.write("s.vssx", &stencil()), "s.odg", "a stencil is not saved as an .odg");
    refused(&dir.path("s.vssx"), "s.otg", "a stencil is not saved as an .odg");
}

/// **A flat drawing is converted to each type** (a drawing opened from a flat `.fodg` is saved by Save As as a `.diacraft` file, an `.odg` package or a VSDX
/// type, which is what a convert is), and the list of types is the engine's: every type `convert` names is written for it, a `.fodg` is no type of the list (the name is a mistake of the line,
/// status 2, nothing written), and the flat file is left as it was.
#[test]
fn a_flat_drawing_is_converted_to_each_drawing_type_and_never_to_a_fodg() {
    let dir = TempDir::new("conv-flat");
    let flat = dir.write("f.fodg", &crate::odg_flat::flat_drawing("Flat"));
    let before = std::fs::read(&flat).unwrap();
    for extension in DRAWING_TYPES {
        let out = dir.path(&format!("out.{extension}"));
        let line = converted(&flat, &out, &[]);
        assert_eq!((line["in"].clone(), line["format"].clone()), (json!("odg"), json!(extension)), ".{extension}: {line}");
        assert_eq!(variant_of(&out), json!(extension), ".{extension}: the file written is the type that was asked for");
        assert_eq!(std::fs::read(&flat).unwrap(), before, ".{extension}: the flat file is as it was");
    }
    assert_eq!(diacraft_odg::Form::of(&std::fs::read(dir.path("out.odg")).unwrap()), diacraft_odg::Form::Package, "an .odg is a package");
    let (line, run) = convert(&flat, &dir.path("out.fodg"), &[]);
    assert_eq!(run.status.code(), Some(2), "{}: {line}", text(&run.stderr));
    assert!(text(&run.stderr).contains("--out must end in"), "{}", text(&run.stderr));
    assert!(!dir.path("out.fodg").exists(), "a .fodg is not written");
}

/// `--drop-macros` and `--drop-objects` are passed through to the engine's options of the same names, and are a mistake for a picture.
#[test]
fn the_options_of_a_save_across_are_the_engines() {
    let dir = TempDir::new("conv-drop");
    let with_macros = VsdxBuilder::new()
        .document_type(MACRO_DRAWING)
        .page(PageSpec::new(1, "A").sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#))
        .with(Pkg::new().default_type("bin", "application/vnd.ms-office.vbaProject").part_without_override("/visio/vbaProject.bin", b"vba").rel(
            "/visio/document.xml",
            "rId50",
            VBA_REL,
            "vbaProject.bin",
        ))
        .build();
    let input = dir.write("m.vsdm", &with_macros);
    // Without the option a macro-free type refuses the project; with it the project is left out and the line says how many parts.
    let (_, refusal) = convert(&input, &dir.path("m.vsdx"), &[]);
    assert_eq!(refusal.status.code(), Some(1), "{}", text(&refusal.stderr));
    let line = converted(&input, &dir.path("m.vsdx"), &["--drop-macros"]);
    assert_eq!(line["macros_dropped"], 1, "{line}");
    // `--drop-objects` is accepted for a drawing type, and a mistake for a picture (status 2, nothing written).
    let line = converted(&input, &dir.path("m.odg"), &["--drop-objects", "--drop-macros"]);
    assert_eq!(line["carried"]["macros"], json!(false), "{line}");
    let (_, bad) = convert(&input, &dir.path("p.png"), &["--drop-objects"]);
    assert_eq!(bad.status.code(), Some(2), "{}", text(&bad.stderr));
    assert!(!dir.path("p.png").exists());
}

/// A macro project is not written to a type that cannot hold one, and nothing is written.
#[test]
fn a_macro_project_is_refused_where_the_engine_refuses_it() {
    let dir = TempDir::new("conv-macros");
    let with_macros = VsdxBuilder::new()
        .document_type(MACRO_DRAWING)
        .page(PageSpec::new(1, "A"))
        .with(Pkg::new().default_type("bin", "application/vnd.ms-office.vbaProject").part_without_override("/visio/vbaProject.bin", b"vba").rel(
            "/visio/document.xml",
            "rId50",
            VBA_REL,
            "vbaProject.bin",
        ))
        .build();
    let input = dir.write("m.vsdm", &with_macros);
    let (_, run) = convert(&input, &dir.path("m.vsdx"), &[]);
    assert_eq!(run.status.code(), Some(1), "{}", text(&run.stderr));
    assert!(!dir.path("m.vsdx").exists());
    // Kept as a macro-enabled file it is written.
    let line = converted(&input, &dir.path("m2.vsdm"), &[]);
    assert_eq!(line["format"], "vsdm");
}

// ------------------------------------------------------------------------------------------------------------------------ the input is never written

/// `--out` naming the input is refused, and so is a link to it, in any spelling; the input is as it was and nothing is made.
#[test]
fn out_is_never_the_input_nor_a_link_to_it() {
    let dir = TempDir::new("conv-same");
    for (name, bytes) in [("d.vsdx", vsdx()), ("o.odg", crate::odg_save::drawing())] {
        let input = dir.write(name, &bytes);
        let (_, run) = convert(&input, &input, &[]);
        assert_eq!(run.status.code(), Some(1), "{name}: {}", text(&run.stderr));
        assert!(text(&run.stderr).contains("--out is the drawing itself"), "{name}: {}", text(&run.stderr));
        // Another spelling of the same file, from the folder it is in.
        let child = std::process::Command::new(BIN)
            .current_dir(dir.path(""))
            .args(["convert", name, "--out", &format!("./{name}")])
            .env_remove("RUST_LOG")
            .output()
            .unwrap();
        assert_eq!(child.status.code(), Some(1), "{name}: {}", text(&child.stderr));
        assert!(text(&child.stderr).contains("--out is the drawing itself"), "{name}: {}", text(&child.stderr));
        #[cfg(unix)]
        {
            let link = dir.path(&format!("link-{name}"));
            std::os::unix::fs::symlink(&input, &link).unwrap();
            let (_, run) = convert(&input, &link, &[]);
            assert_eq!(run.status.code(), Some(1), "{name}: {}", text(&run.stderr));
            assert!(text(&run.stderr).contains("--out is the drawing itself"), "{name}: {}", text(&run.stderr));
            // A link that leads to the input, named as the input (and the other way round).
            let (_, run) = convert(&link, &input, &[]);
            assert_eq!(run.status.code(), Some(1), "{name}: {}", text(&run.stderr));
            assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the link is still a link");
        }
        assert_eq!(std::fs::read(&input).unwrap(), bytes, "{name}: the input is as it was");
    }
    // A link that leads to another file is not written either: the engine's own refusal of a link.
    #[cfg(unix)]
    {
        let input = dir.path("d.vsdx");
        let elsewhere = dir.write("elsewhere.vsdx", b"a file a link leads to");
        let link = dir.path("link-elsewhere.vsdx");
        std::os::unix::fs::symlink(&elsewhere, &link).unwrap();
        let (_, run) = convert(&input, &link, &[]);
        assert_eq!(run.status.code(), Some(1), "{}", text(&run.stderr));
        assert_eq!(std::fs::read(&elsewhere).unwrap(), b"a file a link leads to");
    }
}

// ------------------------------------------------------------------------------------------------------------------------ hostile input and output

/// A file that is not a drawing, one cut off, a package that holds nothing a drawing has, a folder and a name that is not there: each is an error in words that never repeats the
/// path, and writes nothing, for a drawing type and for a picture type.
#[test]
fn a_file_that_will_not_open_is_refused_in_words_with_no_path_and_nothing_is_written() {
    let dir = TempDir::new("conv-bad");
    let good = vsdx();
    let junk_zip = zip(&[ZipItem::deflated("Secret.txt", b"not a drawing")]);
    let hostile: Vec<(&str, Vec<u8>)> = vec![
        ("SECRETNAME-html.vsdx", b"<html>not a zip</html>".to_vec()),
        ("SECRETNAME-empty.vsdx", Vec::new()),
        ("SECRETNAME-cut.vsdx", good[..good.len() / 2].to_vec()),
        ("SECRETNAME-zip.vsdx", junk_zip),
        ("SECRETNAME-noise.diacraft", (0..4096u32).map(|n| (n.wrapping_mul(2_654_435_761) >> 24) as u8).collect()),
    ];
    for (name, bytes) in &hostile {
        let input = dir.write(name, bytes);
        for out_name in ["o.diacraft", "o.vsdx", "o.svg", "o.png", "o.pdf"] {
            let out = dir.write(out_name, b"the old file");
            let (line, run) = convert(&input, &out, &[]);
            assert_eq!(run.status.code(), Some(1), "{name} to {out_name}: {}", text(&run.stderr));
            let err = text(&run.stderr);
            assert!(err.starts_with("diacraft-cli: ") && err.contains("cannot"), "{name} to {out_name}: {err}");
            assert!(
                !err.contains("SECRETNAME") && !err.contains("Secret") && !err.contains(s(&dir.path(""))),
                "no path and no name in a message: {err}"
            );
            assert_eq!((line, std::fs::read(&out).unwrap()), (Value::Null, b"the old file".to_vec()), "{name} to {out_name}");
        }
    }
    // A folder and a name that is not there.
    for (input, words) in [(dir.path(""), "not a file"), (dir.path("missing.vsdx"), "there is no such file")] {
        for out_name in ["o.vsdx", "o.svg"] {
            let (_, run) = convert(&input, &dir.path(out_name), &[]);
            assert_eq!(run.status.code(), Some(1), "{}", text(&run.stderr));
            assert!(text(&run.stderr).contains(words), "{words}: {}", text(&run.stderr));
        }
    }
}

/// A folder that is not there, and a folder where the file should be: an error in words, no path, and the input as it was.
#[test]
fn a_file_that_cannot_be_written_is_an_error_in_words_and_names_no_path() {
    let dir = TempDir::new("conv-nowhere");
    let input = dir.write("in.vsdx", &vsdx());
    std::fs::create_dir(dir.path("a-folder.vsdx")).unwrap();
    for target in ["no-such-folder-SECRET/p.vsdx", "no-such-folder-SECRET/p.diacraft", "no-such-folder-SECRET/p.svg", "a-folder.vsdx"] {
        let out = dir.path(target);
        let (_, run) = convert(&input, &out, &[]);
        assert_eq!(run.status.code(), Some(1), "{target}: {}", text(&run.stderr));
        let err = text(&run.stderr);
        assert!(err.contains("cannot"), "{target}: {err}");
        assert!(!err.contains("SECRET") && !err.contains(s(&dir.path(""))), "{target}: no path in a message: {err}");
    }
}

/// A drawing at the reader's limit (80,000 shapes, 1.7 GB held) beside a second read and a save, in a process that may use 3 GB of address space: the convert ends as a conversion or as a
/// refusal in words and never aborts, to a drawing type and to a picture. (It builds and holds a large drawing, so it is in nextest's memory-heavy group.)
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn a_drawing_of_80000_shapes_is_converted_or_refused_in_words_and_does_not_abort() {
    let dir = TempDir::new("conv-heavy");
    let input = dir.write("in.vsdx", &typical_drawing(20, 4_000, None));
    for out_name in ["out.diacraft", "out.vsdx", "out.png"] {
        let out = dir.path(out_name);
        // A picture is drawn in the bundled faces, which reads nothing from the machine.
        let fonts = if out_name.ends_with(".png") { " --bundled-fonts" } else { "" };
        let script = format!("ulimit -v 3000000 && exec \"{}\" convert \"{}\" --out \"{}\"{fonts}", BIN, input.display(), out.display());
        let run = std::process::Command::new("/bin/sh").arg("-c").arg(script).env_remove("RUST_LOG").output().unwrap();
        let code = run.status.code();
        assert!(
            matches!(code, Some(0) | Some(1)),
            "{out_name}: the convert aborted under `ulimit -v 3000000`: {:?}\n{}",
            run.status,
            text(&run.stderr)
        );
        if code == Some(1) {
            assert!(
                text(&run.stderr).starts_with("diacraft-cli: ") && !out.exists(),
                "{out_name}: refused in words and nothing written: {}",
                text(&run.stderr)
            );
        } else {
            assert!(out.is_file(), "{out_name}: a conversion wrote its file");
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------------ the help

/// The usage lists the command, and names every type it writes, whichever way it is asked for.
#[test]
fn the_help_lists_the_command_and_names_every_type_it_writes() {
    for args in [&["--help"][..], &["convert", "--help"], &["-h"]] {
        let run = cli(args, None);
        assert!(run.status.success(), "{args:?}");
        let help = text(&run.stdout);
        assert!(help.contains("diacraft-cli convert FILE --out PATH"), "{args:?}: {help}");
        for extension in DRAWING_TYPES.iter().chain(&PICTURE_TYPES) {
            assert!(help.contains(&format!(".{extension}")), "{args:?}: .{extension} is named: {help}");
        }
    }
}

/// **The list of types is the engine's.** Each type the engine's Save As allows for some drawing (`Session::save_as_allows`, asked of a new drawing and of one opened from each kind of file)
/// is one `convert` takes and the help names, and each type the help names is one the engine allows for some drawing: the list cannot say more than the engine writes, nor less.
#[test]
fn the_types_convert_names_are_the_types_the_engine_allows_for_some_drawing() {
    let dir = TempDir::new("conv-engine-list");
    let made = made_by_the_engine(&dir, "n.diacraft");
    let files = [
        dir.write("d.vsdx", &vsdx()),
        made,
        dir.write("o.odg", &crate::odg_save::drawing()),
        dir.write("s.vssx", &stencil()),
        dir.write("t.vstx", &template()),
        dir.write("f.fodg", &crate::odg_flat::flat_drawing("Flat")),
    ];
    let mut sessions = vec![Session::new()];
    sessions[0].execute("file.new", &json!({})).unwrap();
    for file in &files {
        let mut session = Session::new();
        session.execute("file.open", &json!({ "path": file, "edit_template": true })).unwrap();
        sessions.push(session);
    }
    // Every extension a drawing or a picture file could be called that DiaCraft knows of, and some it does not.
    let probes = [
        "diacraft",
        "diacraft-stencil",
        "diacraft-template",
        "vsdx",
        "vsdm",
        "vssx",
        "vssm",
        "vstx",
        "vstm",
        "odg",
        "otg",
        "fodg",
        "fotg",
        "vsd",
        "vss",
        "vst",
        "vdx",
        "vsdx.bak",
        "xml",
        "png",
        "svg",
        "pdf",
        "pptx",
        "dxf",
        "emf",
        "txt",
    ];
    let help = text(&cli(&["--help"], None).stdout);
    let input = dir.path("d.vsdx");
    for extension in probes {
        let allowed = sessions.iter().any(|session| session.save_as_allows(extension));
        if allowed {
            assert!(help.contains(&format!(".{extension}")), ".{extension} is written by the engine and the help does not name it");
        }
        // Pictures are `export`'s and not the engine's Save As: they are in the list as they are.
        let picture = PICTURE_TYPES.contains(&extension);
        let named = DRAWING_TYPES.contains(&extension) || picture;
        assert_eq!(allowed, DRAWING_TYPES.contains(&extension), ".{extension}: the engine's answer and the list differ");
        // A name that holds the type is taken by the line (the engine may refuse the save, which is not a mistake in the line).
        let run = cli(&["convert", s(&input), "--out", s(&dir.path(&format!("probe.{extension}")))], None);
        let mistaken = run.status.code() == Some(2) && text(&run.stderr).contains("--out must end in");
        assert_eq!(mistaken, !named, ".{extension}: {}", text(&run.stderr));
    }
}

// ------------------------------------------------------------------------------------------------------------------------ the warnings

/// The demo video's drawing (issue 10): a Basic Network Diagram drawing with a theme, of three devices that show their network name and address as fields, two connectors, saved as `name` (by its extension).
fn made_from_the_network_template(dir: &TempDir, name: &str) -> std::path::PathBuf {
    let path = dir.path(name);
    let steps = [
        ("file.new", json!({ "template": "basic_network" })),
        ("theme.apply", json!({ "theme": "fernwood" })),
        ("shape.drop", json!({ "master": "network.router", "page": 1, "x": 2.0, "y": 5.0 })),
        ("shape.drop", json!({ "master": "network.switch", "page": 1, "x": 5.0, "y": 5.0 })),
        ("shape.drop", json!({ "master": "computers.desktop", "page": 1, "x": 8.0, "y": 5.0 })),
        ("connector.add", json!({ "from": { "id": 1 }, "to": { "id": 2 } })),
        ("connector.add", json!({ "from": { "id": 2 }, "to": { "id": 3 } })),
        ("file.save_as", json!({ "path": path })),
    ];
    let owned: Vec<(String, String)> = steps.iter().map(|(command, params)| ((*command).to_owned(), params.to_string())).collect();
    let mut args = vec!["run"];
    for (command, params) in &owned {
        args.extend(["--cmd", command, "--params", params]);
    }
    let run = cli(&args, None);
    assert!(run.status.success(), "{}", text(&run.stderr));
    path
}

/// The warnings of a line as `(class, words)`: each is an object with both.
fn told_warnings(line: &Value) -> Vec<(String, String)> {
    line["warnings"]
        .as_array()
        .unwrap_or_else(|| panic!("no warnings array: {line}"))
        .iter()
        .map(|w| {
            let class = w["class"].as_str().unwrap_or_else(|| panic!("a warning with no class: {w} in {line}"));
            let words = w["words"].as_str().unwrap_or_else(|| panic!("a warning with no words: {w} in {line}"));
            (class.to_owned(), words.to_owned())
        })
        .collect()
}

#[test]
fn a_drawing_the_app_made_saved_as_an_odg_tells_each_warning_by_a_class_and_in_words_and_none_is_other() {
    // The demo's steps: `convert office-network.vsdx --out office-network.odg` answered `"warnings":["other", ...]`, eight times.
    let dir = TempDir::new("conv-demo");
    let vsdx = made_from_the_network_template(&dir, "office-network.vsdx");
    let line = converted(&vsdx, &dir.path("office-network.odg"), &[]);
    let told = told_warnings(&line);
    assert_eq!(line["warning_count"].as_u64(), Some(told.len() as u64), "{line}");
    assert!(!told.is_empty(), "the .odg has no place for some of what the drawing holds, and says so: {line}");
    for (class, words) in &told {
        assert_ne!(class, "other", "{words}");
        assert!(words.contains(' '), "{class}: words, not a code: {words}");
    }
    let classes: std::collections::BTreeSet<&str> = told.iter().map(|(class, _)| class.as_str()).collect();
    for expected in ["text_format_not_written", "style_links_not_written", "field_as_text"] {
        assert!(classes.contains(expected), "{expected} is among {classes:?}: {line}");
    }
    // What changed and where: the engine's own sentences name the shapes by their numbers.
    let fields = told.iter().find(|(class, _)| class == "field_as_text").unwrap();
    assert!(fields.1.contains("of shape") && fields.1.contains("on page"), "{}", fields.1);
}

#[test]
fn no_convert_of_the_fixtures_to_any_type_gives_a_warning_that_is_other() {
    // Every drawing the tests make, to every type it can be: a warning no class knows is `other`, and none of these saves says one.
    let dir = TempDir::new("conv-classes");
    let inputs = [
        dir.write("fixture.vsdx", &vsdx()),
        dir.write("fixture.vstx", &template()),
        dir.write("fixture.vssx", &stencil()),
        made_by_the_engine(&dir, "made.diacraft"),
        made_by_the_engine(&dir, "made.vsdx"),
        made_by_the_engine(&dir, "made.odg"),
        made_from_the_network_template(&dir, "network.diacraft"),
        made_from_the_network_template(&dir, "network.vsdx"),
    ];
    let mut told_in_all = 0;
    for input in &inputs {
        for extension in DRAWING_TYPES {
            let out = dir.path(&format!("out-{}.{extension}", input.file_name().unwrap().to_str().unwrap()));
            let (line, run) = convert(input, &out, &[]);
            if !run.status.success() {
                // A refusal in the engine's words (a stencil is never an `.odg`, a macro project needs a type that holds one): no line, nothing to tell.
                continue;
            }
            for (class, words) in told_warnings(&line) {
                assert_ne!(class, "other", "{} as .{extension}: {words}", input.display());
                told_in_all += 1;
            }
        }
    }
    assert!(told_in_all > 0, "the fixtures give warnings (a stencil keeps its pages)");
}
