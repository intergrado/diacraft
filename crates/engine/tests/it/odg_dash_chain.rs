//! A regression of the change, found by the 0.2.4 gate: a plain trip of an `.odg` through `.diacraft` (open, Save As `.diacraft`, reopen, Save As `.odg`: the command
//! line's `roundtrip --via diacraft`) keeps **the dash each line is drawn with**.
//!
//! A style that says `draw:stroke="dash"` and names no `draw:stroke-dash` takes its dash from the parent that names one (ODF 1.4 Part 3, 16.3: a style's properties are its own and its parent's,
//! the child's first), and a style that has none in its whole chain is drawn by the program that reads it in its own way. The trip used to write DiaCraft's own dash 2 (700% and 500% of the weight) for
//! both: the stand-in of the cells' `LinePattern` 2, which the model writer wrote beside the held `draw:stroke="dash"`. The owner's ruling on the second (2026-11-27, "Keep the file's form"): a
//! chain that names no dash names none after the trip either, and the other program draws what it drew for the source; DiaCraft draws its own default dash for it. **A dash the person sets in
//! DiaCraft is still written** (`odg_dashes.rs`).
//!
//! The vectors are made by code from the ODF vocabulary. The oracle is a resolver of its own: it reads the markup that comes out, walks each shape's style chain as ODF says and gives the stroke
//! and the dash (name and definition) the shape is drawn with, to be compared with the source's.

use diacraft_container::xml::{El, parse};
use diacraft_engine::Session;
use proptest::prelude::*;
use proptest::test_runner::{Config, RngAlgorithm, TestCaseError, TestRng, TestRunner};
use serde_json::json;

use crate::native_common::{Dir, opened, pinned};
use crate::odg_build::{Drawing, part_of, rect_on};
use crate::run;

/// What one level of a style chain says: the properties of a `style:graphic-properties`. The first level is the automatic style the shapes name, and each next one is the parent of the one before.
#[derive(Clone, Debug, Default, PartialEq)]
struct Level {
    stroke: Option<&'static str>,
    dash: Option<&'static str>,
    /// Whether the level says the weight (`svg:stroke-width`), the colour and the cap the drawing is made with: the properties a line has apart from its dash.
    look: bool,
}

/// The dashes a drawing defines in its common styles: another make's, whose runs are none of the table's (`Pebble`, a mark of 0.10 cm and a gap of 0.08 cm: 2 and 1.6 weights of the 0.05 cm
/// line), another make's whose runs are the table's pattern 9 (`Fine_20_Dots`: 0.15 cm and 0.15 cm, 3 and 3 weights), and one of DiaCraft's own (pattern 4).
const DEFINITIONS: &str = concat!(
    r#"<draw:stroke-dash draw:name="Pebble" draw:display-name="Pebble" draw:style="rect" draw:dots1="1" draw:dots1-length="0.10cm" draw:distance="0.08cm"/>"#,
    r#"<draw:stroke-dash draw:name="Fine_20_Dots" draw:display-name="Fine Dots" draw:style="rect" draw:dots1="1" draw:dots1-length="0.15cm" draw:distance="0.15cm"/>"#,
    r#"<draw:stroke-dash draw:name="DiaCraft_20_dash_20_4" draw:display-name="DiaCraft dash 4" draw:style="rect" draw:dots1="1" draw:dots1-length="700%" draw:dots2="1" draw:dots2-length="0%" draw:distance="500%"/>"#,
    r#"<draw:stroke-dash draw:name="DiaCraft_20_dash_20_4_20_flat" draw:display-name="DiaCraft dash 4 flat" draw:style="rect" draw:dots1="1" draw:dots1-length="800%" draw:dots2="1" draw:dots2-length="100%" draw:distance="400%"/>"#
);

/// The name of the style at depth `k` of a chain: `gr1` for the shapes' own, the common styles `Kerb`, `Kerb2`, ...
fn style_name(k: usize) -> String {
    match k {
        0 => "gr1".to_owned(),
        1 => "Kerb".to_owned(),
        n => format!("Kerb{n}"),
    }
}

