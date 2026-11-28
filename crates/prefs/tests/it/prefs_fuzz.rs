//! The fuzz target `prefs_load` of `crates/prefs/fuzz`, run here: the law of its library (`fuzz/src/lib.rs`, included below) held to hand-made hostile inputs and to
//! random ones, and the hostile inputs that a fuzzer is slow to reach (a file of a hundred thousand keys, a list of a hundred thousand dictionaries), built by hand and bounded by
//! what they make the store do.

use std::sync::Arc;

use diacraft_prefs::{
    EnginePrefs, LocaleSource, MAX_CONFIG_BYTES, MAX_DICTIONARIES, MAX_EDITING_LANGUAGES, MAX_EXCEPTIONS, MAX_TOP_KEYS, MemoryHost, Profile,
};
use proptest::prelude::*;

#[allow(dead_code)]
#[path = "../../fuzz/src/lib.rs"]
mod law;

fn host_with(text: &str) -> (Profile<EnginePrefs>, diacraft_prefs::Report) {
    let host = Arc::new(MemoryHost::default());
    host.set_config(text);
    Profile::open_host(host, LocaleSource::Fixed(Some("de_DE".to_owned())))
}

fn config(text: &str) -> Vec<u8> {
    let mut input = vec![0];
    input.extend(text.as_bytes());
    input
}

#[test]
fn crafted_texts_of_config_toml_hold_the_law() {
    let deep_tables = format!("[{}]\nx = 1\n", (0..200).map(|i| format!("t{i}")).collect::<Vec<_>>().join("."));
    let deep_inline = format!("{}1{}", "a = ".to_owned() + &"{ b = ".repeat(100), " }".repeat(100));
    let many_editing = format!("[language]\nediting = [{}]\n", (0..50_000).map(|i| format!("\"a{}\"", i % 10)).collect::<Vec<_>>().join(", "));
    let many_words =
        format!("[autocorrect.exceptions]\ninitial_caps = [{}]\n", (0..30_000).map(|i| format!("\"E{i}\"")).collect::<Vec<_>>().join(", "));
    let many_dictionaries = format!(
        "[proofing]\ndictionaries = [{}]\n",
        (0..8_000).map(|i| format!("{{ name = \"d{i}\", file = \"d{i}.dic\", language = \"de\", default = true }}")).collect::<Vec<_>>().join(", ")
    );
    let inputs = [
        String::new(),
        "\0\0\0".to_owned(),
        "\u{feff}schema_version = 1\n".to_owned(),
        "schema_version = 99999999999999999999\n".to_owned(),
        "schema_version = -1\n[proofing]\nignore_uppercase = false\n".to_owned(),
        "schema_version = 1\nproofing = 5\nlanguage = [1, 2]\nautocorrect = \"x\"\n".to_owned(),
        "[[proofing]]\nignore_uppercase = false\n[[proofing]]\n".to_owned(),
        "proofing.dictionaries = 5\nlanguage.editing = \"de\"\n".to_owned(),
        "[proofing]\ndictionaries = [{ name = 1, file = [], language = {}, default = \"yes\" }]\n".to_owned(),
        "[proofing]\nignore_uppercase = nan\nhide_errors = inf\n".to_owned(),
        "[language]\nediting = [\"\\u0000\", \"\\u202e\", \"de\\r\\nfr\", \"\u{1F600}\"]\n".to_owned(),
        "[autocorrect.exceptions]\ninitial_caps = [\"a b\", \"\\t\", \"\\u0085\", \"x\"]\nfirst_letter = [[], {}]\n".to_owned(),
        deep_tables,
        deep_inline,
        many_editing,
        many_words,
        many_dictionaries,
        "x".repeat(MAX_CONFIG_BYTES),
        format!("# {}\n", "é".repeat(MAX_CONFIG_BYTES)),
    ];
    for text in &inputs {
        law::check(&config(text));
    }
}

#[test]
fn crafted_word_lists_hold_the_law() {
    let inputs: [Vec<u8>; 9] = [
        Vec::new(),
        b"a b\r\r\nword\n\n\n# c\r\n".to_vec(),
        vec![0xFF, 0xFE],
        vec![0xFE, 0xFF, 0xD8, 0x00, 0xDC, 0x00],
        vec![0xEF, 0xBB, 0xBF],
        "word\n".repeat(40_000).into_bytes(),
        vec![b'z'; 1_000_000],
        b"\x01\x02\n\x7f\n\xe2\x80\xae\n".to_vec(),
        "Grüße\nGRÜSSE\ngrüße\nİstanbul\nistanbul\n".as_bytes().to_vec(),
    ];
    for bytes in &inputs {
        let mut input = vec![1];
        input.extend(bytes);
        law::check(&input);
    }
}

#[test]
fn the_crash_of_fix_prefsbom_holds_the_law() {
    // The input the fuzzer found: word mode, a first line of a vertical tab then U+FEFF, whose rendered list was read back as a byte order mark and lost.
    law::check(&[0x0b, 0x0b, 0xef, 0xbb, 0xbf]);
}

