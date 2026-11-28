//! The tables of `diacraft_doc::lang`: LCID numbers, English names of languages, quotation marks and the script of a language.
//!
//! The tables are generated from ICU4C's `locmap.cpp` (the numbers) and Unicode CLDR's JSON (names, marks, scripts) by `cargo xtask langdata`; the generator's own
//! tests are `xtask/tests/it/langdata.rs`. These tests read what the generated tables answer, and every one names the wrong answer it catches.

use std::sync::Arc;

use diacraft_doc::lang::{
    self, Effective, Quotes, ScriptKind, Tag, document_language, effective, english_name, from_lcid, known, parse, quotes, script_of_language,
    to_lcid,
};
use diacraft_doc::{Cell, CellAddr, Document, Value};
use proptest::prelude::*;

fn tag(text: &str) -> Tag {
    parse(text).unwrap_or_else(|e| panic!("`{text}` should be a well-formed tag: {e}"))
}

fn marks(open: &'static str, close: &'static str, open_inner: &'static str, close_inner: &'static str) -> Quotes {
    Quotes { open, close, open_inner, close_inner }
}

// ---- LCID numbers -------------------------------------------------------------------------------------------------------------------

#[test]
fn the_numbers_of_the_census_pair_with_their_tags_both_ways() {
    // The legacy XML's census (spec R31): 1033, 2060, 2052, 2057 and 1031, and the spec's example 3082. Naive: a table built from the wrong half of the number
    // (the language is the low ten bits, the region the rest), or one that keeps `zh-Hans-CN` where the pair is `zh-CN`.
    let rows = [
        (1033, "en-US"),
        (2052, "zh-CN"),
        (1031, "de-DE"),
        (2057, "en-GB"),
        (2060, "fr-BE"),
        (3082, "es-ES"),
        (1036, "fr-FR"),
        (1041, "ja-JP"),
        (1042, "ko-KR"),
        (1049, "ru-RU"),
        (1040, "it-IT"),
        (1046, "pt-BR"),
        (2070, "pt-PT"),
        (1028, "zh-Hant-TW"),
        (1, "ar"),
        (3081, "en-AU"),
        (1055, "tr-TR"),
    ];
    for (number, text) in rows {
        assert_eq!(from_lcid(number), Some(tag(text)), "{number} is `{text}`");
        assert_eq!(to_lcid(&tag(text)), Some(number), "`{text}` is {number}");
    }
}

#[test]
fn a_language_alone_has_its_own_number() {
    // 0x0009 is English with no region, 0x0007 German, 0x7c04 Traditional Chinese, 0x0004 Simplified. Naive: only the numbers with a region.
    assert_eq!(from_lcid(9), Some(tag("en")));
    assert_eq!(from_lcid(7), Some(tag("de")));
    assert_eq!(from_lcid(0x7c04), Some(tag("zh-Hant")));
    assert_eq!(from_lcid(0x0004), Some(tag("zh-Hans")));
    assert_eq!(to_lcid(&tag("en")), Some(9));
    assert_eq!(to_lcid(&tag("zh-Hant")), Some(0x7c04));
}

#[test]
fn a_sort_variant_of_a_language_reads_as_the_language() {
    // 1034 is Spanish (Spain) with its traditional sort order; the sort order is no part of the language, and a file that says 1034 says Spanish (Spain). 3082 is
    // the modern sort of the same language and the number `es-ES` is given. Naive: a number the table lacks (the keyword `@collation=traditional` kept in the tag).
    assert_eq!(from_lcid(1034), Some(tag("es-ES")));
    assert_eq!(from_lcid(3082), Some(tag("es-ES")));
    assert_eq!(to_lcid(&tag("es-ES")), Some(3082));
}

#[test]
fn zero_the_invariant_locale_and_numbers_the_table_lacks_give_none() {
    // 0 is no language and 65535 is no pair. The invariant locale (0x7f) names no language. Past 16 bits a number is a sort order stacked on a language, not a
    // language ID, so it is no pair either. Naive: ICU's `root` for 0 and `en_US_POSIX` for 127, which are no languages.
    for number in [0, 0x7f, 65535, 0x10409, 0x30404, 0xFFFF_FFFF, 99_999, 0x3ff] {
        assert_eq!(from_lcid(number), None, "{number}");
    }
}

