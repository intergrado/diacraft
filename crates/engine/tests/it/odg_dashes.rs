//! A dashed or dotted line is a dash in an `.odg`, so another program draws it dashed.
//!
//! Every `LinePattern` from 2 to 23 is written as `draw:stroke="dash"` with a `draw:stroke-dash` of DiaCraft's own in `office:styles` (`DiaCraft_20_dash_20_<n>`, and `_20_flat` after it
//! for a flat cap's form), whose lengths are percentages of the line's weight, and is read back as the same pattern. These tests write each route a drawing takes: a new drawing (the model
//! writer), a drawing read from a file (the splice), a `.diacraft` file saved as an `.odg`, a VSDX drawing saved as an `.odg`, and an `.odg` saved as VSDX.

use diacraft_engine::Session;
use diacraft_odg::Limits;
use diacraft_odg::package::Package;
use diacraft_testkit::package::{PageSpec, VsdxBuilder};
use serde_json::{Value, json};

use crate::odg_build::Dir;
use crate::{blank_session, run};

/// What the table of gives each pattern as ODF says it, for a round or projecting cap: `(pattern, dots1, length1, dots2, length2, distance)` with the lengths
/// and the distance in percent of the weight. A mark of 0 is a dot, and `dots2` of 0 says no second kind of mark.
const ROUND: [(u32, u32, u32, u32, u32, u32); 22] = [
    (2, 1, 700, 0, 0, 500),
    (3, 1, 0, 0, 0, 500),
    (4, 1, 700, 1, 0, 500),
    (5, 1, 700, 2, 0, 500),
    (6, 2, 700, 1, 0, 500),
    (7, 1, 1900, 1, 700, 500),
    (8, 1, 1900, 2, 700, 500),
    (9, 1, 300, 0, 0, 300),
    (10, 1, 0, 0, 0, 300),
    (11, 1, 300, 1, 0, 300),
    (12, 1, 300, 2, 0, 300),
    (13, 2, 300, 1, 0, 300),
    (14, 1, 900, 1, 300, 300),
    (15, 1, 900, 2, 300, 300),
    (16, 1, 1500, 0, 0, 900),
    (17, 1, 0, 0, 0, 900),
    (18, 1, 1500, 1, 0, 900),
    (19, 1, 1500, 2, 0, 900),
    (20, 2, 1500, 1, 0, 900),
    (21, 1, 3900, 1, 1500, 900),
    (22, 1, 3900, 2, 1500, 900),
    (23, 1, 100, 0, 0, 200),
];

/// The same under a flat cap: each mark a weight longer and each gap a weight shorter.
const FLAT: [(u32, u32, u32, u32, u32, u32); 22] = [
    (2, 1, 800, 0, 0, 400),
    (3, 1, 100, 0, 0, 400),
    (4, 1, 800, 1, 100, 400),
    (5, 1, 800, 2, 100, 400),
    (6, 2, 800, 1, 100, 400),
    (7, 1, 2000, 1, 800, 400),
    (8, 1, 2000, 2, 800, 400),
    (9, 1, 400, 0, 0, 200),
    (10, 1, 100, 0, 0, 200),
    (11, 1, 400, 1, 100, 200),
    (12, 1, 400, 2, 100, 200),
    (13, 2, 400, 1, 100, 200),
    (14, 1, 1000, 1, 400, 200),
    (15, 1, 1000, 2, 400, 200),
    (16, 1, 1600, 0, 0, 800),
    (17, 1, 100, 0, 0, 800),
    (18, 1, 1600, 1, 100, 800),
    (19, 1, 1600, 2, 100, 800),
    (20, 2, 1600, 1, 100, 800),
    (21, 1, 4000, 1, 1600, 800),
    (22, 1, 4000, 2, 1600, 800),
    (23, 1, 200, 0, 0, 100),
];

fn reopened(path: &str) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

fn page_id(s: &Session) -> u32 {
    s.document().unwrap().pages.iter().find(|p| !p.background).unwrap().id
}

fn shape_ids(s: &Session) -> Vec<u32> {
    let page = s.document().unwrap().pages.iter().find(|p| !p.background).unwrap();
    page.shapes.iter().map(|shape| shape.id).collect()
}

/// The line pattern and the cap of each shape of the page, as `shape.formatting` says them.
fn lines(s: &mut Session) -> Vec<(u64, u64, String)> {
    let page = page_id(s);
    let ids = shape_ids(s);
    let answer = run(s, "shape.formatting", json!({"ids": ids, "page": page}));
    answer["shapes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|look| (look["id"].as_u64().unwrap(), look["pattern"].as_u64().unwrap_or(u64::MAX), look["line_cap"].as_str().unwrap_or("").to_owned()))
        .collect()
}

/// A rectangle for each pattern from 2 to 23, dropped in rows of eight and set to the pattern.
fn rectangles(s: &mut Session, cap: Option<&str>) -> Vec<u32> {
    let mut ids = Vec::new();
    for (k, n) in (2..=23u32).enumerate() {
        let (c, r) = ((k % 8) as f64, (k / 8) as f64);
        let id =
            run(s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 1.2 + 1.2 * c, "y": 1.0 + 1.0 * r}))["id"].as_u64().unwrap() as u32;
        let mut format = json!({"ids": [id], "pattern": n});
        if let Some(cap) = cap {
            format["line_cap"] = json!(cap);
        }
        run(s, "shape.format", format);
        ids.push(id);
    }
    ids
}

fn parts(path: &str) -> (String, String) {
    let bytes = std::fs::read(path).unwrap();
    let pkg = Package::read(&bytes, &Limits::default()).unwrap();
    let text = |name: &str| String::from_utf8_lossy(pkg.part(name).unwrap()).into_owned();
    (text("content.xml"), text("styles.xml"))
}

/// The `draw:stroke-dash` elements of a part, each as its text.
fn dash_elements(styles: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = styles;
    while let Some(at) = rest.find("<draw:stroke-dash ") {
        let tail = &rest[at..];
        let end = tail.find("/>").unwrap_or(tail.len());
        out.push(&tail[..end]);
        rest = &tail[end..];
    }
    out
}

/// The value of attribute `name` in an element's text.
fn attribute<'a>(element: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let at = element.find(&key)? + key.len();
    let tail = &element[at..];
    Some(&tail[..tail.find('"')?])
}

/// What a dash element says, as `(dots1, length1, dots2, length2, distance)` in percent; `dots2` and its length are 0 when absent.
fn dash_numbers(element: &str) -> (u32, u32, u32, u32, u32) {
    let number = |name: &str| attribute(element, name).map(|t| t.trim_end_matches('%').parse::<u32>().unwrap()).unwrap_or(0);
    (number("draw:dots1"), number("draw:dots1-length"), number("draw:dots2"), number("draw:dots2-length"), number("draw:distance"))
}