#[test]
fn the_crash_of_fix_prefsffff_holds_the_law() {
    // The input the fuzzer found: word mode, a UTF-16 byte order mark, then U+FEFF and U+FFFF, which came back as a word the first read never held.
    law::check(&[0x06, 0xFF, 0xFE, 0xFF, 0xFE, 0xFF, 0xFF]);
}

#[test]
fn a_file_of_tens_of_thousands_of_keys_is_not_read() {
    let keys = (0..40_000).map(|i| format!("k{i} = 1\n")).collect::<String>();
    assert!(keys.len() < MAX_CONFIG_BYTES);
    // The keys come first: after a section header they would be keys of that section.
    let (profile, report) = host_with(&format!("{keys}[proofing]\nignore_uppercase = false\n"));
    let note = report.notes.join("; ");
    assert!(note.contains("40001 top-level keys") && note.contains(&format!("more than the {MAX_TOP_KEYS}")) && note.contains("not read"), "{note}");
    assert!(profile.get().proofing.ignore_uppercase, "none of it was read");
}

#[test]
fn a_file_with_the_most_keys_settings_may_have_is_read() {
    let keys = (0..MAX_TOP_KEYS - 1).map(|i| format!("k{i} = 1\n")).collect::<String>();
    let (profile, report) = host_with(&format!("{keys}[proofing]\nignore_uppercase = false\n"));
    assert!(report.notes.is_empty(), "{:?}", report.notes);
    assert!(!profile.get().proofing.ignore_uppercase);
}

#[test]
fn lists_of_tens_of_thousands_of_entries_are_cut_at_their_caps() {
    let names = (0..30_000).map(|i| format!("\"E{i}\"")).collect::<Vec<_>>().join(",");
    let dictionaries =
        (0..3_000).map(|i| format!("{{name=\"d{i}\",file=\"d{i}.dic\",language=\"all\",default=false}}")).collect::<Vec<_>>().join(",");
    let tags = (0..5_000)
        .map(|i| format!("\"x{}{}-US\"", (b'a' + (i % 26) as u8) as char, (b'a' + (i / 26 % 26) as u8) as char))
        .collect::<Vec<_>>()
        .join(",");
    let text = format!(
        "[autocorrect.exceptions]\ninitial_caps=[{names}]\nfirst_letter=[{names}]\n[proofing]\ndictionaries=[{dictionaries}]\n[language]\nediting=[{tags}]\n"
    );
    assert!(text.len() < MAX_CONFIG_BYTES, "{}", text.len());
    let (profile, report) = host_with(&text);
    let e = profile.get();
    assert_eq!(e.proofing.dictionaries.len(), MAX_DICTIONARIES, "{:?}", report.notes);
    assert_eq!(e.autocorrect.exceptions.initial_caps.len(), MAX_EXCEPTIONS);
    assert_eq!(e.autocorrect.exceptions.first_letter.len(), MAX_EXCEPTIONS);
    assert_eq!(e.language.editing.len(), MAX_EDITING_LANGUAGES);
    law::in_range(e);
    // The same past the cap of the file is not read at all.
    let more = format!("{text}# {}\n", "x".repeat(MAX_CONFIG_BYTES));
    let (profile, report) = host_with(&more);
    assert!(report.notes.join("; ").contains("more than"), "{:?}", report.notes);
    assert!(profile.get().proofing.dictionaries.is_empty());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Any bytes, under any of the five readings.
    #[test]
    fn random_bytes_hold_the_law(mode in 0u8..5, bytes in proptest::collection::vec(any::<u8>(), 0..400)) {
        let mut input = vec![mode];
        input.extend(bytes);
        law::check(&input);
    }

    /// Texts made of the pieces of a real file, put together in any order and cut anywhere.
    #[test]
    fn pieces_of_a_real_file_hold_the_law(picks in proptest::collection::vec(0usize..12, 0..24), cut in 0usize..4) {
        const PIECES: [&str; 12] = [
            "schema_version = 1\n",
            "[proofing]\n",
            "ignore_uppercase = false\n",
            "dictionaries = [{ name = \"A\", file = \"A.dic\", language = \"de-DE\", default = true }]\n",
            "dictionaries = [{ name = \"../\", file = \"../x.dic\" }]\n",
            "[autocorrect]\n",
            "smart_quotes = 0\n",
            "[autocorrect.exceptions]\n",
            "initial_caps = [\"IDs\", \"\", \"a b\"]\n",
            "[language]\n",
            "editing = [\"en_us\", \"de-DE\", \"x\"]\n",
            "[interface]\nlanguage = \"de\"\n",
        ];
        let text: String = picks.iter().map(|i| PIECES[*i]).collect();
        let text = &text[..text.len() - text.len() % (cut + 1)];
        law::check(&config(text));
    }
}
