//! UML Class (tests first): the compartments that follow their text, the resize that changes the width only, typing that edits the name, the abstract italic, a package that holds
//! its members, the relationships' ends and patterns of the design's section 6.1, the search words, the Quick Shapes and the
//! templates. The figures are looked at by what they draw: ink at places worked out from the figure's own numbers, and never an absolute text height (the faces on the machine decide a line's).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use diacraft_calc::{Budget, CellAddress, EvalCtx, Value as Calc};
use diacraft_doc::{CellAddr, Document, FormulaSlot, Owner, Resolver, RowKey, SectionKey, SheetRef, TextPart};
use serde_json::{Value, json};

use super::super::flowchart_tests::{Dir, blank, id_of, px, render, run};
use super::super::{builtin_stencil, find_master, registered, stencils};
use crate::Session;
use crate::draw;

const STENCIL: &str = "uml-class";

/// The ids of the masters of the stencil, in the order the Shapes window lists them.
const MASTER_IDS: [&str; 15] = [
    "uml-class.class",
    "uml-class.interface",
    "uml-class.enumeration",
    "uml-class.package",
    "uml-class.data-type",
    "uml-class.object",
    "uml-class.note",
    "uml-class.association",
    "uml-class.directed-association",
    "uml-class.aggregation",
    "uml-class.composition",
    "uml-class.generalization",
    "uml-class.realization",
    "uml-class.dependency",
    "uml-class.note-link",
];

// ---- helpers --------------------------------------------------------------------------------------------------------------------------

pub(crate) fn page_of(s: &Session) -> u32 {
    s.document().unwrap().pages.get(0).unwrap().id
}

/// Drops the master `master` of `stencil` at (`x`, `y`) by `shape.drop`: the new shape's `ID`.
pub(crate) fn drop_in(s: &mut Session, stencil: &str, master: &str, x: f64, y: f64) -> u32 {
    id_of(&run(s, "shape.drop", json!({ "master": master, "stencil": stencil, "page": page_of(s), "x": x, "y": y })))
}

/// [`drop_in`] for this stencil.
fn drop_at(s: &mut Session, master: &str, x: f64, y: f64) -> u32 {
    drop_in(s, STENCIL, master, x, y)
}

pub(crate) fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// What a cell of a shape works out to now: the formula is evaluated live, whatever value the last recalculation cached.
pub(crate) fn live(doc: &Document, id: u32, cell: &str) -> f64 {
    let at = doc.find_shape(Owner::Page(1), id).unwrap_or_else(|| panic!("no shape {id}"));
    let mut ctx = EvalCtx::new(crate::measure::eval_options(), Budget::default());
    match diacraft_calc::evaluate(doc, &CellAddress::new(SheetRef::Shape(at), CellAddr::direct(cell)), &mut ctx) {
        Calc::Number(q) => q.stored,
        other => panic!("{cell} of shape {id}: {other:?}"),
    }
}

/// What a cell of a shape reads through its master as the last recalculation left it, or `None` for no cell.
pub(crate) fn cached(doc: &Document, id: u32, cell: &str) -> Option<f64> {
    let at = doc.find_shape(Owner::Page(1), id)?;
    let found = Resolver::new(doc).resolve(&SheetRef::Shape(at), &CellAddr::direct(cell))?;
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number())
}

/// The formula text of a cell of a shape, through its master.
pub(crate) fn formula_of(doc: &Document, id: u32, cell: &str) -> Option<String> {
    let at = doc.find_shape(Owner::Page(1), id)?;
    let found = Resolver::new(doc).resolve(&SheetRef::Shape(at), &CellAddr::direct(cell))?;
    match &found.formula.cell?.formula {
        FormulaSlot::Local(f) => Some(f.source.to_string()),
        _ => None,
    }
}

/// A cell of a row of a section of a shape's own sections (first row), through its master: `Character` row 0's `Style`, `Paragraph` row 0's `HorzAlign`.
pub(crate) fn row_cell(doc: &Document, id: u32, section: &str, ix: u32, cell: &str) -> Option<f64> {
    let at = doc.find_shape(Owner::Page(1), id)?;
    let sheet = SheetRef::Shape(at);
    let res = Resolver::new(doc);
    let rows = res.effective_rows(&sheet, &SectionKey::new(section, None))?;
    let row = rows.iter().find(|r| r.key == RowKey::Ix(ix))?;
    row.cells.iter().find(|(n, _)| n.as_str() == cell).and_then(|(_, r)| r.value.cell?.value.as_ref()?.number())
}

/// The characters a shape shows: its own text, and when it has none its master shape's (a member of a group master and a line hold their master's placeholder until it is typed over).
pub(crate) fn text_of(doc: &Document, id: u32) -> String {
    let at = doc.find_shape(Owner::Page(1), id).unwrap_or_else(|| panic!("no shape {id}"));
    let shape = doc.shape(&at).unwrap();
    let text = match &shape.text {
        Some(own) => Some(own.clone()),
        None => Resolver::new(doc).master_step(&at).ok().flatten().and_then(|step| doc.shape(&step.shape).and_then(|m| m.text.clone())),
    };
    text.iter()
        .flat_map(|t| t.parts.iter())
        .filter_map(|part| match part {
            TextPart::Chars(s) => Some(s.to_string()),
            _ => None,
        })
        .collect()
}

