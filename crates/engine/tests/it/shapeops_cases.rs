//! Shape operations, the rules beyond the geometric vectors: text and formatting from the first shape, glue and undo, drawing order
//! A group, refusals, a turned operand, every row type carried by Combine, a master instance, the file and the pixels.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_doc::{
    Cell, CellAddr, CellName, Document, Owner, Resolver, Row, RowKey, SectionKey, Shape, ShapeRef, ShapeType, SheetRef, Style, Value,
};
use diacraft_engine::Session;
use diacraft_render::{Rendered, Renderer, View};
use kurbo::{Affine, BezPath, ParamCurve, ParamCurveNearest, Point, Vec2};
use serde_json::{Value as Json, json};

use crate::shapeops::{
    Build, area, assert_near, assert_points, assert_unchanged, at, built, doc, error_of, fill_ring, filled, ids, line, local, local_points, made,
    near, num, page_points, pose_map, put, rect, same_points, section_count, session_with, top,
};
use crate::{blank_session, run, snapshot};

fn id_of(result: &Json) -> u32 {
    u32::try_from(result["id"].as_u64().unwrap()).unwrap()
}

/// An instance of the Rectangle master with the box (`x0`, `y0`) to (`x1`, `y1`).
fn instance(s: &mut Session, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> u32 {
    let id = id_of(&run(s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": (x0 + x1) / 2.0, "y": (y0 + y1) / 2.0})));
    run(s, "shape.size", json!({"ids": [id], "width": x1 - x0, "height": y1 - y0}));
    id
}

/// Changes the shape with this `ID` on page 1 (anywhere in the tree).
fn edit_shape(s: &mut Session, id: u32, f: impl FnOnce(&mut Shape)) {
    s.edit("edit a shape", |doc| {
        let found = doc.find_shape(Owner::Page(1), id).expect("the shape");
        let page = Arc::make_mut(doc.pages.get_mut(0).expect("a page"));
        let (first, rest) = found.path.split_first().expect("a path");
        let mut shape = Arc::make_mut(page.shapes.get_mut(usize::try_from(*first).unwrap()).expect("a shape"));
        for step in rest {
            shape = Arc::make_mut(shape.children.get_mut(usize::try_from(*step).unwrap()).expect("a member"));
        }
        f(shape);
        Ok(())
    })
    .unwrap();
}

fn formatting(s: &mut Session, id: u32) -> Json {
    run(s, "shape.formatting", json!({"ids": [id]}))["shapes"][0].clone()
}

fn text_of(d: &Document, id: u32) -> Option<String> {
    d.shape(&at(d, id))
        .unwrap()
        .text
        .as_ref()
        .map(|t| t.parts.iter().filter_map(|p| if let diacraft_doc::TextPart::Chars(c) = p { Some(c.to_string()) } else { None }).collect::<String>())
}

fn is_bold(s: &mut Session, id: u32) -> bool {
    formatting(s, id)["bold"] == json!(true)
}

// ---- V1: text and formatting come from the first shape --------------------------------------------------------------------------

/// A (1, 1)-(3, 3), red, linked to the style "Dashed", text "A" in bold, a Shape Data row `Cost` of 5, on layer 0, an instance of a master; and B (2, 2)-(4, 4), blue,
/// text "B". The ids are 1 and 2.
fn session_1() -> Session {
    let mut s = blank_session();
    s.edit("a style", |doc| {
        // The style sets a line pattern, and a line colour by a formula that a shape's own cell inherits (as the cells of a theme are held).
        let mut sheet = diacraft_doc::Sheet::default();
        sheet.cells.insert(CellName::new("LinePattern"), Cell::constant(Value::from_text("2")));
        sheet.cells.insert(CellName::new("LineColor"), Cell::with_formula("RGB(10,20,30)", Some(Value::from_text("#0a141e"))));
        doc.add_style(Style { id: 7, name: Some(Arc::from("Dashed")), sheet: Arc::new(sheet), ..Style::default() }).unwrap();
        Ok(())
    })
    .unwrap();
    let a = instance(&mut s, (1.0, 1.0, 3.0, 3.0));
    let b = instance(&mut s, (2.0, 2.0, 4.0, 4.0));
    run(&mut s, "shape.format", json!({"ids": [a], "fill": "#FF0000"}));
    run(&mut s, "shape.format", json!({"ids": [b], "fill": "#0000FF"}));
    run(&mut s, "shape.set_text", json!({"id": a, "text": "A"}));
    run(&mut s, "shape.set_text", json!({"id": b, "text": "B"}));
    run(&mut s, "text.format", json!({"ids": [a], "bold": true}));
    edit_shape(&mut s, a, |shape| {
        let sheet = Arc::make_mut(&mut shape.sheet);
        sheet.links.line = Some(7);
        sheet.set_cell(&CellAddr::in_row("Property", None, RowKey::name("Cost"), "Value"), Cell::constant(Value::from_text("5"))).unwrap();
        sheet.cells.insert(CellName::new("LayerMember"), Cell::constant(Value::from_text("0")));
        sheet.cells.insert(CellName::new("LineColor"), Cell::inherited(Some(Value::from_text("#0a141e"))));
    });
    s
}

#[test]
fn the_produced_shape_takes_its_text_and_formatting_from_the_first_shape_and_nothing_else() {
    let mut s = session_1();
    let a_fill = formatting(&mut s, 1)["fill"].clone();
    assert_eq!(a_fill, json!("#ff0000"));
    assert!(doc(&s).shape(&at(doc(&s), 1)).unwrap().is_instance(), "A is an instance of a master");
    let id = made(&run(&mut s, "shape.union", json!({"ids": [1, 2]})));
    assert_eq!(formatting(&mut s, id)["fill"], a_fill, "red");
    assert!(is_bold(&mut s, id), "in bold");
    let d = doc(&s);
    let shape = d.shape(&at(d, id)).unwrap();
    assert_eq!(shape.sheet.links.line, Some(7), "the link to Dashed");
    assert_eq!(text_of(d, id).as_deref(), Some("A"));
    assert!(shape.sheet.section(&SectionKey::new("Property", None)).is_none(), "no Shape Data: `Cost` is not carried");
    assert_eq!(local(d, id, "LayerMember").and_then(|c| c.1).as_deref(), Some("0"), "the layer");
    // What the style supplies is read through the link and not copied; a cell that inherits the style's formula goes on inheriting it.
    assert!(local(d, id, "LinePattern").is_none() && near(num(d, id, "LinePattern"), 2.0), "the style's line pattern is read through the link");
    assert_eq!(local(d, id, "LineColor"), Some((Some("Inh".into()), Some("#0a141e".into()))), "the inherited formula stays inherited");
    assert!(shape.master.is_none() && !shape.is_instance(), "no master");
    assert_eq!(shape.kind, ShapeType::Shape, "an ordinary shape, not a group or a guide");
    assert!(shape.name.is_none() && shape.name_u.is_none(), "no name, so the default one");
    // The other way round gives blue and "B".
    let mut s = session_1();
    let id = made(&run(&mut s, "shape.union", json!({"ids": [2, 1]})));
    assert_eq!(formatting(&mut s, id)["fill"], json!("#0000ff"));
    assert_eq!(text_of(doc(&s), id).as_deref(), Some("B"));
    assert!(!is_bold(&mut s, id));
}

#[test]
fn an_instance_that_shows_its_masters_text_gives_that_text_to_the_produced_shape() {
    let mut s = blank_session();
    let a = instance(&mut s, (1.0, 1.0, 3.0, 3.0));
    let b = instance(&mut s, (2.0, 2.0, 4.0, 4.0));
    s.edit("text on the master", |doc| {
        let master = doc.find_shape(Owner::Page(1), a).and_then(|at| doc.shape(&at).and_then(|shape| shape.master)).expect("A is an instance");
        let index = doc.masters.iter().position(|m| m.id == master).expect("the master");
        let master = Arc::make_mut(doc.masters.get_mut(index).unwrap());
        let shape = Arc::make_mut(master.shapes.get_mut(0).unwrap());
        shape.text =
            Some(Arc::new(diacraft_doc::Text { parts: vec![diacraft_doc::TextPart::Chars(Arc::from("from the master"))], ..Default::default() }));
        Ok(())
    })
    .unwrap();
    assert!(doc(&s).shape(&at(doc(&s), a)).unwrap().text.is_none(), "A has no text of its own");
    let id = made(&run(&mut s, "shape.union", json!({"ids": [a, b]})));
    assert_eq!(text_of(doc(&s), id).as_deref(), Some("from the master"));
}

#[test]
fn when_the_first_shape_has_no_text_the_produced_shape_has_none() {
    let mut s = session_with(vec![rect(1, (1.0, 1.0, 3.0, 3.0)).build(), rect(2, (2.0, 2.0, 4.0, 4.0)).text("B").build()]);
    let id = made(&run(&mut s, "shape.union", json!({"ids": [1, 2]})));
    assert!(doc(&s).shape(&at(doc(&s), id)).unwrap().text.is_none());
}

#[test]
fn a_field_becomes_the_text_it_shows_and_a_formula_naming_something_missing_becomes_its_value() {
    let mut s = session_with(vec![rect(1, (1.0, 1.0, 3.0, 3.0)).build(), rect(2, (2.0, 2.0, 4.0, 4.0)).build()]);
    edit_shape(&mut s, 1, |shape| {
        shape.text = Some(Arc::new(diacraft_doc::Text {
            parts: vec![diacraft_doc::TextPart::Chars(Arc::from("Page ")), diacraft_doc::TextPart::Fld { ix: 0, text: Arc::from("3") }],
            ..Default::default()
        }));
        let sheet = Arc::make_mut(&mut shape.sheet);
        // A formula of the first shape that names its own Shape Data row, and one that names a cell the produced shape has.
        sheet.cells.insert(CellName::new("LineWeight"), Cell::with_formula("Prop.Cost", Some(Value::from_text("0.02"))).with_unit("IN"));
        sheet.cells.insert(CellName::new("LeftMargin"), Cell::with_formula("Width*0.1", Some(Value::from_text("0.2"))).with_unit("IN"));
    });
    let id = made(&run(&mut s, "shape.union", json!({"ids": [1, 2]})));
    let d = doc(&s);
    assert_eq!(text_of(d, id).as_deref(), Some("Page 3"), "the field is the text it showed");
    assert_eq!(local(d, id, "LineWeight"), Some((None, Some("0.02".into()))), "a formula that names Prop.Cost is its value");
    assert_eq!(local(d, id, "LeftMargin").unwrap().0.as_deref(), Some("Width*0.1"), "a formula of cells the shape has stays a formula");
}

// ----: drawing order -----------------------------------------------------------------------------------------------------

#[test]
fn the_produced_shape_is_at_the_front_so_what_was_between_the_originals_is_behind_it() {
    let mut s =
        session_with(vec![rect(1, (0.0, 0.0, 2.0, 2.0)).build(), rect(2, (5.0, 5.0, 6.0, 6.0)).build(), rect(3, (1.0, 1.0, 3.0, 3.0)).build()]);
    let id = made(&run(&mut s, "shape.union", json!({"ids": [1, 3]})));
    assert_eq!(top(doc(&s)), [2, id], "X then the produced shape");
    assert!(id > 3, "a new id, above every one on the page");
}

// ----: glue and undo -----------------------------------------------------------------------------------------------

#[test]
fn a_connector_glued_to_an_original_keeps_its_end_unglued_and_undo_brings_everything_back_in_one_step() {
    let mut s = blank_session();
    let (a, b, d) = (instance(&mut s, (1.0, 1.0, 3.0, 3.0)), instance(&mut s, (2.0, 2.0, 4.0, 4.0)), instance(&mut s, (7.0, 1.0, 8.0, 2.0)));
    let connector = id_of(&run(&mut s, "connector.add", json!({"from": {"id": d}, "to": {"id": a, "point": 0}})));
    let end_before = (num(doc(&s), connector, "EndX"), num(doc(&s), connector, "EndY"));
    let before = snapshot(&s);
    let connects_to = |d_: &Document, target: u32| d_.page(1).unwrap().connects.iter().filter(|c| c.to_sheet == target).count();
    assert_eq!(connects_to(doc(&s), a), 1);
    let produced = made(&run(&mut s, "shape.union", json!({"ids": [a, b]})));
    let after = doc(&s);
    assert_eq!(connects_to(after, a), 0, "no Connect row for the end that was glued to the original");
    assert_eq!(connects_to(after, produced), 0, "the connector is not glued to the produced shape");
    assert_eq!(connects_to(after, d), 1, "its end at D is still glued");
    assert!(has_id(after, connector), "the connector is not deleted");
    assert_near("EndX", num(after, connector, "EndX"), end_before.0);
    assert_near("EndY", num(after, connector, "EndY"), end_before.1);
    assert!(
        after.shape(&at(after, produced)).unwrap().sheet.section(&SectionKey::new("Connection", None)).is_none(),
        "the produced shape has no connection points"
    );
    // One undo step brings back both originals with their ids, and the connector is glued to A again.
    run(&mut s, "edit.undo", json!({}));
    let undone = doc(&s);
    assert_eq!(undone, before.document.as_ref().unwrap(), "the document is as it was");
    assert_eq!(connects_to(undone, a), 1);
    assert!(has_id(undone, a) && has_id(undone, b) && !has_id(undone, produced));
    assert_eq!((snapshot(&s).undo_steps, snapshot(&s).redo_steps), (before.undo_steps, 1), "one step, now to redo");
    // Redo does it again, to the same result.
    run(&mut s, "edit.redo", json!({}));
    assert!(has_id(doc(&s), produced) && !has_id(doc(&s), a));
}

fn has_id(d: &Document, id: u32) -> bool {
    d.find_shape(Owner::Page(1), id).is_some()
}

#[test]
fn undo_and_redo_of_each_operation_are_one_step_with_the_same_ids() {
    for (command, ids) in [
        ("shape.union", json!([1, 2])),
        ("shape.combine", json!([1, 2])),
        ("shape.fragment", json!([1, 2])),
        ("shape.intersect", json!([1, 2])),
        ("shape.subtract", json!([1, 2])),
    ] {
        let mut s = session_with(vec![rect(1, (1.0, 1.0, 3.0, 3.0)).build(), rect(2, (2.0, 2.0, 4.0, 4.0)).build()]);
        let before = snapshot(&s);
        let result = run(&mut s, command, json!({"ids": ids}));
        let after = snapshot(&s);
        assert_eq!(after.undo_steps, before.undo_steps + 1, "{command}: one step");
        let label = command.strip_prefix("shape.").unwrap();
        assert_eq!(after.next_undo.as_deref().map(str::to_lowercase), Some(label.to_owned()), "{command}: the step is named for the operation");
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(doc(&s), before.document.as_ref().unwrap(), "{command}: undo");
        assert_eq!(snapshot(&s).undo_steps, before.undo_steps);
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(doc(&s), after.document.as_ref().unwrap(), "{command}: redo gives the same document, the same ids: {result}");
    }
}

// ----: inside a group ----------------------------------------------------------------------------------------------------

#[test]
fn shapes_picked_inside_a_group_give_a_result_that_is_the_groups_last_member() {
    let mut s =
        session_with(vec![rect(1, (0.0, 0.0, 2.0, 2.0)).build(), rect(2, (1.0, 1.0, 3.0, 3.0)).build(), rect(3, (5.0, 5.0, 6.0, 6.0)).build()]);
    let group = id_of(&run(&mut s, "shape.group", json!({"ids": [1, 2, 3]})));
    let c_before = doc(&s).shape(&at(doc(&s), 3)).unwrap().clone();
    let result = run(&mut s, "shape.union", json!({"ids": [1, 2]}));
    let produced = made(&result);
    let d = doc(&s);
    let group_shape = d.shape(&at(d, group)).unwrap();
    assert_eq!(group_shape.kind, ShapeType::Group, "G is still a group");
    let members: Vec<u32> = group_shape.children.iter().map(|m| m.id).collect();
    assert_eq!(members, [3, produced], "C is unchanged and the produced shape is the last member");
    assert_eq!(d.shape(&at(d, 3)).unwrap(), &c_before);
    assert_eq!(top(d), [group]);
}

#[test]
fn a_result_inside_a_turned_group_draws_where_the_originals_did() {
    let mut s = session_with(vec![rect(1, (0.0, 0.0, 2.0, 2.0)).build(), rect(2, (1.0, 1.0, 3.0, 3.0)).build()]);
    let group = id_of(&run(&mut s, "shape.group", json!({"ids": [1, 2]})));
    run(&mut s, "shape.rotate", json!({"ids": [group], "angle": 30}));
    let before = doc(&s).clone();
    let (a_on_page, b_on_page) = (page_ring(&before, 1), page_ring(&before, 2));
    let produced = made(&run(&mut s, "shape.union", json!({"ids": [1, 2]})));
    let after = doc(&s);
    let result = page_ring(after, produced);
    // The union of what the originals drew, on the page.
    let expected =
        craft_pathops::try_unite_all(&[(&a_on_page, craft_pathops::FillRule::EvenOdd), (&b_on_page, craft_pathops::FillRule::EvenOdd)]).unwrap();
    assert_near(
        "area on the page",
        craft_pathops::area(&result, craft_pathops::FillRule::EvenOdd),
        craft_pathops::area(&expected, craft_pathops::FillRule::EvenOdd),
    );
    let probe = Point::new(1.5, 1.5);
    let moved = page_map(&before, 1) * probe;
    assert!(diacraft_geom::even_odd_contains(&result, moved), "a point inside both originals is inside the result");
    assert!(near(num(after, produced, "Angle"), 0.0), "the produced shape is unturned in its group's frame");
}

/// The shape's own frame to the page, through the poses of every group it is in.
fn page_map(d: &Document, id: u32) -> Affine {
    let found = at(d, id);
    (1..=found.path.len())
        .rev()
        .map(|k| {
            let prefix = ShapeRef { owner: found.owner, path: found.path[..k].to_vec() };
            pose_of(d, &prefix)
        })
        .fold(Affine::IDENTITY, |acc, map| map * acc)
}

fn pose_of(d: &Document, at: &ShapeRef) -> Affine {
    let res = Resolver::new(d);
    let n = |name: &str| {
        res.resolve(&SheetRef::Shape(at.clone()), &CellAddr::direct(name))
            .and_then(|f| f.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()))
            .unwrap_or(0.0)
    };
    let flip = Affine::scale_non_uniform(if n("FlipX") != 0.0 { -1.0 } else { 1.0 }, if n("FlipY") != 0.0 { -1.0 } else { 1.0 });
    Affine::translate(Vec2::new(n("PinX"), n("PinY")))
        * Affine::rotate(n("Angle"))
        * flip
        * Affine::translate(Vec2::new(-n("LocPinX"), -n("LocPinY")))
}

/// The ring the shape fills, on the page.
fn page_ring(d: &Document, id: u32) -> BezPath {
    let mut ring = fill_ring(d, id);
    // `fill_ring` carried it to the container; carry it on through the groups round the shape.
    let found = at(d, id);
    for k in (1..found.path.len()).rev() {
        let prefix = ShapeRef { owner: found.owner, path: found.path[..k].to_vec() };
        ring.apply_affine(pose_of(d, &prefix));
    }
    ring
}

// ---- V28: refusals --------------------------------------------------------------------------------------------------------------

#[test]
fn the_needs_of_each_command_are_refused_in_words_and_change_nothing() {
    let mut s = session_with(vec![
        rect(1, (1.0, 1.0, 3.0, 3.0)).build(),
        rect(2, (2.0, 2.0, 4.0, 4.0)).build(),
        rect(3, (6.0, 6.0, 7.0, 7.0)).build(),
        rect(4, (6.5, 6.5, 7.5, 7.5)).build(),
    ]);
    let group = id_of(&run(&mut s, "shape.group", json!({"ids": [3, 4]})));
    let before = snapshot(&s);
    for (command, params, why) in [
        ("shape.union", json!({"ids": [1]}), "two shapes at least"),
        ("shape.combine", json!({"ids": [1]}), "two shapes at least"),
        ("shape.intersect", json!({"ids": [1, 1]}), "two shapes at least"),
        ("shape.subtract", json!({"ids": [1]}), "two shapes at least"),
        ("shape.union", json!({"ids": [group, 1]}), "is a group"),
        ("shape.fragment", json!({"ids": [group]}), "is a group"),
        ("shape.union", json!({"ids": [3, 1], "page": 1}), "share one container"),
    ] {
        let error = error_of(&mut s, command, &params);
        assert!(error.to_string().contains(why), "{command} {params}: {error}");
        assert!(error.to_string().contains(&format!("`{command}`")), "the refusal names the command: {error}");
        assert_unchanged(&s, &before, command);
    }
}

#[test]
fn check_says_what_a_command_would_refuse_without_running_it() {
    use diacraft_engine::draw::Op;
    let s = session_with(vec![rect(1, (1.0, 1.0, 3.0, 3.0)).build(), rect(2, (2.0, 2.0, 4.0, 4.0)).build(), line(3, (0.0, 0.0), (4.0, 4.0)).build()]);
    let d = doc(&s);
    let before = d.clone();
    let check = diacraft_engine::draw::check_shape_ops;
    for op in [Op::Union, Op::Combine, Op::Fragment, Op::Intersect, Op::Subtract] {
        assert_eq!(check(d, 1, op, &[1, 2]), Ok(()), "{}", op.id());
    }
    assert!(check(d, 1, Op::Union, &[1]).unwrap_err().contains("Union needs two shapes at least"));
    assert!(check(d, 1, Op::Subtract, &[1]).unwrap_err().contains("Subtract needs"));
    assert_eq!(check(d, 1, Op::Fragment, &[1]), Ok(()), "Fragment takes one");
    assert!(check(d, 1, Op::Union, &[1, 3]).unwrap_err().contains("fill area"));
    assert_eq!(check(d, 1, Op::Combine, &[1, 3]), Ok(()), "Combine takes a line");
    assert!(check(d, 1, Op::Union, &[1, 99]).unwrap_err().contains("no shape 99"));
    assert!(check(d, 7, Op::Union, &[1, 2]).unwrap_err().contains("no page 7"));
    assert_eq!(d, &before, "nothing is changed");
    let names = [Op::Union.id(), Op::Combine.id(), Op::Fragment.id(), Op::Intersect.id(), Op::Subtract.id()];
    assert_eq!(names, ["shape.union", "shape.combine", "shape.fragment", "shape.intersect", "shape.subtract"]);
    assert_eq!((Op::Fragment.fewest(), Op::Union.fewest(), Op::Subtract.fewest()), (1, 2, 2));
}

#[test]
fn a_shape_that_cannot_be_deleted_refuses_every_command_that_would_delete_it_and_subtract_only_for_the_others() {
    let locked = |id, b| rect(id, b).cell("LockDelete", Cell::constant(Value::from_text("1")));
    let mut s = session_with(vec![locked(1, (1.0, 1.0, 3.0, 3.0)).build(), rect(2, (2.0, 2.0, 4.0, 4.0)).build()]);
    let before = snapshot(&s);
    for command in ["shape.union", "shape.combine", "shape.fragment", "shape.intersect"] {
        let error = error_of(&mut s, command, &json!({"ids": [1, 2]}));
        assert!(error.to_string().contains("LockDelete"), "{command}: {error}");
        assert_unchanged(&s, &before, command);
    }
    // Subtract with A first and LockDelete true on A goes ahead: A is not deleted.
    run(&mut s, "shape.subtract", json!({"ids": [1, 2]}));
    assert!(has_id(doc(&s), 1) && !has_id(doc(&s), 2));
    // With LockDelete true on B it is refused.
    let mut s = session_with(vec![rect(1, (1.0, 1.0, 3.0, 3.0)).build(), locked(2, (2.0, 2.0, 4.0, 4.0)).build()]);
    let before = snapshot(&s);
    error_of(&mut s, "shape.subtract", &json!({"ids": [1, 2]}));
    assert_unchanged(&s, &before, "subtract with the other locked");
}

#[test]
fn subtract_with_the_vertices_of_the_first_shape_locked_is_refused() {
    let mut s = session_with(vec![
        rect(1, (1.0, 1.0, 3.0, 3.0)).cell("LockVtxEdit", Cell::constant(Value::from_text("1"))).build(),
        rect(2, (2.0, 2.0, 4.0, 4.0)).build(),
    ]);
    let before = snapshot(&s);
    let error = error_of(&mut s, "shape.subtract", &json!({"ids": [1, 2]}));
    assert!(error.to_string().contains("LockVtxEdit"), "{error}");
    assert_unchanged(&s, &before, "LockVtxEdit");
    // `LockFormat` refuses nothing: the produced shape is new.
    let mut s = session_with(vec![
        rect(1, (1.0, 1.0, 3.0, 3.0)).cell("LockFormat", Cell::constant(Value::from_text("1"))).build(),
        rect(2, (2.0, 2.0, 4.0, 4.0)).build(),
    ]);
    run(&mut s, "shape.union", json!({"ids": [1, 2]}));
}

/// The vertices of a shape that is not the first are not edited by any command (Subtract deletes it, the others delete every shape), so its `LockVtxEdit` refuses nothing: the
/// check is of the first shape of Subtract alone.
#[test]
fn the_vertices_lock_of_a_shape_that_is_not_the_first_refuses_no_command() {
    for command in ["shape.union", "shape.combine", "shape.fragment", "shape.intersect", "shape.subtract"] {
        let mut s = session_with(vec![
            rect(1, (1.0, 1.0, 3.0, 3.0)).build(),
            rect(2, (2.0, 2.0, 4.0, 4.0)).cell("LockVtxEdit", Cell::constant(Value::from_text("1"))).build(),
        ]);
        run(&mut s, command, json!({"ids": [1, 2]}));
        assert!(!has_id(doc(&s), 2), "{command}: the locked shape went with the rest");
    }
}

#[test]
fn a_one_dimensional_shape_with_a_glued_end_is_a_connector_of_a_file_and_one_without_is_a_line() {
    // A connector as a file holds it: the four end cells and a `Connect` row, and none of DiaCraft's own marker of a route.
    let one_d = |id: u32| {
        let length = |n: &str| Cell::constant(Value::from_text(n)).with_unit("IN");
        Build::new(id, 2.0, 0.0)
            .pin(8.0, 5.0)
            .cell("BeginX", length("7"))
            .cell("BeginY", length("5"))
            .cell("EndX", length("9"))
            .cell("EndY", length("5"))
            .section(&[(0.0, 0.0), (2.0, 0.0)], false, true)
            .build()
    };
    let mut s = session_with(vec![rect(1, (1.0, 1.0, 3.0, 3.0)).build(), rect(2, (6.0, 4.0, 7.0, 6.0)).build(), one_d(3), one_d(4)]);
    s.edit("glue the begin of 3 to shape 2", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let connect = diacraft_doc::Connect {
            from_sheet: 3,
            from_cell: Some(Arc::from("BeginX")),
            from_part: Some(9),
            to_sheet: 2,
            to_cell: Some(Arc::from("Connections.X1")),
            to_part: Some(100),
            ..diacraft_doc::Connect::default()
        };
        page.connects = diacraft_doc::Connects::from_vec(vec![connect]);
        Ok(())
    })
    .unwrap();
    let before = snapshot(&s);
    let error = error_of(&mut s, "shape.combine", &json!({"ids": [1, 3]}));
    assert!(error.to_string().contains("shape 3 is a connector"), "{error}");
    assert_unchanged(&s, &before, "a connector of a file");
    // The same shape with no end glued is a line, and Combine takes it.
    run(&mut s, "shape.combine", json!({"ids": [1, 4]}));
}

#[test]
fn a_guide_a_picture_a_connector_a_container_a_text_only_shape_and_a_locked_layer_are_refused() {
    let mut s = blank_session();
    let a = instance(&mut s, (1.0, 1.0, 3.0, 3.0));
    let b = instance(&mut s, (2.0, 2.0, 4.0, 4.0));
    let far = instance(&mut s, (7.0, 1.0, 8.0, 2.0));
    let connector = id_of(&run(&mut s, "connector.add", json!({"from": {"id": far}, "to": {"id": b, "point": 0}})));
    // A connector of ours with both ends free is still one: the route it keeps says so.
    let free = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 9, "y": 6}, "to": {"x": 10, "y": 7}})));
    // A shape that holds members and is not typed a group, and a group with none, are groups for this.
    let mut holds = rect(95, (0.0, 0.0, 1.0, 1.0)).build();
    holds.children = diacraft_doc::ShapeList::from_vec(vec![Arc::new(rect(96, (0.0, 0.0, 1.0, 1.0)).build())]);
    let mut empty_group = rect(97, (0.0, 0.0, 1.0, 1.0)).build();
    empty_group.kind = ShapeType::Group;
    // A shape whose one section is a single move draws nothing.
    let nothing = Build::new(98, 1.0, 1.0).pin(5.0, 5.0).section(&[(0.0, 0.0)], false, true).build();
    put(&mut s, vec![holds, empty_group, nothing]);
    let mut guide = Shape::new(90);
    guide.kind = ShapeType::Guide;
    let mut picture = rect(91, (0.0, 0.0, 1.0, 1.0)).build();
    picture.kind = ShapeType::Foreign;
    let container = rect(92, (0.0, 0.0, 1.0, 1.0)).build();
    put(&mut s, vec![guide, picture, container]);
    edit_shape(&mut s, 92, |shape| {
        Arc::make_mut(&mut shape.sheet)
            .set_cell(&CellAddr::in_row("User", None, RowKey::name("msvStructureType"), "Value"), Cell::constant(Value::from_text("Container")))
            .unwrap();
    });
    put(&mut s, vec![Build::new(93, 1.0, 1.0).pin(5.0, 5.0).text("only text").build()]);
    let before = snapshot(&s);
    for (id, why) in [
        (90, "is a guide"),
        (91, "picture or an embedded object"),
        (connector, "is a connector"),
        (free, "is a connector"),
        (92, "is a container or a list"),
        (93, "no geometry"),
        (95, "is a group"),
        (97, "is a group"),
        (98, "no geometry"),
    ] {
        let error = error_of(&mut s, "shape.union", &json!({"ids": [a, id]}));
        assert!(error.to_string().contains(why), "shape {id}: {error}");
        assert_unchanged(&s, &before, "refused");
    }
    // A plain unglued line is accepted by Combine: Join's and Offset's pages are about drawn lines.
    put(&mut s, vec![line(94, (0.0, 0.0), (1.0, 1.0)).build()]);
    run(&mut s, "shape.combine", json!({"ids": [a, 94]}));
}

