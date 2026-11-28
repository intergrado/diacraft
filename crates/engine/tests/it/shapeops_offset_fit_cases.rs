//! Offset and Fit Curve, the rules beyond the vectors: needs and refusals, a master instance, a group, glue, containers
//! Undo and redo, the file, the pixels and the engine's own calls.

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, CellName, Document, RowKey, Shape, ShapeType, Value};
use diacraft_engine::Session;
use diacraft_engine::draw::{Op, check_shape_ops};
use diacraft_render::{Rendered, Renderer, View};
use serde_json::{Value as Json, json};

use crate::shapeops::{
    Build, assert_near, assert_unchanged, at, built, doc, error_of, flag_of, ids, line, num, put, rect, section_count, session_with, top,
};
use crate::shapeops_cases::{Dir, cells_of, edit_shape, has_id, id_of, instance};
use crate::{blank_session, run, snapshot};

/// The command and its params for each of the two, on these shapes.
fn two(ids: &[u32]) -> [(&'static str, Json); 2] {
    [("shape.offset", json!({"ids": ids, "distance": 0.25})), ("shape.fit_curve", json!({"ids": ids, "tolerance": 0.05, "flags": 2}))]
}

// ----: what each command needs -------------------------------------------------------------------------------------------

#[test]
fn e2_offset_and_fit_curve_take_one_shape_and_name_the_distance_and_the_tolerance_they_lack() {
    for (command, params) in [("shape.offset", json!({"ids": [2], "distance": 0.25})), ("shape.fit_curve", json!({"ids": [1], "tolerance": 0.05}))] {
        let mut s = session_with(vec![rect(1, (1.0, 1.0, 3.0, 3.0)).build(), rect(2, (5.0, 1.0, 7.0, 3.0)).build()]);
        run(&mut s, command, params);
    }
    let mut s = session_with(vec![rect(1, (1.0, 1.0, 3.0, 3.0)).build()]);
    let before = snapshot(&s);
    for (command, params, why) in [
        ("shape.offset", json!({"ids": [], "distance": 1}), "ids"),
        ("shape.offset", json!({"ids": [1]}), "distance"),
        ("shape.fit_curve", json!({"ids": [1]}), "tolerance"),
        ("shape.offset", json!({"ids": [99], "distance": 1}), "no shape 99"),
    ] {
        let error = error_of(&mut s, command, &params);
        assert!(error.to_string().contains(why), "{command}: {error}");
        assert_unchanged(&s, &before, command);
    }
}

#[test]
fn r12_a_group_a_guide_a_picture_a_connector_a_container_a_text_only_shape_and_a_locked_layer_are_refused_by_the_two() {
    let mut s = blank_session();
    let a = instance(&mut s, (1.0, 1.0, 3.0, 3.0));
    let b = instance(&mut s, (2.0, 2.0, 4.0, 4.0));
    let far = instance(&mut s, (7.0, 1.0, 8.0, 2.0));
    let connector = id_of(&run(&mut s, "connector.add", json!({"from": {"id": far}, "to": {"id": b, "point": 0}})));
    let mut holds = rect(95, (0.0, 0.0, 1.0, 1.0)).build();
    holds.children = diacraft_doc::ShapeList::from_vec(vec![Arc::new(rect(96, (0.0, 0.0, 1.0, 1.0)).build())]);
    let nothing = Build::new(98, 1.0, 1.0).pin(5.0, 5.0).section(&[(0.0, 0.0)], false, true).build();
    put(&mut s, vec![holds, nothing]);
    let mut guide = Shape::new(90);
    guide.kind = ShapeType::Guide;
    let mut picture = rect(91, (0.0, 0.0, 1.0, 1.0)).build();
    picture.kind = ShapeType::Foreign;
    put(&mut s, vec![guide, picture, rect(92, (0.0, 0.0, 1.0, 1.0)).build(), Build::new(93, 1.0, 1.0).pin(5.0, 5.0).text("only text").build()]);
    edit_shape(&mut s, 92, |shape| {
        Arc::make_mut(&mut shape.sheet)
            .set_cell(&CellAddr::in_row("User", None, RowKey::name("msvStructureType"), "Value"), Cell::constant(Value::from_text("Container")))
            .unwrap();
    });
    let before = snapshot(&s);
    for (id, why) in [
        (90, "is a guide"),
        (91, "picture or an embedded object"),
        (connector, "is a connector"),
        (92, "is a container or a list"),
        (93, "no geometry"),
        (95, "is a group"),
        (98, "no geometry"),
    ] {
        for (command, params) in two(&[a, id]) {
            let error = error_of(&mut s, command, &params);
            assert!(error.to_string().contains(why) && error.to_string().contains(&format!("`{command}`")), "{command}, shape {id}: {error}");
            assert_unchanged(&s, &before, command);
        }
    }
}

#[test]
fn r12_shapes_that_do_not_share_one_container_are_refused_by_the_two() {
    let mut s =
        session_with(vec![rect(1, (1.0, 1.0, 3.0, 3.0)).build(), rect(2, (2.0, 2.0, 4.0, 4.0)).build(), rect(3, (6.0, 6.0, 7.0, 7.0)).build()]);
    run(&mut s, "shape.group", json!({"ids": [2, 3]}));
    let before = snapshot(&s);
    for (command, params) in two(&[1, 2]) {
        let error = error_of(&mut s, command, &params);
        assert!(error.to_string().contains("share one container"), "{command}: {error}");
        assert_unchanged(&s, &before, command);
    }
}

#[test]
fn r12_a_shape_on_a_locked_layer_is_refused_by_the_two() {
    let mut s = session_with(vec![
        rect(1, (1.0, 1.0, 3.0, 3.0)).cell("LayerMember", Cell::constant(Value::from_text("0"))).build(),
        rect(2, (2.0, 2.0, 4.0, 4.0)).build(),
    ]);
    s.edit("a locked layer", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let mut page_sheet = (*page.sheet).clone();
        page_sheet.set_cell(&CellAddr::in_row("Layer", None, RowKey::Ix(0), "Lock"), Cell::constant(Value::from_text("1"))).unwrap();
        page.sheet = Arc::new(page_sheet);
        Ok(())
    })
    .unwrap();
    let before = snapshot(&s);
    for (command, params) in two(&[1, 2]) {
        let error = error_of(&mut s, command, &params);
        assert!(error.to_string().contains("locked layer"), "{command}: {error}");
        assert_unchanged(&s, &before, command);
    }
}

