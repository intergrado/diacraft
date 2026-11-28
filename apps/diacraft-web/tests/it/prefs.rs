//! The person's settings in the browser: `localStorage` under the window's settings, held to 1 MiB, with a stand-in for the storage so that nothing here needs a browser.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use diacraft_browser::page::{new_app, with_prefs_in_browser};
use diacraft_browser::prefs::{BROKEN_KEY, CONFIG_KEY, FILE_PREFIX, KEY_PREFIX, Storage, StorageHost, WEB_PREFS_BYTES, file_key};
use diacraft_ui_egui::prefs_host::{FileKind, FileName, LocaleSource, PrefsError, Profile, WindowPrefs};

/// The storage of a test: strings by name, which can be made to refuse a write.
#[derive(Clone, Default)]
struct Map {
    keys: Arc<Mutex<BTreeMap<String, String>>>,
    refuse: Arc<Mutex<Option<String>>>,
}

impl Map {
    fn put(&self, key: &str, value: &str) {
        self.keys.lock().unwrap().insert(key.to_owned(), value.to_owned());
    }

    fn value(&self, key: &str) -> Option<String> {
        self.keys.lock().unwrap().get(key).cloned()
    }

    fn names(&self) -> Vec<String> {
        self.keys.lock().unwrap().keys().cloned().collect()
    }

    fn refuse_writes(&self, why: &str) {
        *self.refuse.lock().unwrap() = Some(why.to_owned());
    }
}

impl Storage for Map {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self.value(key))
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        if let Some(why) = self.refuse.lock().unwrap().clone() {
            return Err(why);
        }
        self.put(key, value);
        Ok(())
    }

    fn remove(&self, key: &str) -> Result<(), String> {
        self.keys.lock().unwrap().remove(key);
        Ok(())
    }

    fn keys(&self) -> Result<Vec<String>, String> {
        Ok(self.names())
    }
}

fn open(map: &Map) -> (Profile<WindowPrefs>, diacraft_ui_egui::prefs_host::Report) {
    Profile::open_host(Arc::new(StorageHost::new(map.clone())), LocaleSource::Fixed(Some("de_DE.UTF-8".to_owned())))
}

fn dictionary(name: &str) -> FileName {
    FileName::new(FileKind::Dictionary, name).unwrap()
}

#[test]
fn the_keys_and_the_budget_are_the_documented_ones() {
    assert_eq!(WEB_PREFS_BYTES, 1 << 20, "1 MiB for all of DiaCraft's keys");
    assert_eq!((KEY_PREFIX, CONFIG_KEY, BROKEN_KEY, FILE_PREFIX), ("diacraft.", "diacraft.config", "diacraft.config.broken", "diacraft.proofing."));
    assert_eq!(file_key(&dictionary("Custom.dic")), "diacraft.proofing.dictionaries/Custom.dic");
    assert_eq!(file_key(&FileName::new(FileKind::AutoCorrect, "en.tsv").unwrap()), "diacraft.proofing.autocorrect/en.tsv");
}

#[test]
fn the_settings_are_one_key_of_toml_text_and_come_back() {
    let map = Map::default();
    let (mut profile, report) = open(&map);
    assert!(report.persistent && !report.has_problems(), "{report:?}");
    assert_eq!(profile.get().engine.language.editing, vec!["de-DE".to_owned()], "the browser's language is the first editing language");
    assert_eq!(map.names(), Vec::<String>::new(), "opening stores nothing in the browser: a person who has set nothing leaves nothing");
    assert!(!profile.tick().saved && map.names().is_empty());
    let changes = profile.update(|w| w.engine.proofing.grammar = true);
    assert_eq!(changes.len(), 1);
    profile.flush().unwrap();
    let text = map.value(CONFIG_KEY).unwrap();
    assert!(text.starts_with("schema_version = 1") && text.contains("grammar = true") && text.contains("[interface]"), "{text}");
    let (again, report) = open(&map);
    assert!(!report.has_problems());
    assert_eq!(again.get(), profile.get());
}

#[test]
fn a_word_list_is_a_key_of_its_own() {
    let map = Map::default();
    let (mut profile, _) = open(&map);
    let custom = dictionary("Custom.dic");
    profile.add_word(&custom, "alpha").unwrap();
    profile.add_word(&custom, "beta").unwrap();
    assert_eq!(map.value("diacraft.proofing.dictionaries/Custom.dic").as_deref(), Some("alpha\nbeta\n"));
    assert_eq!(profile.list_files(FileKind::Dictionary).unwrap(), vec![custom.clone()]);
    assert!(profile.list_files(FileKind::AutoCorrect).unwrap().is_empty());
    profile.remove_word(&custom, "alpha").unwrap();
    assert_eq!(map.value("diacraft.proofing.dictionaries/Custom.dic").as_deref(), Some("beta\n"));
    assert_eq!(profile.remove_file(&custom).unwrap().len(), 1);
    assert_eq!(map.value("diacraft.proofing.dictionaries/Custom.dic"), None);
}

