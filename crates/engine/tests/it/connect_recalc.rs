//! **A size that a recalculation changes is a size that routes the connectors glued to the shape again**, as a command that resizes or moves the shape does.
//!
//! The bug: two UML realizations glued to an interface were left with slanted legs, and their arrowheads ended in the interface, after `theme.apply`. The theme gives the text a
//! face of its own, the interface's operations wrap in it and the interface grows, so the connection points it is glued to move; the ends of a connector follow the points by
//! their formulas, and the route between the ends stayed as it was. A command that resizes a shape names it (`touched`), and the routing of what is glued to it follows; a
//! recalculation names nothing, so what it moved was never looked for. Each test here changes a size without naming the shape: a theme, a variant, no theme, a line of text, the
//! size of a character, a formula that reads another shape, a style that the text reads, and then holds the route to what a routing makes: right-angle legs that end on the
//! connection points the connector is glued to.

use std::sync::{Arc, Mutex, MutexGuard};

use diacraft_engine::Session;
use diacraft_engine::draw::{connection_points, connector_path};
use diacraft_engine::measure::set_measure_fonts;
use diacraft_fonts::FontDb;
use diacraft_fonts::synth::SynthFont;
use serde_json::json;

use crate::edit::{doc, drop_shape, id_of, num, route};
use crate::{blank_session, check_session, run};

/// Two inches or a hair of tolerance, in the page's units: the legs of a route are level or upright to this.
const TOLERANCE: f64 = 1.0e-6;

/// The faces the commits measure text in, process-wide: the tests of this file take turns (nextest gives each a process of its own; a plain `cargo test` does not).
static SETTING: Mutex<()> = Mutex::new(());

/// The faces of the machine the bug was found on, made small: **the default face (Liberation Sans, which a run that names none is drawn in) is narrow, and the faces of the
/// themes that change it (DejaVu) are wide**, so a theme changes the width of every line of text and with it the height of a compartment that wraps, on every machine, where
/// the bundled faces would measure every name alike. Held for as long as the test runs, and taken back after it.
struct Faces {
    _held: MutexGuard<'static, ()>,
}

impl Faces {
    fn set() -> Faces {
        let only = SETTING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut db = FontDb::empty();
        for (family, advance) in [("Liberation Sans", 450), ("Liberation Serif", 450), ("DejaVu Sans", 800), ("DejaVu Serif", 800)] {
            assert_eq!(db.add_font_data(SynthFont::new(family).ascii(advance).build()).added, 1, "{family}");
        }
        set_measure_fonts(Some(Arc::new(db)));
        Faces { _held: only }
    }
}

impl Drop for Faces {
    fn drop(&mut self) {
        set_measure_fonts(None);
    }
}

/// What a drop of a master of the UML Class stencil is given, and the realization's two ends, in the order of the bug's steps.
struct Diagram {
    /// Held for the test's life: the faces the text is measured in.
    _faces: Faces,
    s: Session,
    interface: u32,
    /// The text of the interface's operations, which the theme's face wraps and so makes taller.
    operations: u32,
    wires: [u32; 2],
}

fn drop_uml(s: &mut Session, master: &str, x: f64, y: f64) -> u32 {
    id_of(&run(s, "shape.drop", json!({"master": master, "stencil": "uml-class", "page": 1, "x": x, "y": y})))
}

/// The class diagram of the bug: an interface whose operations are two long lines, two classes under it, and a realization from each class to the interface.
fn diagram() -> Diagram {
    let faces = Faces::set();
    let mut s = Session::new();
    run(&mut s, "file.new", json!({"template": "uml_class"}));
    let interface = drop_uml(&mut s, "uml-class.interface", 4.0, 6.0);
    let operations = interface + 2;
    run(&mut s, "shape.set_text", json!({"id": operations, "text": "+ authorize(amount : Money)\n+ capture() : Receipt"}));
    let left = drop_uml(&mut s, "uml-class.class", 3.0, 3.0);
    let right = drop_uml(&mut s, "uml-class.class", 5.5, 3.0);
    let mut wires = [0; 2];
    for (wire, class) in wires.iter_mut().zip([left, right]) {
        *wire = drop_uml(&mut s, "uml-class.realization", 1.0, 1.0);
        run(&mut s, "connector.reconnect", json!({"id": *wire, "end": "begin", "target": {"id": class, "point": 1}}));
        run(&mut s, "connector.reconnect", json!({"id": *wire, "end": "end", "target": {"id": interface, "point": 7}}));
    }
    let d = Diagram { _faces: faces, s, interface, operations, wires };
    d.holds("the diagram as the bug's steps build it");
    d
}

