//! A member that is an instance carries its master's geometry itself, and holds its size and pin as formulas of its group (keeps the copy of the geometry so that a reader that does not know the master draws the members):
//! `shape.group` of instances, and the members of a dropped group master. The masters are the built-in stencils' (Basic Shapes, Basic Flowchart) and
//! masters built in code from the specifications' XML; no file from
//! another application is used.
//!
//! What every test holds to: **the drawing does not change**. The effective rows of a member (its master's put together with its own)
//! are the same before and after, and so are the pixels of the page.

use diacraft_doc::{CellAddr, CellEdit, Document, FormulaSlot, Lookup, Owner, Resolver, Section, ShapeRef, SheetRef, Value, WriteOrigin};
use diacraft_engine::Session;
use diacraft_render::{Rendered, Renderer, View};
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape, shape_with};
use serde_json::json;

use crate::edit::{doc, drop_shape, id_of};
use crate::group_size::{FOLLOWS, factor, own};
use crate::stencil_open::{CT_STENCIL, Dir, LOOK, box_cells, cluster_members, group_master, open};
use crate::{blank_session, check_session, run, snapshot};

pub(crate) fn at(d: &Document, id: u32) -> ShapeRef {
    d.find_shape(Owner::Page(1), id).unwrap_or_else(|| panic!("no shape {id} on page 1"))
}

pub(crate) fn shape_of(d: &Document, id: u32) -> &diacraft_doc::Shape {
    d.shape(&at(d, id)).unwrap()
}

/// The `Geometry` sections a shape holds itself.
pub(crate) fn local_sections(d: &Document, id: u32) -> Vec<&std::sync::Arc<Section>> {
    shape_of(d, id).sheet.sections.iter().filter(|s| s.name.as_str() == "Geometry").collect()
}

/// A cell as the file holds it: formula text, value, unit and error.
pub(crate) fn cell_text(c: &diacraft_doc::Cell) -> String {
    format!("F={:?} V={:?} U={:?} E={:?}", c.formula.text(), c.value.as_ref().map(|v| v.text().to_owned()), c.unit.as_deref(), c.error.as_deref())
}

/// What a shape holds itself under `Geometry`, in the order it holds it: section, `Del`, its cells, each row with its `T`, `Del` and cells.
pub(crate) fn local_dump(d: &Document, id: u32) -> Vec<String> {
    let mut out = Vec::new();
    for section in local_sections(d, id) {
        out.push(format!("section IX={:?} del={}", section.ix, section.del));
        out.extend(section.cells.iter().map(|(n, c)| format!("  {} {}", n.as_str(), cell_text(c))));
        for row in &section.rows {
            out.push(format!("  row IX={:?} T={:?} del={}", row.ix, row.row_type, row.del));
            out.extend(row.cells.iter().map(|(n, c)| format!("    {} {}", n.as_str(), cell_text(c))));
        }
    }
    out
}

/// What a shape holds itself directly under it, by name.
pub(crate) fn cells_dump(d: &Document, id: u32) -> Vec<String> {
    shape_of(d, id).sheet.cells.iter().map(|(n, c)| format!("{} {}", n.as_str(), cell_text(c))).collect()
}

/// Two sizes are the same size when they are equal within a millionth of the larger, or within 1E-6 when both are under 1 (the
/// tolerance the specifications count equal numbers by).
pub(crate) fn same_size(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1.0e-6 * a.abs().max(b.abs()).max(1.0)
}

/// The definition of a resized instance: its `Width` or its `Height` differs from its master shape's. The instance's are found by inheritance, and
/// the master shape's are the master's own.
pub(crate) fn is_resized(d: &Document, id: u32) -> bool {
    let res = Resolver::new(d);
    let step = res.master_step(&at(d, id)).unwrap().expect("an instance");
    let number = |sheet: &SheetRef, name: &str| {
        res.resolve(sheet, &CellAddr::direct(name))
            .and_then(|r| r.value.cell?.value.as_ref()?.number())
            .unwrap_or_else(|| panic!("{name} of {sheet:?}"))
    };
    let (here, there) = (SheetRef::Shape(at(d, id)), SheetRef::Shape(step.shape));
    !same_size(number(&here, "Width"), number(&there, "Width")) || !same_size(number(&here, "Height"), number(&there, "Height"))
}

/// What a shape's geometry is once its master is put together with it: each effective row, with its type and each cell's value
/// and unit.
pub(crate) fn effective(d: &Document, id: u32) -> Vec<String> {
    effective_with(d, id, true)
}

/// [`effective`] with the units left out, for a comparison of values only (a cell `calc` makes on a resize has no `U`, one copied from a master has it).
pub(crate) fn values(d: &Document, id: u32) -> Vec<String> {
    effective_with(d, id, false)
}

fn effective_with(d: &Document, id: u32, units: bool) -> Vec<String> {
    let res = Resolver::new(d);
    let sheet = SheetRef::Shape(at(d, id));
    let mut lines = Vec::new();
    for key in res.effective_sections(&sheet, "Geometry").unwrap() {
        for row in res.effective_rows(&sheet, &key).unwrap() {
            let cells: Vec<String> = row
                .cells
                .iter()
                .map(|(n, r)| {
                    let v = r.value.cell.and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned());
                    let unit = r.value.cell.and_then(|c| c.unit.as_deref()).filter(|_| units);
                    format!("{}={v:?}/{unit:?}", n.as_str())
                })
                .collect();
            lines.push(format!("IX{:?} {:?} T={:?} {}", key.ix, row.key, row.row_type, cells.join(" ")));
        }
    }
    lines
}

const PX_PER_INCH: f64 = 100.0;

/// The page of a built drawing: eleven inches by eight and a half, at scale 1.
pub(crate) const PAGE: &str =
    r#"<Cell N="PageWidth" V="11"/><Cell N="PageHeight" V="8.5"/><Cell N="PageScale" V="1"/><Cell N="DrawingScale" V="1"/>"#;

pub(crate) fn render(s: &Session) -> Rendered {
    let d = s.document().unwrap();
    Renderer::new().render(d, 1, &View::fit_page(d, 1, PX_PER_INCH).unwrap())
}

pub(crate) fn assert_same_picture(s: &Session, before: &Rendered, what: &str) {
    let now = render(s);
    assert!(now.skipped.is_empty(), "{what}: {:?}", now.skipped);
    assert_eq!((now.width, now.height), (before.width, before.height), "{what}");
    let off = now.rgba_premul.iter().zip(&before.rgba_premul).filter(|(a, b)| a != b).count();
    assert_eq!(off, 0, "{what}: {off} bytes of the picture differ");
}

pub(crate) fn group(s: &mut Session, ids: &[u32]) -> u32 {
    id_of(&run(s, "shape.group", json!({"ids": ids})))
}