#[test]
fn r13_a_shape_that_cannot_be_deleted_is_not_refused_by_offset_or_fit_curve() {
    let locked = |id, b| rect(id, b).cell("LockDelete", Cell::constant(Value::from_text("1")));
    let mut s = session_with(vec![locked(1, (1.0, 1.0, 3.0, 3.0)).build(), rect(2, (2.0, 2.0, 4.0, 4.0)).build()]);
    // Offset keeps the originals and Fit Curve changes them in place: neither deletes anything.
    run(&mut s, "shape.offset", json!({"ids": [1, 2], "distance": 0.25}));
    assert!(has_id(doc(&s), 1) && has_id(doc(&s), 2));
    run(&mut s, "shape.fit_curve", json!({"ids": [1, 2], "tolerance": 0.05}));
    assert!(has_id(doc(&s), 1) && has_id(doc(&s), 2));
}

#[test]
fn check_says_what_offset_and_fit_curve_would_refuse_without_running_them() {
    let s = session_with(vec![
        rect(1, (1.0, 1.0, 3.0, 3.0)).build(),
        rect(2, (2.0, 2.0, 4.0, 4.0)).cell("LockVtxEdit", Cell::constant(Value::from_text("1"))).build(),
        line(3, (0.0, 0.0), (4.0, 4.0)).build(),
        Build::new(4, 1.0, 1.0).pin(5.0, 5.0).text("only text").build(),
        rect(5, (6.0, 6.0, 7.0, 7.0)).cell("LockDelete", Cell::constant(Value::from_text("1"))).build(),
    ]);
    let d = doc(&s);
    let before = d.clone();
    assert_eq!(check_shape_ops(d, 1, Op::Offset, &[3]), Ok(()));
    assert_eq!(check_shape_ops(d, 1, Op::Offset, &[1, 3]), Ok(()), "a line and a square");
    assert_eq!(check_shape_ops(d, 1, Op::FitCurve, &[1, 3]), Ok(()));
    assert!(check_shape_ops(d, 1, Op::FitCurve, &[2]).unwrap_err().contains("LockVtxEdit"), "Fit Curve edits the vertices");
    assert_eq!(check_shape_ops(d, 1, Op::Offset, &[2]), Ok(()), "Offset does not");
    assert_eq!(check_shape_ops(d, 1, Op::Offset, &[5]), Ok(()));
    assert_eq!(check_shape_ops(d, 1, Op::FitCurve, &[5]), Ok(()));
    for op in [Op::Offset, Op::FitCurve] {
        assert!(check_shape_ops(d, 1, op, &[1, 4]).unwrap_err().contains("no geometry"), "{}", op.id());
        assert!(check_shape_ops(d, 1, op, &[1, 99]).unwrap_err().contains("no shape 99"), "{}", op.id());
        assert!(check_shape_ops(d, 7, op, &[1, 3]).unwrap_err().contains("no page 7"), "{}", op.id());
        assert!(check_shape_ops(d, 1, op, &[]).is_err(), "{}", op.id());
    }
    assert_eq!(d, &before, "nothing is changed");
    assert_eq!((Op::Offset.id(), Op::FitCurve.id()), ("shape.offset", "shape.fit_curve"));
    assert_eq!((Op::Offset.fewest(), Op::FitCurve.fewest()), (1, 1));
    assert_eq!((Op::Offset.name(), Op::FitCurve.name()), ("Offset", "Fit Curve"));
}

