//! Packages built in code: the package half of the document builder.
//!
//! [`build`](crate::build) makes the *model* a test needs. This makes the *file*: a ZIP archive that follows the Open Packaging
//! Conventions, with the parts, content types and relationships, written by hand from the specs, so a
//! reader can be tested from the bytes in and a hostile package can be made as easily as an honest one. Nothing here comes from a VSDX
//! file: the XML is the text a test writes (the spec vectors are XML fragments), and the builder only supplies what each vector leaves
//! out (the content types, the relationships, the document part, a root style).
//!
//! * [`ZipItem`] and [`zip`]: the archive. Honest stored and deflated entries, and an entry of `n` zero bytes that costs nothing to write
//!   (`zeros_under_the_ratio_cap`), for the bomb tests. The format is PKWARE's APPNOTE, which is public. The writer is the one the `opc` crate's own
//!   tests use, cut to what a reader above `opc` needs.
//! * [`Pkg`]: parts, content types and relationships, assembled into an archive with `[Content_Types].xml` first.
//! * [`VsdxBuilder`]: a VSDX package: a document part, pages, masters, themes and properties, each as XML text the test supplies or a
//!   small default, and every relationship the specs name between them.

// A fixture that cannot be built must fail its test, like an assertion; the shipped-code ban on panicking (AGENTS.md, Never crash) is for
// code that runs for users.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write;

/// `http://schemas.microsoft.com/visio/2010/relationships/`.
pub const VIS: &str = "http://schemas.microsoft.com/visio/2010/relationships/"; // brand-ok: a URI the format defines
/// `http://schemas.openxmlformats.org/officeDocument/2006/relationships/`.
pub const OD: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
/// `http://schemas.openxmlformats.org/package/2006/relationships/metadata/`.
pub const PK: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/";

/// The content type of a relationships part.
pub const CT_RELS: &str = "application/vnd.openxmlformats-package.relationships+xml";
/// The default content type of an `.xml` part.
pub const CT_XML: &str = "application/xml";
/// The content type of a drawing's document part.
pub const CT_DRAWING: &str = "application/vnd.ms-visio.drawing.main+xml"; // brand-ok: a content type the format defines
/// The content type of a page part.
pub const CT_PAGE: &str = "application/vnd.ms-visio.page+xml"; // brand-ok: a content type the format defines
/// The content type of the pages index.
pub const CT_PAGES: &str = "application/vnd.ms-visio.pages+xml"; // brand-ok: a content type the format defines
/// The content type of a master part.
pub const CT_MASTER: &str = "application/vnd.ms-visio.master+xml"; // brand-ok: a content type the format defines
/// The content type of the masters index.
pub const CT_MASTERS: &str = "application/vnd.ms-visio.masters+xml"; // brand-ok: a content type the format defines
/// The content type of a theme part.
pub const CT_THEME: &str = "application/vnd.openxmlformats-officedocument.theme+xml";
/// The content type of `docProps/core.xml`.
pub const CT_CORE: &str = "application/vnd.openxmlformats-package.core-properties+xml";
/// The content type of `docProps/app.xml`.
pub const CT_APP: &str = "application/vnd.openxmlformats-officedocument.extended-properties+xml";
/// The content type of the solutions index.
pub const CT_SOLUTIONS: &str = "application/vnd.ms-visio.solutions+xml"; // brand-ok: a content type the format defines
/// The content type of a solution part.
pub const CT_SOLUTION: &str = "application/vnd.ms-visio.solution+xml"; // brand-ok: a content type the format defines

/// The namespace every sample uses.
pub const NS_2012: &str = "http://schemas.microsoft.com/office/visio/2012/main"; // brand-ok: a namespace the format defines
/// The namespace Microsoft's documents state.
pub const NS_2011: &str = "http://schemas.microsoft.com/office/visio/2011/1/core"; // brand-ok: a namespace the format defines
/// The folder of a package's VSDX parts, with and without the slash that a part name starts with. Code that is not a test's (a fuzz
/// crate that includes this file) names a part in it as `format!("{DIR}/...")`: `cargo xtask brands` reads the string literals of such code.
pub const DIR: &str = "/visio"; // brand-ok: a folder the format defines
const DIR_REL: &str = "visio"; // brand-ok: a folder the format defines

/// The relationships namespace, which `r:id` is in.
pub const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

// ------------------------------------------------------------------------------------------------------------------ the archive

/// One entry of an archive: its name and its data as they sit in the file, with the CRC-32 and size the headers state.
#[derive(Clone, Debug)]
pub struct ZipItem {
    name: Vec<u8>,
    method: u16,
    data: Vec<u8>,
    crc: u32,
    size: u64,
}

impl ZipItem {
    /// An honest stored entry.
    pub fn stored(name: &str, content: &[u8]) -> ZipItem {
        ZipItem { name: name.as_bytes().to_vec(), method: 0, data: content.to_vec(), crc: crc32fast::hash(content), size: content.len() as u64 }
    }

    /// An honest deflated entry.
    pub fn deflated(name: &str, content: &[u8]) -> ZipItem {
        let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(content).expect("deflate into memory");
        let data = enc.finish().expect("deflate into memory");
        ZipItem { name: name.as_bytes().to_vec(), method: 8, data, crc: crc32fast::hash(content), size: content.len() as u64 }
    }