pub(crate) fn height_of(doc: &Document, id: u32) -> f64 {
    live(doc, id, "Height")
}

/// The members of a group, as page `ID`s in the master's order.
pub(crate) fn members_of(doc: &Document, id: u32) -> Vec<u32> {
    let at = doc.find_shape(Owner::Page(1), id).unwrap();
    doc.shape(&at).unwrap().children.iter().map(|m| m.id).collect()
}

/// `n` lines of text, one word each.
fn lines(n: usize) -> String {
    (1..=n).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n")
}

/// The top edge of a member as the page's height of the group's box: the pin's `y` and half the member's height.
pub(crate) fn top_of(doc: &Document, member: u32) -> f64 {
    live(doc, member, "PinY") + live(doc, member, "Height") / 2.0
}

/// The notes a save in place of an `.odg` gives for a new shape of `master` of `stencil` dropped into it: none when the save is silent. A note that says the outline is written as a rectangle
/// means another program draws a rectangle for the shape.
pub(crate) fn odg_notes(stencil: &str, master: &str) -> Vec<String> {
    let dir = Dir::new();
    let path = dir.path("a.odg");
    let mut base = blank();
    run(&mut base, "file.save_as", json!({ "path": path }));
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    drop_in(&mut s, stencil, master, 4.0, 5.0);
    let saved = s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("{master}: the save was refused: {e}"));
    saved["warnings"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect()
}

// ---- the catalogue --------------------------------------------------------------------------------------------------------------------

#[test]
fn the_stencil_ships_fifteen_masters_in_the_order_the_window_lists_them() {
    let stencil = builtin_stencil(STENCIL).expect("uml-class is shipped");
    assert_eq!(stencil.masters().iter().map(|m| m.id).collect::<Vec<_>>(), MASTER_IDS);
    assert_eq!(stencil.masters().iter().filter(|m| m.is_connector()).count(), 8, "eight lines");
    assert!(!stencil.default_open, "closed in a new session: a template opens it");
    assert_eq!(registered().len(), 20);
    assert!(stencils().iter().any(|s| s.id == STENCIL));
}

#[test]
fn the_quick_shapes_are_class_interface_enumeration_and_package() {
    let mut s = blank();
    let quick = run(&mut s, "stencil.quick", json!({ "stencil": STENCIL }));
    let ids: Vec<&str> = quick["stencils"][0]["masters"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["uml-class.class", "uml-class.interface", "uml-class.enumeration", "uml-class.package"]);
}

