//! What the tests of the package share: the packages of the spec's vectors, written by hand from the spec's text (section 8.2), a ZIP builder that can break any rule,
//! and the models the tests write (a drawing of pages that show pictures, and the like). **No file here is a file another program made**: every byte is built by this code.

use std::sync::Arc;

use diacraft_container::zip::{Archive, Method};
use diacraft_container::{Limits as ContainerLimits, Work};
use diacraft_doc::{Document, ForeignData, Master, Media, Page, RawAttrs, Shape, ShapeList, ShapeType};
use diacraft_native::{Limits, Opened, State, WriteOptions, read, write};

use crate::zipbuild::{Item, build};

pub const NS: &str = "urn:app.diacraft:extension";

/// The bytes of a part from the text a vector shows it as: the declaration line, a carriage return and a line feed, and then the element with every line break and the
/// spaces after it removed (the spec's 8.1).
pub fn part(text: &str) -> Vec<u8> {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n");
    for line in text.lines() {
        out.push_str(line.trim_start());
    }
    out.into_bytes()
}

/// The manifest of a package of `format` and `kind` listing `rows`, as `(path, role, extra attributes)`.
pub fn manifest(format: &str, kind: &str, rows: &[(&str, &str, &str)]) -> Vec<u8> {
    let mut text = format!("<diacraft:manifest xmlns:diacraft=\"{NS}\" format=\"{format}\" kind=\"{kind}\" written-by=\"DiaCraft 0.2.0\">\n");
    for (path, role, extra) in rows {
        let required = if matches!(*role, "document" | "page" | "master") { " required=\"1\"" } else { "" };
        text.push_str(&format!("<diacraft:part path=\"{path}\" role=\"{role}\"{required}{extra}/>\n"));
    }
    text.push_str("</diacraft:manifest>");
    part(&text)
}

pub const STYLE_BLOCK: &str = r#"
<diacraft:style name="No Style" name-u="No Style">
<diacraft:sheet>
<diacraft:Cell N="BottomMargin" V="0.05555556"/>
<diacraft:Cell N="DisplayMode" V="2"/>
<diacraft:Cell N="EnableFillProps" V="1"/>
<diacraft:Cell N="EnableLineProps" V="1"/>
<diacraft:Cell N="EnableTextProps" V="1"/>
<diacraft:Cell N="FillBkgnd" V="0"/>
<diacraft:Cell N="FillForegnd" V="1"/>
<diacraft:Cell N="FillPattern" V="1"/>
<diacraft:Cell N="IsSnapTarget" V="1"/>
<diacraft:Cell N="IsTextEditTarget" V="1"/>
<diacraft:Cell N="LeftMargin" V="0.05555556"/>
<diacraft:Cell N="LineColor" V="0"/>
<diacraft:Cell N="LinePattern" V="1"/>
<diacraft:Cell N="LineWeight" V="0.01"/>
<diacraft:Cell N="RightMargin" V="0.05555556"/>
<diacraft:Cell N="SelectMode" V="1"/>
<diacraft:Cell N="TopMargin" V="0.05555556"/>
<diacraft:Cell N="VerticalAlign" V="1"/>
<diacraft:Section N="Character">
<diacraft:Row IX="0">
<diacraft:Cell N="Color" V="0"/>
<diacraft:Cell N="Size" V="0.1666667"/>
</diacraft:Row>
</diacraft:Section>
</diacraft:sheet>
</diacraft:style>
"#;

/// Vector 1's document part.
pub fn document_1() -> Vec<u8> {
    part(&format!(
        "<diacraft:document xmlns:diacraft=\"{NS}\" title=\"Untitled\">\n{STYLE_BLOCK}<diacraft:page-ref id=\"1\" part=\"pages/page1.xml\"/>\n</diacraft:document>"
    ))
}

