//! Language tags and the effective language of a run (design 5.1 and 5.4).
//!
//! The table of design 5.1 is tested row by row through [`effective`]; the order of the document's language through [`document_language`]. Every test names the
//! wrong answer it catches. A number reads as the tag the table of LCID numbers pairs with it (`lang_data.rs`), 0 is no language, and any other number is the document's.

use std::sync::Arc;

use diacraft_doc::lang::{self, Effective, MAX_TAG_BYTES, Tag, TagError, document_language, effective, is_no_language, parse};
use diacraft_doc::{Cell, CellAddr, Document, Owner, Resolver, RowKey, ShapeRef, SheetRef, Value};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use proptest::prelude::*;

fn tag(text: &str) -> Tag {
    parse(text).unwrap_or_else(|e| panic!("`{text}` should be a well-formed tag: {e}"))
}

fn language(text: &str) -> Effective {
    Effective::Language(tag(text))
}

// ---- canonical case ---------------------------------------------------------------------------------------------------------------

#[test]
fn a_tag_is_kept_in_canonical_case() {
    // RFC 5646 2.1.1: the language lower case, the script title case, the region upper case, everything else lower case. The cell keeps what was written; the Tag
    // is what the proofer and the renderer compare. Naive: the text kept as written, so `en-us` and `en-US` are two languages.
    let rows = [
        ("en-us", "en-US"),
        ("EN-US", "en-US"),
        ("en-US", "en-US"),
        ("en", "en"),
        ("sr-latn-rs", "sr-Latn-RS"),
        ("SR-LATN-RS", "sr-Latn-RS"),
        ("zh-hans-cn", "zh-Hans-CN"),
        ("es-419", "es-419"),
        ("en-FI", "en-FI"),
        ("x-none", "x-none"),
        ("X-NONE", "x-none"),
        ("zxx", "zxx"),
        ("de-de-1996", "de-DE-1996"),
        ("zh-cmn-hans-cn", "zh-cmn-Hans-CN"),
        ("EN-US-U-CA-GREGORY", "en-US-u-ca-gregory"),
        ("en-us-x-Priv-Use", "en-US-x-priv-use"),
        ("pt-br", "pt-BR"),
        ("sr-cyrl", "sr-Cyrl"),
    ];
    for (given, canonical) in rows {
        assert_eq!(tag(given).as_str(), canonical, "`{given}`");
    }
}

#[test]
fn a_grammar_irregular_tag_keeps_its_listed_spelling() {
    // The irregular grandfathered tags of RFC 5646 2.2.8 match no production but their own; they are well formed, in the spelling the RFC lists them in.
    let rows =
        [("I-KLINGON", "i-klingon"), ("en-gb-oed", "en-GB-oed"), ("SGN-be-fr", "sgn-BE-FR"), ("sgn-ch-de", "sgn-CH-DE"), ("i-default", "i-default")];
    for (given, canonical) in rows {
        assert_eq!(tag(given).as_str(), canonical, "`{given}`");
    }
    // The regular ones are langtags already, and take the langtag's case.
    assert_eq!(tag("ZH-MIN-NAN").as_str(), "zh-min-nan");
    assert_eq!(tag("ART-LOJBAN").as_str(), "art-lojban");
    // And a prefix of an irregular one is no tag.
    assert!(parse("i").is_err());
    assert!(parse("en-GB-oe").is_err());
}

