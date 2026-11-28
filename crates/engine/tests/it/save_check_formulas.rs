//! The cross-functional flowchart said "18 formulas are lost" after a Save As `.odg`, and so did a BPMN pool (6), a container (2) and any shape whose fill or line a person coloured. The
//! cells were the `FillForegnd` and `LineColor` of the shapes, each `RGB(r,g,b)`, the formula a colour picked from Standard Colors, Recent Colors or the Colors dialog is written as (`shape.format`) and the one
//! the swimlane, pool and container stencils write. The `.odg` writer is handed a copy of the drawing in which a shape's frame and style cells are the constants they resolve to, and the copy put
//! the shape's own formula back only for a drawing from a VSDX file and for the formulas of the theme (`THEMEGUARD`, `THEMEVAL`): any other formula was a value in the file, which holds a colour in the
//! attribute of its style, and no part of the file held the formula, so the drawing read back lost it (a formula that does not come back is a loss, even when its value does). **Every drawing
//! keeps the shape's own formulas now**, in DiaCraft's data on the shape's style beside the value in the file's attribute, as a drawing from a VSDX file did. The neighbours the sweeps found are here too: a
//! formula a person typed in the ShapeSheet, the pin `Width*0` of `shape.pin`, and the formula of a connector's fill. What stays said is the place of a member of a group, which the reader works out from the group's frame.
//!
//! Red on the commit this task starts from: the template saved as `.odg` names 18 formulas, a pool 6, a shape coloured by `shape.format` 2, a typed formula and a pin each as many as were typed.

use diacraft_doc::Document;
use serde_json::{Value, json};

use crate::edit::id_of;
use crate::native_common::{Dir, opened, pinned};
use crate::run;
use crate::swimlane::{ids, structure};
use crate::swimlane_files::state;

/// How many formulas a save's `checked` field says are lost, and the other classes it names.
fn lost_formulas(said: &Value) -> u64 {
    said["checked"]["classes"]
        .as_array()
        .map_or(0, |list| list.iter().filter(|c| c["class"] == "formulas").map(|c| c["count"].as_u64().unwrap_or(0)).sum())
}

/// The classes a save's `checked` field names, in words, for a failure message.
fn classes(said: &Value) -> String {
    said["checked"]["classes"]
        .as_array()
        .map_or_else(|| said["checked"].to_string(), |list| list.iter().map(|c| c["words"].to_string()).collect::<Vec<_>>().join(", "))
}

/// A colour cell of a shape: its name, its formula and its value.
type Colour = (String, Option<String>, Option<String>);

/// The formula and the value of `FillForegnd` and `LineColor` that each shape of the first page that is no background holds itself, in the page's order.
fn colours(doc: &Document) -> Vec<Colour> {
    let page = doc.pages.iter().find(|p| !p.background).expect("a page");
    let mut out = Vec::new();
    for shape in page.shapes.iter() {
        for name in ["FillForegnd", "LineColor"] {
            let cell = shape.sheet.cell(name);
            out.push((
                name.to_owned(),
                cell.and_then(|c| c.formula.text()).map(str::to_owned),
                cell.and_then(|c| c.value.as_ref()).map(|v| v.text().to_ascii_lowercase()),
            ));
        }
    }
    out
}

/// A session with a new drawing from the cross-functional flowchart template.
fn from_the_template() -> diacraft_engine::Session {
    let mut s = pinned();
    run(&mut s, "file.new", json!({ "template": "cross_functional_flowchart" }));
    s
}

/// **The bug.** The template's nine shapes (the frame, two lists, three lanes and three phases) hold an `RGB(r,g,b)` fill and line each: saved as `.odg` they come back, and the check names none lost.
#[test]
fn the_cross_functional_template_saved_as_odg_loses_no_formula() {
    let dir = Dir::new();
    let mut s = from_the_template();
    let drawn = colours(s.document().unwrap());
    assert_eq!(drawn.len(), 18, "nine shapes, a fill and a line each");
    assert!(
        drawn.iter().all(|(_, formula, _)| formula.as_deref().is_some_and(|f| f.starts_with("RGB("))),
        "the stencil writes each as a fixed colour: {drawn:?}"
    );
    let said = run(&mut s, "file.save_as", json!({ "path": dir.path("lanes.odg") }));
    assert_eq!(lost_formulas(&said), 0, "{}", classes(&said));
    let back = opened(&dir.path("lanes.odg"));
    assert_eq!(colours(back.document().unwrap()), drawn, "the formulas and the values come back as they were");
}