#[test]
fn a_shape_on_a_locked_layer_is_refused() {
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
    let error = error_of(&mut s, "shape.union", &json!({"ids": [1, 2]}));
    assert!(error.to_string().contains("locked layer"), "{error}");
    assert_unchanged(&s, &before, "locked layer");
    // The other shape is on no layer: a Union of it with itself needs two shapes, and it is fine with a shape on a layer that is not locked.
    let mut s = session_with(vec![
        rect(1, (1.0, 1.0, 3.0, 3.0)).cell("LayerMember", Cell::constant(Value::from_text("5"))).build(),
        rect(2, (2.0, 2.0, 4.0, 4.0)).build(),
    ]);
    run(&mut s, "shape.union", json!({"ids": [1, 2]}));
}

#[test]
fn union_intersect_and_subtract_need_a_fill_area_in_every_shape_and_combine_does_not() {
    let mut s = session_with(vec![rect(1, (1.0, 1.0, 3.0, 3.0)).build(), line(2, (0.0, 0.0), (4.0, 4.0)).build()]);
    let before = snapshot(&s);
    for command in ["shape.union", "shape.intersect", "shape.subtract"] {
        let error = error_of(&mut s, command, &json!({"ids": [1, 2]}));
        assert!(error.to_string().contains("fill area"), "{command}: {error}");
        assert_unchanged(&s, &before, command);
    }
    run(&mut s, "shape.combine", json!({"ids": [1, 2]}));
    // Fragment needs one shape with a fill area and takes lines for the others, and refuses lines alone.
    let mut s = session_with(vec![line(1, (0.0, 0.0), (1.0, 1.0)).build(), line(2, (0.0, 1.0), (1.0, 0.0)).build()]);
    let before = snapshot(&s);
    let error = error_of(&mut s, "shape.fragment", &json!({"ids": [1, 2]}));
    assert!(error.to_string().contains("fill area"), "{error}");
    assert_unchanged(&s, &before, "fragment of lines");
}

