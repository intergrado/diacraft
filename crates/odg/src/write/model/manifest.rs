//! The manifest written from the model (the design's §3.4 rows for the root's version and the manifest's rows).
//!
//! **What it lists**: the root row `/` (the kind of the package, and the attributes the model holds for it beside the media type), a row for each of `content.xml`, `styles.xml`,
//! `meta.xml` and `settings.xml` (the attributes the model holds for it, else the media type `text/xml`), and a row for each package entry the model holds, with its media type and the other
//! attributes of its row as the file had them; a folder's row is the folder's. Every entry but `mimetype` and those under `META-INF/` is listed, so that no program is offered a repair. **The
//! order is the spec's**: the root, the four parts, then the entries in the model's order, and the rows the splice adds (pictures, the thumbnail, DiaCraft's document file) after them.
//!
//! **What it does not list**: a row the model holds for an entry the package does not have (a picture of a file that is written under another name, an entry a save took out): it names nothing, and
//! LibreOffice would read it as a file the package lacks. **The root keeps its attributes** (the namespace declarations among them, and `manifest:version`), as the model holds them.

use std::sync::Arc;

use diacraft_container::xml::{El, Node};
use diacraft_doc::{Document, Part, RawAttrs};

use super::{WORDS_NAMES, ensure_declarations, root_of, set_version};
use crate::base::VERSION;
use crate::limits::Limits;
use crate::package::MANIFEST_PATH;
use crate::write::{WriteError, WriteWork, media_for};

/// The manifest's namespace.
const MANIFEST_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:manifest:1.0";

/// A row: its full path and its other attributes, in order, with the media type among them.
pub(super) struct Row {
    pub path: String,
    pub attrs: Vec<(String, String)>,
}

/// The local part of an attribute name.
fn local(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, l)| l)
}

/// Whether an attribute name is the media type's (the prefix is whatever the root binds to the manifest namespace).
fn is_media_type(name: &str) -> bool {
    name.rsplit_once(':').map_or(name, |(_, local)| local) == "media-type"
}

/// The attributes of the row the model holds for `name`, if it holds one.
fn held_row<'a>(held: &'a [(Arc<str>, RawAttrs)], name: &str) -> Option<&'a RawAttrs> {
    held.iter().find(|(n, _)| &**n == name).map(|(_, a)| a)
}

/// The attributes of a row: those the model holds in their order, with the media type put in the place of the held one (forced for the root, whose media type is the kind the package says) or
/// added when there is none.
fn row_attrs(held: Option<&RawAttrs>, media: &str, force: bool, default: &[(&str, &str)]) -> Vec<(String, String)> {
    let Some(held) = held else {
        let mut attrs: Vec<(String, String)> = default.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
        attrs.push(("manifest:media-type".to_owned(), media.to_owned()));
        return attrs;
    };
    let mut attrs: Vec<(String, String)> = Vec::new();
    let mut found = false;
    for (k, v) in held.iter() {
        if is_media_type(k) && !found {
            found = true;
            attrs.push((k.to_owned(), if force { media.to_owned() } else { v.to_owned() }));
        } else {
            attrs.push((k.to_owned(), v.to_owned()));
        }
    }
    if !found {
        attrs.push(("manifest:media-type".to_owned(), media.to_owned()));
    }
    attrs
}

/// The row of a package entry the model holds, or none for an entry under `META-INF/` that no row ever listed.
pub(super) fn part_row(part: &Part) -> Option<Row> {
    let listed = !part.media_type.is_empty() || !part.manifest.is_empty();
    if part.name.starts_with("META-INF/") && !listed {
        return None;
    }
    let media = if part.media_type.is_empty() { media_for(&part.name) } else { &*part.media_type };
    let mut attrs = vec![("manifest:media-type".to_owned(), media.to_owned())];
    attrs.extend(part.manifest.iter().filter(|(k, _)| !is_media_type(k)).map(|(k, v)| (k.to_owned(), v.to_owned())));
    Some(Row { path: part.name.to_string(), attrs })
}

