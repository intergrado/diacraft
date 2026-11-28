//! The C4 Model stencil: the twelve masters, their text of Shape Data fields (`[Container: Rust]`), the accents that follow the theme and the grey of an external element, the relationship's
//! label, the title and the key that no connector joins, and the boundary and deployment node that hold what is dropped inside them. C4 is an extra of DiaCraft's with no parity row: its tests
//! are these, the catalogue's invariants (`stencil/catalogue_tests.rs`) and the five per-stencil checks (`tests/it/stencils2_c4.rs`), which run on every master beside them.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use diacraft_doc::{CellAddr, Document, Owner, Resolver, RowKey, SheetRef};
use serde_json::{Value, json};

use super::MASTERS;
use crate::stencil::{MasterSpec, builtin_stencil, catalogue_tests};
use crate::{Session, draw};

// ---- what the design lists, in the order the Shapes window shows (the first four are the Quick Shapes) ---------------------------------------------

/// The twelve masters: id, name, width, height (inches) and connection points. The design's table lists the database and the queue before the component, but the strip offers the first four
/// masters that are no connectors, and the design says those are the Person, the Software System, the Container and the Component: the component comes fourth.
const TABLE: [(&str, &str, f64, f64, usize); 12] = [
    ("c4.person", "Person", 1.4, 1.5, 4),
    ("c4.software-system", "Software System", 2.0, 1.3, 12),
    ("c4.container", "Container", 2.0, 1.3, 12),
    ("c4.component", "Component", 2.0, 1.2, 12),
    ("c4.database-container", "Database Container", 1.8, 1.4, 4),
    ("c4.queue-container", "Queue Container", 2.2, 0.9, 4),
    ("c4.boundary", "Boundary", 4.0, 3.0, 12),
    ("c4.deployment-node", "Deployment Node", 4.0, 3.0, 12),
    ("c4.infrastructure-node", "Infrastructure Node", 1.8, 1.0, 12),
    ("c4.title", "Diagram Title", 4.0, 0.6, 0),
    ("c4.key", "Key", 2.2, 2.4, 0),
    ("c4.relationship", "Relationship", 1.5, 0.0, 0),
];

/// The elements that carry an accent and can be external.
const ACCENTS: [&str; 6] = ["c4.person", "c4.software-system", "c4.container", "c4.component", "c4.database-container", "c4.queue-container"];

fn master(id: &str) -> &'static MasterSpec {
    MASTERS.iter().find(|m| m.id == id).unwrap_or_else(|| panic!("no master {id}"))
}

/// A session with a new drawing.
fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s
}

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command} {params}: {e}"))
}

/// Drops a built-in master of the stencil on page 1 at (x, y) and answers its `ID`.
fn put(s: &mut Session, id: &str, (x, y): (f64, f64)) -> u32 {
    let dropped = run(s, "shape.drop", json!({ "master": id, "stencil": "c4", "page": 1, "x": x, "y": y }));
    u32::try_from(dropped["id"].as_u64().unwrap()).unwrap()
}

/// What the fields of a shape's text show, in order, and the characters between them.
fn label(s: &Session, id: u32) -> (Vec<String>, String) {
    let view = draw::text_view(s.document().unwrap(), 1, id).unwrap();
    (view.fields.into_iter().map(|f| f.text).collect(), view.chars)
}

fn fields(s: &Session, id: u32) -> Vec<String> {
    label(s, id).0
}

fn set(s: &mut Session, id: u32, row: &str, value: &str) {
    run(s, "shapedata.set", json!({ "id": id, "row": row, "value": value }));
}

/// The text a cell of the shape reads through its master.
fn cell_text(doc: &Document, page: u32, id: u32, cell: &str) -> Option<String> {
    let sheet = SheetRef::Shape(doc.find_shape(Owner::Page(page), id)?);
    Resolver::new(doc).resolve(&sheet, &CellAddr::direct(cell))?.value.cell?.value.as_ref().map(|v| v.text().to_owned())
}

fn number(doc: &Document, id: u32, cell: &str) -> Option<f64> {
    let sheet = SheetRef::Shape(doc.find_shape(Owner::Page(1), id)?);
    Resolver::new(doc).resolve(&sheet, &CellAddr::direct(cell))?.value.cell?.value.as_ref()?.number()
}

/// The fill of a shape as `#RRGGBB`.
fn fill(s: &Session, id: u32) -> String {
    cell_text(s.document().unwrap(), 1, id, "FillForegnd").unwrap_or_else(|| panic!("shape {id} has no fill"))
}

fn rgb(hex: &str) -> (u8, u8, u8) {
    assert!(hex.len() == 7 && hex.starts_with('#'), "`{hex}` is no colour");
    let part = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).unwrap();
    (part(1), part(3), part(5))
}

/// How light a colour is, 0 to 255.
fn lightness(hex: &str) -> f64 {
    let (r, g, b) = rgb(hex);
    0.299 * f64::from(r) + 0.587 * f64::from(g) + 0.114 * f64::from(b)
}

/// The ids of the themes the catalogue has.
fn themes(s: &mut Session) -> Vec<String> {
    let listed = run(s, "theme.list", json!({}));
    listed["themes"].as_array().unwrap().iter().filter_map(|t| t["id"].as_str().map(str::to_owned)).collect()
}

// ---- the catalogue --------------------------------------------------------------------------------------------------------------------

