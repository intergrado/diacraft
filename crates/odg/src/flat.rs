//! The flat drawing, `.fodg`: one XML document in place of a package.
//!
//! A flat drawing's root is `office:document`, and its children come in the order: `office:meta`, `office:settings`, `office:scripts`,
//! `office:font-face-decls`, `office:styles`, `office:automatic-styles`, `office:master-styles`, `office:body`. This module tells a flat file
//! from a package by its first bytes ([`Form::of`]) and a flat drawing by its root ([`head`]), and names the eight children ([`Child`]).
//! The reader (`read/flat.rs`) and the writer (`write/flat.rs`) are built on it.
//!
//! **Telling one by its first bytes**. After an optional UTF-8 byte-order mark, white space, an XML declaration, comments and
//! processing instructions, the first start tag must begin and end within the first 64 KiB ([`HEAD_BYTES`]) and name `document` in the office
//! namespace by its URI, whatever its prefix (the namespace may be declared on that very tag). A DOCTYPE before it, a file in UTF-16, or no such
//! tag gives nothing. The scan looks at no byte past 64 KiB, so what it costs does not depend on the file.

use diacraft_container::xml_scan::{AttrSpan, tag_attributes};

use crate::Variant;
use crate::ns::OFFICE;
use crate::package::{DRAWING_MEDIA_TYPE, TEMPLATE_MEDIA_TYPE, unescape};

/// How far into a file the first start tag may begin and end for the file to be told as a flat drawing.
pub const HEAD_BYTES: usize = 64 * 1024;

/// What a drawing is written as: a ZIP package (`.odg`, `.otg`) or one XML document (`.fodg`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Form {
    /// A ZIP package whose first entry is `mimetype` ([`crate::package`]).
    #[default]
    Package,
    /// One XML document whose root is `office:document`.
    Flat,
}

impl Form {
    /// The form `bytes` are read as: a flat document when they begin as XML does (after an optional byte-order mark and white space, a `<`) or
    /// in UTF-16 (read when a caller reads it directly, though [`crate::sniff`] does not tell it), and a package otherwise, so that bytes
    /// that are neither are refused as the archive reader refuses them (`not_zip`). Looks at the first [`HEAD_BYTES`] and no more.
    pub fn of(bytes: &[u8]) -> Form {
        if bytes.starts_with(b"PK") {
            return Form::Package;
        }
        if diacraft_container::xml_scan::is_utf16(bytes) {
            return Form::Flat;
        }
        let bom = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) { 3 } else { 0 };
        // The first byte that is no white space, within the head: a `<` is XML, anything else (or nothing within the head) is not.
        let window = bytes.get(bom..HEAD_BYTES.min(bytes.len())).unwrap_or_default();
        match window.iter().find(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n')) {
            Some(b'<') => Form::Flat,
            _ => Form::Package,
        }
    }
}

/// What a flat drawing's root says it is: the media type on `office:mimetype`, sorted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Claim {
    /// The drawing media type.
    Drawing,
    /// The drawing template's.
    Template,
    /// The media type of another kind of ODF document (a text, a spreadsheet, a presentation...): refused.
    Other(String),
    /// No `office:mimetype`: read as a drawing, with the warning, word `absent`.
    Absent,
    /// Another value, such as `…graphics-flat-xml`: read as a drawing, with the warning, word `unrecognised`.
    Unrecognised,
}

impl Claim {
    /// The claim of `media`.
    pub(crate) fn of(media: Option<&str>) -> Claim {
        match media {
            None => Claim::Absent,
            Some(DRAWING_MEDIA_TYPE) => Claim::Drawing,
            Some(TEMPLATE_MEDIA_TYPE) => Claim::Template,
            Some(other) if is_other_kind(other) => Claim::Other(other.to_owned()),
            Some(_) => Claim::Unrecognised,
        }
    }

    /// The variant [`crate::sniff`] gives: a drawing or a template by what it says, a drawing for what says nothing it knows, and none for another
    /// kind of document.
    pub(crate) fn variant(&self) -> Option<Variant> {
        match self {
            Claim::Drawing | Claim::Absent | Claim::Unrecognised => Some(Variant::Drawing),
            Claim::Template => Some(Variant::Template),
            Claim::Other(_) => None,
        }
    }
}

