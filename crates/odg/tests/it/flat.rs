//! Flat drawings, `.fodg`, the reader's side.
//!
//! A flat drawing is built here from the same pieces a package's parts are (`drawing.rs`), laid out in the order, so that the oracle of most cases is
//! the model **the package of the same pieces gives**: a flat drawing is read as the package that holds the same elements.

use diacraft_doc::Document;
use diacraft_odg::{Form, Limits, Variant, read, sniff, sniff_form};

use crate::drawing::{Drawing, close, cm, only, page, without_package};

pub const MIME: &str = "application/vnd.oasis.opendocument.graphics";

/// A flat drawing being built: each child of `office:document`, as the text of the whole element (empty: left out).
#[derive(Clone)]
pub struct Flat {
    pub declaration: String,
    /// The root's start tag, ending with `>`.
    pub root: String,
    pub meta: String,
    pub settings: String,
    pub scripts: String,
    pub fonts: String,
    pub styles: String,
    pub auto: String,
    pub masters: String,
    pub body: String,
    /// Children in another namespace, put last.
    pub tail: String,
    /// The line break and indent put before each child, as a producer lays the file out.
    pub gap: String,
}

/// The whole element `name` in `whole`, from its start tag to its end tag (or an empty element), or `""` when there is none.
fn element(whole: &str, name: &str) -> String {
    let open = format!("<{name}");
    let Some(from) = whole.find(&open) else { return String::new() };
    let rest = &whole[from..];
    let close = format!("</{name}>");
    match (rest.find(&close), rest.find("/>")) {
        (Some(end), Some(empty)) if empty < end && !rest[..empty].contains('>') => rest[..empty + 2].to_owned(),
        (Some(end), _) => rest[..end + close.len()].to_owned(),
        (None, Some(empty)) => rest[..empty + 2].to_owned(),
        _ => String::new(),
    }
}

impl Flat {
    /// The flat drawing of the pieces of `d`: its meta and settings parts' elements, the faces, the common styles, the one container of automatic styles
    /// (`styles.xml`'s, then `content.xml`'s), the master styles and the pages.
    pub fn of(d: &Drawing) -> Flat {
        let wrap = |name: &str, inner: &str| if inner.is_empty() { String::new() } else { format!("<{name}>{inner}</{name}>") };
        Flat {
            declaration: "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n".to_owned(),
            root: format!("<office:document {} office:version=\"{}\" office:mimetype=\"{MIME}\">", d.ns, d.version),
            meta: d.meta.as_deref().map(|m| element(m, "office:meta")).unwrap_or_default(),
            settings: d.settings.as_deref().map(|m| element(m, "office:settings")).unwrap_or_default(),
            scripts: String::new(),
            fonts: wrap("office:font-face-decls", &d.fonts),
            styles: wrap("office:styles", &d.common),
            auto: wrap("office:automatic-styles", &format!("{}{}", d.styles_auto, d.auto)),
            masters: wrap("office:master-styles", &d.masters),
            body: format!("<office:body><office:drawing>{}</office:drawing></office:body>", d.pages),
            tail: String::new(),
            gap: "\n ".to_owned(),
        }
    }

    pub fn text(&self) -> String {
        let mut out = format!("{}{}", self.declaration, self.root);
        for child in [&self.meta, &self.settings, &self.scripts, &self.fonts, &self.styles, &self.auto, &self.masters, &self.body, &self.tail] {
            if !child.is_empty() {
                out.push_str(&self.gap);
                out.push_str(child);
            }
        }
        out.push_str("\n</office:document>\n");
        out
    }

    pub fn bytes(&self) -> Vec<u8> {
        self.text().into_bytes()
    }

    pub fn read(&self) -> (Document, diacraft_odg::ReadReport) {
        match read(&self.bytes(), &Limits::default()) {
            Ok(done) => done,
            Err(e) => panic!("the flat drawing did not read: {} ({e})", e.class()),
        }
    }
}

/// The class of the error a read gives, or `None` when it reads.
pub fn read_class(bytes: &[u8]) -> Option<&'static str> {
    read(bytes, &Limits::default()).err().map(|e| e.class())
}

/// A model without what only the file's own form decides: both forms' models of the same pieces are equal.
fn same_model(flat: &Document, package: &Document) {
    assert_eq!(without_package(flat), without_package(package), "a flat drawing reads as the package that holds the same elements");
}

// ----------------------------------------------------------------------------------------------------------------------------------------

/// The root is found by its URI whatever its prefix, and what it says of the media type gives the variant.
#[test]
fn odg_package_sniff_tells_a_flat_drawing_by_its_root() {
    let office = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
    let flat = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document xmlns:office=\"{office}\" office:mimetype=\"{MIME}\" office:version=\"1.4\"><office:body><office:drawing><draw:page xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\"/></office:drawing></office:body></office:document>"
    );
    assert_eq!(sniff(flat.as_bytes()), Some(Variant::Drawing));
    assert_eq!(sniff_form(flat.as_bytes()), Some((Variant::Drawing, Form::Flat)));
    // Another prefix, and the template type.
    let prefixed = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<o:document xmlns:o=\"{office}\" o:mimetype=\"{MIME}-template\" o:version=\"1.4\"><o:body/></o:document>"
    );
    assert_eq!(sniff(prefixed.as_bytes()), Some(Variant::Template), "a sniffer that matches the prefix office: gives None here");
    // Another kind of document: no drawing, and the reader says which it is.
    let text = flat.replace(MIME, "application/vnd.oasis.opendocument.text");
    assert_eq!(sniff(text.as_bytes()), None);
    let err = read(text.as_bytes(), &Limits::default()).expect_err("a text document is no drawing");
    assert_eq!(err.class(), "not_a_drawing");
    assert!(err.to_string().contains("application/vnd.oasis.opendocument.text"), "{err}");
    // The root of a package's part is no flat drawing.
    let content = flat.replace("office:document ", "office:document-content ").replace("</office:document>", "</office:document-content>");
    assert_eq!(sniff(content.as_bytes()), None);
    // The first start tag must be within the first 64 KiB.
    let far = format!("<?xml version=\"1.0\"?><!--{}-->{}", "c".repeat(70_000), flat.split_once('\n').map(|(_, rest)| rest).unwrap_or_default());
    assert_eq!(sniff(far.as_bytes()), None);
    // A package is told as before, with the form that says so.
    assert_eq!(sniff_form(&Drawing::a4().page("").bytes()), Some((Variant::Drawing, Form::Package)));
    assert_eq!(Form::of(&Drawing::a4().page("").bytes()), Form::Package);
    // Bytes that are neither a package nor XML are still not a ZIP.
    assert_eq!(sniff(b"not a drawing"), None);
    assert_eq!(read_class(b"not a drawing"), Some("not_zip"));
}

