//! `cargo xtask langdata`: the tables of `diacraft_doc::lang` that Unicode CLDR's JSON and ICU4C's `locmap.cpp` give.
//!
//! The generator is tested on a miniature of each source written here, in scratch folders: what it reads, what it leaves out, what it refuses, and that two runs give
//! the same bytes. The committed tables are held to the pinned sources by their headers; the regeneration against the real downloads is an ignored test
//! (`LANGDATA_CLDR` and `LANGDATA_ICU` name the folders: docs/development.md).

use sha2::{Digest, Sha256};
use xtask::langdata::{self, PINS, Pins, check_i18n_source, generate};

use crate::TempDir;

// ---- the miniature sources ----------------------------------------------------------------------------------------------------------

const LOCMAP_H: &str = "#ifndef LOCMAP_H\n#define LOCMAP_H\n\n#define LANGUAGE_LCID(hostID) (uint16_t)(0x03FF & hostID)\n\n#endif /* LOCMAP_H */\n";

const ICU_LICENSE: &str = "UNICODE LICENSE V3\n\nCOPYRIGHT AND PERMISSION NOTICE\n\nSPDX-License-Identifier: Unicode-3.0\n";

/// A `locmap.cpp` of the same shape as ICU's: comments (one that holds a table), preprocessor lines with a continuation, the one-line macro, the subtables and the map.
const LOCMAP_CPP: &str = r#"// (c) 2016 and later: Unicode, Inc. and others.
#include "locmap.h"
#if U_PLATFORM_HAS_WIN32_API
#include <windows.h>
#endif

