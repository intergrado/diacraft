//! Crow's Foot Database Notation: the eight masters, the line end each cardinality word gives at either end, the defaults of a new relationship, the identifying
//! pattern, the entity that grows with its attribute lines and is resized by its width, the header band that follows the theme, the Quick Shapes, the search words and the template.
//! The catalogue's invariants (`stencil/catalogue_tests.rs`) and the five per-stencil checks (`tests/it/stencils2_crows_foot.rs`) run on every master beside these.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use diacraft_doc::{CellAddr, Document, FormulaSlot, Owner, Resolver, SheetRef};
use serde_json::{Value, json};

use super::{MASTERS, end_number};
use crate::Session;
use crate::stencil::{MasterSpec, builtin_stencil};

// ---- what the design lists -----------------------------------------------------------------------------------------------------------

/// The eight masters as the design's table gives them: id, name, width, height (inches), how many connection points.
const TABLE: [(&str, &str, f64, f64, usize); 8] = [
    ("crows-foot.entity", "Entity", 2.0, 1.2, 12),
    ("crows-foot.entity-with-keys", "Entity with Keys", 2.0, 1.5, 12),
    ("crows-foot.note", "Note", 1.4, 0.8, 4),
    ("crows-foot.relationship", "Relationship", 1.5, 0.0, 0),
    ("crows-foot.one-to-one", "One to One", 1.5, 0.0, 0),
    ("crows-foot.one-to-many", "One to Many", 1.5, 0.0, 0),
    ("crows-foot.many-to-many", "Many to Many", 1.5, 0.0, 0),
    ("crows-foot.note-link", "Note Link", 1.5, 0.0, 0),
];

/// The four cardinality words and the line end each gives (the table: exactly one 25, zero or one 30, one or many 28, zero or many 29).
const WORDS: [(&str, f64); 4] = [("Exactly one", 25.0), ("Zero or one", 30.0), ("One or many", 28.0), ("Zero or many", 29.0)];

fn master(id: &str) -> &'static MasterSpec {
    MASTERS.iter().find(|m| m.id == id).unwrap_or_else(|| panic!("no master {id}"))
}

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s
}

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command} {params}: {e}"))
}

/// Drops a master of the session's drawing at (x, y): its `ID`.
fn put(s: &mut Session, id: &str, (x, y): (f64, f64)) -> u32 {
    let dropped = run(s, "shape.drop", json!({ "master": id, "page": 1, "x": x, "y": y }));
    u32::try_from(dropped["id"].as_u64().unwrap()).unwrap()
}

/// What a cell of the shape `id` reads, through its master.
fn number(doc: &Document, id: u32, cell: &str) -> f64 {
    let at = doc.find_shape(Owner::Page(1), id).unwrap_or_else(|| panic!("no shape {id}"));
    let found = Resolver::new(doc).resolve(&SheetRef::Shape(at), &CellAddr::direct(cell)).unwrap_or_else(|| panic!("no cell {cell}"));
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap_or_else(|| panic!("no number in {cell} of shape {id}"))
}

fn ends_of(s: &Session, id: u32) -> (f64, f64, f64) {
    let doc = s.document().unwrap();
    (number(doc, id, "BeginArrow"), number(doc, id, "EndArrow"), number(doc, id, "LinePattern"))
}

fn set(s: &mut Session, id: u32, row: &str, value: &str) {
    run(s, "shapedata.set", json!({ "id": id, "row": row, "value": value }));
}

// ---- the catalogue --------------------------------------------------------------------------------------------------------------------

