//! A VSDX drawing that holds masters is saved as an `.odg`, and the trip back gives the masters and the instances as they were.
//!
//! **Every file is made by code**, from the standard: the VSDX packages by the testkit's builder (the shapes' XML written by hand from the specifications), the readers are the real ones.

use std::fmt::Write as _;

use diacraft_doc::{Document, Shape, ShapeList};
use diacraft_engine::Session;
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape_with};
use diacraft_testkit::theme::ThemeXml;
use serde_json::json;

use crate::native_common::{Dir, pinned};
use crate::run;
use crate::stencil_open::{STENCIL_COLORS, STENCIL_STYLES, decision_master, line_master, process_master};

const CORE_HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">";

/// The core properties of a file as an application writes them: the fields it has nothing for are empty elements.
fn core_empties() -> String {
    format!(
        "{CORE_HEAD}<dc:title></dc:title><dc:subject></dc:subject><dc:creator>A person</dc:creator><cp:keywords></cp:keywords><dc:description></dc:description><cp:category></cp:category><dcterms:created xsi:type=\"dcterms:W3CDTF\">2020-01-02T03:04:05Z</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">2021-01-02T03:04:05Z</dcterms:modified></cp:coreProperties>"
    )
}

/// The face table as an application writes it: each face has its number.
const FACES: &str = r#"<FaceName ID="1" NameU="Segoe Script" Panose="2 11 6 4 2 2 2 2 2 4" Flags="1"/><FaceName ID="2" NameU="Arial" Panose="2 11 6 4 2 2 2 2 2 4" Flags="1"/>"#;

/// The cells that make a shape draw (a line and a fill, as the shapes of a file hold them: a shape that links no style sheet and holds none draws nothing).
const LOOK: &str = r##"<Cell N="LinePattern" V="1"/><Cell N="LineColor" V="#003366"/><Cell N="LineWeight" V="0.02"/><Cell N="FillPattern" V="1"/><Cell N="VerticalAlign" V="1"/>"##;

fn page_sheet() -> &'static str {
    r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#
}

/// The builder every vector starts from: the stencil's styles, colours and faces, so that the masters follow style sheets and a palette colour.
fn base() -> VsdxBuilder {
    // The core properties are those of a file an application wrote: the fields it has nothing for are empty elements.
    VsdxBuilder::new().colors(STENCIL_COLORS).face_names(FACES).more_styles(STENCIL_STYLES).core(&core_empties())
}

