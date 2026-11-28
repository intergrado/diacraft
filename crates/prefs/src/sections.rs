//! The sections of `config.toml` the engine owns: proofing, AutoCorrect and the editing languages, each with its defaults and caps.
//!
//! **A key is added by adding a field.** Every struct has `#[serde(default)]`, so a file without the key reads as the default, and a file with keys this build does not know keeps them
//! (craft-settings merges key by key). The store (`profile.rs`) holds no list of keys. A value that does not fit a section's types resets that section alone and says so.
//!
//! **Nothing here is trusted.** `config.toml` is a file a person, a program or an attacker can write: [`EnginePrefs::bring_into_range`] caps every list, drops every entry that is not what it
//! must be (a file name that leaves its folder, a tag that is not a language tag, an empty or very long word) and says, in words for the person, what it did.

use craft_settings::Settings;
use serde::{Deserialize, Serialize};

use super::SCHEMA_VERSION;
use super::files::{FileKind, FileName};
use super::locale::{canonical_tag, tag_of_locale};

/// The most custom dictionaries a person has (design 6.2).
pub const MAX_DICTIONARIES: usize = 32;

/// The most entries of each AutoCorrect exception list (design 6.3).
pub const MAX_EXCEPTIONS: usize = 1_000;

/// The most characters of an exception (design 6.3: typed text of at most 64 characters).
pub const MAX_EXCEPTION_CHARS: usize = 64;

/// The most editing languages a person has.
pub const MAX_EDITING_LANGUAGES: usize = 32;

/// The most characters of a dictionary's name.
pub const MAX_DICTIONARY_NAME_CHARS: usize = 64;

/// What a dictionary's `language` says for every language.
pub const ALL_LANGUAGES: &str = "all";

/// What a program that keeps the person's settings knows of them: the sections of the engine. The window's type holds these and its own; the command line's is these alone.
///
/// A type that holds the engine's sections implements [`Prefs`] and the store works on it with no other change.
pub trait Prefs: Settings {
    /// The engine's sections.
    fn engine(&self) -> &EnginePrefs;

    /// The engine's sections, to change.
    fn engine_mut(&mut self) -> &mut EnginePrefs;

    /// Brings every value into range, and says, in words for the person, what it changed. A type with sections of its own overrides this to bring those in too and calls
    /// [`EnginePrefs::bring_into_range`] for the engine's.
    fn sanitize(&mut self) -> Vec<String> {
        self.engine_mut().bring_into_range()
    }
}

/// The engine's sections of `config.toml`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EnginePrefs {
    pub proofing: ProofingPrefs,
    pub autocorrect: AutoCorrectPrefs,
    pub language: LanguagePrefs,
}

impl Settings for EnginePrefs {
    const APP_ID: &'static str = "diacraft";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;
}

impl Prefs for EnginePrefs {
    fn engine(&self) -> &EnginePrefs {
        self
    }

    fn engine_mut(&mut self) -> &mut EnginePrefs {
        self
    }
}

impl EnginePrefs {
    /// Brings every section into range (see the module); says what it changed. What it says is empty for values already in range, so saying it twice says it once.
    pub fn bring_into_range(&mut self) -> Vec<String> {
        let mut notes = Vec::new();
        notes.extend(self.proofing.bring_into_range());
        notes.extend(self.autocorrect.bring_into_range());
        notes.extend(self.language.bring_into_range());
        notes
    }
}

/// `[proofing]`: the options of the Spelling checks (design 9.6) and the list of custom dictionaries (6.2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProofingPrefs {
    /// Check spelling as you type: marks while text is edited (on).
    pub check_as_you_type: bool,
    /// Hide spelling errors: the marks are not drawn, the check still runs (off).
    pub hide_errors: bool,
    /// Ignore words in UPPERCASE (on).
    pub ignore_uppercase: bool,
    /// Ignore words that contain numbers (on).
    pub ignore_numbers: bool,
    /// Ignore Internet and file addresses (on).
    pub ignore_internet: bool,
    /// Flag repeated words (on).
    pub flag_repeated: bool,
    /// Suggest from the main dictionary only: custom dictionaries are not used (off).
    pub main_dictionary_only: bool,
    /// Check grammar, the DiaCraft extra (off).
    pub grammar: bool,
    /// The custom dictionaries, in the order the person keeps them. None until the first Add, which makes `Custom.dic`.
    pub dictionaries: Vec<DictionaryEntry>,
}