#[test]
fn a_tag_names_its_parts() {
    let t = tag("sr-latn-rs");
    assert_eq!((t.primary(), t.script(), t.region()), ("sr", Some("Latn"), Some("RS")));
    let t = tag("en");
    assert_eq!((t.primary(), t.script(), t.region()), ("en", None, None));
    // A region of three digits is a region; a script is four letters.
    let t = tag("es-419");
    assert_eq!((t.primary(), t.script(), t.region()), ("es", None, Some("419")));
    let t = tag("sr-Cyrl");
    assert_eq!((t.primary(), t.script(), t.region()), ("sr", Some("Cyrl"), None));
    // An extended language subtag comes between the language and the script, and is neither.
    let t = tag("zh-cmn-Hans-CN");
    assert_eq!((t.primary(), t.script(), t.region()), ("zh", Some("Hans"), Some("CN")));
    let t = tag("zh-yue-HK");
    assert_eq!((t.primary(), t.script(), t.region()), ("zh", None, Some("HK")));
    // A variant, an extension and private use are none of the three: a four-character variant is not a script (it begins with a digit), and a letter pair after an
    // extension's singleton is no region.
    let t = tag("de-1996");
    assert_eq!((t.primary(), t.script(), t.region()), ("de", None, None));
    let t = tag("en-u-us");
    assert_eq!((t.primary(), t.script(), t.region()), ("en", None, None));
    let t = tag("en-x-latn");
    assert_eq!((t.primary(), t.script(), t.region()), ("en", None, None));
    // The private-use tag of "no language" has the primary subtag `x`.
    assert_eq!(tag("x-none").primary(), "x");
    assert_eq!(tag("en-US").to_string(), "en-US", "a tag is shown as its text");
}

// ---- well-formedness (RFC 5646's syntax) ------------------------------------------------------------------------------------------

#[test]
fn a_tag_that_follows_the_syntax_of_rfc_5646_is_accepted() {
    for text in [
        "en",
        "eng",
        "abcd",
        "abcde",
        "abcdefgh",
        "en-US",
        "en-Latn",
        "en-Latn-US",
        "es-419",
        "de-DE-1996",
        "de-1901-1996",
        "sl-rozaj-biske",
        "sl-IT-nedis",
        "zh-aaa-bbb-ccc",
        "en-a-bb",
        "en-a-bb-cc-dd",
        "en-a-bb-b-cc",
        "en-US-u-ca-gregory-x-priv",
        "x-a",
        "x-none",
        "x-12345678",
        "en-x-a",
        "en-FI",
        "i-klingon",
        "zh-CHS",
        "zh-CHT",
        "hy-Latn-IT-arevela",
    ] {
        assert!(parse(text).is_ok(), "`{text}` is well formed");
    }
}

#[test]
fn a_tag_that_breaks_the_syntax_is_refused() {
    for text in [
        "",
        "e",
        "1",
        "123",
        "en-",
        "-en",
        "en--US",
        "-",
        "--",
        "en_US",
        "en US",
        "en-US ",
        " en-US",
        "abcdefghi",
        // A singleton with nothing after it, and a one-character subtag after one.
        "en-a",
        "en-US-u",
        "en-a-b",
        "en-a-bb-c",
        // `x` begins private use, which needs at least one subtag.
        "x",
        "en-x",
        "en-US-x",
        // The order of the parts: a script twice, a region twice, a region before a script, a variant before a region.
        "en-Latn-Latn",
        "en-US-US",
        "en-US-Latn",
        "en-1996-US",
        // A fourth extended language subtag; an extended language subtag after a language of four letters or more; one after the script.
        "zh-aaa-bbb-ccc-ddd",
        "abcd-abc",
        "abcde-abc",
        "en-Latn-abc",
        // Four letters of a variant: it needs five or a digit first (and after a region it is no script either).
        "en-US-abcd",
        // Characters that are not letters, digits and hyphens.
        "en-US\0",
        "en-U$",
        "ené",
        "日本語",
        "en-\u{FF35}\u{FF33}",
    ] {
        assert!(parse(text).is_err(), "`{text}` is not well formed");
    }
}

#[test]
fn a_tag_is_refused_in_words() {
    // A refusal test asserts the words, not only `is_err`.
    assert_eq!(parse("").unwrap_err().to_string(), "a language tag must not be empty");
    assert_eq!(parse("en_US").unwrap_err().to_string(), "a language tag holds only ASCII letters, digits and hyphens");
    assert_eq!(parse("ené").unwrap_err().to_string(), "a language tag holds only ASCII letters, digits and hyphens");
    assert_eq!(parse("en-US-u").unwrap_err().to_string(), "that is not a well-formed language tag (RFC 5646)");
    assert_eq!(parse("en--US").unwrap_err().to_string(), "that is not a well-formed language tag (RFC 5646)");
    assert_eq!(parse("e").unwrap_err(), TagError::Syntax);
}