    /// An honest deflated entry of `n` zero bytes (at least 1), a ratio of about 518, under the reader's cap of 1,000: the entry reads
    /// as a part of `n` bytes and costs the writer almost nothing, so a test can ask for a part as large as a limit allows.
    pub fn zeros_under_the_ratio_cap(name: &str, n: u64) -> ZipItem {
        ZipItem { name: name.as_bytes().to_vec(), method: 8, data: deflate_zeros(n), crc: crc32_of_zeros(n), size: n }
    }
}

fn u16le(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn u32le(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// An archive of `items` in order, closed by plain end records. Sizes up to 4 GiB: no ZIP64.
pub fn zip(items: &[ZipItem]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut offsets = Vec::with_capacity(items.len());
    for it in items {
        offsets.push(u32::try_from(out.len()).expect("a test archive stays under 4 GiB"));
        u32le(&mut out, 0x0403_4B50);
        u16le(&mut out, 20);
        u16le(&mut out, 0);
        u16le(&mut out, it.method);
        u16le(&mut out, 0);
        u16le(&mut out, 0x21); // date: 1980-01-01
        u32le(&mut out, it.crc);
        u32le(&mut out, u32::try_from(it.data.len()).expect("a test entry stays under 4 GiB"));
        u32le(&mut out, u32::try_from(it.size).expect("a test entry stays under 4 GiB"));
        u16le(&mut out, u16::try_from(it.name.len()).expect("a test name is short"));
        u16le(&mut out, 0);
        out.extend_from_slice(&it.name);
        out.extend_from_slice(&it.data);
    }
    let cd_start = u32::try_from(out.len()).expect("a test archive stays under 4 GiB");
    for (it, offset) in items.iter().zip(&offsets) {
        u32le(&mut out, 0x0201_4B50);
        u16le(&mut out, 45);
        u16le(&mut out, 20);
        u16le(&mut out, 0);
        u16le(&mut out, it.method);
        u16le(&mut out, 0);
        u16le(&mut out, 0x21);
        u32le(&mut out, it.crc);
        u32le(&mut out, u32::try_from(it.data.len()).expect("a test entry stays under 4 GiB"));
        u32le(&mut out, u32::try_from(it.size).expect("a test entry stays under 4 GiB"));
        u16le(&mut out, u16::try_from(it.name.len()).expect("a test name is short"));
        u16le(&mut out, 0);
        u16le(&mut out, 0);
        u16le(&mut out, 0);
        u16le(&mut out, 0);
        u32le(&mut out, 0);
        u32le(&mut out, *offset);
        out.extend_from_slice(&it.name);
    }
    let cd_len = u32::try_from(out.len()).expect("a test archive stays under 4 GiB") - cd_start;
    let count = u16::try_from(items.len()).expect("a test archive has under 65,536 entries");
    u32le(&mut out, 0x0605_4B50);
    u16le(&mut out, 0);
    u16le(&mut out, 0);
    u16le(&mut out, count);
    u16le(&mut out, count);
    u32le(&mut out, cd_len);
    u32le(&mut out, cd_start);
    u16le(&mut out, 0);
    out
}

/// The CRC-32 of `n` zero bytes.
fn crc32_of_zeros(n: u64) -> u32 {
    let zeros = [0u8; 65536];
    let mut h = crc32fast::Hasher::new();
    let mut left = n;
    while left > 0 {
        let k = usize::try_from(left.min(zeros.len() as u64)).expect("at most 65,536");
        h.update(&zeros[..k]);
        left -= k as u64;
    }
    h.finalize()
}

/// Bits into bytes, least significant bit first, as deflate wants them.
struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn put(&mut self, value: u32, nbits: u32) {
        self.acc |= u64::from(value) << self.n;
        self.n += nbits;
        while self.n >= 8 {
            self.out.push((self.acc & 0xFF) as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    /// A Huffman code: its bits go in most significant first.
    fn code(&mut self, code: u32, nbits: u32) {
        let mut rev = 0;
        for i in 0..nbits {
            rev |= ((code >> i) & 1) << (nbits - 1 - i);
        }
        self.put(rev, nbits);
    }

    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push((self.acc & 0xFF) as u8);
        }
        self.out
    }
}

/// A raw deflate stream that inflates to `n` zero bytes (at least 1), with a literal zero after each run of 258: 259 bytes for 4 bits,
/// a ratio of about 518. One final block with its own Huffman codes (literal 0 and end of block two bits, the length-258 symbol one bit,
/// one distance code of one bit), sent through a code-length code of three symbols.
fn deflate_zeros(n: u64) -> Vec<u8> {
    assert!(n >= 1, "a stream of zero bytes needs at least one");
    let mut b = Bits { out: Vec::new(), acc: 0, n: 0 };
    b.put(1, 1); // final block
    b.put(2, 2); // dynamic Huffman codes
    b.put(29, 5); // 286 literal/length code lengths
    b.put(0, 5); // 1 distance code length
    b.put(14, 4); // 18 code-length code lengths, in the order 16 17 18 0 8 7 9 6 10 5 11 4 12 3 13 2 14 1
    for len in [0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 2] {
        b.put(len, 3);
    }
    b.code(0b11, 2); // length 2: literal 0
    b.code(0, 1); // 18: a run of zeros
    b.put(138 - 11, 7);
    b.code(0, 1);
    b.put(117 - 11, 7); // symbols 1 to 255
    b.code(0b11, 2); // length 2: end of block
    b.code(0, 1);
    b.put(28 - 11, 7); // symbols 257 to 284
    b.code(0b10, 2); // length 1: symbol 285
    b.code(0b10, 2); // length 1: the distance code
    b.code(0b10, 2); // literal 0
    let mut left = n - 1;
    while left >= 258 {
        b.code(0, 1); // length 258
        b.code(0, 1); // distance 1
        left -= 258;
        if left >= 1 {
            b.code(0b10, 2);
            left -= 1;
        }
    }
    for _ in 0..left {
        b.code(0b10, 2);
    }
    b.code(0b11, 2); // end of block
    b.finish()
}

// ------------------------------------------------------------------------------------------------------------------ the package

/// An escaped attribute value for text a test writes into XML.
pub fn esc(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[derive(Clone, Debug)]
struct PartSpec {
    /// A row of the content types for a part that is not in the archive: no entry is written for it.
    absent: bool,
    name: String,
    /// The `Override` content type, or none to rely on a `Default`.
    content_type: Option<String>,
    bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
struct RelSpec {
    source: String,
    id: String,
    ty: String,
    target: String,
    external: bool,
}

/// A package: parts, the content types of them, and relationships, to be written as an archive with `[Content_Types].xml` first.
#[derive(Clone, Debug)]
pub struct Pkg {
    parts: Vec<PartSpec>,
    defaults: Vec<(String, String)>,
    rels: Vec<RelSpec>,
    raw_items: Vec<ZipItem>,
    stored: bool,
}

impl Default for Pkg {
    fn default() -> Self {
        Pkg::new()
    }
}

impl Pkg {
    /// A package with the two `Default` rows every VSDX file has, `rels` and `xml`, and nothing else.
    pub fn new() -> Pkg {
        Pkg {
            parts: Vec::new(),
            defaults: vec![("rels".into(), CT_RELS.into()), ("xml".into(), CT_XML.into())],
            rels: Vec::new(),
            raw_items: Vec::new(),
            stored: false,
        }
    }

    /// A part, with an `Override` row for its content type.
    pub fn part(mut self, name: &str, content_type: &str, bytes: impl AsRef<[u8]>) -> Pkg {
        self.parts.push(PartSpec {
            absent: false,
            name: name.to_owned(),
            content_type: Some(content_type.to_owned()),
            bytes: bytes.as_ref().to_vec(),
        });
        self
    }

    /// An `Override` row for a part that the package does not have: no entry of the archive, only the row (a package of another writer has rows like it).
    pub fn override_without_part(mut self, name: &str, content_type: &str) -> Pkg {
        self.parts.push(PartSpec { absent: true, name: name.to_owned(), content_type: Some(content_type.to_owned()), bytes: Vec::new() });
        self
    }

    /// A part with no `Override`: its type comes from a `Default` row, or is none.
    pub fn part_without_override(mut self, name: &str, bytes: impl AsRef<[u8]>) -> Pkg {
        self.parts.push(PartSpec { absent: false, name: name.to_owned(), content_type: None, bytes: bytes.as_ref().to_vec() });
        self
    }

    /// A `Default` content type row.
    pub fn default_type(mut self, extension: &str, content_type: &str) -> Pkg {
        self.defaults.push((extension.to_owned(), content_type.to_owned()));
        self
    }

    /// An internal relationship from `source` (`/` for the package, else a part name with its slash).
    pub fn rel(mut self, source: &str, id: &str, ty: &str, target: &str) -> Pkg {
        self.rels.push(RelSpec { source: source.to_owned(), id: id.to_owned(), ty: ty.to_owned(), target: target.to_owned(), external: false });
        self
    }

    /// The package without the relationships of type `ty` from `source`: a part a file lists in its content types and no relationship names.
    pub fn without_rels_of_type(mut self, source: &str, ty: &str) -> Pkg {
        self.rels.retain(|r| !(r.source == source && r.ty == ty));
        self
    }

    /// An external relationship (a URL), which is kept and never followed.
    pub fn rel_external(mut self, source: &str, id: &str, ty: &str, url: &str) -> Pkg {
        self.rels.push(RelSpec { source: source.to_owned(), id: id.to_owned(), ty: ty.to_owned(), target: url.to_owned(), external: true });
        self
    }

    /// An archive entry written as it is, with no row in the content types and no relationship: a folder item, a stray file, or whatever a
    /// hostile test needs. It comes after the parts.
    pub fn item(mut self, item: ZipItem) -> Pkg {
        self.raw_items.push(item);
        self
    }

    /// Changes the bytes of every part (not the relationships parts, which are made from the relationships) with `f`, which is given the part's
    /// name: for a test that mutates the XML a reader will meet and still wants a package whose checksums are right.
    pub fn map_parts(mut self, mut f: impl FnMut(&str, &mut Vec<u8>)) -> Pkg {
        for part in &mut self.parts {
            f(&part.name, &mut part.bytes);
        }
        self
    }

    /// The names of the parts, in order.
    pub fn part_names(&self) -> Vec<String> {
        self.parts.iter().filter(|p| !p.absent).map(|p| p.name.clone()).collect()
    }

    /// Store every entry instead of deflating it.
    pub fn stored(mut self) -> Pkg {
        self.stored = true;
        self
    }

    /// The `[Content_Types].xml` text.
    pub fn content_types_xml(&self) -> String {
        let mut s = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
        );
        for (ext, ct) in &self.defaults {
            s.push_str(&format!("<Default Extension=\"{}\" ContentType=\"{}\"/>", esc(ext), esc(ct)));
        }
        for part in &self.parts {
            if let Some(ct) = &part.content_type {
                s.push_str(&format!("<Override PartName=\"{}\" ContentType=\"{}\"/>", esc(&part.name), esc(ct)));
            }
        }
        s.push_str("</Types>");
        s
    }

    /// The relationships part of `source`, as text, or `None` when it has no relationship.
    pub fn rels_xml(&self, source: &str) -> Option<String> {
        let rows: Vec<&RelSpec> = self.rels.iter().filter(|r| r.source == source).collect();
        if rows.is_empty() {
            return None;
        }
        let mut s = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
        );
        for r in rows {
            let mode = if r.external { " TargetMode=\"External\"" } else { "" };
            s.push_str(&format!("<Relationship Id=\"{}\" Type=\"{}\" Target=\"{}\"{mode}/>", esc(&r.id), esc(&r.ty), esc(&r.target)));
        }
        s.push_str("</Relationships>");
        Some(s)
    }

    /// The name of the relationships part of `source`.
    fn rels_name(source: &str) -> String {
        let source = source.trim_start_matches('/');
        match source.rsplit_once('/') {
            _ if source.is_empty() => "_rels/.rels".to_owned(),
            Some((folder, file)) => format!("{folder}/_rels/{file}.rels"),
            None => format!("_rels/{source}.rels"),
        }
    }

    /// The entries of the archive, in the order of the reference app's own saves: the content types stream, then the package relationships, then the
    /// other relationships parts, the parts, and any raw items.
    pub fn items(&self) -> Vec<ZipItem> {
        let make = |name: &str, bytes: &[u8]| if self.stored { ZipItem::stored(name, bytes) } else { ZipItem::deflated(name, bytes) };
        let mut items = vec![make("[Content_Types].xml", self.content_types_xml().as_bytes())];
        let mut sources: Vec<String> = Vec::new();
        for r in &self.rels {
            if !sources.contains(&r.source) {
                sources.push(r.source.clone());
            }
        }
        sources.sort_by_key(|s| s != "/");
        for source in &sources {
            if let Some(xml) = self.rels_xml(source) {
                items.push(make(&Pkg::rels_name(source), xml.as_bytes()));
            }
        }
        for part in self.parts.iter().filter(|p| !p.absent) {
            items.push(make(part.name.trim_start_matches('/'), &part.bytes));
        }
        items.extend(self.raw_items.iter().cloned());
        items
    }

    /// The archive.
    pub fn build(&self) -> Vec<u8> {
        zip(&self.items())
    }
}

// --------------------------------------------------------------------------------------------------------------- a VSDX package

/// Which namespace the VSDX parts are written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Namespace {
    /// `2012/main`, the one every sample uses.
    Main2012,
    /// `2011/1/core`, the one Microsoft's documents state.
    Core2011,
    /// No namespace at all.
    None,
    /// This one, whatever it is.
    Other(&'static str),
}

/// One page of a package: a row of the pages index, and its part.
#[derive(Clone, Debug)]
pub struct PageSpec {
    pub id: String,
    pub name: String,
    /// More attributes of the `Page` row, as written: ` Background="1" BackPage="3"`.
    pub attrs: String,
    /// The text of the `PageSheet` element, or none for no `PageSheet`.
    pub sheet: Option<String>,
    /// What goes inside `Shapes`, as text.
    pub shapes: String,
    /// What goes inside `Connects`, as text.
    pub connects: String,
    /// A page row with no `Rel` and no part.
    pub no_part: bool,
    /// The whole page part, in place of the one built from `shapes`.
    pub part_xml: Option<String>,
    /// More relationships of the page part: `(Id, Type, Target)`.
    pub rels: Vec<(String, String, String)>,
}

impl PageSpec {
    /// A page with this `ID` and name, and no shapes.
    pub fn new(id: u32, name: &str) -> PageSpec {
        PageSpec {
            id: id.to_string(),
            name: name.to_owned(),
            attrs: String::new(),
            sheet: Some(String::new()),
            shapes: String::new(),
            connects: String::new(),
            no_part: false,
            part_xml: None,
            rels: Vec::new(),
        }
    }

    /// The shapes, as the text inside `Shapes`.
    pub fn shapes(mut self, xml: &str) -> PageSpec {
        self.shapes = xml.to_owned();
        self
    }

    /// The `Connect` rows, as the text inside `Connects`.
    pub fn connects(mut self, xml: &str) -> PageSpec {
        self.connects = xml.to_owned();
        self
    }

    /// More attributes on the `Page` row.
    pub fn attrs(mut self, attrs: &str) -> PageSpec {
        self.attrs = attrs.to_owned();
        self
    }

    /// The text inside the `PageSheet`.
    pub fn sheet(mut self, xml: &str) -> PageSpec {
        self.sheet = Some(xml.to_owned());
        self
    }

    /// A page row with no part.
    pub fn without_part(mut self) -> PageSpec {
        self.no_part = true;
        self
    }

    /// The whole page part.
    pub fn part(mut self, xml: &str) -> PageSpec {
        self.part_xml = Some(xml.to_owned());
        self
    }

    /// A relationship of the page part (a picture, say).
    pub fn rel(mut self, id: &str, ty: &str, target: &str) -> PageSpec {
        self.rels.push((id.to_owned(), ty.to_owned(), target.to_owned()));
        self
    }
}

/// One master of a package: a row of the masters index, and its part.
#[derive(Clone, Debug)]
pub struct MasterSpec {
    pub id: String,
    pub name: String,
    pub attrs: String,
    pub sheet: Option<String>,
    pub shapes: String,
    pub connects: String,
    pub no_part: bool,
    pub part_xml: Option<String>,
    pub icon: Option<String>,
    pub rels: Vec<(String, String, String)>,
}

impl MasterSpec {
    /// A master with this `ID` and name, and no shapes.
    pub fn new(id: u32, name: &str) -> MasterSpec {
        MasterSpec {
            id: id.to_string(),
            name: name.to_owned(),
            attrs: String::new(),
            sheet: Some(String::new()),
            shapes: String::new(),
            connects: String::new(),
            no_part: false,
            part_xml: None,
            icon: None,
            rels: Vec::new(),
        }
    }

    /// The shapes of the master's part.
    pub fn shapes(mut self, xml: &str) -> MasterSpec {
        self.shapes = xml.to_owned();
        self
    }

    /// More attributes on the `Master` row.
    pub fn attrs(mut self, attrs: &str) -> MasterSpec {
        self.attrs = attrs.to_owned();
        self
    }

    /// A master row with no part.
    pub fn without_part(mut self) -> MasterSpec {
        self.no_part = true;
        self
    }

    /// The whole master part.
    pub fn part(mut self, xml: &str) -> MasterSpec {
        self.part_xml = Some(xml.to_owned());
        self
    }

    /// The `Icon` text.
    pub fn icon(mut self, text: &str) -> MasterSpec {
        self.icon = Some(text.to_owned());
        self
    }

    /// A relationship of the master part.
    pub fn rel(mut self, id: &str, ty: &str, target: &str) -> MasterSpec {
        self.rels.push((id.to_owned(), ty.to_owned(), target.to_owned()));
        self
    }
}

/// A VSDX package: the document part with the styles and tables a test gives it, the pages and masters with their parts, themes and the
/// property parts, and every relationship between them that lists.
#[derive(Clone, Debug)]
pub struct VsdxBuilder {
    ns: Namespace,
    document_type: String,
    colors: Option<String>,
    face_names: String,
    styles: String,
    document_sheet: String,
    document_extra: String,
    pages: Vec<PageSpec>,
    masters: Vec<MasterSpec>,
    themes: Vec<Vec<u8>>,
    core: Option<String>,
    app: Option<String>,
    solution: Option<(String, String)>,
    pages_part: bool,
    masters_part: bool,
    pkg_hook: Vec<Pkg>,
}

/// The root style, `No Style`: the three flags and nothing else.
pub const ROOT_STYLE: &str = r#"<StyleSheet ID="0" NameU="No Style" Name="No Style"><Cell N="EnableFillProps" V="1"/><Cell N="EnableLineProps" V="1"/><Cell N="EnableTextProps" V="1"/></StyleSheet>"#;

impl Default for VsdxBuilder {
    fn default() -> Self {
        VsdxBuilder::new()
    }
}

impl VsdxBuilder {
    /// A drawing in `2012/main` with the root style, an empty document sheet and no page.
    pub fn new() -> VsdxBuilder {
        VsdxBuilder {
            ns: Namespace::Main2012,
            document_type: CT_DRAWING.to_owned(),
            colors: None,
            face_names: String::new(),
            styles: ROOT_STYLE.to_owned(),
            document_sheet: String::new(),
            document_extra: String::new(),
            pages: Vec::new(),
            masters: Vec::new(),
            themes: Vec::new(),
            core: None,
            app: None,
            solution: None,
            pages_part: true,
            masters_part: true,
            pkg_hook: Vec::new(),
        }
    }

    /// The namespace of every VSDX part.
    pub fn namespace(mut self, ns: Namespace) -> VsdxBuilder {
        self.ns = ns;
        self
    }

    /// The content type of the document part: one of the six of R17, or any.
    pub fn document_type(mut self, content_type: &str) -> VsdxBuilder {
        self.document_type = content_type.to_owned();
        self
    }

    /// The text inside `Colors`; without it there is no `Colors` element.
    pub fn colors(mut self, xml: &str) -> VsdxBuilder {
        self.colors = Some(xml.to_owned());
        self
    }

    /// The text inside `FaceNames`.
    pub fn face_names(mut self, xml: &str) -> VsdxBuilder {
        self.face_names = xml.to_owned();
        self
    }

    /// The text inside `StyleSheets`: the root style `ID` 0 is there unless this replaces it.
    pub fn styles(mut self, xml: &str) -> VsdxBuilder {
        self.styles = xml.to_owned();
        self
    }

    /// The text inside `StyleSheets`, after the root style.
    pub fn more_styles(mut self, xml: &str) -> VsdxBuilder {
        self.styles.push_str(xml);
        self
    }

    /// The text inside the `DocumentSheet`.
    pub fn document_sheet(mut self, xml: &str) -> VsdxBuilder {
        self.document_sheet = xml.to_owned();
        self
    }

    /// More children of `VisioDocument`, after `DocumentSheet`: `DocumentSettings`, `EventList`, a foreign element.
    pub fn document_extra(mut self, xml: &str) -> VsdxBuilder {
        self.document_extra.push_str(xml);
        self
    }

    /// A page, after the others.
    pub fn page(mut self, page: PageSpec) -> VsdxBuilder {
        self.pages.push(page);
        self
    }

    /// A master, after the others.
    pub fn master(mut self, master: MasterSpec) -> VsdxBuilder {
        self.masters.push(master);
        self
    }

    /// A theme part, with these bytes.
    pub fn theme(mut self, bytes: &[u8]) -> VsdxBuilder {
        self.themes.push(bytes.to_vec());
        self
    }

    /// The whole `docProps/core.xml`.
    pub fn core(mut self, xml: &str) -> VsdxBuilder {
        self.core = Some(xml.to_owned());
        self
    }

    /// The whole `docProps/app.xml`.
    pub fn app(mut self, xml: &str) -> VsdxBuilder {
        self.app = Some(xml.to_owned());
        self
    }

    /// A solution of this name, whose part is `xml`.
    pub fn solution(mut self, name: &str, xml: &str) -> VsdxBuilder {
        self.solution = Some((name.to_owned(), xml.to_owned()));
        self
    }

    /// No masters part at all, so no masters index.
    pub fn without_masters_part(mut self) -> VsdxBuilder {
        self.masters_part = false;
        self
    }

    /// No pages index.
    pub fn without_pages_part(mut self) -> VsdxBuilder {
        self.pages_part = false;
        self
    }

    /// The package, to add to before it is written: other parts, relationships, hostile items.
    pub fn pkg(&self) -> Pkg {
        let mut pkg = Pkg::new();
        let document = format!("{DIR}/document.xml");
        let document = document.as_str();
        pkg = pkg
            .part(document, &self.document_type, self.document_xml())
            .rel("/", "rId1", &format!("{VIS}document"), &format!("{DIR_REL}/document.xml"))
            .part("/docProps/core.xml", CT_CORE, self.core.clone().unwrap_or_else(default_core))
            .rel("/", "rId2", &format!("{PK}core-properties"), "docProps/core.xml")
            .part("/docProps/app.xml", CT_APP, self.app.clone().unwrap_or_else(default_app))
            .rel("/", "rId3", &format!("{OD}extended-properties"), "docProps/app.xml");
        let mut doc_rel = 0;
        let mut next = |_: &str| {
            doc_rel += 1;
            format!("rId{doc_rel}")
        };
        if self.pages_part {
            pkg = pkg.part(&format!("{DIR}/pages/pages.xml"), CT_PAGES, self.pages_xml()).rel(
                document,
                &next("pages"),
                &format!("{VIS}pages"),
                "pages/pages.xml",
            );
            for (i, page) in self.pages.iter().enumerate() {
                if page.no_part {
                    continue;
                }
                let name = format!("{DIR}/pages/page{}.xml", i + 1);
                pkg = pkg
                    .part(&name, CT_PAGE, page.part_xml.clone().unwrap_or_else(|| self.contents_xml("PageContents", &page.shapes, &page.connects)))
                    .rel(&format!("{DIR}/pages/pages.xml"), &format!("rId{}", i + 1), &format!("{VIS}page"), &format!("page{}.xml", i + 1));
                for (id, ty, target) in &page.rels {
                    pkg = pkg.rel(&name, id, ty, target);
                }
            }
        }
        if self.masters_part {
            pkg = pkg.part(&format!("{DIR}/masters/masters.xml"), CT_MASTERS, self.masters_xml()).rel(
                document,
                &next("masters"),
                &format!("{VIS}masters"),
                "masters/masters.xml",
            );
            for (i, master) in self.masters.iter().enumerate() {
                if master.no_part {
                    continue;
                }
                let name = format!("{DIR}/masters/master{}.xml", i + 1);
                pkg = pkg
                    .part(
                        &name,
                        CT_MASTER,
                        master.part_xml.clone().unwrap_or_else(|| self.contents_xml("MasterContents", &master.shapes, &master.connects)),
                    )
                    .rel(&format!("{DIR}/masters/masters.xml"), &format!("rId{}", i + 1), &format!("{VIS}master"), &format!("master{}.xml", i + 1));
                for (id, ty, target) in &master.rels {
                    pkg = pkg.rel(&name, id, ty, target);
                }
            }
        }
        for (i, bytes) in self.themes.iter().enumerate() {
            let name = format!("{DIR}/theme/theme{}.xml", i + 1);
            pkg = pkg.part(&name, CT_THEME, bytes).rel(document, &next("theme"), &format!("{OD}theme"), &format!("theme/theme{}.xml", i + 1));
        }
        if let Some((name, xml)) = &self.solution {
            let index = format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?><Solutions xmlns=\"{}\" xmlns:r=\"{NS_R}\"><Solution Name=\"{}\"><Rel r:id=\"rId1\"/></Solution></Solutions>",
                self.ns_uri(),
                esc(name)
            );
            pkg = pkg
                .part(&format!("{DIR}/solutions/solutions.xml"), CT_SOLUTIONS, index)
                .rel(document, &next("solutions"), &format!("{VIS}solutions"), "solutions/solutions.xml")
                .part(&format!("{DIR}/solutions/solution1.xml"), CT_SOLUTION, xml)
                .rel(&format!("{DIR}/solutions/solutions.xml"), "rId1", &format!("{VIS}solution"), "solution1.xml");
        }
        for extra in &self.pkg_hook {
            pkg = merge(pkg, extra);
        }
        pkg
    }

    /// The archive.
    pub fn build(&self) -> Vec<u8> {
        self.pkg().build()
    }

    fn ns_uri(&self) -> &str {
        match self.ns {
            Namespace::Main2012 => NS_2012,
            Namespace::Core2011 => NS_2011,
            Namespace::None => "",
            Namespace::Other(uri) => uri,
        }
    }

    /// The opening of a VSDX root element: `<Name xmlns=".." xmlns:r=".." xml:space="preserve">`.
    fn root_open(&self, name: &str) -> String {
        let xmlns = if matches!(self.ns, Namespace::None) { String::new() } else { format!(" xmlns=\"{}\"", self.ns_uri()) };
        format!("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<{name}{xmlns} xmlns:r=\"{NS_R}\" xml:space=\"preserve\">")
    }

    fn document_xml(&self) -> String {
        let mut s = self.root_open("VisioDocument");
        if let Some(colors) = &self.colors {
            s.push_str(&format!("<Colors>{colors}</Colors>"));
        }
        s.push_str(&format!(
            "<FaceNames>{}</FaceNames><StyleSheets>{}</StyleSheets><DocumentSheet>{}</DocumentSheet>",
            self.face_names, self.styles, self.document_sheet
        ));
        s.push_str(&self.document_extra);
        s.push_str("</VisioDocument>");
        s
    }

    fn pages_xml(&self) -> String {
        let mut s = self.root_open("Pages");
        for (i, page) in self.pages.iter().enumerate() {
            s.push_str(&format!("<Page ID=\"{}\" Name=\"{}\" NameU=\"{}\"{}>", page.id, esc(&page.name), esc(&page.name), page.attrs));
            if let Some(sheet) = &page.sheet {
                s.push_str(&format!("<PageSheet>{sheet}</PageSheet>"));
            }
            if !page.no_part {
                s.push_str(&format!("<Rel r:id=\"rId{}\"/>", i + 1));
            }
            s.push_str("</Page>");
        }
        s.push_str("</Pages>");
        s
    }

    fn masters_xml(&self) -> String {
        let mut s = self.root_open("Masters");
        for (i, master) in self.masters.iter().enumerate() {
            s.push_str(&format!("<Master ID=\"{}\" Name=\"{}\" NameU=\"{}\"{}>", master.id, esc(&master.name), esc(&master.name), master.attrs));
            if let Some(sheet) = &master.sheet {
                s.push_str(&format!("<PageSheet>{sheet}</PageSheet>"));
            }
            if let Some(icon) = &master.icon {
                s.push_str(&format!("<Icon>{icon}</Icon>"));
            }
            if !master.no_part {
                s.push_str(&format!("<Rel r:id=\"rId{}\"/>", i + 1));
            }
            s.push_str("</Master>");
        }
        s.push_str("</Masters>");
        s
    }

    fn contents_xml(&self, root: &str, shapes: &str, connects: &str) -> String {
        let mut s = self.root_open(root);
        if !shapes.is_empty() {
            s.push_str(&format!("<Shapes>{shapes}</Shapes>"));
        }
        if !connects.is_empty() {
            s.push_str(&format!("<Connects>{connects}</Connects>"));
        }
        s.push_str(&format!("</{root}>"));
        s
    }

    /// Adds `extra` (its parts, relationships, defaults and raw items) to the package this builds.
    pub fn with(mut self, extra: Pkg) -> VsdxBuilder {
        self.pkg_hook.push(extra);
        self
    }
}

/// `into` with everything `extra` holds added after it.
fn merge(mut into: Pkg, extra: &Pkg) -> Pkg {
    into.parts.extend(extra.parts.iter().cloned());
    for default in &extra.defaults {
        if !into.defaults.contains(default) {
            into.defaults.push(default.clone());
        }
    }
    into.rels.extend(extra.rels.iter().cloned());
    into.raw_items.extend(extra.raw_items.iter().cloned());
    into.stored |= extra.stored;
    into
}

fn default_core() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>Test drawing</dc:title></cp:coreProperties>".to_owned()
}

