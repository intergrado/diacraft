//! `file.open`: a VSDX package built in code (`diacraft-testkit`), opened from disk through the command. Every way it can fail leaves the
//! open document as it was (the M2 plan's Review Focus 5).

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_engine::{EngineError, OpenProblem, Session};
use diacraft_testkit::package::{CT_PAGE, MasterSpec, PageSpec, Pkg, VsdxBuilder, ZipItem, shape};
use serde_json::{Value, json};

use crate::{blank_session, check_session, run, snapshot};

/// A scratch folder under `TMPDIR`, removed on drop.
struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("diacraft-engine-open-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn write(&self, name: &str, bytes: &[u8]) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        path.to_str().unwrap().to_owned()
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        // Litter in a scratch folder is harmless.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn drawing() -> Vec<u8> {
    VsdxBuilder::new()
        .core("<cp:coreProperties xmlns:cp=\"urn:x\" xmlns:dc=\"urn:y\"><dc:title>Plant A</dc:title></cp:coreProperties>")
        .master(MasterSpec::new(1, "M").shapes(&shape(4, "")))
        .page(PageSpec::new(1, "First").shapes(&shape(5, r#"<Cell N="Width" V="2"/>"#)))
        .page(PageSpec::new(2, "Second"))
        .build()
}

fn open_err(s: &mut Session, path: &str) -> EngineError {
    s.execute("file.open", &json!({ "path": path })).unwrap_err()
}

#[test]
fn opening_a_file_makes_it_the_document_with_a_fresh_history() {
    let dir = Dir::new();
    let path = dir.write("a.vsdx", &drawing());
    let mut s = Session::new();
    let result = run(&mut s, "file.open", json!({ "path": path }));
    assert_eq!(result["title"], "Plant A");
    let names: Vec<&str> = result["pages"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["First", "Second"]);
    assert_eq!(result["history"], json!({ "undo": 0, "redo": 0 }));
    assert_eq!(result["file"]["variant"], "vsdx");
    assert_eq!(result["file"]["warning_count"], 0);
    assert_eq!(s.document().unwrap().masters.len(), 1);
    assert_eq!(s.journal().last().map(|e| e.0.as_str()), Some("file.open"));
    check_session(&s).unwrap();
}

#[test]
fn opening_replaces_the_open_document_and_its_history() {
    let dir = Dir::new();
    let path = dir.write("a.vsdx", &drawing());
    let mut s = blank_session();
    run(&mut s, "page.add", json!({}));
    assert_eq!(s.history().unwrap().undo_len(), 1);
    let revision = s.revision();
    run(&mut s, "file.open", json!({ "path": path }));
    assert_eq!(s.history().unwrap().undo_len(), 0);
    assert!(s.revision() > revision);
    assert_eq!(s.document().unwrap().pages.len(), 2);
}

#[test]
fn an_extension_that_disagrees_with_the_package_is_warned_of() {
    // V7 of the package spec: a stencil under the macro-enabled drawing extension reads, with the kind from the content type.
    let dir = Dir::new();
    let stencil = VsdxBuilder::new().document_type("application/vnd.ms-visio.stencil.main+xml").build();
    let path = dir.write("a.vsdm", &stencil);
    let mut s = Session::new();
    let result = run(&mut s, "file.open", json!({ "path": path }));
    assert_eq!(result["file"]["variant"], "vssx");
    let first = result["file"]["warnings"][0].as_str().unwrap();
    assert!(first.contains("extension") && first.contains("stencil"), "{first}");
    // No extension at all, or a VSDX one that agrees, is no warning of that kind.
    let path = dir.write("b.vssx", &stencil);
    let result = run(&mut s, "file.open", json!({ "path": path }));
    assert_eq!(result["file"]["warnings"], json!([]));
}

#[test]
fn a_file_that_cannot_be_opened_leaves_the_open_document_as_it_was() {
    let dir = Dir::new();
    let mut s = blank_session();
    run(&mut s, "page.add", json!({"name": "Mine"}));
    let before = snapshot(&s);
    std::fs::create_dir(dir.path("folder.vsdx")).unwrap();
    let bomb = Pkg::new().item(ZipItem::zeros_under_the_ratio_cap("visio/media/x.bin", 3 * 1024 * 1024));
    let cases: [(String, &str, OpenProblem); 7] = [
        (dir.path("missing.vsdx"), "there is no such file", OpenProblem::NotFound),
        (dir.path("folder.vsdx"), "not a file", OpenProblem::NotAFile),
        (dir.write("html.vsdx", b"<html>not a zip</html>"), "not a drawing file (not_zip)", OpenProblem::NotAPackage),
        (dir.write("empty.vsdx", b""), "not a drawing file (not_zip)", OpenProblem::NotAPackage),
        (dir.write("nopkg.vsdx", &Pkg::new().build()), "not a drawing file (no_drawing_parts)", OpenProblem::NotAPackage),
        (
            dir.write("broken.vsdx", &VsdxBuilder::new().page(PageSpec::new(1, "P").part("<PageContents><Shapes></PageContents>")).build()),
            "damaged (xml_malformed)",
            OpenProblem::Damaged,
        ),
        // Within every limit (a part of zeros under the ratio cap), and a package with no drawing in it.
        (dir.write("bomb.vsdx", &bomb.build()), "", OpenProblem::NotAPackage),
    ];
    for (path, words, problem) in &cases {
        let e = open_err(&mut s, path);
        // The kind of trouble is a typed field, for a frontend that wants its own words; the message is for a person.
        assert!(matches!(&e, EngineError::CannotOpen { problem: p, .. } if p == problem), "{e:?}: wanted {problem:?}");
        assert!(e.to_string().contains(words), "{e}: wanted {words}");
        assert!(!e.to_string().contains(path.as_str()), "the message holds the path: {e}");
        assert_eq!(snapshot(&s), before, "{path}: a failed open changed the session");
        check_session(&s).unwrap();
    }
}

#[test]
fn a_package_past_a_limit_is_refused_as_too_big_and_changes_nothing() {
    // A page part of 300 MiB of zeros inflates past the 256 MiB a part may hold.
    let dir = Dir::new();
    let big = VsdxBuilder::new().with(Pkg::new().item(ZipItem::zeros_under_the_ratio_cap("visio/media/big.bin", 300 * 1024 * 1024))).build();
    let path = dir.write("big.vsdx", &big);
    let mut s = blank_session();
    let before = snapshot(&s);
    let e = open_err(&mut s, &path);
    assert!(e.to_string().contains("too big to open safely") && e.to_string().contains("part_too_large"), "{e}");
    assert_eq!(snapshot(&s), before);
}

#[test]
fn the_path_is_hostile_text_and_never_comes_back_raw() {
    let mut s = blank_session();
    let before = snapshot(&s);
    for path in ["\u{1b}[2Jevil\nline", "nul\0byte", "\u{202e}rtl", &"x".repeat(100_000), "", " ", "/", "/dev/null", "/dev/zero", "/proc/self/mem"] {
        let e = open_err(&mut s, path);
        assert!(!e.to_string().chars().any(char::is_control), "{e:?}");
        assert_eq!(snapshot(&s), before);
    }
    for bad in [json!({}), json!({"path": 5}), json!({"path": null}), json!({"path": ["a"]}), json!({"path": "a", "extra": 1})] {
        assert!(matches!(s.execute("file.open", &bad), Err(EngineError::BadParams { .. })), "{bad}");
    }
}

#[test]
fn opening_inside_a_batch_is_refused_like_a_new_document() {
    let dir = Dir::new();
    let path = dir.write("a.vsdx", &drawing());
    let mut s = blank_session();
    let before = snapshot(&s);
    let batch = json!({"commands": [{"command": "page.add"}, {"command": "file.open", "params": {"path": path}}]});
    let e = s.execute("command.batch", &batch).unwrap_err();
    assert!(e.to_string().contains("file.open"), "{e}");
    assert_eq!(snapshot(&s), before, "the batch is all or nothing");
}

#[test]
fn a_page_that_is_missing_or_a_warning_does_not_stop_the_open_and_is_reported() {
    let dir = Dir::new();
    let weird = VsdxBuilder::new()
        .page(PageSpec::new(1, "A").shapes(&shape(1, "")))
        .page(PageSpec::new(1, "Twin").without_part())
        .with(Pkg::new().part("/visio/pages/extra.xml", CT_PAGE, "<x/>"))
        .build();
    let path = dir.write("w.vsdx", &weird);
    let mut s = Session::new();
    let result = run(&mut s, "file.open", json!({ "path": path }));
    // A page ID used twice is read, and said: the document is open, with both pages.
    assert_eq!(result["pages"].as_array().unwrap().len(), 2);
    assert!(result["file"]["warning_count"].as_u64().unwrap() >= 1, "{result}");
    let texts: Vec<String> = result["file"]["warnings"].as_array().unwrap().iter().map(|w: &Value| w.as_str().unwrap().to_owned()).collect();
    assert!(texts.iter().any(|t| t.contains("ID 1")), "{texts:?}");
}

// ------------------------------------------------------------------------------------------------ the session keeps the file

#[test]
fn the_session_holds_the_files_bytes_and_path_for_as_long_as_the_document_is_open() {
    let dir = Dir::new();
    let bytes = drawing();
    let path = dir.write("a.vsdx", &bytes);
    let mut s = Session::new();
    assert!(s.source().is_none(), "no document, no source");
    let result = run(&mut s, "file.open", json!({ "path": path }));
    let source = s.source().expect("the file the document was opened from");
    assert_eq!(source.bytes(), bytes.as_slice(), "the bytes as they were read");
    assert_eq!(source.path().to_str(), Some(path.as_str()));
    assert_eq!(result["file"]["bytes"], bytes.len());
    // The bytes are the file's own, so a part the model has no slot for can be found in them: the package reads from them.
    let package = diacraft_opc::Package::read(source.bytes(), &diacraft_opc::Limits::default()).unwrap();
    assert!(package.part("/visio/pages/page1.xml").is_some());
    // Edits, undo and redo move the document and not the file it was opened from.
    run(&mut s, "page.add", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.source().unwrap().bytes(), bytes.as_slice());
    // The same copy is shared, not copied, by a holder that outlives the borrow.
    let shared = s.source().unwrap().shared();
    assert_eq!(std::sync::Arc::strong_count(&shared), 2);
    drop(shared);
    // A new document is not the file's.
    run(&mut s, "file.new", json!({}));
    assert!(s.source().is_none());
}

#[test]
fn a_file_that_cannot_be_opened_leaves_the_source_as_it_was_and_a_new_open_replaces_it() {
    let dir = Dir::new();
    let first = drawing();
    let second = VsdxBuilder::new().page(PageSpec::new(1, "Only")).build();
    let (a, b) = (dir.write("a.vsdx", &first), dir.write("b.vsdx", &second));
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": a }));
    let before = snapshot(&s);
    assert!(s.execute("file.open", &json!({ "path": dir.path("missing.vsdx") })).is_err());
    assert_eq!(snapshot(&s), before, "the source is part of what a failed open leaves alone");
    run(&mut s, "file.open", json!({ "path": b }));
    assert_eq!(s.source().unwrap().bytes(), second.as_slice());
    assert_eq!(s.source().unwrap().path().to_str(), Some(b.as_str()));
}

// ----------------------------------------------------------------------------------------------------------- what the reply says

#[test]
fn the_reply_counts_the_extension_warning_and_says_what_the_diacraft_part_held() {
    let dir = Dir::new();
    let mut s = Session::new();
    // A stencil under the macro-enabled drawing extension: one warning, and it is in the count.
    let stencil = VsdxBuilder::new().document_type("application/vnd.ms-visio.stencil.main+xml").build();
    let result = run(&mut s, "file.open", json!({ "path": dir.write("a.vsdm", &stencil) }));
    assert_eq!(result["file"]["warning_count"], 1);
    assert_eq!(result["file"]["warnings"].as_array().unwrap().len(), 1);
    assert_eq!(result["file"]["diacraft"], json!({ "state": "absent" }));
    // DiaCraft's own part: loaded, with the version and the number of its elements; a part named so that is not readable says why.
    let data =
        r#"<SolutionXML Name="DiaCraft"><dc:Data xmlns:dc="urn:app.diacraft:extension" dc:version="3"><dc:Item/><dc:Item/></dc:Data></SolutionXML>"#;
    let with = VsdxBuilder::new().solution("DiaCraft", data).build();
    let result = run(&mut s, "file.open", json!({ "path": dir.write("b.vsdx", &with) }));
    assert_eq!(result["file"]["diacraft"], json!({ "state": "loaded", "version": "3", "children": 2 }));
    let broken = VsdxBuilder::new().solution("DiaCraft", r#"<SolutionXML Name="DiaCraft"><Other/></SolutionXML>"#).build();
    let result = run(&mut s, "file.open", json!({ "path": dir.write("c.vsdx", &broken) }));
    assert_eq!(result["file"]["diacraft"], json!({ "state": "unreadable", "why": "no Data in the DiaCraft namespace" }));
}

#[test]
fn the_problem_words_are_fixed() {
    for (problem, word) in [
        (OpenProblem::NotFound, "not_found"),
        (OpenProblem::Denied, "denied"),
        (OpenProblem::NotAFile, "not_a_file"),
        (OpenProblem::FileTooLarge, "file_too_large"),
        (OpenProblem::OutOfMemory, "out_of_memory"),
        (OpenProblem::Unreadable, "unreadable"),
        (OpenProblem::NotAPackage, "not_a_package"),
        (OpenProblem::TooBig, "too_big"),
        (OpenProblem::Damaged, "damaged"),
    ] {
        assert_eq!(problem.word(), word);
    }
}

// ------------------------------------------------------------------------------------------------ reading a file from disk

#[test]
fn a_file_of_exactly_the_most_bytes_is_read_and_one_byte_more_is_refused() {
    use diacraft_engine::file::{FileError, read_file};
    let dir = Dir::new();
    let path = dir.write("ten.bin", &[7u8; 10]);
    let path = std::path::Path::new(&path);
    assert_eq!(read_file(path, 10).unwrap(), [7u8; 10]);
    assert_eq!(read_file(path, 11).unwrap().len(), 10);
    assert_eq!(read_file(path, 9), Err(FileError::TooLarge { limit: 9 }));
    assert_eq!(read_file(&std::path::PathBuf::from(dir.path("missing.bin")), 10), Err(FileError::NotFound));
    assert_eq!(read_file(&std::path::PathBuf::from(dir.path("")), 10), Err(FileError::NotAFile), "a folder");
}

#[cfg(unix)]
#[test]
fn a_link_is_followed_to_a_file_and_refused_to_a_folder_or_to_nothing() {
    use diacraft_engine::file::{FileError, read_file};
    let dir = Dir::new();
    let file = dir.write("real.bin", b"abc");
    std::fs::create_dir(dir.path("folder")).unwrap();
    std::os::unix::fs::symlink(&file, dir.path("to-file")).unwrap();
    std::os::unix::fs::symlink(dir.path("folder"), dir.path("to-folder")).unwrap();
    std::os::unix::fs::symlink(dir.path("gone"), dir.path("to-nothing")).unwrap();
    std::os::unix::fs::symlink(dir.path("loop"), dir.path("loop")).unwrap();
    let read = |name: &str| read_file(std::path::Path::new(&dir.path(name)), 100);
    assert_eq!(read("to-file").unwrap(), b"abc");
    assert_eq!(read("to-folder"), Err(FileError::NotAFile));
    assert_eq!(read("to-nothing"), Err(FileError::NotFound));
    assert!(read("loop").is_err());
}

#[cfg(unix)]
#[test]
fn a_pipe_is_refused_at_once_and_does_not_block_the_reader() {
    use diacraft_engine::file::{FileError, read_file};
    let dir = Dir::new();
    let fifo = dir.path("pipe");
    // No `mkfifo` on the machine is no pipe to test with.
    if !std::process::Command::new("mkfifo").arg(&fifo).status().is_ok_and(|s| s.success()) {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let path = std::path::PathBuf::from(&fifo);
    std::thread::spawn(move || {
        let _ = tx.send(read_file(&path, 100));
    });
    // A reader that opened the pipe and waited for a writer would never answer: the thread is left blocked, and this fails.
    let answer = rx.recv_timeout(std::time::Duration::from_secs(10)).expect("the reader blocked on a pipe");
    assert_eq!(answer, Err(FileError::NotAFile));
}

#[cfg(unix)]
#[test]
fn a_socket_is_refused_as_not_a_file_without_being_opened() {
    // The path is looked at before it is opened: opening a socket is an error of its own, and opening a device can have effects.
    use diacraft_engine::file::{FileError, read_file};
    let dir = Dir::new();
    let path = dir.path("sock");
    let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
    assert_eq!(read_file(std::path::Path::new(&path), 100), Err(FileError::NotAFile));
}

/// A package that makes the reader warn more than the open lists, under an extension that is not its type: the open lists 20 warnings, **the extension's note first** and the
/// reader's after it (it listed 21 when the note was put in after the cap), and the count is the whole (the review item 4).
#[test]
fn an_open_lists_twenty_warnings_with_the_extension_note_first_and_counts_them_all() {
    let dir = Dir::new();
    let mut builder = VsdxBuilder::new().document_type("application/vnd.ms-visio.stencil.main+xml");
    for n in 0..25 {
        builder = builder.page(PageSpec::new(1, &format!("Twin {n}")));
    }
    let mut s = Session::new();
    let result = run(&mut s, "file.open", json!({ "path": dir.write("many.vsdm", &builder.build()) }));
    let words: Vec<&str> = result["file"]["warnings"].as_array().unwrap().iter().map(|w| w.as_str().unwrap()).collect();
    assert_eq!(words.len(), 20, "{words:?}");
    assert!(words[0].contains("extension"), "the extension's note comes first: {words:?}");
    assert!(words[1..].iter().all(|w| w.contains("ID 1")), "and the reader's follow it: {words:?}");
    assert!(result["file"]["warning_count"].as_u64().unwrap() >= 25, "the count is the whole: {result}");
}
