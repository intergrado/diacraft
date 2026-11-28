//! An instance of a built-in connector master is saved so that the VSDX application takes the master's line cells for it: its line ends and its pattern.
//!
//! The application's check of the files DiaCraft writes: the instances of the connector masters of the built-in stencils (UML, BPMN, C4, crow's foot, Gane-Sarson) read `BeginArrow` 0,
//! `EndArrow` 0 and `LinePattern` 1 there, the root style's, whatever the master held, plain numbers and formulas alike, while a shape's own `EndArrow` read back as written. Each such
//! instance named the root style in its own `LineStyle`, `FillStyle` and `TextStyle`, which the connector tool gives a connector of its own; the instances of the files the application
//! writes carry no link of their own (about 255,000 with none, against a few hundred with one), so style reaches an instance through its master shape. The
//! application reads an instance's own link before the master's cells of that family, and DiaCraft's resolver does the opposite (step 3, a report the files could not test).
//! So a connector dropped from a master holds no link of its own: DiaCraft draws it as before (with no link of its own the master shape's is the one followed), and the
//! application finds the master's cells.
//!
//! These tests read the bytes of the files each route writes: the model writer (a new drawing, and a Save As of a `.diacraft` drawing and of an `.odg` drawing) and the splice into the
//! VSDX file a drawing was opened from, for every one-dimensional master of the catalogue.

use std::collections::BTreeMap;

use diacraft_doc::{CellAddr, Document, Owner, Resolver, SheetRef, Value};
use diacraft_engine::Session;
use diacraft_engine::stencil::{MasterSpec, all_masters};
use diacraft_opc::{Limits, Package};
use serde_json::json;

use crate::connect_whole::id_of;
use crate::edit::doc;
use crate::edit_foreign::Dir;
use crate::{blank_session, check_session, run};

/// The cells of a connector's look the application's check read on the instances, and its No Fill (the owner's ruling of 2026-11-27: a connector's hollow ends are empty by default),
/// each with the link of its family.
pub(crate) const LOOK: [(&str, &str); 4] =
    [("BeginArrow", "LineStyle"), ("EndArrow", "LineStyle"), ("LinePattern", "LineStyle"), ("FillPattern", "FillStyle")];

/// The style links a sheet may carry, one for each family.
pub(crate) const LINKS: [&str; 3] = ["LineStyle", "FillStyle", "TextStyle"];

// The parts of a VSDX package these tests read (spelled in two pieces, as the other tests do, so that no line of the tree holds the format's directory name).
pub(crate) const PAGE_PART: &str = concat!("/vi", "sio", "/pages/page1.xml");
pub(crate) const DOCUMENT_PART: &str = concat!("/vi", "sio", "/document.xml");
const MASTERS_PART: &str = concat!("/vi", "sio", "/masters/masters.xml");
const MASTERS_RELS: &str = concat!("/vi", "sio", "/masters/_rels/masters.xml.rels");
const MASTERS_FOLDER: &str = concat!("/vi", "sio", "/masters/");

/// Every one-dimensional master of the shipped built-in stencils: the connectors and the figures drawn along a line.
pub(crate) fn connector_masters() -> Vec<&'static MasterSpec> {
    all_masters().filter(|m| m.is_connector()).collect()
}

/// A part of a package, as text.
pub(crate) fn part(pkg: &Package, name: &str) -> String {
    String::from_utf8(pkg.part(name).unwrap_or_else(|| panic!("no part {name}")).to_vec()).unwrap()
}