#[test]
fn the_stencil_ships_with_the_twelve_masters_of_the_design_closed_by_default() {
    let stencil = builtin_stencil("c4").expect("c4 is shipped");
    assert_eq!((stencil.name, stencil.category, stencil.default_open, stencil.masters().len()), ("C4 Model", "Software and Database", false, 12));
    assert_eq!(MASTERS.len(), 12);
    for (at, (m, (id, name, w, h, points))) in MASTERS.iter().zip(TABLE).enumerate() {
        assert_eq!((m.id, m.name, m.width, m.height), (id, name, w, h), "master {at}");
        assert_eq!(m.point_count(), points, "{id}: connection points");
        assert_eq!(m.identity, (16, u16::try_from(at + 1).unwrap()), "{id}: series 16, in the window's order");
    }
    // Only the relationship is a line.
    assert_eq!(MASTERS.iter().filter(|m| m.is_connector()).map(|m| m.id).collect::<Vec<_>>(), ["c4.relationship"]);
    let listed = crate::stencil::list();
    let open: Vec<&str> = listed["stencils"].as_array().unwrap().iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(!open.contains(&"c4"), "a new session does not open it: {open:?}");
    let available: Vec<&str> = listed["available"].as_array().unwrap().iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(available.contains(&"c4"), "More Shapes lists it: {available:?}");
}

#[test]
fn every_master_holds_the_shape_data_rows_the_design_gives_it() {
    let rows = |id: &str| -> Vec<String> {
        let mut s = session();
        let shape = put(&mut s, id, (5.0, 5.0));
        let listed = run(&mut s, "shapedata.list", json!({ "id": shape, "hidden": true }));
        listed["fields"].as_array().unwrap().iter().map(|f| f["row"].as_str().unwrap().to_owned()).collect()
    };
    // The people and the software systems have no technology (the type line is their kind alone).
    assert_eq!(rows("c4.person"), ["Name", "Description", "External"]);
    assert_eq!(rows("c4.software-system"), ["Name", "Description", "External"]);
    for id in ["c4.container", "c4.database-container", "c4.queue-container", "c4.component"] {
        assert_eq!(rows(id), ["Name", "Technology", "Description", "External"], "{id}");
    }
    assert_eq!(rows("c4.infrastructure-node"), ["Name", "Technology", "Description"]);
    assert_eq!(rows("c4.boundary"), ["Name", "Kind"]);
    assert_eq!(rows("c4.deployment-node"), ["Name", "Technology", "Instances"]);
    assert_eq!(rows("c4.title"), ["Diagram", "Title", "Description"]);
    assert_eq!(rows("c4.relationship"), ["Description", "Technology"]);
    assert!(rows("c4.key").is_empty(), "the key holds static text");
}

#[test]
fn the_lists_hold_the_designs_items_and_external_is_a_truth_value_that_starts_false() {
    let mut s = session();
    let boundary = put(&mut s, "c4.boundary", (3.0, 5.0));
    let title = put(&mut s, "c4.title", (3.0, 9.0));
    let container = put(&mut s, "c4.container", (8.0, 5.0));
    let items = |s: &mut Session, id: u32, row: &str| -> Vec<String> {
        let listed = run(s, "shapedata.list", json!({ "id": id }));
        let field = listed["fields"].as_array().unwrap().iter().find(|f| f["row"] == row).unwrap().clone();
        field["items"].as_array().unwrap().iter().map(|i| i.as_str().unwrap().to_owned()).collect()
    };
    assert_eq!(items(&mut s, boundary, "Kind"), ["Enterprise", "Software System", "Container", "Group"]);
    assert_eq!(items(&mut s, title, "Diagram"), ["System Context", "Container", "Component", "Deployment", "Dynamic", "System Landscape"]);
    let listed = run(&mut s, "shapedata.list", json!({ "id": container }));
    let external = listed["fields"].as_array().unwrap().iter().find(|f| f["row"] == "External").unwrap().clone();
    assert_eq!(external["type_code"], json!(3), "a truth value: {external}");
    assert!(
        matches!(external["value"].as_str(), Some("FALSE" | "0")) || external["value"] == json!(false) || external["value"] == json!(0),
        "{external}"
    );
}

// ---- the text: name, the type line with its technology, and the description ------------------------------------------------------

#[test]
fn a_container_with_technology_rust_shows_container_rust_and_with_none_shows_container() {
    let mut s = session();
    let container = put(&mut s, "c4.container", (5.0, 5.0));
    // With no technology the type line is the kind alone, with no colon.
    assert_eq!(label(&s, container), (vec!["Name".to_owned(), "[Container]".to_owned(), "Description".to_owned()], "\n\n".to_owned()));
    set(&mut s, container, "Technology", "Rust");
    assert_eq!(fields(&s, container), ["Name", "[Container: Rust]", "Description"]);
    set(&mut s, container, "Name", "Order Service");
    set(&mut s, container, "Description", "Takes and prices orders");
    assert_eq!(fields(&s, container), ["Order Service", "[Container: Rust]", "Takes and prices orders"]);
    set(&mut s, container, "Technology", "");
    assert_eq!(fields(&s, container), ["Order Service", "[Container]", "Takes and prices orders"], "an empty technology leaves out the colon");
    // A technology that is blank is a technology: the colon returns with it.
    set(&mut s, container, "Technology", " ");
    assert_eq!(fields(&s, container)[1], "[Container:  ]");
    // The other kinds put theirs in the same place.
    let kinds = [
        ("c4.component", "[Component]", "[Component: Axum]"),
        ("c4.database-container", "[Container]", "[Container: PostgreSQL]"),
        ("c4.queue-container", "[Container]", "[Container: Kafka]"),
        ("c4.infrastructure-node", "[Infrastructure Node]", "[Infrastructure Node: nginx]"),
    ];
    for (n, (id, bare, with)) in kinds.into_iter().enumerate() {
        let shape = put(&mut s, id, (1.5 + 2.0 * n as f64, 8.0));
        assert_eq!(fields(&s, shape)[1], bare, "{id}");
        set(&mut s, shape, "Technology", with.split(": ").nth(1).unwrap().trim_end_matches(']'));
        assert_eq!(fields(&s, shape)[1], with, "{id}");
    }
    // A person and a software system have no technology to show.
    let person = put(&mut s, "c4.person", (2.0, 2.0));
    let system = put(&mut s, "c4.software-system", (6.0, 2.0));
    assert_eq!(fields(&s, person), ["Name", "[Person]", "Description"]);
    assert_eq!(fields(&s, system), ["Name", "[Software System]", "Description"]);
}

