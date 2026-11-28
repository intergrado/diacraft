//! Every reference to a shape's connection rows kept when rows move or go (`draw::conn_refs`), and the marker of the points a doubling added
//! (`draw::conn_marker`, the User row `DiaCraftAddedPoints`), read safely.
//!
//! The command that halves points is; here are the pieces it calls. The tests build a page with two rectangles and three connectors glued to their
//! points, give the row map of a halving by hand, and hold what the walk does to the formulas and the `Connect` rows: the number or name of a row changes and no
//! other byte, a cell keeps its value and unit and its guard, a trigger is left, the marker is a list (moved terms renumbered, removed terms dropped), the walk is not
//! made for a map that moves nothing, and every loop is counted.

use std::sync::Arc;

use diacraft_calc::row_refs::RowName;
use diacraft_doc::{Cell, CellAddr, CellEdit, Connect, Connects, Document, Owner, Resolver, RowKey, Shape, ShapeRef, SheetRef, Value, WriteOrigin};
use diacraft_engine::Session;
use diacraft_engine::draw::conn_marker::{self, Found, MARKER_ROW, MAX_MARKER_BYTES, MAX_MARKER_TERMS, Marker, Term};
use diacraft_engine::draw::conn_refs::{
    Connected, EndMoved, Fate, MAX_SCAN_BYTES, MAX_SCAN_CELLS, Place, Renumbered, RowEntry, RowMap, ScanWork, renumber_connects, renumber_page,
};
use diacraft_engine::draw::{Problem, Which};
use proptest::prelude::*;
use serde_json::json;

use crate::edit::{drop_shape, id_of};
use crate::{blank_session, run};

/// A page with rectangle `a` at (3, 4) and rectangle `b` at (9, 6), and three connectors glued from `a` to `b`: `c1` from `a`'s point 3 to `b`'s point 1, `c2` from
/// point 0 to point 2, `c3` from point 1 to point 3 (a rectangle's points: 0 top, 1 left, 2 bottom, 3 right).
struct Scene {
    doc: Document,
    a: u32,
    b: u32,
    c1: u32,
    c2: u32,
    c3: u32,
}

/// The session the scene is made in, and the shapes.
fn scene_in() -> (Session, Scene) {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 9.0, 6.0);
    let join = |s: &mut Session, from: u32, to: u32| {
        id_of(&run(s, "connector.add", json!({"from": {"id": a, "point": from}, "to": {"id": b, "point": to}})))
    };
    let (c1, c2, c3) = (join(&mut s, 3, 1), join(&mut s, 0, 2), join(&mut s, 1, 3));
    let doc = s.document().unwrap().clone();
    (s, Scene { doc, a, b, c1, c2, c3 })
}

fn scene() -> Scene {
    scene_in().1
}

fn at(doc: &Document, id: u32) -> ShapeRef {
    doc.find_shape(Owner::Page(1), id).unwrap_or_else(|| panic!("no shape {id} on page 1"))
}

fn user(row: &str) -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(row), "Value")
}

/// The formula text a cell of shape `id` holds, by inheritance.
fn formula(doc: &Document, id: u32, addr: &CellAddr) -> Option<String> {
    let found = Resolver::new(doc).resolve(&SheetRef::Shape(at(doc, id)), addr)?;
    found.formula.cell.and_then(|c| c.formula.text().map(str::to_owned))
}

fn cell_of(doc: &Document, id: u32, addr: &CellAddr) -> Option<Cell> {
    Resolver::new(doc).resolve(&SheetRef::Shape(at(doc, id)), addr)?.formula.cell.cloned()
}

fn put(doc: &mut Document, id: u32, addr: &CellAddr, cell: Cell) {
    let sheet = SheetRef::Shape(at(doc, id));
    doc.write_cell(&sheet, addr, CellEdit::Cell(cell), WriteOrigin::Drawing).unwrap();
}

/// A user cell of shape `id` holding `text` as its formula, with a cached value and a unit, so that a rewrite that drops either is seen.
fn put_formula(doc: &mut Document, id: u32, row: &str, text: &str) {
    put(doc, id, &user(row), Cell::with_formula(text, Some(Value::from_text("2.5"))).with_unit("IN"));
}

/// Rows `from` to `to` (as `IX`) of shape `id`'s `Connection` section, each at (`IX`, 0).
fn put_points(doc: &mut Document, id: u32, from: u32, to: u32) {
    for ix in from..=to {
        for (cell, value) in [("X", f64::from(ix)), ("Y", 0.0)] {
            put(
                doc,
                id,
                &CellAddr::in_row("Connection", None, RowKey::Ix(ix), cell),
                Cell::constant(Value::from_text(&value.to_string())).with_unit("IN"),
            );
        }
    }
}

fn put_marker(doc: &mut Document, id: u32, text: &str) {
    put(doc, id, &conn_marker::marker_cell(), Cell::with_formula(text, Some(Value::from_text("0"))));
}

fn marker_text(doc: &Document, id: u32) -> Option<String> {
    formula(doc, id, &conn_marker::marker_cell())
}

fn entry(index: u32, fate: Fate) -> RowEntry {
    RowEntry { index, name: None, fate }
}

fn map_of(entries: Vec<RowEntry>) -> RowMap {
    RowMap::new(entries).unwrap()
}

/// What a halving that took a's left point (row 1) leaves: row 0 stays, row 1 goes to row 0, the two below move up.
fn left_removed() -> RowMap {
    map_of(vec![entry(0, Fate::Kept(0)), entry(1, Fate::removed_to(0)), entry(2, Fate::Kept(1)), entry(3, Fate::Kept(2))])
}

fn walk(doc: &mut Document, shape: u32, map: &RowMap) -> (Renumbered, ScanWork) {
    let mut work = ScanWork::new();
    let done = renumber_page(doc, 1, shape, map, &mut work).unwrap();
    (done, work)
}

fn connects_of(doc: &Document) -> Vec<Connect> {
    doc.page(1).unwrap().connects.iter().cloned().collect()
}

fn connect(doc: &Document, from: u32, from_part: u32) -> Connect {
    connects_of(doc)
        .into_iter()
        .find(|c| c.from_sheet == from && c.from_part == Some(from_part))
        .unwrap_or_else(|| panic!("no Connect row from {from} part {from_part}"))
}

fn refusal<T: std::fmt::Debug>(result: Result<T, Problem>) -> String {
    result.unwrap_err().message
}

fn glue(shape: u32, n: u32) -> String {
    format!("PAR(PNT(Sheet.{shape}!Connections.X{n},Sheet.{shape}!Connections.Y{n}))")
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------
// The formulas
// ---------------------------------------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_formula_of_another_shape_follows_its_row() {
    let mut sc = scene();
    let a = sc.a;
    put_formula(&mut sc.doc, sc.b, "Anchor", &format!("Sheet.{a}!Connections.X3+Sheet.{a}!Connections.Y3"));
    let before = cell_of(&sc.doc, sc.b, &user("Anchor")).unwrap();
    let (done, work) = walk(&mut sc.doc, a, &left_removed());
    assert_eq!(
        formula(&sc.doc, sc.b, &user("Anchor")).unwrap(),
        format!("Sheet.{a}!Connections.X2+Sheet.{a}!Connections.Y2"),
        "row 2 moved up to row 1"
    );
    // The cell keeps what it held but the formula's text: its value, its unit.
    let after = cell_of(&sc.doc, sc.b, &user("Anchor")).unwrap();
    assert_eq!((after.value.clone(), after.unit.clone()), (before.value.clone(), before.unit.clone()));
    // The glue formulas of the connectors on a's moved and removed points follow: c1 on row 3 (X4 to X3), c3 on the removed row 1 (X2 to its successor's X1);
    // c2 on row 0 and everything that names b stay as they were.
    let cell = |c: u32, name: &str| formula(&sc.doc, c, &CellAddr::direct(name)).unwrap();
    assert_eq!((cell(sc.c1, "BeginX"), cell(sc.c1, "BeginY")), (glue(a, 3), glue(a, 3)));
    assert_eq!((cell(sc.c3, "BeginX"), cell(sc.c3, "BeginY")), (glue(a, 1), glue(a, 1)));
    assert_eq!(cell(sc.c2, "BeginX"), glue(a, 1));
    assert_eq!(cell(sc.c1, "EndX"), glue(sc.b, 2));
    // The counts: the user cell and the four glue cells of c1 and c3.
    assert_eq!(done.formulas, 5);
    assert!(work.cells > 5 && work.bytes > 0, "the walk looked at the page's formulas: {work:?}");
}