#[test]
fn the_autoconnect_strip_passes_over_the_package() {
    // A container is no shape to join with a connector, so it stays a Quick Shape where it is and the strip offers the others.
    let mut s = blank();
    let quick = run(&mut s, "stencil.quick", json!({ "stencil": STENCIL }));
    let offered: Vec<&str> = quick["offer"]["masters"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert_eq!(offered, ["uml-class.class", "uml-class.interface", "uml-class.enumeration"]);
}

// ---- the class: compartments that follow their text -----------------------------------------------------------------------------------

#[test]
fn a_class_compartment_grows_by_one_lines_height_and_the_class_with_it() {
    let mut s = blank();
    let class = drop_at(&mut s, "uml-class.class", 5.0, 5.0);
    let members = members_of(s.document().unwrap(), class);
    assert_eq!(members.len(), 3, "operations, attributes and the name");
    let (operations, attributes, name) = (members[0], members[1], members[2]);
    let (name_was, operations_was) = (height_of(s.document().unwrap(), name), height_of(s.document().unwrap(), operations));
    // Four lines against five (both are taller than the least height): the difference is a line of the text, whatever face draws it.
    run(&mut s, "shape.set_text", json!({ "id": attributes, "text": lines(4) }));
    let (h4, class4) = (height_of(s.document().unwrap(), attributes), height_of(s.document().unwrap(), class));
    run(&mut s, "shape.set_text", json!({ "id": attributes, "text": lines(5) }));
    let doc = s.document().unwrap();
    let (h5, class5) = (height_of(doc, attributes), height_of(doc, class));
    let line = h5 - h4;
    assert!(line > 0.08 && line < 0.35, "a line of ten point text is about a sixth of an inch: {line}");
    assert!(near(class5 - class4, line), "the class grew by the line and no more: {class4} to {class5}");
    // It is the attributes that grew, and the class is as tall as its three compartments together.
    assert!(near(height_of(doc, name), name_was) && near(height_of(doc, operations), operations_was), "the others stay");
    assert!(near(class5, height_of(doc, name) + h5 + height_of(doc, operations)));
    assert!(near(cached(doc, class, "Height").unwrap(), class5), "the recalculation wrote the class's height");
    // One line and two lines fit the least height, so the class does not shrink below its own height: two lines are never shorter than one.
    run(&mut s, "shape.set_text", json!({ "id": attributes, "text": "one" }));
    let one = height_of(s.document().unwrap(), class);
    run(&mut s, "shape.set_text", json!({ "id": attributes, "text": "one\ntwo" }));
    let two = height_of(s.document().unwrap(), class);
    assert!(two >= one - 1e-9, "two lines are not shorter than one: {one} and {two}");
    // The text and the height are one undo step: undo gives the class as it was with the text of one line.
    run(&mut s, "edit.undo", json!({}));
    assert!(near(height_of(s.document().unwrap(), class), one), "one undo takes the two lines and the height together");
    assert_eq!(text_of(s.document().unwrap(), attributes), "one");
}

#[test]
fn a_resize_of_a_class_changes_its_width_only() {
    let mut s = blank();
    let class = drop_at(&mut s, "uml-class.class", 5.0, 5.0);
    let members = members_of(s.document().unwrap(), class);
    run(&mut s, "shape.set_text", json!({ "id": members[1], "text": lines(3) }));
    let frame = draw::shape_frame(s.document().unwrap(), 1, class).unwrap();
    let heights: Vec<f64> = members.iter().map(|m| height_of(s.document().unwrap(), *m)).collect();
    // The window's handles and `shape.resize` with `id` send a box: the width is the box's and the height is the compartments'.
    let (w, h) = (frame.width * 2.0, frame.height * 0.5);
    run(&mut s, "shape.resize", json!({ "id": class, "x": frame.pin_x - w / 2.0, "y": frame.pin_y - h / 2.0, "width": w, "height": h }));
    let doc = s.document().unwrap();
    let after = draw::shape_frame(doc, 1, class).unwrap();
    assert!(near(after.width, w), "the width is sized: {after:?}");
    assert!(near(after.height, frame.height), "the height is the compartments': {} to {}", frame.height, after.height);
    for (m, was) in members.iter().zip(&heights) {
        assert!(near(live(doc, *m, "Width"), w), "the compartment follows the width");
        assert!(near(height_of(doc, *m), *was), "and keeps its height");
    }
    assert!(near(top_of(doc, members[2]), live(doc, class, "Height")), "the name is still at the top");
    // The class says so itself: it holds its height, and the group is selected first.
    assert_eq!(cached(doc, class, "LockHeight"), Some(1.0));
    assert_eq!(cached(doc, class, "SelectMode"), Some(1.0));
}

#[test]
fn typing_on_a_selected_class_edits_its_name() {
    let mut s = blank();
    let class = drop_at(&mut s, "uml-class.class", 5.0, 5.0);
    let doc = s.document().unwrap();
    let members = members_of(doc, class);
    // Typing on the selected group goes to the member last in document order, when the group's own cell says so: that member is the name, at the top.
    assert_eq!(cached(doc, class, "IsTextEditTarget"), Some(0.0), "the text is the members'");
    let last = *members.last().unwrap();
    assert!(near(top_of(doc, last), live(doc, class, "Height")), "the member typing goes to stands at the top of the class");
    assert_eq!(text_of(doc, last), "Class", "and holds the name");
    assert_eq!(text_of(doc, members[1]), "+ attribute : Type");
    assert_eq!(text_of(doc, members[0]), "+ operation()");
    run(&mut s, "shape.set_text", json!({ "id": last, "text": "Account" }));
    let doc = s.document().unwrap();
    assert_eq!(text_of(doc, last), "Account");
    assert_eq!(text_of(doc, members[1]), "+ attribute : Type", "the attributes are as they were");
    assert!(near(top_of(doc, last), live(doc, class, "Height")));
}

#[test]
fn an_abstract_class_has_an_italic_name() {
    let mut s = blank();
    let class = drop_at(&mut s, "uml-class.class", 5.0, 5.0);
    let name = *members_of(s.document().unwrap(), class).last().unwrap();
    // Bold and upright by default (bit 1 of `Style` is bold, bit 2 italic).
    let style = |s: &Session| row_cell(s.document().unwrap(), name, "Character", 0, "Style").unwrap();
    assert_eq!(style(&s), 1.0, "the name is bold");
    assert!(formula_of(s.document().unwrap(), name, "Width").is_some_and(|f| f.contains("Sheet.5")), "a member's formulas name its group");
    let upright = render(&s);
    run(&mut s, "shapedata.set", json!({ "id": class, "row": "Abstract", "value": "yes" }));
    assert_eq!(style(&s), 3.0, "bold and italic when the class is abstract");
    let slanted = render(&s);
    assert_ne!(upright.rgba_premul, slanted.rgba_premul, "the name is drawn in another slant");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(style(&s), 1.0, "undo brings the upright name back");
    run(&mut s, "shapedata.set", json!({ "id": class, "row": "Abstract", "value": "yes" }));
    run(&mut s, "shapedata.set", json!({ "id": class, "row": "Abstract", "value": "no" }));
    assert_eq!(style(&s), 1.0, "false is upright again");
    assert_eq!(upright.rgba_premul, render(&s).rgba_premul, "and draws as it did");
}

#[test]
fn an_interface_shows_its_keyword_in_plain_type_above_its_bold_name() {
    // «interface» in plain type, centred, above the bold name; the name is what typing on the interface edits.
    let mut s = blank();
    let interface = drop_at(&mut s, "uml-class.interface", 5.0, 5.0);
    let doc = s.document().unwrap();
    let members = members_of(doc, interface);
    let (keyword, name) = (members[members.len() - 2], members[members.len() - 1]);
    assert_eq!(text_of(doc, keyword), "\u{ab}interface\u{bb}");
    assert_eq!(text_of(doc, name), "Interface");
    assert_eq!(row_cell(doc, keyword, "Character", 0, "Style"), Some(0.0), "plain type");
    assert_eq!(row_cell(doc, name, "Character", 0, "Style"), Some(1.0), "a bold name");
    assert!(row_cell(doc, keyword, "Paragraph", 0, "HorzAlign").is_none_or(|a| a == 1.0), "centred is the default");
    assert!(top_of(doc, keyword) > top_of(doc, name), "the keyword is above the name");
    assert!(near(top_of(doc, keyword), live(doc, interface, "Height")), "and at the top of the interface");
    // Enumerations and data types have their keyword too, and their name last.
    for (id, keyword_text, name_text) in
        [("uml-class.enumeration", "\u{ab}enumeration\u{bb}", "Enumeration"), ("uml-class.data-type", "\u{ab}dataType\u{bb}", "DataType")]
    {
        let shape = drop_at(&mut s, id, 8.0, 5.0);
        let doc = s.document().unwrap();
        let members = members_of(doc, shape);
        assert_eq!(text_of(doc, members[members.len() - 2]), keyword_text, "{id}");
        assert_eq!(text_of(doc, members[members.len() - 1]), name_text, "{id}");
    }
}

#[test]
fn an_object_names_itself_underlined_and_a_class_names_do_not() {
    let mut s = blank();
    let object = drop_at(&mut s, "uml-class.object", 5.0, 5.0);
    let class = drop_at(&mut s, "uml-class.class", 8.0, 5.0);
    let doc = s.document().unwrap();
    let name_of = |shape: u32| *members_of(doc, shape).last().unwrap();
    assert_eq!(text_of(doc, name_of(object)), "object : Class");
    let style = |shape: u32| row_cell(doc, name_of(shape), "Character", 0, "Style").unwrap() as u32;
    assert_eq!(style(object) & 4, 4, "the underline bit");
    assert_eq!(style(class) & 4, 0);
    assert_eq!(members_of(doc, object).len(), 2, "the name over the slots");
    assert_eq!(text_of(doc, members_of(doc, object)[0]), "attribute = value");
}

#[test]
fn every_compartment_list_is_left_aligned_and_a_name_is_centred() {
    let mut s = blank();
    let class = drop_at(&mut s, "uml-class.class", 5.0, 5.0);
    let doc = s.document().unwrap();
    let members = members_of(doc, class);
    // `HorzAlign` 0 is left and 1 is centred (the default, a row that is not there).
    assert_eq!(row_cell(doc, members[0], "Paragraph", 0, "HorzAlign"), Some(0.0), "operations");
    assert_eq!(row_cell(doc, members[1], "Paragraph", 0, "HorzAlign"), Some(0.0), "attributes");
    assert!(row_cell(doc, members[2], "Paragraph", 0, "HorzAlign").is_none_or(|a| a == 1.0), "the name");
    assert_eq!(cached(doc, members[1], "VerticalAlign"), Some(0.0), "a list starts at the top of its compartment");
}

// ---- the package ---------------------------------------------------------------------------------------------------------------------

#[test]
fn a_package_takes_a_dropped_class_as_a_member() {
    let mut s = blank();
    let package = drop_at(&mut s, "uml-class.package", 5.0, 5.0);
    let info = run(&mut s, "container.info", json!({ "ids": [package] }));
    assert_eq!(info["containers"][0]["id"], package, "an instance of the package is a container: {info}");
    // A class dropped in its body joins it.
    let class = drop_at(&mut s, "uml-class.class", 5.0, 4.8);
    let info = run(&mut s, "container.info", json!({ "ids": [package] }));
    assert_eq!(info["containers"][0]["members"], json!([class]), "{info}");
    // A move of the package takes the class with it.
    let was = draw::shape_corners(s.document().unwrap(), 1, class).unwrap();
    run(&mut s, "shape.move", json!({ "ids": [package], "dx": 1.5, "dy": 1.0 }));
    let now = draw::shape_corners(s.document().unwrap(), 1, class).unwrap();
    assert!(was.iter().zip(now.iter()).all(|(a, b)| near(a.0 + 1.5, b.0) && near(a.1 + 1.0, b.1)), "the class went with the package");
    // A class dropped elsewhere does not join it.
    let outside = drop_at(&mut s, "uml-class.class", 9.5, 7.5);
    let info = run(&mut s, "container.info", json!({ "ids": [package] }));
    assert_eq!(info["containers"][0]["members"], json!([class]), "{outside} is not a member: {info}");
}

#[test]
fn a_package_is_a_tab_over_a_body_with_its_name_in_the_tab_and_its_points_on_the_outline() {
    let mut s = blank();
    let package = drop_at(&mut s, "uml-class.package", 5.0, 4.25);
    run(&mut s, "shape.set_text", json!({ "id": package, "text": "domain" }));
    let drawn = render(&s);
    // The package is 2.4 by 1.6 inches at (5, 4.25): its left edge is at 3.8, its top at 5.05. The tab is a quarter inch tall over the left of the top edge, and the body's top edge is at 4.8.
    let (left, top) = (3.8, 5.05);
    let dark = |x: f64, y: f64| {
        let p = px(&drawn, x, y);
        p[3] > 128 && u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) < 600
    };
    assert!(dark(left + 0.3, top - 0.004), "the tab's top edge is over the left of the box");
    assert!(!dark(left + 2.0, top - 0.004) && !dark(left + 2.0, top - 0.12), "and there is no line there over the right of it");
    assert!(dark(left + 2.0, top - 0.25), "the body's top edge runs under the tab and past it");
    assert!(dark(left + 0.004, top - 1.0) && dark(left + 2.4 - 0.004, top - 1.0), "the sides");
    assert!(dark(left + 1.2, top - 1.6 + 0.004), "and the bottom");
    let points = draw::connection_points(s.document().unwrap(), 1, package);
    assert_eq!(points.len(), 12, "three on each side, the top's on the tab and the body");
}

