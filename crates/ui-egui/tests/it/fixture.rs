//! Drawings built in code, as packages (`.vsdx` bytes), for the tests that open a file and for the run of the real window.
//!
//! Nothing here is a real file or comes from one: the XML is written from the specs, by the package builder of `testkit`. The
//! drawing has what the owner's first look should show: several pages (one a background), shapes of several kinds, text, an
//! instance of a master, a group, a gradient, and a connector with an arrowhead.

use std::path::Path;

use diacraft_testkit::package::{MasterSpec, PageSpec, Pkg, VsdxBuilder, cell, cell_f, shape, shape_with};

/// What the root style holds in every real file, as far as drawing goes: a solid white fill, a thin black line, no shadow, centred
/// text with no margins.
const ROOT_STYLE: &str = r##"<StyleSheet ID="0" NameU="No Style" Name="No Style"><Cell N="EnableFillProps" V="1"/><Cell N="EnableLineProps" V="1"/><Cell N="EnableTextProps" V="1"/><Cell N="FillPattern" V="1"/><Cell N="FillForegnd" V="#FFFFFF"/><Cell N="FillBkgnd" V="#000000"/><Cell N="FillForegndTrans" V="0"/><Cell N="FillBkgndTrans" V="0"/><Cell N="LinePattern" V="1"/><Cell N="LineColor" V="#000000"/><Cell N="LineColorTrans" V="0"/><Cell N="LineWeight" V="0.01"/><Cell N="LineCap" V="0"/><Cell N="Rounding" V="0"/><Cell N="BeginArrow" V="0"/><Cell N="EndArrow" V="0"/><Cell N="BeginArrowSize" V="2"/><Cell N="EndArrowSize" V="2"/><Cell N="ShdwPattern" V="0"/><Cell N="ShapeShdwType" V="0"/><Cell N="ShapeShdwScaleFactor" V="1"/><Cell N="ShapeShdwOffsetX" V="0"/><Cell N="ShapeShdwOffsetY" V="0"/><Cell N="ShapeShdwShow" V="2"/><Cell N="FillGradientEnabled" V="0"/><Cell N="LineGradientEnabled" V="0"/><Cell N="RotateGradientWithShape" V="1"/><Cell N="DisplayMode" V="2"/><Cell N="VerticalAlign" V="1"/><Cell N="LeftMargin" V="0"/><Cell N="RightMargin" V="0"/><Cell N="TopMargin" V="0"/><Cell N="BottomMargin" V="0"/><Cell N="DefaultTabStop" V="0.5"/><Cell N="TextBkgnd" V="0"/><Cell N="TextBkgndTrans" V="0"/></StyleSheet>"##;

/// The page sheet of a page `w` by `h` inches at scale one to one.
fn page_sheet(w: f64, h: f64) -> String {
    [cell("PageWidth", &w.to_string()), cell("PageHeight", &h.to_string()), cell("PageScale", "1"), cell("DrawingScale", "1")].concat()
}

/// The cells that place a shape: its pin at its lower left corner `(x, y)` and its size.
pub fn placed(x: f64, y: f64, w: f64, h: f64) -> String {
    [
        cell("PinX", &x.to_string()),
        cell("PinY", &y.to_string()),
        cell("Width", &w.to_string()),
        cell("Height", &h.to_string()),
        cell("LocPinX", "0"),
        cell("LocPinY", "0"),
    ]
    .concat()
}