#[test]
fn the_name_is_bold_at_11_points_the_type_line_8_and_the_description_9() {
    let mut s = session();
    for id in
        ["c4.person", "c4.software-system", "c4.container", "c4.component", "c4.database-container", "c4.queue-container", "c4.infrastructure-node"]
    {
        let shape = put(&mut s, id, (5.0, 5.0));
        let doc = s.document().unwrap();
        let sheet = SheetRef::Shape(doc.find_shape(Owner::Page(1), shape).unwrap());
        let res = Resolver::new(doc);
        let read = |ix: u32, name: &str| {
            res.resolve(&sheet, &CellAddr::in_row("Character", None, RowKey::Ix(ix), name))
                .and_then(|f| f.value.cell?.value.as_ref()?.number())
                .unwrap()
        };
        assert!((read(0, "Size") - 11.0 / 72.0).abs() < 1e-6 && read(0, "Style") == 1.0, "{id}: the name is bold at 11 points");
        assert!((read(1, "Size") - 8.0 / 72.0).abs() < 1e-6 && read(1, "Style") == 0.0, "{id}: the type line is 8 points");
        assert!((read(2, "Size") - 9.0 / 72.0).abs() < 1e-6 && read(2, "Style") == 0.0, "{id}: the description is 9 points");
    }
}

#[test]
fn a_boundary_shows_its_name_and_kind_and_a_deployment_node_its_name_technology_and_instances() {
    let mut s = session();
    let boundary = put(&mut s, "c4.boundary", (3.0, 5.0));
    assert_eq!(fields(&s, boundary), ["Name", "[Software System]"]);
    set(&mut s, boundary, "Name", "Internet Banking");
    set(&mut s, boundary, "Kind", "Enterprise");
    assert_eq!(fields(&s, boundary), ["Internet Banking", "[Enterprise]"]);
    let node = put(&mut s, "c4.deployment-node", (9.0, 5.0));
    assert_eq!(fields(&s, node), ["Name", "[Deployment Node]", "x 1"]);
    set(&mut s, node, "Technology", "Ubuntu 24.04");
    set(&mut s, node, "Instances", "2");
    assert_eq!(fields(&s, node), ["Name", "[Deployment Node: Ubuntu 24.04]", "x 2"]);
    // An instance count may be a range (the model's examples write `x` and a range).
    set(&mut s, node, "Instances", "1-3");
    assert_eq!(fields(&s, node)[2], "x 1-3");
}

#[test]
fn the_title_shows_its_diagram_kind_and_name_over_its_description() {
    let mut s = session();
    let title = put(&mut s, "c4.title", (5.0, 9.0));
    assert_eq!(label(&s, title), (vec!["[System Context] Title".to_owned(), "Description".to_owned()], "\n".to_owned()));
    set(&mut s, title, "Diagram", "Container");
    set(&mut s, title, "Title", "Internet Banking System");
    set(&mut s, title, "Description", "Shows the applications that make up the system");
    assert_eq!(fields(&s, title), ["[Container] Internet Banking System", "Shows the applications that make up the system"]);
    // A kind that is not an item of the list is refused in words and changes nothing.
    let refused = s.execute("shapedata.set", &json!({ "id": title, "row": "Diagram", "value": "Sequence" }));
    assert!(refused.is_err(), "a list takes only its items");
    assert_eq!(fields(&s, title)[0], "[Container] Internet Banking System");
}

#[test]
fn the_relationship_shows_its_description_over_its_technology() {
    let mut s = session();
    let line = put(&mut s, "c4.relationship", (5.0, 5.0));
    // Dropped, the technology line is empty and the description is its placeholder.
    assert_eq!(label(&s, line), (vec!["Description".to_owned(), String::new()], "\n".to_owned()));
    // The description over `[<technology>]`.
    run(
        &mut s,
        "shapedata.set",
        json!({ "id": line, "values": [{ "row": "Description", "value": "Reads orders from" }, { "row": "Technology", "value": "SQL/TCP" }] }),
    );
    assert_eq!(label(&s, line), (vec!["Reads orders from".to_owned(), "[SQL/TCP]".to_owned()], "\n".to_owned()));
    set(&mut s, line, "Technology", "");
    assert_eq!(fields(&s, line), ["Reads orders from", ""], "a relationship with no technology shows no brackets");
    // It is dashed with a solid arrowhead at its end and none at its begin, and it holds no end cell of its own: the master's are inherited.
    let doc = s.document().unwrap();
    assert_eq!((number(doc, line, "BeginArrow"), number(doc, line, "EndArrow"), number(doc, line, "LinePattern")), (Some(0.0), Some(4.0), Some(2.0)));
    let at = doc.find_shape(Owner::Page(1), line).unwrap();
    assert!(doc.shape(&at).unwrap().sheet.cell("EndArrow").is_none() && doc.shape(&at).unwrap().sheet.cell("LinePattern").is_none());
    // `stencil.list` gives the window's tile the same numbers.
    assert_eq!(master("c4.relationship").look().map(|l| l.numbers()), Some((0, 4, 2)));
}