/// Rectangle 1, ellipse 2 (Basic Shapes), decision 3 and terminator 4 (Basic Flowchart), spread over the page.
pub(crate) fn four() -> Session {
    let mut s = blank_session();
    assert_eq!(drop_shape(&mut s, "rectangle", 2.0, 2.0), 1);
    assert_eq!(drop_shape(&mut s, "ellipse", 6.0, 2.0), 2);
    assert_eq!(drop_shape(&mut s, "decision", 2.0, 5.0), 3);
    assert_eq!(drop_shape(&mut s, "terminator", 6.0, 5.0), 4);
    s
}

fn resize(s: &mut Session, id: u32, width: f64, height: f64) {
    run(s, "shape.resize", json!({"id": id, "x": 1.0, "y": 1.0, "width": width, "height": height}));
}

// ---- what resized means ------------------------------------------------------------------------------------------------------------

#[test]
fn a_size_is_the_same_within_a_millionth_of_the_larger_and_within_1e_6_when_both_are_under_1() {
    assert!(same_size(1.5, 1.5) && same_size(0.4, 0.4000009) && same_size(1.0, 1.0000009));
    assert!(!same_size(0.4, 0.4000011) && !same_size(1.0, 1.0000011));
    assert!(same_size(1000.0, 1000.0009) && !same_size(1000.0, 1000.0011), "a millionth of the larger once a size is over 1");
    assert!(!same_size(0.0, 1.0e-5) && same_size(0.0, 1.0e-7));
}

#[test]
fn an_instance_is_resized_when_its_width_or_its_height_is_not_its_master_shapes() {
    let mut s = four();
    assert!((1..=4).all(|id| !is_resized(doc(&s), id)), "a drop holds the master's size");
    resize(&mut s, 1, 3.0, 2.5);
    run(&mut s, "shape.size", json!({"ids": [2], "height": 1.5}));
    run(&mut s, "shape.size", json!({"ids": [3], "width": 1.5000001}));
    let d = doc(&s);
    assert_eq!(
        (is_resized(d, 1), is_resized(d, 2), is_resized(d, 3), is_resized(d, 4)),
        (true, true, false, false),
        "the height alone counts, and a tenth of a millionth does not"
    );
    // The two thresholds differ: the 1E-6 says shape 3 is at its master's size, and `calc` adds an `Inh` cell wherever a result differs from the
    // master's by more than its own 1E-9. So a shape that is not "resized" can hold a few cells (so), and one at exactly the
    // master's size holds none.
    assert!(!local_sections(d, 3).is_empty(), "a width 1E-7 off holds the cells `calc` gave it");
    assert!(local_sections(d, 4).is_empty(), "a shape at its master's size holds none");
}

// ---- the copy ---------------------------------------------------------------------------------------------------------------------

#[test]
fn an_instance_made_a_member_holds_a_local_copy_of_its_masters_geometry_and_draws_as_it_did() {
    let mut s = four();
    let before = render(&s);
    let was: Vec<Vec<String>> = (1..=4).map(|id| effective(doc(&s), id)).collect();
    for id in 1..=4 {
        assert!(local_sections(doc(&s), id).is_empty(), "an instance holds only what differs until it is grouped");
        assert!(!was[usize::try_from(id - 1).unwrap()].is_empty(), "the masters have geometry");
    }
    let g = group(&mut s, &[1, 2, 3]);
    assert_eq!(g, 5, "The group's ID is above every ID of the page");
    let d = doc(&s);
    // And each holds its size and pin as formulas of the group; the shape left out and the group hold numbers.
    for id in 1..=3 {
        for (name, group_cell) in FOLLOWS {
            let text = own(d, id, name).and_then(|c| c.formula.text().map(str::to_owned)).unwrap_or_else(|| panic!("shape {id} {name}"));
            assert!(factor(&text, g, group_cell).is_some(), "shape {id} {name} is `{text}`");
        }
    }
    assert!(FOLLOWS.iter().all(|(name, _)| own(d, 4, name).is_none_or(|c| c.formula.text().is_none())));
    // The three members hold their sections; the shape left out, and the group, hold none.
    for id in 1..=3 {
        let sections = local_sections(d, id);
        assert!(!sections.is_empty(), "shape {id} holds a Geometry section of its own");
        // Each effective row is a row of the member's own, with its type, and no cell of it comes from the master.
        let res = Resolver::new(d);
        let sheet = SheetRef::Shape(at(d, id));
        for key in res.effective_sections(&sheet, "Geometry").unwrap() {
            let own = sections.iter().find(|s| s.ix == key.ix).unwrap_or_else(|| panic!("shape {id}: no section IX {:?}", key.ix));
            let rows = res.effective_rows(&sheet, &key).unwrap();
            assert_eq!(own.rows.len(), rows.len(), "shape {id}: every row");
            for (held, effective_row) in own.rows.iter().zip(&rows) {
                assert_eq!(held.row_type, effective_row.row_type, "shape {id}: the row's type");
                assert_eq!(held.cells.len(), effective_row.cells.len(), "shape {id}: every cell of the row");
            }
        }
    }
    assert!(local_sections(d, 4).is_empty() && local_sections(d, g).is_empty());
    // The effective rows are what they were, and so are the pixels.
    for id in 1..=3 {
        assert_eq!(effective(d, id), was[usize::try_from(id - 1).unwrap()], "shape {id}: the geometry it draws");
    }
    assert_eq!(effective(d, 4), was[3]);
    assert_same_picture(&s, &before, "a group of instances draws what they drew");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_cell_whose_master_has_a_formula_inherits_it_and_a_plain_value_is_a_constant_of_its_own() {
    let mut s = four();
    group(&mut s, &[1, 3]);
    let d = doc(&s);
    for id in [1, 3] {
        let res = Resolver::new(d);
        let step = res.master_step(&at(d, id)).unwrap().unwrap();
        let master = &res.shape(&step.shape).unwrap().sheet;
        let (mut inherited, mut constants) = (0, 0);
        for section in local_sections(d, id) {
            for row in &section.rows {
                for (name, held) in &row.cells {
                    let addr = CellAddr::in_row("Geometry", section.ix, diacraft_doc::RowKey::Ix(row.ix.unwrap()), name.as_str());
                    let Lookup::Found(of_master) = master.lookup(&addr) else { panic!("shape {id}: the master has no {addr:?}") };
                    match &of_master.formula {
                        FormulaSlot::Local(_) => {
                            // The formula is not copied: the cell inherits it, with the result it has here.
                            assert_eq!(held.formula, FormulaSlot::Inherit, "shape {id} {name:?}");
                            assert!(held.value.is_some(), "shape {id} {name:?}: the cached result");
                            inherited += 1;
                        }
                        _ => {
                            assert_eq!(held.formula, FormulaSlot::Absent, "shape {id} {name:?}");
                            assert_eq!(held.value, of_master.value, "shape {id} {name:?}: the value as the master holds it");
                            constants += 1;
                        }
                    }
                    assert_eq!(held.unit, of_master.unit, "shape {id} {name:?}: the unit");
                    assert!(held.error.is_none());
                }
            }
        }
        assert!(inherited > 0 && constants > 0, "shape {id}: both kinds of cell are there ({inherited} inherited, {constants} constants)");
    }
}

#[test]
fn a_group_of_a_group_copies_for_the_instances_named_and_leaves_the_members_of_the_inner_group_as_they_are() {
    let mut s = four();
    let inner = group(&mut s, &[1, 2]);
    let (own1, own2) = (local_dump(doc(&s), 1), local_dump(doc(&s), 2));
    assert!(!own1.is_empty() && !own2.is_empty());
    let before = render(&s);
    let outer = group(&mut s, &[inner, 3]);
    let d = doc(&s);
    assert_eq!(shape_of(d, outer).children.iter().map(|c| c.id).collect::<Vec<_>>(), [inner, 3]);
    assert!(local_sections(d, inner).is_empty(), "a group has no geometry to copy");
    assert!(!local_sections(d, 3).is_empty(), "the instance named is copied");
    assert!(
        factor(own(d, 3, "Width").and_then(|c| c.formula.text().map(str::to_owned)).as_deref().unwrap_or(""), outer, "Width").is_some(),
        "and tied to the outer group"
    );
    assert_eq!((local_dump(d, 1), local_dump(d, 2)), (own1.clone(), own2), "what the inner members hold is not written again");
    assert_same_picture(&s, &before, "a nested group");
    // And grouping again two members of the inner group is the same: nothing is added twice.
    let before = render(&s);
    let deep = group(&mut s, &[1, 2]);
    assert_eq!(shape_of(doc(&s), deep).children.len(), 2);
    assert_eq!(local_dump(doc(&s), 1), own1, "nothing is added twice");
    assert_same_picture(&s, &before, "a group inside a group");
}

#[test]
fn grouping_is_one_undo_step_and_undo_takes_back_the_copy_with_it() {
    let mut s = four();
    let before = doc(&s).clone();
    let steps = s.history().unwrap().undo_len();
    group(&mut s, &[1, 2, 3, 4]);
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    let after = doc(&s).clone();
    assert!((1..=4).all(|id| !local_sections(&after, id).is_empty()), "the copy is made in the same step");
    assert!((1..=4).all(|id| own(&after, id, "Width").is_some_and(|c| c.formula.text().is_some())), "and so are the formulas");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s), &before, "the copy and the formulas go with the group");
    assert!((1..=4).all(|id| local_sections(doc(&s), id).is_empty() && own(doc(&s), id, "Width").is_none()));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(doc(&s), &after);
}