/// Checks the dash definitions of `styles` against `table`, under the names each pattern has (`suffix` is `_20_flat` or none): one definition for each of the patterns in `patterns`, once.
fn check_definitions(styles: &str, table: &[(u32, u32, u32, u32, u32, u32)], suffix: &str, patterns: &[u32]) {
    let elements = dash_elements(styles);
    assert_eq!(elements.len(), patterns.len(), "one definition for each pattern the drawing uses: {styles}");
    for &n in patterns {
        let name = format!("DiaCraft_20_dash_20_{n}{suffix}");
        let found: Vec<&&str> = elements.iter().filter(|e| attribute(e, "draw:name") == Some(name.as_str())).collect();
        assert_eq!(found.len(), 1, "{name} is defined once: {styles}");
        let want = table.iter().find(|row| row.0 == n).unwrap();
        assert_eq!(dash_numbers(found[0]), (want.1, want.2, want.3, want.4, want.5), "{name}: {}", found[0]);
        assert_eq!(attribute(found[0], "draw:display-name"), Some(name.replace("_20_", " ").as_str()), "{name} has a display name with spaces");
    }
}

#[test]
fn every_line_pattern_of_a_new_drawing_is_written_as_a_dash_and_read_back_as_the_same_pattern() {
    let mut s = blank_session();
    let ids = rectangles(&mut s, None);
    let dir = Dir::new();
    let path = dir.path("dashes.odg");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let (content, styles) = parts(&path);
    assert_eq!(content.matches("draw:stroke=\"dash\"").count(), 22, "every one of the 22 patterns is a dash: {content}");
    for n in 2..=23u32 {
        assert!(content.contains(&format!("draw:stroke-dash=\"DiaCraft_20_dash_20_{n}\"")), "pattern {n} names its dash: {content}");
    }
    let all: Vec<u32> = (2..=23).collect();
    check_definitions(&styles, &ROUND, "", &all);
    let mut back = reopened(&path);
    let got: Vec<u64> = lines(&mut back).iter().map(|l| l.1).collect();
    assert_eq!(got, all.iter().map(|n| u64::from(*n)).collect::<Vec<_>>(), "each rectangle has its pattern again ({ids:?})");
    // A second save of the drawing that was read writes the same markup: no definition is added twice and none is lost.
    run(&mut back, "file.save", json!({}));
    let (content_again, styles_again) = parts(&path);
    assert_eq!(content_again.matches("draw:stroke=\"dash\"").count(), 22);
    check_definitions(&styles_again, &ROUND, "", &all);
    let got: Vec<u64> = lines(&mut reopened(&path)).iter().map(|l| l.1).collect();
    assert_eq!(got, all.iter().map(|n| u64::from(*n)).collect::<Vec<_>>(), "and the pattern is still each one's after a second save");
}

#[test]
fn a_flat_cap_gets_the_flat_form_of_each_dash_and_both_come_back() {
    let mut s = blank_session();
    rectangles(&mut s, Some("flat"));
    let dir = Dir::new();
    let path = dir.path("flat.odg");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let (content, styles) = parts(&path);
    let all: Vec<u32> = (2..=23).collect();
    assert_eq!(content.matches("svg:stroke-linecap=\"butt\"").count(), 22, "{content}");
    check_definitions(&styles, &FLAT, "_20_flat", &all);
    let got = lines(&mut reopened(&path));
    assert_eq!(got.iter().map(|l| l.1).collect::<Vec<_>>(), all.iter().map(|n| u64::from(*n)).collect::<Vec<_>>());
    assert!(got.iter().all(|l| l.2 == "flat"), "{got:?}");
}

#[test]
fn a_line_that_is_solid_or_none_names_no_dash_and_a_drawing_with_no_dash_defines_none() {
    let mut s = blank_session();
    let a = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2.0, "y": 2.0}))["id"].as_u64().unwrap();
    let b = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 4.0, "y": 2.0}))["id"].as_u64().unwrap();
    run(&mut s, "shape.format", json!({"ids": [a], "pattern": 1}));
    run(&mut s, "shape.format", json!({"ids": [b], "pattern": 0}));
    let dir = Dir::new();
    let path = dir.path("plain.odg");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let (content, styles) = parts(&path);
    assert!(!content.contains("draw:stroke-dash") && !styles.contains("<draw:stroke-dash"), "{content}\n{styles}");
}

/// A VSDX drawing with one rectangle whose line pattern is `own` and another that follows a style sheet whose line pattern is `sheet`.
fn vsdx_with_patterns(own: u32, sheet: u32) -> Vec<u8> {
    let rect = |id: u32, x: f64, line_style: u32, own: Option<u32>| {
        let pattern = own.map(|p| format!(r#"<Cell N="LinePattern" V="{p}"/>"#)).unwrap_or_default();
        format!(
            concat!(
                r#"<Shape ID="{id}" Type="Shape" LineStyle="{style}" FillStyle="0" TextStyle="0"><Cell N="PinX" V="{x}"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/>"#,
                r#"<Cell N="Height" V="1"/><Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>{pattern}"#,
                r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row>"#,
                r#"<Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width*1"/><Cell N="Y" V="0" F="Height*0"/></Row>"#,
                r#"<Row T="LineTo" IX="3"><Cell N="X" V="2" F="Width*1"/><Cell N="Y" V="1" F="Height*1"/></Row>"#,
                r#"<Row T="LineTo" IX="4"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="1" F="Height*1"/></Row>"#,
                r#"<Row T="LineTo" IX="5"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row></Section></Shape>"#
            ),
            id = id,
            x = x,
            style = line_style,
            pattern = pattern
        )
    };
    let styles = format!(
        concat!(
            r#"<StyleSheet ID="0" NameU="No Style" Name="No Style"><Cell N="LineWeight" V="0.01"/><Cell N="LinePattern" V="1"/></StyleSheet>"#,
            r#"<StyleSheet ID="1" NameU="Dashed" Name="Dashed" LineStyle="0" FillStyle="0" TextStyle="0"><Cell N="LinePattern" V="{sheet}"/></StyleSheet>"#
        ),
        sheet = sheet
    );
    VsdxBuilder::new()
        .styles(&styles)
        .page(PageSpec::new(1, "Page-1").sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#).shapes(&format!(
            "{}{}",
            rect(1, 2.0, 0, Some(own)),
            rect(2, 5.0, 1, None)
        )))
        .build()
}