/// A well-formed tag of exactly `bytes` bytes: a language of three letters and variants of eight (nine bytes with the hyphen), or one of four letters when the
/// length leaves a byte over.
fn tag_of(bytes: usize) -> String {
    let language = if bytes.saturating_sub(3).is_multiple_of(9) { "eng" } else { "engl" };
    let variants = bytes.saturating_sub(language.len()) / 9;
    let mut text = language.to_owned();
    for _ in 0..variants {
        text.push_str("-abcdefgh");
    }
    assert_eq!(text.len(), bytes, "the builder reaches {bytes} bytes");
    text
}

#[test]
fn a_tag_of_255_bytes_is_accepted_and_one_of_256_is_refused_in_words() {
    assert_eq!(MAX_TAG_BYTES, 255);
    let longest = tag_of(255);
    assert_eq!(parse(&longest).map(|t| t.as_str().len()), Ok(255));
    let past = tag_of(256);
    assert!(past.len() == 256 && past.is_ascii());
    let refused = parse(&past).unwrap_err();
    assert_eq!(refused, TagError::TooLong);
    assert_eq!(refused.to_string(), "a language tag must be at most 255 bytes");
}

#[test]
fn a_huge_text_is_refused_for_its_length_before_it_is_read() {
    // The length is checked first, so a megabyte is one comparison and not a scan: the refusal is the length's even where the text is also not a tag.
    let hyphens = "-".repeat(1 << 20);
    assert_eq!(parse(&hyphens), Err(TagError::TooLong));
    let subtags = "en-abcdefgh".repeat(10_000);
    assert_eq!(parse(&subtags), Err(TagError::TooLong));
    let multibyte = "é".repeat(200);
    assert_eq!(parse(&multibyte), Err(TagError::TooLong), "a text of 400 bytes is too long whatever it holds");
    let multibyte_short = "é".repeat(100);
    assert_eq!(parse(&multibyte_short), Err(TagError::Character));
    let effective_of_huge = effective(Some(&hyphens), &tag("fr-FR"));
    assert_eq!(effective_of_huge, language("fr-FR"), "a refused text is the document's language");
}

// ---- no language ------------------------------------------------------------------------------------------------------------------

#[test]
fn the_words_for_no_language_are_recognised_in_any_case() {
    // Empty, `x-none`, OpenDocument's `zxx` and `none`.
    for text in ["", "x-none", "X-NONE", "X-None", "zxx", "ZXX", "Zxx", "none", "NONE", "None", "nOnE"] {
        assert!(is_no_language(text), "`{text}`");
    }
    // Anything else is a language or an error, not "none": a tag that begins as one, a space, a number.
    for text in ["en-US", "x-none-x", "x-none2", "x", "xnone", "zxx-US", "nonee", "non", " ", " none", "none ", "x_none", "0", "en", "none-none"] {
        assert!(!is_no_language(text), "`{text}`");
    }
}

// ---- the effective language (design 5.1's table) ---------------------------------------------------------------------------------------

#[test]
fn a_well_formed_tag_is_that_language_whether_or_not_any_table_lists_it() {
    let document = tag("tr-TR");
    for text in ["en-US", "de-CH", "sr-Latn-RS", "en-FI", "qq-ZZ", "ja", "x-klingon", "zh-Hans", "es-419", "en-US-u-ca-gregory"] {
        assert_eq!(effective(Some(text), &document), language(text), "`{text}`");
    }
    // In canonical case, whatever was written: `LangID` cells hold what files held.
    assert_eq!(effective(Some("en-us"), &document), language("en-US"));
    assert_eq!(effective(Some("SR-latn-rs"), &document), language("sr-Latn-RS"));
}

#[test]
fn x_none_zxx_empty_and_none_are_no_language() {
    // Naive: checked against the document's language (the first run).
    let document = tag("en-US");
    for text in ["x-none", "X-None", "zxx", "ZXX", "", "none", "None"] {
        assert_eq!(effective(Some(text), &document), Effective::None, "`{text}`");
    }
}

