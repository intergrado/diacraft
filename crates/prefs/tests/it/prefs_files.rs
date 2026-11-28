//! The files beside `config.toml`: the names a word list or an AutoCorrect list may have, the text format of a word list, and a profile that lives in a host that has no
//! file system (the browser's `localStorage`, here a memory host).

use std::sync::Arc;

use diacraft_prefs::{
    Edit, EnginePrefs, FileHost, FileKind, FileName, LocaleSource, MAX_CONFIG_BYTES, MAX_DICT_FILE_BYTES, MAX_WORD_CHARS, MAX_WORDS, MemoryHost,
    PrefsError, PrefsHost, Profile, WordError, WordFile, words,
};

fn fixed(locale: &str) -> LocaleSource {
    LocaleSource::Fixed(Some(locale.to_owned()))
}

fn dictionary_file(name: &str) -> FileName {
    FileName::new(FileKind::Dictionary, name).unwrap()
}

fn host_profile(host: &Arc<MemoryHost>) -> (Profile<EnginePrefs>, diacraft_prefs::Report) {
    Profile::open_host(host.clone(), fixed("fr_FR.UTF-8"))
}

#[test]
fn a_file_name_is_a_plain_name_in_its_folder() {
    for good in ["Custom.dic", "Mi diccionario.dic", "日本語.dic", "a.dic", "CUSTOM.DIC", "Name (2).dic", "x.y.dic"] {
        let name = FileName::new(FileKind::Dictionary, good).unwrap_or_else(|e| panic!("{good}: {e}"));
        assert_eq!(name.kind(), FileKind::Dictionary);
        assert!(name.file().ends_with(".dic"), "{}", name.file());
    }
    assert_eq!(FileName::new(FileKind::Dictionary, "Custom.DIC").unwrap().file(), "Custom.dic", "the extension is kept in lower case");
    assert_eq!(FileName::new(FileKind::Dictionary, "Custom.dic").unwrap().key(), "dictionaries/Custom.dic");
    assert_eq!(FileName::new(FileKind::AutoCorrect, "en.tsv").unwrap().key(), "autocorrect/en.tsv");
    let too_long = format!("{}.dic", "n".repeat(MAX_WORD_CHARS + 1));
    let fit = format!("{}.dic", "n".repeat(MAX_WORD_CHARS));
    for bad in [
        "",
        ".dic",
        "../x.dic",
        "a/b.dic",
        "a\\b.dic",
        "x.txt",
        "x",
        "x.dic.txt",
        "CON.dic",
        "nul.dic",
        "COM1.dic",
        "lpt9.dic",
        ".hidden.dic",
        " lead.dic",
        "trail .dic",
        "dot..dic",
        "a:b.dic",
        "a*b.dic",
        "a?b.dic",
        "a\"b.dic",
        "a<b.dic",
        "a>b.dic",
        "a|b.dic",
        "tab\there.dic",
        "nul\0.dic",
        "bell\u{7}.dic",
        "line\nbreak.dic",
        &too_long,
    ] {
        let err = FileName::new(FileKind::Dictionary, bad).unwrap_err();
        assert!(matches!(err, PrefsError::Refused(_)), "{bad:?}: {err:?}");
    }
    FileName::new(FileKind::Dictionary, &fit).unwrap();
    assert!(FileName::new(FileKind::AutoCorrect, "Custom.dic").is_err(), "the extension is the kind's");
    assert_eq!(FileName::from_key("dictionaries/Custom.dic"), Some(dictionary_file("Custom.dic")));
    assert_eq!(FileName::from_key("autocorrect/de.tsv"), FileName::new(FileKind::AutoCorrect, "de.tsv").ok());
    for bad in ["Custom.dic", "dictionaries/../x.dic", "other/x.dic", "dictionaries/a/b.dic", "dictionaries/x.txt", ""] {
        assert_eq!(FileName::from_key(bad), None, "{bad:?}");
    }
}

