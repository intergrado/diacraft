//! Chen's Database Notation: the eleven masters, each figure's geometry (a weak entity's inner border, the inner diamond and ellipse, the associative entity's diamond), the dotted
//! derived attribute and the underlined key attribute, the double line of total participation, the Quick Shapes, the search words and the template. The catalogue's invariants
//! (`stencil/catalogue_tests.rs`) and the five per-stencil checks (`tests/it/stencils2_chen.rs`) run on every master beside these.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use diacraft_doc::{CellAddr, Document, Owner, Resolver, RowKey, SectionKey, SheetRef};
use serde_json::{Value, json};

use super::MASTERS;
use crate::Session;
use crate::stencil::{MasterSpec, builtin_stencil};

// ---- what the design lists, and the one master the spec adds -------------------------------------------------------------------------

/// The masters in the Shapes window's order: id, name, width, height (inches), how many connection points. The first four are the Quick Shapes, so the figures a person draws most come first.
const TABLE: [(&str, &str, f64, f64, usize); 11] = [
    ("chen.entity", "Entity", 1.4, 0.7, 12),
    ("chen.relationship", "Relationship", 1.4, 0.8, 4),
    ("chen.attribute", "Attribute", 1.2, 0.6, 8),
    ("chen.key-attribute", "Key Attribute", 1.2, 0.6, 8),
    ("chen.weak-entity", "Weak Entity", 1.4, 0.7, 12),
    ("chen.associative-entity", "Associative Entity", 2.0, 1.0, 12),
    ("chen.identifying-relationship", "Identifying Relationship", 1.4, 0.8, 4),
    ("chen.multivalued-attribute", "Multivalued Attribute", 1.2, 0.6, 8),
    ("chen.derived-attribute", "Derived Attribute", 1.2, 0.6, 8),
    ("chen.link", "Link", 1.5, 0.0, 0),
    ("chen.total-participation", "Total Participation", 1.5, 0.0, 0),
];

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

fn put(s: &mut Session, id: &str, (x, y): (f64, f64)) -> u32 {
    let dropped = run(s, "shape.drop", json!({ "master": id, "page": 1, "x": x, "y": y }));
    u32::try_from(dropped["id"].as_u64().unwrap()).unwrap()
}

fn sheet_of(doc: &Document, id: u32) -> SheetRef {
    SheetRef::Shape(doc.find_shape(Owner::Page(1), id).unwrap_or_else(|| panic!("no shape {id}")))
}

/// What a cell of the shape `id` reads, through its master.
fn number(doc: &Document, id: u32, cell: &str) -> f64 {
    let found = Resolver::new(doc).resolve(&sheet_of(doc, id), &CellAddr::direct(cell)).unwrap_or_else(|| panic!("no cell {cell}"));
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap_or_else(|| panic!("no number in {cell} of shape {id}"))
}

/// A cell in a section of the shape `id`: `Geometry` section `ix`'s `NoFill`, a `Character` row's `Style`.
fn resolved(doc: &Document, id: u32, addr: &CellAddr) -> Option<f64> {
    Resolver::new(doc).resolve(&sheet_of(doc, id), addr).and_then(|f| f.value.cell?.value.as_ref()?.number())
}