// ---- E8: a master instance ------------------------------------------------------------------------------------------------------

#[test]
fn e8_what_offset_makes_of_an_instance_has_no_master_and_draws_what_it_drew() {
    let mut s = blank_session();
    let a = instance(&mut s, (1.0, 1.0, 3.0, 3.0));
    run(&mut s, "shape.format", json!({"ids": [a], "line": "#CC0000", "weight": 3.0}));
    let made = ids(&run(&mut s, "shape.offset", json!({"ids": [a], "distance": 0.25})), "ids");
    assert_eq!(made.len(), 2);
    let d = doc(&s);
    for id in &made {
        let shape = d.shape(&at(d, *id)).unwrap();
        assert!(shape.master.is_none() && !shape.is_instance(), "no master");
        assert_eq!(shape.kind, ShapeType::Shape);
        assert!(!built(d, *id).is_empty(), "it draws");
    }
    assert!(d.shape(&at(d, a)).unwrap().is_instance(), "the original is still an instance");
}

#[test]
fn r27_fit_curve_of_an_instance_marks_the_masters_section_deleted_and_the_new_one_is_local_and_it_saves_and_reopens_drawing_the_same() {
    let mut s = blank_session();
    // The Rectangle master: five rows of lines, which a tolerance of half its size makes two cubics (the exact `Ellipse` row of the ellipse master is not made smaller by
    // any fit, and is left as it is: see `fit_curve_leaves_an_ellipse_row_and_an_arc_row_as_they_are`).
    let id = instance(&mut s, (4.5, 3.5, 5.5, 4.5));
    let old: Vec<_> = built(doc(&s), id).iter().map(|(_, stroke, _)| stroke.clone()).collect();
    let result = run(&mut s, "shape.fit_curve", json!({"ids": [id], "tolerance": 0.5}));
    assert_eq!(ids(&result, "changed"), [id]);
    let d = doc(&s);
    let shape = d.shape(&at(d, id)).unwrap();
    assert!(shape.is_instance(), "it is still an instance");
    let deleted: Vec<u32> = shape.sheet.sections.iter().filter(|sec| sec.name.as_str() == "Geometry" && sec.del).filter_map(|sec| sec.ix).collect();
    let local: Vec<u32> = shape.sheet.sections.iter().filter(|sec| sec.name.as_str() == "Geometry" && !sec.del).filter_map(|sec| sec.ix).collect();
    assert!(
        !deleted.is_empty() && !local.is_empty(),
        "the master's sections are deleted on the instance, the new ones are its own: {deleted:?} {local:?}"
    );
    assert!(local.iter().all(|ix| !deleted.contains(ix)), "a new section has an `IX` of its own");
    assert_eq!(built(d, id).len(), local.len());
    let _ = old;
    let dir = Dir::new();
    let path = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut reopened = Session::new();
    run(&mut reopened, "file.open", json!({"path": path}));
    let (before, after) = (built(doc(&s), id), built(doc(&reopened), id));
    assert_eq!(before.len(), after.len());
    for ((_, a, _), (_, b, _)) in before.iter().zip(&after) {
        assert_eq!(a.elements().len(), b.elements().len());
    }
    assert_near("Width", num(doc(&reopened), id, "Width"), num(doc(&s), id, "Width"));
}