#define ILCID_POSIX_ELEMENT_ARRAY(hostID, languageID, posixID) \
constexpr ILcidPosixElement locmap_ ## languageID [] = { \
    {LANGUAGE_LCID(hostID), #languageID},     /* parent locale */ \
    {hostID, #posixID}, \
};
#define ILCID_POSIX_SUBTABLE(id) \
constexpr ILcidPosixElement locmap_ ## id [] =
#define ILCID_POSIX_MAP(_posixID) \
    {UPRV_LENGTHOF(locmap_ ## _posixID), locmap_ ## _posixID}

ILCID_POSIX_ELEMENT_ARRAY(0x0436, af, af_ZA)

ILCID_POSIX_SUBTABLE(de) {
    {0x07,   "de"},
    {0x0c07, "de_AT"},
    {0x0407, "de_DE"},
    {0x10407,"de_DE@collation=phonebook"},  /*This is really de_DE_PHONEBOOK*/
    {0x10407,"de@collation=phonebook"}
};

// Windows uses an empty string for 'invariant'
ILCID_POSIX_SUBTABLE(en) {
    {0x09,   "en"},
    {0x0809, "en_GB"},
    {0x0409, "en_US"},
    {0x007f, "en_US_POSIX"}, /* duplicate for round-tripping */
    {0x2409, "en_029"},
    {0x3009, "en_ZW"},
    {0x0409, "en_AS"}   /* Alias for en_US. Leave last. */
};

ILCID_POSIX_SUBTABLE(en_US_POSIX) {
    {0x007f, "en_US_POSIX"} /* duplicate for roundtripping */
};

ILCID_POSIX_SUBTABLE(es) {
    {0x0a,   "es"},
    {0x0c0a, "es_ES"},      /*Modern sort.*/
    {0x040a, "es_ES@collation=traditional"},
    {0x040a, "es@collation=traditional"}        // Windows will treat this as es-ES
};

ILCID_POSIX_SUBTABLE(ga) { /* Gaelic (Ireland) */
    {0x3c,   "ga"},
    {0x083c, "ga_IE"},
    {0x043c, "gd_GB"}
};

ILCID_POSIX_SUBTABLE(gd) { /* Gaelic (Scotland) */
    {0x91,   "gd"},
    {0x0491, "gd_GB"}
};

ILCID_POSIX_ELEMENT_ARRAY(0x040d, he, he_IL)
ILCID_POSIX_ELEMENT_ARRAY(0x040d, iw, iw_IL)    /*Left in for compatibility*/

/*ILCID_POSIX_SUBTABLE(ber) {
    {0x5f,   "ber"},
    {0x045f, "ber_Arab_DZ"}
};*/

ILCID_POSIX_SUBTABLE(no) {
    {0x14,   "no"},
    {0x7c14, "nb"},
    {0x0414, "nb_NO"},
    {0x0414, "no_NO"},
    {0x0814, "nn_NO"},
    {0x0814, "no_NO_NY"}
};

ILCID_POSIX_SUBTABLE(quc) {
    {0x93,   "quc"},
    {0x0493, "quc_CO"},
    {0x0486, "quc_Latn_GT"}
};

ILCID_POSIX_SUBTABLE(qut) {
    {0x86,   "qut"},
    {0x0486, "qut_GT"},
    {0x0486, "quc_Latn_GT"}
};

ILCID_POSIX_SUBTABLE(root) {
    {0x00,   "root"}
};

ILCID_POSIX_SUBTABLE(zh) {
    {0x0004, "zh_Hans"},
    {0x7804, "zh"},
    {0x0804, "zh_CN"},
    {0x0804, "zh_Hans_CN"},
    {0x0c04, "zh_Hant_HK"},
    {0x0c04, "zh_HK"},
    {0x0404, "zh_Hant_TW"},
    {0x7c04, "zh_Hant"},
    {0x0404, "zh_TW"},
    {0x30404,"zh_Hant_TW"},     /* Bopomofo order */
    {0x20404,"zh_Hant@collation=stroke"}
};

ILCID_POSIX_SUBTABLE(ca) {
    {0x03,   "ca"},
    {0x0403, "ca_ES"},
    {0x0803, "ca_ES_VALENCIA"}
};

/* This must be static and grouped by LCID. */
constexpr ILcidPosixMap gPosixIDmap[] = {
    ILCID_POSIX_MAP(af),    /*  af  Afrikaans                 0x36 */
    ILCID_POSIX_MAP(ca),
    ILCID_POSIX_MAP(de),    /*  de  German                    0x07 */
    ILCID_POSIX_MAP(en),    /*  en  English                   0x09 */
    ILCID_POSIX_MAP(en_US_POSIX), /*    invariant             0x7f */
    ILCID_POSIX_MAP(es),
    ILCID_POSIX_MAP(ga),
    ILCID_POSIX_MAP(gd),
    ILCID_POSIX_MAP(he),
/*    ILCID_POSIX_MAP(ber),     ber Berber/Tamazight          0x5f */
    ILCID_POSIX_MAP(iw),
    ILCID_POSIX_MAP(no),
    ILCID_POSIX_MAP(quc),
    ILCID_POSIX_MAP(qut),
    ILCID_POSIX_MAP(root),  /*  root                          0x00 */
    ILCID_POSIX_MAP(zh),
};
"#;

const QUOTES_ENGLISH: [&str; 4] = ["\u{201c}", "\u{201d}", "\u{2018}", "\u{2019}"];
const QUOTES_GERMAN: [&str; 4] = ["\u{201e}", "\u{201c}", "\u{201a}", "\u{2018}"];
const QUOTES_TRADITIONAL: [&str; 4] = ["\u{300c}", "\u{300d}", "\u{300e}", "\u{300f}"];
const QUOTES_PORTUGAL: [&str; 4] = ["\u{ab}", "\u{bb}", "\u{201c}", "\u{201d}"];
const QUOTES_SWISS_FRENCH: [&str; 4] = ["\u{ab}", "\u{bb}", "\u{2039}", "\u{203a}"];

/// The locales of the miniature CLDR whose `delimiters.json` is written, with the marks each resolves to (the JSON of a release holds resolved data).
const LOCALES: [(&str, [&str; 4]); 14] = [
    ("und", QUOTES_ENGLISH),
    ("en", QUOTES_ENGLISH),
    ("en-GB", QUOTES_ENGLISH),
    ("de", QUOTES_GERMAN),
    ("de-CH", QUOTES_GERMAN),
    ("fr-CH", QUOTES_SWISS_FRENCH),
    ("zh", QUOTES_ENGLISH),
    ("zh-Hans", QUOTES_ENGLISH),
    ("zh-Hant", QUOTES_TRADITIONAL),
    ("zh-Hant-HK", QUOTES_TRADITIONAL),
    ("pt", QUOTES_ENGLISH),
    ("pt-PT", QUOTES_PORTUGAL),
    ("pt-AO", QUOTES_PORTUGAL),
    ("en-US-POSIX", QUOTES_ENGLISH),
];

fn delimiters(locale: &str, marks: [&str; 4]) -> String {
    format!(
        r#"{{"main": {{"{locale}": {{"identity": {{"language": "x"}}, "delimiters": {{"quotationStart": "{}", "quotationEnd": "{}", "alternateQuotationStart": "{}", "alternateQuotationEnd": "{}"}}}}}}}}"#,
        marks[0], marks[1], marks[2], marks[3]
    )
}

fn names(kind: &str, entries: &[(&str, &str)]) -> String {
    let body: Vec<String> = entries.iter().map(|(k, v)| format!("\"{k}\": \"{v}\"")).collect();
    format!(r#"{{"main": {{"en": {{"localeDisplayNames": {{"{kind}": {{{}}}}}}}}}}}"#, body.join(", "))
}

const LIKELY: &str = r#"{"supplemental": {"version": {"_unicodeVersion": "16.0.0", "_cldrVersion": "49"}, "likelySubtags": {
    "und": "en-Latn-US", "en": "en-Latn-US", "de": "de-Latn-DE", "zh": "zh-Hans-CN", "zh-TW": "zh-Hant-TW", "zh-HK": "zh-Hant-HK", "zh-SG": "zh-Hans-SG",
    "pt": "pt-Latn-BR", "pa": "pa-Guru-IN", "pa-PK": "pa-Arab-PK", "ar": "ar-Arab-EG", "ja": "ja-Jpan-JP", "xx": "xx-Latn-ZZ", "und-Latn": "en-Latn-US",
    "zh-Hant": "zh-Hant-TW", "en-FI": "en-Latn-FI"}}}"#;

fn write_cldr(dir: &TempDir, version: &str, license: &str) {
    dir.write("cldr-core/package.json", &format!(r#"{{"name": "cldr-core", "version": "{version}", "license": "{license}", "cldrVersion": "49"}}"#));
    dir.write("LICENSE", "UNICODE LICENSE V3\n");
    dir.write("cldr-core/supplemental/likelySubtags.json", LIKELY);
    let languages = [
        ("de", "German"),
        ("en", "English"),
        ("en-GB", "British English"),
        ("en-GB-alt-short", "UK English"),
        ("ar-001", "Modern Standard Arabic"),
        ("ga", "Irish"),
        ("gd", "Scottish Gaelic"),
        ("he", "Hebrew"),
        ("ca", "Catalan"),
        ("af", "Afrikaans"),
        ("es", "Spanish"),
        ("no", "Norwegian"),
        ("nb", "Norwegian Bokm\u{e5}l"),
        ("nn", "Norwegian Nynorsk"),
        ("quc", "K\u{2019}iche\u{2019}"),
        ("pa", "Punjabi"),
        ("pt", "Portuguese"),
        ("zh", "Chinese"),
        ("ar", "Arabic"),
        ("ja", "Japanese"),
        ("fr", "French"),
    ];
    let scripts =
        [("Latn", "Latin"), ("Hans", "Simplified"), ("Hant", "Traditional"), ("Hans-alt-stand-alone", "Simplified Han"), ("Arab", "Arabic")];
    let territories = [
        ("AS", "American Samoa"),
        ("AT", "Austria"),
        ("CN", "China"),
        ("CO", "Colombia"),
        ("DE", "Germany"),
        ("ES", "Spain"),
        ("GB", "United Kingdom"),
        ("GB-alt-short", "UK"),
        ("GT", "Guatemala"),
        ("HK", "Hong Kong SAR China"),
        ("IE", "Ireland"),
        ("IL", "Israel"),
        ("NO", "Norway"),
        ("TW", "Taiwan"),
        ("US", "United States"),
        ("ZA", "South Africa"),
        ("029", "Caribbean"),
    ];
    dir.write("cldr-localenames-full/main/en/languages.json", &names("languages", &languages));
    dir.write("cldr-localenames-full/main/en/scripts.json", &names("scripts", &scripts));
    dir.write("cldr-localenames-full/main/en/territories.json", &names("territories", &territories));
    dir.write("cldr-localenames-full/main/en/variants.json", &names("variants", &[("VALENCIA", "Valencian"), ("POSIX", "Computer")]));
    dir.write(
        "cldr-localenames-full/main/en/localeDisplayNames.json",
        r#"{"main": {"en": {"localeDisplayNames": {"localeDisplayPattern": {"localePattern": "{0} ({1})", "localeSeparator": "{0}, {1}", "localeKeyTypePattern": "{0}: {1}"}}}}}"#,
    );
    for (locale, marks) in LOCALES {
        dir.write(&format!("cldr-misc-full/main/{locale}/delimiters.json"), &delimiters(locale, marks));
    }
}

fn write_icu(dir: &TempDir, locmap: &str) {
    dir.write("locmap.cpp", locmap);
    dir.write("locmap.h", LOCMAP_H);
    dir.write("LICENSE", ICU_LICENSE);
}

fn sha256(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

struct Fixture {
    cldr: TempDir,
    icu: TempDir,
    sha: String,
}

impl Fixture {
    fn new() -> Self {
        Self::with(LOCMAP_CPP, "49.0.0", "Unicode-3.0")
    }

    fn with(locmap: &str, cldr_version: &str, cldr_license: &str) -> Self {
        let (cldr, icu) = (TempDir::new("langdata-cldr"), TempDir::new("langdata-icu"));
        write_cldr(&cldr, cldr_version, cldr_license);
        write_icu(&icu, locmap);
        Fixture { cldr, icu, sha: sha256(locmap) }
    }

    fn pins(&self) -> Pins<'_> {
        Pins { icu_release: "release-79.1", locmap_sha256: &self.sha, cldr_release: "49.0.0" }
    }

    fn generate(&self) -> Result<langdata::Files, String> {
        generate(self.cldr.path(), self.icu.path(), &self.pins())
    }

    fn files(&self) -> langdata::Files {
        self.generate().unwrap_or_else(|e| panic!("the miniature sources should generate: {e}"))
    }
}

/// The table's rows and declarations, without the header's comments (which say where the data came from, in words of their own).
fn body(text: &str) -> String {
    text.lines().filter(|line| !line.starts_with("//")).collect::<Vec<_>>().join("\n")
}

fn has_row(table: &str, row: &str) -> bool {
    table.lines().any(|line| line.trim() == row)
}

// ---- the LCID table -----------------------------------------------------------------------------------------------------------------

#[test]
fn a_number_gives_the_tag_icu_lists_first_for_it() {
    // ICU's `uprv_convertToPosix`: the subtable of the number's language ID (its low ten bits), the first element with the number. Naive: the last element, or the
    // tag of a later subtable that lists the number again.
    let lcid = Fixture::new().files().lcid;
    for row in [
        "(0x0409, \"en-US\"),",
        "(0x0809, \"en-GB\"),",
        "(0x0c07, \"de-AT\"),",
        "(0x0009, \"en\"),",
        "(0x0007, \"de\"),",
        "(0x0004, \"zh-Hans\"),",
        "(0x0804, \"zh-CN\"),",
        "(0x0404, \"zh-Hant-TW\"),",
        "(0x0c04, \"zh-Hant-HK\"),",
        "(0x040d, \"he-IL\"),",
        "(0x0436, \"af-ZA\"),",
        "(0x0414, \"nb-NO\"),",
        "(0x0814, \"nn-NO\"),",
        "(0x0803, \"ca-ES-valencia\"),",
    ] {
        assert!(has_row(&lcid, row), "{row}\n{lcid}");
    }
    // `iw_IL` lists 0x040d again in a later subtable of the same language ID: ICU never reaches it.
    assert!(!lcid.contains("iw-IL"), "{lcid}");
}

#[test]
fn a_sort_variant_reads_as_its_language_and_sort_orders_past_sixteen_bits_are_left_out() {
    // 0x040a is `es_ES@collation=traditional`: the sort order is no part of the language. 0x10407 and the like are a sort order stacked on a language ID, so they
    // are no language IDs (a `LangID` is 16 bits). Naive: the keyword kept in the tag, or the number kept.
    let lcid = Fixture::new().files().lcid;
    assert!(has_row(&lcid, "(0x040a, \"es-ES\"),"), "{lcid}");
    assert!(has_row(&lcid, "(0x0c0a, \"es-ES\"),"), "{lcid}");
    let rows = body(&lcid);
    assert!(!rows.contains('@') && !rows.contains("collation"), "{lcid}");
    for number in ["0x10407", "0x30404", "0x20404"] {
        assert!(!rows.contains(number), "{number} is no language ID\n{lcid}");
    }
}

#[test]
fn the_root_the_invariant_locale_and_ids_that_are_no_tag_are_left_out() {
    // `root` (0x00) and `en_US_POSIX` (0x7f) are no languages; `no_NO_NY` is no well-formed tag (`NY` is no variant). Naive: every id of the file.
    let lcid = Fixture::new().files().lcid;
    let rows = body(&lcid).to_lowercase();
    for text in ["root", "posix", "0x007f", "0x0000", "no-no-ny"] {
        assert!(!rows.contains(text), "`{text}` is in the table\n{lcid}");
    }
    // The tag of an alias that is a tag is kept for the other direction.
    assert!(has_row(&lcid, "(\"en-AS\", 0x0409),"), "{lcid}");
    assert!(has_row(&lcid, "(\"no-NO\", 0x0414),"), "{lcid}");
}

#[test]
fn an_id_cldr_has_no_name_for_is_left_out_and_the_next_listed_for_the_number_is_taken() {
    // `qut` is a retired code CLDR has no English name for; ICU lists 0x0486 as `qut_GT` first and `quc_Latn_GT` after it. Naive: the number lost, or `qut-GT` kept
    // as a tag with no name. `iw` (the old code for Hebrew) is unnamed too, and its tags are not in the table.
    let lcid = Fixture::new().files().lcid;
    assert!(has_row(&lcid, "(0x0486, \"quc-Latn-GT\"),"), "{lcid}");
    let rows = body(&lcid);
    assert!(!rows.contains("qut") && !rows.contains("iw"), "{lcid}");
}

#[test]
fn a_tag_with_a_region_cldr_does_not_name_is_paired_but_not_offered() {
    // `en_ZW` has no name here (as `sr_Cyrl_CS`, Serbia and Montenegro, has none in CLDR 49): a file made for it must still read, so its number and tag are paired;
    // but a person is not offered a tag whose name would hold the bare code `ZW`.
    let lcid = Fixture::new().files().lcid;
    assert!(has_row(&lcid, "(0x3009, \"en-ZW\"),"), "{lcid}");
    assert!(has_row(&lcid, "(\"en-ZW\", 0x3009),"), "{lcid}");
    let known: Vec<&str> = section(&lcid, "KNOWN").collect();
    assert!(known.contains(&"\"en-US\","), "{known:?}");
    assert!(!known.iter().any(|row| row.contains("en-ZW")), "{known:?}");
    // The language is what must be named: `iw` and `qut` have no name, so none of their tags is paired.
    assert!(!body(&lcid).contains("iw") && !body(&lcid).contains("qut"), "{lcid}");
}

#[test]
fn a_tag_is_looked_up_in_the_subtable_of_its_own_language_first() {
    // The Irish subtable lists `gd_GB` as 0x043c (ICU's slip) and the Scottish Gaelic one as 0x0491: the tag gets 0x0491. And a tag two subtables list the same way
    // gets the number once. The number 0x043c still reads, as ICU reads it, from the subtable of its own language ID (`ga`).
    let lcid = Fixture::new().files().lcid;
    assert!(has_row(&lcid, "(\"gd-GB\", 0x0491),"), "{lcid}");
    assert!(!has_row(&lcid, "(\"gd-GB\", 0x043c),"), "{lcid}");
    assert!(has_row(&lcid, "(0x043c, \"gd-GB\"),"), "{lcid}");
    assert_eq!(lcid.matches("(\"gd-GB\"").count(), 1, "{lcid}");
    // A tag spelled two ways keeps both spellings for the number and the first for the way back.
    assert!(has_row(&lcid, "(\"zh-CN\", 0x0804),"), "{lcid}");
    assert!(has_row(&lcid, "(\"zh-Hans-CN\", 0x0804),"), "{lcid}");
    assert!(has_row(&lcid, "(\"zh-TW\", 0x0404),"), "{lcid}");
    assert!(has_row(&lcid, "(\"es-ES\", 0x0c0a),"), "{lcid}");
}

#[test]
fn the_tables_are_sorted_for_a_binary_search_and_the_known_tags_are_listed_once() {
    let lcid = Fixture::new().files().lcid;
    let from: Vec<&str> = section(&lcid, "FROM_LCID").collect();
    let to: Vec<&str> = section(&lcid, "TO_LCID").collect();
    let known: Vec<&str> = section(&lcid, "KNOWN").collect();
    assert!(from.len() > 10 && to.len() > from.len() / 2 && known.len() > 10, "{from:?} {to:?} {known:?}");
    for rows in [&from, &to, &known] {
        let mut sorted = rows.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(&sorted, rows, "sorted and unique:\n{lcid}");
    }
    // The tags of KNOWN are tags FROM_LCID gives, once each (all of them but the ones whose region CLDR does not name: `en-ZW`).
    let mut tags: Vec<String> = from.iter().filter_map(|row| row.split('"').nth(1)).filter(|tag| *tag != "en-ZW").map(str::to_owned).collect();
    tags.sort();
    tags.dedup();
    let listed: Vec<String> = known.iter().filter_map(|row| row.split('"').nth(1)).map(str::to_owned).collect();
    assert_eq!(listed, tags);
}

/// The rows of one table of the generated `lcid.rs`, in file order: from the line that declares it to the line `];`.
fn section<'a>(text: &'a str, name: &str) -> impl Iterator<Item = &'a str> {
    text.lines()
        .skip_while(move |line| !(line.contains("const") && line.contains(name)))
        .skip(1)
        .take_while(|line| line.trim() != "];")
        .map(str::trim)
        .filter(|line| line.starts_with('(') || line.starts_with('"'))
}

// ---- names, marks, scripts ----------------------------------------------------------------------------------------------------------

#[test]
fn the_names_are_the_plain_codes_with_cldrs_own_pattern() {
    // Dialect names (`en-GB`) and the alternates (`-alt-short`) are left out: a name is built from a language, a script and a region, with CLDR's pattern.
    // Variants are keyed in lower case, as a tag writes them.
    let names = Fixture::new().files().names;
    assert!(names.contains("pub(super) const LOCALE_PATTERN: &str = \"{0} ({1})\";"), "{names}");
    assert!(names.contains("pub(super) const LOCALE_SEPARATOR: &str = \"{0}, {1}\";"), "{names}");
    for row in [
        "(\"en\", \"English\"),",
        "(\"de\", \"German\"),",
        "(\"Latn\", \"Latin\"),",
        "(\"Hans\", \"Simplified\"),",
        "(\"GB\", \"United Kingdom\"),",
        "(\"029\", \"Caribbean\"),",
        "(\"valencia\", \"Valencian\"),",
        "(\"posix\", \"Computer\"),",
    ] {
        assert!(has_row(&names, row), "{row}\n{names}");
    }
    let rows = body(&names);
    for left_out in ["British", "en-GB", "alt-short", "UK", "ar-001", "Simplified Han", "Modern Standard"] {
        assert!(!rows.contains(left_out), "`{left_out}` is in the names\n{names}");
    }
    // The language `nb` is named with a letter outside ASCII: kept as the character, which is a letter.
    assert!(names.contains("Norwegian Bokm\u{e5}l"), "{names}");
}

#[test]
fn the_marks_hold_only_what_differs_from_the_parent_and_the_likely_regions_of_a_script() {
    // The JSON of a release is resolved, so a locale that says what its parent says is redundant: `en`, `en-GB`, `de-CH` and `zh` are left out (the root and the
    // parent say it). `zh-TW` is `zh-Hant-TW` by the likely subtags, which no locale of the release names: it is added as a key, so that the chain `zh-TW`, `zh`
    // does not give Taiwan the Simplified marks. `zh-SG`'s script gives the marks `zh` has already.
    let quotes = Fixture::new().files().quotes;
    assert!(has_row(&quotes, "(\"de\", [\"\u{201e}\", \"\u{201c}\", \"\u{201a}\", \"\u{2018}\"]),"), "{quotes}");
    assert!(has_row(&quotes, "(\"zh-Hant\", [\"\u{300c}\", \"\u{300d}\", \"\u{300e}\", \"\u{300f}\"]),"), "{quotes}");
    assert!(has_row(&quotes, "(\"zh-TW\", [\"\u{300c}\", \"\u{300d}\", \"\u{300e}\", \"\u{300f}\"]),"), "{quotes}");
    assert!(has_row(&quotes, "(\"zh-HK\", [\"\u{300c}\", \"\u{300d}\", \"\u{300e}\", \"\u{300f}\"]),"), "{quotes}");
    assert!(has_row(&quotes, "(\"pt-PT\", [\"\u{ab}\", \"\u{bb}\", \"\u{201c}\", \"\u{201d}\"]),"), "{quotes}");
    assert!(has_row(&quotes, "(\"pt-AO\", [\"\u{ab}\", \"\u{bb}\", \"\u{201c}\", \"\u{201d}\"]),"), "{quotes}");
    assert!(has_row(&quotes, "(\"fr-CH\", [\"\u{ab}\", \"\u{bb}\", \"\u{2039}\", \"\u{203a}\"]),"), "{quotes}");
    for left_out in ["(\"en\"", "(\"en-GB\"", "(\"de-CH\"", "(\"zh\"", "(\"zh-Hans\"", "(\"zh-SG\"", "(\"zh-Hant-HK\"", "(\"und\"", "(\"pt\""] {
        assert!(!quotes.contains(left_out), "{left_out} is redundant\n{quotes}");
    }
    // The variant of a locale id is lower case, as a tag writes it (the marks equal the root's, so no row; the id must not be refused).
    assert!(quotes.contains("pub(super) const ROOT: [&str; 4] = [\"\u{201c}\", \"\u{201d}\", \"\u{2018}\", \"\u{2019}\"];"), "{quotes}");
}

#[test]
fn the_scripts_hold_the_languages_cldr_names_and_the_regions_that_change_the_script() {
    // A language CLDR gives no English name (`xx`) is left out, with `und`; `pa-PK` is Arabic where `pa` is Gurmukhi, and `zh-TW` Traditional where `zh` is
    // Simplified; `zh-SG` and `en-FI` change nothing, and `zh-Hant` names its script itself.
    let scripts = Fixture::new().files().scripts;
    for row in [
        "(\"ar\", \"Arab\"),",
        "(\"de\", \"Latn\"),",
        "(\"en\", \"Latn\"),",
        "(\"ja\", \"Jpan\"),",
        "(\"pa\", \"Guru\"),",
        "(\"pa-PK\", \"Arab\"),",
        "(\"zh\", \"Hans\"),",
        "(\"zh-HK\", \"Hant\"),",
        "(\"zh-TW\", \"Hant\"),",
    ] {
        assert!(has_row(&scripts, row), "{row}\n{scripts}");
    }
    for left_out in ["\"xx\"", "\"und\"", "\"zh-SG\"", "\"en-FI\"", "\"zh-Hant\"", "\"und-Latn\""] {
        assert!(!scripts.contains(left_out), "{left_out}\n{scripts}");
    }
}

// ---- headers, determinism, bytes ----------------------------------------------------------------------------------------------------

#[test]
fn every_table_names_its_sources_releases_and_licence() {
    let fixture = Fixture::new();
    let files = fixture.files();
    for (name, text) in [("lcid", &files.lcid), ("names", &files.names), ("quotes", &files.quotes), ("scripts", &files.scripts)] {
        let first = text.lines().next().unwrap_or("");
        assert!(first.starts_with("// @generated by `cargo xtask langdata`"), "{name}: {first}");
        for needed in ["Unicode-3.0", "CLDR 49.0.0", "cldr-json", "NOTICE"] {
            assert!(text.contains(needed), "{name} lacks `{needed}`\n{}", text.lines().take(14).collect::<Vec<_>>().join("\n"));
        }
        assert!(text.contains("\n//!"), "{name} has a module doc");
    }
    for needed in ["ICU4C", "release-79.1", fixture.sha.as_str(), "locmap.cpp"] {
        assert!(files.lcid.contains(needed), "lcid lacks `{needed}`");
    }
    // The table of numbers never copies a table of the reference app's documents: the header says where it came from, and only that.
    assert!(!files.lcid.to_lowercase().contains("microsoft") && !files.lcid.contains("MS-LCID") && !files.lcid.contains("MS-VSDX"), "{}", files.lcid);
}

#[test]
fn two_runs_give_the_same_bytes_whatever_order_the_files_were_made_in() {
    // Determinism: the order of a directory listing and of a hash map must not reach the output.
    let first = Fixture::new();
    let second = Fixture::new();
    let (a, b) = (first.files(), second.files());
    assert_eq!((&a.lcid, &a.names, &a.quotes, &a.scripts), (&b.lcid, &b.names, &b.quotes, &b.scripts));
    let again = first.files();
    assert_eq!(a.lcid, again.lcid);
    assert_eq!(a.quotes, again.quotes);
    // A different locale directory made first, in a folder of its own, gives the same marks.
    let reordered = TempDir::new("langdata-cldr-reordered");
    write_cldr(&reordered, "49.0.0", "Unicode-3.0");
    for (locale, marks) in LOCALES.iter().rev() {
        reordered.write(&format!("cldr-misc-full/main/{locale}/delimiters.json"), &delimiters(locale, *marks));
    }
    let third = generate(reordered.path(), first.icu.path(), &first.pins()).unwrap();
    assert_eq!(third.quotes, a.quotes);
}

#[test]
fn the_files_are_text_that_rustfmt_leaves_alone_and_that_holds_no_control_or_bidirectional_character() {
    // A table is skipped by rustfmt (so the generator needs no formatter), and no invisible character is written as itself: it is an escape.
    let files = Fixture::new().files();
    for (name, text) in [("lcid", &files.lcid), ("names", &files.names), ("quotes", &files.quotes), ("scripts", &files.scripts)] {
        assert!(text.contains("#[rustfmt::skip]"), "{name}");
        assert!(text.ends_with('\n') && !text.contains('\r') && !text.contains('\t'), "{name}");
        for c in text.chars() {
            let invisible =
                c.is_control() && c != '\n' || matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}');
            assert!(!invisible, "{name} holds U+{:04X} as itself", c as u32);
        }
    }
}

