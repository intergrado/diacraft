//! The check of a part the save made for a start tag that names an attribute twice, or declares one prefix twice.
//!
//! **Why it is there.** The check of the bytes made (`self_check`) scans the tags of `content.xml` and `styles.xml`, which sees no attribute at all, and reads back the tags the save
//! wrote *named attributes* on, which skips every `xmlns` declaration; and the engine does not read a splice's file again with the reader (a save into the file a drawing came from
//! is not compared, which keeps the cost of a save down). So a tag that held `xmlns:svg1` twice went out as a file the engine would not open, and the save said it had
//! saved it. The writer now looks at every part it made, before the file is put together: **each start tag names each attribute once, by the name as written**, which is what the XML
//! reader refuses a repeat by (a declaration is an attribute of its tag). A part that fails is a refusal in words that name the part and what is repeated, and no file is made.
//!
//! **What it does not do**: it resolves no namespace. Two attributes under two prefixes bound to one namespace are one name to the reader of ODF, which `check_tag` finds for the
//! attributes the save wrote; that one is no repeat for XML.
//!
//! **What it costs**: one pass over each part the save changed, the scan of its tags, and for each tag a sort of its attribute names. The bytes are counted apart from the scans'
//! ([`WriteWork::checked_name_bytes`]) and held to [`Limits::max_write_bytes`] on their own, so the check never makes a save past the scans' cap that was within it.

use diacraft_container::Work;
use diacraft_container::limits::XML_DEPTH_CEILING;
use diacraft_container::xml::ParseOptions;
use diacraft_container::xml_scan::{AttrSpan, ScanError, TagKind, is_utf16, scan_tags_with, tag_attributes};

use super::{WriteError, WriteWork, too_much_memory};
use crate::limits::Limits;

/// What the visit of a tag found.
enum Found {
    /// The tag at this byte names an attribute twice (`declaration`: the name is a namespace declaration).
    Repeat { at: usize, declaration: bool },
    /// The attributes of the tag at this byte could not be found.
    Unreadable(usize),
    /// The names of a tag could not be kept for want of memory.
    Memory,
}

