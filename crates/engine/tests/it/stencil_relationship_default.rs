//! An outer declaration never lifts the closed relationship default. A master whose sheet rebinds `r` to another namespace and keeps an
//! `r:id` under it is refused, and so nothing reaches the drawing that its relationships do not resolve; a rebinding on the kept element itself,
//! which the copy gives back with it, is copied and saved with its declaration. Packages are built in code.

use diacraft_opc::{Limits, Package};
use diacraft_testkit::package::{MasterSpec, NS_2012, NS_R, VsdxBuilder, shape_with};
use serde_json::json;

use crate::stencil_open::{CT_STENCIL, Dir, box_cells, ids, open, refusal};
use crate::{blank_session, check_session, run};

fn part_with(shapes: &str) -> String {
    format!(r#"<?xml version="1.0" encoding="utf-8"?><MasterContents xmlns="{NS_2012}" xmlns:r="{NS_R}"><Shapes>{shapes}</Shapes></MasterContents>"#)
}

/// A stencil of one master "M" holding one shape that is `shape_attrs`, `cells` and then `kept`.
fn stencil(shape_attrs: &str, cells: &str, kept: &str) -> Vec<u8> {
    let inner = format!("{}{cells}{kept}", box_cells(1.0, 1.0));
    VsdxBuilder::new()
        .document_type(CT_STENCIL)
        .master(MasterSpec::new(1, "M").attrs(&ids(81, 82)).part(&part_with(&shape_with(5, shape_attrs, &inner))))
        .build()
}

/// The text of every part of a saved file whose name contains `needle`.
fn parts_of(path: &str, needle: &str) -> Vec<(String, String)> {
    let package = Package::read(&std::fs::read(path).unwrap(), &Limits::default()).unwrap();
    let names: Vec<String> = package.parts().filter(|p| p.to_ascii_lowercase().contains(needle)).map(str::to_owned).collect();
    names.into_iter().map(|n| (n.clone(), String::from_utf8(package.part(&n).unwrap().to_vec()).unwrap())).collect()
}

/// Every `r:id` in a saved file that the part's root would bind to the relationships namespace (the element does not rebind `r` itself) and that the
/// part's own relationships do not resolve: `part: <tag>`.
fn dangling(path: &str) -> Vec<String> {
    let package = Package::read(&std::fs::read(path).unwrap(), &Limits::default()).unwrap();
    let mut out = Vec::new();
    for (name, xml) in parts_of(path, ".xml") {
        if name.contains("_rels/") {
            continue;
        }
        let (dir, file) = name.rsplit_once('/').unwrap_or(("", &name));
        let rels = package.part(&format!("{dir}/_rels/{file}.rels")).map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
        for tag in xml.split('<') {
            let Some((_, after)) = tag.split_once(r#" r:id=""#) else { continue };
            let id = after.split('"').next().unwrap_or("");
            if !tag.contains("xmlns:r=") && !rels.contains(&format!(r#"Id="{id}""#)) {
                out.push(format!("{name}: <{tag}"));
            }
        }
    }
    out
}

#[test]
fn a_master_whose_sheet_rebinds_r_and_keeps_an_r_id_under_it_is_refused_and_nothing_dangles() {
    let dir = Dir::new();
    let kept = r#"<Mystery r:id="rId1"/>"#;
    // The rebinding is on the element the model has a field for that the kept node sits in: the shape, a cell, a section or a row.
    let cases: [(&str, String, String, String); 4] = [
        ("shape", r#" xmlns:r="urn:other""#.to_owned(), String::new(), kept.to_owned()),
        ("cell", String::new(), format!(r#"<Cell N="LineWeight" V="0.01" xmlns:r="urn:other">{kept}</Cell>"#), String::new()),
        ("section", String::new(), format!(r#"<Section N="User" xmlns:r="urn:other">{kept}</Section>"#), String::new()),
        ("row", String::new(), format!(r#"<Section N="User"><Row N="Mark" xmlns:r="urn:other">{kept}</Row></Section>"#), String::new()),
    ];
    for (place, shape_attrs, cells, after) in cases {
        let mut s = blank_session();
        open(&mut s, &dir.write(&format!("{place}.vssx"), &stencil(&shape_attrs, &cells, &after)));
        let msg = refusal(&mut s, "shape.drop", json!({"master": "M", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
        assert!(msg.contains("of the stencil's file"), "{place}: {msg}");
        // The drawing is saved and read again: the refused node is nowhere in it, and no `r:id` is there that its relationships do not resolve.
        let out = dir.path(&format!("{place}-out.vsdx"));
        run(&mut s, "file.save_as", json!({"path": out}));
        for (name, xml) in parts_of(&out, "") {
            assert!(!xml.contains("Mystery"), "{place}: {name} holds the refused node");
        }
        assert_eq!(dangling(&out), Vec::<String>::new(), "{place}");
        let mut t = diacraft_engine::Session::new();
        run(&mut t, "file.open", json!({"path": out}));
        assert_eq!(check_session(&t), Ok(()));
    }
}

#[test]
fn a_rebinding_on_the_kept_element_itself_is_copied_and_saved_with_its_declaration() {
    let dir = Dir::new();
    let mut s = blank_session();
    let kept = r#"<Mystery xmlns:r="urn:other" r:id="7"/>"#;
    // The shape rebinds `r` as well, which does not matter: the kept element says what it means.
    open(&mut s, &dir.write("own.vssx", &stencil(r#" xmlns:r="urn:other""#, "", kept)));
    run(&mut s, "shape.drop", json!({"master": "M", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
    let out = dir.path("own-out.vsdx");
    run(&mut s, "file.save_as", json!({"path": out}));
    // The kept element is in the saved master with its own declaration, so its `r:id` is another namespace's and not a dangling relationship; and
    // everything else in the file that names a relationship is resolved by it.
    let mut seen = 0;
    for (name, xml) in parts_of(&out, "masters/master") {
        for tag in xml.split('<').filter(|t| t.starts_with("Mystery")) {
            seen += 1;
            assert!(tag.contains(r#"xmlns:r="urn:other""#) && tag.contains(r#"r:id="7""#), "{name}: <{tag}");
        }
    }
    assert_eq!(seen, 1, "the kept element is in the saved master");
    assert_eq!(dangling(&out), Vec::<String>::new());
    let mut t = diacraft_engine::Session::new();
    run(&mut t, "file.open", json!({"path": out}));
    assert_eq!(check_session(&t), Ok(()));
}