#[test]
fn an_invisible_character_of_the_data_is_written_as_an_escape() {
    // CLDR's French and Arabic data hold no-break and directional marks in places. The generator never writes one as itself (a bidirectional character in a source
    // file is refused by rustc's own lint); here a name with a left-to-right mark and a no-break space.
    let fixture = Fixture::new();
    fixture.cldr.write("cldr-localenames-full/main/en/languages.json", &names("languages", &[("en", "English"), ("de", "German\u{200e}\u{a0}x")]));
    let text = fixture.files().names;
    assert!(text.contains("German\\u{200e}\\u{a0}x"), "{text}");
}

// ---- refusals -----------------------------------------------------------------------------------------------------------------------

#[test]
fn a_cldr_release_other_than_the_pinned_one_is_refused_in_words() {
    // The tables must come from the release craft-i18n's data came from; a download of another release must not be used by mistake.
    let fixture = Fixture::with(LOCMAP_CPP, "48.2.1", "Unicode-3.0");
    let error = fixture.generate().unwrap_err();
    assert!(error.contains("48.2.1") && error.contains("49.0.0"), "{error}");
}

#[test]
fn a_source_that_is_not_under_the_unicode_licence_is_refused() {
    let cldr = Fixture::with(LOCMAP_CPP, "49.0.0", "GPL-3.0-only").generate().unwrap_err();
    assert!(cldr.contains("Unicode-3.0") && cldr.contains("GPL-3.0-only"), "{cldr}");
    let fixture = Fixture::new();
    fixture.icu.write("LICENSE", "Some other licence\n");
    let icu = fixture.generate().unwrap_err();
    assert!(icu.contains("Unicode-3.0") && icu.contains("LICENSE"), "{icu}");
}

