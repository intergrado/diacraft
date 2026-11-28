//! `cargo fuzz run lang_tag`: the text of a language tag, and what a run's `LangID` means. The input is any bytes, read as text by lossy UTF-8 (a byte
//! that is no character becomes U+FFFD, so the fuzzer reaches every text and the properties stay about text).
//!
//! **`parse`** gives a tag or a refusal in words, never a panic, and **a tag it accepts reads again as itself**: the canonical text is as long as the text given, at most
//! `MAX_TAG_BYTES`, the same letters in another case, its own canonical form, and its script and region are subtags of it of the shapes RFC 5646 gives them. A refusal
//! is the same for the text in capitals as for the text in lower case (case is no part of the syntax), and a text past `MAX_TAG_BYTES` is refused as too long whatever
//! it holds. **`is_no_language`** agrees with itself across case and across the canonical form.
//!
//! **`effective`** reads the same text as a `LangID` value, in a drawing whose language the first byte picks (a Turkish one, an English one and a Chinese one): it is
//! a language that is no word for none, the language the table of LCID numbers pairs with a number it pairs, the document's own for a text that is no tag
//! and no such number, and none for the words for none and for 0 in digits; and no value is ever turned into a language that is not well formed. **`document_language`**
//! reads it as the document's `DocLangID` and as its core language, and gives a usable tag whatever they hold, `en-US` for neither.
//!
//! **The tables** are asked the same text as a tag and its first four bytes as an LCID number: a number gives a tag that reads as itself and has a number, a
//! number past 16 bits gives none, and a tag has an English name (non-empty, a few dozen bytes for each subtag at most), four non-empty quotation marks and a number that
//! fits 16 bits when it has one; nothing panics, whatever the tag.
//!
//! The recipe of a run by hand is in `docs/development.md` ("How a target is run by hand"); nothing here needs a corpus, and none is committed.
#![no_main]

use std::sync::Arc;