#[test]
fn a_tag_with_a_region_the_table_lacks_takes_the_number_of_the_longest_listed_prefix() {
    // ICU's own rule for a region it has no number for ("we try to return the number for the same language"): `en-FI` has none, `en` has 9.
    assert_eq!(to_lcid(&tag("en-FI")), Some(9));
    assert_eq!(to_lcid(&tag("zh-Hant-XX")), Some(0x7c04));
    assert_eq!(to_lcid(&tag("de-DE-1996")), Some(1031));
    assert_eq!(to_lcid(&tag("en-US-u-ca-gregory")), Some(1033));
    // A language the table lacks, no language and private use have no number.
    for text in ["qaa", "x-none", "zxx", "i-klingon", "qq-ZZ", "x-private"] {
        assert_eq!(to_lcid(&tag(text)), None, "`{text}`");
    }
}

#[test]
fn a_tag_is_found_in_whatever_case_it_was_written() {
    // `parse` makes the case canonical before the table is read.
    assert_eq!(to_lcid(&tag("EN-us")), Some(1033));
    assert_eq!(to_lcid(&tag("zh-hans-cn")), Some(2052));
    assert_eq!(to_lcid(&tag("sr-latn-rs")), Some(0x241a));
}

#[test]
fn the_two_scripts_of_a_language_keep_their_own_numbers() {
    // `sr-Cyrl-RS` and `sr-Latn-RS` are two numbers (0x281a, 0x241a); `uz-Latn-UZ` and `uz-Cyrl-UZ` (0x0443, 0x0843). Naive: one number for a language.
    assert_eq!(to_lcid(&tag("sr-Cyrl-RS")), Some(0x281a));
    assert_eq!(to_lcid(&tag("sr-Latn-RS")), Some(0x241a));
    assert_eq!(from_lcid(0x281a), Some(tag("sr-Cyrl-RS")));
    assert_eq!(from_lcid(0x241a), Some(tag("sr-Latn-RS")));
    assert_eq!(to_lcid(&tag("uz-Latn-UZ")), Some(0x0443));
    assert_eq!(to_lcid(&tag("uz-Cyrl-UZ")), Some(0x0843));
}

#[test]
fn a_tag_that_is_spelled_two_ways_has_one_number_and_the_first_spelling_back() {
    // ICU lists `zh_CN` and `zh_Hans_CN` for 0x0804: both give 2052, and the number gives the one ICU lists first (`uprv_convertToPosix`'s own choice). Likewise 0x0404:
    // `zh_Hant_TW` is listed before `zh_TW`.
    assert_eq!(to_lcid(&tag("zh-Hans-CN")), Some(2052));
    assert_eq!(to_lcid(&tag("zh-CN")), Some(2052));
    assert_eq!(from_lcid(2052), Some(tag("zh-CN")));
    assert_eq!(to_lcid(&tag("zh-Hant-TW")), Some(1028));
    assert_eq!(to_lcid(&tag("zh-TW")), Some(1028));
    assert_eq!(from_lcid(1028), Some(tag("zh-Hant-TW")));
    // A number whose pair another subtable holds: `fa-AF` is 0x048c in the Persian subtable, `nb-NO` 0x0414 and `nn-NO` 0x0814 in Norwegian's.
    assert_eq!(to_lcid(&tag("fa-AF")), Some(0x048c));
    assert_eq!(to_lcid(&tag("nb-NO")), Some(0x0414));
    assert_eq!(to_lcid(&tag("nn-NO")), Some(0x0814));
    assert_eq!(to_lcid(&tag("no-NO")), Some(0x0414));
    assert_eq!(from_lcid(0x0414), Some(tag("nb-NO")));
}

#[test]
fn a_tag_the_language_subtable_and_another_both_list_takes_the_number_of_its_own_language() {
    // ICU's Irish subtable lists `gd_GB` as 0x043c and the Scottish Gaelic one as 0x0491: a tag is looked up in the subtable of its own language first.
    assert_eq!(to_lcid(&tag("gd-GB")), Some(0x0491));
}