fn default_app() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>Test Writer</Application><AppVersion>16.0000</AppVersion></Properties>".to_owned()
}

// ------------------------------------------------------------------------------------------------------------------- XML text

/// `<Cell N=".." V=".."/>`.
pub fn cell(name: &str, value: &str) -> String {
    format!("<Cell N=\"{}\" V=\"{}\"/>", esc(name), esc(value))
}

/// `<Cell N=".." V=".." U=".."/>`.
pub fn cell_u(name: &str, value: &str, unit: &str) -> String {
    format!("<Cell N=\"{}\" V=\"{}\" U=\"{}\"/>", esc(name), esc(value), esc(unit))
}

/// `<Cell N=".." V=".." F=".."/>`.
pub fn cell_f(name: &str, value: &str, formula: &str) -> String {
    format!("<Cell N=\"{}\" V=\"{}\" F=\"{}\"/>", esc(name), esc(value), esc(formula))
}

/// `<Shape ID="id">inner</Shape>`.
pub fn shape(id: u32, inner: &str) -> String {
    format!("<Shape ID=\"{id}\">{inner}</Shape>")
}

/// A shape with attributes: `shape_with(5, r#" Type="Group""#, "..")`.
pub fn shape_with(id: u32, attrs: &str, inner: &str) -> String {
    format!("<Shape ID=\"{id}\"{attrs}>{inner}</Shape>")
}

