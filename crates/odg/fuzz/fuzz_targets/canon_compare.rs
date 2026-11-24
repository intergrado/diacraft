//! `cargo fuzz run canon_compare`: the content check for OpenDocument drawings on a drawing built from the fuzzer's choices, as a package and as a flat
//! `.fodg` (done-criterion D5).
//!
//! **Structured modes** (the first byte is 0 to 5): the rest is read three bytes at a time as items of a drawing, each picking a fragment from a table: a
//! common style, an automatic style, a font face, or a shape whose place is spelled one of several ways and whose text is one of several (spaces, `text:s`,
//! a link with a lone space between two spans, a picture named by an href or held inline). The drawing is built as a package and as a flat file of the same
//! pieces, and a second drawing is made from the first by the edits that follow the items (two items swapped, one dropped, one repeated, one respelled).
//! **Raw modes** (6 and 7): the rest is the content of `office:drawing` of an otherwise honest package or flat file, so the fuzzer explores the readers.
//! The properties:
//!
//! * a drawing against itself is the same, in each form, when it can be read;
//! * the flat file of a drawing's pieces is the same content as the package of the same pieces (structured modes: the faithful assembly of the two forms);
//! * the rule of empty containers: the drawing with its empty `office:font-face-decls`, `office:styles` and `office:automatic-styles` left out, and with
//!   an empty `office:scripts` written in the package and not in the flat file, is the same content in each form and across them;
//! * the count is symmetric: `count(a, b) == count(b, a)`, when neither is refused, for every pair of the four files, and in the raw modes for every pair where neither file
//!   can hold DiaCraft's markup ([`may_carry`]). That markup is the one thing the check is oriented by (what only the second file holds of it counts as nothing), and
//!   a raw input may write it: a flat file whose root holds an attribute of it counts one more against its own drawing than the other way round;
//! * a flat file can never spell a marker of a package's joined root: the flat file of a drawing's pieces with `part-root-attribute-0` or
//!   `part-name-clash-0` written by hand on its root is not the same as the package that has the marker, and is the same as itself;
//! * a drawing laid out another way (white space between the shapes, a comment after the root) is the same;
//! * Second round: a drawing whose shapes the written file marks with DiaCraft's attributes is the same content as the drawing it was, in each form and across
//!   them, **and not the other way round** (the check is oriented by the carrier: what the source holds must be in the written file); and a drawing with copies of its automatic styles
//!   under other names, when no name is held by two styles of one family, is the same content as the one without them;
//! * The drawing with its automatic styles under other names, and the shapes pointing to the names they were given (when no name is held by two styles
//!   of one family), is the same content in each form and across them, and a shape that points to another style is not; and a drawing whose `meta.xml` says another generator
//!   and writes its fields in another order is the same content, as a package and as a flat file;
//! * the only refusals are an error with a class (a limit, a part that does not parse): never a panic, a hang, or a list longer than 200 or than the count.
#![no_main]

use diacraft_container::XmlLimits;
use diacraft_container::xml::{Node, parse_with};
use diacraft_container::zip::{ArchiveWriter, Method};
use diacraft_odg::{Form, Limits, compare_content};
use libfuzzer_sys::fuzz_target;

/// DiaCraft's namespace, as [`NS`] binds it: the markup the check is oriented by.
const DIACRAFT: &str = "urn:app.diacraft:extension";

const NS: &str = concat!(
    r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
    r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
    r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" "#,
    r#"xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:diacraft="urn:app.diacraft:extension""#
);
const MEDIA: &str = "application/vnd.oasis.opendocument.graphics";
const PNG: [u8; 20] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];

