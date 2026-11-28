//! The masters a stencil drop brings into an `.odg` drawing are carried in DiaCraft's document file like the file's own, and the shape
//! made from one keeps its link to it, so that a file read again has the shape linked to the master the file carries. A save that only returns Ok proves nothing
//! about that: the file is opened again in a session of its own and the shape, its master and the sections the shape resolves are compared with the session's.

use diacraft_doc::{Document, Owner, Resolver, ShapeRef, SheetRef};
use diacraft_engine::Session;
use diacraft_testkit::package::{MasterSpec, VsdxBuilder, shape};
use serde_json::json;

use crate::odg_build::{Dir, content_of, open_drawing, part_bytes};
use crate::odg_file::{assert_reopens_as_saved, assert_saves_again_as_no_edit, four, save};
use crate::run;
use crate::stencil_open::{CT_STENCIL, Dir as StencilDir, LOOK, box_cells, open};

/// Asserts that the shape at `at` of the first page of `read` (a file read again) is linked to its master as the shape of `made` (the session's own) is, that the
/// master the file carries is the session's, body and all, and that the sections the shape resolves are its own: the master's rows do not come in beside them
/// (the resolver merges a shape's rows with its master's one by one). **A shape that holds no `Connection` section in the session holds none in the file** (a line's or a
/// connector's copy holds none of its master's, and the file says it has none, where the reader would give it the four standard rows): it resolves its master's, as the session's does.
#[track_caller]
pub fn assert_link_holds(made: &Document, read: &Document, at: usize, what: &str) {
    let (a, b) = (made.pages.get(0).unwrap().shapes.get(at).unwrap(), read.pages.get(0).unwrap().shapes.get(at).unwrap());
    let id = a.master.unwrap_or_else(|| panic!("{what}: the shape of the session is an instance of a master"));
    assert_eq!((a.master, a.master_shape), (b.master, b.master_shape), "{what}: the shape of the file has the links the session's has");
    let find = |d: &Document| d.masters.iter().find(|m| m.id == id).cloned();
    let (ma, mb) =
        (find(made).expect("the session holds the master"), find(read).unwrap_or_else(|| panic!("{what}: the file carries no master {id}")));
    assert_eq!(*ma, *mb, "{what}: the master the file carries is the session's");
    let res = Resolver::new(read);
    let page = read.pages.get(0).unwrap();
    let place = SheetRef::Shape(ShapeRef::top(Owner::Page(page.id), u32::try_from(at).unwrap()));
    for name in ["Geometry", "Connection"] {
        let keys = res.effective_sections(&place, name).unwrap_or_default();
        let own: Vec<_> = b.sheet.sections.iter().filter(|s| s.name.as_str() == name).collect();
        if name == "Connection" && own.is_empty() && !a.sheet.sections.iter().any(|s| s.name.as_str() == name) {
            let made_res = Resolver::new(made);
            let made_place = SheetRef::Shape(ShapeRef::top(Owner::Page(made.pages.get(0).unwrap().id), u32::try_from(at).unwrap()));
            let rows = |r: &Resolver<'_>, p: &SheetRef| -> Vec<usize> {
                r.effective_sections(p, name).unwrap_or_default().iter().map(|k| r.effective_rows(p, k).unwrap_or_default().len()).collect()
            };
            assert_eq!(
                rows(&res, &place),
                rows(&made_res, &made_place),
                "{what}: the shape holds no {name} section, and resolves what the session's resolves"
            );
            continue;
        }
        // A group draws through its members and holds no geometry of its own: a group master's top-level shape.
        // The shape of the file is a group by its kind, or the session's shape or the file's has members.
        let is_group = b.kind == diacraft_doc::ShapeType::Group || !a.children.is_empty() || !b.children.is_empty();
        assert!(name != "Geometry" || is_group || !own.is_empty(), "{what}: the shape of the file holds its geometry");
        assert_eq!(keys.len(), own.len(), "{what}: the {name} sections the shape resolves are its own");
        for key in keys {
            let rows = res.effective_rows(&place, &key).unwrap_or_default();
            let held = own.iter().find(|s| s.ix == key.ix).unwrap_or_else(|| panic!("{what}: a {name} section the shape does not hold: {key:?}"));
            assert_eq!(rows.len(), held.rows.len(), "{what}: the {name} {:?} rows the shape resolves are its own", key.ix);
            for (row, mine) in rows.iter().zip(held.rows.iter()) {
                if mine.row_type.is_some() {
                    assert_eq!(row.row_type, mine.row_type, "{what}: the type of a row of {name} {:?}", key.ix);
                }
            }
        }
    }
}