/// Refuses the part `bytes`, called `name` in the words of the refusal, when a start tag of it names an attribute twice (a namespace declaration is one). A part in UTF-16 is left
/// alone: its tags cannot be scanned (the container's scan refuses it), no save splices into one and the writer makes none.
pub(crate) fn check_part(name: &str, bytes: &[u8], limits: &Limits, work: &mut WriteWork) -> Result<(), WriteError> {
    if is_utf16(bytes) {
        return Ok(());
    }
    let next = work.checked_name_bytes.saturating_add(bytes.len() as u64);
    if next > limits.max_write_bytes {
        return Err(WriteError::Limit { what: "bytes of parts checked for an attribute named twice", max: limits.max_write_bytes });
    }
    work.checked_name_bytes = next;
    let mut spans: Vec<AttrSpan> = Vec::new();
    let mut names: Vec<&[u8]> = Vec::new();
    let mut container = Work::default();
    // What a part the writer makes may hold, which the container's defaults are narrower than: a manifest of an old OpenDocument writer starts with a document type declaration that holds an
    // external id and nothing else (the reader and the manifest's own edit read it), and the document file of DiaCraft holds shapes nested to the model's cap (at most 407 elements,
    // `ext::docfile::parse_limits`). The scan keeps its own stack, so the depth costs no recursion: it is read at the container's ceiling, the deepest the container reads at all.
    let mut xml = limits.container.xml.clone();
    xml.max_depth = xml.max_depth.max(XML_DEPTH_CEILING);
    let options = ParseOptions { allow_external_doctype: true, ..ParseOptions::default() };
    let result = scan_tags_with(bytes, &xml, options, &mut container, |tag| {
        if tag.kind == TagKind::Close {
            return Ok(());
        }
        spans.clear();
        if tag_attributes(bytes, tag.start, tag.end, &mut spans).is_none() {
            return Err(Found::Unreadable(tag.start));
        }
        // A tag with fewer than two attributes cannot repeat one.
        if spans.len() < 2 {
            return Ok(());
        }
        names.clear();
        names.try_reserve(spans.len()).map_err(|_| Found::Memory)?;
        names.extend(spans.iter().map(|a| bytes.get(a.name.0..a.name.1).unwrap_or_default()));
        names.sort_unstable();
        match names.windows(2).find_map(|pair| match pair {
            [a, b] if a == b => Some(*a),
            _ => None,
        }) {
            Some(twice) => Err(Found::Repeat { at: tag.start, declaration: twice == b"xmlns" || twice.starts_with(b"xmlns:") }),
            None => Ok(()),
        }
    });
    match result {
        Ok(()) => Ok(()),
        Err(ScanError::Visit(Found::Repeat { at, declaration })) => Err(WriteError::SelfCheck(format!(
            "{name} has an element (at byte {at}) that {} twice",
            if declaration { "declares one namespace prefix" } else { "names one attribute" }
        ))),
        Err(ScanError::Visit(Found::Unreadable(at))) => {
            Err(WriteError::SelfCheck(format!("a start tag of {name} (at byte {at}) does not read as one")))
        }
        Err(ScanError::Visit(Found::Memory)) => Err(too_much_memory(limits)),
        Err(ScanError::Xml(e)) => Err(WriteError::SelfCheck(format!("{name} does not scan: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(part: &str) -> Result<(), WriteError> {
        check_part("content.xml", part.as_bytes(), &Limits::default(), &mut WriteWork::default())
    }

    fn why(r: Result<(), WriteError>) -> String {
        match r {
            Err(WriteError::SelfCheck(words)) => words,
            other => panic!("expected a refusal of the check, got {other:?}"),
        }
    }

    #[test]
    fn a_part_whose_tags_name_each_attribute_once_passes() {
        assert!(run(r#"<a xmlns="urn:a" xmlns:p="urn:p" xmlns:q="urn:p" p:x="1" q:x="2" x="3"><b/><b p:x="1"/><c><d x="1" y="1"/></c></a>"#).is_ok());
        assert!(run("<?xml version=\"1.0\"?>\n<!-- a --><a><b x=\"1\" y=\"x\"/>text<c/></a>").is_ok());
        assert!(run("<a/>").is_ok());
    }

    #[test]
    fn a_start_tag_that_names_an_attribute_twice_is_refused_in_words_that_say_which_part_and_what() {
        for part in [r#"<a x="1" x="2"/>"#, r#"<a><b p:x="1" y="2" p:x="1"></b></a>"#, r#"<a><b/><c><d y="2" z="3" y="4"/></c></a>"#] {
            let words = why(run(part));
            assert!(words.contains("content.xml") && words.contains("names one attribute twice"), "{part}: {words}");
        }
    }

    #[test]
    fn a_start_tag_that_declares_a_prefix_twice_is_refused_in_words_that_say_so() {
        for part in [
            r#"<a xmlns:svg1="urn:s" xmlns:svg1="urn:s"/>"#,
            r#"<a xmlns:p="urn:one"><b xmlns:q="urn:q" xmlns:p="urn:two" xmlns:p="urn:two"/></a>"#,
            r#"<a xmlns="urn:a" xmlns="urn:a"></a>"#,
        ] {
            let words = why(run(part));
            assert!(words.contains("content.xml") && words.contains("declares one namespace prefix twice"), "{part}: {words}");
        }
        // A prefix declared again by an element inside another is a nested scope and no repeat.
        assert!(run(r#"<a xmlns:p="urn:one"><b xmlns:p="urn:one"><c xmlns:p="urn:two"/></b></a>"#).is_ok());
    }

    #[test]
    fn a_tag_with_many_attributes_is_looked_at_once_and_a_repeat_at_its_end_is_found() {
        let names: String = (0..2_000).map(|i| format!(" n{i}=\"v\"")).collect();
        assert!(run(&format!("<a{names}/>")).is_ok());
        let words = why(run(&format!("<a{names} n0=\"w\"/>")));
        assert!(words.contains("names one attribute twice"), "{words}");
        let decls: String = (0..2_000).map(|i| format!(" xmlns:p{i}=\"urn:p\"")).collect();
        let words = why(run(&format!("<a{decls} xmlns:p1999=\"urn:p\"/>")));
        assert!(words.contains("declares one namespace prefix twice"), "{words}");
    }

    /// What the writers make that the container's defaults refuse: a manifest with the old document type declaration (an external id and no subset), and the document file's shapes
    /// nested to the model's cap (407 elements); a part past the container's ceiling (512) is still refused, as a part that does not scan.
    #[test]
    fn a_manifest_with_a_document_type_declaration_and_a_part_nested_to_the_models_cap_are_looked_through() {
        let manifest = r#"<?xml version="1.0"?><!DOCTYPE manifest:manifest PUBLIC "-//OpenOffice.org//DTD Manifest 1.0//EN" "Manifest.dtd"><manifest:manifest xmlns:manifest="urn:m" manifest:version="1.2"><manifest:file-entry manifest:full-path="/"/></manifest:manifest>"#;
        assert!(run(manifest).is_ok());
        let repeated = manifest.replace(r#"<manifest:file-entry manifest:full-path="/"/>"#, r#"<manifest:file-entry a="1" a="2"/>"#);
        assert!(why(run(&repeated)).contains("names one attribute twice"));
        let nested = |n: usize| format!("{}{}", "<a>".repeat(n), "</a>".repeat(n));
        assert!(run(&nested(407)).is_ok());
        assert!(run(&nested(512)).is_ok());
        assert!(why(run(&nested(513))).contains("content.xml does not scan"));
        // Any other document type declaration is refused, as the container refuses it.
        assert!(why(run(r#"<!DOCTYPE a [<!ENTITY x "y">]><a/>"#)).contains("does not scan"));
    }

    #[test]
    fn a_part_that_does_not_scan_is_refused_in_words_and_one_in_utf_16_is_left_to_the_writers_that_cannot_have_made_it() {
        assert!(why(run("<a><b></a>")).contains("content.xml does not scan"));
        let utf16: Vec<u8> = "<a x=\"1\" x=\"2\"/>".encode_utf16().flat_map(u16::to_le_bytes).collect();
        let bom: Vec<u8> = [0xFF, 0xFE].into_iter().chain(utf16).collect();
        assert!(check_part("meta.xml", &bom, &Limits::default(), &mut WriteWork::default()).is_ok());
    }

    #[test]
    fn the_bytes_looked_at_are_counted_apart_from_the_scans_and_held_to_the_writers_cap() {
        let part = "<a x=\"1\"><b/></a>";
        let mut work = WriteWork::default();
        assert!(check_part("content.xml", part.as_bytes(), &Limits::default(), &mut work).is_ok());
        assert!(check_part("styles.xml", part.as_bytes(), &Limits::default(), &mut work).is_ok());
        assert_eq!(work.checked_name_bytes, 2 * part.len() as u64);
        assert_eq!(work.scan_bytes, 0, "the scans of the save are charged as they were");
        let tight = Limits { max_write_bytes: part.len() as u64, ..Limits::default() };
        let mut work = WriteWork::default();
        assert!(check_part("content.xml", part.as_bytes(), &tight, &mut work).is_ok(), "exactly the cap");
        assert!(matches!(check_part("styles.xml", part.as_bytes(), &tight, &mut work), Err(WriteError::Limit { .. })), "one byte past it");
    }
}
