//! The seeds of the three targets: packages built in code, the ones the tests of `crates/vsdx` build (`tests/it/fuzz.rs`, `tests/it/carry.rs`). Nothing here is read from a file.
//!
//! * [`plain`]: a package with a little of everything the reader reads (a group, a master, a theme, a connect, a picture, a solution part of DiaCraft's with no items).
//! * [`carried`]: a package whose solution part carries the data of the other format in every place it rides, written by the model writer from a model that holds such data
//!   (the items, their stamps and the parts of DiaCraft's own that hold the bytes are the writer's, so a read keeps them and the judge goes past the stamp).
//!
//! Each is kept as its **items** (a part's name without its slash, and its bytes, the content types first), so that a target replaces or edits one and packs the rest again with right
//! checksums: a change of the archive's own bytes is stopped by the container's checksums nearly always, which is the container's own fuzz.

use std::sync::{Arc, OnceLock};

use diacraft_doc::stamp::Stamp;
use diacraft_doc::{
    CustomProp, DefFamily, Definition, Document, Format, Held, HeldGroup, Kept, OdfPage, OdfRow, OdfShape, OdfStyle, OdfText, Part, PartRole,
    Passthrough, RawAttrs, RawElement, RawNode, Shape, Text, TextPart,
};
use diacraft_opc::{Limits as OpcLimits, Package};
use diacraft_vsdx::{Kind, Limits, Variant, WriteOptions, read, write_with};

use crate::package::{DIR, MasterSpec, NS_2012, OD, PageSpec, Pkg, VIS, VsdxBuilder, ZipItem, cell, shape, shape_with, zip};

/// A package as its items: the name of each part without its leading slash (`[Content_Types].xml` first), and its bytes.
pub type Items = Vec<(String, Vec<u8>)>;

/// The items of a package: the content types, then every part.
pub fn items_of(bytes: &[u8]) -> Option<Items> {
    let pkg = Package::read(bytes, &OpcLimits::default()).ok()?;
    let mut out = vec![("[Content_Types].xml".to_owned(), pkg.content_types_bytes().to_vec())];
    for name in pkg.parts() {
        out.push((name.trim_start_matches('/').to_owned(), pkg.part(name)?.to_vec()));
    }
    Some(out)
}

/// The package of the items, **stored** (not deflated: a target packs many a second, and the reader's work on a stored part is the same), with right checksums.
pub fn pack(items: &[(String, Vec<u8>)]) -> Vec<u8> {
    zip(&items.iter().map(|(name, bytes)| ZipItem::stored(name, bytes)).collect::<Vec<_>>())
}

/// The namespace of the extended properties part.
const OD_APP: &str = "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties";