#[test]
fn a_locmap_other_than_the_pinned_file_is_refused_with_both_hashes() {
    let fixture = Fixture::new();
    fixture.icu.write("locmap.cpp", &format!("{LOCMAP_CPP}\n// edited\n"));
    let error = fixture.generate().unwrap_err();
    assert!(error.contains("release-79.1") && error.contains("SHA-256") && error.contains(&fixture.sha), "{error}");
}

#[test]
fn a_locmap_header_that_masks_the_language_differently_is_refused() {
    // The generator reads the low ten bits as the language ID because the header's macro does; another macro would change the tables.
    let fixture = Fixture::new();
    fixture.icu.write("locmap.h", &LOCMAP_H.replace("0x03FF", "0x01FF"));
    let error = fixture.generate().unwrap_err();
    assert!(error.contains("LANGUAGE_LCID") && error.contains("locmap.h"), "{error}");
}

#[test]
fn a_malformed_subtable_is_refused_with_its_line() {
    let broken = LOCMAP_CPP.replace("    {0x0c07, \"de_AT\"},", "    {0x0c07  \"de_AT\"},");
    let line = LOCMAP_CPP.lines().position(|line| line.contains("de_AT")).unwrap() + 1;
    let error = Fixture::with(&broken, "49.0.0", "Unicode-3.0").generate().unwrap_err();
    assert!(error.contains("locmap.cpp") && error.contains(&format!("line {line}")), "{error}");
    let unterminated = LOCMAP_CPP.replace("    {0x0809, \"en_GB\"},", "    {0x0809, \"en_GB},");
    let error = Fixture::with(&unterminated, "49.0.0", "Unicode-3.0").generate().unwrap_err();
    assert!(error.contains("locmap.cpp") && error.contains("line"), "{error}");
    let number = LOCMAP_CPP.replace("{0x0c07, \"de_AT\"}", "{0xZZ, \"de_AT\"}");
    assert!(Fixture::with(&number, "49.0.0", "Unicode-3.0").generate().unwrap_err().contains("0xZZ"));
}