use diacraft_doc::lang::{self, Effective, MAX_TAG_BYTES, TagError};
use diacraft_doc::{Cell, CellAddr, Document, Value};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The first byte picks the drawing's language for `effective` (its low two bits) and which cells hold the text for `document_language` (the next three); the rest
    // is the text.
    let (pick, rest) = data.split_first().map_or((0, data), |(p, r)| (*p, r));
    let text = String::from_utf8_lossy(rest);
    let all = String::from_utf8_lossy(data);

    // The tag: read, and read again.
    let upper = text.to_ascii_uppercase();
    match lang::parse(&text) {
        Ok(tag) => {
            let canonical = tag.as_str();
            assert!(canonical.len() <= MAX_TAG_BYTES, "a tag of {} bytes", canonical.len());
            assert_eq!(canonical.len(), text.len(), "`{text}` is read as `{canonical}`");
            assert!(canonical.eq_ignore_ascii_case(&text), "`{text}` is read as `{canonical}`");
            assert_eq!(lang::parse(canonical), Ok(tag.clone()), "`{canonical}` does not read as itself");
            assert_eq!(lang::parse(&upper), Ok(tag.clone()), "`{upper}` does not read as `{text}` does");
            assert_eq!(tag.to_string(), canonical);
            assert!(canonical.starts_with(tag.primary()));
            for (name, part) in [("script", tag.script()), ("region", tag.region())] {
                if let Some(part) = part {
                    assert!(canonical.split('-').any(|s| s == part), "the {name} `{part}` is no subtag of `{canonical}`");
                    assert!(part.bytes().all(|b| b.is_ascii_alphanumeric()));
                    assert!((2..=4).contains(&part.len()));
                }
            }
            assert_eq!(lang::is_no_language(canonical), lang::is_no_language(&text), "`{text}` and `{canonical}` differ in meaning no language");
        }
        Err(e) => {
            assert!(!e.to_string().is_empty());
            assert_eq!(lang::parse(&upper), Err(e), "`{text}` and `{upper}` are refused differently");
            // The length is the first thing checked, and a refusal for it is only ever for it.
            assert_eq!(text.len() > MAX_TAG_BYTES, e == TagError::TooLong, "`{text}` is refused as {e:?}");
            assert_eq!(text.is_empty(), e == TagError::Empty, "`{text}` is refused as {e:?}");
        }
    }
    assert_eq!(lang::is_no_language(&text), lang::is_no_language(&upper));

    // The tables: the text as a tag, and four bytes as a number.
    if let Ok(tag) = lang::parse(&text) {
        let name = lang::english_name(&tag);
        assert!(!name.is_empty() && name.len() <= 64 * tag.as_str().len(), "`{tag}` is named `{name}`");
        let marks = lang::quotes(&tag);
        for mark in [marks.open, marks.close, marks.open_inner, marks.close_inner] {
            assert!(!mark.is_empty(), "`{tag}` has an empty mark");
        }
        let _ = lang::script_of_language(&tag);
        assert!(lang::to_lcid(&tag).is_none_or(|number| number <= 0xFFFF), "`{tag}` has a number past 16 bits");
    }
    let number = data.get(..4).and_then(|four| four.try_into().ok()).map_or(0, u32::from_le_bytes);
    if let Some(tag) = lang::from_lcid(number) {
        assert!(number <= 0xFFFF, "{number} is past 16 bits and is paired with `{tag}`");
        assert_eq!(lang::parse(tag.as_str()), Ok(tag.clone()), "{number} is `{tag}`, which is not in canonical case");
        assert!(lang::to_lcid(&tag).is_some(), "{number} is `{tag}`, which has no number");
    }

    // The value: what it means in a drawing in Turkish, in English or in Chinese.
    let document = lang::parse(["tr-TR", "en-US", "zh-Hans-CN"][usize::from(pick & 3) % 3]).expect("a tag that is well formed");
    match lang::effective(Some(&text), &document) {
        Effective::None => assert!(lang::is_no_language(&text) || text.bytes().all(|b| b == b'0'), "`{text}` is no language"),
        Effective::Language(tag) => {
            assert!(!lang::is_no_language(tag.as_str()), "`{text}` is read as `{tag}`, which means no language");
            assert_eq!(lang::parse(tag.as_str()), Ok(tag.clone()));
            // A text that is a tag (and no word for none, which the other arm took) is its own language; digits are the tag the table pairs with the number, if
            // it pairs one; any other text is the document's.
            match lang::parse(&text) {
                Ok(own) => assert_eq!(tag, own, "`{text}` is a language of its own"),
                Err(_) => {
                    let paired = if text.bytes().all(|b| b.is_ascii_digit()) { text.parse::<u32>().ok().and_then(lang::from_lcid) } else { None };
                    assert_eq!(
                        tag,
                        paired.unwrap_or_else(|| document.clone()),
                        "`{text}` is no tag: the table's language for it, else the document's"
                    );
                }
            }
        }
    }
    assert_eq!(lang::effective(None, &document), Effective::Language(document.clone()));

    // The document's language: the text as `DocLangID`, as the core language, and as both (the first byte's bits say which cells hold it).
    let mut doc = Document::empty();
    if pick & 4 != 0 {
        // A cell that cannot be put leaves the document without it, which a fuzz run may meet and ignore.
        let _ = Arc::make_mut(&mut doc.sheet).set_cell(&CellAddr::direct("DocLangID"), Cell::constant(Value::from_text(&all)));
    }
    if pick & 8 != 0 {
        Arc::make_mut(&mut doc.props).language = Some(Arc::from(&*all));
    }
    let preferred = if pick & 16 != 0 { lang::parse("nl-NL").ok() } else { None };
    let language = lang::document_language(&doc, preferred.as_ref());
    assert!(!lang::is_no_language(language.as_str()), "the document is in `{language}`");
    assert_eq!(lang::parse(language.as_str()), Ok(language.clone()));
    if pick & 12 == 0 {
        assert_eq!(language.as_str(), if preferred.is_some() { "nl-NL" } else { "en-US" });
    }
});
