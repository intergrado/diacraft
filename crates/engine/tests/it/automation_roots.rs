//! Automation roots (gaps A2 and A5): every command that reads a file or writes one, run with roots in force, reads only inside a read root and writes only inside a write root.
//! A refusal is in words that name the rule and the flag, holds nothing of the path or of what is there, and changes nothing: not the session, not a file.
//!
//! The commands are the engine's: the MCP server and the control channel run the same ones, and have their tests of the wiring (`crates/mcp`, `crates/ui-egui`).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_engine::file_access::{self, AutomationRoots};
use diacraft_engine::{EngineError, OpenProblem, Session};
use serde_json::{Value, json};

use crate::pictures::real_png;
use crate::stencil_open::stencil_bytes;
use crate::{blank_session, run, snapshot};

/// A folder holding `read` (the read root), `write` (the write root) and `outside`, each with the files the commands read: a drawing, a stencil, a template and a picture. The files outside
/// are the same as the ones inside, so that the only difference between a command that works and one that is refused is where its path is.
struct World {
    base: PathBuf,
    roots: Arc<AutomationRoots>,
}

impl World {
    fn new() -> World {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let base = std::env::temp_dir().join(format!("diacraft-engine-roots-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&base);
        for folder in ["read", "write", "outside"] {
            std::fs::create_dir_all(base.join(folder)).unwrap();
        }
        let world = World { roots: AutomationRoots::new(&[&base.join("read")], &[&base.join("write")]).unwrap().unwrap(), base };
        for folder in ["read", "write", "outside"] {
            world.make_files(folder);
        }
        world
    }

    /// The drawing, stencil, template and picture of one folder, made with no roots in force.
    fn make_files(&self, folder: &str) {
        let save = |name: &str| {
            let mut s = blank_session();
            run(&mut s, "file.save_as", json!({"path": self.p(&format!("{folder}/{name}"))}));
        };
        save("drawing.diacraft");
        save("template.diacraft-template");
        std::fs::write(self.base.join(folder).join("stencil.vssx"), stencil_bytes()).unwrap();
        std::fs::write(self.base.join(folder).join("pic.png"), real_png(24, 12)).unwrap();
    }

    fn p(&self, relative: &str) -> String {
        self.base.join(relative).to_str().unwrap().to_owned()
    }

    /// What is in a file, or `None` when it is not there.
    fn bytes(&self, relative: &str) -> Option<Vec<u8>> {
        std::fs::read(self.base.join(relative)).ok()
    }

    /// Runs `work` with the roots in force.
    fn confined<T>(&self, work: impl FnOnce() -> T) -> T {
        file_access::confine(Some(&self.roots), work)
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// A command that is refused: the words, the session as it was before, and that the refusal names the rule and holds nothing of the path.
fn refused(w: &World, s: &mut Session, id: &str, params: Value) -> String {
    let before = snapshot(s);
    let error = w.confined(|| s.execute(id, &params)).unwrap_err();
    assert_eq!(snapshot(s), before, "a refused {id} changes nothing");
    let words = error.to_string();
    assert!(words.contains("automation"), "{id}: {words}");
    assert!(words.contains("--automation-"), "{id}: names the flag: {words}");
    assert!(!words.contains(&*w.base.to_string_lossy()), "{id}: holds the path: {words}");
    words
}

/// A session with an empty drawing.
fn session() -> Session {
    blank_session()
}

#[test]
fn file_open_reads_only_inside_a_read_root() {
    let w = World::new();
    let mut s = Session::new();
    let opened = w.confined(|| s.execute("file.open", &json!({"path": w.p("read/drawing.diacraft")}))).unwrap();
    assert_eq!(opened["file"]["variant"], "diacraft", "{opened}");
    let mut s = Session::new();
    let words = refused(&w, &mut s, "file.open", json!({"path": w.p("outside/drawing.diacraft")}));
    assert!(words.contains("reading") && words.contains("--automation-read-root"), "{words}");
    // A write root is no place to read.
    refused(&w, &mut s, "file.open", json!({"path": w.p("write/drawing.diacraft")}));
    // `..` out of the root, however it is spelled.
    refused(&w, &mut s, "file.open", json!({"path": format!("{}/../outside/drawing.diacraft", w.p("read"))}));
    refused(&w, &mut s, "file.open", json!({"path": format!("{}/sub/../../outside/drawing.diacraft", w.p("read"))}));
    // A file that is not there, outside, is outside and nothing more: it does not say whether anything is there.
    let missing = refused(&w, &mut s, "file.open", json!({"path": w.p("outside/nothing-here.diacraft")}));
    assert_eq!(missing, refused(&w, &mut s, "file.open", json!({"path": w.p("outside/drawing.diacraft")})));
}

#[test]
fn a_refused_open_is_the_kind_of_trouble_that_says_so() {
    let w = World::new();
    let mut s = Session::new();
    let e = w.confined(|| s.execute("file.open", &json!({"path": w.p("outside/drawing.diacraft")}))).unwrap_err();
    let EngineError::CannotOpen { problem, msg, .. } = e else { panic!("{e:?}") };
    assert_eq!(problem, OpenProblem::NotGranted);
    assert_eq!(problem.word(), "not_granted");
    assert!(msg.contains("automation path rejected"), "{msg}");
}

#[test]
fn a_template_file_is_read_only_inside_a_read_root() {
    let w = World::new();
    let mut s = Session::new();
    let made = w.confined(|| s.execute("file.new", &json!({"template": w.p("read/template.diacraft-template")}))).unwrap();
    assert!(made.get("template").is_some() || made.get("file").is_some(), "{made}");
    let mut s = Session::new();
    refused(&w, &mut s, "file.new", json!({"template": w.p("outside/template.diacraft-template")}));
}

#[test]
fn stencil_open_reads_only_inside_a_read_root() {
    let w = World::new();
    let mut s = session();
    let opened = w.confined(|| s.execute("stencil.open", &json!({"path": w.p("read/stencil.vssx")}))).unwrap();
    assert!(opened.get("id").is_some(), "{opened}");
    let words = refused(&w, &mut s, "stencil.open", json!({"path": w.p("outside/stencil.vssx")}));
    assert!(words.contains("reading"), "{words}");
    refused(&w, &mut s, "stencil.open", json!({"path": w.p("write/stencil.vssx")}));
}

#[test]
fn a_picture_is_read_only_inside_a_read_root() {
    let w = World::new();
    let mut s = session();
    let inserted = w.confined(|| s.execute("insert.picture", &json!({"path": w.p("read/pic.png"), "x": 3.0, "y": 3.0}))).unwrap();
    assert_eq!(inserted["format"], "png", "{inserted}");
    let words = refused(&w, &mut s, "insert.picture", json!({"path": w.p("outside/pic.png"), "x": 3.0, "y": 3.0}));
    assert!(words.contains("reading"), "{words}");
}

#[test]
fn a_position_picture_is_read_only_inside_a_read_root() {
    let w = World::new();
    let mut s = session();
    let boss = run(&mut s, "shape.drop", json!({"master": "orgchart.executive", "page": 1, "x": 4.25, "y": 9.5}))["id"].as_u64().unwrap();
    let set = w.confined(|| s.execute("orgchart.picture_set", &json!({"id": boss, "path": w.p("read/pic.png")}))).unwrap();
    assert_eq!(set["format"], "png", "{set}");
    refused(&w, &mut s, "orgchart.picture_set", json!({"id": boss, "path": w.p("outside/pic.png")}));
}

#[test]
fn file_save_as_writes_only_inside_a_write_root() {
    let w = World::new();
    let mut s = session();
    let saved = w.confined(|| s.execute("file.save_as", &json!({"path": w.p("write/new.vsdx")}))).unwrap();
    assert!(saved["bytes"].as_u64().unwrap() > 0, "{saved}");
    assert!(w.bytes("write/new.vsdx").is_some());
    let words = refused(&w, &mut s, "file.save_as", json!({"path": w.p("outside/new.diacraft")}));
    assert!(words.contains("writing") && words.contains("--automation-write-root"), "{words}");
    assert!(w.bytes("outside/new.diacraft").is_none(), "nothing was written");
    // A read root is no place to write, and a file that is there is not replaced.
    let before = w.bytes("read/drawing.diacraft").unwrap();
    refused(&w, &mut s, "file.save_as", json!({"path": w.p("read/drawing.diacraft")}));
    assert_eq!(w.bytes("read/drawing.diacraft"), Some(before), "the file is as it was");
    refused(&w, &mut s, "file.save_as", json!({"path": format!("{}/../outside/new.diacraft", w.p("write"))}));
    assert!(w.bytes("outside/new.diacraft").is_none());
    // Every type a Save As writes is judged the same.
    for name in ["x.vsdx", "x.vsdm", "x.vssx", "x.vstx", "x.odg", "x.otg", "x.diacraft", "x.diacraft-stencil", "x.diacraft-template"] {
        let outside = w.p(&format!("outside/{name}"));
        let e = w.confined(|| s.execute("file.save_as", &json!({"path": outside}))).unwrap_err().to_string();
        assert!(!w.base.join("outside").join(name).exists(), "{name}: written outside: {e}");
    }
}

#[test]
fn file_save_writes_the_drawings_own_file_only_inside_a_write_root() {
    let w = World::new();
    // A drawing opened from a folder that may be read and not written: a save is refused, and the file is as it was.
    let mut s = Session::new();
    w.confined(|| s.execute("file.open", &json!({"path": w.p("read/drawing.diacraft")}))).unwrap();
    run(&mut s, "page.add", json!({"name": "Edited"}));
    let before = w.bytes("read/drawing.diacraft").unwrap();
    let words = refused(&w, &mut s, "file.save", json!({}));
    assert!(words.contains("writing"), "{words}");
    assert_eq!(w.bytes("read/drawing.diacraft"), Some(before));
    // A drawing the person opened from a folder outside every root is no more written by a program than any other file there.
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": w.p("outside/drawing.diacraft")}));
    run(&mut s, "page.add", json!({"name": "Edited"}));
    let before = w.bytes("outside/drawing.diacraft").unwrap();
    refused(&w, &mut s, "file.save", json!({}));
    assert_eq!(w.bytes("outside/drawing.diacraft"), Some(before));
    // Inside a write root it is written.
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": w.p("write/drawing.diacraft")}));
    run(&mut s, "page.add", json!({"name": "Edited"}));
    let before = w.bytes("write/drawing.diacraft").unwrap();
    let saved = w.confined(|| s.execute("file.save", &json!({}))).unwrap();
    assert!(saved["bytes"].as_u64().unwrap() > 0, "{saved}");
    assert_ne!(w.bytes("write/drawing.diacraft"), Some(before), "the file was written");
}

#[test]
fn file_export_and_file_print_write_only_inside_a_write_root() {
    let w = World::new();
    let mut s = session();
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 3.0}));
    for (id, name) in [("file.export", "p.png"), ("file.export", "p.svg"), ("file.export", "p.pdf"), ("file.print", "print.pdf")] {
        let done = w.confined(|| s.execute(id, &json!({"path": w.p(&format!("write/{name}"))}))).unwrap();
        assert!(done["bytes"].as_u64().unwrap() > 0, "{id} {name}: {done}");
        assert!(w.bytes(&format!("write/{name}")).is_some());
        let words = refused(&w, &mut s, id, json!({"path": w.p(&format!("outside/{name}"))}));
        assert!(words.contains("writing"), "{id}: {words}");
        assert!(w.bytes(&format!("outside/{name}")).is_none(), "{id} {name}: nothing was written");
        refused(&w, &mut s, id, json!({"path": w.p(&format!("read/{name}"))}));
        assert!(w.bytes(&format!("read/{name}")).is_none(), "{id} {name}: a read root is no place to write");
    }
}

#[test]
fn file_check_compatibility_asks_only_about_a_place_that_may_be_written() {
    let w = World::new();
    let mut s = session();
    let asked = w.confined(|| s.execute("file.check_compatibility", &json!({"path": w.p("write/x.vsdx")}))).unwrap();
    assert_eq!(asked["format"], "vsdx", "{asked}");
    assert!(w.bytes("write/x.vsdx").is_none(), "a dry run writes nothing");
    refused(&w, &mut s, "file.check_compatibility", json!({"path": w.p("outside/x.vsdx")}));
}

#[test]
fn a_root_of_one_kind_grants_nothing_of_the_other() {
    let w = World::new();
    let reads = AutomationRoots::new(&[&w.base.join("read")], &[]).unwrap().unwrap();
    let mut s = session();
    let before = snapshot(&s);
    let e = file_access::confine(Some(&reads), || s.execute("file.save_as", &json!({"path": w.p("write/x.diacraft")}))).unwrap_err().to_string();
    assert!(e.contains("not granted") && e.contains("--automation-write-root"), "{e}");
    assert_eq!(snapshot(&s), before);
    assert!(
        file_access::confine(Some(&reads), || s.execute("stencil.open", &json!({"path": w.p("read/stencil.vssx")}))).is_ok(),
        "the read root reads"
    );
    let writes = AutomationRoots::new(&[], &[&w.base.join("write")]).unwrap().unwrap();
    let e = file_access::confine(Some(&writes), || s.execute("stencil.open", &json!({"path": w.p("write/stencil.vssx")}))).unwrap_err().to_string();
    assert!(e.contains("not granted") && e.contains("--automation-read-root"), "{e}");
    assert!(
        file_access::confine(Some(&writes), || s.execute("file.save_as", &json!({"path": w.p("write/ok.diacraft")}))).is_ok(),
        "the write root writes"
    );
}

#[test]
fn the_person_is_not_confined() {
    let w = World::new();
    let mut s = Session::new();
    // What the person does in the window runs `unconfined`, whatever the request in force around it.
    let opened = w.confined(|| file_access::unconfined(|| s.execute("file.open", &json!({"path": w.p("outside/drawing.diacraft")})))).unwrap();
    assert_eq!(opened["file"]["variant"], "diacraft", "{opened}");
    // And with no roots in force nothing is checked: the programs' behaviour with no flag.
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": w.p("outside/drawing.diacraft")}));
    run(&mut s, "file.save_as", json!({"path": w.p("outside/again.diacraft")}));
}

#[cfg(unix)]
#[test]
fn a_link_in_a_root_that_leads_out_is_not_read_and_not_written() {
    let w = World::new();
    std::os::unix::fs::symlink(w.base.join("outside/drawing.diacraft"), w.base.join("read/leak.diacraft")).unwrap();
    std::os::unix::fs::symlink(w.base.join("outside"), w.base.join("write/away")).unwrap();
    std::os::unix::fs::symlink(w.base.join("outside/victim.diacraft"), w.base.join("write/victim.diacraft")).unwrap();
    std::fs::write(w.base.join("outside/victim.diacraft"), b"not a drawing").unwrap();
    let mut s = session();
    refused(&w, &mut s, "file.open", json!({"path": w.p("read/leak.diacraft")}));
    refused(&w, &mut s, "file.save_as", json!({"path": w.p("write/away/new.diacraft")}));
    assert!(w.bytes("outside/new.diacraft").is_none(), "a save through a folder link made no file outside");
    refused(&w, &mut s, "file.save_as", json!({"path": w.p("write/victim.diacraft")}));
    assert_eq!(w.bytes("outside/victim.diacraft"), Some(b"not a drawing".to_vec()), "a save through a file link replaced nothing outside");
    refused(&w, &mut s, "file.export", json!({"path": w.p("write/away/p.png")}));
    assert!(w.bytes("outside/p.png").is_none());
    // The same through the stencil, picture and template paths.
    std::os::unix::fs::symlink(w.base.join("outside/stencil.vssx"), w.base.join("read/leak.vssx")).unwrap();
    std::os::unix::fs::symlink(w.base.join("outside/pic.png"), w.base.join("read/leak.png")).unwrap();
    refused(&w, &mut s, "stencil.open", json!({"path": w.p("read/leak.vssx")}));
    refused(&w, &mut s, "insert.picture", json!({"path": w.p("read/leak.png")}));
}

/// A link that stays inside a root is followed, and a drawing opened through it is saved where the link led.
#[cfg(unix)]
#[test]
fn a_link_that_stays_inside_the_roots_works() {
    let w = World::new();
    std::os::unix::fs::symlink(w.base.join("read/drawing.diacraft"), w.base.join("read/alias.diacraft")).unwrap();
    let mut s = Session::new();
    let opened = w.confined(|| s.execute("file.open", &json!({"path": w.p("read/alias.diacraft")}))).unwrap();
    assert_eq!(opened["file"]["variant"], "diacraft", "{opened}");
}

#[test]
fn a_refusal_changes_nothing_and_a_command_after_it_works() {
    let w = World::new();
    let mut s = session();
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 3.0}));
    let before = snapshot(&s);
    for params in [json!({"path": w.p("outside/x.vsdx")}), json!({"path": ""}), json!({"path": format!("{}/{}", w.p("write"), "a".repeat(70_000))})] {
        let e = w.confined(|| s.execute("file.save_as", &params));
        assert!(e.is_err(), "{params}");
        assert_eq!(snapshot(&s), before);
    }
    w.confined(|| s.execute("file.save_as", &json!({"path": w.p("write/after.vsdx")}))).unwrap();
}