#[test]
fn a_vsdx_drawing_saved_as_odg_writes_each_shapes_pattern_as_a_dash_and_both_patterns_come_back() {
    let dir = Dir::new();
    let source = dir.write("source.vsdx", &vsdx_with_patterns(5, 9));
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": source }));
    let before = lines(&mut s);
    assert_eq!(before.iter().map(|l| l.1).collect::<Vec<_>>(), [5, 9], "the VSDX drawing's own pattern, and the sheet's through the style");
    let path = dir.path("across.odg");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let (content, styles) = parts(&path);
    // Each shape's pattern is a dash of its own automatic style, whether the pattern is the shape's own or its style sheet's (the model writer writes what a shape draws with, the cells its
    // style sheets give it too), and each dash is defined once in `office:styles`. A style sheet of a VSDX drawing is itself written as an empty common style, carried by DiaCraft's data.
    for n in [5, 9] {
        assert!(content.contains(&format!("draw:stroke-dash=\"DiaCraft_20_dash_20_{n}\"")), "pattern {n} is a dash in a shape's style: {content}");
    }
    check_definitions(&styles, &ROUND, "", &[5, 9]);
    let after = lines(&mut reopened(&path));
    assert_eq!(after.iter().map(|l| l.1).collect::<Vec<_>>(), [5, 9], "both patterns come back");
}

#[test]
fn an_odg_with_dashes_saved_as_vsdx_keeps_every_pattern() {
    let mut s = blank_session();
    rectangles(&mut s, None);
    let dir = Dir::new();
    let odg = dir.path("dashes.odg");
    run(&mut s, "file.save_as", json!({ "path": odg }));
    let mut from_odg = reopened(&odg);
    let vsdx = dir.path("dashes.vsdx");
    run(&mut from_odg, "file.save_as", json!({ "path": vsdx }));
    let back = lines(&mut reopened(&vsdx));
    assert_eq!(back.iter().map(|l| l.1).collect::<Vec<_>>(), (2..=23u64).collect::<Vec<_>>());
}

#[test]
fn a_diacraft_file_saved_as_odg_writes_the_dashes_too() {
    let mut s = blank_session();
    rectangles(&mut s, None);
    let dir = Dir::new();
    let native = dir.path("dashes.diacraft");
    run(&mut s, "file.save_as", json!({ "path": native }));
    let mut from_native = reopened(&native);
    let odg = dir.path("from_native.odg");
    run(&mut from_native, "file.save_as", json!({ "path": odg }));
    let (content, styles) = parts(&odg);
    assert_eq!(content.matches("draw:stroke=\"dash\"").count(), 22, "{content}");
    let all: Vec<u32> = (2..=23).collect();
    check_definitions(&styles, &ROUND, "", &all);
    let back = lines(&mut reopened(&odg));
    assert_eq!(back.iter().map(|l| l.1).collect::<Vec<_>>(), (2..=23u64).collect::<Vec<_>>());
}

/// The masters of DiaCraft's own stencils whose line is dashed or dotted, as the drawings a person makes with them hold them: the UML realization, dependency and note link, an ERD
/// relationship (whose pattern follows its `Identifying` data), BPMN's message flow and association and Chen's derived attribute. Each is dropped on a page and the drawing saved as `.odg`.
const DASHED_MASTERS: [(&str, &str); 7] = [
    ("uml-class", "uml-class.realization"),
    ("uml-class", "uml-class.dependency"),
    ("uml-class", "uml-class.note-link"),
    ("crows-foot", "crows-foot.relationship"),
    ("bpmn", "bpmn.message-flow"),
    ("bpmn", "bpmn.association"),
    ("chen", "chen.derived-attribute"),
];

#[test]
fn the_dashed_and_dotted_lines_of_the_stencils_are_dashes_in_an_odg_and_come_back() {
    let mut s = blank_session();
    for (k, (stencil, master)) in DASHED_MASTERS.iter().enumerate() {
        run(&mut s, "stencil.open", json!({ "id": stencil }));
        run(&mut s, "shape.drop", json!({"master": master, "page": 1, "x": 2.0 + 1.5 * k as f64, "y": 2.0 + 0.8 * k as f64}));
    }
    let want: Vec<u64> = lines(&mut s).iter().map(|l| l.1).collect();
    let dashed: Vec<u64> = want.iter().copied().filter(|n| (2..=23).contains(n)).collect();
    assert!(dashed.len() >= 6, "the masters draw dashed or dotted lines: {want:?}");
    let dir = Dir::new();
    let path = dir.path("stencils.odg");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let (content, styles) = parts(&path);
    assert_eq!(
        content.matches("draw:stroke=\"dash\"").count(),
        dashed.len(),
        "every dashed master is a dash another program draws: {want:?}\n{content}"
    );
    let mut kinds: Vec<u32> = dashed.iter().map(|n| *n as u32).collect();
    kinds.sort_unstable();
    kinds.dedup();
    check_definitions(&styles, &ROUND, "", &kinds);
    let back: Vec<u64> = lines(&mut reopened(&path)).iter().map(|l| l.1).collect();
    assert_eq!(back, want, "and each comes back as the pattern it had");
}

/// Two rectangles on one page of a hand-made drawing that no program of ours wrote: `A` on the automatic style `gr1` (a solid line) and `B` on `gr1` too.
fn plain_drawing() -> crate::odg_build::Drawing {
    use crate::odg_build::{Drawing, rect};
    Drawing::new().page(&format!("{}{}", rect("A", (2.0, 2.0, 4.0, 2.0), ""), rect("B", (8.0, 2.0, 4.0, 2.0), "")))
}

