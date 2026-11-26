//! `cargo fuzz run odg_write`: the drawing writer on a read-then-write of any package. A first byte of 0 or more makes that many (at most 8) edits of
//! the model out of the next bytes (move, delete, reorder, resize a shape of the first page, restyle one, retext one, set the page's height, move a point of an
//! outline, put a new rectangle on the page, turn one, flip one, put a new connector between two, turn a gradient off with a colour; added the last four
//! adds a line end on a shape (every number 1 to 45 is written, with a band class in the marker's name; 0, 46 to 49 and a size past 6 are refused in words),
//! and a text edit across runs and paragraphs: a line feed, a run's or a paragraph's
//! marker with a new row of cells, a line feed taken out; a new rectangle and a reorder in one save are ops 9 and 2 together; adds op 16, a style sheet
//! added last and a shape linked to it, and op 17, the Snap and Glue words; a new shape that is turned or flipped (ops 9 with 10 and 11) is written now),
//! after a byte of [`Flags`] (the version the parts declare, no `office:automatic-styles`, a page that is an empty element, the OpenOffice.org manifest), and the
//! rest the page body of an otherwise honest
//! package (with a `styles.xml` of a page layout and a master page), so the fuzzer explores the writer's scan, splice and numbering and not only the
//! archive; a first byte of 255 reads the rest as a package as it is, and saves it unedited. A save gives bytes that read again, with an index of as
//! many pages as the model, or an error of a known class; an unedited save keeps every part; a second save of the same model through the new index
//! is no edit, and gives the same bytes; anything else is a crash the fuzzer reports. Reads the first byte divided by 9 as well ([`Kind`]): the package is
//! a drawing template (`.otg`), and the save asks for a kind (`WriteOptions::variant`); a save that was asked for a kind gives a package that states it in
//! `mimetype` and in the manifest's root row, with every other part the original's. A first byte of 254 or 253 reads the rest as a package as it is and asks for a
//! drawing or for a template: a hostile package saved as the other kind, whose unedited save changes exactly the root row's media type and the
//! bytes of `mimetype`.
//! Adds the page edits, ops of 192 and more (a page added, of the first page's layout or of another size or with a copy of its first shape; taken away; put in
//! another place; renamed, to a name from a table of hard ones or to another page's; every page put in the reverse order), and a drawing of up to two pages more than
//! the one the body is on (the number of bytes of the body modulo 3: one an empty element): a save of a changed list of pages gives bytes that read back with the model's
//! pages in its order under its names and `ID`s, and a second save through the new index is no edit.
//! Adds ops 40 to 43: a cell of a `Character` or a `Paragraph` row edited (the style of the paragraph or the run is then made from the file's own, with only
//! the changed cells' attributes), the place of the pin set as a constant (the pin compensating or not), and a `LayerMember` set.
//! Adds ops 44 (a layer of the page renamed) and 45 (every run of a shape given a size of its own), and a layer set to the document.
//! Adds op 46, a connector's route respelled as the engine's rebuild of line jumps leaves it (the corners the same, a point on a leg dropped, a hop put on a leg or
//! taken away), and the property of for a run of that op alone: every `draw:connector` start tag the page body holds is in the saved part as it was, in the save
//! and in a second one from the index the first gave (the amount's low bit says hop on or off, and the second save flips it).
//! Adds op 47, a corner of a connector's route moved (the route is another, whatever its hop), and the property of for a run of
//! ops 46 and 47 alone: after the save, and after a second save from the index the first gave with the hop the other way, every connector whose route has **no hop** reopens
//! with the corners the model has (a route a save kept for its hop is the file's until a save writes the model's).
//! Adds op 48, the first page's size (its height grown or shortened, its width changed), to the run of route edits: the corners kept for a route a
//! save kept are the file's in the page as that save left it, so the property of now also holds across a page that changes height, in the save that keeps the
//! route and in a later one that carries it (the re-review's blocker); the hop of op 46 may be the gap style's (a `MoveTo` in the middle of the route), and the target's
//! reading of a hop sees it; and a chain that keeps a route in a save that leaves the page as it is, carries it across the save that changes it and then takes the hop
//! away. It also narrows what the target lets by of `write_self_check`: only the refusal of a part that does not scan, and only when the input already
//! has a control character in that part or its own part does not scan ([`is_unscannable_part`]).
//! Gives a package whose page body is 3 bytes long modulo 4 the style sheets a drawing program writes (`office:styles`: a graphic style sheet with a fill, a line and text
//! properties, one whose parent it is, a paragraph style sheet, a style sheet with a bitmap fill and the fill image's definition) and the picture's entry with its row, and an automatic
//! style `gr4` of a bitmap fill that the page body may name, so that the model writer writes style sheets natively and pictures of fill images; a model written from nothing then holds
//! every fill image's picture in the package and reads back with the style sheets the model has.
//! Adds, for a run whose edits are all text edits (ops 6 and 15), the property of R10: every bookmark, reference mark and soft page break the page body holds
//! stands in the saved `content.xml` as often as before (a text edit moves a zero-width element and never drops one), and the seeds of the target hold them in
//! every form R10 places (a point, a range across paragraphs, inside a span, in a list item, in a paragraph that is taken out or joined, beside spaces and fields).
#![no_main]

use std::sync::Arc;

use diacraft_container::zip::{ArchiveWriter, Method};
use diacraft_doc::{
    Cell, CellName, Connect, Connects, Document, ForeignData, Links, Media, Page, RawAttrs, Row, Section, Shape, ShapeType, Sheet, SnapWords, Style,
    TextPart, Value,
};
use diacraft_odg::package::Package;
use diacraft_odg::write::shifted_path;
use diacraft_odg::{Form, Limits, OriginalPackage, SaveNote, Variant, WriteOptions, Written, copy_account, diff_packages, read, sniff, write_with};
use libfuzzer_sys::fuzz_target;

const MEDIA: &str = "application/vnd.oasis.opendocument.graphics";
const TEMPLATE: &str = "application/vnd.oasis.opendocument.graphics-template";

const ROOT: &str = concat!(
    r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
    r#"xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" "#,
    r#"xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" "#,
    r#"xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0""#
);

/// What the byte after the count says of the package round the page body: its bits 0 and 1 the version the parts declare (0 ODF 1.3, 1 ODF 1.1, 2
/// ODF 1.0, 3 none), bit 2 that `content.xml` has no `office:automatic-styles` element, bit 3 that a page with no shape is written as an empty element, and
/// bit 4 that the manifest has the OpenOffice.org document type declaration. The seeds of the milestone-1 faults are inputs with these bits.
#[derive(Clone, Copy)]
struct Flags(u8);

impl Flags {
    /// The `office:version` attribute the parts carry.
    fn version(self) -> &'static str {
        match self.0 & 3 {
            0 => r#" office:version="1.3""#,
            1 => r#" office:version="1.1""#,
            2 => r#" office:version="1.0""#,
            _ => "",
        }
    }

    fn legacy(self) -> bool {
        self.0 & 3 != 0
    }

    fn no_automatic_styles(self) -> bool {
        self.0 & 4 != 0
    }

    fn empty_page(self) -> bool {
        self.0 & 8 != 0
    }

    fn old_manifest(self) -> bool {
        self.0 & 16 != 0
    }

    /// Bits 5 and 6: `office:styles` of `styles.xml`, which the markers of line ends are put into: none (0), holding DiaCraft's markers of
    /// numbers 4 and 10, and of numbers 16 and 24 at a band class, as a program that saved the file writes them (1: the path rewritten, the curves
    /// rounded), an empty element (2), an element with no child (3).
    fn common_styles(self) -> &'static str {
        match (self.0 >> 5) & 3 {
            1 => concat!(
                r#"<office:styles><draw:marker draw:name="DiaCraft_20_end_20_4" draw:display-name="DiaCraft end 4" svg:viewBox="0 0 200 200" svg:d="M100 0l100 200h-200z"/>"#,
                r#"<draw:marker draw:name="DiaCraft_20_end_20_10" draw:display-name="DiaCraft end 10" svg:viewBox="0 0 150 150" svg:d="M75 0c41 0 75 34 75 75s-34 75-75 75-75-34-75-75 34-75 75-75z"/>"#,
                r#"<draw:marker draw:name="DiaCraft_20_end_20_16_20_t23" draw:display-name="DiaCraft end 16 t23" svg:viewBox="0 0 2000 2000" svg:d="M1000 0l1000 2000h-2000zM1000 514l-628 1256h1256z"/>"#,
                r#"<draw:marker draw:name="DiaCraft_20_end_20_24_20_t44" draw:display-name="DiaCraft end 24 t44" svg:viewBox="0 0 1500 1880" svg:d="M0 0h1500v1880h-1500z"/></office:styles>"#
            ),
            2 => "<office:styles/>",
            3 => "<office:styles></office:styles>",
            _ => "",
        }
    }

    /// Bit 7: `content.xml` has an empty `office:font-face-decls`, where a face a save declares is put in (else the element is made).
    fn empty_font_decls(self) -> bool {
        self.0 & 128 != 0
    }
}

/// What the first byte says besides how many edits to make: the byte divided by 9, which the number of edits (the byte modulo 9) leaves free; the
/// bits of [`Flags`] are all in use. Bit 0: the package is a drawing template (`.otg`): `mimetype` and the manifest's root row say so. Bits 1 and 2: the kind the
/// save is asked for (`WriteOptions::variant`): 1 a drawing, 2 a template, 0 and 3 none. Bit 3 makes the drawing a flat `.fodg` (the page body in
/// one `office:document`, with an `office:meta` unless the flags' bit 4 says none); bit 4, of a package, asks for the save as a flat document
/// (`WriteOptions::form`).
#[derive(Clone, Copy, Default)]
struct Kind(u8);

impl Kind {
    fn template(self) -> bool {
        self.0 & 1 != 0
    }

    fn asked(self) -> Option<Variant> {
        match (self.0 >> 1) & 3 {
            1 => Some(Variant::Drawing),
            2 => Some(Variant::Template),
            _ => None,
        }
    }

    /// The drawing is a flat document.
    fn flat(self) -> bool {
        self.0 & 8 != 0
    }

    /// A package is asked to be saved as a flat document.
    fn to_flat(self) -> bool {
        self.0 & 16 != 0
    }
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

/// The text a visit writes back as it was, as a page's first shape: a bold span over part of a paragraph, a span of a style whose attributes no cell reads, a paragraph style that
/// holds text properties (and a span inside it that states the weight alone), and a paragraph that names a style of nothing but attributes no cell reads.
const SEED_TEXT: &str = concat!(
    r#"<draw:frame svg:x="1cm" svg:y="13cm" svg:width="8cm" svg:height="4cm"><draw:text-box>"#,
    r#"<text:p>plain <text:span text:style-name="T3">bold</text:span> and <text:span text:style-name="T4">language</text:span> end</text:p>"#,
    r#"<text:p text:style-name="P4">sized <text:span text:style-name="T3">weight</text:span> text</text:p>"#,
    r#"<text:p text:style-name="P5">orphans</text:p>"#,
    r#"</draw:text-box></draw:frame>"#
);

const STYLES_TAIL: &str = concat!(
    r#"<office:automatic-styles><style:page-layout style:name="PM1"><style:page-layout-properties fo:page-width="21cm" fo:page-height="29.7cm"/></style:page-layout>"#,
    // A layer set of the document: the layers a new shape's `draw:layer` is resolved against, and the rows the document file may rename.
    r#"</office:automatic-styles><office:master-styles><draw:layer-set><draw:layer draw:name="Notes"/><draw:layer draw:name="Other"/></draw:layer-set><style:master-page style:name="Default" style:page-layout-name="PM1"/></office:master-styles></office:document-styles>"#
);

const MANIFEST: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
    r#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3">"#,
    r#"<manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.graphics"/>"#,
    r#"<manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>"#,
    r#"<manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/>"#,
    r#"</manifest:manifest>"#
);

/// An honest flat drawing around the page body `body`, as `flags` says, of the kind `kind` says: the same pieces as [`package`], in one `office:document`.
fn flat(body: &[u8], flags: Flags, kind: Kind) -> Option<Vec<u8>> {
    let styles = if flags.no_automatic_styles() {
        ""
    } else {
        concat!(
            r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000"/></style:style>"##,
            r##"<style:style style:name="gr2" style:family="graphic"><style:graphic-properties draw:stroke="solid"/></style:style>"##,
            r##"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style>"##,
            r##"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"##,
            r##"<style:style style:name="P2" style:family="paragraph"><style:paragraph-properties fo:text-align="center" text:enable-numbering="false" text:number-lines="true" draw:fill-color="#00ff00"/></style:style>"##,
            r##"<style:style style:name="P3" style:family="paragraph"><style:paragraph-properties fo:text-align="right" xmlns:diacraft="urn:app.diacraft:extension" diacraft:older-enable-numbering="true"/></style:style>"##,
            r##"<style:style style:name="T2" style:family="text"><style:text-properties fo:font-weight="bold" text:display="none" text:enable-numbering="true" draw:fill-color="#00ff00"/></style:style>"##,
            r##"<style:style style:name="T3" style:family="text"><style:text-properties fo:font-weight="250" fo:font-style="slanted" xmlns:diacraft="urn:app.diacraft:extension" diacraft:older-font-weight="300"/></style:style>"##,
            r##"<style:style style:name="P4" style:family="paragraph"><style:paragraph-properties text:enable-numbering="false" fo:text-align="left"/></style:style>"##
        )
    };
    let media = if kind.template() { TEMPLATE } else { MEDIA };
    let meta = if flags.old_manifest() { "" } else { "<office:meta><meta:generator>Fuzz/1</meta:generator></office:meta>" };
    let fonts = if flags.empty_font_decls() { "<office:font-face-decls/>" } else { "" };
    let page = if body.is_empty() && flags.empty_page() {
        br#"<draw:page draw:name="p" draw:master-page-name="Default"/>"#.to_vec()
    } else {
        [&br#"<draw:page draw:name="p" draw:master-page-name="Default">"#[..], body, &b"</draw:page>"[..]].concat()
    };
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document {ROOT}{} office:mimetype=\"{media}\">{meta}{fonts}{}<office:automatic-styles>{styles}<style:page-layout style:name=\"PM1\"><style:page-layout-properties fo:page-width=\"21cm\" fo:page-height=\"29.7cm\"/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"/></office:master-styles><office:body><office:drawing>\n",
        flags.version(),
        flags.common_styles()
    )
    .into_bytes();
    out.extend_from_slice(&page);
    out.extend_from_slice(b"\n</office:drawing></office:body></office:document>");
    Some(out)
}

