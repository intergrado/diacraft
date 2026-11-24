//! The manifest edited by rows: a row added is put after the last existing row, one taken out is
//! taken out with the white space before it and nothing else of the manifest moves: the rows' order, the root's attributes, every row a save
//! did not touch keep their bytes, in the quotes and spacing the file used (a manifest written again from a model fails this as soon as the
//! source used single quotes).
//!
//! A new row is written as the last row is: the element's name and the names of its attributes as the file wrote them (so a manifest whose
//! prefix is not `manifest` is added to in its own prefix), with the white space before the last row before it. Rows are found by namespace
//! (the manifest's, whatever the prefix), as the package layer finds them.

use std::collections::{HashMap, HashSet};

use diacraft_container::Work;
use diacraft_container::xml::ParseOptions;
use diacraft_container::xml_scan::{AttrSpan, Tag, TagKind, is_blank, scan_tags_with, tag_attributes};

use super::splice::{Splicer, escape_value};
use super::{WriteError, WriteWork};
use crate::limits::Limits;

/// The manifest's namespace.
const MANIFEST_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:manifest:1.0";

/// Where a row's `manifest:media-type` value lies (the bytes between its quotes, as written) and the quote the file wrote it in.
#[derive(Clone, Copy)]
struct MediaAt {
    value: (usize, usize),
    quote: u8,
}

/// A row of the manifest: its `manifest:full-path`, its unit (the white space before it, and the element) and where its media type is.
struct Row {
    path: String,
    lead: usize,
    end: usize,
    media: Option<MediaAt>,
}

/// The row being read: its path, where its unit starts, how it is written and where its media type is.
struct OpenRow {
    path: String,
    lead: usize,
    style: Option<Style>,
    media: Option<MediaAt>,
}

/// How the file writes a row.
struct Style {
    element: String,
    full_path: String,
    media_type: String,
    /// The white space before the last row.
    indent: String,
}

/// The rows of a manifest and where they lie.
pub(crate) struct Manifest<'a> {
    bytes: &'a [u8],
    rows: Vec<Row>,
    style: Option<Style>,
    /// Where the root's end tag starts.
    root_close: usize,
    /// The place in `rows` of the first row for the package itself (`/`), which is the one the package layer reads.
    root_row: Option<usize>,
    /// The media type the save states for the package: where the root row's is, and the text to put there.
    root_media: Option<(MediaAt, String)>,
}

struct Scan<'a> {
    bytes: &'a [u8],
    rows: Vec<Row>,
    style: Option<Style>,
    root_close: usize,
    root_row: Option<usize>,
    /// Each prefix in force, to the URIs bound to it, the innermost last, and how many each open element declared.
    bound: HashMap<String, Vec<String>>,
    declared: Vec<Vec<String>>,
    open_row: Option<OpenRow>,
    spans: Vec<AttrSpan>,
}