#[test]
fn the_tables_hold_no_alias_that_is_no_language() {
    // ICU keeps `en_US_POSIX` and `root` for round trips, and old or retired codes (`iw`, `ven`, `tmz`, `gaz`) CLDR has no name for: none is a tag of the table, and
    // none gives its number a second tag. Naive: every id of ICU's file.
    for text in ["en-US-posix", "root", "iw-IL", "ven-ZA", "tmz", "gaz-ET", "no-NO-NY"] {
        assert!(!known().contains(&text), "`{text}` is not a tag of the table");
    }
    assert_eq!(from_lcid(0x040d), Some(tag("he-IL")));
    assert_eq!(from_lcid(0x0433), Some(tag("ve-ZA")));
    assert_eq!(from_lcid(0x0814), Some(tag("nn-NO")));
}

#[test]
fn the_whole_range_of_a_language_id_can_be_asked() {
    // Hostile input: every 16-bit number and a spread of larger ones answer without a panic, and an answer is a tag that parses, is in canonical case and has a number.
    for number in (0..=0xFFFF_u32).chain([0x10000, 0xFFFF_FFFF, 0x8000_0000, 0x1_0409 - 1]) {
        if let Some(found) = from_lcid(number) {
            assert_eq!(parse(found.as_str()).as_ref(), Ok(&found), "{number}");
            assert!(to_lcid(&found).is_some(), "{number}: `{found}` has a number");
        }
    }
}

#[test]
fn a_tag_whose_region_cldr_does_not_name_is_paired_but_not_offered() {
    // 0x0c1a and 0x081a are Serbian (Cyrillic and Latin) of Serbia and Montenegro, a region that no longer exists and that CLDR no longer names. A file made then must
    // still read, so the numbers pair; but the name would hold the bare code (`CS`), so a person is not offered the tags. Naive: the two numbers lost with the name.
    assert_eq!(from_lcid(0x0c1a), Some(tag("sr-Cyrl-CS")));
    assert_eq!(from_lcid(0x081a), Some(tag("sr-Latn-CS")));
    assert_eq!(to_lcid(&tag("sr-Cyrl-CS")), Some(0x0c1a));
    assert_eq!(english_name(&tag("sr-Cyrl-CS")), "Serbian (Cyrillic, CS)");
    assert!(!known().contains(&"sr-Cyrl-CS") && !known().contains(&"sr-Latn-CS"));
    assert_eq!(effective(Some("3098"), &tag("en-US")), Effective::Language(tag("sr-Cyrl-CS")));
}

// ---- known tags ---------------------------------------------------------------------------------------------------------------------

#[test]
fn every_tag_of_the_table_parses_in_canonical_case_has_an_english_name_and_a_number() {
    let table = known();
    assert!(table.len() >= 150, "the table lists {} tags: ICU pairs more than that", table.len());
    for text in table {
        let found = parse(text).unwrap_or_else(|e| panic!("`{text}` is in the table and is not a tag: {e}"));
        assert_eq!(found.as_str(), *text, "`{text}` is in canonical case");
        let name = english_name(&found);
        assert!(!name.is_empty(), "`{text}` has a name");
        // A subtag CLDR names is written as its name: the raw code of the language, the script or the region is not left in the name as a word.
        for sub in text.split('-') {
            assert!(!name.split(|c: char| !c.is_alphanumeric()).any(|word| word == sub), "`{text}`: the name `{name}` holds the code `{sub}`");
        }
        let number = to_lcid(&found).unwrap_or_else(|| panic!("`{text}` has no number"));
        assert_eq!(from_lcid(number), Some(found.clone()), "`{text}` is {number}, which is `{text}` again");
    }
}