/// The style sheets a drawing program writes, with a bitmap fill and its definition, for `office:styles`.
const SHEETS: &str = concat!(
    r#"<draw:fill-image draw:name="Img1" xlink:href="Pictures/fill.png" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad" xmlns:xlink="http://www.w3.org/1999/xlink"/>"#,
    r##"<style:style style:name="standard" style:family="graphic"><style:graphic-properties draw:stroke="solid" svg:stroke-color="#3465a4" draw:fill="solid" draw:fill-color="#729fcf" fo:padding-left="0.25cm"/><style:paragraph-properties fo:margin-left="0cm"/><style:text-properties fo:font-size="18pt" style:font-size-asian="18pt"/></style:style>"##,
    r##"<style:style style:name="Filled" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:fill-color="#ffd428"/><style:text-properties fo:font-weight="bold"/></style:style>"##,
    r#"<style:style style:name="Heading" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style>"#,
    // A style sheet that underlines and says nothing else of the text, the second style sheet of a name, and a style sheet with no name.
    r#"<style:style style:name="Plain" style:family="graphic"><style:text-properties style:text-underline-style="solid"/></style:style>"#,
    r#"<style:style style:name="Heading" style:family="paragraph"><style:paragraph-properties fo:text-align="left"/></style:style>"#,
    r#"<style:style style:family="graphic"><style:graphic-properties draw:fill="solid"/></style:style>"#,
    r#"<style:style style:name="Pictured" style:family="graphic"><style:graphic-properties draw:fill="bitmap" draw:fill-image-name="Img1" style:repeat="stretch"/></style:style>"#,
    // A style sheet whose weight no list of ODF 1.4 has (written as the nearest, the weight carried), one carried already, and one whose carrier is outdated.
    r#"<style:style style:name="Light" style:family="graphic"><style:text-properties fo:font-weight="250" style:font-weight-complex="650" fo:font-style="slanted"/></style:style>"#,
    r#"<style:style style:name="Carried" style:family="graphic"><style:text-properties fo:font-weight="200" xmlns:diacraft="urn:app.diacraft:extension" diacraft:older-font-weight="250"/></style:style>"#,
    r#"<style:style style:name="Stale" style:family="graphic"><style:text-properties fo:font-weight="bold" xmlns:diacraft="urn:app.diacraft:extension" diacraft:older-font-weight="250"/></style:style>"#
);

/// A PNG's signature and a little more: a picture the reader holds the bytes of.
const FILL_PNG: [u8; 12] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];

/// `common` (what [`Flags::common_styles`] gives) with the style sheets of [`SHEETS`] put in `office:styles`.
fn with_sheets(common: &str) -> String {
    match common {
        "" | "<office:styles/>" => format!("<office:styles>{SHEETS}</office:styles>"),
        full => full.replacen("</office:styles>", &format!("{SHEETS}</office:styles>"), 1),
    }
}

/// An honest package around the page body `body`, as `flags` says, of the kind `kind` says.
fn package(body: &[u8], flags: Flags, kind: Kind) -> Option<Vec<u8>> {
    // The style sheets and a fill image, for a body 3 bytes long modulo 4.
    let sheets = body.len() % 4 == 3;
    // With an automatic style that the page's shapes may share, so that a restyle clones, edits in place and orphans (or with none at all).
    let styles = if flags.no_automatic_styles() {
        ""
    } else {
        concat!(
            r##"<office:automatic-styles><style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000"/></style:style>"##,
            r##"<style:style style:name="gr2" style:family="graphic"><style:graphic-properties draw:stroke="solid"/></style:style>"##,
            // A line end of DiaCraft's own at a band class, so that taking the shape out or changing its weight has a marker to take out or to name again.
            r##"<style:style style:name="gr3" style:family="graphic"><style:graphic-properties draw:marker-end="DiaCraft_20_end_20_16_20_t23" draw:marker-end-width="6.54pt" svg:stroke-width="0.75pt" draw:stroke="solid"/></style:style>"##,
            // A paragraph style and a text style that hold values no cell maps (a line spacing and a tab stop with a leader; a wave underline with its colour, a
            // lowercase, a position and a language), which an edit of another cell leaves as they are.
            r##"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="center" style:line-spacing="0.1cm"><style:tab-stops><style:tab-stop style:position="2cm" style:leader-style="dotted" style:leader-text="."/></style:tab-stops></style:paragraph-properties></style:style>"##,
            r##"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold" style:text-underline-style="wave" style:text-underline-color="#ff0000" fo:text-transform="lowercase" style:text-position="33% 58%" fo:language="de"/></style:style>"##,
            // GATE.b94e.F1: an attribute of an earlier ODF version on a paragraph's properties (`text:enable-numbering`, which 1.2 and 1.4 have no place for: the model writer carries it in an attribute of
            // DiaCraft's), the carrier itself, and attributes of the standard's namespaces that a properties element may and may not take (`text:number-lines`, `text:display`, `draw:fill-color`).
            r##"<style:style style:name="P2" style:family="paragraph"><style:paragraph-properties fo:text-align="center" text:enable-numbering="false" text:number-lines="true" draw:fill-color="#00ff00"/></style:style>"##,
            r##"<style:style style:name="P3" style:family="paragraph"><style:paragraph-properties fo:text-align="right" xmlns:diacraft="urn:app.diacraft:extension" diacraft:older-enable-numbering="true"/></style:style>"##,
            r##"<style:style style:name="T2" style:family="text"><style:text-properties fo:font-weight="bold" text:display="none" text:enable-numbering="true" draw:fill-color="#00ff00"/></style:style>"##,
            // Styles that nothing names, written whole, with the forms of an earlier version ODF 1.4 has no place for: a weight and a slant no list has, with a carrier of DiaCraft's
            // that is outdated, the paragraph properties with `text:enable-numbering` and the carrier of it in its place, and a weight carried as the writer carries it.
            r##"<style:style style:name="T3" style:family="text"><style:text-properties fo:font-weight="250" fo:font-style="slanted" xmlns:diacraft="urn:app.diacraft:extension" diacraft:older-font-weight="300"/></style:style>"##,
            r##"<style:style style:name="P4" style:family="paragraph"><style:paragraph-properties text:enable-numbering="false" fo:text-align="left"/><style:text-properties fo:font-weight="200" xmlns:diacraft="urn:app.diacraft:extension" diacraft:older-font-weight="250"/></style:style>"##,
            r##"<style:style style:name="P5" style:family="paragraph"><style:paragraph-properties xmlns:diacraft="urn:app.diacraft:extension" diacraft:older-enable-numbering="true" fo:text-align="right"/></style:style>"##,
            // An automatic style that nothing names, one with no name, the second of a name, and one of attributes no field reads.
            r##"<style:style style:name="gr5" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#00ff00"/></style:style>"##,
            r##"<style:style style:family="graphic"><style:graphic-properties draw:fill="solid"/></style:style>"##,
            r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#0000ff"/></style:style>"##,
            r##"<style:style style:name="gr6" style:family="graphic"><style:graphic-properties draw:textarea-horizontal-align="justify"/></style:style>"##,
            // A text style that states the weight alone, one of an attribute no cell reads, a paragraph style that holds text properties, and one of an attribute no cell reads.
            r##"<style:style style:name="T3" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"##,
            r##"<style:style style:name="T4" style:family="text"><style:text-properties fo:language="en" fo:country="US"/></style:style>"##,
            r##"<style:style style:name="P4" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/><style:text-properties fo:font-size="20pt" fo:color="#FF0000"/></style:style>"##,
            r##"<style:style style:name="P5" style:family="paragraph"><style:paragraph-properties fo:orphans="2"/></style:style>"##,
            // A shape's bitmap fill, named by the fill image of the style sheets (when the package has them).
            r#"<style:style style:name="gr4" style:family="graphic" style:parent-style-name="Filled"><style:graphic-properties draw:fill="bitmap" draw:fill-image-name="Img1"/></style:style></office:automatic-styles>"#
        )
    };
    let version = flags.version();
    let fonts = if flags.empty_font_decls() { "<office:font-face-decls/>" } else { "" };
    let mut content = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-content {ROOT}{version}>{fonts}{styles}<office:body><office:drawing>\n"
    )
    .into_bytes();
    if body.is_empty() && flags.empty_page() {
        content.extend_from_slice(b"<draw:page draw:name=\"p\" draw:master-page-name=\"Default\"/>");
    } else {
        content.extend_from_slice(b"<draw:page draw:name=\"p\" draw:master-page-name=\"Default\">");
        // One input in two (the parity of the body's length) starts with the shapes a visit writes back as they were.
        if body.len() % 2 == 1 {
            content.extend_from_slice(SEED_SHAPES.as_bytes());
        }
        // One input in three (the length of the body modulo 3) starts with the text a visit writes back as it was.
        if body.len() % 3 == 1 {
            content.extend_from_slice(SEED_TEXT.as_bytes());
        }
        content.extend_from_slice(body);
        content.extend_from_slice(b"</draw:page>");
    }
    // A drawing of more than one page, so that the page edits have pages to take away, move and rename: up to two more, of no shape, one of them an empty element.
    for k in 0..body.len() % 3 {
        if k == 0 {
            content.extend_from_slice(b"<draw:page draw:name=\"q0\" draw:master-page-name=\"Default\"/>");
        } else {
            content.extend_from_slice(b"<draw:page draw:name=\"q1\" draw:master-page-name=\"Default\"></draw:page>");
        }
    }
    content.extend_from_slice(b"\n</office:drawing></office:body></office:document-content>");
    let common = if sheets { with_sheets(flags.common_styles()) } else { flags.common_styles().to_owned() };
    let styles = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-styles {ROOT}{version}>{common}{STYLES_TAIL}");
    let media = if kind.template() { TEMPLATE } else { MEDIA };
    let manifest = if flags.old_manifest() {
        MANIFEST.replacen(
            "<manifest:manifest",
            "<!DOCTYPE manifest:manifest PUBLIC \"-//OpenOffice.org//DTD Manifest 1.0//EN\" \"Manifest.dtd\">\n<manifest:manifest",
            1,
        )
    } else {
        MANIFEST.to_owned()
    };
    let mut manifest = manifest.replacen(MEDIA, media, 1);
    if sheets {
        manifest = manifest.replacen(
            "</manifest:manifest>",
            r#"<manifest:file-entry manifest:full-path="Pictures/fill.png" manifest:media-type="image/png"/></manifest:manifest>"#,
            1,
        );
    }
    let mut w = ArchiveWriter::new();
    w.add("mimetype", media.as_bytes(), Method::Stored).ok()?;
    w.add("content.xml", &content, Method::Deflated).ok()?;
    w.add("styles.xml", styles.as_bytes(), Method::Deflated).ok()?;
    if sheets {
        w.add("Pictures/fill.png", &FILL_PNG, Method::Stored).ok()?;
    }
    w.add("META-INF/manifest.xml", manifest.as_bytes(), Method::Deflated).ok()?;
    w.finish().ok()
}

/// The classes a save may give. **`write_self_check` is one only for a part of the input the scan cannot read** (and the reviews): the check of
/// the bytes made reads back the tags the save wrote on (names twice, values the reader refuses), so bytes that do not check are a bug of the writer, which the target must
/// find and not let pass as an error of a known class; but a page body with a control character in it (U+0010) is read, and its part does not scan, so any edited save of it
/// is refused in words (`content.xml does not scan`) with nothing written. **The words alone are not enough** (MEDIUM): the writer says
/// " does not scan: " of bytes it made itself as well (`write/mod.rs`: the check of the parts a save made), so a fault of the writer that writes XML which does not scan
/// would have passed as that refusal. The refusal is let by **only when the input's own part is one the scan cannot read** ([`part_does_not_scan`]): it already holds a
/// control character, or the scan of the container refuses it as it is, which is the first thing the writer's scan does. The part is the one the error names
/// (`content.xml` or `styles.xml`), and any of the two when it names none (`the part made`); a flat document is the one part under both names. `input` is the bytes the
/// save was given.
fn is_unscannable_part(e: &diacraft_odg::WriteError, input: &[u8]) -> bool {
    if e.class() != "write_self_check" {
        return false;
    }
    let text = e.to_string();
    if !text.contains(" does not scan: ") {
        return false;
    }
    let named = ["content.xml", "styles.xml"].into_iter().find(|name| text.contains(&format!("{name} does not scan: ")));
    input_parts(input).iter().any(|(name, bytes)| named.is_none_or(|n| n == *name) && part_does_not_scan(bytes))
}

/// The parts of the input a save reads and scans, by name: a package's `content.xml` and `styles.xml`, or a flat document's one part under both names. A package the
/// package layer cannot read has none.
fn input_parts(input: &[u8]) -> Vec<(&'static str, Vec<u8>)> {
    match Form::of(input) {
        Form::Flat => vec![("content.xml", input.to_vec()), ("styles.xml", input.to_vec())],
        Form::Package => match Package::read(input, &Limits::default()) {
            Ok(package) => {
                ["content.xml", "styles.xml"].into_iter().filter_map(|name| package.part(name).map(|bytes| (name, bytes.to_vec()))).collect()
            }
            Err(_) => Vec::new(),
        },
    }
}

/// Whether the part is one the scan cannot read: it holds a byte below 0x20 that XML 1.0 does not allow (anything but a tab, a line feed and a carriage return), or the
/// container's scan, which the writer's scan is made on, refuses it (a part in UTF-16, one that is not well formed).
fn part_does_not_scan(bytes: &[u8]) -> bool {
    let refuses = || {
        let mut work = diacraft_container::Work::default();
        diacraft_container::xml_scan::scan_tags(bytes, &Limits::default().container.xml, &mut work, |_| Ok::<(), ()>(())).is_err()
    };
    bytes.iter().any(|b| *b < 0x20 && !matches!(b, b'\t' | b'\n' | b'\r')) || refuses()
}

const CLASSES: &[&str] = &[
    "write_unsupported",
    "write_mismatch",
    "write_limit",
    "write_thumbnail",
    "write_media_type",
    "not_zip",
    "zip_corrupt",
    "path_traversal",
    "invalid_part_name",
    "zip_ratio_exceeded",
    "part_too_large",
    "package_too_large",
    "too_many_parts",
    "unsupported_compression",
    "encrypted_package",
    "duplicate_part",
    "out_of_memory",
    "xml_malformed",
    "xml_entity_refused",
    "xml_too_complex",
    "missing_manifest",
    "manifest_malformed",
    "not_a_drawing",
    "no_drawing_content",
    "too_many_manifest_entries",
    "too_much_xml",
    "too_many_pages",
    "too_many_styles",
    "too_many_shapes_in_a_page",
    "too_many_shapes",
    "too_many_cells",
    "too_many_items",
    "too_much_text",
    "model_limit",
];

/// The names a page op gives a page: plain, with what an attribute value escapes, with a character the style-name encoding writes as `_20_`, with a line feed
/// and a tab, one that is a default name, one XML 1.0 does not allow, and another script.
const PAGE_NAMES: [&str; 10] = ["a", "p", "a_20_b", "x&y", "q\"<>'", "two\nlines", "tab\there", "Page-1", "bad\u{1}name", "日本語"];

