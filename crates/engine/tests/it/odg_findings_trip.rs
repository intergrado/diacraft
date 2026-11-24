//! The `.odg` findings of the gate on `76278d7e`, from its vectors. Each test is the plain visit a person can make (`file.open`, then `file.save_as` to
//! `.diacraft`, then from it to the file's own format, one leg a session), on a file built here from the structure the vector's note gives, by the OASIS ODF standard.

use crate::native_common::Dir;
use crate::odg_build::Drawing;
use crate::save_check_misses::{all, leg, named, parse};

/// LibreOffice's `standard`: the common style the shapes' automatic style names as its parent.
const STANDARD: &str = r##"<style:style style:name="standard" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ffd966" svg:stroke-color="#3465a4"/></style:style>"##;

/// The vector's automatic styles: `gr1` over `standard`, and the text style `T1` of 11 pt.
const AUTO: &str = concat!(
    r#"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="standard"/>"#,
    r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-size="11pt"/></style:style>"#,
);

/// A drawing on `standard` and `AUTO` whose one page holds `shapes`.
fn on_standard(shapes: &str) -> Vec<u8> {
    let mut d = Drawing::new().common(STANDARD);
    d.auto = AUTO.to_owned();
    d.page(shapes).bytes()
}

/// The plain visit of `source`, an `.odg`: what each leg named, and the directory that holds `a.odg`, `b.diacraft` and `c.odg`. A leg that is refused fails the test, with the refusal's words.
fn visit(source: &[u8]) -> (Vec<String>, Vec<String>, Dir) {
    let dir = Dir::new();
    dir.write("a.odg", source);
    let first = leg(&dir, "a.odg", "b.diacraft");
    let second = leg(&dir, "b.diacraft", "c.odg");
    (named(&first), named(&second), dir)
}

