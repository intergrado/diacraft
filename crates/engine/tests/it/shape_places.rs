//! **The commands that delete, add or reorder shapes keep the place of the unknown nodes of a `Shapes` list**, a page's and a group's.
//!
//! The model keeps an unknown node of a `Shapes` element (a foreign element, a comment between the shapes) as the count of shapes that stood before it, and a model writer puts it back
//! after that many (`sheet_places.rs` has the same for the rows and the sections of a sheet). A command that takes a shape out, puts one in or moves one has to move the count with
//! it, or the node stands one place off in the file a model writer makes (a `.diacraft` file saved as a VSDX file): **a node goes on following the shape it followed, a node whose shape
//! is deleted goes with the shape before it, a new shape goes ahead of a node that stood after the shape before it, and a node before the first shape stays before it.**
//!
//! An outline lists the children of a `Shapes` element in order: a shape as `s<ID>` (with its own members in braces when it holds a `Shapes` element), an unknown element as
//! `?Name` and a comment as `#text`.

use diacraft_container::xml::{El, Node};
use diacraft_engine::Session;
use diacraft_testkit::package::{PageSpec, VsdxBuilder};
use serde_json::{Value, json};

use crate::odg_build::Dir;
use crate::sheet_files_world::parts_of;

const X: &str = r#"xmlns:x="urn:test:unknown""#;

fn box_shape(id: u32) -> String {
    format!(
        r#"<Shape ID="{id}" Type="Shape" NameU="B{id}" Name="B{id}"><Cell N="PinX" V="{id}"/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/></Shape>"#
    )
}

fn shapes() -> String {
    format!(
        r##"
<x:S0 {X}/>
{one}
<!-- after one -->
{two}
<x:S2 {X}/>
{three}
<x:S3 {X}/>
<Shape ID="6" Type="Group" NameU="G" Name="G">
  <Cell N="PinX" V="6"/><Cell N="PinY" V="2"/><Cell N="Width" V="3"/><Cell N="Height" V="1"/>
  <Shapes>
    <x:G0 {X}/>
    {seven}
    <x:G1 {X}/>
    {eight}
    <x:G2 {X}/>
  </Shapes>
</Shape>
<x:S6 {X}/>"##,
        one = box_shape(1),
        two = box_shape(2),
        three = box_shape(3),
        seven = box_shape(7),
        eight = box_shape(8),
    )
}

/// The drawing as a VSDX file.
fn package() -> Vec<u8> {
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&shapes())).build()
}

// ------------------------------------------------------------------------------------------------------------------------------ outlines

/// The children of a `Shapes` element.
fn list(el: &El) -> Vec<String> {
    let mut out = Vec::new();
    for node in &el.children {
        match node {
            Node::El(child) if child.is("Shape") => {
                let id = child.attr("ID").unwrap_or("");
                let members: Vec<String> = child.elements().filter(|c| c.is("Shapes")).flat_map(list).collect();
                if members.is_empty() {
                    out.push(format!("s{id}"));
                } else {
                    out.push(format!("s{id}{{{}}}", members.join(",")));
                }
            }
            Node::El(child) => out.push(format!("?{}", child.local())),
            Node::Comment(text) => out.push(format!("#{}", text.trim())),
            Node::Text(_) | Node::CData(_) | Node::Pi { .. } => {}
        }
    }
    out
}

/// The outline of the first page's `Shapes` element in a package.
fn outline(package: &[u8]) -> Vec<String> {
    let parts = parts_of(package);
    let bytes = parts.iter().find(|(name, _)| name.ends_with("/pages/page1.xml")).map(|(_, bytes)| bytes).expect("the package has no first page");
    let root = diacraft_opc::xml::parse(bytes).unwrap();
    root.elements().filter(|e| e.is("Shapes")).flat_map(list).collect()
}

fn words(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

// -------------------------------------------------------------------------------------------------------------------------------- cases

fn call(s: &mut Session, command: &str, params: Value) {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command} {params}: {e}"));
}

struct Case {
    name: &'static str,
    run: fn(&mut Session),
    steps: usize,
    expect: Vec<String>,
}

/// The page before any command.
fn before() -> Vec<String> {
    words(&["?S0", "s1", "#after one", "s2", "?S2", "s3", "?S3", "s6{?G0,s7,?G1,s8,?G2}", "?S6"])
}

