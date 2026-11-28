//! The input and the properties of the fuzz target `prefs_load`: bytes become the person's settings, and the store is held to its contract on them. It is a library
//! so that the crate's own tests include this file (`crates/prefs/tests/it/prefs_fuzz.rs`) and hold crafted and random inputs to the same law the fuzzer holds the bytes to.
//!
//! **The input.** The first byte, modulo 5, says what the rest is: 0, the text of `config.toml` (by lossy UTF-8); 1, a word list (the whole of it, then half of it with the
//! other half as a word to add and remove); 2, a language tag and a locale; 3, a file name of each kind; 4, a recipe of edits made to a profile, eight bytes each, the first
//! saying which (an option turned, an editing language, an exception, a dictionary, a word added, a word removed, a file written, or a save and a read back), the rest the text it
//! uses, at most 64 of them.
//!
//! **The properties.** Nothing panics or hangs. A profile opened on any text has every list in range (at most 32 dictionaries, each a plain file name, once, one of them the default;
//! at most 1,000 exceptions of at most 64 characters with no space; one to 32 editing languages in canonical case), and **what is in range stays so**: bringing it into range again
//! says nothing and changes nothing. What is saved (at most 1 MiB) is read back as the profile that saved it, with nothing to say. A word list that reads writes back as a list that
//! reads to the same words; a word that is added is in the list, one that is refused changes nothing, one that is removed is gone. A tag that is accepted is accepted as itself,
//! within 255 bytes; a file name that is accepted holds no separator or control character, is accepted as itself and is found again by its key. Every note is one line of
//! text with no control character.

use std::collections::HashSet;
use std::sync::Arc;

use diacraft_prefs::{
    DictionaryEntry, EnginePrefs, FileKind, FileName, LocaleSource, MAX_CONFIG_BYTES, MAX_DICT_FILE_BYTES, MAX_DICTIONARIES,
    MAX_DICTIONARY_NAME_CHARS, MAX_EDITING_LANGUAGES, MAX_EXCEPTION_CHARS, MAX_EXCEPTIONS, MAX_TAG_BYTES, MAX_WORDS, MemoryHost, Profile,
    canonical_tag, tag_of_locale, words,
};

/// The most edits a recipe makes.
const MAX_EDITS: usize = 64;

/// The property of the whole input: reads `data` as the module says and holds the store to its contract.
pub fn check(data: &[u8]) {
    let Some((mode, rest)) = data.split_first() else { return };
    match mode % 5 {
        0 => config_text(&String::from_utf8_lossy(rest)),
        1 => word_list(rest),
        2 => {
            let text = String::from_utf8_lossy(rest);
            tag(canonical_tag(&text));
            tag(tag_of_locale(&text));
        }
        3 => names(&String::from_utf8_lossy(rest)),
        _ => edits(rest),
    }
}

fn open(host: &Arc<MemoryHost>) -> (Profile<EnginePrefs>, diacraft_prefs::Report) {
    Profile::open_host(host.clone(), LocaleSource::Fixed(Some("fr_FR.UTF-8".to_owned())))
}

/// A note is a line of text a person can read: no control character, no newline.
fn a_line(note: &str) {
    assert!(!note.chars().any(char::is_control), "{note:?}");
}

/// Every list of `prefs` is in range (the module's second property).
pub fn in_range(prefs: &EnginePrefs) {
    let editing = &prefs.language.editing;
    assert!(!editing.is_empty() && editing.len() <= MAX_EDITING_LANGUAGES, "{editing:?}");
    for tag in editing {
        assert_eq!(canonical_tag(tag).as_deref(), Some(tag.as_str()), "{tag:?}");
    }
    let mut distinct: HashSet<&String> = HashSet::new();
    assert!(editing.iter().all(|tag| distinct.insert(tag)), "{editing:?}");
    let dictionaries = &prefs.proofing.dictionaries;
    assert!(dictionaries.len() <= MAX_DICTIONARIES);
    assert_eq!(dictionaries.iter().filter(|d| d.default).count(), usize::from(!dictionaries.is_empty()), "one default");
    let mut files = HashSet::new();
    for d in dictionaries {
        assert!(FileName::new(FileKind::Dictionary, &d.file).is_ok(), "{:?}", d.file);
        assert!(files.insert(d.file.to_lowercase()), "{:?} twice", d.file);
        assert!(!d.name.is_empty() && d.name.chars().count() <= MAX_DICTIONARY_NAME_CHARS && !d.name.chars().any(char::is_control), "{:?}", d.name);
        assert!(d.language == "all" || canonical_tag(&d.language).as_deref() == Some(d.language.as_str()), "{:?}", d.language);
    }
    for list in [&prefs.autocorrect.exceptions.initial_caps, &prefs.autocorrect.exceptions.first_letter] {
        assert!(list.len() <= MAX_EXCEPTIONS);
        let mut seen = HashSet::new();
        for entry in list {
            assert!(!entry.is_empty() && entry.chars().count() <= MAX_EXCEPTION_CHARS, "{entry:?}");
            assert!(!entry.chars().any(|c| c.is_control() || c.is_whitespace()), "{entry:?}");
            assert!(seen.insert(entry), "{entry:?} twice");
        }
    }
}