/// Instances of two masters, each with cells of its own over its master's, and a text of its own on one.
fn boxes() -> Vec<u8> {
    let page = format!(
        "{}{}{}",
        shape_with(10, r#" Master="1""#, r#"<Cell N="PinX" V="3"/><Cell N="PinY" V="4"/>"#),
        shape_with(
            11,
            r#" Master="2""#,
            r##"<Cell N="PinX" V="6"/><Cell N="PinY" V="4"/><Cell N="FillForegnd" V="#00aa00"/><Text>local text</Text>"##
        ),
        shape_with(12, r#" Master="1""#, r#"<Cell N="PinX" V="3"/><Cell N="PinY" V="7"/><Cell N="Angle" V="0.5"/>"#),
    );
    base().master(process_master()).master(decision_master()).page(PageSpec::new(1, "Page-1").sheet(page_sheet()).shapes(&page)).build()
}

/// A member of a group of two: a rectangle 1 by 2 with its pin at (`x`, 1) in the group's frame, drawn with a line and the fill `colour`, and the rows of its outline.
fn pair_member(id: u32, x: f64, colour: &str) -> String {
    shape_with(
        id,
        r#" Type="Shape""#,
        &format!(
            r##"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="2"/><Cell N="LocPinX" V="0.5" F="Width*0.5"/><Cell N="LocPinY" V="1" F="Height*0.5"/>{look}<Cell N="FillForegnd" V="{colour}"/><Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="1" F="Width*1"/><Cell N="Y" V="0" F="Height*0"/></Row><Row T="LineTo" IX="3"><Cell N="X" V="1" F="Width*1"/><Cell N="Y" V="2" F="Height*1"/></Row><Row T="LineTo" IX="4"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="2" F="Height*1"/></Row><Row T="LineTo" IX="5"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row></Section>"##,
            look = LOOK
        ),
    )
}

/// The frame of a group of two [`pair_member`]s, 3 by 2 (the box of its members), with its pin at (`x`, `y`).
fn pair_frame(x: f64, y: f64) -> String {
    format!(
        r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="{y}"/><Cell N="Width" V="3"/><Cell N="Height" V="2"/><Cell N="LocPinX" V="1.5"/><Cell N="LocPinY" V="1"/>"#
    )
}

/// A master that is a group of two shapes, and an instance of it (its members name the master's by `MasterShape`).
fn group_master() -> Vec<u8> {
    let members = format!("{}{}", pair_member(5, 0.5, "#ff0000"), pair_member(6, 2.5, "#0000ff"));
    let top = shape_with(4, r#" Type="Group""#, &format!("{}<Shapes>{members}</Shapes>", pair_frame(1.5, 1.0)));
    let instance = shape_with(
        10,
        r#" Master="7" Type="Group""#,
        r##"<Cell N="PinX" V="5"/><Cell N="PinY" V="5"/><Shapes><Shape ID="11" MasterShape="5" Type="Shape"/><Shape ID="12" MasterShape="6" Type="Shape"><Cell N="FillForegnd" V="#00ff00"/></Shape></Shapes>"##,
    );
    base().master(MasterSpec::new(7, "Pair").shapes(&top)).page(PageSpec::new(1, "Page-1").sheet(page_sheet()).shapes(&instance)).build()
}

/// Two boxes of a master with connection points and a connector between them, glued at the master's points.
fn glued() -> Vec<u8> {
    let page = format!(
        "{}{}{}",
        shape_with(10, r#" Master="1""#, r#"<Cell N="PinX" V="2"/><Cell N="PinY" V="2"/>"#),
        shape_with(11, r#" Master="1""#, r#"<Cell N="PinX" V="6"/><Cell N="PinY" V="2"/>"#),
        shape_with(
            12,
            r#" Master="3""#,
            concat!(
                r#"<Cell N="BeginX" V="2.75"/><Cell N="BeginY" V="2"/><Cell N="EndX" V="5.25"/><Cell N="EndY" V="2"/>"#,
                r#"<Cell N="PinX" V="4" F="(BeginX+EndX)/2"/><Cell N="PinY" V="2" F="(BeginY+EndY)/2"/><Cell N="Width" V="2.5" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="LocPinX" V="1.25" F="Width*0.5"/>"#,
                r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2.5" F="Width"/><Cell N="Y" V="0"/></Row></Section>"#
            ),
        ),
    );
    let connects = r#"<Connect FromSheet="12" FromCell="BeginX" FromPart="9" ToSheet="10" ToCell="Connections.X4" ToPart="103"/><Connect FromSheet="12" FromCell="EndX" FromPart="12" ToSheet="11" ToCell="Connections.X2" ToPart="101"/>"#;
    base()
        .master(process_master())
        .master(decision_master())
        .master(line_master())
        .page(PageSpec::new(1, "Page-1").sheet(page_sheet()).shapes(&page).connects(connects))
        .build()
}

/// The master whose shape takes its fill from the theme and its geometry from formulas of its size (as `inherit_pass` builds it), with a theme in the package.
fn themed() -> Vec<u8> {
    let master = concat!(
        r#"<Shape ID="5" Type="Shape" LineStyle="3" FillStyle="3" TextStyle="3"><Cell N="PinX" V="0.5"/><Cell N="PinY" V="0.25"/>"#,
        r#"<Cell N="Width" V="1"/><Cell N="Height" V="0.5"/><Cell N="LocPinX" V="0.5" F="Width*0.5"/><Cell N="LocPinY" V="0.25" F="Height*0.5"/>"#,
        r##"<Cell N="FillForegnd" V="#abcdef" F="THEMEVAL(&quot;FillColor&quot;)"/>"##,
        r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row>"#,
        r#"<Row T="LineTo" IX="2"><Cell N="X" V="1" F="Width*1"/><Cell N="Y" V="0" F="Height*0"/></Row>"#,
        r#"<Row T="LineTo" IX="3"><Cell N="X" V="1" F="Width*1"/><Cell N="Y" V="0.5" F="Height*1"/></Row>"#,
        r#"<Row T="LineTo" IX="4"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0.5" F="Height*1"/></Row>"#,
        r#"<Row T="LineTo" IX="5"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row></Section>"#,
        r#"<Section N="User"><Row N="Span"><Cell N="Value" V="1" F="Width"/></Row></Section></Shape>"#,
    );
    let instances = concat!(
        r#"<Shape ID="2" Master="2" Type="Shape"><Cell N="PinX" V="4"/><Cell N="PinY" V="4"/></Shape>"#,
        r#"<Shape ID="3" Master="2" Type="Shape"><Cell N="PinX" V="6"/><Cell N="PinY" V="4"/><Cell N="Width" V="2"/><Cell N="LocPinX" V="1" F="Width*0.5"/>"#,
        r#"<Section N="Geometry" IX="0"><Row IX="2"><Cell N="X" V="2" F="Width*1"/></Row><Row IX="3"><Cell N="X" V="2" F="Width*1"/></Row></Section></Shape>"#,
    );
    base()
        .theme(ThemeXml::default().xml().as_bytes())
        .master(MasterSpec::new(2, "Box").shapes(master))
        .page(PageSpec::new(1, "P").sheet(page_sheet()).shapes(instances))
        .build()
}

/// A master of two shapes, the second sized by a formula that names the first (`Sheet.5!Width`), and an instance that moves and resizes.
fn named() -> Vec<u8> {
    let outline = r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="W" F="Width*1"/><Cell N="Y" V="0" F="Height*0"/></Row><Row T="LineTo" IX="3"><Cell N="X" V="W" F="Width*1"/><Cell N="Y" V="1" F="Height*1"/></Row><Row T="LineTo" IX="4"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="1" F="Height*1"/></Row><Row T="LineTo" IX="5"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row></Section>"#;
    let first = shape_with(
        5,
        r#" Type="Shape""#,
        &format!(
            r##"<Cell N="PinX" V="1"/><Cell N="PinY" V="0.5"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>{LOOK}<Cell N="FillForegnd" V="#ffcc00"/>{}"##,
            outline.replace("\"W\"", "\"2\"")
        ),
    );
    let second = shape_with(
        6,
        r#" Type="Shape""#,
        &format!(
            r##"<Cell N="PinX" V="3" F="Sheet.5!Width+1"/><Cell N="PinY" V="0.5"/><Cell N="Width" V="1" F="Sheet.5!Width*0.5"/><Cell N="Height" V="1" F="Sheet.5!Height"/><Cell N="LocPinX" V="0.5" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>{LOOK}<Cell N="FillForegnd" V="#99ccff"/>{}"##,
            outline.replace("\"W\"", "\"1\"")
        ),
    );
    let instance = shape_with(
        10,
        r#" Master="9" Type="Group""#,
        r##"<Cell N="PinX" V="4"/><Cell N="PinY" V="4"/><Cell N="Width" V="3.5"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="1.75"/><Cell N="LocPinY" V="0.5"/><Shapes><Shape ID="11" MasterShape="5" Type="Shape"><Cell N="FillForegnd" V="#00ff00"/></Shape><Shape ID="12" MasterShape="6" Type="Shape"/></Shapes>"##,
    );
    base()
        .master(MasterSpec::new(9, "Two").shapes(&format!("{first}{second}")))
        .page(PageSpec::new(1, "P").sheet(page_sheet()).shapes(&instance))
        .build()
}

/// The style sheets of a file an application wrote: the root, and the ones it makes for a drawing, with their names, links to each other, a section of formatting and an `IsCustomNameU`.
const REAL_STYLES: &str = concat!(
    r#"<StyleSheet ID="0" NameU="No Style" IsCustomNameU="1" Name="No Style" IsCustomName="1" LineStyle="0" FillStyle="0" TextStyle="0">"#,
    r#"<Cell N="EnableLineProps" V="1"/><Cell N="EnableFillProps" V="1"/><Cell N="EnableTextProps" V="1"/><Cell N="LineWeight" V="0.01388888888888889"/><Cell N="LineColor" V="0"/><Cell N="LinePattern" V="1"/>"#,
    r##"<Cell N="FillForegnd" V="#ffffff"/><Cell N="FillPattern" V="1"/><Cell N="VerticalAlign" V="1"/>"##,
    r#"<Section N="Character"><Row IX="0"><Cell N="Font" V="Arial"/><Cell N="Color" V="0"/><Cell N="Size" V="0.1111111111111111"/><Cell N="Style" V="0"/></Row></Section>"#,
    r#"<Section N="Paragraph"><Row IX="0"><Cell N="IndFirst" V="0"/><Cell N="HorzAlign" V="1"/></Row></Section></StyleSheet>"#,
    r#"<StyleSheet ID="1" NameU="Text Only" IsCustomNameU="1" Name="Text Only" IsCustomName="1" LineStyle="0" FillStyle="0" TextStyle="0"><Cell N="LinePattern" V="0"/><Cell N="FillPattern" V="0"/></StyleSheet>"#,
    r#"<StyleSheet ID="2" NameU="None" Name="None" LineStyle="0" FillStyle="0" TextStyle="0"><Cell N="LinePattern" V="0"/><Cell N="FillPattern" V="0"/></StyleSheet>"#,
    r#"<StyleSheet ID="3" NameU="Normal" Name="Normal" LineStyle="0" FillStyle="0" TextStyle="0"><Cell N="LineWeight" V="0.01"/></StyleSheet>"#,
    r##"<StyleSheet ID="4" NameU="Guide" Name="Guide" LineStyle="3" FillStyle="2" TextStyle="1"><Cell N="LineColor" V="#808080"/></StyleSheet>"##,
    r#"<StyleSheet ID="5" NameU="Connector" Name="Connector" LineStyle="3" FillStyle="3" TextStyle="3"><Cell N="EndArrow" V="13"/><Cell N="LineWeight" V="0.0138889"/></StyleSheet>"#,
    r##"<StyleSheet ID="7" NameU="Flow Normal" Name="Flow Normal" LineStyle="3" FillStyle="3" TextStyle="3"><Cell N="FillForegnd" V="#E8F0FF"/></StyleSheet>"##,
    r##"<StyleSheet ID="9" NameU="Flow Marked" Name="Flow Marked" LineStyle="7" FillStyle="3" TextStyle="1"><Cell N="LineColor" V="#cc0000"/></StyleSheet>"##,
);

/// The core and extended properties of a file an application wrote, with the custom properties beside them.
fn custom_props() -> diacraft_testkit::package::Pkg {
    diacraft_testkit::package::Pkg::new()
        .part(
            "/docProps/custom.xml",
            "application/vnd.openxmlformats-officedocument.custom-properties+xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/custom-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="2" name="Reviewed"><vt:lpwstr>yes</vt:lpwstr></property><property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="3" name="Revision"><vt:i4>7</vt:i4></property><property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="4" name="Empty"><vt:lpwstr></vt:lpwstr></property></Properties>"#,
        )
        .rel("/", "rId9", "http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties", "docProps/custom.xml")
}

/// A master whose shape holds a text, a formatting of it and shape data, and instances: one that takes the text, one with a text of its own and a value of its own in the shape data.
fn texts_and_data() -> Vec<u8> {
    let master = format!(
        concat!(
            "{}{}",
            r#"<Section N="Paragraph"><Row IX="0"><Cell N="HorzAlign" V="1"/></Row></Section>"#,
            r##"<Section N="Character"><Row IX="0"><Cell N="Size" V="0.2"/><Cell N="Color" V="#cc0000"/></Row></Section>"##,
            r#"<Section N="Property"><Row N="Owner"><Cell N="Label" V="Owner"/><Cell N="Value" V="nobody"/><Cell N="Type" V="0"/></Row></Section>"#,
            r#"<Text><cp IX="0"/>master text</Text>"#
        ),
        LOOK,
        crate::stencil_open::box_cells(1.5, 0.8)
    );
    let master = shape_with(5, "", &master);
    let page = format!(
        "{}{}",
        shape_with(10, r#" Master="1""#, r#"<Cell N="PinX" V="3"/><Cell N="PinY" V="4"/>"#),
        shape_with(
            11,
            r#" Master="1""#,
            r#"<Cell N="PinX" V="6"/><Cell N="PinY" V="4"/><Section N="Property"><Row N="Owner"><Cell N="Value" V="somebody"/></Row></Section><Text><cp IX="0"/>own text</Text>"#
        ),
    );
    base().master(MasterSpec::new(1, "Process").shapes(&master)).page(PageSpec::new(1, "Page-1").sheet(page_sheet()).shapes(&page)).build()
}

/// A master whose text block is `block`: the cells of the block, as formulas of the shape's size.
fn master_with_a_text_block(block: &str) -> Vec<u8> {
    let inner = format!("{}{block}<Text>label</Text>", crate::stencil_open::box_cells(1.5, 0.8));
    let page = shape_with(10, r#" Master="1""#, r#"<Cell N="PinX" V="3"/><Cell N="PinY" V="4"/>"#);
    base()
        .master(MasterSpec::new(1, "Label").shapes(&shape_with(5, "", &inner)))
        .page(PageSpec::new(1, "Page-1").sheet(page_sheet()).shapes(&page))
        .build()
}

/// A master with an icon, a page sheet, and the attributes an application gives a master row.
fn master_row() -> Vec<u8> {
    let page = shape_with(10, r#" Master="1""#, r#"<Cell N="PinX" V="3"/><Cell N="PinY" V="4"/>"#);
    base()
        .master(
            MasterSpec::new(1, "Process")
                .attrs(r#" UniqueID="{00000000-0000-4000-8000-000000000101}" BaseID="{00000000-0000-4000-8000-000000000201}" MatchByName="0" IconSize="1" PatternFlags="0" Prompt="a box" Hidden="0" IconUpdate="1" AlignName="2" MasterType="2""#)
                .icon("QUJD")
                .shapes(&process_master().shapes),
        )
        .page(PageSpec::new(1, "Page-1").sheet(page_sheet()).shapes(&page))
        .build()
}

/// A master that is a picture, and an instance of it.
fn picture_master() -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 2, 2);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(&[200; 16]).unwrap();
    let picture = r#"<Shape ID="5" Type="Foreign"><Cell N="PinX" V="1"/><Cell N="PinY" V="0.5"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/><ForeignData ForeignType="Bitmap" CompressionType="PNG"><Rel r:id="rId1"/></ForeignData></Shape>"#;
    let page = shape_with(10, r#" Master="1" Type="Foreign""#, r#"<Cell N="PinX" V="3"/><Cell N="PinY" V="4"/>"#);
    base()
        .master(MasterSpec::new(1, "Picture").shapes(picture).rel("rId1", &format!("{}image", diacraft_testkit::package::OD), "../media/image1.png"))
        .page(PageSpec::new(1, "Page-1").sheet(page_sheet()).shapes(&page))
        .with(diacraft_testkit::package::Pkg::new().part("/visio/media/image1.png", "image/png", out))
        .build()
}

/// A group whose frame is larger than the box of its members (a group of a master, or of the page: `master`), which a `.odg` has no way to say: the frame of a `draw:g` is the box of its members.
fn roomy_group(master: bool) -> Vec<u8> {
    let member = |id: u32, x: f64| {
        shape_with(
            id,
            r#" Type="Shape""#,
            &format!(
                r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="0.5" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>"#
            ),
        )
    };
    let frame = r#"<Cell N="PinX" V="5"/><Cell N="PinY" V="5"/><Cell N="Width" V="3"/><Cell N="Height" V="2"/><Cell N="LocPinX" V="1.5"/><Cell N="LocPinY" V="1"/>"#;
    let members = format!("{}{}", member(5, 0.75), member(6, 2.25));
    if master {
        let top = shape_with(4, r#" Type="Group""#, &format!("{frame}<Shapes>{members}</Shapes>"));
        let instance = shape_with(
            10,
            r#" Master="7" Type="Group""#,
            r#"<Cell N="PinX" V="5"/><Cell N="PinY" V="5"/><Shapes><Shape ID="11" MasterShape="5" Type="Shape"/><Shape ID="12" MasterShape="6" Type="Shape"/></Shapes>"#,
        );
        base().master(MasterSpec::new(7, "Roomy").shapes(&top)).page(PageSpec::new(1, "P").sheet(page_sheet()).shapes(&instance)).build()
    } else {
        let top = shape_with(4, r#" Type="Group""#, &format!("{frame}<Shapes>{members}</Shapes>"));
        base().page(PageSpec::new(1, "P").sheet(page_sheet()).shapes(&top)).build()
    }
}

/// The drawings a save as an `.odg` still refuses, each with the words it says: the second column is a part of them.
fn refused_vectors() -> Vec<(&'static str, Vec<u8>, &'static str)> {
    vec![
        (
            "styles with a NameU only",
            boxes_builder().styles(&REAL_STYLES.replace(r#" Name="Guide""#, "").replace(r#" Name="Connector""#, "")).build(),
            "it needs a name",
        ),
        (
            "a background page",
            base()
                .master(process_master())
                .page(PageSpec::new(1, "Page-1").sheet(page_sheet()).shapes(&shape_with(
                    10,
                    r#" Master="1""#,
                    r#"<Cell N="PinX" V="3"/><Cell N="PinY" V="4"/>"#,
                )))
                .page(PageSpec::new(2, "Background").attrs(r#" Background="1""#).sheet(page_sheet()).shapes(&shape_with(
                    11,
                    r#" Master="1""#,
                    r#"<Cell N="PinX" V="2"/><Cell N="PinY" V="2"/>"#,
                )))
                .build(),
            "a background page",
        ),
    ]
}

/// The vectors: a name and the package each.
pub(crate) fn vectors() -> Vec<(&'static str, Vec<u8>)> {
    let app = |inner: &str| {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\">{inner}</Properties>"
        )
    };
    vec![
        ("boxes", boxes()),
        ("boxes, core fields empty", boxes_builder().core(&core_empties()).build()),
        ("group master", group_master()),
        ("connection points and a glued connector", glued()),
        ("theme and formulas of the size", themed()),
        ("a master of two shapes, a formula that names one", named()),
        ("styles of an application", boxes_builder().styles(REAL_STYLES).build()),
        (
            "the document's sheet and extras",
            boxes_builder()
                .document_sheet(r#"<Cell N="DocLangID" V="1033"/><Cell N="OutputFormat" V="0"/><Section N="User"><Row N="msvNoAutoConnect"><Cell N="Value" V="0"/></Row></Section>"#)
                .document_extra(r#"<EventList/><HeaderFooter HeaderMargin="0.5"><HeaderFooterFont><Cell N="Font" V="Arial"/></HeaderFooterFont></HeaderFooter>"#)
                .build(),
        ),
        ("custom properties", boxes_builder().with(custom_props()).build()),
        (
            "app fields",
            boxes_builder()
                .app(&app("<Template>x.vst</Template><Application>Another Drawing App</Application><ScaleCrop>false</ScaleCrop><Manager></Manager><Company>Co</Company><Pages>1</Pages><AppVersion>16.0000</AppVersion><HyperlinkBase></HyperlinkBase>"))
                .build(),
        ),
        ("theme in the package", boxes_builder().theme(ThemeXml::default().xml().as_bytes()).build()),
        ("texts and shape data of a master", texts_and_data()),
        ("the row of a master", master_row()),
        ("a picture master", picture_master()),
    ]
}

fn boxes_builder() -> VsdxBuilder {
    let page = shape_with(10, r#" Master="1""#, r#"<Cell N="PinX" V="3"/><Cell N="PinY" V="4"/>"#);
    base().master(process_master()).page(PageSpec::new(1, "Page-1").sheet(page_sheet()).shapes(&page))
}

// ---------------------------------------------------------------------------------------------------------------- reading what a trip gives

/// The lines that tell a shape: its place in the list, its `ID`, its links to a master, its own cells (name, formula, value) and its children.
fn tell_shape(out: &mut String, depth: usize, shape: &Shape) {
    let pad = "  ".repeat(depth);
    let _ = writeln!(out, "{pad}shape {} master={:?} master_shape={:?} kind={:?}", shape.id, shape.master, shape.master_shape, shape.kind);
    for (name, cell) in shape.sheet.cells.iter() {
        let _ = writeln!(out, "{pad}  cell {} f={:?} v={:?}", name.as_str(), cell.formula, cell.value.as_ref().map(|v| v.text().to_owned()));
    }
    for section in shape.sheet.sections.iter() {
        let _ = writeln!(out, "{pad}  section {} rows={}", section.name.as_str(), section.rows.len());
    }
    tell_list(out, depth + 1, &shape.children);
}

fn tell_list(out: &mut String, depth: usize, list: &ShapeList) {
    for shape in list.iter() {
        tell_shape(out, depth, shape);
    }
}

/// What a document holds of its masters and its instances, as lines to compare.
fn tell(doc: &Document) -> (String, String) {
    let mut masters = String::new();
    for m in doc.masters.iter() {
        let _ = writeln!(masters, "master {} name={:?} unique={:?} icon={:?}", m.id, m.name, m.unique_id, m.icon.is_some());
        tell_list(&mut masters, 1, &m.shapes);
    }
    let mut pages = String::new();
    for p in doc.pages.iter() {
        let _ = writeln!(pages, "page {} {:?}", p.id, p.name);
        tell_list(&mut pages, 1, &p.shapes);
    }
    (masters, pages)
}

fn save_as(s: &mut Session, path: &str) -> Result<serde_json::Value, String> {
    s.execute("file.save_as", &json!({ "path": path })).map_err(|e| e.to_string())
}

fn open_in(dir: &Dir, name: &str) -> Session {
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.path(name) }));
    s
}

/// The trip: the VSDX file, as `.diacraft`, as `.odg`, as `.diacraft` again and as VSDX. `Err` names the leg that was refused.
fn trip(bytes: &[u8], dir: &Dir) -> Result<Document, String> {
    trip_with_result(bytes, dir).map(|(back, _)| back)
}

/// [`trip`], and the result of the save as `.odg` (its `checked` field).
fn trip_with_result(bytes: &[u8], dir: &Dir) -> Result<(Document, serde_json::Value), String> {
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", bytes) }));
    save_as(&mut s, &dir.path("a.diacraft")).map_err(|e| format!("to .diacraft: {e}"))?;
    let mut s = open_in(dir, "a.diacraft");
    let odg = save_as(&mut s, &dir.path("b.odg")).map_err(|e| format!("to .odg: {e}"))?;
    let mut s = open_in(dir, "b.odg");
    save_as(&mut s, &dir.path("c.diacraft")).map_err(|e| format!("odg to .diacraft: {e}"))?;
    let mut s = open_in(dir, "c.diacraft");
    save_as(&mut s, &dir.path("d.vsdx")).map_err(|e| format!("to .vsdx: {e}"))?;
    let back = diacraft_vsdx::read(&dir.read("d.vsdx"), &diacraft_vsdx::Limits::default()).map(|(d, _)| d).map_err(|e| e.to_string())?;
    Ok((back, odg))
}

#[test]
fn core_fields_an_application_leaves_empty_do_not_refuse_the_save() {
    let dir = Dir::new();
    let bytes = boxes_builder().core(&core_empties()).build();
    let first = diacraft_vsdx::read(&bytes, &diacraft_vsdx::Limits::default()).unwrap().0;
    assert_eq!(first.props.subject.as_deref(), Some(""), "the reader holds an empty element as an empty text");
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &bytes) }));
    let out = save_as(&mut s, &dir.path("b.odg")).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(out["checked"]["classes"], json!([]), "nothing the check could name differs: {out}");
    // A read of the `.odg` gives the fields back as they were: an empty one is empty, and one that was not there is not.
    let odg = diacraft_odg::read(&dir.read("b.odg"), &diacraft_odg::Limits::default()).unwrap().0;
    let (was, now) = (&*first.props, &*odg.props);
    assert_eq!(
        (&now.subject, &now.keywords, &now.description, &now.creator, &now.created, &now.modified, &now.category),
        (&was.subject, &was.keywords, &was.description, &was.creator, &was.created, &was.modified, &was.category)
    );
    assert_eq!(now.language, None, "a field the file did not have stays out");
    // And the way back to a VSDX file.
    let back = trip(&bytes, &dir).unwrap_or_else(|e| panic!("{e}"));
    // The version of the application that wrote the file is the producer's: a file written here does not say another program's.
    let mut want = (*first.props).clone();
    want.app_version = back.props.app_version.clone();
    assert_eq!(*back.props, want, "the properties come back as the file had them");
}

/// The `.odg` written from a VSDX drawing, saved by a session of its own.
fn odg_of(bytes: &[u8], dir: &Dir) -> Vec<u8> {
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", bytes) }));
    save_as(&mut s, &dir.path("b.odg")).unwrap_or_else(|e| panic!("{e}"));
    dir.read("b.odg")
}

/// The first page's shape `id` of a document, at any depth.
fn shape_of(doc: &Document, id: u32) -> std::sync::Arc<Shape> {
    fn find(list: &ShapeList, id: u32) -> Option<std::sync::Arc<Shape>> {
        for shape in list.iter() {
            if shape.id == id {
                return Some(shape.clone());
            }
            if let Some(found) = find(&shape.children, id) {
                return Some(found);
            }
        }
        None
    }
    let page = doc.pages.iter().find(|p| !p.background).unwrap();
    find(&page.shapes, id).unwrap_or_else(|| panic!("no shape {id}"))
}

/// The characters of a shape's text.
fn chars_of(shape: &Shape) -> String {
    shape
        .text
        .iter()
        .flat_map(|t| t.parts.iter())
        .filter_map(|p| if let diacraft_doc::TextPart::Chars(c) = p { Some(c.to_string()) } else { None })
        .collect()
}

/// **An instance is written with what it shows**: the text and the formatting of the text its master gives it (a shape with no `Text` of its own shows its master's, and the rows of
/// `Character` it holds none of are its master's), so that another program, which has no master to ask, sees the shape's text as DiaCraft does. The instance with a text of its own
/// keeps it, and takes the size its master's row gives it.
#[test]
fn an_instance_is_written_with_the_text_and_the_formatting_its_master_gives_it() {
    let dir = Dir::new();
    let odg = odg_of(&texts_and_data(), &dir);
    let content = crate::odg_build::content_of(&odg);
    assert!(content.contains("master text"), "the instance that holds no text shows its master's: {content}");
    assert!(content.contains("own text"), "{content}");
    // The file read again: the instance holds the text, and the size and colour of the master's row.
    let read = diacraft_odg::read(&odg, &diacraft_odg::Limits::default()).unwrap().0;
    for id in [10, 11] {
        let shape = shape_of(&read, id);
        let section =
            shape.sheet.sections.iter().find(|s| s.name.as_str() == "Character").unwrap_or_else(|| panic!("shape {id} holds the rows of its text"));
        let size = section.rows.iter().find_map(|r| r.cells.get("Size")).and_then(|c| c.value.as_ref()?.number());
        assert!(size.is_some_and(|n| (n - 0.2).abs() < 1e-6), "shape {id}: the size of the master's row, {size:?}");
    }
    assert_eq!(chars_of(&shape_of(&read, 10)), "master text");
    assert_eq!(chars_of(&shape_of(&read, 11)), "own text");
}

/// **The text block an instance shows through its master**: a master that puts the text elsewhere than the whole shape gives the instance the cells of the block, and one whose block is the
/// default (the pin at the middle, the size of the shape, as a file states it) gives it none, so that no instance holds data the file's markup says without it.
#[test]
fn an_instance_holds_the_text_block_its_master_gives_it_when_it_is_not_the_whole_shape() {
    let block = |pin_y: &str| {
        format!(
            r#"<Cell N="TxtPinX" V="0.75" F="Width*0.5"/><Cell N="TxtPinY" V="{pin_y}" F="Height*0.5"/><Cell N="TxtWidth" V="1.5" F="Width*1"/><Cell N="TxtHeight" V="0.8" F="Height*1"/><Cell N="TxtLocPinX" V="0.75" F="TxtWidth*0.5"/><Cell N="TxtLocPinY" V="0.4" F="TxtHeight*0.5"/>"#
        )
    };
    let dir = Dir::new();
    // The default block: the instance is given none of the cells.
    let read = diacraft_odg::read(&odg_of(&master_with_a_text_block(&block("0.4")), &dir), &diacraft_odg::Limits::default()).unwrap().0;
    let shape = shape_of(&read, 10);
    assert_eq!(chars_of(&shape), "label");
    for name in ["TxtPinX", "TxtPinY", "TxtWidth", "TxtHeight", "TxtLocPinX", "TxtLocPinY"] {
        assert!(shape.sheet.cell(name).is_none(), "{name}: the block is the whole shape, and the file says it by having none");
    }
    // A block below the shape: the instance holds the cells, with the master's formulas.
    let below = r#"<Cell N="TxtPinX" V="0.75" F="Width*0.5"/><Cell N="TxtPinY" V="-0.3" F="Height*-0.375"/><Cell N="TxtWidth" V="1.5" F="Width*1"/><Cell N="TxtHeight" V="0.4" F="Height*0.5"/><Cell N="TxtLocPinX" V="0.75" F="TxtWidth*0.5"/><Cell N="TxtLocPinY" V="0.2" F="TxtHeight*0.5"/>"#;
    let read = diacraft_odg::read(&odg_of(&master_with_a_text_block(below), &dir), &diacraft_odg::Limits::default()).unwrap().0;
    let shape = shape_of(&read, 10);
    let pin_y = shape.sheet.cell("TxtPinY").and_then(|c| c.value.as_ref()?.number());
    assert!(pin_y.is_some_and(|n| (n + 0.3).abs() < 1e-6), "the text block of the master, {pin_y:?}");
}

/// **A picture an instance shows through its master is written as the picture it shows**: the frame is a `draw:image`.
#[test]
fn an_instance_of_a_picture_master_is_written_as_the_picture() {
    let dir = Dir::new();
    let odg = odg_of(&picture_master(), &dir);
    let content = crate::odg_build::content_of(&odg);
    assert!(content.contains("<draw:image"), "the instance shows its master's picture: {content}");
    let read = diacraft_odg::read(&odg, &diacraft_odg::Limits::default()).unwrap().0;
    let shape = shape_of(&read, 10);
    assert!(shape.foreign.as_ref().and_then(|f| f.picture.as_ref()).is_some(), "the file read again gives the shape its picture");
}

/// How many pixels of the first page differ (a channel by more than 24 of 255) between how DiaCraft draws the two drawings, and the number of channel values drawn: they are drawn at 40 pixels to the inch.
fn pixels_apart(a: &Document, b: &Document) -> (usize, usize) {
    use diacraft_render::{Renderer, View};
    let draw = |doc: &Document| {
        let page = doc.pages.iter().find(|p| !p.background).map(|p| p.id).unwrap();
        let view = View::fit_page(doc, page, 40.0).unwrap();
        Renderer::new().render(doc, page, &view)
    };
    let (x, y) = (draw(a), draw(b));
    assert_eq!((x.width, x.height), (y.width, y.height), "the page is drawn at one size");
    let apart = x.rgba_premul.iter().zip(y.rgba_premul.iter()).filter(|(p, q)| p.abs_diff(**q) > 24).count();
    (apart, x.rgba_premul.len())
}

/// The drawing a file holds, opened in a session as a person opens it.
fn opened_doc(dir: &Dir, name: &str) -> Document {
    let s = open_in(dir, name);
    s.document().unwrap().clone()
}

/// A shape as [`linked`] tells it: its links to a master, whether it is a group, and its cells with their values.
type Linked = (Option<u32>, Option<u32>, bool, Vec<(String, String)>);

/// The shapes of a page list in order, children after their parent: what a shape is linked to, whether it is a group, and the cells it holds with their values (numbers and texts).
fn linked(doc: &Document) -> Vec<Linked> {
    fn walk(list: &ShapeList, out: &mut Vec<Linked>) {
        for shape in list.iter() {
            let cells = shape
                .sheet
                .cells
                .iter()
                .map(|(n, c)| (n.as_str().to_owned(), c.value.as_ref().map(|v| v.text().to_owned()).unwrap_or_default()))
                .collect();
            out.push((shape.master, shape.master_shape, !shape.children.is_empty(), cells));
            walk(&shape.children, out);
        }
    }
    let mut out = Vec::new();
    for page in doc.pages.iter() {
        walk(&page.shapes, &mut out);
    }
    out
}

/// Whether two values of a cell are the same value: the same number (the files write them in other units and with other noise past nine places), or the same text without regard to case (a colour).
fn same_value(a: &str, b: &str) -> bool {
    match (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
        (Ok(x), Ok(y)) => (x - y).abs() <= 1e-6 * (1.0 + y.abs()),
        _ => a.eq_ignore_ascii_case(b),
    }
}

/// Validates a part of a package against the ODF 1.4 schema when `ODF_SCHEMA_RNG` names it (the OASIS RelaxNG file), DiaCraft's and LibreOffice's markup taken out: the check is skipped where it is not set.
fn assert_schema_valid(odg: &[u8], dir: &Dir, what: &str) {
    let Ok(rng) = std::env::var("ODF_SCHEMA_RNG") else { return };
    for part in ["content.xml", "styles.xml", "meta.xml"] {
        let text = crate::odg_build::part_of(odg, part);
        let mut root = diacraft_container::xml::parse(text.as_bytes()).unwrap();
        strip_extensions(&mut root);
        let file = dir.path(&format!("schema-{part}"));
        std::fs::write(&file, diacraft_container::xml::to_bytes(&root, b"", b"\n", true).unwrap()).unwrap();
        let run = std::process::Command::new("xmllint").arg("--noout").arg("--relaxng").arg(&rng).arg(&file).output().unwrap();
        assert!(run.status.success(), "{what}: {part} is not valid ODF 1.4: {}", String::from_utf8_lossy(&run.stderr));
    }
}

/// `el` with DiaCraft's and LibreOffice's extension markup taken out (their attributes and elements, by the prefixes `diacraft` and `loext`).
fn strip_extensions(el: &mut diacraft_container::xml::El) {
    let ours = |name: &str| ["diacraft:", "loext:", "xmlns:diacraft", "xmlns:loext"].iter().any(|p| name.starts_with(p));
    el.attrs.retain(|(k, _)| !ours(k));
    el.children.retain(|n| !matches!(n, diacraft_container::xml::Node::El(e) if ours(&e.name)));
    for n in &mut el.children {
        if let diacraft_container::xml::Node::El(e) = n {
            strip_extensions(e);
        }
    }
}

/// **Every kind of master crosses**: a VSDX drawing whose shapes are instances is written as an `.odg` that another program reads as ordinary shapes, and DiaCraft draws as it
/// draws the VSDX drawing; it validates against the ODF 1.4 schema; the check after the save names nothing; and the trip VSDX, `.diacraft`, `.odg`, `.diacraft`, VSDX gives the masters as they were
/// and every instance with its link to its master, its cells with the values they had.
#[test]
fn every_kind_of_master_is_written_as_an_odg_drawn_as_it_was_and_comes_back() {
    for (name, bytes) in vectors() {
        let dir = Dir::new();
        let first = diacraft_vsdx::read(&bytes, &diacraft_vsdx::Limits::default()).unwrap().0;
        let (back, out) = trip_with_result(&bytes, &dir).unwrap_or_else(|e| panic!("{name}: {e}"));
        // The check after the save names what differs: nothing, but for the frame formulas of a line, which a file says as the values the ends give.
        let named: Vec<&str> = out["checked"]["classes"].as_array().unwrap().iter().filter_map(|c| c["class"].as_str()).collect();
        let allowed: &[&str] = if name.starts_with("connection points") { &["formulas"] } else { &[] };
        assert_eq!(named, allowed, "{name}: what the check names: {out}");
        assert_schema_valid(&dir.read("b.odg"), &dir, name);
        // Drawn as it was: the `.odg` and the way back draw as the VSDX drawing does (the pixels that differ by more than rounding are fewer than one in a thousand).
        let (vsdx, odg, again) = (opened_doc(&dir, "a.vsdx"), opened_doc(&dir, "b.odg"), opened_doc(&dir, "d.vsdx"));
        for (what, other) in [("the .odg", &odg), ("the way back", &again)] {
            let (apart, total) = pixels_apart(&vsdx, other);
            assert!(apart * 1000 <= total, "{name}: {what} is drawn otherwise: {apart} of {total} values differ");
        }
        // The masters as they were.
        let (m1, _) = tell(&first);
        let (m2, _) = tell(&back);
        assert_eq!(m1, m2, "{name}: the masters");
        // Each instance with its links, and the cells it held with the values it held (the cells of its master it was written with are its own now: the same value).
        let (before, after) = (linked(&first), linked(&back));
        assert_eq!(
            before.iter().map(|(m, ms, g, _)| (*m, *ms, *g)).collect::<Vec<_>>(),
            after.iter().map(|(m, ms, g, _)| (*m, *ms, *g)).collect::<Vec<_>>(),
            "{name}: the shapes and their links to their masters"
        );
        for (k, ((_, _, _, was), (_, _, _, now))) in before.iter().zip(after.iter()).enumerate() {
            for (cell, value) in was {
                let found = now.iter().find(|(n, _)| n == cell);
                assert!(found.is_some_and(|(_, v)| same_value(v, value)), "{name}: shape {k}: the cell {cell} was {value:?} and is {found:?}");
            }
        }
    }
}

/// **A group whose frame is larger than the box of its members** (a group of a master, or of the page) is written as its members: a `draw:g` has no frame of its own. The save says so in words (the group's frame is not
/// kept, and the file reads it back as the box of the members), the check names what differs (the geometry of the shapes), and the file is written: nothing is lost that the format can hold.
#[test]
fn a_group_whose_frame_is_larger_than_its_members_is_written_as_their_box_and_the_save_says_so() {
    for (name, bytes) in [("a group of a master", roomy_group(true)), ("a group of the page", roomy_group(false))] {
        let dir = Dir::new();
        let mut s = pinned();
        run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &bytes) }));
        let out = save_as(&mut s, &dir.path("b.odg")).unwrap_or_else(|e| panic!("{name}: {e}"));
        let said = out["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|w| w.as_str())
            .any(|w| w.contains("the frame of group") && w.contains("is not the box of the group's members"));
        assert!(said, "{name}: the save says the frame is not kept: {out}");
        let named: Vec<&str> = out["checked"]["classes"].as_array().unwrap().iter().filter_map(|c| c["class"].as_str()).collect();
        assert!(named.contains(&"geometry"), "{name}: the check names the geometry: {out}");
    }
}

/// Groups of the page with no master, as a VSDX file holds them, none with connection points: a group of two rectangles with the attributes `attrs` (a name, or none), and beside it a group
/// that holds a group and a rectangle (each group's frame is the box of its members).
fn page_groups(attrs: &str) -> Vec<u8> {
    let pair = shape_with(
        20,
        &format!(r#" Type="Group"{attrs}"#),
        &format!("{}<Shapes>{}{}</Shapes>", pair_frame(2.0, 5.0), pair_member(21, 0.5, "#ff0000"), pair_member(22, 2.5, "#0000ff")),
    );
    let inner = shape_with(
        31,
        r#" Type="Group""#,
        &format!(
            r#"<Cell N="PinX" V="0.5"/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="2"/><Cell N="LocPinX" V="0.5"/><Cell N="LocPinY" V="1"/><Shapes>{}</Shapes>"#,
            pair_member(32, 0.5, "#00ff00")
        ),
    );
    let outer = shape_with(30, r#" Type="Group""#, &format!("{}<Shapes>{inner}{}</Shapes>", pair_frame(6.0, 5.0), pair_member(33, 2.5, "#0000ff")));
    base().page(PageSpec::new(1, "Page-1").sheet(page_sheet()).shapes(&format!("{pair}{outer}"))).build()
}

/// Whether each group of a document's pages holds a `Connection` section (connection points), in the order of the pages and the pre-order of each page.
fn group_connections(doc: &Document) -> Vec<bool> {
    fn walk(list: &ShapeList, out: &mut Vec<bool>) {
        for shape in list.iter() {
            if !shape.children.is_empty() {
                out.push(shape.sheet.sections.iter().any(|s| s.name.as_str() == "Connection"));
                walk(&shape.children, out);
            }
        }
    }
    let mut out = Vec::new();
    for page in doc.pages.iter() {
        walk(&page.shapes, &mut out);
    }
    out
}

/// **A group with no connection points comes back from an `.odg` with none** (the full gate on `786463e8`): the `.odg` reader gives every shape it reads the four standard
/// connection points, groups included, and a `draw:g` has no style to say that it has none, as another shape's style does: the group's entry in DiaCraft's document
/// file says it. Before, a group of a VSDX drawing (an instance of a group master, a group of the page, a group in a group) and a group made in DiaCraft were read back from the `.odg` with
/// four connection points they did not have, and the check after the save named them ("4 connections differ"). Now the file read back gives each group none, the check names no connection,
/// and the file, opened, edited and saved into, keeps it so.
#[test]
fn a_group_with_no_connection_points_comes_back_from_an_odg_with_none() {
    for (name, bytes) in [
        ("an instance of a group master", group_master()),
        ("groups of the page with no name", page_groups("")),
        ("groups of the page, one with a name", page_groups(r#" NameU="Pair" Name="Pair""#)),
    ] {
        let dir = Dir::new();
        let mut s = pinned();
        run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &bytes) }));
        let want = group_connections(s.document().unwrap());
        assert!(!want.is_empty() && want.iter().all(|has| !has), "{name}: the drawing's groups have no connection points: {want:?}");
        let out = save_as(&mut s, &dir.path("b.odg")).unwrap_or_else(|e| panic!("{name}: {e}"));
        let named: Vec<&str> = out["checked"]["classes"].as_array().unwrap().iter().filter_map(|c| c["class"].as_str()).collect();
        assert!(!named.contains(&"connections"), "{name}: the check names no connection: {out}");
        assert_eq!(group_connections(&opened_doc(&dir, "b.odg")), want, "{name}: the groups read back from the .odg");
        // The `.odg` opened, edited and saved into: no refusal, and the groups still have none.
        let mut s = open_in(&dir, "b.odg");
        let first = s.document().unwrap().pages.iter().next().unwrap().shapes.iter().next().unwrap().id;
        run(&mut s, "shape.move", json!({ "ids": [first], "dx": 0.5, "dy": 0.0 }));
        run(&mut s, "file.save", json!({}));
        assert_eq!(group_connections(&opened_doc(&dir, "b.odg")), want, "{name}: the groups read back after a save of an edit");
    }
    // A group made in DiaCraft holds no connection points, and the `.odg` gives it none.
    let dir = Dir::new();
    let mut s = pinned();
    let a = crate::edit::drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = crate::edit::drop_shape(&mut s, "rectangle", 4.0, 2.0);
    run(&mut s, "shape.group", json!({ "ids": [a, b] }));
    let want = group_connections(s.document().unwrap());
    assert_eq!(want, [false], "a group made in DiaCraft has no connection points");
    let out = save_as(&mut s, &dir.path("b.odg")).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(group_connections(&opened_doc(&dir, "b.odg")), want, "the group made in DiaCraft, read back from the .odg: {out}");
}

/// **A connector with no connection points comes back from an `.odg` with none, and in the route it was drawn in**: the reader gives a connector the four standard connection
/// points too, so its style says it has none; and the `.odg` writer writes the route a connector is drawn in (here the right angles the page gives one that holds no route cells
/// of its own) as `draw:type`, which reads back as `ShapeRouteStyle` 1 and `ConLineRouteExt` 1 on the connector: the same route. Before, the check after the save named both ("4 connections
/// differ", "1 shape differs"), behind the group master's failure in the same test.
#[test]
fn a_connector_with_no_connection_points_comes_back_from_an_odg_with_none_in_its_route() {
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &glued()) }));
    let connector = shape_of(s.document().unwrap(), 12);
    let held = |shape: &Shape, name: &str| shape.sheet.cell(name).and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned());
    assert!(!connector.sheet.sections.iter().any(|x| x.name.as_str() == "Connection"), "the connector holds no connection points");
    assert_eq!((held(&connector, "ShapeRouteStyle"), held(&connector, "ConLineRouteExt")), (None, None), "nor a route of its own");
    let out = save_as(&mut s, &dir.path("b.odg")).unwrap_or_else(|e| panic!("{e}"));
    let named: Vec<&str> = out["checked"]["classes"].as_array().unwrap().iter().filter_map(|c| c["class"].as_str()).collect();
    assert_eq!(named, ["formulas"], "the frame formulas of the line only: {out}");
    let back = shape_of(&opened_doc(&dir, "b.odg"), 12);
    assert!(!back.sheet.sections.iter().any(|x| x.name.as_str() == "Connection"), "the connector read back holds none: {:?}", back.sheet.sections);
    assert_eq!(
        (held(&back, "ShapeRouteStyle"), held(&back, "ConLineRouteExt")),
        (Some("1".to_owned()), Some("1".to_owned())),
        "right angles, said on it"
    );
}

