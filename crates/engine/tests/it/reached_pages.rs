//! The pages a commit reaches (the last of gap F40): **the commits a group's refit and the placing of a glued end make route the connectors of the pages they reach**, as the commit of
//! any other command does (`settle::commit`), and **the fields in the text of a reached page show what they read**, as those of the edited page do.
//!
//! Each case is held to what no change may break (`page_readers::exact`): a full evaluation changes no cell after the command, undo and redo give the drawing back exactly (the document compared
//! whole), and a save and a reopen give the pages what the session holds. The routing is held to **the drawing in which the shape was moved by a write of its own cell** (route and ends, cell for
//! cell), as `page_readers.rs` holds a command that moves a shape of another page.

use std::time::{Duration, Instant};

use diacraft_doc::{Owner, TextPart};
use diacraft_engine::Session;
use diacraft_testkit::build::{DocBuilder, MasterBuilder, ShapeBuilder};
use serde_json::{Value, json};

use crate::odg_build::{Dir, Drawing, id_of as odg_id_of, open_drawing};
use crate::page_readers::{counted, exact, timed_twin, value};
use crate::sheet_cross_pages::{settled, sheet_of};
use crate::{blank_session, check_session, run};

fn id_of(v: &Value) -> u32 {
    u32::try_from(v["id"].as_u64().unwrap()).unwrap()
}

// ---- the refit of a group ----------------------------------------------------------------------------------------------------------------------