/// A profile opened on `text` as the text of `config.toml`.
fn config_text(text: &str) {
    let host = Arc::new(MemoryHost::default());
    host.set_config(text);
    let (mut profile, report) = open(&host);
    report.notes.iter().for_each(|note| a_line(note));
    in_range(profile.get());
    let mut again = profile.get().clone();
    assert!(again.bring_into_range().is_empty(), "what is in range stays so");
    assert_eq!(&again, profile.get());
    // What is saved is read back as it was, with nothing left to say; where nothing was written (nothing was changed, and opening had nothing to bring into range that a change
    // would write) it says what it said.
    let before = host.config_text();
    let opened = report.notes.clone();
    if profile.flush().is_ok() {
        let saved = host.config_text().unwrap_or_default();
        assert!(saved.len() <= MAX_CONFIG_BYTES);
        let (back, again) = open(&host);
        assert_eq!(back.get(), profile.get());
        if host.config_text() != before {
            assert!(again.notes.is_empty(), "{:?}", again.notes);
        } else {
            assert_eq!(again.notes, opened, "nothing was written, so it says what it said");
        }
    }
}

/// A word list read from `bytes`, and a word added to half of it.
fn word_list(bytes: &[u8]) {
    if let Ok(list) = words::read(bytes) {
        assert!(list.words().count() <= MAX_WORDS);
        assert!(list.words().all(|w| words::check_word(w).is_ok()));
        let rendered = list.render();
        // A list read from UTF-16 can be longer as UTF-8 than the cap; one that is within it reads again to the same words.
        if rendered.len() <= MAX_DICT_FILE_BYTES {
            let again = words::read(&rendered).expect("what was rendered reads");
            assert_eq!(again.words().collect::<Vec<_>>(), list.words().collect::<Vec<_>>());
            assert_eq!(again.render(), rendered);
        }
    }
    let (first, second) = bytes.split_at(bytes.len() / 2);
    let Ok(mut list) = words::read(first) else { return };
    let word = String::from_utf8_lossy(second).into_owned();
    let before = list.render();
    match list.add(&word) {
        Ok(_) => assert!(list.contains(&word), "{word:?}"),
        Err(_) => assert_eq!(list.render(), before, "a word that is refused changes nothing"),
    }
    assert!(list.render().len() <= MAX_DICT_FILE_BYTES.max(before.len()));
    list.remove(&word);
    assert!(!list.contains(&word), "{word:?}");
}

/// A tag the store accepted is accepted as itself.
fn tag(tag: Option<String>) {
    if let Some(tag) = tag {
        assert!(tag.len() <= MAX_TAG_BYTES, "{tag:?}");
        assert_eq!(canonical_tag(&tag).as_deref(), Some(tag.as_str()));
    }
}

/// A file name of each kind.
fn names(text: &str) {
    for kind in FileKind::ALL {
        let Ok(name) = FileName::new(kind, text) else { continue };
        let file = name.file();
        assert!(file.len() <= 255 && !file.contains(['/', '\\', ':']) && !file.chars().any(char::is_control), "{file:?}");
        assert_eq!(FileName::new(kind, &file).ok().as_ref(), Some(&name));
        assert_eq!(FileName::from_key(&name.key()).as_ref(), Some(&name));
    }
}

/// A recipe of edits to a profile in a host: each ends with every list in range, and a save is read back whole.
fn edits(bytes: &[u8]) {
    let host = Arc::new(MemoryHost::default());
    let (mut profile, _) = open(&host);
    for piece in bytes.chunks(8).take(MAX_EDITS) {
        let Some((kind, text)) = piece.split_first() else { continue };
        let text = String::from_utf8_lossy(text).into_owned();
        let custom = FileName::new(FileKind::Dictionary, "Custom.dic");
        match kind % 8 {
            0 => {
                profile.update(|e| e.proofing.ignore_uppercase = !e.proofing.ignore_uppercase);
            }
            1 => {
                profile.update(|e| e.language.editing.push(text.clone()));
            }
            2 => {
                profile.update(|e| e.autocorrect.exceptions.initial_caps.push(text.clone()));
            }
            3 => {
                let entry = DictionaryEntry { name: text.clone(), file: format!("{text}.dic"), language: text.clone(), default: true };
                profile.update(|e| e.proofing.dictionaries.push(entry));
            }
            4 => {
                if let Ok(file) = &custom {
                    let _ = profile.add_word(file, &text);
                }
            }
            5 => {
                if let Ok(file) = &custom {
                    let _ = profile.remove_word(file, &text);
                }
            }
            6 => {
                if let Ok(file) = FileName::new(FileKind::AutoCorrect, &format!("{text}.tsv")) {
                    let _ = profile.write_file(&file, text.as_bytes());
                }
            }
            _ => {
                if profile.flush().is_ok() {
                    let (back, report) = open(&host);
                    assert!(report.notes.is_empty(), "{:?}", report.notes);
                    assert_eq!(back.get(), profile.get());
                    profile = back;
                }
            }
        }
        in_range(profile.get());
        profile.take_notes().iter().for_each(|note| a_line(note));
    }
}