// ---- colours: the accents follow the theme, external is grey ------------------------------------------------------------------------

#[test]
fn the_kinds_accents_follow_the_theme_and_have_our_defaults_with_none() {
    let mut s = session();
    let shapes: Vec<(&str, u32)> = ["c4.person", "c4.software-system", "c4.container", "c4.component"]
        .iter()
        .enumerate()
        .map(|(k, id)| (*id, put(&mut s, id, (1.5 + 2.0 * k as f64, 5.0))))
        .collect();
    let fills = |s: &Session| -> Vec<String> { shapes.iter().map(|(_, id)| fill(s, *id)).collect() };
    // With no theme: the Container is the base blue, the System a shade darker, the Person darker still, and the Component a tint of the blue (design §4.7).
    let defaults = fills(&s);
    assert_eq!(defaults[2], "#3a7fc0", "the Container is the base blue: {defaults:?}");
    let (person, system, container, component) = (lightness(&defaults[0]), lightness(&defaults[1]), lightness(&defaults[2]), lightness(&defaults[3]));
    assert!(person < system && system < container && container < component, "dark to light: {defaults:?}");
    let (r, g, b) = rgb(&defaults[3]);
    assert!(b > r && b > g, "the component is a light blue: {defaults:?}");
    // Under a theme each follows the theme's accent: they change, and keep their order of lightness.
    let names = themes(&mut s);
    assert!(names.len() >= 3, "the catalogue has themes: {names:?}");
    let mut changed = false;
    for theme in &names {
        run(&mut s, "theme.apply", json!({ "theme": theme }));
        let under = fills(&s);
        changed |= under != defaults;
        let order = under.iter().map(|f| lightness(f)).collect::<Vec<_>>();
        assert!(order[0] <= order[1] && order[1] <= order[2] && order[2] <= order[3], "{theme}: dark to light, as with no theme: {under:?}");
    }
    assert!(changed, "at least one theme gives other accents than our defaults");
    run(&mut s, "theme.apply", json!({ "theme": "none" }));
    assert_eq!(fills(&s), defaults, "with no theme again they are our defaults again");
}

#[test]
fn an_element_dropped_into_a_themed_drawing_has_the_colour_the_theme_gives_it_at_once() {
    // A drawing that has its theme before the shape is dropped: the shape's accent is the theme's, as it is when the theme is applied after the drop.
    let mut s = session();
    let names = themes(&mut s);
    let mut differs = 0;
    for theme in &names {
        let mut after = session();
        let late: Vec<u32> = ACCENTS.iter().enumerate().map(|(k, id)| put(&mut after, id, (1.0 + 1.5 * k as f64, 5.0))).collect();
        let key_after = put(&mut after, "c4.key", (9.0, 8.0));
        run(&mut after, "theme.apply", json!({ "theme": theme }));
        let mut before = session();
        run(&mut before, "theme.apply", json!({ "theme": theme }));
        let early: Vec<u32> = ACCENTS.iter().enumerate().map(|(k, id)| put(&mut before, id, (1.0 + 1.5 * k as f64, 5.0))).collect();
        for (k, id) in ACCENTS.iter().enumerate() {
            assert_eq!(
                fill(&before, early[k]),
                fill(&after, late[k]),
                "{theme}: {id} dropped into the themed drawing has the colour the theme gives it"
            );
        }
        // The text colour is a formula of the theme too (light on the dark kinds), and so are the key's samples, members of a group.
        let ink = |s: &Session, id: u32| -> String {
            let doc = s.document().unwrap();
            let sheet = SheetRef::Shape(doc.find_shape(Owner::Page(1), id).unwrap());
            let found = Resolver::new(doc).resolve(&sheet, &CellAddr::in_row("Character", None, RowKey::Ix(0), "Color")).unwrap();
            found.value.cell.and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()).unwrap()
        };
        assert_eq!(ink(&before, early[2]), ink(&after, late[2]), "{theme}: the text colour of a Container dropped into the themed drawing");
        let key_before = put(&mut before, "c4.key", (9.0, 8.0));
        for member in 1..=5 {
            assert_eq!(
                fill(&before, key_before + member),
                fill(&after, key_after + member),
                "{theme}: sample {member} of a key dropped into the themed drawing has the colour the theme gives it"
            );
        }
        let mut plain = session();
        let none = put(&mut plain, "c4.container", (5.0, 5.0));
        differs += usize::from(fill(&before, early[2]) != fill(&plain, none));
    }
    assert!(differs > 0, "a theme of the catalogue gives the Container another colour than our default");
}

#[test]
fn external_true_makes_the_fill_grey_under_no_theme_and_every_theme() {
    let mut s = session();
    let shapes: Vec<u32> = ACCENTS.iter().enumerate().map(|(k, id)| put(&mut s, id, (1.0 + 1.5 * k as f64, 5.0))).collect();
    let fills = |s: &Session| -> Vec<String> { shapes.iter().map(|id| fill(s, *id)).collect() };
    let mut every = vec!["none".to_owned()];
    every.extend(themes(&mut s));
    for theme in every {
        run(&mut s, "theme.apply", json!({ "theme": theme }));
        let accents = fills(&s);
        for id in &shapes {
            set(&mut s, *id, "External", "TRUE");
        }
        let grey = fills(&s);
        assert!(grey.windows(2).all(|w| w[0] == w[1]), "{theme}: one grey for every kind: {grey:?}");
        assert!(accents.iter().all(|a| *a != grey[0]), "{theme}: the grey is no kind's accent: {accents:?} against {grey:?}");
        if theme == "none" {
            let (r, g, b) = rgb(&grey[0]);
            assert!(r == g && g == b, "with no theme the external fill is a neutral grey: {grey:?}");
            assert!((90..=150).contains(&r), "a mid grey, light enough to read white text on and dark enough to tell from paper: {grey:?}");
        }
        for id in &shapes {
            set(&mut s, *id, "External", "FALSE");
        }
        assert_eq!(fills(&s), accents, "{theme}: external false is the kind's accent again");
    }
}