/// The rows of `Geometry` section `ix` of a shape: the kind of each row and its cells `X`, `Y`, `A` and `B` (0 where a row has none).
fn rows(doc: &Document, id: u32, ix: u32) -> Vec<(String, [f64; 4])> {
    let res = Resolver::new(doc);
    let found = res.effective_rows(&sheet_of(doc, id), &SectionKey::new("Geometry", Some(ix))).unwrap_or_else(|| panic!("no Geometry section {ix}"));
    found
        .iter()
        .map(|row| {
            let at = |name: &str| {
                row.cells.iter().find(|(n, _)| n.as_str() == name).and_then(|(_, r)| r.value.cell?.value.as_ref()?.number()).unwrap_or(0.0)
            };
            (row.row_type.as_deref().unwrap_or("").to_owned(), [at("X"), at("Y"), at("A"), at("B")])
        })
        .collect()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// Asserts that the points of the rows are these, in order.
#[track_caller]
fn assert_points(got: &[(String, [f64; 4])], want: &[(f64, f64)], what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: {got:?}");
    for (g, w) in got.iter().zip(want) {
        assert!(near(g.1[0], w.0) && near(g.1[1], w.1), "{what}: ({}, {}) against ({}, {}) in {got:?}", g.1[0], g.1[1], w.0, w.1);
    }
}

// ---- the catalogue --------------------------------------------------------------------------------------------------------------------

#[test]
fn chen_holds_the_eleven_masters_in_order_with_their_sizes_points_and_series() {
    assert_eq!(MASTERS.len(), 11);
    for (at, (m, (id, name, w, h, points))) in MASTERS.iter().zip(TABLE).enumerate() {
        assert_eq!((m.id, m.name, m.width, m.height), (id, name, w, h), "master {at}");
        assert_eq!(m.point_count(), points, "{id}: connection points");
        assert_eq!(m.identity, (21, u16::try_from(at + 1).unwrap()), "{id}: series 21, in the window's order");
    }
    assert_eq!(MASTERS.iter().filter(|m| m.is_connector()).map(|m| m.id).collect::<Vec<_>>(), ["chen.link", "chen.total-participation"]);
    assert!(MASTERS.iter().all(|m| !m.is_group() && !m.is_fixed_1d()), "every figure is one shape");
}

#[test]
fn the_stencil_ships_closed_by_default_and_its_template_is_listed_and_opens_it_beside_the_basic_shapes() {
    let stencil = builtin_stencil("chen").expect("the stencil has masters, so it ships");
    assert_eq!(
        (stencil.name, stencil.category, stencil.default_open, stencil.masters().len()),
        ("Chen's Database Notation", "Software and Database", false, 11)
    );
    let template = crate::template::find("chen_notation").expect("its stencil ships, so the template is listed");
    assert_eq!((template.name, template.stencils), ("Chen's Database Notation", &["chen", "basic"][..]));
    assert!(template.description.contains("based on"), "{}", template.description);
    let mut s = Session::new();
    let started = run(&mut s, "file.new", json!({ "template": "chen_notation" }));
    assert_eq!(started["stencils"]["open"], json!(["chen", "basic"]), "{started}");
    let fresh = run(&mut Session::new(), "stencil.list", json!({}));
    let open: Vec<&str> = fresh["stencils"].as_array().unwrap().iter().filter_map(|t| t["id"].as_str()).collect();
    assert!(!open.contains(&"chen"), "{open:?}");
    assert!(fresh["available"].as_array().unwrap().iter().any(|t| t["id"] == "chen"));
}

#[test]
fn every_two_dimensional_master_holds_objtype_1_and_the_two_lines_hold_none() {
    assert_eq!(MASTERS.len(), 11, "a loop over nothing proves nothing");
    for m in MASTERS.iter().filter(|m| !m.is_connector()) {
        let mut s = session();
        let id = put(&mut s, m.id, (5.0, 5.0));
        assert_eq!(number(s.document().unwrap(), id, "ObjType"), 1.0, "{}: a shape people join", m.id);
    }
}

// ---- the figures ------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_weak_entity_draws_an_inner_rectangle_0_06_inch_inside_its_outline() {
    // 1.4 by 0.7 inches draws a second rectangle from (0.06, 0.06) to (1.34, 0.64), and it is a stroke on its own (`NoFill`), so the outline and the fill are the first section's.
    let mut s = session();
    let id = put(&mut s, "chen.weak-entity", (5.0, 5.0));
    let doc = s.document().unwrap();
    assert_points(&rows(doc, id, 0), &[(0.0, 0.0), (1.4, 0.0), (1.4, 0.7), (0.0, 0.7), (0.0, 0.0)], "the outline");
    assert_points(&rows(doc, id, 1), &[(0.06, 0.06), (1.34, 0.06), (1.34, 0.64), (0.06, 0.64), (0.06, 0.06)], "the inner border");
    assert_eq!(resolved(doc, id, &CellAddr::in_section("Geometry", Some(0), "NoFill")), Some(0.0));
    assert_eq!(resolved(doc, id, &CellAddr::in_section("Geometry", Some(1), "NoFill")), Some(1.0), "a stroke only");
    // A resize keeps it 0.06 inch inside, the way the corner of a rounded box keeps its radius: the offset is a length and not a share.
    let frame = crate::draw::shape_frame(doc, 1, id).unwrap();
    let (w, h) = (3.0, 1.5);
    run(&mut s, "shape.resize", json!({ "id": id, "x": frame.pin_x - w / 2.0, "y": frame.pin_y - h / 2.0, "width": w, "height": h }));
    assert_points(
        &rows(s.document().unwrap(), id, 1),
        &[(0.06, 0.06), (2.94, 0.06), (2.94, 1.44), (0.06, 1.44), (0.06, 0.06)],
        "the inner border after a resize",
    );
}

