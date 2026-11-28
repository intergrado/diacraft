//! Language tags, and the language a run is in (design `2026-11-26-language-proofing-design.md` 5.1
//! and 5.4).
//!
//! **A run's language is a cell** (`Character.LangID`), and files hold what they held: a culture name (`en-US`), `x-none` for "no language", an empty value, a number in
//! the legacy XML. Reading never rewrites a value. This module is the one reading of that cell that the renderer, the proofer and AutoCorrect share, so that the three
//! agree on what a run is in:
//!
//! * [`parse`] reads a **well-formed language tag** by the syntax of RFC 5646 (the grandfathered tags included), in at most [`MAX_TAG_BYTES`] bytes, and gives a [`Tag`]
//!   in canonical case (`en-us` is `en-US`, `sr-latn-rs` is `sr-Latn-RS`). Well-formed is the syntax and nothing more: a tag no table lists (`en-FI`, `qq-ZZ`) is a tag.
//! * [`is_no_language`] says whether a value means **no language**: empty, `x-none`, `zxx` or `none`, in any case.
//! * [`effective`] is design 5.1's table: what a resolved `LangID` value means in a drawing whose language is `document`.
//! * [`document_language`] is the drawing's language: `DocLangID`, else the core properties' language, else the person's preferred one, else `en-US`.
//! * [`from_lcid`] and [`to_lcid`] pair the numbers of the legacy XML with tags, [`known`] lists the tags a person can pick, [`english_name`] names a tag in English,
//!   [`quotes`] gives a language's quotation marks and [`script_of_language`] the script type of its likely script. **The tables are generated** by `cargo xtask langdata`
//!   (`lang/lcid.rs`, `names.rs`, `quotes.rs` and `scripts.rs`) from ICU4C's `locmap.cpp` and Unicode CLDR's JSON, both under the Unicode License V3 (NOTICE): the numbers
//!   are never taken from a table of the reference app's file format (design call).
//!
//! **Numbers**. A value that is digits only is a number of the legacy XML: 0 is no language, a number [`from_lcid`] pairs with a tag is that language, and every
//! other number (65535, one the table lacks, one past 16 bits) reads as the document's language.
//!
//! **Hostile input.** A language value is any text a file holds. [`parse`] checks the length before it reads a byte, holds the text to ASCII letters, digits and
//! hyphens, and walks the subtags once with no recursion; nothing here indexes, and nothing allocates more than [`MAX_TAG_BYTES`].

use std::fmt;
use std::sync::Arc;

use crate::{CellAddr, Document, Resolver, SheetRef, Value};

mod lcid;
mod names;
mod quotes;
mod scripts;

/// The longest language tag [`parse`] accepts: 255 bytes. A project rule (design 7.8): RFC 5646 sets no length, and every tag of the census is under 20 bytes.
pub const MAX_TAG_BYTES: usize = 255;

/// The language of a drawing that gives none and has no preferred one (the last step).
const FALLBACK: &str = "en-US";

/// The longest subtag of RFC 5646's syntax.
const MAX_SUBTAG_BYTES: usize = 8;

/// The seventeen irregular grandfathered tags of RFC 5646 2.2.8, in the spelling the RFC lists them in. Each matches no production but its own, so each is listed. (The
/// regular grandfathered tags, such as `zh-min-nan`, are ordinary tags by the syntax.)
const IRREGULAR: [&str; 17] = [
    "en-GB-oed",
    "i-ami",
    "i-bnn",
    "i-default",
    "i-enochian",
    "i-hak",
    "i-klingon",
    "i-lux",
    "i-mingo",
    "i-navajo",
    "i-pwn",
    "i-tao",
    "i-tay",
    "i-tsu",
    "sgn-BE-FR",
    "sgn-BE-NL",
    "sgn-CH-DE",
];