#[test]
fn the_tags_of_the_table_are_sorted_and_listed_once() {
    // A picker and a binary search both rely on it.
    let table = known();
    assert!(table.windows(2).all(|pair| pair[0] < pair[1]), "sorted by bytes, no tag twice");
    for text in
        ["en-US", "en-GB", "de-DE", "fr-FR", "es-ES", "pt-BR", "pt-PT", "ja-JP", "zh-CN", "zh-Hant-TW", "ar-SA", "he-IL", "hi-IN", "th-TH", "ko-KR"]
    {
        assert!(table.contains(&text), "`{text}` is a tag of the table");
    }
}

// ---- English names ------------------------------------------------------------------------------------------------------------------

#[test]
fn a_language_is_named_in_english_with_its_region_in_brackets() {
    // CLDR's own pattern (`{0} ({1})`) and its separator for a script and a region (`{0}, {1}`): the names a language list shows. Naive: the tag, or the name of the
    // language alone, or the dialect names ("British English") that CLDR keeps beside.
    let rows = [
        ("en", "English"),
        ("en-US", "English (United States)"),
        ("en-GB", "English (United Kingdom)"),
        ("de-DE", "German (Germany)"),
        ("fr-BE", "French (Belgium)"),
        ("pt-BR", "Portuguese (Brazil)"),
        ("pt-PT", "Portuguese (Portugal)"),
        ("zh-CN", "Chinese (China)"),
        ("zh-Hans-CN", "Chinese (Simplified, China)"),
        ("zh-Hant", "Chinese (Traditional)"),
        ("sr-Latn-RS", "Serbian (Latin, Serbia)"),
        ("es-419", "Spanish (Latin America)"),
        ("ca-ES-valencia", "Catalan (Spain, Valencian)"),
        ("ja-JP", "Japanese (Japan)"),
    ];
    for (text, name) in rows {
        assert_eq!(english_name(&tag(text)), name, "`{text}`");
    }
}

#[test]
fn a_name_the_data_lacks_is_the_subtag_itself_and_an_extension_is_left_out() {
    // A well-formed tag no table lists is still a tag (design 5.1): it gets a name, built from what is known. Naive: a panic, an empty name, or `-u-ca-gregory`
    // read as a region.
    assert_eq!(english_name(&tag("qaa")), "qaa");
    assert_eq!(english_name(&tag("en-AA")), "English (AA)");
    assert_eq!(english_name(&tag("qq-Qaaa-AA")), "qq (Qaaa, AA)");
    assert_eq!(english_name(&tag("en-US-u-ca-gregory")), "English (United States)");
    assert_eq!(english_name(&tag("en-US-x-private")), "English (United States)");
    // Private use and the grandfathered tags have no CLDR name: the tag is the name. `zxx` is a language code of its own, which CLDR names.
    assert_eq!(english_name(&tag("x-none")), "x-none");
    assert_eq!(english_name(&tag("i-klingon")), "i-klingon");
    assert_eq!(english_name(&tag("en-GB-oed")), "en-GB-oed");
    assert_eq!(english_name(&tag("zxx")), "No linguistic content");
}

#[test]
fn a_name_is_built_from_a_tag_of_any_length_within_a_bound() {
    // Hostile input: a tag of the longest length (255 bytes) of many variants names itself without growing past a few times its length.
    let text = format!("en{}", "-abcdefgh".repeat(28));
    assert!(text.len() <= lang::MAX_TAG_BYTES);
    let name = english_name(&tag(&text));
    assert!(name.len() <= text.len() * 4, "{} bytes for {} bytes of tag", name.len(), text.len());
    assert!(name.starts_with("English ("));
}

// ---- quotation marks ----------------------------------------------------------------------------------------------------------------

