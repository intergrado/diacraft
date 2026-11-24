//! Quick Shapes, the masters above a stencil's divider, and the shapes the AutoConnect flyout offers from them.
//! The choice is the drawing's: no undo step, and kept by every save in DiaCraft's own data.

use diacraft_doc::Document;
use diacraft_engine::Session;
use diacraft_testkit::package::{MasterSpec, VsdxBuilder, shape};
use diacraft_vsdx::{WriteOptions, write_with};
use serde_json::{Value, json};

use crate::native_common::{Dir, PINNED};
use crate::stencil_open::{CT_STENCIL, Dir as StencilDir, box_cells, ids, line_master, open};
use crate::{blank_session, check_session, run, snapshot};

fn names(group: &Value) -> Vec<String> {
    group["masters"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap().to_owned()).collect()
}

fn offer(v: &Value) -> Vec<String> {
    v["offer"]["masters"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap().to_owned()).collect()
}

/// The group of one stencil as `stencil.quick` has it.
fn group(s: &mut Session, stencil: &str) -> Value {
    run(s, "stencil.quick", json!({ "stencil": stencil }))["stencils"][0].clone()
}

fn flowchart_file(s: &mut Session, dir: &StencilDir) -> String {
    let mut builder = VsdxBuilder::new().document_type(CT_STENCIL);
    for (id, name) in [(1, "Alpha"), (2, "Bravo"), (3, "Charlie"), (4, "Delta"), (5, "Echo"), (6, "Foxtrot")] {
        builder = builder.master(MasterSpec::new(id, name).attrs(&ids(id, id + 100)).shapes(&shape(5, &box_cells(1.0, 1.0))));
    }
    let mut link = line_master();
    link.id = "7".to_owned();
    builder = builder.master(link);
    let path = dir.write("nato.vssx", &builder.build());
    open(s, &path)["id"].as_str().unwrap().to_owned()
}

/// A stencil that nothing was set for has its first four masters that are not connectors, and the strip offers them.
#[test]
fn a_stencil_with_nothing_set_has_its_first_four_masters_and_the_strip_offers_them() {
    let mut s = blank_session();
    let v = run(&mut s, "stencil.quick", json!({ "stencil": "basic" }));
    assert_eq!(names(&v["stencils"][0]), ["Rectangle", "Square", "Ellipse", "Circle"], "{v}");
    assert_eq!(v["stencils"][0]["customized"], false);
    assert_eq!(v["readable"], true);
    assert_eq!(offer(&v), ["Rectangle", "Square", "Ellipse", "Circle"]);
    assert_eq!(v["offer"]["stencil"], "basic");
    // A stencil with a connector among its first masters: the connector is not a Quick Shape by default.
    let gs = run(&mut s, "stencil.quick", json!({ "stencil": "gane-sarson" }));
    assert_eq!(names(&gs["stencils"][0]), ["Interface", "Process", "Data Store"], "the Data Flow connector is left out: {gs}");
    // Any case, with space round it.
    assert_eq!(group(&mut s, " Flowchart ")["stencil"], "flowchart");
}

/// The Quick Shapes stencil gathers the group of every open stencil in the order the window lists them, and the strip offers the first four of them.
#[test]
fn the_quick_shapes_stencil_gathers_the_groups_of_every_open_stencil() {
    let mut s = blank_session();
    let dir = StencilDir::new();
    flowchart_file(&mut s, &dir);
    for params in [json!({}), json!({ "stencil": "quick" }), json!({ "stencil": "Quick" })] {
        let v = run(&mut s, "stencil.quick", params);
        let ids: Vec<&str> = v["stencils"].as_array().unwrap().iter().map(|g| g["stencil"].as_str().unwrap()).collect();
        assert_eq!(ids, ["basic", "flowchart", "decorative", "arrows", "gane-sarson", "cross-functional", "orgchart", "file-1"], "{v}");
        assert_eq!(v["offer"]["stencil"], "quick");
        assert_eq!(offer(&v), ["Rectangle", "Square", "Ellipse", "Circle"]);
    }
    // The file's group: its first four masters that are not connectors, each told which stencil it is in (a drop takes it).
    let v = run(&mut s, "stencil.quick", json!({}));
    let file = v["stencils"].as_array().unwrap().last().unwrap().clone();
    assert_eq!((file["name"].as_str(), file["builtin"].as_bool()), (Some("nato"), Some(false)));
    assert_eq!(names(&file), ["Alpha", "Bravo", "Charlie", "Delta"]);
    assert_eq!(file["masters"][0]["stencil"], "file-1");
    // A template that opens fewer stencils gathers fewer.
    run(&mut s, "file.new", json!({ "template": "Basic Diagram" }));
    let few = run(&mut s, "stencil.quick", json!({}));
    let ids: Vec<&str> = few["stencils"].as_array().unwrap().iter().map(|g| g["stencil"].as_str().unwrap()).collect();
    assert!(!ids.contains(&"flowchart"), "{ids:?}");
}

