//! What draws a name the machine doesn't have.
//!
//! The specification leaves the stand-ins to the fonts task: search the host and bundled faces for the name, then take the stand-in for that
//! name, then the application's default. This module is that task's answer. It names no Microsoft face's metrics from memory.
//! Each metric-compatible pair is one the open face's own makers state (the Liberation fonts for Arial, Times New Roman and
//! Courier New, Carlito for Calibri, and Caladea for Cambria, which measurements bore
//! out), and everything else falls to a generic class. A stand-in is not promised to lay out as the original did: that is the
//! whole difference a missing font makes, and the report of this task measures it.
//!
//! Names are compared lower case with the spaces squeezed, which [`key`] does.

/// The broad kind of a face, which picks the default stand-in when no name matches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenericClass {
    /// Proportional faces with no serifs, and anything not recognised.
    Sans,
    /// Proportional faces with serifs.
    Serif,
    /// Faces whose letters share one width.
    Mono,
}

impl GenericClass {
    /// The class a PANOSE number gives, from its first four digits: family kind, serif style, weight and proportion. A number
    /// that isn't Latin text gives `None`. Serif styles 2 to 10 are serifed and 11 to 15 are not; proportion 9 is monospaced.
    /// (OpenType `OS/2` table, PANOSE classification, read 2026-10-31; a `FaceName`'s `Panose` holds the ten digits.)
    pub fn from_panose(panose: &[u8; 10]) -> Option<GenericClass> {
        let [kind, serif, _weight, proportion, ..] = *panose;
        if kind != 2 {
            return None;
        }
        Some(match (proportion, serif) {
            (9, _) => GenericClass::Mono,
            (_, 2..=10) => GenericClass::Serif,
            _ => GenericClass::Sans,
        })
    }
}

/// The most characters of a name that [`key`] reads, and the most it gives back.
const MAX_KEY_CHARS: usize = 256;

/// A name as compared: lower case, spaces squeezed, control characters dropped. At most 256 characters of the name are read and the
/// key holds at most 256 characters, counted after the fold (a letter may lower-case to several: `İ` gives `i` and a combining dot),
/// so that a name from a hostile file costs a bounded amount whatever its length. A key that reaches the cap is cut there, even
/// between the characters one letter folded to.
pub fn key(name: &str) -> String {
    let mut out = String::new();
    let mut kept = 0usize;
    let mut pending_space = false;
    for c in name.chars().take(MAX_KEY_CHARS) {
        if c.is_whitespace() {
            pending_space = !out.is_empty();
        } else if !c.is_control() {
            if std::mem::take(&mut pending_space) {
                // The space is kept only with a character after it, so that no key ends in one.
                if kept >= MAX_KEY_CHARS - 1 {
                    break;
                }
                out.push(' ');
                kept += 1;
            }
            // A letter folds to at most three characters: take them one at a time, so that the cap falls where it falls.
            for folded in c.to_lowercase() {
                if kept >= MAX_KEY_CHARS {
                    return out;
                }
                out.push(folded);
                kept += 1;
            }
        }
    }
    out
}

/// A name that says no font is specified: empty, or the number 0.
pub fn is_no_font(key: &str) -> bool {
    key.is_empty() || key == "0"
}

/// The open faces that stand in for a name, best first, as keys. Empty if the name has no twin. Arimo, Tinos and Cousine are
/// the Liberation faces' designs under their other names. `calibri light` takes Carlito as the nearest face, with no claim that
/// the widths match.
pub fn stand_ins(key: &str) -> &'static [&'static str] {
    match key {
        "arial" => &["liberation sans", "arimo"],
        "times new roman" => &["liberation serif", "tinos"],
        "courier new" => &["liberation mono", "cousine"],
        "calibri" | "calibri light" => &["carlito"],
        "cambria" => &["caladea"],
        _ => &[],
    }
}

/// Does `key` hold `phrase` as a word, not inside a longer one? A letter either side makes it part of a longer word; a digit or a
/// space or a mark does not (`courier10` holds `courier`, `monotype` does not hold `mono`).
fn has_word(key: &str, phrase: &str) -> bool {
    let mut from = 0usize;
    while let Some(rest) = key.get(from..) {
        let Some(at) = rest.find(phrase) else { return false };
        let (start, end) = (from + at, from + at + phrase.len());
        let before = key.get(..start).and_then(|b| b.chars().next_back());
        let after = key.get(end..).and_then(|a| a.chars().next());
        if !before.is_some_and(char::is_alphabetic) && !after.is_some_and(char::is_alphabetic) {
            return true;
        }
        from = start + rest.get(at..).and_then(|r| r.chars().next()).map_or(1, char::len_utf8);
    }
    false
}

/// The class a name says it is, by words in it: the names of generic kinds of face, the project's choice. `None` when it says
/// nothing (a name like `Zapfino`), which is not the same as sans.
pub fn class_of_name(key: &str) -> Option<GenericClass> {
    const MONO: &[&str] = &[
        "mono",
        "monospace",
        "courier",
        "consolas",
        "lucida console",
        "monaco",
        "menlo",
        "typewriter",
        "fixed",
        "fixedsys",
        "terminal",
        "code",
        "cousine",
        "inconsolata",
    ];
    const SERIF: &[&str] = &[
        "times",
        "georgia",
        "garamond",
        "palatino",
        "book antiqua",
        "bookman",
        "century schoolbook",
        "cambria",
        "constantia",
        "minion",
        "baskerville",
        "didot",
        "bodoni",
        "roman",
        "mincho",
        "song",
        "ming",
        "serif",
    ];
    // CJK serif names that run into other letters: STSong, SimSun, MingLiU.
    const SERIF_INSIDE: &[&str] = &["simsun", "stsong", "songti", "mingliu"];
    const SANS: &[&str] = &["sans", "gothic", "grotesque"];
    if MONO.iter().any(|w| has_word(key, w)) {
        Some(GenericClass::Mono)
    } else if SANS.iter().any(|w| has_word(key, w)) {
        Some(GenericClass::Sans)
    } else if SERIF.iter().any(|w| has_word(key, w)) || SERIF_INSIDE.iter().any(|w| key.contains(w)) {
        Some(GenericClass::Serif)
    } else {
        None
    }
}

/// The class of a name that has no twin: [`class_of_name`], else sans.
pub fn classify(key: &str) -> GenericClass {
    class_of_name(key).unwrap_or(GenericClass::Sans)
}

/// The faces tried for a class when nothing matched by name, as keys, before a face whose name says the class and the bundled face.
/// Carlito and the Liberation faces come first because the commonest face in the files this project reads is the one Carlito matches
/// (census). Each open twin is followed by the face its makers say it matches (the pairs of [`stand_ins`]), so a machine that
/// has none of the twins, as a Mac or a Windows machine out of the box has none, draws in a face of the same widths, not in the first
/// face whose name says the class (on a Mac that was a Korean "Gothic" face). A machine that has a twin draws in it as before.
pub fn class_defaults(class: GenericClass) -> &'static [&'static str] {
    match class {
        GenericClass::Sans => &["carlito", "calibri", "liberation sans", "arimo", "arial"],
        GenericClass::Serif => &["liberation serif", "tinos", "times new roman", "caladea", "cambria"],
        GenericClass::Mono => &["liberation mono", "cousine", "courier new"],
    }
}