/// **A structure read back from an `.odg` is worked on as one read back from a `.diacraft`**: a lane is inserted and another resized, and the structure, the boxes, the ties and the colours are the same.
#[test]
fn a_structure_read_back_from_an_odg_is_edited_as_one_read_back_from_a_diacraft_file() {
    let dir = Dir::new();
    let mut after_edit: Vec<(&str, Value, Vec<Colour>)> = Vec::new();
    for ext in ["diacraft", "odg"] {
        let mut s = from_the_template();
        run(&mut s, "file.save_as", json!({ "path": dir.path(&format!("lanes.{ext}")) }));
        let mut back = opened(&dir.path(&format!("lanes.{ext}")));
        let lanes = ids(&structure(&mut back)["lanes"].as_array().unwrap().iter().map(|l| l["id"].clone()).collect::<Value>());
        run(&mut back, "swimlane.insert", json!({}));
        run(&mut back, "swimlane.resize", json!({ "id": lanes[1], "size": 2.0 }));
        assert_eq!(structure(&mut back)["lanes"].as_array().unwrap().len(), 4, "{ext}: a lane was added");
        let colours = colours(back.document().unwrap());
        after_edit.push((ext, state(&mut back), colours));
    }
    let (diacraft, odg) = (&after_edit[0], &after_edit[1]);
    assert_eq!(odg.1, diacraft.1, "the structure, the boxes, the ties and the roles after the same edits");
    assert_eq!(odg.2, diacraft.2, "the colours, with their formulas, after the same edits");
}

/// **The BPMN pool is the same** (its frame, lane list and lane are the swimlane stencil's shapes in the pool's colours).
#[test]
fn a_bpmn_pool_saved_as_odg_loses_no_formula() {
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "stencil.open", json!({ "id": "bpmn" }));
    let page = s.document().unwrap().pages.get(0).unwrap().id;
    run(&mut s, "shape.drop", json!({ "master": "bpmn.pool", "page": page, "x": 4.0, "y": 4.0 }));
    let drawn = colours(s.document().unwrap());
    assert!(drawn.iter().any(|(_, formula, _)| formula.as_deref().is_some_and(|f| f.starts_with("RGB("))), "the pool holds fixed colours: {drawn:?}");
    let said = run(&mut s, "file.save_as", json!({ "path": dir.path("pool.odg") }));
    assert_eq!(lost_formulas(&said), 0, "{}", classes(&said));
    assert_eq!(colours(opened(&dir.path("pool.odg")).document().unwrap()), drawn);
}

/// **A colour a person chose is the same**: `shape.format` writes `RGB(r,g,b)` for a fill and a line. It comes back through a Save As, and **an edit saved in place into the file** (a recolour) carries the
/// new formula; the file read again has it, and a third save with no edit is no edit.
#[test]
fn a_colour_a_person_chose_keeps_its_formula_through_a_save_as_and_a_save_in_place() {
    let dir = Dir::new();
    let mut s = pinned();
    let page = s.document().unwrap().pages.get(0).unwrap().id;
    run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": page, "x": 3.0, "y": 3.0 }));
    let id = s.document().unwrap().pages.get(0).unwrap().shapes.iter().next().unwrap().id;
    run(&mut s, "shape.format", json!({ "ids": [id], "fill": "#ff0000", "line": "#00ff00" }));
    let said = run(&mut s, "file.save_as", json!({ "path": dir.path("coloured.odg") }));
    assert_eq!(lost_formulas(&said), 0, "{}", classes(&said));
    let formulas = |s: &diacraft_engine::Session| colours(s.document().unwrap()).into_iter().map(|(_, f, _)| f).collect::<Vec<_>>();
    assert_eq!(formulas(&opened(&dir.path("coloured.odg"))), [Some("RGB(255,0,0)".to_owned()), Some("RGB(0,255,0)".to_owned())]);
    // A recolour, saved in place.
    run(&mut s, "shape.format", json!({ "ids": [id], "fill": "#0000ff" }));
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["edited"], json!(true), "{saved}");
    let back = opened(&dir.path("coloured.odg"));
    assert_eq!(
        colours(back.document().unwrap()),
        [
            ("FillForegnd".to_owned(), Some("RGB(0,0,255)".to_owned()), Some("#0000ff".to_owned())),
            ("LineColor".to_owned(), Some("RGB(0,255,0)".to_owned()), Some("#00ff00".to_owned())),
        ]
    );
    let again = run(&mut s, "file.save", json!({}));
    assert_eq!(again["edited"], json!(false), "a save with no edit after it is no edit: {again}");
}

