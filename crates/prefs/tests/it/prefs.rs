//! The person's settings: `config.toml` in the platform's config folder through craft-settings, one store for the window, the command line and the MCP server.
//!
//! Every folder here is a temporary one and every environment is a made-up one: no test reads or writes the real profile (the folder rules are given a `Probe` of their own).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use craft_settings::Settings;
use diacraft_prefs::{
    AutoCorrectExceptions, AutoCorrectPrefs, Change, DictionaryEntry, EnginePrefs, FileKind, FileName, LanguagePrefs, LocaleSource, Location,
    MAX_CONFIG_BYTES, MAX_DICTIONARIES, MAX_EDITING_LANGUAGES, MAX_EXCEPTIONS, OpenOptions, Prefs, PrefsError, Probe, Profile, ProfileChange,
    ProofingPrefs, Report, SCHEMA_VERSION, diff, location_for,
};
use serde::{Deserialize, Serialize};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("diacraft-prefs-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn config(&self) -> PathBuf {
        self.0.join("config.toml")
    }

    fn text(&self) -> String {
        std::fs::read_to_string(self.config()).unwrap()
    }

    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.0).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixed(locale: &str) -> LocaleSource {
    LocaleSource::Fixed(Some(locale.to_owned()))
}

/// A profile in `dir`, saved at once by `tick`, with an English system.
fn open(dir: &Path) -> (Profile<EnginePrefs>, Report) {
    Profile::open_with(OpenOptions::new(Location::Dir(dir.to_path_buf())).locale(fixed("en_US.UTF-8")).debounce(Duration::ZERO))
}

fn env_of(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
    move |key| pairs.iter().find(|(k, _)| *k == key).map(|(_, v)| OsString::from(*v))
}

fn probe(pairs: &'static [(&'static str, &'static str)], platform: Option<PathBuf>) -> Probe {
    Probe { env: Box::new(env_of(pairs)), exe_dir: None, platform_config_dir: platform }
}

fn dictionary_file(name: &str) -> FileName {
    FileName::new(FileKind::Dictionary, name).unwrap()
}

/// Every key of every section away from its default.
fn lived_in() -> EnginePrefs {
    EnginePrefs {
        proofing: ProofingPrefs {
            check_as_you_type: false,
            hide_errors: true,
            ignore_uppercase: false,
            ignore_numbers: false,
            ignore_internet: false,
            flag_repeated: false,
            main_dictionary_only: true,
            grammar: true,
            dictionaries: vec![
                DictionaryEntry { name: "Custom".into(), file: "Custom.dic".into(), language: "all".into(), default: true },
                DictionaryEntry { name: "Deutsch".into(), file: "Deutsch.dic".into(), language: "de-DE".into(), default: false },
            ],
        },
        autocorrect: AutoCorrectPrefs {
            two_initial_capitals: false,
            sentence_capitals: false,
            day_names: false,
            caps_lock: false,
            replace_text: false,
            use_suggestions: false,
            smart_quotes: false,
            fractions: false,
            arrows_and_faces: false,
            exceptions: AutoCorrectExceptions { initial_caps: vec!["IDs".into(), "KBs".into()], first_letter: vec!["e.g.".into()] },
        },
        language: LanguagePrefs { editing: vec!["pt-BR".into(), "en-GB".into()] },
    }
}

#[test]
fn the_defaults_are_the_designs() {
    let d = EnginePrefs::default();
    let p = &d.proofing;
    assert_eq!((p.check_as_you_type, p.hide_errors, p.ignore_uppercase, p.ignore_numbers), (true, false, true, true));
    assert_eq!((p.ignore_internet, p.flag_repeated, p.main_dictionary_only, p.grammar), (true, true, false, false), "grammar is an extra, off");
    assert!(p.dictionaries.is_empty(), "the first Add makes Custom.dic");
    let a = &d.autocorrect;
    assert!(
        a.two_initial_capitals
            && a.sentence_capitals
            && a.day_names
            && a.caps_lock
            && a.replace_text
            && a.use_suggestions
            && a.smart_quotes
            && a.fractions
            && a.arrows_and_faces,
        "AutoCorrect is on with every option (the owner, O3)"
    );
    assert!(d.language.editing.is_empty(), "the first editing language comes from the system when a profile is opened");
    assert_eq!(d.language.preferred(), "en-US", "and en-US when nothing says");
    assert_eq!(EnginePrefs::APP_ID, "diacraft");
    assert_eq!(EnginePrefs::SCHEMA_VERSION, SCHEMA_VERSION);
}

