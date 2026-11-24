//! The names of DiaCraft's extension in a drawing (the proposal): every one is an `NCName` in
//! the namespace `urn:app.diacraft:extension`.
//!
//! **A tier-2 attribute's name is one of these** (the local name, on the `style:graphic-properties` of a shape's own automatic style):
//!
//! | Local name | Holds |
//! |---|---|
//! | `version` | `1`: written with any of the others |
//! | `id` | the shape's `ID` |
//! | `links`, `filled`, `glue`, `face`, `text-end` | what gives each; this crate reads `links` and keeps the others |
//! | `name`, `name-u` | the shape's `Name` and `NameU`, when they are not what `draw:name` gives both |
//! | `master`, `master-shape` | the shape's `Master` and `MasterShape` |
//! | `cells` | the cells of the shape's own sheet that the reader would not rebuild: one `Sheet` fragment (XML text) of `Cell` elements |
//! | `s-<name>` or `s-<name>-<ix>` | one section the reader would not rebuild, whole: one `Sheet` fragment of one `Section` element, `<name>` its `N` and `<ix>` its `IX` (a `Geometry` section) |
//! | `text` | the shape's whole text: one `Text` fragment, its characters and its `cp`, `pp`, `tp` and `fld` markers |
//! | `vsdx` | the shape's VSDX data that no ODF attribute says (its attributes and elements, `Data1` to `Data3`, its deleted flag, its kind, its slots): one `shape` fragment of the whole-model codec, with no cell in it (`shell.rs`) |
//! | `foreign` | the shape's foreign data beyond its picture: one `shape` fragment that holds only it |
//! | `gone` | what the reader would rebuild and the model lacks: space-separated `c.<name>` (a cell), `s.<name>.<ix>` (a section; `_` for no `IX`), `name`, `name-u`, `text` |
//! | `fp-text`, `fp-format`, `fp-vsdx`, `fp-foreign`, `fp-name`, `fp-master`, `fp-s-<name>[-<ix>]`, `fp-c-<name>` | the fingerprint of the native outputs the item of that key determined (`fingerprint.rs`, tier 3): 16 lowercase hexadecimal digits. Any other `fp-` name is a newer build's: read as nothing, kept as it is |
//!
//! `<name>` is the cell's or section's name **escaped as an `NCName`**: a letter or digit stands for itself and any other byte of its UTF-8 is `_`
//! and two upper-case hexadecimal digits (so `_` itself is `_5F`, a `.` is `_2E`), which makes every escaped name a run of letters, digits and
//! `_`, and leaves `-` and `.` free to separate.

use diacraft_doc::SectionKey;

/// The namespace (`diacraft_doc::DIACRAFT_URN`).
pub(crate) const URN: &str = diacraft_doc::DIACRAFT_URN;

/// The prefix DiaCraft writes for the namespace.
pub(crate) const PREFIX: &str = "diacraft";

/// The version of the extension this build writes and reads: a reader that meets a higher one reads the names it knows, keeps the rest and warns
/// once.
pub(crate) const VERSION: &str = diacraft_doc::DIACRAFT_VERSION;

/// The attribute names of one tier-2 shape that are not sections.
pub(crate) const ID: &str = "id";
pub(crate) const NAME: &str = "name";
pub(crate) const NAME_U: &str = "name-u";
pub(crate) const MASTER: &str = "master";
pub(crate) const MASTER_SHAPE: &str = "master-shape";
/// The kind of shape a shape is when it is not what the reader gives (`Shape`): `Guide`, which no ODF element is.
pub(crate) const TYPE: &str = "type";
pub(crate) const CELLS: &str = "cells";
pub(crate) const TEXT: &str = "text";
pub(crate) const GONE: &str = "gone";
/// The VSDX data of the shape that no ODF attribute says: its attributes and elements, `Data1` to `Data3`, its deleted flag, its kind and its slots, as one fragment of the
/// codec (a shape's shell, `shell.rs`).
pub(crate) const VSDX: &str = "vsdx";
/// The foreign data of the shape beyond its picture: one fragment of the codec, in the shell of a shape that holds nothing else.
pub(crate) const FOREIGN: &str = "foreign";
const SECTION: &str = "s-";

