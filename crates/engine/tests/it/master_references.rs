//! The second security review: **a master that leaves its package carries no reference to a part of it.** What a master's slot keeps beside its shapes (the nodes of its
//! part's `Shapes` and `Connects`) is kept when the master is brought from a stencil or put back by a paste, and an attribute of the relationships namespace among it names a row of the part
//! the master had. The package it is written into gives the master a part of its own, with no such row, so the reference would dangle or lead to another part.
//! The one step both take (`stencil::refs::leave_package_for_another`) leaves such an attribute out, keeps the rest, and the answer says so. Every input is built in code.

use std::sync::Arc;

use diacraft_engine::Session;
use diacraft_engine::draw::{Placement, copy_shapes, paste_shapes};
use diacraft_testkit::package::{MasterSpec, NS_2012, NS_R, OD, PageSpec, Pkg, VsdxBuilder, shape};
use serde_json::json;

use crate::odg_build::Dir;
use crate::run;
use crate::stencil_open::{CT_STENCIL, Dir as StencilDir, LOOK, box_cells, open};

/// A master whose part's `Shapes` carries an `r:id` to a picture of the part's own (a row of its relationships part, and the picture), and a comment beside the shape.
fn master_with_a_reference() -> MasterSpec {
    let part = format!(
        "<MasterContents xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Shapes r:id=\"rId9\"><!-- kept -->{}</Shapes></MasterContents>",
        shape(5, &format!("{LOOK}{}", box_cells(1.0, 1.0)))
    );
    MasterSpec::new(1, "Ref").part(&part).rel("rId9", &format!("{OD}image"), "../media/kept.png")
}

fn picture() -> Pkg {
    Pkg::new().part("/visio/media/kept.png", "image/png", [0x89u8, b'P', b'N', b'G', 1, 2, 3])
}

/// Every `r:id` of a page or master part of the package, with whether it leads to a part the package has.
fn references(bytes: &[u8]) -> Vec<(String, String, bool)> {
    let pkg = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).unwrap();
    let mut out = Vec::new();
    for name in pkg.parts().map(str::to_owned).collect::<Vec<_>>() {
        if !(name.starts_with("/visio/masters/") || name.starts_with("/visio/pages/")) || name.contains("_rels") {
            continue;
        }
        let text = String::from_utf8_lossy(pkg.part(&name).unwrap()).into_owned();
        let rels = pkg.rels(&name).unwrap();
        for piece in text.split("r:id=\"").skip(1) {
            let id = piece.split('"').next().unwrap_or("").to_owned();
            let leads = rels.get(&id).and_then(|r| r.target.clone()).is_some_and(|t| pkg.part(&t).is_some());
            out.push((name.clone(), id, leads));
        }
    }
    out
}

/// The text of the master part of the package that holds `needle`, if one does.
fn master_part_with(bytes: &[u8], needle: &str) -> Option<String> {
    let pkg = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).unwrap();
    pkg.parts()
        .filter(|n| n.starts_with("/visio/masters/master"))
        .filter_map(|n| pkg.part(n).map(|b| String::from_utf8_lossy(b).into_owned()))
        .find(|text| text.contains(needle))
}