const COMMON: [&str; 6] = [
    r##"<style:style style:name="A" style:family="graphic"><style:graphic-properties svg:stroke-width="0.1cm" draw:fill-color="#ff0000"/></style:style>"##,
    r#"<style:style style:name="B" style:family="graphic"><style:graphic-properties svg:stroke-width="0.10cm" fo:padding-top="1mm"/></style:style>"#,
    r#"<style:style style:name="A" style:family="paragraph"><style:paragraph-properties fo:margin-left="1cm" fo:line-height="150%"/></style:style>"#,
    r#"<draw:marker draw:name="M" svg:viewBox="0 0 20 30" svg:d="M10 0l-10 30h20z"/>"#,
    r#"<draw:stroke-dash draw:name="D" draw:dots1="1" draw:dots1-length="0.2cm"/>"#,
    r#"<style:default-style style:family="graphic"><style:text-properties fo:font-size="12pt"/></style:default-style>"#,
];
/// What a shape may name as its style: nothing, or a graphic style that the automatic styles may hold (`gr1`, `gr2`) or not (`gone`).
const STYLED: [&str; 4] = ["", " draw:style-name=\"gr1\"", " draw:style-name=\"gr2\"", " draw:style-name=\"gone\""];
const AUTO: [&str; 4] = [
    r#"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="A"/>"#,
    r#"<style:style style:name="gr2" style:family="graphic"><style:graphic-properties svg:stroke-width="2pt"/></style:style>"#,
    r#"<style:style style:name="gr1" style:family="paragraph"/>"#,
    r#"<style:style style:name="P1" style:family="paragraph"><style:text-properties fo:letter-spacing="0.05cm"/></style:style>"#,
];
const FONTS: [&str; 3] = [
    r#"<style:font-face style:name="Mono" svg:font-family="Mono"/>"#,
    r#"<style:font-face style:name="Sans" svg:font-family="Sans"/>"#,
    r#"<style:font-face style:name="Mono" svg:font-family="Other"/>"#,
];
const PLACES: [&str; 8] = ["1cm", "1.0cm", "1.00cm", "25mm", "+1cm", "1e0cm", "007", "2.5CM"];
const TEXTS: [&str; 10] = [
    "a b",
    "a  b",
    "a<text:s/>b",
    "a<text:s text:c=\"2\"/>b",
    " a ",
    "<text:a xlink:href=\"u\"><text:span>a</text:span> <text:span>b</text:span></text:a>",
    "<text:span>a</text:span><text:span>b</text:span>",
    "a<?x y?>b",
    "a<!-- c -->b",
    "<text:meta> <text:span>a</text:span></text:meta>",
];

#[derive(Clone, Copy)]
struct Item {
    kind: u8,
    a: u8,
    b: u8,
}

/// The pieces of a drawing, as the parts and the flat file hold them.
struct Pieces {
    common: String,
    auto: String,
    /// How many times each of the automatic styles of [`AUTO`] was chosen: a name held twice by styles of one family is not one the check can rename.
    auto_chosen: [u8; 4],
    fonts: String,
    /// The page's shapes as a package holds them (a picture by its href or inline) and as a flat file does (always inline).
    pages: String,
    flat_pages: String,
    picture: bool,
    /// The containers that hold nothing are left out, where they are written empty otherwise.
    omit: bool,
}

fn pick<'a, const N: usize>(table: &'a [&'a str; N], at: u8) -> &'a str {
    table.get(usize::from(at) % N).copied().unwrap_or_default()
}

fn pieces(items: &[Item], spacing: &str) -> Pieces {
    let mut p = Pieces {
        common: String::new(),
        auto: String::new(),
        auto_chosen: [0; 4],
        fonts: String::new(),
        pages: String::new(),
        flat_pages: String::new(),
        picture: false,
        omit: false,
    };
    for (n, item) in items.iter().enumerate() {
        match item.kind % 5 {
            0 => p.common.push_str(pick(&COMMON, item.a)),
            1 => {
                p.auto.push_str(pick(&AUTO, item.a));
                if let Some(held) = p.auto_chosen.get_mut(usize::from(item.a) % AUTO.len()) {
                    *held = held.saturating_add(1);
                }
            }
            2 => p.fonts.push_str(pick(&FONTS, item.a)),
            3 => {
                let shape = format!(
                    "{spacing}<draw:custom-shape draw:name=\"s{n}\"{} svg:x=\"{}\" svg:y=\"1cm\" svg:width=\"2cm\" svg:height=\"1cm\"><text:p>{}</text:p></draw:custom-shape>",
                    pick(&STYLED, item.a ^ item.b),
                    pick(&PLACES, item.a),
                    pick(&TEXTS, item.b)
                );
                p.pages.push_str(&shape);
                p.flat_pages.push_str(&shape);
            }
            _ => {
                p.picture = true;
                let inline = format!(
                    "<draw:image xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"><office:binary-data>{}</office:binary-data></draw:image>",
                    base64(&PNG)
                );
                let by_href = r#"<draw:image xlink:href="Pictures/p.png" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/>"#;
                let frame = |image: &str| {
                    format!(
                        "{spacing}<draw:frame draw:name=\"f{n}\" svg:x=\"1cm\" svg:y=\"1cm\" svg:width=\"1cm\" svg:height=\"1cm\">{image}</draw:frame>"
                    )
                };
                // A package's picture named by an href is given the media type the manifest says (`image/png`), as the converter writes it into a flat file
                // (ruling Call 23), so the flat file of the same pieces holds it inline with that type; a picture inline in the package has none.
                let inline_typed = inline.replacen("<draw:image ", "<draw:image draw:mime-type=\"image/png\" ", 1);
                p.pages.push_str(&frame(if item.a % 2 == 0 { by_href } else { &inline }));
                p.flat_pages.push_str(&frame(if item.a % 2 == 0 { &inline_typed } else { &inline }));
            }
        }
    }
    p
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for k in 0..4 {
            out.push(if k <= chunk.len() { char::from(A[((n >> (18 - 6 * k)) & 63) as usize]) } else { '=' });
        }
    }
    out
}