// ---- the lines ------------------------------------------------------------------------------------------------------------------------

/// The ends, pattern and route the design's section 6.1 and give each line: (master, `BeginArrow`, `EndArrow`, `LinePattern`).
const LINES: [(&str, f64, f64, f64); 8] = [
    ("uml-class.association", 0.0, 0.0, 1.0),
    ("uml-class.directed-association", 0.0, 3.0, 1.0),
    ("uml-class.aggregation", 22.0, 0.0, 1.0),
    ("uml-class.composition", 254.0, 0.0, 1.0),
    ("uml-class.generalization", 0.0, 16.0, 1.0),
    ("uml-class.realization", 0.0, 16.0, 2.0),
    ("uml-class.dependency", 0.0, 3.0, 2.0),
    ("uml-class.note-link", 0.0, 0.0, 2.0),
];

#[test]
fn each_relationship_has_the_ends_and_pattern_of_6_1() {
    let mut s = blank();
    for (n, (id, begin, end, pattern)) in LINES.iter().enumerate() {
        let line = drop_at(&mut s, id, 5.0, 1.0 + n as f64);
        let doc = s.document().unwrap();
        assert_eq!(cached(doc, line, "BeginArrow"), Some(*begin), "{id}: the begin");
        assert_eq!(cached(doc, line, "EndArrow"), Some(*end), "{id}: the end");
        assert_eq!(cached(doc, line, "LinePattern").unwrap_or(1.0), *pattern, "{id}: the pattern");
        // The master holds them and the instance only its ends: the line is the master's look.
        let at = doc.find_shape(Owner::Page(1), line).unwrap();
        let shape = doc.shape(&at).unwrap();
        for cell in ["BeginArrow", "EndArrow", "LinePattern"] {
            assert!(shape.sheet.cell(cell).is_none(), "{id}: the instance holds no `{cell}`");
        }
        // Right angles for a relationship (ours: the class diagram's), straight for the note link.
        let route = if id.ends_with("note-link") { 2.0 } else { 1.0 };
        assert_eq!(cached(doc, line, "ShapeRouteStyle"), Some(route), "{id}: the route of the master's");
    }
    // The listing says the same, for the window's tiles.
    run(&mut s, "stencil.open", json!({ "id": STENCIL }));
    let list = run(&mut s, "stencil.list", json!({}));
    let entry = list["stencils"].as_array().unwrap().iter().find(|e| e["id"] == STENCIL).unwrap();
    for (id, begin, end, pattern) in LINES {
        let master = entry["masters"].as_array().unwrap().iter().find(|m| m["id"] == id).unwrap();
        assert_eq!(master["one_d"], json!(true), "{id}");
        assert_eq!(master["ends"], json!({ "begin": begin as u8, "end": end as u8, "pattern": pattern as u8 }), "{id}");
    }
}

