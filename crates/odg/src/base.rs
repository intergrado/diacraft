//! The package of a drawing made in code, the one the model writer starts from (ODF design decision 10).
//!
//! The `odg` crate writes by splicing an edit into a package that is already there, so a drawing that has no file yet needs one to start from: this is it. It is
//! the smallest package the specs describe for a drawing: `mimetype` first and stored, `content.xml` with one page, `styles.xml` with the page's layout, the five layers
//! and the master page the page names, `meta.xml` with the generator, `settings.xml` whose root holds nothing, and the manifest, whose rows are in the order R11 gives. Every
//! XML part says `office:version="1.4"`. It is written by hand from the OASIS ODF standard, and holds **no data of LibreOffice's**: the names are the five layers'
//! and the master page's `Default`, which interoperation needs, and nothing else of its styles. **No clock is read**: the same layout gives the same
//! bytes each time.
//!
//! [`compose_base`] makes the package of a layout. The model writer ([`crate::write_with`] with no original, `write/model/`) makes its own from the same parts
//! ([`parts`]): it puts the document's `meta.xml`, `settings.xml`, manifest rows and package entries in place of the default ones, and splices the pages and the shapes of the
//! model into the result as it splices an edit into a file.
//!
//! What the package holds of the page is what ODF can say of it: the size, the margins (none) and the way it is held. A theme, the list of stencils a template opens and
//! the snap options are DiaCraft's data, which `.odg` has no place for here (tier 2 and tier 3, `ext/`); they are not in the package, and the document read from it has none of
//! them.
//!
//! Harvested from `crates/engine/src/template/otg.rs` of the shelved branch `task/ODG.13b` of this repository (`compose`, which made the package of a template); see
//! `docs/provenance.md`.

use diacraft_container::zip::{ArchiveWriter, Method};

use crate::Variant;
use crate::package::{MANIFEST_PATH, MIMETYPE_PATH};
use crate::write::WriteError;

/// The name of the one page: the name a new drawing's first page has.
pub(crate) const PAGE_NAME: &str = "Page-1";

/// The master page the page names, and the layout the master page names.
pub(crate) const MASTER_PAGE: &str = "Default";
const LAYOUT: &str = "PM1";

/// The namespace declarations of a part's root: the ones the part uses and no more (ODF Part 3, §1.5).
pub(crate) const NS_OFFICE: &str = r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0""#;
const NS_STYLE: &str = r#"xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0""#;
const NS_DRAW: &str = r#"xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0""#;
const NS_FO: &str = r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0""#;
const NS_SVG: &str = r#"xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0""#;
const NS_TEXT: &str = r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0""#;
pub(crate) const NS_META: &str = r#"xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0""#;
pub(crate) const NS_MANIFEST: &str = r#"xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0""#;

/// The five layers every drawing's layer set names (names only).
pub(crate) const LAYERS: [&str; 5] = ["layout", "background", "backgroundobjects", "controls", "measurelines"];

/// The version every part says it is.
pub(crate) const VERSION: &str = "1.4";

/// The generator, as the splice writer writes it: the application and its version.
pub(crate) use crate::write::GENERATOR;

/// What the package is made for: its kind and the page it starts, the page's size in inches (the unit of the model's pages).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BaseLayout {
    /// A drawing (`.odg`) or a drawing template (`.otg`): the media type `mimetype` and the manifest's root row state.
    pub variant: Variant,
    /// The page's width in inches.
    pub width: f64,
    /// The page's height in inches.
    pub height: f64,
    /// Whether the paper is held with its long side across (`style:print-orientation`).
    pub landscape: bool,
    /// Whether the layout says which way the paper is held. The reader gives the page a `PrintPageOrientation` cell from it, which the blank page a new drawing starts
    /// from has never had (its orientation is the size's), so the blank page's layout says none and a template's, which has always had the cell, does.
    pub stated: bool,
}

/// A length as ODF writes it, in inches: the shortest decimal that reads back exactly. `None` for a size that is not a length (not finite or not above 0).
fn inches(length: f64) -> Option<String> {
    (length.is_finite() && length > 0.0).then(|| format!("{length}in"))
}