/// One shape of each of the seven built-in stencils is dropped into a drawing, which is saved: a session of its own reads the file and has each shape linked to its
/// master, the seven masters equal to the session's, and the sections of each shape its own; a second save is no edit; and a shape moved, formatted and given a text
/// after the reopen is saved as the edit it is and keeps the link.
#[test]
fn a_shape_of_each_stencil_keeps_its_master_link_through_a_save_a_reopen_and_the_edits_after_it() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let mut dropped = Vec::new();
    // The seven stencils of the first batches (the second batch's have their own checks, `stencils2_<stencil>.rs`).
    for (n, stencil) in diacraft_engine::stencil::stencils().iter().filter(|st| st.default_open).enumerate() {
        let master = stencil.masters().first().unwrap();
        let made = run(&mut s, "shape.drop", json!({ "master": master.id, "stencil": stencil.id, "page": 0, "x": 1.0 + 2.0 * n as f64, "y": 5.0 }));
        dropped.push((format!("{}/{}", stencil.id, master.id), made["id"].as_u64().unwrap()));
    }
    assert_eq!(dropped.len(), 7, "seven stencils");
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true);
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path("a.odg") }));
    for (key, id) in &dropped {
        let at = s.document().unwrap().pages.get(0).unwrap().shapes.iter().position(|x| u64::from(x.id) == *id).unwrap();
        assert_link_holds(s.document().unwrap(), again.document().unwrap(), at, key);
    }
    assert_eq!(again.document().unwrap().masters.len(), 7, "seven masters in the file");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    // The file read again by a session of its own saves as no edit as well, and an edit of its shapes is written as the edit it is and the links hold.
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path("a.odg") }));
    let unedited = save(&mut again);
    assert_eq!(unedited["edited"], false, "{unedited}");
    let (key, id) = &dropped[1];
    run(&mut again, "shape.move", json!({ "ids": [id], "dx": 1.0, "dy": 0.5 }));
    run(&mut again, "shape.format", json!({ "ids": [id], "fill": "#123456" }));
    run(&mut again, "shape.set_text", json!({ "id": id, "text": "after" }));
    let edited = save(&mut again);
    assert_eq!(edited["edited"], true, "{edited}");
    let mut third = Session::new();
    run(&mut third, "file.open", json!({ "path": dir.path("a.odg") }));
    let at = again.document().unwrap().pages.get(0).unwrap().shapes.iter().position(|x| u64::from(x.id) == *id).unwrap();
    assert_link_holds(again.document().unwrap(), third.document().unwrap(), at, key);
    assert_eq!(third.document().unwrap().masters.len(), 7);
}

