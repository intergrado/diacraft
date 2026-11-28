//! Language tags and the system's language, as the settings need them.
//!
//! **Why not `diacraft_doc::lang`:** the document crate's tag type comes. The settings need only a well-formedness check and the canonical case, and they read
//! values from a file anyone can write, so [`canonical_tag`] is strict and small: a language of two or three letters, then at most seven subtags of one to eight letters or digits, at
//! most 255 bytes in all. When `diacraft_doc::lang` exists, this can call it.

/// The most bytes of a language tag (RFC 5646's practical limit, and the document crate's `MAX_TAG_BYTES`).
pub const MAX_TAG_BYTES: usize = 255;

/// The most subtags of a tag: the language and seven more.
const MAX_SUBTAGS: usize = 8;

/// `text` as a language tag in canonical case (`en-us` is `en-US`, `sr_latn_rs` is `sr-Latn-RS`), or `None` when it is not a well-formed tag. `_` is read as `-`, as POSIX spells
/// a locale.
pub fn canonical_tag(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || text.len() > MAX_TAG_BYTES {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    for (n, part) in text.split(['-', '_']).enumerate() {
        if n >= MAX_SUBTAGS || part.is_empty() || part.len() > 8 || !part.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return None;
        }
        let letters = part.bytes().all(|b| b.is_ascii_alphabetic());
        if n == 0 {
            if !letters || !(2..=3).contains(&part.len()) {
                return None;
            }
            out.push_str(&part.to_ascii_lowercase());
            continue;
        }
        out.push('-');
        if part.len() == 4 && letters {
            // A script: Latn.
            let mut chars = part.chars();
            if let Some(first) = chars.next() {
                out.push(first.to_ascii_uppercase());
            }
            out.push_str(&chars.as_str().to_ascii_lowercase());
        } else if (part.len() == 2 && letters) || (part.len() == 3 && part.bytes().all(|b| b.is_ascii_digit())) {
            // A region: US, 419.
            out.push_str(&part.to_ascii_uppercase());
        } else {
            out.push_str(&part.to_ascii_lowercase());
        }
    }
    Some(out)
}

/// The language tag a system locale names: `de_DE.UTF-8` is `de-DE`, `en_GB.utf8@euro` is `en-GB`. `C`, `POSIX`, an empty text and what is not a locale give `None`.
pub fn tag_of_locale(locale: &str) -> Option<String> {
    let locale = locale.trim();
    // A list (`LANGUAGE` holds `de_DE:fr`) is not one locale.
    if locale.contains(':') {
        return None;
    }
    // POSIX: language[_territory][.codeset][@modifier]. The codeset and the modifier say nothing of the language.
    let language = locale.split(['.', '@']).next().unwrap_or_default();
    if language.is_empty() || language.eq_ignore_ascii_case("C") || language.eq_ignore_ascii_case("POSIX") {
        return None;
    }
    canonical_tag(language)
}

/// Where the system's language is read from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum LocaleSource {
    /// The system's (`sys-locale`: the environment on Unix, the system's calls on Windows and macOS, the browser's languages on the web).
    #[default]
    System,
    /// What a test says, with no look at the system: `None` is a system that gives no language.
    Fixed(Option<String>),
}

impl LocaleSource {
    /// The locale this source gives, as the system spells it.
    pub fn read(&self) -> Option<String> {
        match self {
            LocaleSource::System => sys_locale::get_locale(),
            LocaleSource::Fixed(locale) => locale.clone(),
        }
    }
}
