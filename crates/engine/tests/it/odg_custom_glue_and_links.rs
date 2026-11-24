//! The two `.odg` losses of the gate on `29a46bbf`, each from the structure of its vector, by the OASIS ODF standard. Each test is the plain visit a person
//! can make (`file.open`, then `file.save_as` to `.diacraft`, then from it to the file's own format, one leg a session).
//!
//! - **A connector end's glue index grew by 4 on every visit** (a custom shape whose `draw:enhanced-geometry` lists glue points): the model writer wrote the geometry element the file held, which
//!   keeps the file's glue ids, and numbered the connector as if it had written the shape's rows as its own (the second case: glue id *r* + 4).
//! - **A flat drawing lost every in-text link**: the root of a flat file is both parts' root, and its namespace declarations (`xlink` among them) were not held, so the package made from it bound
//!   no prefix for a link's `xlink:href` and the writer left the link out.

use crate::native_common::Dir;
use crate::odg_build::Drawing;
use crate::save_check_misses::{all, leg, named, parse};

/// The vector's four listed glue points on a custom shape (`draw:glue-points`, 8 numbers), no `draw:glue-point` of its own: the middles of the top, the left side, the bottom and the right side.
const LISTED: &str = "10800 0 0 10800 10800 21600 21600 10800";

/// The vector's structure: a rectangle (`id2`) and a custom shape (`id1`), the shape's geometry listing `points` when it is given, and a line connector from the rectangle's glue point 1
/// to the shape's glue point `end`. `style` is the shape's style (`gr3` says its rows start at 0), `own_point` adds a `draw:glue-point` of the shape's own (id 8), and `connector_first` puts
/// the connector before the shapes in the page.
struct Vector<'a> {
    points: Option<&'a str>,
    style: &'a str,
    end: u32,
    own_point: bool,
    connector_first: bool,
}