#[test]
fn a_word_list_is_text_with_a_word_to_a_line() {
    let list = words::read(b"alpha\nBeta\n\n# a comment\n  gamma  \n").unwrap();
    assert_eq!(list.words().collect::<Vec<_>>(), ["alpha", "Beta", "gamma"]);
    assert_eq!(list.skipped(), 0);
    assert_eq!(String::from_utf8(list.render()).unwrap(), "alpha\nBeta\n\n# a comment\ngamma\n", "comments and blank lines stay, words are trimmed");
    // CRLF, a byte order mark, and no last newline.
    assert_eq!(words::read(b"one\r\ntwo\r\nthree").unwrap().words().collect::<Vec<_>>(), ["one", "two", "three"]);
    assert_eq!(words::read(b"\xEF\xBB\xBFone\ntwo\n").unwrap().words().collect::<Vec<_>>(), ["one", "two"]);
    // UTF-16 with a byte order mark is read, and written back as UTF-8.
    let utf16 = |le: bool| -> Vec<u8> {
        let mut out = if le { vec![0xFF, 0xFE] } else { vec![0xFE, 0xFF] };
        for unit in "Grüße\n# c\nÅngström\n".encode_utf16() {
            out.extend(if le { unit.to_le_bytes() } else { unit.to_be_bytes() });
        }
        out
    };
    for le in [true, false] {
        let list = words::read(&utf16(le)).unwrap();
        assert_eq!(list.words().collect::<Vec<_>>(), ["Grüße", "Ångström"], "le={le}");
        assert_eq!(String::from_utf8(list.render()).unwrap(), "Grüße\n# c\nÅngström\n");
    }
    // Lines that are not a word are skipped and counted, and kept as they were.
    let odd = words::read(b"fine\ntwo words\nalso fine\n\x01\n").unwrap();
    assert_eq!(odd.words().collect::<Vec<_>>(), ["fine"]);
    assert_eq!(odd.skipped(), 3);
    assert_eq!(String::from_utf8(odd.render()).unwrap(), "fine\ntwo words\nalso fine\n\u{1}\n");
    // An empty file is a list with no word.
    assert_eq!(words::read(b"").unwrap().words().count(), 0);
}

#[test]
fn a_word_list_holds_no_invisible_mark_and_keeps_its_words() {
    // The crash: a first line of a vertical tab (taken off by the trim) and U+FEFF was one word. Rendered first, U+FEFF is the file's byte order mark when it
    // is read again, so the word was lost.
    let only = words::read(b"\x0b\xef\xbb\xbf").unwrap();
    assert_eq!(only.words().collect::<Vec<_>>(), Vec::<&str>::new(), "U+FEFF is not a word");
    assert_eq!(only.skipped(), 1);
    assert_eq!(words::read(&only.render()).unwrap().words().collect::<Vec<_>>(), only.words().collect::<Vec<_>>());
    // The first of several words.
    let first = words::read(b"\x0b\xef\xbb\xbf\nword\n").unwrap();
    assert_eq!(first.words().collect::<Vec<_>>(), ["word"]);
    assert_eq!(words::read(&first.render()).unwrap().words().collect::<Vec<_>>(), first.words().collect::<Vec<_>>());
}

#[test]
fn a_word_list_of_noncharacters_reads_back_to_its_words() {
    // The crash: a UTF-16 byte order mark, then U+FEFF and U+FFFF. The first line is not a word, and rendered again the
    // U+FEFF is the file's byte order mark, so U+FFFF came back as a word the first read never held.
    let bytes = [0xFF, 0xFE, 0xFF, 0xFE, 0xFF, 0xFF];
    let list = words::read(&bytes).unwrap();
    let rendered = list.render();
    let again = words::read(&rendered).unwrap();
    assert_eq!(again.words().collect::<Vec<_>>(), list.words().collect::<Vec<_>>());
    assert_eq!(again.render(), rendered);
    // A noncharacter is never a word: U+FFFE, U+FFFF, U+FDD0 to U+FDEF, and either end of every plane.
    for word in ["\u{fffe}", "\u{ffff}", "\u{fdd0}", "\u{fdef}", "\u{1fffe}", "\u{10ffff}"] {
        assert!(matches!(words::check_word(word), Err(WordError::NotAWord(_))), "{word:?}");
        let list = words::read(format!("{word}\n").as_bytes()).unwrap();
        assert_eq!(list.words().count(), 0, "{word:?}");
        assert_eq!(words::read(&list.render()).unwrap().render(), list.render(), "{word:?}");
    }
}