// ---- E6: inside a group ---------------------------------------------------------------------------------------------------------

#[test]
fn e6_offset_inside_a_group_gives_copies_that_are_the_groups_last_members_and_the_originals_stay() {
    let mut s =
        session_with(vec![line(1, (0.0, 0.0), (2.0, 0.0)).build(), line(2, (2.0, 0.0), (2.0, 2.0)).build(), rect(3, (5.0, 5.0, 6.0, 6.0)).build()]);
    let group = id_of(&run(&mut s, "shape.group", json!({"ids": [1, 2, 3]})));
    run(&mut s, "shape.rotate", json!({"ids": [group], "angle": 30}));
    let copies = ids(&run(&mut s, "shape.offset", json!({"ids": [1], "distance": 0.25})), "ids");
    assert_eq!(copies.len(), 2);
    let d = doc(&s);
    let members: Vec<u32> = d.shape(&at(d, group)).unwrap().children.iter().map(|m| m.id).collect();
    assert_eq!(members[..3], [1, 2, 3], "the originals stay where they were");
    assert_eq!(members[3..], copies[..]);
    assert_eq!(d.shape(&at(d, group)).unwrap().kind, ShapeType::Group);
    assert_near("unturned in its group's frame", num(d, copies[0], "Angle"), 0.0);
    assert_eq!(top(d), [group]);
}

// ---- R14: glue ------------------------------------------------------------------------------------------------------------------

fn connects_to(d: &Document, target: u32) -> usize {
    d.page(1).unwrap().connects.iter().filter(|c| c.to_sheet == target).count()
}