/// The rows of a package: the root's, the four parts', then `entries`'.
pub(super) fn rows(doc: &Document, media: &str, entries: impl Iterator<Item = Row>) -> Vec<Row> {
    let held: &[(Arc<str>, RawAttrs)] = doc.ext.get().map_or(&[], |d| d.odf.manifest_rows.as_slice());
    // The root row says the version the package says, whatever the file the model was read from said (`super::set_version`).
    let mut root = row_attrs(held_row(held, "/"), media, true, &[]);
    match root.iter_mut().find(|(k, _)| local(k) == "version") {
        Some(slot) => slot.1 = VERSION.to_owned(),
        None => root.insert(0, ("manifest:version".to_owned(), VERSION.to_owned())),
    }
    let mut out = vec![Row { path: "/".to_owned(), attrs: root }];
    for part in ["content.xml", "styles.xml", "meta.xml", "settings.xml"] {
        out.push(Row { path: part.to_owned(), attrs: row_attrs(held_row(held, part), "text/xml", false, &[]) });
    }
    out.extend(entries);
    out
}

/// The manifest's bytes.
pub(super) fn write(doc: &Document, rows: &[Row], limits: &Limits, work: &mut WriteWork) -> Result<Vec<u8>, WriteError> {
    if rows.len() > limits.max_manifest_entries {
        return Err(WriteError::Limit { what: "rows in the manifest", max: limits.max_manifest_entries as u64 });
    }
    let held = doc.ext.get().map(|d| d.odf.roots.as_slice());
    let mut attrs = match held.and_then(|list| list.iter().find(|(n, _)| &**n == MANIFEST_PATH)) {
        Some(_) => root_of(held, MANIFEST_PATH, VERSION),
        None => vec![("manifest:version".to_owned(), VERSION.to_owned())],
    };
    let declared = ensure_declarations(&mut attrs, &[("manifest", MANIFEST_URI)])?;
    set_version(&mut attrs, &declared, MANIFEST_URI, VERSION);
    let mut children: Vec<Node> = Vec::new();
    for row in rows {
        let mut row_attrs = vec![("manifest:full-path".to_owned(), row.path.clone())];
        row_attrs.extend(row.attrs.iter().cloned());
        children.push(Node::Text("\n ".to_owned()));
        children.push(Node::El(El { name: "manifest:file-entry".to_owned(), attrs: row_attrs, children: Vec::new() }));
    }
    children.push(Node::Text("\n".to_owned()));
    let root = El { name: "manifest:manifest".to_owned(), attrs, children };
    let bytes = diacraft_container::xml::to_bytes(&root, b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n", b"\n", true)
        .map_err(|_| WriteError::Unsupported(WORDS_NAMES))?;
    work.charge_copy(bytes.len(), limits)?;
    if bytes.len() as u64 > limits.max_manifest_bytes {
        return Err(WriteError::Limit { what: "bytes in the manifest", max: limits.max_manifest_bytes });
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn attrs(pairs: &[(&str, &str)]) -> RawAttrs {
        RawAttrs::from_pairs(pairs.iter().copied())
    }

    fn part(name: &str, media: &str, other: &[(&str, &str)]) -> Part {
        Part {
            format: diacraft_doc::Format::Odf,
            name: Arc::from(name),
            media_type: Arc::from(media),
            role: diacraft_doc::PartRole::Other,
            manifest: attrs(other),
            rels: Arc::new(Vec::new()),
            bytes: Arc::from(&b""[..]),
        }
    }

    /// The media type's attribute is told by its local name, whatever prefix the root binds.
    #[test]
    fn the_media_type_attribute_is_told_by_its_local_name() {
        for name in ["manifest:media-type", "m:media-type", "media-type"] {
            assert!(is_media_type(name), "{name}");
        }
        for name in ["manifest:media-types", "manifest:full-path", "media-type:x", ""] {
            assert!(!is_media_type(name), "{name}");
        }
        assert_eq!((local("m:version"), local("version"), local("a:b:c")), ("version", "version", "c"));
    }

    /// A row's attributes: the held ones in their order with the media type kept, forced or added, and the defaults and the media type when none are held.
    #[test]
    fn a_rows_attributes_are_the_held_ones_with_the_media_type_where_it_belongs() {
        let held = attrs(&[("manifest:size", "9"), ("manifest:media-type", "a/b"), ("manifest:x", "1")]);
        let pairs = |v: Vec<(String, String)>| v.into_iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>();
        assert_eq!(pairs(row_attrs(Some(&held), "text/xml", false, &[])), ["manifest:size=9", "manifest:media-type=a/b", "manifest:x=1"]);
        assert_eq!(pairs(row_attrs(Some(&held), "text/xml", true, &[])), ["manifest:size=9", "manifest:media-type=text/xml", "manifest:x=1"]);
        let none = attrs(&[("manifest:x", "1")]);
        assert_eq!(pairs(row_attrs(Some(&none), "text/xml", false, &[])), ["manifest:x=1", "manifest:media-type=text/xml"]);
        assert_eq!(
            pairs(row_attrs(None, "text/xml", false, &[("manifest:version", "1.4")])),
            ["manifest:version=1.4", "manifest:media-type=text/xml"]
        );
        // Two media types held: the first is the one, the second is kept as it is.
        let twice = attrs(&[("manifest:media-type", "a"), ("m:media-type", "b")]);
        assert_eq!(pairs(row_attrs(Some(&twice), "t", true, &[])), ["manifest:media-type=t", "m:media-type=b"]);
    }

    /// The row of a package entry: its media type or the one its name says, and the other attributes held; an entry under `META-INF/` that no row ever listed has none.
    #[test]
    fn a_part_has_a_row_unless_it_is_one_the_manifest_never_listed() {
        let row = part_row(&part("Pictures/a.png", "", &[])).unwrap();
        assert_eq!((row.path.as_str(), row.attrs), ("Pictures/a.png", vec![("manifest:media-type".to_owned(), "image/png".to_owned())]));
        let held = part_row(&part("Object 1/", "application/x", &[("manifest:version", "1.3"), ("manifest:media-type", "dropped")])).unwrap();
        assert_eq!(held.attrs, [("manifest:media-type".to_owned(), "application/x".to_owned()), ("manifest:version".to_owned(), "1.3".to_owned())]);
        assert!(part_row(&part("META-INF/documentsignatures.xml", "", &[])).is_none());
        assert!(part_row(&part("META-INF/documentsignatures.xml", "text/xml", &[])).is_some(), "one the manifest listed");
        assert!(part_row(&part("manifest.rdf", "", &[])).is_some());
    }

    /// The root row says the kind and the version the package says, over whatever the model holds.
    #[test]
    fn the_root_row_says_the_kind_and_the_version_of_the_package() {
        let mut doc = Document::empty();
        let odf = &mut doc.ext.make_mut().odf;
        odf.manifest_rows.push((Arc::from("/"), attrs(&[("manifest:version", "1.2"), ("manifest:media-type", "wrong")])));
        let held = rows(&doc, "application/x-kind", std::iter::empty());
        let root = &held.first().unwrap().attrs;
        assert_eq!(*root, [("manifest:version".to_owned(), "1.4".to_owned()), ("manifest:media-type".to_owned(), "application/x-kind".to_owned())]);
        assert_eq!(held.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(), ["/", "content.xml", "styles.xml", "meta.xml", "settings.xml"]);
        // A model that holds no root row gets the default, in the same order.
        let bare = rows(&Document::empty(), "k", std::iter::empty());
        assert_eq!(bare[0].attrs, [("manifest:version".to_owned(), "1.4".to_owned()), ("manifest:media-type".to_owned(), "k".to_owned())]);
    }
}