#[test]
fn a_word_list_that_is_not_text_or_too_big_is_refused_in_words() {
    let err = words::read(b"caf\xe9\n").unwrap_err();
    assert_eq!(err, WordError::NotText);
    assert!(err.to_string().contains("not UTF-8 or UTF-16 text"), "{err}");
    assert_eq!(words::read(&[0xFF, 0xFE, 0x41]).unwrap_err(), WordError::NotText, "an odd byte in UTF-16");
    assert_eq!(words::read(&[0xFF, 0xFE, 0x00, 0xD8]).unwrap_err(), WordError::NotText, "a lone surrogate");
    let big = vec![b'a'; MAX_DICT_FILE_BYTES + 1];
    let err = words::read(&big).unwrap_err();
    assert!(matches!(err, WordError::TooBig(_)) && err.to_string().contains("more than 1,000,000 bytes"), "{err}");
    let mut many = String::new();
    for i in 0..=MAX_WORDS {
        many.push_str(&format!("w{i}\n"));
    }
    assert!(many.len() < MAX_DICT_FILE_BYTES);
    let err = words::read(many.as_bytes()).unwrap_err();
    assert!(matches!(err, WordError::TooMany) && err.to_string().contains("more than 100,000 words"), "{err}");
    let mut fits = String::new();
    for i in 0..MAX_WORDS {
        fits.push_str(&format!("w{i}\n"));
    }
    assert_eq!(words::read(fits.as_bytes()).unwrap().words().count(), MAX_WORDS);
    // One line of a megabyte is one line that is not a word.
    let line = vec![b'z'; MAX_DICT_FILE_BYTES];
    let one = words::read(&line).unwrap();
    assert_eq!((one.words().count(), one.skipped()), (0, 1));
}

#[test]
fn a_word_is_added_once_and_removed_by_any_case() {
    let mut list = WordFile::default();
    assert_eq!(list.add("Alpha"), Ok(true));
    assert_eq!(list.add("  beta  "), Ok(true), "trimmed");
    assert_eq!(list.add("ALPHA"), Ok(false), "already there, in any case");
    assert!(list.contains("alpha") && list.contains("Beta") && !list.contains("gamma"));
    assert!(list.remove("BETA"));
    assert!(!list.remove("beta"));
    assert_eq!(list.words().collect::<Vec<_>>(), ["Alpha"]);
    assert_eq!(list.add("").unwrap_err().to_string(), "a word cannot be empty");
    for bad in ["two words", "tab\tinside", "line\nbreak", "bell\u{7}", "nul\0", "\u{feff}", "\u{200b}", "\u{200e}", "\u{202e}", "\u{2066}"] {
        let err = list.add(bad).unwrap_err();
        assert!(matches!(err, WordError::NotAWord(_)), "{bad:?}: {err:?}");
        assert!(err.to_string().contains("one word"), "{err}");
    }
    let err = list.add(&"w".repeat(MAX_WORD_CHARS + 1)).unwrap_err();
    assert!(err.to_string().contains("at most 64 characters"), "{err}");
    list.add(&"w".repeat(MAX_WORD_CHARS)).unwrap();
    // A full list takes no more, and a full file takes no more.
    let mut full = WordFile::default();
    for i in 0..MAX_WORDS {
        full.add(&format!("w{i}")).unwrap();
    }
    assert!(matches!(full.add("one-more"), Err(WordError::TooMany)));
    let mut fat = WordFile::default();
    let mut n = 0;
    let err = loop {
        let word = format!("{n:0>60}");
        if let Err(e) = fat.add(&word) {
            break e;
        }
        n += 1;
    };
    assert!(matches!(err, WordError::TooBig(_)), "{err:?}");
    assert!(fat.render().len() <= MAX_DICT_FILE_BYTES);
}

#[test]
fn a_memory_host_keeps_the_text_and_the_files() {
    let host = MemoryHost::default();
    assert_eq!(host.read_config().unwrap(), None);
    host.write_config("a = 1\n").unwrap();
    assert_eq!(host.read_config().unwrap().as_deref(), Some("a = 1\n"));
    let custom = dictionary_file("Custom.dic");
    assert_eq!(host.read(&custom, 100).unwrap(), None);
    host.write(&custom, b"alpha\n").unwrap();
    assert_eq!(host.read(&custom, 100).unwrap().as_deref(), Some(&b"alpha\n"[..]));
    assert!(host.read(&custom, 3).is_err(), "a file past the limit given is refused, not cut");
    assert_eq!(host.list(FileKind::Dictionary).unwrap(), vec![custom.clone()]);
    assert!(host.list(FileKind::AutoCorrect).unwrap().is_empty());
    assert!(host.remove(&custom).unwrap());
    assert!(!host.remove(&custom).unwrap());
    host.update(&custom, &mut |old| {
        assert_eq!(old, None);
        Ok(Edit::Write(b"x\n".to_vec()))
    })
    .unwrap();
    host.update(&custom, &mut |old| {
        assert_eq!(old.as_deref(), Some(&b"x\n"[..]));
        Ok(Edit::Keep)
    })
    .unwrap();
    assert_eq!(host.read(&custom, 100).unwrap().as_deref(), Some(&b"x\n"[..]), "keep writes nothing");
    host.update(&custom, &mut |_| Ok(Edit::Remove)).unwrap();
    assert_eq!(host.read(&custom, 100).unwrap(), None, "remove removes the file");
    assert!(host.update(&custom, &mut |_| Err(PrefsError::Refused("no".into()))).is_err());
}