#[test]
fn the_shapes_own_formula_follows_its_row() {
    let mut sc = scene();
    put_formula(&mut sc.doc, sc.a, "Own", "Connections.X3+Connections.Y4");
    put_formula(&mut sc.doc, sc.b, "Own", "Connections.X3+Connections.Y4");
    put_formula(&mut sc.doc, sc.b, "Named", &format!("Sheet.{}!Connections.Y4", sc.a));
    walk(&mut sc.doc, sc.a, &left_removed());
    assert_eq!(formula(&sc.doc, sc.a, &user("Own")).unwrap(), "Connections.X2+Connections.Y3", "a's own references name a's rows");
    assert_eq!(
        formula(&sc.doc, sc.b, &user("Own")).unwrap(),
        "Connections.X3+Connections.Y4",
        "b's own references name b's rows, which did not move"
    );
    assert_eq!(formula(&sc.doc, sc.b, &user("Named")).unwrap(), format!("Sheet.{}!Connections.Y3", sc.a));
}

#[test]
fn a_guard_a_string_and_a_reference_after_another_qualifier_are_kept_as_they_were() {
    let mut sc = scene();
    let a = sc.a;
    put_formula(&mut sc.doc, sc.b, "Guarded", &format!("GUARD(Sheet.{a}!Connections.X4)"));
    put_formula(&mut sc.doc, sc.b, "Text", &format!("\"Sheet.{a}!Connections.X4\"&Sheet.{a}!Connections.X4&Pages[Other]!Sheet.{a}!Connections.X4"));
    put_formula(&mut sc.doc, sc.b, "Plain", "Width*2");
    walk(&mut sc.doc, a, &left_removed());
    let guarded = cell_of(&sc.doc, sc.b, &user("Guarded")).unwrap();
    assert!(guarded.is_guarded(), "the guard is part of the text, which keeps every byte but the number");
    assert_eq!(guarded.formula.text().unwrap(), format!("GUARD(Sheet.{a}!Connections.X3)"));
    assert_eq!(
        formula(&sc.doc, sc.b, &user("Text")).unwrap(),
        format!("\"Sheet.{a}!Connections.X4\"&Sheet.{a}!Connections.X3&Pages[Other]!Sheet.{a}!Connections.X4"),
        "the string and the reference after another qualifier are left"
    );
    assert_eq!(formula(&sc.doc, sc.b, &user("Plain")).unwrap(), "Width*2");
}

#[test]
fn a_trigger_is_left_as_it_was() {
    let mut sc = scene();
    let a = sc.a;
    let trigger = format!("_XFTRIGGER(Sheet.{a}!EventXFMod)");
    put(&mut sc.doc, sc.c1, &CellAddr::direct("BegTrigger"), Cell::with_formula(&trigger, Some(Value::from_text("0"))));
    walk(&mut sc.doc, a, &left_removed());
    assert_eq!(formula(&sc.doc, sc.c1, &CellAddr::direct("BegTrigger")).unwrap(), trigger, "a trigger names the shape, which is not a row");
}

#[test]
fn a_row_named_by_its_name_keeps_its_name_when_it_moves_and_a_removed_one_takes_its_successors() {
    let mut sc = scene();
    let a = sc.a;
    put_formula(&mut sc.doc, sc.b, "ByName", &format!("Sheet.{a}!Connections.Row_5.X+Sheet.{a}!Connections.ROW_6.Y+Sheet.{a}!Connections.Row_7.X"));
    let map = map_of(vec![
        RowEntry { index: 0, name: Some("Row_5".into()), fate: Fate::Kept(0) },
        RowEntry { index: 1, name: Some("Row_6".into()), fate: Fate::Removed(Some(Place { index: 0, name: Some("Row_5".into()) })) },
        RowEntry { index: 2, name: Some("Row_7".into()), fate: Fate::Kept(1) },
    ]);
    walk(&mut sc.doc, a, &map);
    // Row_5 and Row_7 keep their names (the name is the row's, not its place); Row_6, written in another case, is gone and names its successor's name.
    assert_eq!(
        formula(&sc.doc, sc.b, &user("ByName")).unwrap(),
        format!("Sheet.{a}!Connections.Row_5.X+Sheet.{a}!Connections.Row_5.Y+Sheet.{a}!Connections.Row_7.X")
    );
}

#[test]
fn members_of_a_group_and_the_pages_own_sheet_are_walked_too() {
    let mut sc = scene();
    let a = sc.a;
    // A member of rectangle b, which names a's row by `Sheet.<a>!`, and a cell of the page's own sheet.
    let mut member = Shape::new(900);
    let mut sheet = diacraft_doc::Sheet::default();
    sheet.set_cell(&user("Mark"), Cell::with_formula(&format!("Sheet.{a}!Connections.X4"), Some(Value::from_text("1")))).unwrap();
    member.sheet = Arc::new(sheet);
    let b_at = {
        let page = Arc::make_mut(sc.doc.pages.get_mut(0).unwrap());
        let at = page.shapes.iter().position(|s| s.id == sc.b).unwrap();
        let parent = Arc::make_mut(page.shapes.get_mut(at).unwrap());
        parent.children.push(Arc::new(member));
        let mut page_sheet = (*page.sheet).clone();
        page_sheet.set_cell(&user("PageMark"), Cell::with_formula(&format!("Sheet.{a}!Connections.X4"), Some(Value::from_text("1")))).unwrap();
        page.sheet = Arc::new(page_sheet);
        u32::try_from(at).unwrap()
    };
    let (done, _) = walk(&mut sc.doc, a, &left_removed());
    let member_at = ShapeRef { owner: Owner::Page(1), path: vec![b_at, 0] };
    let member_cell = Resolver::new(&sc.doc).resolve(&SheetRef::Shape(member_at), &user("Mark")).unwrap();
    assert_eq!(member_cell.formula.cell.unwrap().formula.text().unwrap(), format!("Sheet.{a}!Connections.X3"));
    let page_cell = Resolver::new(&sc.doc).resolve(&SheetRef::Page(1), &user("PageMark")).unwrap();
    assert_eq!(page_cell.formula.cell.unwrap().formula.text().unwrap(), format!("Sheet.{a}!Connections.X3"));
    assert_eq!(done.formulas, 2 + 4, "the member's and the page's, and the four glue cells");
}