/// Why a text is not a language tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TagError {
    #[error("a language tag must not be empty")]
    Empty,
    #[error("a language tag must be at most {max} bytes", max = MAX_TAG_BYTES)]
    TooLong,
    #[error("a language tag holds only ASCII letters, digits and hyphens")]
    Character,
    #[error("that is not a well-formed language tag (RFC 5646)")]
    Syntax,
}

/// A well-formed language tag in canonical case: the language in lower case, a script in title case, a region in upper case and every other subtag in lower case
/// (RFC 5646 2.1.1). It is cheap to clone. Two tags are equal when they are the same language written the same way once the case is made canonical, so `en-us` and
/// `en-US` give equal tags; nothing here knows `en-GB` and `en-UK` to be near each other.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Tag {
    text: Arc<str>,
    /// The bytes of `text` that hold the script subtag and the region subtag, when the tag has one. An irregular grandfathered tag names neither.
    script: Option<(u8, u8)>,
    region: Option<(u8, u8)>,
}

impl Tag {
    /// The tag in canonical case.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The first subtag: the language (`en`), or `x` for a tag of private use alone (`x-none`) and `i` for an irregular grandfathered tag.
    pub fn primary(&self) -> &str {
        self.text.split('-').next().unwrap_or("")
    }

    /// The script subtag in title case (`Latn` of `sr-Latn-RS`), if the tag has one.
    pub fn script(&self) -> Option<&str> {
        part(&self.text, self.script)
    }

    /// The region subtag in upper case (`RS` of `sr-Latn-RS`, `419` of `es-419`), if the tag has one.
    pub fn region(&self) -> Option<&str> {
        part(&self.text, self.region)
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

fn part(text: &str, at: Option<(u8, u8)>) -> Option<&str> {
    let (start, end) = at?;
    text.get(usize::from(start)..usize::from(end))
}

/// The tag of the language a drawing is in when it says none and the person has no preferred language: `en-US`.
pub fn fallback_language() -> Tag {
    Tag { text: Arc::from(FALLBACK), script: None, region: Some((3, 5)) }
}

/// Reads a well-formed language tag (RFC 5646's syntax, case ignored) and gives it in canonical case.
///
/// A text is refused when it is empty, longer than [`MAX_TAG_BYTES`] (checked first, so a huge text costs one comparison), holds anything but ASCII letters, digits and
/// hyphens, or breaks the syntax: an empty subtag, a subtag past eight characters, parts out of order (language, extended languages, script, region, variants,
/// extensions, private use), a singleton or `x` with nothing after it.
pub fn parse(text: &str) -> Result<Tag, TagError> {
    if text.is_empty() {
        return Err(TagError::Empty);
    }
    if text.len() > MAX_TAG_BYTES {
        return Err(TagError::TooLong);
    }
    if !text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Err(TagError::Character);
    }
    if let Some(listed) = IRREGULAR.iter().find(|t| t.eq_ignore_ascii_case(text)) {
        return Ok(Tag { text: Arc::from(*listed), script: None, region: None });
    }
    langtag(text).ok_or(TagError::Syntax)
}

/// How a subtag is written in canonical case.
#[derive(Clone, Copy)]
enum Kind {
    /// Lower case: the language, an extended language, a variant, an extension and private use.
    Lower,
    /// Title case.
    Script,
    /// Upper case.
    Region,
}

/// How far a tag has come. The parts keep this order and none goes back.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Stage {
    Language,
    Script,
    Region,
    Variant,
    Extension,
    Private,
}