#[test]
fn a_store_in_a_folder_round_trips_every_section() {
    let tmp = TempDir::new();
    let (mut p, report) = open(tmp.path());
    assert!(report.persistent && !report.has_problems(), "{report:?}");
    assert_eq!(p.dir(), Some(tmp.path()));
    // Every section differs from its default, so each key's round trip is seen.
    let (before, after) = (EnginePrefs::default(), lived_in());
    assert_ne!(before.proofing, after.proofing);
    assert_ne!(before.autocorrect, after.autocorrect);
    assert_ne!(before.language, after.language);
    p.update(|e| *e = lived_in());
    p.flush().unwrap();
    let text = tmp.text();
    for section in ["schema_version = 1", "[proofing]", "[autocorrect]", "[autocorrect.exceptions]", "[language]"] {
        assert!(text.contains(section), "{section} in\n{text}");
    }
    let (q, report) = open(tmp.path());
    assert!(!report.has_problems(), "{report:?}");
    assert_eq!(*q.get(), lived_in());
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Window {
    #[serde(flatten)]
    engine: EnginePrefs,
    interface: Interface,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Interface {
    language: String,
}

impl Settings for Window {
    const APP_ID: &'static str = "diacraft";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;
}

impl Prefs for Window {
    fn engine(&self) -> &EnginePrefs {
        &self.engine
    }
    fn engine_mut(&mut self) -> &mut EnginePrefs {
        &mut self.engine
    }
}

const HAND_MADE: &str = "# my notes: keep me\nschema_version = 1\n\n[proofing]\nignore_uppercase = false   # I like shouting\n\n[interface]\nlanguage = \"de\"\n\n[future]\nkey = 1\n";

#[test]
fn unknown_keys_and_comments_survive_a_save_by_a_type_that_does_not_know_them() {
    let tmp = TempDir::new();
    std::fs::write(tmp.config(), HAND_MADE).unwrap();
    // The command line's type knows no `[interface]` and no `[future]`.
    let (mut cli, report) = open(tmp.path());
    assert!(!report.has_problems(), "{report:?}");
    assert!(!cli.get().proofing.ignore_uppercase);
    cli.update(|e| e.proofing.hide_errors = true);
    cli.flush().unwrap();
    let text = tmp.text();
    for kept in ["# my notes: keep me", "# I like shouting", "[interface]", "language = \"de\"", "[future]", "key = 1", "hide_errors = true"] {
        assert!(text.contains(kept), "{kept} in\n{text}");
    }
    // The window's type has the engine's sections and its own, in one file.
    let options = OpenOptions::new(Location::Dir(tmp.path().to_path_buf())).locale(fixed("en_US")).debounce(Duration::ZERO);
    let (mut window, report) = Profile::<Window>::open_with(options);
    assert!(!report.has_problems(), "{report:?}");
    assert_eq!(window.get().interface.language, "de");
    assert!(window.get().engine.proofing.hide_errors, "the change of the other type is read");
    window.update(|w| w.interface.language = "fr".into());
    window.flush().unwrap();
    let (cli, _) = open(tmp.path());
    assert!(!cli.get().proofing.ignore_uppercase && cli.get().proofing.hide_errors, "the engine's sections are as they were");
    let text = tmp.text();
    for kept in ["# my notes: keep me", "[future]", "language = \"fr\""] {
        assert!(text.contains(kept), "{kept} in\n{text}");
    }
}

#[test]
fn no_prefs_and_an_empty_config_dir_read_and_write_nothing() {
    const NO_PREFS: &[(&str, &str)] = &[("DIACRAFT_NO_PREFS", "1")];
    const NO_PREFS_EMPTY: &[(&str, &str)] = &[("DIACRAFT_NO_PREFS", "")];
    const EMPTY_DIR: &[(&str, &str)] = &[("DIACRAFT_CONFIG_DIR", "")];
    for pairs in [NO_PREFS, NO_PREFS_EMPTY, EMPTY_DIR] {
        let tmp = TempDir::new();
        let place = tmp.path().join("diacraft");
        std::fs::create_dir_all(&place).unwrap();
        let existing = "schema_version = 1\n[proofing]\nignore_uppercase = false\n";
        std::fs::write(place.join("config.toml"), existing).unwrap();
        let options = OpenOptions::new(location_for(&env_of(pairs))).probe(probe(pairs, Some(tmp.path().to_path_buf()))).locale(fixed("de_DE"));
        let (mut p, report) = Profile::<EnginePrefs>::open_with(options);
        assert!(!report.persistent && report.dir.is_none() && p.dir().is_none() && !p.is_persistent(), "{pairs:?}: {report:?}");
        assert!(p.get().proofing.ignore_uppercase, "{pairs:?}: the profile on disk was not read");
        assert_eq!(p.get().language.editing, vec!["de-DE".to_owned()], "the system's language is still read");
        // A session can still change settings and keep words, for as long as it runs.
        let changes = p.update(|e| e.proofing.hide_errors = true);
        assert_eq!(changes.len(), 1);
        let custom = dictionary_file("Custom.dic");
        p.add_word(&custom, "alpha").unwrap();
        assert_eq!(p.read_file(&custom).unwrap().map(|b| String::from_utf8(b).unwrap()), Some("alpha\n".to_owned()));
        p.flush().unwrap();
        assert_eq!(std::fs::read_to_string(place.join("config.toml")).unwrap(), existing, "{pairs:?}: the file is as it was");
        assert!(!place.join("proofing").exists(), "{pairs:?}: no word file");
        assert_eq!(tmp.names(), vec!["diacraft".to_owned()], "{pairs:?}: nothing else made");
    }
}

#[test]
fn the_environment_decides_where_the_profile_is() {
    let nowhere: &[(&str, &str)] = &[];
    assert_eq!(location_for(&env_of(nowhere)), Location::Auto, "the folder rules of craft-settings decide");
    assert_eq!(location_for(&env_of(&[("DIACRAFT_NO_PREFS", "1")])), Location::Memory);
    assert_eq!(location_for(&env_of(&[("DIACRAFT_NO_PREFS", "")])), Location::Memory, "set at all, whatever it holds");
    assert_eq!(location_for(&env_of(&[("DIACRAFT_CONFIG_DIR", "/elsewhere")])), Location::Auto, "craft-settings reads its own variable");
    // The variable moves the folder.
    let tmp = TempDir::new();
    let elsewhere = tmp.path().join("elsewhere");
    let env = {
        let elsewhere = elsewhere.clone();
        move |key: &str| (key == "DIACRAFT_CONFIG_DIR").then(|| elsewhere.clone().into_os_string())
    };
    let probe = Probe { env: Box::new(env.clone()), exe_dir: None, platform_config_dir: None };
    let (mut p, report) = Profile::<EnginePrefs>::open_with(OpenOptions::new(location_for(&env)).probe(probe).locale(fixed("en")));
    assert_eq!(p.dir(), Some(elsewhere.as_path()), "{report:?}");
    p.flush().unwrap();
    assert!(elsewhere.join("config.toml").is_file());
}

#[test]
fn a_new_profile_is_written_once_so_people_can_find_it() {
    let tmp = TempDir::new();
    let (mut p, _) = open(tmp.path());
    assert!(!tmp.config().exists(), "nothing is written before the first flush");
    p.flush().unwrap();
    assert!(tmp.config().is_file());
    assert!(tmp.text().starts_with("schema_version = 1"), "{}", tmp.text());
}

#[test]
fn a_broken_file_opens_with_defaults_and_is_moved_aside() {
    let tmp = TempDir::new();
    std::fs::write(tmp.config(), "this is [[[ not = toml").unwrap();
    let (p, report) = open(tmp.path());
    assert!(report.has_problems());
    assert_eq!(p.get().proofing, ProofingPrefs::default());
    let note = report.notes.join("; ");
    assert!(note.contains("not valid TOML") && note.contains("moved to") && note.contains("defaults"), "{note}");
    let aside: Vec<String> = tmp.names().into_iter().filter(|n| n.starts_with("config.broken-")).collect();
    assert_eq!(aside.len(), 1, "{:?}", tmp.names());
    assert_eq!(std::fs::read_to_string(tmp.path().join(&aside[0])).unwrap(), "this is [[[ not = toml", "the person's text is kept");
    assert!(!tmp.config().exists());
}

#[test]
fn a_file_that_is_not_text_is_moved_aside_too() {
    let tmp = TempDir::new();
    std::fs::write(tmp.config(), [0x73, 0x63, 0xff, 0xfe, 0x00, 0x80]).unwrap();
    let (p, report) = open(tmp.path());
    assert!(p.get().proofing.dictionaries.is_empty());
    let note = report.notes.join("; ");
    assert!(note.contains("not text") && note.contains("moved to"), "{note}");
    assert!(!tmp.config().exists() && tmp.names().iter().any(|n| n.starts_with("config.broken-")), "{:?}", tmp.names());
    assert!(report.read_only.is_none(), "a file that is moved aside leaves the profile writable");
}

#[test]
fn a_file_that_is_too_big_is_moved_aside_unread() {
    let tmp = TempDir::new();
    let big = format!("schema_version = 1\n# {}\n[proofing]\nignore_uppercase = false\n", "x".repeat(MAX_CONFIG_BYTES));
    std::fs::write(tmp.config(), &big).unwrap();
    let (p, report) = open(tmp.path());
    assert!(p.get().proofing.ignore_uppercase, "its values were never read");
    let note = report.notes.join("; ");
    assert!(note.contains(&format!("{} bytes", big.len())) && note.contains("more than") && note.contains("moved to"), "{note}");
    assert!(!tmp.config().exists());
    assert_eq!(
        std::fs::metadata(tmp.path().join(tmp.names().iter().find(|n| n.starts_with("config.broken-")).unwrap())).unwrap().len() as usize,
        big.len()
    );
}

#[cfg(unix)]
#[test]
fn a_file_that_is_a_device_is_never_read() {
    let tmp = TempDir::new();
    // A link to a device that gives bytes without end: read to its end it would fill the memory.
    std::os::unix::fs::symlink("/dev/zero", tmp.config()).unwrap();
    let (mut p, report) = open(tmp.path());
    let note = report.notes.join("; ");
    assert!(note.contains("not a regular file") && note.contains("in memory only"), "{note}");
    assert!(!report.persistent && p.dir().is_none());
    assert!(p.get().proofing.ignore_uppercase, "defaults");
    p.flush().unwrap();
    assert!(std::fs::symlink_metadata(tmp.config()).unwrap().file_type().is_symlink(), "the link is left as it is");
}

#[test]
fn a_newer_file_is_read_only_and_never_overwritten() {
    let tmp = TempDir::new();
    let newer = "schema_version = 99\n[proofing]\nignore_uppercase = false\n";
    std::fs::write(tmp.config(), newer).unwrap();
    let (mut p, report) = open(tmp.path());
    assert!(report.read_only.is_some() && p.read_only().is_some());
    let note = report.notes.join("; ");
    assert!(note.contains("newer DiaCraft") && note.contains("99") && note.contains("left as it is"), "{note}");
    assert!(!p.get().proofing.ignore_uppercase, "its values are shown");
    // A change lives for the session and is not saved.
    p.update(|e| e.proofing.hide_errors = true);
    assert!(p.get().proofing.hide_errors);
    let err = p.flush().unwrap_err();
    assert!(matches!(err, PrefsError::ReadOnly(_)), "{err:?}");
    assert!(err.to_string().contains("read-only"), "{err}");
    assert!(!p.tick().saved);
    assert_eq!(std::fs::read_to_string(tmp.config()).unwrap(), newer, "the file is as it was");
    // And the words are not changed either: the profile is read-only as a whole.
    let err = p.add_word(&dictionary_file("Custom.dic"), "alpha").unwrap_err();
    assert!(matches!(err, PrefsError::ReadOnly(_)), "{err:?}");
    assert!(!tmp.path().join("proofing").exists());
}

#[test]
fn a_folder_that_cannot_be_made_opens_read_only_with_a_note() {
    let tmp = TempDir::new();
    std::fs::write(tmp.path().join("a-file"), "x").unwrap();
    let (mut p, report) = open(&tmp.path().join("a-file").join("under"));
    let note = report.notes.join("; ");
    assert!(report.read_only.is_some() && note.contains("not saved"), "{note}");
    p.update(|e| e.proofing.grammar = true);
    assert!(p.get().proofing.grammar, "the session still has it");
    assert!(matches!(p.flush(), Err(PrefsError::ReadOnly(_))));
}

#[test]
fn a_file_that_cannot_be_read_does_not_stop_the_app() {
    let tmp = TempDir::new();
    std::fs::create_dir(tmp.config()).unwrap();
    let (p, report) = open(tmp.path());
    assert_eq!(*p.get(), {
        let mut d = EnginePrefs::default();
        d.language.editing = vec!["en-US".into()];
        d
    });
    assert!(report.read_only.is_some() && !report.notes.is_empty(), "{report:?}");
}

#[test]
fn a_section_that_does_not_fit_falls_back_alone() {
    let tmp = TempDir::new();
    std::fs::write(tmp.config(), "schema_version = 1\n[proofing]\nignore_uppercase = \"yes\"\n[language]\nediting = [\"de-DE\"]\n").unwrap();
    let (p, report) = open(tmp.path());
    assert!(p.get().proofing.ignore_uppercase, "the default");
    assert_eq!(p.get().language.editing, vec!["de-DE".to_owned()], "the other section is kept");
    let note = report.notes.join("; ");
    assert!(note.contains("[proofing]") && note.contains("does not fit") && note.contains("defaults"), "{note}");
}

#[test]
fn hostile_values_are_brought_into_range_and_the_person_is_told() {
    let tmp = TempDir::new();
    let mut text = String::from("schema_version = 1\n[proofing]\ndictionaries = [\n");
    for i in 0..40 {
        text.push_str(&format!("  {{ name = \"Dictionary {i}\", file = \"d{i}.dic\", language = \"all\", default = false }},\n"));
    }
    for bad in ["../evil.dic", "a/b.dic", "x.txt", "", "CON.dic", ".hidden.dic"] {
        text.push_str(&format!("  {{ name = \"Bad\", file = \"{bad}\", language = \"all\", default = true }},\n"));
    }
    text.push_str("  { name = \"Lang\", file = \"lang.dic\", language = \"not a tag!\", default = false },\n]\n");
    text.push_str("[autocorrect.exceptions]\ninitial_caps = [");
    for i in 0..1500 {
        text.push_str(&format!("\"E{i}\", "));
    }
    text.push_str(&format!(
        "\"\", \"{}\", \"E1\"]\n[language]\nediting = [\"en_us\", \"de-DE\", \"not a tag\", \"\", \"de-DE\", \"{}\"]\n",
        "x".repeat(70),
        "x".repeat(300)
    ));
    std::fs::write(tmp.config(), &text).unwrap();
    let (mut p, report) = open(tmp.path());
    let e = p.get();
    assert_eq!(e.proofing.dictionaries.len(), MAX_DICTIONARIES, "capped");
    assert!(e.proofing.dictionaries.iter().all(|d| FileName::new(FileKind::Dictionary, &d.file).is_ok()), "no file name that leaves the folder");
    assert_eq!(e.proofing.dictionaries.iter().filter(|d| d.default).count(), 1, "one default");
    assert_eq!(e.autocorrect.exceptions.initial_caps.len(), MAX_EXCEPTIONS, "capped, with no empty, long or repeated entry");
    assert!(e.autocorrect.exceptions.initial_caps.iter().all(|w| !w.is_empty() && w.chars().count() <= 64));
    assert_eq!(e.language.editing, vec!["en-US".to_owned(), "de-DE".to_owned()], "tags well formed, in canonical case, once");
    let note = report.notes.join("; ");
    for said in ["dictionaries", "exceptions", "editing languages"] {
        assert!(note.contains(said), "{said} in {note}");
    }
    // Bringing it into range is saved, and what is in range says nothing more.
    p.flush().unwrap();
    let (q, again) = open(tmp.path());
    assert_eq!(q.get(), p.get());
    assert!(again.notes.is_empty(), "{again:?}");
    let tag = |n: usize| format!("\"{}{}a-GB\"", (b'a' + (n % 26) as u8) as char, (b'a' + ((n / 26) % 26) as u8) as char);
    let many = format!("schema_version = 1\n[language]\nediting = [{}]\n", (0..100).map(tag).collect::<Vec<_>>().join(", "));
    std::fs::write(tmp.config(), many).unwrap();
    let (r, _) = open(tmp.path());
    assert!(r.get().language.editing.len() <= MAX_EDITING_LANGUAGES);
}

#[test]
fn a_word_added_by_two_profiles_in_turn_keeps_both_words() {
    let tmp = TempDir::new();
    let (mut a, _) = open(tmp.path());
    let (mut b, _) = open(tmp.path());
    let custom = dictionary_file("Custom.dic");
    a.add_word(&custom, "alpha").unwrap();
    b.add_word(&custom, "beta").unwrap();
    a.add_word(&custom, "gamma").unwrap();
    let text = String::from_utf8(a.read_file(&custom).unwrap().unwrap()).unwrap();
    assert_eq!(text, "alpha\nbeta\ngamma\n");
    assert_eq!(String::from_utf8(b.read_file(&custom).unwrap().unwrap()).unwrap(), text, "the other program sees them");
    // And a removal is one change too.
    b.remove_word(&custom, "alpha").unwrap();
    assert_eq!(String::from_utf8(a.read_file(&custom).unwrap().unwrap()).unwrap(), "beta\ngamma\n");
    assert!(tmp.path().join("proofing").join("dictionaries").join("Custom.dic").is_file());
    assert_eq!(a.list_files(FileKind::Dictionary).unwrap(), vec![custom.clone()]);
}

#[test]
fn many_programs_adding_words_at_once_lose_none() {
    let tmp = TempDir::new();
    let dir = tmp.path().to_path_buf();
    // Opened one after another: craft-settings' probe of a writable folder is named by the process, so two stores of one process opened at the same moment collide on it. Two
    // programs, which is what this stands for, have two processes.
    let profiles: Vec<_> = (0..4).map(|_| open(&dir).0).collect();
    let threads: Vec<_> = profiles
        .into_iter()
        .enumerate()
        .map(|(t, mut p)| {
            std::thread::spawn(move || {
                let custom = dictionary_file("Custom.dic");
                for n in 0..12 {
                    p.add_word(&custom, &format!("word{t}x{n}")).unwrap();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let (p, _) = open(&dir);
    let bytes = p.read_file(&dictionary_file("Custom.dic")).unwrap().unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert_eq!(text.lines().count(), 48, "{text}");
    let mut words: Vec<&str> = text.lines().collect();
    words.sort_unstable();
    words.dedup();
    assert_eq!(words.len(), 48);
}

#[test]
fn a_word_change_is_named() {
    let tmp = TempDir::new();
    let (mut p, _) = open(tmp.path());
    let custom = dictionary_file("Custom.dic");
    let added = p.add_word(&custom, "alpha").unwrap();
    assert_eq!(added, vec![Change { setting: "dictionaries/Custom.dic".into(), from: None, to: Some("alpha".into()) }]);
    assert_eq!(added[0].describe(), "dictionaries/Custom.dic: added alpha");
    assert!(p.add_word(&custom, "ALPHA").unwrap().is_empty(), "a word already there is no change");
    let removed = p.remove_word(&custom, "alpha").unwrap();
    assert_eq!(removed, vec![Change { setting: "dictionaries/Custom.dic".into(), from: Some("alpha".into()), to: None }]);
    assert_eq!(removed[0].describe(), "dictionaries/Custom.dic: removed alpha");
    assert!(p.remove_word(&custom, "alpha").unwrap().is_empty());
    let err = p.add_word(&custom, "two words").unwrap_err();
    assert!(err.to_string().contains("one word"), "{err}");
}

#[test]
fn the_editing_language_follows_the_system_locale() {
    // A system that names a language gives it, in canonical case; one that names none (`C`, `POSIX`, nothing, what is no locale) gives no language to decide on.
    for (locale, tag) in [
        (Some("de_DE.UTF-8"), Some("de-DE")),
        (Some("pt_BR"), Some("pt-BR")),
        (Some("en_GB.utf8@euro"), Some("en-GB")),
        (Some("zh-Hans-CN"), Some("zh-Hans-CN")),
        (Some("sr_RS@latin"), Some("sr-RS")),
        (Some("fr"), Some("fr")),
        (Some("C"), None),
        (Some("C.UTF-8"), None),
        (Some("POSIX"), None),
        (Some(""), None),
        (Some("not a locale"), None),
        (Some("de_DE.UTF-8:fr"), None),
        (None, None),
    ] {
        let want: Vec<String> = tag.map(str::to_owned).into_iter().collect();
        assert_eq!(LanguagePrefs::default_for(locale).editing, want, "{locale:?}");
    }
    // A new profile takes it, and a profile that has one keeps it.
    let tmp = TempDir::new();
    let options = |locale| OpenOptions::new(Location::Dir(tmp.path().to_path_buf())).locale(fixed(locale));
    let (mut p, _) = Profile::<EnginePrefs>::open_with(options("fr_FR.UTF-8"));
    assert_eq!(p.get().language.editing, vec!["fr-FR".to_owned()]);
    p.flush().unwrap();
    let (q, _) = Profile::<EnginePrefs>::open_with(options("de_DE.UTF-8"));
    assert_eq!(q.get().language.editing, vec!["fr-FR".to_owned()], "the person's language is the person's, not the system's of the day");
    // A system that says none leaves the profile undecided, and the programs read en-US meanwhile.
    let none = OpenOptions::new(Location::Memory).locale(LocaleSource::Fixed(None));
    let (r, _) = Profile::<EnginePrefs>::open_with(none);
    assert!(r.get().language.editing.is_empty(), "no language is stored when the system gives none");
    assert_eq!(r.get().language.preferred(), "en-US", "and en-US is what is read");
}

/// A first run under `LANG=C` (or `POSIX`, or no language variable at all) must not store `en-US` for good: it stores none, and a later run with a real locale decides.
#[test]
fn a_first_run_with_no_real_language_stores_none_and_a_later_run_decides() {
    for system in [LocaleSource::Fixed(None), fixed("C"), fixed("POSIX"), fixed("C.UTF-8"), fixed("")] {
        let tmp = TempDir::new();
        let options = |locale: LocaleSource| OpenOptions::new(Location::Dir(tmp.path().to_path_buf())).locale(locale).debounce(Duration::ZERO);
        let (mut first, report) = Profile::<EnginePrefs>::open_with(options(system.clone()));
        assert!(!report.has_problems(), "{system:?}: {report:?}");
        assert!(first.get().language.editing.is_empty(), "{system:?}: nothing is decided");
        assert_eq!(first.get().language.preferred(), "en-US", "{system:?}: and the programs read en-US meanwhile");
        first.flush().unwrap();
        let text = tmp.text();
        assert!(!text.contains("en-US"), "{system:?}: en-US is not stored for good:\n{text}");
        drop(first);
        // A later run with a real locale decides, and saves what it decided.
        let (mut later, _) = Profile::<EnginePrefs>::open_with(options(fixed("de_DE.UTF-8")));
        assert_eq!(later.get().language.editing, vec!["de-DE".to_owned()], "{system:?}: the later run decides");
        later.flush().unwrap();
        assert!(tmp.text().contains("de-DE"), "{system:?}: and stores it:\n{}", tmp.text());
        drop(later);
        // Once decided, a run with no language does not take the choice back.
        let (again, _) = Profile::<EnginePrefs>::open_with(options(system.clone()));
        assert_eq!(again.get().language.editing, vec!["de-DE".to_owned()], "{system:?}: the person's language stays");
    }
}

#[test]
fn programs_change_settings_as_the_window_does_and_each_change_is_named() {
    let tmp = TempDir::new();
    let (mut p, _) = open(tmp.path());
    let changes = p.update(|e| e.proofing.ignore_uppercase = false);
    assert_eq!(changes, vec![Change { setting: "proofing.ignore_uppercase".into(), from: Some("true".into()), to: Some("false".into()) }]);
    assert_eq!(changes[0].describe(), "proofing.ignore_uppercase: true -> false");
    assert!(p.update(|e| e.proofing.ignore_uppercase = false).is_empty(), "a write of what is there changes nothing");
    let two = p.update(|e| {
        e.autocorrect.smart_quotes = false;
        e.language.editing = vec!["de-DE".into(), "en-US".into()];
        e.autocorrect.exceptions.initial_caps.push("IDs".into());
    });
    let named: Vec<String> = two.iter().map(Change::describe).collect();
    assert_eq!(
        named,
        [
            "autocorrect.smart_quotes: true -> false",
            "autocorrect.exceptions.initial_caps: [] -> [\"IDs\"]",
            "language.editing: [\"en-US\"] -> [\"de-DE\", \"en-US\"]"
        ]
    );
    // A change of a list is one change, and a new section or key is named by its path.
    let json = diacraft_prefs::changes_json(&two);
    assert_eq!(json[0]["setting"], "autocorrect.smart_quotes");
    assert_eq!(json[0]["from"], "true");
    assert_eq!(json[0]["to"], "false");
    // It is saved, and the other program sees it at its next start.
    p.flush().unwrap();
    let (q, _) = open(tmp.path());
    assert_eq!(q.get(), p.get());
}

#[test]
fn a_change_a_session_recorded_is_saved_as_it_was_made() {
    let tmp = TempDir::new();
    let (mut p, _) = open(tmp.path());
    let mut now = p.get().clone();
    now.proofing.grammar = true;
    now.proofing.dictionaries.push(DictionaryEntry { name: "Custom".into(), file: "Custom.dic".into(), language: "all".into(), default: true });
    let named = p.save_change(&ProfileChange::Sections(Box::new(now.clone()))).unwrap();
    assert_eq!(named.iter().map(|c| c.setting.as_str()).collect::<Vec<_>>(), ["proofing.grammar", "proofing.dictionaries"]);
    let custom = dictionary_file("Custom.dic");
    assert_eq!(p.save_change(&ProfileChange::AddWord { file: custom.clone(), word: "alpha".into() }).unwrap().len(), 1);
    assert!(p.save_change(&ProfileChange::AddWord { file: custom.clone(), word: "alpha".into() }).unwrap().is_empty());
    let list = FileName::new(FileKind::AutoCorrect, "en.tsv").unwrap();
    let written = ProfileChange::WriteFile { file: list.clone(), bytes: b"teh\tthe\n".to_vec() };
    assert_eq!(p.save_change(&written).unwrap()[0].setting, "autocorrect/en.tsv");
    p.flush().unwrap();
    let (q, _) = open(tmp.path());
    assert_eq!(*q.get(), now);
    assert_eq!(q.read_file(&list).unwrap().unwrap(), b"teh\tthe\n");
    assert_eq!(p.save_change(&ProfileChange::RemoveFile { file: list.clone() }).unwrap().len(), 1);
    assert!(p.save_change(&ProfileChange::RemoveFile { file: list.clone() }).unwrap().is_empty());
    assert_eq!(q.list_files(FileKind::AutoCorrect).unwrap().len(), 0);
}

#[test]
fn a_save_waits_for_the_quiet_and_flush_does_not() {
    let tmp = TempDir::new();
    let waiting = OpenOptions::new(Location::Dir(tmp.path().to_path_buf())).locale(fixed("en"));
    let (mut p, _) = Profile::<EnginePrefs>::open_with(waiting);
    p.flush().unwrap();
    let idle = p.tick();
    assert!(!idle.saved && idle.next_in.is_none() && idle.failed.is_none(), "nothing to save: {idle:?}");
    p.update(|e| e.proofing.grammar = true);
    let tick = p.tick();
    assert!(!tick.saved && tick.failed.is_none());
    assert!(tick.next_in.is_some_and(|d| d <= Duration::from_millis(500)), "{tick:?}: the window is asked for a frame at the deadline");
    assert!(!tmp.text().contains("grammar = true"), "not saved yet");
    p.flush().unwrap();
    assert!(tmp.text().contains("grammar = true"));
    let (mut q, _) = open(tmp.path());
    q.update(|e| e.proofing.hide_errors = true);
    assert!(q.tick().saved, "with no quiet period the next tick saves");
    assert!(tmp.text().contains("hide_errors = true"));
}

#[test]
fn what_changed_is_read_off_the_keys_whatever_they_are() {
    let before = Window::default();
    let mut after = before.clone();
    after.interface.language = "de".into();
    after.engine.proofing.grammar = true;
    let named: Vec<String> = diff(&before, &after).iter().map(Change::describe).collect();
    assert_eq!(
        named,
        ["proofing.grammar: false -> true", "interface.language: \"\" -> \"de\""],
        "a key someone adds later is named without a change to the store"
    );
    assert!(diff(&after, &after).is_empty());
    // Past the cap the names stop, and a long value is cut.
    let mut many = before.clone();
    many.engine.language.editing = (0..MAX_EDITING_LANGUAGES).map(|i| format!("x{i}")).collect();
    many.engine.autocorrect.exceptions.initial_caps = vec!["y".repeat(64); MAX_EXCEPTIONS];
    let changes = diff(&before, &many);
    assert!(changes.iter().all(|c| c.to.as_ref().is_none_or(|t| t.chars().count() <= 200)), "{changes:?}");
}