#[test]
fn a_shape_that_names_no_moved_row_is_not_copied() {
    let mut sc = scene();
    let before = sc.doc.clone();
    walk(&mut sc.doc, sc.a, &left_removed());
    let sheet_of = |doc: &Document, id: u32| Arc::clone(&doc.shape(&at(doc, id)).unwrap().sheet);
    for id in [sc.a, sc.b, sc.c2] {
        assert!(Arc::ptr_eq(&sheet_of(&before, id), &sheet_of(&sc.doc, id)), "shape {id} names none of the moved rows and keeps its sheet");
    }
    for id in [sc.c1, sc.c3] {
        assert!(!Arc::ptr_eq(&sheet_of(&before, id), &sheet_of(&sc.doc, id)), "connector {id} was rewritten");
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------
// The Connect rows
// ---------------------------------------------------------------------------------------------------------------------------------------------------------

/// The glue of a's own outward point, row 3, to the whole of b: a `Connect` row from a, the shape's own side.
fn add_own_side_glue(sc: &mut Scene) {
    let page = Arc::make_mut(sc.doc.pages.get_mut(0).unwrap());
    let mut list: Vec<Connect> = page.connects.iter().cloned().collect();
    list.push(Connect {
        from_sheet: sc.a,
        from_cell: Some("Connections.X4".into()),
        from_part: Some(103),
        to_sheet: sc.b,
        to_cell: Some("PinX".into()),
        to_part: Some(3),
        ..Connect::default()
    });
    page.connects = Connects::from_vec(list);
}

#[test]
fn a_connect_row_is_renumbered_on_the_to_side_and_on_the_from_side() {
    let mut sc = scene();
    let (a, b) = (sc.a, sc.b);
    add_own_side_glue(&mut sc);
    let mut work = ScanWork::new();
    let done = renumber_connects(&mut sc.doc, 1, a, &left_removed(), &mut work).unwrap();
    // To side: c1's begin was glued to row 3 and is now glued to row 2; c3's begin was on the removed row 1 and goes to its successor, row 0; c2's begin (row 0)
    // and every end on b are untouched.
    let c1 = connect(&sc.doc, sc.c1, 9);
    assert_eq!((c1.to_part, c1.to_cell.as_deref()), (Some(102), Some("Connections.X3")));
    let c3 = connect(&sc.doc, sc.c3, 9);
    assert_eq!((c3.to_part, c3.to_cell.as_deref()), (Some(100), Some("Connections.X1")));
    let c2 = connect(&sc.doc, sc.c2, 9);
    assert_eq!((c2.to_part, c2.to_cell.as_deref()), (Some(100), Some("Connections.X1")));
    let end = connect(&sc.doc, sc.c1, 12);
    assert_eq!((end.to_sheet, end.to_part), (b, Some(101)));
    // From side: the row of a's own outward glue moves with the row, and its other side (the whole of b) is not touched.
    let own = connects_of(&sc.doc).into_iter().find(|c| c.from_sheet == a).unwrap();
    assert_eq!((own.from_part, own.from_cell.as_deref()), (Some(102), Some("Connections.X3")));
    assert_eq!((own.to_sheet, own.to_part, own.to_cell.as_deref()), (b, Some(3), Some("PinX")), "the other side names a shape and no row of a");
    assert_eq!(
        done,
        Connected {
            ends: vec![
                EndMoved { connector: sc.c1, which: Some(Which::Begin), shape: a, from: 3, to: 2 },
                EndMoved { connector: sc.c3, which: Some(Which::Begin), shape: a, from: 1, to: 0 },
            ],
            own_side: 1,
        }
    );
    assert_eq!(work.ends, 3, "the rows rewritten are counted: two ends and the shape's own side");
}

#[test]
fn a_part_below_100_names_no_row_and_the_connect_rows_of_other_shapes_are_left() {
    let mut sc = scene();
    let before = connects_of(&sc.doc);
    let mut work = ScanWork::new();
    // A map for a shape that nothing is glued to: the connector c1, whose own rows have parts 9 and 12.
    let done = renumber_connects(&mut sc.doc, 1, sc.c1, &left_removed(), &mut work).unwrap();
    assert_eq!(connects_of(&sc.doc), before);
    assert_eq!((done, work.ends), (Connected { ends: Vec::new(), own_side: 0 }, 0));
}

#[test]
fn one_end_may_choose_its_own_row_and_the_others_take_the_successor() {
    let (mut s, mut sc) = scene_in();
    let a = sc.a;
    // A fourth connector on the same removed point, a's row 1: it takes the successor, row 0; c3, which says where it goes, goes to new row 1.
    let c4 = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 1}, "to": {"id": sc.b, "point": 0}})));
    sc.doc = s.document().unwrap().clone();
    let mut map = left_removed();
    map.glue_end(sc.c3, Which::Begin, 1);
    assert!(!map.is_identity());
    let mut work = ScanWork::new();
    renumber_connects(&mut sc.doc, 1, a, &map, &mut work).unwrap();
    walk(&mut sc.doc, a, &map);
    let c3 = connect(&sc.doc, sc.c3, 9);
    assert_eq!((c3.to_part, c3.to_cell.as_deref()), (Some(101), Some("Connections.X2")), "c3's begin is glued to new row 1, in the Connect row");
    assert_eq!(
        (formula(&sc.doc, sc.c3, &CellAddr::direct("BeginX")).unwrap(), formula(&sc.doc, sc.c3, &CellAddr::direct("BeginY")).unwrap()),
        (glue(a, 2), glue(a, 2))
    );
    let c4_row = connect(&sc.doc, c4, 9);
    assert_eq!((c4_row.to_part, c4_row.to_cell.as_deref()), (Some(100), Some("Connections.X1")), "c4's begin takes the successor");
    assert_eq!(formula(&sc.doc, c4, &CellAddr::direct("BeginX")).unwrap(), glue(a, 1));
}

#[test]
fn an_ends_own_choice_is_only_for_a_row_that_goes() {
    let mut sc = scene();
    let mut map = left_removed();
    // c1's begin is on row 3, which only moves up to row 2: the end's choice of row 0 is not asked.
    map.glue_end(sc.c1, Which::Begin, 0);
    let mut work = ScanWork::new();
    renumber_connects(&mut sc.doc, 1, sc.a, &map, &mut work).unwrap();
    walk(&mut sc.doc, sc.a, &map);
    assert_eq!(connect(&sc.doc, sc.c1, 9).to_part, Some(102));
    assert_eq!(formula(&sc.doc, sc.c1, &CellAddr::direct("BeginX")).unwrap(), glue(sc.a, 3));
}

#[test]
fn a_formula_that_names_a_removed_row_with_no_successor_is_refused_in_words() {
    let mut sc = scene();
    let a = sc.a;
    put_formula(&mut sc.doc, sc.b, "Anchor", &format!("Sheet.{a}!Connections.X2"));
    let map = map_of((0..4).map(|i| entry(i, Fate::Removed(None))).collect());
    let mut work = ScanWork::new();
    let words = refusal(renumber_page(&mut sc.doc, 1, a, &map, &mut work));
    assert_eq!(words, format!("halving would remove the last connection point of shape {a}, which the formula of Sheet.{}!User.Anchor names", sc.b));
    let mut work = ScanWork::new();
    let words = refusal(renumber_connects(&mut sc.doc, 1, a, &map, &mut work));
    assert_eq!(words, format!("halving would remove the last connection point of shape {a}, which a Connect row of shape {} names", sc.c1));
}

#[test]
fn a_page_or_shape_that_is_not_there_is_refused_in_words() {
    let mut sc = scene();
    let mut work = ScanWork::new();
    assert_eq!(refusal(renumber_page(&mut sc.doc, 7, sc.a, &left_removed(), &mut work)), "there is no page 7");
    assert_eq!(refusal(renumber_page(&mut sc.doc, 1, 999, &left_removed(), &mut work)), "page 1 has no shape 999");
    assert_eq!(refusal(renumber_connects(&mut sc.doc, 7, sc.a, &left_removed(), &mut work)), "there is no page 7");
}