#[test]
fn r14_a_connector_glued_to_an_original_that_offset_keeps_stays_glued_and_one_glued_to_a_fitted_shape_follows_its_point() {
    let mut s = blank_session();
    let (a, d) = (instance(&mut s, (1.0, 1.0, 3.0, 3.0)), instance(&mut s, (7.0, 1.0, 8.0, 2.0)));
    let connector = id_of(&run(&mut s, "connector.add", json!({"from": {"id": d}, "to": {"id": a, "point": 0}})));
    let before = snapshot(&s);
    run(&mut s, "shape.offset", json!({"ids": [a], "distance": 0.5}));
    assert_eq!(connects_to(doc(&s), a), 1, "the original stays and so does the glue");
    assert_eq!(
        (num(doc(&s), connector, "EndX"), num(doc(&s), connector, "EndY")),
        (num(before.document.as_ref().unwrap(), connector, "EndX"), num(before.document.as_ref().unwrap(), connector, "EndY"))
    );
    // Fit Curve of the rectangle instance a connector is glued to: the end follows the connection point where the refit box puts it.
    let mut s = blank_session();
    let e = instance(&mut s, (4.5, 3.5, 5.5, 4.5));
    let d = instance(&mut s, (9.0, 1.0, 10.0, 2.0));
    let connector = id_of(&run(&mut s, "connector.add", json!({"from": {"id": d}, "to": {"id": e, "point": 0}})));
    let result = run(&mut s, "shape.fit_curve", json!({"ids": [e], "tolerance": 0.5}));
    assert_eq!(ids(&result, "changed"), [e]);
    assert_eq!(connects_to(doc(&s), e), 1, "still glued");
    let (px, py, w, h) = (num(doc(&s), e, "PinX"), num(doc(&s), e, "PinY"), num(doc(&s), e, "Width"), num(doc(&s), e, "Height"));
    // The end is on the shape's box, as its connection point is (a point of the box's edge).
    let (ex, ey) = (num(doc(&s), connector, "EndX"), num(doc(&s), connector, "EndY"));
    assert!(
        ex >= px - w / 2.0 - 1e-6 && ex <= px + w / 2.0 + 1e-6 && ey >= py - h / 2.0 - 1e-6 && ey <= py + h / 2.0 + 1e-6,
        "({ex}, {ey}) in the box"
    );
}

fn tie(code: u32, targets: &[u32]) -> String {
    format!("DEPENDSON({code},{})", targets.iter().map(|id| format!("Sheet.{id}!SheetRef()")).collect::<Vec<_>>().join(","))
}

/// The shape's `Relationships` formula without its blanks (the formula is printed with a blank after each comma).
fn relationships(d: &Document, id: u32) -> Option<String> {
    d.shape(&at(d, id)).unwrap().sheet.cell("Relationships").and_then(|c| c.formula.text().map(|t| t.replace(' ', "")))
}
#[test]
fn r14_offset_and_fit_curve_leave_the_places_of_a_shape_in_containers_and_lists() {
    let make = || {
        let mut s = session_with(vec![line(1, (0.0, 0.0), (2.0, 0.0)).build(), rect(3, (10.0, 10.0, 12.0, 12.0)).build()]);
        edit_shape(&mut s, 1, |shape| {
            Arc::make_mut(&mut shape.sheet)
                .cells
                .insert(CellName::new("Relationships"), Cell::with_formula(&format!("SUM({})", tie(4, &[3])), Some(Value::from_text("0"))));
        });
        edit_shape(&mut s, 3, |shape| {
            let sheet = Arc::make_mut(&mut shape.sheet);
            sheet
                .set_cell(&CellAddr::in_row("User", None, RowKey::name("msvStructureType"), "Value"), Cell::constant(Value::from_text("Container")))
                .unwrap();
            sheet.cells.insert(CellName::new("Relationships"), Cell::with_formula(&format!("SUM({})", tie(1, &[1])), Some(Value::from_text("0"))));
        });
        s
    };
    // Offset keeps the originals where they are, in their container, and its copies are in none.
    let mut s = make();
    let copies = ids(&run(&mut s, "shape.offset", json!({"ids": [1], "distance": 0.25})), "ids");
    assert!(copies.iter().all(|c| relationships(doc(&s), *c).is_none()));
    assert_eq!(relationships(doc(&s), 3), Some(format!("SUM({})", tie(1, &[1]))), "the container still holds the original");
    assert_eq!(relationships(doc(&s), 1), Some(format!("SUM({})", tie(4, &[3]))));
    // Fit Curve changes the shape in place: its places are as they were.
    let mut s = make();
    run(&mut s, "shape.fit_curve", json!({"ids": [1], "tolerance": 0.05}));
    assert_eq!(relationships(doc(&s), 3), Some(format!("SUM({})", tie(1, &[1]))));
}

// ---- R15: undo and redo ---------------------------------------------------------------------------------------------------------