impl Default for ProofingPrefs {
    fn default() -> Self {
        ProofingPrefs {
            check_as_you_type: true,
            hide_errors: false,
            ignore_uppercase: true,
            ignore_numbers: true,
            ignore_internet: true,
            flag_repeated: true,
            main_dictionary_only: false,
            grammar: false,
            dictionaries: Vec::new(),
        }
    }
}

/// One custom dictionary: a file of words in `proofing/dictionaries/` and the languages it serves.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DictionaryEntry {
    /// What the dialogs call it.
    pub name: String,
    /// The file, a plain name ending in `.dic` ([`FileName`]).
    pub file: String,
    /// `all`, or a language tag (`en` serves `en-US` and `en-GB`, `en-US` serves `en-US` alone).
    pub language: String,
    /// The dictionary Add and Add to Dictionary write to. One of the list.
    pub default: bool,
}

impl Default for DictionaryEntry {
    fn default() -> Self {
        DictionaryEntry { name: String::new(), file: String::new(), language: ALL_LANGUAGES.to_owned(), default: false }
    }
}

impl ProofingPrefs {
    /// Drops the dictionaries that name a file outside the folder or the same file twice, keeps `MAX_DICTIONARIES`, gives each a name and a language, and keeps one default.
    fn bring_into_range(&mut self) -> Vec<String> {
        let mut notes = Vec::new();
        let (mut unfit, mut repeated, mut unknown_language) = (0_usize, 0_usize, 0_usize);
        let mut seen: Vec<String> = Vec::new();
        let mut kept: Vec<DictionaryEntry> = Vec::new();
        let mut past = 0_usize;
        for mut entry in std::mem::take(&mut self.dictionaries) {
            // A list of a hundred thousand is not looked at past the cap: each entry would be checked against the ones kept.
            if kept.len() >= MAX_DICTIONARIES {
                past += 1;
                continue;
            }
            let Ok(file) = FileName::new(FileKind::Dictionary, &entry.file) else {
                unfit += 1;
                continue;
            };
            let key = file.file().to_lowercase();
            if seen.contains(&key) {
                repeated += 1;
                continue;
            }
            seen.push(key);
            entry.file = file.file();
            entry.name = clean_name(&entry.name, &file);
            match language_of(&entry.language) {
                Some(language) => entry.language = language,
                None => {
                    unknown_language += 1;
                    entry.language = ALL_LANGUAGES.to_owned();
                }
            }
            kept.push(entry);
        }
        let mut have_default = false;
        for entry in &mut kept {
            if entry.default && have_default {
                entry.default = false;
            }
            have_default |= entry.default;
        }
        if let (false, Some(first)) = (have_default, kept.first_mut()) {
            first.default = true;
        }
        self.dictionaries = kept;
        if unfit > 0 {
            notes.push(format!("dictionaries dropped: {unfit} (a file name that is not a plain dictionary file name)"));
        }
        if repeated > 0 {
            notes.push(format!("dictionaries dropped: {repeated} (a file that another dictionary already names)"));
        }
        if past > 0 {
            notes.push(format!("dictionaries dropped: {past} (past the {MAX_DICTIONARIES} that DiaCraft keeps)"));
        }
        if unknown_language > 0 {
            notes.push(format!("dictionaries now serving all languages: {unknown_language} (a language that is not a language tag)"));
        }
        notes
    }
}

/// The name a dictionary is shown by: its own, without control characters and cut, or the file's name.
fn clean_name(name: &str, file: &FileName) -> String {
    let clean: String = name.chars().filter(|c| !c.is_control()).take(MAX_DICTIONARY_NAME_CHARS).collect();
    let clean = clean.trim();
    if clean.is_empty() { file.stem().to_owned() } else { clean.to_owned() }
}

/// `all` for `all` or nothing, else the tag in canonical case; `None` for what is neither.
fn language_of(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || text.eq_ignore_ascii_case(ALL_LANGUAGES) { Some(ALL_LANGUAGES.to_owned()) } else { canonical_tag(text) }
}

/// `[autocorrect]`: the options of AutoCorrect and AutoFormat As You Type (design 6.3, 9.8), all on at first (the owner, O3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoCorrectPrefs {
    pub two_initial_capitals: bool,
    pub sentence_capitals: bool,
    pub day_names: bool,
    pub caps_lock: bool,
    pub replace_text: bool,
    pub use_suggestions: bool,
    pub smart_quotes: bool,
    pub fractions: bool,
    pub arrows_and_faces: bool,
    pub exceptions: AutoCorrectExceptions,
}