#[test]
fn a_pattern_set_on_a_shape_of_a_file_is_written_into_it_and_a_cap_set_after_it_renames_the_dash() {
    let dir = Dir::new();
    let path = dir.write("a.odg", &plain_drawing().bytes());
    let mut s = reopened(&path);
    let (a, _b) = {
        let ids = shape_ids(&s);
        (ids[0], ids[1])
    };
    let page = page_id(&s);
    run(&mut s, "shape.format", json!({"ids": [a], "page": page, "pattern": 7}));
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["edited"], true, "{saved}");
    let (content, styles) = parts(&path);
    assert_eq!(content.matches("draw:stroke-dash=\"DiaCraft_20_dash_20_7\"").count(), 1, "{content}");
    check_definitions(&styles, &ROUND, "", &[7]);
    assert_eq!(lines(&mut reopened(&path))[0].1, 7);
    // A flat cap after it: the dash is the flat form, named for it, and the round one that no style names is taken out.
    run(&mut s, "shape.format", json!({"ids": [a], "page": page, "line_cap": "flat"}));
    run(&mut s, "file.save", json!({}));
    let (content, styles) = parts(&path);
    assert!(
        content.contains("draw:stroke-dash=\"DiaCraft_20_dash_20_7_20_flat\"") && !content.contains("draw:stroke-dash=\"DiaCraft_20_dash_20_7\""),
        "{content}"
    );
    check_definitions(&styles, &FLAT, "_20_flat", &[7]);
    let back = lines(&mut reopened(&path));
    assert_eq!((back[0].1, back[0].2.as_str()), (7, "flat"), "{back:?}");
    // And back to a round one: the round form again.
    run(&mut s, "shape.format", json!({"ids": [a], "page": page, "line_cap": "round"}));
    run(&mut s, "file.save", json!({}));
    let (content, styles) = parts(&path);
    assert!(content.contains("draw:stroke-dash=\"DiaCraft_20_dash_20_7\"") && !content.contains("_20_flat"), "{content}");
    check_definitions(&styles, &ROUND, "", &[7]);
    // Another pattern: the new dash, and no definition left for the old one.
    run(&mut s, "shape.format", json!({"ids": [a], "page": page, "pattern": 12}));
    run(&mut s, "file.save", json!({}));
    let (_, styles) = parts(&path);
    check_definitions(&styles, &ROUND, "", &[12]);
    assert_eq!(lines(&mut reopened(&path))[0].1, 12);
    // Solid again: the line says solid, and the pattern is 1.
    run(&mut s, "shape.format", json!({"ids": [a], "page": page, "pattern": 1}));
    run(&mut s, "file.save", json!({}));
    let (content, _) = parts(&path);
    assert!(!content.contains("draw:stroke=\"dash\""), "{content}");
    assert_eq!(lines(&mut reopened(&path))[0].1, 1);
}