/// The shape a diagram is made of, as much as a drawing of boxes holds: fourteen cells (six of them formulas), a path of five rows of two cells and a text: 136 items, and 18 KB of
/// model in the reader's budget. For the tests of what a read holds (`memory_price.rs` and the engine's `vsdx_memory.rs`). **Each shape's sheet and path are its own** (a place and a
/// value in the path that depend on `id`): a reader that shares what is equal shares nothing of a drawing of these, which is the worst a file can make, and the
/// figures the tests were measured on.
pub fn typical_shape(id: u32) -> String {
    let direct = [
        cell_u("PinX", &format!("{id}.5"), "IN"),
        cell_u("PinY", "2.25", "IN"),
        cell_u("Width", "1", "IN"),
        cell_u("Height", "0.75", "IN"),
        cell_f("LocPinX", "0.5", "Width*0.5"),
        cell_f("LocPinY", "0.375", "Height*0.5"),
        cell("Angle", "0"),
        cell("FlipX", "0"),
        cell("FlipY", "0"),
        cell_u("LineWeight", "0.01", "IN"),
        cell("LineColor", "0"),
        cell("FillForegnd", "#ffffff"),
        cell_f("TxtPinX", "0.5", "Width*0.5"),
        cell_f("TxtPinY", "0.375", "Height*0.5"),
    ]
    .concat();
    let rows: String = (1..=5)
        .map(|r| {
            format!(
                "<Row T=\"LineTo\" IX=\"{r}\"><Cell N=\"X\" V=\"{r}\" U=\"IN\" F=\"Width*{r}\"/><Cell N=\"Y\" V=\"{id}\" U=\"IN\" F=\"Height*1\"/></Row>"
            )
        })
        .collect();
    let geometry = format!("<Section N=\"Geometry\" IX=\"0\"><Cell N=\"NoFill\" V=\"0\"/><Cell N=\"NoLine\" V=\"0\"/>{rows}</Section>");
    shape_with(
        id,
        " Type=\"Shape\" Name=\"Box\" NameU=\"Box\" LineStyle=\"0\" FillStyle=\"0\" TextStyle=\"0\"",
        &format!("{direct}{geometry}<Text><cp IX=\"0\"/>Hello world</Text>"),
    )
}