impl Diagram {
    /// Every connector has right-angle legs and ends on the connection points it is glued to: what a routing makes.
    fn holds(&self, what: &str) {
        holds(&self.s, &self.wires, what);
    }
}

/// The page's place of the connection point `index` of shape `id` (the `Connect` row's `ToPart` less 100).
fn point_of(s: &Session, id: u32, part: u32) -> (f64, f64) {
    let p = connection_points(doc(s), 1, id).into_iter().find(|p| p.index == part - 100).unwrap_or_else(|| panic!("shape {id} has no point {part}"));
    (p.x, p.y)
}

/// The two ends of connector `wire` where the shapes it is glued to hold them now: the begin glued to a shape's point, the end to another's.
fn glued_ends(s: &Session, wire: u32) -> Vec<(f64, f64)> {
    let page = doc(s).page(1).unwrap();
    ["BeginX", "EndX"]
        .iter()
        .map(|end| {
            let row = page
                .connects
                .iter()
                .find(|c| c.from_sheet == wire && c.from_cell.as_deref() == Some(*end))
                .unwrap_or_else(|| panic!("{wire} has no {end} glue"));
            point_of(s, row.to_sheet, row.to_part.unwrap_or(0))
        })
        .collect()
}

/// Whether every connector of `wires` has legs that are level or upright.
fn square(s: &Session, wires: &[u32], what: &str) {
    for wire in wires {
        let path = connector_path(doc(s), 1, *wire).unwrap_or_else(|| panic!("{what}: connector {wire} has no path"));
        for pair in path.points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert!(
                (a.0 - b.0).abs() < TOLERANCE || (a.1 - b.1).abs() < TOLERANCE,
                "{what}: connector {wire} has a slanted leg from {a:?} to {b:?}: {:?}",
                path.points
            );
        }
    }
}

/// Whether every connector of `wires` has legs that are level or upright and a route that runs from the point it is glued to at its begin to the one at its end.
fn holds(s: &Session, wires: &[u32], what: &str) {
    square(s, wires, what);
    for wire in wires {
        let path = connector_path(doc(s), 1, *wire).unwrap_or_else(|| panic!("{what}: connector {wire} has no path"));
        let ends = glued_ends(s, *wire);
        let (first, last) = (path.points[0], path.points[path.points.len() - 1]);
        let near = |p: (f64, f64), q: (f64, f64)| (p.0 - q.0).abs() < TOLERANCE && (p.1 - q.1).abs() < TOLERANCE;
        assert!(near(first, ends[0]) && near(last, ends[1]), "{what}: connector {wire} runs {first:?} to {last:?}, its glue is at {ends:?}");
    }
}

/// The height of shape `id`.
fn height(s: &Session, id: u32) -> f64 {
    num(doc(s), id, "Height")
}

/// The theme gives the interface's text a face of its own: the interface grows, and the realizations are routed to its new bottom edge (the bug).
#[test]
fn a_theme_routes_the_connectors_glued_to_the_interface_it_makes_taller() {
    // The two themes whose face is DejaVu Sans: wider than the default, so the lines wrap and the interface grows.
    for theme in ["fernwood", "inkstone"] {
        let mut d = diagram();
        let was = height(&d.s, d.interface);
        run(&mut d.s, "theme.apply", json!({"theme": theme}));
        assert_ne!(height(&d.s, d.interface), was, "{theme}: the theme changes the interface's height (the fixture is the bug's)");
        d.holds(&format!("after theme.apply {theme}"));
        assert_eq!(check_session(&d.s), Ok(()));
    }
}

/// A theme that follows another, and no theme, change the faces again: each routes what is glued to the shapes that changed size.
#[test]
fn another_theme_and_no_theme_route_the_connectors_again() {
    let mut d = diagram();
    for (params, what) in [
        (json!({"theme": "fernwood"}), "fernwood"),
        (json!({"theme": "cinderglow"}), "cinderglow after fernwood"),
        (json!({"theme": "inkstone", "variant": 2}), "inkstone variant 2"),
        (json!({"theme": "none"}), "no theme"),
    ] {
        run(&mut d.s, "theme.apply", params);
        d.holds(what);
    }
    run(&mut d.s, "theme.apply", json!({"theme": "tidepool"}));
    for variant in 0..4 {
        run(&mut d.s, "theme.variant", json!({"variant": variant}));
        d.holds(&format!("after theme.variant {variant}"));
    }
}