impl Scan<'_> {
    fn uri(&self, prefix: &str) -> Option<&str> {
        self.bound.get(prefix).and_then(|s| s.last()).map(String::as_str)
    }

    fn visit(&mut self, tag: &Tag<'_>) {
        if tag.kind == TagKind::Close {
            if tag.depth == 0 {
                self.root_close = tag.start;
            }
            for prefix in self.declared.pop().unwrap_or_default() {
                if let Some(stack) = self.bound.get_mut(&prefix) {
                    stack.pop();
                }
            }
            if tag.depth == 1 {
                self.finish_row(tag.end);
            }
            return;
        }
        self.spans.clear();
        let parsed = tag_attributes(self.bytes, tag.start, tag.end, &mut self.spans).is_some();
        let mut made = Vec::new();
        for i in 0..self.spans.len() {
            let Some(a) = self.spans.get(i).copied() else { break };
            let name = String::from_utf8_lossy(self.bytes.get(a.name.0..a.name.1).unwrap_or_default()).into_owned();
            let prefix = if name == "xmlns" { Some(String::new()) } else { name.strip_prefix("xmlns:").map(str::to_owned) };
            if let Some(prefix) = prefix {
                let value = crate::package::unescape(self.bytes.get(a.value.0..a.value.1).unwrap_or_default());
                self.bound.entry(prefix.clone()).or_default().push(value);
                made.push(prefix);
            }
        }
        self.declared.push(made);
        if tag.depth == 1 && parsed && self.is_row(tag.name) {
            self.read_row(tag);
        }
        if tag.kind == TagKind::Empty {
            self.close_empty(tag);
        }
    }

    /// An empty element is closed as it is opened: its declarations go out of force, and when it is the row being read the row ends with it. An
    /// empty element inside a row (the `manifest:algorithm` of its encryption data) does not end the row.
    fn close_empty(&mut self, tag: &Tag<'_>) {
        for prefix in self.declared.pop().unwrap_or_default() {
            if let Some(stack) = self.bound.get_mut(&prefix) {
                stack.pop();
            }
        }
        if tag.depth == 1 {
            self.finish_row(tag.end);
        }
    }

    /// The row being read ends at `end`: it is kept, and the first row for `/` is noted as the root.
    fn finish_row(&mut self, end: usize) {
        let Some(open) = self.open_row.take() else { return };
        if open.path == "/" && self.root_row.is_none() {
            self.root_row = Some(self.rows.len());
        }
        self.rows.push(Row { path: open.path, lead: open.lead, end, media: open.media });
        self.style = open.style.or(self.style.take());
    }

    /// Whether the element is `file-entry` of the manifest's namespace.
    fn is_row(&self, name: &str) -> bool {
        let (prefix, local) = name.split_once(':').unwrap_or(("", name));
        local == "file-entry" && self.uri(prefix) == Some(MANIFEST_URI)
    }

    /// Notes the row of the tag just opened: its path, where its unit starts and how it is written.
    fn read_row(&mut self, tag: &Tag<'_>) {
        let lead = match tag.text_before {
            Some((from, to)) if is_blank(self.bytes.get(from..to).unwrap_or_default()) => from,
            _ => tag.start,
        };
        let (mut path, mut full_path, mut media_type, mut media) = (None, None, None, None);
        for a in &self.spans {
            let name = String::from_utf8_lossy(self.bytes.get(a.name.0..a.name.1).unwrap_or_default()).into_owned();
            let Some((prefix, local)) = name.split_once(':') else { continue };
            if self.uri(prefix) != Some(MANIFEST_URI) {
                continue;
            }
            match local {
                "full-path" if path.is_none() => {
                    path = Some(crate::package::unescape(self.bytes.get(a.value.0..a.value.1).unwrap_or_default()));
                    full_path = Some(name.clone());
                }
                "media-type" if media_type.is_none() => {
                    media_type = Some(name.clone());
                    media = Some(MediaAt { value: a.value, quote: a.quote });
                }
                _ => {}
            }
        }
        let style = full_path.zip(media_type).map(|(full_path, media_type)| Style {
            element: tag.name.to_owned(),
            full_path,
            media_type,
            indent: String::from_utf8_lossy(self.bytes.get(lead..tag.start).unwrap_or_default()).into_owned(),
        });
        // A row with no path is not a row the package layer reads: it stays where it is.
        self.open_row = path.map(|path| OpenRow { path, lead, style, media });
    }
}