fn cases() -> Vec<Case> {
    let mut all = Vec::new();
    let mut case = |name: &'static str, steps: usize, run: fn(&mut Session), expect: &[&str]| {
        all.push(Case { name, run, steps, expect: words(expect) });
    };
    case(
        "delete a shape in the middle: its node goes with the shape before it",
        1,
        |s| call(s, "shape.delete", json!({"ids": [2]})),
        &["?S0", "s1", "#after one", "?S2", "s3", "?S3", "s6{?G0,s7,?G1,s8,?G2}", "?S6"],
    );
    case(
        "delete the first shape: the node that followed it leads",
        1,
        |s| call(s, "shape.delete", json!({"ids": [1]})),
        &["?S0", "#after one", "s2", "?S2", "s3", "?S3", "s6{?G0,s7,?G1,s8,?G2}", "?S6"],
    );
    case(
        "delete the last shape: the node that followed it goes with the shape before",
        1,
        |s| call(s, "shape.delete", json!({"ids": [6]})),
        &["?S0", "s1", "#after one", "s2", "?S2", "s3", "?S3", "?S6"],
    );
    case(
        "delete two shapes",
        1,
        |s| call(s, "shape.delete", json!({"ids": [1, 3]})),
        &["?S0", "#after one", "s2", "?S2", "?S3", "s6{?G0,s7,?G1,s8,?G2}", "?S6"],
    );
    case(
        "delete a member of a group: the nodes of the group's list follow the same rule",
        1,
        |s| call(s, "shape.delete", json!({"ids": [7]})),
        &["?S0", "s1", "#after one", "s2", "?S2", "s3", "?S3", "s6{?G0,?G1,s8,?G2}", "?S6"],
    );
    case(
        "draw a shape: it goes ahead of the node that trailed the shapes",
        1,
        |s| call(s, "shape.draw", json!({"page": 1, "tool": "rectangle", "points": [[1.0, 1.0], [2.0, 2.0]]})),
        &["?S0", "s1", "#after one", "s2", "?S2", "s3", "?S3", "s6{?G0,s7,?G1,s8,?G2}", "s9", "?S6"],
    );
    case(
        "bring a shape to the front: its node goes with it",
        1,
        |s| call(s, "shape.order", json!({"ids": [1], "to": "front"})),
        &["?S0", "s2", "?S2", "s3", "?S3", "s6{?G0,s7,?G1,s8,?G2}", "?S6", "s1", "#after one"],
    );
    case(
        "send a shape to the back: its node goes with it",
        1,
        |s| call(s, "shape.order", json!({"ids": [3], "to": "back"})),
        &["?S0", "s3", "?S3", "s1", "#after one", "s2", "?S2", "s6{?G0,s7,?G1,s8,?G2}", "?S6"],
    );
    case(
        "bring a member of a group to the front: the group's nodes follow the same rule",
        1,
        |s| call(s, "shape.order", json!({"ids": [7], "to": "front"})),
        &["?S0", "s1", "#after one", "s2", "?S2", "s3", "?S3", "s6{?G0,s8,?G2,s7,?G1}", "?S6"],
    );
    all
}

// ---------------------------------------------------------------------------------------------------------------------------------- trips

fn open(path: &str) -> Session {
    let mut s = Session::new();
    s.execute("file.open", &json!({ "path": path })).unwrap_or_else(|e| panic!("the file does not open: {e}"));
    s
}

fn save_as(s: &mut Session, path: &str) {
    s.execute("file.save_as", &json!({ "path": path })).unwrap_or_else(|e| panic!("the save as {path}: {e}"));
}

fn check(case: &Case, expect: &[String], got: &[u8], what: &str, failures: &mut Vec<String>) {
    let seen = outline(got);
    if seen != expect {
        failures.push(format!("[{what}] {}:\n      want {expect:?}\n      got  {seen:?}", case.name));
    }
}

fn report(failures: Vec<String>, total: usize) {
    assert!(failures.is_empty(), "{} of {total} trips failed:\n{}", failures.len(), failures.join("\n"));
}

/// The drawing is what its outline says before any command, in the file and in the one a model writer makes of it.
#[test]
fn the_drawing_stands_as_its_outline_says_in_the_file_and_in_what_a_model_writer_makes_of_it() {
    let dir = Dir::new();
    let vsdx = dir.write("a.vsdx", &package());
    assert_eq!(outline(&package()), before());
    let mut s = open(&vsdx);
    let native = dir.path("a.diacraft");
    save_as(&mut s, &native);
    let mut s = open(&native);
    let out = dir.path("b.vsdx");
    save_as(&mut s, &out);
    assert_eq!(outline(&dir.read("b.vsdx")), before());
}

/// A `.diacraft` file, opened, changed, and saved as a VSDX file: the node stands after the shape it followed.
#[test]
fn a_model_writer_puts_each_unknown_node_after_the_shape_it_followed_in_a_diacraft_file_saved_as_vsdx() {
    let mut failures = Vec::new();
    let cases = cases();
    for case in &cases {
        let dir = Dir::new();
        let vsdx = dir.write("a.vsdx", &package());
        let native = dir.path("a.diacraft");
        save_as(&mut open(&vsdx), &native);
        let mut s = open(&native);
        (case.run)(&mut s);
        let out = dir.path("b.vsdx");
        save_as(&mut s, &out);
        check(case, &case.expect, &dir.read("b.vsdx"), ".diacraft to .vsdx", &mut failures);
    }
    report(failures, cases.len());
}