/// **Rectangles joined by connectors, saved as an `.odg`, lose nothing** (from the control): the check after the save names no class for two rectangles joined by a
/// straight connector, or by one routed with right angles, as `connector.add` makes them. Before, every such connector was named for its connection points (the reader's four), its route
/// (said on it), its line end (the size it is drawn with, 2, said on it), the formulas and values of its label (its text block's default formulas, and the label the engine writes for a
/// connector with no text) and its outline (`NoFill`, which is now carried); and the connector read back holds no connection points and its outline's `NoFill`.
#[test]
fn rectangles_joined_by_connectors_saved_as_an_odg_lose_nothing() {
    for (name, to) in [("straight", (5.0, 2.0)), ("with right angles", (5.0, 5.0))] {
        let dir = Dir::new();
        let mut s = pinned();
        let a = crate::edit::drop_shape(&mut s, "rectangle", 2.0, 2.0);
        let b = crate::edit::drop_shape(&mut s, "rectangle", to.0, to.1);
        let c = crate::edit::id_of(&run(&mut s, "connector.add", json!({ "from": { "id": a }, "to": { "id": b } })));
        let out = save_as(&mut s, &dir.path("b.odg")).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(out["checked"]["classes"], json!([]), "{name}: the check names nothing: {out}");
        let back = shape_of(&opened_doc(&dir, "b.odg"), c);
        assert!(!back.sheet.sections.iter().any(|x| x.name.as_str() == "Connection"), "{name}: no connection points: {:?}", back.sheet.sections);
        let outline = back.sheet.sections.iter().find(|x| x.name.as_str() == "Geometry").unwrap();
        assert_eq!(
            outline.cells.get("NoFill").and_then(|v| v.value.as_ref()).map(|v| v.text().to_owned()).as_deref(),
            Some("1"),
            "{name}: the outline is not filled"
        );
    }
}