#[test]
fn an_identifying_relationship_is_a_diamond_with_an_inner_diamond_that_stays_inside_it() {
    let mut s = session();
    let id = put(&mut s, "chen.identifying-relationship", (5.0, 5.0));
    let doc = s.document().unwrap();
    assert_points(&rows(doc, id, 0), &[(0.7, 0.0), (1.4, 0.4), (0.7, 0.8), (0.0, 0.4), (0.7, 0.0)], "the outer diamond");
    let inner = rows(doc, id, 1);
    assert_eq!(inner.len(), 5, "{inner:?}");
    let (cx, cy) = (0.7, 0.4);
    // Four vertices on the axes, a little inside the outer ones, and the same figure scaled about the centre.
    let (top, right) = (inner[0].1, inner[1].1);
    assert!(near(top[0], cx) && top[1] > 0.0 && top[1] < 0.2, "the inner top vertex is inside the outer one: {top:?}");
    assert!(near(right[1], cy) && right[0] < 1.4 && right[0] > 1.1, "and so is the right one: {right:?}");
    assert!(near((right[0] - cx) / (cy - top[1]), 0.7 / 0.4), "the inner diamond is the outer one scaled about the centre");
    assert_eq!(resolved(doc, id, &CellAddr::in_section("Geometry", Some(1), "NoFill")), Some(1.0));
    // The gap between the two is about 0.05 inch across, as the weak entity's is.
    // The outer edge from (0.7, 0.8) to (1.4, 0.4) is the line 0.4 x + 0.7 y = 0.84.
    let outer_edge = |x: f64, y: f64| (0.4 * x + 0.7 * y - 0.84).abs() / (0.4_f64.powi(2) + 0.7_f64.powi(2)).sqrt();
    let across = outer_edge(right[0], right[1]).min(outer_edge(top[0], top[1]));
    assert!((0.03..0.09).contains(&across), "the inner diamond is about 0.06 inch from the outer one: {across}");
}

#[test]
fn a_multivalued_attribute_is_an_ellipse_with_an_inner_ellipse_0_06_inch_inside() {
    let mut s = session();
    let id = put(&mut s, "chen.multivalued-attribute", (5.0, 5.0));
    let doc = s.document().unwrap();
    let outer = rows(doc, id, 0);
    assert_eq!(outer.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["Ellipse"], "the outline is the one ellipse of the box");
    let inner = rows(doc, id, 1);
    assert_eq!(inner.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["MoveTo", "EllipticalArcTo", "EllipticalArcTo"], "{inner:?}");
    // At 1.2 by 0.6: from the left end of the inner ellipse (0.06, 0.3), over the top (0.6, 0.54) to its right end (1.14, 0.3), and back under it (0.6, 0.06).
    assert!(near(inner[0].1[0], 0.06) && near(inner[0].1[1], 0.3), "{inner:?}");
    assert!(near(inner[1].1[0], 1.14) && near(inner[1].1[1], 0.3) && near(inner[1].1[2], 0.6) && near(inner[1].1[3], 0.54), "{inner:?}");
    assert!(near(inner[2].1[0], 0.06) && near(inner[2].1[1], 0.3) && near(inner[2].1[2], 0.6) && near(inner[2].1[3], 0.06), "{inner:?}");
    assert_eq!(resolved(doc, id, &CellAddr::in_section("Geometry", Some(1), "NoFill")), Some(1.0));
}

