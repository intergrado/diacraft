//! The render check: the renderer draws what the OpenDocument reader made of hand-built drawings to the pixels the specs' vectors give
//! At a small size. It lives here because the reader and the renderer are both of layer 3, and
//! neither may use the other; the command line uses both.
//!
//! Every drawing is written by hand from the ODF standard and put together by the container's writer. The page is 4 cm by 3 cm, drawn at
//! 254 pixels to the inch, so a pixel is 0.01 cm and a point (x cm, y cm) from the page's top-left corner is pixel (100 x, 100 y).

use diacraft_container::zip::{ArchiveWriter, Method};
use diacraft_odg::{Limits, read};
use diacraft_render::{Renderer, View};

const DRAWING: &str = "application/vnd.oasis.opendocument.graphics";
const NS: &str = concat!(
    r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" "#,
    r#"xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
    r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" "#,
    r#"xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
    r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" "#,
    r#"xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0""#
);

/// Pixels to the inch: 100 to the centimetre.
const PPI: f64 = 254.0;

/// A drawing of one 4 cm by 3 cm page holding `shapes`, every shape on the automatic style `red` (a red fill and no line) unless it names
/// another.
fn drawing(shapes: &str) -> Vec<u8> {
    let styles = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-styles {NS} office:version=\"1.3\"><office:styles/><office:automatic-styles><style:page-layout style:name=\"PM1\"><style:page-layout-properties fo:page-width=\"4cm\" fo:page-height=\"3cm\"/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"/></office:master-styles></office:document-styles>"
    );
    let content = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-content {NS} office:version=\"1.3\"><office:automatic-styles><style:style style:name=\"red\" style:family=\"graphic\"><style:graphic-properties draw:fill=\"solid\" draw:fill-color=\"#ff0000\" draw:stroke=\"none\"/></style:style><style:style style:name=\"ink\" style:family=\"graphic\"><style:graphic-properties draw:fill=\"none\" draw:stroke=\"solid\" svg:stroke-color=\"#000000\" svg:stroke-width=\"0.2cm\"/></style:style></office:automatic-styles><office:body><office:drawing><draw:page draw:name=\"p\" draw:master-page-name=\"Default\">{shapes}</draw:page></office:drawing></office:body></office:document-content>"
    );
    let manifest = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\"><manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"{DRAWING}\"/><manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/><manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/></manifest:manifest>"
    );
    let mut w = ArchiveWriter::new();
    w.add("mimetype", DRAWING.as_bytes(), Method::Stored).unwrap();
    w.add("content.xml", content.as_bytes(), Method::Deflated).unwrap();
    w.add("styles.xml", styles.as_bytes(), Method::Deflated).unwrap();
    w.add("META-INF/manifest.xml", manifest.as_bytes(), Method::Deflated).unwrap();
    w.finish().unwrap()
}

/// The page's pixels: premultiplied RGBA, `width` by `height`, rows from the top.
struct Pixels {
    rgba: Vec<u8>,
    width: usize,
    height: usize,
}

impl Pixels {
    /// The pixel at (x cm, y cm) from the page's top-left corner.
    fn at(&self, x: f64, y: f64) -> [u8; 4] {
        let (px, py) = ((x * 100.0) as usize, (y * 100.0) as usize);
        assert!(px < self.width && py < self.height, "({x}, {y}) is off the page");
        let i = (py * self.width + px) * 4;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }

    fn red(&self, x: f64, y: f64) -> bool {
        let p = self.at(x, y);
        p[3] > 250 && p[0] > 240 && p[1] < 20 && p[2] < 20
    }

    fn dark(&self, x: f64, y: f64) -> bool {
        let p = self.at(x, y);
        p[3] > 250 && p[0] < 40 && p[1] < 40 && p[2] < 40
    }

    /// Paper: white, or nothing drawn.
    fn blank(&self, x: f64, y: f64) -> bool {
        let p = self.at(x, y);
        p[3] == 0 || (p[0] > 245 && p[1] > 245 && p[2] > 245)
    }
}

fn render(shapes: &str) -> Pixels {
    let (doc, _) = read(&drawing(shapes), &Limits::default()).unwrap();
    let page = doc.pages.get(0).unwrap().id;
    let view = View::fit_page(&doc, page, PPI).unwrap();
    let rendered = Renderer::new().render(&doc, page, &view);
    assert_eq!((rendered.width, rendered.height), (400, 300), "the page is 4 cm by 3 cm");
    Pixels { rgba: rendered.rgba_premul.clone(), width: rendered.width as usize, height: rendered.height as usize }
}