/// The `draw:glue-point` elements of a package's `content.xml`: `(draw:id, svg:x, svg:y)` of each, in document order.
fn glue_points(bytes: &[u8]) -> Vec<(String, String, String)> {
    let root = parse(&crate::odg_build::part_of(bytes, "content.xml"));
    let mut list = Vec::new();
    all(&root, &mut list);
    list.iter()
        .filter(|el| el.name == "draw:glue-point")
        .map(|el| {
            let at = |name: &str| el.attr(name).unwrap_or_default().to_owned();
            (at("draw:id"), at("svg:x"), at("svg:y"))
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------------------------- finding 1: a group with glue points of its own, two levels down

/// The inner group's members: two `draw:glue-point` children of its own, a path, and a group holding one polygon.
const MEMBERS: &str = concat!(
    r#"<draw:glue-point draw:id="4" svg:x="1cm" svg:y="0.5cm" draw:escape-direction="auto"/>"#,
    r#"<draw:glue-point draw:id="5" svg:x="2cm" svg:y="1.5cm" draw:escape-direction="auto"/>"#,
    r#"<draw:path draw:style-name="gr1" draw:name="Outline" svg:x="2cm" svg:y="2cm" svg:width="4cm" svg:height="2cm" svg:viewBox="0 0 4000 2000" svg:d="M0 0L4000 0 4000 2000 0 2000 0 0Z"><text:p/></draw:path>"#,
    r#"<draw:g><draw:polygon draw:style-name="gr1" svg:x="7cm" svg:y="2cm" svg:width="2cm" svg:height="2cm" svg:viewBox="0 0 2000 2000" draw:points="0,2000 1000,0 2000,2000"/></draw:g>"#,
);

/// The vector's structure: an outer `draw:g` with no attribute, a `draw:g` with `draw:id` and `xml:id` "k1" in it, and in that a `draw:g` with no attribute that holds `members`.
fn nested(key: bool, members: &str) -> Vec<u8> {
    let middle = if key { r#"<draw:g draw:id="k1" xml:id="k1">"# } else { "<draw:g>" };
    on_standard(&format!("<draw:g>{middle}<draw:g>{members}</draw:g></draw:g></draw:g>"))
}

/// **Finding 1 (`odg-nested-group-own-glue`): the trip of a group with glue points of its own, two levels down, finishes, and keeps both points.** Since 786463e8 the second leg was refused: "a group
/// with no name that is unique among the page's groups is given data of its own or a link to a master". Red on `76278d7e`.
#[test]
fn an_unnamed_group_with_glue_points_of_its_own_two_levels_down_finishes_the_plain_visit() {
    let source = nested(true, MEMBERS);
    let (_, _, dir) = visit(&source);
    let written = glue_points(&dir.read("c.odg"));
    assert_eq!(written, glue_points(&source), "both glue points come back with their ids and positions");
    assert_eq!(written.len(), 2);
}

/// The controls of finding 1: the middle group with no key, the glue points one level down (refused on every build before this task, and finishing now: a group with glue points of its own that the
/// file's markup gives needs no entry), and no glue points. Each finishes.
#[test]
fn the_controls_of_the_nested_group_with_glue_points_finish_the_plain_visit() {
    // The middle group's id taken off.
    let source = nested(false, MEMBERS);
    let (_, _, dir) = visit(&source);
    assert_eq!(glue_points(&dir.read("c.odg")), glue_points(&source), "no key on the middle group");
    // The group with the glue points one level down.
    let one_level = on_standard(&format!(r#"<draw:g draw:id="k1" xml:id="k1"><draw:g>{MEMBERS}</draw:g></draw:g>"#));
    let (_, _, dir) = visit(&one_level);
    assert_eq!(glue_points(&dir.read("c.odg")), glue_points(&one_level), "one level down");
    // No glue points.
    let none = nested(
        true,
        &MEMBERS
            .replace(r#"<draw:glue-point draw:id="4" svg:x="1cm" svg:y="0.5cm" draw:escape-direction="auto"/>"#, "")
            .replace(r#"<draw:glue-point draw:id="5" svg:x="2cm" svg:y="1.5cm" draw:escape-direction="auto"/>"#, ""),
    );
    let (_, _, dir) = visit(&none);
    assert!(glue_points(&dir.read("c.odg")).is_empty());
}

// ---------------------------------------------------------------------------------------------------------------- finding 2: a link before a trailing space

/// A paragraph of three runs of the text style `T1` whose middle run is a hyperlink to the notes: `last` is the third run.
fn linked_paragraph(last: &str) -> String {
    format!(
        concat!(
            r#"<text:p><text:span text:style-name="T1">See the notes (</text:span>"#,
            r#"<text:span text:style-name="T1"><text:a xlink:href="https://example.org/notes" xlink:type="simple">here</text:a></text:span>"#,
            r#"<text:span text:style-name="T1">{}</text:span></text:p>"#
        ),
        last
    )
}

/// The vector's shapes: a text frame and a rectangle that each hold two linked paragraphs, the second one's last run (`) `) ending in a space.
fn linked_shapes(second_last: &str) -> String {
    let paragraphs = format!("{}{}", linked_paragraph(")."), linked_paragraph(second_last));
    format!(
        concat!(
            r#"<draw:frame draw:style-name="gr1" draw:name="Notes" svg:x="1cm" svg:y="1cm" svg:width="8cm" svg:height="3cm"><draw:text-box>{0}</draw:text-box></draw:frame>"#,
            r#"<draw:rect draw:style-name="gr1" draw:name="Box" svg:x="1cm" svg:y="6cm" svg:width="8cm" svg:height="3cm">{0}</draw:rect>"#
        ),
        paragraphs
    )
}

/// The in-text links of a package's `content.xml`: the address and the words of each, in document order.
fn links(bytes: &[u8]) -> Vec<(String, String)> {
    let root = parse(&crate::odg_build::part_of(bytes, "content.xml"));
    let mut list = Vec::new();
    all(&root, &mut list);
    list.iter()
        .filter(|el| el.name == "text:a")
        .map(|el| {
            // The words the link shows: its own text and that of the runs inside it.
            let mut inner = vec![*el];
            all(el, &mut inner);
            (el.attr("xlink:href").unwrap_or_default().to_owned(), inner.iter().map(|e| e.text()).collect::<Vec<_>>().concat())
        })
        .collect()
}

/// **Finding 2 (`odg-link-before-trailing-space`): an in-text link survives a paragraph whose last run ends in a space.** The reader took the paragraph's trailing space off its characters and left the
/// runs where they were, so the last run (and the paragraph's end) lay one character past the text, the writer could not tell the paragraph from the file's, and wrote it with no link: 4 links in, 2
/// out, the check naming `text-formatting` only. Red on `76278d7e`.
#[test]
fn an_in_text_link_survives_a_last_run_that_ends_in_a_space() {
    let source = on_standard(&linked_shapes(") "));
    assert_eq!(links(&source).len(), 4, "the vector holds four links");
    let (first, second, dir) = visit(&source);
    assert_eq!(links(&dir.read("c.odg")), links(&source), "every link comes back with its address and its words");
    assert_eq!((first, second), (Vec::<String>::new(), Vec::<String>::new()), "and the plain visit names nothing");
}

/// The control of finding 2: the same paragraphs ending in `).` on both: nothing is lost.
#[test]
fn an_in_text_link_survives_a_last_run_that_ends_in_a_stop() {
    let source = on_standard(&linked_shapes(")."));
    let (first, second, dir) = visit(&source);
    assert_eq!(links(&dir.read("c.odg")), links(&source));
    assert_eq!((first, second), (Vec::<String>::new(), Vec::<String>::new()));
}

// ---------------------------------------------------------------------------------------------------------------- finding 3: glue points of the geometry and of the shape's own

/// The vector's custom shape: a `draw:glue-point` child of its own (id 8) before an `draw:enhanced-geometry` that lists four glue points of its own `draw:glue-points`.
fn custom_shape(own: Option<&str>, geometry: bool) -> String {
    let points = if geometry { r#" draw:glue-points="10800 0 0 10800 10800 21600 21600 10800""# } else { "" };
    format!(
        concat!(
            r#"<draw:custom-shape draw:id="id1" draw:style-name="gr1" draw:name="Process" svg:x="2cm" svg:y="2cm" svg:width="4cm" svg:height="2cm">{}"#,
            r#"<draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="flowchart-process"{} draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N"/></draw:custom-shape>"#
        ),
        own.unwrap_or_default(),
        points
    )
}

/// **Finding 3 (`odg-custom-shape-geometry-glue-and-own`): the glue points of a custom shape's geometry and of its own are no `connections` difference.** The geometry's four points are rows of the
/// shape's `Connection` section, and the file's own point comes after them. Red on `76278d7e`: leg d names `connections` 1 with the written point the same as the read one.
#[test]
fn glue_points_of_the_geometry_and_of_the_shape_s_own_name_no_connections_on_the_plain_visit() {
    let own = r#"<draw:glue-point draw:id="8" svg:x="1cm" svg:y="0.5cm"/>"#;
    let source = on_standard(&custom_shape(Some(own), true));
    let (first, second, dir) = visit(&source);
    assert_eq!(glue_points(&dir.read("c.odg")), glue_points(&source), "the shape's own point comes back with its id and place");
    assert!(!first.contains(&"connections".to_owned()) && !second.contains(&"connections".to_owned()), "legs named {first:?} and {second:?}");
    // The controls: the geometry's points alone, and the shape's own point alone.
    for (own, geometry) in [(None, true), (Some(r#"<draw:glue-point draw:id="4" svg:x="1cm" svg:y="0.5cm"/>"#), false)] {
        let source = on_standard(&custom_shape(own, geometry));
        let (first, second, _) = visit(&source);
        assert!(
            !first.contains(&"connections".to_owned()) && !second.contains(&"connections".to_owned()),
            "control {own:?} {geometry}: {first:?} and {second:?}"
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------- finding 4: neighbouring runs of one style

/// A rectangle whose one paragraph is the vector's three runs of `T1`; `middle` is what the middle run is made of.
fn three_runs(middle: &str) -> String {
    format!(
        concat!(
            r#"<draw:rect draw:style-name="gr1" draw:name="Notes" svg:x="1cm" svg:y="1cm" svg:width="8cm" svg:height="3cm"><text:p>"#,
            r#"<text:span text:style-name="T1">See the notes (</text:span>{}<text:span text:style-name="T1">).</text:span></text:p></draw:rect>"#
        ),
        middle
    )
}

/// **Finding 4 (`odg-runs-same-style`): runs of one style the writer joins are the same text and formatting.** The check compared the runs of a paragraph by their number, and the file's one run
/// against the drawing's three was a change of formatting. Red on `76278d7e`: leg d names `text-formatting` 1.
#[test]
fn three_runs_of_one_style_written_as_one_name_nothing() {
    let source = on_standard(&three_runs(r#"<text:span text:style-name="T1">here</text:span>"#));
    let (first, second, _) = visit(&source);
    assert_eq!((first, second), (Vec::<String>::new(), Vec::<String>::new()));
}

/// The controls of finding 4: one run of the same words names nothing, and the middle run inside a link, which stays a run of its own, names nothing either.
#[test]
fn the_controls_of_the_runs_of_one_style_name_nothing() {
    let one = on_standard(
        r#"<draw:rect draw:style-name="gr1" draw:name="Notes" svg:x="1cm" svg:y="1cm" svg:width="8cm" svg:height="3cm"><text:p><text:span text:style-name="T1">See the notes (here).</text:span></text:p></draw:rect>"#,
    );
    let (first, second, _) = visit(&one);
    assert_eq!((first, second), (Vec::<String>::new(), Vec::<String>::new()), "one run");
    let linked = on_standard(&three_runs(
        r#"<text:a xlink:href="https://example.org/notes" xlink:type="simple"><text:span text:style-name="T1">here</text:span></text:a>"#,
    ));
    let (first, second, dir) = visit(&linked);
    assert_eq!(links(&dir.read("c.odg")), links(&linked), "the link comes back");
    assert_eq!((first, second), (Vec::<String>::new(), Vec::<String>::new()), "a link between two runs of one style");
}