/// A part's text: the declaration, the root with its declarations and version, and `body`.
pub(crate) fn xml(namespaces: &[&str], root: &str, body: &str) -> String {
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<{root} {} office:version=\"{VERSION}\">{body}</{root}>\n", namespaces.join(" "))
}

fn content() -> String {
    xml(
        &[NS_OFFICE, NS_STYLE, NS_TEXT, NS_DRAW, NS_FO, NS_SVG],
        "office:document-content",
        &format!(
            "<office:font-face-decls></office:font-face-decls><office:automatic-styles></office:automatic-styles>\
             <office:body><office:drawing><draw:page draw:name=\"{PAGE_NAME}\" draw:master-page-name=\"{MASTER_PAGE}\"></draw:page></office:drawing></office:body>"
        ),
    )
}

fn styles(layout: &BaseLayout) -> Option<String> {
    let (width, height) = (inches(layout.width)?, inches(layout.height)?);
    let orientation = match (layout.stated, layout.landscape) {
        (false, _) => String::new(),
        (true, true) => " style:print-orientation=\"landscape\"".to_owned(),
        (true, false) => " style:print-orientation=\"portrait\"".to_owned(),
    };
    let layers: String = LAYERS.iter().map(|name| format!("<draw:layer draw:name=\"{name}\"/>")).collect();
    Some(xml(
        &[NS_OFFICE, NS_STYLE, NS_TEXT, NS_DRAW, NS_FO, NS_SVG],
        "office:document-styles",
        &format!(
            "<office:font-face-decls></office:font-face-decls><office:styles></office:styles>\
             <office:automatic-styles><style:page-layout style:name=\"{LAYOUT}\">\
             <style:page-layout-properties fo:margin-top=\"0in\" fo:margin-bottom=\"0in\" fo:margin-left=\"0in\" fo:margin-right=\"0in\" \
             fo:page-width=\"{width}\" fo:page-height=\"{height}\"{orientation}/></style:page-layout></office:automatic-styles>\
             <office:master-styles><draw:layer-set>{layers}</draw:layer-set><style:master-page style:name=\"{MASTER_PAGE}\" style:page-layout-name=\"{LAYOUT}\"/></office:master-styles>"
        ),
    ))
}

fn meta() -> String {
    xml(&[NS_OFFICE, NS_META], "office:document-meta", &format!("<office:meta><meta:generator>{GENERATOR}</meta:generator></office:meta>"))
}

/// The default settings: **a root that holds nothing**. The ODF 1.4 schema has no empty `office:settings` (it holds one set of items or more), so the `<office:settings/>`
/// this part held was one the schema refuses (the `settings.xml` of 34 visits of files that had none), and a root with no child is one it takes; counts such a
/// part as none in the content check. It is written as the model writer writes the tree a read of it gives back (an empty element closed in its tag), so that the second save of a drawing that
/// held no settings is the first's bytes.
fn settings() -> String {
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-settings {NS_OFFICE} office:version=\"{VERSION}\"/>\n")
}

/// The manifest: the root row, then a row for each part in the order of R11. `mimetype` and the manifest itself are not listed.
fn manifest(variant: Variant) -> String {
    let mut out = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<manifest:manifest {NS_MANIFEST} manifest:version=\"{VERSION}\">\n");
    out.push_str(&format!(
        " <manifest:file-entry manifest:full-path=\"/\" manifest:version=\"{VERSION}\" manifest:media-type=\"{}\"/>\n",
        variant.media_type()
    ));
    for part in ["content.xml", "styles.xml", "meta.xml", "settings.xml"] {
        out.push_str(&format!(" <manifest:file-entry manifest:full-path=\"{part}\" manifest:media-type=\"text/xml\"/>\n"));
    }
    out.push_str("</manifest:manifest>\n");
    out
}

/// The five parts of the package of `layout`, as text.
pub(crate) struct Parts {
    pub content: String,
    pub styles: String,
    pub meta: String,
    pub settings: String,
    pub manifest: String,
}