/// **What a save still refuses is said in words, and nothing is written** (the counts of the report): each drawing below is refused with the words that name the cause, by the plain writer
/// and the same through a `.diacraft` file.
#[test]
fn what_a_save_as_an_odg_still_refuses_is_said_in_words_and_nothing_is_written() {
    for (name, bytes, words) in refused_vectors() {
        let dir = Dir::new();
        let mut s = pinned();
        run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &bytes) }));
        let e = save_as(&mut s, &dir.path("b.odg")).unwrap_err();
        assert!(e.contains(words) && e.contains("Nothing was written"), "{name}: {e}");
        assert!(!std::path::Path::new(&dir.path("b.odg")).exists(), "{name}: nothing is written");
    }
}

/// Says, for each vector of this file (those that cross, those that are refused and the two groups with a frame larger than their members), whether the trip VSDX, `.diacraft`, `.odg`, `.diacraft`, VSDX is
/// made or refused, and whether the `.odg` holds the text and the picture the instances show (by hand: `--run-ignored only -E 'test(crossing_counts)' --no-capture`): the counts of the report.
#[test]
#[ignore = "counts by hand"]
fn crossing_counts() {
    let mut all: Vec<(String, Vec<u8>)> = vectors().into_iter().map(|(n, b)| (n.to_owned(), b)).collect();
    all.extend(refused_vectors().into_iter().map(|(n, b, _)| (n.to_owned(), b)));
    all.push(("a group of a master whose frame is larger than its members".to_owned(), roomy_group(true)));
    all.push(("a group of the page whose frame is larger than its members".to_owned(), roomy_group(false)));
    let mut crossed = 0;
    for (name, bytes) in &all {
        let dir = Dir::new();
        match trip_with_result(bytes, &dir) {
            Ok(_) => {
                crossed += 1;
                let content = crate::odg_build::content_of(&dir.read("b.odg"));
                println!(
                    "COUNT crossed: {name} (the instances text in the .odg: {}, the picture: {})",
                    content.contains("master text"),
                    content.contains("<draw:image")
                );
            }
            Err(why) => println!("COUNT refused: {name}: {}", why.chars().take(140).collect::<String>()),
        }
    }
    println!("COUNT {crossed} of {} cross", all.len());
}