/// A flat drawing that says nothing, or says `…graphics-flat-xml`, is a drawing, with the warning; one with an empty body is `no_drawing_content`.
#[test]
fn odg_package_a_flat_drawing_with_no_media_type_or_a_flat_xml_one_reads_with_a_warning() {
    let rect = r#"<draw:rect draw:name="r" svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"/>"#;
    let base = Flat::of(&Drawing::a4().page(rect));
    let (doc, report) = base.read();
    assert_eq!((report.warned("mimetype_irregular"), report.variant, report.form), (0, Some(Variant::Drawing), Form::Flat));
    assert_eq!(page(&doc, 0).shapes.len(), 1);
    for (value, word) in
        [(None, "absent"), (Some("application/vnd.oasis.opendocument.graphics-flat-xml"), "unrecognised"), (Some("text/plain"), "unrecognised")]
    {
        let mut flat = base.clone();
        flat.root = match value {
            Some(v) => flat.root.replace(MIME, v),
            None => flat.root.replace(&format!(" office:mimetype=\"{MIME}\""), ""),
        };
        assert_eq!(sniff(&flat.bytes()), Some(Variant::Drawing), "{value:?}");
        let (doc, report) = flat.read();
        assert_eq!(page(&doc, 0).shapes.len(), 1, "{value:?}: the page with its rectangle");
        assert_eq!(report.warned("mimetype_irregular"), 1, "{value:?}");
        assert!(report.warnings.iter().any(|w| w.contains(word)), "{value:?}: {:?}", report.warnings);
    }
    // No media type and an empty body: nothing to read.
    let mut empty = base.clone();
    empty.root = empty.root.replace(&format!(" office:mimetype=\"{MIME}\""), "");
    empty.body = "<office:body/>".to_owned();
    assert_eq!(read_class(&empty.bytes()), Some("no_drawing_content"));
    empty.body = String::new();
    assert_eq!(read_class(&empty.bytes()), Some("no_drawing_content"), "no body at all");
}