#[test]
fn a_map_entry_with_no_subtable_is_refused_and_a_file_with_no_table_too() {
    let undefined = LOCMAP_CPP.replace("    ILCID_POSIX_MAP(zh),\n", "    ILCID_POSIX_MAP(zh),\n    ILCID_POSIX_MAP(nowhere),\n");
    let error = Fixture::with(&undefined, "49.0.0", "Unicode-3.0").generate().unwrap_err();
    assert!(error.contains("nowhere"), "{error}");
    let empty = "// nothing here\n";
    let error = Fixture::with(empty, "49.0.0", "Unicode-3.0").generate().unwrap_err();
    assert!(error.contains("locmap.cpp") && error.contains("no"), "{error}");
}

#[test]
fn a_missing_or_broken_cldr_file_is_refused_naming_it() {
    let fixture = Fixture::new();
    std::fs::remove_file(fixture.cldr.path().join("cldr-localenames-full/main/en/scripts.json")).unwrap();
    let error = fixture.generate().unwrap_err();
    assert!(error.contains("scripts.json"), "{error}");
    let fixture = Fixture::new();
    fixture.cldr.write("cldr-core/supplemental/likelySubtags.json", "{ not json");
    assert!(fixture.generate().unwrap_err().contains("likelySubtags.json"));
    let fixture = Fixture::new();
    fixture.cldr.write("cldr-misc-full/main/de/delimiters.json", &delimiters("de", ["", "x", "y", "z"]));
    let error = fixture.generate().unwrap_err();
    assert!(error.contains("de") && error.contains("delimiters.json"), "{error}");
    let fixture = Fixture::new();
    fixture.cldr.write("cldr-misc-full/main/de/delimiters.json", &delimiters("de", ["abcdefghij", "x", "y", "z"]));
    assert!(fixture.generate().unwrap_err().contains("delimiters.json"));
    let fixture = Fixture::new();
    std::fs::remove_dir_all(fixture.cldr.path().join("cldr-misc-full/main/und")).unwrap();
    assert!(fixture.generate().unwrap_err().contains("und"));
}

