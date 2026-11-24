//! A page's layers and the shapes on them, through `Session::execute` as the control channel, the CLI and MCP call it. The rules are the specs'
//! and the
//! decisions in `docs/editing-commands.md`, "Layers".

use diacraft_doc::{CellAddr, Document, Lookup, Owner, RowKey, SectionKey};
use diacraft_engine::Session;
use serde_json::{Value as Json, json};

use crate::edit::{doc, drop_shape};
use crate::{bad_params_msg, blank_session, check_session, run, snapshot};

/// The layers `layer.list` gives for page 1.
pub(crate) fn listed(s: &mut Session) -> Vec<Json> {
    run(s, "layer.list", json!({}))["layers"].as_array().unwrap().clone()
}

fn names(s: &mut Session) -> Vec<String> {
    listed(s).iter().map(|l| l["name"].as_str().unwrap().to_owned()).collect()
}

/// The layer `name`, as `layer.list` gives it.
fn layer(s: &mut Session, name: &str) -> Json {
    listed(s).into_iter().find(|l| l["name"] == name).unwrap_or_else(|| panic!("no layer {name}"))
}

/// What a cell of the page's `Layer` section row `ix` holds as text, if the page has it.
fn row_cell(d: &Document, ix: u32, cell: &str) -> Option<String> {
    let page = d.page(1)?;
    let section = page.sheet.section(&SectionKey::new("Layer", None))?;
    let row = section.row(&RowKey::Ix(ix))?;
    row.cells.get(cell).and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
}

/// The `LayerMember` text of shape `id` of page 1: `None` for a shape with no such cell.
pub(crate) fn member_text(d: &Document, id: u32) -> Option<String> {
    let at = d.find_shape(Owner::Page(1), id)?;
    let shape = d.shape(&at)?;
    match shape.sheet.lookup(&CellAddr::direct("LayerMember")) {
        Lookup::Found(c) => c.value.as_ref().map(|v| v.text().to_owned()),
        _ => None,
    }
}

fn on(s: &mut Session, ids: &[u32], layers: Json) -> Json {
    run(s, "layer.assign", json!({"ids": ids, "layers": layers}))
}

fn msg(s: &mut Session, id: &str, params: Json) -> String {
    let e = s.execute(id, &params).expect_err("the call is refused");
    bad_params_msg(&e).to_owned()
}

#[test]
fn a_new_layer_is_a_row_with_the_cells_a_new_layer_has() {
    let mut s = blank_session();
    assert_eq!(listed(&mut s).len(), 0, "a new page has no layers");
    let made = run(&mut s, "layer.add", json!({"name": "Notes"}));
    assert_eq!(made["id"], 0);
    assert_eq!(made["name"], "Notes");
    let d = doc(&s);
    // All eleven cells, as the files hold them.
    for (cell, want) in [
        ("Name", "Notes"),
        ("NameUniv", "Notes"),
        ("Visible", "1"),
        ("Print", "1"),
        ("Active", "0"),
        ("Lock", "0"),
        ("Snap", "1"),
        ("Glue", "1"),
        ("Color", "255"),
        ("ColorTrans", "0"),
        ("Status", "0"),
    ] {
        assert_eq!(row_cell(d, 0, cell).as_deref(), Some(want), "{cell}");
    }
    let second = run(&mut s, "layer.add", json!({"name": "Guides", "visible": false, "lock": true, "color": "#ff0000", "transparency": 0.25}));
    assert_eq!(second["id"], 1);
    let l = layer(&mut s, "Guides");
    assert_eq!((l["visible"].clone(), l["lock"].clone(), l["print"].clone()), (json!(false), json!(true), json!(true)));
    assert_eq!(l["color"], "#ff0000");
    assert_eq!(l["transparency"], 0.25);
    assert_eq!(layer(&mut s, "Notes")["color"], Json::Null, "255 is no colour");
    check_session(&s).unwrap();
}

#[test]
fn a_layer_is_added_to_the_page_asked_and_to_no_other() {
    let mut s = blank_session();
    run(&mut s, "page.add", json!({"name": "Second"}));
    run(&mut s, "layer.add", json!({"name": "OnTwo", "page": 2}));
    assert!(listed(&mut s).is_empty(), "page 1 has none (a new layer reaches no other page)");
    assert_eq!(run(&mut s, "layer.list", json!({"page": 2}))["layers"].as_array().unwrap().len(), 1);
    let e = s.execute("layer.add", &json!({"name": "x", "page": 9})).unwrap_err();
    assert!(bad_params_msg(&e).contains("no page 9"), "{e}");
}