#[test]
fn the_text_is_light_on_the_dark_kinds_and_dark_on_the_component_unless_it_is_external() {
    let mut s = session();
    let colour = |s: &Session, id: u32| -> String {
        let doc = s.document().unwrap();
        let sheet = SheetRef::Shape(doc.find_shape(Owner::Page(1), id).unwrap());
        let found = Resolver::new(doc).resolve(&sheet, &CellAddr::in_row("Character", None, RowKey::Ix(0), "Color")).unwrap();
        found.value.cell.and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()).unwrap()
    };
    for id in ["c4.person", "c4.software-system", "c4.container", "c4.database-container", "c4.queue-container"] {
        let shape = put(&mut s, id, (5.0, 5.0));
        assert_eq!(colour(&s, shape), "#ffffff", "{id}: light text on a dark fill");
        set(&mut s, shape, "External", "TRUE");
        assert_eq!(colour(&s, shape), "#ffffff", "{id}: and on the grey");
    }
    let component = put(&mut s, "c4.component", (5.0, 7.0));
    assert!(lightness(&colour(&s, component)) < 60.0, "dark text on the light blue: {}", colour(&s, component));
    set(&mut s, component, "External", "TRUE");
    assert_eq!(colour(&s, component), "#ffffff", "light text once it is grey");
}

// ---- the figures -----------------------------------------------------------------------------------------------------------------------

const PPI: f64 = 200.0;

/// Whether the page `doc` drawn at 200 pixels to the inch has ink at (x, y) inches from its lower left corner.
fn ink_at(doc: &Document, page: u32, (x, y): (f64, f64)) -> bool {
    let view = diacraft_render::View::fit_page(doc, page, PPI).unwrap();
    let mut renderer = diacraft_render::Renderer::new();
    renderer.set_paper(None);
    let frame = renderer.render(doc, page, &view);
    let height = doc.page(page).unwrap().sheet.cell("PageHeight").and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap();
    let (col, row) = ((x * PPI) as usize, ((height - y) * PPI) as usize);
    frame.rgba_premul.get((row * frame.width as usize + col) * 4 + 3).is_some_and(|alpha| *alpha > 8)
}

#[test]
fn a_person_is_a_round_head_over_a_rounded_body_with_room_between_them() {
    // Dropped at (5, 5) the box is 1.4 by 1.5 inches: the head is 0.4 inch across at the top, the body stands to 0.7 of the height, and 0.06 inch between them is paper.
    let (doc, page, id) = dropped(master("c4.person"));
    assert!(ink_at(&doc, page, (5.0, 5.55)), "the head is filled");
    assert!(!ink_at(&doc, page, (5.0, 5.33)), "there is room between the head and the body");
    assert!(ink_at(&doc, page, (4.5, 4.6)), "the body is filled");
    assert!(!ink_at(&doc, page, (4.4, 5.55)), "the head is narrow: the paper beside it is bare");
    assert!(!ink_at(&doc, page, (4.4, 5.6)), "and so above the body, to its side");
    // The first connection point is the top of the head, on the box's top edge.
    let points = draw::connection_points(&doc, page, id);
    assert!((points[0].x - 5.0).abs() < 1e-6 && (points[0].y - 5.75).abs() < 1e-6 && points[0].dir_y == 1.0, "{points:?}");
}

// ---- the title and the key: no connector joins them -----------------------------------------------------------------------------------

/// A page with the master dropped at (5, 5): the document, the page and the shape's `ID`.
fn dropped(spec: &MasterSpec) -> (Document, u32, u32) {
    let mut doc = Document::new_blank();
    let page = doc.pages.get(0).unwrap().id;
    let info = draw::drop_shape(&mut doc, page, spec, 5.0, 5.0).unwrap();
    (doc, page, info.id)
}

#[test]
fn a_title_and_a_key_have_no_connection_points_and_pass_the_catalogue_check() {
    for id in ["c4.title", "c4.key"] {
        let m = master(id);
        assert_eq!(m.point_count(), 0, "{id}: no point");
        assert!(m.dress().is_some_and(|d| matches!(d.role, crate::stencil::Role::Pointless)), "{id}: the catalogue knows it as pointless");
        catalogue_tests::assert_formulas_hold(m);
        catalogue_tests::assert_paths_close(m);
        let (doc, page, shape) = dropped(m);
        assert!(draw::connection_points(&doc, page, shape).is_empty(), "{id}: a dropped instance has none either");
        assert_eq!(crate::stencil::caps_problem(m), None, "{id}");
    }
}

#[test]
fn the_title_is_text_only_with_no_line_and_no_fill() {
    let mut s = session();
    let title = put(&mut s, "c4.title", (5.0, 9.0));
    let doc = s.document().unwrap();
    assert_eq!(number(doc, title, "FillPattern"), Some(0.0), "no fill");
    assert_eq!(number(doc, title, "LinePattern"), Some(0.0), "no line");
}