#[test]
fn a_master_is_added_to_the_quick_shapes_moved_and_taken_away() {
    let mut s = blank_session();
    let v = run(&mut s, "stencil.add_quick", json!({ "stencil": "basic", "master": "hexagon" }));
    assert_eq!(names(&v), ["Rectangle", "Square", "Ellipse", "Circle", "Hexagon"], "{v}");
    assert_eq!((v["customized"].as_bool(), v["changed"].as_bool()), (Some(true), Some(true)));
    // The strip offers the first four: the one added last is not among them until it is moved up.
    assert_eq!(offer(&run(&mut s, "stencil.quick", json!({ "stencil": "basic" }))), ["Rectangle", "Square", "Ellipse", "Circle"]);
    let v = run(&mut s, "stencil.add_quick", json!({ "stencil": "basic", "master": "Hexagon", "at": 0 }));
    assert_eq!(names(&v), ["Hexagon", "Rectangle", "Square", "Ellipse", "Circle"], "a master in the group is moved");
    assert_eq!(offer(&run(&mut s, "stencil.quick", json!({ "stencil": "basic" }))), ["Hexagon", "Rectangle", "Square", "Ellipse"]);
    // A place past the end is the end, and the same place again changes nothing.
    let v = run(&mut s, "stencil.add_quick", json!({ "stencil": "basic", "master": "Hexagon", "at": 99 }));
    assert_eq!(names(&v), ["Rectangle", "Square", "Ellipse", "Circle", "Hexagon"]);
    let again = run(&mut s, "stencil.add_quick", json!({ "stencil": "basic", "master": "hexagon" }));
    assert_eq!(again["changed"], false, "{again}");
    // Taken away: below the divider again.
    let v = run(&mut s, "stencil.remove_quick", json!({ "stencil": "basic", "master": "rectangle" }));
    assert_eq!(names(&v), ["Square", "Ellipse", "Circle", "Hexagon"]);
    assert_eq!(v["changed"], true);
    let v = run(&mut s, "stencil.remove_quick", json!({ "stencil": "basic", "master": "Rectangle" }));
    assert_eq!(v["changed"], false, "it was not in the group");
    // Every master taken away is an empty group that the drawing set, and the strip still offers the stencil's first four.
    for m in ["Square", "Ellipse", "Circle", "Hexagon"] {
        run(&mut s, "stencil.remove_quick", json!({ "stencil": "basic", "master": m }));
    }
    let empty = run(&mut s, "stencil.quick", json!({ "stencil": "basic" }));
    assert!(names(&empty["stencils"][0]).is_empty(), "{empty}");
    assert_eq!(empty["stencils"][0]["customized"], true);
    assert_eq!(offer(&empty), ["Rectangle", "Square", "Ellipse", "Circle"], "a stencil is never one the arrows cannot use");
    // Reset Stencil gives it its own back, once.
    let v = run(&mut s, "stencil.reset", json!({ "stencil": "basic" }));
    assert_eq!(names(&v), ["Rectangle", "Square", "Ellipse", "Circle"]);
    assert_eq!((v["customized"].as_bool(), v["changed"].as_bool()), (Some(false), Some(true)));
    assert_eq!(run(&mut s, "stencil.reset", json!({ "stencil": "basic" }))["changed"], false, "nothing was set");
    assert!(s.document().unwrap().diacraft.is_none(), "a drawing whose groups are all its stencils' own holds nothing of them");
}