#[test]
fn a_name_that_cannot_be_a_layers_is_refused_in_words_and_nothing_changes() {
    let mut s = blank_session();
    run(&mut s, "layer.add", json!({"name": "Notes"}));
    let before = snapshot(&s);
    for (name, why) in [
        (json!(""), "empty"),
        (json!("   "), "empty"),
        (json!("Notes"), "already"),
        (json!("a\u{1}b"), "control"),
        (json!("x".repeat(300)), "long"),
        (json!("layout"), "reserves"),
        (json!("measurelines"), "reserves"),
    ] {
        let e = s.execute("layer.add", &json!({"name": name})).unwrap_err();
        assert!(bad_params_msg(&e).contains(why), "{name}: {e}");
    }
    assert_eq!(snapshot(&s), before, "a refusal changes nothing, not even the history");
}

#[test]
fn adding_a_layer_is_one_undo_step() {
    let mut s = blank_session();
    run(&mut s, "layer.add", json!({"name": "Notes"}));
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Add layer"));
    run(&mut s, "edit.undo", json!({}));
    assert!(listed(&mut s).is_empty());
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(names(&mut s), ["Notes"]);
}

#[test]
fn layer_set_changes_the_flags_and_the_name_and_leaves_the_shapes_on_the_layer() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 3.0);
    run(&mut s, "layer.add", json!({"name": "Notes"}));
    on(&mut s, &[a], json!([0]));
    let changed = run(
        &mut s,
        "layer.set",
        json!({"layer": 0, "visible": false, "print": false, "snap": false, "glue": false, "color": "#00ff7f", "transparency": 0.5}),
    );
    assert_eq!(changed["id"], 0);
    let l = layer(&mut s, "Notes");
    for (key, want) in [("visible", false), ("print", false), ("snap", false), ("glue", false)] {
        assert_eq!(l[key], want, "{key}");
    }
    assert_eq!((l["color"].clone(), l["transparency"].clone()), (json!("#00ff7f"), json!(0.5)));
    assert_eq!(row_cell(doc(&s), 0, "Visible").as_deref(), Some("0"));
    // The name is found by name as well, and a rename leaves the shape on it.
    run(&mut s, "layer.set", json!({"layer": "Notes", "name": "Remarks"}));
    assert_eq!(names(&mut s), ["Remarks"]);
    assert_eq!(member_text(doc(&s), a).as_deref(), Some("0"));
    assert_eq!(layer(&mut s, "Remarks")["shapes"], 1);
    assert_eq!(row_cell(doc(&s), 0, "NameUniv").as_deref(), Some("Remarks"), "the universal name follows a rename the dialog makes");
    // `color: "none"` is 255.
    run(&mut s, "layer.set", json!({"layer": 0, "color": "none"}));
    assert_eq!(row_cell(doc(&s), 0, "Color").as_deref(), Some("255"));
    assert_eq!(layer(&mut s, "Remarks")["color"], Json::Null);
}

#[test]
fn layer_set_refuses_what_cannot_be_and_changes_nothing() {
    let mut s = blank_session();
    run(&mut s, "layer.add", json!({"name": "A"}));
    run(&mut s, "layer.add", json!({"name": "B", "lock": true}));
    let before = snapshot(&s);
    for (params, why) in [
        (json!({"layer": 5, "visible": false}), "no layer"),
        (json!({"layer": "Zed", "visible": false}), "no layer"),
        (json!({"layer": 0}), "nothing"),
        (json!({"layer": 0, "name": "B"}), "already"),
        (json!({"layer": 0, "name": ""}), "empty"),
        (json!({"layer": 0, "color": "red"}), "colour"),
        (json!({"layer": 0, "color": "#12345"}), "colour"),
        (json!({"layer": 0, "transparency": 1.5}), "transparency"),
        (json!({"layer": 0, "transparency": -0.1}), "transparency"),
        (json!({"layer": 1, "active": true}), "locked"),
        (json!({"layer": 0, "visible": "yes"}), "true or false"),
    ] {
        let e = s.execute("layer.set", &params).unwrap_err();
        assert!(bad_params_msg(&e).contains(why), "{params}: {e}");
    }
    assert_eq!(snapshot(&s), before);
}