/// Whether a shape has a fill area is judged from its rings and not by a sweep, so a shape whose rings are thick and cancel under the odd-crossings rule (two equal squares one
/// over the other, as two sections) passes the check and has no area all the same: what each command makes of it is what the rule gives, a refusal in words with nothing
/// changed where nothing would be made, and in a Union the shape that does fill.
#[test]
fn a_shape_whose_sections_cancel_under_the_odd_crossings_rule_fills_nothing_and_a_command_says_so_by_its_result() {
    let square = [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)];
    let cancelling = |id| rect(id, (0.0, 0.0, 2.0, 2.0)).section(&square, true, false).build();
    let make = || session_with(vec![cancelling(1), cancelling(2), rect(3, (5.0, 5.0, 7.0, 7.0)).build()]);
    for (command, ids, why) in [
        ("shape.union", json!([1, 2]), "no area to unite"),
        ("shape.intersect", json!([1, 3]), "no area in common"),
        ("shape.subtract", json!([1, 3]), "nothing of shape 1 is left"),
        ("shape.fragment", json!([1, 2]), "no region"),
    ] {
        let mut s = make();
        let before = snapshot(&s);
        let error = error_of(&mut s, command, &json!({"ids": ids}));
        assert!(error.to_string().contains(why), "{command}: {error}");
        assert_unchanged(&s, &before, command);
    }
    // Union with a shape that fills is that shape's outline: the one that fills nothing adds nothing, and goes with the rest.
    let mut s = make();
    let id = made(&run(&mut s, "shape.union", json!({"ids": [1, 3]})));
    assert_near("the square's area", area(doc(&s), id), 4.0);
    assert_eq!(top(doc(&s)), [2, id]);
}