/// Makes the page edit `(op, which, amount)` (an `op` of 192 or more): a page added (a copy of the first page's layout, or of another size, or one that holds a
/// copy of the first page's first shape), taken away, put in another place, renamed (to a name from [`PAGE_NAMES`] or to another page's), or all the pages put in the
/// reverse order. The pages are matched with the file's by `ID`, which an added page takes above every other.
fn page_edit(doc: &mut Document, (op, which, amount): (u8, u8, u8)) {
    let n = doc.pages.len();
    if n == 0 {
        return;
    }
    let at = usize::from(which) % n;
    let name = PAGE_NAMES[usize::from(amount) % PAGE_NAMES.len()];
    match op % 8 {
        0..=1 | 7 => {
            if n >= 8 {
                return;
            }
            let id = doc.pages.iter().map(|p| p.id).max().unwrap_or(0).saturating_add(1);
            let mut page = Page::new(id, name);
            if let Some(first) = doc.pages.get(0) {
                page.sheet = Arc::clone(&first.sheet);
                if op % 8 == 1 {
                    // Another size: the width is not the first page's.
                    let sheet = Arc::make_mut(&mut page.sheet);
                    if let Some(cell) = sheet.cells.get_mut("PageWidth")
                        && let Some(v) = cell.value.as_ref().and_then(Value::number)
                    {
                        cell.value = Value::from_number(v + 1.0 + f64::from(amount) / 16.0);
                    }
                }
                if op % 8 == 7
                    && let Some(shape) = first.shapes.get(0)
                {
                    let mut shape = (**shape).clone();
                    shape.id = 1;
                    page.shapes.push(Arc::new(shape));
                }
            }
            doc.pages.push(Arc::new(page));
        }
        2 => {
            if n > 1 {
                doc.pages.remove(at);
            }
        }
        3 => {
            if let Some(page) = doc.pages.remove(at) {
                let _ = doc.pages.insert(usize::from(amount) % n, page);
            }
        }
        4 => {
            if let Some(page) = doc.pages.get_mut(at) {
                Arc::make_mut(page).name = Arc::from(name);
            }
        }
        5 => {
            let reversed: Vec<Arc<Page>> = doc.pages.iter().rev().cloned().collect();
            doc.pages = diacraft_doc::SharedVec::from_vec(reversed);
        }
        _ => {
            // The name of another page.
            let other = doc.pages.get(usize::from(amount) % n).map(|p| p.name.clone());
            if let (Some(page), Some(other)) = (doc.pages.get_mut(at), other) {
                Arc::make_mut(page).name = other;
            }
        }
    }
}

/// Op 16 adds a style sheet after the file's (named `Theme` or `My Theme`, so that the escape is written) and links the shape the byte names (the last one, with the
/// high bit set: a new shape) to it, by
/// all three links, or by one of them (which is refused in words); op 17 gives the document the Snap and Glue words the byte says, each of the four present or
/// not, and a word that is not a number; ops 18 and 19 take the last style sheet away and fill in or empty the cells of one.
fn style_or_words(doc: &mut Document, (op, which, amount): (u8, u8, u8)) {
    // Op 18 takes the last style sheet away, as an undo of one a save added does (the shapes that named it are put back on the root when the byte's low bit is set, and
    // left naming it otherwise: a model no command makes); op 19 gives the style sheet the byte names the cells of a `Theme` style, or takes its cells away (
    // the cells of an empty common style are carried, any other change of a style sheet of the file is refused in words).
    if op == 18 {
        let keep = doc.styles.len().saturating_sub(1);
        let gone = u32::try_from(keep).unwrap_or(u32::MAX);
        doc.styles = diacraft_doc::SharedVec::from_vec(doc.styles.iter().take(keep).cloned().collect());
        if amount & 1 == 1 {
            for at in 0..doc.pages.len() {
                let Some(page) = doc.pages.get_mut(at) else { continue };
                let page = Arc::make_mut(page);
                for k in 0..page.shapes.len() {
                    let Some(shape) = page.shapes.get_mut(k) else { continue };
                    let sheet = Arc::make_mut(&mut Arc::make_mut(shape).sheet);
                    if [sheet.links.line, sheet.links.fill, sheet.links.text].contains(&Some(gone)) {
                        sheet.links = Links { line: Some(0), fill: Some(0), text: Some(0) };
                    }
                }
            }
        }
        return;
    }
    if op == 19 {
        let at = usize::from(which) % doc.styles.len().max(1);
        let Some(style) = doc.styles.get_mut(at) else { return };
        let sheet = Arc::make_mut(&mut Arc::make_mut(style).sheet);
        if amount & 1 == 1 {
            sheet.cells.insert(CellName::new("ThemeIndex"), Cell::constant(Value::from_text("65534")));
            sheet.cells.insert(CellName::new("FillForegnd"), Cell::with_formula("THEMEVAL()", Some(Value::from_text("Themed"))));
        } else {
            sheet.cells.clear();
        }
        return;
    }
    if op == 17 {
        let word = |bit: u8, text: String| (amount & bit != 0).then(|| Arc::<str>::from(text));
        let words = SnapWords {
            glue: word(1, u32::from(amount).to_string()),
            snap: word(2, (u32::from(amount) * 257).to_string()),
            extensions: word(4, "34".to_owned()),
            dynamic_grid: word(8, if amount & 16 != 0 { "1".to_owned() } else { "not a number".to_owned() }),
        };
        doc.set_snap_words(&words);
        return;
    }
    let id = u32::try_from(doc.styles.len()).unwrap_or(u32::MAX);
    let name = if amount & 1 == 0 { "Theme" } else { "My Theme" };
    let mut sheet = Sheet { links: Links { line: Some(0), fill: Some(0), text: Some(0) }, ..Sheet::default() };
    sheet.cells.insert(CellName::new("ThemeIndex"), Cell::constant(Value::from_text("65534")));
    sheet.cells.insert(CellName::new("FillForegnd"), Cell::with_formula("THEMEVAL()", Some(Value::from_text("Themed"))));
    doc.styles.push(Arc::new(Style { id, name: Some(Arc::from(name)), name_u: Some(Arc::from(name)), sheet: Arc::new(sheet), ..Style::default() }));
    let Some(page) = doc.pages.get_mut(0) else { return };
    let page = Arc::make_mut(page);
    // A byte with its high bit set names the last shape: the new shape an earlier edit of the run put on the page, which is written on the style.
    let at = if which & 0x80 != 0 { page.shapes.len().saturating_sub(1) } else { usize::from(which) % page.shapes.len().max(1) };
    let Some(shape) = page.shapes.get_mut(at) else { return };
    let sheet = Arc::make_mut(&mut Arc::make_mut(shape).sheet);
    let one = |on: bool| Some(if on { id } else { 0 });
    sheet.links = match amount / 2 % 4 {
        0 | 1 => Links { line: one(true), fill: one(true), text: one(true) },
        2 => Links { line: one(true), fill: one(false), text: one(false) },
        _ => Links { line: Some(id.saturating_add(7)), fill: Some(id.saturating_add(7)), text: Some(id.saturating_add(7)) },
    };
}

/// Ops 40 to 43 (their `op % 16` is 8 to 11: not 6 or 15, which the property takes for a text edit): a cell of a row the shape holds is edited, so
/// that the writer makes the style of a paragraph or a run from the style the file has, with only the attributes of the cells that changed (op 40 a `Character`
/// row's, 41 a `Paragraph` row's); op 42 sets the place of the pin as a constant, the pin compensating when the amount's low bit says so; op 43 puts the shape on
/// the layer of a row of the page (a shape that is new is written on it). The row, the cell and the value are from the bytes.
fn row_edit(doc: &mut Document, (op, which, amount): (u8, u8, u8)) {
    let Some(page) = doc.pages.get_mut(0) else { return };
    let page = Arc::make_mut(page);
    if op == 44 {
        // A layer of the page renamed (the document file carries it, and the markup keeps the old name: a shape put on that row afterwards is written by
        // the markup's name or on `layout`, and reads back on the same row).
        let sheet = Arc::make_mut(&mut page.sheet);
        if let Some(section) = sheet.sections.iter_mut().find(|s| s.name.as_str() == "Layer") {
            let section = Arc::make_mut(section);
            let rows = section.rows.len().max(1);
            if let Some(row) = section.rows.get_mut(usize::from(which) % rows) {
                let name = if amount % 2 == 0 { "Comments" } else { "Notes" };
                Arc::make_mut(row).cells.insert(CellName::new("Name"), Cell::constant(Value::from_text(name)));
            }
        }
        return;
    }
    let at = usize::from(which) % page.shapes.len().max(1);
    let Some(shape) = page.shapes.get_mut(at) else { return };
    let sheet = Arc::make_mut(&mut Arc::make_mut(shape).sheet);
    let number = |v: f64| Cell::constant(Value::from_number(v).unwrap_or_else(|| Value::from_text("0")));
    match op {
        40 | 41 => {
            let (section, cells): (&str, &[&str]) = if op == 40 {
                (
                    "Character",
                    &["Size", "Style", "Case", "Pos", "Color", "Letterspace", "FontScale", "DblUnderline", "Strikethru", "Overline", "Font"],
                )
            } else {
                ("Paragraph", &["HorzAlign", "Flags", "IndLeft", "IndRight", "IndFirst", "SpBefore", "SpAfter", "SpLine"])
            };
            let Some(section) = sheet.sections.iter_mut().find(|s| s.name.as_str() == section) else { return };
            let section = Arc::make_mut(section);
            let rows = section.rows.len().max(1);
            let Some(row) = section.rows.get_mut(usize::from(which) / 2 % rows) else { return };
            let row = Arc::make_mut(row);
            let cell = cells.get(usize::from(amount) % cells.len()).copied().unwrap_or("Size");
            let value = match cell {
                "Size" | "Letterspace" | "IndLeft" | "IndRight" | "IndFirst" | "SpBefore" | "SpAfter" => f64::from(amount) / 64.0,
                "Style" => f64::from(amount % 64),
                "Case" | "Pos" => f64::from(amount % 3),
                "HorzAlign" => f64::from(amount % 5),
                "Flags" | "DblUnderline" | "Strikethru" | "Overline" => f64::from(amount % 2),
                "SpLine" => -f64::from(amount % 8) / 2.0,
                "FontScale" => 0.5 + f64::from(amount) / 128.0,
                _ => 0.0,
            };
            let new = match cell {
                "Color" => Cell::constant(Value::from_text(&format!("#{amount:02x}{:02x}{:02x}", amount.wrapping_mul(5), amount.wrapping_mul(11)))),
                "Font" => Cell::constant(Value::from_text(if amount % 2 == 0 { "Mono" } else { "Sans" })),
                _ => number(value),
            };
            row.cells.insert(CellName::new(cell), new);
        }
        42 => {
            let (place, pin) = if amount & 2 == 0 { ("LocPinX", "PinX") } else { ("LocPinY", "PinY") };
            let number_of = |s: &Sheet, name: &str| s.cell(name).and_then(|c| c.value.as_ref()?.number());
            let (Some(old_place), Some(old_pin)) = (number_of(sheet, place), number_of(sheet, pin)) else { return };
            let shift = f64::from(amount >> 2) / 32.0;
            sheet.cells.insert(CellName::new(place), number(old_place - shift));
            if amount & 1 != 0 {
                sheet.cells.insert(CellName::new(pin), number(old_pin - shift));
            }
        }
        45 => {
            // Every run of the shape its own size: each distinct row is a copy of the style its span names, charged to what the reader reads of a part.
            if let Some(section) = sheet.sections.iter_mut().find(|s| s.name.as_str() == "Character") {
                let section = Arc::make_mut(section);
                for (k, row) in section.rows.iter_mut().enumerate() {
                    Arc::make_mut(row).cells.insert(CellName::new("Size"), number(f64::from(amount % 32) / 64.0 + k as f64 / 256.0));
                }
            }
        }
        _ => {
            sheet.cells.insert(CellName::new("LayerMember"), Cell::constant(Value::from_text(&format!("{};{}", amount % 4, amount / 4 % 4))));
        }
    }
}

/// Op 46 (its `op % 16` is 14, which the property does not take for a text edit): the route of a connector of the first page (the one `which` picks
/// among them) is written again as the engine's rebuild of line jumps writes it, **the same corners in other rows**: a `MoveTo` and a `LineTo` row for each corner, the
/// points the file held on a leg left out, and, when the amount's low bit is 1, a hop on a leg (a `LineTo` to 40% of it and an `ArcTo` to 60%: the chord's ends lie on the
/// leg), the leg being the one the amount's other bits pick. **When the byte `which` has its high bit set the hop is the gap style's** (a `MoveTo` to 60% of
/// the leg, in the middle of the route, where the pen lifts). A route that is not a `MoveTo` and rows of `LineTo`, `ArcTo` and (after the first) `MoveTo` is left as it is.
fn route_respell(doc: &mut Document, (_, which, amount): (u8, u8, u8)) {
    let Some(page) = doc.pages.get_mut(0) else { return };
    let page = Arc::make_mut(page);
    let connectors: Vec<usize> = page.shapes.iter().enumerate().filter(|(_, s)| s.sheet.cell("ShapeRouteStyle").is_some()).map(|(i, _)| i).collect();
    let Some(&at) = connectors.get(usize::from(which) % connectors.len().max(1)) else { return };
    let Some(shape) = page.shapes.get_mut(at) else { return };
    let sheet = Arc::make_mut(&mut Arc::make_mut(shape).sheet);
    let Some(section) = sheet.sections.iter_mut().find(|s| s.name.as_str() == "Geometry" && s.ix == Some(0)) else { return };
    let section = Arc::make_mut(section);
    let mut corners: Vec<(f64, f64)> = Vec::new();
    for (i, row) in section.rows.iter().enumerate() {
        let allowed =
            if i == 0 { row.row_type.as_deref() == Some("MoveTo") } else { matches!(row.row_type.as_deref(), Some("LineTo" | "ArcTo" | "MoveTo")) };
        let at = |n: &str| row.cells.get(n).and_then(|c| c.value.as_ref()?.number()).filter(|v| v.is_finite());
        let (true, Some(x), Some(y)) = (allowed && !row.del, at("X"), at("Y")) else { return };
        // A point on the leg between the one before and the next is no corner: dropped when the next arrives.
        while let [.., a, b] = corners[..] {
            let (ac, ab) = ((x - a.0, y - a.1), (b.0 - a.0, b.1 - a.1));
            let length = ac.0.hypot(ac.1);
            let along = if length > 0.0 { (ab.0 * ac.0 + ab.1 * ac.1) / length } else { 0.0 };
            if length > 1e-6 && (ab.0 * ac.1 - ab.1 * ac.0).abs() / length < 1e-9 && along > 1e-6 && along < length - 1e-6 {
                corners.pop();
            } else {
                break;
            }
        }
        corners.push((x, y));
    }
    if corners.len() < 2 || corners.len() > 4096 {
        return;
    }
    let hop_leg = (amount & 1 == 1).then(|| usize::from(amount >> 1) % (corners.len() - 1));
    let gap = which & 0x80 != 0;
    let number = |v: f64| Cell::constant(Value::from_number(v).unwrap_or_else(|| Value::from_text("0")));
    let mut rows: Vec<Arc<Row>> = Vec::new();
    let mut push = |kind: &str, p: (f64, f64), bow: Option<f64>| {
        let mut row = Row { ix: Some(rows.len() as u32 + 1), row_type: Some(Arc::from(kind)), ..Row::default() };
        row.cells.insert(CellName::new("X"), number(p.0));
        row.cells.insert(CellName::new("Y"), number(p.1));
        if let Some(bow) = bow {
            row.cells.insert(CellName::new("A"), number(bow));
        }
        rows.push(Arc::new(row));
    };
    for (k, p) in corners.iter().enumerate() {
        if k == 0 {
            push("MoveTo", *p, None);
            continue;
        }
        let from = corners[k - 1];
        if hop_leg == Some(k - 1) {
            let at = |t: f64| (from.0 + (p.0 - from.0) * t, from.1 + (p.1 - from.1) * t);
            push("LineTo", at(0.4), None);
            if gap {
                push("MoveTo", at(0.6), None);
            } else {
                push("ArcTo", at(0.6), Some(0.04));
            }
        }
        push("LineTo", *p, None);
    }
    section.rows = rows.into();
}