const LAYOUT: &str =
    r#"<style:page-layout style:name="PM1"><style:page-layout-properties fo:page-width="21cm" fo:page-height="29.7cm"/></style:page-layout>"#;
const MASTER: &str = r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"#;

/// One of the containers a drawing may write empty, as the whole element: nothing when it holds nothing and the drawing leaves such containers out (the rule of
/// empty containers: an empty one is the same as an absent one).
fn container(p: &Pieces, name: &str, inner: &str) -> String {
    if p.omit && inner.is_empty() { String::new() } else { format!("<office:{name}>{inner}</office:{name}>") }
}

fn content_xml(p: &Pieces, tail: &str) -> String {
    // A package writes an empty `office:scripts`, unless it leaves the empty ones out; the flat file of the same pieces never has one.
    let scripts = if p.omit { "" } else { "<office:scripts/>" };
    format!(
        "<?xml version=\"1.0\"?>\n<office:document-content {NS} office:version=\"1.3\">{scripts}{}{}<office:body><office:drawing><draw:page draw:name=\"p\" draw:master-page-name=\"Default\">{}</draw:page></office:drawing></office:body></office:document-content>{tail}",
        container(p, "font-face-decls", &p.fonts),
        container(p, "automatic-styles", &p.auto),
        p.pages
    )
}

fn styles_xml(p: &Pieces, tail: &str) -> String {
    format!(
        "<?xml version=\"1.0\"?>\n<office:document-styles {NS} office:version=\"1.3\">{}{}<office:automatic-styles>{LAYOUT}</office:automatic-styles><office:master-styles>{MASTER}</office:master-styles></office:document-styles>{tail}",
        container(p, "font-face-decls", &p.fonts),
        container(p, "styles", &p.common)
    )
}

fn package(content: &str, styles: &str, picture: bool) -> Option<Vec<u8>> {
    package_with_meta(content, styles, picture, None)
}

/// The package, with a `meta.xml` of this text when there is one.
fn package_with_meta(content: &str, styles: &str, picture: bool, meta: Option<&str>) -> Option<Vec<u8>> {
    let mut rows = String::new();
    for (name, media) in [("content.xml", "text/xml"), ("styles.xml", "text/xml")] {
        rows.push_str(&format!("<manifest:file-entry manifest:full-path=\"{name}\" manifest:media-type=\"{media}\"/>"));
    }
    if meta.is_some() {
        rows.push_str("<manifest:file-entry manifest:full-path=\"meta.xml\" manifest:media-type=\"text/xml\"/>");
    }
    if picture {
        rows.push_str("<manifest:file-entry manifest:full-path=\"Pictures/p.png\" manifest:media-type=\"image/png\"/>");
    }
    let manifest = format!(
        "<?xml version=\"1.0\"?><manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\"><manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"{MEDIA}\"/>{rows}</manifest:manifest>"
    );
    let mut w = ArchiveWriter::new();
    w.add("mimetype", MEDIA.as_bytes(), Method::Stored).ok()?;
    w.add("content.xml", content.as_bytes(), Method::Deflated).ok()?;
    w.add("styles.xml", styles.as_bytes(), Method::Deflated).ok()?;
    if let Some(meta) = meta {
        w.add("meta.xml", meta.as_bytes(), Method::Deflated).ok()?;
    }
    if picture {
        w.add("Pictures/p.png", &PNG, Method::Stored).ok()?;
    }
    w.add("META-INF/manifest.xml", manifest.as_bytes(), Method::Deflated).ok()?;
    w.finish().ok()
}