#[test]
fn a_locked_layer_cannot_be_active_so_locking_an_active_one_makes_it_inactive() {
    let mut s = blank_session();
    run(&mut s, "layer.add", json!({"name": "A", "active": true}));
    assert_eq!(layer(&mut s, "A")["active"], true);
    let said = run(&mut s, "layer.set", json!({"layer": "A", "lock": true}));
    assert_eq!(said["deactivated"], true);
    let l = layer(&mut s, "A");
    assert_eq!((l["lock"].clone(), l["active"].clone()), (json!(true), json!(false)));
    assert!(msg(&mut s, "layer.set", json!({"layer": "A", "active": true})).contains("locked"));
    // One call that locks and activates is the same refusal.
    run(&mut s, "layer.set", json!({"layer": "A", "lock": false}));
    assert!(msg(&mut s, "layer.set", json!({"layer": "A", "lock": true, "active": true})).contains("locked"));
}

#[test]
fn shapes_are_assigned_to_layers_by_a_list_that_is_a_set() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 3.0);
    let b = drop_shape(&mut s, "rectangle", 4.0, 3.0);
    for n in ["Zero", "One", "Two"] {
        run(&mut s, "layer.add", json!({"name": n}));
    }
    // Replace, the default (the Layer dialog's ticks are the shape's whole list).
    let said = on(&mut s, &[a, b], json!([2, 0]));
    assert_eq!(said["shapes"], 2);
    assert_eq!(member_text(doc(&s), a).as_deref(), Some("0;2"), "a list is kept in rising order and holds a number once");
    // By name, and `add` and `remove` change part of the list.
    run(&mut s, "layer.assign", json!({"ids": [a], "layers": ["One", 0], "mode": "add"}));
    assert_eq!(member_text(doc(&s), a).as_deref(), Some("0;1;2"));
    run(&mut s, "layer.assign", json!({"ids": [a], "layers": ["Zero"], "mode": "remove"}));
    assert_eq!(member_text(doc(&s), a).as_deref(), Some("1;2"));
    assert_eq!(member_text(doc(&s), b).as_deref(), Some("0;2"), "the other shape is not touched");
    let counts: Vec<(String, u64)> = listed(&mut s).iter().map(|l| (l["name"].as_str().unwrap().to_owned(), l["shapes"].as_u64().unwrap())).collect();
    assert_eq!(counts, [("Zero".to_owned(), 1), ("One".to_owned(), 1), ("Two".to_owned(), 2)]);
    // None: the shape is on no layer (an empty cell, which stops a master's list too).
    on(&mut s, &[a], json!([]));
    assert_eq!(member_text(doc(&s), a).as_deref(), Some(""));
    assert_eq!(layer(&mut s, "One")["shapes"], 0);
}

#[test]
fn assigning_is_refused_for_a_layer_or_shape_that_is_not_there_and_for_a_locked_layer() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 3.0);
    run(&mut s, "layer.add", json!({"name": "Open"}));
    run(&mut s, "layer.add", json!({"name": "Locked", "lock": true}));
    let before = snapshot(&s);
    assert!(msg(&mut s, "layer.assign", json!({"ids": [a], "layers": [7]})).contains("no layer 7"));
    assert!(msg(&mut s, "layer.assign", json!({"ids": [a], "layers": ["Nope"]})).contains("no layer"));
    assert!(msg(&mut s, "layer.assign", json!({"ids": [99], "layers": [0]})).contains("no shape 99"));
    assert!(msg(&mut s, "layer.assign", json!({"ids": [a], "layers": [1]})).contains("locked"), "a locked layer takes no new members");
    assert!(msg(&mut s, "layer.assign", json!({"ids": [a], "layers": [0], "mode": "sideways"})).contains("mode"));
    assert_eq!(snapshot(&s), before);
    // Taking a shape off a locked layer is allowed; a shape on a locked layer is not edited (it cannot be selected), so a call about it is refused.
    run(&mut s, "layer.set", json!({"layer": 1, "lock": false}));
    on(&mut s, &[a], json!([1]));
    run(&mut s, "layer.set", json!({"layer": 1, "lock": true}));
    assert!(msg(&mut s, "layer.assign", json!({"ids": [a], "layers": [0]})).contains("locked layer"));
}