#[test]
fn a_pattern_set_on_a_shape_of_a_flat_drawing_is_written_into_it_with_its_definition() {
    let dir = Dir::new();
    let path = dir.write("a.fodg", &plain_drawing().flat_bytes());
    let mut s = reopened(&path);
    let page = page_id(&s);
    let a = shape_ids(&s)[0];
    run(&mut s, "shape.format", json!({"ids": [a], "page": page, "pattern": 13, "line_cap": "flat"}));
    run(&mut s, "file.save", json!({}));
    let text = String::from_utf8(std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(text.matches("draw:stroke-dash=\"DiaCraft_20_dash_20_13_20_flat\"").count(), 1, "{text}");
    check_definitions(&text, &FLAT, "_20_flat", &[13]);
    let back = lines(&mut reopened(&path));
    assert_eq!((back[0].1, back[0].2.as_str()), (13, "flat"), "{back:?}");
    // And once more, with the cap round again: the definition no style names is taken out of the flat document too.
    run(&mut s, "shape.format", json!({"ids": [a], "page": page, "line_cap": "round"}));
    run(&mut s, "file.save", json!({}));
    let text = String::from_utf8(std::fs::read(&path).unwrap()).unwrap();
    check_definitions(&text, &ROUND, "", &[13]);
}

#[test]
fn a_drawing_with_the_same_dash_on_two_shapes_defines_it_once_and_a_second_save_adds_nothing() {
    let dir = Dir::new();
    let path = dir.write("a.odg", &plain_drawing().bytes());
    let mut s = reopened(&path);
    let page = page_id(&s);
    let ids = shape_ids(&s);
    run(&mut s, "shape.format", json!({"ids": ids, "page": page, "pattern": 4}));
    run(&mut s, "file.save", json!({}));
    let (content, styles) = parts(&path);
    assert_eq!(content.matches("draw:stroke-dash=\"DiaCraft_20_dash_20_4\"").count(), 2, "{content}");
    check_definitions(&styles, &ROUND, "", &[4]);
    let before = std::fs::read(&path).unwrap();
    let again = run(&mut s, "file.save", json!({}));
    assert_eq!(again["edited"], false);
    assert_eq!(std::fs::read(&path).unwrap(), before, "a save that changes nothing leaves the file as it was");
}

/// A hand-made drawing whose common styles define a dash of another make (absolute lengths, as LibreOffice writes its own: 0.15 cm marks and gaps on a 0.05 cm line are the table's 3, 3) and
/// one of DiaCraft's own, and whose shapes `A` (the other make's) and `B` (ours, pattern 4) draw with them.
fn drawing_with_dashes() -> crate::odg_build::Drawing {
    use crate::odg_build::{Drawing, rect_on};
    let common = concat!(
        r#"<draw:stroke-dash draw:name="Fine_20_Dots" draw:display-name="Fine Dots" draw:style="rect" draw:dots1="1" draw:dots1-length="0.15cm" draw:distance="0.15cm"/>"#,
        r#"<draw:stroke-dash draw:name="DiaCraft_20_dash_20_4" draw:display-name="DiaCraft dash 4" draw:style="rect" draw:dots1="1" draw:dots1-length="700%" draw:dots2="1" draw:dots2-length="0%" draw:distance="500%"/>"#
    );
    let styles = concat!(
        r##"<style:style style:name="gr3" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#3465a4" draw:stroke="dash" draw:stroke-dash="Fine_20_Dots" svg:stroke-width="0.05cm" svg:stroke-color="#000000" draw:auto-grow-height="false"/></style:style>"##,
        r##"<style:style style:name="gr4" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#cc0000" draw:stroke="dash" draw:stroke-dash="DiaCraft_20_dash_20_4" svg:stroke-width="0.05cm" svg:stroke-color="#000000" draw:auto-grow-height="false"/></style:style>"##
    );
    let mut drawing = Drawing::new().common(common);
    drawing.auto.push_str(styles);
    drawing.page(&format!("{}{}", rect_on("gr3", "A", (2.0, 2.0, 4.0, 2.0), ""), rect_on("gr4", "B", (8.0, 2.0, 4.0, 2.0), "")))
}

#[test]
fn a_file_that_holds_dashes_reads_them_saves_unedited_as_it_was_and_keeps_the_dash_it_did_not_write() {
    let dir = Dir::new();
    let bytes = drawing_with_dashes().bytes();
    let path = dir.write("a.odg", &bytes);
    let mut s = reopened(&path);
    assert_eq!(lines(&mut s).iter().map(|l| l.1).collect::<Vec<_>>(), [9, 4], "the other make's dash by its lengths in weights, ours by its name");
    let first = run(&mut s, "file.save", json!({}));
    assert_eq!(first["edited"], false, "{first}");
    assert_eq!(std::fs::read(&path).unwrap(), bytes, "a plain save of an unedited file changes nothing");
    // An edit of something else leaves both dashes and both definitions as they were.
    let page = page_id(&s);
    let ids = shape_ids(&s);
    run(&mut s, "shape.format", json!({"ids": ids, "page": page, "line": "#336699"}));
    run(&mut s, "file.save", json!({}));
    let (content, styles) = parts(&path);
    assert!(content.contains("draw:stroke-dash=\"Fine_20_Dots\"") && content.contains("draw:stroke-dash=\"DiaCraft_20_dash_20_4\""), "{content}");
    assert_eq!(dash_elements(&styles).len(), 2, "{styles}");
    assert_eq!(lines(&mut reopened(&path)).iter().map(|l| l.1).collect::<Vec<_>>(), [9, 4]);
    // The other make's dash set to a pattern of the table is ours now, and its own definition stays (it is another program's, and no save of ours takes it out).
    run(&mut s, "shape.format", json!({"ids": [ids[0]], "page": page, "pattern": 5}));
    run(&mut s, "file.save", json!({}));
    let (content, styles) = parts(&path);
    assert!(content.contains("draw:stroke-dash=\"DiaCraft_20_dash_20_5\""), "{content}");
    assert!(styles.contains("draw:name=\"Fine_20_Dots\""), "the definition of another make stays: {styles}");
    assert_eq!(lines(&mut reopened(&path)).iter().map(|l| l.1).collect::<Vec<_>>(), [5, 4]);
}

#[test]
fn a_hand_made_odg_with_dashes_and_no_data_of_ours_saved_as_vsdx_keeps_the_patterns() {
    let dir = Dir::new();
    let path = dir.write("a.odg", &drawing_with_dashes().bytes());
    let mut s = reopened(&path);
    let vsdx = dir.path("a.vsdx");
    run(&mut s, "file.save_as", json!({ "path": vsdx }));
    let back = lines(&mut reopened(&vsdx));
    assert_eq!(back.iter().map(|l| l.1).collect::<Vec<_>>(), [9, 4]);
    // And the other way round: the VSDX drawing saved as an `.odg` again writes the dash of another make as it held it (its stamp, the pattern 9 it was read as, still holds) and ours by the cells.
    let mut from_vsdx = reopened(&vsdx);
    let odg = dir.path("b.odg");
    run(&mut from_vsdx, "file.save_as", json!({ "path": odg }));
    let (content, styles) = parts(&odg);
    assert!(content.contains("draw:stroke-dash=\"Fine_20_Dots\"") && content.contains("draw:stroke-dash=\"DiaCraft_20_dash_20_4\""), "{content}");
    assert_eq!(dash_elements(&styles).len(), 2, "the definition of the other make as it was, and ours: {styles}");
    assert_eq!(lines(&mut reopened(&odg)).iter().map(|l| l.1).collect::<Vec<_>>(), [9, 4]);
}

/// The `LinePattern` cell of each shape of the page: its number and its formula.
fn pattern_cells(s: &Session) -> Vec<(Option<f64>, Option<String>)> {
    let doc = s.document().unwrap();
    let page = doc.pages.iter().find(|p| !p.background).unwrap();
    page.shapes
        .iter()
        .map(|shape| {
            let cell = shape.sheet.cell("LinePattern");
            (cell.and_then(|c| c.value.as_ref()?.number()), cell.and_then(|c| c.formula.text().map(str::to_owned)))
        })
        .collect()
}

/// A hand-made drawing whose common style `Dashed` is solid and whose shape `A` follows it: the style sheet a program of another make wrote.
fn drawing_with_a_style_sheet() -> crate::odg_build::Drawing {
    use crate::odg_build::{Drawing, rect_on};
    let common = r##"<style:style style:name="Dashed" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#3465a4" draw:stroke="solid" svg:stroke-width="0.05cm" svg:stroke-color="#000000"/></style:style>"##;
    let mut drawing = Drawing::new().common(common);
    drawing.auto.push_str(r#"<style:style style:name="gr3" style:family="graphic" style:parent-style-name="Dashed"/>"#);
    drawing.page(&rect_on("gr3", "A", (2.0, 2.0, 4.0, 2.0), ""))
}

#[test]
fn a_style_sheet_whose_line_pattern_was_edited_is_a_dash_with_its_definition_in_a_drawing_saved_anew() {
    let dir = Dir::new();
    let mut s = crate::odg_build::open_bytes_in(&dir, "a.odg", &drawing_with_a_style_sheet().bytes());
    let doc = s.document().unwrap();
    let id =
        doc.styles.iter().find(|st| st.name.as_deref() == Some("Dashed")).map(|st| st.id).expect("the common style is a style sheet of the model");
    assert_eq!(lines(&mut s)[0].1, 1, "the shape draws the sheet's solid line");
    run(&mut s, "sheet.set_cells", json!({"style": id, "writes": [{"cell": "LinePattern", "formula": "6"}]}));
    assert_eq!(lines(&mut s)[0].1, 6, "and the dashed one after the sheet's pattern is edited");
    // A drawing opened from an `.odg` does not save a changed style sheet into it: the `.diacraft` file holds it, and the `.odg` made from that is written anew from the model.
    let native = dir.path("b.diacraft");
    run(&mut s, "file.save_as", json!({ "path": native }));
    let path = dir.path("b.odg");
    run(&mut reopened(&native), "file.save_as", json!({ "path": path }));
    let (content, styles) = parts(&path);
    assert!(styles.contains("draw:stroke-dash=\"DiaCraft_20_dash_20_6\""), "the common style names the dash another program draws it with: {styles}");
    check_definitions(&styles, &ROUND, "", &[6]);
    assert!(!content.contains("DiaCraft_20_dash"), "the shape follows the style and names no dash of its own: {content}");
    assert_eq!(lines(&mut reopened(&path))[0].1, 6, "and the pattern comes back through the style");
}

#[test]
fn a_pattern_the_table_has_no_row_for_is_carried_whole_and_the_pattern_a_formula_works_out_to_is_a_dash() {
    let mut s = blank_session();
    let mut ids = Vec::new();
    for k in 0..4 {
        ids.push(
            run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 1.5 + 1.5 * f64::from(k), "y": 2.0}))["id"].as_u64().unwrap(),
        );
    }
    let page = page_id(&s);
    // 254 is a custom pattern (a master drawn along the line), 24 and 2.5 are numbers the table has no row for: DiaCraft draws them solid and keeps the cell.
    // The fourth is a formula that works out to 5, a pattern of the table.
    let writes: Vec<Value> = [(ids[0], "254"), (ids[1], "24"), (ids[2], "2.5"), (ids[3], "=GUARD(5)")]
        .iter()
        .map(|(id, formula)| json!({"id": id, "cell": "LinePattern", "formula": formula}))
        .collect();
    run(&mut s, "sheet.set_cells", json!({"page": page, "writes": writes}));
    let want: Vec<Option<f64>> = pattern_cells(&s).iter().map(|c| c.0).collect();
    assert_eq!(want, [Some(254.0), Some(24.0), Some(2.5), Some(5.0)]);
    let dir = Dir::new();
    let path = dir.path("odd.odg");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let (content, styles) = parts(&path);
    // Only the formula's value is a dash: a number the table has no row for is no dash (the style says nothing of the stroke, and DiaCraft draws those solid).
    assert_eq!(content.matches("draw:stroke=\"dash\"").count(), 1, "{content}");
    assert!(content.contains("draw:stroke-dash=\"DiaCraft_20_dash_20_5\""), "{content}");
    check_definitions(&styles, &ROUND, "", &[5]);
    // Every number comes back as it was, and the formula beside the one that has one (a new shape's style cells were written as the constants they work out to, so
    // a formula in a pattern cell was lost; the dash shows the value and DiaCraft's data carries the formula).
    let back = pattern_cells(&reopened(&path));
    assert_eq!(back.iter().map(|c| c.0).collect::<Vec<_>>(), want);
    assert_eq!(back[3], (Some(5.0), Some("GUARD(5)".to_owned())), "the formula comes back with its value: {back:?}");
}