#[test]
fn a_map_that_names_a_row_twice_or_is_too_large_is_refused_in_words() {
    assert_eq!(refusal(RowMap::new(vec![entry(2, Fate::Kept(0)), entry(2, Fate::Kept(1))])), "the row map names row 2 twice");
    let words = refusal(RowMap::new((0..65_537).map(|i| entry(i, Fate::Kept(i))).collect()));
    assert_eq!(words, "the row map holds more than 65536 rows");
    assert!(RowMap::new((0..65_536).map(|i| entry(i, Fate::Kept(i))).collect()).is_ok());
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------
// The marker as a list
// ---------------------------------------------------------------------------------------------------------------------------------------------------------

/// `a` with 10 rows and the marker `text`.
fn marked(text: &str) -> Scene {
    let mut sc = scene();
    put_points(&mut sc.doc, sc.a, 4, 9);
    put_marker(&mut sc.doc, sc.a, text);
    sc
}

#[test]
fn the_markers_terms_follow_moved_rows_and_lose_removed_ones() {
    let mut sc = marked("0*(1*Connections.X5+2*Connections.X6+1*Connections.X7)");
    let a = sc.a;
    // Row 4 (X5, generation 1) is removed; rows 5 and 6 move up to 4 and 5; the rest follow.
    let mut entries: Vec<RowEntry> = (0..4).map(|i| entry(i, Fate::Kept(i))).collect();
    entries.push(entry(4, Fate::removed_to(0)));
    entries.extend((5..10).map(|i| entry(i, Fate::Kept(i - 1))));
    let (done, _) = walk(&mut sc.doc, a, &map_of(entries));
    // A removed row's term is dropped and not mapped to its successor (which would make an original an added point); the others are renumbered in their order.
    assert_eq!(marker_text(&sc.doc, a).unwrap(), "0*(2*Connections.X5+1*Connections.X6)");
    assert_eq!((done.terms_moved, done.terms_dropped), (2, 1));
    assert_eq!(done.formulas, 0, "the marker is not counted among the formulas that were rewritten");
    // The cell keeps its value, and reads back as the marker it says.
    let read = conn_marker::read_in(&sc.doc, &at(&sc.doc, a));
    assert_eq!(
        (read.generation_of(&RowName::Number(5)), read.generation_of(&RowName::Number(6)), read.generation_of(&RowName::Number(7))),
        (2, 1, 0)
    );
}

#[test]
fn a_marker_with_every_term_removed_goes_and_takes_its_emptied_section_with_it() {
    let mut sc = marked("0*(1*Connections.X5)");
    let a = sc.a;
    let mut entries: Vec<RowEntry> = (0..10).map(|i| entry(i, Fate::Kept(i))).collect();
    entries[4] = entry(4, Fate::removed_to(3));
    walk(&mut sc.doc, a, &map_of(entries));
    assert_eq!(marker_text(&sc.doc, a), None);
    let sheet = &sc.doc.shape(&at(&sc.doc, a)).unwrap().sheet;
    assert!(sheet.sections.iter().all(|s| s.name.as_str() != "User"), "the User section the marker made is gone with its last row");
}

#[test]
fn the_markers_terms_that_name_no_row_or_a_row_twice_or_have_no_generation_are_dropped_by_a_renumbering() {
    let mut sc = marked("0*(1*Connections.X5+1*Connections.X5+0*Connections.X6+3*Connections.X99+2*Connections.X8)");
    let a = sc.a;
    let mut entries: Vec<RowEntry> = (0..10).map(|i| entry(i, Fate::Kept(i))).collect();
    entries[9] = entry(9, Fate::removed_to(0));
    // Row 4 is named twice, row 5 has generation 0, row 98 is no row of the map, row 7 (X8) is kept where it is: only X8 stays.
    let (done, _) = walk(&mut sc.doc, a, &map_of(entries));
    assert_eq!(marker_text(&sc.doc, a).unwrap(), "0*(2*Connections.X8)");
    assert_eq!((done.terms_moved, done.terms_dropped), (0, 4));
}

#[test]
fn a_marker_of_another_form_is_left_as_it_is() {
    let text = "0*(1*Connections.X5+#REF!)";
    let mut sc = marked(text);
    let a = sc.a;
    let mut entries: Vec<RowEntry> = (0..10).map(|i| entry(i, Fate::Kept(i))).collect();
    entries[4] = entry(4, Fate::Kept(2));
    let (done, _) = walk(&mut sc.doc, a, &map_of(entries));
    assert_eq!(
        marker_text(&sc.doc, a).unwrap(),
        text,
        "a marker no reader takes as ours is not ours to rewrite, and the walk does not read it as a formula"
    );
    assert_eq!((done.terms_moved, done.terms_dropped, done.formulas), (0, 0, 0));
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------
// Work, counted
// ---------------------------------------------------------------------------------------------------------------------------------------------------------

#[test]
fn the_walk_is_not_made_for_an_identity_map() {
    let mut sc = scene();
    let a = sc.a;
    put_formula(&mut sc.doc, sc.b, "Anchor", &format!("Sheet.{a}!Connections.X3"));
    let before = sc.doc.clone();
    let map = map_of((0..4).map(|i| entry(i, Fate::Kept(i))).collect());
    assert!(map.is_identity());
    let (done, work) = walk(&mut sc.doc, a, &map);
    assert_eq!(sc.doc, before, "a doubling moves no row and no reference changes");
    assert_eq!((done.formulas, done.terms_moved, done.terms_dropped), (0, 0, 0));
    assert_eq!((work.cells, work.bytes, work.ends), (0, 0, 0), "no formula was looked at");
    let mut work = ScanWork::new();
    let connected = renumber_connects(&mut sc.doc, 1, a, &map, &mut work).unwrap();
    assert_eq!((connected.ends.len(), connected.own_side, work.ends), (0, 0, 0));
    assert!(!left_removed().is_identity());
}

#[test]
fn the_walk_is_refused_past_its_cells_and_past_its_bytes_by_its_words_and_by_the_count() {
    let words = "the page holds too many formulas to look through for references to the points";
    let mut sc = scene();
    let mut work = ScanWork::limited(5, usize::MAX, usize::MAX);
    assert_eq!(refusal(renumber_page(&mut sc.doc, 1, sc.a, &left_removed(), &mut work)), words);
    assert_eq!(work.cells, 5, "the cells looked at are counted up to the refusal");
    let mut sc = scene();
    let mut work = ScanWork::limited(usize::MAX, 100, usize::MAX);
    assert_eq!(refusal(renumber_page(&mut sc.doc, 1, sc.a, &left_removed(), &mut work)), words);
    assert!(work.bytes <= 100 && work.bytes > 0, "{work:?}");
    // The caps a call has.
    let fresh = ScanWork::new();
    assert_eq!((fresh.cells, fresh.bytes, fresh.ends), (0, 0, 0));
    assert_eq!((MAX_SCAN_CELLS, MAX_SCAN_BYTES), (4_000_000, 64 * 1024 * 1024));
}

#[test]
fn the_glued_ends_renumbered_are_counted_and_refused_past_their_cap() {
    let mut sc = scene();
    let mut work = ScanWork::limited(usize::MAX, usize::MAX, 1);
    let words = refusal(renumber_connects(&mut sc.doc, 1, sc.a, &left_removed(), &mut work));
    assert_eq!(words, format!("more than 1 connector ends are glued to the points of shape {}", sc.a));
    assert_eq!(work.ends, 1);
}

/// 30,000 shapes, each with a user cell naming a's row 3 by `Sheet.<a>!`.
fn thirty_thousand(sc: &mut Scene, text: &str) {
    let page = Arc::make_mut(sc.doc.pages.get_mut(0).unwrap());
    for i in 0..30_000_u32 {
        let mut shape = Shape::new(1_000 + i);
        let mut sheet = diacraft_doc::Sheet::default();
        sheet.set_cell(&user("Mark"), Cell::with_formula(text, Some(Value::from_text("1")))).unwrap();
        shape.sheet = Arc::new(sheet);
        page.shapes.push(Arc::new(shape));
    }
}

#[test]
fn a_page_of_thirty_thousand_shapes_each_naming_a_row_is_walked_once_and_counted() {
    let mut sc = scene();
    let a = sc.a;
    let text = format!("Sheet.{a}!Connections.X4");
    thirty_thousand(&mut sc, &text);
    let (done, work) = walk(&mut sc.doc, a, &left_removed());
    assert_eq!(done.formulas, 30_000 + 4, "every shape's cell, and the four glue cells");
    assert!(work.cells >= 30_000 && work.cells < 31_000, "one cell for each formula looked at, once: {work:?}");
    assert!(work.bytes >= 30_000 * text.len(), "{work:?}");
    let last = at(&sc.doc, 1_000 + 29_999);
    let found = Resolver::new(&sc.doc).resolve(&SheetRef::Shape(last), &user("Mark")).unwrap();
    assert_eq!(found.formula.cell.unwrap().formula.text().unwrap(), format!("Sheet.{a}!Connections.X3"));
}

#[test]
fn a_formula_of_65536_bytes_of_references_is_rewritten_within_a_counted_budget() {
    let mut sc = scene();
    let a = sc.a;
    let token = format!("Sheet.{a}!Connections.X4+");
    let mut text = token.repeat(65_536 / token.len() + 1);
    text.truncate(65_536 - 1);
    text.push('1');
    put_formula(&mut sc.doc, sc.b, "Big", &text);
    let refs = text.matches("Connections.X4").count();
    let (done, work) = walk(&mut sc.doc, a, &left_removed());
    assert_eq!(done.formulas, 1 + 4);
    let out = formula(&sc.doc, sc.b, &user("Big")).unwrap();
    assert_eq!(out.matches("Connections.X3").count(), refs);
    assert_eq!(out.len(), text.len());
    // The bytes are counted: one pass to know the formula names the section, one to read it, and the bytes written for each reference.
    assert!(work.bytes >= 2 * text.len() + 2 * refs && work.bytes < 2 * text.len() + 2 * refs + 16_384, "{work:?}");
}

#[test]
fn references_to_65536_rows_are_looked_up_in_a_map_of_65536_rows_once_each() {
    let mut sc = scene();
    let a = sc.a;
    let text: String = (1..=65_536_u32).map(|n| format!("Sheet.{a}!Connections.X{n}")).collect::<Vec<_>>().join("+");
    put_formula(&mut sc.doc, sc.b, "Many", &text);
    // Every row moves up by one, and the first goes: a map the size of the largest section.
    let mut entries = vec![entry(0, Fate::removed_to(0))];
    entries.extend((1..65_536_u32).map(|i| entry(i, Fate::Kept(i - 1))));
    let (done, work) = walk(&mut sc.doc, a, &map_of(entries));
    assert_eq!(done.formulas, 1 + 4);
    let out = formula(&sc.doc, sc.b, &user("Many")).unwrap();
    assert!(out.starts_with(&format!("Sheet.{a}!Connections.X1+Sheet.{a}!Connections.X1+Sheet.{a}!Connections.X2+")), "{}", &out[..120]);
    assert!(out.ends_with(&format!("Sheet.{a}!Connections.X65535")), "the last row moved up: {}", &out[out.len() - 60..]);
    assert!(work.bytes >= 2 * text.len() && work.bytes < 3 * text.len() + 1_000_000, "{work:?}");
}

#[test]
fn nested_parentheses_and_floods_of_ref_errors_cost_their_length_and_no_more() {
    let mut sc = scene();
    let a = sc.a;
    let nested = format!("{}Sheet.{a}!Connections.X4{}", "(".repeat(8_000), ")".repeat(8_000));
    let flood = "#REF!+".repeat(2_000);
    put_formula(&mut sc.doc, sc.b, "Deep", &nested);
    put_formula(&mut sc.doc, sc.b, "Flood", &format!("{flood}Sheet.{a}!Connections.X4"));
    let (done, work) = walk(&mut sc.doc, a, &left_removed());
    assert_eq!(done.formulas, 2 + 4);
    assert!(formula(&sc.doc, sc.b, &user("Deep")).unwrap().contains("Connections.X3"));
    assert!(formula(&sc.doc, sc.b, &user("Flood")).unwrap().ends_with("Connections.X3"));
    assert!(work.bytes < 4 * (nested.len() + flood.len()) + 16_384, "{work:?}");
}

#[test]
#[ignore = "timed: run by hand, `cargo test -p diacraft-engine --test it conn_refs -- --ignored --nocapture`"]
fn the_hostile_inputs_finish_beside_their_yardstick() {
    use std::time::Instant;
    let mut sc = scene();
    let a = sc.a;
    thirty_thousand(&mut sc, &format!("Sheet.{a}!Connections.X4"));
    // The yardstick: copying the document, which touches every list of it once.
    let t = Instant::now();
    let copy = sc.doc.clone();
    let yardstick = t.elapsed();
    drop(copy);
    let t = Instant::now();
    walk(&mut sc.doc, a, &left_removed());
    let walked = t.elapsed();
    let marker: String = format!("0*({})", "1*Connections.X5+".repeat(480));
    let t = Instant::now();
    for _ in 0..1_000 {
        assert!(conn_marker::parse(&marker, &anywhere).is_empty());
    }
    let parsed = t.elapsed();
    eprintln!("yardstick {yardstick:?}; 30,000 shapes walked {walked:?}; 1,000 hostile markers read {parsed:?}");
    assert!(walked.as_secs_f64() < 5.0 + 1_000.0 * yardstick.as_secs_f64(), "the walk took {walked:?}, the yardstick {yardstick:?}");
    assert!(parsed.as_secs_f64() < 5.0, "{parsed:?}");
}

#[test]
#[ignore = "timed: run by hand, `cargo test -p diacraft-engine --test it conn_refs -- --ignored --nocapture`"]
fn references_to_a_name_that_65536_rows_share_cost_what_references_to_a_name_no_row_has_cost() {
    use std::time::Instant;
    // Ten formulas of 64 KiB of `Connections.TALLY.X`, a name in a case no row spells: every reference is a lookup by the folded name.
    let page = |name: &dyn Fn(u32) -> String| {
        let mut sc = scene();
        let a = sc.a;
        let token = format!("Sheet.{a}!Connections.TALLY.X+");
        let mut text = token.repeat(65_000 / token.len());
        text.push('1');
        for i in 0..10 {
            put_formula(&mut sc.doc, sc.b, &format!("Big{i}"), &text);
        }
        // Row 0 goes and the rest move up, so the walk is made; a named row that stays is left as it is.
        let mut entries = vec![entry(0, Fate::removed_to(1))];
        entries.extend((1..65_536_u32).map(|i| RowEntry { index: i, name: Some(name(i)), fate: Fate::Kept(i - 1) }));
        let refs = 10 * text.matches("TALLY").count();
        (sc, a, map_of(entries), refs)
    };
    // The yardstick: each of the 65,535 rows has a name of its own and none is `tally`, so a lookup finds nothing.
    let (mut sc, a, map, refs) = page(&|i| format!("Row{i}"));
    let t = Instant::now();
    walk(&mut sc.doc, a, &map);
    let alone = t.elapsed();
    let (mut sc, a, map, _) = page(&|_| "Tally".to_owned());
    let t = Instant::now();
    walk(&mut sc.doc, a, &map);
    let shared = t.elapsed();
    eprintln!("{refs} references: to a name no row has {alone:?}; to a name 65,535 rows share {shared:?}");
    assert!(shared.as_secs_f64() < 2.0 + 20.0 * alone.as_secs_f64(), "{shared:?} against {alone:?}");
}

/// Every row exists, and a name is the one row in any letter case: what a reader with no section to ask would say. A row is told by its number, or by a number made of its
/// name (FNV-1a of it in lower case), which is all a test with no section needs.
fn anywhere(row: &RowName) -> Option<Found> {
    let identity = match row {
        RowName::Number(n) => *n,
        RowName::Name(name) => name.to_ascii_lowercase().bytes().fold(0x811c_9dc5_u32, |hash, b| (hash ^ u32::from(b)).wrapping_mul(0x0100_0193)),
        RowName::Default => return None,
    };
    Some(Found { spelling: row.clone(), identity })
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------
// The marker: read as design 3.1 shows and says
// ---------------------------------------------------------------------------------------------------------------------------------------------------------

fn read_marker(text: &str) -> Marker {
    let mut sc = scene();
    put_points(&mut sc.doc, sc.a, 4, 9);
    put_marker(&mut sc.doc, sc.a, text);
    conn_marker::read_in(&sc.doc, &at(&sc.doc, sc.a))
}

fn generations(marker: &Marker, rows: &[u32]) -> Vec<u8> {
    rows.iter().map(|n| marker.generation_of(&RowName::Number(*n))).collect()
}

#[test]
fn a_marker_is_read_as_the_design_shows_and_a_row_it_does_not_name_is_an_original() {
    let m = read_marker("0*(1*Connections.X5+1*Connections.X6+2*Connections.X9)");
    assert_eq!(m.terms().len(), 3);
    assert_eq!(generations(&m, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]), [0, 0, 0, 0, 1, 1, 0, 0, 2, 0]);
    assert_eq!(m.newest(), Some(2));
    assert_eq!(m.text().as_deref(), Some("0*(1*Connections.X5+1*Connections.X6+2*Connections.X9)"));
    let none = Marker::empty();
    assert!(none.is_empty() && none.newest().is_none() && none.text().is_none());
}

#[test]
fn a_shape_with_no_marker_has_every_point_original() {
    let sc = scene();
    let m = conn_marker::read_in(&sc.doc, &at(&sc.doc, sc.a));
    assert!(m.is_empty());
    assert_eq!(generations(&m, &[1, 2, 3, 4]), [0, 0, 0, 0]);
}

#[test]
fn a_term_that_breaks_a_rule_makes_its_row_an_original_and_the_others_stand() {
    // Generation 0, 256 and 2^32; a row named twice (both terms void); a row the section does not have; the others stand.
    for text in [
        "0*(0*Connections.X5+1*Connections.X6)",
        "0*(256*Connections.X5+1*Connections.X6)",
        "0*(4294967296*Connections.X5+1*Connections.X6)",
        "0*(1*Connections.X5+2*Connections.X5+1*Connections.X6)",
        "0*(1*Connections.X20+1*Connections.X6)",
        "0*(1*Connections.X0+1*Connections.X6)",
    ] {
        let m = read_marker(text);
        assert_eq!(generations(&m, &[5, 6]), [0, 1], "{text}");
        assert_eq!(m.terms().len(), 1, "{text}");
    }
    // The largest generation is allowed.
    assert_eq!(generations(&read_marker("0*(255*Connections.X5)"), &[5]), [255]);
}

#[test]
fn a_term_of_no_generation_still_names_its_row_so_a_row_named_twice_is_an_original() {
    // The row's other term gives no generation (0, 256, 2^32) and still names it: two terms name row 5, and neither stands. Row 6 stands.
    for text in [
        "0*(1*Connections.X5+0*Connections.X5+1*Connections.X6)",
        "0*(0*Connections.X5+1*Connections.X5+1*Connections.X6)",
        "0*(1*Connections.X5+256*Connections.X5+1*Connections.X6)",
        "0*(4294967296*Connections.X5+1*Connections.X5+1*Connections.X6)",
    ] {
        let m = read_marker(text);
        assert_eq!(generations(&m, &[5, 6]), [0, 1], "{text}");
        assert_eq!(m.terms().len(), 1, "{text}");
    }
    // The review's own: row 1 named by a term of generation 1 and one of generation 0.
    let m = read_marker("0*(1*Connections.X1+0*Connections.X1+1*Connections.X2)");
    assert_eq!(generations(&m, &[1, 2]), [0, 1]);
    // A term that names no row does not count against one that does.
    let m = read_marker("0*(1*Connections.X5+0*Connections.X99+1*Connections.X6)");
    assert_eq!(generations(&m, &[5, 6]), [1, 1]);
}

#[test]
fn a_number_and_a_name_for_the_one_row_are_two_terms_for_it_in_a_section_with_a_named_row() {
    let mut sc = scene();
    // The rectangle's rows are numbers 1 to 4; `Tally`, named, is the fifth by its place (`X5`).
    let tally = CellAddr::in_row("Connection", None, RowKey::name("Tally"), "X");
    put(&mut sc.doc, sc.a, &tally, Cell::constant(Value::from_text("1")).with_unit("IN"));
    let mut read = |text: &str| {
        put_marker(&mut sc.doc, sc.a, text);
        conn_marker::read_in(&sc.doc, &at(&sc.doc, sc.a))
    };
    let tally = |m: &Marker| m.generation_of(&RowName::Name("tally".into()));
    for text in [
        "0*(1*Connections.X5+1*Connections.Tally.X)",
        "0*(1*Connections.Tally.X+1*Connections.X5)",
        "0*(1*Connections.Tally.X+0*Connections.X5)",
        "0*(1*Connections.TALLY.X+1*Connections.tally.X)",
        "0*(1*Connections.Tally.X+1*Connections.X5+1*Connections.tally.X)",
    ] {
        let m = read(text);
        assert_eq!(tally(&m), 0, "{text}: named twice, so an original");
        assert!(m.is_empty(), "{text}: {m:?}");
    }
    // The terms of other rows stand beside it, and a term by the name alone stands.
    let m = read("0*(1*Connections.Tally.X+1*Connections.X5+3*Connections.X2)");
    assert_eq!((tally(&m), m.generation_of(&RowName::Number(2))), (0, 3));
    let m = read("0*(2*Connections.tally.X)");
    assert_eq!(m.terms(), [Term { row: RowName::Name("Tally".into()), generation: 2 }]);
}

#[test]
fn a_term_naming_a_deleted_row_is_void() {
    let mut sc = scene();
    put_points(&mut sc.doc, sc.a, 4, 9);
    // Row 6 is deleted in the shape (`Del`).
    {
        let at = at(&sc.doc, sc.a);
        let page = Arc::make_mut(sc.doc.pages.get_mut(0).unwrap());
        let shape = Arc::make_mut(page.shapes.get_mut(at.path[0] as usize).unwrap());
        let sheet = Arc::make_mut(&mut shape.sheet);
        let section = sheet.sections.iter_mut().find(|s| s.name.as_str() == "Connection").unwrap();
        let section = Arc::make_mut(section);
        let row = section.rows.iter_mut().find(|r| r.ix == Some(6)).unwrap();
        Arc::make_mut(row).del = true;
    }
    put_marker(&mut sc.doc, sc.a, "0*(1*Connections.X7+1*Connections.X8)");
    let m = conn_marker::read_in(&sc.doc, &at(&sc.doc, sc.a));
    assert_eq!(generations(&m, &[7, 8]), [0, 1]);
}

#[test]
fn a_marker_of_another_form_or_with_a_ref_error_makes_every_point_an_original() {
    for text in [
        "0*(1*Connections.X5+1*#REF!)",
        "0*(#REF!)",
        "#REF!",
        "1*Connections.X5",
        "0*(1*Connections.X5",
        "0*()",
        "0*(1*Connections.X5+)",
        "0*(1*Connections.X5+1*Connections.X6)+0",
        "0*(1*connections.X5)",
        "0*(1*Connections.x5)",
        "0*(1*Connections.X5 + 1*Connections.X6)",
        "0*(01*Connections.X5)",
        "0*(1*Connections.X05)",
        "0*(1*Connections.X)",
        "0*(1*Sheet.1!Connections.X5)",
        "0*(1*Connections.DirX5)",
        "0*(1*Connections.Row_3.Y)",
        "0*(+1*Connections.X5)",
        "0*(-1*Connections.X5)",
        "0*(1*Connections.X4294967296)",
    ] {
        let m = read_marker(text);
        assert!(m.is_empty(), "{text}: {m:?}");
    }
}

#[test]
fn a_row_named_by_its_name_is_read_in_any_case_and_kept_as_the_row_spells_it() {
    let mut sc = scene();
    for name in ["Row_3", "Row_4"] {
        put(&mut sc.doc, sc.a, &CellAddr::in_row("Connection", None, RowKey::name(name), "X"), Cell::constant(Value::from_text("1")).with_unit("IN"));
    }
    put_marker(&mut sc.doc, sc.a, "0*(1*Connections.row_3.X+2*Connections.ROW_4.X)");
    let m = conn_marker::read_in(&sc.doc, &at(&sc.doc, sc.a));
    assert_eq!(m.terms(), [Term { row: RowName::Name("Row_3".into()), generation: 1 }, Term { row: RowName::Name("Row_4".into()), generation: 2 }]);
    // The terms are in the section's spelling, and a generation is found by it: `row_4` is no more the row `Row_4` than it is a row of its own.
    assert_eq!(m.generation_of(&RowName::Name("Row_4".into())), 2);
    assert_eq!(m.generation_of(&RowName::Name("row_4".into())), 0);
    assert_eq!(m.text().as_deref(), Some("0*(1*Connections.Row_3.X+2*Connections.Row_4.X)"));
}

#[test]
fn a_name_is_read_by_the_row_that_spells_it_so_before_any_other_case_among_rows_that_differ_only_in_case() {
    // `Row_3`, `ROW_3` and `row_3` are three rows. A name names the row of that spelling, and one spelled in a case no row has names the first.
    let mut sc = scene();
    for name in ["Row_3", "ROW_3", "row_3"] {
        put(&mut sc.doc, sc.a, &CellAddr::in_row("Connection", None, RowKey::name(name), "X"), Cell::constant(Value::from_text("1")).with_unit("IN"));
    }
    for (spelled, row) in [("Row_3", "Row_3"), ("ROW_3", "ROW_3"), ("row_3", "row_3"), ("rOw_3", "Row_3"), ("rOW_3", "Row_3")] {
        put_marker(&mut sc.doc, sc.a, &format!("0*(2*Connections.{spelled}.X)"));
        let m = conn_marker::read_in(&sc.doc, &at(&sc.doc, sc.a));
        assert_eq!(m.terms(), [Term { row: RowName::Name(row.into()), generation: 2 }], "{spelled}");
        // The generation is the row's own, and a row of another case is an original.
        for other in ["Row_3", "ROW_3", "row_3"] {
            assert_eq!(m.generation_of(&RowName::Name(other.into())), if other == row { 2 } else { 0 }, "{spelled}: {other}");
        }
    }
}

/// A map of seven rows that closes a gap: row 1 goes to row 0 and rows 2 to 6 move up one. Row `at` has the name `name` when one is given.
fn gap_map(named: Option<(usize, &str)>) -> RowMap {
    let mut entries = vec![entry(0, Fate::Kept(0)), entry(1, Fate::removed_to(0))];
    entries.extend((2..7_u32).map(|i| entry(i, Fate::Kept(i - 1))));
    if let Some((at, name)) = named {
        entries.get_mut(at).unwrap().name = Some(name.into());
    }
    map_of(entries)
}

/// What a walk of the page for shape `a` under `map` makes of the marker `text` it holds: the marker's text after, and the walk's count of terms moved and dropped.
fn renumbered_marker(map: &RowMap, text: &str) -> (Option<String>, usize, usize) {
    let mut sc = scene();
    put_marker(&mut sc.doc, sc.a, text);
    let (done, _) = walk(&mut sc.doc, sc.a, map);
    (marker_text(&sc.doc, sc.a), done.terms_moved, done.terms_dropped)
}

#[test]
fn two_terms_that_spell_a_name_alike_but_for_its_case_stand_for_neither_though_they_name_two_rows() {
    // `Tally` and `TALLY` are two rows, and the marker reads a name in any letter case, so a term of each is read as an original's.
    let mut sc = scene();
    for name in ["Tally", "TALLY"] {
        put(&mut sc.doc, sc.a, &CellAddr::in_row("Connection", None, RowKey::name(name), "X"), Cell::constant(Value::from_text("1")).with_unit("IN"));
    }
    put_marker(&mut sc.doc, sc.a, "0*(1*Connections.Tally.X+2*Connections.TALLY.X)");
    assert!(conn_marker::read_in(&sc.doc, &at(&sc.doc, sc.a)).is_empty());
    put_marker(&mut sc.doc, sc.a, "0*(2*Connections.TALLY.X)");
    assert_eq!(conn_marker::read_in(&sc.doc, &at(&sc.doc, sc.a)).terms(), [Term { row: RowName::Name("TALLY".into()), generation: 2 }]);
    // The same on a map: rows 4 and 5 of it are the two.
    let mut entries: Vec<RowEntry> = (0..4).map(|i| entry(i, Fate::Kept(i))).collect();
    entries.push(RowEntry { index: 4, name: Some("Tally".into()), fate: Fate::Kept(4) });
    entries.push(RowEntry { index: 5, name: Some("TALLY".into()), fate: Fate::Kept(5) });
    entries.push(entry(6, Fate::removed_to(0)));
    let (text, ..) = renumbered_marker(&map_of(entries), "0*(1*Connections.Tally.X+2*Connections.TALLY.X)");
    assert_eq!(text, None, "both terms are dropped, and the marker goes with the last");
}

#[test]
fn a_renumbering_finds_the_row_of_a_term_by_its_exact_spelling_before_any_other_case() {
    // `ROW_5` (IX 4) stays, `Row_5` (IX 5) goes and `Row_6` (IX 6) moves up. The term of `Row_5` is of the row that goes, and not of `ROW_5`, which is first.
    let mut entries: Vec<RowEntry> = (0..4).map(|i| entry(i, Fate::Kept(i))).collect();
    entries.push(RowEntry { index: 4, name: Some("ROW_5".into()), fate: Fate::Kept(4) });
    entries.push(RowEntry { index: 5, name: Some("Row_5".into()), fate: Fate::removed_to(0) });
    entries.push(RowEntry { index: 6, name: Some("Row_6".into()), fate: Fate::Kept(5) });
    let map = map_of(entries);
    let (text, moved, dropped) = renumbered_marker(&map, "0*(1*Connections.Row_5.X+2*Connections.Row_6.X)");
    assert_eq!(text.as_deref(), Some("0*(2*Connections.Row_6.X)"));
    assert_eq!((moved, dropped), (0, 1));
    // The term of the row that stays is kept whichever comes first, and a name in a case no row has is the first row's.
    let (text, _, dropped) = renumbered_marker(&map, "0*(1*Connections.ROW_5.X+2*Connections.Row_6.X)");
    assert_eq!((text.as_deref(), dropped), (Some("0*(1*Connections.ROW_5.X+2*Connections.Row_6.X)"), 0));
    let (text, _, dropped) = renumbered_marker(&map, "0*(1*Connections.rOw_5.X+2*Connections.Row_6.X)");
    assert_eq!((text.as_deref(), dropped), (Some("0*(1*Connections.ROW_5.X+2*Connections.Row_6.X)"), 0), "no row spells it so: the first");
}

#[test]
fn a_renumbering_drops_a_row_named_twice_whatever_the_generation_of_the_other_term() {
    // Row 5 (IX 4) is named by two terms, one of generation 0: neither stands, and row 7 (IX 6, now row 6) does.
    let map = gap_map(None);
    let (text, moved, dropped) = renumbered_marker(&map, "0*(1*Connections.X5+0*Connections.X5+1*Connections.X7)");
    assert_eq!(text.as_deref(), Some("0*(1*Connections.X6)"));
    assert_eq!((moved, dropped), (1, 2));
    let (text, ..) = renumbered_marker(&map, "0*(256*Connections.X5+1*Connections.X5+1*Connections.X7)");
    assert_eq!(text.as_deref(), Some("0*(1*Connections.X6)"));
    // A lone term still moves with its row, and one that names no row is dropped.
    let (text, moved, dropped) = renumbered_marker(&map, "0*(1*Connections.X5+1*Connections.X7+1*Connections.X99)");
    assert_eq!(text.as_deref(), Some("0*(1*Connections.X4+1*Connections.X6)"));
    assert_eq!((moved, dropped), (2, 1));
}

#[test]
fn a_renumbering_counts_a_number_and_a_name_for_the_one_row_as_two_terms_for_it() {
    // IX 4 is named Tally: `Connections.X5` and `Connections.Tally.X` are the one row.
    let map = gap_map(Some((4, "Tally")));
    let (text, moved, dropped) = renumbered_marker(&map, "0*(1*Connections.X5+1*Connections.Tally.X+1*Connections.X7)");
    assert_eq!(text.as_deref(), Some("0*(1*Connections.X6)"));
    assert_eq!((moved, dropped), (1, 2));
    let (text, ..) = renumbered_marker(&map, "0*(1*Connections.tally.X+1*Connections.X5+1*Connections.X7)");
    assert_eq!(text.as_deref(), Some("0*(1*Connections.X6)"));
    // A lone term of either spelling still rewrites as it did: the number moves, the name stays.
    let (text, moved, dropped) = renumbered_marker(&map, "0*(1*Connections.X5+1*Connections.X7)");
    assert_eq!(text.as_deref(), Some("0*(1*Connections.X4+1*Connections.X6)"));
    assert_eq!((moved, dropped), (2, 0));
    let (text, moved, dropped) = renumbered_marker(&map, "0*(1*Connections.Tally.X+1*Connections.X7)");
    assert_eq!(text.as_deref(), Some("0*(1*Connections.Tally.X+1*Connections.X6)"));
    assert_eq!((moved, dropped), (1, 0));
    // Terms of two rows are two terms.
    let (text, ..) = renumbered_marker(&map, "0*(1*Connections.Tally.X+1*Connections.X6+1*Connections.X7)");
    assert_eq!(text.as_deref(), Some("0*(1*Connections.Tally.X+1*Connections.X5+1*Connections.X6)"));
}

fn terms_text(n: u32) -> String {
    format!("0*({})", (1..=n).map(|r| format!("1*Connections.X{r}")).collect::<Vec<_>>().join("+"))
}

#[test]
fn a_marker_of_256_terms_is_read_and_one_of_257_is_void() {
    assert_eq!(MAX_MARKER_TERMS, 256);
    let exists = anywhere;
    assert_eq!(conn_marker::parse(&terms_text(256), &exists).terms().len(), 256);
    assert!(conn_marker::parse(&terms_text(257), &exists).is_empty(), "past the terms a shape may hold the marker is void");
}

#[test]
fn a_marker_of_more_than_8_kib_is_void_and_one_under_it_is_read() {
    assert_eq!(MAX_MARKER_BYTES, 8 * 1024);
    let long = |n: usize| {
        let names: Vec<String> = (0..n).map(|i| format!("1*Connections.{}{i}.X", "n".repeat(1_000))).collect();
        format!("0*({})", names.join("+"))
    };
    let exists = anywhere;
    let under = long(7);
    assert!(under.len() < MAX_MARKER_BYTES, "{}", under.len());
    assert_eq!(conn_marker::parse(&under, &exists).terms().len(), 7);
    let over = long(9);
    assert!(over.len() > MAX_MARKER_BYTES && over.len() < 10_000, "{}", over.len());
    assert!(conn_marker::parse(&over, &exists).is_empty(), "nine terms, and more than 8 KiB of them");
}

#[test]
fn a_marker_on_a_shape_with_more_than_256_rows_is_void() {
    let mut sc = scene();
    put_points(&mut sc.doc, sc.a, 4, 258);
    put_marker(&mut sc.doc, sc.a, "0*(1*Connections.X5)");
    assert!(conn_marker::read_in(&sc.doc, &at(&sc.doc, sc.a)).is_empty(), "a shape of 259 points is one no doubling reads");
}

#[test]
fn a_marker_cell_of_no_formula_is_no_marker() {
    let mut sc = scene();
    put_points(&mut sc.doc, sc.a, 4, 9);
    put(&mut sc.doc, sc.a, &conn_marker::marker_cell(), Cell::constant(Value::from_text("0")));
    assert!(conn_marker::read_in(&sc.doc, &at(&sc.doc, sc.a)).is_empty());
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------
// The marker: written
// ---------------------------------------------------------------------------------------------------------------------------------------------------------

fn term(n: u32, generation: u8) -> Term {
    Term { row: RowName::Number(n), generation }
}

#[test]
fn a_marker_is_written_as_a_user_row_whose_formula_names_each_point_and_reads_back() {
    let mut sc = scene();
    put_points(&mut sc.doc, sc.a, 4, 9);
    let m = Marker::with_terms(vec![term(5, 1), term(6, 1), term(9, 2)]).unwrap();
    let at_a = at(&sc.doc, sc.a);
    conn_marker::write(&mut sc.doc, &at_a, &m).unwrap();
    assert_eq!(MARKER_ROW, "DiaCraftAddedPoints");
    assert_eq!(marker_text(&sc.doc, sc.a).unwrap(), "0*(1*Connections.X5+1*Connections.X6+2*Connections.X9)");
    let cell = cell_of(&sc.doc, sc.a, &conn_marker::marker_cell()).unwrap();
    assert_eq!(cell.value.as_ref().map(|v| v.text()), Some("0"), "the value is 0, which means nothing to a program that does not know it");
    assert_eq!(conn_marker::read_in(&sc.doc, &at(&sc.doc, sc.a)).terms(), m.terms());
    // The row is a row of the User section named for the marker, and holds no other cell.
    let sheet = &sc.doc.shape(&at(&sc.doc, sc.a)).unwrap().sheet;
    let user_section = sheet.sections.iter().find(|s| s.name.as_str() == "User").unwrap();
    let row = user_section.rows.iter().find(|r| r.name.as_deref() == Some(MARKER_ROW)).unwrap();
    assert_eq!(row.cells.keys().map(|k| k.as_str()).collect::<Vec<_>>(), ["Value"]);
}

#[test]
fn a_marker_written_again_replaces_the_formula_and_an_empty_one_removes_the_row_and_keeps_the_other_user_rows() {
    let mut sc = scene();
    put_points(&mut sc.doc, sc.a, 4, 9);
    put_formula(&mut sc.doc, sc.a, "Other", "1+1");
    let at_a = at(&sc.doc, sc.a);
    conn_marker::write(&mut sc.doc, &at_a, &Marker::with_terms(vec![term(5, 1)]).unwrap()).unwrap();
    conn_marker::write(&mut sc.doc, &at_a, &Marker::with_terms(vec![term(5, 1), term(7, 2)]).unwrap()).unwrap();
    assert_eq!(marker_text(&sc.doc, sc.a).unwrap(), "0*(1*Connections.X5+2*Connections.X7)");
    conn_marker::write(&mut sc.doc, &at_a, &Marker::empty()).unwrap();
    assert_eq!(marker_text(&sc.doc, sc.a), None, "the row goes with its last term");
    assert_eq!(formula(&sc.doc, sc.a, &user("Other")).unwrap(), "1+1", "another User row stays, and so does its section");
    // Nothing to remove is no change.
    let before = sc.doc.clone();
    conn_marker::write(&mut sc.doc, &at_a, &Marker::empty()).unwrap();
    assert_eq!(sc.doc, before);
}

#[test]
fn a_marker_that_cannot_be_written_is_refused_in_words() {
    let words = |terms: Vec<Term>| refusal(Marker::with_terms(terms));
    assert_eq!(words(vec![term(5, 0)]), "a marker's generation is a whole number from 1 to 255");
    assert_eq!(words(vec![term(5, 1), term(5, 2)]), "a marker names row 5 twice");
    assert_eq!(words(vec![term(0, 1)]), "a marker's row numbers start at 1");
    assert_eq!(words(vec![Term { row: RowName::Default, generation: 1 }]), "a marker names a row by its number or its name");
    assert_eq!(
        words(vec![Term { row: RowName::Name("a b".into()), generation: 1 }]),
        "a marker names a row by a plain name: letters, digits and underscores"
    );
    assert_eq!(
        words(vec![Term { row: RowName::Name(String::new()), generation: 1 }]),
        "a marker names a row by a plain name: letters, digits and underscores"
    );
    assert_eq!(words((1..=257).map(|n| term(n, 1)).collect()), "a marker holds at most 256 terms");
    let names: Vec<Term> = (0..9).map(|i| Term { row: RowName::Name(format!("{}{i}", "n".repeat(1_000))), generation: 1 }).collect();
    assert_eq!(words(names), "a marker's formula is at most 8192 bytes");
    // The rows 1 to 256 fit, each of the largest generation.
    assert!(Marker::with_terms((1..=256).map(|n| term(n, 255)).collect()).is_ok());
}

#[test]
fn the_terms_of_a_marker_are_the_renumbering_of_its_list() {
    let m = Marker::with_terms(vec![term(5, 1), term(6, 2), term(7, 1)]).unwrap();
    let moved = m.renumbered(&|row| match row {
        RowName::Number(5) => None,
        RowName::Number(n) => Some(RowName::Number(n - 1)),
        other => Some(other.clone()),
    });
    assert_eq!(moved.terms(), [term(5, 2), term(6, 1)]);
}

/// The shapes of the first page that hold a marker, with what it reads as: for a drawing that came back from a file, whose ids a test does not rely on.
fn marked_shapes(doc: &Document) -> Vec<(ShapeRef, Vec<Term>)> {
    let page = doc.pages.get(0).unwrap();
    (0..page.shapes.len())
        .map(|i| ShapeRef { owner: Owner::Page(page.id), path: vec![u32::try_from(i).unwrap()] })
        .filter_map(|at| {
            let marker = conn_marker::read_in(doc, &at);
            (!marker.is_empty()).then(|| (at, marker.terms().to_vec()))
        })
        .collect()
}

#[test]
fn the_marker_survives_a_save_and_a_reopen_in_diacraft_and_vsdx() {
    for ext in ["diacraft", "vsdx"] {
        let (mut s, sc) = scene_in();
        let at_a = at(&sc.doc, sc.a);
        // Added points on rectangle a (rows 4 to 6), the first and the third made by doublings of different generations.
        s.edit("add the points and the marker", |doc| {
            put_points(doc, sc.a, 4, 6);
            conn_marker::write(doc, &at_a, &Marker::with_terms(vec![term(5, 1), term(7, 2)]).unwrap()).map_err(|p| p.into_engine("test"))
        })
        .unwrap();
        let before = marked_shapes(s.document().unwrap());
        assert_eq!(before.len(), 1, "the marker is on one shape");
        let dir = crate::odg_build::Dir::new();
        let path = dir.path(&format!("marked.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut back = Session::new();
        run(&mut back, "file.open", json!({ "path": path }));
        let after = marked_shapes(back.document().unwrap());
        assert_eq!(after.len(), 1, "{ext}: the marker is still on one shape");
        assert_eq!(after[0].1, before[0].1, "{ext}: the same points with the same generations");
        let marker = conn_marker::read_in(back.document().unwrap(), &after[0].0);
        assert_eq!(marker.text().as_deref(), Some("0*(1*Connections.X5+2*Connections.X7)"), "{ext}");
    }
}

#[test]
fn a_duplicate_of_a_shape_keeps_the_marker_and_it_names_the_copys_own_rows() {
    let (mut s, sc) = scene_in();
    let at_a = at(&sc.doc, sc.a);
    s.edit("add the points and the marker", |doc| {
        put_points(doc, sc.a, 4, 6);
        conn_marker::write(doc, &at_a, &Marker::with_terms(vec![term(5, 1), term(7, 2)]).unwrap()).map_err(|p| p.into_engine("test"))
    })
    .unwrap();
    run(&mut s, "shape.duplicate", json!({"ids": [sc.a]}));
    let doc = s.document().unwrap();
    let marked = marked_shapes(doc);
    assert_eq!(marked.len(), 2, "the shape and its copy each hold the marker");
    assert_eq!(marked[0].1, marked[1].1);
    // The terms are references without a sheet, so the copy's marker names the copy's rows and no formula of the copy names the original's.
    for (at, _) in &marked {
        let text = formula(doc, doc.shape(at).unwrap().id, &conn_marker::marker_cell()).unwrap();
        assert!(!text.contains("Sheet."), "{text}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    /// A marker of rows and generations in range is printed and read back as it was, and printed again as the same text.
    #[test]
    fn a_marker_reads_back_as_it_is_written(rows in proptest::collection::btree_set(1_u32..=300, 1..40), generations in proptest::collection::vec(1_u8..=255, 40)) {
        let terms: Vec<Term> = rows.iter().zip(generations.iter()).map(|(n, g)| term(*n, *g)).collect();
        let m = Marker::with_terms(terms).unwrap();
        let text = m.text().unwrap();
        let back = conn_marker::parse(&text, &anywhere);
        prop_assert_eq!(back.terms(), m.terms());
        prop_assert_eq!(back.text(), Some(text));
    }

    /// Any text of the pieces of a marker, in any order: no panic, and what is read is a marker that prints and reads back as itself.
    #[test]
    fn any_text_of_marker_pieces_is_read_without_a_panic_and_what_is_read_is_stable(parts in proptest::collection::vec(proptest::sample::select(vec![
        "0*(", ")", "+", "*", "1", "2", "255", "256", "0", "00", "4294967296", "Connections", ".", "X", "5", "Row_3", "row_3", "#REF!", " ", "Sheet.1!", "é", "-",
    ]), 0..30)) {
        let text: String = parts.concat();
        let first = conn_marker::parse(&text, &anywhere);
        if let Some(printed) = first.text() {
            let second = conn_marker::parse(&printed, &anywhere);
            prop_assert_eq!(second.terms(), first.terms());
        } else {
            prop_assert!(first.is_empty());
        }
    }
}