#[test]
fn zero_is_no_language_and_a_number_no_table_pairs_is_the_documents_language() {
    // 0 reads as no language; 65535 and a number the table lacks read as the document's language. (The numbers the table pairs read as their tags: `lang_data.rs`.)
    let document = tag("tr-TR");
    for text in ["0", "00", "0000000000000000000000"] {
        assert_eq!(effective(Some(text), &document), Effective::None, "`{text}`");
    }
    for text in ["65535", "99999", "4294967295", "4294967296", "99999999999999999999999999"] {
        assert_eq!(effective(Some(text), &document), language("tr-TR"), "`{text}`");
    }
    // A number is digits and nothing else: a sign, a fraction or a space is text that is no tag.
    for text in ["-1", "+1", "1033.0", "10 33", "0x10", "1e3"] {
        assert_eq!(effective(Some(text), &document), language("tr-TR"), "`{text}`");
    }
}

#[test]
fn text_that_is_not_a_well_formed_tag_is_the_documents_language() {
    // "a run whose language is not a well-formed language tag is checked in the document's language". Naive: no language (so unchecked), or a refusal.
    let document = tag("fr-FR");
    for text in ["not a tag", "en_US", "abcdefghi", "en--US", " en-US", "en-US ", "é", "en-", "-", "1033.5", "#abc", "Themed1"] {
        assert_eq!(effective(Some(text), &document), language("fr-FR"), "`{text}`");
    }
}

#[test]
fn a_run_with_no_cell_anywhere_is_the_documents_language() {
    // A drawing made before this feature has no `LangID` on any row.
    assert_eq!(effective(None, &tag("fr-FR")), language("fr-FR"));
    assert_eq!(effective(None, &tag("zh-Hans-CN")), language("zh-Hans-CN"));
    // And the empty value is not the missing cell: it is a value, and it says no language.
    assert_eq!(effective(Some(""), &tag("fr-FR")), Effective::None);
}

#[test]
fn an_effective_language_names_its_tag() {
    assert_eq!(language("de-CH").tag().map(Tag::as_str), Some("de-CH"));
    assert_eq!(Effective::None.tag(), None);
}

// ---- one reading for the renderer and the text measure -----------------------------------------------------------------------

#[test]
fn what_the_shaper_is_handed_is_the_tag_in_canonical_case_or_nothing() {
    // The shaper and `Case` read a text, and an empty text is no language. Naive: `x-none` handed over as if it were a language, or a tag in the case the file wrote.
    let document = tag("tr-TR");
    let rows = [
        (None, "tr-TR"),
        (Some("en-us"), "en-US"),
        (Some("sr-latn-rs"), "sr-Latn-RS"),
        (Some(""), ""),
        (Some("x-none"), ""),
        (Some("ZXX"), ""),
        (Some("none"), ""),
        (Some("0"), ""),
        (Some("000"), ""),
        (Some("65535"), "tr-TR"),
        // A number the table of LCID numbers pairs with a tag is that language (1033 is en-US), and one it does not pair is the document's own.
        (Some("1033"), "en-US"),
        (Some("not a tag"), "tr-TR"),
        (Some("tr_TR"), "tr-TR"),
    ];
    for (value, handed) in rows {
        assert_eq!(lang::shaping_tag(value, &document), handed, "{value:?}");
        assert_eq!(effective(value, &document).shaping(), handed, "{value:?}");
    }
}

fn lang_cell() -> CellAddr {
    CellAddr::in_row("Character", None, RowKey::Ix(0), "LangID")
}

fn shape_at() -> SheetRef {
    SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0))
}

/// A drawing of one shape with a `LangID` on its first character row, and on the root style's, when each is given.
fn drawing(root: Option<&str>, own: Option<&str>) -> Document {
    let mut shape = ShapeBuilder::new(1);
    if let Some(own) = own {
        shape = shape.row_cell("Character", None, RowKey::Ix(0), "LangID", own);
    }
    let mut doc = DocBuilder::new();
    if let Some(root) = root {
        doc = doc.root(|r| r.row_cell("Character", None, RowKey::Ix(0), "LangID", root));
    }
    doc.page(1, PageBuilder::new().shape(shape)).build()
}

fn saved(doc: &Document, at: &SheetRef) -> Option<String> {
    lang::saved_value(&Resolver::new(doc), at, &lang_cell()).map(str::to_owned)
}

