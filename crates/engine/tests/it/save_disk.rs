//! A save and the file on the disk (and L3 of the M3 review): a file changed on the disk since the drawing was opened is not replaced
//! without `force`, `file.save` judges the extension as `file.save_as` does, and a drawing opened through a link saves to the file the link led
//! to when it was opened.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use diacraft_engine::{EngineError, Session};
use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape};
use serde_json::json;

use crate::{blank_session, run, snapshot};

/// A scratch folder under `TMPDIR`, removed on drop.
struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("diacraft-engine-disk-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn drawing(name: &str) -> Vec<u8> {
    VsdxBuilder::new().page(PageSpec::new(1, name).shapes(&shape(5, r#"<Cell N="Width" V="2.50"/>"#))).build()
}

/// A session that has opened `drawing("First")` from a file in `dir`, and edited it.
fn opened_and_edited(dir: &Dir) -> (Session, String) {
    let path = dir.path("plan.vsdx");
    std::fs::write(&path, drawing("First")).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    run(&mut s, "page.add", json!({"name": "Second"}));
    (s, path)
}

fn changed_on_disk(e: &EngineError) -> &str {
    match e {
        EngineError::ChangedOnDisk { msg, .. } => msg,
        other => panic!("expected ChangedOnDisk, got {other:?}"),
    }
}

#[test]
fn a_file_that_changed_on_the_disk_since_it_was_opened_is_not_replaced_without_force() {
    let dir = Dir::new();
    let (mut s, path) = opened_and_edited(&dir);
    // Something else writes the file.
    std::fs::write(&path, drawing("Written by something else")).unwrap();
    let before = snapshot(&s);
    for command in ["file.save", "file.save_as"] {
        let params = if command == "file.save" { json!({}) } else { json!({"path": path}) };
        let error = s.execute(command, &params).unwrap_err();
        let msg = changed_on_disk(&error);
        assert!(msg.contains("force") && !msg.contains("plan.vsdx"), "words for a person, and no path: {msg}");
        assert_eq!(snapshot(&s), before, "{command}: the drawing is as it was");
        assert_eq!(std::fs::read(&path).unwrap(), drawing("Written by something else"), "{command}: the disk is untouched");
    }
    assert!(s.is_dirty());
    // `force` replaces it, and then the file is the drawing's own again.
    let saved = run(&mut s, "file.save", json!({"force": true}));
    assert_eq!(saved["dirty"], false);
    assert!(!s.is_dirty());
    run(&mut s, "page.add", json!({}));
    run(&mut s, "file.save", json!({}));
    let mut again = Session::new();
    let opened = run(&mut again, "file.open", json!({"path": path}));
    assert_eq!(opened["pages"].as_array().unwrap().len(), 3);
}

#[test]
fn a_change_of_the_modified_time_alone_is_a_change() {
    let dir = Dir::new();
    let (mut s, path) = opened_and_edited(&dir);
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_modified(SystemTime::now() + Duration::from_secs(3600)).unwrap();
    drop(file);
    let error = s.execute("file.save", &json!({})).unwrap_err();
    changed_on_disk(&error);
    run(&mut s, "file.save", json!({"force": true}));
}

#[test]
fn a_file_that_is_as_it_was_saves_without_a_word_and_so_does_a_second_save() {
    let dir = Dir::new();
    let (mut s, path) = opened_and_edited(&dir);
    run(&mut s, "file.save", json!({}));
    run(&mut s, "page.add", json!({}));
    run(&mut s, "file.save", json!({"force": false}));
    // A file saved as another name is the drawing's file from then on, and is judged the same way.
    let other = dir.path("copy.vsdx");
    run(&mut s, "file.save_as", json!({"path": other}));
    run(&mut s, "page.add", json!({}));
    std::fs::write(&other, drawing("changed")).unwrap();
    changed_on_disk(&s.execute("file.save", &json!({})).unwrap_err());
    // And the first file, now not the drawing's, is not looked at when it is not the file written.
    std::fs::write(&path, drawing("changed too")).unwrap();
    run(&mut s, "file.save", json!({"force": true}));
}

#[test]
fn a_file_that_was_taken_away_is_written_again_and_a_new_name_is_not_compared() {
    let dir = Dir::new();
    let (mut s, path) = opened_and_edited(&dir);
    std::fs::remove_file(&path).unwrap();
    run(&mut s, "file.save", json!({}));
    assert!(std::path::Path::new(&path).is_file());
    // Saving as a file that is there, under another name, is the caller's choice: the check is for the drawing's own file.
    let there = dir.path("there.vsdx");
    std::fs::write(&there, b"anything").unwrap();
    run(&mut s, "file.save_as", json!({"path": there}));
    // A drawing from `file.new` has no file of its own to have changed.
    let mut fresh = blank_session();
    run(&mut fresh, "file.save_as", json!({"path": dir.path("fresh.vsdx")}));
}

#[test]
fn force_is_true_or_false() {
    let dir = Dir::new();
    let (mut s, _path) = opened_and_edited(&dir);
    for bad in [json!({"force": "yes"}), json!({"force": 1}), json!({"force": []})] {
        let error = s.execute("file.save", &bad).unwrap_err();
        assert!(matches!(error, EngineError::BadParams { .. }), "{bad}: {error:?}");
    }
    run(&mut s, "file.save", json!({"force": null}));
}

// ---- L3 ------------------------------------------------------------------------------------------------------------------------------

#[test]
fn file_save_judges_the_extension_as_save_as_does() {
    let dir = Dir::new();
    let path = dir.path("plan.dat");
    std::fs::write(&path, drawing("First")).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    run(&mut s, "page.add", json!({}));
    let before = snapshot(&s);
    let error = s.execute("file.save", &json!({})).unwrap_err();
    assert!(error.to_string().contains(".vsdx"), "{error}");
    assert_eq!(snapshot(&s), before);
    assert_eq!(std::fs::read(&path).unwrap(), drawing("First"), "the file is as it was");
    // A name with a drawing extension in any case is fine, and the file keeps the kind it was.
    let upper = dir.path("PLAN.VSDX");
    std::fs::write(&upper, drawing("First")).unwrap();
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": upper}));
    run(&mut t, "page.add", json!({}));
    assert_eq!(run(&mut t, "file.save", json!({}))["variant"], "vsdx");
}

#[cfg(unix)]
#[test]
fn a_drawing_opened_through_a_link_saves_to_the_file_the_link_led_to_when_it_was_opened() {
    use std::os::unix::fs::symlink;
    let dir = Dir::new();
    let (real, other, link) = (dir.path("real.vsdx"), dir.path("other.vsdx"), dir.path("link.vsdx"));
    std::fs::write(&real, drawing("Real")).unwrap();
    std::fs::write(&other, drawing("Other")).unwrap();
    symlink(&real, &link).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": link}));
    run(&mut s, "page.add", json!({"name": "Added"}));
    // The link is pointed at another drawing meanwhile.
    std::fs::remove_file(&link).unwrap();
    symlink(&other, &link).unwrap();
    run(&mut s, "file.save", json!({}));
    assert_eq!(std::fs::read(&other).unwrap(), drawing("Other"), "the file the link leads to now is not written");
    let mut again = Session::new();
    let opened = run(&mut again, "file.open", json!({"path": real}));
    assert_eq!(opened["pages"].as_array().unwrap().len(), 2, "the file the link led to when the drawing was opened is");
    assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    assert_eq!(s.path().unwrap().to_str().unwrap(), link, "and the drawing's name is still the one it was opened by");
}

#[cfg(unix)]
#[test]
fn a_file_changed_on_the_disk_is_noticed_through_a_link_too() {
    use std::os::unix::fs::symlink;
    let dir = Dir::new();
    let (real, link) = (dir.path("real.vsdx"), dir.path("link.vsdx"));
    std::fs::write(&real, drawing("Real")).unwrap();
    symlink(&real, &link).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": link}));
    run(&mut s, "page.add", json!({}));
    std::fs::write(&real, drawing("Changed")).unwrap();
    changed_on_disk(&s.execute("file.save", &json!({})).unwrap_err());
    run(&mut s, "file.save", json!({"force": true}));
}
