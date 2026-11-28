//! The languages of a run and of a drawing in an `.odg`: what the
//! reader and the writers share.
//!
//! **ODF holds three languages for a run** (ODF 1.4 Part 3 §20.358: one for the letters of the Latin type, one for the Asian type and one for the complex type), each as a **triplet** of a
//! language (`fo:language`, ISO 639), an optional script (`fo:script`, ISO 15924) and a country (`fo:country`, ISO 3166), with a full tag (`style:rfc-language-tag`) for what the three cannot
//! say; the model holds one `LangID` for a run. So:
//!
//! * **Read** ([`crate::read`]): the triplet of the script type **most of the run's letters belong to** ([`Letters`], with a tie going to the Latin type, then the Asian one), or the
//!   Latin one for a run with no letters, is the run's `LangID`; `zxx` and `none` are no language (`x-none`).
//! * **Written**: a `LangID` that is not what it was read as is written into the triplet of the same script type ([`said`], [`attributes`]) and the other two stay as the file has them.
//! * **The drawing's language** ([`declared`]) is `DocLangID`, else the core properties' language, and a new drawing writes it as `dc:language` and on the default graphic style.
//!
//! Nothing here recurses, indexes or allocates more than a tag's bytes ([`diacraft_doc::lang::MAX_TAG_BYTES`]).

use diacraft_doc::lang::{self, Tag};
use diacraft_doc::{Document, Value};
use diacraft_fonts::script::{FontSlot, script_of, slot_of_script};

use crate::read::names::Ns;

/// The letters of a text, counted by the script type each belongs to. A letter is a character of a script of its own: digits, punctuation and spaces are of none, and are no vote.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Letters([usize; 3]);

impl Letters {
    /// Counts the letters of `text`.
    pub(crate) fn add(&mut self, text: &str) {
        for c in text.chars() {
            self.add_char(c);
        }
    }

    /// Counts `c`, when it is a letter.
    pub(crate) fn add_char(&mut self, c: char) {
        if let Some(script) = script_of(c) {
            let at = match slot_of_script(script) {
                FontSlot::Latin => 0,
                FontSlot::Asian => 1,
                FontSlot::Complex => 2,
            };
            if let Some(n) = self.0.get_mut(at) {
                *n = n.saturating_add(1);
            }
        }
    }

    /// The script type most of the letters belong to: the Latin type when the letters do not say (none, or a tie that holds it), then the Asian type.
    pub(crate) fn slot(self) -> FontSlot {
        let [latin, asian, complex] = self.0;
        if latin >= asian && latin >= complex {
            FontSlot::Latin
        } else if asian >= complex {
            FontSlot::Asian
        } else {
            FontSlot::Complex
        }
    }
}

/// The attributes of the triplet of a script type, in the order language, script, country and full tag.
pub(crate) fn attributes(slot: FontSlot) -> [(Ns, &'static str); 4] {
    match slot {
        FontSlot::Latin => [(Ns::Fo, "language"), (Ns::Fo, "script"), (Ns::Fo, "country"), (Ns::Style, "rfc-language-tag")],
        FontSlot::Asian => {
            [(Ns::Style, "language-asian"), (Ns::Style, "script-asian"), (Ns::Style, "country-asian"), (Ns::Style, "rfc-language-tag-asian")]
        }
        FontSlot::Complex => {
            [(Ns::Style, "language-complex"), (Ns::Style, "script-complex"), (Ns::Style, "country-complex"), (Ns::Style, "rfc-language-tag-complex")]
        }
    }
}

/// The group of cells a run's language is, by the script type it is written for (`runs::owned`): each triplet is a group of its own, so that a changed language is written again into one
/// triplet and the others are left as the file has them.
pub(crate) fn group_id(slot: FontSlot) -> &'static str {
    match slot {
        FontSlot::Latin => "LangLatin",
        FontSlot::Asian => "LangAsian",
        FontSlot::Complex => "LangComplex",
    }
}

/// The attributes a language is said by: the triplet's language, script and country, and the full tag beside them when they cannot say it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Said {
    pub language: Option<String>,
    pub script: Option<String>,
    pub country: Option<String>,
    pub rfc: Option<String>,
}

/// How an attribute of a triplet is written for `said`: its namespace, name and value, in the order language, script, country and full tag, for the script type `slot`.
pub(crate) fn pairs(said: &Said, slot: FontSlot) -> Vec<(Ns, &'static str, String)> {
    let [language, script, country, rfc] = attributes(slot);
    [(language, &said.language), (script, &said.script), (country, &said.country), (rfc, &said.rfc)]
        .into_iter()
        .filter_map(|((ns, local), value)| value.as_ref().map(|v| (ns, local, v.clone())))
        .collect()
}