#[test]
fn the_host_refuses_past_one_mebibyte_in_words_and_changes_nothing() {
    let map = Map::default();
    let (mut profile, _) = open(&map);
    let first = dictionary("First.dic");
    let second = dictionary("Second.dic");
    // A word list of the largest size fits once; the second does not.
    let big = format!("{}\n", "w".repeat(60)).repeat(16_000);
    assert!(big.len() < 1_000_000 && big.len() > 900_000);
    profile.write_file(&first, big.as_bytes()).unwrap();
    let err = profile.write_file(&second, big.as_bytes()).unwrap_err();
    assert!(matches!(err, PrefsError::Refused(_)), "{err:?}");
    let words = err.to_string();
    assert!(words.contains("more than 1 MiB") && words.contains("nothing was changed"), "{words}");
    assert_eq!(map.value(&file_key(&second)), None, "the refused file was not made");
    assert_eq!(map.value(&file_key(&first)).as_deref(), Some(big.as_str()), "the first is as it was");
    // A word added to the first is a write that would pass the budget too.
    let err = profile.write_file(&first, format!("{big}{big}").as_bytes()).unwrap_err();
    assert!(matches!(err, PrefsError::Refused(_)) && err.to_string().contains("more than"), "{err}");
    // The settings text itself is held to the same budget, and keeps working next to the files.
    let changes = profile.update(|w| w.engine.proofing.grammar = true);
    assert_eq!(changes.len(), 1);
    profile.flush().unwrap();
}

#[test]
fn other_keys_of_the_origin_are_not_counted_or_read() {
    let map = Map::default();
    map.put("other-app.settings", &"x".repeat(3 << 20));
    map.put("diacraft", "not a key of ours");
    map.put("diacraft.proofing.dictionaries/../evil.dic", "x");
    map.put("diacraft.proofing.elsewhere/Custom.dic", "x");
    let (mut profile, report) = open(&map);
    assert!(!report.has_problems(), "{report:?}");
    assert!(profile.list_files(FileKind::Dictionary).unwrap().is_empty(), "no key that is not a file of ours is listed");
    profile.flush().unwrap();
    assert_eq!(map.value("other-app.settings").map(|v| v.len()), Some(3 << 20), "another app's key is left as it is");
}

#[test]
fn a_file_that_is_not_text_is_refused_by_the_browser() {
    let map = Map::default();
    let (mut profile, _) = open(&map);
    let list = FileName::new(FileKind::AutoCorrect, "de.tsv").unwrap();
    let err = profile.write_file(&list, &[0xFF, 0xFE, 0x41, 0x00]).unwrap_err();
    assert!(err.to_string().contains("text files only"), "{err}");
    assert_eq!(map.value(&file_key(&list)), None);
}

#[test]
fn a_text_that_is_broken_is_kept_under_its_own_key() {
    let map = Map::default();
    map.put(CONFIG_KEY, "this is [[[ not toml");
    let (mut profile, report) = open(&map);
    let note = report.notes.join("; ");
    assert!(note.contains("not valid TOML") && note.contains(BROKEN_KEY) && note.contains("defaults"), "{note}");
    assert_eq!(map.value(BROKEN_KEY).as_deref(), Some("this is [[[ not toml"));
    profile.flush().unwrap();
    assert!(map.value(CONFIG_KEY).unwrap().starts_with("schema_version = 1"));
}

#[test]
fn a_storage_that_will_not_take_a_write_is_told_once_in_words() {
    let map = Map::default();
    let (mut profile, _) = open(&map);
    profile.flush().unwrap();
    map.refuse_writes("the browser's storage is full");
    profile.update(|w| w.engine.proofing.hide_errors = true);
    let tick = profile.tick();
    assert!(tick.failed.as_deref().is_some_and(|f| f.contains("storage is full")), "{tick:?}");
    assert!(profile.tick().failed.is_none(), "once");
    let err = profile.flush().unwrap_err();
    assert!(matches!(err, PrefsError::Io(_)) && err.to_string().contains("storage is full"), "{err:?}");
}

#[test]
fn the_page_gives_the_window_its_settings() {
    let map = Map::default();
    let app = with_prefs_in_browser(new_app(), Arc::new(StorageHost::new(map.clone())));
    let prefs = app.prefs().expect("the window has its settings");
    assert!(prefs.is_persistent() && prefs.dir().is_none());
    // A text that does not parse is said in the status bar, and the window starts all the same.
    map.put(CONFIG_KEY, "[[[");
    let app = with_prefs_in_browser(new_app(), Arc::new(StorageHost::new(map.clone())));
    assert!(app.prefs().is_some());
    assert!(app.notice().is_some_and(|n| n.contains("not valid TOML")), "{:?}", app.notice());
}