/// Whether the attribute `local` of the extension is one the writer owns: it writes it, replaces it and takes it out as the cells it stands for
/// change (the last row of "Styles"). **The fingerprints of tier 3 are owned**: a save that rewrites an item writes its fingerprint
/// again or takes it out, since one left as the file had it would call the item stale on the next read. `links`, `filled`, `glue`, `face` and `text-end`, the
/// foreign names, an `fp-` name this build does not know and `version` are kept as the file has them.
pub(crate) fn owned(local: &str) -> bool {
    matches!(local, VERSION_ATTR | ID | NAME | NAME_U | MASTER | MASTER_SHAPE | TYPE | CELLS | TEXT | GONE | VSDX | FOREIGN)
        || section_of_attr(local).is_some()
        || super::fingerprint::Key::parse(local).is_some()
}

/// The attribute that says the version of the extension: owned by the writer, which sets it while any attribute of the extension is on the element and takes
/// it out with the last.
pub(crate) const VERSION_ATTR: &str = "version";

/// A name escaped as an `NCName` (see the module documentation).
pub(crate) fn escape(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() {
            out.push(char::from(b));
        } else {
            out.push('_');
            out.push(char::from(HEX[usize::from(b >> 4)]));
            out.push(char::from(HEX[usize::from(b & 15)]));
        }
    }
    out
}

const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// The name `escape` made `text` from, or `None` for text that is not an escaped name.
pub(crate) fn unescape(text: &str) -> Option<String> {
    let mut bytes: Vec<u8> = Vec::with_capacity(text.len());
    let mut it = text.bytes();
    while let Some(b) = it.next() {
        if b == b'_' {
            let hi = hex(it.next()?)?;
            let lo = hex(it.next()?)?;
            bytes.push(hi << 4 | lo);
        } else if b.is_ascii_alphanumeric() {
            bytes.push(b);
        } else {
            return None;
        }
    }
    String::from_utf8(bytes).ok()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// The attribute that holds the section `key`: `s-Geometry-0`, `s-User`.
pub(crate) fn section_attr(key: &SectionKey) -> String {
    match key.ix {
        Some(ix) => format!("{SECTION}{}-{ix}", escape(key.name.as_str())),
        None => format!("{SECTION}{}", escape(key.name.as_str())),
    }
}

/// The section an attribute holds, from its local name; `None` for a name that is not a section's.
pub(crate) fn section_of_attr(local: &str) -> Option<SectionKey> {
    let rest = local.strip_prefix(SECTION)?;
    let (name, ix) = match rest.split_once('-') {
        Some((name, ix)) => (name, Some(canonical(ix)?)),
        None => (rest, None),
    };
    let name = unescape(name).filter(|n| !n.is_empty())?;
    Some(SectionKey::new(&name, ix))
}

/// A number written with digits only and no leading zero, as `section_attr` writes one.
fn canonical(text: &str) -> Option<u32> {
    let n: u32 = text.parse().ok()?;
    (n.to_string() == text).then_some(n)
}

/// What one token of `gone` names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Gone {
    Cell(String),
    Section(SectionKey),
    Name,
    NameU,
    Text,
}

impl Gone {
    /// The token as it is written.
    pub(crate) fn token(&self) -> String {
        match self {
            Gone::Cell(name) => format!("c.{}", escape(name)),
            Gone::Section(key) => format!("s.{}.{}", escape(key.name.as_str()), key.ix.map_or_else(|| "_".to_owned(), |ix| ix.to_string())),
            Gone::Name => NAME.to_owned(),
            Gone::NameU => NAME_U.to_owned(),
            Gone::Text => TEXT.to_owned(),
        }
    }