#[test]
fn the_saved_value_of_a_language_cell_is_what_the_cell_holds_found_through_the_chain() {
    let rows: [(Option<&str>, Option<&str>, Option<&str>); 10] = [
        // No cell anywhere: no answer, which is the document's language. Naive: `en-US`.
        (None, None, None),
        (None, Some("de-DE"), Some("de-DE")),
        // A shape with no cell of its own reads the root style's.
        (Some("tr-TR"), None, Some("tr-TR")),
        (Some("tr-TR"), Some("de-DE"), Some("de-DE")),
        // An empty value is an answer (no language), where a missing cell is none, and it stands over the root style's.
        (None, Some(""), Some("")),
        (Some("tr-TR"), Some(""), Some("")),
        (Some(""), None, Some("")),
        // The value is the cell's own text, whatever it looks like: a number or `true` is not turned into another text.
        (None, Some("1033"), Some("1033")),
        (None, Some("0.0"), Some("0.0")),
        (None, Some("true"), Some("true")),
    ];
    for (root, own, want) in rows {
        assert_eq!(saved(&drawing(root, own), &shape_at()).as_deref(), want, "root {root:?}, own {own:?}");
    }
}

#[test]
fn a_shape_that_is_not_there_has_no_language_and_the_value_comes_from_resolve_or_root() {
    let doc = drawing(Some("tr-TR"), None);
    // A shape that is not there holds no cell, and the root style's does not stand in for it.
    assert_eq!(saved(&doc, &SheetRef::Shape(ShapeRef::top(Owner::Page(9), 0))), None);
    assert_eq!(saved(&doc, &SheetRef::Shape(ShapeRef::top(Owner::Page(1), 7))), None);
    // `resolve_or_root` is the resolution the value comes from: the root style's cell for the shape, the shape's own when it holds one.
    let value = |doc: &Document| {
        let found = Resolver::new(doc).resolve_or_root(&shape_at(), &lang_cell())?;
        found.value.cell.and_then(|cell| cell.value.as_ref()).map(|v| v.text().to_owned())
    };
    assert_eq!(value(&doc).as_deref(), Some("tr-TR"));
    assert_eq!(value(&drawing(Some("tr-TR"), Some("de-DE"))).as_deref(), Some("de-DE"));
    assert_eq!(value(&drawing(None, None)), None);
}

// ---- the document's language (the order) ---------------------------------------------------------------------------------------

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
fn the_documents_language_is_doclangid_then_the_core_language_then_the_preferred_one_then_en_us() {
    let preferred = tag("nl-NL");
    // All three: DocLangID wins. Naive: the preferred language, or the core's.
    assert_eq!(document_language(&doc_with(Some("fr-FR"), Some("de-DE")), Some(&preferred)), tag("fr-FR"));
    // No DocLangID: the core language.
    assert_eq!(document_language(&doc_with(None, Some("de-DE")), Some(&preferred)), tag("de-DE"));
    // Neither: the person's preferred language.
    assert_eq!(document_language(&doc_with(None, None), Some(&preferred)), tag("nl-NL"));
    // Nothing at all, and no preferred language (a program with no profile): `en-US`.
    assert_eq!(document_language(&doc_with(None, None), None), tag("en-US"));
    assert_eq!(document_language(&Document::empty(), None).as_str(), "en-US");
}

#[test]
fn a_document_language_that_is_no_usable_tag_is_passed_over() {
    let preferred = tag("nl-NL");
    // The file whose `DocLangID` is empty. A number no table pairs (one it pairs is that language: `lang_data.rs`); text that is no tag; and `x-none` or
    // `zxx`, which say no language and so cannot be the language a run with no cell falls back to.
    for bad in ["", "0", "65535", "not a tag", "en_US", "x-none", "zxx", "none", "NONE"] {
        assert_eq!(
            document_language(&doc_with(Some(bad), Some("de-DE")), Some(&preferred)),
            tag("de-DE"),
            "DocLangID `{bad}` falls to the core language"
        );
        assert_eq!(
            document_language(&doc_with(Some(bad), None), Some(&preferred)),
            tag("nl-NL"),
            "DocLangID `{bad}` falls to the preferred language"
        );
        assert_eq!(
            document_language(&doc_with(None, Some(bad)), Some(&preferred)),
            tag("nl-NL"),
            "core language `{bad}` falls to the preferred language"
        );
        assert_eq!(document_language(&doc_with(Some(bad), Some(bad)), None), tag("en-US"), "`{bad}` twice falls to en-US");
    }
}