#[test]
fn composition_begins_with_the_filled_diamond() {
    // The whole's end (the begin) holds the filled diamond, the part's end nothing.
    let mut s = blank();
    let whole = drop_at(&mut s, "uml-class.class", 3.0, 5.0);
    let part = drop_at(&mut s, "uml-class.class", 8.0, 5.0);
    let line = drop_at(&mut s, "uml-class.composition", 5.5, 8.0);
    let aggregation = drop_at(&mut s, "uml-class.aggregation", 5.5, 2.0);
    for (id, target) in [(line, [whole, part]), (aggregation, [whole, part])] {
        run(&mut s, "connector.reconnect", json!({ "page": 1, "id": id, "end": "begin", "target": { "id": target[0] } }));
        run(&mut s, "connector.reconnect", json!({ "page": 1, "id": id, "end": "end", "target": { "id": target[1] } }));
    }
    let doc = s.document().unwrap();
    assert_eq!(formula_of(doc, line, "BeginArrow").as_deref(), Some("USE(\"Filled Diamond\")"));
    assert_eq!(cached(doc, line, "BeginArrow"), Some(254.0));
    assert_eq!(cached(doc, line, "EndArrow"), Some(0.0), "and nothing at the part's end");
    // The master came into the drawing with the line, as a line end.
    let master = doc.masters.iter().find(|m| m.name.as_deref() == Some("Filled Diamond")).expect("the line end came with the connector");
    assert_eq!(master.pattern_flags, Some(2), "a master that is a line end");
    // It draws: the middle of the diamond at the whole's end is dark for the composition and bare for the aggregation's hollow one, and the part's end has no diamond.
    let drawn = render(&s);
    let ends = |id: u32| {
        (cached(doc, id, "BeginX").unwrap(), cached(doc, id, "BeginY").unwrap(), cached(doc, id, "EndX").unwrap(), cached(doc, id, "EndY").unwrap())
    };
    let (bx, by, ex, ey) = ends(line);
    let sign = if ex > bx { 1.0 } else { -1.0 };
    let dark = |x: f64, y: f64| {
        let p = px(&drawn, x, y);
        p[3] > 128 && u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) < 300
    };
    assert!(dark(bx + sign * 0.07, by), "the filled diamond is solid near the whole");
    assert!(!dark(ex - sign * 0.07, ey + 0.05) && !dark(ex - sign * 0.07, ey - 0.05), "and nothing is drawn beside the line at the part's end");
    let (abx, aby, _, _) = ends(aggregation);
    assert!(!dark(abx + sign * 0.10, aby), "the aggregation's diamond is hollow at its middle");
}