#[test]
fn quotation_marks_are_the_languages_own() {
    // CLDR's delimiters. Naive: the English marks for every language, or the straight ones.
    assert_eq!(quotes(&tag("en")), marks("\u{201c}", "\u{201d}", "\u{2018}", "\u{2019}"));
    assert_eq!(quotes(&tag("en-US")), marks("\u{201c}", "\u{201d}", "\u{2018}", "\u{2019}"));
    assert_eq!(quotes(&tag("en-GB")), marks("\u{201c}", "\u{201d}", "\u{2018}", "\u{2019}"));
    assert_eq!(quotes(&tag("de")), marks("\u{201e}", "\u{201c}", "\u{201a}", "\u{2018}"));
    assert_eq!(quotes(&tag("de-DE")), marks("\u{201e}", "\u{201c}", "\u{201a}", "\u{2018}"));
    assert_eq!(quotes(&tag("fr")).open, "\u{ab}");
    assert_eq!(quotes(&tag("fr")).close, "\u{bb}");
    assert_eq!(quotes(&tag("fr-CH")), marks("\u{ab}", "\u{bb}", "\u{2039}", "\u{203a}"));
    assert_eq!(quotes(&tag("ja")), marks("\u{300c}", "\u{300d}", "\u{300e}", "\u{300f}"));
    assert_eq!(quotes(&tag("ru")), marks("\u{ab}", "\u{bb}", "\u{201e}", "\u{201c}"));
}

#[test]
fn a_region_that_changes_the_marks_is_read_and_one_that_does_not_falls_to_the_language() {
    // Portugal writes « » where Brazil writes “ ”; Angola follows Portugal (CLDR's parent of `pt-AO` is `pt-PT`). `de-AT` and `en-AU` have none of their own.
    assert_eq!(quotes(&tag("pt-PT")), marks("\u{ab}", "\u{bb}", "\u{201c}", "\u{201d}"));
    assert_eq!(quotes(&tag("pt-BR")), marks("\u{201c}", "\u{201d}", "\u{2018}", "\u{2019}"));
    assert_eq!(quotes(&tag("pt")), marks("\u{201c}", "\u{201d}", "\u{2018}", "\u{2019}"));
    assert_eq!(quotes(&tag("pt-AO")).open, "\u{ab}");
    assert_eq!(quotes(&tag("de-DE-1996")), quotes(&tag("de")));
    assert_eq!(quotes(&tag("en-US-u-ca-gregory")), quotes(&tag("en")));
    assert_eq!(quotes(&tag("en-US-x-private")), quotes(&tag("en")));
}

#[test]
fn a_chinese_region_takes_its_scripts_marks_though_the_data_names_the_script() {
    // CLDR keeps `zh-Hant` and `zh-Hant-HK`, not `zh-TW`: `zh-TW` is `zh-Hant-TW` by the likely subtags, so it writes 「」 where `zh` and `zh-CN` write “”. Naive: the
    // chain `zh-TW`, `zh`, which gives the Simplified marks to Taiwan.
    let traditional = marks("\u{300c}", "\u{300d}", "\u{300e}", "\u{300f}");
    let simplified = marks("\u{201c}", "\u{201d}", "\u{2018}", "\u{2019}");
    for text in ["zh-TW", "zh-HK", "zh-MO", "zh-Hant", "zh-Hant-TW", "zh-Hant-HK"] {
        assert_eq!(quotes(&tag(text)), traditional, "`{text}`");
    }
    for text in ["zh", "zh-CN", "zh-Hans", "zh-Hans-CN", "zh-SG"] {
        assert_eq!(quotes(&tag(text)), simplified, "`{text}`");
    }
}

#[test]
fn a_language_with_no_marks_gets_the_english_ones() {
    // The root of CLDR's data is the English marks; a language it lacks, no language and the grandfathered tags take them.
    let english = marks("\u{201c}", "\u{201d}", "\u{2018}", "\u{2019}");
    for text in ["qaa", "qq-ZZ", "x-none", "zxx", "i-klingon", "x-private"] {
        assert_eq!(quotes(&tag(text)), english, "`{text}`");
    }
}

#[test]
fn every_tag_of_the_table_has_marks() {
    for text in known() {
        let found = quotes(&tag(text));
        assert!(!found.open.is_empty() && !found.close.is_empty(), "`{text}`: outer marks");
        assert!(!found.open_inner.is_empty() && !found.close_inner.is_empty(), "`{text}`: inner marks");
    }
}

// ---- the script of a language -------------------------------------------------------------------------------------------------------