/// How an `.odg` says the language `value` (a `LangID`): `None` for a value it cannot say, which is a number of the legacy XML and a text that is no well-formed tag.
///
/// * No language (empty, `x-none`, `zxx`, `none`) is `zxx` with the country `none`, as LibreOffice writes it.
/// * A tag of a language of two or three letters, with a script and a region of two letters at most, is those three attributes.
/// * Any other tag (a variant, an extension, private use, a region of digits, a language of other length) is the full tag, with the nearest triplet beside it where it has a language of two
///   or three letters (`de-CH-1996` is `de`, `CH` and the tag).
pub(crate) fn said(value: &str) -> Option<Said> {
    if lang::is_no_language(value) {
        return Some(Said { language: Some("zxx".to_owned()), script: None, country: Some("none".to_owned()), rfc: None });
    }
    let tag = lang::parse(value).ok()?;
    let primary = tag.primary();
    let letters = |s: &str, n: std::ops::RangeInclusive<usize>| n.contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphabetic());
    let language = letters(primary, 2..=3).then(|| primary.to_owned());
    let country = tag.region().filter(|r| letters(r, 2..=2)).map(str::to_owned);
    let script = tag.script().filter(|_| language.is_some()).map(str::to_owned);
    let mut plain = String::new();
    if let Some(language) = &language {
        plain.push_str(language);
        for part in [&script, &country].into_iter().flatten() {
            plain.push('-');
            plain.push_str(part);
        }
    }
    let exact = language.is_some() && plain == tag.as_str();
    Some(Said { language, script, country, rfc: (!exact).then(|| tag.as_str().to_owned()) })
}

/// The language the drawing says it is in: `DocLangID` when it is a language tag, else the core properties' language when it is one. A drawing that says none has no declared language
/// (the language it is then checked in is the person's, or English), and a new drawing of it is written with none.
pub(crate) fn declared(doc: &Document) -> Option<Tag> {
    let doc_lang_id = doc.sheet.cell("DocLangID").and_then(|cell| cell.value.as_ref()).map(Value::text);
    let core = doc.props.language.as_deref();
    [doc_lang_id, core].into_iter().flatten().find_map(|text| if lang::is_no_language(text) { None } else { lang::parse(text).ok() })
}