#[test]
fn fragment_cuts_with_an_open_path_that_takes_the_fill_too_and_a_closed_nofill_path_cuts_without_filling() {
    // The line is open with NoFill 0: a line there and back for the fill, which covers nothing, and a cutting line all the same.
    let open_filled = Build::new(2, 0.0, 4.0).pin(2.0, 1.0).section(&[(0.0, -1.0), (0.0, 3.0)], false, false).build();
    let mut s = session_with(vec![rect(1, (0.0, 0.0, 4.0, 2.0)).build(), open_filled]);
    let pieces = ids(&run(&mut s, "shape.fragment", json!({"ids": [1, 2]})), "ids");
    assert_eq!(pieces.len(), 2, "cut in two");
    // A closed square with NoFill 1 across the middle of a bigger one cuts a region out of it and fills nothing of its own: the inside and the frame round it.
    let hole = Build::new(2, 2.0, 2.0).pin(2.0, 2.0).section(&[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)], true, true).build();
    let mut s = session_with(vec![rect(1, (0.0, 0.0, 4.0, 4.0)).build(), hole]);
    let pieces = ids(&run(&mut s, "shape.fragment", json!({"ids": [1, 2]})), "ids");
    assert_eq!(pieces.len(), 2);
    let total: f64 = pieces.iter().map(|id| area(doc(&s), *id)).sum();
    assert_near("the regions are the big square's area between them", total, 16.0);
}

#[test]
fn fragment_takes_one_shape_and_gives_it_back_as_a_shape_without_text() {
    let mut s = session_with(vec![rect(1, (1.0, 1.0, 3.0, 3.0)).text("A").build()]);
    let id = made(&run(&mut s, "shape.fragment", json!({"ids": [1]})));
    let d = doc(&s);
    assert_near("area", area(d, id), 4.0);
    assert!(d.shape(&at(d, id)).unwrap().text.is_none());
}

// ----: a turned operand --------------------------------------------------------------------------------------------------