/// The `style:style` element of the level at depth `k` of `chain`, the lines drawn with the cap `cap` (`round` or `butt`): its parent is the next level, when there is one.
fn style_of(chain: &[Level], k: usize, cap: &str) -> String {
    let level = &chain[k];
    let parent = if k + 1 < chain.len() { format!(r#" style:parent-style-name="{}""#, style_name(k + 1)) } else { String::new() };
    let mut props = String::new();
    if let Some(stroke) = level.stroke {
        props.push_str(&format!(r#" draw:stroke="{stroke}""#));
    }
    if let Some(dash) = level.dash {
        props.push_str(&format!(r#" draw:stroke-dash="{dash}""#));
    }
    if level.look {
        props.push_str(&format!(r##" svg:stroke-width="0.05cm" svg:stroke-color="#000000" svg:stroke-linecap="{cap}""##));
    }
    format!(r#"<style:style style:name="{}" style:family="graphic"{parent}><style:graphic-properties{props}/></style:style>"#, style_name(k))
}

/// How many shapes the drawings have: a line, a custom shape and a plain rectangle, all three on the style `gr1`.
const SHAPES: usize = 3;

/// A line, a custom shape and a plain rectangle, all on the style `gr1`.
fn shapes() -> String {
    format!(
        concat!(
            r#"<draw:line draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" svg:x1="2cm" svg:y1="3cm" svg:x2="8cm" svg:y2="3cm"><text:p/></draw:line>{}"#,
            r#"<draw:rect draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" svg:x="8cm" svg:y="6cm" svg:width="4cm" svg:height="2cm"><text:p/></draw:rect>"#
        ),
        rect_on("gr1", "R", (2.0, 6.0, 4.0, 2.0), "")
    )
}

/// The drawing of `chain`: the first level is the automatic style `gr1`, the others are common styles, and the dashes are defined in the common styles.
fn drawing(chain: &[Level], cap: &str) -> Drawing {
    let mut common = String::from(DEFINITIONS);
    for k in 1..chain.len() {
        common.push_str(&style_of(chain, k, cap));
    }
    let mut d = Drawing::new().common(&common).page(&shapes());
    d.auto = format!("{}{}{}", style_of(chain, 0, cap), crate::odg_build::GR2, crate::odg_build::P1);
    d
}

/// Every `style:style` of graphic family in the parts, by name (the automatic styles of both parts and the common ones), as the element.
fn graphic_styles(parts: &[&El]) -> Vec<(String, El)> {
    fn walk(el: &El, out: &mut Vec<(String, El)>) {
        for child in el.elements() {
            if child.is("style") && child.attr_local("family") == Some("graphic") {
                if let Some(name) = child.attr_local("name") {
                    out.push((name.to_owned(), child.clone()));
                }
            } else {
                walk(child, out);
            }
        }
    }
    let mut out = Vec::new();
    for part in parts {
        walk(part, &mut out);
    }
    out
}

/// What a shape is drawn with, as the markup says it: the stroke, and when it is a dash the dash it names (its name and its definition) or none.
#[derive(Clone, Debug, PartialEq)]
struct Drawn {
    stroke: Option<String>,
    /// The name of the dash and the attributes of its definition, other than the name's, in the order written; `None` for a line that is not a dash and for a chain that names none.
    dash: Option<(String, Vec<(String, String)>)>,
}

/// The first value a style of the chain from `name` says for the graphic property `local`: the style's own, else its parent's, as ODF says.
fn inherited(styles: &[(String, El)], name: &str, local: &str) -> Option<String> {
    let mut at = name.to_owned();
    for _ in 0..64 {
        let (_, style) = styles.iter().find(|(n, _)| *n == at)?;
        if let Some(value) = style.child("graphic-properties").and_then(|p| p.attr_local(local)) {
            return Some(value.to_owned());
        }
        at = style.attr_local("parent-style-name")?.to_owned();
    }
    None
}

/// What each line and rectangle of the first page of the `.odg` in `bytes` is drawn with, in the order written.
fn drawn(bytes: &[u8]) -> Vec<Drawn> {
    let content = parse(part_of(bytes, "content.xml").as_bytes()).unwrap();
    let styles_part = parse(part_of(bytes, "styles.xml").as_bytes()).unwrap();
    let styles = graphic_styles(&[&content, &styles_part]);
    let definitions: Vec<El> = {
        let mut out = Vec::new();
        fn collect(el: &El, out: &mut Vec<El>) {
            for child in el.elements() {
                if child.is("stroke-dash") {
                    out.push(child.clone());
                } else {
                    collect(child, out);
                }
            }
        }
        collect(&styles_part, &mut out);
        out
    };
    let mut shapes = Vec::new();
    fn find<'a>(el: &'a El, out: &mut Vec<&'a El>) {
        for child in el.elements() {
            if (child.is("line") || child.is("custom-shape") || child.is("rect")) && child.attr_local("style-name").is_some() {
                out.push(child);
            } else {
                find(child, out);
            }
        }
    }
    find(&content, &mut shapes);
    shapes
        .iter()
        .map(|shape| {
            let style = shape.attr_local("style-name").unwrap();
            let stroke = inherited(&styles, style, "stroke");
            let dash = (stroke.as_deref() == Some("dash")).then(|| inherited(&styles, style, "stroke-dash")).flatten().map(|name| {
                let attrs = definitions
                    .iter()
                    .find(|d| d.attr_local("name") == Some(name.as_str()))
                    .map(|d| d.attrs.iter().filter(|(k, _)| !k.starts_with("xmlns") && !k.ends_with(":name")).cloned().collect())
                    .unwrap_or_default();
                (name, attrs)
            });
            Drawn { stroke, dash }
        })
        .collect()
}

/// The plain visit: `source` saved as `a.diacraft`, which a session of its own opens and saves as `b.odg`. Gives `b.odg`.
fn visit(dir: &Dir, source: &[u8]) -> Vec<u8> {
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("v.odg", source) }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let mut t = opened(&dir.path("a.diacraft"));
    run(&mut t, "file.save_as", json!({ "path": dir.path("b.odg") }));
    dir.read("b.odg")
}

/// A level that says the dash `dash` under `stroke`, and the look of the line.
fn level(stroke: Option<&'static str>, dash: Option<&'static str>, look: bool) -> Level {
    Level { stroke, dash, look }
}

/// Checks that the trip of `chain` draws what the source draws, every shape of it (a round cap).
fn keeps(what: &str, chain: &[Level]) -> Vec<String> {
    keeps_with(what, chain, "round")
}

/// [`keeps`] under the cap `cap`.
fn keeps_with(what: &str, chain: &[Level], cap: &str) -> Vec<String> {
    let dir = Dir::new();
    let source = drawing(chain, cap).bytes();
    let before = drawn(&source);
    let after = drawn(&visit(&dir, &source));
    let mut found = Vec::new();
    if before.len() != SHAPES || after.len() != SHAPES {
        found.push(format!("{what}: {} shapes drawn before and {} after", before.len(), after.len()));
    }
    for (k, (b, a)) in before.iter().zip(after.iter()).enumerate() {
        if a != b {
            found.push(format!("{what}: shape {k} is drawn with {b:?} before the trip and {a:?} after it"));
        }
    }
    found
}

/// DASH-IS: the line's style says `dash` and names none, and its parent names `Pebble` and says `solid` (the vector as the gate gave it).
#[test]
fn is_a_style_that_says_dash_takes_the_dash_of_a_parent_that_says_solid_and_keeps_it_through_the_trip() {
    let chain = [level(Some("dash"), None, true), level(Some("solid"), Some("Pebble"), false)];
    let found = keeps("IS", &chain);
    assert!(found.is_empty(), "{found:#?}");
}

/// DASH-IN: the same, and the parent says no stroke.
#[test]
fn in_a_style_that_says_dash_takes_the_dash_of_a_parent_that_says_no_stroke_and_keeps_it_through_the_trip() {
    let chain = [level(Some("dash"), None, true), level(None, Some("Pebble"), false)];
    let found = keeps("IN", &chain);
    assert!(found.is_empty(), "{found:#?}");
}

/// ISR: the same as IS, with the weight, the colour and the cap on the parent and none on the line's own style (the line's style states the dash alone).
#[test]
fn isr_the_look_of_the_line_is_the_parents_and_the_dash_is_the_parents_too() {
    let chain = [level(Some("dash"), None, false), level(Some("solid"), Some("Pebble"), true)];
    let found = keeps("ISR", &chain);
    assert!(found.is_empty(), "{found:#?}");
}

/// DASH-U: nothing in the chain names a dash (the parent says only `dash`): no dash is named after the trip either (the owner's ruling "Keep the file's form"), so the other program draws what it
/// drew for the source.
#[test]
fn u_a_chain_that_names_no_dash_names_none_after_the_trip() {
    let chain = [level(Some("dash"), None, true), level(Some("dash"), None, false)];
    let found = keeps("U", &chain);
    assert!(found.is_empty(), "{found:#?}");
    let dir = Dir::new();
    let back = visit(&dir, &drawing(&chain, "round").bytes());
    let content = part_of(&back, "content.xml");
    let styles = part_of(&back, "styles.xml");
    assert!(!content.contains("draw:stroke-dash=") && !styles.contains("draw:stroke-dash="), "no style names a dash: {content}\n{styles}");
    assert!(!styles.contains("DiaCraft_20_dash_20_2"), "and DiaCraft's own dash 2 is not defined for it: {styles}");
}

/// A line that says `dash` and names no dash itself, with no parent at all.
#[test]
fn u_a_single_style_that_says_dash_alone_names_no_dash_after_the_trip() {
    let found = keeps("U, one level", &[level(Some("dash"), None, true)]);
    assert!(found.is_empty(), "{found:#?}");
}

/// Control I: the parent says `dash` and names `Pebble`, the line's style says `dash` (or nothing) and names none.
#[test]
fn control_i_a_parent_that_says_dash_and_names_the_dash_is_kept() {
    for own in [Some("dash"), None] {
        let found = keeps(&format!("I, own stroke {own:?}"), &[level(own, None, true), level(Some("dash"), Some("Pebble"), false)]);
        assert!(found.is_empty(), "{found:#?}");
    }
}

/// Control IR: the same, with the look on the parent.
#[test]
fn control_ir_a_parent_that_says_dash_and_names_the_dash_with_the_look_of_the_line_is_kept() {
    for own in [Some("dash"), None] {
        let found = keeps(&format!("IR, own stroke {own:?}"), &[level(own, None, false), level(Some("dash"), Some("Pebble"), true)]);
        assert!(found.is_empty(), "{found:#?}");
    }
}

/// Control O: the line's own style names `Pebble`.
#[test]
fn control_o_a_style_that_names_the_dash_itself_is_kept() {
    for parent in [None, Some(level(Some("solid"), None, false)), Some(level(Some("dash"), Some("Fine_20_Dots"), false))] {
        let mut chain = vec![level(Some("dash"), Some("Pebble"), true)];
        chain.extend(parent.clone());
        let found = keeps(&format!("O, parent {parent:?}"), &chain);
        assert!(found.is_empty(), "{found:#?}");
    }
}

/// The dashes a person wrote for the table and the other makes' that match it, named in the line's own style or inherited, are kept as they are named.
#[test]
fn a_dash_of_the_table_or_one_that_matches_it_is_kept_whether_the_style_names_it_or_inherits_it() {
    for dash in ["Fine_20_Dots", "DiaCraft_20_dash_20_4", "Pebble"] {
        let named = keeps(&format!("{dash} named"), &[level(Some("dash"), Some(dash), true)]);
        let inherited = keeps(&format!("{dash} inherited"), &[level(Some("dash"), None, true), level(Some("dash"), Some(dash), false)]);
        let deep = keeps(
            &format!("{dash} two levels up"),
            &[level(Some("dash"), None, true), level(None, None, false), level(Some("dash"), Some(dash), false)],
        );
        let all: Vec<String> = named.into_iter().chain(inherited).chain(deep).collect();
        assert!(all.is_empty(), "{all:#?}");
    }
}

/// A dash of DiaCraft's own that the file named in the form of its cap (a flat cap is the form) comes back as it was named: the cells say it, from the pattern and the cap.
#[test]
fn a_dash_of_diacraft_named_for_a_flat_cap_is_kept_with_the_cap() {
    for chain in [
        vec![level(Some("dash"), Some("DiaCraft_20_dash_20_4_20_flat"), true)],
        vec![level(Some("dash"), None, true), level(Some("dash"), Some("DiaCraft_20_dash_20_4_20_flat"), false)],
        vec![level(Some("dash"), None, false), level(Some("dash"), Some("DiaCraft_20_dash_20_4_20_flat"), true)],
    ] {
        let found = keeps_with(&format!("{chain:?}"), &chain, "butt");
        assert!(found.is_empty(), "{found:#?}");
    }
}

/// The ids of the shapes of the first page that is no background, and that page's id.
fn page_and_shapes(s: &Session) -> (u32, Vec<u32>) {
    let page = s.document().unwrap().pages.iter().find(|p| !p.background).unwrap();
    (page.id, page.shapes.iter().map(|shape| shape.id).collect())
}

/// The effective dash of each shape before, and after `edit` has been made to the session that opened the source `chain` as an `.odg` and the file saved in place: the splice writer's.
fn spliced(chain: &[Level], edit: impl FnOnce(&mut Session, u32, &[u32])) -> (Vec<Drawn>, Vec<Drawn>) {
    let dir = Dir::new();
    let source = drawing(chain, "round").bytes();
    let path = dir.write("s.odg", &source);
    let mut s = opened(&path);
    let (page, ids) = page_and_shapes(&s);
    edit(&mut s, page, &ids);
    run(&mut s, "file.save", json!({}));
    (drawn(&source), drawn(&dir.read("s.odg")))
}

/// The vectors the edits below are made to: the chains of the gate's finding (IS, IN, ISR), the one that names no dash (U), and the controls (I, IR, O).
fn vectors() -> Vec<(&'static str, Vec<Level>)> {
    vec![
        ("IS", vec![level(Some("dash"), None, true), level(Some("solid"), Some("Pebble"), false)]),
        ("IN", vec![level(Some("dash"), None, true), level(None, Some("Pebble"), false)]),
        ("U", vec![level(Some("dash"), None, true), level(Some("dash"), None, false)]),
        ("I", vec![level(Some("dash"), None, true), level(Some("dash"), Some("Pebble"), false)]),
        ("ISR", vec![level(Some("dash"), None, false), level(Some("solid"), Some("Pebble"), true)]),
        ("IR", vec![level(Some("dash"), None, false), level(Some("dash"), Some("Pebble"), true)]),
        ("O", vec![level(Some("dash"), Some("Pebble"), true)]),
    ]
}

/// An edit made to a session: the page and the ids of the shapes are given.
type Edit = fn(&mut Session, u32, &[u32]);

/// **The splice writer's save keeps the dash of a line whose style is shared** when an edit of something else (the pose, the fill, the colour, the weight or the cap of the line) restyles the shape:
/// the edit says what it changes, and the dash is not among it.
#[test]
fn an_edit_of_something_else_spliced_into_the_file_keeps_each_shapes_dash() {
    let edits: [(&str, Edit); 5] = [
        ("move", |s, page, ids| {
            run(s, "shape.move", json!({"ids": [ids[0]], "page": page, "dx": 0.5, "dy": 0.25}));
        }),
        ("fill", |s, page, ids| {
            run(s, "shape.format", json!({"ids": [ids[0]], "page": page, "fill": "#336699"}));
        }),
        ("line colour", |s, page, ids| {
            run(s, "shape.format", json!({"ids": [ids[0]], "page": page, "line": "#336699"}));
        }),
        ("weight", |s, page, ids| {
            run(s, "shape.format", json!({"ids": [ids[0]], "page": page, "weight": 0.04}));
        }),
        ("cap", |s, page, ids| {
            run(s, "shape.format", json!({"ids": [ids[0]], "page": page, "line_cap": "flat"}));
        }),
    ];
    let mut found = Vec::new();
    for (name, chain) in vectors() {
        for (what, edit) in &edits {
            let (before, after) = spliced(&chain, *edit);
            if before.len() != after.len() {
                found.push(format!("{name}, {what}: {} shapes before and {} after", before.len(), after.len()));
            }
            for (k, (b, a)) in before.iter().zip(after.iter()).enumerate() {
                if a != b {
                    found.push(format!("{name}, {what}: shape {k} is drawn with {b:?} before the edit and {a:?} after it"));
                }
            }
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}

/// The dash DiaCraft's own makes for the pattern 5 under a round cap: its name, and the attributes of its definition (the table's row: a mark of 7 weights, two dots and a gap of 5).
fn own_dash_5() -> (String, Vec<(String, String)>) {
    let attrs = [
        ("draw:display-name", "DiaCraft dash 5"),
        ("draw:style", "rect"),
        ("draw:dots1", "1"),
        ("draw:dots1-length", "700%"),
        ("draw:dots2", "2"),
        ("draw:dots2-length", "0%"),
        ("draw:distance", "500%"),
    ];
    ("DiaCraft_20_dash_20_5".to_owned(), attrs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect())
}

/// **A dash the person sets is written as writes it**, over the dash the line took from its parent or named in its own style: the first shape takes the pattern 5, and the others, which
/// share the style, are as they were. Through a `.diacraft` file and its model writer, and spliced into the file.
#[test]
fn a_pattern_the_person_sets_is_written_as_a_dash_of_diacraft_through_both_writers() {
    for (name, chain) in vectors() {
        let source = drawing(&chain, "round").bytes();
        let before = drawn(&source);
        let want = Drawn { stroke: Some("dash".to_owned()), dash: Some(own_dash_5()) };
        // Spliced into the file it was read from.
        let (_, after) = spliced(&chain, |s, page, ids| {
            run(s, "shape.format", json!({"ids": [ids[0]], "page": page, "pattern": 5}));
        });
        assert_eq!(after.first(), Some(&want), "{name}, spliced: the first shape is the dash the person set");
        assert_eq!(after.get(1..), before.get(1..), "{name}, spliced: the other shapes keep their dash");
        // Through the model writer: the file saved as `.diacraft`, opened in a session of its own, edited and saved as an `.odg`.
        let dir = Dir::new();
        let mut first = pinned();
        run(&mut first, "file.open", json!({ "path": dir.write("v.odg", &source) }));
        run(&mut first, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
        let mut second = opened(&dir.path("a.diacraft"));
        let (page, ids) = page_and_shapes(&second);
        run(&mut second, "shape.format", json!({"ids": [ids[0]], "page": page, "pattern": 5}));
        run(&mut second, "file.save_as", json!({ "path": dir.path("b.odg") }));
        let after = drawn(&dir.read("b.odg"));
        assert_eq!(after.first(), Some(&want), "{name}, model: the first shape is the dash the person set");
        assert_eq!(after.get(1..), before.get(1..), "{name}, model: the other shapes keep their dash");
    }
}

/// A line whose style says `dash` and names none, saved as it was, **draws what it drew**: another program reading the saved file finds no dash named in any style of the chain, and a second trip
/// of the result names none either. (The file's form, the owner's ruling: DiaCraft draws its own default dash for it, and does not write it.)
#[test]
fn a_dash_that_no_style_names_is_not_named_by_a_second_trip_either() {
    let chain = [level(Some("dash"), None, true), level(Some("dash"), None, false)];
    let once = visit(&Dir::new(), &drawing(&chain, "round").bytes());
    let twice = visit(&Dir::new(), &once);
    for (when, bytes) in [("once", &once), ("twice", &twice)] {
        let (content, styles) = (part_of(bytes, "content.xml"), part_of(bytes, "styles.xml"));
        assert!(!content.contains("stroke-dash=") && !styles.contains("stroke-dash=\""), "{when}: no style names a dash: {content}\n{styles}");
        assert!(!styles.contains("DiaCraft_20_dash_20_2"), "{when}: and DiaCraft's own dash 2 is not defined for it: {styles}");
    }
}

/// The levels of a chain: a stroke, a dash and the look, each at random.
fn levels() -> impl Strategy<Value = Level> {
    (
        prop_oneof![Just(None), Just(Some("solid")), Just(Some("none")), Just(Some("dash")), Just(Some("dash"))],
        prop_oneof![Just(None), Just(None), Just(Some("Pebble")), Just(Some("Fine_20_Dots")), Just(Some("DiaCraft_20_dash_20_4"))],
        any::<bool>(),
    )
        .prop_map(|(stroke, dash, look)| Level { stroke, dash, look })
}

/// **Random chains of one to three styles, with and without dash names, keep the dash each is drawn with through an unedited trip**, under a round cap and under a flat one (a dash of DiaCraft's own
/// is then named for the form of the cap). The look (the weight, the colour and the cap) is where the first level that says it is, and a chain where no level says it has it on the last one.
#[test]
fn a_random_chain_keeps_its_effective_dash_through_an_unedited_trip() {
    // A longer run, and another seed, for a hunt by hand: `DIACRAFT_DASH_CASES` and `DIACRAFT_DASH_SEED` (the default is the fixed run of the gate, 120 cases of seed 7).
    let from_env = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<u32>().ok());
    let cases = from_env("DIACRAFT_DASH_CASES").unwrap_or(120);
    let seed = u8::try_from(from_env("DIACRAFT_DASH_SEED").unwrap_or(7)).unwrap_or(7);
    let config = Config { cases, failure_persistence: None, max_shrink_iters: 200, ..Config::default() };
    let mut runner = TestRunner::new_with_rng(config, TestRng::from_seed(RngAlgorithm::ChaCha, &[seed; 32]));
    let strategy = (proptest::collection::vec(levels(), 1..4), any::<bool>());
    let result = runner.run(&strategy, |(mut chain, butt)| {
        if !chain.iter().any(|l| l.look)
            && let Some(last) = chain.last_mut()
        {
            last.look = true;
        }
        // A dash of DiaCraft's own is named for the form its cap draws, as DiaCraft writes it.
        if butt {
            for l in &mut chain {
                if l.dash == Some("DiaCraft_20_dash_20_4") {
                    l.dash = Some("DiaCraft_20_dash_20_4_20_flat");
                }
            }
        }
        let cap = if butt { "butt" } else { "round" };
        let found = keeps_with("random", &chain, cap);
        if found.is_empty() { Ok(()) } else { Err(TestCaseError::fail(format!("{chain:?}, {cap}: {found:#?}"))) }
    });
    if let Err(e) = result {
        panic!("{e}");
    }
}

/// Writes each vector's source and what it becomes (the trip through `.diacraft`, and a save into the file after an edit of the fill) into the folder `DIACRAFT_DASH_VECTORS`, for a check of what
/// another program draws of them (a black box: the dash that program draws must be the one it drew for the source). Does nothing without the variable.
#[test]
#[ignore = "writes files for a check by another program: set DIACRAFT_DASH_VECTORS to a folder"]
fn write_the_vectors_for_a_check_by_another_program() {
    let Ok(folder) = std::env::var("DIACRAFT_DASH_VECTORS") else { return };
    for (name, chain) in vectors() {
        let source = drawing(&chain, "round").bytes();
        std::fs::write(format!("{folder}/{name}_0_source.odg"), &source).unwrap();
        std::fs::write(format!("{folder}/{name}_1_trip.odg"), visit(&Dir::new(), &source)).unwrap();
        let dir = Dir::new();
        let mut s = opened(&dir.write("s.odg", &source));
        let (page, ids) = page_and_shapes(&s);
        run(&mut s, "shape.format", json!({"ids": [ids[0]], "page": page, "fill": "#336699"}));
        run(&mut s, "file.save", json!({}));
        std::fs::write(format!("{folder}/{name}_2_spliced.odg"), dir.read("s.odg")).unwrap();
    }
}