/// Op 47 (its `op % 16` is 15, a text edit to the property, which a run of this op alone is not: its edits are 47s and the property holds
/// for them all the same, since it asks of text edits only that no zero-width element goes): the `X` of one row of a connector's route, in the connector's own frame, moves
/// by a share of an inch from the byte, so that the route has other corners than the file's. The connector is the one `which` picks among the page's, and the row the
/// amount's. **Never the first row or the last**: they are the route's two ends, where the connector's own ends are (a `Begin` and an `End` the model keeps in step with them),
/// and a route that starts or stops elsewhere than its connector is no route an edit makes (the re-check, MEDIUM: shifted off its end, the route was written
/// from the end and read back with another corner, a property that crashed on a non-bug).
fn route_shift(doc: &mut Document, (_, which, amount): (u8, u8, u8)) {
    let Some(page) = doc.pages.get_mut(0) else { return };
    let page = Arc::make_mut(page);
    let connectors: Vec<usize> = page.shapes.iter().enumerate().filter(|(_, s)| s.sheet.cell("ShapeRouteStyle").is_some()).map(|(i, _)| i).collect();
    let Some(&at) = connectors.get(usize::from(which) % connectors.len().max(1)) else { return };
    let Some(shape) = page.shapes.get_mut(at) else { return };
    let sheet = Arc::make_mut(&mut Arc::make_mut(shape).sheet);
    let Some(section) = sheet.sections.iter_mut().find(|s| s.name.as_str() == "Geometry" && s.ix == Some(0)) else { return };
    let section = Arc::make_mut(section);
    // The rows between the first and the last.
    let inner = section.rows.len().saturating_sub(2);
    if inner == 0 {
        return;
    }
    let Some(row) = section.rows.get_mut(1 + usize::from(amount) % inner) else { return };
    let row = Arc::make_mut(row);
    let Some(cell) = row.cells.get_mut("X") else { return };
    if let Some(x) = cell.value.as_ref().and_then(Value::number) {
        cell.value = Value::from_number(x + (f64::from(amount >> 4) - 8.0) / 8.0);
    }
}

/// Op 48 (its `op % 16` is 0, a resize of a shape, which this op is dispatched ahead of): the first page's size, as `page.size` leaves it, the
/// shapes keeping their `PinX` and `PinY`. The byte `amount` says what changes (its low two bits: 0 and 1 the height, 2 the width, 3 both) and by how much (the rest, an
/// eighth of an inch each: 0 to 7.9), and `which`'s low bit the way: 0 taller or wider, 1 shorter or narrower (never below an inch). A page whose height changes puts
/// every shape of the file at another distance from its top, and a path a save keeps is **shifted with it**.
fn page_size(doc: &mut Document, (_, which, amount): (u8, u8, u8)) {
    let Some(page) = doc.pages.get_mut(0) else { return };
    let page = Arc::make_mut(page);
    let sheet = Arc::make_mut(&mut page.sheet);
    let by = f64::from(amount >> 2) / 8.0 * if which & 1 == 0 { 1.0 } else { -1.0 };
    for (cell, changes) in [("PageHeight", amount & 3 < 2 || amount & 3 == 3), ("PageWidth", amount & 3 >= 2)] {
        if let Some(c) = sheet.cells.get_mut(cell)
            && changes
            && let Some(v) = c.value.as_ref().and_then(Value::number)
        {
            c.value = Value::from_number((v + by).max(1.0));
        }
    }
}

/// Adds op 49: a shape of the first page given a look the writer says natively: a gradient (the layout, the angle, two to four stops and their colours from the bytes), a pattern
/// (any number, 0 to 255: a hatch for the lines, a flat mixture for a stipple, the solid fill for 1, nothing for the rest), or a shadow (its offset, blur, opacity and type), or all
/// of it taken off again. The kind is the byte `amount` modulo 6, the rest of it and `which` give the numbers. A save of the model says each as attributes and definitions of its own and
/// reads back the model's cells (the property of every save), and a second save through the new index is no edit.
fn fx_look(doc: &mut Document, (_, which, amount): (u8, u8, u8)) {
    let Some(page) = doc.pages.get_mut(0) else { return };
    let page = Arc::make_mut(page);
    let count = page.shapes.len();
    if count == 0 {
        return;
    }
    let Some(shape) = page.shapes.get_mut(usize::from(which) % count) else { return };
    let sheet = Arc::make_mut(&mut Arc::make_mut(shape).sheet);
    let number = |v: f64| Cell::constant(Value::from_number(v).unwrap_or_else(|| Value::from_text("0")));
    let colour = |k: u8| Cell::constant(Value::from_text(&format!("#{k:02x}{:02x}{:02x}", k.wrapping_mul(7), k.wrapping_mul(13))));
    let rest = amount / 6;
    match amount % 6 {
        0 | 1 => {
            sheet.cells.insert(CellName::new("FillPattern"), number(1.0));
            sheet.cells.insert(CellName::new("FillGradientEnabled"), number(1.0));
            // The layout 0 to 15 (14 and 15 are none ODF has a form for), and an angle in radians that may be any number.
            sheet.cells.insert(CellName::new("FillGradientDir"), number(f64::from(rest % 16)));
            sheet.cells.insert(CellName::new("FillGradientAngle"), number(f64::from(which) * 0.7 - 20.0));
            sheet.sections.retain(|s| s.name.as_str() != "FillGradient");
            let mut stops = Section::new("FillGradient", None);
            for k in 0..(2 + usize::from(rest % 3)) {
                let k8 = u8::try_from(k).unwrap_or(0);
                let mut row = Row { ix: Some(u32::from(k8)), ..Row::default() };
                row.cells.insert(CellName::new("GradientStopColor"), colour(amount.wrapping_add(k8.wrapping_mul(53))));
                row.cells.insert(CellName::new("GradientStopColorTrans"), number(if k == 1 { 0.25 } else { 0.0 }));
                // Places that may fall out of order, past 1, or not number at all, for the reader's and the writer's refusals.
                row.cells.insert(CellName::new("GradientStopPosition"), number(f64::from(k8) * f64::from(which % 7) / 6.0));
                stops.rows.push(Arc::new(row));
            }
            sheet.sections.push(Arc::new(stops));
        }
        2 => {
            sheet.cells.insert(CellName::new("FillPattern"), number(f64::from(rest) + f64::from(which % 2) * 100.0));
            sheet.cells.insert(CellName::new("FillForegnd"), colour(which));
            sheet.cells.insert(CellName::new("FillBkgnd"), colour(amount));
            sheet.cells.insert(CellName::new("FillGradientEnabled"), number(0.0));
        }
        3 | 4 => {
            sheet.cells.insert(CellName::new("ShdwPattern"), number(f64::from(rest % 3)));
            sheet.cells.insert(CellName::new("ShapeShdwType"), number(f64::from(rest / 3 % 4)));
            sheet.cells.insert(CellName::new("ShdwForegnd"), colour(which));
            sheet.cells.insert(CellName::new("ShdwForegndTrans"), number(f64::from(which % 21) / 20.0));
            sheet.cells.insert(CellName::new("ShapeShdwOffsetX"), number((f64::from(which) - 128.0) / 64.0).with_unit("IN"));
            sheet.cells.insert(CellName::new("ShapeShdwOffsetY"), number((f64::from(amount) - 128.0) / 64.0).with_unit("IN"));
            sheet.cells.insert(CellName::new("ShapeShdwBlur"), number(f64::from(rest) / 16.0).with_unit("IN"));
            sheet.cells.insert(CellName::new("ShapeShdwScaleFactor"), number(f64::from(1 + which % 3) / 2.0));
        }
        _ => {
            for name in ["FillGradientEnabled", "FillGradientDir", "FillGradientAngle", "ShdwPattern", "ShapeShdwType", "ShapeShdwBlur"] {
                sheet.cells.remove(name);
            }
            sheet.sections.retain(|s| s.name.as_str() != "FillGradient");
        }
    }
}

/// The corners of the route of each connector of page 0 on the page, by the shape's `ID`, hops and points on a leg taken out, and whether the route has a hop (a row that is
/// not a `LineTo`, but the first row's `MoveTo`: a hop of any style, the gap's among them); the target's own reading of the rows, in inches, y up. A connector whose cells do not read is left out.
fn route_corners(doc: &Document) -> std::collections::BTreeMap<u32, (bool, Vec<(f64, f64)>)> {
    let mut out = std::collections::BTreeMap::new();
    let Some(page) = doc.pages.get(0) else { return out };
    for shape in page.shapes.iter().filter(|s| s.sheet.cell("ShapeRouteStyle").is_some()) {
        let number = |c: &str| shape.sheet.cell(c).and_then(|cell| cell.value.as_ref()?.number()).filter(|v| v.is_finite());
        let (Some(bx), Some(by), Some(ex), Some(ey)) = (number("BeginX"), number("BeginY"), number("EndX"), number("EndY")) else { continue };
        let Some(section) = shape.sheet.sections.iter().find(|s| s.name.as_str() == "Geometry" && s.ix == Some(0)) else { continue };
        let (sn, cs) = (ey - by).atan2(ex - bx).sin_cos();
        let (mut corners, mut hop, mut ok): (Vec<(f64, f64)>, bool, bool) = (Vec::new(), false, true);
        for (at, row) in section.rows.iter().filter(|r| !r.del).enumerate() {
            // A hop is any row but a `LineTo`, other than the route's first, which is its `MoveTo`: the gap style's is a `MoveTo` in the middle of the route.
            hop |= !matches!((row.row_type.as_deref(), at), (Some("MoveTo"), 0) | (Some("LineTo"), 1..));
            let at = |c: &str| row.cells.get(c).and_then(|cell| cell.value.as_ref()?.number()).filter(|v| v.is_finite());
            let (Some(u), Some(v)) = (at("X"), at("Y")) else {
                ok = false;
                break;
            };
            let p = (bx + u * cs - v * sn, by + u * sn + v * cs);
            if corners.last().is_some_and(|q| (q.0 - p.0).abs() < 1e-4 && (q.1 - p.1).abs() < 1e-4) {
                continue;
            }
            while let [.., a, b] = corners[..] {
                let (ac, ab) = ((p.0 - a.0, p.1 - a.1), (b.0 - a.0, b.1 - a.1));
                let length = ac.0.hypot(ac.1);
                let (along, off) = ((ab.0 * ac.0 + ab.1 * ac.1) / length, (ab.0 * ac.1 - ab.1 * ac.0).abs() / length);
                if off < 1e-3 && along > 1e-4 && along < length - 1e-4 {
                    corners.pop();
                } else {
                    break;
                }
            }
            corners.push(p);
        }
        if ok {
            out.insert(shape.id, (hop, corners));
        }
    }
    out
}

/// SECNAME, ops 24 to 27 (not 20 to 23: 22 has an `op % 16` of 6, which the property takes for a text edit): a drag of a custom shape's handle, as a
/// recalculation leaves the sheet (the cached values move, the formulas stay). The handles' section is the model's `Control` (a formula spells it `Controls.`), and
/// the modifiers are the `User` rows `ODFMod<n>`. Op 24 moves the handle and the modifiers, 25 the handle and one modifier, 26 the handle alone (no modifier
/// follows it: the section is carried whole), 27 the modifiers alone. The value is from the byte.
fn handle_drag(doc: &mut Document, (op, which, amount): (u8, u8, u8)) {
    let Some(page) = doc.pages.get_mut(0) else { return };
    let page = Arc::make_mut(page);
    let at = usize::from(which) % page.shapes.len().max(1);
    let Some(shape) = page.shapes.get_mut(at) else { return };
    let sheet = Arc::make_mut(&mut Arc::make_mut(shape).sheet);
    for section in sheet.sections.iter_mut() {
        let name = section.name.as_str().to_owned();
        let section = Arc::make_mut(section);
        for (i, row) in section.rows.iter_mut().enumerate() {
            let row = Arc::make_mut(row);
            if name == "Control" && op != 27 {
                if let Some(cell) = row.cells.get_mut("X") {
                    cell.value = Value::from_number(f64::from(amount) / 64.0);
                }
            }
            let modifier = row.name.as_deref().is_some_and(|n| n.starts_with("ODFMod"));
            if name == "User" && modifier && op != 26 && (op != 25 || i == usize::from(amount) % 4) {
                if let Some(cell) = row.cells.get_mut("Value") {
                    cell.value = Value::from_number(f64::from(amount) * 21.0);
                }
            }
        }
    }
}