impl<'a> Manifest<'a> {
    /// The rows of the manifest `bytes`, charged to the work.
    pub(crate) fn scan(bytes: &'a [u8], limits: &Limits, work: &mut WriteWork) -> Result<Manifest<'a>, WriteError> {
        work.charge_scan(bytes.len(), limits)?;
        let mut scan = Scan {
            bytes,
            rows: Vec::new(),
            style: None,
            root_close: bytes.len(),
            root_row: None,
            bound: HashMap::new(),
            declared: Vec::new(),
            open_row: None,
            spans: Vec::new(),
        };
        let mut container = Work::default();
        let mut walked = 0u64;
        // The manifest is read as the package layer reads it, which accepts the OpenOffice.org document type declaration (an external id and no internal
        // subset), so the save reads its own file as the reader does.
        let options = ParseOptions { allow_external_doctype: true };
        scan_tags_with::<()>(bytes, &limits.manifest_xml(), options, &mut container, |tag| {
            walked += 1;
            scan.visit(tag);
            Ok(())
        })
        .map_err(|e| match e {
            diacraft_container::xml_scan::ScanError::Xml(error) => {
                WriteError::Original(crate::OdgError::Xml { part: crate::package::MANIFEST_PATH.to_owned(), error })
            }
            diacraft_container::xml_scan::ScanError::Visit(()) => WriteError::Mismatch("a manifest scan refused itself"),
        })?;
        work.elements_walked = work.elements_walked.saturating_add(walked);
        work.package.scan_events = work.package.scan_events.saturating_add(container.xml_events);
        Ok(Manifest { bytes, rows: scan.rows, style: scan.style, root_close: scan.root_close, root_row: scan.root_row, root_media: None })
    }

    /// States `media` as the media type of the package, in the root row, which keeps every other byte it has: the edit puts the text where the
    /// value of the row's `manifest:media-type` is, whole, in the quote the file wrote it in. A manifest with no root row, or one whose root row has
    /// no `manifest:media-type`, has no place to state it: a [`WriteError::MediaType`] and nothing is changed.
    pub(crate) fn state_root_media(&mut self, media: &str) -> Result<(), WriteError> {
        let Some(root) = self.root_row.and_then(|at| self.rows.get(at)) else {
            return Err(WriteError::MediaType(
                "the manifest has no root entry (a row for the package itself, with the full path /) to state the media type in",
            ));
        };
        let Some(at) = root.media else {
            return Err(WriteError::MediaType("the manifest's root entry has no media type to change"));
        };
        self.root_media = Some((at, media.to_owned()));
        Ok(())
    }

    /// The manifest with the rows whose paths are in `remove` taken out and a row for each of `add` (path, media type) after the last: the
    /// paths in `remove` are those of the entries a save drops, and a row is told by its path as written (a folder's with its slash or without).
    pub(crate) fn edit(
        self,
        remove: &HashSet<String>,
        add: &[(String, String)],
        limits: &Limits,
        work: &mut WriteWork,
    ) -> Result<Vec<u8>, WriteError> {
        let mut splicer = Splicer::new(self.bytes);
        for row in self.rows.iter().filter(|r| remove.contains(&r.path)) {
            splicer.remove(row.lead, row.end);
        }
        if let Some((at, media)) = &self.root_media {
            splicer.replace(at.value.0, at.value.1, escape_value(media, at.quote));
        }
        if !add.is_empty() {
            let style = self.style.as_ref();
            let (element, fp, mt, indent) = match style {
                Some(s) => (s.element.as_str(), s.full_path.as_str(), s.media_type.as_str(), s.indent.as_str()),
                None => ("manifest:file-entry", "manifest:full-path", "manifest:media-type", "\n "),
            };
            let at = self.rows.last().map_or(self.root_close, |r| r.end);
            let mut text = String::new();
            for (path, media) in add {
                text.push_str(&format!("{indent}<{element} {fp}=\"{}\" {mt}=\"{}\"/>", escape_value(path, b'"'), escape_value(media, b'"')));
            }
            splicer.insert(at, text);
        }
        splicer.apply(work, limits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const URI: &str = MANIFEST_URI;

    fn scanned<'a>(bytes: &'a [u8], work: &mut WriteWork) -> Manifest<'a> {
        match Manifest::scan(bytes, &Limits::default(), work) {
            Ok(m) => m,
            Err(e) => panic!("the manifest did not scan: {e}"),
        }
    }

    fn edited(bytes: &[u8], remove: &[&str], add: &[(&str, &str)]) -> String {
        let mut work = WriteWork::default();
        let remove: HashSet<String> = remove.iter().map(|s| (*s).to_owned()).collect();
        let add: Vec<(String, String)> = add.iter().map(|(p, m)| ((*p).to_owned(), (*m).to_owned())).collect();
        match scanned(bytes, &mut work).edit(&remove, &add, &Limits::default(), &mut work) {
            Ok(out) => String::from_utf8_lossy(&out).into_owned(),
            Err(e) => panic!("the manifest did not edit: {e}"),
        }
    }

    fn plain() -> String {
        format!(
            "<?xml version=\"1.0\"?>\n<manifest:manifest xmlns:manifest=\"{URI}\" manifest:version=\"1.3\">\n <manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"m\"/>\n <manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/>\n <manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/>\n</manifest:manifest>\n"
        )
    }

    #[test]
    fn a_row_taken_out_goes_with_the_white_space_before_it_and_a_row_added_follows_the_last() {
        let text = plain();
        let content = " <manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/>";
        assert_eq!(edited(text.as_bytes(), &["content.xml"], &[]), text.replace(&format!("\n{content}"), ""));
        assert_eq!(edited(text.as_bytes(), &["content.xml", "styles.xml", "/"], &[]).matches("file-entry").count(), 0);
        assert_eq!(edited(text.as_bytes(), &["nothing"], &[]), text, "no row of that path: nothing changes");
        let last = "<manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/>";
        let new = "\n <manifest:file-entry manifest:full-path=\"a&amp;b.png\" manifest:media-type=\"image/png\"/>";
        assert_eq!(edited(text.as_bytes(), &[], &[("a&b.png", "image/png")]), text.replace(last, &format!("{last}{new}")));
        // Both at once: the rows go and the new one follows the last row that was there.
        let both = edited(text.as_bytes(), &["styles.xml"], &[("a&b.png", "image/png")]);
        assert!(both.contains("a&amp;b.png") && !both.contains("styles.xml"), "{both}");
    }

    #[test]
    fn the_scan_counts_each_tag_and_finds_where_the_root_ends() {
        let text = plain();
        let mut work = WriteWork::default();
        let m = scanned(text.as_bytes(), &mut work);
        assert_eq!(work.elements_walked, 5, "the root and four rows... counted as tags: root open, three rows, root close");
        assert_eq!(m.rows.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(), ["/", "content.xml", "styles.xml"]);
        assert_eq!(m.root_close, text.find("</manifest:manifest>").unwrap_or(0));
        for row in &m.rows {
            assert!(
                text.get(row.lead..row.end).is_some_and(|unit| unit.starts_with("\n <manifest:file-entry") && unit.ends_with("/>")),
                "{:?}",
                text.get(row.lead..row.end)
            );
        }
    }

    #[test]
    fn a_row_with_children_is_one_unit_and_what_is_nested_is_no_row() {
        // The empty elements inside a row (an encrypted entry's `manifest:algorithm`) do not end it: the row ends at its own end tag.
        let text = format!(
            "<manifest:manifest xmlns:manifest=\"{URI}\">\n <manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"m\"/>\n <manifest:file-entry manifest:full-path=\"enc.xml\" manifest:media-type=\"text/xml\">\n  <manifest:encryption-data manifest:checksum-type=\"x\"><manifest:algorithm manifest:algorithm-name=\"a\"/><manifest:file-entry manifest:full-path=\"inner\" manifest:media-type=\"t\"/></manifest:encryption-data>\n </manifest:file-entry>\n</manifest:manifest>"
        );
        let mut work = WriteWork::default();
        let m = scanned(text.as_bytes(), &mut work);
        assert_eq!(m.rows.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(), ["/", "enc.xml"], "a file-entry inside a row is no row");
        let gone = edited(text.as_bytes(), &["enc.xml"], &[]);
        assert_eq!(
            gone,
            format!(
                "<manifest:manifest xmlns:manifest=\"{URI}\">\n <manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"m\"/>\n</manifest:manifest>"
            )
        );
        // A row added to it follows the whole of the last row, its children and its end tag.
        let more = edited(text.as_bytes(), &[], &[("x", "t")]);
        assert!(
            more.contains("</manifest:file-entry>\n <manifest:file-entry manifest:full-path=\"x\"") && more.ends_with("</manifest:manifest>"),
            "{more}"
        );
    }

    #[test]
    fn rows_are_found_by_namespace_and_a_new_row_is_written_as_the_last_one_is() {
        // Another prefix for the manifest's namespace.
        let text = format!("<m:manifest xmlns:m=\"{URI}\">\n\t<m:file-entry m:full-path=\"a\" m:media-type=\"t\"/>\n</m:manifest>");
        assert_eq!(
            edited(text.as_bytes(), &[], &[("b", "u")]),
            format!(
                "<m:manifest xmlns:m=\"{URI}\">\n\t<m:file-entry m:full-path=\"a\" m:media-type=\"t\"/>\n\t<m:file-entry m:full-path=\"b\" m:media-type=\"u\"/>\n</m:manifest>"
            )
        );
        // The elements in the default namespace, the attributes under a prefix.
        let text = format!("<manifest xmlns=\"{URI}\" xmlns:p=\"{URI}\"><file-entry p:full-path=\"a\" p:media-type=\"t\"/></manifest>");
        assert_eq!(
            edited(text.as_bytes(), &[], &[("b", "u")]),
            format!(
                "<manifest xmlns=\"{URI}\" xmlns:p=\"{URI}\"><file-entry p:full-path=\"a\" p:media-type=\"t\"/><file-entry p:full-path=\"b\" p:media-type=\"u\"/></manifest>"
            )
        );
        // No row to take the style from: the conventional names, and the new row goes before the root's end tag.
        let text = format!("<manifest:manifest xmlns:manifest=\"{URI}\"></manifest:manifest>");
        assert_eq!(
            edited(text.as_bytes(), &[], &[("b", "u")]),
            format!(
                "<manifest:manifest xmlns:manifest=\"{URI}\">\n <manifest:file-entry manifest:full-path=\"b\" manifest:media-type=\"u\"/></manifest:manifest>"
            )
        );
    }

    #[test]
    fn what_is_not_a_row_of_the_manifest_s_namespace_is_left_where_it_is() {
        let text = format!(
            "<manifest:manifest xmlns:manifest=\"{URI}\" xmlns:o=\"urn:other\">\n <manifest:other manifest:full-path=\"x\" manifest:media-type=\"t\"/>\n <o:file-entry o:full-path=\"y\" o:media-type=\"t\"/>\n <o:file-entry manifest:full-path=\"z\" manifest:media-type=\"t\"/>\n <manifest:file-entry manifest:media-type=\"t\"/>\n <manifest:file-entry manifest:full-path=\"w\"/>\n</manifest:manifest>"
        );
        let mut work = WriteWork::default();
        let m = scanned(text.as_bytes(), &mut work);
        assert_eq!(
            m.rows.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
            ["w"],
            "only a file-entry of the manifest's namespace with a path is a row"
        );
        assert!(m.style.is_none(), "a row with no media type gives no style: {}", m.style.is_some());
    }

    #[test]
    fn the_first_attribute_of_a_name_is_the_path_and_text_before_a_row_is_not_taken_with_it() {
        let text = format!(
            "<manifest:manifest xmlns:manifest=\"{URI}\" xmlns:m=\"{URI}\">note <manifest:file-entry m:full-path=\"a\" manifest:full-path=\"b\" m:media-type=\"t\" manifest:media-type=\"u\"/></manifest:manifest>"
        );
        let mut work = WriteWork::default();
        let m = scanned(text.as_bytes(), &mut work);
        assert_eq!(m.rows.len(), 1);
        assert_eq!(m.rows[0].path, "a", "the first full-path is the path");
        assert_eq!(m.style.as_ref().map(|s| (s.full_path.as_str(), s.media_type.as_str())), Some(("m:full-path", "m:media-type")));
        assert_eq!(
            text.get(m.rows[0].lead..m.rows[0].end),
            Some("<manifest:file-entry m:full-path=\"a\" manifest:full-path=\"b\" m:media-type=\"t\" manifest:media-type=\"u\"/>")
        );
        assert_eq!(
            edited(text.as_bytes(), &["a"], &[]),
            format!("<manifest:manifest xmlns:manifest=\"{URI}\" xmlns:m=\"{URI}\">note </manifest:manifest>")
        );
    }

    fn stated(bytes: &str, media: &str, remove: &[&str], add: &[(&str, &str)]) -> Result<String, WriteError> {
        let mut work = WriteWork::default();
        let mut manifest = scanned(bytes.as_bytes(), &mut work);
        manifest.state_root_media(media)?;
        let remove: HashSet<String> = remove.iter().map(|s| (*s).to_owned()).collect();
        let add: Vec<(String, String)> = add.iter().map(|(p, m)| ((*p).to_owned(), (*m).to_owned())).collect();
        let out = manifest.edit(&remove, &add, &Limits::default(), &mut work)?;
        Ok(String::from_utf8_lossy(&out).into_owned())
    }

    /// The root row's media type is put where it lies, whole, in the quote the file wrote it in, and no other byte of the manifest moves.
    #[test]
    fn the_root_row_s_media_type_is_replaced_where_it_lies_and_nothing_else_moves() {
        let text = format!(
            "<manifest:manifest xmlns:manifest='{URI}'>\n <manifest:file-entry manifest:media-type='old' manifest:full-path='/'/>\n <manifest:file-entry manifest:full-path='a' manifest:media-type='old'/>\n</manifest:manifest>"
        );
        assert_eq!(stated(&text, "new/type", &[], &[]).unwrap(), text.replacen("media-type='old'", "media-type='new/type'", 1));
        // The value is escaped for the quote it sits in, and for that one only.
        assert_eq!(stated(&text, "a\"b'c&d<e", &[], &[]).unwrap(), text.replacen("media-type='old'", "media-type='a\"b&apos;c&amp;d&lt;e'", 1));
        let double = text.replace('\'', "\"");
        assert_eq!(stated(&double, "a\"b'c&d", &[], &[]).unwrap(), double.replacen("media-type=\"old\"", "media-type=\"a&quot;b'c&amp;d\"", 1));
        // An empty value is replaced too.
        let empty = text.replacen("'old'", "''", 1);
        assert_eq!(stated(&empty, "x", &[], &[]).unwrap(), empty.replacen("media-type=''", "media-type='x'", 1));
    }

    #[test]
    fn the_root_row_that_counts_is_the_first_and_is_found_wherever_it_lies() {
        // Two rows for `/`: the first is the one the package layer reads, though others come before it.
        let two = format!(
            "<manifest:manifest xmlns:manifest=\"{URI}\">\n <manifest:file-entry manifest:full-path=\"a\" manifest:media-type=\"t\"/>\n <manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"first\"/>\n <manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"second\"/>\n</manifest:manifest>"
        );
        assert_eq!(stated(&two, "new", &[], &[]).unwrap(), two.replacen("\"first\"", "\"new\"", 1));
        // A row that is not in the manifest's namespace, or has no path, or is nested in a row, is no root row.
        let not_row = format!(
            "<manifest:manifest xmlns:manifest=\"{URI}\" xmlns:o=\"urn:other\">\n <o:file-entry o:full-path=\"/\" o:media-type=\"x\"/>\n <manifest:other manifest:full-path=\"/\" manifest:media-type=\"y\"/>\n <manifest:file-entry manifest:media-type=\"z\"/>\n <manifest:file-entry manifest:full-path=\"a\" manifest:media-type=\"t\"><manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"nested\"/></manifest:file-entry>\n</manifest:manifest>"
        );
        assert!(matches!(stated(&not_row, "new", &[], &[]), Err(WriteError::MediaType(why)) if why.contains("no root entry")));
        // The row for `/` after a row with children.
        let children = format!(
            "<manifest:manifest xmlns:manifest=\"{URI}\">\n <manifest:file-entry manifest:full-path=\"enc\" manifest:media-type=\"e\"><manifest:encryption-data/></manifest:file-entry>\n <manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"old\"/>\n</manifest:manifest>"
        );
        assert_eq!(stated(&children, "new", &[], &[]).unwrap(), children.replacen("\"old\"", "\"new\"", 1));
        // Two media types on the one row (the same namespace under two prefixes): the first, as the package layer reads it.
        let twice = format!(
            "<manifest:manifest xmlns:manifest=\"{URI}\" xmlns:m=\"{URI}\"><manifest:file-entry manifest:full-path=\"/\" m:media-type=\"one\" manifest:media-type=\"two\"/></manifest:manifest>"
        );
        assert_eq!(stated(&twice, "new", &[], &[]).unwrap(), twice.replacen("\"one\"", "\"new\"", 1));
    }

    #[test]
    fn a_manifest_that_cannot_state_a_media_type_is_refused_and_a_root_row_alone_is_enough() {
        // No root row; a root row with no media type (an attribute of another namespace is none).
        let none = format!(
            "<manifest:manifest xmlns:manifest=\"{URI}\">\n <manifest:file-entry manifest:full-path=\"a\" manifest:media-type=\"t\"/>\n</manifest:manifest>"
        );
        assert!(matches!(stated(&none, "x", &[], &[]), Err(WriteError::MediaType(why)) if why.contains("no root entry")));
        let bare = format!(
            "<manifest:manifest xmlns:manifest=\"{URI}\" xmlns:o=\"urn:o\">\n <manifest:file-entry manifest:full-path=\"/\" o:media-type=\"t\"/>\n</manifest:manifest>"
        );
        assert!(matches!(stated(&bare, "x", &[], &[]), Err(WriteError::MediaType(why)) if why.contains("no media type")));
        let only = format!(
            "<manifest:manifest xmlns:manifest=\"{URI}\"><manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"t\"/></manifest:manifest>"
        );
        assert_eq!(stated(&only, "x", &[], &[]).unwrap(), only.replacen("\"t\"", "\"x\"", 1));
    }

    #[test]
    fn the_root_row_s_media_type_and_the_rows_a_save_adds_and_takes_out_are_one_splice() {
        let text = plain();
        let last = "<manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/>";
        let out = stated(&text, "new", &["content.xml"], &[("a.png", "image/png")]).unwrap();
        let expected = text
            .replacen("manifest:full-path=\"/\" manifest:media-type=\"m\"", "manifest:full-path=\"/\" manifest:media-type=\"new\"", 1)
            .replacen("\n <manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/>", "", 1)
            .replacen(last, &format!("{last}\n <manifest:file-entry manifest:full-path=\"a.png\" manifest:media-type=\"image/png\"/>"), 1);
        assert_eq!(out, expected);
    }

    /// The manifest of OpenOffice.org, with its document type declaration, scans as the reader reads it and is edited with the declaration as it was.
    #[test]
    fn a_manifest_with_the_open_office_document_type_declaration_scans_and_keeps_it() {
        let doctype = "<!DOCTYPE manifest:manifest PUBLIC \"-//OpenOffice.org//DTD Manifest 1.0//EN\" \"Manifest.dtd\">";
        let text = plain().replacen("<manifest:manifest", &format!("{doctype}\n<manifest:manifest"), 1);
        let mut work = WriteWork::default();
        assert_eq!(scanned(text.as_bytes(), &mut work).rows.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(), ["/", "content.xml", "styles.xml"]);
        let out = edited(text.as_bytes(), &["styles.xml"], &[("Thumbnails/thumbnail.png", "image/png")]);
        assert!(out.contains(doctype) && !out.contains("styles.xml") && out.contains("Thumbnails/thumbnail.png"), "{out}");
        // A declaration with an internal subset is still no manifest.
        let subset = plain().replacen("<manifest:manifest", "<!DOCTYPE manifest:manifest [<!ENTITY x \"y\">]>\n<manifest:manifest", 1);
        assert!(Manifest::scan(subset.as_bytes(), &Limits::default(), &mut WriteWork::default()).is_err());
    }
}