/// The `langtag` and `privateuse` productions of RFC 5646 2.1 over `text`, which is non-empty ASCII letters, digits and hyphens: the tag in canonical case with where its
/// script and region are. `None` when a part is out of place or a subtag is empty or too long.
fn langtag(text: &str) -> Option<Tag> {
    let mut out = String::with_capacity(text.len());
    let (mut script, mut region) = (None, None);
    let mut stage = Stage::Language;
    let mut short_language = false;
    let mut extended = 0u8;
    // The last subtag was a singleton, or the `x` of private use, and has nothing after it yet.
    let mut open = false;
    for (n, sub) in text.split('-').enumerate() {
        let len = sub.len();
        if len == 0 || len > MAX_SUBTAG_BYTES {
            return None;
        }
        let bytes = sub.as_bytes();
        let alpha = bytes.iter().all(u8::is_ascii_alphabetic);
        let digits = bytes.iter().all(u8::is_ascii_digit);
        let kind = if n == 0 {
            if sub.eq_ignore_ascii_case("x") {
                stage = Stage::Private;
                open = true;
            } else if alpha && len >= 2 {
                short_language = len <= 3;
            } else {
                return None;
            }
            Kind::Lower
        } else if stage == Stage::Private {
            open = false;
            Kind::Lower
        } else if len == 1 {
            // A singleton begins an extension, and `x` begins private use. Two in a row leave the first empty.
            if open {
                return None;
            }
            stage = if sub.eq_ignore_ascii_case("x") { Stage::Private } else { Stage::Extension };
            open = true;
            Kind::Lower
        } else if stage == Stage::Extension {
            open = false;
            Kind::Lower
        } else if stage == Stage::Language && short_language && alpha && len == 3 && extended < 3 {
            extended += 1;
            Kind::Lower
        } else if stage == Stage::Language && alpha && len == 4 {
            stage = Stage::Script;
            Kind::Script
        } else if stage <= Stage::Script && ((alpha && len == 2) || (digits && len == 3)) {
            stage = Stage::Region;
            Kind::Region
        } else if len >= 5 || (len == 4 && bytes.first().is_some_and(u8::is_ascii_digit)) {
            stage = Stage::Variant;
            Kind::Lower
        } else {
            return None;
        };
        if n > 0 {
            out.push('-');
        }
        let start = out.len();
        for (i, c) in sub.chars().enumerate() {
            out.push(match kind {
                Kind::Lower => c.to_ascii_lowercase(),
                Kind::Script if i == 0 => c.to_ascii_uppercase(),
                Kind::Script => c.to_ascii_lowercase(),
                Kind::Region => c.to_ascii_uppercase(),
            });
        }
        let at = Some((u8::try_from(start).ok()?, u8::try_from(out.len()).ok()?));
        match kind {
            Kind::Script => script = at,
            Kind::Region => region = at,
            Kind::Lower => {}
        }
    }
    if open {
        return None;
    }
    Some(Tag { text: Arc::from(out), script, region })
}

/// Does the value mean **no language**: empty, `x-none` (Office's private-use tag for it), `zxx` (OpenDocument's code for "no linguistic content") or `none`, in any
/// case? Such a run is never checked, never marked and never corrected. Whitespace is not trimmed: a value with a space in it is text that is no tag.
pub fn is_no_language(value: &str) -> bool {
    value.is_empty() || ["x-none", "zxx", "none"].iter().any(|word| value.eq_ignore_ascii_case(word))
}

/// What a run's language is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effective {
    /// A language, which may have no dictionary on this machine.
    Language(Tag),
    /// No language: not checked, not marked, not corrected, and shaped with no language.
    None,
}

impl Effective {
    /// The language's tag, or `None` for no language.
    pub fn tag(&self) -> Option<&Tag> {
        match self {
            Effective::Language(tag) => Some(tag),
            Effective::None => None,
        }
    }

    /// The language as the shaper and `Case` are handed it: the tag in canonical case, and empty for no language.
    pub fn shaping(&self) -> &str {
        self.tag().map_or("", Tag::as_str)
    }
}

