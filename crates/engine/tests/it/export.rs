//! `file.export`: one page of the drawing written as a PNG, an SVG or a PDF. It changes nothing in the drawing, it is judged by the
//! rules of a save before anything is written (a link, the extension of what would be written, the drawing's own file), and a file it cannot
//! write whole is not written.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_engine::{EngineError, Session};
use diacraft_testkit::package::{PageSpec, VsdxBuilder};
use serde_json::{Value, json};

use crate::{blank_session, check_session, run, snapshot};

/// A scratch folder under `TMPDIR`, removed on drop.
struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("diacraft-engine-export-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }

    fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.0).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A blank drawing with a rectangle and a second page.
fn session_with_a_shape() -> Session {
    let mut s = blank_session();
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2, "y": 2}));
    run(&mut s, "page.add", json!({"name": "Second"}));
    s
}

fn be32(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// What an export must leave as it was: the document, its history, its revision, and whether it is unsaved.
fn untouched(s: &Session) -> (Option<diacraft_doc::Document>, usize, usize, u64, bool, Option<PathBuf>) {
    let snap = snapshot(s);
    (snap.document, snap.undo_steps, snap.redo_steps, snap.revision, s.is_dirty(), s.path().map(PathBuf::from))
}

fn fails(s: &mut Session, params: Value) -> EngineError {
    s.execute("file.export", &params).unwrap_err()
}

#[test]
fn a_page_is_written_as_each_format_and_the_drawing_is_not_touched() {
    let dir = Dir::new();
    let mut s = session_with_a_shape();
    let before = untouched(&s);
    for (name, magic, format) in [("p.png", &b"\x89PNG\r\n\x1a\n"[..], "png"), ("p.svg", b"<?xml", "svg"), ("p.pdf", b"%PDF-", "pdf")] {
        let result = run(&mut s, "file.export", json!({"path": dir.path(name)}));
        let file = std::fs::read(dir.path(name)).unwrap();
        assert!(file.starts_with(magic), "{name}");
        assert_eq!(result["format"], format);
        assert_eq!(result["bytes"].as_u64(), Some(file.len() as u64));
        assert_eq!(result["page"], json!({"id": 1, "name": "Page-1"}));
        assert!(result["width"].as_u64().unwrap() > 0 && result["height"].as_u64().unwrap() > 0);
        assert!(result["skipped"].is_array());
        assert_eq!(untouched(&s), before, "{name}: an export changes nothing in the drawing");
    }
    assert_eq!(dir.files(), ["p.pdf", "p.png", "p.svg"], "no temporary file is left");
    // Journaled, as a save is: it is a thing a macro may do.
    assert_eq!(s.journal().last().map(|(id, _)| id.as_str()), Some("file.export"));
    check_session(&s).unwrap();
}

#[test]
fn the_extension_is_read_in_any_case() {
    let dir = Dir::new();
    let mut s = session_with_a_shape();
    assert_eq!(run(&mut s, "file.export", json!({"path": dir.path("a.SVG")}))["format"], "svg");
    assert_eq!(run(&mut s, "file.export", json!({"path": dir.path("b.Pdf")}))["format"], "pdf");
}

#[test]
fn a_png_is_the_pages_size_at_the_resolution_asked_for() {
    let dir = Dir::new();
    let mut s = session_with_a_shape();
    let (w, h) = {
        let doc = s.document().unwrap();
        diacraft_render::page_paper_size(doc, 1).unwrap()
    };
    let path = dir.path("p.png");
    let result = run(&mut s, "file.export", json!({"path": path, "dpi": 50}));
    let file = std::fs::read(&path).unwrap();
    assert_eq!((be32(&file, 16), be32(&file, 20)), ((w * 50.0).ceil() as u32, (h * 50.0).ceil() as u32));
    assert_eq!(result["width"], be32(&file, 16));
    // The default is 96 to the inch.
    let result = run(&mut s, "file.export", json!({"path": path}));
    assert_eq!(result["width"].as_u64(), Some((w * 96.0).ceil() as u64));
}

#[test]
fn the_page_asked_for_is_the_page_written_and_the_first_is_the_default() {
    let dir = Dir::new();
    let mut s = session_with_a_shape();
    let second = run(&mut s, "file.export", json!({"path": dir.path("two.svg"), "page": 2}));
    assert_eq!(second["page"], json!({"id": 2, "name": "Second"}));
    let two = std::fs::read_to_string(dir.path("two.svg")).unwrap();
    let one_result = run(&mut s, "file.export", json!({"path": dir.path("one.svg")}));
    assert_eq!(one_result["page"]["id"], 1);
    let one = std::fs::read_to_string(dir.path("one.svg")).unwrap();
    assert!(one.matches("<path").count() > two.matches("<path").count(), "page 1 holds the rectangle and page 2 only the paper");
    let before = untouched(&s);
    let error = fails(&mut s, json!({"path": dir.path("none.svg"), "page": 9}));
    assert!(matches!(error, EngineError::BadParams { .. }) && error.to_string().contains("no page has id 9"), "{error}");
    assert_eq!(untouched(&s), before);
    assert!(!std::path::Path::new(&dir.path("none.svg")).exists());
}

#[test]
fn text_is_text_unless_outlines_are_asked_for_and_paper_is_white_unless_it_is_not() {
    let dir = Dir::new();
    let mut s = session_with_a_shape();
    run(&mut s, "shape.set_text", json!({"id": 1, "text": "Hello"}));
    run(&mut s, "file.export", json!({"path": dir.path("t.svg")}));
    let text = std::fs::read_to_string(dir.path("t.svg")).unwrap();
    assert!(text.contains(">Hello</text>"), "the letters are text");
    let whites = text.matches("fill=\"#ffffff\"").count();
    assert!(whites >= 1, "the paper is white");
    run(&mut s, "file.export", json!({"path": dir.path("o.svg"), "text": "outlines", "transparent": true}));
    let outlined = std::fs::read_to_string(dir.path("o.svg")).unwrap();
    assert!(!outlined.contains("<text"), "every letter is outlines");
    assert_eq!(outlined.matches("fill=\"#ffffff\"").count(), whites - 1, "no paper: one white rectangle fewer (the shape's own fill is white too)");
    run(&mut s, "file.export", json!({"path": dir.path("t.pdf"), "text": "text"}));
    run(&mut s, "file.export", json!({"path": dir.path("o.pdf"), "text": "outlines", "transparent": true}));
}

#[test]
fn what_cannot_be_asked_is_refused_before_anything_is_written() {
    let dir = Dir::new();
    let mut s = session_with_a_shape();
    let before = untouched(&s);
    let p = |name: &str| dir.path(name);
    for params in [
        json!({}),
        json!({"path": 5}),
        json!({"path": p("x")}),
        json!({"path": p("x.vsdx")}),
        json!({"path": p("x.gif")}),
        json!({"path": p("x.png"), "dpi": 0}),
        json!({"path": p("x.png"), "dpi": -4}),
        json!({"path": p("x.png"), "dpi": 2401}),
        json!({"path": p("x.png"), "dpi": "96"}),
        json!({"path": p("x.svg"), "dpi": 96}),
        json!({"path": p("x.png"), "text": "outlines"}),
        json!({"path": p("x.svg"), "text": "paths"}),
        json!({"path": p("x.svg"), "text": 1}),
        json!({"path": p("x.svg"), "transparent": "yes"}),
        json!({"path": p("x.svg"), "page": -1}),
        json!({"path": p("x.svg"), "page": "one"}),
        json!({"path": p("x.svg"), "paper": "black"}),
    ] {
        let error = fails(&mut s, params.clone());
        assert!(matches!(error, EngineError::BadParams { .. }), "{params}: {error:?}");
        assert!(!error.to_string().chars().any(char::is_control), "{error}");
        assert_eq!(untouched(&s), before, "{params}");
    }
    assert!(dir.files().is_empty(), "nothing was written: {:?}", dir.files());
    // No document, no export.
    assert!(matches!(Session::new().execute("file.export", &json!({"path": p("x.svg")})), Err(EngineError::Disabled { .. })));
}

#[test]
fn an_export_in_a_batch_is_refused_because_a_file_cannot_be_taken_back() {
    let dir = Dir::new();
    let mut s = session_with_a_shape();
    let before = untouched(&s);
    let params = json!({"commands": [{"command": "page.add"}, {"command": "file.export", "params": {"path": dir.path("x.svg")}}]});
    let error = s.execute("command.batch", &params).unwrap_err();
    assert!(error.to_string().contains("batch") || error.to_string().contains("file.export"), "{error}");
    assert_eq!(untouched(&s), before);
    assert!(dir.files().is_empty());
}

#[test]
fn a_file_that_is_there_is_replaced_whole_and_a_folder_that_is_not_there_is_an_error() {
    let dir = Dir::new();
    let mut s = session_with_a_shape();
    std::fs::write(dir.path("p.svg"), b"the old picture").unwrap();
    run(&mut s, "file.export", json!({"path": dir.path("p.svg")}));
    assert!(std::fs::read_to_string(dir.path("p.svg")).unwrap().contains("<svg"));
    assert_eq!(dir.files(), ["p.svg"]);
    let error = fails(&mut s, json!({"path": dir.path("no/such/folder/p.svg")}));
    assert!(matches!(error, EngineError::Failed { .. }), "{error:?}");
    // A folder where the file is to be.
    std::fs::create_dir(dir.path("folder.png")).unwrap();
    let error = fails(&mut s, json!({"path": dir.path("folder.png")}));
    assert!(matches!(error, EngineError::Failed { .. }), "{error:?}");
    check_session(&s).unwrap();
}

/// A drawing, as a file, named with an extension an export writes: so that "never write over the open drawing" has to be judged by what the
/// path is and not by what it ends in.
fn open_drawing_named(dir: &Dir, name: &str) -> (Session, Vec<u8>) {
    let sheet = r#"<Cell N="PageWidth" V="4"/><Cell N="PageHeight" V="3"/><Cell N="PageScale" V="1"/><Cell N="DrawingScale" V="1"/>"#;
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "First").sheet(sheet)).build();
    std::fs::write(dir.path(name), &bytes).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": dir.path(name)}));
    (s, bytes)
}