/// The parts of the package of `layout`: `Err` says in words why there are none (a page whose size is no length).
pub(crate) fn parts(layout: &BaseLayout) -> Result<Parts, WriteError> {
    let styles = styles(layout).ok_or(WriteError::Unsupported(NO_SIZE))?;
    Ok(Parts { content: content(), styles, meta: meta(), settings: settings(), manifest: manifest(layout.variant) })
}

/// The words of a refusal of a page whose size is not a length.
pub(crate) const NO_SIZE: &str =
    "a first page whose width and height are not both lengths above 0 (the package a drawing is written onto is made for that page's size)";

/// The package of a drawing whose one page is as `layout` says: the bytes of an `.odg` or an `.otg`. `Err` says in words why there is none (a size that is no length, or an
/// archive the container's writer refused).
pub fn compose_base(layout: &BaseLayout) -> Result<Vec<u8>, WriteError> {
    let Parts { content, styles, meta, settings, manifest } = parts(layout)?;
    let mut archive = ArchiveWriter::new();
    // The media type first, stored, with no extra field.
    archive.add(MIMETYPE_PATH, layout.variant.media_type().as_bytes(), Method::Stored).map_err(WriteError::Package)?;
    for (name, text) in
        [("content.xml", &content), ("styles.xml", &styles), ("meta.xml", &meta), ("settings.xml", &settings), (MANIFEST_PATH, &manifest)]
    {
        archive.add(name, text.as_bytes(), Method::Deflated).map_err(WriteError::Package)?;
    }
    archive.finish().map_err(WriteError::Package)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::package::Package;
    use crate::{Limits, copy_unchanged, read, sniff};

    fn letter() -> BaseLayout {
        BaseLayout { variant: Variant::Template, width: 11.0, height: 8.5, landscape: true, stated: true }
    }

    fn package() -> Vec<u8> {
        compose_base(&letter()).unwrap()
    }

    /// `mimetype` is the first entry, stored, with no extra field, and says the media type at the fixed offsets, so that a program that tells a package by its
    /// first bytes tells a template.
    #[test]
    fn the_media_type_is_the_first_entry_stored_with_no_extra_field_at_the_fixed_offsets() {
        let bytes = package();
        let media = Variant::Template.media_type();
        assert_eq!(&bytes[..2], b"PK");
        assert_eq!(&bytes[26..30], &[8, 0, 0, 0], "a name of 8 bytes and no extra field");
        assert_eq!(&bytes[8..10], &[0, 0], "stored (method 0)");
        assert_eq!(&bytes[30..38], b"mimetype");
        assert_eq!(&bytes[38..38 + media.len()], media.as_bytes());
    }

    /// The package says what kind it is in both places, lists its parts in the order the rule gives (the root row first, `mimetype` and the manifest
    /// never listed), holds no `Configurations2/` and says `office:version="1.4"` in every XML part. A drawing says it in the same two places.
    #[test]
    fn the_package_is_what_the_package_rules_ask_of_a_new_drawing() {
        for variant in [Variant::Template, Variant::Drawing] {
            let bytes = compose_base(&BaseLayout { variant, ..letter() }).unwrap();
            assert_eq!(sniff(&bytes), Some(variant));
            let package = Package::read(&bytes, &Limits::default()).unwrap();
            assert_eq!(package.media_type(), Some(variant.media_type()));
            assert!(package.mimetype_regular() && package.warnings().is_empty(), "{:?}", package.warnings());
            let rows: Vec<(&str, Option<&str>)> =
                package.manifest().entries.iter().map(|e| (e.full_path.as_str(), e.media_type.as_deref())).collect();
            assert_eq!(
                rows,
                [
                    ("/", Some(variant.media_type())),
                    ("content.xml", Some("text/xml")),
                    ("styles.xml", Some("text/xml")),
                    ("meta.xml", Some("text/xml")),
                    ("settings.xml", Some("text/xml")),
                ]
            );
            let names: Vec<&str> = package.parts().collect();
            assert_eq!(names, ["mimetype", "content.xml", "styles.xml", "meta.xml", "settings.xml", "META-INF/manifest.xml"]);
            assert_eq!(package.folders().count(), 0, "no Configurations2/");
            for part in ["content.xml", "styles.xml", "meta.xml", "settings.xml"] {
                let text = std::str::from_utf8(package.part(part).unwrap()).unwrap();
                assert!(text.contains("office:version=\"1.4\""), "{part}");
            }
        }
    }

    /// The package reads as one page of the size it was composed for, the master page and the layers the page needs, and nothing odd, and the reader's own copy of it,
    /// unedited, is the file byte for byte.
    #[test]
    fn the_package_reads_as_a_drawing_with_the_page_and_copies_unchanged() {
        let bytes = package();
        let (doc, report) = read(&bytes, &Limits::default()).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(report.variant, Some(Variant::Template));
        assert_eq!((doc.pages.len(), report.stats.pages), (1, 1));
        let page = doc.pages.get(0).unwrap();
        assert_eq!(&*page.name, PAGE_NAME);
        let number = |name: &str| page.sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap();
        assert_eq!((number("PageWidth"), number("PageHeight")), (11.0, 8.5));
        assert_eq!(number("PrintPageOrientation"), 2.0, "landscape, stated");
        assert_eq!(report.generator.as_deref(), Some(GENERATOR));
        assert_eq!(report.office_version.as_deref(), Some("1.4"));
        assert_eq!(copy_unchanged(&bytes, &Limits::default()).unwrap(), bytes, "an unedited save of it is the file");
    }

    /// The layout says the way the paper is held only when it is asked to: a blank page's does not (the model's blank page has no such cell), a template's does, and both ways
    /// round.
    #[test]
    fn the_layout_states_the_orientation_only_when_asked() {
        let cell = |layout: BaseLayout| {
            let (doc, _) = read(&compose_base(&layout).unwrap(), &Limits::default()).unwrap();
            doc.pages.get(0).unwrap().sheet.cell("PrintPageOrientation").and_then(|c| c.value.as_ref()?.number())
        };
        assert_eq!(cell(BaseLayout { stated: false, ..letter() }), None);
        assert_eq!(cell(letter()), Some(2.0));
        assert_eq!(cell(BaseLayout { width: 8.5, height: 11.0, landscape: false, ..letter() }), Some(1.0));
    }

    /// A page with no size is no page: a size that is not above 0 or not finite is refused in words, and nothing is made.
    #[test]
    fn a_size_that_is_no_length_is_refused() {
        for (width, height) in [(0.0, 8.5), (11.0, 0.0), (-1.0, 8.5), (f64::NAN, 8.5), (11.0, f64::INFINITY), (f64::NEG_INFINITY, 1.0)] {
            let refused = compose_base(&BaseLayout { width, height, ..letter() }).unwrap_err();
            assert_eq!(refused, WriteError::Unsupported(NO_SIZE), "{width} x {height}");
        }
        // The smallest a person could ask for is a length: the reader's own limits judge it.
        assert!(compose_base(&BaseLayout { width: 0.001, height: 0.001, stated: false, ..letter() }).is_ok());
    }

    /// No clock is read and nothing varies: a package is the same bytes each time it is made, and a size is written as the shortest decimal that reads back.
    #[test]
    fn a_package_is_the_same_bytes_each_time_and_a_size_reads_back_exactly() {
        assert_eq!(package(), package());
        for width in [11.0, 8.5, 8.268, 0.1 + 0.2, 33.1, 1.0 / 3.0] {
            let bytes = compose_base(&BaseLayout { width, height: 5.0, stated: false, ..letter() }).unwrap();
            let (doc, _) = read(&bytes, &Limits::default()).unwrap();
            let read = doc.pages.get(0).unwrap().sheet.cell("PageWidth").and_then(|c| c.value.as_ref()?.number()).unwrap();
            assert_eq!(read, width, "{width}");
        }
    }
}