/// A page of lines for the black-box look: 22 connectors with the patterns 2 to 23 under a round cap (left) and under a flat one (right), at the weight `weight` points.
fn lines_page(weight: f64) -> Session {
    let mut s = blank_session();
    for (k, n) in (2..=23u32).enumerate() {
        let y = 0.7 + 0.33 * k as f64;
        for (x0, cap) in [(0.6, "round"), (4.6, "flat")] {
            let id = run(&mut s, "connector.add", json!({"from": {"x": x0, "y": y}, "to": {"x": x0 + 3.6, "y": y}}))["id"].as_u64().unwrap();
            run(&mut s, "shape.format", json!({"ids": [id], "pattern": n, "weight": weight, "line_cap": cap}));
        }
    }
    s
}

/// Every pattern in the round form and in the flat form, once each.
fn check_definitions_both(styles: &str, patterns: &[u32]) {
    let elements = dash_elements(styles);
    for &n in patterns {
        for (table, suffix) in [(&ROUND, ""), (&FLAT, "_20_flat")] {
            let name = format!("DiaCraft_20_dash_20_{n}{suffix}");
            let found: Vec<&&str> = elements.iter().filter(|e| attribute(e, "draw:name") == Some(name.as_str())).collect();
            assert_eq!(found.len(), 1, "{name} is defined once");
            let want = table.iter().find(|row| row.0 == n).unwrap();
            assert_eq!(dash_numbers(found[0]), (want.1, want.2, want.3, want.4, want.5), "{name}");
        }
    }
}

/// A page of connectors with a pattern each, under both caps, saved as `.odg`: the same markup holds for every kind of line a drawing has.
#[test]
fn a_page_of_connectors_is_written_as_dashes_in_both_forms_and_read_back() {
    let mut s = lines_page(2.0);
    let dir = Dir::new();
    let path = dir.path("lines.odg");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let (content, styles) = parts(&path);
    assert_eq!(content.matches("draw:stroke=\"dash\"").count(), 44, "every one of the 44 lines is a dash: {content}");
    let all: Vec<u32> = (2..=23).collect();
    assert_eq!(dash_elements(&styles).len(), 44, "a definition for each pattern in each form: {styles}");
    check_definitions_both(&styles, &all);
    let back = lines(&mut reopened(&path));
    let patterns: Vec<u64> = back.iter().map(|l| l.1).collect();
    let want: Vec<u64> = (2..=23u64).flat_map(|n| [n, n]).collect();
    assert_eq!(patterns, want);
    let caps: Vec<&str> = back.iter().map(|l| l.2.as_str()).collect();
    assert!(caps.chunks(2).all(|c| c == ["round", "flat"]), "{caps:?}");
}

/// The black-box look: the page of lines at three weights, written as `.odg` files for LibreOffice to draw, beside DiaCraft's own pictures of the same drawings. Run by hand with `ODG_DASHES_DIR`
/// set to an empty folder, then `soffice --headless --convert-to png` on each `.odg` and look at the pictures side by side.
#[test]
#[ignore = "a black-box check against LibreOffice, run by hand with ODG_DASHES_DIR set to an empty folder"]
fn lo_draws_the_dashes_diacraft_draws() {
    let folder = std::path::PathBuf::from(std::env::var("ODG_DASHES_DIR").expect("set ODG_DASHES_DIR to an empty folder"));
    std::fs::create_dir_all(&folder).unwrap();
    for weight in [1.0, 2.0, 4.0] {
        let mut s = lines_page(weight);
        let path = folder.join(format!("lines_{weight}pt.odg")).to_str().unwrap().to_owned();
        run(&mut s, "file.save_as", json!({ "path": path }));
        run(&mut s, "file.export", json!({"path": folder.join(format!("diacraft_{weight}pt.png")).to_str().unwrap(), "dpi": 96}));
        println!("wrote {path} and the picture of it: run LibreOffice on the first, then compare");
    }
    let mut s = blank_session();
    rectangles(&mut s, None);
    let path = folder.join("rectangles.odg").to_str().unwrap().to_owned();
    run(&mut s, "file.save_as", json!({ "path": path }));
    run(&mut s, "file.export", json!({"path": folder.join("diacraft_rectangles.png").to_str().unwrap(), "dpi": 96}));
    // The numbers the table has no row for (DiaCraft draws them solid), beside a solid line and a dashed one.
    let mut s = blank_session();
    for (k, formula) in ["24", "254", "2.5", "1", "2"].iter().enumerate() {
        let y = 1.0 + 0.6 * k as f64;
        let id = run(&mut s, "connector.add", json!({"from": {"x": 1.0, "y": y}, "to": {"x": 5.0, "y": y}}))["id"].as_u64().unwrap();
        run(&mut s, "sheet.set_cells", json!({"id": id, "writes": [{"cell": "LinePattern", "formula": formula}]}));
    }
    let path = folder.join("other_numbers.odg").to_str().unwrap().to_owned();
    run(&mut s, "file.save_as", json!({ "path": path }));
    run(&mut s, "file.export", json!({"path": folder.join("diacraft_other_numbers.png").to_str().unwrap(), "dpi": 96}));
}