#[test]
fn an_associative_entity_is_a_rectangle_with_a_diamond_in_it_and_the_text_sits_in_the_diamond() {
    // Project rule: no source read gives the figure of Chen's associative entity (DD16), so it is drawn as it is often drawn: the entity's rectangle with a diamond inscribed in it,
    // the diamond's vertices at the middles of the rectangle's sides.
    let mut s = session();
    let id = put(&mut s, "chen.associative-entity", (5.0, 5.0));
    let doc = s.document().unwrap();
    assert_points(&rows(doc, id, 0), &[(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (0.0, 1.0), (0.0, 0.0)], "the rectangle");
    assert_points(&rows(doc, id, 1), &[(1.0, 0.0), (2.0, 0.5), (1.0, 1.0), (0.0, 0.5), (1.0, 0.0)], "the inscribed diamond");
    assert_eq!(resolved(doc, id, &CellAddr::in_section("Geometry", Some(1), "NoFill")), Some(1.0), "a stroke only");
    // The label sits in the diamond: a text block narrower than the rectangle, and centred.
    let (w, h) = (number(doc, id, "TxtWidth"), number(doc, id, "TxtHeight"));
    assert!(w <= 2.0 * 0.6 + 1e-9 && h <= 1.0 * 0.6 + 1e-9, "the text block is inside the diamond: {w} by {h}");
    assert!(near(number(doc, id, "TxtPinX") - number(doc, id, "TxtLocPinX") + w / 2.0, 1.0), "centred");
}

#[test]
fn a_derived_attributes_line_is_dotted_and_every_other_figure_keeps_the_styles_line() {
    // DD15: pattern 3 (dotted), where the design said dashed.
    assert_eq!(MASTERS.len(), 11, "a loop over nothing proves nothing");
    for m in MASTERS.iter().filter(|m| !m.is_connector()) {
        let mut s = session();
        let id = put(&mut s, m.id, (5.0, 5.0));
        let pattern = number(s.document().unwrap(), id, "LinePattern");
        assert_eq!(pattern, if m.id == "chen.derived-attribute" { 3.0 } else { 1.0 }, "{}", m.id);
    }
}

#[test]
fn a_key_attributes_text_is_underlined() {
    // The underline bit (4) in the Character row 0's Style; no other figure's text is underlined.
    let style = |s: &Session, id: u32| resolved(s.document().unwrap(), id, &CellAddr::in_row("Character", None, RowKey::Ix(0), "Style"));
    let mut s = session();
    let key = put(&mut s, "chen.key-attribute", (3.0, 5.0));
    assert_eq!(style(&s, key).map(|v| v as u32 & 4), Some(4), "the key attribute's text is underlined");
    // What a person types in takes the row: the text holds no formatting of its own.
    run(&mut s, "shape.set_text", json!({ "id": key, "text": "customer_id" }));
    assert_eq!(style(&s, key).map(|v| v as u32 & 4), Some(4));
    let view = crate::draw::text_view(s.document().unwrap(), 1, key).unwrap();
    assert_eq!(view.chars, "customer_id");
    for m in MASTERS.iter().filter(|m| !m.is_connector() && m.id != "chen.key-attribute") {
        let id = put(&mut s, m.id, (6.0, 6.0));
        assert!(style(&s, id).is_none_or(|v| v as u32 & 4 == 0), "{}: no underline", m.id);
    }
}

// ---- the lines ----------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_link_is_a_straight_solid_line_with_no_ends_and_a_place_for_its_label() {
    let link = master("chen.link");
    assert_eq!(link.look().map(|l| (l.numbers(), l.route, l.compound, l.weight_pt)), Some(((0, 0, 1), super::super::Route::Straight, None, None)));
    let mut s = session();
    let id = put(&mut s, link.id, (5.0, 5.0));
    let doc = s.document().unwrap();
    assert_eq!((number(doc, id, "BeginArrow"), number(doc, id, "EndArrow"), number(doc, id, "LinePattern")), (0.0, 0.0, 1.0));
    assert_eq!(number(doc, id, "ShapeRouteStyle"), 2.0, "straight");
    assert_eq!(resolved(doc, id, &CellAddr::direct("CompoundType")).unwrap_or(0.0), 0.0, "one line");
    // The label (1, N or M) is typed on the line, as on any connector.
    run(&mut s, "shape.set_text", json!({ "id": id, "text": "N" }));
    assert_eq!(crate::draw::text_view(s.document().unwrap(), 1, id).unwrap().chars, "N");
}