fn flat(p: &Pieces, tail: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\"?>\n<office:document {NS} office:version=\"1.3\" office:mimetype=\"{MEDIA}\">{}{}<office:automatic-styles>{LAYOUT}{}</office:automatic-styles><office:master-styles>{MASTER}</office:master-styles><office:body><office:drawing><draw:page draw:name=\"p\" draw:master-page-name=\"Default\">{}</draw:page></office:drawing></office:body></office:document>{tail}",
        container(p, "font-face-decls", &p.fonts),
        container(p, "styles", &p.common),
        p.auto,
        p.flat_pages
    )
    .into_bytes()
}

/// Small limits, so that every limit is within reach of a small input.
fn limits() -> Limits {
    let mut l = Limits::default();
    l.container.xml = XmlLimits { max_depth: 24, max_nodes: 3000, max_attr_bytes: 256 };
    l.container.max_part_bytes = 1 << 16;
    l
}

fn count(a: &[u8], b: &[u8]) -> Option<u64> {
    match compare_content(a, b, &limits()) {
        Ok(d) => {
            assert!(d.shown.len() <= 200 && d.shown.len() as u64 <= d.count, "the list is capped and within the count");
            Some(d.count)
        }
        Err(e) => {
            assert!(!e.class().is_empty(), "a refusal has a class: {e}");
            None
        }
    }
}

fn symmetric(a: &[u8], b: &[u8]) {
    if let (Some(ab), Some(ba)) = (count(a, b), count(b, a)) {
        assert_eq!(ab, ba, "count(a, b) == count(b, a)");
    }
}

/// Whether `file` can hold DiaCraft's markup, so that the count of a pair with it need not be symmetric (orients the check by it). A package can: its entries are not
/// looked into here (they may be compressed, and an entry under `DiaCraft/` is the carrier), and the structured modes hold packages to symmetry. So can a file that does not parse,
/// which the check refuses in any case. A flat file can when one of its names, an element's or an attribute's, has a prefix that a declaration of the file binds to the namespace
/// (for an element with no prefix, the default namespace), wherever the two stand: wider than the scope, so that nothing the check reads as the markup is missed.
fn may_carry(file: &[u8]) -> bool {
    if Form::of(file) == Form::Package {
        return true;
    }
    let Ok(root) = parse_with(file, &limits().container.xml) else { return true };
    let (mut bound, mut elements, mut attributes) = (Vec::new(), Vec::new(), Vec::new());
    let mut stack = vec![&root];
    while let Some(el) = stack.pop() {
        elements.push(el.name.as_str());
        for (key, value) in &el.attrs {
            match (key.as_str(), key.strip_prefix("xmlns:")) {
                ("xmlns", _) if value == DIACRAFT => bound.push(""),
                (_, Some(prefix)) if value == DIACRAFT => bound.push(prefix),
                ("xmlns", _) | (_, Some(_)) => {}
                (name, None) => attributes.push(name),
            }
        }
        stack.extend(el.children.iter().filter_map(|node| match node {
            Node::El(child) => Some(child),
            _ => None,
        }));
    }
    let prefix = |name: &str| name.split_once(':').map(|(prefix, _)| prefix.to_owned());
    let element = |name: &&str| bound.contains(&prefix(name).unwrap_or_default().as_str());
    let attribute = |name: &&str| prefix(name).is_some_and(|p| bound.contains(&p.as_str()));
    elements.iter().any(element) || attributes.iter().any(attribute)
}

fn echo(x: &[u8]) {
    if let Some(c) = count(x, x) {
        assert_eq!(c, 0, "a drawing against itself");
    }
}