/// What a run's `LangID` means (design 5.1). `value` is the cell's saved value found by inheritance, `None` when no cell on the chain holds one;
/// `document` is the drawing's language ([`document_language`]).
///
/// | `value` | Means |
/// |---|---|
/// | a well-formed tag (`en-US`, `de-CH`, `en-FI`) | that language, whether or not any table lists it |
/// | empty, `x-none`, `zxx`, `none` (any case) | [`Effective::None`] |
/// | the number 0 | [`Effective::None`] |
/// | a number [`from_lcid`] pairs with a tag (1033, 2052) | that language |
/// | any other number (65535, a number no table lists, one past 16 bits) | the document's language |
/// | text that is no well-formed tag | the document's language |
/// | no cell anywhere on the chain | the document's language |
pub fn effective(value: Option<&str>, document: &Tag) -> Effective {
    let Some(value) = value else {
        return Effective::Language(document.clone());
    };
    if is_no_language(value) {
        return Effective::None;
    }
    // A number of the legacy XML: 0 is no language, a number the table pairs is that language, and any other is the document's.
    match number(value) {
        Some(0) => return Effective::None,
        Some(lcid) => return Effective::Language(from_lcid(lcid).unwrap_or_else(|| document.clone())),
        None => {}
    }
    match parse(value) {
        Ok(tag) => Effective::Language(tag),
        Err(_) => Effective::Language(document.clone()),
    }
}

/// The language a run is laid out and drawn in, as the text the shaper and `Case` read: [`effective`]'s answer for `value` in a drawing whose language is `document`,
/// empty for no language. The renderer and the text measure (`TEXTWIDTH`, `TEXTHEIGHT`) both take a run's language from this and [`saved_value`], so that a text
/// measured is the text drawn.
pub fn shaping_tag(value: Option<&str>, document: &Tag) -> String {
    effective(value, document).shaping().to_owned()
}

/// The text the language cell `addr` (a `Character` row's `LangID`) of `at` holds, as saved, found by inheritance through the shape's styles and master, and through
/// the root style for a shape that finds none ([`Resolver::resolve_or_root`]): `None` when no cell on the way holds a value, which is the document's language
/// ([`effective`]), and `Some("")` when one holds an empty value, which is an answer of its own (no language). The text is the value's own: a number or `true` is not
/// turned into another text. A formula is not worked out: what the cell holds is what is drawn.
pub fn saved_value<'a>(res: &Resolver<'a>, at: &SheetRef, addr: &CellAddr) -> Option<&'a str> {
    let found = res.resolve_or_root(at, addr)?;
    found.value.cell.and_then(|cell| cell.value.as_ref()).map(Value::text)
}

/// The number a value is when it is digits and nothing else and fits a `u32`.
fn number(value: &str) -> Option<u32> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

/// A tag a document or the person may name as a language: well formed (or a number [`from_lcid`] pairs with a tag), and not one of the words for no language (a drawing
/// in no language has nothing to fall back to, so a run with no cell could not be checked in it).
fn usable(text: &str) -> Option<Tag> {
    if is_no_language(text) {
        return None;
    }
    match number(text) {
        Some(lcid) => from_lcid(lcid),
        None => parse(text).ok(),
    }
}

/// The language of the drawing (the order): `DocLangID` when it is a usable tag (or a number the table pairs with one: the legacy XML's 1033), else the core
/// properties' language when it is one, else `preferred` (the person's preferred editing language, `None` for a program with no profile), else `en-US`. A value that is
/// empty, a number no table pairs (0, 65535), no tag, or a word for no language is passed over. Nothing is written: a new drawing's `DocLangID` is the engine's.
pub fn document_language(doc: &Document, preferred: Option<&Tag>) -> Tag {
    let doc_lang_id = doc.sheet.cell("DocLangID").and_then(|cell| cell.value.as_ref()).map(Value::text);
    let core = doc.props.language.as_deref();
    [doc_lang_id, core].into_iter().flatten().find_map(usable).or_else(|| preferred.cloned()).unwrap_or_else(fallback_language)
}

// ---- the tables ---------------------------------------------------------------------------------------------------------------------