#[test]
fn e6_fit_curve_of_a_member_of_a_group_changes_it_in_its_own_frame() {
    let mut s = session_with(vec![
        Build::new(1, 2.0, 2.0)
            .pin(4.0, 4.0)
            .section(
                &(0..36)
                    .map(|i| (1.0 + (f64::from(i) * 10f64.to_radians()).cos(), 1.0 + (f64::from(i) * 10f64.to_radians()).sin()))
                    .collect::<Vec<_>>(),
                true,
                false,
            )
            .build(),
        rect(2, (8.0, 8.0, 9.0, 9.0)).build(),
    ]);
    let group = id_of(&run(&mut s, "shape.group", json!({"ids": [1, 2]})));
    run(&mut s, "shape.rotate", json!({"ids": [group], "angle": 30}));
    let pose_before = (num(doc(&s), 1, "PinX"), num(doc(&s), 1, "PinY"));
    let result = run(&mut s, "shape.fit_curve", json!({"ids": [1], "tolerance": 0.02, "flags": 2}));
    assert_eq!(ids(&result, "changed"), [1]);
    let d = doc(&s);
    assert!(d.shape(&at(d, 1)).unwrap().sheet.sections.iter().any(|sec| sec.rows.iter().any(|r| r.row_type.as_deref() == Some("ArcTo"))));
    assert_near("the member's pin in its group's frame", num(d, 1, "PinX"), pose_before.0);
    assert_near("the member's pin in its group's frame", num(d, 1, "PinY"), pose_before.1);
    assert_eq!(top(d), [group]);
}

#[test]
fn r15_undo_and_redo_of_offset_and_fit_curve_are_one_step_with_the_same_ids_and_the_label_is_the_commands() {
    for (command, params, label, shapes) in [
        (
            "shape.offset",
            json!({"ids": [1, 2], "distance": 0.25}),
            "offset",
            vec![rect(1, (1.0, 1.0, 3.0, 3.0)).build(), rect(2, (2.0, 2.0, 4.0, 4.0)).build()],
        ),
        ("shape.fit_curve", json!({"ids": [1], "tolerance": 0.02, "flags": 2}), "fit curve", vec![circle_polygon(1)]),
    ] {
        let mut s = session_with(shapes);
        let before = snapshot(&s);
        let result = run(&mut s, command, params);
        let after = snapshot(&s);
        assert_eq!(after.undo_steps, before.undo_steps + 1, "{command}: one step");
        assert_eq!(after.next_undo.as_deref().map(str::to_lowercase), Some(label.to_owned()), "{command}: named for the command");
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(doc(&s), before.document.as_ref().unwrap(), "{command}: undo");
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(doc(&s), after.document.as_ref().unwrap(), "{command}: redo gives the same document: {result}");
    }
}

#[test]
fn r15_a_refused_command_adds_no_step() {
    let mut s = session_with(vec![rect(1, (1.0, 1.0, 3.0, 3.0)).build()]);
    let before = snapshot(&s);
    for (command, params) in [
        ("shape.offset", json!({"ids": [1], "distance": -1})),
        ("shape.offset", json!({"ids": [1], "distance": 0})),
        ("shape.fit_curve", json!({"ids": [1], "tolerance": -1})),
        ("shape.fit_curve", json!({"ids": [7], "tolerance": 1})),
    ] {
        error_of(&mut s, command, &params);
        assert_unchanged(&s, &before, command);
    }
}

// ---- the file -------------------------------------------------------------------------------------------------------------------

fn circle_polygon(id: u32) -> Shape {
    let points: Vec<(f64, f64)> =
        (0..36).map(|i| (1.0 + (f64::from(i) * 10f64.to_radians()).cos(), 1.0 + (f64::from(i) * 10f64.to_radians()).sin())).collect();
    Build::new(id, 2.0, 2.0).pin(6.0, 6.0).section(&points, true, false).build()
}