/// The value a cell of the shape `id` of the first page reads, through its master and its styles (the cached value, which a command's recalculation keeps current).
fn resolved(s: &Session, id: u32, cell: &str) -> f64 {
    use diacraft_doc::{CellAddr, Owner, Resolver, SheetRef};
    let doc = s.document().unwrap();
    let page = doc.pages.iter().find(|p| !p.background).unwrap().id;
    let at = doc.find_shape(Owner::Page(page), id).unwrap_or_else(|| panic!("no shape {id}"));
    let found = Resolver::new(doc).resolve(&SheetRef::Shape(at), &CellAddr::direct(cell)).unwrap_or_else(|| panic!("no cell {cell}"));
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap_or_else(|| panic!("no number in {cell} of shape {id}"))
}

/// The formula of the cell `cell` in the sheet of the shape `id` of the first page itself (not through a master or a style).
fn own_formula(s: &Session, id: u32, cell: &str) -> Option<String> {
    let doc = s.document().unwrap();
    let page = doc.pages.iter().find(|p| !p.background).unwrap();
    let shape = page.shapes.iter().find(|shape| shape.id == id).unwrap();
    shape.sheet.cell(cell).and_then(|c| c.formula.text().map(str::to_owned))
}

#[test]
fn a_formula_the_master_gives_a_new_shape_for_its_pattern_and_its_ends_is_kept_beside_their_values() {
    // An ERD relationship follows its Shape Data: the master holds `IF(Prop.Identifying,1,2)` for the pattern and a formula of `Prop.BeginCardinality` and
    // `Prop.EndCardinality` for each end. The save of a new shape flattened them to the constants they work out to, so after the file was opened again the line no longer followed its
    // data. The dash and the markers show the values; DiaCraft's data carries the formulas.
    let mut s = blank_session();
    run(&mut s, "stencil.open", json!({"id": "crows-foot"}));
    let line = run(&mut s, "shape.drop", json!({"master": "crows-foot.relationship", "page": 1, "x": 3.0, "y": 3.0}))["id"].as_u64().unwrap() as u32;
    run(&mut s, "shapedata.set", json!({"id": line, "row": "Identifying", "value": "0"}));
    let now = (resolved(&s, line, "LinePattern"), resolved(&s, line, "BeginArrow"), resolved(&s, line, "EndArrow"));
    assert_eq!(now, (2.0, 29.0, 25.0), "dashed, zero or many to exactly one");
    let dir = Dir::new();
    let path = dir.path("erd.odg");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let (content, styles) = parts(&path);
    assert_eq!(
        content.matches("draw:stroke-dash=\"DiaCraft_20_dash_20_2\"").count(),
        1,
        "the dash shows the value the formula works out to: {content}"
    );
    check_definitions(&styles, &ROUND, "", &[2]);
    let mut back = reopened(&path);
    let id = shape_ids(&back)[0];
    assert_eq!(own_formula(&back, id, "LinePattern").as_deref(), Some("IF(Prop.Identifying,1,2)"), "the pattern's formula is back");
    let begin = own_formula(&back, id, "BeginArrow");
    assert!(begin.as_deref().is_some_and(|f| f.contains("Prop.BeginCardinality")), "{begin:?}");
    let end = own_formula(&back, id, "EndArrow");
    assert!(end.as_deref().is_some_and(|f| f.contains("Prop.EndCardinality")), "{end:?}");
    assert_eq!(resolved(&back, id, "LinePattern"), 2.0);
    // A save of the drawing as it was read changes nothing: the formulas the file carries are the ones it was read with.
    let before = std::fs::read(&path).unwrap();
    let unedited = run(&mut back, "file.save", json!({}));
    assert_eq!(unedited["edited"], false, "{unedited}");
    assert_eq!(std::fs::read(&path).unwrap(), before, "a save of what was read leaves the file as it was");
    // And the line follows its data again: identifying is solid, and a word at the begin moves its mark.
    run(&mut back, "shapedata.set", json!({"id": id, "row": "Identifying", "value": "1"}));
    assert_eq!(resolved(&back, id, "LinePattern"), 1.0, "solid when identifying");
    run(&mut back, "shapedata.set", json!({"id": id, "row": "BeginCardinality", "value": "One or many"}));
    assert_eq!(resolved(&back, id, "BeginArrow"), 28.0, "the begin's mark follows its word");
}

#[test]
fn a_pattern_formula_that_is_only_a_number_is_no_data_of_ours_and_an_own_one_is_kept_in_any_drawing() {
    // A formula that is a bare number says what a constant says: the masters of the UML and Chen stencils hold `2` and `3` for their dashed lines, and a copy of one of them must
    // not carry a formula for it. Any other formula a person gives a shape's pattern is carried, in a drawing that is no VSDX file too.
    let mut s = blank_session();
    for (k, (stencil, master)) in [("uml-class", "uml-class.realization"), ("chen", "chen.derived-attribute")].iter().enumerate() {
        run(&mut s, "stencil.open", json!({ "id": stencil }));
        run(&mut s, "shape.drop", json!({"master": master, "page": 1, "x": 2.0 + 2.0 * k as f64, "y": 3.0}));
    }
    let extra = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 6.0, "y": 3.0}))["id"].as_u64().unwrap();
    let page = page_id(&s);
    run(&mut s, "sheet.set_cells", json!({"page": page, "writes": [{"id": extra, "cell": "LinePattern", "formula": "IF(Width>1,9,10)"}]}));
    let dir = Dir::new();
    let path = dir.path("numbers.odg");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let back = pattern_cells(&reopened(&path));
    assert_eq!(back.len(), 3);
    assert!(back[0].1.is_none() && back[1].1.is_none(), "a bare number is a constant: {back:?}");
    assert_eq!(back[2], (Some(9.0), Some("IF(Width>1,9,10)".to_owned())), "{back:?}");
}

/// A theme's lines are drawn with a flat cap (the built-in themes' are), so a new shape that follows the `Theme` style has its cap from the style and its copy holds none.
#[test]
fn a_new_shape_that_takes_its_cap_from_the_theme_style_names_the_dash_for_that_cap() {
    let dir = Dir::new();
    let path = dir.write("a.odg", &plain_drawing().bytes());
    let mut s = reopened(&path);
    run(&mut s, "theme.apply", json!({"theme": "fernwood", "scope": "all"}));
    let page = page_id(&s);
    let id = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": page, "x": 4.0, "y": 6.0}))["id"].as_u64().unwrap();
    run(&mut s, "shape.format", json!({"ids": [id], "page": page, "pattern": 5}));
    let seen = lines(&mut s).into_iter().find(|l| l.0 == id).unwrap();
    assert_eq!((seen.1, seen.2.as_str()), (5, "flat"), "the shape draws pattern 5 under the flat cap its style gives it: {seen:?}");
    run(&mut s, "file.save", json!({}));
    let (content, styles) = parts(&path);
    assert!(
        content.contains("draw:stroke-dash=\"DiaCraft_20_dash_20_5_20_flat\"") && !content.contains("draw:stroke-dash=\"DiaCraft_20_dash_20_5\""),
        "the dash is the form a flat cap draws, so a program that draws butt where the file states no cap shows the marks as they are: {content}"
    );
    check_definitions(&styles, &FLAT, "_20_flat", &[5]);
    let back = lines(&mut reopened(&path));
    let again = back.iter().find(|l| l.1 == 5).unwrap_or_else(|| panic!("{back:?}"));
    assert_eq!(again.2, "flat", "and it comes back as it was: {back:?}");
}