#[test]
fn a_turned_operand_takes_part_through_its_turn_and_the_result_is_unturned() {
    // A is 2 wide and 1 high with its pin at (2, 2) turned 90 degrees: it covers x 1.5 to 2.5 and y 1 to 3.
    let a = Build::new(1, 2.0, 1.0).pin(2.0, 2.0).angle_degrees(90.0).section(&[(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (0.0, 1.0)], true, false).build();
    let mut s = session_with(vec![a, rect(2, (2.0, 0.0, 3.0, 4.0)).build()]);
    let id = made(&run(&mut s, "shape.union", json!({"ids": [1, 2]})));
    let d = doc(&s);
    for (cell, want) in [("Angle", 0.0), ("Width", 1.5), ("Height", 4.0), ("PinX", 2.25), ("PinY", 2.0)] {
        assert_near(cell, num(d, id, cell), want);
    }
    assert_near("area", area(d, id), 5.0);
    assert!(filled(d, id, (1.6, 1.2)) && !filled(d, id, (1.2, 2.0)), "A's real outline took part and not a box from x 1 to 3");
}

#[test]
fn a_flipped_and_turned_operand_is_carried_as_drawn_by_every_operation() {
    for (flip_x, flip_y, degrees) in [(true, false, 0.0), (false, true, 0.0), (true, true, 0.0), (true, false, 30.0), (false, false, -75.0)] {
        let triangle = |id| {
            Build::new(id, 2.0, 2.0).pin(5.0, 5.0).angle_degrees(degrees).flip(flip_x, flip_y).section(
                &[(0.0, 0.0), (2.0, 0.0), (0.0, 2.0)],
                true,
                false,
            )
        };
        let mut s = session_with(vec![triangle(1).build(), rect(2, (4.0, 4.0, 5.0, 5.0)).build()]);
        let before = doc(&s).clone();
        let expected = craft_pathops::try_unite_all(&[
            (&fill_ring(&before, 1), craft_pathops::FillRule::EvenOdd),
            (&fill_ring(&before, 2), craft_pathops::FillRule::EvenOdd),
        ])
        .unwrap();
        let id = made(&run(&mut s, "shape.union", json!({"ids": [1, 2]})));
        let got = fill_ring(doc(&s), id);
        assert_close(
            &format!("flip {flip_x} {flip_y} turn {degrees}"),
            craft_pathops::area(&got, craft_pathops::FillRule::EvenOdd),
            craft_pathops::area(&expected, craft_pathops::FillRule::EvenOdd),
        );
        assert!(max_gap(&got, &expected) < 5.0e-6, "the outlines are one: {}", max_gap(&got, &expected));
    }
}

// ---- R21: Combine carries every row type exactly --------------------------------------------------------------------------------

/// A row of constants.
fn row(ix: u32, kind: &str, cells: &[(&str, f64)], text: &[(&str, &str, &str)]) -> Arc<Row> {
    let mut row = Row { ix: Some(ix), row_type: Some(Arc::from(kind)), ..Row::default() };
    for (name, n) in cells {
        row.cells.insert(CellName::new(name), Cell::constant(Value::from_text(&format!("{n}"))));
    }
    for (name, value, unit) in text {
        row.cells.insert(CellName::new(name), Cell::constant(Value::from_text(value)).with_unit(unit));
    }
    Arc::new(row)
}

fn section(ix: u32, rows: Vec<Arc<Row>>) -> Arc<diacraft_doc::Section> {
    let mut section = diacraft_doc::Section::new("Geometry", Some(ix));
    for flag in ["NoFill", "NoLine", "NoShow", "NoSnap", "NoQuickDrag"] {
        section.cells.insert(CellName::new(flag), Cell::constant(Value::from_text("0")));
    }
    section.rows = rows.into();
    Arc::new(section)
}

/// The row kinds, each as a closed section of a 4 by 2 shape.
fn row_kinds() -> Vec<(&'static str, Arc<diacraft_doc::Section>)> {
    let m = |ix, x: f64, y: f64| row(ix, "MoveTo", &[("X", x), ("Y", y)], &[]);
    let l = |ix, x: f64, y: f64| row(ix, "LineTo", &[("X", x), ("Y", y)], &[]);
    vec![
        (
            "ArcTo",
            section(
                0,
                vec![m(1, 0.0, 1.0), row(2, "ArcTo", &[("X", 4.0), ("Y", 1.0), ("A", 0.75)], &[]), l(3, 4.0, 0.0), l(4, 0.0, 0.0), l(5, 0.0, 1.0)],
            ),
        ),
        (
            "EllipticalArcTo",
            section(
                0,
                vec![
                    m(1, 0.0, 0.0),
                    row(2, "EllipticalArcTo", &[("X", 4.0), ("Y", 0.0), ("A", 2.0), ("B", 1.5), ("C", 0.3), ("D", 1.7)], &[]),
                    l(3, 0.0, 0.0),
                ],
            ),
        ),
        (
            "Rel rows",
            section(
                0,
                vec![
                    row(1, "RelMoveTo", &[("X", 0.0), ("Y", 0.0)], &[]),
                    row(2, "RelLineTo", &[("X", 1.0), ("Y", 0.0)], &[]),
                    row(3, "RelCubBezTo", &[("X", 1.0), ("Y", 1.0), ("A", 1.0), ("B", 0.4), ("C", 0.8), ("D", 1.0)], &[]),
                    row(4, "RelQuadBezTo", &[("X", 0.0), ("Y", 1.0), ("A", 0.5), ("B", 1.4)], &[]),
                    row(5, "RelEllipticalArcTo", &[("X", 0.0), ("Y", 0.0), ("A", -0.1), ("B", 0.5), ("C", 0.0), ("D", 1.0)], &[]),
                ],
            ),
        ),
        (
            "PolylineTo",
            section(
                0,
                vec![
                    m(1, 0.0, 0.0),
                    row(2, "PolylineTo", &[("X", 4.0), ("Y", 2.0)], &[("A", "POLYLINE(0,0,0.5,0.1,0.8,0.9)", "POLYLINE")]),
                    l(3, 0.0, 2.0),
                    l(4, 0.0, 0.0),
                ],
            ),
        ),
        (
            "NURBSTo",
            section(
                0,
                vec![
                    m(1, 0.0, 0.0),
                    row(
                        2,
                        "NURBSTo",
                        &[("X", 4.0), ("Y", 0.0), ("A", 0.0), ("B", 1.0), ("C", 0.0), ("D", 1.0)],
                        &[("E", "NURBS(1,3,1,1,1,2,0,1,3,2,0,1)", "NURBS")],
                    ),
                    l(3, 0.0, 0.0),
                ],
            ),
        ),
        (
            "SplineStart and SplineKnot",
            section(
                0,
                vec![
                    m(1, 0.0, 0.0),
                    row(2, "SplineStart", &[("X", 1.0), ("Y", 2.0), ("A", 0.0), ("B", 0.0), ("C", 1.0), ("D", 3.0)], &[]),
                    row(3, "SplineKnot", &[("X", 3.0), ("Y", 2.0), ("A", 0.0)], &[]),
                    row(4, "SplineKnot", &[("X", 4.0), ("Y", 0.0), ("A", 0.0)], &[]),
                    l(5, 0.0, 0.0),
                ],
            ),
        ),
        ("Ellipse", section(0, vec![row(1, "Ellipse", &[("X", 2.0), ("Y", 1.0), ("A", 4.0), ("B", 1.0), ("C", 2.0), ("D", 2.0)], &[])])),
    ]
}

/// How far the drawing `a` is from the drawing `b`, either way round. Two drawings cut into the same number of segments are compared segment by segment at eleven
/// places of each, which is exact where the carry only moved the points. Otherwise (an arc turned is cut in other places) each place of one is measured to the curves
/// of the other, which kurbo finds to about a millionth.
fn max_gap(a: &BezPath, b: &BezPath) -> f64 {
    let (sa, sb): (Vec<_>, Vec<_>) = (a.segments().collect(), b.segments().collect());
    if sa.len() == sb.len() {
        return sa
            .iter()
            .zip(&sb)
            .flat_map(|(x, y)| (0..=10).map(move |i| x.eval(f64::from(i) / 10.0).distance(y.eval(f64::from(i) / 10.0))))
            .fold(0.0, f64::max);
    }
    let one_way = |from: &[kurbo::PathSeg], to: &[kurbo::PathSeg]| {
        from.iter()
            .flat_map(|x| (0..=20).map(move |i| x.eval(f64::from(i) / 20.0)))
            .map(|p| to.iter().map(|s| s.nearest(p, 1.0e-12).distance_sq.sqrt()).fold(f64::INFINITY, f64::min))
            .fold(0.0, f64::max)
    };
    one_way(&sa, &sb).max(one_way(&sb, &sa))
}

#[test]
fn combine_carries_each_row_type_exactly_through_every_turn_and_flip() {
    for (name, geometry) in row_kinds() {
        for (flip_x, flip_y, degrees) in [
            (false, false, 0.0),
            (true, false, 0.0),
            (false, true, 0.0),
            (true, true, 0.0),
            (false, false, 40.0),
            (true, false, 40.0),
            (false, true, -110.0),
        ] {
            let mut first = Build::new(1, 4.0, 2.0).pin(6.0, 3.0).angle_degrees(degrees).flip(flip_x, flip_y).build();
            Arc::make_mut(&mut first.sheet).sections.push(geometry.clone());
            let mut s = session_with(vec![first, rect(2, (0.0, 0.0, 1.0, 1.0)).build()]);
            let before = doc(&s).clone();
            let original = {
                let (_, stroke, fill) = built(&before, 1).remove(0);
                let map = pose_map(&before, 1);
                (apply(stroke, map), apply(fill, map))
            };
            let id = made(&run(&mut s, "shape.combine", json!({"ids": [1, 2]})));
            let d = doc(&s);
            let carried = {
                let (_, stroke, fill) = built(d, id).remove(0);
                let map = pose_map(d, id);
                (apply(stroke, map), apply(fill, map))
            };
            let what = format!("{name}, flip {flip_x} {flip_y}, turn {degrees}");
            assert!(max_gap(&original.0, &carried.0) < 5.0e-6, "{what}: the drawn path moved: {}", max_gap(&original.0, &carried.0));
            assert_close(
                &format!("{what}: the filled area"),
                craft_pathops::area(&carried.1, craft_pathops::FillRule::EvenOdd),
                craft_pathops::area(&original.1, craft_pathops::FillRule::EvenOdd),
            );
            // The kinds stay: the carried section holds the rows of the same types, the relative moves, lines and elliptical arcs apart, which are written as `MoveTo`, `LineTo` and `EllipticalArcTo` with their shares as formulas.
            let kinds = |sec: &diacraft_doc::Section| {
                sec.rows
                    .iter()
                    .map(|r| {
                        r.row_type
                            .as_deref()
                            .unwrap_or("")
                            .replace("RelMoveTo", "MoveTo")
                            .replace("RelLineTo", "LineTo")
                            .replace("RelEllipticalArcTo", "EllipticalArcTo")
                    })
                    .collect::<Vec<_>>()
            };
            let shape = d.shape(&at(d, id)).unwrap();
            let carried_section = shape.sheet.section(&SectionKey::new("Geometry", Some(0))).unwrap();
            assert_eq!(kinds(carried_section), kinds(&geometry), "{what}: the row kinds");
        }
    }
}

/// Two numbers within a hundred-millionth: a cell holds nine places, so the width of a turned result is off by up to half a billionth, and its area by a few
/// billionths. (The vectors with whole numbers hold to 1E-9, in `shapeops.rs`.)
fn assert_close(what: &str, got: f64, want: f64) {
    assert!((got - want).abs() < 1.0e-7, "{what}: got {got}, want {want}");
}

fn apply(mut path: BezPath, map: Affine) -> BezPath {
    path.apply_affine(map);
    path
}

#[test]
fn a_flip_changes_the_sign_of_an_arcs_bow_and_a_turn_does_not() {
    let arc_section = row_kinds().remove(0).1;
    let bow_after = |flip_x: bool, degrees: f64| {
        let mut first = Build::new(1, 4.0, 2.0).pin(6.0, 3.0).angle_degrees(degrees).flip(flip_x, false).build();
        Arc::make_mut(&mut first.sheet).sections.push(arc_section.clone());
        let mut s = session_with(vec![first, rect(2, (0.0, 0.0, 1.0, 1.0)).build()]);
        let id = made(&run(&mut s, "shape.combine", json!({"ids": [1, 2]})));
        let d = doc(&s);
        let section = d.shape(&at(d, id)).unwrap().sheet.section(&SectionKey::new("Geometry", Some(0))).unwrap().clone();
        let arc = section.rows.iter().find(|r| r.row_type.as_deref() == Some("ArcTo")).unwrap().clone();
        arc.cells.get("A").and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap()
    };
    assert_near("not flipped", bow_after(false, 0.0), 0.75);
    assert_near("turned", bow_after(false, 40.0), 0.75);
    assert_near("flipped on one axis", bow_after(true, 0.0), -0.75);
}

// ---- R46: curves -----------------------------------------------------------------------------------------------------------------

/// A circle of radius 1 centred at (`cx`, `cy`), as one `Ellipse` row of a 2 by 2 shape.
fn circle(id: u32, cx: f64, cy: f64) -> Shape {
    let mut shape = Build::new(id, 2.0, 2.0).pin(cx, cy).build();
    let ellipse = row(1, "Ellipse", &[("X", 1.0), ("Y", 1.0), ("A", 2.0), ("B", 1.0), ("C", 1.0), ("D", 2.0)], &[]);
    Arc::make_mut(&mut shape.sheet).sections.push(section(0, vec![ellipse]));
    shape
}

/// The lens two unit circles a unit apart share.
fn lens() -> f64 {
    2.0 * 0.5_f64.acos() - 0.5 * 3.0_f64.sqrt()
}

#[test]
fn curved_edges_stay_within_a_ten_thousandth_of_the_exact_result_and_stay_curves() {
    for (command, want) in
        [("shape.union", 2.0 * std::f64::consts::PI - lens()), ("shape.intersect", lens()), ("shape.subtract", std::f64::consts::PI - lens())]
    {
        let mut s = session_with(vec![circle(1, 0.0, 0.0), circle(2, 1.0, 0.0)]);
        let id = made_or_kept(&run(&mut s, command, json!({"ids": [1, 2]})));
        let d = doc(&s);
        assert!((area(d, id) - want).abs() < 1.0e-4, "{command}: area {} against {want}", area(d, id));
        // Every point of the outline is on the boundary of one circle, to a ten-thousandth, and the rows are cubics, not a polygon of straight edges.
        let map = pose_map(d, id);
        for (_, stroke, _) in built(d, id) {
            let mut stroke = stroke;
            stroke.apply_affine(map);
            for p in flat_points(&stroke, 1.0e-3) {
                let off = [(0.0, 0.0), (1.0, 0.0)].iter().map(|c| (p.distance(Point::new(c.0, c.1)) - 1.0).abs()).fold(f64::INFINITY, f64::min);
                assert!(off < 1.0e-4, "{command}: a point {p:?} is {off} off both circles");
            }
        }
        let shape = d.shape(&at(d, id)).unwrap();
        let kinds: Vec<&str> = shape.sheet.sections.iter().flat_map(|sec| sec.rows.iter()).filter_map(|r| r.row_type.as_deref()).collect();
        assert!(kinds.contains(&"NURBSTo") && kinds.iter().filter(|k| **k == "LineTo").count() <= 2, "{command}: curves stay curves: {kinds:?}");
    }
    // The box of the union is the circles' own: 3 wide and 2 high, a curve counting by its extent.
    let mut s = session_with(vec![circle(1, 0.0, 0.0), circle(2, 1.0, 0.0)]);
    let id = made(&run(&mut s, "shape.union", json!({"ids": [1, 2]})));
    assert!((num(doc(&s), id, "Width") - 3.0).abs() < 1.0e-4 && (num(doc(&s), id, "Height") - 2.0).abs() < 1.0e-4);
}

/// The one shape a command made, or the one it kept (Subtract's first).
fn made_or_kept(result: &Json) -> u32 {
    match result["kept"].as_u64() {
        Some(kept) => u32::try_from(kept).unwrap(),
        None => made(result),
    }
}

/// The points of a path after it is flattened to `tolerance`: they lie on the path itself.
fn flat_points(path: &BezPath, tolerance: f64) -> Vec<Point> {
    let mut points: Vec<Point> = Vec::new();
    kurbo::flatten(path.iter(), tolerance, |el| match el {
        kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) => points.push(p),
        _ => {}
    });
    points
}