#[test]
fn the_produced_and_changed_shapes_save_to_vsdx_and_reopen_with_the_cells_equal() {
    for (command, params, shapes) in [
        (
            "shape.offset",
            json!({"ids": [1, 2], "distance": 0.25}),
            vec![rect(1, (0.0, 0.0, 4.0, 2.0)).text("A").build(), line(2, (6.0, 0.0), (8.0, 2.0)).build()],
        ),
        ("shape.fit_curve", json!({"ids": [1], "tolerance": 0.02, "flags": 2}), vec![circle_polygon(1)]),
    ] {
        let mut s = session_with(shapes);
        let result = run(&mut s, command, params);
        let mut changed: Vec<u32> = ids(&result, "ids");
        changed.extend(ids(&result, "changed"));
        assert!(!changed.is_empty(), "{command}");
        let dir = Dir::new();
        let path = dir.path("out.vsdx");
        run(&mut s, "file.save_as", json!({"path": path}));
        let mut reopened = Session::new();
        run(&mut reopened, "file.open", json!({"path": path}));
        for id in changed {
            assert_eq!(cells_of(doc(&s), id), cells_of(doc(&reopened), id), "{command}: shape {id}");
            assert_eq!(built(doc(&s), id).len(), built(doc(&reopened), id).len());
        }
        assert_eq!(top(doc(&s)), top(doc(&reopened)), "{command}: the drawing order");
    }
}

// ---- the pixels -----------------------------------------------------------------------------------------------------------------

const PX_PER_INCH: f64 = 100.0;
const PAGE_HEIGHT: f64 = 8.5;

fn render(s: &Session) -> Rendered {
    let d = doc(s);
    Renderer::new().render(d, 1, &View::fit_page(d, 1, PX_PER_INCH).unwrap())
}

fn px(r: &Rendered, x: f64, y: f64) -> [u8; 4] {
    let (width, height) = (r.width as usize, r.height as usize);
    let col = ((x * PX_PER_INCH) as usize).min(width - 1);
    let row = (((PAGE_HEIGHT - y) * PX_PER_INCH) as usize).min(height - 1);
    let at = (row * width + col) * 4;
    [r.rgba_premul[at], r.rgba_premul[at + 1], r.rgba_premul[at + 2], r.rgba_premul[at + 3]]
}

fn red(p: [u8; 4]) -> bool {
    p[0] > 200 && p[1] < 80 && p[2] < 80
}

/// Whether some pixel within two of (`x`, `y`) is red: a line of 3 points is four pixels wide.
fn red_near(r: &Rendered, x: f64, y: f64) -> bool {
    (-2..=2).any(|dx| (-2..=2).any(|dy| red(px(r, x + f64::from(dx) / PX_PER_INCH, y + f64::from(dy) / PX_PER_INCH))))
}

fn red_lines(s: &mut Session, ids: &[u32]) {
    run(s, "shape.format", json!({"ids": ids, "line": "#FF0000", "weight": 3.0, "fill": "none"}));
}

#[test]
fn the_render_check_offset_draws_the_originals_and_the_copies() {
    // Offset of a line: the original in the middle and a copy a distance away on each side, the same red; nothing between.
    let mut s = session_with(vec![line(1, (1.0, 2.0), (5.0, 2.0)).build()]);
    red_lines(&mut s, &[1]);
    run(&mut s, "shape.offset", json!({"ids": [1], "distance": 0.5}));
    let r = render(&s);
    for y in [1.5, 2.0, 2.5] {
        assert!(red_near(&r, 3.0, y), "a line at y {y}");
    }
    assert!(!red_near(&r, 3.0, 1.75) && !red_near(&r, 3.0, 2.25), "paper between them");
    assert!(!red_near(&r, 3.0, 1.0) && !red_near(&r, 3.0, 3.0), "and beyond");
}

