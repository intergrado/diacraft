//! A member that is an instance holds what the files hold of a nested instance: its `Width`, `Height`, `PinX` and `PinY` as formulas of
//! the group it is in. The drawing does not change when they are written, a resize of the group moves the members by them, and the
//! cells where R50 gives no form keep their numbers.
//!
//! The masters are the built-in stencils' and masters built in code from the specifications' XML; no file from another application is used.

use diacraft_calc::{Budget, Changes, EvalOptions, commit_changed};
use diacraft_doc::{Cell, CellAddr, CellEdit, CellName, Document, Owner, SheetRef, Value, WriteOrigin};
use diacraft_engine::{EngineError, Session};
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape, shape_with};
use serde_json::json;

use crate::edit::{doc, drop_shape};
use crate::group_geometry::{PAGE, assert_same_picture, cells_dump, four, group, local_dump, render, shape_of};
use crate::stencil_open::{CT_STENCIL, Dir, LOOK, box_cells, open, with_stencil};
use crate::{check_session, run};

/// A cell a shape holds itself, directly under it.
pub(crate) fn own(d: &Document, id: u32, name: &str) -> Option<Cell> {
    shape_of(d, id).sheet.cells.get(&CellName::new(name)).cloned()
}

/// What a cell is now by inheritance: its number.
pub(crate) fn number(d: &Document, id: u32, name: &str) -> f64 {
    crate::edit::num(d, id, name)
}

/// The four cells, by name, and the cell of the group each follows.
pub(crate) const FOLLOWS: [(&str, &str); 4] = [("Width", "Width"), ("Height", "Height"), ("PinX", "Width"), ("PinY", "Height")];

/// The factor of a formula `Sheet.<group>!<cell>*<factor>`, when it is one.
pub(crate) fn factor(formula: &str, group: u32, cell: &str) -> Option<f64> {
    formula.strip_prefix(&format!("Sheet.{group}!{cell}*"))?.parse().ok()
}

/// Whether a shape's cell is a formula of group `group` (`Sheet.<group>!Width` or `Height` times a number).
fn tied(d: &Document, id: u32, name: &str, group: u32) -> bool {
    let cell_of_group = FOLLOWS.iter().find(|(n, _)| *n == name).map(|(_, g)| *g).unwrap();
    own(d, id, name).and_then(|c| c.formula.text().map(str::to_owned)).is_some_and(|t| factor(&t, group, cell_of_group).is_some())
}

/// The page's point that is the origin of the group's frame: its pin less its local pin (the group is neither turned nor flipped).
fn origin_of(d: &Document, group: u32) -> (f64, f64) {
    (number(d, group, "PinX") - number(d, group, "LocPinX"), number(d, group, "PinY") - number(d, group, "LocPinY"))
}

/// The four numbers of a shape: `Width`, `Height`, `PinX`, `PinY`.
fn four_numbers(d: &Document, id: u32) -> [f64; 4] {
    FOLLOWS.map(|(name, _)| number(d, id, name))
}

/// Changes the group's size the way a ShapeSheet edit does: the cells written, and what reads them worked out.
pub(crate) fn resize_group(s: &mut Session, group: u32, width: f64, height: f64) {
    s.edit("resize the group", |d| {
        let failed = |e: String| EngineError::Failed { cmd: "test".into(), msg: e };
        let sheet = SheetRef::Shape(d.find_shape(Owner::Page(1), group).ok_or_else(|| failed("no group".into()))?);
        for (name, v) in [("Width", width), ("Height", height)] {
            d.write_cell(&sheet, &CellAddr::direct(name), CellEdit::Value(Value::from_text(&format!("{v}"))), WriteOrigin::Drawing)
                .map_err(|e| failed(e.to_string()))?;
        }
        let cells = ["Width", "Height"].map(|name| (sheet.clone(), CellAddr::direct(name))).to_vec();
        let report = commit_changed(d, &Budget::default(), EvalOptions::default(), 1, &Changes { cells, ..Changes::default() });
        assert!(!report.budget_exhausted);
        Ok(())
    })
    .unwrap();
}

// ---- what is written ---------------------------------------------------------------------------------------------------------------