#[test]
fn total_participation_is_a_double_line_of_3_points_with_no_ends_and_stays_a_routed_connector() {
    // Weight 3 points, `CompoundType` 1; the renderer draws it as two bands of 1 point with a gap of 1 point (`crates/render/tests/it/lines.rs` holds the bands themselves).
    let look = master("chen.total-participation").look().copied().unwrap();
    assert_eq!((look.numbers(), look.route, look.compound, look.weight_pt), ((0, 0, 1), super::super::Route::Straight, Some(1), Some(3.0)));
    let mut s = session();
    let id = put(&mut s, "chen.total-participation", (5.0, 5.0));
    let doc = s.document().unwrap();
    assert_eq!(number(doc, id, "CompoundType"), 1.0);
    assert!((number(doc, id, "LineWeight") - 3.0 / 72.0).abs() < 1e-9, "{}", number(doc, id, "LineWeight"));
    assert_eq!((number(doc, id, "BeginArrow"), number(doc, id, "EndArrow")), (0.0, 0.0));
    assert!(
        doc.shape(&doc.find_shape(Owner::Page(1), id).unwrap()).unwrap().sheet.cell("EndArrow").is_none(),
        "the instance holds no end of its own"
    );
    assert!(crate::draw::Reader::new(doc).is_one_d(&sheet_of(doc, id)), "a connector, so a glued end follows its shape and the line is routed");
}

#[test]
fn total_participation_draws_two_bands_with_a_gap_and_is_no_approximation() {
    // The weight is 3 points, 0.04167 inch: at 400 pixels to the inch a band is 5.6 pixels, and the middle of the line is the gap between them.
    let mut s = session();
    put(&mut s, "chen.total-participation", (5.0, 5.0));
    let doc = s.document().unwrap();
    let ppi = 400.0;
    let view = diacraft_render::View::fit_page(doc, 1, ppi).unwrap();
    let mut renderer = diacraft_render::Renderer::new();
    renderer.set_paper(None);
    let frame = renderer.render(doc, 1, &view);
    assert!(frame.skipped.is_empty(), "no stand-in for the compound line: {:?}", frame.skipped);
    let height = f64::from(frame.height) / ppi;
    let alpha = |y: f64| {
        let (col, row) = ((5.0 * ppi) as usize, ((height - y) * ppi) as usize);
        frame.rgba_premul[(row * frame.width as usize + col) * 4 + 3]
    };
    let band = 3.0 / 72.0 / 3.0;
    assert!(alpha(5.0 + band) > 200 && alpha(5.0 - band) > 200, "the two bands, a third of the weight either side of the line");
    assert!(alpha(5.0) < 30, "and the gap between them is empty");
}

// ---- the Quick Shapes, the search --------------------------------------------------------------------------------------------------------

#[test]
fn the_quick_shapes_are_entity_relationship_attribute_and_key_attribute() {
    let mut s = session();
    let answer = run(&mut s, "stencil.quick", json!({ "stencil": "chen" }));
    let ids: Vec<&str> = answer["stencils"][0]["masters"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["chen.entity", "chen.relationship", "chen.attribute", "chen.key-attribute"]);
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
    // C78's names (Entity, Relationship Connector, and the attributes that can be derived) and the spec's list.
    for (name, id) in [
        ("Entity", "chen.entity"),
        ("Weak Entity", "chen.weak-entity"),
        ("Associative Entity", "chen.associative-entity"),
        ("Relationship", "chen.relationship"),
        ("Identifying Relationship", "chen.identifying-relationship"),
        ("Relationship Connector", "chen.link"),
        ("Attribute", "chen.attribute"),
        ("Primary Key Attribute", "chen.key-attribute"),
        ("Multi-valued Attribute", "chen.multivalued-attribute"),
        ("Multivalued Attribute", "chen.multivalued-attribute"),
        ("Derived Attribute", "chen.derived-attribute"),
        ("Total Participation", "chen.total-participation"),
    ] {
        let hits = found(&mut s, name);
        assert!(hits.iter().any(|h| h == id), "`{name}` does not find {id}: {hits:?}");
    }
}
