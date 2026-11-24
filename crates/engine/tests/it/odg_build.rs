//! Drawings for the engine's `.odg` tests, written by hand from the OASIS ODF standard (Part 2 for the package and the manifest, Part 3 for the
//! parts and the drawing's elements) and put together by the container's archive writer: `mimetype` first and stored, the parts after it, and a
//! manifest that lists them. The names are those of (the master page `Default`, the layout `PM1`, the automatic styles `gr1` and
//! `P1`, the five layers). No file made by another program is read.

use diacraft_container::zip::{ArchiveWriter, Method};

pub const DRAWING: &str = "application/vnd.oasis.opendocument.graphics";
pub const TEMPLATE: &str = "application/vnd.oasis.opendocument.graphics-template";

/// The namespace declarations of a part's root.
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
    r#"xmlns:script="urn:oasis:names:tc:opendocument:xmlns:script:1.0" "#,
    r#"xmlns:ext="urn:example:extension:of:somebody:else""#
);

const LAYERS: &str = r#"<draw:layer draw:name="layout"/><draw:layer draw:name="background"/><draw:layer draw:name="backgroundobjects"/><draw:layer draw:name="controls"/><draw:layer draw:name="measurelines"/>"#;

/// A graphic style the shapes name: a blue fill and a black line.
pub const GR1: &str = r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#3465a4" draw:stroke="solid" svg:stroke-width="0.05cm" svg:stroke-color="#000000" draw:textarea-vertical-align="middle" draw:auto-grow-height="false"/></style:style>"##;
/// A second graphic style, red.
pub const GR2: &str = r##"<style:style style:name="gr2" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#cc0000" draw:stroke="none"/></style:style>"##;
/// An empty paragraph style.
pub const P1: &str = r#"<style:style style:name="P1" style:family="paragraph"/>"#;

/// A drawing being built: an A4 portrait page on the master page `Default`, with the five layers.
#[derive(Clone)]
pub struct Drawing {
    /// The common styles of `styles.xml` (`office:styles`), none when empty.
    pub common: String,
    pub auto: String,
    pub pages: String,
    pub files: Vec<(String, Vec<u8>)>,
    pub generator: String,
    /// The fields of `office:meta` after the generator (`meta:user-defined` and the like), none when empty.
    pub meta: String,
}

impl Drawing {
    /// A drawing with no pages, whose automatic styles are `gr1`, `gr2` and `P1`.
    pub fn new() -> Drawing {
        Drawing {
            common: String::new(),
            auto: format!("{GR1}{GR2}{P1}"),
            pages: String::new(),
            files: Vec::new(),
            generator: "TestWriter/1".to_owned(),
            meta: String::new(),
        }
    }

    /// The drawing with `fields` added to `office:meta`, after the generator.
    pub fn meta(mut self, fields: &str) -> Drawing {
        self.meta.push_str(fields);
        self
    }

    /// The drawing with another page holding `shapes`.
    pub fn page(mut self, shapes: &str) -> Drawing {
        let n = self.pages.matches("<draw:page ").count() + 1;
        self.pages.push_str(&format!("<draw:page draw:name=\"page{n}\" draw:master-page-name=\"Default\">{shapes}</draw:page>"));
        self
    }

    /// The drawing with `styles` added to the common styles of `styles.xml`.
    pub fn common(mut self, styles: &str) -> Drawing {
        self.common.push_str(styles);
        self
    }

    /// The drawing with another file in its package.
    pub fn file(mut self, name: &str, bytes: &[u8]) -> Drawing {
        self.files.push((name.to_owned(), bytes.to_vec()));
        self
    }