#[test]
fn the_strip_offers_no_connector_even_when_one_is_in_the_group() {
    let mut s = blank_session();
    run(&mut s, "stencil.add_quick", json!({ "stencil": "gane-sarson", "master": "data-flow", "at": 0 }));
    let v = run(&mut s, "stencil.quick", json!({ "stencil": "gane-sarson" }));
    assert_eq!(names(&v["stencils"][0]), ["Data Flow", "Interface", "Process", "Data Store"]);
    assert_eq!(offer(&v), ["Interface", "Process", "Data Store"], "{v}");
}

#[test]
fn the_choice_is_a_setting_no_undo_step_and_undo_and_redo_leave_it_alone() {
    let mut s = blank_session();
    let revision = s.revision();
    run(&mut s, "stencil.add_quick", json!({ "stencil": "arrows", "master": "arrows.bent", "at": 0 }));
    assert!(s.is_dirty(), "the drawing is unsaved");
    assert_eq!(s.history().unwrap().undo_len(), 0, "and the choice is no undo step");
    assert_eq!(s.revision(), revision, "no revision moves: the page is not read again");
    // A drop is an edit, and undoing it leaves the choice.
    run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 1, "x": 3, "y": 3 }));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(names(&group(&mut s, "arrows"))[0], "Bent Arrow");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(names(&group(&mut s, "arrows"))[0], "Bent Arrow", "redo too");
    // A choice made between two edits is not taken back by undoing the first.
    run(&mut s, "stencil.add_quick", json!({ "stencil": "arrows", "master": "arrows.up", "at": 0 }));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(names(&group(&mut s, "arrows"))[..2], ["Up Arrow", "Bent Arrow"]);
    check_session(&s).unwrap();
}

#[test]
fn a_stencil_file_has_its_own_group_by_the_masters_ids() {
    let mut s = blank_session();
    let dir = StencilDir::new();
    let id = flowchart_file(&mut s, &dir);
    assert_eq!(names(&group(&mut s, &id)), ["Alpha", "Bravo", "Charlie", "Delta"]);
    // By name, or by the master's ID.
    run(&mut s, "stencil.add_quick", json!({ "stencil": id, "master": "Foxtrot" }));
    let v = run(&mut s, "stencil.add_quick", json!({ "stencil": id, "master": 5, "at": 1 }));
    assert_eq!(names(&v), ["Alpha", "Echo", "Bravo", "Charlie", "Delta", "Foxtrot"], "{v}");
    assert_eq!(offer(&run(&mut s, "stencil.quick", json!({ "stencil": id }))), ["Alpha", "Echo", "Bravo", "Charlie"]);
    // The connector is a master that can be added, and is not offered.
    run(&mut s, "stencil.add_quick", json!({ "stencil": id, "master": "Link", "at": 0 }));
    assert_eq!(offer(&run(&mut s, "stencil.quick", json!({ "stencil": id })))[0], "Alpha");
    // A stencil that is closed has no group to read or change.
    run(&mut s, "stencil.close", json!({ "id": id }));
    let e = s.execute("stencil.quick", &json!({ "stencil": id })).unwrap_err().to_string();
    assert!(e.contains("no stencil `file-1`"), "{e}");
    // The same file opened again (it is `file-2` now) has the group the drawing set, since the choice names the file.
    let again = flowchart_file(&mut s, &dir);
    assert_eq!(again, "file-2");
    assert_eq!(names(&group(&mut s, &again)), ["Link", "Alpha", "Echo", "Bravo", "Charlie", "Delta", "Foxtrot"]);
}

#[test]
fn what_is_not_a_group_is_refused_in_words_and_nothing_changes() {
    let mut s = blank_session();
    let before = snapshot(&s);
    for (id, params, words) in [
        ("stencil.add_quick", json!({}), "missing `stencil`"),
        ("stencil.add_quick", json!({ "stencil": "basic" }), "missing `master`"),
        ("stencil.add_quick", json!({ "stencil": "nope", "master": "rectangle" }), "no stencil `nope`"),
        ("stencil.add_quick", json!({ "stencil": "basic", "master": "no such" }), "no master `no such` in the stencil `basic`"),
        ("stencil.add_quick", json!({ "stencil": "basic", "master": "rectangle", "at": -1 }), "`at`"),
        ("stencil.add_quick", json!({ "stencil": 3, "master": "rectangle" }), "`stencil` must be text"),
        ("stencil.remove_quick", json!({ "stencil": "basic", "master": "no such" }), "no master `no such`"),
        ("stencil.reset", json!({ "stencil": "nope" }), "no stencil `nope`"),
        ("stencil.quick", json!({ "stencil": "nope" }), "no stencil `nope`"),
    ] {
        let e = s.execute(id, &params).unwrap_err().to_string();
        assert!(e.contains(words), "{id} {params}: {e}");
    }
    assert_eq!(snapshot(&s), before);
    assert!(!s.is_dirty());
    // Without a drawing the changes are disabled and the query shows the stencils' own groups.
    let mut none = Session::new();
    assert!(none.execute("stencil.add_quick", &json!({ "stencil": "basic", "master": "rectangle" })).is_err());
    assert_eq!(names(&none.execute("stencil.quick", &json!({ "stencil": "basic" })).unwrap()["stencils"][0]).len(), 4);
}