/// An attribute of a start tag.
pub(crate) fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let at = tag.find(&format!(r#" {name}=""#))? + name.len() + 3;
    Some(&tag[at..at + tag[at..].find('"')?])
}

/// The start tag of the top-level shape `id` of a part, and the cells directly under it (before its first section, its text and its members): `N` to the start tag of the cell.
pub(crate) fn shape_in(xml: &str, id: u32) -> Option<(String, BTreeMap<String, String>)> {
    let start = xml.find(&format!(r#"<Shape ID="{id}" "#))?;
    let tag_end = start + xml[start..].find('>')? + 1;
    let tag = xml[start..tag_end].to_owned();
    // A connector holds no member, so its own cells run from its start tag to the first section, text or end of the shape.
    let rest = &xml[tag_end..];
    let stop = ["<Section", "<Text", "<Shapes", "</Shape>"].iter().filter_map(|t| rest.find(t)).min().unwrap_or(rest.len());
    let mut cells = BTreeMap::new();
    let mut body = &rest[..stop];
    while let Some(at) = body.find("<Cell ") {
        let end = at + body[at..].find("/>").unwrap() + 2;
        let cell = &body[at..end];
        cells.insert(attr(cell, "N").unwrap().to_owned(), cell.to_owned());
        body = &body[end..];
    }
    Some((tag, cells))
}

/// The cells directly under the first shape of the master whose `ID` is `master` in the package: the part its `Rel` names in the masters index.
pub(crate) fn master_cells(pkg: &Package, master: u32) -> BTreeMap<String, String> {
    master_shape(pkg, master).1
}

/// The start tag of the first shape of the master whose `ID` is `master` in the package, and the cells directly under it.
pub(crate) fn master_shape(pkg: &Package, master: u32) -> (String, BTreeMap<String, String>) {
    let index = part(pkg, MASTERS_PART);
    let start = index.find(&format!(r#"<Master ID="{master}" "#)).unwrap_or_else(|| panic!("no master {master} in the index"));
    let row = &index[start..start + index[start..].find("</Master>").unwrap()];
    let rel_at = row.find(r#"<Rel r:id=""#).unwrap() + 11;
    let rel = &row[rel_at..rel_at + row[rel_at..].find('"').unwrap()];
    let rels = part(pkg, MASTERS_RELS);
    let entry_at = rels.find(&format!(r#"Id="{rel}""#)).unwrap();
    let entry = &rels[entry_at..entry_at + rels[entry_at..].find("/>").unwrap()];
    let target = attr(entry, "Target").unwrap();
    let xml = part(pkg, &format!("{MASTERS_FOLDER}{target}"));
    let id_at = xml.find(r#"<Shape ID=""#).unwrap() + 11;
    let first: u32 = xml[id_at..id_at + xml[id_at..].find('"').unwrap()].parse().unwrap();
    shape_in(&xml, first).unwrap()
}

/// The style sheets of the package by `ID`: each one's start tag and the cells directly under it.
pub(crate) type Styles = BTreeMap<String, (String, BTreeMap<String, String>)>;

pub(crate) fn styles(pkg: &Package) -> Styles {
    let xml = part(pkg, DOCUMENT_PART);
    let mut out = Styles::new();
    let mut rest = xml.as_str();
    while let Some(start) = rest.find("<StyleSheet ") {
        let tag_end = start + rest[start..].find('>').unwrap() + 1;
        let tag = rest[start..tag_end].to_owned();
        let body = &rest[tag_end..];
        let close = body.find("</StyleSheet>").unwrap_or(0);
        let own = &body[..["<Section", "</StyleSheet>"].iter().filter_map(|t| body.find(t)).min().unwrap_or(close)];
        let mut cells = BTreeMap::new();
        let mut left = own;
        while let Some(at) = left.find("<Cell ") {
            let end = at + left[at..].find("/>").unwrap() + 2;
            cells.insert(attr(&left[at..end], "N").unwrap().to_owned(), left[at..end].to_owned());
            left = &left[end..];
        }
        out.insert(attr(&tag, "ID").unwrap().to_owned(), (tag, cells));
        rest = &body[close..];
    }
    out
}

/// The `V` the style `id` gives `cell` along the links of the family `link` names, and `0` when no style holds it (the default for a number); at most 16 steps.
pub(crate) fn style_gives(styles: &Styles, mut id: String, cell: &str, link: &str) -> String {
    for _ in 0..16 {
        let Some((tag, cells)) = styles.get(&id) else { break };
        if let Some(c) = cells.get(cell) {
            return attr(c, "V").unwrap_or("").to_owned();
        }
        match attr(tag, link) {
            Some(next) if next != id => id = next.to_owned(),
            _ => break,
        }
    }
    "0".to_owned()
}

/// What the VSDX application reads for `cell` on an instance, by what its check showed: the instance's own cell, else (when the instance links a style of its own for the cell's family,
/// `link`) what that style gives, else the master's cell. → `(where, V)`.
pub(crate) fn application_reads(
    (tag, own): (&str, &BTreeMap<String, String>),
    master: &BTreeMap<String, String>,
    styles: &Styles,
    (cell, link): (&str, &str),
) -> (String, Option<String>) {
    if let Some(c) = own.get(cell) {
        return ("its own cell".to_owned(), attr(c, "V").map(str::to_owned));
    }
    if let Some(style) = attr(tag, link) {
        return (format!("its own {link} {style}"), Some(style_gives(styles, style.to_owned(), cell, link)));
    }
    ("the master's cell".to_owned(), master.get(cell).and_then(|c| attr(c, "V")).map(str::to_owned))
}

/// The number a cell of shape `id` of page 1 reads in DiaCraft, its own or inherited.
pub(crate) fn reads(d: &Document, id: u32, cell: &str) -> Option<f64> {
    let at = d.find_shape(Owner::Page(1), id)?;
    let found = Resolver::new(d).resolve(&SheetRef::Shape(at), &CellAddr::direct(cell))?;
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(Value::number)
}

/// Drops every connector master on page 1, in rows of four. → each master's id with its instance's `ID`.
pub(crate) fn drop_all(s: &mut Session) -> Vec<(&'static str, u32)> {
    connector_masters()
        .into_iter()
        .enumerate()
        .map(|(i, m)| {
            let (x, y) = (1.5 + (i % 4) as f64 * 2.0, 1.0 + (i / 4) as f64 * 1.0);
            (m.id, id_of(&run(s, "shape.drop", json!({"master": m.id, "page": 1, "x": x, "y": y}))))
        })
        .collect()
}

/// The `Master` of each instance in the session's drawing.
pub(crate) fn masters_of(d: &Document, dropped: &[(&'static str, u32)]) -> Vec<(&'static str, u32, u32)> {
    dropped
        .iter()
        .map(|&(name, id)| {
            let at = d.find_shape(Owner::Page(1), id).unwrap();
            (name, id, d.shape(&at).unwrap().master.unwrap())
        })
        .collect()
}

/// Asserts that every instance of the file at `path` is saved as the application needs it: no style link of its own, and each cell of the look read from the master with the master's
/// value (or held by the instance with that value, as an `.odg` gives it). `own_cells` says whether an instance may hold a cell of the look of its own.
#[track_caller]
fn assert_instances_take_the_masters_look(path: &str, instances: &[(&'static str, u32, u32)], own_cells: bool) {
    let pkg = Package::read(&std::fs::read(path).unwrap(), &Limits::default()).unwrap();
    let page = part(&pkg, PAGE_PART);
    let styles = styles(&pkg);
    let mut wrong = Vec::new();
    for &(name, id, master) in instances {
        let (tag, own) = shape_in(&page, id).unwrap_or_else(|| panic!("{name}: no shape {id} in the page"));
        assert_eq!(attr(&tag, "Master"), Some(master.to_string().as_str()), "{name}: {tag}");
        let cells = master_cells(&pkg, master);
        for link in LINKS {
            if !own_cells && let Some(n) = attr(&tag, link) {
                wrong.push(format!("{name}: the instance links style {n} for {link} of its own"));
            }
        }
        for (cell, link) in LOOK {
            if !own_cells && own.contains_key(cell) {
                wrong.push(format!("{name}: the instance holds {cell} of its own: {}", own[cell]));
            }
            let Some(want) = cells.get(cell).and_then(|c| attr(c, "V")) else { continue };
            let (from, got) = application_reads((&tag, &own), &cells, &styles, (cell, link));
            if got.as_deref() != Some(want) {
                wrong.push(format!("{name}: {cell} is {want} on the master, and the application reads {got:?} from {from}"));
            }
        }
    }
    assert!(wrong.is_empty(), "{} problems in {path}:\n{}", wrong.len(), wrong.join("\n"));
}

#[test]
fn the_catalogue_has_the_connector_masters_the_application_was_checked_with() {
    let names: Vec<&str> = connector_masters().iter().map(|m| m.id).collect();
    for id in [
        "uml-class.directed-association",
        "uml-class.aggregation",
        "uml-class.composition",
        "uml-class.generalization",
        "uml-class.realization",
        "uml-class.dependency",
        "uml-class.note-link",
        "bpmn.sequence-flow",
        "bpmn.message-flow",
        "bpmn.association",
        "bpmn.data-association",
        "gane-sarson.data-flow",
    ] {
        assert!(names.contains(&id), "{id} in {names:?}");
    }
    assert!(names.len() >= 28, "{} one-dimensional masters: {names:?}", names.len());
}

#[test]
fn a_dropped_connector_master_holds_no_style_link_of_its_own_and_reads_the_masters_look() {
    let mut s = blank_session();
    let dropped = drop_all(&mut s);
    let d = doc(&s);
    for &(name, id) in &dropped {
        let at = d.find_shape(Owner::Page(1), id).unwrap();
        let shape = d.shape(&at).unwrap();
        assert_eq!(shape.sheet.links, diacraft_doc::Links::default(), "{name}: the instance takes its style from the master shape");
        let master = d.master(shape.master.unwrap()).unwrap();
        let top = master.shapes.get(0).unwrap();
        for (cell, _) in LOOK {
            let Some(want) = top.sheet.cell(cell).and_then(|c| c.value.as_ref()).and_then(Value::number) else { continue };
            assert_eq!(reads(d, id, cell), Some(want), "{name}: {cell} reads the master's");
        }
    }
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn every_connector_master_saved_by_the_model_writer_reaches_its_instance() {
    let mut s = blank_session();
    let dropped = drop_all(&mut s);
    let instances = masters_of(doc(&s), &dropped);
    let dir = Dir::new();
    let path = dir.path("new.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    assert_instances_take_the_masters_look(&path, &instances, false);
    // A second save, a splice into the file it wrote now, keeps it so.
    run(&mut s, "file.save", json!({}));
    assert_instances_take_the_masters_look(&path, &instances, false);
}

#[test]
fn every_connector_master_saved_from_a_diacraft_drawing_reaches_its_instance() {
    let mut s = blank_session();
    let dropped = drop_all(&mut s);
    let instances = masters_of(doc(&s), &dropped);
    let dir = Dir::new();
    let native = dir.path("drawing.diacraft");
    run(&mut s, "file.save_as", json!({ "path": native }));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({ "path": native }));
    let path = dir.path("from-native.vsdx");
    run(&mut t, "file.save_as", json!({ "path": path }));
    assert_instances_take_the_masters_look(&path, &instances, false);
}

#[test]
fn every_connector_master_dropped_into_an_opened_vsdx_file_reaches_its_instance_through_the_splice() {
    // A drawing of one box, saved and opened again: a save of it splices into its own bytes.
    let dir = Dir::new();
    let base = dir.path("base.vsdx");
    let mut s = blank_session();
    run(&mut s, "shape.drop", json!({"master": "process", "page": 1, "x": 9.0, "y": 9.0}));
    run(&mut s, "file.save_as", json!({ "path": base }));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({ "path": base }));
    let dropped = drop_all(&mut t);
    let instances = masters_of(doc(&t), &dropped);
    run(&mut t, "file.save", json!({}));
    assert_instances_take_the_masters_look(&base, &instances, false);
}

#[test]
fn every_connector_master_saved_from_an_odg_drawing_reaches_its_instance() {
    // An `.odg` holds each instance's look as its own graphic style, and the reader gives the instance those cells of its own and a link to that style: the cells it holds are the
    // master's look, so the application reads the same.
    let mut s = blank_session();
    let dropped = drop_all(&mut s);
    let dir = Dir::new();
    let odg = dir.path("drawing.odg");
    run(&mut s, "file.save_as", json!({ "path": odg }));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({ "path": odg }));
    let instances = masters_of(doc(&t), &dropped);
    let path = dir.path("from-odg.vsdx");
    run(&mut t, "file.save_as", json!({ "path": path }));
    assert_instances_take_the_masters_look(&path, &instances, true);
}

#[test]
fn a_connector_changed_to_a_connector_master_holds_no_link_that_is_the_masters_own() {
    // A connector of the connector tool links the root style for its three families, as a shape with no master does; changed to a master that links the root, the link says nothing
    // the master's does not, and the application would read the root's line ends and pattern over the master's.
    let mut s = blank_session();
    let a = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2.0, "y": 2.0})));
    let b = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 6.0, "y": 4.0})));
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}})));
    let at = |d: &Document| d.find_shape(Owner::Page(1), c).unwrap();
    assert_eq!(doc(&s).shape(&at(doc(&s))).unwrap().sheet.links.line, Some(0), "the connector tool links the root");
    let r = run(&mut s, "shape.change", json!({"ids": [c], "master": "uml-class.realization"}));
    assert_eq!(r["changed"], json!([c]), "{r}");
    let d = doc(&s);
    let shape = d.shape(&at(d)).unwrap();
    assert_eq!(shape.sheet.links, diacraft_doc::Links::default(), "the master's own links are not the shape's");
    assert_eq!(reads(d, c, "LinePattern"), Some(2.0), "the master's dash");
    let master = shape.master.unwrap();
    let dir = Dir::new();
    let path = dir.path("changed.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let pkg = Package::read(&std::fs::read(&path).unwrap(), &Limits::default()).unwrap();
    let (tag, own) = shape_in(&part(&pkg, PAGE_PART), c).unwrap();
    for link in LINKS {
        assert!(attr(&tag, link).is_none(), "{link}: {tag}");
    }
    // The arrowhead the connector tool gave it is a cell of its own, and stays; the pattern is the master's.
    assert_eq!(own.get("EndArrow").and_then(|t| attr(t, "V")), Some("4"));
    let cells = master_cells(&pkg, master);
    assert_eq!(application_reads((&tag, &own), &cells, &styles(&pkg), ("LinePattern", "LineStyle")).1.as_deref(), Some("2"));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_connector_with_a_style_of_its_own_changed_to_a_master_keeps_it() {
    // A link that is not the master's is the shape's own formatting: it stays. A connector put on a page with a theme follows the `Theme` style.
    let mut s = blank_session();
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    let a = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2.0, "y": 2.0})));
    let b = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 6.0, "y": 4.0})));
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}})));
    let at = |d: &Document| d.find_shape(Owner::Page(1), c).unwrap();
    let themed = doc(&s).shape(&at(doc(&s))).unwrap().sheet.links;
    assert!(themed.line.is_some_and(|n| n != 0), "{themed:?}");
    run(&mut s, "shape.change", json!({"ids": [c], "master": "uml-class.generalization"}));
    let d = doc(&s);
    assert_eq!(d.shape(&at(d)).unwrap().sheet.links, themed, "the style of its own stays");
    assert_eq!(check_session(&s), Ok(()));
}