/// Page "One" holds two rectangles in a group; page "Two" (2) holds two rectangles and a connector glued from the first to the second. With `reads` the first rectangle of page 2 follows the group's
/// `PinX` through `Pages[One]!` (it is where it was dropped, 2): a member of the group that moves refits the group, and the refit writes the group's frame cells. → the session, the member, the first
/// rectangle of page 2, the connector.
fn group_page_and_connector(reads: bool) -> (Session, u32, u32, u32) {
    let mut s = blank_session();
    run(&mut s, "page.rename", json!({"id": 1, "name": "One"}));
    let two = id_of(&run(&mut s, "page.add", json!({"name": "Two"})));
    assert_eq!(two, 2);
    let first = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 1.0, "y": 1.0})));
    let member = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 1.0})));
    let group = id_of(&run(&mut s, "shape.group", json!({"ids": [first, member], "page": 1})));
    let a = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": two, "x": 2.0, "y": 2.0})));
    let b = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": two, "x": 6.0, "y": 4.0})));
    let c = id_of(&run(&mut s, "connector.add", json!({"page": two, "from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    if reads {
        // The group's pin, as the engine gives it; the formula adds what puts the rectangle where it was dropped.
        let pin = run(&mut s, "shape.placement", json!({"ids": [group], "page": 1}))["shapes"][0]["x"].as_f64().unwrap();
        let formula = format!("Pages[One]!Sheet.{group}!PinX+{}", 2.0 - pin);
        run(&mut s, "sheet.set_cell", json!({"page": two, "id": a, "cell": "PinX", "formula": formula}));
    }
    (s, member, a, c)
}

/// The refit of a group routes the connectors glued to the shapes of the page that reads the group.
#[test]
fn a_refit_of_a_group_routes_the_connectors_of_the_page_that_reads_it() {
    let (mut through, member, a, c) = group_page_and_connector(true);
    let (mut direct, _, _, _) = group_page_and_connector(false);
    assert!(sheet_of(&through, 2, c) == sheet_of(&direct, 2, c), "the two drawings start alike");
    exact(&mut through, "shape.move", json!({"ids": [member], "page": 1, "dx": 3.0, "dy": 0.0}), |s| {
        assert_ne!(value(s, 2, a, "PinX"), json!(2.0), "the group's frame grew, and the rectangle that reads its pin follows");
    });
    let moved = value(&mut through, 2, a, "PinX");
    run(&mut direct, "sheet.set_cell", json!({"page": 2, "id": a, "cell": "PinX", "formula": moved.as_f64().unwrap().to_string()}));
    assert!(sheet_of(&through, 2, c) == sheet_of(&direct, 2, c), "the connector is where a direct move puts it, route and ends");
    assert_eq!(check_session(&through), Ok(()));
}

// ---- the placing of a glued end ------------------------------------------------------------------------------------------------------------------

/// A rectangle called `name` with `xml:id` and `draw:id` `id`, `w` by `h` cm at (`x`, `y`) cm from the page's top left.
fn rectangle(name: &str, id: &str, (x, y, w, h): (f64, f64, f64, f64)) -> String {
    format!(
        r#"<draw:rect draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" draw:name="{name}" xml:id="{id}" draw:id="{id}" svg:width="{w}cm" svg:height="{h}cm" svg:x="{x}cm" svg:y="{y}cm"><text:p/></draw:rect>"#
    )
}

/// A standard connector called `name` glued to the whole of the shapes `from` and `to`, with its ends stated (`x1` to `y2`, cm).
fn connector(name: &str, from: &str, to: &str, (x1, y1, x2, y2): (f64, f64, f64, f64)) -> String {
    format!(
        r#"<draw:connector draw:style-name="gr2" draw:text-style-name="P1" draw:layer="layout" draw:name="{name}" draw:type="standard" draw:start-shape="{from}" draw:end-shape="{to}" svg:x1="{x1}cm" svg:y1="{y1}cm" svg:x2="{x2}cm" svg:y2="{y2}cm"><text:p/></draw:connector>"#
    )
}

/// Two pages of an `.odg`, "page1" and "page2": A and B and a connector C glued to the whole of each on the first, D and E and a connector F glued to the whole of each on the second. The ends
/// of a connector glued to the whole of a shape hold values the engine places where the shape is, when the shape moves.
fn two_odg_pages() -> Drawing {
    let one = format!(
        "{}{}{}",
        rectangle("A", "a", (3.0, 10.0, 4.0, 2.0)),
        rectangle("B", "b", (8.0, 14.0, 3.0, 2.0)),
        connector("C", "a", "b", (5.0, 11.0, 9.5, 15.0))
    );
    let two = format!(
        "{}{}{}",
        rectangle("D", "d", (3.0, 10.0, 4.0, 2.0)),
        rectangle("E", "e", (12.0, 16.0, 3.0, 2.0)),
        connector("F", "d", "e", (5.0, 11.0, 13.5, 17.0))
    );
    Drawing::new().page(&one).page(&two)
}

/// The sheet of a connector with the cells of its ends as plain places, no trigger and no cell of a connector the application routes. `exact` saves the drawing as a VSDX file, which gives
/// an end glued to the whole of a shape the glue formulas and the trigger the application needs, and a connector of DiaCraft's the routable `ObjType` and `GlueType`, and the
/// drawing of an `.odg` becomes what that file reads as: the session that was saved holds them and the one that was not holds none.
fn plain_ends(sheet: &diacraft_doc::Sheet) -> diacraft_doc::Sheet {
    let mut plain = sheet.clone();
    for name in ["BeginX", "BeginY", "EndX", "EndY"] {
        if let Some(cell) = plain.cells.get_mut(name) {
            cell.formula = diacraft_doc::FormulaSlot::Absent;
            cell.unit = None;
        }
    }
    for name in ["BegTrigger", "EndTrigger", "ObjType", "GlueType"] {
        plain.cells.remove(name);
    }
    plain
}

/// The number a cell of a shape holds, in inches (the internal unit): `sheet.cell` shows it in the unit the page shows, centimetres for a page of an `.odg`.
fn inches(s: &Session, page: u32, id: u32, cell: &str) -> f64 {
    let doc = s.document().unwrap();
    let at = doc.find_shape(Owner::Page(page), id).unwrap();
    doc.shape(&at).unwrap().sheet.cell(cell).and_then(|c| c.value.as_ref()?.number()).unwrap()
}

/// What the session of [`two_odg_pages`] holds: the ids of B, of C (page 1), of D and F (page 2) and the ids of the two pages.
struct Odg {
    s: Session,
    pages: (u32, u32),
    b: u32,
    d: u32,
    f: u32,
}

/// The drawing, opened in `dir`. With `reads` the rectangle D follows the end of connector C through `Pages[page1]!` (it is where it was); without, its `PinX` is the number it is.
fn odg_session(dir: &Dir, reads: bool) -> Odg {
    let mut s = open_drawing(dir, &two_odg_pages());
    let (pages, b, c, d, f) = {
        let doc = s.document().unwrap();
        let pages = (doc.pages.get(0).unwrap().id, doc.pages.get(1).unwrap().id);
        (pages, odg_id_of(doc, 0, "B"), odg_id_of(doc, 0, "C"), odg_id_of(doc, 1, "D"), odg_id_of(doc, 1, "F"))
    };
    // Both drawings write the rectangle's `PinX` (the connector glued to it is placed the way the engine places it): the one a formula that reads the end, the other the number it is.
    let end_x = inches(&s, pages.0, c, "EndX");
    let pin_x = inches(&s, pages.1, d, "PinX");
    // The lengths are inches, said as such: the unit a bare number is read in is the one the cell shows, and a page of an `.odg` shows centimetres.
    let formula = if reads { format!("Pages[page1]!Sheet.{c}!EndX+({} in)", pin_x - end_x) } else { format!("{pin_x} in") };
    run(&mut s, "sheet.set_cell", json!({"page": pages.1, "id": d, "cell": "PinX", "formula": formula}));
    Odg { s, pages, b, d, f }
}

/// A shape that moves, with a connector glued to it by the whole of the shape, has the end of the connector placed by the engine, and the cells that read the end are worked out in a commit of their
/// own: the connectors glued to the shapes of the page that read it are routed again, and the end that is glued to the whole of a shape is placed where the shape is now.
#[test]
fn placing_a_glued_end_routes_the_connectors_of_the_page_that_reads_it() {
    let (dir_through, dir_direct) = (Dir::new(), Dir::new());
    let mut through = odg_session(&dir_through, true);
    let mut direct = odg_session(&dir_direct, false);
    let (p1, p2) = through.pages;
    assert!(sheet_of(&through.s, p2, through.f) == sheet_of(&direct.s, p2, direct.f), "the two drawings start alike");
    let (end0, pin0) = {
        let doc = through.s.document().unwrap();
        let (c, d) = (odg_id_of(doc, 0, "C"), through.d);
        (inches(&through.s, p1, c, "EndX"), inches(&through.s, p2, d, "PinX"))
    };
    exact(&mut through.s, "shape.move", json!({"ids": [through.b], "page": p1, "dx": 1.0, "dy": 0.0}), |s| {
        let (p1, p2) = (s.document().unwrap().pages.get(0).unwrap().id, s.document().unwrap().pages.get(1).unwrap().id);
        let (c, d) = {
            let doc = s.document().unwrap();
            (odg_id_of(doc, 0, "C"), odg_id_of(doc, 1, "D"))
        };
        let end = inches(s, p1, c, "EndX");
        let pin = inches(s, p2, d, "PinX");
        assert!(
            (pin - end - (pin0 - end0)).abs() < 1e-6 && (end - end0).abs() > 0.1,
            "the end of the connector moved, and the rectangle that reads it follows: {end} {pin}"
        );
    });
    let pin = inches(&through.s, p2, through.d, "PinX");
    run(&mut direct.s, "sheet.set_cell", json!({"page": p2, "id": direct.d, "cell": "PinX", "formula": format!("{pin} in")}));
    assert!(
        plain_ends(&sheet_of(&through.s, p2, through.f)) == plain_ends(&sheet_of(&direct.s, p2, direct.f)),
        "the connector of page 2 is where a direct move puts it, route and ends"
    );
    assert_eq!(check_session(&through.s), Ok(()));
}

/// The same drawing saved as a VSDX file (the `.odg`'s `page1` is written as `Page-1`, a name the writer chose and not the person) keeps meaning what it meant: the formula of the rectangle D
/// reads the end of the connector through the page by its new name, so that in the file, opened again, a move of B moves the end and D follows it.
#[test]
fn the_end_of_a_glued_connector_is_placed_through_the_page_the_vsdx_save_renamed() {
    let (dir_odg, dir_file) = (Dir::new(), Dir::new());
    let mut odg = odg_session(&dir_odg, true);
    run(&mut odg.s, "file.save_as", json!({"path": dir_file.path("renamed.vsdx")}));
    let mut file = Session::new();
    run(&mut file, "file.open", json!({"path": dir_file.path("renamed.vsdx")}));
    let (p1, p2, b, c, d) = {
        let doc = file.document().unwrap();
        let names: Vec<String> = doc.pages.iter().map(|p| p.name.to_string()).collect();
        assert_eq!(names, ["Page-1", "Page-2"], "the file holds the names the writer gave");
        (doc.pages.get(0).unwrap().id, doc.pages.get(1).unwrap().id, odg_id_of(doc, 0, "B"), odg_id_of(doc, 0, "C"), odg_id_of(doc, 1, "D"))
    };
    settled(file.document().unwrap());
    let (end0, pin0) = (inches(&file, p1, c, "EndX"), inches(&file, p2, d, "PinX"));
    run(&mut file, "shape.move", json!({"ids": [b], "page": p1, "dx": 1.0, "dy": 0.0}));
    let (end1, pin1) = (inches(&file, p1, c, "EndX"), inches(&file, p2, d, "PinX"));
    assert!((end1 - end0).abs() > 0.1, "the end of the connector moved: {end0} {end1}");
    assert!((pin1 - end1 - (pin0 - end0)).abs() < 1e-6, "the rectangle that reads the end through the renamed page follows it: {pin0} {pin1}");
    settled(file.document().unwrap());
    assert_eq!(check_session(&file), Ok(()));
}

// ---- a write to a sheet no page holds -----------------------------------------------------------------------------------------------------------

/// A master (`METER`: an `ID` the new drawing's own masters do not have) holds a shape 5 of width 1. Page "Two" (2) holds two rectangles and a connector glued from the first to the second, and, with `reads`, the first rectangle's `PinX` reads the width
/// of the master's shape by the master's name (`Masters[Meter]!`): it is where it was dropped (2), and no page holds an instance of the master. → the session, the first rectangle, the connector.
const METER: u32 = 900;

fn master_and_connector(reads: bool) -> (Session, u32, u32) {
    let mut s = blank_session();
    let two = id_of(&run(&mut s, "page.add", json!({"name": "Two"})));
    assert_eq!(two, 2);
    let a = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": two, "x": 2.0, "y": 2.0})));
    let b = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": two, "x": 6.0, "y": 4.0})));
    let c = id_of(&run(&mut s, "connector.add", json!({"page": two, "from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    let meter = MasterBuilder::new().name("Meter").shape(ShapeBuilder::new(5).cell("Width", "1"));
    let made = DocBuilder::new().master(METER, meter).build();
    s.edit("add the master", |doc| {
        for master in made.masters.iter() {
            doc.masters.push(master.clone());
        }
        Ok(())
    })
    .unwrap();
    if reads {
        run(&mut s, "sheet.set_cell", json!({"page": two, "id": a, "cell": "PinX", "formula": "Masters[Meter]!Sheet.5!Width+1"}));
    }
    (s, a, c)
}

/// A write to a cell of a master's shape that no page holds an instance of works out the cells of the pages that read it (`Masters[..]!`) in one commit, and **routes the connectors glued to
/// the shapes it moved**, as it does after any command.
#[test]
fn a_write_to_a_master_routes_the_connectors_of_a_page_that_reads_it_with_no_instance() {
    let (mut through, a, c) = master_and_connector(true);
    let (mut direct, _, _) = master_and_connector(false);
    run(&mut direct, "sheet.set_cell", json!({"page": 2, "id": a, "cell": "PinX", "formula": "2"}));
    assert!(sheet_of(&through, 2, c) == sheet_of(&direct, 2, c), "the two drawings start alike");
    exact(&mut through, "sheet.set_cell", json!({"master": METER, "id": 5, "cell": "Width", "formula": "4"}), |s| {
        assert_eq!(value(s, 2, a, "PinX"), json!(5.0), "the rectangle follows the master's width");
    });
    run(&mut direct, "sheet.set_cell", json!({"page": 2, "id": a, "cell": "PinX", "formula": "5"}));
    assert!(sheet_of(&through, 2, c) == sheet_of(&direct, 2, c), "the connector is where a direct move puts it, route and ends");
    assert_eq!(check_session(&through), Ok(()));
}

// ---- the fields in the text of a reached page --------------------------------------------------------------------------------------------------

/// The text a shape of page `page` shows: its characters and the results of its fields, in order.
fn shown(s: &Session, page: u32, id: u32) -> String {
    let doc = s.document().unwrap();
    let at = doc.find_shape(Owner::Page(page), id).unwrap();
    let shape = doc.shape(&at).unwrap();
    shape
        .text
        .as_ref()
        .map(|t| {
            t.parts
                .iter()
                .map(|p| match p {
                    TextPart::Chars(c) => c.to_string(),
                    TextPart::Fld { text, .. } => text.to_string(),
                    _ => String::new(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Page "One" holds a rectangle; a rectangle of page "Two" (2) holds a field that shows the width of it (a custom formula that names it through `Pages[One]!`). → the session, the rectangle of
/// page 1, the one of page 2.
fn a_field_that_reads_another_page() -> (Session, u32, u32) {
    let mut s = blank_session();
    run(&mut s, "page.rename", json!({"id": 1, "name": "One"}));
    let two = id_of(&run(&mut s, "page.add", json!({"name": "Two"})));
    assert_eq!(two, 2);
    let one = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 1.0, "y": 1.0})));
    let t = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": two, "x": 4.0, "y": 4.0})));
    run(&mut s, "text.insert_field", json!({"id": t, "page": two, "category": "custom", "formula": format!("Pages[One]!Sheet.{one}!Width")}));
    (s, one, t)
}

/// A shape of page 1 resized changes the width a field of page 2 reads: the cell of the field's row is worked out in the same commit, and the text the field shows follows it, as it would on
/// the edited page; a refresh of every field after it changes nothing.
#[test]
fn a_field_in_the_text_of_a_page_another_page_reached_shows_what_it_reads() {
    let (mut s, one, t) = a_field_that_reads_another_page();
    let before = shown(&s, 2, t);
    assert_ne!(before, "3.5", "the field shows the width the rectangle had: {before}");
    exact(&mut s, "shape.resize", json!({"id": one, "page": 1, "x": 1.0, "y": 1.0, "width": 3.5, "height": 1.0}), |s| {
        assert_eq!(shown(s, 2, t), "3.5", "the field shows the width it reads now");
    });
    assert_eq!(run(&mut s, "text.update_fields", json!({}))["updated"], json!(0), "no field is left that a refresh would change");
    assert_eq!(check_session(&s), Ok(()));
}

// ---- 256 pages, counted --------------------------------------------------------------------------------------------------------------------------

const PAGES: u32 = 256;

/// 256 pages, a field on each of the 255 after the first that shows the width of the rectangle on the first: a resize of it reaches every one of them in **one commit**, and the fields of
/// every reached page are refreshed within the budget of a call (at most as many shapes as an edit's own refresh looks at), the others left to `text.update_fields`.
fn a_resize_reaches_the_fields_of_many_pages() {
    let mut s = blank_session();
    let one = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 1.0, "y": 1.0})));
    run(&mut s, "page.rename", json!({"id": 1, "name": "One"}));
    let mut fields: Vec<(u32, u32)> = Vec::new();
    for _ in 2..=PAGES {
        let page = id_of(&run(&mut s, "page.add", json!({})));
        let t = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": page, "x": 4.0, "y": 4.0})));
        run(&mut s, "text.insert_field", json!({"id": t, "page": page, "category": "custom", "formula": format!("Pages[One]!Sheet.{one}!Width")}));
        fields.push((page, t));
    }
    let (_, cost) = counted(&mut s, "shape.resize", json!({"id": one, "page": 1, "x": 1.0, "y": 1.0, "width": 3.5, "height": 1.0}));
    eprintln!("{PAGES} pages that show a width: {cost:?}");
    assert_eq!(
        cost.commits,
        1 + (fields.len() as u64).min(256),
        "the commit of the resize, and one for the text of each field refreshed (at most 256): {cost:?}"
    );
    assert!(cost.steps <= 2_000 * u64::from(PAGES), "{cost:?}");
    // The first fields are refreshed; every one is right after `text.update_fields`.
    let (page, t) = fields[0];
    assert_eq!(shown(&s, page, t), "3.5");
    run(&mut s, "text.update_fields", json!({}));
    let (page, t) = fields[fields.len() - 1];
    assert_eq!(shown(&s, page, t), "3.5");
    settled(s.document().unwrap());
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_resize_reaches_the_fields_of_many_pages_within_the_budget_of_a_call() {
    a_resize_reaches_the_fields_of_many_pages();
}

timed_twin!(a_resize_reaches_the_fields_of_many_pages_is_timed, 60, a_resize_reaches_the_fields_of_many_pages);