#[test]
fn a_profile_in_a_host_round_trips_through_its_text() {
    let host = Arc::new(MemoryHost::default());
    let (mut p, report) = host_profile(&host);
    assert!(report.persistent && report.dir.is_none() && !report.has_problems(), "{report:?}");
    assert_eq!(p.get().language.editing, vec!["fr-FR".to_owned()]);
    // Opening writes nothing: a person who has set nothing leaves nothing in the browser's storage.
    assert_eq!(host.config_text(), None);
    assert!(!p.tick().saved && host.config_text().is_none());
    p.update(|e| {
        e.proofing.ignore_uppercase = false;
        e.autocorrect.exceptions.first_letter.push("e.g.".into());
    });
    // A host has no clock: a change is written at the next tick.
    assert!(p.tick().saved);
    let text = host.config_text().unwrap();
    for part in [
        "schema_version = 1",
        "[proofing]",
        "ignore_uppercase = false",
        "[autocorrect.exceptions]",
        "first_letter = [\"e.g.\"]",
        "editing = [\"fr-FR\"]",
    ] {
        assert!(text.contains(part), "{part} in\n{text}");
    }
    assert!(!p.tick().saved, "and no more");
    let (q, report) = Profile::<EnginePrefs>::open_host(host.clone(), fixed("de_DE"));
    assert!(!report.has_problems());
    assert_eq!(q.get(), p.get());
    assert_eq!(q.get().language.editing, vec!["fr-FR".to_owned()]);
}

#[test]
fn a_host_profile_keeps_unknown_keys_and_comments() {
    let host = Arc::new(MemoryHost::default());
    host.set_config("# kept\nschema_version = 1\n[proofing]\nignore_uppercase = false # and this\n[interface]\nlanguage = \"de\"\n");
    let (mut p, _) = host_profile(&host);
    assert!(!p.get().proofing.ignore_uppercase);
    p.update(|e| e.proofing.hide_errors = true);
    p.flush().unwrap();
    let text = host.config_text().unwrap();
    for part in ["# kept", "# and this", "[interface]", "language = \"de\"", "hide_errors = true"] {
        assert!(text.contains(part), "{part} in\n{text}");
    }
}

#[test]
fn a_host_profile_sees_what_another_tab_wrote() {
    let host = Arc::new(MemoryHost::default());
    let (mut a, _) = host_profile(&host);
    a.flush().unwrap();
    let (mut b, _) = host_profile(&host);
    b.update(|e| e.proofing.grammar = true);
    b.flush().unwrap();
    host.set_config(&format!("{}\n[interface]\nlanguage = \"ja\"\n", host.config_text().unwrap()));
    a.update(|e| e.proofing.hide_errors = true);
    a.flush().unwrap();
    let text = host.config_text().unwrap();
    assert!(
        text.contains("language = \"ja\"") && text.contains("hide_errors = true"),
        "the other tab's key is kept, this one's change is made:\n{text}"
    );
}

#[test]
fn a_broken_text_in_a_host_is_kept_aside_and_the_defaults_used() {
    let host = Arc::new(MemoryHost::default());
    host.set_config("not [[ toml");
    let (mut p, report) = host_profile(&host);
    let note = report.notes.join("; ");
    assert!(note.contains("not valid TOML") && note.contains("kept") && note.contains("defaults"), "{note}");
    assert_eq!(host.broken(), vec!["not [[ toml".to_owned()]);
    assert!(p.get().proofing.ignore_uppercase);
    p.flush().unwrap();
    assert!(host.config_text().unwrap().starts_with("schema_version = 1"));
}

#[test]
fn a_text_in_a_host_that_is_too_big_is_not_read() {
    let host = Arc::new(MemoryHost::default());
    let big = format!("schema_version = 1\n# {}\n[proofing]\nignore_uppercase = false\n", "x".repeat(MAX_CONFIG_BYTES));
    host.set_config(&big);
    let (p, report) = host_profile(&host);
    assert!(p.get().proofing.ignore_uppercase, "never read");
    let note = report.notes.join("; ");
    assert!(note.contains("more than") && note.contains(&big.len().to_string()), "{note}");
}