#[test]
fn a_group_holds_so_many_masters_and_the_refusal_says_so() {
    let mut s = blank_session();
    let dir = StencilDir::new();
    let mut builder = VsdxBuilder::new().document_type(CT_STENCIL);
    for id in 1..=40 {
        builder = builder.master(MasterSpec::new(id, &format!("M{id}")).attrs(&ids(id, id + 100)).shapes(&shape(5, &box_cells(1.0, 1.0))));
    }
    let path = dir.write("forty.vssx", &builder.build());
    let id = open(&mut s, &path)["id"].as_str().unwrap().to_owned();
    for k in 1..=24 {
        run(&mut s, "stencil.add_quick", json!({ "stencil": id, "master": k }));
    }
    assert_eq!(group(&mut s, &id)["masters"].as_array().unwrap().len(), 24);
    let e = s.execute("stencil.add_quick", &json!({ "stencil": id, "master": 25 })).unwrap_err().to_string();
    assert!(e.contains("holds at most 24 Quick Shapes"), "{e}");
    // A master that is in the group is moved, which is no more.
    run(&mut s, "stencil.add_quick", json!({ "stencil": id, "master": 3, "at": 0 }));
}

fn opened(path: &str) -> Session {
    let mut s = Session::new();
    s.pin_clock(Some(PINNED));
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

fn set_up() -> Session {
    let mut s = Session::new();
    s.pin_clock(Some(PINNED));
    run(&mut s, "file.new", json!({}));
    run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 1, "x": 3, "y": 3 }));
    run(&mut s, "stencil.add_quick", json!({ "stencil": "flowchart", "master": "database", "at": 0 }));
    run(&mut s, "stencil.remove_quick", json!({ "stencil": "basic", "master": "square" }));
    s
}

fn the_set_up_groups(s: &mut Session) {
    assert_eq!(names(&group(s, "flowchart"))[..2], ["Database", "Process"]);
    assert_eq!(names(&group(s, "basic")), ["Rectangle", "Ellipse", "Circle"]);
    assert_eq!(group(s, "arrows")["customized"], false, "a stencil nobody set is the stencil's own");
}

/// Kept by `.diacraft`, by VSDX (the model writer and a save into the file itself) and by `.odg`: the choice is DiaCraft's own data, which every format carries.
#[test]
fn the_choice_survives_a_save_and_a_reopen_in_each_format() {
    let dir = Dir::new();
    let mut s = set_up();
    for name in ["q.diacraft", "q.vsdx", "q.odg"] {
        let path = dir.path(name);
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut back = opened(&path);
        the_set_up_groups(&mut back);
        // Saved on from the reopened drawing, to another format, it is still there.
        let on = dir.path(&format!("on-{name}.diacraft"));
        run(&mut back, "file.save_as", json!({ "path": on }));
        the_set_up_groups(&mut opened(&on));
    }
    // A VSDX file saved into itself keeps it (a splice), and so does an `.odg`.
    for name in ["into.vsdx", "into.odg"] {
        let path = dir.path(name);
        let mut fresh = Session::new();
        fresh.pin_clock(Some(PINNED));
        run(&mut fresh, "file.new", json!({}));
        run(&mut fresh, "shape.drop", json!({ "master": "rectangle", "page": 1, "x": 3, "y": 3 }));
        run(&mut fresh, "file.save_as", json!({ "path": path }));
        let mut again = opened(&path);
        run(&mut again, "stencil.add_quick", json!({ "stencil": "flowchart", "master": "database", "at": 0 }));
        run(&mut again, "stencil.remove_quick", json!({ "stencil": "basic", "master": "square" }));
        run(&mut again, "file.save", json!({}));
        the_set_up_groups(&mut opened(&path));
    }
}