    pub fn content_xml(&self) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-content {NS} office:version=\"1.3\"><office:font-face-decls/><office:automatic-styles>{}</office:automatic-styles><office:body><office:drawing>{}</office:drawing></office:body></office:document-content>",
            self.auto, self.pages
        )
    }

    pub fn styles_xml(&self) -> String {
        let styles = if self.common.is_empty() { "<office:styles/>".to_owned() } else { format!("<office:styles>{}</office:styles>", self.common) };
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-styles {NS} office:version=\"1.3\"><office:font-face-decls/>{styles}<office:automatic-styles><style:page-layout style:name=\"PM1\"><style:page-layout-properties fo:margin-top=\"1cm\" fo:margin-bottom=\"1cm\" fo:margin-left=\"1cm\" fo:margin-right=\"1cm\" fo:page-width=\"21cm\" fo:page-height=\"29.7cm\" style:print-orientation=\"portrait\"/></style:page-layout></office:automatic-styles><office:master-styles><draw:layer-set>{LAYERS}</draw:layer-set><style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"/></office:master-styles></office:document-styles>"
        )
    }

    pub fn meta_xml(&self) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-meta {NS} office:version=\"1.3\"><office:meta><meta:generator>{}</meta:generator>{}</office:meta></office:document-meta>",
            self.generator, self.meta
        )
    }

    /// The package: `mimetype`, the four parts, the other files, and a manifest that lists them all.
    pub fn bytes(&self) -> Vec<u8> {
        self.bytes_of_type(DRAWING)
    }

    /// The same drawing as one flat document (ODF 1.4 Part 3 §3.1.4, Table 7, and the order of R30 of the package rules): the root `office:document` states the
    /// media type and the version, and holds `office:meta`, `office:font-face-decls`, `office:styles`, the **one** `office:automatic-styles` (the content's
    /// styles and the page layout `PM1` of `styles.xml`), `office:master-styles` and `office:body`. Written by hand from the standard, and never by the
    /// `odg` crate's own conversion, so that a test of the reader has an oracle of its own.
    pub fn flat_xml(&self, media: &str) -> String {
        let styles = if self.common.is_empty() { "<office:styles/>".to_owned() } else { format!("<office:styles>{}</office:styles>", self.common) };
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document {NS} office:mimetype=\"{media}\" office:version=\"1.3\"><office:meta><meta:generator>{}</meta:generator>{}</office:meta><office:font-face-decls/>{styles}<office:automatic-styles>{}<style:page-layout style:name=\"PM1\"><style:page-layout-properties fo:margin-top=\"1cm\" fo:margin-bottom=\"1cm\" fo:margin-left=\"1cm\" fo:margin-right=\"1cm\" fo:page-width=\"21cm\" fo:page-height=\"29.7cm\" style:print-orientation=\"portrait\"/></style:page-layout></office:automatic-styles><office:master-styles><draw:layer-set>{LAYERS}</draw:layer-set><style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"/></office:master-styles><office:body><office:drawing>{}</office:drawing></office:body></office:document>",
            self.generator, self.meta, self.auto, self.pages
        )
    }

    /// [`Drawing::flat_xml`] of a drawing, as bytes.
    pub fn flat_bytes(&self) -> Vec<u8> {
        self.flat_xml(DRAWING).into_bytes()
    }

    /// The same package under another media type (a template's).
    pub fn bytes_of_type(&self, media: &str) -> Vec<u8> {
        let mut files = vec![
            ("content.xml".to_owned(), self.content_xml().into_bytes()),
            ("styles.xml".to_owned(), self.styles_xml().into_bytes()),
            ("meta.xml".to_owned(), self.meta_xml().into_bytes()),
        ];
        files.extend(self.files.iter().cloned());
        package(media, files)
    }
}

/// The manifest of `media` listing `names` (each with its media type).
fn manifest(media: &str, names: &[(String, &str)]) -> String {
    let mut out = String::from(concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        r#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3">"#,
        "\n"
    ));
    out.push_str(&format!(" <manifest:file-entry manifest:full-path=\"/\" manifest:version=\"1.3\" manifest:media-type=\"{media}\"/>\n"));
    for (path, media_type) in names {
        out.push_str(&format!(" <manifest:file-entry manifest:full-path=\"{path}\" manifest:media-type=\"{media_type}\"/>\n"));
    }
    out.push_str("</manifest:manifest>\n");
    out
}