#[test]
fn an_ungroup_leaves_the_copy_where_it_is_and_the_picture_is_the_same() {
    let mut s = four();
    let before = render(&s);
    let numbers: Vec<[f64; 4]> = (1..=3).map(|id| FOLLOWS.map(|(n, _)| crate::edit::num(doc(&s), id, n))).collect();
    let g = group(&mut s, &[1, 2, 3]);
    let held: Vec<Vec<String>> = (1..=3).map(|id| local_dump(doc(&s), id)).collect();
    assert!(held.iter().all(|h| !h.is_empty()), "the members hold the copy");
    run(&mut s, "shape.ungroup", json!({"ids": [g]}));
    let d = doc(&s);
    for id in 1..=3 {
        assert_eq!(local_dump(d, id), held[usize::try_from(id - 1).unwrap()], "shape {id}");
        // A formula of a group that is gone is its number, and the numbers are the ones the shape drew with.
        for (n, (name, _)) in FOLLOWS.iter().enumerate() {
            let cell = own(d, id, name).unwrap();
            assert_eq!((cell.formula.text(), cell.error.as_deref()), (None, None), "shape {id} {name}");
            assert!((crate::edit::num(d, id, name) - numbers[usize::try_from(id - 1).unwrap()][n]).abs() < 1e-9, "shape {id} {name}");
        }
    }
    assert_same_picture(&s, &before, "group and ungroup");
}

#[test]
fn a_group_turned_and_flipped_after_the_copy_draws_as_its_members_did() {
    let mut s = four();
    group(&mut s, &[1, 2]);
    run(&mut s, "shape.rotate", json!({"ids": [5], "by": 35}));
    run(&mut s, "shape.flip", json!({"ids": [5], "axis": "horizontal"}));
    let before = render(&s);
    // Grouped with another instance: the turned group is a member now and the instance named is copied; the picture does not change.
    let outer = group(&mut s, &[5, 3]);
    assert!(!local_sections(doc(&s), 3).is_empty());
    assert!(own(doc(&s), 3, "PinX").is_some_and(|c| factor(c.formula.text().unwrap_or(""), outer, "Width").is_some()), "tied to the new group");
    assert_same_picture(&s, &before, "a turned and flipped group grouped again");
}

// ---- what the copy does not change -----------------------------------------------------------------------------------------------

#[test]
fn a_member_resized_after_the_group_draws_as_the_same_shape_resized_without_one() {
    let resize = json!({"id": 1, "x": 1.0, "y": 1.0, "width": 3.0, "height": 2.5});
    let mut plain = four();
    run(&mut plain, "shape.resize", resize.clone());
    let mut grouped = four();
    group(&mut grouped, &[1, 2]);
    assert!(!local_sections(doc(&grouped), 1).is_empty(), "the member holds the copy before it is resized");
    run(&mut grouped, "shape.resize", resize);
    // The cells that follow the size are worked out on the member: the same rows, the same picture.
    assert_eq!(values(doc(&grouped), 1), values(doc(&plain), 1));
    assert_same_picture(&grouped, &render(&plain), "a member resized");
    assert_eq!(check_session(&grouped), Ok(()));
}