#[test]
fn a_group_keeps_its_members_layers_unless_asked_to_replace_them() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 3.0);
    let b = drop_shape(&mut s, "rectangle", 4.0, 3.0);
    for n in ["Zero", "One"] {
        run(&mut s, "layer.add", json!({"name": n}));
    }
    on(&mut s, &[a], json!([0]));
    let g = run(&mut s, "shape.group", json!({"ids": [a, b]}))["id"].as_u64().unwrap() as u32;
    // The default keeps what the members hold (the "Preserve group member layers" box, ticked).
    on(&mut s, &[g], json!([1]));
    assert_eq!(member_text(doc(&s), g).as_deref(), Some("1"));
    assert_eq!(member_text(doc(&s), a).as_deref(), Some("0"));
    assert_eq!(member_text(doc(&s), b), None);
    // Not preserving puts the list on every member, at any depth.
    run(&mut s, "layer.assign", json!({"ids": [g], "layers": [1], "mode": "add", "preserve_members": false}));
    assert_eq!(member_text(doc(&s), a).as_deref(), Some("0;1"));
    assert_eq!(member_text(doc(&s), b).as_deref(), Some("1"));
    assert_eq!(layer(&mut s, "One")["shapes"], 3, "the group and both members are on it");
}

#[test]
fn removing_a_layer_closes_the_numbers_and_every_list_follows() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 1.0, 3.0);
    let b = drop_shape(&mut s, "rectangle", 2.0, 3.0);
    let c = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let d = drop_shape(&mut s, "rectangle", 4.0, 3.0);
    for n in ["Zero", "One", "Two"] {
        run(&mut s, "layer.add", json!({"name": n}));
    }
    on(&mut s, &[a], json!([0]));
    on(&mut s, &[b], json!([1]));
    on(&mut s, &[c], json!([1, 2]));
    on(&mut s, &[d], json!([0, 2]));
    let said = run(&mut s, "layer.delete", json!({"layer": "One"}));
    assert_eq!(said["removed"], json!([1]));
    assert_eq!(said["shapes_deleted"], 0);
    assert_eq!(names(&mut s), ["Zero", "Two"]);
    assert_eq!(layer(&mut s, "Two")["id"], 1, "the rows after it close up");
    // The shape on One alone is on no layer; the one on One and Two is on Two, now number 1; the others keep their layers.
    assert_eq!(member_text(doc(&s), a).as_deref(), Some("0"));
    assert_eq!(member_text(doc(&s), b).as_deref(), Some(""));
    assert_eq!(member_text(doc(&s), c).as_deref(), Some("1"));
    assert_eq!(member_text(doc(&s), d).as_deref(), Some("0;1"));
    // One undo step puts it all back.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(names(&mut s), ["Zero", "One", "Two"]);
    assert_eq!(member_text(doc(&s), c).as_deref(), Some("1;2"));
}

#[test]
fn removing_a_layer_can_delete_the_shapes_only_on_it() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 1.0, 3.0);
    let b = drop_shape(&mut s, "rectangle", 2.0, 3.0);
    let c = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    for n in ["Zero", "One"] {
        run(&mut s, "layer.add", json!({"name": n}));
    }
    on(&mut s, &[a], json!([1]));
    on(&mut s, &[b], json!([0, 1]));
    let said = run(&mut s, "layer.delete", json!({"layer": 1, "shapes": "delete"}));
    assert_eq!(said["shapes_deleted"], 1);
    let d = doc(&s);
    assert!(d.find_shape(Owner::Page(1), a).is_none(), "the shape only on the layer goes");
    assert!(d.find_shape(Owner::Page(1), b).is_some(), "the shape on another layer too stays");
    assert!(d.find_shape(Owner::Page(1), c).is_some());
    assert_eq!(member_text(d, b).as_deref(), Some("0"));
    assert!(msg(&mut s, "layer.delete", json!({"layer": 0, "shapes": "everything"})).contains("shapes"));
}