#[test]
fn a_master_brought_from_a_stencil_leaves_its_reference_out_says_so_and_the_file_it_is_saved_in_has_none_that_dangles() {
    let stencil = VsdxBuilder::new().document_type(CT_STENCIL).master(master_with_a_reference()).with(picture()).build();
    // The stencil's master holds the reference beside its shapes (so the case is the one it says).
    let (read, _) = diacraft_vsdx::read(&stencil, &diacraft_vsdx::Limits::default()).unwrap();
    let shapes = read.masters.iter().next().unwrap().ext.get().and_then(|e| e.vsdx.0.iter().find(|(k, _)| &**k == "Shapes").map(|(_, v)| v.clone()));
    assert_eq!(shapes.as_ref().and_then(|k| k.raw.get("r:id")), Some("rId9"), "{shapes:?}");
    let dir = Dir::new();
    let stencils = StencilDir::new();
    let drawing = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, &box_cells(1.0, 1.0)))).build();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &drawing) }));
    let opened = open(&mut s, &stencils.write("refs.vssx", &stencil));
    let stencil_id = opened["id"].as_str().unwrap().to_owned();
    let dropped = run(&mut s, "shape.drop", json!({ "master": "Ref", "stencil": stencil_id, "page": 1, "x": 4.0, "y": 4.0 }));
    let notes = dropped["notes"].as_array().map(|n| n.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" | ")).unwrap_or_default();
    assert!(
        notes.contains("1 reference(s) to parts of the stencil's file") && notes.contains("left out"),
        "the drop says what it left out: {dropped}"
    );
    run(&mut s, "file.save", json!({}));
    let written = dir.read("a.vsdx");
    let bad: Vec<_> = references(&written).into_iter().filter(|(_, _, leads)| !leads).collect();
    assert!(bad.is_empty(), "no reference of the file dangles: {bad:?}");
    let part = master_part_with(&written, "<!-- kept -->").expect("the master's part keeps what it kept beside its shapes");
    assert!(!part.contains("rId9"), "the reference is left out: {part}");
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path("a.vsdx") }));
    crate::check_session(&again).unwrap();
}

#[test]
fn a_master_a_paste_puts_back_leaves_its_reference_out_says_so_and_the_file_it_is_saved_in_has_none_that_dangles() {
    // A drawing whose own master keeps the reference, with an instance of it on its page. The instance is copied, the master and the instance are taken out of the drawing (no command takes a
    // master out; an undo of the drop that brought it does), and the paste puts the master back: it names no part of the file any more, and has no row for the reference.
    let instance = "<Shape ID=\"2\" Type=\"Shape\" Master=\"1\"><Cell N=\"PinX\" V=\"3\"/><Cell N=\"PinY\" V=\"3\"/></Shape>";
    let bytes = VsdxBuilder::new().master(master_with_a_reference()).page(PageSpec::new(1, "P").shapes(instance)).with(picture()).build();
    let (mut doc, _) = diacraft_vsdx::read(&bytes, &diacraft_vsdx::Limits::default()).unwrap();
    let clip = copy_shapes(&doc, 1, &[2]).unwrap();
    doc.masters.remove(0).unwrap();
    Arc::make_mut(doc.pages.get_mut(0).unwrap()).shapes.remove(0).unwrap();
    let pasted = paste_shapes(&mut doc, 1, &clip, Placement::Offset(0.0, 0.0), 2).unwrap();
    assert_eq!(doc.masters.len(), 1, "the paste puts the master back");
    let back = doc.masters.iter().next().unwrap();
    let shapes = back
        .ext
        .get()
        .and_then(|e| e.vsdx.0.iter().find(|(k, _)| &**k == "Shapes").map(|(_, v)| v.clone()))
        .expect("the master keeps its `Shapes` slot");
    assert_eq!(shapes.raw.get("r:id"), None, "the reference is left out");
    assert!(!shapes.kept.is_empty(), "and what it kept beside its shapes stays");
    assert!(pasted.notes.iter().any(|n| n.contains("1 reference(s)")), "the paste says so: {:?}", pasted.notes);
    let opts = diacraft_vsdx::WriteOptions { now: Some(1_700_000_000), ..diacraft_vsdx::WriteOptions::default() };
    let written = diacraft_vsdx::write_with(&doc, None, &opts).unwrap();
    let bad: Vec<_> = references(&written.bytes).into_iter().filter(|(_, _, leads)| !leads).collect();
    assert!(bad.is_empty(), "no reference of the file dangles: {bad:?}");
    diacraft_vsdx::read(&written.bytes, &diacraft_vsdx::Limits::default()).unwrap();
}