/// Whether `media` is the media type of an ODF document that is not a drawing (ODF 1.4 Part 3 §2.2.3 to §2.2.10): a text (and its master and web
/// forms), a spreadsheet, a presentation, a chart, a formula, an image, a database; each with or without `-template` or `-flat-xml`.
fn is_other_kind(media: &str) -> bool {
    let Some(kind) = media.strip_prefix("application/vnd.oasis.opendocument.") else { return false };
    let kind = kind.strip_suffix("-flat-xml").or_else(|| kind.strip_suffix("-template")).unwrap_or(kind);
    matches!(kind, "text" | "text-master" | "text-web" | "spreadsheet" | "presentation" | "chart" | "formula" | "image" | "database")
}

/// What the first bytes of a flat drawing say: its claim, and where the root's start tag and its `office:mimetype` value lie.
#[derive(Debug, Clone)]
pub(crate) struct Head {
    pub claim: Claim,
    /// The root's start tag: from its `<` to after its `>`.
    pub tag: (usize, usize),
    /// The value of `office:mimetype`, between its quotes, when the root has it.
    pub mimetype: Option<(usize, usize)>,
    /// The root's name as written, and the prefix of it (`""` for none): the prefix the office children are written under.
    pub name: String,
    /// The least prefix bound to the office namespace on the root's own tag, other than the empty one: what an attribute of that namespace is
    /// written under.
    pub attr_prefix: Option<String>,
    /// The namespace declarations of the root's tag: prefix (`""` for the default) and URI.
    pub declarations: Vec<(String, String)>,
}

impl Head {
    /// The prefix of the root element's name, `""` when it has none.
    pub(crate) fn prefix(&self) -> &str {
        self.name.split_once(':').map_or("", |(p, _)| p)
    }
}

/// The end of the start tag that begins at `at` (after its `>`), within the first [`HEAD_BYTES`]: a `>` inside a quoted value does not end it.
fn tag_end(bytes: &[u8], at: usize) -> Option<usize> {
    let mut quote: Option<u8> = None;
    let mut i = at;
    while i < HEAD_BYTES {
        let b = *bytes.get(i)?;
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'"' || b == b'\'' => quote = Some(b),
            None if b == b'>' => return Some(i + 1),
            None => {}
        }
        i += 1;
    }
    None
}

/// The index just after the first occurrence of `needle` at or after `from`, within the first [`HEAD_BYTES`].
fn after(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    let window = bytes.get(from..HEAD_BYTES.min(bytes.len()))?;
    let found = window.windows(needle.len()).position(|w| w == needle)?;
    Some(from + found + needle.len())
}

/// The root of the flat drawing in `bytes` by the rule, or `None`: see the module documentation.
pub(crate) fn head(bytes: &[u8]) -> Option<Head> {
    if diacraft_container::xml_scan::is_utf16(bytes) {
        return None;
    }
    let mut at = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) { 3 } else { 0 };
    let start = loop {
        // The first byte that is no white space, within the head: none there is no root in it.
        let window = bytes.get(at..HEAD_BYTES.min(bytes.len()))?;
        let skipped = window.iter().position(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'))?;
        at += skipped;
        let rest = window.get(skipped..)?;
        if rest.starts_with(b"<?") {
            at = after(bytes, at + 2, b"?>")?;
        } else if rest.starts_with(b"<!--") {
            at = after(bytes, at + 4, b"-->")?;
        } else if rest.starts_with(b"<!") {
            // A DOCTYPE, or anything else that is no start tag: not told.
            return None;
        } else if rest.first() == Some(&b'<') && rest.get(1).is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_' || *b == b':') {
            break at;
        } else {
            return None;
        }
    };
    let end = tag_end(bytes, start)?;
    let mut spans: Vec<AttrSpan> = Vec::new();
    tag_attributes(bytes, start, end, &mut spans)?;
    let name_end = bytes.get(start + 1..end)?.iter().position(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'/' | b'>'))? + start + 1;
    let name = std::str::from_utf8(bytes.get(start + 1..name_end)?).ok()?.to_owned();
    let mut declarations: Vec<(String, String)> = Vec::new();
    for a in &spans {
        let key = std::str::from_utf8(bytes.get(a.name.0..a.name.1)?).ok()?;
        let prefix = if key == "xmlns" { Some("") } else { key.strip_prefix("xmlns:") };
        if let Some(prefix) = prefix {
            declarations.push((prefix.to_owned(), unescape(bytes.get(a.value.0..a.value.1)?)));
        }
    }
    let uri_of = |prefix: &str| declarations.iter().rev().find(|(p, _)| p == prefix).map(|(_, u)| u.as_str());
    let (prefix, local) = name.split_once(':').unwrap_or(("", name.as_str()));
    if local != "document" || uri_of(prefix) != Some(OFFICE) {
        return None;
    }
    let attr_prefix = declarations.iter().filter(|(p, u)| !p.is_empty() && u == OFFICE).map(|(p, _)| p.clone()).min();
    // `office:mimetype`: an attribute whose prefix is bound to the office namespace here.
    let mut mimetype = None;
    for a in &spans {
        let key = std::str::from_utf8(bytes.get(a.name.0..a.name.1)?).ok()?;
        if let Some((p, "mimetype")) = key.split_once(':')
            && p != "xmlns"
            && uri_of(p) == Some(OFFICE)
        {
            mimetype = Some(a);
            break;
        }
    }
    let value = mimetype.map(|a| unescape(bytes.get(a.value.0..a.value.1).unwrap_or_default()));
    let claim = Claim::of(value.as_deref());
    Some(Head { claim, tag: (start, end), mimetype: mimetype.map(|a| a.value), name, attr_prefix, declarations })
}