#[test]
fn a_newer_text_in_a_host_is_read_only() {
    let host = Arc::new(MemoryHost::default());
    let newer = "schema_version = 99\n[proofing]\nignore_uppercase = false\n";
    host.set_config(newer);
    let (mut p, report) = host_profile(&host);
    assert!(report.read_only.is_some() && report.notes.join("; ").contains("newer DiaCraft"), "{report:?}");
    assert!(!p.get().proofing.ignore_uppercase);
    p.update(|e| e.proofing.hide_errors = true);
    assert!(matches!(p.flush(), Err(PrefsError::ReadOnly(_))));
    assert!(!p.tick().saved);
    assert_eq!(host.config_text().as_deref(), Some(newer));
    assert!(matches!(p.add_word(&dictionary_file("Custom.dic"), "alpha"), Err(PrefsError::ReadOnly(_))));
}

#[test]
fn a_host_that_cannot_write_is_told_once_and_tried_again_later() {
    let host = Arc::new(MemoryHost::default());
    let (mut p, _) = host_profile(&host);
    host.refuse_writes("the browser's storage is full");
    p.update(|e| e.proofing.grammar = true);
    let first = p.tick();
    assert!(!first.saved && first.failed.as_deref().is_some_and(|f| f.contains("storage is full")), "{first:?}");
    let again = p.tick();
    assert!(again.failed.is_none(), "told once: {again:?}");
    host.allow_writes();
    p.flush().unwrap();
    assert!(host.config_text().unwrap().contains("grammar = true"), "the change was still waiting");
}

#[test]
fn words_in_a_host_profile_are_files_of_the_host() {
    let host = Arc::new(MemoryHost::default());
    let (mut p, _) = host_profile(&host);
    let custom = dictionary_file("Custom.dic");
    assert_eq!(p.add_word(&custom, "alpha").unwrap().len(), 1);
    assert_eq!(p.add_word(&custom, "beta").unwrap().len(), 1);
    assert_eq!(host.file(&custom).as_deref(), Some(&b"alpha\nbeta\n"[..]));
    assert_eq!(p.list_files(FileKind::Dictionary).unwrap(), vec![custom.clone()]);
    p.remove_word(&custom, "alpha").unwrap();
    assert_eq!(p.read_file(&custom).unwrap().as_deref(), Some(&b"beta\n"[..]));
    // A file that the person attached, and that is not a word list, is refused by the edit and left as it is.
    host.put_file(&custom, b"caf\xe9\n");
    let err = p.add_word(&custom, "gamma").unwrap_err();
    assert!(matches!(err, PrefsError::Refused(_)) && err.to_string().contains("not UTF-8 or UTF-16 text"), "{err}");
    assert_eq!(host.file(&custom).as_deref(), Some(&b"caf\xe9\n"[..]));
    // Whole files are written and removed, and a file past its cap is refused.
    let list = FileName::new(FileKind::AutoCorrect, "de.tsv").unwrap();
    assert_eq!(p.write_file(&list, b"teh\tthe\n").unwrap()[0].describe(), "autocorrect/de.tsv: added 8 bytes");
    assert_eq!(p.write_file(&list, b"teh\tthe\n").unwrap().len(), 0, "the same bytes are no change");
    assert_eq!(p.write_file(&list, b"adn\tand\n").unwrap()[0].describe(), "autocorrect/de.tsv: 8 bytes -> 8 bytes");
    let big = vec![b'x'; FileKind::AutoCorrect.max_bytes() + 1];
    let err = p.write_file(&list, &big).unwrap_err();
    assert!(matches!(err, PrefsError::Refused(_)) && err.to_string().contains("more than"), "{err}");
    assert_eq!(p.remove_file(&list).unwrap()[0].describe(), "autocorrect/de.tsv: removed 8 bytes");
}

#[test]
fn a_session_with_no_profile_keeps_its_words_in_memory_within_a_budget() {
    let mut p = Profile::<EnginePrefs>::in_memory(LocaleSource::Fixed(None));
    assert!(!p.is_persistent() && p.dir().is_none());
    let custom = dictionary_file("Custom.dic");
    p.add_word(&custom, "alpha").unwrap();
    assert_eq!(p.read_file(&custom).unwrap().as_deref(), Some(&b"alpha\n"[..]));
    // The total is held too: many files of the largest size are not kept without end.
    let mut refused = None;
    for n in 0..200 {
        let name = dictionary_file(&format!("List{n}.dic"));
        if let Err(e) = p.write_file(&name, &vec![b'a'; FileKind::Dictionary.max_bytes()]) {
            refused = Some((n, e));
            break;
        }
    }
    let (n, err) = refused.expect("a limit");
    assert!((8..=64).contains(&n), "{n}");
    assert!(matches!(err, PrefsError::Refused(_)) && err.to_string().contains("more than"), "{err}");
}