#[test]
fn the_documents_language_is_in_canonical_case() {
    assert_eq!(document_language(&doc_with(Some("fr-fr"), None), None).as_str(), "fr-FR");
    assert_eq!(document_language(&doc_with(None, Some("SR-LATN-RS")), None).as_str(), "sr-Latn-RS");
}

#[test]
fn a_doclangid_with_a_formula_and_no_value_is_not_read() {
    // The model holds the cached value of a cell; a cell with none says nothing.
    let mut doc = Document::empty();
    Arc::make_mut(&mut doc.sheet).set_cell(&CellAddr::direct("DocLangID"), Cell::with_formula("\"fr-FR\"", None)).unwrap();
    assert_eq!(document_language(&doc, None).as_str(), "en-US");
}

#[test]
fn the_fallback_language_is_en_us() {
    assert_eq!(lang::fallback_language().as_str(), "en-US");
}

// ---- properties -------------------------------------------------------------------------------------------------------------------

/// A subtag-shaped piece: letters and digits of lengths 1 to 9, in either case.
fn subtag() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[A-Za-z0-9]{1,9}").unwrap()
}

/// Text made of subtags and hyphens, and now and then a character that is no part of a tag.
fn tag_like() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => proptest::collection::vec(subtag(), 1..8).prop_map(|parts| parts.join("-")),
        1 => proptest::string::string_regex("[A-Za-z0-9_ -]{0,40}").unwrap(),
        1 => any::<String>(),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 2_000, ..ProptestConfig::default() })]

    #[test]
    fn an_accepted_tag_reads_again_as_itself(text in tag_like()) {
        if let Ok(t) = parse(&text) {
            prop_assert!(t.as_str().len() <= MAX_TAG_BYTES);
            prop_assert_eq!(t.as_str().len(), text.len(), "the canonical text is as long as the given");
            prop_assert_eq!(parse(t.as_str()), Ok(t.clone()), "a tag is its own canonical form");
            prop_assert!(t.as_str().eq_ignore_ascii_case(&text), "only the case changed");
        }
    }

    #[test]
    fn case_does_not_matter(text in tag_like()) {
        prop_assert_eq!(parse(&text.to_ascii_lowercase()), parse(&text.to_ascii_uppercase()));
    }

    #[test]
    fn the_parts_of_a_tag_are_pieces_of_it(text in tag_like()) {
        if let Ok(t) = parse(&text) {
            prop_assert!(t.as_str().starts_with(t.primary()));
            for part in [t.script(), t.region()].into_iter().flatten() {
                prop_assert!(t.as_str().split('-').any(|s| s == part), "`{}` is a subtag of `{}`", part, t);
            }
            if let Some(script) = t.script() {
                prop_assert!(script.len() == 4 && script.bytes().all(|b| b.is_ascii_alphabetic()));
            }
            if let Some(region) = t.region() {
                prop_assert!((region.len() == 2 && region.bytes().all(|b| b.is_ascii_alphabetic())) || (region.len() == 3 && region.bytes().all(|b| b.is_ascii_digit())));
            }
        }
    }

    #[test]
    fn an_effective_language_is_a_real_one_or_none(text in tag_like(), document in prop_oneof![Just("en-US"), Just("tr-TR"), Just("zh-Hans-CN")]) {
        let document = tag(document);
        match effective(Some(&text), &document) {
            Effective::None => prop_assert!(is_no_language(&text) || text.bytes().all(|b| b == b'0')),
            Effective::Language(t) => {
                prop_assert!(!is_no_language(t.as_str()), "`{}` is a language", t);
                prop_assert!(parse(t.as_str()).is_ok());
            }
        }
    }

    #[test]
    fn nothing_is_a_panic(bytes in proptest::collection::vec(any::<u8>(), 0..600)) {
        let text = String::from_utf8_lossy(&bytes);
        let _ = parse(&text);
        let _ = is_no_language(&text);
        let _ = effective(Some(&text), &tag("en-US"));
    }
}