/// The eight children of `office:document`, in the order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Child {
    Meta,
    Settings,
    Scripts,
    FontFaceDecls,
    Styles,
    AutomaticStyles,
    MasterStyles,
    Body,
}

impl Child {
    /// All eight, in the order.
    #[cfg(test)]
    pub(crate) const ALL: [Child; 8] =
        [Child::Meta, Child::Settings, Child::Scripts, Child::FontFaceDecls, Child::Styles, Child::AutomaticStyles, Child::MasterStyles, Child::Body];

    /// The child an office-namespace element named `local` is.
    pub(crate) fn of(local: &str) -> Option<Child> {
        Some(match local {
            "meta" => Child::Meta,
            "settings" => Child::Settings,
            "scripts" => Child::Scripts,
            "font-face-decls" => Child::FontFaceDecls,
            "styles" => Child::Styles,
            "automatic-styles" => Child::AutomaticStyles,
            "master-styles" => Child::MasterStyles,
            "body" => Child::Body,
            _ => return None,
        })
    }

    /// Its place in the order, from 0.
    pub(crate) fn rank(self) -> usize {
        self as usize
    }

    /// The local name.
    #[cfg(test)]
    pub(crate) fn local(self) -> &'static str {
        match self {
            Child::Meta => "meta",
            Child::Settings => "settings",
            Child::Scripts => "scripts",
            Child::FontFaceDecls => "font-face-decls",
            Child::Styles => "styles",
            Child::AutomaticStyles => "automatic-styles",
            Child::MasterStyles => "master-styles",
            Child::Body => "body",
        }
    }
}

