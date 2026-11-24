//! Drawings for the reader's tests, written by hand from the OASIS ODF 1.4 standard (Part 3) and the specs' vectors, and put into a
//! package by `build.rs`'s honest writer. Every vector's markup is built here from the spec's words; no file made by another program is
//! read. A drawing starts as an A4 portrait page (`fo:page-width="21cm"`, `fo:page-height="29.7cm"`, the page the vectors
//! use) on the master page `Default`, with LibreOffice's five layer names (names only), and each test adds what its
//! vector gives.

use diacraft_doc::{Cell, CellAddr, Document, Lookup, Page, RowKey, Shape, Value};
use diacraft_odg::{Limits, ReadReport, read};

use crate::build::{DRAWING, package};

/// The namespace declarations of a part's root: ODF's (Part 3 §1.5) and DiaCraft's.
pub const NS: &str = concat!(
    r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" "#,
    r#"xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
    r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" "#,
    r#"xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
    r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" "#,
    r#"xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" "#,
    r#"xmlns:xlink="http://www.w3.org/1999/xlink" "#,
    r#"xmlns:dc="http://purl.org/dc/elements/1.1/" "#,
    r#"xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" "#,
    r#"xmlns:config="urn:oasis:names:tc:opendocument:xmlns:config:1.0" "#,
    r#"xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" "#,
    r#"xmlns:dr3d="urn:oasis:names:tc:opendocument:xmlns:dr3d:1.0" "#,
    r#"xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0" "#,
    r#"xmlns:script="urn:oasis:names:tc:opendocument:xmlns:script:1.0" "#,
    r#"xmlns:diacraft="urn:app.diacraft:extension""#
);

/// LibreOffice's five layers, by name, then any the test adds.
const LAYERS: &str = r#"<draw:layer draw:name="layout"/><draw:layer draw:name="background"/><draw:layer draw:name="backgroundobjects"/><draw:layer draw:name="controls"/><draw:layer draw:name="measurelines"/>"#;

/// A drawing being built.
#[derive(Clone)]
pub struct Drawing {
    pub version: String,
    /// The root's namespace declarations.
    pub ns: String,
    pub fonts: String,
    /// `office:styles` of `styles.xml`.
    pub common: String,
    /// `office:automatic-styles` of `styles.xml`, the page layouts among them.
    pub styles_auto: String,
    /// `office:master-styles`: the layer set and the master pages.
    pub masters: String,
    /// `office:automatic-styles` of `content.xml`.
    pub auto: String,
    /// `office:drawing`'s pages.
    pub pages: String,
    pub settings: Option<String>,
    pub meta: Option<String>,
    /// More files of the package.
    pub files: Vec<(String, Vec<u8>)>,
    /// `styles.xml` left out.
    pub no_styles: bool,
}