/// An undo puts the drawing back as it was, routes and all, and a redo gives the routed drawing the theme made: neither leaves a slanted leg.
#[test]
fn undo_and_redo_of_a_theme_keep_the_routes_of_the_drawing_they_restore() {
    let mut d = diagram();
    let before: Vec<Vec<(f64, f64)>> = d.wires.iter().map(|w| route(doc(&d.s), *w)).collect();
    run(&mut d.s, "theme.apply", json!({"theme": "fernwood"}));
    let themed: Vec<Vec<(f64, f64)>> = d.wires.iter().map(|w| route(doc(&d.s), *w)).collect();
    d.holds("after the theme");
    run(&mut d.s, "edit.undo", json!({}));
    d.holds("after the undo");
    let undone: Vec<Vec<(f64, f64)>> = d.wires.iter().map(|w| route(doc(&d.s), *w)).collect();
    assert_eq!(undone, before, "an undo restores the routes the drawing had");
    run(&mut d.s, "edit.redo", json!({}));
    d.holds("after the redo");
    let redone: Vec<Vec<(f64, f64)>> = d.wires.iter().map(|w| route(doc(&d.s), *w)).collect();
    assert_eq!(redone, themed, "a redo restores the routes the theme made");
}

/// A line typed into a compartment makes the interface taller, which a recalculation works out: the connectors glued to it follow.
#[test]
fn a_text_edit_that_grows_a_compartment_routes_the_connectors_glued_to_the_shape() {
    let mut d = diagram();
    let was = height(&d.s, d.interface);
    run(
        &mut d.s,
        "shape.set_text",
        json!({"id": d.operations, "text": "+ authorize(amount : Money)\n+ capture() : Receipt\n+ void()\n+ refund()\n+ cancel()"}),
    );
    assert!(height(&d.s, d.interface) > was, "three more lines make the interface taller");
    d.holds("after the text grew");
    // And a line taken out again.
    run(&mut d.s, "shape.set_text", json!({"id": d.operations, "text": "+ capture()"}));
    d.holds("after the text shrank");
    // The same through the editing of a range of the text, which the window's in-place editor sends.
    run(&mut d.s, "shape.set_text", json!({"id": d.operations, "start": 11, "end": 11, "text": "\n+ one\n+ two\n+ three\n+ four"}));
    d.holds("after a range of text was typed");
}

/// The size of the characters of a compartment is a format, which is a recalculation of the compartment's height too.
#[test]
fn a_font_size_that_grows_a_compartment_routes_the_connectors_glued_to_the_shape() {
    let mut d = diagram();
    let was = height(&d.s, d.interface);
    run(&mut d.s, "text.format", json!({"ids": [d.operations], "size": 24}));
    assert!(height(&d.s, d.interface) > was, "bigger type makes the interface taller");
    d.holds("after the type grew");
    run(&mut d.s, "text.format", json!({"ids": [d.operations], "size": 8}));
    d.holds("after the type shrank");
}

/// A format put on the group (the whole interface, which is formatted through the shapes inside it) reroutes the connectors glued to it as a format on one compartment
/// does, and so does a line or fill format that changes no size (it routes none: the targets reach the same writes).
#[test]
fn a_format_put_on_a_group_routes_the_connectors_glued_to_it() {
    let mut d = diagram();
    let was = height(&d.s, d.interface);
    run(&mut d.s, "text.format", json!({"ids": [d.interface], "size": 20}));
    assert!(height(&d.s, d.interface) > was, "bigger type on every compartment makes the interface taller");
    d.holds("after the type of the group grew");
    let routes: Vec<Vec<(f64, f64)>> = d.wires.iter().map(|w| route(doc(&d.s), *w)).collect();
    run(&mut d.s, "shape.format", json!({"ids": [d.interface], "line": "#2255aa", "fill": "#eeeeff"}));
    d.holds("after the line and fill of the group");
    assert_eq!(d.wires.iter().map(|w| route(doc(&d.s), *w)).collect::<Vec<_>>(), routes, "a format that changes no size routes nothing");
}