#[test]
fn a_language_has_the_script_type_of_its_likely_script() {
    // The three types of the Character section's faces: East Asian, complex and the rest (Latin, Greek, Cyrillic). By CLDR's likely script of the language.
    let rows = [
        ("en", ScriptKind::Latin),
        ("de-DE", ScriptKind::Latin),
        ("ru", ScriptKind::Latin),
        ("el", ScriptKind::Latin),
        ("ja", ScriptKind::Asian),
        ("ja-JP", ScriptKind::Asian),
        ("ko", ScriptKind::Asian),
        ("zh", ScriptKind::Asian),
        ("zh-TW", ScriptKind::Asian),
        ("yue", ScriptKind::Asian),
        ("ar", ScriptKind::Complex),
        ("ar-SA", ScriptKind::Complex),
        ("he", ScriptKind::Complex),
        ("fa", ScriptKind::Complex),
        ("th", ScriptKind::Complex),
        ("hi", ScriptKind::Complex),
        ("ta", ScriptKind::Complex),
        ("km", ScriptKind::Complex),
    ];
    for (text, kind) in rows {
        assert_eq!(script_of_language(&tag(text)), Some(kind), "`{text}`");
    }
}

#[test]
fn a_script_in_the_tag_or_a_region_that_changes_the_script_is_read_first() {
    // `sr-Latn` is Latin where `sr` is Cyrillic (both Latin type), but `uz-Arab`, `uz-AF` (likely `uz-Arab-AF`) and `mn-CN` (`mn-Mong-CN`) are complex where `uz` and
    // `mn` are not; `zh-Latn` is Latin and `ja-Latn` too. Naive: the language alone.
    assert_eq!(script_of_language(&tag("uz")), Some(ScriptKind::Latin));
    assert_eq!(script_of_language(&tag("uz-Arab")), Some(ScriptKind::Complex));
    assert_eq!(script_of_language(&tag("uz-AF")), Some(ScriptKind::Complex));
    assert_eq!(script_of_language(&tag("mn")), Some(ScriptKind::Latin));
    assert_eq!(script_of_language(&tag("mn-CN")), Some(ScriptKind::Complex));
    assert_eq!(script_of_language(&tag("zh-Latn")), Some(ScriptKind::Latin));
    assert_eq!(script_of_language(&tag("ja-Latn")), Some(ScriptKind::Latin));
    assert_eq!(script_of_language(&tag("sr-Latn-RS")), Some(ScriptKind::Latin));
    assert_eq!(script_of_language(&tag("pa-Guru-IN")), Some(ScriptKind::Complex));
}

#[test]
fn a_language_no_data_lists_has_no_script_type() {
    for text in ["qaa", "qq", "x-none", "zxx", "i-klingon", "x-private"] {
        assert_eq!(script_of_language(&tag(text)), None, "`{text}`");
    }
}

// ---- numbers in effective and document_language -------------------------------------------------------------------------------------

#[test]
fn a_number_of_the_legacy_xml_reads_as_its_tag() {
    // The number the table pairs is that language, whatever the document's is. 0 is no language, 65535 and a number the table lacks are the document's.
    let document = tag("tr-TR");
    for (text, tag_text) in
        [("1033", "en-US"), ("2052", "zh-CN"), ("1031", "de-DE"), ("2057", "en-GB"), ("2060", "fr-BE"), ("3082", "es-ES"), ("1034", "es-ES")]
    {
        assert_eq!(effective(Some(text), &document), Effective::Language(tag(tag_text)), "`{text}`");
    }
    // Leading zeros are the same number.
    assert_eq!(effective(Some("01033"), &document), Effective::Language(tag("en-US")));
    for text in ["0", "00"] {
        assert_eq!(effective(Some(text), &document), Effective::None, "`{text}`");
    }
    for text in ["65535", "99999", "4294967295", "4294967296", "99999999999999999999999999", "67593"] {
        assert_eq!(effective(Some(text), &document), Effective::Language(document.clone()), "`{text}`");
    }
}

fn doc_with(doc_lang_id: Option<&str>, core: Option<&str>) -> Document {
    let mut doc = Document::empty();
    if let Some(text) = doc_lang_id {
        Arc::make_mut(&mut doc.sheet).set_cell(&CellAddr::direct("DocLangID"), Cell::constant(Value::from_text(text))).unwrap();
    }
    if let Some(text) = core {
        Arc::make_mut(&mut doc.props).language = Some(Arc::from(text));
    }
    doc
}