/// The flat file `flat` with `attribute` written by hand at the start of its root.
fn spell(flat: &[u8], attribute: &str) -> Vec<u8> {
    String::from_utf8_lossy(flat).replacen("<office:document ", &format!("<office:document {attribute} "), 1).into_bytes()
}

/// A flat file can never spell a marker of a package's joined root: a package whose `content.xml` root has an attribute of its own (the joined root says so in
/// `part-root-attribute-0`), and a package whose two parts hold a face of one name that differs (it says so in `part-name-clash-0`), are each not the same as the
/// flat file of the pieces with that marker written by hand; and that flat file is the same as itself (its own attribute is compared as one).
fn forged(items: &[Item]) {
    let p = pieces(items, "");
    let content = content_xml(&p, "").replacen("office:version=\"1.3\"", "office:version=\"1.3\" foo=\"bar\"", 1);
    if let Some(pkg) = package(&content, &styles_xml(&p, ""), p.picture) {
        let spelled = spell(&flat(&p, ""), "part-root-attribute-0=\"content.xml foo=bar\"");
        echo(&spelled);
        if let Some(c) = count(&spelled, &pkg) {
            assert!(c >= 1, "a flat file that spells the marker of a part root's attribute is the package that has it");
        }
        symmetric(&spelled, &pkg);
    }
    let (mut in_styles, mut in_content, mut both) = (pieces(items, ""), pieces(items, ""), pieces(items, ""));
    in_styles.fonts = FONTS[0].to_owned();
    in_content.fonts = FONTS[2].to_owned();
    both.fonts = format!("{}{}", FONTS[0], FONTS[2]);
    if let Some(pkg) = package(&content_xml(&in_content, ""), &styles_xml(&in_styles, ""), in_content.picture) {
        let spelled = spell(&flat(&both, ""), "part-name-clash-0=\"font-face-decls: a name of the later part is the first part's too\"");
        echo(&spelled);
        if let Some(c) = count(&spelled, &pkg) {
            assert!(c >= 1, "a flat file that spells the marker of a name both parts hold is the package that has it");
        }
        symmetric(&spelled, &pkg);
    }
}

/// A `meta.xml` whose fields are `fields`, written by a program of the name `generator`.
fn meta_xml(generator: &str, fields: &str) -> String {
    format!(
        "<?xml version=\"1.0\"?>\n<office:document-meta {NS} xmlns:meta=\"urn:oasis:names:tc:opendocument:xmlns:meta:1.0\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" office:version=\"1.3\"><office:meta><meta:generator>{generator}</meta:generator>{fields}</office:meta></office:document-meta>"
    )
}

/// The `office:meta` of the same fields, as a flat file holds it (the first child of the root).
fn flat_with_meta(flat: &[u8], generator: &str, fields: &str) -> Vec<u8> {
    let meta = format!(
        "<office:meta xmlns:meta=\"urn:oasis:names:tc:opendocument:xmlns:meta:1.0\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><meta:generator>{generator}</meta:generator>{fields}</office:meta>"
    );
    let text = String::from_utf8_lossy(flat).into_owned();
    let Some(start) = text.find("<office:document ") else { return flat.to_vec() };
    let Some(end) = text.get(start..).and_then(|rest| rest.find('>')) else { return flat.to_vec() };
    let at = start + end + 1;
    format!("{}{meta}{}", text.get(..at).unwrap_or_default(), text.get(at..).unwrap_or_default()).into_bytes()
}