#[test]
fn a_locale_that_is_no_tag_is_refused() {
    let fixture = Fixture::new();
    fixture.cldr.write("cldr-misc-full/main/not_a_tag/delimiters.json", &delimiters("not_a_tag", QUOTES_ENGLISH));
    let error = fixture.generate().unwrap_err();
    assert!(error.contains("not_a_tag"), "{error}");
}

#[test]
fn craft_i18ns_record_must_name_the_same_cldr_release() {
    // `crates/i18n/data/source.txt` of the craft-libs revision DiaCraft pins: the tables and craft-i18n's own data are one CLDR release.
    let pins = Pins { icu_release: "release-79.1", locmap_sha256: "x", cldr_release: "49.0.0" };
    assert!(check_i18n_source("icu4x-datagen 2.4.0\ncldr 49.0.0\n", &pins).is_ok());
    let error = check_i18n_source("icu4x-datagen 2.3.0\ncldr 48.2.1\n", &pins).unwrap_err();
    assert!(error.contains("48.2.1") && error.contains("49.0.0"), "{error}");
    let error = check_i18n_source("icu4x-datagen 2.4.0\n", &pins).unwrap_err();
    assert!(error.contains("cldr"), "{error}");
}

// ---- the command --------------------------------------------------------------------------------------------------------------------