impl Drawing {
    /// An A4 portrait drawing on master page `Default`, with LibreOffice's five layer names and no pages yet.
    pub fn a4() -> Drawing {
        Drawing {
            version: "1.3".to_owned(),
            ns: NS.to_owned(),
            fonts: String::new(),
            common: String::new(),
            styles_auto: r#"<style:page-layout style:name="PM1"><style:page-layout-properties fo:margin-top="1cm" fo:margin-bottom="1cm" fo:margin-left="1cm" fo:margin-right="1cm" fo:page-width="21cm" fo:page-height="29.7cm" style:print-orientation="portrait"/></style:page-layout>"#.to_owned(),
            masters: format!(r#"<draw:layer-set>{LAYERS}</draw:layer-set><style:master-page style:name="Default" style:page-layout-name="PM1"/>"#),
            auto: String::new(),
            pages: String::new(),
            settings: None,
            meta: None,
            files: Vec::new(),
            no_styles: false,
        }
    }

    /// The drawing with one page holding `shapes`.
    pub fn page(mut self, shapes: &str) -> Drawing {
        let n = self.pages.matches("<draw:page ").count() + 1;
        self.pages.push_str(&format!(r#"<draw:page draw:name="page{n}" draw:master-page-name="Default">{shapes}</draw:page>"#));
        self
    }

    /// The drawing with `styles` added to `office:styles`.
    pub fn common(mut self, styles: &str) -> Drawing {
        self.common.push_str(styles);
        self
    }

    /// The drawing with `styles` added to `content.xml`'s automatic styles.
    pub fn auto(mut self, styles: &str) -> Drawing {
        self.auto.push_str(styles);
        self
    }

    pub fn content_xml(&self) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-content {} office:version=\"{}\"><office:font-face-decls>{}</office:font-face-decls><office:automatic-styles>{}</office:automatic-styles><office:body><office:drawing>{}</office:drawing></office:body></office:document-content>",
            self.ns, self.version, self.fonts, self.auto, self.pages
        )
    }

    pub fn styles_xml(&self) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-styles {} office:version=\"{}\"><office:font-face-decls>{}</office:font-face-decls><office:styles>{}</office:styles><office:automatic-styles>{}</office:automatic-styles><office:master-styles>{}</office:master-styles></office:document-styles>",
            self.ns, self.version, self.fonts, self.common, self.styles_auto, self.masters
        )
    }

    /// The package's files.
    pub fn files(&self) -> Vec<(String, Vec<u8>)> {
        let mut files = vec![("content.xml".to_owned(), self.content_xml().into_bytes())];
        if !self.no_styles {
            files.push(("styles.xml".to_owned(), self.styles_xml().into_bytes()));
        }
        if let Some(meta) = &self.meta {
            files.push(("meta.xml".to_owned(), meta.clone().into_bytes()));
        }
        if let Some(settings) = &self.settings {
            files.push(("settings.xml".to_owned(), settings.clone().into_bytes()));
        }
        files.extend(self.files.iter().cloned());
        files
    }

    pub fn bytes(&self) -> Vec<u8> {
        package(DRAWING, self.files())
    }

    /// Reads the drawing with the default limits; panics on an error.
    pub fn read(&self) -> (Document, ReadReport) {
        match read(&self.bytes(), &Limits::default()) {
            Ok(done) => done,
            Err(e) => panic!("the drawing did not read: {} ({e})", e.class()),
        }
    }
}

/// The model without the bookkeeping of the package it was read from: the media type (`mimetype`), the roots' attributes, the manifest's rows, `meta.xml`'s fields that no property
/// reads, `settings.xml` and the package entries. A save of another kind, an edit that writes the generator and the thumbnail, and the other form of the drawing (a flat document has no package)
/// change these without changing the drawing, so a test that asks whether two models are one drawing asks it of these.
pub fn without_package(doc: &Document) -> Document {
    let mut doc = doc.clone();
    doc.parts = diacraft_doc::SharedVec::new();
    if !doc.ext.is_empty() {
        let odf = &mut doc.ext.make_mut().odf;
        odf.mimetype = None;
        odf.roots.clear();
        odf.manifest_rows.clear();
        odf.meta = diacraft_doc::Passthrough::new();
        odf.settings = None;
    }
    doc
}

/// A `meta.xml` naming `generator`.
pub fn meta(generator: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-meta {NS} office:version=\"1.3\"><office:meta><meta:generator>{generator}</meta:generator></office:meta></office:document-meta>"
    )
}

/// A `meta.xml` naming `generator` and holding `fields` beside it: the generator counts as nothing in the content check, so a test that wants a `meta.xml` that says
/// something gives it a field.
pub fn meta_with(generator: &str, fields: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-meta {NS} office:version=\"1.3\"><office:meta><meta:generator>{generator}</meta:generator>{fields}</office:meta></office:document-meta>"
    )
}

/// A `settings.xml` with `items` in `ooo:configuration-settings`.
pub fn settings(items: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-settings {NS} office:version=\"1.3\"><office:settings><config:config-item-set config:name=\"ooo:configuration-settings\">{items}</config:config-item-set></office:settings></office:document-settings>"
    )
}