/// The generator and the order of the fields of `office:meta` are no content, in a package and in a flat file, and a field that is no generator is.
fn meta_is_free(items: &[Item]) {
    let p = pieces(items, "");
    let (content, styles) = (content_xml(&p, ""), styles_xml(&p, ""));
    let fields = ("<dc:title>T</dc:title><meta:keyword>k</meta:keyword>", "<meta:keyword>k</meta:keyword><dc:title>T</dc:title>");
    let (Some(one), Some(other), Some(more)) = (
        package_with_meta(&content, &styles, p.picture, Some(&meta_xml("G/1", fields.0))),
        package_with_meta(&content, &styles, p.picture, Some(&meta_xml("H/2", fields.1))),
        package_with_meta(&content, &styles, p.picture, Some(&meta_xml("G/1", &format!("{}<dc:subject>S</dc:subject>", fields.0)))),
    ) else {
        return;
    };
    if let Some(c) = count(&one, &other) {
        assert_eq!(c, 0, "another generator and the fields in another order");
    }
    if let Some(c) = count(&one, &more) {
        assert!(c >= 1, "a field added is a difference");
    }
    let flat_bytes = flat(&p, "");
    let (flat_one, flat_other) = (flat_with_meta(&flat_bytes, "G/1", fields.0), flat_with_meta(&flat_bytes, "H/2", fields.1));
    if let Some(c) = count(&flat_one, &flat_other) {
        assert_eq!(c, 0, "the same in a flat file");
    }
    if let Some(c) = count(&flat_one, &one) {
        assert_eq!(c, 0, "a flat file and a package of the same fields");
    }
    symmetric(&one, &more);
}

/// `p` with every shape marked with an attribute of DiaCraft's namespace, as a writer of the model marks what it writes.
fn marked(p: &Pieces) -> Pieces {
    let mark = |text: &str| text.replace("<draw:custom-shape ", "<draw:custom-shape diacraft:id=\"7\" ");
    Pieces {
        common: p.common.clone(),
        auto: p.auto.clone(),
        auto_chosen: p.auto_chosen,
        fonts: p.fonts.clone(),
        pages: mark(&p.pages),
        flat_pages: mark(&p.flat_pages),
        picture: p.picture,
        omit: p.omit,
    }
}

/// `p` with a copy of each of its automatic styles under another name (`gr1` is copied as `cp1`, `gr2` as `cp2`, `P1` as `cP1`), after the styles.
fn copied(p: &Pieces) -> Pieces {
    let copy = |text: &str| text.replace("\"gr1\"", "\"cp1\"").replace("\"gr2\"", "\"cp2\"").replace("\"P1\"", "\"cP1\"");
    Pieces {
        common: p.common.clone(),
        auto: format!("{}{}", p.auto, copy(&p.auto)),
        auto_chosen: p.auto_chosen,
        fonts: p.fonts.clone(),
        pages: p.pages.clone(),
        flat_pages: p.flat_pages.clone(),
        picture: p.picture,
        omit: p.omit,
    }
}

/// DiaCraft's attributes on the written file's shapes are no difference, in a package and in a flat file and across them; the source's must be there (the other way round,
/// when there is a shape, is a difference).
fn carrier_is_free(items: &[Item]) {
    let p = pieces(items, "");
    let q = marked(&p);
    let (Some(pkg_p), Some(pkg_q)) =
        (package(&content_xml(&p, ""), &styles_xml(&p, ""), p.picture), package(&content_xml(&q, ""), &styles_xml(&q, ""), q.picture))
    else {
        return;
    };
    let (flat_p, flat_q) = (flat(&p, ""), flat(&q, ""));
    for (x, y) in [(&pkg_p, &pkg_q), (&flat_p, &flat_q), (&flat_p, &pkg_q), (&pkg_p, &flat_q)] {
        if let Some(c) = count(x, y) {
            assert_eq!(c, 0, "the markup the written file adds");
        }
    }
    if p.pages.contains("<draw:custom-shape ") {
        for (x, y) in [(&pkg_q, &pkg_p), (&flat_q, &flat_p)] {
            if let Some(c) = count(x, y) {
                assert!(c >= 1, "the markup the source had, missing from the written file");
            }
        }
    }
}

/// Copies of the automatic styles under other names count as one, when no name is held by two styles of one family.
fn copies_are_one(items: &[Item]) {
    let p = pieces(items, "");
    if p.auto_chosen.iter().any(|n| *n > 1) {
        return;
    }
    let q = copied(&p);
    let (Some(pkg_p), Some(pkg_q)) =
        (package(&content_xml(&p, ""), &styles_xml(&p, ""), p.picture), package(&content_xml(&q, ""), &styles_xml(&q, ""), q.picture))
    else {
        return;
    };
    let (flat_p, flat_q) = (flat(&p, ""), flat(&q, ""));
    for (x, y) in [(&pkg_p, &pkg_q), (&flat_p, &flat_q), (&flat_p, &pkg_q), (&pkg_p, &flat_q)] {
        if let Some(c) = count(x, y) {
            assert_eq!(c, 0, "the copies of the automatic styles");
        }
        symmetric(x, y);
    }
}

