//! What a start remembers about graphics tests that never returned (`marker`, public issue #17): the text of the two files, the marker of a test that is
//! running, the adapters a start leaves out, and a start that finds files that are no use.

use std::path::{Path, PathBuf};

use diacraft_desktop::graphics::{Key, key_of};
use diacraft_desktop::marker::{HEADER, LEFT_OUT_FILE, MARKER_PREFIX, MARKER_SUFFIX, MOST_LEFT_OUT, Store, describe, format_file, parse_file};
use eframe::wgpu::Backend;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("diacraft-marker-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn store(&self, version: &str) -> Store {
        Store::new(Some(self.0.clone()), version)
    }

    /// The names of the files in the folder, sorted.
    fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.0).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    fn markers(&self) -> Vec<String> {
        self.files().into_iter().filter(|n| n.starts_with(MARKER_PREFIX) && n.ends_with(MARKER_SUFFIX)).collect()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

fn key(name: &str, backend: Backend) -> Key {
    key_of(name, backend)
}

fn uhd() -> Key {
    key("Intel(R) UHD Graphics", Backend::Vulkan)
}

// ---- the text of the files ----

#[test]
fn a_file_is_text_with_a_header_a_version_and_a_line_for_each_adapter() {
    let keys = vec![uhd(), key("Intel(R) UHD Graphics", Backend::Dx12)];
    let text = format_file("0.2.4", "left-out", &keys);
    assert_eq!(text, "diacraft-graphics 1\nversion 0.2.4\nleft-out vulkan Intel(R) UHD Graphics\nleft-out dx12 Intel(R) UHD Graphics\n");
    assert_eq!(HEADER, "diacraft-graphics 1");
    let parsed = parse_file(&text, "left-out").unwrap();
    assert_eq!((parsed.version.as_str(), parsed.keys.clone()), ("0.2.4", keys));
    // The lines of the other kind of file are not this file's.
    assert!(parse_file(&text, "testing").unwrap().keys.is_empty());
    assert_eq!(describe(&uhd()), "Intel(R) UHD Graphics (vulkan)");
}

#[test]
fn text_that_is_not_ours_is_refused_in_words_and_lines_we_do_not_know_are_skipped() {
    for (text, why) in [
        ("", "does not begin"),
        ("hello\nversion 0.2.4\n", "does not begin"),
        ("diacraft-graphics 2\nversion 0.2.4\n", "does not begin"),
        ("diacraft-graphics 1\nleft-out vulkan Radeon\n", "no version"),
        ("diacraft-graphics 1\nversion \n", "no version"),
    ] {
        let error = parse_file(text, "left-out").unwrap_err();
        assert!(error.contains(why), "{text:?}: {error}");
    }
    let messy = "diacraft-graphics 1\r\nversion 0.2.4\r\nfuture-line something\nleft-out nosuchbackend Radeon\nleft-out vulkan \nleft-out vulkan\nleft-out gl Radeon (GL)\nleft-out gl Radeon (GL)\nleft-out\n";
    let parsed = parse_file(messy, "left-out").unwrap();
    assert_eq!(parsed.version, "0.2.4");
    assert_eq!(
        parsed.keys,
        vec![key("Radeon (GL)", Backend::Gl)],
        "a back end wgpu does not have, an empty name and a line with no name are skipped, and an adapter is listed once"
    );
}

#[test]
fn a_file_holds_at_most_so_many_adapters_and_a_name_is_one_bounded_line() {
    let many: Vec<Key> = (0..40).map(|i| key(&format!("GPU {i}"), Backend::Vulkan)).collect();
    let parsed = parse_file(&format_file("0.2.4", "left-out", &many), "left-out").unwrap();
    assert_eq!(parsed.keys.len(), MOST_LEFT_OUT);
    assert_eq!(parsed.keys.first(), Some(&key("GPU 0", Backend::Vulkan)));
    // A driver's text: a line break cannot start a line of its own, a NUL a driver ends it with is a space that is trimmed, and a long name is cut.
    let wild = key("Evil\nleft-out dx12 Other\0", Backend::Vulkan);
    assert_eq!(wild.0, "Evil left-out dx12 Other");
    let long = key(&"x".repeat(1000), Backend::Gl);
    assert_eq!(long.0.chars().count(), 160);
    let text = format_file("0.2.4", "testing", std::slice::from_ref(&wild));
    assert_eq!(text.lines().count(), 3, "{text:?}");
    let parsed = parse_file(&text, "testing").unwrap();
    assert_eq!(parsed.keys, vec![wild]);
    // The long version of a file is cut, not trusted.
    let parsed = parse_file(&format!("{HEADER}\nversion {}\n", "9".repeat(500)), "testing").unwrap();
    assert_eq!(parsed.version.len(), 40);
}

// ---- a test that is running ----

#[test]
fn a_test_writes_a_marker_before_it_runs_and_removes_it_when_it_returns() {
    let dir = TempDir::new();
    let store = dir.store("0.2.4");
    let marker = store.begin(&uhd());
    let names = dir.markers();
    assert_eq!(names, vec![format!("{MARKER_PREFIX}{}{MARKER_SUFFIX}", std::process::id())], "one file for each start, named by its process");
    let text = std::fs::read_to_string(dir.path().join(&names[0])).unwrap();
    assert_eq!(text, "diacraft-graphics 1\nversion 0.2.4\ntesting vulkan Intel(R) UHD Graphics\n", "it names the adapter and the version");
    marker.done();
    assert!(dir.files().is_empty(), "a test that returned leaves nothing: {:?}", dir.files());
    assert_eq!(dir.store("0.2.4").open(), Vec::<Key>::new());
}

#[test]
fn a_start_that_ended_during_a_test_leaves_that_adapter_out_of_the_next_start_and_the_ones_after() {
    crate::capture_logs();
    let dir = TempDir::new();
    let adapter = key("Crashy Driver GPU 7701", Backend::Vulkan);
    // The start that crashed: the marker is on the disk, nothing removed it (dropped, as the process's end drops the handle).
    drop(dir.store("0.2.4").begin(&adapter));
    assert_eq!(dir.markers().len(), 1);

    let left_out = dir.store("0.2.4").open();
    assert_eq!(left_out, vec![adapter.clone()], "the next start leaves the adapter out");
    assert!(dir.markers().is_empty(), "and its marker is used up");
    assert!(
        crate::logged().iter().any(|l| l.contains("the last start ended while testing Crashy Driver GPU 7701 (vulkan): it is left out")),
        "{:?}",
        crate::logged()
    );
    let list = std::fs::read_to_string(dir.path().join(LEFT_OUT_FILE)).unwrap();
    assert_eq!(list, "diacraft-graphics 1\nversion 0.2.4\nleft-out vulkan Crashy Driver GPU 7701\n");

    // The start after that has no marker, and still leaves it out: the list lasts.
    assert_eq!(dir.store("0.2.4").open(), vec![adapter.clone()]);
    // A start that crashes again on another adapter adds it.
    let other = key("Crashy Driver GPU 7701", Backend::Dx12);
    drop(dir.store("0.2.4").begin(&other));
    assert_eq!(dir.store("0.2.4").open(), vec![adapter, other]);
}

#[test]
fn a_marker_of_a_start_that_is_still_running_is_not_blamed_on_any_adapter() {
    // Several drawings opened from the file manager at once start several programs, each testing its adapters: one that finds the marker of another that is still testing leaves nothing out.
    crate::capture_logs();
    let dir = TempDir::new();
    let running = dir.store("0.2.4").begin(&key("Busy GPU 7702", Backend::Dx12));
    assert_eq!(dir.store("0.2.4").open(), Vec::<Key>::new(), "its owner is alive");
    assert_eq!(dir.markers().len(), 1, "and its marker is not taken");
    assert!(!dir.files().contains(&LEFT_OUT_FILE.to_owned()));
    assert!(crate::logged().iter().any(|l| l.contains("still running") && l.contains(MARKER_PREFIX)), "{:?}", crate::logged());
    running.done();
    assert!(dir.files().is_empty());
}

// ---- what lasts ----

#[test]
fn the_list_lasts_until_the_version_changes() {
    crate::capture_logs();
    let dir = TempDir::new();
    dir.store("0.2.4").leave_out(&uhd());
    assert_eq!(dir.store("0.2.4").open(), vec![uhd()]);
    let after = dir.store("0.2.5").open();
    assert!(after.is_empty(), "a newer build may have mended the driver's trouble: every adapter is tested again");
    assert!(!dir.files().contains(&LEFT_OUT_FILE.to_owned()), "the old list is removed: {:?}", dir.files());
    assert!(
        crate::logged().iter().any(|l| l.contains("the adapters DiaCraft 0.2.4 left out are tested again by DiaCraft 0.2.5")),
        "{:?}",
        crate::logged()
    );
}

#[test]
fn a_marker_of_another_version_names_nothing() {
    crate::capture_logs();
    let dir = TempDir::new();
    drop(dir.store("0.2.3").begin(&key("Old Build GPU 7703", Backend::Vulkan)));
    assert!(dir.store("0.2.4").open().is_empty());
    assert!(dir.markers().is_empty(), "it is removed all the same");
    assert!(crate::logged().iter().any(|l| l.contains("from DiaCraft 0.2.3, not 0.2.4")), "{:?}", crate::logged());
}

#[test]
fn the_list_is_bounded_and_drops_the_oldest() {
    let dir = TempDir::new();
    let store = dir.store("0.2.4");
    for i in 0..(MOST_LEFT_OUT + 5) {
        store.leave_out(&key(&format!("GPU {i}"), Backend::Vulkan));
    }
    store.leave_out(&key("GPU 20", Backend::Vulkan));
    let kept = dir.store("0.2.4").open();
    assert_eq!(kept.len(), MOST_LEFT_OUT);
    assert_eq!(kept.first(), Some(&key("GPU 5", Backend::Vulkan)), "the first five are gone");
    assert_eq!(kept.last(), Some(&key("GPU 20", Backend::Vulkan)));
    let size = std::fs::metadata(dir.path().join(LEFT_OUT_FILE)).unwrap().len();
    assert!(size < 4096, "{size} bytes");
}

#[test]
fn reset_forgets_every_adapter_left_out_and_leaves_a_start_that_is_running_alone() {
    let dir = TempDir::new();
    dir.store("0.2.4").leave_out(&uhd());
    // The marker of a start that is gone (another process's, written by hand: two markers of one process would share a file), and one that is running.
    let gone = key("Gone GPU 7704", Backend::Dx12);
    std::fs::write(dir.path().join(format!("{MARKER_PREFIX}99999{MARKER_SUFFIX}")), format_file("0.2.4", "testing", std::slice::from_ref(&gone)))
        .unwrap();
    let running = dir.store("0.2.4").begin(&key("Running GPU 7705", Backend::Dx12));
    let forgotten = dir.store("0.2.4").reset();
    assert_eq!(forgotten, vec![uhd(), gone], "it says what was left out");
    assert_eq!(dir.files(), dir.markers(), "the list and the marker of the gone start are removed: {:?}", dir.files());
    assert_eq!(dir.markers().len(), 1, "the marker of the start that is running is not taken from it");
    assert!(dir.store("0.2.4").open().is_empty(), "every adapter is tested again");
    running.done();
    assert!(dir.store("0.2.4").reset().is_empty(), "nothing to forget");
}

// ---- files that are no use never stop a start ----

#[test]
fn files_that_are_no_use_are_ignored_with_a_line_in_the_log_and_never_stop_a_start() {
    crate::capture_logs();
    let dir = TempDir::new();
    let list = dir.path().join(LEFT_OUT_FILE);
    let marker = |n: u32| dir.path().join(format!("{MARKER_PREFIX}{n}{MARKER_SUFFIX}"));
    // A list that is not text, one that is too long, one that is another program's text; a marker of each kind, and one with nothing in it.
    std::fs::write(&list, [0xff, 0xfe, 0x00, 0xc3]).unwrap();
    std::fs::write(marker(1), "x".repeat(10_000)).unwrap();
    std::fs::write(marker(2), "").unwrap();
    std::fs::write(marker(3), "diacraft-graphics 1\nversion 0.2.4\n").unwrap();
    std::fs::write(marker(4), "diacraft-graphics 1\ntesting vulkan Radeon\n").unwrap();
    std::fs::write(marker(6), "<xml>not ours</xml>").unwrap();
    // Not a marker: a name that merely starts alike, and a folder named like one.
    std::fs::write(dir.path().join("graphics-testing-notes.txt"), "mine").unwrap();
    std::fs::create_dir(dir.path().join(format!("{MARKER_PREFIX}5{MARKER_SUFFIX}"))).unwrap();
    assert!(dir.store("0.2.4").open().is_empty());
    assert_eq!(
        dir.files(),
        vec!["graphics-testing-notes.txt".to_owned(), format!("{MARKER_PREFIX}5{MARKER_SUFFIX}")],
        "what was no use is gone, and what is not ours is left"
    );
    let log = crate::logged();
    for needle in ["is not text", "is longer than 4096 bytes", "names no version", "names no adapter", "does not begin"] {
        assert!(log.iter().any(|l| l.contains("graphics:") && l.contains(needle)), "{needle}: {log:?}");
    }
}

#[test]
fn a_list_that_is_a_folder_or_a_folder_that_cannot_be_written_is_a_line_in_the_log() {
    crate::capture_logs();
    let dir = TempDir::new();
    // Something that is no file where the list should be: reading it fails and so does the write over it. The start goes on.
    std::fs::create_dir(dir.path().join(LEFT_OUT_FILE)).unwrap();
    let store = dir.store("0.2.4");
    assert!(store.open().is_empty());
    store.leave_out(&key("Unwritable GPU 7706", Backend::Vulkan));
    assert!(crate::logged().iter().any(|l| l.contains("graphics:") && l.contains(LEFT_OUT_FILE)), "{:?}", crate::logged());

    // A settings folder that is a file: no marker, no list, no panic.
    let blocked = dir.path().join("settings");
    std::fs::write(&blocked, "a file").unwrap();
    let store = Store::new(Some(blocked.join("diacraft")), "0.2.4");
    let marker = store.begin(&key("Nowhere GPU 7707", Backend::Dx12));
    marker.done();
    assert!(store.open().is_empty());
    store.leave_out(&key("Nowhere GPU 7707", Backend::Dx12));
    assert!(store.reset().is_empty());
    assert!(crate::logged().iter().any(|l| l.contains("no marker for the test of Nowhere GPU 7707 (dx12)")), "{:?}", crate::logged());
}

#[test]
fn a_run_with_no_settings_folder_keeps_nothing_on_the_disk() {
    // DIACRAFT_NO_PREFS: the marker is memory only, which is to say none; the start's own list is the memory's (graphics::Memory).
    let store = Store::default();
    assert_eq!(store.dir(), None);
    let marker = store.begin(&uhd());
    marker.done();
    store.leave_out(&uhd());
    assert!(store.open().is_empty());
    assert!(store.reset().is_empty());
    assert_eq!(Store::new(None, "0.2.4").dir(), None);
}

// ---- what the tries of a start leave out (graphics::Memory) ----

use diacraft_desktop::graphics::Memory;

#[test]
fn a_start_leaves_out_what_earlier_starts_left_out_and_what_failed_in_it() {
    let (a, b, c) = (key("GPU A 7710", Backend::Vulkan), key("GPU B 7710", Backend::Dx12), key("GPU C 7710", Backend::Gl));
    let memory = Memory::new(Store::default(), vec![a.clone()]);
    assert_eq!((memory.failed(), memory.left_out(), memory.excluded()), (vec![], vec![a.clone()], vec![a.clone()]));
    memory.chose(b.clone());
    assert_eq!(memory.fail_chosen(), Some(b.clone()));
    assert_eq!(memory.excluded(), vec![b.clone(), a.clone()], "what failed in this start, then what was left out");
    memory.chose(a.clone());
    memory.fail_chosen();
    assert_eq!(memory.excluded(), vec![b.clone(), a.clone()], "an adapter in both lists is listed once");
    memory.leave_out(&c);
    memory.leave_out(&c);
    assert_eq!(memory.left_out(), vec![a.clone(), c.clone()]);
    assert_eq!(memory.excluded(), vec![b, a, c]);
}

#[test]
fn an_adapter_left_out_in_this_start_is_in_the_file_for_the_next_one() {
    let dir = TempDir::new();
    let memory = Memory::new(dir.store("0.2.4"), Vec::new());
    assert_eq!(memory.store().dir(), Some(dir.path()));
    let hung = key("Hung GPU 7711", Backend::Vulkan);
    memory.leave_out(&hung);
    memory.leave_out(&hung);
    assert_eq!(memory.excluded(), vec![hung.clone()]);
    let list = std::fs::read_to_string(dir.path().join(LEFT_OUT_FILE)).unwrap();
    assert_eq!(list, "diacraft-graphics 1\nversion 0.2.4\nleft-out vulkan Hung GPU 7711\n", "once");
    let next = Memory::new(dir.store("0.2.4"), dir.store("0.2.4").open());
    assert_eq!(next.excluded(), vec![hung]);
    // With no settings folder (DIACRAFT_NO_PREFS) the start still leaves it out for the rest of the start, and no file is made.
    let none = Memory::default();
    none.leave_out(&uhd());
    assert_eq!(none.excluded(), vec![uhd()]);
}

// ---- hostile files (AGENTS.md: a file is hostile) ----

mod hostile {
    use diacraft_desktop::graphics::key_of;
    use diacraft_desktop::marker::{MOST_LEFT_OUT, format_file, parse_file};
    use eframe::wgpu::Backend;
    use proptest::prelude::*;

    fn backend() -> impl Strategy<Value = Backend> {
        prop::sample::select(Backend::ALL.to_vec())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        /// Whatever bytes a file holds, reading them never panics, and what is read is bounded: at most 16 adapters, each a name of at most 160 characters on one line.
        #[test]
        fn any_text_is_read_without_a_panic_and_within_the_bounds(bytes in prop::collection::vec(any::<u8>(), 0..6000), verb in prop::sample::select(vec!["testing", "left-out"])) {
            let text = String::from_utf8_lossy(&bytes);
            if let Ok(parsed) = parse_file(&text, verb) {
                prop_assert!(parsed.keys.len() <= MOST_LEFT_OUT);
                prop_assert!(parsed.version.chars().count() <= 40);
                for (name, _) in &parsed.keys {
                    prop_assert!(!name.is_empty() && name.chars().count() <= 160 && !name.chars().any(char::is_control), "{name:?}");
                }
            }
        }

        /// Lines made like the real ones, with any names: what is written is what is read, in the form `key_of` gives names, at most 16 of them and none twice.
        #[test]
        fn what_is_written_is_read_back(names in prop::collection::vec((".{0,300}", backend()), 0..24)) {
            let keys: Vec<_> = names.iter().map(|(name, backend)| key_of(name, *backend)).filter(|k| !k.0.is_empty()).collect();
            let text = format_file("0.2.4", "left-out", &keys);
            let parsed = parse_file(&text, "left-out").unwrap();
            prop_assert_eq!(parsed.version.as_str(), "0.2.4");
            let mut want: Vec<(String, Backend)> = Vec::new();
            for key in keys {
                if !want.contains(&key) && want.len() < MOST_LEFT_OUT {
                    want.push(key);
                }
            }
            prop_assert_eq!(parsed.keys, want);
        }
    }
}
