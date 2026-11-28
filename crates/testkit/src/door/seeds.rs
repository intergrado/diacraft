//! The seeds: a few files for each format, built in code. No file is read and none is committed.
//!
//! * `.vsdx`: packages made with [`VsdxBuilder`] (a small drawing, one with a little of everything the reader reads, and one with masters and styles that inherit);
//! * `.odg` and `.fodg`: drawings written by hand from the OASIS standard (a plain page and one with groups, a frame, a connector glued to shapes and styles that name their parents),
//!   as a package and as one flat document;
//! * `.diacraft`: what the engine writes from the drawings above (the whole-model codec's output), so that the seeds hold what a real save holds, held data of the other formats
//!   included;
//! * draw.io (`mxfile`): pages of cells, plain, compressed and several to a file.
//!
//! A [`Seed`] also carries the [`Profile`] of its format: what its XML calls its formula cells, its references and its groups, which the XML mutators need.

// A seed that cannot be built must fail its test, like an assertion; the shipped-code ban on panicking (AGENTS.md, Never crash) is for code that runs for users.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::pipeline::engine_convert;
use super::xml::{CellNames, Chain, Profile};
use super::zipfile::{Archive, Entry};
use crate::mxfile as enc;
use crate::package::{CT_PAGE, DIR, MasterSpec, NS_2012, OD, PageSpec, Pkg, VIS, VsdxBuilder, cell, cell_f, shape, shape_with, typical_shape};

/// One seed: a name, the extension its file has, its bytes and, for a package, its entries (the XML mutators change one and write the package again).
#[derive(Clone, Debug)]
pub struct Seed {
    pub name: &'static str,
    pub extension: &'static str,
    pub bytes: Vec<u8>,
    pub archive: Option<Archive>,
    pub profile: Profile,
}

impl Seed {
    /// A seed that is a ZIP package (it must be one this crate's reader of archives can take apart).
    pub fn package(name: &'static str, extension: &'static str, bytes: Vec<u8>, profile: Profile) -> Seed {
        let archive = Archive::parse(&bytes);
        Seed { name, extension, bytes, archive, profile }
    }

    /// A seed that is one text.
    pub fn text(name: &'static str, extension: &'static str, text: String, profile: Profile) -> Seed {
        Seed { name, extension, bytes: text.into_bytes(), archive: None, profile }
    }
}

/// `bytes` as the engine saves them as `to`; a seed that cannot be made fails the test that asked for it.
fn converted(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
    engine_convert(bytes, from, to).unwrap_or_else(|why| panic!("a seed of the door sweeps could not be made: {why}"))
}

// ---------------------------------------------------------------------------------------------------------------- profiles

const CELL: CellNames = CellNames { element: "Cell", name: "N", value: "V", formula: "F" };

/// A chain of shapes that each take their width from the next: item `k` of `n` is the shape with `ID` `1000 + k`.
fn shape_chain(k: usize, n: usize) -> String {
    let next = 1000 + (k + 1) % n;
    format!("<Shape ID=\"{}\"><Cell N=\"Width\" V=\"1\" F=\"Sheet.{next}!Width+1\"/></Shape>", 1000 + k)
}

/// A VSDX package's XML: cells, shapes in `Shapes`, styles and masters that name each other by `ID`.
pub const VSDX_PROFILE: Profile = Profile {
    cell: Some(CELL),
    refs: &[("LineStyle", "ID"), ("FillStyle", "ID"), ("TextStyle", "ID"), ("Master", "ID"), ("MasterShape", "ID"), ("BackPage", "ID")],
    nest_unit: Some(("<Shape ID=\"7\" Type=\"Group\"><Shapes>", "</Shapes></Shape>")),
    nest_into: &["Shapes"],
    chain: Some(Chain { into: "Shapes", item: shape_chain, modest: &[200, 1500, 4000], heavy: &[20_000] }),
    masters: Some(("masters/master", "Shape", "Master")),
};