// ----: a master instance --------------------------------------------------------------------------------------------

#[test]
fn the_produced_shape_of_a_master_instance_has_no_master_and_draws_what_the_instance_drew() {
    let mut s = blank_session();
    let a = instance(&mut s, (1.0, 1.0, 3.0, 3.0));
    let b = instance(&mut s, (2.0, 2.0, 4.0, 4.0));
    let before = doc(&s).clone();
    let id = made(&run(&mut s, "shape.union", json!({"ids": [a, b]})));
    let d = doc(&s);
    assert!(d.shape(&at(d, id)).unwrap().master.is_none());
    let expected = craft_pathops::try_unite_all(&[
        (&fill_ring(&before, a), craft_pathops::FillRule::EvenOdd),
        (&fill_ring(&before, b), craft_pathops::FillRule::EvenOdd),
    ])
    .unwrap();
    assert!(max_gap(&fill_ring(d, id), &expected) < 5.0e-6);
}

#[test]
fn subtract_on_a_master_instance_marks_the_masters_section_deleted_and_the_new_ones_are_local() {
    let mut s = blank_session();
    let a = instance(&mut s, (0.0, 0.0, 4.0, 4.0));
    let b = instance(&mut s, (1.0, 1.0, 2.0, 2.0));
    run(&mut s, "shape.format", json!({"ids": [a], "fill": "#FF0000"}));
    let master = doc(&s).shape(&at(doc(&s), a)).unwrap().master;
    let result = run(&mut s, "shape.subtract", json!({"ids": [a, b]}));
    assert_eq!(result["kept"], json!(a));
    let d = doc(&s);
    let shape = d.shape(&at(d, a)).unwrap();
    assert_eq!(shape.master, master, "the same shape, still an instance");
    let sections: Vec<(Option<u32>, bool)> = shape.sheet.sections.iter().filter(|s| s.name.as_str() == "Geometry").map(|s| (s.ix, s.del)).collect();
    assert!(sections.contains(&(Some(0), true)), "the master's section is marked deleted on the instance: {sections:?}");
    assert_eq!(sections.iter().filter(|(_, deleted)| !deleted).count(), 2, "the outline and the hole are local sections: {sections:?}");
    assert!(sections.iter().all(|(ix, deleted)| *deleted || *ix != Some(0)), "the new ones do not take the deleted section's IX: {sections:?}");
    assert_near("area", area(d, a), 15.0);
    assert!(!filled(d, a, (1.5, 1.5)) && filled(d, a, (3.0, 3.0)));
    assert_eq!(formatting(&mut s, a)["fill"], json!("#ff0000"), "the first shape keeps its formatting");
}

#[test]
fn subtract_keeps_the_first_shapes_text_data_glue_and_place_in_the_drawing() {
    let mut s = blank_session();
    let a = instance(&mut s, (0.0, 0.0, 4.0, 4.0));
    let middle = instance(&mut s, (6.0, 6.0, 7.0, 7.0));
    let b = instance(&mut s, (1.0, 1.0, 2.0, 2.0));
    let far = instance(&mut s, (8.0, 1.0, 9.0, 2.0));
    run(&mut s, "shape.set_text", json!({"id": a, "text": "A"}));
    let connector = id_of(&run(&mut s, "connector.add", json!({"from": {"id": far}, "to": {"id": a, "point": 0}})));
    edit_shape(&mut s, a, |shape| {
        Arc::make_mut(&mut shape.sheet)
            .set_cell(&CellAddr::in_row("Property", None, RowKey::name("Cost"), "Value"), Cell::constant(Value::from_text("5")))
            .unwrap();
    });
    run(&mut s, "shape.subtract", json!({"ids": [a, b]}));
    let d = doc(&s);
    assert_eq!(top(d), [a, middle, far, connector], "A keeps its place in the drawing");
    assert_eq!(text_of(d, a).as_deref(), Some("A"));
    assert!(d.shape(&at(d, a)).unwrap().sheet.section(&SectionKey::new("Property", None)).is_some(), "Shape Data stays");
    assert_eq!(d.page(1).unwrap().connects.iter().filter(|c| c.to_sheet == a).count(), 1, "the connector stays glued to it");
}

#[test]
fn subtract_on_an_instance_that_inherits_sections_it_keeps_replaces_only_the_one_that_takes_the_fill() {
    // Predefined Process: a box (section 0) and two lines (section 1, `NoFill` 1), all the master's. B covers its right half.
    let mut s = blank_session();
    let a = id_of(&run(&mut s, "shape.drop", json!({"master": "predefined_process", "page": 1, "x": 3, "y": 3})));
    let b = instance(&mut s, (3.0, 2.0, 4.0, 4.0));
    let before = doc(&s).clone();
    let lines_before: Vec<Vec<(f64, f64)>> = local_points(&before, a).into_iter().skip(1).collect();
    run(&mut s, "shape.subtract", json!({"ids": [a, b]}));
    let d = doc(&s);
    let shape = d.shape(&at(d, a)).unwrap();
    let sections: Vec<(Option<u32>, bool)> = shape.sheet.sections.iter().filter(|s| s.name.as_str() == "Geometry").map(|s| (s.ix, s.del)).collect();
    assert!(sections.contains(&(Some(0), true)), "the box is deleted on the instance: {sections:?}");
    assert!(!sections.iter().any(|(ix, _)| *ix == Some(1)), "the lines stay the master's and are not touched: {sections:?}");
    assert!(sections.contains(&(Some(2), false)), "the new section takes an IX neither the instance nor its master holds: {sections:?}");
    assert_eq!(section_count(d, a), 1, "the new box is the instance's own section");
    let effective = Resolver::new(d).effective_sections(&SheetRef::Shape(at(d, a)), "Geometry").unwrap();
    assert_eq!(effective.iter().map(|k| k.ix).collect::<Vec<_>>(), [Some(1), Some(2)], "the master's lines and the new box");
    // The frame is kept, so that the lines are where they were, and the box is its left half.
    assert_eq!((num(d, a, "Width"), num(d, a, "Height")), (1.5, 0.8));
    assert_near("the fill", area(d, a), 0.75 * 0.8);
    let kept: Vec<Vec<(f64, f64)>> = local_points(d, a).into_iter().filter(|p| lines_before.iter().any(|l| same_points(l, p))).collect();
    assert_eq!(kept.len(), lines_before.len(), "the lines are as they were");
}

#[test]
fn a_first_shape_that_keeps_its_size_and_has_no_width_to_write_the_geometry_in_is_refused() {
    // The rows are plain lengths, so a shape of no width draws a square; with `LockCalcWH` the geometry would have to be written as shares of that width.
    let a = Build::new(1, 0.0, 2.0)
        .pin(2.0, 1.0)
        .cell("LockCalcWH", Cell::constant(Value::from_text("1")))
        .section(&[(0.0, 0.0), (4.0, 0.0), (4.0, 2.0), (0.0, 2.0)], true, false)
        .build();
    let mut s = session_with(vec![a, rect(2, (2.0, -1.0, 5.0, 3.0)).build()]);
    let before = snapshot(&s);
    let error = error_of(&mut s, "shape.subtract", &json!({"ids": [1, 2]}));
    assert!(error.to_string().contains("no width or no height"), "{error}");
    assert_unchanged(&s, &before, "a first shape with no width");
}