/// A made-up checkout: the folder the tables are written to.
fn checkout() -> TempDir {
    let root = TempDir::new("langdata-root");
    std::fs::create_dir_all(root.path().join("crates/doc/src/lang")).unwrap();
    root
}

fn args<'a>(fixture: &'a Fixture, extra: &[&'a str]) -> Vec<&'a str> {
    let mut all = vec!["--cldr", fixture.cldr.path().to_str().unwrap(), "--icu", fixture.icu.path().to_str().unwrap()];
    all.extend_from_slice(extra);
    all
}

#[test]
fn the_command_writes_the_four_tables_and_check_holds_them_to_the_sources() {
    let (fixture, root) = (Fixture::new(), checkout());
    let pins = fixture.pins();
    langdata::run_with(root.path(), &args(&fixture, &[]), &pins).unwrap();
    for file in ["lcid.rs", "names.rs", "quotes.rs", "scripts.rs"] {
        let written = std::fs::read_to_string(root.path().join("crates/doc/src/lang").join(file)).unwrap();
        assert!(written.starts_with("// @generated by `cargo xtask langdata`"), "{file}");
    }
    // Written bytes are the generated bytes.
    let files = fixture.files();
    assert_eq!(std::fs::read_to_string(root.path().join("crates/doc/src/lang/lcid.rs")).unwrap(), files.lcid);
    // `--check` passes on what was written and fails naming the file when one is edited by hand.
    langdata::run_with(root.path(), &args(&fixture, &["--check"]), &pins).unwrap();
    let quotes = root.path().join("crates/doc/src/lang/quotes.rs");
    let edited = format!("{}// hand edit\n", std::fs::read_to_string(&quotes).unwrap());
    std::fs::write(&quotes, edited).unwrap();
    let error = langdata::run_with(root.path(), &args(&fixture, &["--check"]), &pins).unwrap_err();
    assert!(error.contains("quotes.rs") && !error.contains("lcid.rs"), "{error}");
    // `--check` writes nothing.
    assert!(std::fs::read_to_string(&quotes).unwrap().ends_with("// hand edit\n"));
}

#[test]
fn the_command_refuses_a_missing_option_an_unknown_one_and_a_wrong_i18n_record() {
    let (fixture, root) = (Fixture::new(), checkout());
    let pins = fixture.pins();
    let missing = langdata::run_with(root.path(), &["--cldr", fixture.cldr.path().to_str().unwrap()], &pins).unwrap_err();
    assert!(missing.contains("--icu"), "{missing}");
    let missing = langdata::run_with(root.path(), &["--icu", fixture.icu.path().to_str().unwrap()], &pins).unwrap_err();
    assert!(missing.contains("--cldr"), "{missing}");
    let unknown = langdata::run_with(root.path(), &args(&fixture, &["--fetch"]), &pins).unwrap_err();
    assert!(unknown.contains("--fetch"), "{unknown}");
    // A folder that is not there: the error names it.
    let gone = langdata::run_with(root.path(), &["--cldr", "/nonexistent/cldr", "--icu", fixture.icu.path().to_str().unwrap()], &pins).unwrap_err();
    assert!(gone.contains("/nonexistent/cldr"), "{gone}");
    // `--i18n-source FILE`: the record of craft-i18n's data.
    let record = TempDir::new("langdata-record");
    record.write("source.txt", "icu4x-datagen 2.4.0\ncldr 48.2.1\n");
    let path = record.path().join("source.txt");
    let wrong = langdata::run_with(root.path(), &args(&fixture, &["--i18n-source", path.to_str().unwrap()]), &pins).unwrap_err();
    assert!(wrong.contains("48.2.1"), "{wrong}");
    record.write("source.txt", "icu4x-datagen 2.4.0\ncldr 49.0.0\n");
    langdata::run_with(root.path(), &args(&fixture, &["--i18n-source", path.to_str().unwrap()]), &pins).unwrap();
    // Nothing was written by the refusals that came before the last run.
    let none = checkout();
    assert!(langdata::run_with(none.path(), &args(&fixture, &["--fetch"]), &pins).is_err());
    assert!(std::fs::read_dir(none.path().join("crates/doc/src/lang")).unwrap().next().is_none());
}

#[test]
fn the_command_fetches_nothing_and_has_no_network_option() {
    // No network at build or run time: the sources are folders the person made. The usage says so and no flag names a URL.
    let usage = langdata::USAGE;
    assert!(usage.contains("--cldr") && usage.contains("--icu") && usage.contains("--check") && usage.contains("--i18n-source"), "{usage}");
    assert!(!usage.contains("http") && !usage.to_lowercase().contains("download"), "{usage}");
}

// ---- the committed tables -----------------------------------------------------------------------------------------------------------

#[test]
fn the_committed_tables_are_generated_files_that_name_the_pinned_sources() {
    // The check that needs no download: the headers of the four committed files name the pins of the generator, so that neither is changed without the other.
    let folder = xtask::root().join("crates/doc/src/lang");
    for file in ["lcid.rs", "names.rs", "quotes.rs", "scripts.rs"] {
        let text = std::fs::read_to_string(folder.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert!(text.starts_with("// @generated by `cargo xtask langdata`"), "{file}");
        assert!(
            text.contains(&format!("CLDR {}", PINS.cldr_release)),
            "{file} names another CLDR release than the generator's {}",
            PINS.cldr_release
        );
        assert!(text.contains("Unicode-3.0"), "{file}");
    }
    let lcid = std::fs::read_to_string(folder.join("lcid.rs")).unwrap();
    assert!(lcid.contains(PINS.icu_release) && lcid.contains(PINS.locmap_sha256), "lcid.rs names another ICU file than the generator's pin");
    assert_eq!(PINS.locmap_sha256.len(), 64);
    assert!(PINS.locmap_sha256.bytes().all(|b| b.is_ascii_hexdigit()));
}

#[test]
#[ignore = "needs the CLDR and ICU downloads: LANGDATA_CLDR and LANGDATA_ICU name their folders (docs/development.md)"]
fn regenerating_from_the_real_sources_gives_the_committed_bytes() {
    let (Ok(cldr), Ok(icu)) = (std::env::var("LANGDATA_CLDR"), std::env::var("LANGDATA_ICU")) else {
        panic!("set LANGDATA_CLDR and LANGDATA_ICU to the folders of the downloads");
    };
    let files = generate(std::path::Path::new(&cldr), std::path::Path::new(&icu), &PINS).unwrap();
    let again = generate(std::path::Path::new(&cldr), std::path::Path::new(&icu), &PINS).unwrap();
    assert_eq!(files.lcid, again.lcid, "two runs");
    assert_eq!(files.quotes, again.quotes, "two runs");
    let folder = xtask::root().join("crates/doc/src/lang");
    for (file, text) in [("lcid.rs", &files.lcid), ("names.rs", &files.names), ("quotes.rs", &files.quotes), ("scripts.rs", &files.scripts)] {
        let committed = std::fs::read_to_string(folder.join(file)).unwrap();
        assert!(&committed == text, "{file} is not what the sources give: run `cargo xtask langdata` and commit");
    }
}