/// A flat or packaged OpenDocument drawing's XML: shapes named by `draw:id`, styles by `style:name`.
pub const ODG_PROFILE: Profile = Profile {
    cell: None,
    refs: &[
        ("style:parent-style-name", "style:name"),
        ("draw:style-name", "style:name"),
        ("draw:text-style-name", "style:name"),
        ("draw:start-shape", "draw:id"),
        ("draw:end-shape", "draw:id"),
        ("draw:master-page-name", "style:name"),
    ],
    nest_unit: Some(("<draw:g>", "</draw:g>")),
    nest_into: &["draw:page", "draw:g"],
    chain: Some(Chain { into: "draw:page", item: odg_chain, modest: &[50, 200, 500], heavy: &[20_000] }),
    masters: None,
};

/// Connectors that are glued to the shape before and after them: item `k` of `n`.
fn odg_chain(k: usize, n: usize) -> String {
    let (a, b) = (format!("c{k}"), format!("c{}", (k + 1) % n));
    format!(
        "<draw:custom-shape draw:id=\"{a}\" xml:id=\"{a}\" draw:style-name=\"gr1\" svg:width=\"1cm\" svg:height=\"1cm\" svg:x=\"{}cm\" svg:y=\"1cm\"><text:p/></draw:custom-shape>\
         <draw:connector draw:style-name=\"gr1\" draw:start-shape=\"{a}\" draw:start-glue-point=\"3\" draw:end-shape=\"{b}\" draw:end-glue-point=\"1\" svg:x1=\"1cm\" svg:y1=\"1cm\" svg:x2=\"2cm\" svg:y2=\"2cm\"/>",
        k % 20
    )
}

/// Shapes that each take their width from the next, in the whole-model codec's XML: item `k` of `n`.
fn native_chain(k: usize, n: usize) -> String {
    let next = 1000 + (k + 1) % n;
    format!(
        "<diacraft:shape id=\"{}\"><diacraft:sheet><diacraft:Cell N=\"Width\" V=\"1\" F=\"Sheet.{next}!Width+1\"/></diacraft:sheet></diacraft:shape>",
        1000 + k
    )
}

/// The whole-model codec's XML: the cells of the ShapeSheet under the prefix of DiaCraft's namespace, shapes in shapes, styles and masters named by `id`.
pub const NATIVE_PROFILE: Profile = Profile {
    cell: Some(CellNames { element: "diacraft:Cell", name: "N", value: "V", formula: "F" }),
    refs: &[
        ("LineStyle", "id"),
        ("FillStyle", "id"),
        ("TextStyle", "id"),
        ("master", "id"),
        ("master-shape", "id"),
        ("from-sheet", "id"),
        ("to-sheet", "id"),
    ],
    nest_unit: Some(("<diacraft:shape id=\"7\" type=\"Group\">", "</diacraft:shape>")),
    nest_into: &["diacraft:page", "diacraft:shape"],
    chain: Some(Chain { into: "diacraft:page", item: native_chain, modest: &[200, 1500, 4000], heavy: &[20_000] }),
    masters: Some(("masters/master", "diacraft:shape", "master")),
};

/// A draw.io file: cells that name their parent, and the ends of an edge, by `id`.
pub const DRAWIO_PROFILE: Profile = Profile {
    cell: None,
    refs: &[("parent", "id"), ("source", "id"), ("target", "id")],
    nest_unit: None,
    nest_into: &[],
    chain: Some(Chain { into: "root", item: drawio_chain, modest: &[100, 600, 1500], heavy: &[20_000] }),
    masters: None,
};

/// Cells that are each in the one before (a container of a container of ...): item `k`.
fn drawio_chain(k: usize, _n: usize) -> String {
    let parent = if k == 0 { "1".to_owned() } else { format!("k{}", k - 1) };
    format!(
        "<mxCell id=\"k{k}\" value=\"c{k}\" style=\"container=1;rounded=1;\" vertex=\"1\" parent=\"{parent}\"><mxGeometry x=\"1\" y=\"1\" width=\"90\" height=\"40\" as=\"geometry\"/></mxCell>"
    )
}

// ---------------------------------------------------------------------------------------------------------------- VSDX