/// A master of two top-level shapes, a box 1 by 1 and another 1 by 1 two units to the right of it: dropped, it is a group of the two, whose frame is the joint box of
/// the members (the frame an `.odg` holds for a group: the file keeps none, the reader makes it from the members).
pub fn pair_master() -> MasterSpec {
    let at = |pin_x: &str| {
        format!("{LOOK}{}", box_cells(1.0, 1.0).replace(r#"<Cell N="PinX" V="0.5" F="Width*0.5"/>"#, &format!(r#"<Cell N="PinX" V="{pin_x}"/>"#)))
    };
    MasterSpec::new(1, "Pair").shapes(&format!("{}{}", shape(5, &at("0.5")), shape(6, &at("2.5"))))
}

/// A master of several shapes from an opened stencil file is dropped as a group: an instance that links to the master, with members that stand for its shapes. The
/// file carries the master, the group's links in its entry of the document file and the members' links on their styles, and a file read again has the same tree, the
/// same links, no member twice (a group linked to a master gets the master's shapes it has no member for), and the same picture.
#[test]
fn a_group_dropped_from_a_master_of_several_shapes_keeps_its_links_and_its_members_stand_for_the_masters_shapes() {
    let stencils = StencilDir::new();
    let bytes = VsdxBuilder::new().document_type(CT_STENCIL).master(pair_master()).build();
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let opened = open(&mut s, &stencils.write("group.vssx", &bytes));
    let stencil = opened["id"].as_str().unwrap().to_owned();
    let made = run(&mut s, "shape.drop", json!({ "master": "Pair", "stencil": stencil, "page": 0, "x": 6.0, "y": 6.0 }));
    let id = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
    let in_session = s.document().unwrap().pages.get(0).unwrap().shapes.iter().find(|x| x.id == id).unwrap().clone();
    assert!(in_session.master.is_some() && !in_session.children.is_empty(), "an instance with members");
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true);
    let content = content_of(&dir.read("a.odg"));
    assert!(content.contains("<draw:g"), "a group in the file: {content}");
    let document = String::from_utf8(part_bytes(&dir.read("a.odg"), "DiaCraft/document.xml").expect("the document file")).unwrap();
    assert!(document.contains(&format!("master=\"{}\"", in_session.master.unwrap())), "the group's entry names the master: {document}");
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path("a.odg") }));
    let read = again.document().unwrap().pages.get(0).unwrap().shapes.iter().find(|x| x.id == id).unwrap().clone();
    assert_eq!((read.master, read.master_shape), (in_session.master, in_session.master_shape), "the group's links");
    let links = |shape: &diacraft_doc::Shape| -> Vec<(Option<u32>, usize)> {
        let mut out = Vec::new();
        let mut stack: Vec<&diacraft_doc::Shape> = shape.children.iter().map(|c| &**c).collect();
        stack.reverse();
        while let Some(m) = stack.pop() {
            out.push((m.master_shape, m.children.len()));
            stack.extend(m.children.iter().rev().map(|c| &**c));
        }
        out
    };
    assert_eq!(links(&read), links(&in_session), "the members stand for the same shapes of the master, in the same tree");
    let find = |d: &Document| d.masters.iter().find(|m| Some(m.id) == in_session.master).cloned();
    assert_eq!(find(s.document().unwrap()), find(again.document().unwrap()), "the master is what the session has");
    let members = |doc: &Document| {
        let page = doc.pages.get(0).unwrap();
        let at = page.shapes.iter().position(|x| x.id == id).unwrap();
        let place = ShapeRef::top(Owner::Page(page.id), u32::try_from(at).unwrap());
        Resolver::new(doc).effective_members(&place).map_or(0, |m| m.len())
    };
    assert_eq!(members(again.document().unwrap()), members(s.document().unwrap()), "no member twice and none lost");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    let mut later = Session::new();
    run(&mut later, "file.open", json!({ "path": dir.path("a.odg") }));
    let unedited = save(&mut later);
    assert_eq!(unedited["edited"], false, "{unedited}");
}

/// A stencil whose one master's part holds, in its `Shapes` and its `Connects`, nodes the model has no field for: an attribute of another namespace and a comment in each.
fn kept_nodes_stencil() -> Vec<u8> {
    let ns = diacraft_testkit::package::NS_2012;
    let part = format!(
        "<MasterContents xmlns=\"{ns}\" xmlns:zz=\"urn:example:zz\"><Shapes zz:kept=\"1\"><!-- kept -->{}</Shapes><Connects zz:also=\"2\"><!-- kept too --></Connects></MasterContents>",
        shape(5, &format!("{LOOK}{}", box_cells(1.0, 1.0)))
    );
    VsdxBuilder::new().document_type(CT_STENCIL).master(MasterSpec::new(1, "Kept").part(&part)).build()
}

/// The kept nodes of a master's `Shapes` and `Connects`, by key.
fn kept_nodes(m: &diacraft_doc::Master) -> Vec<(String, diacraft_doc::Kept)> {
    m.ext
        .get()
        .map(|e| e.vsdx.0.iter().filter(|(k, _)| matches!(&**k, "Shapes" | "Connects")).map(|(k, v)| (k.to_string(), v.clone())).collect())
        .unwrap_or_default()
}

fn master_named<'d>(doc: &'d Document, name: &str) -> &'d diacraft_doc::Master {
    doc.masters.iter().find(|m| m.name.as_deref() == Some(name)).unwrap_or_else(|| panic!("no master {name}"))
}