/// The page at `at`.
pub fn page(doc: &Document, at: usize) -> &Page {
    doc.pages.get(at).map(|p| p.as_ref()).expect("no such page")
}

/// The top-level shape at `at` on page `page`, in drawing order.
pub fn shape(doc: &Document, page_at: usize, at: usize) -> &Shape {
    page(doc, page_at).shapes.get(at).map(|s| s.as_ref()).expect("no such shape")
}

/// What the reader kept of a shape's own markup that no field reads (the attributes and the children it left in the shape's `odf` slot): the `.odg`'s, never the VSDX `raw` and
/// `passthrough` of the same shape.
pub fn odf(shape: &Shape) -> diacraft_doc::Kept {
    shape.ext.get().map(|d| d.odf.kept.clone()).unwrap_or_default()
}

/// [`odf`] for a page.
pub fn odf_page(page: &Page) -> diacraft_doc::Kept {
    page.ext.get().map(|d| d.odf.kept.clone()).unwrap_or_default()
}

/// The first top-level shape of the first page.
pub fn only(doc: &Document) -> &Shape {
    shape(doc, 0, 0)
}

/// A cell of a sheet, by address.
pub fn cell_at<'a>(sheet: &'a diacraft_doc::Sheet, addr: &CellAddr) -> Option<&'a Cell> {
    match sheet.lookup(addr) {
        Lookup::Found(cell) => Some(cell),
        _ => None,
    }
}

/// The cell `name` directly under the shape's sheet.
pub fn cell<'a>(shape: &'a Shape, name: &str) -> Option<&'a Cell> {
    shape.sheet.cell(name)
}

/// A cell's value as a number.
pub fn num(cell: Option<&Cell>) -> f64 {
    cell.and_then(|c| c.value.as_ref()).and_then(Value::number).unwrap_or_else(|| panic!("no number in {cell:?}"))
}

/// A cell's value as text.
pub fn text(cell: Option<&Cell>) -> &str {
    cell.and_then(|c| c.value.as_ref()).map(Value::text).unwrap_or_else(|| panic!("no value in {cell:?}"))
}

/// A cell's formula, if any.
pub fn formula(cell: Option<&Cell>) -> Option<&str> {
    cell.and_then(|c| c.formula.text())
}

/// A cell's unit, if any.
pub fn unit(cell: Option<&Cell>) -> Option<&str> {
    cell.and_then(|c| c.unit.as_deref())
}

/// The cell `name` of row `ix` of the section `section` (`Geometry` takes `geo`, the section's `IX`).
pub fn row_cell<'a>(shape: &'a Shape, section: &str, geo: Option<u32>, ix: u32, name: &str) -> Option<&'a Cell> {
    cell_at(&shape.sheet, &CellAddr::in_row(section, geo, RowKey::Ix(ix), name))
}

/// The type of row `ix` of `Geometry` section `geo`.
pub fn row_type(shape: &Shape, geo: u32, ix: u32) -> Option<String> {
    let section = shape.sheet.section(&diacraft_doc::SectionKey::new("Geometry", Some(geo)))?;
    section.row(&RowKey::Ix(ix)).and_then(|r| r.row_type.as_deref().map(str::to_owned))
}

/// How many rows `Geometry` section `geo` holds.
pub fn rows(shape: &Shape, geo: u32) -> usize {
    shape.sheet.section(&diacraft_doc::SectionKey::new("Geometry", Some(geo))).map_or(0, |s| s.rows.len())
}

/// A length in centimetres, as inches.
pub fn cm(n: f64) -> f64 {
    n / 2.54
}

/// What text of `n` bytes is charged, in bytes: nine eighths of it, rounded up (the reader's `cost::text`).
pub fn as_text(n: u64) -> u64 {
    n + n.div_ceil(8)
}

/// Asserts two numbers agree within 1E-9.
#[track_caller]
pub fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "{actual} is not {expected}");
}