#[test]
fn the_key_is_a_group_of_a_title_and_five_samples_in_the_masters_colours() {
    let mut s = session();
    let key = put(&mut s, "c4.key", (5.0, 5.0));
    let doc = s.document().unwrap().clone();
    let at = doc.find_shape(Owner::Page(1), key).unwrap();
    assert_eq!(doc.shape(&at).unwrap().children.len(), 6, "a title and five samples");
    // The master's members: the samples first, and the title last (typing on the selected key goes to it).
    let master_id = doc.shape(&at).unwrap().master.unwrap();
    let members = doc.master(master_id).unwrap().shapes.get(0).unwrap().children.clone();
    let texts: Vec<String> = members
        .iter()
        .map(|m| {
            m.text
                .as_ref()
                .map(|t| {
                    t.parts
                        .iter()
                        .filter_map(|p| if let diacraft_doc::TextPart::Chars(c) = p { Some(c.to_string()) } else { None })
                        .collect::<String>()
                })
                .unwrap_or_default()
        })
        .collect();
    assert_eq!(texts, ["Person", "Software System", "Container", "Component", "External", "Key"]);
    // Each sample has the fill of the master it stands for, with no theme: the same formulas.
    let fill_of = |k: u32| -> String {
        let sheet = SheetRef::Shape(at.child(k));
        Resolver::new(&doc)
            .resolve(&sheet, &CellAddr::direct("FillForegnd"))
            .and_then(|f| f.value.cell?.value.as_ref().map(|v| v.text().to_owned()))
            .unwrap_or_default()
    };
    for (k, id) in ["c4.person", "c4.software-system", "c4.container", "c4.component"].into_iter().enumerate() {
        let shape = put(&mut s, id, (1.5 + 2.0 * k as f64, 9.0));
        assert_eq!(fill_of(u32::try_from(k).unwrap()), fill(&s, shape), "the {id} sample has the master's colour");
    }
    let external = put(&mut s, "c4.container", (9.0, 9.0));
    set(&mut s, external, "External", "TRUE");
    assert_eq!(fill_of(4), fill(&s, external), "the external sample is the grey");
    // The title and the samples all lie inside the key's box.
    let frame = draw::shape_frame(&doc, 1, key).unwrap();
    let corners = draw::shape_corners(&doc, 1, key).unwrap();
    assert!(frame.width > 0.0 && corners.len() == 4);
    // The key holds its height, so a resize changes its width only.
    assert_eq!(number(&doc, key, "LockHeight"), Some(1.0));
}

#[test]
fn the_keys_members_fill_its_box_so_that_an_odg_keeps_its_frame() {
    // A drawing file holds a group as its members and keeps the group's frame only when it is the box the members make together.
    let (doc, page, key) = dropped(master("c4.key"));
    let at = doc.find_shape(Owner::Page(page), key).unwrap();
    let res = Resolver::new(&doc);
    let num =
        |sheet: SheetRef, cell: &str| res.resolve(&sheet, &CellAddr::direct(cell)).and_then(|f| f.value.cell?.value.as_ref()?.number()).unwrap();
    let group = SheetRef::Shape(at.clone());
    let (w, h) = (num(group.clone(), "Width"), num(group, "Height"));
    let (mut left, mut bottom, mut right, mut top) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for k in 0..6 {
        let member = SheetRef::Shape(at.child(k));
        let (x, y) = (num(member.clone(), "PinX"), num(member.clone(), "PinY"));
        let (mw, mh) = (num(member.clone(), "Width"), num(member, "Height"));
        (left, bottom, right, top) = (left.min(x - mw / 2.0), bottom.min(y - mh / 2.0), right.max(x + mw / 2.0), top.max(y + mh / 2.0));
    }
    assert!(
        left.abs() < 1e-9 && bottom.abs() < 1e-9 && (right - w).abs() < 1e-9 && (top - h).abs() < 1e-9,
        "the members' box ({left}, {bottom}) to ({right}, {top}) is not the key's {w} by {h}"
    );
}

// ---- the boundary and the deployment node are containers -----------------------------------------------------------------------------

#[test]
fn a_boundary_takes_a_dropped_container_as_a_member() {
    let mut s = session();
    let boundary = put(&mut s, "c4.boundary", (5.0, 5.0));
    let info = run(&mut s, "container.info", json!({ "ids": [boundary] }));
    assert_eq!(info["containers"][0]["id"], boundary, "an instance of the boundary is a container: {info}");
    assert_eq!(info["containers"][0]["style"], Value::Null, "its look is its master's, not the gallery's");
    // A dashed outline and no fill: what is inside shows through.
    let doc = s.document().unwrap();
    assert_eq!((number(doc, boundary, "FillPattern"), number(doc, boundary, "LinePattern")), (Some(0.0), Some(2.0)));
    // A Container master dropped inside joins it, and a move of the boundary takes it.
    let inner = put(&mut s, "c4.container", (5.0, 5.2));
    let info = run(&mut s, "container.info", json!({ "ids": [boundary] }));
    assert_eq!(info["containers"][0]["members"], json!([inner]), "{info}");
    let was = draw::shape_corners(s.document().unwrap(), 1, inner).unwrap();
    run(&mut s, "shape.move", json!({ "ids": [boundary], "dx": 1.5, "dy": 1.0 }));
    let now = draw::shape_corners(s.document().unwrap(), 1, inner).unwrap();
    assert!(
        was.iter().zip(now.iter()).all(|(a, b)| (a.0 + 1.5 - b.0).abs() < 1e-6 && (a.1 + 1.0 - b.1).abs() < 1e-6),
        "the member went with the boundary"
    );
    // One dropped outside does not join.
    let outside = put(&mut s, "c4.software-system", (1.0, 1.0));
    let info = run(&mut s, "container.info", json!({ "ids": [boundary] }));
    assert_eq!(info["containers"][0]["members"], json!([inner]), "{outside} is not a member: {info}");
}