#[test]
fn glue_to_a_member_and_its_connection_points_are_as_they_were_and_still_follow() {
    let mut s = four();
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": 1, "point": 3}, "to": {"id": 2, "point": 1}})));
    let before = render(&s);
    let rows_before: Vec<(u32, String, u32)> =
        doc(&s).page(1).unwrap().connects.iter().map(|k| (k.from_sheet, k.to_cell.as_deref().unwrap_or("").to_owned(), k.to_sheet)).collect();
    let points = |s: &Session, id: u32| diacraft_engine::draw::connection_points(doc(s), 1, id);
    let (p1, p2) = (points(&s, 1), points(&s, 2));
    let g = group(&mut s, &[1, 2]);
    let d = doc(&s);
    assert!(!local_sections(d, 1).is_empty() && !local_sections(d, 2).is_empty(), "the members hold the copy");
    let rows_after: Vec<(u32, String, u32)> =
        d.page(1).unwrap().connects.iter().map(|k| (k.from_sheet, k.to_cell.as_deref().unwrap_or("").to_owned(), k.to_sheet)).collect();
    assert_eq!(rows_after, rows_before, "the Connect rows");
    assert_eq!((points(&s, 1), points(&s, 2)), (p1.clone(), p2.clone()), "the connection points of the members, on the page");
    assert_same_picture(&s, &before, "a glued member grouped");
    // The group moves, and the connector's ends are on the members' points.
    run(&mut s, "shape.move", json!({"ids": [g], "dx": 1.0, "dy": 0.5}));
    let (q1, q2) = (points(&s, 1), points(&s, 2));
    let d = doc(&s);
    let end = |which: &str| {
        let n = |cell: &str| {
            let found = Resolver::new(d).resolve(&SheetRef::Shape(at(d, c)), &CellAddr::direct(&format!("{which}{cell}"))).unwrap();
            found.value.cell.and_then(|c| c.value.as_ref()).and_then(Value::number).unwrap()
        };
        (n("X"), n("Y"))
    };
    let (b, e) = (end("Begin"), end("End"));
    let want_b = q1.iter().find(|p| p.index == 3).unwrap();
    let want_e = q2.iter().find(|p| p.index == 1).unwrap();
    assert!((b.0 - want_b.x).abs() < 1e-6 && (b.1 - want_b.y).abs() < 1e-6, "{b:?} is on point 3 of the first member {want_b:?}");
    assert!((e.0 - want_e.x).abs() < 1e-6 && (e.1 - want_e.y).abs() < 1e-6, "{e:?} is on point 1 of the second member {want_e:?}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_copy_of_a_group_carries_the_copies_its_members_hold_and_ties_them_to_the_copy() {
    let mut s = four();
    let g = group(&mut s, &[1, 2]);
    let held = [local_dump(doc(&s), 1), local_dump(doc(&s), 2)];
    assert!(!held[0].is_empty() && !held[1].is_empty(), "the members hold the copy");
    run(&mut s, "edit.copy", json!({"ids": [g]}));
    let r = run(&mut s, "edit.paste", json!({}));
    let pasted = u32::try_from(r["ids"][0].as_u64().unwrap()).unwrap();
    let d = doc(&s);
    let members: Vec<u32> = shape_of(d, pasted).children.iter().map(|c| c.id).collect();
    assert_eq!(members.len(), 2);
    for (k, id) in members.iter().enumerate() {
        assert_eq!(local_dump(d, *id), held[k], "pasted member {id}");
        for (name, group_cell) in FOLLOWS {
            let text = own(d, *id, name).and_then(|c| c.formula.text().map(str::to_owned)).unwrap_or_else(|| panic!("member {id} {name}"));
            assert!(
                factor(&text, pasted, group_cell).is_some(),
                "member {id} {name} is `{text}`: it names the copy and not the group it was copied from"
            );
        }
    }
}

/// Every cell of shape `id` and of everything in it: none has an error, and no formula names one of `groups` (`Sheet.<id>!`).
pub(crate) fn assert_names_no_group(d: &Document, id: u32, groups: &[u32], what: &str) {
    let mut stack = vec![at(d, id)];
    while let Some(place) = stack.pop() {
        let shape = d.shape(&place).unwrap();
        for (name, cell) in &shape.sheet.cells {
            assert_eq!(cell.error.as_deref(), None, "{what}: {} of shape {}", name.as_str(), shape.id);
            let text = cell.formula.text().unwrap_or("");
            assert!(groups.iter().all(|g| !text.contains(&format!("Sheet.{g}!"))), "{what}: {} of shape {} is `{text}`", name.as_str(), shape.id);
        }
        stack.extend((0..shape.children.len()).map(|k| place.child(u32::try_from(k).unwrap())));
    }
}

#[test]
fn a_member_copied_cut_or_duplicated_alone_keeps_its_geometry_and_names_no_group_it_has_left() {
    for how in ["copy", "cut", "duplicate"] {
        let mut s = four();
        let g = group(&mut s, &[1, 2]);
        let (held, rows) = (local_dump(doc(&s), 1), effective(doc(&s), 1));
        let numbers = FOLLOWS.map(|(n, _)| crate::edit::num(doc(&s), 1, n));
        assert!(!held.is_empty() && tied_to(doc(&s), 1, g));
        let alone = match how {
            "copy" => {
                run(&mut s, "edit.copy", json!({"ids": [1]}));
                u32::try_from(run(&mut s, "edit.paste", json!({}))["ids"][0].as_u64().unwrap()).unwrap()
            }
            "cut" => {
                run(&mut s, "edit.cut", json!({"ids": [1]}));
                u32::try_from(run(&mut s, "edit.paste", json!({}))["ids"][0].as_u64().unwrap()).unwrap()
            }
            _ => u32::try_from(run(&mut s, "shape.duplicate", json!({"ids": [1]}))["ids"][0].as_u64().unwrap()).unwrap(),
        };
        let d = doc(&s);
        assert_eq!(local_dump(d, alone), held, "{how}: a member copied out of its group keeps the geometry it holds");
        assert_eq!(effective(d, alone), rows, "{how}");
        // The cells that named the group it left are numbers, as an ungroup makes them.
        for (name, _) in FOLLOWS {
            assert_eq!(own(d, alone, name).unwrap().formula.text(), None, "{how}: {name}");
        }
        assert_names_no_group(d, alone, &[g], how);
        if how != "cut" {
            // The size is the one it had; the pin was carried out of the group (and moved by the paste).
            for (n, (name, _)) in FOLLOWS.iter().enumerate().take(2) {
                assert!((crate::edit::num(d, alone, name) - numbers[n]).abs() < 1e-9, "{how}: {name}");
            }
            // Taking the original group apart, or deleting it, leaves the copy as it was: no `#REF!`.
            let before = cells_dump(d, alone);
            let picture = render(&s);
            run(&mut s, "shape.ungroup", json!({"ids": [g]}));
            assert_eq!(cells_dump(doc(&s), alone), before, "{how}: the copy does not read the group that was taken apart");
            assert_names_no_group(doc(&s), alone, &[g], how);
            assert_same_picture(&s, &picture, how);
        }
        assert_eq!(check_session(&s), Ok(()));
    }
}

#[test]
fn a_member_of_a_group_in_a_group_copied_alone_names_neither_group_and_one_regrouped_follows_its_new_group() {
    let mut s = four();
    let inner = group(&mut s, &[1, 2]);
    let outer = group(&mut s, &[inner, 3]);
    run(&mut s, "edit.copy", json!({"ids": [1]}));
    let alone = u32::try_from(run(&mut s, "edit.paste", json!({}))["ids"][0].as_u64().unwrap()).unwrap();
    assert_names_no_group(doc(&s), alone, &[inner, outer], "a copy of a member two groups deep");
    // The copy, grouped with another shape, is tied to the new group and to nothing else.
    let fresh = group(&mut s, &[alone, 4]);
    assert!(tied_to(doc(&s), alone, fresh), "the copy follows its new group");
    assert_names_no_group(doc(&s), alone, &[inner, outer], "the copy regrouped");
    // Taking the groups it left apart changes nothing in it.
    let before = cells_dump(doc(&s), alone);
    run(&mut s, "shape.ungroup", json!({"ids": [outer]}));
    assert_eq!(cells_dump(doc(&s), alone), before);
    assert_eq!(check_session(&s), Ok(()));
}

/// Whether the four cells of shape `id` are formulas of group `group`.
pub(crate) fn tied_to(d: &Document, id: u32, group: u32) -> bool {
    FOLLOWS.iter().all(|(name, group_cell)| {
        own(d, id, name).and_then(|c| c.formula.text().map(str::to_owned)).is_some_and(|t| factor(&t, group, group_cell).is_some())
    })
}

#[test]
fn a_saved_drawing_reads_back_with_the_same_copy_the_same_rows_and_the_same_picture() {
    let dir = Dir::new();
    let mut s = four();
    group(&mut s, &[1, 2, 3]);
    let picture = render(&s);
    let path = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    for id in 1..=3 {
        assert!(!local_dump(doc(&s), id).is_empty(), "shape {id} holds the copy");
        assert_eq!(local_dump(doc(&t), id), local_dump(doc(&s), id), "shape {id} as saved");
        assert_eq!(cells_dump(doc(&t), id), cells_dump(doc(&s), id), "shape {id} as saved: the formulas of its group");
        assert_eq!(effective(doc(&t), id), effective(doc(&s), id), "shape {id}: the rows");
    }
    assert_same_picture(&t, &picture, "reopened");
    // Saved again with nothing changed, the file is the same bytes.
    let again = dir.path("again.vsdx");
    run(&mut t, "file.save_as", json!({"path": again}));
    assert_eq!(std::fs::read(&path).unwrap(), std::fs::read(&again).unwrap(), "a second save of the same drawing");
    assert_eq!(check_session(&t), Ok(()));
}

// ---- a drawing from a file, with geometry of its own --------------------------------------------------------------------------------

/// The master "Box" (ID 1): a box 2 by 1 with Geometry sections 0 (the outline) and 1 (a diagonal), and a connection point.
fn box_with_two_paths() -> MasterSpec {
    let inner = format!(
        r#"{LOOK}{}<Section N="Geometry" IX="1"><Cell N="NoFill" V="1"/><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="1" F="Height"/></Row></Section>"#,
        box_cells(2.0, 1.0)
    );
    MasterSpec::new(1, "Box").shapes(&shape(4, &inner))
}

/// Instances of "Box": 11 and 13 as a drop makes them, 12 with geometry of its own over the master's (a row's `Y` changed (row 3 of section 0), row 5
/// hidden, a new row 6, and section 1 hidden), and 14 resized to a width of 3 with the cells that gives it as a file holds them: `Inh` with the result.
fn page_of_instances() -> String {
    let plain = |id: u32, x: f64, y: f64| shape_with(id, r#" Master="1""#, &format!(r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="{y}"/>"#));
    let own = shape_with(
        12,
        r#" Master="1""#,
        r#"<Cell N="PinX" V="6"/><Cell N="PinY" V="2"/>
<Section N="Geometry" IX="0"><Row T="LineTo" IX="3"><Cell N="Y" V="0.9"/></Row><Row IX="5" Del="1"/>
<Row T="LineTo" IX="6"><Cell N="X" V="1"/><Cell N="Y" V="0.4"/></Row></Section>
<Section N="Geometry" IX="1" Del="1"/>"#,
    );
    let resized = shape_with(
        14,
        r#" Master="1""#,
        r#"<Cell N="PinX" V="8"/><Cell N="PinY" V="6"/><Cell N="Width" V="3"/><Cell N="LocPinX" V="1.5" F="Inh"/>
<Section N="Geometry" IX="0"><Row IX="2"><Cell N="X" V="3" F="Inh"/></Row><Row IX="3"><Cell N="X" V="3" F="Inh"/></Row></Section>
<Section N="Geometry" IX="1"><Row IX="2"><Cell N="X" V="3" F="Inh"/></Row></Section>"#,
    );
    format!("{}{own}{}{resized}", plain(11, 2.0, 2.0), plain(13, 4.0, 5.0))
}

/// Every cell shape `id` of `before` holds under `Geometry` (by section, row and name) is in `after`, as it was.
fn assert_cells_kept(before: &Document, after: &Document, id: u32) {
    for section in local_sections(before, id) {
        let now =
            local_sections(after, id).into_iter().find(|s| s.ix == section.ix).unwrap_or_else(|| panic!("shape {id}: section {:?} went", section.ix));
        for (name, cell) in &section.cells {
            assert_eq!(now.cells.get(name), Some(cell), "shape {id}: {} of section {:?}", name.as_str(), section.ix);
        }
        for row in &section.rows {
            let kept = now.rows.iter().find(|r| r.ix == row.ix).unwrap_or_else(|| panic!("shape {id}: row {:?} went", row.ix));
            assert_eq!(kept.del, row.del, "shape {id}: row {:?}", row.ix);
            for (name, cell) in &row.cells {
                assert_eq!(kept.cells.get(name), Some(cell), "shape {id}: {} of row {:?}", name.as_str(), row.ix);
            }
        }
    }
}

fn opened_instances(dir: &Dir) -> Session {
    let path = dir.path("in.vsdx");
    std::fs::write(
        &path,
        VsdxBuilder::new().master(box_with_two_paths()).page(PageSpec::new(1, "Page-1").sheet(PAGE).shapes(&page_of_instances())).build(),
    )
    .unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    s
}

#[test]
fn what_the_shape_holds_already_is_kept_and_the_rows_it_hides_stay_hidden() {
    let dir = Dir::new();
    let mut s = opened_instances(&dir);
    let before = render(&s);
    let (e11, e12, e14) = (effective(doc(&s), 11), effective(doc(&s), 12), effective(doc(&s), 14));
    // Instance 12 hides row 5 and section 1, changes row 3, and adds row 6: the effective geometry is the master's merged with that.
    assert!(e12.iter().any(|l| l.contains("IX6")) || e12.iter().any(|l| l.contains("Ix(6)")), "{e12:#?}");
    assert!(!e12.iter().any(|l| l.contains("IX1")), "section 1 is hidden");
    let own_12 = local_dump(doc(&s), 12);
    let was = doc(&s).clone();
    assert_eq!((is_resized(&was, 11), is_resized(&was, 12), is_resized(&was, 14)), (false, false, true));
    group(&mut s, &[11, 12, 14]);
    let d = doc(&s);
    assert_eq!(effective(d, 11), e11);
    assert_eq!(effective(d, 14), e14, "the resized instance draws as it did");
    // The resized instance (14) keeps every cell it held, its `Inh` results included, and gets the rest of the copy.
    assert_cells_kept(&was, d, 14);
    assert_cells_kept(&was, d, 12);
    assert!(local_dump(d, 14).len() > local_dump(&was, 14).len(), "and what it lacked is written");
    assert_eq!(effective(d, 12), e12, "the merge of the master's and the shape's own is unchanged");
    // What 12 held is as it was: the changed cell has its constant and no formula, the hidden row and section are still hidden, the new row is its own.
    let section0 = local_sections(d, 12).into_iter().find(|s| s.ix == Some(0)).unwrap();
    let row3 = section0.rows.iter().find(|r| r.ix == Some(3)).unwrap();
    assert_eq!(cell_text(&row3.cells["Y"]), r#"F=None V=Some("0.9") U=None E=None"#);
    assert!(section0.rows.iter().find(|r| r.ix == Some(5)).unwrap().del, "row 5 is still hidden");
    assert!(local_sections(d, 12).into_iter().find(|s| s.ix == Some(1)).unwrap().del, "section 1 is still hidden");
    assert!(local_dump(d, 12).len() > own_12.len(), "and what was missing is written");
    // The cell row 3 lacked is written (X: the master's formula, inherited); the row's type, which it lacked, comes from the master.
    assert_eq!(row3.row_type.as_deref(), Some("LineTo"));
    assert_eq!(row3.cells["X"].formula, FormulaSlot::Inherit);
    // Rows are in the order of their IX, as a file lists them: the new row 6 is last.
    let ixs: Vec<Option<u32>> = section0.rows.iter().map(|r| r.ix).collect();
    assert_eq!(ixs, [Some(1), Some(2), Some(3), Some(4), Some(5), Some(6)]);
    assert!(local_sections(d, 11).iter().all(|s| !s.del) && local_sections(d, 11).len() == 2, "instance 11 gets both sections");
    assert_same_picture(&s, &before, "instances with geometry of their own, grouped");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_copy_a_file_holds_already_is_kept_as_it_is_and_through_a_save() {
    // What an instance of "Box" holds once it has been grouped before: every row of both sections, a constant for each plain value of the master and `Inh`
    // with the result for each formula. Grouped again with an instance that holds none, it is as it was.
    let copy = shape_with(
        16,
        r#" Master="1""#,
        r#"<Cell N="PinX" V="5"/><Cell N="PinY" V="6"/>
<Section N="Geometry" IX="0"><Cell N="NoFill" V="0"/>
<Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2" F="Inh"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="3"><Cell N="X" V="2" F="Inh"/><Cell N="Y" V="1" F="Inh"/></Row><Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="1" F="Inh"/></Row>
<Row T="LineTo" IX="5"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>
<Section N="Geometry" IX="1"><Cell N="NoFill" V="1"/><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="2"><Cell N="X" V="2" F="Inh"/><Cell N="Y" V="1" F="Inh"/></Row></Section>"#,
    );
    let plain = shape_with(17, r#" Master="1""#, r#"<Cell N="PinX" V="2"/><Cell N="PinY" V="2"/>"#);
    let dir = Dir::new();
    let path = dir.path("copy.vsdx");
    std::fs::write(
        &path,
        VsdxBuilder::new().master(box_with_two_paths()).page(PageSpec::new(1, "Page-1").sheet(PAGE).shapes(&format!("{copy}{plain}"))).build(),
    )
    .unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    let (held, rows, before) = (local_dump(doc(&s), 16), effective(doc(&s), 16), render(&s));
    assert!(held.len() > 20, "the copy is there: {}", held.len());
    group(&mut s, &[16, 17]);
    assert_eq!(local_dump(doc(&s), 16), held, "a copy a file holds is data it holds");
    assert_eq!(effective(doc(&s), 16), rows);
    assert_eq!(local_dump(doc(&s), 17), held, "and the instance beside it is given the same");
    assert_same_picture(&s, &before, "an earlier copy, grouped");
    let out = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({"path": out}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": out}));
    assert_eq!(local_dump(doc(&t), 16), held);
}

#[test]
fn a_later_edit_of_the_masters_geometry_reaches_a_formula_cell_a_row_it_gains_and_not_a_number() {
    let dir = Dir::new();
    let mut s = opened_instances(&dir);
    group(&mut s, &[11, 13]);
    let d = doc(&s);
    let step_shape = Resolver::new(d).master_step(&at(d, 11)).unwrap().unwrap().shape;
    let before = effective(d, 11);
    // A number the master holds in a cell with no formula changes: the member holds a constant of its own and keeps the old one. The instance left
    // alone (12) reads the master's new number.
    let mut edited = d.clone();
    let addr = CellAddr::in_row("Geometry", Some(0), diacraft_doc::RowKey::Ix(1), "X");
    edited.write_cell(&SheetRef::Shape(step_shape.clone()), &addr, CellEdit::Value(Value::from_text("0.5")), WriteOrigin::Drawing).unwrap();
    assert_eq!(effective(&edited, 11), before, "a number the master changes does not reach the member");
    assert_eq!(effective(&edited, 13), before, "(the other member as well)");
    assert_ne!(effective(&edited, 12), effective(d, 12), "an instance that holds no copy of that row reads the master's new number");
    // A row the master gains does reach it: the member's section is merged with the master's by `IX`, and holds no row 9.
    let mut more = d.clone();
    let addr = CellAddr::in_row("Geometry", Some(0), diacraft_doc::RowKey::Ix(9), "X");
    more.write_cell(&SheetRef::Shape(step_shape), &addr, CellEdit::Value(Value::from_text("1")), WriteOrigin::Drawing).unwrap();
    assert_eq!(effective(&more, 11).len(), before.len() + 1, "a row the master gains is merged in");
    // The formula the member's cell inherits is the master's own, found there, with the result held by the member.
    let res = Resolver::new(d);
    let addr = CellAddr::in_row("Geometry", Some(0), diacraft_doc::RowKey::Ix(2), "X");
    let found = res.resolve(&SheetRef::Shape(at(d, 11)), &addr).unwrap();
    assert!(matches!(found.formula.source, diacraft_doc::Source::Master(_)), "the formula is the master's, not a copy of its text");
    assert!(matches!(found.value.source, diacraft_doc::Source::Local), "and the value is the member's own");
}

// ---- the members of a dropped group master ---------------------------------------------------------------------------------------

#[test]
fn the_members_of_a_dropped_group_master_hold_their_master_shapes_geometry_and_the_instance_does_not() {
    let dir = Dir::new();
    let bytes = VsdxBuilder::new().document_type(CT_STENCIL).master(group_master(&cluster_members())).build();
    let mut s = blank_session();
    open(&mut s, &dir.write("group.vssx", &bytes));
    let r = run(&mut s, "shape.drop", json!({"master": "Cluster", "stencil": "file-1", "page": 1, "x": 4, "y": 4}));
    assert_eq!(id_of(&r), 1);
    let d = doc(&s);
    let ids: Vec<u32> = vec![2, 3, 4, 5, 6];
    assert!(local_sections(d, 1).is_empty(), "the instance is on the page and holds only what differs");
    // Each member (leaf, inner group, its two leaves, leaf) holds a section 0, and its effective rows are those of the master shape it stands for.
    let res = Resolver::new(d);
    for id in &ids {
        assert!(!local_sections(d, *id).is_empty(), "member {id}");
        // The members of a master of one group shape hold no size or pin: they inherit the master's formulas, which follow the instance.
        assert!(shape_of(d, *id).sheet.cells.is_empty(), "member {id} holds no cell directly under it");
        let step = res.master_step(&at(d, *id)).unwrap().unwrap();
        let from_master = SheetRef::Shape(step.shape);
        let key = diacraft_doc::SectionKey::new("Geometry", Some(0));
        let master_rows = res.effective_rows(&from_master, &key).unwrap();
        let own = res.effective_rows(&SheetRef::Shape(at(d, *id)), &key).unwrap();
        assert_eq!(own.len(), master_rows.len(), "member {id}");
        for (a, b) in own.iter().zip(&master_rows) {
            assert_eq!(a.row_type, b.row_type);
            let value = |r: &diacraft_doc::EffectiveRow<'_>| -> Vec<(String, Option<String>)> {
                r.cells
                    .iter()
                    .map(|(n, x)| (n.as_str().to_owned(), x.value.cell.and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())))
                    .collect()
            };
            assert_eq!(value(a), value(b), "member {id}: the cells of row {:?}", a.key);
        }
    }
    // The first leaf's row 2 `X` is `Width` in the master: inherited, with the result 0.5.
    let leaf = local_sections(d, 2).into_iter().find(|s| s.ix == Some(0)).unwrap();
    let row2 = leaf.rows.iter().find(|r| r.ix == Some(2)).unwrap();
    assert_eq!(
        (row2.cells["X"].formula.clone(), row2.cells["X"].value.as_ref().map(|v| v.text().to_owned())),
        (FormulaSlot::Inherit, Some("0.5".to_owned()))
    );
    // Saved and read back: the same, and a second drop gives the same copy to its members.
    let path = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    for id in &ids {
        assert_eq!(local_dump(doc(&t), *id), local_dump(doc(&s), *id), "member {id} as saved");
    }
    let next = run(&mut s, "shape.drop", json!({"master": "Cluster", "stencil": "file-1", "page": 1, "x": 8, "y": 4}));
    assert_eq!(id_of(&next), 7, "The next drop is above the last member");
    assert_eq!(local_dump(doc(&s), 8), local_dump(doc(&s), 2));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_drop_of_a_group_master_is_still_one_undo_step_and_takes_the_copies_away() {
    let dir = Dir::new();
    let bytes = VsdxBuilder::new().document_type(CT_STENCIL).master(group_master(&cluster_members())).build();
    let mut s = blank_session();
    open(&mut s, &dir.write("group.vssx", &bytes));
    let before = snapshot(&s);
    run(&mut s, "shape.drop", json!({"master": "Cluster", "stencil": "file-1", "page": 1, "x": 4, "y": 4}));
    assert!(!local_sections(doc(&s), 2).is_empty(), "the members hold the copy");
    assert_eq!(s.history().unwrap().undo_len(), before.undo_steps + 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s), before.document.as_ref().unwrap());
}

#[test]
fn a_master_of_several_shapes_drops_as_a_group_whose_members_hold_their_geometry() {
    let dir = Dir::new();
    let pair = MasterSpec::new(2, "Pair").shapes(&format!(
        "{}{}",
        shape(5, &format!("{LOOK}{}", box_cells(1.0, 0.6))),
        shape_with(6, "", &format!("{LOOK}{}", box_cells(0.7, 0.9)))
    ));
    let bytes = VsdxBuilder::new().document_type(CT_STENCIL).master(pair).build();
    let mut s = blank_session();
    open(&mut s, &dir.write("pair.vssx", &bytes));
    let r = run(&mut s, "shape.drop", json!({"master": "Pair", "stencil": "file-1", "page": 1, "x": 4, "y": 4}));
    assert_eq!(id_of(&r), 1);
    let d = doc(&s);
    assert!(local_sections(d, 1).is_empty(), "the group made of the master's shapes has no geometry of its own to copy");
    for id in [2, 3] {
        assert_eq!(local_sections(d, id).len(), 1, "member {id}");
        // The pin and the size of a member of a group made of a master's several shapes are formulas of the group.
        assert!(tied_to(d, id, 1), "member {id} is tied to the group instance");
    }
    assert!(own(d, 1, "Width").is_some_and(|c| c.formula.text().is_none()), "the instance itself holds numbers");
    assert_eq!(effective(d, 2).len(), 5, "the box of the first member: its five rows");
    // The group made is resized: its members follow by the formulas.
    let (w, h) = (crate::edit::num(d, 1, "Width"), crate::edit::num(d, 1, "Height"));
    let was: Vec<[f64; 4]> = [2, 3].map(|id| FOLLOWS.map(|(n, _)| crate::edit::num(d, id, n))).to_vec();
    crate::group_size::resize_group(&mut s, 1, w * 2.0, h * 0.5);
    let d = doc(&s);
    for (k, id) in [2_u32, 3].into_iter().enumerate() {
        let now = FOLLOWS.map(|(n, _)| crate::edit::num(d, id, n));
        let scale = [2.0, 0.5, 2.0, 0.5];
        for n in 0..4 {
            assert!((now[n] - was[k][n] * scale[n]).abs() < 1e-9, "member {id} {}: {} from {}", FOLLOWS[n].0, now[n], was[k][n]);
        }
    }
    // Grouped with another shape: the group instance is a member and has nothing to copy, and is tied to the new group; its own members are as they were.
    let (own2, own3) = (local_dump(d, 2), local_dump(d, 3));
    let (cells2, cells3) = (cells_dump(d, 2), cells_dump(d, 3));
    let rect = drop_shape(&mut s, "rectangle", 8.0, 4.0);
    let outer = group(&mut s, &[1, rect]);
    assert_eq!((local_dump(doc(&s), 2), local_dump(doc(&s), 3)), (own2, own3));
    assert_eq!((cells_dump(doc(&s), 2), cells_dump(doc(&s), 3)), (cells2, cells3));
    assert!(!local_sections(doc(&s), rect).is_empty());
    assert!(tied_to(doc(&s), rect, outer) && tied_to(doc(&s), 1, outer));
    assert_eq!(check_session(&s), Ok(()));
}

// ---- limits ----------------------------------------------------------------------------------------------------------------------

/// A master "Big" (ID 1) whose one shape has a `Geometry` section of `rows` rows, and a page of `n` instances of it.
fn big_drawing(rows: u32, n: u32) -> Vec<u8> {
    let mut body = String::from(
        r#"<Cell N="PinX" V="1" F="Width*0.5"/><Cell N="PinY" V="1" F="Height*0.5"/><Cell N="Width" V="2"/><Cell N="Height" V="2"/><Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="1" F="Height*0.5"/><Section N="Geometry" IX="0">"#,
    );
    for k in 1..=rows {
        let t = if k == 1 { "MoveTo" } else { "LineTo" };
        body.push_str(&format!(r#"<Row T="{t}" IX="{k}"><Cell N="X" V="{}" F="Width*0.5"/><Cell N="Y" V="{}"/></Row>"#, k % 3, k % 2));
    }
    body.push_str("</Section>");
    let master = MasterSpec::new(1, "Big").shapes(&shape(4, &body));
    let page: String =
        (0..n).map(|k| shape_with(10 + k, r#" Master="1""#, &format!(r#"<Cell N="PinX" V="{}"/><Cell N="PinY" V="3"/>"#, 1 + k))).collect();
    VsdxBuilder::new().master(master).page(PageSpec::new(1, "Page-1").sheet(PAGE).shapes(&page)).build()
}

fn open_bytes(dir: &Dir, bytes: &[u8]) -> Session {
    let path = dir.path("big.vsdx");
    std::fs::write(&path, bytes).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    s
}

#[test]
fn a_group_that_would_copy_more_geometry_than_the_cap_is_refused_in_words_and_changes_nothing_and_quickly() {
    // 600 rows of two cells and the row itself: 1,800 units for a shape, so 700 shapes are over a million and 100 are not.
    let dir = Dir::new();
    let mut s = open_bytes(&dir, &big_drawing(600, 700));
    let before = snapshot(&s);
    let ids: Vec<u32> = (10..710).collect();
    let started = std::time::Instant::now();
    let e = s.execute("shape.group", &json!({"ids": ids})).unwrap_err().to_string();
    assert!(started.elapsed().as_secs() < 30, "a refusal is cheap: {:?}", started.elapsed());
    assert!(e.contains("more than 1000000") && e.contains("fewer shapes"), "{e}");
    let now = snapshot(&s);
    assert_eq!(now.document, before.document);
    assert_eq!((now.undo_steps, now.revision), (before.undo_steps, before.revision));
    // A hundred are within it, and are copied.
    let ids: Vec<u32> = (10..110).collect();
    let started = std::time::Instant::now();
    run(&mut s, "shape.group", json!({"ids": ids}));
    assert!(started.elapsed().as_secs() < 60, "{:?}", started.elapsed());
    assert!(!local_sections(doc(&s), 10).is_empty() && !local_sections(doc(&s), 109).is_empty());
    assert!(local_sections(doc(&s), 200).is_empty());
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_master_of_a_section_of_the_most_rows_the_sample_files_hold_is_copied_once_and_quickly() {
    // The largest `Geometry` section of the runs over the sample files holds 31,273 rows. One instance, grouped with a second that is a plain box.
    let dir = Dir::new();
    let mut s = open_bytes(&dir, &big_drawing(31_273, 2));
    let started = std::time::Instant::now();
    run(&mut s, "shape.group", json!({"ids": [10, 11]}));
    assert!(started.elapsed().as_secs() < 60, "{:?}", started.elapsed());
    let d = doc(&s);
    assert_eq!(local_sections(d, 10)[0].rows.len(), 31_273);
    assert_eq!(effective(d, 10).len(), 31_273);
    assert_eq!(check_session(&s), Ok(()));
}

/// A group master of `leaves` boxes, each with a `Geometry` section of `rows` rows of two cells.
fn group_master_of_leaves(leaves: u32, rows: u32) -> Vec<u8> {
    let mut geometry = String::from(r#"<Section N="Geometry" IX="0">"#);
    for k in 1..=rows {
        let t = if k == 1 { "MoveTo" } else { "LineTo" };
        geometry.push_str(&format!(r#"<Row T="{t}" IX="{k}"><Cell N="X" V="{}" F="Width*0.5"/><Cell N="Y" V="{}"/></Row>"#, k % 3, k % 2));
    }
    geometry.push_str("</Section>");
    let leaf = |k: u32| {
        shape(
            10 + k,
            &format!(
                r#"<Cell N="PinX" V="1"/><Cell N="PinY" V="1"/><Cell N="Width" V="2"/><Cell N="Height" V="2"/><Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="1" F="Height*0.5"/>{geometry}"#
            ),
        )
    };
    let members: String = (0..leaves).map(leaf).collect();
    VsdxBuilder::new().document_type(CT_STENCIL).master(group_master(&members)).build()
}

#[test]
fn a_drop_whose_members_hold_more_geometry_than_the_cap_lists_them_without_a_copy_and_is_not_refused() {
    let dir = Dir::new();
    // 5,000 leaves of 70 rows: about 1.05 million sections, rows and cells in all, over the cap; of 40 rows, about 0.6 million, within it.
    let mut s = blank_session();
    open(&mut s, &dir.write("over.vssx", &group_master_of_leaves(5_000, 70)));
    let started = std::time::Instant::now();
    let r = run(&mut s, "shape.drop", json!({"master": "Cluster", "stencil": "file-1", "page": 1, "x": 4, "y": 4}));
    assert!(started.elapsed().as_secs() < 60, "{:?}", started.elapsed());
    assert_eq!(id_of(&r), 1);
    let d = doc(&s);
    assert_eq!(shape_of(d, 1).children.len(), 5_000, "every member is listed");
    assert!(local_sections(d, 2).is_empty() && local_sections(d, 5_001).is_empty(), "and none holds a copy");
    assert_eq!(effective(d, 2).len(), 70, "they draw from the master as they did");
    assert_eq!(check_session(&s), Ok(()));
    // The first session holds a drawing of a million rows, and the process holds it as long as it is open: the second is held to what it leaves.
    drop(s);
    let mut s = blank_session();
    open(&mut s, &dir.write("within.vssx", &group_master_of_leaves(5_000, 40)));
    let started = std::time::Instant::now();
    run(&mut s, "shape.drop", json!({"master": "Cluster", "stencil": "file-1", "page": 1, "x": 4, "y": 4}));
    assert!(started.elapsed().as_secs() < 60, "{:?}", started.elapsed());
    let d = doc(&s);
    assert!(!local_sections(d, 2).is_empty() && !local_sections(d, 5_001).is_empty(), "within the cap every member holds its copy");
    assert_eq!(local_sections(d, 5_001)[0].rows.len(), 40);
    assert_eq!(check_session(&s), Ok(()));
}