#[test]
fn instances_made_members_hold_width_height_and_pin_as_formulas_of_their_group() {
    let mut s = four();
    let was: Vec<[f64; 4]> = (1..=4).map(|id| four_numbers(doc(&s), id)).collect();
    let g = group(&mut s, &[1, 2, 3]);
    let d = doc(&s);
    let (gw, gh) = (number(d, g, "Width"), number(d, g, "Height"));
    let (ox, oy) = origin_of(d, g);
    for id in 1..=3_u32 {
        let k = id as usize - 1;
        // The size is the shape's own; the pin is in the group's frame, which has the box's lower-left corner for its origin.
        let want = [was[k][0], was[k][1], was[k][2] - ox, was[k][3] - oy];
        for (n, (name, group_cell)) in FOLLOWS.iter().enumerate() {
            let cell = own(d, id, name).unwrap_or_else(|| panic!("shape {id} holds no {name}"));
            let text = cell.formula.text().unwrap_or_else(|| panic!("shape {id}: {name} is not a formula"));
            let k_of = factor(text, g, group_cell).unwrap_or_else(|| panic!("shape {id}: {name} is `{text}`"));
            let dim = if *group_cell == "Width" { gw } else { gh };
            assert!((k_of * dim - want[n]).abs() < 1e-9, "shape {id} {name}: {k_of} of {dim} is not {}", want[n]);
            assert!((number(d, id, name) - want[n]).abs() < 1e-9, "shape {id} {name}: the value it holds");
            assert_eq!((cell.unit.as_deref(), cell.error.as_deref()), (Some("IN"), None), "shape {id} {name}");
        }
    }
    // The shape left out holds what it did, and the group its numbers.
    assert!(own(d, 4, "Width").is_none() && own(d, 4, "PinX").is_some_and(|c| c.formula.text().is_none()));
    assert!(own(d, g, "Width").is_some_and(|c| c.formula.text().is_none()), "the group's size is a number of its own");
    // Nothing else is tied: R9 adds no other cell, and the inherited ones are `calc`'s once a result differs.
    for id in 1..=3 {
        let names: Vec<String> = shape_of(d, id).sheet.cells.keys().map(|n| n.as_str().to_owned()).collect();
        assert_eq!(
            names,
            ["Height", "PinX", "PinY", "Width"],
            "shape {id}: no `LocPinX` or other cell directly under it (the copy of the geometry is)"
        );
    }
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn the_picture_is_the_same_when_the_formulas_are_written() {
    let mut s = four();
    let before = render(&s);
    group(&mut s, &[1, 2, 3, 4]);
    assert_same_picture(&s, &before, "a group of four instances");
    // A group of a group and another instance: the inner group is a member that holds no master, the instance is tied to the outer group.
    let extra = drop_shape(&mut s, "rectangle", 9.0, 7.0);
    let before = render(&s);
    group(&mut s, &[5, extra]);
    assert_same_picture(&s, &before, "a group of a group and an instance");
}

#[test]
fn a_resize_of_the_group_moves_and_sizes_its_members_by_the_formulas() {
    let (sx, sy) = (1.5, 0.8);
    let mut s = four();
    let boxes: Vec<(f64, f64, f64, f64)> = (1..=3)
        .map(|id| {
            let d = doc(&s);
            (
                number(d, id, "PinX") - number(d, id, "LocPinX"),
                number(d, id, "PinY") - number(d, id, "LocPinY"),
                number(d, id, "Width"),
                number(d, id, "Height"),
            )
        })
        .collect();
    let g = group(&mut s, &[1, 2, 3]);
    let (x0, y0) = origin_of(doc(&s), g);
    let (gw, gh) = (number(doc(&s), g, "Width"), number(doc(&s), g, "Height"));
    let members_before: Vec<[f64; 4]> = (1..=3).map(|id| four_numbers(doc(&s), id)).collect();
    resize_group(&mut s, g, gw * sx, gh * sy);
    let d = doc(&s);
    for id in 1..=3_u32 {
        let k = id as usize - 1;
        let now = four_numbers(d, id);
        let want = [members_before[k][0] * sx, members_before[k][1] * sy, members_before[k][2] * sx, members_before[k][3] * sy];
        for n in 0..4 {
            assert!((now[n] - want[n]).abs() < 1e-9, "shape {id} {}: {} is not {}", FOLLOWS[n].0, now[n], want[n]);
        }
        // What the master's formulas give from the size is worked out again on the member.
        assert!(
            (number(d, id, "LocPinX") - now[0] * 0.5).abs() < 1e-9 && (number(d, id, "LocPinY") - now[1] * 0.5).abs() < 1e-9,
            "shape {id}: the local pin"
        );
    }
    assert_eq!(check_session(&s), Ok(()));
    // The same drawing, made by resizing each shape alone: the group grows about its pin, so its origin moves.
    let (nx0, ny0) = origin_of(d, g);
    let mut alone = four();
    for (id, (left, bottom, w, h)) in (1..=3).zip(boxes) {
        let (l, b) = (nx0 + (left - x0) * sx, ny0 + (bottom - y0) * sy);
        run(&mut alone, "shape.resize", json!({"id": id, "x": l, "y": b, "width": w * sx, "height": h * sy}));
    }
    assert_same_picture(&s, &render(&alone), "the group resized, and the same shapes resized one by one");
}

#[test]
fn a_member_that_is_a_group_of_ours_holds_numbers_and_the_instance_beside_it_is_tied() {
    let mut s = four();
    let inner = group(&mut s, &[1, 2]);
    let outer = group(&mut s, &[inner, 3]);
    for (name, _) in FOLLOWS {
        assert!(
            own(doc(&s), inner, name).is_some_and(|c| c.formula.text().is_none()),
            "a group made by `shape.group` holds no master: its {name} is a number"
        );
    }
    assert!(tied(doc(&s), 3, "Width", outer), "the instance named is tied to the outer group");
}

// ---- where R50 gives no form --------------------------------------------------------------------------------------------------------

#[test]
fn a_line_keeps_its_numbers_and_the_box_beside_it_is_tied() {
    let (mut s, _dir) = with_stencil();
    let drop = |s: &mut Session, master: &str, x: f64, y: f64| {
        let r = run(s, "shape.drop", json!({"master": master, "stencil": "file-1", "page": 1, "x": x, "y": y}));
        u32::try_from(r["id"].as_u64().unwrap()).unwrap()
    };
    let (boxed, line) = (drop(&mut s, "Process", 3.0, 3.0), drop(&mut s, "Link", 7.0, 5.0));
    let before = render(&s);
    let g = group(&mut s, &[boxed, line]);
    let d = doc(&s);
    for (name, _) in FOLLOWS {
        assert!(tied(d, boxed, name, g), "the box: {name}");
        let text = own(d, line, name).and_then(|c| c.formula.text().map(str::to_owned));
        assert!(text.is_none_or(|t| !t.contains("Sheet.")), "the line's {name} is a formula of its ends or a number, not of the group");
    }
    assert_same_picture(&s, &before, "a box and a line grouped");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_member_turned_by_other_than_a_half_turn_keeps_its_size_and_is_tied_by_its_pin() {
    for (degrees, sizes_tied) in [(90.0, false), (35.0, false), (180.0, true), (0.0, true)] {
        let mut s = four();
        if degrees != 0.0 {
            run(&mut s, "shape.rotate", json!({"ids": [1], "by": degrees}));
        }
        let before = render(&s);
        let g = group(&mut s, &[1, 2]);
        let d = doc(&s);
        for (name, is_size) in [("Width", true), ("Height", true), ("PinX", false), ("PinY", false)] {
            assert_eq!(tied(d, 1, name, g), !is_size || sizes_tied, "{degrees} degrees: {name}");
        }
        assert!(tied(d, 2, "Width", g), "the shape that is not turned is tied");
        assert_same_picture(&s, &before, &format!("a member turned by {degrees}"));
    }
}

/// The master "Box" (ID 1): a box 2 by 1.
fn box_master() -> MasterSpec {
    MasterSpec::new(1, "Box").shapes(&shape(4, &format!("{LOOK}{}", box_cells(2.0, 1.0))))
}

/// A drawing from a file: the master "Box" and a page of these shapes.
fn opened(dir: &Dir, shapes: &str) -> Session {
    let path = dir.path("in.vsdx");
    std::fs::write(&path, VsdxBuilder::new().master(box_master()).page(PageSpec::new(1, "Page-1").sheet(PAGE).shapes(shapes)).build()).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    s
}

fn instance(id: u32, cells: &str) -> String {
    shape_with(id, r#" Master="1""#, cells)
}

#[test]
fn a_cell_that_holds_a_formula_already_keeps_it() {
    let dir = Dir::new();
    let shapes = format!(
        "{}{}",
        instance(11, r#"<Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="Width" V="2" F="GUARD(2)"/><Cell N="Height" V="1" F="Width*0.5"/>"#),
        instance(12, r#"<Cell N="PinX" V="5"/><Cell N="PinY" V="2"/>"#)
    );
    let mut s = opened(&dir, &shapes);
    let before = render(&s);
    let g = group(&mut s, &[11, 12]);
    let d = doc(&s);
    assert_eq!(own(d, 11, "Width").unwrap().formula.text(), Some("GUARD(2)"), "a formula of the file's is data");
    assert_eq!(own(d, 11, "Height").unwrap().formula.text(), Some("Width*0.5"));
    assert!(tied(d, 11, "PinX", g) && tied(d, 11, "PinY", g), "the pin had no formula");
    assert!(tied(d, 12, "Width", g) && tied(d, 12, "Height", g));
    assert_same_picture(&s, &before, "instances holding formulas, grouped");
}

#[test]
fn a_group_of_no_extent_along_an_axis_leaves_the_cells_on_that_axis_as_numbers() {
    // Two boxes of no width on one vertical line: the group is as wide as nothing, and no factor reaches a width or a pin along x.
    let dir = Dir::new();
    let flat = |id: u32, y: f64| {
        instance(id, &format!(r#"<Cell N="PinX" V="3"/><Cell N="PinY" V="{y}"/><Cell N="Width" V="0"/><Cell N="LocPinX" V="0" F="Inh"/>"#))
    };
    let mut s = opened(&dir, &format!("{}{}", flat(11, 2.0), flat(12, 5.0)));
    let g = group(&mut s, &[11, 12]);
    let d = doc(&s);
    assert!(number(d, g, "Width").abs() < 1e-9 && number(d, g, "Height") > 1.0);
    for id in [11, 12] {
        assert!(!tied(d, id, "Width", g) && !tied(d, id, "PinX", g), "shape {id}: along x");
        assert!(tied(d, id, "Height", g) && tied(d, id, "PinY", g), "shape {id}: along y");
    }
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_shape_with_no_master_is_not_touched_but_for_where_the_group_carries_it() {
    let dir = Dir::new();
    let plain = shape(20, &format!("{LOOK}{}", box_cells(1.0, 1.0)));
    let shapes = format!("{}{plain}", instance(11, r#"<Cell N="PinX" V="2"/><Cell N="PinY" V="2"/>"#));
    let mut s = opened(&dir, &shapes);
    let was = cells_dump(doc(&s), 20);
    let g = group(&mut s, &[11, 20]);
    let d = doc(&s);
    assert!(tied(d, 11, "Width", g));
    // Its size and local pin are as they were; only its pin was carried into the group's frame, as a number.
    let now = cells_dump(d, 20);
    assert_eq!(was.len(), now.len());
    for (a, b) in was.iter().zip(&now) {
        if !a.starts_with("PinX") && !a.starts_with("PinY") {
            assert_eq!(a, b);
        }
    }
    assert!(FOLLOWS.iter().all(|(name, _)| !tied(d, 20, name, g)), "a shape with no master is tied to nothing");
}

#[test]
fn a_member_regrouped_inside_a_group_is_tied_to_the_new_group_and_a_formula_of_another_shape_is_kept() {
    // A group (20) whose two instances are tied to it as a file ties them; two of them are grouped inside it. 22's width is a formula of a shape (30) that is
    // not the group it leaves, which is data of the file's.
    let dir = Dir::new();
    let cells = |x: f64, width: &str| {
        format!(
            r#"<Cell N="PinX" V="{x}" F="Sheet.20!Width*{k}"/><Cell N="PinY" V="1" F="Sheet.20!Height*0.5"/>{width}<Cell N="Height" V="1" F="Sheet.20!Height*0.5"/>"#,
            k = x / 4.0
        )
    };
    let group_cells = r#"<Cell N="PinX" V="4"/><Cell N="PinY" V="3"/><Cell N="Width" V="4"/><Cell N="Height" V="2"/><Cell N="LocPinX" V="2" F="Width*0.5"/><Cell N="LocPinY" V="1" F="Height*0.5"/>"#;
    let in_group = cells(1.0, r#"<Cell N="Width" V="2" F="Sheet.20!Width*0.5"/>"#);
    let to_other = cells(3.0, r#"<Cell N="Width" V="2" F="Sheet.30!Width*1"/>"#);
    let outer = shape_with(20, r#" Type="Group""#, &format!("{group_cells}<Shapes>{}{}</Shapes>", instance(21, &in_group), instance(22, &to_other)));
    let free = instance(30, r#"<Cell N="PinX" V="9"/><Cell N="PinY" V="6"/><Cell N="Width" V="2"/>"#);
    let mut s = opened(&dir, &format!("{outer}{free}"));
    let before = render(&s);
    let inner = group(&mut s, &[21, 22]);
    let d = doc(&s);
    assert_eq!(shape_of(d, 20).children.iter().map(|c| c.id).collect::<Vec<_>>(), [inner]);
    for (name, _) in FOLLOWS {
        assert!(tied(d, 21, name, inner), "21 {name}: a tie to the group it left follows the group it is in");
        assert_eq!(tied(d, 22, name, inner), name != "Width", "22 {name}");
    }
    assert_eq!(own(d, 22, "Width").unwrap().formula.text(), Some("Sheet.30!Width*1"), "a formula of another shape is data the file holds");
    assert_same_picture(&s, &before, "members regrouped inside their group");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn hostile_numbers_in_the_cells_of_a_member_never_crash_and_keep_the_values_that_cannot_have_a_factor() {
    for hostile in ["1e300", "-1e300", "1e-300", "4.9e-324", "1.7976931348623157e308", "nan", "inf", "1e22", "-0", "123456789012345678901234567890"] {
        let dir = Dir::new();
        let weird =
            instance(11, &format!(r#"<Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="Width" V="{hostile}"/><Cell N="Height" V="{hostile}"/>"#));
        let plain = instance(12, r#"<Cell N="PinX" V="5"/><Cell N="PinY" V="2"/>"#);
        let mut s = opened(&dir, &format!("{weird}{plain}"));
        let before = doc(&s).clone();
        match s.execute("shape.group", &json!({"ids": [11, 12]})) {
            Ok(_) => assert_eq!(check_session(&s), Ok(()), "{hostile}"),
            Err(_) => assert_eq!(doc(&s), &before, "{hostile}: a refusal changes nothing"),
        }
        // Whatever came of it, the formulas written are short: a factor outside its range is kept as a number.
        let d = doc(&s);
        for id in [11, 12] {
            for (name, _) in FOLLOWS {
                if let Some(text) = own(d, id, name).and_then(|c| c.formula.text().map(str::to_owned)) {
                    let factor_part = text.rsplit('*').next().unwrap_or("");
                    assert!(text.len() < 64 && !factor_part.contains(['e', 'E']), "{hostile}: {name} = {text}");
                }
            }
        }
    }
}

// ---- ungroup ---------------------------------------------------------------------------------------------------------------------

#[test]
fn the_instances_an_ungroup_leaves_in_a_group_are_tied_to_it_and_follow_it() {
    let mut s = four();
    let inner = group(&mut s, &[1, 2]);
    let outer = group(&mut s, &[inner, 3]);
    let before = render(&s);
    run(&mut s, "shape.ungroup", json!({"ids": [inner]}));
    let d = doc(&s);
    assert_eq!(shape_of(d, outer).children.iter().map(|c| c.id).collect::<Vec<_>>(), [1, 2, 3], "the members are in the outer group now");
    for id in [1, 2, 3] {
        for (name, _) in FOLLOWS {
            assert!(tied(d, id, name, outer), "shape {id} {name} follows the group it is in");
        }
    }
    assert_same_picture(&s, &before, "a group inside a group taken apart");
    // The outer group is made wider: the members that were in the inner group follow it as the one that was beside it does.
    let was = four_numbers(d, 1);
    let (w, h) = (number(d, outer, "Width"), number(d, outer, "Height"));
    resize_group(&mut s, outer, w * 2.0, h);
    let now = four_numbers(doc(&s), 1);
    assert!((now[0] - 2.0 * was[0]).abs() < 1e-9 && (now[1] - was[1]).abs() < 1e-9 && (now[2] - 2.0 * was[2]).abs() < 1e-9, "{was:?} to {now:?}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_group_taken_apart_on_the_page_leaves_numbers_and_ties_nothing() {
    let mut s = four();
    let g = group(&mut s, &[1, 2]);
    run(&mut s, "shape.ungroup", json!({"ids": [g]}));
    for id in [1, 2] {
        for (name, _) in FOLLOWS {
            assert!(own(doc(&s), id, name).is_some_and(|c| c.formula.text().is_none() && c.error.is_none()), "shape {id} {name}");
        }
    }
}

#[test]
fn a_plain_group_inside_a_group_and_the_shapes_with_no_master_do_not_follow_a_resize_of_the_group() {
    // Group 20 holds a group 30 of an instance (31) and a shape with no master (32), and an instance (33) beside it. Ungrouping 30 ties the instance
    // to 20, and leaves the shape with no master (and the group 30 itself, a group with no master, while it is there) as numbers (R50 counts instances).
    let dir = Dir::new();
    let plain = shape(32, &format!("{LOOK}{}", box_cells(1.0, 1.0)));
    let inner_cells = r#"<Cell N="PinX" V="2"/><Cell N="PinY" V="1"/><Cell N="Width" V="2"/><Cell N="Height" V="2"/><Cell N="LocPinX" V="1"/><Cell N="LocPinY" V="1"/>"#;
    let inner = shape_with(
        30,
        r#" Type="Group""#,
        &format!("{inner_cells}<Shapes>{}{plain}</Shapes>", instance(31, r#"<Cell N="PinX" V="0.5"/><Cell N="PinY" V="0.5"/>"#)),
    );
    let outer_cells = r#"<Cell N="PinX" V="4"/><Cell N="PinY" V="3"/><Cell N="Width" V="4"/><Cell N="Height" V="4"/><Cell N="LocPinX" V="2"/><Cell N="LocPinY" V="2"/>"#;
    let outer = shape_with(
        20,
        r#" Type="Group""#,
        &format!("{outer_cells}<Shapes>{inner}{}</Shapes>", instance(33, r#"<Cell N="PinX" V="3"/><Cell N="PinY" V="3"/>"#)),
    );
    let mut s = opened(&dir, &outer);
    let before = render(&s);
    run(&mut s, "shape.ungroup", json!({"ids": [30]}));
    let d = doc(&s);
    assert!(FOLLOWS.iter().all(|(name, _)| tied(d, 31, name, 20)), "the instance lifted out is tied to the group it is in now");
    assert!(FOLLOWS.iter().all(|(name, _)| !tied(d, 32, name, 20)), "the shape with no master is not (R50 counts instances)");
    assert_same_picture(&s, &before, "a group in a group taken apart");
    // Resized, the group takes the instance with it and leaves the shape with no master where it is: the drawing is what it is, and says so in the docs.
    let (w, h) = (number(d, 20, "Width"), number(d, 20, "Height"));
    let (x31, x32) = (number(d, 31, "PinX"), number(d, 32, "PinX"));
    resize_group(&mut s, 20, w * 2.0, h);
    assert!((number(doc(&s), 31, "PinX") - 2.0 * x31).abs() < 1e-9);
    assert!((number(doc(&s), 32, "PinX") - x32).abs() < 1e-9, "a shape with no master does not follow");
    assert_eq!(check_session(&s), Ok(()));
}

// ---- what it costs -------------------------------------------------------------------------------------------------------------

/// A drawing of `n` instances of "Box", the first with `ID` 11, in rows.
fn many_instances(n: u32) -> Vec<u8> {
    let shapes: String = (0..n)
        .map(|k| {
            instance(11 + k, &format!(r#"<Cell N="PinX" V="{}"/><Cell N="PinY" V="{}"/>"#, 1.0 + f64::from(k % 100) * 0.1, 1.0 + f64::from(k / 100)))
        })
        .collect();
    VsdxBuilder::new().master(box_master()).page(PageSpec::new(1, "Page-1").sheet(PAGE).shapes(&shapes)).build()
}

fn open_bytes(dir: &Dir, bytes: &[u8]) -> Session {
    let path = dir.path("many.vsdx");
    std::fs::write(&path, bytes).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    s
}

/// The commits, the steps the recalculation of grouping `n` of these instances takes, and the rows of masters the copy of their geometry walks, counted
/// and not timed.
fn steps_of_grouping(n: u32) -> (u64, u64, u64) {
    let dir = Dir::new();
    let mut s = open_bytes(&dir, &many_instances(n));
    let ids: Vec<u32> = (11..11 + n).collect();
    diacraft_calc::work::reset();
    diacraft_doc::work::reset();
    run(&mut s, "shape.group", json!({"ids": ids}));
    let d = doc(&s);
    assert!(tied(d, 11, "PinX", 11 + n) && tied(d, 10 + n, "Height", 11 + n), "every member is tied");
    (diacraft_calc::work::commits(), diacraft_calc::work::steps(), diacraft_doc::work::rows_merged())
}

#[test]
fn grouping_a_thousand_instances_costs_in_proportion_to_the_instances_and_commits_once() {
    let (small_commits, small, small_rows) = steps_of_grouping(250);
    let (large_commits, large, large_rows) = steps_of_grouping(1_000);
    assert_eq!(small_commits, large_commits, "a fixed number of commits, whatever the members");
    assert!(small > 0 && large > 0 && small_rows > 0, "the counters count: {small} {large} {small_rows}");
    let ratio = large as f64 / small as f64;
    assert!((3.5..4.5).contains(&ratio), "{small} steps for 250 instances and {large} for 1,000: a ratio of {ratio}, where linear is 4");
    // The copy of the geometry reads each master row once for each instance: the rows it walks are linear too.
    let rows = large_rows as f64 / small_rows as f64;
    assert!((3.5..4.5).contains(&rows), "{small_rows} rows for 250 instances and {large_rows} for 1,000: a ratio of {rows}, where linear is 4");
}

#[test]
fn grouping_four_thousand_instances_the_most_one_command_names_is_done() {
    let dir = Dir::new();
    let mut s = open_bytes(&dir, &many_instances(4_000));
    let ids: Vec<u32> = (11..4_011).collect();
    let g = group(&mut s, &ids);
    assert!(tied(doc(&s), 4_010, "PinY", g) && tied(doc(&s), 11, "Width", g));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn groups_inside_groups_sixty_deep_tie_each_instance_to_its_own_group_and_stay_valid() {
    // Each step groups the group so far with a new instance. (The work of a level does not grow with the depth: a unit test of `draw/nested.rs` counts it.)
    let mut s = four();
    let mut inner = group(&mut s, &[1, 2]);
    for depth in 0..60 {
        let id = drop_shape(&mut s, "rectangle", 1.0 + f64::from(depth % 9), 7.0);
        let before = render(&s);
        let outer = group(&mut s, &[inner, id]);
        assert!(tied(doc(&s), id, "PinX", outer) && tied(doc(&s), id, "Height", outer), "depth {depth}");
        if depth % 20 == 0 {
            assert_same_picture(&s, &before, &format!("depth {depth}"));
        }
        inner = outer;
    }
    assert_eq!(check_session(&s), Ok(()));
}

// ---- save and read back ---------------------------------------------------------------------------------------------------------

#[test]
fn a_file_that_holds_these_cells_is_untouched_by_an_unrelated_edit_and_reads_back_the_same() {
    let dir = Dir::new();
    let cells = |x: f64| {
        format!(
            r#"<Cell N="PinX" V="{x}" F="Sheet.20!Width*{k}" U="IN"/><Cell N="PinY" V="1" F="Sheet.20!Height*0.5" U="IN"/><Cell N="Width" V="2" F="Sheet.20!Width*0.5" U="IN"/><Cell N="Height" V="1" F="Sheet.20!Height*0.5" U="IN"/>"#,
            k = x / 4.0
        )
    };
    let group_cells = r#"<Cell N="PinX" V="4"/><Cell N="PinY" V="3"/><Cell N="Width" V="4"/><Cell N="Height" V="2"/><Cell N="LocPinX" V="2" F="Width*0.5"/><Cell N="LocPinY" V="1" F="Height*0.5"/>"#;
    let nested =
        shape_with(20, r#" Type="Group""#, &format!("{group_cells}<Shapes>{}{}</Shapes>", instance(21, &cells(1.0)), instance(22, &cells(3.0))));
    let free = instance(30, r#"<Cell N="PinX" V="8"/><Cell N="PinY" V="6"/>"#);
    let mut s = opened(&dir, &format!("{nested}{free}"));
    let was: Vec<Vec<String>> = [21, 22].map(|id| cells_dump(doc(&s), id)).to_vec();
    let sections: Vec<Vec<String>> = [21, 22].map(|id| local_dump(doc(&s), id)).to_vec();
    // An edit that has nothing to do with them, and one of their group.
    run(&mut s, "shape.move", json!({"ids": [30], "dx": 1.0, "dy": 1.0}));
    run(&mut s, "shape.move", json!({"ids": [20], "dx": 0.5, "dy": 0.5}));
    for (k, id) in [21, 22].into_iter().enumerate() {
        assert_eq!(cells_dump(doc(&s), id), was[k], "member {id}: the cells, formulas and results");
        assert_eq!(local_dump(doc(&s), id), sections[k], "member {id}: no geometry written");
    }
    // Saved, read back, saved again: the same.
    let path = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    for (k, id) in [21, 22].into_iter().enumerate() {
        assert_eq!(cells_dump(doc(&t), id), was[k], "member {id} as saved");
    }
    let again = dir.path("again.vsdx");
    run(&mut t, "file.save_as", json!({"path": again}));
    assert_eq!(std::fs::read(&path).unwrap(), std::fs::read(&again).unwrap(), "a second save of the same drawing");
}

#[test]
fn a_drawing_grouped_here_and_saved_reads_back_with_the_same_cells() {
    let dir = Dir::new();
    let mut s = four();
    let g = group(&mut s, &[1, 2, 3]);
    let path = dir.path("grouped.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    for id in 1..=3 {
        assert_eq!(cells_dump(doc(&t), id), cells_dump(doc(&s), id), "shape {id}");
        for (name, _) in FOLLOWS {
            assert!(tied(doc(&t), id, name, g), "shape {id} {name} reads back as a formula of the group");
        }
    }
    // The reopened group is resized and its members follow, as they did before the save.
    let (w, h) = (number(doc(&t), g, "Width"), number(doc(&t), g, "Height"));
    let was = four_numbers(doc(&t), 2);
    resize_group(&mut t, g, w * 2.0, h * 2.0);
    let now = four_numbers(doc(&t), 2);
    for n in 0..4 {
        assert!((now[n] - 2.0 * was[n]).abs() < 1e-9, "{}", FOLLOWS[n].0);
    }
}

// ---- a member that is an instance of a group master --------------------------------------------------------------------------------

/// A group master (ID 1, "Frame"): a group 4 by 2 of one box, whose size and pin are formulas of the group as a master's members hold them.
fn frame_master() -> Vec<u8> {
    let member = shape_with(
        6,
        "",
        &format!(
            r#"{LOOK}<Cell N="PinX" V="1" F="Sheet.5!Width*0.25"/><Cell N="PinY" V="1" F="Sheet.5!Height*0.5"/><Cell N="Width" V="2" F="Sheet.5!Width*0.5"/><Cell N="Height" V="1" F="Sheet.5!Height*0.5"/>
<Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>
<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="3"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="1" F="Height"/></Row><Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="1" F="Height"/></Row><Row T="LineTo" IX="5"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>"#
        ),
    );
    VsdxBuilder::new()
        .document_type(CT_STENCIL)
        .master(MasterSpec::new(1, "Frame").shapes(&shape_with(5, r#" Type="Group""#, &format!("{}<Shapes>{member}</Shapes>", box_cells(4.0, 2.0)))))
        .build()
}

#[test]
fn a_group_instance_made_a_member_follows_its_group_and_its_own_members_follow_it() {
    let dir = Dir::new();
    let mut s = crate::blank_session();
    open(&mut s, &dir.write("frame.vssx", &frame_master()));
    let r = run(&mut s, "shape.drop", json!({"master": "Frame", "stencil": "file-1", "page": 1, "x": 4, "y": 4}));
    let frame = u32::try_from(r["id"].as_u64().unwrap()).unwrap();
    let boxed = drop_shape(&mut s, "rectangle", 8.0, 4.0);
    let member = frame + 1;
    assert_eq!(shape_of(doc(&s), frame).children.iter().map(|c| c.id).collect::<Vec<_>>(), [member], "the instance lists its member");
    let before = render(&s);
    let g = group(&mut s, &[frame, boxed]);
    let d = doc(&s);
    assert!(tied(d, frame, "Width", g) && tied(d, boxed, "Width", g));
    assert!(
        shape_of(d, member).sheet.cells.is_empty(),
        "the member of the instance holds nothing: it inherits the master's formulas of the instance"
    );
    assert_same_picture(&s, &before, "a group instance grouped");
    // The group is made twice as wide: the instance is, and its member is, by the formulas it inherits.
    let (w, h) = (number(d, g, "Width"), number(d, g, "Height"));
    let (frame_w, member_w) = (number(d, frame, "Width"), number(d, member, "Width"));
    resize_group(&mut s, g, w * 2.0, h);
    let d = doc(&s);
    assert!((number(d, frame, "Width") - 2.0 * frame_w).abs() < 1e-9, "the instance follows the group");
    assert!((number(d, member, "Width") - 2.0 * member_w).abs() < 1e-9, "its member follows it: {} from {member_w}", number(d, member, "Width"));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn the_group_command_still_refuses_what_it_refused() {
    let mut s = four();
    let before = doc(&s).clone();
    assert!(s.execute("shape.group", &json!({"ids": [1]})).is_err());
    assert!(s.execute("shape.group", &json!({"ids": [1, 99]})).is_err());
    assert_eq!(doc(&s), &before);
}