pub const PAGE_SHEET: &str = r#"
<diacraft:sheet>
<diacraft:Cell N="DrawingScale" V="1" U="IN"/>
<diacraft:Cell N="PageBottomMargin" V="0" U="IN"/>
<diacraft:Cell N="PageHeight" V="8.5" U="IN"/>
<diacraft:Cell N="PageLeftMargin" V="0" U="IN"/>
<diacraft:Cell N="PageRightMargin" V="0" U="IN"/>
<diacraft:Cell N="PageScale" V="1" U="IN"/>
<diacraft:Cell N="PageTopMargin" V="0" U="IN"/>
<diacraft:Cell N="PageWidth" V="11" U="IN"/>
</diacraft:sheet>
"#;

/// A page part of id 1 with `inner` after its sheet (what Vector 2 and the others hold).
pub fn page_1(inner: &str) -> Vec<u8> {
    page_with_start(&format!("<diacraft:page xmlns:diacraft=\"{NS}\" id=\"1\" name=\"Page-1\" name-u=\"Page-1\">"), inner)
}

pub fn page_with_start(start: &str, inner: &str) -> Vec<u8> {
    part(&format!("{start}\n{PAGE_SHEET}{inner}</diacraft:page>"))
}

/// The rectangle of Vector 2, with `after_sheet` between its sheet and its text and `text_extra` after its text.
pub fn rectangle(start_tag: &str, after_sheet: &str, text_extra: &str) -> String {
    format!(
        r#"{start_tag}
<diacraft:sheet>
<diacraft:Cell N="Height" V="1" U="IN"/>
<diacraft:Cell N="LocPinX" V="1" F="Width*0.5" U="IN"/>
<diacraft:Cell N="LocPinY" V="0.5" F="Height*0.5" U="IN"/>
<diacraft:Cell N="PinX" V="3" U="IN"/>
<diacraft:Cell N="PinY" V="2" U="IN"/>
<diacraft:Cell N="Width" V="2" U="IN"/>
<diacraft:Section N="Geometry" IX="0">
<diacraft:Row IX="1" T="MoveTo">
<diacraft:Cell N="X" V="0"/>
<diacraft:Cell N="Y" V="0"/>
</diacraft:Row>
<diacraft:Row IX="2" T="LineTo">
<diacraft:Cell N="X" V="2" F="Width"/>
<diacraft:Cell N="Y" V="0"/>
</diacraft:Row>
<diacraft:Row IX="3" T="LineTo">
<diacraft:Cell N="X" V="2" F="Width"/>
<diacraft:Cell N="Y" V="1" F="Height"/>
</diacraft:Row>
<diacraft:Row IX="4" T="LineTo">
<diacraft:Cell N="X" V="0"/>
<diacraft:Cell N="Y" V="1" F="Height"/>
</diacraft:Row>
<diacraft:Row IX="5" T="LineTo">
<diacraft:Cell N="X" V="0"/>
<diacraft:Cell N="Y" V="0"/>
</diacraft:Row>
</diacraft:Section>
</diacraft:sheet>
{after_sheet}<diacraft:text>Hello</diacraft:text>
{text_extra}</diacraft:shape>
"#
    )
}

pub const RECT_START: &str = "<diacraft:shape id=\"1\" name=\"Rectangle\" type=\"Shape\">";

/// Vector 2's page part.
pub fn page_2() -> Vec<u8> {
    page_1(&rectangle(RECT_START, "", ""))
}

pub fn state_empty() -> Vec<u8> {
    part(&format!("<diacraft:state xmlns:diacraft=\"{NS}\"/>"))
}

pub fn state_with(content_stamp: Option<&str>, ledger: &[(&str, &str)]) -> Vec<u8> {
    let stamp = content_stamp.map(|s| format!(" content-stamp=\"{s}\"")).unwrap_or_default();
    let mut text = format!("<diacraft:state xmlns:diacraft=\"{NS}\"{stamp}>\n");
    for (page, mark) in ledger {
        text.push_str(&format!("<diacraft:ledger page=\"{page}\" mark=\"{mark}\"/>\n"));
    }
    text.push_str("</diacraft:state>");
    part(&text)
}

pub const MEDIA_DRAWING: &str = "application/vnd.diacraft.drawing";