/// Concern 3 of the F1 round, ruled on: a master whose part's `Shapes` and `Connects` hold nodes the model has no field for (an attribute of another
/// namespace, a comment) is dropped from a stencil file into an `.odg` drawing, which is saved. **The master keeps the nodes** (unknown content survives: `Master::leave_package` drops only
/// what names a place in the stencil's package), the `.odg` document file carries them, and the second save of the same drawing is no edit and the same bytes, as is the
/// save of the file read again.
#[test]
fn a_master_whose_shapes_and_connects_hold_kept_nodes_saves_again_as_no_edit_in_an_odg() {
    let bytes = kept_nodes_stencil();
    let (read, _) = diacraft_vsdx::read(&bytes, &diacraft_vsdx::Limits::default()).unwrap();
    let own = kept_nodes(read.masters.iter().next().unwrap());
    assert_eq!(
        own.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        ["Shapes", "Connects"],
        "the stencil's master keeps the nodes (so the case is the one it says)"
    );
    let stencils = StencilDir::new();
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let opened = open(&mut s, &stencils.write("kept.vssx", &bytes));
    let stencil = opened["id"].as_str().unwrap().to_owned();
    run(&mut s, "shape.drop", json!({ "master": "Kept", "stencil": stencil, "page": 0, "x": 6.0, "y": 6.0 }));
    assert_eq!(kept_nodes(master_named(s.document().unwrap(), "Kept")), own, "the master brought in holds the kept nodes");
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    let mut later = Session::new();
    run(&mut later, "file.open", json!({ "path": dir.path("a.odg") }));
    assert_eq!(kept_nodes(master_named(later.document().unwrap(), "Kept")), own, "the .odg file carries them");
    let unedited = save(&mut later);
    assert_eq!(unedited["edited"], false, "{unedited}");
}

/// The VSDX counterpart: the same master dropped into a drawing read from a `.vsdx` keeps the attribute and the comment of its `Shapes` and its `Connects` through a save and a reopen,
/// in the file's own master part, and a second save is no edit.
#[test]
fn a_master_whose_shapes_and_connects_hold_kept_nodes_keeps_them_in_a_vsdx_through_a_save_and_a_reopen() {
    let bytes = kept_nodes_stencil();
    let (read, _) = diacraft_vsdx::read(&bytes, &diacraft_vsdx::Limits::default()).unwrap();
    let own = kept_nodes(read.masters.iter().next().unwrap());
    let stencils = StencilDir::new();
    let dir = Dir::new();
    let drawing = VsdxBuilder::new().page(diacraft_testkit::package::PageSpec::new(1, "P").shapes(&shape(1, &box_cells(1.0, 1.0)))).build();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &drawing) }));
    let opened = open(&mut s, &stencils.write("kept.vssx", &bytes));
    let stencil = opened["id"].as_str().unwrap().to_owned();
    run(&mut s, "shape.drop", json!({ "master": "Kept", "stencil": stencil, "page": 1, "x": 6.0, "y": 6.0 }));
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["dirty"], false, "{saved}");
    let written = dir.read("a.vsdx");
    let pkg = diacraft_opc::Package::read(&written, &diacraft_opc::Limits::default()).unwrap();
    let masters: Vec<String> =
        pkg.parts().filter(|n| n.starts_with("/visio/masters/") && !n.ends_with("masters.xml") && !n.contains("_rels")).map(str::to_owned).collect();
    assert_eq!(masters.len(), 1, "{masters:?}");
    let text = String::from_utf8(pkg.part(&masters[0]).unwrap().to_vec()).unwrap();
    for kept in ["zz:kept=\"1\"", "<!-- kept -->", "zz:also=\"2\"", "<!-- kept too -->"] {
        assert!(text.contains(kept), "the master part keeps {kept}: {text}");
    }
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path("a.vsdx") }));
    assert_eq!(kept_nodes(master_named(again.document().unwrap(), "Kept")), own, "the file read again holds them");
    let unedited = run(&mut again, "file.save", json!({}));
    assert_eq!(dir.read("a.vsdx"), written, "a second save of the file read again is the same bytes: {unedited}");
}