#[test]
fn crows_foot_holds_the_eight_masters_of_the_design_in_order_with_their_sizes_points_and_series() {
    assert_eq!(MASTERS.len(), 8);
    for (at, (m, (id, name, w, h, points))) in MASTERS.iter().zip(TABLE).enumerate() {
        assert_eq!((m.id, m.name, m.width, m.height), (id, name, w, h), "master {at}");
        assert_eq!(m.point_count(), points, "{id}: connection points");
        assert_eq!(m.identity, (20, u16::try_from(at + 1).unwrap()), "{id}: series 20, in the window's order");
    }
    let lines: Vec<&str> = MASTERS.iter().filter(|m| m.is_connector()).map(|m| m.id).collect();
    assert_eq!(
        lines,
        ["crows-foot.relationship", "crows-foot.one-to-one", "crows-foot.one-to-many", "crows-foot.many-to-many", "crows-foot.note-link"],
        "the five lines are connectors, and none of them is a fixed figure"
    );
    assert!(MASTERS.iter().all(|m| !m.is_fixed_1d()));
    assert!(master("crows-foot.entity").is_group() && master("crows-foot.entity-with-keys").is_group() && !master("crows-foot.note").is_group());
}

#[test]
fn the_stencil_ships_closed_by_default_and_its_template_is_listed_and_opens_it_beside_the_basic_shapes() {
    let stencil = builtin_stencil("crows-foot").expect("the stencil has masters, so it ships");
    assert_eq!(
        (stencil.name, stencil.category, stencil.default_open, stencil.masters().len()),
        ("Crow's Foot Database Notation", "Software and Database", false, 8)
    );
    let template = crate::template::find("crows_foot").expect("its stencil ships, so the template is listed");
    assert_eq!((template.name, template.stencils), ("Crow's Foot Database Notation", &["crows-foot", "basic"][..]));
    assert!(template.description.contains("based on"), "{}", template.description);
    let mut s = Session::new();
    let started = run(&mut s, "file.new", json!({ "template": "crows_foot" }));
    assert_eq!(started["stencils"]["open"], json!(["crows-foot", "basic"]), "{started}");
    let listed = run(&mut s, "template.list", json!({}));
    assert!(listed["templates"].as_array().unwrap().iter().any(|t| t["id"] == "crows_foot"));
    // A new session leaves the stencil closed and More Shapes lists it.
    let fresh = run(&mut Session::new(), "stencil.list", json!({}));
    let open: Vec<&str> = fresh["stencils"].as_array().unwrap().iter().filter_map(|t| t["id"].as_str()).collect();
    assert!(!open.contains(&"crows-foot"), "{open:?}");
    assert!(fresh["available"].as_array().unwrap().iter().any(|t| t["id"] == "crows-foot"));
}

#[test]
fn every_two_dimensional_master_holds_objtype_1_and_the_lines_hold_none() {
    assert_eq!(MASTERS.len(), 8, "a loop over nothing proves nothing");
    for m in MASTERS.iter().filter(|m| !m.is_connector()) {
        let mut s = session();
        let id = put(&mut s, m.id, (5.0, 5.0));
        assert_eq!(number(s.document().unwrap(), id, "ObjType"), 1.0, "{}: a shape people join", m.id);
    }
}

// ---- the line ends ---------------------------------------------------------------------------------------------------------------------

#[test]
fn end_number_gives_the_four_words_their_numbers_and_every_other_text_none() {
    for (word, number) in WORDS {
        assert_eq!(f64::from(end_number(word)), number, "{word}");
    }
    // The enum, the list the Shape Data holds and the table of this test are the same four words, in the same order.
    assert_eq!(super::Cardinality::ALL.map(super::Cardinality::word), WORDS.map(|(word, _)| word));
    assert_eq!(super::Cardinality::ALL.map(super::Cardinality::word).join(";"), super::CARDINALITIES);
    for other in ["", "None", "exactly one", "Exactly  one", "Many", "Zero or more", "Exactly one ", "0", &"x".repeat(40_000)] {
        assert_eq!(end_number(other), 0, "`{}` draws no mark", other.chars().take(20).collect::<String>());
    }
}