#[test]
fn unreferenced_layers_are_removed_together_and_the_rest_close_up() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 1.0, 3.0);
    for n in ["Empty1", "Used", "Empty2", "Used2"] {
        run(&mut s, "layer.add", json!({"name": n}));
    }
    on(&mut s, &[a], json!([1, 3]));
    let said = run(&mut s, "layer.delete", json!({"unreferenced": true}));
    assert_eq!(said["removed"], json!([0, 2]));
    assert_eq!(names(&mut s), ["Used", "Used2"]);
    assert_eq!(member_text(doc(&s), a).as_deref(), Some("0;1"));
    let nothing = run(&mut s, "layer.delete", json!({"unreferenced": true}));
    assert_eq!(nothing["removed"], json!([]));
    assert!(msg(&mut s, "layer.delete", json!({})).contains("layer"));
}

#[test]
fn a_shape_added_with_layers_active_joins_them_and_a_locked_one_is_never_joined() {
    let mut s = blank_session();
    run(&mut s, "layer.add", json!({"name": "A", "active": true}));
    run(&mut s, "layer.add", json!({"name": "B"}));
    run(&mut s, "layer.add", json!({"name": "C", "active": true}));
    run(&mut s, "layer.add", json!({"name": "D", "lock": true}));
    let dropped = drop_shape(&mut s, "rectangle", 2.0, 3.0);
    assert_eq!(member_text(doc(&s), dropped).as_deref(), Some("0;2"), "every active layer");
    let line = run(&mut s, "connector.add", json!({"from": {"x": 1.0, "y": 1.0}, "to": {"x": 3.0, "y": 1.0}}))["id"].as_u64().unwrap() as u32;
    assert_eq!(member_text(doc(&s), line).as_deref(), Some("0;2"), "a connector too");
    let boxed = run(&mut s, "text.box", json!({"page": 1, "x": 5.0, "y": 5.0, "width": 2.0, "height": 1.0}))["id"].as_u64().unwrap() as u32;
    assert_eq!(member_text(doc(&s), boxed).as_deref(), Some("0;2"), "and a text box");
    // A layer locked while active is no longer joined.
    run(&mut s, "layer.set", json!({"layer": "A", "lock": true}));
    let later = drop_shape(&mut s, "rectangle", 6.0, 3.0);
    assert_eq!(member_text(doc(&s), later).as_deref(), Some("2"));
    // With none active a new shape holds no cell of the sort.
    run(&mut s, "layer.set", json!({"layer": "C", "active": false}));
    let plain = drop_shape(&mut s, "rectangle", 7.0, 3.0);
    assert_eq!(member_text(doc(&s), plain), None);
}

#[test]
fn a_layer_works_on_the_page_of_its_shapes_and_not_the_first() {
    let mut s = blank_session();
    run(&mut s, "page.add", json!({"name": "Second"}));
    let on_second = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 2, "x": 2.0, "y": 3.0}))["id"].as_u64().unwrap() as u32;
    run(&mut s, "layer.add", json!({"name": "Only on 2", "page": 2}));
    // A call that names a shape of page 2 finds the page by it.
    run(&mut s, "layer.assign", json!({"ids": [on_second], "layers": [0]}));
    let d = doc(&s);
    let at = d.find_shape(Owner::Page(2), on_second).unwrap();
    assert_eq!(d.shape(&at).unwrap().sheet.cell("LayerMember").and_then(|c| c.value.as_ref()).map(|v| v.text()), Some("0"));
}

#[test]
fn layer_calls_are_in_the_registry_with_their_words() {
    let s = blank_session();
    let listed = s.commands();
    for id in ["layer.add", "layer.set", "layer.assign", "layer.delete", "layer.list"] {
        let c = listed.iter().find(|c| c.id == id).unwrap_or_else(|| panic!("{id} is not a command"));
        assert!(c.enabled, "{id}: {:?}", c.disabled_reason);
        assert!(c.menu.iter().any(|m| m == "Layers") || id == "layer.list", "{id}: {:?}", c.menu);
    }
}