/// **A formula that does not come back is still named.** The check reads the file back and compares: with the fill's formulas taken out of the drawing read back (the hook a test plants a loss with),
/// the save of the template names nine formulas.
#[test]
fn a_formula_the_file_read_back_lacks_is_still_named_lost() {
    let dir = Dir::new();
    let mut s = from_the_template();
    let said = diacraft_engine::check::with_tamper(
        |doc| {
            let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
            for at in 0..page.shapes.len() {
                let shape = std::sync::Arc::make_mut(page.shapes.get_mut(at).unwrap());
                let cell = diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("#e8eef7"));
                std::sync::Arc::make_mut(&mut shape.sheet).cells.insert(diacraft_doc::CellName::new("FillForegnd"), cell);
            }
        },
        || run(&mut s, "file.save_as", json!({ "path": dir.path("lost.odg") })),
    );
    assert_eq!(lost_formulas(&said), 9, "{}", classes(&said));
}

/// The middle of the shape `id` on the first page, in inches.
fn middle_of(s: &diacraft_engine::Session, id: u32) -> (f64, f64) {
    let page = s.document().unwrap().pages.iter().find(|p| !p.background).unwrap().id;
    let c = diacraft_engine::draw::shape_corners(s.document().unwrap(), page, id).unwrap();
    (c.iter().map(|p| p.0).sum::<f64>() / 4.0, c.iter().map(|p| p.1).sum::<f64>() / 4.0)
}

/// The `ID` of the shape a `shape.drop` made.
fn dropped(s: &mut diacraft_engine::Session, master: &str, at: (f64, f64)) -> u32 {
    let page = s.document().unwrap().pages.iter().find(|p| !p.background).unwrap().id;
    id_of(&run(s, "shape.drop", json!({ "master": master, "page": page, "x": at.0, "y": at.1 })))
}