/// Makes the edit `(op, which, amount)` to the first page's top-level shapes, or (`op` 192 and up) to the pages.
fn edit(doc: &mut Document, (op, which, amount): (u8, u8, u8), next_id: u32) {
    if matches!(op, 16..=19) {
        style_or_words(doc, (op, which, amount));
        return;
    }
    if matches!(op, 24..=27) {
        handle_drag(doc, (op, which, amount));
        return;
    }
    if op == 46 {
        route_respell(doc, (op, which, amount));
        return;
    }
    if op == 47 {
        route_shift(doc, (op, which, amount));
        return;
    }
    if op == 48 {
        page_size(doc, (op, which, amount));
        return;
    }
    if op == 49 {
        fx_look(doc, (op, which, amount));
        return;
    }
    if matches!(op, 40..=45) {
        row_edit(doc, (op, which, amount));
        return;
    }
    if op >= 192 {
        return page_edit(doc, (op, which, amount));
    }
    let Some(page) = doc.pages.get_mut(0) else { return };
    let page = Arc::make_mut(page);
    let count = page.shapes.len();
    // Only a new rectangle goes on a page with no shape (op 9: a page that is an empty element is where the writer has to open it).
    if count == 0 && op % 16 != 9 {
        return;
    }
    let at = usize::from(which) % count.max(1);
    // An `ID` above every top-level one and the index's, which a new shape of an earlier edit of this run took.
    let next_id = next_id.max(page.shapes.iter().map(|s| s.id).max().unwrap_or(0).saturating_add(1));
    match op % 16 {
        // A line end on a shape: the number from the byte, 0 to 49 (those the writer has no art for are refused in words, 46 to 49 are no number), a size
        // class that may be one past what has a base, the weight of the line, and the begin too when the byte says so.
        14 => {
            let Some(shape) = page.shapes.get_mut(at) else { return };
            let sheet = Arc::make_mut(&mut Arc::make_mut(shape).sheet);
            let number = |v: f64| Value::from_number(v).unwrap_or_else(|| Value::from_text("0"));
            sheet.cells.insert(CellName::new("EndArrow"), Cell::constant(number(f64::from(amount % 50))));
            // The size class from the byte that names the shape too (0 to 7, so that 6 is written and 7 refused): the shapes are few.
            sheet.cells.insert(CellName::new("EndArrowSize"), Cell::constant(number(f64::from((which / 32 + amount / 50) % 8))));
            if amount & 1 == 1 {
                sheet.cells.insert(CellName::new("BeginArrow"), Cell::constant(number(f64::from(amount / 2 % 50))));
                sheet.cells.insert(CellName::new("BeginArrowSize"), Cell::constant(number(f64::from(which / 16 % 8))));
            }
            sheet.cells.insert(CellName::new("LineWeight"), Cell::constant(number(f64::from(amount % 20) / 72.0)));
        }
        // A text edit across runs and paragraphs: by the byte, a line feed added as a part of its own, a `cp` with a new row of cells and a few characters
        // after it, a `pp` with a new row, or the first line feed taken out (two paragraphs made one).
        15 => {
            let Some(shape) = page.shapes.get_mut(at) else { return };
            let shape = Arc::make_mut(shape);
            let ix = 1 + u32::from(amount % 4);
            let mut parts: Vec<TextPart> = shape.text.as_ref().map(|t| t.parts.clone()).unwrap_or_default();
            let place = usize::from(amount / 4) % (parts.len() + 1);
            let paragraph = amount % 3 == 2;
            let (section, mut cells): (&str, Vec<(&str, f64)>) = if paragraph {
                ("Paragraph", vec![("HorzAlign", f64::from(amount % 5)), ("SpLine", -1.0 - f64::from(amount % 3) / 4.0)])
            } else {
                ("Character", vec![("Style", f64::from(amount % 64)), ("Size", f64::from(8 + amount % 20) / 72.0)])
            };
            match amount % 4 {
                0 => parts.insert(place, TextPart::Chars(Arc::from("\n"))),
                1 | 2 => {
                    parts.insert(place, if paragraph { TextPart::Pp(ix) } else { TextPart::Cp(ix) });
                    parts.insert((place + 1).min(parts.len()), TextPart::Chars(Arc::from("xy")));
                }
                _ => {
                    if let Some(TextPart::Chars(c)) = parts.iter_mut().find(|p| matches!(p, TextPart::Chars(c) if c.contains('\n'))) {
                        *c = Arc::from(c.replacen('\n', "", 1));
                    }
                }
            }
            shape.text = Some(Arc::new(diacraft_doc::Text { parts, ..Default::default() }));
            let sheet = Arc::make_mut(&mut shape.sheet);
            let at_section = sheet.sections.iter().position(|s| s.name.as_str() == section).unwrap_or_else(|| {
                sheet.sections.push(Arc::new(Section::new(section, None)));
                sheet.sections.len() - 1
            });
            if let Some(sec) = sheet.sections.get_mut(at_section) {
                let sec = Arc::make_mut(sec);
                let mut row = Row { ix: Some(ix), ..Row::default() };
                // A list's row: the bullet from the byte (8 is one the writer has none for), and the cells of its list style.
                if paragraph && amount % 2 == 0 {
                    cells.push(("Bullet", f64::from(amount / 2 % 9)));
                    cells.push(("IndLeft", f64::from(amount % 5) / 10.0));
                    cells.push(("IndFirst", -f64::from(amount % 3) / 10.0));
                    if amount % 3 == 0 {
                        cells.push(("BulletFontSize", f64::from(8 + amount % 6) / 72.0));
                    }
                }
                for (name, v) in cells {
                    row.cells.insert(CellName::new(name), Cell::constant(Value::from_number(v).unwrap_or_else(|| Value::from_text("0"))));
                }
                // A font by name that the file declares or does not (a family that is a name taken, one with a quote), or the number 0, and a bullet's own text.
                if !paragraph && amount % 5 < 3 {
                    let families = ["Arial", "Nowhere Sans", "F", "F 2", "O'Neil", "0", "Papyrus"];
                    row.cells.insert(CellName::new("Font"), Cell::constant(Value::from_text(families[usize::from(amount / 5) % families.len()])));
                }
                if paragraph && row.cells.contains_key("Bullet") && amount % 4 == 0 {
                    // A bullet of one character or of two (refused in words), and a face by name or the text 0 (the document's own, which names none).
                    let bullet = if amount % 8 == 4 { "ab" } else { "\u{2013}" };
                    let font = if amount % 3 == 0 { "0" } else { "Symbol" };
                    row.cells.insert(CellName::new("BulletStr"), Cell::constant(Value::from_text(bullet)));
                    row.cells.insert(CellName::new("BulletFont"), Cell::constant(Value::from_text(font)));
                }
                sec.rows.push(Arc::new(row));
            }
        }
        // A turn: the angle in radians from the byte.
        10 => {
            let Some(shape) = page.shapes.get_mut(at) else { return };
            let sheet = Arc::make_mut(&mut Arc::make_mut(shape).sheet);
            let angle = f64::from(amount) * 0.0245;
            sheet
                .cells
                .insert(CellName::new("Angle"), Cell::constant(Value::from_number(angle).unwrap_or_else(|| Value::from_text("0"))).with_unit("RAD"));
        }
        // A flip, on either axis or both, with the turn beside it (milestone-1 class F5).
        11 => {
            let Some(shape) = page.shapes.get_mut(at) else { return };
            let sheet = Arc::make_mut(&mut Arc::make_mut(shape).sheet);
            for (name, on) in [("FlipX", amount & 1 != 0), ("FlipY", amount & 2 != 0)] {
                if on {
                    sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text("1")));
                }
            }
        }
        // A new connector from this shape to the next one, glued at both ends (class F1: a part of ODF 1.0 or 1.1 has no `svg:d` and no `xml:id`).
        12 => {
            let (Some(from), Some(to)) = (page.shapes.get(at).map(|s| s.id), page.shapes.get((at + 1) % count).map(|s| s.id)) else { return };
            let mut shape = Shape::new(next_id);
            let sheet = Arc::make_mut(&mut shape.sheet);
            let (x1, y1, x2, y2) = (1.0, 1.0 + f64::from(amount) / 32.0, 3.0 + f64::from(amount) / 16.0, 2.0);
            let length = ((x2 - x1) * (x2 - x1) + (y2 - y1) * (y2 - y1)).sqrt();
            let number = |v: f64| Value::from_number(v).unwrap_or_else(|| Value::from_text("0"));
            for (name, v) in [
                ("BeginX", x1),
                ("BeginY", y1),
                ("EndX", x2),
                ("EndY", y2),
                ("Width", length),
                ("Height", 0.0),
                ("LocPinX", length / 2.0),
                ("LocPinY", 0.0),
            ] {
                sheet.cells.insert(CellName::new(name), Cell::constant(number(v)).with_unit("IN"));
            }
            sheet.cells.insert(CellName::new("PinX"), Cell::constant(number((x1 + x2) / 2.0)).with_unit("IN"));
            sheet.cells.insert(CellName::new("PinY"), Cell::constant(number((y1 + y2) / 2.0)).with_unit("IN"));
            sheet.cells.insert(CellName::new("Angle"), Cell::constant(number((y2 - y1).atan2(x2 - x1))));
            sheet.cells.insert(CellName::new("ShapeRouteStyle"), Cell::constant(number(1.0)));
            sheet.cells.insert(CellName::new("ConLineRouteExt"), Cell::constant(number(1.0)));
            let mut geometry = Section::new("Geometry", Some(0));
            for (i, (kind, x)) in [("MoveTo", 0.0), ("LineTo", length)].into_iter().enumerate() {
                let mut row = Row { row_type: Some(Arc::from(kind)), ix: Some(i as u32 + 1), ..Row::default() };
                row.cells.insert(CellName::new("X"), Cell::constant(number(x)).with_unit("IN"));
                row.cells.insert(CellName::new("Y"), Cell::constant(number(0.0)).with_unit("IN"));
                geometry.rows.push(Arc::new(row));
            }
            sheet.sections.push(Arc::new(geometry));
            page.shapes.push(Arc::new(shape));
            let mut rows: Vec<Connect> = page.connects.iter().cloned().collect();
            for (begin, target) in [(true, from), (false, to)] {
                rows.push(Connect {
                    from_sheet: next_id,
                    from_cell: Some(Arc::from(if begin { "BeginX" } else { "EndX" })),
                    from_part: Some(if begin { 9 } else { 12 }),
                    to_sheet: target,
                    to_cell: Some(Arc::from("PinX")),
                    to_part: Some(if amount & 1 == 0 { 3 } else { 100 + u32::from(amount % 4) }),
                    ..Connect::default()
                });
            }
            page.connects = Connects::from_vec(rows);
        }
        // A colour on a shape that may hold a gradient, a hatch or a bitmap, or no fill mode at all: the gradient flag turned off (class F4).
        13 => {
            let Some(shape) = page.shapes.get_mut(at) else { return };
            let sheet = Arc::make_mut(&mut Arc::make_mut(shape).sheet);
            sheet.cells.insert(CellName::new("FillGradientEnabled"), Cell::constant(Value::from_text("0")));
            sheet.cells.insert(
                CellName::new("FillForegnd"),
                Cell::constant(Value::from_text(&format!("#{amount:02X}{:02X}{:02X}", amount.wrapping_mul(5), amount.wrapping_mul(11)))),
            );
        }
        0 | 3 => {
            let Some(shape) = page.shapes.get_mut(at) else { return };
            let shape = Arc::make_mut(shape);
            let sheet = Arc::make_mut(&mut shape.sheet);
            let name = if op % 10 == 0 { "PinX" } else { "Width" };
            if let Some(cell) = sheet.cells.get_mut(name)
                && let Some(v) = cell.value.as_ref().and_then(Value::number)
            {
                cell.value = Value::from_number(v + f64::from(amount) / 7.0);
            }
        }
        1 => {
            page.shapes.remove(at);
        }
        2 => {
            if let Some(shape) = page.shapes.remove(at) {
                page.shapes.push(shape);
            }
        }
        // A restyle: a colour from the bytes, or the fill's pattern toggled; and a rename and a link to a master, which are tier 2 on a shape of the
        // file and make the save stamp the fingerprint of the file's own `draw:name` and outline: a save that is refused by the check of its own bytes is a crash.
        4 | 5 => {
            let Some(shape) = page.shapes.get_mut(at) else { return };
            let shape = Arc::make_mut(shape);
            if amount & 0x80 != 0 {
                shape.name = Some(Arc::from(format!("N{amount}")));
            }
            if amount & 0x40 != 0 {
                shape.master = Some(u32::from(amount % 3) + 1);
            }
            let sheet = Arc::make_mut(&mut shape.sheet);
            if op % 10 == 4 {
                let colour = format!("#{amount:02X}{:02X}{:02X}", amount.wrapping_mul(3), amount.wrapping_mul(7));
                sheet.cells.insert(CellName::new("FillForegnd"), Cell::constant(Value::from_text(&colour)));
            } else if let Some(cell) = sheet.cells.get_mut("FillPattern") {
                cell.value = Value::from_number(f64::from(amount % 2));
            }
        }
        // A retext: a letter at the end of the first characters of the shape's text.
        6 => {
            let Some(shape) = page.shapes.get_mut(at) else { return };
            let shape = Arc::make_mut(shape);
            if let Some(text) = shape.text.as_mut() {
                let text = Arc::make_mut(text);
                if let Some(TextPart::Chars(c)) = text.parts.iter_mut().find(|p| matches!(p, TextPart::Chars(_))) {
                    *c = Arc::from(format!("{c}{}", char::from(b'a' + amount % 26)));
                }
            }
        }
        // A page's height.
        7 => {
            let sheet = Arc::make_mut(&mut page.sheet);
            if let Some(cell) = sheet.cells.get_mut("PageHeight")
                && let Some(v) = cell.value.as_ref().and_then(Value::number)
            {
                cell.value = Value::from_number(v + f64::from(amount % 16) / 4.0);
            }
        }
        // A point of the first row of the first outline of a shape: a share of the frame.
        8 => {
            let Some(shape) = page.shapes.get_mut(at) else { return };
            let sheet = Arc::make_mut(&mut Arc::make_mut(shape).sheet);
            if let Some(section) = sheet.sections.iter_mut().find(|s| s.name.as_str() == "Geometry")
                && let Some(row) = Arc::make_mut(section).rows.get_mut(1)
            {
                let row = Arc::make_mut(row);
                row.cells.insert(CellName::new("X"), Cell::constant(Value::from_text(&format!("{}", f64::from(amount) / 8.0))));
            }
        }
        // A new rectangle after the last shape (op 9).
        _ => {
            let mut shape = Shape::new(next_id);
            let sheet = Arc::make_mut(&mut shape.sheet);
            for (name, v) in
                [("Width", 1.0), ("Height", 1.0), ("LocPinX", 0.5), ("LocPinY", 0.5), ("PinX", f64::from(amount) / 16.0 + 1.0), ("PinY", 5.0)]
            {
                sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_number(v).unwrap_or_else(|| Value::from_text("0"))));
            }
            let mut geometry = Section::new("Geometry", Some(0));
            for (i, kind) in ["MoveTo", "LineTo", "LineTo", "LineTo", "LineTo"].iter().enumerate() {
                let mut row = Row { row_type: Some(Arc::from(*kind)), ix: Some(i as u32 + 1), ..Row::default() };
                let (x, y) = [(0, 0), (1, 0), (1, 1), (0, 1), (0, 0)][i];
                for (cell, share) in [("X", x), ("Y", y)] {
                    let axis = if cell == "X" { "Width" } else { "Height" };
                    let formula = if share == 1 { Some(axis) } else { None };
                    row.cells.insert(CellName::new(cell), Cell::read(formula, Some(&share.to_string()), Some("IN"), None, &mut 100));
                }
                geometry.rows.push(Arc::new(row));
            }
            sheet.sections.push(Arc::new(geometry));
            // The four connection points at the middles of the sides, as DiaCraft's basic shapes have them: without them a new shape is not tier 1, and the
            // writer refuses it (a rectangle that is always refused explores nothing of the way a new shape is written).
            let mut connection = Section::new("Connection", None);
            for (i, (xf, xv, yf, yv)) in [
                (Some("Width*0.5"), 0.5, Some("Height"), 1.0),
                (None, 0.0, Some("Height*0.5"), 0.5),
                (Some("Width*0.5"), 0.5, None, 0.0),
                (Some("Width"), 1.0, Some("Height*0.5"), 0.5),
            ]
            .into_iter()
            .enumerate()
            {
                let mut row = Row { ix: Some(i as u32), ..Row::default() };
                row.cells.insert(CellName::new("X"), Cell::read(xf, Some(&xv.to_string()), Some("IN"), None, &mut 100));
                row.cells.insert(CellName::new("Y"), Cell::read(yf, Some(&yv.to_string()), Some("IN"), None, &mut 100));
                connection.rows.push(Arc::new(row));
            }
            sheet.sections.push(Arc::new(connection));
            // A new picture, when the high bit of the amount says so. The type is the one of eight the byte `which` picks (the five the reader draws, a Windows
            // metafile, an SVG, and nothing: those the reader does not draw are refused in words); the bytes after the signature come from the amount; bit 6 shows the
            // bytes of the last new picture again (in an `Arc` of its own, or the same `Arc` when bit 5 is set); bit 4 takes the outline away, as a picture has none.
            if amount & 0x80 != 0 {
                shape.kind = ShapeType::Foreign;
                let earlier =
                    page.shapes.iter().rev().find_map(|s| s.foreign.as_ref().and_then(|f| f.picture.clone()).filter(|m| &*m.part == "fuzz-picture"));
                let media = match earlier {
                    Some(previous) if amount & 0x40 != 0 && amount & 0x20 != 0 => previous,
                    Some(previous) if amount & 0x40 != 0 => Arc::new(Media {
                        part: Arc::from("fuzz-picture"),
                        content_type: Arc::from("image/png"),
                        bytes: Arc::from(&previous.bytes[..]),
                    }),
                    _ => {
                        let head: &[u8] = match which % 8 {
                            0 => &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
                            1 => &[0xFF, 0xD8, 0xFF],
                            2 => b"GIF89a",
                            3 => b"BM",
                            4 => &[0; 40],
                            5 => &[0xD7, 0xCD, 0xC6, 0x9A],
                            6 => b"<svg",
                            _ => b"",
                        };
                        let mut bytes = head.to_vec();
                        if which % 8 == 4 {
                            bytes.extend_from_slice(b" EMF");
                        }
                        bytes.extend((0..usize::from(amount % 32)).map(|i| (i as u8).wrapping_mul(which | 1).wrapping_add(amount)));
                        Arc::new(Media { part: Arc::from("fuzz-picture"), content_type: Arc::from("image/png"), bytes: Arc::from(bytes) })
                    }
                };
                shape.foreign = Some(Arc::new(ForeignData {
                    attrs: RawAttrs::from_pairs([("ForeignType", "Bitmap"), ("CompressionType", "PNG")]),
                    rel_id: Some(Arc::from("rId1")),
                    picture: Some(media),
                    ..ForeignData::default()
                }));
                if amount & 0x10 != 0 {
                    Arc::make_mut(&mut shape.sheet).sections.retain(|s| s.name.as_str() != "Geometry");
                }
            }
            page.shapes.push(Arc::new(shape));
        }
    }
}