#[test]
fn deployment_nodes_nest_and_hold_the_infrastructure_and_the_containers_that_run_on_them() {
    let mut s = session();
    let outer = put(&mut s, "c4.deployment-node", (6.0, 5.0));
    // Make room for a node inside it: 9 inches by 6.
    run(&mut s, "shape.resize", json!({ "id": outer, "x": 1.5, "y": 2.0, "width": 9.0, "height": 6.0 }));
    let inner = put(&mut s, "c4.deployment-node", (4.0, 5.0));
    let infra = put(&mut s, "c4.infrastructure-node", (8.0, 5.0));
    let info = run(&mut s, "container.info", json!({ "ids": [outer, inner] }));
    let members = |id: u32| -> Vec<u64> {
        info["containers"].as_array().unwrap().iter().find(|c| c["id"] == id).unwrap()["members"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_u64)
            .collect()
    };
    assert!(members(outer).contains(&u64::from(inner)) && members(outer).contains(&u64::from(infra)), "{info}");
    assert!(!members(inner).contains(&u64::from(infra)), "the infrastructure node is the outer node's, not the inner one's: {info}");
}

#[test]
fn a_container_command_leaves_a_boundarys_and_a_nodes_outline_alone_and_a_style_is_refused_in_words() {
    // The outline is the master's, so `container.fit` and `container.margin` change the box and no formula of the outline; `container.style` would replace the look by the gallery's.
    let mut s = session();
    for id in ["c4.boundary", "c4.deployment-node"] {
        let shape = put(&mut s, id, (5.0, 5.0));
        let inner = put(&mut s, "c4.component", (5.0, 5.2));
        let outline = |s: &Session| -> Vec<(String, String)> {
            let doc = s.document().unwrap();
            let sheet = SheetRef::Shape(doc.find_shape(Owner::Page(1), shape).unwrap());
            let res = Resolver::new(doc);
            let mut out = Vec::new();
            for key in res.effective_sections(&sheet, "Geometry").unwrap_or_default() {
                for row in res.effective_rows(&sheet, &key).unwrap_or_default() {
                    for (name, found) in &row.cells {
                        out.push((
                            format!("{:?} {:?} {name}", key.ix, row.key),
                            found.formula.cell.and_then(|c| c.formula.text()).unwrap_or("").to_owned(),
                        ));
                    }
                }
            }
            out
        };
        let start = outline(&s);
        run(&mut s, "container.fit", json!({ "ids": [shape] }));
        run(&mut s, "container.margin", json!({ "ids": [shape], "margin": 0.5 }));
        assert_eq!(outline(&s), start, "{id}: the same formulas after a fit and a margin");
        let before = s.document().unwrap().clone();
        let said = s.execute("container.style", &json!({ "ids": [shape], "style": "strip" })).unwrap_err().to_string();
        assert!(said.contains("master") && said.contains("look"), "{id}: the words say the look is the master's: {said}");
        assert_eq!(s.document().unwrap(), &before, "{id}: nothing changed");
        run(&mut s, "shape.delete", json!({ "ids": [shape, inner] }));
    }
}

// ---- what a person does with them ---------------------------------------------------------------------------------------------------

#[test]
fn every_two_dimensional_master_is_a_shape_people_join_and_the_line_is_not() {
    assert_eq!(MASTERS.len(), 12, "the stencil holds its twelve masters, so that the loop below is not a loop over nothing");
    let mut s = session();
    for (k, m) in MASTERS.iter().enumerate() {
        let shape = put(&mut s, m.id, (1.0 + k as f64 * 0.5, 2.0 + k as f64 * 0.5));
        let kind = number(s.document().unwrap(), shape, "ObjType");
        if m.is_connector() {
            assert!(kind.is_none_or(|v| v == 2.0), "{}: a connector holds no placeable flag: {kind:?}", m.id);
        } else {
            assert_eq!(kind, Some(1.0), "{}, a shape people join", m.id);
        }
    }
}