// ---------------------------------------------------------------------------------------------------------------- the files LibreOffice is shown

/// Writes the files that a LibreOffice run is given into the folder `MASTERS_LO_DIR` names (by hand: `MASTERS_LO_DIR=<folder> cargo nextest run -p diacraft-engine --run-ignored only -E 'test(lo_files)'`): for each
/// vector the VSDX drawing, the `.odg` it is saved as, the picture DiaCraft draws of each, and the VSDX the trip gives back. Returns at once with no folder named.
#[test]
#[ignore = "writes files for a LibreOffice run, by hand"]
fn lo_files() {
    let Some(folder) = std::env::var_os("MASTERS_LO_DIR") else { return };
    let folder = std::path::PathBuf::from(folder);
    std::fs::create_dir_all(&folder).unwrap();
    for (n, (name, bytes)) in vectors().into_iter().enumerate() {
        let dir = Dir::new();
        let (_, _) = trip_with_result(&bytes, &dir).unwrap_or_else(|e| panic!("{name}: {e}"));
        for (file, to) in [("a.vsdx", "vsdx"), ("b.odg", "odg"), ("d.vsdx", "back.vsdx")] {
            std::fs::write(folder.join(format!("v{n:02}.{to}")), dir.read(file)).unwrap();
        }
        for (file, tag) in [("a.vsdx", "vsdx"), ("b.odg", "odg")] {
            let mut session = open_in(&dir, file);
            let target = folder.join(format!("v{n:02}-diacraft-{tag}.png"));
            session.execute("file.export", &json!({ "path": target.to_str().unwrap(), "dpi": 48 })).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
        println!("LOFILE v{n:02} {name}");
    }
}

// ---------------------------------------------------------------------------------------------------------------- the scale

/// `count` instances of the master "Process" on one page, each at a place of its own with a fill of its own, as a package; or, with `plain`, `count` shapes that hold what an instance shows
/// (the yardstick: the same shapes with no master).
fn many(count: u32, plain: bool) -> Vec<u8> {
    let mut page = String::new();
    for k in 0..count {
        let (x, y) = (1.0 + f64::from(k % 20) * 0.5, 1.0 + f64::from(k / 20 % 40) * 0.25);
        if plain {
            let inner = format!(
                r##"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="{y}"/><Cell N="FillForegnd" V="#00aa00"/>{}"##,
                crate::stencil_open::box_cells(1.5, 0.8)
            );
            page.push_str(&shape_with(
                k + 10,
                "",
                &inner.replace(r#"<Cell N="PinX" V="0.75" F="Width*0.5"/><Cell N="PinY" V="0.4" F="Height*0.5"/>"#, ""),
            ));
        } else {
            page.push_str(&shape_with(
                k + 10,
                r#" Master="1""#,
                &format!(r##"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="{y}"/><Cell N="FillForegnd" V="#00aa00"/>"##),
            ));
        }
    }
    let mut b = base().page(PageSpec::new(1, "Page-1").sheet(page_sheet()).shapes(&page));
    if !plain {
        b = b.master(process_master());
    }
    b.build()
}

/// A rectangle member of a group, `w` by `h`, with its pin at (`x`, `y`) in the group's frame, and the rows of its outline.
fn member(id: u32, attrs: &str, x: f64, y: f64, w: f64, h: f64) -> String {
    shape_with(
        id,
        &format!(r#" Type="Shape"{attrs}"#),
        &format!(
            concat!(
                r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="{y}"/><Cell N="Width" V="{w}"/><Cell N="Height" V="{h}"/><Cell N="LocPinX" V="{lx}" F="Width*0.5"/><Cell N="LocPinY" V="{ly}" F="Height*0.5"/>"#,
                r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="{w}" F="Width*1"/><Cell N="Y" V="0" F="Height*0"/></Row>"#,
                r#"<Row T="LineTo" IX="3"><Cell N="X" V="{w}" F="Width*1"/><Cell N="Y" V="{h}" F="Height*1"/></Row><Row T="LineTo" IX="4"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="{h}" F="Height*1"/></Row>"#,
                r#"<Row T="LineTo" IX="5"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row></Section>"#
            ),
            x = x,
            y = y,
            w = w,
            h = h,
            lx = w / 2.0,
            ly = h / 2.0
        ),
    )
}

/// A group of the frame `w` by `h` with its pin at (`x`, `y`), holding `inner`.
fn group(id: u32, attrs: &str, x: f64, y: f64, w: f64, h: f64, inner: &str) -> String {
    shape_with(
        id,
        &format!(r#" Type="Group"{attrs}"#),
        &format!(
            r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="{y}"/><Cell N="Width" V="{w}"/><Cell N="Height" V="{h}"/><Cell N="LocPinX" V="{lx}"/><Cell N="LocPinY" V="{ly}"/><Shapes>{inner}</Shapes>"#,
            lx = w / 2.0,
            ly = h / 2.0
        ),
    )
}

/// A master that is `depth` groups, one in the other, round a rectangle, all of one frame (2 by 1), and `instances` instances of it on the page: each group of an instance names the group of the master that it is.
fn deep(depth: u32, instances: u32) -> Vec<u8> {
    let mut master = member(1000, "", 1.0, 0.5, 2.0, 1.0);
    for level in (1..=depth).rev() {
        master = group(level, "", 1.0, 0.5, 2.0, 1.0, &master);
    }
    let mut page = String::new();
    for k in 0..instances {
        let first = 10_000 + k * (depth + 2);
        let mut inner = shape_with(first + depth + 1, r#" Type="Shape" MasterShape="1000""#, "");
        for level in (2..=depth).rev() {
            inner = shape_with(first + level, &format!(r#" Type="Group" MasterShape="{level}""#), &format!("<Shapes>{inner}</Shapes>"));
        }
        let (x, y) = (1.5 + f64::from(k % 10) * 2.5, 1.0 + f64::from(k / 10) * 1.5);
        let top = if depth == 0 { inner } else { format!("<Shapes>{inner}</Shapes>") };
        page.push_str(&shape_with(first + 1, r#" Type="Group" Master="7""#, &format!(r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="{y}"/>{top}"#)));
    }
    base().master(MasterSpec::new(7, "Deep").shapes(&master)).page(PageSpec::new(1, "Page-1").sheet(page_sheet()).shapes(&page)).build()
}

/// A master that is one group of `members` rectangles on a grid, and `instances` instances of it.
fn wide(members: u32, instances: u32) -> Vec<u8> {
    let columns = 100u32;
    let rows = members.div_ceil(columns);
    let (w, h) = (f64::from(columns) * 0.2, f64::from(rows) * 0.2);
    let mut inner = String::new();
    for m in 0..members {
        inner.push_str(&member(10 + m, "", f64::from(m % columns) * 0.2 + 0.1, f64::from(m / columns) * 0.2 + 0.1, 0.2, 0.2));
    }
    let master = group(5, "", w / 2.0, h / 2.0, w, h, &inner);
    let mut page = String::new();
    for k in 0..instances {
        let first = 100_000 + k * (members + 1);
        let mut kids = String::new();
        for m in 0..members {
            kids.push_str(&shape_with(first + 1 + m, &format!(r#" Type="Shape" MasterShape="{}""#, 10 + m), ""));
        }
        page.push_str(&shape_with(
            first,
            r#" Type="Group" Master="7""#,
            &format!(r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="4"/><Shapes>{kids}</Shapes>"#, x = 11.0 + f64::from(k) * (w + 1.0)),
        ));
    }
    base().master(MasterSpec::new(7, "Wide").shapes(&master)).page(PageSpec::new(1, "Page-1").sheet(page_sheet()).shapes(&page)).build()
}

fn measured(name: &str, bytes: &[u8]) {
    let dir = Dir::new();
    let mut s = pinned();
    let begin = std::time::Instant::now();
    let opened = s.execute("file.open", &json!({ "path": dir.write("a.vsdx", bytes) })).map_err(|e| e.to_string());
    let t_open = begin.elapsed();
    match opened {
        Err(e) => println!(
            "SCALE {name}: {} bytes; open refused after {:.2}s: {}",
            bytes.len(),
            t_open.as_secs_f64(),
            e.chars().take(200).collect::<String>()
        ),
        Ok(_) => {
            let begin = std::time::Instant::now();
            let r = save_as(&mut s, &dir.path("b.odg")).map(|v| v["bytes"].as_u64().unwrap_or(0) as usize);
            println!(
                "SCALE {name}: {} bytes; open {:.2}s; save {:.2}s {:?}",
                bytes.len(),
                t_open.as_secs_f64(),
                begin.elapsed().as_secs_f64(),
                r.map_err(|e| e.chars().take(300).collect::<String>())
            );
        }
    }
}

/// A master whose text has `rows` rows of `Character` (a run uses the first), and `count` instances that show it.
fn heavy_text(rows: u32, count: u32) -> Vec<u8> {
    let mut section = String::from(r#"<Section N="Character">"#);
    for ix in 0..rows {
        section.push_str(&format!(r#"<Row IX="{ix}"><Cell N="Size" V="0.{}"/></Row>"#, 1 + ix % 8));
    }
    section.push_str("</Section>");
    let master = format!("{}{section}<Text><cp IX=\"0\"/>heavy</Text>", crate::stencil_open::box_cells(1.5, 0.8));
    let mut page = String::new();
    for k in 0..count {
        page.push_str(&shape_with(
            k + 10,
            r#" Master="1""#,
            &format!(r#"<Cell N="PinX" V="{}"/><Cell N="PinY" V="{}"/>"#, 1.0 + f64::from(k % 20) * 0.5, 1.0 + f64::from(k / 20 % 40) * 0.25),
        ));
    }
    base()
        .master(MasterSpec::new(1, "Heavy").shapes(&shape_with(5, "", &master)))
        .page(PageSpec::new(1, "P").sheet(page_sheet()).shapes(&page))
        .build()
}

#[test]
#[ignore = "measures by hand"]
fn measure_heavy() {
    for (rows, count) in [(1u32, 2_000u32), (500, 2_000), (5_000, 200), (5_000, 2_000)] {
        measured(&format!("{count} instances of a master with {rows} rows of Character"), &heavy_text(rows, count));
    }
}

#[test]
#[ignore = "measures by hand"]
fn measure_scale() {
    for count in [5_000u32, 20_000, 40_000, 60_000, 100_000] {
        measured(&format!("{count} instances of one master"), &many(count, false));
    }
    for depth in [10u32, 50, 100, 190] {
        measured(&format!("deep master depth {depth}, 1 instance"), &deep(depth, 1));
        measured(&format!("deep master depth {depth}, 100 instances"), &deep(depth, 100));
    }
    for members in [100u32, 1_000, 5_000, 10_000] {
        measured(&format!("wide master {members} members, 1 instance"), &wide(members, 1));
    }
    measured("wide master 10,000 members, 3 instances", &wide(10_000, 3));
}