/// The checksum a save puts on the flat document it made, to tell later that an index goes with the bytes (`ShapeIndex`'s fingerprint): the low 32 bits of a hash
/// of the whole document, and its length. **It is a check against an index of other bytes being used by mistake, and no identity of the document**: two different
/// documents of one length share it about once in four billion, the hash is the standard library's default (not kept stable between Rust releases, so an index made
/// by one build is not promised good for another), and nothing here is made to hold against bytes made to collide. A save that is given an index that does not go
/// with its bytes is refused (`write_mismatch`) only when the two differ in this checksum.
pub(crate) fn fingerprint(bytes: &[u8]) -> (u32, u64) {
    use std::hash::Hasher;
    let mut hasher = std::hash::DefaultHasher::new();
    hasher.write(bytes);
    // The low 32 bits of a 64-bit hash, as the package's checksum is 32 bits.
    ((hasher.finish() & 0xFFFF_FFFF) as u32, bytes.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const O: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";

    fn doc(root: &str) -> String {
        format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{root}<office:body/></office:document>")
    }

    #[test]
    fn the_form_is_told_by_the_first_bytes() {
        assert_eq!(Form::of(b"PK\x03\x04rest"), Form::Package);
        assert_eq!(Form::of(b"  \n<?xml version=\"1.0\"?><a/>"), Form::Flat);
        assert_eq!(Form::of(b"\xEF\xBB\xBF<a/>"), Form::Flat);
        assert_eq!(Form::of(b"\xFF\xFE<\0a\0/\0>\0"), Form::Flat);
        assert_eq!(Form::of(b"<\0?\0"), Form::Flat);
        assert_eq!(Form::of(b"not a file"), Form::Package);
        assert_eq!(Form::of(b""), Form::Package);
        // White space up to the head's end is no XML there; one byte less and the `<` is within the head.
        assert_eq!(Form::of(&[vec![b' '; HEAD_BYTES], b"<a/>".to_vec()].concat()), Form::Package);
        assert_eq!(Form::of(&[vec![b' '; HEAD_BYTES - 1], b"<a/>".to_vec()].concat()), Form::Flat);
        // The byte-order mark is not counted as white space, and not part of the window of the head's first byte.
        assert_eq!(Form::of(&[b"\xEF\xBB\xBF".to_vec(), vec![b'\n'; 10], b"<a/>".to_vec()].concat()), Form::Flat);
        assert_eq!(Form::of(b"\xEF\xBB\xBFx<a/>"), Form::Package);
    }

    #[test]
    fn the_root_is_found_by_its_uri_whatever_its_prefix() {
        let plain = doc(&format!("<office:document xmlns:office=\"{O}\" office:mimetype=\"application/vnd.oasis.opendocument.graphics\">"));
        let h = head(plain.as_bytes()).expect("a root");
        assert_eq!(h.claim, Claim::Drawing);
        assert_eq!(h.prefix(), "office");
        let other = format!("<o:document xmlns:o=\"{O}\" o:mimetype=\"application/vnd.oasis.opendocument.graphics-template\">");
        assert_eq!(head(doc(&other).as_bytes()).map(|h| h.claim), Some(Claim::Template));
        // A prefix that merely looks like the office one, with another URI.
        let wrong = "<office:document xmlns:office=\"urn:x\" office:mimetype=\"application/vnd.oasis.opendocument.graphics\">";
        assert!(head(doc(wrong).as_bytes()).is_none());
        // The default namespace.
        let default = format!("<document xmlns=\"{O}\" xmlns:o=\"{O}\" o:mimetype=\"application/vnd.oasis.opendocument.graphics\">");
        let h = head(doc(&default).as_bytes()).expect("a default-namespace root");
        assert_eq!((&h.claim, h.prefix(), h.attr_prefix.as_deref()), (&Claim::Drawing, "", Some("o")));
    }

    #[test]
    fn the_prefix_an_attribute_is_written_under_is_the_least_one_bound_to_the_office_namespace() {
        let h = |root: String| head(format!("{root}</x>").as_bytes()).expect("a root");
        // The default namespace only: no prefix to write an attribute under.
        assert_eq!(h(format!("<document xmlns=\"{O}\">")).attr_prefix, None);
        // Prefixes bound to other namespaces are not the office one; of those bound to it the least is taken, the default namespace never.
        let many = h(format!("<document xmlns=\"{O}\" xmlns:a=\"urn:a\" xmlns:z=\"{O}\" xmlns:m=\"{O}\" xmlns:b=\"urn:b\">"));
        assert_eq!(many.attr_prefix.as_deref(), Some("m"));
        assert_eq!(many.declarations.len(), 5);
        // A prefix rebound later in the tag is what it was last bound to.
        assert_eq!(h(format!("<o:document xmlns:o=\"urn:other\" xmlns:o=\"{O}\">")).prefix(), "o");
    }

    #[test]
    fn only_an_attribute_of_the_office_namespace_called_mimetype_is_the_media_type() {
        let media = "application/vnd.oasis.opendocument.graphics-template";
        let root = |attrs: &str| {
            head(format!("<office:document xmlns:office=\"{O}\" xmlns:x=\"urn:x\"{attrs}></office:document>").as_bytes()).expect("a root")
        };
        // In another namespace, unprefixed, or the declaration of a prefix called `mimetype`: none.
        for attrs in [
            format!(" x:mimetype=\"{media}\""),
            format!(" mimetype=\"{media}\""),
            format!(" xmlns:mimetype=\"{media}\""),
            format!(" office:mime=\"{media}\""),
        ] {
            let h = root(&attrs);
            assert_eq!((h.claim, h.mimetype), (Claim::Absent, None), "{attrs}");
        }
        // The one of the office namespace, wherever it stands among the others.
        let h = root(&format!(" x:mimetype=\"no\" office:version=\"1.4\" office:mimetype=\"{media}\""));
        assert_eq!(h.claim, Claim::Template);
        assert!(h.mimetype.is_some());
    }

    #[test]
    fn what_the_root_says_is_sorted() {
        let with = |m: &str| doc(&format!("<office:document xmlns:office=\"{O}\" office:mimetype=\"{m}\">"));
        let claim = |text: String| head(text.as_bytes()).map(|h| h.claim);
        assert_eq!(claim(with("application/vnd.oasis.opendocument.text")), Some(Claim::Other("application/vnd.oasis.opendocument.text".into())));
        assert_eq!(
            claim(with("application/vnd.oasis.opendocument.spreadsheet-template")),
            Some(Claim::Other("application/vnd.oasis.opendocument.spreadsheet-template".into()))
        );
        assert_eq!(claim(with("application/vnd.oasis.opendocument.graphics-flat-xml")), Some(Claim::Unrecognised));
        assert_eq!(claim(with("application/vnd.oasis.opendocument.graphcis")), Some(Claim::Unrecognised));
        assert_eq!(claim(doc(&format!("<office:document xmlns:office=\"{O}\">"))), Some(Claim::Absent));
        assert_eq!(Claim::Absent.variant(), Some(Variant::Drawing));
        assert_eq!(Claim::Other(String::new()).variant(), None);
    }

    #[test]
    fn nothing_that_is_no_start_tag_before_the_root_is_passed() {
        let root = format!("<office:document xmlns:office=\"{O}\">");
        for text in [
            format!("<!DOCTYPE x>{root}"),
            format!("<!-- c --><!DOCTYPE x>{root}"),
            format!("<![CDATA[x]]>{root}"),
            format!("text{root}"),
            "<?xml version=\"1.0\"?>".to_owned(),
            "<?xml".to_owned(),
            "<!-- unended".to_owned(),
            "<".to_owned(),
            String::new(),
        ] {
            assert!(head(text.as_bytes()).is_none(), "{text}");
        }
        // Comments and instructions are passed, and a `>` inside a value does not end the tag.
        let text = format!("<?xml version=\"1.0\"?><!-- a -->\n<?pi x?><office:document xmlns:office=\"{O}\" x=\">\" office:mimetype=\"m\">");
        assert!(head(text.as_bytes()).is_some());
    }

    #[test]
    fn what_follows_the_angle_bracket_must_begin_a_name() {
        let root = |first: &str| format!("<{first}:document xmlns:{first}=\"{O}\"></{first}:document>");
        // A letter and an underscore begin a name; a digit, a hyphen, a dot, a space, a slash and a quote do not.
        for ok in ["o", "_o", "Zo"] {
            assert!(head(root(ok).as_bytes()).is_some(), "{ok}");
        }
        for bad in ["1o", "-o", ".o", " o", "/o", "\"o"] {
            let text = format!("<{bad}:document xmlns:{bad}=\"{O}\"></{bad}:document>");
            assert!(head(text.as_bytes()).is_none(), "{bad}");
        }
    }

    #[test]
    fn the_start_tag_ends_within_the_head_at_the_byte_and_not_one_after() {
        let tag = |len: usize| {
            // A root start tag of exactly `len` bytes (its `>` at index `len - 1`): the name and a declaration, padded with an attribute.
            let open = format!("<office:document xmlns:office=\"{O}\" x=\"");
            let pad = len - open.len() - 2;
            format!("{open}{}\">", "a".repeat(pad))
        };
        assert!(head(tag(HEAD_BYTES).as_bytes()).is_some(), "the `>` is the last byte of the head");
        assert!(head(tag(HEAD_BYTES + 1).as_bytes()).is_none(), "one byte past it");
        // The same after an XML declaration and white space: the tag's end is what counts, from the start of the file.
        let declaration = "<?xml version=\"1.0\"?>\n";
        assert!(head(format!("{declaration}{}", tag(HEAD_BYTES - declaration.len())).as_bytes()).is_some());
        assert!(head(format!("{declaration}{}", tag(HEAD_BYTES - declaration.len() + 1)).as_bytes()).is_none());
    }

    #[test]
    fn the_start_tag_must_end_within_the_head() {
        let root = format!("<office:document xmlns:office=\"{O}\" x=\"{}\">", "a".repeat(HEAD_BYTES));
        assert!(head(root.as_bytes()).is_none());
        let comment = format!("<!--{}-->{}", "c".repeat(HEAD_BYTES), "<office:document xmlns:office=\"{O}\">");
        assert!(head(comment.as_bytes()).is_none());
    }

    #[test]
    fn the_eight_children_are_told_by_local_name_in_order() {
        for (rank, child) in Child::ALL.into_iter().enumerate() {
            assert_eq!(child.rank(), rank);
            assert_eq!(Child::of(child.local()), Some(child));
        }
        assert_eq!(Child::of("document"), None);
    }

    #[test]
    fn the_fingerprint_changes_with_a_byte_and_with_the_length() {
        // The low 32 bits of a 64-bit hash of the bytes, and the length.
        use std::hash::Hasher;
        let mut hasher = std::hash::DefaultHasher::new();
        hasher.write(b"<a/>");
        assert_eq!(fingerprint(b"<a/>"), ((hasher.finish() & 0xFFFF_FFFF) as u32, 4));
        let a = fingerprint(b"<a/>");
        assert_eq!(a, fingerprint(b"<a/>"));
        assert_ne!(a, fingerprint(b"<b/>"));
        assert_ne!(a.1, fingerprint(b"<a/> ").1);
    }
}