impl Default for AutoCorrectPrefs {
    fn default() -> Self {
        AutoCorrectPrefs {
            two_initial_capitals: true,
            sentence_capitals: true,
            day_names: true,
            caps_lock: true,
            replace_text: true,
            use_suggestions: true,
            smart_quotes: true,
            fractions: true,
            arrows_and_faces: true,
            exceptions: AutoCorrectExceptions::default(),
        }
    }
}

/// `[autocorrect.exceptions]`: the person's own entries of the two exception lists, added to the lists AutoCorrect carries.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoCorrectExceptions {
    /// Words that keep two initial capitals (`IDs`).
    pub initial_caps: Vec<String>,
    /// Abbreviations after which no sentence ends, so the next word keeps its case (`e.g.`).
    pub first_letter: Vec<String>,
}

impl AutoCorrectPrefs {
    fn bring_into_range(&mut self) -> Vec<String> {
        let mut notes = Vec::new();
        for (list, name) in [(&mut self.exceptions.initial_caps, "initial caps"), (&mut self.exceptions.first_letter, "first letter")] {
            let before = list.len();
            let mut kept: Vec<String> = Vec::with_capacity(before.min(MAX_EXCEPTIONS));
            for entry in std::mem::take(list) {
                let entry = entry.trim();
                let fit =
                    !entry.is_empty() && entry.chars().count() <= MAX_EXCEPTION_CHARS && !entry.chars().any(|c| c.is_control() || c.is_whitespace());
                if fit && kept.len() < MAX_EXCEPTIONS && !kept.iter().any(|k| k == entry) {
                    kept.push(entry.to_owned());
                }
            }
            if kept.len() < before {
                notes.push(format!(
                    "AutoCorrect {name} exceptions dropped: {} (empty, longer than {MAX_EXCEPTION_CHARS} characters, with a space, repeated, or past the {MAX_EXCEPTIONS} that DiaCraft keeps)",
                    before - kept.len()
                ));
            }
            *list = kept;
        }
        notes
    }
}

/// `[language]`: the person's editing languages.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LanguagePrefs {
    /// Language tags, the first the preferred one. Empty while nothing has decided it: opening a profile fills it from the system's language, and **a system that names none (`C`,
    /// `POSIX`, no language variable) leaves it empty**, so that nothing is stored and a later run under a real locale decides ([`LanguagePrefs::preferred`] is `en-US` meanwhile).
    pub editing: Vec<String>,
}

impl LanguagePrefs {
    /// The editing languages of a profile that has none, taken from the system (design call): its language, or **none** when it names none. `locale` is what the
    /// system says, in the POSIX or BCP 47 spelling (`de_DE.UTF-8`, `zh-Hans-CN`); `C`, `POSIX`, nothing and what is not a language give an empty list and not `en-US`: `en-US` saved
    /// by a first run under `LANG=C` would stay for good and a run under a real locale would never decide. A program that needs a language meanwhile reads [`LanguagePrefs::preferred`].
    pub fn default_for(locale: Option<&str>) -> LanguagePrefs {
        LanguagePrefs { editing: locale.and_then(tag_of_locale).into_iter().collect() }
    }

    /// The preferred editing language: the first, or `en-US` for a value with none.
    pub fn preferred(&self) -> &str {
        self.editing.first().map_or(DEFAULT_LANGUAGE, String::as_str)
    }

    /// Keeps the tags that are language tags, in canonical case, once each, at most `MAX_EDITING_LANGUAGES`.
    fn bring_into_range(&mut self) -> Vec<String> {
        let before = self.editing.len();
        let mut kept: Vec<String> = Vec::with_capacity(before.min(MAX_EDITING_LANGUAGES));
        for tag in std::mem::take(&mut self.editing) {
            if let Some(tag) = canonical_tag(&tag)
                && kept.len() < MAX_EDITING_LANGUAGES
                && !kept.contains(&tag)
            {
                kept.push(tag);
            }
        }
        let mut notes = Vec::new();
        if kept.len() < before {
            notes.push(format!(
                "editing languages dropped: {} (not a language tag, repeated, or past the {MAX_EDITING_LANGUAGES} that DiaCraft keeps)",
                before - kept.len()
            ));
        }
        self.editing = kept;
        notes
    }
}

/// The language the programs read for a profile that has decided none ([`LanguagePrefs::preferred`]): it is read and never stored.
pub const DEFAULT_LANGUAGE: &str = "en-US";