    /// The thing a token names, or `None` for a token that names none (read as nothing).
    pub(crate) fn parse(token: &str) -> Option<Gone> {
        match token {
            NAME => return Some(Gone::Name),
            NAME_U => return Some(Gone::NameU),
            TEXT => return Some(Gone::Text),
            _ => {}
        }
        if let Some(name) = token.strip_prefix("c.") {
            return unescape(name).filter(|n| !n.is_empty()).map(Gone::Cell);
        }
        let rest = token.strip_prefix("s.")?;
        let (name, ix) = rest.split_once('.')?;
        let ix = if ix == "_" { None } else { Some(canonical(ix)?) };
        let name = unescape(name).filter(|n| !n.is_empty())?;
        Some(Gone::Section(SectionKey::new(&name, ix)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_escapes_to_an_ncname_and_back() {
        for name in ["PinX", "Row_1", "a.b", "a-b", "x y", "é", "", "_", "__", "Prop.Row_2", "日本", "\u{1F600}", "a\u{0}b"] {
            let e = escape(name);
            assert!(e.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'), "{name:?} -> {e}");
            assert_eq!(unescape(&e).as_deref(), Some(name), "{name:?} -> {e}");
        }
        assert_eq!(escape("PinX"), "PinX");
        assert_eq!(escape("_"), "_5F");
        assert_eq!(escape("a.b"), "a_2Eb");
        assert_eq!(escape("é"), "_C3_A9");
    }

    #[test]
    fn text_that_is_not_an_escaped_name_is_none() {
        for bad in ["a b", "a-b", "a.b", "_", "_4", "_4g", "_c3", "é", "_C3"] {
            assert_eq!(unescape(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_section_names_its_attribute_and_the_attribute_names_the_section() {
        let cases = [
            (SectionKey::new("Geometry", Some(0)), "s-Geometry-0"),
            (SectionKey::new("Geometry", Some(12)), "s-Geometry-12"),
            (SectionKey::new("User", None), "s-User"),
            (SectionKey::new("My.Section", None), "s-My_2ESection"),
        ];
        for (key, attr) in cases {
            assert_eq!(section_attr(&key), attr);
            assert_eq!(section_of_attr(attr), Some(key));
            assert!(owned(attr));
        }
        for not in ["s-", "s", "s-Geometry-01", "s-Geometry-x", "s-a-b-c", "s-a b", "cells2", "fp-cells", "links", "version"] {
            assert_eq!(section_of_attr(not), None, "{not}");
        }
    }

    #[test]
    fn the_writer_owns_what_it_writes_and_keeps_every_other_name() {
        for yes in [
            "version",
            "id",
            "name",
            "name-u",
            "master",
            "master-shape",
            "cells",
            "text",
            "gone",
            "vsdx",
            "foreign",
            "s-User",
            "fp-text",
            "fp-format",
            "fp-vsdx",
            "fp-foreign",
            "fp-name",
            "fp-master",
            "fp-s-Geometry-0",
            "fp-c-EndArrow",
        ] {
            assert!(owned(yes), "{yes}");
        }
        for no in ["links", "filled", "glue", "face", "text-end", "fp-cells", "fp-", "fp-future", "future", ""] {
            assert!(!owned(no), "{no}");
        }
    }

    #[test]
    fn a_gone_token_names_what_it_names() {
        let tokens = [
            Gone::Cell("PinX".to_owned()),
            Gone::Cell("a b".to_owned()),
            Gone::Section(SectionKey::new("Geometry", Some(2))),
            Gone::Section(SectionKey::new("User", None)),
            Gone::Name,
            Gone::NameU,
            Gone::Text,
        ];
        for t in tokens {
            assert_eq!(Gone::parse(&t.token()), Some(t.clone()), "{t:?}");
        }
        for bad in ["", "c.", "s.User", "s.User.01", "x", "c.a b", "s._.0", "s..0"] {
            assert_eq!(Gone::parse(bad), None, "{bad}");
        }
    }
}