#[test]
fn the_open_drawing_is_never_written_whatever_the_path_is_spelled_like() {
    let dir = Dir::new();
    let (mut s, bytes) = open_drawing_named(&dir, "d.svg");
    std::fs::create_dir(dir.path("sub")).unwrap();
    let before = untouched(&s);
    for path in [dir.path("d.svg"), dir.path("sub/../d.svg"), dir.path("./d.svg")] {
        let error = fails(&mut s, json!({"path": path}));
        assert!(matches!(error, EngineError::Failed { .. }) && error.to_string().contains("drawing's own file"), "{path}: {error}");
        assert_eq!(std::fs::read(dir.path("d.svg")).unwrap(), bytes, "{path}: the drawing is as it was");
        assert_eq!(untouched(&s), before);
    }
    // The same folder and another name is a picture beside the drawing.
    run(&mut s, "file.export", json!({"path": dir.path("e.svg")}));
    assert_eq!(std::fs::read(dir.path("d.svg")).unwrap(), bytes);
}

#[cfg(unix)]
#[test]
fn a_link_is_refused_unless_it_leads_to_the_open_drawing_and_then_the_drawing_itself_is_refused() {
    use std::os::unix::fs::symlink;
    let dir = Dir::new();
    let (mut s, bytes) = open_drawing_named(&dir, "d.pdf");
    std::fs::write(dir.path("other.txt"), b"another file").unwrap();
    let before = untouched(&s);
    // A link to another file, with a good extension: the file it leads to is never written.
    symlink(dir.path("other.txt"), dir.path("l.svg")).unwrap();
    let error = fails(&mut s, json!({"path": dir.path("l.svg")}));
    assert!(matches!(error, EngineError::Failed { .. }) && error.to_string().contains("link"), "{error}");
    assert_eq!(std::fs::read(dir.path("other.txt")).unwrap(), b"another file");
    // A link to the open drawing: the drawing is not written through it.
    symlink(dir.path("d.pdf"), dir.path("own.png")).unwrap();
    let error = fails(&mut s, json!({"path": dir.path("own.png")}));
    assert!(matches!(error, EngineError::Failed { .. }) && error.to_string().contains("drawing's own file"), "{error}");
    assert_eq!(std::fs::read(dir.path("d.pdf")).unwrap(), bytes);
    // A link that leads nowhere.
    symlink(dir.path("not-there"), dir.path("dangling.svg")).unwrap();
    let error = fails(&mut s, json!({"path": dir.path("dangling.svg")}));
    assert!(matches!(error, EngineError::Failed { .. }) && error.to_string().contains("leads nowhere"), "{error}");
    assert!(!std::path::Path::new(&dir.path("not-there")).exists());
    // A link to a folder.
    std::fs::create_dir(dir.path("folder")).unwrap();
    symlink(dir.path("folder"), dir.path("f.svg")).unwrap();
    assert!(s.execute("file.export", &json!({"path": dir.path("f.svg")})).is_err());
    assert_eq!(untouched(&s), before);
    check_session(&s).unwrap();
}