#[test]
fn subtract_of_a_turned_first_shape_keeps_its_angle_and_nothing_moves_on_the_page() {
    // A is 4 by 2, turned 30 degrees, its local pin at (2, 1) by formula; B is 4 square, turned the same, centred on A's local (3, 1): it covers A's local x from 1 on.
    let a = || {
        Build::new(1, 4.0, 2.0)
            .pin(5.0, 5.0)
            .angle_degrees(30.0)
            .cell("LocPinX", Cell::with_formula("Width*0.5", Some(Value::from_text("2"))).with_unit("IN"))
            .section(&[(0.0, 0.0), (4.0, 0.0), (4.0, 2.0), (0.0, 2.0)], true, false)
            .build()
    };
    let mut s = session_with(vec![a()]);
    let (centre, corner_before) = (pose_map(doc(&s), 1) * Point::new(3.0, 1.0), pose_map(doc(&s), 1) * Point::ORIGIN);
    let b = Build::new(2, 4.0, 4.0)
        .pin(centre.x, centre.y)
        .angle_degrees(30.0)
        .section(&[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)], true, false)
        .build();
    put(&mut s, vec![b]);
    run(&mut s, "shape.subtract", json!({"ids": [1, 2]}));
    let d = doc(&s);
    assert_near("Angle", num(d, 1, "Angle"), 30.0_f64.to_radians());
    assert_near("Width", num(d, 1, "Width"), 1.0);
    assert_near("Height", num(d, 1, "Height"), 2.0);
    // Nothing moved on the page: what is left is the strip of A from its local x 0 to 1, where it was drawn, and its box's lower-left corner has not moved.
    let map = pose_map(d, 1);
    let corner_after = map * Point::ORIGIN;
    assert!(
        near(corner_before.x, corner_after.x) && near(corner_before.y, corner_after.y),
        "the box's lower-left corner is where it was: {corner_before:?} {corner_after:?}"
    );
    let original = pose_map(session_with(vec![a()]).document().unwrap(), 1);
    for local_point in [(0.5, 1.0), (0.9, 0.2), (0.1, 1.9)] {
        let page = original * Point::new(local_point.0, local_point.1);
        assert!(filled(d, 1, (page.x, page.y)), "{local_point:?} of A is still filled where it was drawn");
    }
    let gone = original * Point::new(1.5, 1.0);
    assert!(!filled(d, 1, (gone.x, gone.y)), "what B covered is not");
}

/// What is left is the **right** part of the first shape, so the box's corner is not where the local origin is: the pin moves by the corner's offset too, whatever the first
/// shape is turned and flipped to. A is 4 by 2, turned 30 degrees, flipped each way in turn, its local pin by formula; B covers its local x up to 2. A's local x from 2 to 4 is
/// left, in a box 2 by 2, and every point of it is filled where it was drawn.
#[test]
fn subtract_that_leaves_the_right_part_of_a_turned_and_flipped_first_shape_leaves_it_where_it_was_drawn() {
    for (flip_x, flip_y) in [(false, false), (true, false), (false, true), (true, true)] {
        let a = || {
            Build::new(1, 4.0, 2.0)
                .pin(5.0, 5.0)
                .angle_degrees(30.0)
                .flip(flip_x, flip_y)
                .cell("LocPinX", Cell::with_formula("Width*0.5", Some(Value::from_text("2"))).with_unit("IN"))
                .section(&[(0.0, 0.0), (4.0, 0.0), (4.0, 2.0), (0.0, 2.0)], true, false)
                .build()
        };
        let mut s = session_with(vec![a()]);
        let original = pose_map(doc(&s), 1);
        let centre = original * Point::new(0.0, 1.0);
        let b = Build::new(2, 4.0, 4.0)
            .pin(centre.x, centre.y)
            .angle_degrees(30.0)
            .section(&[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)], true, false)
            .build();
        put(&mut s, vec![b]);
        run(&mut s, "shape.subtract", json!({"ids": [1, 2]}));
        let d = doc(&s);
        let what = format!("flipped {flip_x} {flip_y}");
        assert_near(&format!("Width, {what}"), num(d, 1, "Width"), 2.0);
        assert_near(&format!("Height, {what}"), num(d, 1, "Height"), 2.0);
        // The new box's lower-left corner is where A's local (2, 0) was drawn.
        let corner = pose_map(d, 1) * Point::ORIGIN;
        let was = original * Point::new(2.0, 0.0);
        assert!(near(corner.x, was.x) && near(corner.y, was.y), "{what}: the corner is where it was drawn: {corner:?} {was:?}");
        for local_point in [(2.5, 1.0), (3.9, 0.2), (2.1, 1.9)] {
            let page = original * Point::new(local_point.0, local_point.1);
            assert!(filled(d, 1, (page.x, page.y)), "{what}: {local_point:?} of A is still filled where it was drawn");
        }
        let gone = original * Point::new(1.0, 1.0);
        assert!(!filled(d, 1, (gone.x, gone.y)), "{what}: what B covered is not");
        assert_near(&format!("area, {what}"), area(d, 1), 4.0);
    }
}

/// A connector glued to the first shape's connection point at the right of the box follows the box when it is refitted: A is 4 by 2 with its point at (4, 1), B takes the
/// right half away and the box is 2 by 2, so the end is at (2, 1), where the point is, and not at (3, 1) where the point is while the pin has not yet moved to the new box.
#[test]
fn a_connector_glued_to_the_refitted_first_shape_ends_where_its_connection_point_is_now() {
    let find = |point: u32| {
        let mut s = blank_session();
        let a = instance(&mut s, (0.0, 0.0, 4.0, 2.0));
        let d = instance(&mut s, (7.0, 5.0, 8.0, 6.0));
        let connector = id_of(&run(&mut s, "connector.add", json!({"from": {"id": d}, "to": {"id": a, "point": point}})));
        (s, a, connector)
    };
    let (point, _) = (0..4)
        .map(|p| (p, find(p)))
        .find(|(_, (s, _, connector))| near(num(doc(s), *connector, "EndX"), 4.0) && near(num(doc(s), *connector, "EndY"), 1.0))
        .expect("one of the four points is at the right of the box");
    let (mut s, a, connector) = find(point);
    put(&mut s, vec![rect(50, (2.0, -1.0, 5.0, 3.0)).build()]);
    run(&mut s, "shape.subtract", json!({"ids": [a, 50]}));
    let d = doc(&s);
    assert_near("Width", num(d, a, "Width"), 2.0);
    assert_near("EndX", num(d, connector, "EndX"), 2.0);
    assert_near("EndY", num(d, connector, "EndY"), 1.0);
    assert_eq!(d.page(1).unwrap().connects.iter().filter(|c| c.to_sheet == a).count(), 1, "still glued to A");
}

// ---- the file -------------------------------------------------------------------------------------------------------------------

struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("diacraft-engine-shapeops-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        // Nothing to put back; a folder that cannot go is left for the system's temporary files.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A cell as (where, formula, value, unit).
type CellFacts = (String, Option<String>, Option<String>, Option<String>);

fn cells_of(d: &Document, id: u32) -> Vec<CellFacts> {
    let shape = d.shape(&at(d, id)).unwrap();
    let mut out = Vec::new();
    let show = |c: &Cell| (c.formula.text().map(str::to_owned), c.value.as_ref().map(|v| v.text().to_owned()), c.unit.as_deref().map(str::to_owned));
    for (name, cell) in &shape.sheet.cells {
        let (f, v, u) = show(cell);
        out.push((name.to_string(), f, v, u));
    }
    for section in &shape.sheet.sections {
        for (name, cell) in &section.cells {
            let (f, v, u) = show(cell);
            out.push((format!("{}[{:?}].{name}", section.name.as_str(), section.ix), f, v, u));
        }
        for (i, row) in section.rows.iter().enumerate() {
            for (name, cell) in &row.cells {
                let (f, v, u) = show(cell);
                out.push((format!("{}[{:?}].{i}:{}.{name}", section.name.as_str(), section.ix, row.row_type.as_deref().unwrap_or("")), f, v, u));
            }
        }
    }
    out
}

#[test]
fn the_produced_shapes_save_to_vsdx_and_reopen_with_the_cells_equal() {
    for command in ["shape.union", "shape.combine", "shape.fragment", "shape.intersect"] {
        let mut s = session_with(vec![rect(1, (1.0, 1.0, 3.0, 3.0)).text("A").build(), rect(2, (2.0, 2.0, 4.0, 4.0)).build()]);
        let produced = ids(&run(&mut s, command, json!({"ids": [1, 2]})), "ids");
        let dir = Dir::new();
        let path = dir.path("out.vsdx");
        run(&mut s, "file.save_as", json!({"path": path}));
        let mut reopened = Session::new();
        run(&mut reopened, "file.open", json!({"path": path}));
        for id in produced {
            assert_eq!(cells_of(doc(&s), id), cells_of(doc(&reopened), id), "{command}: shape {id}");
            assert_eq!(area(doc(&s), id), area(doc(&reopened), id), "{command}: shape {id} draws the same");
        }
    }
}

#[test]
fn a_subtracted_instance_saves_and_reopens_drawing_the_same() {
    let mut s = blank_session();
    let a = instance(&mut s, (0.0, 0.0, 4.0, 4.0));
    let b = instance(&mut s, (1.0, 1.0, 2.0, 2.0));
    run(&mut s, "shape.subtract", json!({"ids": [a, b]}));
    let dir = Dir::new();
    let path = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut reopened = Session::new();
    run(&mut reopened, "file.open", json!({"path": path}));
    assert_near("area", area(doc(&reopened), a), 15.0);
    assert_eq!(section_count(doc(&reopened), a), 2);
}

// ---- R14: containers and lists --------------------------------------------------------------------------------------------------

fn tie(code: u32, targets: &[u32]) -> String {
    format!("DEPENDSON({code},{})", targets.iter().map(|id| format!("Sheet.{id}!SheetRef()")).collect::<Vec<_>>().join(","))
}

fn relationships(ties: &[String]) -> Cell {
    Cell::with_formula(&format!("SUM({})", ties.join(",")), Some(Value::from_text("0")))
}

fn formula_of(d: &Document, id: u32, name: &str) -> Option<String> {
    local(d, id, name).and_then(|c| c.0)
}

fn make_container(s: &mut Session, id: u32, kind: &str) {
    edit_shape(s, id, |shape| {
        Arc::make_mut(&mut shape.sheet)
            .set_cell(&CellAddr::in_row("User", None, RowKey::name("msvStructureType"), "Value"), Cell::constant(Value::from_text(kind)))
            .unwrap();
    });
}

#[test]
fn the_produced_shape_takes_the_first_shapes_place_in_containers_and_lists_and_the_other_leaves_its_own() {
    // Container 3 holds 1 and 2, list 4 holds 1 and 2; 1 is in both, 2 is in both, and 1 has a callout 5 (a tie that is not carried).
    let a = rect(1, (1.0, 1.0, 3.0, 3.0)).cell("Relationships", relationships(&[tie(4, &[3]), tie(5, &[4]), tie(3, &[5])])).build();
    let b = rect(2, (2.0, 2.0, 4.0, 4.0)).cell("Relationships", relationships(&[tie(4, &[3]), tie(5, &[4])])).build();
    let container = rect(3, (0.0, 0.0, 6.0, 6.0)).cell("Relationships", relationships(&[tie(1, &[1, 2])])).build();
    let list = rect(4, (0.0, 7.0, 6.0, 8.0)).cell("Relationships", relationships(&[tie(2, &[1, 2])])).build();
    let callout = rect(5, (9.0, 9.0, 10.0, 10.0)).cell("Relationships", relationships(&[tie(6, &[1])])).build();
    let mut s = session_with(vec![a, b, container, list, callout]);
    make_container(&mut s, 3, "Container");
    make_container(&mut s, 4, "List");
    let id = made(&run(&mut s, "shape.union", json!({"ids": [1, 2]})));
    let d = doc(&s);
    // The printer of formulas puts a blank after each comma; what the formula says is the same.
    assert_eq!(
        formula_of(d, id, "Relationships").unwrap().replace(' ', ""),
        format!("SUM({},{})", tie(4, &[3]), tie(5, &[4])),
        "the first shape's places, and no callout"
    );
    assert_eq!(
        formula_of(d, 3, "Relationships").unwrap(),
        format!("SUM({})", tie(1, &[id, 2])),
        "the container names the produced shape where it named the first"
    );
    assert_eq!(formula_of(d, 4, "Relationships").unwrap(), format!("SUM({})", tie(2, &[id, 2])));
    assert_eq!(
        formula_of(d, 5, "Relationships").unwrap(),
        format!("SUM({})", tie(6, &[1])),
        "a callout's tie to a shape that is gone is left to name nothing"
    );
}

#[test]
fn ties_in_sums_and_signs_are_found_and_a_container_inside_a_group_is_retargeted() {
    // The first shape's ties are a sum of a call and a sum that holds a negated call; container 3 is a member of a group with shape 6.
    let a = rect(1, (1.0, 1.0, 3.0, 3.0))
        .cell("Relationships", Cell::with_formula(&format!("{}+SUM(-{})", tie(4, &[3]), tie(5, &[4])), Some(Value::from_text("0"))))
        .build();
    let container = rect(3, (0.0, 0.0, 6.0, 6.0)).cell("Relationships", relationships(&[tie(1, &[1, 2])])).build();
    let list = rect(4, (0.0, 7.0, 6.0, 8.0)).cell("Relationships", relationships(&[tie(2, &[1])])).build();
    let mut s = session_with(vec![a, rect(2, (2.0, 2.0, 4.0, 4.0)).build(), container, list, rect(6, (9.0, 9.0, 10.0, 10.0)).build()]);
    make_container(&mut s, 3, "Container");
    make_container(&mut s, 4, "List");
    run(&mut s, "shape.group", json!({"ids": [3, 6]}));
    let id = made(&run(&mut s, "shape.union", json!({"ids": [1, 2]})));
    let d = doc(&s);
    assert_eq!(
        formula_of(d, id, "Relationships").unwrap().replace(' ', ""),
        format!("SUM({},{})", tie(4, &[3]), tie(5, &[4])),
        "both ties, whatever the formula is made of"
    );
    assert_eq!(
        formula_of(d, 3, "Relationships").unwrap(),
        format!("SUM({})", tie(1, &[id, 2])),
        "the container in the group names the produced shape"
    );
    assert_eq!(formula_of(d, 4, "Relationships").unwrap(), format!("SUM({})", tie(2, &[id])), "so does the list on the page");
}

#[test]
fn a_container_and_a_list_are_refused_whether_or_not_they_say_so_and_a_shape_that_holds_none_is_not() {
    let container = rect(2, (0.0, 0.0, 6.0, 6.0)).cell("Relationships", relationships(&[tie(1, &[1])])).build();
    let mut s = session_with(vec![rect(1, (1.0, 1.0, 3.0, 3.0)).build(), container]);
    let before = snapshot(&s);
    let error = error_of(&mut s, "shape.union", &json!({"ids": [1, 2]}));
    assert!(error.to_string().contains("container or a list"), "{error}");
    assert_unchanged(&s, &before, "a container");
    // A shape in a container, which holds nobody, takes part.
    let member = rect(1, (1.0, 1.0, 3.0, 3.0)).cell("Relationships", relationships(&[tie(4, &[9])])).build();
    let mut s = session_with(vec![member, rect(2, (2.0, 2.0, 4.0, 4.0)).build()]);
    run(&mut s, "shape.union", json!({"ids": [1, 2]}));
}

#[test]
fn a_command_that_makes_many_shapes_puts_none_in_the_first_shapes_container() {
    let a = rect(1, (1.0, 1.0, 3.0, 3.0)).cell("Relationships", relationships(&[tie(4, &[3])])).build();
    let container = rect(3, (0.0, 0.0, 6.0, 6.0)).cell("Relationships", relationships(&[tie(1, &[1])])).build();
    let mut s = session_with(vec![a, rect(2, (2.0, 2.0, 4.0, 4.0)).build(), container]);
    make_container(&mut s, 3, "Container");
    let made_by_fragment = ids(&run(&mut s, "shape.fragment", json!({"ids": [1, 2]})), "ids");
    let d = doc(&s);
    assert!(made_by_fragment.iter().all(|id| formula_of(d, *id, "Relationships").is_none()));
    assert_eq!(
        formula_of(d, 3, "Relationships").unwrap(),
        format!("SUM({})", tie(1, &[1])),
        "the container is left as it was: its tie names a shape that is gone"
    );
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

fn two_filled_squares() -> Session {
    let mut s = session_with(vec![rect(1, (1.0, 1.0, 3.0, 3.0)).build(), rect(2, (2.0, 2.0, 4.0, 4.0)).build()]);
    run(&mut s, "shape.format", json!({"ids": [1], "fill": "#FF0000", "line": "none"}));
    run(&mut s, "shape.format", json!({"ids": [2], "fill": "#0000FF", "line": "none"}));
    s
}

#[test]
fn the_render_check_a_union_draws_the_pixels_of_and_the_same_as_the_outline_written_by_hand() {
    let mut s = two_filled_squares();
    run(&mut s, "shape.union", json!({"ids": [1, 2]}));
    let r = render(&s);
    assert!(r.skipped.is_empty(), "everything was drawn: {:?}", r.skipped);
    assert!(
        red(px(&r, 2.5, 2.5)) && red(px(&r, 1.5, 1.5)) && red(px(&r, 3.5, 3.5)) && red(px(&r, 1.5, 2.5)) && red(px(&r, 2.5, 3.5)),
        "inside the union"
    );
    assert!(!red(px(&r, 3.5, 1.5)) && !red(px(&r, 1.5, 3.5)), "the corners outside it are paper");
    // The same page with the outline written by hand as plain rows: the pixels are the same.
    let mut reference = session_with(vec![
        Build::new(1, 3.0, 3.0)
            .pin(2.5, 2.5)
            .section(&[(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (3.0, 1.0), (3.0, 3.0), (1.0, 3.0), (1.0, 2.0), (0.0, 2.0)], true, false)
            .build(),
    ]);
    run(&mut reference, "shape.format", json!({"ids": [1], "fill": "#FF0000", "line": "none"}));
    assert!(r.rgba_premul == render(&reference).rgba_premul, "the union draws the pixels of the outline");
}

#[test]
fn the_render_check_combine_draws_the_hole_where_the_originals_overlapped() {
    let mut s = two_filled_squares();
    run(&mut s, "shape.combine", json!({"ids": [1, 2]}));
    let r = render(&s);
    assert!(red(px(&r, 1.5, 1.5)) && red(px(&r, 3.5, 3.5)), "A alone and B alone are filled");
    assert!(!red(px(&r, 2.5, 2.5)), "where they overlapped is the page showing through");
}

#[test]
fn the_render_check_a_fragment_draws_each_piece_with_the_first_shapes_fill_and_a_subtract_the_first_less_the_others() {
    let mut s = two_filled_squares();
    run(&mut s, "shape.fragment", json!({"ids": [1, 2]}));
    let r = render(&s);
    assert!(red(px(&r, 2.5, 2.5)) && red(px(&r, 1.5, 1.5)) && red(px(&r, 3.5, 3.5)), "every piece has the first shape's fill");
    let mut s = two_filled_squares();
    run(&mut s, "shape.subtract", json!({"ids": [1, 2]}));
    let r = render(&s);
    assert!(red(px(&r, 1.5, 1.5)) && red(px(&r, 1.5, 2.5)) && red(px(&r, 2.5, 1.5)), "what is left of A");
    assert!(!red(px(&r, 2.5, 2.5)) && !red(px(&r, 3.5, 3.5)), "what B covered is gone");
    let mut s = two_filled_squares();
    run(&mut s, "shape.intersect", json!({"ids": [1, 2]}));
    let r = render(&s);
    assert!(red(px(&r, 2.5, 2.5)) && !red(px(&r, 1.5, 1.5)) && !red(px(&r, 3.5, 3.5)), "only the overlap");
}

#[test]
fn local_points_of_a_fragment_piece_are_in_its_own_frame_with_the_box_at_the_origin() {
    let mut s = session_with(vec![rect(1, (1.0, 1.0, 3.0, 3.0)).build(), rect(2, (2.0, 2.0, 4.0, 4.0)).build()]);
    let pieces = ids(&run(&mut s, "shape.fragment", json!({"ids": [1, 2]})), "ids");
    let d = doc(&s);
    assert_points("the overlap", &local_points(d, pieces[2])[0], &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]);
    assert_eq!(page_points(d, pieces[2]).len(), 4);
}