/// A package of `media` holding `files` and a manifest that lists them: `mimetype` first and stored, the pictures stored, the rest deflated.
pub fn package(media: &str, files: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let rows: Vec<(String, &str)> = files
        .iter()
        .map(|(name, _)| {
            (
                name.clone(),
                if name.ends_with(".png") {
                    "image/png"
                } else if name.ends_with('/') {
                    ""
                } else {
                    "text/xml"
                },
            )
        })
        .collect();
    let mut all = files;
    all.push(("META-INF/manifest.xml".to_owned(), manifest(media, &rows).into_bytes()));
    let mut w = ArchiveWriter::new();
    w.add("mimetype", media.as_bytes(), Method::Stored).unwrap();
    for (name, data) in &all {
        if name.ends_with('/') {
            w.add_directory(name).unwrap();
        } else if name.ends_with(".png") {
            w.add(name, data, Method::Stored).unwrap();
        } else {
            w.add(name, data, Method::Deflated).unwrap();
        }
    }
    w.finish().unwrap()
}

/// A custom shape in the form gives for a rectangle: on style `gr1`, `width` by `height` at (`x`, `y`) centimetres from the page's
/// top left, holding `text` (a plain paragraph, or an empty one).
pub fn rect(name: &str, frame: (f64, f64, f64, f64), text: &str) -> String {
    rect_on("gr1", name, frame, text)
}

/// [`rect`] on the automatic style `style`.
pub fn rect_on(style: &str, name: &str, (x, y, width, height): (f64, f64, f64, f64), text: &str) -> String {
    let para = if text.is_empty() { "<text:p/>".to_owned() } else { format!("<text:p>{text}</text:p>") };
    format!(
        "<draw:custom-shape draw:style-name=\"{style}\" draw:text-style-name=\"P1\" draw:layer=\"layout\" draw:name=\"{name}\" svg:width=\"{width}cm\" svg:height=\"{height}cm\" svg:x=\"{x}cm\" svg:y=\"{y}cm\">{para}<draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"non-primitive\" draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N\"/></draw:custom-shape>"
    )
}

/// A connector glued from the shape `from` (glue point 3: the right side) to `to` (glue point 1: the left side), as a standard connector, with the
/// path and the ends LibreOffice's names give it. `from` and `to` name shapes that carry `xml:id` and `draw:id` of those names.
pub fn glued(name: &str, from: &str, to: &str, (x1, y1, x2, y2): (f64, f64, f64, f64)) -> String {
    format!(
        "<draw:connector draw:style-name=\"gr1\" draw:text-style-name=\"P1\" draw:layer=\"layout\" draw:name=\"{name}\" draw:start-shape=\"{from}\" draw:start-glue-point=\"3\" draw:end-shape=\"{to}\" draw:end-glue-point=\"1\" svg:x1=\"{x1}cm\" svg:y1=\"{y1}cm\" svg:x2=\"{x2}cm\" svg:y2=\"{y2}cm\" svg:d=\"M {} {} L {} {}\" svg:viewBox=\"0 0 {} {}\"><text:p/></draw:connector>",
        x1 * 1000.0,
        y1 * 1000.0,
        x2 * 1000.0,
        y2 * 1000.0,
        (x2 * 1000.0).max(x1 * 1000.0) + 1.0,
        (y2 * 1000.0).max(y1 * 1000.0) + 1.0
    )
}

/// The same, with an attribute (`ext:note`) and a child element (`ext:data`) of a namespace nobody here knows: what a copy of the shape carries that a
/// new shape of this writer cannot.
pub fn rect_with_extras(name: &str, frame: (f64, f64, f64, f64)) -> String {
    let plain = rect(name, frame, "");
    plain.replacen("<draw:custom-shape ", "<draw:custom-shape ext:note=\"kept\" ", 1).replacen(
        "</draw:custom-shape>",
        "<ext:data ext:key=\"k\">v</ext:data></draw:custom-shape>",
        1,
    )
}

/// A scratch folder under `TMPDIR`, removed on drop.
pub struct Dir(std::path::PathBuf);