/// The tag the table pairs with an LCID number of the legacy XML (`1033` is `en-US`, `2052` is `zh-CN`), or `None` for a number it lacks: 0, 65535, the invariant locale
/// (127) and any number past 16 bits (a sort order stacked on a language ID, which is no language ID). A sort variant of a language reads as the language (`1034`, Spanish
/// with its traditional sort, is `es-ES`). Where ICU lists two tags for a number, the first is given.
pub fn from_lcid(lcid: u32) -> Option<Tag> {
    let number = u16::try_from(lcid).ok()?;
    let at = lcid::FROM_LCID.binary_search_by_key(&number, |(n, _)| *n).ok()?;
    let (_, text) = lcid::FROM_LCID.get(at)?;
    parse(text).ok()
}

/// The number the table pairs with a tag (`en-US` is 1033), or failing that the number of the longest listed prefix of it, as ICU does for a region it has no number for:
/// `en-FI` is the number of `en` (9). `None` when not even the language is listed (`tlh`, Klingon, has no number), and for no language and private use.
pub fn to_lcid(tag: &Tag) -> Option<u32> {
    let mut key = tag.as_str();
    loop {
        if let Ok(at) = lcid::TO_LCID.binary_search_by(|(listed, _)| (*listed).cmp(key)) {
            return lcid::TO_LCID.get(at).map(|(_, number)| u32::from(*number));
        }
        key = key.get(..key.rfind('-')?)?;
    }
}

/// The tags a person can pick: every tag [`from_lcid`] gives, once each, sorted by the bytes of the tag. Each parses, is in canonical case, has an English name and a
/// number. A drawing may hold tags that are not here (design 5.4: the list a person picks from is these and the open drawing's own).
pub fn known() -> &'static [&'static str] {
    lcid::KNOWN
}