/// The entries of Vector 1, in the order the format writes them: mimetype stored, the rest deflated.
pub fn vector_1() -> Vec<Item> {
    vec![
        Item::stored("mimetype", MEDIA_DRAWING.as_bytes()),
        Item::deflated(
            "manifest.xml",
            &manifest("1.0", "drawing", &[("document.xml", "document", ""), ("pages/page1.xml", "page", ""), ("state.xml", "state", "")]),
        ),
        Item::deflated("document.xml", &document_1()),
        Item::deflated("pages/page1.xml", &page_1("")),
        Item::deflated("state.xml", &state_empty()),
    ]
}

/// Vector 2's entries: Vector 1's, with the rectangle on the page and the ledger.
pub fn vector_2() -> Vec<Item> {
    let mut items = vector_1();
    replace(&mut items, "pages/page1.xml", page_2());
    replace(&mut items, "state.xml", state_with(None, &[("1", "1")]));
    items
}

/// The 12 bytes of Vector 5's picture: a PNG signature and four more.
pub const PICTURE: [u8; 12] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x01, 0x02, 0x03];

fn picture_shape(id: u32, pin_x: u32) -> String {
    format!(
        r#"<diacraft:shape id="{id}" name="Picture {id}" type="Foreign">
<diacraft:sheet>
<diacraft:Cell N="Height" V="1" U="IN"/>
<diacraft:Cell N="PinX" V="{pin_x}" U="IN"/>
<diacraft:Cell N="PinY" V="2" U="IN"/>
<diacraft:Cell N="Width" V="1" U="IN"/>
</diacraft:sheet>
<diacraft:foreign>
<diacraft:attr name="ForeignType" value="Bitmap"/>
<diacraft:picture part="media/image1.png" type="image/png" blob="0"/>
</diacraft:foreign>
</diacraft:shape>
"#
    )
}

/// Vector 5's entries: a picture shown by two shapes, stored once.
pub fn vector_5() -> Vec<Item> {
    let mut items = vector_1();
    replace(
        &mut items,
        "manifest.xml",
        manifest(
            "1.0",
            "drawing",
            &[
                ("document.xml", "document", ""),
                ("pages/page1.xml", "page", ""),
                ("blobs/0.png", "blob", " media-type=\"image/png\""),
                ("state.xml", "state", ""),
            ],
        ),
    );
    replace(
        &mut items,
        "document.xml",
        part(&format!(
            "<diacraft:document xmlns:diacraft=\"{NS}\" title=\"Untitled\">\n{STYLE_BLOCK}<diacraft:page-ref id=\"1\" part=\"pages/page1.xml\"/>\n<diacraft:blob path=\"blobs/0.png\" media-type=\"image/png\" size=\"12\" stamp=\"71df7ba063b3b4da\"/>\n</diacraft:document>"
        )),
    );
    replace(&mut items, "pages/page1.xml", page_1(&format!("{}{}", picture_shape(1, 2), picture_shape(2, 5))));
    let at = items.iter().position(|i| i.name == b"state.xml").expect("a state entry");
    items.insert(at, Item::stored("blobs/0.png", &PICTURE));
    replace(&mut items, "state.xml", state_with(None, &[("1", "2")]));
    items
}

/// Replaces the content of the entry `name` (deflated), keeping its place.
pub fn replace(items: &mut [Item], name: &str, content: Vec<u8>) {
    let at = items.iter().position(|i| i.name == name.as_bytes()).expect("the entry to replace");
    items[at] = Item::deflated(name, &content);
}

/// Removes the entry `name`.
pub fn remove(items: &mut Vec<Item>, name: &str) {
    items.retain(|i| i.name != name.as_bytes());
}

/// The default limits of a read.
pub fn limits() -> Limits {
    Limits::default()
}

/// Reads and unwraps.
pub fn open_ok(bytes: &[u8]) -> (Document, Opened) {
    read(bytes, &limits()).unwrap_or_else(|e| panic!("the package reads: {e}"))
}

/// The error of a read, or a failure of the test when it read.
pub fn refused(bytes: &[u8]) -> diacraft_native::NativeError {
    match read(bytes, &limits()) {
        Ok(_) => panic!("the package was read and should have been refused"),
        Err(e) => e,
    }
}