#[test]
fn a_new_shape_that_takes_a_round_or_a_square_cap_from_the_theme_style_states_it_and_names_the_table_s_own_form() {
    // The cap the style gives is stated as it is (a file that states none is drawn butt by another program), and the dash is the form of that cap: the table's own for a round or a
    // projecting cap, which a flat one alone changes.
    for (cap, word) in [(0, "round"), (2, "square")] {
        let dir = Dir::new();
        let path = dir.write("a.odg", &plain_drawing().bytes());
        let mut s = reopened(&path);
        run(&mut s, "theme.apply", json!({"theme": "fernwood", "scope": "all"}));
        let style = diacraft_engine::theme::theme_style_id(s.document().unwrap()).unwrap();
        run(&mut s, "sheet.set_cells", json!({"style": style, "writes": [{"cell": "LineCap", "formula": cap.to_string()}]}));
        let page = page_id(&s);
        let id = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": page, "x": 4.0, "y": 6.0}))["id"].as_u64().unwrap();
        run(&mut s, "shape.format", json!({"ids": [id], "page": page, "pattern": 5}));
        let seen = lines(&mut s).into_iter().find(|l| l.0 == id).unwrap();
        assert_eq!((seen.1, seen.2.as_str()), (5, word), "the style gives the cap {word}: {seen:?}");
        run(&mut s, "file.save", json!({}));
        let (content, styles) = parts(&path);
        assert!(content.contains(&format!("svg:stroke-linecap=\"{word}\"")), "{word}: the cap is stated: {content}");
        assert!(
            content.contains("draw:stroke-dash=\"DiaCraft_20_dash_20_5\"") && !content.contains("_20_flat"),
            "{word}: the table's own form: {content}"
        );
        check_definitions(&styles, &ROUND, "", &[5]);
    }
}

#[test]
fn a_fraction_in_a_pattern_cell_reads_as_the_solid_line_and_the_foreground_fill_that_are_drawn() {
    // A fraction is drawn solid (the line solid, the fill the foreground), so the controls read it so. A fraction was rounded to the number beside it: 2.5 showed as the
    // pattern 3 of dots, 1.6 as 2, and 0.4 as no line at all (so no line transparency was shown for a line that is drawn); a fill of 2.5 showed as the hatch 3 and one of 0.4 as no fill.
    let mut s = blank_session();
    let id = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 3.0}))["id"].as_u64().unwrap();
    let page = page_id(&s);
    run(&mut s, "sheet.set_cells", json!({"page": page, "writes": [{"id": id, "cell": "LineColorTrans", "formula": "0.5"}]}));
    let look = |s: &mut Session| run(s, "shape.formatting", json!({"ids": [id], "page": page}))["shapes"][0].clone();
    for (text, pattern, transparency) in
        [("2.5", 1, 50.0), ("0.4", 1, 50.0), ("1.6", 1, 50.0), ("22.5", 1, 50.0), ("5", 5, 50.0), ("1", 1, 50.0), ("0", 0, 0.0)]
    {
        run(&mut s, "sheet.set_cells", json!({"page": page, "writes": [{"id": id, "cell": "LinePattern", "formula": text}]}));
        let shown = look(&mut s);
        assert_eq!(shown["pattern"], pattern, "LinePattern {text}: {shown}");
        assert_eq!(shown["line_transparency"], transparency, "LinePattern {text}: the transparency of a line that is drawn: {shown}");
    }
    for (text, shown_as) in [("2.5", 1), ("0.4", 1), ("24.5", 1), ("6", 6), ("1", 1), ("0", 0)] {
        run(&mut s, "sheet.set_cells", json!({"page": page, "writes": [{"id": id, "cell": "FillPattern", "formula": text}]}));
        let shown = look(&mut s);
        assert_eq!(shown["fill_pattern"], shown_as, "FillPattern {text}: {shown}");
    }
}

/// The black-box look at an inherited cap: a drawing of one shape that takes its cap from the `Theme` style and holds a dash of its own, saved as `.odg` for LibreOffice to draw beside
/// DiaCraft's own picture. Run by hand with `ODGDASH_FOLLOW_DIR` set to an empty folder, then `soffice --headless --convert-to png` on the `.odg` files.
#[test]
#[ignore = "a black-box check against LibreOffice, run by hand with ODGDASH_FOLLOW_DIR set to an empty folder"]
fn lo_draws_the_dash_of_a_shape_that_takes_its_cap_from_the_theme_style() {
    let folder = std::path::PathBuf::from(std::env::var("ODGDASH_FOLLOW_DIR").expect("set ODGDASH_FOLLOW_DIR to an empty folder"));
    std::fs::create_dir_all(&folder).unwrap();
    for (name, weight) in [("thin", 1.0), ("middle", 3.0), ("heavy", 6.0)] {
        let path = folder.join(format!("theme_cap_{name}.odg")).to_str().unwrap().to_owned();
        std::fs::write(&path, plain_drawing().bytes()).unwrap();
        let mut s = reopened(&path);
        run(&mut s, "theme.apply", json!({"theme": "fernwood", "scope": "all"}));
        let page = page_id(&s);
        // A dash of each kind with a dot in it (3, 5 and 9), on lines, so a mark a weight short shows against the picture of DiaCraft's own.
        for (k, pattern) in [3u32, 5, 9, 2].iter().enumerate() {
            let y = 5.0 + 0.8 * k as f64;
            let id =
                run(&mut s, "connector.add", json!({"from": {"x": 1.0, "y": y}, "to": {"x": 6.0, "y": y}, "page": page}))["id"].as_u64().unwrap();
            run(&mut s, "shape.format", json!({"ids": [id], "page": page, "pattern": pattern, "weight": weight}));
        }
        run(&mut s, "file.save", json!({}));
        run(&mut s, "file.export", json!({"path": folder.join(format!("diacraft_theme_cap_{name}.png")).to_str().unwrap(), "dpi": 96}));
        println!("wrote {path} and the picture of it");
    }
}