/// The default graphic style a drawing with a language and no default style of its own is written with: its language in the triplet of the Latin type, which is where a run with
/// no letters, and every run that no style of its own says, reads its language from. `None` for a drawing that holds a default graphic style (it says its own) or declares no language.
pub(crate) fn default_style(doc: &Document) -> Option<String> {
    if doc.ext.get().is_some_and(|d| d.odf.styles.iter().any(|s| &*s.family == "common:graphic" && s.name.is_empty())) {
        return None;
    }
    let tag = declared(doc)?;
    let attributes: String = pairs(&said(tag.as_str())?, FontSlot::Latin)
        .into_iter()
        .map(|(ns, local, value)| format!(" {}:{local}=\"{value}\"", if ns == Ns::Fo { "fo" } else { "style" }))
        .collect();
    Some(format!("<style:default-style style:family=\"graphic\"><style:text-properties{attributes}/></style:default-style>"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn letters(text: &str) -> FontSlot {
        let mut counted = Letters::default();
        counted.add(text);
        counted.slot()
    }

    #[test]
    fn the_script_type_most_letters_belong_to_wins_and_a_tie_goes_latin_then_asian() {
        let (latin, asian, complex) = ("ab", "\u{65E5}\u{672C}", "\u{0639}\u{0639}");
        assert_eq!(letters(""), FontSlot::Latin);
        assert_eq!(letters("123 ,."), FontSlot::Latin, "no letters");
        assert_eq!(letters(asian), FontSlot::Asian);
        assert_eq!(letters(complex), FontSlot::Complex);
        assert_eq!(letters(&format!("{latin}{asian}")), FontSlot::Latin, "a tie of Latin and Asian");
        assert_eq!(letters(&format!("{asian}{complex}")), FontSlot::Asian, "a tie of Asian and complex");
        assert_eq!(letters(&format!("{latin}{asian}{complex}")), FontSlot::Latin);
        assert_eq!(letters(&format!("a{asian}{complex}")), FontSlot::Asian);
        assert_eq!(letters(&format!("{complex}{complex}{asian}")), FontSlot::Complex);
    }

    #[test]
    fn a_language_is_said_by_the_triplet_and_by_the_tag_where_the_triplet_cannot() {
        let said = |v: &str| said(v).map(|s| (s.language, s.script, s.country, s.rfc));
        let some = |s: &str| Some(s.to_owned());
        assert_eq!(said("fr-FR"), Some((some("fr"), None, some("FR"), None)));
        assert_eq!(said("fr"), Some((some("fr"), None, None, None)));
        assert_eq!(said("sr-Latn-RS"), Some((some("sr"), some("Latn"), some("RS"), None)));
        assert_eq!(said("de-CH-1996"), Some((some("de"), None, some("CH"), some("de-CH-1996"))));
        assert_eq!(said("es-419"), Some((some("es"), None, None, some("es-419"))), "a region of digits is no country");
        assert_eq!(said("zh-yue"), Some((some("zh"), None, None, some("zh-yue"))));
        assert_eq!(said("x-none"), Some((some("zxx"), None, some("none"), None)));
        assert_eq!(said("ZXX"), Some((some("zxx"), None, some("none"), None)));
        assert_eq!(said(""), Some((some("zxx"), None, some("none"), None)));
        assert_eq!(said("x-private"), Some((None, None, None, some("x-private"))), "private use is the full tag alone");
        assert_eq!(said("1033"), None, "a number of the legacy XML is no tag");
        assert_eq!(said("d3 4"), None);
        assert_eq!(said(&"a".repeat(300)), None);
    }

    #[test]
    fn the_drawing_s_declared_language_is_doc_lang_id_and_then_the_core_language() {
        use diacraft_doc::{Cell, CellName};
        let mut doc = Document::new_blank();
        assert!(declared(&doc).is_none(), "a drawing that says none declares none");
        std::sync::Arc::make_mut(&mut doc.props).language = Some("fr-FR".into());
        assert_eq!(declared(&doc).map(|t| t.to_string()).as_deref(), Some("fr-FR"));
        let doc_lang_id = |doc: &mut Document, text: &str| {
            std::sync::Arc::make_mut(&mut doc.sheet).cells.insert(CellName::new("DocLangID"), Cell::constant(Value::from_text(text)));
        };
        doc_lang_id(&mut doc, "de-de");
        assert_eq!(declared(&doc).map(|t| t.to_string()).as_deref(), Some("de-DE"), "DocLangID first, in canonical case");
        for passed_over in ["x-none", "zxx", "", "1033", "not a tag"] {
            doc_lang_id(&mut doc, passed_over);
            assert_eq!(declared(&doc).map(|t| t.to_string()).as_deref(), Some("fr-FR"), "{passed_over:?} says no language or is no tag");
        }
    }

    #[test]
    fn a_default_graphic_style_is_made_for_a_declared_language_and_for_no_other() {
        let mut doc = Document::new_blank();
        assert_eq!(default_style(&doc), None);
        std::sync::Arc::make_mut(&mut doc.props).language = Some("sr-Latn-RS".into());
        assert_eq!(
            default_style(&doc).as_deref(),
            Some(
                r#"<style:default-style style:family="graphic"><style:text-properties fo:language="sr" fo:script="Latn" fo:country="RS"/></style:default-style>"#
            )
        );
        std::sync::Arc::make_mut(&mut doc.props).language = Some("de-CH-1996".into());
        let made = default_style(&doc).unwrap_or_default();
        assert!(made.contains(r#"style:rfc-language-tag="de-CH-1996""#) && made.contains(r#"fo:language="de""#), "{made}");
    }

    #[test]
    fn the_pairs_of_a_triplet_are_the_attributes_of_its_script_type() {
        let s = said("sr-Latn-RS").unwrap();
        let names: Vec<_> = pairs(&s, FontSlot::Asian).into_iter().map(|(_, local, value)| format!("{local}={value}")).collect();
        assert_eq!(names, ["language-asian=sr", "script-asian=Latn", "country-asian=RS"]);
        let rfc = said("de-CH-1996").unwrap();
        let names: Vec<_> = pairs(&rfc, FontSlot::Complex).into_iter().map(|(_, local, value)| format!("{local}={value}")).collect();
        assert_eq!(names, ["language-complex=de", "country-complex=CH", "rfc-language-tag-complex=de-CH-1996"]);
    }
}