/// **The structures the commands build** (the neighbours the brief names: a BPMN pool and its lanes, an org chart, the cross-functional flowchart worked on, a container and a callout, whose ties are
/// formulas too) save as `.odg` with no formula lost.
#[test]
fn the_structures_the_commands_build_save_as_odg_with_no_formula_lost() {
    let dir = Dir::new();
    let mut failures = Vec::new();
    let mut scenarios: Vec<(&str, diacraft_engine::Session)> = Vec::new();

    // An org chart: the template's executive, a manager and a position dropped on it and on the manager, laid out again.
    let mut s = pinned();
    run(&mut s, "file.new", json!({ "template": "organization_chart" }));
    let boss = s.document().unwrap().pages.iter().find(|p| !p.background).unwrap().shapes.iter().next().unwrap().id;
    let at = middle_of(&s, boss);
    let manager = dropped(&mut s, "orgchart.manager", at);
    let at = middle_of(&s, manager);
    dropped(&mut s, "orgchart.position", at);
    run(&mut s, "orgchart.layout", json!({ "layout": "left_to_right" }));
    scenarios.push(("an org chart", s));

    // The cross-functional flowchart worked on: a lane, a phase, a label turned, a step dropped in a lane.
    let mut s = from_the_template();
    let lanes = ids(&structure(&mut s)["lanes"].as_array().unwrap().iter().map(|l| l["id"].clone()).collect::<Value>());
    run(&mut s, "swimlane.insert", json!({}));
    run(&mut s, "swimlane.insert_phase", json!({}));
    run(&mut s, "swimlane.rotate_label", json!({ "ids": [lanes[0]] }));
    let at = middle_of(&s, lanes[1]);
    dropped(&mut s, "process", at);
    scenarios.push(("the cross-functional flowchart worked on", s));

    // A BPMN pool with a lane added and a task in it.
    let mut s = pinned();
    run(&mut s, "stencil.open", json!({ "id": "bpmn" }));
    dropped(&mut s, "bpmn.pool", (5.0, 4.0));
    let lanes = ids(&structure(&mut s)["lanes"].as_array().unwrap().iter().map(|l| l["id"].clone()).collect::<Value>());
    run(&mut s, "swimlane.insert", json!({}));
    let at = middle_of(&s, lanes[0]);
    dropped(&mut s, "bpmn.task", at);
    scenarios.push(("a BPMN pool with a lane and a task", s));

    // A container round two shapes, and a callout for one of them.
    let mut s = pinned();
    let (a, b) = (dropped(&mut s, "rectangle", (3.0, 3.0)), dropped(&mut s, "rectangle", (6.0, 3.0)));
    run(&mut s, "container.add", json!({ "ids": [a, b], "heading": "Group" }));
    run(&mut s, "callout.insert", json!({ "ids": [a] }));
    scenarios.push(("a container and a callout", s));

    for (what, mut s) in scenarios {
        let said = run(&mut s, "file.save_as", json!({ "path": dir.path("built.odg") }));
        if lost_formulas(&said) > 0 {
            failures.push(format!("{what}: {}", classes(&said)));
        }
        // And what the file reads back as is saved again with no edit and no refusal.
        if let Err(why) = s.execute("file.save", &json!({})) {
            failures.push(format!("{what}: the save after it was refused: {why}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// **Every template and every master of every stencil**, saved as `.odg`, names no formula lost: the stencils' own colours are formulas too, and each new structure that writes one (a pool, a swimlane,
/// a list) is covered by this sweep. The shapes of one stencil go on one page, each in its own place.
#[test]
fn no_template_and_no_master_of_any_stencil_saved_as_odg_names_a_formula() {
    let dir = Dir::new();
    let mut failures = Vec::new();
    for template in diacraft_engine::template::templates() {
        let mut s = pinned();
        run(&mut s, "file.new", json!({ "template": template.id }));
        let said = run(&mut s, "file.save_as", json!({ "path": dir.path("template.odg") }));
        if lost_formulas(&said) > 0 {
            failures.push(format!("the template {}: {}", template.id, classes(&said)));
        }
    }
    let listed = run(&mut pinned(), "stencil.list", json!({}));
    let mut stencils: Vec<String> = Vec::new();
    for group in ["available", "stencils"] {
        stencils.extend(listed[group].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap().to_owned()));
    }
    assert!(stencils.len() >= 15, "the built-in stencils: {stencils:?}");
    for stencil in &stencils {
        let mut s = pinned();
        let _ = s.execute("stencil.open", &json!({ "id": stencil }));
        let page = s.document().unwrap().pages.get(0).unwrap().id;
        let masters: Vec<String> = run(&mut s, "stencil.list", json!({}))["stencils"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["id"] == stencil.as_str())
            .flat_map(|e| e["masters"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap().to_owned()).collect::<Vec<_>>())
            .collect();
        for (at, master) in masters.iter().enumerate() {
            let (x, y) = (1.5 + 2.5 * (at % 4) as f64, 1.5 + 2.0 * (at / 4) as f64);
            if let Err(why) = s.execute("shape.drop", &json!({ "master": master, "page": page, "x": x, "y": y })) {
                failures.push(format!("{master}: the drop was refused: {why}"));
            }
        }
        match s.execute("file.save_as", &json!({ "path": dir.path("stencil.odg") })) {
            Ok(said) if lost_formulas(&said) > 0 => failures.push(format!("the stencil {stencil}: {}", classes(&said))),
            Ok(_) => {}
            Err(why) => failures.push(format!("the stencil {stencil}: the save was refused: {why}")),
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// **The result of every undoable command**, in the drawing the command sweeps use, saved as `.odg`: the check names no formula lost, but for the places of a group's members (a save a model writer
/// refuses, a background page, is not this test's). The failures name the cells the check says are lost. Red before the general fix: `shape.pin` (the pin `Width*0`), `shape.format` (the fill of a
/// connector in the selection) and `shape.group`.
#[test]
fn the_result_of_every_command_saved_as_odg_names_no_formula_lost() {
    let dir = Dir::new();
    let samples = crate::command_sweep::undoable_samples();
    assert!(samples.len() > 40, "the sweep looks at the commands: {}", samples.len());
    let mut failures = Vec::new();
    for (k, (id, params)) in samples.into_iter().enumerate() {
        // The members of a group hold their place as formulas of the group, which the reader works out again from the group's frame: a file holds them as numbers, and the save and the check say so
        // (the test below).
        if id == "shape.group" {
            continue;
        }
        let mut s = crate::session_with_history();
        for (setup_id, setup_params) in crate::command_sweep::setup_for(id) {
            run(&mut s, setup_id, setup_params);
        }
        run(&mut s, id, params.clone());
        let name = format!("c{k}.odg");
        // A save the model writer refuses (a background page) is no drawing of this test.
        let Ok(said) = s.execute("file.save_as", &json!({ "path": dir.path(&name) })) else { continue };
        if lost_formulas(&said) == 0 {
            continue;
        }
        let (after, _) = diacraft_engine::odgfile::read_odg(&dir.read(&name), &diacraft_odg::Limits::default()).unwrap();
        let cells: Vec<String> =
            diacraft_engine::check::explain(s.document().unwrap(), &after).into_iter().filter(|line| line.contains("formulas:")).take(4).collect();
        failures.push(format!("after `{id}` {params}: {} {cells:?}", classes(&said)));
    }
    assert!(failures.is_empty(), "{} commands:\n{}", failures.len(), failures.join("\n"));
}

/// The formula the shape `id` of the first page holds itself in the cell `name`, and the value it holds.
fn own_cell(s: &diacraft_engine::Session, id: u32, name: &str) -> (Option<String>, Option<String>) {
    let d = s.document().unwrap();
    let page = d.pages.iter().find(|p| !p.background).unwrap().id;
    let shape = d.shape(&d.find_shape(diacraft_doc::Owner::Page(page), id).unwrap()).unwrap();
    let cell = shape.sheet.cell(name);
    (cell.and_then(|c| c.formula.text()).map(str::to_owned), cell.and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()))
}

/// The `ID`s of the shapes of the first page, in order.
fn shape_ids(s: &diacraft_engine::Session) -> Vec<u32> {
    s.document().unwrap().pages.iter().find(|p| !p.background).unwrap().shapes.iter().map(|shape| shape.id).collect()
}

/// **The neighbour the brief names: the formulas a person types**, in the frame and style cells of the shapes of a new drawing (the ShapeSheet window writes them), come back through an `.odg` too. The
/// copy handed to the writer held the cells as the constants they resolve to and put a shape's own formula back only for a drawing from a VSDX file, so `Width = Height*2` was a value in the file and the
/// check named it lost. **A formula that names another shape (`Sheet.2!PinX+2`) comes back naming the same shape**, though the shape has another number when the file is read: the shapes a formula
/// names keep their `ID`s. Red on the commit this task starts from: the check names four formulas.
#[test]
fn a_formula_a_person_types_in_the_frame_or_style_cells_of_a_new_shape_comes_back_through_an_odg() {
    let dir = Dir::new();
    let mut s = pinned();
    let first = dropped(&mut s, "rectangle", (3.0, 3.0));
    let (a, b) = (dropped(&mut s, "rectangle", (5.0, 3.0)), dropped(&mut s, "rectangle", (7.0, 3.0)));
    // The first shape goes: the shape the others name is the second, whose number a read of the file would make 1.
    let page = s.document().unwrap().pages.iter().find(|p| !p.background).unwrap().id;
    run(&mut s, "shape.delete", json!({ "ids": [first], "page": page }));
    assert_eq!((a, b), (2, 3), "the numbers the formulas name");
    let typed = [
        (a, "Width", "Height*2".to_owned()),
        (a, "LineWeight", "0.01*3".to_owned()),
        (b, "PinX", format!("Sheet.{a}!PinX+2")),
        (b, "Height", format!("Sheet.{a}!Height*1.5")),
    ];
    for (id, cell, formula) in &typed {
        run(&mut s, "sheet.set_cell", json!({ "id": id, "cell": cell, "formula": formula }));
    }
    let want: Vec<_> = typed.iter().map(|(id, cell, _)| own_cell(&s, *id, cell)).collect();
    assert!(
        want.iter().zip(&typed).all(|((f, _), (_, _, formula))| f.as_deref() == Some(formula.as_str())),
        "the cells hold what was typed: {want:?}"
    );
    let said = run(&mut s, "file.save_as", json!({ "path": dir.path("typed.odg") }));
    assert_eq!(lost_formulas(&said), 0, "{}", classes(&said));
    let mut back = opened(&dir.path("typed.odg"));
    let ids = shape_ids(&back);
    assert_eq!(ids.len(), 2, "two shapes");
    // The numbers the file's shapes have when read: the formulas name the shape they named, whatever its number is.
    let got: Vec<_> = typed.iter().map(|(id, cell, _)| own_cell(&back, if *id == a { ids[0] } else { ids[1] }, cell)).collect();
    let numbers = |cells: &[(Option<String>, Option<String>)]| -> Vec<Option<f64>> {
        cells.iter().map(|(_, v)| v.as_deref().and_then(|v| v.parse::<f64>().ok())).collect()
    };
    let formulas = |cells: &[(Option<String>, Option<String>)]| cells.iter().map(|(f, _)| f.clone()).collect::<Vec<_>>();
    let named = ids[0];
    assert_eq!(named, a, "the shape a formula names keeps its number, so no formula has to be written again");
    let wanted_formulas: Vec<Option<String>> =
        formulas(&want).into_iter().map(|f| f.map(|f| f.replace(&format!("Sheet.{a}!"), &format!("Sheet.{named}!")))).collect();
    assert_eq!(formulas(&got), wanted_formulas, "the formulas, naming the shape that was named");
    for (x, y) in numbers(&got).into_iter().zip(numbers(&want)) {
        assert!(x.zip(y).is_some_and(|(x, y)| (x - y).abs() <= 1e-9 * (1.0 + y.abs())), "the values: {got:?} against {want:?}");
    }
    // And the formula is live: the shape it names moves, and the shape that names it moves with it.
    let pin = |s: &diacraft_engine::Session, id: u32| {
        let doc = s.document().unwrap();
        let page = doc.pages.iter().find(|p| !p.background).unwrap().id;
        diacraft_engine::draw::shape_frame(doc, page, id).unwrap().pin_x
    };
    let before = (pin(&back, ids[0]), pin(&back, ids[1]));
    let page = back.document().unwrap().pages.iter().find(|p| !p.background).unwrap().id;
    run(&mut back, "shape.move", json!({ "ids": [ids[0]], "page": page, "dx": 1.0, "dy": 0.0 }));
    let after = (pin(&back, ids[0]), pin(&back, ids[1]));
    assert!((after.0 - before.0 - 1.0).abs() < 1e-9 && (after.1 - before.1 - 1.0).abs() < 1e-9, "{before:?} then {after:?}");
}

/// The words of the warnings a save gave.
fn warned(said: &Value) -> Vec<String> {
    said["warnings"].as_array().map(|list| list.iter().map(|w| w.as_str().unwrap_or_default().to_owned()).collect()).unwrap_or_default()
}

/// **A drawing of plain boxes, a line and a connector says nothing of formulas** when it is saved as `.odg`: no warning, and the check names nothing. (A line's frame is worked out by the reader
/// from its ends, and a member of a group's from the group's frame: a formula a shape holds there is said, and only then.)
#[test]
fn a_plain_drawing_of_boxes_a_line_and_a_connector_saved_as_odg_says_nothing_of_formulas() {
    let dir = Dir::new();
    let mut s = pinned();
    let (a, b) = (dropped(&mut s, "rectangle", (3.0, 3.0)), dropped(&mut s, "ellipse", (7.0, 3.0)));
    run(&mut s, "connector.add", json!({ "from": { "id": a }, "to": { "id": b } }));
    let page = s.document().unwrap().pages.iter().find(|p| !p.background).unwrap().id;
    run(&mut s, "shape.draw", json!({ "page": page, "tool": "line", "points": [[1, 8], [5, 7]] }));
    run(&mut s, "shape.pin", json!({ "ids": [a], "at": "top-left" }));
    let said = run(&mut s, "file.save_as", json!({ "path": dir.path("plain.odg") }));
    assert_eq!(lost_formulas(&said), 0, "{}", classes(&said));
    assert!(warned(&said).iter().all(|w| !w.contains("formula")), "{:?}", warned(&said));
}

/// **A group of a new drawing keeps the places of its members as values, and the check names them.** The members of a group hold their place as formulas of the group (`Sheet.4!Width*0.27`), which the
/// reader works out again from the group's frame, so a file holds them as numbers. The save of a new group says nothing of it (the saves of new groups are pinned as clean: `odg_groups`).
#[test]
fn a_group_of_a_new_drawing_saved_as_odg_has_the_places_of_its_members_named_by_the_check_and_not_by_a_warning() {
    let dir = Dir::new();
    let mut s = pinned();
    let (a, b) = (dropped(&mut s, "rectangle", (3.0, 3.0)), dropped(&mut s, "ellipse", (7.0, 3.0)));
    run(&mut s, "shape.group", json!({ "ids": [a, b] }));
    let said = run(&mut s, "file.save_as", json!({ "path": dir.path("group.odg") }));
    assert_eq!(lost_formulas(&said), 8, "the check names the four cells of each member: {}", classes(&said));
    assert!(warned(&said).is_empty(), "{:?}", warned(&said));
}

/// **A fill a person gave a connector comes back**: `shape.format` on a selection that holds a line gives it the fill's cells too. The copy of a line is given its fill (the inside of a
/// hollow line end takes it), and the shape's own `RGB(r,g,b)` stays beside it with the rest of the shape's formulas: the check named a formula lost and an item that differs when the cell was left out.
#[test]
fn a_fill_a_person_gave_a_connector_comes_back_through_an_odg() {
    let dir = Dir::new();
    let mut s = pinned();
    let (a, b) = (dropped(&mut s, "rectangle", (3.0, 3.0)), dropped(&mut s, "ellipse", (7.0, 3.0)));
    let connector = id_of(&run(&mut s, "connector.add", json!({ "from": { "id": a }, "to": { "id": b } })));
    let page = s.document().unwrap().pages.iter().find(|p| !p.background).unwrap().id;
    run(&mut s, "shape.format", json!({ "ids": [connector], "page": page, "fill": "#336699" }));
    assert_eq!(own_cell(&s, connector, "FillForegnd").0.as_deref(), Some("RGB(51,102,153)"));
    let said = run(&mut s, "file.save_as", json!({ "path": dir.path("fill.odg") }));
    assert_eq!(said["checked"]["classes"], json!([]), "{}", classes(&said));
    let back = opened(&dir.path("fill.odg"));
    let ids = shape_ids(&back);
    let line = ids.iter().copied().find(|id| own_cell(&back, *id, "FillForegnd").0.is_some());
    assert!(line.is_some(), "a shape of the file holds the fill's formula: {ids:?}");
}