#[test]
fn odg_render_a_rectangle_lands_where_its_frame_says_y_down() {
    // The frame at a smaller size: from (1 cm, 0.5 cm), 2 cm by 1 cm. The y flip puts it near the top of the page, not the bottom.
    let p = render(r#"<draw:rect draw:style-name="red" svg:x="1cm" svg:y="0.5cm" svg:width="2cm" svg:height="1cm"/>"#);
    assert!(p.red(2.0, 1.0), "the centre");
    assert!(p.red(1.05, 0.55) && p.red(2.95, 1.45), "just inside two corners");
    assert!(p.blank(0.95, 1.0) && p.blank(3.05, 1.0) && p.blank(2.0, 0.45) && p.blank(2.0, 1.55), "just outside each side");
    assert!(p.blank(2.0, 2.5), "not flipped to the bottom");
}

#[test]
fn odg_render_an_ellipse_fills_its_frame_and_not_its_corners() {
    let p = render(r#"<draw:ellipse draw:style-name="red" svg:x="0.5cm" svg:y="0.5cm" svg:width="3cm" svg:height="2cm"/>"#);
    assert!(p.red(2.0, 1.5) && p.red(0.6, 1.5) && p.red(3.4, 1.5) && p.red(2.0, 0.6) && p.red(2.0, 2.4));
    assert!(p.blank(0.6, 0.6) && p.blank(3.4, 2.4), "the frame's corners");
}

#[test]
fn odg_render_a_polygon_s_apex_is_at_the_top() {
    // The points (0, 0), (1000, 0), (500, 1000) of a 1000 square view box: a triangle whose base runs along the frame's top edge.
    let p = render(
        r#"<draw:polygon draw:style-name="red" svg:x="1cm" svg:y="0.5cm" svg:width="2cm" svg:height="2cm" svg:viewBox="0 0 1000 1000" draw:points="0,0 1000,0 500,1000"/>"#,
    );
    assert!(p.red(1.1, 0.6) && p.red(2.9, 0.6), "the base's two ends, at the top");
    assert!(p.red(2.0, 2.4), "the apex, at the bottom middle");
    assert!(p.blank(1.1, 2.4) && p.blank(2.9, 2.4), "the bottom corners of the frame");
}

#[test]
fn odg_render_a_turned_rectangle_turns_counterclockwise() {
    // The turn: a bar 3 cm by 0.4 cm turned 30 degrees about its frame's origin, then moved to (0.5 cm, 2.5 cm). Its far end rises.
    let p = render(
        r#"<draw:rect draw:style-name="red" svg:width="3cm" svg:height="0.4cm" draw:transform="rotate (0.5235987755982988) translate (0.5cm 2.5cm)"/>"#,
    );
    // The bar's axis from (0.5, 2.5 + 0.2 cos 30) runs at 30 degrees up and to the right (y down: y falls as x grows).
    let (x0, y0) = (0.5 + 0.2 * 0.5, 2.5 + 0.2 * 0.866);
    let at = |d: f64| (x0 + d * 0.866, y0 - d * 0.5);
    for d in [0.3, 1.5, 2.7] {
        let (x, y) = at(d);
        assert!(p.red(x, y), "along the turned bar at {d} cm: ({x}, {y})");
    }
    assert!(p.blank(3.3, 2.7), "where an unturned bar, or one turned clockwise, would reach");
}

#[test]
fn odg_render_a_line_strokes_between_its_ends() {
    let p = render(r#"<draw:line draw:style-name="ink" svg:x1="0.5cm" svg:y1="1.5cm" svg:x2="3.5cm" svg:y2="1.5cm"/>"#);
    assert!(p.dark(1.0, 1.5) && p.dark(3.0, 1.5));
    assert!(p.blank(2.0, 1.2) && p.blank(2.0, 1.8), "0.3 cm off a 0.2 cm stroke");
}

#[test]
fn odg_render_a_slice_lies_above_and_right_of_the_centre() {
    // A circle's section from 0 to 60 degrees, counterclockwise as seen from the right-middle.
    let p = render(
        r#"<draw:circle draw:style-name="red" svg:x="1cm" svg:y="0.5cm" svg:width="2cm" svg:height="2cm" draw:kind="section" draw:start-angle="0" draw:end-angle="60"/>"#,
    );
    let (cx, cy) = (2.0, 1.5);
    assert!(p.red(cx + 0.6, cy - 0.2), "above the horizontal, to the right");
    assert!(p.blank(cx + 0.6, cy + 0.2), "below it");
    assert!(p.blank(cx - 0.4, cy - 0.4), "to the left of the centre");
}

/// A custom shape of 2 cm by 2 cm at (1 cm, 0.5 cm) holding `geometry` (attributes and children), on the red style.
fn custom(extra: &str, geometry: &str) -> String {
    format!(
        r#"<draw:custom-shape draw:style-name="red" svg:x="1cm" svg:y="0.5cm" svg:width="2cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" {extra}>{geometry}</draw:enhanced-geometry></draw:custom-shape>"#
    )
}

#[test]
fn odg_render_a_custom_shape_s_path_lands_in_its_frame_y_down() {
    // A triangle with its apex at the top middle of the view box (y runs down): the apex is near the top of the frame, the base on its bottom edge.
    let p = render(&custom(r#"draw:enhanced-path="M 0 21600 L 10800 0 L 21600 21600 Z N""#, ""));
    assert!(p.red(2.0, 2.4), "the base's middle");
    assert!(p.red(2.0, 0.8), "near the apex");
    assert!(p.blank(1.1, 0.7) && p.blank(2.9, 0.7), "the frame's top corners");
    assert!(p.red(1.2, 2.4) && p.red(2.8, 2.4), "the base's ends");
}

#[test]
fn odg_render_a_modifier_moves_a_vertex_through_its_equation() {
    // V14 on a frame 2 cm by 2 cm: M 0 0 L ?f0 0 21600 21600 Z N, with ?f0 = $0*2. At 5400 the second vertex is at the middle of the top edge;
    // at 2700 it is a quarter along it, so a point just under the top edge near the middle is inside the first and outside the second.
    let shape = |modifier: &str| {
        custom(
            &format!(r#"draw:modifiers="{modifier}" draw:enhanced-path="M 0 0 L ?f0 0 21600 21600 Z N""#),
            r#"<draw:equation draw:name="f0" draw:formula="$0*2"/>"#,
        )
    };
    let wide = render(&shape("5400"));
    assert!(wide.red(2.0, 0.65), "the second vertex is at (2 cm, 0.5 cm): the point is under the top edge");
    let narrow = render(&shape("2700"));
    assert!(narrow.blank(2.0, 0.65), "the second vertex is at (1.5 cm, 0.5 cm): the point is above the edge from it to the corner");
}

#[test]
fn odg_render_a_mirrored_custom_shape_is_flipped_in_its_frame() {
    let apex_up = r#"draw:enhanced-path="M 0 21600 L 10800 0 L 21600 21600 Z N""#;
    let up = render(&custom(apex_up, ""));
    let down = render(&custom(&format!(r#"draw:mirror-vertical="true" {apex_up}"#), ""));
    assert!(up.red(2.0, 0.8) && up.blank(1.1, 0.7));
    assert!(down.red(2.0, 2.2) && down.blank(1.1, 2.3), "the apex is at the bottom");
}

#[test]
fn odg_render_an_ooxml_rectangle_with_no_path_fills_its_frame() {
    let p = render(
        r#"<draw:custom-shape draw:style-name="red" svg:x="1cm" svg:y="0.5cm" svg:width="2cm" svg:height="2cm"><draw:enhanced-geometry draw:type="ooxml-rect"/></draw:custom-shape>"#,
    );
    assert!(p.red(1.1, 0.6) && p.red(2.9, 2.4) && p.red(2.0, 1.5));
    assert!(p.blank(0.9, 1.5) && p.blank(3.1, 1.5));
}

#[test]
fn odg_render_a_connector_strokes_its_route_through_the_path_points() {
    // The shape: svg:d in hundredths of a millimetre from the page's top-left corner: right 1 cm, down 1 cm, right 2 cm.
    let p = render(
        r#"<draw:connector draw:style-name="ink" svg:x1="0.5cm" svg:y1="0.5cm" svg:x2="3.5cm" svg:y2="1.5cm" svg:d="M 500 500 h 1000 v 1000 h 2000"/>"#,
    );
    assert!(p.dark(1.0, 0.5), "the first segment");
    assert!(p.dark(1.5, 1.0), "the second, down");
    assert!(p.dark(3.0, 1.5), "the third");
    assert!(p.blank(2.5, 0.5) && p.blank(1.0, 1.5), "not the chord, and not the corner the route skips");
}

#[test]
fn odg_render_the_members_of_a_group_land_where_they_would_alone() {
    // Two rectangles in a group, one of them turned: each drawn as if it stood on the page by itself.
    let p = render(concat!(
        r#"<draw:g><draw:rect draw:style-name="red" svg:x="0.5cm" svg:y="0.5cm" svg:width="1cm" svg:height="1cm"/>"#,
        r#"<draw:rect draw:style-name="red" svg:width="1.5cm" svg:height="0.4cm" draw:transform="rotate (0.5235987755982988) translate (2cm 2.5cm)"/></draw:g>"#
    ));
    assert!(p.red(1.0, 1.0) && p.blank(1.6, 1.0), "the first");
    // The second: a bar from (2, 2.5) cm rising to the right at 30 degrees.
    assert!(p.red(2.5, 2.4) && p.red(3.0, 2.1));
    assert!(p.blank(3.3, 2.7), "where an unturned bar would reach");
}

/// An object frame with no picture is drawn as a labelled box, a pale grey one, and the render does not fail; where it is not, the page is
/// blank. (The namespace of `dr3d` is not declared here, so the second element is no shape.)
#[test]
fn odg_render_an_object_frame_with_no_picture_is_a_labelled_box_and_does_not_fail() {
    let p = render(
        r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"><draw:object xlink:href="./Object 1"/></draw:frame><dr3d:scene svg:width="1cm" svg:height="1cm"/>"#,
    );
    assert_eq!(p.at(1.12, 1.5), [232, 232, 232, 255], "the box is pale grey, beside the label and clear of the cross");
    assert!(p.blank(0.5, 1.5) && p.blank(3.5, 1.5) && p.blank(2.0, 2.5), "and the page is blank round it");
}