/// A package that has a little of everything the reader reads (the seed of the proptest properties of `tests/it/fuzz.rs`).
pub fn plain_package() -> Pkg {
    let shapes = r##"<Shape ID="1" Type="Group" Master="1" LineStyle="1"><Cell N="Width" V="2" F="Height*2" U="IN"/><Cell N="X" V="1" F="Inh"/><Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/></Row></Section><Section N="User"><Row N="a"><Cell N="Value" V="x" E="#N/A"/></Row></Section><Text><cp IX="0"/>ab<fld IX="0">3</fld></Text><Data1>d</Data1><Shapes><Shape ID="2" MasterShape="4"/><Shape ID="3" Type="Foreign"><ForeignData ForeignType="Bitmap"><Rel r:id="rId1"/></ForeignData></Shape></Shapes><Trigger N="RecalcColor"/><x:n xmlns:x="urn:z"/></Shape>"##;
    let theme = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="T"><a:themeElements><a:clrScheme name="C"><a:dk1><a:srgbClr val="000000"/></a:dk1></a:clrScheme><a:fontScheme name="F"><a:majorFont><a:latin typeface="X"/></a:majorFont></a:fontScheme></a:themeElements></a:theme>"#;
    // A file DiaCraft wrote (its extended properties name it): a save into it gives the text of its `User` and `Field` rows the unit, which the seed's `User` row holds with none.
    let app = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Properties xmlns="{OD_APP}"><Application>DiaCraft</Application></Properties>"#
    );
    VsdxBuilder::new()
        .app(&app)
        .colors(r##"<ColorEntry IX="24" RGB="#123456"/>"##)
        .face_names(r#"<FaceName NameU="Sans" Panose="1"/>"#)
        .more_styles(r#"<StyleSheet ID="1" LineStyle="0"><Cell N="LineWeight" V="1" F="THEMEVAL()"/></StyleSheet>"#)
        .document_sheet(r#"<Cell N="DocLangID" V="1033"/>"#)
        .document_extra("<DocumentSettings TopPage=\"0\"/><x:y xmlns:x=\"urn:q\"/>")
        .theme(theme.as_bytes())
        .solution("DiaCraft", "<SolutionXML Name=\"DiaCraft\"><dc:Data xmlns:dc=\"urn:app.diacraft:extension\" dc:version=\"1\"/></SolutionXML>")
        .master(MasterSpec::new(1, "M").shapes(r#"<Shape ID="4"><Cell N="PinX" V="1" F="Width/2"/></Shape>"#).icon("AAAB"))
        .page(
            PageSpec::new(1, "P")
                .shapes(shapes)
                .connects(r#"<Connect FromSheet="1" FromCell="BeginX" FromPart="9" ToSheet="2" ToCell="PinX" ToPart="3"/>"#)
                .rel("rId1", &format!("{OD}image"), "../media/i.png"),
        )
        .page(PageSpec::new(2, "Q").attrs(r#" BackPage="1""#).without_part())
        .with(
            Pkg::new()
                .part(&format!("{DIR}/media/i.png"), "image/png", [1u8, 2, 3])
                // The comments part: an author, a comment on a shape and one on the page, so that the recipes' replacements and splices reach the reader and the writer of it.
                .part(&format!("{DIR}/comments.xml"), "application/vnd.ms-visio.comments+xml", comments_xml()) // brand-ok: a content type the format defines
                .rel(&format!("{DIR}/document.xml"), "rId91", &format!("{VIS}comments"), "comments.xml"),
        )
        .pkg()
}

/// The comments part of the plain seed.
fn comments_xml() -> String {
    format!(
        r#"<Comments xmlns="{NS_2012}" ShowCommentTags="1"><AuthorList><AuthorEntry ID="1" Name="A" Initials="A" SMTP="a@example.org"/></AuthorList><CommentList><CommentEntry AuthorID="1" PageID="1" ShapeID="1" Date="2031-03-07T20:05:09Z" CommentID="1">on a shape</CommentEntry><CommentEntry AuthorID="1" PageID="1" Date="2031-03-08T10:00:00Z" EditDate="2031-03-09T10:00:00Z" CommentID="2" Done="1">on the page</CommentEntry></CommentList></Comments>"#
    )
}

/// A drawing of two pages, a master, a style and two faces: what a VSDX file holds, with a group, a connection point and a text (`base_bytes` of `tests/it/carry.rs`).
fn base_bytes() -> Vec<u8> {
    let second = format!(
        "{}<Section N=\"Connection\"><Row IX=\"0\"><Cell N=\"X\" V=\"1\"/><Cell N=\"Y\" V=\"2\"/></Row></Section><Text>abcd</Text>",
        cell("PinX", "2")
    );
    let group = format!("{}<Shapes>{}</Shapes>", cell("PinX", "3"), shape(4, &cell("PinX", "4")));
    VsdxBuilder::new()
        .face_names(r#"<FaceName NameU="Arial"/><FaceName NameU="Noto Sans"/>"#)
        .more_styles(r#"<StyleSheet ID="1" Name="Fancy" NameU="Fancy"><Cell N="LineWeight" V="0.01"/></StyleSheet>"#)
        .master(MasterSpec::new(1, "M").shapes(&shape(1, &cell("Width", "1"))))
        .page(PageSpec::new(1, "One").shapes(&format!(
            "{}{}{}",
            shape(1, &cell("PinX", "1")),
            shape(2, &second),
            shape_with(3, " Type=\"Group\"", &group)
        )))
        .page(PageSpec::new(2, "Two").shapes(&shape(7, &cell("PinX", "7"))))
        .build()
}

fn raw(name: &str, attrs: &[(&str, &str)]) -> RawElement {
    RawElement { name: Arc::from(name), attrs: RawAttrs::from_pairs(attrs.iter().copied()), children: Vec::new() }
}

fn kept(attrs: &[(&str, &str)]) -> Kept {
    Kept { raw: RawAttrs::from_pairs(attrs.iter().copied()), kept: Passthrough::new() }
}

pub fn part(format: Format, name: &str, media: &str, role: PartRole, bytes: &[u8]) -> Arc<Part> {
    Arc::new(Part {
        format,
        name: Arc::from(name),
        media_type: Arc::from(media),
        role,
        manifest: RawAttrs::from_pairs([("manifest:version", "1.3")]),
        rels: Arc::new(Vec::new()),
        bytes: Arc::from(bytes),
    })
}

fn shape_mut(doc: &mut Document, page: usize, id: u32) -> Option<&mut Shape> {
    let page = Arc::make_mut(doc.pages.get_mut(page)?);
    let slot = (0..page.shapes.len()).find(|&i| page.shapes.get(i).is_some_and(|s| s.id == id))?;
    Some(Arc::make_mut(page.shapes.get_mut(slot)?))
}

/// The drawing of [`base_bytes`], read, with data of the other format in every place it rides (`odf_model` of `tests/it/carry.rs`): the document (its slot, a fill image's bytes, a layer, a
/// custom property, a face), parts of the other format (an object, a folder, a macro project, a picture), a page, a master, a style, three shapes (one in a group), a glue point's row, a text
/// with a part that rides, and what a newer build wrote on a shape.
pub fn carried_model() -> Option<Document> {
    let (mut doc, _) = read(&base_bytes(), &Limits::default()).ok()?;
    {
        let slot = doc.ext.make_mut();
        slot.odf.mimetype = Some(Arc::from("application/vnd.oasis.opendocument.graphics"));
        slot.odf.roots = vec![(Arc::from("content.xml"), RawAttrs::from_pairs([("office:version", "1.3")]))];
        slot.odf.layers = vec![Arc::new(raw("draw:layer", &[("draw:name", "layout")]))];
        slot.odf.definitions = vec![Definition {
            family: DefFamily::FillImage,
            name: Arc::from("img"),
            element: Arc::new(raw("draw:fill-image", &[("draw:name", "img")])),
            bytes: Some(Arc::from(&b"fill image bytes"[..])),
        }];
        slot.native = kept(&[("later", "1")]);
    }
    doc.parts.push(part(Format::Odf, "Object 1/content.xml", "text/xml", PartRole::ObjectFile, b"<office:document-content/>"));
    doc.parts.push(part(Format::Odf, "Object 1/", "", PartRole::ObjectFile, b""));
    doc.parts.push(part(Format::Odf, "Basic/Standard/script-lb.xml", "text/xml", PartRole::MacroProject, b"<ooo:library/>"));
    doc.parts.push(part(Format::Odf, "Pictures/p.png", "image/png", PartRole::Other, &[0x89, b'P', b'N', b'G', 1, 2, 3]));
    Arc::make_mut(&mut doc.props).custom.push(CustomProp {
        format: Format::Odf,
        name: Arc::from("Field"),
        value_type: Arc::from("string"),
        value: Arc::from("v"),
        raw: RawAttrs::new(),
    });
    {
        let fonts = Arc::make_mut(&mut doc.fonts);
        fonts.get_mut(1)?.ext.make_mut().odf = kept(&[("svg:font-family", "Noto Sans")]);
    }
    {
        let page = Arc::make_mut(doc.pages.get_mut(0)?);
        page.ext.make_mut().odf = OdfPage { master_page: Some(Arc::from("Default")), layout: Some(Arc::from("PM1")), ..OdfPage::default() };
    }
    Arc::make_mut(doc.masters.get_mut(0)?).ext.make_mut().native = kept(&[("later-master", "1")]);
    let fancy = doc.styles.iter().position(|s| s.name.as_deref() == Some("Fancy"))?;
    Arc::make_mut(doc.styles.get_mut(fancy)?).ext.make_mut().odf =
        OdfStyle { name: Some(Arc::from("Fancy")), family: Some(Arc::from("common:graphic")), ..OdfStyle::default() };
    shape_mut(&mut doc, 0, 1)?.ext.make_mut().odf = OdfShape {
        style_name: Some(Arc::from("gr1")),
        held: vec![Held {
            group: HeldGroup::Fill,
            stamp: Some(Stamp(0x1234)),
            attrs: RawAttrs::from_pairs([("draw:fill", "hatch")]),
            kept: Passthrough::new(),
        }],
        units: RawAttrs::from_pairs([("svg:width", "2cm")]),
        ..OdfShape::default()
    };
    {
        let two = shape_mut(&mut doc, 0, 2)?;
        two.ext.make_mut().odf.style_name = Some(Arc::from("gr2"));
        two.ext.make_mut().native = kept(&[("later-shape", "x")]);
        let sheet = Arc::make_mut(&mut two.sheet);
        let section = Arc::make_mut(sheet.sections.iter_mut().find(|s| s.name.as_str() == "Connection")?);
        let row = Arc::make_mut(section.rows.get_mut(0)?);
        row.ext.make_mut().odf = OdfRow {
            held: Some(Held {
                group: HeldGroup::Glue,
                stamp: None,
                attrs: RawAttrs::from_pairs([("draw:escape-direction", "horizontal")]),
                kept: Passthrough::new(),
            }),
            ..OdfRow::default()
        };
        two.text = Some(Arc::new(Text {
            parts: vec![
                TextPart::Chars(Arc::from("ab")),
                TextPart::OdfRaw(Box::new(RawNode::Element(raw("text:bookmark", &[("text:name", "b")])))),
                TextPart::Chars(Arc::from("cd")),
            ],
            ext: {
                let mut ext = diacraft_doc::Ext::default();
                ext.make_mut().odf = OdfText { kept: kept(&[("text:style-name", "P1")]), ..OdfText::default() };
                ext
            },
        }));
    }
    {
        let group = shape_mut(&mut doc, 0, 3)?;
        let child = Arc::make_mut(group.children.get_mut(0)?);
        child.ext.make_mut().odf.style_name = Some(Arc::from("gr9"));
    }
    Some(doc)
}

/// The options of a save of the model alone, with the time given (the writer has no clock) and the model said to be the one that was read.
pub fn options(macros: bool) -> WriteOptions {
    WriteOptions {
        now: Some(1_700_000_000),
        content_unchanged: true,
        variant: macros.then_some(Variant { kind: Kind::Drawing, macro_enabled: true }),
        ..WriteOptions::default()
    }
}

/// A package that carries data of the other format in every place it rides, as a macro-enabled drawing (the seed of the properties of `tests/it/fuzz.rs`).
pub fn carried_package() -> Option<Vec<u8>> {
    Some(write_with(&carried_model()?, None, &options(true)).ok()?.bytes)
}

/// [`carried_model`], built once: the model a plan of `vsdx_roundtrip` adds parts to (a clone shares what it holds).
pub fn carried_doc() -> &'static Document {
    static DOC: OnceLock<Document> = OnceLock::new();
    DOC.get_or_init(|| carried_model().expect("the carried seed's model is made"))
}

/// What a target starts from, built once.
pub struct Seeds {
    /// [`plain_package`], as items.
    pub plain: Items,
    /// [`carried_package`], as items.
    pub carried: Items,
}

/// The seeds, built on first use. A seed that cannot be made is a crash of the target itself, said at once.
pub fn seeds() -> &'static Seeds {
    static SEEDS: OnceLock<Seeds> = OnceLock::new();
    SEEDS.get_or_init(|| {
        let plain = items_of(&plain_package().build()).expect("the plain seed is a package");
        let carried = items_of(&carried_package().expect("the carried seed is written")).expect("the carried seed is a package");
        Seeds { plain, carried }
    })
}
