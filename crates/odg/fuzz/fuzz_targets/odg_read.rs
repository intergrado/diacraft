//! `cargo fuzz run odg_read`: the drawing reader on any input (a package, or a flat `.fodg`). A first byte of 0 makes the rest the `content.xml` of an otherwise honest
//! package (with a `styles.xml` of a page layout, a master page and styles), so the fuzzer explores the reader and not only the archive; a first
//! byte of 1 does the same for a drawing template (`.otg`: the media type of `mimetype` and of the manifest's root row is the template's,
//! and the read says it is a template); a first byte of 2 or 3 does the same for a flat drawing, a drawing or a template, with the rest as the content of
//! `office:drawing` of one `office:document` that holds the same styles; a first byte of 4, 5, 6 or 7 puts the rest into the fields of `meta.xml`, the items of
//! `settings.xml`, the rows of the manifest or the font faces of an otherwise honest package (the model keeps these, and the package's entries beside them); a first byte
//! of 8 makes the rest the children of up to 64 `office:annotation` elements of one page, with numbers, threads and shapes that repeat and loop; a first byte of 9 makes
//! the rest the attributes and the children of a `draw:gradient` that a common style and a shape fill with (its stand-in, LibreOffice's `loext:gradient-stop` children, and the
//! rows a shape of fewer deletes); a first byte of 10 makes the rest the master pages of an honest package, after the one its page is on, and the font faces of both parts (
//! a master page that holds no shape is kept whole, and what the drawing does not hold is counted for the open's note); a first byte of 11 makes the rest the shapes of a page after a few of
//! the reader's own; a first byte of 12 makes the rest the language attributes of a text style, of a paragraph style and of the default style, and `dc:language`
//! (the three triplets, `zxx` and `none`, a script, a full tag), over a text in the three script types, so that every run's language is read from one of them and a
//! `LangID` that is read is always a language tag or `x-none`; any other first byte reads the bytes as they are. A read gives a
//! document or an error, and a document holds the model's caps and the
//! index's promises, **and a read on threads is the read on one**; anything else is a crash the fuzzer reports.
#![no_main]

use diacraft_container::zip::{Archive, ArchiveWriter, Method};
use diacraft_odg::{Limits, Variant, read_partial};
use libfuzzer_sys::fuzz_target;

/// The package with the namespace of DiaCraft's own markup spelled as builds made before the id moved to `diacraft.app` spelled it (APPID), in every part that holds it. The readers read
/// both spellings as one, so that every law of this target holds of the one as of the other. `None` for bytes that are no package.
fn spelled_as_before(bytes: &[u8]) -> Option<Vec<u8>> {
    const CURRENT: &str = "urn:app.diacraft:extension";
    const BEFORE: &str = "urn:io.github.one_bit.diacraft:extension";
    let mut archive = Archive::read(bytes, &diacraft_container::Limits::default()).ok()?;
    let names: Vec<String> = archive.entries().iter().map(|e| e.name.clone()).collect();
    let mut out = ArchiveWriter::new();
    for (at, name) in names.iter().enumerate() {
        let part = archive.inflate(at, &mut diacraft_container::Work::default()).ok()?;
        let part = match String::from_utf8(part) {
            Ok(text) => text.replace(CURRENT, BEFORE).into_bytes(),
            Err(bytes) => bytes.into_bytes(),
        };
        out.add(name, &part, if name == "mimetype" || name.ends_with('/') { Method::Stored } else { Method::Deflated }).ok()?;
    }
    out.finish().ok()
}

/// Whether this input is met in the spelling of before: one input in two, by the parity of its bytes.
fn as_before(data: &[u8]) -> bool {
    data.iter().fold(0u8, |a, b| a ^ b) & 1 == 1
}

const MEDIA: &str = "application/vnd.oasis.opendocument.graphics";
const TEMPLATE: &str = "application/vnd.oasis.opendocument.graphics-template";