#[test]
fn the_render_check_offset_of_a_filled_square_draws_the_outer_and_inner_outlines_and_no_fill() {
    let mut s = session_with(vec![rect(1, (2.0, 2.0, 4.0, 4.0)).build()]);
    run(&mut s, "shape.format", json!({"ids": [1], "line": "#FF0000", "weight": 3.0, "fill": "#00FF00"}));
    run(&mut s, "shape.offset", json!({"ids": [1], "distance": 0.5}));
    let r = render(&s);
    assert!(red_near(&r, 1.5, 3.0) && red_near(&r, 4.5, 3.0), "the outer square's sides");
    assert!(red_near(&r, 2.5, 3.0) && red_near(&r, 3.5, 3.0), "the inner square's sides");
    // The copies have no fill: between the outer and the original is paper (the original keeps its fill and is drawn under them).
    let between = px(&r, 1.75, 3.0);
    assert!(between[1] < 200 || between[0] > 200 || between[3] == 0 || (between[0] > 240 && between[1] > 240 && between[2] > 240), "{between:?}");
    assert!(!(between[1] > 200 && between[0] < 100), "the copies are not filled green: {between:?}");
}

#[test]
fn the_render_check_fit_curve_draws_the_circle_as_arcs_where_the_polygon_was() {
    let mut s = session_with(vec![circle_polygon(1)]);
    red_lines(&mut s, &[1]);
    let before = render(&s);
    run(&mut s, "shape.fit_curve", json!({"ids": [1], "tolerance": 0.02, "flags": 2}));
    let after = render(&s);
    for (x, y) in [(7.0, 6.0), (5.0, 6.0), (6.0, 7.0), (6.0, 5.0), (6.0 + std::f64::consts::FRAC_1_SQRT_2, 6.0 + std::f64::consts::FRAC_1_SQRT_2)] {
        assert!(red_near(&before, x, y) && red_near(&after, x, y), "({x}, {y}) is on the circle before and after");
    }
    assert!(!red_near(&after, 6.0, 6.0), "the centre is paper");
    assert_eq!(section_count(doc(&s), 1), 1, "the one section, with new rows");
    assert_near("it keeps its flags: closed, filled", flag_of(doc(&s), 1, 0, "NoFill"), 0.0);
}
// ---- the engine's own calls -----------------------------------------------------------------------------------------------------

#[test]
fn the_engines_call_with_no_arguments_runs_the_commands_that_take_none_and_refuses_what_needs_one() {
    use diacraft_engine::draw::{ShapeOpsArgs, shape_ops, shape_ops_with};
    let s = session_with(vec![rect(1, (0.0, 0.0, 4.0, 2.0)).build(), rect(2, (2.0, 1.0, 6.0, 3.0)).build()]);
    let mut d = doc(&s).clone();
    let made = shape_ops(&mut d, 1, Op::Union, &[1, 2], 10).unwrap();
    assert_eq!((made.produced.len(), made.deleted.clone()), (1, vec![1, 2]));
    assert!(made.produced[0] > 10, "the new id is above the one asked for");
    assert!(made.kept.is_none() && made.changed.is_empty());
    // Offset needs a distance, which no argument is: refused, as a bad parameter. Fit Curve's tolerance of nothing changes nothing.
    let mut d = doc(&s).clone();
    assert!(shape_ops(&mut d, 1, Op::Offset, &[1], 10).unwrap_err().bad_params);
    let made = shape_ops(&mut d, 1, Op::FitCurve, &[1], 10).unwrap();
    assert!(made.changed.is_empty() && made.produced.is_empty());
    assert_eq!(&d, doc(&s), "the document is as it was");
    // With the arguments given, Offset makes its sides and keeps the shapes.
    let made = shape_ops_with(&mut d, 1, Op::Offset, &[1], 10, &ShapeOpsArgs { distance: 0.5, ..ShapeOpsArgs::default() }).unwrap();
    assert_eq!((made.produced.len(), made.deleted.len()), (2, 0));
    // And the page must be there.
    assert!(shape_ops(&mut d, 9, Op::Union, &[1, 2], 10).is_err());
}