#[test]
fn a_relationship_is_glued_between_two_classes_and_follows_the_second() {
    let mut s = blank();
    let a = drop_at(&mut s, "uml-class.class", 3.0, 5.0);
    let b = drop_at(&mut s, "uml-class.interface", 8.0, 5.0);
    let link = drop_at(&mut s, "uml-class.realization", 5.5, 8.0);
    run(&mut s, "connector.reconnect", json!({ "page": 1, "id": link, "end": "begin", "target": { "id": a } }));
    run(&mut s, "connector.reconnect", json!({ "page": 1, "id": link, "end": "end", "target": { "id": b } }));
    assert_eq!(s.document().unwrap().page(1).unwrap().connects.len(), 2, "glued at both ends");
    let before = cached(s.document().unwrap(), link, "EndY").unwrap();
    run(&mut s, "shape.move", json!({ "ids": [b], "dx": 0.0, "dy": -1.5 }));
    let after = cached(s.document().unwrap(), link, "EndY").unwrap();
    assert!(near(after, before - 1.5), "the end follows the interface: {before} to {after}");
    assert_eq!(cached(s.document().unwrap(), link, "EndArrow"), Some(16.0), "the hollow triangle stays");
}

// ---- the note, the search and the templates -----------------------------------------------------------------------------------------

#[test]
fn a_note_is_a_dog_eared_box_with_plain_text_that_a_note_link_joins() {
    let mut s = blank();
    let note = drop_at(&mut s, "uml-class.note", 5.0, 4.25);
    let doc = s.document().unwrap();
    assert_eq!(text_of(doc, note), "Note");
    assert_eq!(draw::connection_points(doc, 1, note).len(), 4);
    let drawn = render(&s);
    // The note is 1.4 by 0.8 inch at (5, 4.25): the top right corner is cut, so the corner is bare paper and the fold is ink.
    let alpha = |x: f64, y: f64| px(&drawn, x, y)[3];
    assert_eq!(alpha(5.0 + 0.7 - 0.02, 4.25 + 0.4 - 0.02), 0, "the cut corner is bare");
    assert!(alpha(5.0 - 0.2, 4.25 + 0.2) > 0 && alpha(5.0 - 0.7 + 0.1, 4.25 - 0.3) > 0, "and the rest is the note");
}

#[test]
fn every_master_has_search_words_and_the_names_of_the_inventory_find_their_masters() {
    for id in MASTER_IDS {
        assert!(!super::keywords(id).is_empty(), "{id} has search words");
    }
    let mut s = blank();
    // Names a drawing from the reference app carries for these figures (the inventory's C67 and C75) find the master that serves the purpose.
    for (word, id) in [
        ("Member", "uml-class.class"),
        ("Composite", "uml-class.class"),
        ("Inheritance", "uml-class.generalization"),
        ("Interface Realization", "uml-class.realization"),
        ("Enum", "uml-class.enumeration"),
        ("Namespace", "uml-class.package"),
        ("Comment", "uml-class.note"),
        ("Whole", "uml-class.composition"),
    ] {
        let found = run(&mut s, "stencil.search", json!({ "text": word }));
        assert!(found.to_string().contains(&format!("\"{id}\"")), "`{word}` finds {id}: {found}");
    }
    assert!(find_master("uml-class.class").is_some_and(|m| m.is_group()));
}

#[test]
fn the_uml_class_template_is_listed_and_opens_its_stencil_beside_the_basic_shapes() {
    let mut s = Session::new();
    let templates = run(&mut s, "template.list", json!({}));
    let entry = templates["templates"].as_array().unwrap().iter().find(|t| t["id"] == "uml_class").unwrap_or_else(|| panic!("listed: {templates}"));
    assert!(entry["description"].as_str().unwrap().contains("UML 2.5.1"), "{entry}");
    run(&mut s, "file.new", json!({ "template": "uml_class" }));
    let list = run(&mut s, "stencil.list", json!({}));
    let open: Vec<&str> = list["stencils"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap()).collect();
    assert_eq!(open, ["uml-class", "basic"], "in the Shapes window's order");
}

// ---- hostile and costly input --------------------------------------------------------------------------------------------------------