/// A page of letter size, so that the drawing is one that any writer can write.
fn sized_page(id: u32, name: &str) -> PageSpec {
    PageSpec::new(id, name).sheet(r#"<Cell N="PageWidth" V="8.5" U="IN"/><Cell N="PageHeight" V="11" U="IN"/><Cell N="PageScale" V="1" U="IN"/><Cell N="DrawingScale" V="1" U="IN"/>"#)
}

fn vsdx_small() -> Vec<u8> {
    let shapes = format!("{}{}{}", typical_shape(1), typical_shape(2), typical_shape(3));
    VsdxBuilder::new()
        .page(
            sized_page(1, "Page-1")
                .shapes(&shapes)
                .connects(r#"<Connect FromSheet="3" FromCell="BeginX" FromPart="9" ToSheet="1" ToCell="PinX" ToPart="3"/>"#),
        )
        .build()
}

fn comments_xml() -> String {
    format!(
        r#"<Comments xmlns="{NS_2012}" ShowCommentTags="1"><AuthorList><AuthorEntry ID="1" Name="A" Initials="A" SMTP="a@example.org"/></AuthorList><CommentList><CommentEntry AuthorID="1" PageID="1" ShapeID="1" Date="2031-03-07T20:05:09Z" CommentID="1">on a shape</CommentEntry></CommentList></Comments>"#
    )
}

/// A PNG of one transparent pixel: the smallest picture a reader of pictures takes.
const PNG: [u8; 67] = [
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

/// A package with a little of everything the reader reads: a group, a master, a theme, a connect, a picture, a comment and a solution part.
fn vsdx_rich() -> Vec<u8> {
    let shapes = r##"<Shape ID="1" Type="Group" Master="1" LineStyle="1"><Cell N="Width" V="2" F="Height*2" U="IN"/><Cell N="Height" V="1" U="IN"/><Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="1" F="Width"/><Cell N="Y" V="1" F="Height"/></Row></Section><Section N="User"><Row N="a"><Cell N="Value" V="x"/></Row></Section><Text><cp IX="0"/>ab<fld IX="0">3</fld></Text><Shapes><Shape ID="2" MasterShape="4"/><Shape ID="3" Type="Foreign"><ForeignData ForeignType="Bitmap"><Rel r:id="rId1"/></ForeignData></Shape></Shapes></Shape>"##;
    let theme = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="T"><a:themeElements><a:clrScheme name="C"><a:dk1><a:srgbClr val="000000"/></a:dk1></a:clrScheme><a:fontScheme name="F"><a:majorFont><a:latin typeface="X"/></a:majorFont></a:fontScheme></a:themeElements></a:theme>"#;
    VsdxBuilder::new()
        .colors(r##"<ColorEntry IX="24" RGB="#123456"/>"##)
        .face_names(r#"<FaceName NameU="Sans" Panose="1"/>"#)
        .more_styles(r#"<StyleSheet ID="1" LineStyle="0"><Cell N="LineWeight" V="1" F="THEMEVAL()"/></StyleSheet>"#)
        .document_sheet(r#"<Cell N="DocLangID" V="1033"/>"#)
        .theme(theme.as_bytes())
        .solution("DiaCraft", "<SolutionXML Name=\"DiaCraft\"><dc:Data xmlns:dc=\"urn:app.diacraft:extension\" dc:version=\"1\"/></SolutionXML>")
        .master(MasterSpec::new(1, "M").shapes(r#"<Shape ID="4"><Cell N="PinX" V="1" F="Width/2"/></Shape>"#).icon("AAAB"))
        .page(
            sized_page(1, "P")
                .shapes(shapes)
                .connects(r#"<Connect FromSheet="1" FromCell="BeginX" FromPart="9" ToSheet="2" ToCell="PinX" ToPart="3"/>"#)
                .rel("rId1", &format!("{OD}image"), "../media/i.png"),
        )
        .page(sized_page(2, "Q").attrs(r#" BackPage="1""#).without_part())
        .with(
            Pkg::new()
                .part(&format!("{DIR}/media/i.png"), "image/png", PNG)
                .part(&format!("{DIR}/comments.xml"), &CT_PAGE.replace("page", "comments"), comments_xml())
                .rel(&format!("{DIR}/document.xml"), "rId91", &format!("{VIS}comments"), "comments.xml"),
        )
        .build()
}

/// Two masters (one an instance of the other's shape), three styles that inherit, two pages one behind the other, text with a field and a connector with a formula.
fn vsdx_styles() -> Vec<u8> {
    let styles = concat!(
        r#"<StyleSheet ID="1" Name="Base" NameU="Base" LineStyle="0" FillStyle="0" TextStyle="0"><Cell N="LineWeight" V="0.01"/></StyleSheet>"#,
        r#"<StyleSheet ID="2" Name="Child" NameU="Child" LineStyle="1" FillStyle="1" TextStyle="1"><Cell N="LineColor" V="2"/></StyleSheet>"#,
        r##"<StyleSheet ID="3" Name="Grandchild" NameU="Grandchild" LineStyle="2" FillStyle="2" TextStyle="2"><Cell N="FillForegnd" V="#336699" F="THEMEVAL()"/></StyleSheet>"##
    );
    let m1 = shape_with(1, " LineStyle=\"2\"", &format!("{}{}", cell("Width", "1"), cell_f("Height", "2", "Width*2")));
    let m2 = shape_with(1, " Master=\"1\" LineStyle=\"3\"", &cell_f("Width", "3", "Sheet.1!Width"));
    let page1 = format!(
        "{}{}{}",
        shape_with(
            1,
            " Master=\"1\" LineStyle=\"2\" FillStyle=\"3\" TextStyle=\"1\"",
            &format!(
                "{}{}{}<Text><cp IX=\"0\"/>a<fld IX=\"0\">7</fld>b</Text>",
                cell("PinX", "2"),
                cell("PinY", "2"),
                cell_f("Width", "1.5", "PinX*0.75")
            )
        ),
        shape_with(2, " Master=\"2\"", &format!("{}{}", cell("PinX", "5"), cell("PinY", "2"))),
        shape(3, &format!("{}{}", cell_f("BeginX", "2", "PinX"), cell_f("EndX", "5", "Sheet.2!PinX")))
    );
    VsdxBuilder::new()
        .more_styles(styles)
        .master(MasterSpec::new(1, "A").shapes(&m1))
        .master(MasterSpec::new(2, "B").shapes(&m2))
        .page(
            sized_page(1, "Front")
                .shapes(&page1)
                .connects(r#"<Connect FromSheet="3" FromCell="BeginX" FromPart="9" ToSheet="1" ToCell="PinX" ToPart="3"/>"#),
        )
        .page(sized_page(2, "Back").shapes(&shape(1, &cell("PinX", "1"))).attrs(r#" BackPage="1""#))
        .build()
}

/// The `.vsdx` seeds.
pub fn vsdx() -> Vec<Seed> {
    vec![
        Seed::package("vsdx-small", "vsdx", vsdx_small(), VSDX_PROFILE),
        Seed::package("vsdx-rich", "vsdx", vsdx_rich(), VSDX_PROFILE),
        Seed::package("vsdx-styles", "vsdx", vsdx_styles(), VSDX_PROFILE),
    ]
}

// ---------------------------------------------------------------------------------------------------------------- OpenDocument

const NS: &str = concat!(
    r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" "#,
    r#"xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
    r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" "#,
    r#"xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
    r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" "#,
    r#"xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" "#,
    r#"xmlns:xlink="http://www.w3.org/1999/xlink" "#,
    r#"xmlns:dc="http://purl.org/dc/elements/1.1/" "#,
    r#"xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" "#,
    r#"xmlns:script="urn:oasis:names:tc:opendocument:xmlns:script:1.0""#
);
const MEDIA: &str = "application/vnd.oasis.opendocument.graphics";
const LAYERS: &str = r#"<draw:layer draw:name="layout"/><draw:layer draw:name="background"/><draw:layer draw:name="backgroundobjects"/><draw:layer draw:name="controls"/><draw:layer draw:name="measurelines"/>"#;
const GR1: &str = r##"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="Base"><style:graphic-properties draw:fill="solid" draw:fill-color="#3465a4" draw:stroke="solid" svg:stroke-width="0.05cm" svg:stroke-color="#000000"/></style:style>"##;
const GR2: &str = r##"<style:style style:name="gr2" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#cc0000" draw:stroke="none"/></style:style>"##;
const P1: &str = r#"<style:style style:name="P1" style:family="paragraph"><style:text-properties fo:font-size="14pt"/></style:style>"#;
const BASE: &str = r#"<style:style style:name="Base" style:family="graphic"><style:graphic-properties draw:fill="none"/></style:style>"#;

fn rect(id: &str, style: &str, (x, y, w, h): (f64, f64, f64, f64), text: &str) -> String {
    format!(
        "<draw:custom-shape draw:id=\"{id}\" xml:id=\"{id}\" draw:style-name=\"{style}\" draw:text-style-name=\"P1\" draw:layer=\"layout\" draw:name=\"{id}\" svg:width=\"{w}cm\" svg:height=\"{h}cm\" svg:x=\"{x}cm\" svg:y=\"{y}cm\"><text:p>{text}</text:p><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"non-primitive\" draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N\"/></draw:custom-shape>"
    )
}

fn glued(from: &str, to: &str) -> String {
    format!(
        "<draw:connector draw:style-name=\"gr1\" draw:text-style-name=\"P1\" draw:layer=\"layout\" draw:start-shape=\"{from}\" draw:start-glue-point=\"3\" draw:end-shape=\"{to}\" draw:end-glue-point=\"1\" svg:x1=\"3cm\" svg:y1=\"2cm\" svg:x2=\"6cm\" svg:y2=\"2cm\" svg:d=\"M 3000 2000 L 6000 2000\" svg:viewBox=\"0 0 6001 2001\"><text:p/></draw:connector>"
    )
}

fn page(name: &str, shapes: &str) -> String {
    format!("<draw:page draw:name=\"{name}\" draw:master-page-name=\"Default\">{shapes}</draw:page>")
}

fn plain_pages() -> String {
    page("page1", &format!("{}{}{}", rect("a", "gr1", (1.0, 1.0, 2.0, 1.0), "A"), rect("b", "gr2", (6.0, 1.0, 2.0, 1.0), "B"), glued("a", "b")))
}

fn rich_pages() -> String {
    let group = format!(
        "<draw:g draw:id=\"g1\" xml:id=\"g1\">{}{}<draw:g>{}</draw:g></draw:g>",
        rect("c", "gr1", (1.0, 5.0, 2.0, 1.0), "C"),
        rect("d", "gr2", (4.0, 5.0, 2.0, 1.0), "D"),
        rect("e", "gr1", (7.0, 5.0, 1.0, 1.0), "")
    );
    let frame = "<draw:frame draw:style-name=\"gr2\" draw:layer=\"layout\" svg:width=\"4cm\" svg:height=\"2cm\" svg:x=\"1cm\" svg:y=\"8cm\"><draw:text-box><text:p text:style-name=\"P1\">in a frame <text:span>and a span</text:span></text:p></draw:text-box></draw:frame>";
    let others = "<draw:ellipse draw:style-name=\"gr1\" draw:layer=\"layout\" svg:width=\"3cm\" svg:height=\"2cm\" svg:x=\"10cm\" svg:y=\"1cm\"><text:p>round</text:p></draw:ellipse><draw:line draw:style-name=\"gr1\" draw:layer=\"layout\" svg:x1=\"1cm\" svg:y1=\"12cm\" svg:x2=\"9cm\" svg:y2=\"13cm\"/>";
    format!(
        "{}{}",
        page(
            "first",
            &format!(
                "{}{}{}{frame}{others}",
                rect("a", "gr1", (1.0, 1.0, 2.0, 1.0), "A"),
                rect("b", "gr2", (6.0, 1.0, 2.0, 1.0), "B"),
                glued("a", "b")
            )
        ),
        page("second", &group)
    )
}

fn styles_xml() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-styles {NS} office:version=\"1.3\"><office:font-face-decls/><office:styles>{BASE}</office:styles><office:automatic-styles><style:page-layout style:name=\"PM1\"><style:page-layout-properties fo:margin-top=\"1cm\" fo:margin-bottom=\"1cm\" fo:margin-left=\"1cm\" fo:margin-right=\"1cm\" fo:page-width=\"21cm\" fo:page-height=\"29.7cm\" style:print-orientation=\"portrait\"/></style:page-layout></office:automatic-styles><office:master-styles><draw:layer-set>{LAYERS}</draw:layer-set><style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"/></office:master-styles></office:document-styles>"
    )
}

fn content_xml(pages: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-content {NS} office:version=\"1.3\"><office:font-face-decls/><office:automatic-styles>{GR1}{GR2}{P1}</office:automatic-styles><office:body><office:drawing>{pages}</office:drawing></office:body></office:document-content>"
    )
}

fn meta_xml() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-meta {NS} office:version=\"1.3\"><office:meta><meta:generator>TestWriter/1</meta:generator></office:meta></office:document-meta>"
    )
}

fn manifest_xml(names: &[&str]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\">\n",
    );
    out.push_str(&format!(" <manifest:file-entry manifest:full-path=\"/\" manifest:version=\"1.3\" manifest:media-type=\"{MEDIA}\"/>\n"));
    for name in names {
        out.push_str(&format!(" <manifest:file-entry manifest:full-path=\"{name}\" manifest:media-type=\"text/xml\"/>\n"));
    }
    out.push_str("</manifest:manifest>\n");
    out
}

/// A package of a drawing whose pages are `pages`: `mimetype` first and stored, the three XML parts, a manifest that lists them.
fn odg_package(pages: &str) -> Vec<u8> {
    let parts = ["content.xml", "styles.xml", "meta.xml"];
    Archive {
        entries: vec![
            Entry::new("mimetype", MEDIA.as_bytes().to_vec(), false),
            Entry::new("content.xml", content_xml(pages).into_bytes(), true),
            Entry::new("styles.xml", styles_xml().into_bytes(), true),
            Entry::new("meta.xml", meta_xml().into_bytes(), true),
            Entry::new("META-INF/manifest.xml", manifest_xml(&parts).into_bytes(), true),
        ],
    }
    .honest()
}

/// The same drawing as one flat document: the root states the media type, and the parts' contents follow in the order the standard gives.
fn odg_flat(pages: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document {NS} office:mimetype=\"{MEDIA}\" office:version=\"1.3\"><office:meta><meta:generator>TestWriter/1</meta:generator></office:meta><office:font-face-decls/><office:styles>{BASE}</office:styles><office:automatic-styles>{GR1}{GR2}{P1}<style:page-layout style:name=\"PM1\"><style:page-layout-properties fo:margin-top=\"1cm\" fo:margin-bottom=\"1cm\" fo:margin-left=\"1cm\" fo:margin-right=\"1cm\" fo:page-width=\"21cm\" fo:page-height=\"29.7cm\" style:print-orientation=\"portrait\"/></style:page-layout></office:automatic-styles><office:master-styles><draw:layer-set>{LAYERS}</draw:layer-set><style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"/></office:master-styles><office:body><office:drawing>{pages}</office:drawing></office:body></office:document>"
    )
}

/// The packaged `.odg` seeds: a plain page, two pages with groups, a frame and more kinds of shape, and one the engine writes from the model (with DiaCraft's own data in it).
pub fn odg() -> Vec<Seed> {
    let mut seeds = vec![
        Seed::package("odg-plain", "odg", odg_package(&plain_pages()), ODG_PROFILE),
        Seed::package("odg-rich", "odg", odg_package(&rich_pages()), ODG_PROFILE),
    ];
    seeds.push(Seed::package("odg-from-vsdx", "odg", converted(&vsdx_small(), "vsdx", "odg"), ODG_PROFILE));
    seeds
}

/// The flat `.fodg` seeds: the plain and the rich drawing as one XML document each.
pub fn fodg() -> Vec<Seed> {
    vec![
        Seed::text("fodg-plain", "fodg", odg_flat(&plain_pages()), ODG_PROFILE),
        Seed::text("fodg-rich", "fodg", odg_flat(&rich_pages()), ODG_PROFILE),
    ]
}

// ---------------------------------------------------------------------------------------------------------------- DiaCraft

/// The `.diacraft` seeds: what the engine writes from a small VSDX drawing, from one with a little of everything, from one with styles that inherit, and from an `.odg` drawing
/// (which carries the held data of the other format).
pub fn diacraft() -> Vec<Seed> {
    let mut seeds = Vec::new();
    for (name, from, bytes) in [
        ("diacraft-small", "vsdx", vsdx_small()),
        ("diacraft-rich", "vsdx", vsdx_rich()),
        ("diacraft-styles", "vsdx", vsdx_styles()),
        ("diacraft-from-odg", "odg", odg_package(&rich_pages())),
    ] {
        seeds.push(Seed::package(name, "diacraft", converted(&bytes, from, "diacraft"), NATIVE_PROFILE));
    }
    seeds
}

// ---------------------------------------------------------------------------------------------------------------- draw.io

fn drawio_cells() -> String {
    let vertex = |id: u32, label: &str, x: u32, y: u32, style: &str| {
        format!(
            "<mxCell id=\"{id}\" value=\"{label}\" style=\"{style}\" vertex=\"1\" parent=\"1\"><mxGeometry x=\"{x}\" y=\"{y}\" width=\"120\" height=\"60\" as=\"geometry\"/></mxCell>"
        )
    };
    format!(
        "{}{}<mxCell id=\"4\" style=\"edgeStyle=orthogonalEdgeStyle;rounded=0;endArrow=block;\" edge=\"1\" parent=\"1\" source=\"2\" target=\"3\"><mxGeometry relative=\"1\" as=\"geometry\"><Array as=\"points\"><mxPoint x=\"200\" y=\"90\"/></Array></mxGeometry></mxCell>\
         <mxCell id=\"5\" value=\"a container\" style=\"swimlane;startSize=24;container=1;\" vertex=\"1\" parent=\"1\"><mxGeometry x=\"40\" y=\"200\" width=\"300\" height=\"160\" as=\"geometry\"/></mxCell>\
         <mxCell id=\"6\" value=\"&lt;b&gt;inside&lt;/b&gt;\" style=\"text;html=1;fontSize=14;rotation=15;opacity=80;\" vertex=\"1\" parent=\"5\"><mxGeometry x=\"20\" y=\"40\" width=\"100\" height=\"30\" as=\"geometry\"/></mxCell>",
        vertex(2, "Start", 40, 40, "rounded=1;whiteSpace=wrap;html=1;fillColor=#dae8fc;strokeWidth=2;"),
        vertex(3, "End", 280, 40, "ellipse;whiteSpace=wrap;html=1;gradientColor=#ffffff;shadow=1;")
    )
}

/// The draw.io seeds: a plain page, a compressed page, and a file of two pages (one of each).
pub fn drawio() -> Vec<Seed> {
    let cells = drawio_cells();
    vec![
        Seed::text("drawio-plain", "drawio", enc::mxfile(&[enc::plain_page("Page-1", &cells)]), DRAWIO_PROFILE),
        Seed::text("drawio-compressed", "drawio", enc::mxfile(&[enc::compressed_page("Packed", &cells)]), DRAWIO_PROFILE),
        Seed::text("drawio-two-pages", "drawio", enc::mxfile(&[enc::plain_page("One", &cells), enc::compressed_page("Two", &cells)]), DRAWIO_PROFILE),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seeds_of_each_format_are_built_in_code_and_are_what_they_say_they_are() {
        assert_eq!(vsdx().len(), 3);
        assert!(vsdx().iter().all(|s| s.archive.as_ref().is_some_and(|a| a.entries.len() > 4) && s.bytes.starts_with(b"PK")));
        assert_eq!(odg().len(), 3, "two by hand and one from the engine");
        assert!(odg().iter().all(|s| s.archive.is_some()));
        assert!(fodg().iter().all(|s| s.archive.is_none() && String::from_utf8_lossy(&s.bytes).contains("<office:document ")));
        assert_eq!(diacraft().len(), 4);
        assert!(diacraft().iter().all(|s| s.archive.is_some()));
        assert!(drawio().iter().all(|s| s.archive.is_none() && String::from_utf8_lossy(&s.bytes).contains("<mxfile")));
    }

    #[test]
    fn seeds_are_the_same_bytes_on_every_build() {
        let names = |seeds: Vec<Seed>| seeds.into_iter().map(|s| (s.name, crc32fast::hash(&s.bytes))).collect::<Vec<_>>();
        assert_eq!(names(vsdx()), names(vsdx()));
        assert_eq!(names(drawio()), names(drawio()));
        // The seeds the engine writes too (a time that a save writes is pinned): a case is replayed from its number alone, in another run.
        assert_eq!(names(odg()), names(odg()));
        assert_eq!(names(diacraft()), names(diacraft()));
    }
}