/// Rectangle B reads the width of rectangle A (`Sheet.1!Width`), and connector 4 joins B to D and is glued to neither A: a write to A widens B by a formula, and the connector
/// is routed to B's new edge though no command named B.
fn follower() -> Session {
    let mut s = blank_session();
    assert_eq!(drop_shape(&mut s, "rectangle", 1.0, 8.0), 1);
    assert_eq!(drop_shape(&mut s, "rectangle", 2.0, 2.0), 2);
    assert_eq!(drop_shape(&mut s, "rectangle", 7.0, 4.0), 3);
    run(&mut s, "sheet.set_cell", json!({"id": 2, "cell": "Width", "formula": "Sheet.1!Width"}));
    assert_eq!(id_of(&run(&mut s, "connector.add", json!({"from": {"id": 2}, "to": {"id": 3}}))), 4);
    holds(&s, &[4], "the connector the steps make");
    s
}

/// A formula that reads another shape's size changes this shape's size when that one's is written: the connector glued to the follower is routed again.
#[test]
fn a_formula_that_changes_a_size_routes_the_connectors_glued_to_the_shape() {
    let mut s = follower();
    let before = route(doc(&s), 4);
    run(&mut s, "sheet.set_cell", json!({"id": 1, "cell": "Width", "formula": "4"}));
    assert_eq!(num(doc(&s), 2, "Width"), 4.0, "B follows A's width");
    assert_ne!(route(doc(&s), 4), before, "the connector was routed again");
    holds(&s, &[4], "after A was widened");
    run(&mut s, "sheet.set_cell", json!({"id": 1, "cell": "Height", "formula": "3"}));
    run(&mut s, "sheet.set_cell", json!({"id": 2, "cell": "Height", "formula": "Sheet.1!Height"}));
    run(&mut s, "sheet.set_cell", json!({"id": 1, "cell": "Height", "formula": "0.5"}));
    holds(&s, &[4], "after A was made shorter");
    assert_eq!(check_session(&s), Ok(()));
}

/// The same write through the batch the window sends for a drag of a field: one undo step, and the connector is routed once, in it.
#[test]
fn a_batch_of_writes_that_changes_a_size_by_formula_routes_the_connectors_too() {
    let mut s = follower();
    run(&mut s, "sheet.set_cells", json!({"writes": [{"id": 1, "cell": "Width", "formula": "5"}]}));
    holds(&s, &[4], "after a batch");
    assert_eq!(check_session(&s), Ok(()));
}

/// Rectangle B (shape 2) is sized by `formula`, which names a cell that is not a shape's (shape 1 is a box that is left out of it), D (3) is plain and connector 4 joins B to D.
fn sized_by(formula: &str) -> Session {
    let mut s = blank_session();
    assert_eq!(drop_shape(&mut s, "rectangle", 1.0, 8.0), 1);
    assert_eq!(drop_shape(&mut s, "rectangle", 2.0, 2.0), 2);
    assert_eq!(drop_shape(&mut s, "rectangle", 7.0, 4.0), 3);
    run(&mut s, "sheet.set_cell", json!({"id": 2, "cell": "Width", "formula": formula}));
    assert_eq!(id_of(&run(&mut s, "connector.add", json!({"from": {"id": 2}, "to": {"id": 3}}))), 4);
    holds(&s, &[4], "the connector the steps make");
    s
}

/// A shape that is as wide as a tenth of the page is widened when the page is: the page setup commands recalculate and the connector glued to the shape follows.
#[test]
fn a_page_size_that_a_shape_reads_routes_the_connectors_glued_to_the_shape() {
    let mut s = sized_by("ThePage!PageWidth*0.2");
    let was = num(doc(&s), 2, "Width");
    run(&mut s, "page.size", json!({"width": 16.0, "height": 8.5}));
    assert!(num(doc(&s), 2, "Width") > was + 0.5, "B is wider with the page");
    holds(&s, &[4], "after the page was widened");
    assert_eq!(check_session(&s), Ok(()));
}