#[test]
fn a_class_holding_the_longest_text_is_finite_and_a_page_of_classes_drops_and_draws() {
    let mut s = blank();
    let class = drop_at(&mut s, "uml-class.class", 5.0, 4.0);
    let attributes = members_of(s.document().unwrap(), class)[1];
    let long = "a".repeat(100_000);
    run(&mut s, "shape.set_text", json!({ "id": attributes, "text": long }));
    let doc = s.document().unwrap();
    assert!(height_of(doc, class).is_finite() && height_of(doc, class) > 1.2, "a tall, finite class");
    // Twenty classes of every master that is a box drop and draw with nothing skipped.
    let mut s = blank();
    for (n, id) in MASTER_IDS.iter().take(7).enumerate() {
        for k in 0..3 {
            drop_at(&mut s, id, 1.0 + 3.0 * f64::from(k), 1.0 + n as f64);
        }
    }
    let drawn = render(&s);
    assert!(drawn.skipped.is_empty(), "{:?}", drawn.skipped);
}

#[test]
fn typing_in_one_class_costs_the_same_on_a_page_of_sixty_classes_as_on_a_page_of_two() {
    // The design's section 4.9: a page of classes is recalculated in counted work, and an edit works out the cells of its own class and no others.
    let cost = |n: u32| -> (u64, u64, u64) {
        let mut s = blank();
        let mut first = 0;
        for k in 0..n {
            let id = drop_at(&mut s, "uml-class.class", 1.0 + f64::from(k % 8) * 1.2, 1.0 + f64::from(k / 8) * 1.3);
            if k == 0 {
                first = id;
            }
        }
        // A first edit, so that what a session keeps between edits is made.
        run(&mut s, "shape.set_text", json!({ "id": first + 2, "text": "warm" }));
        diacraft_calc::work::reset();
        run(&mut s, "shape.set_text", json!({ "id": first + 2, "text": "a\nb\nc" }));
        (diacraft_calc::work::steps(), diacraft_calc::work::formulas_asked(), diacraft_calc::work::cells_walked())
    };
    let (small, large) = (cost(2), cost(60));
    assert_eq!(small, large, "(steps, formulas asked, cells walked) of one edit");
    assert!(small.0 > 0 && small.0 < 5_000, "the steps of the edit: {}", small.0);
}

#[test]
fn no_master_of_the_stencil_is_written_as_a_rectangle_for_other_programs_in_an_odg() {
    // The package's tab stands on its body by a fixed length (`Height` less a quarter inch) which an `.odg` has no place for, so the outline was a rectangle for other programs: the tab is a clamped
    // radius term, which is written as the value it has at the size written.
    for id in MASTER_IDS {
        let notes = odg_notes(STENCIL, id);
        assert!(notes.iter().all(|n| !n.contains("written as a rectangle")), "{id}: {notes:?}");
    }
}

/// Whether every two-dimensional master of `stencil` holds `ObjType` 1 (a shape people join, which a route goes round from the start) and no line holds it: a connector
/// reads the routable 2 it is made with, and a fixed figure none.
pub(crate) fn assert_boxes_hold_object_type_1(stencil: &str) {
    let mut s = blank();
    for (n, master) in builtin_stencil(stencil).unwrap().masters().iter().enumerate() {
        let id = drop_in(&mut s, stencil, master.id, 2.0 + n as f64 * 0.5, 4.0);
        let kind = cached(s.document().unwrap(), id, "ObjType");
        if master.is_fixed_1d() {
            assert_eq!(kind, None, "{}: a fixed figure holds no ObjType", master.id);
        } else if master.is_connector() {
            assert_eq!(kind, Some(2.0), "{}: a line holds no ObjType 1, and is routable", master.id);
        } else {
            assert_eq!(kind, Some(1.0), "{}: a shape people join", master.id);
        }
    }
}

#[test]
fn every_box_of_the_stencil_holds_objtype_1_and_no_line_does() {
    assert_boxes_hold_object_type_1(STENCIL);
}

// ---- at a scale beyond the range of eight -------------------------------------------------------------------------------

/// What a drop of `master` on a page at `drawing` inches to `paper` inch makes, looked at from below: the master's height on paper (inches), the heights of its compartments on paper, and
/// the blank rows (200 to the inch) between the bottom edge and the lowest ink of the last compartment's text, that is the gap under its last line, on paper.
fn compartments_at_scale(stencil: &str, master: &str, (paper, drawing): (f64, f64)) -> (f64, Vec<f64>, usize) {
    let mut s = blank();
    run(&mut s, "page.scale", json!({"paper": paper, "drawing": drawing}));
    let per_inch: f64 = drawing / paper;
    // (A drop's place is on paper.)
    let class = drop_in(&mut s, stencil, master, 5.5, 4.25);
    let doc = s.document().unwrap();
    let (pin_x, pin_y, width, height) = (live(doc, class, "PinX"), live(doc, class, "PinY"), live(doc, class, "Width"), live(doc, class, "Height"));
    let (left, right, bottom) = ((pin_x - width / 2.0) / per_inch, (pin_x + width / 2.0) / per_inch, (pin_y - height / 2.0) / per_inch);
    let members: Vec<u32> = doc.find_shape(Owner::Page(1), class).and_then(|at| doc.shape(&at)).unwrap().children.iter().map(|m| m.id).collect();
    let heights = members.iter().map(|m| live(doc, *m, "Height") / per_inch).collect();
    let rendered = render(&s);
    let ink_in_row = |row: usize| {
        let y = bottom + 0.0025 + row as f64 * 0.005;
        let mut n = 0;
        let mut x = left + 0.03;
        while x < right - 0.03 {
            let p = super::super::flowchart_tests::px(&rendered, x, y);
            n += usize::from(p[3] > 128 && u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) < 600);
            x += 0.005;
        }
        n
    };
    let rows: Vec<usize> = (0..90).map(ink_in_row).collect();
    // The bottom edge is the first rows, a line across; the gap is the blank rows after it, up to the first ink.
    let edge = rows.iter().position(|n| *n < 100).unwrap_or(0);
    let gap = rows.iter().skip(edge).take_while(|n| **n == 0).count();
    (height / per_inch, heights, gap)
}