/// Writes `doc` with an empty state and the default options.
pub fn write_doc(doc: &Document) -> Vec<u8> {
    write(doc, &State::default(), None, &WriteOptions::default()).unwrap_or_else(|e| panic!("the drawing is written: {e}")).bytes
}

/// The entries of a package, in the order of its directory: name and inflated bytes.
pub fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut archive = Archive::read(bytes, &ContainerLimits::default()).expect("the package is a ZIP archive");
    let mut work = Work::default();
    (0..archive.entries().len())
        .map(|i| {
            let name = archive.entries()[i].name.clone();
            (name, archive.inflate(i, &mut work).expect("an entry inflates"))
        })
        .collect()
}

/// The names of the entries, in the order of the directory.
pub fn names(bytes: &[u8]) -> Vec<String> {
    entries(bytes).into_iter().map(|(n, _)| n).collect()
}

/// The inflated bytes of the entry `name`.
pub fn entry(bytes: &[u8], name: &str) -> Vec<u8> {
    entries(bytes).into_iter().find(|(n, _)| n == name).map(|(_, b)| b).unwrap_or_else(|| panic!("the package has no entry {name}"))
}

/// The method of each entry, by name.
pub fn methods(bytes: &[u8]) -> Vec<(String, Option<Method>)> {
    let archive = Archive::read(bytes, &ContainerLimits::default()).expect("the package is a ZIP archive");
    archive.entries().iter().map(|e| (e.name.clone(), e.compression())).collect()
}

/// A picture of PNG bytes: a signature and `tag` repeated `len` times, in a fresh allocation.
pub fn png(tag: u8, len: usize) -> Arc<[u8]> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.extend(std::iter::repeat_n(tag, len));
    Arc::from(bytes)
}

/// A shape that shows `bytes`: a `Foreign` shape with a picture of part name `part`.
pub fn picture_shape_of(id: u32, part_name: &str, bytes: &Arc<[u8]>) -> Shape {
    let media = Media { part: Arc::from(part_name), content_type: Arc::from("image/png"), bytes: Arc::clone(bytes) };
    let foreign = ForeignData { attrs: RawAttrs::from_pairs([("ForeignType", "Bitmap")]), picture: Some(Arc::new(media)), ..ForeignData::default() };
    Shape {
        id,
        name: Some(Arc::from(format!("Picture {id}").as_str())),
        kind: ShapeType::Foreign,
        foreign: Some(Arc::new(foreign)),
        ..Shape::default()
    }
}

/// A page whose shapes are the pictures given, one for each, with ids 1, 2, ...
pub fn page_showing(id: u32, pictures: &[&Arc<[u8]>]) -> Page {
    let mut page = Page::new(id, &format!("Page-{id}"));
    page.shapes =
        ShapeList::from_vec(pictures.iter().enumerate().map(|(i, b)| Arc::new(picture_shape_of(i as u32 + 1, "media/image1.png", b))).collect());
    page
}

/// A drawing of the pages given.
pub fn drawing_of(pages: Vec<Page>) -> Document {
    let mut doc = Document::new_blank();
    doc.pages = diacraft_doc::SharedVec::from_vec(pages.into_iter().map(Arc::new).collect());
    doc
}

/// The bytes of the picture of the first shape of the page `at` of `doc`.
pub fn picture_of(doc: &Document, page: usize, shape: usize) -> Option<Arc<[u8]>> {
    let page = doc.pages.get(page)?;
    let shape = page.shapes.get(shape)?;
    shape.foreign.as_ref()?.picture.as_ref().map(|m| Arc::clone(&m.bytes))
}

/// Assembles the bytes of the entries as a package.
pub fn package(items: &[Item]) -> Vec<u8> {
    build(items)
}

/// A master of this ID and name, with nothing in it.
pub fn master(id: u32, name: &str) -> Master {
    Master { id, name: Some(Arc::from(name)), ..Master::default() }
}