/// The value of `key` in a table sorted by the bytes of its keys.
fn find(table: &'static [(&'static str, &'static str)], key: &str) -> Option<&'static str> {
    let at = table.binary_search_by(|(listed, _)| (*listed).cmp(key)).ok()?;
    table.get(at).map(|(_, value)| *value)
}

/// `pattern` with `{0}` replaced by `first` and `{1}` by `second`, in one pass (a name that held `{1}` would not be read as a place).
fn fill(pattern: &str, first: &str, second: &str) -> String {
    let mut out = String::with_capacity(pattern.len() + first.len() + second.len());
    let mut rest = pattern;
    while let Some(c) = rest.chars().next() {
        if let Some(tail) = rest.strip_prefix("{0}") {
            out.push_str(first);
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix("{1}") {
            out.push_str(second);
            rest = tail;
        } else {
            out.push(c);
            rest = rest.get(c.len_utf8()..).unwrap_or("");
        }
    }
    out
}

/// The English name of what a subtag after the language is: a script (four letters), a region (two letters or three digits) or a variant, by CLDR's names, and the
/// subtag itself when CLDR has none.
fn qualifier(sub: &str) -> &str {
    let letters = sub.bytes().all(|b| b.is_ascii_alphabetic());
    let named = if letters && sub.len() == 4 {
        find(names::SCRIPTS, sub)
    } else if (letters && sub.len() == 2) || (sub.len() == 3 && sub.bytes().all(|b| b.is_ascii_digit())) {
        find(names::TERRITORIES, sub)
    } else {
        find(names::VARIANTS, sub)
    };
    named.unwrap_or(sub)
}

/// The English name of a tag as CLDR's data builds it: the language, and in brackets its script, region and variants (`English (United Kingdom)`,
/// `Chinese (Simplified, China)`), joined by CLDR's own patterns. A subtag CLDR has no name for stands for itself, so a tag no table lists still has a name (`qaa`,
/// `English (AA)`); an extension and private use are left out, and no language and the grandfathered tags, which CLDR has no name for, are their own names.
pub fn english_name(tag: &Tag) -> String {
    let text = tag.as_str();
    let primary = tag.primary();
    if primary == "x" || primary == "i" || IRREGULAR.contains(&text) {
        return text.to_owned();
    }
    let language = find(names::LANGUAGES, primary).unwrap_or(primary);
    let mut qualifiers = text.split('-').skip(1).take_while(|sub| sub.len() > 1).map(qualifier);
    let Some(first) = qualifiers.next() else {
        return language.to_owned();
    };
    let joined = qualifiers.fold(first.to_owned(), |all, next| fill(names::LOCALE_SEPARATOR, &all, next));
    fill(names::LOCALE_PATTERN, language, &joined)
}

/// The quotation marks of a language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quotes {
    /// The opening outer mark (`“` in English).
    pub open: &'static str,
    /// The closing outer mark (`”`).
    pub close: &'static str,
    /// The opening inner mark, for a quotation inside a quotation (`‘`).
    pub open_inner: &'static str,
    /// The closing inner mark (`’`).
    pub close_inner: &'static str,
}

/// The quotation marks CLDR gives a language: the tag's own row, else the row of the tag with its last subtag dropped (`de-DE-1996` is `de`), else CLDR's root, which is
/// the English marks (`“ ” ‘ ’`): a language with none, no language and private use have them. A region that CLDR gives its script (`zh-TW` is `zh-Hant-TW`) has the marks of
/// that script.
pub fn quotes(tag: &Tag) -> Quotes {
    let mut key = tag.as_str();
    let marks = loop {
        let row = quotes::QUOTES.binary_search_by(|(listed, _)| (*listed).cmp(key)).ok().and_then(|at| quotes::QUOTES.get(at));
        if let Some((_, marks)) = row {
            break marks;
        }
        match key.rfind('-') {
            Some(cut) => key = key.get(..cut).unwrap_or(""),
            None => break &quotes::ROOT,
        }
    };
    let [open, close, open_inner, close_inner] = *marks;
    Quotes { open, close, open_inner, close_inner }
}

/// The three types of script a run's faces are chosen by (the Character section's `Font`, `AsianFont` and `ComplexScriptFont`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScriptKind {
    /// Latin, Greek, Cyrillic and every script that is neither of the other two.
    Latin,
    /// Han (Simplified and Traditional), Hiragana, Katakana, Hangul, Bopomofo and Yi.
    Asian,
    /// Arabic, Hebrew, Syriac, Thaana, Thai, Lao, Khmer, Myanmar, Tibetan, Mongolian and the Indic scripts.
    Complex,
}

/// The script type of an ISO 15924 code, as CLDR's likely subtags give them (`Jpan`, `Hans` and the like stand for the East Asian scripts they hold).
fn kind_of_script(script: &str) -> ScriptKind {
    match script {
        "Hani" | "Hans" | "Hant" | "Hanb" | "Hira" | "Kana" | "Hrkt" | "Hang" | "Jpan" | "Kore" | "Bopo" | "Yiii" => ScriptKind::Asian,
        "Arab" | "Hebr" | "Syrc" | "Thaa" | "Thai" | "Laoo" | "Khmr" | "Mymr" | "Tibt" | "Mong" | "Deva" | "Beng" | "Guru" | "Gujr" | "Orya"
        | "Taml" | "Telu" | "Knda" | "Mlym" | "Sinh" => ScriptKind::Complex,
        _ => ScriptKind::Latin,
    }
}

/// The script type of a language, by CLDR's likely script of it: the tag's own script (`uz-Arab`), else the likely script of the language and region (`uz-AF`, `mn-CN`),
/// else of the language. `None` for a language CLDR gives no likely script (`qaa`), for no language and for private use.
pub fn script_of_language(tag: &Tag) -> Option<ScriptKind> {
    let likely = |key: &str| find(scripts::LIKELY_SCRIPT, key);
    let script = match tag.script() {
        Some(script) => script,
        None => tag.region().and_then(|region| likely(&format!("{}-{region}", tag.primary()))).or_else(|| likely(tag.primary()))?,
    };
    Some(kind_of_script(script))
}