impl Dir {
    pub fn new() -> Dir {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("diacraft-engine-odg-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    pub fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }

    /// Writes `bytes` as `name` and gives its path.
    pub fn write(&self, name: &str, bytes: &[u8]) -> String {
        std::fs::write(self.0.join(name), bytes).unwrap();
        self.path(name)
    }

    pub fn read(&self, name: &str) -> Vec<u8> {
        std::fs::read(self.0.join(name)).unwrap()
    }

    pub fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.0).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The bytes of one part of a package, as text.
pub fn part_of(bytes: &[u8], name: &str) -> String {
    let package = diacraft_odg::Package::read(bytes, &diacraft_odg::Limits::default()).unwrap();
    String::from_utf8(package.part(name).unwrap_or_else(|| panic!("no part {name}")).to_vec()).unwrap()
}

/// The bytes of one part of a package, whatever they are, or `None`.
pub fn part_bytes(bytes: &[u8], name: &str) -> Option<Vec<u8>> {
    diacraft_odg::Package::read(bytes, &diacraft_odg::Limits::default()).unwrap().part(name).map(<[u8]>::to_vec)
}

/// `content.xml` of a package.
pub fn content_of(bytes: &[u8]) -> String {
    part_of(bytes, "content.xml")
}

/// A drawing read through the reader of the `odg` crate (never through the writer's own tokenizer).
pub fn read_back(bytes: &[u8]) -> (diacraft_doc::Document, diacraft_odg::ReadReport) {
    diacraft_odg::read(bytes, &diacraft_odg::Limits::default()).unwrap_or_else(|e| panic!("the file did not read: {} ({e})", e.class()))
}

/// A session that has opened `drawing` as `a.odg` in `dir`.
pub fn open_drawing(dir: &Dir, drawing: &Drawing) -> diacraft_engine::Session {
    open_bytes_in(dir, "a.odg", &drawing.bytes())
}

/// A session that has opened `bytes` as `name` in `dir`.
pub fn open_bytes_in(dir: &Dir, name: &str, bytes: &[u8]) -> diacraft_engine::Session {
    let path = dir.write(name, bytes);
    let mut s = diacraft_engine::Session::new();
    crate::run(&mut s, "file.open", serde_json::json!({ "path": path }));
    s
}

/// The `ID` of the shape called `name` on the page at `page`, in the document `doc`.
pub fn id_of(doc: &diacraft_doc::Document, page: usize, name: &str) -> u32 {
    doc.pages.get(page).unwrap().shapes.iter().find(|s| s.name_u.as_deref() == Some(name)).unwrap_or_else(|| panic!("no shape {name}")).id
}

/// The names of the top-level shapes of the page at `page`, in drawing order.
pub fn names_of(doc: &diacraft_doc::Document, page: usize) -> Vec<String> {
    doc.pages.get(page).unwrap().shapes.iter().map(|s| s.name_u.as_deref().unwrap_or("").to_owned()).collect()
}

/// The frame (`PinX`, `PinY`, `Width`, `Height`, `Angle`) of the shape called `name`, as the engine reads it.
pub fn frame_of(doc: &diacraft_doc::Document, page: usize, name: &str) -> [f64; 5] {
    let page_id = doc.pages.get(page).unwrap().id;
    let f = diacraft_engine::draw::shape_frame(doc, page_id, id_of(doc, page, name)).unwrap();
    [f.pin_x, f.pin_y, f.width, f.height, f.angle]
}

/// Asserts two frames are the same to a millionth of an inch (and of a radian, an angle being the same one a whole turn away: 180 degrees and -180 are).
#[track_caller]
pub fn same_frame(made: [f64; 5], read: [f64; 5], what: &str) {
    for (i, (a, b)) in made.iter().zip(read.iter()).enumerate() {
        let same = if i == 4 { (a - b).sin().abs() < 1e-6 && (a - b).cos() > 0.0 } else { (a - b).abs() < 1e-6 };
        assert!(same, "{what}: cell {i}: the model has {a} and the file reads {b} ({made:?} against {read:?})");
    }
}