/// The one container of automatic styles holds `gr1` and the page layout; the model is the package's with `gr1` in `content.xml`.
#[test]
fn odg_package_a_flat_drawing_reads_as_the_package_that_holds_the_same_elements() {
    let mut d = Drawing::a4().common(r##"<style:style style:name="Box" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/></style:style>"##);
    d.auto = r##"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="Box"><style:graphic-properties draw:fill-color="#cc3333"/></style:style>"##.to_owned();
    let d = d.page(r#"<draw:custom-shape draw:style-name="gr1" draw:name="c" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#);
    let (package_doc, _) = d.read();
    let (flat_doc, report) = Flat::of(&d).read();
    let s = only(&flat_doc);
    assert_eq!(crate::drawing::text(s.sheet.cell("FillForegnd")), "#CC3333", "the local value of gr1");
    let links = s.sheet.links;
    assert!(links.line.is_some_and(|l| l != 0) && links.fill == links.line && links.text == links.line, "three links to Box: {links:?}");
    close(crate::drawing::num(page(&flat_doc, 0).sheet.cell("PageWidth")), cm(21.0));
    same_model(&flat_doc, &package_doc);
    assert_eq!(report.warned("style_name_repeated"), 0);
    // The master page's shapes look in the same container.
    let mut m = Drawing::a4();
    m.auto = r##"<style:style style:name="gr9" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#112233"/></style:style>"##.to_owned();
    m.masters.push_str(r#"<style:master-page style:name="Back" style:page-layout-name="PM1"><draw:rect draw:style-name="gr9" svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="2cm"/></style:master-page>"#);
    let m = m.page("");
    let (flat_doc, _) = Flat::of(&m).read();
    let background = flat_doc.pages.iter().find(|p| p.background).expect("the master page that holds a shape is a background page");
    let shape = background.shapes.get(0).expect("its rectangle");
    assert_eq!(crate::drawing::text(shape.sheet.cell("FillForegnd")), "#112233", "a master page's shape finds its style in the one container");
    // Two graphic styles of one name in the one container: the first is used, with one warning that names it.
    let mut twice = d.clone();
    twice
        .auto
        .push_str(r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill-color="#3366aa"/></style:style>"##);
    let (doc, report) = Flat::of(&twice).read();
    assert_eq!(crate::drawing::text(only(&doc).sheet.cell("FillForegnd")), "#CC3333");
    assert_eq!(report.warned("style_name_repeated"), 1);
    assert!(report.warnings.iter().any(|w| w.contains("gr1")), "{:?}", report.warnings);
    // One warning for a name, however many times the container holds it again; one for each name held twice.
    let style = |name: &str, color: &str| {
        format!(r##"<style:style style:name="{name}" style:family="graphic"><style:graphic-properties draw:fill-color="{color}"/></style:style>"##)
    };
    let mut thrice = twice.clone();
    thrice.auto.push_str(&style("gr1", "#445566"));
    assert_eq!(Flat::of(&thrice).read().1.warned("style_name_repeated"), 1, "a name held three times is warned of once");
    let mut two_names = twice.clone();
    two_names.auto.push_str(&style("gr2", "#445566"));
    two_names.auto.push_str(&style("gr2", "#778899"));
    let report = Flat::of(&two_names).read().1;
    assert_eq!(report.warned("style_name_repeated"), 2, "{:?}", report.warnings);
    assert!(report.warnings.iter().any(|w| w.contains("gr2")) && report.warnings.iter().any(|w| w.contains("gr1")), "{:?}", report.warnings);
}

// ----------------------------------------------------------------------------------------------------------------------------------------
// Pictures, objects, DiaCraft's extension, limits, macros, order.

/// Base64 (RFC 4648, padded) of `bytes`, in lines of `width` characters joined by `sep`.
pub fn base64_lines(bytes: &[u8], width: usize, sep: &str) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut chars = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for k in 0..4 {
            if k <= chunk.len() {
                chars.push(char::from(A[((n >> (18 - 6 * k)) & 63) as usize]));
            } else {
                chars.push('=');
            }
        }
    }
    let lines: Vec<&str> = chars.as_bytes().chunks(width).map(|c| std::str::from_utf8(c).unwrap()).collect();
    lines.join(sep)
}

/// A picture of `n` bytes whose first bytes are a PNG's signature.
pub fn picture(n: usize) -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.extend((0..n.saturating_sub(8)).map(|i| (i * 7 % 251) as u8));
    bytes
}

/// A frame holding the `draw:image` `image`.
pub fn frame(image: &str) -> String {
    format!(r#"<draw:frame draw:name="pic" svg:x="1cm" svg:y="2cm" svg:width="3cm" svg:height="2cm">{image}</draw:frame>"#)
}

/// The picture bytes of the first shape of the first page, and its `ForeignType`.
fn picture_of(doc: &Document) -> (Option<Vec<u8>>, Option<String>) {
    let shape = only(doc);
    let foreign = shape.foreign.as_ref();
    (foreign.and_then(|f| f.picture.as_ref()).map(|m| m.bytes.to_vec()), foreign.and_then(|f| f.attrs.get("ForeignType")).map(str::to_owned))
}

/// The reader's side: an inline picture is decoded whatever the lines it is wrapped in; a linked one is a placeholder, counted, with one warning; an
/// href beside inline data is ignored.
#[test]
fn odg_package_inline_pictures_are_decoded_and_linked_ones_are_counted() {
    let png = picture(300);
    // Lines of 40 characters, each after the first starting with four spaces.
    let data = base64_lines(&png, 40, "\n    ");
    let inline = frame(&format!(r#"<draw:image draw:mime-type="image/png"><office:binary-data>{data}</office:binary-data></draw:image>"#));
    let (doc, report) = Flat::of(&Drawing::a4().page(&inline)).read();
    let (bytes, kind) = picture_of(&doc);
    assert_eq!(bytes.as_deref(), Some(&png[..]), "the picture's bytes, decoded exactly");
    assert_eq!(kind.as_deref(), Some("Bitmap"));
    assert_eq!(report.linked_pictures, 0);
    assert_eq!(report.work.media_bytes, png.len() as u64, "its characters are charged to the media budget");
    // A linked picture: a placeholder (a picture-less foreign shape), one warning that counts it, and nothing is opened.
    let linked = frame(r#"<draw:image xlink:href="pic.png"/>"#);
    let (doc, report) = Flat::of(&Drawing::a4().page(&linked)).read();
    assert_eq!(picture_of(&doc), (None, None), "no picture: the placeholder draws");
    assert_eq!((report.linked_pictures, report.warned("linked_picture_not_read"), report.warned("picture_missing")), (1, 1, 0));
    assert!(report.warnings.iter().any(|w| w.starts_with("1 linked")), "{:?}", report.warnings);
    assert_eq!(report.kept("picture_linked"), 1);
    // Three linked pictures are one warning that says three.
    let three = Drawing::a4().page(&format!("{linked}{linked}{linked}"));
    let report = Flat::of(&three).read().1;
    assert_eq!((report.linked_pictures, report.warned("linked_picture_not_read")), (3, 1));
    assert!(report.warnings.iter().any(|w| w.starts_with("3 linked")), "{:?}", report.warnings);
    // Inline data with an href beside it is the inline picture; the href is ignored and nothing is linked.
    let both =
        frame(&format!(r#"<draw:image xlink:href="x.png" draw:mime-type="image/png"><office:binary-data>{data}</office:binary-data></draw:image>"#));
    let (doc, report) = Flat::of(&Drawing::a4().page(&both)).read();
    assert_eq!(picture_of(&doc).0.as_deref(), Some(&png[..]));
    assert_eq!((report.linked_pictures, report.warned("linked_picture_not_read")), (0, 0));
    // The same drawing as a package follows the href, as it always has (the rule of R33 is the flat form's).
    let mut p = Drawing::a4().page(&both);
    p.files.push(("x.png".to_owned(), crate::write_core::PNG.to_vec()));
    assert_eq!(picture_of(&p.read().0).0.as_deref(), Some(&crate::write_core::PNG[..]));
    // Bytes that are no picture the reader draws are kept as the shape's with no type.
    let junk =
        frame(&format!(r#"<draw:image><office:binary-data>{}</office:binary-data></draw:image>"#, base64_lines(b"not a picture at all", 76, "\n")));
    assert_eq!(picture_of(&Flat::of(&Drawing::a4().page(&junk)).read().0).1, None);
    // Base64 that is none is no picture.
    let bad = frame(r#"<draw:image><office:binary-data>!!!! not base64</office:binary-data></draw:image>"#);
    assert_eq!(picture_of(&Flat::of(&Drawing::a4().page(&bad)).read().0), (None, None));
}

/// The inline data beside an href is the flat form's alone: a package's `draw:image` follows its href, whatever data it holds, as it always did.
#[test]
fn odg_package_a_package_image_follows_its_href_and_not_the_inline_data_beside_it() {
    let png = picture(300);
    let encoded = base64_lines(&png, 76, "\n");
    let data = |text: &str| format!(r#"<draw:image xlink:href="Pictures/a.png"><office:binary-data>{text}</office:binary-data></draw:image>"#);
    // The href names a picture, the data is not one: the picture is the part's.
    let mut d = Drawing::a4().page(&frame(&data("!!not base64!!")));
    d.files.push(("Pictures/a.png".to_owned(), png.clone()));
    let (bytes, kind) = picture_of(&d.read().0);
    assert_eq!((bytes, kind.as_deref()), (Some(png.clone()), Some("Bitmap")), "the part's bytes, shown as a picture");
    // The href names a part that is no picture, the data is one: the frame shows none (the data is not looked at).
    let mut d = Drawing::a4().page(&frame(&data(&encoded)));
    d.files.push(("Pictures/a.png".to_owned(), b"not a picture at all, a text".to_vec()));
    assert_eq!(picture_of(&d.read().0).1, None, "the part is no picture a frame can show: no picture type");
}

/// A frame whose `draw:object` holds an inline `office:document` is drawn from its picture, and the inner drawing is no part of the outer one.
#[test]
fn odg_package_an_inline_object_is_part_of_its_frame_and_never_of_the_drawing() {
    let png = picture(64);
    let inner = format!(
        r#"<office:document office:mimetype="{MIME}" office:version="1.4"><office:body><office:drawing><draw:page draw:name="inner"><draw:rect draw:name="inside" svg:x="0cm" svg:y="0cm" svg:width="1cm" svg:height="1cm"/></draw:page></office:drawing></office:body></office:document>"#
    );
    let object = format!(
        r#"<draw:frame draw:name="obj" svg:x="1cm" svg:y="2cm" svg:width="3cm" svg:height="2cm"><draw:object>{inner}</draw:object><draw:image draw:mime-type="image/png"><office:binary-data>{}</office:binary-data></draw:image></draw:frame>"#,
        base64_lines(&png, 76, "\n")
    );
    let (doc, report) = Flat::of(&Drawing::a4().page(&object)).read();
    assert_eq!(doc.pages.len(), 1, "the inner drawing's page is not a page of the drawing");
    assert_eq!(page(&doc, 0).shapes.len(), 1, "the frame; not the inner rectangle");
    assert_eq!(picture_of(&doc).0.as_deref(), Some(&png[..]), "the frame is drawn from its replacement image");
    assert_eq!(report.stats.shapes, 1);
}

/// The reader's side: DiaCraft's extension is the first child in its namespace wherever it stands; a second is one warning; a file without it reads.
#[test]
fn odg_package_diacraft_extension_is_read_from_wherever_it_stands() {
    let ext = |n: u32| format!(r#"<diacraft:Data diacraft:version="1"><diacraft:snap n="{n}"/></diacraft:Data>"#);
    let base = Flat::of(&Drawing::a4().page(""));
    // None: tier 3 decides; no error.
    let (doc, report) = base.read();
    assert!(doc.diacraft.is_none());
    assert_eq!((report.warned("extension_repeated"), report.warned("flat_order_irregular")), (0, 0));
    // Last, after the body (where a save puts it).
    let mut last = base.clone();
    last.tail = ext(1);
    let (doc, report) = last.read();
    let data = doc.diacraft.as_ref().expect("the extension is read");
    assert_eq!(&*data.name, "diacraft:Data");
    assert_eq!(doc.diacraft_child("snap").and_then(|c| c.attrs.get("n")), Some("1"));
    assert_eq!((report.warned("extension_repeated"), report.warned("flat_order_irregular")), (0, 0), "a foreign child is no disorder");
    // As the first child, and after `office:styles`: the same.
    let mut first = base.clone();
    first.meta = ext(1) + &first.meta;
    assert_eq!(first.read().0.diacraft_child("snap").and_then(|c| c.attrs.get("n")), Some("1"));
    let mut middle = base.clone();
    middle.styles = format!("{}{}{}", middle.styles, middle.gap, ext(1));
    assert_eq!(middle.read().0.diacraft_child("snap").and_then(|c| c.attrs.get("n")), Some("1"));
    // Two: the first is used, with one warning.
    let mut two = base.clone();
    two.tail = ext(1) + &two.gap + &ext(2);
    let (doc, report) = two.read();
    assert_eq!(doc.diacraft_child("snap").and_then(|c| c.attrs.get("n")), Some("1"));
    assert_eq!(report.warned("extension_repeated"), 1);
    // Any other foreign child is kept and counted, not read.
    let mut foreign = base.clone();
    foreign.tail = r#"<x:other xmlns:x="urn:x"><x:a/></x:other>"#.to_owned();
    let (doc, report) = foreign.read();
    assert!(doc.diacraft.is_none());
    assert_eq!(report.kept("foreign_child"), 1);
}

/// A flat file past the container's cap on a part is refused before it is parsed; its tree is held to the reader's cap; a DOCTYPE is refused and not
/// told; UTF-16 is read directly and not told.
#[test]
fn odg_package_limits_doctype_and_utf16() {
    // The cap is 256 MiB: one byte past it is `part_too_large`, with nothing parsed (the XML events say so).
    let cap = diacraft_container::Limits::default().max_part_bytes as usize;
    let mut big = Flat::of(&Drawing::a4().page("")).bytes();
    big.resize(cap + 1, b' ');
    let (result, report) = diacraft_odg::read_partial(&big, &Limits::default());
    assert_eq!(result.err().map(|e| e.class()), Some("part_too_large"));
    assert_eq!(report.work.package.xml_events, 0, "refused before it is parsed");
    // At the cap exactly it is parsed (with a lowered cap, so that the test does not hold a quarter of a gigabyte).
    let small = Flat::of(&Drawing::a4().page("")).bytes();
    let limits = |n: usize| Limits { container: diacraft_container::Limits { max_part_bytes: n as u64, ..Default::default() }, ..Limits::default() };
    assert_eq!(read(&small, &limits(small.len())).err().map(|e| e.class()), None);
    assert_eq!(read(&small, &limits(small.len() - 1)).err().map(|e| e.class()), Some("part_too_large"));
    // The reader's cap on a tree: one item past it is `xml_too_complex`, one within it reads.
    let flat = Flat::of(&Drawing::a4().page(""));
    let capped = |n: usize| Limits { max_read_xml_nodes: n, ..Limits::default() };
    let items = (1..10_000).find(|n| read_with(&flat.bytes(), &capped(*n)).is_none()).expect("the drawing reads at some cap");
    assert!(items > 50, "the tree is more than a few items: {items}");
    assert_eq!(read_with(&flat.bytes(), &capped(items - 1)), Some("xml_too_complex"), "one item past the cap");
    assert_eq!(read_with(&flat.bytes(), &capped(items)), None, "the cap exactly");
    // A DOCTYPE with an external id before the root: not told, and refused when read directly.
    let doctype = flat.text().replacen("<office:document", "<!DOCTYPE office:document SYSTEM \"x.dtd\">\n<office:document", 1);
    assert_eq!(sniff(doctype.as_bytes()), None);
    assert_eq!(read_class(doctype.as_bytes()), Some("xml_entity_refused"));
    // UTF-16 with a byte-order mark: not told; read directly, the drawing.
    let utf16 = utf16le(&flat.text().replace("encoding=\"UTF-8\"", "encoding=\"UTF-16\""));
    assert_eq!(sniff(&utf16), None);
    assert_eq!(Form::of(&utf16), Form::Flat);
    let (doc, _) = read(&utf16, &Limits::default()).expect("a flat drawing in UTF-16 reads when read directly");
    same_model(&doc, &flat.read().0);
}

/// The encoding a flat file declares is checked by the reader (the container's R24 rule, which R36 says is the container's), and not by the
/// sniff, which tells a file by its root: a declaration that is not the encoding of the bytes is told as a flat drawing and refused when it is read, and an edit of
/// it is never made from a model the reader gave.
#[test]
fn odg_package_a_declared_encoding_that_is_not_the_bytes_is_told_and_refused_when_read() {
    let flat = Flat::of(&Drawing::a4().page(""));
    for declared in ["ISO-8859-1", "windows-1252", "UTF-16", "us-ascii", "UTF-32"] {
        let text = flat.text().replace("encoding=\"UTF-8\"", &format!("encoding=\"{declared}\""));
        assert!(sniff(text.as_bytes()).is_some(), "told by its root whatever it declares: {declared}");
        assert!(read_class(text.as_bytes()).is_some(), "{declared} is refused when read");
    }
    // UTF-8 in any case, with the declaration as the standard spells it, reads.
    for declared in ["UTF-8", "utf-8", "Utf-8"] {
        let text = flat.text().replace("encoding=\"UTF-8\"", &format!("encoding=\"{declared}\""));
        assert_eq!(read_class(text.as_bytes()), None, "{declared}");
    }
}

fn read_with(bytes: &[u8], limits: &Limits) -> Option<&'static str> {
    read(bytes, limits).err().map(|e| e.class())
}

/// The text in UTF-16, little-endian, with a byte-order mark.
pub fn utf16le(text: &str) -> Vec<u8> {
    let mut out = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        out.extend(unit.to_le_bytes());
    }
    out
}

/// The scripts element of a flat drawing: one `office:script` holding `libraries`.
fn scripts(libraries: &str) -> String {
    format!(
        r#"<office:scripts><office:script script:language="ooo:Basic"><ooo:libraries xmlns:ooo="http://openoffice.org/2004/office" xmlns:xlink="http://www.w3.org/1999/xlink">{libraries}</ooo:libraries></office:script></office:scripts>"#
    )
}

/// A declaration of empty Basic libraries is no macro, in a flat drawing or in a package's `content.xml`; a module or a linked library is.
#[test]
fn odg_package_an_empty_basic_library_declaration_is_no_macro() {
    let base = Flat::of(&Drawing::a4().page(""));
    let macros = |libraries: Option<&str>| {
        let mut flat = base.clone();
        flat.scripts = libraries.map(scripts).unwrap_or_default();
        flat.read().1.macros
    };
    assert!(!macros(None));
    assert!(!macros(Some("")), "an empty ooo:libraries");
    assert!(!macros(Some(r#"<ooo:library-embedded ooo:name="Lib1"/>"#)), "one empty embedded library");
    assert!(
        !macros(Some(r#"<ooo:library-embedded ooo:name="Lib1"> </ooo:library-embedded><ooo:library-embedded ooo:name="Lib2"/>"#)),
        "empty, with white space"
    );
    assert!(
        macros(Some(
            r#"<ooo:library-embedded ooo:name="Lib1"><ooo:module ooo:name="M1">Sub Main
End Sub</ooo:module></ooo:library-embedded>"#
        )),
        "a module holding source"
    );
    assert!(macros(Some(r#"<ooo:library-linked ooo:name="Lib1" xlink:href="vnd.sun.star.expand:$BASIC/x"/>"#)), "a linked library");
    assert!(macros(Some(r#"<ooo:library-embedded ooo:name="Lib1">text</ooo:library-embedded>"#)), "text in a library");
    // Text or another element in the `office:script`, or an `office:script` with no libraries at all.
    let mut flat = base.clone();
    flat.scripts = r#"<office:scripts><office:script script:language="ooo:Basic">Sub Main</office:script></office:scripts>"#.to_owned();
    assert!(flat.read().1.macros);
    flat.scripts = r#"<office:scripts><office:script script:language="ooo:Basic"><other/></office:script></office:scripts>"#.to_owned();
    assert!(flat.read().1.macros);
    flat.scripts = r#"<office:scripts><office:script script:language="ooo:Basic"/></office:scripts>"#.to_owned();
    assert!(flat.read().1.macros);
    // A library in another namespace is not the declaration of one.
    flat.scripts = r#"<office:scripts><office:script><ooo:libraries xmlns:ooo="urn:x"/></office:script></office:scripts>"#.to_owned();
    assert!(flat.read().1.macros);
    // The same in a package's `content.xml`.
    let in_package = |libraries: &str| {
        let d = Drawing::a4().page("");
        let content = d.content_xml().replace("<office:body>", &format!("{}<office:body>", scripts(libraries)));
        let package = crate::build::package(
            crate::build::DRAWING,
            vec![("content.xml".to_owned(), content.into_bytes()), ("styles.xml".to_owned(), d.styles_xml().into_bytes())],
        );
        read(&package, &Limits::default()).unwrap().1.macros
    };
    assert!(!in_package(r#"<ooo:library-embedded ooo:name="Lib1"/>"#));
    assert!(in_package(r#"<ooo:library-linked ooo:name="Lib1"/>"#));
}

/// Children out of the standard's order, or repeated, are read by their names, the first of each, with one warning.
#[test]
fn odg_package_children_out_of_order_or_repeated_are_read_by_name() {
    let mut d = Drawing::a4().common(r##"<style:style style:name="Box" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/></style:style>"##);
    d.auto = r##"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="Box"><style:graphic-properties draw:fill-color="#cc3333"/></style:style>"##.to_owned();
    let d = d.page(r#"<draw:rect draw:style-name="gr1" svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"/>"#);
    let regular = Flat::of(&d);
    let (expected, report) = regular.read();
    assert_eq!(report.warned("flat_order_irregular"), 0);
    // `office:styles` after `office:automatic-styles`.
    let swapped = regular.text().replacen(
        &format!("{}{}{}{}", regular.gap, regular.styles, regular.gap, regular.auto),
        &format!("{}{}{}{}", regular.gap, regular.auto, regular.gap, regular.styles),
        1,
    );
    assert_ne!(swapped, regular.text());
    let (doc, report) = read(swapped.as_bytes(), &Limits::default()).expect("a drawing out of order reads");
    same_model(&doc, &expected);
    assert_eq!(report.warned("flat_order_irregular"), 1);
    // The body first: the same.
    let body_first = regular.text().replacen(&format!("{}{}", regular.gap, regular.body), "", 1).replacen(
        &regular.root,
        &format!("{}{}{}", regular.root, regular.gap, regular.body),
        1,
    );
    let (doc, report) = read(body_first.as_bytes(), &Limits::default()).expect("a body first reads");
    same_model(&doc, &expected);
    assert_eq!(report.warned("flat_order_irregular"), 1);
    // A child repeated: the first is used, and the second, which would change the styles or add a page, is not.
    let mut again = regular.clone();
    again.tail = r##"<office:automatic-styles><style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill-color="#000000"/></style:style></office:automatic-styles><office:body><office:drawing><draw:page draw:name="second" draw:master-page-name="Default"/></office:drawing></office:body>"##.to_owned();
    let (doc, report) = again.read();
    same_model(&doc, &expected);
    assert_eq!(report.warned("flat_order_irregular"), 1, "one warning, whatever the number of faults");
    assert_eq!(report.warned("style_name_repeated"), 0, "the second container is not read");
}

/// The bytes of XML a read parses are capped as for a package (the 512 MiB, here lowered): past it, `too_much_xml`, and the cap is exact.
#[test]
fn odg_package_the_xml_bytes_of_a_flat_read_are_capped() {
    let bytes = Flat::of(&Drawing::a4().page("")).bytes();
    let limits = |n: u64| Limits { max_xml_bytes: n, ..Limits::default() };
    assert_eq!(read_with(&bytes, &limits(bytes.len() as u64)), None);
    assert_eq!(read_with(&bytes, &limits(bytes.len() as u64 - 1)), Some("too_much_xml"));
}

/// A flat tree of 4,000,001 items is `xml_too_complex`, at the default cap of the reader's trees (this holds the 4,000,000 items the parser built
/// before it refused, about 0.9 GB, so the test is in the `memory-heavy` group of `.config/nextest.toml`).
#[test]
fn odg_package_a_tree_of_4000001_items_is_xml_too_complex() {
    let base = Flat::of(&Drawing::a4().page(""));
    let capped = |n: usize| Limits { max_read_xml_nodes: n, ..Limits::default() };
    // The items the document holds as it is: the smallest cap it reads under.
    let items = (1..10_000).find(|n| read_with(&base.bytes(), &capped(*n)).is_none()).expect("the document reads at some cap");
    // Empty elements of the foreign namespace, one item each, up to 4,000,001 items in all: the first, `<x:a xmlns:x=".."/>`, is an element and an attribute (two
    // items), and the line break before the tail is a text (one).
    let mut flat = base.clone();
    flat.tail = "<x:a xmlns:x=\"urn:x\"/>".to_owned() + &"<x:a/>".repeat(4_000_001 - items - 3);
    let class = read_with(&flat.bytes(), &Limits::default());
    assert_eq!(class, Some("xml_too_complex"));
    // One item fewer is read: it is the limit's own, refused at 4,000,001, not at 4,000,000.
    let mut one_less = base.clone();
    one_less.tail = "<x:a xmlns:x=\"urn:x\"/>".to_owned() + &"<x:a/>".repeat(4_000_000 - items - 3);
    let (result, report) = diacraft_odg::read_partial(&one_less.bytes(), &Limits::default());
    assert!(result.is_ok(), "{:?}", result.err().map(|e| e.class()));
    assert_eq!(report.kept("foreign_child"), 4_000_000 - items as u64 - 2);
}

/// What the census found inline beside the drawing's pictures is read and kept: a `draw:fill-image` of `office:styles` and an embedded font's
/// `svg:font-face-uri`, each holding `office:binary-data`, change nothing of what the drawing reads and are in the document's bytes after a save.
#[test]
fn odg_package_inline_data_in_the_styles_and_fonts_is_kept() {
    let data = base64_lines(&picture(80), 76, "\n");
    let mut d = Drawing::a4().page(&crate::write_core::custom("s1", "1cm"));
    d.fonts = format!(
        "<style:font-face style:name=\"Embedded\" svg:font-family=\"Embedded\"><svg:font-face-src><svg:font-face-uri><office:binary-data>{data}</office:binary-data></svg:font-face-uri></svg:font-face-src></style:font-face>"
    );
    d.common =
        format!("<draw:fill-image draw:name=\"Bg\" draw:display-name=\"Bg\"><office:binary-data>{data}</office:binary-data></draw:fill-image>");
    let plain = Drawing::a4().page(&crate::write_core::custom("s1", "1cm"));
    let (doc, report) = Flat::of(&d).read();
    assert_eq!(report.linked_pictures, 0);
    assert_eq!(doc.pages.get(0).map(|p| p.shapes.len()), Some(1));
    // The shape reads as it does without them.
    assert_eq!(only(&doc).sheet, only(&Flat::of(&plain).read().0).sheet);
    // And an edited save keeps them as written.
    let mut o = crate::write_core::open_bytes(Flat::of(&d).bytes());
    crate::write_core::move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    let saved = crate::flat_write::save(&o.doc, &o.bytes, &o.index).unwrap();
    let after = crate::flat_write::text_of(&saved.bytes);
    assert_eq!(after.matches(&data).count(), 2);
}

/// DiaCraft's extension is charged to the document's items by its elements, attributes and texts, and to its bytes by every name and value, before it is
/// copied into the model.
#[test]
fn odg_package_the_extension_is_charged_by_its_items_and_bytes() {
    let base = Flat::of(&Drawing::a4().page(""));
    let ext = format!("<dc:Data xmlns:dc=\"{}\" dc:version=\"1\"><dc:snap n=\"1\"/>text</dc:Data>", diacraft_doc::DIACRAFT_URN);
    let mut with = base.clone();
    with.tail = ext.clone();
    let (a, b) = (base.read().1.work, with.read().1.work);
    // Two elements, three attributes and one text are six items; the bytes of the names and values, in lines of 64 for an item each.
    let bytes =
        ["dc:Data", "xmlns:dc", diacraft_doc::DIACRAFT_URN, "dc:version", "1", "dc:snap", "n", "1", "text"].iter().map(|s| s.len()).sum::<usize>();
    // The element's own start-up cost is in the document either way: what the extension adds is it, the line break before it, and its charge.
    let charged = 6 + bytes.div_ceil(64);
    assert!(b.items >= a.items + charged as u64, "{} over {}", b.items, a.items);
    assert!(b.items <= a.items + charged as u64 + 3, "{} over {}: only the extension is charged", b.items, a.items);
    // A document whose budget the extension does not fit is refused as items.
    let budget = |n: usize| Limits { max_items: n, ..Limits::default() };
    let items = (1..1_000_000).step_by(1).find(|n| read(&with.bytes(), &budget(*n)).is_ok()).expect("it reads at some budget");
    assert!(read(&base.bytes(), &budget(items - 1)).is_ok() || items > 0);
    assert_eq!(read(&with.bytes(), &budget(items - 1)).err().map(|e| e.class()), Some("too_many_items"));
}

/// The generator and the scale come from the first `office:meta` and the first `office:settings`; what is no `office:meta` is not looked in, and a scale that
/// cannot be read is one warning that names the flat document's element.
#[test]
fn odg_package_the_generator_and_the_scale_are_the_first_ones() {
    let items = r#"<config:config-item config:name="ScaleNumerator" config:type="int">1</config:config-item><config:config-item config:name="ScaleDenominator" config:type="int">2</config:config-item>"#;
    let mut d = Drawing::a4().page("");
    d.settings = Some(crate::drawing::settings(items));
    d.meta = Some(crate::drawing::meta("First/1"));
    let (package_doc, package_report) = d.read();
    let (flat_doc, report) = Flat::of(&d).read();
    same_model(&flat_doc, &package_doc);
    assert_eq!(report.generator.as_deref(), Some("First/1"));
    assert_eq!(report.generator, package_report.generator);
    let scale =
        |doc: &Document| crate::drawing::num(page(doc, 0).sheet.cell("PageScale")) / crate::drawing::num(page(doc, 0).sheet.cell("DrawingScale"));
    assert!((scale(&flat_doc) - 0.5).abs() < 1e-9, "the drawing scale 1 to 2 of the settings: {}", scale(&flat_doc));
    // The first `office:meta`'s first generator; a foreign element holding a generator is none, and a second `office:meta` is not looked in.
    let mut meta = Flat::of(&d);
    meta.meta = r#"<x:other xmlns:x="urn:x"><meta:generator>Wrong/0</meta:generator></x:other><office:meta><meta:generator>Right/1</meta:generator><meta:generator>Second/2</meta:generator></office:meta><office:meta><meta:generator>Another/3</meta:generator></office:meta>"#.to_owned();
    assert_eq!(meta.read().1.generator.as_deref(), Some("Right/1"));
    // The scale of the first `office:settings` only; an unreadable scale is one warning naming the document's element.
    let mut bad = Flat::of(&d);
    bad.settings = r#"<office:settings><config:config-item-set config:name="ooo:configuration-settings"><config:config-item config:name="ScaleNumerator" config:type="int">1</config:config-item></config:config-item-set></office:settings>"#.to_owned();
    let report = bad.read().1;
    assert_eq!(report.warned("scale_unreadable"), 1);
    assert!(report.warnings.iter().any(|w| w.contains("office:settings")), "{:?}", report.warnings);
    // A package's own scale warning still names `settings.xml`.
    let mut package = d.clone();
    package.settings = Some(crate::drawing::settings(r#"<config:config-item config:name="ScaleNumerator" config:type="int">1</config:config-item>"#));
    assert!(package.read().1.warnings.iter().any(|w| w.contains("settings.xml")));
}

/// `office:meta` is `meta.xml`'s and `office:settings` is `settings.xml`'s, so **the model of a flat drawing holds them as the model of the package of the
/// same pieces does**: the title, the properties, the fields kept whole (the generator, the statistics) and the settings tree. A flat drawing kept none of it, so a package or a `.diacraft`
/// file made from the model lost its title, its properties, the generator of the program that made it and its settings; and an `office:meta` that has attributes of its own (as a `meta.xml`
/// of that shape is) is not read into fields, and is counted as kept in the file.
#[test]
fn odg_package_the_meta_and_the_settings_of_a_flat_drawing_are_held_as_a_packages_are() {
    let mut d = Drawing::a4().page("");
    d.meta = Some(crate::drawing::meta("G/1").replace(
        "<meta:generator>G/1</meta:generator>",
        concat!(
            "<meta:generator>G/1</meta:generator><dc:title>Plan</dc:title><meta:initial-creator>Someone</meta:initial-creator>",
            "<meta:creation-date>2030-01-02T03:04:05</meta:creation-date>",
            "<meta:user-defined meta:name=\"K\" meta:value-type=\"string\">V</meta:user-defined><meta:document-statistic meta:object-count=\"2\"/>"
        ),
    ));
    d.settings = Some(crate::drawing::settings(r#"<config:config-item config:name="ZoomFactor" config:type="short">100</config:config-item>"#));
    let (package_doc, _) = d.read();
    let (flat_doc, report) = Flat::of(&d).read();
    assert_eq!(&*flat_doc.title, "Plan");
    assert_eq!(flat_doc.props.created.as_deref(), Some("2030-01-02T03:04:05"));
    assert_eq!(flat_doc.props.custom.len(), 1, "the user-defined property");
    assert_eq!(flat_doc.title, package_doc.title);
    assert_eq!(flat_doc.props, package_doc.props, "the properties are the package's");
    let (flat_odf, package_odf) = (&flat_doc.ext.get().expect("the ODF data").odf, &package_doc.ext.get().expect("the ODF data").odf);
    assert_eq!(flat_odf.meta, package_odf.meta, "the generator and the statistics are kept whole, as a package's are");
    assert!(!flat_odf.meta.is_empty());
    let (flat_settings, package_settings) =
        (flat_odf.settings.as_ref().expect("the settings are held"), package_odf.settings.as_ref().expect("the settings are held"));
    assert!(flat_settings.name.ends_with("document-settings"), "the tree of a settings.xml: {}", flat_settings.name);
    assert_eq!(flat_settings.children, package_settings.children, "the `office:settings` element, whole");
    assert_eq!(report.kept.get("flat_meta"), None, "nothing of it is left in the file alone");
    // The settings of a flat document keep the namespaces its root declares, so that the tree is a part of its own.
    assert!(flat_settings.attrs.iter().any(|(k, _)| k == "xmlns:config"), "the `config` prefix the items use is declared on the tree");
    // An `office:meta` with an attribute of its own is not read into fields: the title is not held, and the report counts it.
    let mut odd = Flat::of(&d);
    odd.meta = odd.meta.replacen("<office:meta>", "<office:meta xml:id=\"m\">", 1);
    let (flat_doc, report) = odd.read();
    assert_eq!(&*flat_doc.title, "", "no title is read from it");
    assert_eq!(report.kept.get("flat_meta"), Some(&1), "{:?}", report.kept);
}

/// A picture read from `office:binary-data` is named by no entry of a package (`Media::part` is `office:binary-data`, which the writer does not take for the name of an
/// entry), and two pictures of one flat drawing are two pictures of the model, whatever their name.
#[test]
fn odg_package_an_inline_picture_is_named_by_no_entry() {
    let picture = |bytes: &[u8]| {
        let mut text = String::new();
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        for chunk in bytes.chunks(3) {
            let n = u32::from_be_bytes([0, chunk[0], chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)]);
            for k in 0..4usize {
                text.push(if k <= chunk.len() { char::from(alphabet[((n >> (18 - 6 * k)) & 63) as usize]) } else { '=' });
            }
        }
        format!(
            r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="2cm"><draw:image draw:mime-type="image/png"><office:binary-data>{text}</office:binary-data></draw:image></draw:frame>"#
        )
    };
    // Two pictures of different bytes (PNG signatures, which is all the reader judges a picture by).
    let (a, b) = ([0x89, b'P', b'N', b'G', 1, 2, 3, 4], [0x89, b'P', b'N', b'G', 5, 6, 7, 8]);
    let d = Drawing::a4().page(&format!("{}{}", picture(&a), picture(&b)));
    let (doc, _) = Flat::of(&d).read();
    let held: Vec<(String, Vec<u8>)> = page(&doc, 0)
        .shapes
        .iter()
        .filter_map(|s| s.foreign.as_ref().and_then(|f| f.picture.as_ref()).map(|m| (m.part.to_string(), m.bytes.to_vec())))
        .collect();
    assert_eq!(held, [("office:binary-data".to_owned(), a.to_vec()), ("office:binary-data".to_owned(), b.to_vec())]);
}

/// A document whose children are in the standard's order, `office:meta` among them, is not out of order; the first `office:meta` read is the first child.
#[test]
fn odg_package_a_regular_document_with_every_child_is_not_out_of_order() {
    let mut d = Drawing::a4().common(r#"<style:style style:name="Standard" style:family="graphic"/>"#).page("");
    d.meta = Some(crate::drawing::meta("G/1"));
    d.settings = Some(crate::drawing::settings(""));
    d.fonts = r#"<style:font-face style:name="F" svg:font-family="F"/>"#.to_owned();
    let mut flat = Flat::of(&d);
    flat.scripts = "<office:scripts/>".to_owned();
    let report = flat.read().1;
    assert_eq!(report.warned("flat_order_irregular"), 0, "{:?}", report.warnings);
    assert_eq!(report.kept("flat_child"), 0);
    // An office element that is none of the eight is counted, and is no disorder.
    flat.tail = "<office:annotation/>".to_owned();
    let report = flat.read().1;
    assert_eq!((report.kept("flat_child"), report.warned("flat_order_irregular")), (1, 0));
}