/// Whether a part that declares `version` is one of ODF 1.0 or 1.1, or declares none: the writer's own reading of a version (`write::scan::is_legacy`), which the target
/// cannot call. Such a part has no place for a connector's route (`svg:d`, `svg:viewBox`) and no `draw:line-skew` once a route is written, so the property of the route
/// chain does not hold of it (a save that routes a connector there says so in a note instead: `SaveNote::RouteDroppedForVersion`).
fn is_legacy_version(version: Option<&str>) -> bool {
    let Some(version) = version else { return true };
    let mut numbers = version.trim().split('.');
    let major = numbers.next().and_then(|n| n.parse::<u32>().ok());
    let minor = numbers.next().map_or(Some(0), |n| n.parse::<u32>().ok());
    match (major, minor) {
        (Some(major), Some(minor)) => (major, minor) < (1, 2),
        _ => true,
    }
}

/// The most a coordinate of a route may be for the property of the route chain to hold: a corner this far out is not read back to within the property's tolerance (a
/// double has about fifteen digits and the route is written in hundredths of a millimetre), and the writer says it could not write such a route.
const MOST_COORDINATE: f64 = 1e9;

/// The `ID`s of the connectors of the first page that the notes of a save name as ones whose route it did not write as the model has it (`RouteNotWritten`, and a route a
/// part of ODF 1.0 or 1.1 has no place for): the save says so, and a connector it says that of keeps the file's own route, or none.
fn noted_routes(saved: &Written) -> Vec<u32> {
    saved
        .notes
        .iter()
        .filter_map(|note| match note {
            SaveNote::RouteNotWritten { page: 0, shape } | SaveNote::RouteDroppedForVersion { page: 0, shape } => Some(*shape),
            _ => None,
        })
        .collect()
}

/// The start tags of the `draw:connector` elements of a part, in the order they stand.
fn connector_tags(part: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = part.get(from..).and_then(|rest| rest.windows(13).position(|w| w == b"<draw:connect")) {
        let start = from + at;
        let end = part.get(start..).and_then(|rest| rest.iter().position(|b| *b == b'>')).map_or(part.len(), |e| start + e + 1);
        out.push(part.get(start..end).unwrap_or_default().to_vec());
        from = end.max(start + 1);
    }
    out
}

/// The bytes of the attribute `name` of a start tag, between its double quotes (`None` for one the tag does not have, or spells with single quotes: the same on both sides).
fn attribute(tag: &[u8], name: &str) -> Option<Vec<u8>> {
    let key = format!(" {name}=\"");
    let at = tag.windows(key.len()).position(|w| w == key.as_bytes())? + key.len();
    let rest = tag.get(at..)?;
    Some(rest.get(..rest.iter().position(|b| *b == b'"')?)?.to_vec())
}

/// The bytes of the first `draw:page` of a content part (its start tag to the next page's, or the end): the page the target's size edit changes.
fn first_page(content: &[u8]) -> &[u8] {
    let starts = |from: usize| {
        let mut at = from;
        while let Some(found) = content.get(at..).and_then(|rest| rest.windows(10).position(|w| w == b"<draw:page")) {
            let start = at + found;
            if content.get(start + 10).is_some_and(|b| matches!(b, b' ' | b'>' | b'/' | b'\t' | b'\n' | b'\r')) {
                return Some(start);
            }
            at = start + 1;
        }
        None
    };
    let Some(first) = starts(0) else { return &[] };
    let end = starts(first + 1).unwrap_or(content.len());
    content.get(first..end).unwrap_or_default()
}

/// The text of an attribute's raw bytes with its references resolved (the five named ones and numeric ones), as a start tag's value reads without the normalisation of white
/// space; `None` for a reference of another name or one that is no character, which this does not decode.
fn unreference(raw: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(raw).ok()?;
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(rest.get(..at)?);
        let tail = rest.get(at + 1..)?;
        let end = tail.find(';')?;
        let name = tail.get(..end)?;
        match name {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ => {
                let digits = name.strip_prefix('#')?;
                let code = match digits.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                    None => digits.parse::<u32>().ok()?,
                };
                out.push(char::from_u32(code)?);
            }
        }
        rest = tail.get(end + 1..)?;
    }
    out.push_str(rest);
    Some(out)
}