#[test]
fn a_number_in_doclangid_is_the_drawings_language_when_the_table_pairs_it() {
    // The legacy XML's `DocLangID` is a number (1033). Naive: passed over to the core language, as before the table.
    let preferred = tag("nl-NL");
    assert_eq!(document_language(&doc_with(Some("1033"), Some("de-DE")), Some(&preferred)), tag("en-US"));
    assert_eq!(document_language(&doc_with(None, Some("1031")), Some(&preferred)), tag("de-DE"));
    assert_eq!(document_language(&doc_with(Some("2052"), None), None), tag("zh-CN"));
    // 0 and 65535 and a number the table lacks are passed over, as is any value that says no language.
    for bad in ["0", "65535", "99999", "0x409"] {
        assert_eq!(document_language(&doc_with(Some(bad), Some("de-DE")), Some(&preferred)), tag("de-DE"), "DocLangID `{bad}`");
        assert_eq!(document_language(&doc_with(Some(bad), None), Some(&preferred)), tag("nl-NL"), "DocLangID `{bad}`");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Any tag of the table is read back from its own number, whichever way a file spelled the number (decimal digits, leading zeros).
    #[test]
    fn a_number_written_as_text_reads_as_the_tag_it_is(index in 0usize..known().len(), zeros in 0usize..5) {
        let found = tag(known()[index]);
        let number = to_lcid(&found).unwrap();
        let text = format!("{}{number}", "0".repeat(zeros));
        prop_assert_eq!(effective(Some(&text), &tag("en-US")), Effective::Language(found));
    }

    /// The laws of the `lang_tag` fuzz target (`crates/doc/fuzz`), which cannot run here: digits are the tag the table pairs with the number, else the document's language;
    /// a number gives a tag that reads as itself and has a number; a tag has a name, four marks and a number of 16 bits.
    #[test]
    fn the_laws_of_the_fuzz_target_hold(text in prop_oneof![ "[0-9]{1,6}", "[A-Za-z0-9-]{1,24}", proptest::collection::vec(any::<u8>(), 0..32).prop_map(|b| String::from_utf8_lossy(&b).into_owned()) ]) {
        let document = tag("tr-TR");
        if let Effective::Language(found) = effective(Some(&text), &document)
            && parse(&text).is_err()
        {
            let paired = if text.bytes().all(|b| b.is_ascii_digit()) { text.parse::<u32>().ok().and_then(from_lcid) } else { None };
            prop_assert_eq!(found, paired.unwrap_or(document));
        }
        if let Ok(found) = parse(&text) {
            let name = english_name(&found);
            prop_assert!(!name.is_empty() && name.len() <= 64 * found.as_str().len());
            let marks = quotes(&found);
            prop_assert!([marks.open, marks.close, marks.open_inner, marks.close_inner].iter().all(|m| !m.is_empty()));
            prop_assert!(to_lcid(&found).is_none_or(|n| n <= 0xFFFF));
        }
        if let Ok(number) = text.parse::<u32>()
            && let Some(found) = from_lcid(number)
        {
            prop_assert!(number <= 0xFFFF);
            let again = parse(found.as_str());
            prop_assert_eq!(again.as_ref(), Ok(&found));
            prop_assert!(to_lcid(&found).is_some());
        }
    }

    /// No tag and no number make the lookups panic or give a tag outside the table; a hostile tag keeps its name within a bound.
    #[test]
    fn any_well_formed_tag_can_be_looked_up(text in "[A-Za-z]{2,3}(-[A-Za-z0-9]{1,8}){0,6}") {
        if let Ok(found) = parse(&text) {
            let _ = to_lcid(&found);
            let _ = script_of_language(&found);
            prop_assert!(!english_name(&found).is_empty());
            let marks = quotes(&found);
            prop_assert!(!marks.open.is_empty());
            if let Some(number) = to_lcid(&found) {
                prop_assert!(from_lcid(number).is_some());
            }
        }
    }
}