#[test]
fn a_hostile_page_of_layers_is_met_with_a_refusal_in_words() {
    use std::sync::Arc;

    use diacraft_doc::{Cell, CellName, Row, Section, Value};

    let mut s = blank_session();
    // A page holding the most layers it can: the rows are put in the model directly (a command reads the table once for each layer it makes).
    s.edit("fill", |d| {
        let page = Arc::make_mut(d.pages.get_mut(0).unwrap());
        let sheet = Arc::make_mut(&mut page.sheet);
        let mut section = Section::new("Layer", None);
        for k in 0..4_096u32 {
            let mut row = Row { ix: Some(k), ..Row::default() };
            row.cells.insert(CellName::new("Name"), Cell::constant(Value::from_text(&format!("L{k}"))));
            section.rows.push(Arc::new(row));
        }
        sheet.sections.push(Arc::new(section));
        Ok(())
    })
    .unwrap();
    let e = s.execute("layer.add", &json!({"name": "One more"})).unwrap_err();
    assert!(e.to_string().contains("at most 4096 layers"), "{e}");
    // A table past the cap is refused when read, in words, and nothing hangs.
    s.edit("fill", |d| {
        let page = Arc::make_mut(d.pages.get_mut(0).unwrap());
        let sheet = Arc::make_mut(&mut page.sheet);
        let at = sheet.sections.iter().position(|sec| sec.name.as_str() == "Layer").unwrap();
        let section = Arc::make_mut(sheet.sections.get_mut(at).unwrap());
        let mut row = Row { ix: Some(4_096), ..Row::default() };
        row.cells.insert(CellName::new("Name"), Cell::constant(Value::from_text("Over")));
        section.rows.push(Arc::new(row));
        Ok(())
    })
    .unwrap();
    let e = s.execute("layer.list", &json!({})).unwrap_err();
    assert!(e.to_string().contains("4097 layers"), "{e}");
    check_session(&s).unwrap();
}

/// The `PinX` of shape `id` of page 1, in inches.
fn pin_x(s: &Session, id: u32) -> f64 {
    let at = doc(s).find_shape(Owner::Page(1), id).unwrap();
    doc(s).shape(&at).unwrap().sheet.cell("PinX").and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap()
}

#[test]
fn a_layer_with_snap_clear_is_no_target_for_a_shape_moved_to_it_and_a_shape_on_no_layer_still_is() {
    // Only the handles of shapes are targets. A is 1.5 wide with its right side at 3.75; B is moved so that its left side is 0.04 past it, which the pull of the handles puts right.
    let drive = |layer: Option<(&str, bool)>| -> f64 {
        let mut s = blank_session();
        run(
            &mut s,
            "snap.set",
            json!({"ruler": false, "grid": false, "alignment_box": false, "intersections": false, "guides": false, "snap_geometry": false, "snap_handles": true,
                   "snap_vertices": false, "snap_connection_points": false, "dynamic_grid": false}),
        );
        let a = drop_shape(&mut s, "rectangle", 3.0, 3.0);
        let b = drop_shape(&mut s, "rectangle", 7.0, 3.0);
        if let Some((name, snap)) = layer {
            run(&mut s, "layer.add", json!({"name": name, "snap": snap}));
            on(&mut s, &[a], json!([name]));
        }
        run(&mut s, "shape.move", json!({"ids": [b], "dx": 3.75 - 6.25 + 0.04, "dy": 0.0, "snap": true}));
        pin_x(&s, b)
    };
    let pulled = drive(None);
    assert!((pulled - 4.5).abs() < 1.0e-6, "the handles of A pull B's side to it: {pulled}");
    assert!((drive(Some(("Open", true))) - 4.5).abs() < 1.0e-6, "a layer that allows snapping is as no layer");
    let free = drive(Some(("Quiet", false)));
    assert!((free - 4.54).abs() < 1.0e-6, "a layer with Snap clear is no target: {free}");
}

/// How much of the first sheet of a print of the drawing is ink: the pixels that are not clear.
fn ink_on_paper(s: &Session) -> usize {
    let planned = diacraft_engine::print::plan(doc(s), diacraft_engine::print::Pages::All).unwrap();
    let (_, sheet) = planned.sheet(0).expect("a sheet");
    let drawn = diacraft_render::Renderer::new().render_sheet(doc(s), sheet, 20.0);
    drawn.rgba_premul.chunks(4).filter(|px| px.get(3).is_some_and(|a| *a != 0)).count()
}