/// A stencil file's name is the key of its group, and the data's reader takes a key of at most 128 bytes. A name of 128 bytes and one of 129 (three-byte
/// characters, which the 64 characters of a stencil's name can make up to 259 bytes of) keep their groups through a save and a reopen in each format, and so does every other
/// group the Quick Shapes stencil shows: where the data was written so that it read as nothing, the reopened drawing showed the stencils' own groups.
#[test]
fn a_save_and_a_reopen_keep_every_group_the_window_shows_whatever_the_stencils_are_called() {
    let stencils = StencilDir::new();
    let at_limit = format!("{}ab", "表".repeat(42));
    let past_limit = "表".repeat(43);
    assert_eq!((at_limit.len(), past_limit.len()), (128, 129));
    let paths: Vec<String> = [&at_limit, &past_limit]
        .iter()
        .map(|stem| {
            let mut builder = VsdxBuilder::new().document_type(CT_STENCIL);
            for (id, name) in [(1, "Alpha"), (2, "Bravo"), (3, "Charlie"), (4, "Delta"), (5, "Echo"), (6, "Foxtrot")] {
                builder = builder.master(MasterSpec::new(id, name).attrs(&ids(id, id + 100)).shapes(&shape(5, &box_cells(1.0, 1.0))));
            }
            stencils.write(&format!("{stem}.vssx"), &builder.build())
        })
        .collect();
    let open_all = |s: &mut Session| -> Vec<String> { paths.iter().map(|p| open(s, p)["id"].as_str().unwrap().to_owned()).collect() };
    let mut s = Session::new();
    s.pin_clock(Some(PINNED));
    run(&mut s, "file.new", json!({}));
    let files = open_all(&mut s);
    run(&mut s, "stencil.add_quick", json!({ "stencil": files[0], "master": "Echo", "at": 0 }));
    run(&mut s, "stencil.add_quick", json!({ "stencil": files[1], "master": "Foxtrot", "at": 1 }));
    run(&mut s, "stencil.remove_quick", json!({ "stencil": files[1], "master": "Alpha" }));
    run(&mut s, "stencil.add_quick", json!({ "stencil": "basic", "master": "hexagon", "at": 0 }));
    let shown = run(&mut s, "stencil.quick", json!({}));
    assert_eq!(shown["readable"], true);
    let file_groups: Vec<Value> = shown["stencils"].as_array().unwrap().iter().filter(|g| g["builtin"] == false).cloned().collect();
    assert_eq!(file_groups.len(), 2, "{shown}");
    assert_eq!(names(&file_groups[0]), ["Echo", "Alpha", "Bravo", "Charlie", "Delta"]);
    assert_eq!(names(&file_groups[1]), ["Foxtrot", "Bravo", "Charlie", "Delta"]);
    assert!(file_groups.iter().all(|g| g["customized"] == true));
    let dir = Dir::new();
    for name in ["named.diacraft", "named.vsdx", "named.odg"] {
        let path = dir.path(name);
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut back = opened(&path);
        assert_eq!(open_all(&mut back), files, "the files are numbered as they were");
        assert_eq!(run(&mut back, "stencil.quick", json!({})), shown, "{name}: every group the window showed");
    }
}

/// Reset takes the data out again: a file whose groups are the stencils' own holds none of it.
#[test]
fn reset_takes_the_data_out_and_the_next_save_holds_none() {
    let dir = Dir::new();
    let mut s = set_up();
    run(&mut s, "stencil.reset", json!({ "stencil": "flowchart" }));
    run(&mut s, "stencil.reset", json!({ "stencil": "basic" }));
    assert!(s.document().unwrap().diacraft.is_none());
    let path = dir.path("none.diacraft");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let mut back = opened(&path);
    assert_eq!(group(&mut back, "basic")["customized"], false);
    assert!(back.document().unwrap().diacraft.is_none());
}