impl Vector<'_> {
    fn bytes(&self) -> Vec<u8> {
        self.drawing().bytes()
    }

    /// The drawing as one flat document.
    fn flat(&self) -> Vec<u8> {
        self.drawing().flat_bytes()
    }

    fn drawing(&self) -> Drawing {
        let points = self.points.map(|p| format!(r#" draw:glue-points="{p}""#)).unwrap_or_default();
        let own = if self.own_point { r#"<draw:glue-point draw:id="8" svg:x="1cm" svg:y="0.5cm" draw:escape-direction="auto"/>"# } else { "" };
        let rect = r#"<draw:rect draw:id="id2" xml:id="id2" draw:style-name="gr1" svg:x="2cm" svg:y="2cm" svg:width="3cm" svg:height="2cm"/>"#;
        let shape = format!(
            concat!(
                r#"<draw:custom-shape draw:id="id1" xml:id="id1" draw:style-name="{style}" svg:x="9cm" svg:y="2cm" svg:width="4cm" svg:height="2cm">{own}"#,
                r#"<draw:enhanced-geometry ext:note="kept" svg:viewBox="0 0 21600 21600" draw:type="flowchart-process"{points} draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 Z N"/></draw:custom-shape>"#
            ),
            style = self.style,
            own = own,
            points = points,
        );
        let connector = format!(
            concat!(
                r#"<draw:connector draw:style-name="gr1" draw:type="line" svg:x1="5cm" svg:y1="3cm" svg:x2="11cm" svg:y2="2cm" draw:start-shape="id2" draw:start-glue-point="1" "#,
                r#"draw:end-shape="id1" draw:end-glue-point="{end}"/>"#
            ),
            end = self.end,
        );
        let shapes = if self.connector_first { format!("{connector}{rect}{shape}") } else { format!("{rect}{shape}{connector}") };
        let mut drawing = Drawing::new();
        // A style that says the rows start at 0 with the first listed glue point (the marker of ODF's `diacraft:glue="own"`, declared where it is used).
        drawing.auto.push_str(
            r#"<style:style style:name="gr3" style:family="graphic"><style:graphic-properties xmlns:diacraft="urn:app.diacraft:extension" diacraft:glue="own"/></style:style>"#,
        );
        drawing.page(&shapes)
    }
}

/// The ordinary vector: the shape lists `points` (when given), in the style of the rectangle, and no point of its own.
fn connector_drawing(points: Option<&str>, style: &str, end: u32) -> Vec<u8> {
    Vector { points, style, end, own_point: false, connector_first: false }.bytes()
}

/// The `(start, end)` glue points of each `draw:connector` of a package's `content.xml`, in document order.
fn glue_ends(bytes: &[u8]) -> Vec<(String, String)> {
    let root = parse(&crate::odg_build::part_of(bytes, "content.xml"));
    let mut list = Vec::new();
    all(&root, &mut list);
    list.iter()
        .filter(|el| el.name == "draw:connector")
        .map(|el| (el.attr("draw:start-glue-point").unwrap_or_default().to_owned(), el.attr("draw:end-glue-point").unwrap_or_default().to_owned()))
        .collect()
}

/// Whether the package's `content.xml` holds a style that says `diacraft:glue="own"`.
fn says_own(bytes: &[u8]) -> bool {
    crate::odg_build::part_of(bytes, "content.xml").contains(r#"diacraft:glue="own""#)
}

/// `visits` plain visits of `source`, an `.odg` called `v0.odg`: each is a leg to `.diacraft` and a leg back, the next starting from the file the last made. Gives what each leg named, and
/// the directory that holds `v0.odg`, `v1.odg`, ... and the `.diacraft` files between them.
fn visits(source: &[u8], visits: usize) -> (Vec<Vec<String>>, Dir) {
    let dir = Dir::new();
    dir.write("v0.odg", source);
    let mut legs = Vec::new();
    for k in 0..visits {
        legs.push(named(&leg(&dir, &format!("v{k}.odg"), &format!("d{k}.diacraft"))));
        legs.push(named(&leg(&dir, &format!("d{k}.diacraft"), &format!("v{}.odg", k + 1))));
    }
    (legs, dir)
}

/// Whether any leg named a connection that differs. (A shape's `draw:id` beside its `xml:id` is `kept-data` on any leg, as the gate's note of `odg-custom-shape-geometry-glue-and-own` says: another task's.)
fn connections(legs: &[Vec<String>]) -> bool {
    legs.iter().flatten().any(|class| class == "connections")
}

// ---------------------------------------------------------------------------------------------------------------- bug 1: the glue index

/// **A connector end on a custom shape whose geometry lists glue points keeps its index on the plain visit** (`odg-connector-on-geometry-glue-4`, `-5` and `odg-connector-on-default-glue-2`). The
/// ends 4 and 5 are the first and the second listed point; the end 2 is a standard point (the middle of the bottom side). Each came back 4 more (8, 9 and 6, past the shape's last glue point, 7).
/// Red on `29a46bbf`.
#[test]
fn a_connector_end_on_a_custom_shape_that_lists_glue_points_keeps_its_index_on_the_plain_visit() {
    for end in [4, 5, 6, 7, 2, 0, 1, 3] {
        let source = connector_drawing(Some(LISTED), "gr1", end);
        let (legs, dir) = visits(&source, 1);
        assert_eq!(glue_ends(&dir.read("v1.odg")), vec![("1".to_owned(), end.to_string())], "the end {end}");
        assert!(!connections(&legs), "the end {end}: the legs named {legs:?}");
    }
}

/// **Three visits in a row keep the index**: it moved by 4 on every visit (4, 8, 12).
#[test]
fn three_visits_in_a_row_keep_the_index_of_a_connector_end_on_a_shape_that_lists_glue_points() {
    for end in [4, 5, 2] {
        let source = connector_drawing(Some(LISTED), "gr1", end);
        let (legs, dir) = visits(&source, 3);
        for k in 1..=3 {
            assert_eq!(glue_ends(&dir.read(&format!("v{k}.odg"))), vec![("1".to_owned(), end.to_string())], "the end {end} after {k} visits");
        }
        assert!(!connections(&legs), "the end {end}: the legs named {legs:?}");
    }
}

/// **The page's order does not matter**: the connector stands before the custom shape it is glued to (a connector is written when it is reached, and its target's numbering is known before).
#[test]
fn a_connector_that_stands_before_its_shape_keeps_its_index_too() {
    for end in [5, 2] {
        let source = Vector { points: Some(LISTED), style: "gr1", end, own_point: false, connector_first: true }.bytes();
        let content = crate::odg_build::part_of(&source, "content.xml");
        assert!(content.find("<draw:connector ") < content.find("<draw:rect "), "the vector has the connector first");
        let (legs, dir) = visits(&source, 3);
        for k in 1..=3 {
            assert_eq!(glue_ends(&dir.read(&format!("v{k}.odg"))), vec![("1".to_owned(), end.to_string())], "the end {end} after {k} visits");
        }
        assert!(!connections(&legs), "the end {end}: {legs:?}");
    }
}

/// **A glue point of the shape's own beside the listed ones** (`odg-custom-shape-geometry-glue-and-own`: the point's id is 8, after the four listed, which are 4 to 7): the ends on a listed
/// point and on the shape's own keep their indices.
#[test]
fn a_connector_end_on_a_listed_point_or_the_shapes_own_keeps_its_index() {
    for end in [4, 7, 8] {
        let source = Vector { points: Some(LISTED), style: "gr1", end, own_point: true, connector_first: false }.bytes();
        let (legs, dir) = visits(&source, 3);
        for k in 1..=3 {
            assert_eq!(glue_ends(&dir.read(&format!("v{k}.odg"))), vec![("1".to_owned(), end.to_string())], "the end {end} after {k} visits");
        }
        assert!(!connections(&legs), "the end {end}: {legs:?}");
    }
}

/// The controls of bug 1: a custom shape whose geometry lists no glue point (the end 2, a standard point), and the end 1 and 3 on the one that does, keep the index as they did.
#[test]
fn the_controls_of_the_glue_index_keep_it() {
    let source = connector_drawing(None, "gr1", 2);
    let (legs, dir) = visits(&source, 3);
    for k in 1..=3 {
        assert_eq!(glue_ends(&dir.read(&format!("v{k}.odg"))), vec![("1".to_owned(), "2".to_owned())], "no listed point, after {k} visits");
    }
    assert!(!connections(&legs), "{legs:?}");
}

/// **A shape whose style says its rows start at 0 with its first listed glue point keeps the marker and the index** (DiaCraft's own form of a custom shape with connection points of its own).
/// The index is the same listed point whichever way it is numbered; the marker said how the reader numbers the shape's rows, and a visit that left it out gave the
/// connector another row of the shape (named `connections`).
#[test]
fn a_shape_whose_rows_start_at_0_keeps_its_marker_and_its_connector_end_through_three_visits() {
    for end in [5, 4, 6] {
        let source = connector_drawing(Some("0 0 10800 0 21600 21600"), "gr3", end);
        let (legs, dir) = visits(&source, 3);
        for k in 1..=3 {
            let written = dir.read(&format!("v{k}.odg"));
            assert_eq!(glue_ends(&written), vec![("1".to_owned(), end.to_string())], "the end {end} after {k} visits");
            assert!(says_own(&written), "the end {end} after {k} visits: the shape's style still says its rows start at 0");
        }
        assert!(!connections(&legs), "the end {end}: the legs named {legs:?}");
    }
}

/// **A flat drawing keeps the geometry of a shape that lists glue points, and the index of its connector's end**: the geometry names an attribute of another vocabulary (`ext:note`) that only
/// the root of the file binds, and the package made from the model bound nothing for it, so the geometry was made again from the rows (written with the points from row 0, the end named
/// 4 more). Red on `29a46bbf`.
#[test]
fn a_flat_drawing_keeps_the_geometry_of_a_shape_that_lists_glue_points_and_the_index_of_its_connector() {
    for end in [4, 5] {
        let source = Vector { points: Some(LISTED), style: "gr1", end, own_point: false, connector_first: false }.flat();
        let dir = Dir::new();
        dir.write("a.fodg", &source);
        let first = named(&leg(&dir, "a.fodg", "b.diacraft"));
        let second = named(&leg(&dir, "b.diacraft", "c.odg"));
        let written = dir.read("c.odg");
        assert_eq!(glue_ends(&written), vec![("1".to_owned(), end.to_string())], "the end {end}");
        assert!(crate::odg_build::part_of(&written, "content.xml").contains(r#"ext:note="kept""#), "the geometry the file held is written back");
        assert!(!connections(&[first, second]), "the end {end}");
    }
}

// ---------------------------------------------------------------------------------------------------------------- bug 2: the links of a flat drawing

/// The vector's shapes: a text box frame whose paragraph holds a link, and a custom shape whose paragraph holds a link inside a span.
const LINKED: &str = concat!(
    r#"<draw:frame draw:style-name="gr1" svg:x="2cm" svg:y="2cm" svg:width="8cm" svg:height="3cm"><draw:text-box>"#,
    r#"<text:p>Open <text:a xlink:type="simple" xlink:href="https://example.org/plan">the plan</text:a> first</text:p></draw:text-box></draw:frame>"#,
    r#"<draw:custom-shape draw:style-name="gr1" svg:x="2cm" svg:y="7cm" svg:width="6cm" svg:height="3cm">"#,
    r#"<text:p><text:span text:style-name="T1">Ask <text:a xlink:type="simple" xlink:href="https://example.org/desk">the desk</text:a></text:span></text:p>"#,
    r#"<draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N"/></draw:custom-shape>"#
);

/// The drawing of the vector, with the text style `T1` of 11 pt: the package, or the same drawing as one flat document.
fn linked_drawing(flat: bool) -> Vec<u8> {
    let mut drawing = Drawing::new();
    drawing.auto.push_str(r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-size="11pt"/></style:style>"#);
    let drawing = drawing.page(LINKED);
    if flat { drawing.flat_bytes() } else { drawing.bytes() }
}

/// The in-text links of a file's `content.xml` (a package's) or of the whole document (a flat drawing's): the address and the words of each, in document order.
fn links(bytes: &[u8], flat: bool) -> Vec<(String, String)> {
    let text = if flat { String::from_utf8(bytes.to_vec()).unwrap() } else { crate::odg_build::part_of(bytes, "content.xml") };
    let root = parse(&text);
    let mut list = Vec::new();
    all(&root, &mut list);
    list.iter()
        .filter(|el| el.name == "text:a")
        .map(|el| {
            let mut inner = vec![*el];
            all(el, &mut inner);
            (el.attr("xlink:href").unwrap_or_default().to_owned(), inner.iter().map(|e| e.text()).collect::<Vec<_>>().concat())
        })
        .collect()
}

/// **A flat drawing keeps its in-text links on the plain visit** (`fodg-links`): the two links were left out of the package made from the model, and the check named `hyperlinks` (2). The control is
/// the same drawing as a package. Red on `29a46bbf`.
#[test]
fn a_flat_drawing_keeps_its_in_text_links_on_the_plain_visit() {
    let source = linked_drawing(true);
    assert_eq!(links(&source, true).len(), 2, "the vector holds two links");
    let dir = Dir::new();
    dir.write("a.fodg", &source);
    let first = named(&leg(&dir, "a.fodg", "b.diacraft"));
    let second = named(&leg(&dir, "b.diacraft", "c.odg"));
    assert_eq!(links(&dir.read("c.odg"), false), links(&source, true), "both links come back with their addresses and words");
    assert_eq!((first, second), (Vec::<String>::new(), Vec::<String>::new()), "and the plain visit names nothing");
}

/// The same on a direct convert (a flat drawing saved as an `.odg`), and the control: the drawing as a package, which kept the links always.
#[test]
fn a_flat_drawing_saved_as_an_odg_keeps_its_in_text_links_and_so_does_the_package() {
    let source = linked_drawing(true);
    let dir = Dir::new();
    dir.write("a.fodg", &source);
    let said = named(&leg(&dir, "a.fodg", "b.odg"));
    assert_eq!(links(&dir.read("b.odg"), false), links(&source, true));
    assert!(said.is_empty(), "the direct convert names {said:?}");
    let package = linked_drawing(false);
    let dir = Dir::new();
    dir.write("a.odg", &package);
    let said = named(&leg(&dir, "a.odg", "b.diacraft"));
    assert_eq!(links(&package, false).len(), 2);
    assert!(said.is_empty(), "{said:?}");
    let said = named(&leg(&dir, "b.diacraft", "c.odg"));
    assert_eq!(links(&dir.read("c.odg"), false), links(&package, false));
    assert!(said.is_empty(), "{said:?}");
}