#[test]
fn each_cardinality_word_gives_its_line_end_at_either_end() {
    // The four numbers, at the begin and at the end, and the other end and the pattern stay as they were.
    for (word, number) in WORDS {
        let mut s = session();
        let line = put(&mut s, "crows-foot.relationship", (5.0, 5.0));
        let (begin, end, pattern) = ends_of(&s, line);
        assert_eq!((begin, end, pattern), (29.0, 25.0, 1.0), "a new relationship");
        set(&mut s, line, "BeginCardinality", word);
        assert_eq!(ends_of(&s, line), (number, end, pattern), "{word} at the begin");
        let mut s = session();
        let line = put(&mut s, "crows-foot.relationship", (5.0, 5.0));
        set(&mut s, line, "EndCardinality", word);
        assert_eq!(ends_of(&s, line), (begin, number, pattern), "{word} at the end");
    }
    // One or many at the begin and Zero or one at the end are 28 and 30.
    let mut s = session();
    let line = put(&mut s, "crows-foot.relationship", (5.0, 5.0));
    set(&mut s, line, "BeginCardinality", "One or many");
    set(&mut s, line, "EndCardinality", "Zero or one");
    assert_eq!(ends_of(&s, line), (28.0, 30.0, 1.0));
    // One undo takes the last word back, and the end is the one before it.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(ends_of(&s, line), (28.0, 25.0, 1.0));
}

#[test]
fn a_cardinality_that_matches_no_word_draws_no_end_and_a_huge_one_is_bounded() {
    // The Shape Data window refuses a text that is no item of the list, in words. A file or a formula can still hold one: the fixed list set to text no item matches shows no variant (the
    // design's section 9), here no mark, and a value of 32,768 characters is a bounded text.
    let mut s = session();
    let line = put(&mut s, "crows-foot.relationship", (5.0, 5.0));
    let refused = s.execute("shapedata.set", &json!({ "id": line, "row": "BeginCardinality", "value": "Many" })).unwrap_err().to_string();
    assert!(refused.contains("is not one of the items of this list"), "{refused}");
    assert_eq!(ends_of(&s, line), (29.0, 25.0, 1.0), "the refusal changed nothing");
    run(&mut s, "sheet.set_cell", json!({ "id": line, "cell": "Prop.BeginCardinality", "formula": "\"Many\"" }));
    assert_eq!(ends_of(&s, line), (0.0, 25.0, 1.0), "no word, no mark, and the other end is as it was");
    let huge = format!("\"{}\"", "Zero or many".repeat(2_731));
    let wrote = s.execute("sheet.set_cell", &json!({ "id": line, "cell": "Prop.EndCardinality", "formula": huge }));
    if wrote.is_ok() {
        assert_eq!(ends_of(&s, line).1, 0.0, "a text that only holds a word is no word");
    }
    let doc = s.document().unwrap();
    let view = diacraft_render::View::fit_page(doc, 1, 50.0).unwrap();
    let mut renderer = diacraft_render::Renderer::new();
    renderer.set_paper(None);
    let frame = renderer.render(doc, 1, &view);
    assert!(frame.skipped.is_empty(), "{:?}", frame.skipped);
}

#[test]
fn a_new_relationship_has_zero_or_many_at_the_begin_and_exactly_one_at_the_end() {
    // The defaults 29 and 25, solid; the three shortcuts differ only in the words they start with; the note link has no ends and is dashed.
    let expected = [
        ("crows-foot.relationship", (29.0, 25.0, 1.0), ("Zero or many", "Exactly one")),
        ("crows-foot.one-to-one", (25.0, 25.0, 1.0), ("Exactly one", "Exactly one")),
        ("crows-foot.one-to-many", (25.0, 29.0, 1.0), ("Exactly one", "Zero or many")),
        ("crows-foot.many-to-many", (29.0, 29.0, 1.0), ("Zero or many", "Zero or many")),
    ];
    for (id, ends, (begin, end)) in expected {
        let mut s = session();
        let line = put(&mut s, id, (5.0, 5.0));
        assert_eq!(ends_of(&s, line), ends, "{id}");
        let fields = run(&mut s, "shapedata.list", json!({ "id": line }));
        let value = |row: &str| fields["fields"].as_array().unwrap().iter().find(|f| f["row"] == row).map(|f| f["value"].clone());
        assert_eq!(
            (value("BeginCardinality"), value("EndCardinality")),
            (Some(json!(begin)), Some(json!(end))),
            "{id}: the Shape Data says the words"
        );
        assert_eq!(value("Identifying"), Some(json!("1")), "{id}: identifying until a person says otherwise");
        // The master says the same numbers to a window that draws its tile.
        assert_eq!(master(id).look().map(|l| l.numbers()), Some((ends.0 as u8, ends.1 as u8, 1)), "{id}: the master's own numbers");
    }
    let mut s = session();
    let link = put(&mut s, "crows-foot.note-link", (5.0, 5.0));
    assert_eq!(ends_of(&s, link), (0.0, 0.0, 2.0), "a note link has no ends and is dashed");
}