/// `p` with the automatic styles under other names: `gr1` is `zr1`, `gr2` is `zr2`, `P1` is `zP1`, in the styles and in what names them.
fn renamed(p: &Pieces) -> Pieces {
    let again = |text: &str| text.replace("\"gr1\"", "\"zr1\"").replace("\"gr2\"", "\"zr2\"").replace("\"P1\"", "\"zP1\"");
    Pieces {
        common: p.common.clone(),
        auto: again(&p.auto),
        auto_chosen: p.auto_chosen,
        fonts: p.fonts.clone(),
        pages: again(&p.pages),
        flat_pages: again(&p.flat_pages),
        picture: p.picture,
        omit: p.omit,
    }
}

/// Names that are free. A drawing with its automatic styles under other names (and the shapes naming them) is the same content as it was, in each form and across them,
/// when no name is held by two styles of one family; and one shape pointing to another of the two styles is not.
fn names_are_free(items: &[Item]) {
    let p = pieces(items, "");
    // The property holds when each name that a shape gives is the name of one graphic style (a reference to no style is left as it is on both sides, and so is a name
    // two styles of one family hold).
    let held_twice = p.auto_chosen.iter().any(|n| *n > 1);
    let dangling =
        (p.pages.contains("\"gr1\"") && p.auto_chosen.first() != Some(&1)) || (p.pages.contains("\"gr2\"") && p.auto_chosen.get(1) != Some(&1));
    if held_twice || dangling {
        return;
    }
    let q = renamed(&p);
    let (Some(pkg_p), Some(pkg_q)) =
        (package(&content_xml(&p, ""), &styles_xml(&p, ""), p.picture), package(&content_xml(&q, ""), &styles_xml(&q, ""), q.picture))
    else {
        return;
    };
    let (flat_p, flat_q) = (flat(&p, ""), flat(&q, ""));
    for (x, y) in [(&pkg_p, &pkg_q), (&flat_p, &flat_q), (&flat_p, &pkg_q), (&pkg_p, &flat_q)] {
        if let Some(c) = count(x, y) {
            assert_eq!(c, 0, "the automatic styles under other names");
        }
        symmetric(x, y);
    }
    // A shape that names the other style is a shape that differs: when the drawing has two of its styles and a shape naming the first, naming the second changes it.
    let swapped = |pieces: &Pieces| Pieces {
        common: pieces.common.clone(),
        auto: pieces.auto.clone(),
        auto_chosen: pieces.auto_chosen,
        fonts: pieces.fonts.clone(),
        pages: pieces.pages.replacen("\"zr1\"", "\"zr2\"", 1),
        flat_pages: pieces.flat_pages.replacen("\"zr1\"", "\"zr2\"", 1),
        picture: pieces.picture,
        omit: pieces.omit,
    };
    let (graphic_one, graphic_two) = (p.auto_chosen.first().copied().unwrap_or(0), p.auto_chosen.get(1).copied().unwrap_or(0));
    if graphic_one == 1 && graphic_two == 1 && q.pages.contains("\"zr1\"") {
        let r = swapped(&q);
        if let (Some(pkg_r), Some(count_pr)) = (package(&content_xml(&r, ""), &styles_xml(&r, ""), r.picture), count(&pkg_p, &flat(&r, ""))) {
            // The two styles differ (`gr1` names its parent, `gr2` sets a width), so a shape that points to the other one is another shape.
            assert!(count_pr >= 1, "a shape that names the other style");
            symmetric(&pkg_p, &pkg_r);
        }
    }
}

fn edit(items: &mut Vec<Item>, edits: &[u8]) {
    for e in edits.chunks_exact(3).take(8) {
        let (op, at, other) = (e[0] % 5, usize::from(e[1]), usize::from(e[2]));
        let n = items.len().max(1);
        match op {
            0 => {
                if items.len() > 1 {
                    items.swap(at % n, other % n);
                }
            }
            1 => {
                if !items.is_empty() {
                    items.remove(at % n);
                }
            }
            2 => {
                if let Some(item) = items.get(at % n).copied() {
                    items.push(item);
                }
            }
            3 => {
                if let Some(item) = items.get_mut(at % n) {
                    item.a = item.a.wrapping_add(1);
                }
            }
            _ => {
                if let Some(item) = items.get_mut(at % n) {
                    item.b = item.b.wrapping_add(1);
                }
            }
        }
    }
}