/// The same edit saved in the `.diacraft` file, which is then opened and saved as a VSDX file.
#[test]
fn a_diacraft_file_keeps_the_place_of_each_node_among_the_shapes_through_a_save_and_a_reopen() {
    let mut failures = Vec::new();
    let cases = cases();
    for case in &cases {
        let dir = Dir::new();
        let vsdx = dir.write("a.vsdx", &package());
        let native = dir.path("a.diacraft");
        save_as(&mut open(&vsdx), &native);
        let mut s = open(&native);
        (case.run)(&mut s);
        s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("{}: the save: {e}", case.name));
        let mut again = open(&native);
        let out = dir.path("b.vsdx");
        save_as(&mut again, &out);
        check(case, &case.expect, &dir.read("b.vsdx"), ".diacraft saved, opened, saved as .vsdx", &mut failures);
    }
    report(failures, cases.len());
}

/// A VSDX drawing changed and saved as a `.diacraft` file, which is opened and saved as a VSDX file: a Save As across the formats and back.
#[test]
fn a_save_as_across_the_formats_keeps_the_place_of_each_node_among_the_shapes() {
    let mut failures = Vec::new();
    let cases = cases();
    for case in &cases {
        let dir = Dir::new();
        let vsdx = dir.write("a.vsdx", &package());
        let mut s = open(&vsdx);
        (case.run)(&mut s);
        let native = dir.path("b.diacraft");
        save_as(&mut s, &native);
        let mut again = open(&native);
        let out = dir.path("c.vsdx");
        save_as(&mut again, &out);
        check(case, &case.expect, &dir.read("c.vsdx"), ".vsdx to .diacraft to .vsdx", &mut failures);
    }
    report(failures, cases.len());
}

/// An undo puts every node back where it stood, in what a model writer makes.
#[test]
fn an_undo_puts_each_node_among_the_shapes_back_where_it_stood() {
    let mut failures = Vec::new();
    let cases = cases();
    for case in &cases {
        let dir = Dir::new();
        let vsdx = dir.write("a.vsdx", &package());
        let native = dir.path("a.diacraft");
        save_as(&mut open(&vsdx), &native);
        let mut s = open(&native);
        (case.run)(&mut s);
        for _ in 0..case.steps {
            s.execute("edit.undo", &json!({})).unwrap_or_else(|e| panic!("{}: the undo: {e}", case.name));
        }
        let out = dir.path("undone.vsdx");
        save_as(&mut s, &out);
        check(case, &before(), &dir.read("undone.vsdx"), "after the undo", &mut failures);
    }
    report(failures, cases.len());
}

// ----------------------------------------------------------------------------------------------------------------------------------- costs

/// **Counted, not timed.** The commands that put a shape in, take one out or move one read the unknown nodes of a page of thousands of shapes once each, and the keys of the shapes once
/// each only where the list is rebuilt (a change of the drawing order, group and ungroup): a delete and a drawn shape read no key, and none reads a pass of the list for each node.
#[test]
fn a_command_that_changes_a_page_of_thousands_of_shapes_reads_the_nodes_and_the_keys_once_each() {
    const SHAPES: u32 = 3_000;
    let list: String = (1..=SHAPES).map(|id| format!("{}<!--after {id}-->", box_shape(id))).collect();
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&list)).build();
    let dir = Dir::new();
    let mut s = open(&dir.write("many.vsdx", &bytes));
    // A delete: the nodes once, no key.
    diacraft_doc::work::reset();
    call(&mut s, "shape.delete", json!({"ids": [10]}));
    let (keys, nodes) = (diacraft_doc::work::follow_keys(), diacraft_doc::work::places_read());
    assert_eq!(keys, 0, "a delete reads no key");
    assert!(nodes <= 2 * u64::from(SHAPES), "the nodes of the list were read {nodes} times for one delete: once for the shape taken out");
    // A shape drawn: the same.
    diacraft_doc::work::reset();
    call(&mut s, "shape.draw", json!({"page": 1, "tool": "rectangle", "points": [[1.0, 1.0], [2.0, 2.0]]}));
    assert_eq!(diacraft_doc::work::follow_keys(), 0, "a drawn shape reads no key");
    assert!(diacraft_doc::work::places_read() <= 2 * u64::from(SHAPES));
    // A change of the drawing order rebuilds the list: the keys of the old and the new list once each, and the nodes once.
    diacraft_doc::work::reset();
    call(&mut s, "shape.order", json!({"ids": [20], "to": "front"}));
    let keys = diacraft_doc::work::follow_keys();
    assert!(keys <= 2 * u64::from(SHAPES) + 2, "the keys were read {keys} times for one change of the order of {SHAPES} shapes");
    assert!(diacraft_doc::work::places_read() <= 2 * u64::from(SHAPES));
}