#[test]
fn stencil_list_gives_the_relationships_their_ends_and_pattern_for_a_tile() {
    let mut s = session();
    run(&mut s, "stencil.open", json!({ "id": "crows-foot" }));
    let listed = run(&mut s, "stencil.list", json!({}));
    let stencil = listed["stencils"].as_array().unwrap().iter().find(|t| t["id"] == "crows-foot").expect("the stencil is open now");
    let entry = |id: &str| stencil["masters"].as_array().unwrap().iter().find(|m| m["id"] == id).cloned().unwrap();
    for (id, begin, end, pattern) in [
        ("crows-foot.relationship", 29, 25, 1),
        ("crows-foot.one-to-one", 25, 25, 1),
        ("crows-foot.one-to-many", 25, 29, 1),
        ("crows-foot.many-to-many", 29, 29, 1),
        ("crows-foot.note-link", 0, 0, 2),
    ] {
        let m = entry(id);
        assert_eq!((m["one_d"].clone(), m["ends"].clone()), (json!(true), json!({ "begin": begin, "end": end, "pattern": pattern })), "{id}");
    }
    for id in ["crows-foot.entity", "crows-foot.entity-with-keys", "crows-foot.note"] {
        assert!(entry(id).get("ends").is_none(), "{id} is no line");
    }
}

#[test]
fn a_non_identifying_relationship_is_dashed() {
    // `Identifying` true is a solid line and false a dashed one; the cell is a formula of the Shape Data, so the Shape Data window changes it.
    let mut s = session();
    let line = put(&mut s, "crows-foot.relationship", (5.0, 5.0));
    assert_eq!(number(s.document().unwrap(), line, "LinePattern"), 1.0);
    set(&mut s, line, "Identifying", "0");
    assert_eq!(number(s.document().unwrap(), line, "LinePattern"), 2.0, "dashed");
    set(&mut s, line, "Identifying", "1");
    assert_eq!(number(s.document().unwrap(), line, "LinePattern"), 1.0, "solid again");
    // The master holds the formula, and the ends are not touched by it.
    let doc = s.document().unwrap();
    let at = doc.find_shape(Owner::Page(1), line).unwrap();
    let master = doc.master(doc.shape(&at).unwrap().master.unwrap()).unwrap();
    let cell = master.shapes.get(0).unwrap().sheet.cell("LinePattern").unwrap().clone();
    assert!(matches!(&cell.formula, FormulaSlot::Local(f) if f.source.contains("Prop.Identifying")), "{:?}", cell.formula);
    assert_eq!(ends_of(&s, line), (29.0, 25.0, 1.0));
}

#[test]
fn the_masters_hold_their_ends_as_formulas_of_the_shape_data_and_the_instance_holds_none() {
    // The marks are the line's own begin and end ends. A formula of the Shape Data on the master, so an instance inherits it and an edit of the data moves it; the instance holds only
    // what differs (its Shape Data row and what a recalculation writes for it).
    let mut s = session();
    let line = put(&mut s, "crows-foot.relationship", (5.0, 5.0));
    let doc = s.document().unwrap();
    let at = doc.find_shape(Owner::Page(1), line).unwrap();
    let instance = doc.shape(&at).unwrap();
    assert!(instance.sheet.cell("EndArrow").is_none() && instance.sheet.cell("BeginArrow").is_none(), "the ends are the master's");
    let master = doc.master(instance.master.unwrap()).unwrap().shapes.get(0).unwrap().clone();
    for (cell, row, value) in [("BeginArrow", "BeginCardinality", "29"), ("EndArrow", "EndCardinality", "25")] {
        let c = master.sheet.cell(cell).unwrap();
        assert!(
            matches!(&c.formula, FormulaSlot::Local(f) if f.source.contains(&format!("Prop.{row}")) && f.source.contains("STRSAME")),
            "{cell}: {:?}",
            c.formula
        );
        assert_eq!(c.value.as_ref().map(|v| v.text().to_owned()).as_deref(), Some(value), "{cell}: the cached value is what the formula gives");
    }
    // The route is right-angled, as a relationship between two tables is.
    assert_eq!(number(doc, line, "ShapeRouteStyle"), 1.0);
}