/// The model of any package, **written from nothing** (`write_with(doc, None, ..)`): it ends as bytes that read again, in a package that states its kind, with the pages
/// the model has that are not backgrounds, and that are the same bytes when the model is written a second time, or as an error of a known class that is no fault of the writer
/// (a self-check or a mismatch is one, but for a part the input's own bytes make unscannable). A third of the writes say the content is as it was read.
fn model_write(doc: &Document, input: &[u8], limits: &Limits) {
    let options = WriteOptions {
        thumbnail: (input.len() % 4 != 0).then(|| vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
        content_unchanged: input.len() % 3 == 0,
        ..WriteOptions::default()
    };
    match write_with(doc, None, &options) {
        Err(e) => {
            assert!(CLASSES.contains(&e.class()) || is_unscannable_part(&e, input), "an unknown class from the model writer: {e:?}");
            assert!(
                !matches!(e, diacraft_odg::WriteError::Mismatch(_) | diacraft_odg::WriteError::SelfCheck(_)) || is_unscannable_part(&e, input),
                "a fault of the model writer: {e:?}"
            );
        }
        Ok(written) => {
            assert!(sniff(&written.bytes).is_some(), "the package the model writer made does not say what it is");
            let (back, _) = read(&written.bytes, limits).expect("the package the model writer made reads");
            unpaired_id(&written.bytes, limits);
            let pages = |d: &Document| d.pages.iter().filter(|p| !p.background).count();
            assert_eq!(pages(&back), pages(doc), "the pages the model has are the pages the file has");
            let again = write_with(doc, None, &options).expect("a second write of a model written");
            assert_eq!(again.bytes, written.bytes, "the same model gives the same bytes");
            // Every fill image's picture the model holds is an entry of the package, and the file reads back with the style sheets the model has.
            if let Some(odf) = doc.ext.get().map(|d| &d.odf) {
                let package = Package::read(&written.bytes, limits).expect("the package the model writer made is a package");
                for picture in odf.definitions.iter().filter_map(|d| d.bytes.as_ref()) {
                    assert!(package.parts().any(|p| package.part(p) == Some(&picture[..])), "a fill image's picture is not in the package");
                }
            }
            assert_eq!(back.styles.len(), doc.styles.len(), "the style sheets the model has are the style sheets the file has");
        }
    }
}

/// **A shape the model writer writes with a `draw:id` carries an `xml:id` of the same value** (ODF 1.4 takes `draw:id` only beside one: the schema's
/// `common-draw-id-attlist`). Judged on `content.xml` of what the model writer made, for the shapes the writer itself writes: the children of a `draw:page` that are shapes of ODF's drawing
/// namespace, and the members of those that are groups, down. **Not judged**: what the file's markup holds whole (a shape inside a hyperlink wrapper `draw:a`, a text box or any other
/// element), a page's own `draw:id`, a glue point's (its number), and any part whose root, or an element on the way, binds the prefix `draw` to another namespace.
fn unpaired_id(bytes: &[u8], limits: &Limits) {
    const DRAWING: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
    const SHAPES: [&str; 12] =
        ["rect", "ellipse", "circle", "line", "polygon", "polyline", "path", "regular-polygon", "custom-shape", "frame", "connector", "g"];
    let Ok(package) = Package::read(bytes, limits) else { return };
    let Some(content) = package.part("content.xml") else { return };
    let Ok(root) = diacraft_container::xml::parse(content) else { return };
    let rebinds = |el: &diacraft_container::xml::El| el.attrs.iter().any(|(k, v)| k == "xmlns:draw" && v != DRAWING);
    // Each element, and whether it stands among the shapes of a page (a child of a page, or a member of a group among them).
    let mut stack = vec![(&root, false)];
    while let Some((el, among)) = stack.pop() {
        if rebinds(el) {
            continue;
        }
        let local = el.name.strip_prefix("draw:");
        if among && local.is_some_and(|l| SHAPES.contains(&l)) {
            let value = |name: &str| el.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
            if let Some(id) = value("draw:id") {
                assert_eq!(value("xml:id"), Some(id), "a shape written with a draw:id that has no xml:id of the same value beside it: {}", el.name);
            }
        }
        // Below a page, and below a group among its shapes, the children are shapes; below any other element of a page they are what the file kept.
        let below = el.name == "draw:page" || (among && local == Some("g"));
        let walk_on = below || !among;
        if !walk_on {
            continue;
        }
        for child in el.children.iter() {
            if let diacraft_container::xml::Node::El(e) = child {
                stack.push((e, below));
            }
        }
    }
}

/// The model of any package with some of the fields `meta.xml` has an element for set to an empty text (a VSDX file's empty `dc:subject` and the like), the fields chosen by two bytes of the input,
/// **written from nothing**: the write ends as bytes or as an error of a known class that is no fault of the writer, and the file reads back with exactly the fields the model has (an empty field empty, none none,
/// a text as it was), and a second write of what was read is the same drawing.
fn empty_fields(doc: &Document, input: &[u8], limits: &Limits) {
    let (Some(which), Some(pick)) = (input.first(), input.last()) else { return };
    if pick & 3 != 3 {
        return;
    }
    let mut model = doc.clone();
    let props = Arc::make_mut(&mut model.props);
    for (bit, slot) in
        [&mut props.subject, &mut props.creator, &mut props.last_modified_by, &mut props.keywords, &mut props.description, &mut props.language]
            .into_iter()
            .enumerate()
    {
        if (which >> bit) & 1 == 1 && slot.is_none() {
            *slot = Some(Arc::from(""));
        }
    }
    let options = WriteOptions::default();
    match write_with(&model, None, &options) {
        Err(e) => {
            assert!(CLASSES.contains(&e.class()) || is_unscannable_part(&e, input), "an unknown class from the model writer: {e:?}");
            assert!(
                !matches!(e, diacraft_odg::WriteError::Mismatch(_) | diacraft_odg::WriteError::SelfCheck(_)) || is_unscannable_part(&e, input),
                "a fault of the model writer with empty fields: {e:?}"
            );
        }
        Ok(written) => {
            let (back, _) = read(&written.bytes, limits).expect("the package the model writer made reads");
            let fields = |d: &Document| {
                let p = &d.props;
                [p.subject.clone(), p.creator.clone(), p.last_modified_by.clone(), p.keywords.clone(), p.description.clone(), p.language.clone()]
            };
            assert_eq!(fields(&back), fields(&model), "the fields of the properties come back as the model has them");
            let (twice, _) = read(&write_with(&back, None, &options).expect("a second write of the model read").bytes, limits).expect("reads");
            assert_eq!(fields(&twice), fields(&model), "and again");
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let limits = Limits::default();
    let (bytes, edits, flags, kind): (Vec<u8>, Vec<(u8, u8, u8)>, Flags, Kind) = match data.split_first() {
        Some((255, rest)) => (rest.to_vec(), Vec::new(), Flags(0), Kind(0)),
        // A package as it is, asked for a drawing (kind 1: bit 1 of the kind) or for a template (kind 2: bit 2).
        Some((254, rest)) => (rest.to_vec(), Vec::new(), Flags(0), Kind(2)),
        Some((253, rest)) => (rest.to_vec(), Vec::new(), Flags(0), Kind(4)),
        Some((n, rest)) => {
            let kind = Kind(*n / 9);
            let n = usize::from(*n % 9);
            let (flags, rest) = rest.split_first().map_or((0, &[][..]), |(f, r)| (*f, r));
            let flags = Flags(flags);
            let (ops, body) = rest.split_at(rest.len().min(n * 3));
            let edits = ops.chunks_exact(3).map(|c| (c[0], c[1], c[2])).collect();
            match if kind.flat() { flat(body, flags, kind) } else { package(body, flags, kind) } {
                Some(bytes) => (bytes, edits, flags, kind),
                None => return,
            }
        }
        None => return,
    };
    let Ok((mut doc, report)) = read(&bytes, &limits) else { return };
    model_write(&doc, &bytes, &limits);
    empty_fields(&doc, &bytes, &limits);
    // A package with an entry nothing reads that is damaged (a CRC or a size that is wrong, left out of the model and said) is read, and is no package for the package
    // layer, which refuses it: the checks below read the original and the saved package through it (and a save copies the damaged entry), so they are not made for it.
    if Form::of(&bytes) == Form::Package && Package::read(&bytes, &limits).is_err() {
        return;
    }
    let next_id = report.index.next_id(0).unwrap_or(1);
    // A run of text edits only (ops 6 and 15), which move zero-width elements and never take a shape out.
    let retext_only = !edits.is_empty() && edits.iter().all(|(op, _, _)| matches!(op % 16, 6 | 15));
    // A run of route respells alone (op 46): no connector's start tag changes in the save, nor in a second one with the hop the other way.
    let route_only = !edits.is_empty() && edits.iter().all(|(op, _, _)| *op == 46);
    // ... and a run of ops 46, 47 and 48: the corners may be other than the file's, and a route with no hop is the file's after the save (of the review), **in the
    // page as the save leaves it** (op 48 sets the page's size, and a save that changes its height changes the frame the corners kept for a route are in).
    let route_chain =
        !edits.is_empty() && edits.iter().all(|(op, _, _)| matches!(op, 46..=48)) && edits.iter().any(|(op, _, _)| matches!(op, 47 | 48));
    // The property is of a part of ODF 1.2 or later: an older part (or one with no version) has no place for the route a save writes.
    let legacy_part = flags.legacy() || is_legacy_version(report.office_version.as_deref());
    let page_sizes = edits.iter().any(|(op, _, _)| *op == 48);
    // A run of page-size edits alone: no connector is touched, so each keeps its own path through a change of the page's height.
    let page_only = !edits.is_empty() && edits.iter().all(|(op, _, _)| *op == 48);
    let page_ops = edits.iter().any(|e| e.0 >= 192);
    // The page edits again, in the other order and on other pages, for a second round of saves from the index the first one gave (a rename taken back, a page
    // added and renamed, a page moved again).
    let second_round: Vec<(u8, u8, u8)> = edits.iter().filter(|e| e.0 >= 192).rev().map(|&(op, which, amount)| (op, amount, which)).collect();
    let second_routes: Vec<(u8, u8, u8)> = if route_only || route_chain { edits.iter().copied().filter(|e| e.0 == 46).collect() } else { Vec::new() };
    // Op 48 again, for the third save of the route property (a later change of the page's size, with the arguments the other way round), and the route ops and the page
    // ops of the run apart, for the chain that keeps a route in a save that leaves the page as it is and carries it across the save that changes it.
    let page_again: Vec<(u8, u8, u8)> = if route_chain { edits.iter().copied().filter(|e| e.0 == 48).collect() } else { Vec::new() };
    let route_edits: Vec<(u8, u8, u8)> =
        if route_chain && page_sizes { edits.iter().copied().filter(|e| matches!(e.0, 46 | 47)).collect() } else { Vec::new() };
    for e in edits {
        edit(&mut doc, e, next_id);
    }
    // A package is saved as a flat document when the kind's bit 4 says so; a flat drawing is saved flat.
    let was_flat = Form::of(&bytes) == Form::Flat;
    let to_flat = kind.to_flat() && !was_flat;
    // A third of the saves are a first save (`WriteOptions::first_save`: a package that has no thumbnail gains the one passed, edited or not).
    let first_save = bytes.len() % 3 == 0;
    let options = WriteOptions {
        thumbnail: Some(vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
        first_save,
        variant: kind.asked(),
        form: to_flat.then_some(Form::Flat),
        ..WriteOptions::default()
    };
    let own = sniff(&bytes);
    // The kind the save writes: the one asked for, else the package's own. A package that says nothing and was asked for nothing says nothing.
    let wanted = kind.asked().or(own);
    let retyped = kind.asked().is_some() && kind.asked() != own;
    // A save that asks for the kind the package says it is writes `mimetype` as R2 asks when it is not, edited or not.
    let was_regular = Package::read(&bytes, &limits).is_ok_and(|p| p.mimetype_regular());
    let repaired = kind.asked().is_some() && !retyped && !was_regular;
    match write_with(&doc, Some(&OriginalPackage { bytes: &bytes, index: &report.index }), &options) {
        Err(e) => {
            assert!(CLASSES.contains(&e.class()) || is_unscannable_part(&e, &bytes), "an unknown class: {e:?}");
            let _shown = e.to_string();
        }
        Ok(saved) => {
            assert_eq!(saved.index.pages(), doc.pages.len());
            assert!(read(&saved.bytes, &limits).is_ok(), "the bytes written do not read");
            // A save that changed the pages reads back with the pages the model has, in its order, under its names and its `ID`s.
            if page_ops && !was_flat && !to_flat {
                let (back, _) = read(&saved.bytes, &limits).expect("the bytes written read");
                let names = |d: &Document| d.pages.iter().map(|p| p.name.to_string()).collect::<Vec<_>>();
                let ids = |d: &Document| d.pages.iter().map(|p| p.id).collect::<Vec<_>>();
                assert_eq!(names(&back), names(&doc), "the pages read back under the names the model has");
                assert_eq!(ids(&back), ids(&doc), "and under its IDs");
            }
            // A flat drawing is saved flat, and a package asked to be one is; an unedited flat save of the kind it is, or of no kind, is the bytes;
            // DiaCraft's extension reads back as the model has it.
            if was_flat || to_flat {
                assert_eq!(Form::of(&saved.bytes), Form::Flat, "a flat drawing is saved flat, and a package asked for a flat one is");
                assert_eq!(
                    read(&saved.bytes, &limits).map(|(d, _)| d.diacraft).ok(),
                    Some(doc.diacraft.clone()),
                    "the extension reads back as the model has it"
                );
            }
            // A first save that is no edit gives a package that had no thumbnail the one it was passed, with a manifest row, so it is not "the original's
            // parts" below; and any first save of a package leaves it a thumbnail, the one it was passed or the one it had (a flat drawing has none to leave).
            let gains_thumbnail = first_save
                && !saved.edited
                && !was_flat
                && !to_flat
                && Package::read(&bytes, &limits).is_ok_and(|p| p.part("Thumbnails/thumbnail.png").is_none());
            if first_save && !was_flat && !to_flat {
                assert!(
                    Package::read(&saved.bytes, &limits).is_ok_and(|p| p.part("Thumbnails/thumbnail.png").is_some()),
                    "the first save of a package leaves it a thumbnail"
                );
            }
            // The checks of an unedited save below are switched off for a first save that gives the package its thumbnail, since
            // it is not "the original's parts" any more. What it **is** is asserted here instead: the one entry added is the thumbnail, with the picture that was
            // passed, and its one manifest row (none when the manifest listed it already); the parts that change are the manifest and `mimetype` and no other; every
            // other part is the original's, byte for byte.
            if gains_thumbnail && let (Ok(now), Ok(was)) = (Package::read(&saved.bytes, &limits), Package::read(&bytes, &limits)) {
                const THUMBNAIL: &str = "Thumbnails/thumbnail.png";
                let statements = |name: &str| name == "mimetype" || name == diacraft_odg::package::MANIFEST_PATH;
                let parts = |p: &Package| {
                    let mut names: Vec<String> = p.parts().map(str::to_owned).collect();
                    names.sort();
                    names
                };
                let mut expected = parts(&was);
                expected.push(THUMBNAIL.to_owned());
                expected.sort();
                // (`mimetype` may be new too: a package that had none, and a save that asked for a kind.)
                let without_mimetype = |mut names: Vec<String>| {
                    names.retain(|n| n != "mimetype");
                    names
                };
                assert_eq!(without_mimetype(parts(&now)), without_mimetype(expected), "the one entry a first save adds is the thumbnail");
                assert_eq!(now.part(THUMBNAIL), options.thumbnail.as_deref(), "and it is the picture that was passed");
                for name in parts(&now) {
                    if name != THUMBNAIL && !statements(&name) {
                        assert_eq!(now.part(&name), was.part(&name), "a first save changes no part but the manifest and mimetype: {name}");
                    }
                }
                let paths = |p: &Package| p.manifest().entries.iter().map(|e| e.full_path.clone()).collect::<Vec<_>>();
                let mut rows = paths(&was);
                if was.manifest().entry(THUMBNAIL).is_none() {
                    rows.push(THUMBNAIL.to_owned());
                }
                assert_eq!(paths(&now), rows, "the manifest gains the thumbnail's row and no other, none when it listed it already");
            }
            // (Asking for a kind states it, even when the root says nothing a program knows, which `sniff` reads as a drawing.)
            if was_flat && !saved.edited && kind.asked().is_none() {
                assert_eq!(saved.bytes, bytes, "an unedited save of a flat drawing is the bytes");
            }
            // The zero-width elements a text edit was given are all in the file it made, as often as before. Only for a page body with no comment, no
            // CDATA and no processing instruction, where a literal `<text:bookmark` is a tag and nothing else.
            // (a flat document is the one part, so its bytes are what is counted in; a package's is `content.xml`.)
            let content_of = |file: &[u8]| -> Option<Vec<u8>> {
                match Form::of(file) {
                    Form::Flat => Some(file.to_vec()),
                    Form::Package => Package::read(file, &limits).ok().and_then(|p| p.part("content.xml").map(<[u8]>::to_vec)),
                }
            };
            if retext_only
                && !to_flat
                && let (Some(now), Some(was)) = (content_of(&saved.bytes), content_of(&bytes))
                && let (now, was) = (&now[..], &was[..])
                && !was.windows(4).any(|w| w == b"<!--")
                && !was.windows(9).any(|w| w == b"<![CDATA[")
                && was.windows(2).filter(|w| *w == b"<?").count() <= 1
            {
                // A tag of exactly this name: the byte after it ends the name (`<text:bookmark-x` is another element, which goes with a paragraph taken out).
                let count = |hay: &[u8], needle: &[u8]| {
                    hay.windows(needle.len() + 1)
                        .filter(|w| w.starts_with(needle) && matches!(w[needle.len()], b' ' | b'/' | b'>' | b'\t' | b'\n' | b'\r'))
                        .count()
                };
                for needle in [
                    &b"<text:bookmark"[..],
                    &b"<text:bookmark-start"[..],
                    &b"<text:bookmark-end"[..],
                    &b"<text:reference-mark"[..],
                    &b"<text:reference-mark-start"[..],
                    &b"<text:reference-mark-end"[..],
                    &b"<text:soft-page-break"[..],
                ] {
                    assert_eq!(count(now, needle), count(was, needle), "a text edit changed the number of {}", String::from_utf8_lossy(needle));
                }
            }
            // A connector whose route the edits only respelled keeps its start tag, `svg:d` and `draw:line-skew` with it, in the save and in a
            // second save from the index the first gave, whichever way the hop is in the second.
            if route_only && !to_flat {
                let tags = |file: &[u8]| -> Vec<Vec<u8>> {
                    let Some(part) = content_of(file) else { return Vec::new() };
                    let mut out = Vec::new();
                    let mut from = 0;
                    while let Some(at) = part.get(from..).and_then(|rest| rest.windows(13).position(|w| w == b"<draw:connect")) {
                        let start = from + at;
                        let end = part.get(start..).and_then(|rest| rest.iter().position(|b| *b == b'>')).map_or(part.len(), |e| start + e + 1);
                        out.push(part.get(start..end).unwrap_or_default().to_vec());
                        from = end.max(start + 1);
                    }
                    out
                };
                assert_eq!(tags(&saved.bytes), tags(&bytes), "a route that was only respelled is written, or its skew goes");
                let mut next = doc.clone();
                for &(op, which, amount) in &second_routes {
                    route_respell(&mut next, (op, which, amount ^ 1));
                }
                if let Ok(second) = write_with(&next, Some(&OriginalPackage { bytes: &saved.bytes, index: &saved.index }), &options) {
                    assert_eq!(tags(&second.bytes), tags(&bytes), "a second respell, the hop the other way, is written, or its skew goes");
                }
            }
            // After a save of a run of route edits, and after a second save with the hop the other way, each connector whose
            // route has no hop reopens with the corners the model has: a route a save kept for its hop is the file's, and the next save writes the model's once it has none.
            if route_chain && !to_flat && !was_flat && !legacy_part {
                // Connectors the save says it did not write the route of, and routes with a corner past a billion inches, are left out (the property was unsound for
                // them and crashed on inputs that are no bug).
                let reopens = |written: &Written, model: &Document, what: &str| {
                    let noted = noted_routes(written);
                    if let Ok((back, _)) = read(&written.bytes, &limits) {
                        let (now, was) = (route_corners(model), route_corners(&back));
                        for (id, (hop, corners)) in &now {
                            if noted.contains(id) || corners.iter().any(|p| p.0.abs() > MOST_COORDINATE || p.1.abs() > MOST_COORDINATE) {
                                continue;
                            }
                            let Some((_, read_corners)) = was.get(id) else { continue };
                            let close = corners.len() == read_corners.len()
                                && corners.iter().zip(read_corners).all(|(p, q)| (p.0 - q.0).abs() < 2e-3 && (p.1 - q.1).abs() < 2e-3);
                            assert!(*hop || close, "{what}: connector {id} has no hop and reopens with {read_corners:?} for the model's {corners:?}");
                        }
                    }
                };
                reopens(&saved, &doc, "the first save");
                let mut next = doc.clone();
                for &(op, which, amount) in &second_routes {
                    route_respell(&mut next, (op, which, amount ^ 1));
                }
                if let Ok(second) = write_with(&next, Some(&OriginalPackage { bytes: &saved.bytes, index: &saved.index }), &options) {
                    reopens(&second, &next, "the second save");
                }
                // The page's size changed **again** in a later save while the hop stays (the corners the index holds for a route kept are carried across a page
                // that changes height, and in the page the save leaves), then the hop the other way in the save after it, which writes the route in the page of that one.
                if page_sizes {
                    let mut later = doc.clone();
                    for &(op, which, amount) in &page_again {
                        page_size(&mut later, (op, amount, which));
                    }
                    if let Ok(third) = write_with(&later, Some(&OriginalPackage { bytes: &saved.bytes, index: &saved.index }), &options) {
                        reopens(&third, &later, "the third save, the page's size changed again");
                        for &(op, which, amount) in &second_routes {
                            route_respell(&mut later, (op, which, amount ^ 1));
                        }
                        if let Ok(fourth) = write_with(&later, Some(&OriginalPackage { bytes: &third.bytes, index: &third.index }), &options) {
                            reopens(&fourth, &later, "the fourth save, the hop the other way");
                        }
                    }
                    // The path kept in a save that leaves the page as the file has it (so the corners the index keeps are the file's as they were read), carried across the save
                    // that changes the page with the hop still there, and the hop the other way after it: a carried entry holds the corners as they are, the path being shifted
                    // by what the page grew (it was raised by it while the path stayed).
                    if let Ok((mut start, _)) = read(&bytes, &limits) {
                        for &e in &route_edits {
                            edit(&mut start, e, next_id);
                        }
                        if let Ok(a) = write_with(&start, Some(&OriginalPackage { bytes: &bytes, index: &report.index }), &options) {
                            reopens(&a, &start, "the carried chain's first save, the page as it is");
                            let mut grown = start.clone();
                            for &(op, which, amount) in &page_again {
                                page_size(&mut grown, (op, which, amount));
                            }
                            if let Ok(b) = write_with(&grown, Some(&OriginalPackage { bytes: &a.bytes, index: &a.index }), &options) {
                                reopens(&b, &grown, "the carried chain's save that changes the page, the hop still there");
                                for &(op, which, amount) in &second_routes {
                                    route_respell(&mut grown, (op, which, amount ^ 1));
                                }
                                if let Ok(c) = write_with(&grown, Some(&OriginalPackage { bytes: &b.bytes, index: &b.index }), &options) {
                                    reopens(&c, &grown, "the carried chain's last save, the hop the other way");
                                }
                            }
                        }
                    }
                }
            }
            // A save whose only edits are the page's size, with the page's height changed, touches no connector: each keeps
            // its own path through it, **shifted by what the page grew, byte for byte as the writer's own `shifted_path` gives it** (a path it cannot place is kept as it is, and
            // is exactly the connectors the save names), its `svg:viewBox` and `draw:line-skew` are the file's, and the file reads back with the routes it was read with, **each
            // connector's but the ones the save names, and every one of them present** (a connector missing after the save is a failure, not a skip). A second save, back at
            // the file's own height, gives each route the file had again.
            if page_only
                && !to_flat
                && !was_flat
                && !legacy_part
                && let Ok((original, _)) = read(&bytes, &limits)
            {
                let height = |d: &Document| d.pages.get(0).and_then(|p| p.sheet.cell("PageHeight")).and_then(|c| c.value.as_ref()?.number());
                let far = |routes: &std::collections::BTreeMap<u32, (bool, Vec<(f64, f64)>)>| {
                    routes.values().any(|(_, corners)| corners.iter().any(|p| p.0.abs() > MOST_COORDINATE || p.1.abs() > MOST_COORDINATE))
                };
                let was = route_corners(&original);
                if let (Some(now), Some(then)) = (height(&doc), height(&original))
                    && now.is_finite()
                    && then.is_finite()
                    && now != then
                {
                    let by = (now - then) * 2540.0;
                    let noted = noted_routes(&saved);
                    let (before, after) = (
                        connector_tags(first_page(&content_of(&bytes).unwrap_or_default())),
                        connector_tags(first_page(&content_of(&saved.bytes).unwrap_or_default())),
                    );
                    assert_eq!(before.len(), after.len(), "a page's size edit leaves as many connectors");
                    // The connectors whose path the writer cannot place, which the save names; and those whose path the target cannot read (a quote it does not take, a reference it
                    // does not know), which it cannot say of.
                    let (mut unplaced, mut unknown) = (0usize, 0usize);
                    for (b, a) in before.iter().zip(&after) {
                        for name in ["draw:line-skew", "svg:viewBox"] {
                            assert_eq!(
                                attribute(a, name),
                                attribute(b, name),
                                "a connector no edit touched keeps its {name} through a change of the page's height"
                            );
                        }
                        match (attribute(b, "svg:d"), attribute(a, "svg:d")) {
                            (None, now_d) => {
                                assert_eq!(now_d, None, "a connector with no path has none made");
                                unknown += usize::from(b.windows(8).any(|w| w == b" svg:d='"));
                            }
                            (Some(_), None) => panic!("a connector's path is not taken out by a change of the page's height"),
                            (Some(was_d), Some(now_d)) => match (unreference(&was_d), unreference(&now_d)) {
                                (Some(was_text), Some(now_text)) => match shifted_path(&was_text, by) {
                                    Some(want) => assert_eq!(now_text, want, "svg:d is the file's path shifted by {by}: from {was_text:?}"),
                                    None => {
                                        unplaced += 1;
                                        assert_eq!(now_d, was_d, "a path the writer cannot place is kept as it is");
                                    }
                                },
                                _ => unknown += 1,
                            },
                        }
                    }
                    assert!(
                        noted.len() >= unplaced && noted.len() <= unplaced + unknown,
                        "the save names {} connectors whose path it could not place, and {unplaced} paths cannot be placed ({unknown} could not be read here)",
                        noted.len()
                    );
                    // The routes: every connector's but the ones the save names, and each must be there to compare.
                    let close = |a: &std::collections::BTreeMap<u32, (bool, Vec<(f64, f64)>)>,
                                 b: &std::collections::BTreeMap<u32, (bool, Vec<(f64, f64)>)>,
                                 skipped: &[u32]| {
                        a.iter().filter(|(id, _)| !skipped.contains(id)).all(|(id, (_, corners))| {
                            b.get(id).is_some_and(|(_, other)| {
                                corners.len() == other.len()
                                    && corners.iter().zip(other).all(|(p, q)| (p.0 - q.0).abs() < 2e-3 && (p.1 - q.1).abs() < 2e-3)
                            })
                        })
                    };
                    if !far(&was)
                        && let Ok((back, _)) = read(&saved.bytes, &limits)
                    {
                        assert!(
                            close(&was, &route_corners(&back), &noted),
                            "the path went down the page with the page: the file reads back with the routes it was read with"
                        );
                    }
                    // Back at the file's own height: the routes are the file's again (the shifts cancel).
                    if let Ok(second) = write_with(&original, Some(&OriginalPackage { bytes: &saved.bytes, index: &saved.index }), &options)
                        && !far(&was)
                        && let Ok((back, _)) = read(&second.bytes, &limits)
                    {
                        assert!(close(&was, &route_corners(&back), &noted_routes(&second)), "back at the file's height, each route is the file's");
                    }
                }
            }
            // The new pictures of a package are entries under `Pictures/`, one for each distinct bytes, each with a row in the manifest, and the file reads
            // with every one of them shown (a picture the file lacks would read as a placeholder and a warning).
            if !was_flat && !to_flat {
                let added: std::collections::BTreeSet<Vec<u8>> = doc
                    .pages
                    .iter()
                    .flat_map(|p| p.shapes.iter())
                    .filter_map(|s| s.foreign.as_ref().and_then(|f| f.picture.as_ref()))
                    .filter(|m| &*m.part == "fuzz-picture")
                    .map(|m| m.bytes.to_vec())
                    .collect();
                // The package a save made is one the package layer reads, and so is the one it was made from (the reader read it): no skip when it is not.
                let pictures = |file: &[u8]| {
                    Package::read(file, &limits)
                        .expect("the package layer reads a package the reader read and one the save made")
                        .parts()
                        .filter(|n| n.starts_with("Pictures/"))
                        .count()
                };
                assert_eq!(pictures(&saved.bytes), pictures(&bytes) + added.len(), "one entry for each distinct picture added");
                if !added.is_empty() {
                    let now = Package::read(&saved.bytes, &limits).expect("the package layer reads the package the save made");
                    for entry in now.parts().filter(|n| n.starts_with("Pictures/")) {
                        assert!(now.manifest().entry(entry).is_some(), "a picture's entry has its row: {entry}");
                    }
                    // A picture the file's own frames name and the package lacks is missing before the save as well: what the save adds is never one more (a new picture never takes
                    // a name a frame or a fill of either part dangles at).
                    let missing = |file: &[u8]| read(file, &limits).expect("the reader reads the file").1.warned("picture_missing");
                    assert!(missing(&saved.bytes) <= missing(&bytes), "a picture the save added shows in a frame that named it before");
                }
            }
            // The kind that was asked for is stated in `mimetype` and in the manifest's root row, as R2 asks of `mimetype`; a kind not asked for is
            // the package's own.
            assert_eq!(sniff(&saved.bytes), wanted, "the package states the kind that was asked for");
            if kind.asked().is_some()
                && let Ok(now) = Package::read(&saved.bytes, &limits)
            {
                assert!(now.mimetype_regular(), "a save that asks for a kind states it, and mimetype is as R2 asks: {:?}", now.warnings());
                if retyped {
                    assert_eq!(now.manifest().root().and_then(|r| r.media_type.as_deref()), wanted.map(Variant::media_type));
                }
            }
            // An unedited save of another kind changes the root row's media type, the one element, and the bytes of `mimetype`, and
            // nothing else, whatever the package is like (when both read whole, which a damaged picture's bytes, copied as they are, do not).
            if retyped
                && !saved.edited
                && !gains_thumbnail
                && let Ok(diff) = diff_packages(&bytes, &saved.bytes, &limits)
            {
                let manifest = diacraft_odg::package::MANIFEST_PATH;
                assert_eq!(diff.parts.iter().map(|p| p.part.as_str()).collect::<Vec<_>>(), [manifest], "{diff:?}");
                let part = &diff.parts[0];
                assert_eq!((part.counts.added, part.counts.removed, part.changed.len()), (0, 0, 1), "{part:?}");
                let attributes = &part.changed[0].attributes;
                assert!(attributes.len() == 1 && attributes[0].rsplit(':').next() == Some("media-type"), "{attributes:?}");
                assert!(diff.removed.is_empty() && diff.unparsed.is_empty(), "{diff:?}");
                assert!(diff.bytes_only.iter().chain(&diff.added).all(|n| n == "mimetype"), "{diff:?}");
                if was_regular {
                    assert_eq!(diff.bytes_only, ["mimetype"], "{diff:?}");
                    assert!(diff.methods_changed.is_empty() && !diff.order_changed && diff.added.is_empty(), "{diff:?}");
                    if copy_account(&bytes, &limits).is_ok_and(|a| a.is_exact()) {
                        assert!(diff.headers_changed.is_empty(), "{diff:?}");
                    }
                }
            }
            // Class F1: a part of ODF 1.0 or 1.1 gains no `xml:id` and no `svg:d`, and its version is the file's own.
            if flags.legacy()
                && let (Ok(now), Ok(was)) = (Package::read(&saved.bytes, &limits), Package::read(&bytes, &limits))
            {
                let count =
                    |p: &Package, needle: &[u8]| p.part("content.xml").map_or(0, |c| c.windows(needle.len()).filter(|w| *w == needle).count());
                for needle in [&b" xml:id=\""[..], &b" svg:d=\""[..]] {
                    assert!(count(&now, needle) <= count(&was, needle), "a part of ODF 1.0 or 1.1 gained {}", String::from_utf8_lossy(needle));
                }
                assert_eq!(
                    read(&saved.bytes, &limits).map(|(_, r)| r.office_version).ok(),
                    read(&bytes, &limits).map(|(_, r)| r.office_version).ok(),
                    "the version is the file's"
                );
            }
            if !saved.edited && !gains_thumbnail {
                // Every part is the original's. The file itself is only where the copy keeps the archive whole (headers that disagree with their
                // rows are written anew from the rows), so the parts are what is compared.
                if let (Ok(now), Ok(was)) = (Package::read(&saved.bytes, &limits), Package::read(&bytes, &limits)) {
                    let names = |p: &Package| {
                        let mut names: Vec<String> = p.parts().map(str::to_owned).collect();
                        names.sort();
                        names
                    };
                    // `mimetype` may be new (a package that had none, and a save that asked for a kind).
                    let without = |mut names: Vec<String>| {
                        if retyped || repaired {
                            names.retain(|n| n != "mimetype");
                        }
                        names
                    };
                    assert_eq!(without(names(&now)), without(names(&was)), "an unedited save has the original's parts");
                    for name in names(&now) {
                        // The two statements of the media type are the only parts a save that asks for a kind changes: `mimetype`, and the manifest
                        // when the kind changes (or the root row says nothing).
                        if (retyped || repaired) && (name == "mimetype" || name == diacraft_odg::package::MANIFEST_PATH) {
                            continue;
                        }
                        assert_eq!(now.part(&name), was.part(&name), "an unedited save keeps the part {name}");
                    }
                }
            }
            // The same model through the new index is no edit, and the same bytes, for every kind of edit and with no assertion skipped
            // for any (a skip for the new shapes and the retexts hid a second save that was refused for changes nobody made), and so is a third.
            let again = write_with(&doc, Some(&OriginalPackage { bytes: &saved.bytes, index: &saved.index }), &options);
            let again = again.expect("a second save of a saved model");
            assert!(
                !again.edited,
                "a second save with nothing changed is an edit: {:?}",
                diacraft_odg::diff_packages(&saved.bytes, &again.bytes, &limits)
            );
            assert_eq!(again.bytes, saved.bytes);
            let third = write_with(&doc, Some(&OriginalPackage { bytes: &again.bytes, index: &again.index }), &options)
                .expect("a third save of a saved model");
            assert!(!third.edited && third.bytes == again.bytes, "a third save with nothing changed is an edit");
            // A second round of page edits on the saved model, saved from the saved bytes and the index the first save gave: it reads back under the names and
            // `ID`s the model has, and saved again it is no edit.
            if page_ops && !was_flat && !to_flat {
                let mut next = doc.clone();
                for e in &second_round {
                    page_edit(&mut next, *e);
                }
                match write_with(&next, Some(&OriginalPackage { bytes: &saved.bytes, index: &saved.index }), &options) {
                    Err(e) => assert!(CLASSES.contains(&e.class()) || is_unscannable_part(&e, &saved.bytes), "an unknown class: {e:?}"),
                    Ok(second) => {
                        let (back, _) = read(&second.bytes, &limits).expect("the bytes written read");
                        let names = |d: &Document| d.pages.iter().map(|p| p.name.to_string()).collect::<Vec<_>>();
                        let ids = |d: &Document| d.pages.iter().map(|p| p.id).collect::<Vec<_>>();
                        assert_eq!(names(&back), names(&next), "a second round: the pages read back under the names the model has");
                        assert_eq!(ids(&back), ids(&next), "and under its IDs");
                        let repeat = write_with(&next, Some(&OriginalPackage { bytes: &second.bytes, index: &second.index }), &options)
                            .expect("a save again of a saved model");
                        assert!(!repeat.edited && repeat.bytes == second.bytes, "a second round saved again is an edit");
                    }
                }
            }
        }
    }
});