#[test]
fn a_layer_that_does_not_print_leaves_its_shapes_off_the_paper_and_a_hidden_one_that_prints_is_on_it() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 4.0, 4.0);
    let with = ink_on_paper(&s);
    assert!(with > 0, "a shape on no layer is on the paper");
    run(&mut s, "layer.add", json!({"name": "Screen only", "print": false}));
    run(&mut s, "layer.add", json!({"name": "Paper only", "visible": false}));
    on(&mut s, &[a], json!(["Screen only"]));
    assert_eq!(ink_on_paper(&s), 0, "Print 0: on the screen and off the paper");
    on(&mut s, &[a], json!(["Paper only"]));
    assert_eq!(ink_on_paper(&s), with, "Visible 0 and Print 1: off the screen and on the paper");
    on(&mut s, &[a], json!(["Screen only", "Paper only"]));
    assert_eq!(ink_on_paper(&s), with, "one printing layer among those a shape is on is enough");
}

#[test]
fn a_name_in_a_refusal_is_shown_with_its_control_characters_escaped() {
    let mut s = blank_session();
    for (call, extra) in [("layer.set", json!({"visible": false})), ("layer.delete", json!({}))] {
        let mut params = extra;
        params["layer"] = json!("nul\u{0}byte\nlines");
        let words = msg(&mut s, call, params);
        assert!(!words.chars().any(char::is_control), "{call}: {words:?}");
        assert!(words.contains("no layer named"), "{words}");
    }
}

#[test]
fn the_layers_and_the_selection_by_type_read_one_table_and_one_list_the_same_way() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 3.0);
    let b = drop_shape(&mut s, "rectangle", 5.0, 3.0);
    for n in ["Zero", "One", "Two"] {
        run(&mut s, "layer.add", json!({"name": n}));
    }
    on(&mut s, &[a], json!([0, 2]));
    on(&mut s, &[b], json!([2]));
    run(&mut s, "layer.set", json!({"layer": "Two", "lock": true}));
    let table = listed(&mut s);
    let choices = run(&mut s, "select.choices", json!({}));
    let offered = choices["layers"].as_array().unwrap();
    // The same rows, in the same order, under the same numbers and names, with the same counts of shapes and the same locks.
    assert_eq!(offered.len(), table.len());
    for (one, other) in table.iter().zip(offered) {
        assert_eq!(
            (one["id"].clone(), one["name"].clone(), one["shapes"].clone(), one["lock"].clone()),
            (other["ix"].clone(), other["name"].clone(), other["count"].clone(), other["locked"].clone())
        );
    }
    // A shape on a locked layer is taken by neither a type nor a region, and the pointer's rule (`layers::page_standing`) says the same of it.
    let by_type = run(&mut s, "select.by_type", json!({"types": ["shape"]}));
    assert_eq!(by_type["ids"], json!([]), "both shapes are on the locked layer");
    assert_eq!(by_type["locked"], 2);
    let standing = diacraft_engine::draw::layers::page_standing(doc(&s), 1).unwrap();
    assert!(standing.get(&a).is_some_and(|st| st.locked) && standing.get(&b).is_some_and(|st| st.locked));
}

#[test]
fn a_shape_only_on_hidden_layers_is_taken_by_neither_a_type_nor_a_region_and_one_on_a_shown_layer_too_is() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 3.0);
    let b = drop_shape(&mut s, "rectangle", 5.0, 3.0);
    let c = drop_shape(&mut s, "rectangle", 8.0, 3.0);
    for (name, visible) in [("Shown", true), ("Gone", false)] {
        run(&mut s, "layer.add", json!({"name": name, "visible": visible}));
    }
    on(&mut s, &[a], json!(["Gone"]));
    on(&mut s, &[b], json!(["Shown", "Gone"]));
    let by_type = run(&mut s, "select.by_type", json!({"types": ["shape"]}));
    assert_eq!(by_type["ids"], json!([b, c]), "{by_type}");
    let by_region = run(&mut s, "select.by_region", json!({"rect": [0.0, 0.0, 10.0, 6.0]}));
    assert_eq!(by_region["ids"], json!([b, c]), "{by_region}");
    assert!(by_type.get("locked").is_none_or(|n| n == 0), "a hidden shape is not counted as a locked one: {by_type}");
}