/// The name of the first field of the documents in which they differ, and the index of the first page or master that differs, or `None` when they are equal. Short
/// by design: a failure prints this and not the documents.
pub fn first_difference(a: &Document, b: &Document) -> Option<String> {
    if a == b {
        return None;
    }
    let same = |name: &str, equal: bool| (!equal).then(|| name.to_owned());
    same("version", a.version == b.version)
        .or_else(|| same("title", a.title == b.title))
        .or_else(|| same("props", a.props == b.props))
        .or_else(|| {
            let (x, y): (Vec<_>, Vec<_>) = (a.pages.iter().collect(), b.pages.iter().collect());
            if x.len() != y.len() {
                return Some(format!("pages: {} against {}", x.len(), y.len()));
            }
            x.iter().zip(&y).position(|(p, q)| p != q).map(|i| format!("page {i}"))
        })
        .or_else(|| {
            let (x, y): (Vec<_>, Vec<_>) = (a.masters.iter().collect(), b.masters.iter().collect());
            if x.len() != y.len() {
                return Some(format!("masters: {} against {}", x.len(), y.len()));
            }
            x.iter().zip(&y).position(|(p, q)| p != q).map(|i| format!("master {i}"))
        })
        .or_else(|| same("styles", a.styles == b.styles))
        .or_else(|| same("themes", a.themes == b.themes))
        .or_else(|| same("colors", a.colors == b.colors))
        .or_else(|| same("fonts", a.fonts == b.fonts))
        .or_else(|| same("sheet", a.sheet == b.sheet))
        .or_else(|| same("passthrough", a.passthrough == b.passthrough))
        .or_else(|| same("diacraft", a.diacraft == b.diacraft))
        .or_else(|| same("ext", a.ext == b.ext))
        .or_else(|| same("parts", a.parts == b.parts))
        .or_else(|| same("vsdx", a.vsdx == b.vsdx))
        .or(Some("a field this walk does not look at".to_owned()))
}

/// A shape that shows `bytes` under the media type `content_type`.
pub fn typed_picture(id: u32, content_type: &str, bytes: &Arc<[u8]>) -> Shape {
    let mut shape = picture_shape_of(id, "media/x", bytes);
    if let Some(foreign) = &shape.foreign {
        let mut foreign = (**foreign).clone();
        if let Some(media) = &foreign.picture {
            foreign.picture = Some(Arc::new(Media { content_type: Arc::from(content_type), ..(**media).clone() }));
        }
        shape.foreign = Some(Arc::new(foreign));
    }
    shape
}

/// A page whose shapes are the pictures given, each under its own media type.
pub fn page_of_typed(id: u32, pictures: &[(&str, Arc<[u8]>)]) -> Page {
    let mut page = Page::new(id, &format!("Page-{id}"));
    page.shapes = ShapeList::from_vec(pictures.iter().enumerate().map(|(i, (t, b))| Arc::new(typed_picture(i as u32 + 1, t, b))).collect());
    page
}

/// The entries of the package `bytes`, as items of a new package (`mimetype` stored, the rest deflated), after `change` has seen each: `None` drops the entry, and
/// `Some(content)` keeps it with that content.
pub fn rebuilt(bytes: &[u8], mut change: impl FnMut(&str, Vec<u8>) -> Option<Vec<u8>>) -> Vec<Item> {
    entries(bytes)
        .into_iter()
        .filter_map(|(name, content)| {
            let content = change(&name, content)?;
            Some(if name == "mimetype" { Item::stored(&name, &content) } else { Item::deflated(&name, &content) })
        })
        .collect()
}

/// A small deterministic generator for the tests that need bytes and choices without a crate for it (splitmix64).
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// SHA-256 of `data` as lowercase hex (FIPS 180-4), written here so that the tests need no crate for the one pinned digest.
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be,
        0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa,
        0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85,
        0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f,
        0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in message.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

/// A package of `blobs` pictures of `size` bytes each, all but the first bytes made of one repeated block (so that they deflate far below the ratio limit), shown one on
/// each of the shapes of one page.
pub fn patterned(seed: u64, size: usize) -> Arc<[u8]> {
    let mut rng = Rng(seed);
    let block: Vec<u8> = (0..4096).map(|_| rng.next() as u8).collect();
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    while bytes.len() < size {
        bytes.extend_from_slice(&block);
    }
    bytes.truncate(size);
    Arc::from(bytes)
}