/// A cell of the document's sheet that a shape reads, and a master's: a write works every page out whole, and the connectors glued to the shapes that changed are routed again by the
/// settle step of each page, where a pass of the call's own did before (the master's case is `sheet_write.rs`).
#[test]
fn a_cell_of_the_documents_sheet_that_a_shape_reads_routes_the_connectors_glued_to_the_shape() {
    let mut s = blank_session();
    run(&mut s, "sheet.add_section", json!({"document": true, "section": "User"}));
    run(&mut s, "sheet.add_row", json!({"document": true, "section": "User", "name": "Rate"}));
    run(&mut s, "sheet.set_cell", json!({"document": true, "cell": "User.Rate", "formula": "2"}));
    assert_eq!(drop_shape(&mut s, "rectangle", 2.0, 2.0), 1);
    assert_eq!(drop_shape(&mut s, "rectangle", 7.0, 4.0), 2);
    run(&mut s, "sheet.set_cell", json!({"id": 1, "cell": "Width", "formula": "TheDoc!User.Rate"}));
    assert_eq!(id_of(&run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 2}}))), 3);
    holds(&s, &[3], "the connector the steps make");
    run(&mut s, "sheet.set_cell", json!({"document": true, "cell": "User.Rate", "formula": "5"}));
    assert_eq!(num(doc(&s), 1, "Width"), 5.0, "the shape reads the document's cell");
    holds(&s, &[3], "after the document's cell changed");
    assert_eq!(check_session(&s), Ok(()));
}

/// A theme that gives the text the face it had changes no size: no connector is routed again, so the routes are the ones the drawing had, to the digit (a route that is made again
/// puts the label of its connector back in the middle, where a person may have put it).
#[test]
fn a_theme_that_changes_no_size_leaves_the_routes_as_they_are() {
    let mut d = diagram();
    let before: Vec<Vec<(f64, f64)>> = d.wires.iter().map(|w| route(doc(&d.s), *w)).collect();
    let tall = height(&d.s, d.interface);
    // Liberation Sans, the face the default draws in.
    run(&mut d.s, "theme.apply", json!({"theme": "tidepool"}));
    assert_eq!(height(&d.s, d.interface), tall, "no height changed");
    let after: Vec<Vec<(f64, f64)>> = d.wires.iter().map(|w| route(doc(&d.s), *w)).collect();
    assert_eq!(after, before, "the routes are as they were");
}

/// A drawing saved under faces other than those it is opened under holds sizes that are stale. **An open recalculates nothing** (a cell keeps the value the file saved, and so a connector the
/// route the file holds), and an edit that names other shapes leaves them as they are; the edit that works the page out, a theme, gives the shapes the sizes of the faces and routes what is
/// glued to them, so no leg is left slanted.
#[test]
fn opening_a_drawing_whose_cached_sizes_are_stale_changes_no_route_and_the_next_recalculation_routes_to_the_new_sizes() {
    let mut d = diagram();
    let dir = crate::edit_foreign::Dir::new();
    let path = dir.path("stale.diacraft");
    run(&mut d.s, "file.save_as", json!({"path": path}));
    let saved: Vec<Vec<(f64, f64)>> = d.wires.iter().map(|w| route(doc(&d.s), *w)).collect();
    let tall = height(&d.s, d.interface);
    // Faces in which every line is twice as wide as in the ones the drawing was measured in.
    let mut db = FontDb::empty();
    assert_eq!(db.add_font_data(SynthFont::new("Liberation Sans").ascii(900).build()).added, 1);
    set_measure_fonts(Some(Arc::new(db)));
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    assert_eq!(height(&s, d.interface), tall, "the file's size stands, stale as it is");
    let opened: Vec<Vec<(f64, f64)>> = d.wires.iter().map(|w| route(doc(&s), *w)).collect();
    assert_eq!(opened, saved, "the routes are the file's");
    holds(&s, &d.wires, "as the file holds them");
    // A shape far from the diagram: the connectors are not touched.
    drop_shape(&mut s, "rectangle", 9.0, 9.0);
    assert_eq!(d.wires.iter().map(|w| route(doc(&s), *w)).collect::<Vec<_>>(), saved, "an edit that names other shapes leaves the routes");
    // The theme works the page out in the faces it has now: the interface is taller, and the connectors follow it.
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    assert!(height(&s, d.interface) > tall, "the recalculation gives the interface the height of the faces");
    // Right-angle legs only: a cell the file held that the theme did not change is left as the file has it, so the glue points that read such a cell are the file's.
    square(&s, &d.wires, "after the recalculation of the stale sizes");
}