#[test]
fn the_quick_shapes_are_person_software_system_container_and_component() {
    let mut s = session();
    let answer = run(&mut s, "stencil.quick", json!({ "stencil": "c4" }));
    let ids: Vec<&str> = answer["stencils"][0]["masters"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["c4.person", "c4.software-system", "c4.container", "c4.component"]);
    // The strip never offers the title, the key, the relationship or a container master of the window's own kind.
    let offer = run(&mut s, "stencil.quick", json!({ "stencil": "c4" }));
    let offered: Vec<&str> = offer["offer"]["masters"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert_eq!(offered, ["c4.person", "c4.software-system", "c4.container", "c4.component"]);
}

#[test]
fn the_title_and_the_key_may_be_quick_shapes_and_the_strip_never_offers_them() {
    // The strip adds a shape and joins it to the one it came from: a title and a key have no connection point to join, so they are off it.
    let mut s = session();
    for gone in ["c4.person", "c4.software-system", "c4.container"] {
        run(&mut s, "stencil.remove_quick", json!({ "stencil": "c4", "master": gone }));
    }
    for added in ["c4.title", "c4.key"] {
        run(&mut s, "stencil.add_quick", json!({ "stencil": "c4", "master": added }));
    }
    let answer = run(&mut s, "stencil.quick", json!({ "stencil": "c4" }));
    let held: Vec<&str> = answer["stencils"][0]["masters"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert_eq!(held, ["c4.component", "c4.title", "c4.key"], "the Quick Shapes hold what was added");
    let offered: Vec<&str> = answer["offer"]["masters"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert_eq!(offered, ["c4.component"], "the strip offers none that no connector joins");
    assert!(!master("c4.title").on_strip() && !master("c4.key").on_strip() && master("c4.person").on_strip());
}

/// The ids a search for `text` finds.
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
fn every_master_has_search_words_and_the_words_people_use_find_their_masters() {
    for m in MASTERS.iter() {
        assert!(!super::keywords(m.id).is_empty(), "{} has no search words", m.id);
    }
    let mut s = session();
    for (word, id) in [
        ("actor", "c4.person"),
        ("user", "c4.person"),
        ("system", "c4.software-system"),
        ("service", "c4.container"),
        ("module", "c4.component"),
        ("database", "c4.database-container"),
        ("queue", "c4.queue-container"),
        ("broker", "c4.queue-container"),
        ("scope", "c4.boundary"),
        ("server", "c4.deployment-node"),
        ("load balancer", "c4.infrastructure-node"),
        ("heading", "c4.title"),
        ("legend", "c4.key"),
        ("dependency", "c4.relationship"),
    ] {
        let hits = found(&mut s, word);
        assert!(hits.iter().any(|h| h == id), "`{word}` does not find {id}: {hits:?}");
    }
    // The searches the window's tests pin for the first batches' masters (Arrow Shapes, the Basic Flowchart) find no C4 master.
    for word in ["arrow", "process", "decision"] {
        let hits = found(&mut s, word);
        assert!(hits.iter().all(|h| !h.starts_with("c4.")), "`{word}` finds a C4 master: {hits:?}");
    }
}

#[test]
fn the_c4_template_is_listed_once_the_stencil_ships_and_opens_c4_then_the_basic_shapes() {
    let template = crate::template::find("c4_diagram").expect("the stencil ships, so the template is listed");
    assert_eq!((template.name, template.stencils), ("C4 Model Diagram", &["c4", "basic"][..]));
    assert!(template.description.contains("based on"), "{}", template.description);
    let mut s = Session::new();
    let started = run(&mut s, "file.new", json!({ "template": "c4_diagram" }));
    assert_eq!(started["stencils"]["open"], json!(["c4", "basic"]), "{started}");
    let listed = run(&mut s, "template.list", json!({}));
    assert!(listed["templates"].as_array().unwrap().iter().any(|t| t["id"] == "c4_diagram"));
}

// ---- hostile input ------------------------------------------------------------------------------------------------------------------

#[test]
fn a_value_of_32768_characters_and_a_list_item_no_item_matches_change_nothing_beyond_a_bounded_text() {
    let mut s = session();
    let container = put(&mut s, "c4.container", (5.0, 5.0));
    let boundary = put(&mut s, "c4.boundary", (5.0, 9.0));
    let big = "x".repeat(32_768);
    set(&mut s, container, "Name", &big);
    set(&mut s, container, "Technology", &big);
    set(&mut s, container, "Description", &big);
    // The data is held as the person typed it (the limit is 32,768 characters a value), and each field shows its value whole, the type line with its brackets.
    let shown = fields(&s, container);
    assert_eq!(shown.iter().map(|f| f.chars().count()).collect::<Vec<_>>(), [32_768, 32_768 + "[Container: ]".chars().count(), 32_768]);
    assert!(shown[1].starts_with("[Container: xxx"), "the type line still reads its technology");
    // One character past the limit of a value is refused in words, and the document is as it was.
    let before = s.document().unwrap().clone();
    let refused = s.execute("shapedata.set", &json!({ "id": container, "row": "Name", "value": "y".repeat(32_769) }));
    assert!(refused.is_err(), "a value past the limit is refused");
    assert_eq!(s.document().unwrap(), &before);
    // A fixed list takes only its items: a kind no item matches is refused, and the boundary keeps the one it had.
    let refused = s.execute("shapedata.set", &json!({ "id": boundary, "row": "Kind", "value": "Planet" }));
    assert!(refused.is_err(), "a list takes only its items");
    assert_eq!(fields(&s, boundary)[1], "[Software System]");
    // The page still draws: the renderer lays out the first 16,384 characters of a text and says so in a note, and leaves nothing else out.
    let doc = s.document().unwrap();
    let view = diacraft_render::View::fit_page(doc, 1, 50.0).unwrap();
    let mut renderer = diacraft_render::Renderer::new();
    renderer.set_paper(None);
    let frame = renderer.render(doc, 1, &view);
    // (A face the machine lacks is drawn by a stand-in, and is its own note: nothing is left out for it.)
    let said = |k: &diacraft_render::Skipped| matches!(k.kind, diacraft_render::SkipKind::TextNote | diacraft_render::SkipKind::FaceStandIn);
    assert!(frame.skipped.iter().all(said), "only the note of a cut text: {:?}", frame.skipped);
}

// ---- a master edited by a person keeps working --------------------------------------------------------------------------------------

#[test]
fn a_resize_of_an_element_keeps_its_text_and_data() {
    let mut s = session();
    let container = put(&mut s, "c4.container", (5.0, 5.0));
    set(&mut s, container, "Technology", "Rust");
    run(&mut s, "shape.resize", json!({ "id": container, "x": 3.0, "y": 4.0, "width": 4.0, "height": 2.0 }));
    assert_eq!(fields(&s, container), ["Name", "[Container: Rust]", "Description"]);
    let doc = s.document().unwrap();
    assert!((number(doc, container, "Width").unwrap() - 4.0).abs() < 1e-9 && (number(doc, container, "Height").unwrap() - 2.0).abs() < 1e-9);
    // The twelve connection points of the grid follow the new size: the middle of the top is at its half.
    let points = draw::connection_points(doc, 1, container);
    assert_eq!(points.len(), 12);
    assert!(points.iter().any(|p| (p.x - 5.0).abs() < 1e-6 && (p.y - 6.0).abs() < 1e-6), "the middle of the top is on the new outline: {points:?}");
}