/// A drawing of `pages` pages of `per_page` [`typical_shape`]s each (a stencil when `document_type` is the stencil's content type). Twenty pages of 4,000 are 80,000 shapes: the most the
/// reader admits of them, a model of 2.2 GB in its budget in a file of a few megabytes.
pub fn typical_drawing(pages: u32, per_page: u32, document_type: Option<&str>) -> Vec<u8> {
    let mut builder = VsdxBuilder::new();
    if let Some(content_type) = document_type {
        builder = builder.document_type(content_type);
    }
    // Every shape of every page has an `ID` of its own, and with it a sheet and a path of its own: a drawing of equal pages would be one page to a reader that shares what is equal.
    for page in 1..=pages {
        let shapes: String = (0..per_page).map(|i| typical_shape((page - 1) * per_page + i)).collect();
        builder = builder.page(PageSpec::new(page, "P").shapes(&shapes));
    }
    builder.build()
}

/// `n` shapes nested one inside the next, each in a `Shapes`, the innermost holding `inner`: `ID` 1 is the outermost. For the depth tests.
pub fn nested_shapes(n: usize, inner: &str) -> String {
    let mut s = String::new();
    for i in 1..=n {
        s.push_str(&format!("<Shape ID=\"{i}\"><Shapes>"));
    }
    s.push_str(inner);
    for _ in 1..=n {
        s.push_str("</Shapes></Shape>");
    }
    s
}