#[test]
fn a_changed_cardinality_survives_a_save_and_reopen_in_each_format() {
    // The words are Shape Data and the numbers are the cells a recalculation wrote for the instance: both come back, and the line is drawn with the same ends.
    for ext in ["vsdx", "diacraft"] {
        let dir = crate::stencil::flowchart_tests::Dir::new();
        let mut s = session();
        let line = put(&mut s, "crows-foot.relationship", (5.0, 5.0));
        set(&mut s, line, "BeginCardinality", "One or many");
        set(&mut s, line, "EndCardinality", "Zero or one");
        set(&mut s, line, "Identifying", "0");
        let path = dir.path(&format!("x.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut again = Session::new();
        run(&mut again, "file.open", json!({ "path": path }));
        assert_eq!(ends_of(&again, line), (28.0, 30.0, 2.0), "{ext}");
        // And an edit of the reopened drawing still moves them: the formulas came back with the master.
        set(&mut again, line, "EndCardinality", "One or many");
        assert_eq!(ends_of(&again, line), (28.0, 28.0, 2.0), "{ext}: the ends follow the words after a reopen");
    }
}

// ---- the entity -------------------------------------------------------------------------------------------------------------------------

/// The members of the group `id`, in the order of the master's table: their `ID`s.
fn members(doc: &Document, id: u32) -> Vec<u32> {
    let at = doc.find_shape(Owner::Page(1), id).unwrap();
    doc.shape(&at).unwrap().children.iter().map(|m| m.id).collect()
}

/// The text a member of the group `id` shows before it is typed in: the master's own for the member at `place` of the master's table.
fn placeholder(doc: &Document, id: u32, place: usize) -> String {
    let at = doc.find_shape(Owner::Page(1), id).unwrap();
    let master = doc.master(doc.shape(&at).unwrap().master.unwrap()).unwrap();
    let member = master.shapes.get(0).unwrap().children.get(place).unwrap().clone();
    member
        .text
        .as_ref()
        .map(|t| t.parts.iter().filter_map(|p| if let diacraft_doc::TextPart::Chars(c) = p { Some(c.to_string()) } else { None }).collect())
        .unwrap_or_default()
}

#[test]
fn an_entity_is_a_header_over_its_attributes_and_its_name_is_the_member_typing_goes_to() {
    for (id, compartments, least) in [("crows-foot.entity", 2, 1.2), ("crows-foot.entity-with-keys", 3, 1.5)] {
        let mut s = session();
        let entity = put(&mut s, id, (5.0, 5.0));
        let doc = s.document().unwrap();
        let ids = members(doc, entity);
        assert_eq!(ids.len(), compartments, "{id}: a compartment for each part");
        // The last member of the table is the header: typing on the selected group goes there, and the group picks itself first (SelectMode 1).
        assert_eq!(
            (number(doc, entity, "IsTextEditTarget"), number(doc, entity, "SelectMode"), number(doc, entity, "LockHeight")),
            (0.0, 1.0, 1.0),
            "{id}"
        );
        let tops: Vec<f64> = ids.iter().map(|m| number(doc, *m, "PinY") + number(doc, *m, "Height") / 2.0).collect();
        let top = number(doc, entity, "Height");
        let header = *ids.last().unwrap();
        assert!(
            (number(doc, header, "PinY") + number(doc, header, "Height") / 2.0 - top).abs() < 1e-6,
            "{id}: the last member is the top compartment: {tops:?} of {top}"
        );
        let total: f64 = ids.iter().map(|m| number(doc, *m, "Height")).sum();
        assert!((total - number(doc, entity, "Height")).abs() < 1e-6, "{id}: the group is as tall as its compartments");
        assert!(
            (number(doc, entity, "Height") - least).abs() < 0.02,
            "{id}: the master's height is the sum of the compartments' least: {}",
            number(doc, entity, "Height")
        );
        // An instance's member holds no text of its own until it is typed in: what shows is its master's, `Entity` for the header.
        assert_eq!(crate::draw::text_view(doc, 1, header).unwrap().chars, "", "{id}: nothing typed yet");
        assert_eq!(placeholder(doc, entity, ids.len() - 1), "Entity", "{id}: the header starts as `Entity`");
    }
    // The keys compartment starts as `PK  id` and the attributes as a line of `attribute : type`.
    let mut s = session();
    let entity = put(&mut s, "crows-foot.entity-with-keys", (5.0, 5.0));
    let doc = s.document().unwrap();
    assert_eq!((placeholder(doc, entity, 1).as_str(), placeholder(doc, entity, 0).as_str()), ("PK  id", "attribute : type"));
}

#[test]
fn an_entity_grows_with_its_attribute_lines_and_its_width_resizes_alone() {
    // The compartment is as tall as its text, the group as tall as its compartments, and `LockHeight` makes a resize change the width only. Never an absolute height: a line more is taller.
    let mut s = session();
    let (one, many) = (put(&mut s, "crows-foot.entity", (3.0, 6.0)), put(&mut s, "crows-foot.entity", (8.0, 6.0)));
    let attributes = |s: &Session, id: u32| members(s.document().unwrap(), id)[0];
    let (a1, a2) = (attributes(&s, one), attributes(&s, many));
    run(&mut s, "shape.set_text", json!({ "id": a1, "text": "id : integer" }));
    let lines: Vec<String> = (0..12).map(|k| format!("column{k} : text")).collect();
    run(&mut s, "shape.set_text", json!({ "id": a2, "text": lines.join("\n") }));
    let doc = s.document().unwrap();
    let (h1, h2) = (number(doc, one, "Height"), number(doc, many, "Height"));
    assert!(h2 > h1 + 0.8, "twelve lines make the entity taller than one: {h1} and {h2}");
    assert!((number(doc, a2, "Height") + number(doc, members(doc, many)[1], "Height") - h2).abs() < 1e-6, "the two compartments are all of it");
    // A resize of the width keeps the height that the text gives.
    let frame = crate::draw::shape_frame(doc, 1, many).unwrap();
    run(&mut s, "shape.resize", json!({ "id": many, "x": frame.pin_x - 2.0, "y": frame.pin_y - frame.height / 2.0, "width": 4.0, "height": 0.5 }));
    let doc = s.document().unwrap();
    let after = crate::draw::shape_frame(doc, 1, many).unwrap();
    assert!((after.width - 4.0).abs() < 1e-6, "the width is the one asked: {}", after.width);
    assert!((after.height - h2).abs() < 1e-6, "the height is the text's, not the 0.5 asked: {} and {h2}", after.height);
    // One undo takes the resize back (the width) and leaves the text and its height; the next takes the lines back, and the height with them in the same step.
    run(&mut s, "edit.undo", json!({}));
    let doc = s.document().unwrap();
    assert!(
        (number(doc, many, "Width") - 2.0).abs() < 1e-6 && (number(doc, many, "Height") - h2).abs() < 1e-6,
        "the resize is undone and nothing else"
    );
    run(&mut s, "edit.undo", json!({}));
    let doc = s.document().unwrap();
    assert!((number(doc, many, "Height") - h1).abs() < 0.05, "the lines and their height go together: {} against {h1}", number(doc, many, "Height"));
}

#[test]
fn the_headers_band_is_the_accent_tint_and_follows_the_theme_and_has_its_default_with_none() {
    // The design's section 4.7: `MSOTINT(THEMEVAL("AccentColor",RGB(58,127,192)),70)`. With no theme its default shows, and under a theme the theme's accent does.
    let colour = |s: &Session, id: u32| {
        let doc = s.document().unwrap();
        let at = doc.find_shape(Owner::Page(1), id).unwrap();
        let found = Resolver::new(doc).resolve(&SheetRef::Shape(at), &CellAddr::direct("FillForegnd")).unwrap();
        found.value.cell.and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()).unwrap_or_default()
    };
    let is_colour = |t: &str| t.len() == 7 && t.starts_with('#') && t[1..].bytes().all(|b| b.is_ascii_hexdigit());
    for id in ["crows-foot.entity", "crows-foot.entity-with-keys"] {
        let mut s = session();
        let entity = put(&mut s, id, (5.0, 5.0));
        let ids = members(s.document().unwrap(), entity);
        let (header, body) = (*ids.last().unwrap(), ids[0]);
        let default = colour(&s, header);
        assert!(is_colour(&default), "{id}: the default reads a colour: `{default}`");
        assert_ne!(default, colour(&s, body), "{id}: the band is tinted and the attributes are the style's");
        let themes: Vec<String> = run(&mut s, "theme.list", json!({}))["themes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["id"].as_str().map(str::to_owned))
            .take(3)
            .collect();
        assert_eq!(themes.len(), 3);
        let mut moved = false;
        for theme in &themes {
            run(&mut s, "theme.apply", json!({ "theme": theme }));
            let under = colour(&s, header);
            assert!(is_colour(&under), "{id}: under {theme}: `{under}`");
            moved |= under != default;
            let mut doc = s.document().unwrap().clone();
            let report = diacraft_calc::commit(&mut doc, &diacraft_calc::Budget::default(), diacraft_calc::EvalOptions::default());
            assert_eq!((report.written, report.refused), (0, 0), "{id}: under {theme} the cached values are what the formulas give");
        }
        assert!(moved, "{id}: a theme's accent changes the band");
        run(&mut s, "theme.apply", json!({ "theme": "none" }));
        assert_eq!(colour(&s, header), default, "{id}: with no theme again, the default again");
    }
}

// ---- the note and the lines -------------------------------------------------------------------------------------------------------------

#[test]
fn the_note_is_the_kits_dog_eared_note_and_the_note_link_is_a_straight_dash_with_no_ends() {
    let note = master("crows-foot.note");
    assert!(!note.is_connector() && !note.is_group());
    let mut s = session();
    let id = put(&mut s, note.id, (5.0, 5.0));
    let doc = s.document().unwrap();
    let at = doc.find_shape(Owner::Page(1), id).unwrap();
    let mastered = doc.master(doc.shape(&at).unwrap().master.unwrap()).unwrap().shapes.get(0).unwrap().clone();
    let sections = mastered.sheet.sections.iter().filter(|sec| sec.name.as_str() == "Geometry").count();
    assert_eq!(sections, 2, "the note's body and its fold");
    let link = master("crows-foot.note-link");
    assert_eq!(link.look().map(|l| (l.numbers(), l.route)), Some(((0, 0, 2), super::super::Route::Straight)));
    let mut s = session();
    let id = put(&mut s, link.id, (5.0, 5.0));
    assert_eq!(number(s.document().unwrap(), id, "ShapeRouteStyle"), 2.0, "straight");
}

// ---- the Quick Shapes, the search --------------------------------------------------------------------------------------------------------

fn quick(s: &mut Session, stencil: &str) -> Vec<String> {
    let answer = run(s, "stencil.quick", json!({ "stencil": stencil }));
    answer["stencils"][0]["masters"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn the_quick_shapes_are_entity_entity_with_keys_and_note() {
    let mut s = session();
    assert_eq!(quick(&mut s, "crows-foot"), ["crows-foot.entity", "crows-foot.entity-with-keys", "crows-foot.note"]);
}

fn found(s: &mut Session, text: &str) -> Vec<String> {
    let answer = run(s, "stencil.search", json!({ "text": text }));
    answer["results"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|g| g["masters"].as_array().unwrap().iter())
        .map(|m| m["id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn every_master_has_search_words_and_the_inventorys_names_find_their_masters() {
    for m in &MASTERS {
        assert!(!crate::stencil::keywords::of(m.id).is_empty(), "{} has no search words", m.id);
    }
    let mut s = session();
    // C77's names: Entity, Attribute, Relationship, Primary Key Attribute, Primary Key Separator (and the cardinalities' own).
    for (name, id) in [
        ("Entity", "crows-foot.entity"),
        ("Attribute", "crows-foot.entity"),
        ("Relationship", "crows-foot.relationship"),
        ("Primary Key Attribute", "crows-foot.entity-with-keys"),
        ("Primary Key Separator", "crows-foot.entity-with-keys"),
        ("Cardinality", "crows-foot.relationship"),
        ("Crow's foot", "crows-foot.relationship"),
    ] {
        let hits = found(&mut s, name);
        assert!(hits.iter().any(|h| h == id), "`{name}` does not find {id}: {hits:?}");
    }
}

#[test]
fn no_master_or_search_word_is_a_vendors_or_a_products_name() {
    const VENDORS: [&str; 11] =
        ["erwin", "microsoft", "oracle", "mysql", "postgres", "sqlserver", "powerdesigner", "lucidchart", "dbdesigner", "ibm", "sap"];
    assert_eq!(MASTERS.len(), 8, "a loop over nothing proves nothing");
    for m in &MASTERS {
        let text = format!("{} {} {}", m.id, m.name, crate::stencil::keywords::of(m.id)).to_ascii_lowercase();
        for word in text.split(|c: char| !c.is_ascii_alphanumeric()) {
            assert!(!VENDORS.contains(&word), "{}: `{word}` is a vendor's or a product's name", m.id);
        }
    }
}

// ---- what the ends look like -------------------------------------------------------------------------------------------------------------

const PPI: f64 = 200.0;

fn render(s: &Session) -> diacraft_render::Rendered {
    let doc = s.document().unwrap();
    let view = diacraft_render::View::fit_page(doc, 1, PPI).unwrap();
    let mut renderer = diacraft_render::Renderer::new();
    renderer.set_paper(None);
    renderer.render(doc, 1, &view)
}

/// The pixels of the frame within `x0..x1` inches from the page's left edge, all rows: the part of the picture a mark at one end of the line is in.
fn columns(r: &diacraft_render::Rendered, x0: f64, x1: f64) -> Vec<u8> {
    let (c0, c1) = ((x0 * PPI) as usize, (x1 * PPI) as usize);
    let width = r.width as usize;
    let mut out = Vec::new();
    for row in 0..r.height as usize {
        let from = (row * width + c0) * 4;
        let to = (row * width + c1) * 4;
        out.extend_from_slice(&r.rgba_premul[from..to]);
    }
    out
}

#[test]
fn the_ends_are_drawn_and_a_changed_word_changes_the_drawing_at_its_end_only() {
    let mut s = session();
    let line = put(&mut s, "crows-foot.relationship", (4.0, 5.0));
    let before = render(&s);
    assert!(before.skipped.is_empty(), "{:?}", before.skipped);
    // The line runs from 3.25 to 4.75 inches. A mark is ink beyond the line's own weight, beside the line, within a third of an inch of its end.
    let beside = |r: &diacraft_render::Rendered, x: f64| {
        let height = f64::from(r.height) / PPI;
        (3..=14).any(|k| {
            let dy = f64::from(k) / 100.0;
            [-dy, dy].iter().any(|d| {
                let (col, row) = ((x * PPI) as usize, ((height - (5.0 + d)) * PPI) as usize);
                r.rgba_premul[(row * r.width as usize + col) * 4 + 3] > 128
            })
        })
    };
    assert!((0..30).any(|k| beside(&before, 3.25 + f64::from(k) / 100.0)), "a fork and a disc at the begin");
    assert!((0..30).any(|k| beside(&before, 4.75 - f64::from(k) / 100.0)), "two bars at the end");
    assert!(!beside(&before, 4.0), "and nothing beside the line in the middle");
    set(&mut s, line, "EndCardinality", "Zero or one");
    let after = render(&s);
    assert_ne!(columns(&before, 4.40, 4.80), columns(&after, 4.40, 4.80), "the end now shows a disc");
    assert_eq!(columns(&before, 3.20, 3.60), columns(&after, 3.20, 3.60), "and the begin is as it was");
}