/// A drawing whose data is not one this build can read shows each stencil's own and refuses to change it, and the data stays as it was.
#[test]
fn data_that_cannot_be_read_is_kept_and_the_choice_is_refused_in_words() {
    let dir = Dir::new();
    let too_many = format!("[[\"b\",\"basic\",[{}\"r\"]]]", "\"r\",".repeat(30));
    for (name, text) in [
        ("not.vsdx", "not json".to_owned()),
        ("shape.vsdx", r#"{"basic": 1}"#.to_owned()),
        ("kind.vsdx", r#"[["x","basic",["rectangle"]]]"#.to_owned()),
        ("many.vsdx", too_many),
    ] {
        let mut doc = Document::new_blank();
        assert!(doc.set_diacraft_child("quick_shapes", Some(&[("v", text.as_str())])));
        let path = dir.path(name);
        std::fs::write(&path, write_with(&doc, None, &WriteOptions::default()).unwrap().bytes).unwrap();
        let mut s = opened(&path);
        let v = run(&mut s, "stencil.quick", json!({ "stencil": "basic" }));
        assert_eq!(v["readable"], false, "{name}: {v}");
        assert_eq!(names(&v["stencils"][0]), ["Rectangle", "Square", "Ellipse", "Circle"], "the stencil's own are shown");
        let before = snapshot(&s);
        for (id, params) in [
            ("stencil.add_quick", json!({ "stencil": "basic", "master": "hexagon" })),
            ("stencil.remove_quick", json!({ "stencil": "basic", "master": "rectangle" })),
            ("stencil.reset", json!({ "stencil": "basic" })),
        ] {
            let e = s.execute(id, &params).unwrap_err().to_string();
            assert!(e.contains("cannot read") && e.contains("kept as it is"), "{name} {id}: {e}");
        }
        assert_eq!(snapshot(&s), before);
        assert_eq!(
            s.document().unwrap().diacraft_child("quick_shapes").unwrap().attrs.get("v"),
            Some(text.as_str()),
            "{name}: the data is as it was"
        );
    }
}

// ---- which stencils and masters feed the strip ------------------------------------------------------------------------------------------------------

fn offered_ids(v: &Value) -> Vec<String> {
    v["offer"]["masters"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap().to_owned()).collect()
}

/// The masters of the built-in stencil `id` as `stencil.list` has them.
fn listed(s: &mut Session, id: &str) -> Vec<Value> {
    let list = run(s, "stencil.list", json!({}));
    list["stencils"].as_array().unwrap().iter().find(|st| st["id"] == id).unwrap()["masters"].as_array().unwrap().clone()
}

/// "Not all stencils feed it (the Arrow Shapes stencil does not)". A Swimlane and a Separator are put in a list of lanes or phases when dropped, and
/// are no shape to join: they stay the Cross-Functional stencil's Quick Shapes and the strip does not offer them. A stencil that feeds the strip nothing gives it the next open one's.
#[test]
fn the_strip_offers_no_swimlane_no_separator_and_nothing_of_arrow_shapes_and_takes_the_next_stencil_that_feeds_it() {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({ "template": "cross_functional_flowchart" }));
    let v = run(&mut s, "stencil.quick", json!({ "stencil": "cross-functional" }));
    assert_eq!(names(&v["stencils"][0]), ["Swimlane", "Separator"], "they are its Quick Shapes still: {v}");
    assert!(offered_ids(&v).iter().all(|id| !id.starts_with("xfunc.")), "no lane and no separator is offered: {v}");
    assert_eq!(v["offer"]["stencil"], "flowchart", "the next open stencil that feeds the strip: {v}");
    assert_eq!(offer(&v), offer(&run(&mut s, "stencil.quick", json!({ "stencil": "flowchart" }))));
    // Arrow Shapes feeds it nothing, whatever its group: the first open stencil that feeds it does.
    let mut s = blank_session();
    run(&mut s, "stencil.add_quick", json!({ "stencil": "arrows", "master": "arrows.right", "at": 0 }));
    let a = run(&mut s, "stencil.quick", json!({ "stencil": "arrows" }));
    assert_eq!(a["offer"]["stencil"], "basic", "{a}");
    assert_eq!(offer(&a), ["Rectangle", "Square", "Ellipse", "Circle"], "{a}");
    // The Quick Shapes stencil gathers them as they are and passes over them for the strip.
    run(&mut s, "file.new", json!({ "template": "Basic Diagram" }));
    for m in ["Rectangle", "Square", "Ellipse", "Circle"] {
        run(&mut s, "stencil.remove_quick", json!({ "stencil": "basic", "master": m }));
    }
    let g = run(&mut s, "stencil.quick", json!({}));
    assert!(offered_ids(&g).iter().all(|id| !id.starts_with("arrows.")), "{g}");
    assert_eq!(offer(&g), ["Rectangle", "Square", "Ellipse", "Circle"], "with no Quick Shape that feeds it, the first open stencil's: {g}");
    // What the window reads: a master that is a part of a structure says so, and one the strip never offers says that.
    let mut s = blank_session();
    let xfunc = listed(&mut s, "cross-functional");
    assert!(xfunc.iter().all(|m| m["part"] == true && m["autoconnect"] == false), "{xfunc:?}");
    let arrows = listed(&mut s, "arrows");
    assert!(arrows.iter().all(|m| m["autoconnect"] == false && m.get("part").is_none()), "{arrows:?}");
    let basic = listed(&mut s, "basic");
    assert!(basic.iter().all(|m| m.get("autoconnect").is_none() && m.get("part").is_none()), "{basic:?}");
}

#[test]
fn autoconnect_refuses_a_swimlane_or_a_separator_in_words_and_changes_nothing() {
    let mut s = blank_session();
    let from = crate::edit::drop_shape(&mut s, "rectangle", 3.0, 3.0);
    for master in ["xfunc.swimlane", "Separator"] {
        let before = snapshot(&s);
        let e = s.execute("connector.autoconnect", &json!({ "from": from, "direction": "right", "master": master })).unwrap_err().to_string();
        assert!(e.contains("is a part of a structure") && e.contains("shape.drop"), "{master}: {e}");
        assert_eq!(snapshot(&s), before, "{master}: nothing changed");
    }
}

/// A master of a stencil file whose shape is typed `Container` or `List`: the window is told it is a part, the strip passes over it, and AutoConnect
/// refuses it in words.
#[test]
fn a_stencil_files_master_typed_container_or_list_is_a_part_the_strip_passes_over_and_autoconnect_refuses() {
    let typed = |kind: &str| {
        format!(r#"<Section N="User"><Row N="msvStructureType"><Cell N="Value" V="{kind}" U="STR" F="&quot;{kind}&quot;"/></Row></Section>"#)
    };
    let mut builder = VsdxBuilder::new().document_type(CT_STENCIL);
    for (id, name, kind) in [(1, "Lane", "Container"), (2, "Lanes", "List"), (3, "Step", ""), (4, "Box", "")] {
        let extra = if kind.is_empty() { String::new() } else { typed(kind) };
        builder = builder.master(MasterSpec::new(id, name).attrs(&ids(id, id + 100)).shapes(&shape(5, &format!("{}{extra}", box_cells(1.0, 1.0)))));
    }
    let dir = StencilDir::new();
    let mut s = blank_session();
    let stencil = open(&mut s, &dir.write("parts.vssx", &builder.build()))["id"].as_str().unwrap().to_owned();
    let list = run(&mut s, "stencil.list", json!({}));
    let masters = list["stencils"].as_array().unwrap().iter().find(|st| st["id"] == stencil.as_str()).unwrap()["masters"].clone();
    let flags: Vec<(String, bool, bool)> =
        masters.as_array().unwrap().iter().map(|m| (m["name"].as_str().unwrap().to_owned(), m["part"] == true, m["autoconnect"] == false)).collect();
    assert_eq!(
        flags,
        [("Lane".to_owned(), true, true), ("Lanes".to_owned(), true, true), ("Step".to_owned(), false, false), ("Box".to_owned(), false, false)],
        "{masters}"
    );
    let v = run(&mut s, "stencil.quick", json!({ "stencil": stencil }));
    assert_eq!(names(&v["stencils"][0]), ["Lane", "Lanes", "Step", "Box"], "its Quick Shapes as they are");
    assert_eq!(offer(&v), ["Step", "Box"], "the strip passes over the parts: {v}");
    let from = crate::edit::drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let before = snapshot(&s);
    let e =
        s.execute("connector.autoconnect", &json!({ "from": from, "direction": "down", "master": "1", "stencil": stencil })).unwrap_err().to_string();
    assert!(e.contains("is a part of a structure"), "{e}");
    assert_eq!(snapshot(&s), before);
    run(&mut s, "connector.autoconnect", json!({ "from": from, "direction": "down", "master": "3", "stencil": stencil }));
}