#[test]
fn a_page_too_large_for_the_format_is_an_error_and_not_a_file() {
    let dir = Dir::new();
    let sheet = r#"<Cell N="PageWidth" V="1000000000"/><Cell N="PageHeight" V="3"/><Cell N="PageScale" V="1"/><Cell N="DrawingScale" V="1"/>"#;
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "Huge").sheet(sheet)).build();
    std::fs::write(dir.path("huge.vsdx"), bytes).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": dir.path("huge.vsdx")}));
    let before = untouched(&s);
    for name in ["p.png", "p.svg", "p.pdf"] {
        let error = fails(&mut s, json!({"path": dir.path(name)}));
        assert!(matches!(error, EngineError::Failed { .. }) && error.to_string().contains("too large to export"), "{name}: {error}");
        assert_eq!(untouched(&s), before);
    }
    assert_eq!(dir.files(), ["huge.vsdx"]);
}

#[test]
fn an_export_does_not_change_what_a_save_would_do() {
    let dir = Dir::new();
    let mut s = session_with_a_shape();
    run(&mut s, "file.save_as", json!({"path": dir.path("d.vsdx")}));
    let saved = std::fs::read(dir.path("d.vsdx")).unwrap();
    run(&mut s, "file.export", json!({"path": dir.path("p.svg")}));
    assert!(!s.is_dirty(), "an export does not make the drawing unsaved");
    assert_eq!(s.path().map(PathBuf::from), Some(PathBuf::from(dir.path("d.vsdx"))), "nor does it give the drawing another file");
    run(&mut s, "file.save", json!({}));
    assert_eq!(std::fs::read(dir.path("d.vsdx")).unwrap(), saved);
}