/// Issue 4 (found: "at 100:1 a UML Class's last line sits on the shape's bottom edge after a drop"). **It does not, and nothing here is wrong**: the margins of a text
/// are paper lengths (4 points at the top and the bottom of the block, whatever the page's scale), and a compartment is as tall as its text and its margins
/// (`TEXTHEIGHT`, divided by the scale). Measured, before a change, on a drop of each of the seven masters of compartments (UML Class's five, `class`, `interface`, `enumeration`, `data-type` and `object`, and Crow's
/// Foot's `entity` and `entity-with-keys`) at 1:1, 1:20 and 1:100 (200 rows to the inch): the lowest ink of the last line is 12 to 18 rows (0.06 to 0.09 inch, the margin and the line's own air) from the bottom edge
/// beyond the range of eight, the same at 1:20 and at 1:100. **What changes with the scale is the least height of a compartment**, which is a number in the master's formula (0.4 inch for the
/// class) and so a constant that a drop leaves alone beyond the range (DROPSCALE R15): at 1:1 it holds the last line 0.2 inch off the bottom edge (37 rows), and at 1:20 and 1:100 it is
/// 0.02 and 0.004 inch on paper and holds nothing, so each compartment is as tall as its text and the class is 0.85 inch tall on paper, not the master's 1.2 (Crow's Foot's entity 0.59, not 1.2).
/// The documented remedy is a master whose formula names `DropOnPageScale`, which the owner ruled for the least height and made: the shape is
/// the master's height on paper at every scale now, and the margin of the last line is what this test holds besides.
#[test]
fn the_last_line_of_a_compartment_keeps_its_margin_at_a_scale_beyond_the_range_of_eight() {
    /// The least gap under the last line, in rows of 1/200 inch: 0.05 inch, a little under the 4 points of the margin (11 rows) so that the face the machine draws with does not matter.
    const LEAST_GAP: usize = 10;
    let masters = [
        ("uml-class", "uml-class.class"),
        ("uml-class", "uml-class.interface"),
        ("uml-class", "uml-class.enumeration"),
        ("uml-class", "uml-class.data-type"),
        ("uml-class", "uml-class.object"),
        ("crows-foot", "crows-foot.entity"),
        ("crows-foot", "crows-foot.entity-with-keys"),
    ];
    for (stencil, master) in masters {
        let (whole, whole_heights, whole_gap) = compartments_at_scale(stencil, master, (1.0, 1.0));
        let (twenty, twenty_heights, twenty_gap) = compartments_at_scale(stencil, master, (1.0, 20.0));
        let (hundred, hundred_heights, hundred_gap) = compartments_at_scale(stencil, master, (1.0, 100.0));
        for (what, gap) in [("1:1", whole_gap), ("1:20", twenty_gap), ("1:100", hundred_gap)] {
            assert!(
                gap >= LEAST_GAP,
                "{master} at {what}: the last line is {gap} rows from the bottom edge, and keeps its margin of 4 points (11 rows)"
            );
        }
        // Beyond the range every length of the text, the margins included, is a length on paper: nothing of the compartments changes between 1:20 and 1:100.
        assert!(
            twenty_heights.iter().zip(&hundred_heights).all(|(a, b)| (a - b).abs() < 1e-6) && twenty_gap.abs_diff(hundred_gap) <= 1,
            "{master}: the compartments are as tall on paper at 1:20 as at 1:100 ({twenty_heights:?}, {hundred_heights:?}) and the gap is the same ({twenty_gap}, {hundred_gap})"
        );
        // The least height is scaled: at 1:1 it holds every compartment at least as tall as it says, and beyond the range it is the master's on
        // paper too, so the shape is the master's height on paper at 1:20 and 1:100 as at 1:1, with the margin of its last line kept (the gaps above).
        assert!(whole_heights.iter().all(|h| h + 1e-9 >= 0.26), "{master} at 1:1: {whole_heights:?}");
        assert!(
            (twenty - whole).abs() < 1e-6 && (hundred - whole).abs() < 1e-6,
            "{master}: beyond the range the shape is the master's height on paper ({whole} inch at 1:1, {twenty} at 1:20, {hundred} at 1:100)"
        );
    }
}