fn structured(data: &[u8]) {
    let (head, edits) = data.split_at(data.len().min(30 * 3));
    let mut a: Vec<Item> = head.chunks_exact(3).map(|c| Item { kind: c[0], a: c[1], b: c[2] }).collect();
    let pa = pieces(&a, "");
    let (pkg_a, flat_a) = (package(&content_xml(&pa, ""), &styles_xml(&pa, ""), pa.picture), flat(&pa, ""));
    let Some(pkg_a) = pkg_a else { return };
    // The faithful assembly: the flat file of the pieces is the content of the package of the pieces.
    if let Some(c) = count(&flat_a, &pkg_a) {
        assert_eq!(c, 0, "the flat file and the package of one drawing's pieces");
    }
    echo(&pkg_a);
    echo(&flat_a);
    // The rule of empty containers: the drawing with the empty ones left out (and the package's empty `office:scripts` too) is the same content as with them
    // written, in each form and across the forms.
    let mut omitted = pieces(&a, "");
    omitted.omit = true;
    if let Some(pkg_o) = package(&content_xml(&omitted, ""), &styles_xml(&omitted, ""), omitted.picture) {
        let flat_o = flat(&omitted, "");
        for (x, y) in [(&pkg_a, &pkg_o), (&flat_a, &flat_o), (&flat_a, &pkg_o), (&pkg_a, &flat_o), (&pkg_o, &flat_o)] {
            if let Some(c) = count(x, y) {
                assert_eq!(c, 0, "a drawing with its empty containers written and with them left out");
            }
        }
    }
    forged(&a);
    names_are_free(&a);
    meta_is_free(&a);
    carrier_is_free(&a);
    copies_are_one(&a);
    // Another layout of the same drawing: white space between the shapes, a comment after each root.
    let spaced = pieces(&a, "\n   ");
    if let Some(pkg_spaced) = package(&content_xml(&spaced, "\n<!-- end -->"), &styles_xml(&spaced, "\n<!-- end -->"), spaced.picture) {
        if let Some(c) = count(&pkg_a, &pkg_spaced) {
            assert_eq!(c, 0, "a package laid out another way");
        }
        if let Some(c) = count(&flat_a, &flat(&spaced, "\n<!-- end -->")) {
            assert_eq!(c, 0, "a flat file laid out another way");
        }
    }
    edit(&mut a, edits);
    let pb = pieces(&a, "");
    if let Some(pkg_b) = package(&content_xml(&pb, ""), &styles_xml(&pb, ""), pb.picture) {
        let flat_b = flat(&pb, "");
        for (x, y) in [(&pkg_a, &pkg_b), (&flat_a, &flat_b), (&flat_a, &pkg_b), (&pkg_a, &flat_b)] {
            symmetric(x, y);
        }
    }
}

fn raw(mode: u8, data: &[u8]) {
    let body = String::from_utf8_lossy(data).into_owned();
    let p = Pieces {
        common: String::new(),
        auto: String::new(),
        auto_chosen: [0; 4],
        fonts: String::new(),
        pages: body.clone(),
        flat_pages: body,
        picture: false,
        omit: false,
    };
    let flat_bytes = flat(&p, "");
    let Some(pkg) = package(&content_xml(&p, ""), &styles_xml(&p, ""), false) else { return };
    echo(&flat_bytes);
    echo(&pkg);
    // The package holds what the flat file holds of the fuzzer's, the body, under the same declarations, and no carrier: the flat file tells whether either can hold DiaCraft's
    // markup.
    if !may_carry(&flat_bytes) {
        symmetric(&flat_bytes, &pkg);
        // The bytes themselves as a flat file and as the one part of a package.
        if mode == 7 && !may_carry(data) {
            symmetric(data, &pkg);
            symmetric(data, &flat_bytes);
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((mode, rest)) = data.split_first() else { return };
    match mode % 8 {
        0..=5 => structured(rest),
        m => raw(m, rest),
    }
});