const STYLES: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
    r#"<office:document-styles xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" "#,
    r#"xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
    r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" "#,
    r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:xlink="http://www.w3.org/1999/xlink" office:version="1.3"><office:styles>"#,
    r#"<style:default-style style:family="graphic"><style:graphic-properties svg:stroke-width="0.03cm" draw:fill-color="&#35;e0ecf8"/></style:default-style>"#,
    r#"<style:style style:name="S0" style:family="graphic"><style:graphic-properties draw:fill="solid" svg:stroke-color="&#35;112233"/><style:text-properties fo:font-size="18pt"/></style:style>"#,
    r#"<style:style style:name="P0" style:family="paragraph" style:parent-style-name="S0"><style:paragraph-properties fo:text-align="center"/></style:style>"#,
    r#"<draw:stroke-dash draw:name="D" draw:dots1="1" draw:dots1-length="300%" draw:distance="300%"/>"#,
    // The markers a line end can name: DiaCraft's own, with and without a band class, and another producer's.
    r#"<draw:marker draw:name="DiaCraft_20_end_20_4" svg:viewBox="0 0 2000 2000" svg:d="M 1000 0 L 2000 2000 L 0 2000 Z"/>"#,
    r#"<draw:marker draw:name="DiaCraft_20_end_20_16_20_t23" svg:viewBox="0 0 2000 2000" svg:d="M 1000 0 L 2000 2000 L 0 2000 Z"/>"#,
    r#"<draw:marker draw:name="DiaCraft_20_end_20_45_20_t99" svg:viewBox="0 0 2000 2000" svg:d="M 1000 0 L 2000 2000 L 0 2000 Z"/>"#,
    r#"<draw:marker draw:name="Arrow" svg:viewBox="0 0 20 30" svg:d="m10 0-10 30h20z"/>"#,
    // The definitions of every family and a common style that is held beside its cells.
    r#"<draw:hatch draw:name="H" draw:style="single" draw:color="&#35;000000" draw:distance="0.1cm" draw:rotation="450"/>"#,
    r#"<draw:opacity draw:name="O" draw:style="linear" draw:start="100%" draw:end="0%"/>"#,
    r#"<draw:fill-image draw:name="I" xlink:href="Pictures/none.png"/>"#,
    // Gradients of each layout, with LibreOffice's stops, and a style that fills with one.
    r#"<draw:gradient xmlns:loext="urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0" draw:name="GL" draw:style="axial" draw:angle="30" draw:border="10%"><loext:gradient-stop svg:offset="0" loext:color-type="rgb" loext:color-value="&#35;ff0000"/><loext:gradient-stop svg:offset="0.5" loext:color-type="rgb" loext:color-value="&#35;00ff00"/><loext:gradient-stop svg:offset="0.5" loext:color-type="rgb" loext:color-value="&#35;0000ff"/><loext:gradient-stop svg:offset="1.5" loext:color-type="rgb" loext:color-value="&#35;ffffff"/></draw:gradient>"#,
    r#"<draw:gradient draw:name="GR" draw:style="radial" draw:cx="0%" draw:cy="100%" draw:border="30%" draw:start-color="&#35;102030" draw:end-color="&#35;f0e0d0"/>"#,
    r#"<style:style style:name="GS" style:family="graphic"><style:graphic-properties draw:fill="gradient" draw:fill-gradient-name="GL"/></style:style>"#,
    r#"<style:style style:name="G1" style:family="graphic"><style:graphic-properties draw:fill="hatch" draw:fill-hatch-name="H" draw:opacity-name="O" draw:stroke-linejoin="round" draw:shadow="visible" draw:shadow-blur="1px" fo:padding="1px" style:writing-mode="tb-rl" draw:marker-start="Arrow" draw:marker-start-width="0.3cm" draw:stroke-dash="D"/><style:paragraph-properties style:tab-stop-distance="1cm"><style:tab-stops><style:tab-stop style:position="1cm"/></style:tab-stops></style:paragraph-properties></style:style>"#,
    // A common style whose text values and tab leaders the cells only approximate, and a graphic style with a dash and a marker no definition names.
    r#"<style:style style:name="Z0" style:family="paragraph"><style:paragraph-properties fo:text-align="start" style:line-spacing="0.4cm"><style:tab-stops><style:tab-stop style:position="1cm" style:leader-text="."/></style:tab-stops></style:paragraph-properties><style:text-properties style:text-underline-style="wave" fo:text-transform="lowercase"/></style:style>"#,
    r#"<style:style style:name="Z1" style:family="graphic"><style:graphic-properties draw:stroke="dash" draw:stroke-dash="Nowhere" draw:marker-start="Nothing" draw:marker-start-width="0.5cm" svg:stroke-width="x" fo:padding="0.1cm" style:writing-mode="tb-rl"/></style:style>"#,
    r#"<style:style style:name="T0" style:family="text"><style:text-properties style:text-underline-style="dotted" fo:text-transform="lowercase" style:text-position="30%"/></style:style>"#,
    // A style sheet that says the weight alone, one that underlines and says nothing else of the text, a common style with no name, and the second of a name.
    r#"<style:style style:name="B0" style:family="graphic"><style:text-properties fo:font-weight="bold"/></style:style>"#,
    r#"<style:style style:name="B1" style:family="graphic"><style:text-properties style:text-underline-style="solid"/></style:style>"#,
    r#"<style:style style:name="B0" style:family="graphic"><style:graphic-properties draw:fill="none"/></style:style>"#,
    r#"<style:style style:family="graphic"><style:graphic-properties draw:fill="solid"/></style:style>"#,
    r#"</office:styles><office:automatic-styles><style:page-layout style:name="PM1"><style:page-layout-properties fo:page-width="21cm" fo:page-height="29.7cm"/></style:page-layout>"#,
    // Automatic styles that nothing names (one in each of the three families, one that says nothing), one with no name, the second of a name, and one of attributes no field reads.
    r#"<style:style style:name="U0" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="&#35;00ff00"/></style:style>"#,
    r#"<style:style style:name="U0" style:family="text"><style:text-properties fo:color="&#35;ff0000"/></style:style>"#,
    r#"<style:style style:name="U1" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style>"#,
    r#"<style:style style:name="U2" style:family="graphic"/>"#,
    r#"<style:style style:name="U0" style:family="graphic"><style:graphic-properties draw:fill="none"/></style:style>"#,
    r#"<style:style style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style>"#,
    r#"<style:style style:name="U3" style:family="graphic"><style:graphic-properties draw:textarea-horizontal-align="justify"/><style:text-properties fo:language="en"/></style:style>"#,
    r#"</office:automatic-styles><office:master-styles><draw:layer-set><draw:layer draw:name="layout"/><draw:layer draw:name="L1"/></draw:layer-set>"#,
    r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/><style:master-page style:page-layout-name="PM1"/></office:master-styles></office:document-styles>"#
);