/// A rectangle outline `w` by `h` as geometry section 0.
pub fn rectangle(w: f64, h: f64) -> String {
    let row =
        |t: &str, ix: u32, x: f64, y: f64| format!(r#"<Row T="{t}" IX="{ix}">{}{}</Row>"#, cell("X", &x.to_string()), cell("Y", &y.to_string()));
    format!(
        r#"<Section N="Geometry" IX="0">{}{}{}{}{}{}</Section>"#,
        cell("NoFill", "0"),
        row("MoveTo", 1, 0.0, 0.0),
        row("LineTo", 2, w, 0.0),
        row("LineTo", 3, w, h),
        row("LineTo", 4, 0.0, h),
        row("LineTo", 5, 0.0, 0.0)
    )
}

/// A diamond `w` by `h`.
fn diamond(w: f64, h: f64) -> String {
    let row =
        |t: &str, ix: u32, x: f64, y: f64| format!(r#"<Row T="{t}" IX="{ix}">{}{}</Row>"#, cell("X", &x.to_string()), cell("Y", &y.to_string()));
    format!(
        r#"<Section N="Geometry" IX="0">{}{}{}{}{}{}</Section>"#,
        cell("NoFill", "0"),
        row("MoveTo", 1, w / 2.0, 0.0),
        row("LineTo", 2, w, h / 2.0),
        row("LineTo", 3, w / 2.0, h),
        row("LineTo", 4, 0.0, h / 2.0),
        row("LineTo", 5, w / 2.0, 0.0)
    )
}

/// An ellipse `w` by `h`, as the ellipse row: the centre, then a point on each axis.
fn ellipse(w: f64, h: f64) -> String {
    format!(
        r#"<Section N="Geometry" IX="0">{}<Row T="Ellipse" IX="1">{}{}{}{}{}{}</Row></Section>"#,
        cell("NoFill", "0"),
        cell("X", &(w / 2.0).to_string()),
        cell("Y", &(h / 2.0).to_string()),
        cell("A", &w.to_string()),
        cell("B", &(h / 2.0).to_string()),
        cell("C", &(w / 2.0).to_string()),
        cell("D", &h.to_string())
    )
}

/// A horizontal line of length `w`, no fill: a connector's path.
fn line(w: f64) -> String {
    format!(
        r#"<Section N="Geometry" IX="0">{}<Row T="MoveTo" IX="1">{}{}</Row><Row T="LineTo" IX="2">{}{}</Row></Section>"#,
        cell("NoFill", "1"),
        cell("X", "0"),
        cell("Y", "0"),
        cell("X", &w.to_string()),
        cell("Y", "0")
    )
}

/// A label: text `s` at `size` inches in dark grey.
fn text(s: &str, size: f64) -> String {
    format!(
        r#"<Section N="Character"><Row IX="0">{}{}</Row></Section><Text><cp IX="0"/>{}</Text>"#,
        cell("Size", &size.to_string()),
        cell("Color", "#1D2330"),
        s
    )
}

/// Page 1, "Overview": a titled box, a diamond, an ellipse, a gradient, a group, an instance of a master and a connector with an
/// arrowhead between two boxes. Its background page is "Frame".
fn overview() -> String {
    let start = shape(
        1,
        &[
            placed(0.8, 6.4, 2.2, 1.2),
            cell("FillForegnd", "#BDD7EE"),
            cell("LineColor", "#2E75B6"),
            cell("LineWeight", "0.03"),
            rectangle(2.2, 1.2),
            text("Start", 0.2),
        ]
        .concat(),
    );
    let decision = shape(
        2,
        &[
            placed(4.4, 6.2, 2.0, 1.6),
            cell("FillForegnd", "#FFE699"),
            cell("LineColor", "#BF9000"),
            cell("LineWeight", "0.03"),
            diamond(2.0, 1.6),
            text("Ready?", 0.18),
        ]
        .concat(),
    );
    let finish = shape(
        3,
        &[
            placed(7.8, 6.4, 2.2, 1.2),
            cell("FillForegnd", "#C5E0B4"),
            cell("LineColor", "#548235"),
            cell("LineWeight", "0.03"),
            ellipse(2.2, 1.2),
            text("Done", 0.2),
        ]
        .concat(),
    );
    // A connector from the box to the diamond, with an arrowhead at its end, and one on to the ellipse.
    let first = shape(
        4,
        &[
            cell("BeginX", "3"),
            cell("BeginY", "7"),
            cell("EndX", "4.4"),
            cell("EndY", "7"),
            placed(3.0, 7.0, 1.4, 0.0),
            cell("LineWeight", "0.02"),
            cell("LineColor", "#404040"),
            cell("EndArrow", "4"),
            cell("EndArrowSize", "3"),
            line(1.4),
        ]
        .concat(),
    );
    let second = shape(
        5,
        &[
            cell("BeginX", "6.4"),
            cell("BeginY", "7"),
            cell("EndX", "7.8"),
            cell("EndY", "7"),
            placed(6.4, 7.0, 1.4, 0.0),
            cell("LineWeight", "0.02"),
            cell("LineColor", "#404040"),
            cell("EndArrow", "4"),
            cell("EndArrowSize", "3"),
            line(1.4),
        ]
        .concat(),
    );
    // A rectangle with a red to blue gradient.
    let gradient = shape(
        6,
        &[
            placed(0.8, 3.4, 3.0, 2.0),
            cell("FillGradientEnabled", "1"),
            cell("FillGradientDir", "0"),
            cell("FillGradientAngle", "0"),
            cell("LineWeight", "0.02"),
            rectangle(3.0, 2.0),
            r##"<Section N="FillGradient"><Row IX="0"><Cell N="GradientStopColor" V="#E8453C"/><Cell N="GradientStopColorTrans" V="0"/><Cell N="GradientStopPosition" V="0"/></Row><Row IX="1"><Cell N="GradientStopColor" V="#2D6CDF"/><Cell N="GradientStopColorTrans" V="0"/><Cell N="GradientStopPosition" V="1"/></Row></Section>"##
                .to_owned(),
        ]
        .concat(),
    );
    // A group of a box and a smaller ellipse, with a label of its own.
    let group = shape_with(
        7,
        r#" Type="Group""#,
        &format!(
            "{}{}<Shapes>{}{}</Shapes>",
            placed(4.6, 3.4, 2.4, 2.0),
            text("A group", 0.16),
            shape(8, &[placed(0.0, 0.0, 2.4, 2.0), cell("FillForegnd", "#E4DFEC"), rectangle(2.4, 2.0)].concat()),
            shape(9, &[placed(0.5, 0.4, 1.4, 1.0), cell("FillForegnd", "#B2A1C7"), ellipse(1.4, 1.0)].concat()),
        ),
    );
    // An instance of the master "Rounded box", larger than the master: it keeps the master's rounding, line and label, and its own
    // geometry follows its own size, as the rows of an instance that was resized are written.
    let instance =
        shape_with(10, r#" Master="1""#, &[placed(7.6, 3.4, 2.6, 1.6), cell("FillForegnd", "#F8CBAD"), following_rectangle(2.6, 1.6)].concat());
    format!("{start}{decision}{finish}{first}{second}{gradient}{group}{instance}")
}

/// A rectangle `w` by `h` whose rows follow the shape's width and height (`Width*1`), the cached values being those of this size.
fn following_rectangle(w: f64, h: f64) -> String {
    let row = |t: &str, ix: u32, x: f64, y: f64, fx: &str, fy: &str| {
        format!(r#"<Row T="{t}" IX="{ix}">{}{}</Row>"#, cell_f("X", &x.to_string(), fx), cell_f("Y", &y.to_string(), fy))
    };
    format!(
        r#"<Section N="Geometry" IX="0">{}{}{}{}{}{}</Section>"#,
        cell("NoFill", "0"),
        row("MoveTo", 1, 0.0, 0.0, "Width*0", "Height*0"),
        row("LineTo", 2, w, 0.0, "Width*1", "Height*0"),
        row("LineTo", 3, w, h, "Width*1", "Height*1"),
        row("LineTo", 4, 0.0, h, "Width*0", "Height*1"),
        row("LineTo", 5, 0.0, 0.0, "Width*0", "Height*0")
    )
}

/// The master "Rounded box": a rectangle with rounded corners whose geometry follows its width and height, with a label.
fn rounded_box() -> String {
    shape(
        1,
        &[
            cell_f("Width", "1.5", "1.5"),
            cell_f("Height", "1", "1"),
            cell("LocPinX", "0"),
            cell("LocPinY", "0"),
            cell("Rounding", "0.15"),
            cell("FillForegnd", "#D9D9D9"),
            cell("LineColor", "#7F7F7F"),
            cell("LineWeight", "0.03"),
            following_rectangle(1.5, 1.0),
            text("Master", 0.18),
        ]
        .concat(),
    )
}

/// Page 2, "Details": a table of boxes and a long label.
fn details() -> String {
    let mut out = String::new();
    for (i, fill) in ["#F4B183", "#FFD966", "#A9D18E", "#9DC3E6"].into_iter().enumerate() {
        let x = 0.7 + 2.4 * i as f64;
        out.push_str(&shape(
            20 + i as u32,
            &[
                placed(x, 1.0, 2.0, 5.0),
                cell("FillForegnd", fill),
                cell("LineWeight", "0.02"),
                rectangle(2.0, 5.0),
                text(&format!("Column {}", i + 1), 0.2),
            ]
            .concat(),
        ));
    }
    out
}

/// A background page's frame: a rectangle round the page's edge with no fill.
fn frame() -> String {
    shape(
        30,
        &[placed(0.25, 0.25, 10.5, 8.0), cell("FillPattern", "0"), cell("LineColor", "#7F7F7F"), cell("LineWeight", "0.02"), rectangle(10.5, 8.0)]
            .concat(),
    )
}

/// The windows part: the grid and the rulers on, as a file that was saved so would hold them.
pub const WINDOWS_PART: &str = r#"<?xml version="1.0" encoding="utf-8"?><Windows ClientWidth="1280" ClientHeight="720"><Window ID="0" WindowType="Drawing" WindowState="1073741824" ViewScale="-1"><ShowRulers>1</ShowRulers><ShowGrid>1</ShowGrid></Window></Windows>"#;

/// The same drawing with `windows` as its windows part, or none.
fn with_windows(builder: VsdxBuilder, windows: Option<&str>) -> VsdxBuilder {
    match windows {
        None => builder,
        Some(xml) => builder.with(Pkg::new().part("/visio/windows.xml", "application/vnd.ms-visio.windows+xml", xml).rel(
            "/visio/document.xml",
            "rId90",
            "http://schemas.microsoft.com/visio/2010/relationships/windows",
            "windows.xml",
        )),
    }
}

/// The drawing: pages "Overview" (with the background "Frame"), the background page "Frame" second in the file's order (the tabs
/// put it last), "Details" and "Notes" (empty, 6 by 4 inches).
pub fn demo_builder(windows: Option<&str>) -> VsdxBuilder {
    let builder = VsdxBuilder::new()
        .styles(ROOT_STYLE)
        .master(MasterSpec::new(1, "Rounded box").shapes(&rounded_box()))
        .page(PageSpec::new(1, "Overview").sheet(&page_sheet(11.0, 8.5)).attrs(r#" BackPage="4""#).shapes(&overview()))
        .page(PageSpec::new(4, "Frame").sheet(&page_sheet(11.0, 8.5)).attrs(r#" Background="1""#).shapes(&frame()))
        .page(PageSpec::new(2, "Details").sheet(&page_sheet(11.0, 8.5)).shapes(&details()))
        .page(PageSpec::new(3, "Notes").sheet(&page_sheet(6.0, 4.0)));
    with_windows(builder, windows)
}

/// The drawing as bytes, with the grid and rulers on.
pub fn demo_vsdx() -> Vec<u8> {
    demo_builder(Some(WINDOWS_PART)).build()
}

/// A drawing of one page, 11 by 8.5 inches, holding `shapes` (the XML of `Shape` elements).
pub fn page_with(shapes: &str) -> Vec<u8> {
    VsdxBuilder::new().styles(ROOT_STYLE).page(PageSpec::new(1, "Groups").sheet(&page_sheet(11.0, 8.5)).shapes(shapes)).build()
}

/// A drawing of two pages (ids 1 and 2), 11 by 8.5 inches, **each holding a shape 1 and a shape 2** (a shape's `ID` is the page's own): page 1's two boxes
/// overlap each other, and so do page 2's, at other places. What a command named for one page does to the other shows in the document.
pub fn two_pages_vsdx() -> Vec<u8> {
    // Shape 1 of each page has a name of its own, which Size & Position puts in its title.
    let named = |name: &str, x: f64, y: f64| shape_with(1, &format!(r#" NameU="{name}""#), &[placed(x, y, 2.0, 2.0), rectangle(2.0, 2.0)].concat());
    let one = [named("First page box", 2.0, 2.0), boxed(2, 3.0, 3.0, 2.0, 2.0)].concat();
    let two = [named("Second page box", 5.0, 2.0), boxed(2, 6.0, 3.0, 2.0, 2.0)].concat();
    VsdxBuilder::new()
        .styles(ROOT_STYLE)
        .page(PageSpec::new(1, "One").sheet(&page_sheet(11.0, 8.5)).shapes(&one))
        .page(PageSpec::new(2, "Two").sheet(&page_sheet(11.0, 8.5)).shapes(&two))
        .build()
}

/// Two boxes of an inch with one connection point each where no corner or middle of a box is: box 1 with its lower left at (2, 2) and its point 0.3 along its
/// bottom edge, box 2 at (5, 5) with its point 0.7 along its top edge. What pulls box 1's point to box 2's is the point itself, not a place of the box.
pub fn off_middle_points_vsdx() -> Vec<u8> {
    let point =
        |x: f64, y: f64| format!(r#"<Section N="Connection"><Row IX="0">{}{}</Row></Section>"#, cell("X", &x.to_string()), cell("Y", &y.to_string()));
    let pointed = |id: u32, x: f64, y: f64, at: (f64, f64)| shape(id, &[placed(x, y, 1.0, 1.0), rectangle(1.0, 1.0), point(at.0, at.1)].concat());
    page_with(&[pointed(1, 2.0, 2.0, (0.3, 0.0)), pointed(2, 5.0, 5.0, (0.7, 1.0))].concat())
}

/// A page with a horizontal guide (10) at y = 5 (a page shape of `Type="Guide"`, its line through its pin along its angle 0), a box (2) whose
/// `Connect` row from `AlignBottom` (part 4) holds it to the guide, and a box (3) whose row from `BeginX` (part 9, a line's begin) names the guide and
/// holds no box to it: what a guide's drag carries is the shapes of parts 1 to 6 alone.
pub fn guide_rows_vsdx() -> Vec<u8> {
    let guide = shape_with(10, r#" Type="Guide""#, &[placed(0.0, 5.0, 0.0, 0.0), cell("Angle", "0")].concat());
    let shapes = [guide, boxed(2, 2.0, 5.0, 1.0, 1.0), boxed(3, 6.0, 5.0, 1.0, 1.0)].concat();
    let connects = r#"<Connect FromSheet="2" FromCell="AlignBottom" FromPart="4" ToSheet="10" ToCell="Angle" ToPart="4"/><Connect FromSheet="3" FromCell="BeginX" FromPart="9" ToSheet="10" ToCell="Angle" ToPart="4"/>"#;
    VsdxBuilder::new().styles(ROOT_STYLE).page(PageSpec::new(1, "Guides").sheet(&page_sheet(11.0, 8.5)).shapes(&shapes).connects(connects)).build()
}

/// A box `w` by `h` with its lower left at `(x, y)` of its parent's frame.
pub fn boxed(id: u32, x: f64, y: f64, w: f64, h: f64) -> String {
    shape(id, &[placed(x, y, w, h), rectangle(w, h)].concat())
}

/// A group with `pose` and `extra` cells whose members are `members`.
pub fn group(id: u32, pose: &str, extra: &str, members: &str) -> String {
    shape_with(id, r#" Type="Group""#, &format!("{pose}{extra}<Shapes>{members}</Shapes>"))
}

/// The cells that place a shape with its pin at `(x, y)`, its size, its local pin and its angle in radians.
pub fn posed(x: f64, y: f64, w: f64, h: f64, loc: (f64, f64), angle: f64) -> String {
    [
        cell("PinX", &x.to_string()),
        cell("PinY", &y.to_string()),
        cell("Width", &w.to_string()),
        cell("Height", &h.to_string()),
        cell("LocPinX", &loc.0.to_string()),
        cell("LocPinY", &loc.1.to_string()),
        cell("Angle", &angle.to_string()),
    ]
    .concat()
}

/// Groups of every `SelectMode`: 10 is set to group only at (1, 5), 2 by 2 inches, with a box (11) and a group (12) inside it set to
/// members first, over a box (13) (the member is unselectable under the outer 0); 20 at (4, 5) has no cell, so it reads 1, with one
/// box (21); 30 at (7, 5) is set to members first, with two boxes, 31 on its left half and 32 on its right; 40 is turned a quarter about its middle
/// (6, 3), 2 by 1 inches with its pin in the middle, with a box (41) over all of it and a small one (42) at the lower left of its own frame.
pub fn groups_vsdx() -> Vec<u8> {
    let only = group(
        10,
        &placed(1.0, 5.0, 2.0, 2.0),
        &cell("SelectMode", "0"),
        &[boxed(11, 0.0, 0.0, 2.0, 2.0), group(12, &placed(0.5, 0.5, 1.0, 1.0), &cell("SelectMode", "2"), &boxed(13, 0.0, 0.0, 1.0, 1.0))].concat(),
    );
    let first = group(20, &placed(4.0, 5.0, 2.0, 2.0), "", &boxed(21, 0.0, 0.0, 2.0, 2.0));
    let members =
        group(30, &placed(7.0, 5.0, 2.0, 2.0), &cell("SelectMode", "2"), &[boxed(31, 0.0, 0.0, 1.0, 2.0), boxed(32, 1.0, 0.0, 1.0, 2.0)].concat());
    let turned = group(
        40,
        &posed(6.0, 3.0, 2.0, 1.0, (1.0, 0.5), std::f64::consts::FRAC_PI_2),
        "",
        &[boxed(41, 0.0, 0.0, 2.0, 1.0), boxed(42, 0.0, 0.0, 0.5, 0.5)].concat(),
    );
    page_with(&[only, first, members, turned].concat())
}

/// A drawing of one page with `n` small boxes: large enough that reading it takes a moment.
pub fn crowded_vsdx(n: u32) -> Vec<u8> {
    crowded(n, None)
}

/// The same page as a template file (the template content type): a new drawing made from it takes as long to read.
pub fn crowded_template(n: u32) -> Vec<u8> {
    crowded(n, Some("application/vnd.ms-visio.template.main+xml")) // brand-ok: a content type the format defines
}

fn crowded(n: u32, document_type: Option<&str>) -> Vec<u8> {
    let mut shapes = String::with_capacity(n as usize * 600);
    for i in 0..n {
        let (x, y) = (0.2 + f64::from(i % 100) * 0.1, 0.2 + f64::from(i / 100 % 80) * 0.1);
        shapes.push_str(&shape(
            i + 1,
            &[placed(x, y, 0.08, 0.08), cell("FillForegnd", "#9DC3E6"), cell("LineWeight", "0.002"), rectangle(0.08, 0.08)].concat(),
        ));
    }
    let builder = VsdxBuilder::new().styles(ROOT_STYLE).page(PageSpec::new(1, "Crowded").sheet(&page_sheet(11.0, 8.5)).shapes(&shapes));
    match document_type {
        Some(content_type) => builder.document_type(content_type).build(),
        None => builder.build(),
    }
}

/// A drawing of one page with a shape whose one geometry section makes nine paths (nine moves): the picture of Pan & Zoom keeps eight of them.
pub fn nine_paths_vsdx() -> Vec<u8> {
    let row =
        |t: &str, ix: u32, x: f64, y: f64| format!(r#"<Row T="{t}" IX="{ix}">{}{}</Row>"#, cell("X", &x.to_string()), cell("Y", &y.to_string()));
    let mut rows = String::new();
    let mut ix = 0;
    for k in 0..9u32 {
        let x = f64::from(k) * 0.3;
        for (kind, dx, y) in [("MoveTo", 0.0, 0.0), ("LineTo", 0.2, 0.0), ("LineTo", 0.2, 1.0), ("LineTo", 0.0, 1.0), ("LineTo", 0.0, 0.0)] {
            ix += 1;
            rows.push_str(&row(kind, ix, x + dx, y));
        }
    }
    let sections = format!(r#"<Section N="Geometry" IX="0">{}{rows}</Section>"#, cell("NoFill", "1"));
    let many = shape(1, &[placed(2.0, 2.0, 3.0, 1.0), sections].concat());
    VsdxBuilder::new().styles(ROOT_STYLE).page(PageSpec::new(1, "Paths").sheet(&page_sheet(11.0, 8.5)).shapes(&many)).build()
}

/// A zig-zag outline of `points` points in a box `w` by `h`, as geometry section 0.
fn zigzag(w: f64, h: f64, points: u32) -> String {
    let row =
        |t: &str, ix: u32, x: f64, y: f64| format!(r#"<Row T="{t}" IX="{ix}">{}{}</Row>"#, cell("X", &x.to_string()), cell("Y", &y.to_string()));
    let mut rows = row("MoveTo", 1, 0.0, 0.0);
    for k in 1..points {
        let (x, y) = (w * f64::from(k) / f64::from(points), if k % 2 == 0 { 0.0 } else { h });
        rows.push_str(&row("LineTo", k + 1, x, y));
    }
    format!(r#"<Section N="Geometry" IX="0">{}{rows}</Section>"#, cell("NoFill", "1"))
}

/// A drawing of one page (id 1) holding one shape (id 1) with its lower left at `(x, y)`, `w` by `h`, filled with `fill` (`#RRGGBB`), and a second page
/// (id 2) for each of `extra_pages`: two drawings built with this hold the same page and shape ids and different values.
pub fn filled_vsdx(x: f64, y: f64, w: f64, h: f64, fill: &str, extra_pages: u32) -> Vec<u8> {
    let boxed = shape(1, &[placed(x, y, w, h), cell("FillForegnd", fill), rectangle(w, h)].concat());
    let mut builder = VsdxBuilder::new().styles(ROOT_STYLE).page(PageSpec::new(1, "First").sheet(&page_sheet(11.0, 8.5)).shapes(&boxed));
    for n in 0..extra_pages {
        builder = builder.page(PageSpec::new(2 + n, &format!("Extra {}", n + 1)).sheet(&page_sheet(11.0, 8.5)));
    }
    builder.build()
}

/// A drawing of one page with `n` small shapes whose outlines are zig-zags of `points` points each: the detailed shapes of a drawing that was made from
/// traced pictures. (The engine reads at most 4,096 points of a shape's geometry.)
pub fn detailed_vsdx(n: u32, points: u32) -> Vec<u8> {
    let mut shapes = String::with_capacity(n as usize * points as usize * 110);
    for i in 0..n {
        let (x, y) = (0.5 + f64::from(i % 40) * 0.25, 0.5 + f64::from(i / 40 % 30) * 0.25);
        shapes.push_str(&shape(i + 1, &[placed(x, y, 0.2, 0.2), zigzag(0.2, 0.2, points)].concat()));
    }
    VsdxBuilder::new().styles(ROOT_STYLE).page(PageSpec::new(1, "Detailed").sheet(&page_sheet(11.0, 8.5)).shapes(&shapes)).build()
}

/// A drawing of one page with `n` small boxes at `n` different places across (0.03 inch apart) and a few up the page: shapes a value typed in X would put
/// at one place each, one step of a batch for every place.
pub fn scattered_vsdx(n: u32) -> Vec<u8> {
    let mut shapes = String::new();
    for i in 0..n {
        let (x, y) = (0.3 + f64::from(i) * 0.03, 1.0 + f64::from(i % 7) * 0.1);
        shapes.push_str(&shape(i + 1, &[placed(x, y, 0.02, 0.02), rectangle(0.02, 0.02)].concat()));
    }
    VsdxBuilder::new().styles(ROOT_STYLE).page(PageSpec::new(1, "Scattered").sheet(&page_sheet(11.0, 8.5)).shapes(&shapes)).build()
}

/// A drawing of one page with a shape that has a `NameU` and no `Name`.
pub fn nameu_only_vsdx() -> Vec<u8> {
    let named = shape_with(1, r#" NameU="Only name""#, &[placed(2.0, 2.0, 2.0, 1.0), rectangle(2.0, 1.0)].concat());
    VsdxBuilder::new().styles(ROOT_STYLE).page(PageSpec::new(1, "Named").sheet(&page_sheet(11.0, 8.5)).shapes(&named)).build()
}

/// A drawing of one page and `n` masters with no shapes in them: a document with thousands of masters.
pub fn many_masters_vsdx(n: u32) -> Vec<u8> {
    let mut builder = VsdxBuilder::new().styles(ROOT_STYLE).page(PageSpec::new(1, "Page-1").sheet(&page_sheet(11.0, 8.5)));
    for i in 1..=n {
        builder = builder.master(MasterSpec::new(i, &format!("Master {i}")));
    }
    builder.build()
}

/// A drawing of one page with a shape whose `Width` is guarded: the engine refuses a size for it.
pub fn guarded_width_vsdx() -> Vec<u8> {
    let guarded = shape(
        1,
        &[
            cell("PinX", "3"),
            cell("PinY", "3"),
            cell_f("Width", "2", "GUARD(2)"),
            cell("Height", "1"),
            cell("LocPinX", "1"),
            cell("LocPinY", "0.5"),
            rectangle(2.0, 1.0),
        ]
        .concat(),
    );
    VsdxBuilder::new().styles(ROOT_STYLE).page(PageSpec::new(1, "Guarded").sheet(&page_sheet(11.0, 8.5)).shapes(&guarded)).build()
}

/// A drawing of one page with a shape that has a custom fill pattern (which the renderer draws solid and says so).
pub fn custom_pattern_vsdx() -> Vec<u8> {
    let patterned = shape(1, &[placed(2.0, 2.0, 3.0, 2.0), cell("FillPattern", "254"), rectangle(3.0, 2.0)].concat());
    VsdxBuilder::new().styles(ROOT_STYLE).page(PageSpec::new(1, "Pattern").sheet(&page_sheet(11.0, 8.5)).shapes(&patterned)).build()
}

/// A drawing of one page with a shape that selects a theme and holds the `Themed` marker in its fill, line and shadow cells: the theme
/// cannot answer them, which is what 11 of 12 real files did to the first window. Drawn, it is a blue box with a thin outline.
pub fn themed_vsdx() -> Vec<u8> {
    let themed = |name: &str| cell_f(name, "Themed", "THEMEVAL()");
    let schemes = ["ThemeIndex", "ColorSchemeIndex", "EffectSchemeIndex", "ConnectorSchemeIndex", "FontSchemeIndex"].map(|n| cell(n, "1")).concat();
    let inner = [
        placed(2.0, 2.0, 3.0, 2.0),
        schemes,
        themed("FillPattern"),
        themed("FillForegnd"),
        themed("FillBkgnd"),
        themed("LinePattern"),
        themed("LineColor"),
        themed("LineWeight"),
        themed("ShdwPattern"),
        rectangle(3.0, 2.0),
    ]
    .concat();
    VsdxBuilder::new().styles(ROOT_STYLE).page(PageSpec::new(1, "Themed").sheet(&page_sheet(11.0, 8.5)).shapes(&shape(1, &inner))).build()
}

/// What the label of `labelled_vsdx` is in the file: two runs (the first bold, the second italic) and a field after them. The window's text box
/// holds `abcd`, and the field `3` is shown beside it and not typed into.
pub const LABEL_TEXT: &str = r#"<Text><cp IX="0"/>ab<cp IX="1"/>cd<fld IX="0">3</fld></Text>"#;

/// The same, with `cd` changed to `ce`: what one changed letter must save as (the markers and the field where they were).
pub const LABEL_TEXT_AFTER: &str = r#"<Text><cp IX="0"/>ab<cp IX="1"/>ce<fld IX="0">3</fld></Text>"#;

/// A drawing of two pages, "Labels" (ID 1) and "Second" (ID 2), both 11 by 8.5 inches. On the first, shape 1 is a box 2 by 1 inches centred at
/// (3, 6) whose text is [`LABEL_TEXT`]: runs with formatting of their own and a field.
pub fn labelled_vsdx() -> Vec<u8> {
    let label = shape(
        1,
        &[
            placed(2.0, 5.5, 2.0, 1.0),
            cell("FillForegnd", "#BDD7EE"),
            rectangle(2.0, 1.0),
            r#"<Section N="Character"><Row IX="0"><Cell N="Style" V="1"/><Cell N="Size" V="0.2"/></Row><Row IX="1"><Cell N="Style" V="2"/><Cell N="Size" V="0.2"/></Row></Section>"#
                .to_owned(),
            r#"<Section N="Field"><Row IX="0"><Cell N="Value" V="3"/></Row></Section>"#.to_owned(),
            LABEL_TEXT.to_owned(),
        ]
        .concat(),
    );
    VsdxBuilder::new()
        .styles(ROOT_STYLE)
        .page(PageSpec::new(1, "Labels").sheet(&page_sheet(11.0, 8.5)).shapes(&label))
        .page(PageSpec::new(2, "Second").sheet(&page_sheet(11.0, 8.5)))
        .build()
}

/// A drawing of one page with `instances` instances (in rows of 20, half an inch apart) of a master 0.3 inch square whose `Connection` section holds
/// `rows` rows, with `windows` as its windows part: what a file that turns View > Connection Points on costs the window.
pub fn pointed_instances_vsdx(rows: u32, instances: u32, windows: Option<&str>) -> Vec<u8> {
    let mut section = String::with_capacity(rows as usize * 80);
    section.push_str(r#"<Section N="Connection">"#);
    for i in 0..rows {
        let (x, y) = (0.3 * f64::from(i % 16) / 15.0, 0.3 * f64::from(i / 16 % 16) / 15.0);
        section.push_str(&format!(r#"<Row IX="{i}">{}{}</Row>"#, cell("X", &x.to_string()), cell("Y", &y.to_string())));
    }
    section.push_str("</Section>");
    let master =
        shape(1, &[cell("Width", "0.3"), cell("Height", "0.3"), cell("LocPinX", "0"), cell("LocPinY", "0"), rectangle(0.3, 0.3), section].concat());
    let mut shapes = String::with_capacity(instances as usize * 300);
    for i in 0..instances {
        let (x, y) = (0.3 + f64::from(i % 20) * 0.5, 0.3 + f64::from(i / 20 % 16) * 0.5);
        shapes.push_str(&shape_with(i + 1, r#" Master="1""#, &placed(x, y, 0.3, 0.3)));
    }
    let builder = VsdxBuilder::new()
        .styles(ROOT_STYLE)
        .master(MasterSpec::new(1, "Pointed").shapes(&master))
        .page(PageSpec::new(1, "Points").sheet(&page_sheet(11.0, 8.5)).shapes(&shapes));
    with_windows(builder, windows).build()
}

/// Writes `bytes` to `name` under `dir`.
pub fn write(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// Writes the drawings where the run of the real window can open them: `DIACRAFT_DEMO_DIR` names the folder. Not a test unless asked for.
#[test]
#[ignore = "writes the demo drawings to the folder in DIACRAFT_DEMO_DIR, for opening them in the running window"]
fn write_the_demo_drawings() {
    let dir = std::path::PathBuf::from(std::env::var("DIACRAFT_DEMO_DIR").expect("DIACRAFT_DEMO_DIR names the folder to write to"));
    std::fs::create_dir_all(&dir).unwrap();
    write(&dir, "demo.vsdx", &demo_vsdx());
    write(&dir, "pattern.vssx", &custom_pattern_vsdx());
    write(&dir, "labelled.vsdx", &labelled_vsdx());
    write(&dir, "groups.vsdx", &groups_vsdx());
    write(&dir, "crowded.vsdx", &crowded_vsdx(60_000));
    write(&dir, "cut.vsdx", &demo_vsdx()[..400]);
    write(&dir, "notes.vsdx", b"These are not drawing bytes at all.");
    eprintln!("wrote the drawings to {}", dir.display());
}