const MANIFEST: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
    r#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3">"#,
    r#"<manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.graphics"/>"#,
    r#"<manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>"#,
    r#"<manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/>"#,
    r#"</manifest:manifest>"#
);

/// An honest flat drawing around the pages `content`, of a drawing template when `template`: the children in order, the styles of the package's `styles.xml`.
fn flat(content: &[u8], template: bool) -> Vec<u8> {
    let media = if template { TEMPLATE } else { MEDIA };
    let root = STYLES.replacen("<office:document-styles", "<office:document", 1).replacen(
        r#" office:version="1.3">"#,
        &format!(r#" office:version="1.3" office:mimetype="{media}">"#),
        1,
    );
    let root = root.replacen("</office:document-styles>", "", 1);
    let mut out = root.into_bytes();
    out.extend_from_slice(b"<office:body><office:drawing>");
    out.extend_from_slice(content);
    out.extend_from_slice(b"</office:drawing></office:body></office:document>");
    out
}

/// An honest package around `content`, of a drawing template when `template`.
fn package(content: &[u8], template: bool) -> Option<Vec<u8>> {
    let media = if template { TEMPLATE } else { MEDIA };
    let manifest = MANIFEST.replacen(MEDIA, media, 1);
    let mut w = ArchiveWriter::new();
    w.add("mimetype", media.as_bytes(), Method::Stored).ok()?;
    w.add("content.xml", content, Method::Deflated).ok()?;
    w.add("styles.xml", STYLES.as_bytes(), Method::Deflated).ok()?;
    w.add("META-INF/manifest.xml", manifest.as_bytes(), Method::Deflated).ok()?;
    w.finish().ok()
}

/// The shapes a visit writes back as they were, as a page's first shapes: a rectangle that holds the id a connector names and a glue point of its own, a second target whose
/// id is a `draw:id` alone, a connector between them, a group with no name whose own style and glue point are the file's, a polygon whose view box the reader cannot read and a polyline
/// whose points it cannot, two shapes that claim one id, and two rectangles whose ids are a `draw:id` alone (`n1`, `n2`) with a connector that names them: with the
/// flag byte of an ODF 1.0 file the input is the vector of that task, which ODF 1.4 takes only as a pair.
const SEED_SHAPES: &str = concat!(
    r#"<draw:rect draw:style-name="gr1" xml:id="s1" draw:id="s1" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2cm"><draw:glue-point draw:id="5" svg:x="10%" svg:y="-20%" draw:escape-direction="horizontal"/></draw:rect>"#,
    r#"<draw:rect draw:style-name="gr1" draw:id="s2" svg:x="8cm" svg:y="1cm" svg:width="4cm" svg:height="2cm"/>"#,
    r#"<draw:connector draw:style-name="gr1" draw:start-shape="s1" draw:start-glue-point="5" draw:end-shape="s2" draw:end-glue-point="3" svg:x1="5cm" svg:y1="2cm" svg:x2="8cm" svg:y2="2cm"/>"#,
    r#"<draw:g draw:style-name="gr1"><draw:glue-point draw:id="4" svg:x="1cm" svg:y="1cm"/><draw:rect draw:style-name="gr1" svg:x="1cm" svg:y="5cm" svg:width="2cm" svg:height="2cm"/><draw:rect draw:style-name="gr1" svg:x="4cm" svg:y="5cm" svg:width="2cm" svg:height="2cm"/></draw:g>"#,
    r#"<draw:polygon draw:style-name="gr1" svg:x="1cm" svg:y="9cm" svg:width="4cm" svg:height="2cm" svg:viewBox="0 0 0 100" draw:points="0,0 100,0 50,100"/>"#,
    r#"<draw:polyline draw:style-name="gr1" svg:x="6cm" svg:y="9cm" svg:width="4cm" svg:height="2cm" svg:viewBox="0 0 100 100" draw:points="zz"/>"#,
    r#"<draw:rect draw:style-name="gr1" xml:id="s1" draw:id="s1" svg:x="11cm" svg:y="9cm" svg:width="2cm" svg:height="2cm"/>"#,
    r#"<draw:rect draw:style-name="gr1" draw:id="n1" svg:x="1cm" svg:y="13cm" svg:width="4cm" svg:height="2cm"/>"#,
    r#"<draw:rect draw:style-name="gr1" draw:id="n2" svg:x="8cm" svg:y="13cm" svg:width="4cm" svg:height="2cm"/>"#,
    r#"<draw:connector draw:style-name="gr1" draw:start-shape="n1" draw:start-glue-point="1" draw:end-shape="n2" draw:end-glue-point="3" svg:x1="5cm" svg:y1="14cm" svg:x2="8cm" svg:y2="14cm"/>"#
);

/// An honest package whose one page holds [`SEED_SHAPES`] and then the shapes `rest` gives, on the package's own styles. `None` for text that is not UTF-8 or that the archive's
/// writer refuses.
fn package_shapes(rest: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(rest).ok()?;
    let content = format!(
        concat!(
            r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
            r#"xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" office:version="1.3">"#,
            r#"<office:automatic-styles><style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="&#35;336699"/></style:style></office:automatic-styles>"#,
            r#"<office:body><office:drawing><draw:page draw:name="p">{}{}</draw:page></office:drawing></office:body></office:document-content>"#
        ),
        SEED_SHAPES, text
    );
    let mut w = ArchiveWriter::new();
    w.add("mimetype", MEDIA.as_bytes(), Method::Stored).ok()?;
    w.add("content.xml", content.as_bytes(), Method::Deflated).ok()?;
    w.add("styles.xml", STYLES.as_bytes(), Method::Deflated).ok()?;
    w.add("META-INF/manifest.xml", MANIFEST.as_bytes(), Method::Deflated).ok()?;
    w.finish().ok()
}

/// An honest package whose language attributes are `rest` (the selector byte 12): they are the text properties of a text style `L1`, of a paragraph style `P9` and of the default
/// paragraph style, and the first line of them is `dc:language`; the page holds a span of `L1` over a text in the three script types (Latin letters, Japanese ones, Arabic ones, digits), a paragraph
/// of `P9`, and the same text in a span of a span. `None` for text that is not UTF-8 or that the archive's writer refuses.
fn package_languages(rest: &[u8]) -> Option<Vec<u8>> {
    const NS: &str = concat!(
        r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" "#,
        r#"xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
        r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" "#,
        r#"xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0""#
    );
    const TEXT: &str = "Hello \u{65E5}\u{672C}\u{8A9E} \u{0639}\u{0631}\u{0628}\u{064A} 123 \u{65E5}";
    let text = std::str::from_utf8(rest).ok()?;
    let first_line = text.lines().next().unwrap_or_default();
    let content = format!(
        concat!(
            r#"<office:document-content {ns} office:version="1.3"><office:automatic-styles>"#,
            r#"<style:style style:name="L1" style:family="text"><style:text-properties {attrs}/></style:style>"#,
            r#"<style:style style:name="P9" style:family="paragraph"><style:text-properties {attrs}/></style:style>"#,
            r#"</office:automatic-styles><office:body><office:drawing><draw:page draw:name="p" draw:master-page-name="Default">"#,
            r#"<draw:rect svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2cm"><text:p><text:span text:style-name="L1">{text}</text:span></text:p>"#,
            r#"<text:p text:style-name="P9">{text}</text:p><text:p><text:span text:style-name="L1">a<text:span text:style-name="L1">{text}</text:span></text:span></text:p></draw:rect>"#,
            r#"</draw:page></office:drawing></office:body></office:document-content>"#
        ),
        ns = NS,
        attrs = text,
        text = TEXT
    );
    let styles = STYLES.replacen(
        "</office:styles>",
        &format!(r#"<style:default-style style:family="paragraph"><style:text-properties {text}/></style:default-style></office:styles>"#),
        1,
    );
    let meta = format!(
        r#"<office:document-meta {NS} office:version="1.3"><office:meta><dc:language>{first_line}</dc:language></office:meta></office:document-meta>"#
    );
    let mut w = ArchiveWriter::new();
    w.add("mimetype", MEDIA.as_bytes(), Method::Stored).ok()?;
    w.add("content.xml", content.as_bytes(), Method::Deflated).ok()?;
    w.add("styles.xml", styles.as_bytes(), Method::Deflated).ok()?;
    w.add("meta.xml", meta.as_bytes(), Method::Deflated).ok()?;
    w.add("META-INF/manifest.xml", MANIFEST.as_bytes(), Method::Deflated).ok()?;
    w.finish().ok()
}

/// An honest package whose `meta.xml`, `settings.xml`, manifest or font faces hold `rest`, by `kind` (4, 5, 6 or 7), and whose other parts are as plain as they can be; it holds
/// an entry beside them, and a folder. `None` for text that is not UTF-8 or that the archive's writer refuses.
fn package_data(kind: u8, rest: &[u8]) -> Option<Vec<u8>> {
    const NS: &str = concat!(
        r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" "#,
        r#"xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:config="urn:oasis:names:tc:opendocument:xmlns:config:1.0" "#,
        r#"xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" "#,
        r#"xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:x="urn:example:extension""#
    );
    let text = std::str::from_utf8(rest).ok()?;
    let content = format!(
        r#"<office:document-content {NS} office:version="1.3"><office:body><office:drawing><draw:page draw:name="p"/></office:drawing></office:body></office:document-content>"#
    );
    let styles = match kind {
        7 => STYLES.replacen(
            r#"office:version="1.3">"#,
            &format!(r#"office:version="1.3"><office:font-face-decls>{text}</office:font-face-decls>"#),
            1,
        ),
        _ => STYLES.to_owned(),
    };
    let meta = format!(
        r#"<office:document-meta {NS} office:version="1.3"><office:meta>{}</office:meta></office:document-meta>"#,
        if kind == 4 { text } else { "<meta:generator>f/1</meta:generator>" }
    );
    let settings = format!(
        r#"<office:document-settings {NS} office:version="1.3"><office:settings>{}</office:settings></office:document-settings>"#,
        if kind == 5 { text } else { "" }
    );
    let rows = if kind == 6 { text } else { "" };
    let manifest = MANIFEST.replacen("</manifest:manifest>", &format!("{rows}</manifest:manifest>"), 1);
    let mut w = ArchiveWriter::new();
    w.add("mimetype", MEDIA.as_bytes(), Method::Stored).ok()?;
    w.add("content.xml", content.as_bytes(), Method::Deflated).ok()?;
    w.add("styles.xml", styles.as_bytes(), Method::Deflated).ok()?;
    w.add("meta.xml", meta.as_bytes(), Method::Deflated).ok()?;
    w.add("settings.xml", settings.as_bytes(), Method::Deflated).ok()?;
    w.add("Fonts/a.ttf", b"font", Method::Deflated).ok()?;
    w.add("Configurations2/", b"", Method::Stored).ok()?;
    w.add("META-INF/manifest.xml", manifest.as_bytes(), Method::Deflated).ok()?;
    w.finish().ok()
}

/// An honest package whose one page holds up to 64 `office:annotation` elements, each with the markup `rest` gives it: the first byte is how many, and the numbers,
/// the threads and the shapes they name repeat and loop, so that a read meets a number twice, a reply to a reply, a reply that is its own answer and a reply to another shape.
/// `None` for text that is not UTF-8 or that the archive's writer refuses.
fn annotated(rest: &[u8]) -> Option<Vec<u8>> {
    const NS: &str = concat!(
        r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" "#,
        r#"xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" "#,
        r#"xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
        r#"xmlns:officeooo="http://openoffice.org/2009/office" xmlns:diacraft="urn:app.diacraft:extension""#
    );
    let (first, body) = rest.split_first()?;
    let body = std::str::from_utf8(body).ok()?;
    let copies = 1 + usize::from(*first) % 64;
    let mut page = String::new();
    for k in 0..copies {
        let (id, parent, shape) = ((k * 7 + usize::from(*first)) % 9, (k * 5 + 1) % 9, k % 3);
        let tag = if k % 4 == 3 { "officeooo:annotation" } else { "office:annotation" };
        page.push_str(&format!(
            r#"<{tag} diacraft:version="1" diacraft:id="{id}" diacraft:parent="{parent}" diacraft:shape="{shape}" svg:x="1cm">{body}</{tag}>"#
        ));
    }
    let content = format!(
        r#"<office:document-content {NS} office:version="1.3"><office:body><office:drawing><draw:page draw:name="p" draw:master-page-name="Default">{page}</draw:page></office:drawing></office:body></office:document-content>"#
    );
    let mut w = ArchiveWriter::new();
    w.add("mimetype", MEDIA.as_bytes(), Method::Stored).ok()?;
    w.add("content.xml", content.as_bytes(), Method::Deflated).ok()?;
    w.add("styles.xml", STYLES.as_bytes(), Method::Deflated).ok()?;
    w.add("META-INF/manifest.xml", MANIFEST.as_bytes(), Method::Deflated).ok()?;
    w.finish().ok()
}

/// An honest package whose master styles hold `rest` after the master page its page is on, and whose two parts' font faces are the same few declarations, one of them repeated and one
/// name declared otherwise in each part. `None` for text that is not UTF-8 or that the archive's writer refuses.
fn mastered(rest: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(rest).ok()?;
    let faces = |generic: &str| {
        format!(
            r#"<office:font-face-decls><style:font-face style:name="A" svg:font-family="A"/><style:font-face style:name="A" svg:font-family="A"/><style:font-face style:name="B" svg:font-family="B" style:font-family-generic="{generic}"/></office:font-face-decls>"#
        )
    };
    let styles = STYLES.replacen(r#"office:version="1.3">"#, &format!(r#"office:version="1.3">{}"#, faces("swiss")), 1).replacen(
        "</office:master-styles>",
        &format!("{text}</office:master-styles>"),
        1,
    );
    let content = format!(
        r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" office:version="1.3">{}<office:body><office:drawing><draw:page draw:name="p" draw:master-page-name="Default"><draw:measure svg:x1="1cm" svg:y1="1cm" svg:x2="2cm" svg:y2="1cm"/></draw:page></office:drawing></office:body></office:document-content>"#,
        faces("roman")
    );
    let mut w = ArchiveWriter::new();
    w.add("mimetype", MEDIA.as_bytes(), Method::Stored).ok()?;
    w.add("content.xml", content.as_bytes(), Method::Deflated).ok()?;
    w.add("styles.xml", styles.as_bytes(), Method::Deflated).ok()?;
    w.add("META-INF/manifest.xml", MANIFEST.as_bytes(), Method::Deflated).ok()?;
    w.finish().ok()
}

/// An honest package whose `styles.xml` holds a `draw:gradient` named `F` of the attributes and children `rest`, a common style that fills with it, and a page of two shapes: one on
/// that style, and one on an automatic style under it that names the gradient `GR` (of two rows, where `F` may have up to ten). `None` for text that is not UTF-8 or that the
/// archive's writer refuses.
fn package_gradient(rest: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(rest).ok()?;
    let defined = format!(
        r#"<draw:gradient xmlns:loext="urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0" draw:name="F" {text}</draw:gradient><style:style style:name="GF" style:family="graphic"><style:graphic-properties draw:fill="gradient" draw:fill-gradient-name="F"/></style:style></office:styles>"#
    );
    let styles = STYLES.replacen("</office:styles>", &defined, 1);
    let content = concat!(
        r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
        r#"xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" office:version="1.3">"#,
        r#"<office:automatic-styles><style:style style:name="a1" style:family="graphic" style:parent-style-name="GF"><style:graphic-properties draw:fill-gradient-name="GR"/></style:style></office:automatic-styles>"#,
        r#"<office:body><office:drawing><draw:page draw:name="p"><draw:rect draw:style-name="GF" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2cm"/>"#,
        r#"<draw:rect draw:style-name="a1" svg:x="1cm" svg:y="4cm" svg:width="4cm" svg:height="2cm"/></draw:page></office:drawing></office:body></office:document-content>"#
    );
    let mut w = ArchiveWriter::new();
    w.add("mimetype", MEDIA.as_bytes(), Method::Stored).ok()?;
    w.add("content.xml", content.as_bytes(), Method::Deflated).ok()?;
    w.add("styles.xml", styles.as_bytes(), Method::Deflated).ok()?;
    w.add("META-INF/manifest.xml", MANIFEST.as_bytes(), Method::Deflated).ok()?;
    w.finish().ok()
}

/// **A sheet's gradient stands in as the cells the renderer reads**, asked of the packages of kind 9, whose rows of `FillGradient` only the reader's stand-in makes (a drawing
/// of DiaCraft's may carry any rows in its data): each row of `FillGradient` holds `GradientStopColor` and `GradientStopPosition` and no other cell, a place from
/// 0 to 1 and a colour of `#RRGGBB`, at most ten rows are live, and `FillGradientDir` is a layout from 0 to 13.
fn gradient_rows_are_the_renderers(sheet: &diacraft_doc::Sheet) {
    if let Some(section) = sheet.section(&diacraft_doc::SectionKey::new("FillGradient", None)) {
        let live = section.rows.iter().filter(|r| !r.del).count();
        assert!(live <= 10, "{live} live rows");
        for row in section.rows.iter().filter(|r| !r.del) {
            assert!(row.cells.keys().all(|k| matches!(k.as_str(), "GradientStopColor" | "GradientStopPosition")));
            let at = row.cells.get("GradientStopPosition").and_then(|c| c.value.as_ref()).and_then(diacraft_doc::Value::number).expect("a place");
            assert!((0.0..=1.0).contains(&at), "a place of {at}");
            let colour = row.cells.get("GradientStopColor").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()).expect("a colour");
            assert!(colour.len() == 7 && colour.starts_with('#'), "{colour}");
        }
    }
    if let Some(dir) = sheet.cell("FillGradientDir").and_then(|c| c.value.as_ref()).and_then(diacraft_doc::Value::number) {
        assert!((0.0..=13.0).contains(&dir) && dir.fract() == 0.0, "a layout of {dir}");
    }
}

fuzz_target!(|data: &[u8]| {
    // The kind a wrapped package is, which a read that succeeds says.
    let (bytes, kind) = match data.split_first() {
        Some((8, rest)) => match annotated(rest) {
            Some(bytes) => (bytes, Some(Variant::Drawing)),
            None => return,
        },
        Some((12, rest)) => match package_languages(rest) {
            Some(bytes) => (bytes, Some(Variant::Drawing)),
            None => return,
        },
        Some((11, rest)) => match package_shapes(rest) {
            Some(bytes) => (bytes, Some(Variant::Drawing)),
            None => return,
        },
        Some((10, rest)) => match mastered(rest) {
            Some(bytes) => (bytes, Some(Variant::Drawing)),
            None => return,
        },
        Some((kind @ 4..=7, rest)) => match package_data(*kind, rest) {
            Some(bytes) => (bytes, Some(Variant::Drawing)),
            None => return,
        },
        Some((9, rest)) => match package_gradient(rest) {
            Some(bytes) => (bytes, Some(Variant::Drawing)),
            None => return,
        },
        Some((kind @ (0 | 1), rest)) => match package(rest, *kind == 1) {
            Some(bytes) => (bytes, Some(if *kind == 1 { Variant::Template } else { Variant::Drawing })),
            None => return,
        },
        Some((kind @ (2 | 3), rest)) => (flat(rest, *kind == 3), Some(if *kind == 3 { Variant::Template } else { Variant::Drawing })),
        _ => (data.to_vec(), None),
    };
    let bytes = if as_before(data) { spelled_as_before(&bytes).unwrap_or(bytes) } else { bytes };
    let limits = Limits::default();
    let (result, report) = read_partial(&bytes, &limits);
    // **A read on threads is the read on one**: the same document and report, or the same refusal, with the files of the package inflated on threads.
    let threads = 2 + data.len() % 4;
    let (on_threads, threaded_report) = read_partial(&bytes, &Limits { threads, ..limits.clone() });
    assert!(on_threads == result && threaded_report == report, "a read on {threads} threads is not the read on one");
    // What the drawing does not hold: said first, a head and a line for each class counted, each line within the window's, and counted as that many warnings;
    // nothing when nothing is counted. A read that is refused says it too, as far as it went.
    let classes = report.not_kept.values().filter(|n| **n > 0).count();
    if classes == 0 {
        assert!(!report.warnings.iter().any(|w| w == diacraft_odg::NOT_KEPT_HEAD) && report.warned("not_kept") == 0);
    } else if result.is_ok() {
        assert_eq!(report.warned("not_kept"), 1 + classes as u64);
        assert_eq!(report.warnings.first().map(String::as_str), Some(diacraft_odg::NOT_KEPT_HEAD));
        for line in report.warnings.iter().take(1 + classes) {
            assert!(line.chars().count() <= diacraft_odg::NOT_KEPT_LINE_CHARS, "{line}");
        }
    }
    if let Ok(doc) = result {
        // A master page kept whole is a `style:master-page` that holds no shape the page reader reads.
        if let Some(data) = doc.ext.get() {
            for kept in data.odf.styles.iter().filter(|s| &*s.family == "master:master-page") {
                assert!(kept.element.name.ends_with(":master-page"), "{}", kept.element.name);
            }
        }
        if kind.is_some() {
            assert_eq!(report.variant, kind, "a read says what kind the package is");
        }
        // The form is what the first bytes say, and a flat drawing holds no package.
        assert_eq!(report.form, diacraft_odg::Form::of(&bytes));
        if report.form == diacraft_odg::Form::Flat {
            assert_eq!(report.work.package.inflated, 0);
        }
        // `sniff` and `read` agree on what is a drawing, for a flat drawing too: a file that sniffs as a kind reads as that kind, or as a drawing with the
        // warning `mimetype_irregular` (a flat file that says nothing it knows).
        if let Some(sniffed) = diacraft_odg::sniff(&bytes) {
            assert!(report.variant == Some(sniffed) || report.warned("mimetype_irregular") > 0, "{sniffed:?} {:?}", report.variant);
        }
        assert!(doc.check_limits().is_ok());
        // **The languages**: a `LangID` the reader makes is a language tag in canonical case or `x-none` (a value of a drawing of DiaCraft's may be any text, so only
        // the packages that carry no DiaCraft data are held to it), and `DocLangID`, when the read makes it, is the tag `dc:language` says.
        if data.first() == Some(&12) {
            let tag_or_none = |text: &str| text == "x-none" || diacraft_doc::lang::parse(text).is_ok_and(|tag| tag.as_str() == text);
            let mut sheets: Vec<&diacraft_doc::Sheet> = doc.styles.iter().map(|s| &*s.sheet).collect();
            for page in doc.pages.iter() {
                let mut stack: Vec<&diacraft_doc::Shape> = page.shapes.iter().map(|s| s.as_ref()).collect();
                while let Some(shape) = stack.pop() {
                    sheets.push(&shape.sheet);
                    stack.extend(shape.children.iter().map(|c| c.as_ref()));
                }
            }
            for sheet in sheets {
                for section in sheet.sections.iter().filter(|s| s.name.as_str() == "Character") {
                    for row in section.rows.iter() {
                        if let Some(value) = row.cells.get("LangID").and_then(|c| c.value.as_ref()) {
                            assert!(tag_or_none(value.text()), "a LangID of {:?}", value.text());
                        }
                    }
                }
            }
            if let Some(id) = doc.sheet.cell("DocLangID").and_then(|c| c.value.as_ref()) {
                assert!(
                    diacraft_doc::lang::parse(id.text()).is_ok() && doc.props.language.as_deref() == Some(id.text()),
                    "DocLangID {:?}",
                    id.text()
                );
            }
        }
        // The comments: a number once, a reply on the page and the shape of the comment it answers (which has no parent), a text within the cap, and an index that says
        // which of each page's annotation elements were taken: ascending, among the page's annotation children, and as many in all as the comments.
        let mut numbers = std::collections::HashSet::new();
        for c in doc.comments.iter() {
            assert!(numbers.insert(c.id), "a comment's number is its own");
            assert!(c.text.len() <= diacraft_doc::MAX_COMMENT_TEXT_BYTES);
            if let Some(parent) = c.parent {
                let to = doc.comments.get(parent).expect("a reply answers a comment that is there");
                assert!(to.parent.is_none() && (to.page, to.shape) == (c.page, c.shape), "a thread is one level deep and on one page and shape");
            }
        }
        assert!(doc.comments.len() <= diacraft_doc::MAX_COMMENTS && doc.comments.authors().count() <= diacraft_doc::MAX_COMMENT_AUTHORS);
        let mut taken = 0usize;
        for at in 0..report.index.pages() {
            let (places, total) = report.index.page(at).expect("an index page").comments();
            assert!(places.windows(2).all(|w| w[0] < w[1]) && places.iter().all(|p| *p < total));
            taken += places.len();
        }
        assert_eq!(taken, doc.comments.len(), "each comment is one annotation element the index names");
        // What the model holds of the package: each entry once, none that the model reads as something else, only a folder with no bytes, the bytes within
        // the media budget, the faces within their cap, and a flat drawing holds no package.
        let mut names = std::collections::HashSet::new();
        for part in doc.parts.iter() {
            assert!(names.insert(part.name.clone()), "a part is held once");
            assert_eq!(part.format, diacraft_doc::Format::Odf);
            assert!(!matches!(&*part.name, "mimetype" | "META-INF/manifest.xml" | "content.xml" | "styles.xml"));
            assert!(!part.name.ends_with('/') || part.bytes.is_empty());
        }
        assert!(doc.parts.iter().map(|p| p.bytes.len() as u64).sum::<u64>() <= limits.max_media_bytes);
        assert!(doc.fonts.len() <= 65_536);
        if report.form == diacraft_odg::Form::Flat {
            assert!(doc.parts.is_empty());
        }
        // What the model holds of the constructs its cells do not say: a style sheet's text values are stamped by their row, its other values are of the groups
        // a style gives, a paragraph holds at most the cap of levels and the first one opens every list it is in, an element is held without its children and the runs are in the
        // order they begin.
        let gradients = data.first() == Some(&9);
        for style in doc.styles.iter() {
            if gradients {
                gradient_rows_are_the_renderers(&style.sheet);
            }
            let Some(data) = style.ext.get() else { continue };
            let row = |section: &str| {
                style
                    .sheet
                    .section(&diacraft_doc::SectionKey::new(section, None))
                    .and_then(|s| s.row(&diacraft_doc::RowKey::Ix(0)))
                    .map_or_else(diacraft_doc::Row::default, |r| (**r).clone())
            };
            let (paragraph, run) = (row("Paragraph"), row("Character"));
            for value in &data.odf.text_held {
                let row = if value.group.is_paragraph() { &paragraph } else { &run };
                assert_eq!(value.stamp, Some(diacraft_doc::stamp_text_group(row, value.group)));
            }
            for held in &data.odf.held {
                assert!(!matches!(held.group, diacraft_doc::HeldGroup::Frame | diacraft_doc::HeldGroup::Glue | diacraft_doc::HeldGroup::Geometry));
            }
        }
        assert_eq!(report.index.pages(), doc.pages.len());
        for (at, page) in doc.pages.iter().enumerate() {
            let index = report.index.page(at).expect("an index page for each page");
            // A group's members are modelled shapes of the page too.
            let mut all: Vec<&diacraft_doc::Shape> = Vec::new();
            let mut stack: Vec<&diacraft_doc::Shape> = page.shapes.iter().map(|s| s.as_ref()).collect();
            while let Some(shape) = stack.pop() {
                all.push(shape);
                stack.extend(shape.children.iter().map(|c| c.as_ref()));
            }
            assert_eq!(index.modelled(), all.len());
            // The page's own `.odg` data: a page that holds a master page's name holds the stamp of its cells.
            if let Some(data) = page.ext.get() {
                assert!(data.odf.master_page.is_none() || data.odf.stamp.is_some());
            }
            for shape in all {
                assert!(index.ordinal(shape.id).is_some());
                if gradients {
                    gradient_rows_are_the_renderers(&shape.sheet);
                }
                // The held values: a value of the Frame group is judged by the pose and has no stamp, a value of the Geometry group that is tied to cells has one, and the
                // units held are the six a length is written in.
                if let Some(data) = shape.ext.get() {
                    for held in &data.odf.held {
                        if held.group == diacraft_doc::HeldGroup::Frame {
                            assert!(held.stamp.is_none());
                        }
                        assert!(held.group != diacraft_doc::HeldGroup::Glue, "a glue point is held on its row");
                    }
                    for (_, unit) in data.odf.units.iter() {
                        assert!(["cm", "mm", "in", "pt", "pc", "px"].contains(&unit), "{unit}");
                    }
                }
                // A glue point's value is stamped by the row it is on, whole.
                if let Some(section) = shape.sheet.section(&diacraft_doc::SectionKey::new("Connection", None)) {
                    for row in section.rows.iter() {
                        if let Some(held) = row.ext.get().and_then(|d| d.odf.held.clone()) {
                            assert_eq!(held.stamp, Some(diacraft_doc::stamp_row(row)));
                        }
                    }
                }
                // A text's paragraphs: a run lies in its paragraph, and each paragraph holds the stamp of its row.
                if let Some(text) = shape.text.as_ref().and_then(|t| t.ext.get().map(|d| d.odf.clone())) {
                    for (first, paragraph) in text.paragraphs.iter().enumerate() {
                        assert!(paragraph.style.stamp.is_some() && paragraph.characters.is_some());
                        assert!(paragraph.runs.iter().all(|run| run.from <= run.to));
                        assert!(paragraph.levels.len() <= diacraft_doc::MAX_LIST_LEVELS);
                        if first == 0 {
                            assert!(paragraph.levels.iter().all(|l| l.list.is_some() && l.item.is_some()));
                        }
                        assert!(paragraph.element.as_ref().is_none_or(|e| e.children.is_empty()));
                        assert!(paragraph.runs.iter().all(|r| r.element.as_ref().is_none_or(|e| e.children.is_empty())));
                        assert!(paragraph.runs.windows(2).all(|w| w[0].from < w[1].from || (w[0].from == w[1].from && w[0].to >= w[1].to)));
                    }
                }
                // The rows of a `Connection` section have one `IX` each.
                if let Some(section) = shape.sheet.section(&diacraft_doc::SectionKey::new("Connection", None)) {
                    let mut seen = std::collections::HashSet::new();
                    assert!(section.rows.iter().all(|r| seen.insert(r.ix)));
                }
            }
            // A glued end's `ToPart` is 3 or 100 plus a row a section holds, never a saturated number.
            for connect in page.connects.iter() {
                assert!(connect.to_part.is_none_or(|p| p == 3 || (100..100 + diacraft_doc::MAX_ROWS_PER_SECTION as u32).contains(&p)));
            }
        }
    }
});
